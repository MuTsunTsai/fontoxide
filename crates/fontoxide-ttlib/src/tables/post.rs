//! `post`: PostScript table (`fontTools.ttLib.tables._p_o_s_t`).

use std::collections::{BTreeMap, HashMap, HashSet};

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes};
use crate::generated::standard_glyph_order::STANDARD_GLYPH_ORDER;
use crate::sstruct::{F16Dot16, I16, U32, eval};
use crate::table::{Table, TableType, read_hex};
use crate::{Error, Font, Result};

crate::sstruct! {
	/// The fixed part of the `post` table (`postFormat`).
	pub struct PostHeader = "postFormat" {
		/// 1.0, 2.0, 3.0 or 4.0.
		pub format_type: f64 = "formatType" as F16Dot16,
		/// Italic angle in degrees.
		pub italic_angle: f64 = "italicAngle" as F16Dot16,
		/// Underline position.
		pub underline_position: i16 = "underlinePosition" as I16,
		/// Underline thickness.
		pub underline_thickness: i16 = "underlineThickness" as I16,
		/// Non-zero for monospaced fonts.
		pub is_fixed_pitch: u32 = "isFixedPitch" as U32,
		/// Minimum memory if the font is downloaded as Type 42.
		pub min_mem_type42: u32 = "minMemType42" as U32,
		/// Maximum memory if the font is downloaded as Type 42.
		pub max_mem_type42: u32 = "maxMemType42" as U32,
		/// Minimum memory if the font is downloaded as Type 1.
		pub min_mem_type1: u32 = "minMemType1" as U32,
		/// Maximum memory if the font is downloaded as Type 1.
		pub max_mem_type1: u32 = "maxMemType1" as U32,
	}
}

/// PostScript table (`table__p_o_s_t`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/post>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Post {
	/// The fixed fields.
	pub header: PostHeader,
	/// Glyph names that differ from the names in the `post` table, because those
	/// were empty or not unique (`mapping`; formats 2 and 4).
	pub mapping: Option<BTreeMap<String, String>>,
	/// The names not in the standard Macintosh glyph order (`extraNames`; format
	/// 2).
	pub extra_names: Option<Vec<String>>,
	/// Raw data from TTX `<hexdata>` (`data`). Never set when decompiling.
	pub data: Option<Vec<u8>>,
	/// The glyph order found while decompiling, until the font takes it.
	/// `Some(None)` means the table has no glyph names (format 3).
	glyph_order: Option<Option<Vec<String>>>,
}

impl std::ops::Deref for Post {
	type Target = PostHeader;
	fn deref(&self) -> &PostHeader {
		&self.header
	}
}

impl std::ops::DerefMut for Post {
	fn deref_mut(&mut self) -> &mut PostHeader {
		&mut self.header
	}
}

impl Post {
	/// Hands the glyph names found while decompiling to the font (`getGlyphOrder`).
	/// Only [`Font::glyph_order`] should call this.
	pub(crate) fn take_glyph_order(&mut self) -> Result<Option<Vec<String>>> {
		self.glyph_order
			.take()
			.ok_or_else(|| Error::ttlib("illegal use of getGlyphOrder()"))
	}

	fn decode_format_2_0(&mut self, data: &[u8], glyph_count: usize) -> Result<()> {
		let mut r = Reader::new(data);
		let mut num_glyphs = r.u16()? as usize;
		if num_glyphs > glyph_count {
			// Assume the numGlyphs field is bogus, so sync with maxp.
			num_glyphs = glyph_count;
		}
		// fontTools reads the indices with array.frombytes(), which rejects an odd
		// number of bytes, and then fails with an IndexError if there are too few.
		if r.remaining() < 2 * num_glyphs {
			return Err(Error::ttlib(if r.remaining() % 2 == 1 {
				"bytes length not a multiple of item size"
			} else {
				"array index out of range"
			}));
		}
		let indices = r.u16_array(num_glyphs)?;
		let string_data = data.get(2 + 2 * num_glyphs..).unwrap_or_default();
		let max_index = indices.iter().copied().max().unwrap_or(0) as i64;
		let extra_names = unpack_pstrings(string_data, (max_index - 257).max(0) as usize);
		let mut glyph_order = vec![String::new(); glyph_count];
		for (gid, &index) in indices.iter().enumerate() {
			glyph_order[gid] = if index > 257 {
				extra_names
					.get(index as usize - 258)
					.cloned()
					.unwrap_or_default()
			} else {
				STANDARD_GLYPH_ORDER[index as usize].to_owned()
			};
		}
		self.extra_names = Some(extra_names);
		self.build_ps_name_mapping(glyph_order, glyph_count);
		Ok(())
	}

