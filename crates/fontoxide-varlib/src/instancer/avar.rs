//! Instancing of `avar` (`instantiateAvar`), and all that partial instancing of a
//! version 2 table takes: the variation tables stay in the old coordinate space, and
//! offset compensation in the `avar` table keeps the final coordinates there.

use std::collections::{BTreeMap, HashMap};

use fontoxide_misc::fixed_tools::float_to_fixed_to_float;
use fontoxide_misc::round_tools::ot_round;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::var_store::{
	build_var_region_list, build_var_store, get_extremes, optimize,
};
use fontoxide_ttlib::ot::varc::NO_VARIATION_INDEX;
use fontoxide_ttlib::ot::varmap::DeltaSetIndexMap;
use fontoxide_ttlib::tables::avar::SegmentMap;
use fontoxide_ttlib::tables::colr::Colr;
use fontoxide_ttlib::tables::glyf::Number;
use fontoxide_ttlib::tables::tuple_variation::{Delta, TupleVariation};
use fontoxide_ttlib::tables::{Avar, Base, Cvar, Fvar, Gdef, Gvar, Hvar, Mvar, Vvar};
use fontoxide_ttlib::{Error, Font, Result, Tag};

use super::item_store::TupleVarStore;
use super::limits::{NormalizedAxisTriple, NormalizedLimits, UserLimits, fvar_axes, normalize};
use crate::models::{VariationModel, normalize_value, piecewise_linear_map};

const AVAR: Tag = Tag::new(b"avar");

/// The final coordinate of each axis, by tag.
pub type SelfContainedAxes = BTreeMap<String, f64>;
/// The reachable (lo, hi) range of each axis, in old-space final coordinates.
pub type ReachableRanges = BTreeMap<String, (f64, f64)>;
/// `(a_i, d_i, b_i)`: the old intermediate values at the new minimum, default and
/// maximum.
pub type Intermediates = BTreeMap<String, (f64, f64, f64)>;

fn tag_of(text: &str) -> Result<Tag> {
	Tag::from_str_padded(text).ok_or_else(|| Error::value(format!("invalid axis tag {text:?}")))
}

fn index_error() -> Error {
	Error::ttlib("IndexError: list index out of range")
}

pub fn valid_avar_segment_map(tag: &str, map: &SegmentMap) -> bool {
	if map.is_empty() {
		return true;
	}
	for (from, to) in [(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)] {
		if !map.iter().any(|&(f, t)| f == from && t == to) {
			log::warn!(
				"Invalid avar SegmentMap record for axis '{tag}': does not include all required value maps {{-1.0: -1.0, 0: 0, 1.0: 1.0}}"
			);
			return false;
		}
	}
	let mut sorted = map.clone();
	sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
	let mut previous = None;
	for &(from, to) in &sorted {
		if previous.is_some_and(|p| p > to) {
			log::warn!(
				"Invalid avar AxisValueMap({from}, {to}) record for axis '{tag}': the toCoordinate value must be >= to the toCoordinate value of the preceding record ({previous:?})."
			);
			return false;
		}
		previous = Some(to);
	}
	true
}

