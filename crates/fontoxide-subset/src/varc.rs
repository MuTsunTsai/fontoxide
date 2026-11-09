//! Subsetting of `VARC` (`fontTools.subset`, `fontTools.varLib.multiVarStore`).

use std::collections::{BTreeMap, BTreeSet};

use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::otl::Varc;
use fontoxide_ttlib::{Error as TtError, Font};

use crate::layout::{cov, cov_mut, cov_subset, list_subset};
use crate::varstore::multi_subset_varidxes;
use crate::{GlyphSet, Result, Subsetter};

fn composite_glyphs(table: &mut ot::VARC) -> Result<&mut Vec<ot::VarCompositeGlyph>> {
	Ok(&mut table
		.var_composite_glyphs
		.as_deref_mut()
		.ok_or_else(|| {
			TtError::ttlib("AttributeError: 'NoneType' object has no attribute 'VarCompositeGlyph'")
		})?
		.var_composite_glyph)
}

/// `closure_glyphs`.
pub(crate) fn closure_glyphs(s: &mut Subsetter, font: &mut Font) -> Result<()> {
	let table = &font.require::<Varc>()?.table;
	let Some(records) = &table.var_composite_glyphs else {
		return Ok(());
	};
	let records = &records.var_composite_glyph;
	let glyph_map: BTreeMap<u32, usize> = cov(&table.coverage)?
		.glyphs
		.iter()
		.enumerate()
		.map(|(i, &g)| (g, i))
		.collect();
	let mut covered = GlyphSet::new();
	let mut new = s.glyphs.clone();
	while !new.is_empty() {
		let mut next = GlyphSet::new();
		for g in new {
			if !covered.insert(g) {
				continue;
			}
			let Some(&idx) = glyph_map.get(&g) else {
				continue;
			};
			let glyph = records
				.get(idx)
				.ok_or_else(|| TtError::ttlib("IndexError: list index out of range"))?;
			for comp in &glyph.components {
				s.glyphs.insert(comp.glyph);
				if !covered.contains(&comp.glyph) {
					next.insert(comp.glyph);
				}
			}
		}
		new = next;
	}
	Ok(())
}

/// `subset_glyphs`.
pub(crate) fn subset_glyphs(s: &Subsetter, font: &mut Font) -> Result<bool> {
	let table = &mut font.require::<Varc>()?.table;
	let indices = cov_subset(cov_mut(&mut table.coverage)?, &s.glyphs);
	let records = composite_glyphs(table)?;
	*records = list_subset(records, &indices);
	Ok(!records.is_empty())
}

fn remapped(map: &BTreeMap<u64, u64>, idx: u64) -> Result<u64> {
	map.get(&idx)
		.copied()
		.ok_or_else(|| TtError::ttlib(format!("KeyError: {idx}")).into())
}

/// `ConditionTable.collect_varidxes` or, given a map, `remap_varidxes`.
fn visit_condition(
	condition: &mut ot::ConditionTable,
	used: &mut BTreeSet<u64>,
	map: Option<&BTreeMap<u64, u64>>,
) -> Result<()> {
	match condition {
		ot::ConditionTable::Format2(t) => {
			let idx = u64::from(t.var_idx);
			used.insert(idx);
			if let Some(map) = map {
				t.var_idx = remapped(map, idx)? as u32;
			}
		}
		ot::ConditionTable::Format3(t) => {
			for c in t.condition_table.iter_mut().flatten() {
				visit_condition(c, used, map)?;
			}
		}
		ot::ConditionTable::Format4(t) => {
			for c in t.condition_table.iter_mut().flatten() {
				visit_condition(c, used, map)?;
			}
		}
		ot::ConditionTable::Format5(t) => {
			if let Some(c) = &mut t.condition_table {
				visit_condition(c, used, map)?;
			}
		}
		_ => {}
	}
	Ok(())
}

/// `VARC.collect_varidxes` or, given a map, `remap_varidxes`.
fn visit_varidxes(
	table: &mut ot::VARC,
	used: &mut BTreeSet<u64>,
	map: Option<&BTreeMap<u64, u64>>,
) -> Result<()> {
	for glyph in composite_glyphs(table)? {
		for comp in &mut glyph.components {
			used.insert(comp.axis_values_var_index);
			used.insert(comp.transform_var_index);
			if let Some(map) = map {
				comp.axis_values_var_index = remapped(map, comp.axis_values_var_index)?;
				comp.transform_var_index = remapped(map, comp.transform_var_index)?;
			}
		}
	}
	if let Some(list) = &mut table.condition_list {
		for c in list.condition_table.iter_mut().flatten() {
			visit_condition(c, used, map)?;
		}
	}
	Ok(())
}