	fn decode_format_4_0(&mut self, data: &[u8], glyph_count: usize) -> Result<()> {
		if data.len() % 2 == 1 {
			return Err(Error::value("bytes length not a multiple of item size"));
		}
		let indices: Vec<u16> = data
			.as_chunks::<2>()
			.0
			.iter()
			.map(|&c| u16::from_be_bytes(c))
			.collect();
		let mut glyph_order = vec![String::new(); glyph_count];
		for (i, &index) in indices.iter().take(glyph_count).enumerate() {
			glyph_order[i] = if index == 0xFFFF {
				String::new()
			} else if let Some(name) = fontoxide_agl::uv2agl(index as u32) {
				name.to_owned()
			} else {
				format!("uni{index:04X}")
			};
		}
		self.build_ps_name_mapping(glyph_order, glyph_count);
		Ok(())
	}

	/// Makes the names unique, recording the original names (`build_psNameMapping`).
	fn build_ps_name_mapping(&mut self, mut glyph_order: Vec<String>, glyph_count: usize) {
		let mut mapping = BTreeMap::new();
		let mut all_names: HashMap<String, usize> = HashMap::new();
		let glyph_order_names: HashSet<String> = glyph_order.iter().cloned().collect();
		for (i, slot) in glyph_order.iter_mut().enumerate().take(glyph_count) {
			let ps_name = slot.clone();
			let mut glyph_name = if ps_name.is_empty() {
				crate::glyph_order::synthesized_name(i as u32)
			} else {
				ps_name.clone()
			};
			if let Some(&count) = all_names.get(&glyph_name) {
				// Make up a new glyph name that's unique.
				let mut n = count;
				while glyph_order_names.contains(&format!("{glyph_name}.{n}")) {
					n += 1;
				}
				all_names.insert(glyph_name.clone(), n + 1);
				glyph_name = format!("{glyph_name}.{n}");
			}
			all_names.insert(glyph_name.clone(), 1);
			if glyph_name != ps_name {
				*slot = glyph_name.clone();
				mapping.insert(glyph_name, ps_name);
			}
		}
		self.mapping = Some(mapping);
		self.glyph_order = Some(Some(glyph_order));
	}

	fn encode_format_2_0(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		let num_glyphs = font.glyph_count()?;
		let glyph_order = font.glyph_order()?;
		if glyph_order.len() != num_glyphs {
			return Err(Error::Assertion("len(glyphOrder) == numGlyphs".into()));
		}
		let standard: HashMap<&str, usize> = STANDARD_GLYPH_ORDER
			.iter()
			.enumerate()
			.rev()
			.map(|(i, n)| (*n, i))
			.collect();
		let mut extra_names: Vec<String> = self
			.extra_names
			.as_deref()
			.ok_or_else(|| Error::ttlib("'post' table has no 'extraNames'"))?
			.iter()
			.filter(|n| !standard.contains_key(n.as_str()))
			.cloned()
			.collect();
		let mut extra_dict: HashMap<String, usize> = HashMap::new();
		for (i, name) in extra_names.iter().enumerate() {
			extra_dict.insert(name.clone(), i);
		}
		let mapping = self
			.mapping
			.as_ref()
			.ok_or_else(|| Error::ttlib("'post' table has no 'mapping'"))?;
		let mut indices = Vec::with_capacity(num_glyphs);
		for glyph_name in glyph_order.names() {
			let ps_name = mapping.get(glyph_name).unwrap_or(glyph_name);
			let index = if let Some(&i) = extra_dict.get(ps_name) {
				258 + i
			} else if let Some(&i) = standard.get(ps_name.as_str()) {
				i
			} else {
				let i = extra_names.len();
				extra_dict.insert(ps_name.clone(), i);
				extra_names.push(ps_name.clone());
				258 + i
			};
			indices.push(crate::binary::fit::<u16, _>(index, "glyphNameIndex")?);
		}
		let mut data = Vec::new();
		data.put_u16(crate::binary::fit(num_glyphs, "numGlyphs")?);
		for i in indices {
			data.put_u16(i);
		}
		data.extend(pack_pstrings(&extra_names)?);
		self.extra_names = Some(extra_names);
		Ok(data)
	}

