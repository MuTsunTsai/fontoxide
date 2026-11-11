//! `Tests/ttLib/sfnt_test.py`.
//!
//! fontTools' tests of `SFNTDirectoryEntry`, `WOFFDirectoryEntry` and
//! `WOFFFlavorData` call them on streams; here the same data goes through
//! small sfnt and WOFF files.

#![allow(non_snake_case)]

use std::path::PathBuf;

use fontoxide_ttlib::binary::calc_checksum;
use fontoxide_ttlib::sfnt::{
	SFNT_DIRECTORY_ENTRY_SIZE, SFNT_DIRECTORY_SIZE, SfntReader, SfntReaderOptions,
	ZLIB_COMPRESSION_LEVEL, zlib_compress,
};
use fontoxide_ttlib::tables::Head;
use fontoxide_ttlib::{Error, Flavor, FlavorData, Font, ReadOptions, Tag, WriteOptions};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The `ttfont_path` fixture: `TestTTF-Regular.ttx` compiled.
fn ttfont_data() -> fontoxide_ttlib::Result<Vec<u8>> {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttLib/data/TestTTF-Regular.ttx");
	let mut font = Font::from_ttx(&std::fs::read_to_string(path)?)?;
	font.write(&WriteOptions::default())
}

fn read(data: Vec<u8>) -> fontoxide_ttlib::Result<Font> {
	Font::read(data, ReadOptions::default())
}

/// Asserts a `TTLibError` whose message contains `expected`.
fn assert_ttlib_error<T: std::fmt::Debug>(result: fontoxide_ttlib::Result<T>, expected: &str) {
	match result {
		Err(Error::TtLib(message)) => assert!(message.contains(expected), "{message}"),
		other => panic!("expected TTLibError({expected:?}), got {other:?}"),
	}
}

#[test]
fn test_calcChecksum() {
	assert_eq!(calc_checksum(b"abcd"), 1633837924);
	assert_eq!(calc_checksum(b"abcdxyz"), 3655064932);
}

/// `SFNTDirectoryEntry.loadData` past the end of the data: an sfnt file whose
/// only table record claims 1000 bytes.
#[test]
fn test_loadData_truncated_raises_TTLibError() -> TestResult {
	let mut data = Vec::new();
	data.extend_from_slice(b"\0\x01\0\0");
	data.extend_from_slice(&[0, 1, 0, 16, 0, 0, 0, 0]);
	data.extend_from_slice(b"test");
	data.extend_from_slice(&0u32.to_be_bytes()); // checkSum
	data.extend_from_slice(&0u32.to_be_bytes()); // offset
	data.extend_from_slice(&1000u32.to_be_bytes()); // length
	let reader = SfntReader::new(data, SfntReaderOptions::default())?;
	assert_ttlib_error(
		reader.table_data(Tag::new(b"test")),
		"unexpected end of 'test' table data",
	);
	Ok(())
}

#[test]
fn test_load_truncated_font_raises_TTLibError() -> TestResult {
	// Cut inside the last table, whatever its padding.
	let mut data = ttfont_data()?;
	data.truncate(data.len() - 4);
	let mut font = read(data)?;
	let mut error = None;
	for tag in font.tags() {
		if let Err(e) = font.load(tag) {
			error = Some(e);
			break;
		}
	}
	let error = error.ok_or("no table failed to load")?;
	let message = error.to_string();
	let start = message
		.find("unexpected end of '")
		.ok_or_else(|| format!("unexpected error: {message}"))?;
	let rest = &message[start + "unexpected end of '".len()..];
	assert_eq!(rest.get(4..20), Some("' table data: ex"), "{message}");
	assert!(matches!(error.root_cause(), Error::TtLib(_)), "{error:?}");
	Ok(())
}

#[test]
fn test_truncated_table_directory_raises_TTLibError() -> TestResult {
	let mut data = ttfont_data()?;
	data.truncate(SFNT_DIRECTORY_SIZE + SFNT_DIRECTORY_ENTRY_SIZE / 2);
	assert_ttlib_error(read(data), "unexpected end of table directory");
	Ok(())
}

#[test]
fn test_bogus_numTables_raises_TTLibError() -> TestResult {
	let mut data = ttfont_data()?;
	data[5] ^= 0xFF;
	assert_ttlib_error(read(data), "unexpected end of table directory");
	Ok(())
}

