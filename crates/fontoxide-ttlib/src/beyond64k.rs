//! Converts between Open Font Format beyond-64k companion tables (a port of
//! `fontTools.ttLib.beyond64k`).
//!
//! The beyond-64k specification adds uppercase companion tables and extended layout
//! formats that can address glyph IDs above 65535. [`upper_tables`] and [`lower_tables`]
//! convert a font in place between the compact lowercase table family and the extended
//! uppercase table family. By default they convert every known table present in the font
//! and validate that the result does not mix lowercase and uppercase companion tables.

use std::collections::{BTreeMap, BTreeSet};

use crate::ot::table::OtTable;
use crate::ot::types as ot;
use crate::tables::cmap::{Cmap, CmapSubtable, UvsDict};
use crate::tables::colr::Colr;
use crate::tables::{
	Base, Dmap, Gdef, Glyf, Gpos, Gsub, Gvar, Hhea, Hmtx, Jstf, Loca, Maxp, Post, Vhea, Vmtx, Vorg,
};
use crate::{Error, Font, Result, Tag};

/// `_TABLE_PAIRS`: the compact and extended tags of the companion tables.
const TABLE_PAIRS: [(&[u8; 4], &[u8; 4]); 8] = [
	(b"glyf", b"GLYF"),
	(b"loca", b"LOCA"),
	(b"maxp", b"MAXP"),
	(b"hhea", b"HHEA"),
	(b"hmtx", b"HMTX"),
	(b"vhea", b"VHEA"),
	(b"vmtx", b"VMTX"),
	(b"gvar", b"GVAR"),
];
/// The tables that keep their tag.
const SAME_TAGS: [&[u8; 4]; 7] = [
	b"BASE", b"COLR", b"GDEF", b"GPOS", b"GSUB", b"JSTF", b"VORG",
];

/// A conversion of a table's type (`replacement.__dict__.update(subtable.__dict__)`).
trait Convert<To> {
	fn convert(self) -> To;
}

impl<A: Convert<B>, B> Convert<Vec<B>> for Vec<A> {
	fn convert(self) -> Vec<B> {
		self.into_iter().map(Convert::convert).collect()
	}
}

impl<A: Convert<B>, B> Convert<Option<Box<B>>> for Option<Box<A>> {
	fn convert(self) -> Option<Box<B>> {
		self.map(|v| Box::new((*v).convert()))
	}
}

/// Conversions in both directions between two structures: the fields that are moved, and
/// those of types that are converted in turn.
macro_rules! both {
	($a:ident, $b:ident; $($fa:ident => $fb:ident),* $(,)?; $($ca:ident => $cb:ident),* $(,)?) => {
		impl Convert<ot::$b> for ot::$a {
			fn convert(self) -> ot::$b {
				ot::$b {
					$($fb: self.$fa,)*
					$($cb: self.$ca.convert(),)*
				}
			}
		}
		impl Convert<ot::$a> for ot::$b {
			fn convert(self) -> ot::$a {
				ot::$a {
					$($fa: self.$fb,)*
					$($ca: self.$cb.convert(),)*
				}
			}
		}
	};
}

// `_EXPLICIT_LAYOUT_FORMATS`.
both!(BaseCoordFormat2, BaseCoordFormat4;
	coordinate => coordinate, reference_glyph => reference_glyph, base_coord_point => base_coord_point;);
both!(SinglePosFormat1, SinglePosFormat3;
	coverage => coverage, value_format => value_format, value => value, dont_share => dont_share;);
both!(SinglePosFormat2, SinglePosFormat4;
	coverage => coverage, value_format => value_format, value_count => value_count, value => value,
	dont_share => dont_share;);
both!(PairValueRecord, PairValue2;
	second_glyph => second_glyph, value1 => value1, value2 => value2;);
both!(PairSet, PairSet2;
	pair_value_count => pair_value_count;
	pair_value_record => pair_value_record);
both!(PairPosFormat1, PairPosFormat3;
	coverage => coverage, value_format1 => value_format1, value_format2 => value_format2,
	pair_set_count => pair_set_count, dont_share => dont_share;
	pair_set => pair_set);
both!(PairPosFormat2, PairPosFormat4;
	coverage => coverage, value_format1 => value_format1, value_format2 => value_format2,
	class_def1 => class_def1, class_def2 => class_def2, class1_count => class1_count,
	class2_count => class2_count, class1_record => class1_record, dont_share => dont_share;);
both!(EntryExitRecord, EntryExit2;
	entry_anchor => entry_anchor, exit_anchor => exit_anchor;);
both!(CursivePosFormat1, CursivePosFormat2;
	coverage => coverage, entry_exit_count => entry_exit_count, dont_share => dont_share;
	entry_exit_record => entry_exit_record);
both!(MarkRecord, MarkRecord2; class => class, mark_anchor => mark_anchor;);
both!(MarkArray, MarkArray2; mark_count => mark_count; mark_record => mark_record);
both!(BaseRecord, BaseRecord2; base_anchor => base_anchor;);
both!(BaseArray, BaseArray2; base_count => base_count; base_record => base_record);
both!(MarkBasePosFormat1, MarkBasePosFormat2;
	mark_coverage => mark_coverage, base_coverage => base_coverage, class_count => class_count,
	dont_share => dont_share;
	mark_array => mark_array, base_array => base_array);
both!(ComponentRecord, ComponentRecord2; ligature_anchor => ligature_anchor;);
both!(LigatureAttach, LigatureAttach2;
	component_count => component_count;
	component_record => component_record);
both!(LigatureArray, LigatureArray2;
	ligature_count => ligature_count;
	ligature_attach => ligature_attach);
both!(MarkLigPosFormat1, MarkLigPosFormat2;
	mark_coverage => mark_coverage, ligature_coverage => ligature_coverage,
	class_count => class_count, dont_share => dont_share;
	mark_array => mark_array, ligature_array => ligature_array);
both!(Mark2Record, Mark2Record2; mark2_anchor => mark2_anchor;);
both!(Mark2Array, Mark2Array2; mark2_count => mark2_count; mark2_record => mark2_record);
both!(MarkMarkPosFormat1, MarkMarkPosFormat2;
	mark1_coverage => mark1_coverage, mark2_coverage => mark2_coverage, class_count => class_count,
	dont_share => dont_share;
	mark1_array => mark1_array, mark2_array => mark2_array);
