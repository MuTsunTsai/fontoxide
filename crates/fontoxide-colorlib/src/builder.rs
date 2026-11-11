//! Build COLR/CPAL tables from scratch (`fontTools.colorLib.builder`).

use std::collections::HashMap;
use std::ops::BitOr;

use fontoxide_misc::py;
use fontoxide_misc::tree_tools::{Tree, build_n_ary_tree};
use fontoxide_ttlib::binary::fit;
use fontoxide_ttlib::ot::types::{self as ot, Paint};
use fontoxide_ttlib::tables::colr::Colr;
use fontoxide_ttlib::tables::cpal::{Color, Cpal};
use fontoxide_ttlib::tables::name::Name;
use indexmap::IndexMap;

use crate::enums::PaintFormat;
use crate::generated::{Class, Table, paint_without_children};
use crate::geometry::round_start_circle_stable_containment;
use crate::table_builder::{BuildCallbacks, Dest, GlyphIds, Obj, TableBuilder};
use crate::{Error, Result, Value};

/// `MAX_PAINT_COLR_LAYER_COUNT`: the most layers of a `PaintColrLayers`.
pub const MAX_PAINT_COLR_LAYER_COUNT: usize = 255;
/// `_DEFAULT_ALPHA`.
pub const DEFAULT_ALPHA: f64 = 1.0;
/// `_MAX_REUSE_LEN`.
const MAX_REUSE_LEN: usize = 32;

/// The color glyphs of version 0 (`_ColorGlyphsV0Dict`): base glyph names to
/// `(layer glyph name, palette index)` pairs.
pub type ColorGlyphsV0 = IndexMap<String, Vec<(String, i64)>>;

/// The color glyphs (`_ColorGlyphsDict`): base glyph names to a list of
/// `(layer glyph name, palette index)` pairs for COLRv0, or a paint for
/// COLRv1.
pub type ColorGlyphs = IndexMap<String, Value>;

/// `_beforeBuildPaintRadialGradient`: avoids an abrupt change after rounding
/// when c0 is near c1's perimeter, by rounding the start circle first.
pub fn before_build_paint_radial_gradient(source: Value) -> Result<Value> {
	let Value::Map(mut source) = source else {
		return Err(Error::Type(format!(
			"'{}' object is not subscriptable",
			source.py_type_name()
		)));
	};
	let get = |source: &IndexMap<String, Value>, key: &str| -> Result<f64> {
		source.get(key).ok_or_else(|| Error::key(key))?.as_number()
	};
	let x0 = get(&source, "x0")?;
	let y0 = get(&source, "y0")?;
	let r0 = get(&source, "r0")?;
	let x1 = get(&source, "x1")?;
	let y1 = get(&source, "y1")?;
	let r1 = get(&source, "r1")?;

	let c = round_start_circle_stable_containment((x0, y0), r0, (x1, y1), r1)?;

	// Update the source to ensure the paint is built with corrected values.
	source.insert("x0".into(), Value::Float(c.centre.0));
	source.insert("y0".into(), Value::Float(c.centre.1));
	source.insert("r0".into(), Value::Float(c.radius));
	Ok(Value::Map(source))
}

/// The `CREATE_DEFAULT` callbacks of `_buildPaintCallbacks`: color stops and
/// solid paints with an alpha of 1, color lines that pad.
pub fn create_default_paint_table(class: Class, format: Option<u16>) -> Option<Obj> {
	let mut obj = Obj::new(class);
	match (class, format) {
		(Class::ColorStop | Class::VarColorStop, _) => obj.set("Alpha", DEFAULT_ALPHA),
		(Class::ColorLine | Class::VarColorLine, _) => {
			obj.set("Extend", crate::ExtendMode::Pad);
		}
		(Class::Paint, Some(2 | 3)) => obj.set("Alpha", DEFAULT_ALPHA),
		_ => return None,
	}
	Some(obj)
}

/// The callbacks of `_buildPaintCallbacks()`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PaintCallbacks;

impl BuildCallbacks for PaintCallbacks {
	fn create_default(&mut self, class: Class, format: Option<u16>) -> Option<Obj> {
		create_default_paint_table(class, format)
	}

	fn before_build(
		&mut self,
		_builder: &TableBuilder<'_>,
		class: Class,
		format: Option<u16>,
		dest: Dest,
		source: Value,
	) -> Result<(Dest, Value)> {
		match (class, format) {
			(Class::Paint, Some(6 | 7)) => Ok((dest, before_build_paint_radial_gradient(source)?)),
			_ => Ok((dest, source)),
		}
	}
}

/// The glyph ID of a name, failing like a Python `KeyError`.
fn glyph_id(glyphs: &GlyphIds, name: &str) -> Result<u32> {
	// LENIENT(L32): fontTools keeps the glyph names in the tables it builds, and fails
	// when compiling them if they are not in the font.
	glyphs.get(name).copied().ok_or_else(|| Error::key(name))
}

/// The items of a map of color glyphs, sorted by glyph ID if a glyph map is
/// given.
fn sorted_items<V>(
	items: IndexMap<String, V>,
	glyphs: &GlyphIds,
	glyph_map: bool,
) -> Vec<(String, V)> {
	let mut items: Vec<(String, V)> = items.into_iter().collect();
	if glyph_map {
		// The base glyphs were checked to exist.
		items.sort_by_key(|(name, _)| glyphs.get(name).copied().unwrap_or(u32::MAX));
	}
	items
}

