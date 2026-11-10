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

/// `calcIntBounds`: the bounding rectangle, rounded with `round` (`otRound` in
/// fontTools).
pub fn calc_int_bounds(points: &[Point], round: impl Fn(f64) -> i64) -> [i64; 4] {
	calc_bounds(points).map(round)
}

/// `pointsInRect`: for each point, whether it is inside the rectangle.
pub fn points_in_rect(points: &[Point], rect: Rect) -> Vec<bool> {
	points.iter().map(|&p| point_in_rect(p, rect)).collect()
}

/// `vectorLength`.
pub fn vector_length((x, y): Point) -> f64 {
	(x.powi(2) + y.powi(2)).sqrt()
}

/// `asInt16`: each value rounded half up.
pub fn as_int16(values: &[f64]) -> Vec<i64> {
	values.iter().map(|&v| (v + 0.5).floor() as i64).collect()
}

/// `normRect`: the rectangle with `xMin <= xMax` and `yMin <= yMax`.
pub fn norm_rect(rect: Rect) -> Rect {
	let [x_min, y_min, x_max, y_max] = rect;
	[
		py_min(x_min, x_max),
		py_min(y_min, y_max),
		py_max(x_min, x_max),
		py_max(y_min, y_max),
	]
}

/// `scaleRect`.
pub fn scale_rect(rect: Rect, x: f64, y: f64) -> Rect {
	[rect[0] * x, rect[1] * y, rect[2] * x, rect[3] * y]
}

/// `offsetRect`.
pub fn offset_rect(rect: Rect, dx: f64, dy: f64) -> Rect {
	[rect[0] + dx, rect[1] + dy, rect[2] + dx, rect[3] + dy]
}

/// `insetRect`: the rectangle inset by `dx` and `dy` on all sides.
pub fn inset_rect(rect: Rect, dx: f64, dy: f64) -> Rect {
	[rect[0] + dx, rect[1] + dy, rect[2] - dx, rect[3] - dy]
}

/// `sectRect`: the intersection of the rectangles, or `None` if they do not
/// intersect (`(False, (0, 0, 0, 0))`).
pub fn sect_rect(a: Rect, b: Rect) -> Option<Rect> {
	let r = [
		py_max(a[0], b[0]),
		py_max(a[1], b[1]),
		py_min(a[2], b[2]),
		py_min(a[3], b[3]),
	];
	if r[0] >= r[2] || r[1] >= r[3] {
		None
	} else {
		Some(r)
	}
}

