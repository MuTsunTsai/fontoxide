//! `fontTools.varLib.iup`: interpolation of untouched points (IUP) and the
//! optimizer that leaves out the deltas IUP can infer.
//!
//! It lives in ttlib, rather than in `fontoxide-varlib`, because ttLib needs it:
//! glyph sets apply `iup_delta`, and `TupleVariation.optimize` calls
//! `iup_delta_optimize`. fontTools' `iup` module does not depend on the rest of
//! varLib. `fontoxide_varlib::iup` re-exports this module.

use std::collections::BTreeSet;

use fontoxide_misc::array_tools::Point;

pub use crate::var::{iup_delta, iup_segment};
use crate::{Error, Result};

const MAX_LOOKBACK: i64 = 8;

/// Python's `seq[i]`, where `-1` is the last item.
fn at(points: &[Point], i: i64) -> Point {
	points[if i < 0 { points.len() as i64 + i } else { i } as usize]
}

/// `can_iup_in_between`.
fn can_iup_in_between(deltas: &[Point], coords: &[Point], i: i64, j: i64, tolerance: f64) -> bool {
	let between = (i + 1) as usize..j as usize;
	let interp = iup_segment(
		&coords[between.clone()],
		at(coords, i),
		at(deltas, i),
		at(coords, j),
		at(deltas, j),
	);
	deltas[between].iter().zip(&interp).all(|(d, p)| {
		// `abs(complex(...))`.
		(d.0 - p.0).hypot(d.1 - p.1) <= tolerance
	})
}

/// `_iup_contour_bound_forced_set`: the points that must be encoded explicitly.
fn bound_forced_set(deltas: &[Point], coords: &[Point], tolerance: f64) -> BTreeSet<usize> {
	let n = deltas.len();
	let mut forced = BTreeSet::new();
	let get = |p: Point, j: usize| if j == 0 { p.0 } else { p.1 };
	for i in (0..n).rev() {
		let last = (i + n - 1) % n;
		let next = (i + 1) % n;
		for j in 0..2 {
			let cj = get(coords[i], j);
			let dj = get(deltas[i], j);
			let (lcj, ldj) = (get(coords[last], j), get(deltas[last], j));
			let (ncj, ndj) = (get(coords[next], j), get(deltas[next], j));
			let (c1, c2, d1, d2) = if lcj <= ncj {
				(lcj, ncj, ldj, ndj)
			} else {
				(ncj, lcj, ndj, ldj)
			};
			let mut force = false;
			if c1 == c2 {
				// The interpolation gives the same delta if both are equal, and
				// zero if they differ. This test has to be before the next one.
				if (d1 - d2).abs() > tolerance && dj.abs() > tolerance {
					force = true;
				}
			} else if c1 <= cj && cj <= c2 {
				// Between the adjacent points, the delta must be between theirs.
				if !(d1.min(d2) - tolerance <= dj && dj <= d1.max(d2) + tolerance) {
					force = true;
				}
			} else if d1 != d2 {
				// Otherwise, the delta should either match the closest, or have the
				// same sign as the interpolation of the two deltas.
				if cj < c1 {
					if dj.abs() > tolerance
						&& (dj - d1).abs() > tolerance
						&& ((dj - tolerance < d1) != (d1 < d2))
					{
						force = true;
					}
				} else if dj.abs() > tolerance
					&& (dj - d2).abs() > tolerance
					&& ((d2 < dj + tolerance) != (d1 < d2))
				{
					force = true;
				}
			}
			if force {
				forced.insert(i);
				break;
			}
		}
	}
	forced
}

/// The chain and costs of `_iup_contour_optimize_dp`, for indices from -1.
struct Dp {
	chain: Vec<i64>,
	costs: Vec<i64>,
}

impl Dp {
	fn chain(&self, i: i64) -> i64 {
		self.chain[(i + 1) as usize]
	}

	fn cost(&self, i: i64) -> i64 {
		self.costs[(i + 1) as usize]
	}
}

