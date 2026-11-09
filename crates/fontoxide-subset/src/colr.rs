//! Subsetting of `COLR` and `CPAL` (`fontTools.subset`), and the parts of
//! `fontTools.colorLib` it uses to rebuild the COLRv1 paint graph
//! (`unbuildColrV1`, `buildColrV1`, `LayerReuseCache`, `build_n_ary_tree`).
//!
//! fontTools turns the paints into plain dicts and builds new tables from them.
//! That only changes the layer list: flattened and shared again. Fields keep
//! their values: `_beforeBuildPaintRadialGradient` rounds the start circle, but
//! is the identity on the integers that a table read from a font has.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use fontoxide_ttlib::ot::types::{self as ot, Paint};
use fontoxide_ttlib::tables::Cpal;
use fontoxide_ttlib::tables::colr::{Colr, LayerRecord};
use fontoxide_ttlib::{Error as TtError, Font, GlyphOrder, Tag};

use crate::{GlyphSet, Result, Subsetter};

/// `MAX_PAINT_COLR_LAYER_COUNT`.
const MAX_PAINT_COLR_LAYER_COUNT: usize = 255;
/// `_MAX_REUSE_LEN`; `_reuse_ranges` yields one more than this at most.
const MAX_REUSE_LEN: usize = 32;
/// How deep paints may nest when unbuilding.
const MAX_DEPTH: usize = 100;

type Layers = [Option<Box<Paint>>];

fn no_paint() -> TtError {
	TtError::ttlib("AttributeError: 'NoneType' object has no attribute 'Format'")
}

/// A shared or mutable reference, for `child_paints!`.
macro_rules! reference {
	(shared, $e:expr) => {
		&$e
	};
	(mutable, $e:expr) => {
		&mut $e
	};
}

/// The fields holding the child paints of a paint, in the order of its
/// converters.
macro_rules! child_paints {
	($paint:expr, $kind:ident) => {
		child_paints!(@arms $paint, $kind,
			Format10, Format12, Format13, Format14, Format15, Format16, Format17, Format18,
			Format19, Format20, Format21, Format22, Format23, Format24, Format25, Format26,
			Format27, Format28, Format29, Format30, Format31, Format33)
	};
	(@arms $paint:expr, $kind:ident, $($format:ident),*) => {
		match $paint {
			$(Paint::$format(t) => vec![reference!($kind, t.paint)],)*
			Paint::Format32(t) => {
				vec![reference!($kind, t.source_paint), reference!($kind, t.backdrop_paint)]
			}
			_ => Vec::new(),
		}
	};
}

fn children(paint: &Paint) -> Vec<&Option<Box<Paint>>> {
	child_paints!(paint, shared)
}