/// `instantiateAvar`: the final coordinates of the pinned axes that can be removed
/// from `fvar`.
pub fn instantiate_avar(
	font: &mut Font,
	limits: &UserLimits,
	normalized: &NormalizedLimits,
	old_intermediates: Option<&Intermediates>,
) -> Result<SelfContainedAxes> {
	let pinned = limits.pinned_location();
	let axes = fvar_axes(font)?;
	let ranges = limits.normalize(font, false)?;
	let computed;
	let avar = font.require::<Avar>()?;
	let tag_text = |t: &Tag| t.to_string();
	// Capture the old segment maps before the renormalization below.
	let old_segments = avar.segments.clone();
	// An avar version 2 font is never downgraded to version 1.
	let version = avar.major_version;
	let is_avar2 = version >= 2;

	// Drop the table if all the axes are instantiated. An avar2 store may drive axes
	// that have no segment map, so those fonts require all the axes to be pinned.
	if axes.iter().all(|(t, _)| pinned.contains_key(t))
		|| (!is_avar2
			&& avar
				.segments
				.keys()
				.all(|t| pinned.contains_key(&tag_text(t))))
	{
		log::info!("Dropping avar table");
		font.remove(AVAR);
		return Ok(SelfContainedAxes::new());
	}

	let old_intermediates = match old_intermediates {
		Some(old) => Some(old),
		None if is_avar2 => {
			computed = compute_old_intermediates(font, limits)?;
			Some(&computed)
		}
		None => None,
	};
	let avar = font.require::<Avar>()?;

	log::info!("Instantiating avar table");
	if is_avar2 {
		// Keep identity segments for pinned axes that will be hidden in fvar: compiling
		// needs segment entries for all the axes. Those of self-contained axes will be
		// removed when they're removed from fvar.
		for (tag, mapping) in &mut avar.segments {
			if pinned.contains_key(&tag_text(tag)) {
				*mapping = vec![(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)];
			}
		}
	} else {
		avar.segments
			.retain(|t, _| !pinned.contains_key(&tag_text(t)));
	}

	let mut new_segments = BTreeMap::new();
	for (tag, mapping) in &avar.segments {
		let text = tag_text(tag);
		if !valid_avar_segment_map(&text, mapping) {
			continue;
		}
		let range = ranges.0.get(&text);
		let Some(range) = range.filter(|_| !mapping.is_empty()) else {
			new_segments.insert(*tag, mapping.clone());
			continue;
		};
		let quantize = |v: f64| float_to_fixed_to_float(v, 14);
		let mapped = NormalizedAxisTriple::new(
			quantize(piecewise_linear_map(range.minimum, mapping)),
			quantize(piecewise_linear_map(range.default, mapping)),
			quantize(piecewise_linear_map(range.maximum, mapping)),
			range.distance_negative,
			range.distance_positive,
		)?;
		let mut new_mapping: SegmentMap = Vec::new();
		let mut set = |from: f64, to: f64| match new_mapping.iter_mut().find(|e| e.0 == from) {
			Some(entry) => entry.1 = to,
			None => new_mapping.push((from, to)),
		};
		for &(from, to) in mapping {
			if from < range.minimum || from > range.maximum {
				continue;
			}
			let from = range.renormalize_value(from);
			if !(mapped.minimum <= to && to <= mapped.maximum) {
				return Err(Error::Assertion("mappedMin <= toCoord <= mappedMax".into()));
			}
			let to = mapped.renormalize_value(to);
			set(quantize(from), quantize(to));
		}
		for v in [-1.0, 0.0, 1.0] {
			set(v, v);
		}
		new_segments.insert(*tag, new_mapping);
	}
	avar.segments = new_segments;

	if version == 1 {
		return Ok(SelfContainedAxes::new());
	}
	if version != 2 {
		return Err(Error::Assertion("version == 2".into()));
	}
	let old_intermediates = old_intermediates.ok_or_else(index_error)?;
	instantiate_avar_v2(font, limits, normalized, old_intermediates, &old_segments)
}

/// `_computeOldIntermediates`: the old intermediate coordinate values (post-fvar
/// normalization + avar v1 mapping) at the new min, default, and max user-space values.
pub(super) fn compute_old_intermediates(
	font: &mut Font,
	limits: &UserLimits,
) -> Result<Intermediates> {
	let axes = fvar_axes(font)?;
	let avar = font.require::<Avar>()?;
	let mut result = Intermediates::new();
	for (tag, triple) in axes {
		let Some(limit) = limits.0.get(&tag) else {
			continue;
		};
		let mapping = avar.segments.get(&tag_of(&tag)?).map(Vec::as_slice);
		result.insert(
			tag,
			(
				normalize(limit.minimum, triple, mapping)?,
				normalize(limit.default, triple, mapping)?,
				normalize(limit.maximum, triple, mapping)?,
			),
		);
	}
	Ok(result)
}

/// Above this residual, in F2Dot14 units, a restricted axis triggers a warning.
const AVAR2_OFFSET_WARN_THRESHOLD: i64 = 8;

/// `_estimateAvar2OffsetError`: an estimate of the residual offset-compensation error
/// for a restricted axis, in F2Dot14 units.
fn estimate_offset_error(
	old_seg: Option<&SegmentMap>,
	new_seg: Option<&SegmentMap>,
	old_triple: (f64, f64, f64),
	new_triple: (f64, f64, f64),
	offset_by_z: &[(f64, f64)],
) -> Result<i64> {
	let old_seg = old_seg.filter(|s| !s.is_empty());
	let new_seg = new_seg.filter(|s| !s.is_empty());
	let (lo, _, hi) = new_triple;
	const SAMPLES: i32 = 257;
	let mut us: Vec<f64> = (0..SAMPLES)
		.map(|i| lo + (hi - lo) * f64::from(i) / f64::from(SAMPLES - 1))
		.collect();
	let denorm_old = [
		(-1.0, old_triple.0),
		(0.0, old_triple.1),
		(1.0, old_triple.2),
	];
	let denorm_new = [
		(-1.0, new_triple.0),
		(0.0, new_triple.1),
		(1.0, new_triple.2),
	];
	if let Some(seg) = old_seg {
		us.extend(seg.iter().map(|e| piecewise_linear_map(e.0, &denorm_old)));
	}
	let mut z_to_norm: SegmentMap = Vec::new();
	if let Some(seg) = new_seg {
		us.extend(seg.iter().map(|e| piecewise_linear_map(e.0, &denorm_new)));
		// The inverse, to pull offset(z) knots back to normalized space.
		for &(k, v) in seg {
			match z_to_norm.iter_mut().find(|e| e.0 == v) {
				Some(e) => e.1 = k,
				None => z_to_norm.push((v, k)),
			}
		}
	}
	for &(z, _) in offset_by_z {
		let n = if z_to_norm.is_empty() {
			z
		} else {
			piecewise_linear_map(z, &z_to_norm)
		};
		us.push(piecewise_linear_map(n, &denorm_new));
	}

	let mut max_err = 0;
	for u in us {
		if !(lo <= u && u <= hi) {
			continue;
		}
		let n_old = normalize_value(u, old_triple, false)?;
		let old_final = old_seg.map_or(n_old, |s| piecewise_linear_map(n_old, s));
		let n_new = normalize_value(u, new_triple, false)?;
		let z = new_seg.map_or(n_new, |s| piecewise_linear_map(n_new, s));
		let new_final = z + piecewise_linear_map(z, offset_by_z);
		max_err =
			max_err.max((ot_round(old_final * 16384.0) - ot_round(new_final * 16384.0)).abs());
	}
	Ok(max_err)
}

