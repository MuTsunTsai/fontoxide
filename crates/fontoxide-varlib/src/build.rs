//! Port of `fontTools.varLib` (`__init__.py`): building a variable font from a
//! designspace document and its master fonts.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use fontoxide_designspacelib::split::{SplitOptions, split_interpolable, split_variable_fonts};
use fontoxide_designspacelib::{
	Axis, AxisDescriptor, AxisMappingDescriptor, DesignSpaceDocument, InstanceDescriptor,
	LocationValue, Number, RuleDescriptor, SourceDescriptor,
};
use fontoxide_misc::Tag;
use fontoxide_misc::fixed_tools::float_to_fixed;
use fontoxide_misc::plistlib;
use fontoxide_misc::py::{float_repr, str_repr};
use fontoxide_misc::round_tools::ot_round;
use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::var_store;
use fontoxide_ttlib::ot::writer::WriterGraph;
use fontoxide_ttlib::tables::fvar::{Axis as FvarAxis, NamedInstance};
use fontoxide_ttlib::tables::glyf::Glyph;
use fontoxide_ttlib::tables::name::add_multilingual_name;
use fontoxide_ttlib::tables::tuple_variation::{Delta, TupleVariation};
use fontoxide_ttlib::tables::{
	Avar, Base, Cvar, Cvt, Fpgm, Fvar, Gdef, Glyf, Gpos, Gsub, Gvar, Hmtx, Hvar, Maxp, Mvar, Name,
	Os2, Post, Prep, Vmtx, Vorg, Vvar,
};
use fontoxide_ttlib::{Font, GlyphOrder, ReadOptions};
use indexmap::IndexMap;

use crate::builder::{build_delta_set_index_map, build_var_idx_map};
use crate::errors::{Result, VarLibError};
use crate::feature_vars::{AxisBox, Region, Substitution, add_feature_variations};
use crate::merger::{MasterInfo, Merger};
use crate::models::{
	DictLocation, Support, VariationModel, normalize_value, normalize_value_decreasing,
	piecewise_linear_map,
};
use crate::mvar::{MVAR_ENTRIES, get_mvar_table_tag};
use crate::var_store::OnlineVarStoreBuilder;

/// A lib key for the designspace document: a comma-separated list of OpenType
/// feature tags, to be used as the FeatureVariations feature. If present, the
/// `<rules processing="...">` flag is ignored (`FEAVAR_FEATURETAG_LIB_KEY`).
pub const FEAVAR_FEATURETAG_LIB_KEY: &str = "com.github.fonttools.varLib.featureVarsFeatureTag";

fn validation(message: impl Into<String>) -> VarLibError {
	VarLibError::Validation(message.into())
}

fn key_error(key: &str) -> VarLibError {
	fontoxide_ttlib::Error::ttlib(format!("KeyError: {}", str_repr(key))).into()
}

fn assertion(message: &str) -> VarLibError {
	fontoxide_ttlib::Error::Assertion(message.to_owned()).into()
}

fn tag(s: &str) -> Tag {
	Tag::from_str_padded(s).unwrap_or(Tag::new(b"    "))
}

/// The axes of a designspace by name, in order (`OrderedDict` of
/// `AxisDescriptor`s).
pub type Axes = IndexMap<String, AxisDescriptor>;

/// The continuous axis of a designspace axis; varLib needs `minimum` and
/// `maximum`.
fn continuous(axis: &Axis) -> Result<&AxisDescriptor> {
	axis.as_continuous().ok_or_else(|| {
		fontoxide_ttlib::Error::ttlib(
			"AttributeError: 'DiscreteAxisDescriptor' object has no attribute 'minimum'",
		)
		.into()
	})
}

fn number(v: Number) -> f64 {
	v.to_f64()
}

fn axis_tag(axis: &AxisDescriptor) -> String {
	axis.tag.clone().unwrap_or_else(|| "None".to_owned())
}

fn axis_name(axis: &AxisDescriptor) -> String {
	axis.name.clone().unwrap_or_else(|| "None".to_owned())
}

/// `axis.map_forward(v)` as a float.
fn map_forward(axis: &AxisDescriptor, v: Number) -> Result<f64> {
	Ok(axis.map_forward(v)?.to_f64())
}

//
// Creation routines
//

/// `_add_fvar`: adds an `fvar` table to the font.
pub fn add_fvar(font: &mut Font, axes: &Axes, instances: &[InstanceDescriptor]) -> Result<()> {
	if axes.is_empty() {
		return Err(assertion("axes"));
	}
	log::info!("Generating fvar");
	let mut fvar = Fvar::default();

	// If there are not currently any mac names don't add them here, that's
	// inconsistent (https://github.com/fonttools/fonttools/issues/683).
	let mac_names = font
		.get::<Name>()?
		.ok_or_else(|| key_error("name"))?
		.names
		.iter()
		.any(|n| n.platform_id == 1);
	// We have all the best ways to express mac names.
	let mut platforms = vec![(3, 1, 0x409)];
	if mac_names {
		platforms.insert(0, (1, 0, 0));
	}

	for a in axes.values() {
		let names: Vec<(&str, &str)> = a
			.label_names
			.iter()
			.map(|(k, v)| (k.as_str(), v.as_str()))
			.collect();
		let mut axis = FvarAxis {
			axis_tag: tag_bytes(&axis_tag(a)),
			min_value: number(a.minimum),
			default_value: number(a.default),
			max_value: number(a.maximum),
			flags: u16::from(a.hidden),
			axis_name_id: 0,
		};
		// TODO Skip axes that have no variation.
		axis.axis_name_id = add_multilingual_name(font, &names, None, true, mac_names, 256)?;
		fvar.axes.push(axis);
	}

	let default_coordinates: BTreeMap<Tag, f64> = fvar
		.axes
		.iter()
		.map(|a| (Tag::new(&a.axis_tag), a.default_value))
		.collect();

	for instance in instances {
		// Filter out discrete axis locations.
		let coordinates: Vec<(&String, &LocationValue)> = instance
			.design_location
			.iter()
			.filter(|(name, _)| axes.contains_key(*name))
			.collect();

		let mut localised: IndexMap<String, String> = instance.localised_style_name.clone();
		if !localised.contains_key("en") {
			match instance.style_name.as_deref().filter(|s| !s.is_empty()) {
				Some(style) => {
					localised.insert("en".to_owned(), style.to_owned());
				}
				None => {
					let items: Vec<String> = coordinates
						.iter()
						.map(|(k, v)| format!("{}: {}", str_repr(k), location_value_repr(**v)))
						.collect();
					return Err(validation(format!(
						"Instance at location '{{{}}}' must have a default English style name ('stylename' attribute on the instance element or a stylename element with an 'xml:lang=\"en\"' attribute).",
						items.join(", ")
					)));
				}
			}
		}
		let names: Vec<(&str, &str)> = localised
			.iter()
			.map(|(k, v)| (k.as_str(), v.as_str()))
			.collect();

		let mut inst = NamedInstance::default();
		for (k, v) in &coordinates {
			let axis = &axes[*k];
			inst.coordinates
				.insert(tag(&axis_tag(axis)), axis.map_backward(**v)?.to_f64());
		}

		let name_table = font.require::<Name>()?;
		let subfamily = name_table.find_multilingual_name(&names, true, mac_names, 0)?;
		inst.subfamily_name_id = match subfamily {
			// Instances can only reuse an existing name ID 2 or 17 if they are at the
			// default location across all axes, see
			// https://github.com/fonttools/fonttools/issues/3825.
			Some(id @ (2 | 17)) if inst.coordinates == default_coordinates => id,
			_ => name_table.add_multilingual_name(&names, None, true, mac_names, 256)?,
		};

		if let Some(psname) = &instance.post_script_font_name {
			let name = font.require::<Name>()?;
			inst.postscript_name_id = name.add_name(psname, &platforms, 255)?;
		}
		fvar.instances.push(inst);
	}

	if font.contains(Tag::new(b"fvar")) {
		return Err(assertion("\"fvar\" not in font"));
	}
	font.insert(Box::new(fvar));
	Ok(())
}

fn tag_bytes(s: &str) -> [u8; 4] {
	tag(s).0
}

/// `repr` of a location value.
fn location_value_repr(v: LocationValue) -> String {
	match v {
		LocationValue::Value(n) => n.repr(),
		LocationValue::Anisotropic(x, y) => format!("({}, {})", x.repr(), y.repr()),
	}
}

/// `_add_avar`: adds an `avar` table to the font for the axis maps, and for the
/// axis mappings (`mappings`, which make it version 2). Returns whether it was
/// added: without any mapping there is no need for it.
pub fn add_avar(
	font: &mut Font,
	axes: &Axes,
	mappings: &[AxisMappingDescriptor],
	axis_tags: &[String],
) -> Result<bool> {
	if axes.is_empty() {
		return Err(assertion("axes"));
	}
	log::info!("Generating avar");
	let mut avar = Avar::default();
	let mut interesting = false;
	let mut vals_triples: HashMap<String, (f64, f64, f64)> = HashMap::new();
	for axis in axes.values() {
		let axis_map = axis.get_validated_map()?;
		let name = axis_name(axis);

		// Currently, some rasterizers require that the default value maps (-1 to -1,
		// 0 to 0, and 1 to 1) be present for all the segment maps, even when the
		// default normalization mapping for the axis was not modified.
		let mut curve: Vec<(f64, f64)> = vec![(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)];
		let keys_triple = (
			number(axis.minimum),
			number(axis.default),
			number(axis.maximum),
		);
		let vals_triple = (
			map_forward(axis, axis.minimum)?,
			map_forward(axis, axis.default)?,
			map_forward(axis, axis.maximum)?,
		);
		vals_triples.insert(axis_tag(axis), vals_triple);
		if axis_map.is_empty() {
			avar.segments.insert(tag(&axis_tag(axis)), curve);
			continue;
		}

		// The validated view removes repeated pairs while preserving legal duplicate
		// output values.
		let mut items: Vec<(Number, Number)> = axis_map;
		items.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
		let keys: Vec<Number> = items.iter().map(|i| i.0).collect();
		let vals: Vec<f64> = items.iter().map(|i| i.1.to_f64()).collect();

		// Current avar requirements.
		let min = keys
			.iter()
			.copied()
			.reduce(Number::py_min)
			.unwrap_or_default();
		let max = keys
			.iter()
			.copied()
			.reduce(Number::py_max)
			.unwrap_or_default();
		if axis.minimum != min {
			return Err(validation(format!(
				"Axis '{name}': there must be a mapping for the axis minimum value {} and it must be the lowest input mapping value.",
				axis.minimum
			)));
		}
		if axis.maximum != max {
			return Err(validation(format!(
				"Axis '{name}': there must be a mapping for the axis maximum value {} and it must be the highest input mapping value.",
				axis.maximum
			)));
		}
		if !keys.contains(&axis.default) {
			return Err(validation(format!(
				"Axis '{name}': there must be a mapping for the axis default value {}.",
				axis.default
			)));
		}
		// Mapping outputs must be monotonic in the design axis direction.
		let reverse = vals_triple.0 > vals_triple.2;
		let mut sorted = vals.clone();
		fontoxide_misc::py::sort_floats(&mut sorted);
		if reverse {
			sorted.reverse();
		}
		if sorted != vals {
			return Err(validation(format!(
				"Axis '{name}': mapping output values must be in {} order.",
				if reverse { "descending" } else { "ascending" }
			)));
		}

		let keys: Vec<f64> = keys
			.iter()
			.map(|v| normalize_value(v.to_f64(), keys_triple, false))
			.collect::<fontoxide_ttlib::Result<_>>()?;
		let vals: Vec<f64> = vals
			.iter()
			.map(|v| normalize_value_decreasing(*v, vals_triple, false))
			.collect::<fontoxide_ttlib::Result<_>>()?;

		if keys.iter().zip(&vals).all(|(k, v)| k == v) {
			avar.segments.insert(tag(&axis_tag(axis)), curve);
			continue;
		}
		interesting = true;
		for (&k, &v) in keys.iter().zip(&vals) {
			match curve.iter_mut().find(|(f, _)| *f == k) {
				Some(entry) => entry.1 = v,
				None => curve.push((k, v)),
			}
		}
		let get = |k: f64| curve.iter().find(|(f, _)| *f == k).map(|e| e.1);
		if get(0.0) != Some(0.0) {
			return Err(assertion("0.0 in curve and curve[0.0] == 0.0"));
		}
		if get(-1.0).is_some_and(|v| v != -1.0) {
			return Err(assertion("-1.0 not in curve or curve[-1.0] == -1.0"));
		}
		if get(1.0).is_some_and(|v| v != 1.0) {
			return Err(assertion("+1.0 not in curve or curve[+1.0] == +1.0"));
		}
		avar.segments.insert(tag(&axis_tag(axis)), curve);
	}

	if !mappings.is_empty() {
		interesting = true;
		let normalize_location =
			|location: &fontoxide_designspacelib::SimpleLocation| -> Result<DictLocation> {
				location
					.iter()
					.map(|(name, v)| {
						let axis = axes.get(name).ok_or_else(|| key_error(name))?;
						let t = axis_tag(axis);
						let triple = vals_triples.get(&t).copied().ok_or_else(|| key_error(&t))?;
						Ok((t, normalize_value_decreasing(v.to_f64(), triple, false)?))
					})
					.collect()
			};
		let mut input_locations: Vec<DictLocation> = mappings
			.iter()
			.map(|m| normalize_location(&m.input_location))
			.collect::<Result<_>>()?;
		let mut output_locations: Vec<DictLocation> = mappings
			.iter()
			.map(|m| normalize_location(&m.output_location))
			.collect::<Result<_>>()?;

		// If base-master is missing, insert it at zero location.
		if !input_locations
			.iter()
			.any(|loc| loc.iter().all(|(_, v)| *v == 0.0))
		{
			input_locations.insert(0, Vec::new());
			output_locations.insert(0, Vec::new());
		}

		let model = VariationModel::new(&input_locations, Some(axis_tags), false, None)
			.map_err(model_error)?;
		let mut store_builder = OnlineVarStoreBuilder::new(axis_tags.to_vec())?;
		store_builder.set_model(&model);
		let mut var_idxes: Vec<(String, u64)> = Vec::new();
		for t in axis_tags {
			let mut master_values = Vec::with_capacity(output_locations.len());
			for (vo, vi) in output_locations.iter().zip(&input_locations) {
				let Some(&(_, o)) = vo.iter().find(|(k, _)| k == t) else {
					master_values.push(0.0);
					continue;
				};
				let i = vi.iter().find(|(k, _)| k == t).map_or(0.0, |e| e.1);
				master_values.push(float_to_fixed(o - i, 14) as f64);
			}
			var_idxes.push((t.clone(), store_builder.store_masters(&master_values)?.1));
		}
		let mut store = store_builder.finish(true);
		let optimized = var_store::optimize(&mut store, true)?;
		let mapped: Vec<u64> = var_idxes
			.iter()
			.map(|(_, v)| {
				optimized
					.get(v)
					.copied()
					.ok_or_else(|| key_error(&v.to_string()))
			})
			.collect::<Result<_>>()?;
		let var_idx_map = build_delta_set_index_map(mapped);

		avar.major_version = 2;
		avar.table = ot::Avar {
			var_idx_map: var_idx_map.map(Box::new),
			var_store: Some(Box::new(store)),
			..Default::default()
		};
	}

	if font.contains(Tag::new(b"avar")) {
		return Err(assertion("\"avar\" not in font"));
	}
	if !interesting {
		log::info!("No need for avar");
		return Ok(false);
	}
	font.insert(Box::new(avar));
	Ok(true)
}

