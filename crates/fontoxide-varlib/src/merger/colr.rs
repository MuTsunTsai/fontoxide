//! `COLRVariationMerger`: merges `COLR` version 1 tables into a variable one.
//!
//! The merged tables change type as they become variable (`Affine2x3` to
//! `VarAffine2x3`, `PaintSolid` to `PaintVarSolid`...): in Rust these are other
//! types, so the paints are merged here by format rather than by the generic
//! walk.

use fontoxide_colorlib::builder::MAX_PAINT_COLR_LAYER_COUNT;
use fontoxide_misc::fixed_tools::{fixed_to_float, float_to_fixed};
use fontoxide_misc::py::str_repr;
use fontoxide_misc::tree_tools::{Tree, build_n_ary_tree};
use fontoxide_ttlib::Font;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::types::{ClipBox, Paint};
use fontoxide_ttlib::tables::Colr;

use super::walk::{Walk, merge_eq, merge_opt, repr_debug, repr_int, with_attr, with_index};
use super::{MResult, Merger, MergerKind, master_table};
use crate::errors::MergeErrorKind;

/// `ot.NO_VARIATION_INDEX`.
const NO_VARIATION_INDEX: u32 = 0xFFFF_FFFF;

/// What merging `COLR` tables leaves for `_add_COLR`.
#[derive(Debug, Default)]
pub struct ColrMergeResult {
	/// `merger.varIdxes`.
	pub var_idxes: Vec<u32>,
}

fn not_colr(m: &Merger<'_>) -> bool {
	m.kind != MergerKind::ColrVariation
}

/// `PaintColrLayers` children: the layers of the layer list it refers to.
fn colr_layers_children<'a>(
	t: &ot::PaintFormat1,
	layers: &'a [Option<Box<Paint>>],
) -> MResult<Vec<&'a Paint>> {
	let start = t.first_layer_index as usize;
	let end = start + usize::from(t.num_layers);
	let slice = layers
		.get(start..end)
		.ok_or_else(|| fontoxide_ttlib::Error::ttlib("IndexError: layer index out of range"))?;
	slice
		.iter()
		.map(|p| {
			p.as_deref().ok_or_else(|| {
				fontoxide_ttlib::Error::ttlib(
					"AttributeError: 'NoneType' object has no attribute 'Format'",
				)
				.into()
			})
		})
		.collect()
}

/// `_flatten_layers`: the layers of a `PaintColrLayers`, with nested
/// `PaintColrLayers` flattened.
fn flatten_layers(root: &ot::PaintFormat1, layers: &[Option<Box<Paint>>]) -> MResult<Vec<Paint>> {
	let mut out = Vec::new();
	for paint in colr_layers_children(root, layers)? {
		match paint {
			Paint::Format1(t) => out.extend(flatten_layers(t, layers)?),
			p => out.push(p.clone()),
		}
	}
	Ok(out)
}

/// `COLRVariationMerger.expandPaintColrLayers`: rebuilds the layer list
/// without reuse of layers by `PaintColrLayers`. Each base paint graph is
/// traversed (except `PaintColrGlyph`); the layers referenced by
/// `PaintColrLayers` are collected into a new layer list, with nested
/// `PaintColrLayers` flattened. Layers that no `PaintColrLayers` refers to are
/// dropped.
pub fn expand_paint_colr_layers(colr: &mut ot::COLR) -> crate::errors::Result<()> {
	let Some(layer_list) = colr.layer_list.as_deref() else {
		// If no LayerList, there's nothing to expand.
		return Ok(());
	};
	let old_layers = layer_list.paint.clone();
	let mut new_layers: Vec<Option<Box<Paint>>> = Vec::new();
	if let Some(list) = colr.base_glyph_list.as_deref_mut() {
		for rec in &mut list.base_glyph_paint_record {
			if let Some(paint) = rec.paint.as_deref_mut() {
				expand(paint, &old_layers, &mut new_layers)?;
			}
		}
	}
	if let Some(list) = colr.layer_list.as_deref_mut() {
		list.layer_count = Some(new_layers.len() as u32);
		list.paint = new_layers;
	}
	Ok(())
}

fn expand(
	paint: &mut Paint,
	old_layers: &[Option<Box<Paint>>],
	new_layers: &mut Vec<Option<Box<Paint>>>,
) -> MResult {
	match paint {
		// Don't traverse these, we treat them as constant for merging.
		Paint::Format11(_) => Ok(()),
		Paint::Format1(t) => {
			// De-treeify any nested PaintColrLayers, append unique copies to the new
			// layer list and update PaintColrLayers index/count.
			//
			// LENIENT(L36): fontTools appends a deep copy of a layer that was seen before,
			// but goes on traversing the original; here the appended copies are
			// traversed. This only differs for reused layers with nested
			// PaintColrLayers below them.
			let children = flatten_layers(t, old_layers)?;
			let first = new_layers.len();
			t.first_layer_index = first as u32;
			t.num_layers = fontoxide_ttlib::binary::fit(children.len() as u64, "NumLayers")?;
			new_layers.extend(children.into_iter().map(|c| Some(Box::new(c))));
			for i in first..new_layers.len() {
				let mut child = new_layers[i].take();
				if let Some(c) = child.as_deref_mut() {
					expand(c, old_layers, new_layers)?;
				}
				new_layers[i] = child;
			}
			Ok(())
		}
		p => {
			for child in fontoxide_colorlib::builder::child_paints_mut(p) {
				if let Some(c) = child.as_deref_mut() {
					expand(c, old_layers, new_layers)?;
				}
			}
			Ok(())
		}
	}
}

