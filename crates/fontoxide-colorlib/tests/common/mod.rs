//! Helpers shared by the tests.

#![allow(dead_code)]

use fontoxide_colorlib::table_builder::{NoCallbacks, TableUnbuilder};
use fontoxide_colorlib::{GlyphIds, Result, TableRef, Value};
use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_ttlib::ot::types::Paint;
use fontoxide_ttlib::{Font, GlyphOrder, Table};

/// A glyph order of the names, after `.notdef`.
pub fn order(names: &[&str]) -> GlyphOrder {
	let mut all = vec![".notdef".to_owned()];
	all.extend(names.iter().map(|n| (*n).to_owned()));
	GlyphOrder::new(all)
}

/// The glyph name to ID map of an order.
pub fn ids(order: &GlyphOrder) -> GlyphIds {
	order.reverse_map().clone()
}

/// A font with a glyph order.
pub fn font(order: &GlyphOrder) -> Font {
	let mut font = Font::new();
	font.set_glyph_order(order.names().to_vec());
	font
}

/// The attributes of a paint, as a dict (unbuilt, glyphs as names).
pub fn attrs(paint: &Paint, order: &GlyphOrder) -> Result<Value> {
	TableUnbuilder::new(order).unbuild(TableRef::Paint(paint), &mut NoCallbacks)
}

/// `value[key]` of a dict.
pub fn at<'v>(value: &'v Value, key: &str) -> &'v Value {
	match value {
		Value::Map(map) => map
			.get(key)
			.unwrap_or_else(|| panic!("no {key} in {value:?}")),
		other => panic!("not a dict: {other:?}"),
	}
}

/// `value[path[0]][path[1]]...` of nested dicts.
pub fn path<'v>(value: &'v Value, keys: &[&str]) -> &'v Value {
	keys.iter().fold(value, |v, k| at(v, k))
}

/// The TTX of a table, without the XML declaration.
pub fn table_xml(table: &dyn Table, font: &mut Font) -> Result<String> {
	let mut writer = XmlWriter::with_options("  ", "\n");
	fontoxide_ttlib::table::table_to_xml(table, &mut writer, font)?;
	let text = writer.into_string();
	Ok(text
		.split_once('\n')
		.map_or(text.clone(), |(_, b)| b.to_owned()))
}