both!(ReverseChainSingleSubstFormat1, ReverseChainSingleSubstFormat2;
	coverage => coverage, backtrack_glyph_count => backtrack_glyph_count,
	backtrack_coverage => backtrack_coverage, look_ahead_glyph_count => look_ahead_glyph_count,
	look_ahead_coverage => look_ahead_coverage, glyph_count => glyph_count,
	substitute => substitute, dont_share => dont_share;);

// `_CONTEXTUAL_LAYOUT_FORMATS`.
both!(SubstLookupRecord, SeqLookup;
	sequence_index => sequence_index, lookup_list_index => lookup_list_index;);
both!(PosLookupRecord, SeqLookup;
	sequence_index => sequence_index, lookup_list_index => lookup_list_index;);

macro_rules! contextual {
	(
		$count:ident, $record:ident,
		$rule:ident, $rule_set:ident, $rule_count:ident, $rule_field:ident,
		$class_rule:ident, $class_set:ident, $class_count:ident, $class_field:ident,
		$format1:ident, $format4:ident, $set_count:ident, $set:ident,
		$format2:ident, $format5:ident, $class_set_count:ident, $class_set_field:ident,
		$format3:ident, $format6:ident
	) => {
		both!($rule, SeqRule2;
			glyph_count => glyph_count, $count => seq_lookup_count, input => input_sequence;
			$record => seq_lookup_record);
		both!($rule_set, SeqRuleSet2; $rule_count => seq_rule_count; $rule_field => seq_rule);
		both!($class_rule, ClassSeqRule;
			glyph_count => glyph_count, $count => seq_lookup_count, class => input_sequence;
			$record => seq_lookup_record);
		both!($class_set, ClassSeqRuleSet2;
			$class_count => class_seq_rule_count;
			$class_field => class_seq_rule);
		both!($format1, $format4;
			coverage => coverage, $set_count => seq_rule_set_count, dont_share => dont_share;
			$set => seq_rule_set);
		both!($format2, $format5;
			coverage => coverage, class_def => class_def,
			$class_set_count => class_seq_rule_set_count, dont_share => dont_share;
			$class_set_field => class_seq_rule_set);
		both!($format3, $format6;
			glyph_count => glyph_count, $count => seq_lookup_count, coverage => coverage,
			dont_share => dont_share;
			$record => seq_lookup_record);
	};
}

contextual!(
	subst_count,
	subst_lookup_record,
	SubRule,
	SubRuleSet,
	sub_rule_count,
	sub_rule,
	SubClassRule,
	SubClassSet,
	sub_class_rule_count,
	sub_class_rule,
	ContextSubstFormat1,
	ContextSubstFormat4,
	sub_rule_set_count,
	sub_rule_set,
	ContextSubstFormat2,
	ContextSubstFormat5,
	sub_class_set_count,
	sub_class_set,
	ContextSubstFormat3,
	ContextSubstFormat6
);
contextual!(
	pos_count,
	pos_lookup_record,
	PosRule,
	PosRuleSet,
	pos_rule_count,
	pos_rule,
	PosClassRule,
	PosClassSet,
	pos_class_rule_count,
	pos_class_rule,
	ContextPosFormat1,
	ContextPosFormat4,
	pos_rule_set_count,
	pos_rule_set,
	ContextPosFormat2,
	ContextPosFormat5,
	pos_class_set_count,
	pos_class_set,
	ContextPosFormat3,
	ContextPosFormat6
);

macro_rules! chaining {
	(
		$count:ident, $record:ident,
		$rule:ident, $rule_set:ident, $rule_count:ident, $rule_field:ident,
		$class_rule:ident, $class_set:ident, $class_count:ident, $class_field:ident,
		$format1:ident, $format4:ident, $set_count:ident, $set:ident,
		$format2:ident, $format5:ident, $class_set_count:ident, $class_set_field:ident
	) => {
		both!($rule, ChainedSeqRule2;
			backtrack_glyph_count => backtrack_glyph_count, backtrack => backtrack_sequence,
			input_glyph_count => input_glyph_count, input => input_sequence,
			look_ahead_glyph_count => look_ahead_glyph_count, look_ahead => look_ahead_sequence,
			$count => seq_lookup_count;
			$record => seq_lookup_record);
		both!($rule_set, ChainedSeqRuleSet2;
			$rule_count => chained_seq_rule_count;
			$rule_field => chained_seq_rule);
		both!($class_rule, ChainedClassSeqRule;
			backtrack_glyph_count => backtrack_glyph_count, backtrack => backtrack_sequence,
			input_glyph_count => input_glyph_count, input => input_sequence,
			look_ahead_glyph_count => look_ahead_glyph_count, look_ahead => look_ahead_sequence,
			$count => seq_lookup_count;
			$record => seq_lookup_record);
		both!($class_set, ChainedClassSeqRuleSet2;
			$class_count => chained_class_seq_rule_count;
			$class_field => chained_class_seq_rule);
		both!($format1, $format4;
			coverage => coverage, $set_count => chained_seq_rule_set_count,
			dont_share => dont_share;
			$set => chained_seq_rule_set);
		both!($format2, $format5;
			coverage => coverage, backtrack_class_def => backtrack_class_def,
			input_class_def => input_class_def, look_ahead_class_def => look_ahead_class_def,
			$class_set_count => chained_class_seq_rule_set_count, dont_share => dont_share;
			$class_set_field => chained_class_seq_rule_set);
	};
}

chaining!(
	subst_count,
	subst_lookup_record,
	ChainSubRule,
	ChainSubRuleSet,
	chain_sub_rule_count,
	chain_sub_rule,
	ChainSubClassRule,
	ChainSubClassSet,
	chain_sub_class_rule_count,
	chain_sub_class_rule,
	ChainContextSubstFormat1,
	ChainContextSubstFormat4,
	chain_sub_rule_set_count,
	chain_sub_rule_set,
	ChainContextSubstFormat2,
	ChainContextSubstFormat5,
	chain_sub_class_set_count,
	chain_sub_class_set
);
chaining!(
	pos_count,
	pos_lookup_record,
	ChainPosRule,
	ChainPosRuleSet,
	chain_pos_rule_count,
	chain_pos_rule,
	ChainPosClassRule,
	ChainPosClassSet,
	chain_pos_class_rule_count,
	chain_pos_class_rule,
	ChainContextPosFormat1,
	ChainContextPosFormat4,
	chain_pos_rule_set_count,
	chain_pos_rule_set,
	ChainContextPosFormat2,
	ChainContextPosFormat5,
	chain_pos_class_set_count,
	chain_pos_class_set
);

