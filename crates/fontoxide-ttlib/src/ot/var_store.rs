//! Operations on item variation stores, from `fontTools.varLib.builder` and
//! `fontTools.varLib.varStore`. They are here because both `fontoxide-subset` and
//! `fontoxide-varlib` need them.

#![allow(clippy::type_complexity)]

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};

use fontoxide_misc::round_tools::ot_round;

use crate::generated::ot;
use crate::ot::varc::NO_VARIATION_INDEX;
use crate::{Error, Result};

fn index_error() -> Error {
	Error::ttlib("IndexError: list index out of range")
}

/// The bit length of `i`, but 7 for -128 and 16 for -65536 (the magic of
/// `calculateNumShorts`).
fn bit_length(i: i64) -> u32 {
	let v = i + i64::from(i < -1);
	64 - v.unsigned_abs().leading_zeros()
}

/// `VarData.calculateNumShorts`. With `optimize`, orders the columns from the
/// widest to the narrowest, dropping the ones that are all zeros.
pub fn calculate_num_shorts(data: &mut ot::VarData, optimize: bool) {
	let count = data
		.var_region_count
		.map_or(data.var_region_index.len(), |c| c as usize);
	let mut bit_lengths = vec![0u32; count];
	for item in &data.item {
		for (b, &i) in bit_lengths.iter_mut().zip(item) {
			*b = (*b).max(bit_length(i));
		}
	}
	let mut byte_lengths: Vec<u32> = bit_lengths
		.iter()
		.map(|&b| if b != 0 { (b + 8) >> 3 } else { 0 })
		.collect();
	// https://github.com/fonttools/fonttools/issues/2279
	let long_words = byte_lengths.iter().any(|&b| b > 2);

	if optimize {
		// Reorder columns such that wider columns come before narrower columns.
		let mut mapping: Vec<usize> = Vec::new();
		mapping.extend((0..byte_lengths.len()).filter(|&i| byte_lengths[i] > 2));
		mapping.extend((0..byte_lengths.len()).filter(|&i| byte_lengths[i] == 2));
		mapping.extend((0..byte_lengths.len()).filter(|&i| byte_lengths[i] == 1));
		byte_lengths = mapping.iter().map(|&i| byte_lengths[i]).collect();
		data.var_region_index = mapping
			.iter()
			.filter_map(|&i| data.var_region_index.get(i).copied())
			.collect();
		for item in &mut data.item {
			*item = mapping
				.iter()
				.filter_map(|&i| item.get(i).copied())
				.collect();
		}
	}

	let last = |min: u32| {
		byte_lengths
			.iter()
			.rposition(|&b| b > min)
			.map_or(0, |i| i + 1) as u16
	};
	data.num_shorts = if long_words {
		last(2) | 0x8000
	} else {
		last(1)
	};
	data.var_region_count = Some(data.var_region_index.len() as u32);
}

/// `buildVarData`.
pub fn build_var_data(
	var_region_indices: Vec<u16>,
	items: Vec<Vec<i64>>,
	optimize: bool,
) -> ot::VarData {
	let mut data = ot::VarData {
		var_region_count: Some(var_region_indices.len() as u32),
		var_region_index: var_region_indices,
		item_count: Some(items.len() as u32),
		item: items,
		..Default::default()
	};
	calculate_num_shorts(&mut data, optimize);
	data
}

/// `buildMultiVarData`.
pub fn build_multi_var_data(
	var_region_indices: Vec<u16>,
	items: Vec<Vec<i64>>,
) -> Result<ot::MultiVarData> {
	let region_count = var_region_indices.len();
	for item in &items {
		let valid = if region_count != 0 {
			item.len().is_multiple_of(region_count)
		} else {
			item.is_empty()
		};
		if !valid {
			return Err(Error::Assertion(
				"(len(item) % regionCount == 0) if regionCount else not item".into(),
			));
		}
	}
	Ok(ot::MultiVarData {
		format: 1,
		var_region_count: Some(region_count as u32),
		var_region_index: var_region_indices,
		item: items,
	})
}

/// `buildVarRegion`.
pub fn build_var_region(
	support: &BTreeMap<String, (f64, f64, f64)>,
	axis_tags: &[String],
) -> Result<ot::VarRegion> {
	if let Some(unknown) = support.keys().find(|t| !axis_tags.contains(t)) {
		return Err(Error::Assertion(format!(
			"Unknown axis tag found: {unknown:?}"
		)));
	}
	Ok(ot::VarRegion {
		var_region_axis: axis_tags
			.iter()
			.map(|tag| {
				let &(start_coord, peak_coord, end_coord) =
					support.get(tag).unwrap_or(&(0.0, 0.0, 0.0));
				ot::VarRegionAxis {
					start_coord,
					peak_coord,
					end_coord,
				}
			})
			.collect(),
	})
}

