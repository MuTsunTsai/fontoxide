//! `Tests/ttLib/tables/_m_o_r_t_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;

use common::{TestResult, compile_from_xml, decompile_to_xml, fake_font, get_xml, hex, hex_str};
use fontoxide_ttlib::ot::morph::{AatStateTable, LigAction, MorphSubStruct, MortSubtable};
use fontoxide_ttlib::tables::otl::Mort;
use fontoxide_ttlib::{Font, Table, TableType, Tag};

const TAG: Tag = Tag::new(b"mort");

fn noncontextual_glyphs() -> Vec<String> {
	let mut g: Vec<String> = std::iter::once(".notdef".to_owned())
		.chain((1..140).map(|i| format!("g.{i}")))
		.collect();
	g[11] = "parenleft".into();
	g[13] = "parenright".into();
	g[135] = "parenleft.vertical".into();
	g[136] = "parenright.vertical".into();
	g
}

fn refs(names: &[String]) -> Vec<&str> {
	names.iter().map(String::as_str).collect()
}

#[test]
fn noncontextual_test_decompile_to_xml() -> TestResult {
	let glyphs = noncontextual_glyphs();
	assert_eq!(
		decompile_to_xml(TAG, &refs(&glyphs), &hex(MORT_NONCONTEXTUAL_DATA))?,
		MORT_NONCONTEXTUAL_XML
	);
	Ok(())
}

#[test]
fn noncontextual_test_compile_from_xml() -> TestResult {
	let glyphs = noncontextual_glyphs();
	assert_eq!(
		hex_str(&compile_from_xml(
			TAG,
			&refs(&glyphs),
			MORT_NONCONTEXTUAL_XML
		)?),
		hex_str(&hex(MORT_NONCONTEXTUAL_DATA))
	);
	Ok(())
}

const ALL_TYPES_GLYPHS: &[&str] = &[
	".notdef",
	"space",
	"A",
	"B",
	"C",
	"D",
	"E",
	"F",
	"G",
	"H",
	"O",
	"X",
	"Y",
	"Z",
	"zero",
	"one",
	"one_zero",
	"one_one",
	"one_two",
	"one_three",
	"one_four",
	"one_five",
	"two",
	"three",
	"four",
	"five",
	"six",
	"seven",
	"eight",
	"nine",
];

fn decompile(data: &[u8], font: &mut Font) -> fontoxide_ttlib::Result<Mort> {
	Mort::decompile(data, font)
}

/// `getXML(table.toXML)` parsed into a new table and compiled.
fn xml_round_trip(table: &Mort, font: &mut Font) -> fontoxide_ttlib::Result<Vec<u8>> {
	let xml = get_xml(table, font)?;
	let mut compiled = Mort::default();
	common::from_xml(&mut compiled, &xml.concat(), font)?;
	compiled.compile(font)
}

fn subtables(table: &Mort) -> &[MortSubtable] {
	&table.morph_chain[0].morph_subtable
}

fn state_table(subtable: &MortSubtable) -> &AatStateTable {
	match subtable.sub_struct.as_ref().expect("a subtable") {
		MorphSubStruct::Rearrangement(t) => &t.state_table,
		MorphSubStruct::Contextual(t) => &t.state_table,
		MorphSubStruct::Ligature(t) => &t.state_table,
		MorphSubStruct::Insertion(t) => &t.state_table,
		MorphSubStruct::Noncontextual(_) => panic!("not a state table"),
	}
}

fn state_table_mut(subtable: &mut MortSubtable) -> &mut AatStateTable {
	match subtable.sub_struct.as_mut().expect("a subtable") {
		MorphSubStruct::Rearrangement(t) => &mut t.state_table,
		MorphSubStruct::Contextual(t) => &mut t.state_table,
		MorphSubStruct::Ligature(t) => &mut t.state_table,
		MorphSubStruct::Insertion(t) => &mut t.state_table,
		MorphSubStruct::Noncontextual(_) => panic!("not a state table"),
	}
}

fn gid(font: &mut Font, name: &str) -> u32 {
	font.glyph_id(name).unwrap()
}

/// `{name: name}` lookups by glyph ID.
fn lookup(font: &mut Font, pairs: &[(&str, &str)]) -> BTreeMap<u32, u32> {
	pairs
		.iter()
		.map(|(k, v)| (gid(font, k), gid(font, v)))
		.collect()
}

// Flag indices of the actions (in `flag_names` order).
const MARK_FIRST: usize = 0;
const MARK_LAST: usize = 2;
const SET_COMPONENT: usize = 0;

