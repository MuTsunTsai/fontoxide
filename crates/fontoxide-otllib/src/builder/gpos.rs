//! GPOS subtables and their parts.

use std::collections::{BTreeMap, HashMap};

use fontoxide_ttlib::ot::conv::ValueRecord;
use fontoxide_ttlib::ot::types::{self as ot, Anchor, Coverage};
use indexmap::IndexMap;

use super::class_def::ClassDefBuilder;
use super::{MaybeExtended, build_coverage, sorted_glyphs};
use crate::{Error, GlyphMap, Result};

/// The value records for the first and the second glyph of a pair, either of
/// which may be missing.
pub type ValuePair = (Option<ValueRecord>, Option<ValueRecord>);

/// Builds an anchor table (`buildAnchor`): format 1 with just coordinates,
/// format 2 with a contour point, format 3 with device tables.
pub fn build_anchor(
	x: i16,
	y: i16,
	point: Option<u16>,
	device_x: Option<ot::Device>,
	device_y: Option<ot::Device>,
) -> Result<Anchor> {
	if device_x.is_some() || device_y.is_some() {
		if point.is_some() {
			return Err(Error::Assertion(
				"Either point, or both of deviceX/deviceY, must be None.".into(),
			));
		}
		return Ok(Anchor::Format3(ot::AnchorFormat3 {
			x_coordinate: x,
			y_coordinate: y,
			x_device_table: device_x.map(Box::new),
			y_device_table: device_y.map(Box::new),
		}));
	}
	Ok(match point {
		Some(p) => Anchor::Format2(ot::AnchorFormat2 {
			x_coordinate: x,
			y_coordinate: y,
			anchor_point: p,
		}),
		None => Anchor::Format1(ot::AnchorFormat1 {
			x_coordinate: x,
			y_coordinate: y,
		}),
	})
}

fn anchors(list: Vec<Option<Anchor>>) -> Vec<Option<Box<Anchor>>> {
	list.into_iter().map(|a| a.map(Box::new)).collect()
}

/// Builds a base record from the anchors of each mark class (`buildBaseRecord`).
pub fn build_base_record(
	anchor_list: Vec<Option<Anchor>>,
	extended: bool,
) -> MaybeExtended<ot::BaseRecord, ot::BaseRecord2> {
	let base_anchor = anchors(anchor_list);
	if extended {
		MaybeExtended::Extended(ot::BaseRecord2 { base_anchor })
	} else {
		MaybeExtended::Normal(ot::BaseRecord { base_anchor })
	}
}

/// Builds a base array (`buildBaseArray`): for each base glyph, sorted by glyph
/// ID, its anchors for each of the `num_mark_classes` mark classes.
pub fn build_base_array(
	bases: &IndexMap<String, BTreeMap<u16, Anchor>>,
	num_mark_classes: u16,
	glyph_map: &GlyphMap,
) -> Result<MaybeExtended<ot::BaseArray, ot::BaseArray2>> {
	let extended = glyph_map.has_extended_glyph_ids();
	let mut records = Vec::new();
	for (_, base) in sorted_glyphs(bases.keys().map(String::as_str), glyph_map)? {
		let b = &bases[base];
		let list = (0..num_mark_classes)
			.map(|mc| b.get(&mc).cloned())
			.collect();
		records.push(build_base_record(list, extended));
	}
	let count = Some(records.len() as u32);
	Ok(if extended {
		MaybeExtended::Extended(ot::BaseArray2 {
			base_count: count,
			base_record: records
				.into_iter()
				.filter_map(MaybeExtended::extended)
				.collect(),
		})
	} else {
		MaybeExtended::Normal(ot::BaseArray {
			base_count: count,
			base_record: records
				.into_iter()
				.filter_map(MaybeExtended::normal)
				.collect(),
		})
	})
}

/// Builds a component record of a ligature from the anchors of each mark class
/// (`buildComponentRecord`); `None` without anchors.
pub fn build_component_record(
	anchor_list: Option<Vec<Option<Anchor>>>,
	extended: bool,
) -> Option<MaybeExtended<ot::ComponentRecord, ot::ComponentRecord2>> {
	let list = anchor_list.filter(|l| !l.is_empty())?;
	let ligature_anchor = anchors(list);
	Some(if extended {
		MaybeExtended::Extended(ot::ComponentRecord2 { ligature_anchor })
	} else {
		MaybeExtended::Normal(ot::ComponentRecord { ligature_anchor })
	})
}

