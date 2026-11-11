//! `Tests/ttLib/tables/_m_o_r_x_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::{TestResult, compile_from_xml, decompile_to_xml, hex, hex_str};
use fontoxide_ttlib::{TableType, Tag};

const TAG: Tag = Tag::new(b"morx");

/// `[".notdef"] + ["g.%d" % i for i in range(1, n)]`, with some names replaced.
fn glyphs(n: usize, names: &[(usize, &str)]) -> Vec<String> {
	let mut g: Vec<String> = std::iter::once(".notdef".to_owned())
		.chain((1..n).map(|i| format!("g.{i}")))
		.collect();
	for &(i, name) in names {
		g[i] = name.to_owned();
	}
	g
}

fn refs(names: &[String]) -> Vec<&str> {
	names.iter().map(String::as_str).collect()
}

fn roundtrip(glyphs: &[String], data: &str, xml: &[&str]) -> TestResult {
	let glyphs = refs(glyphs);
	assert_eq!(decompile_to_xml(TAG, &glyphs, &hex(data))?, xml);
	Ok(())
}

fn compile(glyphs: &[String], data: &str, xml: &[&str]) -> TestResult {
	let glyphs = refs(glyphs);
	assert_eq!(
		hex_str(&compile_from_xml(TAG, &glyphs, xml)?),
		hex_str(&hex(data))
	);
	Ok(())
}

fn noncontextual_glyphs() -> Vec<String> {
	glyphs(
		140,
		&[
			(11, "parenleft"),
			(13, "parenright"),
			(135, "parenleft.vertical"),
			(136, "parenright.vertical"),
		],
	)
}

#[test]
fn noncontextual_test_decompile_to_xml() -> TestResult {
	roundtrip(
		&noncontextual_glyphs(),
		MORX_NONCONTEXTUAL_DATA,
		MORX_NONCONTEXTUAL_XML,
	)
}

#[test]
fn noncontextual_test_compile_from_xml() -> TestResult {
	compile(
		&noncontextual_glyphs(),
		MORX_NONCONTEXTUAL_DATA,
		MORX_NONCONTEXTUAL_XML,
	)
}

fn abc() -> Vec<String> {
	[".nodef", "A", "B", "C"].map(str::to_owned).to_vec()
}

#[test]
fn rearrangement_test_decompile_to_xml() -> TestResult {
	roundtrip(&abc(), MORX_REARRANGEMENT_DATA, MORX_REARRANGEMENT_XML)
}

#[test]
fn rearrangement_test_compile_from_xml() -> TestResult {
	compile(&abc(), MORX_REARRANGEMENT_DATA, MORX_REARRANGEMENT_XML)
}

fn contextual_glyphs() -> Vec<String> {
	glyphs(
		910,
		&[
			(80, "C"),
			(50, "A"),
			(52, "B"),
			(201, "X"),
			(202, "Y"),
			(600, "A.swash"),
			(601, "B.swash"),
			(602, "X.swash"),
			(900, "Y.swash"),
		],
	)
}

#[test]
fn contextual_test_decompile_to_xml() -> TestResult {
	roundtrip(
		&contextual_glyphs(),
		MORX_CONTEXTUAL_DATA,
		MORX_CONTEXTUAL_XML,
	)
}

#[test]
fn contextual_test_compile_from_xml() -> TestResult {
	compile(
		&contextual_glyphs(),
		MORX_CONTEXTUAL_DATA,
		MORX_CONTEXTUAL_XML,
	)
}

fn ligature_glyphs() -> Vec<String> {
	let mut g = glyphs(1515, &[]);
	for (start, names) in [
		(20, "a b c d e f g h i"),
		(1000, "adf adg adh adi aef aeg aeh aei"),
		(1008, "bdf bdg bdh bdi bef beg beh bei"),
		(1500, "cdf cdg cdh cdi cef ceg ceh"),
		(1511, "cei"),
	] {
		for (i, name) in names.split(' ').enumerate() {
			g[start + i] = name.to_owned();
		}
	}
	g
}

