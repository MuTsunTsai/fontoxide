//! The tests of fontTools' `Tests/ttLib/tables/_g_v_a_r_test.py`, and
//! `TupleVariationTest.test_compileTupleVariationStore_roundTrip_GVAR` of
//! `Tests/ttLib/tables/TupleVariation_test.py`.
//!
//! fontTools runs `test_decompile` on lazily and fully loaded tables, whose difference is what
//! it checks first; there is no lazy loading here.

use std::collections::HashMap;
use std::path::PathBuf;

use fontoxide_misc::text_tools::de_hex_str;
use fontoxide_misc::xml_reader;
use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_ttlib::tables::fvar::Axis;
use fontoxide_ttlib::tables::glyf::{Glyph, Number};
use fontoxide_ttlib::tables::gvar::{compile_offsets, decompile_offsets};
use fontoxide_ttlib::tables::tuple_variation::{
	Delta, TupleVariation, compile_tuple_variation_store, decompile_tuple_variation_store,
};
use fontoxide_ttlib::tables::{Fvar, Glyf, Gvar};
use fontoxide_ttlib::{Font, ReadOptions, Table, TableType, Tag, TtxOptions};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Variations = Vec<(u32, Vec<TupleVariation>)>;

const GVAR: Tag = Tag::new(b"GVAR");
const GVAR_LOWER: Tag = Tag::new(b"gvar");

const GVAR_DATA: &str = concat!(
	"0001 0000 ",           //   0: majorVersion=1 minorVersion=0
	"0002 0000 ",           //   4: axisCount=2 sharedTupleCount=0
	"0000001C ",            //   8: offsetToSharedTuples=28
	"0003 0000 ",           //  12: glyphCount=3 flags=0
	"0000001C ",            //  16: offsetToGlyphVariationData=28
	"0000 0000 000C 002F ", //  20: offsets=[0,0,12,47], times 2: [0,0,24,94],
	//                 #           +offsetToGlyphVariationData: [28,28,52,122]
	//
	// 28: Glyph variation data for glyph #0, ".notdef"
	// ------------------------------------------------
	// (no variation data for this glyph)
	//
	// 28: Glyph variation data for glyph #1, "space"
	// ----------------------------------------------
	"8001 000C ", //  28: tupleVariationCount=1|TUPLES_SHARE_POINT_NUMBERS, offsetToData=12(+28=40)
	"000A ",      //  32: tvHeader[0].variationDataSize=10
	"8000 ",      //  34: tvHeader[0].tupleIndex=EMBEDDED_PEAK
	"0000 2CCD ", //  36: tvHeader[0].peakTuple={wght:0.0, wdth:0.7}
	"00 ",        //  40: all points
	"03 01 02 03 04 ", //  41: deltaX=[1, 2, 3, 4]
	"03 0b 16 21 2C ", //  46: deltaY=[11, 22, 33, 44]
	"00 ",        //  51: padding
	//
	// 52: Glyph variation data for glyph #2, "I"
	// ------------------------------------------
	"8002 001c ", //  52: tupleVariationCount=2|TUPLES_SHARE_POINT_NUMBERS, offsetToData=28(+52=80)
	"0012 ",      //  56: tvHeader[0].variationDataSize=18
	"C000 ",      //  58: tvHeader[0].tupleIndex=EMBEDDED_PEAK|INTERMEDIATE_REGION
	"2000 0000 ", //  60: tvHeader[0].peakTuple={wght:0.5, wdth:0.0}
	"0000 0000 ", //  64: tvHeader[0].intermediateStart={wght:0.0, wdth:0.0}
	"4000 0000 ", //  68: tvHeader[0].intermediateEnd={wght:1.0, wdth:0.0}
	"0016 ",      //  72: tvHeader[1].variationDataSize=22
	"A000 ",      //  74: tvHeader[1].tupleIndex=EMBEDDED_PEAK|PRIVATE_POINTS
	"C000 3333 ", //  76: tvHeader[1].peakTuple={wght:-1.0, wdth:0.8}
	"00 ",        //  80: all points
	"07 03 01 04 01 ", //  81: deltaX.len=7, deltaX=[3, 1, 4, 1,
	"05 09 02 06 ", //  86:                       5, 9, 2, 6]
	"07 03 01 04 01 ", //  90: deltaY.len=7, deltaY=[3, 1, 4, 1,
	"05 09 02 06 ", //  95:                       5, 9, 2, 6]
	"06 ",        //  99: 6 points
	"05 00 01 03 01 ", // 100: runLen=5(+1=6); delta-encoded run=[0, 1, 4, 5,
	"01 01 ",     // 105:                                    6, 7]
	"05 f8 07 fc 03 fe 01 ", // 107: deltaX.len=5, deltaX=[-8,7,-4,3,-2,1]
	"05 a8 4d 2c 21 ea 0b ", // 114: deltaY.len=5, deltaY=[-88,77,44,33,-22,11]
	"00"          // 121: padding
); // 122: <end>