/// `buildVarRegionList`.
pub fn build_var_region_list(
	supports: &[BTreeMap<String, (f64, f64, f64)>],
	axis_tags: &[String],
) -> Result<ot::VarRegionList> {
	let region = supports
		.iter()
		.map(|s| build_var_region(s, axis_tags))
		.collect::<Result<Vec<_>>>()?;
	Ok(ot::VarRegionList {
		region_axis_count: Some(axis_tags.len() as u32),
		region_count: Some(region.len() as u32),
		region,
	})
}

/// `buildVarStore`.
pub fn build_var_store(region_list: ot::VarRegionList, var_data: Vec<ot::VarData>) -> ot::VarStore {
	ot::VarStore {
		format: 1,
		var_region_list: Some(Box::new(region_list)),
		var_data_count: Some(var_data.len() as u32),
		var_data: var_data.into_iter().map(|d| Some(Box::new(d))).collect(),
	}
}

/// `VarRegion.get_support`: the region of each axis that takes part.
pub fn region_support(
	region: &ot::VarRegion,
	axis_tags: &[String],
) -> Result<BTreeMap<String, (f64, f64, f64)>> {
	let mut support = BTreeMap::new();
	for (i, axis) in region.var_region_axis.iter().enumerate() {
		if axis.peak_coord != 0.0 {
			let tag = axis_tags.get(i).ok_or_else(index_error)?;
			support.insert(
				tag.clone(),
				(axis.start_coord, axis.peak_coord, axis.end_coord),
			);
		}
	}
	Ok(support)
}

/// `VarStore.prune_regions`: drops the regions that no data uses.
pub fn prune_regions(store: &mut ot::VarStore) -> Result<()> {
	// Collect.
	let mut used = BTreeSet::new();
	for data in store.var_data.iter().flatten() {
		used.extend(data.var_region_index.iter().copied());
	}
	// Subset.
	let list = store.var_region_list.as_mut().ok_or_else(|| {
		Error::ttlib("AttributeError: 'NoneType' object has no attribute 'Region'")
	})?;
	let mut map = BTreeMap::new();
	let mut regions = Vec::with_capacity(used.len());
	for &i in &used {
		map.insert(i, regions.len() as u16);
		regions.push(
			list.region
				.get(usize::from(i))
				.cloned()
				.ok_or_else(index_error)?,
		);
	}
	list.region = regions;
	list.region_count = Some(list.region.len() as u32);
	// Map.
	for data in store.var_data.iter_mut().flatten() {
		for i in &mut data.var_region_index {
			*i = map[i];
		}
	}
	Ok(())
}

/// The "characteristic" of a row: how many bytes each column takes, in four bits
/// per column. Words go from the least significant; all have the same length.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Chars(Vec<u64>);

impl Chars {
	fn or(&self, other: &Chars) -> Chars {
		Chars(self.0.iter().zip(&other.0).map(|(a, b)| a | b).collect())
	}

	fn count_ones(&self) -> u32 {
		self.0.iter().map(|w| w.count_ones()).sum()
	}

	/// The columns with any bit set.
	fn columns(&self) -> u32 {
		self.0
			.iter()
			.map(|w| {
				// A nibble of the word is not zero.
				let mut count = 0;
				for shift in (0..64).step_by(4) {
					if (w >> shift) & 0xF != 0 {
						count += 1;
					}
				}
				count
			})
			.sum()
	}

	/// The columns, as a bitmap.
	fn column_bits(&self) -> Vec<u64> {
		let mut bits = vec![0u64; self.0.len() * 16 / 64 + 1];
		for (word, w) in self.0.iter().enumerate() {
			for nibble in 0..16 {
				if (w >> (nibble * 4)) & 0xF != 0 {
					let column = word * 16 + nibble;
					bits[column / 64] |= 1 << (column % 64);
				}
			}
		}
		bits
	}
}

/// `_Encoding`.
#[derive(Debug)]
struct Encoding {
	chars: Chars,
	width: u32,
	column_bits: Vec<u64>,
	overhead: i64,
	items: BTreeSet<Vec<i64>>,
}

