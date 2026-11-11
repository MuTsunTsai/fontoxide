//! The otTables classes whose fontTools representation differs from their binary
//! layout (they define `postRead` / `preWrite`): `Coverage`, `ClassDef`, the GSUB
//! substitution subtables, and others. Their binary layouts are generated as
//! `<Name>Raw`.
//!
//! Classes not written by hand yet are aliases of their raw layout.

use std::collections::BTreeMap;

use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use super::table::{OtTable, XmlCtx, simpletag};
use super::{OtReader, OtWriter};
use crate::generated::ot::{
	ClassDefRaw, ClassDefRawFormat1, ClassDefRawFormat2, ClassDefRawFormat3, ClassDefRawFormat4,
	ClassRangeRecord, ClassRangeRecord2, CoverageRaw, CoverageRawFormat1, CoverageRawFormat2,
	CoverageRawFormat3, CoverageRawFormat4, LookupList, RangeRecord, RangeRecord2,
};
use crate::table::TableType;
use crate::tables::Debg;
use crate::tables::json::JsonValue;
use crate::{Error, Font, Result};

pub use super::colr::ClipList;
pub use super::gsub::{AlternateSubst, LigatureSubst, MultipleSubst, SingleSubst};
pub use super::ift::{EntryIdStringData, MappingEntry};
pub use super::morph::{MortSubtable, MorxSubtable};
pub use super::varc::{DecomposedTransform, VarComponent, VarCompositeGlyph};
pub use super::varmap::{DeltaSetIndexMap, VarIdxMap};

/// Whether the font has glyph IDs above 65535 (`hasExtendedGlyphIDs`).
pub fn has_extended_glyph_ids(font: &mut Font) -> Result<bool> {
	Ok(font.glyph_order()?.len() > 0x10000)
}

/// A coverage table: an ordered list of glyphs (`Coverage`, which fontTools
/// keeps as `glyphs`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
	/// The covered glyphs, in coverage index order.
	pub glyphs: Vec<u32>,
}

impl Coverage {
	/// `postRead`: the table from its binary layout.
	pub fn from_raw(raw: CoverageRaw) -> Self {
		let glyphs = match raw {
			CoverageRaw::Format1(CoverageRawFormat1 { glyph_array, .. })
			| CoverageRaw::Format3(CoverageRawFormat3 { glyph_array, .. }) => glyph_array,
			CoverageRaw::Format2(CoverageRawFormat2 { range_record, .. }) => {
				let ranges: Vec<(u32, u32, u32)> = range_record
					.iter()
					.map(|r| (r.start, r.end, r.start_coverage_index as u32))
					.collect();
				expand_ranges(ranges)
			}
			CoverageRaw::Format4(CoverageRawFormat4 { range_record, .. }) => {
				let ranges = range_record
					.iter()
					.map(|r| (r.start, r.end, r.start_coverage_index))
					.collect();
				expand_ranges(ranges)
			}
			CoverageRaw::Unknown(f) => {
				log::warn!("Unknown Coverage format: {f}");
				Vec::new()
			}
		};
		Coverage { glyphs }
	}

	/// `preWrite`: the table as it is written, with its format.
	pub fn to_raw(&self, font: &mut Font) -> Result<CoverageRaw> {
		let glyphs = &self.glyphs;
		let Some(&first) = glyphs.first() else {
			return Ok(CoverageRaw::Format1(CoverageRawFormat1 {
				glyph_count: None,
				glyph_array: Vec::new(),
			}));
		};
		let broken_order = glyphs.windows(2).any(|w| w[1] < w[0]);
		let mut last = first;
		let mut ranges = vec![(first, first)];
		for &gid in &glyphs[1..] {
			if gid != last.wrapping_add(1) {
				if let Some(r) = ranges.last_mut() {
					r.1 = last;
				}
				ranges.push((gid, gid));
			}
			last = gid;
		}
		if let Some(r) = ranges.last_mut() {
			r.1 = last;
		}
		let max = glyphs.iter().copied().max().unwrap_or(0);
		let extended = has_extended_glyph_ids(font)? && (max > 0xFFFF || glyphs.len() > 0xFFFF);
		if broken_order || ranges.len() * 3 < glyphs.len() {
			// The range format is more compact.
			let mut index = 0u32;
			let mut records: Vec<(u32, u32, u32)> = Vec::with_capacity(ranges.len());
			for &(start, end) in &ranges {
				records.push((start, end, index));
				index = index.wrapping_add(end.wrapping_sub(start)).wrapping_add(1);
			}
			if broken_order {
				log::warn!("GSUB/GPOS Coverage is not sorted by glyph ids.");
				records.sort_by_key(|r| r.0);
			}
			return Ok(if extended {
				CoverageRaw::Format4(CoverageRawFormat4 {
					range_count: None,
					range_record: records
						.into_iter()
						.map(|(start, end, index)| RangeRecord2 {
							start,
							end,
							start_coverage_index: index,
						})
						.collect(),
				})
			} else {
				CoverageRaw::Format2(CoverageRawFormat2 {
					range_count: None,
					range_record: records
						.into_iter()
						.map(|(start, end, index)| {
							Ok(RangeRecord {
								start,
								end,
								start_coverage_index: u16::try_from(index).map_err(|_| {
									Error::Struct(format!(
										"'H' format requires 0 <= number <= 65535: {index}"
									))
								})?,
							})
						})
						.collect::<Result<_>>()?,
				})
			});
		}
		Ok(if extended {
			CoverageRaw::Format3(CoverageRawFormat3 {
				glyph_count: None,
				glyph_array: glyphs.clone(),
			})
		} else {
			CoverageRaw::Format1(CoverageRawFormat1 {
				glyph_count: None,
				glyph_array: glyphs.clone(),
			})
		})
	}
}

