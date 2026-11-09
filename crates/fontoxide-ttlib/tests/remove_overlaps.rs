//! The tests of fontTools' `Tests/ttLib/removeOverlaps_test.py`.

use std::path::PathBuf;

use fontoxide_ttlib::remove_overlaps::{RemoveOverlapsOptions, remove_overlaps};
use fontoxide_ttlib::tables::{Cff, Hmtx};
use fontoxide_ttlib::{Font, ReadOptions, WriteOptions};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn data(name: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttLib/data")
		.join(name)
}

fn width_of(font: &mut Font, name: &str) -> Result<(Option<f64>, u16), Box<dyn std::error::Error>> {
	let order = font.glyph_order()?;
	let gid = order.id(name).ok_or("no such glyph")?;
	let advance = font.require::<Hmtx>()?.metrics[&gid].0;
	let (width, _, _) = font.require::<Cff>()?.cff.char_string_width(gid as usize)?;
	Ok((width, advance))
}

#[test]
fn cff_char_string_width_nominal_width_x() -> TestResult {
	let mut font = Font::read(
		std::fs::read(data("IBMPlexSans-Bold.subset.otf"))?,
		ReadOptions::default(),
	)?;
	assert_eq!(width_of(&mut font, "OE")?, (Some(998.0), 998));

	remove_overlaps(&mut font, &RemoveOverlapsOptions::default())?;
	assert_eq!(width_of(&mut font, "OE")?, (Some(998.0), 998));

	// It stays so when the font is saved.
	let mut font = Font::read(
		font.write(&WriteOptions::default())?,
		ReadOptions::default(),
	)?;
	assert_eq!(width_of(&mut font, "OE")?, (Some(998.0), 998));
	Ok(())
}

#[test]
fn no_outlines() {
	let mut font = Font::new();
	assert!(remove_overlaps(&mut font, &RemoveOverlapsOptions::default()).is_err());
}
