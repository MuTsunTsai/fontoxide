//! Resolution of offset overflows in `GSUB` and `GPOS` with fontTools'
//! pure-Python serializer (`BaseTTXConverter.compile`, `fixLookupOverFlows`,
//! `fixSubTableOverFlows` and the `split…` functions of `otTables`).
//!
//! As in fontTools, the table is modified in place: lookups are promoted to
//! Extension lookups, subtables are marked `DontShare` and split, and it is
//! compiled again until it fits.

use std::any::Any;
use std::collections::BTreeSet;

use fontoxide_misc::Tag;

use super::custom::{ClassDef, Coverage};
use super::table::OtTable;
use super::writer::{OverflowErrorRecord, WriterGraph};
use crate::generated::ot::{
	self, ExtensionPos, ExtensionPosFormat1, ExtensionSubst, ExtensionSubstFormat1, Lookup,
	LookupSubTable, MarkBasePos, PairPos, SinglePos,
};
use crate::{Error, Font, Result};

const GSUB: Tag = Tag::new(b"GSUB");
const GPOS: Tag = Tag::new(b"GPOS");

/// How a table is being packed (`RepackerState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepackerState {
	/// Only with fontTools' serializer, without sharing across Extension
	/// subtables.
	PureFt,
	/// With the HarfBuzz repacker (sharing across Extension subtables), using
	/// fontTools' overflow resolution when it fails.
	HbFt,
	/// fontTools' serializer only, until it fits; then back to `HbFt` for the
	/// final packing.
	FtFallback,
}

/// The version of a `GSUB` or `GPOS` table (`Version`); 0 for other tables.
fn layout_version<T: Any>(table: &T) -> u32 {
	let table = table as &dyn Any;
	if let Some(t) = table.downcast_ref::<ot::GSUB>() {
		return t.version;
	}
	if let Some(t) = table.downcast_ref::<ot::GPOS>() {
		return t.version;
	}
	0
}

/// Compiles an otData table, resolving overflows in `GSUB` and `GPOS` lookups
/// (`BaseTTXConverter.compile`). `GSUB` and `GPOS` are packed with the
/// HarfBuzz repacker as [`Font::use_harfbuzz_repacker`] says.
pub fn compile_table<T: OtTable + Any>(
	table: &mut T,
	tag: Tag,
	font: &mut Font,
) -> Result<Vec<u8>> {
	let is_layout = tag == GSUB || tag == GPOS;
	let mut use_hb_repack = font.use_harfbuzz_repacker;
	if is_layout && use_hb_repack.is_none() && layout_version(table) >= 0x0001_0002 {
		use_hb_repack = Some(false);
	}
	let mut state = if use_hb_repack != Some(false) && is_layout {
		RepackerState::HbFt
	} else {
		RepackerState::PureFt
	};
	let mut hb_first_error_logged = false;
	loop {
		let mut graph = WriterGraph::new(tag);
		table.compile(&mut graph.root(), font)?;
		let result = match state {
			RepackerState::HbFt => graph.get_all_data_using_harfbuzz(!hb_first_error_logged),
			RepackerState::PureFt => graph.get_all_data(),
			RepackerState::FtFallback => match graph.get_all_data() {
				Ok(_) => {
					// Packing with fontTools only succeeded: pack again with HarfBuzz
					// for the final, optimal, packing.
					log::debug!(
						"Re-enabling sharing between extensions and switching back to harfbuzz+fontTools packing."
					);
					state = RepackerState::HbFt;
					continue;
				}
				Err(e) => Err(e),
			},
		};
		let record = match result {
			Ok(data) => return Ok(data),
			Err(Error::OffsetOverflow(record)) => record,
			Err(e) => return Err(e),
		};
		hb_first_error_logged = true;
		// UPSTREAM-BUG(B11): `tryResolveOverflow` gives up when the same overflow
		// comes back, but `OverflowErrorRecord` has no `__eq__`, so the check never
		// matches; resolution stops only when no fix applies.
		log::info!("Attempting to fix OTLOffsetOverflowError {record:?}");
		let Some(lookups) = lookups_mut(table) else {
			return Err(Error::ttlib(
				"AttributeError: 'NoneType' object has no attribute 'Lookup'",
			));
		};
		if try_resolve_overflow(lookups, tag, &record, font)? {
			continue;
		}
		if state == RepackerState::HbFt {
			log::debug!(
				"Harfbuzz packing out of resolutions, disabling sharing between extensions and switching to fontTools only packing."
			);
			state = RepackerState::FtFallback;
		} else {
			return Err(Error::OffsetOverflow(record));
		}
	}
}

