//! Regression tests of `maxp` compilation (no upstream counterpart).

mod common;

use common::{TestResult, upstream};
use fontoxide_ttlib::tables::Maxp;
use fontoxide_ttlib::{Font, ReadOptions, Tag, WriteOptions};

fn read_test_font() -> Result<Font, Box<dyn std::error::Error>> {
	let data = std::fs::read(upstream("ttLib/data/Test-Regular.ttf"))?;
	Ok(Font::read(data, ReadOptions::default())?)
}

/// Saving a font whose `glyf` is loaded but `hmtx` is not: recalculating `maxp`
/// loads `hmtx`, whose decompilation needs `maxp` while it is being compiled.
/// fontTools saves such a font fine.
#[test]
fn test_save_with_only_glyf_loaded() -> TestResult {
	let mut font = read_test_font()?;
	font.load(Tag::new(b"glyf"))?;
	assert!(!font.is_loaded(Tag::new(b"hmtx")));
	let data = font.write(&WriteOptions::default())?;

	// Same result as with `hmtx` loaded up front.
	let mut expected_font = read_test_font()?;
	expected_font.load(Tag::new(b"hmtx"))?;
	expected_font.load(Tag::new(b"glyf"))?;
	let expected = expected_font.write(&WriteOptions::default())?;
	assert_eq!(data, expected);

	let mut reread = Font::read(data, ReadOptions::default())?;
	let num_glyphs = reread.glyph_order()?.len() as u32;
	assert_eq!(reread.require::<Maxp>()?.num_glyphs, num_glyphs);
	Ok(())
}
