//! `head`: Font Header table (`fontTools.ttLib.tables._h_e_a_d`).

use fontoxide_misc::Tag;
use fontoxide_misc::fixed_tools::{float_to_fixed_to_str, str_to_fixed_to_float};
use fontoxide_misc::text_tools::{binary2num, num2binary};
use fontoxide_misc::time_tools::{timestamp_from_string, timestamp_now, timestamp_to_string};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::sstruct::{F16Dot16, I16, U16, U32, U64, Value, eval};
use crate::table::{Table, TableType};
use crate::{Error, Font, Result};

/// The error when `recalc_timestamp` is on but there is no current time.
pub(crate) const NO_TIMESTAMP: &str =
	"invalid SOURCE_DATE_EPOCH or no clock (on wasm, call fontoxide_ttlib::set_clock first)";

crate::sstruct! {
	/// Font Header table (`table__h_e_a_d`).
	///
	/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/head>.
	pub struct Head = "headFormat" {
		/// Table version, normally 1.0.
		pub table_version: f64 = "tableVersion" as F16Dot16,
		/// Font revision, set by the font manufacturer.
		pub font_revision: f64 = "fontRevision" as F16Dot16,
		/// Set when the font is compiled so that the font checksum is 0xB1B0AFBA.
		pub check_sum_adjustment: u32 = "checkSumAdjustment" as U32,
		/// 0x5F0F3CF5.
		pub magic_number: u32 = "magicNumber" as U32,
		/// Flags.
		pub flags: u16 = "flags" as U16,
		/// Units per em.
		pub units_per_em: u16 = "unitsPerEm" as U16,
		/// Creation time, in seconds since 1904-01-01.
		pub created: u64 = "created" as U64,
		/// Modification time, in seconds since 1904-01-01.
		pub modified: u64 = "modified" as U64,
		/// Bounding box of all glyphs.
		pub x_min: i16 = "xMin" as I16,
		/// Bounding box of all glyphs.
		pub y_min: i16 = "yMin" as I16,
		/// Bounding box of all glyphs.
		pub x_max: i16 = "xMax" as I16,
		/// Bounding box of all glyphs.
		pub y_max: i16 = "yMax" as I16,
		/// Bold, italic, … flags.
		pub mac_style: u16 = "macStyle" as U16,
		/// Smallest readable size in pixels.
		pub lowest_rec_ppem: u16 = "lowestRecPPEM" as U16,
		/// Deprecated; set to 2.
		pub font_direction_hint: i16 = "fontDirectionHint" as I16,
		/// 0 for short `loca` offsets, 1 for long.
		pub index_to_loc_format: i16 = "indexToLocFormat" as I16,
		/// 0.
		pub glyph_data_format: i16 = "glyphDataFormat" as I16,
	}
}

impl TableType for Head {
	const TAG: Tag = Tag::new(b"head");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let (mut head, rest) = Head::unpack2(data)?;
		if !rest.is_empty() {
			// Quite illegal, but there seem to be fonts out there that do this.
			log::warn!("extra bytes at the end of 'head' table");
			if rest != b"\0\0" {
				return Err(Error::Assertion("rest == b'\\0\\0'".into()));
			}
		}
		// For timestamp fields, ignore the top four bytes. Some fonts have bogus
		// values there. Since till 2038 those bytes only can be zero, ignore them.
		for (name, stamp) in [
			("created", &mut head.created),
			("modified", &mut head.modified),
		] {
			if *stamp > 0xFFFF_FFFF {
				log::warn!("'{name}' timestamp out of range; ignoring top bytes");
				*stamp &= 0xFFFF_FFFF;
			}
			if *stamp < 0x7C25_9DC0 {
				log::warn!("'{name}' timestamp seems very low; regarding as unix timestamp");
				*stamp += 0x7C25_9DC0;
			}
		}
		Ok(head)
	}
}

impl Head {
	fn set_bbox(&mut self, [x_min, y_min, x_max, y_max]: [i64; 4]) -> Result<()> {
		self.x_min = crate::binary::fit(x_min, "xMin")?;
		self.y_min = crate::binary::fit(y_min, "yMin")?;
		self.x_max = crate::binary::fit(x_max, "xMax")?;
		self.y_max = crate::binary::fit(y_max, "yMax")?;
		Ok(())
	}
}