/// The lookups of a `GSUB` or `GPOS` table (`_getLookupList`): the extended
/// list for version 1.2 and later, else the compact one; whichever exists.
fn lookups_mut<T: Any>(table: &mut T) -> Option<&mut Vec<Option<Box<Lookup>>>> {
	let table = table as &mut dyn Any;
	macro_rules! layout_list {
		($t:expr) => {{
			let t = $t;
			if t.version >= 0x0001_0002 && t.lookup_list2.is_some() || t.lookup_list.is_none() {
				t.lookup_list2.as_deref_mut().map(|l| &mut l.lookup)
			} else {
				t.lookup_list.as_deref_mut().map(|l| &mut l.lookup)
			}
		}};
	}
	if table.is::<ot::GSUB>() {
		return layout_list!(table.downcast_mut::<ot::GSUB>()?);
	}
	if table.is::<ot::GPOS>() {
		return layout_list!(table.downcast_mut::<ot::GPOS>()?);
	}
	None
}

/// `tryResolveOverflow`.
fn try_resolve_overflow(
	lookups: &mut [Option<Box<Lookup>>],
	tag: Tag,
	record: &OverflowErrorRecord,
	font: &mut Font,
) -> Result<bool> {
	if record.item_name.is_none() {
		return fix_lookup_overflows(lookups, tag, record);
	}
	if fix_sub_table_overflows(lookups, tag, record, font)? {
		return Ok(true);
	}
	// Promoting the lookup to an Extension may help, as Extension subtables are
	// not shared across lookups.
	fix_lookup_overflows(lookups, tag, record)
}

fn ext_type(tag: Tag) -> Result<u16> {
	match tag {
		GSUB => Ok(7),
		GPOS => Ok(9),
		_ => Err(Error::ttlib("UnboundLocalError: 'extType'")),
	}
}

/// The lookup type of a subtable's class in a table (`__class__.LookupType`).
fn lookup_type(sub: &LookupSubTable, tag: Tag) -> Option<u16> {
	use LookupSubTable as L;
	Some(match (tag, sub) {
		(GSUB, L::SingleSubst(_)) => 1,
		(GSUB, L::MultipleSubst(_)) => 2,
		(GSUB, L::AlternateSubst(_)) => 3,
		(GSUB, L::LigatureSubst(_)) => 4,
		(GSUB, L::ContextSubst(_)) => 5,
		(GSUB, L::ChainContextSubst(_)) => 6,
		(GSUB, L::ExtensionSubst(_)) => 7,
		(GSUB, L::ReverseChainSingleSubst(_)) => 8,
		(GPOS, L::SinglePos(_)) => 1,
		(GPOS, L::PairPos(_)) => 2,
		(GPOS, L::CursivePos(_)) => 3,
		(GPOS, L::MarkBasePos(_)) => 4,
		(GPOS, L::MarkLigPos(_)) => 5,
		(GPOS, L::MarkMarkPos(_)) => 6,
		(GPOS, L::ContextPos(_)) => 7,
		(GPOS, L::ChainContextPos(_)) => 8,
		(GPOS, L::ExtensionPos(_)) => 9,
		_ => return None,
	})
}

/// A new subtable of a lookup type (`lookupTypes[tag][type]()`).
fn new_sub_table(tag: Tag, lookup_type: u16) -> Option<LookupSubTable> {
	use LookupSubTable as L;
	Some(match (tag, lookup_type) {
		(GSUB, 2) => L::MultipleSubst(Default::default()),
		(GSUB, 3) => L::AlternateSubst(Default::default()),
		(GSUB, 4) => L::LigatureSubst(Default::default()),
		(GPOS, 1) => L::SinglePos(Default::default()),
		(GPOS, 2) => L::PairPos(Default::default()),
		(GPOS, 4) => L::MarkBasePos(Default::default()),
		_ => return None,
	})
}

