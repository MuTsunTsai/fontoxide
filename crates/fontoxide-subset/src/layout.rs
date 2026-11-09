//! Subsetting of `GSUB`, `GPOS` and `GDEF`: the methods `fontTools.subset`
//! adds to the otTables classes.

use std::collections::{BTreeSet, HashMap};

use fontoxide_ttlib::ot::conv::ValueRecord;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::types::{ClassDef, Coverage, LookupSubTable as St};
use fontoxide_ttlib::tables::{Gdef, Gpos, Gsub};
use fontoxide_ttlib::{Font, Tag};

use crate::options::has_str;
use crate::{Error, GlyphSet, Options, Result, Subsetter};

fn err(msg: impl Into<String>) -> Error {
	fontoxide_ttlib::Error::ttlib(msg).into()
}

fn none_attr(attr: &str) -> Error {
	err(format!(
		"AttributeError: 'NoneType' object has no attribute '{attr}'"
	))
}

fn unknown_format(format: u16) -> Error {
	fontoxide_ttlib::Error::Assertion(format!("unknown format: {format}")).into()
}

/// `_uniq_sort`.
fn uniq_sort<T: Ord + Copy>(v: impl IntoIterator<Item = T>) -> Vec<T> {
	let set: BTreeSet<T> = v.into_iter().collect();
	set.into_iter().collect()
}

/// `_list_subset`.
pub(crate) fn list_subset<T: Clone>(l: &[T], indices: &[usize]) -> Vec<T> {
	indices.iter().filter_map(|&i| l.get(i).cloned()).collect()
}

/// `list.index` of a sorted list of unique values.
fn index_of<T: Ord>(l: &[T], v: &T) -> Result<usize> {
	l.binary_search(v)
		.map_err(|_| fontoxide_ttlib::Error::value("ValueError: x not in list").into())
}

pub(crate) fn cov(c: &Option<Box<Coverage>>) -> Result<&Coverage> {
	c.as_deref().ok_or_else(|| none_attr("glyphs"))
}

pub(crate) fn cov_mut(c: &mut Option<Box<Coverage>>) -> Result<&mut Coverage> {
	c.as_deref_mut().ok_or_else(|| none_attr("glyphs"))
}

// Coverage and ClassDef.

/// `Coverage.intersect`: the coverage indices of the glyphs in `glyphs`.
pub(crate) fn cov_intersect(c: &Coverage, glyphs: &GlyphSet) -> Vec<usize> {
	c.glyphs
		.iter()
		.enumerate()
		.filter(|(_, g)| glyphs.contains(g))
		.map(|(i, _)| i)
		.collect()
}

/// `Coverage.intersect_glyphs`.
fn cov_intersect_glyphs(c: &Coverage, glyphs: &GlyphSet) -> GlyphSet {
	c.glyphs
		.iter()
		.copied()
		.filter(|g| glyphs.contains(g))
		.collect()
}

/// `Coverage.subset`: keeps the glyphs in `glyphs`; returns their old indices.
pub(crate) fn cov_subset(c: &mut Coverage, glyphs: &GlyphSet) -> Vec<usize> {
	let indices = cov_intersect(c, glyphs);
	c.glyphs.retain(|g| glyphs.contains(g));
	indices
}

/// `Coverage.remap`.
fn cov_remap(c: &mut Coverage, map: &[usize]) -> Result<()> {
	c.glyphs = map
		.iter()
		.map(|&i| {
			c.glyphs
				.get(i)
				.copied()
				.ok_or_else(|| err("IndexError: list index out of range"))
		})
		.collect::<Result<_>>()?;
	Ok(())
}

/// `ClassDef.intersect`: the classes of `glyphs`, with 0 for glyphs without one.
fn cd_intersect(cd: &ClassDef, glyphs: &GlyphSet) -> Vec<u32> {
	let mut classes: Vec<u32> = cd
		.class_defs
		.iter()
		.filter(|(g, _)| glyphs.contains(g))
		.map(|(_, &v)| v)
		.collect();
	if glyphs.iter().any(|g| !cd.class_defs.contains_key(g)) {
		classes.push(0);
	}
	uniq_sort(classes)
}

/// `ClassDef.intersect_class`: the glyphs of `glyphs` in class `klass`.
fn cd_intersect_class(cd: &ClassDef, glyphs: &GlyphSet, klass: u32) -> GlyphSet {
	if klass == 0 {
		return glyphs
			.iter()
			.copied()
			.filter(|g| !cd.class_defs.contains_key(g))
			.collect();
	}
	cd.class_defs
		.iter()
		.filter(|&(g, &v)| v == klass && glyphs.contains(g))
		.map(|(&g, _)| g)
		.collect()
}

/// `ClassDef.subset`: keeps the glyphs in `glyphs`; returns the remaining
/// classes, which become 0, 1, … with `remap`.
fn cd_subset(cd: &mut ClassDef, glyphs: &GlyphSet, remap: bool, use_class0: bool) -> Vec<u32> {
	cd.class_defs.retain(|g, _| glyphs.contains(g));
	let mut classes: Vec<u32> = cd.class_defs.values().copied().collect();
	if !use_class0 || glyphs.iter().any(|g| !cd.class_defs.contains_key(g)) {
		classes.push(0);
	}
	let indices = uniq_sort(classes);
	if remap {
		for v in cd.class_defs.values_mut() {
			*v = indices.binary_search(v).unwrap_or(0) as u32;
		}
	}
	indices
}

// Lookups of context subtables.

trait LookupRec {
	fn lookup(&self) -> u16;
	fn set_lookup(&mut self, v: u16);
	fn seq(&self) -> u16;
}

macro_rules! lookup_rec {
	($($t:ty),*) => {$(
		impl LookupRec for $t {
			fn lookup(&self) -> u16 { self.lookup_list_index }
			fn set_lookup(&mut self, v: u16) { self.lookup_list_index = v; }
			fn seq(&self) -> u16 { self.sequence_index }
		}
	)*};
}
lookup_rec!(ot::SubstLookupRecord, ot::PosLookupRecord);

/// A rule of a format 1 or 2 context subtable.
trait Rule {
	/// Glyphs (format 1) or classes (format 2).
	type T: Copy + Ord + Into<u32> + TryFrom<u32>;
	type R: LookupRec;
	/// `RuleData`: (backtrack, input, lookahead) of chained rules, else (input).
	fn data(&self) -> Vec<&Vec<Self::T>>;
	fn data_mut(&mut self) -> Vec<&mut Vec<Self::T>>;
	/// `Input`, or `Class` for format 2 non-chained rules.
	fn input(&self) -> &Vec<Self::T>;
	fn records(&self) -> &Vec<Self::R>;
	fn records_mut(&mut self) -> &mut Vec<Self::R>;
}

macro_rules! rule {
	($t:ty, $item:ty, $rec:ty, $records:ident, [$($d:ident),*], $input:ident) => {
		impl Rule for $t {
			type T = $item;
			type R = $rec;
			fn data(&self) -> Vec<&Vec<$item>> { vec![$(&self.$d),*] }
			fn data_mut(&mut self) -> Vec<&mut Vec<$item>> { vec![$(&mut self.$d),*] }
			fn input(&self) -> &Vec<$item> { &self.$input }
			fn records(&self) -> &Vec<$rec> { &self.$records }
			fn records_mut(&mut self) -> &mut Vec<$rec> { &mut self.$records }
		}
	};
}
rule!(
	ot::SubRule,
	u32,
	ot::SubstLookupRecord,
	subst_lookup_record,
	[input],
	input
);
rule!(
	ot::SubClassRule,
	u16,
	ot::SubstLookupRecord,
	subst_lookup_record,
	[class],
	class
);
rule!(
	ot::ChainSubRule,
	u32,
	ot::SubstLookupRecord,
	subst_lookup_record,
	[backtrack, input, look_ahead],
	input
);
rule!(
	ot::ChainSubClassRule,
	u16,
	ot::SubstLookupRecord,
	subst_lookup_record,
	[backtrack, input, look_ahead],
	input
);
rule!(
	ot::PosRule,
	u32,
	ot::PosLookupRecord,
	pos_lookup_record,
	[input],
	input
);
rule!(
	ot::PosClassRule,
	u16,
	ot::PosLookupRecord,
	pos_lookup_record,
	[class],
	class
);
rule!(
	ot::ChainPosRule,
	u32,
	ot::PosLookupRecord,
	pos_lookup_record,
	[backtrack, input, look_ahead],
	input
);
rule!(
	ot::ChainPosClassRule,
	u16,
	ot::PosLookupRecord,
	pos_lookup_record,
	[backtrack, input, look_ahead],
	input
);

trait RuleSet {
	type Rule: Rule;
	fn rules(&self) -> &Vec<Option<Box<Self::Rule>>>;
	fn rules_mut(&mut self) -> &mut Vec<Option<Box<Self::Rule>>>;
}

macro_rules! rule_set {
	($t:ty, $rule:ty, $f:ident) => {
		impl RuleSet for $t {
			type Rule = $rule;
			fn rules(&self) -> &Vec<Option<Box<$rule>>> {
				&self.$f
			}
			fn rules_mut(&mut self) -> &mut Vec<Option<Box<$rule>>> {
				&mut self.$f
			}
		}
	};
}
rule_set!(ot::SubRuleSet, ot::SubRule, sub_rule);
rule_set!(ot::SubClassSet, ot::SubClassRule, sub_class_rule);
rule_set!(ot::ChainSubRuleSet, ot::ChainSubRule, chain_sub_rule);
rule_set!(
	ot::ChainSubClassSet,
	ot::ChainSubClassRule,
	chain_sub_class_rule
);
rule_set!(ot::PosRuleSet, ot::PosRule, pos_rule);
rule_set!(ot::PosClassSet, ot::PosClassRule, pos_class_rule);
rule_set!(ot::ChainPosRuleSet, ot::ChainPosRule, chain_pos_rule);
rule_set!(
	ot::ChainPosClassSet,
	ot::ChainPosClassRule,
	chain_pos_class_rule
);

/// A format 1 or 2 context subtable.
trait RuleContext {
	type Set: RuleSet + Clone;
	fn coverage(&self) -> &Option<Box<Coverage>>;
	fn coverage_mut(&mut self) -> &mut Option<Box<Coverage>>;
	fn sets(&self) -> &Vec<Option<Box<Self::Set>>>;
	fn sets_mut(&mut self) -> &mut Vec<Option<Box<Self::Set>>>;
	/// `ContextData`: the class definitions of format 2, one per sequence.
	fn class_defs(&self) -> Vec<&Option<Box<ClassDef>>> {
		Vec::new()
	}
	fn class_defs_mut(&mut self) -> Vec<&mut Option<Box<ClassDef>>> {
		Vec::new()
	}
	/// The input class definition (`ClassDef` / `InputClassDef`) of format 2.
	fn input_class_def(&self) -> Option<&ClassDef> {
		None
	}
	/// The index of the input class definition in `class_defs`.
	const INPUT_INDEX: usize;
}

