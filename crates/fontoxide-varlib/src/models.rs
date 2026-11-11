//! Port of `fontTools.varLib.models`, with its command line (`main`).

use std::io::Write;

use fontoxide_designspacelib::{DesignSpaceDocument, Number};
use fontoxide_misc::argparse::{Argument, Command, Nargs};
use fontoxide_misc::py;
use fontoxide_ttlib::{Error, Result};

/// A location: axis tags and coordinates, in the order of a Python `dict`.
pub type DictLocation = Vec<(String, f64)>;
/// A support: axis tags and (lower, peak, upper) triples, in the order of a Python `dict`.
pub type Support = Vec<(String, (f64, f64, f64))>;

/// `normalizeValue`: a value normalized by a min/default/max triple.
pub fn normalize_value(v: f64, triple: (f64, f64, f64), extrapolate: bool) -> Result<f64> {
	let (lower, default, upper) = triple;
	if !(lower <= default && default <= upper) {
		return Err(Error::value(format!(
			"Invalid axis values, must be minimum, default, maximum: {lower:.3}, {default:.3}, {upper:.3}"
		)));
	}
	let v = if extrapolate {
		v
	} else {
		v.min(upper).max(lower)
	};
	if v == default || lower == upper {
		return Ok(0.0);
	}
	if (v < default && lower != default) || (v > default && upper == default) {
		Ok((v - default) / (default - lower))
	} else {
		Ok((v - default) / (upper - default))
	}
}

/// `normalizeValue` with `allow_decreasing=True`: the triple can also be decreasing. Its
/// first value still maps to -1 and its last to +1.
pub fn normalize_value_decreasing(
	v: f64,
	triple: (f64, f64, f64),
	extrapolate: bool,
) -> Result<f64> {
	let (lower, default, upper) = triple;
	if lower <= upper {
		return normalize_value(v, triple, extrapolate);
	}
	if !(lower >= default && default >= upper) {
		return Err(Error::value(format!(
			"Invalid axis values, must be minimum, default, maximum: {lower:.3}, {default:.3}, {upper:.3}"
		)));
	}
	let normalized = normalize_value(v, (upper, default, lower), extrapolate)?;
	Ok(if normalized != 0.0 { -normalized } else { 0.0 })
}

/// `normalizeLocation`: a location normalized by the min/default/max values of the axes.
pub fn normalize_location(
	location: &[(String, f64)],
	axes: &[(String, (f64, f64, f64))],
	extrapolate: bool,
) -> Result<DictLocation> {
	axes.iter()
		.map(|(tag, triple)| {
			let v = location_get(location, tag).unwrap_or(triple.1);
			Ok((tag.clone(), normalize_value(v, *triple, extrapolate)?))
		})
		.collect()
}

