//! The mergers that `fontTools.varLib.merger` registers for some types:
//! `AligningMerger` (which aligns the glyphs of the subtables of the masters),
//! `VariationMerger` and `InstancerMerger`.

use std::collections::BTreeSet;

use fontoxide_misc::round_tools::ot_round;
use fontoxide_ttlib::beyond64k::convert_layout_formats;
use fontoxide_ttlib::binary::fit;
use fontoxide_ttlib::ot::conv::ValueRecord;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::types::{ClassDef, Coverage, LookupSubTable as St};
use indexmap::IndexMap;

use super::walk::{
	Walk, check_same_format, merge_eq, merge_eq_list, merge_list, merge_opt, merge_struct,
	repr_debug, repr_int, with_attr, with_index,
};
use super::{MResult, Merger, MergerKind, colr};
use crate::builder::build_var_dev_table;
use crate::errors::{MergeError, MergeErrorKind, VarLibError};
use crate::instancer::otl::{align_matrices, effective_format, new_value_record};
use crate::models::VariationModel;

fn attribute_error(name: &str) -> VarLibError {
	fontoxide_ttlib::Error::ttlib(format!(
		"AttributeError: 'NoneType' object has no attribute '{name}'"
	))
	.into()
}

fn assertion(message: &str) -> VarLibError {
	fontoxide_ttlib::Error::Assertion(message.to_owned()).into()
}

fn glyphs_of(c: &Option<Box<Coverage>>) -> MResult<&Vec<u32>> {
	Ok(&c
		.as_deref()
		.ok_or_else(|| attribute_error("glyphs"))?
		.glyphs)
}

fn coverage(glyphs: Vec<u32>) -> Option<Box<Coverage>> {
	Some(Box::new(Coverage { glyphs }))
}

/// `_merge_GlyphOrders(font, lst, values_lst)`: the combined glyphs and, for
/// each list, the index of the value of each glyph (that of its last
/// occurrence, as a `dict` keeps it). `value_counts` limits the values of each
/// list, as `zip` does; `None` takes the glyphs as a set.
#[allow(clippy::type_complexity)]
fn merge_glyph_orders(
	lists: &[&[u32]],
	value_counts: Option<&[usize]>,
) -> MResult<(Vec<u32>, Vec<Vec<Option<usize>>>)> {
	let mut maps: Vec<std::collections::HashMap<u32, usize>> = Vec::with_capacity(lists.len());
	for (i, list) in lists.iter().enumerate() {
		let count = value_counts.map_or(list.len(), |c| c[i].min(list.len()));
		maps.push(
			list[..count]
				.iter()
				.enumerate()
				.map(|(j, g)| (*g, j))
				.collect(),
		);
	}
	let combined: BTreeSet<u32> = maps.iter().flat_map(|m| m.keys().copied()).collect();
	// Make sure all input glyphsets were in proper order.
	if !lists.iter().all(|l| l.is_sorted()) {
		return Err(MergeError::new(MergeErrorKind::InconsistentGlyphOrder).into());
	}
	let order: Vec<u32> = combined.into_iter().collect();
	let padded = maps
		.iter()
		.map(|m| order.iter().map(|g| m.get(g).copied()).collect())
		.collect();
	Ok((order, padded))
}

/// `buildVarDevTable(store_builder, master_values)` of `varLib.merger`.
fn build_var_dev(m: &mut Merger<'_>, values: &[f64]) -> MResult<(f64, Option<ot::Device>)> {
	if values.iter().all(|v| *v == values[0]) {
		return Ok((values[0], None));
	}
	let (base, var_idx) = m.store_builder.store_masters(values)?;
	Ok((base, Some(build_var_dev_table(fit(var_idx, "VarIdx")?))))
}

/// `otRound(model.interpolateFromValuesAndScalars(values, masterScalars))`.
fn interpolate(m: &Merger<'_>, values: &[f64]) -> MResult<i64> {
	let v = VariationModel::interpolate_from_values_and_scalars(values, &m.master_scalars)?
		.ok_or_else(|| {
			fontoxide_ttlib::Error::ttlib(
				"TypeError: type NoneType doesn't define __round__ method",
			)
		})?;
	Ok(ot_round(v))
}

fn i16_of(v: f64) -> MResult<i16> {
	Ok(fit(v as i64, "Short")?)
}

// ----- ValueRecord -----

const VALUE_FIELDS: [&str; 4] = ["XAdvance", "YAdvance", "XPlacement", "YPlacement"];

fn value_field<'a>(
	v: &'a mut ValueRecord,
	name: &str,
) -> (&'a mut Option<i16>, &'a mut Option<Option<Box<ot::Device>>>) {
	match name {
		"XAdvance" => (&mut v.x_advance, &mut v.x_adv_device),
		"YAdvance" => (&mut v.y_advance, &mut v.y_adv_device),
		"XPlacement" => (&mut v.x_placement, &mut v.x_pla_device),
		_ => (&mut v.y_placement, &mut v.y_pla_device),
	}
}

fn value_get(v: &ValueRecord, name: &str) -> f64 {
	f64::from(
		match name {
			"XAdvance" => v.x_advance,
			"YAdvance" => v.y_advance,
			"XPlacement" => v.x_placement,
			_ => v.y_placement,
		}
		.unwrap_or(0),
	)
}

impl Walk for ValueRecord {
	fn py_type(&self) -> &'static str {
		"ValueRecord"
	}
	fn py_keys(&self) -> &'static [&'static str] {
		&[]
	}
	fn attr_names(&self) -> &'static [&'static str] {
		&[]
	}
	fn merge_attr(&mut self, _m: &mut Merger<'_>, _lst: &[&Self], _attr: &str) -> MResult {
		Ok(())
	}
	fn merge_object(&mut self, m: &mut Merger<'_>, lst: &[&Self], _exclude: &[&str]) -> MResult {
		merge_eq(m, self, lst, repr_debug)
	}
	fn merge_dispatch(&mut self, m: &mut Merger<'_>, lst: &[&Self]) -> MResult {
		match m.kind {
			MergerKind::Instancer => {
				// The merger of `InstancerMerger`.
				for name in VALUE_FIELDS {
					let values: Vec<f64> = lst.iter().map(|a| value_get(a, name)).collect();
					let (field, device) = value_field(self, name);
					if device.is_some() {
						return Err(assertion("not hasattr(self, tableName)"));
					}
					if field.is_some() {
						*field = Some(fit(interpolate(m, &values)?, "ValueRecord")?);
					}
				}
				Ok(())
			}
			_ => {
				// The merger of `VariationMerger`.
				for name in VALUE_FIELDS {
					if value_field(self, name).0.is_none() {
						continue;
					}
					let values: Vec<f64> = lst.iter().map(|a| value_get(a, name)).collect();
					let (value, device) = build_var_dev(m, &values)?;
					let (field, device_field) = value_field(self, name);
					*field = Some(i16_of(value)?);
					if let Some(d) = device {
						*device_field = Some(Some(Box::new(d)));
					}
				}
				Ok(())
			}
		}
	}
}

// ----- Custom types of ttlib -----

impl Walk for Coverage {
	fn py_type(&self) -> &'static str {
		"Coverage"
	}
	fn py_keys(&self) -> &'static [&'static str] {
		&["glyphs"]
	}
	fn attr_names(&self) -> &'static [&'static str] {
		&["glyphs"]
	}
	fn merge_attr(&mut self, m: &mut Merger<'_>, lst: &[&Self], attr: &str) -> MResult {
		if attr == "glyphs" {
			let glyphs: Vec<&Vec<u32>> = lst.iter().map(|l| &l.glyphs).collect();
			merge_eq_list(m, &self.glyphs, &glyphs, repr_int)?;
		}
		Ok(())
	}
	fn merge_object(&mut self, m: &mut Merger<'_>, lst: &[&Self], exclude: &[&str]) -> MResult {
		if !exclude.contains(&"glyphs") {
			with_attr(m, "glyphs", |m| self.merge_attr(m, lst, "glyphs"))?;
		}
		Ok(())
	}
}

