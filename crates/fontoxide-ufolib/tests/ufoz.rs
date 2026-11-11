//! Port of fontTools' `Tests/ufoLib/UFOZ_test.py`.
//!
//! Not ported: `TestMemoryFS` (skipped by fontTools without the `fs`
//! package; there are no user-supplied file system objects here), and
//! `test_path_attribute_deprecated` (a deprecation warning).

mod common;

use std::path::{Path, PathBuf};

use common::{TempDir, py_dict, tempdir, testdata};
use fontoxide_ufolib::{Result, UfoFileStructure, UfoReader, UfoWriter, UfoWriterOptions};

const TEST_UFO3: &str = "TestFont1 (UFO3).ufo";
const TEST_UFOZ: &str = "TestFont1 (UFO3).ufoz";

fn copy_dir(src: &Path, dst: &Path) {
	std::fs::create_dir_all(dst).expect("mkdir");
	for entry in std::fs::read_dir(src).expect("read_dir") {
		let entry = entry.expect("entry");
		let target = dst.join(entry.file_name());
		if entry.path().is_dir() {
			copy_dir(&entry.path(), &target);
		} else {
			std::fs::copy(entry.path(), target).expect("copy");
		}
	}
}

/// The `testufo` fixture: a copy of the test font, a directory or a zip file.
fn testufo(name: &str) -> (TempDir, PathBuf) {
	let tmp = tempdir("ufoz");
	let src = testdata().join(name);
	let dst = tmp.join(name);
	if src.is_dir() {
		copy_dir(&src, &dst);
	} else {
		std::fs::copy(&src, &dst).expect("copy");
	}
	(tmp, dst)
}

// TestUFOZ

#[test]
fn test_read() -> Result<()> {
	let (_tmp, testufoz) = testufo(TEST_UFOZ);
	let reader = UfoReader::new(&testufoz, true)?;
	assert_eq!(reader.file_structure(), UfoFileStructure::Zip);
	assert_eq!(reader.format_version_tuple().major(), 3);
	let gs = reader.get_glyph_set(None, None, None)?;
	let a = String::from_utf8(gs.get_glif("a")?).expect("utf-8");
	assert_eq!(
		a.lines().collect::<Vec<_>>(),
		[
			"<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
			"<glyph name=\"a\" format=\"2\">",
			"  <advance height=\"750\" width=\"388\"/>",
			"  <unicode hex=\"0061\"/>",
			"  <outline>",
			"    <contour>",
			"      <point x=\"66\" y=\"0\" type=\"line\"/>",
			"      <point x=\"322\" y=\"0\" type=\"line\"/>",
			"      <point x=\"194\" y=\"510\" type=\"line\"/>",
			"    </contour>",
			"  </outline>",
			"</glyph>",
		]
	);
	Ok(())
}

#[test]
fn test_get_file_modification_time() -> Result<()> {
	let (_tmp, testufoz) = testufo(TEST_UFOZ);
	let reader = UfoReader::new(&testufoz, true)?;
	assert!(
		reader
			.get_file_modification_time("metainfo.plist")?
			.is_some()
	);
	Ok(())
}

#[test]
fn test_write() -> Result<()> {
	let (_tmp, testufoz) = testufo(TEST_UFOZ);
	let mut writer = UfoWriter::new(
		&testufoz,
		UfoWriterOptions {
			structure: Some(UfoFileStructure::Zip),
			..Default::default()
		},
	)?;
	writer.write_lib(&py_dict("{'hello world': 123}"), None)?;
	writer.close()?;
	let reader = UfoReader::new(&testufoz, true)?;
	assert_eq!(reader.read_lib(None)?, py_dict("{'hello world': 123}"));
	// the rest of the font is still there
	assert!(reader.get_glyph_set(None, None, None)?.contains("a"));
	Ok(())
}

#[test]
fn test_pathlike() -> Result<()> {
	for name in [TEST_UFO3, TEST_UFOZ] {
		let (_tmp, testufo) = testufo(name);
		let path = testufo.to_string_lossy().into_owned();
		let reader = UfoReader::new(&testufo, true)?;
		assert_eq!(reader.path(), path);
		drop(reader);
		let writer = UfoWriter::new(&testufo, UfoWriterOptions::default())?;
		assert_eq!(writer.path(), path);
	}
	Ok(())
}

#[test]
fn test_write_new_zip() -> Result<()> {
	// Not in fontTools: a new zipped UFO is written when the writer closes.
	let tmp = tempdir("newzip");
	let path = tmp.join("New.ufoz");
	let mut writer = UfoWriter::new(
		&path,
		UfoWriterOptions {
			structure: Some(UfoFileStructure::Zip),
			..Default::default()
		},
	)?;
	writer.write_lib(&py_dict("{'a': 1}"), None)?;
	writer.close()?;
	let reader = UfoReader::new(&path, true)?;
	assert_eq!(reader.file_structure(), UfoFileStructure::Zip);
	assert_eq!(reader.read_lib(None)?, py_dict("{'a': 1}"));
	Ok(())
}