/// `_characteristic_overhead`: the bytes for this characteristic as a `VarData`:
/// 4 for the offset, 6 for the header, 2 per column.
fn overhead(columns: u32) -> i64 {
	10 + i64::from(columns) * 2
}

impl Encoding {
	fn new(chars: Chars) -> Self {
		Encoding {
			width: chars.count_ones(),
			column_bits: chars.column_bits(),
			overhead: overhead(chars.columns()),
			items: BTreeSet::new(),
			chars,
		}
	}

	/// `gain_from_merging`.
	fn gain_from_merging(&self, other: &Encoding) -> i64 {
		let combined_width = i64::from(self.chars.or(&other.chars).count_ones());
		let combined_columns: u32 = self
			.column_bits
			.iter()
			.zip(&other.column_bits)
			.map(|(a, b)| (a | b).count_ones())
			.sum();
		self.overhead + other.overhead
			- overhead(combined_columns)
			- (combined_width - i64::from(self.width)) * self.items.len() as i64
			- (combined_width - i64::from(other.width)) * other.items.len() as i64
	}

	/// `width_sort_key`: the width, then the characteristic as a number.
	fn sort_key(&self) -> (u32, Vec<u64>) {
		(self.width, self.chars.0.iter().rev().copied().collect())
	}
}

/// `_row_characteristics`.
fn row_characteristics(row: &[i64], words: usize) -> Chars {
	let mut chars = vec![0u64; words];
	let mut set = |column: usize, value: u64| {
		chars[column / 16] |= value << ((column % 16) * 4);
	};
	let long_words = row.iter().any(|v| !(-32768..=32767).contains(v));
	for (column, &v) in row.iter().enumerate() {
		if long_words {
			// Only allow 2-byte and 4-byte encodings.
			if v != 0 {
				set(column, 0b0011);
			}
			if !(-32768..=32767).contains(&v) {
				set(column, 0b1100);
			}
		} else {
			if v != 0 {
				set(column, 0b0001);
			}
			if !(-128..=127).contains(&v) {
				set(column, 0b0010);
			}
		}
	}
	Chars(chars)
}

/// `VarStore.optimize` (with `quantization=1`): optimizes the storage, and returns
/// the mapping from old variation indices to new ones.
pub fn optimize(
	store: &mut ot::VarStore,
	use_no_variation_index: bool,
) -> Result<HashMap<u64, u64>> {
	optimize_quantized(store, use_no_variation_index, 1)
}