macro_rules! walk_eq {
	($ty:ty, $name:literal, $attr:literal, $field:ident) => {
		impl Walk for $ty {
			fn py_type(&self) -> &'static str {
				$name
			}
			fn py_keys(&self) -> &'static [&'static str] {
				&[$attr]
			}
			fn attr_names(&self) -> &'static [&'static str] {
				&[$attr]
			}
			fn merge_attr(&mut self, m: &mut Merger<'_>, lst: &[&Self], attr: &str) -> MResult {
				if attr == $attr {
					let values: Vec<_> = lst.iter().map(|l| &l.$field).collect();
					merge_eq(m, &self.$field, &values, repr_debug)?;
				}
				Ok(())
			}
			fn merge_object(
				&mut self,
				m: &mut Merger<'_>,
				lst: &[&Self],
				exclude: &[&str],
			) -> MResult {
				if !exclude.contains(&$attr) {
					with_attr(m, $attr, |m| self.merge_attr(m, lst, $attr))?;
				}
				Ok(())
			}
		}
	};
}

walk_eq!(ClassDef, "ClassDef", "classDefs", class_defs);
walk_eq!(ot::SingleSubst, "SingleSubst", "mapping", mapping);
walk_eq!(ot::MultipleSubst, "MultipleSubst", "mapping", mapping);
walk_eq!(
	ot::AlternateSubst,
	"AlternateSubst",
	"alternates",
	alternates
);
walk_eq!(ot::LigatureSubst, "LigatureSubst", "ligatures", ligatures);
walk_eq!(ot::DeltaSetIndexMap, "DeltaSetIndexMap", "mapping", mapping);
walk_eq!(ot::VarIdxMap, "VarIdxMap", "mapping", mapping);

impl Walk for ot::ClipList {
	fn py_type(&self) -> &'static str {
		"ClipList"
	}
	fn py_keys(&self) -> &'static [&'static str] {
		&["Format", "clips"]
	}
	fn attr_names(&self) -> &'static [&'static str] {
		&["Format", "clips"]
	}
	fn merge_attr(&mut self, m: &mut Merger<'_>, lst: &[&Self], attr: &str) -> MResult {
		match attr {
			"Format" => {
				let values: Vec<_> = lst.iter().map(|l| &l.format).collect();
				merge_eq(m, &self.format, &values, repr_debug)
			}
			"clips" => colr::merge_clips(self, m, lst),
			_ => Ok(()),
		}
	}
	fn merge_object(&mut self, m: &mut Merger<'_>, lst: &[&Self], exclude: &[&str]) -> MResult {
		for key in ["Format", "clips"] {
			if !exclude.contains(&key) {
				with_attr(m, key, |m| self.merge_attr(m, lst, key))?;
			}
		}
		Ok(())
	}
}

// ----- GDEF -----

/// The merger of `GDEF.GlyphClassDef` and `GDEF.GlyphClassDef2`: the union of the
/// classes of the masters, which must not conflict.
pub(crate) fn gdef_glyph_class_def(
	m: &mut Merger<'_>,
	out: &mut Option<Box<ClassDef>>,
	lst: &[&Option<Box<ClassDef>>],
) -> MResult {
	let Some(out) = out else {
		if lst.iter().all(|l| l.is_none()) {
			return Ok(());
		}
		return Err(m.not_a_none(lst.iter().map(|l| l.as_ref().map(|_| "ClassDef")).collect()));
	};
	let defs: Vec<&ClassDef> = lst
		.iter()
		.map(|l| l.as_deref().ok_or_else(|| attribute_error("classDefs")))
		.collect::<MResult<_>>()?;
	let mut all_keys = BTreeSet::new();
	for d in &defs {
		all_keys.extend(d.class_defs.keys().copied());
	}
	out.class_defs.clear();
	for k in all_keys {
		let values: Vec<u32> = defs
			.iter()
			.filter_map(|d| d.class_defs.get(&k).copied())
			.collect();
		if values.iter().any(|v| *v != values[0]) {
			// The values are compared with the class definitions of the masters,
			// so the first master is the offender.
			return Err(m
				.should_be_constant(
					values[0].to_string(),
					defs.iter()
						.map(|d| (false, repr_debug(&d.class_defs)))
						.collect(),
				)
				.push_stack(format!(".{}", m.glyph_name(k))));
		}
		out.class_defs.insert(k, values[0]);
	}
	Ok(())
}

// ----- SinglePos -----

fn single_pos_format(st: &ot::SinglePos) -> MResult<u16> {
	match st {
		ot::SinglePos::Format1(t) => Ok(t.value_format),
		ot::SinglePos::Format2(t) => Ok(t.value_format),
		_ => Err(attribute_error("ValueFormat")),
	}
}

fn single_pos_coverage(st: &ot::SinglePos) -> MResult<&Vec<u32>> {
	match st {
		ot::SinglePos::Format1(t) => glyphs_of(&t.coverage),
		ot::SinglePos::Format2(t) => glyphs_of(&t.coverage),
		_ => Err(attribute_error("Coverage")),
	}
}

/// `_SinglePosUpgradeToFormat2`.
fn single_pos_upgrade(st: &ot::SinglePos) -> MResult<ot::SinglePosFormat2> {
	match st {
		ot::SinglePos::Format2(t) => Ok(t.clone()),
		ot::SinglePos::Format1(t) => {
			let n = glyphs_of(&t.coverage)?.len();
			Ok(ot::SinglePosFormat2 {
				coverage: t.coverage.clone(),
				value_format: t.value_format,
				value_count: Some(n as u32),
				value: vec![t.value.clone(); n],
				dont_share: false,
			})
		}
		_ => Err(attribute_error("Coverage")),
	}
}

/// `_Lookup_SinglePos_get_effective_value`.
fn single_pos_effective_value(
	m: &Merger<'_>,
	subtables: &[Option<Box<St>>],
	glyph: u32,
) -> MResult<Option<ValueRecord>> {
	for st in subtables {
		let Some(St::SinglePos(st)) = st.as_deref() else {
			continue;
		};
		match st {
			ot::SinglePos::Format1(t) => {
				if t.coverage
					.as_ref()
					.is_some_and(|c| c.glyphs.contains(&glyph))
				{
					return Ok(t.value.clone());
				}
			}
			ot::SinglePos::Format2(t) => {
				let Some(c) = &t.coverage else {
					continue;
				};
				if let Some(i) = c.glyphs.iter().position(|g| *g == glyph) {
					return Ok(t.value.get(i).cloned().flatten());
				}
			}
			_ => {
				let covered = match st {
					ot::SinglePos::Format3(t) => {
						t.coverage.as_ref().map(|c| c.glyphs.contains(&glyph))
					}
					ot::SinglePos::Format4(t) => {
						t.coverage.as_ref().map(|c| c.glyphs.contains(&glyph))
					}
					_ => None,
				};
				if covered == Some(true) {
					return Err(m.unsupported_format("single positioning lookup", None));
				}
			}
		}
	}
	Ok(None)
}

