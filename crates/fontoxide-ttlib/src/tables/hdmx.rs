//! `hdmx` and `LTSH`: device metrics (`fontTools.ttLib.tables._h_d_m_x`,
//! `L_T_S_H_`).

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes, fit};
use crate::sstruct::{I32, U16, eval};
use crate::table::{Table, TableType};
use crate::tables::Maxp;
use crate::{Error, Font, Result};

crate::sstruct! {
	/// The `hdmx` header (`hdmxHeaderFormat`).
	pub struct HdmxHeader = "hdmxHeaderFormat" {
		/// 0.
		pub version: u16 = "version" as U16,
		/// Number of device records.
		pub num_records: u16 = "numRecords" as U16,
		/// Size of a device record, padded to a multiple of four.
		pub record_size: i32 = "recordSize" as I32,
	}
}

/// Horizontal Device Metrics table (`table__h_d_m_x`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/hdmx>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hdmx {
	/// Advance widths by ppem, then glyph ID (`hdmx`, keyed by glyph name in
	/// fontTools).
	pub hdmx: BTreeMap<u8, BTreeMap<u32, u8>>,
}

impl TableType for Hdmx {
	const TAG: Tag = Tag::new(b"hdmx");

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		let num_glyphs = font.require::<Maxp>()?.num_glyphs as usize;
		let glyph_count = font.glyph_order()?.len();
		let (header, mut rest) = HdmxHeader::unpack2(data)?;
		let mut hdmx = BTreeMap::new();
		for _ in 0..header.num_records {
			if rest.len() < 2 {
				return Err(Error::ttlib("index out of range"));
			}
			let ppem = rest[0];
			let widths = &rest[2..rest.len().min(2 + num_glyphs)];
			// Widths are looked up by glyph ID; IDs past the data have none.
			let map: BTreeMap<u32, u8> = (0..glyph_count.min(widths.len()))
				.map(|gid| (gid as u32, widths[gid]))
				.collect();
			hdmx.insert(ppem, map);
			rest = rest
				.get(header.record_size.max(0) as usize..)
				.unwrap_or_default();
		}
		if !rest.is_empty() {
			return Err(Error::Assertion("too much hdmx data".into()));
		}
		Ok(Hdmx { hdmx })
	}
}

impl Table for Hdmx {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		for widths in self.hdmx.values_mut() {
			*widths = crate::ot::aat::remap_keys(std::mem::take(widths), &mut *f)?;
		}
		Ok(())
	}

	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		let num_glyphs = font.require::<Maxp>()?.num_glyphs as usize;
		let order = font.glyph_order()?;
		let record_size = 4 * (2 + num_glyphs).div_ceil(4);
		let pad = record_size - 2 - num_glyphs;
		let mut data = Vec::new();
		HdmxHeader {
			version: 0,
			num_records: fit(self.hdmx.len(), "numRecords")?,
			record_size: fit(record_size, "recordSize")?,
		}
		.pack(&mut data)?;
		for (&ppem, widths) in &self.hdmx {
			let max = widths
				.values()
				.copied()
				.max()
				.ok_or_else(|| Error::value("max() arg is an empty sequence"))?;
			data.put_u8(ppem);
			data.put_u8(max);
			for gid in 0..order.len() as u32 {
				let width = widths
					.get(&gid)
					.ok_or_else(|| Error::GlyphNotFound(order.name(gid).into_owned()))?;
				data.put_u8(*width);
			}
			data.resize(data.len() + pad, 0);
		}
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		writer.begintag("hdmxData", &[]);
		writer.newline();
		let order = font.glyph_order()?;
		let mut names: Vec<(&str, u32)> = order
			.names()
			.iter()
			.enumerate()
			.map(|(i, n)| (n.as_str(), i as u32))
			.collect();
		names.sort();
		let width = names
			.iter()
			.map(|(n, _)| n.chars().count())
			.max()
			.unwrap_or(0);
		let row = |label: &str, values: &[String]| format!("{label:>width$}:{} ;", values.concat());
		let ppems: Vec<String> = self.hdmx.keys().map(|p| format!("{p:4}")).collect();
		writer.write(&row("ppem", &ppems));
		writer.newline();
		writer.newline();
		for (name, gid) in names {
			let mut values = Vec::with_capacity(self.hdmx.len());
			for widths in self.hdmx.values() {
				let w = widths
					.get(&gid)
					.ok_or_else(|| Error::ttlib("array index out of range"))?;
				values.push(format!("{w:4}"));
			}
			let label = name.replace(';', "\\x3b");
			writer.write(&row(&label, &values));
			writer.newline();
		}
		writer.endtag("hdmxData");
		writer.newline();
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		if element.name != "hdmxData" {
			return Ok(());
		}
		let bad = || Error::Assertion("illegal hdmx format".into());
		let int = |s: &str| -> Result<i128> {
			fontoxide_misc::py::parse_int(s)
				.ok_or_else(|| Error::value(format!("invalid literal for int(): {s:?}")))
		};
		let content = element.text();
		let mut lines = content.split(';');
		let top: Vec<&str> = lines
			.next()
			.unwrap_or_default()
			.split_whitespace()
			.collect();
		if top.first() != Some(&"ppem:") {
			return Err(bad());
		}
		let ppems: Vec<u8> = top[1..]
			.iter()
			.map(|s| int(s).and_then(|v| fit(v, "ppem")))
			.collect::<Result<_>>()?;
		self.hdmx = ppems.iter().map(|&p| (p, BTreeMap::new())).collect();
		for line in lines {
			let parts: Vec<&str> = line.split_whitespace().collect();
			let Some(first) = parts.first() else {
				continue;
			};
			let name = first.strip_suffix(':').ok_or_else(bad)?;
			let name = if name.contains('\\') {
				fontoxide_misc::text_tools::parse_string_body(name).ok_or_else(bad)?
			} else {
				name.to_owned()
			};
			if parts.len() - 1 != ppems.len() {
				return Err(bad());
			}
			let gid = font.glyph_id(&name)?;
			for (ppem, value) in ppems.iter().zip(&parts[1..]) {
				let v: u8 = fit(int(value)?, "width")?;
				if let Some(widths) = self.hdmx.get_mut(ppem) {
					widths.insert(gid, v);
				}
			}
		}
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}

