//! Ports of fontTools' `Tests/pens/t2CharStringPen_test.py`,
//! `Tests/pens/ttGlyphPen_test.py`, and `test_all_offcurve_glyph` of
//! `Tests/pens/roundingPen_test.py`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use fontoxide_misc::round_tools::ot_round_f64;
use fontoxide_pens::recording_pen::{PenOp, RecordingPen, RecordingPointPen, replay_recording};
use fontoxide_pens::rounding_pen::{RoundFunc, RoundingPen, no_round};
use fontoxide_pens::{
	GlyphSet as PenGlyphSet, Pen, Point, PointPen, Result as PenResult, SegmentType, Transform,
};
use fontoxide_ttlib::cff::charstring::Token;
use fontoxide_ttlib::pens::{
	GlyphOptions, MAX_F2DOT14, T2CharStringPen, TtGlyphPen, TtGlyphPointPen,
};
use fontoxide_ttlib::tables::Glyf;
use fontoxide_ttlib::tables::glyf::Glyph;
use fontoxide_ttlib::{Font, GlyphOrder, GlyphSet, Result};

// `t2CharStringPen_test.py`.

#[derive(Debug, PartialEq)]
enum P {
	N(f64),
	O(&'static str),
}

fn program(tokens: &[Token]) -> Vec<P> {
	tokens
		.iter()
		.map(|t| match t {
			Token::Int(i) => P::N(*i as f64),
			Token::Float(f) => P::N(*f),
			Token::Op(o) => P::O(o),
			Token::Mask(_) => P::O("mask"),
		})
		.collect()
}

/// A program written as numbers and operators.
macro_rules! prog {
	($($x:expr),* $(,)?) => {{
		let mut v: Vec<P> = Vec::new();
		$( v.push(prog!(@one $x)); )*
		v
	}};
	(@one $x:literal) => { prog!(@lit $x) };
	(@lit $x:literal) => { P::from($x) };
}

impl From<i32> for P {
	fn from(v: i32) -> Self {
		P::N(v as f64)
	}
}
impl From<f64> for P {
	fn from(v: f64) -> Self {
		P::N(v)
	}
}
impl From<&'static str> for P {
	fn from(v: &'static str) -> Self {
		P::O(v)
	}
}

fn t2(
	width: Option<f64>,
	tolerance: f64,
	draw: impl FnOnce(&mut dyn Pen) -> PenResult<()>,
) -> Result<Vec<P>> {
	let mut pen = T2CharStringPen::new(width, None, tolerance, false)?;
	draw(&mut pen)?;
	Ok(program(&pen.get_char_string(true)?.program))
}

/// `assertAlmostEqualProgram`: numbers to 7 decimal places.
fn assert_almost(expected: &[P], actual: &[P]) {
	assert_eq!(expected.len(), actual.len(), "{actual:?}");
	for (e, a) in expected.iter().zip(actual) {
		match (e, a) {
			(P::N(x), P::N(y)) => assert!((x - y).abs() < 5e-8, "{x} != {y}"),
			_ => assert_eq!(e, a),
		}
	}
}

#[test]
fn test_t2_draw_lines() -> Result<()> {
	assert_eq!(
		t2(Some(100.0), 0.5, |p| {
			p.move_to((0.0, 0.0))?;
			p.line_to((10.0, 0.0))?;
			p.line_to((10.0, 10.0))?;
			p.line_to((0.0, 10.0))?;
			p.close_path()?;
			p.move_to((10.0, 10.0))?;
			p.line_to((10.0, 20.0))?;
			p.line_to((0.0, 20.0))?;
			p.line_to((0.0, 10.0))?;
			p.close_path()
		})?,
		prog![
			100, 0, "hmoveto", 10, 10, -10, "hlineto", 10, "hmoveto", 10, -10, -10, "vlineto",
			"endchar"
		]
	);
	assert_eq!(
		t2(Some(100.0), 0.5, |p| {
			p.move_to((5.0, 5.0))?;
			p.line_to((25.0, 15.0))?;
			p.line_to((35.0, 35.0))?;
			p.line_to((15.0, 25.0))?;
			p.close_path()
		})?,
		prog![
			100, 5, 5, "rmoveto", 20, 10, 10, 20, -20, -10, "rlineto", "endchar"
		]
	);
	Ok(())
}

