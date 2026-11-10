//! Pens rounding coordinates (`fontTools.pens.roundingPen`).

use fontoxide_misc::round_tools::ot_round_f64;

use crate::{Pen, Point, PointPen, Result, SegmentType, Transform};

/// A rounding function (`roundFunc`).
pub type RoundFunc = fn(f64) -> f64;

/// `noRound`.
pub fn no_round(v: f64) -> f64 {
	v
}

fn round_transform(t: Transform, round: RoundFunc, transform_round: RoundFunc) -> Transform {
	Transform::new(
		transform_round(t.xx),
		transform_round(t.xy),
		transform_round(t.yx),
		transform_round(t.yy),
		round(t.dx),
		round(t.dy),
	)
}

/// Rounds the coordinates and the component offsets with `round_func`
/// (`otRound` by default), and the rest of the component transformations
/// with `transform_round_func` (none by default) (`RoundingPen`).
pub struct RoundingPen<'p> {
	out: &'p mut dyn Pen,
	/// `roundFunc`.
	pub round_func: RoundFunc,
	/// `transformRoundFunc`.
	pub transform_round_func: RoundFunc,
}

impl std::fmt::Debug for RoundingPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("RoundingPen").finish_non_exhaustive()
	}
}

impl<'p> RoundingPen<'p> {
	/// `RoundingPen(outPen)`.
	pub fn new(out: &'p mut dyn Pen) -> Self {
		RoundingPen {
			out,
			round_func: ot_round_f64,
			transform_round_func: no_round,
		}
	}

	fn r(&self, (x, y): Point) -> Point {
		((self.round_func)(x), (self.round_func)(y))
	}
}

impl Pen for RoundingPen<'_> {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		let pt = self.r(pt);
		self.out.move_to(pt)
	}
	fn line_to(&mut self, pt: Point) -> Result<()> {
		let pt = self.r(pt);
		self.out.line_to(pt)
	}
	fn curve_to(&mut self, points: &[Point]) -> Result<()> {
		let pts: Vec<Point> = points.iter().map(|&p| self.r(p)).collect();
		self.out.curve_to(&pts)
	}
	fn q_curve_to(&mut self, off_curves: &[Point], on_curve: Option<Point>) -> Result<()> {
		let pts: Vec<Point> = off_curves.iter().map(|&p| self.r(p)).collect();
		let on = on_curve.map(|p| self.r(p));
		self.out.q_curve_to(&pts, on)
	}
	fn close_path(&mut self) -> Result<()> {
		self.out.close_path()
	}
	fn end_path(&mut self) -> Result<()> {
		self.out.end_path()
	}
	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> Result<()> {
		let t = round_transform(transformation, self.round_func, self.transform_round_func);
		self.out.add_component(glyph_name, t)
	}
}

/// The point pen version of [`RoundingPen`] (`RoundingPointPen`).
pub struct RoundingPointPen<'p> {
	out: &'p mut dyn PointPen,
	/// `roundFunc`.
	pub round_func: RoundFunc,
	/// `transformRoundFunc`.
	pub transform_round_func: RoundFunc,
}

impl std::fmt::Debug for RoundingPointPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("RoundingPointPen").finish_non_exhaustive()
	}
}

impl<'p> RoundingPointPen<'p> {
	/// `RoundingPointPen(outPen)`.
	pub fn new(out: &'p mut dyn PointPen) -> Self {
		RoundingPointPen {
			out,
			round_func: ot_round_f64,
			transform_round_func: no_round,
		}
	}
}

impl PointPen for RoundingPointPen<'_> {
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()> {
		self.out.begin_path(identifier)
	}
	fn end_path(&mut self) -> Result<()> {
		self.out.end_path()
	}
	fn add_point(
		&mut self,
		(x, y): Point,
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		let pt = ((self.round_func)(x), (self.round_func)(y));
		self.out
			.add_point(pt, segment_type, smooth, name, identifier)
	}
	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: Transform,
		identifier: Option<&str>,
	) -> Result<()> {
		let t = round_transform(transformation, self.round_func, self.transform_round_func);
		self.out.add_component(base_glyph_name, t, identifier)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::recording_pen::{PenOp, PointOp, RecordingPen, RecordingPointPen};

	// The doctests of `RoundingPen` and `RoundingPointPen`.
	#[test]
	fn doctests() -> Result<()> {
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

		let mut rec = RecordingPointPen::new();
		{
			let mut pen = RoundingPointPen::new(&mut rec);
			pen.begin_path(None)?;
			pen.add_point((0.4, 0.6), Some(SegmentType::Line), false, None, None)?;
			pen.add_point((1.6, 2.5), Some(SegmentType::Line), false, None, None)?;
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
				PointOp::point((0.0, 1.0), Some(SegmentType::Line), false),
				PointOp::point((2.0, 3.0), Some(SegmentType::Line), false),
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
		Ok(())
	}
}
