//! Pens computing the bounds of outlines (`fontTools.pens.boundsPen`).

use fontoxide_misc::array_tools::{point_in_rect, union_rect, update_bounds};
use fontoxide_misc::bezier_tools::{calc_cubic_bounds, calc_quadratic_bounds};

use crate::base_pen::{BasePen, SegmentPen};
use crate::{Error, GlyphSet, Point, Rect, Result};

/// The state of [`ControlBoundsPen`] and [`BoundsPen`].
#[derive(Debug, Clone, Default)]
pub struct Bounds {
	/// The bounds so far (`bounds`), `None` until something is drawn.
	pub bounds: Option<Rect>,
	/// Whether contours of a single point are left out (`ignoreSinglePoints`).
	pub ignore_single_points: bool,
	/// Whether the bounds are those of the curves rather than of their control
	/// points.
	exact: bool,
	start: Option<Point>,
}

fn none_error() -> Error {
	Error::Usage("TypeError: cannot unpack non-iterable NoneType object".into())
}

impl Bounds {
	/// `_addMoveTo`.
	fn add_move_to(&mut self) {
		let Some(start) = self.start.take() else {
			return;
		};
		self.bounds = Some(match self.bounds {
			Some(b) => update_bounds(b, start),
			None => [start.0, start.1, start.0, start.1],
		});
	}

	fn update(&mut self, pt: Point) -> Result<Rect> {
		let b = update_bounds(self.bounds.ok_or_else(none_error)?, pt);
		self.bounds = Some(b);
		Ok(b)
	}
}

impl SegmentPen for Bounds {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.start = Some(pt);
		if !self.ignore_single_points {
			self.add_move_to();
		}
		Ok(())
	}

	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.add_move_to();
		self.update(pt)?;
		Ok(())
	}

	fn curve_to_one(
		&mut self,
		current: Option<Point>,
		bcp1: Point,
		bcp2: Point,
		pt: Point,
	) -> Result<()> {
		self.add_move_to();
		if !self.exact {
			self.update(bcp1)?;
			self.update(bcp2)?;
			self.update(pt)?;
			return Ok(());
		}
		let mut bounds = self.update(pt)?;
		if !point_in_rect(bcp1, bounds) || !point_in_rect(bcp2, bounds) {
			let current = current.ok_or_else(none_error)?;
			bounds = union_rect(bounds, calc_cubic_bounds(current, bcp1, bcp2, pt));
		}
		self.bounds = Some(bounds);
		Ok(())
	}

	fn q_curve_to_one(&mut self, current: Option<Point>, bcp: Point, pt: Point) -> Result<()> {
		self.add_move_to();
		if !self.exact {
			self.update(bcp)?;
			self.update(pt)?;
			return Ok(());
		}
		let mut bounds = self.update(pt)?;
		if !point_in_rect(bcp, bounds) {
			let current = current.ok_or_else(none_error)?;
			bounds = union_rect(bounds, calc_quadratic_bounds(current, bcp, pt));
		}
		self.bounds = Some(bounds);
		Ok(())
	}
}

/// A pen computing the bounds of all control points (`ControlBoundsPen`), which
/// can be larger than those of the curves.
pub type ControlBoundsPen<'g> = BasePen<'g, Bounds>;

/// A pen computing the exact bounds of the outline (`BoundsPen`).
///
/// Both kinds are a [`BasePen`] over [`Bounds`]; [`BasePen::new_bounds`] and
/// [`BasePen::new_control_bounds`] create them.
pub type BoundsPen<'g> = BasePen<'g, Bounds>;

impl<'g> BasePen<'g, Bounds> {
	/// `BoundsPen(glyphSet)`.
	pub fn new_bounds(glyph_set: Option<&'g dyn GlyphSet>) -> Self {
		BasePen::with_inner(
			Bounds {
				exact: true,
				..Default::default()
			},
			glyph_set,
		)
	}

	/// `ControlBoundsPen(glyphSet, ignoreSinglePoints)`.
	pub fn new_control_bounds(
		glyph_set: Option<&'g dyn GlyphSet>,
		ignore_single_points: bool,
	) -> Self {
		BasePen::with_inner(
			Bounds {
				ignore_single_points,
				..Default::default()
			},
			glyph_set,
		)
	}

	/// The bounds so far (`bounds`), `None` until something is drawn.
	pub fn bounds(&self) -> Option<Rect> {
		self.inner.bounds
	}

	/// Forgets the bounds (`init`).
	pub fn reset(&mut self) {
		self.inner.bounds = None;
		self.inner.start = None;
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::Pen;

	// Cases from fontTools' Tests/pens/boundsPen_test.py.
	#[test]
	fn bounds() -> Result<()> {
		let mut pen = BoundsPen::new_bounds(None);
		pen.move_to((0.0, 0.0))?;
		pen.curve_to(&[(0.0, 100.0), (100.0, 100.0), (100.0, 0.0)])?;
		pen.close_path()?;
		assert_eq!(pen.bounds(), Some([0.0, 0.0, 100.0, 75.0]));

		let mut pen = BoundsPen::new_bounds(None);
		pen.move_to((0.0, 0.0))?;
		pen.q_curve_to(&[(50.0, 100.0)], Some((100.0, 0.0)))?;
		pen.close_path()?;
		assert_eq!(pen.bounds(), Some([0.0, 0.0, 100.0, 50.0]));
		Ok(())
	}

	#[test]
	fn control_bounds() -> Result<()> {
		let mut pen = ControlBoundsPen::new_control_bounds(None, false);
		pen.move_to((0.0, 0.0))?;
		pen.curve_to(&[(0.0, 100.0), (100.0, 100.0), (100.0, 0.0)])?;
		pen.close_path()?;
		assert_eq!(pen.bounds(), Some([0.0, 0.0, 100.0, 100.0]));

		let mut pen = ControlBoundsPen::new_control_bounds(None, true);
		pen.move_to((-100.0, -100.0))?;
		pen.end_path()?;
		pen.move_to((0.0, 0.0))?;
		pen.line_to((100.0, 100.0))?;
		pen.end_path()?;
		assert_eq!(pen.bounds(), Some([0.0, 0.0, 100.0, 100.0]));
		Ok(())
	}
}
