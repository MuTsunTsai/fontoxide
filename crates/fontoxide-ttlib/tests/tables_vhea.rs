//! `Tests/ttLib/tables/_v_h_e_a_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use fontoxide_ttlib::tables::Vhea;
use fontoxide_ttlib::{Font, Table, TableType};

const VHEA_DATA_VERSION_11: &str =
	"0001 1000 01F4 FE0C 0000 0BB8 FC16 FD5B 0B70 0000 0001 0000 0000 0000 0000 0000 0000 000C";

fn vhea_data_version_10() -> Vec<u8> {
	let mut data = hex("00010000");
	data.extend_from_slice(&hex(VHEA_DATA_VERSION_11)[4..]);
	data
}

const VHEA_XML_VERSION_11: [&str; 17] = [
	r#"<tableVersion value="0x00011000"/>"#,
	r#"<ascent value="500"/>"#,
	r#"<descent value="-500"/>"#,
	r#"<lineGap value="0"/>"#,
	r#"<advanceHeightMax value="3000"/>"#,
	r#"<minTopSideBearing value="-1002"/>"#,
	r#"<minBottomSideBearing value="-677"/>"#,
	r#"<yMaxExtent value="2928"/>"#,
	r#"<caretSlopeRise value="0"/>"#,
	r#"<caretSlopeRun value="1"/>"#,
	r#"<caretOffset value="0"/>"#,
	r#"<reserved1 value="0"/>"#,
	r#"<reserved2 value="0"/>"#,
	r#"<reserved3 value="0"/>"#,
	r#"<reserved4 value="0"/>"#,
	r#"<metricDataFormat value="0"/>"#,
	r#"<numberOfVMetrics value="12"/>"#,
];