/// Builds a cursive positioning (GPOS3) subtable from glyph names to entry and
/// exit anchors (`buildCursivePosSubtable`); `None` for no glyphs.
pub fn build_cursive_pos_subtable(
	attach: &IndexMap<String, (Option<Anchor>, Option<Anchor>)>,
	glyph_map: &GlyphMap,
) -> Result<Option<ot::CursivePos>> {
	if attach.is_empty() {
		return Ok(None);
	}
	let extended = glyph_map.has_extended_glyph_ids();
	let glyphs = sorted_glyphs(attach.keys().map(String::as_str), glyph_map)?;
	let coverage = Some(Box::new(Coverage {
		glyphs: glyphs.iter().map(|&(id, _)| id).collect(),
	}));
	let count = Some(glyphs.len() as u32);
	let entry_exit = glyphs.iter().map(|&(_, g)| {
		let (entry, exit) = &attach[g];
		(entry.clone().map(Box::new), exit.clone().map(Box::new))
	});
	Ok(Some(if extended {
		ot::CursivePos::Format2(ot::CursivePosFormat2 {
			coverage,
			entry_exit_count: count,
			entry_exit_record: entry_exit
				.map(|(entry_anchor, exit_anchor)| ot::EntryExit2 {
					entry_anchor,
					exit_anchor,
				})
				.collect(),
			dont_share: false,
		})
	} else {
		ot::CursivePos::Format1(ot::CursivePosFormat1 {
			coverage,
			entry_exit_count: count,
			entry_exit_record: entry_exit
				.map(|(entry_anchor, exit_anchor)| ot::EntryExitRecord {
					entry_anchor,
					exit_anchor,
				})
				.collect(),
			dont_share: false,
		})
	}))
}

/// Builds a device table from pixel sizes to adjustments in pixels
/// (`buildDevice`); `None` for no sizes.
pub fn build_device(deltas: &BTreeMap<i64, i64>) -> Result<Option<ot::Device>> {
	let (Some((&start, _)), Some((&end, _))) = (deltas.first_key_value(), deltas.last_key_value())
	else {
		return Ok(None);
	};
	if !(0 <= start && start <= end) {
		return Err(Error::Assertion("0 <= startSize <= endSize".into()));
	}
	let values: Vec<i64> = (start..=end)
		.map(|size| deltas.get(&size).copied().unwrap_or(0))
		.collect();
	let max = values.iter().copied().max().unwrap_or(0);
	let min = values.iter().copied().min().unwrap_or(0);
	if !(min > -129 && max < 128) {
		return Err(Error::Assertion(
			"minDelta > -129 and maxDelta < 128".into(),
		));
	}
	let delta_format = if min > -3 && max < 2 {
		1
	} else if min > -9 && max < 8 {
		2
	} else {
		3
	};
	let size = |v: i64| {
		u16::try_from(v).map_err(|_| Error::Value(format!("device size out of range: {v}")))
	};
	Ok(Some(ot::Device {
		start_size: size(start)?,
		end_size: size(end)?,
		delta_format,
		delta_value: values,
	}))
}

/// Builds a ligature array (`buildLigatureArray`): for each ligature glyph,
/// sorted by glyph ID, the anchors of each of its components for each of the
/// `num_mark_classes` mark classes.
pub fn build_ligature_array(
	ligs: &IndexMap<String, Vec<BTreeMap<u16, Anchor>>>,
	num_mark_classes: u16,
	glyph_map: &GlyphMap,
) -> Result<MaybeExtended<ot::LigatureArray, ot::LigatureArray2>> {
	let extended = glyph_map.has_extended_glyph_ids();
	let mut attaches = Vec::new();
	for (_, lig) in sorted_glyphs(ligs.keys().map(String::as_str), glyph_map)? {
		let components = ligs[lig]
			.iter()
			.map(|c| {
				Some(
					(0..num_mark_classes)
						.map(|mc| c.get(&mc).cloned())
						.collect(),
				)
			})
			.collect();
		attaches.push(build_ligature_attach(components, extended));
	}
	let count = Some(attaches.len() as u32);
	Ok(if extended {
		MaybeExtended::Extended(ot::LigatureArray2 {
			ligature_count: count,
			ligature_attach: attaches
				.into_iter()
				.filter_map(MaybeExtended::extended)
				.map(|a| Some(Box::new(a)))
				.collect(),
		})
	} else {
		MaybeExtended::Normal(ot::LigatureArray {
			ligature_count: count,
			ligature_attach: attaches
				.into_iter()
				.filter_map(MaybeExtended::normal)
				.map(|a| Some(Box::new(a)))
				.collect(),
		})
	})
}