macro_rules! rule_context {
	(1, $t:ty, $set:ty, $sets:ident, $n:literal, $input:literal) => {
		impl RuleContext for $t {
			type Set = $set;
			fn coverage(&self) -> &Option<Box<Coverage>> { &self.coverage }
			fn coverage_mut(&mut self) -> &mut Option<Box<Coverage>> { &mut self.coverage }
			fn sets(&self) -> &Vec<Option<Box<$set>>> { &self.$sets }
			fn sets_mut(&mut self) -> &mut Vec<Option<Box<$set>>> { &mut self.$sets }
			const INPUT_INDEX: usize = $input;
		}
	};
	(2, $t:ty, $set:ty, $sets:ident, [$($cd:ident),*], $input_cd:ident, $input:literal) => {
		impl RuleContext for $t {
			type Set = $set;
			fn coverage(&self) -> &Option<Box<Coverage>> { &self.coverage }
			fn coverage_mut(&mut self) -> &mut Option<Box<Coverage>> { &mut self.coverage }
			fn sets(&self) -> &Vec<Option<Box<$set>>> { &self.$sets }
			fn sets_mut(&mut self) -> &mut Vec<Option<Box<$set>>> { &mut self.$sets }
			fn class_defs(&self) -> Vec<&Option<Box<ClassDef>>> { vec![$(&self.$cd),*] }
			fn class_defs_mut(&mut self) -> Vec<&mut Option<Box<ClassDef>>> { vec![$(&mut self.$cd),*] }
			fn input_class_def(&self) -> Option<&ClassDef> { self.$input_cd.as_deref() }
			const INPUT_INDEX: usize = $input;
		}
	};
}
rule_context!(
	1,
	ot::ContextSubstFormat1,
	ot::SubRuleSet,
	sub_rule_set,
	1,
	0
);
rule_context!(
	1,
	ot::ChainContextSubstFormat1,
	ot::ChainSubRuleSet,
	chain_sub_rule_set,
	3,
	1
);
rule_context!(1, ot::ContextPosFormat1, ot::PosRuleSet, pos_rule_set, 1, 0);
rule_context!(
	1,
	ot::ChainContextPosFormat1,
	ot::ChainPosRuleSet,
	chain_pos_rule_set,
	3,
	1
);
rule_context!(
	2,
	ot::ContextSubstFormat2,
	ot::SubClassSet,
	sub_class_set,
	[class_def],
	class_def,
	0
);
rule_context!(
	2,
	ot::ChainContextSubstFormat2,
	ot::ChainSubClassSet,
	chain_sub_class_set,
	[backtrack_class_def, input_class_def, look_ahead_class_def],
	input_class_def,
	1
);
rule_context!(
	2,
	ot::ContextPosFormat2,
	ot::PosClassSet,
	pos_class_set,
	[class_def],
	class_def,
	0
);
rule_context!(
	2,
	ot::ChainContextPosFormat2,
	ot::ChainPosClassSet,
	chain_pos_class_set,
	[backtrack_class_def, input_class_def, look_ahead_class_def],
	input_class_def,
	1
);

/// A format 3 (coverage-based) context subtable.
trait CoverageContext {
	type R: LookupRec;
	/// `RuleData`: all coverages, in order.
	fn rule_data(&self) -> Vec<&Option<Box<Coverage>>>;
	fn rule_data_mut(&mut self) -> Vec<&mut Option<Box<Coverage>>>;
	/// The input coverages.
	fn input(&self) -> &Vec<Option<Box<Coverage>>>;
	fn records(&self) -> &Vec<Self::R>;
	fn records_mut(&mut self) -> &mut Vec<Self::R>;
}

macro_rules! coverage_context {
	($t:ty, $rec:ty, $records:ident, [$($d:ident),*], $input:ident) => {
		impl CoverageContext for $t {
			type R = $rec;
			fn rule_data(&self) -> Vec<&Option<Box<Coverage>>> {
				let mut v = Vec::new();
				$(v.extend(self.$d.iter());)*
				v
			}
			fn rule_data_mut(&mut self) -> Vec<&mut Option<Box<Coverage>>> {
				let mut v = Vec::new();
				$(v.extend(self.$d.iter_mut());)*
				v
			}
			fn input(&self) -> &Vec<Option<Box<Coverage>>> { &self.$input }
			fn records(&self) -> &Vec<$rec> { &self.$records }
			fn records_mut(&mut self) -> &mut Vec<$rec> { &mut self.$records }
		}
	};
}
coverage_context!(
	ot::ContextSubstFormat3,
	ot::SubstLookupRecord,
	subst_lookup_record,
	[coverage],
	coverage
);
coverage_context!(
	ot::ChainContextSubstFormat3,
	ot::SubstLookupRecord,
	subst_lookup_record,
	[backtrack_coverage, input_coverage, look_ahead_coverage],
	input_coverage
);
coverage_context!(
	ot::ContextPosFormat3,
	ot::PosLookupRecord,
	pos_lookup_record,
	[coverage],
	coverage
);
coverage_context!(
	ot::ChainContextPosFormat3,
	ot::PosLookupRecord,
	pos_lookup_record,
	[backtrack_coverage, input_coverage, look_ahead_coverage],
	input_coverage
);

// Glyph closure over GSUB.

/// The state of the closure over `GSUB` (`s.glyphs`, `s._doneLookups`).
struct Closure<'a> {
	glyphs: &'a mut GlyphSet,
	/// Per lookup: the glyph count when last seen, and the glyphs covered.
	done: HashMap<usize, (usize, GlyphSet)>,
	lookups: &'a [Option<Box<ot::Lookup>>],
}

impl Closure<'_> {
	fn lookup(&self, i: u16) -> Result<&ot::Lookup> {
		self.lookups
			.get(usize::from(i))
			.ok_or_else(|| err("IndexError: list index out of range"))?
			.as_deref()
			.ok_or_else(|| none_attr("closure_glyphs"))
	}

	/// `Lookup.closure_glyphs`.
	fn lookup_closure(&mut self, index: usize, cur_glyphs: Option<GlyphSet>) -> Result<()> {
		let cur = cur_glyphs.unwrap_or_else(|| self.glyphs.clone());
		let len = self.glyphs.len();
		let entry = self.done.entry(index).or_insert((0, GlyphSet::new()));
		if entry.0 != len {
			*entry = (len, GlyphSet::new());
		}
		if cur.is_subset(&entry.1) {
			return Ok(());
		}
		entry.1.extend(&cur);
		let lookups = self.lookups;
		let lookup = lookups[index]
			.as_deref()
			.ok_or_else(|| none_attr("closure_glyphs"))?;
		for st in lookup.sub_table.iter().flatten() {
			self.subtable_closure(st, &cur)?;
		}
		Ok(())
	}

	/// `closure_glyphs` of the subtables.
	fn subtable_closure(&mut self, st: &St, cur: &GlyphSet) -> Result<()> {
		match st {
			St::SingleSubst(t) => {
				for (g, v) in &t.mapping {
					if cur.contains(g) {
						self.glyphs.insert(*v);
					}
				}
			}
			St::MultipleSubst(t) => {
				for (g, subst) in &t.mapping {
					if cur.contains(g) {
						self.glyphs.extend(subst);
					}
				}
			}
			St::AlternateSubst(t) => {
				for (g, alts) in &t.alternates {
					if cur.contains(g) {
						self.glyphs.extend(alts);
					}
				}
			}
			St::LigatureSubst(t) => {
				let mut add = Vec::new();
				for (g, seqs) in &t.ligatures {
					if cur.contains(g) {
						add.extend(
							seqs.iter()
								.filter(|seq| seq.component.iter().all(|c| self.glyphs.contains(c)))
								.map(|seq| seq.lig_glyph),
						);
					}
				}
				self.glyphs.extend(add);
			}
			St::ReverseChainSingleSubst(ot::ReverseChainSingleSubst::Format1(t)) => {
				let indices = cov_intersect(cov(&t.coverage)?, cur);
				if indices.is_empty() {
					return Ok(());
				}
				for c in t.look_ahead_coverage.iter().chain(&t.backtrack_coverage) {
					if cov_intersect(cov(c)?, self.glyphs).is_empty() {
						return Ok(());
					}
				}
				for i in indices {
					let g = *t
						.substitute
						.get(i)
						.ok_or_else(|| err("IndexError: list index out of range"))?;
					self.glyphs.insert(g);
				}
			}
			St::ContextSubst(ot::ContextSubst::Format1(t)) => self.rule_closure(t, cur, false)?,
			St::ContextSubst(ot::ContextSubst::Format2(t)) => self.class_closure(t, cur)?,
			St::ContextSubst(ot::ContextSubst::Format3(t)) => {
				self.coverage_closure(t, t.coverage.first(), cur)?;
			}
			St::ChainContextSubst(ot::ChainContextSubst::Format1(t)) => {
				self.rule_closure(t, cur, true)?;
			}
			St::ChainContextSubst(ot::ChainContextSubst::Format2(t)) => {
				self.class_closure(t, cur)?
			}
			St::ChainContextSubst(ot::ChainContextSubst::Format3(t)) => {
				self.coverage_closure(t, t.input_coverage.first(), cur)?;
			}
			St::ExtensionSubst(ot::ExtensionSubst::Format1(t)) => {
				let sub = t
					.ext_sub_table
					.as_deref()
					.ok_or_else(|| none_attr("closure_glyphs"))?;
				self.subtable_closure(sub, cur)?;
			}
			other => return Err(unsupported_subtable(other)),
		}
		Ok(())
	}

	/// Applies the lookups of a matched rule (the `chaos` loop): positions after
	/// a lookup that may change the glyph count get all glyphs.
	fn apply_records<R: LookupRec>(
		&mut self,
		records: &[R],
		input_len: usize,
		mut pos_glyphs: impl FnMut(&Self, u16) -> Result<GlyphSet>,
	) -> Result<()> {
		let mut chaos = BTreeSet::new();
		for ll in records {
			let seqi = ll.seq();
			let pos = if chaos.contains(&seqi) {
				None
			} else {
				Some(pos_glyphs(self, seqi)?)
			};
			let index = ll.lookup();
			let non_1to1 = may_have_non_1to1(self.lookup(index)?)?;
			chaos.insert(seqi);
			if non_1to1 {
				chaos.extend(seqi..(input_len + 2) as u16);
			}
			self.lookup_closure(usize::from(index), pos)?;
		}
		Ok(())
	}

	/// `closure_glyphs` of format 1 (glyph) rules.
	fn rule_closure<C: RuleContext>(&mut self, t: &C, cur: &GlyphSet, _chain: bool) -> Result<()>
	where
		<<C::Set as RuleSet>::Rule as Rule>::T: Into<u32>,
	{
		let coverage = cov(t.coverage())?;
		let indices = cov_intersect(coverage, cur);
		if indices.is_empty() {
			return Ok(());
		}
		let rss = t.sets();
		for i in indices {
			let Some(Some(rs)) = rss.get(i) else {
				continue;
			};
			for r in rs.rules().iter().flatten() {
				let ok = r
					.data()
					.iter()
					.all(|klist| klist.iter().all(|&k| self.glyphs.contains(&k.into())));
				if !ok {
					continue;
				}
				let first = coverage.glyphs[i];
				let input: Vec<u32> = r.input().iter().map(|&k| k.into()).collect();
				self.apply_records(r.records(), input.len(), |_, seqi| {
					if seqi == 0 {
						Ok(GlyphSet::from([first]))
					} else {
						input
							.get(usize::from(seqi) - 1)
							.map(|&g| GlyphSet::from([g]))
							.ok_or_else(|| err("IndexError: list index out of range"))
					}
				})?;
			}
		}
		Ok(())
	}

	/// `closure_glyphs` of format 2 (class) rules.
	fn class_closure<C: RuleContext>(&mut self, t: &C, cur: &GlyphSet) -> Result<()> {
		let coverage = cov(t.coverage())?;
		if cov_intersect(coverage, cur).is_empty() {
			return Ok(());
		}
		let cur = cov_intersect_glyphs(coverage, cur);
		let class_def = t.input_class_def().ok_or_else(|| none_attr("intersect"))?;
		let indices = cd_intersect(class_def, &cur);
		let context_data = t.class_defs();
		let rss = t.sets();
		for i in indices {
			let Some(Some(rs)) = rss.get(i as usize) else {
				continue;
			};
			for r in rs.rules().iter().flatten() {
				let mut ok = true;
				for (cd, klist) in context_data.iter().zip(r.data()) {
					for &k in klist {
						let k: u32 = k.into();
						let non_empty = match cd {
							Some(cd) => !cd_intersect_class(cd, self.glyphs, k).is_empty(),
							None => k == 0 && !self.glyphs.is_empty(),
						};
						if !non_empty {
							ok = false;
						}
					}
				}
				if !ok {
					continue;
				}
				let input: Vec<u32> = r.input().iter().map(|&k| k.into()).collect();
				let cur = &cur;
				self.apply_records(r.records(), input.len(), |s, seqi| {
					if seqi == 0 {
						Ok(cd_intersect_class(class_def, cur, i))
					} else {
						let k = input
							.get(usize::from(seqi) - 1)
							.ok_or_else(|| err("IndexError: list index out of range"))?;
						Ok(cd_intersect_class(class_def, s.glyphs, *k))
					}
				})?;
			}
		}
		Ok(())
	}

	/// `closure_glyphs` of format 3 (coverage) subtables.
	fn coverage_closure<C: CoverageContext>(
		&mut self,
		t: &C,
		first: Option<&Option<Box<Coverage>>>,
		cur: &GlyphSet,
	) -> Result<()> {
		let first = cov(first.ok_or_else(|| err("IndexError: list index out of range"))?)?;
		if cov_intersect(first, cur).is_empty() {
			return Ok(());
		}
		let cur = cov_intersect_glyphs(first, cur);
		for x in t.rule_data() {
			match x {
				Some(x) if !cov_intersect(x, self.glyphs).is_empty() => {}
				_ => return Ok(()),
			}
		}
		let inputs = t.input();
		self.apply_records(t.records(), inputs.len().saturating_sub(1), |s, seqi| {
			if seqi == 0 {
				Ok(cur.clone())
			} else {
				let c = inputs
					.get(usize::from(seqi))
					.ok_or_else(|| err("IndexError: list index out of range"))?;
				Ok(cov_intersect_glyphs(cov(c)?, s.glyphs))
			}
		})
	}
}