/// `varIdxMap[i]`, or the implicit identity mapping.
fn axis_var_idx(map: Option<&DeltaSetIndexMap>, axis: usize) -> u64 {
	match map {
		Some(map) => map.mapping.get(axis).copied().unwrap_or(NO_VARIATION_INDEX),
		None => axis as u64,
	}
}

/// `_effectiveAvar2VarIdx`: the index an axis actually resolves to. Entries that point
/// outside the store behave as "no variation" at runtime; this also covers a NULL store.
fn effective_var_idx(
	store: Option<&ot::VarStore>,
	map: Option<&DeltaSetIndexMap>,
	axis: usize,
) -> u64 {
	let var_idx = axis_var_idx(map, axis);
	let Some(store) = store.filter(|_| var_idx != NO_VARIATION_INDEX) else {
		return NO_VARIATION_INDEX;
	};
	let (outer, inner) = ((var_idx >> 16) as usize, var_idx & 0xFFFF);
	match store.var_data.get(outer) {
		Some(Some(data)) => {
			let count = data.item_count.map_or(data.item.len() as u64, u64::from);
			if inner >= count {
				NO_VARIATION_INDEX
			} else {
				var_idx
			}
		}
		_ => NO_VARIATION_INDEX,
	}
}

/// `builder.buildDeltaSetIndexMap`: `None` for the identity mapping.
fn build_delta_set_index_map(mapping: Vec<u64>) -> Option<Box<DeltaSetIndexMap>> {
	crate::builder::build_delta_set_index_map(mapping).map(Box::new)
}

fn coordinate_is_nonzero(var: &TupleVariation, inner: usize) -> Result<bool> {
	match var.coordinates.get(inner) {
		Some(Some(Delta::Value(n))) => Ok(n.value() != 0.0),
		Some(_) => Ok(true),
		None => Err(index_error()),
	}
}

/// A delta row with the value at one item.
fn single_coordinate(count: usize, inner: usize, value: i64) -> Result<Vec<Option<Delta>>> {
	let mut coordinates = vec![Some(Delta::Value(Number::Int(0))); count];
	*coordinates.get_mut(inner).ok_or_else(index_error)? = Some(Delta::Value(Number::Int(value)));
	Ok(coordinates)
}

