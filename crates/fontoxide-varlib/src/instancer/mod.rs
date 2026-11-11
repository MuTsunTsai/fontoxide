//! Port of `fontTools.varLib.instancer`: partially instantiates a variable font.
//!
//! [`instantiate_variable_font`] makes full instances (static fonts) of variable
//! fonts, and "partial" variable fonts with only a part of the original variation
//! space.

#![allow(clippy::type_complexity)]

mod avar;
mod cff;
pub mod cli;
mod colr;
mod feature_vars;
mod item_store;
mod limits;
pub(crate) mod metrics;
mod names;
pub(crate) mod otl;
mod solver;
pub(crate) mod tuple;
mod varc;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use fontoxide_misc::round_tools::ot_round;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::remove_overlaps::{RemoveOverlapsOptions, remove_overlaps};
use fontoxide_ttlib::tables::metrics::Metric;
use fontoxide_ttlib::tables::tuple_variation::Delta;
use fontoxide_ttlib::tables::{
	Avar, Cff2, Cvar, Cvt, Fvar, Glyf, Gvar, Hmtx, Name, Os2, Post, Stat, Vmtx,
};
use fontoxide_ttlib::{Error, Font, ReadOptions, Result, Tag, WriteOptions};

pub use self::limits::{
	AxisLimits, AxisTriple, Limit, NormalizedAxisTriple, NormalizedLimits, UserLimits,
};
pub use self::solver::{Tent, rebase_tent};

// The internals that the tests ported from fontTools call directly.
#[doc(hidden)]
pub use self::avar::{
	ReachableRanges, cull_variations, instantiate_avar, is_dead, valid_avar_segment_map,
};
#[doc(hidden)]
pub use self::cff::instantiate_cff2;
#[doc(hidden)]
pub use self::feature_vars::{
	instantiate_condition_values, instantiate_feature_variations,
	limit_feature_variation_condition_range,
};
#[doc(hidden)]
pub use self::item_store::{DefaultDeltas, TupleVarStore, instantiate_item_variation_store};
#[doc(hidden)]
pub use self::limits::renormalize_location;
#[doc(hidden)]
pub use self::metrics::{instantiate_hvar, instantiate_mvar, instantiate_vvar};
#[doc(hidden)]
pub use self::otl::{instantiate_otl, mutator_merge_gpos};
#[doc(hidden)]
pub use self::tuple::change_axis_limit;

use self::tuple::instantiate_tuple_variation_store;
use crate::models::piecewise_linear_map;

const AVAR: Tag = Tag::new(b"avar");
const CFF2: Tag = Tag::new(b"CFF2");
const VARC: Tag = Tag::new(b"VARC");
const DSIG: Tag = Tag::new(b"DSIG");
const FVAR: Tag = Tag::new(b"fvar");
const CVAR: Tag = Tag::new(b"cvar");
const LTAG: Tag = Tag::new(b"ltag");
const NAME: Tag = Tag::new(b"name");
const OS2: Tag = Tag::new(b"OS/2");
const POST: Tag = Tag::new(b"post");
const STAT: Tag = Tag::new(b"STAT");

/// The internal functions that fontTools' tests call directly.
#[doc(hidden)]
pub mod test_support {
	use std::collections::{BTreeMap, BTreeSet};

	use fontoxide_ttlib::{Font, GlyphSet, Result, Tag};

	use super::limits::normalize_font_location;
	use super::{AxisLimits, NormalizedLimits, UserLimits};

	pub use super::names::ELIDABLE_AXIS_VALUE_NAME;

	/// `names.updateNameTable`.
	pub fn update_name_table(font: &mut Font, limits: &AxisLimits) -> Result<()> {
		let limits = UserLimits::from_limits(limits, font)?;
		super::names::update_name_table(font, &limits)
	}

	/// `names.getVariationNameIDs`.
	pub fn variation_name_ids(font: &mut Font) -> Result<BTreeSet<u16>> {
		super::variation_name_ids(font)
	}

	/// The end of `names.pruningUnusedNames`.
	pub fn prune_unused_names(font: &mut Font, original: &BTreeSet<u16>) -> Result<()> {
		super::prune_unused_names(font, original)
	}