/// A WOFF file: the header with the given metadata and private data blocks
/// (offset, length[, original length]), the tables (tag, compressed data,
/// original length), and `tail` at the end.
fn woff(
	meta: (u32, u32, u32),
	private: (u32, u32),
	tables: &[(&[u8; 4], &[u8], u32)],
	tail: &[u8],
) -> Vec<u8> {
	let mut directory = Vec::new();
	let mut body = Vec::new();
	let data_start = 44 + 20 * tables.len();
	for (tag, data, orig_length) in tables {
		directory.extend_from_slice(*tag);
		directory.extend_from_slice(&((data_start + body.len()) as u32).to_be_bytes());
		directory.extend_from_slice(&(data.len() as u32).to_be_bytes());
		directory.extend_from_slice(&orig_length.to_be_bytes());
		directory.extend_from_slice(&0u32.to_be_bytes()); // origChecksum
		body.extend_from_slice(data);
		while body.len() % 4 != 0 {
			body.push(0);
		}
	}
	let length = (data_start + body.len() + tail.len()) as u32;
	let mut file = Vec::new();
	file.extend_from_slice(b"wOFF");
	file.extend_from_slice(b"\0\x01\0\0");
	file.extend_from_slice(&length.to_be_bytes());
	file.extend_from_slice(&(tables.len() as u16).to_be_bytes());
	file.extend_from_slice(&[0, 0]); // reserved
	file.extend_from_slice(&0u32.to_be_bytes()); // totalSfntSize
	file.extend_from_slice(&[0, 1, 0, 0]); // majorVersion, minorVersion
	for v in [meta.0, meta.1, meta.2, private.0, private.1] {
		file.extend_from_slice(&v.to_be_bytes());
	}
	file.extend_from_slice(&directory);
	file.extend_from_slice(&body);
	file.extend_from_slice(tail);
	file
}

#[test]
fn test_woff_table_bad_compressed_length_raises_TTLibError() -> TestResult {
	// A compressed table must be smaller than its original size.
	let mut compressed = zlib_compress(&[b'A'; 10], ZLIB_COMPRESSION_LEVEL);
	compressed.resize(20, 0);
	let font = read(woff((0, 0, 0), (0, 0), &[(b"test", &compressed, 10)], b""))?;
	let reader = font.reader().ok_or("no reader")?;
	assert_ttlib_error(
		reader.table_data(Tag::new(b"test")),
		"corrupt WOFF table 'test'",
	);
	Ok(())
}

#[test]
fn test_woff_table_decompressed_size_mismatch_raises_TTLibError() -> TestResult {
	let payload = [b'A'; 100];
	let raw_data = zlib_compress(&payload, ZLIB_COMPRESSION_LEVEL);
	// Lie about the original size.
	let orig_length = payload.len() as u32 + 5;
	assert!((raw_data.len() as u32) < orig_length);
	let font = read(woff(
		(0, 0, 0),
		(0, 0),
		&[(b"test", &raw_data, orig_length)],
		b"",
	))?;
	let reader = font.reader().ok_or("no reader")?;
	assert_ttlib_error(
		reader.table_data(Tag::new(b"test")),
		"unexpected size for decompressed WOFF table 'test'",
	);
	Ok(())
}

#[test]
fn test_woff_metadata_truncated_raises_TTLibError() {
	// 100 bytes of metadata at the start of a 5-byte tail.
	assert_ttlib_error(
		read(woff((44, 100, 100), (0, 0), &[], b"short")),
		"unexpected end of WOFF metadata",
	);
}

#[test]
fn test_woff_metadata_size_mismatch_raises_TTLibError() {
	let meta = b"<metadata/>";
	let compressed = zlib_compress(meta, ZLIB_COMPRESSION_LEVEL);
	// Lie about the uncompressed size.
	let header = (44, compressed.len() as u32, meta.len() as u32 + 3);
	assert_ttlib_error(
		read(woff(header, (0, 0), &[], &compressed)),
		"unexpected size for decompressed WOFF metadata",
	);
}

#[test]
fn test_woff_privData_truncated_raises_TTLibError() {
	assert_ttlib_error(
		read(woff((0, 0, 0), (44, 100), &[], b"short")),
		"unexpected end of WOFF private data",
	);
}

#[test]
fn test_ttLib_sfnt_write_privData() -> TestResult {
	let mut font = read(ttfont_data()?)?;
	let priv_data = b"Private Eyes".to_vec();
	let mut data = FlavorData::new(Flavor::Woff);
	let revision = format!("{:.3}", font.require::<Head>()?.font_revision);
	let (major, minor) = revision.split_once('.').ok_or("no dot")?;
	data.major_version = Some(major.parse()?);
	data.minor_version = Some(minor.parse()?);
	data.priv_data = Some(priv_data.clone());
	font.flavor = Some(Flavor::Woff);
	font.flavor_data = Some(data);
	let output = font.write(&WriteOptions::default())?;

	let font = read(output)?;
	let flavor_data = font.flavor_data.as_ref().ok_or("no flavor data")?;
	assert_eq!(flavor_data.priv_data.as_ref(), Some(&priv_data));
	Ok(())
}
