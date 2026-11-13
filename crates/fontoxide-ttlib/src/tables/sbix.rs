//! `sbix`: Standard Bitmap Graphics table (`fontTools.ttLib.tables._s_b_i_x`,
//! `sbixStrike`, `sbixGlyph`).

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::text_tools::{binary2num, num2binary, parse_string_body};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, fit};
use crate::sstruct::{Bytes, I16, U16, U32, Value, eval};
use crate::table::{Table, TableType, TableXml, read_hex};
use crate::tables::eblc::{clip, int_attr, required_attr};
use crate::{Error, Font, Result};

crate::sstruct! {
	/// The header (`sbixHeaderFormat`).
	pub struct SbixHeader = "sbixHeaderFormat" {
		/// The table version.
		pub version: u16 = "version" as U16,
		/// Flags.
		pub flags: u16 = "flags" as U16,
		/// The number of strikes.
		pub num_strikes: u32 = "numStrikes" as U32,
	}
}

crate::sstruct! {
	/// The header of a strike (`sbixStrikeHeaderFormat`).
	pub struct SbixStrikeHeader = "sbixStrikeHeaderFormat" {
		/// The PPEM for which the strike was designed.
		pub ppem: u16 = "ppem" as U16,
		/// The screen resolution in dpi for which the strike was designed.
		pub resolution: u16 = "resolution" as U16,
	}
}

crate::sstruct! {
	/// The header of a glyph data record (`sbixGlyphHeaderFormat`).
	pub struct SbixGlyphHeader = "sbixGlyphHeaderFormat" {
		/// The horizontal origin of the glyph.
		pub origin_offset_x: i16 = "originOffsetX" as I16,
		/// The vertical origin of the glyph.
		pub origin_offset_y: i16 = "originOffsetY" as I16,
		/// The type of the graphic, e.g. `png `.
		pub graphic_type: [u8; 4] = "graphicType" as Bytes<4>,
	}
}

/// A glyph data record (`Glyph`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SbixGlyph {
	/// The horizontal origin (`originOffsetX`).
	pub origin_offset_x: i16,
	/// The vertical origin (`originOffsetY`).
	pub origin_offset_y: i16,
	/// The type of the graphic; `None` for an empty record (`graphicType`).
	pub graphic_type: Option<[u8; 4]>,
	/// The image (`imageData`).
	pub image_data: Option<Vec<u8>>,
	/// The glyph whose image this one uses, for the `dupe` and `flip` types
	/// (`referenceGlyphName`).
	pub reference: Option<u32>,
}

impl SbixGlyph {
	/// `is_reference_type`.
	pub fn is_reference_type(&self) -> bool {
		matches!(self.graphic_type.as_ref(), Some(b"dupe" | b"flip"))
	}

	fn decompile(raw: &[u8]) -> Result<Self> {
		let mut glyph = SbixGlyph::default();
		if raw.is_empty() {
			return Ok(glyph);
		}
		if raw.len() < SbixGlyphHeader::SIZE {
			return Err(Error::ttlib("Glyph header too short."));
		}
		let (header, rest) = SbixGlyphHeader::unpack2(raw)?;
		glyph.origin_offset_x = header.origin_offset_x;
		glyph.origin_offset_y = header.origin_offset_y;
		glyph.graphic_type = Some(header.graphic_type);
		if glyph.is_reference_type() {
			if rest.len() != 2 {
				return Err(Error::Struct(
					"unpack requires a buffer of 2 bytes".to_owned(),
				));
			}
			glyph.reference = Some(u32::from(Reader::new(rest).u16()?));
		} else {
			glyph.image_data = Some(rest.to_vec());
		}
		Ok(glyph)
	}

