//! `Tests/ttLib/tables/_g_l_y_f_test.py`.
//!
//! fontoxide keys glyphs by glyph ID, so the `glyphSet` dicts of the upstream
//! tests become a [`Glyf`] table plus a [`GlyphOrder`]. `TTGlyphPen` glyphs with
//! components are built with [`component`], which does what the pen does for a
//! kept component. "Expanding" a glyph is dropping its compact `data`, after
//! which the parsed fields are what gets compiled.

#![allow(clippy::unwrap_used)]

mod common;

use std::collections::{BTreeSet, HashMap};

use common::{TestResult, fake_font, names, parse_xml, tables_data, upstream, writer_lines};
use fontoxide_misc::fixed_tools::float_to_fixed_to_float;
use fontoxide_misc::round_tools::ot_round;
use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_pens::recording_pen::{PenOp, PointOp};
use fontoxide_pens::{Pen, PointToSegmentPen, RecordingPen, RecordingPointPen, SegmentType};
use fontoxide_ttlib::beyond64k::{ConvertOptions, upper_tables};
use fontoxide_ttlib::pens::{GlyphOptions, TtGlyphPen};
use fontoxide_ttlib::tables::glyf::{
	self, ComponentPlacement, FLAG_CUBIC, FLAG_ON_CURVE, Glyph, GlyphComponent, Number,
	drop_implied_on_curve_points,
};
use fontoxide_ttlib::tables::tt_program::Program;
use fontoxide_ttlib::tables::{Glyf, Head, Hmtx, Loca, Maxp};
use fontoxide_ttlib::{
	Font, GlyphOrder, ReadOptions, TableKey, TableType, Tag, TtxImportOptions, TtxOptions,
	WriteOptions,
};

const ARGS_ARE_XY_VALUES: i64 = 0x0002;
const WE_HAVE_A_SCALE: i64 = 0x0008;
const WE_HAVE_AN_X_AND_Y_SCALE: i64 = 0x0040;
const WE_HAVE_A_TWO_BY_TWO: i64 = 0x0080;
const SCALED_COMPONENT_OFFSET: i64 = 0x0800;
const UNSCALED_COMPONENT_OFFSET: i64 = 0x1000;

const GLYF: Tag = Tag::new(b"glyf");
const GLYF_UPPER: Tag = Tag::new(b"GLYF");
const HEAD: Tag = Tag::new(b"head");

type Coords = Vec<(f64, f64)>;

fn order(list: &[&str]) -> GlyphOrder {
	GlyphOrder::new(names(list))
}

fn error_text<T: std::fmt::Debug>(result: fontoxide_ttlib::Result<T>) -> String {
	result.expect_err("an error").to_string()
}

/// `TTGlyphPen(None)` drawing closed contours of line segments, then `glyph()`.
fn contours_glyph(contours: &[&[(f64, f64)]]) -> Glyph {
	let mut pen = TtGlyphPen::new(None);
	for contour in contours {
		pen.move_to(contour[0]).unwrap();
		for &p in &contour[1..] {
			pen.line_to(p).unwrap();
		}
		pen.close_path().unwrap();
	}
	pen.glyph(&GlyphOrder::default()).unwrap()
}

/// `TTGlyphPen(None).glyph()`: an empty glyph.
fn empty_glyph() -> Glyph {
	contours_glyph(&[])
}

/// A component as `TTGlyphPen.addComponent(name, transformation)` keeps it:
/// the transformation quantized to F2Dot14, the offset rounded, and
/// `ROUND_XY_TO_GRID` set.
fn component(gid: u32, t: [f64; 6]) -> GlyphComponent {
	let m = [t[0], t[1], t[2], t[3]].map(|v| float_to_fixed_to_float(v, 14));
	GlyphComponent {
		glyph: gid,
		flags: 0x04,
		placement: ComponentPlacement::Offset {
			x: Number::Int(ot_round(t[4])),
			y: Number::Int(ot_round(t[5])),
		},
		transform: (m != [1.0, 0.0, 0.0, 1.0]).then_some([[m[0], m[1]], [m[2], m[3]]]),
	}
}

const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn translation(dx: f64, dy: f64) -> [f64; 6] {
	[1.0, 0.0, 0.0, 1.0, dx, dy]
}

/// A composite glyph, as `TTGlyphPen.glyph()` makes it.
fn composite(components: Vec<GlyphComponent>) -> Glyph {
	Glyph {
		number_of_contours: -1,
		components,
		..Default::default()
	}
}

/// A table of glyphs by glyph ID.
fn glyf_of(glyphs: Vec<Glyph>) -> Glyf {
	let mut glyf = Glyf::default();
	for (gid, g) in glyphs.into_iter().enumerate() {
		glyf.glyphs.insert(gid as u32, g);
	}
	glyf
}

/// A glyph with the given points, flags and contour ends, and an empty
/// program.
fn simple_glyph(coords: &[(f64, f64)], flags: &[u8], ends: &[i64]) -> Glyph {
	Glyph {
		number_of_contours: ends.len() as i64,
		coordinates: coords.to_vec(),
		flags: flags.to_vec(),
		end_pts_of_contours: ends.to_vec(),
		program: Some(Program::default()),
		..Default::default()
	}
}

/// `glyph.compile(table)` (with `recalcBBoxes`), for a table with the given tag
/// whose glyph order is `names` and where the glyph is the last one.
fn compile_in(glyph: &Glyph, tag: Tag, names: &[&str]) -> fontoxide_ttlib::Result<Vec<u8>> {
	let order = order(names);
	let mut glyf = Glyf::new_as(tag);
	let gid = names.len() as u32 - 1;
	glyf.glyphs.insert(gid, glyph.clone());
	glyf.glyph_data(gid, &order, true)
}

/// `Glyph(data)`: compact, with the parsed fields alongside.
fn compact_glyph(data: &[u8], num_glyphs: usize, extended: bool) -> Glyph {
	let mut glyph = Glyph::decompile_ext(data, num_glyphs, extended).unwrap();
	glyph.data = Some(data.to_vec());
	glyph
}

fn draw(glyph: &Glyph) -> Vec<PenOp> {
	let mut pen = RecordingPen::new();
	glyph.draw(&GlyphOrder::default(), &mut pen, 0.0).unwrap();
	pen.value
}

fn strip_ttlib_version(text: &str) -> String {
	let mut out = String::new();
	let mut rest = text;
	while let Some(at) = rest.find(" ttLibVersion=\"") {
		out.push_str(&rest[..at]);
		let after = &rest[at + " ttLibVersion=\"".len()..];
		rest = &after[after.find('"').unwrap() + 1..];
	}
	out.push_str(rest);
	out
}

fn ttx_lines(text: &str) -> Vec<String> {
	strip_ttlib_version(text)
		.lines()
		.map(str::to_owned)
		.collect()
}

fn dump(font: &mut Font, tables: Vec<TableKey>) -> String {
	font.to_ttx(&TtxOptions {
		tables,
		write_version: false,
		..Default::default()
	})
	.unwrap()
}

const GLYF_TTX: &str = "_g_l_y_f_outline_flag_bit6.ttx";
const INST_TTX: &str = "_g_l_y_f_instructions.ttx";

fn read_data(name: &str) -> Vec<u8> {
	std::fs::read(tables_data(name)).unwrap()
}

fn glyf_ttx_font() -> Font {
	Font::from_ttx(&std::fs::read_to_string(tables_data(GLYF_TTX)).unwrap()).unwrap()
}

fn approx(a: f64, b: f64) -> bool {
	(a - b).abs() <= (1e-6 * b.abs()).max(1e-12)
}

mod glyph_coordinates_test {
	//! `GlyphCoordinatesTest`: coordinates are a `Vec<(f64, f64)>`, and the
	//! operators of `GlyphCoordinates` map onto [`glyf::translate`],
	//! [`glyf::scale`], [`glyf::transform`] and element-wise arithmetic. The
	//! `TypeError` checks for `object()` operands have no counterpart.

	#![allow(non_snake_case)]

	use super::*;

	fn add(a: &[(f64, f64)], b: &[(f64, f64)]) -> Coords {
		a.iter()
			.zip(b)
			.map(|(p, q)| (p.0 + q.0, p.1 + q.1))
			.collect()
	}

	fn sub(a: &[(f64, f64)], b: &[(f64, f64)]) -> Coords {
		a.iter()
			.zip(b)
			.map(|(p, q)| (p.0 - q.0, p.1 - q.1))
			.collect()
	}

	fn scaled(g: &[(f64, f64)], s: (f64, f64)) -> Coords {
		let mut g = g.to_vec();
		glyf::scale(&mut g, s);
		g
	}

	fn translated(g: &[(f64, f64)], d: (f64, f64)) -> Coords {
		let mut g = g.to_vec();
		glyf::translate(&mut g, d);
		g
	}

	#[test]
	fn test_translate() {
		let mut g: Coords = vec![(1.0, 2.0)];
		glyf::translate(&mut g, (0.5, 0.0));
		assert_eq!(g, [(1.5, 2.0)]);
	}

	#[test]
	fn test_scale() {
		let mut g: Coords = vec![(1.0, 2.0)];
		glyf::scale(&mut g, (0.5, 0.0));
		assert_eq!(g, [(0.5, 0.0)]);
	}

	#[test]
	fn test_transform() {
		let mut g: Coords = vec![(1.0, 2.0)];
		glyf::transform(&mut g, [[0.5, 0.0], [0.2, 0.5]]);
		assert_eq!(g[0], (0.9, 1.0));
	}

	#[test]
	fn test__eq__() {
		let g: Coords = vec![(1.0, 2.0)];
		let g2: Coords = vec![(1.0, 2.0)];
		let g3: Coords = vec![(1.5, 2.0)];
		assert!(g == g2);
		assert!(g != g3);
		assert!(g2 != g3);
	}

	#[test]
	fn test__ne__() {
		let g: Coords = vec![(1.0, 2.0)];
		let g2: Coords = vec![(1.0, 2.0)];
		let g3: Coords = vec![(1.5, 2.0)];
		assert!(!(g != g2));
		assert!(g != g3);
		assert!(g2 != g3);
	}