fn model_error(e: fontoxide_ttlib::Error) -> VarLibError {
	let message = e.to_string();
	match message.strip_prefix("VariationModelError: ") {
		Some(m) => VarLibError::VariationModel(m.to_owned()),
		None => e.into(),
	}
}

/// `_add_stat`: a minimal `STAT` table with the `fvar` axes, unless the font has
/// one.
pub fn add_stat(font: &mut Font) -> Result<()> {
	if font.contains(Tag::new(b"STAT")) {
		return Ok(());
	}
	use fontoxide_otllib::builder::{StatAxis, StatName, build_stat_table};
	let fvar = font.require::<Fvar>()?;
	let axes: Vec<StatAxis> = fvar
		.axes
		.iter()
		.map(|a| StatAxis {
			tag: Tag::new(&a.axis_tag),
			name: StatName::Id(a.axis_name_id),
			ordering: None,
			values: Vec::new(),
		})
		.collect();
	build_stat_table(font, &axes, None, &StatName::Id(2), true, true)?;
	Ok(())
}

/// `_get_TT_table_family`: `upper` for the beyond-64k companion tables.
fn tt_table_family(font: &Font) -> Option<&'static str> {
	if font.contains(Tag::new(b"GLYF")) {
		return Some("upper");
	}
	if font.contains(Tag::new(b"glyf")) {
		return Some("lower");
	}
	None
}

/// `_get_TT_tag`: the tag of a TrueType table, or of its companion.
fn tt_tag(font: &Font, t: &str) -> Tag {
	let companion = match t {
		"glyf" => "GLYF",
		"gvar" => "GVAR",
		"hmtx" => "HMTX",
		"maxp" => "MAXP",
		"vmtx" => "VMTX",
		_ => t,
	};
	if tt_table_family(font) == Some("upper") {
		tag(companion)
	} else {
		tag(t)
	}
}

/// `_check_matching_TT_table_families`.
fn check_matching_tt_table_families(masters: &[Font]) -> Result<()> {
	let families: Vec<Option<&str>> = masters.iter().map(tt_table_family).collect();
	if families.iter().any(|f| *f != families[0]) {
		return Err(validation(
			"Masters must all use the same glyf/GLYF table family.",
		));
	}
	Ok(())
}

/// `_GlyphControls`.
#[allow(clippy::type_complexity)]
#[derive(Debug, Clone, PartialEq)]
struct GlyphControls {
	number_of_contours: i64,
	end_pts: Vec<i64>,
	flags: Option<Vec<u8>>,
	components: Option<Vec<(String, Option<[[f64; 2]; 2]>)>>,
}

type GlyphData = (Vec<(f64, f64)>, GlyphControls);

/// `glyf._getCoordinatesAndControls` for the glyphs of `order` (the glyph order of
/// the variable font), by name; `None` for the glyphs the master doesn't have.
fn master_glyph_data(master: &mut Font, order: &GlyphOrder) -> Result<Vec<Option<GlyphData>>> {
	let glyf_tag = tt_tag(master, "glyf");
	let hmtx_tag = tt_tag(master, "hmtx");
	let vmtx_tag = tt_tag(master, "vmtx");
	let master_order = master.glyph_order()?;
	let h_metrics = master
		.get_at::<Hmtx>(hmtx_tag)?
		.ok_or_else(|| key_error(&hmtx_tag.to_string()))?
		.metrics
		.clone();
	let v_metrics = master.get_at::<Vmtx>(vmtx_tag)?.map(|t| t.metrics.clone());
	let glyf = master
		.get_at::<Glyf>(glyf_tag)?
		.ok_or_else(|| key_error(&glyf_tag.to_string()))?;
	let mut out = Vec::with_capacity(order.len());
	for name in order.names() {
		let Some(gid) = master_order.id(name) else {
			out.push(None);
			continue;
		};
		let Some(glyph) = glyf.glyphs.get(&gid).cloned() else {
			out.push(None);
			continue;
		};
		let (coords, end_pts) =
			glyf.coordinates_and_controls(gid, &master_order, &h_metrics, v_metrics.as_ref())?;
		let controls = if glyph.is_composite() {
			GlyphControls {
				number_of_contours: glyph.number_of_contours,
				end_pts,
				flags: None,
				components: Some(
					glyph
						.components
						.iter()
						.map(|c| (master_order.name(c.glyph).into_owned(), c.transform))
						.collect(),
				),
			}
		} else {
			let flags = if glyph.number_of_contours > 0 {
				glyph.flags.clone()
			} else {
				Vec::new()
			};
			GlyphControls {
				number_of_contours: glyph.number_of_contours,
				end_pts,
				flags: Some(flags),
				components: None,
			}
		};
		out.push(Some((coords, controls)));
	}
	Ok(out)
}

/// A `Number` of a delta: an `int` if integral, as `GlyphCoordinates` gives it.
fn delta_number(v: f64) -> fontoxide_ttlib::tables::glyf::Number {
	use fontoxide_ttlib::tables::glyf::Number as N;
	if v.fract() == 0.0 && v.abs() < 1e15 {
		N::Int(v as i64)
	} else {
		N::Float(v)
	}
}

fn support_axes(support: &Support) -> BTreeMap<String, (f64, f64, f64)> {
	support.iter().cloned().collect()
}

/// `_add_gvar`: the glyph variations from the masters' outlines.
fn add_gvar(
	font: &mut Font,
	master_model: &VariationModel,
	masters: &mut [Font],
	tolerance: f64,
	optimize: bool,
) -> Result<()> {
	if tolerance < 0.0 {
		return Err(fontoxide_ttlib::Error::value("`tolerance` must be a positive number.").into());
	}
	let gvar_tag = tt_tag(font, "gvar");
	log::info!("Generating {gvar_tag}");
	if font.contains(gvar_tag) {
		return Err(assertion("gvarTag not in font"));
	}
	let order = font.glyph_order()?;
	let default_master_index = master_model.reverse_mapping[0];
	let datas: Vec<Vec<Option<GlyphData>>> = masters
		.iter_mut()
		.map(|m| master_glyph_data(m, &order))
		.collect::<Result<_>>()?;

	let mut variations: Vec<(u32, Vec<TupleVariation>)> = Vec::new();
	for (gid, glyph) in order.names().iter().enumerate() {
		log::debug!("building gvar for glyph '{glyph}'");
		let mut all_data: Vec<Option<&GlyphData>> = datas.iter().map(|d| d[gid].as_ref()).collect();
		let default = all_data[default_master_index].ok_or_else(|| {
			fontoxide_ttlib::Error::ttlib("AttributeError: 'NoneType' object is not subscriptable")
		})?;
		if default.1.number_of_contours != 0 {
			// If the default master is not empty, interpret empty non-default masters
			// as missing glyphs from a sparse master.
			all_data = all_data
				.into_iter()
				.map(|d| d.filter(|d| d.1.number_of_contours != 0))
				.collect();
		}
		let mask: Vec<bool> = all_data.iter().map(Option::is_some).collect();
		let model = master_model.sub_model(&mask)?;
		let all_data: Vec<&GlyphData> = all_data.into_iter().flatten().collect();

		let control = &all_data[0].1;
		if !all_data.iter().all(|d| d.1 == *control) {
			log::warn!("glyph {glyph} has incompatible masters; skipping");
			continue;
		}

		// Update gvar.
		let all_coords: Vec<Vec<f64>> = all_data
			.iter()
			.map(|d| d.0.iter().flat_map(|p| [p.0, p.1]).collect())
			.collect();
		let deltas = model.get_deltas_vec(&all_coords, f64::round_ties_even)?;
		let supports = &model.supports;
		if deltas.len() != supports.len() {
			return Err(assertion("len(deltas) == len(supports)"));
		}

		// Prepare for IUP optimization.
		let orig_coords: Vec<(f64, f64)> = deltas[0].chunks(2).map(|c| (c[0], c[1])).collect();
		let end_pts: Vec<usize> = control.end_pts.iter().map(|e| *e as usize).collect();

		let mut glyph_variations = Vec::new();
		for (delta, support) in deltas.iter().zip(supports).skip(1) {
			if delta.iter().all(|v| *v == 0.0) {
				continue;
			}
			let mut var = TupleVariation {
				axes: support_axes(support),
				coordinates: delta
					.chunks(2)
					.map(|c| Some(Delta::Point(delta_number(c[0]), delta_number(c[1]))))
					.collect(),
			};
			if optimize {
				crate::instancer::tuple::optimize_with_tolerance(
					&mut var,
					&orig_coords,
					&end_pts,
					tolerance,
				)?;
			}
			glyph_variations.push(var);
		}
		variations.push((gid as u32, glyph_variations));
	}
	let gvar = Gvar {
		variations,
		extended: gvar_tag == Tag::new(b"GVAR"),
		..Default::default()
	};
	font.insert(Box::new(gvar));
	Ok(())
}

