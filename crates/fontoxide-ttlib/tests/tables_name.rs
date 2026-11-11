//! `Tests/ttLib/tables/_n_a_m_e_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use fontoxide_misc::encoding_tools::get_encoding;
use fontoxide_ttlib::tables::misc_tables::Ltag;
use fontoxide_ttlib::tables::name::{self, Name, NameRecord, NameString};
use fontoxide_ttlib::{Font, Table, TableType, Tag};

type Names = Vec<(u16, u16, u16, u16, String)>;

/// `names(nameTable)`: the sorted records as tuples.
fn names_of(table: &Name) -> Names {
	let mut result: Names = table
		.names
		.iter()
		.map(|n| {
			(
				n.name_id,
				n.platform_id,
				n.plat_enc_id,
				n.lang_id,
				n.to_unicode().unwrap(),
			)
		})
		.collect();
	result.sort();
	result
}

fn make_name(string: impl Into<NameString>, name_id: u16, p: u16, e: u16, l: u16) -> NameRecord {
	NameRecord::new(string, name_id, p, e, l)
}

/// `table.getName(...).toUnicode()`.
fn get_name(table: &Name, name_id: u16, p: u16, e: u16, l: u16) -> Option<String> {
	table
		.get_name(name_id, p, e, Some(l))
		.map(|r| r.to_unicode().unwrap())
}

#[test]
fn test_init() {
	let table = Name::default();
	assert!(table.names.is_empty());
	assert_eq!(table.tag(), Tag::new(b"name"));
}

#[test]
fn test_get_debug_name() {
	let table = Name {
		names: vec![
			make_name("Bold", 258, 1, 0, 0),
			make_name("Gras", 258, 1, 0, 1),
			make_name("Fett", 258, 1, 0, 2),
			make_name("Sem Fracções", 292, 1, 0, 8),
		],
	};
	assert_eq!(table.get_debug_name(258).as_deref(), Some("Bold"));
	assert_eq!(table.get_debug_name(292).as_deref(), Some("Sem Fracções"));
	assert_eq!(table.get_debug_name(999), None);
}

#[test]
fn test_get_debug_name_prefers_windows_english() {
	let table = Name {
		names: vec![
			make_name("Mac Bold", 258, 1, 0, 0),
			make_name("Mac Gras", 258, 1, 0, 1),
			make_name("Windows Gras", 258, 3, 1, 0x40C),
			make_name("Windows Bold", 258, 3, 1, 0x409),
			make_name("Mac Regular", 259, 1, 0, 0),
			make_name("Windows Normal", 259, 3, 1, 0x40C),
		],
	};
	assert_eq!(table.get_debug_name(258).as_deref(), Some("Windows Bold"));
	// Falls back to the Mac English name without a Windows English one.
	assert_eq!(table.get_debug_name(259).as_deref(), Some("Mac Regular"));
}

#[test]
fn test_set_name() {
	let mut table = Name::default();
	table.set_name("Regular", 2, 1, 0, 0);
	table.set_name("Version 1.000", 5, 3, 1, 0x409);
	table.set_name("寬鬆", 276, 1, 2, 0x13);
	assert_eq!(get_name(&table, 2, 1, 0, 0).as_deref(), Some("Regular"));
	assert_eq!(
		get_name(&table, 5, 3, 1, 0x409).as_deref(),
		Some("Version 1.000")
	);
	assert_eq!(get_name(&table, 276, 1, 2, 0x13).as_deref(), Some("寬鬆"));
	assert_eq!(table.names.len(), 3);
	table.set_name("緊縮", 276, 1, 2, 0x13);
	assert_eq!(get_name(&table, 276, 1, 2, 0x13).as_deref(), Some("緊縮"));
	assert_eq!(table.names.len(), 3);
	// Passing bytes issues a warning. (Other types are rejected at compile time.)
	let (logs, ()) = capture_logs(|| table.set_name(b"abc".to_vec(), 0, 1, 0, 0));
	assert_eq!(count_logs(&logs, log::Level::Warn, "string is bytes"), 1);
}

