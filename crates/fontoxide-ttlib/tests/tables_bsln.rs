//! `Tests/ttLib/tables/_b_s_l_n_test.py`.

mod common;

use common::{TestResult, compile_from_xml, decompile_to_xml, hex, hex_str};
use fontoxide_ttlib::Tag;

const TAG: Tag = Tag::new(b"bsln");

const GLYPHS: &[&str] = &[
	".notdef", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q",
	"R", "S", "T", "U", "V", "W", "X", "Y", "Z",
];

const BSLN_FORMAT_0_DATA: &str = concat!(
	"0001000000000000000001d10000054101fb0000000000000000000000000000",
	"0000000000000000000000000000000000000000000000000000000000000000",
	"0000000000000000",
);

const BSLN_FORMAT_0_XML: &[&str] = &[
	"<Version value=\"0x00010000\"/>",
	"<Baseline Format=\"0\">",
	"  <DefaultBaseline value=\"0\"/>",
	"  <Delta index=\"0\" value=\"0\"/>",
	"  <Delta index=\"1\" value=\"465\"/>",
	"  <Delta index=\"2\" value=\"0\"/>",
	"  <Delta index=\"3\" value=\"1345\"/>",
	"  <Delta index=\"4\" value=\"507\"/>",
	"  <Delta index=\"5\" value=\"0\"/>",
	"  <Delta index=\"6\" value=\"0\"/>",
	"  <Delta index=\"7\" value=\"0\"/>",
	"  <Delta index=\"8\" value=\"0\"/>",
	"  <Delta index=\"9\" value=\"0\"/>",
	"  <Delta index=\"10\" value=\"0\"/>",
	"  <Delta index=\"11\" value=\"0\"/>",
	"  <Delta index=\"12\" value=\"0\"/>",
	"  <Delta index=\"13\" value=\"0\"/>",
	"  <Delta index=\"14\" value=\"0\"/>",
	"  <Delta index=\"15\" value=\"0\"/>",
	"  <Delta index=\"16\" value=\"0\"/>",
	"  <Delta index=\"17\" value=\"0\"/>",
	"  <Delta index=\"18\" value=\"0\"/>",
	"  <Delta index=\"19\" value=\"0\"/>",
	"  <Delta index=\"20\" value=\"0\"/>",
	"  <Delta index=\"21\" value=\"0\"/>",
	"  <Delta index=\"22\" value=\"0\"/>",
	"  <Delta index=\"23\" value=\"0\"/>",
	"  <Delta index=\"24\" value=\"0\"/>",
	"  <Delta index=\"25\" value=\"0\"/>",
	"  <Delta index=\"26\" value=\"0\"/>",
	"  <Delta index=\"27\" value=\"0\"/>",
	"  <Delta index=\"28\" value=\"0\"/>",
	"  <Delta index=\"29\" value=\"0\"/>",
	"  <Delta index=\"30\" value=\"0\"/>",
	"  <Delta index=\"31\" value=\"0\"/>",
	"</Baseline>",
];

const BSLN_FORMAT_1_DATA: &str = concat!(
	"000100000001000100000357000005f000000000000000000000000000000000",
	"0000000000000000000000000000000000000000000000000000000000000000",
	"0000000000000000000200060001000600000000000a00020000ffffffff0000",
);

const BSLN_FORMAT_1_XML: &[&str] = &[
	"<Version value=\"0x00010000\"/>",
	"<Baseline Format=\"1\">",
	"  <DefaultBaseline value=\"1\"/>",
	"  <Delta index=\"0\" value=\"0\"/>",
	"  <Delta index=\"1\" value=\"855\"/>",
	"  <Delta index=\"2\" value=\"0\"/>",
	"  <Delta index=\"3\" value=\"1520\"/>",
	"  <Delta index=\"4\" value=\"0\"/>",
	"  <Delta index=\"5\" value=\"0\"/>",
	"  <Delta index=\"6\" value=\"0\"/>",
	"  <Delta index=\"7\" value=\"0\"/>",
	"  <Delta index=\"8\" value=\"0\"/>",
	"  <Delta index=\"9\" value=\"0\"/>",
	"  <Delta index=\"10\" value=\"0\"/>",
	"  <Delta index=\"11\" value=\"0\"/>",
	"  <Delta index=\"12\" value=\"0\"/>",
	"  <Delta index=\"13\" value=\"0\"/>",
	"  <Delta index=\"14\" value=\"0\"/>",
	"  <Delta index=\"15\" value=\"0\"/>",
	"  <Delta index=\"16\" value=\"0\"/>",
	"  <Delta index=\"17\" value=\"0\"/>",
	"  <Delta index=\"18\" value=\"0\"/>",
	"  <Delta index=\"19\" value=\"0\"/>",
	"  <Delta index=\"20\" value=\"0\"/>",
	"  <Delta index=\"21\" value=\"0\"/>",
	"  <Delta index=\"22\" value=\"0\"/>",
	"  <Delta index=\"23\" value=\"0\"/>",
	"  <Delta index=\"24\" value=\"0\"/>",
	"  <Delta index=\"25\" value=\"0\"/>",
	"  <Delta index=\"26\" value=\"0\"/>",
	"  <Delta index=\"27\" value=\"0\"/>",
	"  <Delta index=\"28\" value=\"0\"/>",
	"  <Delta index=\"29\" value=\"0\"/>",
	"  <Delta index=\"30\" value=\"0\"/>",
	"  <Delta index=\"31\" value=\"0\"/>",
	"  <BaselineValues>",
	"    <Lookup glyph=\"B\" value=\"0\"/>",
	"    <Lookup glyph=\"C\" value=\"0\"/>",
	"    <Lookup glyph=\"D\" value=\"0\"/>",
	"    <Lookup glyph=\"E\" value=\"0\"/>",
	"    <Lookup glyph=\"F\" value=\"0\"/>",
	"    <Lookup glyph=\"G\" value=\"0\"/>",
	"    <Lookup glyph=\"H\" value=\"0\"/>",
	"    <Lookup glyph=\"I\" value=\"0\"/>",
	"    <Lookup glyph=\"J\" value=\"0\"/>",
	"  </BaselineValues>",
	"</Baseline>",
];

