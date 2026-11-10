//! The lookup builders (`LookupBuilder` and its subclasses), which collect the
//! rules of a lookup and build it.
//!
//! Contextual rules refer to other lookup builders, which can change after the
//! rule is added (feaLib adds substitutions to chained lookups as it reads
//! them), so builders are shared as [`LookupRef`]s, as Python shares objects.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use fontoxide_ttlib::Font;
use fontoxide_ttlib::ot::conv::ValueRecord;
use fontoxide_ttlib::ot::types::{self as ot, Anchor, LookupSubTable};
use indexmap::IndexMap;

pub use super::chain::{
	ChainContextualRuleset, build_format1_subtable, build_format2_subtable, build_format3_subtable,
};
use super::class_def::ClassDefBuilder;
use super::gpos::{
	ValuePair, build_mark_array, build_mark2_record, build_pair_pos_classes_subtable,
};
use super::{
	LayoutTable, MaybeExtended, build_alternate_subst_subtable, build_coverage,
	build_cursive_pos_subtable, build_ligature_subst_subtable, build_lookup,
	build_mark_base_pos_subtable, build_mark_lig_pos_subtable, build_multiple_subst_subtable,
	build_pair_pos_glyphs, build_single_pos, build_single_subst_subtable, chain, sorted_glyphs,
};
use crate::{Error, GlyphMap, Location, Result};

/// A shared lookup builder.
pub type LookupRef = Rc<RefCell<LookupBuilder>>;

/// A key of the mappings of the lookup builders: a glyph (or a sequence of
/// glyphs), or an explicit subtable break, which fontTools stores in the same
/// dict under the key `(SUBTABLE_BREAK_, location)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MapKey<K> {
	/// A rule for a glyph or a sequence of glyphs.
	Key(K),
	/// A subtable break.
	SubtableBreak(Location),
}

impl<K> MapKey<K> {
	/// The key, unless this is a subtable break.
	pub fn key(&self) -> Option<&K> {
		match self {
			MapKey::Key(k) => Some(k),
			MapKey::SubtableBreak(_) => None,
		}
	}
}

/// What all lookup builders have (`LookupBuilder`).
#[derive(Debug, Clone)]
pub struct LookupCommon {
	/// The glyph map (`glyphMap`).
	pub glyph_map: GlyphMap,
	/// Where the lookup came from (`location`).
	pub location: Location,
	/// The table of the lookup (`table`).
	pub table: LayoutTable,
	/// The lookup type (`lookup_type`); 0 for [`AnySubstBuilder`].
	pub lookup_type: u16,
	/// The lookup flags (`lookupflag`).
	pub lookupflag: u16,
	/// The mark filtering set (`markFilterSet`).
	pub mark_filter_set: Option<u16>,
	/// The index of the lookup in the lookup list, assigned when the tables are
	/// built (`lookup_index`).
	pub lookup_index: Option<usize>,
	/// Whether the lookup is built with Extension subtables (`extension`).
	pub extension: bool,
}

impl LookupCommon {
	/// `LookupBuilder.__init__`.
	pub fn new(
		glyph_map: GlyphMap,
		location: Location,
		table: LayoutTable,
		lookup_type: u16,
	) -> Self {
		LookupCommon {
			glyph_map,
			location,
			table,
			lookup_type,
			lookupflag: 0,
			mark_filter_set: None,
			lookup_index: None,
			extension: false,
		}
	}

	fn equals(&self, other: &LookupCommon) -> bool {
		self.table == other.table
			&& self.lookupflag == other.lookupflag
			&& self.mark_filter_set == other.mark_filter_set
			&& self.extension == other.extension
	}

	/// `buildLookup_`.
	fn build_lookup(&self, subtables: Vec<Option<LookupSubTable>>) -> Result<Option<ot::Lookup>> {
		build_lookup(
			subtables,
			self.lookupflag,
			self.mark_filter_set,
			Some(self.table),
			self.extension,
		)
	}

	/// `buildMarkClasses_`: mark class names to IDs, in the order the classes
	/// first appear when the marks are sorted by glyph ID. These are the IDs the
	/// AFDKO makeotf tool assigns.
	fn build_mark_classes(
		&self,
		marks: &IndexMap<String, (String, Anchor)>,
	) -> Result<IndexMap<String, u16>> {
		let mut ids = IndexMap::new();
		for (_, mark) in sorted_glyphs(marks.keys().map(String::as_str), &self.glyph_map)? {
			let class = &marks[mark].0;
			if !ids.contains_key(class) {
				let n = ids.len() as u16;
				ids.insert(class.clone(), n);
			}
		}
		Ok(ids)
	}

	fn coverages(&self, seq: &[Vec<String>]) -> Result<Vec<Option<Box<ot::Coverage>>>> {
		seq.iter()
			.map(|c| {
				build_coverage(c.iter().map(String::as_str), &self.glyph_map)
					.map(|c| Some(Box::new(c)))
			})
			.collect()
	}
}

/// `build_subst_subtables`: the mapping split at subtable breaks.
fn split_at_breaks<K: Clone + Eq + std::hash::Hash, V: Clone>(
	mapping: &IndexMap<MapKey<K>, V>,
) -> Vec<IndexMap<K, V>> {
	let mut parts = vec![IndexMap::new()];
	for (k, v) in mapping {
		match k {
			MapKey::SubtableBreak(_) => parts.push(IndexMap::new()),
			MapKey::Key(k) => {
				if let Some(last) = parts.last_mut() {
					last.insert(k.clone(), v.clone());
				}
			}
		}
	}
	parts
}

/// Builds an Alternate Substitution (GSUB3) lookup (`AlternateSubstBuilder`).
#[derive(Debug, Clone)]
pub struct AlternateSubstBuilder {
	/// The common settings.
	pub common: LookupCommon,
	/// Glyph names to their alternates (`alternates`).
	pub alternates: IndexMap<MapKey<String>, Vec<String>>,
}

/// One position of a contextual rule: the lookups to apply there.
#[derive(Debug, Clone)]
pub enum RuleLookup {
	/// A single lookup, as feaLib uses for the lookups it makes up.
	One(LookupRef),
	/// The lookups named in a rule.
	Many(Vec<LookupRef>),
}

impl PartialEq for RuleLookup {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(RuleLookup::One(a), RuleLookup::One(b)) => Rc::ptr_eq(a, b),
			(RuleLookup::Many(a), RuleLookup::Many(b)) => {
				a.len() == b.len() && a.iter().zip(b).all(|(a, b)| Rc::ptr_eq(a, b))
			}
			_ => false,
		}
	}
}

/// A contextual rule (`ChainContextualRule`): glyph classes before, at and
/// after the input positions, and the lookups to apply at each input position.
#[derive(Debug, Clone, PartialEq)]
pub enum ChainContextualRule {
	/// A rule.
	Rule {
		/// The glyph classes before the input (`prefix`).
		prefix: Vec<Vec<String>>,
		/// The glyph classes of the input (`glyphs`).
		glyphs: Vec<Vec<String>>,
		/// The glyph classes after the input (`suffix`).
		suffix: Vec<Vec<String>>,
		/// The lookups at each input position (`lookups`).
		lookups: Vec<Option<RuleLookup>>,
	},
	/// An explicit subtable break (`is_subtable_break`).
	SubtableBreak,
}

impl ChainContextualRule {
	/// A rule.
	pub fn new(
		prefix: Vec<Vec<String>>,
		glyphs: Vec<Vec<String>>,
		suffix: Vec<Vec<String>>,
		lookups: Vec<Option<RuleLookup>>,
	) -> Self {
		ChainContextualRule::Rule {
			prefix,
			glyphs,
			suffix,
			lookups,
		}
	}

	/// Whether this is a subtable break (`is_subtable_break`).
	pub fn is_subtable_break(&self) -> bool {
		matches!(self, ChainContextualRule::SubtableBreak)
	}

	/// The prefix; empty for a subtable break.
	pub fn prefix(&self) -> &[Vec<String>] {
		match self {
			ChainContextualRule::Rule { prefix, .. } => prefix,
			ChainContextualRule::SubtableBreak => &[],
		}
	}

	/// The input glyph classes; empty for a subtable break.
	pub fn glyphs(&self) -> &[Vec<String>] {
		match self {
			ChainContextualRule::Rule { glyphs, .. } => glyphs,
			ChainContextualRule::SubtableBreak => &[],
		}
	}

