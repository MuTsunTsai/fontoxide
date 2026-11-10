//! Building OpenType Layout tables (`fontTools.otlLib.builder`).
//!
//! Like fontTools, the builders take glyph names; the tables they produce refer
//! to glyphs by ID, so the functions that fontTools runs without a glyph map
//! (`buildSingleSubstSubtable` and such) take one here too.
//!
//! Python dicts become [`IndexMap`]s where their order matters, and sets of glyph
//! names become sorted lists (fontTools sorts sets wherever their order would
//! matter).

mod chain;
mod class_def;
mod gdef;
mod gpos;
mod gsub;
mod lookups;
mod math;
mod stat;

use fontoxide_ttlib::ot::types::{self as ot, Coverage, LookupSubTable};

pub use class_def::ClassDefBuilder;
pub use gdef::{
	CaretCoord, build_attach_list, build_attach_point, build_caret_value_for_coord,
	build_caret_value_for_point, build_lig_caret_list, build_lig_glyph, build_mark_glyph_sets_def,
};
pub use gpos::{
	ValuePair, build_anchor, build_base_array, build_base_record, build_component_record,
	build_cursive_pos_subtable, build_device, build_ligature_array, build_ligature_attach,
	build_mark_array, build_mark_base_pos_subtable, build_mark_lig_pos_subtable, build_mark_record,
	build_mark2_record, build_pair_pos_classes_subtable, build_pair_pos_glyphs,
	build_pair_pos_glyphs_subtable, build_single_pos, build_single_pos_subtable, build_value,
	get_single_pos_value_key, value_record_with_format,
};
pub use gsub::{
	build_alternate_subst_subtable, build_ligature_subst_subtable, build_multiple_subst_subtable,
	build_single_subst_subtable, ligature_sort_key,
};
pub use lookups::*;
pub use math::{GlyphPart, MathKernSide, MathKerns, MathTableArgs, build_math_table};
pub use stat::{
	AXIS_VALUE_NEGATIVE_INFINITY, AXIS_VALUE_POSITIVE_INFINITY, StatAxis, StatAxisValue,
	StatAxisValueKind, StatLocation, StatName, StatNameRecord, build_stat_table,
};

use crate::{Error, GlyphMap, Result};

/// `LOOKUP_FLAG_RIGHT_TO_LEFT`.
pub const LOOKUP_FLAG_RIGHT_TO_LEFT: u16 = 0x0001;
/// `LOOKUP_FLAG_IGNORE_BASE_GLYPHS`.
pub const LOOKUP_FLAG_IGNORE_BASE_GLYPHS: u16 = 0x0002;
/// `LOOKUP_FLAG_IGNORE_LIGATURES`.
pub const LOOKUP_FLAG_IGNORE_LIGATURES: u16 = 0x0004;
/// `LOOKUP_FLAG_IGNORE_MARKS`.
pub const LOOKUP_FLAG_IGNORE_MARKS: u16 = 0x0008;
/// `LOOKUP_FLAG_USE_MARK_FILTERING_SET`.
pub const LOOKUP_FLAG_USE_MARK_FILTERING_SET: u16 = 0x0010;

/// A table or a record in its normal format, or in the format for fonts with
/// more than 65536 glyphs (`BaseArray` or `BaseArray2`, and such). fontTools
/// picks the class from the size of the glyph map.
#[derive(Debug, Clone, PartialEq)]
pub enum MaybeExtended<N, E> {
	/// The normal format.
	Normal(N),
	/// The extended format (the classes whose names end in `2`).
	Extended(E),
}

impl<N, E> MaybeExtended<N, E> {
	/// The normal format, if it is that.
	pub fn normal(self) -> Option<N> {
		match self {
			MaybeExtended::Normal(n) => Some(n),
			MaybeExtended::Extended(_) => None,
		}
	}

	/// The extended format, if it is that.
	pub fn extended(self) -> Option<E> {
		match self {
			MaybeExtended::Extended(e) => Some(e),
			MaybeExtended::Normal(_) => None,
		}
	}
}

/// The glyphs of `glyphs`, without duplicates and sorted by glyph ID, with their
/// IDs (`sorted(set(glyphs), key=glyphMap.__getitem__)`).
pub(crate) fn sorted_glyphs<'a, I>(glyphs: I, glyph_map: &GlyphMap) -> Result<Vec<(u32, &'a str)>>
where
	I: IntoIterator<Item = &'a str>,
{
	let mut seen = std::collections::HashSet::new();
	let mut out = Vec::new();
	for g in glyphs {
		if seen.insert(g) {
			out.push((glyph_map.id(g)?, g));
		}
	}
	out.sort_by_key(|&(id, _)| id);
	Ok(out)
}