#[test]
fn test_names_sort_bytes_str() -> TestResult {
	// Duplicate records, one with text and one with bytes.
	let mut table = Name {
		names: vec![
			make_name("Test", 25, 3, 1, 0x409),
			make_name(
				"Test"
					.encode_utf16()
					.flat_map(u16::to_be_bytes)
					.collect::<Vec<u8>>(),
				25,
				3,
				1,
				0x409,
			),
		],
	};
	table.compile(&mut Font::new())?;
	Ok(())
}

#[test]
fn test_names_sort_encoding() {
	// Encoding errors do not prevent sorting by the IDs.
	let mut table = Name {
		names: vec![
			make_name("Mac Unicode 寬 encodes ok", 25, 3, 0, 0x409),
			make_name("Win Latin 寬 fails to encode", 25, 1, 0, 0),
		],
	};
	table.sort();
	assert_eq!(table.names[0].platform_id, 1);
	assert_eq!(table.names[1].platform_id, 3);
}

#[test]
fn test_add_name() -> TestResult {
	let mut table = Name::default();
	let mut name_ids = Vec::new();
	for string in ["Width", "Weight", "Custom"] {
		name_ids.push(table.add_name_default(string)?);
	}
	assert_eq!(name_ids, [256, 257, 258]);
	assert_eq!(table.names.len(), 6);
	let strings: Vec<_> = table.names.iter().map(|n| n.string.clone()).collect();
	assert_eq!(
		strings,
		["Width", "Width", "Weight", "Weight", "Custom", "Custom"].map(NameString::from)
	);
	assert!(
		table
			.add_name("Invalid nameID", &[(1, 0, 0), (3, 1, 0x409)], 32767)
			.is_err()
	);
	Ok(())
}

#[test]
fn test_remove_names() {
	let mut table = Name::default();
	table.set_name("Regular", 2, 1, 0, 0);
	table.set_name("Regular", 2, 3, 1, 0x409);
	table.remove_names(Some(2), None, None, None);
	assert!(table.names.is_empty());

	let mut table = Name::default();
	table.set_name("FamilyName", 1, 1, 0, 0);
	table.set_name("Regular", 2, 1, 0, 0);
	table.set_name("FamilyName", 1, 3, 1, 0x409);
	table.set_name("Regular", 2, 3, 1, 0x409);
	table.remove_names(None, Some(1), None, None);
	assert_eq!(table.names.len(), 2);
	assert!(table.get_name(1, 1, 0, Some(0)).is_none());
	assert!(table.get_name(2, 1, 0, Some(0)).is_none());
	let rec1 = table.get_name(1, 3, 1, Some(0x409)).unwrap();
	assert_eq!(rec1.to_str_lossy(), "FamilyName");
	let rec2 = table.get_name(2, 3, 1, Some(0x409)).unwrap();
	assert_eq!(rec2.to_str_lossy(), "Regular");

	let mut table = Name::default();
	table.set_name("FamilyName", 1, 1, 0, 0);
	table.set_name("Regular", 2, 1, 0, 0);
	table.remove_names(Some(1), None, None, None);
	assert_eq!(table.names.len(), 1);
	assert!(table.get_name(1, 1, 0, Some(0)).is_none());
	assert_eq!(
		table.get_name(2, 1, 0, Some(0)).unwrap().to_str_lossy(),
		"Regular"
	);

	let mut table = Name::default();
	table.set_name("FamilyName", 1, 1, 0, 0);
	table.set_name("Regular", 2, 1, 0, 0);
	table.remove_names(Some(2), Some(1), Some(0), Some(0));
	assert_eq!(table.names.len(), 1);
	assert!(table.get_name(2, 1, 0, Some(0)).is_none());
	assert_eq!(
		table.get_name(1, 1, 0, Some(0)).unwrap().to_str_lossy(),
		"FamilyName"
	);

	let mut table = Name::default();
	table.set_name("FamilyName", 1, 1, 0, 0);
	table.set_name("Regular", 2, 1, 0, 0);
	table.remove_names(None, None, None, None);
	assert_eq!(table.names.len(), 2);
	assert_eq!(
		table.get_name(1, 1, 0, Some(0)).unwrap().to_str_lossy(),
		"FamilyName"
	);
	assert_eq!(
		table.get_name(2, 1, 0, Some(0)).unwrap().to_str_lossy(),
		"Regular"
	);
}

