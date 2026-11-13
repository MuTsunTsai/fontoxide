//! `cmap`: Character to Glyph Index Mapping table
//! (`fontTools.ttLib.tables._c_m_a_p`).
//!
//! fontTools maps character codes to glyph *names*; fontoxide maps them to glyph
//! IDs, and converts to and from names only for TTX. Compiling goes through the
//! names all the same (glyph ID → name → glyph ID), so that fonts whose glyph order
//! has duplicate names compile exactly as in fontTools.
//!
//! fontTools decompiles subtables lazily; fontoxide always decompiles them eagerly,
//! which is what fontTools does with `TTFont(lazy=False)`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use fontoxide_misc::Tag;
use fontoxide_misc::text_tools::Literal;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{WriteBytes, fit, get_search_range};
use crate::glyph_order::GlyphOrder;
use crate::sstruct::eval;
use crate::table::{Table, TableType, TableXml, read_hex};
use crate::{Error, Font, Result};

/// Character code to glyph ID (`subtable.cmap`, which fontTools keys to glyph
/// names).
pub type CharMap = BTreeMap<u32, u32>;

/// Unicode Variation Sequences of a format 14 or 15 subtable (`uvsDict`): variation
/// selector to `(base character, glyph ID)` pairs, where `None` means the default
/// glyph of the base character.
pub type UvsDict = BTreeMap<u32, Vec<(u32, Option<u32>)>>;

/// The (platform ID, encoding ID) pairs `get_best_cmap` searches by default, in
/// order of preference.
pub const DEFAULT_CMAP_PREFERENCES: [(u16, u16); 8] = [
	(3, 10),
	(0, 6),
	(0, 4),
	(3, 1),
	(0, 3),
	(0, 2),
	(0, 1),
	(0, 0),
];

/// The mapping of one Unicode subtable, as used to derive glyph names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnicodeMapping {
	/// Code point to glyph ID.
	pub cmap: BTreeMap<u32, u32>,
}

/// The mappings of the Unicode subtables (`isUnicode()`) of a `cmap` or `DMAP`
/// table, in subtable order. (`DMAP` has the same structure as `cmap`.)
pub fn unicode_mappings(_tag: Tag, data: &[u8]) -> Result<Vec<UnicodeMapping>> {
	let table = Cmap::parse(data)?;
	Ok(table
		.tables
		.iter()
		.filter(|t| t.is_unicode())
		.map(|t| UnicodeMapping {
			cmap: (*t.cmap).clone(),
		})
		.collect())
}

/// The lowest and highest code points of the font's Unicode `cmap` subtables,
/// loading `cmap` as `ttFont["cmap"]` does (used by
/// `OS/2.updateFirstAndLastCharIndex`). `None` without a `cmap` or code points.
pub fn unicode_code_range(font: &mut Font) -> Result<Option<(u32, u32)>> {
	let Some(cmap) = font.get::<Cmap>()? else {
		return Ok(None);
	};
	let mut range: Option<(u32, u32)> = None;
	for t in cmap.tables.iter().filter(|t| t.is_unicode()) {
		if let (Some((&lo, _)), Some((&hi, _))) =
			(t.cmap.first_key_value(), t.cmap.last_key_value())
		{
			range = Some(match range {
				Some((a, b)) => (a.min(lo), b.max(hi)),
				None => (lo, hi),
			});
		}
	}
	Ok(range)
}

/// Character to Glyph Index Mapping table (`table__c_m_a_p`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/cmap>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cmap {
	/// The table version (`tableVersion`), 0.
	pub table_version: u16,
	/// The subtables (`tables`).
	pub tables: Vec<CmapSubtable>,
}

/// The subtable class, which decides how a subtable is read and written
/// (`cmap_format_0`, `cmap_format_2`, …, `cmap_format_unknown`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubtableKind {
	/// Byte encoding table.
	Format0,
	/// High-byte mapping through table.
	Format2,
	/// Segment mapping to delta values.
	Format4,
	/// Trimmed table mapping.
	Format6,
	/// Segmented coverage.
	Format12,
	/// Many-to-one range mappings.
	Format13,
	/// Unicode Variation Sequences.
	Format14,
	/// Unicode Variation Sequences with 24-bit glyph IDs.
	Format15,
	/// Any other format (including 8 and 10), kept as raw data.
	Unknown,
}

impl SubtableKind {
	/// The class for a format (`getSubtableClass`).
	pub fn from_format(format: u16) -> Self {
		match format {
			0 => SubtableKind::Format0,
			2 => SubtableKind::Format2,
			4 => SubtableKind::Format4,
			6 => SubtableKind::Format6,
			12 => SubtableKind::Format12,
			13 => SubtableKind::Format13,
			14 => SubtableKind::Format14,
			15 => SubtableKind::Format15,
			_ => SubtableKind::Unknown,
		}
	}

	/// The size of the fixed header (`struct.calcsize(headerFormat)`).
	fn header_size(self) -> usize {
		match self {
			SubtableKind::Format12 | SubtableKind::Format13 => 16,
			SubtableKind::Format14 | SubtableKind::Format15 => 10,
			SubtableKind::Unknown => 0,
			_ => 6,
		}
	}

	/// Whether glyph names of the form `gidNNN` are accepted (formats 2, 4, 12 and
	/// 13 allow such "virtual" glyph IDs).
	fn allows_virtual_gids(self) -> bool {
		matches!(
			self,
			SubtableKind::Format2
				| SubtableKind::Format4
				| SubtableKind::Format12
				| SubtableKind::Format13
		)
	}
}

/// A `cmap` subtable (`CmapSubtable` and its subclasses).
#[derive(Debug, Clone, PartialEq)]
pub struct CmapSubtable {
	/// The subtable class.
	pub kind: SubtableKind,
	/// The format number (`format`).
	pub format: u16,
	/// The platform ID (`platformID`).
	pub platform_id: u16,
	/// The encoding ID (`platEncID`).
	pub plat_enc_id: u16,
	/// The language (`language`); `None` where fontTools has none (a subtable not
	/// read from anywhere yet, or an unknown format read from TTX).
	pub language: Option<u32>,
	/// The mapping (`cmap`). Subtables at the same offset in a decompiled table
	/// share it, as in fontTools, and are compiled only once; use
	/// [`Arc::make_mut`] to modify it.
	pub cmap: Arc<CharMap>,
	/// The reserved field of formats 12 and 13 (`reserved`).
	pub reserved: u16,
	/// The subtable length, as read (`length`; written to TTX for formats 12 and
	/// 13).
	pub length: u32,
	/// The number of groups, as read (`nGroups`; formats 12 and 13).
	pub n_groups: u32,
	/// The number of variation selector records, as read (`numVarSelectorRecords`;
	/// formats 14 and 15).
	pub num_var_selector_records: u32,
	/// The variation sequences of formats 14 and 15 (`uvsDict`). `None` if not set,
	/// as for a subtable sharing its data with an earlier one.
	pub uvs_dict: Option<UvsDict>,
	/// The raw subtable of an unknown format, header included (`data`).
	pub data: Option<Vec<u8>>,
}

impl CmapSubtable {
	/// A new, empty subtable of the given format (`CmapSubtable.newSubtable`).
	pub fn new(format: u16) -> Self {
		CmapSubtable {
			kind: SubtableKind::from_format(format),
			format,
			platform_id: 0,
			plat_enc_id: 0,
			language: None,
			cmap: Arc::new(CharMap::new()),
			reserved: 0,
			length: 0,
			n_groups: 0,
			num_var_selector_records: 0,
			uvs_dict: None,
			data: None,
		}
	}

	/// Whether the characters are Unicode code points (`isUnicode`).
	pub fn is_unicode(&self) -> bool {
		self.platform_id == 0 || (self.platform_id == 3 && matches!(self.plat_enc_id, 0 | 1 | 10))
	}

