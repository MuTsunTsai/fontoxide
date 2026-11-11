//! Port of `fontTools.varLib.featureVars`: building `FeatureVariations` tables.
//!
//! See <https://docs.microsoft.com/en-us/typography/opentype/spec/chapter2#featurevariations-table>.
//!
//! Terminology, as in fontTools:
//!
//! - A *box* ([`AxisBox`]) maps axis tags to `(minimum, maximum)` ranges, in
//!   normalized coordinates: an orthogonal "rectangular" part of the design
//!   space. Missing axes extend to the end of the axis in both directions.
//! - A *region* ([`Region`]) is a list of boxes: the union of them.
//! - A *substitution* ([`Substitution`]) maps glyph names to substitute glyph
//!   names.
//!
//! Python dicts are [`IndexMap`]s: boxes compare as dicts do, regardless of
//! the order of their axes, and with `-0.0 == 0.0`.
//!
//! Where fontTools iterates sets, the result never depends on the order,
//! except for the order of names in two error messages, which are sorted here
//! (see the `LENIENT` comments).

use std::any::Any;
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use fontoxide_misc::py::str_repr;
use fontoxide_otllib::GlyphMap;
use fontoxide_otllib::builder::{build_lookup, build_single_subst_subtable};
use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::{Fvar, Gsub, Os2};
use fontoxide_ttlib::{Error, Font, Result, TableType, Tag};
use indexmap::IndexMap;

/// A box: axis tag to `(minimum, maximum)`.
pub type AxisBox = IndexMap<String, (f64, f64)>;
/// A region: the union of boxes.
pub type Region = Vec<AxisBox>;
/// A substitution: glyph name to substitute glyph name.
pub type Substitution = IndexMap<String, String>;
/// A substitution as sorted `(glyph, substitute)` pairs, as
/// `makeSubstitutionsHashable` makes them (a tuple of tuples).
pub type SubstitutionTuple = Vec<(String, String)>;
/// The overlaid rules of [`overlay_feature_variations`]: each box with the
/// substitutions that apply in it. The substitutions are shared, as in
/// fontTools, rather than merged.
pub type OverlaidSubstitutions = Vec<(AxisBox, Vec<Arc<Substitution>>)>;

const RVRN: Tag = Tag::new(b"rvrn");

/// `VarLibError` and `VarLibValidationError`.
fn varlib_error(message: impl Into<String>) -> Error {
	Error::ttlib(message)
}

fn attribute_error(name: &str) -> Error {
	Error::ttlib(format!(
		"AttributeError: 'NoneType' object has no attribute '{name}'"
	))
}

fn index_error() -> Error {
	Error::ttlib("IndexError: list index out of range")
}

/// A lookup or feature index as the `uint16` it is written as.
// LENIENT(L30): fontTools keeps Python ints and fails only when compiling the table
// (`struct.error`); this fails as soon as an index does not fit.
fn to_u16(value: usize, what: &str) -> Result<u16> {
	u16::try_from(value).map_err(|_| Error::value(format!("{what} {value} out of range")))
}

/// `featureTags = sorted(featureTag)` and `processLast`.
fn sorted_tags(feature_tags: &[Tag]) -> (Vec<Tag>, bool) {
	let mut tags = feature_tags.to_vec();
	tags.sort();
	// Process first when "rvrn" is the only listed tag.
	let process_last = !tags.contains(&RVRN) || tags.len() > 1;
	(tags, process_last)
}

// ---------------------------------------------------------------------------
// The layout table parts
// ---------------------------------------------------------------------------

/// The lookup list of a layout table: either `LookupList` or `LookupList2`.
#[derive(Debug)]
pub struct LookupListMut<'a> {
	/// `Lookup`.
	pub lookup: &'a mut Vec<Option<Box<ot::Lookup>>>,
	/// `LookupCount`.
	pub lookup_count: &'a mut Option<u32>,
}

/// The parts of a `GSUB` or `GPOS` table that feature variations touch, with
/// the lists chosen as `_getLayoutList` does: the extended lists (`ScriptList2`
/// and such) for version 1.2 and later, else the compact ones; whichever exists.
#[derive(Debug)]
pub struct LayoutParts<'a> {
	/// `Version`.
	pub version: &'a mut u32,
	/// `_getLayoutList(table, "ScriptList")`.
	pub script_list: Option<&'a mut ot::ScriptList>,
	/// `_getLayoutList(table, "FeatureList")`.
	pub feature_list: Option<&'a mut ot::FeatureList>,
	/// `_getLookupList(table)`.
	pub lookup_list: Option<LookupListMut<'a>>,
	/// `FeatureVariations`.
	pub feature_variations: &'a mut Option<Box<ot::FeatureVariations>>,
}

/// A `GSUB` or `GPOS` table (`ot.GSUB` and `ot.GPOS`).
pub trait OtlTable {
	/// The parts of the table that feature variations touch.
	fn layout_parts(&mut self) -> LayoutParts<'_>;
}

/// `compact or extended`, or `extended or compact`.
fn pick<'a, T>(
	extended_first: bool,
	compact: &'a mut Option<Box<T>>,
	extended: &'a mut Option<Box<T>>,
) -> Option<&'a mut T> {
	if extended_first {
		extended.as_deref_mut().or(compact.as_deref_mut())
	} else {
		compact.as_deref_mut().or(extended.as_deref_mut())
	}
}