fn unsupported_subtable(st: &St) -> Error {
	Error::Unsupported(format!("lookup subtable {}", subtable_name(st)))
}

fn subtable_name(st: &St) -> String {
	use fontoxide_ttlib::ot::table::OtTable;
	format!("{} format {}", st.class_name(), st.format().unwrap_or(0))
}

/// `may_have_non_1to1` of a lookup: whether it may change the glyph count.
fn may_have_non_1to1(lookup: &ot::Lookup) -> Result<bool> {
	for st in lookup.sub_table.iter().flatten() {
		if subtable_non_1to1(st)? {
			return Ok(true);
		}
	}
	Ok(false)
}

fn subtable_non_1to1(st: &St) -> Result<bool> {
	Ok(match st {
		St::SingleSubst(_) | St::AlternateSubst(_) | St::ReverseChainSingleSubst(_) => false,
		St::MultipleSubst(_)
		| St::LigatureSubst(_)
		| St::ContextSubst(_)
		| St::ChainContextSubst(_) => true,
		St::ExtensionSubst(ot::ExtensionSubst::Format1(t)) => subtable_non_1to1(
			t.ext_sub_table
				.as_deref()
				.ok_or_else(|| none_attr("may_have_non_1to1"))?,
		)?,
		other => return Err(unsupported_subtable(other)),
	})
}

// Subsetting of the subtables.

/// `Device.is_hinting`.
fn is_hinting(d: &ot::Device) -> bool {
	matches!(d.delta_format, 1..=3)
}

/// `ValueRecord.prune_hints`: removes hinting devices.
fn value_prune_hints(v: &mut ValueRecord) {
	for d in [
		&mut v.x_pla_device,
		&mut v.y_pla_device,
		&mut v.x_adv_device,
		&mut v.y_adv_device,
	] {
		if d.as_ref()
			.is_some_and(|d| d.as_deref().is_some_and(is_hinting))
		{
			*d = None;
		}
	}
}

/// `ValueRecord.getEffectiveFormat`: the bits of the non-zero values.
fn effective_format(v: &ValueRecord) -> u16 {
	let mut format = 0;
	for (bit, value) in [
		(0x0001, v.x_placement),
		(0x0002, v.y_placement),
		(0x0004, v.x_advance),
		(0x0008, v.y_advance),
	] {
		if value.is_some_and(|x| x != 0) {
			format |= bit;
		}
	}
	for (bit, d) in [
		(0x0010, &v.x_pla_device),
		(0x0020, &v.y_pla_device),
		(0x0040, &v.x_adv_device),
		(0x0080, &v.y_adv_device),
	] {
		if matches!(d, Some(Some(_))) {
			format |= bit;
		}
	}
	for (i, r) in v.reserved.iter().enumerate() {
		if r.is_some_and(|x| x != 0) {
			format |= 0x0100 << i;
		}
	}
	format
}

/// `Anchor.prune_hints`.
fn anchor_prune_hints(a: &mut ot::Anchor) {
	match a {
		ot::Anchor::Format2(f) => {
			*a = ot::Anchor::Format1(ot::AnchorFormat1 {
				x_coordinate: f.x_coordinate,
				y_coordinate: f.y_coordinate,
			});
		}
		ot::Anchor::Format3(f) => {
			for d in [&mut f.x_device_table, &mut f.y_device_table] {
				if d.as_deref().is_some_and(is_hinting) {
					*d = None;
				}
			}
			if f.x_device_table.is_none() && f.y_device_table.is_none() {
				*a = ot::Anchor::Format1(ot::AnchorFormat1 {
					x_coordinate: f.x_coordinate,
					y_coordinate: f.y_coordinate,
				});
			}
		}
		_ => {}
	}
}

fn anchors_prune_hints<'a>(anchors: impl IntoIterator<Item = &'a mut Option<Box<ot::Anchor>>>) {
	for a in anchors.into_iter().flatten() {
		anchor_prune_hints(a);
	}
}

/// The classes of the kept marks, and the marks renumbered to them.
fn prune_mark_classes(marks: &mut ot::MarkArray) -> Result<Vec<u16>> {
	let class_indices = uniq_sort(marks.mark_record.iter().map(|m| m.class));
	for m in &mut marks.mark_record {
		m.class = index_of(&class_indices, &m.class)? as u16;
	}
	Ok(class_indices)
}

/// The anchors of the kept classes (`_list_subset (anchors, class_indices)`).
fn subset_anchors(anchors: &mut Vec<Option<Box<ot::Anchor>>>, class_indices: &[u16]) {
	let indices: Vec<usize> = class_indices.iter().map(|&c| usize::from(c)).collect();
	*anchors = list_subset(anchors, &indices);
}

fn any_anchor(anchors: &[Option<Box<ot::Anchor>>], class_indices: &[u16]) -> Result<bool> {
	for &j in class_indices {
		match anchors.get(usize::from(j)) {
			Some(a) if a.is_some() => return Ok(true),
			Some(_) => {}
			None => return Err(err("IndexError: list index out of range")),
		}
	}
	Ok(false)
}