/// Builds a coverage table from glyph names (`buildCoverage`). Duplicates are
/// removed, and the glyphs are sorted by glyph ID; no glyphs give an empty
/// table.
pub fn build_coverage<'a, I>(glyphs: I, glyph_map: &GlyphMap) -> Result<Coverage>
where
	I: IntoIterator<Item = &'a str>,
{
	let sorted = sorted_glyphs(glyphs, glyph_map).map_err(|e| match e {
		Error::Value(m) => Error::Value(format!(
			"Could not find glyph {} in font",
			m.strip_prefix("KeyError: ").unwrap_or(&m)
		)),
		e => e,
	})?;
	Ok(Coverage {
		glyphs: sorted.into_iter().map(|(id, _)| id).collect(),
	})
}

/// The lookup type of a subtable (`LookupType` of its class).
pub fn subtable_lookup_type(subtable: &LookupSubTable) -> Option<u16> {
	use LookupSubTable as L;
	Some(match subtable {
		L::SingleSubst(_) | L::SinglePos(_) => 1,
		L::MultipleSubst(_) | L::PairPos(_) => 2,
		L::AlternateSubst(_) | L::CursivePos(_) => 3,
		L::LigatureSubst(_) | L::MarkBasePos(_) => 4,
		L::ContextSubst(_) | L::MarkLigPos(_) => 5,
		L::ChainContextSubst(_) | L::MarkMarkPos(_) => 6,
		L::ExtensionSubst(_) | L::ContextPos(_) => 7,
		L::ReverseChainSingleSubst(_) | L::ChainContextPos(_) => 8,
		L::ExtensionPos(_) => 9,
		_ => return None,
	})
}

/// The table a lookup is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayoutTable {
	/// `GSUB`.
	Gsub,
	/// `GPOS`.
	Gpos,
}

impl LayoutTable {
	/// The table tag.
	pub fn tag(self) -> fontoxide_misc::Tag {
		match self {
			LayoutTable::Gsub => fontoxide_misc::Tag::new(b"GSUB"),
			LayoutTable::Gpos => fontoxide_misc::Tag::new(b"GPOS"),
		}
	}
}

impl std::fmt::Display for LayoutTable {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(match self {
			LayoutTable::Gsub => "GSUB",
			LayoutTable::Gpos => "GPOS",
		})
	}
}

/// Turns subtables into a lookup (`buildLookup`); `None` without subtables.
///
/// `mark_filter_set` sets `LOOKUP_FLAG_USE_MARK_FILTERING_SET`; `extension`
/// wraps the subtables in Extension subtables of `table`.
pub fn build_lookup(
	subtables: Vec<Option<LookupSubTable>>,
	flags: u16,
	mark_filter_set: Option<u16>,
	table: Option<LayoutTable>,
	extension: bool,
) -> Result<Option<ot::Lookup>> {
	let mut subtables: Vec<LookupSubTable> = subtables.into_iter().flatten().collect();
	if subtables.is_empty() {
		return Ok(None);
	}
	let types: Vec<Option<u16>> = subtables.iter().map(subtable_lookup_type).collect();
	if types.iter().any(|t| *t != types[0]) {
		let repr: Vec<String> = types
			.iter()
			.map(|t| t.map_or_else(|| "None".to_owned(), |t| t.to_string()))
			.collect();
		return Err(Error::Assertion(format!(
			"all subtables must have the same LookupType; got [{}]",
			repr.join(", ")
		)));
	}
	let lookup_type = if extension {
		let table = table.ok_or_else(|| Error::Assertion("table in ('GPOS', 'GSUB')".into()))?;
		subtables = subtables
			.into_iter()
			.map(|st| {
				let ext_type = subtable_lookup_type(&st).unwrap_or(0);
				let ext_sub_table = Some(Box::new(st));
				match table {
					LayoutTable::Gsub => LookupSubTable::ExtensionSubst(
						ot::ExtensionSubst::Format1(ot::ExtensionSubstFormat1 {
							extension_lookup_type: ext_type,
							ext_sub_table,
							dont_share: false,
						}),
					),
					LayoutTable::Gpos => LookupSubTable::ExtensionPos(ot::ExtensionPos::Format1(
						ot::ExtensionPosFormat1 {
							extension_lookup_type: ext_type,
							ext_sub_table,
							dont_share: false,
						},
					)),
				}
			})
			.collect();
		match table {
			LayoutTable::Gsub => 7,
			LayoutTable::Gpos => 9,
		}
	} else {
		types[0].ok_or_else(|| Error::Value("subtable has no LookupType".into()))?
	};
	let mut lookup = ot::Lookup {
		lookup_type,
		lookup_flag: flags,
		sub_table_count: Some(subtables.len() as u32),
		sub_table: subtables.into_iter().map(|s| Some(Box::new(s))).collect(),
		mark_filtering_set: 0,
	};
	match mark_filter_set {
		Some(set) => {
			lookup.lookup_flag |= LOOKUP_FLAG_USE_MARK_FILTERING_SET;
			lookup.mark_filtering_set = set;
		}
		None => {
			if lookup.lookup_flag & LOOKUP_FLAG_USE_MARK_FILTERING_SET != 0 {
				return Err(Error::Assertion(format!(
					"if markFilterSet is None, flags must not set LOOKUP_FLAG_USE_MARK_FILTERING_SET; flags=0x{flags:04x}"
				)));
			}
		}
	}
	Ok(Some(lookup))
}
