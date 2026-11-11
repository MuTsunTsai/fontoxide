//! Reading TTX documents (`fontTools.misc.xmlReader.XMLReader`, the
//! TTX-specific part), and the file names of TTX dumps split into several files
//! (`saveXML(splitTables=True, splitGlyphs=True)`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

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

/// One file of a TTX dump: its path and its contents.
///
/// A dump split into several files ([`crate::TtxOptions::split_tables`],
/// [`crate::TtxOptions::split_glyphs`]) refers to the table and glyph files by
/// their file names (`src` attributes), relative to the file that refers to
/// them; the files go in the same directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtxFile {
	/// The path, derived from the path of the main file.
	pub path: String,
	/// The TTX text.
	pub contents: String,
}

/// Loads a file that a TTX file refers to (with a `src` attribute) as text.
pub type TtxLoader<'a> = dyn FnMut(&Path) -> Result<String> + 'a;

/// Reads a TTX file from the file system as UTF-8 text, without a byte order mark.
pub fn read_ttx_file(path: &Path) -> Result<String> {
	let text = std::fs::read_to_string(path)?;
	Ok(match text.strip_prefix('\u{feff}') {
		Some(rest) => rest.to_owned(),
		None => text,
	})
}

/// Where the `src` attributes of the file being read are resolved.
pub(crate) struct SubFiles<'a, 'b> {
	/// The directory of the file being read (`os.path.dirname(file.name)`), or
	/// the current directory (empty) if it has no name.
	pub dir: PathBuf,
	/// Loads a file.
	pub load: &'a mut TtxLoader<'b>,
}

impl SubFiles<'_, '_> {
	/// Parses the file of a `src` attribute (`os.path.join(dirname, subFile)`),
	/// whose root must be `ttFont`. Returns the file's root and its directory.
	fn read(&mut self, src: &str) -> Result<(Element, PathBuf)> {
		let path = self.dir.join(src);
		let text = (self.load)(&path)?;
		let root =
			fontoxide_misc::xml_reader::parse(&text).map_err(|e| Error::ttx(e.to_string()))?;
		if root.name != "ttFont" {
			return Err(Error::ttx(format!("illegal root tag: {}", root.name)));
		}
		let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
		Ok((root, dir))
	}
}

/// Imports a parsed TTX document into `font`.
pub(crate) fn import(
	font: &mut Font,
	root: &Element,
	options: &TtxImportOptions,
	files: &mut SubFiles<'_, '_>,
) -> Result<()> {
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
		if let Some(src) = element.attr("src") {
			// A table in a file of its own, read by a new `XMLReader`.
			let (sub_root, dir) = files.read(src)?;
			let mut sub_files = SubFiles {
				dir,
				load: &mut *files.load,
			};
			import(font, &sub_root, options, &mut sub_files)?;
			continue;
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
				import_table(font, tag, element, files)?
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

fn import_table(
	font: &mut Font,
	tag: Tag,
	element: &Element,
	files: &mut SubFiles<'_, '_>,
) -> Result<()> {
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
		if let Some(src) = child.attr("src") {
			// An element in a file of its own (a glyph of a split `glyf` table),
			// read by an `XMLReader` with `contentOnly=True`: the elements below
			// the second level of that file each replace this one, so that the
			// last one is read.
			let (sub_root, _) = files.read(src)?;
			let last = sub_root
				.elements()
				.flat_map(Element::elements)
				.last()
				.ok_or_else(|| {
					Error::ttx(format!(
						"TypeError: cannot unpack non-iterable NoneType object (no content in {src})"
					))
				})?;
			font.with_table(tag, |table, font| table.from_xml(last, font))?;
			continue;
		}
		font.with_table(tag, |table, font| table.from_xml(child, font))?;
	}
	Ok(())
}

// --- File names of split dumps

/// The path separators of `os.path`.
const SEPARATORS: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };

/// `os.path.splitext`: the extension starts at the last period of the file name,
/// unless only periods precede it.
pub(crate) fn splitext(path: &str) -> (&str, &str) {
	let name_start = path.rfind(SEPARATORS).map_or(0, |i| i + 1);
	let name = &path[name_start..];
	let leading = name.len() - name.trim_start_matches('.').len();
	match name[leading..].rfind('.') {
		Some(at) => path.split_at(name_start + leading + at),
		None => (path, ""),
	}
}

/// `os.path.basename`.
pub(crate) fn basename(path: &str) -> &str {
	path.rfind(SEPARATORS).map_or(path, |i| &path[i + 1..])
}

/// `fontTools.misc.filenames.userNameToFileName`, which names the glyph files.
pub(crate) fn user_name_to_file_name(
	user_name: &str,
	existing: &HashSet<String>,
	prefix: &str,
	suffix: &str,
) -> Result<String> {
	fontoxide_misc::filenames::user_name_to_file_name(user_name, existing, prefix, suffix)
		.map_err(|e| Error::value(e.to_string()))
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn path_helpers() {
		assert_eq!(splitext("a/b.c/font.ttx"), ("a/b.c/font", ".ttx"));
		assert_eq!(splitext("a.b/font"), ("a.b/font", ""));
		assert_eq!(splitext("dir/.hidden"), ("dir/.hidden", ""));
		assert_eq!(splitext("font._g_l_y_f.ttx"), ("font._g_l_y_f", ".ttx"));
		assert_eq!(basename("a/b/font.ttx"), "font.ttx");
		assert_eq!(basename("font.ttx"), "font.ttx");
	}
}
