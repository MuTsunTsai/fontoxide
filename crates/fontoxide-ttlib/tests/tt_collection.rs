//! `Tests/ttLib/ttCollection_test.py`.
//!
//! fontoxide reads everything up front, so the `lazy` parametrizations run once.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use fontoxide_misc::text_tools::de_hex_str;
use fontoxide_ttlib::sfnt::{TTC_V1_1, TTC_V2_1, read_ttc_header};
use fontoxide_ttlib::tables::{Dsig, Head, Maxp};
use fontoxide_ttlib::{Font, ReadOptions, Table, TableType, TtCollection};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn ttx_data(name: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttx/data")
		.join(name)
}

fn read(name: &str) -> fontoxide_ttlib::Result<TtCollection> {
	TtCollection::read(std::fs::read(ttx_data(name))?, ReadOptions::default())
}

/// `deHexStr`.
fn hex(text: &str) -> Vec<u8> {
	de_hex_str(text).expect("hex data")
}

fn num_glyphs(font: &mut Font) -> fontoxide_ttlib::Result<u32> {
	Ok(font.require::<Maxp>()?.num_glyphs)
}

/// A DSIG table without signatures, compiled.
fn empty_dsig() -> fontoxide_ttlib::Result<Vec<u8>> {
	let mut dsig = Dsig {
		version: 1,
		num_sigs: 0,
		flag: 0,
		signature_records: Some(Vec::new()),
	};
	dsig.compile(&mut Font::new())
}

/// `test_lazy_open_path` and `test_lazy_open_file`: fontoxide reads from bytes.
#[test]
fn test_lazy_open_path() -> TestResult {
	let mut collection = read("TestTTC.ttc")?;
	assert_eq!(collection.fonts.len(), 2);
	assert_eq!(num_glyphs(&mut collection.fonts[0])?, 6);
	assert_eq!(num_glyphs(&mut collection.fonts[1])?, 6);
	Ok(())
}

#[test]
fn test_extended_ttc_roundtrip() -> TestResult {
	for (with_dsig, expected_version, expected_header_size) in
		[(false, TTC_V1_1, 24), (true, TTC_V2_1, 36)]
	{
		let mut source = read("TestTTC.ttc")?;
		let mut legacy = source.fonts.remove(0);
		let mut extended = source.fonts.remove(0);
		legacy.require::<Head>()?.font_revision = 1.25;
		extended.require::<Head>()?.font_revision = 2.5;

		let mut collection = TtCollection {
			fonts: vec![extended],
			legacy_fonts: Some(vec![legacy]),
			dsig: None,
		};
		if with_dsig {
			collection.dsig = Some(Some(empty_dsig()?));
		}
		let data = collection.write(true)?;

		let header = read_ttc_header(&data)?;
		assert_eq!(header.version, expected_version);
		assert_eq!(header.num_fonts, 1);
		let offset_table2 = header.offset_table2.ok_or("no second offset table")?;
		assert_eq!(offset_table2.len(), 1);
		assert_eq!(header.offset_table[0], expected_header_size);
		assert!(offset_table2[0] > header.offset_table[0]);

		let mut collection = TtCollection::read(data.clone(), ReadOptions::default())?;
		assert_eq!(collection.fonts.len(), 1);
		let legacy_fonts = collection.legacy_fonts.as_mut().ok_or("no legacy fonts")?;
		assert_eq!(legacy_fonts.len(), 1);
		assert_eq!(legacy_fonts[0].require::<Head>()?.font_revision, 1.25);
		assert_eq!(collection.fonts[0].require::<Head>()?.font_revision, 2.5);
		assert_eq!(collection.dsig.is_some(), with_dsig);

		let mut font = Font::read(
			data,
			ReadOptions {
				font_number: Some(0),
				..Default::default()
			},
		)?;
		assert_eq!(font.require::<Head>()?.font_revision, 2.5);
	}
	Ok(())
}

#[test]
fn test_save_ttc_v2_dsig() -> TestResult {
	let mut ttc = read("TestTTC.ttc")?;
	ttc.dsig = Some(Some(empty_dsig()?));
	let data = ttc.write(true)?;
	assert_eq!(
		data[..32],
		hex(concat!(
			"74 74 63 66", // ttfc
			"00 02 00 00", // ttc version 2
			"00 00 00 02", // number of fonts
			"00 00 00 20", // font 0 offset
			"00 00 09 40", // font 1 offset
			"44 53 49 47", // ulDsigTag
			"00 00 00 08", // ulDsigLength
			"00 00 0A 3C", // ulDsigOffset
		))[..]
	);
	// The DSIG table is at the end.
	assert_eq!(
		data[data.len() - 8..],
		hex(concat!(
			"00 00 00 01", // DSIG table version
			"00 00 00 00", // numSignatures, flags
		))[..]
	);

	let mut collection = TtCollection::read(data, ReadOptions::default())?;
	assert_eq!(collection.fonts.len(), 2);
	assert_eq!(num_glyphs(&mut collection.fonts[0])?, 6);
	assert_eq!(num_glyphs(&mut collection.fonts[1])?, 6);
	let dsig_data = collection.dsig.flatten().ok_or("no DSIG")?;
	let dsig = Dsig::decompile(&dsig_data, &mut Font::new())?;
	assert_eq!(dsig.version, 1);
	assert_eq!(dsig.num_sigs, 0);
	assert_eq!(dsig.flag, 0);
	Ok(())
}