/// `VarStore.optimize`: optimizes the storage, and returns the mapping from old
/// variation indices to new ones.
pub fn optimize_quantized(
	store: &mut ot::VarStore,
	use_no_variation_index: bool,
	quantization: i64,
) -> Result<HashMap<u64, u64>> {
	let n = store
		.var_region_list
		.as_ref()
		.ok_or_else(|| Error::ttlib("AttributeError: 'NoneType' object has no attribute 'Region'"))?
		.region
		.len();
	let words = n.div_ceil(16).max(1);

	// Collect all items into a set of full rows, with one column per region.
	let mut front_mapping: Vec<(u64, Option<Vec<i64>>)> = Vec::new();
	let mut encodings: HashMap<Chars, Encoding> = HashMap::new();
	for (major, data) in store.var_data.iter().enumerate() {
		let Some(data) = data else {
			return Err(Error::ttlib(
				"AttributeError: 'NoneType' object has no attribute 'VarRegionIndex'",
			));
		};
		for (minor, item) in data.item.iter().enumerate() {
			let mut row = vec![0i64; n];
			for (&region, &v) in data.var_region_index.iter().zip(item) {
				let v = if quantization == 1 {
					v
				} else {
					(v as f64 / quantization as f64).round_ties_even() as i64 * quantization
				};
				*row.get_mut(usize::from(region)).ok_or_else(index_error)? += v;
			}
			let index = ((major as u64) << 16) + minor as u64;
			if use_no_variation_index && row.iter().all(|v| *v == 0) {
				front_mapping.push((index, None));
				continue;
			}
			let chars = row_characteristics(&row, words);
			encodings
				.entry(chars.clone())
				.or_insert_with(|| Encoding::new(chars))
				.items
				.insert(row.clone());
			front_mapping.push((index, Some(row)));
		}
	}

	// Prepare for the main algorithm.
	let mut todo: Vec<Encoding> = encodings.into_values().collect();
	todo.sort_by_cached_key(Encoding::sort_key);
	let mut todo: Vec<Option<Encoding>> = todo.into_iter().map(Some).collect();

	// Repeatedly pick the two best encodings to combine, and combine them.
	let mut heap: BinaryHeap<Reverse<(i64, usize, usize)>> = BinaryHeap::new();
	for i in 0..todo.len() {
		for j in i + 1..todo.len() {
			if let (Some(a), Some(b)) = (&todo[i], &todo[j]) {
				let gain = a.gain_from_merging(b);
				if gain > 0 {
					heap.push(Reverse((-gain, i, j)));
				}
			}
		}
	}
	while let Some(Reverse((_, i, j))) = heap.pop() {
		if todo[i].is_none() || todo[j].is_none() {
			continue;
		}
		let (Some(a), Some(b)) = (todo[i].take(), todo[j].take()) else {
			continue;
		};
		let mut combined = Encoding::new(a.chars.or(&b.chars));
		combined.items.extend(a.items);
		combined.items.extend(b.items);
		let next = todo.len();
		#[allow(clippy::needless_range_loop)]
		for k in 0..next {
			let Some(enc) = &todo[k] else {
				continue;
			};
			// In the unlikely event that the same encoding exists already, combine it.
			if enc.chars == combined.chars {
				if let Some(enc) = todo[k].take() {
					combined.items.extend(enc.items);
				}
				continue;
			}
			let gain = combined.gain_from_merging(enc);
			if gain > 0 {
				heap.push(Reverse((-gain, k, next)));
			}
		}
		todo.push(Some(combined));
	}
	let mut encodings: Vec<Encoding> = todo.into_iter().flatten().collect();

	// Assemble the final store.
	encodings.sort_by_cached_key(Encoding::sort_key);
	let mut back_mapping: HashMap<Vec<i64>, u64> = HashMap::new();
	let mut var_data = Vec::new();
	for encoding in encodings {
		let items: Vec<Vec<i64>> = encoding.items.into_iter().collect();
		// Each major can only encode up to 0xFFFF entries.
		for chunk in items.chunks(0xFFFF) {
			let major = var_data.len() as u64;
			for (minor, item) in chunk.iter().enumerate() {
				back_mapping.insert(item.clone(), (major << 16) + minor as u64);
			}
			var_data.push(ot::VarData {
				var_region_index: (0..n as u32).map(|i| i as u16).collect(),
				var_region_count: Some(n as u32),
				item: chunk.to_vec(),
				item_count: Some(chunk.len() as u32),
				..Default::default()
			});
		}
	}

	// Compile the final mapping.
	let mut map = HashMap::with_capacity(front_mapping.len() + 1);
	map.insert(NO_VARIATION_INDEX, NO_VARIATION_INDEX);
	for (index, row) in front_mapping {
		let new = match row {
			Some(row) => *back_mapping
				.get(&row)
				.ok_or_else(|| Error::ttlib("KeyError: row"))?,
			None => NO_VARIATION_INDEX,
		};
		map.insert(index, new);
	}

	// Recalculate things and go home.
	if let Some(list) = &mut store.var_region_list {
		list.region_count = Some(list.region.len() as u32);
	}
	store.var_data_count = Some(var_data.len() as u32);
	store.var_data = var_data
		.into_iter()
		.map(|mut data| {
			calculate_num_shorts(&mut data, true);
			Some(Box::new(data))
		})
		.collect();

	// Remove unused regions.
	prune_regions(store)?;
	Ok(map)
}

/// `_tentValue`: the per-axis support-scalar factor at coordinate `v`.
fn tent_value(v: f64, start: f64, peak: f64, end: f64) -> f64 {
	if v == peak {
		1.0
	} else if v <= start || end <= v {
		0.0
	} else if v < peak {
		(v - start) / (peak - start)
	} else {
		(end - v) / (end - peak)
	}
}

/// The most grid vertices [`get_extremes`] enumerates for the exact answer, before
/// falling back to the (looser but conservative and cheap) interval bound.
const GET_EXTREMES_MAX_GRID: usize = 1 << 14;
const F2DOT14_UNIT: f64 = 1.0 / (1 << 14) as f64;

