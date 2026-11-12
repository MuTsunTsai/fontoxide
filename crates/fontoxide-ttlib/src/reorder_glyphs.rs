//! Reorder the glyphs of a font (`fontTools.ttLib.reorderGlyphs`).
//!
//! fontTools refers to glyphs by name, so most tables follow a new glyph order by
//! themselves; here every table's glyph IDs are remapped instead
//! ([`Font::remap_glyphs`]). What does not follow by itself is the same as in
//! fontTools: the coverage tables of `GDEF`, `GPOS`, `GSUB` and `MATH`, which keep
//! their order, are sorted again with the arrays parallel to them, and the
//! charstrings of `CFF ` and `CFF2` are put in the new order.

use std::collections::{BTreeSet, HashMap};

use crate::generated::ot;
use crate::ot::table::OtTable;
use crate::ot::types::Coverage;
use crate::tables::{Cff, Cff2, Gdef, Gpos, Gsub, Math};
use crate::{Error, Font, Result, Tag};

/// `repr(set)` of glyph names, sorted (the order of a Python set of strings
/// depends on the hash seed).
fn set_repr(names: &BTreeSet<&str>) -> String {
	if names.is_empty() {
		return "set()".into();
	}
	let items: Vec<String> = names
		.iter()
		.map(|n| fontoxide_misc::py::str_repr(n))
		.collect();
	format!("{{{}}}", items.join(", "))
}

/// Sorts a coverage by glyph ID, and the list parallel to it the same way
/// (`ReorderCoverage` with a `parallel_list_attr`).
fn sort_coverage<T>(coverage: &mut Option<Box<Coverage>>, parallel: &mut Vec<T>) -> Result<()> {
	// UPSTREAM-BUG(B48): fontTools fails with an AttributeError on a null coverage
	// offset (as the `ExtendedShapeCoverage` or `HorizGlyphCoverage` of a MATH
	// table built without extended shapes or horizontal variants); here it is
	// skipped.
	let Some(coverage) = coverage else {
		return Ok(());
	};
	if parallel.len() != coverage.glyphs.len() {
		return Err(Error::Assertion("Nothing makes sense".into()));
	}
	let mut pairs: Vec<(u32, T)> = coverage.glyphs.drain(..).zip(parallel.drain(..)).collect();
	pairs.sort_by_key(|p| p.0);
	for (g, item) in pairs {
		coverage.glyphs.push(g);
		parallel.push(item);
	}
	Ok(())
}

/// Sorts a coverage by glyph ID (`ReorderCoverage` without a parallel list).
fn sort_glyphs(coverage: &mut Option<Box<Coverage>>) -> Result<()> {
	// UPSTREAM-BUG(B48): see `sort_coverage`.
	if let Some(coverage) = coverage {
		coverage.glyphs.sort_unstable();
	}
	Ok(())
}

/// Sorts each coverage of a list (`ReorderCoverage` of an attribute that is a list).
fn sort_coverages(coverages: &mut [Option<Box<Coverage>>]) -> Result<()> {
	for coverage in coverages {
		sort_glyphs(coverage)?;
	}
	Ok(())
}

/// Sorts a list by the glyph of its items (`ReorderList`).
fn sort_pairs<T>(list: &mut [T], key: impl Fn(&T) -> u32) -> Result<()> {
	list.sort_by_key(key);
	Ok(())
}