fn none_attr(name: &str) -> Error {
	Error::ttlib(format!(
		"AttributeError: 'NoneType' object has no attribute '{name}'"
	))
}

fn index_error() -> Error {
	Error::ttlib("IndexError: list index out of range")
}

fn lookup_at(lookups: &mut [Option<Box<Lookup>>], index: usize) -> Result<&mut Lookup> {
	lookups
		.get_mut(index)
		.ok_or_else(index_error)?
		.as_deref_mut()
		.ok_or_else(|| none_attr("SubTable"))
}

/// `fixLookupOverFlows`: promotes lookups to Extension lookups, from the one
/// that overflowed (or the one before, when the offset to a lookup overflowed)
/// to the end.
fn fix_lookup_overflows(
	lookups: &mut [Option<Box<Lookup>>],
	tag: Tag,
	record: &OverflowErrorRecord,
) -> Result<bool> {
	let index = record.lookup_list_index.ok_or_else(|| {
		Error::ttlib("TypeError: unsupported operand type(s) for -: 'NoneType' and 'int'")
	})? as i64;
	let mut index = if record.sub_table_index.is_none() {
		index - 1
	} else {
		index
	};
	if index < 0 {
		return Ok(false);
	}
	let ext = ext_type(tag)?;
	// If the lookup is an Extension already, look further back.
	loop {
		let lookup = lookup_at(lookups, index as usize)?;
		let first = lookup
			.sub_table
			.first()
			.ok_or_else(index_error)?
			.as_deref()
			.ok_or_else(|| none_attr("LookupType"))?;
		if lookup_type(first, tag) != Some(ext) {
			break;
		}
		index -= 1;
		if index < 0 {
			if record.sub_table_index.is_none() {
				log::error!(
					"{tag} LookupList offset overflowed and all lookups are already Extension lookups, so the overflow can't be resolved by promotion; reduce the number of lookups."
				);
			}
			return Ok(false);
		}
	}
	let mut ok = false;
	for i in index as usize..lookups.len() {
		let lookup = lookup_at(lookups, i)?;
		if lookup.lookup_type == ext {
			continue;
		}
		let inner_type = lookup.lookup_type;
		lookup.lookup_type = ext;
		for sub in &mut lookup.sub_table {
			let inner = sub.take();
			*sub = Some(Box::new(extension(tag, inner_type, inner)));
			ok = true;
		}
	}
	Ok(ok)
}

/// An Extension subtable (format 1) around a subtable.
fn extension(tag: Tag, inner_type: u16, inner: Option<Box<LookupSubTable>>) -> LookupSubTable {
	if tag == GSUB {
		LookupSubTable::ExtensionSubst(ExtensionSubst::Format1(ExtensionSubstFormat1 {
			extension_lookup_type: inner_type,
			ext_sub_table: inner,
			dont_share: false,
		}))
	} else {
		LookupSubTable::ExtensionPos(ExtensionPos::Format1(ExtensionPosFormat1 {
			extension_lookup_type: inner_type,
			ext_sub_table: inner,
			dont_share: false,
		}))
	}
}

/// The subtable inside an Extension subtable (`ExtSubTable`), if it is one.
fn ext_sub_table(sub: &mut LookupSubTable) -> Option<&mut Option<Box<LookupSubTable>>> {
	match sub {
		LookupSubTable::ExtensionSubst(ExtensionSubst::Format1(e)) => Some(&mut e.ext_sub_table),
		LookupSubTable::ExtensionPos(ExtensionPos::Format1(e)) => Some(&mut e.ext_sub_table),
		_ => None,
	}
}

