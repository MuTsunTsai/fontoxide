//! Subsetting of `COLR` and `CPAL` (`fontTools.subset`).
//!
//! fontTools turns the paints into plain dicts (`unbuildColrV1`) and builds new
//! tables from them (`buildColrV1`). That only changes the layer list:
//! flattened and shared again. The paints are rebuilt here with
//! [`LayerListBuilder::rebuild_paint`] of colorLib, which does the same without
//! the round trip through dicts.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use fontoxide_colorlib::builder::{child_paints, child_paints_mut, layer_range};
use fontoxide_colorlib::unbuilder::check_unbuild_paint;
use fontoxide_colorlib::{GlyphIds, LayerListBuilder};
use fontoxide_ttlib::ot::types::{self as ot, Paint};
use fontoxide_ttlib::tables::Cpal;
use fontoxide_ttlib::tables::colr::{Colr, LayerRecord};
use fontoxide_ttlib::{Error as TtError, Font, GlyphOrder, Tag};

use crate::{GlyphSet, Result, Subsetter};

type Layers = [Option<Box<Paint>>];

fn no_paint() -> TtError {
	TtError::ttlib("AttributeError: 'NoneType' object has no attribute 'Format'")
}

/// A colorLib error, as the error of the font library that it stands for.
fn colorlib_error(error: fontoxide_colorlib::Error) -> crate::Error {
	match error {
		fontoxide_colorlib::Error::TtLib(e) => e.into(),
		fontoxide_colorlib::Error::Assertion(m) => TtError::Assertion(m).into(),
		other => TtError::ttlib(format!("{}: {other}", other.py_type_name())).into(),
	}
}

/// The palette indices of a paint (`PaletteIndex`, or those of `ColorLine`).
macro_rules! palette_indices {
	($paint:expr, [$($r:tt)*], $as_deref:ident, $iter:ident) => {
		match $paint {
			Paint::Format2(t) => vec![$($r)* t.palette_index],
			Paint::Format3(t) => vec![$($r)* t.palette_index],
			Paint::Format4(t) => palette_indices!(@line t, [$($r)*], $as_deref, $iter),
			Paint::Format5(t) => palette_indices!(@line t, [$($r)*], $as_deref, $iter),
			Paint::Format6(t) => palette_indices!(@line t, [$($r)*], $as_deref, $iter),
			Paint::Format7(t) => palette_indices!(@line t, [$($r)*], $as_deref, $iter),
			Paint::Format8(t) => palette_indices!(@line t, [$($r)*], $as_deref, $iter),
			Paint::Format9(t) => palette_indices!(@line t, [$($r)*], $as_deref, $iter),
			_ => Vec::new(),
		}
	};
	(@line $t:ident, [$($r:tt)*], $as_deref:ident, $iter:ident) => {
		$t.color_line
			.$as_deref()
			.ok_or_else(|| {
				TtError::ttlib("AttributeError: 'NoneType' object has no attribute 'ColorStop'")
			})?
			.color_stop
			.$iter()
			.map(|stop| $($r)* stop.palette_index)
			.collect()
	};
}

fn colors(paint: &Paint) -> Result<Vec<u16>> {
	let indices: Vec<&u16> = palette_indices!(paint, [&], as_deref, iter);
	Ok(indices.into_iter().copied().collect())
}

fn colors_mut(paint: &mut Paint) -> Result<Vec<&mut u16>> {
	Ok(palette_indices!(paint, [&mut], as_deref_mut, iter_mut))
}

/// `Paint.traverse`: walks the paints reachable from some paints.
///
/// LENIENT(L21): fontTools keeps no visited set, and does not end if
/// `PaintColrGlyph`s or layers form a cycle. A layer or base glyph is only
/// walked once here; what is collected from the paints does not depend on how
/// many times they are seen.
struct Traversal<'a> {
	layers: &'a Layers,
	base_glyphs: HashMap<u32, &'a Option<Box<Paint>>>,
	seen_layers: BTreeSet<usize>,
	seen_base_glyphs: HashSet<u32>,
	order: &'a GlyphOrder,
}