/// Applies `_REORDER_RULES` to one table of the layout tables.
fn reorder(any: &mut dyn std::any::Any) -> Result<()> {
	macro_rules! rules {
		($($ty:ty => |$t:ident| $body:expr;)*) => {
			$(if let Some($t) = any.downcast_mut::<$ty>() {
				return $body;
			})*
		};
	}
	rules! {
		// GPOS
		ot::SinglePosFormat1 => |t| sort_glyphs(&mut t.coverage);
		ot::SinglePosFormat3 => |t| sort_glyphs(&mut t.coverage);
		ot::SinglePosFormat2 => |t| sort_coverage(&mut t.coverage, &mut t.value);
		ot::SinglePosFormat4 => |t| sort_coverage(&mut t.coverage, &mut t.value);
		ot::PairPosFormat1 => |t| sort_coverage(&mut t.coverage, &mut t.pair_set);
		ot::PairPosFormat3 => |t| sort_coverage(&mut t.coverage, &mut t.pair_set);
		ot::PairSet => |t| sort_pairs(&mut t.pair_value_record, |r| r.second_glyph);
		ot::PairSet2 => |t| sort_pairs(&mut t.pair_value_record, |r| r.second_glyph);
		ot::PairPosFormat2 => |t| sort_glyphs(&mut t.coverage);
		ot::PairPosFormat4 => |t| sort_glyphs(&mut t.coverage);
		ot::CursivePosFormat1 => |t| sort_coverage(&mut t.coverage, &mut t.entry_exit_record);
		ot::CursivePosFormat2 => |t| sort_coverage(&mut t.coverage, &mut t.entry_exit_record);
		ot::MarkBasePosFormat1 => |t| {
			sort_coverage(&mut t.mark_coverage, mark_records(&mut t.mark_array)?)?;
			sort_coverage(&mut t.base_coverage, &mut required(&mut t.base_array)?.base_record)
		};
		ot::MarkBasePosFormat2 => |t| {
			sort_coverage(&mut t.mark_coverage, mark_records2(&mut t.mark_array)?)?;
			sort_coverage(&mut t.base_coverage, &mut required(&mut t.base_array)?.base_record)
		};
		ot::MarkLigPosFormat1 => |t| {
			sort_coverage(&mut t.mark_coverage, mark_records(&mut t.mark_array)?)?;
			sort_coverage(
				&mut t.ligature_coverage,
				&mut required(&mut t.ligature_array)?.ligature_attach,
			)
		};
		ot::MarkLigPosFormat2 => |t| {
			sort_coverage(&mut t.mark_coverage, mark_records2(&mut t.mark_array)?)?;
			sort_coverage(
				&mut t.ligature_coverage,
				&mut required(&mut t.ligature_array)?.ligature_attach,
			)
		};
		ot::MarkMarkPosFormat1 => |t| {
			sort_coverage(&mut t.mark1_coverage, mark_records(&mut t.mark1_array)?)?;
			sort_coverage(&mut t.mark2_coverage, &mut required(&mut t.mark2_array)?.mark2_record)
		};
		ot::MarkMarkPosFormat2 => |t| {
			sort_coverage(&mut t.mark1_coverage, mark_records2(&mut t.mark1_array)?)?;
			sort_coverage(&mut t.mark2_coverage, &mut required(&mut t.mark2_array)?.mark2_record)
		};
		ot::ContextPosFormat1 => |t| sort_coverage(&mut t.coverage, &mut t.pos_rule_set);
		ot::ContextPosFormat2 => |t| sort_glyphs(&mut t.coverage);
		ot::ContextPosFormat5 => |t| sort_glyphs(&mut t.coverage);
		ot::ContextPosFormat3 => |t| sort_coverages(&mut t.coverage);
		ot::ContextPosFormat6 => |t| sort_coverages(&mut t.coverage);
		ot::ContextPosFormat4 => |t| sort_coverage(&mut t.coverage, &mut t.seq_rule_set);
		ot::ChainContextPosFormat1 => |t| sort_coverage(&mut t.coverage, &mut t.chain_pos_rule_set);
		ot::ChainContextPosFormat2 => |t| sort_glyphs(&mut t.coverage);
		ot::ChainContextPosFormat5 => |t| sort_glyphs(&mut t.coverage);
		ot::ChainContextPosFormat3 => |t| {
			sort_coverages(&mut t.backtrack_coverage)?;
			sort_coverages(&mut t.input_coverage)?;
			sort_coverages(&mut t.look_ahead_coverage)
		};
		ot::ChainContextPosFormat4 => |t| sort_coverage(&mut t.coverage, &mut t.chained_seq_rule_set);
		// GSUB
		ot::ContextSubstFormat1 => |t| sort_coverage(&mut t.coverage, &mut t.sub_rule_set);
		ot::ContextSubstFormat2 => |t| sort_glyphs(&mut t.coverage);
		ot::ContextSubstFormat5 => |t| sort_glyphs(&mut t.coverage);
		ot::ContextSubstFormat3 => |t| sort_coverages(&mut t.coverage);
		ot::ContextSubstFormat6 => |t| sort_coverages(&mut t.coverage);
		ot::ContextSubstFormat4 => |t| sort_coverage(&mut t.coverage, &mut t.seq_rule_set);
		ot::ChainContextSubstFormat1 => |t| sort_coverage(&mut t.coverage, &mut t.chain_sub_rule_set);
		ot::ChainContextSubstFormat2 => |t| sort_glyphs(&mut t.coverage);
		ot::ChainContextSubstFormat5 => |t| sort_glyphs(&mut t.coverage);
		ot::ChainContextSubstFormat3 => |t| {
			sort_coverages(&mut t.backtrack_coverage)?;
			sort_coverages(&mut t.input_coverage)?;
			sort_coverages(&mut t.look_ahead_coverage)
		};
		ot::ChainContextSubstFormat4 => |t| sort_coverage(&mut t.coverage, &mut t.chained_seq_rule_set);
		ot::ReverseChainSingleSubstFormat1 => |t| {
			sort_coverage(&mut t.coverage, &mut t.substitute)?;
			sort_coverages(&mut t.backtrack_coverage)?;
			sort_coverages(&mut t.look_ahead_coverage)
		};
		ot::ReverseChainSingleSubstFormat2 => |t| {
			sort_coverage(&mut t.coverage, &mut t.substitute)?;
			sort_coverages(&mut t.backtrack_coverage)?;
			sort_coverages(&mut t.look_ahead_coverage)
		};
		// GDEF
		ot::AttachList => |t| sort_coverage(&mut t.coverage, &mut t.attach_point);
		ot::LigCaretList => |t| sort_coverage(&mut t.coverage, &mut t.lig_glyph);
		ot::LigCaretList2 => |t| sort_coverage(&mut t.coverage, &mut t.lig_glyph);
		ot::MarkGlyphSetsDef => |t| sort_coverages(&mut t.coverage);
		// MATH
		ot::MathGlyphInfo => |t| sort_glyphs(&mut t.extended_shape_coverage);
		ot::MathItalicsCorrectionInfo => |t| sort_coverage(&mut t.coverage, &mut t.italics_correction);
		ot::MathTopAccentAttachment => |t| {
			sort_coverage(&mut t.top_accent_coverage, &mut t.top_accent_attachment)
		};
		ot::MathKernInfo => |t| sort_coverage(&mut t.math_kern_coverage, &mut t.math_kern_info_records);
		ot::MathVariants => |t| {
			sort_coverage(&mut t.vert_glyph_coverage, &mut t.vert_glyph_construction)?;
			sort_coverage(&mut t.horiz_glyph_coverage, &mut t.horiz_glyph_construction)
		};
	}
	Ok(())
}

