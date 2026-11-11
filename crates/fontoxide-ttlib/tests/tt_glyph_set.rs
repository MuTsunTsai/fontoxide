//! `Tests/ttLib/ttGlyphSet_test.py`.
//!
//! The bounds of the `VARC` test fonts' glyphs are compared with fontTools in
//! `glyph_set.rs`.

#![allow(non_snake_case)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use fontoxide_misc::round_tools::ot_round;
use fontoxide_pens::recording_pen::{
	DecomposingRecordingPen, DecomposingRecordingPointPen, PenOp, PointOp,
};
use fontoxide_pens::{
	DecomposedTransform, GlyphSet as _, NullPen, Pen, Point, PointPen, RecordingPen,
	RecordingPointPen, SegmentType, Transform, TransformPen, TransformPointPen,
};
use fontoxide_ttlib::beyond64k::{ConvertOptions, upper_tables};
use fontoxide_ttlib::glyph_set::{GlyphMetrics, LerpGlyphSet};
use fontoxide_ttlib::ot::types::{
	AxisIndicesList, ConditionTable, ConditionTableFormat2, ConditionTableFormat3,
	ConditionTableFormat4, ConditionTableFormat5, Coverage, HVAR, VARC, VVAR, VarCompositeGlyphs,
	VarData, VarRegion, VarRegionAxis, VarRegionList, VarStore,
};
use fontoxide_ttlib::ot::varc::{NO_VARIATION_INDEX, VarComponent, VarCompositeGlyph, flags};
use fontoxide_ttlib::ot::varmap::VarIdxMap;
use fontoxide_ttlib::tables::fvar::Axis;
use fontoxide_ttlib::tables::otl::Varc;
use fontoxide_ttlib::tables::{Fvar, Gvar, Hmtx, Hvar, Vhea, Vmtx, Vvar};
use fontoxide_ttlib::var::Location;
use fontoxide_ttlib::{Font, GlyphSet, ReadOptions, Result, Tag, TtxOptions, WriteOptions};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

/// A location in user coordinates, as axis tags and values.
type UserLocation<'a> = &'a [(&'a str, f64)];

fn data_path(name: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttLib/data")
		.join(name)
}

fn read_font(name: &str) -> Result<Font> {
	Font::read(std::fs::read(data_path(name))?, ReadOptions::default())
}

/// `TTFont(BytesIO(font.save()))`.
fn save_and_reload(font: &mut Font) -> Result<Font> {
	Font::read(
		font.write(&WriteOptions::default())?,
		ReadOptions::default(),
	)
}

/// `font.saveXML()` followed by `TTFont().importXML()`.
fn ttx_round_trip(font: &mut Font) -> Result<Font> {
	Font::from_ttx(&font.to_ttx(&TtxOptions::default())?)
}

fn loc(entries: &[(&str, f64)]) -> BTreeMap<String, f64> {
	entries.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
}

/// `font.getGlyphSet(location=location)`.
fn glyph_set<'f>(font: &'f mut Font, location: Option<&[(&str, f64)]>) -> Result<GlyphSet<'f>> {
	match location {
		None => GlyphSet::new(font),
		Some(l) => GlyphSet::with_user_location(font, &loc(l)),
	}
}

/// `font.getGlyphSet(location=location, normalized=True)`.
fn normalized_glyph_set<'f>(font: &'f mut Font, location: &[(&str, f64)]) -> Result<GlyphSet<'f>> {
	GlyphSet::with_location(font, loc(location))
}

fn record(glyph_set: &GlyphSet<'_>, name: &str) -> Result<Vec<PenOp>> {
	let mut pen = RecordingPen::new();
	glyph_set.draw(name, &mut pen)?;
	Ok(pen.value)
}

fn record_decomposed(glyph_set: &GlyphSet<'_>, name: &str) -> Result<Vec<PenOp>> {
	let mut pen = DecomposingRecordingPen::new(glyph_set);
	glyph_set.draw(name, &mut pen)?;
	Ok(pen.recording.value)
}

fn record_points(glyph_set: &GlyphSet<'_>, name: &str) -> Result<Vec<PointOp>> {
	let mut pen = RecordingPointPen::new();
	glyph_set.draw_points(name, &mut pen)?;
	Ok(pen.value)
}

fn m(x: f64, y: f64) -> PenOp {
	PenOp::MoveTo((x, y))
}

fn l(x: f64, y: f64) -> PenOp {
	PenOp::LineTo((x, y))
}

fn c(points: &[Point]) -> PenOp {
	PenOp::CurveTo(points.to_vec())
}

/// `qCurveTo` with an on-curve end point.
fn q(points: &[Point]) -> PenOp {
	let (last, off) = points.split_last().expect("points");
	PenOp::QCurveTo(off.to_vec(), Some(*last))
}

fn z() -> PenOp {
	PenOp::ClosePath
}

/// The points of a recording rounded with `otRound`.
fn rounded(ops: &[PenOp]) -> Vec<(&'static str, Vec<(i64, i64)>)> {
	ops.iter()
		.map(|op| {
			let points = op
				.points()
				.into_iter()
				.flatten()
				.map(|(x, y)| (ot_round(x), ot_round(y)))
				.collect();
			(op.operator(), points)
		})
		.collect()
}

/// A point pen that decomposes components and takes no variable components
/// (`OutlinePointPen`).
struct OutlinePointPen<'g>(DecomposingRecordingPointPen<'g>);

impl PointPen for OutlinePointPen<'_> {
	fn begin_path(&mut self, identifier: Option<&str>) -> fontoxide_pens::Result<()> {
		self.0.begin_path(identifier)
	}

	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		self.0.end_path()
	}

	fn add_point(
		&mut self,
		pt: Point,
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> fontoxide_pens::Result<()> {
		self.0.add_point(pt, segment_type, smooth, name, identifier)
	}

	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: Transform,
		identifier: Option<&str>,
	) -> fontoxide_pens::Result<()> {
		self.0
			.add_component(base_glyph_name, transformation, identifier)
	}
}

fn varc_table(font: &mut Font) -> Result<&mut VARC> {
	Ok(&mut font.require::<Varc>()?.table)
}

fn component(glyph: u32, axis_indices_index: Option<u64>, axis_values: &[f64]) -> VarComponent {
	VarComponent {
		glyph,
		axis_indices_index,
		axis_values: axis_values.to_vec(),
		..Default::default()
	}
}