/// The merger of `ot.SinglePos`.
pub(crate) fn single_pos(
	out: &mut ot::SinglePos,
	m: &mut Merger<'_>,
	lst: &[&ot::SinglePos],
) -> MResult {
	let mut value_format = 0;
	for l in lst {
		value_format |= single_pos_format(l)?;
	}
	match out {
		ot::SinglePos::Format1(t) => t.value_format = value_format,
		ot::SinglePos::Format2(t) => t.value_format = value_format,
		_ => {}
	}
	if !(lst.len() == 1 || value_format & !0xF == 0) {
		return Err(m.unsupported_format("single positioning lookup", None));
	}

	// If all have same coverage table and all are format 1,
	let coverage_glyphs = single_pos_coverage(out)?.clone();
	let mut all_same = true;
	for l in lst {
		if !matches!(l, ot::SinglePos::Format1(_)) || *single_pos_coverage(l)? != coverage_glyphs {
			all_same = false;
			break;
		}
	}
	if all_same && let ot::SinglePos::Format1(t) = out {
		let mut value = new_value_record(value_format, t.value.as_ref());
		if value_format != 0 {
			// If v.Value is None, it means a kerning of 0; we want it to
			// participate in the model still.
			let values: Vec<ValueRecord> = lst
				.iter()
				.map(|l| match l {
					ot::SinglePos::Format1(v) => v.value.clone().unwrap_or_default(),
					_ => ValueRecord::default(),
				})
				.collect();
			let refs: Vec<&ValueRecord> = values.iter().collect();
			merge_struct(m, &mut value, &refs)?;
		}
		t.value_format = value.format();
		t.value = Some(value);
		return Ok(());
	}

	// Upgrade everything to Format=2.
	let lst: Vec<ot::SinglePosFormat2> = lst
		.iter()
		.map(|l| single_pos_upgrade(l))
		.collect::<MResult<_>>()?;
	let lists: Vec<&[u32]> = lst
		.iter()
		.map(|l| glyphs_of(&l.coverage).map(Vec::as_slice))
		.collect::<MResult<_>>()?;
	let counts: Vec<usize> = lst.iter().map(|l| l.value.len()).collect();
	let (glyphs, padded) = merge_glyph_orders(&lists, Some(&counts))?;

	let mut values_out: Vec<ValueRecord> = glyphs
		.iter()
		.map(|_| new_value_record(value_format, None))
		.collect();
	let mut padded_values: Vec<Vec<ValueRecord>> = Vec::with_capacity(lst.len());
	for (i, indices) in padded.iter().enumerate() {
		let mut row = Vec::with_capacity(glyphs.len());
		for (j, glyph) in glyphs.iter().enumerate() {
			let present = indices[j].and_then(|k| lst[i].value[k].clone());
			let v = match present {
				Some(v) => v,
				None => {
					// Fill in value from other subtables.
					let subtables = m.lookup_subtables.get(i).cloned().unwrap_or_default();
					single_pos_effective_value(m, &subtables, *glyph)?
						.unwrap_or_else(|| new_value_record(value_format, None))
				}
			};
			row.push(v);
		}
		padded_values.push(row);
	}
	let refs: Vec<&Vec<ValueRecord>> = padded_values.iter().collect();
	merge_list(m, &mut values_out, &refs)?;
	let mut format = 0;
	for v in &values_out {
		format |= effective_format(v);
	}
	let old_coverage = match out {
		ot::SinglePos::Format1(t) => t.coverage.take(),
		ot::SinglePos::Format2(t) => t.coverage.take(),
		_ => None,
	};
	let _ = old_coverage;
	*out = ot::SinglePos::Format2(ot::SinglePosFormat2 {
		coverage: coverage(glyphs),
		value_format: format,
		value_count: Some(values_out.len() as u32),
		value: values_out.into_iter().map(Some).collect(),
		dont_share: false,
	});
	Ok(())
}

// ----- PairPos -----

/// `_Lookup_PairPos_get_effective_value_pair`.
fn pair_pos_effective_value_pair(
	m: &Merger<'_>,
	subtables: &[Option<Box<St>>],
	first: u32,
	second: u32,
) -> MResult<Option<(Option<ValueRecord>, Option<ValueRecord>)>> {
	for st in subtables {
		let Some(St::PairPos(st)) = st.as_deref() else {
			continue;
		};
		match st {
			ot::PairPos::Format1(t) => {
				let Some(c) = &t.coverage else {
					continue;
				};
				let Some(i) = c.glyphs.iter().position(|g| *g == first) else {
					continue;
				};
				let ps = t
					.pair_set
					.get(i)
					.ok_or_else(|| {
						fontoxide_ttlib::Error::ttlib("IndexError: list index out of range")
					})?
					.as_deref()
					.ok_or_else(|| attribute_error("PairValueRecord"))?;
				for rec in &ps.pair_value_record {
					if rec.second_glyph == second {
						return Ok(Some((rec.value1.clone(), rec.value2.clone())));
					}
				}
			}
			ot::PairPos::Format2(t) => {
				let Some(c) = &t.coverage else {
					continue;
				};
				if !c.glyphs.contains(&first) {
					continue;
				}
				let class = |d: &Option<Box<ClassDef>>, g: u32| -> MResult<usize> {
					let d = d.as_deref().ok_or_else(|| attribute_error("classDefs"))?;
					Ok(d.class_defs.get(&g).copied().unwrap_or(0) as usize)
				};
				let k1 = class(&t.class_def1, first)?;
				let k2 = class(&t.class_def2, second)?;
				let rec = t
					.class1_record
					.get(k1)
					.and_then(|r| r.class2_record.get(k2))
					.ok_or_else(|| {
						fontoxide_ttlib::Error::ttlib("IndexError: list index out of range")
					})?;
				return Ok(Some((rec.value1.clone(), rec.value2.clone())));
			}
			_ => {
				let covered = match st {
					ot::PairPos::Format3(t) => {
						t.coverage.as_ref().map(|c| c.glyphs.contains(&first))
					}
					ot::PairPos::Format4(t) => {
						t.coverage.as_ref().map(|c| c.glyphs.contains(&first))
					}
					_ => None,
				};
				if covered == Some(true) {
					return Err(m.unsupported_format("pair positioning lookup", None));
				}
			}
		}
	}
	Ok(None)
}

/// `otBase.ValueRecord(format, src=src) if format else None`.
fn value_or_none(format: u16, src: Option<&ValueRecord>) -> Option<ValueRecord> {
	(format != 0).then(|| new_value_record(format, src))
}

/// The merger of `ot.PairSet`.
pub(crate) fn pair_set(out: &mut ot::PairSet, m: &mut Merger<'_>, lst: &[&ot::PairSet]) -> MResult {
	let (vf1, vf2) = m.value_formats;
	let first = m
		.first_glyph
		.ok_or_else(|| attribute_error("_firstGlyph"))?;
	let seconds: Vec<Vec<u32>> = lst
		.iter()
		.map(|l| l.pair_value_record.iter().map(|r| r.second_glyph).collect())
		.collect();
	let lists: Vec<&[u32]> = seconds.iter().map(Vec::as_slice).collect();
	let counts: Vec<usize> = lst.iter().map(|l| l.pair_value_record.len()).collect();
	let (glyphs, padded) = merge_glyph_orders(&lists, Some(&counts))?;

	out.pair_value_record = glyphs
		.iter()
		.map(|g| ot::PairValueRecord {
			second_glyph: *g,
			value1: value_or_none(vf1, None),
			value2: value_or_none(vf2, None),
		})
		.collect();
	out.pair_value_count = Some(out.pair_value_record.len() as u32);

	let mut padded_values: Vec<Vec<ot::PairValueRecord>> = Vec::with_capacity(lst.len());
	for (i, indices) in padded.iter().enumerate() {
		let mut row = Vec::with_capacity(glyphs.len());
		for (j, glyph) in glyphs.iter().enumerate() {
			// Fill in value from other subtables.
			let vpair = match indices[j] {
				Some(k) => {
					let r = &lst[i].pair_value_record[k];
					Some((r.value1.clone(), r.value2.clone()))
				}
				None => {
					let subtables = m.lookup_subtables.get(i).cloned().unwrap_or_default();
					pair_pos_effective_value_pair(m, &subtables, first, *glyph)?
				}
			};
			let (v1, v2) = vpair.unwrap_or((None, None));
			row.push(ot::PairValueRecord {
				second_glyph: *glyph,
				value1: value_or_none(vf1, v1.as_ref()),
				value2: value_or_none(vf2, v2.as_ref()),
			});
		}
		padded_values.push(row);
	}
	m.first_glyph = None;
	let refs: Vec<&Vec<ot::PairValueRecord>> = padded_values.iter().collect();
	merge_list(m, &mut out.pair_value_record, &refs)
}

/// `_PairPosFormat1_merge`.
fn pair_pos_format1_merge(
	out: &mut ot::PairPosFormat1,
	lst: &[&ot::PairPosFormat1],
	m: &mut Merger<'_>,
) -> MResult {
	let zeros: Vec<bool> = lst
		.iter()
		.filter(|l| !l.pair_set.is_empty())
		.map(|l| l.value_format2 == 0)
		.collect();
	if zeros.iter().any(|z| *z != zeros[0]) {
		return Err(assertion("Report bug against fonttools."));
	}
	let empty = ot::PairSet {
		pair_value_count: Some(0),
		pair_value_record: Vec::new(),
	};
	let lists: Vec<&[u32]> = lst
		.iter()
		.map(|l| glyphs_of(&l.coverage).map(Vec::as_slice))
		.collect::<MResult<_>>()?;
	let counts: Vec<usize> = lst.iter().map(|l| l.pair_set.len()).collect();
	let (glyphs, padded) = merge_glyph_orders(&lists, Some(&counts))?;
	let mut sets: Vec<Vec<&ot::PairSet>> = Vec::with_capacity(lst.len());
	for (i, indices) in padded.iter().enumerate() {
		let mut row = Vec::with_capacity(glyphs.len());
		for k in indices {
			row.push(match k {
				Some(k) => lst[i].pair_set[*k]
					.as_deref()
					.ok_or_else(|| attribute_error("PairValueRecord"))?,
				None => &empty,
			});
		}
		sets.push(row);
	}
	out.coverage = coverage(glyphs.clone());
	out.pair_set = glyphs
		.iter()
		.map(|_| Some(Box::new(ot::PairSet::default())))
		.collect();
	out.pair_set_count = Some(out.pair_set.len() as u32);
	{
		for (j, glyph) in glyphs.iter().enumerate() {
			let values: Vec<&ot::PairSet> = sets.iter().map(|row| row[j]).collect();
			let Some(ps) = out.pair_set[j].as_deref_mut() else {
				continue;
			};
			m.first_glyph = Some(*glyph);
			with_index(m, j, |m| merge_struct(m, ps, &values))?;
		}
		Ok(())
	}
}