	#[test]
	fn test__pos__() {
		let g: Coords = vec![(1.0, 2.0)];
		let g2 = g.clone();
		assert_eq!(g, g2);
	}

	#[test]
	fn test__neg__() {
		let g: Coords = vec![(1.0, 2.0)];
		let g2 = scaled(&g, (-1.0, -1.0));
		assert_eq!(g2, [(-1.0, -2.0)]);
	}

	#[test]
	fn test__round__() {
		// `round(g)` rounds with otRound.
		let g: Coords = vec![(-1.5, 2.0)];
		let g2: Vec<(i64, i64)> = g.iter().map(|p| (ot_round(p.0), ot_round(p.1))).collect();
		assert_eq!(g2, [(-1, 2)]);
	}

	#[test]
	fn test__add__() {
		let g1: Coords = vec![(1.0, 2.0)];
		let g2: Coords = vec![(3.0, 4.0)];
		assert_eq!(add(&g1, &g2), [(4.0, 6.0)]);
		assert_eq!(translated(&g1, (1.0, 1.0)), [(2.0, 3.0)]);
	}

	#[test]
	fn test__sub__() {
		let g1: Coords = vec![(1.0, 2.0)];
		let g2: Coords = vec![(3.0, 4.0)];
		assert_eq!(sub(&g1, &g2), [(-2.0, -2.0)]);
		assert_eq!(translated(&g1, (-1.0, -1.0)), [(0.0, 1.0)]);
	}

	#[test]
	fn test__rsub__() {
		// `other + (-self)`.
		let g: Coords = vec![(1.0, 2.0)];
		assert_eq!(
			translated(&scaled(&g, (-1.0, -1.0)), (1.0, 1.0)),
			[(0.0, -1.0)]
		);
	}

	#[test]
	fn test__mul__() {
		let g: Coords = vec![(1.0, 2.0)];
		assert_eq!(scaled(&g, (3.0, 3.0)), [(3.0, 6.0)]);
		assert_eq!(scaled(&g, (3.0, 2.0)), [(3.0, 4.0)]);
		assert_eq!(scaled(&g, (1.0, 1.0)), g);
	}

	#[test]
	fn test__truediv__() {
		// Division scales by the reciprocals.
		let g: Coords = vec![(1.0, 2.0)];
		assert_eq!(scaled(&g, (1.0 / 2.0, 1.0 / 2.0)), [(0.5, 1.0)]);
		assert_eq!(scaled(&g, (1.0 / 1.0, 1.0 / 2.0)), [(1.0, 1.0)]);
		assert_eq!(scaled(&g, (1.0, 1.0)), g);
	}

	#[test]
	fn test__iadd__() {
		let mut g: Coords = vec![(1.0, 2.0)];
		glyf::translate(&mut g, (0.5, 0.0));
		assert_eq!(g, [(1.5, 2.0)]);
		let g = add(&g, &[(3.0, 4.0)]);
		assert_eq!(g, [(4.5, 6.0)]);
	}

	#[test]
	fn test__isub__() {
		let mut g: Coords = vec![(1.0, 2.0)];
		glyf::translate(&mut g, (-0.5, -0.0));
		assert_eq!(g, [(0.5, 2.0)]);
		let g = sub(&g, &[(3.0, 4.0)]);
		assert_eq!(g, [(-2.5, -2.0)]);
	}

	#[test]
	fn test__itruediv__() {
		let mut g: Coords = vec![(1.0, 3.0)];
		glyf::scale(&mut g, (1.0 / 0.5, 1.0 / 1.5));
		glyf::scale(&mut g, (1.0 / 2.0, 1.0 / 2.0));
		assert_eq!(g, [(1.0, 1.0)]);
	}

	#[test]
	fn test__bool__() {
		// Truthiness is whether there are points, whatever their values.
		let g: Coords = vec![];
		assert!(g.is_empty());
		let g: Coords = vec![(0.0, 0.0), (0.0, 0.0)];
		assert!(!g.is_empty());
		let g: Coords = vec![(0.0, 0.0), (1.0, 0.0)];
		assert!(!g.is_empty());
		let g: Coords = vec![(0.0, 0.5), (0.0, 0.0)];
		assert!(!g.is_empty());
	}

	#[test]
	fn test_double_precision_float() -> TestResult {
		// https://github.com/fonttools/fonttools/issues/963: coordinates are
		// doubles, so `toInt` rounds 242.50000000000003 up.
		let afloat = 242.500_000_000_000_03;
		assert_eq!(ot_round(afloat), 243);
		// The same when compiling, which rounds the coordinates.
		let glyph = simple_glyph(&[(afloat, 0.0)], &[FLAG_ON_CURVE], &[0]);
		let data = compile_in(&glyph, GLYF, &["a"])?;
		let reloaded = Glyph::decompile(&data, 1)?;
		assert_eq!(reloaded.coordinates, [(243.0, 0.0)]);
		Ok(())
	}

	#[test]
	fn test__check_float_overflow() {
		let mut g: Coords = vec![(1.0, 1.0)];
		g.push((f64::from(0x8000), 0.0));
		let flat: Vec<f64> = g.iter().flat_map(|p| [p.0, p.1]).collect();
		assert_eq!(flat, [1.0, 1.0, 32768.0, 0.0]);
	}
}

mod glyf_table_test {
	//! `GlyfTableTest`.

	use super::*;

	#[test]
	fn test_to_xml() -> TestResult {
		let mut font = Font::new();
		font.sfnt_version = Tag::new(b"\x00\x01\x00\x00");
		let maxp = Maxp::decompile(&read_data("_g_l_y_f_outline_flag_bit6.maxp.bin"), &mut font)?;
		font.insert(Box::new(maxp));
		let head = Head::decompile(&read_data("_g_l_y_f_outline_flag_bit6.head.bin"), &mut font)?;
		font.insert(Box::new(head));
		let loca = Loca::decompile(&read_data("_g_l_y_f_outline_flag_bit6.loca.bin"), &mut font)?;
		font.insert(Box::new(loca));
		let glyf = Glyf::decompile(&read_data("_g_l_y_f_outline_flag_bit6.glyf.bin"), &mut font)?;
		font.insert(Box::new(glyf));
		let expected = std::fs::read_to_string(tables_data(GLYF_TTX))?;
		assert_eq!(
			ttx_lines(&dump(&mut font, Vec::new())),
			ttx_lines(&expected)
		);
		Ok(())
	}

	#[test]
	fn test_from_xml() -> TestResult {
		let mut font = glyf_ttx_font();
		let data = font.table_data(GLYF)?;
		assert_eq!(data, read_data("_g_l_y_f_outline_flag_bit6.glyf.bin"));
		Ok(())
	}

	#[test]
	fn test_instructions_roundtrip() -> TestResult {
		let original = std::fs::read_to_string(tables_data(INST_TTX))?;
		let mut font = Font::from_ttx(&original)?;
		font.table_data(GLYF)?;
		assert_eq!(
			ttx_lines(&dump(&mut font, Vec::new())),
			ttx_lines(&original)
		);
		Ok(())
	}

	#[test]
	fn test_recursive_component() {
		let order = order(&["A", "B"]);
		let glyf = glyf_of(vec![
			composite(vec![component(1, IDENTITY)]),
			composite(vec![component(0, IDENTITY)]),
		]);
		let error = error_text(glyf.coordinates(0, &order, false));
		assert!(
			error.contains("contains a recursive component reference"),
			"{error}"
		);
	}

	/// The `A -> B -> A` cycle of integer translates, which takes the fast path
	/// of `recalcBounds`.
	#[test]
	fn test_recalc_bounds_recursive_component() {
		let order = order(&["A", "B"]);
		let mut glyf = glyf_of(vec![
			composite(vec![component(1, translation(10.0, 0.0))]),
			composite(vec![component(0, translation(0.0, 10.0))]),
		]);
		for error in [
			error_text(glyf.recalc_glyph_bounds(0, &order)),
			// And with the `boundsDone` set that compiling the table passes.
			error_text(glyf.glyph_data(0, &order, true)),
		] {
			assert!(
				error.contains("contains a recursive component reference"),
				"{error}"
			);
		}
	}

	#[test]
	fn test_recalc_bounds_self_referencing_component() {
		let order = order(&["A"]);
		let mut glyf = glyf_of(vec![composite(vec![component(0, translation(5.0, 5.0))])]);
		let error = error_text(glyf.recalc_glyph_bounds(0, &order));
		assert!(
			error.contains("glyph 'A' contains a recursive component reference"),
			"{error}"
		);
	}

	#[test]
	fn test_recalc_bounds_acyclic_composite() -> TestResult {
		let order = order(&["base", "A"]);
		let base = contours_glyph(&[&[(5.0, 5.0), (10.0, 10.0), (10.0, 5.0), (5.0, 5.0)]]);
		let mut glyf = glyf_of(vec![
			base,
			composite(vec![component(0, translation(100.0, 200.0))]),
		]);
		glyf.recalc_glyph_bounds(0, &order)?;
		glyf.glyph_data(1, &order, true)?;
		let a = &glyf.glyphs[&1];
		assert_eq!((a.x_min, a.y_min, a.x_max, a.y_max), (105, 205, 110, 210));
		Ok(())
	}

	#[test]
	fn test_trim_remove_hinting_composite_glyph() -> TestResult {
		let order = order(&["composite", "dummy"]);
		let program = Program::from_assembly(&["SVTCA[0]"])?;
		let mut composite = composite(vec![component(1, IDENTITY)]);
		composite.program = Some(program.clone());
		let mut glyf = glyf_of(vec![composite, empty_glyph()]);

		// `composite.compact(glyfTable)`.
		let data = glyf.glyph_data(0, &order, true)?;
		let composite = glyf.glyphs.get_mut(&0).unwrap();
		composite.data = Some(data);
		assert!(composite.data.is_some());

		// Remove hinting from the compact glyph, without expanding it.
		composite.trim(true)?;

		// After expanding the glyph, there are no instructions.
		composite.data = None;
		assert!(composite.program.is_none());

		// Now remove hinting from the expanded glyph.
		composite.program = Some(program);
		composite.trim(true)?;
		assert!(composite.program.is_none());

		glyf.glyph_data(0, &order, true)?;
		Ok(())
	}

