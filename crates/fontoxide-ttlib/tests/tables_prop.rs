//! `Tests/ttLib/tables/_p_r_o_p_test.py`.

mod common;

use common::{TestResult, compile_from_xml, decompile_to_xml, hex, hex_str};
use fontoxide_ttlib::Tag;

const TAG: Tag = Tag::new(b"prop");

const GLYPHS: &[&str] = &[".notdef", "A", "B", "C", "D", "E", "F", "G"];

const PROP_FORMAT_0_DATA: &str = "0001000000000005";

const PROP_FORMAT_0_XML: &[&str] = &[
	"<Version value=\"1.0\"/>",
	"<GlyphProperties Format=\"0\">",
	"  <DefaultProperties value=\"5\"/>",
	"</GlyphProperties>",
];

const PROP_FORMAT_1_DATA: &str = "0003000000010000000800030004000b000a600b0005";

const PROP_FORMAT_1_XML: &[&str] = &[
	"<Version value=\"3.0\"/>",
	"<GlyphProperties Format=\"1\">",
	"  <DefaultProperties value=\"0\"/>",
	"  <Properties>",
	"    <Lookup glyph=\"C\" value=\"11\"/>",
	"    <Lookup glyph=\"D\" value=\"10\"/>",
	"    <Lookup glyph=\"E\" value=\"24587\"/>",
	"    <Lookup glyph=\"F\" value=\"5\"/>",
	"  </Properties>",
	"</GlyphProperties>",
];

#[test]
fn test_decompile_to_xml_format0() -> TestResult {
	assert_eq!(
		decompile_to_xml(TAG, GLYPHS, &hex(PROP_FORMAT_0_DATA))?,
		PROP_FORMAT_0_XML
	);
	Ok(())
}

#[test]
fn test_decompile_to_xml_format1() -> TestResult {
	assert_eq!(
		decompile_to_xml(TAG, GLYPHS, &hex(PROP_FORMAT_1_DATA))?,
		PROP_FORMAT_1_XML
	);
	Ok(())
}

#[test]
fn test_compile_from_xml_format0() -> TestResult {
	assert_eq!(
		hex_str(&compile_from_xml(TAG, GLYPHS, PROP_FORMAT_0_XML)?),
		hex_str(&hex(PROP_FORMAT_0_DATA))
	);
	Ok(())
}

#[test]
fn test_compile_from_xml_format1() -> TestResult {
	assert_eq!(
		hex_str(&compile_from_xml(TAG, GLYPHS, PROP_FORMAT_1_XML)?),
		hex_str(&hex(PROP_FORMAT_1_DATA))
	);
	Ok(())
}