// The other table objects.
both!(LigCaretList, LigCaretList2;
	coverage => coverage, lig_glyph_count => lig_glyph_count, lig_glyph => lig_glyph;);
both!(LookupList, LookupList2; lookup_count => lookup_count, lookup => lookup;);
both!(ExtenderGlyph, ExtenderGlyph2; glyph_count => glyph_count, extender_glyph => extender_glyph;);
both!(JstfScript, JstfScript2;
	def_jstf_lang_sys => def_jstf_lang_sys, jstf_lang_sys_count => jstf_lang_sys_count,
	jstf_lang_sys_record => jstf_lang_sys_record;
	extender_glyph => extender_glyph);
both!(JstfScriptRecord, JstfScriptRecord2;
	jstf_script_tag => jstf_script_tag;
	jstf_script => jstf_script);

/// Replaces a value with a conversion of it; `Unknown` stands in meanwhile.
macro_rules! switch_format {
	($value:expr, $enum:ident, $unknown:expr, { $($from:ident => $to:ident),* $(,)? }) => {{
		let value: &mut ot::$enum = $value;
		match std::mem::replace(value, ot::$enum::Unknown($unknown)) {
			$(ot::$enum::$from(v) => {
				*value = ot::$enum::$to(v.convert());
				true
			})*
			other => {
				*value = other;
				false
			}
		}
	}};
}

/// `_convert_layout_formats`, for one object of a table.
fn convert_object(any: &mut dyn std::any::Any, extended: bool, changed: &mut bool) {
	macro_rules! dispatch {
		($enum:ident, $(($compact:ident, $wide:ident)),* $(,)?) => {
			if let Some(value) = any.downcast_mut::<ot::$enum>() {
				*changed |= if extended {
					switch_format!(value, $enum, 0, { $($compact => $wide,)* })
				} else {
					switch_format!(value, $enum, 0, { $($wide => $compact,)* })
				};
				return;
			}
		};
	}
	dispatch!(
		ContextSubst,
		(Format1, Format4),
		(Format2, Format5),
		(Format3, Format6)
	);
	dispatch!(
		ContextPos,
		(Format1, Format4),
		(Format2, Format5),
		(Format3, Format6)
	);
	dispatch!(ChainContextSubst, (Format1, Format4), (Format2, Format5));
	dispatch!(ChainContextPos, (Format1, Format4), (Format2, Format5));
	dispatch!(BaseCoord, (Format2, Format4));
	dispatch!(SinglePos, (Format1, Format3), (Format2, Format4));
	dispatch!(PairPos, (Format1, Format3), (Format2, Format4));
	dispatch!(CursivePos, (Format1, Format2));
	dispatch!(MarkBasePos, (Format1, Format2));
	dispatch!(MarkLigPos, (Format1, Format2));
	dispatch!(MarkMarkPos, (Format1, Format2));
	dispatch!(ReverseChainSingleSubst, (Format1, Format2));
}

/// `_convert_layout_formats`: whether anything changed. `extended` selects the direction.
pub fn convert_layout_formats<T: OtTable>(table: &mut T, extended: bool) -> Result<bool> {
	let mut changed = false;
	table.visit_mut(&mut |any| {
		convert_object(any, extended, &mut changed);
		Ok(())
	})?;
	Ok(changed)
}

/// `_move_fields`: moves a field of a table to its extended or compact counterpart.
fn move_field<T>(
	compact: &mut Option<T>,
	wide: &mut Option<T>,
	names: (&str, &str),
	extended: bool,
	overwrite: bool,
) -> Result<()> {
	let (source, destination, source_name, destination_name) = if extended {
		(compact, wide, names.0, names.1)
	} else {
		(wide, compact, names.1, names.0)
	};
	if source.is_some() && destination.is_some() && !overwrite {
		return Err(Error::value(format!(
			"Both '{source_name}' and '{destination_name}' exist; set overwrite=True to replace the destination"
		)));
	}
	if source.is_some() {
		*destination = source.take();
	}
	Ok(())
}

/// `_move_fields` and `_convert_table_object`: the fields have different types.
fn move_converted<A, B>(
	compact: &mut Option<Box<A>>,
	wide: &mut Option<Box<B>>,
	names: (&str, &str),
	extended: bool,
	overwrite: bool,
) -> Result<()>
where
	A: Convert<B>,
	B: Convert<A>,
{
	let (source_some, destination_some, source_name, destination_name) = if extended {
		(compact.is_some(), wide.is_some(), names.0, names.1)
	} else {
		(wide.is_some(), compact.is_some(), names.1, names.0)
	};
	if source_some && destination_some && !overwrite {
		return Err(Error::value(format!(
			"Both '{source_name}' and '{destination_name}' exist; set overwrite=True to replace the destination"
		)));
	}
	if extended {
		if compact.is_some() {
			*wide = compact.take().convert();
		}
	} else if wide.is_some() {
		*compact = wide.take().convert();
	}
	Ok(())
}

fn upper_gdef(table: &mut ot::GDEF, overwrite: bool) -> Result<()> {
	convert_gdef_fields(table, true, overwrite)?;
	table.version = table.version.max(0x0001_0004);
	convert_layout_formats(table, true)?;
	Ok(())
}

fn lower_gdef(table: &mut ot::GDEF, overwrite: bool) -> Result<()> {
	convert_gdef_fields(table, false, overwrite)?;
	table.version = if table.var_store.is_some() {
		0x0001_0003
	} else if table.mark_glyph_sets_def.is_some() {
		0x0001_0002
	} else {
		0x0001_0000
	};
	convert_layout_formats(table, false)?;
	Ok(())
}