/// `populateCOLRv0`: builds version 0 color layers and adds them to a `COLR`
/// table.
///
/// `glyph_map` says whether fontTools' `glyphMap` is given: the base glyph
/// records are then sorted by glyph ID, and missing glyphs give legible
/// errors. The glyph names are looked up in `glyphs` either way.
pub fn populate_colr_v0(
	table: &mut ot::COLR,
	color_glyphs_v0: ColorGlyphsV0,
	glyphs: &GlyphIds,
	glyph_map: bool,
) -> Result<()> {
	if glyph_map {
		check_base_glyphs_exist(color_glyphs_v0.keys(), glyphs, "populateCOLRv0")?;
	}
	let mut base_glyph_records = Vec::new();
	let mut layer_records = Vec::new();
	for (base_glyph, layers) in sorted_items(color_glyphs_v0, glyphs, glyph_map) {
		base_glyph_records.push(ot::BaseGlyphRecord {
			base_glyph: glyph_id(glyphs, &base_glyph)?,
			first_layer_index: fit(layer_records.len(), "FirstLayerIndex")?,
			num_layers: fit(layers.len(), "NumLayers")?,
		});
		for (layer_glyph, palette_index) in layers {
			if glyph_map && !glyphs.contains_key(&layer_glyph) {
				return Err(Error::ColorLib(format!(
					"populateCOLRv0: layer glyph not found in glyphMap: {}",
					py::str_repr(&layer_glyph)
				)));
			}
			layer_records.push(ot::LayerRecord {
				layer_glyph: glyph_id(glyphs, &layer_glyph)?,
				palette_index: fit(palette_index, "PaletteIndex")?,
			});
		}
	}
	table.base_glyph_record_count = Some(fit(base_glyph_records.len(), "BaseGlyphRecordCount")?);
	table.layer_record_count = Some(fit(layer_records.len(), "LayerRecordCount")?);
	table.base_glyph_record_array = (!base_glyph_records.is_empty()).then(|| {
		Box::new(ot::BaseGlyphRecordArray {
			base_glyph_record: base_glyph_records,
		})
	});
	table.layer_record_array = (!layer_records.is_empty()).then(|| {
		Box::new(ot::LayerRecordArray {
			layer_record: layer_records,
		})
	});
	Ok(())
}

/// The options of [`build_colr`].
#[derive(Debug, Clone)]
pub struct BuildColrOptions {
	/// The version of the table (`version`). If `None`, the version is
	/// determined by the presence of COLRv1 paints or variation data, which
	/// require version 1; otherwise, if all base glyphs use only simple color
	/// layers, version 0 is used.
	pub version: Option<i64>,
	/// Whether fontTools' `glyphMap` is given: the base glyph records are sorted
	/// by glyph ID, the glyphs that the color glyphs use are checked to exist,
	/// and `PaintGlyph`s of glyphs above 65535 become `PaintGlyph2`.
	pub glyph_map: bool,
	/// Item variation store for the deltas of version 1 tables (`varStore`).
	pub var_store: Option<ot::VarStore>,
	/// The map of the variation indices (`varIndexMap`).
	pub var_index_map: Option<ot::DeltaSetIndexMap>,
	/// Base glyph names to clip boxes (`clipBoxes`): 4 values
	/// `(xMin, yMin, xMax, yMax)`, 5 values with a `varIndexBase`, or a built
	/// `ClipBox`.
	pub clip_boxes: Option<IndexMap<String, Value>>,
	/// Whether layers are shared between color glyphs (`allowLayerReuse`).
	pub allow_layer_reuse: bool,
}

impl Default for BuildColrOptions {
	fn default() -> Self {
		BuildColrOptions {
			version: None,
			glyph_map: false,
			var_store: None,
			var_index_map: None,
			clip_boxes: None,
			allow_layer_reuse: true,
		}
	}
}