/// Builds a ligature attach table from the anchors of each component
/// (`buildLigatureAttach`).
///
/// A component without anchors has no record in fontTools (which cannot then
/// compile the table); here it gets a record without anchors.
pub fn build_ligature_attach(
	components: Vec<Option<Vec<Option<Anchor>>>>,
	extended: bool,
) -> MaybeExtended<ot::LigatureAttach, ot::LigatureAttach2> {
	let count = Some(components.len() as u32);
	let records = components
		.into_iter()
		.map(|c| build_component_record(c, extended));
	if extended {
		MaybeExtended::Extended(ot::LigatureAttach2 {
			component_count: count,
			component_record: records
				.map(|r| r.and_then(MaybeExtended::extended).unwrap_or_default())
				.collect(),
		})
	} else {
		MaybeExtended::Normal(ot::LigatureAttach {
			component_count: count,
			component_record: records
				.map(|r| r.and_then(MaybeExtended::normal).unwrap_or_default())
				.collect(),
		})
	}
}

/// Builds a mark record (`buildMarkRecord`).
pub fn build_mark_record(
	class: u16,
	anchor: Anchor,
	extended: bool,
) -> MaybeExtended<ot::MarkRecord, ot::MarkRecord2> {
	let mark_anchor = Some(Box::new(anchor));
	if extended {
		MaybeExtended::Extended(ot::MarkRecord2 { class, mark_anchor })
	} else {
		MaybeExtended::Normal(ot::MarkRecord { class, mark_anchor })
	}
}

/// Builds a mark array (`buildMarkArray`): for each mark glyph, sorted by glyph
/// ID, its mark class and anchor.
pub fn build_mark_array(
	marks: &IndexMap<String, (u16, Anchor)>,
	glyph_map: &GlyphMap,
) -> Result<MaybeExtended<ot::MarkArray, ot::MarkArray2>> {
	let extended = glyph_map.has_extended_glyph_ids();
	let mut records = Vec::new();
	for (_, mark) in sorted_glyphs(marks.keys().map(String::as_str), glyph_map)? {
		let (class, anchor) = &marks[mark];
		records.push(build_mark_record(*class, anchor.clone(), extended));
	}
	let count = Some(records.len() as u32);
	Ok(if extended {
		MaybeExtended::Extended(ot::MarkArray2 {
			mark_count: count,
			mark_record: records
				.into_iter()
				.filter_map(MaybeExtended::extended)
				.collect(),
		})
	} else {
		MaybeExtended::Normal(ot::MarkArray {
			mark_count: count,
			mark_record: records
				.into_iter()
				.filter_map(MaybeExtended::normal)
				.collect(),
		})
	})
}

fn class_count(marks: &IndexMap<String, (u16, Anchor)>) -> Result<u16> {
	marks
		.values()
		.map(|(mc, _)| *mc)
		.max()
		.map(|m| m + 1)
		.ok_or_else(|| Error::Value("max() iterable argument is empty".into()))
}

/// Builds a mark-to-base positioning (GPOS4) subtable (`buildMarkBasePosSubtable`)
/// from mark glyphs to their class and anchor, and base glyphs to the anchors
/// of each class.
pub fn build_mark_base_pos_subtable(
	marks: &IndexMap<String, (u16, Anchor)>,
	bases: &IndexMap<String, BTreeMap<u16, Anchor>>,
	glyph_map: &GlyphMap,
) -> Result<ot::MarkBasePos> {
	let mark_coverage = Some(Box::new(build_coverage(
		marks.keys().map(String::as_str),
		glyph_map,
	)?));
	let mark_array = build_mark_array(marks, glyph_map)?;
	let count = class_count(marks)?;
	let base_coverage = Some(Box::new(build_coverage(
		bases.keys().map(String::as_str),
		glyph_map,
	)?));
	let base_array = build_base_array(bases, count, glyph_map)?;
	Ok(match (mark_array, base_array) {
		(MaybeExtended::Extended(mark_array), MaybeExtended::Extended(base_array)) => {
			ot::MarkBasePos::Format2(ot::MarkBasePosFormat2 {
				mark_coverage,
				base_coverage,
				class_count: Some(count as u32),
				mark_array: Some(Box::new(mark_array)),
				base_array: Some(Box::new(base_array)),
				dont_share: false,
			})
		}
		(mark_array, base_array) => ot::MarkBasePos::Format1(ot::MarkBasePosFormat1 {
			mark_coverage,
			base_coverage,
			class_count: Some(count as u32),
			mark_array: mark_array.normal().map(Box::new),
			base_array: base_array.normal().map(Box::new),
			dont_share: false,
		}),
	})
}