	/// The suffix; empty for a subtable break.
	pub fn suffix(&self) -> &[Vec<String>] {
		match self {
			ChainContextualRule::Rule { suffix, .. } => suffix,
			ChainContextualRule::SubtableBreak => &[],
		}
	}

	/// The lookups at each input position; empty for a subtable break.
	pub fn lookups(&self) -> &[Option<RuleLookup>] {
		match self {
			ChainContextualRule::Rule { lookups, .. } => lookups,
			ChainContextualRule::SubtableBreak => &[],
		}
	}
}

/// An item of the lookups that [`ChainContextualBuilder::find_chainable_single_pos`]
/// looks at: the lookups of the rules, where subtable breaks appear as
/// [`ChainTarget::SubtableBreak`].
#[derive(Debug, Clone)]
pub enum ChainTarget {
	/// The lookups of a rule position.
	Lookup(Option<RuleLookup>),
	/// The subtable break marker of a subtable break rule.
	SubtableBreak,
}

/// Builds a Chained Contextual Positioning (GPOS8) or Substitution (GSUB6)
/// lookup (`ChainContextPosBuilder`, `ChainContextSubstBuilder`), or the
/// non-chaining type when no rule has context.
#[derive(Debug, Clone)]
pub struct ChainContextualBuilder {
	/// The common settings.
	pub common: LookupCommon,
	/// The rules (`rules`).
	pub rules: Vec<ChainContextualRule>,
}

impl ChainContextualBuilder {
	/// The rules split at subtable breaks, without empty rulesets (`rulesets`).
	pub fn rulesets(&self) -> Vec<ChainContextualRuleset> {
		let mut rulesets = vec![ChainContextualRuleset::new()];
		for rule in &self.rules {
			if rule.is_subtable_break() {
				rulesets.push(ChainContextualRuleset::new());
				continue;
			}
			if let Some(last) = rulesets.last_mut() {
				last.add_rule(rule.clone());
			}
		}
		rulesets.retain(|r| !r.rules.is_empty());
		rulesets
	}

	/// `ChainContextSubstBuilder.getAlternateGlyphs`: the alternates of the
	/// lookups the rules use, for building `aalt`.
	pub fn get_alternate_glyphs(&self) -> Result<IndexMap<String, Vec<String>>> {
		let mut result: IndexMap<String, Vec<String>> = IndexMap::new();
		for rule in &self.rules {
			for lookups in rule.lookups().iter().flatten() {
				let list: Vec<&LookupRef> = match lookups {
					RuleLookup::One(l) => vec![l],
					RuleLookup::Many(ls) => ls.iter().collect(),
				};
				for lookup in list {
					let alts = lookup
						.try_borrow()
						.map_err(|_| Error::Value("a lookup refers to itself".into()))?
						.get_alternate_glyphs()?;
					for (glyph, replacements) in alts {
						let for_glyph = result.entry(glyph).or_default();
						for g in replacements {
							if !for_glyph.contains(&g) {
								for_glyph.push(g);
							}
						}
					}
				}
			}
		}
		Ok(result)
	}

	/// The last single positioning lookup among `lookups`, since the last
	/// subtable break, that can position all `glyphs` with `value`
	/// (`find_chainable_single_pos`).
	pub fn find_chainable_single_pos(
		&self,
		lookups: &[ChainTarget],
		glyphs: &[String],
		value: &ValueRecord,
	) -> Option<LookupRef> {
		let mut res = None;
		for lookup in lookups.iter().rev() {
			let l = match lookup {
				ChainTarget::SubtableBreak => return res,
				ChainTarget::Lookup(Some(RuleLookup::One(l))) => l,
				ChainTarget::Lookup(_) => continue,
			};
			let ok = match l.try_borrow().as_deref() {
				Ok(LookupBuilder::SinglePos(b)) => glyphs.iter().all(|g| b.can_add(g, value)),
				_ => false,
			};
			if ok {
				res = Some(l.clone());
			}
		}
		res
	}

	/// The lookups of the rules, for [`ChainContextualBuilder::find_chainable_single_pos`].
	pub fn targets(&self) -> Vec<ChainTarget> {
		let mut out = Vec::new();
		for rule in &self.rules {
			match rule {
				ChainContextualRule::SubtableBreak => out.push(ChainTarget::SubtableBreak),
				ChainContextualRule::Rule { lookups, .. } => {
					out.extend(lookups.iter().cloned().map(ChainTarget::Lookup));
				}
			}
		}
		out
	}

	/// The last lookup of a rule since the last subtable break that `test`
	/// accepts.
	fn find_chainable(&self, test: impl Fn(&LookupBuilder) -> bool) -> Option<LookupRef> {
		let mut res = None;
		for rule in self.rules.iter().rev() {
			if rule.is_subtable_break() {
				return res;
			}
			for sub in rule.lookups() {
				if let Some(RuleLookup::One(l)) = sub
					&& l.try_borrow().is_ok_and(|b| test(&b))
				{
					res = Some(l.clone());
				}
			}
		}
		res
	}

	/// The last single substitution lookup since the last subtable break whose
	/// mapping does not conflict with `mapping` (`find_chainable_subst` with
	/// `SingleSubstBuilder`).
	pub fn find_chainable_single_subst(
		&self,
		mapping: &IndexMap<String, String>,
	) -> Option<LookupRef> {
		self.find_chainable(|b| match b {
			LookupBuilder::SingleSubst(sub) => !sub.mapping.iter().any(|(g, v)| {
				g.key()
					.is_some_and(|g| mapping.get(g).is_some_and(|m| m != v))
			}),
			_ => false,
		})
	}

	/// The last multiple substitution lookup since the last subtable break whose
	/// mapping does not conflict with `mapping` (`find_chainable_subst` with
	/// `MultipleSubstBuilder`).
	pub fn find_chainable_multiple_subst(
		&self,
		mapping: &IndexMap<String, Vec<String>>,
	) -> Option<LookupRef> {
		self.find_chainable(|b| match b {
			LookupBuilder::MultipleSubst(sub) => !sub.mapping.iter().any(|(g, v)| {
				g.key()
					.is_some_and(|g| mapping.get(g).is_some_and(|m| m != v))
			}),
			_ => false,
		})
	}

	/// The last alternate substitution lookup since the last subtable break
	/// that has no alternates for `glyph` (`find_chainable_alternate_subst`).
	pub fn find_chainable_alternate_subst(&self, glyph: &str) -> Option<LookupRef> {
		let key = MapKey::Key(glyph.to_owned());
		self.find_chainable(|b| match b {
			LookupBuilder::AlternateSubst(sub) => !sub.alternates.contains_key(&key),
			_ => false,
		})
	}

	/// The last ligature substitution lookup since the last subtable break in
	/// which every sequence of `glyphs` is unmapped or mapped to `replacement`
	/// (`find_chainable_ligature_subst`).
	pub fn find_chainable_ligature_subst(
		&self,
		glyphs: &[Vec<String>],
		replacement: &str,
	) -> Option<LookupRef> {
		let sequences = product(glyphs);
		self.find_chainable(|b| match b {
			LookupBuilder::LigatureSubst(sub) => sequences.iter().all(|seq| {
				sub.ligatures
					.get(&MapKey::Key(seq.clone()))
					.is_none_or(|v| v == replacement)
			}),
			_ => false,
		})
	}
}

/// `itertools.product(*glyphs)`.
pub fn product(glyphs: &[Vec<String>]) -> Vec<Vec<String>> {
	let mut out: Vec<Vec<String>> = vec![Vec::new()];
	for class in glyphs {
		let mut next = Vec::with_capacity(out.len() * class.len());
		for prefix in &out {
			for g in class {
				let mut seq = prefix.clone();
				seq.push(g.clone());
				next.push(seq);
			}
		}
		out = next;
	}
	out
}

/// Builds a Ligature Substitution (GSUB4) lookup (`LigatureSubstBuilder`).
#[derive(Debug, Clone)]
pub struct LigatureSubstBuilder {
	/// The common settings.
	pub common: LookupCommon,
	/// Component glyph sequences to ligature glyphs (`ligatures`).
	pub ligatures: IndexMap<MapKey<Vec<String>>, String>,
}

