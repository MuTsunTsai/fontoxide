//! Port of fontTools' `Tests/ufoLib/ufoLib_test.py`.
//!
//! `test_formatVersion_deprecated` is not ported: the deprecated
//! `formatVersion` attribute and its warning have no Rust counterpart.

mod common;

use std::path::PathBuf;

use common::{TempDir, py_value, take_logs, tempdir, write_plist};
use fontoxide_ufolib::{
	Result, UfoFormatVersion, UfoLibError, UfoReader, UfoWriter, UfoWriterOptions,
};

/// The `ufo_path` fixture.
fn ufo_path() -> (TempDir, PathBuf) {
	let dir = tempdir("ufolib");
	let ufo = dir.join("TestFont.ufo");
	std::fs::create_dir(&ufo).expect("mkdir");
	write_plist(
		&ufo.join("metainfo.plist"),
		&py_value("{'creator': 'pytest', 'formatVersion': 3}"),
	);
	std::fs::create_dir(ufo.join("glyphs")).expect("mkdir");
	write_plist(
		&ufo.join("layercontents.plist"),
		&py_value("[('public.default', 'glyphs')]"),
	);
	(dir, ufo)
}

fn writer(path: &std::path::Path, format_version: Option<(i64, i64)>) -> Result<UfoWriter> {
	UfoWriter::new(
		path,
		UfoWriterOptions {
			format_version: format_version.map(Into::into),
			..Default::default()
		},
	)
}

#[test]
fn test_format_version_tuple() -> Result<()> {
	let (_dir, ufo) = ufo_path();
	let reader = UfoReader::new(&ufo, true)?;
	assert_eq!(reader.format_version_tuple().as_tuple(), (3, 0));
	assert_eq!(reader.format_version_tuple().major(), 3);
	assert_eq!(reader.format_version_tuple().minor(), 0);
	assert_eq!(reader.format_version_tuple().to_string(), "3.0");
	Ok(())
}

#[test]
fn test_read_meta_info_errors() {
	let (_dir, ufo) = ufo_path();
	let metainfo = ufo.join("metainfo.plist");
	std::fs::remove_file(&metainfo).expect("unlink");
	let e = UfoReader::new(&ufo, true).expect_err("missing");
	assert!(
		e.is_ufo_lib_error() && e.to_string().contains("'metainfo.plist' is missing"),
		"{e}"
	);

	write_plist(&metainfo, &py_value("{}"));
	let e = UfoReader::new(&ufo, true).expect_err("no version");
	assert!(
		e.is_ufo_lib_error() && e.to_string().contains("Missing required formatVersion"),
		"{e}"
	);

	write_plist(&metainfo, &py_value("[]"));
	let e = UfoReader::new(&ufo, true).expect_err("not a dict");
	assert!(
		e.is_ufo_lib_error()
			&& e.to_string()
				.contains("metainfo.plist is not properly formatted"),
		"{e}"
	);
}

#[test]
fn test_read_meta_info_unsupported_format_version() -> Result<()> {
	let (_dir, ufo) = ufo_path();
	write_plist(
		&ufo.join("metainfo.plist"),
		&py_value("{'formatVersion': 10, 'formatVersionMinor': 15}"),
	);
	// validate=True by default
	assert!(matches!(
		UfoReader::new(&ufo, true),
		Err(UfoLibError::UnsupportedUfoFormat(_))
	));

	take_logs();
	UfoReader::new(&ufo, false)?;
	let logs = take_logs();
	assert_eq!(logs.len(), 1, "{logs:?}");
	assert!(logs[0].contains("Unsupported UFO format"));
	assert!(logs[0].contains("Assuming the latest supported version"));
	Ok(())
}

#[test]
fn test_ufo_writer_format_version() -> Result<()> {
	let dir = tempdir("writerversion");
	let ufo = dir.join("TestFont.ufo");
	let w = writer(&ufo, Some((3, 0)))?;
	assert_eq!(w.format_version_tuple().as_tuple(), (3, 0));
	drop(w);

	std::fs::remove_dir_all(&ufo)?;
	let w = writer(&ufo, Some((2, 0)))?;
	assert_eq!(w.format_version_tuple().as_tuple(), (2, 0));
	Ok(())
}

#[test]
fn test_ufo_writer_format_version_default_latest() -> Result<()> {
	let dir = tempdir("writerlatest");
	let w = writer(&dir.join("TestFont.ufo"), None)?;
	assert_eq!(
		w.format_version_tuple(),
		UfoFormatVersion::default_version()
	);
	Ok(())
}

#[test]
fn test_ufo_writer_unsupported_format_version() {
	let dir = tempdir("writerunsupported");
	assert!(matches!(
		writer(dir.path(), Some((123, 456))),
		Err(UfoLibError::UnsupportedUfoFormat(_))
	));
}

#[test]
fn test_ufo_writer_previous_higher_format_version() {
	let (_dir, ufo) = ufo_path();
	match writer(&ufo, Some((2, 0))) {
		Err(UfoLibError::UnsupportedUfoFormat(m)) => {
			assert!(
				m.contains("UFO located at this path is a higher version"),
				"{m}"
			);
		}
		other => panic!("{other:?}"),
	}
}

#[test]
fn test_get_file_modification_time() -> Result<()> {
	let (_dir, ufo) = ufo_path();
	let writer = writer(&ufo, None)?;
	let raw_metainfo = writer
		.read_bytes_from_path("metainfo.plist")?
		.expect("metainfo");
	let modified = writer.get_file_modification_time("metainfo.plist")?;
	assert!(modified.is_some());
	// writeBytesToPath only modifies a file if the content to be written is not the same
	writer.write_bytes_to_path("metainfo.plist", &raw_metainfo)?;
	assert_eq!(
		writer.get_file_modification_time("metainfo.plist")?,
		modified
	);
	std::thread::sleep(std::time::Duration::from_millis(100));
	let mut changed = raw_metainfo.clone();
	changed.push(b'\n');
	writer.write_bytes_to_path("metainfo.plist", &changed)?;
	assert_ne!(
		writer.get_file_modification_time("metainfo.plist")?,
		modified
	);
	Ok(())
}
