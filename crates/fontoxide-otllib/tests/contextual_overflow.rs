//! `Tests/otlLib/contextual_overflow_test.py`.

use fontoxide_otllib::builder::{ChainContextualRule, LookupBuilder, RuleLookup};
use fontoxide_otllib::{Error, GlyphMap, Result};
use fontoxide_ttlib::Font;
use fontoxide_ttlib::ot::types::{self as ot, LookupSubTable};

fn make_builder() -> Result<(Font, LookupBuilder, Vec<Vec<String>>)> {
	let mut font = Font::new();
	let glyphs: Vec<String> = std::iter::once(".notdef".to_owned())
		.chain((1..44000).map(|i| format!("g{i}")))
		.collect();
	let mut order = glyphs.clone();
	order.extend(["before".to_owned(), "after".to_owned()]);
	font.set_glyph_order(order);
	let builder = LookupBuilder::chain_context_subst(GlyphMap::from_font(&mut font)?, None);
	let even: Vec<String> = glyphs.iter().skip(2).step_by(2).cloned().collect();
	let odd: Vec<String> = glyphs.iter().skip(1).step_by(2).cloned().collect();
	// Overlapping classes rule out format 2; each coverage is about 44KB, so
	// format 3 overflows the lookup offsets when all four rules are sized
	// together, but each consecutive pair fits.
	let with = |first: &str, rest: &[String]| {
		let mut v = vec![first.to_owned()];
		v.extend_from_slice(rest);
		v
	};
	let classes = vec![
		even.clone(),
		with("g1", &even),
		odd.clone(),
		with("g2", &odd),
	];
	Ok((font, builder, classes))
}

/// A lookup that is only there for its index (fontTools' test uses a
/// `SimpleNamespace(lookup_index=index)`).
fn indexed(index: usize, gm: &GlyphMap) -> RuleLookup {
	let lookup = LookupBuilder::single_subst(gm.clone(), None).into_ref();
	lookup.borrow_mut().common_mut().lookup_index = Some(index);
	RuleLookup::One(lookup)
}

fn rules(builder: &mut LookupBuilder) -> &mut Vec<ChainContextualRule> {
	match builder {
		LookupBuilder::ChainContextSubst(b) => &mut b.rules,
		_ => unreachable!(),
	}
}

fn sorted(mut v: Vec<u32>) -> Vec<u32> {
	v.sort_unstable();
	v
}

#[test]
fn test_overflow_splits_rules_in_order() -> Result<()> {
	let (mut font, mut builder, classes) = make_builder()?;
	let gm = builder.common().glyph_map.clone();
	for (index, glyphs) in classes.iter().enumerate() {
		// Only the last rule has context. Splitting must keep the lookup chaining
		// for the earlier rules, which have neither prefix nor suffix.
		let context = index == 3;
		let around = |g: &str| {
			if context {
				vec![vec![g.to_owned()]]
			} else {
				vec![]
			}
		};
		rules(&mut builder).push(ChainContextualRule::new(
			around("before"),
			vec![glyphs.clone()],
			around("after"),
			vec![(index != 0).then(|| indexed(index, &gm))],
		));
	}
	let lookup = builder.build(&mut font)?.expect("lookup");
	assert_eq!(lookup.sub_table.len(), 4);
	let id = |name: &str| gm.get(name).expect("glyph");
	for (index, (subtable, glyphs)) in lookup.sub_table.iter().zip(&classes).enumerate() {
		let Some(LookupSubTable::ChainContextSubst(ot::ChainContextSubst::Format3(st))) =
			subtable.as_deref()
		else {
			panic!("ChainContextSubst format 3 expected");
		};
		let coverage = |c: &Option<Box<ot::Coverage>>| c.as_ref().expect("coverage").glyphs.clone();
		let input: Vec<Vec<u32>> = st
			.input_coverage
			.iter()
			.map(|c| sorted(coverage(c)))
			.collect();
		assert_eq!(input, [sorted(glyphs.iter().map(|g| id(g)).collect())]);
		let backtrack: Vec<Vec<u32>> = st.backtrack_coverage.iter().map(coverage).collect();
		let look_ahead: Vec<Vec<u32>> = st.look_ahead_coverage.iter().map(coverage).collect();
		if index == 3 {
			assert_eq!(backtrack, [[id("before")]]);
			assert_eq!(look_ahead, [[id("after")]]);
		} else {
			assert!(backtrack.is_empty() && look_ahead.is_empty());
		}
		let records: Vec<(u16, u16)> = st
			.subst_lookup_record
			.iter()
			.map(|r| (r.sequence_index, r.lookup_list_index))
			.collect();
		if index == 0 {
			assert!(records.is_empty());
		} else {
			assert_eq!(records, [(0, index as u16)]);
		}
	}
	Ok(())
}

#[test]
fn test_single_rule_overflow_still_raises() -> Result<()> {
	let (mut font, mut builder, classes) = make_builder()?;
	// An offset overflow inside a single format 3 subtable cannot be split.
	rules(&mut builder).push(ChainContextualRule::new(
		vec![],
		classes[..3].to_vec(),
		vec![],
		vec![None, None, None],
	));
	let err = builder.build(&mut font).expect_err("overflow");
	assert!(matches!(&err, Error::OpenTypeLib { .. }));
	assert_eq!(err.to_string(), "All candidates overflowed");
	Ok(())
}