#[test]
fn test_t2_draw_curves() -> Result<()> {
	assert_eq!(
		t2(Some(100.0), 0.5, |p| {
			p.move_to((0.0, 0.0))?;
			p.curve_to(&[(10.0, 0.0), (20.0, 10.0), (20.0, 20.0)])?;
			p.curve_to(&[(20.0, 30.0), (10.0, 40.0), (0.0, 40.0)])?;
			p.end_path()
		})?,
		prog![
			100,
			0,
			"hmoveto",
			10,
			10,
			10,
			10,
			10,
			-10,
			10,
			-10,
			"hvcurveto",
			"endchar"
		]
	);
	assert_eq!(
		t2(Some(100.0), 0.5, |p| {
			p.move_to((95.0, 25.0))?;
			p.curve_to(&[(115.0, 44.0), (115.0, 76.0), (95.0, 95.0)])?;
			p.curve_to(&[(76.0, 114.0), (44.0, 115.0), (25.0, 95.0)])?;
			p.end_path()
		})?,
		prog![
			100,
			95,
			25,
			"rmoveto",
			20,
			19,
			0,
			32,
			-20,
			19,
			-19,
			19,
			-32,
			1,
			-19,
			-20,
			"rrcurveto",
			"endchar"
		]
	);
	assert_eq!(
		t2(Some(100.0), 0.5, |p| {
			p.move_to((10.0, 10.0))?;
			p.curve_to(&[(20.0, 10.0), (50.0, 10.0), (60.0, 10.0)])?;
			p.curve_to(&[(60.0, 20.0), (60.0, 50.0), (60.0, 60.0)])?;
			p.curve_to(&[(50.0, 50.0), (40.0, 60.0), (30.0, 60.0)])?;
			p.curve_to(&[(40.0, 50.0), (30.0, 40.0), (30.0, 30.0)])?;
			p.curve_to(&[(30.0, 25.0), (25.0, 19.0), (20.0, 20.0)])?;
			p.curve_to(&[(15.0, 20.0), (9.0, 25.0), (10.0, 30.0)])?;
			p.curve_to(&[(7.0, 25.0), (6.0, 15.0), (10.0, 10.0)])?;
			p.end_path()
		})?,
		prog![
			100,
			10,
			10,
			"rmoveto",
			10,
			30,
			0,
			10,
			"hhcurveto",
			10,
			0,
			30,
			10,
			"vvcurveto",
			-10,
			-10,
			-10,
			10,
			-10,
			"hhcurveto",
			10,
			-10,
			-10,
			-10,
			-10,
			"vvcurveto",
			-5,
			-5,
			-6,
			-5,
			1,
			"vhcurveto",
			-5,
			-6,
			5,
			5,
			1,
			"hvcurveto",
			-3,
			-5,
			-1,
			-10,
			4,
			-5,
			"rrcurveto",
			"endchar"
		]
	);
	Ok(())
}

#[test]
fn test_t2_default_width_and_rounding() -> Result<()> {
	assert_eq!(t2(None, 0.5, |_| Ok(()))?, prog!["endchar"]);

	let curves = |y: f64| {
		move |p: &mut dyn Pen| -> PenResult<()> {
			p.move_to((0.0, 0.0))?;
			p.curve_to(&[(10.1, 0.1), (19.9, 9.9), (20.49, 20.49)])?;
			p.curve_to(&[(20.49, y), (9.9, 39.9), (0.1, 40.1)])?;
			p.close_path()
		}
	};
	// The advance width is always rounded.
	assert_almost(
		&prog![
			100,
			0,
			"hmoveto",
			10.1,
			0.1,
			9.8,
			9.8,
			0.59,
			10.59,
			"rrcurveto",
			10,
			-10.59,
			9.41,
			-9.8,
			0.2,
			"vhcurveto",
			"endchar"
		],
		&t2(Some(100.1), 0.0, curves(30.49))?,
	);
	assert_eq!(
		t2(Some(100.1), 0.5, curves(30.5))?,
		prog![
			100,
			0,
			"hmoveto",
			10,
			10,
			10,
			10,
			11,
			-10,
			9,
			-10,
			"hvcurveto",
			"endchar"
		]
	);
	// Within the tolerance, the first two points are rounded; not the third.
	assert_almost(
		&prog![
			100, 0, "hmoveto", 10, "hlineto", 10, 10, 0.49, 10.49, "rlineto", "endchar"
		],
		&t2(Some(100.0), 0.2, |p| {
			p.move_to((0.0, 0.0))?;
			p.line_to((10.1, 0.1))?;
			p.line_to((19.9, 9.9))?;
			p.line_to((20.49, 20.49))?;
			p.close_path()
		})?,
	);
	let e = T2CharStringPen::new(None, None, -0.1, false);
	assert!(
		matches!(e, Err(fontoxide_ttlib::Error::Value(m)) if m.contains("Rounding tolerance must be positive"))
	);
	Ok(())
}

// `ttGlyphPen_test.py`.

/// `_TestGlyph`: draws the coordinates of a glyph as a polygon.
#[derive(Default)]
struct LineGlyphs(BTreeMap<String, Vec<Point>>);

impl PenGlyphSet for LineGlyphs {
	fn draw_glyph(&self, name: &str, pen: &mut dyn Pen) -> PenResult<bool> {
		let Some(c) = self.0.get(name) else {
			return Ok(false);
		};
		pen.move_to(c[0])?;
		for &p in &c[1..] {
			pen.line_to(p)?;
		}
		pen.close_path()?;
		Ok(true)
	}

	fn draw_glyph_points(&self, name: &str, pen: &mut dyn PointPen) -> PenResult<bool> {
		let Some(c) = self.0.get(name) else {
			return Ok(false);
		};
		pen.begin_path(None)?;
		for &p in c {
			pen.add_point(p, Some(SegmentType::Line), false, None, None)?;
		}
		pen.end_path()?;
		Ok(true)
	}
}

fn order(names: &[&str]) -> GlyphOrder {
	GlyphOrder::new(names.iter().map(|s| s.to_string()).collect())
}

fn t(xx: f64, xy: f64, yx: f64, yy: f64, dx: f64, dy: f64) -> Transform {
	Transform::new(xx, xy, yx, yy, dx, dy)
}

const LINE: Option<SegmentType> = Some(SegmentType::Line);

/// The triangle (0, 0), (0, 1), (1, 0), moved by `dx`.
fn triangle(pen: &mut dyn Pen, dx: f64) -> PenResult<()> {
	pen.move_to((dx, 0.0))?;
	pen.line_to((dx, 1.0))?;
	pen.line_to((dx + 1.0, 0.0))?;
	pen.close_path()
}