	/// `compile`, returning `rawdata`.
	fn compile(&self, out: &mut Vec<u8>) -> Result<()> {
		let Some(graphic_type) = self.graphic_type else {
			return Ok(());
		};
		SbixGlyphHeader {
			origin_offset_x: self.origin_offset_x,
			origin_offset_y: self.origin_offset_y,
			graphic_type,
		}
		.pack(out)?;
		if self.is_reference_type() {
			let reference = self
				.reference
				.ok_or_else(|| Error::ttlib("TypeError: no reference glyph"))?;
			out.extend_from_slice(&fit::<u16, _>(reference, "referenceGlyph")?.to_be_bytes());
		} else {
			let image = self
				.image_data
				.as_ref()
				.ok_or_else(|| Error::Assertion("self.imageData is not None".into()))?;
			out.extend_from_slice(image);
		}
		Ok(())
	}

	fn to_xml(&self, writer: &mut XmlWriter, name: &str, font: &mut Font) -> Result<()> {
		let Some(graphic_type) = &self.graphic_type else {
			writer.simpletag("glyph", &[("name", &name)]);
			writer.newline();
			return Ok(());
		};
		writer.begintag(
			"glyph",
			&[
				("graphicType", &Value::Bytes(graphic_type.to_vec())),
				("name", &name),
				("originOffsetX", &self.origin_offset_x),
				("originOffsetY", &self.origin_offset_y),
			],
		);
		writer.newline();
		if self.is_reference_type() {
			let reference = self
				.reference
				.ok_or_else(|| Error::ttlib("TypeError: no reference glyph"))?;
			let reference = font.glyph_name(reference)?;
			writer.simpletag("ref", &[("glyphname", &reference)]);
		} else {
			writer.begintag("hexdata", &[]);
			writer.newline();
			writer.dumphex(self.image_data.as_deref().unwrap_or_default());
			writer.endtag("hexdata");
		}
		writer.newline();
		writer.endtag("glyph");
		writer.newline();
		Ok(())
	}

	/// The `Glyph` constructor's clean-up of `graphicType`, for a `safeEval`ed
	/// attribute.
	fn graphic_type_from_str(value: &str) -> Result<[u8; 4]> {
		let mut chars: Vec<char> = value.chars().collect();
		match chars.last() {
			None => return Err(Error::ttlib("IndexError: string index out of range")),
			Some('\0') => {
				chars.pop();
			}
			Some(_) => {}
		}
		if chars.len() > 4 {
			return Err(Error::ttlib(
				"Glyph.graphicType must not be longer than 4 characters.",
			));
		}
		chars.resize(4, ' ');
		let mut out = [0; 4];
		for (o, c) in out.iter_mut().zip(chars) {
			*o = u8::try_from(c)
				.ok()
				.filter(u8::is_ascii)
				.ok_or_else(|| Error::value("'ascii' codec can't encode graphicType"))?;
		}
		Ok(out)
	}

	/// `Glyph.fromXML` followed by `compile`, for one child element.
	fn child_from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"ref" => {
				let name = string_literal(required_attr(element, "glyphname")?)?;
				let gid = font.glyph_id(&name)?;
				self.image_data = Some(fit::<u16, _>(gid, "glyphname")?.to_be_bytes().to_vec());
				self.reference = Some(gid);
			}
			"hexdata" => self.image_data = Some(read_hex(element)?),
			name => return Err(Error::ttlib(format!("can't handle '{name}' element"))),
		}
		self.compile(&mut Vec::new())
	}
}

/// `safeEval("'''" + value + "'''")`.
fn string_literal(value: &str) -> Result<String> {
	parse_string_body(value).ok_or_else(|| Error::value(format!("malformed value: {value:?}")))
}

/// A strike: bitmaps for one size (`Strike`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SbixStrike {
	/// The PPEM for which the strike was designed (`ppem`).
	pub ppem: u16,
	/// The screen resolution in dpi for which the strike was designed
	/// (`resolution`).
	pub resolution: u16,
	/// The glyph data records by glyph ID (`glyphs`, by glyph name in fontTools).
	pub glyphs: BTreeMap<u32, SbixGlyph>,
}

