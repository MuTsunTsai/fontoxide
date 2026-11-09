//! Instancing of `GDEF`, `GPOS`, `MATH`, `JSTF` and `BASE`: `instantiateOTL`, and
//! what `MutatorMerger` (`fontTools.varLib.merger`) does with one font.
//!
//! The merger walks the tables and adds the default deltas to the values with
//! device tables. It reattaches lookups that were split, and rebuilds parts of the
//! subtables on the way, so that those must be rebuilt here as well.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use fontoxide_misc::round_tools::ot_round;
use fontoxide_ttlib::beyond64k::convert_layout_formats;
use fontoxide_ttlib::binary::fit;
use fontoxide_ttlib::ot::conv::ValueRecord;
use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::types::{ClassDef, Coverage, LookupSubTable as St};
use fontoxide_ttlib::ot::var_store;
use fontoxide_ttlib::ot::varc::NO_VARIATION_INDEX;
use fontoxide_ttlib::tables::{Base, Gdef, Gpos, Gsub, Jstf, Math};
use fontoxide_ttlib::{Error, Font, GlyphOrder, Result, Tag};

use super::feature_vars::{for_each_condition, instantiate_condition_values};
use super::item_store::{DefaultDeltas, TupleVarStore, instantiate_item_variation_store};
use super::limits::{NormalizedLimits, fvar_axes};

const VARIATION_INDEX_DEVICE: u16 = 0x8000;

/// `MutatorMerger`.
struct Mutator<'a> {
	deltas: &'a DefaultDeltas,
	delete_variations: bool,
	order: &'a GlyphOrder,
}

/// `_BEYOND64K_GPOS_TYPES`: `_convert_beyond64k_GPOS` of a subtable.
fn convert_beyond64k_gpos(st: &mut Option<Box<St>>, extended: bool) -> Result<()> {
	match st.as_deref_mut() {
		Some(St::SinglePos(t)) => convert_layout_formats(t, extended)?,
		Some(St::PairPos(t)) => convert_layout_formats(t, extended)?,
		Some(St::CursivePos(t)) => convert_layout_formats(t, extended)?,
		Some(St::MarkBasePos(t)) => convert_layout_formats(t, extended)?,
		Some(St::MarkMarkPos(t)) => convert_layout_formats(t, extended)?,
		_ => false,
	};
	Ok(())
}

/// Whether a subtable has one of the extended formats that the merger handles.
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

fn inconsistent_glyph_order() -> Error {
	Error::ttlib("InconsistentGlyphOrder")
}

fn no_attribute(name: &str) -> Error {
	Error::ttlib(format!(
		"AttributeError: 'NoneType' object has no attribute '{name}'"
	))
}

fn glyphs_of(c: &Option<Box<Coverage>>) -> Result<&Vec<u32>> {
	Ok(&c.as_deref().ok_or_else(|| no_attribute("glyphs"))?.glyphs)
}

/// The check of `_merge_GlyphOrders`.
fn check_order(glyphs: &[u32]) -> Result<()> {
	if glyphs.is_sorted() {
		Ok(())
	} else {
		Err(inconsistent_glyph_order())
	}
}

fn coverage(glyphs: Vec<u32>) -> Option<Box<Coverage>> {
	Some(Box::new(Coverage { glyphs }))
}

impl Mutator<'_> {
	fn device_delta(&self, device: &ot::Device) -> Result<i64> {
		let index = (u64::from(device.start_size) << 16) + u64::from(device.end_size);
		Ok(ot_round(self.deltas.get(index)?))
	}

	/// The merger of `otBase.ValueRecord`.
	fn value_record(&self, value: &mut ValueRecord) -> Result<()> {
		let apply =
			|field: &mut Option<i16>, device: &mut Option<Option<Box<ot::Device>>>| -> Result<()> {
				let Some(dev) = device else {
					return Ok(());
				};
				if dev
					.as_ref()
					.is_some_and(|d| d.delta_format != VARIATION_INDEX_DEVICE)
				{
					return Ok(());
				}
				let delta = match dev {
					Some(d) => Some(self.device_delta(d)?),
					None => None,
				};
				if self.delete_variations {
					*device = None;
				}
				if let Some(delta) = delta {
					*field = Some(fit(i64::from(field.unwrap_or(0)) + delta, "ValueRecord")?);
				}
				Ok(())
			};
		apply(&mut value.x_advance, &mut value.x_adv_device)?;
		apply(&mut value.y_advance, &mut value.y_adv_device)?;
		apply(&mut value.x_placement, &mut value.x_pla_device)?;
		apply(&mut value.y_placement, &mut value.y_pla_device)
	}

	/// The merger of `ot.Anchor`.
	fn anchor(&self, anchor: &mut ot::Anchor) -> Result<()> {
		let ot::Anchor::Format3(a) = anchor else {
			return Ok(());
		};
		let apply = |coordinate: &mut i16, device: &mut Option<Box<ot::Device>>| -> Result<()> {
			if device
				.as_ref()
				.is_some_and(|d| d.delta_format != VARIATION_INDEX_DEVICE)
			{
				return Ok(());
			}
			if let Some(d) = device.as_ref() {
				*coordinate = fit(i64::from(*coordinate) + self.device_delta(d)?, "Coordinate")?;
			}
			if self.delete_variations {
				*device = None;
			}
			Ok(())
		};
		apply(&mut a.x_coordinate, &mut a.x_device_table)?;
		apply(&mut a.y_coordinate, &mut a.y_device_table)?;
		if self.delete_variations && a.x_device_table.is_none() && a.y_device_table.is_none() {
			*anchor = ot::Anchor::Format1(ot::AnchorFormat1 {
				x_coordinate: a.x_coordinate,
				y_coordinate: a.y_coordinate,
			});
		}
		Ok(())
	}

	/// The merger of `ot.CaretValue` and `ot.BaseCoord`: the new coordinate if the
	/// value is static now.
	fn coordinate(
		&self,
		coordinate: &mut i16,
		device: &mut Option<Box<ot::Device>>,
	) -> Result<Option<i16>> {
		if device
			.as_ref()
			.is_some_and(|d| d.delta_format != VARIATION_INDEX_DEVICE)
		{
			return Ok(None);
		}
		if let Some(d) = device.as_ref() {
			*coordinate = fit(i64::from(*coordinate) + self.device_delta(d)?, "Coordinate")?;
		}
		if self.delete_variations {
			*device = None;
			return Ok(Some(*coordinate));
		}
		Ok(None)
	}

	/// The merger of `ot.MathValueRecord`.
	fn math_value(&self, value: &mut ot::MathValueRecord) -> Result<()> {
		let Some(device) = value
			.device_table
			.as_ref()
			.filter(|d| d.delta_format == VARIATION_INDEX_DEVICE)
		else {
			return Ok(());
		};
		value.value = fit(i64::from(value.value) + self.device_delta(device)?, "Value")?;
		if self.delete_variations {
			value.device_table = None;
		}
		Ok(())
	}

	/// `otBase.ValueRecord(format, src)`, then merged.
	fn merged_value(&self, format: u16, src: Option<&ValueRecord>) -> Result<ValueRecord> {
		let mut value = new_value_record(format, src);
		self.value_record(&mut value)?;
		Ok(value)
	}
}