/// `COLRVariationMerger.mergeTables`.
pub(crate) fn merge_tables(
	m: &mut Merger<'_>,
	font: &mut Font,
	master_fonts: &mut [&mut Font],
) -> crate::errors::Result<()> {
	let Some(out_colr) = font.get::<Colr>()?.and_then(|c| c.table.as_mut()) else {
		return Ok(());
	};
	// The merger modifies the destination COLR table in-place: rebuild its
	// LayerList so that it contains only unique layers.
	expand_paint_colr_layers(out_colr)?;
	m.colr.out_layers = out_colr
		.layer_list
		.as_deref()
		.map(|l| l.paint.clone())
		.unwrap_or_default();

	m.ttfs = (0..master_fonts.len()).collect();
	let mut tables: Vec<Option<ot::COLR>> = Vec::with_capacity(master_fonts.len());
	for master in master_fonts.iter_mut() {
		tables.push(master_table(master, &m.order, |f| {
			Ok(f.get::<Colr>()?.and_then(|c| c.table.clone()))
		})?);
	}
	m.colr.master_layers = tables
		.iter()
		.map(|t| {
			t.as_ref()
				.and_then(|t| t.layer_list.as_deref())
				.map(|l| l.paint.clone())
				.unwrap_or_default()
		})
		.collect();
	let Some(out) = font.get::<Colr>()?.and_then(|c| c.table.as_mut()) else {
		return Ok(());
	};
	let lst: Vec<Option<&ot::COLR>> = tables.iter().map(Option::as_ref).collect();
	super::walk::merge_opt(m, Some(out), &lst)
		.map_err(|e| e.push_stack(".table").push_stack("COLR"))?;
	Ok(())
}

/// The merger of `ot.BaseGlyphList`: the base glyphs of the masters are matched
/// by glyph, so that masters can miss some.
pub(crate) fn base_glyph_list(
	out: &mut ot::BaseGlyphList,
	m: &mut Merger<'_>,
	lst: &[&ot::BaseGlyphList],
) -> MResult {
	if not_colr(m) {
		return out.merge_object(m, lst, &[]);
	}
	// Ignore BaseGlyphCount, allow sparse glyph sets across masters.
	for (i, rec) in out.base_glyph_paint_record.iter_mut().enumerate() {
		let g = rec.base_glyph;
		let values: Vec<Option<&ot::BaseGlyphPaintRecord>> = lst
			.iter()
			.map(|l| {
				l.base_glyph_paint_record
					.iter()
					.rev()
					.find(|r| r.base_glyph == g)
			})
			.collect();
		if let Err(e) = merge_opt(m, Some(rec), &values) {
			let mut e = e.push_stack(format!(".BaseGlyphPaintRecord[{i}]"));
			if let crate::errors::VarLibError::Merge(err) = &mut e
				&& let Some((_, name)) = &mut err.offender
			{
				name.push_str(&format!(" (base glyph {})", str_repr(&m.glyph_name(g))));
			}
			return Err(e);
		}
	}
	m.colr.done_base_glyphs = true;
	Ok(())
}

/// The merger of `ot.LayerList`: the layers collected while merging the
/// `PaintColrLayers` of the base glyphs.
pub(crate) fn layer_list(
	out: &mut ot::LayerList,
	m: &mut Merger<'_>,
	lst: &[&ot::LayerList],
) -> MResult {
	if not_colr(m) {
		return out.merge_object(m, lst, &[]);
	}
	if !m.colr.done_base_glyphs {
		return Err(fontoxide_ttlib::Error::Assertion(
			"BaseGlyphList must be merged before LayerList".into(),
		)
		.into());
	}
	out.layer_count = Some(m.colr.layers.len() as u32);
	out.paint = m
		.colr
		.layers
		.iter()
		.map(|p| Some(Box::new(p.clone())))
		.collect();
	Ok(())
}

/// The name of a `PaintFormat`.
fn paint_format_name(f: u16) -> &'static str {
	const NAMES: [&str; 33] = [
		"PaintColrLayers",
		"PaintSolid",
		"PaintVarSolid",
		"PaintLinearGradient",
		"PaintVarLinearGradient",
		"PaintRadialGradient",
		"PaintVarRadialGradient",
		"PaintSweepGradient",
		"PaintVarSweepGradient",
		"PaintGlyph",
		"PaintColrGlyph",
		"PaintTransform",
		"PaintVarTransform",
		"PaintTranslate",
		"PaintVarTranslate",
		"PaintScale",
		"PaintVarScale",
		"PaintScaleAroundCenter",
		"PaintVarScaleAroundCenter",
		"PaintScaleUniform",
		"PaintVarScaleUniform",
		"PaintScaleUniformAroundCenter",
		"PaintVarScaleUniformAroundCenter",
		"PaintRotate",
		"PaintVarRotate",
		"PaintRotateAroundCenter",
		"PaintVarRotateAroundCenter",
		"PaintSkew",
		"PaintVarSkew",
		"PaintSkewAroundCenter",
		"PaintVarSkewAroundCenter",
		"PaintComposite",
		"PaintGlyph2",
	];
	NAMES
		.get(usize::from(f).wrapping_sub(1))
		.copied()
		.unwrap_or("")
}

