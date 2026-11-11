//! Port of `fontTools.diff.diff`: the unified diff of the TTX dumps of two
//! fonts, with the built-in diff ([`u_diff`]) or an external diff tool
//! ([`run_external_diff`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_ttlib::{Font, ReadOptions, TableKey, Tag, TtxOptions};

use crate::difflib::unified_diff;
use crate::utils::get_file_modtime;
use crate::{DiffError, Result};

/// The tables to compare, as the command line gives them: tags padded to four
/// characters, or `GlyphOrder`.
#[derive(Debug, Clone, Default)]
pub struct DiffOptions {
	/// The tables to dump (`include_tables`); all if `None` or empty.
	pub include_tables: Option<Vec<String>>,
	/// The tables not to dump when dumping all (`exclude_tables`).
	pub exclude_tables: Option<Vec<String>>,
	/// The font of a collection to compare in the first file
	/// (`font_number_a`); negative for none.
	pub font_number_a: i64,
	/// The font of a collection to compare in the second file
	/// (`font_number_b`); negative for none.
	pub font_number_b: i64,
	/// Dump the two fonts in parallel (`use_multiprocess`).
	pub use_multiprocess: bool,
}

impl DiffOptions {
	/// The defaults of `u_diff`: all tables, single fonts, in parallel.
	pub fn new() -> Self {
		DiffOptions {
			font_number_a: -1,
			font_number_b: -1,
			use_multiprocess: true,
			..Default::default()
		}
	}
}

/// `TTFont(path, fontNumber=font_number)`.
pub(crate) fn open_font(path: &str, font_number: i64) -> Result<Font> {
	let options = ReadOptions {
		font_number: usize::try_from(font_number).ok(),
		..Default::default()
	};
	Ok(Font::read(std::fs::read(path)?, options)?)
}

/// A table name of the command line as a [`TableKey`]: `None` if it cannot
/// name a table of a font (fontTools then finds no such table).
fn table_key(name: &str) -> Option<TableKey> {
	if name == "GlyphOrder" {
		return Some(TableKey::GlyphOrder);
	}
	let bytes = fontoxide_misc::py::latin1_encode(name)?;
	let bytes: [u8; 4] = bytes.try_into().ok()?;
	Some(TableKey::Table(Tag::new(&bytes)))
}

/// `ttf.saveXML(filepath, tables=include_tables, skipTables=exclude_tables)`,
/// as a string.
fn ttx_dump(font: &mut Font, options: &DiffOptions) -> Result<String> {
	let keys = |names: &Option<Vec<String>>| -> Vec<TableKey> {
		names
			.iter()
			.flatten()
			.filter_map(|n| table_key(n))
			.collect()
	};
	let mut tables = keys(&options.include_tables);
	if tables.is_empty()
		&& options
			.include_tables
			.as_ref()
			.is_some_and(|t| !t.is_empty())
	{
		// Only tables no font can have: fontTools dumps none of them, rather
		// than all tables.
		let tags = font.tags();
		let absent = (0..=u32::MAX)
			.map(|n| Tag::new(&n.to_be_bytes()))
			.find(|t| !tags.contains(t))
			.ok_or_else(|| DiffError::Message("no absent tag".into()))?;
		tables.push(TableKey::Table(absent));
	}
	Ok(font.to_ttx(&TtxOptions {
		tables,
		skip_tables: keys(&options.exclude_tables),
		..Default::default()
	})?)
}

/// Reads a font and dumps it.
fn load_and_dump(path: &str, font_number: i64, options: &DiffOptions) -> Result<String> {
	ttx_dump(&mut open_font(path, font_number)?, options)
}

/// `_get_fonts_and_save_xml` and `_mp_save_ttx_xml`: the TTX dumps of the two
/// fonts, made in parallel if `use_multiprocess` and there are several CPUs.
fn dump_fonts(
	filepath_a: &str,
	filepath_b: &str,
	options: &DiffOptions,
) -> Result<(String, String)> {
	// Both fonts are opened first, as fontTools does, so that the errors come
	// in the same order.
	drop(open_font(filepath_a, options.font_number_a)?);
	drop(open_font(filepath_b, options.font_number_b)?);
	let parallel =
		options.use_multiprocess && std::thread::available_parallelism().is_ok_and(|n| n.get() > 1);
	if parallel {
		let (left, right) = std::thread::scope(|scope| {
			let left = scope.spawn(|| load_and_dump(filepath_a, options.font_number_a, options));
			let right = load_and_dump(filepath_b, options.font_number_b, options);
			let left = left
				.join()
				.unwrap_or_else(|_| Err(DiffError::Message("the TTX dump panicked".into())));
			(left, right)
		});
		Ok((left?, right?))
	} else {
		let left = load_and_dump(filepath_a, options.font_number_a, options)?;
		let right = load_and_dump(filepath_b, options.font_number_b, options)?;
		Ok((left, right))
	}
}

