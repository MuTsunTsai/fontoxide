//! Rectangles and point arrays (`fontTools.misc.arrayTools`).
//!
//! Comparisons follow Python's `min` and `max`, which keep the first argument
//! unless the second one is strictly smaller or larger.

/// A point.
pub type Point = (f64, f64);

/// A rectangle: `(xMin, yMin, xMax, yMax)`.
pub type Rect = [f64; 4];

/// Python's `min(a, b)`.
fn py_min(a: f64, b: f64) -> f64 {
	if b < a { b } else { a }
}

/// Python's `max(a, b)`.
fn py_max(a: f64, b: f64) -> f64 {
	if b > a { b } else { a }
}

/// `calcBounds`: the bounding rectangle of the points; `(0, 0, 0, 0)` if there
/// are none.
pub fn calc_bounds(points: &[Point]) -> Rect {
	let Some(&(x0, y0)) = points.first() else {
		return [0.0; 4];
	};
	points[1..].iter().fold([x0, y0, x0, y0], |b, &(x, y)| {
		[
			py_min(b[0], x),
			py_min(b[1], y),
			py_max(b[2], x),
			py_max(b[3], y),
		]
	})
}

/// `updateBounds`: the rectangle extended to include `p`.
pub fn update_bounds(bounds: Rect, (x, y): Point) -> Rect {
	[
		py_min(bounds[0], x),
		py_min(bounds[1], y),
		py_max(bounds[2], x),
		py_max(bounds[3], y),
	]
}

/// `pointInRect`: whether the point is inside the rectangle or on its edge.
pub fn point_in_rect((x, y): Point, rect: Rect) -> bool {
	rect[0] <= x && x <= rect[2] && rect[1] <= y && y <= rect[3]
}

/// `unionRect`: the smallest rectangle containing both.
pub fn union_rect(a: Rect, b: Rect) -> Rect {
	[
		py_min(a[0], b[0]),
		py_min(a[1], b[1]),
		py_max(a[2], b[2]),
		py_max(a[3], b[3]),
	]
}

/// `intRect`: the smallest integer rectangle containing the rectangle.
pub fn int_rect(rect: Rect) -> [i64; 4] {
	[
		rect[0].floor() as i64,
		rect[1].floor() as i64,
		rect[2].ceil() as i64,
		rect[3].ceil() as i64,
	]
}

#[cfg(test)]
mod tests {
	use super::*;

	// Cases from fontTools' doctests.
	#[test]
	fn doctests() {
		assert_eq!(
			calc_bounds(&[(0.0, 40.0), (0.0, 100.0), (50.0, 50.0), (80.0, 10.0)]),
			[0.0, 10.0, 80.0, 100.0]
		);
		assert_eq!(calc_bounds(&[]), [0.0; 4]);
		assert_eq!(
			union_rect([0.0, 10.0, 20.0, 30.0], [0.0, 40.0, 20.0, 50.0]),
			[0.0, 10.0, 20.0, 50.0]
		);
		assert_eq!(int_rect([0.9, 2.9, 3.1, 4.1]), [0, 2, 4, 5]);
		assert!(point_in_rect((50.0, 50.0), [0.0, 0.0, 100.0, 100.0]));
		assert!(!point_in_rect((150.0, 50.0), [0.0, 0.0, 100.0, 100.0]));
		assert_eq!(
			update_bounds([0.0, 0.0, 100.0, 100.0], (200.0, -10.0)),
			[0.0, -10.0, 200.0, 100.0]
		);
	}
}
