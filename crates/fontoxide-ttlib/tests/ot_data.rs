//! `Tests/ttLib/tables/otData_test.py`.
//!
//! The tests check the `otData` definitions; here they check the types generated
//! from them: the formats a table has, the types of the fields, and the size of
//! the counts and offsets they compile to (the expected data comes from
//! fontTools).

#![allow(clippy::unwrap_used)]

use fontoxide_misc::text_tools::hex_str;
use fontoxide_ttlib::ot::WriterGraph;
use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::{Font, Tag};

fn font() -> Font {
	let mut font = Font::new();
	font.set_glyph_order((0..10).map(|i| format!("g{i}")).collect());
	font
}

fn compile<T: OtTable>(table: &T) -> TestResult<String> {
	let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
	table.compile(&mut graph.root(), &mut font())?;
	Ok(hex_str(&graph.get_all_data()?))
}

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// The formats of a format-switching table, among 0 to 9.
fn formats<T: OtTable>(is_unknown: impl Fn(&T) -> bool) -> Vec<u16> {
	(0..10)
		.filter(|&f| !is_unknown(&T::with_format(f)))
		.collect()
}

#[test]
fn test_extended_layout_formats() {
	assert_eq!(
		formats(|t: &ot::CoverageRaw| matches!(t, ot::CoverageRaw::Unknown(_))),
		[1, 2, 3, 4]
	);
	assert_eq!(
		formats(|t: &ot::ClassDefRaw| matches!(t, ot::ClassDefRaw::Unknown(_))),
		[1, 2, 3, 4]
	);
	assert_eq!(
		formats(|t: &ot::ContextSubst| matches!(t, ot::ContextSubst::Unknown(_))),
		[1, 2, 3, 4, 5, 6]
	);
	assert_eq!(
		formats(|t: &ot::ContextPos| matches!(t, ot::ContextPos::Unknown(_))),
		[1, 2, 3, 4, 5, 6]
	);
	assert_eq!(
		formats(|t: &ot::ChainContextSubst| matches!(t, ot::ChainContextSubst::Unknown(_))),
		[1, 2, 3, 4, 5]
	);
	assert_eq!(
		formats(|t: &ot::ChainContextPos| matches!(t, ot::ChainContextPos::Unknown(_))),
		[1, 2, 3, 4, 5]
	);
}

#[test]
fn test_extended_layout_header_fields() -> TestResult {
	// The types of the fields (`LOffsetTo(ScriptList)` and so on).
	type Lists<'a> = (
		&'a Option<Box<ot::ScriptList>>,
		&'a Option<Box<ot::FeatureList>>,
		&'a Option<Box<ot::LookupList2>>,
	);
	fn gsub(t: &ot::GSUB) -> Lists<'_> {
		(&t.script_list2, &t.feature_list2, &t.lookup_list2)
	}
	fn gpos(t: &ot::GPOS) -> Lists<'_> {
		(&t.script_list2, &t.feature_list2, &t.lookup_list2)
	}
	fn gdef(t: &ot::GDEF) -> (&Option<Box<ot::ClassDef>>, &Option<Box<ot::LigCaretList2>>) {
		(&t.glyph_class_def2, &t.lig_caret_list2)
	}
	let _ = (gsub, gpos, gdef);

	// The extended offsets are 32-bit (`LOffsetTo`): Version, three null 16-bit
	// offsets, FeatureVariations, then ScriptList2, FeatureList2, LookupList2.
	let table = ot::GSUB {
		version: 0x00010002,
		script_list2: Some(Box::default()),
		feature_list2: Some(Box::default()),
		lookup_list2: Some(Box::default()),
		..Default::default()
	};
	assert_eq!(
		compile(&table)?,
		"00010002000000000000000000000000001a0000001c0000001e000000000000"
	);

	// `LigCaretList2.LigGlyph` is `Offset24To(LigGlyph)`, with a 24-bit count.
	let table = ot::LigCaretList2 {
		coverage: Some(Box::new(ot::Coverage { glyphs: vec![1] })),
		lig_glyph_count: None,
		lig_glyph: vec![Some(Box::default())],
	};
	assert_eq!(compile(&table)?, "0000000a0000010000100001000100010000");

	// `ClassDefFormat3.ClassValueArray` is `uint24`.
	let table = ot::ClassDefRaw::Format3(ot::ClassDefRawFormat3 {
		start_glyph: 1,
		glyph_count: None,
		class_value_array: vec![0x10000],
	});
	assert_eq!(compile(&table)?, "0003000001000001010000");

	// `JSTF.JstfScriptRecord2` is only there from version 1.1 on.
	let mut table = ot::JSTF {
		version: 0x00010000,
		..Default::default()
	};
	assert_eq!(compile(&table)?, "000100000000");
	table.version = 0x00010001;
	assert_eq!(compile(&table)?, "0001000100000000");
	Ok(())
}

fn chained_seq_rule2() -> ot::ChainedSeqRule2 {
	ot::ChainedSeqRule2 {
		backtrack_sequence: vec![5],
		input_sequence: vec![6],
		look_ahead_sequence: vec![7],
		..Default::default()
	}
}

#[test]
fn test_extended_context_count_exceptions() -> TestResult {
	// These counts remain 16-bit despite the surrounding extended offsets and
	// glyph IDs.
	assert_eq!(
		compile(&chained_seq_rule2())?,
		"0001000005000200000600010000070000"
	);
	let table = ot::ReverseChainSingleSubst::Format2(ot::ReverseChainSingleSubstFormat2 {
		coverage: Some(Box::new(ot::Coverage { glyphs: vec![1] })),
		backtrack_coverage: vec![Some(Box::new(ot::Coverage { glyphs: vec![2] }))],
		substitute: vec![3],
		..Default::default()
	});
	let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
	{
		let mut root = graph.root();
		root.set(
			"LookupType",
			fontoxide_ttlib::ot::StateValue::Count(fontoxide_ttlib::ot::CountRef::new(
				"LookupType",
				2,
				None,
			)),
		);
		table.compile(&mut root, &mut font())?;
	}
	assert_eq!(
		hex_str(&graph.get_all_data()?),
		"00020000001300010000190000000001000003000100010001000100010002"
	);
	Ok(())
}

#[test]
fn test_extended_chained_rule_offsets() -> TestResult {
	let table = ot::ChainedSeqRuleSet2 {
		chained_seq_rule_count: None,
		chained_seq_rule: vec![Some(Box::new(chained_seq_rule2()))],
	};
	assert_eq!(
		compile(&table)?,
		"00010000050001000005000200000600010000070000"
	);
	Ok(())
}
