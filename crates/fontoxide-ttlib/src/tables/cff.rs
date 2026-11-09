//! `CFF ` and `CFF2` tables (`fontTools.ttLib.tables.C_F_F_`, `C_F_F__2`).

use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::cff::compile::CompileContext;
use crate::cff::font::CffFontSet;
use crate::table::{Table, TableType};
use crate::{Error, Font, Result, Tag};

macro_rules! cff_table {
	($name:ident, $tag:literal, $cff2:literal, $doc:literal) => {
		#[doc = $doc]
		#[derive(Debug, Clone, Default, PartialEq)]
		pub struct $name {
			/// The font set (`cff`).
			pub cff: CffFontSet,
		}

		impl TableType for $name {
			const TAG: Tag = Tag::new($tag);

			fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
				let order = if $cff2 {
					Some(font.glyph_order()?.names().to_vec())
				} else {
					None
				};
				let cff = CffFontSet::decompile(data, $cff2, order.as_deref())?;
				if cff.font_names.len() != 1 {
					return Err(Error::Assertion(
						"can't deal with multi-font CFF tables.".into(),
					));
				}
				Ok($name { cff })
			}
		}

		impl Table for $name {
			/// The charstrings are kept by glyph name, as in fontTools, so they
			/// follow the glyph order by name: dropping glyphs is up to the caller.
			fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
				let _ = f;
				Ok(())
			}

			fn tag(&self) -> Tag {
				Self::TAG
			}

			fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
				let order = font.glyph_order()?;
				let ctx = CompileContext {
					glyph_order: order.names(),
					recalc_bboxes: font.recalc_bboxes,
					has_varc: font.contains(Tag::new(b"VARC")),
				};
				self.cff.compile($cff2, &ctx)
			}

			fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
				self.cff.to_xml(writer)
			}

			fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
				self.cff.from_xml(element)
			}

			fn clone_table(&self) -> Box<dyn Table> {
				Box::new(self.clone())
			}
		}
	};
}

cff_table!(Cff, b"CFF ", false, "The `CFF ` table: a CFF font set.");
cff_table!(Cff2, b"CFF2", true, "The `CFF2` table: a CFF2 font set.");