/// Builds a Multiple Substitution (GSUB2) lookup (`MultipleSubstBuilder`).
#[derive(Debug, Clone)]
pub struct MultipleSubstBuilder {
	/// The common settings.
	pub common: LookupCommon,
	/// Glyphs to the sequences that replace them (`mapping`).
	pub mapping: IndexMap<MapKey<String>, Vec<String>>,
}

/// Builds a Cursive Positioning (GPOS3) lookup (`CursivePosBuilder`).
#[derive(Debug, Clone)]
pub struct CursivePosBuilder {
	/// The common settings.
	pub common: LookupCommon,
	/// Glyphs to their entry and exit anchors (`attachments`).
	pub attachments: IndexMap<MapKey<String>, (Option<Anchor>, Option<Anchor>)>,
}

impl CursivePosBuilder {
	/// Sets the entry and exit anchors of glyphs (`add_attachment`).
	pub fn add_attachment(
		&mut self,
		_location: &Location,
		glyphs: &[String],
		entry_anchor: Option<Anchor>,
		exit_anchor: Option<Anchor>,
	) {
		for glyph in glyphs {
			self.attachments.insert(
				MapKey::Key(glyph.clone()),
				(entry_anchor.clone(), exit_anchor.clone()),
			);
		}
	}
}

/// Mark glyphs to their mark class name and anchor.
pub type MarkMap = IndexMap<String, (String, Anchor)>;

/// Glyphs to the anchors of each mark class name.
pub type AnchorMap = IndexMap<String, IndexMap<String, Anchor>>;

/// Ligature glyphs to the anchors of each mark class name, for each component.
pub type LigatureAnchorMap = IndexMap<String, Vec<IndexMap<String, Anchor>>>;

/// Builds a Mark-To-Base Positioning (GPOS4) lookup (`MarkBasePosBuilder`).
#[derive(Debug, Clone)]
pub struct MarkBasePosBuilder {
	/// The common settings.
	pub common: LookupCommon,
	/// Mark glyphs to their class and anchor (`marks`).
	pub marks: MarkMap,
	/// Base glyphs to the anchor of each mark class (`bases`).
	pub bases: AnchorMap,
	/// The marks and bases before each subtable break (`subtables_`).
	pub subtables: Vec<(MarkMap, AnchorMap)>,
}

impl MarkBasePosBuilder {
	/// `get_subtables_`. Like fontTools, this appends the current marks and
	/// bases to the subtables each time it is called.
	pub fn get_subtables(&mut self) -> &[(MarkMap, AnchorMap)] {
		if !self.bases.is_empty() || !self.marks.is_empty() {
			self.subtables
				.push((self.marks.clone(), self.bases.clone()));
		}
		&self.subtables
	}
}

/// Builds a Mark-To-Ligature Positioning (GPOS5) lookup (`MarkLigPosBuilder`).
#[derive(Debug, Clone)]
pub struct MarkLigPosBuilder {
	/// The common settings.
	pub common: LookupCommon,
	/// Mark glyphs to their class and anchor (`marks`).
	pub marks: MarkMap,
	/// Ligature glyphs to the anchors of each mark class on each component
	/// (`ligatures`).
	pub ligatures: LigatureAnchorMap,
	/// The marks and ligatures before each subtable break (`subtables_`).
	pub subtables: Vec<(MarkMap, LigatureAnchorMap)>,
}

impl MarkLigPosBuilder {
	/// `get_subtables_`, which appends the current marks and ligatures each time.
	pub fn get_subtables(&mut self) -> &[(MarkMap, LigatureAnchorMap)] {
		if !self.ligatures.is_empty() || !self.marks.is_empty() {
			self.subtables
				.push((self.marks.clone(), self.ligatures.clone()));
		}
		&self.subtables
	}
}

/// Builds a Mark-To-Mark Positioning (GPOS6) lookup (`MarkMarkPosBuilder`).
#[derive(Debug, Clone)]
pub struct MarkMarkPosBuilder {
	/// The common settings.
	pub common: LookupCommon,
	/// Mark glyphs to their class and anchor (`marks`).
	pub marks: MarkMap,
	/// Base mark glyphs to the anchor of each mark class (`baseMarks`).
	pub base_marks: AnchorMap,
	/// The marks and base marks before each subtable break (`subtables_`).
	pub subtables: Vec<(MarkMap, AnchorMap)>,
}

impl MarkMarkPosBuilder {
	/// `get_subtables_`, which appends the current marks and base marks each
	/// time.
	pub fn get_subtables(&mut self) -> &[(MarkMap, AnchorMap)] {
		if !self.base_marks.is_empty() || !self.marks.is_empty() {
			self.subtables
				.push((self.marks.clone(), self.base_marks.clone()));
		}
		&self.subtables
	}
}

/// A rule of a reverse chaining substitution: the prefix, the suffix and the
/// substitutions.
pub type ReverseChainRule = (Vec<Vec<String>>, Vec<Vec<String>>, IndexMap<String, String>);

/// Builds a Reverse Chaining Contextual Single Substitution (GSUB8) lookup
/// (`ReverseChainSingleSubstBuilder`).
#[derive(Debug, Clone)]
pub struct ReverseChainSingleSubstBuilder {
	/// The common settings.
	pub common: LookupCommon,
	/// The rules (`rules`); each becomes a subtable.
	pub rules: Vec<ReverseChainRule>,
}

/// A temporary builder for Single, Multiple or Ligature Substitution lookups
/// (`AnySubstBuilder`), turned into the right kind by
/// [`AnySubstBuilder::promote_lookup_type`].
#[derive(Debug, Clone)]
pub struct AnySubstBuilder {
	/// The common settings.
	pub common: LookupCommon,
	/// Input glyph sequences to output glyph sequences (`mapping`).
	pub mapping: IndexMap<MapKey<Vec<String>>, Vec<String>>,
}

/// Which builder an [`AnySubstBuilder`] rule becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubstKind {
	Single,
	Multiple,
	Ligature,
}

impl AnySubstBuilder {
	/// Whether rules can be added without mixing multiple and ligature
	/// substitutions, which cannot be one lookup (`can_add_mapping`).
	pub fn can_add_mapping(&self, mapping: Option<&IndexMap<Vec<String>, Vec<String>>>) -> bool {
		let Some(mapping) = mapping else {
			return true;
		};
		let is_multi = mapping.values().any(|v| v.len() > 1);
		let is_liga = mapping.keys().any(|k| k.len() > 1);
		let mut has_multi = false;
		let mut has_liga = false;
		for (k, v) in &self.mapping {
			let MapKey::Key(k) = k else {
				continue;
			};
			if k.len() > 1 {
				has_liga = true;
			}
			if v.len() > 1 {
				has_multi = true;
			}
		}
		!(has_multi && is_liga) && !(has_liga && is_multi)
	}

	/// Turns the builder into Single, Multiple or Ligature Substitution
	/// builders (`promote_lookup_type`): single substitutions become multiple or
	/// ligature substitutions if the others are. A named lookup cannot be split,
	/// so mixed rules give `None` for it.
	pub fn promote_lookup_type(&self, is_named_lookup: bool) -> Result<Option<Vec<LookupBuilder>>> {
		let mut kinds = Vec::with_capacity(self.mapping.len());
		for (key, value) in &self.mapping {
			kinds.push(match key {
				MapKey::SubtableBreak(_) => None,
				MapKey::Key(k) if k.len() == 1 && value.len() == 1 => Some(SubstKind::Single),
				MapKey::Key(k) if k.len() == 1 => Some(SubstKind::Multiple),
				MapKey::Key(k) if k.len() > 1 && value.len() == 1 => Some(SubstKind::Ligature),
				MapKey::Key(_) => return Err(Error::Assertion("Should not happen".into())),
			});
		}
		let has_multiple = kinds.contains(&Some(SubstKind::Multiple));
		let has_ligature = kinds.contains(&Some(SubstKind::Ligature));
		let target = if !has_ligature && !has_multiple {
			Some(SubstKind::Single)
		} else if has_multiple && !has_ligature {
			Some(SubstKind::Multiple)
		} else if has_ligature && !has_multiple {
			Some(SubstKind::Ligature)
		} else {
			None
		};
		let mut ret: Vec<LookupBuilder> = Vec::new();
		match target {
			Some(kind) => {
				let mut builder = self.new_builder(kind);
				for (key, value) in &self.mapping {
					add_to(&mut builder, key, value)?;
				}
				ret.push(builder);
			}
			None if is_named_lookup => return Ok(None),
			None => {
				for (kind, (key, value)) in kinds.iter().zip(&self.mapping) {
					let Some(kind) = kind else {
						// fontTools calls the builder class of the break, None.
						return Err(Error::Value(
							"TypeError: 'NoneType' object is not callable".into(),
						));
					};
					let same = ret.last().is_some_and(|b| {
						matches!(
							(kind, b),
							(SubstKind::Single, LookupBuilder::SingleSubst(_))
								| (SubstKind::Multiple, LookupBuilder::MultipleSubst(_))
								| (SubstKind::Ligature, LookupBuilder::LigatureSubst(_))
						)
					});
					if !same {
						ret.push(self.new_builder(*kind));
					}
					if let Some(b) = ret.last_mut() {
						add_to(b, key, value)?;
					}
				}
			}
		}
		for builder in &mut ret {
			let c = builder.common_mut();
			c.extension = self.common.extension;
			c.lookupflag = self.common.lookupflag;
			c.mark_filter_set = self.common.mark_filter_set;
		}
		Ok(Some(ret))
	}

