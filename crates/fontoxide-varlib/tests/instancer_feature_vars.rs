//! `InstantiateFeatureVariationsTest` of fontTools' `Tests/varLib/instancer/instancer_test.py`.
//!
//! fontTools runs `test_null_substitution` on lazily and fully loaded tables too, which are the
//! same here.

#![allow(clippy::type_complexity)]

mod common;
mod instancer_common;

use std::collections::{BTreeMap, BTreeSet};

use common::*;
use fontoxide_ttlib::beyond64k::{ConvertOptions, lower_tables, upper_tables};
use fontoxide_ttlib::ot::conv::ValueRecord;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::var_store::{build_var_data, build_var_region_list, build_var_store};
use fontoxide_ttlib::tables::{Gdef, Gpos, Gsub};
use fontoxide_ttlib::var::{Location, VarStoreInstancer};
use fontoxide_ttlib::{Font, Table, TableType, Tag};
use fontoxide_varlib::instancer::{
	AxisTriple, NormalizedAxisTriple, NormalizedLimits, instantiate_condition_values,
	instantiate_feature_variations,
};
use instancer_common::*;

type Condition = Option<Box<ot::ConditionTable>>;
type Subs = BTreeMap<String, String>;

fn subs(pairs: &[(&str, &str)]) -> Subs {
	pairs
		.iter()
		.map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
		.collect()
}

/// `_lookupVariationCondition`.
fn range_condition(minimum: f64, maximum: f64, axis_index: u16) -> Condition {
	Some(Box::new(ot::ConditionTable::Format1(
		ot::ConditionTableFormat1 {
			axis_index,
			filter_range_min_value: minimum,
			filter_range_max_value: maximum,
		},
	)))
}

/// `_lookupVariationValueCondition`.
fn value_condition(default_value: i16, var_idx: u32) -> Condition {
	Some(Box::new(ot::ConditionTable::Format2(
		ot::ConditionTableFormat2 {
			default_value,
			var_idx,
		},
	)))
}

/// `_lookupVariationCompoundCondition`.
fn compound_condition(format: u16, conditions: Vec<Condition>) -> Condition {
	let condition_count = Some(conditions.len() as u32);
	Some(Box::new(if format == 3 {
		ot::ConditionTable::Format3(ot::ConditionTableFormat3 {
			condition_count,
			condition_table: conditions,
		})
	} else {
		ot::ConditionTable::Format4(ot::ConditionTableFormat4 {
			condition_count,
			condition_table: conditions,
		})
	}))
}

fn negate_condition(condition: Condition) -> Condition {
	Some(Box::new(ot::ConditionTable::Format5(
		ot::ConditionTableFormat5 {
			condition_table: condition,
		},
	)))
}

/// `_lookupVariationRecord`.
fn lookup_record(condition: Condition, lookup_indices: &[u16]) -> ot::LookupConditionRecord {
	ot::LookupConditionRecord {
		condition_table: condition,
		lookup_index_list: Some(Box::new(ot::LookupIndexList {
			lookup_index_count: Some(lookup_indices.len() as u32),
			lookup_index: lookup_indices.to_vec(),
		})),
	}
}

/// `buildFeatureTableSubstitutionRecord`.
fn substitution_record(
	feature_index: u16,
	lookup_indices: &[u16],
) -> ot::FeatureTableSubstitutionRecord {
	ot::FeatureTableSubstitutionRecord {
		feature_index,
		feature: Some(Box::new(ot::Feature {
			feature_params: None,
			lookup_count: Some(lookup_indices.len() as u32),
			lookup_list_index: lookup_indices.to_vec(),
		})),
	}
}

/// `buildFeatureVariationRecord([condition], [substitution])`.
fn variation_record(
	condition: Condition,
	substitution: ot::FeatureTableSubstitutionRecord,
) -> ot::FeatureVariationRecord {
	ot::FeatureVariationRecord {
		condition_set: Some(Box::new(ot::ConditionSet {
			condition_count: Some(1),
			condition_table: vec![condition],
		})),
		feature_table_substitution: Some(Box::new(ot::FeatureTableSubstitution {
			version: 0x0001_0000,
			substitution_count: Some(1),
			substitution_record: vec![substitution],
		})),
	}
}

/// `makeLookupVariationsFont`.
fn lookup_variations_font() -> TestResult<Font> {
	let mut font = fixture("lookup_variations")?;
	restore_condition_values(&mut font, &[0.24])?;
	Ok(font)
}

/// `makeLookupVariationsFont(withVarStore=True)`.
fn lookup_variations_font_with_store() -> TestResult<Font> {
	let mut font = fixture("lookup_variations_store")?;
	restore_condition_values(&mut font, &[0.24])?;
	Ok(font)
}

/// `makeFeatureVariationConditionsFont`.
fn conditions_font(format: u16) -> TestResult<Font> {
	let mut font = fixture(&format!("conditions_{format}"))?;
	restore_condition_values(&mut font, &[0.24])?;
	Ok(font)
}

/// The `makeFeatureVarsFont` of `test_partial_instance` and `test_full_instance`.
fn three_rules_font() -> TestResult<Font> {
	let mut font = fixture("feature_vars_three_rules")?;
	restore_condition_values(&mut font, &[0.20886, -0.45654])?;
	Ok(font)
}

fn gsub_variations(font: &mut Font) -> TestResult<&mut ot::FeatureVariations> {
	Ok(font
		.require::<Gsub>()?
		.feature_variations
		.as_mut()
		.ok_or("no FeatureVariations")?)
}

fn gsub_lookup_conditions(font: &mut Font) -> TestResult<&mut ot::FeatureLookupsTable> {
	Ok(gsub_variations(font)?.lookup_variation_record[0]
		.feature_lookups_table
		.as_mut()
		.ok_or("no FeatureLookupsTable")?)
}

/// Sets the variations to legacy records only, as many tests do.
fn make_legacy(variations: &mut ot::FeatureVariations, records: Vec<ot::FeatureVariationRecord>) {
	variations.version = 0x0001_0000;
	variations.feature_variation_count = Some(records.len() as u32);
	variations.feature_variation_record = records;
	variations.lookup_variation_count = None;
	variations.lookup_variation_record = Vec::new();
}

/// `font[tag] = newTable(tag); font[tag].decompile(font[tag].compile(font), font)`.
fn recompile<T: TableType + Table + 'static>(font: &mut Font) -> TestResult {
	let data = font.table_data(T::TAG)?;
	let table = T::decompile(&data, font)?;
	font.insert(Box::new(table));
	Ok(())
}

fn table_is_gsub(tag: &str) -> bool {
	tag == "GSUB"
}

/// `_evaluateCondition`.
fn evaluate_condition(
	condition: Option<&ot::ConditionTable>,
	axes: &[String],
	location: &Location,
	instancer: &dyn Fn(u64) -> f64,
) -> bool {
	let Some(condition) = condition else {
		// A null condition offset denotes True.
		return true;
	};
	match condition {
		ot::ConditionTable::Format1(c) => {
			let value = location
				.get(&axes[usize::from(c.axis_index)])
				.copied()
				.unwrap_or(0.0);
			c.filter_range_min_value <= value && value <= c.filter_range_max_value
		}
		ot::ConditionTable::Format2(c) => {
			let mut value = f64::from(c.default_value);
			if u64::from(c.var_idx) != NO_VARIATION_INDEX {
				value += instancer(u64::from(c.var_idx));
			}
			value > 0.0
		}
		ot::ConditionTable::Format3(c) => c
			.condition_table
			.iter()
			.all(|c| evaluate_condition(c.as_deref(), axes, location, instancer)),
		ot::ConditionTable::Format4(c) => c
			.condition_table
			.iter()
			.any(|c| evaluate_condition(c.as_deref(), axes, location, instancer)),
		ot::ConditionTable::Format5(c) => {
			!evaluate_condition(c.condition_table.as_deref(), axes, location, instancer)
		}
		ot::ConditionTable::Unknown(format) => panic!("unknown format {format}"),
	}
}