	/// The codec of the subtable's encoding, `None` if unknown (`getEncoding()`).
	pub fn encoding(&self) -> Option<&'static str> {
		// Only a few small language IDs select a specific codec.
		let language = self.language.unwrap_or(0).min(0xFFFF) as u16;
		fontoxide_misc::encoding_tools::get_encoding(self.platform_id, self.plat_enc_id, language)
	}

	/// Whether the subtable is for the Symbol encoding (3, 0) (`isSymbol`).
	pub fn is_symbol(&self) -> bool {
		self.platform_id == 3 && self.plat_enc_id == 0
	}

	/// The mapping, for modification ([`Arc::make_mut`] on [`CmapSubtable::cmap`]).
	pub fn cmap_mut(&mut self) -> &mut CharMap {
		Arc::make_mut(&mut self.cmap)
	}

	/// The TTX element name (the class name).
	fn element_name(&self) -> String {
		match self.kind {
			SubtableKind::Format0 => "cmap_format_0".into(),
			SubtableKind::Format2 => "cmap_format_2".into(),
			SubtableKind::Format4 => "cmap_format_4".into(),
			SubtableKind::Format6 => "cmap_format_6".into(),
			SubtableKind::Format12 => "cmap_format_12".into(),
			SubtableKind::Format13 => "cmap_format_13".into(),
			SubtableKind::Format14 => "cmap_format_14".into(),
			SubtableKind::Format15 => "cmap_format_15".into(),
			SubtableKind::Unknown => format!("cmap_format_{}", self.format),
		}
	}

	/// Decompiles a subtable from its data, header included (`decompile(data,
	/// ttFont)`).
	pub fn decompile(&mut self, data: &[u8]) -> Result<()> {
		self.decompile_header(data)?;
		self.decompile_body(data)
	}

	/// Reads the header (`decompileHeader`).
	fn decompile_header(&mut self, data: &[u8]) -> Result<()> {
		let size = self.kind.header_size();
		if data.len() < size {
			return Err(unpack_error(size));
		}
		match self.kind {
			SubtableKind::Format12 | SubtableKind::Format13 => {
				let format = be16(data, 0);
				let reserved = be16(data, 2);
				let length = be32(data, 4);
				let language = be32(data, 8);
				let n_groups = be32(data, 12);
				if data.len() as u64 != length as u64 {
					return Err(Error::ttlib(format!(
						"cmap subtable format {} is truncated: length {length}, got {} bytes",
						self.format,
						data.len()
					)));
				}
				let expected = 16 + n_groups as u64 * 12;
				if length as u64 != expected {
					return Err(Error::ttlib(format!(
						"cmap subtable format {} has an inconsistent group count: length {length}, but {n_groups} groups need {expected} bytes",
						self.format
					)));
				}
				self.format = format;
				self.reserved = reserved;
				self.length = length;
				self.language = Some(language);
				self.n_groups = n_groups;
			}
			SubtableKind::Format14 | SubtableKind::Format15 => {
				self.length = be32(data, 2);
				self.num_var_selector_records = be32(data, 6);
				self.language = Some(0xFF);
			}
			SubtableKind::Unknown => {
				self.language = Some(0);
				self.data = Some(data.to_vec());
			}
			_ => {
				let format = be16(data, 0);
				let length = be16(data, 2);
				let language = be16(data, 4);
				if data.len() != length as usize {
					return Err(Error::Assertion(format!(
						"corrupt cmap table format {format} (data length: {}, header length: {length})",
						data.len()
					)));
				}
				self.format = format;
				self.length = length as u32;
				self.language = Some(language as u32);
			}
		}
		Ok(())
	}

	/// Reads the rest of the subtable (`decompile`).
	fn decompile_body(&mut self, data: &[u8]) -> Result<()> {
		let body = &data[self.kind.header_size().min(data.len())..];
		let pairs = match self.kind {
			SubtableKind::Format0 => {
				if self.length != 262 {
					return Err(Error::Assertion(
						"Format 0 cmap subtable not 262 bytes".into(),
					));
				}
				body.iter()
					.enumerate()
					.map(|(i, &g)| (i as u32, g as u32))
					.collect()
			}
			SubtableKind::Format2 => decompile_format_2(body)?,
			SubtableKind::Format4 => decompile_format_4(body)?,
			SubtableKind::Format6 => decompile_format_6(body)?,
			SubtableKind::Format12 | SubtableKind::Format13 => {
				decompile_format_12_or_13(body, self.kind, self.format, self.n_groups)?
			}
			SubtableKind::Format14 | SubtableKind::Format15 => {
				self.uvs_dict = Some(decompile_format_14(
					body,
					self.kind,
					self.num_var_selector_records,
				)?);
				Vec::new()
			}
			SubtableKind::Unknown => Vec::new(),
		};
		self.cmap = Arc::new(make_map(pairs));
		Ok(())
	}

	/// Compiles the subtable (`compile`).
	pub fn compile(&self, font: &mut Font) -> Result<Vec<u8>> {
		match self.kind {
			SubtableKind::Unknown => match &self.data {
				Some(data) if !data.is_empty() => Ok(data.clone()),
				_ => Err(Error::value(
					"can only concatenate bytes (not \"NoneType\") to bytes: unknown cmap subtable has no data",
				)),
			},
			SubtableKind::Format14 | SubtableKind::Format15 => self.compile_format_14(font),
			_ => {
				let order = font.glyph_order()?;
				let items: Vec<(u32, u32)> = self
					.cmap
					.iter()
					.map(|(&code, &gid)| (code, canonical_gid(&order, gid)))
					.collect();
				let language = self.language;
				match self.kind {
					SubtableKind::Format0 => compile_format_0(&items, language),
					SubtableKind::Format2 => compile_format_2(&items, language),
					SubtableKind::Format4 => compile_format_4(&items, self.format, language),
					SubtableKind::Format6 => compile_format_6(&items, language),
					_ => self.compile_format_12_or_13(&items),
				}
			}
		}
	}

	fn compile_format_12_or_13(&self, items: &[(u32, u32)]) -> Result<Vec<u8>> {
		let language = language_u32(self.language)?;
		let mut data = Vec::new();
		data.put_u16(self.format);
		data.put_u16(self.reserved);
		if items.is_empty() {
			data.put_u32(16);
			data.put_u32(language);
			data.put_u32(0);
			return Ok(data);
		}
		let step: i64 = if self.kind == SubtableKind::Format12 {
			1
		} else {
			0
		};
		let in_same_run = |gid: i64, last_gid: i64, code: i64, last_code: i64| {
			if step == 1 {
				gid == 1 + last_gid && code == 1 + last_code
			} else {
				gid == last_gid && code == 1 + last_code
			}
		};
		let mut groups = Vec::new();
		let (mut start_code, mut start_gid) = (items[0].0 as i64, items[0].1 as i64);
		let mut last_gid = start_gid - step;
		let mut last_code = start_code - 1;
		for &(code, gid) in items {
			let (code, gid) = (code as i64, gid as i64);
			if !in_same_run(gid, last_gid, code, last_code) {
				groups.push((start_code, last_code, start_gid));
				start_code = code;
				start_gid = gid;
			}
			last_gid = gid;
			last_code = code;
		}
		groups.push((start_code, last_code, start_gid));
		let length = 16 + groups.len() * 12;
		data.put_u32(fit(length, "length")?);
		data.put_u32(language);
		data.put_u32(fit(groups.len(), "nGroups")?);
		for (start, end, gid) in groups {
			data.put_u32(fit(start, "startCharCode")?);
			data.put_u32(fit(end, "endCharCode")?);
			data.put_u32(fit(gid, "startGlyphID")?);
		}
		Ok(data)
	}

	fn compile_format_14(&self, font: &mut Font) -> Result<Vec<u8>> {
		let uvs_dict = self
			.uvs_dict
			.as_ref()
			.ok_or_else(|| no_attribute(self, "uvsDict"))?;
		let order = font.glyph_order()?;
		let record_size = if self.kind == SubtableKind::Format15 {
			6
		} else {
			5
		};
		let num_records = uvs_dict.len();
		let mut offset = 10 + num_records * 11;
		let mut data = Vec::new();
		let mut var_selector_records = Vec::new();
		for (&uvs, entries) in uvs_dict {
			let mut def_list: Vec<u32> = entries
				.iter()
				.filter(|e| e.1.is_none())
				.map(|e| e.0)
				.collect();
			let def_offset = if def_list.is_empty() {
				0
			} else {
				def_list.sort_unstable();
				let mut last_uv = def_list[0] as i64;
				let mut cnt: i64 = -1;
				let mut def_recs = Vec::new();
				for &entry in &def_list {
					cnt += 1;
					if last_uv + cnt != entry as i64 || cnt > 255 {
						def_recs.push((last_uv, cnt - 1));
						last_uv = entry as i64;
						cnt = 0;
					}
				}
				def_recs.push((last_uv, cnt));
				data.put_u32(fit(def_recs.len(), "numUnicodeValueRanges")?);
				for &(uv, count) in &def_recs {
					data.put_u24(cvt_from_uvs(uv)?);
					data.put_u8(fit(count, "additionalCount")?);
				}
				let o = offset;
				offset += 4 + def_recs.len() * 4;
				o
			};
			let mut ndef_list: Vec<(u32, u32, String)> = entries
				.iter()
				.filter_map(|&(uv, gid)| gid.map(|g| (uv, g, order.name(g).into_owned())))
				.collect();
			let non_def_offset = if ndef_list.is_empty() {
				0
			} else {
				ndef_list.sort_by(|a, b| (a.0, &a.2).cmp(&(b.0, &b.2)));
				let o = offset;
				data.put_u32(fit(ndef_list.len(), "numUVSMappings")?);
				offset += 4 + ndef_list.len() * record_size;
				for (uv, gid, _) in &ndef_list {
					let gid = canonical_gid(&order, *gid);
					data.put_u24(cvt_from_uvs(*uv as i64)?);
					if self.kind == SubtableKind::Format15 {
						data.put_u24(cvt_from_uvs(gid as i64)?);
					} else {
						data.put_u16(fit(gid, "glyphID")?);
					}
				}
				o
			};
			var_selector_records.put_u24(cvt_from_uvs(uvs as i64)?);
			var_selector_records.put_u32(fit(def_offset, "defaultUVSOffset")?);
			var_selector_records.put_u32(fit(non_def_offset, "nonDefaultUVSOffset")?);
		}
		let length = 10 + var_selector_records.len() + data.len();
		let mut out = Vec::with_capacity(length);
		out.put_u16(self.format);
		out.put_u32(fit(length, "length")?);
		out.put_u32(fit(num_records, "numVarSelectorRecords")?);
		out.extend(var_selector_records);
		out.extend(data);
		Ok(out)
	}

	/// Writes the subtable's TTX element (`toXML`).
	pub fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let name = self.element_name();
		match self.kind {
			SubtableKind::Unknown => {
				let data = self.data.as_ref().ok_or_else(|| {
					Error::value(
						"object of type 'NoneType' has no len(): unknown cmap subtable has no data",
					)
				})?;
				writer.begintag_ordered(
					&name,
					&[
						("platformID", &self.platform_id),
						("platEncID", &self.plat_enc_id),
					],
				);
				writer.newline();
				writer.dumphex(data);
			}
			SubtableKind::Format14 | SubtableKind::Format15 => {
				let uvs_dict = self
					.uvs_dict
					.as_ref()
					.ok_or_else(|| no_attribute(self, "uvsDict"))?;
				let order = font.glyph_order()?;
				writer.begintag_ordered(
					&name,
					&[
						("platformID", &self.platform_id),
						("platEncID", &self.plat_enc_id),
					],
				);
				writer.newline();
				for (&uvs, entries) in uvs_dict {
					let mut entries: Vec<(u32, Option<String>)> = entries
						.iter()
						.map(|&(uv, gid)| (uv, gid.map(|g| order.name(g).into_owned())))
						.collect();
					entries.sort_by(|a, b| {
						(a.1.is_some(), a.0, &a.1).cmp(&(b.1.is_some(), b.0, &b.1))
					});
					for (uv, glyph_name) in &entries {
						let uv = format!("{uv:#x}");
						let uvs = format!("{uvs:#x}");
						match glyph_name {
							Some(n) => writer.simpletag_ordered(
								"map",
								&[("uv", &uv), ("uvs", &uvs), ("name", n)],
							),
							None => writer.simpletag_ordered("map", &[("uv", &uv), ("uvs", &uvs)]),
						}
						writer.newline();
					}
				}
			}
			SubtableKind::Format12 | SubtableKind::Format13 => {
				let language = LanguageAttr(self.language);
				writer.begintag_ordered(
					&name,
					&[
						("platformID", &self.platform_id),
						("platEncID", &self.plat_enc_id),
						("format", &self.format),
						("reserved", &self.reserved),
						("length", &self.length),
						("language", &language),
						("nGroups", &self.n_groups),
					],
				);
				writer.newline();
				self.write_codes(writer, font)?;
			}
			_ => {
				let language = LanguageAttr(self.language);
				writer.begintag_ordered(
					&name,
					&[
						("platformID", &self.platform_id),
						("platEncID", &self.plat_enc_id),
						("language", &language),
					],
				);
				writer.newline();
				self.write_codes(writer, font)?;
			}
		}
		writer.endtag(&name);
		writer.newline();
		Ok(())
	}

	/// `_writeCodes`.
	fn write_codes(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let order = font.glyph_order()?;
		let is_unicode = self.is_unicode();
		for (&code, &gid) in self.cmap.iter() {
			let code_str = format!("{code:#x}");
			let name = order.name(gid);
			writer.simpletag("map", &[("code", &code_str), ("name", &name.as_ref())]);
			if is_unicode {
				writer.comment(&fontoxide_unicodedata::unicode_name(code));
			}
			writer.newline();
		}
		Ok(())
	}

	/// Reads the subtable's TTX element (`fromXML`); the platform and encoding IDs
	/// are read by the caller.
	pub fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		match self.kind {
			SubtableKind::Unknown => {
				self.data = Some(read_hex(element)?);
				self.cmap = Arc::new(CharMap::new());
				return Ok(());
			}
			SubtableKind::Format14 | SubtableKind::Format15 => {
				return self.read_xml_format_14(element, font);
			}
			SubtableKind::Format12 | SubtableKind::Format13 => {
				self.format = fit(int_attr(element, "format")?, "format")?;
				self.reserved = fit(int_attr(element, "reserved")?, "reserved")?;
				self.length = fit(int_attr(element, "length")?, "length")?;
				self.language = language_attr(element)?;
				self.n_groups = fit(int_attr(element, "nGroups")?, "nGroups")?;
			}
			_ => self.language = language_attr(element)?,
		}
		let order = font.glyph_order()?;
		let cmap = Arc::make_mut(&mut self.cmap);
		for child in element.elements() {
			if child.name != "map" {
				if self.kind == SubtableKind::Format4 {
					return Err(Error::Assertion(
						"Unrecognized keyword in cmap subtable".into(),
					));
				}
				continue;
			}
			let code: u32 = fit(int_attr(child, "code")?, "code")?;
			let name = str_attr(child, "name")?;
			cmap.insert(
				code,
				glyph_id_for(&order, name, self.kind.allows_virtual_gids())?,
			);
		}
		Ok(())
	}

	fn read_xml_format_14(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		self.language = Some(0xFF);
		let order = font.glyph_order()?;
		// For backwards compatibility, "None" means the default mapping, unless the
		// font actually has a glyph named "None".
		let mut has_glyph_named_none = None;
		let uvs_dict = self.uvs_dict.get_or_insert_with(UvsDict::new);
		for child in element.elements() {
			if child.name != "map" {
				continue;
			}
			let uvs: u32 = fit(int_attr(child, "uvs")?, "uvs")?;
			let uv: u32 = fit(int_attr(child, "uv")?, "uv")?;
			let mut name = child.attr("name");
			if name == Some("None") {
				let has = *has_glyph_named_none
					.get_or_insert_with(|| order.names().iter().any(|n| n == "None"));
				if !has {
					name = None;
				}
			}
			let gid = match name {
				Some(n) => Some(glyph_id_for(&order, n, false)?),
				None => None,
			};
			uvs_dict.entry(uvs).or_default().push((uv, gid));
		}
		Ok(())
	}

	/// The sort key (`__lt__`).
	fn sort_key(&self) -> (u16, u16, Option<u32>) {
		(self.platform_id, self.plat_enc_id, self.language)
	}
}