	/// `instantiateCFF2(font, limits, round=(otRound if round else noRound),
	/// reachableRanges=reachable_ranges)`.
	pub fn instantiate_cff2(
		font: &mut Font,
		limits: &NormalizedLimits,
		round: bool,
		reachable_ranges: Option<&BTreeMap<String, (f64, f64)>>,
	) -> Result<()> {
		super::cff::instantiate_cff2(font, limits, round, false, reachable_ranges)?;
		Ok(())
	}

	/// `font.getGlyphSet(location=location)`.
	pub fn glyph_set_at<'f>(
		font: &'f mut Font,
		location: &BTreeMap<String, f64>,
	) -> Result<GlyphSet<'f>> {
		if location.is_empty() || !font.contains(Tag::new(b"fvar")) {
			return GlyphSet::new(font);
		}
		let location = normalize_font_location(font, location)?;
		GlyphSet::with_location(font, location)
	}
}

/// What to do with overlapping contours (`OverlapMode`). Only full instances are
/// concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlapMode {
	/// `KEEP_AND_DONT_SET_FLAGS`.
	KeepAndDontSetFlags,
	/// `KEEP_AND_SET_FLAGS`: some renderers on Apple platforms need the flags.
	#[default]
	KeepAndSetFlags,
	/// `REMOVE`.
	Remove,
	/// `REMOVE_AND_IGNORE_ERRORS`.
	RemoveAndIgnoreErrors,
}

/// The options of [`instantiate_variable_font`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstancerOptions {
	/// Perform IUP optimization on the remaining `gvar` deltas (`optimize`).
	pub optimize: bool,
	/// What to do with overlaps (`overlap`).
	pub overlap: OverlapMode,
	/// Pin the axes not in the limits to their defaults (`static`).
	pub make_static: bool,
	/// Update the name table and the style bits from the axis values of `STAT`
	/// (`updateFontNames`).
	pub update_font_names: bool,
	/// Downgrade `CFF2` to `CFF ` for a full instance (`downgradeCFF2`).
	pub downgrade_cff2: bool,
}

impl Default for InstancerOptions {
	fn default() -> Self {
		InstancerOptions {
			optimize: true,
			overlap: OverlapMode::default(),
			make_static: false,
			update_font_names: false,
			downgrade_cff2: false,
		}
	}
}

/// `instantiateGvar`, with `_instantiateGvarGlyph`.
#[doc(hidden)]
pub fn instantiate_gvar(font: &mut Font, limits: &NormalizedLimits, optimize: bool) -> Result<()> {
	log::info!("Instantiating glyf/gvar tables");
	// `gvarTag`, and what goes with it.
	let extended = font.require::<Gvar>()?.extended;
	if !font.contains(Font::locked_tag::<Glyf>(extended)) {
		return Err(Error::ttlib(format!(
			"KeyError: '{}'",
			Font::locked_tag::<Glyf>(extended)
		)));
	}
	let order = font.glyph_order()?;
	let mut h_metrics: BTreeMap<u32, Metric> =
		std::mem::take(&mut font.require_locked::<Hmtx>(extended)?.metrics);
	let mut v_metrics = match font.get_locked::<Vmtx>(extended)? {
		Some(vmtx) => Some(std::mem::take(&mut vmtx.metrics)),
		None => None,
	};
	let gvar = font.require::<Gvar>()?;
	let original: Vec<(u32, Vec<_>)> = std::mem::take(&mut gvar.variations);
	let keys: Vec<u32> = original.iter().map(|e| e.0).collect();
	let mut variations: HashMap<u32, Vec<_>> = original.into_iter().collect();

	let glyf = font.require::<Glyf>()?;
	// A composite glyph must come after its base glyphs, or its bounds would be
	// calculated before the deltas of those are applied.
	let mut cache = HashMap::new();
	let mut sorted = Vec::with_capacity(order.len());
	for gid in 0..order.len() as u32 {
		let depth = match glyf.glyphs.get(&gid) {
			Some(g) if g.is_composite() => {
				glyf.composite_maxp_values(gid, &order, 1, &mut cache, &mut vec![gid])?
					.2
			}
			_ => 0,
		};
		sorted.push((depth, order.name(gid).into_owned(), gid));
	}
	sorted.sort();

	for (_, _, gid) in sorted {
		let (mut coordinates, end_pts) =
			glyf.coordinates_and_controls(gid, &order, &h_metrics, v_metrics.as_ref())?;
		let ends: Vec<usize> = end_pts.iter().map(|&e| e as usize).collect();
		// Not every glyph may have variations.
		let store = variations.get_mut(&gid);
		let mut has_variations = false;
		if let Some(store) = store {
			if !store.is_empty() {
				let default = instantiate_tuple_variation_store(
					store,
					limits,
					Some((&coordinates, &ends)),
					true,
				)?;
				for (c, d) in coordinates.iter_mut().zip(&default) {
					if let Some(Delta::Point(x, y)) = d {
						c.0 += x.value();
						c.1 += y.value();
					}
				}
			}
			has_variations = !store.is_empty();
		}
		// This also sets the metrics from the phantom points and the bounding box,
		// even without deltas: the bounds of a composite glyph may have changed.
		glyf.set_coordinates(
			gid,
			&order,
			&coordinates,
			&mut h_metrics,
			v_metrics.as_mut(),
		)?;
		if !has_variations {
			variations.remove(&gid);
			continue;
		}
		if optimize {
			// IUP depends on point equality, so round before optimizing, as what
			// happens now must be the same as at render time. This must come after
			// calculating the metrics above.
			for c in &mut coordinates {
				c.0 = ot_round(c.0) as f64;
				c.1 = ot_round(c.1) as f64;
			}
			if let Some(store) = variations.get_mut(&gid) {
				for var in store {
					tuple::optimize(var, &coordinates, &ends)?;
				}
			}
		}
	}

	font.require::<Hmtx>()?.metrics = h_metrics;
	if let Some(metrics) = v_metrics {
		font.require::<Vmtx>()?.metrics = metrics;
	}
	let gvar = font.require::<Gvar>()?;
	gvar.variations = keys
		.into_iter()
		.filter_map(|g| variations.remove(&g).map(|v| (g, v)))
		.collect();
	if gvar.variations.is_empty() {
		font.remove(font.family_tag::<Gvar>());
	}
	Ok(())
}