macro_rules! otl_table {
	($ty:ident) => {
		impl OtlTable for ot::$ty {
			fn layout_parts(&mut self) -> LayoutParts<'_> {
				let ot::$ty {
					version,
					script_list,
					feature_list,
					lookup_list,
					feature_variations,
					script_list2,
					feature_list2,
					lookup_list2,
				} = self;
				let extended_first = *version >= 0x0001_0002;
				let compact = lookup_list.as_deref_mut().map(|l| LookupListMut {
					lookup: &mut l.lookup,
					lookup_count: &mut l.lookup_count,
				});
				let extended = lookup_list2.as_deref_mut().map(|l| LookupListMut {
					lookup: &mut l.lookup,
					lookup_count: &mut l.lookup_count,
				});
				let lookup_list = if extended_first {
					extended.or(compact)
				} else {
					compact.or(extended)
				};
				LayoutParts {
					script_list: pick(extended_first, script_list, script_list2),
					feature_list: pick(extended_first, feature_list, feature_list2),
					lookup_list,
					feature_variations,
					version,
				}
			}
		}
	};
}

otl_table!(GSUB);
otl_table!(GPOS);

// ---------------------------------------------------------------------------
// High level
// ---------------------------------------------------------------------------

/// `addFeatureVariations`: adds conditional substitutions to a variable font.
///
/// `conditional_substitutions` is a list of `(region, substitution)` pairs.
/// For efficiency, the boxes of a region should ideally not overlap, but it
/// works if they do. The coordinates are normalized.
///
/// `feature_tags` are the features that the variations are associated with
/// (fontTools' `featureTag` takes a string or several; its default is `rvrn`).
/// If `rvrn` is the only one, the substitution lookups are inserted at the
/// beginning of the lookup list so that they are processed before the others;
/// otherwise they are appended.
pub fn add_feature_variations(
	font: &mut Font,
	conditional_substitutions: &[(Region, Substitution)],
	feature_tags: &[Tag],
) -> Result<()> {
	let (feature_tags, process_last) = sorted_tags(feature_tags);

	let glyph_map = GlyphMap::from_font(font).map_err(otl_error)?;
	check_substitution_glyphs_exist(&glyph_map, conditional_substitutions)?;

	let substitutions = overlay_feature_variations(conditional_substitutions);

	// Turn substitution dicts into tuples of tuples, so they are hashable.
	let (conditional_substitutions, all_substitutions) =
		make_substitutions_hashable(&substitutions);
	if font.get::<Gsub>()?.is_none() {
		font.insert(Box::new(build_gsub()));
	} else {
		let gsub = font.require::<Gsub>()?;
		let existing = existing_variable_features(&mut gsub.table)?;
		let existing: BTreeSet<Tag> = existing
			.into_iter()
			.filter(|t| feature_tags.contains(t))
			.collect();
		if !existing.is_empty() {
			// LENIENT(L30): fontTools prints the set in hash order; this sorts it.
			let tags: Vec<String> = existing.iter().map(|t| str_repr(&t.as_string())).collect();
			return Err(varlib_error(format!(
				"FeatureVariations already exist for feature tag(s): {{{}}}",
				tags.join(", ")
			)));
		}
	}

	font.with_table(Gsub::TAG, |table, font| {
		let gsub = (*table)
			.as_any_mut()
			.downcast_mut::<Gsub>()
			.ok_or(Error::WrongTableType(Gsub::TAG))?;
		// Set up the lookups.
		let lookup_map = build_substitution_lookups(
			&mut gsub.table,
			&all_substitutions,
			process_last,
			&glyph_map,
		)?;

		// `addFeatureVariationsRaw` takes a list of (condition, lookup indices),
		// so rearrange the lookups to match.
		let mut conditions_and_lookups = Vec::with_capacity(conditional_substitutions.len());
		for (condition_set, substitutions) in conditional_substitutions {
			let mut indices = Vec::with_capacity(substitutions.len());
			for s in &substitutions {
				let index = lookup_map
					.get(s)
					.copied()
					.ok_or_else(|| Error::ttlib("KeyError: substitution"))?;
				indices.push(to_u16(index, "lookup index")?);
			}
			conditions_and_lookups.push((condition_set, indices));
		}
		add_feature_variations_raw(
			font,
			&mut gsub.table,
			&conditions_and_lookups,
			&feature_tags,
		)
	})?;

	// Update OS/2.usMaxContext in case the font didn't have features before,
	// but does now. Feature variations are always 1:1 mappings, so it is at
	// least 1.
	if let Some(os2) = font.get::<Os2>()? {
		os2.us_max_context = os2.us_max_context.max(1);
	}
	Ok(())
}

fn otl_error(e: fontoxide_otllib::Error) -> Error {
	match e {
		fontoxide_otllib::Error::TtLib(e) => e,
		fontoxide_otllib::Error::Value(m) => Error::Value(m),
		fontoxide_otllib::Error::Assertion(m) => Error::Assertion(m),
		e => Error::ttlib(e.to_string()),
	}
}

/// `_existingVariableFeatures`: the tags of the features that feature
/// variations substitute.
fn existing_variable_features<T: OtlTable + ?Sized>(table: &mut T) -> Result<BTreeSet<Tag>> {
	let parts = table.layout_parts();
	let mut tags = BTreeSet::new();
	let Some(variations) = parts.feature_variations.as_deref() else {
		return Ok(tags);
	};
	let features = &parts
		.feature_list
		.ok_or_else(|| attribute_error("FeatureRecord"))?
		.feature_record;
	for fvr in &variations.feature_variation_record {
		let Some(substitution) = &fvr.feature_table_substitution else {
			continue;
		};
		for ftsr in &substitution.substitution_record {
			let feature = features
				.get(usize::from(ftsr.feature_index))
				.ok_or_else(index_error)?;
			tags.insert(feature.feature_tag);
		}
	}
	Ok(tags)
}