/// An array that the rule follows: fontTools fails without it.
fn required<T>(table: &mut Option<Box<T>>) -> Result<&mut T> {
	table
		.as_deref_mut()
		.ok_or_else(|| Error::ttlib("AttributeError: 'NoneType' object has no attribute"))
}

fn mark_records(array: &mut Option<Box<ot::MarkArray>>) -> Result<&mut Vec<ot::MarkRecord>> {
	Ok(&mut required(array)?.mark_record)
}

fn mark_records2(array: &mut Option<Box<ot::MarkArray2>>) -> Result<&mut Vec<ot::MarkRecord2>> {
	Ok(&mut required(array)?.mark_record)
}

fn reorder_table<T: OtTable>(table: &mut T) -> Result<()> {
	table.visit_mut(&mut reorder)
}

/// Puts the charstrings of a CFF font set in the new glyph order and sets its
/// charset to it.
fn reorder_cff(cff: &mut crate::cff::font::CffFontSet, new_order: &[String]) -> Result<()> {
	let Some(top) = cff.top_dicts.first_mut() else {
		return Ok(());
	};
	if cff.major > 1 {
		// UPSTREAM-BUG(B46): fontTools reads the Top DICT of CFF2 only when it is first
		// accessed, here after the glyph order has changed, so the charstrings are
		// named after the new glyph order where they are: they do not move.
		if let Some(cs) = &mut top.char_strings {
			for (name, new) in cs.names.iter_mut().zip(new_order) {
				name.clone_from(new);
			}
		}
		top.charset = Some(new_order.to_vec());
		return Ok(());
	}
	if let Some(cs) = &mut top.char_strings {
		let index: HashMap<&str, usize> = cs
			.names
			.iter()
			.enumerate()
			.map(|(i, n)| (n.as_str(), i))
			.collect();
		let mut positions = Vec::with_capacity(new_order.len());
		for name in new_order {
			// fontTools stores `None` for a glyph without a charstring, which
			// cannot be compiled.
			let i = *index
				.get(name.as_str())
				.ok_or_else(|| Error::ttlib(format!("KeyError: {name:?}")))?;
			positions.push(i);
		}

		cs.privates = positions.iter().map(|&i| cs.privates[i]).collect();
		// The FDSelect read with the CharStrings INDEX goes with the charstrings
		// (fontTools looks it up by the charstring's index).
		if let Some(fd) = &cs.fd_select {
			let fd: Vec<Option<i64>> = positions
				.iter()
				.map(|&i| fd.get(i).copied().flatten())
				.collect();
			cs.fd_select = Some(fd);
		}
		cs.names = positions.iter().map(|&i| cs.names[i].clone()).collect();
		cs.items = positions.iter().map(|&i| cs.items[i].clone()).collect();
	}
	// UPSTREAM-BUG(B47): the Top DICT's FDSelect of a CID-keyed font keeps the old order,
	// so after reordering, the glyphs get the wrong font DICTs.
	top.charset = Some(new_order.to_vec());
	Ok(())
}