/// `language` as Python formats it: `None` when unset.
struct LanguageAttr(Option<u32>);

impl fontoxide_misc::xml_writer::AttrValue for LanguageAttr {
	fn attr_string(&self) -> std::borrow::Cow<'_, str> {
		match self.0 {
			Some(l) => l.to_string().into(),
			None => "None".into(),
		}
	}
}

impl Cmap {
	/// Parses a `cmap` (or `DMAP`) table; this needs no font, since glyphs are
	/// stored by ID.
	pub fn parse(data: &[u8]) -> Result<Self> {
		if data.len() < 4 {
			return Err(Error::ttlib("cmap table is too short"));
		}
		let table_version = be16(data, 0);
		let num_sub_tables = be16(data, 2) as usize;
		let mut tables: Vec<CmapSubtable> = Vec::new();
		let mut seen_offsets: HashMap<usize, usize> = HashMap::new();
		let len = data.len();
		for i in 0..num_sub_tables {
			let start = 4 + i * 8;
			if start + 8 > len {
				return Err(Error::ttlib("cmap subtable directory is truncated"));
			}
			let platform_id = be16(data, start);
			let plat_enc_id = be16(data, start + 2);
			let offset = be32(data, start + 4) as usize;
			if offset as u64 + 4 > len as u64 {
				return Err(Error::ttlib(format!(
					"cmap subtable offset {offset} is out of bounds (data length {len})"
				)));
			}
			let format = be16(data, offset);
			let mut length = be16(data, offset + 2) as u64;
			if matches!(format, 8 | 10 | 12 | 13) {
				if offset + 8 > len {
					return Err(Error::ttlib("cmap subtable header is truncated"));
				}
				length = be32(data, offset + 4) as u64;
			} else if matches!(format, 14 | 15) {
				if offset + 6 > len {
					return Err(Error::ttlib("cmap subtable header is truncated"));
				}
				length = be32(data, offset + 2) as u64;
			}
			if length == 0 {
				log::error!(
					"cmap subtable is reported as having zero length: platformID {platform_id}, platEncID {plat_enc_id}, format {format} offset {offset}. Skipping table."
				);
				continue;
			}
			let mut table = CmapSubtable::new(format);
			table.platform_id = platform_id;
			table.plat_enc_id = plat_enc_id;
			let min_header_size = table.kind.header_size() as u64;
			if length < min_header_size {
				return Err(Error::ttlib(format!(
					"cmap subtable header is truncated: length {length} < {min_header_size}"
				)));
			}
			if offset as u64 + length > len as u64 {
				return Err(Error::ttlib(format!(
					"cmap subtable is truncated: needs {length} bytes, only {} available",
					len - offset
				)));
			}
			let sub_data = &data[offset..offset + length as usize];
			table.decompile_header(sub_data)?;
			if let Some(&first) = seen_offsets.get(&offset) {
				// Mark as decompiled, sharing the mapping. As in fontTools, the index
				// is that of the directory entry, which differs from the index in
				// `tables` after a skipped zero-length subtable.
				table.data = None;
				table.cmap = tables
					.get(first)
					.ok_or_else(|| Error::value("list index out of range"))?
					.cmap
					.clone();
			} else {
				seen_offsets.insert(offset, i);
				table.decompile_body(sub_data)?;
			}
			tables.push(table);
		}
		Ok(Cmap {
			table_version,
			tables,
		})
	}

