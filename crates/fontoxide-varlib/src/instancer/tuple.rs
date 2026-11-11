//! The tuple variations of `fontTools.varLib.instancer`, and the methods of
//! `TupleVariation` it uses.

use std::collections::HashMap;

use fontoxide_ttlib::tables::tuple_variation::{Delta, TupleVariation};
use fontoxide_ttlib::{Error, Result};

use super::limits::{NormalizedAxisTriple, NormalizedLimits};
use super::solver::rebase_tent;

/// `scaleDeltas`.
pub(crate) fn scale_deltas(var: &mut TupleVariation, scalar: f64) {
	var.scale_deltas(scalar);
}

/// `roundDeltas`.
pub(crate) fn round_deltas(var: &mut TupleVariation) {
	var.round_deltas();
}

/// `optimize`: leaves out the deltas that can be interpolated, if that makes the
/// variation smaller.
pub(crate) fn optimize(
	var: &mut TupleVariation,
	coords: &[(f64, f64)],
	ends: &[usize],
) -> Result<()> {
	var.optimize(coords, ends, 0.5)
}

/// `optimize(origCoords, endPts, tolerance)`.
pub(crate) fn optimize_with_tolerance(
	var: &mut TupleVariation,
	coords: &[(f64, f64)],
	ends: &[usize],
	tolerance: f64,
) -> Result<()> {
	var.optimize(coords, ends, tolerance)
}

/// `changeTupleVariationAxisLimit`.
pub fn change_axis_limit(
	mut var: TupleVariation,
	tag: &str,
	limit: NormalizedAxisTriple,
) -> Result<Vec<TupleVariation>> {
	// Skip when the axis is missing or peaks at 0 (it doesn't participate).
	let Some(&tent) = var.axes.get(tag) else {
		return Ok(vec![var]);
	};
	let (lower, peak, upper) = tent;
	if peak == 0.0 {
		// Explicitly defined, no-op axes can be omitted.
		var.axes.remove(tag);
		return Ok(vec![var]);
	}
	// Drop if the tent isn't well-formed.
	if !(lower <= peak && peak <= upper) || (lower < 0.0 && upper > 0.0) {
		return Ok(Vec::new());
	}
	let solutions = rebase_tent(tent, limit)?;
	let mut out = Vec::with_capacity(solutions.len());
	for (scalar, tent) in solutions {
		let mut new_var = var.clone();
		match tent {
			None => {
				new_var.axes.remove(tag);
			}
			Some(tent) => {
				if tent.1 == 0.0 {
					return Err(Error::Assertion(format!("{tent:?}")));
				}
				new_var.axes.insert(tag.to_owned(), tent);
			}
		}
		scale_deltas(&mut new_var, scalar);
		out.push(new_var);
	}
	Ok(out)
}

/// The key of a region: as in Python, `-0.0` and `0.0` are the same.
pub(crate) fn region_key(var: &TupleVariation) -> Vec<(String, [u64; 3])> {
	var.axes
		.iter()
		.map(|(tag, t)| {
			let bits = |v: f64| (v + 0.0).to_bits();
			(tag.clone(), [bits(t.0), bits(t.1), bits(t.2)])
		})
		.collect()
}

/// `instantiateTupleVariationStore`: changes the variations for the limits, and
/// returns the overall adjustment of the deltas that were fully instantiated.
/// `outline` has the coordinates and contour ends of a `gvar` glyph.
pub(crate) fn instantiate_tuple_variation_store(
	variations: &mut Vec<TupleVariation>,
	limits: &NormalizedLimits,
	outline: Option<(&[(f64, f64)], &[usize])>,
	round_all: bool,
) -> Result<Vec<Option<Delta>>> {
	// `changeTupleVariationsAxisLimits`, in the sorted order of the axes.
	let mut current = std::mem::take(variations);
	for (tag, &limit) in &limits.0 {
		let mut next = Vec::with_capacity(current.len());
		for var in current {
			next.extend(change_axis_limit(var, tag, limit)?);
		}
		current = next;
	}
	// Merge variations with overlapping tents, in the order they appear.
	let mut merged: Vec<TupleVariation> = Vec::with_capacity(current.len());
	let mut index: HashMap<Vec<(String, [u64; 3])>, usize> = HashMap::new();
	for mut var in current {
		// Inferred deltas are only computed for gvar.
		if let Some((coords, ends)) = outline {
			var.calc_inferred_deltas(coords, ends)?;
		}
		let key = region_key(&var);
		match index.get(&key) {
			Some(&i) => merged[i].add_deltas(&var)?,
			None => {
				index.insert(key, merged.len());
				merged.push(var);
			}
		}
	}
	// A variation with all axes pinned goes to the default instance.
	let default = merged
		.iter()
		.position(|v| v.axes.is_empty())
		.map(|i| merged.remove(i));
	if round_all {
		merged.iter_mut().for_each(round_deltas);
	}
	*variations = merged;
	Ok(default.map_or(Vec::new(), |v| v.coordinates))
}
