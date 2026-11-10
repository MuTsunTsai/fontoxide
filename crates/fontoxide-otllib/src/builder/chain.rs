//! Contextual and chaining contextual lookups (`ChainContextualBuilder`).

use fontoxide_ttlib::Font;
use fontoxide_ttlib::ot::WriterGraph;
use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types::{self as ot, Coverage, LookupSubTable};

use super::class_def::ClassDefBuilder;
use super::lookups::{ChainContextualRule, LookupCommon, LookupRef, RuleLookup};
use super::{LayoutTable, build_coverage, build_lookup};
use crate::{Error, Result};

/// The rules of a contextual lookup up to a subtable break
/// (`ChainContextualRuleset`).
#[derive(Debug, Clone, Default)]
pub struct ChainContextualRuleset {
	/// The rules (never subtable breaks).
	pub rules: Vec<ChainContextualRule>,
}

impl ChainContextualRuleset {
	/// A ruleset without rules.
	pub fn new() -> Self {
		Self::default()
	}

	/// Adds a rule (`addRule`).
	pub fn add_rule(&mut self, rule: ChainContextualRule) {
		self.rules.push(rule);
	}

	/// Whether any rule has a prefix or a suffix; if none has, the rules can be
	/// a non-chaining lookup (`hasPrefixOrSuffix`).
	pub fn has_prefix_or_suffix(&self) -> bool {
		self.rules
			.iter()
			.any(|r| !r.prefix().is_empty() || !r.suffix().is_empty())
	}

	/// Whether any rule has a glyph class of more than one glyph; if none has,
	/// the rules can be a format 1 subtable (`hasAnyGlyphClasses`).
	pub fn has_any_glyph_classes(&self) -> bool {
		self.rules.iter().any(|r| {
			[r.prefix(), r.glyphs(), r.suffix()]
				.iter()
				.any(|seq| seq.iter().any(|x| x.len() > 1))
		})
	}

	/// The class definitions of the prefix, input and suffix glyphs for a
	/// format 2 subtable; `None` if the glyph classes overlap
	/// (`format2ClassDefs`).
	pub fn format2_class_defs(&self) -> Option<[ClassDefBuilder; 3]> {
		let prefix = class_builder_for_context(self.rules.iter().map(|r| r.prefix()))?;
		let glyphs = class_builder_for_context(self.rules.iter().map(|r| r.glyphs()))?;
		let suffix = class_builder_for_context(self.rules.iter().map(|r| r.suffix()))?;
		Some([prefix, glyphs, suffix])
	}
}

/// The distinct glyphs of a class, sorted (`sorted(set(glyphs))`).
fn distinct(glyphs: &[String]) -> Vec<String> {
	let mut v = glyphs.to_vec();
	v.sort();
	v.dedup();
	v
}

/// `_classBuilderForContext`.
fn class_builder_for_context<'a>(
	context: impl Iterator<Item = &'a [Vec<String>]>,
) -> Option<ClassDefBuilder> {
	let mut builder = ClassDefBuilder::new(false);
	for position in context {
		for glyphset in position {
			let glyphs = distinct(glyphset);
			if !builder.can_add(&glyphs) {
				return None;
			}
			builder.add(&glyphs).ok()?;
		}
	}
	Some(builder)
}

/// A lookup record of a rule: which lookup applies at which position.
#[derive(Debug, Clone, Copy)]
struct Rec {
	seq: u16,
	index: u16,
}

/// A format 1 or 2 rule: glyph IDs (format 1) or classes (format 2).
#[derive(Debug, Clone, Default)]
struct SeqRule<T> {
	backtrack: Vec<T>,
	input: Vec<T>,
	look_ahead: Vec<T>,
	records: Vec<Rec>,
}

fn opt_box<T>(v: T) -> Option<Box<T>> {
	Some(Box::new(v))
}

fn count<T>(v: &[T]) -> Option<u32> {
	Some(v.len() as u32)
}

