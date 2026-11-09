//! Subsetting of `MATH` (`fontTools.subset`).

use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::Math;
use fontoxide_ttlib::{Error as TtError, Font};

use crate::layout::{cov_intersect, cov_mut, cov_subset, list_subset};
use crate::{GlyphSet, Result, Subsetter};

/// `MathGlyphConstruction.closure_glyphs`, for the constructions of the glyphs
/// of a coverage.
fn construction_closure(
	coverage: &ot::Coverage,
	constructions: &[Option<Box<ot::MathGlyphConstruction>>],
	glyphs: &GlyphSet,
	variants: &mut GlyphSet,
) -> Result<()> {
	for i in cov_intersect(coverage, glyphs) {
		let construction = constructions
			.get(i)
			.ok_or_else(|| TtError::ttlib("IndexError: list index out of range"))?
			.as_deref()
			.ok_or_else(|| {
				TtError::ttlib(
					"AttributeError: 'NoneType' object has no attribute 'closure_glyphs'",
				)
			})?;
		variants.extend(
			construction
				.math_glyph_variant_record
				.iter()
				.map(|v| v.variant_glyph),
		);
		if let Some(assembly) = &construction.glyph_assembly {
			variants.extend(assembly.part_records.iter().map(|p| p.glyph));
		}
	}
	Ok(())
}

/// `closure_glyphs`.
pub(crate) fn closure_glyphs(s: &mut Subsetter, font: &mut Font) -> Result<()> {
	let table = &font.require::<Math>()?.table;
	let Some(t) = &table.math_variants else {
		return Ok(());
	};
	let mut variants = GlyphSet::new();
	if let Some(coverage) = &t.vert_glyph_coverage {
		construction_closure(
			coverage,
			&t.vert_glyph_construction,
			&s.glyphs,
			&mut variants,
		)?;
	}
	if let Some(coverage) = &t.horiz_glyph_coverage {
		construction_closure(
			coverage,
			&t.horiz_glyph_construction,
			&s.glyphs,
			&mut variants,
		)?;
	}
	s.glyphs.extend(variants);
	Ok(())
}

/// `MathGlyphInfo.subset_glyphs`.
fn glyph_info_subset(t: &mut ot::MathGlyphInfo, glyphs: &GlyphSet) -> Result<()> {
	if let Some(t) = &mut t.math_italics_correction_info {
		let indices = cov_subset(cov_mut(&mut t.coverage)?, glyphs);
		t.italics_correction = list_subset(&t.italics_correction, &indices);
		t.italics_correction_count = Some(t.italics_correction.len() as u32);
	}
	if let Some(t) = &mut t.math_top_accent_attachment {
		let indices = cov_subset(cov_mut(&mut t.top_accent_coverage)?, glyphs);
		t.top_accent_attachment = list_subset(&t.top_accent_attachment, &indices);
		t.top_accent_attachment_count = Some(t.top_accent_attachment.len() as u32);
	}
	if let Some(t) = &mut t.math_kern_info {
		let indices = cov_subset(cov_mut(&mut t.math_kern_coverage)?, glyphs);
		t.math_kern_info_records = list_subset(&t.math_kern_info_records, &indices);
		t.math_kern_count = Some(t.math_kern_info_records.len() as u32);
	}
	if let Some(coverage) = &mut t.extended_shape_coverage {
		cov_subset(coverage, glyphs);
	}
	Ok(())
}

/// `subset_glyphs`. Its coverages use the glyphs after the `MATH` closure, but
/// variants are only kept for the glyphs before it.
pub(crate) fn subset_glyphs(s: &Subsetter, font: &mut Font) -> Result<bool> {
	let table = &mut font.require::<Math>()?.table;
	if let Some(t) = &mut table.math_glyph_info {
		glyph_info_subset(t, &s.glyphs_mathed)?;
	}
	if let Some(t) = &mut table.math_variants {
		let glyphs = &s.glyphs_wo_math_closure;
		if let Some(coverage) = &mut t.vert_glyph_coverage {
			let indices = cov_subset(coverage, glyphs);
			t.vert_glyph_construction = list_subset(&t.vert_glyph_construction, &indices);
			t.vert_glyph_count = Some(t.vert_glyph_construction.len() as u32);
		}
		if let Some(coverage) = &mut t.horiz_glyph_coverage {
			let indices = cov_subset(coverage, glyphs);
			t.horiz_glyph_construction = list_subset(&t.horiz_glyph_construction, &indices);
			t.horiz_glyph_count = Some(t.horiz_glyph_construction.len() as u32);
		}
	}
	Ok(true)
}