/// `_remove_TTHinting`.
fn remove_tt_hinting(font: &mut Font) -> Result<()> {
	for t in [b"cvar", b"cvt ", b"fpgm", b"prep"] {
		font.remove(Tag::new(t));
	}
	let maxp_tag = tt_tag(font, "maxp");
	let maxp = font
		.get_at::<Maxp>(maxp_tag)?
		.ok_or_else(|| key_error(&maxp_tag.to_string()))?;
	maxp.v1.max_twilight_points = 0;
	maxp.v1.max_storage = 0;
	maxp.v1.max_function_defs = 0;
	maxp.v1.max_instruction_defs = 0;
	maxp.v1.max_stack_elements = 0;
	maxp.v1.max_size_of_instructions = 0;
	maxp.v1.max_zones = 1;
	let glyf_tag = tt_tag(font, "glyf");
	let glyf = font
		.get_at::<Glyf>(glyf_tag)?
		.ok_or_else(|| key_error(&glyf_tag.to_string()))?;
	let extended = glyf.extended;
	for glyph in glyf.glyphs.values_mut() {
		glyph.trim_ext(true, extended)?;
	}
	// TODO: Modify gasp table to deactivate gridfitting for all ranges?
	Ok(())
}

/// `_merge_TTHinting`: checks that the hinting of the masters is compatible, and
/// makes a `cvar` table from their `cvt ` tables.
#[allow(clippy::type_complexity)]
fn merge_tt_hinting(
	font: &mut Font,
	master_model: &VariationModel,
	masters: &mut [Font],
) -> Result<()> {
	log::info!("Merging TT hinting");
	if font.contains(Tag::new(b"cvar")) {
		return Err(assertion("\"cvar\" not in font"));
	}

	// fpgm and prep table
	for t in ["fpgm", "prep"] {
		let mut all = Vec::new();
		for m in masters.iter_mut() {
			if let Some(p) = program_table(m, t)? {
				all.push(p);
			}
		}
		if all.is_empty() {
			continue;
		}
		let font_pgm = program_table(font, t)?.flatten();
		if all.iter().any(|p| *p != font_pgm) {
			log::warn!("Masters have incompatible {t} tables, hinting is discarded.");
			return remove_tt_hinting(font);
		}
	}

	// glyf table
	let glyf_tag = tt_tag(font, "glyf");
	let order = font.glyph_order()?;
	let mut master_programs: Vec<(
		Arc<GlyphOrder>,
		HashMap<u32, Option<fontoxide_ttlib::tables::tt_program::Program>>,
	)> = Vec::new();
	for m in masters.iter_mut() {
		let master_order = m.glyph_order()?;
		let glyf = m
			.get_at::<Glyf>(glyf_tag)?
			.ok_or_else(|| key_error(&glyf_tag.to_string()))?;
		let programs = glyf
			.glyphs
			.iter()
			.map(|(g, glyph)| (*g, glyph.program.clone()))
			.collect();
		master_programs.push((master_order, programs));
	}
	let font_glyf = font
		.get_at::<Glyf>(glyf_tag)?
		.ok_or_else(|| key_error(&glyf_tag.to_string()))?;
	let mut incompatible = None;
	for (gid, glyph) in &font_glyf.glyphs {
		let name = order.name(*gid);
		let all: Vec<Option<&fontoxide_ttlib::tables::tt_program::Program>> = master_programs
			.iter()
			.map(|(o, p)| o.id(&name).and_then(|g| p.get(&g)).and_then(Option::as_ref))
			.collect();
		let truthy = |p: &Option<&fontoxide_ttlib::tables::tt_program::Program>| {
			p.is_some_and(|p| !p.is_empty())
		};
		if !all.iter().any(truthy) {
			continue;
		}
		let font_pgm = glyph.program.as_ref();
		if all.iter().filter(|p| truthy(p)).any(|p| *p != font_pgm) {
			incompatible = Some(name.into_owned());
			break;
		}
	}
	if let Some(name) = incompatible {
		log::warn!(
			"Masters have incompatible glyph programs in glyph '{name}', hinting is discarded."
		);
		// TODO Only drop hinting from this glyph.
		return remove_tt_hinting(font);
	}

	// cvt table
	let mut all_cvs: Vec<Option<Vec<i16>>> = Vec::new();
	for m in masters.iter_mut() {
		all_cvs.push(m.get::<Cvt>()?.map(|c| c.values.clone()));
	}
	let non_none: Vec<&Vec<i16>> = all_cvs.iter().flatten().collect();
	if non_none.is_empty() {
		// There is no cvt table to make a cvar table from, we're done here.
		return Ok(());
	}
	if non_none.iter().any(|c| c.len() != non_none[0].len()) {
		log::warn!("Masters have incompatible cvt tables, hinting is discarded.");
		return remove_tt_hinting(font);
	}

	let mask: Vec<bool> = all_cvs.iter().map(Option::is_some).collect();
	let model = master_model.sub_model(&mask)?;
	let values: Vec<Vec<f64>> = non_none
		.iter()
		.map(|c| c.iter().map(|v| f64::from(*v)).collect())
		.collect();
	let deltas = model.get_deltas_vec(&values, f64::round_ties_even)?;
	let mut variations = Vec::new();
	for (delta, support) in deltas.iter().zip(&model.supports).skip(1) {
		if delta.iter().all(|v| *v == 0.0) {
			continue;
		}
		variations.push(TupleVariation {
			axes: support_axes(support),
			coordinates: delta
				.iter()
				.map(|v| Some(Delta::Value(delta_number(*v))))
				.collect(),
		});
	}

	// We can build the cvar table now.
	if !variations.is_empty() {
		let cvar = Cvar {
			variations,
			..Default::default()
		};
		font.insert(Box::new(cvar));
	}
	Ok(())
}

/// The program of `fpgm` or `prep`: `None` if the font has no such table, else
/// its program, if any.
#[allow(clippy::option_option)]
fn program_table(
	font: &mut Font,
	t: &str,
) -> Result<Option<Option<fontoxide_ttlib::tables::tt_program::Program>>> {
	Ok(match t {
		"fpgm" => font.get::<Fpgm>()?.map(|t| t.program.clone()),
		_ => font.get::<Prep>()?.map(|t| t.program.clone()),
	})
}

const USE_MY_METRICS: i64 = 0x0200;

/// `_unset_inconsistent_use_my_metrics_flags`: clears `USE_MY_METRICS` on the
/// components of composite glyphs if the masters set it inconsistently, so that
/// advance widths are not determined by that single component's phantom points.
fn unset_inconsistent_use_my_metrics_flags(vf: &mut Font, masters: &mut [Font]) -> Result<()> {
	let glyf_tag = tt_tag(vf, "glyf");
	// The composite glyphs of the masters, by name, with their component names
	// and flags.
	type Composite = Vec<(String, i64)>;
	let mut master_glyfs: Vec<HashMap<String, Option<Composite>>> = Vec::new();
	for m in masters.iter_mut() {
		if !m.contains(glyf_tag) {
			continue;
		}
		let order = m.glyph_order()?;
		let glyf = m
			.get_at::<Glyf>(glyf_tag)?
			.ok_or_else(|| key_error(&glyf_tag.to_string()))?;
		let glyphs = glyf
			.glyphs
			.iter()
			.map(|(g, glyph)| {
				let comps = glyph.is_composite().then(|| {
					glyph
						.components
						.iter()
						.map(|c| (order.name(c.glyph).into_owned(), c.flags))
						.collect()
				});
				(order.name(*g).into_owned(), comps)
			})
			.collect();
		master_glyfs.push(glyphs);
	}
	if master_glyfs.is_empty() {
		// Should not happen: at least the base master (as copied into vf) has glyf.
		return Ok(());
	}
	let order = vf.glyph_order()?;
	let glyf = vf
		.get_at::<Glyf>(glyf_tag)?
		.ok_or_else(|| key_error(&glyf_tag.to_string()))?;
	for (gid, glyph) in glyf.glyphs.iter_mut() {
		if !glyph.is_composite() {
			continue;
		}
		// Collect indices of component(s) that carry the USE_MY_METRICS flag.
		let flagged: Vec<(usize, String)> = glyph
			.components
			.iter()
			.enumerate()
			.filter(|(_, c)| c.flags & USE_MY_METRICS != 0)
			.map(|(i, c)| (i, order.name(c.glyph).into_owned()))
			.collect();
		if flagged.is_empty() {
			// Nothing to fix.
			continue;
		}
		let name = order.name(*gid).into_owned();
		let expected = glyph.components.len();
		let inconsistent = master_glyfs.iter().any(|g| {
			// 'sparse' glyph master doesn't contribute.
			let Some(Some(comps)) = g.get(&name) else {
				return false;
			};
			if comps.len() != expected {
				return false;
			}
			for (i, base) in &flagged {
				let (comp_name, flags) = &comps[*i];
				if comp_name != base {
					break;
				}
				if flags & USE_MY_METRICS == 0 {
					return true;
				}
			}
			false
		});
		if inconsistent {
			let names: Vec<&String> = flagged.iter().map(|(_, n)| n).collect();
			log::info!(
				"Composite glyph '{name}' has inconsistent USE_MY_METRICS flags across masters; clearing the flag on component{} {:?}",
				if names.len() > 1 { "s" } else { "" },
				names
			);
			for (i, _) in &flagged {
				glyph.components[*i].flags &= !USE_MY_METRICS;
			}
			glyph.data = None;
		}
	}
	Ok(())
}

/// `_MetricsFields`: `HVAR_FIELDS` or `VVAR_FIELDS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetricsFields {
	/// `HVAR_FIELDS`.
	Hvar,
	/// `VVAR_FIELDS`.
	Vvar,
}

/// The deltas and supports of a value of each glyph, with the masters that have
/// the value (which identify the supports, as `id(supports)` does).
pub(crate) type DeltasAndSupports = (Vec<f64>, Vec<Support>, Vec<bool>);

