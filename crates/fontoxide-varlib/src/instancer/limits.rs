//! The axis limits of `fontTools.varLib.instancer`.

use std::collections::BTreeMap;

use fontoxide_misc::fixed_tools::{fixed_to_float, float_to_fixed, float_to_fixed_to_float};
use fontoxide_misc::round_tools::ot_round;
use fontoxide_ttlib::tables::{Avar, Fvar};
use fontoxide_ttlib::var::{Location, VarStoreInstancer};
use fontoxide_ttlib::{Error, Font, Result, Tag};

use crate::models::{normalize_value, piecewise_linear_map};

/// A triple of (min, default, max) axis values (`AxisTriple`). Any can be missing,
/// to be filled in from the `fvar` table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisTriple {
	/// `minimum`.
	pub minimum: Option<f64>,
	/// `default`.
	pub default: Option<f64>,
	/// `maximum`.
	pub maximum: Option<f64>,
}

impl AxisTriple {
	/// A triple, checked to be in order. The default is the minimum if it is
	/// missing and the minimum and maximum are equal.
	pub fn new(minimum: Option<f64>, default: Option<f64>, maximum: Option<f64>) -> Result<Self> {
		let default = if default.is_none() && minimum == maximum {
			minimum
		} else {
			default
		};
		let out_of_order =
			|a: Option<f64>, b: Option<f64>| matches!((a, b), (Some(a), Some(b)) if a > b);
		if out_of_order(minimum, default)
			|| out_of_order(default, maximum)
			|| out_of_order(minimum, maximum)
		{
			return Err(Error::value(format!(
				"AxisTriple minimum ({minimum:?}), default ({default:?}), maximum ({maximum:?}) must be in sorted order"
			)));
		}
		Ok(AxisTriple {
			minimum,
			default,
			maximum,
		})
	}

	/// A pin at a single value.
	pub fn pin(value: f64) -> Self {
		AxisTriple {
			minimum: Some(value),
			default: Some(value),
			maximum: Some(value),
		}
	}

	/// A restriction to a range, keeping the default.
	pub fn range(minimum: f64, maximum: f64) -> Result<Self> {
		Self::new(Some(minimum), None, Some(maximum))
	}

	/// `limitRangeAndPopulateDefaults`: the values, limited to those of `fvar`,
	/// with the missing ones filled in.
	fn limit_range_and_populate_defaults(&self, fvar: (f64, f64, f64)) -> Result<Limit> {
		let minimum = self.minimum.unwrap_or(fvar.0);
		let default = self.default.unwrap_or(fvar.1);
		let maximum = self.maximum.unwrap_or(fvar.2);
		let minimum = minimum.max(fvar.0).min(fvar.2);
		let maximum = maximum.max(fvar.0).min(fvar.2);
		let default = minimum.max(maximum.min(default));
		AxisTriple::new(Some(minimum), Some(default), Some(maximum))?;
		Ok(Limit {
			minimum,
			default,
			maximum,
		})
	}
}

/// What to do with an axis (`axisLimits`, by axis tag): `None` drops it at its
/// default.
pub type AxisLimits = BTreeMap<String, Option<AxisTriple>>;

/// The complete limits of an axis, in user coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limit {
	/// `minimum`.
	pub minimum: f64,
	/// `default`.
	pub default: f64,
	/// `maximum`.
	pub maximum: f64,
}

/// Limits filled in from `fvar`, by axis tag (`AxisLimits` after
/// `limitAxesAndPopulateDefaults`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserLimits(pub BTreeMap<String, Limit>);

/// The axes of `fvar`: tag and (min, default, max).
pub(crate) fn fvar_axes(font: &mut Font) -> Result<Vec<(String, (f64, f64, f64))>> {
	Ok(font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| (a.tag_text(), (a.min_value, a.default_value, a.max_value)))
		.collect())
}

fn tag_of(text: &str) -> Result<Tag> {
	Tag::from_str_padded(text).ok_or_else(|| Error::value(format!("invalid axis tag {text:?}")))
}

