//! `OS/2`: OS/2 and Windows Metrics table (`fontTools.ttLib.tables.O_S_2f_2`).

use fontoxide_misc::Tag;
use fontoxide_misc::text_tools::{Literal, binary2num, num2binary};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes, fit};
use crate::sstruct::{Bytes, I16, U8, U16, U32, eval};
use crate::table::{Table, TableType};
use crate::{Error, Font, Result};

crate::sstruct! {
	/// PANOSE classification (`Panose`).
	pub struct Panose = "panoseFormat" {
		/// Family type.
		pub b_family_type: u8 = "bFamilyType" as U8,
		/// Serif style.
		pub b_serif_style: u8 = "bSerifStyle" as U8,
		/// Weight.
		pub b_weight: u8 = "bWeight" as U8,
		/// Proportion.
		pub b_proportion: u8 = "bProportion" as U8,
		/// Contrast.
		pub b_contrast: u8 = "bContrast" as U8,
		/// Stroke variation.
		pub b_stroke_variation: u8 = "bStrokeVariation" as U8,
		/// Arm style.
		pub b_arm_style: u8 = "bArmStyle" as U8,
		/// Letter form.
		pub b_letter_form: u8 = "bLetterForm" as U8,
		/// Midline.
		pub b_midline: u8 = "bMidline" as U8,
		/// x-height.
		pub b_x_height: u8 = "bXHeight" as U8,
	}
}

crate::sstruct! {
	/// The version 0 layout (`OS2_format_0`); used to check the field list.
	pub struct Os2Format0 = "OS2_format_0" {
		/// Version.
		pub version: u16 = "version" as U16,
		/// Average character width.
		pub x_avg_char_width: i16 = "xAvgCharWidth" as I16,
		/// Weight class.
		pub us_weight_class: u16 = "usWeightClass" as U16,
		/// Width class.
		pub us_width_class: u16 = "usWidthClass" as U16,
		/// Embedding permissions.
		pub fs_type: u16 = "fsType" as U16,
		/// Subscript horizontal size.
		pub y_subscript_x_size: i16 = "ySubscriptXSize" as I16,
		/// Subscript vertical size.
		pub y_subscript_y_size: i16 = "ySubscriptYSize" as I16,
		/// Subscript x offset.
		pub y_subscript_x_offset: i16 = "ySubscriptXOffset" as I16,
		/// Subscript y offset.
		pub y_subscript_y_offset: i16 = "ySubscriptYOffset" as I16,
		/// Superscript horizontal size.
		pub y_superscript_x_size: i16 = "ySuperscriptXSize" as I16,
		/// Superscript vertical size.
		pub y_superscript_y_size: i16 = "ySuperscriptYSize" as I16,
		/// Superscript x offset.
		pub y_superscript_x_offset: i16 = "ySuperscriptXOffset" as I16,
		/// Superscript y offset.
		pub y_superscript_y_offset: i16 = "ySuperscriptYOffset" as I16,
		/// Strikeout size.
		pub y_strikeout_size: i16 = "yStrikeoutSize" as I16,
		/// Strikeout position.
		pub y_strikeout_position: i16 = "yStrikeoutPosition" as I16,
		/// Family class and subclass.
		pub s_family_class: i16 = "sFamilyClass" as I16,
		/// PANOSE.
		pub panose: [u8; 10] = "panose" as Bytes<10>,
		/// Unicode ranges, bits 0–31.
		pub ul_unicode_range1: u32 = "ulUnicodeRange1" as U32,
		/// Unicode ranges, bits 32–63.
		pub ul_unicode_range2: u32 = "ulUnicodeRange2" as U32,
		/// Unicode ranges, bits 64–95.
		pub ul_unicode_range3: u32 = "ulUnicodeRange3" as U32,
		/// Unicode ranges, bits 96–127.
		pub ul_unicode_range4: u32 = "ulUnicodeRange4" as U32,
		/// Vendor ID.
		pub ach_vend_id: [u8; 4] = "achVendID" as Bytes<4>,
		/// Selection flags.
		pub fs_selection: u16 = "fsSelection" as U16,
		/// First character index.
		pub us_first_char_index: u16 = "usFirstCharIndex" as U16,
		/// Last character index.
		pub us_last_char_index: u16 = "usLastCharIndex" as U16,
		/// Typographic ascender.
		pub s_typo_ascender: i16 = "sTypoAscender" as I16,
		/// Typographic descender.
		pub s_typo_descender: i16 = "sTypoDescender" as I16,
		/// Typographic line gap.
		pub s_typo_line_gap: i16 = "sTypoLineGap" as I16,
		/// Windows ascent.
		pub us_win_ascent: u16 = "usWinAscent" as U16,
		/// Windows descent.
		pub us_win_descent: u16 = "usWinDescent" as U16,
	}
}