/// `_iup_contour_optimize_dp`: for each index, the least costly encoding of the
/// points up to it, with that point explicit.
fn optimize_dp(
	deltas: &[Point],
	coords: &[Point],
	forced: &BTreeSet<usize>,
	tolerance: f64,
	lookback: Option<usize>,
) -> Dp {
	let n = deltas.len() as i64;
	let lookback = (lookback.map_or(n, |l| l as i64)).min(MAX_LOOKBACK);
	let is_forced = |i: i64| i >= 0 && forced.contains(&(i as usize));
	let mut dp = Dp {
		chain: vec![i64::MIN],
		costs: vec![0],
	};
	for i in 0..n {
		let mut best_cost = dp.cost(i - 1) + 1;
		dp.costs.push(best_cost);
		dp.chain.push(i - 1);
		if is_forced(i - 1) {
			continue;
		}
		let stop = (i - lookback).max(-2);
		let mut j = i - 2;
		while j > stop {
			let cost = dp.cost(j) + 1;
			if cost < best_cost && can_iup_in_between(deltas, coords, j, i, tolerance) {
				best_cost = cost;
				dp.costs[(i + 1) as usize] = cost;
				dp.chain[(i + 1) as usize] = j;
			}
			if is_forced(j) {
				break;
			}
			j -= 1;
		}
	}
	dp
}

/// `_iup_contour_bound_forced_set`, for tests.
#[doc(hidden)]
pub fn iup_contour_bound_forced_set(
	deltas: &[Point],
	coords: &[Point],
	tolerance: f64,
) -> BTreeSet<usize> {
	bound_forced_set(deltas, coords, tolerance)
}

/// `_iup_contour_optimize_dp`, for tests: the chain and costs by index from -1.
#[doc(hidden)]
pub fn iup_contour_optimize_dp(
	deltas: &[Point],
	coords: &[Point],
	forced: &BTreeSet<usize>,
	tolerance: f64,
) -> (Vec<i64>, Vec<i64>) {
	let dp = optimize_dp(deltas, coords, forced, tolerance, None);
	(dp.chain, dp.costs)
}

/// `_rot_list`: item 0 goes to position `k`.
fn rot_list(l: &[Point], k: usize) -> Vec<Point> {
	let n = l.len();
	let k = k % n;
	let mut out = l[n - k..].to_vec();
	out.extend_from_slice(&l[..n - k]);
	out
}

/// `iup_contour_optimize`: which deltas of a contour must be explicit.
pub fn iup_contour_optimize(
	deltas: &[Point],
	coords: &[Point],
	tolerance: f64,
) -> Result<Vec<bool>> {
	let n = deltas.len();
	// If all are within tolerance distance of 0, encode nothing.
	if deltas.iter().all(|p| p.0.hypot(p.1) <= tolerance) {
		return Ok(vec![false; n]);
	}
	// If there's exactly one point, return it.
	if n == 1 {
		return Ok(vec![true]);
	}
	// If all deltas are exactly the same, return just the first one.
	if deltas.iter().all(|d| *d == deltas[0]) {
		let mut keep = vec![false; n];
		keep[0] = true;
		return Ok(keep);
	}

	let forced = bound_forced_set(deltas, coords, tolerance);
	let not_subset = || Error::Assertion("forced <= solution".into());
	if let Some(&last) = forced.last() {
		// Rotate the contour such that the last point is a forced one.
		let k = n - 1 - last;
		let rotated_deltas = rot_list(deltas, k);
		let rotated_coords = rot_list(coords, k);
		let rotated_forced: BTreeSet<usize> = forced.iter().map(|v| (v + k) % n).collect();
		let dp = optimize_dp(
			&rotated_deltas,
			&rotated_coords,
			&rotated_forced,
			tolerance,
			None,
		);
		let mut solution = BTreeSet::new();
		let mut i = n as i64 - 1;
		while i != -1 {
			solution.insert(i as usize);
			i = dp.chain(i);
		}
		if !rotated_forced.is_subset(&solution) {
			return Err(not_subset());
		}
		let mut keep = vec![false; n];
		for i in solution {
			keep[(i + n - k) % n] = true;
		}
		Ok(keep)
	} else {
		// Repeat the contour, solve the new case, then look for solutions of the
		// circular problem in the solution for the linear one.
		let doubled_deltas = [deltas, deltas].concat();
		let doubled_coords = [coords, coords].concat();
		let dp = optimize_dp(
			&doubled_deltas,
			&doubled_coords,
			&forced,
			tolerance,
			Some(n),
		);
		let n = n as i64;
		let mut best: Option<BTreeSet<usize>> = None;
		let mut best_cost = n + 1;
		for start in n - 1..2 * n - 1 {
			let mut solution = BTreeSet::new();
			let mut i = start;
			while i > start - n {
				solution.insert((i % n) as usize);
				i = dp.chain(i);
			}
			if i == start - n {
				let cost = dp.cost(start) - dp.cost(start - n);
				if cost <= best_cost {
					best = Some(solution);
					best_cost = cost;
				}
			}
		}
		let best = best.ok_or_else(not_subset)?;
		Ok((0..n as usize).map(|i| best.contains(&i)).collect())
	}
}