fn triangle_points(pen: &mut dyn PointPen, dx: f64) -> PenResult<()> {
	pen.begin_path(None)?;
	pen.add_point((dx, 0.0), LINE, false, None, None)?;
	pen.add_point((dx, 1.0), LINE, false, None, None)?;
	pen.add_point((dx + 1.0, 0.0), LINE, false, None, None)?;
	pen.end_path()
}

#[test]
fn test_move_to_error_within_contour() {
	let mut pen = TtGlyphPen::new(None);
	assert!(pen.move_to((0.0, 0.0)).is_ok());
	assert!(matches!(
		pen.move_to((1.0, 0.0)),
		Err(fontoxide_pens::Error::Pen(_))
	));
}

#[test]
fn test_close_path_ignores_anchors() -> Result<()> {
	let mut pen = TtGlyphPen::new(None);
	pen.move_to((0.0, 0.0))?;
	pen.close_path()?;
	assert!(pen.points().is_empty() && pen.types().is_empty() && pen.end_pts().is_empty());
	Ok(())
}

#[test]
fn test_end_path_same_as_close_path() -> Result<()> {
	let o = order(&[]);
	let mut pen = TtGlyphPen::new(None);
	pen.move_to((0.0, 0.0))?;
	pen.line_to((0.0, 1.0))?;
	pen.line_to((1.0, 0.0))?;
	pen.close_path()?;
	let close = pen.glyph(&o)?;
	pen.move_to((0.0, 0.0))?;
	pen.line_to((0.0, 1.0))?;
	pen.line_to((1.0, 0.0))?;
	pen.end_path()?;
	assert_eq!(close, pen.glyph(&o)?);
	Ok(())
}

#[test]
fn test_glyph_error_on_unended_contour() -> Result<()> {
	let o = order(&[]);
	let mut pen = TtGlyphPen::new(None);
	pen.move_to((0.0, 0.0))?;
	assert!(pen.glyph(&o).is_err());

	let mut pen = TtGlyphPointPen::new(None);
	pen.begin_path(None)?;
	pen.add_point((0.0, 0.0), None, false, None, None)?;
	assert!(pen.glyph(&o).is_err());
	// `test_beginPath_beginPathOnOpenPath`.
	assert!(matches!(
		pen.begin_path(None),
		Err(fontoxide_pens::Error::Pen(_))
	));
	Ok(())
}

/// The glyph set with the triangle as "a", as `_TestGlyph(pen.glyph())`.
fn triangle_set() -> Result<(LineGlyphs, GlyphOrder)> {
	let o = order(&["a"]);
	let mut pen = TtGlyphPen::new(None);
	triangle(&mut pen, 0.0)?;
	let glyph = pen.glyph(&o)?;
	let mut set = LineGlyphs::default();
	set.0.insert("a".into(), glyph.coordinates);
	Ok((set, o))
}

#[test]
fn test_glyph_decomposes() -> Result<()> {
	let (set, o) = triangle_set()?;
	let mut pen = TtGlyphPen::new(Some(&set));
	triangle(&mut pen, 0.0)?;
	pen.add_component("a", t(1.0, 0.0, 0.0, 1.0, 2.0, 0.0))?;
	// Skipped.
	pen.add_component("missing", Transform::default())?;
	let composite = pen.glyph(&o)?;
	triangle(&mut pen, 0.0)?;
	triangle(&mut pen, 2.0)?;
	assert_eq!(pen.glyph(&o)?, composite);

	let mut pen = TtGlyphPointPen::new(Some(&set));
	triangle_points(&mut pen, 0.0)?;
	pen.add_component("a", t(1.0, 0.0, 0.0, 1.0, 2.0, 0.0), None)?;
	pen.add_component("missing", Transform::default(), None)?;
	let composite = pen.glyph(&o)?;
	triangle_points(&mut pen, 0.0)?;
	triangle_points(&mut pen, 2.0)?;
	assert_eq!(pen.glyph(&o)?, composite);
	Ok(())
}

#[test]
fn test_remove_extra_move_points() -> Result<()> {
	for (end, extra_line, n) in [
		((0.0, 0.0), false, 4),
		((30.0, 30.0), false, 5),
		((0.0, 0.0), true, 5),
	] {
		let mut pen = TtGlyphPen::new(None);
		pen.move_to((0.0, 0.0))?;
		pen.line_to((100.0, 0.0))?;
		pen.q_curve_to(&[(100.0, 50.0), (50.0, 100.0)], Some(end))?;
		if extra_line {
			// The duplicate point is kept.
			pen.line_to((0.0, 0.0))?;
		}
		pen.close_path()?;
		assert_eq!(pen.points().len(), n);
		assert_eq!(pen.points()[0], (0.0, 0.0));
	}

	let mut pen = TtGlyphPointPen::new(None);
	pen.begin_path(None)?;
	pen.add_point((0.0, 0.0), LINE, false, None, None)?;
	pen.add_point((100.0, 0.0), LINE, false, None, None)?;
	pen.add_point((100.0, 50.0), None, false, None, None)?;
	pen.add_point((50.0, 100.0), None, false, None, None)?;
	pen.add_point((0.0, 0.0), Some(SegmentType::QCurve), false, None, None)?;
	pen.add_point((0.0, 0.0), LINE, false, None, None)?;
	pen.end_path()?;
	assert_eq!(pen.points().len(), 6);
	assert_eq!(pen.points()[0], (0.0, 0.0));
	Ok(())
}

