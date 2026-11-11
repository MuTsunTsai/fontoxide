//! `Tests/varLib/builder_test.py`: the tests whose masters are built with
//! FontBuilder and TTGlyphPen. The others are in
//! `crates/fontoxide-varlib/tests/builder.rs`.

#![cfg(all(feature = "designspacelib", feature = "varlib", feature = "repacker"))]

use fontoxide::designspacelib::{
	AxisDescriptor, DesignSpaceDocument, LocationValue, Number, SourceDescriptor,
};
use fontoxide::font_builder::{FontBuilder, NameKey, NameValue, Value};
use fontoxide::misc::Tag;
use fontoxide::misc::xml_writer::XmlWriter;
use fontoxide::pens::Pen;
use fontoxide::ttlib::pens::TtGlyphPen;
use fontoxide::ttlib::tables::Glyf;
use fontoxide::ttlib::tables::glyf::Glyph;
use fontoxide::ttlib::{Font, Table};
use fontoxide::varlib::build::{BuildOptions, SourceFonts, build, identity_finder};
use indexmap::IndexMap;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn s(v: &str) -> String {
	v.to_owned()
}

fn weight_axis(minimum: i64, default: i64, maximum: i64) -> AxisDescriptor {
	AxisDescriptor {
		tag: Some(s("wght")),
		name: Some(s("Weight")),
		minimum: Number::Int(minimum),
		default: Number::Int(default),
		maximum: Number::Int(maximum),
		..Default::default()
	}
}

/// `SourceDescriptor(font=..., location={"Weight": wght})`.
fn add_source(doc: &mut DesignSpaceDocument, fonts: &mut SourceFonts, font: Font, wght: i64) {
	let mut source = SourceDescriptor::default();
	source
		.design_location
		.insert(s("Weight"), LocationValue::Value(Number::Int(wght)));
	doc.add_source(source);
	fonts.push(Some(font));
}

fn names(family: &str) -> Vec<(NameKey, NameValue)> {
	vec![
		(NameKey::from("familyName"), NameValue::from(family)),
		(NameKey::from("styleName"), NameValue::from("Regular")),
	]
}

fn build_vf(
	doc: &mut DesignSpaceDocument,
	fonts: &mut SourceFonts,
) -> Result<Font, Box<dyn std::error::Error>> {
	Ok(build(doc, fonts, &identity_finder, &BuildOptions::default())?.0)
}

/// HVAR/VHVAR should be present but empty when there are no glyph metrics
/// variations, and should use a direct mapping for optimal encoding.
#[test]
fn test_empty_vhvar_size() -> TestResult {
	// Make a designspace that varies the outlines of 'A' but not its advance.
	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(weight_axis(400, 400, 700));
	let mut fonts = SourceFonts::new();
	for wght in [400, 700] {
		// Outlines depend on weight.
		let w = wght as f64;
		let mut pen = TtGlyphPen::new(None);
		pen.line_to((0.0, w))?;
		pen.line_to((w, w))?;
		pen.line_to((w, 0.0))?;
		pen.close_path()?;

		let mut fb = FontBuilder::new(1000, true, false)?;
		fb.setup_glyph_order(vec![s("A")]);
		let order = fb.font.glyph_order()?;
		let glyphs: IndexMap<String, Glyph> = [(s("A"), pen.glyph(&order)?)].into_iter().collect();
		fb.setup_glyf(glyphs, true)?;
		let (x_min, y_max) = {
			let g = &fb.font.require::<Glyf>()?.glyphs[&0];
			(g.x_min, g.y_max)
		};

		// Horizontal advance does not vary.
		fb.setup_horizontal_metrics(&[(s("A"), (500.0, x_min as f64))].into_iter().collect())?;
		fb.setup_horizontal_header(&[("ascent", Value::Int(1000)), ("descent", Value::Int(0))])?;

		// Vertical advance does not vary.
		fb.setup_vertical_metrics(
			&[(s("A"), (500.0, (1000 - y_max) as f64))]
				.into_iter()
				.collect(),
		)?;
		fb.setup_vertical_header(&[("ascent", Value::Int(1000)), ("descent", Value::Int(0))])?;

		fb.setup_name_table(&names("TestEmptyVhvar"), true, true)?;
		fb.setup_post(true, &[])?;
		add_source(&mut doc, &mut fonts, fb.font, wght);
	}

	// Compile.
	let mut vf = build_vf(&mut doc, &mut fonts)?;

	// Test both tables' encodings:
	for (table, optimal_size) in [("HVAR", 42), ("VVAR", 46)] {
		let tag = Tag::from_str_padded(table).expect("a tag");
		// Variable glyph metrics table should be built even when there are no
		// glyph metrics variations.
		assert!(vf.contains(tag));

		// The table should be empty, and use a direct mapping for optimal size.
		let expected = r#"<?xml version="1.0" encoding="UTF-8"?>
<Version value="0x00010000"/>
<VarStore Format="1">
  <Format value="1"/>
  <VarRegionList>
    <!-- RegionAxisCount=1 -->
    <!-- RegionCount=0 -->
  </VarRegionList>
  <!-- VarDataCount=1 -->
  <VarData index="0">
    <!-- ItemCount=1 -->
    <NumShorts value="0"/>
    <!-- VarRegionCount=0 -->
    <Item index="0" value="[]"/>
  </VarData>
</VarStore>
"#;
		let actual = vf.with_table(tag, |t: &mut dyn Table, font: &mut Font| {
			let mut writer = XmlWriter::with_options("  ", "\n");
			t.to_xml(&mut writer, font)?;
			Ok(writer.into_string())
		})?;
		assert_eq!(actual, expected);

		// The table should be encodable in at least this size. (VVAR has an extra
		// Offset32 to point to a vertical origin mapping)
		assert!(vf.table_data(tag)?.len() <= optimal_size, "{table}");
	}
	Ok(())
}