#[test]
fn ligature_test_decompile_to_xml() -> TestResult {
	roundtrip(&ligature_glyphs(), MORX_LIGATURE_DATA, MORX_LIGATURE_XML)
}

#[test]
fn ligature_test_compile_from_xml() -> TestResult {
	compile(&ligature_glyphs(), MORX_LIGATURE_DATA, MORX_LIGATURE_XML)
}

#[test]
fn insertion_test_decompile_to_xml() -> TestResult {
	roundtrip(&glyphs(910, &[]), MORX_INSERTION_DATA, MORX_INSERTION_XML)
}

#[test]
fn insertion_test_compile_from_xml() -> TestResult {
	compile(&glyphs(910, &[]), MORX_INSERTION_DATA, MORX_INSERTION_XML)
}

fn check_flags(
	flags: u8,
	text_direction: &str,
	processing_order: &str,
	check_compile: bool,
) -> TestResult {
	let mut data = hex(MORX_REARRANGEMENT_DATA);
	data[28] = flags << 4;
	let xml: Vec<String> = MORX_REARRANGEMENT_XML
		.iter()
		.map(|line| {
			if line.starts_with("    <TextDirection ") {
				format!("    <TextDirection value=\"{text_direction}\"/>")
			} else if line.starts_with("    <ProcessingOrder ") {
				format!("    <ProcessingOrder value=\"{processing_order}\"/>")
			} else {
				(*line).to_owned()
			}
		})
		.collect();
	let glyphs = abc();
	let glyphs = refs(&glyphs);
	assert_eq!(decompile_to_xml(TAG, &glyphs, &data)?, xml);
	if check_compile {
		let xml = refs(&xml);
		assert_eq!(
			hex_str(&compile_from_xml(TAG, &glyphs, &xml)?),
			hex_str(&data)
		);
	}
	Ok(())
}

#[test]
fn test_coverage_flags() -> TestResult {
	check_flags(0x0, "Horizontal", "LayoutOrder", true)?;
	check_flags(0x1, "Horizontal", "LogicalOrder", true)?;
	check_flags(0x2, "Any", "LayoutOrder", true)?;
	check_flags(0x3, "Any", "LogicalOrder", true)?;
	check_flags(0x4, "Horizontal", "ReversedLayoutOrder", true)?;
	check_flags(0x5, "Horizontal", "ReversedLogicalOrder", true)?;
	check_flags(0x6, "Any", "ReversedLayoutOrder", true)?;
	check_flags(0x7, "Any", "ReversedLogicalOrder", true)?;
	check_flags(0x8, "Vertical", "LayoutOrder", true)?;
	check_flags(0x9, "Vertical", "LogicalOrder", true)?;
	// Some flag combinations do not make sense to emit in binary: with
	// TextDirection=Any (bit 28), bit 30 (Vertical) is ignored, and the encoder
	// does not set it.
	check_flags(0xA, "Any", "LayoutOrder", false)?;
	check_flags(0xB, "Any", "LogicalOrder", false)?;
	check_flags(0xC, "Vertical", "ReversedLayoutOrder", true)?;
	check_flags(0xD, "Vertical", "ReversedLogicalOrder", true)?;
	check_flags(0xE, "Any", "ReversedLayoutOrder", false)?;
	check_flags(0xF, "Any", "ReversedLogicalOrder", false)?;
	Ok(())
}

#[test]
fn test_reserved_coverage_flags() -> TestResult {
	// 8A BC DE = TextDirection=Vertical, Reserved=0xABCDE.
	let mut data = hex(MORX_REARRANGEMENT_DATA);
	data[28..31].copy_from_slice(&[0x8A, 0xBC, 0xDE]);
	let glyphs = abc();
	let glyphs = refs(&glyphs);
	let mut font = common::fake_font(&glyphs);
	let table = fontoxide_ttlib::tables::otl::Morx::decompile(&data, &mut font)?;
	let subtable = &table.morph_chain[0].morph_subtable[0];
	assert_eq!(subtable.reserved, 0xABCDE);
	let xml = common::get_xml(&table, &mut font)?;
	assert!(xml.iter().any(|l| l == "    <Reserved value=\"0xabcde\"/>"));
	let xml = refs(&xml);
	assert_eq!(
		hex_str(&compile_from_xml(TAG, &glyphs, &xml)?[28..31]),
		"8abcde"
	);
	Ok(())
}