/// A `VARC` table with the given coverage, axis indices and glyphs, and neither
/// conditions nor variations.
fn new_varc(coverage: &[u32], axis_indices: &[&[i64]], glyphs: Vec<Vec<VarComponent>>) -> Varc {
	Varc {
		table: VARC {
			version: 0x00010000,
			coverage: Some(Box::new(Coverage {
				glyphs: coverage.to_vec(),
			})),
			multi_var_store: None,
			condition_list: None,
			axis_indices_list: Some(Box::new(AxisIndicesList {
				item: axis_indices.iter().map(|i| i.to_vec()).collect(),
			})),
			var_composite_glyphs: Some(Box::new(VarCompositeGlyphs {
				var_composite_glyph: glyphs
					.into_iter()
					.map(|components| VarCompositeGlyph { components })
					.collect(),
			})),
		},
	}
}

#[test]
fn test_varc_reset_pen_location() -> TestResult {
	for initial in [&[][..], &[("wght", 0.75)][..]] {
		for reset in [false, true] {
			for nested in [false, true] {
				for points in [false, true] {
					varc_reset_pen_location(initial, reset, nested, points)?;
				}
			}
		}
	}
	Ok(())
}

fn varc_reset_pen_location(
	initial: &[(&str, f64)],
	reset: bool,
	nested: bool,
	points: bool,
) -> TestResult {
	let mut font = read_font("I.ttf")?;
	let i = font.glyph_id("I")?;
	let mut comp = component(i, Some(0), &[0.25]);
	if reset {
		comp.flags = flags::RESET_UNSPECIFIED_AXES;
	}
	font.insert(Box::new(new_varc(&[0], &[&[1]], vec![vec![comp]])));

	let glyph_set = normalized_glyph_set(&mut font, initial)?;
	let check = || -> TestResult {
		// The component, decomposed.
		let expected: Vec<String> = if points {
			let mut pen = OutlinePointPen(DecomposingRecordingPointPen::new(&glyph_set));
			glyph_set.draw_points(".notdef", &mut pen)?;
			pen.0
				.recording
				.value
				.iter()
				.map(|op| format!("{op:?}"))
				.collect()
		} else {
			record_decomposed(&glyph_set, ".notdef")?
				.iter()
				.map(|op| format!("{op:?}"))
				.collect()
		};
		// The component, as is.
		let (name, transform, location) = if points {
			let recording = record_points(&glyph_set, ".notdef")?;
			assert_eq!(recording.len(), 1);
			let PointOp::AddVarComponent(name, transform, location, None) = &recording[0] else {
				panic!("not addVarComponent: {recording:?}");
			};
			// The point pens of fontoxide take no `resetUnspecifiedAxes` argument.
			(name.clone(), *transform, location.clone())
		} else {
			let recording = record(&glyph_set, ".notdef")?;
			assert_eq!(recording.len(), 1);
			let PenOp::AddVarComponent(name, transform, location, flag) = &recording[0] else {
				panic!("not addVarComponent: {recording:?}");
			};
			assert_eq!(*flag, reset);
			(name.clone(), *transform, location.clone())
		};
		assert_eq!(location, vec![("wdth".to_owned(), 0.25)]);

		// Drawing it where it says gives the same outline.
		let location: Location = location.into_iter().collect();
		let actual: Vec<String> = glyph_set.push_location(&location, reset, || -> Result<_> {
			if points {
				let mut pen = OutlinePointPen(DecomposingRecordingPointPen::new(&glyph_set));
				let mut tpen = TransformPointPen::new(&mut pen, transform.to_transform());
				glyph_set.draw_points(&name, &mut tpen)?;
				Ok(pen
					.0
					.recording
					.value
					.iter()
					.map(|op| format!("{op:?}"))
					.collect())
			} else {
				let mut pen = DecomposingRecordingPen::new(&glyph_set);
				let mut tpen = TransformPen::new(&mut pen, transform.to_transform());
				glyph_set.draw(&name, &mut tpen)?;
				Ok(pen
					.recording
					.value
					.iter()
					.map(|op| format!("{op:?}"))
					.collect())
			}
		})?;
		assert_eq!(actual, expected);
		Ok(())
	};
	if nested {
		glyph_set.push_location(&loc(&[("wght", 0.25)]), false, check)
	} else {
		check()
	}
}

#[test]
fn test_glyphset() -> TestResult {
	let rect = |x0: f64, x1: f64| vec![m(x0, 0.0), l(x1, 0.0), l(x1, 1456.0), l(x0, 1456.0), z()];
	let cases: Vec<(&str, Option<UserLocation<'_>>, Vec<PenOp>)> = vec![
		("I.ttf", None, rect(175.0, 367.0)),
		("I.ttf", Some(&[]), rect(175.0, 367.0)),
		("I.ttf", Some(&[("wght", 100.0)]), rect(175.0, 271.0)),
		("I.ttf", Some(&[("wght", 1000.0)]), rect(128.0, 550.0)),
		(
			"I.ttf",
			Some(&[("wght", 1000.0), ("wdth", 25.0)]),
			rect(140.0, 553.0),
		),
		(
			"I.ttf",
			Some(&[("wght", 1000.0), ("wdth", 50.0)]),
			rect(136.0, 552.0),
		),
		(
			"I.otf",
			Some(&[("wght", 1000.0)]),
			vec![
				m(179.0, 74.0),
				l(28.0, 59.0),
				l(28.0, 0.0),
				l(367.0, 0.0),
				l(367.0, 59.0),
				l(212.0, 74.0),
				l(179.0, 74.0),
				z(),
				m(179.0, 578.0),
				l(212.0, 578.0),
				l(367.0, 593.0),
				l(367.0, 652.0),
				l(28.0, 652.0),
				l(28.0, 593.0),
				l(179.0, 578.0),
				z(),
				m(98.0, 310.0),
				c(&[(98.0, 205.0), (98.0, 101.0), (95.0, 0.0)]),
				l(299.0, 0.0),
				c(&[(296.0, 103.0), (296.0, 207.0), (296.0, 311.0)]),
				l(296.0, 342.0),
				c(&[(296.0, 447.0), (296.0, 551.0), (299.0, 652.0)]),
				l(95.0, 652.0),
				c(&[(98.0, 549.0), (98.0, 445.0), (98.0, 342.0)]),
				l(98.0, 310.0),
				z(),
			],
		),
		(
			// /I has an lsb of 30, but an xMin of 25, so an offset of 5 units
			// applies when drawing the outline (fonttools#2824).
			"issue2824.ttf",
			None,
			vec![
				m(309.0, 180.0),
				q(&[
					(274.0, 151.0),
					(187.0, 136.0),
					(104.0, 166.0),
					(74.0, 201.0),
				]),
				q(&[(45.0, 236.0), (30.0, 323.0), (59.0, 407.0), (95.0, 436.0)]),
				q(&[
					(130.0, 466.0),
					(217.0, 480.0),
					(301.0, 451.0),
					(330.0, 415.0),
				]),
				q(&[
					(360.0, 380.0),
					(374.0, 293.0),
					(345.0, 210.0),
					(309.0, 180.0),
				]),
				z(),
			],
		),
	];
	for (file, location, expected) in cases {
		let mut font = read_font(file)?;
		let glyph_set = glyph_set(&mut font, location)?;
		assert_eq!(glyph_set.keys(), [".notdef", "I"]);
		assert!(glyph_set.contains("I"));
		assert_eq!(glyph_set.len(), 2);
		assert!(!glyph_set.contains("foobar"));
		assert_eq!(record(&glyph_set, "I")?, expected, "{file} {location:?}");
	}
	Ok(())
}