/// `buildCOLR`: builds a `COLR` table from color layers mapping.
///
/// `color_glyphs` maps base glyph names to either a list of `(layer glyph
/// name, palette index)` pairs for COLRv0, or a single paint or a list of
/// paints for COLRv1. Glyph names are looked up in `glyphs` (the font's
/// `getReverseGlyphMap()`); see [`BuildColrOptions::glyph_map`] for what
/// fontTools' `glyphMap` argument adds.
pub fn build_colr(
	color_glyphs: ColorGlyphs,
	glyphs: &GlyphIds,
	options: BuildColrOptions,
) -> Result<Colr> {
	let BuildColrOptions {
		version,
		glyph_map,
		var_store,
		var_index_map,
		clip_boxes,
		allow_layer_reuse,
	} = options;

	if var_store.is_some() && version == Some(0) {
		return Err(Error::Value("Can't add VarStore to COLRv0".into()));
	}

	let (color_glyphs_v0, color_glyphs_v1) =
		if matches!(version, None | Some(0)) && var_store.is_none() {
			// Split color glyphs into v0 and v1 and encode separately.
			let (v0, v1) = split_color_glyphs_by_version(color_glyphs)?;
			if version == Some(0) && !v1.is_empty() {
				return Err(Error::Value("Can't encode COLRv1 glyphs in COLRv0".into()));
			}
			(v0, v1)
		} else {
			// Unless explicitly requested for v1 or have variations, in which case
			// all color glyphs are encoded as v1.
			(ColorGlyphsV0::new(), color_glyphs)
		};

	let mut colr = ot::COLR::default();

	populate_colr_v0(&mut colr, color_glyphs_v0, glyphs, glyph_map)?;

	let has_v1 = !color_glyphs_v1.is_empty();
	let (layer_list, base_glyph_list) =
		build_colr_v1(color_glyphs_v1, glyphs, glyph_map, allow_layer_reuse)?;
	colr.layer_list = layer_list.map(Box::new);
	colr.base_glyph_list = Some(Box::new(base_glyph_list));

	let version = match version {
		None => u16::from(var_store.is_some() || has_v1),
		Some(v @ (0 | 1)) => v as u16,
		Some(v) => return Err(Error::NotImplemented(v.to_string())),
	};
	colr.version = version;

	if version == 0 {
		let color_layers = Colr::decompile_color_layers_v0(&colr)?;
		Ok(Colr {
			version,
			color_layers: Some(color_layers),
			table: None,
		})
	} else {
		colr.clip_list = match clip_boxes {
			Some(boxes) if !boxes.is_empty() => Some(Box::new(build_clip_list(boxes, glyphs)?)),
			_ => None,
		};
		colr.var_index_map = var_index_map.map(Box::new);
		colr.var_store = var_store.map(Box::new);
		Ok(Colr {
			version,
			color_layers: None,
			table: Some(colr),
		})
	}
}

/// `buildClipList`.
pub fn build_clip_list(
	clip_boxes: IndexMap<String, Value>,
	glyphs: &GlyphIds,
) -> Result<ot::ClipList> {
	let mut clips = std::collections::BTreeMap::new();
	for (name, clip_box) in clip_boxes {
		let clip_box = build_clip_box(clip_box)?;
		// LENIENT(L32): fontTools keeps the clip boxes of glyphs that are not in the
		// font (and dumps them to TTX), but leaves them out when compiling; the
		// clip list here is keyed by glyph ID, so they are dropped.
		if let Some(&gid) = glyphs.get(&name) {
			clips.insert(gid, clip_box);
		}
	}
	Ok(ot::ClipList {
		format: Some(1),
		clips,
	})
}

/// `math.floor` or `math.ceil` of a number, as an integer.
fn int_bound(value: &Value, ceil: bool) -> Result<i64> {
	match value {
		Value::Int(v) => Ok(*v),
		Value::Float(v) if v.is_nan() => {
			Err(Error::Value("cannot convert float NaN to integer".into()))
		}
		Value::Float(v) if v.is_infinite() => Err(Error::Overflow(
			"cannot convert float infinity to integer".into(),
		)),
		Value::Float(v) => Ok(if ceil { v.ceil() } else { v.floor() } as i64),
		other => Err(Error::Type(format!(
			"must be real number, not {}",
			other.py_type_name()
		))),
	}
}

/// `buildClipBox`: a clip box from 4 values `(xMin, yMin, xMax, yMax)`, 5
/// values with a `VarIndexBase`, or a built `ClipBox`.
pub fn build_clip_box(clip_box: Value) -> Result<ot::ClipBox> {
	let items = match clip_box {
		Value::Table(Table::ClipBox(b)) => return Ok(b),
		Value::List(items) => items,
		Value::Map(_) | Value::Str(_) | Value::Table(_) => {
			return Err(Error::Type(format!(
				"invalid ClipBox: {}",
				clip_box.py_repr()
			)));
		}
		other => {
			return Err(Error::Type(format!(
				"object of type '{}' has no len()",
				other.py_type_name()
			)));
		}
	};
	let n = items.len();
	if n != 4 && n != 5 {
		return Err(Error::Value(format!(
			"Invalid ClipBox: expected 4 or 5 values, found {n}"
		)));
	}
	// `intRect`: the box rounded outwards.
	let x_min = fit(int_bound(&items[0], false)?, "xMin")?;
	let y_min = fit(int_bound(&items[1], false)?, "yMin")?;
	let x_max = fit(int_bound(&items[2], true)?, "xMax")?;
	let y_max = fit(int_bound(&items[3], true)?, "yMax")?;
	if n == 5 {
		let var_index_base = match &items[4] {
			Value::Int(v) => *v,
			// `int(float)` truncates.
			Value::Float(v) if v.is_finite() => v.trunc() as i64,
			other => {
				return Err(Error::Type(format!(
					"int() argument must be a string, a bytes-like object or a real number, not '{}'",
					other.py_type_name()
				)));
			}
		};
		Ok(ot::ClipBox::Format2(ot::ClipBoxFormat2 {
			x_min,
			y_min,
			x_max,
			y_max,
			var_index_base: fit(var_index_base, "VarIndexBase")?,
		}))
	} else {
		Ok(ot::ClipBox::Format1(ot::ClipBoxFormat1 {
			x_min,
			y_min,
			x_max,
			y_max,
		}))
	}
}

/// `ColorPaletteType`: the flags of a CPAL palette.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ColorPaletteType(u32);