	#[test]
	fn test_bit6_draw_to_pen_issue1771() -> TestResult {
		// https://github.com/fonttools/fonttools/issues/1771: glyph00003 has bit 6
		// set in the flags of its first point.
		let mut font = glyf_ttx_font();
		let order = font.glyph_order()?;
		let glyph = &font.require::<Glyf>()?.glyphs[&3];
		let mut pen = RecordingPen::new();
		glyph.draw(&order, &mut pen, 0.0)?;
		let expected = vec![
			PenOp::MoveTo((501.0, 1430.0)),
			PenOp::LineTo((683.0, 1430.0)),
			PenOp::LineTo((1172.0, 0.0)),
			PenOp::LineTo((983.0, 0.0)),
			PenOp::LineTo((591.0, 1193.0)),
			PenOp::LineTo((199.0, 0.0)),
			PenOp::LineTo((12.0, 0.0)),
			PenOp::ClosePath,
			PenOp::MoveTo((249.0, 514.0)),
			PenOp::LineTo((935.0, 514.0)),
			PenOp::LineTo((935.0, 352.0)),
			PenOp::LineTo((249.0, 352.0)),
			PenOp::ClosePath,
		];
		assert_eq!(pen.value, expected);
		Ok(())
	}

	#[test]
	fn test_bit6_draw_to_pointpen() -> TestResult {
		let mut font = glyf_ttx_font();
		let order = font.glyph_order()?;
		let glyph = &font.require::<Glyf>()?.glyphs[&3];
		let mut pen = RecordingPointPen::new();
		glyph.draw_points(&order, &mut pen, 0.0)?;
		let line = Some(SegmentType::Line);
		let expected = vec![
			PointOp::BeginPath(None),
			PointOp::point((501.0, 1430.0), line, false),
			PointOp::point((683.0, 1430.0), line, false),
			PointOp::point((1172.0, 0.0), line, false),
			PointOp::point((983.0, 0.0), line, false),
		];
		assert_eq!(pen.value[..expected.len()], expected);
		Ok(())
	}

	#[test]
	fn test_draw_vs_drawpoints() -> TestResult {
		let mut font = glyf_ttx_font();
		let order = font.glyph_order()?;
		let glyph = &font.require::<Glyf>()?.glyphs[&3];
		let mut pen1 = RecordingPen::new();
		let mut pen2 = RecordingPen::new();
		glyph.draw(&order, &mut pen1, 0.0)?;
		glyph.draw_points(&order, &mut PointToSegmentPen::new(&mut pen2, false), 0.0)?;
		assert_eq!(pen1.value, pen2.value);
		Ok(())
	}

	#[test]
	fn test_compile_empty_table() -> TestResult {
		let mut font = glyf_ttx_font();
		let count = font.glyph_order()?.len() as u32;
		// Every glyph without contours.
		font.require::<Glyf>()?.glyphs = (0..count).map(|gid| (gid, Glyph::default())).collect();
		assert_eq!(font.table_data(GLYF)?, [0]);
		let num_glyphs = font.require::<Maxp>()?.num_glyphs as usize;
		assert_eq!(font.require::<Loca>()?.locations(), vec![0; num_glyphs + 1]);
		Ok(())
	}

	#[test]
	fn test_decompile_empty_table() -> TestResult {
		let mut font = fake_font(&[".notdef", "space"]);
		let mut loca = Loca::new_as(Loca::TAG);
		loca.set(vec![0; 3]);
		font.insert(Box::new(loca));
		let glyf = Glyf::decompile(&[0], &mut font)?;
		assert_eq!(glyf.glyphs.len(), 2);
		assert_eq!(glyf.glyphs[&0].number_of_contours, 0);
		assert_eq!(glyf.glyphs[&1].number_of_contours, 0);
		Ok(())
	}

	/// https://github.com/fonttools/fonttools/issues/2295. The deprecated
	/// `getPhantomPoints(glyphName, ttFont, defaultVerticalOrigin=0)`
	/// synthesizes the vertical metrics from `head.unitsPerEm` when there is no
	/// `vmtx`; fontoxide only has `_getCoordinatesAndControls`, which is given
	/// those metrics here.
	#[test]
	fn test_get_phantom_points() -> TestResult {
		let mut font = fake_font(&[".notdef"]);
		let mut loca = Loca::new_as(Loca::TAG);
		loca.set(vec![0; 2]);
		font.insert(Box::new(loca));
		let mut glyf = Glyf::decompile(&[0], &mut font)?;
		let order = font.glyph_order()?;
		let h_metrics = [(0, (100, 0))].into();
		let units_per_em = 1000;
		let v_metrics = [(0, (units_per_em, 0))].into();
		let (coords, _) = glyf.coordinates_and_controls(0, &order, &h_metrics, Some(&v_metrics))?;
		assert_eq!(
			coords,
			[(0.0, 0.0), (100.0, 0.0), (0.0, 0.0), (0.0, -1000.0)]
		);
		Ok(())
	}
}

mod glyph_test {
	//! `GlyphTest`.

	use super::*;

	const SQUARE: &[(f64, f64)] = &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];

	#[test]
	#[allow(clippy::approx_constant)] // The rotation of the upstream test.
	fn test_get_coordinates() -> TestResult {
		let order = order(&["a", "b", "c", "d", "e", "f"]);
		// A simple contour glyph.
		let a = contours_glyph(&[SQUARE]);
		// A composite glyph with only an offset.
		let b = composite(vec![component(0, translation(10.0, 20.0))]);
		// A composite glyph with a scale (and referencing another composite glyph).
		let c = composite(vec![component(1, [0.5, 0.0, 0.0, 0.5, 0.0, 0.0])]);
		// A composite glyph with an unscaled offset (MS-style).
		let mut d = composite(vec![component(0, [0.5, 0.0, 0.0, 0.5, 10.0, 20.0])]);
		d.components[0].flags |= UNSCALED_COMPONENT_OFFSET;
		// A composite glyph with a scaled offset (Apple-style).
		let mut e = composite(vec![component(0, [0.5, 0.0, 0.0, 0.5, 10.0, 20.0])]);
		e.components[0].flags |= SCALED_COMPONENT_OFFSET;
		// A composite glyph whose 2nd and 3rd components use anchor points.
		let mut f = composite(vec![component(0, IDENTITY)]);
		// Align the new component's point 0 to point 2 of the points so far.
		f.components.push(GlyphComponent {
			glyph: 0,
			flags: 0,
			placement: ComponentPlacement::Points {
				first: 2,
				second: 0,
			},
			transform: None,
		});
		// Align the new component's point 0 to point 6, rotated by 45°.
		f.components.push(GlyphComponent {
			glyph: 0,
			flags: WE_HAVE_A_TWO_BY_TWO,
			placement: ComponentPlacement::Points {
				first: 6,
				second: 0,
			},
			transform: Some([[0.707107, 0.707107], [-0.707107, 0.707107]]),
		});
		let glyf = glyf_of(vec![a, b, c, d, e, f]);

		let flat = |gid| glyf.coordinates(gid, &order, false).unwrap();
		assert_eq!(flat(0), (SQUARE.to_vec(), vec![3], vec![1; 4]));
		assert_eq!(
			flat(1),
			(
				vec![(10.0, 20.0), (110.0, 20.0), (110.0, 120.0), (10.0, 120.0)],
				vec![3],
				vec![1; 4]
			)
		);
		let scaled = (
			vec![(5.0, 10.0), (55.0, 10.0), (55.0, 60.0), (5.0, 60.0)],
			vec![3],
			vec![1; 4],
		);
		assert_eq!(flat(2), scaled);
		assert_eq!(
			flat(3),
			(
				vec![(10.0, 20.0), (60.0, 20.0), (60.0, 70.0), (10.0, 70.0)],
				vec![3],
				vec![1; 4]
			)
		);
		assert_eq!(flat(4), scaled);

		let (coords, end_pts, flags) = flat(5);
		assert_eq!(end_pts, [3, 7, 11]);
		assert_eq!(flags, [1; 12]);
		let expected = [
			0.0, 0.0, 100.0, 0.0, 100.0, 100.0, 0.0, 100.0, 100.0, 100.0, 200.0, 100.0, 200.0,
			200.0, 100.0, 200.0, 200.0, 200.0, 270.7107, 270.7107, 200.0, 341.4214, 129.2893,
			270.7107,
		];
		let actual: Vec<f64> = coords.iter().flat_map(|p| [p.0, p.1]).collect();
		assert_eq!(actual.len(), expected.len());
		for (a, e) in actual.iter().zip(expected) {
			assert!(approx(*a, e), "{actual:?}");
		}
		Ok(())
	}

	#[test]
	fn test_get_components_nested() -> TestResult {
		// Check that getCoordinates rounds correctly with nested components.
		let order = order(&["base", "simpleXform", "nestedTrickyXForm"]);
		let mut pen = TtGlyphPen::new(None);
		pen.move_to((372.0, 736.0))?;
		pen.line_to((68.0, 426.0))?;
		pen.q_curve_to(
			&[(68.0, 426.0), (64.25, 414.0), (62.5, 386.25)],
			Some((64.0, 372.0)),
		)?;
		pen.line_to((0.0, 100.0))?;
		pen.line_to((372.0, 736.0))?;
		pen.close_path()?;
		// No rounding here: as if the glyph had been through cubic to quadratic
		// conversion, which introduced some floats.
		let base = pen.glyph_with(
			&order,
			&GlyphOptions {
				round: |v| v,
				..Default::default()
			},
		)?;
		// A simple translation.
		let simple = composite(vec![component(0, translation(10.0, 10.0))]);
		// Flipped on the y axis.
		let tricky = composite(vec![component(1, [-1.0, 0.0, 0.0, -1.0, 834.0, 793.0])]);
		let glyf = glyf_of(vec![base, simple, tricky]);

		let (coords, _, _) = glyf.coordinates(2, &order, true)?;
		assert!(
			coords
				.iter()
				.all(|p| p.0.fract() == 0.0 && p.1.fract() == 0.0),
			"{coords:?}"
		);
		Ok(())
	}

	#[test]
	fn test_get_composite_maxp_values() -> TestResult {
		// https://github.com/fonttools/fonttools/issues/2044
		let order = order(&[
			"fraction",
			"zero.numr",
			"zero.dnom",
			"percent",
			"perthousand",
		]);
		let glyf = glyf_of(vec![
			// Empty non-composite glyphs.
			empty_glyph(),
			empty_glyph(),
			composite(vec![component(1, IDENTITY)]),
			composite(vec![
				component(1, IDENTITY),
				component(0, IDENTITY),
				component(2, IDENTITY),
			]),
			composite(vec![
				component(1, IDENTITY),
				component(0, IDENTITY),
				component(2, IDENTITY),
				component(2, IDENTITY),
			]),
		]);
		let depth = |gid| {
			glyf.composite_maxp_values(gid, &order, 1, &mut HashMap::new(), &mut vec![gid])
				.unwrap()
				.2
		};
		assert_eq!(depth(2), 1);
		assert_eq!(depth(3), 2);
		assert_eq!(depth(4), 2);
		Ok(())
	}

	fn bounds(glyf: &Glyf, gid: u32) -> (i64, i64, i64, i64) {
		let g = &glyf.glyphs[&gid];
		(g.x_min, g.y_min, g.x_max, g.y_max)
	}

	const DOO: &[(f64, f64)] = &[(5.0, 5.0), (10.0, 10.0), (10.0, 5.0), (5.0, 5.0)];

	#[test]
	fn test_recalc_bounds_empty_components() -> TestResult {
		let order = order(&["foo", "doo", "bar"]);
		let mut glyf = glyf_of(vec![
			// An empty simple glyph.
			empty_glyph(),
			// A non-empty simple glyph.
			contours_glyph(&[DOO]),
			// The empty 'foo' with some offset, and 'doo' at the origin.
			composite(vec![
				component(0, translation(-80.0, 50.0)),
				component(1, IDENTITY),
			]),
		]);
		for gid in 0..3 {
			glyf.recalc_glyph_bounds(gid, &order)?;
		}
		assert_eq!(bounds(&glyf, 0), (0, 0, 0, 0));
		assert_eq!(bounds(&glyf, 1), (5, 5, 10, 10));
		// The empty 'foo' does not count.
		assert_eq!(bounds(&glyf, 2), (5, 5, 10, 10));
		Ok(())
	}

	#[test]
	fn test_recalc_bounds_empty_components_nested() -> TestResult {
		// 'foofoo' is a composite glyph whose components have no contours: it
		// does not count either when referenced.
		let order = order(&["foo", "foofoo", "doo", "bar"]);
		let mut glyf = glyf_of(vec![
			empty_glyph(),
			composite(vec![component(0, translation(20.0, 20.0))]),
			contours_glyph(&[DOO]),
			composite(vec![component(1, IDENTITY), component(2, IDENTITY)]),
		]);
		for gid in 0..4 {
			glyf.recalc_glyph_bounds(gid, &order)?;
		}
		assert_eq!(bounds(&glyf, 0), (0, 0, 0, 0));
		assert_eq!(bounds(&glyf, 2), (5, 5, 10, 10));
		assert_eq!(bounds(&glyf, 3), (5, 5, 10, 10));
		Ok(())
	}
}