/// `instantiateCvar`.
#[doc(hidden)]
pub fn instantiate_cvar(font: &mut Font, limits: &NormalizedLimits) -> Result<()> {
	log::info!("Instantiating cvt/cvar tables");
	let cvar = font.require::<Cvar>()?;
	let default = instantiate_tuple_variation_store(&mut cvar.variations, limits, None, true)?;
	let empty = cvar.variations.is_empty();
	if !default.is_empty() {
		// `setCvarDeltas`.
		let cvt = font.require::<Cvt>()?;
		for (i, delta) in default.iter().enumerate() {
			if let Some(Delta::Value(delta)) = delta
				&& delta.value() != 0.0
			{
				let value = cvt
					.values
					.get_mut(i)
					.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
				*value = fontoxide_ttlib::binary::fit(
					i64::from(*value) + ot_round(delta.value()),
					"cvt",
				)?;
			}
		}
	}
	if empty {
		font.remove(CVAR);
	}
	Ok(())
}

/// `isInstanceWithinAxisRanges`.
fn is_instance_within_axis_ranges(location: &BTreeMap<Tag, f64>, limits: &UserLimits) -> bool {
	location.iter().all(|(tag, coord)| {
		limits
			.0
			.get(&tag.to_string())
			.is_none_or(|l| !(*coord < l.minimum || *coord > l.maximum))
	})
}

/// `instantiateFvar`.
#[doc(hidden)]
pub fn instantiate_fvar(font: &mut Font, limits: &UserLimits) -> Result<()> {
	let location = limits.pinned_location();
	let fvar = font.require::<Fvar>()?;
	// Drop the table if all the axes are instantiated.
	if fvar
		.axes
		.iter()
		.all(|a| location.contains_key(&a.tag_text()))
	{
		log::info!("Dropping fvar table");
		font.remove(FVAR);
		return Ok(());
	}
	log::info!("Instantiating fvar table");
	fvar.axes.retain(|a| !location.contains_key(&a.tag_text()));
	for axis in &mut fvar.axes {
		if let Some(limit) = limits.0.get(&axis.tag_text()) {
			axis.min_value = limit.minimum;
			axis.default_value = limit.default;
			axis.max_value = limit.maximum;
		}
	}
	// Only keep the named instances whose coordinates are at the pinned location.
	let mut instances = Vec::with_capacity(fvar.instances.len());
	'instances: for mut instance in std::mem::take(&mut fvar.instances) {
		for (tag, value) in &location {
			let tag = Tag::from_str_padded(tag)
				.ok_or_else(|| Error::value(format!("invalid axis tag {tag:?}")))?;
			let coord = instance
				.coordinates
				.get(&tag)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {tag}")))?;
			if coord != value {
				continue 'instances;
			}
		}
		for tag in location.keys() {
			if let Some(tag) = Tag::from_str_padded(tag) {
				instance.coordinates.remove(&tag);
			}
		}
		if is_instance_within_axis_ranges(&instance.coordinates, limits) {
			instances.push(instance);
		}
	}
	fvar.instances = instances;
	Ok(())
}