/// `otBase.ValueRecord(valueFormat, src)`.
fn new_value_record(format: u16, src: Option<&ValueRecord>) -> ValueRecord {
	let bit = |mask: u16| format & mask != 0;
	let int = |mask: u16, from: Option<Option<i16>>| bit(mask).then(|| from.flatten().unwrap_or(0));
	let device = |mask: u16, from: Option<Option<Option<Box<ot::Device>>>>| {
		bit(mask).then(|| from.flatten().flatten())
	};
	let mut value = ValueRecord {
		x_placement: int(0x0001, src.map(|s| s.x_placement)),
		y_placement: int(0x0002, src.map(|s| s.y_placement)),
		x_advance: int(0x0004, src.map(|s| s.x_advance)),
		y_advance: int(0x0008, src.map(|s| s.y_advance)),
		x_pla_device: device(0x0010, src.map(|s| s.x_pla_device.clone())),
		y_pla_device: device(0x0020, src.map(|s| s.y_pla_device.clone())),
		x_adv_device: device(0x0040, src.map(|s| s.x_adv_device.clone())),
		y_adv_device: device(0x0080, src.map(|s| s.y_adv_device.clone())),
		..Default::default()
	};
	for (i, field) in value.reserved.iter_mut().enumerate() {
		if bit(0x0100 << i) {
			*field = Some(src.and_then(|s| s.reserved[i]).unwrap_or(0));
		}
	}
	value
}

/// `getEffectiveFormat`.
fn effective_format(value: &ValueRecord) -> u16 {
	let mut format = 0;
	let mut int = |mask: u16, v: Option<i16>| {
		if v.is_some_and(|v| v != 0) {
			format |= mask;
		}
	};
	int(0x0001, value.x_placement);
	int(0x0002, value.y_placement);
	int(0x0004, value.x_advance);
	int(0x0008, value.y_advance);
	for (mask, device) in [
		(0x0010, &value.x_pla_device),
		(0x0020, &value.y_pla_device),
		(0x0040, &value.x_adv_device),
		(0x0080, &value.y_adv_device),
	] {
		if matches!(device, Some(Some(_))) {
			format |= mask;
		}
	}
	for (i, v) in value.reserved.iter().enumerate() {
		if v.is_some_and(|v| v != 0) {
			format |= 0x0100 << i;
		}
	}
	format
}

/// `_Lookup_SinglePos_get_effective_value`.
fn single_pos_effective_value(
	subtables: &[Option<Box<St>>],
	glyph: u32,
) -> Result<Option<ValueRecord>> {
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
					return t
						.value
						.get(i)
						.cloned()
						.ok_or_else(|| Error::ttlib("IndexError: list index out of range"));
				}
			}
			_ => {}
		}
	}
	Ok(None)
}