/// `_add_VHVAR`: adds `HVAR` or `VVAR` from the deltas of the advances (and
/// vertical origins) of each glyph.
pub(crate) fn add_vhvar(
	font: &mut Font,
	axis_tags: &[String],
	fields: MetricsFields,
	advances: &[DeltasAndSupports],
	v_origs: Option<&[DeltasAndSupports]>,
	single_model: Option<bool>,
) -> Result<()> {
	let table_tag = match fields {
		MetricsFields::Hvar => "HVAR",
		MetricsFields::Vvar => "VVAR",
	};
	if font.contains(tag(table_tag)) {
		return Err(assertion("tableTag not in font"));
	}
	let order = font.glyph_order()?;
	log::info!("Generating {table_tag}");

	// `allEqual(id(supports))`: the masters with values identify the sub-model
	// whose supports they are.
	let single_model = match v_origs {
		Some(_) => false,
		None => single_model.unwrap_or_else(|| advances.iter().all(|a| a.2 == advances[0].2)),
	};

	let mut direct_store = None;
	// The direct (no-map) store indexes a single VarData by glyph id, but
	// VarData.ItemCount is uint16, so it cannot represent > 0xFFFF glyphs. Skip it
	// past the boundary and rely on the indirect (mapped) store.
	if single_model && order.len() <= 0xFFFF && !advances.is_empty() {
		// Build direct mapping.
		let supports: Vec<BTreeMap<String, (f64, f64, f64)>> =
			advances[0].1.iter().skip(1).map(support_axes).collect();
		let region_list = var_store::build_var_region_list(&supports, axis_tags)?;
		let indexes: Vec<u16> = (0..supports.len() as u16).collect();
		let mut var_data = var_store::build_var_data(indexes, Vec::new(), false);
		for (deltas, _, _) in advances {
			add_item(&mut var_data, deltas)?;
		}
		var_store::calculate_num_shorts(&mut var_data, true);
		let mut store = var_store::build_var_store(region_list, vec![var_data]);
		// Remove unused regions from VarRegionList.
		var_store::prune_regions(&mut store)?;
		direct_store = Some(store);
	}

	// Build optimized indirect mapping.
	let mut store_builder = OnlineVarStoreBuilder::new(axis_tags.to_vec())?;
	let mut adv_mapping = Vec::with_capacity(order.len());
	for (deltas, supports, _) in advances {
		store_builder.set_supports(supports);
		adv_mapping.push(store_builder.store_deltas_rounded(deltas, |v| v)?);
	}
	let mut v_orig_mapping_raw = Vec::new();
	if let Some(v_origs) = v_origs {
		for (deltas, supports, _) in v_origs {
			store_builder.set_supports(supports);
			v_orig_mapping_raw.push(store_builder.store_deltas_rounded(deltas, |v| v)?);
		}
	}
	let mut indirect_store = store_builder.finish(true);
	let mapping2 = var_store::optimize(&mut indirect_store, false)?;
	let remap = |v: &u64| -> Result<u64> {
		mapping2
			.get(v)
			.copied()
			.ok_or_else(|| key_error(&v.to_string()))
	};
	let adv_mapping: Vec<u64> = adv_mapping.iter().map(remap).collect::<Result<_>>()?;
	let gids: Vec<u32> = (0..order.len() as u32).collect();
	let advance_mapping = build_var_idx_map(adv_mapping, gids.clone());
	let v_orig_map: Vec<u64> = v_orig_mapping_raw
		.iter()
		.map(remap)
		.collect::<Result<_>>()?;

	let mut use_direct = false;
	if let Some(direct) = &direct_store {
		// Compile both, see which is more compact.
		let mut graph = WriterGraph::new(tag(table_tag));
		direct.compile(&mut graph.root(), font)?;
		let direct_size = graph.get_all_data()?.len();

		let mut graph = WriterGraph::new(tag(table_tag));
		{
			let mut root = graph.root();
			indirect_store.compile(&mut root, font)?;
			advance_mapping.compile(&mut root, font)?;
		}
		let indirect_size = graph.get_all_data()?.len();
		use_direct = direct_size < indirect_size;
	}

	let (store, advance_mapping, v_orig_mapping) = if use_direct {
		(direct_store.unwrap_or_default(), None, None)
	} else {
		let v = v_origs.map(|_| build_var_idx_map(v_orig_map, gids));
		(indirect_store, Some(advance_mapping), v)
	};

	match fields {
		MetricsFields::Hvar => {
			let hvar = Hvar {
				table: ot::HVAR {
					version: 0x0001_0000,
					var_store: Some(Box::new(store)),
					adv_width_map: advance_mapping.map(Box::new),
					lsb_map: None,
					rsb_map: None,
				},
			};
			font.insert(Box::new(hvar));
		}
		MetricsFields::Vvar => {
			let vvar = Vvar {
				table: ot::VVAR {
					version: 0x0001_0000,
					var_store: Some(Box::new(store)),
					adv_height_map: advance_mapping.map(Box::new),
					tsb_map: None,
					bsb_map: None,
					v_org_map: v_orig_mapping.map(Box::new),
				},
			};
			font.insert(Box::new(vvar));
		}
	}
	Ok(())
}

/// `VarData.addItem(deltas, round=noRound)`.
fn add_item(data: &mut ot::VarData, deltas: &[f64]) -> Result<()> {
	let count_us = data.var_region_index.len();
	let deltas: &[f64] = if count_us + 1 == deltas.len() {
		&deltas[1..]
	} else if count_us == deltas.len() {
		deltas
	} else {
		return Err(assertion("countUs == countThem"));
	};
	let item: Vec<i64> = deltas
		.iter()
		.map(|d| {
			if d.fract() == 0.0 {
				Ok(*d as i64)
			} else {
				Err(fontoxide_ttlib::Error::ttlib(format!(
					"TypeError: delta {d} is not an integer"
				)))
			}
		})
		.collect::<fontoxide_ttlib::Result<_>>()?;
	data.item.push(item);
	data.item_count = Some(data.item.len() as u32);
	Ok(())
}

/// `masterModel.getDeltasAndSupports(items, round=round)`, with the mask of the
/// masters that have a value.
fn deltas_and_supports(model: &VariationModel, items: &[Option<f64>]) -> Result<DeltasAndSupports> {
	let mask: Vec<bool> = items.iter().map(Option::is_some).collect();
	let (deltas, supports) = model.get_deltas_and_supports_rounded(items, f64::round_ties_even)?;
	Ok((deltas, supports, mask))
}

/// `_get_advance_metrics`: the deltas of the advances, and of the vertical
/// origins for `VVAR` if the masters have `VORG`.
#[allow(clippy::type_complexity)]
fn get_advance_metrics(
	font: &mut Font,
	master_model: &VariationModel,
	masters: &mut [Font],
	fields: MetricsFields,
) -> Result<(Vec<DeltasAndSupports>, Option<Vec<DeltasAndSupports>>)> {
	let order = font.glyph_order()?;
	// Build list of source font advance widths for each glyph.
	let metrics_tag = match fields {
		MetricsFields::Hvar => tt_tag(font, "hmtx"),
		MetricsFields::Vvar => tt_tag(font, "vmtx"),
	};
	let mut adv_metricses: Vec<HashMap<String, u16>> = Vec::new();
	for m in masters.iter_mut() {
		let master_order = m.glyph_order()?;
		let metrics = match fields {
			MetricsFields::Hvar => m.get_at::<Hmtx>(metrics_tag)?.map(|t| t.metrics.clone()),
			MetricsFields::Vvar => m.get_at::<Vmtx>(metrics_tag)?.map(|t| t.metrics.clone()),
		}
		.ok_or_else(|| key_error(&metrics_tag.to_string()))?;
		adv_metricses.push(
			metrics
				.iter()
				.map(|(g, (adv, _))| (master_order.name(*g).into_owned(), *adv))
				.collect(),
		);
	}

	// Build list of source font vertical origin coords for each glyph.
	let mut v_orig_metricses: Option<Vec<(HashMap<String, i16>, i16)>> = None;
	if fields == MetricsFields::Vvar && masters[0].contains(Tag::new(b"VORG")) {
		let mut list = Vec::new();
		for m in masters.iter_mut() {
			let master_order = m.glyph_order()?;
			let vorg = m.get::<Vorg>()?.ok_or_else(|| key_error("VORG"))?;
			list.push((
				vorg.v_origin_records
					.iter()
					.map(|(g, v)| (master_order.name(*g).into_owned(), *v))
					.collect(),
				vorg.default_vert_origin_y,
			));
		}
		v_orig_metricses = Some(list);
	}

	// HACK: we treat width 65535 as a sentinel value to signal that a glyph from a
	// non-default master should not participate in computing {H,V}VAR, as if it
	// were missing. Allows to variate other glyph-related data independently from
	// glyph metrics.
	let sparse_advance = 0xFFFF;
	let mut advances = Vec::with_capacity(order.len());
	for glyph in order.names() {
		let values: Vec<Option<f64>> = adv_metricses
			.iter()
			.map(|m| {
				m.get(glyph)
					.filter(|a| **a != sparse_advance)
					.map(|a| f64::from(*a))
			})
			.collect();
		advances.push(deltas_and_supports(master_model, &values)?);
	}

	let mut v_origs = None;
	if let Some(metricses) = v_orig_metricses {
		let mut list = Vec::with_capacity(order.len());
		for glyph in order.names() {
			// We need to supply a vOrigs tuple with non-None default values for each
			// glyph. vOrigMetricses contains values only for those glyphs which have
			// a non-default vOrig.
			let values: Vec<Option<f64>> = metricses
				.iter()
				.map(|(m, default)| Some(f64::from(*m.get(glyph).unwrap_or(default))))
				.collect();
			list.push(deltas_and_supports(master_model, &values)?);
		}
		// `if vOrigDeltasAndSupports:` of an empty dict.
		if !list.is_empty() {
			v_origs = Some(list);
		}
	}
	Ok((advances, v_origs))
}

fn add_hvar(
	font: &mut Font,
	model: &VariationModel,
	masters: &mut [Font],
	axis_tags: &[String],
) -> Result<()> {
	let (advances, v_origs) = get_advance_metrics(font, model, masters, MetricsFields::Hvar)?;
	add_vhvar(
		font,
		axis_tags,
		MetricsFields::Hvar,
		&advances,
		v_origs.as_deref(),
		None,
	)
}

fn add_vvar(
	font: &mut Font,
	model: &VariationModel,
	masters: &mut [Font],
	axis_tags: &[String],
) -> Result<()> {
	let (advances, v_origs) = get_advance_metrics(font, model, masters, MetricsFields::Vvar)?;
	add_vhvar(
		font,
		axis_tags,
		MetricsFields::Vvar,
		&advances,
		v_origs.as_deref(),
		None,
	)
}

/// `_add_MVAR`.
fn add_mvar(
	font: &mut Font,
	master_model: &VariationModel,
	masters: &mut [Font],
	axis_tags: &[String],
) -> Result<()> {
	log::info!("Generating MVAR");
	let mut store_builder = OnlineVarStoreBuilder::new(axis_tags.to_vec())?;
	// LENIENT(L36): the `OS/2` fields of versions before 2 (`sxHeight`, `sCapHeight`)
	// exist in Rust; fontTools fails on them with an AttributeError.
	let mut records: Vec<ot::MetricsValueRecord> = Vec::new();
	let mut last_table_tag: Option<String> = None;
	let mut tables: Option<Vec<usize>> = None;
	// HACK: we need to special-case post.underlineThickness and .underlinePosition
	// and unilaterally/arbitrarily define a sentinel value to distinguish the case
	// when a post table is present in a given master simply because that's where
	// the glyph names in TrueType must be stored, but the underline values are not
	// meant to be used for building MVAR's deltas. The value of -0x8000 (-32768)
	// the minimum FWord (int16) value, was chosen for its unlikelyhood to appear in
	// real-world underline position/thickness values.
	let special = |t: &str| matches!(t, "unds" | "undo");
	let mut entries = MVAR_ENTRIES.to_vec();
	entries.sort_by(|a, b| a.1.cmp(&b.1));
	let mut font_table_tag = String::new();
	for (value_tag, (table_tag, item)) in entries {
		let this_tag = get_mvar_table_tag(font, table_tag);
		// For each tag, fetch the associated table from all fonts (or not when we
		// are still looking at a tag from the same tables) and set up the variation
		// model for them.
		if last_table_tag.as_deref() != Some(&this_tag) {
			tables = None;
			if font.contains(tag(&this_tag)) {
				let mut present = Vec::with_capacity(masters.len());
				for m in masters.iter_mut() {
					let master_tag = get_mvar_table_tag(m, table_tag);
					let value = crate::mvar::get_value(m, &master_tag, item)?;
					present.push(match value {
						None => false,
						Some(v) => !(special(value_tag) && v == -0x8000),
					});
				}
				let model = master_model.sub_model(&present)?;
				store_builder.set_model(&model);
				tables = Some(
					present
						.iter()
						.enumerate()
						.filter(|(_, p)| **p)
						.map(|(i, _)| i)
						.collect(),
				);
			}
			last_table_tag = Some(this_tag.clone());
			font_table_tag = this_tag;
		}
		let Some(tables) = &tables else {
			// Tag not applicable to the master font.
			continue;
		};

		// TODO support gasp entries
		let mut master_values = Vec::with_capacity(tables.len());
		for &i in tables {
			let master_tag = get_mvar_table_tag(&masters[i], table_tag);
			let v = crate::mvar::get_value(&mut masters[i], &master_tag, item)?
				.ok_or_else(|| key_error(&master_tag))?;
			master_values.push(v as f64);
		}
		let (base, var_idx) = if master_values.iter().all(|v| *v == master_values[0]) {
			(master_values[0], None)
		} else {
			let (base, idx) = store_builder.store_masters(&master_values)?;
			(base, Some(idx))
		};
		crate::mvar::set_value(font, &font_table_tag, item, base as i64)?;
		let Some(var_idx) = var_idx else {
			continue;
		};
		log::info!("\t{value_tag}: {font_table_tag}.{item}\t{master_values:?}");
		records.push(ot::MetricsValueRecord {
			value_tag: tag(value_tag),
			var_idx: fontoxide_ttlib::binary::fit(var_idx, "VarIdx")?,
			more_bytes: Vec::new(),
		});
	}

	if font.contains(Tag::new(b"MVAR")) {
		return Err(assertion("\"MVAR\" not in font"));
	}
	if !records.is_empty() {
		let mut store = store_builder.finish(true);
		// Optimize.
		let mapping = var_store::optimize(&mut store, true)?;
		for rec in &mut records {
			let new = mapping
				.get(&u64::from(rec.var_idx))
				.copied()
				.ok_or_else(|| key_error(&rec.var_idx.to_string()))?;
			rec.var_idx = fontoxide_ttlib::binary::fit(new, "VarIdx")?;
		}
		records.sort_by_key(|r| r.value_tag);
		let mvar = Mvar {
			table: ot::MVAR {
				version: 0x0001_0000,
				reserved: 0,
				value_record_size: Some(8),
				value_record_count: Some(records.len() as u32),
				var_store: Some(Box::new(store)),
				value_record: records,
			},
		};
		font.insert(Box::new(mvar));
	}
	Ok(())
}