/// The tags of the design axes of `STAT`.
fn design_axis_tags(stat: &Stat) -> Result<Vec<String>> {
	let axes = stat
		.design_axis_record
		.as_ref()
		.ok_or_else(|| Error::ttlib("AttributeError: 'NoneType' object has no attribute 'Axis'"))?;
	Ok(axes.axis.iter().map(|a| a.axis_tag.to_string()).collect())
}

/// Whether `axisValuesFromAxisLimits` keeps a value: only those whose axis is not
/// pinned nor restricted, or that are at the pinned value or in the new range, given
/// as (minimum, maximum).
fn axis_value_is_kept(
	value: &ot::AxisValue,
	tags: &[String],
	ranges: &BTreeMap<String, (f64, f64)>,
) -> Result<bool> {
	let outside = |index: u16, value: f64| -> Result<bool> {
		let tag = tags
			.get(usize::from(index))
			.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
		Ok(ranges.get(tag).is_some_and(|l| value < l.0 || value > l.1))
	};
	Ok(match value {
		ot::AxisValue::Format1(v) => !outside(v.axis_index, v.value)?,
		ot::AxisValue::Format2(v) => !outside(v.axis_index, v.nominal_value)?,
		ot::AxisValue::Format3(v) => !outside(v.axis_index, v.value)?,
		ot::AxisValue::Format4(v) => {
			// Drop a "non-analytic" value if any record doesn't match.
			let mut keep = true;
			for record in &v.axis_value_record {
				if outside(record.axis_index, record.value)? {
					keep = false;
					break;
				}
			}
			keep
		}
		ot::AxisValue::Unknown(format) => {
			log::warn!("Unknown AxisValue table format ({format}); ignored");
			true
		}
	})
}

/// `instantiateSTAT`.
#[doc(hidden)]
pub fn instantiate_stat(font: &mut Font, limits: &UserLimits) -> Result<()> {
	let stat = font.require::<Stat>()?;
	let has_values = stat
		.axis_value_array
		.as_ref()
		.is_some_and(|a| !a.axis_value.is_empty());
	if stat.design_axis_record.is_none() || !has_values {
		// The table is empty: nothing to do.
		return Ok(());
	}
	log::info!("Instantiating STAT table");
	let tags = design_axis_tags(stat)?;
	let ranges: BTreeMap<String, (f64, f64)> = limits
		.0
		.iter()
		.map(|(tag, l)| (tag.clone(), (l.minimum, l.maximum)))
		.collect();
	let Some(mut array) = stat.axis_value_array.take() else {
		return Ok(());
	};
	let mut kept = Vec::with_capacity(array.axis_value.len());
	for value in std::mem::take(&mut array.axis_value) {
		let keep = axis_value_is_kept(
			value.as_deref().ok_or_else(|| {
				Error::ttlib("AttributeError: 'NoneType' object has no attribute 'Format'")
			})?,
			&tags,
			&ranges,
		)?;
		if keep {
			kept.push(value);
		}
	}
	stat.axis_value_count = Some(kept.len() as u32);
	if !kept.is_empty() {
		array.axis_value = kept;
		stat.axis_value_array = Some(array);
	}
	Ok(())
}

/// `getVariationNameIDs`: the name IDs used by `fvar` and `STAT`, from 256 up.
fn variation_name_ids(font: &mut Font) -> Result<BTreeSet<u16>> {
	let mut used = BTreeSet::new();
	if let Some(fvar) = font.get::<Fvar>()? {
		used.extend(fvar.axes.iter().map(|a| a.axis_name_id));
		for instance in &fvar.instances {
			used.insert(instance.subfamily_name_id);
			if instance.postscript_name_id != 0xFFFF {
				used.insert(instance.postscript_name_id);
			}
		}
	}
	if let Some(stat) = font.get::<Stat>()? {
		if let Some(axes) = &stat.design_axis_record {
			used.extend(axes.axis.iter().map(|a| a.axis_name_id));
		}
		if let Some(values) = &stat.axis_value_array {
			for value in values.axis_value.iter().flatten() {
				used.insert(match &**value {
					ot::AxisValue::Format1(v) => v.value_name_id,
					ot::AxisValue::Format2(v) => v.value_name_id,
					ot::AxisValue::Format3(v) => v.value_name_id,
					ot::AxisValue::Format4(v) => v.value_name_id,
					ot::AxisValue::Unknown(_) => {
						return Err(Error::ttlib("AttributeError: no attribute 'ValueNameID'"));
					}
				});
			}
		}
		used.insert(stat.elided_fallback_name_id);
	}
	used.retain(|&id| id > 255);
	Ok(used)
}