/// Linear Threshold table (`table_L_T_S_H_`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/ltsh>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ltsh {
	/// The ppem from which each glyph scales linearly, by glyph ID (`yPels`).
	pub y_pels: BTreeMap<u32, u8>,
}

impl TableType for Ltsh {
	const TAG: Tag = Tag::new(b"LTSH");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let mut r = Reader::new(data);
		let version = r.u16()?;
		let num_glyphs = r.u16()? as usize;
		let rest = r.rest();
		if version != 0 {
			return Err(Error::Assertion(format!("unknown version: {version}")));
		}
		if num_glyphs == 0 {
			return Err(Error::value("integer modulo by zero"));
		}
		if rest.len() % num_glyphs >= 4 {
			return Err(Error::Assertion(
				"numGlyphs doesn't match data length".into(),
			));
		}
		if rest.len() < num_glyphs {
			return Err(Error::ttlib("array index out of range"));
		}
		Ok(Ltsh {
			y_pels: (0..num_glyphs).map(|i| (i as u32, rest[i])).collect(),
		})
	}
}

impl Table for Ltsh {
	/// The values are by position; fontTools does not subset `LTSH` either.
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Err(Error::ttlib(
			"remapping the glyphs of 'LTSH' is not supported",
		))
	}

	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let num_glyphs = self.y_pels.len();
		let mut y_pels = vec![0u8; num_glyphs];
		for (&gid, &v) in &self.y_pels {
			*y_pels
				.get_mut(gid as usize)
				.ok_or_else(|| Error::ttlib("list assignment index out of range"))? = v;
		}
		let mut data = Vec::with_capacity(4 + num_glyphs);
		data.put_u16(0);
		data.put_u16(fit(num_glyphs, "numGlyphs")?);
		data.extend(y_pels);
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let order = font.glyph_order()?;
		let mut entries: Vec<(String, u8)> = self
			.y_pels
			.iter()
			.map(|(&gid, &v)| (order.name(gid).into_owned(), v))
			.collect();
		entries.sort();
		for (name, value) in &entries {
			writer.simpletag("yPel", &[("name", name), ("value", value)]);
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		if element.name != "yPel" {
			return Ok(());
		}
		let name = element
			.attr("name")
			.ok_or_else(|| Error::ttx("missing 'name' on <yPel>"))?;
		let value = element
			.attr("value")
			.ok_or_else(|| Error::ttx("missing 'value' on <yPel>"))?;
		let v = eval(value)?
			.as_int()
			.ok_or_else(|| Error::value("invalid yPel value"))?;
		let gid = font.glyph_id(name)?;
		self.y_pels.insert(gid, fit(v, "yPel")?);
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn format_matches_upstream() {
		crate::sstruct::tests::check(HdmxHeader::FORMAT_NAME, HdmxHeader::FIELDS);
	}
}