	/// The first subtable with the given platform and encoding IDs (`getcmap`).
	pub fn getcmap(&self, platform_id: u16, plat_enc_id: u16) -> Option<&CmapSubtable> {
		self.tables
			.iter()
			.find(|t| t.platform_id == platform_id && t.plat_enc_id == plat_enc_id)
	}

	/// The mapping of the first subtable found in order of preference
	/// (`getBestCmap`); [`DEFAULT_CMAP_PREFERENCES`] is the default order.
	pub fn get_best_cmap(&self, preferences: &[(u16, u16)]) -> Option<&CharMap> {
		preferences
			.iter()
			.find_map(|&(p, e)| self.getcmap(p, e))
			.map(|t| &*t.cmap)
	}

	/// Glyph ID to the code points mapped to it in the Unicode subtables
	/// (`buildReversed`).
	pub fn build_reversed(&self) -> BTreeMap<u32, BTreeSet<u32>> {
		let mut result: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
		for t in self.tables.iter().filter(|t| t.is_unicode()) {
			for (&code, &gid) in t.cmap.iter() {
				result.entry(gid).or_default().insert(code);
			}
		}
		result
	}

	/// Glyph ID to the lowest code point mapped to it in the Unicode subtables
	/// (`buildReversedMin`).
	pub fn build_reversed_min(&self) -> BTreeMap<u32, u32> {
		let mut result: BTreeMap<u32, u32> = BTreeMap::new();
		for t in self.tables.iter().filter(|t| t.is_unicode()) {
			for (&code, &gid) in t.cmap.iter() {
				result
					.entry(gid)
					.and_modify(|c| *c = (*c).min(code))
					.or_insert(code);
			}
		}
		result
	}

	/// Sorts the subtables by (platform ID, encoding ID, language), as the
	/// specification requires (`tables.sort()`).
	pub fn sort_tables(&mut self) -> Result<()> {
		// Python fails to compare a missing language (None) with a number.
		let mut languages: HashMap<(u16, u16), (bool, bool)> = HashMap::new();
		for t in &self.tables {
			let e = languages.entry((t.platform_id, t.plat_enc_id)).or_default();
			if t.language.is_some() {
				e.0 = true;
			} else {
				e.1 = true;
			}
		}
		if languages.values().any(|&(some, none)| some && none) {
			return Err(Error::value(
				"'<' not supported between instances of 'int' and 'NoneType'",
			));
		}
		self.tables.sort_by_key(|t| t.sort_key());
		Ok(())
	}
}

impl TableType for Cmap {
	const TAG: Tag = Tag::new(b"cmap");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		Cmap::parse(data)
	}
}

impl Table for Cmap {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		for t in &mut self.tables {
			let mut cmap = (*t.cmap).clone();
			for g in cmap.values_mut() {
				*g = f(*g)?;
			}
			t.cmap = std::sync::Arc::new(cmap);
			if let Some(uvs) = &mut t.uvs_dict {
				for records in uvs.values_mut() {
					for (_, g) in records.iter_mut() {
						if let Some(g) = g {
							*g = f(*g)?;
						}
					}
				}
			}
		}
		Ok(())
	}

	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		self.sort_tables()?;
		let mut seen = HashSet::new();
		let mut duplicates = BTreeSet::new();
		for t in &self.tables {
			if !seen.insert(t.sort_key()) {
				duplicates.insert(t.sort_key());
			}
		}
		if !duplicates.is_empty() {
			let items: Vec<String> = duplicates
				.iter()
				.map(|(p, e, l)| match l {
					Some(l) => format!("({p}, {e}, {l})"),
					None => format!("({p}, {e}, None)"),
				})
				.collect();
			return Err(Error::value(format!(
				"cmap subtables have duplicate (platformID, platEncID, language) entries, which the OpenType spec does not allow: {{{}}}",
				items.join(", ")
			)));
		}
		let num_sub_tables = self.tables.len();
		let total_offset = 4 + 8 * num_sub_tables;
		let mut data = Vec::new();
		data.put_u16(self.table_version);
		data.put_u16(fit(num_sub_tables, "numSubTables")?);
		let mut table_data = Vec::new();
		// Some subtables share their mapping: compile them once.
		let mut seen: HashMap<usize, usize> = HashMap::new();
		// Some subtables are different, but compile to the same data.
		let mut done: HashMap<Vec<u8>, usize> = HashMap::new();
		for table in &self.tables {
			let id = Arc::as_ptr(&table.cmap) as usize;
			let offset = match seen.get(&id) {
				Some(&offset) => offset,
				None => {
					let chunk = table.compile(font)?;
					match done.get(&chunk) {
						Some(&offset) => offset,
						None => {
							let offset = total_offset + table_data.len();
							seen.insert(id, offset);
							table_data.extend_from_slice(&chunk);
							done.insert(chunk, offset);
							offset
						}
					}
				}
			};
			data.put_u16(table.platform_id);
			data.put_u16(table.plat_enc_id);
			data.put_u32(fit(offset, "offset")?);
		}
		data.extend(table_data);
		Ok(data)
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}