/// Draws the components onto both pens, and checks that the glyphs match.
fn compare_components(first: &[Transform], second: &[Transform]) -> Result<()> {
	let (set, o) = triangle_set()?;
	let mut pen = TtGlyphPen::new(Some(&set));
	for &c in first {
		pen.add_component("a", c)?;
	}
	let a = pen.glyph(&o)?;
	for &c in second {
		pen.add_component("a", c)?;
	}
	assert_eq!(pen.glyph(&o)?, a);

	let mut pen = TtGlyphPointPen::new(Some(&set));
	for &c in first {
		pen.add_component("a", c, None)?;
	}
	let b = pen.glyph(&o)?;
	for &c in second {
		pen.add_component("a", c, None)?;
	}
	assert_eq!(pen.glyph(&o)?, b);
	assert_eq!(a, b);
	Ok(())
}

#[test]
fn test_within_range_component_transform() -> Result<()> {
	let cs = [
		t(1.5, 0.0, 0.0, 1.0, 0.0, 0.0),
		t(1.0, 0.0, 0.0, -1.5, 0.0, 0.0),
	];
	compare_components(&cs, &cs)
}

#[test]
fn test_clamp_to_almost_2_component_transform() -> Result<()> {
	let m = MAX_F2DOT14;
	compare_components(
		&[
			t(1.99999, 0.0, 0.0, 1.0, 0.0, 0.0),
			t(1.0, 2.0, 0.0, 1.0, 0.0, 0.0),
			t(1.0, 0.0, 2.0, 1.0, 0.0, 0.0),
			t(1.0, 0.0, 0.0, 2.0, 0.0, 0.0),
			t(-2.0, 0.0, 0.0, -2.0, 0.0, 0.0),
		],
		&[
			t(m, 0.0, 0.0, 1.0, 0.0, 0.0),
			t(1.0, m, 0.0, 1.0, 0.0, 0.0),
			t(1.0, 0.0, m, 1.0, 0.0, 0.0),
			t(1.0, 0.0, 0.0, m, 0.0, 0.0),
			t(-2.0, 0.0, 0.0, -2.0, 0.0, 0.0),
		],
	)
}

#[test]
fn test_out_of_range_transform_decomposed() -> Result<()> {
	let (set, o) = triangle_set()?;
	let components = [
		t(3.0, 0.0, 0.0, 2.0, 0.0, 0.0),
		t(1.0, 0.0, 0.0, 1.0, -1.0, 2.0),
		t(2.0, 0.0, 0.0, -3.0, 0.0, 0.0),
	];
	let expected_contours: [[Point; 3]; 3] = [
		[(0.0, 0.0), (0.0, 2.0), (3.0, 0.0)],
		[(-1.0, 2.0), (-1.0, 3.0), (0.0, 2.0)],
		[(0.0, 0.0), (0.0, -3.0), (2.0, 0.0)],
	];
	let mut pen = TtGlyphPen::new(Some(&set));
	for &c in &components {
		pen.add_component("a", c)?;
	}
	let composite = pen.glyph(&o)?;
	for c in expected_contours {
		pen.move_to(c[0])?;
		pen.line_to(c[1])?;
		pen.line_to(c[2])?;
		pen.close_path()?;
	}
	assert_eq!(pen.glyph(&o)?, composite);

	let mut pen = TtGlyphPointPen::new(Some(&set));
	for &c in &components {
		pen.add_component("a", c, None)?;
	}
	let composite = pen.glyph(&o)?;
	for c in expected_contours {
		pen.begin_path(None)?;
		for p in c {
			pen.add_point(p, LINE, false, None, None)?;
		}
		pen.end_path()?;
	}
	assert_eq!(pen.glyph(&o)?, composite);
	Ok(())
}

/// A glyph set that only knows which glyphs exist, for composite glyphs that
/// are not decomposed.
struct Names(&'static [&'static str]);

impl PenGlyphSet for Names {
	fn draw_glyph(&self, name: &str, _: &mut dyn Pen) -> PenResult<bool> {
		Ok(self.0.contains(&name))
	}
}

/// A `glyf` table of named glyphs, as the dict glyph sets of the tests.
fn glyf_of(glyphs: Vec<(&str, Glyph)>) -> (Glyf, GlyphOrder) {
	let order = order(&glyphs.iter().map(|(n, _)| *n).collect::<Vec<_>>());
	let mut glyf = Glyf::default();
	for (i, (_, g)) in glyphs.into_iter().enumerate() {
		glyf.glyphs.insert(i as u32, g);
	}
	(glyf, order)
}

fn bounds(glyf: &mut Glyf, order: &GlyphOrder, name: &str) -> Result<[i64; 4]> {
	let gid = order.id(name).unwrap_or(u32::MAX);
	glyf.recalc_glyph_bounds(gid, order)?;
	let g = &glyf.glyphs[&gid];
	Ok([g.x_min, g.y_min, g.x_max, g.y_max])
}

