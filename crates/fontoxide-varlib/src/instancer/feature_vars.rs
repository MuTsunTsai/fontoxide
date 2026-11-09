//! Port of `fontTools.varLib.instancer.featureVars`.

use std::collections::{BTreeSet, HashMap, HashSet};

use fontoxide_misc::round_tools::ot_round;
use fontoxide_ttlib::binary::fit;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::varc::NO_VARIATION_INDEX;
use fontoxide_ttlib::tables::{Gdef, Gpos, Gsub};
use fontoxide_ttlib::var::{Location, VarStoreInstancer};
use fontoxide_ttlib::{Error, Font, Result, Tag};

use super::item_store::DefaultDeltas;
use super::limits::{NormalizedAxisTriple, NormalizedLimits, fvar_axes};

const GPOS: Tag = Tag::new(b"GPOS");
const GSUB: Tag = Tag::new(b"GSUB");

/// `_LOOKUP_CONDITION_*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Condition {
	False,
	True,
	Keep,
}

type Variations = Option<Box<ot::FeatureVariations>>;

/// Calls `f` on every condition in `GSUB` and `GPOS`, in the order of
/// `_iterLookupVariationConditions`.
pub(crate) fn for_each_condition(
	font: &mut Font,
	mut f: impl FnMut(&mut ot::ConditionTable) -> Result<()>,
) -> Result<()> {
	// The variations are taken out, to refer to both tables' at the same time.
	let mut gsub: Variations = None;
	let mut gpos: Variations = None;
	if let Some(t) = font.get::<Gsub>()? {
		gsub = t.feature_variations.take();
	}
	if let Some(t) = font.get::<Gpos>()? {
		gpos = t.feature_variations.take();
	}
	let result = walk_conditions([&mut gsub, &mut gpos], &mut f);
	if let Some(t) = font.get::<Gsub>()? {
		t.feature_variations = gsub;
	}
	if let Some(t) = font.get::<Gpos>()? {
		t.feature_variations = gpos;
	}
	result
}

fn walk_conditions(
	tables: [&mut Variations; 2],
	f: &mut impl FnMut(&mut ot::ConditionTable) -> Result<()>,
) -> Result<()> {
	let mut stack: Vec<&mut Option<Box<ot::ConditionTable>>> = Vec::new();
	for variations in tables.into_iter().flatten() {
		for record in &mut variations.feature_variation_record {
			if let Some(set) = &mut record.condition_set {
				stack.extend(set.condition_table.iter_mut());
			}
		}
		for record in &mut variations.lookup_variation_record {
			let table = record.feature_lookups_table.as_mut().ok_or_else(|| {
				Error::ttlib("AttributeError: no attribute 'LookupConditionRecord'")
			})?;
			for condition_record in &mut table.lookup_condition_record {
				stack.push(&mut condition_record.condition_table);
			}
		}
	}
	while let Some(condition) = stack.pop() {
		let Some(condition) = condition else {
			continue;
		};
		f(condition)?;
		match &mut **condition {
			ot::ConditionTable::Format3(c) => stack.extend(c.condition_table.iter_mut()),
			ot::ConditionTable::Format4(c) => stack.extend(c.condition_table.iter_mut()),
			ot::ConditionTable::Format5(c) => stack.push(&mut c.condition_table),
			_ => {}
		}
	}
	Ok(())
}