/// `_instantiateAvarV2`: rebases the regions of the store, and adds offset compensation
/// entries (an empty-region bias and tent deltas) for the axes that aren't self-contained,
/// so that the final coordinates remain in the old space.
#[allow(clippy::too_many_lines)]
fn instantiate_avar_v2(
	font: &mut Font,
	limits: &UserLimits,
	normalized: &NormalizedLimits,
	old_intermediates: &Intermediates,
	old_segments: &BTreeMap<Tag, SegmentMap>,
) -> Result<SelfContainedAxes> {
	let axes = fvar_axes(font)?;
	let tags: Vec<String> = axes.iter().map(|a| a.0.clone()).collect();
	let avar = font.require::<Avar>()?;
	let num_axes = axes.len();

	// A compiled map may be stored truncated. It is written into by axis index below,
	// so pad it back to full length first, with what reading it would have given.
	let mut var_idx_map = avar.table.var_idx_map.take();
	if let Some(map) = &mut var_idx_map
		&& map.mapping.len() < num_axes
	{
		let fill = map.mapping.last().copied().unwrap_or(NO_VARIATION_INDEX);
		map.mapping.resize(num_axes, fill);
	}

	// Normalize mapping entries that don't resolve to a real store row to
	// NO_VARIATION_INDEX, mirroring runtime behavior. With a NULL store, start from an
	// empty one: offset compensation creates delta rows on demand.
	let store = avar.table.var_store.take();
	let raw: Vec<u64> = (0..num_axes)
		.map(|i| axis_var_idx(var_idx_map.as_deref(), i))
		.collect();
	let effective: Vec<u64> = (0..num_axes)
		.map(|i| effective_var_idx(store.as_deref(), var_idx_map.as_deref(), i))
		.collect();
	if effective != raw {
		match &mut var_idx_map {
			Some(map) => map.mapping = effective,
			None => var_idx_map = build_delta_set_index_map(effective),
		}
	}
	let store = match store {
		Some(store) => store,
		None => Box::new(build_var_store(
			build_var_region_list(&[], &tags)?,
			Vec::new(),
		)),
	};

	// Step 1: Convert the store to tuple variations and rebase them.
	let mut tuple_store = TupleVarStore::from_item_var_store(&store, &tags)?;
	let original_axis_order = tuple_store.axis_order.clone();
	let mut default_deltas = tuple_store.instantiate(normalized, true)?;
	// For avar2, keep all the fvar axes.
	tuple_store.axis_order = original_axis_order;

	// Step 1.5: Detect self-contained pinned axes. A pinned axis is self-contained if
	// its final coordinate doesn't vary with any remaining axes.
	let mut self_contained = SelfContainedAxes::new();
	let pinned = limits.pinned_location();
	let clamp = |v: i64| v.clamp(-(1 << 14), 1 << 14);
	for (axis_index, (tag, _)) in axes.iter().enumerate() {
		if !pinned.contains_key(tag) {
			continue;
		}
		let var_idx = axis_var_idx(var_idx_map.as_deref(), axis_index);
		let intermediate = old_intermediates
			.get(tag)
			.ok_or_else(|| Error::ttlib(format!("KeyError: {tag:?}")))?;
		if var_idx == NO_VARIATION_INDEX {
			// No store entry: the final coordinate is the intermediate one.
			let final_int = clamp(ot_round(intermediate.1 * 16384.0));
			self_contained.insert(tag.clone(), final_int as f64 / 16384.0);
			continue;
		}
		let (outer, inner) = ((var_idx >> 16) as usize, (var_idx & 0xFFFF) as usize);
		let Some(variations) = tuple_store.tuple_var_data.get(outer) else {
			continue;
		};
		let mut has_delta = false;
		for var in variations {
			if coordinate_is_nonzero(var, inner)? {
				has_delta = true;
				break;
			}
		}
		if !has_delta {
			// Self-contained: final coord = intermediate + default delta.
			let d_int = ot_round(intermediate.1 * 16384.0);
			let default = default_deltas[outer][inner];
			let final_int = clamp(d_int + ot_round(default));
			self_contained.insert(tag.clone(), final_int as f64 / 16384.0);
		}
	}
	if !self_contained.is_empty() {
		log::info!("Self-contained pinned axes (removable): {self_contained:?}");
	}

	// Step 1.75: Privatize shared delta rows before adding offset compensation. The map
	// may map several axes to the same row; writing an axis's offset compensation deltas
	// into a shared row would corrupt every other axis that reads that row.
	let mut ref_count: HashMap<u64, i64> = HashMap::new();
	for i in 0..num_axes {
		*ref_count
			.entry(axis_var_idx(var_idx_map.as_deref(), i))
			.or_insert(0) += 1;
	}
	for (axis_index, (tag, _)) in axes.iter().enumerate() {
		// Only axes that will receive offset compensation can contaminate a shared row.
		if !limits.0.contains_key(tag) || self_contained.contains_key(tag) {
			continue;
		}
		let var_idx = axis_var_idx(var_idx_map.as_deref(), axis_index);
		if var_idx == NO_VARIATION_INDEX {
			continue; // gets a fresh, private VarData in the offset loop below
		}
		if ref_count.get(&var_idx).copied().unwrap_or(0) <= 1 {
			continue; // sole owner: safe to write offsets in place
		}
		let (outer, inner) = ((var_idx >> 16) as usize, (var_idx & 0xFFFF) as usize);
		let new_inner = tuple_store.item_counts[outer];
		for var in &mut tuple_store.tuple_var_data[outer] {
			let value = *var.coordinates.get(inner).ok_or_else(index_error)?;
			var.coordinates.push(value);
		}
		tuple_store.item_counts[outer] += 1;
		let default = *default_deltas[outer].get(inner).ok_or_else(index_error)?;
		default_deltas[outer].push(default);
		let new_var_idx = ((outer as u64) << 16) | new_inner as u64;
		// A shared index implies an explicit map.
		let map = var_idx_map
			.as_mut()
			.ok_or_else(|| Error::Assertion("varIdxMap is not None".into()))?;
		map.mapping[axis_index] = new_var_idx;
		*ref_count.entry(var_idx).or_insert(0) -= 1;
		*ref_count.entry(new_var_idx).or_insert(0) += 1;
	}

	// Step 2: Add offset compensation tuple variations.
	let mut processed = std::collections::HashSet::new();
	let mut approx_warn = BTreeMap::new();
	let new_segments = font.require::<Avar>()?.segments.clone();
	for (axis_index, (tag, fvar_triple)) in axes.iter().enumerate() {
		let mut var_idx = axis_var_idx(var_idx_map.as_deref(), axis_index);
		let Some(limit) = limits.0.get(tag) else {
			// A free or private axis, not being restricted. A non-zero default delta from
			// rebasing the restricted axes' contributions is added back as a bias: the
			// standard instancing subtracts default deltas, but here they must stay. Skip
			// if this index was already processed (axes can share it).
			if var_idx == NO_VARIATION_INDEX || !processed.insert(var_idx) {
				continue;
			}
			let (outer, inner) = ((var_idx >> 16) as usize, (var_idx & 0xFFFF) as usize);
			let Some(row) = default_deltas.get(outer) else {
				continue;
			};
			let default = ot_round(*row.get(inner).ok_or_else(index_error)?);
			if default != 0 {
				let count = tuple_store.item_counts[outer];
				tuple_store.tuple_var_data[outer].push(TupleVariation {
					axes: BTreeMap::new(),
					coordinates: single_coordinate(count, inner, default)?,
				});
			}
			continue;
		};
		if self_contained.contains_key(tag) {
			// Skip offset compensation: its contribution will be folded into gvar/HVAR/etc.
			continue;
		}
		// This axis is being restricted or pinned (non-self-contained).
		let &(a_i, d_i, b_i) = old_intermediates
			.get(tag)
			.ok_or_else(|| Error::ttlib(format!("KeyError: {tag:?}")))?;
		let is_pinned = limit.minimum == limit.maximum;
		if var_idx == NO_VARIATION_INDEX {
			// This axis has no existing mapping. Create a new VarData with one item to
			// hold the offset compensation.
			let outer = tuple_store.tuple_var_data.len();
			tuple_store.tuple_var_data.push(Vec::new());
			tuple_store.item_counts.push(1);
			default_deltas.push(vec![0.0]);
			var_idx = (outer as u64) << 16;
			match &mut var_idx_map {
				// Identity mapping for all axes, except this one.
				None => {
					let mut mapping: Vec<u64> = (0..num_axes as u64).collect();
					mapping[axis_index] = var_idx;
					var_idx_map = build_delta_set_index_map(mapping);
				}
				Some(map) => map.mapping[axis_index] = var_idx,
			}
		}
		let (outer, inner) = ((var_idx >> 16) as usize, (var_idx & 0xFFFF) as usize);
		let item_count = tuple_store.item_counts[outer];
		let default = *default_deltas
			.get(outer)
			.and_then(|d| d.get(inner))
			.ok_or_else(index_error)?;

		// Offset compensation encodes, as avar2 deltas on this axis, the piecewise-linear
		// function offset(z) = inv_renorm(z) - z, where inv_renorm maps a new intermediate
		// coordinate z back to the old intermediate coordinate. It is known at these
		// breakpoints in the new intermediate space:
		//   z = -1  ->  a_i + 1   (new minimum)
		//   z =  0  ->  d_i       (new default)
		//   z = +1  ->  b_i - 1   (new maximum)
		// If the axis default moved, inv_renorm also kinks where the old default lands in
		// the new space (old intermediate crosses 0), at
		//   z = z_old  ->  -z_old
		let mut offset_by_z = vec![(-1.0, a_i + 1.0), (0.0, d_i), (1.0, b_i - 1.0)];
		if !is_pinned {
			let segment_tag = tag_of(tag)?;
			let new_seg = new_segments.get(&segment_tag).filter(|s| !s.is_empty());
			let new_triple = (limit.minimum, limit.default, limit.maximum);
			// Moved-default kink: where the old default lands in new space.
			let z_old_norm = normalize_value(fvar_triple.1, new_triple, false)?;
			let z_old = new_seg.map_or(z_old_norm, |s| piecewise_linear_map(z_old_norm, s));
			let z_old = float_to_fixed_to_float(z_old, 14);
			if -1.0 < z_old && z_old < 1.0 && !offset_by_z.iter().any(|e| e.0 == z_old) {
				offset_by_z.push((z_old, -z_old));
			}

			// Interior avar v1 breakpoints inside the retained range each put a kink in
			// offset(z). The retained map keeps exactly the in-range old breakpoints; add
			// each one's output coordinate, with its old intermediate value.
			let old_seg = old_segments.get(&segment_tag).filter(|s| !s.is_empty());
			let mut with_breakpoints = offset_by_z.clone();
			if let (Some(old), Some(new)) = (old_seg, new_seg) {
				let denorm_new = [
					(-1.0, new_triple.0),
					(0.0, new_triple.1),
					(1.0, new_triple.2),
				];
				for &(from_new, z) in new {
					if matches!(from_new, -1.0 | 0.0 | 1.0) {
						continue; // anchors already seeded
					}
					let z = float_to_fixed_to_float(z, 14);
					if !(-1.0 < z && z < 1.0) || with_breakpoints.iter().any(|e| e.0 == z) {
						continue;
					}
					let user = piecewise_linear_map(from_new, &denorm_new);
					let n_old = normalize_value(user, *fvar_triple, false)?;
					let x_old = piecewise_linear_map(n_old, old);
					with_breakpoints.push((z, float_to_fixed_to_float(x_old, 14) - z));
				}
			}

			// Extra tents cost F2Dot14 rounding, so for a steep segment they can add more
			// quantization noise than the structural error they remove. Keep the interior
			// breakpoints only when they do not increase the estimated residual.
			let err_anchors =
				estimate_offset_error(old_seg, new_seg, *fvar_triple, new_triple, &offset_by_z)?;
			let approx_err = if with_breakpoints.len() == offset_by_z.len() {
				err_anchors
			} else {
				let err_with = estimate_offset_error(
					old_seg,
					new_seg,
					*fvar_triple,
					new_triple,
					&with_breakpoints,
				)?;
				if err_with <= err_anchors {
					offset_by_z = with_breakpoints;
					err_with
				} else {
					err_anchors
				}
			};
			if approx_err > AVAR2_OFFSET_WARN_THRESHOLD {
				approx_warn.insert(tag.clone(), approx_err);
			}
		}

		if is_pinned {
			// A pinned axis has zero range: only the constant bias applies.
			let bias = ot_round(d_i * 16384.0) + ot_round(default);
			if bias != 0 {
				tuple_store.tuple_var_data[outer].push(TupleVariation {
					axes: BTreeMap::new(),
					coordinates: single_coordinate(item_count, inner, bias)?,
				});
			}
		} else {
			offset_by_z.sort_by(|a, b| a.0.total_cmp(&b.0));
			let locations: Vec<Vec<(String, f64)>> = offset_by_z
				.iter()
				.map(|e| vec![(tag.clone(), e.0)])
				.collect();
			let model =
				VariationModel::new(&locations, Some(std::slice::from_ref(tag)), false, None)?;
			let values: Vec<f64> = offset_by_z.iter().map(|e| e.1).collect();
			let deltas = model.get_deltas(&values)?;
			for (delta, support) in deltas.iter().zip(&model.supports) {
				let mut di = ot_round(delta * 16384.0);
				if support.is_empty() {
					// Base (empty) region: fold in the rebased default delta.
					di += ot_round(default);
				}
				if di != 0 {
					tuple_store.tuple_var_data[outer].push(TupleVariation {
						axes: support.iter().cloned().collect(),
						coordinates: single_coordinate(item_count, inner, di)?,
					});
				}
			}
		}
	}
	if !approx_warn.is_empty() {
		log::warn!(
			"avar2 partial instancing is approximate (not bit-exact) for axes {:?}: the avar v1 segment map has interior breakpoint(s) in the retained range that, after a moved/edge default, require a steep F2Dot14 segment the offset compensation cannot fully reproduce. Max residual: {approx_warn:?} F2Dot14 units.",
			approx_warn.keys().collect::<Vec<_>>()
		);
	}

	// Step 3: Remove self-contained axes from the axis order: the region list must match
	// the post-removal fvar axis count.
	tuple_store
		.axis_order
		.retain(|t| !self_contained.contains_key(t));

	// Step 4: Rebuild regions and convert back to an item variation store.
	tuple_store.rebuild_regions();
	let mut new_store = tuple_store.as_item_var_store(true)?;

	// Step 5: Optimize the store (merge regions, remove zero deltas).
	let var_idx_mapping = optimize(&mut new_store, false)?;
	match &mut var_idx_map {
		Some(map) => {
			for var_idx in &mut map.mapping {
				*var_idx = var_idx_mapping.get(var_idx).copied().unwrap_or(*var_idx);
			}
		}
		None => {
			if !var_idx_mapping.is_empty() {
				// There was no map (implicit identity). Create one if the mapping is no
				// longer identity.
				var_idx_map = build_delta_set_index_map(
					(0..num_axes as u64)
						.map(|i| var_idx_mapping.get(&i).copied().unwrap_or(i))
						.collect(),
				);
			}
		}
	}
	let avar = font.require::<Avar>()?;
	avar.table.var_idx_map = var_idx_map;
	avar.table.var_store = Some(Box::new(new_store));
	Ok(self_contained)
}