/// `checkFormatEnum`: the format must be known and static, and the same in all
/// masters.
fn check_format_enum<T: Walk>(
	m: &Merger<'_>,
	out: &T,
	lst: &[&T],
	enum_name: &str,
	name: fn(u16) -> &'static str,
	is_variable: fn(u16) -> bool,
) -> MResult<u16> {
	let repr = |f: Option<u16>| match f {
		Some(f) if !name(f).is_empty() => format!("<{enum_name}.{}: {f}>", name(f)),
		Some(f) => f.to_string(),
		None => "None".to_owned(),
	};
	let fmt = out.format_number();
	let ok = fmt.is_some_and(|f| !name(f).is_empty() && !is_variable(f));
	if !ok {
		return Err(m.unsupported_format(out.py_type(), Some(repr(fmt))));
	}
	if lst.iter().any(|l| l.format_number() != fmt) {
		return Err(m.merge_error(
			MergeErrorKind::InconsistentFormats {
				subtable: out.py_type().to_owned(),
				value: None,
			},
			Some((
				repr(fmt),
				lst.iter()
					.map(|l| (l.format_number() == fmt, repr(l.format_number())))
					.collect(),
			)),
		));
	}
	Ok(fmt.unwrap_or(0))
}

/// How a variable attribute is stored: `BaseFixedValue` converters store
/// integers.
#[derive(Debug, Clone, Copy)]
enum Conv {
	Int,
	F2Dot14,
	Fixed,
	Angle,
	BiasedAngle,
}

impl Conv {
	/// `conv.toInt(value)`.
	fn to_int(self, v: f64) -> f64 {
		match self {
			Conv::Int => v,
			Conv::F2Dot14 => float_to_fixed(v, 14) as f64,
			Conv::Fixed => float_to_fixed(v, 16) as f64,
			Conv::Angle => float_to_fixed(v / 180.0, 14) as f64,
			Conv::BiasedAngle => float_to_fixed(v / 180.0 - 1.0, 14) as f64,
		}
	}

	/// `conv.fromInt(value)`.
	#[allow(clippy::wrong_self_convention)]
	fn from_int(self, v: f64) -> f64 {
		match self {
			Conv::Int => v,
			Conv::F2Dot14 => fixed_to_float(v as i64, 14),
			Conv::Fixed => fixed_to_float(v as i64, 16),
			Conv::Angle => fixed_to_float(v as i64, 14) * 180.0,
			Conv::BiasedAngle => (fixed_to_float(v as i64, 14) + 1.0) * 180.0,
		}
	}
}

/// `storeVariationIndices`: reuses a `VarIndexBase` for the same variation
/// indices, or adds them.
fn store_variation_indices(m: &mut Merger<'_>, var_idxes: &[u32]) -> u32 {
	let state = &mut m.colr;
	if let Some(&base) = state.var_index_cache.get(var_idxes) {
		return base;
	}
	let n = var_idxes.len();
	// Scan for a full match anywhere in the varIdxes.
	if state.var_idxes.len() >= n {
		for i in 0..=state.var_idxes.len() - n {
			if state.var_idxes[i..i + n] == *var_idxes {
				state.var_index_cache.insert(var_idxes.to_vec(), i as u32);
				return i as u32;
			}
		}
	}
	// Try find a partial match at the end of the varIdxes.
	for k in (1..n).rev() {
		if state.var_idxes.len() >= k
			&& state.var_idxes[state.var_idxes.len() - k..] == var_idxes[..k]
		{
			let base = (state.var_idxes.len() - k) as u32;
			state.var_index_cache.insert(var_idxes.to_vec(), base);
			state.var_idxes.extend_from_slice(&var_idxes[k..]);
			return base;
		}
	}
	// No match found, append at the end.
	let base = state.var_idxes.len() as u32;
	state.var_index_cache.insert(var_idxes.to_vec(), base);
	state.var_idxes.extend_from_slice(var_idxes);
	base
}

/// `mergeVariableAttrs`: the merged values of the attributes, and the
/// `VarIndexBase` of their variations (`NO_VARIATION_INDEX` if none).
fn merge_variable_attrs(
	m: &mut Merger<'_>,
	attrs: &[(Conv, Vec<f64>)],
) -> MResult<(Vec<f64>, u32)> {
	let mut bases = Vec::with_capacity(attrs.len());
	let mut var_idxes = Vec::with_capacity(attrs.len());
	for (conv, values) in attrs {
		// `storeMastersForAttr`.
		let master_values: Vec<f64> = values.iter().map(|v| conv.to_int(*v)).collect();
		let mut base = master_values[0];
		let mut var_idx = NO_VARIATION_INDEX;
		if master_values.iter().any(|v| *v != master_values[0]) {
			let (b, idx) = m.store_builder.store_masters(&master_values)?;
			base = b;
			var_idx = fontoxide_ttlib::binary::fit(idx, "VarIdx")?;
		}
		bases.push(conv.from_int(base));
		var_idxes.push(var_idx);
	}
	let mut base = NO_VARIATION_INDEX;
	if var_idxes.iter().any(|v| *v != NO_VARIATION_INDEX) {
		base = store_variation_indices(m, &var_idxes);
	}
	Ok((bases, base))
}

fn i16_of(v: f64) -> MResult<i16> {
	Ok(fontoxide_ttlib::binary::fit(v as i64, "Short")?)
}