/// Expands (start, end, start coverage index) ranges, sorted by coverage index.
fn expand_ranges(mut ranges: Vec<(u32, u32, u32)>) -> Vec<u32> {
	let sorted = {
		let mut s = ranges.clone();
		s.sort_by_key(|r| r.2);
		s
	};
	if sorted != ranges {
		log::warn!("GSUB/GPOS Coverage is not sorted by glyph ids.");
		ranges = sorted;
	}
	let mut glyphs = Vec::new();
	for (start, end, _) in ranges {
		glyphs.extend(start..end.saturating_add(1));
	}
	glyphs
}

impl OtTable for Coverage {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		for g in &mut self.glyphs {
			*g = f(*g)?;
		}
		Ok(())
	}

	fn class_name(&self) -> &'static str {
		"Coverage"
	}

	fn decompile(r: &mut OtReader<'_>, font: &mut Font) -> Result<Self> {
		Ok(Coverage::from_raw(CoverageRaw::decompile(r, font)?))
	}

	fn compile(&self, w: &mut OtWriter<'_>, font: &mut Font) -> Result<()> {
		self.to_raw(font)?.compile(w, font)
	}

	fn to_xml2(&self, xw: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let order = font.glyph_order()?;
		for &gid in &self.glyphs {
			simpletag(xw, "Glyph", &[("value", order.name(gid).into_owned())]);
			xw.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font, _ctx: &mut XmlCtx) -> Result<()> {
		let name = element
			.attr("value")
			.ok_or_else(|| Error::ttx("KeyError: 'value'"))?;
		self.glyphs.push(font.glyph_id(name)?);
		Ok(())
	}

	fn populate_defaults(&mut self, _ctx: &mut XmlCtx) -> Result<()> {
		Ok(())
	}
}

/// A class definition table: glyph to class (`ClassDef`, which fontTools keeps as
/// `classDefs`). Glyphs of class 0 are not listed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassDef {
	/// The class of each glyph that is not in class 0.
	pub class_defs: BTreeMap<u32, u32>,
}

impl ClassDef {
	/// `postRead`: the table from its binary layout.
	pub fn from_raw(raw: ClassDefRaw) -> Self {
		let mut class_defs = BTreeMap::new();
		let mut array = |start: u32, classes: &mut dyn Iterator<Item = u32>| {
			for (i, cls) in classes.enumerate() {
				if cls != 0 {
					class_defs.insert(start + i as u32, cls);
				}
			}
		};
		match raw {
			ClassDefRaw::Format1(ClassDefRawFormat1 {
				start_glyph,
				class_value_array,
				..
			}) => array(
				start_glyph,
				&mut class_value_array.iter().map(|&c| c as u32),
			),
			ClassDefRaw::Format3(ClassDefRawFormat3 {
				start_glyph,
				class_value_array,
				..
			}) => array(start_glyph, &mut class_value_array.into_iter()),
			ClassDefRaw::Format2(ClassDefRawFormat2 {
				class_range_record, ..
			}) => {
				for rec in class_range_record {
					if rec.class == 0 {
						continue;
					}
					for gid in rec.start..rec.end.saturating_add(1) {
						class_defs.insert(gid, rec.class as u32);
					}
				}
			}
			ClassDefRaw::Format4(ClassDefRawFormat4 {
				class_range_record, ..
			}) => {
				for rec in class_range_record {
					if rec.class == 0 {
						continue;
					}
					for gid in rec.start..rec.end.saturating_add(1) {
						class_defs.insert(gid, rec.class as u32);
					}
				}
			}
			ClassDefRaw::Unknown(f) => log::warn!("Unknown ClassDef format: {f}"),
		}
		ClassDef { class_defs }
	}