/// `_checkSubstitutionGlyphsExist`.
fn check_substitution_glyphs_exist(
	glyph_map: &GlyphMap,
	substitutions: &[(Region, Substitution)],
) -> Result<()> {
	let mut missing = BTreeSet::new();
	for (_, substitution) in substitutions {
		for (from, to) in substitution {
			for name in [from, to] {
				if glyph_map.get(name).is_none() {
					missing.insert(name.as_str());
				}
			}
		}
	}
	if missing.is_empty() {
		return Ok(());
	}
	// LENIENT(L30): fontTools joins the names of a set, in hash order; this sorts them.
	let names: Vec<&str> = missing.into_iter().collect();
	Err(varlib_error(format!(
		"Missing glyphs are referenced in conditional substitution rules: {}",
		names.join(", ")
	)))
}

// ---------------------------------------------------------------------------
// Overlaying boxes
// ---------------------------------------------------------------------------

/// A Python float as a dict key: `-0.0 == 0.0`.
fn float_key(v: f64) -> u64 {
	(v + 0.0).to_bits()
}

/// A box as a `hashdict` key: equal whatever the order of the axes.
type BoxKey = Vec<(String, u64, u64)>;

fn box_key(b: &AxisBox) -> BoxKey {
	let mut key: BoxKey = b
		.iter()
		.map(|(tag, &(min, max))| (tag.clone(), float_key(min), float_key(max)))
		.collect();
	key.sort();
	key
}

/// `tuple(sorted(substitution.items()))`.
fn substitution_tuple(substitution: &Substitution) -> SubstitutionTuple {
	let mut items: SubstitutionTuple = substitution
		.iter()
		.map(|(k, v)| (k.clone(), v.clone()))
		.collect();
	items.sort();
	items
}

/// Python's `<` between floats as an [`Ordering`] (equal when unordered).
fn float_cmp(a: f64, b: f64) -> Ordering {
	a.partial_cmp(&b).unwrap_or(Ordering::Equal)
}

/// Compares boxes by `tuple(sorted(box.items()))`.
fn box_cmp(a: &AxisBox, b: &AxisBox) -> Ordering {
	let (a, b) = (sorted_items(a), sorted_items(b));
	for (x, y) in a.iter().zip(&b) {
		let ord = item_cmp(x, y);
		if ord != Ordering::Equal {
			return ord;
		}
	}
	a.len().cmp(&b.len())
}

/// Compares `(tag, (minimum, maximum))` items as Python tuples.
fn item_cmp(x: &(&String, (f64, f64)), y: &(&String, (f64, f64))) -> Ordering {
	x.0.cmp(y.0)
		.then_with(|| float_cmp(x.1.0, y.1.0))
		.then_with(|| float_cmp(x.1.1, y.1.1))
}

/// `sorted(box.items())`.
fn sorted_items(b: &AxisBox) -> Vec<(&String, (f64, f64))> {
	let mut items: Vec<(&String, (f64, f64))> = b.iter().map(|(k, v)| (k, *v)).collect();
	items.sort_by(item_cmp);
	items
}

/// A rank: the set of the indices of the contributing rules (a Python int
/// used as a bit set).
#[derive(Debug, Clone, Default)]
struct Rank(Vec<u64>);

impl Rank {
	fn bit(i: usize) -> Self {
		let mut words = vec![0; i / 64 + 1];
		words[i / 64] = 1 << (i % 64);
		Rank(words)
	}

	fn or(&mut self, other: &Rank) {
		if self.0.len() < other.0.len() {
			self.0.resize(other.0.len(), 0);
		}
		for (a, b) in self.0.iter_mut().zip(&other.0) {
			*a |= b;
		}
	}

	fn is_zero(&self) -> bool {
		self.0.iter().all(|&w| w == 0)
	}

	/// `bit_count`.
	fn count(&self) -> u32 {
		self.0.iter().map(|w| w.count_ones()).sum()
	}

	/// The indices of the bits, from the lowest.
	fn bits(&self) -> impl Iterator<Item = usize> + '_ {
		self.0.iter().enumerate().flat_map(|(i, &w)| {
			(0..64)
				.filter(move |b| w & (1 << b) != 0)
				.map(move |b| i * 64 + b)
		})
	}
}