fn convert_gdef_fields(table: &mut ot::GDEF, extended: bool, overwrite: bool) -> Result<()> {
	move_field(
		&mut table.glyph_class_def,
		&mut table.glyph_class_def2,
		("GlyphClassDef", "GlyphClassDef2"),
		extended,
		overwrite,
	)?;
	move_field(
		&mut table.attach_list,
		&mut table.attach_list2,
		("AttachList", "AttachList2"),
		extended,
		overwrite,
	)?;
	move_converted(
		&mut table.lig_caret_list,
		&mut table.lig_caret_list2,
		("LigCaretList", "LigCaretList2"),
		extended,
		overwrite,
	)?;
	move_field(
		&mut table.mark_attach_class_def,
		&mut table.mark_attach_class_def2,
		("MarkAttachClassDef", "MarkAttachClassDef2"),
		extended,
		overwrite,
	)?;
	move_field(
		&mut table.mark_glyph_sets_def,
		&mut table.mark_glyph_sets_def2,
		("MarkGlyphSetsDef", "MarkGlyphSetsDef2"),
		extended,
		overwrite,
	)
}

macro_rules! layout_header {
	($upper:ident, $lower:ident, $ty:ident) => {
		fn $upper(table: &mut ot::$ty, overwrite: bool) -> Result<()> {
			move_field(
				&mut table.script_list,
				&mut table.script_list2,
				("ScriptList", "ScriptList2"),
				true,
				overwrite,
			)?;
			move_field(
				&mut table.feature_list,
				&mut table.feature_list2,
				("FeatureList", "FeatureList2"),
				true,
				overwrite,
			)?;
			move_converted(
				&mut table.lookup_list,
				&mut table.lookup_list2,
				("LookupList", "LookupList2"),
				true,
				overwrite,
			)?;
			table.version = table.version.max(0x0001_0002);
			convert_layout_formats(table, true)?;
			Ok(())
		}

		fn $lower(table: &mut ot::$ty, overwrite: bool) -> Result<()> {
			move_field(
				&mut table.script_list,
				&mut table.script_list2,
				("ScriptList", "ScriptList2"),
				false,
				overwrite,
			)?;
			move_field(
				&mut table.feature_list,
				&mut table.feature_list2,
				("FeatureList", "FeatureList2"),
				false,
				overwrite,
			)?;
			move_converted(
				&mut table.lookup_list,
				&mut table.lookup_list2,
				("LookupList", "LookupList2"),
				false,
				overwrite,
			)?;
			table.version = if table.feature_variations.is_some() {
				0x0001_0001
			} else {
				0x0001_0000
			};
			convert_layout_formats(table, false)?;
			Ok(())
		}
	};
}

layout_header!(upper_gsub, lower_gsub, GSUB);
layout_header!(upper_gpos, lower_gpos, GPOS);

fn upper_jstf(table: &mut ot::JSTF) {
	table.jstf_script_record2 = std::mem::take(&mut table.jstf_script_record).convert();
	table.jstf_script_count2 = table.jstf_script_count.take().unwrap_or(0) as u16;
	table.version = table.version.max(0x0001_0001);
}

fn lower_jstf(table: &mut ot::JSTF) {
	table.jstf_script_record = std::mem::take(&mut table.jstf_script_record2).convert();
	table.jstf_script_count = Some(u32::from(std::mem::take(&mut table.jstf_script_count2)));
	table.version = 0x0001_0000;
}

/// `_iter_colr_paints` with `_upper_colr` or `_lower_colr`.
fn convert_colr(colr: &mut Colr, extended: bool) -> Result<()> {
	let Some(table) = colr.table.as_mut() else {
		return Ok(());
	};
	table.visit_mut(&mut |any| {
		if let Some(paint) = any.downcast_mut::<ot::Paint>() {
			match std::mem::replace(paint, ot::Paint::Unknown(0)) {
				ot::Paint::Format10(p) if extended => {
					*paint = ot::Paint::Format33(ot::PaintFormat33 {
						paint: p.paint,
						glyph: p.glyph,
					});
				}
				ot::Paint::Format33(p) if !extended => {
					*paint = ot::Paint::Format10(ot::PaintFormat10 {
						paint: p.paint,
						glyph: p.glyph,
					});
				}
				other => *paint = other,
			}
		}
		Ok(())
	})
}

/// `_has_extended_layout_formats`: whether any lookup subtable has an extended format.
/// This doesn't descend through every rule and value record.
fn has_extended_layout_formats(lookups: Option<&ot::LookupList>) -> bool {
	fn is_extended(st: &ot::LookupSubTable) -> bool {
		use ot::LookupSubTable as St;
		match st {
			St::ExtensionPos(ot::ExtensionPos::Format1(t)) => {
				t.ext_sub_table.as_deref().is_some_and(is_extended)
			}
			St::ExtensionSubst(ot::ExtensionSubst::Format1(t)) => {
				t.ext_sub_table.as_deref().is_some_and(is_extended)
			}
			St::ContextSubst(t) => matches!(
				t,
				ot::ContextSubst::Format4(_)
					| ot::ContextSubst::Format5(_)
					| ot::ContextSubst::Format6(_)
			),
			St::ContextPos(t) => matches!(
				t,
				ot::ContextPos::Format4(_)
					| ot::ContextPos::Format5(_)
					| ot::ContextPos::Format6(_)
			),
			St::ChainContextSubst(t) => matches!(
				t,
				ot::ChainContextSubst::Format4(_) | ot::ChainContextSubst::Format5(_)
			),
			St::ChainContextPos(t) => matches!(
				t,
				ot::ChainContextPos::Format4(_) | ot::ChainContextPos::Format5(_)
			),
			St::SinglePos(t) => {
				matches!(t, ot::SinglePos::Format3(_) | ot::SinglePos::Format4(_))
			}
			St::PairPos(t) => matches!(t, ot::PairPos::Format3(_) | ot::PairPos::Format4(_)),
			St::CursivePos(t) => matches!(t, ot::CursivePos::Format2(_)),
			St::MarkBasePos(t) => matches!(t, ot::MarkBasePos::Format2(_)),
			St::MarkLigPos(t) => matches!(t, ot::MarkLigPos::Format2(_)),
			St::MarkMarkPos(t) => matches!(t, ot::MarkMarkPos::Format2(_)),
			St::ReverseChainSingleSubst(t) => {
				matches!(t, ot::ReverseChainSingleSubst::Format2(_))
			}
			_ => false,
		}
	}
	lookups.is_some_and(|list| {
		list.lookup
			.iter()
			.flatten()
			.any(|lookup| lookup.sub_table.iter().flatten().any(|st| is_extended(st)))
	})
}

