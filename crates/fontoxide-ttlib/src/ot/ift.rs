//! Incremental Font Transfer patch maps (`IFT `/`IFTX`): the mapping entries
//! (`MappingEntriesConverter`) and the entry ID string data (`EntryIdStringData`).

use fontoxide_misc::Tag;
use fontoxide_misc::fixed_tools::{fixed_to_float, float_to_fixed};
use fontoxide_misc::ift_sparse_bit_set;
use fontoxide_misc::py::{float_repr, parse_float};
use fontoxide_misc::text_tools::{de_hex_str, parse_int_radix, safe_eval_int};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use super::conv::parse_tag;
use super::table::{OtTable, XmlCtx};
use super::{OtReader, OtWriter, StateValue};
use crate::{Error, Font, Result};

/// `MappingEntryFormat` bits.
pub mod format {
	/// Features and design space segments follow (`HAS_SUBSET_DEF`).
	pub const HAS_SUBSET_DEF: i64 = 0x01;
	/// Child entry indices follow (`HAS_CHILD_ENTRIES`).
	pub const HAS_CHILD_ENTRIES: i64 = 0x02;
	/// Delta-encoded entry IDs follow (`HAS_ENTRY_ID`).
	pub const HAS_ENTRY_ID: i64 = 0x04;
	/// A patch format follows (`HAS_PATCH_FORMAT`).
	pub const HAS_PATCH_FORMAT: i64 = 0x08;
	/// The code point bias size; any of them means a code point set follows
	/// (`CODE_POINT_BIAS_BITS`).
	pub const CODE_POINT_BIAS_BITS: i64 = 0x30;
}

/// A design space segment of a mapping entry.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignSpaceSegment {
	/// The axis tag.
	pub tag: Tag,
	/// The start of the range.
	pub start: f64,
	/// The end of the range.
	pub end: f64,
}

/// A mapping entry of a patch map. fontTools keeps it as a `dict`; the fields
/// are `None` where the key is missing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MappingEntry {
	/// `formatFlags` ([`format`] bits).
	pub format_flags: i64,
	/// `featureTags`.
	pub feature_tags: Option<Vec<Tag>>,
	/// `designSpaceSegments`.
	pub design_space_segments: Option<Vec<DesignSpaceSegment>>,
	/// `childEntryConjunctive`.
	pub child_entry_conjunctive: Option<bool>,
	/// `childEntryIndices`.
	pub child_entry_indices: Option<Vec<i64>>,
	/// `entryIds`.
	pub entry_ids: Option<Vec<i64>>,
	/// `patchFormat`.
	pub patch_format: Option<i64>,
	/// `codePointsBias`.
	pub code_points_bias: Option<i64>,
	/// `codePoints`, sorted when read from a font.
	pub code_points: Option<Vec<i64>>,
}

/// Reads the `NumEntries` mapping entries (`MappingEntriesConverter.read`).
pub fn read_mapping_entries(r: &mut OtReader<'_>) -> Result<Vec<MappingEntry>> {
	let count = r
		.get("NumEntries")
		.and_then(StateValue::as_int)
		.ok_or_else(|| Error::ttlib("KeyError: 'NumEntries'"))?;
	let mut entries = Vec::new();
	let mut last_id: i64 = -1;
	for _ in 0..count {
		let flags = r.read_u8()? as i64;
		let mut entry = MappingEntry {
			format_flags: flags,
			..Default::default()
		};
		if flags & format::HAS_SUBSET_DEF != 0 {
			let n = r.read_u8()?;
			let mut tags = Vec::with_capacity(n as usize);
			for _ in 0..n {
				tags.push(r.read_tag()?);
			}
			entry.feature_tags = Some(tags);
			let n = r.read_u16()?;
			let mut segments = Vec::new();
			for _ in 0..n {
				let tag = r.read_tag()?;
				let start = fixed_to_float(r.read_i32()? as i64, 16);
				let end = fixed_to_float(r.read_i32()? as i64, 16);
				segments.push(DesignSpaceSegment { tag, start, end });
			}
			entry.design_space_segments = Some(segments);
		}
		if flags & format::HAS_CHILD_ENTRIES != 0 {
			let mode_and_count = r.read_u8()?;
			entry.child_entry_conjunctive = Some(mode_and_count & 0x80 != 0);
			let mut indices = Vec::new();
			for _ in 0..mode_and_count & 0x7F {
				indices.push(r.read_u24()? as i64);
			}
			entry.child_entry_indices = Some(indices);
		}
		let mut ids = Vec::new();
		if flags & format::HAS_ENTRY_ID != 0 {
			loop {
				// The low bit tells whether more IDs follow; the rest is the
				// delta from the previous ID, minus one.
				let encoded = r.read_i24()? as i64;
				last_id += 1 + (encoded >> 1);
				ids.push(last_id);
				if encoded & 1 == 0 {
					break;
				}
			}
		} else {
			last_id += 1;
			ids.push(last_id);
		}
		entry.entry_ids = Some(ids);
		if flags & format::HAS_PATCH_FORMAT != 0 {
			entry.patch_format = Some(r.read_u8()? as i64);
		}
		if flags & format::CODE_POINT_BIAS_BITS != 0 {
			let bias = match flags & format::CODE_POINT_BIAS_BITS {
				0x30 => r.read_u24()? as i64,
				0x20 => r.read_u16()? as i64,
				_ => 0,
			};
			entry.code_points_bias = Some(bias);
			let data = r.data().get(r.pos()..).unwrap_or_default();
			let (points, consumed) = ift_sparse_bit_set::decode(data, bias, 0x10FFFF)
				.map_err(|e| Error::ttlib(format!("SparseBitSetDecodeError: {e}")))?;
			r.advance(consumed);
			entry.code_points = Some(points.into_iter().map(i64::from).collect());
		}
		entries.push(entry);
	}
	Ok(entries)
}

