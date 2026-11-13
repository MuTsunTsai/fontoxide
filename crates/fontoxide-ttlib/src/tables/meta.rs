//! `meta`: the metadata table (`fontTools.ttLib.tables._m_e_t_a`).

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::fit;
use crate::sstruct::{Bytes, U32};
use crate::table::{Table, TableType, TableXml, read_hex};
use crate::{Error, Font, Result};

crate::sstruct! {
	/// The header (`META_HEADER_FORMAT`).
	pub struct MetaHeader = "META_HEADER_FORMAT" {
		/// The table version (1).
		pub version: u32 = "version" as U32,
		/// Flags (0).
		pub flags: u32 = "flags" as U32,
		/// The offset of the data.
		pub data_offset: u32 = "dataOffset" as U32,
		/// The number of data maps.
		pub num_data_maps: u32 = "numDataMaps" as U32,
	}
}

crate::sstruct! {
	/// A data map record (`DATA_MAP_FORMAT`).
	pub struct DataMap = "DATA_MAP_FORMAT" {
		/// The metadata tag.
		pub tag: [u8; 4] = "tag" as Bytes<4>,
		/// The offset of the data.
		pub data_offset: u32 = "dataOffset" as U32,
		/// The length of the data.
		pub data_length: u32 = "dataLength" as U32,
	}
}

/// A metadata value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaValue {
	/// Text, for the `dlng` and `slng` tags (stored as UTF-8).
	Text(String),
	/// Binary data, for the other tags.
	Binary(Vec<u8>),
}

/// Whether the data of a tag is text.
fn is_text_tag(tag: &str) -> bool {
	tag == "dlng" || tag == "slng"
}

/// Metadata table (`table__m_e_t_a`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/meta>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Meta {
	/// The metadata by tag (`data`). Tags are sorted when compiling.
	pub data: BTreeMap<String, MetaValue>,
}

impl TableType for Meta {
	const TAG: Tag = Tag::new(b"meta");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let (header, _) = MetaHeader::unpack2(data)?;
		if header.version != 1 {
			return Err(Error::ttlib(format!(
				"unsupported 'meta' version {}",
				header.version
			)));
		}
		let mut table = Meta::default();
		for i in 0..header.num_data_maps as usize {
			let start = (16 + i * 12).min(data.len());
			let end = (start + 12).min(data.len());
			let (map, _) = DataMap::unpack2(&data[start..end])?;
			// sstruct decodes ASCII byte strings.
			// Other tags would stay `bytes` in fontTools; fontoxide does not
			// represent them and fails instead.
			let tag = if map.tag.is_ascii() {
				map.tag.iter().map(|&b| b as char).collect::<String>()
			} else {
				return Err(Error::ttlib(format!(
					"unsupported 'meta' tag {}",
					fontoxide_misc::py::bytes_repr(&map.tag)
				)));
			};
			let offset = (map.data_offset as usize).min(data.len());
			let end = offset
				.saturating_add(map.data_length as usize)
				.min(data.len());
			let bytes = data[offset..end].to_vec();
			let value = if is_text_tag(&tag) {
				MetaValue::Text(
					String::from_utf8(bytes)
						.map_err(|e| Error::value(format!("UnicodeDecodeError: {e}")))?,
				)
			} else {
				MetaValue::Binary(bytes)
			};
			table.data.insert(tag, value);
		}
		Ok(table)
	}
}

impl Table for Meta {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let mut data_offset = 16 + self.data.len() * 12;
		let mut out = Vec::new();
		MetaHeader {
			version: 1,
			flags: 0,
			data_offset: fit(data_offset, "dataOffset")?,
			num_data_maps: fit(self.data.len(), "numDataMaps")?,
		}
		.pack(&mut out)?;
		let mut blocks = Vec::new();
		for (tag, value) in &self.data {
			let bytes = match (is_text_tag(tag), value) {
				(true, MetaValue::Text(s)) => s.as_bytes(),
				(true, MetaValue::Binary(_)) => {
					return Err(Error::ttlib(
						"AttributeError: 'bytes' object has no attribute 'encode'",
					));
				}
				(false, MetaValue::Binary(b)) => b.as_slice(),
				(false, MetaValue::Text(_)) => {
					return Err(Error::value(
						"TypeError: sequence item: expected a bytes-like object, str found",
					));
				}
			};
			// struct.pack("4s") of the ASCII-encoded tag truncates or pads it.
			if !tag.is_ascii() {
				return Err(Error::value(format!(
					"UnicodeEncodeError: 'ascii' codec can't encode tag {tag:?}"
				)));
			}
			let mut tag_bytes = [0u8; 4];
			let n = tag.len().min(4);
			tag_bytes[..n].copy_from_slice(&tag.as_bytes()[..n]);
			DataMap {
				tag: tag_bytes,
				data_offset: fit(data_offset, "dataOffset")?,
				data_length: fit(bytes.len(), "dataLength")?,
			}
			.pack(&mut out)?;
			blocks.push(bytes);
			data_offset += bytes.len();
		}
		for block in blocks {
			out.extend_from_slice(block);
		}
		Ok(out)
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Ok(())
	}
}

impl TableXml for Meta {
	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		for (tag, value) in &self.data {
			match (is_text_tag(tag), value) {
				(true, MetaValue::Text(text)) => {
					writer.begintag("text", &[("tag", tag)]);
					writer.newline();
					writer.write(text);
					writer.newline();
					writer.endtag("text");
					writer.newline();
				}
				(false, MetaValue::Binary(data)) => {
					writer.begintag("hexdata", &[("tag", tag)]);
					writer.newline();
					let (Some(&min), Some(&max)) = (data.iter().min(), data.iter().max()) else {
						return Err(Error::value("min() iterable argument is empty"));
					};
					if min >= 0x20 && max <= 0x7E {
						let ascii: String = data.iter().map(|&b| b as char).collect();
						writer.comment(&format!("ascii: {ascii}"));
						writer.newline();
					}
					writer.dumphex(data);
					writer.endtag("hexdata");
					writer.newline();
				}
				_ => {
					return Err(Error::value(format!(
						"unexpected type of 'meta' data for tag {tag:?}"
					)));
				}
			}
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		let tag = || {
			element
				.attr("tag")
				.ok_or_else(|| Error::ttx(format!("missing 'tag' on <{}>", element.name)))
		};
		match element.name.as_str() {
			"hexdata" => {
				let data = read_hex(element)?;
				self.data.insert(tag()?.to_owned(), MetaValue::Binary(data));
			}
			"text" if is_text_tag(tag()?) => {
				let text = fontoxide_misc::py::strip(&element.text()).to_owned();
				self.data.insert(tag()?.to_owned(), MetaValue::Text(text));
			}
			name => return Err(Error::ttlib(format!("can't handle '{name}' element"))),
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn formats_match_upstream() {
		use crate::sstruct::tests::check;
		check(MetaHeader::FORMAT_NAME, MetaHeader::FIELDS);
		check(DataMap::FORMAT_NAME, DataMap::FIELDS);
	}
}
