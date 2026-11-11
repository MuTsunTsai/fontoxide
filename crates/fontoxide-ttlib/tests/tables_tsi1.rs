//! `Tests/ttLib/tables/T_S_I__1_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;

use common::{TestResult, capture_logs, count_logs, names};
use fontoxide_ttlib::tables::{TsiIndex, TsiPrograms};
use fontoxide_ttlib::{Font, Result};

const TSI1_DATA: &[u8] = b"abcdefghijklmnopqrstuvxywz0123456789";
const TSI1_UTF8_DATA: &[u8] = b"abcd\xc3\xa9ghijklmnopqrstuvxywz0123456789";

/// The `indextable` fixture.
fn indextable() -> TsiIndex {
	let mut table = TsiIndex::default();
	table.set(
		vec![
			(0, 1, 0), // gid 0, length=1, offset=0, text='a'
			(1, 5, 1), // gid 1, length=5, offset=1, text='bcdef'
			(2, 0, 1), // gid 2, length=0, offset=1, text=''
			(3, 0, 1), // gid 3, length=0, offset=1, text=''
			(4, 8, 6), // gid 4, length=8, offset=6, text='ghijklmn'
		],
		vec![
			(0xFFFA, 2, 14),  // 'ppgm', length=2, offset=14, text='op'
			(0xFFFB, 4, 16),  // 'cvt', length=4, offset=16, text='qrst'
			(0xFFFC, 6, 20),  // 'reserved', length=6, offset=20, text='uvxywz'
			(0xFFFD, 10, 26), // 'fpgm', length=10, offset=26, text='0123456789'
		],
	);
	table
}

/// The `font` fixture: glyphs 'a', 'b', 'c', ...
fn font() -> (Font, TsiIndex) {
	let mut font = Font::new();
	font.set_glyph_order(names(&["a", "b", "c", "d", "e"]));
	(font, indextable())
}

/// The `empty_font` fixture.
fn empty_font() -> (Font, TsiIndex) {
	let mut font = Font::new();
	font.set_glyph_order(Vec::new());
	let mut index = TsiIndex::default();
	index.set(
		Vec::new(),
		vec![
			(0xFFFA, 0, 0),
			(0xFFFB, 0, 0),
			(0xFFFC, 0, 0),
			(0xFFFD, 0, 0),
		],
	);
	(font, index)
}

fn decompile(data: &[u8], font: &mut Font, index: &TsiIndex) -> Result<TsiPrograms> {
	let mut table = TsiPrograms::default();
	table.decompile_with_index(data, index, font)?;
	Ok(table)
}

fn glyph_programs(table: &TsiPrograms, font: &mut Font) -> BTreeMap<String, String> {
	table
		.glyph_programs
		.iter()
		.map(|(&gid, text)| (font.glyph_name(gid).unwrap(), text.clone()))
		.collect()
}

fn map(items: &[(&str, &str)]) -> BTreeMap<String, String> {
	items
		.iter()
		.map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
		.collect()
}

fn standard_extras() -> BTreeMap<String, String> {
	map(&[
		("ppgm", "op"),
		("cvt", "qrst"),
		("reserved", "uvxywz"),
		("fpgm", "0123456789"),
	])
}

#[test]
fn test_decompile() -> TestResult {
	let (mut font, index) = font();
	let table = decompile(TSI1_DATA, &mut font, &index)?;
	// Zero-length entries are skipped.
	assert_eq!(
		glyph_programs(&table, &mut font),
		map(&[("a", "a"), ("b", "bcdef"), ("e", "ghijklmn")])
	);
	assert_eq!(table.extra_programs, standard_extras());
	Ok(())
}

#[test]
fn test_decompile_utf8() -> TestResult {
	let (mut font, index) = font();
	let table = decompile(TSI1_UTF8_DATA, &mut font, &index)?;
	assert_eq!(
		glyph_programs(&table, &mut font),
		map(&[("a", "a"), ("b", "bcd\u{e9}"), ("e", "ghijklmn")])
	);
	assert_eq!(table.extra_programs, standard_extras());
	Ok(())
}

#[test]
fn test_decompile_empty() -> TestResult {
	let (mut font, index) = empty_font();
	let table = decompile(b"", &mut font, &index)?;
	assert!(table.glyph_programs.is_empty());
	assert!(table.extra_programs.is_empty());
	Ok(())
}

#[test]
fn test_decompile_invalid_length() {
	let (mut font, mut index) = empty_font();
	font.set_glyph_order(names(&["a"]));
	index.indices = Some(vec![(0, 0x8000 + 1, 0)]);
	let error = decompile(b"", &mut font, &index).unwrap_err();
	assert!(
		error
			.to_string()
			.contains("textLength (32769) must not be > 32768"),
		"{error}"
	);
}

#[test]
fn test_decompile_offset_past_end() -> TestResult {
	let (mut font, mut index) = empty_font();
	font.set_glyph_order(names(&["foo", "bar"]));
	let data = b"baz";
	index.indices = Some(vec![(0, 3, 0), (1, 1, 4)]);

	let (logs, table) = capture_logs(|| decompile(data, &mut font, &index));
	let table = table?;
	// The 'bar' program is skipped because its offset > len(data).
	assert_eq!(glyph_programs(&table, &mut font), map(&[("foo", "baz")]));
	assert_eq!(
		count_logs(&logs, log::Level::Warn, "textOffset > totalLength"),
		1
	);
	Ok(())
}