fn u16_of(v: f64) -> MResult<u16> {
	Ok(fontoxide_ttlib::binary::fit(v as i64, "UShort")?)
}

/// `mergeAttrs` of a value that must be the same.
fn eq_attr<T: PartialEq>(
	m: &mut Merger<'_>,
	name: &str,
	out: &T,
	values: Vec<&T>,
	repr: fn(&T) -> String,
) -> MResult {
	with_attr(m, name, |m| merge_eq(m, out, &values, repr))
}

/// `mergeAttrs` of a child paint.
fn child_attr(
	m: &mut Merger<'_>,
	name: &str,
	out: &mut Option<Box<Paint>>,
	values: Vec<Option<&Paint>>,
) -> MResult {
	with_attr(m, name, |m| merge_opt(m, out.as_deref_mut(), &values))
}

/// The merger of `ot.ColorStop`: its `VarIndexBase` if it has variations.
fn merge_color_stop(
	m: &mut Merger<'_>,
	out: &mut ot::ColorStop,
	lst: &[&ot::ColorStop],
) -> MResult<u32> {
	eq_attr(
		m,
		"PaletteIndex",
		&out.palette_index,
		lst.iter().map(|l| &l.palette_index).collect(),
		repr_int,
	)?;
	let (bases, base) = merge_variable_attrs(
		m,
		&[
			(Conv::F2Dot14, lst.iter().map(|l| l.stop_offset).collect()),
			(Conv::F2Dot14, lst.iter().map(|l| l.alpha).collect()),
		],
	)?;
	out.stop_offset = bases[0];
	out.alpha = bases[1];
	Ok(base)
}

/// The merger of `ot.ColorLine`: the `VarIndexBase` of each stop, if it has
/// variations.
fn merge_color_line(
	m: &mut Merger<'_>,
	out: &mut ot::ColorLine,
	lst: &[&ot::ColorLine],
) -> MResult<Option<Vec<u32>>> {
	eq_attr(
		m,
		"Extend",
		&out.extend,
		lst.iter().map(|l| &l.extend).collect(),
		repr_int,
	)?;
	eq_attr(
		m,
		"StopCount",
		&out.stop_count,
		lst.iter().map(|l| &l.stop_count).collect(),
		super::walk::repr_opt_int,
	)?;
	let bases = with_attr(m, "ColorStop", |m| -> MResult<Vec<u32>> {
		super::walk::check_lengths(
			m,
			out.color_stop.len(),
			lst.iter().map(|l| l.color_stop.len()),
		)?;
		let mut bases = Vec::with_capacity(out.color_stop.len());
		for (i, stop) in out.color_stop.iter_mut().enumerate() {
			let values: Vec<&ot::ColorStop> = lst.iter().map(|l| &l.color_stop[i]).collect();
			let mut base = NO_VARIATION_INDEX;
			with_index(m, i, |m| {
				if values.iter().any(|v| v.py_type() != stop.py_type()) {
					return Ok(());
				}
				base = merge_color_stop(m, stop, &values)?;
				Ok(())
			})?;
			bases.push(base);
		}
		Ok(bases)
	})?;
	Ok(bases
		.iter()
		.any(|b| *b != NO_VARIATION_INDEX)
		.then_some(bases))
}

/// `ColorLine` as `VarColorLine`, with the `VarIndexBase` of its stops.
fn var_color_line(line: ot::ColorLine, bases: Option<Vec<u32>>) -> ot::VarColorLine {
	let bases = bases.unwrap_or_default();
	ot::VarColorLine {
		extend: line.extend,
		stop_count: line.stop_count,
		color_stop: line
			.color_stop
			.into_iter()
			.enumerate()
			.map(|(i, s)| ot::VarColorStop {
				stop_offset: s.stop_offset,
				palette_index: s.palette_index,
				alpha: s.alpha,
				var_index_base: bases.get(i).copied().unwrap_or(NO_VARIATION_INDEX),
			})
			.collect(),
	}
}

/// The `ColorLine` child of a paint: merged, with the stops' variations.
fn color_line_attr(
	m: &mut Merger<'_>,
	out: &mut Option<Box<ot::ColorLine>>,
	values: Vec<Option<&ot::ColorLine>>,
) -> MResult<Option<Vec<u32>>> {
	with_attr(m, "ColorLine", |m| {
		let Some(out) = out.as_deref_mut() else {
			return merge_opt::<ot::ColorLine>(m, None, &values).map(|_| None);
		};
		if values.iter().any(Option::is_none) {
			// Masters without a color line sit out.
			let mask: Vec<bool> = values.iter().map(Option::is_some).collect();
			let saved = m.push_sub_model(&mask)?;
			let present: Vec<&ot::ColorLine> = values.iter().flatten().copied().collect();
			let r = merge_color_line(m, out, &present)?;
			m.pop_sub_model(saved);
			return Ok(r);
		}
		let present: Vec<&ot::ColorLine> = values.iter().flatten().copied().collect();
		merge_color_line(m, out, &present)
	})
}

