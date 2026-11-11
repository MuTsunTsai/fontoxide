//! `Tests/ttLib/tables/_t_r_a_k_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::{TestResult, from_xml, get_xml, hex};
use fontoxide_ttlib::sstruct::Value;
use fontoxide_ttlib::tables::name::{NameRecord, NameString};
use fontoxide_ttlib::tables::trak::{TrackData, TrackTableEntry};
use fontoxide_ttlib::tables::{Name, Trak};
use fontoxide_ttlib::{Font, Table, TableType};

/// `/Library/Fonts/Osaka.ttf` from OSX has a trak table with both horizData and
/// vertData.
fn osaka_trak_table_data() -> Vec<u8> {
	hex(concat!(
		"00 01 00 00 00 00 00 0c 00 40 00 00 00 03 00 02 00 00 00 2c ff ff ",
		"00 00 01 06 00 34 00 00 00 00 01 07 00 38 00 01 00 00 01 08 00 3c ",
		"00 0c 00 00 00 18 00 00 ff f4 ff f4 00 00 00 00 00 0c 00 0c 00 03 ",
		"00 02 00 00 00 60 ff ff 00 00 01 09 00 68 00 00 00 00 01 0a 00 6c ",
		"00 01 00 00 01 0b 00 70 00 0c 00 00 00 18 00 00 ff f4 ff f4 00 00 ",
		"00 00 00 0c 00 0c"
	))
}

fn entry(values: &[(f64, i64)], name_index: i64) -> TrackTableEntry {
	TrackTableEntry::new(values, Some(name_index))
}

fn osaka_horiz_track_entries() -> TrackData {
	TrackData::new(vec![
		(-1.0, entry(&[(24.0, -12), (12.0, -12)], 262)),
		(0.0, entry(&[(24.0, 0), (12.0, 0)], 263)),
		(1.0, entry(&[(24.0, 12), (12.0, 12)], 264)),
	])
}

fn osaka_vert_track_entries() -> TrackData {
	TrackData::new(vec![
		(-1.0, entry(&[(24.0, -12), (12.0, -12)], 265)),
		(0.0, entry(&[(24.0, 0), (12.0, 0)], 266)),
		(1.0, entry(&[(24.0, 12), (12.0, 12)], 267)),
	])
}

const OSAKA_TRAK_TABLE_XML: &[&str] = &[
	"<version value=\"1.0\"/>",
	"<format value=\"0\"/>",
	"<horizData>",
	"  <!-- nTracks=3, nSizes=2 -->",
	"  <trackEntry value=\"-1.0\" nameIndex=\"262\">",
	"    <!-- Tight -->",
	"    <track size=\"12.0\" value=\"-12\"/>",
	"    <track size=\"24.0\" value=\"-12\"/>",
	"  </trackEntry>",
	"  <trackEntry value=\"0.0\" nameIndex=\"263\">",
	"    <!-- Normal -->",
	"    <track size=\"12.0\" value=\"0\"/>",
	"    <track size=\"24.0\" value=\"0\"/>",
	"  </trackEntry>",
	"  <trackEntry value=\"1.0\" nameIndex=\"264\">",
	"    <!-- Loose -->",
	"    <track size=\"12.0\" value=\"12\"/>",
	"    <track size=\"24.0\" value=\"12\"/>",
	"  </trackEntry>",
	"</horizData>",
	"<vertData>",
	"  <!-- nTracks=3, nSizes=2 -->",
	"  <trackEntry value=\"-1.0\" nameIndex=\"265\">",
	"    <!-- Tight -->",
	"    <track size=\"12.0\" value=\"-12\"/>",
	"    <track size=\"24.0\" value=\"-12\"/>",
	"  </trackEntry>",
	"  <trackEntry value=\"0.0\" nameIndex=\"266\">",
	"    <!-- Normal -->",
	"    <track size=\"12.0\" value=\"0\"/>",
	"    <track size=\"24.0\" value=\"0\"/>",
	"  </trackEntry>",
	"  <trackEntry value=\"1.0\" nameIndex=\"267\">",
	"    <!-- Loose -->",
	"    <track size=\"12.0\" value=\"12\"/>",
	"    <track size=\"24.0\" value=\"12\"/>",
	"  </trackEntry>",
	"</vertData>",
];

