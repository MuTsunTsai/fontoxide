//! Round trips of `VARC` tables in fontTools' test fonts.
//!
//! The parity runner cannot compile these fonts yet, because recalculating the
//! `head` bounding box needs the glyphs drawn; so the table is compared alone.

use std::path::PathBuf;

use fontoxide_ttlib::{Font, ReadOptions, Result, Tag, TtxImportOptions, TtxOptions};

const VARC: Tag = Tag::new(b"VARC");

const FONTS: &[&str] = &[
	"ttLib/data/varc-6868.ttf",
	"ttLib/data/varc-ac00-ac01.ttf",
	"ttLib/data/varc-ac01-conditional.ttf",
	"ttLib/data/varc-static-gvar.ttf",
	"cffLib/data/varc-short-cff2.otf",
];

fn fonts() -> Vec<(PathBuf, Vec<u8>)> {
	let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../upstream/fonttools/Tests");
	let fonts: Vec<_> = FONTS
		.iter()
		.filter_map(|f| {
			let path = dir.join(f);
			std::fs::read(&path).ok().map(|data| (path, data))
		})
		.collect();
	assert_eq!(fonts.len(), FONTS.len(), "fontTools' test fonts are missing");
	fonts
}

fn read(data: Vec<u8>) -> Result<Font> {
	Font::read(
		data,
		ReadOptions {
			recalc_timestamp: false,
			..Default::default()
		},
	)
}

#[test]
fn binary_round_trip() -> Result<()> {
	for (path, data) in fonts() {
		let mut font = read(data)?;
		let original = font.table_data(VARC)?;
		font.load(VARC)?;
		assert_eq!(font.table_data(VARC)?, original, "{}", path.display());
	}
	Ok(())
}

#[test]
fn ttx_round_trip() -> Result<()> {
	for (path, data) in fonts() {
		let mut font = read(data)?;
		let original = font.table_data(VARC)?;
		let ttx = font.to_ttx(&TtxOptions::default())?;
		let mut imported = Font::new();
		imported.import_ttx_with(
			&ttx,
			&TtxImportOptions {
				skip_unimplemented: true,
			},
		)?;
		assert_eq!(imported.table_data(VARC)?, original, "{}", path.display());
	}
	Ok(())
}
