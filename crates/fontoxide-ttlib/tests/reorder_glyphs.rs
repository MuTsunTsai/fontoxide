//! `Tests/ttLib/reorderGlyphs_test.py`.
//!
//! The fonts that the tests build with FontBuilder and feaLib are in
//! `tests/data/reorder_glyphs`, made with the same code by
//! `tools/reference/reorder_scale_fixtures.py`. The tests with a `lazy` parameter only
//! have the fully loaded case, as nothing is lazy here.

use std::path::PathBuf;

use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::reorder_glyphs::reorder_glyphs;
use fontoxide_ttlib::tables::{Cff, Gpos, Gsub};
use fontoxide_ttlib::{Font, ReadOptions, WriteOptions};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn upstream(relative: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttLib/data")
		.join(relative)
}

fn read(relative: &str) -> TestResult<Font> {
	Ok(Font::read(
		std::fs::read(upstream(relative))?,
		ReadOptions::default(),
	)?)
}

/// A font of `tests/data/reorder_glyphs`, through binary.
fn fixture(name: &str) -> TestResult<Font> {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data/reorder_glyphs")
		.join(format!("{name}.ttx"));
	let mut font = Font::from_ttx(&std::fs::read_to_string(path)?)?;
	save_and_read(&mut font)
}

fn save_and_read(font: &mut Font) -> TestResult<Font> {
	Ok(Font::read(
		font.write(&WriteOptions::default())?,
		ReadOptions::default(),
	)?)
}

/// `order[:1] + list(reversed(order[1:]))`.
fn reversed_but_first(font: &mut Font) -> TestResult<Vec<String>> {
	let order = font.glyph_order()?.names().to_vec();
	Ok(order[..1]
		.iter()
		.chain(order[1..].iter().rev())
		.cloned()
		.collect())
}

fn names(font: &mut Font, gids: &[u32]) -> TestResult<Vec<String>> {
	let order = font.glyph_order()?;
	Ok(gids.iter().map(|&g| order.name(g).into_owned()).collect())
}

fn first_subtable(lookups: Option<&ot::LookupList>) -> ot::LookupSubTable {
	lookups
		.and_then(|l| l.lookup.first())
		.and_then(|l| l.as_deref())
		.and_then(|l| l.sub_table.first())
		.and_then(|s| s.as_deref())
		.cloned()
		.expect("a first subtable")
}

fn gsub_subtable(font: &mut Font) -> TestResult<ot::LookupSubTable> {
	Ok(first_subtable(
		font.require::<Gsub>()?.table.lookup_list.as_deref(),
	))
}

fn gpos_subtable(font: &mut Font) -> TestResult<ot::LookupSubTable> {
	Ok(first_subtable(
		font.require::<Gpos>()?.table.lookup_list.as_deref(),
	))
}

#[test]
fn test_reorder_lazy_ligatures() -> TestResult {
	let mut font = fixture("ligatures")?;
	let order = reversed_but_first(&mut font)?;
	reorder_glyphs(&mut font, order)?;
	let mut font = save_and_read(&mut font)?;
	let ot::LookupSubTable::LigatureSubst(subtable) = gsub_subtable(&mut font)? else {
		panic!("not a LigatureSubst");
	};
	let a = font.glyph_id("a")?;
	let ligature = &subtable.ligatures[&a][0];
	assert_eq!(names(&mut font, &ligature.component)?, ["b"]);
	assert_eq!(font.glyph_name(ligature.lig_glyph)?, "a_b");
	Ok(())
}

/// `GSUB.LookupList.Lookup[0].SubTable[0].Coverage[0].glyphs` and
/// `GPOS.LookupList.Lookup[0].SubTable[0].Coverage.glyphs` of `Test-Regular.ttf`.
fn coverages(font: &mut Font) -> TestResult<(Vec<String>, Vec<String>)> {
	let ot::LookupSubTable::ContextSubst(ot::ContextSubst::Format3(gsub)) = gsub_subtable(font)?
	else {
		panic!("not a ContextSubst format 3");
	};
	let ot::LookupSubTable::PairPos(ot::PairPos::Format2(gpos)) = gpos_subtable(font)? else {
		panic!("not a PairPos format 2");
	};
	let coverage1 = gsub.coverage[0]
		.as_ref()
		.expect("a coverage")
		.glyphs
		.clone();
	let coverage2 = gpos.coverage.expect("a coverage").glyphs;
	Ok((names(font, &coverage1)?, names(font, &coverage2)?))
}

fn reversed(v: &[String]) -> Vec<String> {
	v.iter().rev().cloned().collect()
}

#[test]
fn test_reorder_glyphs() -> TestResult {
	let mut font = read("Test-Regular.ttf")?;
	let (old_coverage1, old_coverage2) = coverages(&mut font)?;

	let ga = reversed_but_first(&mut font)?;
	reorder_glyphs(&mut font, ga)?;

	let (new_coverage1, new_coverage2) = coverages(&mut font)?;
	assert_eq!(reversed(&old_coverage1), new_coverage1);
	assert_eq!(reversed(&old_coverage2), new_coverage2);
	Ok(())
}