impl Default for SbixStrike {
	fn default() -> Self {
		SbixStrike {
			ppem: 0,
			resolution: 72,
			glyphs: BTreeMap::new(),
		}
	}
}

impl SbixStrike {
	fn decompile(raw: &[u8]) -> Result<Self> {
		if raw.len() < SbixStrikeHeader::SIZE {
			return Err(Error::ttlib("Strike header too short."));
		}
		let (header, rest) = SbixStrikeHeader::unpack2(raw)?;
		let first = Reader::new(rest).u32()?;
		// One more offset than glyphs.
		let num_glyphs = (i64::from(first) - 4).div_euclid(4) - 1;
		let count = (num_glyphs + 1).max(0) as usize;
		let offsets = Reader::new(rest).u32_array(count)?;
		let mut glyphs = BTreeMap::new();
		for (gid, pair) in offsets.windows(2).enumerate() {
			let data = clip(raw, u64::from(pair[0]), u64::from(pair[1]));
			glyphs.insert(gid as u32, SbixGlyph::decompile(data)?);
		}
		Ok(SbixStrike {
			ppem: header.ppem,
			resolution: header.resolution,
			glyphs,
		})
	}

	fn compile(&self, num_glyphs: usize) -> Result<Vec<u8>> {
		let mut bitmap_data = Vec::new();
		let mut out = Vec::new();
		SbixStrikeHeader {
			ppem: self.ppem,
			resolution: self.resolution,
		}
		.pack(&mut out)?;
		let base = SbixStrikeHeader::SIZE + 4 * (num_glyphs + 1);
		for gid in 0..num_glyphs {
			let offset: u32 = fit(base + bitmap_data.len(), "glyphDataOffset")?;
			out.extend_from_slice(&offset.to_be_bytes());
			if let Some(glyph) = self.glyphs.get(&(gid as u32)) {
				glyph.compile(&mut bitmap_data)?;
			}
		}
		let offset: u32 = fit(base + bitmap_data.len(), "glyphDataOffset")?;
		out.extend_from_slice(&offset.to_be_bytes());
		out.extend(bitmap_data);
		Ok(out)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		writer.begintag("strike", &[]);
		writer.newline();
		writer.simpletag("ppem", &[("value", &self.ppem)]);
		writer.newline();
		writer.simpletag("resolution", &[("value", &self.resolution)]);
		writer.newline();
		let order = font.glyph_order()?;
		for (gid, name) in order.names().iter().enumerate() {
			if let Some(glyph) = self.glyphs.get(&(gid as u32)) {
				glyph.to_xml(writer, name, font)?;
			}
		}
		writer.endtag("strike");
		writer.newline();
		Ok(())
	}

	fn child_from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"ppem" => self.ppem = int_attr(element, "value")?,
			"resolution" => self.resolution = int_attr(element, "value")?,
			"glyph" => {
				let mut glyph = SbixGlyph::default();
				if let Some(value) = element.attr("graphicType") {
					glyph.graphic_type =
						Some(SbixGlyph::graphic_type_from_str(&string_literal(value)?)?);
				}
				let name = element
					.attr("glyphname")
					.or_else(|| element.attr("name"))
					.ok_or_else(|| Error::ttlib("Glyph must have a glyph name."))?;
				let gid = font.glyph_id(&string_literal(name)?)?;
				if element.attr("originOffsetX").is_some() {
					glyph.origin_offset_x = int_attr(element, "originOffsetX")?;
				}
				if element.attr("originOffsetY").is_some() {
					glyph.origin_offset_y = int_attr(element, "originOffsetY")?;
				}
				for child in element.elements() {
					glyph.child_from_xml(child, font)?;
				}
				self.glyphs.insert(gid, glyph);
			}
			name => return Err(Error::ttlib(format!("can't handle '{name}' element"))),
		}
		Ok(())
	}
}