/// Reorders the glyphs of a font (`reorderGlyphs`): sets the glyph order to
/// `new_glyph_order`, which must have the same glyphs, and updates the tables
/// that do not follow by themselves.
///
/// All tables are decompiled first.
pub fn reorder_glyphs(font: &mut Font, new_glyph_order: Vec<String>) -> Result<()> {
	let old_order = font.glyph_order()?;
	if new_glyph_order.len() != old_order.len() {
		return Err(Error::value(format!(
			"New glyph order contains {} glyphs, but font has {} glyphs",
			new_glyph_order.len(),
			old_order.len()
		)));
	}
	let old: BTreeSet<&str> = old_order.names().iter().map(String::as_str).collect();
	let new: BTreeSet<&str> = new_glyph_order.iter().map(String::as_str).collect();
	if old != new {
		return Err(Error::value(format!(
			"New glyph order does not contain the same set of glyphs as the font:\n\
			 * only in new: {}\n\
			 * only in old: {}",
			set_repr(&new.difference(&old).copied().collect()),
			set_repr(&old.difference(&new).copied().collect()),
		)));
	}

	// Changing the order of glyphs requires that all tables that use glyph
	// indexes have been fully decompiled.
	font.ensure_decompiled()?;

	let new_ids: HashMap<&str, u32> = new_glyph_order
		.iter()
		.enumerate()
		.map(|(i, n)| (n.as_str(), i as u32))
		.collect();
	let map: Vec<u32> = old_order
		.names()
		.iter()
		.map(|n| new_ids.get(n.as_str()).copied().unwrap_or(0))
		.collect();
	// A glyph past the glyph order is named `glyph<ID>` in fontTools, which stays
	// the same glyph ID.
	let mut f = |gid: u32| -> Result<u32> { Ok(map.get(gid as usize).copied().unwrap_or(gid)) };
	font.remap_glyphs(&mut f, new_glyph_order.clone())?;

	for tag in [b"GDEF", b"GPOS", b"GSUB", b"MATH"] {
		let tag = Tag::new(tag);
		if !font.contains(tag) {
			continue;
		}
		let Some(table) = font.get_dyn(tag)? else {
			continue;
		};
		let any = table.as_any_mut();
		if let Some(t) = any.downcast_mut::<Gdef>() {
			reorder_table(&mut t.table)?;
		} else if let Some(t) = any.downcast_mut::<Gpos>() {
			reorder_table(&mut t.table)?;
		} else if let Some(t) = any.downcast_mut::<Gsub>() {
			reorder_table(&mut t.table)?;
		} else if let Some(t) = any.downcast_mut::<Math>() {
			reorder_table(&mut t.table)?;
		}
	}

	if let Some(t) = font.get::<Cff>()? {
		reorder_cff(&mut t.cff, &new_glyph_order)?;
	}
	if let Some(t) = font.get::<Cff2>()? {
		reorder_cff(&mut t.cff, &new_glyph_order)?;
	}
	Ok(())
}

impl Font {
	/// Reorders the glyphs of the font (`TTFont.reorderGlyphs`); see
	/// [`reorder_glyphs`].
	pub fn reorder_glyphs(&mut self, new_glyph_order: Vec<String>) -> Result<()> {
		reorder_glyphs(self, new_glyph_order)
	}
}
