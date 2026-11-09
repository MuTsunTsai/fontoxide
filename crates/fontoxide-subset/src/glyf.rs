//! Subsetting of `glyf`.

use fontoxide_ttlib::Font;
use fontoxide_ttlib::tables::Glyf;
use fontoxide_ttlib::tables::glyf::Glyph;

use crate::{GlyphSet, Result, Subsetter};

/// `glyf.closure_glyphs`: adds the components of composite glyphs, recursively.
pub(crate) fn closure_glyphs(s: &mut Subsetter, font: &mut Font) -> Result<()> {
	let glyf = font.require::<Glyf>()?;
	let mut decompose = s.glyphs.clone();
	while !decompose.is_empty() {
		let mut components = GlyphSet::new();
		for g in &decompose {
			if let Some(glyph) = glyf.glyphs.get(g) {
				components.extend(glyph.components.iter().map(|c| c.glyph));
			}
		}
		components.retain(|g| !s.glyphs.contains(g));
		s.glyphs.extend(&components);
		decompose = components;
	}
	Ok(())
}

/// `glyf.prune_pre_subset`: empties glyph 0 unless its outline is kept.
pub(crate) fn prune_pre_subset(s: &mut Subsetter, font: &mut Font) -> Result<bool> {
	if s.options.notdef_glyph && !s.options.notdef_outline {
		let glyf = font.require::<Glyf>()?;
		glyf.glyphs.insert(
			0,
			Glyph {
				data: Some(Vec::new()),
				..Default::default()
			},
		);
	}
	Ok(true)
}

/// `glyf.subset_glyphs`. The component glyph IDs are remapped with the other
/// tables at the end (`remapComponentsFast`).
pub(crate) fn subset_glyphs(s: &mut Subsetter, font: &mut Font) -> Result<bool> {
	let glyf = font.require::<Glyf>()?;
	glyf.glyphs.retain(|g, _| s.glyphs.contains(g));
	for &g in &s.glyphs_emptied {
		glyf.glyphs.insert(
			g,
			Glyph {
				data: Some(Vec::new()),
				..Default::default()
			},
		);
	}
	// An empty `glyf` is kept, else `loca` would not be subset.
	Ok(true)
}

/// `glyf.prune_post_subset`: trims padding, and hinting unless kept.
pub(crate) fn prune_post_subset(s: &mut Subsetter, font: &mut Font) -> Result<bool> {
	let glyf = font.require::<Glyf>()?;
	let extended = glyf.extended;
	for g in glyf.glyphs.values_mut() {
		g.trim_ext(!s.options.hinting, extended)?;
	}
	Ok(true)
}