/// The merger of `ot.SinglePos`.
fn merge_single_pos(
	m: &Mutator<'_>,
	subtables: &mut [Option<Box<St>>],
	index: usize,
) -> Result<()> {
	let glyphs_and_values = match subtables[index].as_deref() {
		Some(St::SinglePos(ot::SinglePos::Format2(t))) => {
			let glyphs = glyphs_of(&t.coverage)?.clone();
			check_order(&glyphs)?;
			Some((glyphs, t.value.clone(), t.value_format))
		}
		_ => None,
	};
	let filled = match glyphs_and_values {
		Some((glyphs, values, format)) => {
			let mut out = Vec::with_capacity(glyphs.len());
			for (j, glyph) in glyphs.iter().enumerate() {
				// Fill in missing values from the other subtables.
				let value = match values.get(j).cloned().flatten() {
					Some(v) => v,
					None => single_pos_effective_value(subtables, *glyph)?
						.unwrap_or_else(|| new_value_record(format, None)),
				};
				out.push(value);
			}
			Some(out)
		}
		None => None,
	};
	let Some(St::SinglePos(st)) = subtables[index].as_deref_mut() else {
		return Ok(());
	};
	match st {
		ot::SinglePos::Format1(t) => {
			let format = t.value_format;
			let mut value = new_value_record(format, t.value.as_ref());
			if format != 0 {
				m.value_record(&mut value)?;
			}
			t.value_format = value.format();
			t.value = Some(value);
		}
		ot::SinglePos::Format2(t) => {
			let mut format = 0;
			let mut values = filled.unwrap_or_default();
			for value in &mut values {
				m.value_record(value)?;
				format |= effective_format(value);
			}
			t.value_count = Some(values.len() as u32);
			t.value = values.into_iter().map(Some).collect();
			t.value_format = format;
		}
		_ => {}
	}
	Ok(())
}

/// `_PairSet_flatten`.
fn pair_set_flatten(sets: &[&ot::PairSet]) -> Result<ot::PairSet> {
	let mut records: BTreeMap<u32, ot::PairValueRecord> = BTreeMap::new();
	for set in sets {
		let seconds: Vec<u32> = set
			.pair_value_record
			.iter()
			.map(|r| r.second_glyph)
			.collect();
		check_order(&seconds)?;
		// Within a set the last of a glyph wins, among the sets the first.
		let mut own = BTreeMap::new();
		for record in &set.pair_value_record {
			own.insert(record.second_glyph, record.clone());
		}
		for (glyph, record) in own {
			records.entry(glyph).or_insert(record);
		}
	}
	let pair_value_record: Vec<_> = records.into_values().collect();
	Ok(ot::PairSet {
		pair_value_count: Some(pair_value_record.len() as u32),
		pair_value_record,
	})
}

/// `_Lookup_PairPosFormat1_subtables_flatten`.
fn pair_pos_format1_flatten(list: &[&ot::PairPosFormat1]) -> Result<ot::PairPosFormat1> {
	let (mut format1, mut format2) = (0, 0);
	let mut by_glyph: BTreeMap<u32, Vec<&ot::PairSet>> = BTreeMap::new();
	for l in list {
		format1 |= l.value_format1;
		format2 |= l.value_format2;
		let glyphs = glyphs_of(&l.coverage)?;
		check_order(glyphs)?;
		let mut own = BTreeMap::new();
		for (glyph, set) in glyphs.iter().zip(&l.pair_set) {
			own.insert(
				*glyph,
				set.as_deref()
					.ok_or_else(|| no_attribute("PairValueRecord"))?,
			);
		}
		for (glyph, set) in own {
			by_glyph.entry(glyph).or_default().push(set);
		}
	}
	let mut glyphs = Vec::with_capacity(by_glyph.len());
	let mut pair_set = Vec::with_capacity(by_glyph.len());
	for (glyph, sets) in by_glyph {
		glyphs.push(glyph);
		pair_set.push(Some(Box::new(pair_set_flatten(&sets)?)));
	}
	Ok(ot::PairPosFormat1 {
		coverage: coverage(glyphs),
		value_format1: format1,
		value_format2: format2,
		pair_set_count: Some(pair_set.len() as u32),
		pair_set,
		dont_share: false,
	})
}

/// `classifyTools.Classifier`: the classes, sorted by decreasing size, then by the
/// sorted glyph names.
fn classify(
	sets: impl IntoIterator<Item = BTreeSet<u32>>,
	order: &GlyphOrder,
) -> Vec<BTreeSet<u32>> {
	let mut classes: Vec<BTreeSet<u32>> = Vec::new();
	let mut mapping: HashMap<u32, usize> = HashMap::new();
	for set in sets {
		if set.is_empty() {
			continue;
		}
		let mut by_class: BTreeMap<usize, BTreeSet<u32>> = BTreeMap::new();
		let mut new = BTreeSet::new();
		for g in set {
			match mapping.get(&g) {
				Some(&class) => {
					by_class.entry(class).or_default().insert(g);
				}
				None => {
					new.insert(g);
				}
			}
		}
		if !new.is_empty() {
			for &g in &new {
				mapping.insert(g, classes.len());
			}
			classes.push(new);
		}
		for (class, part) in by_class {
			for g in &part {
				classes[class].remove(g);
				mapping.insert(*g, classes.len());
			}
			classes.push(part);
		}
	}
	let mut keyed: Vec<(usize, Vec<String>, BTreeSet<u32>)> = classes
		.into_iter()
		.filter(|c| !c.is_empty())
		.map(|c| {
			let mut names: Vec<String> = c.iter().map(|&g| order.name(g).into_owned()).collect();
			names.sort();
			(c.len(), names, c)
		})
		.collect();
	keyed.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
	keyed.into_iter().map(|k| k.2).collect()
}