impl TableXml for Cmap {
	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		writer.simpletag("tableVersion", &[("version", &self.table_version)]);
		writer.newline();
		for table in &self.tables {
			table.to_xml(writer, font)?;
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		if element.name == "tableVersion" {
			self.table_version = fit(int_attr(element, "version")?, "tableVersion")?;
			return Ok(());
		}
		let Some(format) = element.name.strip_prefix("cmap_format_") else {
			return Ok(());
		};
		let format = eval(format)?
			.as_int()
			.ok_or_else(|| Error::value(format!("invalid cmap subtable format: {format:?}")))?;
		let mut table = CmapSubtable::new(fit(format, "format")?);
		table.platform_id = fit(int_attr(element, "platformID")?, "platformID")?;
		table.plat_enc_id = fit(int_attr(element, "platEncID")?, "platEncID")?;
		table.from_xml(element, font)?;
		self.tables.push(table);
		Ok(())
	}
}

/// Delta map table (`table_D_M_A_P_`): the same structure as `cmap`, whose
/// mappings it takes precedence over.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dmap(pub Cmap);

impl TableType for Dmap {
	const TAG: Tag = Tag::new(b"DMAP");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		Cmap::parse(data).map(Dmap)
	}
}

impl Table for Dmap {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		self.0.remap_glyphs(f)
	}

	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		self.0.compile(font)
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}

impl TableXml for Dmap {
	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		self.0.to_xml(writer, font)
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		self.0.from_xml(element, font)
	}
}

// ---------------------------------------------------------------------------
// Helpers

fn be16(data: &[u8], pos: usize) -> u16 {
	u16::from_be_bytes([data[pos], data[pos + 1]])
}

fn be24(data: &[u8], pos: usize) -> u32 {
	u32::from_be_bytes([0, data[pos], data[pos + 1], data[pos + 2]])
}

fn be32(data: &[u8], pos: usize) -> u32 {
	u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
}

fn unpack_error(size: usize) -> Error {
	Error::Struct(format!("unpack requires a buffer of {size} bytes"))
}

fn index_error() -> Error {
	Error::value("array index out of range")
}

fn no_attribute(table: &CmapSubtable, attr: &str) -> Error {
	Error::value(format!(
		"'{}' object has no attribute '{attr}'",
		table.element_name()
	))
}

/// `data[start:end]` with Python's clamping (non-negative bounds).
fn clamp<T>(data: &[T], start: usize, end: usize) -> &[T] {
	let start = start.min(data.len());
	let end = end.clamp(start, data.len());
	&data[start..end]
}

/// `data[start:end]` with Python's handling of negative and out-of-range bounds.
fn py_slice(data: &[u8], start: i64, end: i64) -> &[u8] {
	let len = data.len() as i64;
	let norm = |i: i64| {
		if i < 0 { (i + len).max(0) } else { i.min(len) }
	};
	let (s, e) = (norm(start), norm(end));
	if s >= e {
		&[]
	} else {
		&data[s as usize..e as usize]
	}
}

/// `array.array("H").frombytes(data)`, big-endian.
fn u16_array(data: &[u8]) -> Result<Vec<u16>> {
	if data.len() % 2 == 1 {
		return Err(Error::value("bytes length not a multiple of item size"));
	}
	Ok(data
		.as_chunks::<2>()
		.0
		.iter()
		.map(|c| u16::from_be_bytes(*c))
		.collect())
}

/// `_make_map`: drops the code points mapped to glyph 0. Later pairs override
/// earlier ones, as in a Python dict.
fn make_map(pairs: Vec<(u32, u32)>) -> CharMap {
	let mut map: CharMap = pairs.into_iter().collect();
	map.retain(|_, gid| *gid != 0);
	map
}

/// The glyph ID fontTools would compile for a glyph ID it read: the ID of the
/// glyph's name (`getGlyphID(getGlyphName(gid))`), which differs only when the glyph
/// order has duplicate names.
fn canonical_gid(order: &GlyphOrder, gid: u32) -> u32 {
	order.id(&order.name(gid)).unwrap_or(gid)
}

/// The glyph ID for a glyph name read from TTX. Formats 2, 4, 12 and 13 also accept
/// `gidNNN` names for glyphs not in the glyph order.
fn glyph_id_for(order: &GlyphOrder, name: &str, allow_virtual: bool) -> Result<u32> {
	if let Some(&gid) = order.reverse_map().get(name) {
		return Ok(gid);
	}
	if allow_virtual && let Some(digits) = name.strip_prefix("gid") {
		let gid = fontoxide_misc::py::parse_int(digits)
			.ok_or_else(|| Error::GlyphNotFound(name.to_owned()))?;
		return u32::try_from(gid)
			.map_err(|_| Error::Struct(format!("cmap cannot represent glyph ID {gid}")));
	}
	order
		.id(name)
		.ok_or_else(|| Error::GlyphNotFound(name.to_owned()))
}

fn str_attr<'a>(element: &'a Element, name: &str) -> Result<&'a str> {
	element
		.attr(name)
		.ok_or_else(|| Error::ttx(format!("missing '{name}' attribute on <{}>", element.name)))
}

fn int_attr(element: &Element, name: &str) -> Result<i128> {
	let value = str_attr(element, name)?;
	eval(value)?
		.as_int()
		.ok_or_else(|| Error::value(format!("invalid '{name}' value: {value:?}")))
}

fn language_attr(element: &Element) -> Result<Option<u32>> {
	let value = str_attr(element, "language")?;
	match eval(value)? {
		Literal::None => Ok(None),
		v => {
			let l = v
				.as_int()
				.ok_or_else(|| Error::value(format!("invalid 'language' value: {value:?}")))?;
			Ok(Some(fit(l, "language")?))
		}
	}
}

/// The language of a 16-bit subtable header.
fn language_u16(language: Option<u32>) -> Result<u16> {
	match language {
		Some(l) => fit(l, "language"),
		None => Err(Error::Struct("required argument is not an integer".into())),
	}
}

/// The language of a 32-bit subtable header.
fn language_u32(language: Option<u32>) -> Result<u32> {
	language.ok_or_else(|| Error::Struct("required argument is not an integer".into()))
}

/// `cvtFromUVS`.
fn cvt_from_uvs(value: i64) -> Result<u32> {
	if !(0..0x1000000).contains(&value) {
		return Err(Error::Assertion("0 <= val < 0x1000000".into()));
	}
	Ok(value as u32)
}

// ---------------------------------------------------------------------------
// Decompiling

struct SubHeader {
	first_code: i64,
	entry_count: i64,
	id_delta: i64,
	id_range_offset: i64,
	glyph_index_array: Vec<i64>,
}

