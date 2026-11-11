//! fontoxide without the features that are fontTools' extras behaves as
//! fontTools does without the Python package: these check the errors that
//! fontTools raises then. fontTools does not test this; run them with
//! `cargo test -p fontoxide-ttlib --no-default-features`.
//!
//! The `graphite` feature (lz4) is checked by the unit tests of
//! `tables::graphite`.

// Which helpers are used depends on the features that are off.
#![allow(dead_code, unused_imports)]

use std::path::PathBuf;

use fontoxide_ttlib::remove_overlaps::{RemoveOverlapsOptions, remove_overlaps};
use fontoxide_ttlib::tables::Gsub;
use fontoxide_ttlib::{Error, Flavor, FlavorData, Font, ReadOptions, TableType, WriteOptions};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn upstream(path: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests")
		.join(path)
}

/// Whether `result` failed with the `ImportError` `message`.
fn is_import_error<T>(result: fontoxide_ttlib::Result<T>, message: &str) -> bool {
	match result {
		Err(e) => matches!(e.root_cause(), Error::Import(m) if m == message),
		Ok(_) => false,
	}
}

fn woff_font() -> fontoxide_ttlib::Result<Font> {
	let data = std::fs::read(upstream("ttx/data/TestWOFF.woff"))?;
	Font::read(data, ReadOptions::default())
}

/// `WOFF2Reader` without brotli: `ImportError("No module named brotli")`.
#[test]
#[cfg(not(feature = "woff"))]
fn woff2_read_without_brotli() -> TestResult {
	let data = std::fs::read(upstream("ttx/data/TestWOFF2.woff2"))?;
	assert!(is_import_error(
		Font::read(data, ReadOptions::default()),
		"No module named brotli"
	));
	Ok(())
}

/// `WOFF2Writer` (through `TTFont.save`) and `WOFF2FlavorData` without brotli:
/// `ImportError("No module named brotli")`.
#[test]
#[cfg(not(feature = "woff"))]
fn woff2_write_without_brotli() -> TestResult {
	assert!(is_import_error(
		FlavorData::woff2(&[]),
		"No module named brotli"
	));
	let mut font = woff_font()?;
	font.flavor = Some(Flavor::Woff2);
	font.flavor_data = None;
	assert!(is_import_error(
		font.write(&WriteOptions::default()),
		"No module named brotli"
	));
	Ok(())
}

/// WOFF 1.0 only needs zlib; `sfnt.compress` with `USE_ZOPFLI` without zopfli:
/// `ModuleNotFoundError: No module named 'zopfli'`.
#[test]
#[cfg(not(feature = "woff"))]
fn woff_without_zopfli() -> TestResult {
	let mut font = woff_font()?;
	let data = font.write(&WriteOptions::default())?;
	assert_eq!(
		Font::read(data, ReadOptions::default())?.flavor,
		Some(Flavor::Woff)
	);
	let mut font = woff_font()?;
	assert!(is_import_error(
		font.write(&WriteOptions {
			use_zopfli: true,
			..Default::default()
		}),
		"No module named 'zopfli'"
	));
	Ok(())
}

/// `from fontTools.ttLib.removeOverlaps import removeOverlaps` without
/// skia-pathops: `ModuleNotFoundError: No module named 'pathops'`.
#[test]
#[cfg(not(feature = "pathops"))]
fn remove_overlaps_without_pathops() -> TestResult {
	let mut font = woff_font()?;
	assert!(is_import_error(
		remove_overlaps(&mut font, &RemoveOverlapsOptions::default()),
		"No module named 'pathops'"
	));
	Ok(())
}

fn gsub_font(use_harfbuzz_repacker: Option<bool>) -> Font {
	let mut font = Font::new();
	font.set_glyph_order(vec![".notdef".to_owned()]);
	font.use_harfbuzz_repacker = use_harfbuzz_repacker;
	font.insert(Box::new(Gsub {
		table: fontoxide_ttlib::ot::types::GSUB {
			version: 0x0001_0000,
			..Default::default()
		},
	}));
	font
}

/// `BaseTTXConverter.compile` without uharfbuzz: `USE_HARFBUZZ_REPACKER` True
/// raises `ImportError("No module named 'uharfbuzz'")`; the default (None)
/// packs with fontTools' serializer.
#[test]
#[cfg(not(feature = "repacker"))]
fn gsub_without_uharfbuzz() -> TestResult {
	assert!(is_import_error(
		gsub_font(Some(true)).table_data(Gsub::TAG),
		"No module named 'uharfbuzz'"
	));
	assert_eq!(
		gsub_font(None).table_data(Gsub::TAG)?,
		[0, 1, 0, 0, 0, 0, 0, 0, 0, 0]
	);
	assert_eq!(
		gsub_font(Some(false)).table_data(Gsub::TAG)?,
		[0, 1, 0, 0, 0, 0, 0, 0, 0, 0]
	);
	Ok(())
}

/// The same GSUB table with the repacker.
#[test]
#[cfg(feature = "repacker")]
fn gsub_with_uharfbuzz() -> TestResult {
	for option in [None, Some(true), Some(false)] {
		assert_eq!(
			gsub_font(option).table_data(Gsub::TAG)?,
			[0, 1, 0, 0, 0, 0, 0, 0, 0, 0]
		);
	}
	Ok(())
}