/// `_isTupleVariationDead`: whether a variation is entirely outside the reachable ranges.
pub fn is_dead(var: &TupleVariation, ranges: &ReachableRanges) -> bool {
	ranges.iter().any(|(tag, &(lo, hi))| {
		// Strict comparisons: a tent whose peak sits exactly on the boundary is still live.
		var.axes
			.get(tag)
			.is_some_and(|&(start, _, end)| hi < start || lo > end)
	})
}

/// `_cullItemVariationStore`: removes dead regions from an item variation store.
fn cull_item_variation_store(
	store: &mut ot::VarStore,
	axes: &[String],
	ranges: &ReachableRanges,
) -> Result<usize> {
	let mut tuple_store = TupleVarStore::from_item_var_store(store, axes)?;
	let total = cull_tuple_var_store(&mut tuple_store, ranges);
	if total != 0 {
		tuple_store.rebuild_regions();
		let new_store = tuple_store.as_item_var_store(true)?;
		store.var_region_list = new_store.var_region_list;
		store.var_data = new_store.var_data;
	}
	Ok(total)
}

/// The culling of `_cullItemVariationStore`, without building a store: the number of
/// variations removed.
pub(super) fn cull_tuple_var_store(store: &mut TupleVarStore, ranges: &ReachableRanges) -> usize {
	let mut total = 0;
	for variations in &mut store.tuple_var_data {
		let before = variations.len();
		variations.retain(|v| !is_dead(v, ranges));
		total += before - variations.len();
	}
	total
}