/// Whether the builder is for positioning (`subtable_type == "Pos"`).
fn is_pos(common: &LookupCommon) -> bool {
	common.table == LayoutTable::Gpos
}

/// `buildLookupList`: the lookup records of a rule.
fn lookup_records(common: &LookupCommon, rule: &ChainContextualRule) -> Result<Vec<Rec>> {
	let mut records = Vec::new();
	for (sequence_index, lookups) in rule.lookups().iter().enumerate() {
		let Some(lookups) = lookups else {
			continue;
		};
		let list: Vec<&LookupRef> = match lookups {
			RuleLookup::One(l) => vec![l],
			RuleLookup::Many(ls) => ls.iter().collect(),
		};
		for l in list {
			let index = l
				.try_borrow()
				.map_err(|_| Error::Value("a lookup refers to itself".into()))?
				.common()
				.lookup_index;
			let Some(index) = index else {
				let other = if is_pos(common) {
					"substitution"
				} else {
					"positioning"
				};
				return Err(Error::otl(
					format!("Missing index of the specified lookup, might be a {other} lookup"),
					&common.location,
				));
			};
			records.push(Rec {
				seq: fit_u16(sequence_index)?,
				index: fit_u16(index)?,
			});
		}
	}
	Ok(records)
}

fn fit_u16(v: usize) -> Result<u16> {
	u16::try_from(v).map_err(|_| {
		Error::TtLib(fontoxide_ttlib::Error::Struct(format!(
			"'H' format requires 0 <= number <= 65535: {v}"
		)))
	})
}

fn first(class: &[String]) -> Result<&str> {
	class
		.first()
		.map(String::as_str)
		.ok_or_else(|| Error::Value("IndexError: list index out of range".into()))
}

/// The coverage tables of a sequence of glyph classes.
fn coverages(common: &LookupCommon, seq: &[Vec<String>]) -> Result<Vec<Option<Box<Coverage>>>> {
	seq.iter()
		.map(|c| build_coverage(c.iter().map(String::as_str), &common.glyph_map).map(opt_box))
		.collect()
}

/// Builds a format 1 subtable, for rules without glyph classes
/// (`buildFormat1Subtable`). For fonts with more than 65536 glyphs, the format
/// is 4.
pub fn build_format1_subtable(
	common: &LookupCommon,
	ruleset: &ChainContextualRuleset,
	chaining: bool,
) -> Result<LookupSubTable> {
	let gm = &common.glyph_map;
	let mut first_glyphs: Vec<&str> = Vec::new();
	let mut by_first: indexmap::IndexMap<&str, Vec<SeqRule<u32>>> = indexmap::IndexMap::new();
	for rule in &ruleset.rules {
		let ids = |seq: &[Vec<String>]| -> Result<Vec<u32>> {
			seq.iter().map(|x| gm.id(first(x)?)).collect()
		};
		let mut backtrack = Vec::new();
		let mut look_ahead = Vec::new();
		if chaining {
			let mut prefix: Vec<Vec<String>> = rule.prefix().to_vec();
			prefix.reverse();
			backtrack = ids(&prefix)?;
			look_ahead = ids(rule.suffix())?;
		}
		let glyphs = rule.glyphs();
		let input = ids(glyphs.get(1..).unwrap_or(&[]))?;
		let records = lookup_records(common, rule)?;
		let first_glyph = first(glyphs.first().map_or(&[][..], Vec::as_slice))?;
		if !by_first.contains_key(first_glyph) {
			first_glyphs.push(first_glyph);
		}
		by_first.entry(first_glyph).or_default().push(SeqRule {
			backtrack,
			input,
			look_ahead,
			records,
		});
	}
	let coverage = build_coverage(first_glyphs.iter().copied(), gm)?;
	let mut sets = Vec::with_capacity(coverage.glyphs.len());
	for (_, g) in super::sorted_glyphs(first_glyphs.iter().copied(), gm)? {
		sets.push(by_first[g].clone());
	}
	Ok(format1(common, chaining, coverage, sets))
}