/// `instantiateLookupVariationConditionValues`.
pub fn instantiate_condition_values(
	font: &mut Font,
	defaults: Option<&DefaultDeltas>,
	mapping: Option<&HashMap<u64, u64>>,
) -> Result<()> {
	for_each_condition(font, |condition| {
		let ot::ConditionTable::Format2(condition) = condition else {
			return Ok(());
		};
		let var_idx = u64::from(condition.var_idx);
		let new = match mapping {
			None => var_idx,
			Some(map) => map.get(&var_idx).copied().unwrap_or(NO_VARIATION_INDEX),
		};
		if let Some(defaults) = defaults {
			let value = f64::from(condition.default_value)
				+ defaults.0.get(&var_idx).copied().unwrap_or(0.0);
			// Once constant, only the unrounded sign matters: rounding a small
			// positive value to zero would change the condition.
			let value = if new == NO_VARIATION_INDEX {
				i64::from(value > 0.0)
			} else {
				ot_round(value)
			};
			condition.default_value = fit(value, "DefaultValue")?;
		}
		if mapping.is_some() {
			condition.var_idx = fit(new, "VarIdx")?;
		}
		Ok(())
	})
}

/// `_checkFeatureVariationConditions`.
pub(crate) fn check_conditions(font: &mut Font) -> Result<()> {
	fn check(variations: &Variations, tag: &str) -> Result<()> {
		let Some(variations) = variations else {
			return Ok(());
		};
		for record in &variations.feature_variation_record {
			let Some(set) = &record.condition_set else {
				continue;
			};
			for condition in set.condition_table.iter().flatten() {
				if let ot::ConditionTable::Unknown(format) = **condition {
					return Err(Error::ttlib(format!(
						"NotImplementedError: {tag} FeatureVariations condition format {format} is not supported"
					)));
				}
			}
		}
		Ok(())
	}
	if let Some(t) = font.get::<Gpos>()? {
		check(&t.feature_variations, "GPOS")?;
	}
	if let Some(t) = font.get::<Gsub>()? {
		check(&t.feature_variations, "GSUB")?;
	}
	Ok(())
}

/// `_limitFeatureVariationConditionRange`.
pub fn limit_feature_variation_condition_range(
	condition: &ot::ConditionTableFormat1,
	limit: NormalizedAxisTriple,
) -> Option<(f64, f64)> {
	let (minimum, maximum) = (
		condition.filter_range_min_value,
		condition.filter_range_max_value,
	);
	if minimum > maximum || minimum > limit.maximum || maximum < limit.minimum {
		// The condition is invalid or out of range.
		return None;
	}
	Some((
		limit.renormalize_value_clamped(minimum),
		limit.renormalize_value_clamped(maximum),
	))
}

/// `_conditionAppliesAtDefault`.
fn applies_at_default(
	condition: Option<&ot::ConditionTable>,
	limits: &NormalizedLimits,
	axes: &[String],
	depth: u32,
	instancer: &VarStoreInstancer<'_>,
) -> Result<bool> {
	let Some(condition) = condition else {
		return Ok(true);
	};
	if depth == 0 {
		return Ok(false);
	}
	Ok(match condition {
		ot::ConditionTable::Format1(c) => {
			let Some(tag) = axes.get(usize::from(c.axis_index)) else {
				return Ok(false);
			};
			let default = limits.0.get(tag).map_or(0.0, |l| l.default);
			c.filter_range_min_value <= default && default <= c.filter_range_max_value
		}
		ot::ConditionTable::Format2(c) => {
			f64::from(c.default_value) + instancer.get(u64::from(c.var_idx))? > 0.0
		}
		ot::ConditionTable::Format3(c) => {
			for child in &c.condition_table {
				if !applies_at_default(child.as_deref(), limits, axes, depth - 1, instancer)? {
					return Ok(false);
				}
			}
			true
		}
		ot::ConditionTable::Format4(c) => {
			for child in &c.condition_table {
				if applies_at_default(child.as_deref(), limits, axes, depth - 1, instancer)? {
					return Ok(true);
				}
			}
			false
		}
		ot::ConditionTable::Format5(c) => !applies_at_default(
			c.condition_table.as_deref(),
			limits,
			axes,
			depth - 1,
			instancer,
		)?,
		ot::ConditionTable::Unknown(_) => false,
	})
}

/// What the instancing of conditions refers to.
struct Context<'a> {
	limits: &'a NormalizedLimits,
	axes: &'a [String],
	axis_index_map: HashMap<&'a str, u16>,
}