fn decompile_format_2(data: &[u8]) -> Result<Vec<(u32, u32)>> {
	let all_keys = u16_array(clamp(data, 0, 512))?;
	let data = clamp(data, 512, data.len());
	let sub_header_keys: Vec<usize> = all_keys.iter().map(|&k| (k / 8) as usize).collect();
	let max_index = *sub_header_keys
		.iter()
		.max()
		.ok_or_else(|| Error::value("max() arg is an empty sequence"))?;
	let mut sub_headers = Vec::with_capacity(max_index + 1);
	let mut pos = 0usize;
	for _ in 0..=max_index {
		let rec = clamp(data, pos, pos + 8);
		if rec.len() < 8 {
			return Err(unpack_error(8));
		}
		let first_code = be16(rec, 0) as i64;
		let entry_count = be16(rec, 2) as i64;
		let id_delta = i16::from_be_bytes([rec[4], rec[5]]) as i64;
		let id_range_offset = be16(rec, 6) as i64;
		pos += 8;
		let gi_pos = pos + id_range_offset as usize - 2;
		let gi = u16_array(clamp(data, gi_pos, gi_pos + entry_count as usize * 2))?;
		sub_headers.push(SubHeader {
			first_code,
			entry_count,
			id_delta,
			id_range_offset,
			glyph_index_array: gi.into_iter().map(i64::from).collect(),
		});
	}
	let mut pairs = Vec::new();
	for first_byte in 0..256i64 {
		let index = *sub_header_keys
			.get(first_byte as usize)
			.ok_or_else(index_error)?;
		let sh = &sub_headers[index];
		if index == 0 {
			if first_byte < sh.first_code || first_byte >= sh.first_code + sh.entry_count {
				continue; // gi is notdef.
			}
			let gi = *sh
				.glyph_index_array
				.get((first_byte - sh.first_code) as usize)
				.ok_or_else(index_error)?;
			if gi == 0 {
				continue; // gi is notdef.
			}
			pairs.push((
				first_byte as u32,
				(gi + sh.id_delta).rem_euclid(0x10000) as u32,
			));
		} else if sh.entry_count != 0 {
			let char_code_offset = first_byte * 256 + sh.first_code;
			for offset_index in 0..sh.entry_count {
				let gi = *sh
					.glyph_index_array
					.get(offset_index as usize)
					.ok_or_else(index_error)?;
				if gi == 0 {
					continue;
				}
				pairs.push((
					(char_code_offset + offset_index) as u32,
					(gi + sh.id_delta).rem_euclid(0x10000) as u32,
				));
			}
		}
		// If not entryCount, all char codes with this first byte map to .notdef.
	}
	Ok(pairs)
}

fn decompile_format_4(data: &[u8]) -> Result<Vec<(u32, u32)>> {
	if data.len() < 8 {
		return Err(unpack_error(8));
	}
	let seg_count = (be16(data, 0) / 2) as usize;
	let all_codes = u16_array(&data[8..])?;
	let end_code = clamp(&all_codes, 0, seg_count);
	let rest = clamp(&all_codes, seg_count + 1, all_codes.len()); // skip reservedPad
	let start_code = clamp(rest, 0, seg_count);
	let rest = clamp(rest, seg_count, rest.len());
	let id_delta = clamp(rest, 0, seg_count);
	let rest = clamp(rest, seg_count, rest.len());
	let id_range_offset = clamp(rest, 0, seg_count);
	let glyph_index_array = clamp(rest, seg_count, rest.len());
	let len_gi_array = glyph_index_array.len() as i64;

	// Later segments override earlier ones, but a missing glyph does not erase an
	// earlier mapping.
	let mut code_to_gid: CharMap = CharMap::new();
	let num_segments = start_code.len().saturating_sub(1); // don't do 0xffff!
	for (i, &start) in start_code.iter().enumerate().take(num_segments) {
		let start = start as i64;
		let delta = *id_delta.get(i).ok_or_else(index_error)? as i64;
		let range_offset = *id_range_offset.get(i).ok_or_else(index_error)? as i64;
		let partial = range_offset / 2 - start + i as i64 - id_range_offset.len() as i64;
		let end = *end_code.get(i).ok_or_else(index_error)? as i64;
		if range_offset == 0 {
			for code in start..=end {
				let gid = (code + delta) & 0xFFFF;
				if gid != 0 {
					code_to_gid.insert(code as u32, gid as u32);
				}
			}
		} else {
			let mut gids = Vec::new();
			for code in start..=end {
				let index = code + partial;
				// A negative index would wrap around to the end of the array.
				if !(0..len_gi_array).contains(&index) {
					return Err(Error::ttlib(format!(
						"cmap format 4 subtable: glyph index array offset {index} out of range [0, {len_gi_array}) in segment {i}"
					)));
				}
				let g = glyph_index_array[index as usize] as i64;
				let gid = if g != 0 { (g + delta) & 0xFFFF } else { 0 };
				gids.push((code as u32, gid as u32));
			}
			code_to_gid.extend(gids.into_iter().filter(|&(_, g)| g != 0));
		}
	}
	Ok(code_to_gid.into_iter().collect())
}

fn decompile_format_6(data: &[u8]) -> Result<Vec<(u32, u32)>> {
	if data.len() < 4 {
		return Err(unpack_error(4));
	}
	let first_code = be16(data, 0) as u32;
	let entry_count = be16(data, 2) as usize;
	let gids = u16_array(clamp(data, 4, 4 + 2 * entry_count))?;
	Ok(gids
		.into_iter()
		.enumerate()
		.map(|(i, g)| (first_code + i as u32, g as u32))
		.collect())
}

fn decompile_format_12_or_13(
	data: &[u8],
	kind: SubtableKind,
	format: u16,
	n_groups: u32,
) -> Result<Vec<(u32, u32)>> {
	let groups = clamp(data, 0, n_groups as usize * 12);
	if !groups.len().is_multiple_of(4) {
		return Err(Error::value("bytes length not a multiple of item size"));
	}
	let is_12 = kind == SubtableKind::Format12;
	// Group ranges come straight from the font, so follow HarfBuzz: clamp each
	// group to U+10FFFF and skip any group that is inverted or starts before the
	// previous one ends.
	let mut code_to_gid = CharMap::new();
	let (mut clamped, mut skipped) = (false, false);
	let mut last_end = 0u32;
	let mut dropped = false;
	for g in groups.as_chunks::<12>().0 {
		let mut start = be32(g, 0);
		let mut end = be32(g, 4);
		let mut glyph_id = be32(g, 8) as u64;
		if end > 0x10FFFF {
			end = 0x10FFFF;
			clamped = true;
		}
		if start > end || start < last_end {
			skipped = true;
			continue;
		}
		last_end = end;
		if glyph_id == 0 {
			// The missing glyph maps nothing: all of a format 13 group, only the
			// first code point of a format 12 one.
			if !is_12 {
				continue;
			}
			start += 1;
			glyph_id += 1;
		}
		for (k, code) in (start..=end).enumerate() {
			let gid = if is_12 { glyph_id + k as u64 } else { glyph_id };
			match u32::try_from(gid) {
				Ok(gid) => {
					code_to_gid.insert(code, gid);
				}
				Err(_) => dropped = true,
			}
		}
	}
	if clamped {
		log::warn!("cmap subtable format {format}: group ranges beyond U+10FFFF were truncated");
	}
	if skipped {
		log::warn!("cmap subtable format {format}: skipped unsorted or overlapping groups");
	}
	if dropped {
		// fontTools keeps these as "glyphNNNNN" names; glyph IDs are 32-bit here.
		log::warn!("cmap subtable format {format}: dropped glyph IDs beyond 0xFFFFFFFF");
	}
	Ok(code_to_gid.into_iter().collect())
}

