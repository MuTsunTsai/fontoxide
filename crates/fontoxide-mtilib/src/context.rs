//! Contextual and chaining contextual subtables (`parseContext`,
//! `ContextHelper` and `bucketizeRules`).
//!
//! Their lookup records refer to lookups by name, which may be defined later in
//! the file; the subtables are made once all lookups are known.

use std::collections::{BTreeSet, HashMap};
use std::hash::Hash;

use fontoxide_ttlib::ot::types::{self as ot, ClassDef, Coverage, LookupSubTable};

use crate::parser::{
	Parser, coverage_of, ensure, first_token, fit, index_error, int_split_comma, line_repr,
	none_coverage, py_int, rest, strip_split_comma,
};
use crate::tokenizer::Tokenizer;
use crate::{Error, Result};

/// Substitution or positioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Which {
	Subst,
	Pos,
}

/// A lookup record, with the name of its lookup.
#[derive(Debug, Clone)]
struct RecordSrc {
	sequence_index: u16,
	lookup: String,
}

/// A rule: the backtrack, the input without its first item and the lookahead
/// sequences, and the lookup records.
#[derive(Debug, Clone)]
struct RuleSrc<T> {
	backtrack: Vec<T>,
	input: Vec<T>,
	look_ahead: Vec<T>,
	records: Vec<RecordSrc>,
}

#[derive(Debug)]
enum Body {
	/// Format 1: glyph sequences.
	Glyphs {
		coverage: Option<Coverage>,
		rule_sets: Vec<Option<Vec<RuleSrc<u32>>>>,
	},
	/// Format 2: class sequences.
	Classes {
		coverage: Option<Coverage>,
		class_defs: Vec<Option<ClassDef>>,
		rule_sets: Vec<Option<Vec<RuleSrc<i128>>>>,
	},
	/// Format 3: coverage sequences.
	Coverages {
		coverages: Vec<Vec<Option<Coverage>>>,
		records: Vec<RecordSrc>,
	},
}

/// A contextual subtable waiting for its lookup indices.
#[derive(Debug)]
pub(crate) struct ContextSrc {
	which: Which,
	chain: bool,
	body: Body,
}

type Rules<T> = Vec<(Vec<Vec<T>>, Vec<RecordSrc>)>;

/// `bucketizeRules`: the rules by the first item of their input, for each of
/// `keys`.
fn bucketize<T: Clone + Eq + Hash>(
	rules: Rules<T>,
	input_idx: usize,
	keys: &[T],
) -> Result<Vec<Option<Vec<RuleSrc<T>>>>> {
	let mut buckets: HashMap<T, Vec<RuleSrc<T>>> = HashMap::new();
	for (seq, records) in rules {
		let key = seq[input_idx]
			.first()
			.cloned()
			.ok_or_else(|| index_error("list"))?;
		let mut parts: Vec<Vec<T>> = seq
			.into_iter()
			.enumerate()
			.map(|(i, s)| if i == input_idx { s[1..].to_vec() } else { s })
			.collect();
		let rule = if parts.len() == 3 {
			let look_ahead = parts.pop().unwrap_or_default();
			let input = parts.pop().unwrap_or_default();
			let backtrack = parts.pop().unwrap_or_default();
			RuleSrc {
				backtrack,
				input,
				look_ahead,
				records,
			}
		} else {
			RuleSrc {
				backtrack: Vec::new(),
				input: parts.pop().unwrap_or_default(),
				look_ahead: Vec::new(),
				records,
			}
		};
		buckets.entry(key).or_default().push(rule);
	}
	Ok(keys.iter().map(|k| buckets.get(k).cloned()).collect())
}