impl<'a> Traversal<'a> {
	fn new(table: &'a ot::COLR, order: &'a GlyphOrder) -> Self {
		let mut base_glyphs = HashMap::new();
		if let Some(list) = &table.base_glyph_list {
			for rec in &list.base_glyph_paint_record {
				// iterPaintSubTables uses the first record of a glyph.
				base_glyphs.entry(rec.base_glyph).or_insert(&rec.paint);
			}
		}
		Traversal {
			layers: table
				.layer_list
				.as_ref()
				.map_or(&[], |list| list.paint.as_slice()),
			base_glyphs,
			seen_layers: BTreeSet::new(),
			seen_base_glyphs: HashSet::new(),
			order,
		}
	}

	fn walk(
		&mut self,
		root: &'a Paint,
		visit: &mut impl FnMut(&Paint) -> Result<()>,
	) -> Result<()> {
		let mut stack = vec![root];
		while let Some(paint) = stack.pop() {
			visit(paint)?;
			match paint {
				Paint::Format1(t) => {
					for i in layer_range(t, self.layers) {
						if self.seen_layers.insert(i) {
							stack.push(self.layers[i].as_deref().ok_or_else(no_paint)?);
						}
					}
				}
				Paint::Format11(t) => {
					if self.seen_base_glyphs.insert(t.glyph) {
						let paint = self.base_glyphs.get(&t.glyph).ok_or_else(|| {
							TtError::ttlib(format!(
								"KeyError: \"'{}' not in colr.BaseGlyphList\"",
								self.order.name(t.glyph)
							))
						})?;
						stack.push(paint.as_deref().ok_or_else(no_paint)?);
					}
				}
				_ => {
					for child in child_paints(paint) {
						stack.push(child.as_deref().ok_or_else(no_paint)?);
					}
				}
			}
		}
		Ok(())
	}
}

fn base_glyph_list(table: &ot::COLR) -> Result<&ot::BaseGlyphList> {
	Ok(table.base_glyph_list.as_deref().ok_or_else(|| {
		TtError::ttlib("AttributeError: 'NoneType' object has no attribute 'BaseGlyphPaintRecord'")
	})?)
}

fn v1_table(colr: &Colr) -> Result<&ot::COLR> {
	Ok(colr.table.as_ref().ok_or_else(|| {
		TtError::ttlib("AttributeError: 'table_C_O_L_R_' object has no attribute 'table'")
	})?)
}

/// `closure_glyphs`.
pub(crate) fn closure_glyphs(s: &mut Subsetter, font: &mut Font) -> Result<()> {
	let colr = font.require::<Colr>()?;
	let empty = BTreeMap::new();
	let owned_layers;
	let mut v1 = None;
	let layers_v0: &BTreeMap<u32, Vec<LayerRecord>> = if colr.version > 0 {
		let table = v1_table(colr)?;
		owned_layers = Colr::decompile_color_layers_v0(table)?;
		let mut paints = HashMap::new();
		for rec in &base_glyph_list(table)?.base_glyph_paint_record {
			paints.insert(rec.base_glyph, rec.paint.as_deref());
		}
		v1 = Some((Traversal::new(table, &s.orig_glyph_order), paints));
		&owned_layers
	} else {
		colr.color_layers.as_ref().unwrap_or(&empty)
	};

	let mut glyphs = s.glyphs.clone();
	let mut decompose = glyphs.clone();
	while !decompose.is_empty() {
		let mut layers = GlyphSet::new();
		for g in &decompose {
			if let Some(records) = layers_v0.get(g) {
				layers.extend(records.iter().map(|l| l.glyph));
			}
			if let Some((traversal, paints)) = &mut v1
				&& let Some(Some(paint)) = paints.get(g)
			{
				traversal.walk(paint, &mut |paint| {
					layers.extend(paint_glyph(paint));
					Ok(())
				})?;
			}
		}
		layers.retain(|g| !glyphs.contains(g));
		glyphs.extend(layers.iter().copied());
		decompose = layers;
	}
	s.glyphs = glyphs;
	Ok(())
}

/// `_paint_glyph_names`, for one paint: the glyph it refers to, if it does.
fn paint_glyph(paint: &Paint) -> Option<u32> {
	match paint {
		Paint::Format10(t) => Some(t.glyph),
		Paint::Format11(t) => Some(t.glyph),
		Paint::Format33(t) => Some(t.glyph),
		_ => None,
	}
}