/// `subset_glyphs` of a subtable: keeps what applies to `glyphs`; whether
/// anything is left.
fn subtable_subset(st: &mut St, glyphs: &GlyphSet) -> Result<bool> {
	Ok(match st {
		St::SingleSubst(t) => {
			t.mapping
				.retain(|g, v| glyphs.contains(g) && glyphs.contains(v));
			!t.mapping.is_empty()
		}
		St::MultipleSubst(t) => {
			t.mapping
				.retain(|g, v| glyphs.contains(g) && v.iter().all(|s| glyphs.contains(s)));
			!t.mapping.is_empty()
		}
		St::AlternateSubst(t) => {
			t.alternates
				.retain(|g, v| glyphs.contains(g) && v.iter().any(|a| glyphs.contains(a)));
			for v in t.alternates.values_mut() {
				v.retain(|a| glyphs.contains(a));
			}
			!t.alternates.is_empty()
		}
		St::LigatureSubst(t) => {
			t.ligatures.retain(|g, _| glyphs.contains(g));
			for seqs in t.ligatures.values_mut() {
				seqs.retain(|seq| {
					glyphs.contains(&seq.lig_glyph)
						&& seq.component.iter().all(|c| glyphs.contains(c))
				});
			}
			t.ligatures.retain(|_, v| !v.is_empty());
			!t.ligatures.is_empty()
		}
		St::ReverseChainSingleSubst(ot::ReverseChainSingleSubst::Format1(t)) => {
			let indices = cov_subset(cov_mut(&mut t.coverage)?, glyphs);
			t.substitute = list_subset(&t.substitute, &indices);
			// Drop the rules that generate unwanted glyphs.
			let indices: Vec<usize> = t
				.substitute
				.iter()
				.enumerate()
				.filter(|(_, g)| glyphs.contains(g))
				.map(|(i, _)| i)
				.collect();
			t.substitute = list_subset(&t.substitute, &indices);
			cov_remap(cov_mut(&mut t.coverage)?, &indices)?;
			if t.substitute.is_empty() {
				return Ok(false);
			}
			for c in t
				.look_ahead_coverage
				.iter_mut()
				.chain(&mut t.backtrack_coverage)
			{
				if cov_subset(cov_mut(c)?, glyphs).is_empty() {
					return Ok(false);
				}
			}
			true
		}
		St::SinglePos(ot::SinglePos::Format1(t)) => {
			!cov_subset(cov_mut(&mut t.coverage)?, glyphs).is_empty()
		}
		St::SinglePos(ot::SinglePos::Format2(t)) => {
			let indices = cov_subset(cov_mut(&mut t.coverage)?, glyphs);
			t.value = list_subset(&t.value, &indices);
			!t.value.is_empty()
		}
		St::PairPos(ot::PairPos::Format1(t)) => {
			let indices = cov_subset(cov_mut(&mut t.coverage)?, glyphs);
			t.pair_set = list_subset(&t.pair_set, &indices);
			for p in &mut t.pair_set {
				let p = p
					.as_deref_mut()
					.ok_or_else(|| none_attr("PairValueRecord"))?;
				p.pair_value_record
					.retain(|r| glyphs.contains(&r.second_glyph));
			}
			// Remove empty pair sets.
			let indices: Vec<usize> = t
				.pair_set
				.iter()
				.enumerate()
				.filter(|(_, p)| p.as_ref().is_some_and(|p| !p.pair_value_record.is_empty()))
				.map(|(i, _)| i)
				.collect();
			cov_remap(cov_mut(&mut t.coverage)?, &indices)?;
			t.pair_set = list_subset(&t.pair_set, &indices);
			!t.pair_set.is_empty()
		}
		St::PairPos(ot::PairPos::Format2(t)) => {
			let class1_count = t.class1_record.len() as u32;
			let class2_count = t.class1_record.first().map_or(0, |r| r.class2_record.len()) as u32;
			let covered: GlyphSet = cov(&t.coverage)?
				.glyphs
				.iter()
				.copied()
				.filter(|g| glyphs.contains(g))
				.collect();
			let cd1 = t
				.class_def1
				.as_deref_mut()
				.ok_or_else(|| none_attr("subset"))?;
			let class1_map: Vec<u32> = cd_subset(cd1, &covered, true, true)
				.into_iter()
				.filter(|&c| c < class1_count)
				.collect();
			let cd2 = t
				.class_def2
				.as_deref_mut()
				.ok_or_else(|| none_attr("subset"))?;
			let class2_map: Vec<u32> = cd_subset(cd2, glyphs, true, false)
				.into_iter()
				.filter(|&c| c < class2_count)
				.collect();
			let rows: Vec<usize> = class1_map.iter().map(|&c| c as usize).collect();
			let cols: Vec<usize> = class2_map.iter().map(|&c| c as usize).collect();
			t.class1_record = list_subset(&t.class1_record, &rows);
			for r in &mut t.class1_record {
				r.class2_record = list_subset(&r.class2_record, &cols);
			}
			// If only class 2 0 is left, nothing needs to be kept.
			!class1_map.is_empty()
				&& class2_map.len() > 1
				&& !cov_subset(cov_mut(&mut t.coverage)?, glyphs).is_empty()
		}
		St::CursivePos(ot::CursivePos::Format1(t)) => {
			let indices = cov_subset(cov_mut(&mut t.coverage)?, glyphs);
			t.entry_exit_record = list_subset(&t.entry_exit_record, &indices);
			!t.entry_exit_record.is_empty()
		}
		St::MarkBasePos(ot::MarkBasePos::Format1(t)) => {
			let mark_indices = cov_subset(cov_mut(&mut t.mark_coverage)?, glyphs);
			let marks = t
				.mark_array
				.as_deref_mut()
				.ok_or_else(|| none_attr("MarkRecord"))?;
			marks.mark_record = list_subset(&marks.mark_record, &mark_indices);
			let class_indices = uniq_sort(marks.mark_record.iter().map(|m| m.class));
			let base = cov(&t.base_coverage)?;
			let intersect = cov_intersect(base, glyphs);
			let records = &t
				.base_array
				.as_deref()
				.ok_or_else(|| none_attr("BaseRecord"))?
				.base_record;
			let mut base_indices = Vec::new();
			for i in intersect {
				if i < records.len() && any_anchor(&records[i].base_anchor, &class_indices)? {
					base_indices.push(i);
				}
			}
			if base_indices.is_empty() {
				return Ok(false);
			}
			cov_remap(cov_mut(&mut t.base_coverage)?, &base_indices)?;
			let base_array = t
				.base_array
				.as_deref_mut()
				.ok_or_else(|| none_attr("BaseRecord"))?;
			base_array.base_record = list_subset(&base_array.base_record, &base_indices);
			// Prune empty classes.
			let marks = t
				.mark_array
				.as_deref_mut()
				.ok_or_else(|| none_attr("MarkRecord"))?;
			prune_mark_classes(marks)?;
			t.class_count = Some(class_indices.len() as u32);
			for b in &mut base_array.base_record {
				subset_anchors(&mut b.base_anchor, &class_indices);
			}
			!class_indices.is_empty()
				&& !marks.mark_record.is_empty()
				&& !base_array.base_record.is_empty()
		}
		St::MarkLigPos(ot::MarkLigPos::Format1(t)) => {
			let mark_indices = cov_subset(cov_mut(&mut t.mark_coverage)?, glyphs);
			let marks = t
				.mark_array
				.as_deref_mut()
				.ok_or_else(|| none_attr("MarkRecord"))?;
			marks.mark_record = list_subset(&marks.mark_record, &mark_indices);
			let class_indices = uniq_sort(marks.mark_record.iter().map(|m| m.class));
			let intersect = cov_intersect(cov(&t.ligature_coverage)?, glyphs);
			let ligatures = t
				.ligature_array
				.as_deref()
				.ok_or_else(|| none_attr("LigatureAttach"))?;
			let num_ligatures = ligatures.ligature_attach.len();
			let mut ligature_indices = Vec::new();
			for i in intersect {
				if i >= num_ligatures {
					continue;
				}
				let attach = ligatures.ligature_attach[i]
					.as_deref()
					.ok_or_else(|| none_attr("ComponentRecord"))?;
				let mut any = false;
				for component in &attach.component_record {
					if any_anchor(&component.ligature_anchor, &class_indices)? {
						any = true;
						break;
					}
				}
				if any {
					ligature_indices.push(i);
				}
			}
			if ligature_indices.is_empty() {
				return Ok(false);
			}
			cov_remap(cov_mut(&mut t.ligature_coverage)?, &ligature_indices)?;
			let ligatures = t
				.ligature_array
				.as_deref_mut()
				.ok_or_else(|| none_attr("LigatureAttach"))?;
			ligatures.ligature_attach = list_subset(&ligatures.ligature_attach, &ligature_indices);
			let marks = t
				.mark_array
				.as_deref_mut()
				.ok_or_else(|| none_attr("MarkRecord"))?;
			prune_mark_classes(marks)?;
			t.class_count = Some(class_indices.len() as u32);
			for l in ligatures.ligature_attach.iter_mut().flatten() {
				for c in &mut l.component_record {
					subset_anchors(&mut c.ligature_anchor, &class_indices);
				}
			}
			!class_indices.is_empty()
				&& !marks.mark_record.is_empty()
				&& !ligatures.ligature_attach.is_empty()
		}
		St::MarkMarkPos(ot::MarkMarkPos::Format1(t)) => {
			let mark1_indices = cov_subset(cov_mut(&mut t.mark1_coverage)?, glyphs);
			let marks = t
				.mark1_array
				.as_deref_mut()
				.ok_or_else(|| none_attr("MarkRecord"))?;
			marks.mark_record = list_subset(&marks.mark_record, &mark1_indices);
			let class_indices = uniq_sort(marks.mark_record.iter().map(|m| m.class));
			let intersect = cov_intersect(cov(&t.mark2_coverage)?, glyphs);
			let records = &t
				.mark2_array
				.as_deref()
				.ok_or_else(|| none_attr("Mark2Record"))?
				.mark2_record;
			let mut mark2_indices = Vec::new();
			for i in intersect {
				if i < records.len() && any_anchor(&records[i].mark2_anchor, &class_indices)? {
					mark2_indices.push(i);
				}
			}
			if mark2_indices.is_empty() {
				return Ok(false);
			}
			cov_remap(cov_mut(&mut t.mark2_coverage)?, &mark2_indices)?;
			let mark2 = t
				.mark2_array
				.as_deref_mut()
				.ok_or_else(|| none_attr("Mark2Record"))?;
			mark2.mark2_record = list_subset(&mark2.mark2_record, &mark2_indices);
			let marks = t
				.mark1_array
				.as_deref_mut()
				.ok_or_else(|| none_attr("MarkRecord"))?;
			prune_mark_classes(marks)?;
			t.class_count = Some(class_indices.len() as u32);
			for b in &mut mark2.mark2_record {
				subset_anchors(&mut b.mark2_anchor, &class_indices);
			}
			!class_indices.is_empty()
				&& !marks.mark_record.is_empty()
				&& !mark2.mark2_record.is_empty()
		}
		St::ContextSubst(ot::ContextSubst::Format1(t)) => rule_subset(t, glyphs)?,
		St::ContextSubst(ot::ContextSubst::Format2(t)) => class_subset(t, glyphs)?,
		St::ContextSubst(ot::ContextSubst::Format3(t)) => coverage_subset(t, glyphs)?,
		St::ChainContextSubst(ot::ChainContextSubst::Format1(t)) => rule_subset(t, glyphs)?,
		St::ChainContextSubst(ot::ChainContextSubst::Format2(t)) => class_subset(t, glyphs)?,
		St::ChainContextSubst(ot::ChainContextSubst::Format3(t)) => coverage_subset(t, glyphs)?,
		St::ContextPos(ot::ContextPos::Format1(t)) => rule_subset(t, glyphs)?,
		St::ContextPos(ot::ContextPos::Format2(t)) => class_subset(t, glyphs)?,
		St::ContextPos(ot::ContextPos::Format3(t)) => coverage_subset(t, glyphs)?,
		St::ChainContextPos(ot::ChainContextPos::Format1(t)) => rule_subset(t, glyphs)?,
		St::ChainContextPos(ot::ChainContextPos::Format2(t)) => class_subset(t, glyphs)?,
		St::ChainContextPos(ot::ChainContextPos::Format3(t)) => coverage_subset(t, glyphs)?,
		St::ExtensionSubst(ot::ExtensionSubst::Format1(ot::ExtensionSubstFormat1 {
			ext_sub_table,
			..
		}))
		| St::ExtensionPos(ot::ExtensionPos::Format1(ot::ExtensionPosFormat1 {
			ext_sub_table,
			..
		})) => subtable_subset(
			ext_sub_table
				.as_deref_mut()
				.ok_or_else(|| none_attr("subset_glyphs"))?,
			glyphs,
		)?,
		other => return Err(unsupported_subtable(other)),
	})
}

/// `subset_glyphs` of format 1 (glyph) context subtables.
fn rule_subset<C: RuleContext>(t: &mut C, glyphs: &GlyphSet) -> Result<bool> {
	let indices = cov_subset(cov_mut(t.coverage_mut())?, glyphs);
	let mut rss = list_subset(t.sets(), &indices);
	for rs in rss.iter_mut().flatten() {
		rs.rules_mut().retain(|r| {
			r.as_deref().is_some_and(|r| {
				r.data()
					.iter()
					.all(|glist| glist.iter().all(|&g| glyphs.contains(&g.into())))
			})
		});
	}
	// Prune empty rule sets.
	let indices: Vec<usize> = rss
		.iter()
		.enumerate()
		.filter(|(_, rs)| rs.as_ref().is_some_and(|rs| !rs.rules().is_empty()))
		.map(|(i, _)| i)
		.collect();
	cov_remap(cov_mut(t.coverage_mut())?, &indices)?;
	*t.sets_mut() = list_subset(&rss, &indices);
	Ok(!t.sets().is_empty())
}

