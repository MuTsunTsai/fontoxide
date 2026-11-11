//! `Tests/ttLib/tables/_a_n_k_r_test.py`.

mod common;

use common::{TestResult, compile_from_xml, decompile_to_xml, hex, hex_str};
use fontoxide_ttlib::Tag;

const TAG: Tag = Tag::new(b"ankr");

const GLYPHS: &[&str] = &[".notdef", "A", "B", "C", "D"];

const ANKR_FORMAT_0_DATA: &str = concat!(
	"000000000000000c000000240006000400020008000100000001000000030008",
	"ffff0000000000010235045e00000001fed2045e",
);

const ANKR_FORMAT_0_XML: &[&str] = &[
	"<AnchorPoints Format=\"0\">",
	"  <Flags value=\"0\"/>",
	"  <Anchors>",
	"    <Lookup glyph=\"A\">",
	"      <!-- AnchorPointCount=1 -->",
	"      <AnchorPoint index=\"0\">",
	"        <XCoordinate value=\"565\"/>",
	"        <YCoordinate value=\"1118\"/>",
	"      </AnchorPoint>",
	"    </Lookup>",
	"    <Lookup glyph=\"C\">",
	"      <!-- AnchorPointCount=1 -->",
	"      <AnchorPoint index=\"0\">",
	"        <XCoordinate value=\"-302\"/>",
	"        <YCoordinate value=\"1118\"/>",
	"      </AnchorPoint>",
	"    </Lookup>",
	"  </Anchors>",
	"</AnchorPoints>",
];

const ANKR_FORMAT_0_STRAY_DATA: &str = concat!(
	"000000000000001800000034deadbeefcafedeadbeefcafe0006000400020008",
	"000100000001000000030008ffff0000beeff00d000000010235045e00000001",
	"fed2045e",
);

const ANKR_FORMAT_0_SHARING_DATA: &str = concat!(
	"000000000000000c000000280006000400030008000100040001000000030008",
	"00040000ffff0000000000010235045e00000002000b000c001b001c",
);

const ANKR_FORMAT_0_SHARING_XML: &[&str] = &[
	"<AnchorPoints Format=\"0\">",
	"  <Flags value=\"0\"/>",
	"  <Anchors>",
	"    <Lookup glyph=\"A\">",
	"      <!-- AnchorPointCount=1 -->",
	"      <AnchorPoint index=\"0\">",
	"        <XCoordinate value=\"565\"/>",
	"        <YCoordinate value=\"1118\"/>",
	"      </AnchorPoint>",
	"    </Lookup>",
	"    <Lookup glyph=\"C\">",
	"      <!-- AnchorPointCount=2 -->",
	"      <AnchorPoint index=\"0\">",
	"        <XCoordinate value=\"11\"/>",
	"        <YCoordinate value=\"12\"/>",
	"      </AnchorPoint>",
	"      <AnchorPoint index=\"1\">",
	"        <XCoordinate value=\"27\"/>",
	"        <YCoordinate value=\"28\"/>",
	"      </AnchorPoint>",
	"    </Lookup>",
	"    <Lookup glyph=\"D\">",
	"      <!-- AnchorPointCount=1 -->",
	"      <AnchorPoint index=\"0\">",
	"        <XCoordinate value=\"565\"/>",
	"        <YCoordinate value=\"1118\"/>",
	"      </AnchorPoint>",
	"    </Lookup>",
	"  </Anchors>",
	"</AnchorPoints>",
];

#[test]
fn test_format0() -> TestResult {
	assert_eq!(
		decompile_to_xml(TAG, GLYPHS, &hex(ANKR_FORMAT_0_DATA))?,
		ANKR_FORMAT_0_XML
	);
	assert_eq!(
		hex_str(&compile_from_xml(TAG, GLYPHS, ANKR_FORMAT_0_XML)?),
		hex_str(&hex(ANKR_FORMAT_0_DATA))
	);
	Ok(())
}

#[test]
fn test_format0_stray() -> TestResult {
	assert_eq!(
		decompile_to_xml(TAG, GLYPHS, &hex(ANKR_FORMAT_0_STRAY_DATA))?,
		ANKR_FORMAT_0_XML
	);
	Ok(())
}

#[test]
fn test_format0_sharing() -> TestResult {
	assert_eq!(
		decompile_to_xml(TAG, GLYPHS, &hex(ANKR_FORMAT_0_SHARING_DATA))?,
		ANKR_FORMAT_0_SHARING_XML
	);
	assert_eq!(
		hex_str(&compile_from_xml(TAG, GLYPHS, ANKR_FORMAT_0_SHARING_XML)?),
		hex_str(&hex(ANKR_FORMAT_0_SHARING_DATA))
	);
	Ok(())
}
