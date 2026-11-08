//! A pen transforming coordinates (`fontTools.pens.transformPen`).

use fontoxide_misc::transform::Transform;

use crate::{Pen, Point, Result};

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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::BoundsPen;

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