/// `fixSubTableOverFlows`: first stops sharing the subtables below the one that
/// overflowed; then splits it in two.
fn fix_sub_table_overflows(
	lookups: &mut [Option<Box<Lookup>>],
	tag: Tag,
	record: &OverflowErrorRecord,
	font: &mut Font,
) -> Result<bool> {
	let lookup_index = record.lookup_list_index.ok_or_else(|| {
		Error::ttlib("TypeError: list indices must be integers or slices, not NoneType")
	})?;
	let sub_index = record.sub_table_index.ok_or_else(|| {
		Error::ttlib("TypeError: list indices must be integers or slices, not NoneType")
	})?;
	let lookup = lookup_at(lookups, lookup_index)?;
	let sub = lookup
		.sub_table
		.get_mut(sub_index)
		.ok_or_else(index_error)?
		.as_deref_mut()
		.ok_or_else(|| none_attr("DontShare"))?;
	if !sub.dont_share() {
		sub.set_dont_share();
		return Ok(true);
	}
	let (sub_type, to_insert, target) = match ext_sub_table(sub) {
		Some(inner) => {
			let inner = inner
				.as_deref_mut()
				.ok_or_else(|| none_attr("LookupType"))?;
			let sub_type = lookup_type(inner, tag);
			let new = sub_type.and_then(|t| new_sub_table(tag, t));
			(sub_type, new.map(|n| (true, n)), inner)
		}
		None => {
			let sub_type = lookup_type(sub, tag);
			let new = sub_type.and_then(|t| new_sub_table(tag, t));
			(sub_type, new.map(|n| (false, n)), sub)
		}
	};
	let Some((wrapped, mut new)) = to_insert else {
		if let Some(c) = lookup.sub_table_count.as_mut() {
			*c += 1;
		}
		log::error!(
			"Don't know how to split {tag} lookup type {}",
			sub_type.map_or_else(|| "None".to_owned(), |t| t.to_string())
		);
		return Ok(false);
	};
	let ok = split(target, &mut new, record, font)?;
	if let Some(c) = lookup.sub_table_count.as_mut() {
		*c += 1;
	}
	if ok {
		let new = if wrapped {
			extension(tag, sub_type.unwrap_or_default(), Some(Box::new(new)))
		} else {
			new
		};
		lookup.sub_table.insert(sub_index + 1, Some(Box::new(new)));
	}
	Ok(ok)
}

/// `splitTable[tag][type](old, new, record)`.
fn split(
	old: &mut LookupSubTable,
	new: &mut LookupSubTable,
	record: &OverflowErrorRecord,
	font: &mut Font,
) -> Result<bool> {
	use LookupSubTable as L;
	match (old, new) {
		(L::MultipleSubst(o), L::MultipleSubst(n)) => {
			split_mapping(&mut o.mapping, &mut n.mapping, record, "Sequence", font)
		}
		(L::AlternateSubst(o), L::AlternateSubst(n)) => split_mapping(
			&mut o.alternates,
			&mut n.alternates,
			record,
			"AlternateSet",
			font,
		),
		(L::LigatureSubst(o), L::LigatureSubst(n)) => split_mapping(
			&mut o.ligatures,
			&mut n.ligatures,
			record,
			"LigatureSet",
			font,
		),
		(L::SinglePos(o), L::SinglePos(n)) => split_single_pos(o, n),
		(L::PairPos(o), L::PairPos(n)) => split_pair_pos(o, n),
		(L::MarkBasePos(o), L::MarkBasePos(n)) => split_mark_base_pos(o, n),
		_ => Ok(false),
	}
}

/// `splitMultipleSubst` / `splitAlternateSubst` / `splitLigatureSubst`: moves the
/// items from a point on, in glyph name order, to the new subtable.
fn split_mapping<V: Clone>(
	old: &mut std::collections::BTreeMap<u32, V>,
	new: &mut std::collections::BTreeMap<u32, V>,
	record: &OverflowErrorRecord,
	set_name: &str,
	font: &mut Font,
) -> Result<bool> {
	let order = font.glyph_order()?.clone();
	let mut keys: Vec<(String, u32)> = old
		.keys()
		.map(|&g| (order.name(g).into_owned(), g))
		.collect();
	keys.sort();
	let len = keys.len() as i64;
	let new_len = match record.item_name.as_deref() {
		Some("Coverage" | "RangeRecord") => len / 2,
		Some(name) if name == set_name => {
			record.item_index.ok_or_else(|| {
				Error::ttlib("TypeError: unsupported operand type(s) for -: 'NoneType' and 'int'")
			})? as i64 - 1
		}
		_ => return Err(Error::ttlib("UnboundLocalError: 'newLen'")),
	};
	new.clear();
	for i in new_len..len {
		// Python indexing: negative indices count from the end.
		let at = if i < 0 { i + len } else { i };
		let key = keys.get(at as usize).ok_or_else(index_error)?.1;
		let value = old
			.remove(&key)
			.ok_or_else(|| Error::ttlib(format!("KeyError: {:?}", order.name(key))))?;
		new.insert(key, value);
	}
	Ok(true)
}

