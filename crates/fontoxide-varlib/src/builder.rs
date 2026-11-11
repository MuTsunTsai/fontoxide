//! Port of `fontTools.varLib.builder`: building item variation stores, multi
//! variation stores, delta-set index maps and variation device tables.
//!
//! The builders that other crates also need live in
//! [`fontoxide_ttlib::ot::var_store`] and are re-exported here under the same
//! names.
//!
//! Supports are maps from axis tags to `(start, peak, end)` triples, as in
//! fontTools.

use std::collections::BTreeMap;

use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::varmap::{DeltaSetIndexMap, VarIdxMap};
use fontoxide_ttlib::{Error, Result};

pub use fontoxide_ttlib::ot::var_store::{
	build_multi_var_data, build_var_data, build_var_region, build_var_region_list, build_var_store,
	calculate_num_shorts,
};

/// A support: axis tag to `(start, peak, end)`.
pub type AxisSupport = BTreeMap<String, (f64, f64, f64)>;

/// `buildVarRegionAxis`.
pub fn build_var_region_axis(axis_support: (f64, f64, f64)) -> ot::VarRegionAxis {
	let (start_coord, peak_coord, end_coord) = axis_support;
	ot::VarRegionAxis {
		start_coord,
		peak_coord,
		end_coord,
	}
}

/// `buildSparseVarRegionAxis`.
pub fn build_sparse_var_region_axis(
	axis_index: u16,
	axis_support: (f64, f64, f64),
) -> ot::SparseVarRegionAxis {
	let (start_coord, peak_coord, end_coord) = axis_support;
	ot::SparseVarRegionAxis {
		axis_index,
		start_coord,
		peak_coord,
		end_coord,
	}
}

fn check_axis_tags(support: &AxisSupport, axis_tags: &[String]) -> Result<()> {
	match support.keys().find(|t| !axis_tags.contains(t)) {
		Some(unknown) => Err(Error::Assertion(format!(
			"Unknown axis tag found: {unknown:?}"
		))),
		None => Ok(()),
	}
}

/// `buildSparseVarRegion`: only the axes of the support, by their index in
/// `axis_tags`.
pub fn build_sparse_var_region(
	support: &AxisSupport,
	axis_tags: &[String],
) -> Result<ot::SparseVarRegion> {
	check_axis_tags(support, axis_tags)?;
	let mut axes = Vec::new();
	for (i, tag) in axis_tags.iter().enumerate() {
		let Some(&axis_support) = support.get(tag) else {
			continue;
		};
		let index = u16::try_from(i).map_err(|_| Error::value("axis index out of range"))?;
		axes.push(Some(Box::new(build_sparse_var_region_axis(
			index,
			axis_support,
		))));
	}
	Ok(ot::SparseVarRegion {
		sparse_region_count: Some(axes.len() as u32),
		sparse_var_region_axis: axes,
	})
}

/// `buildSparseVarRegionList`. fontTools also sets a `RegionAxisCount`
/// attribute, which the table does not have.
pub fn build_sparse_var_region_list(
	supports: &[AxisSupport],
	axis_tags: &[String],
) -> Result<ot::SparseVarRegionList> {
	let region = supports
		.iter()
		.map(|s| build_sparse_var_region(s, axis_tags).map(|r| Some(Box::new(r))))
		.collect::<Result<Vec<_>>>()?;
	Ok(ot::SparseVarRegionList {
		region_count: Some(region.len() as u32),
		region,
	})
}

/// `VarData.optimize`: `calculateNumShorts(optimize=True)`.
pub fn var_data_optimize(data: &mut ot::VarData) {
	calculate_num_shorts(data, true);
}

/// `buildVarData`, with its assertion that each item has a delta per region,
/// which [`build_var_data`] does not check.
pub fn build_var_data_checked(
	var_region_indices: Vec<u16>,
	items: Vec<Vec<i64>>,
	optimize: bool,
) -> Result<ot::VarData> {
	if items
		.iter()
		.any(|item| item.len() != var_region_indices.len())
	{
		return Err(Error::Assertion("len(item) == regionCount".into()));
	}
	Ok(build_var_data(var_region_indices, items, optimize))
}

/// `buildMultiVarStore`.
pub fn build_multi_var_store(
	var_region_list: ot::SparseVarRegionList,
	multi_var_data_list: Vec<ot::MultiVarData>,
) -> ot::MultiVarStore {
	ot::MultiVarStore {
		format: 1,
		sparse_var_region_list: Some(Box::new(var_region_list)),
		multi_var_data_count: Some(multi_var_data_list.len() as u32),
		multi_var_data: multi_var_data_list
			.into_iter()
			.map(|d| Some(Box::new(d)))
			.collect(),
	}
}

/// `buildVarIdxMap`: the variation index of each glyph. fontTools takes glyph
/// names; [`VarIdxMap`] is keyed by glyph ID, so this takes the IDs. Like `zip`,
/// it stops at the shorter of the two.
pub fn build_var_idx_map(
	var_idxes: impl IntoIterator<Item = u64>,
	glyph_order: impl IntoIterator<Item = u32>,
) -> VarIdxMap {
	VarIdxMap {
		mapping: glyph_order.into_iter().zip(var_idxes).collect(),
	}
}

/// `buildDeltaSetIndexMap`: `None` for the identity mapping.
pub fn build_delta_set_index_map(
	var_idxes: impl IntoIterator<Item = u64>,
) -> Option<DeltaSetIndexMap> {
	let mapping: Vec<u64> = var_idxes.into_iter().collect();
	if mapping.iter().enumerate().all(|(i, &v)| i as u64 == v) {
		return None;
	}
	Some(DeltaSetIndexMap {
		format: Some(u16::from(mapping.len() > 0xFFFF)),
		mapping,
	})
}

/// `buildVarDevTable`: a `Device` table that refers to a variation index
/// (`DeltaFormat` 0x8000, outer index in `StartSize`, inner in `EndSize`).
pub fn build_var_dev_table(var_idx: u32) -> ot::Device {
	ot::Device {
		start_size: (var_idx >> 16) as u16,
		end_size: (var_idx & 0xFFFF) as u16,
		delta_format: 0x8000,
		delta_value: Vec::new(),
	}
}