const NAMES: [(&str, &str); 3] = [("en", "Width"), ("de-CH", "Breite"), ("gsw-LI", "Bräiti")];
const NAMES_SUBSET: [(&str, &str); 2] = [("en", "Width"), ("de-CH", "Breite")];
const NAMES_SUPERSET: [(&str, &str); 4] = [
	("en", "Width"),
	("de-CH", "Breite"),
	("gsw-LI", "Bräiti"),
	("nl", "Breedte"),
];

#[test]
fn test_find_multilingual_name() -> TestResult {
	let mut table = Name::default();
	let name_id = table.add_multilingual_name(&NAMES, None, true, true, 0)?;
	assert_eq!(
		table.find_multilingual_name(&NAMES, true, true, 0)?,
		Some(name_id)
	);
	assert_eq!(
		table.find_multilingual_name(&NAMES_SUBSET, true, true, 0)?,
		Some(name_id)
	);
	assert_eq!(
		table.find_multilingual_name(&NAMES_SUPERSET, true, true, 0)?,
		None
	);
	Ok(())
}

#[test]
fn test_find_multilingual_name_compiled() -> TestResult {
	let mut table = Name::default();
	let name_id = table.add_multilingual_name(&NAMES, None, true, true, 0)?;
	// After compiling and decompiling, the strings are bytes.
	let mut font = Font::new();
	let data = table.compile(&mut font)?;
	let table = Name::decompile(&data, &mut font)?;
	assert_eq!(
		table.find_multilingual_name(&NAMES, true, true, 0)?,
		Some(name_id)
	);
	assert_eq!(
		table.find_multilingual_name(&NAMES_SUBSET, true, true, 0)?,
		Some(name_id)
	);
	assert_eq!(
		table.find_multilingual_name(&NAMES_SUPERSET, true, true, 0)?,
		None
	);
	Ok(())
}

#[test]
fn test_add_multilingual_name_reuse() -> TestResult {
	let mut table = Name::default();
	let name_id = table.add_multilingual_name(&NAMES, None, true, true, 0)?;
	assert_eq!(
		table.add_multilingual_name(&NAMES, None, true, true, 0)?,
		name_id
	);
	assert_eq!(
		table.add_multilingual_name(&NAMES_SUBSET, None, true, true, 0)?,
		name_id
	);
	table.add_multilingual_name(&NAMES_SUPERSET, None, true, true, 0)?;
	Ok(())
}

#[test]
fn test_find_multilingual_name_no_mac() -> TestResult {
	let mut table = Name::default();
	let name_id = table.add_multilingual_name(&NAMES, None, true, false, 0)?;
	assert_eq!(
		table.find_multilingual_name(&NAMES, true, false, 0)?,
		Some(name_id)
	);
	assert_eq!(table.find_multilingual_name(&NAMES, true, true, 0)?, None);
	assert_eq!(
		table.find_multilingual_name(&NAMES_SUBSET, true, false, 0)?,
		Some(name_id)
	);
	assert_eq!(
		table.find_multilingual_name(&NAMES_SUBSET, true, true, 0)?,
		None
	);
	assert_eq!(
		table.find_multilingual_name(&NAMES_SUPERSET, true, true, 0)?,
		None
	);
	Ok(())
}

/// `FakeFont(glyphs=[".notdef", "A"])` with a new `name` table.
fn font_with_name() -> Font {
	let mut font = fake_font(&[".notdef", "A"]);
	font.insert(Box::new(Name::default()));
	font
}

fn ltag_tags(font: &mut Font) -> Vec<String> {
	font.require::<Ltag>().unwrap().tags.clone()
}

