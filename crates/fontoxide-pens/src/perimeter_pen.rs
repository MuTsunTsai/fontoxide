//! The perimeter of a glyph (`fontTools.pens.perimeterPen`).

use fontoxide_misc::bezier_tools::{
	approximate_cubic_arc_length_c, approximate_quadratic_arc_length_c, calc_cubic_arc_length_c,
	calc_quadratic_arc_length_c,
};
use fontoxide_misc::complex::Complex;
use fontoxide_misc::py::hypot;

use crate::base_pen::{BasePen, SegmentPen};
use crate::{Error, GlyphSet, Point, Result};

/// Sums the length of the contours (`PerimeterPen`).
#[derive(Debug, Clone)]
pub struct Perimeter {
	/// `value`.
	pub value: f64,
	/// `tolerance`: below 0.0015 cubic curves are measured recursively, and
	/// below 0.00075 quadratic curves exactly; otherwise both are approximated
	/// by quadrature.
	pub tolerance: f64,
	start: Option<Point>,
	/// The current point (`_getCurrentPoint()`).
	current: Option<Point>,
}

/// `PerimeterPen`.
pub type PerimeterPen<'g> = BasePen<'g, Perimeter>;

impl<'g> PerimeterPen<'g> {
	/// `PerimeterPen(glyphset, tolerance)`; the tolerance is 0.005 in
	/// fontTools.
	pub fn new_perimeter(glyph_set: Option<&'g dyn GlyphSet>, tolerance: f64) -> Self {
		BasePen::with_inner(
			Perimeter {
				value: 0.0,
				tolerance,
				start: None,
				current: None,
			},
			glyph_set,
		)
	}

	/// `value`.
	pub fn value(&self) -> f64 {
		self.inner.value
	}
}

impl Perimeter {
	fn current(&self) -> Result<Point> {
		self.current
			.ok_or_else(|| Error::Usage("TypeError: 'NoneType' object is not subscriptable".into()))
	}
}

impl SegmentPen for Perimeter {
	fn move_to(&mut self, p0: Point) -> Result<()> {
		self.start = Some(p0);
		self.current = Some(p0);
		Ok(())
	}

	fn line_to(&mut self, p1: Point) -> Result<()> {
		let p0 = self.current()?;
		// `_distance`: `math.hypot`, not C's `hypot`.
		self.value += hypot(&[p0.0 - p1.0, p0.1 - p1.1]);
		self.current = Some(p1);
		Ok(())
	}

	fn q_curve_to_one(&mut self, _: Option<Point>, p1: Point, p2: Point) -> Result<()> {
		let p0 = self.current()?;
		let [c0, c1, c2] = [p0, p1, p2].map(Complex::from_point);
		self.value += if self.tolerance >= 0.00075 {
			approximate_quadratic_arc_length_c(c0, c1, c2)
		} else {
			calc_quadratic_arc_length_c(c0, c1, c2)
		};
		self.current = Some(p2);
		Ok(())
	}

	fn curve_to_one(&mut self, _: Option<Point>, p1: Point, p2: Point, p3: Point) -> Result<()> {
		let p0 = self.current()?;
		let [c0, c1, c2, c3] = [p0, p1, p2, p3].map(Complex::from_point);
		self.value += if self.tolerance >= 0.0015 {
			approximate_cubic_arc_length_c(c0, c1, c2, c3)
		} else {
			calc_cubic_arc_length_c(c0, c1, c2, c3, self.tolerance)
		};
		self.current = Some(p3);
		Ok(())
	}

	fn close_path(&mut self) -> Result<()> {
		if self.current != self.start
			&& let Some(start) = self.start
		{
			self.line_to(start)?;
		}
		self.current = None;
		Ok(())
	}

	fn end_path(&mut self) -> Result<()> {
		self.current = None;
		Ok(())
	}
}