/// `_cullVariationsForAvar2`: culls dead variations outside the reachable old-space
/// final-coord ranges. `gvar` and `cvar` lose tuple variations; tables with an item
/// variation store lose region columns.
pub fn cull_variations(font: &mut Font, ranges: &ReachableRanges) -> Result<()> {
	let axes: Vec<String> = fvar_axes(font)?.into_iter().map(|a| a.0).collect();
	if font.has::<Gvar>() {
		for (_, variations) in &mut font.require::<Gvar>()?.variations {
			variations.retain(|v| !is_dead(v, ranges));
		}
	}
	if font.contains(Tag::new(b"cvar")) {
		font.require::<Cvar>()?
			.variations
			.retain(|v| !is_dead(v, ranges));
	}
	// A fixed tag list rather than probing every table in the font, which would force
	// decompiling unrelated tables for nothing. VARC's store is not culled here.
	macro_rules! cull {
		($ty:ty, $tag:literal) => {
			if font.contains(Tag::new($tag))
				&& let Some(store) = font.require::<$ty>()?.table.var_store.as_deref_mut()
			{
				let removed = cull_item_variation_store(store, &axes, ranges)?;
				if removed != 0 {
					log::info!("avar2 culling: removed {removed} dead region references");
				}
			}
		};
	}
	cull!(Hvar, b"HVAR");
	cull!(Vvar, b"VVAR");
	cull!(Mvar, b"MVAR");
	cull!(Gdef, b"GDEF");
	cull!(Base, b"BASE");
	if font.contains(Tag::new(b"COLR"))
		&& let Some(table) = font.require::<Colr>()?.table.as_mut()
		&& let Some(store) = table.var_store.as_deref_mut()
	{
		cull_item_variation_store(store, &axes, ranges)?;
	}
	Ok(())
}