/// Builds a mark-to-ligature positioning (GPOS5) subtable
/// (`buildMarkLigPosSubtable`) from mark glyphs to their class and anchor, and
/// ligature glyphs to the anchors of each class on each component.
pub fn build_mark_lig_pos_subtable(
	marks: &IndexMap<String, (u16, Anchor)>,
	ligs: &IndexMap<String, Vec<BTreeMap<u16, Anchor>>>,
	glyph_map: &GlyphMap,
) -> Result<ot::MarkLigPos> {
	let mark_coverage = Some(Box::new(build_coverage(
		marks.keys().map(String::as_str),
		glyph_map,
	)?));
	let mark_array = build_mark_array(marks, glyph_map)?;
	let count = class_count(marks)?;
	let ligature_coverage = Some(Box::new(build_coverage(
		ligs.keys().map(String::as_str),
		glyph_map,
	)?));
	let ligature_array = build_ligature_array(ligs, count, glyph_map)?;
	Ok(match (mark_array, ligature_array) {
		(MaybeExtended::Extended(mark_array), MaybeExtended::Extended(ligature_array)) => {
			ot::MarkLigPos::Format2(ot::MarkLigPosFormat2 {
				mark_coverage,
				ligature_coverage,
				class_count: Some(count as u32),
				mark_array: Some(Box::new(mark_array)),
				ligature_array: Some(Box::new(ligature_array)),
				dont_share: false,
			})
		}
		(mark_array, ligature_array) => ot::MarkLigPos::Format1(ot::MarkLigPosFormat1 {
			mark_coverage,
			ligature_coverage,
			class_count: Some(count as u32),
			mark_array: mark_array.normal().map(Box::new),
			ligature_array: ligature_array.normal().map(Box::new),
			dont_share: false,
		}),
	})
}

/// Builds a mark-to-mark base record from the anchors of each mark class
/// (`buildMark2Record`).
pub fn build_mark2_record(
	anchor_list: Vec<Option<Anchor>>,
	extended: bool,
) -> MaybeExtended<ot::Mark2Record, ot::Mark2Record2> {
	let mark2_anchor = anchors(anchor_list);
	if extended {
		MaybeExtended::Extended(ot::Mark2Record2 { mark2_anchor })
	} else {
		MaybeExtended::Normal(ot::Mark2Record { mark2_anchor })
	}
}

/// `ValueRecord(src=src, valueFormat=value_format)`: a value record with the
/// fields of `value_format`, taken from `src` where it has them and zero (or a
/// null device) otherwise.
pub fn value_record_with_format(src: Option<&ValueRecord>, value_format: u16) -> ValueRecord {
	let empty = ValueRecord::default();
	let src = src.unwrap_or(&empty);
	let int = |bit: u16, v: Option<i16>| (value_format & bit != 0).then(|| v.unwrap_or(0));
	let device = |bit: u16, v: &Option<Option<Box<ot::Device>>>| {
		(value_format & bit != 0).then(|| v.clone().unwrap_or(None))
	};
	let mut reserved = [None; 8];
	for (i, r) in reserved.iter_mut().enumerate() {
		if value_format & (0x100 << i) != 0 {
			*r = Some(src.reserved[i].unwrap_or(0));
		}
	}
	ValueRecord {
		x_placement: int(0x0001, src.x_placement),
		y_placement: int(0x0002, src.y_placement),
		x_advance: int(0x0004, src.x_advance),
		y_advance: int(0x0008, src.y_advance),
		x_pla_device: device(0x0010, &src.x_pla_device),
		y_pla_device: device(0x0020, &src.y_pla_device),
		x_adv_device: device(0x0040, &src.x_adv_device),
		y_adv_device: device(0x0080, &src.y_adv_device),
		reserved,
	}
}