#[test]
fn test_no_handle_overflowing_transform() -> Result<()> {
	let (set, o) = triangle_set()?;
	let mut pen = TtGlyphPen::new(Some(&set));
	pen.handle_overflowing_transforms = false;
	pen.add_component("a", t(3.0, 0.0, 0.0, 1.0, 0.0, 0.0))?;
	let composite = pen.glyph(&o)?;
	assert_eq!(
		composite.components[0].transform,
		Some([[3.0, 0.0], [0.0, 1.0]])
	);

	let mut pen = TtGlyphPen::new(None);
	triangle(&mut pen, 0.0)?;
	let base = pen.glyph(&o)?;
	let (mut glyf, order) = glyf_of(vec![("a", base), ("b", composite)]);
	// `struct.error` when compiling.
	assert!(matches!(
		glyf.glyph_data(1, &order, true),
		Err(fontoxide_ttlib::Error::Struct(_))
	));
	Ok(())
}

#[test]
fn test_round_float_coordinates_and_component_offsets() -> Result<()> {
	let o = order(&["a", "b"]);
	for point in [false, true] {
		let draw = |dx: f64| -> Result<Glyph> {
			if point {
				let mut pen = TtGlyphPointPen::new(None);
				pen.begin_path(None)?;
				pen.add_point((0.0, 0.0), LINE, false, None, None)?;
				pen.add_point((0.0, 1.0), LINE, false, None, None)?;
				pen.add_point((dx, 0.0), LINE, false, None, None)?;
				pen.end_path()?;
				pen.glyph(&o)
			} else {
				let mut pen = TtGlyphPen::new(None);
				pen.move_to((0.0, 0.0))?;
				pen.line_to((0.0, 1.0))?;
				pen.line_to((dx, 0.0))?;
				pen.close_path()?;
				pen.glyph(&o)
			}
		};
		let simple = draw(367.6)?;
		let set = LineGlyphs(BTreeMap::from([(
			"a".to_owned(),
			simple.coordinates.clone(),
		)]));
		let composite = if point {
			let mut pen = TtGlyphPointPen::new(Some(&set));
			pen.add_component("a", t(1.0, 0.0, 0.0, 1.0, -86.4, 0.0), None)?;
			pen.glyph(&o)?
		} else {
			let mut pen = TtGlyphPen::new(Some(&set));
			pen.add_component("a", t(1.0, 0.0, 0.0, 1.0, -86.4, 0.0))?;
			pen.glyph(&o)?
		};
		let (mut glyf, order) = glyf_of(vec![("a", simple), ("b", composite)]);
		assert_eq!(bounds(&mut glyf, &order, "a")?, [0, 0, 368, 1]);
		assert_eq!(bounds(&mut glyf, &order, "b")?, [-86, 0, 282, 1]);
	}
	Ok(())
}

#[test]
fn test_scaled_component_bounds() -> Result<()> {
	let o = order(&["gravecomb", "circumflexcomb", "uni0302_uni0300"]);
	let rect = |x0: f64, x1: f64| -> Result<Glyph> {
		let mut pen = TtGlyphPen::new(None);
		pen.move_to((x0, 939.0))?;
		pen.line_to((x1, 939.0))?;
		pen.line_to((x1, 745.0))?;
		pen.line_to((x0, 745.0))?;
		pen.close_path()?;
		pen.glyph(&o)
	};
	let grave = rect(-231.0, -55.0)?;
	let circumflex = rect(-278.0, 8.0)?;
	let set = LineGlyphs(BTreeMap::from([
		("gravecomb".to_owned(), grave.coordinates.clone()),
		("circumflexcomb".to_owned(), circumflex.coordinates.clone()),
	]));
	let mut pen = TtGlyphPen::new(Some(&set));
	pen.add_component("circumflexcomb", Transform::default())?;
	pen.add_component("gravecomb", t(0.9, 0.0, 0.0, 0.9, 198.0, 180.0))?;
	let composite = pen.glyph(&o)?;
	let (mut glyf, order) = glyf_of(vec![
		("gravecomb", grave),
		("circumflexcomb", circumflex),
		("uni0302_uni0300", composite),
	]);
	assert_eq!(
		bounds(&mut glyf, &order, "uni0302_uni0300")?,
		[-278, 745, 148, 1025]
	);
	Ok(())
}

#[test]
fn test_output_implied_closing_line() -> Result<()> {
	let o = order(&[]);
	for (implied, n) in [(false, 3), (true, 4)] {
		let mut pen = TtGlyphPen::new(None);
		pen.output_implied_closing_line = implied;
		pen.move_to((0.0, 0.0))?;
		pen.line_to((10.0, 0.0))?;
		pen.line_to((0.0, 10.0))?;
		pen.line_to((0.0, 0.0))?;
		pen.close_path()?;
		assert_eq!(pen.glyph(&o)?.coordinates.len(), n);
	}
	Ok(())
}

#[test]
fn test_point_pen_glyph_simple_and_curve() -> Result<()> {
	let o = order(&[]);
	let mut pen = TtGlyphPointPen::new(None);
	pen.begin_path(None)?;
	for p in [(50.0, 0.0), (450.0, 0.0), (450.0, 700.0), (50.0, 700.0)] {
		pen.add_point(p, LINE, false, None, None)?;
	}
	pen.end_path()?;
	let glyph = pen.glyph(&o)?;
	assert_eq!(glyph.number_of_contours, 1);
	assert_eq!(glyph.end_pts_of_contours, [3]);

	// `test_addPoint_noErrorOnCurve`.
	let mut pen = TtGlyphPointPen::new(None);
	pen.begin_path(None)?;
	pen.add_point((0.0, 0.0), Some(SegmentType::Curve), false, None, None)?;
	pen.end_path()?;
	Ok(())
}

