//! `Tests/ttLib/tables/C_F_F_test.py` and `Tests/ttLib/tables/C_F_F__2_test.py`.

mod common;

use common::{TestResult, tables_data};
use fontoxide_ttlib::tables::{Cff, Cff2};
use fontoxide_ttlib::{Font, ReadOptions, Table, TableType, Tag, TtxOptions};

/// `strip_ttLibVersion` / `strip_VariableItems`, then `splitlines`.
fn strip_variable_items(text: &str) -> Vec<String> {
	let mut out = Vec::new();
	for line in text.lines() {
		let mut line = line.to_owned();
		if let Some(start) = line.find(" ttLibVersion=\"") {
			// `.*` is greedy: everything up to the last quote of the line.
			if let Some(end) = line.rfind('"')
				&& end > start + 15
			{
				line.replace_range(start..=end, "");
			}
		}
		for prefix in ["<checkSumAdjustment value=\"", "<modified value=\""] {
			if let Some(start) = line.find(prefix)
				&& let Some(len) = line[start + prefix.len()..].find("\"/>")
			{
				line.replace_range(start..start + prefix.len() + len + 3, "");
			}
		}
		out.push(line);
	}
	out
}

/// `CFFTableTest::test_toXML` of `C_F_F_test.py`.
#[test]
fn cff_test_to_xml() -> TestResult {
	let data = std::fs::read(tables_data("C_F_F_.bin"))?;
	let expected = std::fs::read_to_string(tables_data("C_F_F_.ttx"))?;
	let mut font = Font::new();
	font.sfnt_version = Tag::new(b"OTTO");
	let cff = Cff::decompile(&data, &mut font)?;
	font.insert(Box::new(cff));
	let ttx = font.to_ttx(&TtxOptions::default())?;
	assert_eq!(strip_variable_items(&ttx), strip_variable_items(&expected));
	Ok(())
}

/// `CFFTableTest::test_fromXML` of `C_F_F_test.py`.
#[test]
fn cff_test_from_xml() -> TestResult {
	let data = std::fs::read(tables_data("C_F_F_.bin"))?;
	let mut font = Font::new();
	font.sfnt_version = Tag::new(b"OTTO");
	font.import_ttx(&std::fs::read_to_string(tables_data("C_F_F_.ttx"))?)?;
	let mut cff = font.require::<Cff>()?.clone();
	assert_eq!(cff.compile(&mut font)?, data);
	Ok(())
}

/// `TTFont(file=CFF_BIN)` and `font["CFF2"].compile(font)`.
fn cff2_font() -> fontoxide_ttlib::Result<(Font, Vec<u8>)> {
	let mut font = Font::read(
		std::fs::read(tables_data("C_F_F__2.bin"))?,
		ReadOptions::default(),
	)?;
	let mut cff2 = font.require::<Cff2>()?.clone();
	let data = cff2.compile(&mut font)?;
	Ok((font, data))
}

/// `CFFTableTest::test_toXML` of `C_F_F__2_test.py`.
#[test]
fn cff2_test_to_xml() -> TestResult {
	let expected = std::fs::read_to_string(tables_data("C_F_F__2.ttx"))?;
	let (mut font, _) = cff2_font()?;
	let ttx = font.to_ttx(&TtxOptions::default())?;
	assert_eq!(strip_variable_items(&ttx), strip_variable_items(&expected));
	Ok(())
}

/// `CFFTableTest::test_fromXML` of `C_F_F__2_test.py`.
#[test]
fn cff2_test_from_xml() -> TestResult {
	let (_, expected) = cff2_font()?;
	let mut font = Font::new();
	font.sfnt_version = Tag::new(b"OTTO");
	font.import_ttx(&std::fs::read_to_string(tables_data("C_F_F__2.ttx"))?)?;
	let mut cff2 = font.require::<Cff2>()?.clone();
	assert_eq!(cff2.compile(&mut font)?, expected);
	Ok(())
}