impl UserLimits {
	/// `limitAxesAndPopulateDefaults`.
	#[doc(hidden)]
	pub fn from_limits(limits: &AxisLimits, font: &mut Font) -> Result<Self> {
		let axes = fvar_axes(font)?;
		let mut out = BTreeMap::new();
		for (tag, triple) in limits {
			let &(_, fvar) = axes
				.iter()
				.find(|(t, _)| t == tag)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {tag:?}")))?;
			let limit = match triple {
				None => Limit {
					minimum: fvar.1,
					default: fvar.1,
					maximum: fvar.1,
				},
				Some(triple) => triple.limit_range_and_populate_defaults(fvar)?,
			};
			out.insert(tag.clone(), limit);
		}
		Ok(UserLimits(out))
	}

	/// Keeps the axes that change: explicitly retaining an axis's original range
	/// requires no instancing. Compared at the 16.16 precision of `fvar`.
	pub(crate) fn without_unchanged(&self, font: &mut Font) -> Result<Self> {
		let mut out = BTreeMap::new();
		for (tag, fvar) in fvar_axes(font)? {
			let Some(&limit) = self.0.get(&tag) else {
				continue;
			};
			let fixed = |v: f64| float_to_fixed(v, 16);
			if limit.minimum == limit.maximum
				|| fixed(limit.minimum) != fixed(fvar.0)
				|| fixed(limit.default) != fixed(fvar.1)
				|| fixed(limit.maximum) != fixed(fvar.2)
			{
				out.insert(tag, limit);
			}
		}
		Ok(UserLimits(out))
	}

	/// `defaultLocation`.
	pub(crate) fn default_location(&self) -> BTreeMap<String, f64> {
		self.0.iter().map(|(k, v)| (k.clone(), v.default)).collect()
	}

	/// `pinnedLocation`.
	#[doc(hidden)]
	pub fn pinned_location(&self) -> BTreeMap<String, f64> {
		self.0
			.iter()
			.filter(|(_, v)| v.minimum == v.maximum)
			.map(|(k, v)| (k.clone(), v.default))
			.collect()
	}

	/// `normalize`: the limits in normalized coordinates. With `using_avar`, the
	/// `avar` table warps the default normalization.
	#[doc(hidden)]
	pub fn normalize(&self, font: &mut Font, using_avar: bool) -> Result<NormalizedLimits> {
		let all_axes = fvar_axes(font)?;
		if let Some(bad) = self
			.0
			.keys()
			.find(|t| !all_axes.iter().any(|(a, _)| a == *t))
		{
			return Err(Error::value(format!(
				"Cannot limit: {{{bad:?}}} not present in fvar"
			)));
		}
		let axes: Vec<_> = all_axes
			.iter()
			.filter(|(t, _)| self.0.contains_key(t))
			.cloned()
			.collect();
		let has_avar = using_avar && font.contains(Tag::new(b"avar"));
		if has_avar && font.require::<Avar>()?.major_version >= 2 {
			let pinned = self.pinned_location();
			if all_axes.iter().all(|(t, _)| pinned.contains_key(t)) {
				// Full instancing of an avar2 font: the table normalizes the location.
				let mut location = Location::new();
				for (tag, triple) in &axes {
					location.insert(tag.clone(), normalize(pinned[tag], *triple, None)?);
				}
				let mapped = renormalize_location(&location, font)?;
				let mut out = BTreeMap::new();
				for (tag, v) in mapped {
					out.insert(tag, NormalizedAxisTriple::new(v, v, v, 1.0, 1.0)?);
				}
				return Ok(NormalizedLimits(out));
			}
			// Partial instancing of an avar2 font falls through to the standard v1
			// normalization, which yields intermediate-space limits; the avar v2
			// offset compensation in the instancer handles the rest.
		}
		let mut limits = Vec::new();
		for (tag, fvar) in &axes {
			let limit = self.0[tag];
			let values = (
				normalize_value(limit.minimum, *fvar, false)?,
				normalize_value(limit.default, *fvar, false)?,
				normalize_value(limit.maximum, *fvar, false)?,
			);
			limits.push((tag, values, fvar.1 - fvar.0, fvar.2 - fvar.1));
		}
		let mut out = BTreeMap::new();
		for (tag, (minimum, default, maximum), negative, positive) in limits {
			// `renormalizeAxisLimits`, or just the quantizing to F2Dot14, to avoid
			// surprise interpolations.
			let map = |v: f64, segments: Option<&[(f64, f64)]>| {
				let v = segments.map_or(v, |s| piecewise_linear_map(v, s));
				float_to_fixed_to_float(v, 14)
			};
			let segments = if has_avar {
				font.require::<Avar>()?.segments.get(&tag_of(tag)?).cloned()
			} else {
				None
			};
			let segments = segments.as_deref();
			out.insert(
				tag.clone(),
				NormalizedAxisTriple::new(
					map(minimum, segments),
					map(default, segments),
					map(maximum, segments),
					negative,
					positive,
				)?,
			);
		}
		Ok(NormalizedLimits(out))
	}
}

