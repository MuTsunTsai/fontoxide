//! The table interface (`DefaultTable`) and the registry that maps tags to table
//! implementations (`getTableClass`).

use std::any::Any;
use std::fmt;

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::{Error, Font, Result};

/// Upcasting to [`Any`], implemented for every sized `'static` type.
pub trait AsAny: Any {
	/// `self` as `&dyn Any`.
	fn as_any(&self) -> &dyn Any;
	/// `self` as `&mut dyn Any`.
	fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any> AsAny for T {
	fn as_any(&self) -> &dyn Any {
		self
	}
	fn as_any_mut(&mut self) -> &mut dyn Any {
		self
	}
}

/// A font table (`DefaultTable` and its subclasses).
///
/// The methods take the [`Font`] because, as in fontTools, tables consult and even
/// modify each other: `hmtx` needs `hhea` to decompile, compiling `maxp` updates the
/// bounding box in `head`, and so on. While one of these methods runs, the table
/// itself is detached from the font.
pub trait Table: AsAny + fmt::Debug + Send + Sync {
	/// The table tag (`tableTag`).
	fn tag(&self) -> Tag;

	/// Compiles the table to binary (`compile`).
	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>>;

	/// Writes the table's TTX content, without the enclosing table element
	/// (`toXML`).
	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()>;

	/// Reads one child element of the table's TTX element (`fromXML`).
	#[allow(clippy::wrong_self_convention)]
	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()>;

	/// A deep copy.
	fn clone_table(&self) -> Box<dyn Table>;

	/// Whether this is a plain [`DefaultTable`], which TTX marks as `raw`.
	fn is_raw(&self) -> bool {
		false
	}

	/// The error message, if decompilation failed and the table was kept as raw
	/// data (`ERROR`).
	fn decompile_error(&self) -> Option<&str> {
		None
	}
}

impl Clone for Box<dyn Table> {
	fn clone(&self) -> Self {
		self.clone_table()
	}
}

/// A table type with a fixed tag, which [`Font::get`] can return.
pub trait TableType: Table + Sized {
	/// The tag of this table type.
	const TAG: Tag;

	/// Parses the table (`decompile`).
	fn decompile(data: &[u8], font: &mut Font) -> Result<Self>;
}

/// A table kept as raw bytes (`DefaultTable`): used for tables fontoxide does not
/// implement, and for tables that failed to decompile when decompile errors are
/// ignored.
#[derive(Clone, PartialEq, Eq)]
pub struct DefaultTable {
	/// The table tag.
	pub tag: Tag,
	/// The table data.
	pub data: Vec<u8>,
	/// The decompilation error, if this table replaces one that failed (`ERROR`).
	pub error: Option<String>,
}

impl fmt::Debug for DefaultTable {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("DefaultTable")
			.field("tag", &self.tag)
			.field("len", &self.data.len())
			.field("error", &self.error)
			.finish()
	}
}

impl DefaultTable {
	/// A raw table with the given data.
	pub fn new(tag: Tag, data: Vec<u8>) -> Self {
		DefaultTable {
			tag,
			data,
			error: None,
		}
	}
}

impl Table for DefaultTable {
	fn tag(&self) -> Tag {
		self.tag
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		Ok(self.data.clone())
	}

	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		if let Some(error) = &self.error {
			writer.comment("An error occurred during the decompilation of this table");
			writer.newline();
			writer.comment(error);
			writer.newline();
		}
		writer.begintag("hexdata", &[]);
		writer.newline();
		writer.dumphex(&self.data);
		writer.endtag("hexdata");
		writer.newline();
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		if element.name != "hexdata" {
			return Err(Error::ttlib(format!(
				"can't handle '{}' element",
				element.name
			)));
		}
		self.data = read_hex(element)?;
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn is_raw(&self) -> bool {
		true
	}

	fn decompile_error(&self) -> Option<&str> {
		self.error.as_deref()
	}
}