impl ColorPaletteType {
	/// `USABLE_WITH_LIGHT_BACKGROUND`.
	pub const USABLE_WITH_LIGHT_BACKGROUND: Self = ColorPaletteType(0x0001);
	/// `USABLE_WITH_DARK_BACKGROUND`.
	pub const USABLE_WITH_DARK_BACKGROUND: Self = ColorPaletteType(0x0002);

	/// `ColorPaletteType(value)`, enforcing the reserved bits.
	pub fn new(value: i64) -> Result<Self> {
		if !(0..=0xFFFF).contains(&value) || value & 0xFFFC != 0 {
			return Err(Error::Value(format!(
				"{value} is not a valid ColorPaletteType"
			)));
		}
		Ok(ColorPaletteType(value as u32))
	}

	/// `ColorPaletteType(value)` of any value.
	pub fn from_value(value: &Value) -> Result<Self> {
		match value {
			Value::Int(v) => Self::new(*v),
			other => Err(Error::Value(format!(
				"{} is not a valid ColorPaletteType",
				other.py_repr()
			))),
		}
	}

	/// The flags (`.value`).
	pub fn value(self) -> u32 {
		self.0
	}
}

impl BitOr for ColorPaletteType {
	type Output = Self;

	fn bitor(self, rhs: Self) -> Self {
		ColorPaletteType(self.0 | rhs.0)
	}
}

/// A label of a palette or palette entry (`_OptionalLocalizedString` without
/// `None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalizedString {
	/// An English label.
	Default(String),
	/// Labels keyed by BCP 47 language code.
	Localized(IndexMap<String, String>),
}

impl From<&str> for LocalizedString {
	fn from(s: &str) -> Self {
		LocalizedString::Default(s.to_owned())
	}
}

/// `buildPaletteLabels`: adds the labels to a `name` table and returns their
/// name IDs, [`Cpal::NO_NAME_ID`] for no label.
pub fn build_palette_labels(
	labels: &[Option<LocalizedString>],
	name_table: &mut Name,
) -> Result<Vec<u16>> {
	labels
		.iter()
		.map(|label| match label {
			None => Ok(Cpal::NO_NAME_ID),
			Some(LocalizedString::Default(s)) => Ok(name_table.add_multilingual_name(
				&[("en", s.as_str())],
				None,
				true,
				false,
				0,
			)?),
			Some(LocalizedString::Localized(names)) => {
				let names: Vec<(&str, &str)> = names
					.iter()
					.map(|(k, v)| (k.as_str(), v.as_str()))
					.collect();
				Ok(name_table.add_multilingual_name(&names, None, true, false, 0)?)
			}
		})
		.collect()
}

/// `repr` of an RGBA tuple.
fn color_repr(color: &[f64; 4]) -> String {
	let items: Vec<String> = color.iter().map(|&v| py::float_repr(v)).collect();
	format!("({})", items.join(", "))
}

/// `buildCPAL`: builds a `CPAL` table from a list of color palettes.
///
/// The colors are `(R, G, B, A)` in the range `[0..1]`. The palette types and
/// labels, one for each palette, and the palette entry labels, one for each
/// palette entry, make a version 1 table; the labels are added to
/// `name_table`, which is then required.
pub fn build_cpal(
	palettes: &[Vec<[f64; 4]>],
	palette_types: Option<&[ColorPaletteType]>,
	palette_labels: Option<&[Option<LocalizedString>]>,
	palette_entry_labels: Option<&[Option<LocalizedString>]>,
	mut name_table: Option<&mut Name>,
) -> Result<Cpal> {
	let mut lengths: Vec<usize> = palettes.iter().map(Vec::len).collect();
	lengths.sort_unstable();
	lengths.dedup();
	if lengths.len() != 1 {
		return Err(Error::ColorLib(
			"color palettes have different lengths".into(),
		));
	}

	let has_labels = palette_labels.is_some_and(|l| !l.is_empty())
		|| palette_entry_labels.is_some_and(|l| !l.is_empty());
	if has_labels && name_table.is_none() {
		return Err(Error::Type(
			"nameTable is required if palette or palette entries have labels".into(),
		));
	}

	let mut cpal = Cpal {
		num_palette_entries: fit(lengths[0], "numPaletteEntries")?,
		..Cpal::default()
	};

	for (i, palette) in palettes.iter().enumerate() {
		let mut colors = Vec::with_capacity(palette.len());
		for (j, color) in palette.iter().enumerate() {
			if color.iter().any(|&v| v > 1.0 || v < 0.0) {
				return Err(Error::ColorLib(format!(
					"palette[{i}][{j}] has invalid out-of-range [0..1] color: {}",
					color_repr(color)
				)));
			}
			// Input colors are RGBA, CPAL encodes them as BGRA.
			let channel = |v: f64| -> Result<u8> {
				let v = py::round(v * 255.0);
				if v.is_nan() {
					return Err(Error::Value("cannot convert float NaN to integer".into()));
				}
				Ok(v as u8)
			};
			let [red, green, blue, alpha] = *color;
			colors.push(Color {
				blue: channel(blue)?,
				green: channel(green)?,
				red: channel(red)?,
				alpha: channel(alpha)?,
			});
		}
		cpal.palettes.push(colors);
	}

	if palette_types.is_none() && palette_labels.is_none() && palette_entry_labels.is_none() {
		cpal.version = 0;
		return Ok(cpal);
	}
	cpal.version = 1;

	cpal.palette_types = match palette_types {
		Some(types) => {
			if types.len() != palettes.len() {
				return Err(Error::ColorLib(format!(
					"Expected {} paletteTypes, got {}",
					palettes.len(),
					types.len()
				)));
			}
			types.iter().map(|t| t.value()).collect()
		}
		None => vec![Cpal::DEFAULT_PALETTE_TYPE; palettes.len()],
	};

	cpal.palette_labels = match palette_labels {
		Some(labels) => {
			if labels.len() != palettes.len() {
				return Err(Error::ColorLib(format!(
					"Expected {} paletteLabels, got {}",
					palettes.len(),
					labels.len()
				)));
			}
			match name_table.as_deref_mut() {
				Some(name) => build_palette_labels(labels, name)?,
				// `nameTable.addMultilingualName` of `None`.
				None if labels.is_empty() => Vec::new(),
				None => return Err(no_name_table()),
			}
		}
		None => vec![Cpal::NO_NAME_ID; palettes.len()],
	};

	let entries = usize::from(cpal.num_palette_entries);
	cpal.palette_entry_labels = match palette_entry_labels {
		Some(labels) => {
			if labels.len() != entries {
				return Err(Error::ColorLib(format!(
					"Expected {entries} paletteEntryLabels, got {}",
					labels.len()
				)));
			}
			match name_table {
				Some(name) => build_palette_labels(labels, name)?,
				None if labels.is_empty() => Vec::new(),
				None => return Err(no_name_table()),
			}
		}
		None => vec![Cpal::NO_NAME_ID; entries],
	};

	Ok(cpal)
}

