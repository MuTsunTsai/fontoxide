//! Helpers for manipulating 2D points and vectors in COLR table
//! (`fontTools.colorLib.geometry`).

use std::f64::consts::PI;
use std::fmt;

use fontoxide_misc::py;

use crate::{Error, Result};

/// A point or vector.
pub type Point = (f64, f64);

fn vector_between(origin: Point, target: Point) -> Point {
	(target.0 - origin.0, target.1 - origin.1)
}

/// `otRound`, failing like Python on infinities and NaN.
fn ot_round(value: f64) -> Result<f64> {
	if value.is_nan() {
		return Err(Error::Value("cannot convert float NaN to integer".into()));
	}
	if value.is_infinite() {
		return Err(Error::Overflow(
			"cannot convert float infinity to integer".into(),
		));
	}
	Ok((value + 0.5).floor())
}

fn round_point(pt: Point) -> Result<Point> {
	Ok((ot_round(pt.0)?, ot_round(pt.1)?))
}

fn unit_vector(vec: Point) -> Option<Point> {
	let length = py::hypot(&[vec.0, vec.1]);
	if length == 0.0 {
		return None;
	}
	Some((vec.0 / length, vec.1 / length))
}

/// `_CIRCLE_INSIDE_TOLERANCE`.
const CIRCLE_INSIDE_TOLERANCE: f64 = 1e-4;

/// `_UNIT_VECTOR_THRESHOLD`: `cos(3 / 8 * pi)`.
fn unit_vector_threshold() -> f64 {
	(3.0 / 8.0 * PI).cos()
}

/// `_rounding_offset`: -1, 0 or +1 for each coordinate, approximately in the
/// direction of the vector.
///
/// The unit circle is divided in 8 equal slices oriented towards the cardinal
/// (N, E, S, W) and intermediate (NE, SE, SW, NW) directions. To each slice one
/// of the possible cases is mapped: -1, 0, +1 for either X and Y coordinate.
fn rounding_offset(direction: Point) -> Point {
	let Some(uv) = unit_vector(direction) else {
		return (0.0, 0.0);
	};
	let threshold = unit_vector_threshold();
	let offset = |c: f64| {
		if -threshold <= c && c < threshold {
			// Direction almost orthogonal to the axis: keep the coordinate.
			0.0
		} else {
			// Nudge the coordinate by +/- 1.0 in the direction of the vector.
			1f64.copysign(c)
		}
	};
	(offset(uv.0), offset(uv.1))
}

/// A circle (`Circle`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
	/// The centre.
	pub centre: Point,
	/// The radius.
	pub radius: f64,
}

impl fmt::Display for Circle {
	/// `repr(circle)`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"Circle(centre=({}, {}), radius={})",
			py::float_repr(self.centre.0),
			py::float_repr(self.centre.1),
			py::float_repr(self.radius)
		)
	}
}

impl Circle {
	/// `Circle(centre, radius)`.
	pub fn new(centre: Point, radius: f64) -> Self {
		Circle { centre, radius }
	}

	/// `round`: the circle with its centre and radius rounded (`otRound`).
	pub fn round(&self) -> Result<Circle> {
		Ok(Circle::new(
			round_point(self.centre)?,
			ot_round(self.radius)?,
		))
	}

	/// `inside`: whether the circle is inside another one, with a relative
	/// tolerance.
	pub fn inside(&self, outer_circle: &Circle) -> bool {
		let v = vector_between(self.centre, outer_circle.centre);
		let dist = self.radius + py::hypot(&[v.0, v.1]);
		py::is_close_tol(outer_circle.radius, dist, CIRCLE_INSIDE_TOLERANCE, 0.0)
			|| outer_circle.radius > dist
	}

	/// `concentric`.
	pub fn concentric(&self, other: &Circle) -> bool {
		self.centre == other.centre
	}

	/// `move`.
	pub fn move_by(&mut self, dx: f64, dy: f64) {
		self.centre = (self.centre.0 + dx, self.centre.1 + dy);
	}
}

/// Rounds the start circle so that it stays inside or outside the end circle
/// after rounding (`round_start_circle_stable_containment`).
///
/// The rounding of circle coordinates to integers may cause an abrupt change
/// if the start circle c0 is so close to the end circle c1's perimeter that it
/// ends up falling outside (or inside) as a result of the rounding. To keep the
/// gradient unchanged, it is nudged in the right direction. See
/// <https://github.com/googlefonts/colr-gradients-spec/issues/204> and
/// <https://github.com/googlefonts/picosvg/issues/158>.
pub fn round_start_circle_stable_containment(
	c0: Point,
	r0: f64,
	c1: Point,
	r1: f64,
) -> Result<Circle> {
	let (start, end) = (Circle::new(c0, r0), Circle::new(c1, r1));

	let inside_before_round = start.inside(&end);

	let mut round_start = start.round()?;
	let round_end = end.round()?;
	let inside_after_round = round_start.inside(&round_end);

	let (direction, radius_delta) = if inside_before_round == inside_after_round {
		return Ok(round_start);
	} else if inside_after_round {
		// Start was outside before rounding: push start away from end.
		(vector_between(round_end.centre, round_start.centre), 1.0)
	} else {
		// Start was inside before rounding: push start towards end.
		(vector_between(round_start.centre, round_end.centre), -1.0)
	};
	let (dx, dy) = rounding_offset(direction);

	// At most 2 iterations ought to be enough to converge: normal rounding can
	// move each coordinate by at most 0.5, and two nudges by 1.0 on both axes
	// cover twice the largest distance this can make.
	const MAX_ATTEMPTS: usize = 2;
	for _ in 0..MAX_ATTEMPTS {
		if round_start.concentric(&round_end) {
			// Can't move c0 towards c1 (they are the same), so change the radius.
			round_start.radius += radius_delta;
			if round_start.radius < 0.0 {
				return Err(Error::Assertion("round_start.radius >= 0".into()));
			}
		} else {
			round_start.move_by(dx, dy);
		}
		if inside_before_round == round_start.inside(&round_end) {
			return Ok(round_start);
		}
	}
	// Likely a bug.
	Err(Error::Assertion(format!(
		"Rounding circle {start} {} {end} failed after {MAX_ATTEMPTS} attempts!",
		if inside_before_round {
			"inside"
		} else {
			"outside"
		}
	)))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn threshold() {
		assert_eq!(unit_vector_threshold(), 0.38268343236508984);
	}
}