	fn new_builder(&self, kind: SubstKind) -> LookupBuilder {
		let gm = self.common.glyph_map.clone();
		let loc = self.common.location.clone();
		match kind {
			SubstKind::Single => LookupBuilder::single_subst(gm, loc),
			SubstKind::Multiple => LookupBuilder::multiple_subst(gm, loc),
			SubstKind::Ligature => LookupBuilder::ligature_subst(gm, loc),
		}
	}
}

/// `_add_to_single_subst`, `_add_to_multiple_subst`, `_add_to_ligature_subst`.
fn add_to(builder: &mut LookupBuilder, key: &MapKey<Vec<String>>, value: &[String]) -> Result<()> {
	let first = |v: &[String]| {
		v.first()
			.cloned()
			.ok_or_else(|| Error::Value("IndexError: tuple index out of range".into()))
	};
	match builder {
		LookupBuilder::SingleSubst(b) => match key {
			MapKey::SubtableBreak(l) => {
				b.mapping
					.insert(MapKey::SubtableBreak(l.clone()), String::new());
			}
			MapKey::Key(k) => {
				b.mapping.insert(MapKey::Key(first(k)?), first(value)?);
			}
		},
		LookupBuilder::MultipleSubst(b) => match key {
			MapKey::SubtableBreak(l) => {
				b.mapping
					.insert(MapKey::SubtableBreak(l.clone()), Vec::new());
			}
			MapKey::Key(k) => {
				b.mapping.insert(MapKey::Key(first(k)?), value.to_vec());
			}
		},
		LookupBuilder::LigatureSubst(b) => match key {
			MapKey::SubtableBreak(l) => {
				b.ligatures
					.insert(MapKey::SubtableBreak(l.clone()), String::new());
			}
			MapKey::Key(k) => {
				b.ligatures.insert(MapKey::Key(k.clone()), first(value)?);
			}
		},
		_ => return Err(Error::Assertion("Should not happen".into())),
	}
	Ok(())
}

/// Builds a Single Substitution (GSUB1) lookup (`SingleSubstBuilder`).
#[derive(Debug, Clone)]
pub struct SingleSubstBuilder {
	/// The common settings.
	pub common: LookupCommon,
	/// Glyphs to their replacements (`mapping`).
	pub mapping: IndexMap<MapKey<String>, String>,
}

/// A rule of [`PairPosBuilder::pairs`]: a class pair or a subtable break.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // Breaks are rare.
pub enum PairRule {
	/// Two glyph classes (sorted glyph names) and their value records.
	ClassPair(
		Vec<String>,
		Option<ValueRecord>,
		Vec<String>,
		Option<ValueRecord>,
	),
	/// A subtable break.
	SubtableBreak,
}

/// Builds class-based Pair Positioning (GPOS2 format 2) subtables
/// (`ClassPairPosSubtableBuilder`), starting a new subtable whenever a pair's
/// classes overlap the classes so far.
#[derive(Debug, Clone)]
pub struct ClassPairPosSubtableBuilder {
	glyph_map: GlyphMap,
	class_def1: Option<ClassDefBuilder>,
	class_def2: Option<ClassDefBuilder>,
	values: IndexMap<(Vec<String>, Vec<String>), ValuePair>,
	force_subtable_break: bool,
	subtables: Vec<ot::PairPos>,
}

impl ClassPairPosSubtableBuilder {
	/// A builder for a lookup with this glyph map.
	pub fn new(glyph_map: GlyphMap) -> Self {
		ClassPairPosSubtableBuilder {
			glyph_map,
			class_def1: None,
			class_def2: None,
			values: IndexMap::new(),
			force_subtable_break: false,
			subtables: Vec::new(),
		}
	}

	/// Adds a pair positioning rule for two glyph classes (`addPair`).
	pub fn add_pair(
		&mut self,
		gc1: &[String],
		value1: Option<ValueRecord>,
		gc2: &[String],
		value2: Option<ValueRecord>,
	) -> Result<()> {
		let mergeable = !self.force_subtable_break
			&& self.class_def1.as_ref().is_some_and(|c| c.can_add(gc1))
			&& self.class_def2.as_ref().is_some_and(|c| c.can_add(gc2));
		if !mergeable {
			self.flush()?;
			self.class_def1 = Some(ClassDefBuilder::new(true));
			self.class_def2 = Some(ClassDefBuilder::new(false));
			self.values = IndexMap::new();
		}
		if let Some(c) = &mut self.class_def1 {
			c.add(gc1)?;
		}
		if let Some(c) = &mut self.class_def2 {
			c.add(gc2)?;
		}
		self.values
			.insert((gc1.to_vec(), gc2.to_vec()), (value1, value2));
		Ok(())
	}

	/// Adds an explicit subtable break (`addSubtableBreak`).
	pub fn add_subtable_break(&mut self) {
		self.force_subtable_break = true;
	}

	/// The subtables built (`subtables`).
	pub fn subtables(&mut self) -> Result<Vec<ot::PairPos>> {
		self.flush()?;
		Ok(self.subtables.clone())
	}

	fn flush(&mut self) -> Result<()> {
		if self.class_def1.is_none() || self.class_def2.is_none() {
			return Ok(());
		}
		let st = build_pair_pos_classes_subtable(&self.values, &self.glyph_map, None, None)?;
		self.subtables.push(st);
		self.force_subtable_break = false;
		Ok(())
	}
}

/// Builds a Pair Positioning (GPOS2) lookup (`PairPosBuilder`).
#[derive(Debug, Clone)]
pub struct PairPosBuilder {
	/// The common settings.
	pub common: LookupCommon,
	/// The class pairs and subtable breaks (`pairs`).
	pub pairs: Vec<PairRule>,
	/// Glyph pairs to their value records (`glyphPairs`).
	pub glyph_pairs: IndexMap<(String, String), ValuePair>,
	/// Where each glyph pair came from (`locations`).
	pub locations: HashMap<(String, String), Location>,
}

impl PairPosBuilder {
	/// Adds a class pair rule (`addClassPair`). The classes are sorted lists of
	/// glyph names.
	pub fn add_class_pair(
		&mut self,
		_location: &Location,
		glyphclass1: Vec<String>,
		value1: Option<ValueRecord>,
		glyphclass2: Vec<String>,
		value2: Option<ValueRecord>,
	) {
		self.pairs.push(PairRule::ClassPair(
			glyphclass1,
			value1,
			glyphclass2,
			value2,
		));
	}

	/// Adds a glyph pair rule (`addGlyphPair`). The first value of a pair wins,
	/// as the feature file specification wants for pairs from `enum` rules.
	pub fn add_glyph_pair(
		&mut self,
		location: &Location,
		glyph1: &str,
		value1: Option<ValueRecord>,
		glyph2: &str,
		value2: Option<ValueRecord>,
	) {
		let key = (glyph1.to_owned(), glyph2.to_owned());
		if self.glyph_pairs.contains_key(&key) {
			let other = self.locations.get(&key).cloned().flatten();
			log::debug!(
				"Already defined position for pair {glyph1} {glyph2} at {}; choosing the first value",
				other.as_deref().unwrap_or("None")
			);
		} else {
			self.glyph_pairs.insert(key.clone(), (value1, value2));
			self.locations.insert(key, location.clone());
		}
	}
}