fn master_infos(masters: &mut [Font], paths: &[Option<String>]) -> Result<Vec<MasterInfo>> {
	masters
		.iter_mut()
		.enumerate()
		.map(|(i, m)| MasterInfo::of(m, paths.get(i).and_then(|p| p.as_deref())))
		.collect()
}

/// `_add_BASE`.
fn add_base(
	font: &mut Font,
	model: &VariationModel,
	masters: &mut [Font],
	axis_tags: &[String],
	infos: &[MasterInfo],
) -> Result<()> {
	log::info!("Generating BASE");
	let mut merger = Merger::variation(model, axis_tags, font, infos)?;
	let mut refs: Vec<&mut Font> = masters.iter_mut().collect();
	merger.merge_tables(font, &mut refs, &["BASE"])?;
	let store = merger.store_builder.finish(true);
	if store.var_data.is_empty() {
		return Ok(());
	}
	let base = &mut font.require::<Base>()?.table;
	if base.version != 0x0001_0000 {
		return Err(assertion("base.Version == 0x00010000"));
	}
	base.version = 0x0001_0001;
	base.var_store = Some(Box::new(store));
	Ok(())
}

/// `remap_device_varidxes`: the variation indices of the `Device` tables.
fn remap_device_varidxes<T: OtTable>(table: &mut T, map: &HashMap<u64, u64>) -> Result<()> {
	table.visit_mut(&mut |any| {
		if let Some(device) = any.downcast_mut::<ot::Device>()
			&& device.delta_format == 0x8000
		{
			let old = (u64::from(device.start_size) << 16) + u64::from(device.end_size);
			let new = *map
				.get(&old)
				.ok_or_else(|| fontoxide_ttlib::Error::ttlib(format!("KeyError: {old}")))?;
			device.start_size = (new >> 16) as u16;
			device.end_size = (new & 0xFFFF) as u16;
		}
		Ok(())
	})?;
	Ok(())
}

/// `_merge_OTL`.
fn merge_otl(
	font: &mut Font,
	model: &VariationModel,
	masters: &mut [Font],
	axis_tags: &[String],
	infos: &[MasterInfo],
) -> Result<()> {
	let otl_tags = ["GSUB", "GDEF", "GPOS"];
	if !otl_tags.iter().any(|t| font.contains(tag(t))) {
		return Ok(());
	}
	log::info!("Merging OpenType Layout tables");
	let mut merger = Merger::variation(model, axis_tags, font, infos)?;
	{
		let mut refs: Vec<&mut Font> = masters.iter_mut().collect();
		merger.merge_tables(font, &mut refs, &otl_tags)?;
	}
	let store = merger.store_builder.finish(true);
	if store.var_data.is_empty() {
		return Ok(());
	}
	match font.get::<Gdef>()? {
		Some(gdef) => {
			// v1.4 is the beyond-64k GDEF version; bump to >= v1.3 to carry the
			// VarStore.
			if gdef.table.version > 0x0001_0004 {
				return Err(assertion("GDEF.Version <= 0x00010004"));
			}
			gdef.table.version = gdef.table.version.max(0x0001_0003);
		}
		None => {
			let gdef = Gdef {
				table: ot::GDEF {
					version: 0x0001_0003,
					..Default::default()
				},
			};
			font.insert(Box::new(gdef));
		}
	}
	let gdef = font.require::<Gdef>()?;
	gdef.table.var_store = Some(Box::new(store));

	// Optimize.
	let varidx_map = match gdef.table.var_store.as_deref_mut() {
		Some(store) => var_store::optimize(store, true)?,
		None => HashMap::new(),
	};
	remap_device_varidxes(&mut gdef.table, &varidx_map)?;
	if let Some(t) = font.get::<Gsub>()? {
		remap_device_varidxes(&mut t.table, &varidx_map)?;
	}
	if let Some(t) = font.get::<Gpos>()? {
		remap_device_varidxes(&mut t.table, &varidx_map)?;
	}
	Ok(())
}

/// `_add_GSUB_feature_variations`.
fn add_gsub_feature_variations(
	font: &mut Font,
	axes: &Axes,
	internal_axis_supports: &IndexMap<String, (f64, f64, f64)>,
	rules: &[RuleDescriptor],
	feature_tags: &[Tag],
) -> Result<()> {
	let normalize = |name: &str, value: f64| -> Result<f64> {
		let support = internal_axis_supports
			.get(name)
			.ok_or_else(|| key_error(name))?;
		Ok(normalize_value_decreasing(value, *support, false)?)
	};
	log::info!("Generating GSUB FeatureVariations");
	let mut conditional_subs: Vec<(Region, Substitution)> = Vec::new();
	for rule in rules {
		let mut region: Region = Vec::new();
		for conditions in &rule.condition_sets {
			let mut space = AxisBox::new();
			for condition in conditions {
				let axis_name = &condition.name;
				let (mut minimum, mut maximum) = (condition.minimum, condition.maximum);
				let support = internal_axis_supports
					.get(axis_name)
					.ok_or_else(|| key_error(axis_name))?;
				if support.0 > support.2 {
					std::mem::swap(&mut minimum, &mut maximum);
				}
				let minimum = match minimum {
					Some(v) => normalize(axis_name, v.to_f64())?,
					None => -1.0,
				};
				let maximum = match maximum {
					Some(v) => normalize(axis_name, v.to_f64())?,
					None => 1.0,
				};
				let axis = axes.get(axis_name).ok_or_else(|| key_error(axis_name))?;
				space.insert(axis_tag(axis), (minimum, maximum));
			}
			region.push(space);
		}
		let subs: Substitution = rule.subs.iter().cloned().collect();
		conditional_subs.push((region, subs));
	}
	add_feature_variations(font, &conditional_subs, feature_tags)?;
	Ok(())
}

/// What `load_designspace` gives (`_DesignSpaceData`).
#[derive(Debug, Clone)]
pub struct DesignSpaceData {
	/// `axes`: the continuous axes by name.
	pub axes: Axes,
	/// `axisMappings`.
	pub axis_mappings: Vec<AxisMappingDescriptor>,
	/// `internal_axis_supports`: the design minimum, default and maximum of each
	/// axis by name.
	pub internal_axis_supports: IndexMap<String, (f64, f64, f64)>,
	/// `base_idx`: the master at the default location.
	pub base_idx: Option<usize>,
	/// `normalized_master_locs`, by axis name.
	pub normalized_master_locs: Vec<DictLocation>,
	/// `masters`.
	pub masters: Vec<SourceDescriptor>,
	/// `instances`.
	pub instances: Vec<InstanceDescriptor>,
	/// `rules`.
	pub rules: Vec<RuleDescriptor>,
	/// `rulesProcessingLast`.
	pub rules_processing_last: bool,
	/// `lib`.
	pub lib: plistlib::Dict,
}

/// `load_designspace`: validates the document and fills in what varLib needs:
/// axis tags and names, and full design locations of the sources and instances.
pub fn load_designspace(
	ds: &mut DesignSpaceDocument,
	log_enabled: bool,
	require_sources: bool,
) -> Result<DesignSpaceData> {
	if require_sources && ds.sources.is_empty() {
		return Err(validation("Designspace must have at least one source."));
	}

	// TODO: Use fontTools.designspaceLib.tagForAxisName instead.
	let standard_axis_map: [(&str, &str, &str); 5] = [
		("weight", "wght", "Weight"),
		("width", "wdth", "Width"),
		("slant", "slnt", "Slant"),
		("optical", "opsz", "Optical Size"),
		("italic", "ital", "Italic"),
	];

	// Setup axes.
	if ds.axes.is_empty() {
		return Err(validation("Designspace must have at least one axis."));
	}

	let mut axes = Axes::new();
	for (axis_index, axis) in ds.axes.iter_mut().enumerate() {
		let Axis::Continuous(axis) = axis else {
			return Err(continuous(axis)
				.err()
				.unwrap_or_else(|| validation("discrete axis")));
		};
		let axis_name = match axis.name.clone().filter(|n| !n.is_empty()) {
			Some(n) => n,
			None => {
				let Some(t) = axis.tag.clone().filter(|t| !t.is_empty()) else {
					return Err(validation(format!(
						"Axis at index {axis_index} needs a tag."
					)));
				};
				axis.name = Some(t.clone());
				t
			}
		};
		if let Some((_, t, label)) = standard_axis_map.iter().find(|(n, _, _)| *n == axis_name) {
			if axis.tag.is_none() {
				axis.tag = Some((*t).to_owned());
			}
			if axis.label_names.is_empty() {
				axis.label_names
					.insert("en".to_owned(), (*label).to_owned());
			}
		} else {
			if axis.tag.as_deref().is_none_or(str::is_empty) {
				return Err(validation(format!(
					"Axis at index {axis_index} needs a tag."
				)));
			}
			if axis.label_names.is_empty() {
				axis.label_names.insert("en".to_owned(), axis_name.clone());
			}
		}
		axes.insert(axis_name, axis.clone());
	}
	if log_enabled {
		log::info!("Axes:\n{:?}", axes.values().collect::<Vec<_>>());
	}

	let axis_mappings = ds.axis_mappings.clone();
	if !axis_mappings.is_empty() && log_enabled {
		log::info!("Mappings:\n{axis_mappings:?}");
	}

	// Check all master and instance locations are valid and fill in defaults.
	let doc = ds.clone();
	let check = |obj_name: &str,
	             loc: &fontoxide_designspacelib::AnisotropicLocation|
	 -> Result<()> {
		for axis_name in loc.keys() {
			if !axes.contains_key(axis_name) {
				return Err(validation(format!(
					"Location axis '{axis_name}' unknown for '{obj_name}'."
				)));
			}
		}
		for (axis_name, axis) in &axes {
			let v = axis.map_backward(*loc.get(axis_name).ok_or_else(|| key_error(axis_name))?)?;
			if !(axis.minimum <= v && v <= axis.maximum) {
				return Err(validation(format!(
					"Source or instance '{obj_name}' has out-of-range location for axis '{axis_name}': is mapped to {v} but must be in mapped range [{}..{}] (NOTE: all values are in user-space).",
					axis.minimum, axis.maximum
				)));
			}
		}
		Ok(())
	};
	for source in &mut ds.sources {
		let obj_name = source
			.name
			.clone()
			.filter(|n| !n.is_empty())
			.or_else(|| source.style_name.clone().filter(|n| !n.is_empty()))
			.unwrap_or_default();
		let loc = source.get_full_design_location(&doc)?;
		source.design_location = loc.clone();
		check(&obj_name, &loc)?;
	}
	for instance in &mut ds.instances {
		let obj_name = instance
			.name
			.clone()
			.filter(|n| !n.is_empty())
			.or_else(|| instance.style_name.clone().filter(|n| !n.is_empty()))
			.unwrap_or_default();
		let loc = instance.get_full_design_location(&doc)?;
		instance.design_location = loc.clone();
		check(&obj_name, &loc)?;
	}

	// Normalize master locations.
	let doc = ds.clone();
	let internal_master_locs: Vec<fontoxide_designspacelib::AnisotropicLocation> = ds
		.sources
		.iter()
		.map(|s| s.get_full_design_location(&doc))
		.collect::<fontoxide_designspacelib::Result<_>>()?;
	if log_enabled {
		log::info!("Internal master locations:\n{internal_master_locs:?}");
	}

	// TODO This mapping should ideally be moved closer to logic in _add_fvar/avar.
	let mut internal_axis_supports: IndexMap<String, (f64, f64, f64)> = IndexMap::new();
	for axis in axes.values() {
		internal_axis_supports.insert(
			axis_name(axis),
			(
				map_forward(axis, axis.minimum)?,
				map_forward(axis, axis.default)?,
				map_forward(axis, axis.maximum)?,
			),
		);
	}
	if log_enabled {
		log::info!("Internal axis supports:\n{internal_axis_supports:?}");
	}

	// `models.normalizeLocation(m, internal_axis_supports, allow_decreasing=True)`.
	let mut normalized_master_locs: Vec<DictLocation> = Vec::new();
	for loc in &internal_master_locs {
		let mut out = DictLocation::new();
		for (name, triple) in &internal_axis_supports {
			let v = loc.get(name).map_or(triple.1, |v| v.x().to_f64());
			out.push((name.clone(), normalize_value_decreasing(v, *triple, false)?));
		}
		normalized_master_locs.push(out);
	}
	if log_enabled {
		log::info!("Normalized master locations:\n{normalized_master_locs:?}");
	}

	// Find base master.
	let mut base_idx = None;
	for (i, m) in normalized_master_locs.iter().enumerate() {
		if m.iter().all(|(_, v)| *v == 0.0) {
			if base_idx.is_some() {
				return Err(validation(
					"More than one base master found in Designspace.",
				));
			}
			base_idx = Some(i);
		}
	}
	if require_sources && base_idx.is_none() {
		return Err(validation(
			"Base master not found; no master at default location?",
		));
	}
	if log_enabled {
		log::info!("Index of base master: {base_idx:?}");
	}

	Ok(DesignSpaceData {
		axes,
		axis_mappings,
		internal_axis_supports,
		base_idx,
		normalized_master_locs,
		masters: ds.sources.clone(),
		instances: ds.instances.clone(),
		rules: ds.rules.clone(),
		rules_processing_last: ds.rules_processing_last,
		lib: ds.lib.clone(),
	})
}

