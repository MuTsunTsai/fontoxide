//! Subsetting of item variation stores (`fontTools.varLib.varStore`:
//! `subset_varidxes`, `prune_regions`, the device visitors; and
//! `varLib.builder`'s `calculateNumShorts`), and the tables that use them:
//! `HVAR`, `VVAR`, and the `GDEF` store (`_pruneGDEF`).

use std::collections::{BTreeMap, BTreeSet};

use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::var_store::{calculate_num_shorts, prune_regions};
use fontoxide_ttlib::tables::{Gdef, Gpos, Gsub, Hvar, Jstf, Math, Vvar};
use fontoxide_ttlib::{Error as TtError, Font};

use crate::{Result, Subsetter};

/// `NO_VARIATION_INDEX`.
pub(crate) const NO_VARIATION_INDEX: u64 = 0xFFFF_FFFF;

fn index_error() -> TtError {
	TtError::ttlib("IndexError: list index out of range")
}

fn key_error(k: u64) -> TtError {
	TtError::ttlib(format!("KeyError: {k}"))
}

/// `VarStore.subset_varidxes`: keeps the delta sets of `var_idxes`, and returns
/// the map from their old to their new indices.
///
/// With `retain_first_map`, the delta sets of the first data keep their
/// indices, those not used being zeroed; else the delta sets of `adv_idxes` come
/// first.
pub(crate) fn subset_varidxes(
	store: &mut ot::VarStore,
	var_idxes: &BTreeSet<u64>,
	retain_first_map: bool,
	adv_idxes: &BTreeSet<u64>,
) -> Result<BTreeMap<u64, u64>> {
	// Sort out used varIdxes by major/minor.
	let mut used: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
	for &v in var_idxes {
		if v == NO_VARIATION_INDEX {
			continue;
		}
		used.entry(v >> 16).or_default().insert(v & 0xFFFF);
	}

	let mut new_var_data = Vec::new();
	let mut map = BTreeMap::from([(NO_VARIATION_INDEX, NO_VARIATION_INDEX)]);
	for (major, data) in std::mem::take(&mut store.var_data).into_iter().enumerate() {
		let major = major as u64;
		let Some(used_minors) = used.get(&major) else {
			continue;
		};
		let new_major = new_var_data.len() as u64;
		let mut data = data.ok_or_else(|| {
			TtError::ttlib("AttributeError: 'NoneType' object has no attribute 'Item'")
		})?;
		let items = std::mem::take(&mut data.item);
		let mut new_items = Vec::new();
		if major == 0 && retain_first_map {
			for (minor, item) in items.into_iter().enumerate() {
				let minor = minor as u64;
				new_items.push(if used_minors.contains(&minor) {
					item
				} else {
					vec![0; item.len()]
				});
				map.insert(minor, minor);
			}
		} else {
			let minors: Vec<u64> = if major == 0 {
				adv_idxes
					.iter()
					.copied()
					.chain(used_minors.difference(adv_idxes).copied())
					.collect()
			} else {
				used_minors.iter().copied().collect()
			};
			for minor in minors {
				let new_minor = new_items.len() as u64;
				let item = usize::try_from(minor)
					.ok()
					.and_then(|m| items.get(m))
					.ok_or_else(index_error)?;
				new_items.push(item.clone());
				map.insert((major << 16) + minor, (new_major << 16) + new_minor);
			}
		}
		data.item = new_items;
		data.item_count = Some(data.item.len() as u32);
		calculate_num_shorts(&mut data, true);
		new_var_data.push(Some(data));
	}
	store.var_data_count = Some(new_var_data.len() as u32);
	store.var_data = new_var_data;
	prune_regions(store)?;
	Ok(map)
}