#[test]
fn test_add_multilingual_name() -> TestResult {
	let mut font = font_with_name();
	let (logs, ids) = capture_logs(|| -> fontoxide_ttlib::Result<(u16, u16)> {
		let width_id = name::add_multilingual_name(&mut font, &NAMES, None, true, false, 0)?;
		let x_height_id = name::add_multilingual_name(
			&mut font,
			&[("en", "X-Height"), ("gsw-LI", "X-Hööchi")],
			None,
			true,
			false,
			0,
		)?;
		Ok((width_id, x_height_id))
	});
	assert_eq!(ids?, (256, 257));
	assert!(
		count_logs(
			&logs,
			log::Level::Warn,
			"cannot add Windows name in language gsw-LI"
		) > 0
	);
	assert_eq!(
		names_of(font.require::<Name>()?),
		[
			(256, 0, 4, 0, "Bräiti".to_owned()),
			(256, 3, 1, 0x0409, "Width".to_owned()),
			(256, 3, 1, 0x0807, "Breite".to_owned()),
			(257, 0, 4, 0, "X-Hööchi".to_owned()),
			(257, 3, 1, 0x0409, "X-Height".to_owned()),
		]
	);
	let mut tags = font.tags();
	tags.sort();
	assert_eq!(tags, [Tag::new(b"ltag"), Tag::new(b"name")]);
	assert_eq!(ltag_tags(&mut font), ["gsw-LI"]);
	Ok(())
}

#[test]
fn test_add_multilingual_name_legacy_mac_encoding() -> TestResult {
	let mut font = font_with_name();
	let (logs, id) = capture_logs(|| {
		name::add_multilingual_name(&mut font, &[("la", "SPQR")], None, true, true, 0)
	});
	id?;
	assert!(
		count_logs(
			&logs,
			log::Level::Warn,
			"cannot add Windows name in language la"
		) > 0
	);
	assert_eq!(
		names_of(font.require::<Name>()?),
		[(256, 1, 0, 131, "SPQR".to_owned())]
	);
	assert!(!font.contains(Tag::new(b"ltag")));
	Ok(())
}

#[test]
fn test_add_multilingual_name_legacy_mac_encoding_but_unencodable_name() -> TestResult {
	let mut font = font_with_name();
	let (logs, id) = capture_logs(|| {
		name::add_multilingual_name(&mut font, &[("la", "ⱾƤℚⱤ")], None, true, true, 0)
	});
	id?;
	assert!(
		count_logs(
			&logs,
			log::Level::Warn,
			"cannot add Windows name in language la"
		) > 0
	);
	assert_eq!(
		names_of(font.require::<Name>()?),
		[(256, 0, 4, 0, "ⱾƤℚⱤ".to_owned())]
	);
	assert!(font.contains(Tag::new(b"ltag")));
	assert_eq!(ltag_tags(&mut font), ["la"]);
	Ok(())
}

#[test]
fn test_add_multilingual_name_legacy_mac_encoding_but_no_codec() -> TestResult {
	let mut font = font_with_name();
	let (logs, id) = capture_logs(|| {
		name::add_multilingual_name(
			&mut font,
			&[("az-Arab", "آذربايجان ديلی")],
			None,
			true,
			true,
			0,
		)
	});
	id?;
	assert!(
		count_logs(
			&logs,
			log::Level::Warn,
			"cannot add Windows name in language az-Arab"
		) > 0
	);
	assert_eq!(
		names_of(font.require::<Name>()?),
		[(256, 0, 4, 0, "آذربايجان ديلی".to_owned())]
	);
	assert!(font.contains(Tag::new(b"ltag")));
	assert_eq!(ltag_tags(&mut font), ["az-Arab"]);
	Ok(())
}

#[test]
fn test_add_multilingual_name_mac_language_codes() -> TestResult {
	let mut font = font_with_name();
	name::add_multilingual_name(
		&mut font,
		&[
			("es", "Ancho"),
			("et", "Laius"),
			("ga", "Leithead"),
			("ga-Latg", "Leithead"),
		],
		None,
		false,
		true,
		0,
	)?;
	assert_eq!(
		names_of(font.require::<Name>()?),
		[
			(256, 1, 0, 6, "Ancho".to_owned()),
			(256, 1, 0, 35, "Leithead".to_owned()),
			(256, 1, 0, 146, "Leithead".to_owned()),
			(256, 1, 29, 27, "Laius".to_owned()),
		]
	);
	assert!(!font.contains(Tag::new(b"ltag")));
	Ok(())
}

