//! `Tests/ttLib/tables/_l_t_a_g_test.py`.

mod common;

use common::{TestResult, from_xml, get_xml};
use fontoxide_ttlib::tables::Ltag;
use fontoxide_ttlib::{Font, Table, TableType};

/// `struct.pack(">LLLHHHHHH", 1, 0, 3, 24 + 0, 2, 24 + 2, 7, 24 + 2, 2) + b"enzh-Hant"`.
fn data() -> Vec<u8> {
	let mut data = Vec::new();
	for v in [1u32, 0, 3] {
		data.extend(v.to_be_bytes());
	}
	for v in [24u16, 2, 26, 7, 26, 2] {
		data.extend(v.to_be_bytes());
	}
	data.extend(b"enzh-Hant");
	data
}

const TAGS: [&str; 3] = ["en", "zh-Hant", "zh"];

#[test]
fn test_add_tag() {
	let mut table = Ltag::default();
	assert_eq!(table.add_tag("de-CH"), 0);
	assert_eq!(table.add_tag("gsw-LI"), 1);
	assert_eq!(table.add_tag("de-CH"), 0);
	assert_eq!(table.tags, ["de-CH", "gsw-LI"]);
}

#[test]
fn test_decompile_compile() -> TestResult {
	let mut font = Font::new();
	let mut table = Ltag::decompile(&data(), &mut font)?;
	assert_eq!(table.version, 1);
	assert_eq!(table.flags, 0);
	assert_eq!(table.tags, TAGS);
	assert_eq!(table.compile(&mut font)?, data());
	Ok(())
}

#[test]
fn test_from_xml() -> TestResult {
	let mut table = Ltag::default();
	from_xml(
		&mut table,
		"<version value=\"1\"/><flags value=\"777\"/><LanguageTag tag=\"sr-Latn\"/><LanguageTag tag=\"fa\"/>",
		&mut Font::new(),
	)?;
	assert_eq!(table.version, 1);
	assert_eq!(table.flags, 777);
	assert_eq!(table.tags, ["sr-Latn", "fa"]);
	Ok(())
}

#[test]
fn test_to_xml() -> TestResult {
	let mut font = Font::new();
	let table = Ltag::decompile(&data(), &mut font)?;
	assert_eq!(
		get_xml(&table, &mut font)?,
		[
			"<version value=\"1\"/>",
			"<flags value=\"0\"/>",
			"<LanguageTag tag=\"en\"/>",
			"<LanguageTag tag=\"zh-Hant\"/>",
			"<LanguageTag tag=\"zh\"/>",
		]
	);
	Ok(())
}
