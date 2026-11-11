//! `Tests/ttLib/tables/C_B_L_C_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::{TestResult, get_xml, hex, tables_data};
use fontoxide_ttlib::tables::Cblc;
use fontoxide_ttlib::{Font, ReadOptions, TableType, WriteOptions};

/// The `CBLC` of `NotoColorEmoji.subset.index_format_3.ttx` as compiled by
/// HarfBuzz' hb-subset, with the correct padding (base64 in the upstream test).
const CBLC_INDEX_FORMAT_3: &str = "0003000000000001000000380000002c000000020000000065e58800000000000000000065e588000000000000000000000100036d6d2001000100020000001000030003000000200003001100000004000003a6110d0000000300110000111100000852";

#[test]
fn test_compile_decompile_index_table_format_3() -> TestResult {
	let ttx = std::fs::read_to_string(tables_data("NotoColorEmoji.subset.index_format_3.ttx"))?;
	let mut font = Font::from_ttx(&ttx)?;
	let data = font.write(&WriteOptions::default())?;
	let mut font = Font::read(data, ReadOptions::default())?;

	let raw = font.reader().unwrap().table_data(Cblc::TAG)?;
	assert_eq!(&raw[..], &hex(CBLC_INDEX_FORMAT_3)[..]);

	let cblc = font.require::<Cblc>()?.clone();
	assert_eq!(
		get_xml(&cblc, &mut font)?,
		[
			"<header version=\"3.0\"/>",
			"<strike index=\"0\">",
			"  <bitmapSizeTable>",
			"    <sbitLineMetrics direction=\"hori\">",
			"      <ascender value=\"101\"/>",
			"      <descender value=\"-27\"/>",
			"      <widthMax value=\"136\"/>",
			"      <caretSlopeNumerator value=\"0\"/>",
			"      <caretSlopeDenominator value=\"0\"/>",
			"      <caretOffset value=\"0\"/>",
			"      <minOriginSB value=\"0\"/>",
			"      <minAdvanceSB value=\"0\"/>",
			"      <maxBeforeBL value=\"0\"/>",
			"      <minAfterBL value=\"0\"/>",
			"      <pad1 value=\"0\"/>",
			"      <pad2 value=\"0\"/>",
			"    </sbitLineMetrics>",
			"    <sbitLineMetrics direction=\"vert\">",
			"      <ascender value=\"101\"/>",
			"      <descender value=\"-27\"/>",
			"      <widthMax value=\"136\"/>",
			"      <caretSlopeNumerator value=\"0\"/>",
			"      <caretSlopeDenominator value=\"0\"/>",
			"      <caretOffset value=\"0\"/>",
			"      <minOriginSB value=\"0\"/>",
			"      <minAdvanceSB value=\"0\"/>",
			"      <maxBeforeBL value=\"0\"/>",
			"      <minAfterBL value=\"0\"/>",
			"      <pad1 value=\"0\"/>",
			"      <pad2 value=\"0\"/>",
			"    </sbitLineMetrics>",
			"    <colorRef value=\"0\"/>",
			"    <startGlyphIndex value=\"1\"/>",
			"    <endGlyphIndex value=\"3\"/>",
			"    <ppemX value=\"109\"/>",
			"    <ppemY value=\"109\"/>",
			"    <bitDepth value=\"32\"/>",
			"    <flags value=\"1\"/>",
			"  </bitmapSizeTable>",
			"  <!-- GlyphIds are written but not read. The firstGlyphIndex and",
			"       lastGlyphIndex values will be recalculated by the compiler. -->",
			"  <eblc_index_sub_table_3 imageFormat=\"17\" firstGlyphIndex=\"1\" lastGlyphIndex=\"2\">",
			"    <glyphLoc id=\"1\" name=\"eight\"/>",
			"    <glyphLoc id=\"2\" name=\"registered\"/>",
			"  </eblc_index_sub_table_3>",
			"  <eblc_index_sub_table_3 imageFormat=\"17\" firstGlyphIndex=\"3\" lastGlyphIndex=\"3\">",
			"    <glyphLoc id=\"3\" name=\"uni2049\"/>",
			"  </eblc_index_sub_table_3>",
			"</strike>",
		]
	);
	Ok(())
}