/// `iup_delta_optimize`: which deltas of an outline, whose contours end at `ends`
/// and whose last four points are the phantom points, must be explicit.
pub fn iup_delta_optimize(
	deltas: &[Point],
	coords: &[Point],
	ends: &[usize],
	tolerance: f64,
) -> Result<Vec<bool>> {
	let expected = ends.last().map_or(0, |e| e + 1) + 4;
	if !ends.is_sorted() || coords.len() != expected || deltas.len() != coords.len() {
		return Err(Error::Assertion(
			"sorted(ends) == ends and len(coords) == (ends[-1] + 1 if ends else 0) + 4".into(),
		));
	}
	let n = coords.len();
	let mut out = Vec::with_capacity(n);
	let mut start = 0;
	for end in ends.iter().copied().chain([n - 4, n - 3, n - 2, n - 1]) {
		out.extend(iup_contour_optimize(
			&deltas[start..=end],
			&coords[start..=end],
			tolerance,
		)?);
		start = end + 1;
	}
	Ok(out)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn optimize_keeps_what_interpolation_cannot_give() -> Result<()> {
		let coords = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
		// All the same.
		assert_eq!(
			iup_contour_optimize(&[(5.0, 5.0); 4], &coords, 0.5)?,
			[true, false, false, false]
		);
		// All within tolerance.
		assert_eq!(
			iup_contour_optimize(&[(0.1, 0.1); 4], &coords, 0.5)?,
			[false; 4]
		);
		// One point.
		assert_eq!(
			iup_contour_optimize(&[(1.0, 2.0)], &[(0.0, 0.0)], 0.5)?,
			[true]
		);
		// Values from fontTools.
		let coords = [(0.0, 0.0), (50.0, 0.0), (100.0, 0.0), (50.0, 50.0)];
		let deltas = [(0.0, 0.0), (5.0, 0.0), (10.0, 0.0), (20.0, 30.0)];
		assert_eq!(
			iup_contour_optimize(&deltas, &coords, 0.5)?,
			[false, true, true, true]
		);
		let coords = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
		let deltas = [(1.0, 0.0), (2.0, 0.0), (3.0, 5.0), (0.0, 9.0)];
		assert_eq!(iup_contour_optimize(&deltas, &coords, 0.5)?, [true; 4]);
		let coords = [
			(0.0, 0.0),
			(10.0, 0.0),
			(20.0, 0.0),
			(30.0, 0.0),
			(40.0, 5.0),
			(30.0, 10.0),
			(20.0, 10.0),
			(10.0, 10.0),
			(0.0, 10.0),
		];
		let deltas = [
			(0.0, 0.0),
			(1.0, 0.0),
			(2.0, 0.0),
			(3.0, 0.0),
			(9.0, 9.0),
			(3.0, 1.0),
			(2.0, 1.0),
			(1.0, 1.0),
			(0.0, 1.0),
		];
		assert_eq!(
			iup_contour_optimize(&deltas, &coords, 0.5)?,
			[true, false, false, true, true, true, false, false, false]
		);
		Ok(())
	}
}