/// Builds a value record from field names and values (`buildValue`), such as
/// `[("XAdvance", 50)]`. Device tables can be set on the result.
pub fn build_value(fields: &[(&str, i16)]) -> Result<ValueRecord> {
	let mut v = ValueRecord::default();
	for &(name, value) in fields {
		match name {
			"XPlacement" => v.x_placement = Some(value),
			"YPlacement" => v.y_placement = Some(value),
			"XAdvance" => v.x_advance = Some(value),
			"YAdvance" => v.y_advance = Some(value),
			_ => match name
				.strip_prefix("Reserved")
				.and_then(|n| n.parse::<usize>().ok())
			{
				Some(n @ 1..=8) => v.reserved[n - 1] = Some(value as u16),
				_ => return Err(Error::Value(format!("unknown value record field: {name}"))),
			},
		}
	}
	Ok(v)
}

/// `_getValueFormat`: the given format, or the union of the formats of the
/// first (`i == 0`) or second value records.
fn value_format<'a>(f: Option<u16>, values: impl Iterator<Item = &'a ValuePair>, i: usize) -> u16 {
	if let Some(f) = f {
		return f;
	}
	let mut mask = 0;
	for value in values {
		let v = if i == 0 { &value.0 } else { &value.1 };
		if let Some(v) = v {
			mask |= v.format();
		}
	}
	mask
}

fn value_or_none(src: Option<&ValueRecord>, value_format: u16) -> Option<ValueRecord> {
	(value_format != 0).then(|| value_record_with_format(src, value_format))
}

/// Builds a class-based pair adjustment (GPOS2 format 2) subtable
/// (`buildPairPosClassesSubtable`) from pairs of glyph classes to value
/// records. Each class is a sorted list of glyph names; a glyph may be in only
/// one first and one second class.
pub fn build_pair_pos_classes_subtable(
	pairs: &IndexMap<(Vec<String>, Vec<String>), ValuePair>,
	glyph_map: &GlyphMap,
	value_format1: Option<u16>,
	value_format2: Option<u16>,
) -> Result<ot::PairPos> {
	let mut coverage: Vec<&str> = Vec::new();
	let mut class_def1 = ClassDefBuilder::new(true);
	let mut class_def2 = ClassDefBuilder::new(false);
	let mut keys: Vec<&(Vec<String>, Vec<String>)> = pairs.keys().collect();
	keys.sort();
	for (gc1, gc2) in keys {
		coverage.extend(gc1.iter().map(String::as_str));
		class_def1.add(gc1)?;
		class_def2.add(gc2)?;
	}
	let vf1 = value_format(value_format1, pairs.values(), 0);
	let vf2 = value_format(value_format2, pairs.values(), 1);
	let coverage = Some(Box::new(build_coverage(coverage, glyph_map)?));
	let classes1 = class_def1.classes();
	let classes2 = class_def2.classes();
	let none: ValuePair = (None, None);
	let mut class1_record = Vec::with_capacity(classes1.len());
	for c1 in &classes1 {
		let mut class2_record = Vec::with_capacity(classes2.len());
		for c2 in &classes2 {
			let (val1, val2) = pairs.get(&(c1.clone(), c2.clone())).unwrap_or(&none);
			class2_record.push(ot::Class2Record {
				value1: value_or_none(val1.as_ref(), vf1),
				value2: value_or_none(val2.as_ref(), vf2),
			});
		}
		class1_record.push(ot::Class1Record { class2_record });
	}
	let class_def1 = Some(Box::new(class_def1.build(glyph_map)?));
	let class_def2 = Some(Box::new(class_def2.build(glyph_map)?));
	let class1_count = Some(class1_record.len() as u32);
	let class2_count = Some(classes2.len() as u32);
	Ok(if glyph_map.has_extended_glyph_ids() {
		ot::PairPos::Format4(ot::PairPosFormat4 {
			coverage,
			value_format1: vf1,
			value_format2: vf2,
			class_def1,
			class_def2,
			class1_count,
			class2_count,
			class1_record,
			dont_share: false,
		})
	} else {
		ot::PairPos::Format2(ot::PairPosFormat2 {
			coverage,
			value_format1: vf1,
			value_format2: vf2,
			class_def1,
			class_def2,
			class1_count,
			class2_count,
			class1_record,
			dont_share: false,
		})
	})
}

