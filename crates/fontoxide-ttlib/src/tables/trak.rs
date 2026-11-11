//! `trak`: the AAT tracking table (`fontTools.ttLib.tables._t_r_a_k`).
//!
//! See <https://developer.apple.com/fonts/TrueType-Reference-Manual/RM06/Chap6trak.html>.

use fontoxide_misc::Tag;
use fontoxide_misc::fixed_tools::{
	fixed_to_float, float_to_fixed, float_to_fixed_to_str, str_to_fixed_to_float,
};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes, fit};
use crate::sstruct::{F16Dot16, U16, U32, Value, eval};
use crate::table::{Table, TableType};
use crate::tables::Name;
use crate::{Error, Font, Result};

crate::sstruct! {
	/// The header (`TRAK_HEADER_FORMAT`).
	pub struct TrakHeader = "TRAK_HEADER_FORMAT" {
		/// The table version.
		pub version: f64 = "version" as F16Dot16,
		/// The table format.
		pub format: u16 = "format" as U16,
		/// The offset of the horizontal track data.
		pub horiz_offset: u16 = "horizOffset" as U16,
		/// The offset of the vertical track data.
		pub vert_offset: u16 = "vertOffset" as U16,
		/// Reserved.
		pub reserved: u16 = "reserved" as U16,
	}
}

crate::sstruct! {
	/// The header of track data (`TRACK_DATA_FORMAT`).
	pub struct TrackDataHeader = "TRACK_DATA_FORMAT" {
		/// The number of tracks.
		pub n_tracks: u16 = "nTracks" as U16,
		/// The number of sizes.
		pub n_sizes: u16 = "nSizes" as U16,
		/// The offset of the size table, from the start of the table.
		pub size_table_offset: u32 = "sizeTableOffset" as U32,
	}
}

crate::sstruct! {
	/// A track table entry record (`TRACK_TABLE_ENTRY_FORMAT`).
	pub struct TrackEntryRecord = "TRACK_TABLE_ENTRY_FORMAT" {
		/// The track value.
		pub track: f64 = "track" as F16Dot16,
		/// The name ID of the track.
		pub name_index: u16 = "nameIndex" as U16,
		/// The offset of the per-size values, from the start of the table.
		pub offset: u16 = "offset" as U16,
	}
}

/// The per-size tracking values of a track (`TrackTableEntry`): a mapping
/// from point size to value in font units, with the name ID of the track.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrackTableEntry {
	/// The name ID of the track (`nameIndex`); required to compile or dump.
	pub name_index: Option<i64>,
	/// The values by size, in insertion order (a dict in fontTools).
	pub values: Vec<(f64, i64)>,
}

impl TrackTableEntry {
	/// An entry with values and a name ID.
	pub fn new(values: &[(f64, i64)], name_index: Option<i64>) -> Self {
		let mut entry = TrackTableEntry {
			name_index,
			values: Vec::new(),
		};
		for &(size, value) in values {
			entry.insert(size, value);
		}
		entry
	}

	/// `entry[size] = value`.
	pub fn insert(&mut self, size: f64, value: i64) {
		match self.values.iter_mut().find(|(s, _)| *s == size) {
			Some(item) => item.1 = value,
			None => self.values.push((size, value)),
		}
	}

	/// The sizes, in insertion order.
	pub fn sizes(&self) -> Vec<f64> {
		self.values.iter().map(|(s, _)| *s).collect()
	}

	/// The values sorted by size (`sorted(entry.items())`).
	fn sorted(&self) -> Vec<(f64, i64)> {
		let mut items = self.values.clone();
		items.sort_by(|a, b| a.0.total_cmp(&b.0));
		items
	}

	/// Whether two entries are equal as fontTools compares them: same name ID
	/// and same mapping, whatever the order.
	pub fn same_as(&self, other: &TrackTableEntry) -> bool {
		self.name_index == other.name_index && self.sorted() == other.sorted()
	}
}

/// The tracks of one direction (`TrackData`): a mapping from track value to
/// [`TrackTableEntry`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrackData {
	/// The entries by track value, in insertion order (a dict in fontTools).
	pub entries: Vec<(f64, TrackTableEntry)>,
}

fn sorted_set(values: &[f64]) -> Vec<f64> {
	let mut v = values.to_vec();
	v.sort_by(f64::total_cmp);
	v.dedup();
	v
}

