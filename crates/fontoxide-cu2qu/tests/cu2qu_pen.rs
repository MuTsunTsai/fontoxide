//! `Tests/pens/cu2quPen_test.py`. The UFO glyphs of `Tests/pens/data` are
//! recorded in `data/cu2qu_glyphs.txt` by `tools/reference/pen_fixtures.py`.

mod common;

use std::collections::BTreeMap;

use common::{Glyph, approx_points, approx_segments, glyphs, ops};
use fontoxide_cu2qu::pens::{Cu2QuMultiPen, Cu2QuPen, Cu2QuPointPen};
use fontoxide_pens::recording_pen::{
	PenOp, PointOp, RecordingPen, RecordingPointPen, replay_recording,
};
use fontoxide_pens::{Pen, Point, PointPen, Result, SegmentType, Transform};

const MAX_ERR: f64 = 1.0;

/// `convert_glyph` with `Cu2QuPen`.
fn cu2qu_segments(
	source: &Glyph,
	reverse: bool,
	stats: Option<&mut BTreeMap<String, usize>>,
) -> Result<Vec<PenOp>> {
	let mut rec = RecordingPen::new();
	{
		let mut pen = Cu2QuPen::new(&mut rec, MAX_ERR, reverse);
		if stats.is_some() {
			pen.stats = Some(BTreeMap::new());
		}
		replay_recording(&source.segments, &mut pen)?;
		if let (Some(out), Some(got)) = (stats, pen.stats) {
			for (k, v) in got {
				*out.entry(k).or_insert(0) += v;
			}
		}
	}
	Ok(rec.value)
}

/// `convert_glyph` with `Cu2QuPointPen`.
fn cu2qu_points(
	source: &Glyph,
	reverse: bool,
	stats: Option<&mut BTreeMap<String, usize>>,
) -> Result<Vec<PointOp>> {
	let mut rec = RecordingPointPen::new();
	{
		let mut pen = Cu2QuPointPen::new(&mut rec, MAX_ERR, reverse);
		if stats.is_some() {
			pen.stats = Some(BTreeMap::new());
		}
		for op in &source.points {
			op.replay(&mut pen)?;
		}
		if let (Some(out), Some(got)) = (stats, pen.stats) {
			for (k, v) in got {
				*out.entry(k).or_insert(0) += v;
			}
		}
	}
	Ok(rec.value)
}

#[test]
fn test_cu2qu_convert_glyphs() -> Result<()> {
	let cubic = glyphs("cubic");
	let quad = glyphs("quadratic");
	// `test_convert_simple_glyph` and `test_convert_mixed_glyph`.
	for name in ["a", "A", "Eacute"] {
		let converted = cu2qu_segments(&cubic[name], false, None)?;
		assert_ne!(converted, cubic[name].segments);
		assert!(approx_segments(&converted, &quad[name].segments), "{name}");

		let converted = cu2qu_points(&cubic[name], false, None)?;
		assert_ne!(converted, cubic[name].points);
		assert!(approx_points(&converted, &quad[name].points), "{name}");
	}
	// `test_convert_composite_glyph`: components do not change.
	assert_eq!(
		cu2qu_segments(&cubic["Aacute"], false, None)?,
		cubic["Aacute"].segments
	);
	assert_eq!(
		cu2qu_points(&cubic["Aacute"], false, None)?,
		cubic["Aacute"].points
	);
	Ok(())
}

#[test]
fn test_cu2qu_reverse_direction() -> Result<()> {
	let cubic = glyphs("cubic");
	for name in ["a", "A", "Eacute"] {
		let normal = cu2qu_segments(&cubic[name], false, None)?;
		let reversed = cu2qu_segments(&cubic[name], true, None)?;
		assert_ne!(normal, reversed);
		let normal = cu2qu_points(&cubic[name], false, None)?;
		let reversed = cu2qu_points(&cubic[name], true, None)?;
		assert_ne!(normal, reversed);
	}
	Ok(())
}

#[test]
fn test_cu2qu_stats() -> Result<()> {
	let cubic = glyphs("cubic");
	let mut stats = BTreeMap::new();
	for g in cubic.values() {
		cu2qu_segments(g, false, Some(&mut stats))?;
	}
	assert!(stats.contains_key("1"));
	let mut stats = BTreeMap::new();
	for g in cubic.values() {
		cu2qu_points(g, false, Some(&mut stats))?;
	}
	assert!(stats.contains_key("1"));
	Ok(())
}

#[test]
fn test_cu2qu_add_component() -> Result<()> {
	let t = Transform::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
	let mut rec = RecordingPen::new();
	Cu2QuPen::new(&mut rec, MAX_ERR, false).add_component("a", t)?;
	assert_eq!(rec.value, [PenOp::AddComponent("a".into(), t)]);
	let mut rec = RecordingPointPen::new();
	Cu2QuPointPen::new(&mut rec, MAX_ERR, false).add_component("a", t, None)?;
	assert_eq!(rec.value, [PointOp::AddComponent("a".into(), t, None)]);
	Ok(())
}

