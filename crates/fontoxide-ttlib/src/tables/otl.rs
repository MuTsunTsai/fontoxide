//! Tables built from otData types (`BaseTTXConverter`): GDEF, GSUB, GPOS, BASE,
//! JSTF, MATH, STAT, the variation tables and the AAT tables that use otData.
//!
//! Each wraps the generated top-level type, which is available through
//! [`std::ops::Deref`].

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::generated::ot;
use crate::ot::OtReader;
use crate::ot::table::{OtTable, XmlCtx};
use crate::table::{Table, TableType, TableXml};
use crate::{Error, Font, Result};

macro_rules! otl_table {
	($(#[$meta:meta])* $name:ident, $tag:literal, $ty:ty) => {
		otl_table!(@impl $(#[$meta])* $name, $tag, $ty, |table: &mut $ty, element: &Element, font: &mut Font| {
			let mut ctx = XmlCtx::default();
			table.from_xml(element, font, &mut ctx)?;
			table.populate_defaults(&mut ctx)
		});
	};
	($(#[$meta:meta])* $name:ident, $tag:literal, $ty:ty, from_xml: $from_xml:expr) => {
		otl_table!(@impl $(#[$meta])* $name, $tag, $ty, $from_xml);
	};
	(@impl $(#[$meta:meta])* $name:ident, $tag:literal, $ty:ty, $from_xml:expr) => {
		$(#[$meta])*
		#[derive(Debug, Clone, PartialEq, Default)]
		pub struct $name {
			/// The table contents (`table`).
			pub table: $ty,
		}

		impl std::ops::Deref for $name {
			type Target = $ty;
			fn deref(&self) -> &$ty {
				&self.table
			}
		}

		impl std::ops::DerefMut for $name {
			fn deref_mut(&mut self) -> &mut $ty {
				&mut self.table
			}
		}

		impl TableType for $name {
			const TAG: Tag = Tag::new($tag);

			fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
				let mut reader = OtReader::new(data, Self::TAG);
				Ok($name {
					table: <$ty>::decompile(&mut reader, font)?,
				})
			}
		}

		impl Table for $name {
			fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
				self.table.remap_glyphs(f)
			}

			fn tag(&self) -> Tag {
				Self::TAG
			}

			fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
				crate::ot::overflow::compile_table(&mut self.table, Self::TAG, font)
			}

			fn clone_table(&self) -> Box<dyn Table> {
				Box::new(self.clone())
			}
		}

		impl TableXml for $name {
			fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
				self.table.to_xml2(writer, font)
			}

			fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
				let from_xml: fn(&mut $ty, &Element, &mut Font) -> Result<()> = $from_xml;
				from_xml(&mut self.table, element, font)
			}
		}
	};
}

otl_table!(
	/// Baseline table.
	Base,
	b"BASE",
	ot::BASE
);
otl_table!(
	/// Glyph Definition table.
	Gdef,
	b"GDEF",
	ot::GDEF
);
otl_table!(
	/// Glyph Positioning table.
	Gpos,
	b"GPOS",
	ot::GPOS
);
otl_table!(
	/// Glyph Substitution table.
	Gsub,
	b"GSUB",
	ot::GSUB
);
otl_table!(
	/// Horizontal Metrics Variations table.
	Hvar,
	b"HVAR",
	ot::HVAR
);
otl_table!(
	/// Justification table.
	Jstf,
	b"JSTF",
	ot::JSTF
);
otl_table!(
	/// Mathematical Typesetting table.
	Math,
	b"MATH",
	ot::MATH
);
otl_table!(
	/// Metrics Variations table.
	Mvar,
	b"MVAR",
	ot::MVAR
);
otl_table!(
	/// Style Attributes table.
	Stat,
	b"STAT",
	ot::STAT
);
otl_table!(
	/// VTT character group data.
	Tsic,
	b"TSIC",
	ot::TSIC
);
otl_table!(
	/// Variable Composites table.
	Varc,
	b"VARC",
	ot::VARC
);
otl_table!(
	/// Vertical Metrics Variations table.
	Vvar,
	b"VVAR",
	ot::VVAR
);
otl_table!(
	/// AAT anchor point table.
	Ankr,
	b"ankr",
	ot::Ankr
);
otl_table!(
	/// AAT baseline table.
	Bsln,
	b"bsln",
	ot::Bsln
);
otl_table!(
	/// AAT CID to glyph mapping table.
	Cidg,
	b"cidg",
	ot::Cidg
);
otl_table!(
	/// AAT feature name table.
	Feat,
	b"feat",
	ot::Feat
);
otl_table!(
	/// AAT glyph to CID mapping table.
	Gcid,
	b"gcid",
	ot::Gcid
);
otl_table!(
	/// AAT ligature caret table.
	Lcar,
	b"lcar",
	ot::Lcar
);
otl_table!(
	/// AAT glyph metamorphosis table.
	Mort,
	b"mort",
	ot::Mort
);
otl_table!(
	/// AAT extended glyph metamorphosis table.
	Morx,
	b"morx",
	ot::Morx
);
otl_table!(
	/// AAT optical bounds table.
	Opbd,
	b"opbd",
	ot::Opbd
);
otl_table!(
	/// AAT glyph properties table.
	Prop,
	b"prop",
	ot::Prop
);

// UPSTREAM-BUG(B8): the patch map is format-switching, but `toXML2` of the
// top-level table does not write its `Format`, so fontTools cannot read the TTX
// back: every element fails with AttributeError.
fn patch_map_from_xml(
	_table: &mut ot::PatchMap,
	_element: &Element,
	_font: &mut Font,
) -> Result<()> {
	Err(Error::ttlib("AttributeError: Format"))
}

otl_table!(
	/// Incremental Font Transfer patch map.
	Ift, b"IFT ", ot::PatchMap, from_xml: patch_map_from_xml
);
otl_table!(
	/// Incremental Font Transfer patch map, for extensions.
	Iftx, b"IFTX", ot::PatchMap, from_xml: patch_map_from_xml
);