/// The merger of `ot.Affine2x3`: its `VarIndexBase` if it has variations.
fn merge_affine(
	m: &mut Merger<'_>,
	out: &mut ot::Affine2x3,
	lst: &[&ot::Affine2x3],
) -> MResult<u32> {
	let attrs: Vec<(Conv, Vec<f64>)> = [
		|a: &ot::Affine2x3| a.xx,
		|a: &ot::Affine2x3| a.yx,
		|a: &ot::Affine2x3| a.xy,
		|a: &ot::Affine2x3| a.yy,
		|a: &ot::Affine2x3| a.dx,
		|a: &ot::Affine2x3| a.dy,
	]
	.iter()
	.map(|f| (Conv::Fixed, lst.iter().map(|a| f(a)).collect()))
	.collect();
	let (b, base) = merge_variable_attrs(m, &attrs)?;
	(out.xx, out.yx, out.xy, out.yy, out.dx, out.dy) = (b[0], b[1], b[2], b[3], b[4], b[5]);
	Ok(base)
}

/// The paints of a format of `lst`.
macro_rules! of_format {
	($lst:expr, $variant:ident) => {
		of_format!($lst, Paint, $variant)
	};
	($lst:expr, $enum:ident, $variant:ident) => {
		$lst.iter()
			.filter_map(|p| {
				if let $enum::$variant(t) = p {
					Some(t)
				} else {
					None
				}
			})
			.collect::<Vec<_>>()
	};
}

fn int(v: impl Into<f64>) -> f64 {
	v.into()
}

/// Merges the attributes of a paint with a child paint and variable attributes
/// `$($attr: $conv),*`, converting it to `$var` if it has variations.
macro_rules! paint_with_child {
	($m:expr, $out:expr, $l:expr, $var:ident, $vt:ident, [$($attr:ident: $conv:expr => $set:expr),*], [$($center:ident),*]) => {{
		let t = $out;
		let l = $l;
		child_attr($m, "Paint", &mut t.paint, l.iter().map(|x| x.paint.as_deref()).collect())?;
		let mut attrs: Vec<(Conv, Vec<f64>)> = Vec::new();
		$( attrs.push(($conv, l.iter().map(|x| int(x.$attr)).collect())); )*
		$( attrs.push((Conv::Int, l.iter().map(|x| int(x.$center)).collect())); )*
		let (bases, base) = merge_variable_attrs($m, &attrs)?;
		let mut i = 0;
		$( t.$attr = $set(bases[i])?; i += 1; )*
		$( t.$center = i16_of(bases[i])?; i += 1; )*
		let _ = i;
		if base != NO_VARIATION_INDEX {
			Some(Paint::$var(ot::$vt {
				paint: t.paint.take(),
				$($attr: t.$attr,)*
				$($center: t.$center,)*
				var_index_base: base,
			}))
		} else {
			None
		}
	}};
}

fn f(v: f64) -> MResult<f64> {
	Ok(v)
}