mod glyph_component_test {
	//! `GlyphComponentTest`.

	use super::*;

	fn to_xml(comp: &GlyphComponent) -> Vec<String> {
		let mut writer = XmlWriter::with_options("  ", "\n");
		comp.to_xml(&mut writer, &order(&["a"]));
		writer_lines(writer)
	}

	fn from_xml(xml: &str) -> GlyphComponent {
		let mut font = fake_font(&["a"]);
		let mut comp = None;
		for e in parse_xml(xml) {
			comp = Some(GlyphComponent::from_xml(&e, &mut font).unwrap());
		}
		comp.unwrap()
	}

	fn offset_component(flags: i64, transform: Option<[[f64; 2]; 2]>) -> GlyphComponent {
		GlyphComponent {
			glyph: 0,
			flags,
			placement: ComponentPlacement::Offset {
				x: Number::Int(1),
				y: Number::Int(2),
			},
			transform,
		}
	}

	fn assert_offset(comp: &GlyphComponent) {
		assert_eq!(comp.glyph, 0);
		assert!(comp.flags & ARGS_ARE_XY_VALUES != 0);
		assert_eq!(
			comp.placement,
			ComponentPlacement::Offset {
				x: Number::Int(1),
				y: Number::Int(2)
			}
		);
	}

	fn assert_transform(comp: &GlyphComponent, expected: [f64; 4]) {
		let t = comp.transform.expect("a transform");
		for (value, expected) in [t[0][0], t[0][1], t[1][0], t[1][1]]
			.into_iter()
			.zip(expected)
		{
			assert!(approx(value, expected), "{t:?}");
		}
	}

