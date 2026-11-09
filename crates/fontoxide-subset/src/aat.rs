//! Subsetting of the AAT tables `ankr`, `bsln`, `lcar`, `opbd` and `prop`
//! (`fontTools.subset`).

use std::collections::BTreeMap;

use fontoxide_ttlib::ot::aat::AatLookup;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::otl::{Ankr, Bsln, Lcar, Opbd, Prop};
use fontoxide_ttlib::{Error as TtError, Font};

use crate::{GlyphSet, Result, Subsetter};

fn unknown_format(tag: &str, format: u16) -> crate::Error {
	TtError::Assertion(format!("unknown '{tag}' format {format}")).into()
}

/// `Counter(values).most_common(1)[0][0]`.
///
/// UPSTREAM-BUG(B20): fontTools counts the values of a set of glyph names, whose
/// order changes from one run to the next, and takes the first it saw among the
/// most common ones. The first in glyph order is taken here.
fn most_common(values: impl Iterator<Item = u16>) -> Option<u16> {
	let mut counts: BTreeMap<u16, (usize, usize)> = BTreeMap::new();
	for (i, value) in values.enumerate() {
		counts.entry(value).or_insert((0, i)).0 += 1;
	}
	counts
		.into_iter()
		.max_by_key(|&(_, (count, first))| (count, std::cmp::Reverse(first)))
		.map(|(value, _)| value)
}

/// The value of every glyph, with the most common one taken out and made the
/// default.
fn subset_default(glyphs: &GlyphSet, values: &AatLookup<u16>, default: &mut u16) -> AatLookup<u16> {
	let all: Vec<(u32, u16)> = glyphs
		.iter()
		.map(|&g| (g, values.get(&g).copied().unwrap_or(*default)))
		.collect();
	if let Some(common) = most_common(all.iter().map(|&(_, v)| v)) {
		*default = common;
	}
	all.into_iter().filter(|&(_, v)| v != *default).collect()
}

/// `subset_glyphs` of `ankr`.
pub(crate) fn ankr_subset_glyphs(s: &Subsetter, font: &mut Font) -> Result<bool> {
	match &mut font.require::<Ankr>()?.table.anchor_points {
		ot::AnchorPoints::Format0(t) => {
			t.anchors.retain(|g, _| s.glyphs.contains(g));
			Ok(!t.anchors.is_empty())
		}
		ot::AnchorPoints::Unknown(format) => Err(unknown_format("ankr", *format)),
	}
}

/// `closure_glyphs` of `bsln`.
pub(crate) fn bsln_closure_glyphs(s: &mut Subsetter, font: &mut Font) -> Result<()> {
	match &font.require::<Bsln>()?.table.baseline {
		ot::Baseline::Format2(t) => {
			s.glyphs.insert(t.standard_glyph);
		}
		ot::Baseline::Format3(t) => {
			s.glyphs.insert(t.standard_glyph);
		}
		_ => {}
	}
	Ok(())
}

/// `subset_glyphs` of `bsln`.
pub(crate) fn bsln_subset_glyphs(s: &Subsetter, font: &mut Font) -> Result<bool> {
	let baseline = &mut font.require::<Bsln>()?.table.baseline;
	match baseline {
		ot::Baseline::Format1(t) => {
			let values = subset_default(&s.glyphs, &t.baseline_values, &mut t.default_baseline);
			if values.is_empty() {
				*baseline = ot::Baseline::Format0(ot::BaselineFormat0 {
					default_baseline: t.default_baseline,
					delta: std::mem::take(&mut t.delta),
				});
			} else {
				t.baseline_values = values;
			}
		}
		ot::Baseline::Format3(t) => {
			let values = subset_default(&s.glyphs, &t.baseline_values, &mut t.default_baseline);
			if values.is_empty() {
				*baseline = ot::Baseline::Format2(ot::BaselineFormat2 {
					default_baseline: t.default_baseline,
					standard_glyph: t.standard_glyph,
					control_point: std::mem::take(&mut t.control_point),
				});
			} else {
				t.baseline_values = values;
			}
		}
		_ => {}
	}
	Ok(true)
}

/// `subset_glyphs` of `lcar`.
pub(crate) fn lcar_subset_glyphs(s: &Subsetter, font: &mut Font) -> Result<bool> {
	match &mut font.require::<Lcar>()?.table.ligature_carets {
		ot::LigatureCarets::Format0(t) => {
			t.carets.retain(|g, _| s.glyphs.contains(g));
			Ok(!t.carets.is_empty())
		}
		ot::LigatureCarets::Format1(t) => {
			t.carets.retain(|g, _| s.glyphs.contains(g));
			Ok(!t.carets.is_empty())
		}
		ot::LigatureCarets::Unknown(format) => Err(unknown_format("lcar", *format)),
	}
}

/// `subset_glyphs` of `opbd`.
pub(crate) fn opbd_subset_glyphs(s: &Subsetter, font: &mut Font) -> Result<bool> {
	match &mut font.require::<Opbd>()?.table.optical_bounds {
		ot::OpticalBounds::Format0(t) => {
			t.optical_bounds_deltas.retain(|g, _| s.glyphs.contains(g));
			Ok(!t.optical_bounds_deltas.is_empty())
		}
		ot::OpticalBounds::Format1(t) => {
			t.optical_bounds_points.retain(|g, _| s.glyphs.contains(g));
			Ok(!t.optical_bounds_points.is_empty())
		}
		ot::OpticalBounds::Unknown(format) => Err(unknown_format("opbd", *format)),
	}
}

/// `subset_glyphs` of `prop`.
pub(crate) fn prop_subset_glyphs(s: &Subsetter, font: &mut Font) -> Result<bool> {
	let properties = &mut font.require::<Prop>()?.table.glyph_properties;
	match properties {
		ot::GlyphProperties::Format0(t) => Ok(t.default_properties != 0),
		ot::GlyphProperties::Format1(t) => {
			let values = subset_default(&s.glyphs, &t.properties, &mut t.default_properties);
			if values.is_empty() {
				let default_properties = t.default_properties;
				*properties =
					ot::GlyphProperties::Format0(ot::GlyphPropertiesFormat0 { default_properties });
				return Ok(default_properties != 0);
			}
			t.properties = values;
			Ok(true)
		}
		ot::GlyphProperties::Unknown(format) => Err(unknown_format("prop", *format)),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn most_common_prefers_the_first_seen() {
		assert_eq!(most_common([].into_iter()), None);
		assert_eq!(most_common([5, 3, 3].into_iter()), Some(3));
		assert_eq!(most_common([5, 3, 3, 5].into_iter()), Some(5));
		assert_eq!(most_common([3, 5, 5, 3].into_iter()), Some(3));
	}
}