#[test]
fn test_add_multilingual_name_windows_sami_language_codes() -> TestResult {
	let mut table = Name::default();
	table.add_multilingual_name(
		&[("sms", "Skolt"), ("sma", "Southern")],
		None,
		true,
		false,
		0,
	)?;
	assert_eq!(
		names_of(&table),
		[
			(256, 3, 1, 0x1C3B, "Southern".to_owned()),
			(256, 3, 1, 0x203B, "Skolt".to_owned()),
		]
	);
	Ok(())
}

#[test]
fn test_add_multilingual_name_no_ttfont() -> TestResult {
	// Without a font, the names that need an ltag table are left out.
	let mut table = Name::default();
	let (logs, id) = capture_logs(|| {
		table.add_multilingual_name(&[("en", "A"), ("la", "ⱾƤℚⱤ")], None, true, true, 0)
	});
	id?;
	assert!(
		count_logs(
			&logs,
			log::Level::Warn,
			"cannot store language la into 'ltag' table"
		) > 0
	);
	Ok(())
}

#[test]
fn test_add_multilingual_name_ttfont() -> TestResult {
	// With a font, there is no warning about the ltag table.
	let mut font = font_with_name();
	let (logs, id) = capture_logs(|| {
		name::add_multilingual_name(&mut font, &[("en", "A"), ("ar", "ع")], None, true, true, 0)
	});
	id?;
	assert!(logs.iter().all(|(l, _)| *l > log::Level::Warn), "{logs:?}");
	Ok(())
}

#[test]
fn test_add_multilingual_name_min_name_id() -> TestResult {
	let mut table = Name::default();
	let name_id = table.add_multilingual_name(&NAMES, Some(2), true, true, 0)?;
	assert_eq!(name_id, 2);
	let name_id = table.add_multilingual_name(&NAMES, None, true, true, 0)?;
	assert_eq!(name_id, 2);
	let name_id = table.add_multilingual_name(&NAMES, None, true, true, 256)?;
	assert!(name_id >= 256);
	assert_eq!(
		table.find_multilingual_name(&NAMES, true, true, 256)?,
		Some(name_id)
	);
	Ok(())
}

#[test]
fn test_add_multilingual_name_name_inconsistencies() -> TestResult {
	let mut table = Name::default();
	table.set_name("Weight", 270, 3, 1, 0x409);
	// The existing record lacks the Mac name, so a new name ID is added.
	let name_id = table.add_multilingual_name(&[("en", "Weight")], None, true, true, 256)?;
	assert_eq!(name_id, 271);
	Ok(())
}

#[test]
fn test_decompile_bad_offset() -> TestResult {
	// https://github.com/fonttools/fonttools/issues/525
	let mut data = hex("0001 0001 0012");
	// platformID, platEncID, langID, nameID, length, offset (out of range).
	data.extend(hex("0001 0003 0007 0001 0003 223D"));
	let table = Name::decompile(&data, &mut Font::new())?;
	assert!(table.names.is_empty());
	Ok(())
}

#[test]
fn test_to_unicode_utf16be() {
	let name = make_name("Foo Bold", 111, 0, 2, 7);
	assert_eq!(name.encoding(), "utf_16_be");
	assert_eq!(name.to_unicode().unwrap(), "Foo Bold");
}

#[test]
fn test_to_unicode_macroman() {
	let name = make_name("Foo Italic", 222, 1, 0, 7);
	assert_eq!(name.encoding(), "mac_roman");
	assert_eq!(name.to_unicode().unwrap(), "Foo Italic");
}

#[test]
fn test_to_unicode_macromanian() {
	let name = make_name(b"Foo Italic\xfb".to_vec(), 222, 1, 0, 37);
	assert_eq!(name.encoding(), "mac_romanian");
	assert_eq!(name.to_unicode().unwrap(), "Foo Italic\u{02DA}");
}

#[test]
fn test_to_unicode_unicode_decode_error() {
	let name = make_name(b"\x01".to_vec(), 111, 0, 2, 7);
	assert_eq!(name.encoding(), "utf_16_be");
	assert!(name.to_unicode().is_err());
}

#[test]
fn test_to_unicode_single_char() {
	// https://github.com/fonttools/fonttools/issues/1997
	let name = make_name("A", 256, 3, 1, 0x409);
	assert_eq!(name.to_unicode().unwrap(), "A");
}

