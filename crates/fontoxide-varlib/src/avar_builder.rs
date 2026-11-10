//! Building the `avar` table from axis mappings (`fontTools.varLib._add_avar`).

use fontoxide_misc::Tag;
use fontoxide_ttlib::tables::avar::Avar;
use fontoxide_ttlib::{Error, Font, Result};

use crate::models::{normalize_value, normalize_value_decreasing, piecewise_linear_map};

/// The parts of a designspace axis (`designspaceLib.AxisDescriptor`) that
/// building `avar` uses.
#[derive(Debug, Clone, PartialEq)]
pub struct AvarAxis {
	/// The axis tag (`tag`).
	pub tag: Tag,
	/// The axis name, for messages (`name`).
	pub name: String,
	/// The minimum user value (`minimum`).
	pub minimum: f64,
	/// The default user value (`default`).
	pub default: f64,
	/// The maximum user value (`maximum`).
	pub maximum: f64,
	/// The mapping from user to design values (`map`).
	pub map: Vec<(f64, f64)>,
}

impl AvarAxis {
	/// The mapping without repeated pairs (`get_validated_map`); fails if an
	/// input has different outputs.
	pub fn get_validated_map(&self) -> Result<Vec<(f64, f64)>> {
		let mut validated: Vec<(f64, f64)> = Vec::new();
		for &(input, output) in &self.map {
			match validated.iter().find(|(i, _)| *i == input) {
				Some(&(_, previous)) => {
					if previous != output {
						return Err(Error::value(format!(
							"Axis '{}': mapping input value {} has conflicting outputs {} and {}.",
							self.name,
							py_float(input),
							py_float(previous),
							py_float(output)
						)));
					}
				}
				None => validated.push((input, output)),
			}
		}
		Ok(validated)
	}

	/// A user value mapped to a design value (`map_forward`).
	pub fn map_forward(&self, v: f64) -> Result<f64> {
		let map = self.get_validated_map()?;
		Ok(piecewise_linear_map(v, &map))
	}
}

fn py_float(v: f64) -> String {
	fontoxide_misc::py::float_repr(v)
}

/// Adds an `avar` table for the axis mappings (`_add_avar`, without
/// `mappings`). Returns whether the table was added: if no axis maps any
/// value, there is no need for one.
///
/// fontTools also builds an `avar` version 2 from designspace axis mappings;
/// that needs designspaceLib, which is not ported yet.
pub fn add_avar(font: &mut Font, axes: &[AvarAxis]) -> Result<bool> {
	if axes.is_empty() {
		return Err(Error::Assertion("axes".into()));
	}
	log::info!("Generating avar");
	let mut avar = Avar::default();
	let mut interesting = false;
	for axis in axes {
		let axis_map = axis.get_validated_map()?;
		// Some rasterizers require the default maps (-1 to -1, 0 to 0 and 1 to 1)
		// in every segment map, even when the axis does not change them.
		let mut curve: Vec<(f64, f64)> = vec![(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)];
		let keys_triple = (axis.minimum, axis.default, axis.maximum);
		let vals_triple = (
			axis.map_forward(axis.minimum)?,
			axis.map_forward(axis.default)?,
			axis.map_forward(axis.maximum)?,
		);
		if axis_map.is_empty() {
			avar.segments.insert(axis.tag, curve);
			continue;
		}
		let mut items = axis_map.clone();
		items.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
		let keys: Vec<f64> = items.iter().map(|i| i.0).collect();
		let vals: Vec<f64> = items.iter().map(|i| i.1).collect();
		let min = keys.iter().copied().fold(f64::INFINITY, f64::min);
		let max = keys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
		if axis.minimum != min {
			return Err(Error::value(format!(
				"Axis '{}': there must be a mapping for the axis minimum value {} and it must be the lowest input mapping value.",
				axis.name,
				py_float(axis.minimum)
			)));
		}
		if axis.maximum != max {
			return Err(Error::value(format!(
				"Axis '{}': there must be a mapping for the axis maximum value {} and it must be the highest input mapping value.",
				axis.name,
				py_float(axis.maximum)
			)));
		}
		if !keys.contains(&axis.default) {
			return Err(Error::value(format!(
				"Axis '{}': there must be a mapping for the axis default value {}.",
				axis.name,
				py_float(axis.default)
			)));
		}
		// The outputs must be monotonic in the direction of the design axis.
		let reverse = vals_triple.0 > vals_triple.2;
		let mut sorted = vals.clone();
		sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
		if reverse {
			sorted.reverse();
		}
		if sorted != vals {
			return Err(Error::value(format!(
				"Axis '{}': mapping output values must be in {} order.",
				axis.name,
				if reverse { "descending" } else { "ascending" }
			)));
		}
		let keys = keys
			.iter()
			.map(|&v| normalize_value(v, keys_triple, false))
			.collect::<Result<Vec<_>>>()?;
		let vals = vals
			.iter()
			.map(|&v| normalize_value_decreasing(v, vals_triple, false))
			.collect::<Result<Vec<_>>>()?;
		if keys.iter().zip(&vals).all(|(k, v)| k == v) {
			avar.segments.insert(axis.tag, curve);
			continue;
		}
		interesting = true;
		for (&k, &v) in keys.iter().zip(&vals) {
			match curve.iter_mut().find(|(f, _)| *f == k) {
				Some(entry) => entry.1 = v,
				None => curve.push((k, v)),
			}
		}
		let get = |k: f64| curve.iter().find(|(f, _)| *f == k).map(|e| e.1);
		if get(0.0) != Some(0.0)
			|| get(-1.0).is_some_and(|v| v != -1.0)
			|| get(1.0).is_some_and(|v| v != 1.0)
		{
			return Err(Error::Assertion(
				"the curve must map -1, 0 and 1 to themselves".into(),
			));
		}
		avar.segments.insert(axis.tag, curve);
	}
	if font.contains(Tag::new(b"avar")) {
		return Err(Error::Assertion("\"avar\" not in font".into()));
	}
	if !interesting {
		log::info!("No need for avar");
		return Ok(false);
	}
	font.insert(Box::new(avar));
	Ok(true)
}
