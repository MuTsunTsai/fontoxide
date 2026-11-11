//! Port of `fontTools.cu2qu`: cubic Bézier curves approximated by quadratic
//! splines.
//!
//! | fontoxide   | fontTools                  |
//! |-------------|----------------------------|
//! | crate root  | `fontTools.cu2qu.cu2qu`    |
//! | [`Error`], [`errors`] | `fontTools.cu2qu.errors` |
//! | [`ufo`]     | `fontTools.cu2qu.ufo`      |
//! | [`cli`]     | `fontTools.cu2qu.cli`      |
//! | [`pens`]    | `fontTools.pens.cu2quPen`  |
//!
//! The arithmetic is the one of fontTools in pure Python (not compiled with
//! Cython), with Python's complex numbers ([`Complex`]), so the results are
//! bit for bit the same.

#![forbid(unsafe_code)]

pub mod cli;
pub mod errors;
pub mod pens;
pub mod ufo;

pub use errors::{Incompatibility, IncompatibleFontsError, IncompatibleGlyphsError};

use fontoxide_misc::array_tools::Point;
use fontoxide_misc::complex::{Complex, J};

/// `MAX_N`: the largest number of quadratic segments tried.
pub const MAX_N: usize = 100;

/// A cu2qu error (`fontTools.cu2qu.errors`).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Error {
	/// `ApproxNotFoundError`: no approximation within the tolerance, with at
	/// most [`MAX_N`] segments.
	#[error("no approximation found: {0}")]
	ApproxNotFound(String),
	/// `ValueError`.
	#[error("{0}")]
	Value(&'static str),
}

/// `dot`: the dot product of two vectors; tiny values become 0.
fn dot(v1: Complex, v2: Complex) -> f64 {
	let result = (v1 * v2.conj()).re;
	// Perpendicular vectors can give slightly different results in Python and
	// Cython; fontTools normalizes them, as the result is a denominator.
	if result.abs() < 1e-15 { 0.0 } else { result }
}

/// `_complex_div_by_real`: two separate divisions.
fn div_real(z: Complex, den: f64) -> Complex {
	Complex::new(z.re / den, z.im / den)
}

/// `calc_cubic_points`.
fn calc_cubic_points(a: Complex, b: Complex, c: Complex, d: Complex) -> [Complex; 4] {
	let p1 = d;
	let p2 = div_real(c, 3.0) + d;
	let p3 = div_real(b + c, 3.0) + p2;
	let p4 = a + d + c + b;
	[p1, p2, p3, p4]
}

/// `calc_cubic_parameters`.
fn calc_cubic_parameters(p0: Complex, p1: Complex, p2: Complex, p3: Complex) -> [Complex; 4] {
	let c = (p1 - p0) * 3.0;
	let b = (p2 - p1) * 3.0 - c;
	let d = p0;
	let a = p3 - d - c - b;
	[a, b, c, d]
}

/// `split_cubic_into_n_iter`: the curve split into `n` parts of equal
/// curve time.
fn split_cubic_into_n(
	p0: Complex,
	p1: Complex,
	p2: Complex,
	p3: Complex,
	n: usize,
) -> Vec<[Complex; 4]> {
	match n {
		2 => split_cubic_into_two(p0, p1, p2, p3).to_vec(),
		3 => split_cubic_into_three(p0, p1, p2, p3).to_vec(),
		4 => {
			let [a, b] = split_cubic_into_two(p0, p1, p2, p3);
			let mut v = split_cubic_into_two(a[0], a[1], a[2], a[3]).to_vec();
			v.extend(split_cubic_into_two(b[0], b[1], b[2], b[3]));
			v
		}
		6 => {
			let [a, b] = split_cubic_into_two(p0, p1, p2, p3);
			let mut v = split_cubic_into_three(a[0], a[1], a[2], a[3]).to_vec();
			v.extend(split_cubic_into_three(b[0], b[1], b[2], b[3]));
			v
		}
		_ => split_cubic_into_n_gen(p0, p1, p2, p3, n),
	}
}

/// `_split_cubic_into_n_gen`.
fn split_cubic_into_n_gen(
	p0: Complex,
	p1: Complex,
	p2: Complex,
	p3: Complex,
	n: usize,
) -> Vec<[Complex; 4]> {
	let [a, b, c, d] = calc_cubic_parameters(p0, p1, p2, p3);
	let dt = 1.0 / n as f64;
	let delta_2 = dt * dt;
	let delta_3 = dt * delta_2;
	(0..n)
		.map(|i| {
			let t1 = i as f64 * dt;
			let t1_2 = t1 * t1;
			// Calculate the new a, b, c and d.
			let a1 = a * delta_3;
			let b1 = (3.0 * a * t1 + b) * delta_2;
			let c1 = (2.0 * b * t1 + c + 3.0 * a * t1_2) * dt;
			let d1 = a * t1 * t1_2 + b * t1_2 + c * t1 + d;
			calc_cubic_points(a1, b1, c1, d1)
		})
		.collect()
}