crate::sstruct! {
	/// The fields version 1 adds (`OS2_format_1_addition`).
	pub struct Os2Format1Addition = "OS2_format_1_addition" {
		/// Code page ranges, bits 0–31.
		pub ul_code_page_range1: u32 = "ulCodePageRange1" as U32,
		/// Code page ranges, bits 32–63.
		pub ul_code_page_range2: u32 = "ulCodePageRange2" as U32,
	}
}

crate::sstruct! {
	/// The fields versions 2–4 add (`OS2_format_2_addition`).
	pub struct Os2Format2Addition = "OS2_format_2_addition" {
		/// Code page ranges, bits 0–31.
		pub ul_code_page_range1: u32 = "ulCodePageRange1" as U32,
		/// Code page ranges, bits 32–63.
		pub ul_code_page_range2: u32 = "ulCodePageRange2" as U32,
		/// x-height.
		pub sx_height: i16 = "sxHeight" as I16,
		/// Cap height.
		pub s_cap_height: i16 = "sCapHeight" as I16,
		/// Default character.
		pub us_default_char: u16 = "usDefaultChar" as U16,
		/// Break character.
		pub us_break_char: u16 = "usBreakChar" as U16,
		/// Maximum context of the layout features.
		pub us_max_context: u16 = "usMaxContext" as U16,
	}
}

crate::sstruct! {
	/// The fields version 5 adds (`OS2_format_5_addition`), with the optical sizes
	/// in TWIPs as stored.
	pub struct Os2Format5Addition = "OS2_format_5_addition" {
		/// Code page ranges, bits 0–31.
		pub ul_code_page_range1: u32 = "ulCodePageRange1" as U32,
		/// Code page ranges, bits 32–63.
		pub ul_code_page_range2: u32 = "ulCodePageRange2" as U32,
		/// x-height.
		pub sx_height: i16 = "sxHeight" as I16,
		/// Cap height.
		pub s_cap_height: i16 = "sCapHeight" as I16,
		/// Default character.
		pub us_default_char: u16 = "usDefaultChar" as U16,
		/// Break character.
		pub us_break_char: u16 = "usBreakChar" as U16,
		/// Maximum context of the layout features.
		pub us_max_context: u16 = "usMaxContext" as U16,
		/// Lower optical size, in TWIPs.
		pub us_lower_optical_point_size: u16 = "usLowerOpticalPointSize" as U16,
		/// Upper optical size, in TWIPs.
		pub us_upper_optical_point_size: u16 = "usUpperOpticalPointSize" as U16,
	}
}

/// OS/2 and Windows Metrics table (`table_O_S_2f_2`).
///
/// Fields that the table's version does not have are kept (as zeros) but neither
/// written nor dumped.
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/os2>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Os2 {
	/// The version 0 fields; `base.panose` is unused (see [`Os2::panose`]).
	pub base: Os2Format0,
	/// PANOSE classification.
	pub panose: Panose,
	/// Code page ranges, bits 0–31 (version ≥ 1).
	pub ul_code_page_range1: u32,
	/// Code page ranges, bits 32–63 (version ≥ 1).
	pub ul_code_page_range2: u32,
	/// x-height (version ≥ 2).
	pub sx_height: i16,
	/// Cap height (version ≥ 2).
	pub s_cap_height: i16,
	/// Default character (version ≥ 2).
	pub us_default_char: u16,
	/// Break character (version ≥ 2).
	pub us_break_char: u16,
	/// Maximum context (version ≥ 2).
	pub us_max_context: u16,
	/// Lower optical size in points (version 5); stored as TWIPs (×20).
	pub us_lower_optical_point_size: f64,
	/// Upper optical size in points (version 5); stored as TWIPs (×20).
	pub us_upper_optical_point_size: f64,
}

impl std::ops::Deref for Os2 {
	type Target = Os2Format0;
	fn deref(&self) -> &Os2Format0 {
		&self.base
	}
}

impl std::ops::DerefMut for Os2 {
	fn deref_mut(&mut self) -> &mut Os2Format0 {
		&mut self.base
	}
}

