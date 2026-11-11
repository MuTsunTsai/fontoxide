//! The private tables of Microsoft Visual TrueType (VTT), which store its
//! hinting source: `TSI0`/`TSI2` (`T_S_I__0`, `T_S_I__2`), `TSI1`/`TSI3`
//! (`T_S_I__1`, `T_S_I__3`), `TSI5` (`T_S_I__5`) and the source text tables
//! `TSIB`, `TSID`, `TSIJ`, `TSIP`, `TSIS` and `TSIV` (`T_S_I_V_` and its
//! subclasses); also `TTFA` (`T_T_F_A_`), which shares their base class
//! (`asciiTable`).
//!
//! See <https://learn.microsoft.com/en-us/typography/tools/vtt/tsi-tables>.

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes, fit};
use crate::sstruct::eval;
use crate::table::{DefaultTable, Table, TableType};
use crate::tables::Maxp;
use crate::{Error, Font, Result};

const TSI0: Tag = Tag::new(b"TSI0");
const TSI1: Tag = Tag::new(b"TSI1");
const TSI2: Tag = Tag::new(b"TSI2");
const TSI3: Tag = Tag::new(b"TSI3");

/// The magic record between the glyph and the extra records of an index table.
const MAGIC: TsiEntry = (0xFFFE, 0, 0xABFC_1F34);

/// An index record: (glyph ID or extra program code, text length, text
/// offset).
pub type TsiEntry = (u32, u32, u32);

/// The index of the programs in `TSI1` (`TSI0`) or `TSI3` (`TSI2`)
/// (`table_T_S_I__0`, `table_T_S_I__2`).
///
/// It is recalculated when its program table is compiled.
#[derive(Debug, Clone, PartialEq)]
pub struct TsiIndex {
	/// `TSI0` or `TSI2`.
	pub tag: Tag,
	/// The records of the glyph programs (`indices`); `None` until the table is
	/// decompiled or set by its program table, in which case it compiles to no
	/// data.
	pub indices: Option<Vec<TsiEntry>>,
	/// The records of the four extra programs (`extra_indices`).
	pub extra_indices: Vec<TsiEntry>,
}

impl Default for TsiIndex {
	fn default() -> Self {
		TsiIndex::new_as(TSI0)
	}
}

impl TsiIndex {
	/// Sets the records (`set`), as the program table does when it is compiled.
	pub fn set(&mut self, indices: Vec<TsiEntry>, extra_indices: Vec<TsiEntry>) {
		self.indices = Some(indices);
		self.extra_indices = extra_indices;
	}

	/// Parses the records; `num_glyphs` is only used for a warning.
	pub fn decompile_records(&mut self, data: &[u8], num_glyphs: usize) -> Result<()> {
		let num_entries = data.len() / 8;
		if num_entries != num_glyphs + 5 {
			let diff = num_entries as i64 - num_glyphs as i64 - 5;
			log::warn!(
				"Number of glyphPrograms differs from the number of glyphs in the font by {} ({} programs vs. {num_glyphs} glyphs).",
				diff.abs(),
				num_entries as i64 - 5
			);
		}
		let mut r = Reader::new(data);
		let mut indices = Vec::with_capacity(num_entries);
		for _ in 0..num_entries {
			indices.push((u32::from(r.u16()?), u32::from(r.u16()?), r.u32()?));
		}
		if r.remaining() != 0 {
			return Err(Error::Assertion(String::new()));
		}
		if indices.len() < 5 {
			return Err(Error::ttlib("IndexError: list index out of range"));
		}
		if indices[indices.len() - 5] != MAGIC {
			return Err(Error::Assertion("bad magic number".into()));
		}
		self.extra_indices = indices[indices.len() - 4..].to_vec();
		indices.truncate(indices.len() - 5);
		self.indices = Some(indices);
		Ok(())
	}

	/// Compiles the records.
	pub fn compile_records(&self) -> Result<Vec<u8>> {
		let Some(indices) = &self.indices else {
			// There is no corresponding program table: no data, which effectively
			// means "ignore us".
			return Ok(Vec::new());
		};
		let mut data = Vec::new();
		for &(index, length, offset) in indices
			.iter()
			.chain(std::iter::once(&MAGIC))
			.chain(&self.extra_indices)
		{
			data.put_u16(fit(index, "glyphID")?);
			data.put_u16(fit(length, "textLength")?);
			data.put_u32(offset);
		}
		Ok(data)
	}
}

