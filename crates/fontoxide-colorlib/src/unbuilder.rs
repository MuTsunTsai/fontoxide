//! Turn COLRv1 tables back into the dicts that the builder takes
//! (`fontTools.colorLib.unbuilder`).

use fontoxide_ttlib::GlyphOrder;
use fontoxide_ttlib::ot::types::{self as ot, Paint};
use indexmap::IndexMap;

use crate::builder::{child_paints, layer_range};
use crate::generated::{Class, TableRef};
use crate::table_builder::{MAX_DEPTH, TableUnbuilder, UnbuildCallbacks};
use crate::{Error, Result, Value};

/// `unbuildColrV1`: the paint of each base glyph, as a dict whose
/// `PaintColrLayers` have their layers (flattened) instead of a slice of the
/// layer list. Glyphs are written as names of `order`.
///
/// Like the dict comprehension of fontTools, the last record of a base glyph
/// wins, in the place of its first one.
pub fn unbuild_colr_v1(
	layer_list: Option<&ot::LayerList>,
	base_glyph_list: &ot::BaseGlyphList,
	order: &GlyphOrder,
) -> Result<IndexMap<String, Value>> {
	let layers = layer_list.map_or(&[][..], |l| l.paint.as_slice());
	let mut unbuilder = LayerListUnbuilder::new(layers, order);
	let mut color_glyphs = IndexMap::new();
	for rec in &base_glyph_list.base_glyph_paint_record {
		let paint = unbuilder.unbuild_paint(rec.paint.as_deref())?;
		color_glyphs.insert(order.name(rec.base_glyph).into_owned(), paint);
	}
	Ok(color_glyphs)
}

/// `_flatten_layers`: the layers of unbuilt `PaintColrLayers` in place of
/// them.
fn flatten_layers(list: Vec<Value>, out: &mut Vec<Value>) {
	for paint in list {
		match paint {
			Value::Map(mut map) if map.get("Format") == Some(&Value::Int(1)) => {
				match map.shift_remove("Layers") {
					Some(Value::List(layers)) => flatten_layers(layers, out),
					_ => out.push(Value::Map(map)),
				}
			}
			other => out.push(other),
		}
	}
}

/// `LayerListUnbuilder`.
#[derive(Debug, Clone)]
pub struct LayerListUnbuilder<'a> {
	/// The layer list (`layers`).
	pub layers: &'a [Option<Box<Paint>>],
	table_unbuilder: TableUnbuilder<'a>,
}

impl<'a> LayerListUnbuilder<'a> {
	/// An unbuilder of paints whose `PaintColrLayers` refer to `layers`.
	pub fn new(layers: &'a [Option<Box<Paint>>], order: &'a GlyphOrder) -> Self {
		LayerListUnbuilder {
			layers,
			table_unbuilder: TableUnbuilder::new(order),
		}
	}

	/// `unbuildPaint`.
	pub fn unbuild_paint(&mut self, paint: Option<&Paint>) -> Result<Value> {
		let paint = paint.ok_or_else(not_a_paint)?;
		let mut unbuilder = self.table_unbuilder.clone();
		let mut callbacks = ColrLayers {
			layers: self.layers,
		};
		unbuilder.unbuild(TableRef::Paint(paint), &mut callbacks)
	}
}

fn not_a_paint() -> Error {
	Error::Assertion("isinstance(paint, ot.Paint)".into())
}

/// The callback of `LayerListUnbuilder`: `_unbuildPaintColrLayers`.
struct ColrLayers<'a> {
	layers: &'a [Option<Box<Paint>>],
}

impl UnbuildCallbacks for ColrLayers<'_> {
	fn unbuilt(
		&mut self,
		unbuilder: &mut TableUnbuilder<'_>,
		class: Class,
		format: Option<u16>,
		source: IndexMap<String, Value>,
	) -> Result<Value> {
		if class != Class::Paint || format != Some(1) {
			return Ok(Value::Map(source));
		}
		let index = |name: &str| -> Result<u32> {
			match source.get(name) {
				Some(&Value::Int(v)) => Ok(u32::try_from(v).unwrap_or(u32::MAX)),
				_ => Err(Error::key(name)),
			}
		};
		let colr_layers = ot::PaintFormat1 {
			first_layer_index: index("FirstLayerIndex")?,
			num_layers: u8::try_from(index("NumLayers")?).unwrap_or(u8::MAX),
		};
		let layers = self.layers;
		let mut unbuilt = Vec::new();
		for child in &layers[layer_range(&colr_layers, layers)] {
			let child = child.as_deref().ok_or_else(not_a_paint)?;
			unbuilt.push(unbuilder.unbuild(TableRef::Paint(child), self)?);
		}
		let mut flat = Vec::new();
		flatten_layers(unbuilt, &mut flat);

		if flat.len() == 1 {
			return Ok(flat.remove(0));
		}
		let mut result = IndexMap::new();
		result.insert("Format".to_owned(), Value::Int(1));
		result.insert("Layers".to_owned(), Value::List(flat));
		Ok(Value::Map(result))
	}
}

/// The errors that `LayerListUnbuilder.unbuildPaint` gives for a paint read
/// from a font, without unbuilding it: a missing paint, or paints nesting too
/// deeply (or in a cycle). For use with
/// [`LayerListBuilder::rebuild_paint`](crate::LayerListBuilder::rebuild_paint).
pub fn check_unbuild_paint(paint: Option<&Paint>, layers: &[Option<Box<Paint>>]) -> Result<()> {
	check_unbuild(paint, layers, 0)
}

fn check_unbuild(paint: Option<&Paint>, layers: &[Option<Box<Paint>>], depth: usize) -> Result<()> {
	let Some(paint) = paint else {
		return Err(not_a_paint());
	};
	if depth > MAX_DEPTH {
		return Err(Error::Recursion("maximum recursion depth exceeded".into()));
	}
	if let Paint::Format1(t) = paint {
		for layer in &layers[layer_range(t, layers)] {
			check_unbuild(layer.as_deref(), layers, depth + 1)?;
		}
	}
	for child in child_paints(paint) {
		check_unbuild(child.as_deref(), layers, depth + 1)?;
	}
	Ok(())
}