const RANGE_FIELDS: [&str; 6] = [
	"ulUnicodeRange1",
	"ulUnicodeRange2",
	"ulUnicodeRange3",
	"ulUnicodeRange4",
	"ulCodePageRange1",
	"ulCodePageRange2",
];

/// A field value for TTX.
enum Field {
	Int(i64),
	Float(f64),
	Panose,
	VendId,
}

impl Os2 {
	/// The field names of the table's version, in order.
	fn field_names(&self) -> Vec<&'static str> {
		let mut names: Vec<&'static str> = Os2Format0::FIELDS.iter().map(|f| f.name).collect();
		let extra: &[crate::sstruct::FieldInfo] = match self.version {
			1 => Os2Format1Addition::FIELDS,
			2..=4 => Os2Format2Addition::FIELDS,
			5 => Os2Format5Addition::FIELDS,
			_ => &[],
		};
		names.extend(extra.iter().map(|f| f.name));
		names
	}

	fn field(&self, name: &str) -> Field {
		let b = &self.base;
		Field::Int(match name {
			"panose" => return Field::Panose,
			"achVendID" => return Field::VendId,
			"usLowerOpticalPointSize" => return Field::Float(self.us_lower_optical_point_size),
			"usUpperOpticalPointSize" => return Field::Float(self.us_upper_optical_point_size),
			"ulCodePageRange1" => self.ul_code_page_range1 as i64,
			"ulCodePageRange2" => self.ul_code_page_range2 as i64,
			"sxHeight" => self.sx_height as i64,
			"sCapHeight" => self.s_cap_height as i64,
			"usDefaultChar" => self.us_default_char as i64,
			"usBreakChar" => self.us_break_char as i64,
			"usMaxContext" => self.us_max_context as i64,
			_ => match b.values().into_iter().find(|(n, _)| *n == name) {
				Some((_, crate::sstruct::Value::Int(i))) => i,
				_ => 0,
			},
		})
	}

	/// The set of `ulUnicodeRange*` bits that are set (`getUnicodeRanges`).
	pub fn unicode_ranges(&self) -> std::collections::BTreeSet<u32> {
		let ranges = [
			self.ul_unicode_range1,
			self.ul_unicode_range2,
			self.ul_unicode_range3,
			self.ul_unicode_range4,
		];
		let mut bits = std::collections::BTreeSet::new();
		for (k, r) in ranges.iter().enumerate() {
			for i in 0..32 {
				if r & (1 << i) != 0 {
					bits.insert(i + 32 * k as u32);
				}
			}
		}
		bits
	}

	/// Sets the `ulUnicodeRange*` fields to the given bits (`setUnicodeRanges`).
	pub fn set_unicode_ranges(&mut self, bits: impl IntoIterator<Item = u32>) -> Result<()> {
		let mut ul = [0u32; 4];
		for bit in bits {
			if bit >= 128 {
				return Err(Error::value(format!("expected 0 <= int <= 127: {bit}")));
			}
			ul[(bit / 32) as usize] |= 1 << (bit % 32);
		}
		[
			self.base.ul_unicode_range1,
			self.base.ul_unicode_range2,
			self.base.ul_unicode_range3,
			self.base.ul_unicode_range4,
		] = ul;
		Ok(())
	}

	/// Sets `usFirstCharIndex` and `usLastCharIndex` from the Unicode `cmap`
	/// subtables (`updateFirstAndLastCharIndex`).
	pub fn update_first_and_last_char_index(&mut self, font: &mut Font) -> Result<()> {
		if let Some((min, max)) = crate::tables::cmap::unicode_code_range(font)? {
			// USHORT cannot hold code points greater than 0xFFFF.
			self.base.us_first_char_index = min.min(0xFFFF) as u16;
			self.base.us_last_char_index = max.min(0xFFFF) as u16;
		}
		Ok(())
	}

	fn check_head_mac_style(&self, tag: &[u8; 4], font: &mut Font) -> Result<()> {
		let tag = Tag::new(tag);
		if !font.contains(tag) {
			return Ok(());
		}
		// The table is loaded, as `ttFont[tag]` does in fontTools.
		let mac_style = if tag == *b"head" {
			font.require::<crate::tables::Head>()?.mac_style
		} else {
			font.load(tag)?;
			return Ok(());
		};
		if self.fs_selection & 1 != 0 && mac_style & (1 << 1) == 0 {
			log::warn!(
				"fsSelection bit 0 (italic) and {tag} table macStyle bit 1 (italic) should match"
			);
		}
		if self.fs_selection & (1 << 5) != 0 && mac_style & 1 == 0 {
			log::warn!(
				"fsSelection bit 5 (bold) and {tag} table macStyle bit 0 (bold) should match"
			);
		}
		Ok(())
	}
}

