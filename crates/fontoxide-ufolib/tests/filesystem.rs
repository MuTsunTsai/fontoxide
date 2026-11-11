//! `Tests/misc/filesystem_test.py`.
//!
//! fontoxide's port of `fontTools.misc.filesystem` is internal to ufoLib, so
//! the tests go through [`UfoReader`]: `readBytesFromPath` reads from the
//! `OSFS` of the UFO, glyph sets read from a `SubFS` of it, and zipped UFOs
//! from a `ReadZipFS`.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_ufolib::{UfoLibError, UfoReader};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

fn tmp_path(name: &str) -> PathBuf {
	static COUNTER: AtomicUsize = AtomicUsize::new(0);
	let dir = std::env::temp_dir().join(format!(
		"fontoxide-ufolib-fs-{}-{name}-{}",
		std::process::id(),
		COUNTER.fetch_add(1, Ordering::Relaxed)
	));
	std::fs::create_dir_all(&dir).expect("create temp dir");
	dir
}

const METAINFO: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>creator</key>
	<string>org.robofab.ufoLib</string>
	<key>formatVersion</key>
	<integer>3</integer>
</dict>
</plist>
"#;

/// A UFO at `path` with only `metainfo.plist`, so that it can be opened.
fn make_ufo(path: &Path) {
	std::fs::create_dir_all(path).expect("create ufo");
	std::fs::write(path.join("metainfo.plist"), METAINFO).expect("write");
}

fn is_illegal_back_reference(e: &UfoLibError) -> bool {
	matches!(e, UfoLibError::Filesystem(_))
}

#[test]
fn test_osfs_reads_within_root() {
	let tmp = tmp_path("within");
	make_ufo(&tmp);
	std::fs::create_dir(tmp.join("sub")).expect("mkdir");
	std::fs::write(tmp.join("sub").join("a.txt"), b"hello").expect("write");
	let reader = UfoReader::new(&tmp, false).expect("reader");
	assert_eq!(
		reader.read_bytes_from_path("sub/a.txt").expect("read"),
		Some(b"hello".to_vec())
	);
	// a back-reference that stays within the root is still resolved
	assert_eq!(
		reader
			.read_bytes_from_path("sub/../sub/a.txt")
			.expect("read"),
		Some(b"hello".to_vec())
	);
	let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn test_osfs_rejects_traversal() {
	for path in [
		"../outside.txt",
		"../../etc/hosts",
		"sub/../../outside.txt",
		"nonexistent/../../outside.txt",
	] {
		let tmp = tmp_path("traversal");
		let root = tmp.join("root");
		make_ufo(&root);
		std::fs::write(tmp.join("outside.txt"), b"secret").expect("write");
		let reader = UfoReader::new(&root, false).expect("reader");
		let err = reader.read_bytes_from_path(path).expect_err(path);
		assert!(is_illegal_back_reference(&err), "{path}: {err}");
		let _ = std::fs::remove_dir_all(tmp);
	}
}

#[test]
fn test_subfs_rejects_traversal_out_of_root() {
	// mirrors how ufoLib opens a glyph set: an OSFS rooted at the UFO, a SubFS
	// into "glyphs", and a file name coming from the untrusted contents.plist
	let tmp = tmp_path("subfs");
	let ufo = tmp.join("font.ufo");
	make_ufo(&ufo);
	std::fs::create_dir(ufo.join("glyphs")).expect("mkdir");
	std::fs::write(
		ufo.join("layercontents.plist"),
		br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<array><array><string>public.default</string><string>glyphs</string></array></array>
</plist>
"#,
	)
	.expect("write");
	std::fs::write(
		ufo.join("glyphs").join("contents.plist"),
		br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict><key>a</key><string>../../outside.txt</string></dict>
</plist>
"#,
	)
	.expect("write");
	std::fs::write(tmp.join("outside.txt"), b"secret").expect("write");
	let reader = UfoReader::new(&ufo, false).expect("reader");
	let glyph_set = reader
		.get_glyph_set(None, Some(false), Some(false))
		.expect("glyph set");
	let err = glyph_set.get_glif("a").expect_err("escaping file name");
	assert!(is_illegal_back_reference(&err), "{err}");
	let _ = std::fs::remove_dir_all(tmp);
}

fn evil_zip(path: &Path) {
	let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
	for (name, data) in [
		("font.ufo/metainfo.plist", METAINFO),
		("../escaped.txt", b"pwned".as_slice()),
		("font.ufo/lib.plist", b"y".as_slice()),
	] {
		zip.start_file(name, SimpleFileOptions::default())
			.expect("zip entry");
		zip.write_all(data).expect("zip data");
	}
	let data = zip.finish().expect("zip").into_inner();
	std::fs::write(path, data).expect("write");
}

#[test]
fn test_readzipfs_member_cannot_escape_root() {
	let tmp = tmp_path("evil");
	let path = tmp.join("evil.ufoz");
	evil_zip(&path);
	// building the directory mirror must not create files outside its root
	let err = UfoReader::new(&path, false).expect_err("escaping member");
	// the offending zip entry is named
	assert!(err.to_string().contains("../escaped.txt"), "{err}");
	assert!(!tmp.join("escaped.txt").exists());
	let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn test_readzipfs_rejection_is_repeatable() {
	let tmp = tmp_path("repeat");
	let path = tmp.join("evil.ufoz");
	evil_zip(&path);
	for _ in 0..2 {
		let err = UfoReader::new(&path, false).expect_err("escaping member");
		assert!(err.to_string().contains("../escaped.txt"), "{err}");
	}
	let _ = std::fs::remove_dir_all(tmp);
}
