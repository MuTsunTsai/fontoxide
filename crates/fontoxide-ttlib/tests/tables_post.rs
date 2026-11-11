//! `Tests/ttLib/tables/_p_o_s_t_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use fontoxide_ttlib::tables::{Maxp, Post};
use fontoxide_ttlib::{Font, TableType};

const POST_FORMAT_2_0_DATA: &str = "0002 0000 0000 0000 FF88 0028 0000 0000 0000 0000 0000 0000 0000 0000 0000 0000 \
	0005 0000 0102 0103 0104 0105 025859 025859 025859 0458592E32";

const POST_FORMAT_2_0_XML: [&str; 27] = [
	r#"<formatType value="2.0"/>"#,
	r#"<italicAngle value="0.0"/>"#,
	r#"<underlinePosition value="-120"/>"#,
	r#"<underlineThickness value="40"/>"#,
	r#"<isFixedPitch value="0"/>"#,
	r#"<minMemType42 value="0"/>"#,
	r#"<maxMemType42 value="0"/>"#,
	r#"<minMemType1 value="0"/>"#,
	r#"<maxMemType1 value="0"/>"#,
	"<psNames>",
	"  <!-- This file uses unique glyph names based on the information",
	"       found in the 'post' table. Since these names might not be unique,",
	"       we have to invent artificial names in case of clashes. In order to",
	"       be able to retain the original information, we need a name to",
	"       ps name mapping for those cases where they differ. That's what",
	"       you see below.",
	"        -->",
	r#"  <psName name="XY.1" psName="XY"/>"#,
	r#"  <psName name="XY.3" psName="XY"/>"#,
	"</psNames>",
	"<extraNames>",
	"  <!-- following are the name that are not taken from the standard Mac glyph order -->",
	r#"  <psName name="XY"/>"#,
	r#"  <psName name="XY"/>"#,
	r#"  <psName name="XY"/>"#,
	r#"  <psName name="XY.2"/>"#,
	"</extraNames>",
];

/// `font["maxp"] = FakeMaxp(n)`.
fn with_maxp(mut font: Font, num_glyphs: u32) -> Font {
	font.insert(Box::new(Maxp {
		num_glyphs,
		..Default::default()
	}));
	font
}

#[test]
fn test_no_names() -> TestResult {
	let mut font = with_maxp(fake_font(&[".notdef", "XY", "XY", "XY", "XY.2"]), 5);
	let table = Post::decompile(&hex(POST_FORMAT_2_0_DATA), &mut font)?;
	assert_eq!(get_xml(&table, &mut font)?, POST_FORMAT_2_0_XML);
	Ok(())
}

#[test]
fn test_zero_num_glyphs() -> TestResult {
	// post.numGlyphs should match maxp.numGlyphs, but some broken fonts set it to
	// zero; https://github.com/fonttools/fonttools/issues/3060
	let mut data = hex(POST_FORMAT_2_0_DATA)[..32].to_vec();
	data.extend(hex("0000"));
	// The glyph order comes from the table, as `table.getGlyphOrder()`.
	let mut font = with_maxp(Font::new(), 3);
	let table = Post::decompile(&data, &mut font)?;
	assert_eq!(table.extra_names.as_deref(), Some(&[][..]));
	font.insert(Box::new(table));
	assert_eq!(
		font.glyph_order()?.names().to_vec(),
		names(&["glyph00000", "glyph00001", "glyph00002"])
	);
	Ok(())
}
