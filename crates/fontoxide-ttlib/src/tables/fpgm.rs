//! `fpgm` and `prep`: the Font Program and the Control Value Program
//! (`fontTools.ttLib.tables._f_p_g_m`, `_p_r_e_p`).

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::table::{Table, TableType, TableXml};
use crate::tables::tt_program::Program;
use crate::{Error, Font, Result};

macro_rules! program_table {
	($(#[$meta:meta])* $name:ident, $tag:literal, $class:literal) => {
		$(#[$meta])*
		#[derive(Debug, Clone, Default, PartialEq, Eq)]
		pub struct $name {
			/// The program (`program`), if set.
			pub program: Option<Program>,
		}

		impl $name {
			/// Whether the table has no program or an empty one (`not __bool__`).
			pub fn is_empty(&self) -> bool {
				self.program.as_ref().is_none_or(Program::is_empty)
			}
		}

		impl TableType for $name {
			const TAG: Tag = Tag::new($tag);

			fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
				Ok($name {
					program: Some(Program::from_bytecode(data)),
				})
			}
		}

		impl Table for $name {
			fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
				let _ = f;
				Ok(())
			}

			fn tag(&self) -> Tag {
				Self::TAG
			}

			fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
				Ok(self
					.program
					.as_ref()
					.map(|p| p.bytecode.clone())
					.unwrap_or_default())
			}

			fn clone_table(&self) -> Box<dyn Table> {
				Box::new(self.clone())
			}
		}

		impl TableXml for $name {
			fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
				let program = self.program.as_ref().ok_or_else(|| {
					Error::ttlib(concat!(
						"AttributeError: '",
						$class,
						"' object has no attribute 'program'"
					))
				})?;
				program.to_xml_with(writer, font.disassemble_instructions);
				Ok(())
			}

			fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
				self.program = Some(Program::from_xml(element)?);
				Ok(())
			}
		}
	};
}

program_table!(
	/// Font Program table (`table__f_p_g_m`): functions used by the glyph
	/// programs, run once when the font is first used.
	///
	/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/fpgm>.
	Fpgm, b"fpgm", "table__f_p_g_m"
);

program_table!(
	/// Control Value Program table (`table__p_r_e_p`): run whenever the size
	/// or transformation changes.
	///
	/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/prep>.
	Prep, b"prep", "table__p_r_e_p"
);
