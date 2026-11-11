//! `Tests/ttLib/tables/S_T_A_T_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use fontoxide_ttlib::tables::Stat;
use fontoxide_ttlib::{Table, TableType};

const STAT_DATA: &str = concat!(
	"00010000000800020000001200030000002277676874012d000254455354012e00010006001200260001000000000191",
	"01900000000200010000019200020000000100000003000000030000000000020190000002bc0000",
);

const STAT_XML: &[&str] = &[
	"<Version value=\"0x00010000\"/>",
	"<DesignAxisRecordSize value=\"8\"/>",
	"<!-- DesignAxisCount=2 -->",
	"<DesignAxisRecord>",
	"  <Axis index=\"0\">",
	"    <AxisTag value=\"wght\"/>",
	"    <AxisNameID value=\"301\"/>",
	"    <AxisOrdering value=\"2\"/>",
	"  </Axis>",
	"  <Axis index=\"1\">",
	"    <AxisTag value=\"TEST\"/>",
	"    <AxisNameID value=\"302\"/>",
	"    <AxisOrdering value=\"1\"/>",
	"  </Axis>",
	"</DesignAxisRecord>",
	"<!-- AxisValueCount=3 -->",
	"<AxisValueArray>",
	"  <AxisValue index=\"0\" Format=\"1\">",
	"    <AxisIndex value=\"0\"/>",
	"    <Flags value=\"0\"/>",
	"    <ValueNameID value=\"401\"/>",
	"    <Value value=\"400.0\"/>",
	"  </AxisValue>",
	"  <AxisValue index=\"1\" Format=\"2\">",
	"    <AxisIndex value=\"1\"/>",
	"    <Flags value=\"0\"/>",
	"    <ValueNameID value=\"402\"/>",
	"    <NominalValue value=\"2.0\"/>",
	"    <RangeMinValue value=\"1.0\"/>",
	"    <RangeMaxValue value=\"3.0\"/>",
	"  </AxisValue>",
	"  <AxisValue index=\"2\" Format=\"3\">",
	"    <AxisIndex value=\"0\"/>",
	"    <Flags value=\"0\"/>",
	"    <ValueNameID value=\"2\"/>",
	"    <Value value=\"400.0\"/>",
	"    <LinkedValue value=\"700.0\"/>",
	"  </AxisValue>",
	"</AxisValueArray>",
];

const STAT_DATA_WITH_AXIS_JUNK: &str =
	"00010000000a00020000001200000000000077676874012d0002dead54455354012e0001beef";

const STAT_XML_WITH_AXIS_JUNK: &[&str] = &[
	"<Version value=\"0x00010000\"/>",
	"<DesignAxisRecordSize value=\"10\"/>",
	"<!-- DesignAxisCount=2 -->",
	"<DesignAxisRecord>",
	"  <Axis index=\"0\">",
	"    <AxisTag value=\"wght\"/>",
	"    <AxisNameID value=\"301\"/>",
	"    <AxisOrdering value=\"2\"/>",
	"    <MoreBytes index=\"0\" value=\"222\"/>",
	"    <MoreBytes index=\"1\" value=\"173\"/>",
	"  </Axis>",
	"  <Axis index=\"1\">",
	"    <AxisTag value=\"TEST\"/>",
	"    <AxisNameID value=\"302\"/>",
	"    <AxisOrdering value=\"1\"/>",
	"    <MoreBytes index=\"0\" value=\"190\"/>",
	"    <MoreBytes index=\"1\" value=\"239\"/>",
	"  </Axis>",
	"</DesignAxisRecord>",
	"<!-- AxisValueCount=0 -->",
];

const STAT_DATA_AXIS_VALUE_FORMAT3: &str =
	"00010000000800010000001200010000001a7767687401020000000200030000000200020190000002bc0000";

