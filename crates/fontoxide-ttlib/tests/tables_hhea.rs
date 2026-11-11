//! `Tests/ttLib/tables/_h_h_e_a_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use fontoxide_ttlib::tables::Hhea;
use fontoxide_ttlib::{Font, Table, TableType};

const HHEA_DATA: &str =
	"0001 0000 02EE FF06 00C8 03E8 FFE7 FFEC 03D1 0000 0001 0010 0000 0000 0000 0000 0000 002A";

const HHEA_XML: [&str; 17] = [
	r#"<tableVersion value="0x00010000"/>"#,
	r#"<ascent value="750"/>"#,
	r#"<descent value="-250"/>"#,
	r#"<lineGap value="200"/>"#,
	r#"<advanceWidthMax value="1000"/>"#,
	r#"<minLeftSideBearing value="-25"/>"#,
	r#"<minRightSideBearing value="-20"/>"#,
	r#"<xMaxExtent value="977"/>"#,
	r#"<caretSlopeRise value="0"/>"#,
	r#"<caretSlopeRun value="1"/>"#,
	r#"<caretOffset value="16"/>"#,
	r#"<reserved0 value="0"/>"#,
	r#"<reserved1 value="0"/>"#,
	r#"<reserved2 value="0"/>"#,
	r#"<reserved3 value="0"/>"#,
	r#"<metricDataFormat value="0"/>"#,
	r#"<numberOfHMetrics value="42"/>"#,
];

/// `HHEA_AS_DICT`, which is also the table of `HheaCompileOrToXMLTest.setUp`.
fn hhea_as_dict() -> Hhea {
	Hhea {
		table_version: 0x00010000,
		ascent: 750,
		descent: -250,
		line_gap: 200,
		advance_width_max: 1000,
		min_left_side_bearing: -25,
		min_right_side_bearing: -20,
		x_max_extent: 977,
		caret_slope_rise: 0,
		caret_slope_run: 1,
		caret_offset: 16,
		metric_data_format: 0,
		number_of_h_metrics: 42,
		..Default::default()
	}
}

/// `TTFont(sfntVersion="OTTO")`.
fn otto_font() -> Font {
	let mut font = Font::new();
	font.sfnt_version = fontoxide_ttlib::Tag::new(b"OTTO");
	font
}

#[test]
fn test_compile() -> TestResult {
	let mut font = otto_font();
	assert_eq!(hhea_as_dict().compile(&mut font)?, hex(HHEA_DATA));
	Ok(())
}

#[test]
fn test_compile_version_10_as_float() -> TestResult {
	let mut font = otto_font();
	// `tableVersion = 1.0`: versions below 0x10000 are taken as floats.
	let mut hhea = Hhea {
		table_version: 1,
		..hhea_as_dict()
	};
	let (logs, data) = capture_logs(|| hhea.compile(&mut font));
	assert_eq!(data?, hex(HHEA_DATA));
	assert_eq!(
		count_logs(&logs, log::Level::Warn, "Table version value is a float"),
		1
	);
	Ok(())
}

#[test]
fn test_to_xml() -> TestResult {
	let mut font = otto_font();
	assert_eq!(get_xml(&hhea_as_dict(), &mut font)?, HHEA_XML);
	Ok(())
}

#[test]
fn test_to_xml_version_as_float() -> TestResult {
	let mut font = otto_font();
	let hhea = Hhea {
		table_version: 1,
		..hhea_as_dict()
	};
	let (logs, xml) = capture_logs(|| get_xml(&hhea, &mut font));
	assert_eq!(xml?, HHEA_XML);
	assert_eq!(
		count_logs(&logs, log::Level::Warn, "Table version value is a float"),
		1
	);
	Ok(())
}

#[test]
fn test_aliases() {
	let mut hhea = hhea_as_dict();
	assert_eq!(hhea.ascent, hhea.ascender());
	assert_eq!(hhea.descent, hhea.descender());
	hhea.ascent = 800;
	assert_eq!(hhea.ascender(), 800);
	hhea.descent = -299;
	assert_eq!(hhea.descender(), -299);
}

#[test]
fn test_decompile() -> TestResult {
	let mut font = otto_font();
	assert_eq!(Hhea::decompile(&hex(HHEA_DATA), &mut font)?, hhea_as_dict());
	Ok(())
}

#[test]
fn test_from_xml() -> TestResult {
	let mut font = otto_font();
	let mut hhea = Hhea::default();
	from_xml(&mut hhea, &HHEA_XML.concat(), &mut font)?;
	assert_eq!(hhea, hhea_as_dict());
	Ok(())
}

#[test]
fn test_from_xml_version_as_float() -> TestResult {
	let mut font = otto_font();
	let mut hhea = Hhea::default();
	let xml = [r#"<tableVersion value="1.0"/>"#]
		.into_iter()
		.chain(HHEA_XML[1..].iter().copied())
		.collect::<String>();
	let (logs, result) = capture_logs(|| from_xml(&mut hhea, &xml, &mut font));
	result?;
	assert_eq!(
		count_logs(&logs, log::Level::Warn, "Table version value is a float"),
		1
	);
	assert_eq!(hhea, hhea_as_dict());
	Ok(())
}

/// `font.importXML(path); hhea.recalc(font)`.
fn recalc(name: &str) -> Result<Hhea, Box<dyn std::error::Error>> {
	let mut font = Font::new();
	font.import_ttx(&std::fs::read_to_string(tables_data(name))?)?;
	let mut hhea = font.require::<Hhea>()?.clone();
	hhea.recalc(&mut font)?;
	Ok(hhea)
}

#[test]
fn test_recalc_ttf() -> TestResult {
	let hhea = recalc("_h_h_e_a_recalc_TTF.ttx")?;
	assert_eq!(hhea.advance_width_max, 600);
	assert_eq!(hhea.min_left_side_bearing, -56);
	assert_eq!(hhea.min_right_side_bearing, 100);
	assert_eq!(hhea.x_max_extent, 400);
	Ok(())
}

#[test]
fn test_recalc_otf() -> TestResult {
	let hhea = recalc("_h_h_e_a_recalc_OTF.ttx")?;
	assert_eq!(hhea.advance_width_max, 600);
	assert_eq!(hhea.min_left_side_bearing, -56);
	assert_eq!(hhea.min_right_side_bearing, 100);
	assert_eq!(hhea.x_max_extent, 400);
	Ok(())
}

#[test]
fn test_recalc_empty() -> TestResult {
	let hhea = recalc("_h_h_e_a_recalc_empty.ttx")?;
	assert_eq!(hhea.advance_width_max, 600);
	assert_eq!(hhea.min_left_side_bearing, 0);
	assert_eq!(hhea.min_right_side_bearing, 0);
	assert_eq!(hhea.x_max_extent, 0);
	Ok(())
}
