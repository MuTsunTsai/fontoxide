//! `Tests/ttLib/tables/_m_e_t_a_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;

use common::{TestResult, from_xml, get_xml, hex};
use fontoxide_ttlib::tables::Meta;
use fontoxide_ttlib::tables::meta::MetaValue;
use fontoxide_ttlib::{Font, Table, TableType};

/// From a real font on MacOS X, but with the 'bild' tag substituted by 'TEST',
/// and a shortened payload.
fn meta_data() -> Vec<u8> {
	hex(concat!(
		"00 00 00 01 00 00 00 00 00 00 00 1C 00 00 00 01 ",
		"54 45 53 54 00 00 00 1C 00 00 00 04 CA FE BE EF"
	))
}

/// The 'dlng' and 'slng' tags with UTF-8 text.
fn meta_data_text() -> Vec<u8> {
	hex(concat!(
		"00 00 00 01 00 00 00 00 00 00 00 28 00 00 00 02 ",
		"64 6C 6E 67 00 00 00 28 00 00 00 0E 73 6C 6E 67 ",
		"00 00 00 36 00 00 00 0E 4C 61 74 6E 2C 47 72 65 ",
		"6B 2C 43 79 72 6C 4C 61 74 6E 2C 47 72 65 6B 2C ",
		"43 79 72 6C"
	))
}

fn data(items: &[(&str, MetaValue)]) -> BTreeMap<String, MetaValue> {
	items
		.iter()
		.map(|(k, v)| ((*k).to_owned(), v.clone()))
		.collect()
}

fn binary(b: &[u8]) -> MetaValue {
	MetaValue::Binary(b.to_vec())
}

fn text(s: &str) -> MetaValue {
	MetaValue::Text(s.to_owned())
}

fn stripped(table: &Meta) -> Vec<String> {
	get_xml(table, &mut Font::new())
		.unwrap()
		.iter()
		.map(|l| l.trim().to_owned())
		.collect()
}

#[test]
fn test_decompile() -> TestResult {
	let table = Meta::decompile(&meta_data(), &mut Font::new())?;
	assert_eq!(table.data, data(&[("TEST", binary(b"\xca\xfe\xbe\xef"))]));
	Ok(())
}

#[test]
fn test_compile() -> TestResult {
	let mut table = Meta::default();
	table
		.data
		.insert("TEST".into(), binary(b"\xca\xfe\xbe\xef"));
	assert_eq!(table.compile(&mut Font::new())?, meta_data());
	Ok(())
}

#[test]
fn test_decompile_text() -> TestResult {
	let table = Meta::decompile(&meta_data_text(), &mut Font::new())?;
	assert_eq!(
		table.data,
		data(&[
			("dlng", text("Latn,Grek,Cyrl")),
			("slng", text("Latn,Grek,Cyrl")),
		])
	);
	Ok(())
}

#[test]
fn test_compile_text() -> TestResult {
	let mut table = Meta::default();
	table.data.insert("dlng".into(), text("Latn,Grek,Cyrl"));
	table.data.insert("slng".into(), text("Latn,Grek,Cyrl"));
	assert_eq!(table.compile(&mut Font::new())?, meta_data_text());
	Ok(())
}

#[test]
fn test_to_xml() {
	let mut table = Meta::default();
	table
		.data
		.insert("TEST".into(), binary(b"\xca\xfe\xbe\xef"));
	assert_eq!(
		stripped(&table),
		["<hexdata tag=\"TEST\">", "cafebeef", "</hexdata>"]
	);
}

#[test]
fn test_to_xml_ascii_data() {
	let mut table = Meta::default();
	table.data.insert("TEST".into(), binary(b"Hello!"));
	assert_eq!(
		stripped(&table),
		[
			"<hexdata tag=\"TEST\">",
			"<!-- ascii: Hello! -->",
			"48656c6c 6f21",
			"</hexdata>",
		]
	);
}

#[test]
fn test_from_xml() -> TestResult {
	let mut table = Meta::default();
	from_xml(
		&mut table,
		"<hexdata tag=\"TEST\">    cafebeef</hexdata>",
		&mut Font::new(),
	)?;
	assert_eq!(table.data, data(&[("TEST", binary(b"\xca\xfe\xbe\xef"))]));
	Ok(())
}

#[test]
fn test_to_xml_text() {
	let mut table = Meta::default();
	table.data.insert("dlng".into(), text("Latn,Grek,Cyrl"));
	assert_eq!(
		stripped(&table),
		["<text tag=\"dlng\">", "Latn,Grek,Cyrl", "</text>"]
	);
}

#[test]
fn test_from_xml_text() -> TestResult {
	let mut table = Meta::default();
	from_xml(
		&mut table,
		"<text tag=\"dlng\">    Latn,Grek,Cyrl</text>",
		&mut Font::new(),
	)?;
	assert_eq!(table.data, data(&[("dlng", text("Latn,Grek,Cyrl"))]));
	Ok(())
}