/// `MultiVarStore.subset_varidxes`: `VarStore.subset_varidxes` without the
/// parts for the first data and without `calculateNumShorts`.
pub(crate) fn multi_subset_varidxes(
	store: &mut ot::MultiVarStore,
	var_idxes: &BTreeSet<u64>,
) -> Result<BTreeMap<u64, u64>> {
	let mut used: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
	for &v in var_idxes {
		if v != NO_VARIATION_INDEX {
			used.entry(v >> 16).or_default().insert(v & 0xFFFF);
		}
	}

	let mut new_var_data = Vec::new();
	let mut map = BTreeMap::from([(NO_VARIATION_INDEX, NO_VARIATION_INDEX)]);
	for (major, data) in std::mem::take(&mut store.multi_var_data)
		.into_iter()
		.enumerate()
	{
		let major = major as u64;
		let Some(used_minors) = used.get(&major) else {
			continue;
		};
		let new_major = new_var_data.len() as u64;
		let mut data = data.ok_or_else(|| {
			TtError::ttlib("AttributeError: 'NoneType' object has no attribute 'Item'")
		})?;
		let items = std::mem::take(&mut data.item);
		let mut new_items = Vec::with_capacity(used_minors.len());
		for &minor in used_minors {
			let new_minor = new_items.len() as u64;
			new_items.push(items.get(minor as usize).cloned().ok_or_else(index_error)?);
			map.insert((major << 16) + minor, (new_major << 16) + new_minor);
		}
		data.item = new_items;
		new_var_data.push(Some(data));
	}
	store.multi_var_data_count = Some(new_var_data.len() as u32);
	store.multi_var_data = new_var_data;

	// prune_regions.
	let mut used_regions = BTreeSet::new();
	for data in store.multi_var_data.iter().flatten() {
		used_regions.extend(data.var_region_index.iter().copied());
	}
	let list = store.sparse_var_region_list.as_mut().ok_or_else(|| {
		TtError::ttlib("AttributeError: 'NoneType' object has no attribute 'Region'")
	})?;
	let mut region_map = BTreeMap::new();
	let mut regions = Vec::with_capacity(used_regions.len());
	for &i in &used_regions {
		region_map.insert(i, regions.len() as u16);
		regions.push(
			list.region
				.get(usize::from(i))
				.cloned()
				.ok_or_else(index_error)?,
		);
	}
	list.region = regions;
	list.region_count = Some(list.region.len() as u32);
	for data in store.multi_var_data.iter_mut().flatten() {
		for i in &mut data.var_region_index {
			*i = region_map[i];
		}
	}
	Ok(map)
}

/// The variation indices of the `Device` tables (`DeltaFormat` 0x8000) and
/// the format 2 condition tables (`collect_device_varidxes`).
pub fn collect_device_varidxes<T: OtTable>(table: &mut T, used: &mut BTreeSet<u64>) -> Result<()> {
	table.visit_mut(&mut |x| {
		if let Some(d) = x.downcast_mut::<ot::Device>() {
			if d.delta_format == 0x8000 {
				used.insert((u64::from(d.start_size) << 16) + u64::from(d.end_size));
			}
		} else if let Some(c) = x.downcast_mut::<ot::ConditionTableFormat2>() {
			used.insert(u64::from(c.var_idx));
		}
		Ok(())
	})?;
	Ok(())
}

/// Maps the variation indices that [`collect_device_varidxes`] finds
/// (`remap_device_varidxes`).
pub fn remap_device_varidxes<T: OtTable>(table: &mut T, map: &BTreeMap<u64, u64>) -> Result<()> {
	table.visit_mut(&mut |x| {
		if let Some(d) = x.downcast_mut::<ot::Device>() {
			if d.delta_format == 0x8000 {
				let old = (u64::from(d.start_size) << 16) + u64::from(d.end_size);
				// UPSTREAM-BUG(B18): `_pruneGDEF` leaves out the indices of delta
				// sets that the store lacks, then fails to map them here.
				let new = *map.get(&old).ok_or_else(|| key_error(old))?;
				d.start_size = (new >> 16) as u16;
				d.end_size = (new & 0xFFFF) as u16;
			}
		} else if let Some(c) = x.downcast_mut::<ot::ConditionTableFormat2>() {
			let new = map
				.get(&u64::from(c.var_idx))
				.copied()
				.unwrap_or(NO_VARIATION_INDEX);
			c.var_idx = new as u32;
		}
		Ok(())
	})?;
	Ok(())
}

/// The tables whose devices use the `GDEF` store, besides `GDEF`.
macro_rules! with_device_tables {
	($font:expr, |$t:ident| $body:expr) => {{
		if let Some(x) = $font.get::<Gsub>()? {
			let $t = &mut x.table;
			$body;
		}
		if let Some(x) = $font.get::<Gpos>()? {
			let $t = &mut x.table;
			$body;
		}
		if let Some(x) = $font.get::<Math>()? {
			let $t = &mut x.table;
			$body;
		}
		if let Some(x) = $font.get::<Jstf>()? {
			let $t = &mut x.table;
			$body;
		}
	}};
}