/// A made-up table containing only vertData (no horizData).
fn osaka_vert_only_trak_table_data() -> Vec<u8> {
	hex(concat!(
		"00 01 00 00 00 00 00 00 00 0c 00 00 00 03 00 02 00 00 00 2c ff ff ",
		"00 00 01 09 00 34 00 00 00 00 01 0a 00 38 00 01 00 00 01 0b 00 3c ",
		"00 0c 00 00 00 18 00 00 ff f4 ff f4 00 00 00 00 00 0c 00 0c"
	))
}

const OSAKA_VERT_ONLY_TRAK_TABLE_XML: &[&str] = &[
	"<version value=\"1.0\"/>",
	"<format value=\"0\"/>",
	"<horizData>",
	"  <!-- nTracks=0, nSizes=0 -->",
	"</horizData>",
	"<vertData>",
	"  <!-- nTracks=3, nSizes=2 -->",
	"  <trackEntry value=\"-1.0\" nameIndex=\"265\">",
	"    <!-- Tight -->",
	"    <track size=\"12.0\" value=\"-12\"/>",
	"    <track size=\"24.0\" value=\"-12\"/>",
	"  </trackEntry>",
	"  <trackEntry value=\"0.0\" nameIndex=\"266\">",
	"    <!-- Normal -->",
	"    <track size=\"12.0\" value=\"0\"/>",
	"    <track size=\"24.0\" value=\"0\"/>",
	"  </trackEntry>",
	"  <trackEntry value=\"1.0\" nameIndex=\"267\">",
	"    <!-- Loose -->",
	"    <track size=\"12.0\" value=\"12\"/>",
	"    <track size=\"24.0\" value=\"12\"/>",
	"  </trackEntry>",
	"</vertData>",
];

/// `/Library/Fonts/Skia.ttf` contains a trak table with horizData.
fn skia_trak_table_data() -> Vec<u8> {
	hex(concat!(
		"00 01 00 00 00 00 00 0c 00 00 00 00 00 03 00 05 00 00 00 2c ff ff ",
		"00 00 01 13 00 40 00 00 00 00 01 2f 00 4a 00 01 00 00 01 14 00 54 ",
		"00 09 00 00 00 0a 00 00 00 0c 00 00 00 12 00 00 00 13 00 00 ff f6 ",
		"ff e2 ff c4 ff c1 ff c1 00 0f 00 00 ff fb ff e7 ff e7 00 8c 00 82 ",
		"00 7d 00 73 00 73"
	))
}

fn skia_track_entries() -> TrackData {
	TrackData::new(vec![
		(
			-1.0,
			entry(
				&[
					(9.0, -10),
					(10.0, -30),
					(19.0, -63),
					(12.0, -60),
					(18.0, -63),
				],
				275,
			),
		),
		(
			0.0,
			entry(
				&[(9.0, 15), (10.0, 0), (19.0, -25), (12.0, -5), (18.0, -25)],
				303,
			),
		),
		(
			1.0,
			entry(
				&[
					(9.0, 140),
					(10.0, 130),
					(19.0, 115),
					(12.0, 125),
					(18.0, 115),
				],
				276,
			),
		),
	])
}

const SKIA_TRAK_TABLE_XML: &[&str] = &[
	"<version value=\"1.0\"/>",
	"<format value=\"0\"/>",
	"<horizData>",
	"  <!-- nTracks=3, nSizes=5 -->",
	"  <trackEntry value=\"-1.0\" nameIndex=\"275\">",
	"    <!-- Tight -->",
	"    <track size=\"9.0\" value=\"-10\"/>",
	"    <track size=\"10.0\" value=\"-30\"/>",
	"    <track size=\"12.0\" value=\"-60\"/>",
	"    <track size=\"18.0\" value=\"-63\"/>",
	"    <track size=\"19.0\" value=\"-63\"/>",
	"  </trackEntry>",
	"  <trackEntry value=\"0.0\" nameIndex=\"303\">",
	"    <!-- Normal -->",
	"    <track size=\"9.0\" value=\"15\"/>",
	"    <track size=\"10.0\" value=\"0\"/>",
	"    <track size=\"12.0\" value=\"-5\"/>",
	"    <track size=\"18.0\" value=\"-25\"/>",
	"    <track size=\"19.0\" value=\"-25\"/>",
	"  </trackEntry>",
	"  <trackEntry value=\"1.0\" nameIndex=\"276\">",
	"    <!-- Loose -->",
	"    <track size=\"9.0\" value=\"140\"/>",
	"    <track size=\"10.0\" value=\"130\"/>",
	"    <track size=\"12.0\" value=\"125\"/>",
	"    <track size=\"18.0\" value=\"115\"/>",
	"    <track size=\"19.0\" value=\"115\"/>",
	"  </trackEntry>",
	"</horizData>",
	"<vertData>",
	"  <!-- nTracks=0, nSizes=0 -->",
	"</vertData>",
];