fn float_list_repr(values: &[f64]) -> String {
	let items: Vec<String> = values
		.iter()
		.map(|v| fontoxide_misc::py::float_repr(*v))
		.collect();
	format!("[{}]", items.join(", "))
}

impl TrackData {
	/// Track data from entries.
	pub fn new(entries: Vec<(f64, TrackTableEntry)>) -> Self {
		let mut data = TrackData::default();
		for (track, entry) in entries {
			data.insert(track, entry);
		}
		data
	}

	/// `data[track] = entry`.
	pub fn insert(&mut self, track: f64, entry: TrackTableEntry) {
		match self.entries.iter_mut().find(|(t, _)| *t == track) {
			Some(item) => item.1 = entry,
			None => self.entries.push((track, entry)),
		}
	}

	/// The number of tracks.
	pub fn len(&self) -> usize {
		self.entries.len()
	}

	/// Whether there are no tracks.
	pub fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}

	/// Whether two track data are equal as fontTools compares them (as dicts).
	pub fn same_as(&self, other: &TrackData) -> bool {
		self.len() == other.len()
			&& self.entries.iter().all(|(t, e)| {
				other
					.entries
					.iter()
					.any(|(t2, e2)| t == t2 && e.same_as(e2))
			})
	}

	/// The sizes, which all entries must share (`sizes`), sorted.
	pub fn sizes(&self) -> Result<Vec<f64>> {
		let Some((_, first)) = self.entries.first() else {
			return Ok(Vec::new());
		};
		let sizes = sorted_set(&first.sizes());
		for (_, entry) in &self.entries[1..] {
			let entry_sizes = sorted_set(&entry.sizes());
			if entry_sizes != sizes {
				return Err(Error::ttlib(format!(
					"'trak' table entries must specify the same sizes: {} != {}",
					float_list_repr(&sizes),
					float_list_repr(&entry_sizes)
				)));
			}
		}
		Ok(sizes)
	}

	fn sorted(&self) -> Vec<&(f64, TrackTableEntry)> {
		let mut items: Vec<&(f64, TrackTableEntry)> = self.entries.iter().collect();
		items.sort_by(|a, b| a.0.total_cmp(&b.0));
		items
	}

	/// Compiles the track data at `offset` from the start of the table.
	pub fn compile(&self, offset: usize) -> Result<Vec<u8>> {
		let n_tracks = self.len();
		let sizes = self.sizes()?;
		let n_sizes = sizes.len();
		let mut offset = offset + TrackDataHeader::SIZE + TrackEntryRecord::SIZE * n_tracks;
		let mut out = Vec::new();
		TrackDataHeader {
			n_tracks: fit(n_tracks, "nTracks")?,
			n_sizes: fit(n_sizes, "nSizes")?,
			size_table_offset: fit(offset, "sizeTableOffset")?,
		}
		.pack(&mut out)?;
		offset += 4 * n_sizes;
		let mut per_size = Vec::new();
		for (track, entry) in self.sorted() {
			let name_index = entry
				.name_index
				.ok_or_else(|| Error::Assertion("entry.nameIndex is not None".into()))?;
			TrackEntryRecord {
				track: *track,
				name_index: fit(name_index, "nameIndex")?,
				offset: fit(offset, "offset")?,
			}
			.pack(&mut out)?;
			for (_, value) in entry.sorted() {
				per_size.put_i16(fit(value, "perSizeValue")?);
			}
			offset += 2 * n_sizes;
		}
		for size in sizes {
			out.put_i32(fit(float_to_fixed(size, 16), "size")?);
		}
		out.extend(per_size);
		Ok(out)
	}

	/// Parses the track data at `offset` of the table data.
	pub fn decompile(data: &[u8], offset: usize) -> Result<Self> {
		let slice = |start: usize, len: usize| {
			let start = start.min(data.len());
			&data[start..(start + len).min(data.len())]
		};
		let header_data = slice(offset, 8);
		if header_data.len() != 8 {
			return Err(Error::ttlib(
				"not enough data to decompile TrackData header",
			));
		}
		let header = TrackDataHeader::unpack(&mut Reader::new(header_data))?;
		let mut offset = offset + 8;
		let mut size_offset = header.size_table_offset as usize;
		let mut size_table = Vec::new();
		for _ in 0..header.n_sizes {
			let d = slice(size_offset, 4);
			if d.len() < 4 {
				return Err(Error::ttlib(
					"not enough data to decompile TrackData size subtable",
				));
			}
			let raw = i32::from_be_bytes([d[0], d[1], d[2], d[3]]);
			size_table.push(fixed_to_float(i64::from(raw), 16));
			size_offset += 4;
		}
		let mut track_data = TrackData::default();
		for _ in 0..header.n_tracks {
			let d = slice(offset, 8);
			if d.len() < 8 {
				return Err(Error::ttlib(
					"not enough data to decompile TrackTableEntry record",
				));
			}
			let record = TrackEntryRecord::unpack(&mut Reader::new(d))?;
			let mut entry = TrackTableEntry {
				name_index: Some(i64::from(record.name_index)),
				values: Vec::new(),
			};
			let mut per_size_offset = record.offset as usize;
			for &size in &size_table {
				let d = slice(per_size_offset, 2);
				if d.len() < 2 {
					return Err(Error::ttlib(
						"not enough data to decompile per-size track values",
					));
				}
				entry.insert(size, i64::from(i16::from_be_bytes([d[0], d[1]])));
				per_size_offset += 2;
			}
			track_data.insert(record.track, entry);
			offset += 8;
		}
		Ok(track_data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let n_sizes = self.sizes()?.len();
		writer.comment(&format!("nTracks={}, nSizes={n_sizes}", self.len()));
		writer.newline();
		for (track, entry) in self.sorted() {
			let name_index = entry
				.name_index
				.ok_or_else(|| Error::Assertion("entry.nameIndex is not None".into()))?;
			let name_table = font
				.get::<Name>()?
				.ok_or(Error::TableNotFound(Tag::new(b"name")))?;
			let name = u16::try_from(name_index)
				.ok()
				.and_then(|id| name_table.get_debug_name(id));
			writer.begintag_ordered(
				"trackEntry",
				&[
					("value", &float_to_fixed_to_str(*track, 16)),
					("nameIndex", &name_index),
				],
			);
			writer.newline();
			if let Some(name) = name.filter(|n| !n.is_empty()) {
				writer.comment(&name);
				writer.newline();
			}
			for (size, value) in entry.sorted() {
				writer.simpletag(
					"track",
					&[
						("size", &float_to_fixed_to_str(size, 16)),
						("value", &value),
					],
				);
				writer.newline();
			}
			writer.endtag("trackEntry");
			writer.newline();
		}
		Ok(())
	}

	fn read_xml(element: &Element) -> Result<Self> {
		let mut data = TrackData::default();
		for child in element.elements() {
			if child.name != "trackEntry" {
				continue;
			}
			let track = fixed_attr(child, "value")?;
			let name_index = int_value(child, "nameIndex")?;
			let mut entry = TrackTableEntry {
				name_index: Some(name_index),
				values: Vec::new(),
			};
			for t in child.elements() {
				if t.name != "track" {
					continue;
				}
				entry.insert(fixed_attr(t, "size")?, int_value(t, "value")?);
			}
			data.insert(track, entry);
		}
		Ok(data)
	}
}