/// `_PairPosFormat2_merge`.
fn pair_pos_format2_merge(
	out: &mut ot::PairPosFormat2,
	lst: &[&ot::PairPosFormat2],
	m: &mut Merger<'_>,
) -> MResult {
	let zeros: Vec<bool> = lst
		.iter()
		.filter(|l| !l.class1_record.is_empty())
		.map(|l| l.value_format2 == 0)
		.collect();
	if zeros.iter().any(|z| *z != zeros[0]) {
		return Err(assertion("Report bug against fonttools."));
	}
	// Align coverages.
	let lists: Vec<&[u32]> = lst
		.iter()
		.map(|l| glyphs_of(&l.coverage).map(Vec::as_slice))
		.collect::<MResult<_>>()?;
	let (glyphs, _) = merge_glyph_orders(&lists, None)?;
	out.coverage = coverage(glyphs.clone());

	// Currently, if the coverage of PairPosFormat2 subtables are different, we do
	// NOT bother walking down the subtable list when filling in new rows for
	// alignment. As such, this is only correct if current subtable is the last
	// subtable in the lookup. Ensure that.
	for (l, subtables) in lst.iter().zip(&m.lookup_subtables) {
		if *glyphs_of(&l.coverage)? != glyphs {
			let last = subtables.last().and_then(|s| s.as_deref());
			if !matches!(last, Some(St::PairPos(ot::PairPos::Format2(t))) if t == *l) {
				return Err(assertion("l == subtables[-1]"));
			}
		}
	}

	let matrices = align_matrices(out, lst, &m.order, false)?;
	let matrices: Vec<Vec<ot::Class1Record>> = matrices
		.into_iter()
		.map(|rows| {
			rows.into_iter()
				.map(|row| {
					Ok(ot::Class1Record {
						class2_record: row
							.into_iter()
							.map(|r| r.ok_or_else(|| attribute_error("Value1")))
							.collect::<MResult<_>>()?,
					})
				})
				.collect::<MResult<_>>()
		})
		.collect::<MResult<_>>()?;
	out.class1_record = matrices.first().cloned().unwrap_or_default();
	let refs: Vec<&Vec<ot::Class1Record>> = matrices.iter().collect();
	merge_list(m, &mut out.class1_record, &refs)
}

/// The merger of `ot.PairPos`.
pub(crate) fn pair_pos(out: &mut ot::PairPos, m: &mut Merger<'_>, lst: &[&ot::PairPos]) -> MResult {
	let formats = |st: &ot::PairPos| -> MResult<(u16, u16)> {
		match st {
			ot::PairPos::Format1(t) => Ok((t.value_format1, t.value_format2)),
			ot::PairPos::Format2(t) => Ok((t.value_format1, t.value_format2)),
			_ => Err(attribute_error("ValueFormat1")),
		}
	};
	let (mut vf1, mut vf2) = (0, 0);
	for l in lst {
		let (a, b) = formats(l)?;
		vf1 |= a;
		vf2 |= b;
	}
	m.value_formats = (vf1, vf2);
	match out {
		ot::PairPos::Format1(t) => {
			t.value_format1 = vf1;
			t.value_format2 = vf2;
		}
		ot::PairPos::Format2(t) => {
			t.value_format1 = vf1;
			t.value_format2 = vf2;
		}
		_ => {}
	}
	// `mergeObjects` checks that the masters have the same format.
	check_same_format(m, out, lst)?;
	match out {
		ot::PairPos::Format1(t) => {
			let l: Vec<&ot::PairPosFormat1> = lst
				.iter()
				.filter_map(|x| {
					if let ot::PairPos::Format1(v) = x {
						Some(v)
					} else {
						None
					}
				})
				.collect();
			pair_pos_format1_merge(t, &l, m)?;
		}
		ot::PairPos::Format2(t) => {
			let l: Vec<&ot::PairPosFormat2> = lst
				.iter()
				.filter_map(|x| {
					if let ot::PairPos::Format2(v) = x {
						Some(v)
					} else {
						None
					}
				})
				.collect();
			pair_pos_format2_merge(t, &l, m)?;
		}
		_ => return Err(m.unsupported_format("pair positioning lookup", None)),
	}

	// Now examine the list of value records, and update to the union of format
	// values, as merge might have created new values.
	let (mut vf1, mut vf2) = (0, 0);
	let mut add = |v1: &Option<ValueRecord>, v2: &Option<ValueRecord>| {
		if let Some(v) = v1 {
			vf1 |= v.format();
		}
		if let Some(v) = v2 {
			vf2 |= v.format();
		}
	};
	match out {
		ot::PairPos::Format1(t) => {
			for ps in t.pair_set.iter().flatten() {
				for r in &ps.pair_value_record {
					add(&r.value1, &r.value2);
				}
			}
			t.value_format1 = vf1;
			t.value_format2 = vf2;
		}
		ot::PairPos::Format2(t) => {
			for row in &t.class1_record {
				for r in &row.class2_record {
					add(&r.value1, &r.value2);
				}
			}
			t.value_format1 = vf1;
			t.value_format2 = vf2;
		}
		_ => {}
	}
	Ok(())
}

// ----- Mark attachment -----

/// A new `ot.Anchor()` with `Format = 1` and no coordinates yet.
fn new_anchor() -> ot::Anchor {
	ot::Anchor::Format1(ot::AnchorFormat1::default())
}

/// `anchor = ot.Anchor(); anchor.Format = 1; merger.mergeThings(anchor, allAnchors)`,
/// or `None` if all are `None`.
fn merge_new_anchor(
	m: &mut Merger<'_>,
	anchors: &[Option<&ot::Anchor>],
) -> MResult<Option<Box<ot::Anchor>>> {
	if anchors.iter().all(Option::is_none) {
		return Ok(None);
	}
	let mut anchor = new_anchor();
	merge_opt(m, Some(&mut anchor), anchors)?;
	Ok(Some(Box::new(anchor)))
}