/// `_evaluateCondition(c, axes, location, {})`: the instancer has nothing.
fn no_instancer(var_idx: u64) -> f64 {
	panic!("KeyError: {var_idx}")
}

/// `_getSubstitutions`.
fn get_substitutions(font: &mut Font, lookup_indices: &[u16]) -> TestResult<Subs> {
	let order = font.glyph_order()?;
	let gsub = font.require::<Gsub>()?;
	let list = gsub.lookup_list.as_ref().ok_or("no LookupList")?;
	let mut out = Subs::new();
	for (index, lookup) in list.lookup.iter().enumerate() {
		if !lookup_indices.contains(&(index as u16)) {
			continue;
		}
		for subtable in &lookup.as_ref().ok_or("no lookup")?.sub_table {
			let Some(ot::LookupSubTable::SingleSubst(subtable)) = subtable.as_deref() else {
				return Err("not SingleSubst".into());
			};
			for (a, b) in &subtable.mapping {
				out.insert(order.name(*a).into_owned(), order.name(*b).into_owned());
			}
		}
	}
	Ok(out)
}

/// The axis tags of `fvar`, if it is there.
fn axes_of(font: &mut Font) -> TestResult<Vec<String>> {
	if font.contains(Tag::new(b"fvar")) {
		axis_tags(font)
	} else {
		Ok(Vec::new())
	}
}

fn location_of(pairs: &[(&str, f64)]) -> Location {
	pairs.iter().map(|(t, v)| ((*t).to_owned(), *v)).collect()
}

/// `_getFeatureVariationSubstitutions`.
fn get_feature_variation_substitutions(
	font: &mut Font,
	location: &[(&str, f64)],
) -> TestResult<Subs> {
	let axes = axes_of(font)?;
	let location = location_of(location);
	let store = match font.get::<Gdef>()? {
		Some(gdef) => gdef.var_store.clone(),
		None => None,
	};
	let evaluator = VarStoreInstancer::new(store.as_deref(), &axes, location.clone());
	let instancer = |var_idx: u64| evaluator.get(var_idx).expect("a delta");
	let gsub = font.require::<Gsub>()?;
	let list = gsub.feature_list.as_ref().ok_or("no FeatureList")?;
	let mut feature = list.feature_record[0]
		.feature
		.as_ref()
		.ok_or("no Feature")?;
	if let Some(variations) = &gsub.feature_variations {
		for record in &variations.feature_variation_record {
			let matches = record.condition_set.as_ref().is_none_or(|set| {
				set.condition_table
					.iter()
					.all(|c| evaluate_condition(c.as_deref(), &axes, &location, &instancer))
			});
			if matches {
				feature = record
					.feature_table_substitution
					.as_ref()
					.ok_or("no FeatureTableSubstitution")?
					.substitution_record[0]
					.feature
					.as_ref()
					.ok_or("no Feature")?;
				break;
			}
		}
	}
	let indices = feature.lookup_list_index.clone();
	get_substitutions(font, &indices)
}

/// `gsub.FeatureList.FeatureRecord[0].Feature.LookupListIndex`.
fn first_feature_lookups(font: &mut Font) -> TestResult<Vec<u16>> {
	Ok(font
		.require::<Gsub>()?
		.feature_list
		.as_ref()
		.ok_or("no FeatureList")?
		.feature_record[0]
		.feature
		.as_ref()
		.ok_or("no Feature")?
		.lookup_list_index
		.clone())
}

fn assert_no_feature_variations(font: &mut Font) -> TestResult {
	assert!(font.require::<Gsub>()?.feature_variations.is_none());
	Ok(())
}

fn compile_gsub(font: &mut Font) -> TestResult {
	font.table_data(Tag::new(b"GSUB"))?;
	Ok(())
}

/// The location of a pinned font: `{tag: value}`, as limits.
fn pins(pairs: &[(&str, f64)]) -> Vec<(String, Option<AxisTriple>)> {
	pairs
		.iter()
		.map(|(t, v)| ((*t).to_owned(), pin(*v)))
		.collect()
}

fn instantiate_pins(font: &mut Font, pairs: &[(&str, f64)]) -> TestResult {
	let limits = pins(pairs);
	let limits: Vec<(&str, Option<AxisTriple>)> =
		limits.iter().map(|(t, l)| (t.as_str(), *l)).collect();
	instantiate(font, &limits)
}

/// The variations of `GSUB` or `GPOS`.
fn variations_of<'a>(
	font: &'a mut Font,
	tag: &str,
) -> TestResult<Option<&'a ot::FeatureVariations>> {
	Ok(if table_is_gsub(tag) {
		font.require::<Gsub>()?.feature_variations.as_deref()
	} else {
		font.require::<Gpos>()?.feature_variations.as_deref()
	})
}

/// Moves the variations of `GSUB` to `GPOS` and removes `GSUB`.
fn move_variations_to_gpos(font: &mut Font) -> TestResult {
	let variations = font.require::<Gsub>()?.feature_variations.take();
	font.remove(Tag::new(b"GSUB"));
	let gpos = font.require::<Gpos>()?;
	gpos.feature_variations = variations;
	gpos.version = 0x0001_0001;
	Ok(())
}

fn first_single_pos(font: &mut Font) -> TestResult<&mut ot::SinglePosFormat1> {
	match font
		.require::<Gpos>()?
		.lookup_list
		.as_mut()
		.ok_or("no LookupList")?
		.lookup[0]
		.as_mut()
		.ok_or("no Lookup")?
		.sub_table[0]
		.as_deref_mut()
	{
		Some(ot::LookupSubTable::SinglePos(ot::SinglePos::Format1(st))) => Ok(st),
		_ => Err("not SinglePos format 1".into()),
	}
}

fn store_of(font: &mut Font) -> TestResult<Option<ot::VarStore>> {
	Ok(font.require::<Gdef>()?.var_store.as_deref().cloned())
}

mod instantiate_feature_variations_test {
	use super::*;

	const WRAPPERS: [&str; 4] = ["value", "and", "or", "not"];

	#[test]
	fn test_fractional_condition_remaining_variable() -> TestResult {
		let cases: [(i16, [i64; 2], f64, f64, bool); 5] = [
			(0, [1, -1], 0.25, 1.0, false),
			(1, [-1, -1], 0.75, 1.0, false),
			(0, [1, -1], 0.25, 1.0, true),
			(32767, [1, -32768], 0.25, 1.0, false),
			(0, [1, -1], 0.25, 0.75, false),
		];
		for table_tag in ["GSUB", "GPOS"] {
			for legacy in [false, true] {
				for wrapper in WRAPPERS {
					for (default, deltas, pinned, peak, tensor) in cases {
						fractional_condition_remaining_variable(
							table_tag, legacy, wrapper, default, deltas, pinned, peak, tensor,
						)?;
					}
				}
			}
		}
		Ok(())
	}