	#[test]
	fn test_to_xml_no_transform() {
		let comp = offset_component(ARGS_ARE_XY_VALUES, None);
		assert_eq!(
			to_xml(&comp),
			[r#"<component glyphName="a" x="1" y="2" flags="0x2"/>"#]
		);
	}

	#[test]
	fn test_to_xml_transform_scale() {
		let comp = offset_component(
			ARGS_ARE_XY_VALUES | WE_HAVE_A_SCALE,
			Some([[0.2999878, 0.0], [0.0, 0.2999878]]),
		);
		assert_eq!(
			to_xml(&comp),
			[r#"<component glyphName="a" x="1" y="2" scale="0.3" flags="0xa"/>"#]
		);
	}

	#[test]
	fn test_to_xml_transform_xy_scale() {
		let comp = offset_component(
			ARGS_ARE_XY_VALUES | WE_HAVE_AN_X_AND_Y_SCALE,
			Some([[0.5999756, 0.0], [0.0, 0.2999878]]),
		);
		assert_eq!(
			to_xml(&comp),
			[r#"<component glyphName="a" x="1" y="2" scalex="0.6" scaley="0.3" flags="0x42"/>"#]
		);
	}

	#[test]
	fn test_to_xml_transform_2x2_scale() {
		let comp = offset_component(
			ARGS_ARE_XY_VALUES | WE_HAVE_A_TWO_BY_TWO,
			Some([[0.5999756, -0.2000122], [0.2000122, 0.2999878]]),
		);
		assert_eq!(
			to_xml(&comp),
			[
				r#"<component glyphName="a" x="1" y="2" scalex="0.6" scale01="-0.2" scale10="0.2" scaley="0.3" flags="0x82"/>"#
			]
		);
	}

	#[test]
	fn test_from_xml_no_transform() {
		let comp = from_xml(r#"<component glyphName="a" x="1" y="2" flags="0x2"/>"#);
		assert_offset(&comp);
		assert!(comp.transform.is_none());
	}

	#[test]
	fn test_from_xml_transform_scale() {
		let comp = from_xml(r#"<component glyphName="a" x="1" y="2" scale="0.3" flags="0xa"/>"#);
		assert_offset(&comp);
		assert!(comp.flags & WE_HAVE_A_SCALE != 0);
		assert_transform(&comp, [0.2999878, 0.0, 0.0, 0.2999878]);
	}

	#[test]
	fn test_from_xml_transform_xy_scale() {
		let comp = from_xml(
			r#"<component glyphName="a" x="1" y="2" scalex="0.6" scaley="0.3" flags="0x42"/>"#,
		);
		assert_offset(&comp);
		assert!(comp.flags & WE_HAVE_AN_X_AND_Y_SCALE != 0);
		assert_transform(&comp, [0.5999756, 0.0, 0.0, 0.2999878]);
	}

	#[test]
	fn test_from_xml_transform_2x2_scale() {
		let comp = from_xml(
			r#"<component glyphName="a" x="1" y="2" scalex="0.6" scale01="-0.2" scale10="0.2" scaley="0.3" flags="0x82"/>"#,
		);
		assert_offset(&comp);
		assert!(comp.flags & WE_HAVE_A_TWO_BY_TWO != 0);
		assert_transform(&comp, [0.5999756, -0.2000122, 0.2000122, 0.2999878]);
	}

	#[test]
	fn test_to_xml_reference_points() {
		let comp = GlyphComponent {
			glyph: 0,
			flags: 0,
			placement: ComponentPlacement::Points {
				first: 1,
				second: 2,
			},
			transform: None,
		};
		assert_eq!(
			to_xml(&comp),
			[r#"<component glyphName="a" firstPt="1" secondPt="2" flags="0x0"/>"#]
		);
	}

	#[test]
	fn test_from_xml_reference_points() {
		let comp = from_xml(r#"<component glyphName="a" firstPt="1" secondPt="2" flags="0x0"/>"#);
		assert_eq!(comp.glyph, 0);
		assert_eq!(comp.flags, 0);
		assert_eq!(
			comp.placement,
			ComponentPlacement::Points {
				first: 1,
				second: 2
			}
		);
		assert!(comp.transform.is_none());
	}

	#[test]
	fn test_compile_for_speed() -> TestResult {
		let glyph = simple_glyph(
			&[
				(0.0, 0.0),
				(1.0, 0.0),
				(1.0, 0.0),
				(1.0, 1.0),
				(1.0, 1.0),
				(0.0, 1.0),
				(0.0, 1.0),
			],
			&[
				FLAG_ON_CURVE,
				FLAG_CUBIC,
				FLAG_CUBIC,
				FLAG_CUBIC,
				FLAG_CUBIC,
				FLAG_CUBIC,
				FLAG_CUBIC,
			],
			&[6],
		);
		// `glyph.compile(None, optimizeSize=...)` writes the extended format.
		let order = order(&["a"]);
		let mut glyf = Glyf::new_as(GLYF_UPPER);
		glyf.glyphs.insert(0, glyph);
		let size_bytes = glyf.glyph_data_ext(0, &order, true, true)?;
		let speed_bytes = glyf.glyph_data_ext(0, &order, true, false)?;

		assert!(size_bytes.len() < speed_bytes.len());

		for data in [size_bytes, speed_bytes] {
			let glyph = Glyph::decompile_ext(&data, 1, true)?;
			assert_eq!(
				draw(&glyph),
				[
					PenOp::MoveTo((0.0, 0.0)),
					PenOp::CurveTo(vec![(1.0, 0.0), (1.0, 0.0), (1.0, 0.5)]),
					PenOp::CurveTo(vec![(1.0, 1.0), (1.0, 1.0), (0.5, 1.0)]),
					PenOp::CurveTo(vec![(0.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
					PenOp::ClosePath,
				]
			);
		}
		Ok(())
	}

	fn abc_glyf() -> (Glyf, GlyphOrder) {
		let order = order(&[".notdef", "a", "b"]);
		(
			glyf_of(vec![Glyph::default(), Glyph::default(), Glyph::default()]),
			order,
		)
	}

	/// `glyf.items()`, `iter(glyf)` and `glyf.values()` go in glyph order;
	/// `glyphs` is ordered by glyph ID.
	#[test]
	fn test_items() {
		let (glyf, order) = abc_glyf();
		let items: Vec<(String, &Glyph)> = glyf
			.glyphs
			.iter()
			.map(|(&gid, g)| (order.name(gid).into_owned(), g))
			.collect();
		let default = Glyph::default();
		assert_eq!(
			items,
			[
				(".notdef".to_owned(), &default),
				("a".to_owned(), &default),
				("b".to_owned(), &default)
			]
		);
	}

	#[test]
	fn test_iter() {
		let (glyf, order) = abc_glyf();
		let names: Vec<_> = glyf.glyphs.keys().map(|&gid| order.name(gid)).collect();
		assert_eq!(names, [".notdef", "a", "b"]);
	}

	#[test]
	fn test_values() {
		let (glyf, _) = abc_glyf();
		let glyphs: Vec<&Glyph> = glyf.glyphs.values().collect();
		assert_eq!(
			glyphs,
			[&glyf.glyphs[&0], &glyf.glyphs[&1], &glyf.glyphs[&2]]
		);
	}

	/// The `_lazy` tests: fontoxide reads tables eagerly, which gives the same
	/// glyphs in the same order.
	fn test_ttf() -> (Font, GlyphOrder) {
		let data = std::fs::read(upstream("ttx/data/TestTTF.ttf")).unwrap();
		let mut font = Font::read(data, ReadOptions::default()).unwrap();
		let order = (*font.glyph_order().unwrap()).clone();
		(font, order)
	}

	const TEST_TTF_GLYPHS: [&str; 6] = [".notdef", ".null", "CR", "space", "period", "ellipsis"];

	#[test]
	fn test_items_lazy() -> TestResult {
		let (mut font, order) = test_ttf();
		let glyf = font.require::<Glyf>()?;
		let items: Vec<_> = glyf.glyphs.iter().collect();
		assert_eq!(items[0].1.number_of_contours, 2);
		let names: Vec<_> = items.iter().map(|(gid, _)| order.name(**gid)).collect();
		assert_eq!(names, TEST_TTF_GLYPHS);
		Ok(())
	}

	#[test]
	fn test_iter_lazy() -> TestResult {
		let (mut font, order) = test_ttf();
		let glyf = font.require::<Glyf>()?;
		let names: Vec<_> = glyf.glyphs.keys().map(|&gid| order.name(gid)).collect();
		assert_eq!(names, TEST_TTF_GLYPHS);
		Ok(())
	}

	#[test]
	fn test_values_lazy() -> TestResult {
		let (mut font, _) = test_ttf();
		let glyf = font.require::<Glyf>()?;
		let glyphs: Vec<&Glyph> = glyf.glyphs.values().collect();
		assert_eq!(glyphs.len(), TEST_TTF_GLYPHS.len());
		assert_eq!(glyphs[0].number_of_contours, 2);
		Ok(())
	}
}

mod glyph_cubic_test {
	//! `GlyphCubicTest`.

	use super::*;

	#[test]
	fn test_draw_points_all_offcurve() -> TestResult {
		for count in [2, 4, 8] {
			for cubic in [false, true] {
				let coords: Coords = (0..count).map(|i| (i as f64, (i % 2) as f64)).collect();
				let flags = vec![if cubic { FLAG_CUBIC } else { 0 }; count];
				let glyph = simple_glyph(&coords, &flags, &[count as i64 - 1]);
				let mut pen = RecordingPointPen::new();
				let result = glyph.draw_points(&GlyphOrder::default(), &mut pen, 0.0);
				if cubic {
					let error = error_text(result);
					assert!(error.contains("All-off-curve cubic"), "{error}");
					assert!(pen.value.is_empty());
					draw(&glyph);
				} else {
					result?;
					let points: Coords = pen
						.value
						.iter()
						.filter_map(|op| match op {
							PointOp::AddPoint { pt, .. } => Some(*pt),
							_ => None,
						})
						.collect();
					assert_eq!(points, coords);
				}
			}
		}
		Ok(())
	}

	/// `optimize_size` has no counterpart: glyphs are always packed for size.
	#[test]
	fn test_cubic_requires_extended_serialization() -> TestResult {
		for tag in [GLYF, GLYF_UPPER] {
			let glyph = simple_glyph(
				&[(0.0, 0.0), (0.0, 100.0), (100.0, 100.0), (100.0, 0.0)],
				&[FLAG_ON_CURVE, FLAG_CUBIC, FLAG_CUBIC, FLAG_ON_CURVE],
				&[3],
			);

			// Drawing an in-memory outline must keep the actual segment type.
			let drawn = draw(&glyph);
			assert!(drawn.contains(&PenOp::CurveTo(vec![
				(0.0, 100.0),
				(100.0, 100.0),
				(100.0, 0.0)
			])));
			let mut point_pen = RecordingPointPen::new();
			glyph.draw_points(&GlyphOrder::default(), &mut point_pen, 0.0)?;
			let types: Vec<_> = point_pen
				.value
				.iter()
				.filter_map(|op| match op {
					PointOp::AddPoint { segment_type, .. } => Some(*segment_type),
					_ => None,
				})
				.collect();
			assert_eq!(
				types,
				[
					Some(SegmentType::Line),
					None,
					None,
					Some(SegmentType::Curve)
				]
			);

			if tag == GLYF {
				let error = error_text(compile_in(&glyph, tag, &["a"]));
				assert!(error.contains("extended GLYF"), "{error}");
				let mut font = fake_font(&["a"]);
				let glyf = glyf_of(vec![glyph]);
				let mut writer = XmlWriter::new();
				let error = error_text(fontoxide_ttlib::Table::to_xml(
					&glyf,
					&mut writer,
					&mut font,
				));
				assert!(error.contains("extended GLYF"), "{error}");
			} else {
				let data = compile_in(&glyph, tag, &["a"])?;
				let reloaded = Glyph::decompile_ext(&data, 1, true)?;
				assert_eq!(reloaded.flags, glyph.flags);
				assert_eq!(draw(&reloaded), drawn);
			}
		}
		Ok(())
	}

	#[test]
	fn test_import_cubic_flags() -> TestResult {
		let xml = r#"<TTGlyph name="a"><contour>
            <pt x="0" y="0" on="1"/>
            <pt x="0" y="100" on="0" cubic="1"/>
            <pt x="100" y="100" on="0" cubic="1"/>
            <pt x="100" y="0" on="1"/>
        </contour><instructions/></TTGlyph>"#;
		for tag in [GLYF, GLYF_UPPER] {
			let mut font = fake_font(&["a"]);
			let mut glyf = Glyf::new_as(tag);
			let result = common::from_xml(&mut glyf, xml, &mut font);
			if tag == GLYF {
				let error = error_text(result);
				assert!(error.contains("extended GLYF"), "{error}");
			} else {
				result?;
				assert_eq!(glyf.glyphs[&0].flags, [1, FLAG_CUBIC, FLAG_CUBIC, 1]);
			}
		}
		Ok(())
	}

	#[test]
	fn test_draw_points_rejects_later_cubic_contour_atomically() {
		let mut flags = vec![FLAG_ON_CURVE; 3];
		flags.extend([FLAG_CUBIC; 4]);
		let glyph = simple_glyph(
			&[
				(0.0, 0.0),
				(10.0, 0.0),
				(10.0, 10.0),
				(20.0, 0.0),
				(30.0, 0.0),
				(30.0, 10.0),
				(20.0, 10.0),
			],
			&flags,
			&[2, 6],
		);
		let mut pen = RecordingPointPen::new();
		let error = error_text(glyph.draw_points(&GlyphOrder::default(), &mut pen, 0.0));
		assert!(error.contains("All-off-curve cubic"), "{error}");
		assert!(pen.value.is_empty());
	}

	#[test]
	fn test_roundtrip() -> TestResult {
		let data = std::fs::read(tables_data("NotoSans-VF-cubic.subset.ttf"))?;
		let mut font = Font::read(data, ReadOptions::default())?;
		upper_tables(
			&mut font,
			&ConvertOptions {
				tables: Some(&["glyf", "loca", "maxp", "hhea", "hmtx", "gvar"]),
				..Default::default()
			},
		)?;
		let mut tables = vec![TableKey::GlyphOrder];
		tables.extend(
			font.tags()
				.into_iter()
				.filter(|&t| t != HEAD)
				.map(TableKey::Table),
		);
		let xml = dump(&mut font, Vec::new());
		let xml1 = dump(&mut font, tables.clone());
		let mut font = Font::from_ttx(&xml)?;
		let ttf = font.write(&WriteOptions::default())?;
		let mut font = Font::read(ttf, ReadOptions::default())?;
		let xml2 = dump(&mut font, tables);
		assert_eq!(xml1, xml2);
		Ok(())
	}

	const SPLINE: &[(f64, f64)] = &[
		(0.0, 0.0),
		(1.0, 0.0),
		(1.0, 0.0),
		(1.0, 1.0),
		(1.0, 1.0),
		(0.0, 1.0),
		(0.0, 1.0),
	];

	/// Draws the glyph, then compiles it (`glyph.compile(None)`, which leaves the
	/// outline as it is) and draws it again.
	fn draw_twice(glyph: Glyph) -> [Vec<PenOp>; 2] {
		let first = draw(&glyph);
		let order = order(&["a"]);
		let mut glyf = Glyf::new_as(GLYF_UPPER);
		glyf.glyphs.insert(0, glyph);
		glyf.glyph_data(0, &order, true).unwrap();
		[first, draw(&glyf.glyphs[&0])]
	}

	#[test]
	fn test_no_oncurves() {
		let mut coords = SPLINE.to_vec();
		coords.push((0.0, 0.0));
		let glyph = simple_glyph(&coords, &[FLAG_CUBIC; 8], &[7]);
		for value in draw_twice(glyph) {
			assert_eq!(
				value,
				[
					PenOp::MoveTo((0.0, 0.0)),
					PenOp::CurveTo(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 0.0)]),
					PenOp::CurveTo(vec![(1.0, 0.0), (1.0, 1.0), (1.0, 1.0)]),
					PenOp::CurveTo(vec![(1.0, 1.0), (0.0, 1.0), (0.0, 1.0)]),
					PenOp::CurveTo(vec![(0.0, 1.0), (0.0, 0.0), (0.0, 0.0)]),
					PenOp::ClosePath,
				]
			);
		}
	}

	#[test]
	fn test_spline() {
		let mut flags = vec![FLAG_ON_CURVE];
		flags.extend([FLAG_CUBIC; 6]);
		let glyph = simple_glyph(SPLINE, &flags, &[6]);
		for value in draw_twice(glyph) {
			assert_eq!(
				value,
				[
					PenOp::MoveTo((0.0, 0.0)),
					PenOp::CurveTo(vec![(1.0, 0.0), (1.0, 0.0), (1.0, 0.5)]),
					PenOp::CurveTo(vec![(1.0, 1.0), (1.0, 1.0), (0.5, 1.0)]),
					PenOp::CurveTo(vec![(0.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
					PenOp::ClosePath,
				]
			);
		}
	}
}

/// `struct.pack(">hhhhh", 0, 0, 0, 0, 0)` and the instructions, as a
/// zero-contour glyph.
fn zero_contour_data(instructions: &[u8]) -> Vec<u8> {
	let mut data = vec![0; 10];
	data.extend_from_slice(&(instructions.len() as u16).to_be_bytes());
	data.extend_from_slice(instructions);
	data
}

#[test]
fn test_zero_contour_instructions() -> TestResult {
	for length in [0, 1, 0x8000, 0xFFFF] {
		for remove_hinting in [false, true] {
			for expanded in [false, true] {
				let instructions = vec![0u8; length];
				let data = zero_contour_data(&instructions);
				let mut padded = data.clone();
				padded.extend([0; 3]);
				let mut glyph = compact_glyph(&padded, 0, false);
				if expanded {
					glyph.data = None;
					assert_eq!(glyph.program.as_ref().unwrap().bytecode, instructions);
				}
				glyph.trim(remove_hinting)?;
				if remove_hinting || length == 0 {
					glyph.data = None;
					assert!(glyph.program.as_ref().is_none_or(Program::is_empty));
					assert_eq!(compile_in(&glyph, GLYF_UPPER, &["a"])?, b"");
				} else {
					if !expanded {
						assert_eq!(glyph.data.as_deref(), Some(&data[..]));
					}
					glyph.data = None;
					assert_eq!(glyph.program.as_ref().unwrap().bytecode, instructions);
					assert_eq!(compile_in(&glyph, GLYF_UPPER, &["a"])?, data);
				}
			}
		}
	}
	Ok(())
}

#[test]
fn test_trim_zero_contour_without_instructions() -> TestResult {
	for data in [vec![], vec![0; 10], vec![0; 11]] {
		for remove_hinting in [false, true] {
			let mut glyph = compact_glyph(&data, 0, false);
			glyph.trim(remove_hinting)?;
			glyph.data = None;
			assert_eq!(compile_in(&glyph, GLYF_UPPER, &["a"])?, b"");
		}
	}
	Ok(())
}

/// Only `split_glyphs=False`: fontoxide cannot read TTX split into several
/// files.
#[test]
fn test_zero_contour_instructions_xml() -> TestResult {
	let mut font = fake_font(&[".notdef", "space"]);
	font.recalc_timestamp = false;
	let data = zero_contour_data(&[0xb0, 0x00, 0x21]);
	font.insert(Box::new(glyf_of(vec![
		Glyph::default(),
		compact_glyph(&data, 2, false),
	])));
	let ttx = dump(&mut font, vec![TableKey::GlyphOrder, TableKey::Table(GLYF)]);
	let mut result = Font::from_ttx(&ttx)?;
	let order = result.glyph_order()?;
	assert_eq!(result.require::<Glyf>()?.glyph_data(1, &order, true)?, data);
	Ok(())
}

#[test]
fn test_trim_compact_composite() -> TestResult {
	let transforms: [(u16, &[i16]); 4] = [
		(0, &[]),
		(WE_HAVE_A_SCALE as u16, &[0x2000]),
		(WE_HAVE_AN_X_AND_Y_SCALE as u16, &[0x2000, 0x4000]),
		(
			WE_HAVE_A_TWO_BY_TWO as u16,
			&[0x2000, 0x1000, 0x1000, 0x4000],
		),
	];
	for extended in [false, true] {
		for wide_gid in [false, true] {
			for word_args in [false, true] {
				for (transform_flag, transform_values) in transforms {
					for remove_hinting in [false, true] {
						let header = common::hex("ffff 0000 0000 0064 0064");
						let mut records: Vec<Vec<u8>> = Vec::new();
						for (index, gid) in [2u32, 1].into_iter().enumerate() {
							let mut flags = ARGS_ARE_XY_VALUES as u16 | transform_flag;
							if index == 0 {
								flags |= 0x0020; // MORE_COMPONENTS
								if wide_gid {
									flags |= 0x2000; // GID_IS_24_BIT, reserved in lowercase glyf
								}
							} else {
								flags |= 0x0100; // WE_HAVE_INSTRUCTIONS
							}
							if word_args {
								flags |= 0x0001; // ARG_1_AND_2_ARE_WORDS
							}
							let gid_size = if extended && flags & 0x2000 != 0 {
								3
							} else {
								2
							};
							let mut record = flags.to_be_bytes().to_vec();
							record.extend_from_slice(&gid.to_be_bytes()[4 - gid_size..]);
							if word_args {
								record.extend_from_slice(&123i16.to_be_bytes());
								record.extend_from_slice(&(-42i16).to_be_bytes());
							} else {
								record.extend([123u8, (-42i8) as u8]);
							}
							for v in transform_values {
								record.extend_from_slice(&v.to_be_bytes());
							}
							records.push(record);
						}
						let program = common::hex("0003 b00121");
						let data = [header.clone(), records.concat(), program].concat();
						let mut padded = data.clone();
						padded.extend([0; 3]);
						let mut glyph = compact_glyph(&padded, 3, extended);

						let expected = if remove_hinting {
							// `glyf.removeHinting()`.
							glyph.trim_ext(true, extended)?;
							let last = records.last_mut().unwrap();
							last[0] &= !1;
							[header, records.concat()].concat()
						} else {
							glyph.trim_ext(false, extended)?;
							data
						};
						assert_eq!(glyph.data.as_deref(), Some(&expected[..]));
						glyph.data = None;
						let gids: Vec<u32> = glyph.components.iter().map(|c| c.glyph).collect();
						assert_eq!(gids, [2, 1]);
						for c in &glyph.components {
							assert_eq!(
								c.placement,
								ComponentPlacement::Offset {
									x: Number::Int(123),
									y: Number::Int(-42)
								}
							);
						}
						assert_eq!(glyph.program.is_some(), !remove_hinting);
					}
				}
			}
		}
	}
	Ok(())
}

/// `build_interpolatable_glyphs`: a glyph from contours of (point, flag), and
/// copies of it transformed by each `(xx, xy, yx, yy, dx, dy)`.
fn build_interpolatable_glyphs(
	contours: &[&[((f64, f64), u8)]],
	transforms: &[[f64; 6]],
) -> Vec<Glyph> {
	let coords: Coords = contours
		.iter()
		.flat_map(|c| c.iter().map(|p| p.0))
		.collect();
	let flags: Vec<u8> = contours
		.iter()
		.flat_map(|c| c.iter().map(|p| p.1))
		.collect();
	let mut ends = Vec::new();
	let mut total = 0;
	for c in contours {
		total += c.len() as i64;
		ends.push(total - 1);
	}
	let glyph1 = Glyph {
		number_of_contours: contours.len() as i64,
		coordinates: coords,
		flags,
		end_pts_of_contours: ends,
		..Default::default()
	};
	let mut result = vec![glyph1.clone()];
	for t in transforms {
		let mut glyph = glyph1.clone();
		glyf::transform(&mut glyph.coordinates, [[t[0], t[1]], [t[2], t[3]]]);
		glyf::translate(&mut glyph.coordinates, (t[4], t[5]));
		result.push(glyph);
	}
	result
}

/// `Transform().scale(2.0)`.
const SCALE_2: [f64; 6] = [2.0, 0.0, 0.0, 2.0, 0.0, 0.0];
/// `Transform().translate(10.0)`.
const TRANSLATE_10: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 10.0, 0.0];

fn empty_master() -> Glyph {
	Glyph {
		number_of_contours: 0,
		..Default::default()
	}
}

const QUAD_DIAMOND_1: &[((f64, f64), u8)] = &[
	((0.0, 1.0), FLAG_ON_CURVE),
	((1.0, 1.0), 0),
	((1.0, 0.0), FLAG_ON_CURVE),
	((1.0, -1.0), 0),
	((0.0, -1.0), FLAG_ON_CURVE),
	((-1.0, -1.0), 0),
	((-1.0, 0.0), FLAG_ON_CURVE),
	((-1.0, 1.0), 0),
];

fn set(items: &[usize]) -> BTreeSet<usize> {
	items.iter().copied().collect()
}

#[test]
fn test_drop_implied_on_curve_points_all_quad_off_curves() -> TestResult {
	// Two interpolatable glyphs with the same structure, the coordinates of one
	// twice the other; all the on-curve points are impliable in each one, so
	// they are dropped from both, leaving contours of off-curve points only.
	let quad_diamond_2: Vec<((f64, f64), u8)> = QUAD_DIAMOND_1
		.iter()
		.map(|&((x, y), f)| ((x * 2.0, y * 2.0), f))
		.collect();
	let mut glyphs = build_interpolatable_glyphs(&[QUAD_DIAMOND_1, &quad_diamond_2], &[SCALE_2]);
	// Also an empty glyph, which is ignored: the trick for sparse masters.
	let mut glyph3 = empty_master();
	let [glyph1, glyph2] = &mut glyphs[..] else {
		unreachable!()
	};
	let dropped = drop_implied_on_curve_points(&mut [glyph1, glyph2, &mut glyph3])?;
	assert_eq!(dropped, set(&[0, 2, 4, 6, 8, 10, 12, 14]));

	assert_eq!(glyph1.flags, [0; 8]);
	assert_eq!(glyph2.flags, [0; 8]);
	assert_eq!(
		glyph1.coordinates,
		[
			(1.0, 1.0),
			(1.0, -1.0),
			(-1.0, -1.0),
			(-1.0, 1.0),
			(2.0, 2.0),
			(2.0, -2.0),
			(-2.0, -2.0),
			(-2.0, 2.0)
		]
	);
	assert_eq!(
		glyph2.coordinates,
		[
			(2.0, 2.0),
			(2.0, -2.0),
			(-2.0, -2.0),
			(-2.0, 2.0),
			(4.0, 4.0),
			(4.0, -4.0),
			(-4.0, -4.0),
			(-4.0, 4.0)
		]
	);
	assert_eq!(glyph1.end_pts_of_contours, [3, 7]);
	assert_eq!(glyph2.end_pts_of_contours, [3, 7]);
	assert_eq!(glyph3.number_of_contours, 0);
	Ok(())
}

#[test]
fn test_drop_implied_on_curve_points_all_cubic_off_curves() -> TestResult {
	// The same, with cubic curves.
	let contour: &[((f64, f64), u8)] = &[
		((0.0, 1.0), FLAG_ON_CURVE),
		((1.0, 1.0), FLAG_CUBIC),
		((1.0, 1.0), FLAG_CUBIC),
		((1.0, 0.0), FLAG_ON_CURVE),
		((1.0, -1.0), FLAG_CUBIC),
		((1.0, -1.0), FLAG_CUBIC),
		((0.0, -1.0), FLAG_ON_CURVE),
		((-1.0, -1.0), FLAG_CUBIC),
		((-1.0, -1.0), FLAG_CUBIC),
		((-1.0, 0.0), FLAG_ON_CURVE),
		((-1.0, 1.0), FLAG_CUBIC),
		((-1.0, 1.0), FLAG_CUBIC),
	];
	let mut glyphs = build_interpolatable_glyphs(&[contour], &[TRANSLATE_10]);
	let mut glyph3 = empty_master();
	let [glyph1, glyph2] = &mut glyphs[..] else {
		unreachable!()
	};
	let dropped = drop_implied_on_curve_points(&mut [glyph1, glyph2, &mut glyph3])?;
	assert_eq!(dropped, set(&[0, 3, 6, 9]));

	assert_eq!(glyph1.flags, [FLAG_CUBIC; 8]);
	assert_eq!(glyph2.flags, [FLAG_CUBIC; 8]);
	assert_eq!(
		glyph1.coordinates,
		[
			(1.0, 1.0),
			(1.0, 1.0),
			(1.0, -1.0),
			(1.0, -1.0),
			(-1.0, -1.0),
			(-1.0, -1.0),
			(-1.0, 1.0),
			(-1.0, 1.0)
		]
	);
	assert_eq!(
		glyph2.coordinates,
		[
			(11.0, 1.0),
			(11.0, 1.0),
			(11.0, -1.0),
			(11.0, -1.0),
			(9.0, -1.0),
			(9.0, -1.0),
			(9.0, 1.0),
			(9.0, 1.0)
		]
	);
	assert_eq!(glyph1.end_pts_of_contours, [7]);
	assert_eq!(glyph2.end_pts_of_contours, [7]);
	assert_eq!(glyph3.number_of_contours, 0);
	Ok(())
}

#[test]
fn test_drop_implied_on_curve_points_not_all_impliable() -> TestResult {
	// As in the quadratic test, but one of the glyphs is perturbed so that the
	// 2nd on-curve point is no longer halfway between its off-curve neighbors.
	let mut glyphs = build_interpolatable_glyphs(
		&[QUAD_DIAMOND_1],
		// `Transform().translate(10.0).scale(2.0)` scales, then moves.
		&[TRANSLATE_10, [2.0, 0.0, 0.0, 2.0, 10.0, 0.0]],
	);
	let p2 = glyphs[1].coordinates[2];
	glyphs[1].coordinates[2] = (p2.0 + 2.0, p2.1 - 2.0);
	let [glyph1, glyph2, glyph3] = &mut glyphs[..] else {
		unreachable!()
	};
	let dropped = drop_implied_on_curve_points(&mut [glyph1, glyph2, glyph3])?;
	// 2 is not implied: it is no longer impliable in all the glyphs.
	assert_eq!(dropped, set(&[0, 4, 6]));
	assert_eq!(glyphs[1].flags, [0, FLAG_ON_CURVE, 0, 0, 0]);
	Ok(())
}

#[test]
fn test_drop_implied_on_curve_points_all_empty_glyphs() -> TestResult {
	let mut glyph1 = empty_master();
	let mut glyph2 = empty_master();
	assert_eq!(
		drop_implied_on_curve_points(&mut [&mut glyph1, &mut glyph2])?,
		BTreeSet::new()
	);
	Ok(())
}

fn incompatible(glyph1: Glyph, glyph2: Glyph) -> String {
	let (mut glyph1, mut glyph2) = (glyph1, glyph2);
	error_text(drop_implied_on_curve_points(&mut [
		&mut glyph1,
		&mut glyph2,
	]))
}

#[test]
fn test_drop_implied_on_curve_points_incompatible_number_of_contours() {
	let coords = [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)];
	let error = incompatible(
		simple_glyph(&coords, &[1; 4], &[3]),
		simple_glyph(&coords, &[1; 4], &[1, 3]),
	);
	assert!(error.contains("Incompatible numberOfContours"), "{error}");
}

#[test]
fn test_drop_implied_on_curve_points_incompatible_flags() {
	let coords = [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)];
	let error = incompatible(
		simple_glyph(&coords, &[1; 4], &[3]),
		simple_glyph(&coords, &[0; 4], &[3]),
	);
	assert!(error.contains("Incompatible flags"), "{error}");
}

#[test]
fn test_drop_implied_on_curve_points_incompatible_end_pts_of_contours() {
	let coords: Coords = (0..7).map(|i| (i as f64, i as f64)).collect();
	let error = incompatible(
		simple_glyph(&coords, &[1; 7], &[2, 6]),
		simple_glyph(&coords, &[1; 7], &[3, 6]),
	);
	assert!(error.contains("Incompatible endPtsOfContours"), "{error}");
}

#[test]
fn test_unsigned_instruction_length() -> TestResult {
	for length in [0x7FFF, 0x8000, 0xFFFF, 0x10000] {
		for is_composite in [false, true] {
			let names = [".notdef", "base", "glyph", "reloaded"];
			let order = order(&names);
			let base = contours_glyph(&[&[(10.0, 20.0), (100.0, 20.0), (10.0, 100.0)]]);
			let mut glyph = if is_composite {
				composite(vec![component(1, translation(30.0, 40.0))])
			} else {
				base.clone()
			};
			glyph.program = Some(Program::from_bytecode(&vec![0; length]));
			let mut glyf = glyf_of(vec![Glyph::default(), base, glyph]);

			let result = glyf.glyph_data(2, &order, true);
			if length > 0xFFFF {
				let error = error_text(result);
				assert!(error.contains("format requires"), "{error}");
				continue;
			}
			let data = result?;
			let reloaded = Glyph::decompile(&data, names.len())?;
			assert_eq!(
				reloaded.program.as_ref().unwrap().bytecode,
				glyf.glyphs[&2].program.as_ref().unwrap().bytecode
			);
			glyf.glyphs.insert(3, reloaded);
			assert_eq!(
				glyf.coordinates(3, &order, false)?,
				glyf.coordinates(2, &order, false)?
			);
			assert_eq!(glyf.glyph_data(3, &order, true)?, data);
		}
	}
	Ok(())
}

/// `build_composite_dag`: a table where composite `g{k}` uses `g{k-1}` twice,
/// for k from 1 to n, and `g0` is a square: `g{k}` flattens to 4 * 2**k
/// points. `g{k}` has glyph ID k + 1.
fn build_composite_dag(n: u32) -> (Glyf, Vec<String>) {
	let square = || contours_glyph(&[&[(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)]]);
	let mut names = vec![".notdef".to_owned()];
	names.extend((0..=n).map(|k| format!("g{k}")));
	let mut glyphs = vec![square(), square()];
	for k in 1..=n {
		glyphs.push(composite(vec![
			component(k, IDENTITY),
			component(k, IDENTITY),
		]));
	}
	(glyf_of(glyphs), names)
}

fn dag_order(names: &[String]) -> GlyphOrder {
	GlyphOrder::new(names.to_vec())
}

#[test]
fn test_composite_dag_under_point_limit() -> TestResult {
	let (glyf, names) = build_composite_dag(13);
	let order = dag_order(&names);
	let g13 = 14;

	let (coords, end_pts, flags) = glyf.coordinates(g13, &order, false)?;
	assert_eq!(coords.len(), 32768);
	assert_eq!(flags.len(), 32768);
	assert_eq!(end_pts, (3..32768).step_by(4).collect::<Vec<i64>>());
	assert_eq!(
		coords[..4],
		[(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)]
	);
	let distinct: BTreeSet<(i64, i64)> = coords.iter().map(|p| (p.0 as i64, p.1 as i64)).collect();
	assert_eq!(distinct, [(0, 0), (0, 10), (10, 10), (10, 0)].into());

	for _ in 0..2 {
		assert_eq!(
			glyf.composite_maxp_values(g13, &order, 1, &mut HashMap::new(), &mut vec![g13])?,
			(32768, 8192, 13)
		);
	}
	Ok(())
}

/// The upstream test bounds the number of glyph lookups; here the default
/// cache only has to give the right results (an exponential traversal would
/// make the test noticeably slow).
#[test]
fn test_composite_dag_default_cache() -> TestResult {
	let (mut glyf, names) = build_composite_dag(12);
	let order = dag_order(&names);
	let g12 = 13;
	assert_eq!(
		glyf.composite_maxp_values(g12, &order, 1, &mut HashMap::new(), &mut vec![g12])?,
		(16384, 4096, 12)
	);
	glyf.recalc_glyph_bounds(g12, &order)?;
	let g = &glyf.glyphs[&g12];
	assert_eq!((g.x_min, g.y_min, g.x_max, g.y_max), (0, 0, 10, 10));
	Ok(())
}

#[test]
fn test_composite_point_limit() {
	let big_coords: Coords = (0..0xFFFF)
		.map(|i| ((i % 100) as f64, (i / 100) as f64))
		.collect();
	let big = simple_glyph(&big_coords, &vec![FLAG_ON_CURVE; 0xFFFF], &[0xFFFF - 1]);
	let dot = simple_glyph(&[(0.0, 0.0)], &[FLAG_ON_CURVE], &[0]);
	let order = order(&["big", "dot", "atLimit", "overLimit"]);
	let glyf = glyf_of(vec![
		big,
		dot,
		composite(vec![component(0, IDENTITY)]),
		composite(vec![component(0, IDENTITY), component(1, IDENTITY)]),
	]);

	let (coords, end_pts, flags) = glyf.coordinates(2, &order, false).unwrap();
	assert_eq!(coords.len(), 0xFFFF);
	assert_eq!(flags.len(), 0xFFFF);
	assert_eq!(end_pts, [0xFFFF - 1]);

	let error = error_text(glyf.coordinates(3, &order, false));
	assert!(error.contains("has more than 65535 points"), "{error}");
}

#[test]
fn test_composite_dag_over_point_limit() {
	let (glyf, names) = build_composite_dag(20);
	let order = dag_order(&names);
	let error = error_text(glyf.coordinates(21, &order, false));
	assert!(error.contains("has more than 65535 points"), "{error}");
	// 4 * 2**14 points is one more than the limit.
	let error = error_text(glyf.coordinates(15, &order, false));
	assert!(error.contains("has more than 65535 points"), "{error}");
}

#[test]
fn test_composite_dag_empty_leaf() -> TestResult {
	let (mut glyf, names) = build_composite_dag(22);
	let order = dag_order(&names);
	glyf.glyphs.insert(1, Glyph::default());
	assert_eq!(
		glyf.coordinates(23, &order, false)?,
		(Vec::new(), Vec::new(), Vec::new())
	);
	Ok(())
}

#[test]
fn test_get_coordinates_reused_component() -> TestResult {
	for round in [false, true] {
		let (mut glyf, mut names) = build_composite_dag(1);
		let g1 = 2;
		let transforms = [
			[0.5, 0.0, 0.0, 1.5, 3.0, 7.0],
			[1.0, 0.0, 0.0, 1.0, -20.0, 5.0],
			[0.3, 0.2, -0.1, 0.7, 11.5, 0.0],
		];
		let first_single = names.len() as u32;
		for (i, t) in transforms.iter().enumerate() {
			names.push(format!("single{i}"));
			glyf.glyphs
				.insert(first_single + i as u32, composite(vec![component(g1, *t)]));
		}
		let multi = names.len() as u32;
		names.push("multi".to_owned());
		let indices: Vec<usize> = (0..3).chain((0..3).rev()).collect();
		glyf.glyphs.insert(
			multi,
			composite(
				indices
					.iter()
					.map(|&i| component(g1, transforms[i]))
					.collect(),
			),
		);
		let order = dag_order(&names);

		let (coords, end_pts, flags) = glyf.coordinates(multi, &order, round)?;
		let mut expected = Vec::new();
		for &i in &indices {
			expected.extend(glyf.coordinates(first_single + i as u32, &order, round)?.0);
		}
		assert_eq!(coords, expected);
		assert_eq!(
			end_pts,
			(3..expected.len() as i64).step_by(4).collect::<Vec<_>>()
		);
		assert_eq!(flags, vec![FLAG_ON_CURVE; expected.len()]);
	}
	Ok(())
}

/// `FontBuilder` is not available here: the font gets the tables that
/// `setupGlyf` and `setupHorizontalMetrics` would give it.
fn recalc_composite_dag_maxp(n: u32) -> fontoxide_ttlib::Result<(Maxp, Font)> {
	let (glyf, names) = build_composite_dag(n);
	let mut font = Font::new();
	font.set_glyph_order(names.clone());
	font.insert(Box::new(Head {
		units_per_em: 1000,
		..Default::default()
	}));
	let mut maxp = Maxp::new_as(Maxp::TAG);
	maxp.table_version = 0x0001_0000;
	font.insert(Box::new(maxp));
	font.insert(Box::new(Loca::new_as(Loca::TAG)));
	font.insert(Box::new(glyf));
	font.insert(Box::new(Hmtx {
		metrics: (0..names.len() as u32).map(|gid| (gid, (10, 0))).collect(),
		..Default::default()
	}));

	// The bounds of composites with integer offsets don't need flattening.
	font.table_data(GLYF)?;

	let mut maxp = font.require::<Maxp>()?.clone();
	maxp.recalc(&mut font)?;
	Ok((maxp, font))
}

#[test]
fn test_maxp_recalc_composite_dag() -> TestResult {
	// LENIENT(L38), timing as L9/L24: with n = 20, maxCompositePoints is 4 * 2**20,
	// which fontTools stores in `maxp` and only fails to pack when compiling it;
	// the typed `maxp` fields reject it in `recalc` already.
	let error = error_text(recalc_composite_dag_maxp(20));
	assert!(error.contains("maxCompositePoints"), "{error}");

	// The upstream assertions, for the largest DAG whose values fit.
	let n = 13;
	let (maxp, mut font) = recalc_composite_dag_maxp(n)?;
	assert_eq!(maxp.v1.max_points, 4);
	assert_eq!(maxp.v1.max_contours, 1);
	assert_eq!(u32::from(maxp.v1.max_composite_points), 4 * 2u32.pow(n));
	assert_eq!(u32::from(maxp.v1.max_composite_contours), 2u32.pow(n));
	assert_eq!(maxp.v1.max_component_elements, 2);
	assert_eq!(u32::from(maxp.v1.max_component_depth), n);
	let head = font.require::<Head>()?;
	assert_eq!(
		(head.x_min, head.y_min, head.x_max, head.y_max),
		(0, 0, 10, 10)
	);
	Ok(())
}

#[test]
fn test_get_composite_maxp_values_cache() -> TestResult {
	let paths = [
		Some("ttx/data/TestTTF.ttf"),
		Some("qu2cu/data/NotoSansArabic-Regular.quadratic.subset.ttf"),
		Some("subset/data/BungeeColor-Regular.ttx"),
		Some("varLib/data/MutatorSans_All_Variable.ttx"),
		Some("varLib/instancer/data/PartialInstancerTest3-VF.ttx"),
		None,
	];
	for path in paths {
		let (glyf, order) = match path {
			None => {
				let (glyf, names) = build_composite_dag(12);
				(glyf, dag_order(&names))
			}
			Some(path) => {
				let path = upstream(path);
				let mut font = if path.extension().is_some_and(|e| e == "ttx") {
					let mut font = Font::new();
					font.import_ttx_with(
						&std::fs::read_to_string(&path)?,
						&TtxImportOptions {
							skip_unimplemented: true,
						},
					)?;
					font
				} else {
					Font::read(std::fs::read(&path)?, ReadOptions::default())?
				};
				let order = (*font.glyph_order()?).clone();
				(font.require::<Glyf>()?.clone(), order)
			}
		};
		let composites: Vec<u32> = (0..order.len() as u32)
			.filter(|gid| glyf.glyphs[gid].is_composite())
			.collect();
		assert!(!composites.is_empty(), "{path:?}");

		for depth in [1, 3] {
			let values = |gid: u32, cache: &mut HashMap<_, _>| {
				glyf.composite_maxp_values(gid, &order, depth, cache, &mut vec![gid])
					.unwrap()
			};
			let expected: HashMap<u32, _> = composites
				.iter()
				.map(|&gid| (gid, values(gid, &mut HashMap::new())))
				.collect();
			let reversed: Vec<u32> = composites.iter().rev().copied().collect();
			for gids in [&composites, &reversed] {
				let mut cache = HashMap::new();
				for &gid in gids {
					assert_eq!(values(gid, &mut cache), expected[&gid], "{path:?}");
				}
			}
		}
	}
	Ok(())
}