#[test]
fn test_decompile_magic_length_last_extra() -> TestResult {
	let (mut font, mut index) = empty_font();
	index.extra_indices[3] = (0xFFFD, 0x8000, 0);
	let content = "0".repeat(0x8000 + 1);
	let table = decompile(content.as_bytes(), &mut font, &index)?;
	assert_eq!(table.extra_programs["fpgm"], content);
	Ok(())
}

#[test]
fn test_decompile_magic_length_last_glyph() -> TestResult {
	let (mut font, mut index) = empty_font();
	font.set_glyph_order(names(&["foo", "bar"]));
	// The actual length of the 'bar' program is the difference between the first
	// extra's offset and the 'bar' offset: 0x8004 - 3.
	index.indices = Some(vec![(0, 3, 0), (1, 0x8000, 3)]);
	index.extra_indices = vec![
		(0xFFFA, 0, 0x8004),
		(0xFFFB, 0, 0x8004),
		(0xFFFC, 0, 0x8004),
		(0xFFFD, 0, 0x8004),
	];
	let foo_content = "0".repeat(3);
	let bar_content = "1".repeat(0x8000 + 1);
	let data = format!("{foo_content}{bar_content}");
	let table = decompile(data.as_bytes(), &mut font, &index)?;
	assert_eq!(table.glyph_programs[&0], foo_content);
	assert_eq!(table.glyph_programs[&1], bar_content);
	Ok(())
}

#[test]
fn test_decompile_magic_length_non_last() -> TestResult {
	let (mut font, mut index) = empty_font();
	// The actual length of the 'cvt' program is nextTextOffset - textOffset:
	// 0x8004 - 3.
	index.extra_indices = vec![
		(0xFFFA, 3, 0),
		(0xFFFB, 0x8000, 3),
		(0xFFFC, 0, 0x8004),
		(0xFFFD, 0, 0x8004),
	];
	let ppgm_content = "0".repeat(3);
	let cvt_content = "1".repeat(0x8000 + 1);
	let data = format!("{ppgm_content}{cvt_content}");
	let table = decompile(data.as_bytes(), &mut font, &index)?;
	assert_eq!(table.extra_programs["ppgm"], ppgm_content);
	assert_eq!(table.extra_programs["cvt"], cvt_content);

	// The last entry is truncated.
	let bytes = data.as_bytes();
	let (logs, table) = capture_logs(|| decompile(&bytes[..bytes.len() - 1], &mut font, &index));
	let table = table?;
	assert_eq!(
		count_logs(&logs, log::Level::Warn, "nextTextOffset > totalLength"),
		1
	);
	assert_eq!(
		table.extra_programs["cvt"],
		cvt_content[..cvt_content.len() - 1]
	);
	Ok(())
}

/// Compiling `TSI1` updates `TSI0`, and a binary and TTX round trip through
/// the font keeps the programs; the TTX matches fontTools (without `post`,
/// the glyph names are synthesized).
#[test]
fn font_round_trip() -> TestResult {
	use fontoxide_ttlib::{ReadOptions, TableKey, Tag, TtxOptions, WriteOptions};
	let mut font = Font::new();
	font.set_glyph_order(names(&[".notdef", "A", "B"]));
	font.insert(Box::new(fontoxide_ttlib::tables::Maxp {
		table_version: 0x5000,
		num_glyphs: 3,
		..Default::default()
	}));
	font.insert(Box::new(TsiIndex::default()));
	let mut programs = TsiPrograms::default();
	programs.glyph_programs.insert(1, "SVTCA[0]\rRTG[]".into());
	programs.glyph_programs.insert(2, "abc".into());
	programs
		.extra_programs
		.insert("fpgm".into(), "/* fpgm */".into());
	font.insert(Box::new(programs));
	let data = font.write(&WriteOptions::default())?;
	assert_eq!(
		font.table_data(Tag::new(b"TSI0"))?,
		common::hex(concat!(
			"0000 0000 00000000",
			"0001 000E 00000000",
			"0002 0003 0000000E",
			"FFFE 0000 ABFC1F34",
			"FFFA 0000 00000012",
			"FFFB 0000 00000012",
			"FFFC 0000 00000012",
			"FFFD 000A 00000012",
		))
	);

	let mut font = Font::read(data, ReadOptions::default())?;
	let ttx = font.to_ttx(&TtxOptions {
		tables: vec![
			TableKey::Table(Tag::new(b"TSI0")),
			TableKey::Table(Tag::new(b"TSI1")),
		],
		..Default::default()
	})?;
	let body: Vec<&str> = ttx.lines().skip(3).collect();
	assert_eq!(
		body,
		[
			"  <TSI0>",
			"    <!-- This table will be calculated by the compiler -->",
			"  </TSI0>",
			"",
			"  <TSI1>",
			"",
			"    <glyphProgram name=\"glyph00001\">",
			"SVTCA[0]",
			"RTG[]",
			"    </glyphProgram>",
			"",
			"    <glyphProgram name=\"glyph00002\">",
			"abc",
			"    </glyphProgram>",
			"",
			"    <extraProgram name=\"fpgm\">",
			"/* fpgm */",
			"    </extraProgram>",
			"",
			"  </TSI1>",
			"",
			"</ttFont>",
		]
	);
	let mut font2 = Font::new();
	font2.set_glyph_order(names(&[".notdef", "A", "B"]));
	font2.import_ttx(&ttx)?;
	let programs = font2.require::<TsiPrograms>()?;
	assert_eq!(programs.glyph_programs[&1], "SVTCA[0]\rRTG[]");
	Ok(())
}