/// Builds a format 2 subtable, where the glyphs are matched by class
/// (`buildFormat2Subtable`). For fonts with more than 65536 glyphs, the format
/// is 5.
pub fn build_format2_subtable(
	common: &LookupCommon,
	ruleset: &ChainContextualRuleset,
	classdefs: &[ClassDefBuilder; 3],
	chaining: bool,
) -> Result<LookupSubTable> {
	let gm = &common.glyph_map;
	let [back_cd, input_cd, look_cd] = classdefs;
	let back_classes = back_cd.class_defs();
	let input_classes = input_cd.class_defs();
	let look_classes = look_cd.class_defs();
	let class_of = |map: &indexmap::IndexMap<String, u32>, x: &[String]| -> Result<u16> {
		let g = first(x)?;
		let c = map
			.get(g)
			.ok_or_else(|| Error::Value(format!("KeyError: {}", crate::glyph_map::py_repr(g))))?;
		fit_u16(*c as usize)
	};
	let in_classes = input_cd.classes();
	let mut class_sets: Vec<Vec<SeqRule<u16>>> = vec![Vec::new(); in_classes.len()];
	let mut coverage: Vec<&str> = Vec::new();
	for rule in &ruleset.rules {
		let glyphs = rule.glyphs();
		let mut seq_rule = SeqRule::default();
		if chaining {
			seq_rule.backtrack = rule
				.prefix()
				.iter()
				.rev()
				.map(|x| class_of(&back_classes, x))
				.collect::<Result<_>>()?;
			seq_rule.look_ahead = rule
				.suffix()
				.iter()
				.map(|x| class_of(&look_classes, x))
				.collect::<Result<_>>()?;
		}
		seq_rule.input = glyphs
			.get(1..)
			.unwrap_or(&[])
			.iter()
			.map(|x| class_of(&input_classes, x))
			.collect::<Result<_>>()?;
		let set_index = class_of(
			&input_classes,
			glyphs.first().map_or(&[][..], Vec::as_slice),
		)?;
		seq_rule.records = lookup_records(common, rule)?;
		if let Some(first_class) = glyphs.first() {
			coverage.extend(first_class.iter().map(String::as_str));
		}
		class_sets
			.get_mut(set_index as usize)
			.ok_or_else(|| Error::Value("IndexError: list index out of range".into()))?
			.push(seq_rule);
	}
	let class_sets: Vec<Option<Vec<SeqRule<u16>>>> = class_sets
		.into_iter()
		.map(|s| (!s.is_empty()).then_some(s))
		.collect();
	let coverage = build_coverage(coverage, gm)?;
	let class_defs = if chaining {
		[
			Some(back_cd.build(gm)?),
			Some(input_cd.build(gm)?),
			Some(look_cd.build(gm)?),
		]
	} else {
		[None, Some(input_cd.build(gm)?), None]
	};
	Ok(format2(common, chaining, coverage, class_defs, class_sets))
}