	#[allow(clippy::too_many_arguments)]
	fn fractional_condition_remaining_variable(
		table_tag: &str,
		legacy: bool,
		wrapper: &str,
		default: i16,
		deltas: [i64; 2],
		pinned: f64,
		peak: f64,
		tensor: bool,
	) -> TestResult {
		let mut font = fixture("lookup_variations_store_gpos")?;
		restore_condition_values(&mut font, &[0.24])?;
		// A positioning value shares the condition's source variation row.
		let position = first_single_pos(&mut font)?;
		position
			.value
			.get_or_insert_with(ValueRecord::default)
			.x_adv_device = Some(Some(build_var_dev_table(0)));
		position.value_format |= 0x40;
		if !table_is_gsub(table_tag) {
			move_variations_to_gpos(&mut font)?;
		}
		let mut wdth_region = vec![("wdth", (0.0, 1.0, 1.0))];
		if tensor {
			wdth_region.push(("wght", (0.0, peak, 1.0)));
		}
		font.require::<Gdef>()?.var_store = Some(Box::new(build_var_store(
			build_var_region_list(
				&[region(&[("wght", (0.0, peak, 1.0))]), region(&wdth_region)],
				&["wght".to_owned(), "wdth".to_owned()],
			)?,
			vec![build_var_data(vec![0, 1], vec![deltas.to_vec()], true)],
		)));
		let value = value_condition(default, 0);
		let condition = match wrapper {
			"and" => compound_condition(3, vec![value.clone(), value]),
			"or" => compound_condition(4, vec![value.clone(), value]),
			"not" => negate_condition(value),
			_ => value,
		};
		{
			let variations = if table_is_gsub(table_tag) {
				gsub_variations(&mut font)?
			} else {
				font.require::<Gpos>()?
					.feature_variations
					.as_mut()
					.ok_or("no FeatureVariations")?
			};
			if legacy {
				make_legacy(
					variations,
					vec![variation_record(condition, substitution_record(0, &[1]))],
				);
			} else {
				let lookups = variations.lookup_variation_record[0]
					.feature_lookups_table
					.as_mut()
					.ok_or("no FeatureLookupsTable")?;
				lookups.lookup_condition_record = vec![lookup_record(condition, &[1])];
				lookups.lookup_condition_count = Some(1);
			}
		}

		let mut original = font.clone();
		instantiate_pins(&mut font, &[("wght", pinned)])?;
		// Exercise the serialized integer fields, not just the Python objects.
		recompile::<Gdef>(&mut font)?;
		if table_is_gsub(table_tag) {
			recompile::<Gsub>(&mut font)?;
		}
		recompile::<Gpos>(&mut font)?;

		let variations = variations_of(&mut font, table_tag)?.ok_or("no FeatureVariations")?;
		let condition = if legacy {
			variations.feature_variation_record[0]
				.condition_set
				.as_ref()
				.ok_or("no ConditionSet")?
				.condition_table[0]
				.clone()
		} else {
			variations.lookup_variation_record[0]
				.feature_lookups_table
				.as_ref()
				.ok_or("no FeatureLookupsTable")?
				.lookup_condition_record[0]
				.condition_table
				.clone()
		};
		let original_axes = axis_tags(&mut original)?;
		let original_store = store_of(&mut original)?;
		let axes = axis_tags(&mut font)?;
		for coordinate in [
			0.0,
			0.125,
			0.25,
			0.33331298828125,
			0.3333740234375,
			0.5,
			16383.0 / 16384.0,
			1.0,
		] {
			let source_evaluator = VarStoreInstancer::new(
				original_store.as_ref(),
				&original_axes,
				location_of(&[("wght", pinned), ("wdth", coordinate)]),
			);
			let store = store_of(&mut font)?;
			let location = location_of(&[("wdth", coordinate)]);
			let evaluator = VarStoreInstancer::new(store.as_ref(), &axes, location.clone());
			let positive = f64::from(default) + source_evaluator.get(0)? > 0.0;
			let expected = if wrapper == "not" {
				!positive
			} else {
				positive
			};
			assert_eq!(
				evaluate_condition(condition.as_deref(), &axes, &location, &|i| evaluator
					.get(i)
					.expect("a delta")),
				expected,
				"{table_tag} {legacy} {wrapper} {default} {deltas:?} {pinned} {peak} {tensor} {coordinate}"
			);
			if !tensor {
				let device = first_single_pos(&mut font)?
					.value
					.as_ref()
					.ok_or("no value")?
					.x_adv_device
					.clone()
					.flatten()
					.ok_or("no device")?;
				let var_idx = (u64::from(device.start_size) << 16) | u64::from(device.end_size);
				assert_eq!(evaluator.get(var_idx)?, deltas[1] as f64 * coordinate);
			}
			if legacy && table_is_gsub(table_tag) {
				let mut full = font.clone();
				instantiate_pins(&mut full, &[("wdth", coordinate)])?;
				assert_eq!(
					get_feature_variation_substitutions(&mut full, &[])?,
					get_feature_variation_substitutions(
						&mut original,
						&[("wght", pinned), ("wdth", coordinate)]
					)?
				);
			}
		}
		Ok(())
	}

	/// The glyphs a lookup applies to in `GPOS`.
	fn affected_glyphs(font: &mut Font, active: &[u16]) -> TestResult<BTreeSet<String>> {
		let order = font.glyph_order()?;
		let gpos = font.require::<Gpos>()?;
		let list = gpos.lookup_list.as_ref().ok_or("no LookupList")?;
		let mut names = BTreeSet::new();
		for &index in active {
			let lookup = list.lookup[usize::from(index)]
				.as_ref()
				.ok_or("no lookup")?;
			for subtable in &lookup.sub_table {
				let coverage = match subtable.as_deref() {
					Some(ot::LookupSubTable::SinglePos(ot::SinglePos::Format1(st))) => &st.coverage,
					Some(ot::LookupSubTable::SinglePos(ot::SinglePos::Format2(st))) => &st.coverage,
					_ => return Err("not SinglePos".into()),
				};
				for glyph in &coverage.as_ref().ok_or("no Coverage")?.glyphs {
					names.insert(order.name(*glyph).into_owned());
				}
			}
		}
		Ok(names)
	}

	#[test]
	fn test_fractional_condition_becoming_constant() -> TestResult {
		let cases: [(i16, i64, f64); 8] = [
			(0, 1, 0.25),
			(1, -1, 0.75),
			(-1, 2, 0.51),
			(1, -2, 0.49),
			(0, 1, 0.0),
			(1, -1, 1.0),
			(-1, 2, 0.49),
			(0, -1, 0.25),
		];
		for table_tag in ["GSUB", "GPOS"] {
			for legacy in [false, true] {
				for partial in [false, true] {
					for wrapper in WRAPPERS {
						for (default, delta, coordinate) in cases {
							fractional_condition_becoming_constant(
								table_tag, legacy, partial, wrapper, default, delta, coordinate,
							)?;
						}
					}
				}
			}
		}
		Ok(())
	}

