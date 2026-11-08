//! Drawing glyphs through a [`GlyphSet`], compared with fontTools'
//! `TTFont.getGlyphSet()` and `BoundsPen` on its `VARC` test fonts.

use std::path::PathBuf;

use fontoxide_pens::BoundsPen;
use fontoxide_ttlib::{Font, GlyphSet, ReadOptions, Result};

type Bounds = Option<[f64; 4]>;

/// The bounds of every glyph, as fontTools computes them.
const EXPECTED: &[(&str, &[(&str, Bounds)])] = &[
	(
		"ttLib/data/varc-6868.ttf",
		&[
			(".notdef", None),
			(
				"uni6868",
				Some([
					50.722889005690945,
					-71.20580673217773,
					947.9224070891651,
					831.5889850884232,
				]),
			),
			(
				"glyph00002",
				Some([
					44.09370686307888,
					-77.9921875,
					898.9346923828125,
					938.3757062011948,
				]),
			),
			("glyph00003", Some([0.0, 0.0, 1000.0, 75.0])),
			("glyph00004", Some([-66.0, -64.0, 0.0, 0.0])),
			(
				"glyph00005",
				Some([212.82153246920194, -70.0, 543.0, 830.390625]),
			),
			("glyph00006", Some([-2000.0, 0.0, 1000.0, 1000.0])),
			(
				"glyph00007",
				Some([65.98086262037884, -70.046875, 939.5692120203089, 829.765625]),
			),
			("glyph00008", Some([0.0, 0.0, 1000.0, 75.0])),
			("glyph00009", Some([0.0, -120.0, 1000.0, 880.0])),
		],
	),
	(
		"ttLib/data/varc-ac00-ac01.ttf",
		&[
			(".notdef", None),
			("uniAC00", Some([50.206988965879646, -82.0, 887.0, 803.0])),
			(
				"uniAC01",
				Some([58.77988662192911, -93.4794921875, 894.0, 803.0]),
			),
			(
				"glyph00003",
				Some([
					50.206988965879646,
					118.50342495677887,
					507.9096544100603,
					722.0,
				]),
			),
			("glyph00004", Some([-7.0, -35.0, 840.0, 698.0])),
			("glyph00005", Some([666.0, -82.0, 887.0, 803.0])),
			("glyph00006", Some([679.0, -82.0, 900.0, 803.0])),
			(
				"glyph00007",
				Some([
					58.77988662192911,
					284.1153125260489,
					507.90479741730724,
					746.0,
				]),
			),
			("glyph00008", Some([673.0, 285.7421875, 894.0, 803.0])),
			(
				"glyph00009",
				Some([196.0, -93.4794921875, 749.14111328125, 236.0]),
			),
			("glyph00010", Some([90.0, -35.0, 840.0, 698.0])),
		],
	),
	(
		"ttLib/data/varc-ac01-conditional.ttf",
		&[
			(".notdef", None),
			(
				"uniAC01",
				Some([58.77988662192911, -93.4794921875, 749.14111328125, 746.0]),
			),
			(
				"glyph00002",
				Some([
					58.77988662192911,
					284.1153125260489,
					507.90479741730724,
					746.0,
				]),
			),
			("glyph00003", Some([-7.0, -35.0, 840.0, 698.0])),
			("glyph00004", Some([673.0, 285.7421875, 894.0, 803.0])),
			("glyph00005", Some([679.0, -82.0, 900.0, 803.0])),
			(
				"glyph00006",
				Some([196.0, -93.4794921875, 749.14111328125, 236.0]),
			),
			("glyph00007", Some([90.0, -35.0, 840.0, 698.0])),
		],
	),
	(
		"ttLib/data/varc-static-gvar.ttf",
		&[
			(".notdef", None),
			("a", Some([50.0, 0.0, 450.0, 500.0])),
			("b", Some([0.0, 0.0, 400.0, 500.0])),
		],
	),
	(
		"cffLib/data/varc-short-cff2.otf",
		&[
			(".notdef", None),
			("leaf", Some([100.0, 0.0, 200.0, 100.0])),
			("unused", None),
			("composite", Some([600.0, 0.0, 800.0, 200.0])),
		],
	),
];

#[test]
fn varc_bounds_match_fonttools() -> Result<()> {
	let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../upstream/fonttools/Tests");
	for (path, glyphs) in EXPECTED {
		let data = std::fs::read(dir.join(path)).expect("fontTools' test fonts are missing");
		let mut font = Font::read(data, ReadOptions::default())?;
		let glyph_set = GlyphSet::new(&mut font)?;
		for (name, expected) in *glyphs {
			let mut pen = BoundsPen::new_bounds(Some(&glyph_set));
			glyph_set.draw(name, &mut pen)?;
			assert_eq!(pen.bounds(), *expected, "{path}: {name}");
		}
	}
	Ok(())
}