	/// `_getClassRanges`: (class, start, end) runs of consecutive glyphs.
	fn class_ranges(&self) -> Vec<(u32, u32, u32)> {
		let mut ranges: Vec<(u32, u32, u32)> = Vec::new();
		let mut last: Option<(u32, u32)> = None;
		for (&gid, &cls) in self.class_defs.iter().filter(|(_, c)| **c != 0) {
			match (last, ranges.last_mut()) {
				(Some((last_gid, last_cls)), Some(r)) if gid == last_gid + 1 && cls == last_cls => {
					r.2 = gid
				}
				_ => ranges.push((cls, gid, gid)),
			}
			last = Some((gid, cls));
		}
		ranges
	}

	/// `preWrite`: the table as it is written, with its format.
	pub fn to_raw(&self) -> Result<ClassDefRaw> {
		let ranges = self.class_ranges();
		if ranges.is_empty() {
			return Ok(ClassDefRaw::Format2(ClassDefRawFormat2 {
				class_range_count: None,
				class_range_record: Vec::new(),
			}));
		}
		let start_glyph = ranges[0].1;
		let end_glyph = ranges[ranges.len() - 1].2;
		let glyph_count = (end_glyph - start_glyph + 1) as u64;
		let max_class = ranges.iter().map(|r| r.0).max().unwrap_or(0);
		let extended = start_glyph > 0xFFFF
			|| end_glyph > 0xFFFF
			|| glyph_count > 0xFFFF
			|| ranges.len() > 0xFFFF
			|| max_class > 0xFFFF;
		let mut range_size = 4.0 + ranges.len() as f64 * 6.0;
		if extended {
			range_size = if max_class <= 0xFFFF {
				5.0 + ranges.len() as f64 * 8.0
			} else {
				f64::INFINITY
			};
		}
		let array_size = if extended {
			8.0 + glyph_count as f64 * 3.0
		} else {
			6.0 + glyph_count as f64 * 2.0
		};
		if range_size < array_size {
			// The range format is more compact.
			return Ok(if extended {
				ClassDefRaw::Format4(ClassDefRawFormat4 {
					class_range_count: None,
					class_range_record: ranges
						.iter()
						.map(|&(cls, start, end)| ClassRangeRecord2 {
							start,
							end,
							class: cls as u16,
						})
						.collect(),
				})
			} else {
				ClassDefRaw::Format2(ClassDefRawFormat2 {
					class_range_count: None,
					class_range_record: ranges
						.iter()
						.map(|&(cls, start, end)| ClassRangeRecord {
							start,
							end,
							class: cls as u16,
						})
						.collect(),
				})
			});
		}
		// The array format is more compact.
		let mut classes = vec![0u32; glyph_count as usize];
		for &(cls, start, end) in &ranges {
			for g in start - start_glyph..=end - start_glyph {
				classes[g as usize] = cls;
			}
		}
		Ok(if extended {
			ClassDefRaw::Format3(ClassDefRawFormat3 {
				start_glyph,
				glyph_count: None,
				class_value_array: classes,
			})
		} else {
			ClassDefRaw::Format1(ClassDefRawFormat1 {
				start_glyph,
				glyph_count: None,
				class_value_array: classes.into_iter().map(|c| c as u16).collect(),
			})
		})
	}
}