/// `_ClassDef_merge_classify`: with `all_glyphs`, class 0 has the glyphs of those
/// without another class; else it is empty.
fn class_def_merge_classify(
	list: &[&Option<Box<ClassDef>>],
	all_glyphs: Option<&[&[u32]]>,
	order: &GlyphOrder,
) -> (Option<Box<ClassDef>>, Vec<BTreeSet<u32>>) {
	let mut sets = Vec::new();
	for (i, class_def) in list.iter().enumerate() {
		let empty = BTreeMap::new();
		let defs = class_def.as_ref().map_or(&empty, |c| &c.class_defs);
		let max = defs.values().copied().max().unwrap_or(0) as usize;
		let mut inverted: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); max + 1];
		for (&g, &v) in defs {
			inverted[v as usize].insert(g);
		}
		match all_glyphs {
			Some(all) => {
				let mut class0: BTreeSet<u32> = all[i].iter().copied().collect();
				for set in inverted.iter_mut().skip(1) {
					set.retain(|g| class0.contains(g));
					for g in set.iter() {
						class0.remove(g);
					}
				}
				inverted[0] = class0;
				sets.extend(inverted);
			}
			None => sets.extend(inverted.into_iter().skip(1)),
		}
	}
	let mut classes = classify(sets, order);
	if all_glyphs.is_none() {
		classes.insert(0, BTreeSet::new());
	}
	let mut class_defs = BTreeMap::new();
	for (i, class) in classes.iter().enumerate().skip(1) {
		for &g in class {
			class_defs.insert(g, i as u32);
		}
	}
	(Some(Box::new(ClassDef { class_defs })), classes)
}

/// A row of `_PairPosFormat2_align_matrices`; `None` for a transparent record.
type AlignedRow = Vec<Option<ot::Class2Record>>;

/// `_PairPosFormat2_align_matrices`: sets the class definitions and counts of `out`,
/// and returns the matrix of each subtable.
fn align_matrices(
	out: &mut ot::PairPosFormat2,
	list: &[&ot::PairPosFormat2],
	order: &GlyphOrder,
	transparent: bool,
) -> Result<Vec<Vec<AlignedRow>>> {
	let index_error = || Error::ttlib("IndexError: list index out of range");
	// Align the first classes.
	let coverages: Vec<&[u32]> = list
		.iter()
		.map(|l| glyphs_of(&l.coverage).map(Vec::as_slice))
		.collect::<Result<_>>()?;
	let class_defs1: Vec<&Option<Box<ClassDef>>> = list.iter().map(|l| &l.class_def1).collect();
	let (class_def1, classes) = class_def_merge_classify(&class_defs1, Some(&coverages), order);
	out.class_def1 = class_def1;
	out.class1_count = Some(classes.len() as u32);
	let mut matrices: Vec<Vec<AlignedRow>> = Vec::with_capacity(list.len());
	for (l, glyphs) in list.iter().zip(&coverages) {
		let in_coverage: BTreeSet<u32> = glyphs.iter().copied().collect();
		let defs = l
			.class_def1
			.as_ref()
			.ok_or_else(|| no_attribute("classDefs"))?;
		let class2_count = l.class2_count.map_or_else(
			|| l.class1_record.first().map_or(0, |r| r.class2_record.len()),
			|c| c as usize,
		);
		let mut rows = Vec::with_capacity(classes.len());
		for class in &classes {
			let exemplar = class
				.iter()
				.next()
				.ok_or_else(|| Error::ttlib("StopIteration"))?;
			if in_coverage.contains(exemplar) {
				let klass = defs.class_defs.get(exemplar).copied().unwrap_or(0) as usize;
				let record = l.class1_record.get(klass).ok_or_else(index_error)?;
				rows.push(record.class2_record.iter().cloned().map(Some).collect());
			} else {
				let null = (0..class2_count).map(|_| {
					(!transparent).then(|| ot::Class2Record {
						value1: (out.value_format1 != 0)
							.then(|| new_value_record(out.value_format1, None)),
						value2: (out.value_format2 != 0)
							.then(|| new_value_record(out.value_format2, None)),
					})
				});
				rows.push(null.collect());
			}
		}
		matrices.push(rows);
	}
	// Align the second classes.
	let class_defs2: Vec<&Option<Box<ClassDef>>> = list.iter().map(|l| &l.class_def2).collect();
	let (class_def2, classes) = class_def_merge_classify(&class_defs2, None, order);
	out.class_def2 = class_def2;
	out.class2_count = Some(classes.len() as u32);
	let mut new_matrices = Vec::with_capacity(list.len());
	for (l, matrix) in list.iter().zip(matrices) {
		let defs = l
			.class_def2
			.as_ref()
			.ok_or_else(|| no_attribute("classDefs"))?;
		let mut rows = Vec::with_capacity(matrix.len());
		for old in matrix {
			let mut row = Vec::with_capacity(classes.len());
			for class in &classes {
				let klass = match class.iter().next() {
					None => 0,
					Some(exemplar) => defs.class_defs.get(exemplar).copied().unwrap_or(0) as usize,
				};
				row.push(old.get(klass).ok_or_else(index_error)?.clone());
			}
			rows.push(row);
		}
		new_matrices.push(rows);
	}
	Ok(new_matrices)
}