/// `normalize`: a value in normalized coordinates, quantized to F2Dot14.
pub(crate) fn normalize(
	value: f64,
	triple: (f64, f64, f64),
	avar_mapping: Option<&[(f64, f64)]>,
) -> Result<f64> {
	let mut value = normalize_value(value, triple, false)?;
	if let Some(mapping) = avar_mapping.filter(|m| !m.is_empty()) {
		value = piecewise_linear_map(value, mapping);
	}
	// Quantize to F2Dot14, to avoid surprise interpolations.
	Ok(float_to_fixed_to_float(value, 14))
}

/// `avar.renormalizeLocation(location, font, dropZeroes=False)`, for version 2.
pub fn renormalize_location(location: &Location, font: &mut Font) -> Result<Location> {
	let axes: Vec<String> = fvar_axes(font)?.into_iter().map(|a| a.0).collect();
	let avar = font.require::<Avar>()?;
	let mut mapped = Location::new();
	for (tag, &value) in location {
		let value = match avar.segments.get(&tag_of(tag)?) {
			Some(mapping) => piecewise_linear_map(value, mapping),
			None => value,
		};
		mapped.insert(tag.clone(), value);
	}
	let store = avar.table.var_store.as_deref();
	let map = avar.table.var_idx_map.as_deref();
	let instancer = VarStoreInstancer::new(store, &axes, mapped.clone());
	let mut out = Location::new();
	for (index, axis) in axes.iter().enumerate() {
		let mut v = float_to_fixed(mapped.get(axis).copied().unwrap_or(0.0), 14);
		let var_idx = match map {
			// A compiled map may be truncated: reads clamp to the last entry.
			Some(map) => map
				.mapping
				.get(index)
				.or(map.mapping.last())
				.copied()
				.map_or(0xFFFF_FFFF, u64::from),
			None => index as u64,
		};
		if store.is_some() {
			v += ot_round(instancer.get(var_idx)?);
			v = v.clamp(-(1 << 14), 1 << 14);
		}
		out.insert(axis.clone(), fixed_to_float(v, 14));
	}
	Ok(out)
}

/// `TTFont.normalizeLocation`: a location in user coordinates, normalized with `avar`.
pub(crate) fn normalize_font_location(
	font: &mut Font,
	location: &BTreeMap<String, f64>,
) -> Result<Location> {
	let axes = fvar_axes(font)?;
	let pairs: Vec<(String, f64)> = location.iter().map(|(k, v)| (k.clone(), *v)).collect();
	let mut out: Location = crate::models::normalize_location(&pairs, &axes, false)?
		.into_iter()
		.collect();
	if font.contains(Tag::new(b"avar")) {
		if font.require::<Avar>()?.major_version >= 2 {
			// `renormalizeLocation` drops the zeroes by default.
			out = renormalize_location(&out, font)?;
			out.retain(|_, v| *v != 0.0);
		} else {
			let avar = font.require::<Avar>()?;
			for (tag, value) in &mut out {
				if let Some(mapping) = avar.segments.get(&tag_of(tag)?) {
					*value = piecewise_linear_map(*value, mapping);
				}
			}
		}
	}
	Ok(out)
}

/// A triple of (min, default, max) normalized axis values, with the distances
/// between min and default, and default and max, in the pre-normalized space
/// (`NormalizedAxisTripleAndDistances`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalizedAxisTriple {
	/// `minimum`.
	pub minimum: f64,
	/// `default`.
	pub default: f64,
	/// `maximum`.
	pub maximum: f64,
	/// `distanceNegative`.
	pub distance_negative: f64,
	/// `distancePositive`.
	pub distance_positive: f64,
}