/// `subset_glyphs`.
pub(crate) fn subset_glyphs(s: &Subsetter, font: &mut Font) -> Result<bool> {
	// Only the glyphs after the COLR closure: see fontTools' comment and
	// https://github.com/fonttools/fonttools/issues/2461.
	let glyphs = &s.glyphs_colred;
	let colr = font.require::<Colr>()?;
	if colr.version == 0 {
		let layers = colr.color_layers.get_or_insert_with(BTreeMap::new);
		layers.retain(|g, _| glyphs.contains(g));
		return Ok(!layers.is_empty());
	}

	let table = colr.table.as_mut().ok_or_else(|| {
		TtError::ttlib("AttributeError: 'table_C_O_L_R_' object has no attribute 'table'")
	})?;
	let mut layers_v0 = Colr::decompile_color_layers_v0(table)?;
	layers_v0.retain(|g, _| glyphs.contains(g));

	// unbuildColrV1: a dict, so the last record of a glyph wins, in the place of
	// the first one.
	base_glyph_list(table)?;
	let old_list = table.layer_list.take();
	let old_layers: &Layers = old_list.as_ref().map_or(&[], |list| list.paint.as_slice());
	let old_base = table.base_glyph_list.take();
	let old_records = old_base
		.as_ref()
		.map_or(&[][..], |list| list.base_glyph_paint_record.as_slice());
	let mut index = HashMap::new();
	let mut records: Vec<&ot::BaseGlyphPaintRecord> = Vec::new();
	for rec in old_records {
		check_unbuild_paint(rec.paint.as_deref(), old_layers).map_err(colorlib_error)?;
		match index.get(&rec.base_glyph) {
			Some(&i) => records[i] = rec,
			None => {
				index.insert(rec.base_glyph, records.len());
				records.push(rec);
			}
		}
	}

	// buildColrV1, without a glyphMap.
	let no_glyph_names = GlyphIds::new();
	let mut builder = LayerListBuilder::new(&no_glyph_names, false, true);
	let mut base_records = Vec::new();
	for rec in records
		.iter()
		.filter(|rec| glyphs.contains(&rec.base_glyph))
	{
		let paint = rec.paint.as_deref().ok_or_else(no_paint)?;
		let paint = builder
			.rebuild_paint(paint, old_layers)
			.map_err(colorlib_error)?;
		base_records.push(ot::BaseGlyphPaintRecord {
			base_glyph: rec.base_glyph,
			paint: Some(Box::new(paint)),
		});
	}
	let layer_list = builder.build();

	if base_records.is_empty() {
		// No more COLRv1 glyphs: downgrade to version 0.
		let retain = !layers_v0.is_empty();
		colr.version = 0;
		colr.table = None;
		colr.color_layers = Some(layers_v0);
		return Ok(retain);
	}

	table.layer_list = layer_list.map(Box::new);
	table.base_glyph_list = Some(Box::new(ot::BaseGlyphList {
		base_glyph_count: Some(base_records.len() as u32),
		base_glyph_paint_record: base_records,
	}));
	if let Some(clip_list) = &mut table.clip_list {
		clip_list.clips.retain(|g, _| glyphs.contains(g));
	}
	// UPSTREAM-BUG(B19): fontTools goes through the glyphs in the order of a set
	// of strings, which changes from one run to the next, to lay out the layer
	// records. They are in glyph order here, as those of version 0 tables are.
	Colr::populate_color_layers_v0(table, &layers_v0)?;
	Ok(true)
}

/// The palette indices that a COLR table uses, and the layers that have them.
fn used_colors(colr: &Colr, order: &GlyphOrder) -> Result<(BTreeSet<u16>, BTreeSet<usize>)> {
	let mut used = BTreeSet::new();
	if colr.version == 0 {
		for layers in colr.color_layers.iter().flat_map(|m| m.values()) {
			used.extend(layers.iter().map(|l| l.color_id));
		}
		return Ok((used, BTreeSet::new()));
	}
	let table = v1_table(colr)?;
	if let Some(array) = &table.layer_record_array {
		used.extend(array.layer_record.iter().map(|l| l.palette_index));
	}
	let mut traversal = Traversal::new(table, order);
	for rec in &base_glyph_list(table)?.base_glyph_paint_record {
		let paint = rec.paint.as_deref().ok_or_else(no_paint)?;
		traversal.walk(paint, &mut |paint| {
			used.extend(colors(paint)?);
			Ok(())
		})?;
	}
	Ok((used, traversal.seen_layers))
}