#[test]
fn test_unsupported_lookup_type() {
	let mut data = hex(MORX_NONCONTEXTUAL_DATA);
	data[67] = 66;
	data.remove(68);
	let error = decompile_to_xml(TAG, &[".notdef"], &data).unwrap_err();
	assert!(
		error
			.to_string()
			.contains("unsupported 'morx' lookup type 66"),
		"{error}"
	);
}

const MORX_NONCONTEXTUAL_DATA: &str = concat!(
	"0002000000000001000000010000005800000003000000010004000000000001",
	"ffffffff0004000100000000fffffffe00000001000000000000000000000024",
	"8000000400000001000600040002000800010000000b0087000d0088ffff0000",
);

const MORX_NONCONTEXTUAL_XML: &[&str] = &[
	"<Version value=\"2\"/>",
	"<Reserved value=\"0\"/>",
	"<!-- MorphChainCount=1 -->",
	"<MorphChain index=\"0\">",
	"  <DefaultFlags value=\"0x00000001\"/>",
	"  <!-- StructLength=88 -->",
	"  <!-- MorphFeatureCount=3 -->",
	"  <!-- MorphSubtableCount=1 -->",
	"  <MorphFeature index=\"0\">",
	"    <FeatureType value=\"4\"/>",
	"    <FeatureSetting value=\"0\"/>",
	"    <EnableFlags value=\"0x00000001\"/>",
	"    <DisableFlags value=\"0xFFFFFFFF\"/>",
	"  </MorphFeature>",
	"  <MorphFeature index=\"1\">",
	"    <FeatureType value=\"4\"/>",
	"    <FeatureSetting value=\"1\"/>",
	"    <EnableFlags value=\"0x00000000\"/>",
	"    <DisableFlags value=\"0xFFFFFFFE\"/>",
	"  </MorphFeature>",
	"  <MorphFeature index=\"2\">",
	"    <FeatureType value=\"0\"/>",
	"    <FeatureSetting value=\"1\"/>",
	"    <EnableFlags value=\"0x00000000\"/>",
	"    <DisableFlags value=\"0x00000000\"/>",
	"  </MorphFeature>",
	"  <MorphSubtable index=\"0\">",
	"    <!-- StructLength=36 -->",
	"    <TextDirection value=\"Vertical\"/>",
	"    <ProcessingOrder value=\"LayoutOrder\"/>",
	"    <!-- MorphType=4 -->",
	"    <SubFeatureFlags value=\"0x00000001\"/>",
	"    <NoncontextualMorph>",
	"      <Substitution>",
	"        <Lookup glyph=\"parenleft\" value=\"parenleft.vertical\"/>",
	"        <Lookup glyph=\"parenright\" value=\"parenright.vertical\"/>",
	"      </Substitution>",
	"    </NoncontextualMorph>",
	"  </MorphSubtable>",
	"</MorphChain>",
];

const MORX_REARRANGEMENT_DATA: &str = concat!(
	"0002000000000001000000010000007800000000000000010000006880000000",
	"000000010000000600000010000000280000004c000600040002000800010000",
	"0001000500030004ffff00000000000100020003000200010003000300030003",
	"000300030001000300030003000200020002ffff0001a00d0000800600020000",
);

