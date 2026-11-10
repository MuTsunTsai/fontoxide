//! GSUB subtables.

use std::collections::BTreeMap;

use fontoxide_ttlib::ot::types::{
	self as ot, AlternateSubst, LigatureSubst, MultipleSubst, SingleSubst,
};
use indexmap::IndexMap;

use crate::{GlyphMap, Result};

fn ids(glyphs: &[String], glyph_map: &GlyphMap) -> Result<Vec<u32>> {
	glyphs.iter().map(|g| glyph_map.id(g)).collect()
}

/// Builds a single substitution (GSUB1) subtable from input to output glyph
/// names (`buildSingleSubstSubtable`); `None` for an empty mapping.
pub fn build_single_subst_subtable(
	mapping: &IndexMap<String, String>,
	glyph_map: &GlyphMap,
) -> Result<Option<SingleSubst>> {
	if mapping.is_empty() {
		return Ok(None);
	}
	let mut out = BTreeMap::new();
	for (k, v) in mapping {
		out.insert(glyph_map.id(k)?, glyph_map.id(v)?);
	}
	Ok(Some(SingleSubst {
		mapping: out,
		dont_share: false,
	}))
}

/// Builds a multiple substitution (GSUB2) subtable from input glyph names to
/// lists of output glyph names (`buildMultipleSubstSubtable`); `None` for an
/// empty mapping.
pub fn build_multiple_subst_subtable(
	mapping: &IndexMap<String, Vec<String>>,
	glyph_map: &GlyphMap,
) -> Result<Option<MultipleSubst>> {
	if mapping.is_empty() {
		return Ok(None);
	}
	let mut out = BTreeMap::new();
	for (k, v) in mapping {
		out.insert(glyph_map.id(k)?, ids(v, glyph_map)?);
	}
	Ok(Some(MultipleSubst {
		mapping: out,
		dont_share: false,
	}))
}

/// Builds an alternate substitution (GSUB3) subtable from input glyph names to
/// lists of alternate glyph names (`buildAlternateSubstSubtable`); `None` for an
/// empty mapping.
pub fn build_alternate_subst_subtable(
	mapping: &IndexMap<String, Vec<String>>,
	glyph_map: &GlyphMap,
) -> Result<Option<AlternateSubst>> {
	if mapping.is_empty() {
		return Ok(None);
	}
	let mut out = BTreeMap::new();
	for (k, v) in mapping {
		out.insert(glyph_map.id(k)?, ids(v, glyph_map)?);
	}
	Ok(Some(AlternateSubst {
		alternates: out,
		dont_share: false,
	}))
}

/// The key that orders the ligatures of a ligature set: longest first, and in
/// the original order for the same length (`LigatureSubst._getLigatureSortKey`).
pub fn ligature_sort_key(components: &[String]) -> i64 {
	-(components.len() as i64)
}

/// Builds a ligature substitution (GSUB4) subtable from sequences of component
/// glyph names to ligature glyph names (`buildLigatureSubstSubtable`); `None`
/// for an empty mapping.
pub fn build_ligature_subst_subtable(
	mapping: &IndexMap<Vec<String>, String>,
	glyph_map: &GlyphMap,
) -> Result<Option<LigatureSubst>> {
	if mapping.is_empty() {
		return Ok(None);
	}
	let mut keys: Vec<&Vec<String>> = mapping.keys().collect();
	keys.sort_by_key(|k| ligature_sort_key(k));
	let mut ligatures: BTreeMap<u32, Vec<ot::Ligature>> = BTreeMap::new();
	for components in keys {
		let Some(first) = components.first() else {
			return Err(crate::Error::Value(
				"IndexError: tuple index out of range".into(),
			));
		};
		let ligature = ot::Ligature {
			lig_glyph: glyph_map.id(&mapping[components])?,
			comp_count: Some(components.len() as u32),
			component: ids(&components[1..], glyph_map)?,
		};
		ligatures
			.entry(glyph_map.id(first)?)
			.or_default()
			.push(ligature);
	}
	Ok(Some(LigatureSubst {
		ligatures,
		dont_share: false,
	}))
}