/// `_instantiateLookupConditionImpl`.
fn instantiate_condition(
	condition: Option<&mut ot::ConditionTable>,
	cx: &Context<'_>,
	depth: u32,
) -> Result<Condition> {
	let Some(condition) = condition else {
		return Ok(Condition::True);
	};
	if depth == 0 {
		log::warn!("LookupVariationRecord condition nesting is too deep; ignored");
		return Ok(Condition::Keep);
	}
	match condition {
		ot::ConditionTable::Format1(c) => {
			let Some(tag) = cx.axes.get(usize::from(c.axis_index)) else {
				log::warn!(
					"LookupVariationRecord condition has invalid axis index {}; ignored",
					c.axis_index
				);
				return Ok(Condition::Keep);
			};
			let triple = match cx.limits.0.get(tag) {
				Some(triple) => *triple,
				None => NormalizedAxisTriple::new(-1.0, 0.0, 1.0, 1.0, 1.0)?,
			};
			let (minimum, maximum) = (c.filter_range_min_value, c.filter_range_max_value);
			if minimum > maximum || triple.minimum > maximum || triple.maximum < minimum {
				return Ok(Condition::False);
			}
			if triple.minimum == triple.maximum {
				return Ok(if minimum <= triple.default && triple.default <= maximum {
					Condition::True
				} else {
					Condition::False
				});
			}
			let Some((new_min, new_max)) = limit_feature_variation_condition_range(c, triple)
			else {
				return Ok(Condition::False);
			};
			let Some(&index) = cx.axis_index_map.get(tag.as_str()) else {
				return Ok(Condition::False);
			};
			c.axis_index = index;
			c.filter_range_min_value = new_min;
			c.filter_range_max_value = new_max;
			Ok(if new_min <= -1.0 && new_max >= 1.0 {
				Condition::True
			} else {
				Condition::Keep
			})
		}
		ot::ConditionTable::Format2(c) => Ok(if u64::from(c.var_idx) != NO_VARIATION_INDEX {
			Condition::Keep
		} else if c.default_value > 0 {
			Condition::True
		} else {
			Condition::False
		}),
		ot::ConditionTable::Format3(c) => {
			let mut kept = Vec::with_capacity(c.condition_table.len());
			for mut child in std::mem::take(&mut c.condition_table) {
				match instantiate_condition(child.as_deref_mut(), cx, depth - 1)? {
					Condition::False => return Ok(Condition::False),
					Condition::Keep => kept.push(child),
					Condition::True => {}
				}
			}
			c.condition_count = Some(kept.len() as u32);
			let keep = !kept.is_empty();
			c.condition_table = kept;
			Ok(if keep {
				Condition::Keep
			} else {
				Condition::True
			})
		}
		ot::ConditionTable::Format4(c) => {
			let mut kept = Vec::with_capacity(c.condition_table.len());
			for mut child in std::mem::take(&mut c.condition_table) {
				match instantiate_condition(child.as_deref_mut(), cx, depth - 1)? {
					Condition::True => return Ok(Condition::True),
					Condition::Keep => kept.push(child),
					Condition::False => {}
				}
			}
			c.condition_count = Some(kept.len() as u32);
			let keep = !kept.is_empty();
			c.condition_table = kept;
			Ok(if keep {
				Condition::Keep
			} else {
				Condition::False
			})
		}
		ot::ConditionTable::Format5(c) => Ok(
			match instantiate_condition(c.condition_table.as_deref_mut(), cx, depth - 1)? {
				Condition::True => Condition::False,
				Condition::False => Condition::True,
				Condition::Keep => Condition::Keep,
			},
		),
		ot::ConditionTable::Unknown(format) => {
			log::warn!("LookupVariationRecord condition has unsupported format {format}; ignored");
			Ok(Condition::Keep)
		}
	}
}