fn cu2qu_draw(draw: impl FnOnce(&mut dyn Pen) -> Result<()>) -> Result<Vec<PenOp>> {
	let mut rec = RecordingPen::new();
	{
		let mut pen = Cu2QuPen::new(&mut rec, MAX_ERR, false);
		pen.move_to((0.0, 0.0))?;
		draw(&mut pen)?;
	}
	Ok(rec.value)
}

#[test]
fn test_cu2qu_pen_point_counts() -> Result<()> {
	// `qCurveTo` passes through; `curveTo` with fewer than 3 points becomes a
	// `qCurveTo`.
	assert_eq!(
		cu2qu_draw(|p| p.q_curve_to(&[], Some((1.0, 1.0))))?,
		ops("M 0 0; Q 1 1")
	);
	assert_eq!(
		cu2qu_draw(|p| p.q_curve_to(&[(1.0, 1.0)], Some((2.0, 2.0))))?,
		ops("M 0 0; Q 1 1 2 2")
	);
	assert_eq!(
		cu2qu_draw(|p| p.curve_to(&[(1.0, 1.0)]))?,
		ops("M 0 0; Q 1 1")
	);
	assert_eq!(
		cu2qu_draw(|p| p.curve_to(&[(1.0, 1.0), (2.0, 2.0)]))?,
		ops("M 0 0; Q 1 1 2 2")
	);
	assert_eq!(
		cu2qu_draw(|p| p.curve_to(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]))?,
		ops("M 0 0; Q 0.75 0.75 2.25 2.25 3 3")
	);
	// A "super Bézier".
	assert_eq!(
		cu2qu_draw(|p| p.curve_to(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0), (4.0, 4.0)]))?,
		ops("M 0 0; Q 0.75 0.75 1.625 1.625 2 2; Q 2.375 2.375 3.25 3.25 4 4")
	);
	Ok(())
}

fn point_pen_output(
	points: &[(Point, Option<SegmentType>, bool, Option<&str>)],
) -> Result<Vec<PointOp>> {
	let mut rec = RecordingPointPen::new();
	{
		let mut pen = Cu2QuPointPen::new(&mut rec, MAX_ERR, false);
		pen.begin_path(None)?;
		for &(pt, st, smooth, name) in points {
			pen.add_point(pt, st, smooth, name, None)?;
		}
		pen.end_path()?;
	}
	Ok(rec.value)
}

fn pp(pt: Point, st: Option<SegmentType>, smooth: bool, name: Option<&str>) -> PointOp {
	PointOp::AddPoint {
		pt,
		segment_type: st,
		smooth,
		name: name.map(str::to_owned),
		identifier: None,
	}
}

const MOVE: Option<SegmentType> = Some(SegmentType::Move);
const CURVE: Option<SegmentType> = Some(SegmentType::Curve);
const QCURVE: Option<SegmentType> = Some(SegmentType::QCurve);

#[test]
fn test_cu2qu_point_pen_super_bezier_curve() -> Result<()> {
	assert_eq!(
		point_pen_output(&[
			((0.0, 0.0), MOVE, false, None),
			((1.0, 1.0), None, false, None),
			((2.0, 2.0), None, false, None),
			((3.0, 3.0), None, false, None),
			((4.0, 4.0), CURVE, false, Some("up")),
		])?,
		[
			PointOp::BeginPath(None),
			pp((0.0, 0.0), MOVE, false, None),
			pp((0.75, 0.75), None, false, None),
			pp((1.625, 1.625), None, false, None),
			pp((2.0, 2.0), QCURVE, true, None),
			pp((2.375, 2.375), None, false, None),
			pp((3.25, 3.25), None, false, None),
			pp((4.0, 4.0), QCURVE, false, Some("up")),
			PointOp::EndPath,
		]
	);
	Ok(())
}

fn on_curves(ops: &[PointOp]) -> Vec<Point> {
	ops.iter()
		.filter_map(|op| match op {
			PointOp::AddPoint {
				pt,
				segment_type: Some(_),
				..
			} => Some(*pt),
			_ => None,
		})
		.collect()
}