/// `VarStore.getExtremes`: conservatively bounds the value this store produces for an
/// index, in F2Dot14 delta units, as the input axes vary over `-1..+1`, or the
/// `(min, max)` from `axis_limits` for the axes listed there. With `identity_axis`, that
/// axis's own coordinate is added to the bound.
///
/// Before the final rounding, the value is piecewise multilinear in the axis coordinates,
/// so its extremes are at the vertices of the grid formed by each axis's tent breakpoints.
/// When that grid is small enough it is enumerated and the result is exact; otherwise the
/// result falls back to a per-region interval bound, which may be wider, never narrower.
pub fn get_extremes(
	store: &ot::VarStore,
	var_idx: u64,
	axes: &[String],
	axis_limits: &std::collections::BTreeMap<String, (f64, f64)>,
	identity_axis: Option<usize>,
) -> Result<(i64, i64)> {
	let index_error = || Error::ttlib("IndexError: list index out of range");
	let axis_interval = |axis: usize| -> Result<(f64, f64)> {
		let tag = axes.get(axis).ok_or_else(index_error)?;
		Ok(axis_limits.get(tag).copied().unwrap_or((-1.0, 1.0)))
	};
	// Per region: the delta, and the tents with the axes supportScalar would ignore
	// already dropped.
	let mut active: Vec<(f64, Vec<(usize, f64, f64, f64)>)> = Vec::new();
	if var_idx != super::varc::NO_VARIATION_INDEX {
		let major = (var_idx >> 16) as usize;
		let minor = (var_idx & 0xFFFF) as usize;
		let data = store
			.var_data
			.get(major)
			.and_then(|d| d.as_deref())
			.ok_or_else(index_error)?;
		let regions = &store
			.var_region_list
			.as_ref()
			.ok_or_else(|| {
				Error::ttlib("AttributeError: 'NoneType' object has no attribute 'Region'")
			})?
			.region;
		let deltas = data.item.get(minor).ok_or_else(index_error)?;
		for (&region_index, &delta) in data.var_region_index.iter().zip(deltas) {
			if delta == 0 {
				continue;
			}
			let region = regions
				.get(usize::from(region_index))
				.ok_or_else(index_error)?;
			let mut tents = Vec::new();
			for (i, axis) in region.var_region_axis.iter().enumerate() {
				let (start, peak, end) = (axis.start_coord, axis.peak_coord, axis.end_coord);
				// Mirror supportScalar's OT rules for axes it ignores.
				if peak == 0.0 || start > peak || peak > end || (start < 0.0 && end > 0.0) {
					continue;
				}
				tents.push((i, start, peak, end));
			}
			active.push((delta as f64, tents));
		}
	}

	// Try the exact path: enumerate the breakpoint grid.
	let mut breakpoints: std::collections::BTreeMap<usize, Vec<f64>> =
		std::collections::BTreeMap::new();
	let add = |list: &mut Vec<f64>, v: f64| {
		if !list.contains(&v) {
			list.push(v);
		}
	};
	if let Some(axis) = identity_axis {
		let (lo, hi) = axis_interval(axis)?;
		let list = breakpoints.entry(axis).or_default();
		add(list, lo);
		add(list, hi);
	}
	for (_, tents) in &active {
		for &(i, start, peak, end) in tents {
			let (lo, hi) = axis_interval(i)?;
			let list = breakpoints.entry(i).or_default();
			add(list, lo);
			add(list, hi);
			let clamp = |v: f64| v.max(lo).min(hi);
			for v in [start, peak, end] {
				add(list, clamp(v));
			}
			if start == peak {
				add(list, clamp(peak - F2DOT14_UNIT));
			}
			if peak == end {
				add(list, clamp(peak + F2DOT14_UNIT));
			}
		}
	}
	let mut grid_size: usize = 1;
	for points in breakpoints.values() {
		grid_size *= points.len();
		if grid_size > GET_EXTREMES_MAX_GRID {
			break;
		}
	}

	if grid_size <= GET_EXTREMES_MAX_GRID {
		// Exact: evaluate at every grid vertex.
		let axis_indices: Vec<usize> = breakpoints.keys().copied().collect();
		let lists: Vec<Vec<f64>> = axis_indices
			.iter()
			.map(|i| {
				let mut points = breakpoints[i].clone();
				points.sort_by(f64::total_cmp);
				points
			})
			.collect();
		let position = |axis: usize| axis_indices.iter().position(|&a| a == axis);
		let mut extremes: Option<(f64, f64)> = None;
		let mut counters = vec![0usize; lists.len()];
		loop {
			let vertex = |axis: usize| position(axis).map(|p| lists[p][counters[p]]);
			let mut v = 0.0;
			for (delta, tents) in &active {
				let mut scalar = 1.0;
				for &(i, start, peak, end) in tents {
					let at = vertex(i).ok_or_else(index_error)?;
					scalar *= tent_value(at, start, peak, end);
					if scalar == 0.0 {
						break;
					}
				}
				if scalar != 0.0 {
					v += delta * scalar;
				}
			}
			if let Some(axis) = identity_axis {
				v += ot_round(vertex(axis).ok_or_else(index_error)? * 16384.0) as f64;
			}
			extremes = Some(match extremes {
				None => (v, v),
				Some((min, max)) => (min.min(v), max.max(v)),
			});
			// The next vertex; the last axis changes the fastest.
			let mut k = lists.len();
			loop {
				if k == 0 {
					let (min, max) = extremes.unwrap_or((0.0, 0.0));
					return Ok((ot_round(min), ot_round(max)));
				}
				k -= 1;
				counters[k] += 1;
				if counters[k] < lists[k].len() {
					break;
				}
				counters[k] = 0;
			}
		}
	}

	// Fallback: per-region interval arithmetic. Each region's scalar range over the box
	// is exact, but correlations between regions (and with the identity term) are ignored.
	let (mut min, mut max) = (0.0, 0.0);
	for (delta, tents) in &active {
		let (mut scalar_min, mut scalar_max) = (1.0, 1.0);
		for &(i, start, peak, end) in tents {
			let (lo, hi) = axis_interval(i)?;
			// The tent rises to the peak then falls, so over the interval the maximum is
			// at the clamped peak, the minimum at an endpoint.
			scalar_max *= tent_value(peak.max(lo).min(hi), start, peak, end);
			scalar_min *= tent_value(lo, start, peak, end).min(tent_value(hi, start, peak, end));
			if scalar_max == 0.0 {
				break;
			}
		}
		if scalar_max == 0.0 {
			continue;
		}
		let (a, b): (f64, f64) = (delta * scalar_min, delta * scalar_max);
		min += a.min(b);
		max += a.max(b);
	}
	if let Some(axis) = identity_axis {
		let (lo, hi) = axis_interval(axis)?;
		min += ot_round(lo * 16384.0) as f64;
		max += ot_round(hi * 16384.0) as f64;
	}
	Ok((ot_round(min), ot_round(max)))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn store(rows: Vec<Vec<i64>>, regions: usize) -> ot::VarStore {
		let tags: Vec<String> = (0..regions).map(|i| format!("a{i:03}")).collect();
		let supports: Vec<BTreeMap<String, (f64, f64, f64)>> = (0..regions)
			.map(|i| BTreeMap::from([(tags[i].clone(), (0.0, 1.0, 1.0))]))
			.collect();
		let list = build_var_region_list(&supports, &tags).expect("regions");
		let data = build_var_data((0..regions as u16).collect(), rows, false);
		build_var_store(list, vec![data])
	}

	/// The cases of `test_optimize` of `Tests/varLib/varStore_test.py`.
	#[test]
	fn optimize_merges_and_sorts() -> Result<()> {
		let mut s = store(vec![vec![0, 0], vec![1, 0], vec![0, 200], vec![1, 0]], 2);
		let map = optimize(&mut s, true)?;
		assert_eq!(map[&0], NO_VARIATION_INDEX);
		assert_eq!(map[&1], map[&3]);
		let total: usize = s.var_data.iter().flatten().map(|d| d.item.len()).sum();
		assert_eq!(total, 2);
		assert_eq!(map[&NO_VARIATION_INDEX], NO_VARIATION_INDEX);
		Ok(())
	}

	#[test]
	fn keeps_zero_rows_without_no_variation_index() -> Result<()> {
		let mut s = store(vec![vec![0, 0], vec![1, 0]], 2);
		let map = optimize(&mut s, false)?;
		assert_ne!(map[&0], NO_VARIATION_INDEX);
		Ok(())
	}

	#[test]
	fn bit_lengths() {
		assert_eq!(bit_length(0), 0);
		assert_eq!(bit_length(-1), 1);
		assert_eq!(bit_length(127), 7);
		assert_eq!(bit_length(-128), 7);
		assert_eq!(bit_length(128), 8);
		assert_eq!(bit_length(-65536), 16);
	}

	#[test]
	fn characteristics() {
		assert_eq!(row_characteristics(&[0, 1, 200], 1).0, [0x310]);
		assert_eq!(row_characteristics(&[1, 70000], 1).0, [0xF3]);
		// Many regions.
		let mut row = vec![0; 17];
		row[16] = 1;
		assert_eq!(row_characteristics(&row, 2).0, [0, 1]);
	}
}