/// Builds a format 3 subtable, with a coverage table for each position
/// (`buildFormat3Subtable`). For fonts with more than 65536 glyphs, the
/// non-chaining format is 6.
pub fn build_format3_subtable(
	common: &LookupCommon,
	rule: &ChainContextualRule,
	chaining: bool,
) -> Result<LookupSubTable> {
	let records = lookup_records(common, rule)?;
	let extended = common.glyph_map.has_extended_glyph_ids();
	let pos = is_pos(common);
	if chaining {
		let mut prefix: Vec<Vec<String>> = rule.prefix().to_vec();
		prefix.reverse();
		let backtrack_coverage = coverages(common, &prefix)?;
		let look_ahead_coverage = coverages(common, rule.suffix())?;
		let input_coverage = coverages(common, rule.glyphs())?;
		if extended {
			return Err(Error::Assertion("format in (1, 2)".into()));
		}
		return Ok(if pos {
			LookupSubTable::ChainContextPos(ot::ChainContextPos::Format3(
				ot::ChainContextPosFormat3 {
					backtrack_glyph_count: count(&backtrack_coverage),
					backtrack_coverage,
					input_glyph_count: count(&input_coverage),
					input_coverage,
					look_ahead_glyph_count: count(&look_ahead_coverage),
					look_ahead_coverage,
					pos_count: count(&records),
					pos_lookup_record: records.iter().map(pos_rec).collect(),
					dont_share: false,
				},
			))
		} else {
			LookupSubTable::ChainContextSubst(ot::ChainContextSubst::Format3(
				ot::ChainContextSubstFormat3 {
					backtrack_glyph_count: count(&backtrack_coverage),
					backtrack_coverage,
					input_glyph_count: count(&input_coverage),
					input_coverage,
					look_ahead_glyph_count: count(&look_ahead_coverage),
					look_ahead_coverage,
					subst_count: count(&records),
					subst_lookup_record: records.iter().map(subst_rec).collect(),
					dont_share: false,
				},
			))
		});
	}
	let coverage = coverages(common, rule.glyphs())?;
	Ok(match (pos, extended) {
		(true, false) => {
			LookupSubTable::ContextPos(ot::ContextPos::Format3(ot::ContextPosFormat3 {
				glyph_count: count(&coverage),
				pos_count: count(&records),
				coverage,
				pos_lookup_record: records.iter().map(pos_rec).collect(),
				dont_share: false,
			}))
		}
		(true, true) => {
			LookupSubTable::ContextPos(ot::ContextPos::Format6(ot::ContextPosFormat6 {
				glyph_count: count(&coverage),
				seq_lookup_count: count(&records),
				coverage,
				seq_lookup_record: records.iter().map(seq_rec).collect(),
				dont_share: false,
			}))
		}
		(false, false) => {
			LookupSubTable::ContextSubst(ot::ContextSubst::Format3(ot::ContextSubstFormat3 {
				glyph_count: count(&coverage),
				subst_count: count(&records),
				coverage,
				subst_lookup_record: records.iter().map(subst_rec).collect(),
				dont_share: false,
			}))
		}
		(false, true) => {
			LookupSubTable::ContextSubst(ot::ContextSubst::Format6(ot::ContextSubstFormat6 {
				glyph_count: count(&coverage),
				seq_lookup_count: count(&records),
				coverage,
				seq_lookup_record: records.iter().map(seq_rec).collect(),
				dont_share: false,
			}))
		}
	})
}

fn pos_rec(r: &Rec) -> ot::PosLookupRecord {
	ot::PosLookupRecord {
		sequence_index: r.seq,
		lookup_list_index: r.index,
	}
}

fn subst_rec(r: &Rec) -> ot::SubstLookupRecord {
	ot::SubstLookupRecord {
		sequence_index: r.seq,
		lookup_list_index: r.index,
	}
}

fn seq_rec(r: &Rec) -> ot::SeqLookup {
	ot::SeqLookup {
		sequence_index: r.seq,
		lookup_list_index: r.index,
	}
}