const MORX_REARRANGEMENT_XML: &[&str] = &[
	"<Version value=\"2\"/>",
	"<Reserved value=\"0\"/>",
	"<!-- MorphChainCount=1 -->",
	"<MorphChain index=\"0\">",
	"  <DefaultFlags value=\"0x00000001\"/>",
	"  <!-- StructLength=120 -->",
	"  <!-- MorphFeatureCount=0 -->",
	"  <!-- MorphSubtableCount=1 -->",
	"  <MorphSubtable index=\"0\">",
	"    <!-- StructLength=104 -->",
	"    <TextDirection value=\"Vertical\"/>",
	"    <ProcessingOrder value=\"LayoutOrder\"/>",
	"    <!-- MorphType=0 -->",
	"    <SubFeatureFlags value=\"0x00000001\"/>",
	"    <RearrangementMorph>",
	"      <StateTable>",
	"        <!-- GlyphClassCount=6 -->",
	"        <GlyphClass glyph=\"A\" value=\"5\"/>",
	"        <GlyphClass glyph=\"C\" value=\"4\"/>",
	"        <State index=\"0\">",
	"          <Transition onGlyphClass=\"0\">",
	"            <NewState value=\"2\"/>",
	"            <Flags value=\"MarkFirst,DontAdvance,MarkLast\"/>",
	"            <ReservedFlags value=\"0x1FF0\"/>",
	"            <Verb value=\"15\"/><!-- ABxCD ⇒ DCxBA -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"1\">",
	"            <NewState value=\"1\"/>",
	"            <Flags value=\"MarkFirst,MarkLast\"/>",
	"            <Verb value=\"13\"/><!-- ABxCD ⇒ CDxBA -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"2\">",
	"            <NewState value=\"0\"/>",
	"            <Flags value=\"MarkFirst\"/>",
	"            <Verb value=\"6\"/><!-- xCD ⇒ CDx -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"3\">",
	"            <NewState value=\"2\"/>",
	"            <Verb value=\"0\"/><!-- no change -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"4\">",
	"            <NewState value=\"0\"/>",
	"            <Flags value=\"MarkFirst\"/>",
	"            <Verb value=\"6\"/><!-- xCD ⇒ CDx -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"5\">",
	"            <NewState value=\"1\"/>",
	"            <Flags value=\"MarkFirst,MarkLast\"/>",
	"            <Verb value=\"13\"/><!-- ABxCD ⇒ CDxBA -->",
	"          </Transition>",
	"        </State>",
	"        <State index=\"1\">",
	"          <Transition onGlyphClass=\"0\">",
	"            <NewState value=\"2\"/>",
	"            <Verb value=\"0\"/><!-- no change -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"1\">",
	"            <NewState value=\"2\"/>",
	"            <Verb value=\"0\"/><!-- no change -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"2\">",
	"            <NewState value=\"2\"/>",
	"            <Verb value=\"0\"/><!-- no change -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"3\">",
	"            <NewState value=\"2\"/>",
	"            <Verb value=\"0\"/><!-- no change -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"4\">",
	"            <NewState value=\"2\"/>",
	"            <Verb value=\"0\"/><!-- no change -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"5\">",
	"            <NewState value=\"2\"/>",
	"            <Verb value=\"0\"/><!-- no change -->",
	"          </Transition>",
	"        </State>",
	"        <State index=\"2\">",
	"          <Transition onGlyphClass=\"0\">",
	"            <NewState value=\"1\"/>",
	"            <Flags value=\"MarkFirst,MarkLast\"/>",
	"            <Verb value=\"13\"/><!-- ABxCD ⇒ CDxBA -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"1\">",
	"            <NewState value=\"2\"/>",
	"            <Verb value=\"0\"/><!-- no change -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"2\">",
	"            <NewState value=\"2\"/>",
	"            <Verb value=\"0\"/><!-- no change -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"3\">",
	"            <NewState value=\"2\"/>",
	"            <Verb value=\"0\"/><!-- no change -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"4\">",
	"            <NewState value=\"0\"/>",
	"            <Flags value=\"MarkFirst\"/>",
	"            <Verb value=\"6\"/><!-- xCD ⇒ CDx -->",
	"          </Transition>",
	"          <Transition onGlyphClass=\"5\">",
	"            <NewState value=\"0\"/>",
	"            <Flags value=\"MarkFirst\"/>",
	"            <Verb value=\"6\"/><!-- xCD ⇒ CDx -->",
	"          </Transition>",
	"        </State>",
	"      </StateTable>",
	"    </RearrangementMorph>",
	"  </MorphSubtable>",
	"</MorphChain>",
];

