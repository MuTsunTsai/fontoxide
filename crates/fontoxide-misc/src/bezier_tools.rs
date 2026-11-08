//! Bézier curve math (`fontTools.misc.bezierTools`).

use crate::array_tools::{Point, Rect, calc_bounds};

const EPSILON: f64 = 1e-10;

/// `solveQuadratic`: the real roots of `a*x*x + b*x + c = 0`, neither sorted nor
/// unique.
pub fn solve_quadratic(a: f64, b: f64, c: f64) -> Vec<f64> {
	if a.abs() < EPSILON {
		if b.abs() < EPSILON {
			Vec::new()
		} else {
			vec![-c / b]
		}
	} else {
		let dd = b * b - 4.0 * a * c;
		if dd >= 0.0 {
			let r = dd.sqrt();
			vec![(-b + r) / 2.0 / a, (-b - r) / 2.0 / a]
		} else {
			Vec::new()
		}
	}
}

/// `calcQuadraticParameters`: the polynomial coefficients `(a, b, c)`.
pub fn calc_quadratic_parameters(pt1: Point, pt2: Point, pt3: Point) -> (Point, Point, Point) {
	let (x2, y2) = pt2;
	let (x3, y3) = pt3;
	let (cx, cy) = pt1;
	let bx = (x2 - cx) * 2.0;
	let by = (y2 - cy) * 2.0;
	let ax = x3 - cx - bx;
	let ay = y3 - cy - by;
	((ax, ay), (bx, by), (cx, cy))
}

/// `calcCubicParameters`: the polynomial coefficients `(a, b, c, d)`.
pub fn calc_cubic_parameters(
	pt1: Point,
	pt2: Point,
	pt3: Point,
	pt4: Point,
) -> (Point, Point, Point, Point) {
	let (x2, y2) = pt2;
	let (x3, y3) = pt3;
	let (x4, y4) = pt4;
	let (dx, dy) = pt1;
	let cx = (x2 - dx) * 3.0;
	let cy = (y2 - dy) * 3.0;
	let bx = (x3 - x2) * 3.0 - cx;
	let by = (y3 - y2) * 3.0 - cy;
	let ax = x4 - dx - cx - bx;
	let ay = y4 - dy - cy - by;
	((ax, ay), (bx, by), (cx, cy), (dx, dy))
}

fn in_unit(t: &f64) -> bool {
	0.0 <= *t && *t < 1.0
}

/// `calcQuadraticBounds`: the bounding rectangle of a quadratic segment.
pub fn calc_quadratic_bounds(pt1: Point, pt2: Point, pt3: Point) -> Rect {
	let ((ax, ay), (bx, by), (cx, cy)) = calc_quadratic_parameters(pt1, pt2, pt3);
	let ax2 = ax * 2.0;
	let ay2 = ay * 2.0;
	let mut roots = Vec::new();
	if ax2 != 0.0 {
		roots.push(-bx / ax2);
	}
	if ay2 != 0.0 {
		roots.push(-by / ay2);
	}
	let mut points: Vec<Point> = roots
		.into_iter()
		.filter(in_unit)
		.map(|t| (ax * t * t + bx * t + cx, ay * t * t + by * t + cy))
		.collect();
	points.push(pt1);
	points.push(pt3);
	calc_bounds(&points)
}

/// `calcCubicBounds`: the bounding rectangle of a cubic segment.
pub fn calc_cubic_bounds(pt1: Point, pt2: Point, pt3: Point, pt4: Point) -> Rect {
	let ((ax, ay), (bx, by), (cx, cy), (dx, dy)) = calc_cubic_parameters(pt1, pt2, pt3, pt4);
	let ax3 = ax * 3.0;
	let ay3 = ay * 3.0;
	let bx2 = bx * 2.0;
	let by2 = by * 2.0;
	let mut roots: Vec<f64> = solve_quadratic(ax3, bx2, cx)
		.into_iter()
		.filter(in_unit)
		.collect();
	roots.extend(solve_quadratic(ay3, by2, cy).into_iter().filter(in_unit));
	let mut points: Vec<Point> = roots
		.into_iter()
		.map(|t| {
			(
				ax * t * t * t + bx * t * t + cx * t + dx,
				ay * t * t * t + by * t * t + cy * t + dy,
			)
		})
		.collect();
	points.push(pt1);
	points.push(pt4);
	calc_bounds(&points)
}

#[cfg(test)]
mod tests {
	use super::*;

	// Cases from fontTools' doctests.
	#[test]
	fn quadratic_bounds() {
		assert_eq!(
			calc_quadratic_bounds((0.0, 0.0), (50.0, 100.0), (100.0, 0.0)),
			[0.0, 0.0, 100.0, 50.0]
		);
		assert_eq!(
			calc_quadratic_bounds((0.0, 0.0), (100.0, 0.0), (100.0, 100.0)),
			[0.0, 0.0, 100.0, 100.0]
		);
	}

	#[test]
	fn cubic_bounds() {
		assert_eq!(
			calc_cubic_bounds((0.0, 0.0), (25.0, 100.0), (75.0, 100.0), (100.0, 0.0)),
			[0.0, 0.0, 100.0, 75.0]
		);
		assert_eq!(
			calc_cubic_bounds((0.0, 0.0), (50.0, 0.0), (100.0, 50.0), (100.0, 100.0)),
			[0.0, 0.0, 100.0, 100.0]
		);
		let b = calc_cubic_bounds((50.0, 0.0), (0.0, 100.0), (100.0, 100.0), (50.0, 0.0));
		assert_eq!(
			format!("{:.6} {:.6} {:.6} {:.6}", b[0], b[1], b[2], b[3]),
			"35.566243 0.000000 64.433757 75.000000"
		);
	}
}