const BSLN_FORMAT_2_DATA: &str = concat!(
	"0001000000020004001600500051ffff0052ffffffffffffffffffffffffffff",
	"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
	"ffffffffffffffffffff",
);

const BSLN_FORMAT_2_XML: &[&str] = &[
	"<Version value=\"0x00010000\"/>",
	"<Baseline Format=\"2\">",
	"  <DefaultBaseline value=\"4\"/>",
	"  <StandardGlyph value=\"V\"/>",
	"  <ControlPoint index=\"0\" value=\"80\"/>",
	"  <ControlPoint index=\"1\" value=\"81\"/>",
	"  <ControlPoint index=\"2\" value=\"65535\"/>",
	"  <ControlPoint index=\"3\" value=\"82\"/>",
	"  <ControlPoint index=\"4\" value=\"65535\"/>",
	"  <ControlPoint index=\"5\" value=\"65535\"/>",
	"  <ControlPoint index=\"6\" value=\"65535\"/>",
	"  <ControlPoint index=\"7\" value=\"65535\"/>",
	"  <ControlPoint index=\"8\" value=\"65535\"/>",
	"  <ControlPoint index=\"9\" value=\"65535\"/>",
	"  <ControlPoint index=\"10\" value=\"65535\"/>",
	"  <ControlPoint index=\"11\" value=\"65535\"/>",
	"  <ControlPoint index=\"12\" value=\"65535\"/>",
	"  <ControlPoint index=\"13\" value=\"65535\"/>",
	"  <ControlPoint index=\"14\" value=\"65535\"/>",
	"  <ControlPoint index=\"15\" value=\"65535\"/>",
	"  <ControlPoint index=\"16\" value=\"65535\"/>",
	"  <ControlPoint index=\"17\" value=\"65535\"/>",
	"  <ControlPoint index=\"18\" value=\"65535\"/>",
	"  <ControlPoint index=\"19\" value=\"65535\"/>",
	"  <ControlPoint index=\"20\" value=\"65535\"/>",
	"  <ControlPoint index=\"21\" value=\"65535\"/>",
	"  <ControlPoint index=\"22\" value=\"65535\"/>",
	"  <ControlPoint index=\"23\" value=\"65535\"/>",
	"  <ControlPoint index=\"24\" value=\"65535\"/>",
	"  <ControlPoint index=\"25\" value=\"65535\"/>",
	"  <ControlPoint index=\"26\" value=\"65535\"/>",
	"  <ControlPoint index=\"27\" value=\"65535\"/>",
	"  <ControlPoint index=\"28\" value=\"65535\"/>",
	"  <ControlPoint index=\"29\" value=\"65535\"/>",
	"  <ControlPoint index=\"30\" value=\"65535\"/>",
	"  <ControlPoint index=\"31\" value=\"65535\"/>",
	"</Baseline>",
];

const BSLN_FORMAT_3_DATA: &str = concat!(
	"0001000000030001001600500051ffff0052ffffffffffffffffffffffffffff",
	"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
	"ffffffffffffffffffff000200060001000600000000000a00020000ffffffff",
	"0000",
);

