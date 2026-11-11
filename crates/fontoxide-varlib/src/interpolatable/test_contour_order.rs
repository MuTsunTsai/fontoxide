//! Port of `fontTools.varLib.interpolatableTestContourOrder`.

use super::helpers::{ContourVector, matching_for_vectors};
use super::{Backend, Glyph, Result};

/// The contour vectors, if all the contours have them.
fn all_vectors(vectors: &[Option<ContourVector>]) -> Option<Vec<ContourVector>> {
	vectors.iter().copied().collect()
}

/// `test_contour_order(glyph0, glyph1)`: the tolerance of the best matching of
/// the contours of the second glyph to those of the first, and the matching,
/// unless the contours are in the same order.
///
/// Both the vectors of the control polygons (`StatisticsControlPen`) and of
/// the outlines (`StatisticsPen`) are tried. If either finds an identity
/// matching, it is accepted. This is crucial for fonts like Kablammo[MORF].ttf
/// and Nabla[EDPT,EHLT].ttf, since they really confuse the `StatisticsPen`
/// vector because of their area=0 contours.
///
/// LENIENT(L40): fontTools fails with a `TypeError` when a glyph of several
/// contours has an open contour, which has no vector; such glyphs are not
/// checked here.
pub fn test_contour_order(
	glyph0: &Glyph,
	glyph1: &Glyph,
	backend: Backend,
) -> Result<(f64, Option<Vec<usize>>)> {
	let n = glyph0.control_vectors.len();
	let mut matching = None;
	let mut matching_cost = 0.0;
	let mut identity_cost = 0.0;
	let mut done = n <= 1;
	let vectors = (
		all_vectors(&glyph0.control_vectors),
		all_vectors(&glyph1.control_vectors),
		all_vectors(&glyph0.green_vectors),
		all_vectors(&glyph1.green_vectors),
	);
	let (Some(m0_control), Some(m1_control), Some(m0_green), Some(m1_green)) = vectors else {
		return Ok((1.0, None));
	};

	let mut control = None;
	if !done {
		let (matching_control, matching_cost_control, identity_cost_control) =
			matching_for_vectors(&m0_control, &m1_control, backend)?;
		done = matching_cost_control == identity_cost_control;
		control = Some((
			matching_control,
			matching_cost_control,
			identity_cost_control,
		));
	}
	let mut green = None;
	if !done {
		let (matching_green, matching_cost_green, identity_cost_green) =
			matching_for_vectors(&m0_green, &m1_green, backend)?;
		done = matching_cost_green == identity_cost_green;
		green = Some((matching_green, matching_cost_green, identity_cost_green));
	}

	if !done && let (Some(control), Some(green)) = (control, green) {
		// See if reversing contours in one master helps. That's a common
		// problem. Then the wrong_start_point test will fix them.
		//
		// Reverse the sign of the area (0); the rest stay the same.
		let reversed = |vectors: &[ContourVector]| -> Vec<ContourVector> {
			vectors
				.iter()
				.map(|m| {
					let mut m = *m;
					m[0] = -m[0];
					m
				})
				.collect()
		};
		let (_, matching_cost_control_reversed, identity_cost_control_reversed) =
			matching_for_vectors(&m0_control, &reversed(&m1_control), backend)?;
		done = matching_cost_control_reversed == identity_cost_control_reversed;
		if !done {
			let (_, matching_cost_green_reversed, identity_cost_green_reversed) =
				matching_for_vectors(&m0_green, &reversed(&m1_green), backend)?;
			done = matching_cost_green_reversed == identity_cost_green_reversed;
		}

		if !done {
			// Otherwise, use the best of the two matchings.
			let (matching_control, matching_cost_control, identity_cost_control) = control;
			let (matching_green, matching_cost_green, identity_cost_green) = green;
			if matching_cost_control * identity_cost_green
				< matching_cost_green * identity_cost_control
			{
				matching = Some(matching_control);
				matching_cost = matching_cost_control;
				identity_cost = identity_cost_control;
			} else {
				matching = Some(matching_green);
				matching_cost = matching_cost_green;
				identity_cost = identity_cost_green;
			}
		}
	}

	let this_tolerance = if identity_cost != 0.0 {
		matching_cost / identity_cost
	} else {
		1.0
	};
	log::debug!(
		"test-contour-order: tolerance {}",
		fontoxide_misc::py::format_g(this_tolerance, 6)
	);
	Ok((this_tolerance, matching))
}