/// Changes the palette indices of a paint and its child paints.
fn remap_colors(paint: &mut Paint, map: &BTreeMap<u16, u16>) -> Result<()> {
	for index in colors_mut(paint)? {
		if let Some(&new) = map.get(index) {
			*index = new;
		}
	}
	for child in child_paints_mut(paint) {
		remap_colors(child.as_deref_mut().ok_or_else(no_paint)?, map)?;
	}
	Ok(())
}

/// `prune_post_subset` of `CPAL`.
pub(crate) fn cpal_prune_post_subset(s: &Subsetter, font: &mut Font) -> Result<bool> {
	// Keep the whole table if "SVG " is present, as it may be referenced by the
	// latter via 'var(--color{palette_entry_index}, ...)' CSS color variables.
	if font.contains(Tag::new(b"SVG ")) {
		return Ok(true);
	}
	// Drop CPAL if COLR was subsetted to empty.
	let Some(colr) = font.get::<Colr>()? else {
		return Ok(false);
	};
	let (mut retained, seen_layers) = used_colors(colr, &s.orig_glyph_order)?;
	// Don't remap palette entry index 0xFFFF: this is always the foreground color.
	retained.remove(&0xFFFF);
	let map: BTreeMap<u16, u16> = retained.iter().copied().zip(0..).collect();

	if colr.version == 0 {
		for layers in colr.color_layers.iter_mut().flat_map(|m| m.values_mut()) {
			for layer in layers {
				if let Some(&new) = map.get(&layer.color_id) {
					layer.color_id = new;
				}
			}
		}
	} else if let Some(table) = &mut colr.table {
		if let Some(array) = &mut table.layer_record_array {
			for layer in &mut array.layer_record {
				if let Some(&new) = map.get(&layer.palette_index) {
					layer.palette_index = new;
				}
			}
		}
		if let Some(list) = &mut table.base_glyph_list {
			for rec in &mut list.base_glyph_paint_record {
				if let Some(paint) = rec.paint.as_deref_mut() {
					remap_colors(paint, &map)?;
				}
			}
		}
		if let Some(list) = &mut table.layer_list {
			for i in seen_layers {
				if let Some(paint) = list.paint[i].as_deref_mut() {
					remap_colors(paint, &map)?;
				}
			}
		}
	}

	let cpal = font.require::<Cpal>()?;
	for palette in &mut cpal.palettes {
		let mut i = 0;
		palette.retain(|_| {
			let keep = i <= u16::MAX as usize && retained.contains(&(i as u16));
			i += 1;
			keep
		});
		if palette.len() != retained.len() {
			return Err(
				TtError::Assertion("len(palette) == len(retained_palette_indices)".into()).into(),
			);
		}
	}
	let first = cpal
		.palettes
		.first()
		.ok_or_else(|| TtError::ttlib("IndexError: list index out of range"))?;
	cpal.num_palette_entries = first.len() as u16;
	if cpal.version == 1 {
		let mut i = 0;
		cpal.palette_entry_labels.retain(|_| {
			let keep = i <= u16::MAX as usize && retained.contains(&(i as u16));
			i += 1;
			keep
		});
	}
	Ok(cpal.num_palette_entries != 0)
}

#[cfg(test)]
mod tests {
	/// `Tests/subset/subset_test.py::test_colr_paint_glyph2_closure`.
	#[test]
	fn test_colr_paint_glyph2_closure() {
		let paint = Paint::Format33(fontoxide_ttlib::ot::types::PaintFormat33 {
			paint: Some(Box::new(Paint::Format2(
				fontoxide_ttlib::ot::types::PaintFormat2 {
					palette_index: 0,
					alpha: 1.0,
				},
			))),
			glyph: 0x10000,
		});
		assert_eq!(super::paint_glyph(&paint), Some(0x10000));
	}

	use super::*;
}