fn coverage_glyphs(c: &Option<Box<Coverage>>) -> Result<Vec<u32>> {
	Ok(c.as_deref()
		.ok_or_else(|| none_attr("glyphs"))?
		.glyphs
		.clone())
}

fn coverage(glyphs: Vec<u32>) -> Option<Box<Coverage>> {
	Some(Box::new(Coverage { glyphs }))
}

/// `splitSinglePos`: only format 2 has a value per glyph worth splitting.
fn split_single_pos(old: &mut SinglePos, new: &mut SinglePos) -> Result<bool> {
	let SinglePos::Format2(o) = old else {
		return Ok(false);
	};
	let glyphs = coverage_glyphs(&o.coverage)?;
	if glyphs.len() <= 1 {
		return Ok(false);
	}
	let at = glyphs.len() / 2;
	let values = std::mem::take(&mut o.value);
	let (old_values, new_values) = values.split_at(at.min(values.len()));
	let n = ot::SinglePosFormat2 {
		coverage: coverage(glyphs[at..].to_vec()),
		value_format: o.value_format,
		value_count: Some(new_values.len() as u32),
		value: new_values.to_vec(),
		dont_share: false,
	};
	o.coverage = coverage(glyphs[..at].to_vec());
	o.value = old_values.to_vec();
	o.value_count = Some(o.value.len() as u32);
	*new = SinglePos::Format2(n);
	Ok(true)
}

/// `splitPairPos`: format 1 is split by coverage, format 2 by first class.
fn split_pair_pos(old: &mut PairPos, new: &mut PairPos) -> Result<bool> {
	match old {
		PairPos::Format1(o) if o.pair_set.len() > 1 => {
			let glyphs = coverage_glyphs(&o.coverage)?;
			let at = o.pair_set.len() / 2;
			let new_sets = o.pair_set.split_off(at);
			let n = ot::PairPosFormat1 {
				coverage: coverage(glyphs.get(at..).unwrap_or_default().to_vec()),
				value_format1: o.value_format1,
				value_format2: o.value_format2,
				pair_set_count: Some(new_sets.len() as u32),
				pair_set: new_sets,
				dont_share: false,
			};
			o.coverage = coverage(glyphs[..at.min(glyphs.len())].to_vec());
			o.pair_set_count = Some(o.pair_set.len() as u32);
			*new = PairPos::Format1(n);
			Ok(true)
		}
		PairPos::Format2(o) if o.class1_record.len() > 1 => {
			let class2_count = match o.class2_count {
				Some(c) => c,
				None => o.class1_record[0].class2_record.len() as u32,
			};
			o.class2_count = Some(class2_count);
			// Both halves keep ClassDef2, whose sharing would overflow again: the
			// one written second (the old one) does not share.
			o.dont_share = true;
			let glyphs = coverage_glyphs(&o.coverage)?;
			let class_defs = o
				.class_def1
				.as_deref()
				.ok_or_else(|| none_attr("classDefs"))?
				.class_defs
				.clone();
			let at = (o.class1_record.len() / 2) as u32;
			let moved: BTreeSet<u32> = class_defs
				.iter()
				.filter(|&(_, &c)| c >= at)
				.map(|(&g, _)| g)
				.collect();
			let new_records = o.class1_record.split_off(at as usize);
			// Glyphs of class `at` become class 0 of the new subtable, which a
			// ClassDef leaves out: they are in its coverage, so they still get
			// the first record (formerly B12, withdrawn: not a bug).
			let n = ot::PairPosFormat2 {
				coverage: coverage(
					glyphs
						.iter()
						.copied()
						.filter(|g| moved.contains(g))
						.collect(),
				),
				value_format1: o.value_format1,
				value_format2: o.value_format2,
				class_def1: Some(Box::new(ClassDef {
					class_defs: class_defs
						.iter()
						.filter(|&(_, &c)| c > at)
						.map(|(&g, &c)| (g, c - at))
						.collect(),
				})),
				class_def2: o.class_def2.clone(),
				class1_count: Some(new_records.len() as u32),
				class2_count: Some(class2_count),
				class1_record: new_records,
				dont_share: false,
			};
			o.coverage = coverage(glyphs.into_iter().filter(|g| !moved.contains(g)).collect());
			o.class_def1 = Some(Box::new(ClassDef {
				class_defs: class_defs.into_iter().filter(|&(_, c)| c < at).collect(),
			}));
			o.class1_count = Some(o.class1_record.len() as u32);
			*new = PairPos::Format2(n);
			Ok(true)
		}
		_ => Ok(false),
	}
}

