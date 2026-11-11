//! Port of `fontTools.varLib.interpolatableTestStartingPoint`.

use std::f64::consts::PI;

use fontoxide_misc::complex::Complex;
use fontoxide_pens::Transform;

use super::helpers::{ContourVector, Isomorphism, vdiff_hypot2_complex};
use super::{Glyph, InterpolatableError, Result};

/// `min(enumerate(costs), key=lambda x: x[1])`: the first smallest cost.
fn min_cost(costs: &[f64]) -> Option<(usize, f64)> {
	let mut best: Option<(usize, f64)> = None;
	for (i, &c) in costs.iter().enumerate() {
		match best {
			// NaNs never replace the minimum, as in Python.
			Some((_, b)) if c.partial_cmp(&b) != Some(std::cmp::Ordering::Less) => {}
			_ => best = Some((i, c)),
		}
	}
	best
}

fn type_error(message: &str) -> InterpolatableError {
	InterpolatableError::Type(format!("TypeError: {message}"))
}

/// The transformation of a contour vector by its covariance matrix: rotated
/// by the angle of the major eigenvector, and scaled by the square roots of
/// the eigenvalues.
fn covariance_transform(vector: &ContourVector) -> Transform {
	// meanX = vector[1]
	// meanY = vector[2]
	let stddev_x = vector[3] * 0.5;
	let stddev_y = vector[4] * 0.5;
	let mut correlation = vector[5];
	if correlation != 0.0 {
		correlation /= vector[0].abs();
	}

	// https://cookierobotics.com/007/
	let a = stddev_x * stddev_x; // VarianceX
	let c = stddev_y * stddev_y; // VarianceY
	let b = correlation * stddev_x * stddev_y; // Covariance

	let delta = (((a - c) * 0.5).powf(2.0) + b * b).powf(0.5);
	let lambda1 = (a + c) * 0.5 + delta; // Major eigenvalue
	let mut lambda2 = (a + c) * 0.5 - delta; // Minor eigenvalue
	let theta = if b != 0.0 {
		(lambda1 - a).atan2(b)
	} else if a < c {
		PI * 0.5
	} else {
		0.0
	};
	// Don't translate here. We are working on the complex-vector that includes
	// more than just the points. It's horrible what we are doing anyway...
	let trans = Transform::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0).rotate(theta);
	if lambda2 < 0.0 {
		lambda2 = 0.0;
	}
	trans.scale(lambda1.sqrt(), Some(lambda2.sqrt()))
}

/// The vector of an isomorphism transformed.
fn transform_vector(trans: &Transform, vector: &[Complex]) -> Vec<Complex> {
	vector
		.iter()
		.map(|pt| Complex::from_point(trans.transform_point((pt.re, pt.im))))
		.collect()
}