/// `WDTH_VALUE_TO_OS2_WIDTH_CLASS`.
const WDTH_VALUE_TO_OS2_WIDTH_CLASS: [(f64, f64); 9] = [
	(50.0, 1.0),
	(62.5, 2.0),
	(75.0, 3.0),
	(87.5, 4.0),
	(100.0, 5.0),
	(112.5, 6.0),
	(125.0, 7.0),
	(150.0, 8.0),
	(200.0, 9.0),
];

/// `set_default_weight_width_slant`: sets `OS/2.usWeightClass`,
/// `OS/2.usWidthClass` and `post.italicAngle` from the default location.
// `max(a, min(v, b))` is not `clamp` with NaN.
#[allow(clippy::manual_clamp)]
pub fn set_default_weight_width_slant(font: &mut Font, location: &[(String, f64)]) -> Result<()> {
	let get = |t: &str| location.iter().find(|(k, _)| k == t).map(|e| e.1);
	if font.contains(Tag::new(b"OS/2")) {
		if let Some(wght) = get("wght") {
			let weight_class = ot_round(wght.min(1000.0).max(1.0));
			let os2 = font.require::<Os2>()?;
			if i64::from(os2.base.us_weight_class) != weight_class {
				log::info!("Setting OS/2.usWeightClass = {weight_class}");
				os2.base.us_weight_class =
					fontoxide_ttlib::binary::fit(weight_class, "usWeightClass")?;
			}
		}
		if let Some(wdth) = get("wdth") {
			// Map 'wdth' axis (50..200) to OS/2.usWidthClass (1..9), rounding to
			// closest.
			let width_value = wdth.max(50.0).min(200.0);
			let width_class = ot_round(piecewise_linear_map(
				width_value,
				&WDTH_VALUE_TO_OS2_WIDTH_CLASS,
			));
			let os2 = font.require::<Os2>()?;
			if i64::from(os2.base.us_width_class) != width_class {
				log::info!("Setting OS/2.usWidthClass = {width_class}");
				os2.base.us_width_class =
					fontoxide_ttlib::binary::fit(width_class, "usWidthClass")?;
			}
		}
	}
	if let Some(slnt) = get("slnt")
		&& font.contains(Tag::new(b"post"))
	{
		let italic_angle = slnt.min(90.0).max(-90.0);
		let post = font.require::<Post>()?;
		if post.header.italic_angle != italic_angle {
			log::info!("Setting post.italicAngle = {}", float_repr(italic_angle));
			post.header.italic_angle = italic_angle;
		}
	}
	Ok(())
}

/// `drop_implied_oncurve_points`: drops the on-curve points of the simple glyphs
/// of the masters that are halfway between two off-curve points in all of them.
/// Returns the number of points dropped.
pub fn drop_implied_oncurve_points(masters: &mut [&mut Font]) -> Result<usize> {
	use fontoxide_ttlib::tables::glyf::drop_implied_on_curve_points;
	// The glyphs of the masters by name.
	let mut names: IndexMap<String, Vec<(usize, u32)>> = IndexMap::new();
	for (i, font) in masters.iter_mut().enumerate() {
		let glyf_tag = tt_tag(font, "glyf");
		let order = font.glyph_order()?;
		let glyf = font
			.get_at::<Glyf>(glyf_tag)?
			.ok_or_else(|| key_error(&glyf_tag.to_string()))?;
		for gid in glyf.glyphs.keys() {
			names
				.entry(order.name(*gid).into_owned())
				.or_default()
				.push((i, *gid));
		}
	}
	let mut glyfs: Vec<Glyf> = Vec::with_capacity(masters.len());
	for font in masters.iter_mut() {
		let glyf_tag = tt_tag(font, "glyf");
		glyfs.push(
			font.get_at::<Glyf>(glyf_tag)?
				.ok_or_else(|| key_error(&glyf_tag.to_string()))?
				.clone(),
		);
	}
	let mut count = 0;
	for (name, glyphs) in &names {
		let mut taken: Vec<Glyph> = glyphs
			.iter()
			.map(|(i, g)| glyfs[*i].glyphs.get(g).cloned().unwrap_or_default())
			.collect();
		let mut refs: Vec<&mut Glyph> = taken.iter_mut().collect();
		match drop_implied_on_curve_points(&mut refs) {
			Ok(dropped) => {
				count += dropped.len();
				for ((i, g), glyph) in glyphs.iter().zip(taken) {
					glyfs[*i].glyphs.insert(*g, glyph);
				}
			}
			Err(e) => {
				// We don't fail for incompatible glyphs in _add_gvar so we shouldn't
				// here.
				log::warn!(
					"Failed to drop implied oncurves for {}: {e}",
					str_repr(name)
				);
			}
		}
	}
	for (font, glyf) in masters.iter_mut().zip(glyfs) {
		let glyf_tag = tt_tag(font, "glyf");
		*font
			.get_at::<Glyf>(glyf_tag)?
			.ok_or_else(|| key_error(&glyf_tag.to_string()))? = glyf;
	}
	Ok(count)
}

/// The options of [`build`] and [`build_many`].
#[derive(Debug, Clone)]
pub struct BuildOptions {
	/// The tables not to build (`exclude`).
	pub exclude: Vec<String>,
	/// Whether to optimize `gvar` with IUP (`optimize`).
	pub optimize: bool,
	/// Whether to reuse layers when rebuilding a variable `COLR`
	/// (`colr_layer_reuse`).
	pub colr_layer_reuse: bool,
	/// Whether to drop the on-curve points that can be implied
	/// (`drop_implied_oncurves`).
	pub drop_implied_oncurves: bool,
}

impl Default for BuildOptions {
	fn default() -> Self {
		BuildOptions {
			exclude: Vec::new(),
			optimize: true,
			colr_layer_reuse: true,
			drop_implied_oncurves: false,
		}
	}
}

/// Maps a master path of the designspace to the path of a font binary or TTX
/// (`master_finder`).
pub type MasterFinder<'a> = &'a dyn Fn(&str) -> String;

/// The identity [`MasterFinder`] (`lambda s: s`).
pub fn identity_finder(s: &str) -> String {
	s.to_owned()
}

/// The fonts of the sources of a designspace, which fontTools keeps in
/// `SourceDescriptor.font`: one for each source, `None` until loaded.
pub type SourceFonts = Vec<Option<Font>>;

/// `build_many`: builds the variable fonts of a designspace, version 5 which can
/// define several, or version 4 which has implicitly one covering the whole
/// document. `skip_vf` skips the variable fonts by name. Returns the fonts by
/// variable font name.
///
/// `fonts` are the source fonts already loaded, by index of `designspace.sources`
/// (they may be fewer); the sources of the sub-documents are matched to them by
/// their file name, path, name and layer name.
pub fn build_many(
	designspace: &mut DesignSpaceDocument,
	fonts: &mut SourceFonts,
	master_finder: MasterFinder<'_>,
	options: &BuildOptions,
	skip_vf: &dyn Fn(&str) -> bool,
) -> Result<IndexMap<String, Font>> {
	let mut res = IndexMap::new();
	// varLib.build by default only builds an incomplete 'STAT' with an empty
	// AxisValueArray--unless the VF inherited 'STAT' from its base master.
	// Designspace version 5 can also be used to define 'STAT' labels or customize
	// axes ordering, etc. To avoid overwriting a pre-existing 'STAT' or redoing the
	// same work twice, here we check if designspace contains any 'STAT' info before
	// proceeding to call buildVFStatTable for each VF.
	let do_build_stat_from_ds_v5 = !options.exclude.iter().any(|t| t == "STAT")
		&& designspace.format_tuple()? >= (5, 0)
		&& (designspace
			.axes
			.iter()
			.any(|a| !a.axis_labels().is_empty() || a.axis_ordering().is_some())
			|| !designspace.location_labels.is_empty());
	fonts.resize_with(designspace.sources.len().max(fonts.len()), || None);
	for (_location, sub_doc) in split_interpolable(designspace, SplitOptions::interpolable())? {
		for (name, mut vf_doc) in split_variable_fonts(&sub_doc, SplitOptions::variable_fonts())? {
			if skip_vf(&name) {
				log::debug!("Skipping variable TTF font: {name}");
				continue;
			}
			// LENIENT(L36): the descriptors carry no font object, so the sources of the
			// sub-document are matched to the given fonts by file name, path, name and
			// layer name.
			let mut used = vec![false; designspace.sources.len()];
			let indices: Vec<Option<usize>> = vf_doc
				.sources
				.iter()
				.map(|s| {
					let found = designspace.sources.iter().enumerate().position(|(i, o)| {
						!used[i]
							&& o.filename == s.filename
							&& o.path == s.path && o.name == s.name
							&& o.layer_name == s.layer_name
					});
					if let Some(i) = found {
						used[i] = true;
					}
					found
				})
				.collect();
			let mut sub_fonts: SourceFonts = indices
				.iter()
				.map(|i| i.and_then(|i| fonts[i].clone()))
				.collect();
			let (vf, _, _) = build(&mut vf_doc, &mut sub_fonts, master_finder, options)?;
			for (i, f) in indices.iter().zip(sub_fonts) {
				if let Some(i) = i
					&& fonts[*i].is_none()
				{
					fonts[*i] = f;
				}
			}
			let mut vf = vf;
			if do_build_stat_from_ds_v5 {
				crate::stat::build_vf_stat_table(&mut vf, designspace, &name)?;
			}
			res.insert(name, vf);
		}
	}
	Ok(res)
}