fn no_name_table() -> Error {
	Error::Attribute("'NoneType' object has no attribute 'addMultilingualName'".into())
}

// COLR v1 tables
// See draft proposal at: https://github.com/googlefonts/colr-gradients-spec

/// `_is_colrv0_layer`: whether a layer is a `(layerGlyph, paletteIndex)` pair
/// (a tuple or a list), of a string and an integer.
///
/// See <https://github.com/googlefonts/ufo2ft/issues/426>.
fn is_colrv0_layer(layer: &Value) -> bool {
	matches!(layer, Value::List(items) if matches!(items.as_slice(), [Value::Str(_), Value::Int(_)]))
}

/// `all(_is_colrv0_layer(l) for l in layers)`.
fn all_colrv0_layers(layers: &Value) -> Result<bool> {
	match layers {
		Value::List(items) => Ok(items.iter().all(is_colrv0_layer)),
		// The keys of a dict and the characters of a string are strings, which
		// are no layer pairs.
		Value::Map(map) => Ok(map.is_empty()),
		Value::Str(s) => Ok(s.is_empty()),
		other => Err(Error::Type(format!(
			"'{}' object is not iterable",
			other.py_type_name()
		))),
	}
}

/// The `(layerGlyph, paletteIndex)` pairs of version 0 layers.
fn colrv0_layers(layers: Value) -> Vec<(String, i64)> {
	match layers {
		Value::List(items) => items
			.into_iter()
			.filter_map(|layer| match layer {
				Value::List(pair) => match <[Value; 2]>::try_from(pair) {
					Ok([Value::Str(glyph), Value::Int(index)]) => Some((glyph, index)),
					_ => None,
				},
				_ => None,
			})
			.collect(),
		_ => Vec::new(),
	}
}

/// `_split_color_glyphs_by_version`: the color glyphs with only version 0
/// layers, and the others.
pub fn split_color_glyphs_by_version(
	color_glyphs: ColorGlyphs,
) -> Result<(ColorGlyphsV0, ColorGlyphs)> {
	let mut color_glyphs_v0 = ColorGlyphsV0::new();
	let mut color_glyphs_v1 = ColorGlyphs::new();
	for (base_glyph, layers) in color_glyphs {
		if all_colrv0_layers(&layers)? {
			color_glyphs_v0.insert(base_glyph, colrv0_layers(layers));
		} else {
			color_glyphs_v1.insert(base_glyph, layers);
		}
	}
	Ok((color_glyphs_v0, color_glyphs_v1))
}

/// The key by which the reuse cache compares paints: their value, with
/// `-0.0 == 0.0` as in Python.
fn paint_key(paint: &Paint) -> String {
	format!("{paint:?}")
		.replace(": -0.0,", ": 0.0,")
		.replace(": -0.0 }", ": 0.0 }")
}

/// `LayerReuseCache`: finds slices of layers that were added to the layer
/// list before.
///
/// fontTools compares the paints by value, as nested tuples of their
/// attributes. Here each distinct paint gets a number, so that slices of
/// layers are compared and hashed cheaply.
#[derive(Debug, Clone, Default)]
pub struct LayerReuseCache {
	numbers: HashMap<String, u32>,
	/// `reusePool`: the first layer index of each slice.
	reuse_pool: HashMap<Box<[u32]>, u32>,
}

impl LayerReuseCache {
	/// An empty cache.
	pub fn new() -> Self {
		Self::default()
	}

	/// The numbers of the paints (`_as_tuple`).
	pub(crate) fn numbers(&mut self, paints: &[Paint]) -> Vec<u32> {
		paints.iter().map(|p| self.number(p)).collect()
	}