/// `_MarkBasePosFormat1_merge`, for the parts that are the same in `MarkBasePos`
/// and `MarkMarkPos`: the merged mark records and base anchors.
#[allow(clippy::type_complexity)]
fn mark_base_merge(
	m: &mut Merger<'_>,
	mark_coverages: &[&[u32]],
	mark_records: &[&Vec<ot::MarkRecord>],
	base_coverages: &[&[u32]],
	base_records: &[Vec<&Vec<Option<Box<ot::Anchor>>>>],
	class_count: usize,
) -> MResult<(
	Vec<u32>,
	Vec<ot::MarkRecord>,
	Vec<u32>,
	Vec<Vec<Option<Box<ot::Anchor>>>>,
)> {
	let counts: Vec<usize> = mark_records.iter().map(|r| r.len()).collect();
	let (mark_glyphs, mark_padded) = merge_glyph_orders(mark_coverages, Some(&counts))?;
	let counts: Vec<usize> = base_records.iter().map(Vec::len).collect();
	let (base_glyphs, base_padded) = merge_glyph_orders(base_coverages, Some(&counts))?;

	// MarkArray
	let mut records = Vec::with_capacity(mark_glyphs.len());
	for j in 0..mark_glyphs.len() {
		let glyph_records: Vec<Option<&ot::MarkRecord>> = mark_padded
			.iter()
			.enumerate()
			.map(|(i, p)| p[j].map(|k| &mark_records[i][k]))
			.collect();
		let all_classes: Vec<u16> = glyph_records.iter().flatten().map(|r| r.class).collect();
		// TODO Right now we require that all marks have same class in all masters
		// that cover them. This is not required.
		if all_classes.iter().any(|c| *c != all_classes[0]) {
			return Err(m.should_be_constant(
				all_classes[0].to_string(),
				all_classes
					.iter()
					.map(|c| (*c == all_classes[0], c.to_string()))
					.collect(),
			));
		}
		let anchors: Vec<Option<&ot::Anchor>> = glyph_records
			.iter()
			.map(|r| r.and_then(|r| r.mark_anchor.as_deref()))
			.collect();
		let anchor = merge_new_anchor(m, &anchors)?;
		records.push(ot::MarkRecord {
			class: all_classes[0],
			mark_anchor: anchor,
		});
	}

	// BaseArray
	let mut bases = Vec::with_capacity(base_glyphs.len());
	for j in 0..base_glyphs.len() {
		let glyph_records: Vec<Option<&Vec<Option<Box<ot::Anchor>>>>> = base_padded
			.iter()
			.enumerate()
			.map(|(i, p)| p[j].map(|k| base_records[i][k]))
			.collect();
		if glyph_records.iter().all(Option::is_none) {
			// LENIENT(L36): fontTools makes the record None, which fails to compile.
			return Err(attribute_error("BaseAnchor"));
		}
		let mut anchors = Vec::with_capacity(class_count);
		for c in 0..class_count {
			let all: Vec<Option<&ot::Anchor>> = glyph_records
				.iter()
				.map(|r| r.and_then(|r| r.get(c)).and_then(|a| a.as_deref()))
				.collect();
			anchors.push(merge_new_anchor(m, &all)?);
		}
		// Lists longer than the class count keep their extra anchors.
		let longest = glyph_records
			.iter()
			.flatten()
			.map(|r| r.len())
			.max()
			.unwrap_or(0);
		for c in class_count..longest {
			let all: Vec<Option<&ot::Anchor>> = glyph_records
				.iter()
				.map(|r| r.and_then(|r| r.get(c)).and_then(|a| a.as_deref()))
				.collect();
			if glyph_records.iter().any(|r| r.is_none_or(|r| r.len() <= c)) {
				// `zip(*glyphAnchors)` stops at the shortest.
				break;
			}
			anchors.push(merge_new_anchor(m, &all)?);
		}
		bases.push(anchors);
	}
	Ok((mark_glyphs, records, base_glyphs, bases))
}

fn inconsistent_formats<T: Walk>(m: &Merger<'_>, out: &T, lst: &[&T], subtable: &str) -> MResult {
	let f = out.format_number();
	if lst.iter().all(|l| l.format_number() == f) {
		return Ok(());
	}
	let repr = |f: Option<u16>| f.map_or_else(|| "None".to_owned(), |f| f.to_string());
	Err(m.merge_error(
		MergeErrorKind::InconsistentFormats {
			subtable: subtable.to_owned(),
			value: None,
		},
		Some((
			repr(f),
			lst.iter()
				.map(|l| (l.format_number() == f, repr(l.format_number())))
				.collect(),
		)),
	))
}

/// The merger of `ot.MarkBasePos`.
pub(crate) fn mark_base_pos(
	out: &mut ot::MarkBasePos,
	m: &mut Merger<'_>,
	lst: &[&ot::MarkBasePos],
) -> MResult {
	inconsistent_formats(m, out, lst, "mark-to-base positioning lookup")?;
	let ot::MarkBasePos::Format1(t) = out else {
		return Err(m.unsupported_format("mark-to-base positioning lookup", None));
	};
	let l: Vec<&ot::MarkBasePosFormat1> = lst
		.iter()
		.filter_map(|x| {
			if let ot::MarkBasePos::Format1(v) = x {
				Some(v)
			} else {
				None
			}
		})
		.collect();
	let class_count = l.iter().filter_map(|l| l.class_count).max().unwrap_or(0);
	t.class_count = Some(class_count);
	let mark_coverages: Vec<&[u32]> = l
		.iter()
		.map(|l| glyphs_of(&l.mark_coverage).map(Vec::as_slice))
		.collect::<MResult<_>>()?;
	let mark_records: Vec<&Vec<ot::MarkRecord>> = l
		.iter()
		.map(|l| {
			Ok(&l
				.mark_array
				.as_deref()
				.ok_or_else(|| attribute_error("MarkRecord"))?
				.mark_record)
		})
		.collect::<MResult<_>>()?;
	let base_coverages: Vec<&[u32]> = l
		.iter()
		.map(|l| glyphs_of(&l.base_coverage).map(Vec::as_slice))
		.collect::<MResult<_>>()?;
	let base_records: Vec<Vec<&Vec<Option<Box<ot::Anchor>>>>> = l
		.iter()
		.map(|l| {
			Ok(l.base_array
				.as_deref()
				.ok_or_else(|| attribute_error("BaseRecord"))?
				.base_record
				.iter()
				.map(|r| &r.base_anchor)
				.collect())
		})
		.collect::<MResult<_>>()?;
	let (mark_glyphs, marks, base_glyphs, bases) = mark_base_merge(
		m,
		&mark_coverages,
		&mark_records,
		&base_coverages,
		&base_records,
		class_count as usize,
	)?;
	t.mark_coverage = coverage(mark_glyphs);
	t.base_coverage = coverage(base_glyphs);
	t.mark_array = Some(Box::new(ot::MarkArray {
		mark_count: Some(marks.len() as u32),
		mark_record: marks,
	}));
	t.base_array = Some(Box::new(ot::BaseArray {
		base_count: Some(bases.len() as u32),
		base_record: bases
			.into_iter()
			.map(|base_anchor| ot::BaseRecord { base_anchor })
			.collect(),
	}));
	Ok(())
}

/// The merger of `ot.MarkMarkPos`.
pub(crate) fn mark_mark_pos(
	out: &mut ot::MarkMarkPos,
	m: &mut Merger<'_>,
	lst: &[&ot::MarkMarkPos],
) -> MResult {
	inconsistent_formats(m, out, lst, "mark-to-mark positioning lookup")?;
	let ot::MarkMarkPos::Format1(t) = out else {
		return Err(m.unsupported_format("mark-to-mark positioning lookup", None));
	};
	let l: Vec<&ot::MarkMarkPosFormat1> = lst
		.iter()
		.filter_map(|x| {
			if let ot::MarkMarkPos::Format1(v) = x {
				Some(v)
			} else {
				None
			}
		})
		.collect();
	let class_count = l.iter().filter_map(|l| l.class_count).max().unwrap_or(0);
	t.class_count = Some(class_count);
	let mark_coverages: Vec<&[u32]> = l
		.iter()
		.map(|l| glyphs_of(&l.mark1_coverage).map(Vec::as_slice))
		.collect::<MResult<_>>()?;
	let mark_records: Vec<&Vec<ot::MarkRecord>> = l
		.iter()
		.map(|l| {
			Ok(&l
				.mark1_array
				.as_deref()
				.ok_or_else(|| attribute_error("MarkRecord"))?
				.mark_record)
		})
		.collect::<MResult<_>>()?;
	let base_coverages: Vec<&[u32]> = l
		.iter()
		.map(|l| glyphs_of(&l.mark2_coverage).map(Vec::as_slice))
		.collect::<MResult<_>>()?;
	let base_records: Vec<Vec<&Vec<Option<Box<ot::Anchor>>>>> = l
		.iter()
		.map(|l| {
			Ok(l.mark2_array
				.as_deref()
				.ok_or_else(|| attribute_error("Mark2Record"))?
				.mark2_record
				.iter()
				.map(|r| &r.mark2_anchor)
				.collect())
		})
		.collect::<MResult<_>>()?;
	let (mark_glyphs, marks, base_glyphs, bases) = mark_base_merge(
		m,
		&mark_coverages,
		&mark_records,
		&base_coverages,
		&base_records,
		class_count as usize,
	)?;
	t.mark1_coverage = coverage(mark_glyphs);
	t.mark2_coverage = coverage(base_glyphs);
	t.mark1_array = Some(Box::new(ot::MarkArray {
		mark_count: Some(marks.len() as u32),
		mark_record: marks,
	}));
	t.mark2_array = Some(Box::new(ot::Mark2Array {
		mark2_count: Some(bases.len() as u32),
		mark2_record: bases
			.into_iter()
			.map(|mark2_anchor| ot::Mark2Record { mark2_anchor })
			.collect(),
	}));
	Ok(())
}