impl TableType for TsiIndex {
	const TAG: Tag = TSI0;

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		Self::decompile_as(TSI0, data, font)
	}

	fn decompile_as(tag: Tag, data: &[u8], font: &mut Font) -> Result<Self> {
		let num_glyphs = font.require::<Maxp>()?.num_glyphs as usize;
		let mut table = Self::new_as(tag);
		table.decompile_records(data, num_glyphs)?;
		Ok(table)
	}

	fn new_as(tag: Tag) -> Self {
		TsiIndex {
			tag,
			indices: None,
			extra_indices: Vec::new(),
		}
	}
}

impl Table for TsiIndex {
	fn tag(&self) -> Tag {
		self.tag
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		self.compile_records()
	}

	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		writer.comment("This table will be calculated by the compiler");
		writer.newline();
		Ok(())
	}

	/// `DefaultTable.fromXML`: hex data is decompiled.
	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		let mut raw = DefaultTable::new(self.tag, Vec::new());
		raw.from_xml(element, font)?;
		*self = Self::decompile_as(self.tag, &raw.data, font)?;
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	/// The records are recalculated when the program table is compiled.
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Ok(())
	}
}

/// The extra programs of `TSI1`, by code.
const TSI1_EXTRAS: [(u32, &str); 4] = [
	(0xFFFA, "ppgm"),
	(0xFFFB, "cvt"),
	(0xFFFC, "reserved"),
	(0xFFFD, "fpgm"),
];

/// The extra programs of `TSI3`, by code.
const TSI3_EXTRAS: [(u32, &str); 4] = [
	(0xFFFA, "reserved0"),
	(0xFFFB, "reserved1"),
	(0xFFFC, "reserved2"),
	(0xFFFD, "reserved3"),
];

/// The source text of the glyph programs and extra programs, in low-level
/// assembly (`TSI1`) or VTTTalk (`TSI3`) (`table_T_S_I__1`, `table_T_S_I__3`).
///
/// The index is in `TSI0` or `TSI2`, which compiling this table updates.
///
/// LENIENT(L38): fontTools fails to dump a table that was neither decompiled nor
/// read from TTX (the program attributes do not exist yet); here it is empty.
#[derive(Debug, Clone, PartialEq)]
pub struct TsiPrograms {
	/// `TSI1` or `TSI3`.
	pub tag: Tag,
	/// The glyph programs by glyph ID (`glyphPrograms`, keyed by glyph name in
	/// fontTools). Lines are separated by `"\r"`.
	pub glyph_programs: BTreeMap<u32, String>,
	/// The extra programs by name (`extraPrograms`).
	pub extra_programs: BTreeMap<String, String>,
}

impl Default for TsiPrograms {
	fn default() -> Self {
		TsiPrograms::new_as(TSI1)
	}
}

impl TsiPrograms {
	/// The tag of the index table (`indextable`).
	pub fn index_tag(&self) -> Tag {
		if self.tag == TSI3 { TSI2 } else { TSI0 }
	}

