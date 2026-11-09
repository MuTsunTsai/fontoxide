//! The tuple variations of `fontTools.varLib.instancer`, and the methods of
//! `TupleVariation` it uses.

use std::collections::HashMap;

use fontoxide_misc::round_tools::ot_round;
use fontoxide_ttlib::tables::glyf::Number;
use fontoxide_ttlib::tables::tuple_variation::{Delta, TupleVariation, compile_points};
use fontoxide_ttlib::var::iup_delta;
use fontoxide_ttlib::{Error, Result};

use super::limits::{NormalizedAxisTriple, NormalizedLimits};
use super::solver::rebase_tent;
use crate::iup::iup_delta_optimize;

fn scale(n: Number, scalar: f64) -> Number {
	Number::Float(n.value() * scalar)
}

fn round(n: Number) -> Number {
	Number::Int(ot_round(n.value()))
}

/// `getCoordWidth`: 2 for (x, y) deltas, 1 for single values, or 0 if there are
/// none.
fn coord_width(var: &TupleVariation) -> u8 {
	match var.coordinates.iter().flatten().next() {
		Some(Delta::Point(..)) => 2,
		Some(Delta::Value(_)) => 1,
		None => 0,
	}
}

/// `scaleDeltas`.
pub(crate) fn scale_deltas(var: &mut TupleVariation, scalar: f64) {
	if scalar == 1.0 {
		return;
	}
	for delta in var.coordinates.iter_mut().flatten() {
		*delta = match *delta {
			Delta::Point(x, y) => Delta::Point(scale(x, scalar), scale(y, scalar)),
			Delta::Value(v) => Delta::Value(scale(v, scalar)),
		};
	}
}

/// `roundDeltas`.
pub(crate) fn round_deltas(var: &mut TupleVariation) {
	for delta in var.coordinates.iter_mut().flatten() {
		*delta = match *delta {
			Delta::Point(x, y) => Delta::Point(round(x), round(y)),
			Delta::Value(v) => Delta::Value(round(v)),
		};
	}
}

/// `calcInferredDeltas`.
fn calc_inferred_deltas(
	var: &mut TupleVariation,
	coords: &[(f64, f64)],
	ends: &[usize],
) -> Result<()> {
	if coord_width(var) == 1 {
		return Err(Error::ttlib(
			"TypeError: Only 'gvar' TupleVariation can have inferred deltas",
		));
	}
	if var.coordinates.iter().all(Option::is_some) {
		return Ok(());
	}
	if var.coordinates.len() != coords.len() {
		return Err(Error::value(format!(
			"Expected len(origCoords) == {}; found {}",
			var.coordinates.len(),
			coords.len()
		)));
	}
	let deltas: Vec<Option<(f64, f64)>> = var
		.coordinates
		.iter()
		.map(|d| match d {
			Some(Delta::Point(x, y)) => Some((x.value(), y.value())),
			_ => None,
		})
		.collect();
	var.coordinates = iup_delta(&deltas, coords, ends)?
		.into_iter()
		.map(|(x, y)| Some(Delta::Point(Number::Float(x), Number::Float(y))))
		.collect();
	Ok(())
}

/// The size of a compiled variation, in a store of its own (`compile`).
fn compiled_size(var: &TupleVariation, axis_tags: &[String]) -> Result<usize> {
	let Some(points) = var.used_points() else {
		return Ok(0);
	};
	let (tuple, aux) = var.compile(axis_tags, &HashMap::new(), &compile_points(&points), true)?;
	Ok(tuple.len() + aux.len())
}

/// `optimize`: leaves out the deltas that can be interpolated, if that makes the
/// variation smaller.
pub(crate) fn optimize(
	var: &mut TupleVariation,
	coords: &[(f64, f64)],
	ends: &[usize],
) -> Result<()> {
	const TOLERANCE: f64 = 0.5;
	let mut deltas = Vec::with_capacity(var.coordinates.len());
	for delta in &var.coordinates {
		match delta {
			// Already optimized.
			None => return Ok(()),
			Some(Delta::Point(x, y)) => deltas.push((x.value(), y.value())),
			Some(Delta::Value(_)) => {
				return Err(Error::ttlib("TypeError: cannot unpack non-iterable int"));
			}
		}
	}
	let keep = iup_delta_optimize(&deltas, coords, ends, TOLERANCE)?;
	if keep.iter().all(|k| *k) {
		return Ok(());
	}
	let optimized = TupleVariation {
		axes: var.axes.clone(),
		coordinates: var
			.coordinates
			.iter()
			.zip(&keep)
			.map(|(d, k)| d.filter(|_| *k))
			.collect(),
	};
	// Shouldn't matter that this is different from fvar.
	let axis_tags: Vec<String> = var.axes.keys().cloned().collect();
	if compiled_size(&optimized, &axis_tags)? < compiled_size(var, &axis_tags)? {
		var.coordinates = optimized.coordinates;
	}
	Ok(())
}

/// `TupleVariation.__iadd__`.
fn add_deltas(a: &mut TupleVariation, b: &TupleVariation) -> Result<()> {
	if a.coordinates.len() != b.coordinates.len() {
		return Err(Error::value(
			"cannot sum TupleVariation deltas with different lengths",
		));
	}
	let add = |x: Number, y: Number| match (x, y) {
		(Number::Int(x), Number::Int(y)) => Number::Int(x + y),
		_ => Number::Float(x.value() + y.value()),
	};
	if coord_width(a) == 2 {
		for (d1, d2) in a.coordinates.iter_mut().zip(&b.coordinates) {
			match (*d1, d2) {
				(Some(Delta::Point(x1, y1)), Some(Delta::Point(x2, y2))) => {
					*d1 = Some(Delta::Point(add(x1, *x2), add(y1, *y2)));
				}
				_ => return Err(Error::value("cannot sum gvar deltas with inferred points")),
			}
		}
	} else {
		// `None` in `cvar` means no adjustment.
		for (d1, d2) in a.coordinates.iter_mut().zip(&b.coordinates) {
			match (*d1, d2) {
				(Some(Delta::Value(x)), Some(Delta::Value(y))) => {
					*d1 = Some(Delta::Value(add(x, *y)));
				}
				(None, Some(d)) => *d1 = Some(*d),
				_ => {}
			}
		}
	}
	Ok(())
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
			calc_inferred_deltas(&mut var, coords, ends)?;
		}
		let key = region_key(&var);
		match index.get(&key) {
			Some(&i) => add_deltas(&mut merged[i], &var)?,
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
