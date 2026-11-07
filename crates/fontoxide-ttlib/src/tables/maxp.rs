//! `maxp`: Maximum Profile table (`fontTools.ttLib.tables._m_a_x_p`).

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::WriteBytes;
use crate::sstruct::{I32, U16, eval};
use crate::table::{Table, TableType};
use crate::{Error, Font, Result};

crate::sstruct! {
	/// The version 0.5 part of `maxp` (`maxpFormat_0_5`). Only used to check the
	/// layout against fontTools; [`Maxp`] holds the fields.
	pub struct MaxpFormat05 = "maxpFormat_0_5" {
		/// Table version.
		pub table_version: i32 = "tableVersion" as I32,
		/// Number of glyphs.
		pub num_glyphs: u16 = "numGlyphs" as U16,
	}
}

crate::sstruct! {
	/// The fields that version 1.0 adds (`maxpFormat_1_0_add`).
	pub struct MaxpV1 = "maxpFormat_1_0_add" {
		/// Maximum points in a simple glyph.
		pub max_points: u16 = "maxPoints" as U16,
		/// Maximum contours in a simple glyph.
		pub max_contours: u16 = "maxContours" as U16,
		/// Maximum points in a composite glyph.
		pub max_composite_points: u16 = "maxCompositePoints" as U16,
		/// Maximum contours in a composite glyph.
		pub max_composite_contours: u16 = "maxCompositeContours" as U16,
		/// 1 without twilight zone instructions, 2 with.
		pub max_zones: u16 = "maxZones" as U16,
		/// Maximum points used in the twilight zone.
		pub max_twilight_points: u16 = "maxTwilightPoints" as U16,
		/// Number of storage area locations.
		pub max_storage: u16 = "maxStorage" as U16,
		/// Number of FDEFs.
		pub max_function_defs: u16 = "maxFunctionDefs" as U16,
		/// Number of IDEFs.
		pub max_instruction_defs: u16 = "maxInstructionDefs" as U16,
		/// Maximum stack depth.
		pub max_stack_elements: u16 = "maxStackElements" as U16,
		/// Maximum byte count of glyph instructions.
		pub max_size_of_instructions: u16 = "maxSizeOfInstructions" as U16,
		/// Maximum number of components at the top level of a composite glyph.
		pub max_component_elements: u16 = "maxComponentElements" as U16,
		/// Maximum levels of recursion in composite glyphs.
		pub max_component_depth: u16 = "maxComponentDepth" as U16,
	}
}

/// Maximum Profile table (`table__m_a_x_p`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/maxp>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Maxp {
	/// 0x00005000 for CFF fonts, 0x00010000 for TrueType.
	pub table_version: u32,
	/// Number of glyphs. Set from the glyph order when compiling.
	pub num_glyphs: u16,
	/// The version 1.0 fields. fontTools leaves them unset for version 0.5; here
	/// they are kept (as zeros) but not written.
	pub v1: MaxpV1,
}

const VERSION_0_5: u32 = 0x0000_5000;

impl TableType for Maxp {
	const TAG: Tag = Tag::new(b"maxp");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let header_size = 6;
		// int.from_bytes() on short data gives whatever is there.
		let int = |bytes: &[u8]| bytes.iter().fold(0u64, |acc, &b| acc << 8 | b as u64);
		let table_version = int(&data[..data.len().min(4)]) as u32;
		let num_glyphs = int(data.get(4..data.len().min(header_size)).unwrap_or_default()) as u16;
		let mut rest = data.get(header_size..).unwrap_or_default();
		let mut v1 = MaxpV1::default();
		if table_version != VERSION_0_5 {
			let (parsed, r) = MaxpV1::unpack2(rest)?;
			v1 = parsed;
			rest = r;
		}
		if !rest.is_empty() {
			return Err(Error::Assertion("len(data) == 0".into()));
		}
		Ok(Maxp {
			table_version,
			num_glyphs,
			v1,
		})
	}
}

impl Table for Maxp {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		let glyf = Tag::new(b"glyf");
		if font.contains(glyf) && font.is_loaded(glyf) && font.recalc_bboxes {
			self.recalc(font)?;
		}
		self.num_glyphs = crate::binary::fit(font.glyph_order()?.len(), "numGlyphs")?;
		if self.table_version != VERSION_0_5 {
			self.table_version = 0x0001_0000;
		}
		let mut data = Vec::new();
		data.put_u32(self.table_version);
		data.put_u16(self.num_glyphs);
		if self.table_version == 0x0001_0000 {
			self.v1.pack(&mut data)?;
		}
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		let v1 = self.table_version != VERSION_0_5;
		if v1 {
			writer.comment("Most of this table will be recalculated by the compiler");
			writer.newline();
		}
		let version = format!("{:#x}", self.table_version);
		writer.simpletag("tableVersion", &[("value", &version)]);
		writer.newline();
		writer.simpletag("numGlyphs", &[("value", &self.num_glyphs)]);
		writer.newline();
		if v1 {
			for (name, value) in self.v1.values() {
				writer.simpletag(name, &[("value", &value)]);
				writer.newline();
			}
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		let name = element.name.as_str();
		let value = element
			.attr("value")
			.ok_or_else(|| Error::ttx(format!("missing 'value' on <{name}>")))?;
		let lit = eval(value)?;
		match name {
			"tableVersion" => {
				let v = lit
					.as_int()
					.ok_or_else(|| Error::value("invalid tableVersion"))?;
				self.table_version = crate::binary::fit(v, name)?;
			}
			"numGlyphs" => {
				let v = lit
					.as_int()
					.ok_or_else(|| Error::value("invalid numGlyphs"))?;
				self.num_glyphs = crate::binary::fit(v, name)?;
			}
			_ => {
				if !self.v1.set_literal(name, &lit)? {
					return Err(Error::ttx(format!("unknown 'maxp' field: {name}")));
				}
			}
		}
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}

impl Maxp {
	/// Recalculates the font bounding box and the outline maxima from `glyf`, and
	/// updates `head` (`recalc`).
	///
	/// Requires the `glyf` table implementation; until it exists this is a no-op.
	pub fn recalc(&mut self, font: &mut Font) -> Result<()> {
		crate::tables::metrics::recalc_maxp(self, font)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn formats_match_upstream() {
		crate::sstruct::tests::check(MaxpFormat05::FORMAT_NAME, MaxpFormat05::FIELDS);
		crate::sstruct::tests::check(MaxpV1::FORMAT_NAME, MaxpV1::FIELDS);
	}
}