const STAT_XML_AXIS_VALUE_FORMAT3: &[&str] = &[
	"<Version value=\"0x00010000\"/>",
	"<DesignAxisRecordSize value=\"8\"/>",
	"<!-- DesignAxisCount=1 -->",
	"<DesignAxisRecord>",
	"  <Axis index=\"0\">",
	"    <AxisTag value=\"wght\"/>",
	"    <AxisNameID value=\"258\"/>",
	"    <AxisOrdering value=\"0\"/>",
	"  </Axis>",
	"</DesignAxisRecord>",
	"<!-- AxisValueCount=1 -->",
	"<AxisValueArray>",
	"  <AxisValue index=\"0\" Format=\"3\">",
	"    <AxisIndex value=\"0\"/>",
	"    <Flags value=\"2\"/>  <!-- ElidableAxisValueName -->",
	"    <ValueNameID value=\"2\"/>",
	"    <Value value=\"400.0\"/>",
	"    <LinkedValue value=\"700.0\"/>",
	"  </AxisValue>",
	"</AxisValueArray>",
];

const STAT_DATA_VERSION_1_1: &str =
	"00010001000800010000001400010000001c01017767687401020000000200030000000200020190000002bc0000";

const STAT_XML_VERSION_1_1: &[&str] = &[
	"<Version value=\"0x00010001\"/>",
	"<DesignAxisRecordSize value=\"8\"/>",
	"<!-- DesignAxisCount=1 -->",
	"<DesignAxisRecord>",
	"  <Axis index=\"0\">",
	"    <AxisTag value=\"wght\"/>",
	"    <AxisNameID value=\"258\"/>",
	"    <AxisOrdering value=\"0\"/>",
	"  </Axis>",
	"</DesignAxisRecord>",
	"<!-- AxisValueCount=1 -->",
	"<AxisValueArray>",
	"  <AxisValue index=\"0\" Format=\"3\">",
	"    <AxisIndex value=\"0\"/>",
	"    <Flags value=\"2\"/>  <!-- ElidableAxisValueName -->",
	"    <ValueNameID value=\"2\"/>",
	"    <Value value=\"400.0\"/>",
	"    <LinkedValue value=\"700.0\"/>",
	"  </AxisValue>",
	"</AxisValueArray>",
	"<ElidedFallbackNameID value=\"257\"/>",
];

fn decompile_to_xml(data: &str, xml: &[&str]) -> TestResult {
	let mut font = fake_font(&[".notdef"]);
	let table = Stat::decompile(&hex(data), &mut font)?;
	assert_eq!(get_xml(&table, &mut font)?, xml);
	Ok(())
}

fn compile_from_xml(xml: &[&str], data: &str) -> TestResult {
	let mut font = fake_font(&[".notdef"]);
	let mut table = Stat::default();
	from_xml(&mut table, &xml.concat(), &mut font)?;
	assert_eq!(hex_str(&table.compile(&mut font)?), data);
	Ok(())
}

#[test]
fn test_decompile_to_xml() -> TestResult {
	decompile_to_xml(STAT_DATA, STAT_XML)
}

#[test]
fn test_decompile_to_xml_with_axis_junk() -> TestResult {
	decompile_to_xml(STAT_DATA_WITH_AXIS_JUNK, STAT_XML_WITH_AXIS_JUNK)
}

#[test]
fn test_decompile_to_xml_format3() -> TestResult {
	decompile_to_xml(STAT_DATA_AXIS_VALUE_FORMAT3, STAT_XML_AXIS_VALUE_FORMAT3)
}

#[test]
fn test_decompile_to_xml_version_1_1() -> TestResult {
	decompile_to_xml(STAT_DATA_VERSION_1_1, STAT_XML_VERSION_1_1)
}

#[test]
fn test_compile_from_xml() -> TestResult {
	compile_from_xml(STAT_XML, STAT_DATA)
}

#[test]
fn test_compile_from_xml_with_axis_junk() -> TestResult {
	compile_from_xml(STAT_XML_WITH_AXIS_JUNK, STAT_DATA_WITH_AXIS_JUNK)
}

#[test]
fn test_compile_from_xml_format3() -> TestResult {
	compile_from_xml(STAT_XML_AXIS_VALUE_FORMAT3, STAT_DATA_AXIS_VALUE_FORMAT3)
}

#[test]
fn test_compile_from_xml_version_1_1() -> TestResult {
	compile_from_xml(STAT_XML_VERSION_1_1, STAT_DATA_VERSION_1_1)
}