/// `overlayFeatureVariations`: computes the overlaps between all conditional
/// substitutions.
///
/// Overlaps of distinct substitution boxes (not regions) become distinct
/// rules, and rules with the same box are merged. The more specific rules
/// come first. Each box comes with the list of the input substitutions that
/// apply in it, which are not merged so that they can be shared when they are
/// turned into tables.
///
/// fontTools extends the region lists of its input when it merges rules with
/// the same substitution; this does not modify its input.
pub fn overlay_feature_variations(
	conditional_substitutions: &[(Region, Substitution)],
) -> OverlaidSubstitutions {
	// Merge same-substitutions rules, as this creates fewer lookups.
	let mut merged: IndexMap<SubstitutionTuple, (Region, Substitution)> = IndexMap::new();
	for (value, key) in conditional_substitutions {
		match merged.entry(substitution_tuple(key)) {
			indexmap::map::Entry::Occupied(mut e) => e.get_mut().0.extend(value.iter().cloned()),
			indexmap::map::Entry::Vacant(e) => {
				e.insert((value.clone(), key.clone()));
			}
		}
	}

	// Merge same-region rules, as this is cheaper, after removing the default
	// ranges of the boxes. Reversing is such that earlier entries win in case
	// of conflicting substitution rules for the same region.
	let mut by_region: IndexMap<Vec<BoxKey>, (Region, Substitution)> = IndexMap::new();
	for (region, substitution) in merged.into_values().rev() {
		let mut boxes: Region = region.iter().map(cleanup_box).collect();
		boxes.sort_by(box_cmp);
		let key: Vec<BoxKey> = boxes.iter().map(box_key).collect();
		match by_region.entry(key) {
			indexmap::map::Entry::Occupied(mut e) => e.get_mut().1.extend(substitution),
			indexmap::map::Entry::Vacant(e) => {
				e.insert((boxes, substitution));
			}
		}
	}
	let rules: Vec<(Region, Arc<Substitution>)> = by_region
		.into_values()
		.rev()
		.map(|(region, substitution)| (region, Arc::new(substitution)))
		.collect();

	// Overlay. The rank is the bit set of the indices of all contributing
	// layers; the empty box is the entire space.
	let init = || {
		let mut map: IndexMap<BoxKey, (AxisBox, Rank)> = IndexMap::new();
		map.insert(Vec::new(), (AxisBox::new(), Rank::default()));
		map
	};
	let mut box_map = init();
	for (i, (curr_region, _)) in rules.iter().enumerate() {
		let mut new_map = init();
		let curr_rank = Rank::bit(i);
		for (bx, rank) in box_map.values() {
			for curr_box in curr_region {
				let (intersection, remainder) = overlay_box(curr_box, bx);
				if let Some(intersection) = intersection {
					let entry = new_map
						.entry(box_key(&intersection))
						.or_insert_with(|| (intersection, Rank::default()));
					entry.1.or(rank);
					entry.1.or(&curr_rank);
				}
				if let Some(remainder) = remainder {
					let entry = new_map
						.entry(box_key(&remainder))
						.or_insert_with(|| (remainder, Rank::default()));
					entry.1.or(rank);
				}
			}
		}
		box_map = new_map;
	}

	// Generate output.
	let mut entries: Vec<(AxisBox, Rank)> = box_map.into_values().collect();
	entries.sort_by_key(|(_, rank)| std::cmp::Reverse(rank.count()));
	let mut items = Vec::new();
	for (bx, rank) in entries {
		// Skip any box that doesn't have any substitution.
		if rank.is_zero() {
			continue;
		}
		let substs = rank.bits().map(|i| Arc::clone(&rules[i].1)).collect();
		items.push((bx, substs));
	}
	items
}

/// Python's `max(a, b)`: `a` unless `b` is greater.
fn py_max(a: f64, b: f64) -> f64 {
	if b > a { b } else { a }
}

/// Python's `min(a, b)`: `a` unless `b` is less.
fn py_min(a: f64, b: f64) -> f64 {
	if b < a { b } else { a }
}

/// `overlayBox`: overlays the `top` box on the `bot` box.
///
/// Returns the intersection of the boxes, or `None` if they don't intersect,
/// and the remainder of `bot`, or `None` if it is empty. The remainder might
/// not be exact (the remainder might not be a simple box), but includes the
/// exact remainder.
pub fn overlay_box(top: &AxisBox, bot: &AxisBox) -> (Option<AxisBox>, Option<AxisBox>) {
	// Intersection.
	let mut intersection = top.clone();
	for (tag, &range) in bot {
		intersection.insert(tag.clone(), range);
	}
	for (tag, &(min1, max1)) in top {
		let Some(&(min2, max2)) = bot.get(tag) else {
			continue;
		};
		let minimum = py_max(min1, min2);
		let maximum = py_min(max1, max2);
		// `not minimum < maximum`, which also holds for NaN.
		if minimum.partial_cmp(&maximum) != Some(Ordering::Less) {
			// Do not intersect.
			return (None, Some(bot.clone()));
		}
		intersection.insert(tag.clone(), (minimum, maximum));
	}

	// Remainder.
	//
	// The remainder is empty if each axis range of bot lies within that of the
	// intersection.
	//
	// The remainder shrinks if each axis range of bot, except for exactly one,
	// lies within that of the intersection, and on that one axis, it extrudes
	// out of the intersection only on one side.
	//
	// Bot is returned in full as the remainder otherwise, as the true remainder
	// is not representable as a single box.
	let mut remainder = bot.clone();
	let mut extruding = top.keys().any(|tag| !bot.contains_key(tag));
	let mut fully_inside = !extruding;
	for (tag, &(min2, max2)) in bot {
		if !top.contains_key(tag) {
			// The axis range lies fully within.
			continue;
		}
		let (min1, max1) = intersection[tag];
		if min1 <= min2 && max2 <= max1 {
			// The axis range lies fully within.
			continue;
		}

		// The range of bot doesn't lie fully within that of top on this axis. We
		// know they intersect, so it cannot lie fully without either; so they
		// overlap.

		// If we have had an overlapping axis before, the remainder is not
		// representable as a box, so return the full bottom and go home.
		if extruding {
			return (Some(intersection), Some(bot.clone()));
		}
		extruding = true;
		fully_inside = false;

		// Otherwise, cut the remainder on this axis and continue.
		let range = if min1 <= min2 {
			// The right side survives.
			(py_max(max1, min2), max2)
		} else if max2 <= max1 {
			// The left side survives.
			(min2, py_min(min1, max2))
		} else {
			// The remainder leaks out from both sides. Can't cut either.
			return (Some(intersection), Some(bot.clone()));
		};
		remainder.insert(tag.clone(), range);
	}

	if fully_inside {
		// Bot is fully within the intersection. The remainder is empty.
		return (Some(intersection), None);
	}
	(Some(intersection), Some(remainder))
}