/// `readHex(content)` for an element.
pub fn read_hex(element: &Element) -> Result<Vec<u8>> {
	fontoxide_misc::text_tools::read_hex(element.text_chunks())
		.ok_or_else(|| Error::value("invalid hex data"))
}

macro_rules! registry {
	($($tag:literal => $ty:ty),* $(,)?) => {
		/// Decompiles a table with the implementation registered for its tag, or as a
		/// [`DefaultTable`] if there is none (`getTableClass(tag)().decompile(...)`).
		pub fn decompile_table(tag: Tag, data: &[u8], font: &mut Font) -> Result<Box<dyn Table>> {
			match &tag.0 {
				$($tag => Ok(Box::new(<$ty as TableType>::decompile(data, font)?)),)*
				_ => Ok(Box::new(DefaultTable::new(tag, data.to_vec()))),
			}
		}

		/// An empty table of the type registered for `tag`, for filling from TTX
		/// (`newTable`).
		pub fn new_table(tag: Tag) -> Box<dyn Table> {
			match &tag.0 {
				$($tag => Box::new(<$ty>::default()),)*
				_ => Box::new(DefaultTable::new(tag, Vec::new())),
			}
		}

		/// Whether fontoxide implements the table (rather than keeping it raw).
		pub fn is_implemented(tag: Tag) -> bool {
			matches!(&tag.0, $($tag)|*)
		}
	};
}

registry! {
	b"cvt " => crate::tables::Cvt,
	b"BASE" => crate::tables::otl::Base,
	b"GDEF" => crate::tables::otl::Gdef,
	b"GPOS" => crate::tables::otl::Gpos,
	b"GSUB" => crate::tables::otl::Gsub,
	b"HVAR" => crate::tables::otl::Hvar,
	b"IFT " => crate::tables::otl::Ift,
	b"IFTX" => crate::tables::otl::Iftx,
	b"JSTF" => crate::tables::otl::Jstf,
	b"MATH" => crate::tables::otl::Math,
	b"MVAR" => crate::tables::otl::Mvar,
	b"STAT" => crate::tables::otl::Stat,
	b"TSIC" => crate::tables::otl::Tsic,
	b"VARC" => crate::tables::otl::Varc,
	b"VVAR" => crate::tables::otl::Vvar,
	b"ankr" => crate::tables::otl::Ankr,
	b"bsln" => crate::tables::otl::Bsln,
	b"cidg" => crate::tables::otl::Cidg,
	b"feat" => crate::tables::otl::Feat,
	b"gcid" => crate::tables::otl::Gcid,
	b"lcar" => crate::tables::otl::Lcar,
	b"mort" => crate::tables::otl::Mort,
	b"morx" => crate::tables::otl::Morx,
	b"opbd" => crate::tables::otl::Opbd,
	b"prop" => crate::tables::otl::Prop,
	b"FFTM" => crate::tables::Fftm,
	b"avar" => crate::tables::Avar,
	b"COLR" => crate::tables::Colr,
	b"CPAL" => crate::tables::Cpal,
	b"fvar" => crate::tables::Fvar,
	b"gasp" => crate::tables::Gasp,
	b"cmap" => crate::tables::Cmap,
	b"hdmx" => crate::tables::Hdmx,
	b"head" => crate::tables::Head,
	b"LTSH" => crate::tables::Ltsh,
	b"hhea" => crate::tables::Hhea,
	b"hmtx" => crate::tables::Hmtx,
	b"ltag" => crate::tables::Ltag,
	b"maxp" => crate::tables::Maxp,
	b"name" => crate::tables::Name,
	b"OS/2" => crate::tables::Os2,
	b"post" => crate::tables::Post,
	b"vhea" => crate::tables::Vhea,
	b"vmtx" => crate::tables::Vmtx,
	b"VORG" => crate::tables::Vorg,
}

/// The tables that must be compiled before `tag` (`dependencies`), as declared by
/// the fontTools table classes.
pub fn dependencies(tag: Tag) -> &'static [Tag] {
	crate::generated::tables::dependencies(tag)
}