/// `build`: builds a variable font from a designspace document and its masters.
/// `fonts` are the source fonts by index of the sources (`SourceDescriptor.font`),
/// loaded with `master_finder` where missing. Returns the variable font, the
/// variation model and the paths of the masters.
pub fn build(
	designspace: &mut DesignSpaceDocument,
	fonts: &mut SourceFonts,
	master_finder: MasterFinder<'_>,
	options: &BuildOptions,
) -> Result<(Font, VariationModel, Vec<Option<String>>)> {
	let ds = load_designspace(designspace, true, true)?;
	log::info!("Building variable font");

	log::info!("Loading master fonts");
	let paths = load_masters(designspace, fonts, master_finder)?;
	let mut master_fonts: Vec<Font> = fonts
		.iter()
		.take(designspace.sources.len())
		.map(|f| {
			f.clone()
				.ok_or_else(|| validation("master font not loaded"))
		})
		.collect::<Result<_>>()?;
	check_matching_tt_table_families(&master_fonts)?;

	// TODO: 'master_ttfs' is unused except for return value, remove later.
	let master_ttfs = paths;

	let base_idx = ds
		.base_idx
		.ok_or_else(|| validation("Base master not found"))?;
	let glyf_tag = tt_tag(&master_fonts[base_idx], "glyf");
	if options.drop_implied_oncurves && master_fonts[base_idx].contains(glyf_tag) {
		let mut refs: Vec<&mut Font> = master_fonts.iter_mut().collect();
		let drop_count = drop_implied_oncurve_points(&mut refs)?;
		log::info!(
			"Dropped {drop_count} on-curve points from simple glyphs in the '{glyf_tag}' table"
		);
	}

	// Copy the base master to work from it.
	let mut vf = master_fonts[base_idx].clone();
	vf.remove(Tag::new(b"DSIG"));

	// Clear USE_MY_METRICS composite flags if set inconsistently across masters.
	if vf.contains(glyf_tag) {
		unset_inconsistent_use_my_metrics_flags(&mut vf, &mut master_fonts)?;
	}

	let excluded = |t: &str| options.exclude.iter().any(|e| e == t);

	// TODO append masters as named-instances as well; needs .designspace change.
	add_fvar(&mut vf, &ds.axes, &ds.instances)?;
	if !excluded("STAT") {
		add_stat(&mut vf)?;
	}

	// Map from axis names to axis tags...
	let normalized_master_locs: Vec<DictLocation> = ds
		.normalized_master_locs
		.iter()
		.map(|loc| {
			loc.iter()
				.map(|(k, v)| (axis_tag(&ds.axes[k]), *v))
				.collect()
		})
		.collect();
	// From here on, we use fvar axes only.
	let axis_tags: Vec<String> = vf
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| Tag::new(&a.axis_tag).to_string())
		.collect();

	// Assume single-model for now.
	let mut model = VariationModel::new(&normalized_master_locs, Some(&axis_tags), false, None)
		.map_err(model_error)?;
	if model.mapping[base_idx] != 0 {
		return Err(assertion("0 == model.mapping[ds.base_idx]"));
	}

	let infos = master_infos(&mut master_fonts, &master_ttfs)?;

	log::info!("Building variations tables");
	if !excluded("avar") {
		add_avar(&mut vf, &ds.axes, &ds.axis_mappings, &axis_tags)?;
	}
	if !excluded("BASE") && vf.contains(Tag::new(b"BASE")) {
		add_base(&mut vf, &model, &mut master_fonts, &axis_tags, &infos)?;
	}
	if !excluded("MVAR") {
		add_mvar(&mut vf, &model, &mut master_fonts, &axis_tags)?;
	}
	if !excluded("HVAR") {
		add_hvar(&mut vf, &model, &mut master_fonts, &axis_tags)?;
	}
	if !excluded("VVAR") && vf.contains(tt_tag(&vf, "vmtx")) {
		add_vvar(&mut vf, &model, &mut master_fonts, &axis_tags)?;
	}
	if !excluded("GDEF") || !excluded("GPOS") {
		merge_otl(&mut vf, &model, &mut master_fonts, &axis_tags, &infos)?;
	}
	if !excluded("gvar") && !excluded("GVAR") && vf.contains(glyf_tag) {
		add_gvar(&mut vf, &model, &mut master_fonts, 0.5, options.optimize)?;
	}
	if !excluded("cvar") && vf.contains(glyf_tag) {
		merge_tt_hinting(&mut vf, &model, &mut master_fonts)?;
	}
	if !excluded("GSUB") && !ds.rules.is_empty() {
		let feature_tags = feature_variations_tags(&ds)?;
		add_gsub_feature_variations(
			&mut vf,
			&ds.axes,
			&ds.internal_axis_supports,
			&ds.rules,
			&feature_tags,
		)?;
	}
	if !excluded("CFF2") && (vf.contains(Tag::new(b"CFF ")) || vf.contains(Tag::new(b"CFF2"))) {
		crate::cff::add_cff2(&mut vf, &mut model, &mut master_fonts)?;
		if vf.contains(Tag::new(b"post")) {
			// Set 'post' to format 2 to keep the glyph names dropped from CFF2.
			let post = vf.require::<Post>()?;
			if post.header.format_type != 2.0 {
				post.header.format_type = 2.0;
				post.extra_names = Some(Vec::new());
				post.mapping = Some(BTreeMap::new());
			}
		}
	}
	if !excluded("COLR")
		&& let Some(colr) = vf.get::<fontoxide_ttlib::tables::Colr>()?
		&& colr.version > 0
	{
		add_colr(
			&mut vf,
			&model,
			&mut master_fonts,
			&axis_tags,
			options.colr_layer_reuse,
			&infos,
		)?;
	}

	let defaults: Vec<(String, f64)> = vf
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| (Tag::new(&a.axis_tag).to_string(), a.default_value))
		.collect();
	set_default_weight_width_slant(&mut vf, &defaults)?;

	for t in &options.exclude {
		if let Some(t) = Tag::from_str_padded(t) {
			vf.remove(t);
		}
	}

	// TODO: Only return vf for 4.0+, the rest is unused.
	Ok((vf, model, master_ttfs))
}

/// `_add_COLR`.
pub fn add_colr(
	font: &mut Font,
	model: &VariationModel,
	masters: &mut [Font],
	axis_tags: &[String],
	colr_layer_reuse: bool,
	infos: &[MasterInfo],
) -> Result<()> {
	let mut merger = Merger::colr_variation(model, axis_tags, font, infos, colr_layer_reuse)?;
	{
		let mut refs: Vec<&mut Font> = masters.iter_mut().collect();
		merger.merge_colr(font, &mut refs)?;
	}
	let var_idxes = merger.colr.var_idxes.clone();
	let store = std::mem::replace(
		&mut merger.store_builder,
		OnlineVarStoreBuilder::new(Vec::new())?,
	)
	.finish(true);
	let colr = font
		.get::<fontoxide_ttlib::tables::Colr>()?
		.ok_or_else(|| key_error("COLR"))?;
	let table = colr.table.as_mut().ok_or_else(|| key_error("COLR"))?;
	if !store.var_data.is_empty() {
		let mut store = store;
		let mapping = var_store::optimize(&mut store, true)?;
		table.var_store = Some(Box::new(store));
		let mapped: Vec<u64> = var_idxes
			.iter()
			.map(|v| {
				mapping
					.get(&u64::from(*v))
					.copied()
					.ok_or_else(|| key_error(&v.to_string()))
			})
			.collect::<Result<_>>()?;
		table.var_index_map = build_delta_set_index_map(mapped).map(Box::new);
	}
	Ok(())
}

/// `guessFileType` of `fontTools.ttx`.
fn guess_file_type(path: &str) -> Option<&'static str> {
	let data = std::fs::read(path).ok()?;
	let mut header: &[u8] = &data[..data.len().min(256)];
	if let Some(rest) = header.strip_prefix(b"\xef\xbb\xbf")
		&& rest.starts_with(b"<?xml")
	{
		header = rest;
	}
	if path.to_ascii_lowercase().ends_with(".dfont") {
		return Some("TTF");
	}
	match header.get(..4) {
		Some(b"OTTO") => Some("OTF"),
		Some(b"ttcf") => Some("TTC"),
		Some(b"\0\x01\0\0" | b"true") => Some("TTF"),
		Some(b"wOFF") => Some("WOFF"),
		Some(b"wOF2") => Some("WOFF2"),
		Some(b"<?xm") => {
			let text: String = header.iter().map(|&b| b as char).collect();
			if text.contains("sfntVersion=\"OTTO\"") || text.contains("sfntVersion='OTTO'") {
				Some("OTX")
			} else {
				Some("TTX")
			}
		}
		_ => None,
	}
}

/// `os.path.normpath`, for the separators of the platform.
fn normpath(path: &str) -> String {
	let sep = std::path::MAIN_SEPARATOR;
	let unified: String = path
		.chars()
		.map(|c| if c == '/' || c == '\\' { sep } else { c })
		.collect();
	let absolute = unified.starts_with(sep);
	let mut parts: Vec<&str> = Vec::new();
	let mut prefix = String::new();
	let mut rest = unified.as_str();
	if cfg!(windows) && rest.len() >= 2 && rest.as_bytes()[1] == b':' {
		prefix = rest[..2].to_owned();
		rest = &rest[2..];
	}
	let rooted = rest.starts_with(sep);
	for part in rest.split(sep) {
		match part {
			"" | "." => {}
			".." => {
				if parts.last().is_some_and(|p| *p != "..") {
					parts.pop();
				} else if !rooted {
					parts.push("..");
				}
			}
			p => parts.push(p),
		}
	}
	let joined = parts.join(&sep.to_string());
	let _ = absolute;
	let out = format!(
		"{prefix}{}{joined}",
		if rooted {
			sep.to_string()
		} else {
			String::new()
		}
	);
	if out.is_empty() { ".".to_owned() } else { out }
}

/// `_open_font`: loads a master from a TTX or OpenType binary path, or else from
/// the path `master_finder` gives.
pub fn open_font(path: &str, master_finder: MasterFinder<'_>) -> Result<Font> {
	let mut master_path = normpath(path);
	let mut tp = guess_file_type(&master_path);
	if tp.is_none() {
		// Not an OpenType binary/ttx, fall back to the master finder.
		master_path = master_finder(&master_path);
		tp = guess_file_type(&master_path);
	}
	match tp {
		Some("TTX" | "OTX") => {
			let mut font = Font::new();
			font.import_ttx(&std::fs::read_to_string(&master_path)?)?;
			Ok(font)
		}
		Some("TTF" | "OTF" | "WOFF" | "WOFF2") => Ok(Font::read(
			std::fs::read(&master_path)?,
			ReadOptions::default(),
		)?),
		_ => Err(validation(format!(
			"Invalid master path: {}",
			str_repr(&master_path)
		))),
	}
}

/// `load_masters`: makes sure every source has a font, loading those that
/// don't from their path. Returns the paths of the fonts (`None` for the fonts
/// that were given).
///
/// The paths can point to either an OpenType font, a TTX file, or a UFO. In the
/// latter case, use the provided `master_finder` to map from UFO paths to the
/// respective master font binaries (e.g. .ttf, .otf or .ttx).
pub fn load_masters(
	designspace: &DesignSpaceDocument,
	fonts: &mut SourceFonts,
	master_finder: MasterFinder<'_>,
) -> Result<Vec<Option<String>>> {
	fonts.resize_with(designspace.sources.len().max(fonts.len()), || None);
	for (master, font) in designspace.sources.iter().zip(fonts.iter()) {
		// If a SourceDescriptor has a layer name, demand that the compiled TTFont be
		// supplied by the caller. This spares us from modifying MasterFinder.
		if master.layer_name.is_some() && font.is_none() {
			return Err(validation(format!(
				"Designspace source '{}' specified a layer name but lacks the required TTFont object in the 'font' attribute.",
				master
					.name
					.as_deref()
					.filter(|n| !n.is_empty())
					.unwrap_or("<Unknown>")
			)));
		}
	}
	// `designspace.loadSourceFonts(_open_font, master_finder=master_finder)`: fonts
	// with the same path are loaded once.
	let mut loaded: HashMap<String, Font> = HashMap::new();
	let mut paths = Vec::with_capacity(designspace.sources.len());
	for (source, font) in designspace.sources.iter().zip(fonts.iter_mut()) {
		if font.is_some() {
			paths.push(None);
			continue;
		}
		let Some(path) = &source.path else {
			return Err(
				fontoxide_designspacelib::DesignSpaceDocumentError::Document(format!(
					"Designspace source '{}' has no 'path' attribute",
					source.name.as_deref().unwrap_or("<Unknown>")
				))
				.into(),
			);
		};
		let f = match loaded.get(path) {
			Some(f) => f.clone(),
			None => {
				let f = open_font(path, master_finder)?;
				loaded.insert(path.clone(), f.clone());
				f
			}
		};
		*font = Some(f);
		paths.push(Some(path.clone()));
	}
	Ok(paths)
}