#[test]
fn test_ttfont_reorder_glyphs() -> TestResult {
	let mut font = read("Test-Regular.ttf")?;
	let ga = reversed_but_first(&mut font)?;
	let (old_coverage1, old_coverage2) = coverages(&mut font)?;

	font.reorder_glyphs(ga)?;

	let (new_coverage1, new_coverage2) = coverages(&mut font)?;
	assert_eq!(reversed(&old_coverage1), new_coverage1);
	assert_eq!(reversed(&old_coverage2), new_coverage2);
	Ok(())
}

#[test]
fn test_reorder_glyphs_cff() -> TestResult {
	let mut font = read("TestVGID-Regular.otf")?;
	let ga: Vec<String> = font.glyph_order()?.names().iter().rev().cloned().collect();
	reorder_glyphs(&mut font, ga.clone())?;

	let top = &font.require::<Cff>()?.cff.top_dicts[0];
	assert_eq!(top.char_strings.as_ref().expect("CharStrings").names, ga);
	assert_eq!(top.charset.as_ref(), Some(&ga));

	// The charstrings follow their glyphs (with `.notdef` first, which compiling
	// requires).
	let mut original = read("TestVGID-Regular.otf")?;
	let mut font = read("TestVGID-Regular.otf")?;
	let ga = reversed_but_first(&mut font)?;
	reorder_glyphs(&mut font, ga.clone())?;
	let mut reordered = save_and_read(&mut font)?;
	let mut a = original.require::<Cff>()?.cff.clone();
	let mut b = reordered.require::<Cff>()?.cff.clone();
	for name in &ga {
		let i = a.glyph_index(name).expect("a glyph");
		let j = b.glyph_index(name).expect("a glyph");
		assert_eq!(a.calc_bounds(i)?, b.calc_bounds(j)?, "{name}");
	}
	Ok(())
}

#[test]
fn test_reorder_glyphs_bad_length() -> TestResult {
	let mut font = read("Test-Regular.ttf")?;
	let err = reorder_glyphs(&mut font, vec!["A".into()]).expect_err("a bad length");
	assert!(
		err.to_string()
			.contains("New glyph order contains 1 glyphs"),
		"{err}"
	);
	Ok(())
}

#[test]
fn test_reorder_glyphs_bad_set() -> TestResult {
	let mut font = read("Test-Regular.ttf")?;
	let mut ga = font.glyph_order()?.names().to_vec();
	ga.push("AA".into());
	let removed = ga.remove(1);
	let err = reorder_glyphs(&mut font, ga)
		.expect_err("a bad set")
		.to_string();
	assert!(
		err.contains("New glyph order does not contain the same set of glyphs"),
		"{err}"
	);
	assert!(err.contains("* only in new: {'AA'}"), "{err}");
	assert!(
		err.contains(&format!("* only in old: {{'{removed}'}}")),
		"{err}"
	);
	Ok(())
}

fn test_reorder_lazy_arrays(pairpos: bool) -> TestResult {
	let mut font = fixture(if pairpos { "pair_pos" } else { "single_pos" })?;
	let order = font.glyph_order()?.names().to_vec();
	let new_order = reversed_but_first(&mut font)?;
	reorder_glyphs(&mut font, new_order)?;
	let mut font = save_and_read(&mut font)?;
	let subtable = gpos_subtable(&mut font)?;
	let reversed_glyphs: Vec<String> = order[1..].iter().rev().cloned().collect();
	if pairpos {
		let ot::LookupSubTable::PairPos(ot::PairPos::Format1(subtable)) = subtable else {
			panic!("not a PairPos format 1");
		};
		let coverage = subtable.coverage.as_ref().expect("a coverage");
		assert_eq!(names(&mut font, &coverage.glyphs)?, ["g0"]);
		let pairs = &subtable.pair_set[0]
			.as_ref()
			.expect("a PairSet")
			.pair_value_record;
		let second: Vec<u32> = pairs.iter().map(|r| r.second_glyph).collect();
		assert_eq!(names(&mut font, &second)?, reversed_glyphs);
		let mut kerning = Vec::new();
		for (&first, pair_set) in coverage.glyphs.iter().zip(&subtable.pair_set) {
			for record in &pair_set.as_ref().expect("a PairSet").pair_value_record {
				kerning.push((
					font.glyph_name(first)?,
					font.glyph_name(record.second_glyph)?,
					record.value1.as_ref().and_then(|v| v.x_advance),
				));
			}
		}
		kerning.sort();
		let mut expected: Vec<_> = (0..10)
			.map(|i| ("g0".to_owned(), format!("g{i}"), Some(i as i16 + 1)))
			.collect();
		expected.sort();
		assert_eq!(kerning, expected);
	} else {
		let ot::LookupSubTable::SinglePos(ot::SinglePos::Format2(subtable)) = subtable else {
			panic!("not a SinglePos format 2");
		};
		let coverage = subtable.coverage.as_ref().expect("a coverage");
		assert_eq!(names(&mut font, &coverage.glyphs)?, reversed_glyphs);
		let advances: Vec<Option<i16>> = subtable
			.value
			.iter()
			.map(|v| v.as_ref().and_then(|v| v.x_advance))
			.collect();
		let expected: Vec<Option<i16>> = (1..=10).rev().map(Some).collect();
		assert_eq!(advances, expected);
	}
	Ok(())
}

#[test]
fn test_reorder_lazy_arrays_single_pos() -> TestResult {
	test_reorder_lazy_arrays(false)
}

#[test]
fn test_reorder_lazy_arrays_pair_pos() -> TestResult {
	test_reorder_lazy_arrays(true)
}