/// `subset_glyphs` of format 2 (class) context subtables.
fn class_subset<C: RuleContext>(t: &mut C, glyphs: &GlyphSet) -> Result<bool> {
	if cov_subset(cov_mut(t.coverage_mut())?, glyphs).is_empty() {
		return Ok(false);
	}
	let klass_maps: Vec<Option<Vec<u32>>> = t
		.class_defs_mut()
		.into_iter()
		.map(|x| x.as_deref_mut().map(|cd| cd_subset(cd, glyphs, true, true)))
		.collect();
	// Keep the rule sets of the classes that survived.
	let indices: Vec<usize> = klass_maps[C::INPUT_INDEX]
		.as_ref()
		.ok_or_else(|| err("TypeError: 'NoneType' object is not iterable"))?
		.iter()
		.map(|&c| c as usize)
		.collect();
	let rss = list_subset(t.sets(), &indices);
	// Delete, but don't renumber, unreachable rule sets.
	let class_def = t.input_class_def().ok_or_else(|| none_attr("intersect"))?;
	let coverage: GlyphSet = cov(t.coverage())?.glyphs.iter().copied().collect();
	let reachable = cd_intersect(class_def, &coverage);
	let mut rss: Vec<_> = rss
		.into_iter()
		.enumerate()
		.map(|(i, rs)| {
			if reachable.contains(&(i as u32)) {
				rs
			} else {
				None
			}
		})
		.collect();
	for rs in rss.iter_mut().flatten() {
		let mut kept = Vec::new();
		for r in std::mem::take(rs.rules_mut()) {
			let Some(mut r) = r else {
				continue;
			};
			let mut ok = true;
			for (map, klist) in klass_maps.iter().zip(r.data()) {
				for &k in klist {
					match map {
						Some(m) if m.contains(&k.into()) => {}
						Some(_) => ok = false,
						None => {
							return Err(err(
								"TypeError: argument of type 'NoneType' is not iterable",
							));
						}
					}
				}
			}
			if !ok {
				continue;
			}
			// Remap the rule classes.
			for (map, klist) in klass_maps.iter().zip(r.data_mut()) {
				let Some(map) = map else {
					continue;
				};
				for k in klist.iter_mut() {
					let new = index_of(map, &(*k).into())? as u32;
					*k = new
						.try_into()
						.map_err(|_| err("OverflowError: class value too large"))?;
				}
			}
			kept.push(Some(r));
		}
		*rs.rules_mut() = kept;
	}
	// Prune empty rule sets.
	for rs in &mut rss {
		if rs.as_ref().is_some_and(|rs| rs.rules().is_empty()) {
			*rs = None;
		}
	}
	while rss.last().is_some_and(Option::is_none) {
		rss.pop();
	}
	*t.sets_mut() = rss;
	Ok(!t.sets().is_empty())
}

/// `subset_glyphs` of format 3 (coverage) context subtables.
fn coverage_subset<C: CoverageContext>(t: &mut C, glyphs: &GlyphSet) -> Result<bool> {
	for x in t.rule_data_mut() {
		let Some(c) = x.as_deref_mut() else {
			return Ok(false);
		};
		if cov_subset(c, glyphs).is_empty() {
			return Ok(false);
		}
	}
	Ok(true)
}

/// `subset_lookups` of a subtable: keeps the lookup records of the kept lookups,
/// renumbered.
fn subtable_subset_lookups(st: &mut St, lookup_indices: &[u16]) -> Result<()> {
	fn records<R: LookupRec>(records: &mut Vec<R>, lookup_indices: &[u16]) -> Result<()> {
		records.retain(|ll| lookup_indices.contains(&ll.lookup()));
		for ll in records.iter_mut() {
			let i = lookup_indices
				.iter()
				.position(|&l| l == ll.lookup())
				.ok_or_else(|| err("ValueError: x not in list"))?;
			ll.set_lookup(i as u16);
		}
		Ok(())
	}
	fn rules<C: RuleContext>(t: &mut C, lookup_indices: &[u16]) -> Result<()> {
		for rs in t.sets_mut().iter_mut().flatten() {
			for r in rs.rules_mut().iter_mut().flatten() {
				records(r.records_mut(), lookup_indices)?;
			}
		}
		Ok(())
	}
	match st {
		St::ContextSubst(ot::ContextSubst::Format1(t)) => rules(t, lookup_indices),
		St::ContextSubst(ot::ContextSubst::Format2(t)) => rules(t, lookup_indices),
		St::ContextSubst(ot::ContextSubst::Format3(t)) => records(t.records_mut(), lookup_indices),
		St::ChainContextSubst(ot::ChainContextSubst::Format1(t)) => rules(t, lookup_indices),
		St::ChainContextSubst(ot::ChainContextSubst::Format2(t)) => rules(t, lookup_indices),
		St::ChainContextSubst(ot::ChainContextSubst::Format3(t)) => {
			records(t.records_mut(), lookup_indices)
		}
		St::ContextPos(ot::ContextPos::Format1(t)) => rules(t, lookup_indices),
		St::ContextPos(ot::ContextPos::Format2(t)) => rules(t, lookup_indices),
		St::ContextPos(ot::ContextPos::Format3(t)) => records(t.records_mut(), lookup_indices),
		St::ChainContextPos(ot::ChainContextPos::Format1(t)) => rules(t, lookup_indices),
		St::ChainContextPos(ot::ChainContextPos::Format2(t)) => rules(t, lookup_indices),
		St::ChainContextPos(ot::ChainContextPos::Format3(t)) => {
			records(t.records_mut(), lookup_indices)
		}
		St::ExtensionSubst(ot::ExtensionSubst::Format1(ot::ExtensionSubstFormat1 {
			ext_sub_table,
			..
		}))
		| St::ExtensionPos(ot::ExtensionPos::Format1(ot::ExtensionPosFormat1 {
			ext_sub_table,
			..
		})) => subtable_subset_lookups(
			ext_sub_table
				.as_deref_mut()
				.ok_or_else(|| none_attr("subset_lookups"))?,
			lookup_indices,
		),
		St::SingleSubst(_)
		| St::MultipleSubst(_)
		| St::AlternateSubst(_)
		| St::LigatureSubst(_)
		| St::ReverseChainSingleSubst(_)
		| St::SinglePos(_)
		| St::PairPos(_)
		| St::CursivePos(_)
		| St::MarkBasePos(_)
		| St::MarkLigPos(_)
		| St::MarkMarkPos(_) => Ok(()),
		other => Err(unsupported_subtable(other)),
	}
}

/// `collect_lookups` of a subtable: the lookups its rules refer to.
fn subtable_collect_lookups(st: &St) -> Result<Vec<u16>> {
	fn rules<C: RuleContext>(t: &C) -> Vec<u16> {
		t.sets()
			.iter()
			.flatten()
			.flat_map(|rs| rs.rules().iter().flatten())
			.flat_map(|r| r.records().iter().map(LookupRec::lookup))
			.collect()
	}
	fn records<R: LookupRec>(r: &[R]) -> Vec<u16> {
		r.iter().map(LookupRec::lookup).collect()
	}
	Ok(match st {
		St::ContextSubst(ot::ContextSubst::Format1(t)) => rules(t),
		St::ContextSubst(ot::ContextSubst::Format2(t)) => rules(t),
		St::ContextSubst(ot::ContextSubst::Format3(t)) => records(t.records()),
		St::ChainContextSubst(ot::ChainContextSubst::Format1(t)) => rules(t),
		St::ChainContextSubst(ot::ChainContextSubst::Format2(t)) => rules(t),
		St::ChainContextSubst(ot::ChainContextSubst::Format3(t)) => records(t.records()),
		St::ContextPos(ot::ContextPos::Format1(t)) => rules(t),
		St::ContextPos(ot::ContextPos::Format2(t)) => rules(t),
		St::ContextPos(ot::ContextPos::Format3(t)) => records(t.records()),
		St::ChainContextPos(ot::ChainContextPos::Format1(t)) => rules(t),
		St::ChainContextPos(ot::ChainContextPos::Format2(t)) => rules(t),
		St::ChainContextPos(ot::ChainContextPos::Format3(t)) => records(t.records()),
		St::ExtensionSubst(ot::ExtensionSubst::Format1(ot::ExtensionSubstFormat1 {
			ext_sub_table,
			..
		}))
		| St::ExtensionPos(ot::ExtensionPos::Format1(ot::ExtensionPosFormat1 {
			ext_sub_table,
			..
		})) => subtable_collect_lookups(
			ext_sub_table
				.as_deref()
				.ok_or_else(|| none_attr("collect_lookups"))?,
		)?,
		St::SingleSubst(_)
		| St::MultipleSubst(_)
		| St::AlternateSubst(_)
		| St::LigatureSubst(_)
		| St::ReverseChainSingleSubst(_)
		| St::SinglePos(_)
		| St::PairPos(_)
		| St::CursivePos(_)
		| St::MarkBasePos(_)
		| St::MarkLigPos(_)
		| St::MarkMarkPos(_) => Vec::new(),
		other => return Err(unsupported_subtable(other)),
	})
}

/// `prune_post_subset` of a subtable.
fn subtable_prune_post_subset(st: &mut St, options: &Options) -> Result<bool> {
	Ok(match st {
		St::SinglePos(sp) => {
			let mut downgrade = None;
			match sp {
				ot::SinglePos::Format1(t) => {
					let Some(v) = &mut t.value else {
						return Ok(true);
					};
					if !options.hinting {
						value_prune_hints(v);
					}
					t.value_format = effective_format(v);
				}
				ot::SinglePos::Format2(t) => {
					if t.value.iter().any(Option::is_none) {
						// Records without values: ValueFormat is 0.
					} else {
						if !options.hinting {
							for v in t.value.iter_mut().flatten() {
								value_prune_hints(v);
							}
						}
						t.value_format = t
							.value
							.iter()
							.flatten()
							.fold(0, |f, v| f | effective_format(v));
					}
					// Downgrade to format 1 if all values are the same.
					if t.value.iter().all(|v| Some(v) == t.value.first()) {
						let value = if t.value_format != 0 {
							t.value.first().cloned().flatten()
						} else {
							None
						};
						downgrade = Some(ot::SinglePosFormat1 {
							coverage: t.coverage.take(),
							value_format: t.value_format,
							value,
							dont_share: t.dont_share,
						});
					}
				}
				other => return Err(unknown_format(other_format(other))),
			}
			if let Some(f1) = downgrade {
				*sp = ot::SinglePos::Format1(f1);
			}
			true
		}
		St::PairPos(pp) => {
			let (vf1, vf2) = match pp {
				ot::PairPos::Format1(t) => {
					if !options.hinting {
						t.value_format1 = 0;
						t.value_format2 = 0;
						for p in t.pair_set.iter_mut().flatten() {
							for r in &mut p.pair_value_record {
								if let Some(v) = &mut r.value1 {
									value_prune_hints(v);
									t.value_format1 |= effective_format(v);
								}
								if let Some(v) = &mut r.value2 {
									value_prune_hints(v);
									t.value_format2 |= effective_format(v);
								}
							}
						}
					}
					(t.value_format1, t.value_format2)
				}
				ot::PairPos::Format2(t) => {
					if !options.hinting {
						t.value_format1 = 0;
						t.value_format2 = 0;
						for c in &mut t.class1_record {
							for r in &mut c.class2_record {
								if let Some(v) = &mut r.value1 {
									value_prune_hints(v);
									t.value_format1 |= effective_format(v);
								}
								if let Some(v) = &mut r.value2 {
									value_prune_hints(v);
									t.value_format2 |= effective_format(v);
								}
							}
						}
					}
					(t.value_format1, t.value_format2)
				}
				other => return Err(unknown_format(pair_format(other))),
			};
			vf1 | vf2 != 0
		}
		St::CursivePos(ot::CursivePos::Format1(t)) => {
			if !options.hinting {
				for rec in &mut t.entry_exit_record {
					anchors_prune_hints([&mut rec.entry_anchor, &mut rec.exit_anchor]);
				}
			}
			true
		}
		St::MarkBasePos(ot::MarkBasePos::Format1(t)) => {
			if !options.hinting {
				if let Some(m) = &mut t.mark_array {
					anchors_prune_hints(m.mark_record.iter_mut().map(|m| &mut m.mark_anchor));
				}
				if let Some(b) = &mut t.base_array {
					anchors_prune_hints(b.base_record.iter_mut().flat_map(|b| &mut b.base_anchor));
				}
			}
			true
		}
		St::MarkLigPos(ot::MarkLigPos::Format1(t)) => {
			if !options.hinting {
				if let Some(m) = &mut t.mark_array {
					anchors_prune_hints(m.mark_record.iter_mut().map(|m| &mut m.mark_anchor));
				}
				if let Some(l) = &mut t.ligature_array {
					anchors_prune_hints(
						l.ligature_attach
							.iter_mut()
							.flatten()
							.flat_map(|l| &mut l.component_record)
							.flat_map(|c| &mut c.ligature_anchor),
					);
				}
			}
			true
		}
		St::MarkMarkPos(ot::MarkMarkPos::Format1(t)) => {
			if !options.hinting {
				if let Some(m) = &mut t.mark1_array {
					anchors_prune_hints(m.mark_record.iter_mut().map(|m| &mut m.mark_anchor));
				}
				if let Some(m) = &mut t.mark2_array {
					anchors_prune_hints(
						m.mark2_record.iter_mut().flat_map(|b| &mut b.mark2_anchor),
					);
				}
			}
			true
		}
		St::SingleSubst(_)
		| St::MultipleSubst(_)
		| St::AlternateSubst(_)
		| St::LigatureSubst(_)
		| St::ReverseChainSingleSubst(_)
		| St::ContextSubst(_)
		| St::ChainContextSubst(_)
		| St::ContextPos(_)
		| St::ChainContextPos(_) => true,
		St::ExtensionSubst(ot::ExtensionSubst::Format1(ot::ExtensionSubstFormat1 {
			ext_sub_table,
			..
		}))
		| St::ExtensionPos(ot::ExtensionPos::Format1(ot::ExtensionPosFormat1 {
			ext_sub_table,
			..
		})) => subtable_prune_post_subset(
			ext_sub_table
				.as_deref_mut()
				.ok_or_else(|| none_attr("prune_post_subset"))?,
			options,
		)?,
		other => return Err(unsupported_subtable(other)),
	})
}