impl TableType for Os2 {
	const TAG: Tag = Tag::new(b"OS/2");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let (base, rest) = Os2Format0::unpack2(data)?;
		let mut os2 = Os2 {
			panose: Panose::unpack(&mut Reader::new(&base.panose))?,
			base,
			..Default::default()
		};
		let rest = match os2.version {
			0 => rest,
			1 => {
				let (a, rest) = Os2Format1Addition::unpack2(rest)?;
				os2.ul_code_page_range1 = a.ul_code_page_range1;
				os2.ul_code_page_range2 = a.ul_code_page_range2;
				rest
			}
			2..=4 => {
				let (a, rest) = Os2Format2Addition::unpack2(rest)?;
				os2.ul_code_page_range1 = a.ul_code_page_range1;
				os2.ul_code_page_range2 = a.ul_code_page_range2;
				os2.sx_height = a.sx_height;
				os2.s_cap_height = a.s_cap_height;
				os2.us_default_char = a.us_default_char;
				os2.us_break_char = a.us_break_char;
				os2.us_max_context = a.us_max_context;
				rest
			}
			5 => {
				let (a, rest) = Os2Format5Addition::unpack2(rest)?;
				os2.ul_code_page_range1 = a.ul_code_page_range1;
				os2.ul_code_page_range2 = a.ul_code_page_range2;
				os2.sx_height = a.sx_height;
				os2.s_cap_height = a.s_cap_height;
				os2.us_default_char = a.us_default_char;
				os2.us_break_char = a.us_break_char;
				os2.us_max_context = a.us_max_context;
				os2.us_lower_optical_point_size = a.us_lower_optical_point_size as f64 / 20.0;
				os2.us_upper_optical_point_size = a.us_upper_optical_point_size as f64 / 20.0;
				rest
			}
			v => {
				return Err(Error::ttlib(format!(
					"unknown format for OS/2 table: version {v}"
				)));
			}
		};
		if !rest.is_empty() {
			log::warn!("too much 'OS/2' table data");
		}
		Ok(os2)
	}
}