/// varLib can build HVAR/VVAR for fonts with more than 0xFFFF glyphs.
///
/// The advance store uses the indirect mapping (the direct/no-map store can't
/// index more than 0xFFFF glyphs, since VarData.ItemCount is uint16), and the
/// advance index map escalates to DeltaSetIndexMap Format 1 (uint32
/// MappingCount) when a glyph beyond gid 0xFFFF varies.
#[test]
fn test_vhvar_beyond_64k() -> TestResult {
	let n = 0x10001; // 65537 glyphs -> highest gid is 0x10000
	let mut glyph_order = vec![s(".notdef")];
	glyph_order.extend((1..n).map(|i| format!("g{i:05}")));
	let high = glyph_order[glyph_order.len() - 1].clone();

	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(weight_axis(0, 0, 1000));
	let mut fonts = SourceFonts::new();
	// The two advances just need to differ so the high glyph actually varies.
	for (wght, adv) in [(0, 480.0), (1000, 520.0)] {
		let mut fb = FontBuilder::new(1000, true, false)?;
		fb.setup_glyph_order(glyph_order.clone());
		fb.setup_glyf(
			glyph_order
				.iter()
				.map(|g| (g.clone(), Glyph::default()))
				.collect(),
			true,
		)?;
		// Only the highest glyph's advances vary, so the map cannot be trimmed
		// below 65536 entries and must use Format 1.
		let mut metrics: IndexMap<String, (f64, f64)> = glyph_order
			.iter()
			.map(|g| (g.clone(), (500.0, 0.0)))
			.collect();
		metrics.insert(high.clone(), (adv, 0.0));
		fb.setup_horizontal_metrics(&metrics)?;
		fb.setup_horizontal_header(&[("ascent", Value::Int(1000)), ("descent", Value::Int(0))])?;
		fb.setup_vertical_metrics(&metrics)?;
		fb.setup_vertical_header(&[("ascent", Value::Int(1000)), ("descent", Value::Int(0))])?;
		fb.setup_name_table(&names("TestBeyond64k"), true, true)?;
		fb.setup_post(false, &[])?;
		add_source(&mut doc, &mut fonts, fb.font, wght);
	}

	let mut vf = build_vf(&mut doc, &mut fonts)?;

	for table in ["HVAR", "VVAR"] {
		let tag = Tag::from_str_padded(table).expect("a tag");
		assert!(vf.contains(tag));
		// The offset of AdvWidthMap / AdvHeightMap follows the version and the
		// offset of the VarStore.
		let data = vf.table_data(tag)?;
		let offset = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize;
		assert_ne!(offset, 0, "{table} has an advance map");
		assert_eq!(
			data[offset], 1,
			"{table} advance map should use DeltaSetIndexMap Format 1"
		);
	}
	Ok(())
}