/// Builds glyph-based pair adjustment (GPOS2 format 1) subtables
/// (`buildPairPosGlyphs`): one for each combination of value formats, in order
/// of the formats.
pub fn build_pair_pos_glyphs(
	pairs: &IndexMap<(String, String), ValuePair>,
	glyph_map: &GlyphMap,
) -> Result<Vec<ot::PairPos>> {
	let mut p: IndexMap<(u16, u16), IndexMap<(String, String), ValuePair>> = IndexMap::new();
	for (key, (a, b)) in pairs {
		let fa = a.as_ref().map_or(0, ValueRecord::format);
		let fb = b.as_ref().map_or(0, ValueRecord::format);
		p.entry((fa, fb))
			.or_default()
			.insert(key.clone(), (a.clone(), b.clone()));
	}
	let mut groups: Vec<_> = p.into_iter().collect();
	groups.sort_by_key(|(k, _)| *k);
	groups
		.into_iter()
		.map(|((fa, fb), pos)| build_pair_pos_glyphs_subtable(&pos, glyph_map, Some(fa), Some(fb)))
		.collect()
}

/// Builds a glyph-based pair adjustment (GPOS2 format 1) subtable
/// (`buildPairPosGlyphsSubtable`) from pairs of glyph names to value records.
pub fn build_pair_pos_glyphs_subtable(
	pairs: &IndexMap<(String, String), ValuePair>,
	glyph_map: &GlyphMap,
	value_format1: Option<u16>,
	value_format2: Option<u16>,
) -> Result<ot::PairPos> {
	let extended = glyph_map.has_extended_glyph_ids();
	let vf1 = value_format(value_format1, pairs.values(), 0);
	let vf2 = value_format(value_format2, pairs.values(), 1);
	let mut p: HashMap<&str, Vec<(&str, &ValuePair)>> = HashMap::new();
	for ((a, b), v) in pairs {
		p.entry(a.as_str()).or_default().push((b.as_str(), v));
	}
	let glyphs = sorted_glyphs(pairs.keys().map(|(a, _)| a.as_str()), glyph_map)?;
	let mut sets = Vec::with_capacity(glyphs.len());
	for &(_, glyph) in &glyphs {
		let mut seconds = Vec::new();
		for &(g2, v) in &p[glyph] {
			seconds.push((glyph_map.id(g2)?, v));
		}
		seconds.sort_by_key(|&(id, _)| id);
		let records: Vec<(u32, Option<ValueRecord>, Option<ValueRecord>)> = seconds
			.into_iter()
			.map(|(id, (v1, v2))| {
				(
					id,
					value_or_none(v1.as_ref(), vf1),
					value_or_none(v2.as_ref(), vf2),
				)
			})
			.collect();
		sets.push(records);
	}
	let coverage = Some(Box::new(Coverage {
		glyphs: glyphs.iter().map(|&(id, _)| id).collect(),
	}));
	let pair_set_count = Some(sets.len() as u32);
	Ok(if extended {
		ot::PairPos::Format3(ot::PairPosFormat3 {
			coverage,
			value_format1: vf1,
			value_format2: vf2,
			pair_set_count,
			pair_set: sets
				.into_iter()
				.map(|records| {
					Some(Box::new(ot::PairSet2 {
						pair_value_count: Some(records.len() as u32),
						pair_value_record: records
							.into_iter()
							.map(|(second_glyph, value1, value2)| ot::PairValue2 {
								second_glyph,
								value1,
								value2,
							})
							.collect(),
					}))
				})
				.collect(),
			dont_share: false,
		})
	} else {
		ot::PairPos::Format1(ot::PairPosFormat1 {
			coverage,
			value_format1: vf1,
			value_format2: vf2,
			pair_set_count,
			pair_set: sets
				.into_iter()
				.map(|records| {
					Some(Box::new(ot::PairSet {
						pair_value_count: Some(records.len() as u32),
						pair_value_record: records
							.into_iter()
							.map(|(second_glyph, value1, value2)| ot::PairValueRecord {
								second_glyph,
								value1,
								value2,
							})
							.collect(),
					}))
				})
				.collect(),
			dont_share: false,
		})
	})
}