	fn encode_format_4_0(&self, font: &mut Font) -> Result<Vec<u8>> {
		let num_glyphs = font.glyph_count()?;
		let glyph_order = font.glyph_order()?;
		if glyph_order.len() != num_glyphs {
			return Err(Error::Assertion("len(glyphOrder) == numGlyphs".into()));
		}
		let mut data = Vec::with_capacity(2 * num_glyphs);
		for name in glyph_order.names() {
			let name = name.split('#').next().unwrap_or_default();
			let index = if let Some(uv) = fontoxide_agl::agl2uv(name) {
				crate::binary::fit(uv, "index")?
			} else if name.chars().count() == 7 && name.starts_with("uni") {
				let hex = &name[3..];
				let v = u32::from_str_radix(hex.trim(), 16).map_err(|_| {
					Error::value(format!("invalid literal for int() with base 16: {hex:?}"))
				})?;
				crate::binary::fit(v, "index")?
			} else {
				0xFFFF
			};
			data.put_u16(index);
		}
		Ok(data)
	}
}

/// Reads `n` Pascal strings; missing ones are empty (`unpackPStrings`).
fn unpack_pstrings(data: &[u8], n: usize) -> Vec<String> {
	let mut strings = Vec::with_capacity(n);
	let mut index = 0usize;
	let len = data.len();
	for _ in 0..n {
		let length = if len <= index {
			0
		} else {
			data[index] as usize
		};
		index += 1;
		let name = if (len as isize) < (index + length) as isize {
			String::new()
		} else {
			fontoxide_misc::py::latin1_decode(&data[index..index + length])
		};
		strings.push(name);
		index += length;
	}
	if index < len {
		log::warn!("{} extra bytes in post.stringData array", len - index);
	} else if len < index {
		log::warn!("not enough data in post.stringData array");
	}
	strings
}

/// Packs strings as Pascal strings in Latin-1 (`packPStrings`).
fn pack_pstrings(strings: &[String]) -> Result<Vec<u8>> {
	let mut data = Vec::new();
	for s in strings {
		let bytes = fontoxide_misc::py::latin1_encode(s)
			.ok_or_else(|| Error::value(format!("'latin-1' codec can't encode {s:?}")))?;
		let len: u8 = u8::try_from(s.chars().count())
			.map_err(|_| Error::value("bytes must be in range(0, 256)"))?;
		data.push(len);
		data.extend(bytes);
	}
	Ok(data)
}

impl TableType for Post {
	const TAG: Tag = Tag::new(b"post");

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		let header_data = &data[..data.len().min(PostHeader::SIZE)];
		if header_data.len() != PostHeader::SIZE {
			return Err(Error::Struct(format!(
				"unpack requires a buffer of {} bytes",
				PostHeader::SIZE
			)));
		}
		let (header, rest) = PostHeader::unpack2(data)?;
		let mut post = Post {
			header,
			..Default::default()
		};
		match post.format_type {
			1.0 => {
				let count = font.glyph_count()?;
				post.glyph_order = Some(Some(
					STANDARD_GLYPH_ORDER
						.iter()
						.take(count)
						.map(|s| (*s).to_owned())
						.collect(),
				));
			}
			2.0 => {
				let count = font.glyph_count()?;
				post.decode_format_2_0(rest, count)?;
			}
			3.0 => post.glyph_order = Some(None),
			4.0 => {
				let count = font.glyph_count()?;
				post.decode_format_4_0(rest, count)?;
			}
			f => {
				return Err(Error::ttlib(format!(
					"'post' table format {f:.6} not supported"
				)));
			}
		}
		Ok(post)
	}
}