	fn fractional_condition_becoming_constant(
		table_tag: &str,
		legacy: bool,
		partial: bool,
		wrapper: &str,
		default: i16,
		delta: i64,
		coordinate: f64,
	) -> TestResult {
		let mut font = if table_is_gsub(table_tag) {
			lookup_variations_font_with_store()?
		} else {
			let mut font = fixture("lookup_variations_store_gpos")?;
			restore_condition_values(&mut font, &[0.24])?;
			move_variations_to_gpos(&mut font)?;
			font
		};
		font.require::<Gdef>()?
			.var_store
			.as_mut()
			.ok_or("no VarStore")?
			.var_data[0]
			.as_mut()
			.ok_or("no VarData")?
			.item = vec![vec![delta]];
		let value = value_condition(default, 0);
		let condition = match wrapper {
			"and" => compound_condition(3, vec![value.clone(), value]),
			"or" => compound_condition(
				4,
				vec![value, value_condition(0, NO_VARIATION_INDEX as u32)],
			),
			"not" => negate_condition(value),
			_ => value,
		};
		{
			let variations = if table_is_gsub(table_tag) {
				gsub_variations(&mut font)?
			} else {
				font.require::<Gpos>()?
					.feature_variations
					.as_mut()
					.ok_or("no FeatureVariations")?
			};
			if legacy {
				make_legacy(
					variations,
					vec![variation_record(condition, substitution_record(0, &[1]))],
				);
			} else {
				let lookups = variations.lookup_variation_record[0]
					.feature_lookups_table
					.as_mut()
					.ok_or("no FeatureLookupsTable")?;
				lookups.lookup_condition_record = vec![lookup_record(condition, &[1])];
				lookups.lookup_condition_count = Some(1);
			}
		}

		let mut limits = vec![("wdth", coordinate)];
		if !partial {
			limits.push(("wght", 0.0));
		}
		instantiate_pins(&mut font, &limits)?;
		font.table_data(Tag::new(if table_is_gsub(table_tag) {
			b"GSUB"
		} else {
			b"GPOS"
		}))?;
		let axes = axes_of(&mut font)?;
		let location = Location::new();
		let (feature_list, variations) = if table_is_gsub(table_tag) {
			let t = font.require::<Gsub>()?;
			(t.feature_list.clone(), t.feature_variations.clone())
		} else {
			let t = font.require::<Gpos>()?;
			(t.feature_list.clone(), t.feature_variations.clone())
		};
		let mut active = feature_list.ok_or("no FeatureList")?.feature_record[0]
			.feature
			.as_ref()
			.ok_or("no Feature")?
			.lookup_list_index
			.clone();
		if let Some(variations) = &variations {
			for record in &variations.feature_variation_record {
				let matches = record.condition_set.as_ref().is_none_or(|set| {
					set.condition_table
						.iter()
						.all(|c| evaluate_condition(c.as_deref(), &axes, &location, &no_instancer))
				});
				if matches {
					active = record
						.feature_table_substitution
						.as_ref()
						.ok_or("no FeatureTableSubstitution")?
						.substitution_record[0]
						.feature
						.as_ref()
						.ok_or("no Feature")?
						.lookup_list_index
						.clone();
					break;
				}
			}
			for record in &variations.lookup_variation_record {
				let table = record
					.feature_lookups_table
					.as_ref()
					.ok_or("no FeatureLookupsTable")?;
				for condition in &table.lookup_condition_record {
					if evaluate_condition(
						condition.condition_table.as_deref(),
						&axes,
						&location,
						&no_instancer,
					) {
						active.extend(
							&condition
								.lookup_index_list
								.as_ref()
								.ok_or("no LookupIndexList")?
								.lookup_index,
						);
					}
				}
			}
		}

		let positive = f64::from(default) + delta as f64 * coordinate > 0.0;
		let applies = if wrapper == "not" {
			!positive
		} else {
			positive
		};
		let expected: BTreeSet<String> = if legacy && applies {
			["B"].as_slice()
		} else if applies {
			["A", "B"].as_slice()
		} else {
			["A"].as_slice()
		}
		.iter()
		.map(|s| (*s).to_owned())
		.collect();
		let affected: BTreeSet<String> = if table_is_gsub(table_tag) {
			get_substitutions(&mut font, &active)?.into_keys().collect()
		} else {
			affected_glyphs(&mut font, &active)?
		};
		assert_eq!(
			affected, expected,
			"{table_tag} {legacy} {partial} {wrapper} {default} {delta} {coordinate}"
		);
		Ok(())
	}

	#[test]
	fn test_shared_conditions_remapped_once() -> TestResult {
		for legacy in [false, true] {
			for limits in [
				vec![("wght", pin(0.3))],
				vec![("wdth", triple(-0.5, 0.25, 1.0))],
			] {
				let wght_limits = limits[0].0 == "wght";
				let mut font = lookup_variations_font()?;
				let shared = range_condition(0.5, 1.0, 1);
				let conditions = [
					compound_condition(3, vec![shared.clone(), shared.clone()]),
					compound_condition(4, vec![shared]),
				];
				let variations = gsub_variations(&mut font)?;
				if legacy {
					variations.feature_variation_record = conditions
						.iter()
						.zip([1, 2])
						.map(|(c, i)| variation_record(c.clone(), substitution_record(0, &[i])))
						.collect();
					variations.feature_variation_count = Some(2);
					variations.lookup_variation_record = Vec::new();
					variations.lookup_variation_count = Some(0);
				} else {
					let lookups = gsub_lookup_conditions(&mut font)?;
					lookups.lookup_condition_record = conditions
						.iter()
						.zip([1, 2])
						.map(|(c, i)| lookup_record(c.clone(), &[i]))
						.collect();
					lookups.lookup_condition_count = Some(2);
				}

				// Share a root across GSUB and GPOS, which are instanced in sequence.
				let mut other = fixture("other_gpos")?;
				let mut gpos = other.require::<Gpos>()?.clone();
				gpos.version = 0x0001_0001;
				gpos.feature_variations = Some(Box::new(ot::FeatureVariations {
					version: 0x0001_0000,
					feature_variation_count: Some(1),
					feature_variation_record: vec![variation_record(
						conditions[0].clone(),
						substitution_record(0, &[0]),
					)],
					..Default::default()
				}));
				font.insert(Box::new(gpos));
				let mut original = font.clone();

				instantiate(&mut font, &limits)?;

				recompile::<Gsub>(&mut font)?;
				recompile::<Gpos>(&mut font)?;
				let locations: Vec<(f64, Vec<(&str, f64)>)> = if wght_limits {
					[0.0, 0.8]
						.iter()
						.map(|w| (*w, vec![("wght", 0.3), ("wdth", *w)]))
						.collect()
				} else {
					[(0.0, 0.25), (1.0, 1.0)]
						.iter()
						.map(|(w, old)| (*w, vec![("wdth", *old)]))
						.collect()
				};
				let axes = axis_tags(&mut font)?;
				for (width, old_location) in locations {
					let location = [("wdth", width)];
					if legacy {
						assert_eq!(
							get_feature_variation_substitutions(&mut font, &location)?,
							get_feature_variation_substitutions(&mut original, &old_location)?
						);
					} else {
						let table = gsub_lookup_conditions(&mut font)?;
						let at = location_of(&location);
						let selected: Vec<u16> = table
							.lookup_condition_record
							.iter()
							.filter(|r| {
								evaluate_condition(
									r.condition_table.as_deref(),
									&axes,
									&at,
									&no_instancer,
								)
							})
							.map(|r| r.lookup_index_list.as_ref().expect("a list").lookup_index[0])
							.collect();
						let old_width = old_location
							.iter()
							.find(|(t, _)| *t == "wdth")
							.ok_or("no wdth")?
							.1;
						let expected = if old_width >= 0.5 {
							subs(&[("B", "B.alt"), ("C", "C.alt")])
						} else {
							Subs::new()
						};
						assert_eq!(get_substitutions(&mut font, &selected)?, expected);
					}
				}
			}
		}
		Ok(())
	}

	#[test]
	fn test_partial_instance_preserves_universal_first_match() -> TestResult {
		for earlier_conditional in [false, true] {
			for compound in [false, true] {
				let record = |condition, lookup| {
					variation_record(condition, substitution_record(0, &[lookup]))
				};
				let mut font = lookup_variations_font()?;
				let mut condition = range_condition(0.25, 1.0, 0);
				if compound {
					condition = compound_condition(3, vec![condition]);
				}
				let mut records = vec![
					record(condition, 1),
					record(range_condition(0.5, 1.0, 1), 2),
				];
				if earlier_conditional {
					records.insert(0, record(range_condition(0.75, 1.0, 1), 3));
				}
				make_legacy(gsub_variations(&mut font)?, records);
				let mut original = font.clone();

				instantiate_pins(&mut font, &[("wght", 0.3)])?;

				// A universal record must block later records, including after saving.
				recompile::<Gsub>(&mut font)?;
				for width in [-1.0, 0.0, 0.6, 0.8, 1.0] {
					assert_eq!(
						get_feature_variation_substitutions(&mut font, &[("wdth", width)])?,
						get_feature_variation_substitutions(
							&mut original,
							&[("wght", 0.3), ("wdth", width)]
						)?,
						"{earlier_conditional} {compound} {width}"
					);
				}
			}
		}
		Ok(())
	}

