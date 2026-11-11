//! `Tests/ttLib/tables/T_S_I__5_test.py`.

mod common;

use std::collections::BTreeMap;

use common::{TestResult, hex, names};
use fontoxide_ttlib::tables::{Maxp, Tsi5};
use fontoxide_ttlib::{Font, Table, TableType};

/// The test's `Font`: three glyphs, and `maxp.numGlyphs` as given.
fn font(num_glyphs: u16) -> Font {
	let mut font = Font::new();
	font.set_glyph_order(names(&[".notdef", "I", "space"]));
	font.insert(Box::new(Maxp {
		num_glyphs: num_glyphs.into(),
		..Default::default()
	}));
	font
}

/// `TSI5_GLYPHGROUPS`, by glyph ID.
fn glyph_groups() -> BTreeMap<u32, i64> {
	[(0, 1), (1, 1), (2, 1)].into_iter().collect()
}

#[test]
fn test_decompile() -> TestResult {
	// "simple", "fewer entries", "fewer glyphs".
	for num_glyphs in [3, 4, 2] {
		let mut font = font(num_glyphs);
		let table = Tsi5::decompile(&hex("0001 0001 0001"), &mut font)?;
		assert_eq!(table.glyph_grouping.len(), 3);
		assert_eq!(table.glyph_grouping, glyph_groups());
	}
	Ok(())
}

#[test]
fn test_compile() -> TestResult {
	for num_glyphs in [3, 4, 2] {
		let mut table = Tsi5 {
			glyph_grouping: glyph_groups(),
		};
		assert_eq!(table.compile(&mut font(num_glyphs))?, hex("0001 0001 0001"));
	}
	Ok(())
}

/// TTX round trip, checked against fontTools.
#[test]
fn xml_round_trip() -> TestResult {
	let mut font = font(3);
	let table = Tsi5 {
		glyph_grouping: [(0, 1), (2, 7)].into_iter().collect(),
	};
	let xml = common::get_xml(&table, &mut font)?;
	assert_eq!(
		xml,
		[
			"<glyphgroup name=\".notdef\" value=\"1\"/>",
			"<glyphgroup name=\"space\" value=\"7\"/>",
		]
	);
	let mut table2 = Tsi5::default();
	common::from_xml(&mut table2, &xml.join("\n"), &mut font)?;
	assert_eq!(table2, table);
	assert_eq!(table2.compile(&mut font)?, hex("0001 0000 0007"));
	Ok(())
}