/// `splitMarkBasePos`: half of the mark classes go to the new subtable.
fn split_mark_base_pos(old: &mut MarkBasePos, new: &mut MarkBasePos) -> Result<bool> {
	macro_rules! split_format {
		($o:expr, $variant:ident, $fmt:ident, $marks:ident, $bases:ident, $base_record:ident) => {{
			let o = $o;
			let class_count = o.class_count.ok_or_else(|| none_attr("ClassCount"))?;
			if class_count < 2 {
				return Ok(false);
			}
			let old_classes = class_count / 2;
			let new_classes = class_count - old_classes;
			let glyphs = coverage_glyphs(&o.mark_coverage)?;
			let marks = o
				.mark_array
				.as_deref_mut()
				.ok_or_else(|| none_attr("MarkRecord"))?;
			let (mut old_glyphs, mut old_marks) = (Vec::new(), Vec::new());
			let (mut new_glyphs, mut new_marks) = (Vec::new(), Vec::new());
			for (g, mut rec) in glyphs
				.into_iter()
				.zip(std::mem::take(&mut marks.mark_record))
			{
				if (rec.class as u32) < old_classes {
					old_glyphs.push(g);
					old_marks.push(rec);
				} else {
					rec.class -= old_classes as u16;
					new_glyphs.push(g);
					new_marks.push(rec);
				}
			}
			let bases = o
				.base_array
				.as_deref_mut()
				.ok_or_else(|| none_attr("BaseRecord"))?;
			let (mut old_bases, mut new_bases) = (Vec::new(), Vec::new());
			for rec in &bases.base_record {
				let split = (old_classes as usize).min(rec.base_anchor.len());
				old_bases.push(ot::$base_record {
					base_anchor: rec.base_anchor[..split].to_vec(),
				});
				new_bases.push(ot::$base_record {
					base_anchor: rec.base_anchor[split..].to_vec(),
				});
			}
			marks.mark_count = Some(old_marks.len() as u32);
			marks.mark_record = old_marks;
			bases.base_count = Some(old_bases.len() as u32);
			bases.base_record = old_bases;
			o.mark_coverage = coverage(old_glyphs);
			o.class_count = Some(old_classes);
			let n = ot::$fmt {
				mark_coverage: coverage(new_glyphs),
				base_coverage: o.base_coverage.clone(),
				class_count: Some(new_classes),
				mark_array: Some(Box::new(ot::$marks {
					mark_count: Some(new_marks.len() as u32),
					mark_record: new_marks,
				})),
				base_array: Some(Box::new(ot::$bases {
					base_count: Some(new_bases.len() as u32),
					base_record: new_bases,
				})),
				dont_share: false,
			};
			*new = MarkBasePos::$variant(n);
			Ok(true)
		}};
	}
	match old {
		MarkBasePos::Format1(o) => {
			split_format!(
				o,
				Format1,
				MarkBasePosFormat1,
				MarkArray,
				BaseArray,
				BaseRecord
			)
		}
		MarkBasePos::Format2(o) => {
			split_format!(
				o,
				Format2,
				MarkBasePosFormat2,
				MarkArray2,
				BaseArray2,
				BaseRecord2
			)
		}
		MarkBasePos::Unknown(_) => Err(none_attr("ClassCount")),
	}
}