#[test]
fn test_cff2_push_location_inherits_and_restores() -> TestResult {
	let font = read_font("I.otf")?;
	for initial in [&[][..], &[("wght", 0.75), ("CNTR", 0.5)][..]] {
		for reset in [false, true] {
			let mut font1 = font.clone();
			let glyph_set = normalized_glyph_set(&mut font1, initial)?;
			let initial = loc(initial);
			let check = |location: &BTreeMap<String, f64>| -> TestResult {
				assert_eq!(&glyph_set.location(), location);
				let mut font2 = font.clone();
				let expected = GlyphSet::with_location(&mut font2, location.clone())?;
				assert_eq!(record(&glyph_set, "I")?, record(&expected, "I")?);
				Ok(())
			};
			glyph_set.push_location(&loc(&[("wght", 0.25)]), false, || -> TestResult {
				let mut parent = initial.clone();
				parent.insert("wght".into(), 0.25);
				check(&parent)?;
				glyph_set.push_location(&loc(&[("CNTR", 0.25)]), reset, || {
					let mut l = if reset {
						initial.clone()
					} else {
						parent.clone()
					};
					l.insert("CNTR".into(), 0.25);
					check(&l)
				})?;
				check(&parent)?;
				glyph_set.push_location(&loc(&[]), false, || check(&parent))?;
				check(&parent)
			})?;
			check(&initial)?;
		}
	}
	Ok(())
}

#[test]
fn test_cff2_push_location_restores_after_exception() -> TestResult {
	let mut font = read_font("I.otf")?;
	let location = loc(&[("wght", 0.75), ("CNTR", 0.5)]);
	let glyph_set = GlyphSet::with_location(&mut font, location.clone())?;
	let expected = record(&glyph_set, "I")?;

	let result: std::result::Result<(), &str> =
		glyph_set.push_location(&loc(&[("wght", 0.25)]), false, || Err("drawing failed"));
	assert_eq!(result, Err("drawing failed"));

	assert_eq!(glyph_set.location(), location);
	assert_eq!(record(&glyph_set, "I")?, expected);
	Ok(())
}

#[test]
fn test_varc_cff2_nested_locations() -> TestResult {
	for reset in [false, true] {
		let mut font = read_font("I.otf")?;
		let mut expected = Vec::new();
		for weight in [if reset { 0.75 } else { 0.25 }, 0.75] {
			let mut font = font.clone();
			let glyph_set = normalized_glyph_set(&mut font, &[("wght", weight), ("CNTR", 0.25)])?;
			expected.extend(record(&glyph_set, "I")?);
		}

		let i = font.glyph_id("I")?;
		let first = component(i, Some(0), &[0.25]);
		let second = component(i, None, &[]);
		let mut leaf = component(i, Some(1), &[0.25]);
		if reset {
			leaf.flags = flags::RESET_UNSPECIFIED_AXES;
		}
		font.insert(Box::new(new_varc(
			&[0, i],
			&[&[0], &[1]],
			vec![vec![first, second], vec![leaf]],
		)));

		let check = |font: &mut Font| -> TestResult {
			let glyph_set = normalized_glyph_set(font, &[("wght", 0.75), ("CNTR", 0.5)])?;
			assert_eq!(record_decomposed(&glyph_set, ".notdef")?, expected);
			// Drawing again does not depend on the axes of a previous component.
			assert_eq!(record_decomposed(&glyph_set, ".notdef")?, expected);
			Ok(())
		};
		check(&mut font)?;
		check(&mut save_and_reload(&mut font)?)?;
	}
	Ok(())
}

#[test]
fn test_lerp_glyphset() -> TestResult {
	let cases = [
		(
			0.5,
			vec![
				m(151.5, 0.0),
				l(458.5, 0.0),
				l(458.5, 1456.0),
				l(151.5, 1456.0),
				z(),
			],
		),
		(
			0.25,
			vec![
				m(163.25, 0.0),
				l(412.75, 0.0),
				l(412.75, 1456.0),
				l(163.25, 1456.0),
				z(),
			],
		),
	];
	for (factor, expected) in cases {
		let mut font1 = read_font("I.ttf")?;
		let mut font2 = font1.clone();
		let glyph_set1 = glyph_set(&mut font1, Some(&[("wght", 400.0)]))?;
		let glyph_set2 = glyph_set(&mut font2, Some(&[("wght", 1000.0)]))?;
		let glyph_set = LerpGlyphSet {
			glyph_set1: &glyph_set1,
			glyph_set2: &glyph_set2,
			factor,
		};
		assert!(glyph_set.contains("I"));
		assert!(!glyph_set.contains("foobar"));
		let mut pen = RecordingPen::new();
		assert!(glyph_set.draw_glyph("I", &mut pen)?);
		assert_eq!(pen.value, expected);
	}
	Ok(())
}

#[test]
fn test_glyphset_varComposite_components() -> TestResult {
	let mut font = read_font("varc-ac00-ac01.ttf")?;
	let glyph_set = GlyphSet::new(&mut font)?;
	let identity = DecomposedTransform::default();
	assert_eq!(
		record(&glyph_set, "uniAC00")?,
		vec![
			PenOp::AddVarComponent("glyph00003".into(), identity, vec![], false),
			PenOp::AddVarComponent("glyph00005".into(), identity, vec![], false),
		]
	);
	Ok(())
}

#[test]
fn test_glyphset_varComposite_conditional() -> TestResult {
	let mut font = read_font("varc-ac01-conditional.ttf")?;
	assert_eq!(record(&GlyphSet::new(&mut font)?, "uniAC01")?.len(), 2);
	let glyph_set = glyph_set(&mut font, Some(&[("wght", 800.0)]))?;
	assert_eq!(record(&glyph_set, "uniAC01")?.len(), 3);
	Ok(())
}

