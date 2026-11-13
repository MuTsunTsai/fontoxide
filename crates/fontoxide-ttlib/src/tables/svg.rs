//! `SVG `: Scalable Vector Graphics table (`fontTools.ttLib.tables.S_V_G_`).

use std::collections::HashMap;
use std::io::{Read, Write};

use fontoxide_misc::Tag;
use fontoxide_misc::py;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes, fit};
use crate::table::{Table, TableType, TableXml};
use crate::tables::eblc::{clip, required_attr};
use crate::{Error, Font, Result};

const HEADER_SIZE: u64 = 10;
const ENTRY_SIZE: u64 = 12;

/// An SVG document (`SVGDocument`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SvgDocument {
	/// The document (`data`).
	pub data: String,
	/// `startGlyphID`.
	pub start_glyph_id: u16,
	/// `endGlyphID`.
	pub end_glyph_id: u16,
	/// Whether it is compressed in the font (`compressed`).
	pub compressed: bool,
}

/// Scalable Vector Graphics table (`table_S_V_G_`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/svg>.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Svg {
	/// The documents (`docList`), if set.
	pub doc_list: Option<Vec<SvgDocument>>,
	/// Compress all documents when compiling (`compressed`).
	pub compressed: bool,
}

fn no_doc_list() -> Error {
	Error::ttlib("AttributeError: 'table_S_V_G_' object has no attribute 'docList'")
}

fn slice_of(data: &[u8], start: u64, n: u64) -> Result<Reader<'_>> {
	let slice = clip(data, start, start.saturating_add(n));
	if (slice.len() as u64) < n {
		return Err(Error::Struct(format!(
			"unpack requires a buffer of {n} bytes"
		)));
	}
	Ok(Reader::new(slice))
}

/// `GzipFile.read`, which reads all the members of the file.
fn gunzip(data: &[u8]) -> Result<Vec<u8>> {
	let mut out = Vec::new();
	flate2::read::MultiGzDecoder::new(data)
		.read_to_end(&mut out)
		.map_err(|e| Error::ttlib(format!("gzip error: {e}")))?;
	Ok(out)
}

/// `GzipFile(None, "w", fileobj, mtime=0)` with the default compression level.
///
/// LENIENT(L25): the header is the same as fontTools', but the deflate stream
/// comes from another implementation than zlib.
fn gzip(data: &[u8]) -> Vec<u8> {
	let mut encoder = flate2::GzBuilder::new()
		.mtime(0)
		.operating_system(255)
		.write(Vec::new(), flate2::Compression::new(9));
	// Writing to a Vec cannot fail.
	let _ = encoder.write_all(data);
	encoder.finish().unwrap_or_default()
}

const GZIP_MAGIC: &[u8] = b"\x1f\x8b";

impl TableType for Svg {
	const TAG: Tag = Tag::new(b"SVG ");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let mut r = slice_of(data, 0, HEADER_SIZE)?;
		let version = r.u16()?;
		let offset = u64::from(r.u32()?);
		if version != 0 {
			log::warn!("Unknown SVG table version '{version}'. Decompiling as version 0.");
		}
		let num_entries = slice_of(data, offset, 2)?.u16()?;
		let entries_at = offset.saturating_add(2);
		let mut docs = Vec::with_capacity(usize::from(num_entries));
		let mut entries = Vec::with_capacity(usize::from(num_entries));
		for i in 0..u64::from(num_entries) {
			let mut r = slice_of(data, entries_at.saturating_add(i * ENTRY_SIZE), ENTRY_SIZE)?;
			entries.push((r.u16()?, r.u16()?, r.u32()?, r.u32()?));
		}
		for (start_glyph_id, end_glyph_id, doc_offset, doc_length) in entries {
			let start = u64::from(doc_offset) + offset;
			let doc = clip(data, start, start + u64::from(doc_length));
			let compressed = doc.starts_with(GZIP_MAGIC);
			let doc = if compressed {
				gunzip(doc)?
			} else {
				doc.to_vec()
			};
			let data = String::from_utf8(doc)
				.map_err(|e| Error::ttlib(format!("UnicodeDecodeError: {e}")))?;
			docs.push(SvgDocument {
				data,
				start_glyph_id,
				end_glyph_id,
				compressed,
			});
		}
		Ok(Svg {
			doc_list: Some(docs),
			compressed: false,
		})
	}
}

