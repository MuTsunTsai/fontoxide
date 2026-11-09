//! `hhea` and `vhea`: Horizontal and Vertical Header tables
//! (`fontTools.ttLib.tables._h_h_e_a`, `_v_h_e_a`).

use fontoxide_misc::Tag;
use fontoxide_misc::fixed_tools::{ensure_version_is_long, version_to_fixed};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::WriteBytes;
use crate::sstruct::{I16, U16, U16Wide, U32, eval};
use crate::table::{Table, TableType};
use crate::{Error, Font, Result};

crate::sstruct! {
	/// Horizontal Header table (`table__h_h_e_a`).
	///
	/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/hhea>.
	pub struct Hhea = "hheaFormat" {
		/// Table version, 0x00010000.
		pub table_version: u32 = "tableVersion" as U32,
		/// Typographic ascent (`ascender` in the OpenType specification).
		pub ascent: i16 = "ascent" as I16,
		/// Typographic descent (`descender` in the OpenType specification).
		pub descent: i16 = "descent" as I16,
		/// Typographic line gap.
		pub line_gap: i16 = "lineGap" as I16,
		/// Maximum advance width.
		pub advance_width_max: u16 = "advanceWidthMax" as U16,
		/// Minimum left side bearing of glyphs with contours.
		pub min_left_side_bearing: i16 = "minLeftSideBearing" as I16,
		/// Minimum right side bearing of glyphs with contours.
		pub min_right_side_bearing: i16 = "minRightSideBearing" as I16,
		/// Maximum of `lsb + (xMax - xMin)`.
		pub x_max_extent: i16 = "xMaxExtent" as I16,
		/// Slope of the caret: rise.
		pub caret_slope_rise: i16 = "caretSlopeRise" as I16,
		/// Slope of the caret: run.
		pub caret_slope_run: i16 = "caretSlopeRun" as I16,
		/// Caret offset for slanted fonts.
		pub caret_offset: i16 = "caretOffset" as I16,
		/// Reserved.
		pub reserved0: i16 = "reserved0" as I16,
		/// Reserved.
		pub reserved1: i16 = "reserved1" as I16,
		/// Reserved.
		pub reserved2: i16 = "reserved2" as I16,
		/// Reserved.
		pub reserved3: i16 = "reserved3" as I16,
		/// 0.
		pub metric_data_format: i16 = "metricDataFormat" as I16,
		/// Number of long metrics in `hmtx`.
		pub number_of_h_metrics: u32 = "numberOfHMetrics" as U16Wide,
	}
	extra {
		/// Whether this is `HHEA`, with a 32-bit `numberOfHMetrics`.
		pub extended: bool,
	}
}

crate::sstruct! {
	/// Vertical Header table (`table__v_h_e_a`).
	///
	/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/vhea>.
	pub struct Vhea = "vheaFormat" {
		/// Table version, 0x00010000 or 0x00011000.
		pub table_version: u32 = "tableVersion" as U32,
		/// Vertical typographic ascender.
		pub ascent: i16 = "ascent" as I16,
		/// Vertical typographic descender.
		pub descent: i16 = "descent" as I16,
		/// Vertical typographic line gap.
		pub line_gap: i16 = "lineGap" as I16,
		/// Maximum advance height.
		pub advance_height_max: u16 = "advanceHeightMax" as U16,
		/// Minimum top side bearing of glyphs with contours.
		pub min_top_side_bearing: i16 = "minTopSideBearing" as I16,
		/// Minimum bottom side bearing of glyphs with contours.
		pub min_bottom_side_bearing: i16 = "minBottomSideBearing" as I16,
		/// Maximum of `tsb + (yMax - yMin)`.
		pub y_max_extent: i16 = "yMaxExtent" as I16,
		/// Slope of the caret: rise.
		pub caret_slope_rise: i16 = "caretSlopeRise" as I16,
		/// Slope of the caret: run.
		pub caret_slope_run: i16 = "caretSlopeRun" as I16,
		/// Caret offset (also called `reserved0` in fontTools).
		pub caret_offset: i16 = "caretOffset" as I16,
		/// Reserved.
		pub reserved1: i16 = "reserved1" as I16,
		/// Reserved.
		pub reserved2: i16 = "reserved2" as I16,
		/// Reserved.
		pub reserved3: i16 = "reserved3" as I16,
		/// Reserved.
		pub reserved4: i16 = "reserved4" as I16,
		/// 0.
		pub metric_data_format: i16 = "metricDataFormat" as I16,
		/// Number of long metrics in `vmtx`.
		pub number_of_v_metrics: u32 = "numberOfVMetrics" as U16Wide,
	}
	extra {
		/// Whether this is `VHEA`, with a 32-bit `numberOfVMetrics`.
		pub extended: bool,
	}
}

impl Hhea {
	/// `ascender`, the OpenType specification's name for [`Hhea::ascent`].
	pub fn ascender(&self) -> i16 {
		self.ascent
	}

	/// `descender`, the OpenType specification's name for [`Hhea::descent`].
	pub fn descender(&self) -> i16 {
		self.descent
	}
}

/// `fi2ve(tableVersion)`: a version below 0x10000 is taken as a float.
fn version_as_long(v: u32) -> Result<u32> {
	crate::binary::fit(ensure_version_is_long(v as f64), "tableVersion")
}