	#[test]
	fn test_condition_formats_full_instance() -> TestResult {
		for format in [2, 3, 4, 5] {
			for (wght, wdth) in [(0.0, 0.0), (0.3, 0.0), (0.3, 0.8), (0.6, 0.8), (0.0, 0.8)] {
				let mut font = conditions_font(format)?;
				let location = [("wght", wght), ("wdth", wdth)];
				let expected = get_feature_variation_substitutions(&mut font, &location)?;

				instantiate_pins(&mut font, &location)?;

				assert_no_feature_variations(&mut font)?;
				assert_eq!(
					get_feature_variation_substitutions(&mut font, &[])?,
					expected,
					"{format} {wght} {wdth}"
				);
				compile_gsub(&mut font)?;
			}
		}
		Ok(())
	}

	#[test]
	fn test_condition_formats_partial_instance() -> TestResult {
		for format in [2, 3, 4, 5] {
			for pinned in [
				("wght", 0.0),
				("wght", 0.3),
				("wght", 0.6),
				("wdth", 0.0),
				("wdth", 0.8),
			] {
				let mut font = conditions_font(format)?;
				let mut original = font.clone();
				let remaining = if pinned.0 == "wght" { "wdth" } else { "wght" };

				instantiate_pins(&mut font, &[pinned])?;

				// Exercise serialization as well as the in-memory condition tree.
				recompile::<Gsub>(&mut font)?;
				for value in [-1.0, 0.0, 0.3, 0.6, 0.8, 1.0] {
					assert_eq!(
						get_feature_variation_substitutions(&mut font, &[(remaining, value)])?,
						get_feature_variation_substitutions(
							&mut original,
							&[pinned, (remaining, value)]
						)?,
						"{format} {pinned:?} {value}"
					);
				}
			}
		}
		Ok(())
	}

	#[test]
	fn test_condition_formats_first_matching_record() -> TestResult {
		let mut font = conditions_font(2)?;
		let variations = gsub_variations(&mut font)?;
		let mut second = variations.feature_variation_record[0].clone();
		let set = second.condition_set.as_mut().ok_or("no ConditionSet")?;
		let Some(ot::ConditionTable::Format2(condition)) = set.condition_table[0].as_deref_mut()
		else {
			return Err("not format 2".into());
		};
		condition.default_value = 1;
		// Use the other conditional substitution to distinguish the records.
		second
			.feature_table_substitution
			.as_mut()
			.ok_or("no FeatureTableSubstitution")?
			.substitution_record[0]
			.feature
			.as_mut()
			.ok_or("no Feature")?
			.lookup_list_index = vec![2];
		variations.feature_variation_record.push(second);
		variations.feature_variation_count = Some(2);
		let mut original = font.clone();

		instantiate_pins(&mut font, &[("wght", 0.0)])?;

		for width in [0.0, 0.3, 0.8, 1.0] {
			assert_eq!(
				get_feature_variation_substitutions(&mut font, &[("wdth", width)])?,
				get_feature_variation_substitutions(
					&mut original,
					&[("wght", 0.0), ("wdth", width)]
				)?
			);
		}
		Ok(())
	}

	/// A condition of the format with a null child.
	fn null_child_condition(format: u16, with_count: bool) -> Condition {
		Some(Box::new(match format {
			3 => ot::ConditionTable::Format3(ot::ConditionTableFormat3 {
				condition_count: with_count.then_some(1),
				condition_table: vec![None],
			}),
			4 => ot::ConditionTable::Format4(ot::ConditionTableFormat4 {
				condition_count: with_count.then_some(1),
				condition_table: vec![None],
			}),
			_ => ot::ConditionTable::Format5(ot::ConditionTableFormat5 {
				condition_table: None,
			}),
		}))
	}

	#[test]
	fn test_condition_formats_null_child() -> TestResult {
		for (format, expected) in [(3, true), (4, true), (5, false)] {
			let mut font = conditions_font(3)?;
			gsub_variations(&mut font)?.feature_variation_record[0]
				.condition_set
				.as_mut()
				.ok_or("no ConditionSet")?
				.condition_table = vec![null_child_condition(format, true)];

			instantiate_pins(&mut font, &[("wght", 0.0), ("wdth", 0.0)])?;

			assert_eq!(
				get_feature_variation_substitutions(&mut font, &[])?,
				if expected {
					subs(&[("B", "B.alt")])
				} else {
					subs(&[("A", "A.alt")])
				}
			);
		}
		Ok(())
	}

	#[test]
	fn test_lookup_variations_null_condition() -> TestResult {
		for format in [None, Some(3), Some(4), Some(5)] {
			let mut font = lookup_variations_font_with_store()?;
			gsub_lookup_conditions(&mut font)?.lookup_condition_record[0].condition_table =
				format.and_then(|f| null_child_condition(f, false));
			instantiate_condition_values(&mut font, None, None)?;
			instantiate_pins(&mut font, &[("wght", 0.0), ("wdth", 0.0)])?;

			assert_no_feature_variations(&mut font)?;
			let lookup_indices = first_feature_lookups(&mut font)?;
			let mut expected = subs(&[("A", "A.alt")]);
			if format != Some(5) {
				expected.insert("B".to_owned(), "B.alt".to_owned());
			}
			assert_eq!(get_substitutions(&mut font, &lookup_indices)?, expected);
		}
		Ok(())
	}

	#[test]
	fn test_lookup_variations_full_instance() -> TestResult {
		let cases = [
			(
				0.3,
				0.0,
				subs(&[("A", "A.alt"), ("B", "B.alt"), ("D", "D.alt")]),
			),
			(
				0.3,
				0.8,
				subs(&[("A", "A.alt"), ("C", "C.alt"), ("D", "D.alt")]),
			),
		];
		for (wght, wdth, expected) in cases {
			let mut font = lookup_variations_font()?;

			instantiate_feature_variations(
				&mut font,
				&normalized(&[("wght", &[wght]), ("wdth", &[wdth])]),
			)?;

			assert_no_feature_variations(&mut font)?;
			let lookup_indices = first_feature_lookups(&mut font)?;
			assert_eq!(get_substitutions(&mut font, &lookup_indices)?, expected);
		}
		Ok(())
	}

	#[test]
	fn test_lookup_variations_partial_instance() -> TestResult {
		let mut font = lookup_variations_font()?;

		instantiate_feature_variations(&mut font, &normalized(&[("wght", &[0.3])]))?;

		let variations = gsub_variations(&mut font)?;
		assert_eq!(variations.lookup_variation_count, Some(1));
		let records = &variations.lookup_variation_record[0]
			.feature_lookups_table
			.as_ref()
			.ok_or("no FeatureLookupsTable")?
			.lookup_condition_record;
		assert_eq!(records.len(), 3);
		let check = |count: Option<u32>, children: &[Condition]| {
			assert_eq!(count, Some(1));
			let Some(ot::ConditionTable::Format1(condition)) = children[0].as_deref() else {
				panic!("not format 1");
			};
			assert_eq!(condition.axis_index, 0);
		};
		match records[0].condition_table.as_deref() {
			Some(ot::ConditionTable::Format3(c)) => check(c.condition_count, &c.condition_table),
			other => panic!("not format 3: {other:?}"),
		}
		match records[1].condition_table.as_deref() {
			Some(ot::ConditionTable::Format4(c)) => check(c.condition_count, &c.condition_table),
			other => panic!("not format 4: {other:?}"),
		}
		assert!(matches!(
			records[2].condition_table.as_deref(),
			Some(ot::ConditionTable::Format2(_))
		));
		Ok(())
	}