const GVAR_DATA_EMPTY_VARIATIONS: &str = concat!(
	"0001 0000 ",          //  0: majorVersion=1 minorVersion=0
	"0002 0000 ",          //  4: axisCount=2 sharedTupleCount=0
	"0000001c ",           //  8: offsetToSharedTuples=28
	"0003 0000 ",          // 12: glyphCount=3 flags=0
	"0000001c ",           // 16: offsetToGlyphVariationData=28
	"0000 0000 0000 0000"  // 20: offsets=[0, 0, 0, 0]
); // 28: <end>

const GVAR_XML: [&str; 34] = [
	r#"<version value="1"/>"#,
	r#"<reserved value="0"/>"#,
	r#"<glyphVariations glyph="I">"#,
	"  <tuple>",
	r#"    <coord axis="wght" min="0.0" value="0.5" max="1.0"/>"#,
	r#"    <delta pt="0" x="3" y="3"/>"#,
	r#"    <delta pt="1" x="1" y="1"/>"#,
	r#"    <delta pt="2" x="4" y="4"/>"#,
	r#"    <delta pt="3" x="1" y="1"/>"#,
	r#"    <delta pt="4" x="5" y="5"/>"#,
	r#"    <delta pt="5" x="9" y="9"/>"#,
	r#"    <delta pt="6" x="2" y="2"/>"#,
	r#"    <delta pt="7" x="6" y="6"/>"#,
	"  </tuple>",
	"  <tuple>",
	r#"    <coord axis="wght" value="-1.0"/>"#,
	r#"    <coord axis="wdth" value="0.8"/>"#,
	r#"    <delta pt="0" x="-8" y="-88"/>"#,
	r#"    <delta pt="1" x="7" y="77"/>"#,
	r#"    <delta pt="4" x="-4" y="44"/>"#,
	r#"    <delta pt="5" x="3" y="33"/>"#,
	r#"    <delta pt="6" x="-2" y="-22"/>"#,
	r#"    <delta pt="7" x="1" y="11"/>"#,
	"  </tuple>",
	"</glyphVariations>",
	r#"<glyphVariations glyph="space">"#,
	"  <tuple>",
	r#"    <coord axis="wdth" value="0.7"/>"#,
	r#"    <delta pt="0" x="1" y="11"/>"#,
	r#"    <delta pt="1" x="2" y="22"/>"#,
	r#"    <delta pt="2" x="3" y="33"/>"#,
	r#"    <delta pt="3" x="4" y="44"/>"#,
	"  </tuple>",
	"</glyphVariations>",
];

fn hex(text: &str) -> Vec<u8> {
	de_hex_str(text).expect("hex data")
}

fn deltas(points: &[Option<(i64, i64)>]) -> Vec<Option<Delta>> {
	points
		.iter()
		.map(|p| p.map(|(x, y)| Delta::Point(Number::Int(x), Number::Int(y))))
		.collect()
}

fn all_deltas(points: &[(i64, i64)]) -> Vec<Option<Delta>> {
	let points: Vec<_> = points.iter().copied().map(Some).collect();
	deltas(&points)
}