impl Table for Head {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Ok(())
	}

	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		if font.recalc_bboxes {
			// For TrueType-flavored fonts, the bounding box is set in maxp.recalc().
			if font.contains(Tag::new(b"VARC")) {
				// Loading the outlines may need this table, which is detached from the
				// font while compiling; a copy stands in until that is over.
				font.insert(self.clone_table());
				let glyph_set = crate::glyph_set::GlyphSet::new(font)?;
				let mut pen = fontoxide_pens::BoundsPen::new_bounds(Some(&glyph_set));
				for name in glyph_set.glyph_order().names() {
					glyph_set.draw(name, &mut pen)?;
				}
				if let Some(b) = pen.bounds() {
					self.set_bbox(fontoxide_misc::array_tools::int_rect(b))?;
				}
			} else if font.contains(Tag::new(b"CFF ")) {
				let cff = &font.require::<crate::tables::Cff>()?.cff;
				let top = cff
					.top_dicts
					.first()
					.ok_or_else(crate::cff::charstring::index_error)?;
				let bbox = top
					.get("FontBBox")
					.ok_or_else(|| Error::ttlib("AttributeError: FontBBox"))?;
				let rect = match &bbox {
					crate::cff::dict::Value::List(v) if v.len() == 4 => {
						let n = |i: usize| {
							v[i].number()
								.ok_or_else(|| Error::ttlib("TypeError: must be real number"))
						};
						[n(0)?, n(1)?, n(2)?, n(3)?]
					}
					_ => return Err(Error::value("not enough values to unpack (expected 4)")),
				};
				self.set_bbox(fontoxide_misc::array_tools::int_rect(rect))?;
			} else if font.contains(Tag::new(b"CFF2"))
				&& let Some(rect) = font.require::<crate::tables::Cff2>()?.cff.font_bounds()?
			{
				self.set_bbox(fontoxide_misc::array_tools::int_rect(rect))?;
			}
		}
		if font.recalc_timestamp {
			let now = timestamp_now().ok_or_else(|| Error::value(NO_TIMESTAMP))?;
			self.modified = crate::binary::fit(now, "modified")?;
		}
		let mut data = Vec::with_capacity(Self::SIZE);
		self.pack(&mut data)?;
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		writer.comment("Most of this table will be recalculated by the compiler");
		writer.newline();
		for (field, (name, value)) in Self::FIELDS.iter().zip(self.values()) {
			let text = match (name, value) {
				(_, Value::Float(f)) => float_to_fixed_to_str(f, field.fraction_bits.unwrap_or(16)),
				("created" | "modified", Value::Int(i)) => timestamp_to_string(i),
				("magicNumber" | "checkSumAdjustment", Value::Int(i)) => format!("{i:#x}"),
				("macStyle" | "flags", Value::Int(i)) => num2binary(i, 16),
				(_, v) => fontoxide_misc::xml_writer::AttrValue::attr_string(&v).into_owned(),
			};
			writer.simpletag(name, &[("value", &text)]);
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		let name = element.name.as_str();
		let value = element
			.attr("value")
			.ok_or_else(|| Error::ttx(format!("missing 'value' on <{name}>")))?;
		let fixed = Self::FIELDS
			.iter()
			.find(|f| f.name == name)
			.and_then(|f| f.fraction_bits);
		match name {
			_ if fixed.is_some() => {
				let v = str_to_fixed_to_float(value, fixed.unwrap_or(16)).ok_or_else(|| {
					Error::value(format!("could not convert string to float: {value:?}"))
				})?;
				let lit = fontoxide_misc::text_tools::Literal::Float(v);
				self.set_literal(name, &lit)?;
			}
			"created" | "modified" => {
				let t = timestamp_from_string(value)
					.ok_or_else(|| Error::value(format!("invalid timestamp: {value:?}")))?;
				let t: u64 = crate::binary::fit(t, name)?;
				if name == "created" {
					self.created = t;
				} else {
					self.modified = t;
				}
			}
			"macStyle" | "flags" => {
				let v: u16 = crate::binary::fit(binary2num(value), name)?;
				if name == "flags" {
					self.flags = v;
				} else {
					self.mac_style = v;
				}
			}
			_ => {
				if !self.set_literal(name, &eval(value)?)? {
					return Err(Error::ttx(format!("unknown 'head' field: {name}")));
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
		crate::sstruct::tests::check(Head::FORMAT_NAME, Head::FIELDS);
	}
}