/// The merger of `ot.Paint`.
#[allow(clippy::unnecessary_lazy_evaluations)]
pub(crate) fn paint(out: &mut Paint, m: &mut Merger<'_>, lst: &[&Paint]) -> MResult {
	if not_colr(m) {
		return out.merge_object(m, lst, &[]);
	}
	let is_variable = |f: u16| {
		matches!(
			f,
			3 | 5 | 7 | 9 | 13 | 15 | 17 | 19 | 21 | 23 | 25 | 27 | 29 | 31
		)
	};
	check_format_enum(m, out, lst, "PaintFormat", paint_format_name, is_variable)?;
	let new: Option<Paint> = match out {
		Paint::Format1(_) => {
			merge_paint_colr_layers(m, out, lst)?;
			None
		}
		Paint::Format2(t) => {
			let l = of_format!(lst, Format2);
			eq_attr(
				m,
				"PaletteIndex",
				&t.palette_index,
				l.iter().map(|x| &x.palette_index).collect(),
				repr_int,
			)?;
			let (b, base) =
				merge_variable_attrs(m, &[(Conv::F2Dot14, l.iter().map(|x| x.alpha).collect())])?;
			t.alpha = b[0];
			(base != NO_VARIATION_INDEX).then(|| {
				Paint::Format3(ot::PaintFormat3 {
					palette_index: t.palette_index,
					alpha: t.alpha,
					var_index_base: base,
				})
			})
		}
		Paint::Format4(t) => {
			let l = of_format!(lst, Format4);
			let stops = color_line_attr(
				m,
				&mut t.color_line,
				l.iter().map(|x| x.color_line.as_deref()).collect(),
			)?;
			let attrs: Vec<(Conv, Vec<f64>)> = [
				|x: &ot::PaintFormat4| x.x0,
				|x: &ot::PaintFormat4| x.y0,
				|x: &ot::PaintFormat4| x.x1,
				|x: &ot::PaintFormat4| x.y1,
				|x: &ot::PaintFormat4| x.x2,
				|x: &ot::PaintFormat4| x.y2,
			]
			.iter()
			.map(|g| (Conv::Int, l.iter().map(|x| f64::from(g(x))).collect()))
			.collect();
			let (b, base) = merge_variable_attrs(m, &attrs)?;
			(t.x0, t.y0, t.x1, t.y1, t.x2, t.y2) = (
				i16_of(b[0])?,
				i16_of(b[1])?,
				i16_of(b[2])?,
				i16_of(b[3])?,
				i16_of(b[4])?,
				i16_of(b[5])?,
			);
			(base != NO_VARIATION_INDEX || stops.is_some()).then(|| {
				Paint::Format5(ot::PaintFormat5 {
					color_line: t
						.color_line
						.take()
						.map(|c| Box::new(var_color_line(*c, stops))),
					x0: t.x0,
					y0: t.y0,
					x1: t.x1,
					y1: t.y1,
					x2: t.x2,
					y2: t.y2,
					var_index_base: base,
				})
			})
		}
		Paint::Format6(t) => {
			let l = of_format!(lst, Format6);
			let stops = color_line_attr(
				m,
				&mut t.color_line,
				l.iter().map(|x| x.color_line.as_deref()).collect(),
			)?;
			let attrs = vec![
				(Conv::Int, l.iter().map(|x| f64::from(x.x0)).collect()),
				(Conv::Int, l.iter().map(|x| f64::from(x.y0)).collect()),
				(Conv::Int, l.iter().map(|x| f64::from(x.r0)).collect()),
				(Conv::Int, l.iter().map(|x| f64::from(x.x1)).collect()),
				(Conv::Int, l.iter().map(|x| f64::from(x.y1)).collect()),
				(
					Conv::Int,
					l.iter().map(|x| f64::from(x.r1)).collect::<Vec<f64>>(),
				),
			];
			let (b, base) = merge_variable_attrs(m, &attrs)?;
			(t.x0, t.y0, t.r0, t.x1, t.y1, t.r1) = (
				i16_of(b[0])?,
				i16_of(b[1])?,
				u16_of(b[2])?,
				i16_of(b[3])?,
				i16_of(b[4])?,
				u16_of(b[5])?,
			);
			(base != NO_VARIATION_INDEX || stops.is_some()).then(|| {
				Paint::Format7(ot::PaintFormat7 {
					color_line: t
						.color_line
						.take()
						.map(|c| Box::new(var_color_line(*c, stops))),
					x0: t.x0,
					y0: t.y0,
					r0: t.r0,
					x1: t.x1,
					y1: t.y1,
					r1: t.r1,
					var_index_base: base,
				})
			})
		}
		Paint::Format8(t) => {
			let l = of_format!(lst, Format8);
			let stops = color_line_attr(
				m,
				&mut t.color_line,
				l.iter().map(|x| x.color_line.as_deref()).collect(),
			)?;
			let attrs = vec![
				(Conv::Int, l.iter().map(|x| f64::from(x.center_x)).collect()),
				(Conv::Int, l.iter().map(|x| f64::from(x.center_y)).collect()),
				(Conv::BiasedAngle, l.iter().map(|x| x.start_angle).collect()),
				(
					Conv::BiasedAngle,
					l.iter().map(|x| x.end_angle).collect::<Vec<f64>>(),
				),
			];
			let (b, base) = merge_variable_attrs(m, &attrs)?;
			(t.center_x, t.center_y, t.start_angle, t.end_angle) =
				(i16_of(b[0])?, i16_of(b[1])?, b[2], b[3]);
			(base != NO_VARIATION_INDEX || stops.is_some()).then(|| {
				Paint::Format9(ot::PaintFormat9 {
					color_line: t
						.color_line
						.take()
						.map(|c| Box::new(var_color_line(*c, stops))),
					center_x: t.center_x,
					center_y: t.center_y,
					start_angle: t.start_angle,
					end_angle: t.end_angle,
					var_index_base: base,
				})
			})
		}
		Paint::Format10(t) => {
			let l = of_format!(lst, Format10);
			child_attr(
				m,
				"Paint",
				&mut t.paint,
				l.iter().map(|x| x.paint.as_deref()).collect(),
			)?;
			eq_attr(
				m,
				"Glyph",
				&t.glyph,
				l.iter().map(|x| &x.glyph).collect(),
				repr_int,
			)?;
			None
		}
		Paint::Format11(t) => {
			let l = of_format!(lst, Format11);
			eq_attr(
				m,
				"Glyph",
				&t.glyph,
				l.iter().map(|x| &x.glyph).collect(),
				repr_int,
			)?;
			None
		}
		Paint::Format12(t) => {
			let l = of_format!(lst, Format12);
			child_attr(
				m,
				"Paint",
				&mut t.paint,
				l.iter().map(|x| x.paint.as_deref()).collect(),
			)?;
			let values: Vec<Option<&ot::Affine2x3>> =
				l.iter().map(|x| x.transform.as_deref()).collect();
			let base = with_attr(m, "Transform", |m| -> MResult<u32> {
				let Some(out) = t.transform.as_deref_mut() else {
					merge_opt::<ot::Affine2x3>(m, None, &values)?;
					return Ok(NO_VARIATION_INDEX);
				};
				if values.iter().any(Option::is_none) {
					let mask: Vec<bool> = values.iter().map(Option::is_some).collect();
					let saved = m.push_sub_model(&mask)?;
					let present: Vec<&ot::Affine2x3> = values.iter().flatten().copied().collect();
					let r = merge_affine(m, out, &present)?;
					m.pop_sub_model(saved);
					return Ok(r);
				}
				let present: Vec<&ot::Affine2x3> = values.iter().flatten().copied().collect();
				merge_affine(m, out, &present)
			})?;
			(base != NO_VARIATION_INDEX).then(|| {
				Paint::Format13(ot::PaintFormat13 {
					paint: t.paint.take(),
					transform: t.transform.take().map(|a| {
						Box::new(ot::VarAffine2x3 {
							xx: a.xx,
							yx: a.yx,
							xy: a.xy,
							yy: a.yy,
							dx: a.dx,
							dy: a.dy,
							var_index_base: base,
						})
					}),
				})
			})
		}
		Paint::Format14(t) => {
			let l = of_format!(lst, Format14);
			paint_with_child!(m, t, l, Format15, PaintFormat15, [], [dx, dy])
		}
		Paint::Format16(t) => {
			let l = of_format!(lst, Format16);
			paint_with_child!(m, t, l, Format17, PaintFormat17, [scale_x: Conv::F2Dot14 => f, scale_y: Conv::F2Dot14 => f], [])
		}
		Paint::Format18(t) => {
			let l = of_format!(lst, Format18);
			paint_with_child!(m, t, l, Format19, PaintFormat19, [scale_x: Conv::F2Dot14 => f, scale_y: Conv::F2Dot14 => f], [center_x, center_y])
		}
		Paint::Format20(t) => {
			let l = of_format!(lst, Format20);
			paint_with_child!(m, t, l, Format21, PaintFormat21, [scale: Conv::F2Dot14 => f], [])
		}
		Paint::Format22(t) => {
			let l = of_format!(lst, Format22);
			paint_with_child!(m, t, l, Format23, PaintFormat23, [scale: Conv::F2Dot14 => f], [center_x, center_y])
		}
		Paint::Format24(t) => {
			let l = of_format!(lst, Format24);
			paint_with_child!(m, t, l, Format25, PaintFormat25, [angle: Conv::Angle => f], [])
		}
		Paint::Format26(t) => {
			let l = of_format!(lst, Format26);
			paint_with_child!(m, t, l, Format27, PaintFormat27, [angle: Conv::Angle => f], [center_x, center_y])
		}
		Paint::Format28(t) => {
			let l = of_format!(lst, Format28);
			paint_with_child!(m, t, l, Format29, PaintFormat29, [x_skew_angle: Conv::Angle => f, y_skew_angle: Conv::Angle => f], [])
		}
		Paint::Format30(t) => {
			let l = of_format!(lst, Format30);
			paint_with_child!(m, t, l, Format31, PaintFormat31, [x_skew_angle: Conv::Angle => f, y_skew_angle: Conv::Angle => f], [center_x, center_y])
		}
		Paint::Format32(t) => {
			let l = of_format!(lst, Format32);
			child_attr(
				m,
				"SourcePaint",
				&mut t.source_paint,
				l.iter().map(|x| x.source_paint.as_deref()).collect(),
			)?;
			eq_attr(
				m,
				"CompositeMode",
				&t.composite_mode,
				l.iter().map(|x| &x.composite_mode).collect(),
				repr_int,
			)?;
			child_attr(
				m,
				"BackdropPaint",
				&mut t.backdrop_paint,
				l.iter().map(|x| x.backdrop_paint.as_deref()).collect(),
			)?;
			None
		}
		Paint::Format33(t) => {
			let l = of_format!(lst, Format33);
			child_attr(
				m,
				"Paint",
				&mut t.paint,
				l.iter().map(|x| x.paint.as_deref()).collect(),
			)?;
			eq_attr(
				m,
				"Glyph",
				&t.glyph,
				l.iter().map(|x| &x.glyph).collect(),
				repr_int,
			)?;
			None
		}
		_ => None,
	};
	if let Some(new) = new {
		*out = new;
	}
	Ok(())
}