// ----- CursivePos -----

/// The merger of `ot.CursivePos`.
pub(crate) fn cursive_pos(
	out: &mut ot::CursivePos,
	m: &mut Merger<'_>,
	lst: &[&ot::CursivePos],
) -> MResult {
	let l: Vec<&ot::CursivePosFormat1> = lst
		.iter()
		.map(|x| match x {
			ot::CursivePos::Format1(v) => Ok(v),
			_ => Err(attribute_error("EntryExitRecord")),
		})
		.collect::<MResult<_>>()?;
	let lists: Vec<&[u32]> = l
		.iter()
		.map(|l| glyphs_of(&l.coverage).map(Vec::as_slice))
		.collect::<MResult<_>>()?;
	let counts: Vec<usize> = l.iter().map(|l| l.entry_exit_record.len()).collect();
	let (glyphs, padded) = merge_glyph_orders(&lists, Some(&counts))?;
	let mut records: Vec<ot::EntryExitRecord> = glyphs
		.iter()
		.map(|_| ot::EntryExitRecord {
			entry_anchor: Some(Box::new(new_anchor())),
			exit_anchor: Some(Box::new(new_anchor())),
		})
		.collect();
	{
		for (j, rec) in records.iter_mut().enumerate() {
			let values: Vec<Option<&ot::EntryExitRecord>> = padded
				.iter()
				.enumerate()
				.map(|(i, p)| p[j].map(|k| &l[i].entry_exit_record[k]))
				.collect();
			with_index(m, j, |m| merge_opt(m, Some(rec), &values))?;
		}
	}
	*out = ot::CursivePos::Format1(ot::CursivePosFormat1 {
		coverage: coverage(glyphs),
		entry_exit_count: Some(records.len() as u32),
		entry_exit_record: records,
		dont_share: false,
	});
	Ok(())
}

/// The merger of `ot.EntryExitRecord`.
pub(crate) fn entry_exit_record(
	out: &mut ot::EntryExitRecord,
	m: &mut Merger<'_>,
	lst: &[&ot::EntryExitRecord],
) -> MResult {
	if lst.iter().all(|r| r.entry_anchor.is_none()) {
		out.entry_anchor = None;
	}
	if lst.iter().all(|r| r.exit_anchor.is_none()) {
		out.exit_anchor = None;
	}
	out.merge_object(m, lst, &[])
}

// ----- Lookup -----

fn extension_info(st: &St) -> Option<u16> {
	match st {
		St::ExtensionPos(ot::ExtensionPos::Format1(t)) => Some(t.extension_lookup_type),
		St::ExtensionSubst(ot::ExtensionSubst::Format1(t)) => Some(t.extension_lookup_type),
		_ => None,
	}
}

fn is_extension(st: &Option<Box<St>>) -> bool {
	matches!(
		st.as_deref(),
		Some(St::ExtensionPos(_) | St::ExtensionSubst(_))
	)
}

/// Removes the Extension subtables of a lookup.
fn unwrap_extensions(
	m: &Merger<'_>,
	lookup_type: &mut u16,
	sts: &mut [Option<Box<St>>],
) -> MResult {
	if sts.is_empty() || !is_extension(&sts[0]) {
		return Ok(());
	}
	let class = |st: &Option<Box<St>>| st.as_deref().map_or("NoneType", |s| s.py_type());
	let first = class(&sts[0]);
	if sts.iter().any(|st| class(st) != first) {
		return Err(m.merge_error(
			MergeErrorKind::InconsistentExtensions,
			Some((
				"'Extension'".to_owned(),
				sts.iter()
					.map(|st| (false, format!("'{}'", class(st))))
					.collect(),
			)),
		));
	}
	let types: Vec<Option<u16>> = sts
		.iter()
		.map(|st| st.as_deref().and_then(extension_info))
		.collect();
	if types.iter().any(|t| *t != types[0]) {
		return Err(m.merge_error(MergeErrorKind::InconsistentExtensions, None));
	}
	if let Some(t) = types[0] {
		*lookup_type = t;
	}
	for st in sts.iter_mut() {
		*st = match st.as_deref_mut() {
			Some(St::ExtensionPos(ot::ExtensionPos::Format1(t))) => t.ext_sub_table.take(),
			Some(St::ExtensionSubst(ot::ExtensionSubst::Format1(t))) => t.ext_sub_table.take(),
			_ => None,
		};
	}
	Ok(())
}

/// `_BEYOND64K_GPOS_TYPES`: `_convert_beyond64k_GPOS` of a subtable.
fn convert_beyond64k_gpos(sts: &mut [Option<Box<St>>], extended: bool) -> MResult {
	for st in sts {
		match st.as_deref_mut() {
			Some(St::SinglePos(t)) => convert_layout_formats(t, extended)?,
			Some(St::PairPos(t)) => convert_layout_formats(t, extended)?,
			Some(St::CursivePos(t)) => convert_layout_formats(t, extended)?,
			Some(St::MarkBasePos(t)) => convert_layout_formats(t, extended)?,
			Some(St::MarkMarkPos(t)) => convert_layout_formats(t, extended)?,
			_ => false,
		};
	}
	Ok(())
}

/// Whether a subtable has one of the extended formats.
fn is_extended_gpos(st: &Option<Box<St>>) -> bool {
	matches!(
		st.as_deref(),
		Some(St::SinglePos(
			ot::SinglePos::Format3(_) | ot::SinglePos::Format4(_)
		)) | Some(St::PairPos(
			ot::PairPos::Format3(_) | ot::PairPos::Format4(_)
		)) | Some(St::CursivePos(ot::CursivePos::Format2(_)))
			| Some(St::MarkBasePos(ot::MarkBasePos::Format2(_)))
			| Some(St::MarkMarkPos(ot::MarkMarkPos::Format2(_)))
	)
}

fn pair_pos_format(st: &Option<Box<St>>) -> Option<u16> {
	match st.as_deref() {
		Some(St::PairPos(p)) => p.format_number(),
		_ => None,
	}
}

/// `_PairSet_flatten`.
fn pair_set_flatten(lst: &[&ot::PairSet]) -> MResult<ot::PairSet> {
	let seconds: Vec<Vec<u32>> = lst
		.iter()
		.map(|l| l.pair_value_record.iter().map(|r| r.second_glyph).collect())
		.collect();
	let lists: Vec<&[u32]> = seconds.iter().map(Vec::as_slice).collect();
	let counts: Vec<usize> = lst.iter().map(|l| l.pair_value_record.len()).collect();
	let (glyphs, padded) = merge_glyph_orders(&lists, Some(&counts))?;
	let mut records = Vec::with_capacity(glyphs.len());
	for j in 0..glyphs.len() {
		let found = padded
			.iter()
			.enumerate()
			.find_map(|(i, p)| p[j].map(|k| lst[i].pair_value_record[k].clone()))
			.ok_or_else(|| assertion("False"))?;
		records.push(found);
	}
	Ok(ot::PairSet {
		pair_value_count: Some(records.len() as u32),
		pair_value_record: records,
	})
}

/// `_Lookup_PairPosFormat1_subtables_flatten`.
fn pair_pos_format1_flatten(lst: &[&ot::PairPosFormat1]) -> MResult<ot::PairPosFormat1> {
	let zeros: Vec<bool> = lst
		.iter()
		.filter(|l| !l.pair_set.is_empty())
		.map(|l| l.value_format2 == 0)
		.collect();
	if zeros.iter().any(|z| *z != zeros[0]) {
		return Err(assertion("Report bug against fonttools."));
	}
	let (mut vf1, mut vf2) = (0, 0);
	for l in lst {
		vf1 |= l.value_format1;
		vf2 |= l.value_format2;
	}
	let lists: Vec<&[u32]> = lst
		.iter()
		.map(|l| glyphs_of(&l.coverage).map(Vec::as_slice))
		.collect::<MResult<_>>()?;
	let counts: Vec<usize> = lst.iter().map(|l| l.pair_set.len()).collect();
	let (glyphs, padded) = merge_glyph_orders(&lists, Some(&counts))?;
	let mut pair_set = Vec::with_capacity(glyphs.len());
	for j in 0..glyphs.len() {
		let sets: Vec<&ot::PairSet> = padded
			.iter()
			.enumerate()
			.filter_map(|(i, p)| p[j].map(|k| lst[i].pair_set[k].as_deref()))
			.map(|s| s.ok_or_else(|| attribute_error("PairValueRecord")))
			.collect::<MResult<_>>()?;
		pair_set.push(Some(Box::new(pair_set_flatten(&sets)?)));
	}
	Ok(ot::PairPosFormat1 {
		coverage: coverage(glyphs),
		value_format1: vf1,
		value_format2: vf2,
		pair_set_count: Some(pair_set.len() as u32),
		pair_set,
		dont_share: false,
	})
}