#[test]
fn test_unrounded_coords_rounded_bounds() -> Result<()> {
	// Faustina-Italic.glyphs; googlefonts/fontc#1206.
	let o = order(&["hungarumlaut.case", "dblgravecomb.case"]);
	let names = Names(&["hungarumlaut.case"]);
	let mut pen = TtGlyphPointPen::new(Some(&names));
	let q = Some(SegmentType::QCurve);
	let contours = [
		[
			((235.0, 680.0), LINE),
			((292.0, 729.0), LINE),
			((314.5, 748.5), None),
			((342.0, 780.75), None),
			((342.0, 792.0), q),
			((342.0, 800.0), None),
			((336.66666666666663, 815.9166666666666), None),
			((318.0, 829.5), None),
			((298.0, 834.0), q),
			((209.0, 702.0), LINE),
		],
		[
			((90.0, 680.0), LINE),
			((147.0, 729.0), LINE),
			((169.5, 748.5), None),
			((197.0, 780.75), None),
			((197.0, 792.0), q),
			((197.0, 800.0), None),
			((191.66666666666663, 815.9166666666666), None),
			((173.0, 829.5), None),
			((153.0, 834.0), q),
			((64.0, 702.0), LINE),
		],
	];
	for contour in contours {
		pen.begin_path(None)?;
		for (p, st) in contour {
			pen.add_point(p, st, false, None, None)?;
		}
		pen.end_path()?;
	}
	// `round=noRound` keeps the coordinates unrounded.
	let no: fn(f64) -> f64 = |v| v;
	let base = pen.glyph_with(
		&o,
		&GlyphOptions {
			drop_implied_on_curves: true,
			round: no,
			..GlyphOptions::default()
		},
	)?;
	// A shear and a reflection.
	pen.add_component(
		"hungarumlaut.case",
		t(-1.0, 0.0, 0.28108, 1.0, 196.0, 0.0),
		None,
	)?;
	let composite = pen.glyph_with(
		&o,
		&GlyphOptions {
			round: no,
			..GlyphOptions::default()
		},
	)?;
	let (mut glyf, order) = glyf_of(vec![
		("hungarumlaut.case", base),
		("dblgravecomb.case", composite),
	]);
	assert_eq!(
		bounds(&mut glyf, &order, "dblgravecomb.case")?,
		[74, 680, 329, 834]
	);
	Ok(())
}

#[test]
fn test_composite_bounds_with_nested_components() -> Result<()> {
	// Joan.glyphs: the rounding happens only at the end.
	let o = order(&["period", "bullet", "bullet.sc"]);
	let names = Names(&["period", "bullet"]);
	let mut pen = TtGlyphPointPen::new(Some(&names));
	pen.begin_path(None)?;
	let q = Some(SegmentType::QCurve);
	for (p, st) in [
		((141.0, -8.0), q),
		((168.0, -8.0), None),
		((205.0, 30.5), None),
		((205.0, 56.0), q),
		((205.0, 82.25), None),
		((168.0, 118.0), None),
		((141.0, 118.0), q),
		((114.0, 118.0), None),
		((78.0, 79.5), None),
		((78.0, 54.0), q),
		((78.0, 27.75), None),
		((114.0, -8.0), None),
	] {
		pen.add_point(p, st, false, None, None)?;
	}
	pen.end_path()?;
	let period = pen.glyph(&o)?;
	pen.add_component("period", t(1.6, 0.0, 0.0, 1.6, -41.0, 140.0), None)?;
	let bullet = pen.glyph(&o)?;
	pen.add_component("bullet", t(0.9, 0.0, 0.0, 0.9, 9.0, 49.0), None)?;
	let bullet_sc = pen.glyph(&o)?;
	let (mut glyf, order) = glyf_of(vec![
		("period", period),
		("bullet", bullet),
		("bullet.sc", bullet_sc),
	]);
	let (coords, _, _) = glyf.coordinates(2, &order, false)?;
	let x_min = coords.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
	assert!((x_min - 84.42033).abs() < 1e-5);
	assert_eq!(bounds(&mut glyf, &order, "bullet.sc")?, [84, 163, 267, 345]);
	Ok(())
}

#[test]
fn test_open_path_starting_with_move_and_empty_contours() -> Result<()> {
	let mut points = Vec::new();
	for first in [Some(SegmentType::Move), LINE] {
		let mut pen = TtGlyphPointPen::new(None);
		pen.begin_path(None)?;
		pen.add_point((0.0, 0.0), first, false, None, None)?;
		pen.add_point((10.0, 10.0), LINE, false, None, None)?;
		pen.add_point((20.0, 20.0), None, false, None, None)?;
		pen.add_point((20.0, 0.0), Some(SegmentType::QCurve), false, None, None)?;
		pen.end_path()?;
		let o = order(&[]);
		let g = pen.glyph(&o)?;
		points.push((g.coordinates, g.flags));
	}
	assert_eq!(points[0], points[1]);
	assert_eq!(
		points[0].0,
		[(0.0, 0.0), (10.0, 10.0), (20.0, 20.0), (20.0, 0.0)]
	);
	assert_eq!(points[0].1, [1, 1, 0, 1]);

	let mut pen = TtGlyphPointPen::new(None);
	for _ in 0..2 {
		pen.begin_path(None)?;
		pen.end_path()?;
	}
	assert_eq!(pen.glyph(&order(&[]))?.number_of_contours, 0);
	Ok(())
}