impl OtTable for ClassDef {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		self.class_defs = super::aat::remap_keys(std::mem::take(&mut self.class_defs), f)?;
		Ok(())
	}

	fn class_name(&self) -> &'static str {
		"ClassDef"
	}

	fn decompile(r: &mut OtReader<'_>, font: &mut Font) -> Result<Self> {
		Ok(ClassDef::from_raw(ClassDefRaw::decompile(r, font)?))
	}

	fn compile(&self, w: &mut OtWriter<'_>, font: &mut Font) -> Result<()> {
		self.to_raw()?.compile(w, font)
	}

	fn to_xml2(&self, xw: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let order = font.glyph_order()?;
		let mut items: Vec<(String, u32)> = self
			.class_defs
			.iter()
			.map(|(&gid, &cls)| (order.name(gid).into_owned(), cls))
			.collect();
		items.sort();
		for (name, cls) in items {
			simpletag(
				xw,
				"ClassDef",
				&[("glyph", name), ("class", cls.to_string())],
			);
			xw.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font, _ctx: &mut XmlCtx) -> Result<()> {
		let glyph = element
			.attr("glyph")
			.ok_or_else(|| Error::ttx("KeyError: 'glyph'"))?;
		let cls = element
			.attr("class")
			.ok_or_else(|| Error::ttx("KeyError: 'class'"))?;
		let cls = fontoxide_misc::py::parse_int(cls)
			.and_then(|v| u32::try_from(v).ok())
			.ok_or_else(|| Error::value(format!("invalid literal for int(): {cls:?}")))?;
		self.class_defs.insert(font.glyph_id(glyph)?, cls);
		Ok(())
	}

	fn populate_defaults(&mut self, _ctx: &mut XmlCtx) -> Result<()> {
		Ok(())
	}
}

/// The key of feaLib's lookup debugging information in the `Debg` table
/// (`LOOKUP_DEBUG_INFO_KEY`).
pub const LOOKUP_DEBUG_INFO_KEY: &str = "com.github.fonttools.feaLib";

/// The Python type name of a JSON value.
fn py_type_name(value: &JsonValue) -> &'static str {
	match value {
		JsonValue::Null => "NoneType",
		JsonValue::Bool(_) => "bool",
		JsonValue::Int(_) => "int",
		JsonValue::Float(_) => "float",
		JsonValue::Str(_) => "str",
		JsonValue::Array(_) => "list",
		JsonValue::Object(_) => "dict",
	}
}

/// `repr(value)` of a JSON value.
fn py_repr(value: &JsonValue) -> String {
	match value {
		JsonValue::Null => "None".to_owned(),
		JsonValue::Bool(true) => "True".to_owned(),
		JsonValue::Bool(false) => "False".to_owned(),
		JsonValue::Int(digits) => digits.clone(),
		JsonValue::Float(v) => fontoxide_misc::py::float_repr(*v),
		JsonValue::Str(s) => fontoxide_misc::py::str_repr(s),
		JsonValue::Array(items) => {
			let items: Vec<String> = items.iter().map(py_repr).collect();
			format!("[{}]", items.join(", "))
		}
		JsonValue::Object(items) => {
			let items: Vec<String> = items
				.iter()
				.map(|(k, v)| format!("{}: {}", fontoxide_misc::py::str_repr(k), py_repr(v)))
				.collect();
			format!("{{{}}}", items.join(", "))
		}
	}
}

/// `str(value)` (and `f"{value}"`) of a JSON value.
fn py_str(value: &JsonValue) -> String {
	match value {
		JsonValue::Str(s) => s.clone(),
		_ => py_repr(value),
	}
}

/// `bool(value)` of a JSON value.
fn py_truthy(value: &JsonValue) -> bool {
	match value {
		JsonValue::Null => false,
		JsonValue::Bool(b) => *b,
		JsonValue::Int(digits) => digits != "0",
		JsonValue::Float(v) => *v != 0.0,
		JsonValue::Str(s) => !s.is_empty(),
		JsonValue::Array(items) => !items.is_empty(),
		JsonValue::Object(items) => !items.is_empty(),
	}
}

/// `key in container` for a string key.
fn py_contains(container: &JsonValue, key: &str) -> Result<bool> {
	match container {
		JsonValue::Object(items) => Ok(items.iter().any(|(k, _)| k == key)),
		JsonValue::Array(items) => Ok(items
			.iter()
			.any(|v| matches!(v, JsonValue::Str(s) if s == key))),
		JsonValue::Str(s) => Ok(s.contains(key)),
		other => Err(Error::ttlib(format!(
			"TypeError: argument of type '{}' is not iterable",
			py_type_name(other)
		))),
	}
}

/// `container[key]` for a string key.
fn py_getitem<'a>(container: &'a JsonValue, key: &str) -> Result<&'a JsonValue> {
	match container {
		JsonValue::Object(_) => container.get(key).ok_or_else(|| {
			Error::ttlib(format!("KeyError: {}", fontoxide_misc::py::str_repr(key)))
		}),
		JsonValue::Array(_) => Err(Error::ttlib(
			"TypeError: list indices must be integers or slices, not str",
		)),
		JsonValue::Str(_) => Err(Error::ttlib(
			"TypeError: string indices must be integers, not 'str'",
		)),
		other => Err(Error::ttlib(format!(
			"TypeError: '{}' object is not subscriptable",
			py_type_name(other)
		))),
	}
}