/// `_Lookup_PairPosFormat2_subtables_flatten`.
fn pair_pos_format2_flatten(
	lst: &[&ot::PairPosFormat2],
	order: &fontoxide_ttlib::GlyphOrder,
) -> MResult<ot::PairPosFormat2> {
	let zeros: Vec<bool> = lst
		.iter()
		.filter(|l| !l.class1_record.is_empty())
		.map(|l| l.value_format2 == 0)
		.collect();
	if zeros.iter().any(|z| *z != zeros[0]) {
		return Err(assertion("Report bug against fonttools."));
	}
	let mut out = ot::PairPosFormat2::default();
	for l in lst {
		out.value_format1 |= l.value_format1;
		out.value_format2 |= l.value_format2;
	}
	let lists: Vec<&[u32]> = lst
		.iter()
		.map(|l| glyphs_of(&l.coverage).map(Vec::as_slice))
		.collect::<MResult<_>>()?;
	let (glyphs, _) = merge_glyph_orders(&lists, None)?;
	out.coverage = coverage(glyphs);
	let matrices = align_matrices(&mut out, lst, order, true)?;
	let num_rows = matrices.iter().map(Vec::len).min().unwrap_or(0);
	for r in 0..num_rows {
		let num_columns = matrices.iter().map(|m| m[r].len()).min().unwrap_or(0);
		let mut row = Vec::with_capacity(num_columns);
		for c in 0..num_columns {
			let column = matrices
				.iter()
				.find_map(|m| m[r][c].clone())
				.ok_or_else(|| fontoxide_ttlib::Error::ttlib("StopIteration"))?;
			row.push(column);
		}
		out.class1_record
			.push(ot::Class1Record { class2_record: row });
	}
	Ok(out)
}

/// `_Lookup_PairPos_subtables_canonicalize`: merges the Format 1 subtables at the
/// beginning, and the Format 2 subtables at the end.
fn canonicalize_pair_pos(
	subtables: &[Option<Box<St>>],
	order: &fontoxide_ttlib::GlyphOrder,
) -> MResult<Vec<Option<Box<St>>>> {
	let first_end = subtables
		.iter()
		.take_while(|s| pair_pos_format(s) == Some(1))
		.count();
	let format1: Vec<&ot::PairPosFormat1> = subtables[..first_end]
		.iter()
		.filter_map(|s| match s.as_deref() {
			Some(St::PairPos(ot::PairPos::Format1(t))) => Some(t),
			_ => None,
		})
		.collect();
	let mut lst: Vec<Option<Box<St>>> = Vec::with_capacity(subtables.len());
	lst.push(Some(Box::new(St::PairPos(ot::PairPos::Format1(
		pair_pos_format1_flatten(&format1)?,
	)))));
	lst.extend(subtables[first_end..].iter().cloned());
	let last_start = lst.len()
		- lst
			.iter()
			.rev()
			.take_while(|s| pair_pos_format(s) == Some(2))
			.count();
	let format2: Vec<&ot::PairPosFormat2> = lst[last_start..]
		.iter()
		.filter_map(|s| match s.as_deref() {
			Some(St::PairPos(ot::PairPos::Format2(t))) => Some(t),
			_ => None,
		})
		.collect();
	let flat2 = pair_pos_format2_flatten(&format2, order)?;
	lst.truncate(last_start);
	lst.push(Some(Box::new(St::PairPos(ot::PairPos::Format2(flat2)))));
	Ok(lst)
}

/// `_Lookup_SinglePos_subtables_flatten`.
fn single_pos_flatten(lst: &[Option<Box<St>>], format: u16) -> MResult<Vec<Option<Box<St>>>> {
	let subtables: Vec<&ot::SinglePos> = lst
		.iter()
		.map(|s| match s.as_deref() {
			Some(St::SinglePos(p)) => Ok(p),
			_ => Err(attribute_error("Coverage")),
		})
		.collect::<MResult<_>>()?;
	let lists: Vec<&[u32]> = subtables
		.iter()
		.map(|s| single_pos_coverage(s).map(Vec::as_slice))
		.collect::<MResult<_>>()?;
	let (glyphs, _) = merge_glyph_orders(&lists, None)?;
	let mut values: Vec<Option<ValueRecord>> = vec![None; glyphs.len()];
	let index = |g: u32| glyphs.iter().position(|x| *x == g);
	for st in &subtables {
		match st {
			ot::SinglePos::Format1(t) => {
				for g in glyphs_of(&t.coverage)? {
					if let Some(i) = index(*g) {
						values[i] = t.value.clone();
					}
				}
			}
			ot::SinglePos::Format2(t) => {
				for (j, g) in glyphs_of(&t.coverage)?.iter().enumerate() {
					if let Some(i) = index(*g) {
						values[i] = t.value.get(j).cloned().flatten();
					}
				}
			}
			_ => {}
		}
	}
	Ok(vec![Some(Box::new(St::SinglePos(ot::SinglePos::Format2(
		ot::SinglePosFormat2 {
			coverage: coverage(glyphs.clone()),
			value_format: format,
			value_count: Some(glyphs.len() as u32),
			value: values,
			dont_share: false,
		},
	))))])
}

