//! `EBSC`: the Embedded Bitmap Scaling table (`fontTools.ttLib.tables.E_B_S_C_`).
//!
//! See <https://learn.microsoft.com/en-us/typography/opentype/spec/ebsc>.

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::fit;
use crate::sstruct::{F16Dot16, U8, U32, Value, eval};
use crate::table::{Table, TableType};
use crate::tables::eblc::SbitLineMetrics;
use crate::{Error, Font, Result};

crate::sstruct! {
	/// The header (`ebscHeaderFormat`).
	pub struct EbscHeader = "ebscHeaderFormat" {
		/// The table version.
		pub version: f64 = "version" as F16Dot16,
		/// The number of bitmap scale tables.
		pub num_sizes: u32 = "numSizes" as U32,
	}
}

crate::sstruct! {
	/// The sizes of a bitmap scale table (`bitmapScaleTableFormatPart2`).
	pub struct BitmapScalePart2 = "bitmapScaleTableFormatPart2" {
		/// Target horizontal pixels per em.
		pub ppem_x: u8 = "ppemX" as U8,
		/// Target vertical pixels per em.
		pub ppem_y: u8 = "ppemY" as U8,
		/// Horizontal pixels per em of the strike to scale.
		pub substitute_ppem_x: u8 = "substitutePpemX" as U8,
		/// Vertical pixels per em of the strike to scale.
		pub substitute_ppem_y: u8 = "substitutePpemY" as U8,
	}
}

/// A strike made by scaling another one (`BitmapScaleTable`).
///
/// LENIENT(L22): line metrics that a TTX document leaves out are zero.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BitmapScaleTable {
	/// Horizontal line metrics (`hori`).
	pub hori: SbitLineMetrics,
	/// Vertical line metrics (`vert`).
	pub vert: SbitLineMetrics,
	/// The sizes.
	pub part2: BitmapScalePart2,
}

impl BitmapScaleTable {
	const SIZE: usize = 2 * SbitLineMetrics::SIZE + BitmapScalePart2::SIZE;

	fn to_xml(&self, index: usize, writer: &mut XmlWriter) {
		writer.begintag_ordered("bitmapScaleTable", &[("index", &index)]);
		writer.newline();
		for (name, metrics) in [("hori", &self.hori), ("vert", &self.vert)] {
			writer.begintag_ordered("sbitLineMetrics", &[("direction", &name)]);
			writer.newline();
			for (field, value) in metrics.values() {
				writer.simpletag(field, &[("value", &value)]);
				writer.newline();
			}
			writer.endtag("sbitLineMetrics");
			writer.newline();
		}
		for (field, value) in self.part2.values() {
			writer.simpletag(field, &[("value", &value)]);
			writer.newline();
		}
		writer.endtag("bitmapScaleTable");
		writer.newline();
	}

	fn read_xml(element: &Element) -> Result<Self> {
		let mut table = BitmapScaleTable::default();
		for child in element.elements() {
			if child.name == "sbitLineMetrics" {
				let direction = child
					.attr("direction")
					.ok_or_else(|| Error::ttx("missing 'direction' on <sbitLineMetrics>"))?;
				if direction != "hori" && direction != "vert" {
					return Err(Error::ttlib(format!(
						"invalid sbitLineMetrics direction: {}",
						fontoxide_misc::py::str_repr(direction)
					)));
				}
				let mut metrics = SbitLineMetrics::default();
				for m in child.elements() {
					if SbitLineMetrics::FIELDS.iter().any(|f| f.name == m.name) {
						metrics.set_literal(&m.name, &value_literal(m)?)?;
					}
				}
				if direction == "hori" {
					table.hori = metrics;
				} else {
					table.vert = metrics;
				}
			} else if BitmapScalePart2::FIELDS
				.iter()
				.any(|f| f.name == child.name)
			{
				table
					.part2
					.set_literal(&child.name, &value_literal(child)?)?;
			}
		}
		Ok(table)
	}
}