	fn number(&mut self, paint: &Paint) -> u32 {
		let next = self.numbers.len() as u32;
		*self.numbers.entry(paint_key(paint)).or_insert(next)
	}

	/// `try_reuse`: replaces slices of the layers that are in the layer list
	/// already by `PaintColrLayers`, preferring longer slices, and later ones
	/// among slices of the same length.
	pub fn try_reuse(&mut self, mut layers: Vec<Paint>) -> Vec<Paint> {
		let mut numbers = self.numbers(&layers);
		self.try_reuse_numbered(&mut layers, &mut numbers);
		layers
	}

	pub(crate) fn try_reuse_numbered(&mut self, layers: &mut Vec<Paint>, numbers: &mut Vec<u32>) {
		'found: loop {
			let n = layers.len();
			// `_reuse_ranges` sorted by (length, ubound, lbound), reversed.
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

	/// `add`: registers the slices of layers starting at `first_layer_index`
	/// in the layer list.
	pub fn add(&mut self, layers: &[Paint], first_layer_index: usize) {
		let numbers = self.numbers(layers);
		self.add_numbered(&numbers, first_layer_index);
	}

	pub(crate) fn add_numbered(&mut self, numbers: &[u32], first_layer_index: usize) {
		for lbound in 0..numbers.len() {
			for ubound in lbound + 2..=numbers.len().min(lbound + 1 + MAX_REUSE_LEN) {
				let first = u32::try_from(lbound + first_layer_index).unwrap_or(u32::MAX);
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

/// The child paints of a paint (`Paint`, `SourcePaint`, `BackdropPaint`), in
/// the order of its converters; not the layers of a `PaintColrLayers`.
pub fn child_paints(paint: &Paint) -> Vec<&Option<Box<Paint>>> {
	child_paints!(paint, shared)
}

/// [`child_paints`], mutable.
pub fn child_paints_mut(paint: &mut Paint) -> Vec<&mut Option<Box<Paint>>> {
	child_paints!(paint, mutable)
}

/// Sets `StopCount` of the color line of a gradient, as `TableBuilder` does.
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
pub fn layer_range(t: &ot::PaintFormat1, layers: &[Option<Box<Paint>>]) -> std::ops::Range<usize> {
	let start = (t.first_layer_index as usize).min(layers.len());
	let end = (start + t.num_layers as usize).min(layers.len());
	start..end
}

/// `_flatten_layers` of an unbuilt `PaintColrLayers`, whose `Layers` are
/// flattened already.
fn flatten_layers<'p>(
	paint: &'p ot::PaintFormat1,
	layers: &'p [Option<Box<Paint>>],
	out: &mut Vec<&'p Paint>,
) {
	for layer in layers[layer_range(paint, layers)].iter().flatten() {
		match &**layer {
			Paint::Format1(t) => flatten_layers(t, layers, out),
			other => out.push(other),
		}
	}
}

fn no_paint() -> Error {
	Error::Attribute("'NoneType' object has no attribute 'Format'".into())
}

/// `LayerListBuilder`: builds paints, collecting the layers of their
/// `PaintColrLayers` in a layer list, shared between paints if allowed.
#[derive(Debug, Clone)]
pub struct LayerListBuilder<'a> {
	glyphs: &'a GlyphIds,
	glyph_map: bool,
	/// The layer list (`layers`).
	pub layers: Vec<Paint>,
	/// The reuse cache (`cache`), if layers are shared.
	pub cache: Option<LayerReuseCache>,
}

impl<'a> LayerListBuilder<'a> {
	/// `LayerListBuilder(glyphMap=..., allowLayerReuse=...)`.
	///
	/// Glyph names are looked up in `glyphs`; `glyph_map` says whether
	/// fontTools' `glyphMap` is given, which checks that the glyphs of
	/// `PaintGlyph`, `PaintGlyph2` and `PaintColrGlyph` exist and makes a
	/// `PaintGlyph` of a glyph above 65535 a `PaintGlyph2`.
	pub fn new(glyphs: &'a GlyphIds, glyph_map: bool, allow_layer_reuse: bool) -> Self {
		LayerListBuilder {
			glyphs,
			glyph_map,
			layers: Vec::new(),
			cache: allow_layer_reuse.then(LayerReuseCache::new),
		}
	}

	/// The table builder (`tableBuilder`); build with the layer list builder as
	/// the callbacks.
	pub fn table_builder(&self) -> TableBuilder<'a> {
		TableBuilder::new(self.glyphs)
	}

	/// `buildPaint`.
	pub fn build_paint(&mut self, paint: Value) -> Result<Paint> {
		let builder = self.table_builder();
		builder.build_as::<Paint>(self, paint)
	}

	/// `build`: the layer list, if there are layers.
	pub fn build(self) -> Option<ot::LayerList> {
		if self.layers.is_empty() {
			return None;
		}
		Some(ot::LayerList {
			layer_count: Some(self.layers.len() as u32),
			paint: self.layers.into_iter().map(|p| Some(Box::new(p))).collect(),
		})
	}

	/// `_checkPaintGlyphExists`.
	fn check_paint_glyph_exists(&self, paint: &mut Obj) -> Result<()> {
		let glyph = paint
			.attrs
			.get("Glyph")
			.ok_or_else(|| Error::Attribute("'Paint' object has no attribute 'Glyph'".into()))?;
		let gid = match glyph {
			Value::Str(name) => self.glyphs.get(name).copied(),
			_ => None,
		};
		let Some(gid) = gid else {
			let name = paint
				.format
				.and_then(|f| PaintFormat::from_value(i64::from(f)))
				.map_or("Paint", PaintFormat::py_name);
			return Err(Error::ColorLib(format!(
				"{name}: glyph not found in glyphMap: {}",
				glyph.py_repr()
			)));
		};
		if paint.format == Some(PaintFormat::PaintGlyph as u16) && gid > 0xFFFF {
			paint.format = Some(PaintFormat::PaintGlyph2 as u16);
		}
		Ok(())
	}

	/// `_beforeBuildPaintColrLayers`: builds the layers of a `PaintColrLayers`
	/// into the layer list.
	fn before_build_paint_colr_layers(&mut self, source: Value) -> Result<Paint> {
		let Value::Map(mut source) = source else {
			return Err(Error::Attribute(format!(
				"'{}' object has no attribute 'get'",
				source.py_type_name()
			)));
		};
		// Sketchy gymnastics: a sequence input will have dropped its layers into
		// NumLayers; get it back.
		let layers = match source.shift_remove("NumLayers") {
			Some(layers @ (Value::List(_) | Value::Str(_))) => layers,
			_ => source
				.shift_remove("Layers")
				.ok_or_else(|| Error::key("Layers"))?,
		};

		// Convert maps seqs or whatever into typed objects.
		let mut built = Vec::new();
		for layer in layers.py_iter()? {
			built.push(self.build_paint(layer)?);
		}
		self.colr_layers(built)
	}

	/// The rest of `_beforeBuildPaintColrLayers`, given the built layers: a
	/// `PaintColrLayers` of the layers, added to the layer list, or the only
	/// layer.
	pub fn colr_layers(&mut self, mut layers: Vec<Paint>) -> Result<Paint> {
		// No reason to have a colr layers with just one entry.
		if layers.len() == 1 {
			return Ok(layers.remove(0));
		}

		// Look for reuse, with preference to longer sequences. This may make the
		// layer list smaller.
		let mut numbers = Vec::new();
		if let Some(cache) = &mut self.cache {
			numbers = cache.numbers(&layers);
			cache.try_reuse_numbered(&mut layers, &mut numbers);
		}

		// The layer list is now final; if it's too big we need to tree it.
		let is_tree = layers.len() > MAX_PAINT_COLR_LAYER_COUNT;
		let tree = build_n_ary_tree(layers, MAX_PAINT_COLR_LAYER_COUNT)
			.map_err(|e| Error::Assertion(e.to_string()))?;

		// We now have a tree of sequences with Paint leaves. Convert the
		// sequences into PaintColrLayers.
		let mut layers = Vec::with_capacity(tree.len());
		for node in tree {
			layers.push(self.list_to_colr_layers(node)?);
		}

		// No reason to have a colr layers with just one entry.
		if layers.len() == 1 {
			return Ok(layers.remove(0));
		}

		let first_layer_index = self.layers.len();
		let paint = Paint::Format1(ot::PaintFormat1 {
			num_layers: fit(layers.len(), "NumLayers")?,
			first_layer_index: fit(first_layer_index, "FirstLayerIndex")?,
		});
		self.layers.extend(layers);

		// Register our parts for reuse provided we aren't a tree. If we are a
		// tree the leaves registered for reuse and that will suffice.
		if let Some(cache) = &mut self.cache
			&& !is_tree
		{
			cache.add_numbered(&numbers, first_layer_index);
		}
		Ok(paint)
	}

	/// `listToColrLayers`.
	fn list_to_colr_layers(&mut self, node: Tree<Paint>) -> Result<Paint> {
		match node {
			Tree::Leaf(paint) => Ok(paint),
			Tree::Node(nodes) => {
				let mut layers = Vec::with_capacity(nodes.len());
				for node in nodes {
					layers.push(self.list_to_colr_layers(node)?);
				}
				// `buildPaint({"Format": PaintColrLayers, "Layers": layers})`, of
				// built layers.
				self.colr_layers(layers)
			}
		}
	}

	/// `buildPaint(unbuildPaint(paint))` for a paint read from a font, whose
	/// `PaintColrLayers` refer to `old_layers`, without the round trip through
	/// dicts: the layers are flattened and shared again, the other fields keep
	/// their values.
	///
	/// This is how `fontTools.subset` and `fontTools.varLib` rebuild the layer
	/// list. `_beforeBuildPaintRadialGradient` is the identity on the integers
	/// that a table read from a font has; the `glyphMap` checks are not made, as
	/// these callers give no `glyphMap`. Call
	/// [`check_unbuild_paint`](crate::unbuilder::check_unbuild_paint) first for
	/// the errors of unbuilding.
	pub fn rebuild_paint(
		&mut self,
		paint: &Paint,
		old_layers: &[Option<Box<Paint>>],
	) -> Result<Paint> {
		let original = match paint {
			Paint::Format1(t) => {
				let mut flat = Vec::new();
				flatten_layers(t, old_layers, &mut flat);
				if let [only] = flat[..] {
					return self.rebuild_paint(only, old_layers);
				}
				let mut layers = Vec::with_capacity(flat.len());
				for layer in flat {
					layers.push(self.rebuild_paint(layer, old_layers)?);
				}
				return self.colr_layers(layers);
			}
			Paint::Unknown(format) => {
				return Err(Error::Assertion(format!(
					"<class 'fontTools.ttLib.tables.otTables.Paint'> invalid Format: {format}"
				)));
			}
			other => other,
		};
		// Copy the paint without its children, which are rebuilt in their turn.
		let mut paint = paint_without_children(original);
		for (slot, child) in child_paints_mut(&mut paint)
			.into_iter()
			.zip(child_paints(original))
		{
			let child = child.as_deref().ok_or_else(no_paint)?;
			*slot = Some(Box::new(self.rebuild_paint(child, old_layers)?));
		}
		set_stop_count(&mut paint);
		Ok(paint)
	}
}

impl BuildCallbacks for LayerListBuilder<'_> {
	fn create_default(&mut self, class: Class, format: Option<u16>) -> Option<Obj> {
		create_default_paint_table(class, format)
	}

	fn before_build(
		&mut self,
		builder: &TableBuilder<'_>,
		class: Class,
		format: Option<u16>,
		dest: Dest,
		source: Value,
	) -> Result<(Dest, Value)> {
		match (class, format) {
			// COLR layers is unusual in that it modifies shared state, so we need a
			// callback into an object.
			(Class::Paint, Some(1)) => {
				let paint = self.before_build_paint_colr_layers(source)?;
				// We've fully built dest; empty source prevents generalized build
				// from kicking in.
				Ok((
					Dest::Table(Table::Paint(paint)),
					Value::Map(IndexMap::new()),
				))
			}
			_ => PaintCallbacks.before_build(builder, class, format, dest, source),
		}
	}

	fn after_build(
		&mut self,
		_builder: &TableBuilder<'_>,
		class: Class,
		format: Option<u16>,
		mut dest: Dest,
	) -> Result<Dest> {
		// When a glyph map is given, check that the glyphs referenced by
		// PaintGlyph, PaintGlyph2 and PaintColrGlyph exist, so that a missing
		// glyph gives a legible error here instead of failing obscurely later.
		// https://github.com/fonttools/fonttools/issues/2629
		if self.glyph_map
			&& class == Class::Paint
			&& matches!(format, Some(10 | 11 | 33))
			&& let Dest::Obj(obj) = &mut dest
		{
			self.check_paint_glyph_exists(obj)?;
		}
		Ok(dest)
	}
}

/// `buildBaseGlyphPaintRecord`.
pub fn build_base_glyph_paint_record(
	base_glyph: &str,
	layer_builder: &mut LayerListBuilder<'_>,
	paint: Value,
) -> Result<ot::BaseGlyphPaintRecord> {
	Ok(ot::BaseGlyphPaintRecord {
		base_glyph: glyph_id(layer_builder.glyphs, base_glyph)?,
		paint: Some(Box::new(layer_builder.build_paint(paint)?)),
	})
}

/// `_check_base_glyphs_exist`: checks that every base glyph name exists in the
/// glyph map.
fn check_base_glyphs_exist<'n>(
	names: impl Iterator<Item = &'n String>,
	glyphs: &GlyphIds,
	place: &str,
) -> Result<()> {
	let missing: Vec<&str> = names
		.filter(|name| !glyphs.contains_key(*name))
		.map(String::as_str)
		.collect();
	if missing.is_empty() {
		return Ok(());
	}
	let preview = missing[..missing.len().min(10)].join(", ");
	let extra = if missing.len() > 10 {
		format!(" (and {} more)", missing.len() - 10)
	} else {
		String::new()
	};
	Err(Error::ColorLib(format!(
		"{place}: base glyph(s) not found in glyphMap: {preview}{extra}"
	)))
}