/// `_featureVariationRecordIsUnique`.
fn record_is_unique(
	record: &ot::FeatureVariationRecord,
	seen: &mut HashSet<(u32, BTreeSet<(u16, u64, u64)>)>,
) -> bool {
	let mut conditions = BTreeSet::new();
	if let Some(set) = &record.condition_set {
		for condition in set.condition_table.iter().flatten() {
			let ot::ConditionTable::Format1(c) = &**condition else {
				// Can't tell whether this is a duplicate: assume it is unique.
				return true;
			};
			let bits = |v: f64| (v + 0.0).to_bits();
			conditions.insert((
				c.axis_index,
				bits(c.filter_range_min_value),
				bits(c.filter_range_max_value),
			));
		}
	}
	let version = record
		.feature_table_substitution
		.as_ref()
		.map_or(0x0001_0000, |s| s.version);
	seen.insert((version, conditions))
}

/// `_instantiateFeatureVariationRecord`: whether it applies at the default,
/// whether to keep it, and whether it applies everywhere.
fn instantiate_record(
	record: &mut ot::FeatureVariationRecord,
	defaults: &[bool],
	cx: &Context<'_>,
) -> Result<(bool, bool, bool)> {
	let mut applies = true;
	let mut should_keep = false;
	let mut new_conditions = Some(Vec::new());
	let set = record.condition_set.get_or_insert_with(|| {
		Box::new(ot::ConditionSet {
			condition_count: Some(0),
			condition_table: Vec::new(),
		})
	});
	let conditions = std::mem::take(&mut set.condition_table);
	let mut remaining = conditions.into_iter().enumerate();
	let mut original_rest = Vec::new();
	for (i, mut condition) in remaining.by_ref() {
		if condition.is_none() {
			continue;
		}
		if let Some(ot::ConditionTable::Unknown(format)) = condition.as_deref() {
			return Err(Error::ttlib(format!(
				"NotImplementedError: FeatureVariations condition format {format} is not supported"
			)));
		}
		applies &= defaults.get(i).copied().unwrap_or(true);
		match instantiate_condition(condition.as_deref_mut(), cx, 64)? {
			Condition::False => {
				new_conditions = None;
				applies = false;
				original_rest.push(condition);
				break;
			}
			Condition::Keep => {
				if let Some(list) = &mut new_conditions {
					list.push(condition);
				}
				should_keep = true;
			}
			Condition::True => {
				if !cx.axis_index_map.is_empty() {
					should_keep = true;
				}
			}
		}
	}
	// Satisfied pinned conditions can leave a universal first-match record.
	let universal = new_conditions.as_ref().is_some_and(Vec::is_empty);
	if let Some(list) = new_conditions.filter(|_| should_keep || universal) {
		if list.is_empty() {
			record.condition_set = None;
		} else {
			set.condition_table = list;
		}
		should_keep = should_keep || !cx.axis_index_map.is_empty();
	} else {
		should_keep = false;
	}
	Ok((applies, should_keep, universal))
}