/// The end of `pruningUnusedNames`.
fn prune_unused_names(font: &mut Font, original: &BTreeSet<u16>) -> Result<()> {
	log::info!("Pruning name table");
	let current = variation_name_ids(font)?;
	if !font.contains(NAME) {
		return Err(Error::ttlib("KeyError: 'name'"));
	}
	let name = font.require::<Name>()?;
	name.names
		.retain(|r| !original.contains(&r.name_id) || current.contains(&r.name_id));
	let uses_ltag = name
		.names
		.iter()
		.any(|r| r.platform_id == 0 && r.lang_id != 0xFFFF);
	// Drop `ltag` if all the language-dependent Unicode names have been dropped.
	if font.contains(LTAG) && !uses_ltag {
		font.remove(LTAG);
	}
	Ok(())
}

/// `set_default_weight_width_slant`.
// Not `clamp`, as it would keep a NaN.
#[allow(clippy::manual_clamp)]
fn set_default_weight_width_slant(font: &mut Font, location: &BTreeMap<String, f64>) -> Result<()> {
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
	if font.contains(OS2) {
		let os2 = font.require::<Os2>()?;
		if let Some(&wght) = location.get("wght") {
			os2.us_weight_class = ot_round(wght.min(1000.0).max(1.0)) as u16;
		}
		if let Some(&wdth) = location.get("wdth") {
			// Map the 'wdth' axis (50..200) to the width class (1..9).
			let value = wdth.max(50.0).min(200.0);
			os2.us_width_class =
				ot_round(piecewise_linear_map(value, &WDTH_VALUE_TO_OS2_WIDTH_CLASS)) as u16;
		}
	}
	if let Some(&slnt) = location.get("slnt")
		&& font.contains(POST)
	{
		font.require::<Post>()?.italic_angle = slnt.min(90.0).max(-90.0);
	}
	Ok(())
}

/// `setMacOverlapFlags`.
#[doc(hidden)]
pub fn set_mac_overlap_flags(font: &mut Font) -> Result<()> {
	const OVERLAP_COMPOUND: i64 = 0x0400;
	const FLAG_OVERLAP_SIMPLE: u8 = 0x40;
	for glyph in font.require::<Glyf>()?.glyphs.values_mut() {
		glyph.data = None;
		if glyph.is_composite() {
			if let Some(component) = glyph.components.first_mut() {
				component.flags |= OVERLAP_COMPOUND;
			}
		} else if glyph.number_of_contours > 0
			&& let Some(flag) = glyph.flags.first_mut()
		{
			*flag |= FLAG_OVERLAP_SIMPLE;
		}
	}
	Ok(())
}

/// `_instantiateVariationTables`.
fn instantiate_variation_tables(
	font: &mut Font,
	limits: &NormalizedLimits,
	optimize: bool,
	mut downgrade_cff2: bool,
) -> Result<bool> {
	if font.contains(VARC) {
		let pinned = limits.pinned_location();
		varc::instantiate_varc(font, &|tag| limits.0.contains_key(tag), &|tag| {
			pinned.contains_key(tag)
		})?;
	}
	if font.contains(CFF2) {
		downgrade_cff2 = cff::instantiate_cff2(font, limits, true, downgrade_cff2, None)?;
	}
	if font.has::<Gvar>() {
		instantiate_gvar(font, limits, optimize)?;
	}
	if font.contains(CVAR) {
		instantiate_cvar(font, limits)?;
	}
	if font.contains(Tag::new(b"MVAR")) {
		metrics::instantiate_mvar(font, limits)?;
	}
	if font.contains(Tag::new(b"HVAR")) {
		metrics::instantiate_hvar(font, limits)?;
	}
	if font.contains(Tag::new(b"VVAR")) {
		metrics::instantiate_vvar(font, limits)?;
	}
	colr::instantiate_colr(font, limits)?;
	otl::instantiate_otl(font, limits)?;
	feature_vars::instantiate_feature_variations(font, limits)?;
	Ok(downgrade_cff2)
}