fn children_mut(paint: &mut Paint) -> Vec<&mut Option<Box<Paint>>> {
	child_paints!(paint, mutable)
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

/// Sets `StopCount`, as `TableBuilder` does.
fn set_stop_count(paint: &mut Paint) {
	macro_rules! arms {
		($($format:ident),*) => {
			match paint {
				$(Paint::$format(t) => {
					if let Some(line) = &mut t.color_line {
						line.stop_count = Some(line.color_stop.len() as u32);
					}
				})*
				_ => {}
			}
		};
	}
	arms!(Format4, Format5, Format6, Format7, Format8, Format9);
}

/// `layers[FirstLayerIndex : FirstLayerIndex + NumLayers]`, as a range of
/// `layers`.
fn layer_range(t: &ot::PaintFormat1, layers: &Layers) -> std::ops::Range<usize> {
	let start = (t.first_layer_index as usize).min(layers.len());
	let end = (start + t.num_layers as usize).min(layers.len());
	start..end
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
					for child in children(paint) {
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
					// `_paint_glyph_names`.
					match paint {
						Paint::Format10(t) => layers.insert(t.glyph),
						Paint::Format11(t) => layers.insert(t.glyph),
						Paint::Format33(t) => layers.insert(t.glyph),
						_ => false,
					};
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

/// The errors that `LayerListUnbuilder.unbuildPaint` gives.
fn check_unbuild(paint: Option<&Paint>, layers: &Layers, depth: usize) -> Result<()> {
	let Some(paint) = paint else {
		return Err(TtError::Assertion("isinstance(paint, ot.Paint)".into()).into());
	};
	if depth > MAX_DEPTH {
		return Err(TtError::ttlib("RecursionError: maximum recursion depth exceeded").into());
	}
	if let Paint::Format1(t) = paint {
		for layer in &layers[layer_range(t, layers)] {
			check_unbuild(layer.as_deref(), layers, depth + 1)?;
		}
	}
	for child in children(paint) {
		check_unbuild(child.as_deref(), layers, depth + 1)?;
	}
	Ok(())
}

/// `_flatten_layers` of an unbuilt `PaintColrLayers`, whose `Layers` are
/// flattened already.
fn flatten_layers<'p>(paint: &'p ot::PaintFormat1, layers: &'p Layers, out: &mut Vec<&'p Paint>) {
	for layer in layers[layer_range(paint, layers)].iter().flatten() {
		match &**layer {
			Paint::Format1(t) => flatten_layers(t, layers, out),
			other => out.push(other),
		}
	}
}

/// A node of `build_n_ary_tree`.
enum Tree {
	Leaf(Paint),
	Node(Vec<Tree>),
}

/// `build_n_ary_tree`.
fn build_n_ary_tree(mut leaves: Vec<Tree>, n: usize) -> Vec<Tree> {
	if leaves.is_empty() {
		return leaves;
	}
	let depth = ((leaves.len() as f64).ln() / (n as f64).ln()).ceil() as u32;
	if depth <= 1 {
		return leaves;
	}
	// Fully populate complete subtrees of root until we have enough leaves left.
	let mut root = Vec::new();
	let mut unassigned = None;
	let full_step = n.pow(depth - 1);
	while !leaves.is_empty() {
		let rest = leaves.split_off(full_step.min(leaves.len()));
		let mut subtree = std::mem::replace(&mut leaves, rest);
		if subtree.len() < full_step {
			unassigned = Some(subtree);
			break;
		}
		while subtree.len() > n {
			let mut nodes = Vec::with_capacity(subtree.len() / n + 1);
			while !subtree.is_empty() {
				let rest = subtree.split_off(n.min(subtree.len()));
				nodes.push(Tree::Node(std::mem::replace(&mut subtree, rest)));
			}
			subtree = nodes;
		}
		root.push(Tree::Node(subtree));
	}
	if let Some(unassigned) = unassigned {
		// Recurse to fill the last subtree, the only partially populated one.
		let subtree = build_n_ary_tree(unassigned, n);
		if subtree.len() <= n - root.len() {
			root.extend(subtree);
		} else {
			root.push(Tree::Node(subtree));
		}
	}
	root
}

/// `LayerListBuilder`, with its `LayerReuseCache`.
///
/// The cache compares paints by value. Here each distinct paint gets a number,
/// so that slices of layers are compared and hashed cheaply.
struct LayerListBuilder<'a> {
	old_layers: &'a Layers,
	layers: Vec<Option<Box<Paint>>>,
	numbers: HashMap<String, u32>,
	/// `reusePool`.
	reuse_pool: HashMap<Box<[u32]>, u32>,
}

impl<'a> LayerListBuilder<'a> {
	fn new(old_layers: &'a Layers) -> Self {
		LayerListBuilder {
			old_layers,
			layers: Vec::new(),
			numbers: HashMap::new(),
			reuse_pool: HashMap::new(),
		}
	}

	/// `buildPaint` of an unbuilt paint.
	fn build_paint(&mut self, paint: &Paint) -> Result<Paint> {
		let mut paint = match paint {
			Paint::Format1(t) => {
				let old_layers = self.old_layers;
				let mut flat = Vec::new();
				flatten_layers(t, old_layers, &mut flat);
				if let [only] = flat[..] {
					return self.build_paint(only);
				}
				let mut layers = Vec::with_capacity(flat.len());
				for layer in flat {
					layers.push(self.build_paint(layer)?);
				}
				return self.colr_layers(layers);
			}
			Paint::Unknown(format) => {
				return Err(TtError::Assertion(format!(
					"<class 'fontTools.ttLib.tables.otTables.Paint'> invalid Format: {format}"
				))
				.into());
			}
			other => other.clone(),
		};
		for slot in children_mut(&mut paint) {
			let child = slot.take().ok_or_else(no_paint)?;
			*slot = Some(Box::new(self.build_paint(&child)?));
		}
		set_stop_count(&mut paint);
		Ok(paint)
	}

	fn number(&mut self, paint: &Paint) -> u32 {
		let next = self.numbers.len() as u32;
		*self.numbers.entry(format!("{paint:?}")).or_insert(next)
	}

	/// `LayerReuseCache.try_reuse`: looks for reuse, preferring longer
	/// sequences, and later ones among those of the same length.
	fn try_reuse(&mut self, layers: &mut Vec<Paint>, numbers: &mut Vec<u32>) {
		'found: loop {
			let n = layers.len();
			for len in (2..=n.min(MAX_REUSE_LEN + 1)).rev() {
				for ubound in (len..=n).rev() {
					let lbound = ubound - len;
					let Some(&first) = self.reuse_pool.get(&numbers[lbound..ubound]) else {
						continue;
					};
					let slice = Paint::Format1(ot::PaintFormat1 {
						num_layers: len as u8,
						first_layer_index: first,
					});
					let number = self.number(&slice);
					layers.splice(lbound..ubound, [slice]);
					numbers.splice(lbound..ubound, [number]);
					continue 'found;
				}
			}
			return;
		}
	}

	/// `LayerReuseCache.add`.
	fn add(&mut self, numbers: &[u32], first_layer_index: usize) {
		for lbound in 0..numbers.len() {
			for ubound in lbound + 2..=numbers.len().min(lbound + 1 + MAX_REUSE_LEN) {
				let first = (lbound + first_layer_index) as u32;
				match self.reuse_pool.get_mut(&numbers[lbound..ubound]) {
					Some(value) => *value = first,
					None => {
						self.reuse_pool
							.insert(numbers[lbound..ubound].into(), first);
					}
				}
			}
		}
	}

	/// `listToColrLayers`.
	fn tree_to_paint(&mut self, node: Tree) -> Result<Paint> {
		match node {
			Tree::Leaf(paint) => Ok(paint),
			Tree::Node(nodes) => {
				let mut layers = Vec::with_capacity(nodes.len());
				for node in nodes {
					layers.push(self.tree_to_paint(node)?);
				}
				self.colr_layers(layers)
			}
		}
	}

	/// `_beforeBuildPaintColrLayers`, given the built layers.
	fn colr_layers(&mut self, mut layers: Vec<Paint>) -> Result<Paint> {
		// No reason to have a colr layers with just one entry.
		if layers.len() == 1 {
			return Ok(layers.remove(0));
		}
		let mut numbers: Vec<u32> = layers.iter().map(|p| self.number(p)).collect();
		self.try_reuse(&mut layers, &mut numbers);

		// The layer list is now final; if it's too big we need to tree it.
		let is_tree = layers.len() > MAX_PAINT_COLR_LAYER_COUNT;
		let leaves = layers.into_iter().map(Tree::Leaf).collect();
		let mut layers = Vec::new();
		for node in build_n_ary_tree(leaves, MAX_PAINT_COLR_LAYER_COUNT) {
			layers.push(self.tree_to_paint(node)?);
		}
		if layers.len() == 1 {
			return Ok(layers.remove(0));
		}

		let first = self.layers.len();
		let paint = Paint::Format1(ot::PaintFormat1 {
			num_layers: layers.len() as u8,
			first_layer_index: first as u32,
		});
		self.layers
			.extend(layers.into_iter().map(|p| Some(Box::new(p))));
		// If we are a tree the leaves registered for reuse and that will suffice.
		if !is_tree {
			self.add(&numbers, first);
		}
		Ok(paint)
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
		check_unbuild(rec.paint.as_deref(), old_layers, 0)?;
		match index.get(&rec.base_glyph) {
			Some(&i) => records[i] = rec,
			None => {
				index.insert(rec.base_glyph, records.len());
				records.push(rec);
			}
		}
	}

	// buildColrV1.
	let mut builder = LayerListBuilder::new(old_layers);
	let mut base_records = Vec::new();
	for rec in records
		.iter()
		.filter(|rec| glyphs.contains(&rec.base_glyph))
	{
		let paint = rec.paint.as_deref().ok_or_else(no_paint)?;
		base_records.push(ot::BaseGlyphPaintRecord {
			base_glyph: rec.base_glyph,
			paint: Some(Box::new(builder.build_paint(paint)?)),
		});
	}
	let new_layers = std::mem::take(&mut builder.layers);
	drop(builder);

	if base_records.is_empty() {
		// No more COLRv1 glyphs: downgrade to version 0.
		let retain = !layers_v0.is_empty();
		colr.version = 0;
		colr.table = None;
		colr.color_layers = Some(layers_v0);
		return Ok(retain);
	}

	table.layer_list = (!new_layers.is_empty()).then(|| {
		Box::new(ot::LayerList {
			layer_count: Some(new_layers.len() as u32),
			paint: new_layers,
		})
	});
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
	for child in children_mut(paint) {
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
	use super::*;

	/// The shape of a tree: the number of leaves or a list of shapes.
	fn shape(nodes: &[Tree]) -> String {
		let mut out = String::new();
		let mut leaves = 0;
		for node in nodes {
			match node {
				Tree::Leaf(_) => leaves += 1,
				Tree::Node(nodes) => {
					if leaves > 0 {
						out += &format!("{leaves} ");
						leaves = 0;
					}
					out += &format!("[{}] ", shape(nodes));
				}
			}
		}
		if leaves > 0 {
			out += &format!("{leaves}");
		}
		out.trim_end().to_owned()
	}

	fn tree(leaves: usize, n: usize) -> String {
		let leaves = (0..leaves).map(|_| Tree::Leaf(Paint::Unknown(0))).collect();
		shape(&build_n_ary_tree(leaves, n))
	}

	/// Cases of `Tests/misc/treeTools_test.py`.
	#[test]
	fn n_ary_tree() {
		assert_eq!(tree(0, 3), "");
		assert_eq!(tree(1, 2), "1");
		assert_eq!(tree(2, 2), "2");
		assert_eq!(tree(3, 2), "[2] 1");
		assert_eq!(tree(3, 3), "3");
		assert_eq!(tree(4, 2), "[2] [2]");
		assert_eq!(tree(4, 3), "[3] 1");
		assert_eq!(tree(5, 3), "[3] 2");
		assert_eq!(tree(6, 3), "[3] [3]");
		assert_eq!(tree(7, 3), "[3] [3] 1");
		assert_eq!(tree(8, 3), "[3] [3] [2]");
		assert_eq!(tree(9, 3), "[3] [3] [3]");
		assert_eq!(tree(10, 3), "[[3] [3] [3]] 1");
		assert_eq!(tree(11, 3), "[[3] [3] [3]] 2");
		assert_eq!(tree(12, 3), "[[3] [3] [3]] [3]");
		assert_eq!(tree(13, 3), "[[3] [3] [3]] [3] 1");
		assert_eq!(tree(14, 3), "[[3] [3] [3]] [[3] 2]");
		assert_eq!(tree(15, 3), "[[3] [3] [3]] [3] [3]");
		assert_eq!(tree(16, 3), "[[3] [3] [3]] [[3] [3] 1]");
	}
}