/// `MasterFinder(template)`: fills `{fullname}`, `{dirname}`, `{basename}`,
/// `{stem}` and `{ext}` of the template from a source path.
pub fn master_finder_template(template: &str, src_path: &str) -> Result<String> {
	let fullname = fontoxide_designspacelib::paths::abspath_str(src_path)?;
	let (dirname, basename) = match fullname.rfind(['/', '\\']) {
		Some(i) => (fullname[..i].to_owned(), fullname[i + 1..].to_owned()),
		None => (String::new(), fullname.clone()),
	};
	let (stem, ext) = match basename.rfind('.') {
		Some(i) if i > 0 => (basename[..i].to_owned(), basename[i..].to_owned()),
		_ => (basename.clone(), String::new()),
	};
	let path = template
		.replace("{fullname}", &fullname)
		.replace("{dirname}", &dirname)
		.replace("{basename}", &basename)
		.replace("{stem}", &stem)
		.replace("{ext}", &ext);
	Ok(normpath(&path))
}

/// `_feature_variations_tags`.
fn feature_variations_tags(ds: &DesignSpaceData) -> Result<Vec<Tag>> {
	let raw = match ds.lib.get(FEAVAR_FEATURETAG_LIB_KEY) {
		Some(v) => v
			.as_str()
			.ok_or_else(|| {
				fontoxide_ttlib::Error::ttlib("AttributeError: object has no attribute 'split'")
			})?
			.to_owned(),
		None => if ds.rules_processing_last {
			"rclt"
		} else {
			"rvrn"
		}
		.to_owned(),
	};
	let mut tags: Vec<String> = raw
		.split(',')
		.map(|t| fontoxide_misc::py::strip(t).to_owned())
		.collect();
	tags.sort();
	tags.dedup();
	tags.iter()
		.map(|t| {
			Tag::from_str_padded(t)
				.ok_or_else(|| fontoxide_ttlib::Error::value(format!("invalid tag: {t:?}")).into())
		})
		.collect()
}

/// `addGSUBFeatureVariations`: adds a `GSUB` FeatureVariations table to a variable
/// font, from the rules of a designspace. Without `feature_tags`, they come from
/// the `com.github.fonttools.varLib.featureVarsFeatureTag` key of the lib, or are
/// `rclt` with `<rules processing="last">`, else `rvrn`.
pub fn add_gsub_feature_variations_from_designspace(
	vf: &mut Font,
	designspace: &mut DesignSpaceDocument,
	feature_tags: &[Tag],
	log_enabled: bool,
) -> Result<()> {
	let ds = load_designspace(designspace, log_enabled, true)?;
	if ds.rules.is_empty() {
		return Ok(());
	}
	let tags = if feature_tags.is_empty() {
		feature_variations_tags(&ds)?
	} else {
		feature_tags.to_vec()
	};
	add_gsub_feature_variations(vf, &ds.axes, &ds.internal_axis_supports, &ds.rules, &tags)
}

/// LENIENT(L36): there is no regular expression engine; `--variable-fonts` supports
/// these patterns only.
///
/// `re.fullmatch(pattern, text)` for the simple patterns of `--variable-fonts`:
/// literals, `.`, `\` escapes and the `*`, `+` and `?` quantifiers.
fn fullmatch(pattern: &str, text: &str) -> bool {
	#[derive(Debug, Clone, Copy)]
	enum Atom {
		Any,
		Char(char),
	}
	let mut items: Vec<(Atom, char)> = Vec::new();
	let mut chars = pattern.chars();
	while let Some(c) = chars.next() {
		let atom = match c {
			'.' => Atom::Any,
			'\\' => Atom::Char(chars.next().unwrap_or('\\')),
			'*' | '+' | '?' => {
				if let Some(last) = items.last_mut()
					&& last.1 == '1'
				{
					last.1 = c;
					continue;
				}
				Atom::Char(c)
			}
			c => Atom::Char(c),
		};
		items.push((atom, '1'));
	}
	fn matches(atom: Atom, c: char) -> bool {
		match atom {
			Atom::Any => c != '\n',
			Atom::Char(a) => a == c,
		}
	}
	fn go(items: &[(Atom, char)], text: &[char]) -> bool {
		let Some(&(atom, q)) = items.first() else {
			return text.is_empty();
		};
		match q {
			'1' => !text.is_empty() && matches(atom, text[0]) && go(&items[1..], &text[1..]),
			'?' => {
				go(&items[1..], text)
					|| (!text.is_empty() && matches(atom, text[0]) && go(&items[1..], &text[1..]))
			}
			_ => {
				let min = usize::from(q == '+');
				let mut n = 0;
				while n < text.len() && matches(atom, text[n]) {
					n += 1;
				}
				(min..=n).rev().any(|k| go(&items[1..], &text[k..]))
			}
		}
	}
	let text: Vec<char> = text.chars().collect();
	go(&items, &text)
}

/// `os.path.basename`.
fn basename(p: &str) -> &str {
	match p.rfind(['/', '\\']) {
		Some(i) => &p[i + 1..],
		None => p,
	}
}

/// `os.path.dirname`.
fn dirname(p: &str) -> &str {
	match p.rfind(['/', '\\']) {
		Some(i) => &p[..i],
		None => "",
	}
}

/// The usage of [`main`].
pub fn help() -> &'static str {
	"usage: varLib [-h] [-o OUTPUTFILE | -d OUTPUTDIR] [-x TAG] [--disable-iup]
              [--no-colr-layer-reuse] [--drop-implied-oncurves]
              [--master-finder MASTER_FINDER] [--variable-fonts VF_NAME]
              [-v | -q]
              designspace

Build variable fonts from a designspace file and masters
"
}

fn usage(message: impl Into<String>) -> VarLibError {
	VarLibError::Usage(format!("varLib: error: {}", message.into()))
}

/// `main`: builds the variable fonts of a designspace file from its masters.
pub fn main(args: &[&str]) -> Result<i32> {
	let mut designspace_filename: Option<&str> = None;
	let mut outfile: Option<String> = None;
	let mut output_dir: Option<String> = None;
	let mut options = BuildOptions::default();
	let mut master_finder = "master_ttf_interpolatable/{stem}.ttf".to_owned();
	let mut variable_fonts = ".*".to_owned();
	let mut it = args.iter();
	while let Some(&arg) = it.next() {
		let mut value = |name: &str| -> Result<String> {
			it.next()
				.map(|s| (*s).to_owned())
				.ok_or_else(|| usage(format!("argument {name}: expected one argument")))
		};
		match arg {
			"-h" | "--help" => {
				print!("{}", help());
				return Ok(0);
			}
			"-o" => outfile = Some(value("-o")?),
			"-d" | "--output-dir" => output_dir = Some(value("-d/--output-dir")?),
			"-x" => options.exclude.push(value("-x")?),
			"--disable-iup" => options.optimize = false,
			"--no-colr-layer-reuse" => options.colr_layer_reuse = false,
			"--drop-implied-oncurves" => options.drop_implied_oncurves = true,
			"--master-finder" => master_finder = value("--master-finder")?,
			"--variable-fonts" => variable_fonts = value("--variable-fonts")?,
			"-v" | "--verbose" | "-q" | "--quiet" => {}
			a if a.starts_with('-') && a.len() > 1 => {
				return Err(usage(format!("unrecognized arguments: {a}")));
			}
			a => {
				if designspace_filename.is_some() {
					return Err(usage(format!("unrecognized arguments: {a}")));
				}
				designspace_filename = Some(a);
			}
		}
	}
	if outfile.is_some() && output_dir.is_some() {
		return Err(usage(
			"argument -d/--output-dir: not allowed with argument -o",
		));
	}
	let designspace_filename = designspace_filename
		.ok_or_else(|| usage("the following arguments are required: designspace"))?;
	let mut designspace = DesignSpaceDocument::from_file(designspace_filename)?;

	let vf_descriptors = designspace.get_variable_fonts();
	if vf_descriptors.is_empty() {
		return Err(usage(format!(
			"No variable fonts in given designspace {}",
			designspace
				.path
				.as_deref()
				.map_or_else(|| "None".to_owned(), str_repr)
		)));
	}
	let vfs_to_build: Vec<_> = vf_descriptors
		.into_iter()
		// Skip variable fonts that do not match the user's inclusion regex if given.
		.filter(|vf| fullmatch(&variable_fonts, &vf.name))
		.collect();
	if vfs_to_build.is_empty() {
		return Err(usage(format!(
			"No variable fonts matching {}",
			str_repr(&variable_fonts)
		)));
	}
	if outfile.is_some() && vfs_to_build.len() > 1 {
		return Err(usage(
			"can't specify -o because there are multiple VFs to build; use --output-dir, or select a single VF with --variable-fonts",
		));
	}

	let output_dir = output_dir.unwrap_or_else(|| dirname(designspace_filename).to_owned());
	let mut output_paths: HashMap<String, String> = HashMap::new();
	match &outfile {
		Some(o) if vfs_to_build.len() == 1 => {
			output_paths.insert(vfs_to_build[0].name.clone(), o.clone());
		}
		_ => {
			for vf in &vfs_to_build {
				let filename = match &vf.filename {
					// Only use basename to prevent path traversal attacks.
					Some(f) => basename(f).to_owned(),
					// vf.name comes straight from the designspace and is equally
					// attacker-controlled, so basename it too.
					None => format!("{}.{{ext}}", basename(&vf.name)),
				};
				let path = if output_dir.is_empty() {
					filename
				} else {
					std::path::Path::new(&output_dir)
						.join(filename)
						.to_string_lossy()
						.into_owned()
				};
				output_paths.insert(vf.name.clone(), path);
			}
		}
	}

	let names: Vec<String> = vfs_to_build.iter().map(|vf| vf.name.clone()).collect();
	let finder =
		|s: &str| master_finder_template(&master_finder, s).unwrap_or_else(|_| s.to_owned());
	let vfs = build_many(
		&mut designspace,
		&mut SourceFonts::new(),
		&finder,
		&options,
		&|name| !names.iter().any(|n| n == name),
	)?;

	for (vf_name, mut vf) in vfs {
		let ext = if vf.sfnt_version == Tag::new(b"OTTO") {
			"otf"
		} else {
			"ttf"
		};
		let output_path = output_paths
			.get(&vf_name)
			.ok_or_else(|| key_error(&vf_name))?
			.replace("{ext}", ext);
		let dir = dirname(&output_path);
		if !dir.is_empty() {
			std::fs::create_dir_all(dir)?;
		}
		log::info!("Saving variation font {output_path}");
		let data = vf.write(&fontoxide_ttlib::WriteOptions::default())?;
		std::fs::write(&output_path, data)?;
	}
	Ok(0)
}

#[cfg(test)]
mod tests {
	use super::fullmatch;

	#[test]
	fn simple_fullmatch() {
		assert!(fullmatch(".*", "BuildMain-VF"));
		assert!(fullmatch("Build.*", "BuildMain-VF"));
		assert!(!fullmatch("FooBar", "BuildMain-VF"));
		assert!(fullmatch("a+b?", "aa"));
	}
}