fn assert_semantics(table: &Mort, font: &mut Font) {
	let subtables = subtables(table);
	let types: Vec<u32> = subtables
		.iter()
		.map(|s| s.sub_struct.as_ref().unwrap().lookup_type())
		.collect();
	assert_eq!(types, [0, 1, 2, 4, 5]);

	let rearrangement = state_table(&subtables[0]);
	assert!(rearrangement.states[0].transitions[&4].flags[MARK_FIRST]);
	let transition = &rearrangement.states[0].transitions[&5];
	assert!(transition.flags[MARK_LAST]);
	assert_eq!(transition.verb, 1);

	let contextual = state_table(&subtables[1]);
	assert_eq!(contextual.per_glyph_lookups, [lookup(font, &[("C", "D")])]);
	assert_eq!(contextual.states[0].transitions[&4].current_index, 0);

	let ligature = state_table(&subtables[2]);
	let transition = &ligature.states[1].transitions[&5];
	assert!(transition.flags[SET_COMPONENT]);
	assert_eq!(
		transition.actions,
		[
			LigAction {
				store: false,
				glyph_index_delta: 0
			},
			LigAction {
				store: false,
				glyph_index_delta: 30
			},
		]
	);
	assert_eq!(ligature.lig_components, Some(vec![0; 60]));
	assert_eq!(ligature.ligatures, Some(vec![gid(font, "one")]));

	match subtables[3].sub_struct.as_ref().unwrap() {
		MorphSubStruct::Noncontextual(t) => {
			assert_eq!(t.substitution, lookup(font, &[("G", "H")]));
		}
		other => panic!("unexpected subtable {other:?}"),
	}

	let insertion = state_table(&subtables[4]);
	assert_eq!(
		insertion.states[0].transitions[&4].current_insertion_action,
		[gid(font, "Y")]
	);
}

#[test]
fn all_types_test_decompile_all_types() -> TestResult {
	let mut font = fake_font(ALL_TYPES_GLYPHS);
	let table = decompile(&hex(MORT_ALL_TYPES_DATA), &mut font)?;
	assert_semantics(&table, &mut font);
	Ok(())
}

#[test]
fn all_types_test_binary_roundtrip() -> TestResult {
	let mut font = fake_font(ALL_TYPES_GLYPHS);
	let mut table = decompile(&hex(MORT_ALL_TYPES_DATA), &mut font)?;
	let data = table.compile(&mut font)?;
	assert_semantics(&decompile(&data, &mut font)?, &mut font);
	Ok(())
}

#[test]
fn all_types_test_xml_roundtrip() -> TestResult {
	let mut font = fake_font(ALL_TYPES_GLYPHS);
	let table = decompile(&hex(MORT_ALL_TYPES_DATA), &mut font)?;
	let data = xml_round_trip(&table, &mut font)?;
	assert_semantics(&decompile(&data, &mut font)?, &mut font);
	Ok(())
}

#[test]
fn all_types_test_xml_roundtrip_preserves_unused_glyph_class() -> TestResult {
	let mut font = fake_font(ALL_TYPES_GLYPHS);
	let mut table = decompile(&hex(MORT_ALL_TYPES_DATA), &mut font)?;
	let state_table = state_table_mut(&mut table.table.morph_chain[0].morph_subtable[0]);
	state_table.glyph_class_count = 7;
	for state in &mut state_table.states {
		let transition = state.transitions[&5].clone();
		state.transitions.insert(6, transition);
	}

	let data = xml_round_trip(&table, &mut font)?;
	let table = decompile(&data, &mut font)?;
	let state_table = self::state_table(&subtables(&table)[0]);
	assert_eq!(state_table.glyph_class_count, 7);
	assert!(state_table.states[0].transitions.contains_key(&6));
	Ok(())
}

fn trimmed_glyphs() -> Vec<String> {
	std::iter::once(".notdef".to_owned())
		.chain((1..30).map(|i| format!("g{i}")))
		.chain(["hyphen".to_owned(), "emdash".to_owned()])
		.chain((32..40).map(|i| format!("h{i}")))
		.collect()
}

fn assert_trimmed_semantics(table: &Mort, font: &mut Font) {
	let state_table = state_table(&subtables(table)[0]);
	assert_eq!(
		state_table.per_glyph_lookups,
		[lookup(font, &[("hyphen", "emdash")])]
	);
	assert_eq!(
		state_table.glyph_classes,
		[(gid(font, "hyphen"), 4)].into_iter().collect()
	);
	let transition = &state_table.states[1].transitions[&4];
	assert_eq!(transition.mark_index, 0);
	assert_eq!(transition.current_index, 0xFFFF);
}