	/// The extra programs by code (`extras`).
	pub fn extras(&self) -> &'static [(u32, &'static str); 4] {
		if self.tag == TSI3 {
			&TSI3_EXTRAS
		} else {
			&TSI1_EXTRAS
		}
	}

	/// Parses the programs with the index `index` (`decompile`).
	pub fn decompile_with_index(
		&mut self,
		data: &[u8],
		index: &TsiIndex,
		font: &mut Font,
	) -> Result<()> {
		let total_length = data.len() as u64;
		let indices = index
			.indices
			.as_ref()
			.ok_or_else(|| Error::ttlib("AttributeError: 'indices'"))?;
		for (records, is_extra) in [(indices, false), (&index.extra_indices, true)] {
			let mut glyph_programs = BTreeMap::new();
			let mut extra_programs = BTreeMap::new();
			for (i, &(glyph_id, text_length, text_offset)) in records.iter().enumerate() {
				let name = if is_extra {
					let (_, name) = self
						.extras()
						.iter()
						.find(|(code, _)| *code == glyph_id)
						.ok_or_else(|| Error::ttlib(format!("KeyError: {glyph_id}")))?;
					(*name).to_owned()
				} else {
					font.glyph_name(glyph_id)?
				};
				let text_offset = u64::from(text_offset);
				if text_offset > total_length {
					log::warn!(
						"textOffset > totalLength; {} skipped",
						fontoxide_misc::py::str_repr(&name)
					);
					continue;
				}
				let mut text_length = u64::from(text_length);
				if text_length == 0x8000 {
					let is_last = i == records.len() - 1;
					let mut next_text_offset = if !is_last {
						u64::from(records[i + 1].2)
					} else if is_extra {
						// The last record of the table runs to its end.
						total_length
					} else {
						// The last glyph record runs to the first extra record.
						let first = index
							.extra_indices
							.first()
							.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
						u64::from(first.2)
					};
					if next_text_offset < text_offset {
						return Err(Error::Assertion("entries not sorted by offset".into()));
					}
					if next_text_offset > total_length {
						log::warn!(
							"nextTextOffset > totalLength; {} truncated",
							fontoxide_misc::py::str_repr(&name)
						);
						next_text_offset = total_length;
					}
					text_length = next_text_offset - text_offset;
				} else if text_length > 0x8000 {
					return Err(Error::ttlib(format!(
						"{} textLength ({text_length}) must not be > 32768",
						fontoxide_misc::py::str_repr(&name)
					)));
				}
				let end = (text_offset + text_length).min(total_length);
				let text = &data[text_offset as usize..end as usize];
				if text.len() as u64 != text_length {
					return Err(Error::Assertion(String::new()));
				}
				let text = std::str::from_utf8(text)
					.map_err(|e| Error::value(format!("UnicodeDecodeError: {e}")))?;
				if !text.is_empty() {
					if is_extra {
						extra_programs.insert(name, text.to_owned());
					} else {
						glyph_programs.insert(glyph_id, text.to_owned());
					}
				}
			}
			if is_extra {
				self.extra_programs = extra_programs;
			} else {
				self.glyph_programs = glyph_programs;
			}
		}
		Ok(())
	}

	/// Compiles the programs and returns the data and the index records
	/// (`compile`, without updating the index table).
	pub fn compile_with_index(
		&self,
		font: &mut Font,
	) -> Result<(Vec<u8>, Vec<TsiEntry>, Vec<TsiEntry>)> {
		let mut data: Vec<u8> = Vec::new();
		let num_glyphs = font.glyph_order()?.len();
		// Aligned on 2-byte boundaries, filled with return characters.
		let align = |data: &mut Vec<u8>| {
			if !data.len().is_multiple_of(2) {
				data.push(b'\r');
			}
		};
		let mut indices = Vec::with_capacity(num_glyphs);
		for gid in 0..num_glyphs as u32 {
			align(&mut data);
			let text = self
				.glyph_programs
				.get(&gid)
				.map_or(&[][..], |t| t.as_bytes());
			let length = text.len().min(0x8000) as u32;
			indices.push((gid, length, fit(data.len(), "textOffset")?));
			data.extend_from_slice(text);
		}
		let mut extra_indices = Vec::with_capacity(4);
		for &(code, name) in self.extras() {
			align(&mut data);
			let text = self
				.extra_programs
				.get(name)
				.map_or(&[][..], |t| t.as_bytes());
			let length = text.len().min(0x8000) as u32;
			extra_indices.push((code, length, fit(data.len(), "textOffset")?));
			data.extend_from_slice(text);
		}
		Ok((data, indices, extra_indices))
	}
}

impl TableType for TsiPrograms {
	const TAG: Tag = TSI1;

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		Self::decompile_as(TSI1, data, font)
	}

	fn decompile_as(tag: Tag, data: &[u8], font: &mut Font) -> Result<Self> {
		let mut table = Self::new_as(tag);
		let index_tag = table.index_tag();
		let index = font
			.get_at::<TsiIndex>(index_tag)?
			.ok_or(Error::TableNotFound(index_tag))?
			.clone();
		table.decompile_with_index(data, &index, font)?;
		Ok(table)
	}

	fn new_as(tag: Tag) -> Self {
		TsiPrograms {
			tag,
			glyph_programs: BTreeMap::new(),
			extra_programs: BTreeMap::new(),
		}
	}
}