macro_rules! header_table {
	($ty:ident, $tag:literal, $upper:literal, $count:ident) => {
		impl TableType for $ty {
			const TAG: Tag = Tag::new($tag);
			const COMPANION: Option<Tag> = Some(Tag::new($upper));

			fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
				Self::decompile_as(Self::TAG, data, font)
			}

			fn decompile_as(tag: Tag, data: &[u8], _font: &mut Font) -> Result<Self> {
				// sstruct.unpack requires the exact size.
				let extended = tag == Tag::new($upper);
				let size = if extended { Self::SIZE + 2 } else { Self::SIZE };
				if data.len() != size {
					return Err(Error::Struct(format!(
						"unpack requires a buffer of {size} bytes"
					)));
				}
				if !extended {
					return Self::unpack2(data).map(|(t, _)| t);
				}
				// The count is the last field; the others are the same.
				let (head, tail) = data.split_at(Self::SIZE - 2);
				let mut packed = head.to_vec();
				packed.extend_from_slice(&[0, 0]);
				let (mut table, _) = Self::unpack2(&packed)?;
				table.$count = u32::from_be_bytes([tail[0], tail[1], tail[2], tail[3]]);
				table.extended = true;
				Ok(table)
			}

			fn new_as(tag: Tag) -> Self {
				Self {
					extended: tag == Tag::new($upper),
					..Default::default()
				}
			}
		}

		impl Table for $ty {
			fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
				let _ = f;
				Ok(())
			}

			fn tag(&self) -> Tag {
				Font::locked_tag::<Self>(self.extended)
			}

			fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
				let glyf: &[u8; 4] = if self.extended { b"GLYF" } else { b"glyf" };
				let outline_loaded = [glyf, b"CFF ", b"CFF2"]
					.iter()
					.any(|t| font.is_loaded(Tag::new(t)));
				if font.recalc_bboxes && outline_loaded {
					self.recalc(font)?;
				}
				self.table_version = version_as_long(self.table_version)?;
				let mut data = Vec::with_capacity(Self::SIZE + 2);
				if self.extended {
					// `L` instead of `H` for the last field.
					let count = self.$count;
					self.$count = 0;
					let packed = self.pack(&mut data);
					self.$count = count;
					packed?;
					data.truncate(Self::SIZE - 2);
					data.put_u32(count);
				} else {
					self.pack(&mut data)?;
				}
				Ok(data)
			}

			fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
				for (name, value) in self.values() {
					if name == "tableVersion" {
						let v = format!("0x{:08x}", version_as_long(self.table_version)?);
						writer.simpletag(name, &[("value", &v)]);
					} else {
						writer.simpletag(name, &[("value", &value)]);
					}
					writer.newline();
				}
				Ok(())
			}

			fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
				let name = element.name.as_str();
				let value = element
					.attr("value")
					.ok_or_else(|| Error::ttx(format!("missing 'value' on <{name}>")))?;
				if name == "tableVersion" {
					let v = version_to_fixed(value)
						.ok_or_else(|| Error::value(format!("invalid version: {value:?}")))?;
					self.table_version = crate::binary::fit(v, name)?;
					return Ok(());
				}
				// The legacy alias `reserved0` of vhea's caretOffset.
				let name = if self.tag().0.eq_ignore_ascii_case(b"vhea") && name == "reserved0" {
					"caretOffset"
				} else {
					name
				};
				if !self.set_literal(name, &eval(value)?)? {
					return Err(Error::ttx(format!(
						"unknown '{}' field: {name}",
						self.tag()
					)));
				}
				Ok(())
			}

			fn clone_table(&self) -> Box<dyn Table> {
				Box::new(self.clone())
			}
		}
	};
}

header_table!(Hhea, b"hhea", b"HHEA", number_of_h_metrics);
header_table!(Vhea, b"vhea", b"VHEA", number_of_v_metrics);

impl Hhea {
	/// Recalculates `advanceWidthMax`, the side bearing minima and `xMaxExtent`
	/// from `hmtx` and the outlines (`recalc`).
	pub fn recalc(&mut self, font: &mut Font) -> Result<()> {
		let Some(r) = crate::tables::metrics::recalc_header(font, false, self.extended)? else {
			return Ok(());
		};
		self.advance_width_max = r.advance_max;
		self.min_left_side_bearing = r.min_start_side_bearing;
		self.min_right_side_bearing = r.min_end_side_bearing;
		self.x_max_extent = r.max_extent;
		Ok(())
	}
}

impl Vhea {
	/// Recalculates `advanceHeightMax`, the side bearing minima and `yMaxExtent`
	/// from `vmtx` and the outlines (`recalc`).
	pub fn recalc(&mut self, font: &mut Font) -> Result<()> {
		let Some(r) = crate::tables::metrics::recalc_header(font, true, self.extended)? else {
			return Ok(());
		};
		self.advance_height_max = r.advance_max;
		self.min_top_side_bearing = r.min_start_side_bearing;
		self.min_bottom_side_bearing = r.min_end_side_bearing;
		self.y_max_extent = r.max_extent;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn formats_match_upstream() {
		crate::sstruct::tests::check(Hhea::FORMAT_NAME, Hhea::FIELDS);
		crate::sstruct::tests::check(Vhea::FORMAT_NAME, Vhea::FIELDS);
	}
}