const BSLN_FORMAT_3_XML: &[&str] = &[
	"<Version value=\"0x00010000\"/>",
	"<Baseline Format=\"3\">",
	"  <DefaultBaseline value=\"1\"/>",
	"  <StandardGlyph value=\"V\"/>",
	"  <ControlPoint index=\"0\" value=\"80\"/>",
	"  <ControlPoint index=\"1\" value=\"81\"/>",
	"  <ControlPoint index=\"2\" value=\"65535\"/>",
	"  <ControlPoint index=\"3\" value=\"82\"/>",
	"  <ControlPoint index=\"4\" value=\"65535\"/>",
	"  <ControlPoint index=\"5\" value=\"65535\"/>",
	"  <ControlPoint index=\"6\" value=\"65535\"/>",
	"  <ControlPoint index=\"7\" value=\"65535\"/>",
	"  <ControlPoint index=\"8\" value=\"65535\"/>",
	"  <ControlPoint index=\"9\" value=\"65535\"/>",
	"  <ControlPoint index=\"10\" value=\"65535\"/>",
	"  <ControlPoint index=\"11\" value=\"65535\"/>",
	"  <ControlPoint index=\"12\" value=\"65535\"/>",
	"  <ControlPoint index=\"13\" value=\"65535\"/>",
	"  <ControlPoint index=\"14\" value=\"65535\"/>",
	"  <ControlPoint index=\"15\" value=\"65535\"/>",
	"  <ControlPoint index=\"16\" value=\"65535\"/>",
	"  <ControlPoint index=\"17\" value=\"65535\"/>",
	"  <ControlPoint index=\"18\" value=\"65535\"/>",
	"  <ControlPoint index=\"19\" value=\"65535\"/>",
	"  <ControlPoint index=\"20\" value=\"65535\"/>",
	"  <ControlPoint index=\"21\" value=\"65535\"/>",
	"  <ControlPoint index=\"22\" value=\"65535\"/>",
	"  <ControlPoint index=\"23\" value=\"65535\"/>",
	"  <ControlPoint index=\"24\" value=\"65535\"/>",
	"  <ControlPoint index=\"25\" value=\"65535\"/>",
	"  <ControlPoint index=\"26\" value=\"65535\"/>",
	"  <ControlPoint index=\"27\" value=\"65535\"/>",
	"  <ControlPoint index=\"28\" value=\"65535\"/>",
	"  <ControlPoint index=\"29\" value=\"65535\"/>",
	"  <ControlPoint index=\"30\" value=\"65535\"/>",
	"  <ControlPoint index=\"31\" value=\"65535\"/>",
	"  <BaselineValues>",
	"    <Lookup glyph=\"B\" value=\"0\"/>",
	"    <Lookup glyph=\"C\" value=\"0\"/>",
	"    <Lookup glyph=\"D\" value=\"0\"/>",
	"    <Lookup glyph=\"E\" value=\"0\"/>",
	"    <Lookup glyph=\"F\" value=\"0\"/>",
	"    <Lookup glyph=\"G\" value=\"0\"/>",
	"    <Lookup glyph=\"H\" value=\"0\"/>",
	"    <Lookup glyph=\"I\" value=\"0\"/>",
	"    <Lookup glyph=\"J\" value=\"0\"/>",
	"  </BaselineValues>",
	"</Baseline>",
];

#[test]
fn test_format0() -> TestResult {
	assert_eq!(
		decompile_to_xml(TAG, GLYPHS, &hex(BSLN_FORMAT_0_DATA))?,
		BSLN_FORMAT_0_XML
	);
	assert_eq!(
		hex_str(&compile_from_xml(TAG, GLYPHS, BSLN_FORMAT_0_XML)?),
		hex_str(&hex(BSLN_FORMAT_0_DATA))
	);
	Ok(())
}

#[test]
fn test_format1() -> TestResult {
	assert_eq!(
		decompile_to_xml(TAG, GLYPHS, &hex(BSLN_FORMAT_1_DATA))?,
		BSLN_FORMAT_1_XML
	);
	assert_eq!(
		hex_str(&compile_from_xml(TAG, GLYPHS, BSLN_FORMAT_1_XML)?),
		hex_str(&hex(BSLN_FORMAT_1_DATA))
	);
	Ok(())
}

#[test]
fn test_format2() -> TestResult {
	assert_eq!(
		decompile_to_xml(TAG, GLYPHS, &hex(BSLN_FORMAT_2_DATA))?,
		BSLN_FORMAT_2_XML
	);
	assert_eq!(
		hex_str(&compile_from_xml(TAG, GLYPHS, BSLN_FORMAT_2_XML)?),
		hex_str(&hex(BSLN_FORMAT_2_DATA))
	);
	Ok(())
}

#[test]
fn test_format3() -> TestResult {
	assert_eq!(
		decompile_to_xml(TAG, GLYPHS, &hex(BSLN_FORMAT_3_DATA))?,
		BSLN_FORMAT_3_XML
	);
	assert_eq!(
		hex_str(&compile_from_xml(TAG, GLYPHS, BSLN_FORMAT_3_XML)?),
		hex_str(&hex(BSLN_FORMAT_3_DATA))
	);
	Ok(())
}