/// How to restore a table made compact by [`compact_layout_tables`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Restore {
	Header,
	Formats,
}

/// Compact in-memory layouts of `GDEF`, `GSUB` and `GPOS`, for algorithms that dispatch on
/// them (the first half of `_compact_layout_tables`). No compact serialization occurs
/// here: glyph IDs and counts stay intact.
fn lower_layout_tables(font: &mut Font) -> Result<Vec<(Tag, Restore)>> {
	let mut restore = Vec::new();
	if let Some(gdef) = font.get::<Gdef>()?
		&& gdef.version >= 0x0001_0004
	{
		lower_gdef(&mut gdef.table, true)?;
		restore.push((Tag::new(b"GDEF"), Restore::Header));
	}
	macro_rules! layout {
		($ty:ident, $tag:literal, $lower:ident) => {
			if let Some(t) = font.get::<$ty>()? {
				if t.version >= 0x0001_0002 {
					$lower(&mut t.table, true)?;
					restore.push((Tag::new($tag), Restore::Header));
				} else if has_extended_layout_formats(t.table.lookup_list.as_deref()) {
					convert_layout_formats(&mut t.table, false)?;
					restore.push((Tag::new($tag), Restore::Formats));
				}
			}
		};
	}
	layout!(Gsub, b"GSUB", lower_gsub);
	layout!(Gpos, b"GPOS", lower_gpos);
	Ok(restore)
}

/// The second half of `_compact_layout_tables`.
fn restore_layout_tables(font: &mut Font, restore: &[(Tag, Restore)]) -> Result<()> {
	for &(tag, how) in restore {
		match &tag.0 {
			b"GDEF" => {
				if let Some(t) = font.get::<Gdef>()? {
					upper_gdef(&mut t.table, true)?;
				}
			}
			b"GSUB" => {
				if let Some(t) = font.get::<Gsub>()? {
					match how {
						Restore::Header => upper_gsub(&mut t.table, true)?,
						Restore::Formats => {
							convert_layout_formats(&mut t.table, true)?;
						}
					}
				}
			}
			_ => {
				if let Some(t) = font.get::<Gpos>()? {
					match how {
						Restore::Header => upper_gpos(&mut t.table, true)?,
						Restore::Formats => {
							convert_layout_formats(&mut t.table, true)?;
						}
					}
				}
			}
		}
	}
	Ok(())
}

/// `_compact_layout_tables`: runs `f` with the layout tables in their compact in-memory
/// forms, and restores them afterwards.
pub fn with_compact_layout_tables<R, E: From<Error>>(
	font: &mut Font,
	f: impl FnOnce(&mut Font) -> std::result::Result<R, E>,
) -> std::result::Result<R, E> {
	let restore = lower_layout_tables(font)?;
	let result = f(font);
	// As with `finally`, restoring happens in any case; the first error wins.
	let restored = restore_layout_tables(font, &restore);
	let result = result?;
	restored?;
	Ok(result)
}

/// `_lower_glyf`: validation has rejected active or hinted cubic bits. Explicit lowering
/// may discard the remaining inert bits on unhinted on-curve points.
fn lower_glyf(glyf: &mut Glyf) {
	for glyph in glyf.glyphs.values_mut() {
		glyph.data = None;
		for flag in &mut glyph.flags {
			*flag &= !crate::tables::glyf::FLAG_CUBIC;
		}
	}
}

fn table_identity(tag: &str) -> Option<[u8; 4]> {
	let bytes: [u8; 4] = tag.as_bytes().try_into().ok()?;
	for (lower, upper) in TABLE_PAIRS {
		if bytes == *lower || bytes == *upper {
			return Some(*lower);
		}
	}
	SAME_TAGS.iter().find(|t| ***t == bytes).map(|t| **t)
}

/// `_normalize_tables`.
fn normalize_tables(tables: Option<&[&str]>) -> Result<BTreeSet<[u8; 4]>> {
	match tables {
		None => Ok(TABLE_PAIRS
			.iter()
			.map(|p| *p.0)
			.chain(SAME_TAGS.iter().map(|t| **t))
			.collect()),
		Some(tables) => tables
			.iter()
			.map(|tag| {
				table_identity(tag).ok_or_else(|| {
					Error::value(format!(
						"Unsupported beyond-64k table: {}",
						fontoxide_misc::py::str_repr(tag)
					))
				})
			})
			.collect(),
	}
}

fn tag_text(tags: &[Tag]) -> String {
	let mut names: Vec<String> = tags.iter().map(|t| t.as_string()).collect();
	names.sort();
	names.join(", ")
}

/// The options of [`upper_tables`] and [`lower_tables`].
#[derive(Debug, Clone)]
pub struct ConvertOptions<'a> {
	/// The tables to convert, in either spelling; by default all (`tables`).
	pub tables: Option<&'a [&'a str]>,
	/// Reject conversions that leave the font with a mixed lowercase/uppercase companion
	/// table family (`validate`).
	pub validate: bool,
	/// Replace an existing destination table (`overwrite`).
	pub overwrite: bool,
	/// Ignore requested source tables that are not present, unless their destination is
	/// (`ignore_missing`).
	pub ignore_missing: bool,
}

impl Default for ConvertOptions<'_> {
	fn default() -> Self {
		ConvertOptions {
			tables: None,
			validate: true,
			overwrite: false,
			ignore_missing: true,
		}
	}
}

/// `_validate_family`.
fn mixed_family(tags: &BTreeSet<Tag>) -> Option<(Vec<Tag>, Vec<Tag>)> {
	let lower: Vec<Tag> = TABLE_PAIRS
		.iter()
		.map(|p| Tag::new(p.0))
		.filter(|t| tags.contains(t))
		.collect();
	let upper: Vec<Tag> = TABLE_PAIRS
		.iter()
		.map(|p| Tag::new(p.1))
		.filter(|t| tags.contains(t))
		.collect();
	(!lower.is_empty() && !upper.is_empty()).then_some((lower, upper))
}

