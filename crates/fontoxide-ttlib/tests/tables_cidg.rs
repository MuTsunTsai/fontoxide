//! `Tests/ttLib/tables/_c_i_d_g_test.py`.

mod common;

use common::{TestResult, compile_from_xml, decompile_to_xml, hex, hex_str};
use fontoxide_ttlib::Tag;

const TAG: Tag = Tag::new(b"cidg");

const GLYPHS: &[&str] = &[".notdef", "A", "B", "C", "D"];

const CIDG_DATA: &str = concat!(
	"0000000000000098000041646f62650000000000000000000000000000000000",
	"0000000000000000000000000000000000000000000000000000000000000000",
	"000000000000000000000002434e533100000000000000000000000000000000",
	"0000000000000000000000000000000000000000000000000000000000000000",
	"000000000000000000000000000000040000ffff00030001",
);

const CIDG_XML: &[&str] = &[
	"<CIDGlyphMapping Format=\"0\">",
	"  <DataFormat value=\"0\"/>",
	"  <!-- StructLength=152 -->",
	"  <Registry value=\"0\"/>",
	"  <RegistryName value=\"Adobe\"/>",
	"  <Order value=\"2\"/>",
	"  <OrderName value=\"CNS1\"/>",
	"  <SupplementVersion value=\"0\"/>",
	"  <Mapping>",
	"    <CID cid=\"0\" glyph=\".notdef\"/>",
	"    <CID cid=\"2\" glyph=\"C\"/>",
	"    <CID cid=\"3\" glyph=\"A\"/>",
	"  </Mapping>",
	"</CIDGlyphMapping>",
];

#[test]
fn test_decompile_to_xml() -> TestResult {
	assert_eq!(decompile_to_xml(TAG, GLYPHS, &hex(CIDG_DATA))?, CIDG_XML);
	Ok(())
}

#[test]
fn test_compile_from_xml() -> TestResult {
	assert_eq!(
		hex_str(&compile_from_xml(TAG, GLYPHS, CIDG_XML)?),
		hex_str(&hex(CIDG_DATA))
	);
	Ok(())
}
