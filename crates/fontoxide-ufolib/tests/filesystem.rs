//! `Tests/misc/filesystem_test.py`.
//!
//! fontoxide's port of `fontTools.misc.filesystem` is internal to ufoLib, so
//! the tests go through [`UfoReader`]: `readBytesFromPath` reads from the
//! `OSFS` of the UFO, glyph sets read from a `SubFS` of it, and zipped UFOs
//! from a `ReadZipFS`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_ufolib::{UfoLibError, UfoReader};
use rawzip::ZipArchiveWriter;
use rawzip::path::EntryPath;

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

/// Writes an archive of stored `(name, data)` members, with the names as they
/// are (`zipfile.ZipFile.writestr`).
fn write_zip(path: &Path, members: &[(&str, &[u8])]) {
	let mut zip = ZipArchiveWriter::new(Vec::new());
	for &(name, data) in members {
		let (mut entry, config) = zip
			.new_file(EntryPath::verbatim(name.as_bytes()))
			.start()
			.expect("zip entry");
		let mut writer = config.wrap(&mut entry);
		writer.write_all(data).expect("zip data");
		let (_, descriptor) = writer.finish().expect("zip data");
		entry.finish(descriptor).expect("zip entry");
	}
	std::fs::write(path, zip.finish().expect("zip")).expect("write");
}

fn evil_zip(path: &Path) {
	write_zip(
		path,
		&[
			("font.ufo/metainfo.plist", METAINFO),
			("../escaped.txt", b"pwned"),
			("font.ufo/lib.plist", b"y"),
		],
	);
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

#[test]
fn test_readzipfs_failed_mirror_is_not_cached() {
	// the mirror can also fail to build for reasons the member-name pre-scan
	// cannot see -- here a member named both as a file and as a directory, which
	// makes makedirs raise -- and the half-built mirror must not be published
	let tmp = tmp_path("conflict");
	let path = tmp.join("conflict.zip");
	write_zip(
		&path,
		&[
			("node", b"file"),
			("node/child.txt", b"x"),
			("after.txt", b"y"),
		],
	);
	// fontoxide reads the archive when the reader is created, so the reader
	// cannot be created, with the OSError that makedirs raises
	for _ in 0..2 {
		match UfoReader::new(&path, false) {
			Err(UfoLibError::UfoLib(e)) => assert!(e.contains("File exists: 'node'"), "{e}"),
			Err(e) => panic!("unexpected error: {e}"),
			Ok(_) => panic!("a member is both a file and a directory"),
		}
	}
	let _ = std::fs::remove_dir_all(tmp);
}