impl Parser<'_> {
	/// `parseLookupRecords`.
	fn parse_lookup_records(&mut self, items: &[String]) -> Result<Vec<RecordSrc>> {
		let mut out = Vec::new();
		for item in items {
			let item = strip_split_comma(item);
			ensure(item.len() == 2, || line_repr(&item))?;
			let idx = py_int(&item[0])?;
			ensure(idx > 0, || idx.to_string())?;
			let sequence_index = fit(idx - 1, "SequenceIndex")?;
			self.lookup_ref(&item[1]);
			out.push(RecordSrc {
				sequence_index,
				lookup: item[1].clone(),
			});
		}
		Ok(out)
	}

	/// `parseContext`.
	pub(crate) fn parse_context(
		&mut self,
		lines: &mut Tokenizer<'_>,
		which: Which,
		chain: bool,
	) -> Result<ContextSrc> {
		let (data_len, input_idx) = if chain { (3, 1) } else { (1, 0) };
		let typ = first_token(&lines.peeks0())?.to_lowercase();
		let body = if typ == "glyph" {
			log::debug!("Parsing context format 1");
			let mut rules: Rules<u32> = Vec::new();
			while let Some(mut line) = lines.next() {
				ensure(line[0].to_lowercase() == "glyph", || line[0].clone())?;
				while line.len() < 1 + data_len {
					line.push(String::new());
				}
				let mut seq = Vec::new();
				for i in &line[1..1 + data_len] {
					let mut ids = Vec::new();
					for g in strip_split_comma(i) {
						ids.push(self.glyph_id(&g)?);
					}
					seq.push(ids);
				}
				let records = self.parse_lookup_records(&line[1 + data_len..])?;
				rules.push((seq, records));
			}
			let mut first = Vec::new();
			for (seq, _) in &rules {
				first.push(*seq[input_idx].first().ok_or_else(|| index_error("list"))?);
			}
			let coverage = coverage_of(first).ok_or_else(none_coverage)?;
			let rule_sets = bucketize(rules, input_idx, &coverage.glyphs)?;
			Body::Glyphs {
				coverage: Some(coverage),
				rule_sets,
			}
		} else if typ.ends_with("class") {
			log::debug!("Parsing context format 2");
			let mut class_defs: Vec<Option<ClassDef>> = vec![None; data_len];
			while lines.peeks0().ends_with("class definition begin") {
				let first = lines.peeks0();
				let typ = first[..first.len() - "class definition begin".len()].to_lowercase();
				let idx = match (data_len, typ.as_str()) {
					(1, "") => 0,
					(3, "backtrack") => 0,
					(3, "") => 1,
					(3, "lookahead") => 2,
					_ => return Err(crate::parser::key_error(&typ)),
				};
				ensure(class_defs[idx].is_none(), || idx.to_string())?;
				class_defs[idx] = self.parse_class_def(lines)?;
			}
			let mut rules: Rules<i128> = Vec::new();
			while let Some(mut line) = lines.next() {
				ensure(line[0].to_lowercase().starts_with("class"), || {
					line[0].clone()
				})?;
				while line.len() < 1 + data_len {
					line.push(String::new());
				}
				let seq = line[1..1 + data_len]
					.iter()
					.map(|i| int_split_comma(i))
					.collect::<Result<Vec<_>>>()?;
				let records = self.parse_lookup_records(&line[1 + data_len..])?;
				rules.push((seq, records));
			}
			let mut first_classes = BTreeSet::new();
			for (seq, _) in &rules {
				first_classes.insert(*seq[input_idx].first().ok_or_else(|| index_error("list"))?);
			}
			let input = class_defs[input_idx].as_ref().ok_or_else(|| {
				Error::Value(
					"AttributeError: 'NoneType' object has no attribute 'classDefs'".into(),
				)
			})?;
			let coverage = coverage_of(
				input
					.class_defs
					.iter()
					.filter(|(_, c)| first_classes.contains(&i128::from(**c)))
					.map(|(g, _)| *g),
			);
			let Some(&max) = first_classes.last() else {
				return Err(Error::Value(
					"ValueError: max() iterable argument is empty".into(),
				));
			};
			if max >= 0 {
				fit::<u16>(max, "Class")?;
			}
			let keys: Vec<i128> = (0..=max).collect();
			let rule_sets = bucketize(rules, input_idx, &keys)?;
			Body::Classes {
				coverage,
				class_defs,
				rule_sets,
			}
		} else if typ.ends_with("coverage") {
			log::debug!("Parsing context format 3");
			let mut coverages: Vec<Vec<Option<Coverage>>> = vec![Vec::new(); data_len];
			while lines.peeks0().ends_with("coverage definition begin") {
				let first = lines.peeks0();
				let typ = first[..first.len() - "coverage definition begin".len()].to_lowercase();
				let idx = match (data_len, typ.as_str()) {
					(1, "") => 0,
					(3, "backtrack") => 0,
					(3, "input") => 1,
					(3, "lookahead") => 2,
					_ => return Err(crate::parser::key_error(&typ)),
				};
				let coverage = self.parse_coverage(lines)?;
				coverages[idx].push(coverage);
			}
			let rest = rest(lines);
			ensure(rest.len() == 1, String::new)?;
			let line = &rest[0];
			ensure(line[0].to_lowercase() == "coverage", || line[0].clone())?;
			let records = self.parse_lookup_records(&line[1..])?;
			Body::Coverages { coverages, records }
		} else {
			return Err(Error::Assertion(typ));
		};
		Ok(ContextSrc { which, chain, body })
	}
}

