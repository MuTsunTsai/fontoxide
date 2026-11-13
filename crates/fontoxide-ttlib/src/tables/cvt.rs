//! `cvt `: Control Value Table (`fontTools.ttLib.tables._c_v_t`).

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{WriteBytes, fit};
use crate::sstruct::eval;
use crate::table::{Table, TableType, TableXml};
use crate::{Error, Font, Result};

/// Control Value Table (`table__c_v_t`): values referenced by TrueType
/// instructions.
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/cvt>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cvt {
	/// The control values (`values`).
	pub values: Vec<i16>,
}

impl TableType for Cvt {
	const TAG: Tag = Tag::new(b"cvt ");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		if data.len() % 2 == 1 {
			return Err(Error::value("bytes length not a multiple of item size"));
		}
		Ok(Cvt {
			values: data
				.as_chunks::<2>()
				.0
				.iter()
				.map(|&c| i16::from_be_bytes(c))
				.collect(),
		})
	}
}

impl Table for Cvt {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Ok(())
	}

	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let mut data = Vec::with_capacity(2 * self.values.len());
		for &v in &self.values {
			data.put_i16(v);
		}
		Ok(data)
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}

impl TableXml for Cvt {
	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		for (i, value) in self.values.iter().enumerate() {
			writer.simpletag("cv", &[("value", value), ("index", &i)]);
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		if element.name != "cv" {
			return Ok(());
		}
		let int = |name: &str| -> Result<i128> {
			let text = element
				.attr(name)
				.ok_or_else(|| Error::ttx(format!("missing '{name}' on <cv>")))?;
			eval(text)?
				.as_int()
				.ok_or_else(|| Error::value(format!("invalid {name}")))
		};
		let index: usize = fit(int("index")?, "index")?;
		let value: i16 = fit(int("value")?, "value")?;
		if self.values.len() <= index {
			self.values.resize(index + 1, 0);
		}
		self.values[index] = value;
		Ok(())
	}
}