/// `_pruneGDEF`: subsets the `GDEF` store to the variation indices that the
/// devices use.
pub(crate) fn prune_gdef(font: &mut Font) -> Result<()> {
	let Some(gdef) = font.get::<Gdef>()? else {
		return Ok(());
	};
	if gdef.table.var_store.is_none() {
		return Ok(());
	}

	// Collect.
	let mut used = BTreeSet::new();
	collect_device_varidxes(&mut gdef.table, &mut used)?;
	with_device_tables!(font, |t| collect_device_varidxes(t, &mut used)?);

	// Subset.
	let gdef = font.require::<Gdef>()?;
	let store = gdef
		.table
		.var_store
		.as_deref_mut()
		.ok_or_else(index_error)?;
	// Missing condition delta sets contribute zero, just like NO_VARIATION_INDEX.
	let mut kept = BTreeSet::new();
	for &index in &used {
		let data = usize::try_from(index >> 16)
			.ok()
			.and_then(|m| store.var_data.get(m));
		if let Some(data) = data {
			let data = data.as_deref().ok_or_else(|| {
				TtError::ttlib("AttributeError: 'NoneType' object has no attribute 'Item'")
			})?;
			if ((index & 0xFFFF) as usize) < data.item.len() {
				kept.insert(index);
			}
		}
	}
	let map = subset_varidxes(store, &kept, false, &BTreeSet::new())?;

	// Map.
	remap_device_varidxes(&mut gdef.table, &map)?;
	with_device_tables!(font, |t| remap_device_varidxes(t, &map)?);
	Ok(())
}

/// `_remap_index_map`: maps the indices of the kept glyphs; the emptied ones
/// map to 0, or to the last kept glyph's index past it.
fn remap_index_map(
	s: &Subsetter,
	map: &BTreeMap<u64, u64>,
	mapping: &BTreeMap<u32, u64>,
) -> Result<BTreeMap<u32, u64>> {
	let mut out = BTreeMap::new();
	for (&g, v) in mapping {
		out.insert(g, *map.get(v).ok_or_else(|| key_error(*v))?);
	}
	let last = s.last_retained_order;
	let last_v = mapping
		.get(&last)
		.ok_or_else(|| TtError::ttlib(format!("KeyError: {:?}", s.glyph_name(last))))?;
	let last_idx = *map.get(last_v).ok_or_else(|| key_error(*last_v))?;
	for &g in &s.glyphs_emptied {
		out.insert(g, if g > last { last_idx } else { 0 });
	}
	Ok(out)
}

/// `subset_glyphs` of `HVAR` and `VVAR`: `maps` are the advance map, then the
/// others.
fn metrics_subset_glyphs(
	s: &Subsetter,
	store: &mut Option<Box<ot::VarStore>>,
	maps: &mut [&mut Option<Box<ot::VarIdxMap>>],
) -> Result<bool> {
	let mut used = BTreeSet::new();
	let mut adv_idxes = BTreeSet::new();
	let mut retain_adv_map = false;
	for (i, m) in maps.iter_mut().enumerate() {
		match m {
			Some(m) => {
				m.mapping.retain(|g, _| s.glyphs.contains(g));
				used.extend(m.mapping.values().copied());
			}
			None if i == 0 => {
				// The advances are indexed by glyph ID.
				used.extend(s.glyphs_retained.iter().map(|&g| u64::from(g)));
				adv_idxes = used.clone();
				retain_adv_map = s.options.retain_gids;
			}
			None => {}
		}
	}
	let store = store.as_deref_mut().ok_or_else(|| {
		TtError::ttlib("AttributeError: 'NoneType' object has no attribute 'subset_varidxes'")
	})?;
	let map = subset_varidxes(store, &used, retain_adv_map, &adv_idxes)?;
	for m in maps.iter_mut().filter_map(|m| m.as_mut()) {
		m.mapping = remap_index_map(s, &map, &m.mapping)?;
	}
	// TODO Return emptiness...
	Ok(true)
}

/// `HVAR.subset_glyphs`.
pub(crate) fn hvar_subset_glyphs(s: &Subsetter, font: &mut Font) -> Result<bool> {
	let t = &mut font.require::<Hvar>()?.table;
	metrics_subset_glyphs(
		s,
		&mut t.var_store,
		&mut [&mut t.adv_width_map, &mut t.lsb_map, &mut t.rsb_map],
	)
}

/// `VVAR.subset_glyphs`.
pub(crate) fn vvar_subset_glyphs(s: &Subsetter, font: &mut Font) -> Result<bool> {
	let t = &mut font.require::<Vvar>()?.table;
	metrics_subset_glyphs(
		s,
		&mut t.var_store,
		&mut [
			&mut t.adv_height_map,
			&mut t.tsb_map,
			&mut t.bsb_map,
			&mut t.v_org_map,
		],
	)
}