fn value_literal(element: &Element) -> Result<fontoxide_misc::text_tools::Literal> {
	eval(
		element
			.attr("value")
			.ok_or_else(|| Error::ttx(format!("missing 'value' on <{}>", element.name)))?,
	)
}

/// Embedded Bitmap Scaling table (`table_E_B_S_C_`).
#[derive(Debug, Clone, PartialEq)]
pub struct Ebsc {
	/// The table version: a float when decompiled, as written in TTX otherwise.
	pub version: Value,
	/// The number of sizes as read; recalculated when compiling.
	pub num_sizes: i64,
	/// The bitmap scale tables (`bitmapScaleTables`).
	pub bitmap_scale_tables: Vec<BitmapScaleTable>,
}

impl Default for Ebsc {
	fn default() -> Self {
		Ebsc {
			version: Value::Float(2.0),
			num_sizes: 0,
			bitmap_scale_tables: Vec::new(),
		}
	}
}

impl TableType for Ebsc {
	const TAG: Tag = Tag::new(b"EBSC");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let (header, mut rest) = EbscHeader::unpack2(data)?;
		let mut tables = Vec::new();
		for _ in 0..header.num_sizes {
			if rest.len() < BitmapScaleTable::SIZE {
				// The first of the three structures that is too short fails.
				let size = if rest.len() < 2 * SbitLineMetrics::SIZE {
					SbitLineMetrics::SIZE
				} else {
					BitmapScalePart2::SIZE
				};
				return Err(Error::Struct(format!(
					"unpack requires a buffer of {size} bytes"
				)));
			}
			let (hori, r) = SbitLineMetrics::unpack2(rest)?;
			let (vert, r) = SbitLineMetrics::unpack2(r)?;
			let (part2, r) = BitmapScalePart2::unpack2(r)?;
			rest = r;
			tables.push(BitmapScaleTable { hori, vert, part2 });
		}
		Ok(Ebsc {
			version: Value::Float(header.version),
			num_sizes: i64::from(header.num_sizes),
			bitmap_scale_tables: tables,
		})
	}
}

impl Table for Ebsc {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		self.num_sizes = self.bitmap_scale_tables.len() as i64;
		let version = match &self.version {
			Value::Int(i) => *i as f64,
			Value::Float(f) => *f,
			Value::Bytes(_) => return Err(Error::value("invalid version")),
		};
		let mut out = Vec::new();
		EbscHeader {
			version,
			num_sizes: fit(self.num_sizes, "numSizes")?,
		}
		.pack(&mut out)?;
		for table in &self.bitmap_scale_tables {
			table.hori.pack(&mut out)?;
			table.vert.pack(&mut out)?;
			table.part2.pack(&mut out)?;
		}
		Ok(out)
	}

	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		writer.simpletag_ordered(
			"header",
			&[
				("version", &self.version),
				("numSizes", &self.bitmap_scale_tables.len()),
			],
		);
		writer.newline();
		for (index, table) in self.bitmap_scale_tables.iter().enumerate() {
			table.to_xml(index, writer);
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"header" => {
				let attr = |name: &str| {
					element
						.attr(name)
						.ok_or_else(|| Error::ttx(format!("missing '{name}' on <header>")))
				};
				let version = eval(attr("version")?)?;
				self.version = match version.as_int() {
					Some(i) => Value::Int(fit(i, "version")?),
					None => Value::Float(
						version
							.as_float()
							.ok_or_else(|| Error::value("invalid value for version"))?,
					),
				};
				self.num_sizes = fit(
					eval(attr("numSizes")?)?
						.as_int()
						.ok_or_else(|| Error::value("invalid value for numSizes"))?,
					"numSizes",
				)?;
			}
			"bitmapScaleTable" => {
				self.bitmap_scale_tables
					.push(BitmapScaleTable::read_xml(element)?);
			}
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
		check(EbscHeader::FORMAT_NAME, EbscHeader::FIELDS);
		check(BitmapScalePart2::FORMAT_NAME, BitmapScalePart2::FIELDS);
	}
}