impl Table for Svg {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let docs = self.doc_list.as_ref().ok_or_else(no_doc_list)?;
		let num_entries: u16 = fit(docs.len(), "numEntries")?;
		let mut out = Vec::new();
		out.put_u16(0);
		out.put_u32(HEADER_SIZE as u32);
		out.put_u32(0);
		out.put_u16(num_entries);
		let mut cur_offset = 2 + ENTRY_SIZE * u64::from(num_entries);
		let mut seen = HashMap::<Vec<u8>, u32>::new();
		let mut doc_data = Vec::new();
		for doc in docs {
			let mut bytes = doc.data.clone().into_bytes();
			if (self.compressed || doc.compressed) && !bytes.starts_with(GZIP_MAGIC) {
				let gzipped = gzip(&bytes);
				if gzipped.len() < bytes.len() {
					bytes = gzipped;
				}
			}
			let length: u32 = fit(bytes.len(), "svgDocLength")?;
			let offset = match seen.get(&bytes) {
				Some(&offset) => offset,
				None => {
					let offset: u32 = fit(cur_offset, "svgDocOffset")?;
					cur_offset += bytes.len() as u64;
					doc_data.extend_from_slice(&bytes);
					seen.insert(bytes, offset);
					offset
				}
			};
			out.put_u16(doc.start_glyph_id);
			out.put_u16(doc.end_glyph_id);
			out.put_u32(offset);
			out.put_u32(length);
		}
		out.extend(doc_data);
		Ok(out)
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn remap_glyphs(&mut self, _f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		// The documents are subset with the glyphs, which sets the IDs.
		Ok(())
	}
}

