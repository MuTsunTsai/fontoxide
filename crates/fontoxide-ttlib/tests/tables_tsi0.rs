//! `Tests/ttLib/tables/T_S_I__0_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::{TestResult, get_xml, hex};
use fontoxide_ttlib::tables::Maxp;
use fontoxide_ttlib::tables::TsiIndex;
use fontoxide_ttlib::tables::tsi::TsiEntry;
use fontoxide_ttlib::{Font, Table, TableType};

/// (gid, length, offset) for glyph programs.
const TSI0_INDICES: [TsiEntry; 5] = [(0, 1, 0), (1, 5, 1), (2, 0, 1), (3, 0, 1), (4, 8, 6)];

/// (type, length, offset) for 'extra' programs: ppgm, cvt, reserved, fpgm.
const TSI0_EXTRA_INDICES: [TsiEntry; 4] = [
	(0xFFFA, 2, 14),
	(0xFFFB, 4, 16),
	(0xFFFC, 6, 20),
	(0xFFFD, 10, 26),
];

fn tsi0_data() -> Vec<u8> {
	hex(concat!(
		"0000 0001 00000000",
		"0001 0005 00000001",
		"0002 0000 00000001",
		"0003 0000 00000001",
		"0004 0008 00000006",
		"FFFE 0000 ABFC1F34", // 'magic' separates glyph from extra programs
		"FFFA 0002 0000000E",
		"FFFB 0004 00000010",
		"FFFC 0006 00000014",
		"FFFD 000A 0000001A",
	))
}

/// An empty font has no glyph programs, but the 4 extra programs are always
/// present.
const EMPTY_TSI0_EXTRA_INDICES: [TsiEntry; 4] = [
	(0xFFFA, 0, 0),
	(0xFFFB, 0, 0),
	(0xFFFC, 0, 0),
	(0xFFFD, 0, 0),
];

fn empty_tsi0_data() -> Vec<u8> {
	hex(concat!(
		"FFFE 0000 ABFC1F34",
		"FFFA 0000 00000000",
		"FFFB 0000 00000000",
		"FFFC 0000 00000000",
		"FFFD 0000 00000000",
	))
}

/// `{"maxp": SimpleNamespace(numGlyphs=numGlyphs)}`.
fn font_with_glyphs(num_glyphs: u16) -> Font {
	let mut font = Font::new();
	font.insert(Box::new(Maxp {
		num_glyphs: num_glyphs.into(),
		..Default::default()
	}));
	font
}

#[test]
fn test_decompile() -> TestResult {
	for (num_glyphs, data, indices, extra) in [
		(5, tsi0_data(), TSI0_INDICES.to_vec(), TSI0_EXTRA_INDICES),
		(0, empty_tsi0_data(), Vec::new(), EMPTY_TSI0_EXTRA_INDICES),
	] {
		let mut font = font_with_glyphs(num_glyphs);
		let table = TsiIndex::decompile(&data, &mut font)?;
		let decompiled = table.indices.as_ref().unwrap();
		assert_eq!(decompiled.len(), num_glyphs as usize);
		assert_eq!(*decompiled, indices);
		assert_eq!(table.extra_indices.len(), 4);
		assert_eq!(table.extra_indices, extra);
	}
	Ok(())
}

#[test]
fn test_decompile_glyphs_mismatch() -> TestResult {
	// More entries than glyphs, and fewer: only a warning.
	for num_glyphs in [4, 6] {
		let mut font = font_with_glyphs(num_glyphs);
		let (logs, table) = common::capture_logs(|| TsiIndex::decompile(&tsi0_data(), &mut font));
		let table = table?;
		assert_eq!(
			common::count_logs(&logs, log::Level::Warn, "Number of glyphPrograms differs"),
			1
		);
		assert_eq!(table.indices.as_ref().unwrap().len(), 5);
		assert_eq!(*table.indices.as_ref().unwrap(), TSI0_INDICES);
		assert_eq!(table.extra_indices.len(), 4);
		assert_eq!(table.extra_indices, TSI0_EXTRA_INDICES);
	}
	Ok(())
}

#[test]
fn test_compile() -> TestResult {
	for (indices, extra, expected) in [
		(TSI0_INDICES.to_vec(), TSI0_EXTRA_INDICES, tsi0_data()),
		(Vec::new(), EMPTY_TSI0_EXTRA_INDICES, empty_tsi0_data()),
	] {
		let mut font = Font::new();
		let mut table = TsiIndex::default();
		assert_eq!(table.compile(&mut font)?, b"");

		table.set(indices, extra.to_vec());
		assert_eq!(table.compile(&mut font)?, expected);
	}
	Ok(())
}

#[test]
fn test_set() {
	let mut table = TsiIndex::default();
	table.set(TSI0_INDICES.to_vec(), TSI0_EXTRA_INDICES.to_vec());
	assert_eq!(table.indices.as_deref(), Some(&TSI0_INDICES[..]));
	assert_eq!(table.extra_indices, TSI0_EXTRA_INDICES);
}

#[test]
fn test_to_xml() -> TestResult {
	let table = TsiIndex::default();
	assert_eq!(
		get_xml(&table, &mut Font::new())?,
		["<!-- This table will be calculated by the compiler -->"]
	);
	Ok(())
}