/// `rectCenter`.
pub fn rect_center(rect: Rect) -> Point {
	((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0)
}

/// `rectArea`.
pub fn rect_area(rect: Rect) -> f64 {
	(rect[3] - rect[1]) * (rect[2] - rect[0])
}

/// `quantizeRect`: the rectangle, normalized, with its edges moved outwards to
/// multiples of `factor`. `None` for a factor below 1 (`ValueError`).
pub fn quantize_rect(rect: Rect, factor: f64) -> Option<[i64; 4]> {
	if factor < 1.0 {
		return None;
	}
	let [x_min, y_min, x_max, y_max] = norm_rect(rect);
	Some([
		((x_min / factor).floor() * factor) as i64,
		((y_min / factor).floor() * factor) as i64,
		((x_max / factor).ceil() * factor) as i64,
		((y_max / factor).ceil() * factor) as i64,
	])
}

/// `pairwise`: each item with the next one, the last with the first; in
/// reverse order if `reverse`.
pub fn pairwise<T: Copy>(items: &[T], reverse: bool) -> Vec<(T, T)> {
	let n = items.len();
	let at = |i: usize| if reverse { items[n - 1 - i] } else { items[i] };
	(0..n).map(|i| (at(i), at((i + 1) % n))).collect()
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

	// `arrayTools._test`.
	#[test]
	fn array_tools_test() {
		assert_eq!(
			update_bounds([0.0; 4], (100.0, 100.0)),
			[0.0, 0.0, 100.0, 100.0]
		);
		assert!(point_in_rect((0.0, 0.0), [0.0, 0.0, 100.0, 100.0]));
		assert!(point_in_rect((100.0, 100.0), [0.0, 0.0, 100.0, 100.0]));
		assert!(!point_in_rect((101.0, 100.0), [0.0, 0.0, 100.0, 100.0]));
		assert_eq!(
			points_in_rect(
				&[(50.0, 50.0), (0.0, 0.0), (100.0, 100.0), (101.0, 100.0)],
				[0.0, 0.0, 100.0, 100.0]
			),
			[true, true, true, false]
		);
		assert_eq!(vector_length((3.0, 4.0)), 5.0);
		assert_eq!(vector_length((1.0, 1.0)), 2f64.sqrt());
		assert_eq!(as_int16(&[0.0, 0.1, 0.5, 0.9]), [0, 0, 1, 1]);
		assert_eq!(
			norm_rect([0.0, 10.0, 100.0, 200.0]),
			[0.0, 10.0, 100.0, 200.0]
		);
		assert_eq!(
			norm_rect([100.0, 200.0, 0.0, 10.0]),
			[0.0, 10.0, 100.0, 200.0]
		);
		assert_eq!(
			scale_rect([10.0, 20.0, 50.0, 150.0], 1.5, 2.0),
			[15.0, 40.0, 75.0, 300.0]
		);
		assert_eq!(
			offset_rect([10.0, 20.0, 30.0, 40.0], 5.0, 6.0),
			[15.0, 26.0, 35.0, 46.0]
		);
		assert_eq!(
			inset_rect([10.0, 20.0, 50.0, 60.0], 5.0, 10.0),
			[15.0, 30.0, 45.0, 50.0]
		);
		assert_eq!(
			inset_rect([10.0, 20.0, 50.0, 60.0], -5.0, -10.0),
			[5.0, 10.0, 55.0, 70.0]
		);
		assert_eq!(
			sect_rect([0.0, 10.0, 20.0, 30.0], [0.0, 40.0, 20.0, 50.0]),
			None
		);
		assert_eq!(
			sect_rect([0.0, 10.0, 20.0, 30.0], [5.0, 20.0, 35.0, 50.0]),
			Some([5.0, 20.0, 20.0, 30.0])
		);
		assert_eq!(rect_center([0.0, 0.0, 100.0, 200.0]), (50.0, 100.0));
		assert_eq!(rect_center([0.0, 0.0, 100.0, 199.0]), (50.0, 99.5));
	}

	#[test]
	fn quantize_rect_doctests() {
		let bounds = [72.3, -218.4, 1201.3, 919.1];
		assert_eq!(quantize_rect(bounds, 1.0), Some([72, -219, 1202, 920]));
		assert_eq!(quantize_rect(bounds, 10.0), Some([70, -220, 1210, 920]));
		assert_eq!(quantize_rect(bounds, 100.0), Some([0, -300, 1300, 1000]));
		assert_eq!(quantize_rect(bounds, 0.5), None);
	}

	#[test]
	fn pairwise_doctests() {
		assert_eq!(pairwise::<i32>(&[], false), []);
		assert_eq!(pairwise(&[0], false), [(0, 0)]);
		assert_eq!(pairwise(&[0], true), [(0, 0)]);
		assert_eq!(pairwise(&[0, 1], false), [(0, 1), (1, 0)]);
		assert_eq!(pairwise(&[0, 1], true), [(1, 0), (0, 1)]);
		assert_eq!(pairwise(&[0, 1, 2], false), [(0, 1), (1, 2), (2, 0)]);
		assert_eq!(pairwise(&[0, 1, 2], true), [(2, 1), (1, 0), (0, 2)]);
		assert_eq!(
			pairwise(&['a', 'b', 'c', 'd'], true),
			[('d', 'c'), ('c', 'b'), ('b', 'a'), ('a', 'd')]
		);
	}
}