/// `_instantiateLookupVariations`.
fn instantiate_lookup_variations(
	variations: &mut ot::FeatureVariations,
	feature_list: &mut Option<Box<ot::FeatureList>>,
	cx: &Context<'_>,
) -> Result<()> {
	if variations.lookup_variation_record.is_empty() {
		return Ok(());
	}
	let fully_instanced = cx.axis_index_map.is_empty();
	let mut new_records = Vec::new();
	for mut record in std::mem::take(&mut variations.lookup_variation_record) {
		let table = record
			.feature_lookups_table
			.as_mut()
			.ok_or_else(|| Error::ttlib("AttributeError: no attribute 'LookupConditionRecord'"))?;
		let mut selected: Vec<u16> = Vec::new();
		let mut new_conditions = Vec::new();
		let mut can_bake = fully_instanced;
		for mut condition_record in std::mem::take(&mut table.lookup_condition_record) {
			match instantiate_condition(condition_record.condition_table.as_deref_mut(), cx, 64)? {
				Condition::False => {}
				Condition::True => {
					if let Some(list) = &condition_record.lookup_index_list {
						selected.extend(&list.lookup_index);
					}
					if !fully_instanced {
						condition_record.condition_table = Some(Box::new(
							ot::ConditionTable::Format2(ot::ConditionTableFormat2 {
								default_value: 1,
								var_idx: NO_VARIATION_INDEX as u32,
							}),
						));
						new_conditions.push(condition_record);
					}
				}
				Condition::Keep => {
					can_bake = false;
					new_conditions.push(condition_record);
				}
			}
		}
		match feature_list.as_mut().filter(|_| can_bake) {
			Some(list) => {
				let feature_record = list
					.feature_record
					.get_mut(usize::from(record.feature_index))
					.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
				let feature = feature_record.feature.as_mut().ok_or_else(|| {
					Error::ttlib("AttributeError: no attribute 'LookupListIndex'")
				})?;
				let mut indices: BTreeSet<u16> = if table.flags & 1 != 0 {
					feature.lookup_list_index.iter().copied().collect()
				} else {
					BTreeSet::new()
				};
				indices.extend(selected);
				feature.lookup_list_index = indices.into_iter().collect();
				feature.lookup_count = Some(feature.lookup_list_index.len() as u32);
			}
			None => {
				table.lookup_condition_count = Some(new_conditions.len() as u32);
				table.lookup_condition_record = new_conditions;
				new_records.push(record);
			}
		}
	}
	variations.lookup_variation_count = Some(new_records.len() as u32);
	variations.lookup_variation_record = new_records;
	Ok(())
}

/// `_instantiateFeatureVariations`.
fn instantiate_feature_variations_table(
	slot: &mut Variations,
	feature_list: &mut Option<Box<ot::FeatureList>>,
	version: &mut u32,
	defaults: &[Vec<bool>],
	cx: &Context<'_>,
) -> Result<()> {
	let Some(variations) = slot else {
		return Ok(());
	};
	let mut applied = false;
	let mut unique = HashSet::new();
	let mut new_records = Vec::new();
	let mut defaults_substs: Option<Box<ot::FeatureTableSubstitution>> = None;
	let mut universal = false;
	for (i, mut record) in std::mem::take(&mut variations.feature_variation_record)
		.into_iter()
		.enumerate()
	{
		let (applies, should_keep, is_universal) =
			instantiate_record(&mut record, defaults.get(i).map_or(&[], Vec::as_slice), cx)?;
		universal = is_universal;
		if applies && !applied {
			if let Some(substitution) = &record.feature_table_substitution {
				if substitution.version != 0x0001_0000 {
					return Err(Error::Assertion(
						"record.FeatureTableSubstitution.Version == 0x00010000".into(),
					));
				}
				let mut saved = substitution.clone();
				let list = feature_list
					.as_mut()
					.ok_or_else(|| Error::ttlib("AttributeError: no attribute 'FeatureRecord'"))?;
				for (default, rec) in saved
					.substitution_record
					.iter_mut()
					.zip(&substitution.substitution_record)
				{
					let target = list
						.feature_record
						.get_mut(usize::from(rec.feature_index))
						.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
					default.feature = target.feature.clone();
					target.feature = rec.feature.clone();
				}
				defaults_substs = Some(saved);
			}
			// Set variations only once: a null substitution still matches and blocks
			// later records.
			applied = true;
		}
		if should_keep && record_is_unique(&record, &mut unique) {
			new_records.push(record);
		}
		// Further records don't have a chance to apply after a universal one.
		if universal {
			break;
		}
	}
	// Insert a catch-all record to reinstate the old features if necessary.
	if applied && !new_records.is_empty() && !universal {
		new_records.push(ot::FeatureVariationRecord {
			condition_set: Some(Box::new(ot::ConditionSet {
				condition_count: Some(0),
				condition_table: Vec::new(),
			})),
			feature_table_substitution: defaults_substs,
		});
	}
	variations.feature_variation_count = Some(new_records.len() as u32);
	let empty = new_records.is_empty();
	variations.feature_variation_record = new_records;
	instantiate_lookup_variations(variations, feature_list, cx)?;
	if empty && variations.lookup_variation_record.is_empty() {
		*slot = None;
		// Downgrade the table version if there are no FeatureVariations left.
		*version = 0x0001_0000;
	}
	Ok(())
}

