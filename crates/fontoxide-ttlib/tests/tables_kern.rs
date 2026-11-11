//! `Tests/ttLib/tables/_k_e_r_n_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;

use common::*;
use fontoxide_ttlib::sstruct::Value;
use fontoxide_ttlib::tables::Kern;
use fontoxide_ttlib::tables::kern::{KernFormat0, KernSubtable};
use fontoxide_ttlib::{Font, Table, TableType};

const KERN_VER_0_FMT_0_DATA: &str =
	"0000 0001 0000 0020 00 01 0003 000C 0001 0006 0004 000C FFD8 0004 001C 0028 0005 0028 FFCE";

const KERN_VER_0_FMT_0_XML: [&str; 6] = [
	r#"<version value="0"/>"#,
	r#"<kernsubtable coverage="1" format="0">"#,
	r#"  <pair l="E" r="M" v="-40"/>"#,
	r#"  <pair l="E" r="c" v="40"/>"#,
	r#"  <pair l="F" r="o" v="-50"/>"#,
	"</kernsubtable>",
];

const KERN_VER_1_FMT_0_DATA: &str = "0001 0000 0000 0001 0000 0022 00 00 0000 0003 000C 0001 0006 0004 000C FFD8 0004 001C 0028 0005 0028 FFCE";

const KERN_VER_1_FMT_0_XML: [&str; 6] = [
	r#"<version value="1.0"/>"#,
	r#"<kernsubtable coverage="0" format="0" tupleIndex="0">"#,
	r#"  <pair l="E" r="M" v="-40"/>"#,
	r#"  <pair l="E" r="c" v="40"/>"#,
	r#"  <pair l="F" r="o" v="-50"/>"#,
	"</kernsubtable>",
];

const KERN_VER_0_FMT_UNKNOWN_DATA: &str =
	"0000 0002 0000 000A 04 01 1234 5678 0000 000A 05 01 9ABC DEF0";

const KERN_VER_0_FMT_UNKNOWN_XML: [&str; 11] = [
	r#"<version value="0"/>"#,
	r#"<kernsubtable format="4">"#,
	"  <!-- unknown 'kern' subtable format -->",
	"  0000000A 04011234",
	"  5678             ",
	"</kernsubtable>",
	r#"<kernsubtable format="5">"#,
	"<!-- unknown 'kern' subtable format -->",
	"  0000000A 05019ABC",
	"  DEF0             ",
	"</kernsubtable>",
];

const KERN_VER_1_FMT_UNKNOWN_DATA: &str =
	"0001 0000 0000 0002 0000 000C 00 04 0000 1234 5678 0000 000C 00 05 0000 9ABC DEF0";

const KERN_VER_1_FMT_UNKNOWN_XML: [&str; 11] = [
	r#"<version value="1"/>"#,
	r#"<kernsubtable format="4">"#,
	"  <!-- unknown 'kern' subtable format -->",
	"  0000000C 00040000",
	"  12345678         ",
	"</kernsubtable>",
	r#"<kernsubtable format="5">"#,
	"  <!-- unknown 'kern' subtable format -->",
	"  0000000C 00050000",
	"  9ABCDEF0         ",
	"</kernsubtable>",
];

fn overflowing_data() -> Vec<u8> {
	let mut data = hex("0000 0001 0000 0274 00 01 2B11 C000 000D 4266");
	for a in 0..105u16 {
		for b in 0..105u16 {
			data.extend(a.to_be_bytes());
			data.extend(b.to_be_bytes());
			data.extend([0, 0]);
		}
	}
	data
}

/// The `font` fixture.
fn font() -> Font {
	let glyphs: Vec<String> = ('A'..='Z').chain('a'..='z').map(String::from).collect();
	let mut font = Font::new();
	font.set_glyph_order(glyphs);
	font
}

/// The `overflowing_font` fixture.
fn overflowing_font() -> Font {
	let mut font = Font::new();
	font.set_glyph_order((0..105).map(|i| format!("glyph{i}")).collect());
	font
}

/// `{("E", "M"): -40, ("E", "c"): 40, ("F", "o"): -50}` in glyph IDs.
fn pairs(font: &mut Font) -> BTreeMap<(u32, u32), i16> {
	let mut id = |n: &str| font.glyph_id(n).unwrap();
	BTreeMap::from([
		((id("E"), id("M")), -40),
		((id("E"), id("c")), 40),
		((id("F"), id("o")), -50),
	])
}

/// The version of the parametrized cases: 0, or the float 1.0.
fn version(apple: bool) -> Value {
	if apple {
		Value::Float(1.0)
	} else {
		Value::Int(0)
	}
}