const MORX_CONTEXTUAL_DATA: &str = concat!(
	"000200000000000100000001000000b40000000000000001000000a480000001",
	"000000010000000600000014000000380000005c000000740006000400050010",
	"0002000400320004003400040050000500c9000400ca0004ffff000000000000",
	"0000000000000001000000000000000000000001000000000000000000020001",
	"00000000ffffffff00020000ffffffff00000000ffff00000000000400060004",
	"0004001000020000003202580034025900c9025a00ca0384ffff0000",
);

const MORX_CONTEXTUAL_XML: &[&str] = &[
	"<Version value=\"2\"/>",
	"<Reserved value=\"0\"/>",
	"<!-- MorphChainCount=1 -->",
	"<MorphChain index=\"0\">",
	"  <DefaultFlags value=\"0x00000001\"/>",
	"  <!-- StructLength=180 -->",
	"  <!-- MorphFeatureCount=0 -->",
	"  <!-- MorphSubtableCount=1 -->",
	"  <MorphSubtable index=\"0\">",
	"    <!-- StructLength=164 -->",
	"    <TextDirection value=\"Vertical\"/>",
	"    <ProcessingOrder value=\"LayoutOrder\"/>",
	"    <!-- MorphType=1 -->",
	"    <SubFeatureFlags value=\"0x00000001\"/>",
	"    <ContextualMorph>",
	"      <StateTable>",
	"        <!-- GlyphClassCount=6 -->",
	"        <GlyphClass glyph=\"A\" value=\"4\"/>",
	"        <GlyphClass glyph=\"B\" value=\"4\"/>",
	"        <GlyphClass glyph=\"C\" value=\"5\"/>",
	"        <GlyphClass glyph=\"X\" value=\"4\"/>",
	"        <GlyphClass glyph=\"Y\" value=\"4\"/>",
	"        <State index=\"0\">",
	"          <Transition onGlyphClass=\"0\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"1\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"2\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"3\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"4\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"5\">",
	"            <NewState value=\"2\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"        </State>",
	"        <State index=\"1\">",
	"          <Transition onGlyphClass=\"0\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"1\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"2\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"3\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"4\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"5\">",
	"            <NewState value=\"2\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"        </State>",
	"        <State index=\"2\">",
	"          <Transition onGlyphClass=\"0\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"1\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"2\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"3\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"4\">",
	"            <NewState value=\"0\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"5\">",
	"            <NewState value=\"2\"/>",
	"            <MarkIndex value=\"65535\"/>",
	"            <CurrentIndex value=\"65535\"/>",
	"          </Transition>",
	"        </State>",
	"        <PerGlyphLookup index=\"0\">",
	"          <Lookup glyph=\"A\" value=\"A.swash\"/>",
	"          <Lookup glyph=\"B\" value=\"B.swash\"/>",
	"          <Lookup glyph=\"X\" value=\"X.swash\"/>",
	"          <Lookup glyph=\"Y\" value=\"Y.swash\"/>",
	"        </PerGlyphLookup>",
	"      </StateTable>",
	"    </ContextualMorph>",
	"  </MorphSubtable>",
	"</MorphChain>",
];

const MORX_LIGATURE_DATA: &str = concat!(
	"000200000000000100000001000000da0000000000000001000000ca80000002",
	"00000001000000070000001c0000004000000078000000900000009c000000ae",
	"000200060003000c00010006001600140004001800170005001c001a0006ffff",
	"ffff000000000000000000000001000000000000000000000000000100000000",
	"0000000000000000000100020000000000000000000000010002000300000000",
	"00000002800000000003800000000000a00000003fffffe73fffffedbffffff2",
	"00000001000200030000000400000008001003e803e903ea03eb03ec03ed03ee",
	"03ef",
);