/// Builds a Single Positioning (GPOS1) lookup (`SinglePosBuilder`).
#[derive(Debug, Clone)]
pub struct SinglePosBuilder {
	/// The common settings.
	pub common: LookupCommon,
	/// Where each glyph's rule came from (`locations`).
	pub locations: HashMap<String, Location>,
	/// Glyphs to their value records (`mapping`).
	pub mapping: IndexMap<String, ValueRecord>,
}

impl SinglePosBuilder {
	/// Adds a single positioning rule (`add_pos`); no value record means an
	/// empty one. Fails if the glyph already has a different value.
	pub fn add_pos(
		&mut self,
		location: &Location,
		glyph: &str,
		value: Option<ValueRecord>,
	) -> Result<()> {
		let value = value.unwrap_or_default();
		if !self.can_add(glyph, &value) {
			let other = self.locations.get(glyph).cloned().flatten();
			return Err(Error::otl(
				format!(
					"Already defined different position for glyph \"{glyph}\" at {}",
					other.as_deref().unwrap_or("None")
				),
				location,
			));
		}
		self.mapping.insert(glyph.to_owned(), value);
		self.locations.insert(glyph.to_owned(), location.clone());
		Ok(())
	}

	/// Whether the glyph has no value yet, or the same value (`can_add`).
	pub fn can_add(&self, glyph: &str, value: &ValueRecord) -> bool {
		self.mapping.get(glyph).is_none_or(|v| v == value)
	}
}

/// The kind of a lookup builder (its Python class).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LookupKind {
	/// `AlternateSubstBuilder`.
	AlternateSubst,
	/// `ChainContextPosBuilder`.
	ChainContextPos,
	/// `ChainContextSubstBuilder`.
	ChainContextSubst,
	/// `LigatureSubstBuilder`.
	LigatureSubst,
	/// `MultipleSubstBuilder`.
	MultipleSubst,
	/// `CursivePosBuilder`.
	CursivePos,
	/// `MarkBasePosBuilder`.
	MarkBasePos,
	/// `MarkLigPosBuilder`.
	MarkLigPos,
	/// `MarkMarkPosBuilder`.
	MarkMarkPos,
	/// `ReverseChainSingleSubstBuilder`.
	ReverseChainSingleSubst,
	/// `AnySubstBuilder`.
	AnySubst,
	/// `SingleSubstBuilder`.
	SingleSubst,
	/// `PairPosBuilder`.
	PairPos,
	/// `SinglePosBuilder`.
	SinglePos,
}

/// A lookup builder of any kind (`LookupBuilder` and its subclasses).
#[derive(Debug, Clone)]
pub enum LookupBuilder {
	/// `AlternateSubstBuilder`.
	AlternateSubst(AlternateSubstBuilder),
	/// `ChainContextPosBuilder`.
	ChainContextPos(ChainContextualBuilder),
	/// `ChainContextSubstBuilder`.
	ChainContextSubst(ChainContextualBuilder),
	/// `LigatureSubstBuilder`.
	LigatureSubst(LigatureSubstBuilder),
	/// `MultipleSubstBuilder`.
	MultipleSubst(MultipleSubstBuilder),
	/// `CursivePosBuilder`.
	CursivePos(CursivePosBuilder),
	/// `MarkBasePosBuilder`.
	MarkBasePos(MarkBasePosBuilder),
	/// `MarkLigPosBuilder`.
	MarkLigPos(MarkLigPosBuilder),
	/// `MarkMarkPosBuilder`.
	MarkMarkPos(MarkMarkPosBuilder),
	/// `ReverseChainSingleSubstBuilder`.
	ReverseChainSingleSubst(ReverseChainSingleSubstBuilder),
	/// `AnySubstBuilder`.
	AnySubst(AnySubstBuilder),
	/// `SingleSubstBuilder`.
	SingleSubst(SingleSubstBuilder),
	/// `PairPosBuilder`.
	PairPos(PairPosBuilder),
	/// `SinglePosBuilder`.
	SinglePos(SinglePosBuilder),
}

macro_rules! each {
	($self:expr, $b:ident => $e:expr) => {
		match $self {
			LookupBuilder::AlternateSubst($b) => $e,
			LookupBuilder::ChainContextPos($b) => $e,
			LookupBuilder::ChainContextSubst($b) => $e,
			LookupBuilder::LigatureSubst($b) => $e,
			LookupBuilder::MultipleSubst($b) => $e,
			LookupBuilder::CursivePos($b) => $e,
			LookupBuilder::MarkBasePos($b) => $e,
			LookupBuilder::MarkLigPos($b) => $e,
			LookupBuilder::MarkMarkPos($b) => $e,
			LookupBuilder::ReverseChainSingleSubst($b) => $e,
			LookupBuilder::AnySubst($b) => $e,
			LookupBuilder::SingleSubst($b) => $e,
			LookupBuilder::PairPos($b) => $e,
			LookupBuilder::SinglePos($b) => $e,
		}
	};
}

impl LookupBuilder {
	/// `AlternateSubstBuilder(font, location)`.
	pub fn alternate_subst(glyph_map: GlyphMap, location: Location) -> Self {
		LookupBuilder::AlternateSubst(AlternateSubstBuilder {
			common: LookupCommon::new(glyph_map, location, LayoutTable::Gsub, 3),
			alternates: IndexMap::new(),
		})
	}

	/// `ChainContextPosBuilder(font, location)`.
	pub fn chain_context_pos(glyph_map: GlyphMap, location: Location) -> Self {
		LookupBuilder::ChainContextPos(ChainContextualBuilder {
			common: LookupCommon::new(glyph_map, location, LayoutTable::Gpos, 8),
			rules: Vec::new(),
		})
	}

	/// `ChainContextSubstBuilder(font, location)`.
	pub fn chain_context_subst(glyph_map: GlyphMap, location: Location) -> Self {
		LookupBuilder::ChainContextSubst(ChainContextualBuilder {
			common: LookupCommon::new(glyph_map, location, LayoutTable::Gsub, 6),
			rules: Vec::new(),
		})
	}

	/// `LigatureSubstBuilder(font, location)`.
	pub fn ligature_subst(glyph_map: GlyphMap, location: Location) -> Self {
		LookupBuilder::LigatureSubst(LigatureSubstBuilder {
			common: LookupCommon::new(glyph_map, location, LayoutTable::Gsub, 4),
			ligatures: IndexMap::new(),
		})
	}

	/// `MultipleSubstBuilder(font, location)`.
	pub fn multiple_subst(glyph_map: GlyphMap, location: Location) -> Self {
		LookupBuilder::MultipleSubst(MultipleSubstBuilder {
			common: LookupCommon::new(glyph_map, location, LayoutTable::Gsub, 2),
			mapping: IndexMap::new(),
		})
	}

	/// `CursivePosBuilder(font, location)`.
	pub fn cursive_pos(glyph_map: GlyphMap, location: Location) -> Self {
		LookupBuilder::CursivePos(CursivePosBuilder {
			common: LookupCommon::new(glyph_map, location, LayoutTable::Gpos, 3),
			attachments: IndexMap::new(),
		})
	}

	/// `MarkBasePosBuilder(font, location)`.
	pub fn mark_base_pos(glyph_map: GlyphMap, location: Location) -> Self {
		LookupBuilder::MarkBasePos(MarkBasePosBuilder {
			common: LookupCommon::new(glyph_map, location, LayoutTable::Gpos, 4),
			marks: IndexMap::new(),
			bases: IndexMap::new(),
			subtables: Vec::new(),
		})
	}

	/// `MarkLigPosBuilder(font, location)`.
	pub fn mark_lig_pos(glyph_map: GlyphMap, location: Location) -> Self {
		LookupBuilder::MarkLigPos(MarkLigPosBuilder {
			common: LookupCommon::new(glyph_map, location, LayoutTable::Gpos, 5),
			marks: IndexMap::new(),
			ligatures: IndexMap::new(),
			subtables: Vec::new(),
		})
	}