fn decompile_format_14(data: &[u8], kind: SubtableKind, num_records: u32) -> Result<UvsDict> {
	let record_size: i64 = if kind == SubtableKind::Format15 { 6 } else { 5 };
	let mut uvs_dict = UvsDict::new();
	let mut rec_offset = 0usize;
	// The offsets count from the start of the subtable, whose 10-byte header has
	// been stripped; fontTools slices with possibly negative indices.
	let read_u32 = |start: i64| -> Result<u32> {
		let s = py_slice(data, start, start + 4);
		if s.len() != 4 {
			return Err(unpack_error(4));
		}
		Ok(be32(s, 0))
	};
	for _ in 0..num_records {
		let rec = clamp(data, rec_offset, rec_offset + 11);
		if rec.len() != 11 {
			return Err(unpack_error(11));
		}
		rec_offset += 11;
		let var_uvs = be24(rec, 0);
		let def_offset = be32(rec, 3);
		let non_def_offset = be32(rec, 7);
		if def_offset != 0 {
			let mut start = def_offset as i64 - 10;
			let num_values = read_u32(start)?;
			start += 4;
			for _ in 0..num_values {
				let r = py_slice(data, start, start + 4);
				if r.len() != 4 {
					return Err(unpack_error(4));
				}
				start += 4;
				let first = be24(r, 0);
				let count = r[3] as u32 + 1;
				let list = uvs_dict.entry(var_uvs).or_default();
				list.extend((first..first + count).map(|uv| (uv, None)));
			}
		}
		if non_def_offset != 0 {
			let mut start = non_def_offset as i64 - 10;
			let num_recs = read_u32(start)?;
			start += 4;
			let mut local = Vec::new();
			for _ in 0..num_recs {
				let r = py_slice(data, start, start + record_size);
				if r.len() != record_size as usize {
					return Err(unpack_error(record_size as usize));
				}
				start += record_size;
				let uv = be24(r, 0);
				let gid = if kind == SubtableKind::Format15 {
					be24(r, 3)
				} else {
					be16(r, 3) as u32
				};
				local.push((uv, Some(gid)));
			}
			uvs_dict.entry(var_uvs).or_default().extend(local);
		}
	}
	Ok(uvs_dict)
}

// ---------------------------------------------------------------------------
// Compiling

fn compile_format_0(items: &[(u32, u32)], language: Option<u32>) -> Result<Vec<u8>> {
	if items.iter().any(|&(code, _)| code > 255) {
		return Err(Error::Assertion(
			"set(cmap.keys()).issubset(range(256))".into(),
		));
	}
	let mut gids = [0u32; 256];
	for &(code, gid) in items {
		gids[code as usize] = gid;
	}
	let mut data = Vec::with_capacity(262);
	data.put_u16(0);
	data.put_u16(262);
	data.put_u16(language_u16(language)?);
	for gid in gids {
		data.put_u8(
			u8::try_from(gid)
				.map_err(|_| Error::value("unsigned byte integer is greater than maximum"))?,
		);
	}
	Ok(data)
}

fn check_16_bit_gids(items: &[(u32, u32)], format: u16) -> Result<()> {
	for &(_, gid) in items {
		if gid > 0xFFFF {
			return Err(Error::Struct(format!(
				"cmap format {format} cannot represent glyph ID {gid}"
			)));
		}
	}
	Ok(())
}

/// `cmap_format_2.setIDDelta`.
fn set_id_delta(sub_header: &mut SubHeader) {
	sub_header.id_delta = 0;
	// An empty subheader (an all-notdef two-byte range) has no glyphs to adjust.
	let Some(&first) = sub_header.glyph_index_array.first() else {
		return;
	};
	// Find the minimum glyph index which is not zero.
	let mut min_gi = first;
	for &gid in &sub_header.glyph_index_array {
		if gid != 0 && gid < min_gi {
			min_gi = gid;
		}
	}
	// Pick an idDelta such that the first glyph index is 1, so that subranges are
	// more likely to be shared; idDelta is a short, so wrap around past 0x8000.
	if min_gi > 1 {
		let id_delta = if min_gi > 0x8000 {
			-(0x10000 - min_gi) - 1
		} else {
			min_gi - 1
		};
		sub_header.id_delta = id_delta;
		for i in 0..sub_header.entry_count as usize {
			let gid = sub_header.glyph_index_array[i];
			if gid > 0 {
				sub_header.glyph_index_array[i] = (gid - id_delta) & 0xFFFF;
			}
		}
	}
}

fn compile_format_2(items: &[(u32, u32)], language: Option<u32>) -> Result<Vec<u8>> {
	check_16_bit_gids(items, 2)?;
	const EMPTY_TWO_CHAR_CODE_RANGE: i64 = -1;
	let new_sub_header = |first_code, entry_count, id_range_offset| SubHeader {
		first_code,
		entry_count,
		id_delta: 0,
		id_range_offset,
		glyph_index_array: Vec::new(),
	};
	// One-byte char codes all map to subheader 0; two-byte ranges are assumed to
	// map to the empty subheader unless proven otherwise.
	let mut sub_header_keys = [EMPTY_TWO_CHAR_CODE_RANGE; 256];
	let mut list: Vec<SubHeader> = Vec::new();
	// Char codes mapped to .notdef are left unencoded.
	let items: Vec<(i64, i64)> = items
		.iter()
		.filter(|&&(_, gid)| gid != 0)
		.map(|&(c, g)| (c as i64, g as i64))
		.collect();
	// Force subheader 0 to exist when no one-byte char code is mapped.
	if items.first().is_none_or(|&(code, _)| code > 255) {
		list.push(new_sub_header(0, 0, 0));
	}
	let set_one_byte_keys = |keys: &mut [i64; 256], sh: &SubHeader| -> Result<()> {
		for index in 0..sh.entry_count {
			let code = (sh.first_code + index) as usize;
			*keys.get_mut(code).ok_or_else(index_error)? = 0;
		}
		Ok(())
	};
	let mut last_first_byte: i64 = -1;
	for &(char_code, gid) in &items {
		let first_byte = char_code >> 8;
		let second_byte = char_code & 0xFF;
		if first_byte != last_first_byte {
			if last_first_byte > -1
				&& let Some(sh) = list.last_mut()
			{
				set_id_delta(sh);
				if last_first_byte == 0 {
					set_one_byte_keys(&mut sub_header_keys, sh)?;
				}
			}
			let mut sh = new_sub_header(second_byte, 1, 0);
			sh.glyph_index_array.push(gid);
			list.push(sh);
			*sub_header_keys
				.get_mut(first_byte as usize)
				.ok_or_else(index_error)? = list.len() as i64 - 1;
			last_first_byte = first_byte;
		} else if let Some(sh) = list.last_mut() {
			// Fill in .notdef for the code points between the last and this one.
			let code_diff = second_byte - (sh.first_code + sh.entry_count);
			for _ in 0..code_diff {
				sh.glyph_index_array.push(0);
			}
			sh.glyph_index_array.push(gid);
			sh.entry_count += code_diff + 1;
		}
	}
	if let Some(sh) = list.last_mut() {
		set_id_delta(sh);
		if last_first_byte == 0 {
			set_one_byte_keys(&mut sub_header_keys, sh)?;
		}
	}
	// A final subheader for the first bytes that map to empty two-byte ranges.
	list.push(new_sub_header(0, 0, 2));
	let empty_index = list.len() as i64 - 1;
	for key in sub_header_keys.iter_mut() {
		if *key == EMPTY_TWO_CHAR_CODE_RANGE {
			*key = empty_index;
		}
	}
	// Offset to the start of the glyph index array from the first subheader's
	// idRangeOffset; subheaders with the same subrange share it.
	let mut id_range_offset = (list.len() as i64 - 1) * 8 + 2;
	for index in 0..list.len() - 1 {
		list[index].id_range_offset = 0;
		for j in 0..index {
			if list[j].glyph_index_array == list[index].glyph_index_array {
				list[index].id_range_offset = list[j].id_range_offset - (index - j) as i64 * 8;
				list[index].glyph_index_array = Vec::new();
				break;
			}
		}
		if list[index].id_range_offset == 0 {
			list[index].id_range_offset = id_range_offset;
			id_range_offset = (id_range_offset - 8) + list[index].entry_count * 2;
		} else {
			id_range_offset -= 8;
		}
	}
	let mut length = 6 + 512 + 8 * list.len();
	for sh in &list[..list.len() - 1] {
		length += sh.glyph_index_array.len() * 2;
	}
	let mut data = Vec::with_capacity(length);
	data.put_u16(2);
	data.put_u16(fit(length, "length")?);
	data.put_u16(language_u16(language)?);
	for key in sub_header_keys {
		data.put_u16(fit(key * 8, "subHeaderKeys")?);
	}
	for sh in &list {
		data.put_u16(fit(sh.first_code, "firstCode")?);
		data.put_u16(fit(sh.entry_count, "entryCount")?);
		data.put_i16(fit(sh.id_delta, "idDelta")?);
		data.put_u16(fit(sh.id_range_offset, "idRangeOffset")?);
	}
	for sh in &list[..list.len() - 1] {
		for &gi in &sh.glyph_index_array {
			data.put_u16(fit(gi, "glyphIndexArray")?);
		}
	}
	Ok(data)
}