/// `CubicGlyfTest.test_cubic_wrap_stays_in_current_contour`.
#[test]
fn test_cubic_wrap_stays_in_current_contour() -> Result<()> {
	let o = order(&[]);
	// `previous`: None, "line" or "qcurve" (a contour of off-curve points).
	for (prefix, st) in [(false, None), (true, LINE), (true, None)] {
		let mut pen = TtGlyphPointPen::new(None);
		let mut prefix_flags = vec![];
		if prefix {
			pen.begin_path(None)?;
			for p in [(0.0, 0.0), (100.0, 0.0), (0.0, 100.0)] {
				pen.add_point(p, st, false, None, None)?;
			}
			pen.end_path()?;
			prefix_flags = vec![if st.is_some() { 1 } else { 0 }; 3];
		}
		pen.begin_path(None)?;
		pen.add_point((0.0, 200.0), None, false, None, None)?;
		pen.add_point((100.0, 300.0), None, false, None, None)?;
		pen.add_point((200.0, 200.0), Some(SegmentType::Curve), false, None, None)?;
		pen.add_point((300.0, 100.0), None, false, None, None)?;
		pen.add_point((0.0, 100.0), None, false, None, None)?;
		pen.end_path()?;
		let glyph = pen.glyph(&o)?;
		let mut expected = prefix_flags;
		expected.extend([128, 128, 1, 128, 128]);
		assert_eq!(glyph.flags, expected);

		let mut rec = RecordingPen::new();
		glyph.draw(&o, &mut rec, 0.0)?;
		let n = rec.value.len();
		assert_eq!(
			rec.value[n - 4..],
			[
				PenOp::MoveTo((200.0, 200.0)),
				PenOp::CurveTo(vec![(300.0, 100.0), (0.0, 100.0), (0.0, 150.0)]),
				PenOp::CurveTo(vec![(0.0, 200.0), (100.0, 300.0), (200.0, 200.0)]),
				PenOp::ClosePath,
			]
		);
	}
	Ok(())
}

/// `CubicGlyfTest.test_cubic_simple`.
#[test]
fn test_cubic_simple() -> Result<()> {
	let o = order(&[]);
	let mut spen = TtGlyphPen::new(None);
	spen.move_to((0.0, 0.0))?;
	spen.curve_to(&[(0.0, 1.0), (1.0, 1.0), (1.0, 0.0)])?;
	spen.close_path()?;
	let mut ppen = TtGlyphPointPen::new(None);
	ppen.begin_path(None)?;
	ppen.add_point((0.0, 0.0), LINE, false, None, None)?;
	ppen.add_point((0.0, 1.0), None, false, None, None)?;
	ppen.add_point((1.0, 1.0), None, false, None, None)?;
	ppen.add_point((1.0, 0.0), Some(SegmentType::Curve), false, None, None)?;
	ppen.end_path()?;
	for glyph in [spen.glyph(&o)?, ppen.glyph(&o)?] {
		assert_eq!(
			glyph.coordinates,
			[(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)]
		);
		assert_eq!(glyph.flags, [0x01, 0x80, 0x80, 0x01]);
		let mut rec = RecordingPen::new();
		glyph.draw(&o, &mut rec, 0.0)?;
		assert_eq!(
			rec.value,
			[
				PenOp::MoveTo((0.0, 0.0)),
				PenOp::CurveTo(vec![(0.0, 1.0), (1.0, 1.0), (1.0, 0.0)]),
				PenOp::ClosePath
			]
		);
	}
	Ok(())
}

/// `CubicGlyfTest.test_cubic_topology`.
#[test]
fn test_cubic_topology() -> Result<()> {
	let o = order(&[]);
	let c1 = [(0.0, 1.0), (1.0, 2.0), (2.0, 2.0)];
	let two_no_merge = vec![
		PenOp::MoveTo((0.0, 0.0)),
		PenOp::CurveTo(c1.to_vec()),
		PenOp::CurveTo(vec![(3.0, 3.0), (4.0, 1.0), (4.0, 0.0)]),
		PenOp::ClosePath,
	];
	let two_merge = vec![
		PenOp::MoveTo((0.0, 0.0)),
		PenOp::CurveTo(c1.to_vec()),
		PenOp::CurveTo(vec![(3.0, 2.0), (4.0, 1.0), (4.0, 0.0)]),
		PenOp::ClosePath,
	];
	let mut twice = two_merge.clone();
	twice.extend(two_merge.clone());
	// dropImpliedOnCurves, the commands, and the expected coordinates, flags
	// and contour ends.
	type Case = (bool, Vec<PenOp>, Vec<Point>, Vec<u8>, Vec<i64>);
	let cases: Vec<Case> = vec![
		(
			true,
			two_no_merge,
			vec![
				(0.0, 0.0),
				(0.0, 1.0),
				(1.0, 2.0),
				(2.0, 2.0),
				(3.0, 3.0),
				(4.0, 1.0),
				(4.0, 0.0),
			],
			vec![1, 0x80, 0x80, 1, 0x80, 0x80, 1],
			vec![6],
		),
		(
			true,
			two_merge.clone(),
			vec![
				(0.0, 0.0),
				(0.0, 1.0),
				(1.0, 2.0),
				(3.0, 2.0),
				(4.0, 1.0),
				(4.0, 0.0),
			],
			vec![1, 0x80, 0x80, 0x80, 0x80, 1],
			vec![5],
		),
		(
			false,
			two_merge.clone(),
			vec![
				(0.0, 0.0),
				(0.0, 1.0),
				(1.0, 2.0),
				(2.0, 2.0),
				(3.0, 2.0),
				(4.0, 1.0),
				(4.0, 0.0),
			],
			vec![1, 0x80, 0x80, 1, 0x80, 0x80, 1],
			vec![6],
		),
		(
			true,
			twice,
			[
				(0.0, 0.0),
				(0.0, 1.0),
				(1.0, 2.0),
				(3.0, 2.0),
				(4.0, 1.0),
				(4.0, 0.0),
			]
			.repeat(2),
			[1, 0x80, 0x80, 0x80, 0x80, 1].repeat(2),
			vec![5, 11],
		),
	];
	for (drop, commands, coords, flags, ends) in cases {
		let options = GlyphOptions {
			drop_implied_on_curves: drop,
			..GlyphOptions::default()
		};
		let mut spen = TtGlyphPen::new(None);
		replay_recording(&commands, &mut spen)?;
		// The point pen version of the commands.
		let mut points = RecordingPointPen::new();
		{
			let mut seg = fontoxide_pens::SegmentToPointPen::new(&mut points, false);
			replay_recording(&commands, &mut seg)?;
		}
		let mut ppen = TtGlyphPointPen::new(None);
		for op in &points.value {
			op.replay(&mut ppen)?;
		}
		for glyph in [
			spen.glyph_with(&o, &options)?,
			ppen.glyph_with(&o, &options)?,
		] {
			assert_eq!(glyph.coordinates, coords);
			assert_eq!(glyph.flags, flags);
			assert_eq!(glyph.end_pts_of_contours, ends);
			let mut rec = RecordingPen::new();
			glyph.draw(&o, &mut rec, 0.0)?;
			assert_eq!(rec.value, commands);
		}
	}
	Ok(())
}