#[test]
fn trimmed_contextual_test_decompile_trimmed_window() -> TestResult {
	let glyphs = trimmed_glyphs();
	let mut font = fake_font(&refs(&glyphs));
	let table = decompile(&hex(MORT_TRIMMED_CONTEXTUAL_DATA), &mut font)?;
	assert_trimmed_semantics(&table, &mut font);
	Ok(())
}

#[test]
fn trimmed_contextual_test_binary_roundtrip() -> TestResult {
	let glyphs = trimmed_glyphs();
	let mut font = fake_font(&refs(&glyphs));
	let mut table = decompile(&hex(MORT_TRIMMED_CONTEXTUAL_DATA), &mut font)?;
	let data = table.compile(&mut font)?;
	assert_trimmed_semantics(&decompile(&data, &mut font)?, &mut font);
	Ok(())
}

#[test]
fn trimmed_contextual_test_xml_roundtrip() -> TestResult {
	let glyphs = trimmed_glyphs();
	let mut font = fake_font(&refs(&glyphs));
	let table = decompile(&hex(MORT_TRIMMED_CONTEXTUAL_DATA), &mut font)?;
	let data = xml_round_trip(&table, &mut font)?;
	assert_trimmed_semantics(&decompile(&data, &mut font)?, &mut font);
	Ok(())
}

fn assert_window_semantics(table: &Mort, font: &mut Font) {
	let state_table = state_table(&subtables(table)[0]);
	assert_eq!(
		state_table.per_glyph_lookups,
		[
			lookup(font, &[("hyphen", "emdash")]),
			lookup(font, &[("g1", "emdash")]),
			BTreeMap::new(),
		]
	);
	let transition = &state_table.states[0].transitions[&4];
	assert_eq!(transition.mark_index, 2);
	assert_eq!(transition.current_index, 0xFFFF);
	let transition = &state_table.states[1].transitions[&4];
	assert_eq!(transition.mark_index, 0);
	assert_eq!(transition.current_index, 1);
}

#[test]
fn trimmed_contextual_test_decompile_empty_and_tail_clamped_windows() -> TestResult {
	let glyphs = trimmed_glyphs();
	let mut font = fake_font(&refs(&glyphs));
	let table = decompile(&hex(MORT_TRIMMED_CONTEXTUAL_WINDOWS_DATA), &mut font)?;
	assert_window_semantics(&table, &mut font);
	Ok(())
}

#[test]
fn trimmed_contextual_test_windows_binary_roundtrip() -> TestResult {
	let glyphs = trimmed_glyphs();
	let mut font = fake_font(&refs(&glyphs));
	let mut table = decompile(&hex(MORT_TRIMMED_CONTEXTUAL_WINDOWS_DATA), &mut font)?;
	let data = table.compile(&mut font)?;
	assert_window_semantics(&decompile(&data, &mut font)?, &mut font);
	Ok(())
}

#[test]
fn trimmed_contextual_test_windows_xml_roundtrip() -> TestResult {
	let glyphs = trimmed_glyphs();
	let mut font = fake_font(&refs(&glyphs));
	let table = decompile(&hex(MORT_TRIMMED_CONTEXTUAL_WINDOWS_DATA), &mut font)?;
	let data = xml_round_trip(&table, &mut font)?;
	assert_window_semantics(&decompile(&data, &mut font)?, &mut font);
	Ok(())
}

const REBASE_GLYPHS: &[&str] = &[".notdef", "space", "a", "b", "c", "a_c", "b_c"];

#[test]
fn ligature_round_trip_test_binary_roundtrip_preserves_component_rebasing() -> TestResult {
	let mut font = fake_font(REBASE_GLYPHS);
	let data = hex(MORT_LIGATURE_REBASE_DATA);
	let mut table = decompile(&data, &mut font)?;
	assert_eq!(table.compile(&mut font)?, data);
	Ok(())
}

#[test]
fn ligature_round_trip_test_xml_roundtrip_preserves_component_rebasing() -> TestResult {
	let mut font = fake_font(REBASE_GLYPHS);
	let data = hex(MORT_LIGATURE_REBASE_DATA);
	let table = decompile(&data, &mut font)?;
	let xml = get_xml(&table, &mut font)?;
	let first = xml
		.iter()
		.position(|l| l == "        <MortLigatureRebase>")
		.unwrap();
	let last = first
		+ xml[first..]
			.iter()
			.position(|l| l == "        </MortLigatureRebase>")
			.unwrap()
		+ 1;
	assert_eq!(
		xml[first..last],
		[
			"        <MortLigatureRebase>",
			"          <Component index=\"4\"/>",
			"          <Component index=\"5\"/>",
			"          <Component index=\"6\"/>",
			"          <Component index=\"7\"/>",
			"        </MortLigatureRebase>",
		]
	);
	assert_eq!(xml_round_trip(&table, &mut font)?, data);
	Ok(())
}

