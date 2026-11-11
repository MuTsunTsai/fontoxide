//! `Tests/misc/filesystem_zipfs_test.py`.
//!
//! `WriteZipFS` is internal to ufoLib in fontoxide, so the tests write zipped
//! UFOs with [`UfoWriter`], which writes the archive when it is closed.

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_ufolib::{UfoFileStructure, UfoWriter, UfoWriterOptions};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

fn tmp_path(name: &str) -> PathBuf {
	static COUNTER: AtomicUsize = AtomicUsize::new(0);
	let dir = std::env::temp_dir().join(format!(
		"fontoxide-ufolib-zipfs-{}-{name}-{}",
		std::process::id(),
		COUNTER.fetch_add(1, Ordering::Relaxed)
	));
	std::fs::create_dir_all(&dir).expect("create temp dir");
	dir
}

fn zip_writer(path: &Path) -> UfoWriter {
	UfoWriter::new(
		path,
		UfoWriterOptions {
			structure: Some(UfoFileStructure::Zip),
			..Default::default()
		},
	)
	.expect("writer")
}

/// The content of an archive member.
fn read_member(path: &Path, name: &str) -> Vec<u8> {
	let data = std::fs::read(path).expect("read archive");
	let mut archive = ZipArchive::new(Cursor::new(data)).expect("zip");
	let mut file = archive.by_name(name).expect("member");
	let mut out = Vec::new();
	file.read_to_end(&mut out).expect("read member");
	out
}

/// The member written as `hello.txt`, below the root directory of the UFO.
fn hello_member(path: &Path) -> String {
	let stem = path
		.file_stem()
		.map(|s| s.to_string_lossy().into_owned())
		.unwrap_or_default();
	format!("{stem}.ufo/hello.txt")
}

#[test]
fn test_writezipfs_repeated_close() {
	for suffix in [".zip", ".ufoz"] {
		let tmp = tmp_path("close");
		let path = tmp.join(format!("archive{suffix}"));
		let mut fs = zip_writer(&path);
		fs.write_bytes_to_path("hello.txt", b"hello")
			.expect("write");
		fs.close().expect("close");
		let archive = std::fs::read(&path).expect("read");

		fs.close().expect("close again");

		assert_eq!(std::fs::read(&path).expect("read"), archive);
		assert_eq!(read_member(&path, &hello_member(&path)), b"hello");
		drop(fs);
		assert_eq!(std::fs::read(&path).expect("read"), archive);
		let _ = std::fs::remove_dir_all(tmp);
	}
}

#[test]
fn test_writezipfs_explicit_close_in_context_manager() {
	for suffix in [".zip", ".ufoz"] {
		let tmp = tmp_path("context");
		let path = tmp.join(format!("archive{suffix}"));
		{
			let mut fs = zip_writer(&path);
			fs.write_bytes_to_path("hello.txt", b"hello")
				.expect("write");
			fs.close().expect("close");
			// dropped here, as at the end of the `with` block
		}
		assert_eq!(read_member(&path, &hello_member(&path)), b"hello");
		let _ = std::fs::remove_dir_all(tmp);
	}
}

#[test]
fn test_writezipfs_preserves_sibling_archive() {
	for suffix in [".ufoz", ".ZIP", ""] {
		let tmp = tmp_path("sibling");
		let path = tmp.join(format!("archive{suffix}"));
		let sibling = tmp.join("archive.zip");
		let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
		zip.start_file("original.txt", SimpleFileOptions::default())
			.expect("zip entry");
		zip.write_all(b"keep this archive").expect("zip data");
		let original = zip.finish().expect("zip").into_inner();
		std::fs::write(&sibling, &original).expect("write");
		if path.exists() {
			// Output and sibling names alias on this filesystem
			let _ = std::fs::remove_dir_all(tmp);
			continue;
		}

		{
			let fs = zip_writer(&path);
			fs.write_bytes_to_path("hello.txt", b"hello")
				.expect("write");
		}

		assert_eq!(std::fs::read(&sibling).expect("read"), original);
		assert_eq!(read_member(&path, &hello_member(&path)), b"hello");
		let _ = std::fs::remove_dir_all(tmp);
	}
}
