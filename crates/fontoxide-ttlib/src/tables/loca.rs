//! `loca`: Index to Location table (`fontTools.ttLib.tables._l_o_c_a`).

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::table::{Table, TableType, TableXml};
use crate::tables::{Head, Maxp};
use crate::{Error, Font, Result};

/// Index to Location table (`table__l_o_c_a`): the offset of each glyph in
/// `glyf`, plus the end of the last one. It is rebuilt when `glyf` is compiled.
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/loca>.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Loca {
	/// The glyph offsets (`locations`), if set.
	pub locations: Option<Vec<u32>>,
	/// Whether this is `LOCA`, the beyond-64k companion of the table.
	pub extended: bool,
}

impl Loca {
	/// Sets the glyph offsets (`set`).
	pub fn set(&mut self, locations: Vec<u32>) {
		self.locations = Some(locations);
	}

	/// The glyph offsets; empty if not set.
	pub fn locations(&self) -> &[u32] {
		self.locations.as_deref().unwrap_or_default()
	}
}

impl TableType for Loca {
	const TAG: Tag = Tag::new(b"loca");
	const COMPANION: Option<Tag> = Some(Tag::new(b"LOCA"));

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		Self::decompile_as(Self::TAG, data, font)
	}

	fn new_as(tag: Tag) -> Self {
		Loca {
			locations: None,
			extended: tag == Tag::new(b"LOCA"),
		}
	}

	fn decompile_as(tag: Tag, data: &[u8], font: &mut Font) -> Result<Self> {
		let extended = tag == Tag::new(b"LOCA");
		let long = font.require::<Head>()?.index_to_loc_format != 0;
		let size = if long { 4 } else { 2 };
		if !data.len().is_multiple_of(size) {
			return Err(Error::value("bytes length not a multiple of item size"));
		}
		let locations: Vec<u32> = if long {
			data.as_chunks::<4>()
				.0
				.iter()
				.map(|&c| u32::from_be_bytes(c))
				.collect()
		} else {
			data.as_chunks::<2>()
				.0
				.iter()
				.map(|&c| 2 * u16::from_be_bytes(c) as u32)
				.collect()
		};
		let num_glyphs = font.require_locked::<Maxp>(extended)?.num_glyphs as usize;
		if locations.len() < num_glyphs + 1 {
			log::warn!(
				"corrupt 'loca' table, or wrong numGlyphs in 'maxp': {} {num_glyphs}",
				locations.len() as i64 - 1
			);
		}
		Ok(Loca {
			locations: Some(locations),
			extended,
		})
	}
}

impl Table for Loca {
	/// The offsets are positional and rebuilt from `glyf` when compiling.
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Ok(())
	}

	fn tag(&self) -> Tag {
		Font::locked_tag::<Self>(self.extended)
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		// UPSTREAM-BUG(B13): only missing locations are handled (as none); an
		// empty list fails in `max()`, e.g. a `loca` read from empty data.
		let max = match &self.locations {
			None => {
				self.set(Vec::new());
				0
			}
			Some(l) => *l
				.iter()
				.max()
				.ok_or_else(|| Error::value("max() arg is an empty sequence"))?,
		};
		let locations = self.locations();
		let short = max < 0x20000 && locations.iter().all(|l| l % 2 == 0);
		let mut data = Vec::with_capacity(locations.len() * if short { 2 } else { 4 });
		for &l in locations {
			if short {
				data.extend_from_slice(&((l / 2) as u16).to_be_bytes());
			} else {
				data.extend_from_slice(&l.to_be_bytes());
			}
		}
		font.require::<Head>()?.index_to_loc_format = if short { 0 } else { 1 };
		Ok(data)
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}

impl TableXml for Loca {
	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		writer.comment("The 'loca' table will be calculated by the compiler");
		writer.newline();
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		// `DefaultTable.fromXML`: raw data is accepted but unused.
		if element.name == "hexdata" {
			return Ok(());
		}
		Err(Error::ttlib(format!(
			"can't handle '{}' element",
			element.name
		)))
	}
}