/// `downgradeCFF2ToCFF`.
///
/// The font is saved and reloaded before and after the conversion, as in fontTools.
fn downgrade_cff2_to_cff(font: &mut Font) -> Result<()> {
	let (recalc_timestamp, recalc_bboxes) = (font.recalc_timestamp, font.recalc_bboxes);
	let use_harfbuzz_repacker = font.use_harfbuzz_repacker;
	let read_options = || ReadOptions {
		font_number: Some(0),
		recalc_timestamp: false,
		recalc_bboxes: false,
		use_harfbuzz_repacker,
		..Default::default()
	};
	font.recalc_timestamp = false;
	font.recalc_bboxes = false;
	let data = font.write(&WriteOptions::default())?;
	let mut new = Font::read(data, read_options())?;
	// UPSTREAM-BUG(B26): the conversion clears `cff2GetGlyphOrder` before the charstrings of
	// the reloaded font are loaded, which read the FDSelect with it.
	if new.contains(CFF2)
		&& new
			.require::<Cff2>()?
			.cff
			.top_dicts
			.first()
			.is_some_and(|t| t.raw.iter().any(|(k, _)| *k == "FDSelect"))
	{
		return Err(Error::ttlib("TypeError: 'NoneType' object is not callable"));
	}
	fontoxide_ttlib::cff::cff2_to_cff::convert_cff2_to_cff(&mut new, true)?;
	let data = new.write(&WriteOptions::default())?;
	let mut new = Font::read(data, read_options())?;
	new.recalc_timestamp = recalc_timestamp;
	new.recalc_bboxes = recalc_bboxes;
	*font = new;
	Ok(())
}

/// `sanityCheckVariableTables`.
#[doc(hidden)]
pub fn sanity_check_variable_tables(font: &Font) -> Result<()> {
	if !font.contains(FVAR) {
		return Err(Error::value("Missing required table fvar"));
	}
	let gvar_tag = font.family_tag::<Gvar>();
	if font.contains(gvar_tag) && !font.contains(Font::locked_tag::<Glyf>(gvar_tag == *b"GVAR")) {
		return Err(Error::value("Can't have gvar without glyf"));
	}
	Ok(())
}