/// The merger of `ot.Lookup`.
pub(crate) fn lookup(out: &mut ot::Lookup, m: &mut Merger<'_>, lst: &[&ot::Lookup]) -> MResult {
	// The masters' lookups are changed in fontTools; they are copied here.
	let mut lookup_types: Vec<u16> = lst.iter().map(|l| l.lookup_type).collect();
	let mut subtables: Vec<Vec<Option<Box<St>>>> =
		lst.iter().map(|l| l.sub_table.clone()).collect();

	// Remove Extension subtables.
	for (lt, sts) in lookup_types.iter_mut().zip(subtables.iter_mut()) {
		unwrap_extensions(m, lt, sts)?;
	}
	unwrap_extensions(m, &mut out.lookup_type, &mut out.sub_table)?;

	// Beyond-64k: demote extended GPOS subtables to format 1/2 so the mergers can
	// handle them; the merged output is promoted back at the end. Explicit
	// extended formats remain valid below the 64k glyph threshold.
	let extended = m.extended
		|| out.sub_table.iter().any(is_extended_gpos)
		|| subtables.iter().flatten().any(is_extended_gpos);
	if extended {
		convert_beyond64k_gpos(&mut out.sub_table, false)?;
		for sts in &mut subtables {
			convert_beyond64k_gpos(sts, false)?;
		}
	}

	let first = out.sub_table.first().and_then(|s| s.as_deref());
	let is_pair_pos = matches!(first, Some(St::PairPos(_)));
	let is_single_pos = matches!(first, Some(St::SinglePos(_)));
	let mut flattened = false;
	if is_pair_pos {
		// AFDKO and feaLib sometimes generate two Format1 subtables instead of
		// one. Merge those before continuing.
		out.sub_table = canonicalize_pair_pos(&out.sub_table, &m.order)?;
		subtables = subtables
			.iter()
			.map(|st| canonicalize_pair_pos(st, &m.order))
			.collect::<MResult<_>>()?;
	} else if is_single_pos {
		let num: Vec<usize> = subtables.iter().map(Vec::len).collect();
		if num.iter().any(|n| *n != num[0]) {
			// Flatten list of SinglePos subtables to single Format 2 subtable, with
			// all value records set to the rec format type.
			let mut format = 0;
			let mut any = false;
			for st in subtables.iter().flatten() {
				if let Some(St::SinglePos(p)) = st.as_deref() {
					format |= single_pos_format(p)?;
					any = true;
				}
			}
			if !any {
				return Err(fontoxide_ttlib::Error::ttlib(
					"TypeError: reduce() of empty iterable with no initial value",
				)
				.into());
			}
			out.sub_table = single_pos_flatten(&out.sub_table, format)?;
			subtables = subtables
				.iter()
				.map(|st| single_pos_flatten(st, format))
				.collect::<MResult<_>>()?;
			flattened = true;
		}
	}
	m.lookup_subtables = subtables.clone();

	let refs: Vec<&Vec<Option<Box<St>>>> = subtables.iter().collect();
	super::walk::merge_opt_list(m, &mut out.sub_table, &refs)?;
	out.sub_table_count = Some(out.sub_table.len() as u32);

	if is_pair_pos {
		let empty = |st: Option<&Option<Box<St>>>, format: u16| -> MResult<bool> {
			match st.and_then(|s| s.as_deref()) {
				Some(St::PairPos(ot::PairPos::Format1(t))) if format == 1 => {
					Ok(glyphs_of(&t.coverage)?.is_empty())
				}
				Some(St::PairPos(ot::PairPos::Format2(t))) if format == 2 => {
					Ok(glyphs_of(&t.coverage)?.is_empty())
				}
				_ => Err(assertion(&format!(
					"len(self.SubTable) >= 1 and self.SubTable[...].Format == {format}"
				))),
			}
		};
		// If format-1 subtable created during canonicalization is empty, remove it.
		if empty(out.sub_table.first(), 1)? {
			out.sub_table.remove(0);
		}
		// If format-2 subtable created during canonicalization is empty, remove it.
		if empty(out.sub_table.last(), 2)? {
			out.sub_table.pop();
		}
		out.sub_table_count = Some(out.sub_table.len() as u32);

		// Compact the merged subtables.
		if m.compression_level != 0 {
			log::info!("Compacting GPOS...");
			let glyph_map = fontoxide_otllib::GlyphMap::new(m.order.reverse_map().clone());
			fontoxide_otllib::optimize::gpos::compact_lookup_with_map(
				&glyph_map,
				m.compression_level,
				out,
			)?;
		}
	} else if is_single_pos && flattened {
		let Some(St::SinglePos(ot::SinglePos::Format2(t))) =
			out.sub_table.first().and_then(|s| s.as_deref())
		else {
			return Err(assertion("singlePosTable.Format == 2"));
		};
		let mut mapping: IndexMap<String, ValueRecord> = IndexMap::new();
		for (g, v) in glyphs_of(&t.coverage)?.iter().zip(&t.value) {
			mapping.insert(m.glyph_name(*g), v.clone().unwrap_or_default());
		}
		let glyph_map = fontoxide_otllib::GlyphMap::new(m.order.reverse_map().clone());
		out.sub_table = fontoxide_otllib::builder::build_single_pos(&mapping, &glyph_map)?
			.into_iter()
			.map(|p| Some(Box::new(St::SinglePos(p))))
			.collect();
	}

	let masters: Vec<ot::Lookup> = lst
		.iter()
		.zip(&lookup_types)
		.map(|(l, t)| ot::Lookup {
			lookup_type: *t,
			lookup_flag: l.lookup_flag,
			sub_table_count: None,
			sub_table: Vec::new(),
			mark_filtering_set: l.mark_filtering_set,
		})
		.collect();
	let refs: Vec<&ot::Lookup> = masters.iter().collect();
	out.merge_object(m, &refs, &["SubTable", "SubTableCount"])?;

	// Beyond-64k: promote the merged output back to the extended formats.
	if extended {
		convert_beyond64k_gpos(&mut out.sub_table, true)?;
	}
	m.lookup_subtables = Vec::new();
	Ok(())
}

// ----- Variable values -----

/// The merger of `ot.Anchor`.
pub(crate) fn anchor(out: &mut ot::Anchor, m: &mut Merger<'_>, lst: &[&ot::Anchor]) -> MResult {
	let coords = |a: &ot::Anchor| -> (f64, f64) {
		match a {
			ot::Anchor::Format1(t) => (t.x_coordinate.into(), t.y_coordinate.into()),
			ot::Anchor::Format2(t) => (t.x_coordinate.into(), t.y_coordinate.into()),
			ot::Anchor::Format3(t) => (t.x_coordinate.into(), t.y_coordinate.into()),
			ot::Anchor::Unknown(_) => (0.0, 0.0),
		}
	};
	let xs: Vec<f64> = lst.iter().map(|a| coords(a).0).collect();
	let ys: Vec<f64> = lst.iter().map(|a| coords(a).1).collect();
	match m.kind {
		MergerKind::Instancer => {
			let ot::Anchor::Format1(t) = out else {
				return Err(assertion("self.Format == 1"));
			};
			t.x_coordinate = fit(interpolate(m, &xs)?, "XCoordinate")?;
			t.y_coordinate = fit(interpolate(m, &ys)?, "YCoordinate")?;
			Ok(())
		}
		_ => {
			let ot::Anchor::Format1(t) = out else {
				return Err(m.unsupported_format("an anchor", None));
			};
			let (x, x_dev) = build_var_dev(m, &xs)?;
			let (y, y_dev) = build_var_dev(m, &ys)?;
			t.x_coordinate = i16_of(x)?;
			t.y_coordinate = i16_of(y)?;
			if x_dev.is_some() || y_dev.is_some() {
				*out = ot::Anchor::Format3(ot::AnchorFormat3 {
					x_coordinate: i16_of(x)?,
					y_coordinate: i16_of(y)?,
					x_device_table: x_dev.map(Box::new),
					y_device_table: y_dev.map(Box::new),
				});
			}
			Ok(())
		}
	}
}

/// The merger of `ot.CaretValue`.
pub(crate) fn caret_value(
	out: &mut ot::CaretValue,
	m: &mut Merger<'_>,
	lst: &[&ot::CaretValue],
) -> MResult {
	let coordinate = |c: &ot::CaretValue| -> MResult<f64> {
		match c {
			ot::CaretValue::Format1(t) => Ok(t.coordinate.into()),
			ot::CaretValue::Format3(t) => Ok(t.coordinate.into()),
			_ => Err(attribute_error("Coordinate")),
		}
	};
	let values: Vec<f64> = lst.iter().map(|c| coordinate(c)).collect::<MResult<_>>()?;
	match m.kind {
		MergerKind::Instancer => {
			let ot::CaretValue::Format1(t) = out else {
				return Err(assertion("self.Format == 1"));
			};
			t.coordinate = fit(interpolate(m, &values)?, "Coordinate")?;
			Ok(())
		}
		_ => {
			let ot::CaretValue::Format1(t) = out else {
				return Err(m.unsupported_format("a caret", None));
			};
			let (value, dev) = build_var_dev(m, &values)?;
			t.coordinate = i16_of(value)?;
			if let Some(dev) = dev {
				*out = ot::CaretValue::Format3(ot::CaretValueFormat3 {
					coordinate: i16_of(value)?,
					device_table: Some(Box::new(dev)),
				});
			}
			Ok(())
		}
	}
}

/// The merger of `ot.BaseCoord`.
pub(crate) fn base_coord(
	out: &mut ot::BaseCoord,
	m: &mut Merger<'_>,
	lst: &[&ot::BaseCoord],
) -> MResult {
	if m.kind == MergerKind::Instancer {
		return out.merge_object(m, lst, &[]);
	}
	let ot::BaseCoord::Format1(t) = out else {
		return Err(m.unsupported_format("a baseline coordinate", None));
	};
	let coordinate = |c: &ot::BaseCoord| -> MResult<f64> {
		match c {
			ot::BaseCoord::Format1(t) => Ok(t.coordinate.into()),
			ot::BaseCoord::Format2(t) => Ok(t.coordinate.into()),
			ot::BaseCoord::Format3(t) => Ok(t.coordinate.into()),
			ot::BaseCoord::Format4(t) => Ok(t.coordinate.into()),
			ot::BaseCoord::Unknown(_) => Err(attribute_error("Coordinate")),
		}
	};
	let values: Vec<f64> = lst.iter().map(|c| coordinate(c)).collect::<MResult<_>>()?;
	let (value, dev) = build_var_dev(m, &values)?;
	t.coordinate = i16_of(value)?;
	if let Some(dev) = dev {
		*out = ot::BaseCoord::Format3(ot::BaseCoordFormat3 {
			coordinate: i16_of(value)?,
			device_table: Some(Box::new(dev)),
		});
	}
	Ok(())
}

// ----- COLR -----

pub(crate) use colr::{
	affine2x3, base_glyph_list, clip_box, color_line, color_stop, layer_list, paint,
};
