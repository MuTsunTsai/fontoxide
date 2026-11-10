//! Ports of fontTools' `Tests/pens/hashPointPen_test.py` and
//! `Tests/pens/roundingPen_test.py` (except `test_all_offcurve_glyph`, which
//! needs `TTGlyphPen`: see fontoxide-ttlib).

use fontoxide_misc::fixed_tools::float_to_fixed_to_float;
use fontoxide_pens::hash_point_pen::HashPointPen;
use fontoxide_pens::recording_pen::{PenOp, PointOp, RecordingPen, RecordingPointPen};
use fontoxide_pens::rounding_pen::{RoundingPen, RoundingPointPen};
use fontoxide_pens::{Error, GlyphSet, Pen, PointPen, Result, SegmentType, Transform};

const LINE: Option<SegmentType> = Some(SegmentType::Line);
const CURVE: Option<SegmentType> = Some(SegmentType::Curve);
const MOVE: Option<SegmentType> = Some(SegmentType::Move);

/// The `_TestGlyph` variants: 1 to 4, and 5 (a component of "b").
fn draw_test_glyph(variant: u8, pen: &mut dyn PointPen) -> Result<()> {
	if variant == 5 {
		return pen.add_component("b", Transform::default(), None);
	}
	pen.begin_path(Some("abc"))?;
	let first = if variant == 4 { MOVE } else { LINE };
	pen.add_point((0.0, 0.0), first, false, Some("start"), Some("0000"))?;
	let second = if variant == 2 {
		(101.0, 10.0)
	} else {
		(10.0, 110.0)
	};
	pen.add_point(second, LINE, false, None, Some("0001"))?;
	if variant == 3 {
		pen.end_path()?;
		pen.begin_path(Some("pth2"))?;
	}
	pen.add_point((50.0, 75.0), None, false, None, Some("0002"))?;
	pen.add_point((60.0, 50.0), None, false, None, Some("0003"))?;
	pen.add_point((50.0, 0.0), CURVE, true, Some("last"), Some("0004"))?;
	pen.end_path()
}

/// A glyph set of `_TestGlyph` variants.
struct Glyphs(&'static [(&'static str, u8)]);

impl GlyphSet for Glyphs {
	fn draw_glyph(&self, _: &str, _: &mut dyn Pen) -> Result<bool> {
		Ok(false)
	}

	fn draw_glyph_points(&self, name: &str, pen: &mut dyn PointPen) -> Result<bool> {
		match self.0.iter().find(|(n, _)| *n == name) {
			Some(&(_, variant)) => {
				draw_test_glyph(variant, pen)?;
				Ok(true)
			}
			None => Ok(false),
		}
	}
}

fn hash_of(variant: u8) -> Result<String> {
	let mut pen = HashPointPen::new(500.0, None);
	draw_test_glyph(variant, &mut pen)?;
	Ok(pen.hash())
}

#[test]
fn test_add_component() -> Result<()> {
	let glyphs = Glyphs(&[("a", 1)]);
	let mut pen = HashPointPen::new(500.0, Some(&glyphs));
	pen.add_component("a", Transform::new(2.0, 0.0, 0.0, 3.0, -10.0, 5.0), None)?;
	assert_eq!(
		pen.hash(),
		"w500[l0+0l10+110o50+75o60+50c50+0|(+2+0+0+3-10+5)]"
	);
	Ok(())
}

#[test]
fn test_nested_components() -> Result<()> {
	// "a" contains "b" as a component.
	let glyphs = Glyphs(&[("a", 5), ("b", 1)]);
	let mut pen = HashPointPen::new(500.0, Some(&glyphs));
	pen.add_component("a", Transform::new(2.0, 0.0, 0.0, 3.0, -10.0, 5.0), None)?;
	assert_eq!(
		pen.hash(),
		"w500[[l0+0l10+110o50+75o60+50c50+0|(+1+0+0+1+0+0)](+2+0+0+3-10+5)]"
	);
	Ok(())
}

#[test]
fn test_outline_and_component() -> Result<()> {
	let glyphs = Glyphs(&[("a", 1)]);
	let mut pen = HashPointPen::new(500.0, Some(&glyphs));
	draw_test_glyph(1, &mut pen)?;
	pen.add_component("a", Transform::new(2.0, 0.0, 0.0, 2.0, -10.0, 5.0), None)?;
	assert_eq!(
		pen.hash(),
		"w500l0+0l10+110o50+75o60+50c50+0|[l0+0l10+110o50+75o60+50c50+0|(+2+0+0+2-10+5)]"
	);
	Ok(())
}

#[test]
fn test_add_component_missing_raises() {
	let glyphs = Glyphs(&[]);
	let mut pen = HashPointPen::new(500.0, Some(&glyphs));
	let e = pen.add_component("a", Transform::default(), None);
	assert!(matches!(e, Err(Error::MissingComponent(name)) if name == "a"));
}