fn fit<T: TryFrom<i64>>(value: i64, format: &str) -> Result<T> {
	T::try_from(value)
		.map_err(|_| Error::Struct(format!("argument out of range for '{format}' format")))
}

fn write_u24(w: &mut OtWriter<'_>, value: i64) -> Result<()> {
	w.write_u24(fit(value, "L")?)
}

/// Writes mapping entries (`MappingEntriesConverter.write`).
pub fn write_mapping_entries(w: &mut OtWriter<'_>, entries: &[MappingEntry]) -> Result<()> {
	let mut last_id: i64 = -1;
	for entry in entries {
		let flags = entry.format_flags;
		w.write_u8(fit(flags, "B")?);
		if flags & format::HAS_SUBSET_DEF != 0 {
			let tags = entry.feature_tags.as_deref().unwrap_or_default();
			w.write_u8(fit(tags.len() as i64, "B")?);
			for &tag in tags {
				w.write_tag(tag);
			}
			let segments = entry.design_space_segments.as_deref().unwrap_or_default();
			w.write_u16(fit(segments.len() as i64, "H")?);
			for s in segments {
				w.write_tag(s.tag);
				w.write_i32(fit(float_to_fixed(s.start, 16), "l")?);
				w.write_i32(fit(float_to_fixed(s.end, 16), "l")?);
			}
		}
		if flags & format::HAS_CHILD_ENTRIES != 0 {
			let indices = entry.child_entry_indices.as_deref().unwrap_or_default();
			let conjunctive = entry.child_entry_conjunctive.unwrap_or(false);
			w.write_u8((indices.len() & 0x7F) as u8 | if conjunctive { 0x80 } else { 0 });
			for &i in indices {
				write_u24(w, i)?;
			}
		}
		if flags & format::HAS_ENTRY_ID != 0 {
			// UPSTREAM-BUG(B7): with no entry IDs, nothing is written, although a
			// reader always expects at least one.
			let ids = entry.entry_ids.as_deref().unwrap_or_default();
			for (i, &id) in ids.iter().enumerate() {
				let has_more = i64::from(i + 1 < ids.len());
				let encoded = ((id - last_id - 1) << 1) | has_more;
				write_u24(w, encoded & 0xFF_FFFF)?;
				last_id = id;
			}
		} else {
			last_id += 1;
		}
		if flags & format::HAS_PATCH_FORMAT != 0 {
			w.write_u8(fit(entry.patch_format.unwrap_or(0), "B")?);
		}
		if flags & format::CODE_POINT_BIAS_BITS != 0 {
			let bias = entry.code_points_bias.unwrap_or(0);
			match flags & format::CODE_POINT_BIAS_BITS {
				0x30 => write_u24(w, bias)?,
				0x20 => w.write_u16(fit(bias, "H")?),
				_ => {}
			}
			let points = entry.code_points.as_deref().unwrap_or_default();
			let values = points
				.iter()
				.filter(|&&c| c >= bias)
				.map(|&c| c - bias)
				.map(|v| u64::try_from(v).map_err(|_| Error::value("negative value")))
				.collect::<Result<Vec<_>>>()?;
			let data = ift_sparse_bit_set::encode(values)
				.map_err(|e| Error::value(format!("ValueError: {e}")))?;
			w.write_data(&data);
		}
	}
	Ok(())
}

