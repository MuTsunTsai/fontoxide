//! `Tests/ttLib/tables/E_B_S_C__test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::{TestResult, from_xml, get_xml, names};
use fontoxide_ttlib::sstruct::Value;
use fontoxide_ttlib::tables::eblc::SbitLineMetrics;
use fontoxide_ttlib::tables::ebsc::{BitmapScalePart2, BitmapScaleTable};
use fontoxide_ttlib::tables::{Ebdt, Eblc, Ebsc};
use fontoxide_ttlib::{Font, ReadOptions, Table, TableType, Tag, WriteOptions};

const EBSC_XML: &str = r#"<header version="2.0" numSizes="1"/>
<bitmapScaleTable index="0">
  <sbitLineMetrics direction="hori">
    <ascender value="1"/>
    <descender value="-1"/>
    <widthMax value="3"/>
    <caretSlopeNumerator value="4"/>
    <caretSlopeDenominator value="5"/>
    <caretOffset value="6"/>
    <minOriginSB value="7"/>
    <minAdvanceSB value="8"/>
    <maxBeforeBL value="9"/>
    <minAfterBL value="10"/>
    <pad1 value="0"/>
    <pad2 value="0"/>
  </sbitLineMetrics>
  <sbitLineMetrics direction="vert">
    <ascender value="11"/>
    <descender value="-2"/>
    <widthMax value="13"/>
    <caretSlopeNumerator value="14"/>
    <caretSlopeDenominator value="15"/>
    <caretOffset value="16"/>
    <minOriginSB value="17"/>
    <minAdvanceSB value="18"/>
    <maxBeforeBL value="19"/>
    <minAfterBL value="20"/>
    <pad1 value="0"/>
    <pad2 value="0"/>
  </sbitLineMetrics>
  <ppemX value="21"/>
  <ppemY value="22"/>
  <substitutePpemX value="23"/>
  <substitutePpemY value="24"/>
</bitmapScaleTable>
"#;

fn make_bitmap_scale_table() -> BitmapScaleTable {
	BitmapScaleTable {
		hori: SbitLineMetrics {
			ascender: 1,
			descender: -1,
			width_max: 3,
			caret_slope_numerator: 4,
			caret_slope_denominator: 5,
			caret_offset: 6,
			min_origin_sb: 7,
			min_advance_sb: 8,
			max_before_bl: 9,
			min_after_bl: 10,
			pad1: 0,
			pad2: 0,
		},
		vert: SbitLineMetrics {
			ascender: 11,
			descender: -2,
			width_max: 13,
			caret_slope_numerator: 14,
			caret_slope_denominator: 15,
			caret_offset: 16,
			min_origin_sb: 17,
			min_advance_sb: 18,
			max_before_bl: 19,
			min_after_bl: 20,
			pad1: 0,
			pad2: 0,
		},
		part2: BitmapScalePart2 {
			ppem_x: 21,
			ppem_y: 22,
			substitute_ppem_x: 23,
			substitute_ppem_y: 24,
		},
	}
}

#[test]
fn test_compile_decompile_and_roundtrip_ttx() -> TestResult {
	let mut font = Font::new();
	let mut table = Ebsc::default();
	assert_eq!(table.tag(), Tag::new(b"EBSC"));
	table.version = Value::Float(2.0);
	table.bitmap_scale_tables = vec![make_bitmap_scale_table()];

	let data = table.compile(&mut font)?;

	let table2 = Ebsc::decompile(&data, &mut font)?;
	assert_eq!(table.version, table2.version);
	assert_eq!(table2.bitmap_scale_tables, [make_bitmap_scale_table()]);
	assert_eq!(
		get_xml(&table2, &mut font)?,
		EBSC_XML.lines().collect::<Vec<_>>()
	);

	let mut table3 = Ebsc::default();
	from_xml(&mut table3, EBSC_XML, &mut font)?;
	assert_eq!(table3.version, Value::Float(2.0));
	assert_eq!(table3.num_sizes, 1);
	assert_eq!(table3.bitmap_scale_tables, [make_bitmap_scale_table()]);
	Ok(())
}

#[test]
fn test_fromxml_accepts_bitmap_scale_table_before_header() -> TestResult {
	let (header, rest) = EBSC_XML.split_once('\n').unwrap();
	let mut table = Ebsc::default();
	from_xml(&mut table, &format!("{rest}{header}\n"), &mut Font::new())?;
	assert_eq!(table.version, Value::Float(2.0));
	assert_eq!(table.num_sizes, 1);
	assert_eq!(table.bitmap_scale_tables, [make_bitmap_scale_table()]);
	Ok(())
}

#[test]
fn test_fromxml_rejects_invalid_sbit_line_metrics_direction() {
	let mut table = Ebsc::default();
	let error = from_xml(
		&mut table,
		"<bitmapScaleTable index=\"0\">\n  <sbitLineMetrics direction=\"diagonal\"/>\n</bitmapScaleTable>\n",
		&mut Font::new(),
	)
	.unwrap_err();
	assert!(
		error
			.to_string()
			.contains("invalid sbitLineMetrics direction: 'diagonal'"),
		"{error}"
	);
}

#[test]
fn test_save_writes_ebsc_after_ebdt_and_eblc() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(names(&[".notdef"]));
	font.insert(Box::new(Ebdt {
		version: 2.0,
		..Default::default()
	}));
	font.insert(Box::new(Eblc {
		version: 2.0,
		..Default::default()
	}));
	font.insert(Box::new(Ebsc {
		version: Value::Float(2.0),
		bitmap_scale_tables: vec![make_bitmap_scale_table()],
		..Default::default()
	}));

	let data = font.write(&WriteOptions::default())?;
	let font = Font::read(data, ReadOptions::default())?;
	let tags: Vec<Tag> = font.reader().unwrap().tags().collect();
	assert_eq!(
		tags,
		[Tag::new(b"EBDT"), Tag::new(b"EBLC"), Tag::new(b"EBSC")]
	);
	Ok(())
}