fn format0(kern: &Kern) -> &KernFormat0 {
	let tables = kern.kern_tables.as_ref().unwrap();
	assert_eq!(tables.len(), 1);
	match &tables[0] {
		KernSubtable::Format0(st) => st,
		other => panic!("not format 0: {other:?}"),
	}
}

fn check_single_format_0(kern: &Kern, apple: bool, font: &mut Font) {
	assert_eq!(kern.version, Some(version(apple)));
	let st = format0(kern);
	assert_eq!(st.apple, apple);
	// Horizontal kerning is coverage 0x01 in OT kern, but the default (0) in AAT.
	assert_eq!(st.coverage, if apple { 0 } else { 1 });
	assert_eq!(st.tuple_index, if apple { Some(0) } else { None });
	assert_eq!(st.kern_table.len(), 3);
	assert_eq!(st.kern_table, pairs(font));
}

/// The table of `test_compile_single_format_0` and `test_toXML_single_format_0`.
fn single_format_0(apple: bool, font: &mut Font) -> Kern {
	Kern {
		version: Some(version(apple)),
		kern_tables: Some(vec![KernSubtable::Format0(KernFormat0 {
			apple,
			coverage: if apple { 0 } else { 1 },
			tuple_index: if apple { Some(0) } else { None },
			kern_table: pairs(font),
		})]),
	}
}

#[test]
fn test_decompile_single_format_0() -> TestResult {
	for (data, apple) in [
		(KERN_VER_0_FMT_0_DATA, false),
		(KERN_VER_1_FMT_0_DATA, true),
	] {
		let mut font = font();
		let kern = Kern::decompile(&hex(data), &mut font)?;
		check_single_format_0(&kern, apple, &mut font);
	}
	Ok(())
}

#[test]
fn test_compile_single_format_0() -> TestResult {
	for (apple, expected) in [
		(false, KERN_VER_0_FMT_0_DATA),
		(true, KERN_VER_1_FMT_0_DATA),
	] {
		let mut font = font();
		let mut kern = single_format_0(apple, &mut font);
		assert_eq!(kern.compile(&mut font)?, hex(expected));
	}
	Ok(())
}

#[test]
fn test_from_xml_single_format_0() -> TestResult {
	for (xml, apple) in [(KERN_VER_0_FMT_0_XML, false), (KERN_VER_1_FMT_0_XML, true)] {
		let mut font = font();
		let mut kern = Kern::default();
		from_xml(&mut kern, &xml.concat(), &mut font)?;
		check_single_format_0(&kern, apple, &mut font);
	}
	Ok(())
}

#[test]
fn test_to_xml_single_format_0() -> TestResult {
	for (apple, expected) in [(false, KERN_VER_0_FMT_0_XML), (true, KERN_VER_1_FMT_0_XML)] {
		let mut font = font();
		let kern = single_format_0(apple, &mut font);
		assert_eq!(get_xml(&kern, &mut font)?, expected);
	}
	Ok(())
}

fn unknown(table: &KernSubtable) -> (i64, &[u8]) {
	match table {
		KernSubtable::Unknown { format, data } => (*format, data),
		other => panic!("not unknown: {other:?}"),
	}
}

#[test]
fn test_decompile_format_unknown() -> TestResult {
	for (data, apple, header_length, st_length) in [
		(KERN_VER_0_FMT_UNKNOWN_DATA, false, 4, 10),
		(KERN_VER_1_FMT_UNKNOWN_DATA, true, 8, 12),
	] {
		let data = hex(data);
		let kern = Kern::decompile(&data, &mut font())?;
		assert_eq!(kern.version, Some(version(apple)));
		let tables = kern.kern_tables.as_ref().unwrap();
		assert_eq!(tables.len(), 2);
		let st_data = &data[header_length..];
		assert_eq!(unknown(&tables[0]), (4, &st_data[..st_length]));
		let st_data = &st_data[st_length..];
		assert_eq!(unknown(&tables[1]), (5, &st_data[..st_length]));
	}
	Ok(())
}

#[test]
fn test_compile_format_unknown() -> TestResult {
	for (apple, st_length, expected) in [
		(false, 10u32, KERN_VER_0_FMT_UNKNOWN_DATA),
		(true, 12, KERN_VER_1_FMT_UNKNOWN_DATA),
	] {
		let mut tables = Vec::new();
		for (format, kern_data) in [(4u8, "1234 5678"), (5, "9ABC DEF0")] {
			let mut data = if apple {
				hex(&format!("{st_length:08X} {:02X} {format:02X} {:04X}", 0, 0))
			} else {
				hex(&format!("{:04X} {st_length:04X} {format:02X} {:02X}", 0, 1))
			};
			data.extend(hex(kern_data));
			tables.push(KernSubtable::Unknown {
				format: i64::from(format),
				data,
			});
		}
		let mut kern = Kern {
			version: Some(version(apple)),
			kern_tables: Some(tables),
		};
		assert_eq!(kern.compile(&mut font())?, hex(expected));
	}
	Ok(())
}