impl TableXml for Svg {
	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		for doc in self.doc_list.as_ref().ok_or_else(no_doc_list)? {
			if doc.compressed {
				writer.begintag(
					"svgDoc",
					&[
						("startGlyphID", &doc.start_glyph_id),
						("endGlyphID", &doc.end_glyph_id),
						("compressed", &1),
					],
				);
			} else {
				writer.begintag(
					"svgDoc",
					&[
						("startGlyphID", &doc.start_glyph_id),
						("endGlyphID", &doc.end_glyph_id),
					],
				);
			}
			writer.newline();
			writer.writecdata(&doc.data);
			writer.newline();
			writer.endtag("svgDoc");
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		if element.name != "svgDoc" {
			log::warn!("Unknown {} {:?}", element.name, element.text());
			return Ok(());
		}
		let int = |name: &str| -> Result<u16> {
			let value = required_attr(element, name)?;
			let value = py::parse_int(value).ok_or_else(|| {
				Error::value(format!("invalid literal for int() with base 10: '{value}'"))
			})?;
			fit(value, name)
		};
		let compressed = match element.attr("compressed") {
			Some(value) => {
				crate::sstruct::eval(value)?
					.as_int()
					.ok_or_else(|| Error::value("invalid value for compressed"))?
					!= 0
			}
			None => false,
		};
		let doc = SvgDocument {
			data: py::strip(&element.text()).to_owned(),
			start_glyph_id: int("startGlyphID")?,
			end_glyph_id: int("endGlyphID")?,
			compressed,
		};
		self.doc_list.get_or_insert_default().push(doc);
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use fontoxide_misc::xml_reader;

	use super::*;

	const DOC0: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" version=\"1.1\"><defs><rect x=\"100\" y=\"-200\" width=\"300\" height=\"400\" id=\"p1\"/></defs><g id=\"glyph1\"><use xlink:href=\"#p1\" fill=\"#red\"/></g><g id=\"glyph2\"><use xlink:href=\"#p1\" fill=\"#blue\"/></g><g id=\"glyph4\"><use xlink:href=\"#p1\" fill=\"#green\"/></g></svg>";
	const DOC1: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\"><g id=\"glyph3\"><path d=\"M0,0 L100,0 L50,100 Z\" fill=\"#red\"/><path d=\"M10,10 L110,10 L60,110 Z\" fill=\"#blue\"/><path d=\"M20,20 L120,20 L70,120 Z\" fill=\"#green\"/></g></svg>";

	/// `OTSVG_DATA`. fontTools' test compresses with zlib; here with the same
	/// implementation as the table (L25).
	fn data() -> Vec<u8> {
		let compressed = gzip(DOC1.as_bytes());
		let mut out = Vec::new();
		out.put_u16(0);
		out.put_u32(10);
		out.put_u32(0);
		out.put_u16(3);
		out.put_u16(1);
		out.put_u16(2);
		out.put_u32(0x26);
		out.put_u32(DOC0.len() as u32);
		out.put_u16(3);
		out.put_u16(3);
		out.put_u32(0x26 + DOC0.len() as u32);
		out.put_u32(compressed.len() as u32);
		out.put_u16(4);
		out.put_u16(4);
		out.put_u32(0x26);
		out.put_u32(DOC0.len() as u32);
		out.extend_from_slice(DOC0.as_bytes());
		out.extend(compressed);
		out
	}

	fn ttx() -> Vec<String> {
		vec![
			"<svgDoc endGlyphID=\"2\" startGlyphID=\"1\">".to_owned(),
			format!("  <![CDATA[{DOC0}]]>"),
			"</svgDoc>".to_owned(),
			"<svgDoc compressed=\"1\" endGlyphID=\"3\" startGlyphID=\"3\">".to_owned(),
			format!("  <![CDATA[{DOC1}]]>"),
			"</svgDoc>".to_owned(),
			"<svgDoc endGlyphID=\"4\" startGlyphID=\"4\">".to_owned(),
			format!("  <![CDATA[{DOC0}]]>"),
			"</svgDoc>".to_owned(),
		]
	}

	fn get_xml(table: &Svg, font: &mut Font) -> Result<Vec<String>> {
		let mut writer = XmlWriter::with_options("  ", "\n");
		table.to_xml(&mut writer, font)?;
		let text = writer.into_string();
		let body = text.split_once('\n').map_or(text.as_str(), |(_, b)| b);
		Ok(body.lines().map(str::to_owned).collect())
	}

	fn parse_ttx(lines: &[String], table: &mut Svg, font: &mut Font) -> Result<()> {
		let doc = format!("<root>{}</root>", lines.join("\n"));
		let root = xml_reader::parse(&doc).map_err(|e| Error::ttx(e.to_string()))?;
		for element in root.elements() {
			table.from_xml(element, font)?;
		}
		Ok(())
	}

	#[test]
	fn decompile_and_compile() -> Result<()> {
		let mut font = Font::new();
		let mut table = Svg::decompile(&data(), &mut font)?;
		assert_eq!(table.compile(&mut font)?, data());
		Ok(())
	}

	#[test]
	fn decompile_and_dump_ttx() -> Result<()> {
		let mut font = Font::new();
		let table = Svg::decompile(&data(), &mut font)?;
		assert_eq!(get_xml(&table, &mut font)?, ttx());
		Ok(())
	}

	#[test]
	fn load_from_ttx_and_compile() -> Result<()> {
		let mut font = Font::new();
		let mut table = Svg::default();
		parse_ttx(&ttx(), &mut table, &mut font)?;
		assert_eq!(table.compile(&mut font)?, data());
		Ok(())
	}

	#[test]
	fn round_trip_ttx() -> Result<()> {
		let mut font = Font::new();
		let mut table = Svg::default();
		parse_ttx(&ttx(), &mut table, &mut font)?;
		let compiled = table.compile(&mut font)?;
		let table = Svg::decompile(&compiled, &mut font)?;
		assert_eq!(get_xml(&table, &mut font)?, ttx());
		Ok(())
	}

	/// A document with `]]>` must not escape the CDATA section it is written into.
	#[test]
	fn round_trip_ttx_doc_containing_cdata_end() -> Result<()> {
		let doc = "]]></svgDoc></SVG><name><namerecord nameID=\"6\" platformID=\"3\" platEncID=\"1\" langID=\"0x409\">x</namerecord></name><SVG><svgDoc startGlyphID=\"1\" endGlyphID=\"1\"><![CDATA[y";
		let mut font = Font::new();
		let table = Svg {
			doc_list: Some(vec![SvgDocument {
				data: doc.to_owned(),
				start_glyph_id: 1,
				end_glyph_id: 1,
				compressed: false,
			}]),
			compressed: false,
		};
		let xml = get_xml(&table, &mut font)?;
		let mut table = Svg::default();
		parse_ttx(&xml, &mut table, &mut font)?;
		let docs = table.doc_list.ok_or_else(no_doc_list)?;
		assert_eq!(docs.len(), 1);
		assert_eq!(docs[0].data, doc);
		Ok(())
	}

	#[test]
	fn compressed_documents() -> Result<()> {
		let mut font = Font::new();
		let table = Svg::decompile(&data(), &mut font)?;
		let docs = table.doc_list.ok_or_else(no_doc_list)?;
		assert_eq!(
			docs.iter().map(|d| d.compressed).collect::<Vec<_>>(),
			[false, true, false]
		);
		assert_eq!(docs[0].data, DOC0);
		assert_eq!(docs[1].data, DOC1);
		assert_eq!((docs[1].start_glyph_id, docs[1].end_glyph_id), (3, 3));
		Ok(())
	}
}