/// `test_starting_point(glyph0, glyph1, ix, tolerance, matching)`: the
/// tolerance of the best starting point of contour `ix` of the second glyph
/// (contour `matching[ix]` before reordering), the point and whether the
/// contour is reversed.
///
/// The contours must have isomorphisms and vectors.
pub fn test_starting_point(
	glyph0: &Glyph,
	glyph1: &Glyph,
	ix: usize,
	tolerance: f64,
	matching: Option<&[usize]>,
) -> Result<(f64, usize, bool)> {
	let identity: Vec<usize>;
	let matching = match matching {
		Some(m) => m,
		None => {
			identity = (0..glyph0.isomorphisms.len()).collect();
			&identity
		}
	};
	let index_error = || InterpolatableError::Type("IndexError: list index out of range".into());
	let not_subscriptable = || type_error("'NoneType' object is not subscriptable");
	let m1ix = *matching.get(ix).ok_or_else(index_error)?;
	let contour0: &[Isomorphism] = glyph0
		.isomorphisms
		.get(ix)
		.ok_or_else(index_error)?
		.as_deref()
		.ok_or_else(not_subscriptable)?;
	let contour1: &[Isomorphism] = glyph1
		.isomorphisms
		.get(m1ix)
		.ok_or_else(index_error)?
		.as_deref()
		.ok_or_else(not_subscriptable)?;
	let m0_vector = glyph0
		.green_vectors
		.get(ix)
		.ok_or_else(index_error)?
		.ok_or_else(not_subscriptable)?;
	let m1_vector = glyph1
		.green_vectors
		.get(m1ix)
		.ok_or_else(index_error)?
		.ok_or_else(not_subscriptable)?;

	let c0 = contour0.first().ok_or_else(index_error)?;
	// Next few lines duplicated below.
	let costs: Vec<f64> = contour1
		.iter()
		.map(|c1| vdiff_hypot2_complex(&c0.vector, &c1.vector))
		.collect();
	let (min_cost_idx, mut min_cost_value) = min_cost(&costs)
		.ok_or_else(|| InterpolatableError::Value("min() iterable argument is empty".into()))?;
	let mut first_cost = costs[0];
	let proposed_point = contour1[min_cost_idx].point;
	let reverse = contour1[min_cost_idx].reverse;

	if min_cost_value < first_cost * tolerance {
		// c0 is the first isomorphism of the m0 master; contour1 is the list of
		// all isomorphisms of the m1 master.
		//
		// If the two shapes are both circle-ish and slightly rotated, we detect
		// wrong start point. This is for example the case hundreds of times in
		// RobotoSerif-Italic[GRAD,opsz,wdth,wght].ttf
		//
		// If the proposed point is only one off from the first point (and not
		// reversed), try harder:
		//
		// Find the major eigenvector of the covariance matrix, and rotate the
		// contours by that angle. Then find the closest point again. If it
		// matches this time, let it pass.

		// UPSTREAM-BUG(B43): the number of points is that of contour `ix` of the
		// second glyph before reordering, not that of the contour it is
		// compared with (`matching[ix]`).
		// LENIENT(L40): if that contour is a component or open, fontTools fails
		// with a `TypeError`; the matched contour is used instead.
		let points = match glyph1.points.get(ix) {
			Some(Some(points)) => points,
			_ => glyph1
				.points
				.get(m1ix)
				.and_then(Option::as_ref)
				.ok_or_else(|| type_error("object of type 'NoneType' has no len()"))?,
		};
		let num_points = points.len() as i64;
		let leeway = 3;
		let proposed = proposed_point as i64;
		if !reverse && (proposed <= leeway || proposed >= num_points - leeway) {
			// Try harder

			// Recover the covariance matrix from the GreenVectors. This is a
			// 2x2 matrix.
			let trans = covariance_transform(&m0_vector);
			let new_c0 = transform_vector(&trans, &c0.vector);
			let trans = covariance_transform(&m1_vector);
			let new_contour1: Vec<Vec<Complex>> = contour1
				.iter()
				.map(|c1| transform_vector(&trans, &c1.vector))
				.collect();

			// Next few lines duplicate from above.
			let costs: Vec<f64> = new_contour1
				.iter()
				.map(|new_c1| vdiff_hypot2_complex(&new_c0, new_c1))
				.collect();
			if let Some((_, m)) = min_cost(&costs) {
				min_cost_value = m;
			}
			first_cost = costs[0];
			// fontTools does not report the problem in that case, but keeps the
			// proposed point:
			// if min_cost < first_cost * tolerance:
			//     min_cost = first_cost
			//     reverse = False
			//     proposed_point = 0  # new_contour1[min_cost_idx][1]
		}
	}

	let this_tolerance = if first_cost != 0.0 {
		min_cost_value / first_cost
	} else {
		1.0
	};
	log::debug!(
		"test-starting-point: tolerance {}",
		fontoxide_misc::py::format_g(this_tolerance, 6)
	);
	Ok((this_tolerance, proposed_point, reverse))
}
