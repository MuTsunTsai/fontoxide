//! Bézier curve math (`fontTools.misc.bezierTools`).
//!
//! The functions taking [`Complex`] points are the `…C` variants of fontTools,
//! which treat points as complex numbers.

use crate::array_tools::{Point, Rect, calc_bounds, rect_area, sect_rect};
use crate::complex::{Complex, J};
use crate::py::{is_close, round_digits};
use crate::transform::IDENTITY;

/// `epsilon`.
const EPSILON: f64 = 1e-10;
/// `epsilonDigits`.
const EPSILON_DIGITS: i32 = 6;
/// The `EPSILON` of the arc length recursion.
const ARC_EPSILON: f64 = 1e-9;

/// `ValueError` of a function given a segment of the wrong degree.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ValueError(pub &'static str);

/// `Intersection`: where two segments intersect, and at which times.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Intersection {
	/// `pt`.
	pub pt: Point,
	/// `t1`: the time on the first segment.
	pub t1: f64,
	/// `t2`: the time on the second segment.
	pub t2: f64,
}

/// `calcCubicArcLength`: the arc length of a cubic segment, measured by
/// dividing it until the parts are shorter than `tolerance` (0.005 in
/// fontTools).
pub fn calc_cubic_arc_length(
	pt1: Point,
	pt2: Point,
	pt3: Point,
	pt4: Point,
	tolerance: f64,
) -> f64 {
	calc_cubic_arc_length_c(
		Complex::from_point(pt1),
		Complex::from_point(pt2),
		Complex::from_point(pt3),
		Complex::from_point(pt4),
		tolerance,
	)
}

fn split_cubic_into_two(
	p0: Complex,
	p1: Complex,
	p2: Complex,
	p3: Complex,
) -> ([Complex; 4], [Complex; 4]) {
	let mid = (p0 + 3.0 * (p1 + p2) + p3) * 0.125;
	let deriv3 = (p3 + p2 - p1 - p0) * 0.125;
	(
		[p0, (p0 + p1) * 0.5, mid - deriv3, mid],
		[mid, mid + deriv3, (p2 + p3) * 0.5, p3],
	)
}

fn calc_cubic_arc_length_c_recurse(
	mult: f64,
	p0: Complex,
	p1: Complex,
	p2: Complex,
	p3: Complex,
) -> f64 {
	let arch = (p0 - p3).abs();
	let box_ = (p0 - p1).abs() + (p1 - p2).abs() + (p2 - p3).abs();
	if arch * mult + ARC_EPSILON >= box_ {
		(arch + box_) * 0.5
	} else {
		let (one, two) = split_cubic_into_two(p0, p1, p2, p3);
		calc_cubic_arc_length_c_recurse(mult, one[0], one[1], one[2], one[3])
			+ calc_cubic_arc_length_c_recurse(mult, two[0], two[1], two[2], two[3])
	}
}

/// `calcCubicArcLengthC`.
pub fn calc_cubic_arc_length_c(
	pt1: Complex,
	pt2: Complex,
	pt3: Complex,
	pt4: Complex,
	tolerance: f64,
) -> f64 {
	// The 1.5 is an empirical hack; no math.
	let mult = 1.0 + 1.5 * tolerance;
	calc_cubic_arc_length_c_recurse(mult, pt1, pt2, pt3, pt4)
}

/// `_dot`.
fn dot(v1: Complex, v2: Complex) -> f64 {
	(v1 * v2.conj()).re
}

/// `_intSecAtan`.
fn int_sec_atan(x: f64) -> f64 {
	// In : sympy.integrate(sp.sec(sp.atan(x)))
	// Out: x*sqrt(x**2 + 1)/2 + asinh(x)/2
	x * (x.powi(2) + 1.0).sqrt() / 2.0 + x.asinh() / 2.0
}

/// `calcQuadraticArcLength`: the arc length of a quadratic segment.
pub fn calc_quadratic_arc_length(pt1: Point, pt2: Point, pt3: Point) -> f64 {
	calc_quadratic_arc_length_c(
		Complex::from_point(pt1),
		Complex::from_point(pt2),
		Complex::from_point(pt3),
	)
}

/// `calcQuadraticArcLengthC`.
pub fn calc_quadratic_arc_length_c(pt1: Complex, pt2: Complex, pt3: Complex) -> f64 {
	// Analytical solution to the length of a quadratic bezier.
	// Documentation: https://github.com/fonttools/fonttools/issues/3055
	let d0 = pt2 - pt1;
	let d1 = pt3 - pt2;
	let d = d1 - d0;
	let n = d * J;
	let scale = n.abs();
	if scale == 0.0 {
		return (pt3 - pt1).abs();
	}
	let orig_dist = dot(n, d0);
	if orig_dist.abs() < EPSILON {
		if dot(d0, d1) >= 0.0 {
			return (pt3 - pt1).abs();
		}
		let (a, b) = (d0.abs(), d1.abs());
		return (a * a + b * b) / (a + b);
	}
	let x0 = dot(d, d0) / orig_dist;
	let x1 = dot(d, d1) / orig_dist;
	(2.0 * (int_sec_atan(x1) - int_sec_atan(x0)) * orig_dist / (scale * (x1 - x0))).abs()
}

/// `approximateQuadraticArcLength`: the arc length of a quadratic segment, by
/// Gauss-Legendre quadrature.
pub fn approximate_quadratic_arc_length(pt1: Point, pt2: Point, pt3: Point) -> f64 {
	approximate_quadratic_arc_length_c(
		Complex::from_point(pt1),
		Complex::from_point(pt2),
		Complex::from_point(pt3),
	)
}