	/// `MarkMarkPosBuilder(font, location)`.
	pub fn mark_mark_pos(glyph_map: GlyphMap, location: Location) -> Self {
		LookupBuilder::MarkMarkPos(MarkMarkPosBuilder {
			common: LookupCommon::new(glyph_map, location, LayoutTable::Gpos, 6),
			marks: IndexMap::new(),
			base_marks: IndexMap::new(),
			subtables: Vec::new(),
		})
	}

	/// `ReverseChainSingleSubstBuilder(font, location)`.
	pub fn reverse_chain_single_subst(glyph_map: GlyphMap, location: Location) -> Self {
		LookupBuilder::ReverseChainSingleSubst(ReverseChainSingleSubstBuilder {
			common: LookupCommon::new(glyph_map, location, LayoutTable::Gsub, 8),
			rules: Vec::new(),
		})
	}

	/// `AnySubstBuilder(font, location)`.
	pub fn any_subst(glyph_map: GlyphMap, location: Location) -> Self {
		LookupBuilder::AnySubst(AnySubstBuilder {
			common: LookupCommon::new(glyph_map, location, LayoutTable::Gsub, 0),
			mapping: IndexMap::new(),
		})
	}

	/// `SingleSubstBuilder(font, location)`.
	pub fn single_subst(glyph_map: GlyphMap, location: Location) -> Self {
		LookupBuilder::SingleSubst(SingleSubstBuilder {
			common: LookupCommon::new(glyph_map, location, LayoutTable::Gsub, 1),
			mapping: IndexMap::new(),
		})
	}

	/// `PairPosBuilder(font, location)`.
	pub fn pair_pos(glyph_map: GlyphMap, location: Location) -> Self {
		LookupBuilder::PairPos(PairPosBuilder {
			common: LookupCommon::new(glyph_map, location, LayoutTable::Gpos, 2),
			pairs: Vec::new(),
			glyph_pairs: IndexMap::new(),
			locations: HashMap::new(),
		})
	}

	/// `SinglePosBuilder(font, location)`.
	pub fn single_pos(glyph_map: GlyphMap, location: Location) -> Self {
		LookupBuilder::SinglePos(SinglePosBuilder {
			common: LookupCommon::new(glyph_map, location, LayoutTable::Gpos, 1),
			locations: HashMap::new(),
			mapping: IndexMap::new(),
		})
	}

	/// A builder of a kind.
	pub fn new(kind: LookupKind, glyph_map: GlyphMap, location: Location) -> Self {
		match kind {
			LookupKind::AlternateSubst => Self::alternate_subst(glyph_map, location),
			LookupKind::ChainContextPos => Self::chain_context_pos(glyph_map, location),
			LookupKind::ChainContextSubst => Self::chain_context_subst(glyph_map, location),
			LookupKind::LigatureSubst => Self::ligature_subst(glyph_map, location),
			LookupKind::MultipleSubst => Self::multiple_subst(glyph_map, location),
			LookupKind::CursivePos => Self::cursive_pos(glyph_map, location),
			LookupKind::MarkBasePos => Self::mark_base_pos(glyph_map, location),
			LookupKind::MarkLigPos => Self::mark_lig_pos(glyph_map, location),
			LookupKind::MarkMarkPos => Self::mark_mark_pos(glyph_map, location),
			LookupKind::ReverseChainSingleSubst => {
				Self::reverse_chain_single_subst(glyph_map, location)
			}
			LookupKind::AnySubst => Self::any_subst(glyph_map, location),
			LookupKind::SingleSubst => Self::single_subst(glyph_map, location),
			LookupKind::PairPos => Self::pair_pos(glyph_map, location),
			LookupKind::SinglePos => Self::single_pos(glyph_map, location),
		}
	}

	/// This builder as a [`LookupRef`].
	pub fn into_ref(self) -> LookupRef {
		Rc::new(RefCell::new(self))
	}

	/// The kind of builder.
	pub fn kind(&self) -> LookupKind {
		match self {
			LookupBuilder::AlternateSubst(_) => LookupKind::AlternateSubst,
			LookupBuilder::ChainContextPos(_) => LookupKind::ChainContextPos,
			LookupBuilder::ChainContextSubst(_) => LookupKind::ChainContextSubst,
			LookupBuilder::LigatureSubst(_) => LookupKind::LigatureSubst,
			LookupBuilder::MultipleSubst(_) => LookupKind::MultipleSubst,
			LookupBuilder::CursivePos(_) => LookupKind::CursivePos,
			LookupBuilder::MarkBasePos(_) => LookupKind::MarkBasePos,
			LookupBuilder::MarkLigPos(_) => LookupKind::MarkLigPos,
			LookupBuilder::MarkMarkPos(_) => LookupKind::MarkMarkPos,
			LookupBuilder::ReverseChainSingleSubst(_) => LookupKind::ReverseChainSingleSubst,
			LookupBuilder::AnySubst(_) => LookupKind::AnySubst,
			LookupBuilder::SingleSubst(_) => LookupKind::SingleSubst,
			LookupBuilder::PairPos(_) => LookupKind::PairPos,
			LookupBuilder::SinglePos(_) => LookupKind::SinglePos,
		}
	}

	/// The common settings.
	pub fn common(&self) -> &LookupCommon {
		each!(self, b => &b.common)
	}

	/// The common settings, mutably.
	pub fn common_mut(&mut self) -> &mut LookupCommon {
		each!(self, b => &mut b.common)
	}

	/// Whether two builders would build the same lookup (`equals`). Like
	/// fontTools, mark builders add their current marks to their subtables.
	pub fn equals(&mut self, other: &mut LookupBuilder) -> bool {
		if self.kind() != other.kind() || !self.common().equals(other.common()) {
			return false;
		}
		match (self, other) {
			(LookupBuilder::AlternateSubst(a), LookupBuilder::AlternateSubst(b)) => {
				a.alternates == b.alternates
			}
			(LookupBuilder::ChainContextPos(a), LookupBuilder::ChainContextPos(b))
			| (LookupBuilder::ChainContextSubst(a), LookupBuilder::ChainContextSubst(b)) => {
				a.rules == b.rules
			}
			(LookupBuilder::LigatureSubst(a), LookupBuilder::LigatureSubst(b)) => {
				a.ligatures == b.ligatures
			}
			(LookupBuilder::MultipleSubst(a), LookupBuilder::MultipleSubst(b)) => {
				a.mapping == b.mapping
			}
			(LookupBuilder::CursivePos(a), LookupBuilder::CursivePos(b)) => {
				a.attachments == b.attachments
			}
			(LookupBuilder::MarkBasePos(a), LookupBuilder::MarkBasePos(b)) => {
				a.get_subtables() == b.get_subtables()
			}
			(LookupBuilder::MarkLigPos(a), LookupBuilder::MarkLigPos(b)) => {
				a.get_subtables() == b.get_subtables()
			}
			(LookupBuilder::MarkMarkPos(a), LookupBuilder::MarkMarkPos(b)) => {
				a.get_subtables() == b.get_subtables()
			}
			(
				LookupBuilder::ReverseChainSingleSubst(a),
				LookupBuilder::ReverseChainSingleSubst(b),
			) => a.rules == b.rules,
			(LookupBuilder::AnySubst(a), LookupBuilder::AnySubst(b)) => a.mapping == b.mapping,
			(LookupBuilder::SingleSubst(a), LookupBuilder::SingleSubst(b)) => {
				a.mapping == b.mapping
			}
			(LookupBuilder::PairPos(a), LookupBuilder::PairPos(b)) => {
				a.glyph_pairs == b.glyph_pairs && a.pairs == b.pairs
			}
			(LookupBuilder::SinglePos(a), LookupBuilder::SinglePos(b)) => a.mapping == b.mapping,
			_ => false,
		}
	}

	/// The builders that this one becomes (`promote_lookup_type`): itself,
	/// except for [`AnySubstBuilder`]. `None` if a named lookup mixes kinds of
	/// substitutions.
	pub fn promote_lookup_type(self, is_named_lookup: bool) -> Result<Option<Vec<LookupBuilder>>> {
		match self {
			LookupBuilder::AnySubst(b) => b.promote_lookup_type(is_named_lookup),
			other => Ok(Some(vec![other])),
		}
	}