/// `_validate_lowering`.
fn validate_lowering(font: &mut Font, tables: &BTreeSet<[u8; 4]>) -> Result<()> {
	use crate::tables::glyf::{FLAG_CUBIC, FLAG_ON_CURVE};
	if tables.contains(b"glyf") && font.contains(Tag::new(b"GLYF")) {
		let order = font.glyph_order()?;
		let glyf = font.require_locked::<Glyf>(true)?;
		for gid in 0..order.len() as u32 {
			let name = order.name(gid);
			let glyph = glyf
				.glyphs
				.get_mut(&gid)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {name:?}")))?;
			glyph.data = None;
			if glyph.is_composite() {
				for component in &glyph.components {
					if component.glyph > 0xFFFF {
						return Err(Error::value(format!(
							"GLYF component {} does not fit in glyf",
							fontoxide_misc::py::str_repr(&order.name(component.glyph))
						)));
					}
				}
			} else {
				let cubic: Vec<u8> = glyph
					.flags
					.iter()
					.copied()
					.filter(|f| f & FLAG_CUBIC != 0)
					.collect();
				// Hinting can flip an on-curve point and activate its cubic bit.
				let hinted = glyph.program.as_ref().is_some_and(|p| !p.is_empty());
				if !cubic.is_empty() && (cubic.iter().any(|f| f & FLAG_ON_CURVE == 0) || hinted) {
					return Err(Error::value(format!(
						"GLYF glyph {} has cubic outlines or hinted CUBIC flags",
						fontoxide_misc::py::str_repr(&name)
					)));
				}
			}
		}
	}
	if tables.contains(b"maxp")
		&& font.contains(Tag::new(b"MAXP"))
		&& font.require_locked::<Maxp>(true)?.num_glyphs > 0xFFFF
	{
		return Err(Error::value("MAXP.numGlyphs does not fit in maxp"));
	}
	if tables.contains(b"hhea")
		&& font.contains(Tag::new(b"HHEA"))
		&& font.require_locked::<Hhea>(true)?.number_of_h_metrics > 0xFFFF
	{
		return Err(Error::value("HHEA.numberOfHMetrics does not fit in hhea"));
	}
	if tables.contains(b"vhea")
		&& font.contains(Tag::new(b"VHEA"))
		&& font.require_locked::<Vhea>(true)?.number_of_v_metrics > 0xFFFF
	{
		return Err(Error::value("VHEA.numberOfVMetrics does not fit in vhea"));
	}
	if tables.contains(b"VORG")
		&& let Some(vorg) = font.get::<Vorg>()?
	{
		if vorg.v_origin_records.len() > 0xFFFF {
			return Err(Error::value("VORG record count does not fit in version 1"));
		}
		if let Some(&gid) = vorg.v_origin_records.keys().find(|g| **g > 0xFFFF) {
			let order = font.glyph_order()?;
			return Err(Error::value(format!(
				"VORG glyph {} does not fit in version 1",
				fontoxide_misc::py::str_repr(&order.name(gid))
			)));
		}
	}
	if tables.contains(b"COLR") && font.contains(Tag::new(b"COLR")) {
		let mut high = None;
		if let Some(table) = font.require::<Colr>()?.table.as_mut() {
			table.visit_mut(&mut |any| {
				if let Some(ot::Paint::Format33(p)) = any.downcast_ref::<ot::Paint>()
					&& p.glyph > 0xFFFF
					&& high.is_none()
				{
					high = Some(p.glyph);
				}
				Ok(())
			})?;
		}
		if let Some(gid) = high {
			let order = font.glyph_order()?;
			return Err(Error::value(format!(
				"COLR PaintGlyph2 glyph {} does not fit in PaintGlyph",
				fontoxide_misc::py::str_repr(&order.name(gid))
			)));
		}
	}
	validate_cmap_uvs_lowering(font)
}

/// `_uvs_has_high_glyph_id`.
fn uvs_has_high_glyph_id(subtable: &CmapSubtable) -> bool {
	subtable
		.uvs_dict
		.iter()
		.flat_map(|d| d.values())
		.flatten()
		.any(|(_, gid)| gid.is_some_and(|g| g > 0xFFFF))
}

/// The character map tables of a font (`_character_map_tables`).
fn for_each_character_map(
	font: &mut Font,
	mut f: impl FnMut(&str, &mut Cmap) -> Result<()>,
) -> Result<()> {
	if let Some(cmap) = font.get::<Cmap>()? {
		f("cmap", cmap)?;
	}
	if let Some(dmap) = font.get::<Dmap>()? {
		f("DMAP", &mut dmap.0)?;
	}
	Ok(())
}

/// `_validate_cmap_uvs_lowering`.
fn validate_cmap_uvs_lowering(font: &mut Font) -> Result<()> {
	for_each_character_map(font, |tag, map| {
		for subtable in &map.tables {
			if subtable.format == 15 && uvs_has_high_glyph_id(subtable) {
				return Err(Error::value(format!(
					"{tag} format 15 glyph IDs do not fit in format 14"
				)));
			}
		}
		Ok(())
	})
}

fn subtable_key(subtable: &CmapSubtable) -> (u16, u16, Option<u32>) {
	(
		subtable.platform_id,
		subtable.plat_enc_id,
		subtable.language,
	)
}

/// `_merge_uvs_dict`.
fn merge_uvs_dict(destination: &mut UvsDict, source: &UvsDict) {
	for (selector, entries) in source {
		let mut merged: BTreeMap<u32, Option<u32>> = destination
			.get(selector)
			.into_iter()
			.flatten()
			.copied()
			.collect();
		merged.extend(entries.iter().copied());
		destination.insert(*selector, merged.into_iter().collect());
	}
}

/// `_copy_uvs_subtable`.
fn copy_uvs_subtable(format: u16, source: &CmapSubtable) -> CmapSubtable {
	let mut subtable = CmapSubtable::new(format);
	subtable.platform_id = source.platform_id;
	subtable.plat_enc_id = source.plat_enc_id;
	subtable.language = source.language;
	subtable.uvs_dict = source.uvs_dict.clone();
	subtable
}

