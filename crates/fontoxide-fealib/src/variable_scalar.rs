//! Values that vary across the designspace (`fontTools.feaLib.variableScalar`).

use std::collections::HashMap;
use std::fmt;

use fontoxide_designspacelib::{Axis as DsAxis, DesignSpaceDocument};
use fontoxide_misc::py::{float_repr, str_repr};
use fontoxide_ttlib::Font;
use fontoxide_ttlib::tables::{Avar, Fvar};
use fontoxide_varlib::models::{
	Support, VariationModel, normalize_value, normalize_value_decreasing, piecewise_linear_map,
};
use fontoxide_varlib::var_store::OnlineVarStoreBuilder;
use indexmap::IndexMap;

use crate::error::{Error, Result};

/// A hashable user-space location: `(axis tag, coordinate)` pairs sorted by axis
/// tag (`LocationTuple`).
pub type LocationTuple = Vec<(String, f64)>;

/// `Location(mapping)`: a [`LocationTuple`] from `(axis tag, coordinate)` pairs,
/// sorted like `tuple(sorted(location.items()))`.
pub fn location<S: Into<String>>(location: impl IntoIterator<Item = (S, f64)>) -> LocationTuple {
	let mut result: LocationTuple = Vec::new();
	for (axis, value) in location {
		let axis = axis.into();
		// A dict keeps one value per key; the last one wins.
		match result.iter_mut().find(|(a, _)| *a == axis) {
			Some(entry) => entry.1 = value,
			None => result.push((axis, value)),
		}
	}
	result.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
	result
}

/// A scalar with different values at different points in the designspace
/// (`VariableScalar`).
///
/// `values` keeps insertion order like the Python dict; adding a value at a
/// location that is already present replaces it in place.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VariableScalar {
	/// The values across various user locations (`values`).
	pub values: Vec<(LocationTuple, i64)>,
}

impl VariableScalar {
	/// `VariableScalar()`.
	pub fn new() -> Self {
		Self::default()
	}

	/// `VariableScalar(location_value)`.
	pub fn from_values<S: Into<String>, L: IntoIterator<Item = (S, f64)>>(
		location_value: impl IntoIterator<Item = (L, i64)>,
	) -> Self {
		let mut scalar = Self::new();
		for (loc, value) in location_value {
			scalar.add_value(loc, value);
		}
		scalar
	}

	/// Whether the values are not all the same (`does_vary`).
	pub fn does_vary(&self) -> bool {
		match self.values.split_first() {
			Some(((_, first), rest)) => rest.iter().any(|(_, v)| v != first),
			None => false,
		}
	}

	/// `add_value(location, value)`.
	pub fn add_value<S: Into<String>>(
		&mut self,
		loc: impl IntoIterator<Item = (S, f64)>,
		value: i64,
	) {
		let key = location(loc);
		match self.values.iter_mut().find(|(k, _)| *k == key) {
			Some(entry) => entry.1 = value,
			None => self.values.push((key, value)),
		}
	}
}

impl fmt::Display for VariableScalar {
	/// `VariableScalar.__repr__`, which is also what `asFea()` writes, e.g.
	/// `(20 wght=400:1 wght=500.5:4)`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let items: Vec<String> = self
			.values
			.iter()
			.map(|(loc, value)| {
				if loc.is_empty() {
					value.to_string()
				} else {
					let loc: Vec<String> = loc
						.iter()
						.map(|(ax, coord)| format!("{ax}={}", coord_str(*coord)))
						.collect();
					format!("{}:{value}", loc.join(","))
				}
			})
			.collect();
		write!(f, "({})", items.join(" "))
	}
}

/// `int(coord) if float(coord).is_integer() else coord`, formatted.
fn coord_str(coord: f64) -> String {
	if coord.is_finite() && coord.fract() == 0.0 && coord.abs() < 1e18 {
		// `int(-0.0)` is `0`.
		format!("{}", coord as i64)
	} else {
		float_repr(coord)
	}
}

impl VariableScalar {
	/// `add_to_variation_store(store_builder, model_cache, avar)`, the deprecated
	/// shim of [`VariableScalarBuilder::add_to_variation_store`]. Python reads
	/// the axes from the `axes` attribute; here they are passed as
	/// `(tag, minimum, default, maximum)`, with the `avar` table if any.
	pub fn add_to_variation_store(
		&self,
		axes: &[(String, f64, f64, f64)],
		store_builder: &mut OnlineVarStoreBuilder,
		avar: Option<&Avar>,
	) -> Result<(i64, u64)> {
		log::warn!(
			"VariableScalar.add_to_variation_store() is deprecated. Use VariableScalarBuilder.add_to_variation_store() instead."
		);
		if axes.is_empty() {
			return Err(Error::Value(
				".axes must be defined on variable scalar before calling add_to_variation_store()"
					.to_owned(),
			));
		}
		let mut builder = VariableScalarBuilder {
			axis_triples: axes
				.iter()
				.map(|(tag, min, default, max)| (tag.clone(), (*min, *default, *max)))
				.collect(),
			axis_mappings: avar.map(avar_mappings).unwrap_or_default(),
			model_cache: HashMap::new(),
		};
		builder.add_to_variation_store(self, store_builder)
	}
}