/// `FontBuilder.setupVerticalMetrics` with the same metrics for all glyphs,
/// and `setupVerticalHeader`.
fn setup_vertical_metrics(font: &mut Font, metric: (u16, i16)) -> Result<()> {
	let count = font.glyph_order()?.len() as u32;
	let mut vmtx = Vmtx::default();
	for gid in 0..count {
		vmtx.set(gid, metric);
	}
	font.insert(Box::new(vmtx));
	font.insert(Box::new(Vhea {
		table_version: 0x00011000,
		..Default::default()
	}));
	Ok(())
}

fn upper(font: &mut Font) -> Result<()> {
	upper_tables(font, &ConvertOptions::default())
}

#[test]
fn test_glyphset_varComposite_uppercase_metrics() -> TestResult {
	for location in [None, Some(&[("wght", 800.0)][..])] {
		let mut font = read_font("varc-ac01-conditional.ttf")?;
		setup_vertical_metrics(&mut font, (1024, 30))?;
		let (expected, expected_metrics) = {
			let glyph_set = glyph_set(&mut font, location)?;
			(
				record_decomposed(&glyph_set, "uniAC01")?,
				glyph_set.metrics("uniAC01")?,
			)
		};
		assert_eq!(expected_metrics.height, Some(1024.0));

		upper(&mut font)?;
		let mut font = save_and_reload(&mut font)?;
		for (lower, upper) in [(b"hmtx", b"HMTX"), (b"vmtx", b"VMTX")] {
			assert!(font.contains(Tag::new(upper)) && !font.contains(Tag::new(lower)));
		}
		let glyph_set = glyph_set(&mut font, location)?;
		assert_eq!(record_decomposed(&glyph_set, "uniAC01")?, expected);
		assert_eq!(glyph_set.metrics("uniAC01")?, expected_metrics);
	}
	Ok(())
}

#[test]
fn test_glyphset_varComposite_uppercase_static_axes() -> TestResult {
	for draw_points in [false, true] {
		let mut font = read_font("varc-static-gvar.ttf")?;
		let glyph = varc_table(&mut font)?
			.coverage
			.as_ref()
			.ok_or("no coverage")?
			.glyphs[0];
		let glyph_name = font.glyph_name(glyph)?;
		let draw = |glyph_set: &GlyphSet<'_>| -> Result<String> {
			Ok(if draw_points {
				let mut pen = DecomposingRecordingPointPen::new(glyph_set);
				glyph_set.draw_points(&glyph_name, &mut pen)?;
				format!("{:?}", pen.recording.value)
			} else {
				format!("{:?}", record_decomposed(glyph_set, &glyph_name)?)
			})
		};
		let expected = draw(&GlyphSet::new(&mut font)?)?;

		upper(&mut font)?;
		let mut font = save_and_reload(&mut font)?;
		assert!(!font.contains(Tag::new(b"fvar")));
		assert!(!font.contains(Tag::new(b"gvar")));
		let axis_count = font.require::<Gvar>()?.axis_count;
		let glyph_set = GlyphSet::new(&mut font)?;
		assert_eq!(glyph_set.axes().len(), 1);
		assert_eq!(axis_count, 1);
		assert_eq!(draw(&glyph_set)?, expected);
	}
	Ok(())
}

type OutlinesAndMetrics = BTreeMap<String, (Vec<PenOp>, GlyphMetrics)>;

fn outlines_and_metrics(font: &mut Font) -> Result<OutlinesAndMetrics> {
	let names = font.glyph_order()?.names().to_vec();
	let glyph_set = GlyphSet::new(font)?;
	names
		.into_iter()
		.map(|name| {
			let outline = record_decomposed(&glyph_set, &name)?;
			let metrics = glyph_set.metrics(&name)?;
			Ok((name, (outline, metrics)))
		})
		.collect()
}

#[test]
fn test_glyphset_cff2_uppercase_metrics() -> TestResult {
	for file in ["I.otf", "../../cffLib/data/varc-short-cff2.otf"] {
		for vertical in [false, true] {
			let mut font = read_font(file)?;
			if vertical {
				setup_vertical_metrics(&mut font, (1024, 30))?;
			} else {
				font.remove(Tag::new(b"vhea"));
				font.remove(Tag::new(b"vmtx"));
			}
			let expected = outlines_and_metrics(&mut font)?;
			upper(&mut font)?;
			assert_eq!(outlines_and_metrics(&mut font)?, expected);
			let mut reloaded = save_and_reload(&mut font)?;
			assert_eq!(outlines_and_metrics(&mut reloaded)?, expected);
		}
	}
	Ok(())
}

/// `buildVarRegionList` with the regions of the first axis.
fn var_store(axis_count: usize, deltas: Vec<Vec<Vec<i64>>>) -> VarStore {
	let mut region = VarRegion {
		var_region_axis: vec![
			VarRegionAxis {
				start_coord: 0.0,
				peak_coord: 0.0,
				end_coord: 0.0,
			};
			axis_count
		],
	};
	region.var_region_axis[0] = VarRegionAxis {
		start_coord: 0.0,
		peak_coord: 1.0,
		end_coord: 1.0,
	};
	VarStore {
		format: 1,
		var_region_list: Some(Box::new(VarRegionList {
			region_axis_count: Some(axis_count as u32),
			region_count: Some(1),
			region: vec![region],
		})),
		var_data_count: Some(deltas.len() as u32),
		var_data: deltas
			.into_iter()
			.map(|item| {
				Some(Box::new(VarData {
					item_count: Some(item.len() as u32),
					// `calculateNumShorts`, for one region.
					num_shorts: u16::from(item.iter().flatten().any(|d| !(-128..=127).contains(d))),
					var_region_count: Some(1),
					var_region_index: vec![0],
					item,
				}))
			})
			.collect(),
	}
}

#[test]
fn test_glyphset_vvar_height() -> TestResult {
	for file in [
		"I.ttf",
		"I.otf",
		"varc-ac01-conditional.ttf",
		"../../cffLib/data/varc-short-cff2.otf",
	] {
		for uppercase in [false, true] {
			for mapped in [false, true] {
				for hvar in [false, true] {
					glyphset_vvar_height(file, uppercase, mapped, hvar)
						.map_err(|e| format!("{file} {uppercase} {mapped} {hvar}: {e}"))?;
				}
			}
		}
	}
	Ok(())
}