fn other_format(p: &ot::SinglePos) -> u16 {
	use fontoxide_ttlib::ot::table::OtTable;
	p.format().unwrap_or(0)
}

fn pair_format(p: &ot::PairPos) -> u16 {
	use fontoxide_ttlib::ot::table::OtTable;
	p.format().unwrap_or(0)
}

// Lookups and lookup lists.

/// `Lookup.subset_glyphs`.
fn lookup_subset_glyphs(
	l: &mut ot::Lookup,
	glyphs: &GlyphSet,
	used_mark_sets: &[u32],
) -> Result<bool> {
	let mut kept = Vec::new();
	for st in std::mem::take(&mut l.sub_table) {
		if let Some(mut st) = st
			&& subtable_subset(&mut st, glyphs)?
		{
			kept.push(Some(st));
		}
	}
	l.sub_table = kept;
	if l.lookup_flag & 0x0010 != 0 {
		match used_mark_sets
			.iter()
			.position(|&m| m == u32::from(l.mark_filtering_set))
		{
			Some(i) => l.mark_filtering_set = i as u16,
			None => {
				l.mark_filtering_set = 0;
				l.lookup_flag &= !0x10;
				l.lookup_flag |= 0x8;
			}
		}
	}
	Ok(!l.sub_table.is_empty())
}

/// `Lookup.collect_lookups`.
fn lookup_collect_lookups(l: &ot::Lookup) -> Result<Vec<u16>> {
	let mut out = Vec::new();
	for st in l.sub_table.iter().flatten() {
		out.extend(subtable_collect_lookups(st)?);
	}
	Ok(out)
}

/// `LookupList.closure_lookups`: the lookups reachable from `lookup_indices`.
fn closure_lookups(list: &ot::LookupList, lookup_indices: Vec<u16>) -> Result<Vec<u16>> {
	let mut lookup_indices = uniq_sort(lookup_indices);
	let count = list.lookup.len();
	let mut recurse = lookup_indices.clone();
	loop {
		let mut recurse_lookups = Vec::new();
		for &i in &recurse {
			if usize::from(i) < count {
				let l = list.lookup[usize::from(i)]
					.as_deref()
					.ok_or_else(|| none_attr("collect_lookups"))?;
				recurse_lookups.extend(lookup_collect_lookups(l)?);
			}
		}
		recurse_lookups.retain(|l| !lookup_indices.contains(l) && usize::from(*l) < count);
		if recurse_lookups.is_empty() {
			return Ok(uniq_sort(lookup_indices));
		}
		let recurse_lookups = uniq_sort(recurse_lookups);
		lookup_indices.extend(&recurse_lookups);
		recurse = recurse_lookups;
	}
}

// Features, scripts and language systems.

/// `Feature.subset_lookups`: whether the feature is kept.
fn feature_subset_lookups(f: &mut ot::Feature, lookup_indices: &[u16]) -> bool {
	f.lookup_list_index = f
		.lookup_list_index
		.iter()
		.filter_map(|l| lookup_indices.iter().position(|x| x == l).map(|i| i as u16))
		.collect();
	// The `size` feature is kept even without lookups.
	!f.lookup_list_index.is_empty()
		|| matches!(
			f.feature_params.as_deref(),
			Some(ot::FeatureParamsAny::FeatureParamsSize(_))
		)
}

/// `FeatureRecord == FeatureRecord` (`BaseTable.__eq__`), without the counts
/// fontTools keeps in step with the lists.
fn feature_eq(a: &ot::FeatureRecord, b: &ot::FeatureRecord) -> bool {
	a.feature_tag == b.feature_tag
		&& match (&a.feature, &b.feature) {
			(Some(x), Some(y)) => {
				x.lookup_list_index == y.lookup_list_index && x.feature_params == y.feature_params
			}
			(None, None) => true,
			_ => false,
		}
}

/// `LangSys.subset_features`.
fn langsys_subset_features(l: &mut ot::LangSys, feature_indices: &[u16]) -> bool {
	l.req_feature_index = feature_indices
		.binary_search(&l.req_feature_index)
		.map_or(65535, |i| i as u16);
	l.feature_index = l
		.feature_index
		.iter()
		.filter_map(|f| feature_indices.binary_search(f).ok().map(|i| i as u16))
		.collect();
	!l.feature_index.is_empty() || l.req_feature_index != 65535
}

/// `LangSys.prune_features`.
fn langsys_prune_features(l: &mut ot::LangSys, map: &std::collections::BTreeMap<u16, u16>) -> bool {
	l.req_feature_index = map.get(&l.req_feature_index).copied().unwrap_or(65535);
	l.feature_index = l
		.feature_index
		.iter()
		.filter_map(|f| map.get(f).copied())
		.collect();
	!l.feature_index.is_empty() || l.req_feature_index != 65535
}

/// `LangSys.collect_features`.
fn langsys_collect_features(l: &ot::LangSys) -> Vec<u16> {
	let mut v = l.feature_index.clone();
	if l.req_feature_index != 65535 {
		v.push(l.req_feature_index);
	}
	uniq_sort(v)
}

/// The tables of a `GSUB` or `GPOS` table.
trait Layout {
	const IS_GSUB: bool;
	fn version(&mut self) -> &mut u32;
	fn script_list(&mut self) -> &mut Option<Box<ot::ScriptList>>;
	fn feature_list(&mut self) -> &mut Option<Box<ot::FeatureList>>;
	fn lookup_list(&mut self) -> &mut Option<Box<ot::LookupList>>;
	fn feature_variations(&mut self) -> &mut Option<Box<ot::FeatureVariations>>;
}

macro_rules! layout {
	($t:ty, $gsub:literal) => {
		impl Layout for $t {
			const IS_GSUB: bool = $gsub;
			fn version(&mut self) -> &mut u32 {
				&mut self.version
			}
			fn script_list(&mut self) -> &mut Option<Box<ot::ScriptList>> {
				&mut self.script_list
			}
			fn feature_list(&mut self) -> &mut Option<Box<ot::FeatureList>> {
				&mut self.feature_list
			}
			fn lookup_list(&mut self) -> &mut Option<Box<ot::LookupList>> {
				&mut self.lookup_list
			}
			fn feature_variations(&mut self) -> &mut Option<Box<ot::FeatureVariations>> {
				&mut self.feature_variations
			}
		}
	};
}
layout!(ot::GSUB, true);
layout!(ot::GPOS, false);

/// `ScriptList.collect_features`.
fn collect_features<T: Layout>(t: &mut T) -> Vec<u16> {
	let Some(sl) = t.script_list() else {
		return Vec::new();
	};
	let mut v = Vec::new();
	for s in &sl.script_record {
		let Some(script) = &s.script else {
			continue;
		};
		for l in &script.lang_sys_record {
			if let Some(l) = &l.lang_sys {
				v.extend(langsys_collect_features(l));
			}
		}
		if let Some(d) = &script.default_lang_sys {
			v.extend(langsys_collect_features(d));
		}
	}
	uniq_sort(v)
}

/// `FeatureList.collect_lookups` and `FeatureVariations.collect_lookups`.
fn collect_lookups<T: Layout>(t: &mut T, feature_indices: &[u16]) -> Vec<u16> {
	let mut lookups = Vec::new();
	if let Some(fl) = t.feature_list() {
		for &i in feature_indices {
			if let Some(Some(f)) = fl.feature_record.get(usize::from(i)).map(|r| &r.feature) {
				lookups.extend(&f.lookup_list_index);
			}
		}
	}
	if let Some(fv) = t.feature_variations() {
		for vr in &fv.feature_variation_record {
			if let Some(sub) = &vr.feature_table_substitution {
				for r in &sub.substitution_record {
					if feature_indices.contains(&r.feature_index)
						&& let Some(f) = &r.feature
					{
						lookups.extend(&f.lookup_list_index);
					}
				}
			}
		}
		for record in &fv.lookup_variation_record {
			if feature_indices.contains(&record.feature_index)
				&& let Some(flt) = &record.feature_lookups_table
			{
				for r in &flt.lookup_condition_record {
					if let Some(l) = &r.lookup_index_list {
						lookups.extend(&l.lookup_index);
					}
				}
			}
		}
	}
	lookups
}

/// `ScriptList.subset_features` (`retain_empty` for `GSUB`).
fn script_list_subset_features<T: Layout>(t: &mut T, feature_indices: &[u16]) -> Result<()> {
	let retain_empty = T::IS_GSUB;
	let Some(sl) = t.script_list() else {
		return Ok(());
	};
	let mut kept = Vec::new();
	for mut s in std::mem::take(&mut sl.script_record) {
		let keep_empty_default = s.script_tag == *b"DFLT";
		let script = s
			.script
			.as_deref_mut()
			.ok_or_else(|| none_attr("subset_features"))?;
		if let Some(d) = &mut script.default_lang_sys
			&& !langsys_subset_features(d, feature_indices)
			&& !keep_empty_default
		{
			script.default_lang_sys = None;
		}
		let mut langs = Vec::new();
		for mut l in std::mem::take(&mut script.lang_sys_record) {
			let ls = l
				.lang_sys
				.as_deref_mut()
				.ok_or_else(|| none_attr("subset_features"))?;
			if langsys_subset_features(ls, feature_indices) {
				langs.push(l);
			}
		}
		script.lang_sys_record = langs;
		let non_empty = !script.lang_sys_record.is_empty() || script.default_lang_sys.is_some();
		if non_empty || retain_empty {
			kept.push(s);
		}
	}
	sl.script_record = kept;
	Ok(())
}