/// `NameRecordTest.toXML(name)`.
fn record_xml(name: NameRecord) -> Vec<String> {
	let table = Name { names: vec![name] };
	get_xml(&table, &mut Font::new()).unwrap()
}

fn expected_xml(open: &str, text: &str) -> Vec<String> {
	vec![
		open.to_owned(),
		format!("  {text}"),
		"</namerecord>".to_owned(),
	]
}

const UTF16_OPEN: &str = r#"<namerecord nameID="111" platformID="0" platEncID="2" langID="0x7">"#;

#[test]
fn test_to_xml_utf16be() {
	assert_eq!(
		record_xml(make_name("Foo Bold", 111, 0, 2, 7)),
		expected_xml(UTF16_OPEN, "Foo Bold")
	);
}

#[test]
fn test_to_xml_utf16be_odd_length1() {
	assert_eq!(
		record_xml(make_name(b"\0F\0o\0o\0".to_vec(), 111, 0, 2, 7)),
		expected_xml(UTF16_OPEN, "Foo")
	);
}

#[test]
fn test_to_xml_utf16be_odd_length2() {
	assert_eq!(
		record_xml(make_name(b"\0Fooz".to_vec(), 111, 0, 2, 7)),
		expected_xml(UTF16_OPEN, "Fooz")
	);
}

#[test]
fn test_to_xml_utf16be_double_encoded() {
	assert_eq!(
		record_xml(make_name(b"\0\0\0F\0\0\0o".to_vec(), 111, 0, 2, 7)),
		expected_xml(UTF16_OPEN, "Fo")
	);
}

const MACROMAN_OPEN: &str =
	r#"<namerecord nameID="222" platformID="1" platEncID="0" langID="0x7" unicode="True">"#;

#[test]
fn test_to_xml_macroman() {
	assert_eq!(
		record_xml(make_name("Foo Italic", 222, 1, 0, 7)),
		expected_xml(MACROMAN_OPEN, "Foo Italic")
	);
}

#[test]
fn test_to_xml_macroman_actual_utf16be() {
	assert_eq!(
		record_xml(make_name("\0F\0o\0o", 222, 1, 0, 7)),
		expected_xml(MACROMAN_OPEN, "Foo")
	);
}

#[test]
fn test_to_xml_unknown_plat_enc_id_non_ascii() {
	assert_eq!(
		record_xml(make_name(b"B\x8arli".to_vec(), 333, 1, 9876, 7)),
		expected_xml(
			r#"<namerecord nameID="333" platformID="1" platEncID="9876" langID="0x7" unicode="False">"#,
			"B&#138;rli"
		)
	);
}

#[test]
fn test_to_xml_unknown_plat_enc_id_ascii() {
	assert_eq!(
		record_xml(make_name(b"Barli".to_vec(), 333, 1, 9876, 7)),
		expected_xml(
			r#"<namerecord nameID="333" platformID="1" platEncID="9876" langID="0x7" unicode="True">"#,
			"Barli"
		)
	);
}

#[test]
fn test_encoding_macroman_misc() {
	let mut name = make_name("", 123, 1, 0, 17);
	assert_eq!(name.encoding(), "mac_turkish");
	name.lang_id = 37;
	assert_eq!(name.encoding(), "mac_romanian");
	name.lang_id = 45;
	assert_eq!(name.encoding(), "mac_roman");
}

#[test]
fn test_extended_mac_encodings() {
	let name = make_name(b"\xfe".to_vec(), 123, 1, 1, 0);
	assert_eq!(name.to_unicode().unwrap(), "\u{2122}");
}