/// `_merge_PaintColrLayers`: the number of flattened layers must be the same in
/// all masters, but `FirstLayerIndex` may differ, for sparse glyph sets.
fn merge_paint_colr_layers(m: &mut Merger<'_>, out: &mut Paint, lst: &[&Paint]) -> MResult {
	let Paint::Format1(out_t) = out else {
		return Ok(());
	};
	let mut out_layers = flatten_layers(out_t, &m.colr.out_layers)?;
	// Sanity check ttfs are subset to current values before matching each master
	// PaintColrLayers to its respective COLR by position.
	if m.ttfs.len() != lst.len() {
		return Err(fontoxide_ttlib::Error::Assertion("len(self.ttfs) == len(lst)".into()).into());
	}
	let mut master_layerses = Vec::with_capacity(lst.len());
	for (i, p) in lst.iter().enumerate() {
		let Paint::Format1(t) = p else {
			continue;
		};
		let layers = m
			.colr
			.master_layers
			.get(m.ttfs[i])
			.cloned()
			.unwrap_or_default();
		master_layerses.push(flatten_layers(t, &layers)?);
	}

	let merged = (|| -> MResult {
		super::walk::check_lengths(m, out_layers.len(), master_layerses.iter().map(Vec::len))?;
		for (i, layer) in out_layers.iter_mut().enumerate() {
			let values: Vec<&Paint> = master_layerses.iter().map(|l| &l[i]).collect();
			with_index(m, i, |m| super::walk::merge_struct(m, layer, &values))?;
		}
		Ok(())
	})();
	// NOTE: This attribute doesn't actually exist in PaintColrLayers but it's
	// handy to have it in the stack trace for debugging.
	merged.map_err(|e| e.push_stack(".Layers"))?;

	if let Some(cache) = &mut m.colr.layer_reuse_cache {
		// Successful reuse can make the list smaller.
		out_layers = cache.try_reuse(out_layers);
	}
	// If the list is still too big we need to tree-fy it.
	let is_tree = out_layers.len() > MAX_PAINT_COLR_LAYER_COUNT;
	let tree = build_n_ary_tree(out_layers, MAX_PAINT_COLR_LAYER_COUNT)
		.map_err(|_| fontoxide_ttlib::Error::value("n must be > 1"))?;

	// We now have a tree of sequences with Paint leaves. Convert the sequences
	// into PaintColrLayers.
	fn list_to_colr_layers(m: &mut Merger<'_>, node: Tree<Paint>) -> MResult<Paint> {
		match node {
			Tree::Leaf(p) => Ok(p),
			Tree::Node(children) => {
				let mut layers = Vec::with_capacity(children.len());
				for c in children {
					layers.push(list_to_colr_layers(m, c)?);
				}
				let first = m.colr.layers.len();
				let paint = Paint::Format1(ot::PaintFormat1 {
					num_layers: fontoxide_ttlib::binary::fit(layers.len() as u64, "NumLayers")?,
					first_layer_index: first as u32,
				});
				if let Some(cache) = &mut m.colr.layer_reuse_cache {
					cache.add(&layers, first);
				}
				m.colr.layers.extend(layers);
				Ok(paint)
			}
		}
	}
	let mut converted = Vec::with_capacity(tree.len());
	for node in tree {
		converted.push(list_to_colr_layers(m, node)?);
	}

	let Paint::Format1(out_t) = out else {
		return Ok(());
	};
	if converted.len() == 1
		&& let Paint::Format1(single) = &converted[0]
	{
		// Special case when the reuse cache finds a single perfect PaintColrLayers
		// match; we assign it directly and avoid creating an extra table.
		out_t.num_layers = single.num_layers;
		out_t.first_layer_index = single.first_layer_index;
	} else {
		out_t.num_layers = fontoxide_ttlib::binary::fit(converted.len() as u64, "NumLayers")?;
		let first = m.colr.layers.len();
		out_t.first_layer_index = first as u32;
		// Register our parts for reuse provided we aren't a tree.
		if !is_tree && let Some(cache) = &mut m.colr.layer_reuse_cache {
			cache.add(&converted, first);
		}
		m.colr.layers.extend(converted);
	}
	Ok(())
}