/// Assembles a format 1 (or 4) subtable of the right type.
fn format1(
	common: &LookupCommon,
	chaining: bool,
	coverage: Coverage,
	sets: Vec<Vec<SeqRule<u32>>>,
) -> LookupSubTable {
	let coverage = opt_box(coverage);
	let extended = common.glyph_map.has_extended_glyph_ids();
	let pos = is_pos(common);
	macro_rules! sets {
		($set:ident { $rc:ident, $r:ident }, |$rule:ident| $make:expr) => {
			sets.into_iter()
				.map(|rules| {
					let list: Vec<_> = rules.into_iter().map(|$rule| opt_box($make)).collect();
					opt_box(ot::$set {
						$rc: count(&list),
						$r: list,
					})
				})
				.collect::<Vec<_>>()
		};
	}
	if extended {
		if chaining {
			let set = sets!(
				ChainedSeqRuleSet2 {
					chained_seq_rule_count,
					chained_seq_rule
				},
				|r| {
					ot::ChainedSeqRule2 {
						backtrack_glyph_count: count(&r.backtrack),
						backtrack_sequence: r.backtrack,
						input_glyph_count: Some(r.input.len() as u32 + 1),
						input_sequence: r.input,
						look_ahead_glyph_count: count(&r.look_ahead),
						look_ahead_sequence: r.look_ahead,
						seq_lookup_count: count(&r.records),
						seq_lookup_record: r.records.iter().map(seq_rec).collect(),
					}
				}
			);
			let n = count(&set);
			return if pos {
				LookupSubTable::ChainContextPos(ot::ChainContextPos::Format4(
					ot::ChainContextPosFormat4 {
						coverage,
						chained_seq_rule_set_count: n,
						chained_seq_rule_set: set,
						dont_share: false,
					},
				))
			} else {
				LookupSubTable::ChainContextSubst(ot::ChainContextSubst::Format4(
					ot::ChainContextSubstFormat4 {
						coverage,
						chained_seq_rule_set_count: n,
						chained_seq_rule_set: set,
						dont_share: false,
					},
				))
			};
		}
		let set = sets!(
			SeqRuleSet2 {
				seq_rule_count,
				seq_rule
			},
			|r| ot::SeqRule2 {
				glyph_count: Some(r.input.len() as u32 + 1),
				seq_lookup_count: count(&r.records),
				input_sequence: r.input,
				seq_lookup_record: r.records.iter().map(seq_rec).collect(),
			}
		);
		let n = count(&set);
		return if pos {
			LookupSubTable::ContextPos(ot::ContextPos::Format4(ot::ContextPosFormat4 {
				coverage,
				seq_rule_set_count: n,
				seq_rule_set: set,
				dont_share: false,
			}))
		} else {
			LookupSubTable::ContextSubst(ot::ContextSubst::Format4(ot::ContextSubstFormat4 {
				coverage,
				seq_rule_set_count: n,
				seq_rule_set: set,
				dont_share: false,
			}))
		};
	}
	match (pos, chaining) {
		(true, true) => {
			let set = sets!(
				ChainPosRuleSet {
					chain_pos_rule_count,
					chain_pos_rule
				},
				|r| {
					ot::ChainPosRule {
						backtrack_glyph_count: count(&r.backtrack),
						backtrack: r.backtrack,
						input_glyph_count: Some(r.input.len() as u32 + 1),
						input: r.input,
						look_ahead_glyph_count: count(&r.look_ahead),
						look_ahead: r.look_ahead,
						pos_count: count(&r.records),
						pos_lookup_record: r.records.iter().map(pos_rec).collect(),
					}
				}
			);
			LookupSubTable::ChainContextPos(ot::ChainContextPos::Format1(
				ot::ChainContextPosFormat1 {
					coverage,
					chain_pos_rule_set_count: count(&set),
					chain_pos_rule_set: set,
					dont_share: false,
				},
			))
		}
		(false, true) => {
			let set = sets!(
				ChainSubRuleSet {
					chain_sub_rule_count,
					chain_sub_rule
				},
				|r| {
					ot::ChainSubRule {
						backtrack_glyph_count: count(&r.backtrack),
						backtrack: r.backtrack,
						input_glyph_count: Some(r.input.len() as u32 + 1),
						input: r.input,
						look_ahead_glyph_count: count(&r.look_ahead),
						look_ahead: r.look_ahead,
						subst_count: count(&r.records),
						subst_lookup_record: r.records.iter().map(subst_rec).collect(),
					}
				}
			);
			LookupSubTable::ChainContextSubst(ot::ChainContextSubst::Format1(
				ot::ChainContextSubstFormat1 {
					coverage,
					chain_sub_rule_set_count: count(&set),
					chain_sub_rule_set: set,
					dont_share: false,
				},
			))
		}
		(true, false) => {
			let set = sets!(
				PosRuleSet {
					pos_rule_count,
					pos_rule
				},
				|r| ot::PosRule {
					glyph_count: Some(r.input.len() as u32 + 1),
					pos_count: count(&r.records),
					input: r.input,
					pos_lookup_record: r.records.iter().map(pos_rec).collect(),
				}
			);
			LookupSubTable::ContextPos(ot::ContextPos::Format1(ot::ContextPosFormat1 {
				coverage,
				pos_rule_set_count: count(&set),
				pos_rule_set: set,
				dont_share: false,
			}))
		}
		(false, false) => {
			let set = sets!(
				SubRuleSet {
					sub_rule_count,
					sub_rule
				},
				|r| ot::SubRule {
					glyph_count: Some(r.input.len() as u32 + 1),
					subst_count: count(&r.records),
					input: r.input,
					subst_lookup_record: r.records.iter().map(subst_rec).collect(),
				}
			);
			LookupSubTable::ContextSubst(ot::ContextSubst::Format1(ot::ContextSubstFormat1 {
				coverage,
				sub_rule_set_count: count(&set),
				sub_rule_set: set,
				dont_share: false,
			}))
		}
	}
}

