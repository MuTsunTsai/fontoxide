//! Whether a point is inside a glyph (`fontTools.pens.pointInsidePen`).

use fontoxide_misc::bezier_tools::solve_cubic;
use fontoxide_misc::py::sort_floats;

use crate::base_pen::{BasePen, SegmentPen};
use crate::{Error, GlyphSet, Pen, Point, Result};

/// Counts how often a ray from the test point to the right crosses the
/// outline (`PointInsidePen`): up minus down crossings for the non-zero
/// winding rule, all crossings for the even-odd rule.
#[derive(Debug, Clone)]
pub struct PointInside {
	/// `testPoint`.
	pub test_point: Point,
	/// `evenOdd`.
	pub even_odd: bool,
	first_point: Option<Point>,
	current: Option<Point>,
	/// `intersectionCount`.
	pub intersection_count: i64,
}

/// `PointInsidePen`.
pub type PointInsidePen<'g> = BasePen<'g, PointInside>;

impl<'g> PointInsidePen<'g> {
	/// `PointInsidePen(glyphSet, testPoint, evenOdd)`.
	pub fn new_point_inside(
		glyph_set: Option<&'g dyn GlyphSet>,
		test_point: Point,
		even_odd: bool,
	) -> Self {
		BasePen::with_inner(
			PointInside {
				test_point,
				even_odd,
				first_point: None,
				current: None,
				intersection_count: 0,
			},
			glyph_set,
		)
	}

	/// `setTestPoint`: resets the pen for another point.
	pub fn set_test_point(&mut self, test_point: Point, even_odd: bool) {
		self.inner.test_point = test_point;
		self.inner.even_odd = even_odd;
		self.inner.first_point = None;
		self.inner.intersection_count = 0;
	}

	/// `getWinding`: closes the open contour, if any, and returns the count.
	pub fn get_winding(&mut self) -> Result<i64> {
		if self.inner.first_point.is_some() {
			// The algorithm only works for closed contours.
			self.close_path()?;
		}
		Ok(self.inner.intersection_count)
	}

	/// `getResult`: whether the test point is inside.
	pub fn get_result(&mut self) -> Result<bool> {
		let winding = self.get_winding()?;
		Ok(if self.inner.even_odd {
			winding.rem_euclid(2) != 0
		} else {
			self.inner.intersection_count != 0
		})
	}
}

impl PointInside {
	fn add_intersection(&mut self, going_up: bool) {
		if self.even_odd || going_up {
			self.intersection_count += 1;
		} else {
			self.intersection_count -= 1;
		}
	}

	fn current(&self) -> Result<Point> {
		self.current.ok_or_else(|| {
			Error::Usage("TypeError: cannot unpack non-iterable NoneType object".into())
		})
	}

	/// `_closePath`, as called through `closePath`.
	fn close(&mut self) -> Result<()> {
		if self.current != self.first_point
			&& let Some(first) = self.first_point
		{
			self.line(first)?;
		}
		self.first_point = None;
		self.current = None;
		Ok(())
	}

	fn line(&mut self, point: Point) -> Result<()> {
		let (x, y) = self.test_point;
		let (x1, y1) = self.current()?;
		let (x2, y2) = point;
		self.current = Some(point);
		if x1 < x && x2 < x {
			return Ok(());
		}
		if y1 < y && y2 < y {
			return Ok(());
		}
		if y1 >= y && y2 >= y {
			return Ok(());
		}
		let dx = x2 - x1;
		let dy = y2 - y1;
		let t = (y - y1) / dy;
		let ix = dx * t + x1;
		if ix < x {
			return Ok(());
		}
		self.add_intersection(y2 > y1);
		Ok(())
	}
}

impl SegmentPen for PointInside {
	fn move_to(&mut self, point: Point) -> Result<()> {
		if self.first_point.is_some() {
			// The algorithm only works for closed contours.
			self.close()?;
		}
		self.first_point = Some(point);
		self.current = Some(point);
		Ok(())
	}

	fn line_to(&mut self, point: Point) -> Result<()> {
		self.line(point)
	}

	fn curve_to_one(
		&mut self,
		_: Option<Point>,
		bcp1: Point,
		bcp2: Point,
		point: Point,
	) -> Result<()> {
		let (x, y) = self.test_point;
		let (x1, y1) = self.current()?;
		let (x2, y2) = bcp1;
		let (x3, y3) = bcp2;
		let (x4, y4) = point;
		self.current = Some(point);

		if x1 < x && x2 < x && x3 < x && x4 < x {
			return Ok(());
		}
		if y1 < y && y2 < y && y3 < y && y4 < y {
			return Ok(());
		}
		if y1 >= y && y2 >= y && y3 >= y && y4 >= y {
			return Ok(());
		}

		let dy = y1;
		let cy = (y2 - dy) * 3.0;
		let by = (y3 - y2) * 3.0 - cy;
		let ay = y4 - dy - cy - by;
		let mut solutions = solve_cubic(ay, by, cy, dy - y);
		sort_floats(&mut solutions);
		solutions.retain(|&t| (-0.0..=1.0).contains(&t));
		if solutions.is_empty() {
			return Ok(());
		}

		let dx = x1;
		let cx = (x2 - dx) * 3.0;
		let bx = (x3 - x2) * 3.0 - cx;
		let ax = x4 - dx - cx - bx;

		let mut last_t = None;
		for t in solutions {
			if Some(t) == last_t {
				continue;
			}
			last_t = Some(t);
			let t2 = t * t;
			let t3 = t2 * t;

			let mut direction = 3.0 * ay * t2 + 2.0 * by * t + cy;
			let mut incoming_going_up = direction > 0.0;
			let mut outgoing_going_up = incoming_going_up;
			if direction == 0.0 {
				direction = 6.0 * ay * t + 2.0 * by;
				outgoing_going_up = direction > 0.0;
				incoming_going_up = !outgoing_going_up;
				if direction == 0.0 {
					direction = ay;
					incoming_going_up = direction > 0.0;
					outgoing_going_up = incoming_going_up;
				}
			}

			let xt = ax * t3 + bx * t2 + cx * t + dx;
			if xt < x {
				continue;
			}

			if t == 0.0 {
				if !outgoing_going_up {
					self.add_intersection(outgoing_going_up);
				}
			} else if t == 1.0 {
				if incoming_going_up {
					self.add_intersection(incoming_going_up);
				}
			} else if incoming_going_up == outgoing_going_up {
				self.add_intersection(outgoing_going_up);
			}
			// Otherwise the curve merely touches the ray.
		}
		Ok(())
	}

	fn close_path(&mut self) -> Result<()> {
		self.close()
	}

	fn end_path(&mut self) -> Result<()> {
		// Insideness is not defined for open contours.
		Err(Error::Usage("NotImplementedError".into()))
	}
}