/// `cleanupBox`: a sparse copy of `box`, without the default ranges
/// `(-1.0, 1.0)`.
///
/// ```
/// use fontoxide_varlib::feature_vars::{AxisBox, cleanup_box};
///
/// assert_eq!(cleanup_box(&AxisBox::new()), AxisBox::new());
/// let b = AxisBox::from([("wdth".to_owned(), (0.0, 1.0))]);
/// assert_eq!(cleanup_box(&b), b);
/// let b = AxisBox::from([("wdth".to_owned(), (-1.0, 1.0))]);
/// assert_eq!(cleanup_box(&b), AxisBox::new());
/// ```
pub fn cleanup_box(b: &AxisBox) -> AxisBox {
	b.iter()
		.filter(|(_, limit)| **limit != (-1.0, 1.0))
		.map(|(tag, limit)| (tag.clone(), *limit))
		.collect()
}

// ---------------------------------------------------------------------------
// Low level implementation
// ---------------------------------------------------------------------------

/// `addFeatureVariationsRaw`: the low level implementation of
/// [`add_feature_variations`], which directly models the `FeatureVariations`
/// table.
///
/// `table` is a `GSUB` or `GPOS` table (not necessarily in `font`, which only
/// provides the `fvar` axes). `conditional_substitutions` lists, for each
/// condition set (axis tag to `(minimum, maximum)`), the indices of the lookups
/// to apply. The lookups are added to the `feature_tags` features, which are
/// created (and added to all scripts and language systems) if they don't exist.
pub fn add_feature_variations_raw<T: OtlTable + ?Sized>(
	font: &mut Font,
	table: &mut T,
	conditional_substitutions: &[(AxisBox, Vec<u16>)],
	feature_tags: &[Tag],
) -> Result<()> {
	let (feature_tags, process_last) = sorted_tags(feature_tags);
	let mut parts = table.layout_parts();

	// If a <featureTag> feature is not present: make an empty <featureTag>
	// feature, sort the features, get the <featureTag> feature index, and add
	// it to all scripts. If it is present, reuse its index. Then add the feature
	// variations.
	if *parts.version < 0x0001_0001 {
		// Allow table.FeatureVariations.
		*parts.version = 0x0001_0001;
	}

	let mut var_feature_indices: BTreeSet<u16> = BTreeSet::new();

	let feature_list = parts
		.feature_list
		.as_deref_mut()
		.ok_or_else(|| attribute_error("FeatureRecord"))?;
	let existing_tags: HashSet<Tag> = feature_list
		.feature_record
		.iter()
		.map(|f| f.feature_tag)
		.filter(|t| feature_tags.contains(t))
		.collect();

	let new_tags: BTreeSet<Tag> = feature_tags
		.iter()
		.copied()
		.filter(|t| !existing_tags.contains(t))
		.collect();
	if !new_tags.is_empty() {
		let first = feature_list.feature_record.len();
		for &tag in &new_tags {
			feature_list
				.feature_record
				.push(build_feature_record(tag, Vec::new()));
		}
		feature_list.feature_count = Some(feature_list.feature_record.len() as u32);

		let feature_remap = sort_feature_list_parts(&mut parts)?;

		let script_list = parts
			.script_list
			.as_deref_mut()
			.ok_or_else(|| attribute_error("ScriptRecord"))?;
		for k in 0..new_tags.len() {
			// `featureList.FeatureRecord.index(varFeature)`: the new features have
			// distinct tags that no other feature has.
			let var_feature_index = feature_remap
				.get(&(first + k))
				.copied()
				.ok_or_else(index_error)?;
			for script_record in &mut script_list.script_record {
				let script = script_record
					.script
					.as_deref_mut()
					.ok_or_else(|| attribute_error("DefaultLangSys"))?;
				// We need to have a default LangSys to attach variations to.
				let default = script.default_lang_sys.get_or_insert_with(|| {
					Box::new(ot::LangSys {
						lookup_order: None,
						req_feature_index: 0xFFFF,
						feature_count: Some(0),
						feature_index: Vec::new(),
					})
				});
				push_feature_index(default, var_feature_index);
				for record in &mut script.lang_sys_record {
					let lang_sys = record
						.lang_sys
						.as_deref_mut()
						.ok_or_else(|| attribute_error("FeatureIndex"))?;
					push_feature_index(lang_sys, var_feature_index);
				}
			}
			var_feature_indices.insert(var_feature_index);
		}
	}

	let feature_list = parts
		.feature_list
		.as_deref_mut()
		.ok_or_else(|| attribute_error("FeatureRecord"))?;
	if !existing_tags.is_empty() {
		// The indices may have changed if we inserted new features and sorted
		// the feature list, so this must come after.
		for (index, feature) in feature_list.feature_record.iter().enumerate() {
			if existing_tags.contains(&feature.feature_tag) {
				var_feature_indices.insert(to_u16(index, "feature index")?);
			}
		}
	}

	// `{axis.axisTag: axisIndex for axisIndex, axis in enumerate(font["fvar"].axes)}`.
	let fvar = font.require::<Fvar>()?;
	let mut axis_indices: HashMap<String, u16> = HashMap::new();
	for (i, axis) in fvar.axes.iter().enumerate() {
		axis_indices.insert(axis.tag_text(), to_u16(i, "axis index")?);
	}

	let has_feature_variations = parts.feature_variations.is_some();

	let mut feature_variation_records = Vec::new();
	for (condition_set, lookup_indices) in conditional_substitutions {
		let items = sorted_items(condition_set);
		let mut condition_table = Vec::with_capacity(items.len());
		for (axis_tag, (min_value, max_value)) in items {
			if min_value > max_value {
				return Err(varlib_error(
					"A condition set has a minimum value above the maximum value.",
				));
			}
			let axis_index = *axis_indices
				.get(axis_tag)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {}", str_repr(axis_tag))))?;
			condition_table.push(build_condition_table(axis_index, min_value, max_value));
		}
		let mut records = Vec::with_capacity(var_feature_indices.len());
		for &var_feature_index in &var_feature_indices {
			let existing = &feature_list
				.feature_record
				.get(usize::from(var_feature_index))
				.ok_or_else(index_error)?
				.feature
				.as_deref()
				.ok_or_else(|| attribute_error("LookupListIndex"))?
				.lookup_list_index;
			let combined: Vec<u16> = if process_last {
				existing.iter().chain(lookup_indices).copied().collect()
			} else {
				lookup_indices.iter().chain(existing).copied().collect()
			};
			records.push(build_feature_table_substitution_record(
				var_feature_index,
				combined,
			));
		}
		let found = match parts.feature_variations.as_deref_mut() {
			Some(variations) if has_feature_variations => {
				find_feature_variation_record(variations, &condition_table)?
			}
			_ => None,
		};
		match found {
			Some(fvr) => {
				let substitution = fvr.feature_table_substitution.get_or_insert_with(|| {
					Box::new(ot::FeatureTableSubstitution {
						version: 0x0001_0000,
						substitution_count: Some(0),
						substitution_record: Vec::new(),
					})
				});
				substitution.substitution_record.extend(records);
				// The spec requires records sorted by increasing FeatureIndex.
				substitution
					.substitution_record
					.sort_by_key(|r| r.feature_index);
				substitution.substitution_count =
					Some(substitution.substitution_record.len() as u32);
			}
			None => {
				feature_variation_records
					.push(build_feature_variation_record(condition_table, records));
			}
		}
	}

	match parts.feature_variations.as_deref_mut() {
		Some(variations) if has_feature_variations => {
			check_feature_variations_version(variations)?;
			variations
				.feature_variation_record
				.extend(feature_variation_records);
			variations.feature_variation_count =
				Some(variations.feature_variation_record.len() as u32);
		}
		_ => {
			*parts.feature_variations = Some(Box::new(build_feature_variations(
				feature_variation_records,
			)));
		}
	}
	Ok(())
}