/// `_upper_beyond64k_cmap_uvs` and `_lower_beyond64k_cmap_uvs`: moves the UVS subtables
/// with the given format to those with the other one, which can come before or after.
fn convert_cmap_uvs(map: &mut Cmap, tag: &str, from: u16, to: u16) -> Result<()> {
	let mut arena = std::mem::take(&mut map.tables);
	let mut by_key: BTreeMap<(u16, u16, Option<u32>), usize> = BTreeMap::new();
	for (i, subtable) in arena.iter().enumerate() {
		if subtable.format == to {
			by_key.insert(subtable_key(subtable), i);
		}
	}
	let mut output = Vec::with_capacity(arena.len());
	for i in 0..arena.len() {
		let subtable = &arena[i];
		// From format 14, only those that need it move; to format 14, all must.
		let high = uvs_has_high_glyph_id(subtable);
		if subtable.format != from || (from == 14 && !high) {
			output.push(i);
			continue;
		}
		if from == 15 && high {
			map.tables = arena;
			return Err(Error::value(format!(
				"{tag} format 15 glyph IDs do not fit in format 14"
			)));
		}
		let key = subtable_key(subtable);
		match by_key.get(&key).copied() {
			Some(target) => {
				let uvs = arena[i].uvs_dict.clone().unwrap_or_default();
				merge_uvs_dict(
					arena[target].uvs_dict.get_or_insert_with(UvsDict::new),
					&uvs,
				);
			}
			None => {
				let copy = copy_uvs_subtable(to, subtable);
				by_key.insert(key, arena.len());
				output.push(arena.len());
				arena.push(copy);
			}
		}
	}
	let mut slots: Vec<Option<CmapSubtable>> = arena.into_iter().map(Some).collect();
	map.tables = output.into_iter().filter_map(|i| slots[i].take()).collect();
	Ok(())
}

/// `_drop_beyond64k_cmap_format4`.
fn drop_cmap_format4(map: &mut Cmap) {
	let is_bmp = |s: &CmapSubtable| s.format == 4 && s.is_unicode() && !s.is_symbol();
	if !map.tables.iter().any(is_bmp) {
		return;
	}
	if !map.tables.iter().any(|s| s.format == 12) {
		let mut cmap12 = crate::tables::cmap::CharMap::new();
		for subtable in map.tables.iter().filter(|s| is_bmp(s)) {
			cmap12.extend(subtable.cmap.iter().map(|(k, v)| (*k, *v)));
		}
		if !cmap12.is_empty() {
			let mut subtable = CmapSubtable::new(12);
			subtable.platform_id = 3;
			subtable.plat_enc_id = 10;
			subtable.language = Some(0);
			subtable.cmap = std::sync::Arc::new(cmap12);
			map.tables.push(subtable);
		}
	}
	map.tables.retain(|s| !is_bmp(s));
}

/// Moves a table to its other tag (the end of `_convert_table`).
fn move_table<T>(font: &mut Font, source_tag: Tag, set: impl FnOnce(&mut T)) -> Result<()>
where
	T: crate::table::TableType + Clone,
{
	let table = font
		.get_at::<T>(source_tag)?
		.ok_or(Error::TableNotFound(source_tag))?;
	let mut moved = table.clone();
	set(&mut moved);
	font.insert(Box::new(moved));
	font.remove(source_tag);
	Ok(())
}

/// `_convert_tables`, for one direction.
fn convert_tables(font: &mut Font, options: &ConvertOptions<'_>, extended: bool) -> Result<()> {
	let tables = normalize_tables(options.tables)?;
	if !extended {
		validate_lowering(font, &tables)?;
	}

	// (source tag, destination tag, identity), in the order of the conversions.
	let pairs = TABLE_PAIRS.iter().map(|(lower, upper)| {
		if extended {
			(Tag::new(lower), Tag::new(upper), **lower)
		} else {
			(Tag::new(upper), Tag::new(lower), **lower)
		}
	});
	let same = SAME_TAGS.iter().map(|t| (Tag::new(t), Tag::new(t), **t));
	let conversions: Vec<(Tag, Tag, [u8; 4])> = pairs
		.chain(same)
		.filter(|c| tables.contains(&c.2))
		.collect();

	let mut pending = Vec::new();
	for &(source, destination, identity) in &conversions {
		let source_exists = font.contains(source);
		let destination_exists = font.contains(destination);
		if source != destination && source_exists && destination_exists && !options.overwrite {
			return Err(Error::value(format!(
				"Both '{source}' and '{destination}' exist; set overwrite=True to replace the destination"
			)));
		}
		if !source_exists {
			if destination_exists || options.ignore_missing {
				continue;
			}
			return Err(Error::ttlib(format!("KeyError: '{source}'")));
		}
		pending.push((source, destination, identity));
	}

	if options.validate {
		let mut result: BTreeSet<Tag> = font.tags().into_iter().collect();
		for &(source, destination, _) in &pending {
			result.remove(&source);
			result.insert(destination);
		}
		if let Some((lower, upper)) = mixed_family(&result) {
			return Err(Error::value(format!(
				"Conversion would create a mixed beyond-64k table family: lowercase {}; uppercase {}",
				tag_text(&lower),
				tag_text(&upper)
			)));
		}
	}

	// All the sources are read before any of them changes.
	for &(source, _, _) in &pending {
		font.load(source)?;
	}
	for &(source, _, identity) in &pending {
		match &identity {
			b"glyf" => move_table::<Glyf>(font, source, |t| {
				if !extended {
					lower_glyf(t);
				}
				t.extended = extended;
			})?,
			b"loca" => move_table::<Loca>(font, source, |t| t.extended = extended)?,
			b"maxp" => move_table::<Maxp>(font, source, |t| t.extended = extended)?,
			b"hhea" => move_table::<Hhea>(font, source, |t| t.extended = extended)?,
			b"hmtx" => move_table::<Hmtx>(font, source, |t| t.extended = extended)?,
			b"vhea" => move_table::<Vhea>(font, source, |t| t.extended = extended)?,
			b"vmtx" => move_table::<Vmtx>(font, source, |t| t.extended = extended)?,
			b"gvar" => move_table::<Gvar>(font, source, |t| t.extended = extended)?,
			b"BASE" => {
				convert_layout_formats(&mut font.require::<Base>()?.table, extended)?;
			}
			b"COLR" => convert_colr(font.require::<Colr>()?, extended)?,
			b"GDEF" => {
				let table = &mut font.require::<Gdef>()?.table;
				if extended {
					upper_gdef(table, options.overwrite)?;
				} else {
					lower_gdef(table, options.overwrite)?;
				}
			}
			b"GSUB" => {
				let table = &mut font.require::<Gsub>()?.table;
				if extended {
					upper_gsub(table, options.overwrite)?;
				} else {
					lower_gsub(table, options.overwrite)?;
				}
			}
			b"GPOS" => {
				let table = &mut font.require::<Gpos>()?.table;
				if extended {
					upper_gpos(table, options.overwrite)?;
				} else {
					lower_gpos(table, options.overwrite)?;
				}
			}
			b"JSTF" => {
				let table = &mut font.require::<Jstf>()?.table;
				if extended {
					upper_jstf(table);
				} else {
					lower_jstf(table);
				}
			}
			_ => {
				let vorg = font.require::<Vorg>()?;
				vorg.major_version = if extended { 2 } else { 1 };
				vorg.minor_version = 0;
			}
		}
	}

	if options.validate {
		let result: BTreeSet<Tag> = font.tags().into_iter().collect();
		if let Some((lower, upper)) = mixed_family(&result) {
			return Err(Error::value(format!(
				"Mixed beyond-64k table family: lowercase {}; uppercase {}",
				tag_text(&lower),
				tag_text(&upper)
			)));
		}
	}
	Ok(())
}