/// `setUp`: a table with version 1.0 and format 0.
fn new_table() -> Trak {
	Trak {
		version: Some(Value::Float(1.0)),
		format: Some(0),
		..Default::default()
	}
}

/// `add_name`: a Mac Roman name record.
fn add_name(font: &mut Font, string: &str, name_id: u16) {
	if font.get::<Name>().unwrap().is_none() {
		font.insert(Box::new(Name::default()));
	}
	let name = font.get::<Name>().unwrap().unwrap();
	name.names.push(NameRecord::new(
		NameString::Bytes(string.as_bytes().to_vec()),
		name_id,
		1,
		0,
		0,
	));
}

fn xml(lines: &[&str]) -> String {
	lines.join("\n")
}

fn assert_same(a: &TrackData, b: &TrackData) {
	assert!(a.same_as(b), "{a:?} != {b:?}");
}

#[test]
fn test_compile_horiz() -> TestResult {
	let mut table = new_table();
	table.horiz_data = skia_track_entries();
	assert_eq!(table.compile(&mut Font::new())?, skia_trak_table_data());
	Ok(())
}

#[test]
fn test_compile_vert() -> TestResult {
	let mut table = new_table();
	table.vert_data = osaka_vert_track_entries();
	assert_eq!(
		table.compile(&mut Font::new())?,
		osaka_vert_only_trak_table_data()
	);
	Ok(())
}

#[test]
fn test_compile_horiz_and_vert() -> TestResult {
	let mut table = new_table();
	table.horiz_data = osaka_horiz_track_entries();
	table.vert_data = osaka_vert_track_entries();
	assert_eq!(table.compile(&mut Font::new())?, osaka_trak_table_data());
	Ok(())
}

#[test]
fn test_compile_longword_aligned() -> TestResult {
	let mut table = new_table();
	// Without padding, this 'horizData' would end up 46 bytes long.
	table.horiz_data = TrackData::new(vec![(0.0, entry(&[(12.0, 0), (24.0, 0), (36.0, 0)], 256))]);
	table.vert_data = TrackData::new(vec![(0.0, entry(&[(12.0, 0), (24.0, 0), (36.0, 0)], 257))]);
	table.compile(&mut Font::new())?;
	assert_eq!(table.vert_offset % 4, 0);
	Ok(())
}

#[test]
fn test_compile_sizes_mismatch() {
	let mut table = new_table();
	table.horiz_data = TrackData::new(vec![
		(-1.0, entry(&[(9.0, -10), (10.0, -30)], 256)),
		(0.0, entry(&[(8.0, 20), (12.0, 0)], 257)),
	]);
	let error = table.compile(&mut Font::new()).unwrap_err();
	assert!(
		error
			.to_string()
			.contains("entries must specify the same sizes"),
		"{error}"
	);
}

#[test]
fn test_decompile_horiz() -> TestResult {
	let table = Trak::decompile(&skia_trak_table_data(), &mut Font::new())?;
	assert_same(&table.horiz_data, &skia_track_entries());
	assert_same(&table.vert_data, &TrackData::default());
	Ok(())
}

#[test]
fn test_decompile_vert() -> TestResult {
	let table = Trak::decompile(&osaka_vert_only_trak_table_data(), &mut Font::new())?;
	assert_same(&table.horiz_data, &TrackData::default());
	assert_same(&table.vert_data, &osaka_vert_track_entries());
	Ok(())
}

