//! Reading TTX documents (`fontTools.misc.xmlReader.XMLReader`, the
//! TTX-specific part).

use fontoxide_misc::Tag;
use fontoxide_misc::text_tools::{Literal, safe_eval};
use fontoxide_misc::xml_reader::Element;

use crate::font::{TableKey, xml_to_tag};
use crate::table::{DefaultTable, new_table};
use crate::{Error, Font, Result};

/// Options for importing TTX.
#[derive(Debug, Clone, Copy, Default)]
pub struct TtxImportOptions {
	/// Skip tables that fontoxide does not implement yet, instead of failing on
	/// them (unless they are dumped as raw data). For testing only: the skipped
	/// tables are missing from the font.
	pub skip_unimplemented: bool,
}

/// Imports a parsed TTX document into `font`.
pub(crate) fn import(font: &mut Font, root: &Element, options: &TtxImportOptions) -> Result<()> {
	if root.name != "ttFont" {
		return Err(Error::ttx(format!("illegal root tag: {}", root.name)));
	}
	if font.reader().is_none()
		&& font.tags().is_empty()
		&& let Some(v) = root.attr("sfntVersion")
	{
		font.sfnt_version = parse_sfnt_version(v)?;
	}
	for element in root.elements() {
		if element.attr("src").is_some() {
			return Err(Error::ttx(
				"TTX files split into several files (the 'src' attribute) are not supported yet",
			));
		}
		let key = xml_to_tag(&element.name)
			.ok_or_else(|| Error::ttx(format!("invalid table element: {}", element.name)))?;
		log::info!("Parsing '{}' table...", element.name);
		match key {
			TableKey::GlyphOrder => import_glyph_order(font, element)?,
			TableKey::Table(tag) => {
				let raw = element.attr("ERROR").is_some()
					|| element
						.attr("raw")
						.and_then(safe_eval)
						.is_some_and(|v| v.is_truthy());
				if options.skip_unimplemented && !raw && !crate::table::is_implemented(tag) {
					log::warn!("skipping unimplemented '{tag}' table");
					continue;
				}
				import_table(font, tag, element)?
			}
		}
	}
	Ok(())
}

/// The `sfntVersion` attribute: four characters, or a Python string literal body
/// with escapes such as `\x00\x01\x00\x00`.
fn parse_sfnt_version(v: &str) -> Result<Tag> {
	let text = if v.chars().count() == 4 {
		v.to_owned()
	} else {
		match safe_eval(&format!("\"{v}\"")) {
			Some(Literal::Str(s)) => s,
			_ => return Err(Error::ttx(format!("invalid sfntVersion: {v:?}"))),
		}
	};
	let bytes = fontoxide_misc::py::latin1_encode(&text)
		.ok_or_else(|| Error::ttx(format!("invalid sfntVersion: {v:?}")))?;
	// Packed as "4s": truncated or padded with NUL bytes.
	let mut tag = [0u8; 4];
	let n = bytes.len().min(4);
	tag[..n].copy_from_slice(&bytes[..n]);
	Ok(Tag(tag))
}

fn import_glyph_order(font: &mut Font, element: &Element) -> Result<()> {
	let mut names = Vec::new();
	let mut any = false;
	for e in element.elements() {
		any = true;
		if e.name == "GlyphID" {
			let name = e
				.attr("name")
				.ok_or_else(|| Error::ttx("missing 'name' on <GlyphID>"))?;
			names.push(name.to_owned());
		}
	}
	// fontTools calls setGlyphOrder for every child element, so an empty element
	// leaves the glyph order alone.
	if any {
		font.set_glyph_order(names);
	}
	Ok(())
}

fn import_table(font: &mut Font, tag: Tag, element: &Element) -> Result<()> {
	let raw = element.attr("ERROR").is_some()
		|| element
			.attr("raw")
			.and_then(safe_eval)
			.is_some_and(|v| v.is_truthy());
	let is_loca = tag == Tag::new(b"loca") || tag == Tag::new(b"LOCA");
	if is_loca && font.contains(tag) {
		// The original 'loca' is needed if 'glyf' isn't recompiled.
		font.load(tag)?;
	} else if raw {
		font.insert(Box::new(DefaultTable::new(tag, Vec::new())));
	} else {
		font.insert(new_table(tag));
	}
	for child in element.elements() {
		font.with_table(tag, |table, font| table.from_xml(child, font))?;
	}
	Ok(())
}