	#[test]
	fn test_lookup_variations_full_instance_does_not_modify_shared_feature() -> TestResult {
		let mut font = lookup_variations_font()?;
		let gsub = font.require::<Gsub>()?;
		let list = gsub.feature_list.as_mut().ok_or("no FeatureList")?;
		let shared_feature = list.feature_record[0].feature.clone();
		list.feature_record.push(ot::FeatureRecord {
			feature_tag: Tag::new(b"test"),
			feature: shared_feature.clone(),
		});
		list.feature_count = list.feature_count.map(|c| c + 1);

		instantiate_feature_variations(
			&mut font,
			&normalized(&[("wght", &[0.3]), ("wdth", &[0.0])]),
		)?;

		let gsub = font.require::<Gsub>()?;
		let list = gsub.feature_list.as_ref().ok_or("no FeatureList")?;
		// Instead of comparing objects: the shared feature stays as it was in the second
		// record only.
		assert_ne!(list.feature_record[0].feature, shared_feature);
		assert_eq!(list.feature_record[1].feature, shared_feature);
		let indices = shared_feature.ok_or("no Feature")?.lookup_list_index;
		assert_eq!(
			get_substitutions(&mut font, &indices)?,
			subs(&[("A", "A.alt")])
		);
		Ok(())
	}

	#[test]
	fn test_lookup_variations_full_instance_replaces_default_lookups() -> TestResult {
		let mut font = lookup_variations_font()?;
		gsub_lookup_conditions(&mut font)?.flags = 0;

		instantiate_feature_variations(
			&mut font,
			&normalized(&[("wght", &[0.3]), ("wdth", &[0.0])]),
		)?;

		let lookup_indices = first_feature_lookups(&mut font)?;
		assert_eq!(
			get_substitutions(&mut font, &lookup_indices)?,
			subs(&[("B", "B.alt"), ("D", "D.alt")])
		);
		Ok(())
	}

	#[test]
	fn test_lookup_variations_use_current_feature_lookups() -> TestResult {
		let mut font = lookup_variations_font()?;
		let variations = gsub_variations(&mut font)?;
		let lookup_variation = &variations.lookup_variation_record[0];
		let conditional_c = lookup_variation
			.feature_lookups_table
			.as_ref()
			.ok_or("no FeatureLookupsTable")?
			.lookup_condition_record[1]
			.lookup_index_list
			.as_ref()
			.ok_or("no LookupIndexList")?
			.lookup_index
			.clone();
		let substitution = substitution_record(lookup_variation.feature_index, &conditional_c);
		variations.feature_variation_record = vec![variation_record(
			range_condition(0.25, 1.0, 0),
			substitution,
		)];
		variations.feature_variation_count = Some(1);

		instantiate_feature_variations(
			&mut font,
			&normalized(&[("wght", &[0.3]), ("wdth", &[0.0])]),
		)?;

		assert_no_feature_variations(&mut font)?;
		let lookup_indices = first_feature_lookups(&mut font)?;
		assert_eq!(
			get_substitutions(&mut font, &lookup_indices)?,
			subs(&[("B", "B.alt"), ("C", "C.alt"), ("D", "D.alt")])
		);
		Ok(())
	}

	#[test]
	fn test_lookup_variations_condition_value_partial_instance() -> TestResult {
		let mut font = lookup_variations_font_with_store()?;

		instantiate_pins(&mut font, &[("wght", 0.3)])?;

		let table = gsub_lookup_conditions(&mut font)?;
		let Some(ot::ConditionTable::Format4(compound)) =
			table.lookup_condition_record[1].condition_table.as_deref()
		else {
			return Err("not format 4".into());
		};
		let Some(ot::ConditionTable::Format2(value_condition)) =
			compound.condition_table[0].as_deref()
		else {
			return Err("not format 2".into());
		};
		assert_eq!(value_condition.default_value, -1);
		assert_eq!(value_condition.var_idx, 0);
		font.table_data(Tag::new(b"GDEF"))?;
		compile_gsub(&mut font)?;
		Ok(())
	}

	#[test]
	fn test_lookup_variations_condition_value_bakes_delta() -> TestResult {
		let mut font = lookup_variations_font_with_store()?;

		instantiate_pins(&mut font, &[("wdth", 0.8)])?;

		let table = gsub_lookup_conditions(&mut font)?;
		assert_eq!(table.lookup_condition_count, Some(2));
		let Some(ot::ConditionTable::Format2(condition)) =
			table.lookup_condition_record[0].condition_table.as_deref()
		else {
			return Err("not format 2".into());
		};
		assert_eq!(condition.default_value, 1);
		assert_eq!(u64::from(condition.var_idx), NO_VARIATION_INDEX);
		Ok(())
	}

	#[test]
	fn test_lookup_variations_condition_value_full_instance() -> TestResult {
		let mut font = lookup_variations_font_with_store()?;

		instantiate_pins(&mut font, &[("wght", 0.3), ("wdth", 0.8)])?;

		assert_no_feature_variations(&mut font)?;
		let lookup_indices = first_feature_lookups(&mut font)?;
		assert_eq!(
			get_substitutions(&mut font, &lookup_indices)?,
			subs(&[("A", "A.alt"), ("C", "C.alt"), ("D", "D.alt")])
		);
		Ok(())
	}

	/// `substitutions` of `test_partial_instance_preserves_rule_substitutions`.
	fn rule_substitutions(font: &mut Font, location: &[(&str, f64)]) -> TestResult<Subs> {
		let axes = axis_tags(font)?;
		let at = location_of(location);
		let gsub = font.require::<Gsub>()?;
		let list = gsub.feature_list.as_ref().ok_or("no FeatureList")?;
		let mut feature = list.feature_record[0]
			.feature
			.as_ref()
			.ok_or("no Feature")?;
		if let Some(variations) = &gsub.feature_variations {
			for record in &variations.feature_variation_record {
				let matches = record.condition_set.as_ref().is_none_or(|set| {
					set.condition_table.iter().flatten().all(|c| {
						let ot::ConditionTable::Format1(c) = &**c else {
							panic!("not format 1");
						};
						let value = at
							.get(&axes[usize::from(c.axis_index)])
							.copied()
							.unwrap_or(0.0);
						c.filter_range_min_value <= value && value <= c.filter_range_max_value
					})
				});
				if matches {
					feature = record
						.feature_table_substitution
						.as_ref()
						.ok_or("no FeatureTableSubstitution")?
						.substitution_record[0]
						.feature
						.as_ref()
						.ok_or("no Feature")?;
					break;
				}
			}
		}
		let indices = feature.lookup_list_index.clone();
		get_substitutions(font, &indices)
	}