/// A [`LocationTuple`] as a hash key: floats by their bits, with `-0.0` as
/// `0.0` (Python compares them equal).
type LocationKey = Vec<(String, u64)>;

fn location_key(location: &LocationTuple) -> LocationKey {
	location
		.iter()
		.map(|(tag, v)| (tag.clone(), (v + 0.0).to_bits()))
		.collect()
}

/// `repr()` of a location tuple, for error messages.
fn location_repr(location: &[(String, f64)]) -> String {
	let items: Vec<String> = location
		.iter()
		.map(|(tag, v)| format!("({}, {})", str_repr(tag), float_repr(*v)))
		.collect();
	if items.len() == 1 {
		format!("({},)", items[0])
	} else {
		format!("({})", items.join(", "))
	}
}

/// The `avar` segment maps by axis tag.
fn avar_mappings(avar: &Avar) -> IndexMap<String, Vec<(f64, f64)>> {
	avar.segments
		.iter()
		.map(|(tag, segments)| (tag.to_string(), segments.clone()))
		.collect()
}

/// A helper for building variable scalars, or otherwise interrogating their
/// variation model for interpolation or similar (`VariableScalarBuilder`).
#[derive(Debug, Clone, Default)]
pub struct VariableScalarBuilder {
	/// Minimum, default, and maximum for each axis in user coordinates
	/// (`axis_triples`).
	pub axis_triples: IndexMap<String, (f64, f64, f64)>,
	/// Optional mappings from normalized user coordinates to normalized design
	/// coordinates (`axis_mappings`).
	pub axis_mappings: IndexMap<String, Vec<(f64, f64)>>,
	/// The models by the fully specified user locations of the scalars
	/// (`model_cache`): the same locations are used for many scalars.
	pub model_cache: HashMap<Vec<Vec<(String, u64)>>, VariationModel>,
}

impl VariableScalarBuilder {
	/// `VariableScalarBuilder.from_ttf(ttf)`: the axes of `fvar`, and the
	/// segment maps of `avar` if there is one.
	pub fn from_ttf(font: &mut Font) -> Result<Self> {
		let axis_triples = match font.get::<Fvar>()? {
			Some(fvar) => fvar
				.axes
				.iter()
				.map(|a| {
					(
						a.tag().to_string(),
						(a.min_value, a.default_value, a.max_value),
					)
				})
				.collect(),
			None => return Err(Error::Value("KeyError: 'fvar'".to_owned())),
		};
		let axis_mappings = font
			.get::<Avar>()?
			.map(|a| avar_mappings(a))
			.unwrap_or_default();
		Ok(VariableScalarBuilder {
			axis_triples,
			axis_mappings,
			model_cache: HashMap::new(),
		})
	}

	/// `VariableScalarBuilder.from_designspace(doc)`: the axes of the document,
	/// with their maps normalized.
	pub fn from_designspace(doc: &DesignSpaceDocument) -> Result<Self> {
		let mut axis_triples = IndexMap::new();
		let mut axis_mappings = IndexMap::new();
		for axis in &doc.axes {
			let DsAxis::Continuous(axis) = axis else {
				return Err(Error::Value(
					"AttributeError: 'DiscreteAxisDescriptor' object has no attribute 'minimum'"
						.to_owned(),
				));
			};
			let tag = axis.tag.clone().unwrap_or_else(|| "None".to_owned());
			let user = (
				axis.minimum.to_f64(),
				axis.default.to_f64(),
				axis.maximum.to_f64(),
			);
			axis_triples.insert(tag.clone(), user);
			if axis.map.is_empty() {
				continue;
			}
			let forward = |v| -> Result<f64> {
				Ok(axis
					.map_forward(v)
					.map_err(|e| Error::Value(e.to_string()))?
					.to_f64())
			};
			let design = (
				forward(axis.minimum)?,
				forward(axis.default)?,
				forward(axis.maximum)?,
			);
			let mut mapping: Vec<(f64, f64)> = Vec::new();
			for (u, d) in &axis.map {
				let key = normalize_value(u.to_f64(), user, false)?;
				let value = normalize_value_decreasing(d.to_f64(), design, false)?;
				// A dict: the last value of a key wins.
				match mapping.iter_mut().find(|(k, _)| *k == key) {
					Some(entry) => entry.1 = value,
					None => mapping.push((key, value)),
				}
			}
			axis_mappings.insert(tag, mapping);
		}
		Ok(VariableScalarBuilder {
			axis_triples,
			axis_mappings,
			model_cache: HashMap::new(),
		})
	}

	/// Validates a user-space location and fills in the missing axes with their
	/// defaults (`_fully_specify_location`).
	fn fully_specify_location(&self, location: &[(String, f64)]) -> Result<LocationTuple> {
		let mut full: Vec<(String, f64)> = Vec::new();
		for (axtag, value) in location {
			if !self.axis_triples.contains_key(axtag) {
				return Err(Error::Value(format!(
					"Unknown axis {axtag} in {}",
					location_repr(location)
				)));
			}
			match full.iter_mut().find(|(t, _)| t == axtag) {
				Some(entry) => entry.1 = *value,
				None => full.push((axtag.clone(), *value)),
			}
		}
		for (axtag, (_, axis_default, _)) in &self.axis_triples {
			if !full.iter().any(|(t, _)| t == axtag) {
				full.push((axtag.clone(), *axis_default));
			}
		}
		Ok(self::location(full))
	}