/// `approximateQuadraticArcLengthC`.
pub fn approximate_quadratic_arc_length_c(pt1: Complex, pt2: Complex, pt3: Complex) -> f64 {
	// abs(BezierCurveC[2].diff(t).subs({t:T})) for T in sorted(.5, .5±sqrt(3/5)/2),
	// weighted 5/18, 8/18, 5/18 respectively.
	let v0 = (-0.492943519233745 * pt1 + 0.430331482911935 * pt2 + 0.0626120363218102 * pt3).abs();
	let v1 = (pt3 - pt1).abs() * 0.4444444444444444;
	let v2 = (-0.0626120363218102 * pt1 - 0.430331482911935 * pt2 + 0.492943519233745 * pt3).abs();
	v0 + v1 + v2
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

/// `approximateCubicArcLength`: the arc length of a cubic segment, by
/// Gauss-Lobatto quadrature.
pub fn approximate_cubic_arc_length(pt1: Point, pt2: Point, pt3: Point, pt4: Point) -> f64 {
	approximate_cubic_arc_length_c(
		Complex::from_point(pt1),
		Complex::from_point(pt2),
		Complex::from_point(pt3),
		Complex::from_point(pt4),
	)
}

/// `approximateCubicArcLengthC`.
pub fn approximate_cubic_arc_length_c(
	pt1: Complex,
	pt2: Complex,
	pt3: Complex,
	pt4: Complex,
) -> f64 {
	// abs(BezierCurveC[3].diff(t).subs({t:T})) for T in sorted(0, .5±(3/7)**.5/2, .5, 1),
	// weighted 1/20, 49/180, 32/90, 49/180, 1/20 respectively.
	let v0 = (pt2 - pt1).abs() * 0.15;
	let v1 = (-0.558983582205757 * pt1
		+ 0.325650248872424 * pt2
		+ 0.208983582205757 * pt3
		+ 0.024349751127576 * pt4)
		.abs();
	let v2 = (pt4 - pt1 + pt3 - pt2).abs() * 0.26666666666666666;
	let v3 = (-0.024349751127576 * pt1 - 0.208983582205757 * pt2 - 0.325650248872424 * pt3
		+ 0.558983582205757 * pt4)
		.abs();
	let v4 = (pt4 - pt3).abs() * 0.15;
	v0 + v1 + v2 + v3 + v4
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

/// `(x, y)[isHorizontal]`.
fn coord((x, y): Point, is_horizontal: bool) -> f64 {
	if is_horizontal { y } else { x }
}

/// `splitLine`: splits a line where it crosses `where_`, a Y coordinate if
/// `is_horizontal`, else an X coordinate.
pub fn split_line(pt1: Point, pt2: Point, where_: f64, is_horizontal: bool) -> Vec<[Point; 2]> {
	let (pt1x, pt1y) = pt1;
	let (pt2x, pt2y) = pt2;
	let ax = pt2x - pt1x;
	let ay = pt2y - pt1y;
	let bx = pt1x;
	let by = pt1y;
	let a = coord((ax, ay), is_horizontal);
	if a == 0.0 {
		return vec![[pt1, pt2]];
	}
	let t = (where_ - coord((bx, by), is_horizontal)) / a;
	if in_unit(&t) {
		let mid = (ax * t + bx, ay * t + by);
		vec![[pt1, mid], [mid, pt2]]
	} else {
		vec![[pt1, pt2]]
	}
}

/// `sorted(t for t in solutions if 0 <= t < 1)`.
fn sorted_unit(solutions: Vec<f64>) -> Vec<f64> {
	let mut ts: Vec<f64> = solutions.into_iter().filter(in_unit).collect();
	crate::py::sort_floats(&mut ts);
	ts
}

/// `splitQuadratic`: splits a quadratic segment where it crosses `where_`.
pub fn split_quadratic(
	pt1: Point,
	pt2: Point,
	pt3: Point,
	where_: f64,
	is_horizontal: bool,
) -> Vec<[Point; 3]> {
	let (a, b, c) = calc_quadratic_parameters(pt1, pt2, pt3);
	let solutions = sorted_unit(solve_quadratic(
		coord(a, is_horizontal),
		coord(b, is_horizontal),
		coord(c, is_horizontal) - where_,
	));
	if solutions.is_empty() {
		return vec![[pt1, pt2, pt3]];
	}
	let mut split = split_quadratic_at_t_params(a, b, c, &solutions);
	// As in splitQuadraticAtT, the last segment must end exactly at pt3.
	if let Some(last) = split.last_mut() {
		last[2] = pt3;
	}
	split
}

/// `splitCubic`: splits a cubic segment where it crosses `where_`.
pub fn split_cubic(
	pt1: Point,
	pt2: Point,
	pt3: Point,
	pt4: Point,
	where_: f64,
	is_horizontal: bool,
) -> Vec<[Point; 4]> {
	let (a, b, c, d) = calc_cubic_parameters(pt1, pt2, pt3, pt4);
	let solutions = sorted_unit(solve_cubic(
		coord(a, is_horizontal),
		coord(b, is_horizontal),
		coord(c, is_horizontal),
		coord(d, is_horizontal) - where_,
	));
	if solutions.is_empty() {
		return vec![[pt1, pt2, pt3, pt4]];
	}
	let mut split = split_cubic_at_t_params(a, b, c, d, &solutions);
	// As in splitCubicAtT, the last segment must end exactly at pt4.
	if let Some(last) = split.last_mut() {
		last[3] = pt4;
	}
	split
}

/// `splitQuadraticAtT`: splits a quadratic segment at the times `ts`.
pub fn split_quadratic_at_t(pt1: Point, pt2: Point, pt3: Point, ts: &[f64]) -> Vec<[Point; 3]> {
	let (a, b, c) = calc_quadratic_parameters(pt1, pt2, pt3);
	let mut split = split_quadratic_at_t_params(a, b, c, ts);
	// The split can introduce floating point errors; the last segment ends at
	// pt3. The first one starts at pt1 already, as t1 is 0.0 for it.
	if let Some(last) = split.last_mut() {
		last[2] = pt3;
	}
	split
}

/// `splitCubicAtT`: splits a cubic segment at the times `ts`.
pub fn split_cubic_at_t(
	pt1: Point,
	pt2: Point,
	pt3: Point,
	pt4: Point,
	ts: &[f64],
) -> Vec<[Point; 4]> {
	let (a, b, c, d) = calc_cubic_parameters(pt1, pt2, pt3, pt4);
	let mut split = split_cubic_at_t_params(a, b, c, d, ts);
	// The split can introduce floating point errors; the last segment ends at
	// pt4. The first one starts at pt1 already, as t1 is 0.0 for it.
	if let Some(last) = split.last_mut() {
		last[3] = pt4;
	}
	split
}

/// `splitCubicAtTC`: splits a cubic segment of complex points at the times `ts`.
pub fn split_cubic_at_t_c(
	pt1: Complex,
	pt2: Complex,
	pt3: Complex,
	pt4: Complex,
	ts: &[f64],
) -> Vec<[Complex; 4]> {
	let (a, b, c, d) = calc_cubic_parameters_c(pt1, pt2, pt3, pt4);
	let ts = with_ends(ts);
	let mut segments = Vec::with_capacity(ts.len() - 1);
	for w in ts.windows(2) {
		let (t1, t2) = (w[0], w[1]);
		let delta = t2 - t1;
		let delta_2 = delta * delta;
		let delta_3 = delta * delta_2;
		let t1_2 = t1 * t1;
		let t1_3 = t1 * t1_2;
		// Calculate the new a, b, c and d.
		let a1 = a * delta_3;
		let b1 = (3.0 * a * t1 + b) * delta_2;
		let c1 = (2.0 * b * t1 + c + 3.0 * a * t1_2) * delta;
		let d1 = a * t1_3 + b * t1_2 + c * t1 + d;
		let [p1, p2, p3, p4] = calc_cubic_points_c(a1, b1, c1, d1);
		// The reparametrisation can introduce floating point errors, so a
		// segment that ends at t == 1 is made to end exactly at pt4.
		segments.push([p1, p2, p3, if t2 == 1.0 { pt4 } else { p4 }]);
	}
	segments
}

/// `splitCubicIntoTwoAtTC`: splits a cubic segment of complex points at `t`.
pub fn split_cubic_into_two_at_t_c(
	pt1: Complex,
	pt2: Complex,
	pt3: Complex,
	pt4: Complex,
	t: f64,
) -> ([Complex; 4], [Complex; 4]) {
	let t2 = t * t;
	let _1_t = 1.0 - t;
	let _1_t_2 = _1_t * _1_t;
	let _2_t_1_t = 2.0 * t * _1_t;
	let point_at_t =
		_1_t_2 * _1_t * pt1 + 3.0 * (_1_t_2 * t * pt2 + _1_t * t2 * pt3) + t2 * t * pt4;
	let off1 = _1_t_2 * pt1 + _2_t_1_t * pt2 + t2 * pt3;
	let off2 = _1_t_2 * pt2 + _2_t_1_t * pt3 + t2 * pt4;
	let pt2 = pt1 + (pt2 - pt1) * t;
	let pt3 = pt4 + (pt3 - pt4) * _1_t;
	([pt1, pt2, off1, point_at_t], [point_at_t, off2, pt3, pt4])
}

/// `[0.0, *ts, 1.0]`.
fn with_ends(ts: &[f64]) -> Vec<f64> {
	let mut all = Vec::with_capacity(ts.len() + 2);
	all.push(0.0);
	all.extend_from_slice(ts);
	all.push(1.0);
	all
}

/// `_splitQuadraticAtT`.
fn split_quadratic_at_t_params(a: Point, b: Point, c: Point, ts: &[f64]) -> Vec<[Point; 3]> {
	let ts = with_ends(ts);
	let (ax, ay) = a;
	let (bx, by) = b;
	let (cx, cy) = c;
	let mut segments = Vec::with_capacity(ts.len() - 1);
	for w in ts.windows(2) {
		let (t1, t2) = (w[0], w[1]);
		let delta = t2 - t1;
		// Calculate the new a, b and c.
		let delta_2 = delta * delta;
		let a1x = ax * delta_2;
		let a1y = ay * delta_2;
		let b1x = (2.0 * ax * t1 + bx) * delta;
		let b1y = (2.0 * ay * t1 + by) * delta;
		let t1_2 = t1 * t1;
		let c1x = ax * t1_2 + bx * t1 + cx;
		let c1y = ay * t1_2 + by * t1 + cy;
		let (pt1, pt2, pt3) = calc_quadratic_points((a1x, a1y), (b1x, b1y), (c1x, c1y));
		segments.push([pt1, pt2, pt3]);
	}
	segments
}

/// `_splitCubicAtT`.
fn split_cubic_at_t_params(a: Point, b: Point, c: Point, d: Point, ts: &[f64]) -> Vec<[Point; 4]> {
	let ts = with_ends(ts);
	let (ax, ay) = a;
	let (bx, by) = b;
	let (cx, cy) = c;
	let (dx, dy) = d;
	let mut segments = Vec::with_capacity(ts.len() - 1);
	for w in ts.windows(2) {
		let (t1, t2) = (w[0], w[1]);
		let delta = t2 - t1;
		let delta_2 = delta * delta;
		let delta_3 = delta * delta_2;
		let t1_2 = t1 * t1;
		let t1_3 = t1 * t1_2;
		// Calculate the new a, b, c and d.
		let a1x = ax * delta_3;
		let a1y = ay * delta_3;
		let b1x = (3.0 * ax * t1 + bx) * delta_2;
		let b1y = (3.0 * ay * t1 + by) * delta_2;
		let c1x = (2.0 * bx * t1 + cx + 3.0 * ax * t1_2) * delta;
		let c1y = (2.0 * by * t1 + cy + 3.0 * ay * t1_2) * delta;
		let d1x = ax * t1_3 + bx * t1_2 + cx * t1 + dx;
		let d1y = ay * t1_3 + by * t1_2 + cy * t1 + dy;
		let (pt1, pt2, pt3, pt4) =
			calc_cubic_points((a1x, a1y), (b1x, b1y), (c1x, c1y), (d1x, d1y));
		segments.push([pt1, pt2, pt3, pt4]);
	}
	segments
}

/// `solveQuadratic`: the real roots of `a*x*x + b*x + c = 0`, neither sorted nor
/// unique.
pub fn solve_quadratic(a: f64, b: f64, c: f64) -> Vec<f64> {
	if a.abs() < EPSILON {
		if b.abs() < EPSILON {
			// A non-equation: no valid solution.
			Vec::new()
		} else {
			// A linear equation with one root.
			vec![-c / b]
		}
	} else {
		let dd = b * b - 4.0 * a * c;
		if dd >= 0.0 {
			let r = dd.sqrt();
			vec![(-b + r) / 2.0 / a, (-b - r) / 2.0 / a]
		} else {
			// Complex roots, ignored.
			Vec::new()
		}
	}
}

/// `solveCubic`: the real roots of `a*x*x*x + b*x*x + c*x + d = 0`, neither
/// sorted nor unique.
pub fn solve_cubic(a: f64, b: f64, c: f64, d: f64) -> Vec<f64> {
	// Adapted from CUBIC.C, "Solve a cubic polynomial", public domain by Ross
	// Cottrell.
	if a.abs() < EPSILON {
		// Not just a test for zero: for very small values of `a` the result
		// is unreliable, so this falls back to a quadratic.
		return solve_quadratic(b, c, d);
	}
	let a1 = b / a;
	let a2 = c / a;
	let a3 = d / a;

	let q = (a1 * a1 - 3.0 * a2) / 9.0;
	let r = (2.0 * a1 * a1 * a1 - 9.0 * a1 * a2 + 27.0 * a3) / 54.0;

	let mut r2 = r * r;
	let mut q3 = q * q * q;
	if r2 < EPSILON {
		r2 = 0.0;
	}
	if q3.abs() < EPSILON {
		q3 = 0.0;
	}
	let r2_q3 = r2 - q3;

	if r2 == 0.0 && q3 == 0.0 {
		let x = round_digits(-a1 / 3.0, EPSILON_DIGITS);
		vec![x, x, x]
	} else if r2_q3 <= EPSILON * 0.5 {
		// The EPSILON * 0.5 above ensures that q3 is not zero.
		let theta = (r / q3.sqrt()).clamp_py(-1.0, 1.0).acos();
		let r_q2 = -2.0 * q.sqrt();
		let a1_3 = a1 / 3.0;
		let x0 = r_q2 * (theta / 3.0).cos() - a1_3;
		let x1 = r_q2 * ((theta + 2.0 * std::f64::consts::PI) / 3.0).cos() - a1_3;
		let x2 = r_q2 * ((theta + 4.0 * std::f64::consts::PI) / 3.0).cos() - a1_3;
		let mut xs = [x0, x1, x2];
		crate::py::sort_floats(&mut xs);
		let [mut x0, mut x1, mut x2] = xs;
		// Merge the roots that are close enough.
		if x1 - x0 < EPSILON && x2 - x1 < EPSILON {
			let x = round_digits((x0 + x1 + x2) / 3.0, EPSILON_DIGITS);
			(x0, x1, x2) = (x, x, x);
		} else if x1 - x0 < EPSILON {
			let x = round_digits((x0 + x1) / 2.0, EPSILON_DIGITS);
			(x0, x1) = (x, x);
			x2 = round_digits(x2, EPSILON_DIGITS);
		} else if x2 - x1 < EPSILON {
			x0 = round_digits(x0, EPSILON_DIGITS);
			let x = round_digits((x1 + x2) / 2.0, EPSILON_DIGITS);
			(x1, x2) = (x, x);
		} else {
			x0 = round_digits(x0, EPSILON_DIGITS);
			x1 = round_digits(x1, EPSILON_DIGITS);
			x2 = round_digits(x2, EPSILON_DIGITS);
		}
		vec![x0, x1, x2]
	} else {
		let mut x = (r2_q3.sqrt() + r.abs()).powf(1.0 / 3.0);
		x += q / x;
		if r >= 0.0 {
			x = -x;
		}
		vec![round_digits(x - a1 / 3.0, EPSILON_DIGITS)]
	}
}

/// `max(min(x, hi), lo)` with Python's `min` and `max`.
trait ClampPy {
	fn clamp_py(self, lo: f64, hi: f64) -> f64;
}

impl ClampPy for f64 {
	fn clamp_py(self, lo: f64, hi: f64) -> f64 {
		let m = if hi < self { hi } else { self };
		if lo > m { lo } else { m }
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

/// `calcCubicParametersC`.
fn calc_cubic_parameters_c(
	pt1: Complex,
	pt2: Complex,
	pt3: Complex,
	pt4: Complex,
) -> (Complex, Complex, Complex, Complex) {
	let c = (pt2 - pt1) * 3.0;
	let b = (pt3 - pt2) * 3.0 - c;
	let a = pt4 - pt1 - c - b;
	(a, b, c, pt1)
}

/// `calcQuadraticPoints`: the control points of the polynomial.
pub fn calc_quadratic_points(a: Point, b: Point, c: Point) -> (Point, Point, Point) {
	let (ax, ay) = a;
	let (bx, by) = b;
	let (cx, cy) = c;
	let x1 = cx;
	let y1 = cy;
	let x2 = (bx * 0.5) + cx;
	let y2 = (by * 0.5) + cy;
	let x3 = ax + bx + cx;
	let y3 = ay + by + cy;
	((x1, y1), (x2, y2), (x3, y3))
}

/// `calcCubicPoints`: the control points of the polynomial.
pub fn calc_cubic_points(a: Point, b: Point, c: Point, d: Point) -> (Point, Point, Point, Point) {
	let (ax, ay) = a;
	let (bx, by) = b;
	let (cx, cy) = c;
	let (dx, dy) = d;
	let x1 = dx;
	let y1 = dy;
	let x2 = (cx / 3.0) + dx;
	let y2 = (cy / 3.0) + dy;
	let x3 = (bx + cx) / 3.0 + x2;
	let y3 = (by + cy) / 3.0 + y2;
	let x4 = ax + dx + cx + bx;
	let y4 = ay + dy + cy + by;
	((x1, y1), (x2, y2), (x3, y3), (x4, y4))
}

/// `calcCubicPointsC`.
fn calc_cubic_points_c(a: Complex, b: Complex, c: Complex, d: Complex) -> [Complex; 4] {
	let p2 = c * (1.0 / 3.0) + d;
	let p3 = (b + c) * (1.0 / 3.0) + p2;
	let p4 = a + b + c + d;
	[d, p2, p3, p4]
}

/// `linePointAtT`: the point at time `t` on a line.
pub fn line_point_at_t(pt1: Point, pt2: Point, t: f64) -> Point {
	(pt1.0 * (1.0 - t) + pt2.0 * t, pt1.1 * (1.0 - t) + pt2.1 * t)
}

/// `quadraticPointAtT`: the point at time `t` on a quadratic segment.
pub fn quadratic_point_at_t(pt1: Point, pt2: Point, pt3: Point, t: f64) -> Point {
	let x = (1.0 - t) * (1.0 - t) * pt1.0 + 2.0 * (1.0 - t) * t * pt2.0 + t * t * pt3.0;
	let y = (1.0 - t) * (1.0 - t) * pt1.1 + 2.0 * (1.0 - t) * t * pt2.1 + t * t * pt3.1;
	(x, y)
}

/// `cubicPointAtT`: the point at time `t` on a cubic segment.
pub fn cubic_point_at_t(pt1: Point, pt2: Point, pt3: Point, pt4: Point, t: f64) -> Point {
	let t2 = t * t;
	let _1_t = 1.0 - t;
	let _1_t_2 = _1_t * _1_t;
	let x = _1_t_2 * _1_t * pt1.0 + 3.0 * (_1_t_2 * t * pt2.0 + _1_t * t2 * pt3.0) + t2 * t * pt4.0;
	let y = _1_t_2 * _1_t * pt1.1 + 3.0 * (_1_t_2 * t * pt2.1 + _1_t * t2 * pt3.1) + t2 * t * pt4.1;
	(x, y)
}

/// `cubicPointAtTC`.
pub fn cubic_point_at_t_c(
	pt1: Complex,
	pt2: Complex,
	pt3: Complex,
	pt4: Complex,
	t: f64,
) -> Complex {
	let t2 = t * t;
	let _1_t = 1.0 - t;
	let _1_t_2 = _1_t * _1_t;
	_1_t_2 * _1_t * pt1 + 3.0 * (_1_t_2 * t * pt2 + _1_t * t2 * pt3) + t2 * t * pt4
}

/// `segmentPointAtT`: the point at time `t` on a line, quadratic or cubic
/// segment.
pub fn segment_point_at_t(seg: &[Point], t: f64) -> Result<Point, ValueError> {
	match *seg {
		[a, b] => Ok(line_point_at_t(a, b, t)),
		[a, b, c] => Ok(quadratic_point_at_t(a, b, c, t)),
		[a, b, c, d] => Ok(cubic_point_at_t(a, b, c, d, t)),
		_ => Err(ValueError("Unknown curve degree")),
	}
}

/// `_line_t_of_pt`: the time of a point on a line; -1 if the line is a point.
pub fn line_t_of_pt(s: Point, e: Point, pt: Point) -> f64 {
	let (sx, sy) = s;
	let (ex, ey) = e;
	let (px, py) = pt;
	if (sx - ex).abs() < EPSILON && (sy - ey).abs() < EPSILON {
		// The line is a point.
		return -1.0;
	}
	// Use the largest.
	if (sx - ex).abs() > (sy - ey).abs() {
		(px - sx) / (ex - sx)
	} else {
		(py - sy) / (ey - sy)
	}
}

/// `_both_points_are_on_same_side_of_origin`.
fn both_points_are_on_same_side_of_origin(a: Point, b: Point, origin: Point) -> bool {
	let x_diff = (a.0 - origin.0) * (b.0 - origin.0);
	let y_diff = (a.1 - origin.1) * (b.1 - origin.1);
	!(x_diff <= 0.0 && y_diff <= 0.0)
}

fn intersection(pt: Point, s1: Point, e1: Point, s2: Point, e2: Point) -> Intersection {
	Intersection {
		pt,
		t1: line_t_of_pt(s1, e1, pt),
		t2: line_t_of_pt(s2, e2, pt),
	}
}

/// `lineLineIntersections`: where two lines intersect.
pub fn line_line_intersections(s1: Point, e1: Point, s2: Point, e2: Point) -> Vec<Intersection> {
	let (s1x, s1y) = s1;
	let (e1x, e1y) = e1;
	let (s2x, s2y) = s2;
	let (e2x, e2y) = e2;
	if is_close(s2x, e2x) && is_close(s1x, e1x) {
		// Parallel vertical.
		return Vec::new();
	}
	if is_close(s2y, e2y) && is_close(s1y, e1y) && !is_close(s1y, s2y) {
		// Parallel horizontal.
		return Vec::new();
	}
	if is_close(s2x, e2x) && is_close(s2y, e2y) {
		// The line segment is tiny.
		return Vec::new();
	}
	if is_close(s1x, e1x) && is_close(s1y, e1y) {
		// The line segment is tiny.
		return Vec::new();
	}
	if is_close(e1x, s1x) {
		let x = s1x;
		let slope34 = (e2y - s2y) / (e2x - s2x);
		let y = slope34 * (x - s2x) + s2y;
		return vec![intersection((x, y), s1, e1, s2, e2)];
	}
	if is_close(s2x, e2x) {
		let x = s2x;
		let slope12 = (e1y - s1y) / (e1x - s1x);
		let y = slope12 * (x - s1x) + s1y;
		return vec![intersection((x, y), s1, e1, s2, e2)];
	}
	let slope12 = (e1y - s1y) / (e1x - s1x);
	let slope34 = (e2y - s2y) / (e2x - s2x);
	if is_close(slope12, slope34) {
		return Vec::new();
	}
	let x = (slope12 * s1x - s1y - slope34 * s2x + s2y) / (slope12 - slope34);
	let y = slope12 * (x - s1x) + s1y;
	let pt = (x, y);
	if both_points_are_on_same_side_of_origin(pt, e1, s1)
		&& both_points_are_on_same_side_of_origin(pt, s2, e2)
	{
		return vec![intersection(pt, s1, e1, s2, e2)];
	}
	Vec::new()
}

/// `_alignment_transformation`: the transformation that puts a segment on the
/// X axis, starting at the origin.
fn alignment_transformation(segment: &[Point]) -> crate::transform::Transform {
	let start = segment[0];
	let end = segment[segment.len() - 1];
	let angle = (end.1 - start.1).atan2(end.0 - start.0);
	IDENTITY.rotate(-angle).translate(-start.0, -start.1)
}

/// `_curve_line_intersections_t`.
fn curve_line_intersections_t(curve: &[Point], line: &[Point]) -> Result<Vec<f64>, ValueError> {
	let aligned = alignment_transformation(line).transform_points(curve);
	let intersections = match *aligned {
		[p1, p2, p3] => {
			let (a, b, c) = calc_quadratic_parameters(p1, p2, p3);
			solve_quadratic(a.1, b.1, c.1)
		}
		[p1, p2, p3, p4] => {
			let (a, b, c, d) = calc_cubic_parameters(p1, p2, p3, p4);
			solve_cubic(a.1, b.1, c.1, d.1)
		}
		_ => return Err(ValueError("Unknown curve degree")),
	};
	let mut ts: Vec<f64> = intersections
		.into_iter()
		.filter(|&i| (0.0..=1.0).contains(&i))
		.collect();
	crate::py::sort_floats(&mut ts);
	Ok(ts)
}

/// `curveLineIntersections`: where a quadratic or cubic segment and a line
/// intersect.
pub fn curve_line_intersections(
	curve: &[Point],
	line: &[Point],
) -> Result<Vec<Intersection>, ValueError> {
	if !matches!(curve.len(), 3 | 4) {
		return Err(ValueError("Unknown curve degree"));
	}
	let (ls, le) = (line[0], line[1]);
	let mut intersections = Vec::new();
	for t in curve_line_intersections_t(curve, line)? {
		let pt = segment_point_at_t(curve, t)?;
		// Back-project the point onto the line, to avoid problems with the
		// numerical accuracy of vertical and horizontal lines.
		let line_t = line_t_of_pt(ls, le, pt);
		let pt = line_point_at_t(ls, le, line_t);
		intersections.push(Intersection {
			pt,
			t1: t,
			t2: line_t,
		});
	}
	Ok(intersections)
}

/// `_curve_bounds`.
fn curve_bounds(c: &[Point]) -> Result<Rect, ValueError> {
	match *c {
		[a, b, c] => Ok(calc_quadratic_bounds(a, b, c)),
		[a, b, c, d] => Ok(calc_cubic_bounds(a, b, c, d)),
		_ => Err(ValueError("Unknown curve degree")),
	}
}

/// `_split_segment_at_t`.
fn split_segment_at_t(c: &[Point], t: f64) -> Result<(Vec<Point>, Vec<Point>), ValueError> {
	match *c {
		[s, e] => {
			let mid = line_point_at_t(s, e, t);
			Ok((vec![s, mid], vec![mid, e]))
		}
		[a, b, c] => {
			let split = split_quadratic_at_t(a, b, c, &[t]);
			Ok((split[0].to_vec(), split[1].to_vec()))
		}
		[a, b, c, d] => {
			let split = split_cubic_at_t(a, b, c, d, &[t]);
			Ok((split[0].to_vec(), split[1].to_vec()))
		}
		_ => Err(ValueError("Unknown curve degree")),
	}
}

/// `_curve_curve_intersections_t`.
fn curve_curve_intersections_t(
	curve1: &[Point],
	curve2: &[Point],
	precision: f64,
	range1: (f64, f64),
	range2: (f64, f64),
) -> Result<Vec<(f64, f64)>, ValueError> {
	let bounds1 = curve_bounds(curve1)?;
	let bounds2 = curve_bounds(curve2)?;

	// If the bounds don't intersect, go home.
	if sect_rect(bounds1, bounds2).is_none() {
		return Ok(Vec::new());
	}

	let midpoint = |r: (f64, f64)| 0.5 * (r.0 + r.1);

	// If they overlap but are tiny, approximate.
	if rect_area(bounds1) < precision && rect_area(bounds2) < precision {
		return Ok(vec![(midpoint(range1), midpoint(range2))]);
	}

	let (c11, c12) = split_segment_at_t(curve1, 0.5)?;
	let c11_range = (range1.0, midpoint(range1));
	let c12_range = (midpoint(range1), range1.1);

	let (c21, c22) = split_segment_at_t(curve2, 0.5)?;
	let c21_range = (range2.0, midpoint(range2));
	let c22_range = (midpoint(range2), range2.1);

	let mut found = Vec::new();
	found.extend(curve_curve_intersections_t(
		&c11, &c21, precision, c11_range, c21_range,
	)?);
	found.extend(curve_curve_intersections_t(
		&c12, &c21, precision, c12_range, c21_range,
	)?);
	found.extend(curve_curve_intersections_t(
		&c11, &c22, precision, c11_range, c22_range,
	)?);
	found.extend(curve_curve_intersections_t(
		&c12, &c22, precision, c12_range, c22_range,
	)?);

	let mut seen = std::collections::HashSet::new();
	let mut unique_values = Vec::new();
	for ts in found {
		let key = ((ts.0 / precision) as i64, (ts.1 / precision) as i64);
		if seen.insert(key) {
			unique_values.push(ts);
		}
	}
	Ok(unique_values)
}

/// `_is_linelike`: whether a segment is a straight line.
pub fn is_linelike(segment: &[Point]) -> bool {
	alignment_transformation(segment)
		.transform_points(segment)
		.iter()
		.all(|p| is_close(p.1, 0.0))
}

/// `curveCurveIntersections`: where two quadratic or cubic segments intersect.
pub fn curve_curve_intersections(
	curve1: &[Point],
	curve2: &[Point],
) -> Result<Vec<Intersection>, ValueError> {
	if is_linelike(curve1) {
		let line1 = [curve1[0], curve1[curve1.len() - 1]];
		if is_linelike(curve2) {
			let line2 = [curve2[0], curve2[curve2.len() - 1]];
			return Ok(line_line_intersections(
				line1[0], line1[1], line2[0], line2[1],
			));
		}
		let hits = curve_line_intersections(curve2, &line1)?;
		// The curve is passed first to this function but is the second
		// segment, so t1 and t2 are swapped in the result.
		return Ok(hits
			.into_iter()
			.map(|x| Intersection {
				pt: x.pt,
				t1: x.t2,
				t2: x.t1,
			})
			.collect());
	} else if is_linelike(curve2) {
		let line2 = [curve2[0], curve2[curve2.len() - 1]];
		return curve_line_intersections(curve1, &line2);
	}
	curve_curve_intersections_t(curve1, curve2, 1e-3, (0.0, 1.0), (0.0, 1.0))?
		.into_iter()
		.map(|ts| {
			Ok(Intersection {
				pt: segment_point_at_t(curve1, ts.0)?,
				t1: ts.0,
				t2: ts.1,
			})
		})
		.collect()
}

/// `segmentSegmentIntersections`: where two segments of any degree intersect.
pub fn segment_segment_intersections(
	seg1: &[Point],
	seg2: &[Point],
) -> Result<Vec<Intersection>, ValueError> {
	// Arrange by degree.
	let (seg1, seg2, swapped) = if seg2.len() > seg1.len() {
		(seg2, seg1, true)
	} else {
		(seg1, seg2, false)
	};
	let intersections = if seg1.len() > 2 {
		if seg2.len() > 2 {
			curve_curve_intersections(seg1, seg2)?
		} else {
			curve_line_intersections(seg1, seg2)?
		}
	} else if seg1.len() == 2 && seg2.len() == 2 {
		line_line_intersections(seg1[0], seg1[1], seg2[0], seg2[1])
	} else {
		return Err(ValueError(
			"Couldn't work out which intersection function to use",
		));
	};
	if !swapped {
		return Ok(intersections);
	}
	Ok(intersections
		.into_iter()
		.map(|i| Intersection {
			pt: i.pt,
			t1: i.t2,
			t2: i.t1,
		})
		.collect())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::py::format_g;

	/// `_segmentrepr` of a list of segments, one per line (`printSegments`).
	fn segments<const N: usize>(segments: &[[Point; N]]) -> Vec<String> {
		segments
			.iter()
			.map(|s| {
				let pts: Vec<String> = s
					.iter()
					.map(|p| format!("({}, {})", format_g(p.0, 6), format_g(p.1, 6)))
					.collect();
				format!("({})", pts.join(", "))
			})
			.collect()
	}

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

	#[test]
	fn quadratic_arc_length() {
		let cases = [
			((0.0, 0.0), (0.0, 0.0), (0.0, 0.0), 0.0),
			((0.0, 0.0), (50.0, 0.0), (80.0, 0.0), 80.0),
			((0.0, 0.0), (0.0, 50.0), (0.0, 80.0), 80.0),
			((0.0, 0.0), (50.0, 20.0), (100.0, 40.0), 107.70329614269008),
			((0.0, 0.0), (0.0, 100.0), (100.0, 0.0), 154.02976155645263),
			((0.0, 0.0), (0.0, 50.0), (100.0, 0.0), 120.21581243984076),
			((0.0, 0.0), (50.0, -10.0), (80.0, 50.0), 102.53273816445825),
			((0.0, 0.0), (40.0, 0.0), (-40.0, 0.0), 66.66666666666667),
			((0.0, 0.0), (40.0, 0.0), (0.0, 0.0), 40.0),
		];
		for (p1, p2, p3, expected) in cases {
			assert_eq!(calc_quadratic_arc_length(p1, p2, p3), expected);
		}
	}

	#[test]
	fn cubic_arc_length() {
		let cases = [
			(
				[(0.0, 0.0), (25.0, 100.0), (75.0, 100.0), (100.0, 0.0)],
				190.04332968932817,
			),
			(
				[(0.0, 0.0), (50.0, 0.0), (100.0, 50.0), (100.0, 100.0)],
				154.8852074945903,
			),
			(
				[(0.0, 0.0), (50.0, 0.0), (100.0, 0.0), (150.0, 0.0)],
				149.99999999999991,
			),
			(
				[(0.0, 0.0), (50.0, 0.0), (100.0, 0.0), (-50.0, 0.0)],
				136.9267662156362,
			),
			(
				[(0.0, 0.0), (50.0, 0.0), (100.0, -50.0), (-50.0, 0.0)],
				154.80848416537057,
			),
		];
		for ([p1, p2, p3, p4], expected) in cases {
			assert_eq!(approximate_cubic_arc_length(p1, p2, p3, p4), expected);
		}
	}

	#[test]
	fn split_line_doctests() {
		let o = (0.0, 0.0);
		assert_eq!(
			segments(&split_line(o, (100.0, 100.0), 50.0, true)),
			["((0, 0), (50, 50))", "((50, 50), (100, 100))"]
		);
		assert_eq!(
			segments(&split_line(o, (100.0, 100.0), 100.0, true)),
			["((0, 0), (100, 100))"]
		);
		assert_eq!(
			segments(&split_line(o, (100.0, 100.0), 0.0, true)),
			["((0, 0), (0, 0))", "((0, 0), (100, 100))"]
		);
		assert_eq!(
			segments(&split_line(o, (100.0, 100.0), 0.0, false)),
			["((0, 0), (0, 0))", "((0, 0), (100, 100))"]
		);
		assert_eq!(
			segments(&split_line((100.0, 0.0), o, 50.0, false)),
			["((100, 0), (50, 0))", "((50, 0), (0, 0))"]
		);
		assert_eq!(
			segments(&split_line((0.0, 100.0), o, 50.0, true)),
			["((0, 100), (0, 50))", "((0, 50), (0, 0))"]
		);
	}

	#[test]
	fn split_quadratic_doctests() {
		let (a, b, c) = ((0.0, 0.0), (50.0, 100.0), (100.0, 0.0));
		assert_eq!(
			segments(&split_quadratic(a, b, c, 150.0, false)),
			["((0, 0), (50, 100), (100, 0))"]
		);
		assert_eq!(
			segments(&split_quadratic(a, b, c, 50.0, false)),
			[
				"((0, 0), (25, 50), (50, 50))",
				"((50, 50), (75, 50), (100, 0))"
			]
		);
		assert_eq!(
			segments(&split_quadratic(a, b, c, 25.0, false)),
			[
				"((0, 0), (12.5, 25), (25, 37.5))",
				"((25, 37.5), (62.5, 75), (100, 0))"
			]
		);
		assert_eq!(
			segments(&split_quadratic(a, b, c, 25.0, true)),
			[
				"((0, 0), (7.32233, 14.6447), (14.6447, 25))",
				"((14.6447, 25), (50, 75), (85.3553, 25))",
				"((85.3553, 25), (92.6777, 14.6447), (100, 0))",
			]
		);
		assert_eq!(
			segments(&split_quadratic(a, b, c, 50.0, true)),
			[
				"((0, 0), (25, 50), (50, 50))",
				"((50, 50), (50, 50), (50, 50))",
				"((50, 50), (75, 50), (100, 0))",
			]
		);
	}

	#[test]
	fn split_cubic_doctests() {
		let (a, b, c, d) = ((0.0, 0.0), (25.0, 100.0), (75.0, 100.0), (100.0, 0.0));
		assert_eq!(
			segments(&split_cubic(a, b, c, d, 150.0, false)),
			["((0, 0), (25, 100), (75, 100), (100, 0))"]
		);
		assert_eq!(
			segments(&split_cubic(a, b, c, d, 50.0, false)),
			[
				"((0, 0), (12.5, 50), (31.25, 75), (50, 75))",
				"((50, 75), (68.75, 75), (87.5, 50), (100, 0))",
			]
		);
		assert_eq!(
			segments(&split_cubic(a, b, c, d, 25.0, true)),
			[
				"((0, 0), (2.29379, 9.17517), (4.79804, 17.5085), (7.47414, 25))",
				"((7.47414, 25), (31.2886, 91.6667), (68.7114, 91.6667), (92.5259, 25))",
				"((92.5259, 25), (95.202, 17.5085), (97.7062, 9.17517), (100, 0))",
			]
		);
		assert_eq!(
			segments(&split_quadratic_at_t(
				(0.0, 0.0),
				(50.0, 100.0),
				(100.0, 0.0),
				&[0.5, 0.75]
			)),
			[
				"((0, 0), (25, 50), (50, 50))",
				"((50, 50), (62.5, 50), (75, 37.5))",
				"((75, 37.5), (87.5, 25), (100, 0))",
			]
		);
		assert_eq!(
			segments(&split_cubic_at_t(a, b, c, d, &[0.5, 0.75])),
			[
				"((0, 0), (12.5, 50), (31.25, 75), (50, 75))",
				"((50, 75), (59.375, 75), (68.75, 68.75), (77.3438, 56.25))",
				"((77.3438, 56.25), (85.9375, 43.75), (93.75, 25), (100, 0))",
			]
		);
	}

	#[test]
	fn solve_cubic_doctests() {
		let r = solve_cubic(1.0, 1.0, -6.0, 0.0);
		assert_eq!(r, [-3.0, -0.0, 2.0]);
		assert!(r[1].is_sign_negative());
		assert_eq!(solve_cubic(-10.0, -9.0, 48.0, -29.0), [-2.9, 1.0, 1.0]);
		assert_eq!(
			solve_cubic(-9.875, -9.0, 47.625, -28.75),
			[-2.911392, 1.0, 1.0]
		);
		assert_eq!(solve_cubic(1.0, -4.5, 6.75, -3.375), [1.5, 1.5, 1.5]);
		assert_eq!(
			solve_cubic(-12.0, 18.0, -9.0, 1.50023651123),
			[0.5, 0.5, 0.5]
		);
		assert_eq!(
			solve_cubic(9.0, 0.0, 0.0, -7.62939453125e-05),
			[-0.0, -0.0, -0.0]
		);
	}

	#[test]
	fn intersections() -> Result<(), ValueError> {
		let a = line_line_intersections(
			(310.0, 389.0),
			(453.0, 222.0),
			(289.0, 251.0),
			(447.0, 367.0),
		);
		assert_eq!(a.len(), 1);
		assert_eq!(a[0].pt, (374.44882952482897, 313.73458370177315));
		assert_eq!(
			(a[0].t1, a[0].t2),
			(0.45069111555824465, 0.5408153767394238)
		);

		let curve = [(100.0, 240.0), (30.0, 60.0), (210.0, 230.0), (160.0, 30.0)];
		let line = [(25.0, 260.0), (230.0, 20.0)];
		let i = curve_line_intersections(&curve, &line)?;
		assert_eq!(i.len(), 3);
		assert_eq!(i[0].pt, (84.9000930760723, 189.87306176459828));

		let curve1 = [(10.0, 100.0), (90.0, 30.0), (40.0, 140.0), (220.0, 220.0)];
		let curve2 = [(5.0, 150.0), (180.0, 20.0), (80.0, 250.0), (210.0, 190.0)];
		let i = curve_curve_intersections(&curve1, &curve2)?;
		assert_eq!(i.len(), 3);
		assert_eq!(i[0].pt, (81.7831487395506, 109.88904552375288));
		let i = segment_segment_intersections(&curve1, &curve2)?;
		assert_eq!(i[0].pt, (81.7831487395506, 109.88904552375288));
		let i = segment_segment_intersections(&curve, &line)?;
		assert_eq!(i.len(), 3);
		assert_eq!(i[0].pt, (84.9000930760723, 189.87306176459828));
		Ok(())
	}
}
