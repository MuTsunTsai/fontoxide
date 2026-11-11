//! Apple's bitmap tables `bdat`, `bloc` and `bhed` (`_b_d_a_t`, `_b_l_o_c`,
//! `_b_h_e_d`), which fontTools has no tests of: a font with these tables, built
//! by tools/reference/apple_bitmap_fixtures.py, read, dumped and compiled as
//! fontTools does.

use std::path::PathBuf;

use fontoxide_ttlib::tables::{Bdat, Bhed, Bloc, Ebdt, Eblc, Head};
use fontoxide_ttlib::{Font, ReadOptions, TableKey, Tag, TtxOptions, WriteOptions};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn data(name: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data/apple_bitmap")
		.join(name)
}

const NO_TIMESTAMP: ReadOptions = ReadOptions {
	font_number: None,
	check_checksums: 0,
	recalc_bboxes: true,
	recalc_timestamp: false,
	ignore_decompile_errors: false,
	use_legacy_ttc: false,
	use_harfbuzz_repacker: None,
	optimize_font_speed: false,
};

fn read() -> fontoxide_ttlib::Result<Font> {
	Font::read(std::fs::read(data("apple.ttf"))?, NO_TIMESTAMP)
}

const TAGS: [&[u8; 4]; 3] = [b"bhed", b"bloc", b"bdat"];

#[test]
fn tables_are_implemented() -> TestResult {
	let mut font = read()?;
	assert!(!font.contains(Tag::new(b"EBDT")));
	assert!(!font.contains(Tag::new(b"EBLC")));
	let bhed = font.require::<Bhed>()?;
	assert_eq!(bhed.units_per_em, 1000);
	let created = bhed.created;
	// A copy of `head`, from before fontTools set its checksum adjustment.
	assert_eq!(font.require::<Head>()?.created, created);
	let strikes = font.require::<Bloc>()?.strikes.len();
	assert_eq!(strikes, 2);
	let bdat = font.require::<Bdat>()?;
	assert_eq!(bdat.strike_data.len(), 2);
	for tag in TAGS {
		assert!(fontoxide_ttlib::table::is_implemented(Tag::new(tag)));
	}
	// The types are distinct from their OpenType counterparts.
	assert!(font.get::<Ebdt>()?.is_none());
	assert!(font.get::<Eblc>()?.is_none());
	Ok(())
}

#[test]
fn dumps_match_fonttools() -> TestResult {
	for (name, format) in [("apple.ttx", "raw"), ("apple.row.ttx", "row")] {
		let mut font = read()?;
		let text = font.to_ttx(&TtxOptions {
			tables: TAGS.iter().map(|t| TableKey::Table(Tag::new(t))).collect(),
			bitmap_glyph_data_format: format.into(),
			..Default::default()
		})?;
		let expected = std::fs::read_to_string(data(name))?;
		assert!(text == expected, "{name}:\n{text}");
	}
	Ok(())
}

#[test]
fn compile_matches_fonttools() -> TestResult {
	let mut font = read()?;
	for tag in TAGS {
		assert!(font.load(Tag::new(tag))?);
	}
	let compiled = font.write(&WriteOptions::default())?;
	assert!(compiled == std::fs::read(data("apple.recompiled.ttf"))?);
	Ok(())
}

#[test]
fn from_ttx_matches_fonttools() -> TestResult {
	let mut font = read()?;
	font.import_ttx(&std::fs::read_to_string(data("apple.ttx"))?)?;
	let compiled = font.write(&WriteOptions::default())?;
	assert!(compiled == std::fs::read(data("apple.fromttx.ttf"))?);
	Ok(())
}
