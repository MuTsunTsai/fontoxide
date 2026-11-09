//! `CPAL`: Color Palette table (`fontTools.ttLib.tables.C_P_A_L_`).

use std::collections::HashMap;

use fontoxide_misc::Tag;
use fontoxide_misc::text_tools::parse_int_radix;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::fit;
use crate::sstruct::eval;
use crate::table::{Table, TableType};
use crate::tables::Name;
use crate::{Error, Font, Result};

/// A palette entry (`Color`), stored as BGRA.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Color {
	/// Blue.
	pub blue: u8,
	/// Green.
	pub green: u8,
	/// Red.
	pub red: u8,
	/// Alpha.
	pub alpha: u8,
}

impl Color {
	/// `#RRGGBBAA`.
	pub fn hex(&self) -> String {
		format!(
			"#{:02X}{:02X}{:02X}{:02X}",
			self.red, self.green, self.blue, self.alpha
		)
	}

	/// Parses `#RRGGBB` or `#RRGGBBAA`; the `#` is optional (`Color.fromHex`).
	pub fn from_hex(value: &str) -> Result<Self> {
		let value = value.strip_prefix('#').unwrap_or(value);
		let chars: Vec<char> = value.chars().collect();
		let component = |i: usize| -> Result<u8> {
			let s: String = chars.iter().skip(i).take(2).collect();
			// LENIENT(L7): fontTools accepts any integer here and fails when writing.
			parse_int_radix(&s, 16)
				.and_then(|v| u8::try_from(v).ok())
				.ok_or_else(|| {
					Error::value(format!("invalid literal for int() with base 16: {s:?}"))
				})
		};
		Ok(Color {
			red: component(0)?,
			green: component(2)?,
			blue: component(4)?,
			alpha: if chars.len() >= 8 {
				component(6)?
			} else {
				0xFF
			},
		})
	}
}

/// Color Palette table (`table_C_P_A_L_`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/cpal>.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cpal {
	/// Table version, 0 or 1.
	pub version: u16,
	/// The number of colors in each palette (`numPaletteEntries`).
	pub num_palette_entries: u16,
	/// The palettes.
	pub palettes: Vec<Vec<Color>>,
	/// The type flags of each palette (`paletteTypes`, version 1).
	pub palette_types: Vec<u32>,
	/// The `name` ID of each palette (`paletteLabels`, version 1).
	pub palette_labels: Vec<u16>,
	/// The `name` ID of each palette entry (`paletteEntryLabels`, version 1).
	pub palette_entry_labels: Vec<u16>,
}

impl Cpal {
	/// No name (`NO_NAME_ID`).
	pub const NO_NAME_ID: u16 = 0xFFFF;
	/// The default palette type (`DEFAULT_PALETTE_TYPE`).
	pub const DEFAULT_PALETTE_TYPE: u32 = 0;
}

fn struct_error() -> Error {
	Error::Struct("unpack requires a buffer of more bytes".into())
}

fn u16_at(data: &[u8], pos: usize) -> Result<u16> {
	data.get(pos..pos + 2)
		.map(|b| u16::from_be_bytes([b[0], b[1]]))
		.ok_or_else(struct_error)
}

fn u32_at(data: &[u8], pos: usize) -> Result<u32> {
	data.get(pos..pos + 4)
		.map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
		.ok_or_else(struct_error)
}

/// `_decompileUInt16Array` / `_decompileUInt32Array`.
fn read_array<T: Copy>(
	data: &[u8],
	offset: usize,
	count: usize,
	default: T,
	size: usize,
	read: impl Fn(&[u8]) -> T,
) -> Result<Vec<T>> {
	if offset == 0 {
		return Ok(vec![default; count]);
	}
	let end = offset.saturating_add(size * count).min(data.len());
	let bytes = data.get(offset..end).unwrap_or_default();
	if bytes.len() % size != 0 {
		// `array.frombytes` needs whole items.
		return Err(Error::value("bytes length not a multiple of item size"));
	}
	if bytes.len() != size * count {
		return Err(Error::Assertion(format!(
			"expected {count} items, got {}",
			bytes.len() / size
		)));
	}
	Ok(bytes.chunks_exact(size).map(read).collect())
}

