//! Port of `fontTools.qu2cu`: quadratic splines approximated by cubic
//! curves.
//!
//! | fontoxide   | fontTools                  |
//! |-------------|----------------------------|
//! | crate root  | `fontTools.qu2cu.qu2cu`    |
//! | [`cli`]     | `fontTools.qu2cu.cli`      |
//! | [`pens`]    | `fontTools.pens.qu2cuPen`  |
//!
//! The arithmetic is the one of fontTools in pure Python (not compiled with
//! Cython), with Python's complex numbers ([`Complex`]), so the results are
//! bit for bit the same.

#![forbid(unsafe_code)]

mod argparse;
pub mod cli;
pub mod pens;

use fontoxide_misc::array_tools::Point;
use fontoxide_misc::bezier_tools::split_cubic_at_t_c;
use fontoxide_misc::complex::Complex;

/// A qu2cu error (`ValueError` and `AssertionError`).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub String);

/// `cubic_farthest_fit_inside`, copied from cu2qu.
fn cubic_farthest_fit_inside(
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

/// `elevate_quadratic`: the cubic equal to a quadratic curve.
pub fn elevate_quadratic(p0: Complex, p1: Complex, p2: Complex) -> [Complex; 4] {
	// https://pomax.github.io/bezierinfo/#reordering
	let p1_2_3 = p1 * (2.0 / 3.0);
	[p0, p0 * (1.0 / 3.0) + p1_2_3, p2 * (1.0 / 3.0) + p1_2_3, p2]
}

/// A cubic curve and the curve times of the cuts it replaces.
type Merged = ([Complex; 4], Vec<f64>);

/// `merge_curves`: one cubic with the end points and the tangents of
/// `curves[start..start + n]`, and the curve times of the cuts. `None` for a
/// `ZeroDivisionError`.
fn merge_curves(curves: &[[Complex; 4]], start: usize, n: usize) -> Result<Option<Merged>, Error> {
	// Reconstruct the t values of the cuts.
	let mut prod_ratio = 1.0;
	let mut sum_ratio = 1.0;
	let mut ts = vec![1.0];
	for k in 1..n {
		let ck = curves[start + k];
		let c_before = curves[start + k - 1];

		// |t_(k+1) - t_k| / |t_k - t_(k - 1)| = ratio
		if ck[0] != c_before[3] {
			return Err(Error("AssertionError".into()));
		}
		let den = (c_before[3] - c_before[2]).abs();
		if den == 0.0 {
			return Ok(None);
		}
		let ratio = (ck[1] - ck[0]).abs() / den;

		prod_ratio *= ratio;
		sum_ratio += prod_ratio;
		ts.push(sum_ratio);
	}

	// (t(n) - t(n - 1)) / (t_(1) - t(0)) = prod_ratio
	ts.pop();
	let ts: Vec<f64> = ts.iter().map(|t| t / sum_ratio).collect();

	let p0 = curves[start][0];
	let p1 = curves[start][1];
	let p2 = curves[start + n - 1][2];
	let p3 = curves[start + n - 1][3];

	// Build the curve by scaling the control points: `(p1 - p0) / ts[0]`, a
	// complex division, or by 1 if there are no cuts.
	let first = ts.first().copied().unwrap_or(1.0);
	let last = ts.last().map_or(1.0, |t| 1.0 - t);
	let Some(d1) = (p1 - p0).div_real(first) else {
		return Ok(None);
	};
	let Some(d2) = (p2 - p3).div_real(last) else {
		return Ok(None);
	};
	Ok(Some(([p0, p0 + d1, p3 + d2, p3], ts)))
}

/// `add_implicit_on_curves`: the on-curve points implied between successive
/// off-curve points, inserted.
pub fn add_implicit_on_curves(p: &[Complex]) -> Vec<Complex> {
	let mut q = p.to_vec();
	let num_offcurves = p.len().saturating_sub(2);
	for (count, i) in (1..num_offcurves).enumerate() {
		let off1 = p[i];
		let off2 = p[i + 1];
		let on = off1 + (off2 - off1) * 0.5;
		q.insert(i + 1 + count, on);
	}
	q
}

/// `quadratic_to_curves`: quadratic splines connected end to start (on-curve
/// points first and last, implied on-curve points between the off-curve
/// ones), as quadratic (three points) and cubic (four points) curves within
/// `max_err` (0.5 in fontTools). With `all_cubic`, only cubic curves.
pub fn quadratic_to_curves(
	quads: &[Vec<Point>],
	max_err: f64,
	all_cubic: bool,
) -> Result<Vec<Vec<Point>>, Error> {
	let quads: Vec<Vec<Complex>> = quads
		.iter()
		.map(|q| q.iter().map(|&p| Complex::from_point(p)).collect())
		.collect();
	Ok(quadratic_to_curves_c(&quads, max_err, all_cubic)?
		.into_iter()
		.map(|c| c.into_iter().map(Complex::to_point).collect())
		.collect())
}

/// [`quadratic_to_curves`] with complex points.
pub fn quadratic_to_curves_c(
	quads: &[Vec<Complex>],
	max_err: f64,
	all_cubic: bool,
) -> Result<Vec<Vec<Complex>>, Error> {
	if quads.is_empty() {
		return Ok(Vec::new());
	}
	if max_err <= 0.0 {
		return Err(Error("max_err must be greater than zero".into()));
	}
	if quads.iter().any(|s| s.len() < 3) {
		return Err(Error(
			"Quadratic splines must contain at least 3 points".into(),
		));
	}

	let mut q = vec![quads[0][0]];
	let mut costs = vec![1];
	let mut cost = 1;
	for p in quads {
		let last = q[q.len() - 1];
		if last != p[0] {
			return Err(Error(format!(
				"Quadratic splines must connect end-to-start; got {} then {}",
				last.repr(),
				p[0].repr()
			)));
		}
		for _ in 0..p.len() - 2 {
			cost += 1;
			costs.push(cost);
			costs.push(cost);
		}
		let qq = add_implicit_on_curves(p);
		costs.pop();
		q.extend_from_slice(&qq[1..]);
		cost += 1;
		costs.push(cost);
	}

	spline_to_curves(&q, &costs, max_err, all_cubic)
}

/// `Solution`: compared as a tuple, like the namedtuple.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
struct Solution {
	num_points: i64,
	error: f64,
	start_index: usize,
	is_cubic: bool,
}

/// `spline_to_curves`: `q` is a quadratic spline of alternating on-curve and
/// off-curve points; `costs` the cumulative number of points to encode it.
pub fn spline_to_curves(
	q: &[Complex],
	costs: &[i64],
	tolerance: f64,
	all_cubic: bool,
) -> Result<Vec<Vec<Complex>>, Error> {
	if q.len() < 3 {
		return Err(Error(
			"AssertionError: quadratic spline requires at least 3 points".into(),
		));
	}

	// Elevate the quadratic segments to cubic.
	let elevated: Vec<[Complex; 4]> = (0..q.len() - 2)
		.step_by(2)
		.map(|i| elevate_quadratic(q[i], q[i + 1], q[i + 2]))
		.collect();

	// Sharp corners have to be on-curve points.
	let mut forced = std::collections::HashSet::new();
	for i in 1..elevated.len() {
		let p0 = elevated[i - 1][2];
		let p1 = elevated[i][0];
		let p2 = elevated[i][1];
		if (p1 - p0).abs() + (p2 - p1).abs() > tolerance + (p2 - p0).abs() {
			forced.insert(i);
		}
	}

	// Dynamic programming: the solution with the fewest cubic curves, and
	// among them the one with the smallest error.
	let mut sols = vec![Solution {
		num_points: 0,
		error: 0.0,
		start_index: 0,
		is_cubic: false,
	}];
	let impossible = Solution {
		num_points: elevated.len() as i64 * 3 + 1,
		error: 0.0,
		start_index: 1,
		is_cubic: false,
	};
	let mut start = 0;
	for i in 1..=elevated.len() {
		let mut best_sol = impossible;
		for j in start..i {
			let (j_sol_count, j_sol_error) = (sols[j].num_points, sols[j].error);

			if !all_cubic {
				// A solution with quadratics between j and i.
				let this_count = costs[2 * i - 1] - costs[2 * j] + 1;
				let i_sol = Solution {
					num_points: j_sol_count + this_count,
					error: j_sol_error,
					start_index: i - j,
					is_cubic: false,
				};
				if i_sol < best_sol {
					best_sol = i_sol;
				}
				if this_count <= 3 {
					// Nothing better on the path below.
					continue;
				}
			}

			// Fit elevated[j..i] into one cubic.
			let Some((curve, ts)) = merge_curves(&elevated, j, i - j)? else {
				continue;
			};

			// Reconstruct the segments from the fitted curve.
			let reconstructed_iter =
				split_cubic_at_t_c(curve[0], curve[1], curve[2], curve[3], &ts);
			let mut reconstructed = Vec::new();

			// Knot errors.
			let mut error = 0.0f64;
			for (k, reconst) in reconstructed_iter.into_iter().enumerate() {
				let orig = elevated[j + k];
				let err = (reconst[3] - orig[3]).abs();
				// `max(error, err)`.
				if err > error {
					error = err;
				}
				if error > tolerance {
					break;
				}
				reconstructed.push(reconst);
			}
			if error > tolerance {
				// Not feasible.
				continue;
			}

			// Interior errors.
			for (k, reconst) in reconstructed.iter().enumerate() {
				let orig = elevated[j + k];
				let [p0, p1, p2, p3] = [0, 1, 2, 3].map(|m| reconst[m] - orig[m]);
				if !cubic_farthest_fit_inside(p0, p1, p2, p3, tolerance) {
					error = tolerance + 1.0;
					break;
				}
			}
			if error > tolerance {
				// Not feasible.
				continue;
			}

			// Save the best solution.
			let i_sol_count = j_sol_count + 3;
			let i_sol = Solution {
				num_points: i_sol_count,
				error: if error > j_sol_error {
					error
				} else {
					j_sol_error
				},
				start_index: i - j,
				is_cubic: true,
			};
			if i_sol < best_sol {
				best_sol = i_sol;
			}

			if i_sol_count == 3 {
				// Nothing better.
				break;
			}
		}

		sols.push(best_sol);
		if forced.contains(&i) {
			start = i;
		}
	}

	// Reconstruct the solution.
	let mut splits = Vec::new();
	let mut cubic = Vec::new();
	let mut i = sols.len() - 1;
	while i > 0 {
		let (count, is_cubic) = (sols[i].start_index, sols[i].is_cubic);
		splits.push(i);
		cubic.push(is_cubic);
		i -= count;
	}
	let mut curves = Vec::new();
	let mut j = 0;
	for (&i, &is_cubic) in splits.iter().zip(&cubic).rev() {
		if is_cubic {
			let (curve, _) = merge_curves(&elevated, j, i - j)?
				.ok_or_else(|| Error("ZeroDivisionError: float division by zero".into()))?;
			curves.push(curve.to_vec());
		} else {
			for k in j..i {
				curves.push(q[k * 2..k * 2 + 3].to_vec());
			}
		}
		j = i;
	}
	Ok(curves)
}