/// Writes mapping entries as TTX (`MappingEntriesConverter.xmlWrite`): the
/// entries go directly into the enclosing table.
pub fn xml_mapping_entries(xw: &mut XmlWriter, entries: &[MappingEntry]) {
	for entry in entries {
		xw.begintag("entry", &[("formatFlags", &entry.format_flags)]);
		xw.newline();
		for id in entry.entry_ids.iter().flatten() {
			xw.simpletag("entryId", &[("value", id)]);
			xw.newline();
		}
		for tag in entry.feature_tags.iter().flatten() {
			xw.simpletag("featureTag", &[("value", &tag.to_string())]);
			xw.newline();
		}
		for s in entry.design_space_segments.iter().flatten() {
			xw.simpletag(
				"designSpaceSegment",
				&[
					("tag", &s.tag.to_string()),
					("start", &float_repr(s.start)),
					("end", &float_repr(s.end)),
				],
			);
			xw.newline();
		}
		if let Some(indices) = &entry.child_entry_indices {
			let conjunctive = i64::from(entry.child_entry_conjunctive.unwrap_or(false));
			xw.simpletag("childEntryConjunctive", &[("value", &conjunctive)]);
			xw.newline();
			for i in indices {
				xw.simpletag("childEntry", &[("index", i)]);
				xw.newline();
			}
		}
		if let Some(f) = entry.patch_format {
			xw.simpletag("patchFormat", &[("value", &f)]);
			xw.newline();
		}
		if let Some(points) = &entry.code_points {
			let bias = entry.code_points_bias.unwrap_or(0);
			xw.simpletag("codePointsBias", &[("value", &bias)]);
			xw.newline();
			let text = points
				.iter()
				.map(i64::to_string)
				.collect::<Vec<_>>()
				.join(" ");
			xw.simpletag("codePoints", &[("value", &text)]);
			xw.newline();
		}
		xw.endtag("entry");
		xw.newline();
	}
}

fn attr<'a>(e: &'a Element, name: &str) -> Result<&'a str> {
	e.attr(name)
		.ok_or_else(|| Error::ttx(format!("KeyError: {name:?}")))
}

fn eval_int(s: &str) -> Result<i64> {
	safe_eval_int(s)
		.and_then(|v| i64::try_from(v).ok())
		.ok_or_else(|| Error::value(format!("malformed node or string: {s:?}")))
}

/// Reads mapping entries from the `entry` children of `element`
/// (`MappingEntriesConverter.xmlRead`).
pub fn read_mapping_entries_xml(element: &Element) -> Result<Vec<MappingEntry>> {
	let mut entries = Vec::new();
	for e in element.elements().filter(|e| e.name == "entry") {
		let mut entry = MappingEntry {
			format_flags: eval_int(e.attr("formatFlags").unwrap_or("0"))?,
			..Default::default()
		};
		for c in e.elements() {
			match c.name.as_str() {
				"entryId" => entry
					.entry_ids
					.get_or_insert_with(Vec::new)
					.push(eval_int(attr(c, "value")?)?),
				// LENIENT(L8): fontTools keeps the tag text and fails when writing
				// one that is not four characters long.
				"featureTag" => entry
					.feature_tags
					.get_or_insert_with(Vec::new)
					.push(parse_tag(attr(c, "value")?)?),
				"designSpaceSegment" => {
					let float = |name: &str| -> Result<f64> {
						let s = attr(c, name)?;
						parse_float(s).ok_or_else(|| {
							Error::value(format!("could not convert string to float: {s:?}"))
						})
					};
					let segment = DesignSpaceSegment {
						tag: parse_tag(attr(c, "tag")?)?,
						start: float("start")?,
						end: float("end")?,
					};
					entry
						.design_space_segments
						.get_or_insert_with(Vec::new)
						.push(segment);
				}
				"childEntryConjunctive" => {
					entry.child_entry_conjunctive = Some(eval_int(attr(c, "value")?)? != 0)
				}
				"childEntry" => entry
					.child_entry_indices
					.get_or_insert_with(Vec::new)
					.push(eval_int(attr(c, "index")?)?),
				"patchFormat" => entry.patch_format = Some(eval_int(attr(c, "value")?)?),
				"codePointsBias" => entry.code_points_bias = Some(eval_int(attr(c, "value")?)?),
				"codePoints" => {
					entry.code_points = Some(
						attr(c, "value")?
							.split_whitespace()
							.map(|s| {
								parse_int_radix(s, 10)
									.and_then(|v| i64::try_from(v).ok())
									.ok_or_else(|| {
										Error::value(format!(
											"invalid literal for int() with base 10: {s:?}"
										))
									})
							})
							.collect::<Result<_>>()?,
					)
				}
				_ => {}
			}
		}
		entries.push(entry);
	}
	Ok(entries)
}