fn glyphset_vvar_height(file: &str, uppercase: bool, mapped: bool, hvar: bool) -> TestResult {
	let mut font = read_font(file)?;
	if !font.contains(Tag::new(b"fvar")) {
		// `FontBuilder.setupFvar([("TEST", 0, 0, 1, "Test")], [])`.
		let mut axis = Axis::new(Tag::new(b"TEST"));
		axis.min_value = 0.0;
		axis.max_value = 1.0;
		axis.axis_name_id = 256;
		font.insert(Box::new(Fvar {
			axes: vec![axis],
			..Default::default()
		}));
	}
	let order = font.glyph_order()?.names().to_vec();
	setup_vertical_metrics(&mut font, (1000, 20))?;
	let axes: Vec<String> = font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| a.tag_text())
		.collect();
	let n = order.len();
	let deltas: Vec<i64> = (0..n as i64).map(|gid| 100 + gid * 20).collect();
	let indices: Vec<usize> = if mapped {
		(0..n).rev().collect()
	} else {
		(0..n).collect()
	};
	let vvar = VVAR {
		version: 0x00010000,
		var_store: Some(Box::new(var_store(
			axes.len(),
			vec![deltas.iter().map(|&d| vec![d]).collect()],
		))),
		adv_height_map: mapped.then(|| {
			Box::new(VarIdxMap {
				mapping: indices
					.iter()
					.enumerate()
					.map(|(gid, &i)| (gid as u32, i as u64))
					.collect(),
			})
		}),
		..Default::default()
	};
	font.insert(Box::new(Vvar { table: vvar }));
	if hvar && !font.contains(Tag::new(b"HVAR")) {
		let table = HVAR {
			version: 0x00010000,
			var_store: Some(Box::new(var_store(axes.len(), vec![vec![vec![0]; n]]))),
			..Default::default()
		};
		font.insert(Box::new(Hvar { table }));
	} else if !hvar {
		font.remove(Tag::new(b"HVAR"));
	}
	if uppercase {
		upper(&mut font)?;
	}
	let mut font = save_and_reload(&mut font)?;
	let glyph_set = normalized_glyph_set(&mut font, &[(axes[0].as_str(), 0.5)])?;
	for (gid, name) in order.iter().enumerate() {
		let expected = 1000.0 + deltas[indices[gid]] as f64 / 2.0;
		assert_eq!(glyph_set.metrics(name)?.height, Some(expected), "{name}");
		record_decomposed(&glyph_set, name)?;
		assert_eq!(glyph_set.metrics(name)?.height, Some(expected), "{name}");
	}
	Ok(())
}

/// fontTools monkeypatches `getGlyphID` to give the first glyph a high glyph
/// ID; here the glyph order is extended so that it has that ID.
#[test]
fn test_advance_mapping_high_glyph_id() -> TestResult {
	for gid in [0u32, 0xFFFF, 0x10000, 0x10001] {
		for mapped in [false, true] {
			for uppercase in [false, true] {
				advance_mapping_high_glyph_id(gid, mapped, uppercase)
					.map_err(|e| format!("{gid} {mapped} {uppercase}: {e}"))?;
			}
		}
	}
	Ok(())
}

fn advance_mapping_high_glyph_id(gid: u32, mapped: bool, uppercase: bool) -> TestResult {
	let mut font = read_font("I.otf")?;
	let order = font.glyph_order()?.names().to_vec();
	setup_vertical_metrics(&mut font, (800, 20))?;
	let axis_count = font.require::<Fvar>()?.axes.len();
	let store = var_store(axis_count, vec![vec![vec![10]], vec![vec![100], vec![200]]]);
	let mapping = || {
		mapped.then(|| {
			Box::new(VarIdxMap {
				mapping: (0..order.len() as u32).map(|g| (g, 0x10000)).collect(),
			})
		})
	};
	font.insert(Box::new(Hvar {
		table: HVAR {
			version: 0x00010000,
			var_store: Some(Box::new(store.clone())),
			adv_width_map: mapping(),
			..Default::default()
		},
	}));
	font.insert(Box::new(Vvar {
		table: VVAR {
			version: 0x00010000,
			var_store: Some(Box::new(store)),
			adv_height_map: mapping(),
			..Default::default()
		},
	}));
	if uppercase {
		upper(&mut font)?;
	}
	// Move the first glyph to `gid`, with its metrics and variation indices.
	let advance = font.require::<Hmtx>()?.metrics[&0].0;
	if gid != 0 {
		let mut names: Vec<String> = (0..=gid.max(order.len() as u32 - 1))
			.map(|g| format!("glyph{g:05}"))
			.collect();
		names[gid as usize] = order[0].clone();
		names[0] = "moved".into();
		let hmtx = font.require::<Hmtx>()?;
		let metric = hmtx.metrics[&0];
		hmtx.metrics.insert(gid, metric);
		let vmtx = font.require::<Vmtx>()?;
		let metric = vmtx.metrics[&0];
		vmtx.metrics.insert(gid, metric);
		if mapped {
			if let Some(map) = font.require::<Hvar>()?.table.adv_width_map.as_mut() {
				map.mapping.insert(gid, 0x10000);
			}
			if let Some(map) = font.require::<Vvar>()?.table.adv_height_map.as_mut() {
				map.mapping.insert(gid, 0x10000);
			}
		}
		font.set_glyph_order(names);
	}
	let glyph_set = normalized_glyph_set(&mut font, &[("wght", 0.5)])?;
	let metrics = glyph_set.metrics(&order[0])?;
	let delta = if mapped {
		50.0
	} else if gid == 0 {
		5.0
	} else {
		0.0
	};
	assert_eq!(metrics.width, f64::from(advance) + delta);
	assert_eq!(metrics.height, Some(800.0 + delta));
	Ok(())
}

fn uniac01_component_count(font: &mut Font, location: Option<&[(&str, f64)]>) -> Result<usize> {
	Ok(record(&glyph_set(font, location)?, "uniAC01")?.len())
}

fn set_condition(font: &mut Font, condition: Option<ConditionTable>) -> Result<()> {
	varc_table(font)?
		.condition_list
		.as_mut()
		.expect("ConditionList")
		.condition_table[0] = condition.map(Box::new);
	Ok(())
}

#[test]
fn test_glyphset_varComposite_null_condition() -> TestResult {
	let cases: [(Option<u16>, usize); 4] = [(None, 3), (Some(3), 3), (Some(4), 3), (Some(5), 2)];
	for (format, expected) in cases {
		let mut font = read_font("varc-ac01-conditional.ttf")?;
		let condition = format.map(|format| match format {
			3 => ConditionTable::Format3(ConditionTableFormat3 {
				condition_count: None,
				condition_table: vec![None],
			}),
			4 => ConditionTable::Format4(ConditionTableFormat4 {
				condition_count: None,
				condition_table: vec![None],
			}),
			_ => ConditionTable::Format5(ConditionTableFormat5 {
				condition_table: None,
			}),
		});
		set_condition(&mut font, condition)?;
		let mut font = save_and_reload(&mut font)?;
		assert_eq!(
			uniac01_component_count(&mut font, None)?,
			expected,
			"{format:?}"
		);
	}
	Ok(())
}