/// `piecewiseLinearMap`, for a mapping in the order of a Python `dict`.
pub fn piecewise_linear_map(v: f64, mapping: &[(f64, f64)]) -> f64 {
	if mapping.is_empty() {
		return v;
	}
	if let Some(&(_, to)) = mapping.iter().find(|(from, _)| *from == v) {
		return to;
	}
	let extreme = |pick: fn(f64, f64) -> f64| {
		mapping
			.iter()
			.copied()
			.reduce(|a, b| if pick(a.0, b.0) == a.0 { a } else { b })
	};
	if let Some((k, to)) = extreme(f64::min)
		&& v < k
	{
		return v + to - k;
	}
	if let Some((k, to)) = extreme(f64::max)
		&& v > k
	{
		return v + to - k;
	}
	// Interpolate.
	let a = mapping
		.iter()
		.copied()
		.filter(|(k, _)| *k < v)
		.reduce(|a, b| if a.0 >= b.0 { a } else { b });
	let b = mapping
		.iter()
		.copied()
		.filter(|(k, _)| *k > v)
		.reduce(|a, b| if a.0 <= b.0 { a } else { b });
	match (a, b) {
		(Some((a, va)), Some((b, vb))) => va + (vb - va) * (v - a) / (b - a),
		_ => v,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn normalize() -> Result<()> {
		assert_eq!(normalize_value(400.0, (100.0, 400.0, 900.0), false)?, 0.0);
		assert_eq!(normalize_value(100.0, (100.0, 400.0, 900.0), false)?, -1.0);
		assert_eq!(normalize_value(650.0, (100.0, 400.0, 900.0), false)?, 0.5);
		assert_eq!(normalize_value(0.0, (100.0, 400.0, 900.0), false)?, -1.0);
		assert_eq!(normalize_value(1000.0, (100.0, 400.0, 900.0), true)?, 1.2);
		assert!(normalize_value(1.0, (2.0, 1.0, 3.0), false).is_err());
		Ok(())
	}

	/// `test_piecewiseLinearMap`.
	#[test]
	fn piecewise() {
		let map = [(0.0, 0.0), (1.0, 2.0), (-1.0, -3.0)];
		assert_eq!(piecewise_linear_map(0.5, &map), 1.0);
		assert_eq!(piecewise_linear_map(-0.5, &map), -1.5);
		assert_eq!(piecewise_linear_map(2.0, &map), 3.0);
		assert_eq!(piecewise_linear_map(-2.0, &map), -4.0);
		assert_eq!(piecewise_linear_map(1.0, &map), 2.0);
		assert_eq!(piecewise_linear_map(0.25, &[]), 0.25);
	}
}

fn location_get(location: &[(String, f64)], axis: &str) -> Option<f64> {
	location.iter().find(|e| e.0 == axis).map(|e| e.1)
}

/// `supportScalar` (with `ot=True`, without extrapolation): the scalar multiplier at a
/// location, for a master with a support. A peak value of zero for an axis means "axis does
/// not participate".
pub fn support_scalar(location: &[(String, f64)], support: &[(String, (f64, f64, f64))]) -> f64 {
	let mut scalar = 1.0;
	for (axis, (lower, peak, upper)) in support {
		let (lower, peak, upper) = (*lower, *peak, *upper);
		if peak == 0.0 || lower > peak || peak > upper || (lower < 0.0 && upper > 0.0) {
			continue;
		}
		let v = location_get(location, axis).unwrap_or(0.0);
		if v == peak {
			continue;
		}
		if v <= lower || upper <= v {
			scalar = 0.0;
			break;
		}
		if v < peak {
			scalar *= (v - lower) / (peak - lower);
		} else {
			scalar *= (v - upper) / (peak - upper);
		}
	}
	scalar
}

/// `supportScalar` with `extrapolate=True`; `axis_ranges` has the (min, max) of each axis.
pub fn support_scalar_extrapolate(
	location: &[(String, f64)],
	support: &[(String, (f64, f64, f64))],
	axis_ranges: &[(String, (f64, f64))],
) -> Result<f64> {
	let mut scalar = 1.0;
	for (axis, (lower, peak, upper)) in support {
		let (lower, peak, upper) = (*lower, *peak, *upper);
		if peak == 0.0 || lower > peak || peak > upper || (lower < 0.0 && upper > 0.0) {
			continue;
		}
		let v = location_get(location, axis).unwrap_or(0.0);
		if v == peak {
			continue;
		}
		let (axis_min, axis_max) = axis_ranges
			.iter()
			.find(|e| e.0 == *axis)
			.map(|e| e.1)
			.ok_or_else(|| Error::ttlib(format!("KeyError: {axis:?}")))?;
		if v < axis_min && lower <= axis_min {
			if peak <= axis_min && peak < upper {
				scalar *= (v - upper) / (peak - upper);
				continue;
			} else if axis_min < peak {
				scalar *= (v - lower) / (peak - lower);
				continue;
			}
		} else if axis_max < v && axis_max <= upper {
			if axis_max <= peak && lower < peak {
				scalar *= (v - lower) / (peak - lower);
				continue;
			} else if peak < axis_max {
				scalar *= (v - upper) / (peak - upper);
				continue;
			}
		}
		if v <= lower || upper <= v {
			scalar = 0.0;
			break;
		}
		if v < peak {
			scalar *= (v - lower) / (peak - lower);
		} else {
			scalar *= (v - upper) / (peak - upper);
		}
	}
	Ok(scalar)
}

/// `supportScalar` with all its options. With `ot` a peak value of zero for an axis means
/// "axis does not participate", as in OpenType; `axis_ranges` has the (min, max) of each
/// axis, and must be given with `extrapolate`.
pub fn support_scalar_with(
	location: &[(String, f64)],
	support: &[(String, (f64, f64, f64))],
	ot: bool,
	extrapolate: bool,
	axis_ranges: Option<&[(String, (f64, f64))]>,
) -> Result<f64> {
	if extrapolate && axis_ranges.is_none() {
		return Err(Error::ttlib(
			"TypeError: axisRanges must be passed when extrapolate is True",
		));
	}
	if ot {
		return match axis_ranges.filter(|_| extrapolate) {
			Some(ranges) => support_scalar_extrapolate(location, support, ranges),
			None => Ok(support_scalar(location, support)),
		};
	}
	let mut scalar = 1.0;
	for (axis, (lower, peak, upper)) in support {
		let (lower, peak, upper) = (*lower, *peak, *upper);
		let v = location_get(location, axis)
			.ok_or_else(|| Error::Assertion("axis in location".into()))?;
		if v == peak {
			continue;
		}
		if let Some(ranges) = axis_ranges.filter(|_| extrapolate) {
			let (axis_min, axis_max) = ranges
				.iter()
				.find(|e| e.0 == *axis)
				.map(|e| e.1)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {axis:?}")))?;
			if v < axis_min && lower <= axis_min {
				if peak <= axis_min && peak < upper {
					scalar *= (v - upper) / (peak - upper);
					continue;
				} else if axis_min < peak {
					scalar *= (v - lower) / (peak - lower);
					continue;
				}
			} else if axis_max < v && axis_max <= upper {
				if axis_max <= peak && lower < peak {
					scalar *= (v - lower) / (peak - lower);
					continue;
				} else if peak < axis_max {
					scalar *= (v - upper) / (peak - upper);
					continue;
				}
			}
		}
		if v <= lower || upper <= v {
			scalar = 0.0;
			break;
		}
		if v < peak {
			scalar *= (v - lower) / (peak - lower);
		} else {
			scalar *= (v - upper) / (peak - upper);
		}
	}
	Ok(scalar)
}

/// `VariationModel`. The locations must have the base master at the origin (ie. 0).
#[derive(Debug, Clone, PartialEq)]
pub struct VariationModel {
	/// `origLocations`.
	pub orig_locations: Vec<DictLocation>,
	/// `axisOrder`.
	pub axis_order: Vec<String>,
	/// `extrapolate`.
	pub extrapolate: bool,
	/// `axisRanges`: the (min, max) of each axis.
	pub axis_ranges: Vec<(String, (f64, f64))>,
	/// `locations`, sorted.
	pub locations: Vec<DictLocation>,
	/// `mapping`: from the user's master order to the sorted one.
	pub mapping: Vec<usize>,
	/// `reverseMapping`.
	pub reverse_mapping: Vec<usize>,
	/// `supports`.
	pub supports: Vec<Support>,
	/// `deltaWeights`, by master index in order.
	pub delta_weights: Vec<Vec<(usize, f64)>>,
}

/// `dict == dict`.
fn same_location(a: &[(String, f64)], b: &[(String, f64)]) -> bool {
	a.len() == b.len() && a.iter().all(|(k, v)| location_get(b, k) == Some(*v))
}

fn location_position(list: &[DictLocation], location: &[(String, f64)]) -> Option<usize> {
	list.iter().position(|l| same_location(l, location))
}

/// `sign`.
fn sign(v: f64) -> i8 {
	if v < 0.0 {
		-1
	} else if v > 0.0 {
		1
	} else {
		0
	}
}

/// The sort key of `getMasterLocationsSortKeyFunc`.
#[derive(Debug, PartialEq)]
struct SortKey {
	rank: usize,
	on_point_axes: i64,
	axis_positions: Vec<usize>,
	ordered_axes: Vec<String>,
	signs: Vec<i8>,
	abs_values: Vec<f64>,
}

impl SortKey {
	fn cmp(&self, other: &SortKey) -> std::cmp::Ordering {
		let floats = |a: &[f64], b: &[f64]| {
			for (x, y) in a.iter().zip(b) {
				let ord = x.total_cmp(y);
				if ord != std::cmp::Ordering::Equal {
					return ord;
				}
			}
			a.len().cmp(&b.len())
		};
		self.rank
			.cmp(&other.rank)
			.then(self.on_point_axes.cmp(&other.on_point_axes))
			.then_with(|| self.axis_positions.cmp(&other.axis_positions))
			.then_with(|| self.ordered_axes.cmp(&other.ordered_axes))
			.then_with(|| self.signs.cmp(&other.signs))
			.then_with(|| floats(&self.abs_values, &other.abs_values))
	}
}

impl VariationModel {
	/// `VariationModel(locations, axisOrder, extrapolate, axisRanges=...)`.
	pub fn new(
		locations: &[DictLocation],
		axis_order: Option<&[String]>,
		extrapolate: bool,
		axis_ranges: Option<&[(String, (f64, f64))]>,
	) -> Result<Self> {
		let locations: Vec<DictLocation> = locations
			.iter()
			.map(|l| l.iter().filter(|e| e.1 != 0.0).cloned().collect())
			.collect();
		for (i, a) in locations.iter().enumerate() {
			if locations[..i].iter().any(|b| same_location(a, b)) {
				return Err(Error::ttlib(
					"VariationModelError: Locations must be unique.",
				));
			}
		}
		let axis_order = axis_order.map_or_else(Vec::new, <[String]>::to_vec);
		let axis_ranges = match axis_ranges {
			Some(ranges) => ranges.to_vec(),
			None if extrapolate => Self::compute_axis_ranges(&locations),
			None => {
				let mut all: Vec<(String, (f64, f64))> = Vec::new();
				for (axis, _) in locations.iter().flatten() {
					if !all.iter().any(|e| e.0 == *axis) {
						all.push((axis.clone(), (-1.0, 1.0)));
					}
				}
				all
			}
		};
		let sorted = sort_locations(&locations, &axis_order)?;
		let mapping = locations
			.iter()
			.map(|l| location_position(&sorted, l).unwrap_or(0))
			.collect();
		let reverse_mapping = sorted
			.iter()
			.map(|l| location_position(&locations, l).unwrap_or(0))
			.collect();
		let mut model = VariationModel {
			orig_locations: locations,
			axis_order,
			extrapolate,
			axis_ranges,
			locations: sorted,
			mapping,
			reverse_mapping,
			supports: Vec::new(),
			delta_weights: Vec::new(),
		};
		model.compute_master_supports()?;
		Ok(model)
	}

	/// `computeAxisRanges`.
	pub fn compute_axis_ranges(locations: &[DictLocation]) -> Vec<(String, (f64, f64))> {
		let mut all: Vec<&String> = Vec::new();
		for (axis, _) in locations.iter().flatten() {
			if !all.contains(&axis) {
				all.push(axis);
			}
		}
		let mut ranges: Vec<(String, (f64, f64))> = Vec::new();
		for location in locations {
			for &axis in &all {
				let value = location_get(location, axis).unwrap_or(0.0);
				match ranges.iter_mut().find(|e| e.0 == *axis) {
					Some(e) => e.1 = (e.1.0.min(value), e.1.1.max(value)),
					None => ranges.push((axis.clone(), (value, value))),
				}
			}
		}
		ranges
	}

	fn axis_range(&self, axis: &str) -> Result<(f64, f64)> {
		self.axis_ranges
			.iter()
			.find(|e| e.0 == axis)
			.map(|e| e.1)
			.ok_or_else(|| Error::ttlib(format!("KeyError: {axis:?}")))
	}

	/// `_computeMasterSupports`.
	fn compute_master_supports(&mut self) -> Result<()> {
		// `_locationsToRegions`.
		let mut regions: Vec<Support> = Vec::with_capacity(self.locations.len());
		for location in &self.locations {
			let mut region = Vec::with_capacity(location.len());
			for (axis, v) in location {
				let range = self.axis_range(axis)?;
				region.push((
					axis.clone(),
					if *v > 0.0 {
						(0.0, *v, range.1)
					} else {
						(range.0, *v, 0.0)
					},
				));
			}
			regions.push(region);
		}
		let mut supports: Vec<Support> = Vec::with_capacity(regions.len());
		for i in 0..regions.len() {
			let (previous, rest) = regions.split_at_mut(i);
			let region = &mut rest[0];
			// Walk over previous masters now.
			for prev_region in previous.iter() {
				// Masters with different axes do not participate.
				let has_axes = prev_region.len() == region.len()
					&& prev_region
						.iter()
						.all(|(axis, _)| region.iter().any(|e| e.0 == *axis));
				if !has_axes {
					continue;
				}
				let prev_peak = |axis: &str| -> f64 {
					prev_region
						.iter()
						.find(|e| e.0 == axis)
						.map_or(0.0, |e| e.1.1)
				};
				// If it's NOT in the current box, it does not participate.
				let relevant = region.iter().all(|(axis, (lower, peak, upper))| {
					let p = prev_peak(axis);
					p == *peak || (*lower < p && p < *upper)
				});
				if !relevant {
					continue;
				}
				// Split the box for the new master, in whatever direction has the largest
				// range ratio. For symmetry, cut across multiple axes if they have the
				// largest, equal, ratio.
				let mut best_axes: Vec<(String, (f64, f64, f64))> = Vec::new();
				let mut best_ratio = -1.0;
				for (axis, prev) in prev_region {
					let val = prev.1;
					let (lower, loc_v, upper) =
						region
							.iter()
							.find(|e| e.0 == *axis)
							.map(|e| e.1)
							.ok_or_else(|| Error::Assertion("axis in region".into()))?;
					let (mut new_lower, mut new_upper) = (lower, upper);
					let ratio;
					if val < loc_v {
						new_lower = val;
						ratio = (val - loc_v) / (lower - loc_v);
					} else if loc_v < val {
						new_upper = val;
						ratio = (val - loc_v) / (upper - loc_v);
					} else {
						// Can't split the box in this direction.
						continue;
					}
					if ratio > best_ratio {
						best_axes.clear();
						best_ratio = ratio;
					}
					if ratio == best_ratio {
						best_axes.push((axis.clone(), (new_lower, loc_v, new_upper)));
					}
				}
				for (axis, triple) in best_axes {
					if let Some(e) = region.iter_mut().find(|e| e.0 == axis) {
						e.1 = triple;
					}
				}
			}
			supports.push(region.clone());
		}
		self.supports = supports;
		// `_computeDeltaWeights`.
		self.delta_weights = self
			.locations
			.iter()
			.enumerate()
			.map(|(i, location)| {
				self.supports[..i]
					.iter()
					.enumerate()
					.filter_map(|(j, support)| {
						let scalar = support_scalar(location, support);
						(scalar != 0.0).then_some((j, scalar))
					})
					.collect()
			})
			.collect();
		Ok(())
	}

	/// `getDeltas`.
	pub fn get_deltas(&self, master_values: &[f64]) -> Result<Vec<f64>> {
		self.get_deltas_rounded(master_values, |v| v)
	}

	/// `getDeltas(masterValues, round=round)`.
	pub fn get_deltas_rounded(
		&self,
		master_values: &[f64],
		round: impl Fn(f64) -> f64,
	) -> Result<Vec<f64>> {
		if master_values.len() != self.delta_weights.len() {
			return Err(Error::Assertion(format!(
				"({}, {})",
				master_values.len(),
				self.delta_weights.len()
			)));
		}
		let mut out: Vec<f64> = Vec::with_capacity(master_values.len());
		for (i, weights) in self.delta_weights.iter().enumerate() {
			let mut delta = master_values[self.reverse_mapping[i]];
			for &(j, weight) in weights {
				if weight == 1.0 {
					delta -= out[j];
				} else {
					delta -= out[j] * weight;
				}
			}
			out.push(round(delta));
		}
		Ok(out)
	}

	/// `getSubModel`: a model of the masters whose value isn't `None`, and those values.
	pub fn get_sub_model(&self, items: &[Option<f64>]) -> Result<(VariationModel, Vec<f64>)> {
		if items.len() != self.orig_locations.len() {
			return Err(Error::Assertion("len(truth) == len(lst)".into()));
		}
		let locations: Vec<DictLocation> = self
			.orig_locations
			.iter()
			.zip(items)
			.filter(|(_, v)| v.is_some())
			.map(|(l, _)| l.clone())
			.collect();
		let model = VariationModel::new(
			&locations,
			Some(&self.axis_order),
			self.extrapolate,
			Some(&self.axis_ranges),
		)?;
		Ok((model, items.iter().flatten().copied().collect()))
	}

	/// `getSubModel` for the masters in `present`: the model itself if all are.
	pub fn sub_model(&self, present: &[bool]) -> Result<VariationModel> {
		if present.len() != self.orig_locations.len() {
			return Err(Error::Assertion("len(truth) == len(lst)".into()));
		}
		if present.iter().all(|p| *p) {
			return Ok(self.clone());
		}
		let locations: Vec<DictLocation> = self
			.orig_locations
			.iter()
			.zip(present)
			.filter(|(_, p)| **p)
			.map(|(l, _)| l.clone())
			.collect();
		VariationModel::new(
			&locations,
			Some(&self.axis_order),
			self.extrapolate,
			Some(&self.axis_ranges),
		)
	}

	/// `getDeltas` of vectors, such as the coordinates of a glyph: each component
	/// as [`VariationModel::get_deltas_rounded`] computes it.
	pub fn get_deltas_vec(
		&self,
		master_values: &[Vec<f64>],
		round: impl Fn(f64) -> f64,
	) -> Result<Vec<Vec<f64>>> {
		if master_values.len() != self.delta_weights.len() {
			return Err(Error::Assertion(format!(
				"({}, {})",
				master_values.len(),
				self.delta_weights.len()
			)));
		}
		let mut out: Vec<Vec<f64>> = Vec::with_capacity(master_values.len());
		for (i, weights) in self.delta_weights.iter().enumerate() {
			let mut delta = master_values[self.reverse_mapping[i]].clone();
			for &(j, weight) in weights {
				let other = &out[j];
				if other.len() != delta.len() {
					return Err(Error::value("operands must have the same length"));
				}
				for (d, o) in delta.iter_mut().zip(other) {
					if weight == 1.0 {
						*d -= o;
					} else {
						*d -= o * weight;
					}
				}
			}
			out.push(delta.into_iter().map(&round).collect());
		}
		Ok(out)
	}

	/// `reorderMasters`: the masters in a new order, and the model updated for it,
	/// without recomputing supports.
	pub fn reorder_masters<T: Clone>(&mut self, master_list: &[T], mapping: &[usize]) -> Vec<T> {
		let new_list = mapping.iter().map(|&i| master_list[i].clone()).collect();
		self.orig_locations = mapping
			.iter()
			.map(|&i| self.orig_locations[i].clone())
			.collect();
		let locations: Vec<DictLocation> = self
			.orig_locations
			.iter()
			.map(|l| l.iter().filter(|e| e.1 != 0.0).cloned().collect())
			.collect();
		self.mapping = locations
			.iter()
			.map(|l| location_position(&self.locations, l).unwrap_or(0))
			.collect();
		self.reverse_mapping = self
			.locations
			.iter()
			.map(|l| location_position(&locations, l).unwrap_or(0))
			.collect();
		new_list
	}

	/// `getDeltasAndSupports`.
	pub fn get_deltas_and_supports(
		&self,
		items: &[Option<f64>],
	) -> Result<(Vec<f64>, Vec<Support>)> {
		self.get_deltas_and_supports_rounded(items, |v| v)
	}

	/// `getDeltasAndSupports(items, round=round)`.
	pub fn get_deltas_and_supports_rounded(
		&self,
		items: &[Option<f64>],
		round: impl Fn(f64) -> f64,
	) -> Result<(Vec<f64>, Vec<Support>)> {
		let (model, items) = self.get_sub_model(items)?;
		Ok((model.get_deltas_rounded(&items, round)?, model.supports))
	}

	/// `getScalars`: the scalars of each delta, for the given location.
	pub fn get_scalars(&self, location: &[(String, f64)]) -> Result<Vec<f64>> {
		self.supports
			.iter()
			.map(|support| {
				if self.extrapolate {
					support_scalar_extrapolate(location, support, &self.axis_ranges)
				} else {
					Ok(support_scalar(location, support))
				}
			})
			.collect()
	}

	/// `getMasterScalars`: the multipliers of each master, for the given location.
	pub fn get_master_scalars(&self, location: &[(String, f64)]) -> Result<Vec<f64>> {
		let mut out = self.get_scalars(location)?;
		for i in (0..self.delta_weights.len()).rev() {
			for &(j, weight) in &self.delta_weights[i] {
				out[j] -= out[i] * weight;
			}
		}
		Ok((0..out.len()).map(|i| out[self.mapping[i]]).collect())
	}

	/// `interpolateFromValuesAndScalars`; `None` if no scalar applies.
	pub fn interpolate_from_values_and_scalars(
		values: &[f64],
		scalars: &[f64],
	) -> Result<Option<f64>> {
		if values.len() != scalars.len() {
			return Err(Error::Assertion("len(values) == len(scalars)".into()));
		}
		let mut v = None;
		for (value, scalar) in values.iter().zip(scalars) {
			if *scalar == 0.0 {
				continue;
			}
			let contribution = value * scalar;
			v = Some(v.map_or(contribution, |v: f64| v + contribution));
		}
		Ok(v)
	}

	/// `interpolateFromDeltasAndScalars`; `None` if no scalar applies.
	pub fn interpolate_from_deltas_and_scalars(
		deltas: &[f64],
		scalars: &[f64],
	) -> Result<Option<f64>> {
		Self::interpolate_from_values_and_scalars(deltas, scalars)
	}

	/// `interpolateFromDeltas`.
	pub fn interpolate_from_deltas(
		&self,
		location: &[(String, f64)],
		deltas: &[f64],
	) -> Result<Option<f64>> {
		Self::interpolate_from_values_and_scalars(deltas, &self.get_scalars(location)?)
	}

	/// `interpolateFromMasters`.
	pub fn interpolate_from_masters(
		&self,
		location: &[(String, f64)],
		master_values: &[f64],
	) -> Result<Option<f64>> {
		Self::interpolate_from_values_and_scalars(
			master_values,
			&self.get_master_scalars(location)?,
		)
	}

	/// `interpolateFromMastersAndScalars`.
	pub fn interpolate_from_masters_and_scalars(
		&self,
		master_values: &[f64],
		scalars: &[f64],
	) -> Result<Option<f64>> {
		Self::interpolate_from_values_and_scalars(&self.get_deltas(master_values)?, scalars)
	}
}

/// `sorted(locations, key=getMasterLocationsSortKeyFunc(locations, axisOrder))`.
fn sort_locations(locations: &[DictLocation], axis_order: &[String]) -> Result<Vec<DictLocation>> {
	if !locations.iter().any(Vec::is_empty) {
		return Err(Error::ttlib("VariationModelError: Base master not found."));
	}
	let mut axis_points: Vec<(String, Vec<f64>)> = Vec::new();
	for location in locations {
		let [(axis, value)] = location.as_slice() else {
			continue;
		};
		let points = match axis_points.iter_mut().find(|e| e.0 == *axis) {
			Some(e) => &mut e.1,
			None => {
				axis_points.push((axis.clone(), vec![0.0]));
				&mut axis_points
					.last_mut()
					.ok_or_else(|| Error::ttlib("unreachable"))?
					.1
			}
		};
		if points.contains(value) {
			return Err(Error::Assertion(format!(
				"Value \"{value}\" in axisPoints[\"{axis}\"]"
			)));
		}
		points.push(*value);
	}
	let key = |location: &DictLocation| -> SortKey {
		let on_point = location
			.iter()
			.filter(|(axis, value)| {
				axis_points
					.iter()
					.any(|(a, points)| a == axis && points.contains(value))
			})
			.count();
		let mut ordered: Vec<String> = axis_order
			.iter()
			.filter(|a| location_get(location, a).is_some())
			.cloned()
			.collect();
		let mut others: Vec<String> = location
			.iter()
			.map(|e| e.0.clone())
			.filter(|a| !axis_order.contains(a))
			.collect();
		others.sort();
		ordered.extend(others);
		let value = |axis: &String| location_get(location, axis).unwrap_or(0.0);
		SortKey {
			rank: location.len(),
			on_point_axes: -(on_point as i64),
			axis_positions: ordered
				.iter()
				.map(|axis| axis_order.iter().position(|a| a == axis).unwrap_or(0x10000))
				.collect(),
			signs: ordered.iter().map(|a| sign(value(a))).collect(),
			abs_values: ordered.iter().map(|a| value(a).abs()).collect(),
			ordered_axes: ordered,
		}
	};
	let mut keyed: Vec<(SortKey, DictLocation)> =
		locations.iter().map(|l| (key(l), l.clone())).collect();
	keyed.sort_by(|a, b| a.0.cmp(&b.0));
	Ok(keyed.into_iter().map(|e| e.1).collect())
}

/// A Python object, as [`pprint`] prints it.
#[derive(Debug, Clone)]
enum PyObject {
	/// An `int` or a `float`.
	Number(Number),
	/// A `tuple`.
	Tuple(Vec<PyObject>),
	/// A `list`.
	List(Vec<PyObject>),
	/// A `dict` with `str` keys.
	Dict(Vec<(String, PyObject)>),
}

impl PyObject {
	/// `repr(object)`, with the keys of the dicts sorted, as `pprint` writes it.
	fn repr(&self) -> String {
		let join = |items: &[PyObject]| -> Vec<String> { items.iter().map(Self::repr).collect() };
		match self {
			PyObject::Number(n) => n.repr(),
			PyObject::Tuple(items) if items.len() == 1 => format!("({},)", items[0].repr()),
			PyObject::Tuple(items) => format!("({})", join(items).join(", ")),
			PyObject::List(items) => format!("[{}]", join(items).join(", ")),
			PyObject::Dict(items) => {
				let items: Vec<String> = sorted_items(items)
					.iter()
					.map(|(k, v)| format!("{}: {}", py::str_repr(k), v.repr()))
					.collect();
				format!("{{{}}}", items.join(", "))
			}
		}
	}
}

/// The items of a `dict`, sorted by key as `pprint` sorts them.
fn sorted_items(items: &[(String, PyObject)]) -> Vec<&(String, PyObject)> {
	let mut items: Vec<&(String, PyObject)> = items.iter().collect();
	items.sort_by(|a, b| a.0.cmp(&b.0));
	items
}

/// `pprint.pprint(object)` (width 80, indent 1), as a string ending with a
/// newline.
fn pprint(object: &PyObject) -> String {
	let mut out = String::new();
	pformat(object, &mut out, 0, 0);
	out.push('\n');
	out
}

/// `PrettyPrinter._format`.
fn pformat(object: &PyObject, out: &mut String, indent: usize, allowance: usize) {
	const WIDTH: isize = 80;
	let rep = object.repr();
	let max_width = WIDTH - indent as isize - allowance as isize;
	if rep.chars().count() as isize <= max_width {
		out.push_str(&rep);
		return;
	}
	match object {
		PyObject::Number(_) => out.push_str(&rep),
		PyObject::List(items) => {
			out.push('[');
			format_items(items, out, indent, allowance + 1);
			out.push(']');
		}
		PyObject::Tuple(items) => {
			out.push('(');
			format_items(items, out, indent, allowance + 1);
			out.push_str(if items.len() == 1 { ",)" } else { ")" });
		}
		PyObject::Dict(items) => {
			// `_pprint_dict` and `_format_dict_items`.
			out.push('{');
			let items = sorted_items(items);
			let allowance = allowance + 1;
			let indent = indent + 1;
			let last_index = items.len().saturating_sub(1);
			for (i, (key, value)) in items.into_iter().enumerate() {
				let last = i == last_index;
				let key = py::str_repr(key);
				out.push_str(&key);
				out.push_str(": ");
				let width = key.chars().count();
				pformat(
					value,
					out,
					indent + width + 2,
					if last { allowance } else { 1 },
				);
				if !last {
					out.push_str(",\n");
					out.push_str(&" ".repeat(indent));
				}
			}
			out.push('}');
		}
	}
}

/// `PrettyPrinter._format_items` (not compact).
fn format_items(items: &[PyObject], out: &mut String, indent: usize, allowance: usize) {
	let indent = indent + 1;
	for (i, item) in items.iter().enumerate() {
		let last = i + 1 == items.len();
		if i > 0 {
			out.push_str(",\n");
			out.push_str(&" ".repeat(indent));
		}
		pformat(item, out, indent, if last { allowance } else { 1 });
	}
}

const HELP: &str = "\
usage: fonttools varLib.models [-h] [--loglevel LEVEL] (-d DESIGNSPACE |
                               -l LOCATION [LOCATION ...])