fn variation(axes: &[(&str, (f64, f64, f64))], coordinates: Vec<Option<Delta>>) -> TupleVariation {
	TupleVariation {
		axes: axes
			.iter()
			.map(|(tag, region)| ((*tag).to_owned(), *region))
			.collect(),
		coordinates,
	}
}

/// `GVAR_VARIATIONS`, with the axes under these names.
fn gvar_variations(wght: &str, wdth: &str) -> Variations {
	vec![
		(0, vec![]),
		(
			1,
			vec![variation(
				&[(wdth, (0.0, 0.7000122, 0.7000122))],
				all_deltas(&[(1, 11), (2, 22), (3, 33), (4, 44)]),
			)],
		),
		(
			2,
			vec![
				variation(
					&[(wght, (0.0, 0.5, 1.0))],
					all_deltas(&[
						(3, 3),
						(1, 1),
						(4, 4),
						(1, 1),
						(5, 5),
						(9, 9),
						(2, 2),
						(6, 6),
					]),
				),
				variation(
					&[
						(wght, (-1.0, -1.0, 0.0)),
						(wdth, (0.0, 0.7999878, 0.7999878)),
					],
					deltas(&[
						Some((-8, -88)),
						Some((7, 77)),
						None,
						None,
						Some((-4, 44)),
						Some((3, 33)),
						Some((-2, -22)),
						Some((1, 11)),
					]),
				),
			],
		),
	]
}

/// `assertVariationsAlmostEqual`.
fn assert_variations_almost_equal(actual: &Variations, expected: &Variations) {
	let keys = |v: &Variations| {
		let mut keys: Vec<u32> = v.iter().map(|(g, _)| *g).collect();
		keys.sort();
		keys
	};
	assert_eq!(keys(actual), keys(expected));
	for (glyph, variations1) in actual {
		let (_, variations2) = expected
			.iter()
			.find(|(g, _)| g == glyph)
			.expect("the same glyphs");
		assert_eq!(variations1.len(), variations2.len());
		for (v1, v2) in variations1.iter().zip(variations2) {
			assert_eq!(
				v1.axes.keys().collect::<Vec<_>>(),
				v2.axes.keys().collect::<Vec<_>>()
			);
			for (axis, s1) in &v1.axes {
				let s2 = &v2.axes[axis];
				for (a, b) in [(s1.0, s2.0), (s1.1, s2.1), (s1.2, s2.2)] {
					// `assertAlmostEqual`: seven decimal places.
					assert!(((a - b) * 1e7).round() == 0.0, "{a} != {b}");
				}
			}
			assert_eq!(v1.coordinates, v2.coordinates);
		}
	}
}

/// `makeFont`: the font and its table, which is not in the font.
fn make_font(variations: Variations, extended: bool) -> (Font, Gvar) {
	let mut font = Font::new();
	font.set_glyph_order(vec![".notdef".into(), "space".into(), "I".into()]);
	let mut glyf = Glyf::new_as(Tag::new(if extended { b"GLYF" } else { b"glyf" }));
	for gid in 0..3 {
		glyf.glyphs.insert(gid, Glyph::default());
	}
	if let Some(glyph) = glyf.glyphs.get_mut(&2) {
		glyph.coordinates = vec![(10.0, 10.0), (10.0, 20.0), (20.0, 20.0), (20.0, 10.0)];
	}
	font.insert(Box::new(glyf));
	let mut fvar = Fvar::default();
	fvar.axes.push(Axis::new(Tag::new(b"wght")));
	fvar.axes.push(Axis::new(Tag::new(b"wdth")));
	font.insert(Box::new(fvar));
	let mut gvar = Gvar::new_as(if extended { GVAR } else { GVAR_LOWER });
	gvar.variations = variations;
	(font, gvar)
}

/// `makeStaticFont`.
fn make_static_font(variations: Variations) -> (Font, Gvar) {
	let (mut font, gvar) = make_font(variations, false);
	font.remove(Tag::new(b"fvar"));
	(font, gvar)
}