/// Standard Bitmap Graphics table (`table__s_b_i_x`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/sbix>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sbix {
	/// The table version.
	pub version: u16,
	/// Flags.
	pub flags: u16,
	/// The strikes, by ppem (`strikes`).
	pub strikes: BTreeMap<u16, SbixStrike>,
}

impl Default for Sbix {
	fn default() -> Self {
		Sbix {
			version: 1,
			flags: 1,
			strikes: BTreeMap::new(),
		}
	}
}

impl TableType for Sbix {
	const TAG: Tag = Tag::new(b"sbix");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let (header, rest) = SbixHeader::unpack2(data)?;
		let offsets = Reader::new(rest).u32_array(header.num_strikes as usize)?;
		let mut strikes = BTreeMap::new();
		// Each strike ends where the next one starts.
		let mut end = data.len() as u64;
		for &offset in offsets.iter().rev() {
			let offset = u64::from(offset);
			let strike = SbixStrike::decompile(clip(data, offset, end))?;
			end = end.min(offset);
			if strikes.insert(strike.ppem, strike).is_some() {
				return Err(Error::ttlib("Pixel 'ppem' must be unique for each Strike"));
			}
		}
		Ok(Sbix {
			version: header.version,
			flags: header.flags,
			strikes,
		})
	}
}

impl Table for Sbix {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		let num_glyphs = font.glyph_order()?.len();
		let mut header = Vec::new();
		SbixHeader {
			version: self.version,
			flags: self.flags,
			num_strikes: fit(self.strikes.len(), "numStrikes")?,
		}
		.pack(&mut header)?;
		let mut offset = SbixHeader::SIZE + 4 * self.strikes.len();
		let mut strike_data = Vec::new();
		for strike in self.strikes.values() {
			let data = strike.compile(num_glyphs)?;
			let strike_offset: u32 = fit(offset, "strikeOffset")?;
			header.extend_from_slice(&strike_offset.to_be_bytes());
			offset += data.len();
			strike_data.extend(data);
		}
		header.extend(strike_data);
		Ok(header)
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		for strike in self.strikes.values_mut() {
			let glyphs = std::mem::take(&mut strike.glyphs);
			for (gid, mut glyph) in glyphs {
				if let Some(reference) = &mut glyph.reference {
					*reference = f(*reference)?;
				}
				strike.glyphs.insert(f(gid)?, glyph);
			}
		}
		Ok(())
	}
}

impl TableXml for Sbix {
	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		writer.simpletag("version", &[("value", &self.version)]);
		writer.newline();
		writer.simpletag(
			"flags",
			&[("value", &num2binary(i64::from(self.flags), 16))],
		);
		writer.newline();
		for strike in self.strikes.values() {
			strike.to_xml(writer, font)?;
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"version" => {
				self.version = fit(
					eval(required_attr(element, "value")?)?
						.as_int()
						.ok_or_else(|| Error::value("invalid value for version"))?,
					"version",
				)?;
			}
			"flags" => {
				self.flags = fit(binary2num(required_attr(element, "value")?), "flags")?;
			}
			"strike" => {
				let mut strike = SbixStrike::default();
				for child in element.elements() {
					strike.child_from_xml(child, font)?;
				}
				self.strikes.insert(strike.ppem, strike);
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
		check(SbixHeader::FORMAT_NAME, SbixHeader::FIELDS);
		check(SbixStrikeHeader::FORMAT_NAME, SbixStrikeHeader::FIELDS);
		check(SbixGlyphHeader::FORMAT_NAME, SbixGlyphHeader::FIELDS);
	}

	#[test]
	fn graphic_type_clean_up() -> Result<()> {
		assert_eq!(SbixGlyph::graphic_type_from_str("png")?, *b"png ");
		assert_eq!(SbixGlyph::graphic_type_from_str("png\0")?, *b"png ");
		assert!(SbixGlyph::graphic_type_from_str("pngxy").is_err());
		assert!(SbixGlyph::graphic_type_from_str("").is_err());
		Ok(())
	}
}