/// The class sequence of a rule, as written.
fn classes(v: Vec<i128>) -> Result<Vec<u16>> {
	v.into_iter().map(|c| fit(c, "Class")).collect()
}

fn boxed(v: Vec<Option<Coverage>>) -> Vec<Option<Box<Coverage>>> {
	v.into_iter().map(|c| c.map(Box::new)).collect()
}

/// Three items of a vector, `None` (or empty) for missing ones.
fn three<T: Default>(v: Vec<T>) -> (T, T, T) {
	let mut it = v.into_iter();
	(
		it.next().unwrap_or_default(),
		it.next().unwrap_or_default(),
		it.next().unwrap_or_default(),
	)
}

macro_rules! finish_family {
	(
		$fn:ident, $Rec:ident, $count:ident, $records:ident,
		$Ctx:ident, $F1:ident, $F2:ident, $F3:ident,
		$RuleSet:ident { $rs_count:ident, $rs:ident, $r_count:ident, $r:ident }, $Rule:ident,
		$ClassSet:ident { $cs_count:ident, $cs:ident, $cr_count:ident, $cr:ident }, $ClassRule:ident,
		$CCtx:ident, $CF1:ident, $CF2:ident, $CF3:ident,
		$CRuleSet:ident { $crs_count:ident, $crs:ident, $crr_count:ident, $crr:ident }, $CRule:ident,
		$CClassSet:ident { $ccs_count:ident, $ccs:ident, $ccr_count:ident, $ccr:ident }, $CClassRule:ident
	) => {
		fn $fn(
			chain: bool,
			body: Body,
			lookup_index: &dyn Fn(&str) -> Result<u16>,
		) -> Result<LookupSubTable> {
			let records = |recs: &[RecordSrc]| -> Result<Vec<ot::$Rec>> {
				recs.iter()
					.map(|r| {
						Ok(ot::$Rec {
							sequence_index: r.sequence_index,
							lookup_list_index: lookup_index(&r.lookup)?,
						})
					})
					.collect()
			};
			Ok(match (chain, body) {
				(
					false,
					Body::Glyphs {
						coverage,
						rule_sets,
					},
				) => {
					let mut sets = Vec::new();
					for rules in rule_sets {
						let Some(rules) = rules else {
							sets.push(None);
							continue;
						};
						let mut out = Vec::new();
						for r in rules {
							let recs = records(&r.records)?;
							out.push(Some(Box::new(ot::$Rule {
								glyph_count: Some(r.input.len() as u32 + 1),
								$count: Some(recs.len() as u32),
								input: r.input,
								$records: recs,
							})));
						}
						sets.push(Some(Box::new(ot::$RuleSet {
							$r_count: Some(out.len() as u32),
							$r: out,
						})));
					}
					LookupSubTable::$Ctx(ot::$Ctx::Format1(ot::$F1 {
						coverage: coverage.map(Box::new),
						$rs_count: Some(sets.len() as u32),
						$rs: sets,
						dont_share: false,
					}))
				}
				(
					false,
					Body::Classes {
						coverage,
						class_defs,
						rule_sets,
					},
				) => {
					let mut sets = Vec::new();
					for rules in rule_sets {
						let Some(rules) = rules else {
							sets.push(None);
							continue;
						};
						let mut out = Vec::new();
						for r in rules {
							let recs = records(&r.records)?;
							out.push(Some(Box::new(ot::$ClassRule {
								glyph_count: Some(r.input.len() as u32 + 1),
								$count: Some(recs.len() as u32),
								class: classes(r.input)?,
								$records: recs,
							})));
						}
						sets.push(Some(Box::new(ot::$ClassSet {
							$cr_count: Some(out.len() as u32),
							$cr: out,
						})));
					}
					LookupSubTable::$Ctx(ot::$Ctx::Format2(ot::$F2 {
						coverage: coverage.map(Box::new),
						class_def: class_defs.into_iter().next().flatten().map(Box::new),
						$cs_count: Some(sets.len() as u32),
						$cs: sets,
						dont_share: false,
					}))
				}
				(
					false,
					Body::Coverages {
						coverages,
						records: recs,
					},
				) => {
					let coverage = boxed(coverages.into_iter().next().unwrap_or_default());
					let recs = records(&recs)?;
					LookupSubTable::$Ctx(ot::$Ctx::Format3(ot::$F3 {
						glyph_count: Some(coverage.len() as u32),
						$count: Some(recs.len() as u32),
						coverage,
						$records: recs,
						dont_share: false,
					}))
				}
				(
					true,
					Body::Glyphs {
						coverage,
						rule_sets,
					},
				) => {
					let mut sets = Vec::new();
					for rules in rule_sets {
						let Some(rules) = rules else {
							sets.push(None);
							continue;
						};
						let mut out = Vec::new();
						for r in rules {
							let recs = records(&r.records)?;
							out.push(Some(Box::new(ot::$CRule {
								backtrack_glyph_count: Some(r.backtrack.len() as u32),
								backtrack: r.backtrack,
								input_glyph_count: Some(r.input.len() as u32 + 1),
								input: r.input,
								look_ahead_glyph_count: Some(r.look_ahead.len() as u32),
								look_ahead: r.look_ahead,
								$count: Some(recs.len() as u32),
								$records: recs,
							})));
						}
						sets.push(Some(Box::new(ot::$CRuleSet {
							$crr_count: Some(out.len() as u32),
							$crr: out,
						})));
					}
					LookupSubTable::$CCtx(ot::$CCtx::Format1(ot::$CF1 {
						coverage: coverage.map(Box::new),
						$crs_count: Some(sets.len() as u32),
						$crs: sets,
						dont_share: false,
					}))
				}
				(
					true,
					Body::Classes {
						coverage,
						class_defs,
						rule_sets,
					},
				) => {
					let mut sets = Vec::new();
					for rules in rule_sets {
						let Some(rules) = rules else {
							sets.push(None);
							continue;
						};
						let mut out = Vec::new();
						for r in rules {
							let recs = records(&r.records)?;
							out.push(Some(Box::new(ot::$CClassRule {
								backtrack_glyph_count: Some(r.backtrack.len() as u32),
								backtrack: classes(r.backtrack)?,
								input_glyph_count: Some(r.input.len() as u32 + 1),
								input: classes(r.input)?,
								look_ahead_glyph_count: Some(r.look_ahead.len() as u32),
								look_ahead: classes(r.look_ahead)?,
								$count: Some(recs.len() as u32),
								$records: recs,
							})));
						}
						sets.push(Some(Box::new(ot::$CClassSet {
							$ccr_count: Some(out.len() as u32),
							$ccr: out,
						})));
					}
					let (backtrack, input, look_ahead) = three(class_defs);
					LookupSubTable::$CCtx(ot::$CCtx::Format2(ot::$CF2 {
						coverage: coverage.map(Box::new),
						backtrack_class_def: backtrack.map(Box::new),
						input_class_def: input.map(Box::new),
						look_ahead_class_def: look_ahead.map(Box::new),
						$ccs_count: Some(sets.len() as u32),
						$ccs: sets,
						dont_share: false,
					}))
				}
				(
					true,
					Body::Coverages {
						coverages,
						records: recs,
					},
				) => {
					let (backtrack, input, look_ahead) = three(coverages);
					let recs = records(&recs)?;
					LookupSubTable::$CCtx(ot::$CCtx::Format3(ot::$CF3 {
						backtrack_glyph_count: Some(backtrack.len() as u32),
						backtrack_coverage: boxed(backtrack),
						input_glyph_count: Some(input.len() as u32),
						input_coverage: boxed(input),
						look_ahead_glyph_count: Some(look_ahead.len() as u32),
						look_ahead_coverage: boxed(look_ahead),
						$count: Some(recs.len() as u32),
						$records: recs,
						dont_share: false,
					}))
				}
			})
		}
	};
}