/// The items of an iterable JSON value (`list(value)`), `None` if it is not
/// iterable.
fn py_iter(value: &JsonValue) -> Option<Vec<JsonValue>> {
	match value {
		JsonValue::Array(items) => Some(items.clone()),
		JsonValue::Object(items) => Some(
			items
				.iter()
				.map(|(k, _)| JsonValue::Str(k.clone()))
				.collect(),
		),
		JsonValue::Str(s) => Some(s.chars().map(|c| JsonValue::Str(c.to_string())).collect()),
		_ => None,
	}
}

/// The table that a `LookupList` belongs to (`LookupList.table`): `GSUB` or
/// `GPOS`, after the class of the first subtable that tells.
fn lookup_list_table(list: &LookupList) -> Result<&'static str> {
	for lookup in &list.lookup {
		let Some(lookup) = lookup else {
			return Err(Error::ttlib(
				"AttributeError: 'NoneType' object has no attribute 'SubTable'",
			));
		};
		for st in lookup.sub_table.iter().flatten() {
			let name = st.class_name();
			if name.ends_with("Subst") {
				return Ok("GSUB");
			}
			if name.ends_with("Pos") {
				return Ok("GPOS");
			}
		}
	}
	Err(Error::ttlib("ValueError"))
}

/// The lookup debugging information of a `LookupList` (the first part of
/// `LookupList.toXML2`): `font["Debg"].data[LOOKUP_DEBUG_INFO_KEY][self.table]`,
/// or `None` if the font has no `Debg` table or no feaLib data in it. As in
/// fontTools, a `Debg` table without data for the `LookupList`'s table is an
/// error (`KeyError`).
pub fn lookup_list_debug_data(list: &LookupList, font: &mut Font) -> Result<Option<JsonValue>> {
	if !font.contains(Debg::TAG) {
		return Ok(None);
	}
	let Some(debg) = font.get::<Debg>()? else {
		return Ok(None);
	};
	if !py_contains(&debg.data, LOOKUP_DEBUG_INFO_KEY)? {
		return Ok(None);
	}
	let info = py_getitem(&debg.data, LOOKUP_DEBUG_INFO_KEY)?.clone();
	let table = lookup_list_table(list)?;
	Ok(Some(py_getitem(&info, table)?.clone()))
}

/// Writes the comment with the debugging information of a lookup, if there is
/// any (the loop of `LookupList.toXML2`), e.g.
/// `<!-- SomeLookup: file.fea:4:5 in tst2 (DFLT/dflt) -->`.
pub fn write_lookup_debug_comment(
	xw: &mut XmlWriter,
	debug: Option<&JsonValue>,
	index: usize,
) -> Result<()> {
	let Some(debug) = debug else {
		return Ok(());
	};
	let key = index.to_string();
	if !py_contains(debug, &key)? {
		return Ok(());
	}
	// LookupDebugInfo(*debugData[str(lookupIndex)])
	let entry = py_getitem(debug, &key)?;
	let fields = py_iter(entry).ok_or_else(|| {
		Error::ttlib(format!(
			"TypeError: LookupDebugInfo() argument after * must be an iterable, not {}",
			py_type_name(entry)
		))
	})?;
	let [location, name, feature] = <[JsonValue; 3]>::try_from(fields).map_err(|fields| {
		Error::ttlib(format!(
			"TypeError: LookupDebugInfo() takes 3 positional arguments but {} were given",
			fields.len()
		))
	})?;
	let mut tag = py_str(&location);
	if py_truthy(&name) {
		tag = format!("{}: {tag}", py_str(&name));
	}
	if py_truthy(&feature) {
		let items = py_iter(&feature).ok_or_else(|| {
			Error::ttlib(format!(
				"TypeError: cannot unpack non-iterable {} object",
				py_type_name(&feature)
			))
		})?;
		let [script, language, feature] = <[JsonValue; 3]>::try_from(items).map_err(|items| {
			Error::ttlib(format!(
				"ValueError: wrong number of values to unpack (expected 3, got {})",
				items.len()
			))
		})?;
		tag = format!(
			"{tag} in {} ({}/{})",
			py_str(&feature),
			py_str(&script),
			py_str(&language)
		);
	}
	xw.comment(&tag);
	xw.newline();
	Ok(())
}