	#[test]
	fn test_partial_instance_preserves_rule_substitutions() -> TestResult {
		for pinned_axis in ["wght", "wdth"] {
			let mut font = fixture("feature_vars_rules")?;
			let mut original = reload(&mut font)?;
			let value = if pinned_axis == "wght" { 900.0 } else { 200.0 };
			let mut partial = instantiated(&original, &[(pinned_axis, pin(value))])?;
			let mut partial = reload(&mut partial)?;
			let remaining_axis = if pinned_axis == "wght" {
				"wdth"
			} else {
				"wght"
			};
			for value in [-1.0, 0.0, 0.5, 0.8, 1.0] {
				assert_eq!(
					rule_substitutions(&mut partial, &[(remaining_axis, value)])?,
					rule_substitutions(
						&mut original,
						&[(pinned_axis, 1.0), (remaining_axis, value)]
					)?,
					"{pinned_axis} {value}"
				);
			}
		}
		Ok(())
	}

	fn condition_set_as_dict(
		set: Option<&ot::ConditionSet>,
		axis_order: &[String],
	) -> TestResult<BTreeMap<String, (f64, f64)>> {
		let mut result = BTreeMap::new();
		if let Some(set) = set {
			for condition in &set.condition_table {
				let Some(ot::ConditionTable::Format1(c)) = condition.as_deref() else {
					return Err("not format 1".into());
				};
				result.insert(
					axis_order[usize::from(c.axis_index)].clone(),
					(c.filter_range_min_value, c.filter_range_max_value),
				);
			}
		}
		Ok(result)
	}

	fn conditions(pairs: &[(&str, (f64, f64))]) -> BTreeMap<String, (f64, f64)> {
		pairs.iter().map(|(t, r)| ((*t).to_owned(), *r)).collect()
	}

