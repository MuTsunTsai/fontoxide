//! `Tests/ttLib/tables/_o_p_b_d_test.py`.

mod common;

use common::{TestResult, compile_from_xml, decompile_to_xml, hex, hex_str};
use fontoxide_ttlib::Tag;

const TAG: Tag = Tag::new(b"opbd");

/// The upstream glyph names are not formatted: all but two are `X.alt%d`.
const GLYPHS: &[&str] = &[
	".notdef", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d",
	"X.alt%d", "X.alt%d", "C", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d",
	"X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d",
	"X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d",
	"X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d",
	"X.alt%d", "X.alt%d", "A", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d", "X.alt%d",
];

const OPBD_FORMAT_0_DATA: &str = concat!(
	"000100000000000600040002000800010000000a001e002b0026ffff0000ffce",
	"00050037fffbfff6000f00000000",
);

const OPBD_FORMAT_0_XML: &[&str] = &[
	"<Version value=\"0x00010000\"/>",
	"<OpticalBounds Format=\"0\">",
	"  <OpticalBoundsDeltas>",
	"    <Lookup glyph=\"A\">",
	"      <Left value=\"-10\"/>",
	"      <Top value=\"15\"/>",
	"      <Right value=\"0\"/>",
	"      <Bottom value=\"0\"/>",
	"    </Lookup>",
	"    <Lookup glyph=\"C\">",
	"      <Left value=\"-50\"/>",
	"      <Top value=\"5\"/>",
	"      <Right value=\"55\"/>",
	"      <Bottom value=\"-5\"/>",
	"    </Lookup>",
	"  </OpticalBoundsDeltas>",
	"</OpticalBounds>",
];

const OPBD_FORMAT_1_DATA: &str = concat!(
	"000100000001000600040002000800010000000a001e002b0026ffff00000024",
	"00250026002700200029ffffffff",
);

const OPBD_FORMAT_1_XML: &[&str] = &[
	"<Version value=\"0x00010000\"/>",
	"<OpticalBounds Format=\"1\">",
	"  <OpticalBoundsPoints>",
	"    <Lookup glyph=\"A\">",
	"      <Left value=\"32\"/>",
	"      <Top value=\"41\"/>",
	"      <Right value=\"-1\"/>",
	"      <Bottom value=\"-1\"/>",
	"    </Lookup>",
	"    <Lookup glyph=\"C\">",
	"      <Left value=\"36\"/>",
	"      <Top value=\"37\"/>",
	"      <Right value=\"38\"/>",
	"      <Bottom value=\"39\"/>",
	"    </Lookup>",
	"  </OpticalBoundsPoints>",
	"</OpticalBounds>",
];

const OPBD_APPLE_CHANCERY_DATA: &str = concat!(
	"000100000000000400060011006000040006017d017d00720183018000740186",
	"0185007c018f018b008001a00196008a01a501a300a001aa01aa00a601ac01ac",
	"00a801fb01f100aa0214020900c0021d021600d80222022000e80227022500ee",
	"0229022900f4023b023b00f6023e023e00f8ffffffff00fa0100010801100118",
	"0120012801300138014001480150015801600168017001780180018801900198",
	"01a001a801b001b801c001c801d001d801e001e801f001f80200020802100218",
	"0220022802300238024002480250025802600268027002780280028802900298",
	"02a002a802b002b802c002c802d002d802e002e802f002f80300030803100318",
	"fd98000000000000fdbc000000000000fdbc000000000000fdbf000000000000",
	"fdbc000000000000fd98000000000000fda9000000000000fd98000000000000",
	"fd98000000000000fd9800000000000000000000020500000000000002050000",
	"0000000002a4000000000000027e00000000000002f400000000000002a40000",
	"00000000036500000000000002910000000000000291000000000000026a0000",
	"0000000002b800000000000002cb00000000000002a400000000000001a90000",
	"00000000024400000000000002a400000000000002cb00000000000002440000",
	"0000000003070000000000000307000000000000037f00000000000003070000",
	"0000000003070000000000000307000000000000030700000000000003070000",
	"0000000003e3000000000000030c00000000000003070000fe30000000000000",
	"fe7e000000000000fe91000000000000fe6a000000000000fe6a000000000000",
	"fecb000000000000fe6a000000000000fe7e000000000000fea4000000000000",
	"fe7e000000000000fe44000000000000fea4000000000000feb8000000000000",
	"fe7e000000000000fe5e000000000000fe37000000000000fe37000000000000",
	"fcbd000000000000fd84000000000000fd98000000000000fd82000000000000",
	"fcbd000000000000fd84000000000000fcbd000000000000fcbd000000000000",
	"fe72000000000000ff9d00000000000000000000032f00000000000003ba0000",
);

#[test]
fn test_decompile_to_xml_format0() -> TestResult {
	assert_eq!(
		decompile_to_xml(TAG, GLYPHS, &hex(OPBD_FORMAT_0_DATA))?,
		OPBD_FORMAT_0_XML
	);
	Ok(())
}

#[test]
fn test_decompile_to_xml_format1() -> TestResult {
	assert_eq!(
		decompile_to_xml(TAG, GLYPHS, &hex(OPBD_FORMAT_1_DATA))?,
		OPBD_FORMAT_1_XML
	);
	Ok(())
}

#[test]
fn test_compile_from_xml_format0() -> TestResult {
	assert_eq!(
		hex_str(&compile_from_xml(TAG, GLYPHS, OPBD_FORMAT_0_XML)?),
		hex_str(&hex(OPBD_FORMAT_0_DATA))
	);
	Ok(())
}

#[test]
fn test_compile_from_xml_format1() -> TestResult {
	assert_eq!(
		hex_str(&compile_from_xml(TAG, GLYPHS, OPBD_FORMAT_1_XML)?),
		hex_str(&hex(OPBD_FORMAT_1_DATA))
	);
	Ok(())
}

/// The 'opbd' table of AppleChancery.ttf does not crash
/// (https://github.com/fonttools/fonttools/issues/1031).
#[test]
fn test_decompile_apple_chancery() -> TestResult {
	let xml = decompile_to_xml(TAG, GLYPHS, &hex(OPBD_APPLE_CHANCERY_DATA))?;
	assert!(xml.iter().any(|l| l == "<OpticalBounds Format=\"0\">"));
	Ok(())
}