/// Text written with `"\r"` line separators shown as `"\n"`.
fn write_source(writer: &mut XmlWriter, element: &str, name: &str, text: &str) {
	writer.begintag(element, &[("name", &name)]);
	writer.newline();
	writer.write_noindent(&text.replace('\r', "\n"));
	writer.newline();
	writer.endtag(element);
	writer.newline();
}

/// The lines of an element's text but the first and the last (the line breaks
/// around the text), as fontTools reads source text.
fn inner_lines(element: &Element, cr: bool) -> Vec<String> {
	let mut text = element.text();
	if cr {
		text = text.replace('\r', "\n");
	}
	let lines: Vec<&str> = text.split('\n').collect();
	if lines.len() < 2 {
		return Vec::new();
	}
	lines[1..lines.len() - 1]
		.iter()
		.map(|l| (*l).to_owned())
		.collect()
}

impl Table for TsiPrograms {
	fn tag(&self) -> Tag {
		self.tag
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		let (data, indices, extra_indices) = self.compile_with_index(font)?;
		let index_tag = self.index_tag();
		font.get_at::<TsiIndex>(index_tag)?
			.ok_or(Error::TableNotFound(index_tag))?
			.set(indices, extra_indices);
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let order = font.glyph_order()?;
		let mut glyph_programs: Vec<(String, &String)> = self
			.glyph_programs
			.iter()
			.map(|(&gid, text)| (order.name(gid).into_owned(), text))
			.collect();
		glyph_programs.sort();
		writer.newline();
		for (name, text) in glyph_programs {
			if text.is_empty() {
				continue;
			}
			write_source(writer, "glyphProgram", &name, text);
			writer.newline();
		}
		for (name, text) in &self.extra_programs {
			if text.is_empty() {
				continue;
			}
			write_source(writer, "extraProgram", name, text);
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		let text = inner_lines(element, true).join("\r");
		let name = || {
			element
				.attr("name")
				.ok_or_else(|| Error::ttx(format!("missing 'name' on <{}>", element.name)))
		};
		match element.name.as_str() {
			"glyphProgram" => {
				let gid = font.glyph_id(name()?)?;
				self.glyph_programs.insert(gid, text);
			}
			"extraProgram" => {
				self.extra_programs.insert(name()?.to_owned(), text);
			}
			_ => {}
		}
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	/// Programs of glyphs that no longer exist are dropped, as fontTools ignores
	/// names that are not in the glyph order when compiling.
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let mut programs = BTreeMap::new();
		for (gid, text) in std::mem::take(&mut self.glyph_programs) {
			if let Ok(new) = f(gid) {
				programs.insert(new, text);
			}
		}
		self.glyph_programs = programs;
		Ok(())
	}
}

/// VTT character groups (`table_T_S_I__5`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tsi5 {
	/// The group of each glyph by glyph ID (`glyphGrouping`, keyed by glyph name
	/// in fontTools). Glyphs without an entry are in group 0.
	pub glyph_grouping: BTreeMap<u32, i64>,
}

impl TableType for Tsi5 {
	const TAG: Tag = Tag::new(b"TSI5");

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		let num_glyphs = font.require::<Maxp>()?.num_glyphs as usize;
		if !data.len().is_multiple_of(2) {
			return Err(Error::value("bytes length not a multiple of item size"));
		}
		let num_entries = data.len() / 2;
		if num_entries != num_glyphs {
			let diff = num_entries as i64 - num_glyphs as i64;
			log::warn!(
				"Number of entries differs from the number of glyphs in the font by {} ({num_entries} entries vs. {num_glyphs} glyphs).",
				diff.abs()
			);
		}
		let mut glyph_grouping = BTreeMap::new();
		for (i, &v) in data.as_chunks::<2>().0.iter().enumerate() {
			glyph_grouping.insert(i as u32, i64::from(u16::from_be_bytes(v)));
		}
		Ok(Tsi5 { glyph_grouping })
	}
}