#[test]
fn test_from_xml_format_unknown() -> TestResult {
	for (xml, version, st_length) in [
		(&KERN_VER_0_FMT_UNKNOWN_XML, Value::Int(0), 10),
		(&KERN_VER_1_FMT_UNKNOWN_XML, Value::Int(1), 12),
	] {
		let mut kern = Kern::default();
		from_xml(&mut kern, &xml.concat(), &mut font())?;
		// fontTools compares with `==`, where 1 == 1.0.
		assert_eq!(kern.version, Some(version));
		let tables = kern.kern_tables.as_ref().unwrap();
		assert_eq!(tables.len(), 2);
		let (format, data) = unknown(&tables[0]);
		assert_eq!((format, data.len()), (4, st_length));
		let (format, data) = unknown(&tables[1]);
		assert_eq!((format, data.len()), (5, st_length));
	}
	Ok(())
}

#[test]
fn test_to_xml_format_unknown() -> TestResult {
	for (apple, version_str) in [(false, "0"), (true, "1.0")] {
		let kern = Kern {
			version: Some(version(apple)),
			kern_tables: Some(vec![KernSubtable::Unknown {
				format: 4,
				data: b"ABCD".to_vec(),
			}]),
		};
		assert_eq!(
			get_xml(&kern, &mut font())?,
			[
				format!(r#"<version value="{version_str}"/>"#),
				r#"<kernsubtable format="4">"#.to_owned(),
				"  <!-- unknown 'kern' subtable format -->".to_owned(),
				"  41424344   ".to_owned(),
				"</kernsubtable>".to_owned(),
			]
		);
	}
	Ok(())
}

#[test]
fn test_getkern() {
	let mut table = Kern {
		version: Some(Value::Int(0)),
		kern_tables: Some(Vec::new()),
	};
	assert!(table.get_kern(0).is_none());
	let st0 = KernSubtable::Format0(KernFormat0::default());
	table.kern_tables.as_mut().unwrap().push(st0.clone());
	assert_eq!(table.get_kern(0), Some(&st0));
	assert!(table.get_kern(4).is_none());
	table
		.kern_tables
		.as_mut()
		.unwrap()
		.push(KernSubtable::Unknown {
			format: 4,
			data: Vec::new(),
		});
	assert_eq!(table.get_kern(4).map(KernSubtable::format), Some(4));
}

#[test]
fn test_decompile_bad_glyph_id() -> TestResult {
	// The subtable of the upstream test, in a version 0 table.
	let mut data = hex("0000 0001");
	data.extend(hex(
		"0000 001a 00 00 0002 0000 0000 0000 0001 0003 0001 0001 ffff 0002",
	));
	let mut font = font();
	let kern = Kern::decompile(&data, &mut font)?;
	let st = format0(&kern);
	let b = font.glyph_id("B")?;
	let d = font.glyph_id("D")?;
	assert_eq!(st.kern_table.get(&(b, d)), Some(&1));
	assert_eq!(st.kern_table.get(&(b, 0xFFFF)), Some(&2));
	assert_eq!(font.glyph_name(0xFFFF)?, "glyph65535");
	Ok(())
}

#[test]
fn test_compile_overflowing_subtable() -> TestResult {
	let mut font = overflowing_font();
	let mut kern_table = BTreeMap::new();
	for a in 0..105 {
		for b in 0..105 {
			kern_table.insert((a, b), 0);
		}
	}
	assert_eq!(kern_table.len(), 11025);
	let mut kern = Kern {
		version: Some(Value::Int(0)),
		kern_tables: Some(vec![KernSubtable::Format0(KernFormat0 {
			apple: false,
			coverage: 1,
			tuple_index: None,
			kern_table,
		})]),
	};
	assert_eq!(kern.compile(&mut font)?, overflowing_data());
	Ok(())
}

#[test]
fn test_decompile_overflowing_subtable() -> TestResult {
	let mut font = overflowing_font();
	let kern = Kern::decompile(&overflowing_data(), &mut font)?;
	let st = format0(&kern);
	let mut expected = BTreeMap::new();
	for a in 0..105 {
		for b in 0..105 {
			expected.insert((a, b), 0);
		}
	}
	assert_eq!(st.kern_table, expected);
	Ok(())
}