/// `VHEA_XML_VERSION_11` with another `tableVersion` element.
fn xml_with_version(version: &str) -> Vec<String> {
	let mut lines = vec![format!(r#"<tableVersion value="{version}"/>"#)];
	lines.extend(VHEA_XML_VERSION_11[1..].iter().map(|s| (*s).to_owned()));
	lines
}

/// `VHEA_VERSION_11_AS_DICT`.
fn vhea_version_11() -> Vhea {
	Vhea {
		table_version: 0x00011000,
		ascent: 500,
		descent: -500,
		line_gap: 0,
		advance_height_max: 3000,
		min_top_side_bearing: -1002,
		min_bottom_side_bearing: -677,
		y_max_extent: 2928,
		caret_slope_rise: 0,
		caret_slope_run: 1,
		caret_offset: 0,
		metric_data_format: 0,
		number_of_v_metrics: 12,
		..Default::default()
	}
}

/// `VHEA_VERSION_10_AS_DICT`, which is also the table of
/// `VheaCompileOrToXMLTest.setUp`.
fn vhea_version_10() -> Vhea {
	Vhea {
		table_version: 0x00010000,
		..vhea_version_11()
	}
}

fn otto_font() -> Font {
	let mut font = Font::new();
	font.sfnt_version = fontoxide_ttlib::Tag::new(b"OTTO");
	font
}

const FLOAT_WARNING: &str = "Table version value is a float";

#[test]
fn test_compile_version_10() -> TestResult {
	assert_eq!(
		vhea_version_10().compile(&mut otto_font())?,
		vhea_data_version_10()
	);
	Ok(())
}

#[test]
fn test_compile_version_10_as_float() -> TestResult {
	// `tableVersion = 1.0`: versions below 0x10000 are taken as floats.
	let mut vhea = Vhea {
		table_version: 1,
		..vhea_version_10()
	};
	let mut font = otto_font();
	let (logs, data) = capture_logs(|| vhea.compile(&mut font));
	assert_eq!(data?, vhea_data_version_10());
	assert_eq!(count_logs(&logs, log::Level::Warn, FLOAT_WARNING), 1);
	Ok(())
}

#[test]
fn test_compile_version_11() -> TestResult {
	assert_eq!(
		vhea_version_11().compile(&mut otto_font())?,
		hex(VHEA_DATA_VERSION_11)
	);
	Ok(())
}

#[test]
fn test_to_xml_version_10() -> TestResult {
	assert_eq!(
		get_xml(&vhea_version_10(), &mut otto_font())?,
		xml_with_version("0x00010000")
	);
	Ok(())
}

#[test]
fn test_to_xml_version_10_as_float() -> TestResult {
	let vhea = Vhea {
		table_version: 1,
		..vhea_version_10()
	};
	let mut font = otto_font();
	let (logs, xml) = capture_logs(|| get_xml(&vhea, &mut font));
	assert_eq!(xml?, xml_with_version("0x00010000"));
	assert_eq!(count_logs(&logs, log::Level::Warn, FLOAT_WARNING), 1);
	Ok(())
}

#[test]
fn test_to_xml_version_11() -> TestResult {
	assert_eq!(
		get_xml(&vhea_version_11(), &mut otto_font())?,
		VHEA_XML_VERSION_11
	);
	Ok(())
}

#[test]
fn test_decompile_version_10() -> TestResult {
	assert_eq!(
		Vhea::decompile(&vhea_data_version_10(), &mut otto_font())?,
		vhea_version_10()
	);
	Ok(())
}

#[test]
fn test_decompile_version_11() -> TestResult {
	assert_eq!(
		Vhea::decompile(&hex(VHEA_DATA_VERSION_11), &mut otto_font())?,
		vhea_version_11()
	);
	Ok(())
}

/// Reads the XML lines into a new table, with the number of float warnings.
fn read_xml(lines: &[String]) -> Result<(Vhea, usize), Box<dyn std::error::Error>> {
	let mut font = otto_font();
	let mut vhea = Vhea::default();
	let (logs, result) = capture_logs(|| from_xml(&mut vhea, &lines.concat(), &mut font));
	result?;
	Ok((vhea, count_logs(&logs, log::Level::Warn, FLOAT_WARNING)))
}

#[test]
fn test_from_xml_version_10() -> TestResult {
	let (vhea, _) = read_xml(&xml_with_version("0x00010000"))?;
	assert_eq!(vhea, vhea_version_10());
	Ok(())
}

#[test]
fn test_from_xml_version_10_as_float() -> TestResult {
	let (vhea, warnings) = read_xml(&xml_with_version("1.0"))?;
	assert_eq!(warnings, 1);
	assert_eq!(vhea, vhea_version_10());
	Ok(())
}

#[test]
fn test_from_xml_version_11() -> TestResult {
	let (vhea, _) = read_xml(&xml_with_version("0x00011000"))?;
	assert_eq!(vhea, vhea_version_11());
	Ok(())
}

#[test]
fn test_from_xml_version_11_as_float() -> TestResult {
	let (vhea, warnings) = read_xml(&xml_with_version("1.0625"))?;
	assert_eq!(warnings, 1);
	assert_eq!(vhea, vhea_version_11());
	Ok(())
}

/// The legacy name of `caretOffset` is accepted when reading TTX (the behaviour
/// behind `test_compile_caretOffset_as_reserved0`).
#[test]
fn test_from_xml_caret_offset_as_reserved0() -> TestResult {
	let mut lines = xml_with_version("0x00010000");
	lines[10] = r#"<reserved0 value="7"/>"#.to_owned();
	let (vhea, _) = read_xml(&lines)?;
	assert_eq!(
		vhea,
		Vhea {
			caret_offset: 7,
			..vhea_version_10()
		}
	);
	Ok(())
}

fn recalc(name: &str) -> Result<Vhea, Box<dyn std::error::Error>> {
	let mut font = Font::new();
	font.import_ttx(&std::fs::read_to_string(tables_data(name))?)?;
	let mut vhea = font.require::<Vhea>()?.clone();
	vhea.recalc(&mut font)?;
	Ok(vhea)
}

#[test]
fn test_recalc_ttf() -> TestResult {
	let vhea = recalc("_v_h_e_a_recalc_TTF.ttx")?;
	assert_eq!(vhea.advance_height_max, 900);
	assert_eq!(vhea.min_top_side_bearing, 200);
	assert_eq!(vhea.min_bottom_side_bearing, 377);
	assert_eq!(vhea.y_max_extent, 312);
	Ok(())
}

#[test]
fn test_recalc_otf() -> TestResult {
	let vhea = recalc("_v_h_e_a_recalc_OTF.ttx")?;
	assert_eq!(vhea.advance_height_max, 900);
	assert_eq!(vhea.min_top_side_bearing, 200);
	assert_eq!(vhea.min_bottom_side_bearing, 377);
	assert_eq!(vhea.y_max_extent, 312);
	Ok(())
}

#[test]
fn test_recalc_empty() -> TestResult {
	let vhea = recalc("_v_h_e_a_recalc_empty.ttx")?;
	assert_eq!(vhea.advance_height_max, 900);
	assert_eq!(vhea.min_top_side_bearing, 0);
	assert_eq!(vhea.min_bottom_side_bearing, 0);
	assert_eq!(vhea.y_max_extent, 0);
	Ok(())
}