/// `Tests/colorLib/unbuilder_test.py::TEST_COLOR_GLYPHS`.
pub fn test_color_glyphs() -> indexmap::IndexMap<String, Value> {
	use fontoxide_colorlib::{PaintFormat as F, value};

	let many_layers: Vec<Value> = (0..256)
		.map(|i| {
			value!({
				"Format": F::PaintGlyph,
				"Paint": {
					"Format": F::PaintSolid,
					"PaletteIndex": i,
					"Alpha": 1.0,
				},
				// Not an f-string upstream.
				"Glyph": "glyph{str(18 + i).zfill(5)}",
			})
		})
		.collect();

	let value = value!({
		"glyph00010": {
			"Format": F::PaintColrLayers,
			"Layers": [
				{
					"Format": F::PaintGlyph,
					"Paint": {
						"Format": F::PaintSolid,
						"PaletteIndex": 2,
						"Alpha": 0.5,
					},
					"Glyph": "glyph00011",
				},
				{
					"Format": F::PaintGlyph,
					"Paint": {
						"Format": F::PaintVarLinearGradient,
						"ColorLine": {
							"Extend": "repeat",
							"ColorStop": [
								{
									"StopOffset": 0.0,
									"PaletteIndex": 3,
									"Alpha": 1.0,
									"VarIndexBase": 0,
								},
								{
									"StopOffset": 0.5,
									"PaletteIndex": 4,
									"Alpha": 1.0,
									"VarIndexBase": 1,
								},
								{
									"StopOffset": 1.0,
									"PaletteIndex": 5,
									"Alpha": 1.0,
									"VarIndexBase": 2,
								},
							],
						},
						"x0": 1,
						"y0": 2,
						"x1": -3,
						"y1": -4,
						"x2": 5,
						"y2": 6,
						"VarIndexBase": 0xFFFFFFFFu32,
					},
					"Glyph": "glyph00012",
				},
				{
					"Format": F::PaintGlyph,
					"Paint": {
						"Format": F::PaintVarTransform,
						"Paint": {
							"Format": F::PaintRadialGradient,
							"ColorLine": {
								"Extend": "pad",
								"ColorStop": [
									{
										"StopOffset": 0,
										"PaletteIndex": 6,
										"Alpha": 1.0,
									},
									{
										"StopOffset": 1.0,
										"PaletteIndex": 7,
										"Alpha": 0.4,
									},
								],
							},
							"x0": 7,
							"y0": 8,
							"r0": 9,
							"x1": 10,
							"y1": 11,
							"r1": 12,
						},
						"Transform": {
							"xx": -13.0,
							"yx": 14.0,
							"xy": 15.0,
							"yy": -17.0,
							"dx": 18.0,
							"dy": 19.0,
							"VarIndexBase": 3,
						},
					},
					"Glyph": "glyph00013",
				},
				{
					"Format": F::PaintVarTranslate,
					"Paint": {
						"Format": F::PaintRotate,
						"Paint": {
							"Format": F::PaintVarSkew,
							"Paint": {
								"Format": F::PaintGlyph,
								"Paint": {
									"Format": F::PaintSolid,
									"PaletteIndex": 2,
									"Alpha": 0.5,
								},
								"Glyph": "glyph00011",
							},
							"xSkewAngle": -11.0,
							"ySkewAngle": 5.0,
							"VarIndexBase": 4,
						},
						"angle": 45.0,
					},
					"dx": 257.0,
					"dy": 258.0,
					"VarIndexBase": 5,
				},
			],
		},
		"glyph00014": {
			"Format": F::PaintComposite,
			"SourcePaint": {
				"Format": F::PaintColrGlyph,
				"Glyph": "glyph00010",
			},
			"CompositeMode": "src_over",
			"BackdropPaint": {
				"Format": F::PaintTransform,
				"Paint": {
					"Format": F::PaintColrGlyph,
					"Glyph": "glyph00010",
				},
				"Transform": {
					"xx": 1.0,
					"yx": 0.0,
					"xy": 0.0,
					"yy": 1.0,
					"dx": 300.0,
					"dy": 0.0,
				},
			},
		},
		"glyph00015": {
			"Format": F::PaintGlyph,
			"Paint": {
				"Format": F::PaintSweepGradient,
				"ColorLine": {
					"Extend": "pad",
					"ColorStop": [
						{
							"StopOffset": 0.0,
							"PaletteIndex": 3,
							"Alpha": 1.0,
						},
						{
							"StopOffset": 1.0,
							"PaletteIndex": 5,
							"Alpha": 1.0,
						},
					],
				},
				"centerX": 259,
				"centerY": 300,
				"startAngle": 45.0,
				"endAngle": 135.0,
			},
			"Glyph": "glyph00011",
		},
		"glyph00016": {
			"Format": F::PaintColrLayers,
			"Layers": [
				{
					"Format": F::PaintGlyph,
					"Paint": {
						"Format": F::PaintVarSolid,
						"PaletteIndex": 2,
						"Alpha": 0.5,
						"VarIndexBase": 6,
					},
					"Glyph": "glyph00011",
				},
				{
					"Format": F::PaintGlyph,
					"Paint": {
						"Format": F::PaintVarLinearGradient,
						"ColorLine": {
							"Extend": "repeat",
							"ColorStop": [
								{
									"StopOffset": 0.0,
									"PaletteIndex": 3,
									"Alpha": 1.0,
									"VarIndexBase": 7,
								},
								{
									"StopOffset": 0.5,
									"PaletteIndex": 4,
									"Alpha": 1.0,
									"VarIndexBase": 8,
								},
								{
									"StopOffset": 1.0,
									"PaletteIndex": 5,
									"Alpha": 1.0,
									"VarIndexBase": 9,
								},
							],
						},
						"x0": 1,
						"y0": 2,
						"x1": -3,
						"y1": -4,
						"x2": 5,
						"y2": 6,
						"VarIndexBase": 0xFFFFFFFFu32,
					},
					"Glyph": "glyph00012",
				},
			],
		},
		// When PaintColrLayers contains more than 255 layers, a tree of nested
		// PaintColrLayers of max 255 items is built (NumLayers field is a uint8).
		// unbuildColrV1 restores a flat list of layers without nested
		// PaintColrLayers.
		"glyph00017": {
			"Format": F::PaintColrLayers,
			"Layers": Value::List(many_layers),
		},
	});
	match value {
		Value::Map(map) => map,
		_ => indexmap::IndexMap::new(),
	}
}

/// The glyph order of [`test_color_glyphs`].
pub fn test_color_glyphs_order() -> GlyphOrder {
	let mut names: Vec<String> = (1..18).map(|i| format!("glyph{i:05}")).collect();
	names.push("glyph{str(18 + i).zfill(5)}".to_owned());
	let names: Vec<&str> = names.iter().map(String::as_str).collect();
	order(&names)
}