	#[test]
	fn test_partial_instance() -> TestResult {
		const DOLLAR: (&str, &str) = ("uni0024", "uni0024.nostroke");
		const UPPER: (&str, &str) = ("uni0041", "uni0061");
		const LOWER: (&str, &str) = ("uni0061", "uni0041");
		const WGHT_HIGH: (&str, (f64, f64)) = ("wght", (0.20886, 1.0));
		const WGHT_LOW: (&str, (f64, f64)) = ("wght", (-1.0, -0.45654));
		const CNTR_HIGH: (&str, (f64, f64)) = ("cntr", (0.75, 1.0));
		const CNTR_LOW: (&str, (f64, f64)) = ("cntr", (0.0, 0.25));
		type Record<'a> = (&'a [(&'a str, (f64, f64))], &'a [(&'a str, &'a str)]);
		type Case<'a> = (
			(&'a str, &'a [f64]),
			&'a [(&'a str, &'a str)],
			&'a [Record<'a>],
		);
		let cases: [Case<'_>; 8] = [
			(("wght", &[0.0]), &[], &[(&[CNTR_HIGH], &[UPPER])]),
			(
				("wght", &[-1.0]),
				&[LOWER],
				&[
					(&[CNTR_LOW], &[LOWER]),
					(&[CNTR_HIGH], &[UPPER]),
					(&[], &[]),
				],
			),
			(
				("wght", &[1.0]),
				&[DOLLAR],
				&[(&[CNTR_HIGH], &[DOLLAR, UPPER]), (&[], &[DOLLAR])],
			),
			(
				("cntr", &[0.0]),
				&[],
				&[(&[WGHT_LOW], &[LOWER]), (&[WGHT_HIGH], &[DOLLAR])],
			),
			(
				("cntr", &[1.0]),
				&[UPPER],
				&[(&[WGHT_HIGH], &[DOLLAR, UPPER]), (&[], &[UPPER])],
			),
			(
				("cntr", &[-0.5, 0.0, 1.0]),
				&[],
				&[
					(&[WGHT_HIGH, CNTR_HIGH], &[DOLLAR, UPPER]),
					(&[WGHT_LOW, CNTR_LOW], &[LOWER]),
					(&[CNTR_HIGH], &[UPPER]),
					(&[WGHT_HIGH], &[DOLLAR]),
				],
			),
			(
				("cntr", &[0.8, 0.9, 1.0]),
				&[UPPER],
				&[(&[WGHT_HIGH], &[DOLLAR, UPPER]), (&[], &[UPPER])],
			),
			(
				("cntr", &[0.7, 0.9, 1.0]),
				&[UPPER],
				&[
					(
						&[("cntr", (-0.7499999999999999, 1.0)), WGHT_HIGH],
						&[DOLLAR, UPPER],
					),
					(&[("cntr", (-0.7499999999999999, 1.0))], &[UPPER]),
					(&[WGHT_HIGH], &[DOLLAR]),
					(&[], &[]),
				],
			),
		];
		for ((axis, location), applied_subs, expected_records) in cases {
			let mut font = three_rules_font()?;

			instantiate_feature_variations(&mut font, &normalized(&[(axis, location)]))?;

			let mut axis_order = axis_tags(&mut font)?;
			if location.len() == 1 {
				axis_order.retain(|t| t != axis);
			}
			let variations = gsub_variations(&mut font)?.clone();
			assert_eq!(
				variations.feature_variation_count,
				Some(expected_records.len() as u32)
			);
			for (i, (expected_condition_set, expected_subs)) in expected_records.iter().enumerate()
			{
				let record = &variations.feature_variation_record[i];
				let condition_set =
					condition_set_as_dict(record.condition_set.as_deref(), &axis_order)?;
				assert_eq!(
					condition_set,
					conditions(expected_condition_set),
					"{axis} {i}"
				);

				let lookup_indices = &record
					.feature_table_substitution
					.as_ref()
					.ok_or("no FeatureTableSubstitution")?
					.substitution_record[0]
					.feature
					.as_ref()
					.ok_or("no Feature")?
					.lookup_list_index;
				assert_eq!(
					get_substitutions(&mut font, lookup_indices)?,
					subs(expected_subs),
					"{axis} {i}"
				);
			}
			let applied = first_feature_lookups(&mut font)?;
			assert_eq!(get_substitutions(&mut font, &applied)?, subs(applied_subs));
		}
		Ok(())
	}

	#[test]
	fn test_full_instance() -> TestResult {
		const DOLLAR: (&str, &str) = ("uni0024", "uni0024.nostroke");
		const UPPER: (&str, &str) = ("uni0041", "uni0061");
		const LOWER: (&str, &str) = ("uni0061", "uni0041");
		let cases: [(f64, f64, &[(&str, &str)]); 6] = [
			(0.0, 0.0, &[]),
			(-1.0, 0.0, &[LOWER]),
			(1.0, 0.0, &[DOLLAR]),
			(0.0, 1.0, &[UPPER]),
			(1.0, 1.0, &[UPPER, DOLLAR]),
			(-1.0, 0.3, &[]),
		];
		for (wght, cntr, applied_subs) in cases {
			let mut font = three_rules_font()?;
			let gsub = font.require::<Gsub>()?;
			assert!(gsub.feature_variations.is_some());
			assert_eq!(gsub.version, 0x0001_0001);

			instantiate_feature_variations(
				&mut font,
				&normalized(&[("wght", &[wght]), ("cntr", &[cntr])]),
			)?;

			let gsub = font.require::<Gsub>()?;
			assert!(gsub.feature_variations.is_none());
			assert_eq!(gsub.version, 0x0001_0000);

			if applied_subs.is_empty() {
				assert!(
					gsub.feature_list
						.as_ref()
						.ok_or("no FeatureList")?
						.feature_record
						.is_empty()
				);
			} else {
				let lookup_indices = first_feature_lookups(&mut font)?;
				assert_eq!(
					get_substitutions(&mut font, &lookup_indices)?,
					subs(applied_subs)
				);
			}
		}
		Ok(())
	}

	#[test]
	fn test_null_conditionset() -> TestResult {
		// A null ConditionSet offset should be treated like an empty ConditionTable, i.e. all
		// contexts are matched; see https://github.com/fonttools/fonttools/issues/3211
		for null_conditionset in [false, true] {
			let mut font = fixture("feature_vars_null_conditionset")?;
			let record = &mut gsub_variations(&mut font)?.feature_variation_record[0];
			record.condition_set = if null_conditionset {
				None
			} else {
				Some(Box::new(ot::ConditionSet {
					condition_count: Some(1),
					condition_table: vec![None],
				}))
			};

			instantiate_feature_variations(&mut font, &normalized(&[("wght", &[0.5])]))?;

			let gsub = font.require::<Gsub>()?;
			assert!(gsub.feature_variations.is_none());
			assert_eq!(gsub.version, 0x0001_0000);

			let lookup_indices = first_feature_lookups(&mut font)?;
			assert_eq!(
				get_substitutions(&mut font, &lookup_indices)?,
				subs(&[("uni0024", "uni0024.nostroke")])
			);
		}
		Ok(())
	}

	#[test]
	fn test_null_substitution() -> TestResult {
		let cases: [(bool, f64, (&str, &str)); 7] = [
			(false, 0.0, ("A.alt", "A")),
			(false, 0.375, ("A", "A.alt")),
			(false, 0.75, ("A.alt", "A")),
			(true, -0.5, ("A.alt", "A")),
			(true, -0.25, ("A", "A.alt")),
			(true, 0.0, ("A.alt", "A")),
			(true, 0.5, ("A.alt", "A")),
		];
		for explicit_empty in [false, true] {
			for (partial, coordinate, expected) in cases {
				let mut font = fixture("feature_vars_null_substitution")?;
				let variations = gsub_variations(&mut font)?;
				let mut first = variations.feature_variation_record[0].clone();
				let set = first.condition_set.as_mut().ok_or("no ConditionSet")?;
				let Some(ot::ConditionTable::Format1(condition)) =
					set.condition_table[0].as_deref_mut()
				else {
					return Err("not format 1".into());
				};
				condition.filter_range_min_value = 0.5;
				if explicit_empty {
					let substitution = first
						.feature_table_substitution
						.as_mut()
						.ok_or("no FeatureTableSubstitution")?;
					substitution.substitution_record = Vec::new();
					substitution.substitution_count = Some(0);
				} else {
					first.feature_table_substitution = None;
				}
				variations.feature_variation_record.insert(0, first);
				variations.feature_variation_count = Some(2);
				recompile::<Gsub>(&mut font)?;

				if partial {
					instantiate_feature_variations(
						&mut font,
						&normalized(&[("wght", &[-1.0, 0.75, 1.0])]),
					)?;
					let variations = gsub_variations(&mut font)?;
					assert_eq!(variations.feature_variation_count, Some(2));
					let first = &variations.feature_variation_record[0];
					if explicit_empty {
						assert_eq!(
							first
								.feature_table_substitution
								.as_ref()
								.ok_or("no FeatureTableSubstitution")?
								.substitution_count,
							Some(0)
						);
					} else {
						assert!(first.feature_table_substitution.is_none());
					}
					recompile::<Gsub>(&mut font)?;
				}

				instantiate_feature_variations(&mut font, &normalized(&[("wght", &[coordinate])]))?;
				assert_no_feature_variations(&mut font)?;
				let indices = first_feature_lookups(&mut font)?;
				assert_eq!(
					get_substitutions(&mut font, &indices)?,
					subs(&[expected]),
					"{explicit_empty} {partial} {coordinate}"
				);
			}
		}
		Ok(())
	}

	#[test]
	fn test_unsupported_condition_format() -> TestResult {
		for full_font in [false, true] {
			for partial in [false, true] {
				let mut font = fixture("feature_vars_unsupported_condition")?;
				let record = &mut gsub_variations(&mut font)?.feature_variation_record[0];
				let set = record.condition_set.as_mut().ok_or("no ConditionSet")?;
				set.condition_table[0] = Some(Box::new(ot::ConditionTable::Unknown(99)));
				let record = record.clone();

				let result = if full_font {
					let location: &[(&str, f64)] = if partial {
						&[("wght", 900.0)]
					} else {
						&[("wght", 900.0), ("wdth", 70.0)]
					};
					instantiate_pins(&mut font, location).map_err(|e| e.to_string())
				} else {
					let location = if partial {
						normalized(&[("wght", &[1.0])])
					} else {
						normalized(&[("wght", &[1.0]), ("wdth", &[-1.0])])
					};
					instantiate_feature_variations(&mut font, &location).map_err(|e| e.to_string())
				};
				let error = result.err().ok_or("no error")?;
				assert!(
					error.contains("GSUB FeatureVariations condition format 99"),
					"{error}"
				);

				// Rejection happens before either entry point starts mutating the font.
				assert_eq!(
					gsub_variations(&mut font)?.feature_variation_record[0],
					record
				);
			}
		}
		Ok(())
	}

	#[test]
	fn test_gsub_feature_variations_is_none() -> TestResult {
		let mut font = varfont2()?;
		let gsub = font.require::<Gsub>()?;
		gsub.version = 0x0001_0001;
		gsub.feature_variations = None;
		let mut varfont = reload(&mut font)?;

		// DO NOT raise an exception when the optional 'FeatureVariations' attribute is present
		// but is set to None (e.g. with GSUB 1.1); skip and do nothing.
		assert_no_feature_variations(&mut varfont)?;
		// The limits are not normalized in fontTools either, nor checked here.
		let unnormalized = |v: f64| NormalizedAxisTriple {
			minimum: v,
			default: v,
			maximum: v,
			distance_negative: 1.0,
			distance_positive: 1.0,
		};
		let limits = NormalizedLimits(
			[
				("wght".to_owned(), unnormalized(400.0)),
				("wdth".to_owned(), unnormalized(100.0)),
			]
			.into_iter()
			.collect(),
		);
		instantiate_feature_variations(&mut varfont, &limits)?;
		assert_no_feature_variations(&mut varfont)?;
		Ok(())
	}

	#[test]
	fn test_extended_layout_header() -> TestResult {
		for lookup_variations in [false, true] {
			for table_tag in ["GSUB", "GPOS"] {
				for partial in [false, true] {
					// The fonts of the test, with its changes for the GPOS case.
					let name = match (table_tag, lookup_variations) {
						("GSUB", false) => "extended_layout_gsub_0",
						("GSUB", true) => "lookup_variations",
						(_, false) => "extended_layout_gpos_0",
						(_, true) => "extended_layout_gpos_1",
					};
					let mut font = fixture(name)?;
					let wght: &[f64] = if partial {
						&[0.25, 0.3, 0.75]
					} else if lookup_variations {
						&[0.3]
					} else {
						&[0.75]
					};
					let location = if lookup_variations {
						normalized(&[("wght", wght), ("wdth", &[0.8])])
					} else {
						normalized(&[("wght", wght)])
					};
					let mut reference = font.clone();
					let tables = [table_tag];
					let options = ConvertOptions {
						tables: Some(&tables),
						..Default::default()
					};
					upper_tables(&mut font, &options)?;

					for target in [&mut reference, &mut font] {
						instantiate_feature_variations(target, &location)?;
					}
					let (version, variations) = if table_tag == "GSUB" {
						let table = font.require::<Gsub>()?;
						(table.version, table.feature_variations.is_some())
					} else {
						let table = font.require::<Gpos>()?;
						(table.version, table.feature_variations.is_some())
					};
					assert_eq!(version, 0x0001_0002);
					assert_eq!(variations, partial, "{name} {partial}");
					lower_tables(&mut font, &options)?;
					assert_eq!(
						to_ttx(&mut font, &[table_tag])?,
						to_ttx(&mut reference, &[table_tag])?,
						"{name} {partial}"
					);
				}
			}
		}
		Ok(())
	}
}