fn push_feature_index(lang_sys: &mut ot::LangSys, index: u16) {
	lang_sys.feature_index.push(index);
	lang_sys.feature_count = Some(lang_sys.feature_index.len() as u32);
}

fn check_feature_variations_version(variations: &ot::FeatureVariations) -> Result<()> {
	if variations.version != 0x0001_0000 {
		return Err(varlib_error(format!(
			"Unsupported FeatureVariations table version: 0x{:08x} (expected 0x00010000).",
			variations.version
		)));
	}
	Ok(())
}

// ---------------------------------------------------------------------------
// Building GSUB/FeatureVariations internals
// ---------------------------------------------------------------------------

/// `buildGSUB`: a `GSUB` table from scratch, with a `DFLT` script.
pub fn build_gsub() -> Gsub {
	let lang_sys = ot::LangSys {
		lookup_order: None,
		req_feature_index: 0xFFFF,
		feature_count: None,
		feature_index: Vec::new(),
	};
	let script_record = ot::ScriptRecord {
		script_tag: Tag::new(b"DFLT"),
		script: Some(Box::new(ot::Script {
			default_lang_sys: Some(Box::new(lang_sys)),
			lang_sys_count: Some(0),
			lang_sys_record: Vec::new(),
		})),
	};
	Gsub {
		table: ot::GSUB {
			// Allow FeatureVariations.
			version: 0x0001_0001,
			script_list: Some(Box::new(ot::ScriptList {
				script_count: Some(1),
				script_record: vec![script_record],
			})),
			feature_list: Some(Box::default()),
			lookup_list: Some(Box::default()),
			feature_variations: None,
			..Default::default()
		},
	}
}

/// `makeSubstitutionsHashable`: turns the substitutions into sorted tuples, so
/// that duplicates are found and no redundant data is written. Returns the
/// conditions with their substitutions, and all the distinct substitutions,
/// sorted.
pub fn make_substitutions_hashable<S: Borrow<Substitution>>(
	conditional_substitutions: &[(AxisBox, Vec<S>)],
) -> (
	Vec<(AxisBox, Vec<SubstitutionTuple>)>,
	Vec<SubstitutionTuple>,
) {
	let mut all_substitutions = BTreeSet::new();
	let mut cond_subst = Vec::with_capacity(conditional_substitutions.len());
	for (condition_set, substitution_maps) in conditional_substitutions {
		let mut substitutions = Vec::with_capacity(substitution_maps.len());
		for map in substitution_maps {
			let subst = substitution_tuple(map.borrow());
			all_substitutions.insert(subst.clone());
			substitutions.push(subst);
		}
		cond_subst.push((condition_set.clone(), substitutions));
	}
	(cond_subst, all_substitutions.into_iter().collect())
}