/// `split_cubic_into_two`: the curve split at t = 0.5.
pub fn split_cubic_into_two(
	p0: Complex,
	p1: Complex,
	p2: Complex,
	p3: Complex,
) -> [[Complex; 4]; 2] {
	let mid = (p0 + 3.0 * (p1 + p2) + p3) * 0.125;
	let deriv3 = (p3 + p2 - p1 - p0) * 0.125;
	[
		[p0, (p0 + p1) * 0.5, mid - deriv3, mid],
		[mid, mid + deriv3, (p2 + p3) * 0.5, p3],
	]
}

/// `split_cubic_into_three`: the curve split at t = 1/3 and t = 2/3.
pub fn split_cubic_into_three(
	p0: Complex,
	p1: Complex,
	p2: Complex,
	p3: Complex,
) -> [[Complex; 4]; 3] {
	let k = 1.0 / 27.0;
	let mid1 = (8.0 * p0 + 12.0 * p1 + 6.0 * p2 + p3) * k;
	let deriv1 = (p3 + 3.0 * p2 - 4.0 * p0) * k;
	let mid2 = (p0 + 6.0 * p1 + 12.0 * p2 + 8.0 * p3) * k;
	let deriv2 = (4.0 * p3 - 3.0 * p1 - p0) * k;
	[
		[p0, div_real(2.0 * p0 + p1, 3.0), mid1 - deriv1, mid1],
		[mid1, mid1 + deriv1, mid2 - deriv2, mid2],
		[mid2, mid2 + deriv2, div_real(p2 + 2.0 * p3, 3.0), p3],
	]
}

/// `cubic_approx_control`: a candidate control point of the quadratic
/// approximation.
fn cubic_approx_control(t: f64, p0: Complex, p1: Complex, p2: Complex, p3: Complex) -> Complex {
	let _p1 = p0 + (p1 - p0) * 1.5;
	let _p2 = p3 + (p2 - p3) * 1.5;
	_p1 + (_p2 - _p1) * t
}

/// `calc_intersect`: where two lines intersect; NaNs if they do not.
fn calc_intersect(a: Complex, b: Complex, c: Complex, d: Complex) -> Complex {
	let ab = b - a;
	let cd = d - c;
	let p = ab * J;
	let den = dot(p, cd);
	if den == 0.0 {
		// ZeroDivisionError. If 3 or 4 points are equal, there is an
		// intersection anyway: one of the off-curve points, so that a
		// one-curve solution can be tried (linebender/kurbo#484).
		if b == c && (a == b || c == d) {
			return b;
		}
		return Complex::new(f64::NAN, f64::NAN);
	}
	let h = dot(p, a - c) / den;
	c + cd * h
}

/// `cubic_farthest_fit_inside`: whether the inside of the curve is within
/// `tolerance` of the origin.
pub fn cubic_farthest_fit_inside(
	p0: Complex,
	p1: Complex,
	p2: Complex,
	p3: Complex,
	tolerance: f64,
) -> bool {
	// First p2 then p1, as p2 has a higher error early on.
	if p2.abs() <= tolerance && p1.abs() <= tolerance {
		return true;
	}
	// Split.
	let mid = (p0 + 3.0 * (p1 + p2) + p3) * 0.125;
	if mid.abs() > tolerance {
		return false;
	}
	let deriv3 = (p3 + p2 - p1 - p0) * 0.125;
	cubic_farthest_fit_inside(p0, (p0 + p1) * 0.5, mid - deriv3, mid, tolerance)
		&& cubic_farthest_fit_inside(mid, mid + deriv3, (p2 + p3) * 0.5, p3, tolerance)
}

/// `cubic_approx_quadratic`: a single quadratic within the tolerance.
fn cubic_approx_quadratic(cubic: &[Complex; 4], tolerance: f64) -> Option<Vec<Complex>> {
	let q1 = calc_intersect(cubic[0], cubic[1], cubic[2], cubic[3]);
	if q1.im.is_nan() {
		return None;
	}
	let c0 = cubic[0];
	let c3 = cubic[3];
	let c1 = c0 + (q1 - c0) * (2.0 / 3.0);
	let c2 = c3 + (q1 - c3) * (2.0 / 3.0);
	let zero = Complex::real(0.0);
	if !cubic_farthest_fit_inside(zero, c1 - cubic[1], c2 - cubic[2], zero, tolerance) {
		return None;
	}
	Some(vec![c0, q1, c3])
}

