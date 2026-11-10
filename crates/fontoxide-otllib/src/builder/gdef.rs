//! GDEF subtables.

use std::collections::HashSet;

use fontoxide_ttlib::ot::types::{self as ot, Coverage};
use indexmap::IndexMap;

use super::{MaybeExtended, build_coverage, sorted_glyphs};
use crate::{GlyphMap, Result};

/// Builds an attachment point list from glyph names to contour point indices
/// (`buildAttachList`); `None` for no glyphs.
pub fn build_attach_list(
	attach_points: &IndexMap<String, Vec<u16>>,
	glyph_map: &GlyphMap,
) -> Result<Option<ot::AttachList>> {
	if attach_points.is_empty() {
		return Ok(None);
	}
	let glyphs = sorted_glyphs(attach_points.keys().map(String::as_str), glyph_map)?;
	let attach_point: Vec<Option<Box<ot::AttachPoint>>> = glyphs
		.iter()
		.map(|&(_, g)| build_attach_point(&attach_points[g]).map(Box::new))
		.collect();
	Ok(Some(ot::AttachList {
		coverage: Some(Box::new(Coverage {
			glyphs: glyphs.iter().map(|&(id, _)| id).collect(),
		})),
		glyph_count: Some(attach_point.len() as u32),
		attach_point,
	}))
}

/// Builds an attachment point table from contour point indices, which are
/// sorted and deduplicated (`buildAttachPoint`); `None` for no points.
pub fn build_attach_point(points: &[u16]) -> Option<ot::AttachPoint> {
	if points.is_empty() {
		return None;
	}
	let mut point_index = points.to_vec();
	point_index.sort_unstable();
	point_index.dedup();
	Some(ot::AttachPoint {
		point_count: Some(point_index.len() as u32),
		point_index,
	})
}

/// A ligature caret position: a coordinate, possibly with a device table.
#[derive(Debug, Clone, PartialEq)]
pub enum CaretCoord {
	/// A coordinate (format 1).
	Coord(i16),
	/// A coordinate and a device table (format 3).
	WithDevice(i16, Option<ot::Device>),
}

impl CaretCoord {
	fn coordinate(&self) -> i16 {
		match self {
			CaretCoord::Coord(c) | CaretCoord::WithDevice(c, _) => *c,
		}
	}
}

/// Builds a caret value from a coordinate (`buildCaretValueForCoord`).
pub fn build_caret_value_for_coord(coord: &CaretCoord) -> ot::CaretValue {
	match coord {
		CaretCoord::Coord(c) => ot::CaretValue::Format1(ot::CaretValueFormat1 { coordinate: *c }),
		CaretCoord::WithDevice(c, d) => ot::CaretValue::Format3(ot::CaretValueFormat3 {
			coordinate: *c,
			device_table: d.clone().map(Box::new),
		}),
	}
}

/// Builds a caret value from a contour point (`buildCaretValueForPoint`).
pub fn build_caret_value_for_point(point: u16) -> ot::CaretValue {
	ot::CaretValue::Format2(ot::CaretValueFormat2 {
		caret_value_point: point,
	})
}

/// Builds a ligature caret list (`buildLigCaretList`) from ligature glyph names
/// to caret coordinates and to caret contour points; `None` if there are no
/// carets.
pub fn build_lig_caret_list(
	coords: &IndexMap<String, Vec<CaretCoord>>,
	points: &IndexMap<String, Vec<u16>>,
	glyph_map: &GlyphMap,
) -> Result<Option<MaybeExtended<ot::LigCaretList, ot::LigCaretList2>>> {
	let mut names: Vec<&str> = Vec::new();
	let mut seen = HashSet::new();
	for g in coords.keys().chain(points.keys()) {
		if seen.insert(g.as_str()) {
			names.push(g);
		}
	}
	let carets: IndexMap<&str, ot::LigGlyph> = names
		.into_iter()
		.filter_map(|g| {
			build_lig_glyph(
				coords.get(g).map(Vec::as_slice),
				points.get(g).map(Vec::as_slice),
			)
			.map(|c| (g, c))
		})
		.collect();
	if carets.is_empty() {
		return Ok(None);
	}
	let coverage = build_coverage(carets.keys().copied(), glyph_map)?;
	let mut lig_glyph = Vec::with_capacity(coverage.glyphs.len());
	for (_, g) in sorted_glyphs(carets.keys().copied(), glyph_map)? {
		lig_glyph.push(Some(Box::new(carets[g].clone())));
	}
	let lig_glyph_count = Some(lig_glyph.len() as u32);
	let coverage = Some(Box::new(coverage));
	Ok(Some(if glyph_map.has_extended_glyph_ids() {
		MaybeExtended::Extended(ot::LigCaretList2 {
			coverage,
			lig_glyph_count,
			lig_glyph,
		})
	} else {
		MaybeExtended::Normal(ot::LigCaretList {
			coverage,
			lig_glyph_count,
			lig_glyph,
		})
	}))
}

/// Builds the carets of a ligature glyph from coordinates and contour points,
/// each sorted (`buildLigGlyph`); `None` without carets.
pub fn build_lig_glyph(
	coords: Option<&[CaretCoord]>,
	points: Option<&[u16]>,
) -> Option<ot::LigGlyph> {
	let mut carets = Vec::new();
	if let Some(coords) = coords {
		let mut coords = coords.to_vec();
		coords.sort_by_key(CaretCoord::coordinate);
		carets.extend(coords.iter().map(build_caret_value_for_coord));
	}
	if let Some(points) = points {
		let mut points = points.to_vec();
		points.sort_unstable();
		carets.extend(points.into_iter().map(build_caret_value_for_point));
	}
	if carets.is_empty() {
		return None;
	}
	Some(ot::LigGlyph {
		caret_count: Some(carets.len() as u32),
		caret_value: carets.into_iter().map(|c| Some(Box::new(c))).collect(),
	})
}

/// Builds the mark glyph sets definition from sets of glyph names
/// (`buildMarkGlyphSetsDef`); `None` for no sets.
pub fn build_mark_glyph_sets_def(
	mark_sets: &[Vec<String>],
	glyph_map: &GlyphMap,
) -> Result<Option<ot::MarkGlyphSetsDef>> {
	if mark_sets.is_empty() {
		return Ok(None);
	}
	let coverage = mark_sets
		.iter()
		.map(|m| build_coverage(m.iter().map(String::as_str), glyph_map).map(|c| Some(Box::new(c))))
		.collect::<Result<Vec<_>>>()?;
	Ok(Some(ot::MarkGlyphSetsDef {
		mark_set_table_format: 1,
		mark_set_count: Some(coverage.len() as u32),
		coverage,
	}))
}
