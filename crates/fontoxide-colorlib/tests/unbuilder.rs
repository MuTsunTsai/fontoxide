//! `Tests/colorLib/unbuilder_test.py`, and the tables of the builder compared
//! with those of fontTools (`tools/reference/colorlib_fixtures.py`).

#![recursion_limit = "512"]

mod common;

use common::*;
use fontoxide_colorlib::unbuilder::unbuild_colr_v1;
use fontoxide_colorlib::{
	BuildColrOptions, PaintFormat as F, Result, Value, build_colr, build_colr_v1, value,
};
use fontoxide_ttlib::Table;
use indexmap::IndexMap;

/// `test_unbuildColrV1`.
#[test]
fn test_unbuild_colr_v1() -> Result<()> {
	let order = test_color_glyphs_order();
	let (layers, base_glyphs) = build_colr_v1(test_color_glyphs(), &ids(&order), false, true)?;
	let color_glyphs = unbuild_colr_v1(layers.as_ref(), &base_glyphs, &order)?;
	assert_eq!(Value::Map(color_glyphs), Value::Map(test_color_glyphs()));
	Ok(())
}

/// `test_unbuildColrV1_noLayers`.
#[test]
fn test_unbuild_colr_v1_no_layers() -> Result<()> {
	let order = test_color_glyphs_order();
	let (_, base_glyphs_v1) = build_colr_v1(test_color_glyphs(), &ids(&order), false, true)?;
	// Just looking to see we don't crash.
	let result = unbuild_colr_v1(None, &base_glyphs_v1, &order);
	// Without the layer list, PaintColrLayers have no layers.
	assert!(result.is_ok());
	Ok(())
}

fn hex(data: &[u8]) -> String {
	data.iter().map(|b| format!("{b:02x}")).collect()
}

fn check_fixture(
	name: &str,
	order: &fontoxide_ttlib::GlyphOrder,
	color_glyphs: IndexMap<String, Value>,
	clip_boxes: Option<IndexMap<String, Value>>,
) -> Result<()> {
	let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
	let expected_xml = std::fs::read_to_string(dir.join(format!("{name}.xml")))
		.map_err(|e| fontoxide_colorlib::Error::Value(e.to_string()))?;
	let expected_hex = std::fs::read_to_string(dir.join(format!("{name}.hex")))
		.map_err(|e| fontoxide_colorlib::Error::Value(e.to_string()))?;

	let mut font = font(order);
	let mut colr = build_colr(
		color_glyphs,
		&ids(order),
		BuildColrOptions {
			clip_boxes,
			..BuildColrOptions::default()
		},
	)?;
	let xml = table_xml(&colr, &mut font)?;
	let expected_body = expected_xml
		.split_once('\n')
		.map_or(expected_xml.as_str(), |(_, b)| b);
	assert_eq!(xml, expected_body, "{name}: TTX");
	let data = colr.compile(&mut font)?;
	assert_eq!(hex(&data), expected_hex.trim(), "{name}: compiled");
	Ok(())
}

/// `buildCOLR(TEST_COLOR_GLYPHS)`, compared with fontTools.
#[test]
fn fixture_unbuilder() -> Result<()> {
	check_fixture(
		"unbuilder",
		&test_color_glyphs_order(),
		test_color_glyphs(),
		None,
	)
}

/// Shared layers, version 0 glyphs, a nudged radial gradient, transforms and
/// clip boxes, with base glyphs out of glyph order; compared with fontTools.
#[test]
fn fixture_mixed() -> Result<()> {
	let order = order(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
	let solid = value!({"Format": F::PaintSolid, "PaletteIndex": 2, "Alpha": 0.8});
	let paints: Vec<Value> = "abcdefgh"
		.chars()
		.map(|c| value!({"Format": F::PaintGlyph, "Paint": solid.clone(), "Glyph": c.to_string()}))
		.collect();
	let color_glyphs = value!({
		"j": [("a", 0), ("b", 1)],
		"c": (F::PaintColrLayers, paints[0..4].to_vec()),
		"b": (F::PaintColrLayers, paints[0..6].to_vec()),
		"d": (F::PaintColrLayers, paints[2..8].to_vec()),
		"i": {
			"Format": F::PaintGlyph,
			"Glyph": "i",
			"Paint": {
				"Format": F::PaintVarRadialGradient,
				"ColorLine": {
					"Extend": "reflect",
					"ColorStop": [(0.0, 3), (0.5, 4, 0.25, 7), (1.0, 5)],
				},
				"x0": 1.4,
				"y0": 0,
				"r0": 0,
				"x1": 2.6,
				"y1": 0,
				"r1": 1.3,
				"VarIndexBase": 9,
			},
		},
		"h": {
			"Format": F::PaintComposite,
			"SourcePaint": {"Format": F::PaintColrGlyph, "Glyph": "c"},
			"CompositeMode": "multiply",
			"BackdropPaint": {
				"Format": F::PaintVarTransform,
				"Paint": (F::PaintGlyph, (F::PaintSolid, 1), "e"),
				"Transform": (1, 0.3333, -0.5, 1.25, 100.5, -20, 4),
			},
		},
		"g": (
			F::PaintColrLayers,
			[
				{
					"Format": F::PaintSweepGradient,
					"ColorLine": {"ColorStop": [(0.0, 0), (1.0, 1, 0.5)]},
					"centerX": 10.6,
					"centerY": -3,
					"startAngle": 45,
					"endAngle": 270.5,
				},
				{
					"Format": F::PaintRotateAroundCenter,
					"Paint": (F::PaintColrGlyph, "b"),
					"angle": -30.25,
					"centerX": 5,
					"centerY": 6,
				},
				{
					"Format": F::PaintVarSkewAroundCenter,
					"Paint": paints[0].clone(),
					"xSkewAngle": 10,
					"ySkewAngle": 0.5,
					"centerX": 0,
					"centerY": 0,
				},
			],
		),
	});
	let clip_boxes = value!({
		"c": (-100.8, -200.4, 1100.1, 1200.5),
		"b": (0, 0, 1000, 1000, 3),
		"i": (0, 0, 10, 10),
		"d": (0, 0, 10, 10),
	});
	let (Value::Map(color_glyphs), Value::Map(clip_boxes)) = (color_glyphs, clip_boxes) else {
		unreachable!()
	};
	check_fixture("mixed", &order, color_glyphs, Some(clip_boxes))
}