#[test]
fn test_glyphset_varComposite_static_value_condition() -> TestResult {
	for (default_value, expected) in [(-1, 2), (0, 2), (1, 3)] {
		for with_store in [false, true] {
			let mut font = read_font("varc-ac01-conditional.ttf")?;
			set_condition(
				&mut font,
				Some(ConditionTable::Format2(ConditionTableFormat2 {
					default_value,
					var_idx: NO_VARIATION_INDEX as u32,
				})),
			)?;
			if !with_store {
				let varc = varc_table(&mut font)?;
				varc.multi_var_store = None;
				for glyph in &mut varc
					.var_composite_glyphs
					.as_mut()
					.ok_or("no glyphs")?
					.var_composite_glyph
				{
					for component in &mut glyph.components {
						component.axis_values_var_index = NO_VARIATION_INDEX;
						component.transform_var_index = NO_VARIATION_INDEX;
					}
				}
			}
			let mut font = save_and_reload(&mut font)?;
			assert_eq!(uniac01_component_count(&mut font, None)?, expected);
		}
	}
	Ok(())
}

fn components(font: &mut Font) -> Result<impl Iterator<Item = &mut VarComponent>> {
	Ok(varc_table(font)?
		.var_composite_glyphs
		.as_mut()
		.expect("VarCompositeGlyphs")
		.var_composite_glyph
		.iter_mut()
		.flat_map(|g| g.components.iter_mut()))
}

#[test]
fn test_varc_xml_implicit_scale_y() -> TestResult {
	for scale in [-0.5, 0.5] {
		let mut font = read_font("varc-ac00-ac01.ttf")?;
		let component = components(&mut font)?.next().ok_or("no component")?;
		component.transform.scale_x = scale;
		component.transform.scale_y = scale;
		component.flags |= flags::HAVE_SCALE_X;
		component.flags &= !flags::HAVE_SCALE_Y;

		let draw = |font: &mut Font| -> Result<Vec<PenOp>> {
			record_decomposed(&GlyphSet::new(font)?, "uniAC00")
		};
		let expected = draw(&mut font)?;
		let mut font = ttx_round_trip(&mut font)?;
		assert_eq!(draw(&mut font)?, expected);
		assert_eq!(draw(&mut save_and_reload(&mut font)?)?, expected);
	}
	Ok(())
}