/// The merger of `ot.ClipBox`.
pub(crate) fn clip_box(out: &mut ot::ClipBox, m: &mut Merger<'_>, lst: &[&ot::ClipBox]) -> MResult {
	if not_colr(m) {
		return out.merge_object(m, lst, &[]);
	}
	let name = |f: u16| match f {
		1 => "Static",
		2 => "Variable",
		_ => "",
	};
	check_format_enum(m, out, lst, "ClipBoxFormat", name, |f| f == 2)?;
	let ot::ClipBox::Format1(t) = out else {
		return Ok(());
	};
	let l = of_format!(lst, ClipBox, Format1);
	let attrs = vec![
		(Conv::Int, l.iter().map(|x| f64::from(x.x_min)).collect()),
		(Conv::Int, l.iter().map(|x| f64::from(x.y_min)).collect()),
		(Conv::Int, l.iter().map(|x| f64::from(x.x_max)).collect()),
		(
			Conv::Int,
			l.iter().map(|x| f64::from(x.y_max)).collect::<Vec<f64>>(),
		),
	];
	let (b, base) = merge_variable_attrs(m, &attrs)?;
	(t.x_min, t.y_min, t.x_max, t.y_max) =
		(i16_of(b[0])?, i16_of(b[1])?, i16_of(b[2])?, i16_of(b[3])?);
	if base != NO_VARIATION_INDEX {
		*out = ot::ClipBox::Format2(ot::ClipBoxFormat2 {
			x_min: t.x_min,
			y_min: t.y_min,
			x_max: t.x_max,
			y_max: t.y_max,
			var_index_base: base,
		});
	}
	Ok(())
}

/// The merger of `ot.Affine2x3` outside of a paint (not reached by `COLR`).
pub(crate) fn affine2x3(
	out: &mut ot::Affine2x3,
	m: &mut Merger<'_>,
	lst: &[&ot::Affine2x3],
) -> MResult {
	out.merge_object(m, lst, &[])
}

/// The merger of `ot.ColorStop` outside of a paint (not reached by `COLR`).
pub(crate) fn color_stop(
	out: &mut ot::ColorStop,
	m: &mut Merger<'_>,
	lst: &[&ot::ColorStop],
) -> MResult {
	out.merge_object(m, lst, &[])
}

/// The merger of `ot.ColorLine` outside of a paint (not reached by `COLR`).
pub(crate) fn color_line(
	out: &mut ot::ColorLine,
	m: &mut Merger<'_>,
	lst: &[&ot::ColorLine],
) -> MResult {
	out.merge_object(m, lst, &[])
}

/// The merger of `ClipList.clips`: masters may omit clip boxes for some or all
/// glyphs (`mergeSparseDict`).
pub(crate) fn merge_clips(
	out: &mut ot::ClipList,
	m: &mut Merger<'_>,
	lst: &[&ot::ClipList],
) -> MResult {
	if not_colr(m) {
		let values: Vec<_> = lst.iter().map(|l| &l.clips).collect();
		return merge_eq(m, &out.clips, &values, repr_debug);
	}
	for (k, clip) in out.clips.iter_mut() {
		let values: Vec<Option<&ot::ClipBox>> = lst.iter().map(|l| l.clips.get(k)).collect();
		merge_opt(m, Some(clip), &values)
			.map_err(|e| e.push_stack(format!("[{}]", str_repr(&m.glyph_name(*k)))))?;
	}
	Ok(())
}