/// `ScriptList.prune_features`.
fn script_list_prune_features<T: Layout>(
	t: &mut T,
	map: &std::collections::BTreeMap<u16, u16>,
) -> Result<()> {
	let retain_empty = T::IS_GSUB;
	let Some(sl) = t.script_list() else {
		return Ok(());
	};
	let mut kept = Vec::new();
	for mut s in std::mem::take(&mut sl.script_record) {
		let keep_empty_default = s.script_tag == *b"DFLT";
		let script = s
			.script
			.as_deref_mut()
			.ok_or_else(|| none_attr("prune_features"))?;
		if let Some(d) = &mut script.default_lang_sys
			&& !langsys_prune_features(d, map)
			&& !keep_empty_default
		{
			script.default_lang_sys = None;
		}
		let mut langs = Vec::new();
		for mut l in std::mem::take(&mut script.lang_sys_record) {
			let ls = l
				.lang_sys
				.as_deref_mut()
				.ok_or_else(|| none_attr("prune_features"))?;
			if langsys_prune_features(ls, map) {
				langs.push(l);
			}
		}
		script.lang_sys_record = langs;
		let non_empty = !script.lang_sys_record.is_empty() || script.default_lang_sys.is_some();
		if non_empty || retain_empty {
			kept.push(s);
		}
	}
	sl.script_record = kept;
	Ok(())
}

/// `FeatureList.subset_features` and `FeatureVariations.subset_features`.
fn subset_features<T: Layout>(t: &mut T, feature_indices: &[u16], variations: bool) {
	if let Some(fl) = t.feature_list() {
		let indices: Vec<usize> = feature_indices.iter().map(|&i| usize::from(i)).collect();
		fl.feature_record = list_subset(&fl.feature_record, &indices);
	}
	if variations && let Some(fv) = t.feature_variations() {
		for r in &mut fv.feature_variation_record {
			if let Some(sub) = &mut r.feature_table_substitution {
				sub.substitution_record
					.retain(|r| feature_indices.contains(&r.feature_index));
				for r in &mut sub.substitution_record {
					r.feature_index = feature_indices
						.iter()
						.position(|&f| f == r.feature_index)
						.unwrap_or(0) as u16;
				}
			}
		}
		prune_trailing_variation_records(fv);
		let map: std::collections::BTreeMap<u16, u16> = feature_indices
			.iter()
			.enumerate()
			.map(|(new, &old)| (old, new as u16))
			.collect();
		remap_lookup_variation_records(fv, &map);
	}
}

/// Prunes empty feature variation records, at the end only.
fn prune_trailing_variation_records(fv: &mut ot::FeatureVariations) {
	while fv.feature_variation_record.last().is_some_and(|r| {
		r.feature_table_substitution
			.as_ref()
			.is_none_or(|s| s.substitution_record.is_empty())
	}) {
		fv.feature_variation_record.pop();
	}
	fv.feature_variation_count = Some(fv.feature_variation_record.len() as u32);
}

fn remap_lookup_variation_records(
	fv: &mut ot::FeatureVariations,
	map: &std::collections::BTreeMap<u16, u16>,
) {
	fv.lookup_variation_record
		.retain(|r| map.contains_key(&r.feature_index));
	for r in &mut fv.lookup_variation_record {
		r.feature_index = map[&r.feature_index];
	}
}

/// `subset_lookups` of a `GSUB` or `GPOS` table: keeps the given lookups, then
/// removes the empty features, language systems and scripts.
fn table_subset_lookups<T: Layout>(t: &mut T, lookup_indices: &[u16]) -> Result<()> {
	if let Some(ll) = t.lookup_list() {
		let count = ll.lookup.len();
		let mut lookups = Vec::new();
		for &i in lookup_indices {
			if usize::from(i) < count {
				lookups.push(ll.lookup[usize::from(i)].clone());
			}
		}
		for l in lookups.iter_mut() {
			let l = l
				.as_deref_mut()
				.ok_or_else(|| none_attr("subset_lookups"))?;
			for st in &mut l.sub_table {
				let st = st
					.as_deref_mut()
					.ok_or_else(|| none_attr("subset_lookups"))?;
				subtable_subset_lookups(st, lookup_indices)?;
			}
		}
		ll.lookup = lookups;
	}
	let mut feature_indices = Vec::new();
	if let Some(fl) = t.feature_list() {
		for (i, f) in fl.feature_record.iter_mut().enumerate() {
			let feature = f
				.feature
				.as_deref_mut()
				.ok_or_else(|| none_attr("subset_lookups"))?;
			if feature_subset_lookups(feature, lookup_indices) || f.feature_tag == *b"pref" {
				feature_indices.push(i as u16);
			}
		}
	}
	if let Some(fv) = t.feature_variations() {
		for r in &mut fv.feature_variation_record {
			if let Some(sub) = &mut r.feature_table_substitution {
				for r in &mut sub.substitution_record {
					let f = r
						.feature
						.as_deref_mut()
						.ok_or_else(|| none_attr("subset_lookups"))?;
					if feature_subset_lookups(f, lookup_indices) {
						feature_indices.push(r.feature_index);
					}
				}
			}
		}
		let map: std::collections::BTreeMap<u16, u16> = lookup_indices
			.iter()
			.enumerate()
			.map(|(new, &old)| (old, new as u16))
			.collect();
		for record in &mut fv.lookup_variation_record {
			let flt = record
				.feature_lookups_table
				.as_deref_mut()
				.ok_or_else(|| none_attr("subset_lookups"))?;
			let mut kept = Vec::new();
			for mut r in std::mem::take(&mut flt.lookup_condition_record) {
				let list = r
					.lookup_index_list
					.as_deref_mut()
					.ok_or_else(|| none_attr("subset_lookups"))?;
				list.lookup_index = list
					.lookup_index
					.iter()
					.filter_map(|i| map.get(i).copied())
					.collect();
				if !list.lookup_index.is_empty() {
					kept.push(r);
				}
			}
			flt.lookup_condition_record = kept;
			if !flt.lookup_condition_record.is_empty() {
				feature_indices.push(record.feature_index);
			}
		}
	}
	let feature_indices = uniq_sort(feature_indices);
	subset_features(t, &feature_indices, true);
	script_list_subset_features(t, &feature_indices)
}

/// `prune_lookups()` of `GSUB` or `GPOS`: removes the lookups that no feature
/// refers to.
pub fn prune_table_lookups(font: &mut Font, tag: Tag) -> Result<()> {
	if tag == *b"GSUB" {
		prune_lookups(&mut font.require::<Gsub>()?.table, true)
	} else {
		prune_lookups(&mut font.require::<Gpos>()?.table, true)
	}
}

/// `prune_lookups`: removes (`remap`) or neuters the lookups that no feature
/// refers to.
fn prune_lookups<T: Layout>(t: &mut T, remap: bool) -> Result<()> {
	let feature_indices = collect_features(t);
	let lookup_indices = uniq_sort(collect_lookups(t, &feature_indices));
	let lookup_indices = match t.lookup_list() {
		Some(ll) => closure_lookups(ll, lookup_indices)?,
		None => Vec::new(),
	};
	if remap {
		table_subset_lookups(t, &lookup_indices)
	} else {
		if let Some(ll) = t.lookup_list() {
			for (i, l) in ll.lookup.iter_mut().enumerate() {
				if !lookup_indices.contains(&(i as u16)) {
					*l = None;
				}
			}
		}
		Ok(())
	}
}

/// `subset_feature_tags`.
fn subset_feature_tags<T: Layout>(t: &mut T, tags: &[crate::Item]) -> Result<()> {
	let feature_indices: Vec<u16> = match t.feature_list() {
		Some(fl) => fl
			.feature_record
			.iter()
			.enumerate()
			.filter(|(_, f)| has_str(tags, &f.feature_tag.to_string()))
			.map(|(i, _)| i as u16)
			.collect(),
		None => Vec::new(),
	};
	if t.feature_list().is_some() {
		subset_features(t, &feature_indices, true);
	}
	script_list_subset_features(t, &feature_indices)
}

/// `subset_script_tags`: `script` or `script.language` tags.
fn subset_script_tags<T: Layout>(t: &mut T, tags: &[crate::Item]) -> Result<()> {
	let mut langsys: HashMap<String, BTreeSet<String>> = HashMap::new();
	let mut script_tags = BTreeSet::new();
	for tag in tags {
		let crate::Item::Str(tag) = tag else {
			return Err(err("AttributeError: 'int' object has no attribute 'split'"));
		};
		let (script, lang) = tag.split_once('.').unwrap_or((tag, "*"));
		script_tags.insert(format!("{script:<4}"));
		langsys
			.entry(script.to_owned())
			.or_default()
			.insert(format!("{lang:<4}"));
	}
	let Some(sl) = t.script_list() else {
		return Ok(());
	};
	sl.script_record
		.retain(|s| script_tags.contains(&s.script_tag.to_string()));
	for record in &mut sl.script_record {
		let tag = record.script_tag.to_string();
		let Some(langs) = langsys.get(&tag) else {
			continue;
		};
		if langs.contains("*   ") {
			continue;
		}
		let script = record
			.script
			.as_deref_mut()
			.ok_or_else(|| none_attr("LangSysRecord"))?;
		script
			.lang_sys_record
			.retain(|l| langs.contains(&l.lang_sys_tag.to_string()));
		if !langs.contains("dflt") {
			script.default_lang_sys = None;
		}
	}
	Ok(())
}

/// `remap_duplicate_features`: the features to keep (without duplicates) and
/// the new index of each feature.
fn remap_duplicate_features<T: Layout>(
	t: &mut T,
	feature_indices: &[u16],
) -> Result<(Vec<u16>, std::collections::BTreeMap<u16, u16>)> {
	let lookup_variation_features: BTreeSet<u16> = t
		.feature_variations()
		.as_ref()
		.map(|fv| {
			fv.lookup_variation_record
				.iter()
				.map(|r| r.feature_index)
				.collect()
		})
		.unwrap_or_default();
	let fl = t
		.feature_list()
		.as_ref()
		.ok_or_else(|| none_attr("FeatureRecord"))?;
	let features = &fl.feature_record;
	let feature = |i: u16| {
		features
			.get(usize::from(i))
			.ok_or_else(|| err("IndexError: list index out of range"))
	};
	let mut unique_features: HashMap<Tag, BTreeSet<u16>> = HashMap::new();
	let mut duplicate_features = std::collections::BTreeMap::new();
	for &i in feature_indices {
		let f = feature(i)?;
		let Some(same_tag) = unique_features.get_mut(&f.feature_tag) else {
			unique_features.insert(f.feature_tag, BTreeSet::from([i]));
			duplicate_features.insert(i, i);
			continue;
		};
		let mut found = None;
		for &other in same_tag.iter() {
			if !lookup_variation_features.contains(&i)
				&& !lookup_variation_features.contains(&other)
				&& feature_eq(feature(other)?, f)
			{
				found = Some(other);
				break;
			}
		}
		match found {
			Some(other) => {
				duplicate_features.insert(i, other);
			}
			None => {
				same_tag.insert(i);
				duplicate_features.insert(i, i);
			}
		}
	}
	// Remap the retained feature indices.
	let mut feature_map = std::collections::BTreeMap::new();
	let mut new_idx = 0;
	for &i in feature_indices {
		let unique = duplicate_features.get(&i).copied().unwrap_or(i);
		match feature_map.get(&unique).copied() {
			Some(v) => {
				feature_map.insert(i, v);
			}
			None => {
				feature_map.insert(i, new_idx);
				new_idx += 1;
			}
		}
	}
	let retained = uniq_sort(unique_features.into_values().flatten());
	Ok((retained, feature_map))
}