/// `upper_tables`: converts the tables to their beyond-64k companion forms.
pub fn upper_tables(font: &mut Font, options: &ConvertOptions<'_>) -> Result<()> {
	convert_tables(font, options, true)?;
	for_each_character_map(font, |tag, map| convert_cmap_uvs(map, tag, 14, 15))?;
	// `_drop_beyond64k_cmap_format4`.
	if (font.has::<Cmap>() || font.has::<Dmap>()) && font.glyph_order()?.len() > 0x10000 {
		for_each_character_map(font, |_, map| {
			drop_cmap_format4(map);
			Ok(())
		})?;
	}
	// `_drop_beyond64k_post_glyph_names`: post 2.0 numberOfGlyphs is uint16.
	if font.has::<Post>()
		&& font.glyph_order()?.len() > 0xFFFF
		&& let Some(post) = font.get::<Post>()?
		&& post.format_type == 2.0
	{
		post.format_type = 3.0;
		post.extra_names = Some(Vec::new());
		post.mapping = Some(BTreeMap::new());
	}
	Ok(())
}

/// `lower_tables`: converts the tables from their beyond-64k companion forms. Besides
/// mixed-family validation, this rejects data that cannot be represented in compact
/// tables, including glyph counts above 65535, cubic `GLYF` outlines, and component glyph
/// IDs that do not fit in `glyf`.
pub fn lower_tables(font: &mut Font, options: &ConvertOptions<'_>) -> Result<()> {
	convert_tables(font, options, false)?;
	for_each_character_map(font, |tag, map| convert_cmap_uvs(map, tag, 15, 14))
}

/// The help of [`main`].
const HELP: &str = "\
usage: fonttools ttLib.beyond64k [-h] [-o OUTPUT] [--no-validate] [--overwrite]
                                 [--no-ignore-missing]
                                 {upper,lower} INPUT [TABLE ...]

Convert between Open Font Format beyond-64k companion tables.

    fonttools ttLib.beyond64k upper input.ttf -o upper.ttf
    fonttools ttLib.beyond64k lower upper.ttf -o lower.ttf
    fonttools ttLib.beyond64k upper input.ttf GSUB GPOS glyf

Lowering validates lossy cases such as MAXP.numGlyphs values above 65535, GLYF glyphs
with cubic outlines, and component glyph IDs that do not fit in the compact glyf table.

positional arguments:
  {upper,lower}         'upper' converts to beyond-64k companions; 'lower' converts back
  INPUT                 input font path
  TABLE                 optional table tags to convert, e.g. GSUB GPOS glyf

options:
  -h, --help            show this help message and exit
  -o, --output OUTPUT   output font path
  --no-validate         skip mixed-family and lossy-lowering validation
  --overwrite           replace destination tables if they already exist
  --no-ignore-missing   raise an error when a requested source table is missing
";

/// The help text of the command line.
pub fn help() -> &'static str {
	HELP
}

/// `main`: converts a font file; returns the exit code.
pub fn main(args: &[&str]) -> Result<i32> {
	let usage = |message: String| Error::value(format!("error: {message}"));
	let mut positional = Vec::new();
	let mut output = None;
	let mut options = ConvertOptions::default();
	let mut it = args.iter();
	while let Some(&arg) = it.next() {
		match arg {
			"-h" | "--help" => {
				print!("{HELP}");
				return Ok(0);
			}
			"-o" | "--output" => {
				output = Some(
					*it.next()
						.ok_or_else(|| usage(format!("argument {arg}: expected one argument")))?,
				);
			}
			"--no-validate" => options.validate = false,
			"--overwrite" => options.overwrite = true,
			"--no-ignore-missing" => options.ignore_missing = false,
			_ if arg.starts_with("--output=") => output = Some(&arg["--output=".len()..]),
			_ if arg.len() > 1 && arg.starts_with('-') => {
				return Err(usage(format!("unrecognized arguments: {arg}")));
			}
			_ => positional.push(arg),
		}
	}
	let [direction, input, tables @ ..] = positional.as_slice() else {
		return Err(usage(
			"the following arguments are required: direction, INPUT".to_owned(),
		));
	};
	let extended = match *direction {
		"upper" => true,
		"lower" => false,
		other => {
			return Err(usage(format!(
				"argument direction: invalid choice: {}",
				fontoxide_misc::py::str_repr(other)
			)));
		}
	};
	if !tables.is_empty() {
		options.tables = Some(tables);
	}
	let output = match output {
		Some(output) => output.to_owned(),
		None => fontoxide_misc::cli_tools::make_output_file_name(
			input,
			None,
			None,
			true,
			&format!("-{direction}"),
		),
	};
	let mut font = Font::read(std::fs::read(input)?, crate::ReadOptions::default())?;
	if extended {
		upper_tables(&mut font, &options)?;
	} else {
		lower_tables(&mut font, &options)?;
	}
	std::fs::write(output, font.write(&crate::WriteOptions::default())?)?;
	Ok(0)
}
