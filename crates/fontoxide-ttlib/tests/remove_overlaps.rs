//! The tests of fontTools' `Tests/ttLib/removeOverlaps_test.py`.

#![cfg(feature = "pathops")]

use std::path::PathBuf;

use fontoxide_ttlib::remove_overlaps::{RemoveOverlapsOptions, remove_overlaps};
use fontoxide_ttlib::tables::glyf::{FLAG_ON_CURVE, Glyph};
use fontoxide_ttlib::tables::tt_program::Program;
use fontoxide_ttlib::tables::{Cff, Glyf, Hmtx, Loca, Maxp};
use fontoxide_ttlib::{Font, ReadOptions, TableType, Tag, WriteOptions};

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

#[test]
fn test_remove_overlaps_beyond64k_glyf() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(vec![".notdef".to_owned(), "A".to_owned()]);

	let coordinates = vec![
		(0.0, 0.0),
		(100.0, 0.0),
		(100.0, 100.0),
		(0.0, 100.0),
		(50.0, 0.0),
		(150.0, 0.0),
		(150.0, 100.0),
		(50.0, 100.0),
	];
	let mut glyf = Glyf::new_as(Tag::new(b"GLYF"));
	glyf.glyphs.insert(
		0,
		Glyph {
			program: Some(Program::default()),
			..Default::default()
		},
	);
	glyf.glyphs.insert(
		1,
		Glyph {
			number_of_contours: 2,
			x_min: 0,
			y_min: 0,
			x_max: 150,
			y_max: 100,
			end_pts_of_contours: vec![3, 7],
			flags: vec![FLAG_ON_CURVE; coordinates.len()],
			coordinates,
			program: Some(Program::default()),
			..Default::default()
		},
	);
	font.insert(Box::new(glyf));
	font.insert(Box::new(Loca::new_as(Tag::new(b"LOCA"))));
	font.insert(Box::new(Maxp::new_as(Tag::new(b"MAXP"))));
	let mut hmtx = Hmtx::new_as(Tag::new(b"HMTX"));
	hmtx.set(0, (500, 0));
	hmtx.set(1, (500, 10));
	font.insert(Box::new(hmtx));

	remove_overlaps(&mut font, &RemoveOverlapsOptions::default())?;

	assert!(font.contains(Tag::new(b"GLYF")));
	assert!(font.contains(Tag::new(b"HMTX")));
	assert_eq!(font.require::<Glyf>()?.glyphs[&1].number_of_contours, 1);
	assert_eq!(font.require::<Hmtx>()?.metrics[&1], (500, 10));
	Ok(())
}