#[test]
fn test_extended_mac_encodings_errors() {
	use fontoxide_encodings::codecs::Errors;
	let s = "汉仪彩云体简";
	let encoded = fontoxide_encodings::codecs::encode("x_mac_simp_chinese_ttx", s).unwrap();
	let mut name = make_name(encoded.clone(), 123, 1, 25, 0);
	// first check we round-trip with 'strict'
	assert_eq!(name.to_unicode_with(Errors::Strict).unwrap(), s);

	// append an incomplete invalid sequence and check that we handle
	// errors with the requested error handler
	let mut truncated = encoded;
	truncated.push(0xba);
	name.string = NameString::Bytes(truncated);
	assert_eq!(
		name.to_unicode_with(Errors::BackslashReplace).unwrap(),
		format!("{s}\\xba")
	);
	assert_eq!(
		name.to_unicode_with(Errors::Replace).unwrap(),
		format!("{s}\u{FFFD}")
	);
	assert!(name.to_unicode().is_err());
	// `str(name)`.
	assert_eq!(name.to_str_lossy(), format!("{s}\\xba"));
}

#[test]
fn test_extended_unknown() {
	let name = make_name(b"\xfe".to_vec(), 123, 10, 11, 12);
	assert_eq!(name.encoding(), "ascii");
	assert_eq!(
		get_encoding(name.platform_id, name.plat_enc_id, name.lang_id),
		None
	);
}

fn family_names() -> Name {
	Name {
		names: vec![
			make_name("Copyright", 0, 1, 0, 0),
			make_name("Family Name ID 1", 1, 1, 0, 0),
			make_name("SubFamily Name ID 2", 2, 1, 0, 0),
			make_name("Unique Name ID 3", 3, 1, 0, 0),
			make_name("Full Name ID 4", 4, 1, 0, 0),
			make_name("PS Name ID 6", 6, 1, 0, 0),
			make_name("Version Name ID 5", 5, 1, 0, 0),
			make_name("Trademark Name ID 7", 7, 1, 0, 0),
		],
	}
}

#[test]
fn test_get_family_name() {
	let mut name = family_names();
	assert_eq!(name.best_family_name().as_deref(), Some("Family Name ID 1"));
	name.set_name("Family Name ID 16", 16, 1, 0, 0);
	assert_eq!(
		name.best_family_name().as_deref(),
		Some("Family Name ID 16")
	);
	name.set_name("Family Name ID 21", 21, 1, 0, 0);
	assert_eq!(
		name.best_family_name().as_deref(),
		Some("Family Name ID 21")
	);
}

#[test]
fn test_get_subfamily_name() {
	let mut name = family_names();
	assert_eq!(
		name.best_sub_family_name().as_deref(),
		Some("SubFamily Name ID 2")
	);
	name.set_name("Family Name ID 17", 17, 1, 0, 0);
	assert_eq!(
		name.best_sub_family_name().as_deref(),
		Some("Family Name ID 17")
	);
	name.set_name("Family Name ID 22", 22, 1, 0, 0);
	assert_eq!(
		name.best_sub_family_name().as_deref(),
		Some("Family Name ID 22")
	);
}

#[test]
fn test_get_nice_full_name() {
	let mut name = Name {
		names: vec![
			make_name("NID 1", 1, 1, 0, 0),
			make_name("NID 2", 2, 1, 0, 0),
			make_name("NID 4", 4, 1, 0, 0),
			make_name("NID 6", 6, 1, 0, 0),
		],
	};
	assert_eq!(name.best_full_name().as_deref(), Some("NID 1 NID 2"));
	// Still the same: name ID 17 is missing.
	name.set_name("NID 16", 16, 1, 0, 0);
	assert_eq!(name.best_full_name().as_deref(), Some("NID 1 NID 2"));
	name.set_name("NID 17", 17, 1, 0, 0);
	assert_eq!(name.best_full_name().as_deref(), Some("NID 16 NID 17"));
	// Still the same: name ID 22 is missing.
	name.set_name("NID 21", 21, 1, 0, 0);
	assert_eq!(name.best_full_name().as_deref(), Some("NID 16 NID 17"));
	name.set_name("NID 22", 22, 1, 0, 0);
	assert_eq!(name.best_full_name().as_deref(), Some("NID 21 NID 22"));
	for nid in [2, 16, 17, 21, 22] {
		name.remove_names(Some(nid), None, None, None);
	}
	assert_eq!(name.best_full_name().as_deref(), Some("NID 4"));
	name.set_name("Regular", 2, 1, 0, 0);
	assert_eq!(name.best_full_name().as_deref(), Some("NID 1"));
}
