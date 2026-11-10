//! Port of fontTools' `Tests/pens/filterPen_test.py`.

use std::collections::HashSet;

use fontoxide_pens::filter_pen::{
	DecomposeFilter, DecomposingFilterPen, DecomposingFilterPointPen, FilterPointPen,
	OnCurveFirstPointPen,
};
use fontoxide_pens::recording_pen::{PenOp, PointOp, RecordingPen, RecordingPointPen};
use fontoxide_pens::{
	Error, GlyphSet, Pen, Point, PointPen, PointToSegmentPen, Result, ReverseFlipped, SegmentType,
	Transform,
};

/// `GLYPHSET`.
struct Glyphs;

impl GlyphSet for Glyphs {
	fn draw_glyph(&self, name: &str, pen: &mut dyn Pen) -> Result<bool> {
		match name {
			"simple_glyph" => {
				pen.move_to((0.0, 0.0))?;
				pen.curve_to(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)])?;
				pen.close_path()?;
			}
			"composite_glyph" => pen.add_component(
				"simple_glyph",
				Transform::new(1.0, 0.0, 0.0, 1.0, -1.0, 1.0),
			)?,
			"missing_component" => pen.add_component("foobar", Transform::default())?,
			"flipped_component" => {
				pen.add_component("simple_glyph", Transform::default())?;
				pen.add_component(
					"composite_glyph",
					Transform::new(-1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
				)?;
			}
			_ => return Ok(false),
		}
		Ok(true)
	}

	fn draw_glyph_points(&self, name: &str, pen: &mut dyn PointPen) -> Result<bool> {
		match name {
			"simple_glyph" => {
				pen.begin_path(None)?;
				pen.add_point((0.0, 0.0), Some(SegmentType::Line), false, None, None)?;
				pen.add_point((1.0, 1.0), None, false, None, None)?;
				pen.add_point((2.0, 2.0), None, false, None, None)?;
				pen.add_point((3.0, 3.0), Some(SegmentType::Curve), false, None, None)?;
				pen.end_path()?;
			}
			"composite_glyph" => pen.add_component(
				"simple_glyph",
				Transform::new(1.0, 0.0, 0.0, 1.0, -1.0, 1.0),
				None,
			)?,
			"missing_component" => pen.add_component("foobar", Transform::default(), None)?,
			"flipped_component" => {
				pen.add_component("simple_glyph", Transform::default(), None)?;
				pen.add_component(
					"composite_glyph",
					Transform::new(-1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
					None,
				)?;
			}
			_ => return Ok(false),
		}
		Ok(true)
	}
}

#[test]
fn test_filter_point_pen_positional_identifier() -> Result<()> {
	let mut rec = RecordingPointPen::new();
	{
		let mut pen = FilterPointPen::new(&mut rec);
		pen.begin_path(Some("glyph1"))?;
		pen.add_point(
			(0.0, 0.0),
			Some(SegmentType::Line),
			false,
			None,
			Some("pt1"),
		)?;
		pen.end_path()?;
	}
	assert_eq!(
		rec.value,
		[
			PointOp::BeginPath(Some("glyph1".into())),
			PointOp::AddPoint {
				pt: (0.0, 0.0),
				segment_type: Some(SegmentType::Line),
				smooth: false,
				name: None,
				identifier: Some("pt1".into())
			},
			PointOp::EndPath,
		]
	);
	Ok(())
}

/// The options of `_init_rec_and_filter_pens`.
#[derive(Default, Clone)]
struct Options {
	skip_missing: bool,
	reverse_flipped: bool,
	components: Option<DecomposeFilter>,
}

/// Draws a glyph with `DecomposingFilterPen`, or with
/// `DecomposingFilterPointPen` onto a `PointToSegmentPen`, and returns the
/// recording.
fn draw(point: bool, glyph: &str, options: &Options) -> Result<Vec<PenOp>> {
	let mut rec = RecordingPen::new();
	let reverse = if options.reverse_flipped {
		ReverseFlipped::KeepStart
	} else {
		ReverseFlipped::No
	};
	if point {
		let mut out = PointToSegmentPen::new(&mut rec, false);
		let mut pen = DecomposingFilterPointPen::new(&mut out, &Glyphs);
		pen.decomposer.skip_missing_components = options.skip_missing;
		pen.decomposer.reverse_flipped = reverse;
		if let Some(c) = &options.components {
			pen.components = c.clone();
		}
		Glyphs.draw_glyph_points(glyph, &mut pen)?;
	} else {
		let mut pen = DecomposingFilterPen::new(&mut rec, &Glyphs);
		pen.decomposer.skip_missing_components = options.skip_missing;
		pen.decomposer.reverse_flipped = reverse;
		if let Some(c) = &options.components {
			pen.components = c.clone();
		}
		Glyphs.draw_glyph(glyph, &mut pen)?;
	}
	Ok(rec.value)
}