fn required<'a>(element: &'a Element, name: &str) -> Result<&'a str> {
	element
		.attr(name)
		.ok_or_else(|| Error::ttx(format!("missing '{name}' on <{}>", element.name)))
}

/// `strToFixedToFloat(attrs[name], 16)`.
fn fixed_attr(element: &Element, name: &str) -> Result<f64> {
	let text = required(element, name)?;
	str_to_fixed_to_float(text, 16)
		.ok_or_else(|| Error::value(format!("could not convert string to float: {text:?}")))
}

/// `safeEval(attrs[name])` of an integer.
fn int_value(element: &Element, name: &str) -> Result<i64> {
	let v = eval(required(element, name)?)?
		.as_int()
		.ok_or_else(|| Error::value(format!("invalid value for {name}")))?;
	fit(v, name)
}

/// A `safeEval` value that is a number.
fn number(element: &Element) -> Result<Value> {
	let lit = eval(required(element, "value")?)?;
	if let Some(i) = lit.as_int() {
		return Ok(Value::Int(fit(i, "value")?));
	}
	lit.as_float()
		.map(Value::Float)
		.ok_or_else(|| Error::value("invalid number"))
}

fn value_as_float(v: &Value) -> f64 {
	match v {
		Value::Int(i) => *i as f64,
		Value::Float(f) => *f,
		Value::Bytes(_) => f64::NAN,
	}
}