impl Table for Os2 {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		self.update_first_and_last_char_index(font)?;
		self.check_head_mac_style(b"head", font)?;
		self.check_head_mac_style(b"bhed", font)?;
		if self.fs_selection & (1 << 6) != 0 && self.fs_selection & (1 + (1 << 5)) != 0 {
			log::warn!(
				"fsSelection bit 6 (regular) is set, bits 0 (italic) and 5 (bold) must be clear"
			);
		}
		if self.version < 4 && self.fs_selection & 0b11_1000_0000 != 0 {
			log::warn!(
				"fsSelection bits 7, 8 and 9 are only defined in OS/2 table version 4 and up: version {}",
				self.version
			);
		}
		let mut base = self.base.clone();
		let mut panose = Vec::with_capacity(10);
		self.panose.pack(&mut panose)?;
		base.panose.copy_from_slice(&panose);
		let mut data = Vec::new();
		match self.version {
			0..=5 => base.pack(&mut data)?,
			v => {
				return Err(Error::ttlib(format!(
					"unknown format for OS/2 table: version {v}"
				)));
			}
		}
		if self.version >= 1 {
			data.put_u32(self.ul_code_page_range1);
			data.put_u32(self.ul_code_page_range2);
		}
		if self.version >= 2 {
			data.put_i16(self.sx_height);
			data.put_i16(self.s_cap_height);
			data.put_u16(self.us_default_char);
			data.put_u16(self.us_break_char);
			data.put_u16(self.us_max_context);
		}
		if self.version == 5 {
			let twips = |v: f64, name: &str| -> Result<u16> {
				fit(fontoxide_misc::py::round(v * 20.0) as i64, name)
			};
			data.put_u16(twips(
				self.us_lower_optical_point_size,
				"usLowerOpticalPointSize",
			)?);
			data.put_u16(twips(
				self.us_upper_optical_point_size,
				"usUpperOpticalPointSize",
			)?);
		}
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		writer.comment(
			"The fields 'usFirstCharIndex' and 'usLastCharIndex'\nwill be recalculated by the compiler",
		);
		writer.newline();
		for name in self.field_names() {
			match self.field(name) {
				Field::Panose => {
					writer.begintag("panose", &[]);
					writer.newline();
					for (n, v) in self.panose.values() {
						writer.simpletag(n, &[("value", &v)]);
						writer.newline();
					}
					writer.endtag("panose");
				}
				Field::VendId => {
					// repr(value)[1:-1], where value is a str if the bytes are ASCII.
					let id = &self.ach_vend_id;
					let r = if id.is_ascii() {
						fontoxide_misc::py::str_repr(&fontoxide_misc::py::latin1_decode(id))
					} else {
						fontoxide_misc::py::bytes_repr(id)
					};
					let v = &r[1..r.len() - 1];
					writer.simpletag(name, &[("value", &v)]);
				}
				Field::Int(i) if RANGE_FIELDS.contains(&name) => {
					writer.simpletag(name, &[("value", &num2binary(i, 32))]);
				}
				Field::Int(i) if name == "fsType" || name == "fsSelection" => {
					writer.simpletag(name, &[("value", &num2binary(i, 16))]);
				}
				Field::Int(i) => writer.simpletag(name, &[("value", &i)]),
				Field::Float(f) => writer.simpletag(name, &[("value", &f)]),
			}
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		let name = element.name.as_str();
		if name == "panose" {
			let mut panose = Panose::default();
			for e in element.elements() {
				let value = e
					.attr("value")
					.ok_or_else(|| Error::ttx(format!("missing 'value' on <{}>", e.name)))?;
				panose.set_literal(&e.name, &eval(value)?)?;
			}
			self.panose = panose;
			return Ok(());
		}
		let value = element
			.attr("value")
			.ok_or_else(|| Error::ttx(format!("missing 'value' on <{name}>")))?;
		if RANGE_FIELDS.contains(&name) || name == "fsType" || name == "fsSelection" {
			let v = binary2num(value);
			match name {
				"ulCodePageRange1" => self.ul_code_page_range1 = fit(v, name)?,
				"ulCodePageRange2" => self.ul_code_page_range2 = fit(v, name)?,
				_ => {
					self.base.set_literal(name, &Literal::Int(v as i128))?;
				}
			}
			return Ok(());
		}
		if name == "achVendID" {
			// safeEval("'''" + value + "'''"), then packed as ASCII "4s".
			let s = fontoxide_misc::text_tools::parse_string_body(value)
				.ok_or_else(|| Error::value(format!("invalid achVendID: {value:?}")))?;
			if !s.is_ascii() {
				return Err(Error::value("'ascii' codec can't encode achVendID"));
			}
			let mut id = [0u8; 4];
			for (d, b) in id.iter_mut().zip(s.bytes()) {
				*d = b;
			}
			self.base.ach_vend_id = id;
			return Ok(());
		}
		let lit = eval(value)?;
		let int = |lit: &Literal| {
			lit.as_int()
				.ok_or_else(|| Error::value(format!("invalid {name}")))
		};
		match name {
			"sxHeight" => self.sx_height = fit(int(&lit)?, name)?,
			"sCapHeight" => self.s_cap_height = fit(int(&lit)?, name)?,
			"usDefaultChar" => self.us_default_char = fit(int(&lit)?, name)?,
			"usBreakChar" => self.us_break_char = fit(int(&lit)?, name)?,
			"usMaxContext" | "usMaxContex" => self.us_max_context = fit(int(&lit)?, name)?,
			"usLowerOpticalPointSize" => {
				self.us_lower_optical_point_size = lit
					.as_float()
					.ok_or_else(|| Error::value(format!("invalid {name}")))?;
			}
			"usUpperOpticalPointSize" => {
				self.us_upper_optical_point_size = lit
					.as_float()
					.ok_or_else(|| Error::value(format!("invalid {name}")))?;
			}
			_ => {
				let name = match name {
					"fsFirstCharIndex" => "usFirstCharIndex",
					"fsLastCharIndex" => "usLastCharIndex",
					n => n,
				};
				if !self.base.set_literal(name, &lit)? {
					return Err(Error::ttx(format!("unknown 'OS/2' field: {name}")));
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
	fn formats_match_upstream() {
		use crate::sstruct::tests::check;
		check(Panose::FORMAT_NAME, Panose::FIELDS);
		check(Os2Format0::FORMAT_NAME, Os2Format0::FIELDS);
		check(Os2Format1Addition::FORMAT_NAME, Os2Format1Addition::FIELDS);
		check(Os2Format2Addition::FORMAT_NAME, Os2Format2Addition::FIELDS);
		check(Os2Format5Addition::FORMAT_NAME, Os2Format5Addition::FIELDS);
	}
}