impl TableType for Cpal {
	const TAG: Tag = Tag::new(b"CPAL");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let version = u16_at(data, 0)?;
		let num_palette_entries = u16_at(data, 2)?;
		let num_palettes = u16_at(data, 4)? as usize;
		let num_color_records = u16_at(data, 6)? as usize;
		let first_color_record = u32_at(data, 8)? as usize;
		if version > 1 {
			return Err(Error::Assertion(
				"Version of CPAL table is higher than I know how to handle".into(),
			));
		}
		let n = num_palette_entries as usize;
		let mut palettes = Vec::with_capacity(num_palettes.min(4096));
		for i in 0..num_palettes {
			let start = u16_at(data, 12 + 2 * i)? as usize;
			if start + n > num_color_records {
				return Err(Error::Assertion(
					"startIndex + self.numPaletteEntries <= numColorRecords".into(),
				));
			}
			let mut palette = Vec::with_capacity(n);
			for j in 0..n {
				let pos = first_color_record + (start + j) * 4;
				let b = data.get(pos..pos + 4).ok_or_else(struct_error)?;
				palette.push(Color {
					blue: b[0],
					green: b[1],
					red: b[2],
					alpha: b[3],
				});
			}
			palettes.push(palette);
		}
		let (types_offset, labels_offset, entry_labels_offset) = if version == 0 {
			(0, 0, 0)
		} else {
			let pos = 12 + num_palettes * 2;
			(
				u32_at(data, pos)? as usize,
				u32_at(data, pos + 4)? as usize,
				u32_at(data, pos + 8)? as usize,
			)
		};
		let u16s = |b: &[u8]| u16::from_be_bytes([b[0], b[1]]);
		Ok(Cpal {
			version,
			num_palette_entries,
			palettes,
			palette_types: read_array(
				data,
				types_offset,
				num_palettes,
				Self::DEFAULT_PALETTE_TYPE,
				4,
				|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
			)?,
			palette_labels: read_array(
				data,
				labels_offset,
				num_palettes,
				Self::NO_NAME_ID,
				2,
				u16s,
			)?,
			palette_entry_labels: read_array(
				data,
				entry_labels_offset,
				n,
				Self::NO_NAME_ID,
				2,
				u16s,
			)?,
		})
	}
}

impl Cpal {
	/// `_compileColorRecords`: identical palettes share their color records.
	fn compile_color_records(&self) -> Result<(Vec<u8>, Vec<u8>)> {
		let mut indices = Vec::new();
		let mut records: Vec<Vec<u8>> = Vec::new();
		let mut pool: HashMap<Vec<u8>, usize> = HashMap::new();
		for palette in &self.palettes {
			if palette.len() != self.num_palette_entries as usize {
				return Err(Error::Assertion(
					"len(palette) == self.numPaletteEntries".into(),
				));
			}
			let packed: Vec<u8> = palette
				.iter()
				.flat_map(|c| [c.blue, c.green, c.red, c.alpha])
				.collect();
			let index = match pool.get(&packed) {
				Some(&i) => i,
				None => {
					let i = records.len();
					records.push(packed.clone());
					pool.insert(packed, i);
					i
				}
			};
			let start: u16 = fit(
				index * self.num_palette_entries as usize,
				"colorRecordIndex",
			)?;
			indices.extend_from_slice(&start.to_be_bytes());
		}
		Ok((indices, records.concat()))
	}
}