/// `_computeReachableRangesForAvar2`: for the axes not in the ranges yet (those with store
/// entries), conservative bounds on the reachable old-space final coordinate. This is
/// critical for parametric fonts, where gvar references private axes driven by public
/// axes through avar2. Must be called after [`instantiate_fvar_for_avar2`].
pub(super) fn compute_reachable_ranges(
	font: &mut Font,
	limits: &UserLimits,
	ranges: &mut ReachableRanges,
) -> Result<()> {
	let fvar_flags: Vec<(String, u16)> = font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| (a.tag_text(), a.flags))
		.collect();
	let tags: Vec<String> = fvar_flags.iter().map(|a| a.0.clone()).collect();
	let avar = font.require::<Avar>()?;
	let Some(store) = avar.table.var_store.as_deref() else {
		return Ok(());
	};
	let map = avar.table.var_idx_map.as_deref();
	// Private axes (originally hidden, not user-restricted) always have intermediate
	// coordinate 0: any region referencing them as input gets scalar 0.
	let private_limits: BTreeMap<String, (f64, f64)> = fvar_flags
		.iter()
		.filter(|(tag, flags)| flags & 1 != 0 && !limits.0.contains_key(tag))
		.map(|(tag, _)| (tag.clone(), (0.0, 0.0)))
		.collect();
	for (axis_index, (tag, flags)) in fvar_flags.iter().enumerate() {
		if ranges.contains_key(tag) {
			continue; // Already computed (NO_VARIATION_INDEX, exact)
		}
		let var_idx = axis_var_idx(map, axis_index);
		if var_idx == NO_VARIATION_INDEX {
			continue;
		}
		// Private axes have no identity term (intermediate always 0, final = delta only).
		let identity = (flags & 1 == 0).then_some(axis_index);
		let (min, max) = get_extremes(store, var_idx, &tags, &private_limits, identity)?;
		let lo = (min as f64 / 16384.0).clamp(-1.0, 1.0);
		let hi = (max as f64 / 16384.0).clamp(-1.0, 1.0);
		if lo > hi {
			return Err(Error::Assertion("lo <= hi".into()));
		}
		ranges.insert(tag.clone(), (lo, hi));
		log::info!("avar2 getExtremes range for {tag}: [{lo:.4}, {hi:.4}]");
	}
	Ok(())
}