	/// Normalizes a user-space location, applying the `avar` mappings if present
	/// (`_normalize_location`). Like fontTools, this only handles `avar` version
	/// 1 mappings.
	fn normalize_location(&self, location: &[(String, f64)]) -> Result<Vec<(String, f64)>> {
		let mut result = Vec::with_capacity(location.len());
		for (axtag, value) in location {
			let triple = *self
				.axis_triples
				.get(axtag)
				.ok_or_else(|| Error::Value(format!("KeyError: {}", str_repr(axtag))))?;
			let mut normalized = normalize_value(*value, triple, false)?;
			if let Some(mapping) = self.axis_mappings.get(axtag) {
				normalized = piecewise_linear_map(normalized, mapping);
			}
			result.push((axtag.clone(), normalized));
		}
		Ok(result)
	}

	/// The fully specified user-space locations and the values of a scalar, in
	/// order (`_full_locations_and_values`).
	fn full_locations_and_values(
		&self,
		scalar: &VariableScalar,
	) -> Result<Vec<(LocationTuple, i64)>> {
		scalar
			.values
			.iter()
			.map(|(loc, val)| Ok((self.fully_specify_location(loc)?, *val)))
			.collect()
	}

	/// The default value of a variable scalar (`default_value`).
	pub fn default_value(&self, scalar: &VariableScalar) -> Result<i64> {
		let default_loc = self::location(
			self.axis_triples
				.iter()
				.map(|(tag, (_, default, _))| (tag.clone(), *default)),
		);
		for (location, value) in self.full_locations_and_values(scalar)? {
			if location == default_loc {
				return Ok(value);
			}
		}
		Err(Error::Value("Default value could not be found".to_owned()))
	}

	/// Interpolates the value of a scalar at a user-space location
	/// (`value_at_location`).
	pub fn value_at_location(
		&mut self,
		scalar: &VariableScalar,
		location: &[(String, f64)],
	) -> Result<f64> {
		let location = self.fully_specify_location(location)?;
		let pairs = self.full_locations_and_values(scalar)?;
		// If the user location matches exactly, no axis mapping or variation
		// model is needed.
		for (loc, val) in &pairs {
			if *loc == location {
				return Ok(*val as f64);
			}
		}
		let values: Vec<f64> = pairs.iter().map(|(_, v)| *v as f64).collect();
		let normalized_location = self.normalize_location(&location)?;
		let value = self
			.model(scalar)?
			.interpolate_from_masters(&normalized_location, &values)?;
		value.ok_or_else(|| Error::Value("Insufficient number of values to interpolate".to_owned()))
	}

	/// The variation model of a scalar's locations (`model`); scalars with the
	/// same fully specified user locations share a cached model.
	pub fn model(&mut self, scalar: &VariableScalar) -> Result<&VariationModel> {
		let pairs = self.full_locations_and_values(scalar)?;
		let cache_key: Vec<LocationKey> = pairs.iter().map(|(loc, _)| location_key(loc)).collect();
		if !self.model_cache.contains_key(&cache_key) {
			let normalized_locations = pairs
				.iter()
				.map(|(loc, _)| self.normalize_location(loc))
				.collect::<Result<Vec<_>>>()?;
			let axis_order: Vec<String> = self.axis_triples.keys().cloned().collect();
			let model = VariationModel::new(&normalized_locations, Some(&axis_order), false, None)?;
			self.model_cache.insert(cache_key.clone(), model);
		}
		self.model_cache
			.get(&cache_key)
			.ok_or_else(|| Error::Value("KeyError: model".to_owned()))
	}

	/// The deltas and supports of a scalar's variation model, with the deltas
	/// rounded (`get_deltas_and_supports`).
	pub fn get_deltas_and_supports(
		&mut self,
		scalar: &VariableScalar,
	) -> Result<(Vec<f64>, Vec<Support>)> {
		let values: Vec<Option<f64>> = scalar.values.iter().map(|(_, v)| Some(*v as f64)).collect();
		Ok(self
			.model(scalar)?
			.get_deltas_and_supports_rounded(&values, f64::round_ties_even)?)
	}

	/// Adds a scalar's deltas to a store, returning the default value and the
	/// variation index (`add_to_variation_store`).
	pub fn add_to_variation_store(
		&mut self,
		scalar: &VariableScalar,
		store_builder: &mut OnlineVarStoreBuilder,
	) -> Result<(i64, u64)> {
		let (deltas, supports) = self.get_deltas_and_supports(scalar)?;
		store_builder.set_supports(&supports);
		let index = store_builder.store_deltas_rounded(&deltas, |v| v)?;
		// The default value is an exact integer by construction.
		Ok((self.default_value(scalar)?, index))
	}
}