/// `{old: new for new, old in enumerate(usedIndices)}`.
fn index_map(used: &BTreeSet<u64>) -> BTreeMap<u64, u64> {
	used.iter().copied().zip(0..).collect()
}

/// `prune_post_subset`.
pub(crate) fn prune_post_subset(font: &mut Font) -> Result<bool> {
	let table = &mut font.require::<Varc>()?.table;

	if table.axis_indices_list.is_some() {
		let used: BTreeSet<u64> = composite_glyphs(table)?
			.iter()
			.flat_map(|g| &g.components)
			.filter_map(|c| c.axis_indices_index)
			.collect();
		if used.is_empty() {
			table.axis_indices_list = None;
		} else {
			let map = index_map(&used);
			for glyph in composite_glyphs(table)? {
				for comp in &mut glyph.components {
					if let Some(i) = &mut comp.axis_indices_index {
						*i = map[i];
					}
				}
			}
			if let Some(list) = &mut table.axis_indices_list {
				let indices: Vec<usize> = used.iter().map(|&i| i as usize).collect();
				list.item = list_subset(&list.item, &indices);
			}
		}
	}

	if table.condition_list.is_some() {
		let used: BTreeSet<u64> = composite_glyphs(table)?
			.iter()
			.flat_map(|g| &g.components)
			.filter_map(|c| c.condition_index)
			.collect();
		if used.is_empty() {
			table.condition_list = None;
		} else {
			let map = index_map(&used);
			for glyph in composite_glyphs(table)? {
				for comp in &mut glyph.components {
					if let Some(i) = &mut comp.condition_index {
						*i = map[i];
					}
				}
			}
			if let Some(list) = &mut table.condition_list {
				let indices: Vec<usize> = used.iter().map(|&i| i as usize).collect();
				list.condition_table = list_subset(&list.condition_table, &indices);
				list.condition_count = Some(list.condition_table.len() as u32);
			}
		}
	}

	// Conditions can reference the variation store, so subset them first.
	if table.multi_var_store.is_some() {
		let mut used = BTreeSet::new();
		visit_varidxes(table, &mut used, None)?;
		let mut store = table.multi_var_store.take();
		let map = match store.as_deref_mut() {
			Some(store) => multi_subset_varidxes(store, &used)?,
			None => BTreeMap::new(),
		};
		visit_varidxes(table, &mut BTreeSet::new(), Some(&map))?;
		// `MultiVarStore.__bool__`.
		table.multi_var_store = store.filter(|s| !s.multi_var_data.is_empty());
	}
	Ok(true)
}

#[cfg(test)]
mod tests {
	use fontoxide_ttlib::ot::varc::{VarComponent, VarCompositeGlyph};

	use super::*;

	/// `test_varComposite_closure_visited`: glyphs are 0 ("a"), 1, 2 and 3.
	#[test]
	fn closure_visits_each_glyph_once() -> Result<()> {
		let cases: [&[(u32, &[u32])]; 3] = [
			&[(0, &[0])],
			&[(0, &[1]), (1, &[0])],
			&[(0, &[1, 2]), (1, &[3]), (2, &[3]), (3, &[])],
		];
		for edges in cases {
			let mut table = ot::VARC {
				coverage: Some(Box::new(ot::Coverage {
					glyphs: edges.iter().map(|e| e.0).collect(),
				})),
				..Default::default()
			};
			table.var_composite_glyphs = Some(Box::new(ot::VarCompositeGlyphs {
				var_composite_glyph: edges
					.iter()
					.map(|(_, components)| VarCompositeGlyph {
						components: components
							.iter()
							.map(|&glyph| VarComponent {
								glyph,
								..Default::default()
							})
							.collect(),
					})
					.collect(),
			}));
			let mut font = Font::new();
			font.insert(Box::new(Varc { table }));
			let mut s = Subsetter {
				glyphs: GlyphSet::from([0]),
				..Default::default()
			};
			closure_glyphs(&mut s, &mut font)?;
			assert_eq!(s.glyphs, edges.iter().map(|e| e.0).collect::<GlyphSet>());
		}
		Ok(())
	}
}