/// `ShifterVisitor`: adds `shift` to the lookup indices of features and of
/// contextual lookup records.
fn shift_visit(any: &mut dyn Any, shift: usize) -> Result<()> {
	let shifted = |l: u16| to_u16(usize::from(l) + shift, "lookup index");
	if let Some(feature) = any.downcast_mut::<ot::Feature>() {
		for l in &mut feature.lookup_list_index {
			*l = shifted(*l)?;
		}
	} else if let Some(r) = any.downcast_mut::<ot::SubstLookupRecord>() {
		r.lookup_list_index = shifted(r.lookup_list_index)?;
	} else if let Some(r) = any.downcast_mut::<ot::PosLookupRecord>() {
		r.lookup_list_index = shifted(r.lookup_list_index)?;
	} else if let Some(r) = any.downcast_mut::<ot::SeqLookup>() {
		r.lookup_list_index = shifted(r.lookup_list_index)?;
	}
	Ok(())
}

/// `buildSubstitutionLookups`: builds the lookups of the substitutions, and
/// returns the lookup index of each.
///
/// The lookups are appended with `process_last`; otherwise they are inserted
/// at the beginning of the lookup list
/// (<https://github.com/googlefonts/fontmake/issues/950>), and all the lookup
/// indices of the table are shifted.
///
/// `glyph_map` maps the glyph names to IDs, which fontTools' tables do not need.
pub fn build_substitution_lookups<T: OtlTable + ?Sized>(
	gsub: &mut T,
	all_substitutions: &[SubstitutionTuple],
	process_last: bool,
	glyph_map: &GlyphMap,
) -> Result<HashMap<SubstitutionTuple, usize>> {
	let parts = gsub.layout_parts();
	let lookup_list = parts.lookup_list.ok_or_else(|| attribute_error("Lookup"))?;
	let first_index = if process_last {
		lookup_list.lookup.len()
	} else {
		0
	};
	let mut lookup_map = HashMap::with_capacity(all_substitutions.len());
	for (i, subst) in all_substitutions.iter().enumerate() {
		lookup_map.insert(subst.clone(), first_index + i);
	}

	if !process_last {
		// Shift all lookup indices in GSUB by the number of new lookups.
		let shift = all_substitutions.len();
		let mut visit = |any: &mut dyn Any| shift_visit(any, shift);
		let feature_list = parts
			.feature_list
			.ok_or_else(|| attribute_error("FeatureRecord"))?;
		for record in &mut feature_list.feature_record {
			record.visit_mut(&mut visit)?;
		}
		for lookup in lookup_list.lookup.iter_mut().flatten() {
			lookup.visit_mut(&mut visit)?;
		}
		// UPSTREAM-BUG(B28): the visitor shifts the features of FeatureVariations,
		// but not the `LookupIndex` of its LookupVariationRecords, which then
		// refer to the wrong lookups.
		if let Some(variations) = parts.feature_variations.as_deref_mut() {
			variations.visit_mut(&mut visit)?;
		}
	}

	for (i, subst) in all_substitutions.iter().enumerate() {
		let subst_map: IndexMap<String, String> = subst.iter().cloned().collect();
		let subtable = build_single_subst_subtable(&subst_map, glyph_map)
			.map_err(otl_error)?
			.map(ot::LookupSubTable::SingleSubst);
		let lookup = build_lookup(vec![subtable], 0, None, None, false)
			.map_err(otl_error)?
			.map(Box::new);
		if process_last {
			lookup_list.lookup.push(lookup);
		} else {
			lookup_list.lookup.insert(i, lookup);
		}
	}
	*lookup_list.lookup_count = Some(lookup_list.lookup.len() as u32);
	Ok(lookup_map)
}

/// `buildFeatureVariations`: the `FeatureVariations` subtable.
pub fn build_feature_variations(
	feature_variation_records: Vec<ot::FeatureVariationRecord>,
) -> ot::FeatureVariations {
	ot::FeatureVariations {
		version: 0x0001_0000,
		feature_variation_count: Some(feature_variation_records.len() as u32),
		feature_variation_record: feature_variation_records,
		..Default::default()
	}
}

/// `buildFeatureRecord`.
pub fn build_feature_record(feature_tag: Tag, lookup_list_indices: Vec<u16>) -> ot::FeatureRecord {
	ot::FeatureRecord {
		feature_tag,
		// `populateDefaults()`.
		feature: Some(Box::new(ot::Feature {
			feature_params: None,
			lookup_count: Some(lookup_list_indices.len() as u32),
			lookup_list_index: lookup_list_indices,
		})),
	}
}

/// `buildFeatureVariationRecord`: without conditions, the record has no
/// `ConditionSet`.
pub fn build_feature_variation_record(
	condition_table: Vec<ot::ConditionTable>,
	substitution_records: Vec<ot::FeatureTableSubstitutionRecord>,
) -> ot::FeatureVariationRecord {
	let condition_set = if condition_table.is_empty() {
		None
	} else {
		Some(Box::new(ot::ConditionSet {
			condition_count: Some(condition_table.len() as u32),
			condition_table: condition_table
				.into_iter()
				.map(|c| Some(Box::new(c)))
				.collect(),
		}))
	};
	ot::FeatureVariationRecord {
		condition_set,
		feature_table_substitution: Some(Box::new(ot::FeatureTableSubstitution {
			version: 0x0001_0000,
			substitution_count: Some(substitution_records.len() as u32),
			substitution_record: substitution_records,
		})),
	}
}