impl Table for Tsi5 {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		let num_glyphs = font.glyph_order()?.len() as u32;
		let mut data = Vec::new();
		for gid in 0..num_glyphs {
			let v = self.glyph_grouping.get(&gid).copied().unwrap_or(0);
			data.put_u16(fit(v, "glyphgroup")?);
		}
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let order = font.glyph_order()?;
		let mut groups: Vec<(String, i64)> = self
			.glyph_grouping
			.iter()
			.map(|(&gid, &v)| (order.name(gid).into_owned(), v))
			.collect();
		groups.sort();
		for (name, value) in groups {
			writer.simpletag("glyphgroup", &[("name", &name), ("value", &value)]);
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		if element.name != "glyphgroup" {
			return Ok(());
		}
		let attr = |name: &str| {
			element
				.attr(name)
				.ok_or_else(|| Error::ttx(format!("missing '{name}' on <glyphgroup>")))
		};
		let gid = font.glyph_id(attr("name")?)?;
		let value = eval(attr("value")?)?
			.as_int()
			.ok_or_else(|| Error::value("invalid glyphgroup value"))?;
		self.glyph_grouping.insert(gid, fit(value, "glyphgroup")?);
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	/// Entries of glyphs that no longer exist are dropped, as fontTools ignores
	/// names that are not in the glyph order when compiling.
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let mut grouping = BTreeMap::new();
		for (gid, v) in std::mem::take(&mut self.glyph_grouping) {
			if let Ok(new) = f(gid) {
				grouping.insert(new, v);
			}
		}
		self.glyph_grouping = grouping;
		Ok(())
	}
}

/// A table of ASCII source text (`asciiTable`): `TTFA`, and the VTT source
/// tables `TSIB`, `TSID`, `TSIJ`, `TSIP`, `TSIS` and `TSIV` (`T_S_I_V_`), whose
/// lines are separated by `"\r"`.
#[derive(Debug, Clone, PartialEq)]
pub struct AsciiTable {
	/// The table tag.
	pub tag: Tag,
	/// The text (`data`).
	pub data: Vec<u8>,
}

impl Default for AsciiTable {
	fn default() -> Self {
		AsciiTable::new_as(Tag::new(b"TTFA"))
	}
}

impl AsciiTable {
	/// Whether this is a VTT table, with `"\r"` line separators.
	fn is_vtt(&self) -> bool {
		self.tag != Tag::new(b"TTFA")
	}
}

impl TableType for AsciiTable {
	const TAG: Tag = Tag::new(b"TTFA");

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		Self::decompile_as(Self::TAG, data, font)
	}

	fn decompile_as(tag: Tag, data: &[u8], _font: &mut Font) -> Result<Self> {
		Ok(AsciiTable {
			tag,
			data: data.to_vec(),
		})
	}

	fn new_as(tag: Tag) -> Self {
		AsciiTable {
			tag,
			data: Vec::new(),
		}
	}
}

fn ascii_error(data: &[u8]) -> Error {
	let pos = data.iter().position(|b| !b.is_ascii()).unwrap_or_default();
	Error::value(format!(
		"UnicodeDecodeError: 'ascii' codec can't decode byte 0x{:02x} in position {pos}: ordinal not in range(128)",
		data.get(pos).copied().unwrap_or_default()
	))
}

impl Table for AsciiTable {
	fn tag(&self) -> Tag {
		self.tag
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		Ok(self.data.clone())
	}

	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		if !self.data.is_ascii() {
			return Err(ascii_error(&self.data));
		}
		// Null bytes are removed.
		let text: String = self
			.data
			.iter()
			.filter(|&&b| b != 0)
			.map(|&b| b as char)
			.collect();
		let text = if self.is_vtt() {
			text.replace('\r', "\n")
		} else {
			text
		};
		writer.begintag("source", &[]);
		writer.newline();
		writer.write_noindent(&text);
		writer.newline();
		writer.endtag("source");
		writer.newline();
		Ok(())
	}

	/// Every element is read as the source, whatever its name.
	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		let separator = if self.is_vtt() { "\r" } else { "\n" };
		let text = inner_lines(element, false).join(separator);
		if !text.is_ascii() {
			let pos = text.chars().position(|c| !c.is_ascii()).unwrap_or_default();
			return Err(Error::value(format!(
				"UnicodeEncodeError: 'ascii' codec can't encode character in position {pos}: ordinal not in range(128)"
			)));
		}
		self.data = text.into_bytes();
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Ok(())
	}
}