const MORX_LIGATURE_XML: &[&str] = &[
	"<Version value=\"2\"/>",
	"<Reserved value=\"0\"/>",
	"<!-- MorphChainCount=1 -->",
	"<MorphChain index=\"0\">",
	"  <DefaultFlags value=\"0x00000001\"/>",
	"  <!-- StructLength=218 -->",
	"  <!-- MorphFeatureCount=0 -->",
	"  <!-- MorphSubtableCount=1 -->",
	"  <MorphSubtable index=\"0\">",
	"    <!-- StructLength=202 -->",
	"    <TextDirection value=\"Vertical\"/>",
	"    <ProcessingOrder value=\"LayoutOrder\"/>",
	"    <!-- MorphType=2 -->",
	"    <SubFeatureFlags value=\"0x00000001\"/>",
	"    <LigatureMorph>",
	"      <StateTable>",
	"        <!-- GlyphClassCount=7 -->",
	"        <GlyphClass glyph=\"a\" value=\"4\"/>",
	"        <GlyphClass glyph=\"b\" value=\"4\"/>",
	"        <GlyphClass glyph=\"c\" value=\"4\"/>",
	"        <GlyphClass glyph=\"d\" value=\"5\"/>",
	"        <GlyphClass glyph=\"e\" value=\"5\"/>",
	"        <GlyphClass glyph=\"g\" value=\"6\"/>",
	"        <GlyphClass glyph=\"h\" value=\"6\"/>",
	"        <GlyphClass glyph=\"i\" value=\"6\"/>",
	"        <State index=\"0\">",
	"          <Transition onGlyphClass=\"0\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"1\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"2\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"3\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"4\">",
	"            <NewState value=\"2\"/>",
	"            <Flags value=\"SetComponent\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"5\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"6\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"        </State>",
	"        <State index=\"1\">",
	"          <Transition onGlyphClass=\"0\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"1\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"2\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"3\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"4\">",
	"            <NewState value=\"2\"/>",
	"            <Flags value=\"SetComponent\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"5\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"6\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"        </State>",
	"        <State index=\"2\">",
	"          <Transition onGlyphClass=\"0\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"1\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"2\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"3\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"4\">",
	"            <NewState value=\"2\"/>",
	"            <Flags value=\"SetComponent\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"5\">",
	"            <NewState value=\"3\"/>",
	"            <Flags value=\"SetComponent\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"6\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"        </State>",
	"        <State index=\"3\">",
	"          <Transition onGlyphClass=\"0\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"1\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"2\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"3\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"4\">",
	"            <NewState value=\"2\"/>",
	"            <Flags value=\"SetComponent\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"5\">",
	"            <NewState value=\"3\"/>",
	"            <Flags value=\"SetComponent\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"6\">",
	"            <NewState value=\"0\"/>",
	"            <Flags value=\"SetComponent\"/>",
	"            <Action GlyphIndexDelta=\"-25\"/>",
	"            <Action GlyphIndexDelta=\"-19\"/>",
	"            <Action GlyphIndexDelta=\"-14\"/>",
	"          </Transition>",
	"        </State>",
	"        <LigComponents>",
	"          <LigComponent index=\"0\" value=\"0\"/>",
	"          <LigComponent index=\"1\" value=\"1\"/>",
	"          <LigComponent index=\"2\" value=\"2\"/>",
	"          <LigComponent index=\"3\" value=\"3\"/>",
	"          <LigComponent index=\"4\" value=\"0\"/>",
	"          <LigComponent index=\"5\" value=\"4\"/>",
	"          <LigComponent index=\"6\" value=\"0\"/>",
	"          <LigComponent index=\"7\" value=\"8\"/>",
	"          <LigComponent index=\"8\" value=\"16\"/>",
	"        </LigComponents>",
	"        <Ligatures>",
	"          <Ligature glyph=\"adf\" index=\"0\"/>",
	"          <Ligature glyph=\"adg\" index=\"1\"/>",
	"          <Ligature glyph=\"adh\" index=\"2\"/>",
	"          <Ligature glyph=\"adi\" index=\"3\"/>",
	"          <Ligature glyph=\"aef\" index=\"4\"/>",
	"          <Ligature glyph=\"aeg\" index=\"5\"/>",
	"          <Ligature glyph=\"aeh\" index=\"6\"/>",
	"          <Ligature glyph=\"aei\" index=\"7\"/>",
	"        </Ligatures>",
	"      </StateTable>",
	"    </LigatureMorph>",
	"  </MorphSubtable>",
	"</MorphChain>",
];

