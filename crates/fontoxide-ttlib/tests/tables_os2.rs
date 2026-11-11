//! `Tests/ttLib/tables/O_S_2f_2_test.py`, and the doctest of
//! `O_S_2f_2.intersectUnicodeRanges`.

#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeSet;

use common::*;
use fontoxide_ttlib::tables::Os2;
use fontoxide_ttlib::tables::os2::intersect_unicode_ranges;

fn set(bits: &[u32]) -> BTreeSet<u32> {
	bits.iter().copied().collect()
}

/// The code points of the Unicode `cmap` subtable of `makeOS2_cmap(mapping)`,
/// which `recalcUnicodeRanges` and `recalcCodePageRanges` read from the font.
fn unicodes(codes: &[u32]) -> BTreeSet<u32> {
	set(codes)
}

#[test]
// The fields are those of `base`, reached through `DerefMut`.
#[allow(clippy::field_reassign_with_default)]
fn test_get_unicode_ranges() {
	let mut table = Os2::default();
	table.ul_unicode_range1 = 0xFFFFFFFF;
	table.ul_unicode_range2 = 0xFFFFFFFF;
	table.ul_unicode_range3 = 0xFFFFFFFF;
	table.ul_unicode_range4 = 0xFFFFFFFF;
	let bits = table.unicode_ranges();
	for i in 0..127 {
		assert!(bits.contains(&i));
	}
}

#[test]
fn test_set_unicode_ranges() -> TestResult {
	let mut table = Os2::default();
	let bits: BTreeSet<u32> = (0..128).collect();
	table.set_unicode_ranges(bits.iter().copied())?;
	assert_eq!(table.unicode_ranges(), bits);
	// Negative bits cannot be expressed.
	assert!(table.set_unicode_ranges([128, 255]).is_err());
	Ok(())
}

#[test]
fn test_recalc_unicode_ranges() -> TestResult {
	let mut os2 = Os2::default();
	os2.base.version = 4;
	let mut codes = vec![0x0041, 0x03B1, 0x0410];
	os2.set_unicode_ranges([0, 1, 9])?;
	// `pruneOnly` clears the bits without intersection (bit 1, Latin 1
	// Supplement), but does not set bit 7 (Greek and Coptic) for "alpha".
	assert_eq!(
		os2.recalc_unicode_ranges(&unicodes(&codes), true)?,
		set(&[0, 9])
	);
	assert_eq!(
		os2.recalc_unicode_ranges(&unicodes(&codes), false)?,
		set(&[0, 7, 9])
	);
	// A non-BMP character of Mahjong Tiles (bit 122) also sets bit 57 (Non Plane
	// 0).
	codes.push(0x1F000);
	assert_eq!(
		os2.recalc_unicode_ranges(&unicodes(&codes), false)?,
		set(&[0, 7, 9, 57, 122])
	);
	Ok(())
}

/// Also the doctest of `intersectUnicodeRanges`.
#[test]
fn test_intersect_unicode_ranges() {
	assert_eq!(intersect_unicode_ranges(&set(&[0x0410]), false), set(&[9]));
	assert_eq!(
		intersect_unicode_ranges(&set(&[0x0410, 0x1F000]), false),
		set(&[9, 57, 122])
	);
	let all: BTreeSet<u32> = (0..123).collect();
	assert_eq!(
		intersect_unicode_ranges(&set(&[0x0410, 0x1F000]), true),
		all.difference(&set(&[9, 57, 122])).copied().collect()
	);
}

#[test]
fn test_get_code_page_ranges() {
	let mut table = Os2::default();
	// Version 0 has no code page ranges.
	table.base.version = 0;
	assert_eq!(table.code_page_ranges(), BTreeSet::new());
	table.base.version = 1;
	table.ul_code_page_range1 = 0xFFFFFFFF;
	table.ul_code_page_range2 = 0xFFFFFFFF;
	let bits = table.code_page_ranges();
	for i in 0..63 {
		assert!(bits.contains(&i));
	}
}

#[test]
fn test_set_code_page_ranges() -> TestResult {
	let mut table = Os2::default();
	table.base.version = 4;
	let bits: BTreeSet<u32> = (0..64).collect();
	table.set_code_page_ranges(bits.iter().copied())?;
	assert_eq!(table.code_page_ranges(), bits);
	// Negative bits cannot be expressed.
	assert!(table.set_code_page_ranges([64]).is_err());
	assert!(table.set_code_page_ranges([255]).is_err());
	Ok(())
}

#[test]
fn test_set_code_page_ranges_bump_version() -> TestResult {
	// Setting code page ranges makes a version 0 table a version 1 table.
	let mut table = Os2::default();
	table.base.version = 0;
	assert_eq!(table.code_page_ranges(), BTreeSet::new());
	table.set_code_page_ranges([0, 1, 2])?;
	assert_eq!(table.code_page_ranges(), set(&[0, 1, 2]));
	assert_eq!(table.version, 1);
	Ok(())
}

#[test]
fn test_recalc_code_page_ranges() -> TestResult {
	let mut os2 = Os2::default();
	os2.base.version = 4;
	let mut codes = vec!['A' as u32, 'Ά' as u32, 'Б' as u32];
	os2.set_code_page_ranges([0, 2, 9])?;
	// `pruneOnly` clears the code pages without characters in the cmap.
	assert_eq!(
		os2.recalc_code_page_ranges(&unicodes(&codes), true)?,
		set(&[2])
	);
	// Otherwise, code pages are also set.
	assert_eq!(
		os2.recalc_code_page_ranges(&unicodes(&codes), false)?,
		set(&[2, 3])
	);
	// A Korean character sets code page 21 (Korean Johab).
	codes.push('곴' as u32);
	assert_eq!(
		os2.recalc_code_page_ranges(&unicodes(&codes), false)?,
		set(&[2, 3, 21])
	);
	// Without characters, code page 0 (Latin 1) is still set.
	assert_eq!(
		os2.recalc_code_page_ranges(&unicodes(&[]), false)?,
		set(&[0])
	);
	Ok(())
}