impl Table for Post {
	/// Glyph names come from the glyph order when compiling.
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Ok(())
	}

	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		let mut data = Vec::new();
		self.header.pack(&mut data)?;
		match self.format_type {
			1.0 | 3.0 => {}
			2.0 => data.extend(self.encode_format_2_0(font)?),
			4.0 => data.extend(self.encode_format_4_0(font)?),
			f => {
				return Err(Error::ttlib(format!(
					"'post' table format {f:.6} not supported"
				)));
			}
		}
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		for (name, value) in self.header.values() {
			writer.simpletag(name, &[("value", &value)]);
			writer.newline();
		}
		if let Some(mapping) = &self.mapping {
			writer.begintag("psNames", &[]);
			writer.newline();
			writer.comment(
				"This file uses unique glyph names based on the information\n\
				 found in the 'post' table. Since these names might not be unique,\n\
				 we have to invent artificial names in case of clashes. In order to\n\
				 be able to retain the original information, we need a name to\n\
				 ps name mapping for those cases where they differ. That's what\n\
				 you see below.\n",
			);
			writer.newline();
			for (name, ps_name) in mapping {
				writer.simpletag("psName", &[("name", name), ("psName", ps_name)]);
				writer.newline();
			}
			writer.endtag("psNames");
			writer.newline();
		}
		if let Some(extra_names) = &self.extra_names {
			writer.begintag("extraNames", &[]);
			writer.newline();
			writer.comment(
				"following are the name that are not taken from the standard Mac glyph order",
			);
			writer.newline();
			for name in extra_names {
				writer.simpletag("psName", &[("name", name)]);
				writer.newline();
			}
			writer.endtag("extraNames");
			writer.newline();
		}
		if let Some(data) = &self.data {
			writer.begintag("hexdata", &[]);
			writer.newline();
			writer.dumphex(data);
			writer.endtag("hexdata");
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"psNames" => {
				let mut mapping = BTreeMap::new();
				for e in element.elements().filter(|e| e.name == "psName") {
					let name = e.attr("name").ok_or_else(|| Error::ttx("missing 'name'"))?;
					let ps_name = e
						.attr("psName")
						.ok_or_else(|| Error::ttx("missing 'psName'"))?;
					mapping.insert(name.to_owned(), ps_name.to_owned());
				}
				self.mapping = Some(mapping);
			}
			"extraNames" => {
				let mut names = Vec::new();
				for e in element.elements().filter(|e| e.name == "psName") {
					names.push(
						e.attr("name")
							.ok_or_else(|| Error::ttx("missing 'name'"))?
							.to_owned(),
					);
				}
				self.extra_names = Some(names);
			}
			"hexdata" => self.data = Some(read_hex(element)?),
			name => {
				let value = element
					.attr("value")
					.ok_or_else(|| Error::ttx(format!("missing 'value' on <{name}>")))?;
				if !self.header.set_literal(name, &eval(value)?)? {
					return Err(Error::ttx(format!("unknown 'post' field: {name}")));
				}
			}
		}
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn format_matches_upstream() {
		crate::sstruct::tests::check(PostHeader::FORMAT_NAME, PostHeader::FIELDS);
	}

	#[test]
	fn pstrings() {
		assert_eq!(unpack_pstrings(b"\x01a\x02bc", 2), ["a", "bc"]);
		assert_eq!(unpack_pstrings(b"\x05ab", 2), ["", ""]);
		assert_eq!(
			pack_pstrings(&["a".into(), "bc".into()]).ok(),
			Some(b"\x01a\x02bc".to_vec())
		);
	}
}