/// The entry ID string data of a patch map (`EntryIdStringData`): the bytes
/// from its offset to the end of the table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryIdStringData {
	/// The raw bytes (`data`); `None` if never set.
	pub data: Option<Vec<u8>>,
}

impl OtTable for EntryIdStringData {
	fn class_name(&self) -> &'static str {
		"EntryIdStringData"
	}

	fn decompile(r: &mut OtReader<'_>, _font: &mut Font) -> Result<Self> {
		Ok(EntryIdStringData {
			data: Some(r.data().get(r.pos()..).unwrap_or_default().to_vec()),
		})
	}

	fn compile(&self, w: &mut OtWriter<'_>, _font: &mut Font) -> Result<()> {
		let data = self.data.as_deref().ok_or_else(|| {
			Error::ttlib("AttributeError: 'EntryIdStringData' object has no attribute 'data'")
		})?;
		w.write_data(data);
		Ok(())
	}

	fn to_xml2(&self, xw: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		let data = self.data.as_deref().ok_or_else(|| {
			Error::ttlib("AttributeError: 'EntryIdStringData' object has no attribute 'data'")
		})?;
		let hex: String = data.iter().map(|b| format!("{b:02x}")).collect();
		xw.simpletag("data", &[("value", &hex)]);
		xw.newline();
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font, _ctx: &mut XmlCtx) -> Result<()> {
		if element.name == "data" {
			let s = attr(element, "value")?;
			// `bytes.fromhex` allows whitespace between bytes, not within one.
			let ok = s.split_whitespace().all(|p| p.len() % 2 == 0);
			let data = de_hex_str(s)
				.filter(|_| ok)
				.ok_or_else(|| Error::value("non-hexadecimal number found in fromhex() arg"))?;
			self.data = Some(data);
		}
		Ok(())
	}

	fn populate_defaults(&mut self, _ctx: &mut XmlCtx) -> Result<()> {
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ot::WriterGraph;

	#[test]
	fn entries_round_trip() -> Result<()> {
		let entries = vec![
			MappingEntry {
				format_flags: 0x39,
				feature_tags: Some(vec![Tag::new(b"liga")]),
				design_space_segments: Some(vec![DesignSpaceSegment {
					tag: Tag::new(b"wght"),
					start: 100.0,
					end: 900.5,
				}]),
				patch_format: Some(2),
				code_points_bias: Some(0x10000),
				code_points: Some(vec![0x10041, 0x10100]),
				..Default::default()
			},
			MappingEntry {
				format_flags: 0x06,
				entry_ids: Some(vec![5, 3, 10]),
				child_entry_conjunctive: Some(true),
				child_entry_indices: Some(vec![0]),
				..Default::default()
			},
		];
		let mut graph = WriterGraph::new(Tag::new(b"IFT "));
		write_mapping_entries(&mut graph.root(), &entries)?;
		let data = graph.get_all_data()?;
		let mut r = OtReader::new(&data, Tag::new(b"IFT "));
		r.set("NumEntries", StateValue::Int(2));
		let read = read_mapping_entries(&mut r)?;
		assert_eq!(r.pos(), data.len());
		assert_eq!(read[0].entry_ids, Some(vec![0]));
		assert_eq!(read[0].code_points, entries[0].code_points);
		assert_eq!(
			read[0].design_space_segments,
			entries[0].design_space_segments
		);
		assert_eq!(read[1].entry_ids, entries[1].entry_ids);
		assert_eq!(read[1].child_entry_indices, entries[1].child_entry_indices);
		Ok(())
	}
}