	/// The GDEF glyph classes that the rules imply, such as `{"cedilla": 3}`
	/// (`inferGlyphClasses`).
	pub fn infer_glyph_classes(&mut self) -> IndexMap<String, u16> {
		let mut result = IndexMap::new();
		match self {
			LookupBuilder::MarkBasePos(b) => {
				for (marks, bases) in b.get_subtables() {
					result.extend(bases.keys().map(|g| (g.clone(), 1)));
					result.extend(marks.keys().map(|g| (g.clone(), 3)));
				}
			}
			LookupBuilder::MarkLigPos(b) => {
				for (marks, ligatures) in b.get_subtables() {
					result.extend(ligatures.keys().map(|g| (g.clone(), 2)));
					result.extend(marks.keys().map(|g| (g.clone(), 3)));
				}
			}
			LookupBuilder::MarkMarkPos(b) => {
				for (marks, base_marks) in b.get_subtables() {
					result.extend(base_marks.keys().map(|g| (g.clone(), 3)));
					result.extend(marks.keys().map(|g| (g.clone(), 3)));
				}
			}
			_ => {}
		}
		result
	}

	/// The alternates of the glyphs, for building `aalt` (`getAlternateGlyphs`).
	///
	/// fontTools also returns the subtable breaks of single substitutions,
	/// which then fail as glyph names when the `aalt` lookup is built; they are
	/// left out here.
	pub fn get_alternate_glyphs(&self) -> Result<IndexMap<String, Vec<String>>> {
		// LENIENT(L29): no subtable breaks of single substitutions.
		Ok(match self {
			LookupBuilder::AlternateSubst(b) => b
				.alternates
				.iter()
				.filter_map(|(k, v)| k.key().map(|k| (k.clone(), v.clone())))
				.collect(),
			LookupBuilder::ChainContextSubst(b) => b.get_alternate_glyphs()?,
			LookupBuilder::LigatureSubst(b) => b
				.ligatures
				.iter()
				.filter_map(|(k, v)| match k {
					MapKey::Key(k) if k.len() == 1 => Some((k[0].clone(), vec![v.clone()])),
					_ => None,
				})
				.collect(),
			LookupBuilder::MultipleSubst(b) => b
				.mapping
				.iter()
				.filter_map(|(k, v)| match k {
					MapKey::Key(k) if v.len() == 1 => Some((k.clone(), v.clone())),
					_ => None,
				})
				.collect(),
			LookupBuilder::AnySubst(b) => b
				.mapping
				.iter()
				.filter_map(|(k, v)| match k {
					MapKey::Key(k) if k.len() == 1 && v.len() == 1 => {
						Some((k[0].clone(), v.clone()))
					}
					_ => None,
				})
				.collect(),
			LookupBuilder::SingleSubst(b) => b
				.mapping
				.iter()
				.filter_map(|(k, v)| k.key().map(|k| (k.clone(), vec![v.clone()])))
				.collect(),
			_ => IndexMap::new(),
		})
	}

	/// Adds an explicit subtable break (`add_subtable_break`). Lookup types that
	/// do not support it log a warning.
	pub fn add_subtable_break(&mut self, location: &Location) {
		let brk = || MapKey::SubtableBreak(location.clone());
		match self {
			LookupBuilder::AlternateSubst(b) => {
				b.alternates.insert(brk(), Vec::new());
			}
			LookupBuilder::ChainContextPos(b) | LookupBuilder::ChainContextSubst(b) => {
				b.rules.push(ChainContextualRule::SubtableBreak);
			}
			LookupBuilder::LigatureSubst(b) => {
				b.ligatures
					.insert(MapKey::SubtableBreak(location.clone()), String::new());
			}
			LookupBuilder::MultipleSubst(b) => {
				b.mapping.insert(brk(), Vec::new());
			}
			LookupBuilder::CursivePos(b) => {
				b.attachments.insert(brk(), (None, None));
			}
			LookupBuilder::MarkBasePos(b) => {
				let marks = std::mem::take(&mut b.marks);
				let bases = std::mem::take(&mut b.bases);
				b.subtables.push((marks, bases));
			}
			LookupBuilder::MarkLigPos(b) => {
				let marks = std::mem::take(&mut b.marks);
				let ligatures = std::mem::take(&mut b.ligatures);
				b.subtables.push((marks, ligatures));
			}
			LookupBuilder::MarkMarkPos(b) => {
				let marks = std::mem::take(&mut b.marks);
				let base_marks = std::mem::take(&mut b.base_marks);
				b.subtables.push((marks, base_marks));
			}
			// Each substitution is in its own subtable already.
			LookupBuilder::ReverseChainSingleSubst(_) => {}
			LookupBuilder::AnySubst(b) => {
				b.mapping
					.insert(MapKey::SubtableBreak(location.clone()), Vec::new());
			}
			LookupBuilder::SingleSubst(b) => {
				b.mapping.insert(brk(), String::new());
			}
			LookupBuilder::PairPos(b) => b.pairs.push(PairRule::SubtableBreak),
			LookupBuilder::SinglePos(_) => {
				let message = Error::otl(
					"unsupported \"subtable\" statement for lookup type",
					location,
				);
				log::warn!("{message}");
			}
		}
	}

	/// Whether the rules of `mapping` can be added (`can_add_mapping`); only
	/// [`AnySubstBuilder`] can refuse.
	pub fn can_add_mapping(&self, mapping: Option<&IndexMap<Vec<String>, Vec<String>>>) -> bool {
		match self {
			LookupBuilder::AnySubst(b) => b.can_add_mapping(mapping),
			_ => true,
		}
	}