const MORX_INSERTION_DATA: &str = concat!(
	"000200000000000100000001000000a400000000000000010000009400000005",
	"0000000100000006000000140000004a0000006e000000860002000600060018",
	"0002000c00ac00ac000501eb01e6000501f001f0000401f801f6000401fc01fa",
	"0004025002500005ffffffff0000000000000000000000010000000000000000",
	"00000001000000000000000100000001000200000000ffffffff00020000ffff",
	"ffff000028200000ffff022f",
);

const MORX_INSERTION_XML: &[&str] = &[
	"<Version value=\"2\"/>",
	"<Reserved value=\"0\"/>",
	"<!-- MorphChainCount=1 -->",
	"<MorphChain index=\"0\">",
	"  <DefaultFlags value=\"0x00000001\"/>",
	"  <!-- StructLength=164 -->",
	"  <!-- MorphFeatureCount=0 -->",
	"  <!-- MorphSubtableCount=1 -->",
	"  <MorphSubtable index=\"0\">",
	"    <!-- StructLength=148 -->",
	"    <TextDirection value=\"Horizontal\"/>",
	"    <ProcessingOrder value=\"LayoutOrder\"/>",
	"    <!-- MorphType=5 -->",
	"    <SubFeatureFlags value=\"0x00000001\"/>",
	"    <InsertionMorph>",
	"      <StateTable>",
	"        <!-- GlyphClassCount=6 -->",
	"        <GlyphClass glyph=\"g.172\" value=\"5\"/>",
	"        <GlyphClass glyph=\"g.486\" value=\"5\"/>",
	"        <GlyphClass glyph=\"g.487\" value=\"5\"/>",
	"        <GlyphClass glyph=\"g.488\" value=\"5\"/>",
	"        <GlyphClass glyph=\"g.489\" value=\"5\"/>",
	"        <GlyphClass glyph=\"g.490\" value=\"5\"/>",
	"        <GlyphClass glyph=\"g.491\" value=\"5\"/>",
	"        <GlyphClass glyph=\"g.496\" value=\"4\"/>",
	"        <GlyphClass glyph=\"g.502\" value=\"4\"/>",
	"        <GlyphClass glyph=\"g.503\" value=\"4\"/>",
	"        <GlyphClass glyph=\"g.504\" value=\"4\"/>",
	"        <GlyphClass glyph=\"g.506\" value=\"4\"/>",
	"        <GlyphClass glyph=\"g.507\" value=\"4\"/>",
	"        <GlyphClass glyph=\"g.508\" value=\"4\"/>",
	"        <GlyphClass glyph=\"g.592\" value=\"5\"/>",
	"        <State index=\"0\">",
	"          <Transition onGlyphClass=\"0\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"1\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"2\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"3\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"4\">",
	"            <NewState value=\"2\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"5\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"        </State>",
	"        <State index=\"1\">",
	"          <Transition onGlyphClass=\"0\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"1\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"2\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"3\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"4\">",
	"            <NewState value=\"2\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"5\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"        </State>",
	"        <State index=\"2\">",
	"          <Transition onGlyphClass=\"0\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"1\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"2\">",
	"            <NewState value=\"2\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"3\">",
	"            <NewState value=\"0\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"4\">",
	"            <NewState value=\"2\"/>",
	"          </Transition>",
	"          <Transition onGlyphClass=\"5\">",
	"            <NewState value=\"0\"/>",
	"            <Flags value=\"CurrentIsKashidaLike,CurrentInsertBefore\"/>",
	"            <CurrentInsertionAction glyph=\"g.559\"/>",
	"          </Transition>",
	"        </State>",
	"      </StateTable>",
	"    </InsertionMorph>",
	"  </MorphSubtable>",
	"</MorphChain>",
];