/// `_Lookup_PairPosFormat2_subtables_flatten`.
fn pair_pos_format2_flatten(
	list: &[&ot::PairPosFormat2],
	order: &GlyphOrder,
) -> Result<ot::PairPosFormat2> {
	let mut out = ot::PairPosFormat2::default();
	let mut glyphs = BTreeSet::new();
	for l in list {
		out.value_format1 |= l.value_format1;
		out.value_format2 |= l.value_format2;
		let own = glyphs_of(&l.coverage)?;
		check_order(own)?;
		glyphs.extend(own.iter().copied());
	}
	out.coverage = coverage(glyphs.into_iter().collect());
	let matrices = align_matrices(&mut out, list, order, true)?;
	let num_rows = matrices.first().map_or(0, Vec::len);
	for r in 0..num_rows {
		let num_columns = matrices[0][r].len();
		let mut row = Vec::with_capacity(num_columns);
		for c in 0..num_columns {
			let column = matrices
				.iter()
				.find_map(|m| m[r][c].clone())
				.ok_or_else(|| Error::ttlib("StopIteration"))?;
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
	subtables: Vec<Option<Box<St>>>,
	order: &GlyphOrder,
) -> Result<Vec<Option<Box<St>>>> {
	let is_format = |st: &Option<Box<St>>, f: u16| match st.as_deref() {
		Some(St::PairPos(ot::PairPos::Format1(_))) => f == 1,
		Some(St::PairPos(ot::PairPos::Format2(_))) => f == 2,
		_ => false,
	};
	let first_end = subtables.iter().take_while(|s| is_format(s, 1)).count();
	let last_start = subtables.len()
		- subtables[first_end..]
			.iter()
			.rev()
			.take_while(|s| is_format(s, 2))
			.count();
	let format1: Vec<&ot::PairPosFormat1> = subtables[..first_end]
		.iter()
		.filter_map(|s| match s.as_deref() {
			Some(St::PairPos(ot::PairPos::Format1(t))) => Some(t),
			_ => None,
		})
		.collect();
	let format2: Vec<&ot::PairPosFormat2> = subtables[last_start..]
		.iter()
		.filter_map(|s| match s.as_deref() {
			Some(St::PairPos(ot::PairPos::Format2(t))) => Some(t),
			_ => None,
		})
		.collect();
	let flat1 = pair_pos_format1_flatten(&format1)?;
	let flat2 = pair_pos_format2_flatten(&format2, order)?;
	let mut out = Vec::with_capacity(subtables.len());
	out.push(Some(Box::new(St::PairPos(ot::PairPos::Format1(flat1)))));
	out.extend(subtables[first_end..last_start].iter().cloned());
	out.push(Some(Box::new(St::PairPos(ot::PairPos::Format2(flat2)))));
	Ok(out)
}

/// The merger of `ot.PairPos`.
fn merge_pair_pos(m: &Mutator<'_>, st: &mut ot::PairPos) -> Result<()> {
	let (mut vf1, mut vf2) = (0, 0);
	let mut record_formats = |v1: &Option<ValueRecord>, v2: &Option<ValueRecord>| {
		if let Some(v) = v1 {
			vf1 |= v.format();
		}
		if let Some(v) = v2 {
			vf2 |= v.format();
		}
	};
	match st {
		ot::PairPos::Format1(t) => {
			let glyphs = glyphs_of(&t.coverage)?;
			check_order(glyphs)?;
			for set in &mut t.pair_set {
				let set = set
					.as_deref_mut()
					.ok_or_else(|| no_attribute("PairValueRecord"))?;
				let seconds: Vec<u32> = set
					.pair_value_record
					.iter()
					.map(|r| r.second_glyph)
					.collect();
				check_order(&seconds)?;
				for record in &mut set.pair_value_record {
					record.value1 = (t.value_format1 != 0)
						.then(|| m.merged_value(t.value_format1, record.value1.as_ref()))
						.transpose()?;
					record.value2 = (t.value_format2 != 0)
						.then(|| m.merged_value(t.value_format2, record.value2.as_ref()))
						.transpose()?;
					record_formats(&record.value1, &record.value2);
				}
				set.pair_value_count = Some(set.pair_value_record.len() as u32);
			}
			t.value_format1 = vf1;
			t.value_format2 = vf2;
		}
		ot::PairPos::Format2(t) => {
			let glyphs = glyphs_of(&t.coverage)?;
			check_order(glyphs)?;
			let source = t.clone();
			let matrices = align_matrices(t, &[&source], m.order, false)?;
			let mut rows = Vec::new();
			for row in matrices.into_iter().next().unwrap_or_default() {
				let mut records = Vec::with_capacity(row.len());
				for record in row {
					let mut record = record.ok_or_else(|| no_attribute("Value1"))?;
					if let Some(v) = &mut record.value1 {
						m.value_record(v)?;
					}
					if let Some(v) = &mut record.value2 {
						m.value_record(v)?;
					}
					record_formats(&record.value1, &record.value2);
					records.push(record);
				}
				rows.push(ot::Class1Record {
					class2_record: records,
				});
			}
			t.class1_record = rows;
			t.value_format1 = vf1;
			t.value_format2 = vf2;
		}
		_ => {}
	}
	Ok(())
}

/// `_MarkBasePosFormat1_merge`, without what stays the same: the coverages must be
/// in order, and the records are padded to the class count.
fn merge_mark_base(
	mark_coverage: &Option<Box<Coverage>>,
	base_coverage: &Option<Box<Coverage>>,
	class_count: Option<u32>,
	bases: Vec<&mut Vec<Option<Box<ot::Anchor>>>>,
) -> Result<()> {
	check_order(glyphs_of(mark_coverage)?)?;
	check_order(glyphs_of(base_coverage)?)?;
	let count = class_count.ok_or_else(|| no_attribute("ClassCount"))? as usize;
	for anchors in bases {
		if anchors.len() < count {
			anchors.resize(count, None);
		}
	}
	Ok(())
}

/// The merger of `ot.Lookup`, up to the parts that the walk over the anchors does.
fn merge_lookup(m: &Mutator<'_>, lookup: &mut ot::Lookup) -> Result<()> {
	// Remove the Extension subtables.
	let extension_type = |st: &St| match st {
		St::ExtensionPos(ot::ExtensionPos::Format1(t)) => Some(t.extension_lookup_type),
		St::ExtensionSubst(ot::ExtensionSubst::Format1(t)) => Some(t.extension_lookup_type),
		_ => None,
	};
	if let Some(lookup_type) = lookup
		.sub_table
		.first()
		.and_then(|s| s.as_deref())
		.and_then(extension_type)
	{
		for st in &lookup.sub_table {
			if st.as_deref().and_then(extension_type) != Some(lookup_type) {
				return Err(Error::ttlib("InconsistentExtensions"));
			}
		}
		lookup.lookup_type = lookup_type;
		for st in &mut lookup.sub_table {
			let inner = match st.as_deref_mut() {
				Some(St::ExtensionPos(ot::ExtensionPos::Format1(t))) => t.ext_sub_table.take(),
				Some(St::ExtensionSubst(ot::ExtensionSubst::Format1(t))) => t.ext_sub_table.take(),
				_ => None,
			};
			*st = inner;
		}
	}

	// Beyond-64k: demote extended GPOS subtables to format 1/2 so the mergers can handle
	// them; the merged output is promoted back at the end. Explicit extended formats remain
	// valid below the 64k glyph threshold.
	let extended = m.order.len() > 0x10000 || lookup.sub_table.iter().any(is_extended_gpos);
	if extended {
		for st in &mut lookup.sub_table {
			convert_beyond64k_gpos(st, false)?;
		}
	}

	let is_pair_pos = matches!(
		lookup.sub_table.first().and_then(|s| s.as_deref()),
		Some(St::PairPos(_))
	);
	if is_pair_pos {
		// Compilers sometimes generate multiple subtables. Merge those.
		lookup.sub_table = canonicalize_pair_pos(std::mem::take(&mut lookup.sub_table), m.order)?;
	}

	for index in 0..lookup.sub_table.len() {
		// The first and last subtables may be empty after canonicalizing; they are
		// removed below.
		match lookup.sub_table[index].as_deref_mut() {
			Some(St::SinglePos(_)) => merge_single_pos(m, &mut lookup.sub_table, index)?,
			Some(St::PairPos(st)) => merge_pair_pos(m, st)?,
			Some(St::MarkBasePos(ot::MarkBasePos::Format1(t))) => {
				let array = t
					.base_array
					.as_deref_mut()
					.ok_or_else(|| no_attribute("BaseRecord"))?;
				merge_mark_base(
					&t.mark_coverage,
					&t.base_coverage,
					t.class_count,
					array
						.base_record
						.iter_mut()
						.map(|r| &mut r.base_anchor)
						.collect(),
				)?;
			}
			Some(St::MarkMarkPos(ot::MarkMarkPos::Format1(t))) => {
				let array = t
					.mark2_array
					.as_deref_mut()
					.ok_or_else(|| no_attribute("Mark2Record"))?;
				merge_mark_base(
					&t.mark1_coverage,
					&t.mark2_coverage,
					t.class_count,
					array
						.mark2_record
						.iter_mut()
						.map(|r| &mut r.mark2_anchor)
						.collect(),
				)?;
			}
			Some(St::CursivePos(ot::CursivePos::Format1(t))) => {
				let glyphs = glyphs_of(&t.coverage)?;
				check_order(glyphs)?;
				if t.entry_exit_record.len() > glyphs.len() {
					t.entry_exit_record.truncate(glyphs.len());
				}
				t.entry_exit_count = Some(t.entry_exit_record.len() as u32);
			}
			_ => {}
		}
	}

	if is_pair_pos {
		// Remove the first and last subtables if canonicalizing left them empty.
		let empty = |st: &Option<Box<St>>| match st.as_deref() {
			Some(St::PairPos(ot::PairPos::Format1(t))) => {
				t.coverage.as_ref().is_some_and(|c| c.glyphs.is_empty())
			}
			Some(St::PairPos(ot::PairPos::Format2(t))) => {
				t.coverage.as_ref().is_some_and(|c| c.glyphs.is_empty())
			}
			_ => false,
		};
		if lookup.sub_table.first().is_some_and(empty) {
			lookup.sub_table.remove(0);
		}
		if lookup.sub_table.last().is_some_and(empty) {
			lookup.sub_table.pop();
		}
	}
	lookup.sub_table_count = Some(lookup.sub_table.len() as u32);
	if extended {
		for st in &mut lookup.sub_table {
			convert_beyond64k_gpos(st, true)?;
		}
	}
	Ok(())
}

/// `merger.mergeTables` for a table.
fn merge_table<T: OtTable>(m: &Mutator<'_>, table: &mut T) -> Result<()> {
	table.visit_mut(&mut |any| {
		if let Some(lookup) = any.downcast_mut::<ot::Lookup>() {
			merge_lookup(m, lookup)?;
		} else if let Some(anchor) = any.downcast_mut::<ot::Anchor>() {
			m.anchor(anchor)?;
		} else if let Some(value) = any.downcast_mut::<ot::MathValueRecord>() {
			m.math_value(value)?;
		} else if let Some(caret) = any.downcast_mut::<ot::CaretValue>() {
			if let ot::CaretValue::Format3(c) = caret
				&& let Some(coordinate) = m.coordinate(&mut c.coordinate, &mut c.device_table)?
			{
				*caret = ot::CaretValue::Format1(ot::CaretValueFormat1 { coordinate });
			}
		} else if let Some(coord) = any.downcast_mut::<ot::BaseCoord>()
			&& let ot::BaseCoord::Format3(c) = coord
			&& let Some(coordinate) = m.coordinate(&mut c.coordinate, &mut c.device_table)?
		{
			*coord = ot::BaseCoord::Format1(ot::BaseCoordFormat1 { coordinate });
		}
		Ok(())
	})
}

/// `remap_device_varidxes`, for the device tables only.
fn remap_devices<T: OtTable>(table: &mut T, map: &HashMap<u64, u64>) -> Result<()> {
	table.visit_mut(&mut |any| {
		if let Some(device) = any.downcast_mut::<ot::Device>()
			&& device.delta_format == VARIATION_INDEX_DEVICE
		{
			let old = (u64::from(device.start_size) << 16) + u64::from(device.end_size);
			let new = *map
				.get(&old)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {old}")))?;
			device.start_size = (new >> 16) as u16;
			device.end_size = (new & 0xFFFF) as u16;
		}
		Ok(())
	})
}

fn has_regions(store: &ot::VarStore) -> bool {
	store
		.var_region_list
		.as_ref()
		.is_some_and(|l| !l.region.is_empty())
}

/// `_instantiateBASE`.
fn instantiate_base(font: &mut Font, axes: &[String], limits: &NormalizedLimits) -> Result<()> {
	let order = font.glyph_order()?;
	let Some(base) = font.get::<Base>()? else {
		return Ok(());
	};
	let table = &mut base.table;
	let Some(store) = &mut table.var_store else {
		return Ok(());
	};
	log::info!("Instantiating BASE table");
	let deltas = instantiate_item_variation_store(store, axes, limits)?;
	let keep = has_regions(store);
	let mutator = Mutator {
		deltas: &deltas,
		delete_variations: !keep,
		order: &order,
	};
	merge_table(&mutator, table)?;
	if keep {
		if let Some(store) = &mut table.var_store {
			let map = var_store::optimize(store, true)?;
			remap_devices(table, &map)?;
		}
	} else {
		// Downgrade BASE: it no longer references an ItemVariationStore.
		table.var_store = None;
		table.version = 0x0001_0000;
	}
	Ok(())
}

/// `_instantiateConditionValueRows`: rows of their own for the conditions whose
/// values don't fit, or whose rows would be rounded.
fn instantiate_condition_value_rows(
	store: &mut TupleVarStore,
	defaults: &mut Vec<Vec<f64>>,
	condition: &mut ot::ConditionTableFormat2,
) -> Result<()> {
	let var_idx = u64::from(condition.var_idx);
	if var_idx == NO_VARIATION_INDEX {
		return Ok(());
	}
	let (major, minor) = ((var_idx >> 16) as usize, (var_idx & 0xFFFF) as usize);
	if major >= store.tuple_var_data.len() || minor >= store.item_counts[major] {
		return Ok(());
	}
	use fontoxide_ttlib::tables::glyf::Number;
	use fontoxide_ttlib::tables::tuple_variation::{Delta, TupleVariation};
	let value_at = |v: &TupleVariation| match v.coordinates.get(minor) {
		Some(Some(Delta::Value(n))) => n.value(),
		_ => 0.0,
	};
	let variations = &store.tuple_var_data[major];
	let deltas: Vec<f64> = variations.iter().map(value_at).collect();
	if deltas.iter().all(|d| *d == 0.0) {
		return Ok(());
	}
	let value = f64::from(condition.default_value) + defaults[major][minor];
	let maximum = deltas
		.iter()
		.chain([&value])
		.fold(0.0f64, |m, v| m.max(v.abs()));
	let ldexp = |v: f64, shift: i32| v * 2f64.powi(shift);
	let mut shift = 0;
	while ldexp(maximum, shift) > 2147483647.0 {
		shift -= 1;
	}
	// A positive rescaling preserves the Boolean boundary. Prefer an exact integer
	// encoding; otherwise use the available 32-bit precision.
	while deltas.iter().chain([&value]).any(|&v| {
		let scaled = ldexp(v, shift);
		scaled != ot_round(scaled) as f64
	}) && ldexp(maximum, shift) * 2.0 <= 2147483647.0
	{
		shift += 1;
	}
	let mut default = ot_round(ldexp(value, shift));
	if shift == 0 && (-0x8000..=0x7FFF).contains(&default) {
		return Ok(());
	}
	if store.tuple_var_data.len() >= 0xFFFF {
		return Err(Error::value(
			"Too many variation subtables for a private condition row",
		));
	}
	// Conditions can share their source row with positioning values, which must
	// not be rescaled. Append a private copy before rounding those rows.
	let mut private: Vec<TupleVariation> = variations
		.iter()
		.zip(&deltas)
		.filter(|(_, d)| **d != 0.0)
		.map(|(var, d)| TupleVariation {
			axes: var.axes.clone(),
			coordinates: vec![Some(Delta::Value(Number::Int(ot_round(ldexp(*d, shift)))))],
		})
		.collect();
	if !(-0x8000..=0x7FFF).contains(&default) {
		private.push(TupleVariation {
			axes: Default::default(),
			coordinates: vec![Some(Delta::Value(Number::Int(default)))],
		});
		default = 0;
	}
	condition.default_value = default as i16;
	condition.var_idx = fit((store.tuple_var_data.len() as u64) << 16, "VarIdx")?;
	store.tuple_var_data.push(private);
	store.item_counts.push(1);
	defaults.push(vec![0.0]);
	Ok(())
}

/// Whether a `GDEF` has any of the fields of version 1.4.
fn has_extended_gdef_fields(gdef: &Gdef) -> bool {
	gdef.glyph_class_def2.is_some()
		|| gdef.attach_list2.is_some()
		|| gdef.lig_caret_list2.is_some()
		|| gdef.mark_attach_class_def2.is_some()
		|| gdef.mark_glyph_sets_def2.is_some()
}

/// `instantiateOTL`.
pub fn instantiate_otl(font: &mut Font, limits: &NormalizedLimits) -> Result<()> {
	let axes: Vec<String> = fvar_axes(font)?.into_iter().map(|a| a.0).collect();
	instantiate_base(font, &axes, limits)?;

	let usable = match font.get::<Gdef>()? {
		Some(gdef) => {
			gdef.version >= 0x0001_0003
				&& gdef
					.var_store
					.as_ref()
					.is_some_and(|s| !s.var_data.is_empty())
		}
		None => false,
	};
	if !usable {
		return instantiate_condition_values(font, None, Some(&HashMap::new()));
	}
	log::info!("Instantiating GDEF, GPOS, MATH and JSTF tables");
	let order = font.glyph_order()?;

	let gdef = font.require::<Gdef>()?;
	let store = gdef
		.var_store
		.as_ref()
		.ok_or_else(|| no_attribute("VarData"))?;
	let mut tuple_store = TupleVarStore::from_item_var_store(store, &axes)?;
	// `instantiateItemVariationStore` with the condition values.
	let mut has_conditions = false;
	for_each_condition(font, |c| {
		has_conditions |= matches!(c, ot::ConditionTable::Format2(_));
		Ok(())
	})?;
	let mut defaults = tuple_store.instantiate(limits, !has_conditions)?;
	if has_conditions {
		for_each_condition(font, |c| match c {
			ot::ConditionTable::Format2(c) => {
				instantiate_condition_value_rows(&mut tuple_store, &mut defaults, c)
			}
			_ => Ok(()),
		})?;
		super::item_store::round_all(&mut tuple_store);
		tuple_store.rebuild_regions();
	}
	let new_store = tuple_store.as_item_var_store(true)?;
	let gdef = font.require::<Gdef>()?;
	let store = gdef
		.var_store
		.as_mut()
		.ok_or_else(|| no_attribute("VarData"))?;
	store.var_region_list = new_store.var_region_list;
	if !has_conditions
		&& store
			.var_data_count
			.is_some_and(|c| Some(c) != new_store.var_data_count)
	{
		return Err(Error::Assertion(
			"itemVarStore.VarDataCount == newItemVarStore.VarDataCount".into(),
		));
	}
	store.var_data_count = new_store.var_data_count;
	store.var_data = new_store.var_data;
	let keep = has_regions(store);
	let deltas = DefaultDeltas::from_array(&defaults);

	// The merger adds the default deltas to the anchors, value records, ligature
	// carets and MATH values, and deletes the device tables if the store is fully
	// instanced.
	let mutator = Mutator {
		deltas: &deltas,
		delete_variations: !keep,
		order: &order,
	};
	merge_table(&mutator, &mut font.require::<Gdef>()?.table)?;
	if let Some(t) = font.get::<Gpos>()? {
		merge_table(&mutator, &mut t.table)?;
	}
	if let Some(t) = font.get::<Math>()? {
		merge_table(&mutator, &mut t.table)?;
	}
	if let Some(t) = font.get::<Jstf>()? {
		merge_table(&mutator, &mut t.table)?;
	}

	let gdef = font.require::<Gdef>()?;
	let store = gdef
		.var_store
		.as_mut()
		.ok_or_else(|| no_attribute("VarData"))?;
	let map = if keep {
		var_store::optimize(store, true)?
	} else {
		HashMap::new()
	};
	instantiate_condition_values(font, Some(&deltas), Some(&map))?;

	if keep {
		remap_devices(&mut font.require::<Gdef>()?.table, &map)?;
		if let Some(t) = font.get::<Gsub>()? {
			remap_devices(&mut t.table, &map)?;
		}
		if let Some(t) = font.get::<Gpos>()? {
			remap_devices(&mut t.table, &map)?;
		}
		if let Some(t) = font.get::<Math>()? {
			remap_devices(&mut t.table, &map)?;
		}
		if let Some(t) = font.get::<Jstf>()? {
			remap_devices(&mut t.table, &map)?;
		}
	} else if has_extended_gdef_fields(font.require::<Gdef>()?) {
		// A beyond-64k GDEF: keep version 1.4 and NULL the store; don't drop the *2 fields.
		let gdef = font.require::<Gdef>()?;
		gdef.var_store = None;
		gdef.version = 0x0001_0004;
	} else {
		// Downgrade GDEF.
		let gdef = font.require::<Gdef>()?;
		gdef.var_store = None;
		gdef.version = 0x0001_0002;
		if gdef.mark_glyph_sets_def.is_none() {
			gdef.version = 0x0001_0000;
		}
		if gdef.lig_caret_list.is_none()
			&& gdef.mark_attach_class_def.is_none()
			&& gdef.glyph_class_def.is_none()
			&& gdef.attach_list.is_none()
			&& !(gdef.version >= 0x0001_0002 && gdef.mark_glyph_sets_def.is_some())
		{
			font.remove(Tag::new(b"GDEF"));
		}
	}
	Ok(())
}
