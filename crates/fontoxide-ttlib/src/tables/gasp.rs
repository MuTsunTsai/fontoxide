//! `gasp`: Grid-fitting and Scan-conversion Procedure table
//! (`fontTools.ttLib.tables._g_a_s_p`).

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes, fit};
use crate::sstruct::eval;
use crate::table::{Table, TableType};
use crate::{Error, Font, Result};

/// Use grid-fitting (`GASP_GRIDFIT`).
pub const GASP_GRIDFIT: u16 = 0x0001;
/// Use grayscale rendering (`GASP_DOGRAY`).
pub const GASP_DOGRAY: u16 = 0x0002;
/// Use grid-fitting with ClearType symmetric smoothing (`GASP_SYMMETRIC_GRIDFIT`).
pub const GASP_SYMMETRIC_GRIDFIT: u16 = 0x0004;
/// Use smoothing along multiple axes with ClearType (`GASP_SYMMETRIC_SMOOTHING`).
pub const GASP_SYMMETRIC_SMOOTHING: u16 = 0x0008;

/// Grid-fitting and Scan-conversion Procedure table (`table__g_a_s_p`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/gasp>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Gasp {
	/// The version read from the font. Compiling derives the version from the
	/// behaviors instead.
	pub version: u16,
	/// Behavior flags by upper PPEM limit (`gaspRange`).
	pub gasp_range: BTreeMap<u16, u16>,
}

impl TableType for Gasp {
	const TAG: Tag = Tag::new(b"gasp");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let mut r = Reader::new(data);
		let version = r.u16()?;
		let num_ranges = r.u16()?;
		if version > 1 {
			return Err(Error::Assertion(format!(
				"unknown 'gasp' format: {version}"
			)));
		}
		let mut gasp = Gasp {
			version,
			gasp_range: BTreeMap::new(),
		};
		for _ in 0..num_ranges {
			let max_ppem = r.u16()?;
			let behavior = r.u16()?;
			gasp.gasp_range.insert(max_ppem, behavior);
		}
		if r.remaining() > 0 {
			return Err(Error::Assertion("too much data".into()));
		}
		Ok(gasp)
	}
}

impl Table for Gasp {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let mut version = 0;
		let mut ranges = Vec::new();
		for (&max_ppem, &behavior) in &self.gasp_range {
			ranges.put_u16(max_ppem);
			ranges.put_u16(behavior);
			if behavior & !(GASP_GRIDFIT | GASP_DOGRAY) != 0 {
				version = 1;
			}
		}
		let mut data = Vec::with_capacity(4 + ranges.len());
		data.put_u16(version);
		data.put_u16(fit(self.gasp_range.len(), "numRanges")?);
		data.extend(ranges);
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		for (max_ppem, behavior) in &self.gasp_range {
			writer.simpletag_ordered(
				"gaspRange",
				&[("rangeMaxPPEM", max_ppem), ("rangeGaspBehavior", behavior)],
			);
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		if element.name != "gaspRange" {
			return Ok(());
		}
		let int = |name: &str| -> Result<u16> {
			let text = element
				.attr(name)
				.ok_or_else(|| Error::ttx(format!("missing '{name}' on <gaspRange>")))?;
			let v = eval(text)?
				.as_int()
				.ok_or_else(|| Error::value(format!("invalid {name}")))?;
			fit(v, name)
		};
		self.gasp_range
			.insert(int("rangeMaxPPEM")?, int("rangeGaspBehavior")?);
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}