#[test]
fn test_glyphset_varComposite_negated_condition() -> TestResult {
	let cases: [(Option<UserLocation<'_>>, usize, usize); 4] = [
		(None, 1, 3),
		(Some(&[("wght", 800.0)]), 1, 2),
		(None, 2, 2),
		(Some(&[("wght", 800.0)]), 2, 3),
	];
	for (location, negations, expected) in cases {
		let mut font = read_font("varc-ac01-conditional.ttf")?;
		let conditions = &mut varc_table(&mut font)?
			.condition_list
			.as_mut()
			.ok_or("no ConditionList")?
			.condition_table;
		for _ in 0..negations {
			let negated = ConditionTable::Format5(ConditionTableFormat5 {
				condition_table: conditions[0].take(),
			});
			conditions[0] = Some(Box::new(negated));
		}
		let mut font = save_and_reload(&mut font)?;
		assert_eq!(uniac01_component_count(&mut font, location)?, expected);
	}
	Ok(())
}

fn expected_rounded(ops: &[(&'static str, &[(i64, i64)])]) -> Vec<(&'static str, Vec<(i64, i64)>)> {
	ops.iter().map(|(op, pts)| (*op, pts.to_vec())).collect()
}

#[test]
fn test_glyphset_varComposite1() -> TestResult {
	let mut font = read_font("varc-ac00-ac01.ttf")?;
	let glyph_set = glyph_set(&mut font, Some(&[("wght", 600.0)]))?;
	let expected = expected_rounded(&[
		("moveTo", &[(82, 108)]),
		(
			"qCurveTo",
			&[(188, 138), (350, 240), (461, 384), (518, 567), (518, 678)],
		),
		("lineTo", &[(518, 732)]),
		("lineTo", &[(74, 732)]),
		("lineTo", &[(74, 630)]),
		("lineTo", &[(456, 630)]),
		("lineTo", &[(403, 660)]),
		(
			"qCurveTo",
			&[(403, 575), (358, 431), (267, 314), (128, 225), (34, 194)],
		),
		("closePath", &[]),
		("moveTo", &[(702, 385)]),
		("lineTo", &[(897, 385)]),
		("lineTo", &[(897, 485)]),
		("lineTo", &[(702, 485)]),
		("closePath", &[]),
		("moveTo", &[(641, -92)]),
		("lineTo", &[(752, -92)]),
		("lineTo", &[(752, 813)]),
		("lineTo", &[(641, 813)]),
		("closePath", &[]),
	]);
	assert_eq!(
		rounded(&record_decomposed(&glyph_set, "uniAC00")?),
		expected
	);
	// Drawing twice does not change the component.
	assert_eq!(
		rounded(&record_decomposed(&glyph_set, "uniAC00")?),
		expected
	);
	assert!(!record_points(&glyph_set, "uniAC00")?.is_empty());
	Ok(())
}

#[test]
fn test_glyphset_varComposite2() -> TestResult {
	// This test font has axis variations.
	let mut font = read_font("varc-6868.ttf")?;
	let glyph_set = glyph_set(&mut font, Some(&[("wght", 600.0)]))?;
	let expected = expected_rounded(&[
		("moveTo", &[(460, 565)]),
		(
			"qCurveTo",
			&[
				(482, 577),
				(526, 603),
				(568, 632),
				(607, 663),
				(644, 698),
				(678, 735),
				(708, 775),
				(721, 796),
			],
		),
		("lineTo", &[(632, 835)]),
		(
			"qCurveTo",
			&[
				(621, 817),
				(595, 784),
				(566, 753),
				(534, 724),
				(499, 698),
				(462, 675),
				(423, 653),
				(403, 644),
			],
		),
		("closePath", &[]),
		("moveTo", &[(616, 765)]),
		("lineTo", &[(590, 682)]),
		("lineTo", &[(830, 682)]),
		("lineTo", &[(833, 682)]),
		("lineTo", &[(828, 693)]),
		(
			"qCurveTo",
			&[
				(817, 671),
				(775, 620),
				(709, 571),
				(615, 525),
				(492, 490),
				(413, 480),
			],
		),
		("lineTo", &[(454, 386)]),
		(
			"qCurveTo",
			&[
				(544, 403),
				(687, 455),
				(798, 519),
				(877, 590),
				(926, 655),
				(937, 684),
			],
		),
		("lineTo", &[(937, 765)]),
		("closePath", &[]),
		("moveTo", &[(723, 555)]),
		(
			"qCurveTo",
			&[
				(713, 563),
				(693, 579),
				(672, 595),
				(651, 610),
				(629, 625),
				(606, 638),
				(583, 651),
				(572, 657),
			],
		),
		("lineTo", &[(514, 590)]),
		(
			"qCurveTo",
			&[
				(525, 584),
				(547, 572),
				(568, 559),
				(589, 545),
				(609, 531),
				(629, 516),
				(648, 500),
				(657, 492),
			],
		),
		("closePath", &[]),
		("moveTo", &[(387, 375)]),
		("lineTo", &[(387, 830)]),
		("lineTo", &[(289, 830)]),
		("lineTo", &[(289, 375)]),
		("closePath", &[]),
		("moveTo", &[(96, 383)]),
		(
			"qCurveTo",
			&[
				(116, 390),
				(156, 408),
				(194, 427),
				(231, 449),
				(268, 472),
				(302, 497),
				(335, 525),
				(351, 539),
			],
		),
		("lineTo", &[(307, 610)]),
		(
			"qCurveTo",
			&[
				(291, 597),
				(257, 572),
				(221, 549),
				(185, 528),
				(147, 509),
				(108, 492),
				(69, 476),
				(48, 469),
			],
		),
		("closePath", &[]),
		("moveTo", &[(290, 653)]),
		(
			"qCurveTo",
			&[
				(281, 664),
				(261, 687),
				(240, 708),
				(219, 729),
				(196, 749),
				(173, 768),
				(148, 786),
				(136, 794),
			],
		),
		("lineTo", &[(69, 727)]),
		(
			"qCurveTo",
			&[
				(81, 719),
				(105, 702),
				(129, 684),
				(151, 665),
				(173, 645),
				(193, 625),
				(213, 604),
				(222, 593),
			],
		),
		("closePath", &[]),
		("moveTo", &[(913, -57)]),
		("lineTo", &[(953, 30)]),
		(
			"qCurveTo",
			&[
				(919, 41),
				(854, 68),
				(790, 98),
				(729, 134),
				(671, 173),
				(616, 217),
				(564, 264),
				(540, 290),
			],
		),
		("lineTo", &[(522, 286)]),
		(
			"qCurveTo",
			&[(511, 267), (498, 235), (493, 213), (492, 206)],
		),
		("lineTo", &[(515, 209)]),
		("qCurveTo", &[(569, 146), (695, 45), (835, -32), (913, -57)]),
		("closePath", &[]),
		("moveTo", &[(474, 274)]),
		("lineTo", &[(452, 284)]),
		(
			"qCurveTo",
			&[
				(428, 260),
				(377, 214),
				(323, 172),
				(266, 135),
				(206, 101),
				(144, 71),
				(80, 46),
				(47, 36),
			],
		),
		("lineTo", &[(89, -53)]),
		("qCurveTo", &[(163, -29), (299, 46), (423, 142), (476, 201)]),
		("lineTo", &[(498, 196)]),
		(
			"qCurveTo",
			&[(498, 203), (494, 225), (482, 255), (474, 274)],
		),
		("closePath", &[]),
		("moveTo", &[(450, 250)]),
		("lineTo", &[(550, 250)]),
		("lineTo", &[(550, 379)]),
		("lineTo", &[(450, 379)]),
		("closePath", &[]),
		("moveTo", &[(68, 215)]),
		("lineTo", &[(932, 215)]),
		("lineTo", &[(932, 305)]),
		("lineTo", &[(68, 305)]),
		("closePath", &[]),
		("moveTo", &[(450, -71)]),
		("lineTo", &[(550, -71)]),
		("lineTo", &[(550, -71)]),
		("lineTo", &[(550, 267)]),
		("lineTo", &[(450, 267)]),
		("lineTo", &[(450, -71)]),
		("closePath", &[]),
	]);
	assert_eq!(
		rounded(&record_decomposed(&glyph_set, "uni6868")?),
		expected
	);
	assert!(!record_points(&glyph_set, "uni6868")?.is_empty());
	Ok(())
}

fn point(x: f64, y: f64, segment_type: Option<SegmentType>) -> PointOp {
	PointOp::point((x, y), segment_type, false)
}

#[test]
fn test_cubic_glyf() -> TestResult {
	let mut font = read_font("dot-cubic.ttf")?;
	upper_tables(
		&mut font,
		&ConvertOptions {
			tables: Some(&["glyf", "loca", "maxp", "hhea", "hmtx"]),
			..Default::default()
		},
	)?;
	let glyph_set = GlyphSet::new(&mut font)?;

	let expected = vec![
		m(76.0, 181.0),
		c(&[(103.0, 181.0), (125.0, 158.0), (125.0, 131.0)]),
		c(&[(125.0, 104.0), (103.0, 82.0), (76.0, 82.0)]),
		c(&[(48.0, 82.0), (26.0, 104.0), (26.0, 131.0)]),
		c(&[(26.0, 158.0), (48.0, 181.0), (76.0, 181.0)]),
		z(),
	];
	let curve = Some(SegmentType::Curve);
	let points = |list: &[(f64, f64, Option<SegmentType>)]| -> Vec<PointOp> {
		let mut ops = vec![PointOp::BeginPath(None)];
		ops.extend(list.iter().map(|&(x, y, s)| point(x, y, s)));
		ops.push(PointOp::EndPath);
		ops
	};

	assert_eq!(record(&glyph_set, "one")?, expected);
	assert_eq!(
		record_points(&glyph_set, "one")?,
		points(&[
			(76.0, 181.0, curve),
			(103.0, 181.0, None),
			(125.0, 158.0, None),
			(125.0, 104.0, None),
			(103.0, 82.0, None),
			(76.0, 82.0, curve),
			(48.0, 82.0, None),
			(26.0, 104.0, None),
			(26.0, 158.0, None),
			(48.0, 181.0, None),
		])
	);

	assert_eq!(record(&glyph_set, "two")?, expected);
	assert_eq!(
		record_points(&glyph_set, "two")?,
		points(&[
			(26.0, 158.0, None),
			(48.0, 181.0, None),
			(76.0, 181.0, curve),
			(103.0, 181.0, None),
			(125.0, 158.0, None),
			(125.0, 104.0, None),
			(103.0, 82.0, None),
			(76.0, 82.0, curve),
			(48.0, 82.0, None),
			(26.0, 104.0, None),
		])
	);

	assert_eq!(record(&glyph_set, "three")?, expected);
	assert_eq!(
		record_points(&glyph_set, "three")?,
		points(&[
			(48.0, 82.0, None),
			(26.0, 104.0, None),
			(26.0, 158.0, None),
			(48.0, 181.0, None),
			(76.0, 181.0, curve),
			(103.0, 181.0, None),
			(125.0, 158.0, None),
			(125.0, 104.0, None),
			(103.0, 82.0, None),
			(76.0, 82.0, curve),
		])
	);

	assert_eq!(
		record(&glyph_set, "four")?,
		vec![
			m(75.5, 181.0),
			c(&[(103.0, 181.0), (125.0, 158.0), (125.0, 131.0)]),
			c(&[(125.0, 104.0), (103.0, 82.0), (75.5, 82.0)]),
			c(&[(48.0, 82.0), (26.0, 104.0), (26.0, 131.0)]),
			c(&[(26.0, 158.0), (48.0, 181.0), (75.5, 181.0)]),
			z(),
		]
	);

	// The point protocol cannot tell these from quadratic controls
	// (fonttools#3191).
	let mut pen = RecordingPointPen::new();
	let error = glyph_set
		.draw_points("four", &mut pen)
		.expect_err("all-off-curve cubic");
	assert!(error.to_string().contains("All-off-curve cubic"), "{error}");
	assert!(pen.value.is_empty());
	Ok(())
}

#[test]
fn test_varc_in_memory_component_conditions() -> TestResult {
	for location in [None, Some(&[("wght", 800.0)][..])] {
		for from_xml in [false, true] {
			for points in [false, true] {
				let draw = |font: &mut Font| -> Result<String> {
					let glyph_set = glyph_set(font, location)?;
					Ok(if points {
						format!("{:?}", record_points(&glyph_set, "uniAC01")?)
					} else {
						format!("{:?}", record(&glyph_set, "uniAC01")?)
					})
				};
				let mut font = read_font("varc-ac01-conditional.ttf")?;
				let expected = draw(&mut font)?;
				let mut font = if from_xml {
					ttx_round_trip(&mut font)?
				} else {
					// Constructors and XML import need not set derived binary
					// flags.
					for component in components(&mut font)? {
						component.flags &= !flags::HAVE_CONDITION;
					}
					font
				};
				assert_eq!(draw(&mut font)?, expected);
				assert_eq!(draw(&mut save_and_reload(&mut font)?)?, expected);
			}
		}
	}
	Ok(())
}

#[test]
fn test_varc_remove_component_condition() -> TestResult {
	for location in [None, Some(&[("wght", 800.0)][..])] {
		for roundtrip in [false, true] {
			let mut font = read_font("varc-ac01-conditional.ttf")?;
			let mut removed = 0;
			for component in components(&mut font)? {
				if component.condition_index.take().is_some() {
					removed += 1;
				}
			}
			assert_eq!(removed, 1);
			if roundtrip {
				font = save_and_reload(&mut font)?;
			}
			assert_eq!(uniac01_component_count(&mut font, location)?, 3);
		}
	}
	Ok(())
}

#[test]
fn test_glyf_width_without_hvar() -> TestResult {
	let mut font = Font::from_ttx(&std::fs::read_to_string(data_path("I-512upem.ttx"))?)?;
	let width = |font: &mut Font, location: Option<&[(&str, f64)]>, name: &str| -> Result<f64> {
		Ok(glyph_set(font, location)?.metrics(name)?.width)
	};
	// With HVAR, the width comes from it.
	assert_eq!(width(&mut font, Some(&[("wght", 1000.0)]), "I")?, 170.0);
	// At the default location, it is that of hmtx.
	assert_eq!(width(&mut font, None, "I")?, 136.0);

	font.remove(Tag::new(b"HVAR"));
	assert_eq!(width(&mut font, None, "I")?, 136.0);
	// Without HVAR, the width comes from the phantom points, without drawing
	// first.
	assert_eq!(width(&mut font, Some(&[("wght", 1000.0)]), "I")?, 170.0);
	// The default location is still that of hmtx.
	assert_eq!(width(&mut font, Some(&[("wght", 400.0)]), "I")?, 136.0);
	// .notdef does not vary.
	assert_eq!(
		width(&mut font, Some(&[("wght", 1000.0)]), ".notdef")?,
		227.0
	);
	// Drawing does not change it.
	let glyph_set = glyph_set(&mut font, Some(&[("wght", 1000.0)]))?;
	glyph_set.draw("I", &mut NullPen)?;
	assert_eq!(glyph_set.metrics("I")?.width, 170.0);
	glyph_set.draw_points("I", &mut RecordingPointPen::new())?;
	assert_eq!(glyph_set.metrics("I")?.width, 170.0);
	Ok(())
}

#[test]
fn test_varc_gvar_axes_without_fvar() -> TestResult {
	let mut font = read_font("varc-static-gvar.ttf")?;
	assert!(!font.contains(Tag::new(b"fvar")));
	let expected = vec![m(50.0, 0.0), l(450.0, 0.0), l(250.0, 500.0), z()];
	let check = |font: &mut Font| -> TestResult {
		assert_eq!(record(&GlyphSet::new(font)?, "a")?, expected);
		Ok(())
	};
	check(&mut font)?;
	// The hidden numeric axes survive binary round trips.
	check(&mut save_and_reload(&mut font)?)?;
	let mut roundtripped = ttx_round_trip(&mut font)?;
	check(&mut save_and_reload(&mut roundtripped)?)?;
	Ok(())
}

/// The pens that take a variable component record it rather than having it
/// decomposed.
#[test]
fn pens_without_var_components_get_them_decomposed() -> TestResult {
	let mut font = read_font("varc-ac00-ac01.ttf")?;
	let glyph_set = GlyphSet::new(&mut font)?;
	struct Outline(RecordingPen);
	impl Pen for Outline {
		fn move_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
			self.0.move_to(pt)
		}
		fn line_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
			self.0.line_to(pt)
		}
		fn curve_to(&mut self, points: &[Point]) -> fontoxide_pens::Result<()> {
			self.0.curve_to(points)
		}
		fn q_curve_to(&mut self, off: &[Point], on: Option<Point>) -> fontoxide_pens::Result<()> {
			self.0.q_curve_to(off, on)
		}
		fn close_path(&mut self) -> fontoxide_pens::Result<()> {
			self.0.close_path()
		}
		fn end_path(&mut self) -> fontoxide_pens::Result<()> {
			self.0.end_path()
		}
		fn add_component(&mut self, name: &str, t: Transform) -> fontoxide_pens::Result<()> {
			self.0.add_component(name, t)
		}
	}
	let mut pen = Outline(RecordingPen::new());
	glyph_set.draw("uniAC00", &mut pen)?;
	assert!(
		pen.0
			.value
			.iter()
			.all(|op| !matches!(op, PenOp::AddVarComponent(..)))
	);
	assert_eq!(pen.0.value, record_decomposed(&glyph_set, "uniAC00")?);
	Ok(())
}
