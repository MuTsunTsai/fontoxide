//! `fontTools.ttLib.macUtils` and `TTFont(path, res_name_or_index)`, which
//! fontTools has no tests of: [`fontoxide_ttlib::mac_utils`] on the `.dfont`
//! file of `Tests/ttx/data`, with the results of fontTools.

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use fontoxide_ttlib::mac_utils::{
	ResNameOrIndex, get_sfnt_res_indices, open_tt_fonts, sfnt_resource,
};
use fontoxide_ttlib::{Font, ReadOptions};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn data(name: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttx/data")
		.join(name)
}

#[test]
fn get_sfnt_res_indices_of_dfont_and_ttf() -> TestResult {
	assert_eq!(get_sfnt_res_indices(data("TestDFONT.dfont"))?, [1]);
	assert!(get_sfnt_res_indices(data("TestTTF.ttf"))?.is_empty());
	Ok(())
}

#[test]
fn open_dfont() -> TestResult {
	let path = data("TestDFONT.dfont");
	for res in [ResNameOrIndex::Index(0), ResNameOrIndex::Index(1)] {
		let font = Font::open(&path, Some(&res), ReadOptions::default())?;
		let tags: Vec<String> = font.tags().iter().map(|t| t.as_string()).collect();
		let mut sorted = tags.clone();
		sorted.sort();
		assert_eq!(
			sorted,
			[
				"FFTM", "GDEF", "OS/2", "cmap", "cvt ", "fpgm", "gasp", "glyf", "head", "hhea",
				"hmtx", "loca", "maxp", "name", "post", "prep"
			]
		);
	}
	assert_eq!(sfnt_resource(&path, &ResNameOrIndex::Index(1))?.len(), 2488);
	// The only name is that of the `FOND` resource.
	let error = Font::open(
		&path,
		Some(&ResNameOrIndex::Name("Test TTF".into())),
		ReadOptions::default(),
	)
	.unwrap_err();
	assert_eq!(error.to_string(), "sfnt resource not found: Test TTF");
	let error = Font::open(
		&path,
		Some(&ResNameOrIndex::Index(2)),
		ReadOptions::default(),
	)
	.unwrap_err();
	assert_eq!(error.to_string(), "sfnt resource not found: 2");
	// Without `res_name_or_index`, the file is read as an sfnt.
	let error = Font::open(&path, None, ReadOptions::default()).unwrap_err();
	assert_eq!(
		error.to_string(),
		"Not a TrueType or OpenType font (bad sfntVersion)"
	);
	Ok(())
}

#[test]
fn open_ttf() -> TestResult {
	let path = data("TestTTF.ttf");
	// Index 0 falls back to the file itself.
	let mut font = Font::open(
		&path,
		Some(&ResNameOrIndex::Index(0)),
		ReadOptions::default(),
	)?;
	let expected = Font::read(std::fs::read(&path)?, ReadOptions::default())?.tags();
	assert_eq!(font.tags(), expected);
	font.glyph_order()?;
	let error = Font::open(
		&path,
		Some(&ResNameOrIndex::Index(1)),
		ReadOptions::default(),
	)
	.unwrap_err();
	assert_eq!(
		error.to_string(),
		"ResourceError: Cannot read resource (not enough data)"
	);
	Ok(())
}

#[test]
fn open_tt_fonts_of_dfont_and_ttf() -> TestResult {
	assert_eq!(open_tt_fonts(data("TestDFONT.dfont"))?.len(), 1);
	assert_eq!(open_tt_fonts(data("TestTTF.ttf"))?.len(), 1);
	Ok(())
}
