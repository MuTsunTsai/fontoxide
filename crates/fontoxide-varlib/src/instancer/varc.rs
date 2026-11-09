//! Instancing of `VARC` (`instantiateVARC`). Only axes that its components don't refer
//! to can be instanced.

use std::collections::{BTreeSet, HashMap};

use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::otl::Varc;
use fontoxide_ttlib::{Error, Font, Result};

use super::limits::fvar_axes;

fn no_axis_error() -> Error {
	Error::ttlib("IndexError: list index out of range")
}

/// `_getVARCAxisIndexRecords`, calling `f` with the `AxisIndex` of each variation-region
/// and condition record.
fn for_each_axis_index(
	varc: &mut ot::VARC,
	f: &mut dyn FnMut(&mut u16) -> Result<()>,
) -> Result<()> {
	if let Some(store) = &mut varc.multi_var_store {
		let list = store.sparse_var_region_list.as_mut().ok_or_else(|| {
			Error::ttlib("AttributeError: 'NoneType' object has no attribute 'Region'")
		})?;
		for region in &mut list.region {
			let region = region.as_mut().ok_or_else(|| {
				Error::ttlib(
					"AttributeError: 'NoneType' object has no attribute 'SparseVarRegionAxis'",
				)
			})?;
			for axis in region.sparse_var_region_axis.iter_mut().flatten() {
				f(&mut axis.axis_index)?;
			}
		}
	}
	let mut conditions: Vec<&mut Option<Box<ot::ConditionTable>>> = match &mut varc.condition_list {
		Some(list) => list.condition_table.iter_mut().collect(),
		None => Vec::new(),
	};
	while let Some(condition) = conditions.pop() {
		let Some(condition) = condition else {
			continue;
		};
		match &mut **condition {
			ot::ConditionTable::Format1(c) => f(&mut c.axis_index)?,
			ot::ConditionTable::Format3(c) => conditions.extend(c.condition_table.iter_mut()),
			ot::ConditionTable::Format4(c) => conditions.extend(c.condition_table.iter_mut()),
			ot::ConditionTable::Format5(c) => conditions.push(&mut c.condition_table),
			_ => {}
		}
	}
	Ok(())
}

/// `_checkVARCAxisLimits`.
pub(super) fn check_axis_limits(font: &mut Font, limited: &dyn Fn(&str) -> bool) -> Result<()> {
	let tags: Vec<String> = fvar_axes(font)?.into_iter().map(|a| a.0).collect();
	let varc = &mut font.require::<Varc>()?.table;
	let mut referenced = BTreeSet::new();
	for_each_axis_index(varc, &mut |index| {
		referenced.insert(i64::from(*index));
		Ok(())
	})?;
	if let Some(list) = &varc.axis_indices_list {
		for indices in &list.item {
			referenced.extend(indices.iter().copied());
		}
	}
	for i in referenced {
		let tag = usize::try_from(i)
			.ok()
			.and_then(|i| tags.get(i))
			.ok_or_else(no_axis_error)?;
		if limited(tag) {
			return Err(Error::ttlib(
				"NotImplementedError: Instancing across VarComponent axes is not supported.",
			));
		}
	}
	Ok(())
}

/// `instantiateVARC`.
pub(super) fn instantiate_varc(
	font: &mut Font,
	limited: &dyn Fn(&str) -> bool,
	pinned: &dyn Fn(&str) -> bool,
) -> Result<()> {
	log::info!("Instantiating VARC tables");
	check_axis_limits(font, limited)?;
	let tags: Vec<String> = fvar_axes(font)?.into_iter().map(|a| a.0).collect();
	let mut reverse_axis_map = HashMap::new();
	let mut next = 0;
	for (i, tag) in tags.iter().enumerate() {
		if !pinned(tag) {
			reverse_axis_map.insert(i as i64, next);
			next += 1;
		}
	}
	let map = |index: i64| {
		reverse_axis_map
			.get(&index)
			.copied()
			.ok_or_else(|| Error::ttlib(format!("KeyError: {index}")))
	};
	let varc = &mut font.require::<Varc>()?.table;
	if let Some(list) = &mut varc.axis_indices_list {
		for indices in &mut list.item {
			for index in indices {
				*index = map(*index)?;
			}
		}
	}
	for_each_axis_index(varc, &mut |index| {
		*index = u16::try_from(map(i64::from(*index))?).map_err(|_| no_axis_error())?;
		Ok(())
	})
}