/// `prune_features`: removes unreferenced and duplicate features.
fn prune_features<T: Layout>(t: &mut T) -> Result<()> {
	let feature_indices = collect_features(t);
	let (feature_indices, map) = remap_duplicate_features(t, &feature_indices)?;
	if t.feature_list().is_some() {
		subset_features(t, &feature_indices, false);
	}
	if let Some(fv) = t.feature_variations() {
		for r in &mut fv.feature_variation_record {
			if let Some(sub) = &mut r.feature_table_substitution {
				sub.substitution_record
					.retain(|r| map.contains_key(&r.feature_index));
				for r in &mut sub.substitution_record {
					r.feature_index = map[&r.feature_index];
				}
			}
		}
		prune_trailing_variation_records(fv);
		remap_lookup_variation_records(fv, &map);
	}
	script_list_prune_features(t, &map)
}

/// `remove_redundant_langsys`: removes language systems equal to the default.
fn remove_redundant_langsys<T: Layout>(t: &mut T) -> Result<()> {
	let features = match t.feature_list() {
		Some(fl) => fl.feature_record.clone(),
		None => return Ok(()),
	};
	let Some(sl) = t.script_list() else {
		return Ok(());
	};
	let feature = |i: u16| {
		features
			.get(usize::from(i))
			.ok_or_else(|| err("IndexError: list index out of range"))
	};
	for s in &mut sl.script_record {
		let Some(script) = s.script.as_deref_mut() else {
			continue;
		};
		let Some(d) = script.default_lang_sys.clone() else {
			continue;
		};
		let mut kept = Vec::new();
		for lr in std::mem::take(&mut script.lang_sys_record) {
			let l = lr
				.lang_sys
				.as_deref()
				.ok_or_else(|| none_attr("FeatureIndex"))?;
			let mut equal = d.feature_index.len() == l.feature_index.len()
				&& (d.req_feature_index == 65535) == (l.req_feature_index == 65535);
			if equal && d.req_feature_index != 65535 {
				equal = feature_eq(feature(d.req_feature_index)?, feature(l.req_feature_index)?);
			}
			if equal {
				for (a, b) in d.feature_index.iter().zip(&l.feature_index) {
					if !feature_eq(feature(*a)?, feature(*b)?) {
						equal = false;
						break;
					}
				}
			}
			if !equal {
				kept.push(lr);
			}
		}
		script.lang_sys_record = kept;
	}
	Ok(())
}

fn with_layout<R>(
	font: &mut Font,
	tag: Tag,
	f: impl FnOnce(&mut dyn LayoutDyn) -> Result<R>,
) -> Result<R> {
	if tag == *b"GSUB" {
		let t = font.require::<Gsub>()?;
		f(&mut t.table)
	} else {
		let t = font.require::<Gpos>()?;
		f(&mut t.table)
	}
}

/// The table-level methods, for either table.
trait LayoutDyn {
	fn prune_pre_subset(&mut self, options: &Options) -> Result<bool>;
	fn subset_glyphs(&mut self, glyphs: &GlyphSet, used_mark_sets: &[u32]) -> Result<bool>;
	fn prune_post_subset(&mut self, options: &Options) -> Result<bool>;
}

impl<T: Layout> LayoutDyn for T {
	/// `prune_pre_subset`: drops the unwanted scripts and features, and neuters
	/// the lookups no feature refers to.
	fn prune_pre_subset(&mut self, options: &Options) -> Result<bool> {
		if !has_str(&options.layout_scripts, "*") {
			subset_script_tags(self, &options.layout_scripts)?;
		}
		if !has_str(&options.layout_features, "*") {
			subset_feature_tags(self, &options.layout_features)?;
		}
		prune_lookups(self, false)?;
		Ok(true)
	}

	/// `subset_glyphs` (with `s.glyphs_gsubed`).
	fn subset_glyphs(&mut self, glyphs: &GlyphSet, used_mark_sets: &[u32]) -> Result<bool> {
		let mut lookup_indices = Vec::new();
		if let Some(ll) = self.lookup_list() {
			for (i, l) in ll.lookup.iter_mut().enumerate() {
				if let Some(l) = l
					&& lookup_subset_glyphs(l, glyphs, used_mark_sets)?
				{
					lookup_indices.push(i as u16);
				}
			}
		}
		table_subset_lookups(self, &lookup_indices)?;
		Ok(true)
	}

	/// `prune_post_subset`.
	fn prune_post_subset(&mut self, options: &Options) -> Result<bool> {
		prune_lookups(self, true)?;
		if let Some(ll) = self.lookup_list() {
			for l in ll.lookup.iter_mut().flatten() {
				for st in l.sub_table.iter_mut().flatten() {
					subtable_prune_post_subset(st, options)?;
				}
			}
		}
		if self.lookup_list().is_none() {
			*self.feature_list() = None;
		}
		if self.feature_list().is_some() {
			remove_redundant_langsys(self)?;
			prune_features(self)?;
		}
		if *self.version() >= 0x0001_0001 {
			// Drop the feature variations if there are no features to substitute.
			let has_features = self.feature_list().is_some();
			if let Some(fv) = self.feature_variations()
				&& !(has_features
					&& (!fv.feature_variation_record.is_empty()
						|| !fv.lookup_variation_record.is_empty()))
			{
				*self.feature_variations() = None;
			}
			// Downgrade the version if there are no feature variations.
			if self.feature_variations().is_none() && *self.version() == 0x0001_0001 {
				*self.version() = 0x0001_0000;
			}
		}
		Ok(true)
	}
}

/// `GSUB.closure_glyphs`: adds the glyphs the lookups of the features may
/// produce, until nothing changes.
pub(crate) fn gsub_closure_glyphs(s: &mut Subsetter, font: &mut Font) -> Result<()> {
	let gsub = &mut font.require::<Gsub>()?.table;
	let feature_indices = collect_features(gsub);
	let lookup_indices = uniq_sort(collect_lookups(gsub, &feature_indices));
	let Some(ll) = gsub.lookup_list.as_deref() else {
		return Ok(());
	};
	let mut closure = Closure {
		glyphs: &mut s.glyphs,
		done: HashMap::new(),
		lookups: &ll.lookup,
	};
	loop {
		let orig = closure.glyphs.clone();
		for &i in &lookup_indices {
			let i = usize::from(i);
			if i >= ll.lookup.len() || ll.lookup[i].is_none() {
				continue;
			}
			closure.lookup_closure(i, None)?;
		}
		if orig == *closure.glyphs {
			break;
		}
	}
	Ok(())
}

pub(crate) fn prune_pre_subset(s: &Subsetter, font: &mut Font, tag: Tag) -> Result<bool> {
	with_layout(font, tag, |t| t.prune_pre_subset(&s.options))
}

pub(crate) fn subset_glyphs(s: &Subsetter, font: &mut Font, tag: Tag) -> Result<bool> {
	with_layout(font, tag, |t| {
		t.subset_glyphs(&s.glyphs_gsubed, &s.used_mark_sets)
	})
}

pub(crate) fn prune_post_subset(s: &Subsetter, font: &mut Font, tag: Tag) -> Result<bool> {
	with_layout(font, tag, |t| t.prune_post_subset(&s.options))
}

/// `GDEF.subset_glyphs` (with `s.glyphs_gsubed`).
pub(crate) fn gdef_subset_glyphs(s: &mut Subsetter, font: &mut Font) -> Result<bool> {
	let glyphs = &s.glyphs_gsubed;
	let t = &mut font.require::<Gdef>()?.table;
	if let Some(l) = &mut t.lig_caret_list {
		let indices = cov_subset(cov_mut(&mut l.coverage)?, glyphs);
		l.lig_glyph = list_subset(&l.lig_glyph, &indices);
	}
	if let Some(cd) = &mut t.mark_attach_class_def {
		cd.class_defs.retain(|g, _| glyphs.contains(g));
	}
	if let Some(cd) = &mut t.glyph_class_def {
		cd.class_defs.retain(|g, _| glyphs.contains(g));
	}
	if let Some(a) = &mut t.attach_list {
		let indices = cov_subset(cov_mut(&mut a.coverage)?, glyphs);
		a.attach_point = list_subset(&a.attach_point, &indices);
	}
	if t.version >= 0x0001_0002
		&& let Some(m) = &mut t.mark_glyph_sets_def
	{
		for c in m.coverage.iter_mut().flatten() {
			cov_subset(c, glyphs);
		}
		let has_glyphs =
			|c: &Option<Box<Coverage>>| c.as_ref().is_some_and(|c| !c.glyphs.is_empty());
		s.used_mark_sets = m
			.coverage
			.iter()
			.enumerate()
			.filter(|(_, c)| has_glyphs(c))
			.map(|(i, _)| i as u32)
			.collect();
		m.coverage.retain(has_glyphs);
	}
	Ok(true)
}

/// `GDEF.prune_post_subset`.
pub(crate) fn gdef_prune_post_subset(font: &mut Font) -> Result<bool> {
	let t = &mut font.require::<Gdef>()?.table;
	if t.lig_caret_list
		.as_ref()
		.is_some_and(|l| l.lig_glyph.is_empty())
	{
		t.lig_caret_list = None;
	}
	if t.mark_attach_class_def
		.as_ref()
		.is_some_and(|c| c.class_defs.is_empty())
	{
		t.mark_attach_class_def = None;
	}
	if t.glyph_class_def
		.as_ref()
		.is_some_and(|c| c.class_defs.is_empty())
	{
		t.glyph_class_def = None;
	}
	if t.attach_list
		.as_ref()
		.is_some_and(|a| a.attach_point.is_empty())
	{
		t.attach_list = None;
	}
	if t.version >= 0x0001_0003 {
		crate::varstore::prune_gdef(font)?;
		let t = &mut font.require::<Gdef>()?.table;
		if t.var_store.as_ref().is_none_or(|s| s.var_data.is_empty()) && t.version == 0x0001_0003 {
			t.version = 0x0001_0002;
		}
	}
	let t = &mut font.require::<Gdef>()?.table;
	if t.mark_glyph_sets_def
		.as_ref()
		.is_none_or(|m| m.coverage.is_empty())
		|| t.version < 0x0001_0002
	{
		t.mark_glyph_sets_def = None;
		if t.version == 0x0001_0002 {
			t.version = 0x0001_0000;
		}
	}
	Ok(t.lig_caret_list.is_some()
		|| t.mark_attach_class_def.is_some()
		|| t.glyph_class_def.is_some()
		|| t.attach_list.is_some()
		|| (t.version >= 0x0001_0002 && t.mark_glyph_sets_def.is_some())
		|| (t.version >= 0x0001_0003 && t.var_store.is_some()))
}