const MORT_NONCONTEXTUAL_DATA: &str = concat!(
	"00010000000000010000000100000050000300010004000000000001ffffffff",
	"0004000100000000fffffffe0000000100000000000000000020800400000001",
	"000600040002000800010000000b0087000d0088ffff0000",
);

const MORT_ALL_TYPES_DATA: &str = concat!(
	"00010000000000010000001f0000021000000005004420000000000100060008",
	"002a00300000001e010104050101010101010101010101010101010101010101",
	"010101010101000000000102002a0000002a8000002a20010088200100000002",
	"0005000a002c003200420000001e010101010401010101010101010101010101",
	"010101010101010101010101000000000100002c000000000000002c00000000",
	"0021000000000000000000050000000000000000000000000000000000000000",
	"0000000000000000000000000000000000000000000000000000000000000000",
	"00d42002000000040006000e0030003c0048005000c80000001e010101010101",
	"0405010101010101010101010101010101010101010101010000000001000000",
	"0000000200300000003680000030804800000028800000460000000000000000",
	"0000000000000000000000000000000000000000000000000000000000000000",
	"000000000000000000000000000000000000000000c800c800c800c800c800c8",
	"00c800c800c800c800c800c800c800c800c800c800c800c800c800c800c800c8",
	"00c800c800c800c800c800c800c800c8000f0000001820040000000800060004",
	"000100040000000000080009004c2005000000100005000a002c003200420000",
	"001e010101010101010101010104010101010101010101010101010101010101",
	"000000000100002c0000ffffffff002c00200000ffff000c",
);

const MORT_LIGATURE_REBASE_DATA: &str = concat!(
	"00010000000000010000000100000078000200010004000000000001ffffffff",
	"00000001000000000000000000542002000000010007000e001a002200300038",
	"00480000000701010405060101000000000001010200001a0000001a8000001a",
	"803000000000001a8000001e00000002000000000048004a0048004800050006",
);

const MORT_TRIMMED_CONTEXTUAL_DATA: &str = concat!(
	"0001000000000001000000010000004c0000000100400001000000010005000a",
	"0010001a0032001e000104000000000001000000000200100000000000000015",
	"80000000000000100000fffb0000001f00000000",
);

const MORT_TRIMMED_CONTEXTUAL_WINDOWS_DATA: &str = concat!(
	"0001000000000001000000010000004c0000000100400001000000010005000a",
	"0010001a0032001e000104000000000001000000000200100000000000000015",
	"80000064000000100000fffb0018001f00000000",
);

const MORT_NONCONTEXTUAL_XML: &[&str] = &[
	"<Version value=\"0x00010000\"/>",
	"<!-- MorphChainCount=1 -->",
	"<MorphChain index=\"0\">",
	"  <DefaultFlags value=\"0x00000001\"/>",
	"  <!-- StructLength=80 -->",
	"  <!-- MorphFeatureCount=3 -->",
	"  <!-- MorphSubtableCount=1 -->",
	"  <MorphFeature index=\"0\">",
	"    <FeatureType value=\"4\"/>",
	"    <FeatureSetting value=\"0\"/>",
	"    <EnableFlags value=\"0x00000001\"/>",
	"    <DisableFlags value=\"0xFFFFFFFF\"/>",
	"  </MorphFeature>",
	"  <MorphFeature index=\"1\">",
	"    <FeatureType value=\"4\"/>",
	"    <FeatureSetting value=\"1\"/>",
	"    <EnableFlags value=\"0x00000000\"/>",
	"    <DisableFlags value=\"0xFFFFFFFE\"/>",
	"  </MorphFeature>",
	"  <MorphFeature index=\"2\">",
	"    <FeatureType value=\"0\"/>",
	"    <FeatureSetting value=\"1\"/>",
	"    <EnableFlags value=\"0x00000000\"/>",
	"    <DisableFlags value=\"0x00000000\"/>",
	"  </MorphFeature>",
	"  <MorphSubtable index=\"0\">",
	"    <!-- StructLength=32 -->",
	"    <CoverageFlags value=\"128\"/>",
	"    <!-- MorphType=4 -->",
	"    <SubFeatureFlags value=\"0x00000001\"/>",
	"    <NoncontextualMorph>",
	"      <Substitution>",
	"        <Lookup glyph=\"parenleft\" value=\"parenleft.vertical\"/>",
	"        <Lookup glyph=\"parenright\" value=\"parenright.vertical\"/>",
	"      </Substitution>",
	"    </NoncontextualMorph>",
	"  </MorphSubtable>",
	"</MorphChain>",
];