/// The reachable ranges of the axes without variation: the final coordinate equals the
/// intermediate coordinate.
pub(super) fn exact_reachable_ranges(
	font: &mut Font,
	limits: &UserLimits,
	old_intermediates: &Intermediates,
) -> Result<ReachableRanges> {
	let axes = fvar_axes(font)?;
	let avar = font.require::<Avar>()?;
	let store = avar.table.var_store.as_deref();
	let map = avar.table.var_idx_map.as_deref();
	let mut ranges = ReachableRanges::new();
	for (axis_index, (tag, _)) in axes.iter().enumerate() {
		let Some(limit) = limits.0.get(tag) else {
			continue;
		};
		if limit.minimum == limit.maximum {
			continue; // Pinned axis, not restricted
		}
		let Some(&(a_i, _, b_i)) = old_intermediates.get(tag) else {
			continue;
		};
		if effective_var_idx(store, map, axis_index) == NO_VARIATION_INDEX {
			ranges.insert(tag.clone(), (a_i.min(b_i), a_i.max(b_i)));
		}
	}
	Ok(ranges)
}

/// `_instantiateFvarForAvar2`: updates `fvar` for avar2 partial instancing. Self-contained
/// pinned axes are removed from `fvar`, their contributions folded into variation tables.
/// Non-self-contained pinned axes are kept as hidden axes. Restricted axes have their
/// ranges updated.
pub(super) fn instantiate_fvar_for_avar2(
	font: &mut Font,
	limits: &UserLimits,
	self_contained: &SelfContainedAxes,
) -> Result<()> {
	log::info!("Instantiating fvar table (avar2 mode)");
	let pinned = limits.pinned_location();
	let fvar = font.require::<Fvar>()?;
	let original_count = fvar.axes.len();
	let mut removed = Vec::new();
	let mut axes = Vec::with_capacity(original_count);
	for (index, mut axis) in std::mem::take(&mut fvar.axes).into_iter().enumerate() {
		let tag = axis.tag_text();
		if self_contained.contains_key(&tag) {
			removed.push(index);
			continue;
		}
		if let Some(limit) = limits.0.get(&tag) {
			axis.min_value = limit.minimum;
			axis.default_value = limit.default;
			axis.max_value = limit.maximum;
			if pinned.contains_key(&tag) {
				// Non-self-contained pinned: keep as hidden.
				axis.flags |= 1;
			}
		}
		axes.push(axis);
	}
	fvar.axes = axes;

	// Filter the named instances: keep those within the new ranges.
	let mut instances = Vec::with_capacity(fvar.instances.len());
	for mut instance in std::mem::take(&mut fvar.instances) {
		let mut keep = true;
		for (tag, value) in &pinned {
			if instance.coordinates.get(&tag_of(tag)?) != Some(value) {
				keep = false;
				break;
			}
		}
		if !keep || !super::is_instance_within_axis_ranges(&instance.coordinates, limits) {
			continue;
		}
		for tag in self_contained.keys() {
			instance.coordinates.remove(&tag_of(tag)?);
		}
		instances.push(instance);
	}
	fvar.instances = instances;

	// Update avar for the removed axes.
	if !removed.is_empty() && font.contains(AVAR) {
		let avar = font.require::<Avar>()?;
		for tag in self_contained.keys() {
			avar.segments.remove(&tag_of(tag)?);
		}
		if avar.major_version >= 2 {
			match &mut avar.table.var_idx_map {
				Some(map) => {
					let mut i = 0;
					map.mapping.retain(|_| {
						i += 1;
						!removed.contains(&(i - 1))
					});
				}
				None => {
					// Implicit identity mapping: axis i -> index i. After removing axes, an
					// explicit mapping is needed if it isn't the identity anymore.
					avar.table.var_idx_map = build_delta_set_index_map(
						(0..original_count)
							.filter(|i| !removed.contains(i))
							.map(|i| i as u64)
							.collect(),
					);
				}
			}
		}
	}
	Ok(())
}