/// `test__flushContour_restore_starting_point`, through `addPoint`: the
/// closed contour is the one whose segments the test passes to
/// `_flushContour`.
#[test]
fn test_cu2qu_point_pen_restore_starting_point() -> Result<()> {
	let closed = point_pen_output(&[
		((1.0, 1.0), None, false, None),
		((2.0, 2.0), None, false, None),
		((3.0, 3.0), CURVE, false, None),
		((2.0, 2.0), None, false, None),
		((1.0, 1.0), None, false, None),
		((0.0, 0.0), CURVE, false, None),
	])?;
	let on = on_curves(&closed);
	// The original starting point is restored.
	assert_eq!(on.first(), Some(&(3.0, 3.0)));
	assert_eq!(on.last(), Some(&(0.0, 0.0)));

	let open = point_pen_output(&[
		((0.0, 0.0), MOVE, false, None),
		((1.0, 1.0), None, false, None),
		((2.0, 2.0), None, false, None),
		((3.0, 3.0), CURVE, false, None),
	])?;
	let on = on_curves(&open);
	assert_eq!(on.first(), Some(&(0.0, 0.0)));
	assert_eq!(on.last(), Some(&(3.0, 3.0)));
	Ok(())
}

#[test]
fn test_cu2qu_point_pen_quad_no_oncurve() -> Result<()> {
	assert_eq!(
		point_pen_output(&[
			((1.0, 1.0), None, false, None),
			((2.0, 2.0), None, false, None),
			((3.0, 3.0), None, false, None),
		])?,
		[
			PointOp::BeginPath(None),
			pp((1.0, 1.0), None, false, None),
			pp((2.0, 2.0), None, false, None),
			pp((3.0, 3.0), None, false, None),
			PointOp::EndPath,
		]
	);
	Ok(())
}

#[test]
fn test_cu2qu_multi_pen() -> Result<()> {
	let mut recs = [RecordingPen::new(), RecordingPen::new()];
	{
		let [a, b] = &mut recs;
		let mut pen = Cu2QuMultiPen::new(vec![a, b], 0.1, false);
		pen.move_to(vec![(0.0, 0.0), (0.0, 0.0)])?;
		pen.line_to(vec![(0.0, 1.0), (0.0, 1.0)])?;
		pen.q_curve_to(vec![vec![(0.0, 2.0)], vec![(0.0, 2.0)]])?;
		pen.q_curve_to(vec![
			vec![(0.0, 3.0), (1.0, 3.0)],
			vec![(0.0, 3.0), (1.0, 4.0)],
		])?;
		pen.curve_to(vec![
			[(2.0, 3.0), (0.0, 3.0), (0.0, 0.0)],
			[(1.1, 4.0), (0.0, 4.0), (0.0, 0.0)],
		])?;
		pen.close_path()?;
	}
	assert_eq!(recs[0].value.len(), 6);
	assert_eq!(recs[1].value.len(), 6);
	for (op0, op1) in recs[0].value.iter().zip(&recs[1].value) {
		assert_eq!(op0.operator(), op1.operator());
		assert_ne!(op0.operator(), "curveTo");
	}
	Ok(())
}

#[test]
fn test_cu2qu_all_quadratic_false() -> Result<()> {
	for (curve, expected) in [
		(
			[(0.0, 1.0), (2.0, 1.0), (2.0, 0.0)],
			"M 0 0; C 0 1 2 1 2 0; Z",
		),
		([(2.0, 2.0), (4.0, 2.0), (6.0, 0.0)], "M 0 0; Q 3 3 6 0; Z"),
	] {
		let mut rec = RecordingPen::new();
		{
			let mut pen = Cu2QuPen::new(&mut rec, 0.1, false);
			pen.all_quadratic = false;
			pen.move_to((0.0, 0.0))?;
			pen.curve_to(&curve)?;
			pen.close_path()?;
		}
		assert_eq!(rec.value, ops(expected));
	}

	for (curve, expected) in [
		(
			[(0.0, 1.0), (2.0, 1.0), (2.0, 0.0)],
			vec![
				pp((0.0, 1.0), None, false, None),
				pp((2.0, 1.0), None, false, None),
				pp((2.0, 0.0), CURVE, false, None),
			],
		),
		(
			[(2.0, 2.0), (4.0, 2.0), (6.0, 0.0)],
			vec![
				pp((3.0, 3.0), None, false, None),
				pp((6.0, 0.0), QCURVE, false, None),
			],
		),
	] {
		let mut rec = RecordingPointPen::new();
		{
			let mut pen = Cu2QuPointPen::new(&mut rec, 0.1, false);
			pen.all_quadratic = false;
			pen.begin_path(None)?;
			pen.add_point((0.0, 0.0), MOVE, false, None, None)?;
			pen.add_point(curve[0], None, false, None, None)?;
			pen.add_point(curve[1], None, false, None, None)?;
			pen.add_point(curve[2], CURVE, false, None, None)?;
			pen.end_path()?;
		}
		let mut want = vec![PointOp::BeginPath(None), pp((0.0, 0.0), MOVE, false, None)];
		want.extend(expected);
		want.push(PointOp::EndPath);
		assert_eq!(rec.value, want);
	}
	Ok(())
}