/// A field value in a [`SinglePosValueKey`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ValueKeyItem {
	/// A number.
	Int(i64),
	/// A null device table.
	None,
	/// A device table: `(DeltaFormat, StartSize, EndSize, DeltaValue)`, without
	/// the deltas of a variation index (`_DeviceTuple`).
	Device(u16, u16, u16, Vec<i64>),
}

/// The fields of a value record, which makes equal records equal
/// (`_getSinglePosValueKey`): the format, then the fields sorted by name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SinglePosValueKey {
	/// The format of the fields that are present.
	pub format: u16,
	/// The fields, sorted by name.
	pub items: Vec<(&'static str, ValueKeyItem)>,
}

impl SinglePosValueKey {
	/// How many 16-bit words the record takes (`_getSinglePosValueSize`).
	fn size(&self) -> usize {
		self.items
			.iter()
			.map(|(_, v)| match v {
				ValueKeyItem::Device(_, _, _, d) => d.len() + 3,
				_ => 1,
			})
			.sum()
	}
}

const FIELD_NAMES: [&str; 16] = [
	"XPlacement",
	"YPlacement",
	"XAdvance",
	"YAdvance",
	"XPlaDevice",
	"YPlaDevice",
	"XAdvDevice",
	"YAdvDevice",
	"Reserved1",
	"Reserved2",
	"Reserved3",
	"Reserved4",
	"Reserved5",
	"Reserved6",
	"Reserved7",
	"Reserved8",
];

/// The key that makes equal value records equal (`_getSinglePosValueKey`).
pub fn get_single_pos_value_key(v: &ValueRecord) -> SinglePosValueKey {
	let mut items = Vec::new();
	let mut format = 0;
	let ints = [v.x_placement, v.y_placement, v.x_advance, v.y_advance];
	for (i, value) in ints.iter().enumerate() {
		if let Some(x) = value {
			items.push((FIELD_NAMES[i], ValueKeyItem::Int(*x as i64)));
			format |= 1 << i;
		}
	}
	let devices = [
		&v.x_pla_device,
		&v.y_pla_device,
		&v.x_adv_device,
		&v.y_adv_device,
	];
	for (i, value) in devices.iter().enumerate() {
		if let Some(d) = value {
			let item = match d {
				None => ValueKeyItem::None,
				Some(d) => ValueKeyItem::Device(
					d.delta_format,
					d.start_size,
					d.end_size,
					if d.delta_format & 0x8000 != 0 {
						Vec::new()
					} else {
						d.delta_value.clone()
					},
				),
			};
			items.push((FIELD_NAMES[4 + i], item));
			format |= 0x10 << i;
		}
	}
	for (i, value) in v.reserved.iter().enumerate() {
		if let Some(x) = value {
			items.push((FIELD_NAMES[8 + i], ValueKeyItem::Int(*x as i64)));
			format |= 0x100 << i;
		}
	}
	items.sort_by(|a, b| a.0.cmp(b.0));
	SinglePosValueKey { format, items }
}