fn from_xml(gvar: &mut Gvar, font: &mut Font, lines: &[String]) -> TestResult {
	let root = xml_reader::parse(&format!("<root>{}</root>", lines.join("\n")))?;
	for element in root.elements() {
		gvar.from_xml(element, font)?;
	}
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_GVAR_compile_decompile() -> TestResult {
	let variations = gvar_variations("wght", "wdth");
	let (mut font, mut gvar) = make_font(variations.clone(), true);
	let data = gvar.compile(&mut font)?;
	assert_eq!(data[12..15], [0, 0, 3]);
	let offset = u32::from_be_bytes([data[17], data[18], data[19], data[20]]) as usize;
	assert_eq!(data[offset..offset + 5], [0x80, 0x01, 0x00, 0x00, 0x0d]);

	font.insert(Box::new(gvar));
	let decompiled = Gvar::decompile_as(GVAR, &data, &mut font)?;
	assert!(decompiled.extended);
	// The glyph count is checked when decompiling, and not kept.
	assert_eq!(decompiled.variations.len(), 3);
	assert_variations_almost_equal(&decompiled.variations, &variations);
	Ok(())
}

#[test]
fn test_compile() -> TestResult {
	let (mut font, mut gvar) = make_font(gvar_variations("wght", "wdth"), false);
	assert_eq!(gvar.compile(&mut font)?, hex(GVAR_DATA));
	Ok(())
}

#[test]
fn test_compile_no_variations() -> TestResult {
	let (mut font, mut gvar) = make_font(vec![], false);
	assert_eq!(gvar.compile(&mut font)?, hex(GVAR_DATA_EMPTY_VARIATIONS));
	Ok(())
}

#[test]
fn test_compile_empty_variations() -> TestResult {
	let (mut font, mut gvar) = make_font(vec![(0, vec![]), (1, vec![]), (2, vec![])], false);
	assert_eq!(gvar.compile(&mut font)?, hex(GVAR_DATA_EMPTY_VARIATIONS));
	Ok(())
}

#[test]
fn test_decompile() -> TestResult {
	let (mut font, _) = make_font(vec![], false);
	let gvar = Gvar::decompile(&hex(GVAR_DATA), &mut font)?;
	assert_variations_almost_equal(&gvar.variations, &gvar_variations("wght", "wdth"));
	Ok(())
}

#[test]
fn test_decompile_no_variations() -> TestResult {
	let (mut font, _) = make_font(vec![], false);
	let gvar = Gvar::decompile(&hex(GVAR_DATA_EMPTY_VARIATIONS), &mut font)?;
	assert_eq!(gvar.variations, [(0, vec![]), (1, vec![]), (2, vec![])]);
	Ok(())
}

#[test]
fn test_from_xml() -> TestResult {
	let (mut font, mut gvar) = make_font(vec![], false);
	let lines: Vec<String> = GVAR_XML.iter().map(|l| (*l).to_owned()).collect();
	from_xml(&mut gvar, &mut font, &lines)?;
	let expected: Variations = gvar_variations("wght", "wdth")
		.into_iter()
		.filter(|(_, v)| !v.is_empty())
		.collect();
	assert_variations_almost_equal(&gvar.variations, &expected);
	Ok(())
}

#[test]
fn test_from_xml_numbered_axes() -> TestResult {
	let (mut font, mut gvar) = make_static_font(vec![]);
	let mut lines = Vec::new();
	for element in GVAR_XML {
		lines.push(
			element
				.replace(r#"axis="wght""#, r#"axis="0""#)
				.replace(r#"axis="wdth""#, r#"axis="1""#),
		);
		if element == r#"<reserved value="0"/>"# {
			lines.push(r#"<axisCount value="2"/>"#.to_owned());
		}
	}
	from_xml(&mut gvar, &mut font, &lines)?;
	assert_eq!(gvar.axis_count, 2);
	let expected: Variations = gvar_variations("0", "1")
		.into_iter()
		.filter(|(_, v)| !v.is_empty())
		.collect();
	assert_variations_almost_equal(&gvar.variations, &expected);
	Ok(())
}

#[test]
fn test_import_xml_gvar_before_fvar() -> TestResult {
	// fontTools writes 'fvar' before 'gvar', but a TTX can list them in
	// any order; reading 'gvar' first must still give back the same table.
	// This font also has numeric-looking axis tags ('0000'..'0028').
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttLib/data/varc-6868.ttf");
	let mut font = Font::read(std::fs::read(path)?, ReadOptions::default())?;
	let expected = font.table_data(GVAR_LOWER)?;
	let xml = font.to_ttx(&TtxOptions::default())?;
	let start = xml.find("\n  <gvar>").ok_or("no gvar")?;
	let end = start + xml[start..].find("</gvar>\n").ok_or("no end of gvar")? + "</gvar>\n".len();
	let gvar_block = &xml[start..end];
	let without = format!("{}\n{}", &xml[..start], &xml[end..]);
	let fvar_start = without.find("\n  <fvar>").ok_or("no fvar")?;
	let xml = format!(
		"{}{}{}",
		&without[..fvar_start],
		gvar_block.trim_end_matches('\n'),
		&without[fvar_start..]
	);
	assert!(xml.find("<gvar>") < xml.find("<fvar>"));

	let mut font = Font::new();
	font.import_ttx(&xml)?;
	assert_eq!(font.table_data(GVAR_LOWER)?, expected);
	Ok(())
}

#[test]
fn test_to_xml() -> TestResult {
	let (mut font, gvar) = make_font(gvar_variations("wght", "wdth"), false);
	let mut writer = XmlWriter::with_options("  ", "\n");
	gvar.to_xml(&mut writer, &mut font)?;
	let text = writer.into_string();
	let lines: Vec<&str> = text
		.lines()
		.skip_while(|l| l.starts_with("<?xml"))
		.collect();
	assert_eq!(lines, GVAR_XML);
	Ok(())
}

#[test]
fn test_compile_offsets_short_format() -> TestResult {
	assert_eq!(
		compile_offsets(&[0, 4, 0x1FF80])?,
		(hex("00 00 00 02 FF C0"), 0)
	);
	Ok(())
}

#[test]
fn test_compile_offsets_long_format() -> TestResult {
	assert_eq!(
		compile_offsets(&[0, 4, 0xCAFE_BEEF])?,
		(hex("00 00 00 00 00 00 00 04 CA FE BE EF"), 1)
	);
	Ok(())
}

#[test]
fn test_decompile_offsets_short_format() {
	let data = hex("00 11 22 33 44 55 66 77 88 99 aa bb");
	assert_eq!(
		decompile_offsets(&data, 0, 5),
		[
			2 * 0x0011,
			2 * 0x2233,
			2 * 0x4455,
			2 * 0x6677,
			2 * 0x8899,
			2 * 0xAABB
		]
	);
}

#[test]
fn test_decompile_offsets_long_format() {
	let data = hex("00 11 22 33 44 55 66 77 88 99 aa bb");
	assert_eq!(
		decompile_offsets(&data, 1, 2),
		[0x0011_2233, 0x4455_6677, 0x8899_AABB]
	);
}

/// `TupleVariationTest.test_compileTupleVariationStore_roundTrip_GVAR`.
#[test]
fn test_compile_tuple_variation_store_round_trip_gvar() -> TestResult {
	let points = all_deltas(&[(1, 1), (2, 2), (3, 3), (4, 4)]);
	let variations = [
		variation(
			&[("wght", (0.5, 1.0, 1.0)), ("wdth", (1.0, 1.0, 1.0))],
			points.clone(),
		),
		variation(
			&[("wght", (1.0, 1.0, 1.0)), ("wdth", (1.0, 1.0, 1.0))],
			points,
		),
	];
	let axis_tags = ["wght".to_owned(), "wdth".to_owned()];
	let (count, tuples, data) =
		compile_tuple_variation_store(&variations, &axis_tags, &HashMap::new(), true, true)?;
	let tuples_len = tuples.len();
	let all: Vec<u8> = tuples.into_iter().chain(data).collect();
	assert_eq!(
		decompile_tuple_variation_store(
			"GVAR",
			&axis_tags,
			count as u16,
			4,
			Some(&[]),
			&all,
			0,
			tuples_len,
		)?,
		variations
	);
	Ok(())
}