/// `file.readlines()` of a file opened in text mode: the lines with their
/// ends, the universal newlines translated to `\n`.
///
/// LENIENT(L54): the dumps are UTF-8; fontTools reads them back with the locale's
/// encoding (on Windows, mangling or failing on non-ASCII names unless Python
/// runs in UTF-8 mode).
fn readlines(text: &str) -> Vec<String> {
	let mut lines = Vec::new();
	let mut current = String::new();
	let mut chars = text.chars().peekable();
	while let Some(c) = chars.next() {
		match c {
			'\r' => {
				if chars.peek() == Some(&'\n') {
					chars.next();
				}
				current.push('\n');
				lines.push(std::mem::take(&mut current));
			}
			'\n' => {
				current.push('\n');
				lines.push(std::mem::take(&mut current));
			}
			c => current.push(c),
		}
	}
	if !current.is_empty() {
		lines.push(current);
	}
	lines
}

/// `u_diff`: the lines (with their ends) of the unified diff, with
/// `context_lines` lines of context, of the TTX dumps of two fonts, with
/// Python's `difflib`. The headers name the files with their modification
/// times.
pub fn u_diff(
	filepath_a: &str,
	filepath_b: &str,
	context_lines: i128,
	options: &DiffOptions,
) -> Result<Vec<String>> {
	let (left, right) = dump_fonts(filepath_a, filepath_b, options)?;
	let fromlines = readlines(&left);
	let tolines = readlines(&right);
	let fromdate = get_file_modtime(filepath_a)?;
	let todate = get_file_modtime(filepath_b)?;
	let a: Vec<&str> = fromlines.iter().map(String::as_str).collect();
	let b: Vec<&str> = tolines.iter().map(String::as_str).collect();
	Ok(unified_diff(
		&a,
		&b,
		filepath_a,
		filepath_b,
		&fromdate,
		&todate,
		context_lines,
		"\n",
	))
}

/// A temporary directory, removed when dropped (`tempfile.TemporaryDirectory`).
struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
	fn new() -> Result<Self> {
		static COUNTER: AtomicUsize = AtomicUsize::new(0);
		let nanos = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map_or(0, |d| d.subsec_nanos());
		for _ in 0..100 {
			let name = format!(
				"tmp{:x}{:x}{:x}",
				std::process::id(),
				nanos,
				COUNTER.fetch_add(1, Ordering::Relaxed)
			);
			let dir = std::env::temp_dir().join(name);
			match std::fs::create_dir(&dir) {
				Ok(()) => return Ok(TemporaryDirectory(dir)),
				Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
				Err(e) => return Err(e.into()),
			}
		}
		Err(DiffError::Message(
			"No usable temporary directory name found".into(),
		))
	}

	fn path(&self) -> &Path {
		&self.0
	}
}

impl Drop for TemporaryDirectory {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

/// `run_external_diff`: the lines (with their ends) of the diff of the TTX
/// dumps of two fonts, saved as `left.ttx` and `right.ttx` in a temporary
/// directory, by the external tool `diff_tool` with the arguments `diff_args`.
/// Fails with what the tool writes to standard error, if anything.
pub fn run_external_diff(
	diff_tool: &Path,
	diff_args: &[String],
	filepath_a: &str,
	filepath_b: &str,
	options: &DiffOptions,
) -> Result<Vec<String>> {
	let tmp = TemporaryDirectory::new()?;
	let (left, right) = dump_fonts(filepath_a, filepath_b, options)?;
	let left_ttxpath = tmp.path().join("left.ttx");
	let right_ttxpath = tmp.path().join("right.ttx");
	std::fs::write(&left_ttxpath, left)?;
	std::fs::write(&right_ttxpath, right)?;
	let output = std::process::Command::new(diff_tool)
		.args(diff_args)
		.arg(&left_ttxpath)
		.arg(&right_ttxpath)
		.stdin(std::process::Stdio::null())
		.output()?;
	// Python decodes the output as UTF-8 (strictly), with universal newlines.
	let decode = |data: Vec<u8>| {
		String::from_utf8(data).map_err(|e| {
			DiffError::Message(format!(
				"'utf-8' codec can't decode byte 0x{:02x} in position {}: invalid start byte",
				e.as_bytes()[e.utf8_error().valid_up_to()],
				e.utf8_error().valid_up_to()
			))
		})
	};
	let lines = readlines(&decode(output.stdout)?);
	let err = readlines(&decode(output.stderr)?).concat();
	if !err.is_empty() {
		return Err(DiffError::External(err));
	}
	Ok(lines)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn universal_newlines() {
		assert_eq!(readlines("a\r\nb\rc\nd"), ["a\n", "b\n", "c\n", "d"]);
		assert!(readlines("").is_empty());
	}

	#[test]
	fn table_keys() {
		assert_eq!(table_key("GlyphOrder"), Some(TableKey::GlyphOrder));
		assert_eq!(table_key("cvt "), Some(TableKey::Table(Tag::new(b"cvt "))));
		assert_eq!(table_key("toolong"), None);
		assert_eq!(table_key("中文中文"), None);
	}
}
