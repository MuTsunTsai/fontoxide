//! `Tests/ttLib/tables/_g_c_i_d_test.py`.

mod common;

use common::{TestResult, compile_from_xml, decompile_to_xml, hex, hex_str};
use fontoxide_ttlib::Tag;

const TAG: Tag = Tag::new(b"gcid");

const GLYPHS: &[&str] = &[".notdef", "A", "B", "C", "D"];

const GCID_DATA: &str = concat!(
	"0000000000000098000041646f62650000000000000000000000000000000000",
	"0000000000000000000000000000000000000000000000000000000000000000",
	"0000000000000000000000034b6f726561310000000000000000000000000000",
	"0000000000000000000000000000000000000000000000000000000000000000",
	"000000000000000000000000000100041234ffff0007def0",
);

const GCID_XML: &[&str] = &[
	"<GlyphCIDMapping Format=\"0\">",
	"  <DataFormat value=\"0\"/>",
	"  <!-- StructLength=152 -->",
	"  <Registry value=\"0\"/>",
	"  <RegistryName value=\"Adobe\"/>",
	"  <Order value=\"3\"/>",
	"  <OrderName value=\"Korea1\"/>",
	"  <SupplementVersion value=\"1\"/>",
	"  <Mapping>",
	"    <CID glyph=\".notdef\" value=\"4660\"/>",
	"    <CID glyph=\"B\" value=\"7\"/>",
	"    <CID glyph=\"C\" value=\"57072\"/>",
	"  </Mapping>",
	"</GlyphCIDMapping>",
];

#[test]
fn test_decompile_to_xml() -> TestResult {
	assert_eq!(decompile_to_xml(TAG, GLYPHS, &hex(GCID_DATA))?, GCID_XML);
	Ok(())
}

#[test]
fn test_compile_from_xml() -> TestResult {
	assert_eq!(
		hex_str(&compile_from_xml(TAG, GLYPHS, GCID_XML)?),
		hex_str(&hex(GCID_DATA))
	);
	Ok(())
}