/// `cubic_approx_spline`: a spline of `n` quadratics within the tolerance
/// (`n + 2` points). Without `all_quadratic`, two segments mean the cubic
/// itself.
fn cubic_approx_spline(
	cubic: &[Complex; 4],
	n: usize,
	tolerance: f64,
	all_quadratic: bool,
) -> Option<Vec<Complex>> {
	if n == 1 {
		return cubic_approx_quadratic(cubic, tolerance);
	}
	if n == 2 && !all_quadratic {
		return Some(cubic.to_vec());
	}

	let cubics = split_cubic_into_n(cubic[0], cubic[1], cubic[2], cubic[3], n);

	// Calculate the spline of quadratics, checking the errors at the same time.
	let mut next_cubic = cubics[0];
	let mut next_q1 = cubic_approx_control(
		0.0,
		next_cubic[0],
		next_cubic[1],
		next_cubic[2],
		next_cubic[3],
	);
	let mut q2 = cubic[0];
	let mut d1 = Complex::new(0.0, 0.0);
	let mut spline = vec![cubic[0], next_q1];
	for i in 1..=n {
		// The current cubic to convert.
		let [_c0, c1, c2, c3] = next_cubic;

		// The current quadratic approximation of the current cubic.
		let q0 = q2;
		let q1 = next_q1;
		if i < n {
			next_cubic = *cubics.get(i).unwrap_or(&next_cubic);
			next_q1 = cubic_approx_control(
				i as f64 / (n - 1) as f64,
				next_cubic[0],
				next_cubic[1],
				next_cubic[2],
				next_cubic[3],
			);
			spline.push(next_q1);
			q2 = (q1 + next_q1) * 0.5;
		} else {
			q2 = c3;
		}

		// End-point deltas.
		let d0 = d1;
		d1 = q2 - c3;

		if d1.abs() > tolerance
			|| !cubic_farthest_fit_inside(
				d0,
				q0 + (q1 - q0) * (2.0 / 3.0) - c1,
				q2 + (q1 - q2) * (2.0 / 3.0) - c2,
				d1,
				tolerance,
			) {
			return None;
		}
	}
	spline.push(cubic[3]);
	Some(spline)
}

/// `repr` of a list of complex numbers.
fn repr_list(points: &[Complex]) -> String {
	let items: Vec<String> = points.iter().map(|p| p.repr()).collect();
	format!("[{}]", items.join(", "))
}

/// `curve_to_quadratic`: a quadratic spline approximating the cubic curve
/// within `max_err`. With `all_quadratic` false (`all_quadratic=False`),
/// the result is either a single quadratic (three points) or the cubic (four
/// points).
pub fn curve_to_quadratic(
	curve: &[Point; 4],
	max_err: f64,
	all_quadratic: bool,
) -> Result<Vec<Point>, Error> {
	let curve = curve.map(Complex::from_point);
	for n in 1..=MAX_N {
		if let Some(spline) = cubic_approx_spline(&curve, n, max_err, all_quadratic) {
			return Ok(spline.into_iter().map(Complex::to_point).collect());
		}
	}
	Err(Error::ApproxNotFound(repr_list(&curve)))
}

/// `curves_to_quadratic`: quadratic splines approximating the cubic curves,
/// each within its `max_errors`, all with the same number of points.
pub fn curves_to_quadratic(
	curves: &[[Point; 4]],
	max_errors: &[f64],
	all_quadratic: bool,
) -> Result<Vec<Vec<Point>>, Error> {
	let curves: Vec<[Complex; 4]> = curves.iter().map(|c| c.map(Complex::from_point)).collect();
	if max_errors.len() != curves.len() {
		return Err(Error::Value("max_errors must match the number of curves"));
	}
	if curves.is_empty() {
		return Ok(Vec::new());
	}

	let l = curves.len();
	let mut splines: Vec<Option<Vec<Complex>>> = vec![None; l];
	let mut last_i = 0;
	let mut i = 0;
	let mut n = 1;
	loop {
		match cubic_approx_spline(&curves[i], n, max_errors[i], all_quadratic) {
			None => {
				if n == MAX_N {
					break;
				}
				n += 1;
				last_i = i;
				continue;
			}
			Some(spline) => splines[i] = Some(spline),
		}
		i = (i + 1) % l;
		if i == last_i {
			// Done.
			return Ok(splines
				.into_iter()
				.map(|s| {
					s.unwrap_or_default()
						.into_iter()
						.map(Complex::to_point)
						.collect()
				})
				.collect());
		}
	}
	let all: Vec<String> = curves.iter().map(|c| repr_list(c)).collect();
	Err(Error::ApproxNotFound(format!("[{}]", all.join(", "))))
}

#[cfg(test)]
mod tests {
	use super::*;

	// The doctest of `curves_to_quadratic`.
	#[test]
	fn doctest() -> Result<(), Error> {
		let r = curves_to_quadratic(
			&[
				[(50.0, 50.0), (100.0, 100.0), (150.0, 100.0), (200.0, 50.0)],
				[(75.0, 50.0), (120.0, 100.0), (150.0, 75.0), (200.0, 60.0)],
			],
			&[1.0, 1.0],
			true,
		)?;
		assert_eq!(
			r,
			[
				vec![
					(50.0, 50.0),
					(75.0, 75.0),
					(125.0, 91.66666666666666),
					(175.0, 75.0),
					(200.0, 50.0)
				],
				vec![
					(75.0, 50.0),
					(97.5, 75.0),
					(135.41666666666666, 82.08333333333333),
					(175.0, 67.5),
					(200.0, 60.0)
				],
			]
		);
		Ok(())
	}
}