#[test]
fn test_decompile_horiz_and_vert() -> TestResult {
	let table = Trak::decompile(&osaka_trak_table_data(), &mut Font::new())?;
	assert_same(&table.horiz_data, &osaka_horiz_track_entries());
	assert_same(&table.vert_data, &osaka_vert_track_entries());
	Ok(())
}

#[test]
fn test_roundtrip_decompile_compile() -> TestResult {
	for data in [
		osaka_trak_table_data(),
		osaka_vert_only_trak_table_data(),
		skia_trak_table_data(),
	] {
		let mut font = Font::new();
		let mut table = Trak::decompile(&data, &mut font)?;
		assert_eq!(table.compile(&mut font)?, data);
	}
	Ok(())
}

fn check_from_xml(lines: &[&str], horiz: &TrackData, vert: &TrackData) -> TestResult {
	let mut table = new_table();
	from_xml(&mut table, &xml(lines), &mut Font::new())?;
	assert_eq!(table.version, Some(Value::Float(1.0)));
	assert_eq!(table.format, Some(0));
	assert_same(&table.horiz_data, horiz);
	assert_same(&table.vert_data, vert);
	Ok(())
}

#[test]
fn test_from_xml_horiz() -> TestResult {
	check_from_xml(
		SKIA_TRAK_TABLE_XML,
		&skia_track_entries(),
		&TrackData::default(),
	)
}

#[test]
fn test_from_xml_horiz_and_vert() -> TestResult {
	check_from_xml(
		OSAKA_TRAK_TABLE_XML,
		&osaka_horiz_track_entries(),
		&osaka_vert_track_entries(),
	)
}

#[test]
fn test_from_xml_vert() -> TestResult {
	check_from_xml(
		OSAKA_VERT_ONLY_TRAK_TABLE_XML,
		&TrackData::default(),
		&osaka_vert_track_entries(),
	)
}

#[test]
fn test_to_xml_horiz() -> TestResult {
	let mut font = Font::new();
	let mut table = new_table();
	table.horiz_data = skia_track_entries();
	add_name(&mut font, "Tight", 275);
	add_name(&mut font, "Normal", 303);
	add_name(&mut font, "Loose", 276);
	assert_eq!(get_xml(&table, &mut font)?, SKIA_TRAK_TABLE_XML);
	Ok(())
}

#[test]
fn test_to_xml_horiz_and_vert() -> TestResult {
	let mut font = Font::new();
	let mut table = new_table();
	table.horiz_data = osaka_horiz_track_entries();
	table.vert_data = osaka_vert_track_entries();
	for (name, id) in [
		("Tight", 262),
		("Normal", 263),
		("Loose", 264),
		("Tight", 265),
		("Normal", 266),
		("Loose", 267),
	] {
		add_name(&mut font, name, id);
	}
	assert_eq!(get_xml(&table, &mut font)?, OSAKA_TRAK_TABLE_XML);
	Ok(())
}

#[test]
fn test_to_xml_vert() -> TestResult {
	let mut font = Font::new();
	let mut table = new_table();
	table.vert_data = osaka_vert_track_entries();
	add_name(&mut font, "Tight", 265);
	add_name(&mut font, "Normal", 266);
	add_name(&mut font, "Loose", 267);
	assert_eq!(get_xml(&table, &mut font)?, OSAKA_VERT_ONLY_TRAK_TABLE_XML);
	Ok(())
}

#[test]
fn test_roundtrip_from_xml_to_xml() -> TestResult {
	let mut font = Font::new();
	for (name, id) in [
		("Tight", 275),
		("Normal", 303),
		("Loose", 276),
		("Tight", 262),
		("Normal", 263),
		("Loose", 264),
		("Tight", 265),
		("Normal", 266),
		("Loose", 267),
	] {
		add_name(&mut font, name, id);
	}
	for input in [
		SKIA_TRAK_TABLE_XML,
		OSAKA_TRAK_TABLE_XML,
		OSAKA_VERT_ONLY_TRAK_TABLE_XML,
	] {
		let mut table = Trak::default();
		from_xml(&mut table, &xml(input), &mut font)?;
		assert_eq!(get_xml(&table, &mut font)?, input);
	}
	Ok(())
}