	/// Builds the lookup (`build`); `None` if it has no subtables. The font
	/// provides the `otlLib` options and is used to measure contextual
	/// subtables.
	pub fn build(&mut self, font: &mut Font) -> Result<Option<ot::Lookup>> {
		match self {
			LookupBuilder::AlternateSubst(b) => {
				let subtables = split_at_breaks(&b.alternates)
					.iter()
					.map(|m| {
						build_alternate_subst_subtable(m, &b.common.glyph_map)
							.map(|s| s.map(LookupSubTable::AlternateSubst))
					})
					.collect::<Result<_>>()?;
				b.common.build_lookup(subtables)
			}
			LookupBuilder::ChainContextPos(b) | LookupBuilder::ChainContextSubst(b) => {
				chain::build(&b.common, b.rulesets(), font)
			}
			LookupBuilder::LigatureSubst(b) => {
				let subtables = split_at_breaks(&b.ligatures)
					.iter()
					.map(|m| {
						build_ligature_subst_subtable(m, &b.common.glyph_map)
							.map(|s| s.map(LookupSubTable::LigatureSubst))
					})
					.collect::<Result<_>>()?;
				b.common.build_lookup(subtables)
			}
			LookupBuilder::MultipleSubst(b) => {
				let subtables = split_at_breaks(&b.mapping)
					.iter()
					.map(|m| {
						build_multiple_subst_subtable(m, &b.common.glyph_map)
							.map(|s| s.map(LookupSubTable::MultipleSubst))
					})
					.collect::<Result<_>>()?;
				b.common.build_lookup(subtables)
			}
			LookupBuilder::SingleSubst(b) => {
				let subtables = split_at_breaks(&b.mapping)
					.iter()
					.map(|m| {
						build_single_subst_subtable(m, &b.common.glyph_map)
							.map(|s| s.map(LookupSubTable::SingleSubst))
					})
					.collect::<Result<_>>()?;
				b.common.build_lookup(subtables)
			}
			LookupBuilder::CursivePos(b) => {
				let subtables = split_at_breaks(&b.attachments)
					.iter()
					.map(|m| {
						build_cursive_pos_subtable(m, &b.common.glyph_map)
							.map(|s| s.map(LookupSubTable::CursivePos))
					})
					.collect::<Result<_>>()?;
				b.common.build_lookup(subtables)
			}
			LookupBuilder::MarkBasePos(b) => {
				let common = b.common.clone();
				let mut subtables = Vec::new();
				for (marks, bases) in b.get_subtables() {
					let classes = common.build_mark_classes(marks)?;
					let mut m: IndexMap<String, (u16, Anchor)> = IndexMap::new();
					for (mark, (mc, anchor)) in marks {
						let Some(&id) = classes.get(mc) else {
							return Err(Error::Value(format!(
								"Mark class {mc} not found for mark glyph {mark}"
							)));
						};
						m.insert(mark.clone(), (id, anchor.clone()));
					}
					let mut bs: IndexMap<String, BTreeMap<u16, Anchor>> = IndexMap::new();
					for (glyph, anchors) in bases {
						let entry = bs.entry(glyph.clone()).or_default();
						for (mc, anchor) in anchors {
							let Some(&id) = classes.get(mc) else {
								return Err(Error::Value(format!(
									"Mark class {mc} not found for base glyph {glyph}"
								)));
							};
							entry.insert(id, anchor.clone());
						}
					}
					subtables.push(Some(LookupSubTable::MarkBasePos(
						build_mark_base_pos_subtable(&m, &bs, &common.glyph_map)?,
					)));
				}
				common.build_lookup(subtables)
			}
			LookupBuilder::MarkLigPos(b) => {
				let common = b.common.clone();
				let mut subtables = Vec::new();
				for (marks, ligatures) in b.get_subtables() {
					let classes = common.build_mark_classes(marks)?;
					let class = |mc: &String| {
						classes.get(mc).copied().ok_or_else(|| {
							Error::Value(format!("KeyError: {}", crate::glyph_map::py_repr(mc)))
						})
					};
					let mut m: IndexMap<String, (u16, Anchor)> = IndexMap::new();
					for (mark, (mc, anchor)) in marks {
						m.insert(mark.clone(), (class(mc)?, anchor.clone()));
					}
					let mut ligs: IndexMap<String, Vec<BTreeMap<u16, Anchor>>> = IndexMap::new();
					for (lig, components) in ligatures {
						let mut list = Vec::new();
						for c in components {
							let mut map = BTreeMap::new();
							for (mc, a) in c {
								map.insert(class(mc)?, a.clone());
							}
							list.push(map);
						}
						ligs.insert(lig.clone(), list);
					}
					subtables.push(Some(LookupSubTable::MarkLigPos(
						build_mark_lig_pos_subtable(&m, &ligs, &common.glyph_map)?,
					)));
				}
				common.build_lookup(subtables)
			}
			LookupBuilder::MarkMarkPos(b) => {
				let common = b.common.clone();
				let gm = &common.glyph_map;
				let extended = gm.has_extended_glyph_ids();
				let mut subtables = Vec::new();
				for (marks, base_marks) in b.get_subtables() {
					let classes = common.build_mark_classes(marks)?;
					let class_list: Vec<&String> = classes.keys().collect();
					let mut m: IndexMap<String, (u16, Anchor)> = IndexMap::new();
					for (mark, (mc, anchor)) in marks {
						let id = classes.get(mc).copied().ok_or_else(|| {
							Error::Value(format!("KeyError: {}", crate::glyph_map::py_repr(mc)))
						})?;
						m.insert(mark.clone(), (id, anchor.clone()));
					}
					let mark1_coverage =
						Some(Box::new(build_coverage(m.keys().map(String::as_str), gm)?));
					let mark2_glyphs = sorted_glyphs(base_marks.keys().map(String::as_str), gm)?;
					let mark2_coverage = Some(Box::new(ot::Coverage {
						glyphs: mark2_glyphs.iter().map(|&(id, _)| id).collect(),
					}));
					let mark1_array = build_mark_array(&m, gm)?;
					let records: Vec<_> = mark2_glyphs
						.iter()
						.map(|&(_, base)| {
							let anchors = class_list
								.iter()
								.map(|mc| base_marks[base].get(*mc).cloned())
								.collect();
							build_mark2_record(anchors, extended)
						})
						.collect();
					let class_count = Some(classes.len() as u32);
					let mark2_count = Some(records.len() as u32);
					let st = match mark1_array {
						MaybeExtended::Extended(mark1_array) => {
							ot::MarkMarkPos::Format2(ot::MarkMarkPosFormat2 {
								mark1_coverage,
								mark2_coverage,
								class_count,
								mark1_array: Some(Box::new(mark1_array)),
								mark2_array: Some(Box::new(ot::Mark2Array2 {
									mark2_count,
									mark2_record: records
										.into_iter()
										.filter_map(MaybeExtended::extended)
										.collect(),
								})),
								dont_share: false,
							})
						}
						MaybeExtended::Normal(mark1_array) => {
							ot::MarkMarkPos::Format1(ot::MarkMarkPosFormat1 {
								mark1_coverage,
								mark2_coverage,
								class_count,
								mark1_array: Some(Box::new(mark1_array)),
								mark2_array: Some(Box::new(ot::Mark2Array {
									mark2_count,
									mark2_record: records
										.into_iter()
										.filter_map(MaybeExtended::normal)
										.collect(),
								})),
								dont_share: false,
							})
						}
					};
					subtables.push(Some(LookupSubTable::MarkMarkPos(st)));
				}
				common.build_lookup(subtables)
			}
			LookupBuilder::ReverseChainSingleSubst(b) => {
				let common = &b.common;
				let gm = &common.glyph_map;
				let mut subtables = Vec::new();
				for (prefix, suffix, mapping) in &b.rules {
					let mut reversed = prefix.clone();
					reversed.reverse();
					let backtrack_coverage = common.coverages(&reversed)?;
					let look_ahead_coverage = common.coverages(suffix)?;
					let glyphs = sorted_glyphs(mapping.keys().map(String::as_str), gm)?;
					let coverage = Some(Box::new(ot::Coverage {
						glyphs: glyphs.iter().map(|&(id, _)| id).collect(),
					}));
					let substitute = glyphs
						.iter()
						.map(|&(_, g)| gm.id(&mapping[g]))
						.collect::<Result<Vec<_>>>()?;
					let (bc, lc, gc) = (
						Some(backtrack_coverage.len() as u32),
						Some(look_ahead_coverage.len() as u32),
						Some(mapping.len() as u32),
					);
					let st = if gm.has_extended_glyph_ids() {
						ot::ReverseChainSingleSubst::Format2(ot::ReverseChainSingleSubstFormat2 {
							coverage,
							backtrack_glyph_count: bc,
							backtrack_coverage,
							look_ahead_glyph_count: lc,
							look_ahead_coverage,
							glyph_count: gc,
							substitute,
							dont_share: false,
						})
					} else {
						ot::ReverseChainSingleSubst::Format1(ot::ReverseChainSingleSubstFormat1 {
							coverage,
							backtrack_glyph_count: bc,
							backtrack_coverage,
							look_ahead_glyph_count: lc,
							look_ahead_coverage,
							glyph_count: gc,
							substitute,
							dont_share: false,
						})
					};
					subtables.push(Some(LookupSubTable::ReverseChainSingleSubst(st)));
				}
				common.build_lookup(subtables)
			}
			LookupBuilder::AnySubst(_) => {
				Err(Error::Assertion("AnySubstBuilder cannot be built".into()))
			}
			LookupBuilder::PairPos(b) => {
				let mut builder = ClassPairPosSubtableBuilder::new(b.common.glyph_map.clone());
				for rule in &b.pairs {
					match rule {
						PairRule::SubtableBreak => builder.add_subtable_break(),
						PairRule::ClassPair(gc1, v1, gc2, v2) => {
							builder.add_pair(gc1, v1.clone(), gc2, v2.clone())?;
						}
					}
				}
				let mut subtables: Vec<Option<LookupSubTable>> = Vec::new();
				if !b.glyph_pairs.is_empty() {
					subtables.extend(
						build_pair_pos_glyphs(&b.glyph_pairs, &b.common.glyph_map)?
							.into_iter()
							.map(|s| Some(LookupSubTable::PairPos(s))),
					);
				}
				subtables.extend(
					builder
						.subtables()?
						.into_iter()
						.map(|s| Some(LookupSubTable::PairPos(s))),
				);
				let mut lookup = b.common.build_lookup(subtables)?;
				// Compacting now gives smaller subtables, which may prevent overflows.
				let level = font.gpos_compression_level;
				if level != 0
					&& let Some(lookup) = &mut lookup
				{
					log::info!("Compacting GPOS...");
					crate::optimize::gpos::compact_lookup_with_map(
						&b.common.glyph_map,
						level,
						lookup,
					)?;
				}
				Ok(lookup)
			}
			LookupBuilder::SinglePos(b) => {
				let subtables = build_single_pos(&b.mapping, &b.common.glyph_map)?
					.into_iter()
					.map(|s| Some(LookupSubTable::SinglePos(s)))
					.collect();
				b.common.build_lookup(subtables)
			}
		}
	}
}
