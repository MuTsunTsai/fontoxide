//! `Tests/ttLib/tables/C_O_L_R_test.py`.
//!
//! The sample data of the upstream test (`COLR_V*_SAMPLE`, `COLR_V*_XML`) is
//! saved in `tests/data/colr` by `tools/reference/colr_fixtures.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::{TestResult, get_xml, hex, tables_data};
use fontoxide_ttlib::tables::Colr;
use fontoxide_ttlib::{Font, GlyphSet, Table, TableType};

fn fixture(name: &str) -> String {
	let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data/colr")
		.join(name);
	std::fs::read_to_string(path).unwrap()
}

/// A `*_SAMPLE`: one hex fragment and its description per line.
fn sample(name: &str) -> Vec<u8> {
	fixture(&format!("{name}.hex"))
		.lines()
		.flat_map(|line| hex(line.split_once(' ').map_or(line, |(h, _)| h)))
		.collect()
}

/// A `COLR_V*_XML` list.
fn xml(name: &str) -> Vec<String> {
	fixture(&format!("{name}.txt"))
		.lines()
		.map(str::to_owned)
		.collect()
}

/// The `font` fixture.
fn font() -> Font {
	let mut font = Font::new();
	font.set_glyph_order((0..30).map(|i| format!("glyph{i:05}")).collect());
	font
}

/// `parseXML(lines)` into a new table.
fn load_xml(lines: &[String], font: &mut Font) -> fontoxide_ttlib::Result<Colr> {
	let mut colr = Colr::default();
	common::from_xml(&mut colr, &lines.join("\n"), font)?;
	Ok(colr)
}

fn decompile_and_compile(sample_name: &str) -> TestResult {
	let mut font = font();
	let data = sample(sample_name);
	let mut colr = Colr::decompile(&data, &mut font)?;
	assert_eq!(colr.compile(&mut font)?, data);
	Ok(())
}

fn decompile_and_dump_xml(sample_name: &str, xml_name: &str) -> TestResult {
	let mut font = font();
	let colr = Colr::decompile(&sample(sample_name), &mut font)?;
	assert_eq!(get_xml(&colr, &mut font)?, xml(xml_name));
	Ok(())
}

fn load_from_xml_and_compile(xml_name: &str, sample_name: &str) -> TestResult {
	let mut font = font();
	let mut colr = load_xml(&xml(xml_name), &mut font)?;
	assert_eq!(colr.compile(&mut font)?, sample(sample_name));
	Ok(())
}

fn round_trip_xml(lines: Vec<String>) -> TestResult {
	let mut font = font();
	let mut colr = load_xml(&lines, &mut font)?;
	let compiled = colr.compile(&mut font)?;
	let colr = Colr::decompile(&compiled, &mut font)?;
	assert_eq!(get_xml(&colr, &mut font)?, lines);
	Ok(())
}

/// `COLR_V0_Test::test_decompile_and_compile`.
#[test]
fn colr_v0_test_decompile_and_compile() -> TestResult {
	decompile_and_compile("COLR_V0_SAMPLE")
}

/// `COLR_V0_Test::test_decompile_and_dump_xml`.
#[test]
fn colr_v0_test_decompile_and_dump_xml() -> TestResult {
	decompile_and_dump_xml("COLR_V0_SAMPLE", "COLR_V0_XML")
}

/// `COLR_V0_Test::test_load_from_xml_and_compile`.
#[test]
fn colr_v0_test_load_from_xml_and_compile() -> TestResult {
	load_from_xml_and_compile("COLR_V0_XML", "COLR_V0_SAMPLE")
}

/// `COLR_V0_Test::test_round_trip_xml`.
#[test]
fn colr_v0_test_round_trip_xml() -> TestResult {
	round_trip_xml(xml("COLR_V0_XML"))
}

/// `COLR_V1_Test::test_decompile_and_compile`.
#[test]
fn colr_v1_test_decompile_and_compile() -> TestResult {
	decompile_and_compile("COLR_V1_SAMPLE")
}

/// `COLR_V1_Test::test_decompile_and_dump_xml`.
#[test]
fn colr_v1_test_decompile_and_dump_xml() -> TestResult {
	decompile_and_dump_xml("COLR_V1_SAMPLE", "COLR_V1_XML")
}

/// `COLR_V1_Test::test_load_from_xml_and_compile`.
#[test]
fn colr_v1_test_load_from_xml_and_compile() -> TestResult {
	load_from_xml_and_compile("COLR_V1_XML", "COLR_V1_SAMPLE")
}

/// `COLR_V1_Test::test_round_trip_xml`.
#[test]
fn colr_v1_test_round_trip_xml() -> TestResult {
	round_trip_xml(xml("COLR_V1_XML"))
}

/// `COLR_V1_Variable_Test::test_round_trip_xml`.
#[test]
fn colr_v1_variable_test_round_trip_xml() -> TestResult {
	let mut lines = xml("COLR_V1_XML");
	lines.extend(xml("COLR_V1_VAR_XML"));
	round_trip_xml(lines)
}

/// The `ClipList` of a font's `COLR`, as TTX.
fn clip_list_xml(font: &mut Font) -> fontoxide_ttlib::Result<Option<Vec<String>>> {
	let colr = font.require::<Colr>()?.clone();
	let Some(clip_list) = colr.table.as_ref().and_then(|t| t.clip_list.as_deref()) else {
		return Ok(None);
	};
	let mut writer = fontoxide_misc::xml_writer::XmlWriter::with_options("  ", "\n");
	fontoxide_ttlib::ot::table::xml_struct(&mut writer, font, Some(clip_list), None, &[])?;
	Ok(Some(common::writer_lines(writer)))
}

/// `COLR_V1_Test::test_computeClipBoxes`.
#[test]
fn colr_v1_test_compute_clip_boxes() -> TestResult {
	for flavor in ["glyf", "cff"] {
		for quantization in [1, 10, 100] {
			let ttx =
				std::fs::read_to_string(tables_data(&format!("COLRv1-clip-boxes-{flavor}.ttx")))?;
			let mut font = Font::from_ttx(&ttx)?;
			assert!(clip_list_xml(&mut font)?.is_none());

			let mut colr = font.require::<Colr>()?.clone();
			colr.compute_clip_boxes(&GlyphSet::new(&mut font)?, quantization)?;
			assert!(
				!colr
					.table
					.as_ref()
					.and_then(|t| t.clip_list.as_ref())
					.unwrap()
					.clips
					.is_empty()
			);
			font.insert(Box::new(colr));

			let expected_ttx = std::fs::read_to_string(tables_data(&format!(
				"COLRv1-clip-boxes-q{quantization}-expected.ttx"
			)))?;
			// fontoxide keys clip boxes by glyph ID: the expected `COLR` needs the
			// glyph order of the font.
			let mut expected = Font::new();
			expected.set_glyph_order(font.glyph_order()?.names().to_vec());
			expected.import_ttx(&expected_ttx)?;
			assert_eq!(
				clip_list_xml(&mut font)?,
				clip_list_xml(&mut expected)?,
				"{flavor} q{quantization}"
			);
		}
	}
	Ok(())
}