/// Assembles a format 2 (or 5) subtable of the right type. `class_defs` are
/// the backtrack, input and lookahead class definitions; only the input one for
/// non-chaining subtables.
fn format2(
	common: &LookupCommon,
	chaining: bool,
	coverage: Coverage,
	class_defs: [Option<ot::ClassDef>; 3],
	sets: Vec<Option<Vec<SeqRule<u16>>>>,
) -> LookupSubTable {
	let coverage = opt_box(coverage);
	let extended = common.glyph_map.has_extended_glyph_ids();
	let pos = is_pos(common);
	let [back, input, look] = class_defs.map(|c| c.map(Box::new));
	macro_rules! sets {
		($set:ident { $rc:ident, $r:ident }, |$rule:ident| $make:expr) => {
			sets.into_iter()
				.map(|rules| {
					rules.map(|rules| {
						let list: Vec<_> = rules.into_iter().map(|$rule| opt_box($make)).collect();
						Box::new(ot::$set {
							$rc: count(&list),
							$r: list,
						})
					})
				})
				.collect::<Vec<_>>()
		};
	}
	if extended {
		if chaining {
			let set = sets!(
				ChainedClassSeqRuleSet2 {
					chained_class_seq_rule_count,
					chained_class_seq_rule
				},
				|r| ot::ChainedClassSeqRule {
					backtrack_glyph_count: count(&r.backtrack),
					backtrack_sequence: r.backtrack,
					input_glyph_count: Some(r.input.len() as u32 + 1),
					input_sequence: r.input,
					look_ahead_glyph_count: count(&r.look_ahead),
					look_ahead_sequence: r.look_ahead,
					seq_lookup_count: count(&r.records),
					seq_lookup_record: r.records.iter().map(seq_rec).collect(),
				}
			);
			let n = count(&set);
			return if pos {
				LookupSubTable::ChainContextPos(ot::ChainContextPos::Format5(
					ot::ChainContextPosFormat5 {
						coverage,
						backtrack_class_def: back,
						input_class_def: input,
						look_ahead_class_def: look,
						chained_class_seq_rule_set_count: n,
						chained_class_seq_rule_set: set,
						dont_share: false,
					},
				))
			} else {
				LookupSubTable::ChainContextSubst(ot::ChainContextSubst::Format5(
					ot::ChainContextSubstFormat5 {
						coverage,
						backtrack_class_def: back,
						input_class_def: input,
						look_ahead_class_def: look,
						chained_class_seq_rule_set_count: n,
						chained_class_seq_rule_set: set,
						dont_share: false,
					},
				))
			};
		}
		let set = sets!(
			ClassSeqRuleSet2 {
				class_seq_rule_count,
				class_seq_rule
			},
			|r| {
				ot::ClassSeqRule {
					glyph_count: Some(r.input.len() as u32 + 1),
					seq_lookup_count: count(&r.records),
					input_sequence: r.input,
					seq_lookup_record: r.records.iter().map(seq_rec).collect(),
				}
			}
		);
		let n = count(&set);
		return if pos {
			LookupSubTable::ContextPos(ot::ContextPos::Format5(ot::ContextPosFormat5 {
				coverage,
				class_def: input,
				class_seq_rule_set_count: n,
				class_seq_rule_set: set,
				dont_share: false,
			}))
		} else {
			LookupSubTable::ContextSubst(ot::ContextSubst::Format5(ot::ContextSubstFormat5 {
				coverage,
				class_def: input,
				class_seq_rule_set_count: n,
				class_seq_rule_set: set,
				dont_share: false,
			}))
		};
	}
	match (pos, chaining) {
		(true, true) => {
			let set = sets!(
				ChainPosClassSet {
					chain_pos_class_rule_count,
					chain_pos_class_rule
				},
				|r| {
					ot::ChainPosClassRule {
						backtrack_glyph_count: count(&r.backtrack),
						backtrack: r.backtrack,
						input_glyph_count: Some(r.input.len() as u32 + 1),
						input: r.input,
						look_ahead_glyph_count: count(&r.look_ahead),
						look_ahead: r.look_ahead,
						pos_count: count(&r.records),
						pos_lookup_record: r.records.iter().map(pos_rec).collect(),
					}
				}
			);
			LookupSubTable::ChainContextPos(ot::ChainContextPos::Format2(
				ot::ChainContextPosFormat2 {
					coverage,
					backtrack_class_def: back,
					input_class_def: input,
					look_ahead_class_def: look,
					chain_pos_class_set_count: count(&set),
					chain_pos_class_set: set,
					dont_share: false,
				},
			))
		}
		(false, true) => {
			let set = sets!(
				ChainSubClassSet {
					chain_sub_class_rule_count,
					chain_sub_class_rule
				},
				|r| {
					ot::ChainSubClassRule {
						backtrack_glyph_count: count(&r.backtrack),
						backtrack: r.backtrack,
						input_glyph_count: Some(r.input.len() as u32 + 1),
						input: r.input,
						look_ahead_glyph_count: count(&r.look_ahead),
						look_ahead: r.look_ahead,
						subst_count: count(&r.records),
						subst_lookup_record: r.records.iter().map(subst_rec).collect(),
					}
				}
			);
			LookupSubTable::ChainContextSubst(ot::ChainContextSubst::Format2(
				ot::ChainContextSubstFormat2 {
					coverage,
					backtrack_class_def: back,
					input_class_def: input,
					look_ahead_class_def: look,
					chain_sub_class_set_count: count(&set),
					chain_sub_class_set: set,
					dont_share: false,
				},
			))
		}
		(true, false) => {
			let set = sets!(
				PosClassSet {
					pos_class_rule_count,
					pos_class_rule
				},
				|r| ot::PosClassRule {
					glyph_count: Some(r.input.len() as u32 + 1),
					pos_count: count(&r.records),
					class: r.input,
					pos_lookup_record: r.records.iter().map(pos_rec).collect(),
				}
			);
			LookupSubTable::ContextPos(ot::ContextPos::Format2(ot::ContextPosFormat2 {
				coverage,
				class_def: input,
				pos_class_set_count: count(&set),
				pos_class_set: set,
				dont_share: false,
			}))
		}
		(false, false) => {
			let set = sets!(
				SubClassSet {
					sub_class_rule_count,
					sub_class_rule
				},
				|r| {
					ot::SubClassRule {
						glyph_count: Some(r.input.len() as u32 + 1),
						subst_count: count(&r.records),
						class: r.input,
						subst_lookup_record: r.records.iter().map(subst_rec).collect(),
					}
				}
			);
			LookupSubTable::ContextSubst(ot::ContextSubst::Format2(ot::ContextSubstFormat2 {
				coverage,
				class_def: input,
				sub_class_set_count: count(&set),
				sub_class_set: set,
				dont_share: false,
			}))
		}
	}
}