#[test]
fn test_save_ttc_v2_dsig_none() -> TestResult {
	let mut ttc = read("TestTTC.ttc")?;
	ttc.dsig = Some(None);
	let data = ttc.write(true)?;
	assert_eq!(
		data[..32],
		hex(concat!(
			"74 74 63 66", // ttfc
			"00 02 00 00", // ttc version 2
			"00 00 00 02", // number of fonts
			"00 00 00 20", // font 0 offset
			"00 00 09 40", // font 1 offset
			"00 00 00 00", // ulDsigTag
			"00 00 00 00", // ulDsigLength
			"00 00 00 00", // ulDsigOffset
		))[..]
	);
	// The end is identical to the original.
	assert_eq!(data[data.len() - 8..], hex("00 00 04 88 00 00 00 01")[..]);

	let mut collection = TtCollection::read(data, ReadOptions::default())?;
	assert_eq!(collection.fonts.len(), 2);
	assert_eq!(num_glyphs(&mut collection.fonts[0])?, 6);
	assert_eq!(num_glyphs(&mut collection.fonts[1])?, 6);
	assert_eq!(collection.dsig, Some(None));
	Ok(())
}

/// The collection holds the DSIG table compiled; the step that fails in
/// fontTools' `save` is compiling an incomplete table.
#[test]
fn test_save_ttc_v2_dsig_invalid() {
	let mut dsig = Dsig::default();
	assert!(dsig.compile(&mut Font::new()).is_err());
}

/// The header and the end of `TestTTCv2.ttc` saved again.
fn check_ttc_v2(data: &[u8]) {
	assert_eq!(
		data[..32],
		hex(concat!(
			"74 74 63 66", // ttfc
			"00 02 00 00", // ttc version 2
			"00 00 00 02", // number of fonts
			"00 00 00 20", // font 0 offset
			"00 00 09 3C", // font 1 offset
			"44 53 49 47", // ulDsigTag
			"00 00 00 08", // ulDsigLength
			"00 00 0A 38", // ulDsigOffset
		))[..]
	);
	assert_eq!(
		data[data.len() - 8..],
		hex(concat!(
			"00 00 00 01", // DSIG table version
			"00 00 00 00", // numSignatures, flags
		))[..]
	);
}

#[test]
fn test_roundtrip_ttc_dsig() -> TestResult {
	let mut collection = read("TestTTCv2.ttc")?;
	assert_eq!(collection.fonts.len(), 2);
	assert_eq!(num_glyphs(&mut collection.fonts[0])?, 6);
	assert_eq!(num_glyphs(&mut collection.fonts[1])?, 6);
	assert!(collection.dsig.is_some());
	check_ttc_v2(&collection.write(true)?);
	Ok(())
}

#[test]
fn test_roundtrip_ttc_dsig_decompile() -> TestResult {
	let mut collection = read("TestTTCv2.ttc")?;
	assert_eq!(collection.fonts.len(), 2);
	assert_eq!(num_glyphs(&mut collection.fonts[0])?, 6);
	assert_eq!(num_glyphs(&mut collection.fonts[1])?, 6);
	// Decompiling the DSIG table and compiling it again.
	let data = collection.dsig.clone().flatten().ok_or("no DSIG")?;
	let mut dsig = Dsig::decompile(&data, &mut Font::new())?;
	collection.dsig = Some(Some(dsig.compile(&mut Font::new())?));
	check_ttc_v2(&collection.write(true)?);
	Ok(())
}

static TICKS: AtomicU64 = AtomicU64::new(0);

/// A clock that advances a day on every call.
fn ticking_clock() -> f64 {
	(TICKS.fetch_add(1, Ordering::SeqCst) + 1) as f64 * 86400.0
}

#[test]
fn test_save_ttc_deterministic_across_clock_tick() -> TestResult {
	let save_bytes = || -> fontoxide_ttlib::Result<Vec<u8>> {
		let mut ttc = read("TestTTCv2.ttc")?;
		ttc.write(true)
	};
	// The other tests of this file do not depend on the clock.
	fontoxide_ttlib::set_clock(|| 1000.0);
	let steady = save_bytes()?;
	fontoxide_ttlib::set_clock(ticking_clock);
	let ticking = save_bytes()?;

	// A clock tick does not change the output: the shared 'head' is written once.
	assert_eq!(ticking.len(), steady.len());
	let mut ttc = TtCollection::read(ticking, ReadOptions::default())?;
	let modified0 = ttc.fonts[0].require::<Head>()?.modified;
	let modified1 = ttc.fonts[1].require::<Head>()?.modified;
	assert_eq!(modified0, modified1);
	Ok(())
}
