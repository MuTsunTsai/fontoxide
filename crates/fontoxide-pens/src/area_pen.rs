//! The area of a glyph (`fontTools.pens.areaPen`).

use crate::base_pen::{BasePen, SegmentPen};
use crate::{Error, GlyphSet, Point, Result};

/// Sums the signed area of the contours (`AreaPen`): negative for
/// counter-clockwise contours.
#[derive(Debug, Default, Clone)]
pub struct Area {
	/// `value`.
	pub value: f64,
	p0: Option<Point>,
	start: Option<Point>,
}

/// `AreaPen`.
pub type AreaPen<'g> = BasePen<'g, Area>;

impl<'g> AreaPen<'g> {
	/// `AreaPen(glyphset)`.
	pub fn new_area(glyph_set: Option<&'g dyn GlyphSet>) -> Self {
		BasePen::with_inner(Area::default(), glyph_set)
	}

	/// `value`.
	pub fn value(&self) -> f64 {
		self.inner.value
	}
}

impl Area {
	fn p0(&self) -> Result<Point> {
		self.p0.ok_or_else(|| {
			Error::Usage("AttributeError: 'AreaPen' object has no attribute '_p0'".into())
		})
	}

	fn line(&mut self, p1: Point) -> Result<()> {
		let (x0, y0) = self.p0()?;
		let (x1, y1) = p1;
		self.value -= (x1 - x0) * (y1 + y0) * 0.5;
		self.p0 = Some(p1);
		Ok(())
	}
}

impl SegmentPen for Area {
	fn move_to(&mut self, p0: Point) -> Result<()> {
		self.p0 = Some(p0);
		self.start = Some(p0);
		Ok(())
	}

	fn line_to(&mut self, p1: Point) -> Result<()> {
		self.line(p1)
	}

	fn q_curve_to_one(&mut self, _: Option<Point>, p1: Point, p2: Point) -> Result<()> {
		// https://github.com/Pomax/bezierinfo/issues/44
		let (x0, y0) = self.p0()?;
		let (x1, y1) = (p1.0 - x0, p1.1 - y0);
		let (x2, y2) = (p2.0 - x0, p2.1 - y0);
		self.value -= (x2 * y1 - x1 * y2) / 3.0;
		self.line(p2)
	}

	fn curve_to_one(&mut self, _: Option<Point>, p1: Point, p2: Point, p3: Point) -> Result<()> {
		// https://github.com/Pomax/bezierinfo/issues/44
		let (x0, y0) = self.p0()?;
		let (x1, y1) = (p1.0 - x0, p1.1 - y0);
		let (x2, y2) = (p2.0 - x0, p2.1 - y0);
		let (x3, y3) = (p3.0 - x0, p3.1 - y0);
		self.value -= (x1 * (-y2 - y3) + x2 * (y1 - 2.0 * y3) + x3 * (y1 + 2.0 * y2)) * 0.15;
		self.line(p3)
	}

	fn close_path(&mut self) -> Result<()> {
		let start = self.start.ok_or_else(|| {
			Error::Usage("AttributeError: 'AreaPen' object has no attribute '_startPoint'".into())
		})?;
		self.line(start)?;
		self.p0 = None;
		self.start = None;
		Ok(())
	}

	fn end_path(&mut self) -> Result<()> {
		if self.p0 != self.start {
			// The area of open contours is not defined.
			return Err(Error::Usage("NotImplementedError".into()));
		}
		self.p0 = None;
		self.start = None;
		Ok(())
	}
}