/// The size of the subtables compiled as a lookup, or `None` if an offset
/// overflows (`getCompiledSize_`).
fn compiled_size(
	common: &LookupCommon,
	subtables: &[LookupSubTable],
	font: &mut Font,
) -> Result<Option<usize>> {
	if subtables.is_empty() {
		return Ok(Some(0));
	}
	let lookup = build_lookup(
		subtables.iter().cloned().map(Some).collect(),
		common.lookupflag,
		common.mark_filter_set,
		Some(common.table),
		common.extension,
	)?;
	let Some(lookup) = lookup else {
		return Ok(Some(0));
	};
	let mut graph = WriterGraph::new(common.table.tag());
	graph.set_root_name("Lookup", Some(0));
	lookup.compile(&mut graph.root(), font)?;
	match graph.get_all_data() {
		Ok(data) => Ok(Some(data.len())),
		Err(fontoxide_ttlib::Error::OffsetOverflow(e)) => {
			log::debug!("overflow: {e:?}");
			Ok(None)
		}
		Err(e) => Err(e.into()),
	}
}

/// Builds the lookup of a contextual builder (`ChainContextualBuilder.build`):
/// for each ruleset, the smallest of the formats that can express it. Rulesets
/// whose candidates all overflow are split in two.
pub(super) fn build(
	common: &LookupCommon,
	rulesets: Vec<ChainContextualRuleset>,
	font: &mut Font,
) -> Result<Option<ot::Lookup>> {
	let mut subtables: Vec<LookupSubTable> = Vec::new();
	let mut chaining = rulesets
		.iter()
		.any(ChainContextualRuleset::has_prefix_or_suffix);
	// Apple's CoreText did not process GPOS7 correctly before macOS 13.2, so
	// contextual positioning is chaining unless WRITE_GPOS7 is set.
	if !font.write_gpos7 && is_pos(common) {
		chaining = true;
	}
	let mut pending: Vec<ChainContextualRuleset> = rulesets.into_iter().rev().collect();
	while let Some(ruleset) = pending.pop() {
		let extended = common.glyph_map.has_extended_glyph_ids();
		let mut candidates: [Option<Vec<LookupSubTable>>; 4] = [None, None, None, None];
		if !(extended && chaining) {
			let mut f3 = Vec::new();
			for rule in &ruleset.rules {
				f3.push(build_format3_subtable(common, rule, chaining)?);
			}
			candidates[3] = Some(f3);
		}
		if let Some(classdefs) = ruleset.format2_class_defs() {
			candidates[2] = Some(vec![build_format2_subtable(
				common, &ruleset, &classdefs, chaining,
			)?]);
		}
		if !ruleset.has_any_glyph_classes() {
			candidates[1] = Some(vec![build_format1_subtable(common, &ruleset, chaining)?]);
		}
		let mut by_size: Vec<(usize, Vec<LookupSubTable>)> = Vec::new();
		for (i, candidate) in candidates.into_iter().enumerate().skip(1) {
			let Some(candidate) = candidate.filter(|c| !c.is_empty()) else {
				continue;
			};
			match compiled_size(common, &candidate, font)? {
				Some(size) => by_size.push((size, candidate)),
				None => log::warn!(
					"Contextual format {i} at {} overflowed",
					common.location.as_deref().unwrap_or("None")
				),
			}
		}
		if by_size.is_empty() {
			if ruleset.rules.len() <= 1 {
				return Err(Error::otl("All candidates overflowed", &common.location));
			}
			// Retry smaller consecutive groups in the same lookup, in order, which
			// keeps the first-matching-rule semantics.
			let mid = ruleset.rules.len() / 2;
			let (a, b) = ruleset.rules.split_at(mid);
			pending.push(ChainContextualRuleset { rules: b.to_vec() });
			pending.push(ChainContextualRuleset { rules: a.to_vec() });
			continue;
		}
		let mut best = 0;
		for (i, (size, _)) in by_size.iter().enumerate() {
			if *size < by_size[best].0 {
				best = i;
			}
		}
		subtables.extend(by_size.swap_remove(best).1);
	}
	build_lookup(
		subtables.into_iter().map(Some).collect(),
		common.lookupflag,
		common.mark_filter_set,
		Some(common.table),
		common.extension,
	)
}