Normalize locations on a given designspace

options:
  -h, --help            show this help message and exit
  --loglevel LEVEL      Logging level (defaults to INFO)
  -d, --designspace DESIGNSPACE
  -l, --locations LOCATION [LOCATION ...]
                        Master locations as comma-separate coordinates. One
                        must be all zeros.
";

const COMMAND: Command = Command {
	prog: "fonttools varLib.models",
	module: "varLib.models",
	help: HELP,
	arguments: &[
		Argument::help(),
		Argument::option(&["--loglevel"]),
		Argument::option(&["-d", "--designspace"]).group(0),
		Argument::option(&["-l", "--locations"])
			.nargs(Nargs::OneOrMore)
			.group(0),
	],
	required_groups: &[0],
};

/// Locations with the `int` or `float` values Python has.
type NumberLocation = Vec<(String, Number)>;

/// The locations as a `list` of `dict`s.
fn locations_object(locations: &[NumberLocation]) -> PyObject {
	PyObject::List(
		locations
			.iter()
			.map(|l| {
				PyObject::Dict(
					l.iter()
						.map(|(k, v)| (k.clone(), PyObject::Number(*v)))
						.collect(),
				)
			})
			.collect(),
	)
}

/// The source locations of a designspace (`[s.location for s in doc.sources]`).
///
/// LENIENT(L56): the x value of an anisotropic location is used; fontTools prints
/// the `(x, y)` tuple, then fails in `VariationModel`.
fn source_locations(doc: &DesignSpaceDocument) -> Vec<NumberLocation> {
	doc.sources
		.iter()
		.map(|s| {
			s.design_location
				.iter()
				.map(|(k, v)| (k.clone(), v.x()))
				.collect()
		})
		.collect()
}