/// Builds single adjustment (GPOS1) subtables from glyph names to value records
/// (`buildSinglePos`), choosing the formats to make them small: format 1 for
/// records that many glyphs share, format 2 for records with the same fields.
/// The subtables are sorted by decreasing number of glyphs.
pub fn build_single_pos(
	mapping: &IndexMap<String, ValueRecord>,
	glyph_map: &GlyphMap,
) -> Result<Vec<ot::SinglePos>> {
	let mut result: Vec<ot::SinglePos> = Vec::new();
	let mut handled: std::collections::HashSet<SinglePosValueKey> =
		std::collections::HashSet::new();
	let mut coverages: IndexMap<SinglePosValueKey, Vec<&str>> = IndexMap::new();
	let mut masks: IndexMap<u16, Vec<SinglePosValueKey>> = IndexMap::new();
	let mut values: HashMap<SinglePosValueKey, &ValueRecord> = HashMap::new();
	for (glyph, value) in mapping {
		let key = get_single_pos_value_key(value);
		coverages.entry(key.clone()).or_default().push(glyph);
		masks.entry(key.format).or_default().push(key.clone());
		values.insert(key, value);
	}

	// A record shared by several glyphs gets a format 1 subtable, the most
	// compact form. 5 words is the cost of another subtable.
	for (key, glyphs) in &coverages {
		if glyphs.len() * key.size() > 5 {
			let format1: IndexMap<String, ValueRecord> = glyphs
				.iter()
				.map(|g| ((*g).to_owned(), values[key].clone()))
				.collect();
			result.push(build_single_pos_subtable(&format1, glyph_map)?);
			handled.insert(key.clone());
		}
	}

	// Remaining records with the same fields get format 2. Like fontTools, a
	// record counts once for each glyph that uses it.
	for keys in masks.values() {
		let f2: Vec<&SinglePosValueKey> = keys.iter().filter(|k| !handled.contains(*k)).collect();
		if f2.len() > 1 {
			let mut format2: IndexMap<String, ValueRecord> = IndexMap::new();
			for k in &f2 {
				for g in &coverages[*k] {
					format2.insert((*g).to_owned(), values[*k].clone());
				}
			}
			result.push(build_single_pos_subtable(&format2, glyph_map)?);
			for k in f2 {
				handled.insert(k.clone());
			}
		}
	}

	// The rest are used by few glyphs, normally one; they get format 1 again.
	// Like fontTools, only the subtable of the last glyph is kept.
	for (key, glyphs) in &coverages {
		if handled.contains(key) {
			continue;
		}
		let mut st = None;
		for g in glyphs {
			let one: IndexMap<String, ValueRecord> = [((*g).to_owned(), values[key].clone())]
				.into_iter()
				.collect();
			st = Some(build_single_pos_subtable(&one, glyph_map)?);
		}
		result.extend(st);
	}

	// Layout engines stop at the first matching subtable, so the subtables with
	// more glyphs come first; ties go by glyph ID, for determinism.
	let mut keyed: Vec<((i64, u32), ot::SinglePos)> = result
		.into_iter()
		.map(|t| (single_pos_table_key(&t), t))
		.collect();
	keyed.sort_by_key(|(k, _)| *k);
	Ok(keyed.into_iter().map(|(_, t)| t).collect())
}

/// `_getSinglePosTableKey`.
fn single_pos_table_key(subtable: &ot::SinglePos) -> (i64, u32) {
	let coverage = match subtable {
		ot::SinglePos::Format1(t) => &t.coverage,
		ot::SinglePos::Format2(t) => &t.coverage,
		ot::SinglePos::Format3(t) => &t.coverage,
		ot::SinglePos::Format4(t) => &t.coverage,
		ot::SinglePos::Unknown(_) => &None,
	};
	let glyphs = coverage.as_ref().map_or(&[][..], |c| &c.glyphs[..]);
	(-(glyphs.len() as i64), glyphs.first().copied().unwrap_or(0))
}

/// Builds a single adjustment (GPOS1) subtable from glyph names to value records
/// (`buildSinglePosSubtable`): format 1 if all records are equal, format 2
/// otherwise.
pub fn build_single_pos_subtable(
	values: &IndexMap<String, ValueRecord>,
	glyph_map: &GlyphMap,
) -> Result<ot::SinglePos> {
	let glyphs = sorted_glyphs(values.keys().map(String::as_str), glyph_map)?;
	let value_format = values.values().fold(0, |f, v| f | v.format());
	let records: Vec<ValueRecord> = glyphs
		.iter()
		.map(|&(_, g)| value_record_with_format(Some(&values[g]), value_format))
		.collect();
	let coverage = Some(Box::new(Coverage {
		glyphs: glyphs.iter().map(|&(id, _)| id).collect(),
	}));
	let extended = glyph_map.has_extended_glyph_ids();
	if records.iter().all(|v| *v == records[0]) {
		let value = (value_format != 0).then(|| records[0].clone());
		Ok(if extended {
			ot::SinglePos::Format3(ot::SinglePosFormat3 {
				coverage,
				value_format,
				value,
				dont_share: false,
			})
		} else {
			ot::SinglePos::Format1(ot::SinglePosFormat1 {
				coverage,
				value_format,
				value,
				dont_share: false,
			})
		})
	} else {
		let value_count = Some(records.len() as u32);
		let value = records.into_iter().map(Some).collect();
		Ok(if extended {
			ot::SinglePos::Format4(ot::SinglePosFormat4 {
				coverage,
				value_format,
				value_count,
				value,
				dont_share: false,
			})
		} else {
			ot::SinglePos::Format2(ot::SinglePosFormat2 {
				coverage,
				value_format,
				value_count,
				value,
				dont_share: false,
			})
		})
	}
}