/// `instantiateVariableFont`: instantiates a variable font in place, fully or
/// partially, depending on whether the limits refer to all or some of its axes.
pub fn instantiate_variable_font(
	font: &mut Font,
	limits: &AxisLimits,
	options: &InstancerOptions,
) -> Result<()> {
	sanity_check_variable_tables(font)?;
	feature_vars::check_conditions(font)?;

	let mut limits = limits.clone();
	if options.make_static {
		for (tag, _) in limits::fvar_axes(font)? {
			limits.entry(tag).or_insert(None);
		}
	}
	let user = UserLimits::from_limits(&limits, font)?.without_unchanged(font)?;
	// Keep the limited VARC support consistent even when avar2 partial instancing bypasses
	// `instantiate_variation_tables`.
	if font.contains(VARC) {
		varc::check_axis_limits(font, &|tag| user.0.contains_key(tag))?;
	}
	log::info!("Restricted limits: {user:?}");
	let normalized = user.normalize(font, true)?;
	log::info!("Normalized limits: {normalized:?}");

	if font.contains(DSIG) {
		font.remove(DSIG);
	}

	if options.update_font_names {
		log::info!("Updating name table");
		names::update_name_table(font, &user)?;
	}

	// Detect avar2 partial instancing: avar version 2, and not all fvar axes are pinned. In
	// this mode, the variation tables are kept in the old final-coordinate space, and the
	// offset compensation of avar ensures the correct final coordinates are produced.
	let is_avar2_partial = font.contains(AVAR) && font.require::<Avar>()?.major_version >= 2 && {
		let pinned = user.pinned_location();
		!limits::fvar_axes(font)?
			.iter()
			.all(|(tag, _)| pinned.contains_key(tag))
	};
	let mut downgrade_cff2 = options.downgrade_cff2;
	if is_avar2_partial {
		log::info!("avar2 partial instancing: keeping variation tables in old coordinate space");
		if downgrade_cff2 {
			// The result is still variable, so a downgrade to CFF is never possible.
			log::warn!("downgradeCFF2 is ignored for avar2 partial instancing");
			downgrade_cff2 = false;
		}
	} else {
		downgrade_cff2 =
			instantiate_variation_tables(font, &normalized, options.optimize, downgrade_cff2)?;
	}

	// For avar2 partial instancing, compute old intermediate values before avar instancing
	// modifies the segments.
	let mut reachable_ranges = avar::ReachableRanges::new();
	let mut old_intermediates = None;
	if is_avar2_partial {
		let intermediates = avar::compute_old_intermediates(font, &user)?;
		// For axes without a store delta, the final coord equals the intermediate coord.
		reachable_ranges = avar::exact_reachable_ranges(font, &user, &intermediates)?;
		old_intermediates = Some(intermediates);
	}

	let mut self_contained = avar::SelfContainedAxes::new();
	if font.contains(AVAR) {
		self_contained =
			avar::instantiate_avar(font, &user, &normalized, old_intermediates.as_ref())?;
	}

	// For avar2 partial instancing, run variation instancing for self-contained pinned axes
	// (those that can be removed from fvar): they have a constant old-space final coordinate,
	// so their contributions can be folded into gvar/HVAR/etc. at that coordinate.
	if is_avar2_partial && !self_contained.is_empty() {
		let mut limits = BTreeMap::new();
		for (tag, &v) in &self_contained {
			limits.insert(tag.clone(), NormalizedAxisTriple::new(v, v, v, 1.0, 1.0)?);
		}
		log::info!(
			"Instancing variation tables for self-contained axes: {:?}",
			self_contained.keys().collect::<Vec<_>>()
		);
		downgrade_cff2 = instantiate_variation_tables(
			font,
			&NormalizedLimits(limits),
			options.optimize,
			downgrade_cff2,
		)?;
	}

	let original_names = variation_name_ids(font)?;
	if font.contains(STAT) {
		instantiate_stat(font, &user)?;
	}
	if is_avar2_partial {
		avar::instantiate_fvar_for_avar2(font, &user, &self_contained)?;
	} else {
		instantiate_fvar(font, &user)?;
	}
	prune_unused_names(font, &original_names)?;

	// Cull dead gvar/cvar regions outside reachable old-space final-coord ranges. This must be
	// after updating fvar, so that the axis orders of fvar and the store match.
	if is_avar2_partial {
		// Extend the ranges for axes with store entries.
		avar::compute_reachable_ranges(font, &user, &mut reachable_ranges)?;
		// Component overrides can reach outside the font-level ranges.
		if !reachable_ranges.is_empty() && !font.contains(VARC) {
			avar::cull_variations(font, &reachable_ranges)?;
			if font.contains(CFF2)
				&& font
					.require::<Cff2>()?
					.cff
					.top_dicts
					.first()
					.is_some_and(|t| t.var_store.is_some())
			{
				// Via a populate/cull/depopulate cycle: blend operators encode region counts,
				// so the store can't be culled without updating them.
				cff::instantiate_cff2(
					font,
					&NormalizedLimits::default(),
					false,
					false,
					Some(&reachable_ranges),
				)?;
			}
		}
	}

	if font.contains(OS2) {
		let widths: Vec<u16> = match font.get::<Hmtx>()? {
			Some(hmtx) => hmtx.metrics.values().map(|m| m.0).collect(),
			None => Vec::new(),
		};
		font.require::<Os2>()?.recalc_avg_char_width(widths);
	}
	set_default_weight_width_slant(font, &user.default_location())?;

	if options.update_font_names {
		// After the name table has been updated.
		names::set_ribbi_bits(font)?;
	}

	if downgrade_cff2 {
		downgrade_cff2_to_cff(font)?;
	}

	if !font.contains(FVAR) {
		match options.overlap {
			OverlapMode::KeepAndSetFlags => {
				if font.has::<Glyf>() {
					set_mac_overlap_flags(font)?;
				}
			}
			OverlapMode::KeepAndDontSetFlags => {}
			OverlapMode::Remove | OverlapMode::RemoveAndIgnoreErrors => {
				log::info!("Removing glyph outlines overlaps");
				remove_overlaps(
					font,
					&RemoveOverlapsOptions {
						ignore_errors: options.overlap == OverlapMode::RemoveAndIgnoreErrors,
						..Default::default()
					},
				)?;
			}
		}
	}
	Ok(())
}