#[test]
fn test_similar_glyphs() -> Result<()> {
	let h1 = hash_of(1)?;
	for variant in [2, 3, 4] {
		assert_ne!(h1, hash_of(variant)?);
	}
	Ok(())
}

#[test]
fn test_glyph_vs_composite() -> Result<()> {
	// A glyph with a component differs from the decomposed glyph.
	let glyphs = Glyphs(&[("a", 1)]);
	let mut pen = HashPointPen::new(500.0, Some(&glyphs));
	pen.add_component("a", Transform::default(), None)?;
	assert_ne!(pen.hash(), hash_of(1)?);
	Ok(())
}

#[test]
fn test_rounding_pen_general() -> Result<()> {
	let mut rec = RecordingPen::new();
	{
		let mut pen = RoundingPen::new(&mut rec);
		pen.move_to((0.4, 0.6))?;
		pen.line_to((1.6, 2.5))?;
		pen.q_curve_to(&[(2.4, 4.6), (3.3, 5.7)], Some((4.9, 6.1)))?;
		pen.curve_to(&[(6.4, 8.6), (7.3, 9.7), (8.9, 10.1)])?;
		pen.add_component("a", Transform::new(1.5, 0.0, 0.0, 1.5, 10.5, -10.5))?;
	}
	assert_eq!(
		rec.value,
		[
			PenOp::MoveTo((0.0, 1.0)),
			PenOp::LineTo((2.0, 3.0)),
			PenOp::QCurveTo(vec![(2.0, 5.0), (3.0, 6.0)], Some((5.0, 6.0))),
			PenOp::CurveTo(vec![(6.0, 9.0), (7.0, 10.0), (9.0, 10.0)]),
			PenOp::AddComponent("a".into(), Transform::new(1.5, 0.0, 0.0, 1.5, 11.0, -10.0)),
		]
	);
	Ok(())
}

fn tt_scale_round(v: f64) -> f64 {
	float_to_fixed_to_float(v, 14)
}

#[test]
fn test_rounding_pen_transform_round() -> Result<()> {
	let mut rec = RecordingPen::new();
	{
		let mut pen = RoundingPen::new(&mut rec);
		pen.transform_round_func = tt_scale_round;
		// 0.913 is a scale of 91.3% in a source editor.
		pen.add_component(
			"a",
			Transform::new(0.9130000305, 0.0, 0.0, -1.0, 10.5, -10.5),
		)?;
	}
	// The value equals its F2Dot14 representation.
	assert_eq!(
		rec.value,
		[PenOp::AddComponent(
			"a".into(),
			Transform::new(0.91302490234375, 0.0, 0.0, -1.0, 11.0, -10.0)
		)]
	);
	Ok(())
}

#[test]
fn test_rounding_point_pen() -> Result<()> {
	let mut rec = RecordingPointPen::new();
	{
		let mut pen = RoundingPointPen::new(&mut rec);
		pen.begin_path(None)?;
		pen.add_point((0.4, 0.6), LINE, false, None, None)?;
		pen.add_point((1.6, 2.5), LINE, false, None, None)?;
		pen.add_point((2.4, 4.6), None, false, None, None)?;
		pen.add_point((3.3, 5.7), None, false, None, None)?;
		pen.add_point((4.9, 6.1), Some(SegmentType::QCurve), false, None, None)?;
		pen.end_path()?;
		pen.add_component("a", Transform::new(1.5, 0.0, 0.0, 1.5, 10.5, -10.5), None)?;
	}
	assert_eq!(
		rec.value,
		[
			PointOp::BeginPath(None),
			PointOp::point((0.0, 1.0), LINE, false),
			PointOp::point((2.0, 3.0), LINE, false),
			PointOp::point((2.0, 5.0), None, false),
			PointOp::point((3.0, 6.0), None, false),
			PointOp::point((5.0, 6.0), Some(SegmentType::QCurve), false),
			PointOp::EndPath,
			PointOp::AddComponent(
				"a".into(),
				Transform::new(1.5, 0.0, 0.0, 1.5, 11.0, -10.0),
				None
			),
		]
	);

	let mut rec = RecordingPointPen::new();
	{
		let mut pen = RoundingPointPen::new(&mut rec);
		pen.transform_round_func = tt_scale_round;
		pen.add_component(
			"a",
			Transform::new(0.913, 0.0, 0.0, -1.0, 10.5, -10.5),
			None,
		)?;
	}
	assert_eq!(
		rec.value,
		[PointOp::AddComponent(
			"a".into(),
			Transform::new(0.91302490234375, 0.0, 0.0, -1.0, 11.0, -10.0),
			None
		)]
	);
	Ok(())
}
