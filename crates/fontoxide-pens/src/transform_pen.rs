//! A pen transforming coordinates (`fontTools.pens.transformPen`).

use fontoxide_misc::transform::Transform;

use crate::{Pen, Point, PointPen, Result, SegmentType};

/// Passes everything on to another pen with the coordinates transformed
/// (`TransformPen`). Components are passed on with the transformations
/// combined.
pub struct TransformPen<'p> {
	out: &'p mut dyn Pen,
	transformation: Transform,
}

impl std::fmt::Debug for TransformPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TransformPen")
			.field("transformation", &self.transformation)
			.finish_non_exhaustive()
	}
}

impl<'p> TransformPen<'p> {
	/// `TransformPen(outPen, transformation)`.
	pub fn new(out: &'p mut dyn Pen, transformation: Transform) -> Self {
		TransformPen {
			out,
			transformation,
		}
	}
}

impl Pen for TransformPen<'_> {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.out.move_to(self.transformation.transform_point(pt))
	}

	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.out.line_to(self.transformation.transform_point(pt))
	}

	fn curve_to(&mut self, points: &[Point]) -> Result<()> {
		self.out
			.curve_to(&self.transformation.transform_points(points))
	}

	fn q_curve_to(&mut self, off_curves: &[Point], on_curve: Option<Point>) -> Result<()> {
		let off_curves = self.transformation.transform_points(off_curves);
		let on_curve = on_curve.map(|p| self.transformation.transform_point(p));
		self.out.q_curve_to(&off_curves, on_curve)
	}

	fn close_path(&mut self) -> Result<()> {
		self.out.close_path()
	}

	fn end_path(&mut self) -> Result<()> {
		self.out.end_path()
	}

	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> Result<()> {
		let t = self.transformation.transform(&transformation);
		self.out.add_component(glyph_name, t)
	}
}

/// Passes everything on to another point pen with the coordinates
/// transformed (`TransformPointPen`).
pub struct TransformPointPen<'p> {
	out: &'p mut dyn PointPen,
	transformation: Transform,
}

impl std::fmt::Debug for TransformPointPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TransformPointPen")
			.field("transformation", &self.transformation)
			.finish_non_exhaustive()
	}
}

impl<'p> TransformPointPen<'p> {
	/// `TransformPointPen(outPointPen, transformation)`.
	pub fn new(out: &'p mut dyn PointPen, transformation: Transform) -> Self {
		TransformPointPen {
			out,
			transformation,
		}
	}
}

impl PointPen for TransformPointPen<'_> {
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()> {
		self.out.begin_path(identifier)
	}

	fn end_path(&mut self) -> Result<()> {
		self.out.end_path()
	}

	fn add_point(
		&mut self,
		pt: Point,
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		self.out.add_point(
			self.transformation.transform_point(pt),
			segment_type,
			smooth,
			name,
			identifier,
		)
	}

	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: Transform,
		identifier: Option<&str>,
	) -> Result<()> {
		let t = self.transformation.transform(&transformation);
		self.out.add_component(base_glyph_name, t, identifier)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::BoundsPen;
	use crate::recording_pen::{PointOp, RecordingPointPen};

	// The doctest of `TransformPointPen`.
	#[test]
	fn transform_point_pen() -> Result<()> {
		let mut rec = RecordingPointPen::new();
		{
			let mut pen =
				TransformPointPen::new(&mut rec, Transform::new(2.0, 0.0, 0.0, 2.0, -10.0, 5.0));
			pen.begin_path(Some("contour-0"))?;
			pen.add_point((100.0, 100.0), Some(SegmentType::Line), false, None, None)?;
			pen.end_path()?;
			pen.add_component(
				"a",
				Transform::new(1.0, 0.0, 0.0, 1.0, -10.0, 5.0),
				Some("component-0"),
			)?;
		}
		assert_eq!(
			rec.value,
			[
				PointOp::BeginPath(Some("contour-0".into())),
				PointOp::point((190.0, 205.0), Some(SegmentType::Line), false),
				PointOp::EndPath,
				PointOp::AddComponent(
					"a".into(),
					Transform::new(2.0, 0.0, 0.0, 2.0, -30.0, 15.0),
					Some("component-0".into())
				),
			]
		);
		Ok(())
	}

	#[test]
	fn transform() -> Result<()> {
		let mut bounds = BoundsPen::new_bounds(None);
		{
			let mut pen =
				TransformPen::new(&mut bounds, Transform::new(2.0, 0.0, 0.0, 3.0, 10.0, 20.0));
			pen.move_to((0.0, 0.0))?;
			pen.line_to((10.0, 10.0))?;
			pen.close_path()?;
		}
		assert_eq!(bounds.bounds(), Some([10.0, 20.0, 30.0, 50.0]));
		Ok(())
	}
}