fn curve(pts: [Point; 3]) -> PenOp {
	PenOp::CurveTo(pts.to_vec())
}

#[test]
fn test_decomposing_filter_pen() -> Result<()> {
	let o = Options::default();
	for point in [false, true] {
		assert_eq!(
			draw(point, "simple_glyph", &o)?,
			[
				PenOp::MoveTo((0.0, 0.0)),
				curve([(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]),
				PenOp::ClosePath
			]
		);
		assert_eq!(
			draw(point, "composite_glyph", &o)?,
			[
				PenOp::MoveTo((-1.0, 1.0)),
				curve([(0.0, 2.0), (1.0, 3.0), (2.0, 4.0)]),
				PenOp::ClosePath
			]
		);
		assert!(matches!(
			draw(point, "missing_component", &o),
			Err(Error::MissingComponent(_))
		));
		assert_eq!(
			draw(point, "flipped_component", &o)?,
			[
				PenOp::MoveTo((0.0, 0.0)),
				curve([(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]),
				PenOp::ClosePath,
				PenOp::MoveTo((1.0, 1.0)),
				curve([(0.0, 2.0), (-1.0, 3.0), (-2.0, 4.0)]),
				PenOp::ClosePath,
			]
		);
	}
	Ok(())
}

#[test]
fn test_decomposing_filter_pen_skip_missing() -> Result<()> {
	// A warning is logged; nothing is drawn.
	let o = Options {
		skip_missing: true,
		..Options::default()
	};
	for point in [false, true] {
		assert!(draw(point, "missing_component", &o)?.is_empty());
	}
	Ok(())
}

#[test]
fn test_decomposing_filter_pen_reverse_flipped() -> Result<()> {
	let o = Options {
		reverse_flipped: true,
		..Options::default()
	};
	for point in [false, true] {
		assert_eq!(
			draw(point, "flipped_component", &o)?,
			[
				PenOp::MoveTo((0.0, 0.0)),
				curve([(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]),
				PenOp::ClosePath,
				PenOp::MoveTo((1.0, 1.0)),
				PenOp::LineTo((-2.0, 4.0)),
				curve([(-1.0, 3.0), (0.0, 2.0), (1.0, 1.0)]),
				PenOp::ClosePath,
			]
		);
	}
	Ok(())
}

#[test]
fn test_decomposing_filter_pen_include_decompose_nested() -> Result<()> {
	for (decompose_nested, expected) in [
		(
			true,
			vec![
				PenOp::AddComponent("simple_glyph".into(), Transform::default()),
				PenOp::MoveTo((1.0, 1.0)),
				curve([(0.0, 2.0), (-1.0, 3.0), (-2.0, 4.0)]),
				PenOp::ClosePath,
			],
		),
		(
			false,
			vec![
				PenOp::AddComponent("simple_glyph".into(), Transform::default()),
				PenOp::AddComponent(
					"simple_glyph".into(),
					Transform::new(-1.0, 0.0, 0.0, 1.0, 1.0, 1.0),
				),
			],
		),
	] {
		let o = Options {
			components: Some(DecomposeFilter {
				include: Some(HashSet::from(["composite_glyph".to_owned()])),
				decompose_nested,
			}),
			..Options::default()
		};
		for point in [false, true] {
			assert_eq!(draw(point, "flipped_component", &o)?, expected);
		}
	}
	Ok(())
}

fn on_curve_first(draw: impl FnOnce(&mut dyn PointPen) -> Result<()>) -> Result<Vec<PointOp>> {
	let mut rec = RecordingPointPen::new();
	{
		let mut pen = OnCurveFirstPointPen::on_curve_first(&mut rec);
		draw(&mut pen)?;
	}
	Ok(rec.value)
}

fn p(pt: Point, st: Option<SegmentType>) -> PointOp {
	PointOp::point(pt, st, false)
}

#[test]
fn test_on_curve_first_point_pen() -> Result<()> {
	let line = Some(SegmentType::Line);
	let curve = Some(SegmentType::Curve);
	let mv = Some(SegmentType::Move);
	// A closed contour starting with an off-curve point is rotated.
	assert_eq!(
		on_curve_first(|pen| {
			pen.begin_path(None)?;
			pen.add_point((0.0, 0.0), None, false, None, None)?;
			pen.add_point((100.0, 100.0), line, false, None, None)?;
			pen.add_point((200.0, 0.0), None, false, None, None)?;
			pen.add_point((300.0, 100.0), curve, false, None, None)?;
			pen.end_path()
		})?,
		[
			PointOp::BeginPath(None),
			p((100.0, 100.0), line),
			p((200.0, 0.0), None),
			p((300.0, 100.0), curve),
			p((0.0, 0.0), None),
			PointOp::EndPath
		]
	);
	// Already starting with an on-curve point.
	assert_eq!(
		on_curve_first(|pen| {
			pen.begin_path(None)?;
			pen.add_point((100.0, 100.0), line, false, None, None)?;
			pen.add_point((200.0, 0.0), None, false, None, None)?;
			pen.add_point((300.0, 100.0), curve, false, None, None)?;
			pen.end_path()
		})?,
		[
			PointOp::BeginPath(None),
			p((100.0, 100.0), line),
			p((200.0, 0.0), None),
			p((300.0, 100.0), curve),
			PointOp::EndPath
		]
	);
	// An open contour.
	assert_eq!(
		on_curve_first(|pen| {
			pen.begin_path(None)?;
			pen.add_point((0.0, 0.0), mv, false, None, None)?;
			pen.add_point((100.0, 100.0), line, false, None, None)?;
			pen.add_point((200.0, 200.0), curve, false, None, None)?;
			pen.end_path()
		})?,
		[
			PointOp::BeginPath(None),
			p((0.0, 0.0), mv),
			p((100.0, 100.0), line),
			p((200.0, 200.0), curve),
			PointOp::EndPath
		]
	);
	// Off-curve points only.
	assert_eq!(
		on_curve_first(|pen| {
			pen.begin_path(None)?;
			pen.add_point((0.0, 0.0), None, false, None, None)?;
			pen.add_point((100.0, 100.0), None, false, None, None)?;
			pen.add_point((200.0, 0.0), None, false, None, None)?;
			pen.end_path()
		})?,
		[
			PointOp::BeginPath(None),
			p((0.0, 0.0), None),
			p((100.0, 100.0), None),
			p((200.0, 0.0), None),
			PointOp::EndPath
		]
	);
	// Components pass through.
	let t = Transform::new(1.0, 0.0, 0.0, 1.0, 10.0, 20.0);
	assert_eq!(
		on_curve_first(|pen| pen.add_component("a", t, None))?,
		[PointOp::AddComponent("a".into(), t, None)]
	);
	// Identifiers are forwarded.
	let id = |pt: Point, st: Option<SegmentType>, id: &str| PointOp::AddPoint {
		pt,
		segment_type: st,
		smooth: false,
		name: None,
		identifier: Some(id.into()),
	};
	assert_eq!(
		on_curve_first(|pen| {
			pen.begin_path(Some("path1"))?;
			pen.add_point((0.0, 0.0), mv, false, None, Some("start"))?;
			pen.add_point((100.0, 100.0), line, false, None, Some("line-id"))?;
			pen.end_path()
		})?,
		[
			PointOp::BeginPath(Some("path1".into())),
			id((0.0, 0.0), mv, "start"),
			id((100.0, 100.0), line, "line-id"),
			PointOp::EndPath
		]
	);
	Ok(())
}

/// `SimpleGlyphStartingWithOffCurve`.
struct OffCurveFirst;

impl GlyphSet for OffCurveFirst {
	fn draw_glyph(&self, _: &str, _: &mut dyn Pen) -> Result<bool> {
		Ok(false)
	}

	fn draw_glyph_points(&self, name: &str, pen: &mut dyn PointPen) -> Result<bool> {
		if name != "base" {
			return Ok(false);
		}
		pen.begin_path(None)?;
		pen.add_point((0.0, 0.0), None, false, None, None)?;
		pen.add_point((50.0, 50.0), None, false, None, None)?;
		pen.add_point((100.0, 0.0), Some(SegmentType::QCurve), false, None, None)?;
		pen.add_point((50.0, -50.0), None, false, None, None)?;
		pen.end_path()?;
		Ok(true)
	}
}

fn first_point_flipped(reverse_flipped: ReverseFlipped) -> Result<(Point, Option<SegmentType>)> {
	let mut rec = RecordingPointPen::new();
	{
		let mut pen = DecomposingFilterPointPen::new(&mut rec, &OffCurveFirst);
		pen.decomposer.reverse_flipped = reverse_flipped;
		pen.add_component(
			"base",
			Transform::new(-1.0, 0.0, 0.0, 1.0, 200.0, 0.0),
			None,
		)?;
	}
	assert_eq!(rec.value.len(), 6);
	assert_eq!(rec.value[0], PointOp::BeginPath(None));
	match &rec.value[1] {
		PointOp::AddPoint {
			pt, segment_type, ..
		} => Ok((*pt, *segment_type)),
		op => panic!("{op:?}"),
	}
}

#[test]
fn test_decomposing_filter_point_pen_reverse_flipped() -> Result<()> {
	// `keep_start`: reversed, still starting with an off-curve point.
	assert_eq!(
		first_point_flipped(ReverseFlipped::KeepStart)?,
		((200.0, 0.0), None)
	);
	// `on_curve_first`: reversed and rotated to start with an on-curve point.
	assert_eq!(
		first_point_flipped(ReverseFlipped::OnCurveFirst)?,
		((100.0, 0.0), Some(SegmentType::QCurve))
	);
	// `no`: not reversed.
	assert_eq!(
		first_point_flipped(ReverseFlipped::No)?,
		((200.0, 0.0), None)
	);
	Ok(())
}