/// `main`: prints the sorted master locations and the supports of the variation
/// model of the masters of a designspace (`-d`, after their original and
/// normalized locations) or of the given locations (`-l`). Returns the exit
/// code.
///
/// `--loglevel` is checked as `configLogger` does; logging is up to the
/// caller's `log` implementation.
pub fn main(args: &[&str]) -> crate::errors::Result<i32> {
	run(
		args,
		&mut std::io::stdout().lock(),
		&mut std::io::stderr().lock(),
	)
}

/// [`main`], writing to `out`, and the usage errors to `err`.
pub fn run(args: &[&str], out: &mut dyn Write, err: &mut dyn Write) -> crate::errors::Result<i32> {
	let options = match COMMAND.parse_args(args, out, err)? {
		Ok(options) => options,
		Err(code) => return Ok(code),
	};
	let level = options.str("--loglevel").unwrap_or("INFO");
	if !matches!(
		level,
		"CRITICAL" | "FATAL" | "ERROR" | "WARN" | "WARNING" | "INFO" | "DEBUG" | "NOTSET"
	) {
		return Err(Error::value(format!("Unknown level: {}", py::str_repr(level))).into());
	}

	let locs: Vec<NumberLocation> = match options.str("--designspace") {
		Some(path) if !path.is_empty() => {
			let mut doc = DesignSpaceDocument::new();
			doc.read(path)?;
			writeln!(out, "Original locations:")?;
			out.write_all(pprint(&locations_object(&source_locations(&doc))).as_bytes())?;
			doc.normalize()?;
			writeln!(out, "Normalized locations:")?;
			let locs = source_locations(&doc);
			out.write_all(pprint(&locations_object(&locs)).as_bytes())?;
			locs
		}
		_ => {
			let Some(seen) = options.get("--locations") else {
				return Err(Error::value("TypeError: 'NoneType' object is not iterable").into());
			};
			let mut locs = Vec::new();
			for s in &seen.values {
				let mut loc: NumberLocation = Vec::new();
				for (axis, v) in ('A'..='Z').zip(s.split(',')) {
					let v = py::parse_float(v).ok_or_else(|| {
						Error::value(format!(
							"ValueError: could not convert string to float: {}",
							py::str_repr(v)
						))
					})?;
					loc.push((axis.to_string(), Number::Float(v)));
				}
				locs.push(loc);
			}
			locs
		}
	};

	let dict_locs: Vec<DictLocation> = locs
		.iter()
		.map(|l| l.iter().map(|(k, v)| (k.clone(), v.to_f64())).collect())
		.collect();
	let model = VariationModel::new(&dict_locs, None, false, None)?;

	// The values of the model are those of the locations, `int` or `float`.
	let number = |axis: &str, v: f64| -> Number {
		locs.iter()
			.flatten()
			.find(|(k, n)| k == axis && n.to_f64() == v)
			.map_or(Number::Float(v), |(_, n)| *n)
	};
	writeln!(out, "Sorted locations:")?;
	let sorted: Vec<NumberLocation> = model
		.locations
		.iter()
		.map(|l| l.iter().map(|(k, v)| (k.clone(), number(k, *v))).collect())
		.collect();
	out.write_all(pprint(&locations_object(&sorted)).as_bytes())?;
	writeln!(out, "Supports:")?;
	// The bounds of the regions are `int`s (0, and the axis ranges -1 and 1)
	// unless a box was split at the peak of another master, which falls strictly
	// inside the box.
	let bound = |v: f64| -> Number {
		if v == 0.0 || v == 1.0 || v == -1.0 {
			Number::Int(v as i64)
		} else {
			Number::Float(v)
		}
	};
	let supports = PyObject::List(
		model
			.supports
			.iter()
			.map(|support| {
				PyObject::Dict(
					support
						.iter()
						.map(|(axis, (lower, peak, upper))| {
							(
								axis.clone(),
								PyObject::Tuple(vec![
									PyObject::Number(bound(*lower)),
									PyObject::Number(number(axis, *peak)),
									PyObject::Number(bound(*upper)),
								]),
							)
						})
						.collect(),
				)
			})
			.collect(),
	);
	out.write_all(pprint(&supports).as_bytes())?;
	Ok(0)
}