/// Splits a range of consecutive character codes into subranges with
/// consecutive glyph IDs, where that makes the format 4 subtable smaller
/// (`splitRange`). Returns the additional start codes and the end codes.
fn split_range(start_code: i64, end_code: i64, cmap: &HashMap<i64, i64>) -> (Vec<i64>, Vec<i64>) {
	if start_code == end_code {
		return (Vec::new(), vec![end_code]);
	}
	let gid = |code: i64| cmap.get(&code).copied().unwrap_or_default();
	let mut last_id = gid(start_code);
	let mut last_code = start_code;
	let mut in_order: Option<bool> = None;
	let mut ordered_begin = start_code;
	let mut sub_ranges: Vec<(i64, i64)> = Vec::new();
	// Gather subranges in which the glyph IDs are consecutive.
	for code in start_code + 1..=end_code {
		let glyph_id = gid(code);
		if glyph_id - 1 == last_id {
			if in_order != Some(true) {
				in_order = Some(true);
				ordered_begin = last_code;
			}
		} else if in_order == Some(true) {
			in_order = Some(false);
			sub_ranges.push((ordered_begin, last_code));
		}
		last_id = glyph_id;
		last_code = code;
	}
	if in_order == Some(true) {
		sub_ranges.push((ordered_begin, last_code));
	}
	// Filter out the subranges that would only make the data bigger: a new segment
	// costs 8 bytes, not using one costs 2 bytes per character.
	let mut new_ranges = Vec::new();
	for &(b, e) in &sub_ranges {
		if b == start_code && e == end_code {
			break; // the whole range, we're fine
		}
		let threshold = if b == start_code || e == end_code {
			4
		} else {
			8
		};
		if e - b + 1 > threshold {
			new_ranges.push((b, e));
		}
	}
	let mut sub_ranges = new_ranges;
	let (Some(&(first, _)), Some(&(_, last))) = (sub_ranges.first(), sub_ranges.last()) else {
		return (Vec::new(), vec![end_code]);
	};
	if first != start_code {
		sub_ranges.insert(0, (start_code, first - 1));
	}
	if last != end_code {
		sub_ranges.push((last + 1, end_code));
	}
	// Fill the holes: the segments in which the glyph IDs are not consecutive.
	let mut i = 1;
	while i < sub_ranges.len() {
		if sub_ranges[i - 1].1 + 1 != sub_ranges[i].0 {
			sub_ranges.insert(i, (sub_ranges[i - 1].1 + 1, sub_ranges[i].0 - 1));
			i += 1;
		}
		i += 1;
	}
	let mut start: Vec<i64> = sub_ranges.iter().map(|r| r.0).collect();
	let end: Vec<i64> = sub_ranges.iter().map(|r| r.1).collect();
	start.remove(0);
	(start, end)
}

/// A value for `array.array("H", …)`.
fn u16_item(value: i64) -> Result<u16> {
	u16::try_from(value).map_err(|_| {
		Error::value(if value < 0 {
			"unsigned short is less than minimum"
		} else {
			"unsigned short is greater than maximum"
		})
	})
}

fn compile_format_4(items: &[(u32, u32)], format: u16, language: Option<u32>) -> Result<Vec<u8>> {
	let mut cmap: HashMap<i64, i64> = HashMap::new();
	let (start_code, end_code) = if items.is_empty() {
		(vec![0xFFFF], vec![0xFFFF])
	} else {
		check_16_bit_gids(items, 4)?;
		for &(code, gid) in items {
			cmap.insert(code as i64, gid as i64);
		}
		// Split the char codes in ranges of consecutive char codes, then split each
		// range in more ranges of consecutive/not consecutive glyph IDs.
		let codes: Vec<i64> = items.iter().map(|&(c, _)| c as i64).collect();
		let mut last_code = codes[0];
		let mut end_code = Vec::new();
		let mut start_code = vec![last_code];
		for &code in &codes[1..] {
			if code == last_code + 1 {
				last_code = code;
				continue;
			}
			let (start, end) = split_range(start_code[start_code.len() - 1], last_code, &cmap);
			start_code.extend(start);
			end_code.extend(end);
			start_code.push(code);
			last_code = code;
		}
		let (start, end) = split_range(start_code[start_code.len() - 1], last_code, &cmap);
		start_code.extend(start);
		end_code.extend(end);
		start_code.push(0xFFFF);
		end_code.push(0xFFFF);
		(start_code, end_code)
	};

	let mut id_delta = Vec::new();
	let mut id_range_offset = Vec::new();
	let mut glyph_index_array: Vec<i64> = Vec::new();
	for i in 0..end_code.len() - 1 {
		// skip the closing codes (0xffff)
		let indices: Vec<i64> = (start_code[i]..=end_code[i])
			.map(|c| cmap.get(&c).copied().unwrap_or_default())
			.collect();
		let consecutive = indices
			.iter()
			.enumerate()
			.all(|(k, &g)| g == indices[0] + k as i64);
		if consecutive {
			id_delta.push((indices[0] - start_code[i]).rem_euclid(0x10000));
			id_range_offset.push(0);
		} else {
			id_delta.push(0);
			id_range_offset.push(2 * (end_code.len() + glyph_index_array.len() - i) as i64);
			glyph_index_array.extend(indices);
		}
	}
	id_delta.push(1); // 0xffff + 1 == 0, so this end code maps to .notdef
	id_range_offset.push(0);

	let seg_count = end_code.len();
	let (search_range, entry_selector, range_shift) =
		get_search_range(fit(seg_count, "segCount")?, 2);
	let mut data = Vec::new();
	for &v in end_code.iter().chain(&[0]).chain(&start_code) {
		data.put_u16(u16_item(v)?);
	}
	for &v in id_delta
		.iter()
		.chain(&id_range_offset)
		.chain(&glyph_index_array)
	{
		data.put_u16(u16_item(v)?);
	}
	let length = 14 + data.len();
	let mut out = Vec::with_capacity(length);
	out.put_u16(format);
	out.put_u16(fit(length, "length")?);
	out.put_u16(language_u16(language)?);
	out.put_u16(fit(seg_count * 2, "segCountX2")?);
	out.put_u16(fit(search_range, "searchRange")?);
	out.put_u16(fit(entry_selector, "entrySelector")?);
	out.put_u16(fit(range_shift, "rangeShift")?);
	out.extend(data);
	Ok(out)
}

fn compile_format_6(items: &[(u32, u32)], language: Option<u32>) -> Result<Vec<u8>> {
	let mut gids = Vec::new();
	let (first_code, count) = match (items.first(), items.last()) {
		(Some(&(first, _)), Some(&(last, _))) => {
			let map: HashMap<u32, u32> = items.iter().copied().collect();
			for code in first..=last {
				let gid = map.get(&code).copied().unwrap_or(0);
				gids.put_u16(u16_item(gid as i64)?);
			}
			(first, (last - first) as usize + 1)
		}
		_ => (0, 0),
	};
	let mut data = Vec::with_capacity(10 + gids.len());
	data.put_u16(6);
	data.put_u16(fit(gids.len() + 10, "length")?);
	data.put_u16(language_u16(language)?);
	data.put_u16(fit(first_code, "firstCode")?);
	data.put_u16(fit(count, "entryCount")?);
	data.extend(gids);
	Ok(data)
}

#[cfg(test)]
mod tests;