/// Tracking table (`table__t_r_a_k`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Trak {
	/// The table version (`version`): a float when decompiled, as written in
	/// TTX otherwise. Required to compile or dump.
	pub version: Option<Value>,
	/// The table format (`format`). Required to compile or dump.
	pub format: Option<i64>,
	/// The horizontal tracks (`horizData`).
	pub horiz_data: TrackData,
	/// The vertical tracks (`vertData`).
	pub vert_data: TrackData,
	/// The offset of the horizontal track data, as read or last compiled.
	pub horiz_offset: u16,
	/// The offset of the vertical track data, as read or last compiled.
	pub vert_offset: u16,
}

impl TableType for Trak {
	const TAG: Tag = Tag::new(b"trak");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let (header, _) = TrakHeader::unpack2(data)?;
		let mut table = Trak {
			version: Some(Value::Float(header.version)),
			format: Some(i64::from(header.format)),
			horiz_offset: header.horiz_offset,
			vert_offset: header.vert_offset,
			..Default::default()
		};
		if header.horiz_offset != 0 {
			table.horiz_data = TrackData::decompile(data, header.horiz_offset as usize)?;
		}
		if header.vert_offset != 0 {
			table.vert_data = TrackData::decompile(data, header.vert_offset as usize)?;
		}
		Ok(table)
	}
}

fn missing(name: &str) -> Error {
	Error::ttlib(format!(
		"AttributeError: 'table__t_r_a_k' object has no attribute '{name}'"
	))
}

impl Table for Trak {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let mut data_list = Vec::new();
		let mut offset = 12;
		let mut offsets = [0u16; 2];
		for (i, track_data) in [&self.horiz_data, &self.vert_data].into_iter().enumerate() {
			if track_data.is_empty() {
				continue;
			}
			// Track data must be longword aligned.
			let aligned = (offset + 3) & !3;
			data_list.resize(data_list.len() + (aligned - offset), 0u8);
			offset = aligned;
			offsets[i] = fit(offset, if i == 0 { "horizOffset" } else { "vertOffset" })?;
			let data = track_data.compile(offset)?;
			offset += data.len();
			data_list.extend(data);
		}
		self.horiz_offset = offsets[0];
		self.vert_offset = offsets[1];
		let version = self.version.as_ref().ok_or_else(|| missing("version"))?;
		let format = self.format.ok_or_else(|| missing("format"))?;
		let mut out = Vec::new();
		TrakHeader {
			version: value_as_float(version),
			format: fit(format, "format")?,
			horiz_offset: offsets[0],
			vert_offset: offsets[1],
			reserved: 0,
		}
		.pack(&mut out)?;
		out.extend(data_list);
		Ok(out)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let version = self.version.as_ref().ok_or_else(|| missing("version"))?;
		let format = self.format.ok_or_else(|| missing("format"))?;
		writer.simpletag("version", &[("value", version)]);
		writer.newline();
		writer.simpletag("format", &[("value", &format)]);
		writer.newline();
		for (name, data) in [
			("horizData", &self.horiz_data),
			("vertData", &self.vert_data),
		] {
			writer.begintag(name, &[]);
			writer.newline();
			data.to_xml(writer, font)?;
			writer.endtag(name);
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"version" => self.version = Some(number(element)?),
			"format" => {
				let v = eval(required(element, "value")?)?
					.as_int()
					.ok_or_else(|| Error::value("invalid value for format"))?;
				self.format = Some(fit(v, "format")?);
			}
			"horizData" => self.horiz_data = TrackData::read_xml(element)?,
			"vertData" => self.vert_data = TrackData::read_xml(element)?,
			_ => {}
		}
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn formats_match_upstream() {
		use crate::sstruct::tests::check;
		check(TrakHeader::FORMAT_NAME, TrakHeader::FIELDS);
		check(TrackDataHeader::FORMAT_NAME, TrackDataHeader::FIELDS);
		check(TrackEntryRecord::FORMAT_NAME, TrackEntryRecord::FIELDS);
	}
}
