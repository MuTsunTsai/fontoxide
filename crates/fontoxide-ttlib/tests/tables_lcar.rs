//! `Tests/ttLib/tables/_l_c_a_r_test.py`.

mod common;

use common::{TestResult, compile_from_xml, decompile_to_xml, hex, hex_str};
use fontoxide_ttlib::Tag;

const TAG: Tag = Tag::new(b"lcar");

const GLYPHS: &[&str] = &[".notdef", "f_r", "X", "f_f_l"];

const LCAR_FORMAT_0_DATA: &str = concat!(
	"0001000000000006000400020008000100000001001e00030022ffff00000001",
	"00dc000200ef01d8",
);

const LCAR_FORMAT_0_XML: &[&str] = &[
	"<Version value=\"0x00010000\"/>",
	"<LigatureCarets Format=\"0\">",
	"  <Carets>",
	"    <Lookup glyph=\"f_f_l\">",
	"      <!-- DivsionPointCount=2 -->",
	"      <DivisionPoint index=\"0\" value=\"239\"/>",
	"      <DivisionPoint index=\"1\" value=\"472\"/>",
	"    </Lookup>",
	"    <Lookup glyph=\"f_r\">",
	"      <!-- DivsionPointCount=1 -->",
	"      <DivisionPoint index=\"0\" value=\"220\"/>",
	"    </Lookup>",
	"  </Carets>",
	"</LigatureCarets>",
];

const LCAR_FORMAT_1_DATA: &str = concat!(
	"0001000000010006000400020008000100000001001e00030022ffff00000001",
	"003200020037004b",
);

const LCAR_FORMAT_1_XML: &[&str] = &[
	"<Version value=\"0x00010000\"/>",
	"<LigatureCarets Format=\"1\">",
	"  <Carets>",
	"    <Lookup glyph=\"f_f_l\">",
	"      <!-- DivsionPointCount=2 -->",
	"      <DivisionPoint index=\"0\" value=\"55\"/>",
	"      <DivisionPoint index=\"1\" value=\"75\"/>",
	"    </Lookup>",
	"    <Lookup glyph=\"f_r\">",
	"      <!-- DivsionPointCount=1 -->",
	"      <DivisionPoint index=\"0\" value=\"50\"/>",
	"    </Lookup>",
	"  </Carets>",
	"</LigatureCarets>",
];

#[test]
fn test_decompile_to_xml_format0() -> TestResult {
	assert_eq!(
		decompile_to_xml(TAG, GLYPHS, &hex(LCAR_FORMAT_0_DATA))?,
		LCAR_FORMAT_0_XML
	);
	Ok(())
}

#[test]
fn test_decompile_to_xml_format1() -> TestResult {
	assert_eq!(
		decompile_to_xml(TAG, GLYPHS, &hex(LCAR_FORMAT_1_DATA))?,
		LCAR_FORMAT_1_XML
	);
	Ok(())
}

#[test]
fn test_compile_from_xml_format0() -> TestResult {
	assert_eq!(
		hex_str(&compile_from_xml(TAG, GLYPHS, LCAR_FORMAT_0_XML)?),
		hex_str(&hex(LCAR_FORMAT_0_DATA))
	);
	Ok(())
}

#[test]
fn test_compile_from_xml_format1() -> TestResult {
	assert_eq!(
		hex_str(&compile_from_xml(TAG, GLYPHS, LCAR_FORMAT_1_XML)?),
		hex_str(&hex(LCAR_FORMAT_1_DATA))
	);
	Ok(())
}