/// The defaults of `conditionDefaults`, by record and condition.
fn record_defaults(
	variations: &Variations,
	limits: &NormalizedLimits,
	axes: &[String],
	instancer: &VarStoreInstancer<'_>,
) -> Result<Vec<Vec<bool>>> {
	let Some(variations) = variations else {
		return Ok(Vec::new());
	};
	let mut out = Vec::with_capacity(variations.feature_variation_record.len());
	for record in &variations.feature_variation_record {
		let mut row = Vec::new();
		if let Some(set) = &record.condition_set {
			for condition in &set.condition_table {
				row.push(applies_at_default(
					condition.as_deref(),
					limits,
					axes,
					64,
					instancer,
				)?);
			}
		}
		out.push(row);
	}
	Ok(out)
}

/// `Table.prune_lookups()`.
fn prune_lookups(font: &mut Font, tag: Tag) -> Result<()> {
	fontoxide_subset::prune_lookups(font, tag).map_err(|e| match e {
		fontoxide_subset::Error::Ttlib(e) => e,
		e => Error::ttlib(e.to_string()),
	})
}

/// `instantiateFeatureVariations`.
pub fn instantiate_feature_variations(font: &mut Font, limits: &NormalizedLimits) -> Result<()> {
	check_conditions(font)?;
	let axes: Vec<String> = fvar_axes(font)?.into_iter().map(|a| a.0).collect();
	let store = match font.get::<Gdef>()? {
		Some(gdef) => gdef.var_store.clone(),
		None => None,
	};
	let instancer = VarStoreInstancer::new(store.as_deref(), &axes, Location::new());
	// Record default applicability before any condition is modified.
	let gpos_defaults = match font.get::<Gpos>()? {
		Some(t) => record_defaults(&t.feature_variations, limits, &axes, &instancer)?,
		None => Vec::new(),
	};
	let gsub_defaults = match font.get::<Gsub>()? {
		Some(t) => record_defaults(&t.feature_variations, limits, &axes, &instancer)?,
		None => Vec::new(),
	};
	let pinned = limits.pinned_location();
	let axis_index_map: HashMap<&str, u16> = axes
		.iter()
		.filter(|t| !pinned.contains_key(*t))
		.enumerate()
		.map(|(i, t)| (t.as_str(), i as u16))
		.collect();
	let cx = Context {
		limits,
		axes: &axes,
		axis_index_map,
	};
	if let Some(t) = font.get::<Gpos>()?
		&& t.feature_variations.is_some()
	{
		log::info!("Instantiating FeatureVariations of GPOS table");
		let t = &mut t.table;
		instantiate_feature_variations_table(
			&mut t.feature_variations,
			&mut t.feature_list,
			&mut t.version,
			&gpos_defaults,
			&cx,
		)?;
		// Remove unreferenced lookups.
		prune_lookups(font, GPOS)?;
	}
	if let Some(t) = font.get::<Gsub>()?
		&& t.feature_variations.is_some()
	{
		log::info!("Instantiating FeatureVariations of GSUB table");
		let t = &mut t.table;
		instantiate_feature_variations_table(
			&mut t.feature_variations,
			&mut t.feature_list,
			&mut t.version,
			&gsub_defaults,
			&cx,
		)?;
		prune_lookups(font, GSUB)?;
	}
	Ok(())
}