/// `RoundingPenTest.test_all_offcurve_glyph`.
#[test]
fn test_rounding_all_offcurve_glyph() -> Result<()> {
	let o = order(&[]);
	let cases: [(RoundFunc, [Point; 4]); 3] = [
		(
			ot_round_f64,
			[(0.0, 1.0), (11.0, 1.0), (11.0, 13.0), (0.0, 12.0)],
		),
		(
			f64::round_ties_even,
			[(0.0, 1.0), (11.0, 0.0), (11.0, 13.0), (0.0, 12.0)],
		),
		(
			no_round,
			[(0.4, 0.6), (10.6, 0.5), (11.4, 12.6), (0.4, 11.6)],
		),
	];
	for (round, expected) in cases {
		let mut pen = TtGlyphPen::new(None);
		pen.q_curve_to(&[(0.4, 0.6), (10.6, 0.5), (11.4, 12.6), (0.4, 11.6)], None)?;
		pen.close_path()?;
		let glyph = pen.glyph_with(
			&o,
			&GlyphOptions {
				round: no_round,
				..GlyphOptions::default()
			},
		)?;
		let mut rec = RecordingPen::new();
		{
			let mut rpen = RoundingPen::new(&mut rec);
			rpen.round_func = round;
			glyph.draw(&o, &mut rpen, 0.0)?;
		}
		assert_eq!(
			rec.value,
			[PenOp::QCurveTo(expected.to_vec(), None), PenOp::ClosePath]
		);
	}
	Ok(())
}

// `TTGlyphPenTestBase.runEndToEnd`: each glyph drawn onto the pen compiles to
// the same data.

fn run_end_to_end(file: &str, point: bool) -> Result<()> {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttLib/data")
		.join(file);
	let text = std::fs::read_to_string(&path).map_err(fontoxide_ttlib::Error::Io)?;
	let mut font = Font::from_ttx(&text)?;
	let order = font.glyph_order()?;
	let mut new_glyphs = Vec::new();
	{
		let gs = GlyphSet::new(&mut font)?;
		for name in order.names() {
			let glyph = if point {
				let mut pen = TtGlyphPointPen::new(Some(&gs));
				gs.draw_glyph_points(name, &mut pen)?;
				pen.glyph(&order)?
			} else {
				let mut pen = TtGlyphPen::new(Some(&gs));
				gs.draw_glyph(name, &mut pen)?;
				pen.glyph(&order)?
			};
			new_glyphs.push(glyph);
		}
	}
	let glyf = font.require::<Glyf>()?.clone();
	for (gid, mut new) in new_glyphs.into_iter().enumerate() {
		let gid = gid as u32;
		let mut old_table = glyf.clone();
		let old = old_table.glyph_data(gid, &order, true)?;
		if let Some(g) = old_table.glyphs.get(&gid)
			&& g.program.is_some()
		{
			new.program = g.program.clone();
		}
		let mut new_table = glyf.clone();
		new_table.glyphs.insert(gid, new);
		let compiled = new_table.glyph_data(gid, &order, true)?;
		assert_eq!(old, compiled, "{file}: {}", order.name(gid));
	}
	Ok(())
}

#[test]
fn test_e2e_lines_and_simple_components() -> Result<()> {
	run_end_to_end("TestTTF-Regular.ttx", false)?;
	run_end_to_end("TestTTF-Regular.ttx", true)
}

#[test]
fn test_e2e_curves_and_component_transforms() -> Result<()> {
	run_end_to_end("TestTTFComplex-Regular.ttx", false)?;
	run_end_to_end("TestTTFComplex-Regular.ttx", true)
}
