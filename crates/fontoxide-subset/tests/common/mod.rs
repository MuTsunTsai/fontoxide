//! Helpers of the tests ported from fontTools' `Tests/subset`.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use fontoxide_ttlib::{Font, ReadOptions, TableKey, Tag, TtxOptions, WriteOptions};

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// A file of fontTools' `Tests` directory.
pub fn tests_path(relative: &str) -> String {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests")
		.join(relative)
		.to_string_lossy()
		.into_owned()
}

/// `getpath`: a file of `Tests/subset/data`.
pub fn getpath(name: &str) -> String {
	tests_path(&format!("subset/data/{name}"))
}

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

/// A temporary directory, removed at the end of the test.
pub struct TempDir {
	dir: PathBuf,
	files: u32,
}

impl TempDir {
	pub fn new() -> TestResult<Self> {
		let n = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
		let dir = std::env::temp_dir().join(format!("fontoxide-subset-{}-{n}", std::process::id()));
		std::fs::create_dir_all(&dir)?;
		Ok(TempDir { dir, files: 0 })
	}

	/// `temp_path`.
	pub fn path(&mut self, suffix: &str) -> String {
		self.files += 1;
		self.dir
			.join(format!("tmp{}{suffix}", self.files))
			.to_string_lossy()
			.into_owned()
	}

	/// `compile_font`: compiles a TTX file.
	pub fn compile_font(&mut self, ttx: &str, suffix: &str) -> TestResult<String> {
		let mut font = import(ttx)?;
		let path = self.path(suffix);
		std::fs::write(
			&path,
			font.write(&WriteOptions {
				reorder_tables: None,
				..WriteOptions::default()
			})?,
		)?;
		Ok(path)
	}
}

impl Drop for TempDir {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.dir);
	}
}

/// `TTFont()` with `importXML`, not recalculating anything.
pub fn import(ttx: &str) -> TestResult<Font> {
	let mut font = Font::new();
	font.recalc_bboxes = false;
	font.recalc_timestamp = false;
	font.import_ttx(&std::fs::read_to_string(ttx)?)?;
	Ok(font)
}

/// `TTFont(path)`.
pub fn open(path: impl AsRef<Path>) -> TestResult<Font> {
	read(std::fs::read(path)?)
}

/// `TTFont(data)`.
pub fn read(data: Vec<u8>) -> TestResult<Font> {
	Ok(Font::read(data, ReadOptions::default())?)
}

/// A font of `tests/data` (see `tools/reference/subset_fixtures.py`), compiled and
/// read back.
pub fn fixture(name: &str) -> TestResult<Font> {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data")
		.join(format!("{name}.ttx"));
	let mut font = Font::new();
	font.import_ttx(&std::fs::read_to_string(path)?)?;
	save_and_read(&mut font)
}

/// A font saved and read back.
pub fn save_and_read(font: &mut Font) -> TestResult<Font> {
	read(font.write(&WriteOptions::default())?)
}

/// `subset.main`, which must succeed.
pub fn subset_main(args: &[&str]) -> TestResult {
	assert_eq!(fontoxide_subset::main(args)?, 0, "{args:?}");
	Ok(())
}

fn remove_between(s: &str, start: &str, end: &str) -> String {
	let mut out = String::with_capacity(s.len());
	let mut rest = s;
	while let Some(i) = rest.find(start) {
		let after = &rest[i + start.len()..];
		let Some(j) = after.find(end) else {
			break;
		};
		out.push_str(&rest[..i]);
		rest = &after[j + end.len()..];
	}
	out.push_str(rest);
	out
}

/// `read_ttx`: `stripVariableItemsFromTTX` with `sfntVersion=True`.
pub fn strip_ttx(ttx: &str) -> Vec<String> {
	let s = ttx.replace("\r\n", "\n");
	let s = remove_between(&s, " ttLibVersion=\"", "\"");
	let s = remove_between(&s, " sfntVersion=\"", "\"");
	let s = remove_between(&s, "<checkSumAdjustment value=\"", "\"/>");
	let s = remove_between(&s, "<modified value=\"", "\"/>");
	let s = remove_between(&s, "<created value=\"", "\"/>");
	s.split_inclusive('\n').map(str::to_owned).collect()
}

fn key(name: &str) -> TableKey {
	if name == "GlyphOrder" {
		TableKey::GlyphOrder
	} else {
		TableKey::Table(Tag::from_str_padded(name).expect("a table tag"))
	}
}

/// The TTX of some tables, or all if there are none.
pub fn to_ttx(font: &mut Font, tables: &[&str]) -> TestResult<String> {
	Ok(font.to_ttx(&TtxOptions {
		tables: tables.iter().map(|t| key(t)).collect(),
		newline: "\n".into(),
		..Default::default()
	})?)
}

/// `expect_ttx`.
pub fn expect_ttx(font: &mut Font, expected: &str, tables: &[&str]) -> TestResult {
	let actual = strip_ttx(&to_ttx(font, tables)?);
	let expected_lines = strip_ttx(&std::fs::read_to_string(expected)?);
	if actual != expected_lines {
		for (i, (a, e)) in actual.iter().zip(&expected_lines).enumerate() {
			if a != e {
				panic!("{expected}, line {}:\n  actual   {a}  expected {e}", i + 1);
			}
		}
		panic!(
			"{expected}: {} lines, expected {}",
			actual.len(),
			expected_lines.len()
		);
	}
	Ok(())
}

/// The names of the glyph order (`getGlyphOrder`).
pub fn glyph_names(font: &mut Font) -> TestResult<Vec<String>> {
	let order = font.glyph_order()?;
	Ok((0..order.len() as u32)
		.map(|g| order.name(g).into_owned())
		.collect())
}

/// The program of a charstring as dumped to TTX, as tokens.
pub fn charstring_program(font: &mut Font, table: &str, glyph: &str) -> TestResult<Vec<String>> {
	let ttx = to_ttx(font, &[table])?;
	let root = fontoxide_misc::xml_reader::parse(&ttx)?;
	fn find<'a>(
		e: &'a fontoxide_misc::xml_reader::Element,
		glyph: &str,
	) -> Option<&'a fontoxide_misc::xml_reader::Element> {
		if e.name == "CharString" && e.attr("name") == Some(glyph) {
			return Some(e);
		}
		e.elements().find_map(|c| find(c, glyph))
	}
	let element = find(&root, glyph).ok_or_else(|| format!("no charstring of {glyph}"))?;
	Ok(element
		.text()
		.split_whitespace()
		.map(str::to_owned)
		.collect())
}