impl NormalizedAxisTriple {
	/// A triple, checked to be in the -1..+1 range and in order.
	pub fn new(
		minimum: f64,
		default: f64,
		maximum: f64,
		distance_negative: f64,
		distance_positive: f64,
	) -> Result<Self> {
		if !(-1.0 <= minimum && minimum <= default && default <= maximum && maximum <= 1.0) {
			return Err(Error::value(format!(
				"Normalized axis values not in -1..+1 range; got minimum={minimum}, default={default}, maximum={maximum})"
			)));
		}
		Ok(NormalizedAxisTriple {
			minimum,
			default,
			maximum,
			distance_negative,
			distance_positive,
		})
	}

	/// `reverse_negate`.
	pub fn reverse_negate(self) -> Self {
		NormalizedAxisTriple {
			minimum: -self.maximum,
			default: -self.default,
			maximum: -self.minimum,
			distance_negative: self.distance_positive,
			distance_positive: self.distance_negative,
		}
	}

	/// `renormalizeValue(v, extrapolate=False)`.
	pub fn renormalize_value_clamped(self, v: f64) -> f64 {
		self.renormalize_value(v.min(self.maximum).max(self.minimum))
	}

	/// `renormalizeValue(v)`: a normalized value in the range of this axis.
	pub fn renormalize_value(self, v: f64) -> f64 {
		let (lower, default, upper) = (self.minimum, self.default, self.maximum);
		if v == default {
			return 0.0;
		}
		if default < 0.0 {
			return -self.reverse_negate().renormalize_value(-v);
		}
		// default >= 0 and v != default
		if v > default {
			return (v - default) / (upper - default);
		}
		// v < default
		if lower >= 0.0 {
			return (v - default) / (default - lower);
		}
		// lower < 0 and v < default
		let total_distance = self.distance_negative * -lower + self.distance_positive * default;
		let v_distance = if v >= 0.0 {
			(default - v) * self.distance_positive
		} else {
			-v * self.distance_negative + self.distance_positive * default
		};
		-v_distance / total_distance
	}
}

/// Normalized limits, by axis tag (`NormalizedAxisLimits`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NormalizedLimits(pub BTreeMap<String, NormalizedAxisTriple>);

impl NormalizedLimits {
	/// `pinnedLocation`.
	#[doc(hidden)]
	pub fn pinned_location(&self) -> BTreeMap<String, f64> {
		self.0
			.iter()
			.filter(|(_, v)| v.minimum == v.maximum)
			.map(|(k, v)| (k.clone(), v.default))
			.collect()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn axis_triple() -> Result<()> {
		let triple = AxisTriple::range(100.0, 100.0)?;
		assert_eq!(triple.default, Some(100.0));
		assert!(AxisTriple::range(200.0, 100.0).is_err());
		let limit = AxisTriple::new(Some(100.0), None, Some(300.0))?
			.limit_range_and_populate_defaults((200.0, 400.0, 900.0))?;
		assert_eq!(
			limit,
			Limit {
				minimum: 200.0,
				default: 300.0,
				maximum: 300.0
			}
		);
		Ok(())
	}

	/// `NormalizedAxisTripleAndDistances.renormalizeValue` of the upstream tests.
	#[test]
	fn renormalize_value() -> Result<()> {
		let t = NormalizedAxisTriple::new(-1.0, 0.0, 1.0, 1.0, 1.0)?;
		assert_eq!(t.renormalize_value(0.5), 0.5);
		let t = NormalizedAxisTriple::new(-1.0, 0.5, 1.0, 1.0, 1.0)?;
		assert_eq!(t.renormalize_value(0.5), 0.0);
		assert_eq!(t.renormalize_value(0.75), 0.5);
		assert_eq!(t.renormalize_value(-1.0), -1.0);
		let t = NormalizedAxisTriple::new(-1.0, -0.5, 1.0, 1.0, 1.0)?;
		assert_eq!(t.renormalize_value(-0.75), -0.5);
		Ok(())
	}
}