impl Table for Cpal {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Ok(())
	}

	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let (indices, records) = self.compile_color_records()?;
		let v1 = self.version >= 1;
		let types: Vec<u8> = if !v1 || self.palette_types.iter().all(|&t| t == 0) {
			Vec::new()
		} else {
			if self.palette_types.len() != self.palettes.len() {
				return Err(Error::Assertion(
					"len(self.paletteTypes) == len(self.palettes)".into(),
				));
			}
			self.palette_types
				.iter()
				.flat_map(|t| t.to_be_bytes())
				.collect()
		};
		let labels = |list: &[u16], len: usize, what: &str| -> Result<Vec<u8>> {
			if !v1 || list.iter().all(|&l| l == Self::NO_NAME_ID) {
				return Ok(Vec::new());
			}
			if list.len() != len {
				return Err(Error::Assertion(format!("len({what}) mismatch")));
			}
			Ok(list.iter().flat_map(|l| l.to_be_bytes()).collect())
		};
		let palette_labels = labels(
			&self.palette_labels,
			self.palettes.len(),
			"self.paletteLabels",
		)?;
		let entry_labels = labels(
			&self.palette_entry_labels,
			self.num_palette_entries as usize,
			"self.paletteEntryLabels",
		)?;
		let num_color_records: u16 = fit(records.len() / 4, "numColorRecords")?;
		let num_palettes: u16 = fit(self.palettes.len(), "numPalettes")?;
		let mut first_color_record = 12 + indices.len();
		if v1 {
			first_color_record += 12;
		}
		let mut out = Vec::new();
		out.extend_from_slice(&self.version.to_be_bytes());
		out.extend_from_slice(&self.num_palette_entries.to_be_bytes());
		out.extend_from_slice(&num_palettes.to_be_bytes());
		out.extend_from_slice(&num_color_records.to_be_bytes());
		out.extend_from_slice(
			&fit::<u32, _>(first_color_record, "offsetToFirstColorRecord")?.to_be_bytes(),
		);
		out.extend_from_slice(&indices);
		if !v1 {
			out.extend_from_slice(&records);
			return Ok(out);
		}
		let mut pos = first_color_record + records.len();
		let mut offset = |data: &[u8], advance: usize| -> Result<u32> {
			if data.is_empty() {
				return Ok(0);
			}
			let at = fit::<u32, _>(pos, "offset")?;
			pos += advance;
			Ok(at)
		};
		let types_offset = offset(&types, types.len())?;
		let labels_offset = offset(&palette_labels, palette_labels.len())?;
		// fontTools advances by the palette labels' length here; nothing follows.
		let entry_labels_offset = offset(&entry_labels, palette_labels.len())?;
		for o in [types_offset, labels_offset, entry_labels_offset] {
			out.extend_from_slice(&o.to_be_bytes());
		}
		out.extend_from_slice(&records);
		out.extend_from_slice(&types);
		out.extend_from_slice(&palette_labels);
		out.extend_from_slice(&entry_labels);
		Ok(out)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		writer.simpletag("version", &[("value", &self.version)]);
		writer.newline();
		writer.simpletag("numPaletteEntries", &[("value", &self.num_palette_entries)]);
		writer.newline();
		let v1 = self.version > 0;
		for (index, palette) in self.palettes.iter().enumerate() {
			let palette_type = self
				.palette_types
				.get(index)
				.copied()
				.unwrap_or(Self::DEFAULT_PALETTE_TYPE);
			let label = self
				.palette_labels
				.get(index)
				.copied()
				.unwrap_or(Self::NO_NAME_ID);
			let label_text = label.to_string();
			let type_text = palette_type.to_string();
			let mut attrs: Vec<(&str, &dyn fontoxide_misc::xml_writer::AttrValue)> =
				vec![("index", &index)];
			if v1 && label != Self::NO_NAME_ID {
				attrs.push(("label", &label_text));
			}
			if v1 && palette_type != Self::DEFAULT_PALETTE_TYPE {
				attrs.push(("type", &type_text));
			}
			writer.begintag("palette", &attrs);
			writer.newline();
			if v1
				&& label != Self::NO_NAME_ID
				&& let Some(name) = font.get::<Name>()?.and_then(|n| n.get_debug_name(label))
			{
				writer.comment(&name);
				writer.newline();
			}
			if palette.len() != self.num_palette_entries as usize {
				return Err(Error::Assertion(
					"len(palette) == self.numPaletteEntries".into(),
				));
			}
			for (cindex, color) in palette.iter().enumerate() {
				writer.simpletag("color", &[("value", &color.hex()), ("index", &cindex)]);
				writer.newline();
			}
			writer.endtag("palette");
			writer.newline();
		}
		if v1
			&& !self
				.palette_entry_labels
				.iter()
				.all(|&l| l == Self::NO_NAME_ID)
		{
			writer.begintag("paletteEntryLabels", &[]);
			writer.newline();
			for (index, &label) in self.palette_entry_labels.iter().enumerate() {
				if label == Self::NO_NAME_ID {
					continue;
				}
				writer.simpletag("label", &[("index", &index), ("value", &label)]);
				if label != 0
					&& let Some(name) = font.get::<Name>()?.and_then(|n| n.get_debug_name(label))
				{
					writer.comment(&name);
				}
				writer.newline();
			}
			writer.endtag("paletteEntryLabels");
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		// LENIENT(L7): fontTools keeps any integer and fails when writing one that
		// does not fit; values are range-checked here.
		let int = |name: &str, default: Option<i128>| -> Result<i128> {
			match element.attr(name) {
				Some(s) => parse_int_radix(s, 10).ok_or_else(|| {
					Error::value(format!("invalid literal for int() with base 10: {s:?}"))
				}),
				None => default.ok_or_else(|| Error::ttx(format!("KeyError: {name:?}"))),
			}
		};
		match element.name.as_str() {
			"palette" => {
				self.palette_labels
					.push(fit(int("label", Some(Self::NO_NAME_ID as i128))?, "label")?);
				self.palette_types.push(fit(
					int("type", Some(Self::DEFAULT_PALETTE_TYPE as i128))?,
					"type",
				)?);
				let palette = element
					.elements()
					.map(|e| {
						let value = e
							.attr("value")
							.ok_or_else(|| Error::ttx("KeyError: 'value'"))?;
						Color::from_hex(value)
					})
					.collect::<Result<_>>()?;
				self.palettes.push(palette);
			}
			"paletteEntryLabels" => {
				let mut labels: HashMap<i128, u16> = HashMap::new();
				for e in element.elements().filter(|e| e.name == "label") {
					let attr = |name: &str| -> Result<i128> {
						let s = e
							.attr(name)
							.ok_or_else(|| Error::ttx(format!("KeyError: {name:?}")))?;
						eval(s)?
							.as_int()
							.ok_or_else(|| Error::value(format!("invalid {name}: {s:?}")))
					};
					let index = attr("index")?;
					labels.insert(index, fit(attr("value")?, "nameID")?);
				}
				self.palette_entry_labels = (0..self.num_palette_entries as i128)
					.map(|i| labels.get(&i).copied().unwrap_or(Self::NO_NAME_ID))
					.collect();
			}
			name => {
				if let Some(value) = element.attr("value") {
					let value = eval(value)?
						.as_int()
						.ok_or_else(|| Error::value(format!("invalid {name}: {value:?}")))?;
					match name {
						"version" => self.version = fit(value, "version")?,
						"numPaletteEntries" => {
							self.num_palette_entries = fit(value, "numPaletteEntries")?;
							self.palette_entry_labels =
								vec![Self::NO_NAME_ID; self.num_palette_entries as usize];
						}
						_ => {}
					}
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

	use fontoxide_misc::text_tools::de_hex_str;

	// Data from fontTools' C_P_A_L_test.py.
	const V0: &str = "0000 0002 0002 0004 00000010 0000 0002 000000FF FFCC66FF 000000FF 000080FF";
	const V0_SHARING: &str = "0000 0003 0004 0006 00000014 0000 0000 0003 0000 		443322FF 77889911 55555555 443322FF 77889911 FFFFFFFF";
	const V1_NO_LABELS: &str = "0001 0003 0002 0006 0000001C 0000 0003 00000000 00000000 00000000 		CAFECAFE 00112233 44556677 31415927 42424242 00331337";
	const V1: &str = "0001 0003 0002 0006 0000001C 0000 0003 00000034 0000003C 00000040 		CAFECAFE 00112233 44556677 31415927 42424242 00331337 		00000001 00000002 0102 0103 0201 0202 0203";

	fn data(hex: &str) -> Vec<u8> {
		de_hex_str(hex).unwrap_or_default()
	}

	#[test]
	fn round_trips() -> Result<()> {
		let mut font = Font::new();
		for hex in [V0, V0_SHARING, V1_NO_LABELS, V1] {
			let data = data(hex);
			let mut cpal = Cpal::decompile(&data, &mut font)?;
			assert_eq!(cpal.compile(&mut font)?, data);
		}
		Ok(())
	}

	#[test]
	fn decompile_v1() -> Result<()> {
		let mut font = Font::new();
		let cpal = Cpal::decompile(&data(V1), &mut font)?;
		assert_eq!(cpal.palette_types, [1, 2]);
		assert_eq!(cpal.palette_labels, [258, 259]);
		assert_eq!(cpal.palette_entry_labels, [513, 514, 515]);
		let mut writer = XmlWriter::new();
		cpal.to_xml(&mut writer, &mut font)?;
		let xml = writer.into_string();
		assert!(xml.contains("<palette index=\"0\" label=\"258\" type=\"1\">"));
		assert!(xml.contains("<color index=\"1\" value=\"#22110033\"/>"));
		assert!(xml.contains("<label index=\"2\" value=\"515\"/>"));
		Ok(())
	}

	#[test]
	fn colors_from_hex() -> Result<()> {
		let c = Color::from_hex("#11223344")?;
		assert_eq!(c.hex(), "#11223344");
		assert_eq!(Color::from_hex("AABBCC")?.alpha, 0xFF);
		assert!(Color::from_hex("#0x1234").is_err());
		Ok(())
	}
}
