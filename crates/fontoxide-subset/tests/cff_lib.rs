//! `Tests/cffLib/cffLib_test.py`: `test_VARC_short_CharStrings_roundtrip` in
//! full. The other tests of that file, and this one without its final
//! subsetting step, are in `fontoxide-ttlib/tests/cff_lib.rs`; this one needs
//! the subsetter.

#![allow(non_snake_case)]

mod common;

use common::*;
use fontoxide_pens::BoundsPen;
use fontoxide_subset::{Options, Subsetter};
use fontoxide_ttlib::cff::font::TopDict;
use fontoxide_ttlib::tables::hhea::{Hhea, Vhea};
use fontoxide_ttlib::tables::otl::Varc;
use fontoxide_ttlib::tables::{Cff2, Head};
use fontoxide_ttlib::{Font, GlyphSet, TtxOptions};

fn top_cff2(font: &mut Font) -> fontoxide_ttlib::Result<&mut TopDict> {
	let cff2 = font.require::<Cff2>()?;
	cff2.cff
		.top_dicts
		.first_mut()
		.ok_or_else(|| fontoxide_ttlib::Error::ttlib("no Top DICT"))
}

/// `check` of `_check_VARC_short_CharStrings_roundtrip`.
fn check(font: &mut Font) -> TestResult {
	let order = font.glyph_order()?.names().to_vec();
	{
		let top = top_cff2(font)?;
		let cs = top.char_strings.as_ref().ok_or("no CharStrings")?;
		assert!(cs.names.len() < order.len());
		assert_eq!(top.charset.as_deref(), Some(&order[..cs.names.len()]));
		let fd_select = top.fd_select.as_ref().ok_or("no FDSelect")?;
		assert_eq!(fd_select.gid_array.len(), order.len());
		assert_eq!(fd_select.gid_array.last(), Some(&Some(1)));
	}
	let composite = {
		let gid = font
			.require::<Varc>()?
			.coverage
			.as_ref()
			.ok_or("no Coverage")?
			.glyphs[0];
		font.glyph_name(gid)?
	};
	{
		let glyph_set = GlyphSet::new(font)?;
		assert_eq!(glyph_set.keys(), order);
		assert!(glyph_set.contains(&composite));
		let mut pen = BoundsPen::new_bounds(Some(&glyph_set));
		glyph_set.draw(&composite, &mut pen)?;
		assert_eq!(pen.bounds(), Some([600.0, 0.0, 800.0, 200.0]));
		assert_eq!(glyph_set.metrics(&composite)?.width, 500.0);
	}
	assert_eq!(font.require::<Head>()?.x_max, 800);
	assert_eq!(font.require::<Head>()?.y_max, 200);
	assert_eq!(font.require::<Hhea>()?.x_max_extent, 200);
	assert_eq!(font.require::<Vhea>()?.y_max_extent, 200);
	Ok(())
}

/// `_check_VARC_short_CharStrings_roundtrip`.
fn check_varc_short_char_strings_roundtrip(format: i64, retain_gids: bool) -> TestResult {
	let mut font = open(tests_path("cffLib/data/varc-short-cff2.otf"))?;
	top_cff2(&mut font)?
		.fd_select
		.as_mut()
		.ok_or("no FDSelect")?
		.format = Some(format);

	for _ in 0..2 {
		check(&mut font)?;
		font = save_and_read(&mut font)?;
	}
	let xml = font.to_ttx(&TtxOptions::default())?;
	// `TTFont().importXML(xml)`.
	let mut font = Font::new();
	font.import_ttx(&xml)?;
	let mut font = save_and_read(&mut font)?;
	check(&mut font)?;

	let options = Options {
		retain_gids,
		// Preserve both FDs: one is used only by the VARC-only glyph.
		hinting: true,
		..Options::default()
	};
	let mut sub = Subsetter::new(options);
	sub.populate(&["composite"], &[], &[], "");
	sub.subset(&mut font)?;
	let mut font = save_and_read(&mut font)?;
	check(&mut font)?;
	Ok(())
}

#[test]
fn test_VARC_short_CharStrings_roundtrip() -> TestResult {
	for format in [3, 4] {
		for retain_gids in [false, true] {
			check_varc_short_char_strings_roundtrip(format, retain_gids)
				.map_err(|e| format!("format={format}, retainGids={retain_gids}: {e}"))?;
		}
	}
	Ok(())
}