/// `buildColrV1`: the layer list and the base glyph list of COLRv1 color
/// glyphs.
///
/// The errors of the color glyphs that fail to build are collected into
/// [`Error::BaseGlyphList`].
pub fn build_colr_v1(
	color_glyphs: ColorGlyphs,
	glyphs: &GlyphIds,
	glyph_map: bool,
	allow_layer_reuse: bool,
) -> Result<(Option<ot::LayerList>, ot::BaseGlyphList)> {
	if glyph_map {
		check_base_glyphs_exist(color_glyphs.keys(), glyphs, "buildColrV1")?;
	}
	let items = sorted_items(color_glyphs, glyphs, glyph_map);

	let mut errors = IndexMap::new();
	let mut base_glyphs = Vec::new();
	let mut layer_builder = LayerListBuilder::new(glyphs, glyph_map, allow_layer_reuse);
	for (base_glyph, paint) in items {
		match build_base_glyph_paint_record(&base_glyph, &mut layer_builder, paint) {
			Ok(record) => base_glyphs.push(record),
			Err(e) if e.is_glyph_error() => {
				errors.insert(base_glyph, e);
			}
			Err(e) => return Err(e),
		}
	}

	if !errors.is_empty() {
		return Err(Error::BaseGlyphList(errors));
	}

	let layers = layer_builder.build();
	let glyphs = ot::BaseGlyphList {
		base_glyph_count: Some(fit(base_glyphs.len(), "BaseGlyphCount")?),
		base_glyph_paint_record: base_glyphs,
	};
	Ok((layers, glyphs))
}