finish_family!(
	finish_subst,
	SubstLookupRecord,
	subst_count,
	subst_lookup_record,
	ContextSubst,
	ContextSubstFormat1,
	ContextSubstFormat2,
	ContextSubstFormat3,
	SubRuleSet {
		sub_rule_set_count,
		sub_rule_set,
		sub_rule_count,
		sub_rule
	},
	SubRule,
	SubClassSet {
		sub_class_set_count,
		sub_class_set,
		sub_class_rule_count,
		sub_class_rule
	},
	SubClassRule,
	ChainContextSubst,
	ChainContextSubstFormat1,
	ChainContextSubstFormat2,
	ChainContextSubstFormat3,
	ChainSubRuleSet {
		chain_sub_rule_set_count,
		chain_sub_rule_set,
		chain_sub_rule_count,
		chain_sub_rule
	},
	ChainSubRule,
	ChainSubClassSet {
		chain_sub_class_set_count,
		chain_sub_class_set,
		chain_sub_class_rule_count,
		chain_sub_class_rule
	},
	ChainSubClassRule
);

finish_family!(
	finish_pos,
	PosLookupRecord,
	pos_count,
	pos_lookup_record,
	ContextPos,
	ContextPosFormat1,
	ContextPosFormat2,
	ContextPosFormat3,
	PosRuleSet {
		pos_rule_set_count,
		pos_rule_set,
		pos_rule_count,
		pos_rule
	},
	PosRule,
	PosClassSet {
		pos_class_set_count,
		pos_class_set,
		pos_class_rule_count,
		pos_class_rule
	},
	PosClassRule,
	ChainContextPos,
	ChainContextPosFormat1,
	ChainContextPosFormat2,
	ChainContextPosFormat3,
	ChainPosRuleSet {
		chain_pos_rule_set_count,
		chain_pos_rule_set,
		chain_pos_rule_count,
		chain_pos_rule
	},
	ChainPosRule,
	ChainPosClassSet {
		chain_pos_class_set_count,
		chain_pos_class_set,
		chain_pos_class_rule_count,
		chain_pos_class_rule
	},
	ChainPosClassRule
);

impl ContextSrc {
	/// The subtable, with the lookup indices of the lookup names.
	pub(crate) fn finish(
		self,
		lookup_index: &dyn Fn(&str) -> Result<u16>,
	) -> Result<LookupSubTable> {
		match self.which {
			Which::Subst => finish_subst(self.chain, self.body, lookup_index),
			Which::Pos => finish_pos(self.chain, self.body, lookup_index),
		}
	}
}