/// `buildFeatureTableSubstitutionRecord`.
pub fn build_feature_table_substitution_record(
	feature_index: u16,
	lookup_list_indices: Vec<u16>,
) -> ot::FeatureTableSubstitutionRecord {
	ot::FeatureTableSubstitutionRecord {
		feature_index,
		feature: Some(Box::new(ot::Feature {
			feature_params: None,
			lookup_count: Some(lookup_list_indices.len() as u32),
			lookup_list_index: lookup_list_indices,
		})),
	}
}

/// `buildConditionTable`: a format 1 condition.
pub fn build_condition_table(
	axis_index: u16,
	filter_range_min_value: f64,
	filter_range_max_value: f64,
) -> ot::ConditionTable {
	ot::ConditionTable::Format1(ot::ConditionTableFormat1 {
		axis_index,
		filter_range_min_value,
		filter_range_max_value,
	})
}

/// `findFeatureVariationRecord`: the first record with the same conditions.
pub fn find_feature_variation_record<'a>(
	feature_variations: &'a mut ot::FeatureVariations,
	condition_table: &[ot::ConditionTable],
) -> Result<Option<&'a mut ot::FeatureVariationRecord>> {
	check_feature_variations_version(feature_variations)?;
	Ok(feature_variations
		.feature_variation_record
		.iter_mut()
		.find(|fvr| {
			let existing = fvr
				.condition_set
				.as_deref()
				.map_or(&[][..], |s| &s.condition_table[..]);
			existing.len() == condition_table.len()
				&& existing
					.iter()
					.zip(condition_table)
					.all(|(a, b)| a.as_deref() == Some(b))
		}))
}

/// `sortFeatureList`: sorts the feature list by feature tag, and remaps the
/// feature indices elsewhere. This is needed after the feature list has been
/// modified.
pub fn sort_feature_list<T: OtlTable + ?Sized>(table: &mut T) -> Result<()> {
	sort_feature_list_parts(&mut table.layout_parts()).map(|_| ())
}

/// `sortFeatureList`, returning the map from old to new feature indices.
fn sort_feature_list_parts(parts: &mut LayoutParts<'_>) -> Result<HashMap<usize, u16>> {
	let feature_list = parts
		.feature_list
		.as_deref_mut()
		.ok_or_else(|| attribute_error("FeatureRecord"))?;
	// Decorate, sort, undecorate, because we need an index remapping table.
	let mut decorated: Vec<(usize, ot::FeatureRecord)> =
		std::mem::take(&mut feature_list.feature_record)
			.into_iter()
			.enumerate()
			.collect();
	decorated.sort_by(|a, b| a.1.feature_tag.cmp(&b.1.feature_tag).then(a.0.cmp(&b.0)));
	let mut feature_remap = HashMap::with_capacity(decorated.len());
	for (new, (old, _)) in decorated.iter().enumerate() {
		feature_remap.insert(*old, to_u16(new, "feature index")?);
	}
	feature_list.feature_record = decorated.into_iter().map(|(_, f)| f).collect();

	// Remap the feature indices.
	remap_features_parts(parts, &feature_remap)?;
	Ok(feature_remap)
}

/// `remapFeatures`: remaps the feature indices of the scripts and of the
/// feature variations, from old to new index.
pub fn remap_features<T: OtlTable + ?Sized>(
	table: &mut T,
	feature_remap: &HashMap<usize, u16>,
) -> Result<()> {
	remap_features_parts(&mut table.layout_parts(), feature_remap)
}

fn remap_index(feature_remap: &HashMap<usize, u16>, index: u16) -> Result<u16> {
	feature_remap
		.get(&usize::from(index))
		.copied()
		.ok_or_else(|| Error::ttlib(format!("KeyError: {index}")))
}

fn remap_features_parts(
	parts: &mut LayoutParts<'_>,
	feature_remap: &HashMap<usize, u16>,
) -> Result<()> {
	let script_list = parts
		.script_list
		.as_deref_mut()
		.ok_or_else(|| attribute_error("ScriptRecord"))?;
	for script in &mut script_list.script_record {
		let script = script
			.script
			.as_deref_mut()
			.ok_or_else(|| attribute_error("DefaultLangSys"))?;
		if let Some(default) = script.default_lang_sys.as_deref_mut() {
			remap_lang_sys(default, feature_remap)?;
		}
		for record in &mut script.lang_sys_record {
			let lang_sys = record
				.lang_sys
				.as_deref_mut()
				.ok_or_else(|| attribute_error("ReqFeatureIndex"))?;
			remap_lang_sys(lang_sys, feature_remap)?;
		}
	}

	// UPSTREAM-BUG(B29): the `FeatureIndex` of LookupVariationRecords is not
	// remapped, so they refer to the wrong features after sorting.
	if let Some(variations) = parts.feature_variations.as_deref_mut() {
		for fvr in &mut variations.feature_variation_record {
			let Some(substitution) = fvr.feature_table_substitution.as_deref_mut() else {
				continue;
			};
			for ftsr in &mut substitution.substitution_record {
				ftsr.feature_index = remap_index(feature_remap, ftsr.feature_index)?;
			}
		}
	}
	Ok(())
}

/// `_remapLangSys`.
fn remap_lang_sys(lang_sys: &mut ot::LangSys, feature_remap: &HashMap<usize, u16>) -> Result<()> {
	if lang_sys.req_feature_index != 0xFFFF {
		lang_sys.req_feature_index = remap_index(feature_remap, lang_sys.req_feature_index)?;
	}
	lang_sys.feature_index = lang_sys
		.feature_index
		.iter()
		.map(|&i| remap_index(feature_remap, i))
		.collect::<Result<_>>()?;
	Ok(())
}
