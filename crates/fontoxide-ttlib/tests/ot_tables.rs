//! `Tests/ttLib/tables/otTables_test.py`.
//!
//! `test_VarIdxMap_format1_beyond_64k` is in `beyond64k.rs`. The `postRead` and
//! `preWrite` tests call `from_raw` and `to_raw`, which convert between a table
//! and its binary layout. Tables refer to glyphs by ID, so `SparseFakeFont` is a
//! font whose glyph order puts the names at those IDs. The `lazy` parameters are
//! dropped: nothing is read lazily (L4, L17).

#![allow(clippy::unwrap_used)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Once;

use fontoxide_misc::text_tools::{de_hex_str, hex_str};
use fontoxide_misc::xml_reader;
use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_ttlib::beyond64k::{ConvertOptions, upper_tables};
use fontoxide_ttlib::ot::conv::ValueRecord;
use fontoxide_ttlib::ot::morph::{
	AatAction, AatState, ActionIndex, ActionKind, LigAction, compile_actions,
};
use fontoxide_ttlib::ot::overflow::{
	fix_lookup_overflows, split_mark_base_pos, split_multiple_subst, split_single_pos,
};
use fontoxide_ttlib::ot::table::{OtTable, XmlCtx};
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::varc::{VarComponent, flags as varc_flags};
use fontoxide_ttlib::ot::{CountRef, OtReader, OverflowErrorRecord, StateValue, WriterGraph};
use fontoxide_ttlib::tables::{Gpos, Gsub};
use fontoxide_ttlib::{Font, Table, TableType, TableXml, Tag};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn hex(s: &str) -> Vec<u8> {
	de_hex_str(s).unwrap()
}

/// `FakeFont(glyphs)`.
fn fake_font(glyphs: &str) -> Font {
	let mut font = Font::new();
	font.set_glyph_order(glyphs.split_whitespace().map(str::to_owned).collect());
	font
}

/// `SparseFakeFont(glyphMap)`: the other glyphs get `glyphXXXXX` names.
fn sparse_font(map: &[(&str, u32)]) -> Font {
	let max = map.iter().map(|m| m.1).max().unwrap_or(0);
	let mut names: Vec<String> = (0..=max).map(|g| format!("glyph{g:05}")).collect();
	for &(name, gid) in map {
		names[gid as usize] = name.to_owned();
	}
	let mut font = Font::new();
	font.set_glyph_order(names);
	font
}

fn gids(font: &mut Font, names: &[&str]) -> Vec<u32> {
	names.iter().map(|n| font.glyph_id(n).unwrap()).collect()
}

fn gid(font: &mut Font, name: &str) -> u32 {
	font.glyph_id(name).unwrap()
}

fn coverage(font: &mut Font, names: &[&str]) -> Option<Box<ot::Coverage>> {
	Some(Box::new(ot::Coverage {
		glyphs: gids(font, names),
	}))
}

fn glyph_names(font: &mut Font, glyphs: &[u32]) -> Vec<String> {
	let order = font.glyph_order().unwrap();
	glyphs.iter().map(|&g| order.name(g).into_owned()).collect()
}

/// `compileTable`: a lookup subtable gets a `LookupType` to report to.
fn compile_table<T: OtTable>(table: &T, font: &mut Font, lookup: bool) -> TestResult<String> {
	let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
	{
		let mut root = graph.root();
		if lookup {
			root.set(
				"LookupType",
				StateValue::Count(CountRef::new("LookupType", 2, None)),
			);
		}
		table.compile(&mut root, font)?;
	}
	Ok(hex_str(&graph.get_all_data()?))
}

/// `decompileTable`.
fn decompile_table<T: OtTable>(data: &str, font: &mut Font) -> TestResult<T> {
	let data = hex(data);
	Ok(T::decompile(
		&mut OtReader::new(&data, Tag::new(b"GSUB")),
		font,
	)?)
}

/// The lines of what `f` writes, without the XML declaration (`getXML`).
fn get_xml(f: impl FnOnce(&mut XmlWriter) -> fontoxide_ttlib::Result<()>) -> Vec<String> {
	let mut xw = XmlWriter::new();
	f(&mut xw).unwrap();
	xw.as_str().lines().skip(1).map(str::to_owned).collect()
}

/// `parseXML(text)`: the elements.
fn parse_xml(text: &str) -> Vec<xml_reader::Element> {
	xml_reader::parse(&format!("<root>{text}</root>"))
		.unwrap()
		.elements()
		.cloned()
		.collect()
}

fn from_xml<T: OtTable>(table: &mut T, text: &str, font: &mut Font) -> TestResult {
	let mut ctx = XmlCtx::default();
	for e in parse_xml(text) {
		table.from_xml(&e, font, &mut ctx)?;
	}
	table.populate_defaults(&mut ctx)?;
	Ok(())
}

fn is_assertion<T: std::fmt::Debug>(result: fontoxide_ttlib::Result<T>) -> bool {
	matches!(result, Err(fontoxide_ttlib::Error::Assertion(_)))
}

thread_local! {
	static CAPTURED: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

struct Capture;

impl log::Log for Capture {
	fn enabled(&self, _: &log::Metadata<'_>) -> bool {
		true
	}

	fn log(&self, record: &log::Record<'_>) {
		if record.level() <= log::Level::Error {
			CAPTURED.with(|c| {
				if let Some(records) = c.borrow_mut().as_mut() {
					records.push(record.args().to_string());
				}
			});
		}
	}

	fn flush(&self) {}
}

static CAPTURE: Capture = Capture;
static INIT: Once = Once::new();

/// The errors `f` logs on this thread (`caplog.at_level(logging.ERROR)`).
fn capture<R>(f: impl FnOnce() -> R) -> (Vec<String>, R) {
	INIT.call_once(|| {
		let _ = log::set_logger(&CAPTURE);
		log::set_max_level(log::LevelFilter::Warn);
	});
	CAPTURED.with(|c| *c.borrow_mut() = Some(Vec::new()));
	let result = f();
	let records = CAPTURED.with(|c| c.borrow_mut().take()).unwrap_or_default();
	(records, result)
}

// CoverageTest

fn coverage_font() -> Font {
	sparse_font(&[
		("a", 0x10000),
		("b", 0x10001),
		("c", 0x10002),
		("d", 0x10003),
	])
}

#[test]
fn coverage_test_post_read_format3() {
	let mut font = coverage_font();
	let glyphs = gids(&mut font, &["a", "c"]);
	let table = ot::Coverage::from_raw(ot::CoverageRaw::Format3(ot::CoverageRawFormat3 {
		glyph_count: None,
		glyph_array: glyphs.clone(),
	}));
	assert_eq!(table.glyphs, glyphs);
}

#[test]
fn coverage_test_post_read_format4() {
	let mut font = coverage_font();
	let table = ot::Coverage::from_raw(ot::CoverageRaw::Format4(ot::CoverageRawFormat4 {
		range_count: None,
		range_record: vec![ot::RangeRecord2 {
			start: gid(&mut font, "a"),
			end: gid(&mut font, "d"),
			start_coverage_index: 0,
		}],
	}));
	assert_eq!(table.glyphs, gids(&mut font, &["a", "b", "c", "d"]));
}

#[test]
fn coverage_test_decompile_format4() -> TestResult {
	let mut font = coverage_font();
	let table: ot::Coverage = decompile_table("0004000001010000010003000000", &mut font)?;
	assert_eq!(table.glyphs, gids(&mut font, &["a", "b", "c", "d"]));
	Ok(())
}

#[test]
fn coverage_test_pre_write_format3() -> TestResult {
	let mut font = coverage_font();
	let table = ot::Coverage {
		glyphs: gids(&mut font, &["a", "c"]),
	};
	let ot::CoverageRaw::Format3(raw) = table.to_raw(&mut font)? else {
		panic!("not format 3");
	};
	assert_eq!(raw.glyph_array, gids(&mut font, &["a", "c"]));
	assert_eq!(
		compile_table(&table, &mut font, false)?,
		"0003000002010000010002"
	);
	Ok(())
}

#[test]
fn coverage_test_pre_write_format4() -> TestResult {
	let mut font = coverage_font();
	let table = ot::Coverage {
		glyphs: gids(&mut font, &["a", "b", "c", "d"]),
	};
	let raw = table.to_raw(&mut font)?;
	assert!(matches!(raw, ot::CoverageRaw::Format4(_)));
	assert_eq!(
		compile_table(&table, &mut font, false)?,
		"0004000001010000010003000000"
	);
	Ok(())
}

// ClassDefTest

fn class_def_font() -> Font {
	sparse_font(&[
		("a", 0x10000),
		("b", 0x10001),
		("c", 0x20000),
		("d", 0x20001),
	])
}

fn class_defs(font: &mut Font, items: &[(&str, u32)]) -> BTreeMap<u32, u32> {
	items.iter().map(|&(g, c)| (gid(font, g), c)).collect()
}

#[test]
fn class_def_test_post_read_format3() {
	let mut font = class_def_font();
	let table = ot::ClassDef::from_raw(ot::ClassDefRaw::Format3(ot::ClassDefRawFormat3 {
		start_glyph: gid(&mut font, "a"),
		glyph_count: None,
		class_value_array: vec![1, 2],
	}));
	assert_eq!(
		table.class_defs,
		class_defs(&mut font, &[("a", 1), ("b", 2)])
	);
}

#[test]
fn class_def_test_post_read_format4() {
	let mut font = class_def_font();
	let table = ot::ClassDef::from_raw(ot::ClassDefRaw::Format4(ot::ClassDefRawFormat4 {
		class_range_count: None,
		class_range_record: vec![ot::ClassRangeRecord2 {
			start: gid(&mut font, "c"),
			end: gid(&mut font, "d"),
			class: 3,
		}],
	}));
	assert_eq!(
		table.class_defs,
		class_defs(&mut font, &[("c", 3), ("d", 3)])
	);
}

#[test]
fn class_def_test_decompile_format4() -> TestResult {
	let mut font = class_def_font();
	let table: ot::ClassDef =
		decompile_table("000400000201000001000000010200000200000002", &mut font)?;
	assert_eq!(
		table.class_defs,
		class_defs(&mut font, &[("a", 1), ("c", 2)])
	);
	Ok(())
}

#[test]
fn class_def_test_pre_write_format3() -> TestResult {
	let mut font = class_def_font();
	let table = ot::ClassDef {
		class_defs: class_defs(&mut font, &[("a", 1), ("b", 2)]),
	};
	let ot::ClassDefRaw::Format3(raw) = table.to_raw()? else {
		panic!("not format 3");
	};
	assert_eq!(raw.class_value_array, [1, 2]);
	assert_eq!(
		compile_table(&table, &mut font, false)?,
		"0003010000000002000001000002"
	);
	Ok(())
}

#[test]
fn class_def_test_pre_write_format3_large_classes() -> TestResult {
	let mut font = fake_font("a b");
	let table = ot::ClassDef {
		class_defs: class_defs(&mut font, &[("a", 0x10000), ("b", 0xFFFFFF)]),
	};
	let ot::ClassDefRaw::Format3(raw) = table.to_raw()? else {
		panic!("not format 3");
	};
	assert_eq!(raw.class_value_array, [0x10000, 0xFFFFFF]);
	assert_eq!(
		compile_table(&table, &mut font, false)?,
		"0003000000000002010000ffffff"
	);
	Ok(())
}

#[test]
fn class_def_test_pre_write_format4() -> TestResult {
	let mut font = class_def_font();
	let table = ot::ClassDef {
		class_defs: class_defs(&mut font, &[("a", 1), ("c", 2)]),
	};
	assert!(matches!(table.to_raw()?, ot::ClassDefRaw::Format4(_)));
	assert_eq!(
		compile_table(&table, &mut font, false)?,
		"000400000201000001000000010200000200000002"
	);
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_VarIdxMap_format0_roundtrip() -> TestResult {
	let mut font = fake_font(".notdef a b c");
	let table = ot::VarIdxMap {
		mapping: BTreeMap::from([(0, 0), (1, 1), (2, 0), (3, 0)]),
	};
	let data = compile_table(&table, &mut font, false)?;
	// Format 0 (uint8).
	assert_eq!(&data[..2], "00");
	let table2: ot::VarIdxMap = decompile_table(&data, &mut font)?;
	assert_eq!(table2.mapping, table.mapping);
	// There is no `Format` attribute to keep: it is derived when compiling.
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_FeatureVariations_version_1_1_roundtrip() -> TestResult {
	let condition = ot::ConditionTable::Format1(ot::ConditionTableFormat1 {
		axis_index: 1,
		filter_range_min_value: -0.5,
		filter_range_max_value: 0.75,
	});
	let lookup_condition = ot::LookupConditionRecord {
		condition_table: Some(Box::new(condition)),
		lookup_index_list: Some(Box::new(ot::LookupIndexList {
			lookup_index_count: Some(2),
			lookup_index: vec![3, 5],
		})),
	};
	let feature_lookups = ot::FeatureLookupsTable {
		version: 0x00010000,
		flags: 1,
		lookup_condition_count: Some(1),
		lookup_condition_record: vec![lookup_condition],
	};
	let table = ot::FeatureVariations {
		version: 0x00010001,
		feature_variation_count: Some(0),
		feature_variation_record: Vec::new(),
		lookup_variation_count: Some(1),
		lookup_variation_record: vec![ot::LookupVariationRecord {
			feature_index: 2,
			feature_lookups_table: Some(Box::new(feature_lookups)),
		}],
	};
	let mut font = fake_font("");
	let data = compile_table(&table, &mut font, false)?;
	assert_eq!(
		data,
		"000100010000000000000001000200000012\
		 00010000000100000001000000120000001a\
		 00010001e0003000000200030005"
	);

	let rt: ot::FeatureVariations = decompile_table(&data, &mut font)?;
	assert_eq!(rt.version, 0x00010001);
	assert_eq!(rt.feature_variation_count, Some(0));
	assert_eq!(rt.lookup_variation_count, Some(1));
	let record = &rt.lookup_variation_record[0];
	assert_eq!(record.feature_index, 2);
	let lookups = record.feature_lookups_table.as_deref().ok_or("no table")?;
	assert_eq!(lookups.flags, 1);
	let lookup_condition = &lookups.lookup_condition_record[0];
	let Some(ot::ConditionTable::Format1(condition)) = lookup_condition.condition_table.as_deref()
	else {
		panic!("not a format 1 condition");
	};
	assert_eq!(condition.axis_index, 1);
	assert_eq!(condition.filter_range_min_value, -0.5);
	assert_eq!(condition.filter_range_max_value, 0.75);
	assert_eq!(
		lookup_condition
			.lookup_index_list
			.as_deref()
			.ok_or("no list")?
			.lookup_index,
		[3, 5]
	);
	Ok(())
}

#[test]
fn test_var_component_xml_scale_y() -> TestResult {
	for scale_x in [-0.5, 0.0, 0.5, 1.0, 2.0] {
		for scale_y in [None, Some(0.25), Some(1.0)] {
			let mut font = fake_font(".notdef a");
			let mut xml =
				format!(r#"<VarComponent><glyphName value="a"/><scaleX value="{scale_x}"/>"#);
			if let Some(y) = scale_y {
				xml += &format!(r#"<scaleY value="{y}"/>"#);
			}
			xml += "</VarComponent>";
			let component = VarComponent::from_xml(&xml_reader::parse(&xml)?, &mut font)?;
			assert_eq!(component.transform.scale_x, scale_x);
			assert_eq!(component.transform.scale_y, scale_y.unwrap_or(scale_x));
			assert_eq!(
				component.flags & varc_flags::HAVE_SCALE_Y != 0,
				scale_y.is_some()
			);

			let mut data = Vec::new();
			component.compile(&mut data)?;
			let (roundtripped, end) = VarComponent::decompile(&data, 0, 2, None)?;
			assert_eq!(end, data.len());
			assert_eq!(roundtripped.transform, component.transform);
		}
	}
	Ok(())
}

// SingleSubstTest

const SINGLE_GLYPHS: &str = ".notdef A B C D E a b c d e";

fn glyph_map(font: &mut Font, pairs: &[(&str, &str)]) -> BTreeMap<u32, u32> {
	pairs
		.iter()
		.map(|&(a, b)| (gid(font, a), gid(font, b)))
		.collect()
}

fn writer_root(graph: &mut WriterGraph) -> fontoxide_ttlib::ot::OtWriter<'_> {
	graph.root()
}

#[test]
fn single_subst_test_post_read_format1() -> TestResult {
	let mut font = fake_font(SINGLE_GLYPHS);
	let table =
		ot::SingleSubst::from_raw(ot::SingleSubstRaw::Format1(ot::SingleSubstRawFormat1 {
			coverage: coverage(&mut font, &["A", "B", "C"]),
			delta_glyph_id: 5,
		}))?;
	assert_eq!(
		table.mapping,
		glyph_map(&mut font, &[("A", "a"), ("B", "b"), ("C", "c")])
	);
	Ok(())
}

#[test]
fn single_subst_test_post_read_format2() -> TestResult {
	let mut font = fake_font(SINGLE_GLYPHS);
	let table =
		ot::SingleSubst::from_raw(ot::SingleSubstRaw::Format2(ot::SingleSubstRawFormat2 {
			coverage: coverage(&mut font, &["A", "B", "C"]),
			glyph_count: Some(3),
			substitute: gids(&mut font, &["c", "b", "a"]),
		}))?;
	assert_eq!(
		table.mapping,
		glyph_map(&mut font, &[("A", "c"), ("B", "b"), ("C", "a")])
	);
	Ok(())
}

#[test]
fn single_subst_test_post_read_format3() -> TestResult {
	let mut font = sparse_font(&[("a", 0x10000), ("b", 0x10001), ("c", 0x10002)]);
	let table =
		ot::SingleSubst::from_raw(ot::SingleSubstRaw::Format3(ot::SingleSubstRawFormat3 {
			coverage: coverage(&mut font, &["a", "b"]),
			delta_glyph_id: 1,
		}))?;
	assert_eq!(
		table.mapping,
		glyph_map(&mut font, &[("a", "b"), ("b", "c")])
	);
	Ok(())
}

#[test]
fn single_subst_test_post_read_format4() -> TestResult {
	let mut font = sparse_font(&[("a", 0x10000), ("b", 0x10001), ("c", 0x20000)]);
	let table =
		ot::SingleSubst::from_raw(ot::SingleSubstRaw::Format4(ot::SingleSubstRawFormat4 {
			coverage: coverage(&mut font, &["a", "b"]),
			glyph_count: Some(2),
			substitute: gids(&mut font, &["c", "a"]),
		}))?;
	assert_eq!(
		table.mapping,
		glyph_map(&mut font, &[("a", "c"), ("b", "a")])
	);
	Ok(())
}

#[test]
fn single_subst_test_post_read_format_unknown() {
	assert!(is_assertion(ot::SingleSubst::from_raw(
		ot::SingleSubstRaw::Unknown(987)
	)));
}

#[test]
fn single_subst_test_pre_write_format1() -> TestResult {
	let mut font = fake_font(SINGLE_GLYPHS);
	let table = ot::SingleSubst {
		mapping: glyph_map(&mut font, &[("A", "a"), ("B", "b"), ("C", "c")]),
		..Default::default()
	};
	let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
	let ot::SingleSubstRaw::Format1(raw) = table.to_raw(&mut writer_root(&mut graph), &mut font)?
	else {
		panic!("not format 1");
	};
	assert_eq!(
		raw.coverage.ok_or("no coverage")?.glyphs,
		gids(&mut font, &["A", "B", "C"])
	);
	assert_eq!(raw.delta_glyph_id, 5);
	Ok(())
}

#[test]
fn single_subst_test_pre_write_format2() -> TestResult {
	let mut font = fake_font(SINGLE_GLYPHS);
	let table = ot::SingleSubst {
		mapping: glyph_map(&mut font, &[("A", "c"), ("B", "b"), ("C", "a")]),
		..Default::default()
	};
	let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
	let ot::SingleSubstRaw::Format2(raw) = table.to_raw(&mut writer_root(&mut graph), &mut font)?
	else {
		panic!("not format 2");
	};
	assert_eq!(
		raw.coverage.ok_or("no coverage")?.glyphs,
		gids(&mut font, &["A", "B", "C"])
	);
	assert_eq!(raw.substitute, gids(&mut font, &["c", "b", "a"]));
	Ok(())
}

#[test]
fn single_subst_test_pre_write_format3() -> TestResult {
	let mut font = sparse_font(&[("a", 0x10000), ("b", 0x10001), ("c", 0x10002)]);
	let table = ot::SingleSubst {
		mapping: glyph_map(&mut font, &[("a", "b"), ("b", "c")]),
		..Default::default()
	};
	let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
	let ot::SingleSubstRaw::Format3(raw) = table.to_raw(&mut writer_root(&mut graph), &mut font)?
	else {
		panic!("not format 3");
	};
	assert_eq!(raw.delta_glyph_id, 1);
	assert_eq!(
		compile_table(&table, &mut font, true)?,
		"0003000000090000010003000002010000010001"
	);
	Ok(())
}

#[test]
fn single_subst_test_pre_write_format4() -> TestResult {
	let mut font = sparse_font(&[("a", 0x10000), ("b", 0x10001), ("c", 0x20000)]);
	let table = ot::SingleSubst {
		mapping: glyph_map(&mut font, &[("a", "c"), ("b", "a")]),
		..Default::default()
	};
	let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
	let ot::SingleSubstRaw::Format4(raw) = table.to_raw(&mut writer_root(&mut graph), &mut font)?
	else {
		panic!("not format 4");
	};
	assert_eq!(raw.substitute, gids(&mut font, &["c", "a"]));
	assert_eq!(
		compile_table(&table, &mut font, true)?,
		"00040000000f0000020200000100000003000002010000010001"
	);
	Ok(())
}

#[test]
fn single_subst_test_pre_write_empty_mapping() -> TestResult {
	let mut font = fake_font(SINGLE_GLYPHS);
	let table = ot::SingleSubst::default();
	let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
	let ot::SingleSubstRaw::Format2(raw) = table.to_raw(&mut writer_root(&mut graph), &mut font)?
	else {
		panic!("not format 2");
	};
	assert!(raw.coverage.ok_or("no coverage")?.glyphs.is_empty());
	assert!(raw.substitute.is_empty());
	Ok(())
}

#[test]
fn single_subst_test_to_xml2() -> TestResult {
	let mut font = fake_font(SINGLE_GLYPHS);
	let table = ot::SingleSubst {
		mapping: glyph_map(&mut font, &[("A", "a"), ("B", "b"), ("C", "c")]),
		..Default::default()
	};
	assert_eq!(
		get_xml(|xw| table.to_xml2(xw, &mut font)),
		[
			r#"<Substitution in="A" out="a"/>"#,
			r#"<Substitution in="B" out="b"/>"#,
			r#"<Substitution in="C" out="c"/>"#,
		]
	);
	Ok(())
}

#[test]
fn single_subst_test_from_xml() -> TestResult {
	let mut font = fake_font(SINGLE_GLYPHS);
	let mut table = ot::SingleSubst::default();
	from_xml(
		&mut table,
		r#"<Substitution in="A" out="a"/><Substitution in="B" out="b"/><Substitution in="C" out="c"/>"#,
		&mut font,
	)?;
	assert_eq!(
		table.mapping,
		glyph_map(&mut font, &[("A", "a"), ("B", "b"), ("C", "c")])
	);
	Ok(())
}

// MultipleSubstTest

const MULTIPLE_GLYPHS: &str = ".notdef c f i t c_t f_f_i";

fn multiple_map(font: &mut Font, items: &[(&str, &[&str])]) -> BTreeMap<u32, Vec<u32>> {
	items
		.iter()
		.map(|&(g, s)| (gid(font, g), gids(font, s)))
		.collect()
}

#[test]
fn multiple_subst_test_post_read_format1() -> TestResult {
	let mut font = fake_font(MULTIPLE_GLYPHS);
	let sequence = |font: &mut Font, s: &[&str]| {
		Some(Box::new(ot::Sequence {
			glyph_count: None,
			substitute: gids(font, s),
		}))
	};
	let table =
		ot::MultipleSubst::from_raw(ot::MultipleSubstRaw::Format1(ot::MultipleSubstRawFormat1 {
			coverage: coverage(&mut font, &["c_t", "f_f_i"]),
			sequence_count: None,
			sequence: vec![
				sequence(&mut font, &["c", "t"]),
				sequence(&mut font, &["f", "f", "i"]),
			],
		}))?;
	assert_eq!(
		table.mapping,
		multiple_map(
			&mut font,
			&[("c_t", &["c", "t"]), ("f_f_i", &["f", "f", "i"])]
		)
	);
	Ok(())
}

#[test]
fn multiple_subst_test_post_read_format2() -> TestResult {
	let mut font = sparse_font(&[("a", 0x10000), ("b", 0x10001), ("c", 0x20000)]);
	let table =
		ot::MultipleSubst::from_raw(ot::MultipleSubstRaw::Format2(ot::MultipleSubstRawFormat2 {
			coverage: coverage(&mut font, &["a"]),
			sequence_count: None,
			sequence: vec![Some(Box::new(ot::Sequence2 {
				glyph_count: None,
				substitute: gids(&mut font, &["b", "c"]),
			}))],
		}))?;
	assert_eq!(
		table.mapping,
		multiple_map(&mut font, &[("a", &["b", "c"])])
	);
	Ok(())
}

#[test]
fn multiple_subst_test_decompile_format2() -> TestResult {
	let mut font = sparse_font(&[("a", 0x10000), ("b", 0x10001), ("c", 0x20000)]);
	let table: ot::MultipleSubst = decompile_table(
		"00020000000c00000100001400030000010100000002010001020000",
		&mut font,
	)?;
	assert_eq!(
		table.mapping,
		multiple_map(&mut font, &[("a", &["b", "c"])])
	);
	Ok(())
}

#[test]
fn multiple_subst_test_post_read_format_unknown() {
	assert!(is_assertion(ot::MultipleSubst::from_raw(
		ot::MultipleSubstRaw::Unknown(987)
	)));
}

#[test]
fn multiple_subst_test_pre_write_format1() -> TestResult {
	let mut font = fake_font(MULTIPLE_GLYPHS);
	let table = ot::MultipleSubst {
		mapping: multiple_map(
			&mut font,
			&[("c_t", &["c", "t"]), ("f_f_i", &["f", "f", "i"])],
		),
		..Default::default()
	};
	let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
	let ot::MultipleSubstRaw::Format1(raw) =
		table.to_raw(&mut writer_root(&mut graph), &mut font)?
	else {
		panic!("not format 1");
	};
	assert_eq!(
		raw.coverage.ok_or("no coverage")?.glyphs,
		gids(&mut font, &["c_t", "f_f_i"])
	);
	Ok(())
}

#[test]
fn multiple_subst_test_pre_write_format2() -> TestResult {
	let mut font = sparse_font(&[("a", 0x10000), ("b", 0x10001), ("c", 0x20000)]);
	let table = ot::MultipleSubst {
		mapping: multiple_map(&mut font, &[("a", &["b", "c"])]),
		..Default::default()
	};
	let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
	let ot::MultipleSubstRaw::Format2(raw) =
		table.to_raw(&mut writer_root(&mut graph), &mut font)?
	else {
		panic!("not format 2");
	};
	let sequence: &ot::Sequence2 = raw.sequence[0].as_deref().ok_or("no sequence")?;
	assert_eq!(sequence.substitute, gids(&mut font, &["b", "c"]));
	assert_eq!(
		compile_table(&table, &mut font, true)?,
		"00020000000c00000100001400030000010100000002010001020000"
	);
	Ok(())
}

#[test]
fn multiple_subst_test_to_xml2() -> TestResult {
	let mut font = fake_font(MULTIPLE_GLYPHS);
	let table = ot::MultipleSubst {
		mapping: multiple_map(
			&mut font,
			&[("c_t", &["c", "t"]), ("f_f_i", &["f", "f", "i"])],
		),
		..Default::default()
	};
	assert_eq!(
		get_xml(|xw| table.to_xml2(xw, &mut font)),
		[
			r#"<Substitution in="c_t" out="c,t"/>"#,
			r#"<Substitution in="f_f_i" out="f,f,i"/>"#,
		]
	);
	Ok(())
}

#[test]
fn multiple_subst_test_from_xml() -> TestResult {
	let mut font = fake_font(MULTIPLE_GLYPHS);
	let mut table = ot::MultipleSubst::default();
	from_xml(
		&mut table,
		r#"<Substitution in="c_t" out="c,t"/><Substitution in="f_f_i" out="f,f,i"/>"#,
		&mut font,
	)?;
	assert_eq!(
		table.mapping,
		multiple_map(
			&mut font,
			&[("c_t", &["c", "t"]), ("f_f_i", &["f", "f", "i"])]
		)
	);
	Ok(())
}

#[test]
fn multiple_subst_test_from_xml_old_format() -> TestResult {
	let mut font = fake_font(MULTIPLE_GLYPHS);
	let mut table = ot::MultipleSubst::default();
	from_xml(
		&mut table,
		r#"<Coverage>
		  <Glyph value="c_t"/>
		  <Glyph value="f_f_i"/>
		</Coverage>
		<Sequence index="0">
		  <Substitute index="0" value="c"/>
		  <Substitute index="1" value="t"/>
		</Sequence>
		<Sequence index="1">
		  <Substitute index="0" value="f"/>
		  <Substitute index="1" value="f"/>
		  <Substitute index="2" value="i"/>
		</Sequence>"#,
		&mut font,
	)?;
	assert_eq!(
		table.mapping,
		multiple_map(
			&mut font,
			&[("c_t", &["c", "t"]), ("f_f_i", &["f", "f", "i"])]
		)
	);
	Ok(())
}

#[test]
fn multiple_subst_test_from_xml_old_format_bug385() -> TestResult {
	// https://github.com/fonttools/fonttools/issues/385
	// Glyph names are resolved when reading TTX (L3), so the font has "o" and "l".
	let mut font = fake_font(".notdef c f i t c_t f_f_i o l");
	let mut table = ot::MultipleSubst::default();
	from_xml(
		&mut table,
		r#"<Coverage>
		  <Glyph value="o"/>
		  <Glyph value="l"/>
		</Coverage>
		<Sequence>
		  <Substitute value="o"/>
		  <Substitute value="l"/>
		  <Substitute value="o"/>
		</Sequence>
		<Sequence>
		  <Substitute value="o"/>
		</Sequence>"#,
		&mut font,
	)?;
	assert_eq!(
		table.mapping,
		multiple_map(&mut font, &[("o", &["o", "l", "o"]), ("l", &["o"])])
	);
	Ok(())
}

// LigatureSubstTest

const LIGATURE_GLYPHS: &str = ".notdef c f i t c_t f_f f_i f_f_i";

/// `makeLigature`: "ffi" is the ligature `f_f_i` of `f`, `f`, `i`.
fn make_ligature(font: &mut Font, s: &str) -> ot::Ligature {
	let component: Vec<String> = s.chars().map(String::from).collect();
	let names: Vec<&str> = component.iter().map(String::as_str).collect();
	ot::Ligature {
		lig_glyph: gid(font, &component.join("_")),
		comp_count: None,
		component: gids(font, &names),
	}
}

fn make_ligatures(font: &mut Font, s: &str) -> Vec<ot::Ligature> {
	s.split_whitespace()
		.map(|l| make_ligature(font, l))
		.collect()
}

/// The ligatures as (glyph, [(ligature glyph, components)]) names.
type NamedLigatures = Vec<(String, Vec<(String, Vec<String>)>)>;

fn named_ligatures(font: &mut Font, table: &ot::LigatureSubst) -> NamedLigatures {
	table
		.ligatures
		.iter()
		.map(|(&g, ligs)| {
			(
				glyph_names(font, &[g]).remove(0),
				ligs.iter()
					.map(|l| {
						(
							glyph_names(font, &[l.lig_glyph]).remove(0),
							glyph_names(font, &l.component),
						)
					})
					.collect(),
			)
		})
		.collect()
}

/// (glyph, [(ligature glyph, components)]).
type Ligs<'a> = [(&'a str, &'a [(&'a str, &'a [&'a str])])];

fn ligs(items: &Ligs<'_>) -> NamedLigatures {
	items
		.iter()
		.map(|&(g, l)| {
			(
				g.to_owned(),
				l.iter()
					.map(|&(lig, comps)| {
						(
							lig.to_owned(),
							comps.iter().map(|c| (*c).to_owned()).collect(),
						)
					})
					.collect(),
			)
		})
		.collect()
}

#[test]
fn ligature_subst_test_post_read_format1() -> TestResult {
	let mut font = fake_font(LIGATURE_GLYPHS);
	let set = |font: &mut Font, s: &str| {
		Some(Box::new(ot::LigatureSet {
			ligature_count: None,
			ligature: make_ligatures(font, s)
				.into_iter()
				.map(|l| Some(Box::new(l)))
				.collect(),
		}))
	};
	let table =
		ot::LigatureSubst::from_raw(ot::LigatureSubstRaw::Format1(ot::LigatureSubstRawFormat1 {
			coverage: coverage(&mut font, &["c", "f"]),
			lig_set_count: None,
			ligature_set: vec![set(&mut font, "ct"), set(&mut font, "ffi ff fi")],
		}))?;
	assert_eq!(
		named_ligatures(&mut font, &table),
		ligs(&[
			("c", &[("c_t", &["c", "t"])]),
			(
				"f",
				&[
					("f_f_i", &["f", "f", "i"]),
					("f_f", &["f", "f"]),
					("f_i", &["f", "i"]),
				]
			),
		])
	);
	Ok(())
}

fn ligature2_raw(font: &mut Font) -> ot::LigatureSubstRaw {
	ot::LigatureSubstRaw::Format2(ot::LigatureSubstRawFormat2 {
		coverage: coverage(font, &["a"]),
		lig_set_count: None,
		ligature_set: vec![Some(Box::new(ot::LigatureSet2 {
			ligature_count: None,
			ligature: vec![Some(Box::new(ot::Ligature2 {
				lig_glyph: gid(font, "c"),
				comp_count: None,
				component: gids(font, &["b"]),
			}))],
		}))],
	})
}

#[test]
fn ligature_subst_test_post_read_format2() -> TestResult {
	let mut font = sparse_font(&[("a", 0x10000), ("b", 0x10001), ("c", 0x20000)]);
	let raw = ligature2_raw(&mut font);
	let table = ot::LigatureSubst::from_raw(raw)?;
	assert_eq!(
		named_ligatures(&mut font, &table),
		ligs(&[("a", &[("c", &["b"])])])
	);
	Ok(())
}

#[test]
fn ligature_subst_test_decompile_format2() -> TestResult {
	let mut font = sparse_font(&[("a", 0x10000), ("b", 0x10001), ("c", 0x20000)]);
	let table: ot::LigatureSubst = decompile_table(
		"00020000001900000100000c000100000502000000020100010003000001010000",
		&mut font,
	)?;
	assert_eq!(
		named_ligatures(&mut font, &table),
		ligs(&[("a", &[("c", &["b"])])])
	);
	Ok(())
}

#[test]
fn ligature_subst_test_post_read_format_unknown() {
	assert!(is_assertion(ot::LigatureSubst::from_raw(
		ot::LigatureSubstRaw::Unknown(987)
	)));
}

#[test]
fn ligature_subst_test_pre_write_format1() -> TestResult {
	let mut font = fake_font(LIGATURE_GLYPHS);
	let table = ot::LigatureSubst {
		ligatures: BTreeMap::from([
			(gid(&mut font, "c"), make_ligatures(&mut font, "ct")),
			(gid(&mut font, "f"), make_ligatures(&mut font, "ffi ff fi")),
		]),
		..Default::default()
	};
	let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
	let ot::LigatureSubstRaw::Format1(raw) =
		table.to_raw(&mut writer_root(&mut graph), &mut font)?
	else {
		panic!("not format 1");
	};
	assert_eq!(
		raw.coverage.as_deref().ok_or("no coverage")?.glyphs,
		gids(&mut font, &["c", "f"])
	);
	let sets: Vec<Vec<ot::Ligature>> = raw
		.ligature_set
		.iter()
		.map(|s| {
			s.as_deref()
				.unwrap()
				.ligature
				.iter()
				.map(|l| (**l.as_ref().unwrap()).clone())
				.collect()
		})
		.collect();
	assert_eq!(
		sets,
		[
			make_ligatures(&mut font, "ct"),
			make_ligatures(&mut font, "ffi ff fi")
		]
	);
	Ok(())
}

#[test]
fn ligature_subst_test_pre_write_format2() -> TestResult {
	let mut font = sparse_font(&[("a", 0x10000), ("b", 0x10001), ("c", 0x20000)]);
	let table = ot::LigatureSubst {
		ligatures: BTreeMap::from([(
			gid(&mut font, "a"),
			vec![ot::Ligature {
				lig_glyph: gid(&mut font, "c"),
				comp_count: None,
				component: gids(&mut font, &["b"]),
			}],
		)]),
		..Default::default()
	};
	let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
	let raw = table.to_raw(&mut writer_root(&mut graph), &mut font)?;
	assert!(matches!(raw, ot::LigatureSubstRaw::Format2(_)));
	assert_eq!(
		compile_table(&table, &mut font, true)?,
		"00020000001900000100000c000100000502000000020100010003000001010000"
	);
	Ok(())
}

// `test_preWrite_format2_highLevel` sets `ligatures` to a {components: glyph}
// dict, the input of `buildLigatureSubstSubtable`; here `ligatures` has one
// type, so the case is the same as `test_preWrite_format2`.

#[test]
fn ligature_subst_test_to_xml2() -> TestResult {
	let mut font = fake_font(LIGATURE_GLYPHS);
	let table = ot::LigatureSubst {
		ligatures: BTreeMap::from([
			(gid(&mut font, "c"), make_ligatures(&mut font, "ct")),
			(gid(&mut font, "f"), make_ligatures(&mut font, "ffi ff fi")),
		]),
		..Default::default()
	};
	assert_eq!(
		get_xml(|xw| table.to_xml2(xw, &mut font)),
		[
			r#"<LigatureSet glyph="c">"#,
			r#"  <Ligature components="c,t" glyph="c_t"/>"#,
			"</LigatureSet>",
			r#"<LigatureSet glyph="f">"#,
			r#"  <Ligature components="f,f,i" glyph="f_f_i"/>"#,
			r#"  <Ligature components="f,f" glyph="f_f"/>"#,
			r#"  <Ligature components="f,i" glyph="f_i"/>"#,
			"</LigatureSet>",
		]
	);
	Ok(())
}

#[test]
fn ligature_subst_test_from_xml() -> TestResult {
	let mut font = fake_font(LIGATURE_GLYPHS);
	let mut table = ot::LigatureSubst::default();
	from_xml(
		&mut table,
		r#"<LigatureSet glyph="f">
		  <Ligature components="f,f,i" glyph="f_f_i"/>
		  <Ligature components="f,f" glyph="f_f"/>
		</LigatureSet>"#,
		&mut font,
	)?;
	assert_eq!(
		named_ligatures(&mut font, &table),
		ligs(&[("f", &[("f_f_i", &["f", "f", "i"]), ("f_f", &["f", "f"])])])
	);
	Ok(())
}

// AlternateSubstTest

const ALTERNATE_GLYPHS: &str = ".notdef G G.alt1 G.alt2 Z Z.fina";

#[test]
fn alternate_subst_test_post_read_format1() -> TestResult {
	let mut font = fake_font(ALTERNATE_GLYPHS);
	let set = |font: &mut Font, s: &str| {
		let names: Vec<&str> = s.split_whitespace().collect();
		Some(Box::new(ot::AlternateSet {
			glyph_count: None,
			alternate: gids(font, &names),
		}))
	};
	let table = ot::AlternateSubst::from_raw(ot::AlternateSubstRaw::Format1(
		ot::AlternateSubstRawFormat1 {
			coverage: coverage(&mut font, &["G", "Z"]),
			alternate_set_count: None,
			alternate_set: vec![set(&mut font, "G.alt2 G.alt1"), set(&mut font, "Z.fina")],
		},
	))?;
	assert_eq!(
		table.alternates,
		multiple_map(
			&mut font,
			&[("G", &["G.alt2", "G.alt1"]), ("Z", &["Z.fina"])]
		)
	);
	Ok(())
}

#[test]
fn alternate_subst_test_post_read_format2() -> TestResult {
	let mut font = sparse_font(&[("a", 0x10000), ("b", 0x10001), ("c", 0x20000)]);
	let table = ot::AlternateSubst::from_raw(ot::AlternateSubstRaw::Format2(
		ot::AlternateSubstRawFormat2 {
			coverage: coverage(&mut font, &["a"]),
			alternate_set_count: None,
			alternate_set: vec![Some(Box::new(ot::AlternateSet2 {
				glyph_count: None,
				alternate: gids(&mut font, &["b", "c"]),
			}))],
		},
	))?;
	assert_eq!(
		table.alternates,
		multiple_map(&mut font, &[("a", &["b", "c"])])
	);
	Ok(())
}

#[test]
fn alternate_subst_test_decompile_format2() -> TestResult {
	let mut font = sparse_font(&[("a", 0x10000), ("b", 0x10001), ("c", 0x20000)]);
	let table: ot::AlternateSubst = decompile_table(
		"00020000001400000100000c00020100010200000003000001010000",
		&mut font,
	)?;
	assert_eq!(
		table.alternates,
		multiple_map(&mut font, &[("a", &["b", "c"])])
	);
	Ok(())
}

#[test]
fn alternate_subst_test_post_read_format_unknown() {
	assert!(is_assertion(ot::AlternateSubst::from_raw(
		ot::AlternateSubstRaw::Unknown(987)
	)));
}

#[test]
fn alternate_subst_test_pre_write_format1() -> TestResult {
	let mut font = fake_font(ALTERNATE_GLYPHS);
	let table = ot::AlternateSubst {
		alternates: multiple_map(
			&mut font,
			&[("G", &["G.alt2", "G.alt1"]), ("Z", &["Z.fina"])],
		),
		..Default::default()
	};
	let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
	let ot::AlternateSubstRaw::Format1(raw) =
		table.to_raw(&mut writer_root(&mut graph), &mut font)?
	else {
		panic!("not format 1");
	};
	assert_eq!(
		raw.coverage.as_deref().ok_or("no coverage")?.glyphs,
		gids(&mut font, &["G", "Z"])
	);
	let sets: Vec<Vec<u32>> = raw
		.alternate_set
		.iter()
		.map(|s| s.as_deref().unwrap().alternate.clone())
		.collect();
	assert_eq!(
		sets,
		[
			gids(&mut font, &["G.alt2", "G.alt1"]),
			gids(&mut font, &["Z.fina"])
		]
	);
	Ok(())
}

#[test]
fn alternate_subst_test_pre_write_format2() -> TestResult {
	let mut font = sparse_font(&[("a", 0x10000), ("b", 0x10001), ("c", 0x20000)]);
	let table = ot::AlternateSubst {
		alternates: multiple_map(&mut font, &[("a", &["b", "c"])]),
		..Default::default()
	};
	let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
	let ot::AlternateSubstRaw::Format2(raw) =
		table.to_raw(&mut writer_root(&mut graph), &mut font)?
	else {
		panic!("not format 2");
	};
	assert_eq!(
		raw.alternate_set[0].as_deref().ok_or("no set")?.alternate,
		gids(&mut font, &["b", "c"])
	);
	assert_eq!(
		compile_table(&table, &mut font, true)?,
		"00020000001400000100000c00020100010200000003000001010000"
	);
	Ok(())
}

#[test]
fn alternate_subst_test_to_xml2() -> TestResult {
	let mut font = fake_font(ALTERNATE_GLYPHS);
	let table = ot::AlternateSubst {
		alternates: multiple_map(
			&mut font,
			&[("G", &["G.alt2", "G.alt1"]), ("Z", &["Z.fina"])],
		),
		..Default::default()
	};
	assert_eq!(
		get_xml(|xw| table.to_xml2(xw, &mut font)),
		[
			r#"<AlternateSet glyph="G">"#,
			r#"  <Alternate glyph="G.alt2"/>"#,
			r#"  <Alternate glyph="G.alt1"/>"#,
			"</AlternateSet>",
			r#"<AlternateSet glyph="Z">"#,
			r#"  <Alternate glyph="Z.fina"/>"#,
			"</AlternateSet>",
		]
	);
	Ok(())
}

#[test]
fn alternate_subst_test_from_xml() -> TestResult {
	let mut font = fake_font(ALTERNATE_GLYPHS);
	let mut table = ot::AlternateSubst::default();
	from_xml(
		&mut table,
		r#"<AlternateSet glyph="G">
		  <Alternate glyph="G.alt2"/>
		  <Alternate glyph="G.alt1"/>
		</AlternateSet>
		<AlternateSet glyph="Z">
		  <Alternate glyph="Z.fina"/>
		</AlternateSet>"#,
		&mut font,
	)?;
	assert_eq!(
		table.alternates,
		multiple_map(
			&mut font,
			&[("G", &["G.alt2", "G.alt1"]), ("Z", &["Z.fina"])]
		)
	);
	Ok(())
}

// The AAT actions

fn action_xml(font: &mut Font, action: &AatAction) -> Vec<String> {
	get_xml(|xw| action.to_xml(xw, font, &[("Test", &"Foo")], "Transition"))
}

#[test]
fn rearrangement_morph_action_test_compile() -> TestResult {
	let mut r = AatAction::new(ActionKind::Rearrangement);
	r.new_state = 0x1234;
	r.flags = vec![true, true, true];
	r.reserved_flags = 0x1FF0;
	r.verb = 0xD;
	assert_eq!(hex_str(&r.compile(None)?), "1234fffd");
	Ok(())
}

#[test]
fn rearrangement_morph_action_test_compile_actions() -> TestResult {
	let mut font = fake_font(".notdef A B C");
	assert!(compile_actions(ActionKind::Rearrangement, &mut font, &[])?.is_none());
	Ok(())
}

#[test]
fn rearrangement_morph_action_test_decompile_to_xml() -> TestResult {
	let mut font = fake_font(".notdef A B C");
	let data = hex("1234fffd");
	let r = AatAction::decompile(
		ActionKind::Rearrangement,
		&mut OtReader::new(&data, Tag::new(b"morx")),
		None,
	)?;
	assert_eq!(
		action_xml(&mut font, &r),
		[
			r#"<Transition Test="Foo">"#,
			r#"  <NewState value="4660"/>"#,
			r#"  <Flags value="MarkFirst,DontAdvance,MarkLast"/>"#,
			r#"  <ReservedFlags value="0x1FF0"/>"#,
			"  <Verb value=\"13\"/><!-- ABxCD \u{21d2} CDxBA -->",
			"</Transition>",
		]
	);
	Ok(())
}

#[test]
fn contextual_morph_action_test_compile() -> TestResult {
	let mut a = AatAction::new(ActionKind::Contextual);
	a.new_state = 0x1234;
	a.flags = vec![true, true];
	a.reserved_flags = 0x3117;
	a.mark_index = 0xDEAD;
	a.current_index = 0xBEEF;
	assert_eq!(hex_str(&a.compile(None)?), "1234f117deadbeef");
	Ok(())
}

#[test]
fn contextual_morph_action_test_compile_actions() -> TestResult {
	let mut font = fake_font(".notdef A B C");
	assert!(compile_actions(ActionKind::Contextual, &mut font, &[])?.is_none());
	Ok(())
}

#[test]
fn contextual_morph_action_test_decompile_to_xml() -> TestResult {
	let mut font = fake_font(".notdef A B C");
	let data = hex("1234f117deadbeef");
	let a = AatAction::decompile(
		ActionKind::Contextual,
		&mut OtReader::new(&data, Tag::new(b"morx")),
		None,
	)?;
	assert_eq!(
		action_xml(&mut font, &a),
		[
			r#"<Transition Test="Foo">"#,
			r#"  <NewState value="4660"/>"#,
			r#"  <Flags value="SetMark,DontAdvance"/>"#,
			r#"  <ReservedFlags value="0x3117"/>"#,
			r#"  <MarkIndex value="57005"/>"#,
			r#"  <CurrentIndex value="48879"/>"#,
			"</Transition>",
		]
	);
	Ok(())
}

#[test]
fn ligature_morph_action_test_decompile_to_xml() -> TestResult {
	let mut font = fake_font(".notdef A B C");
	let actions = hex("DEADBEEF 7FFFFFFE 80000003");
	let data = hex("1234FAB30001");
	let a = AatAction::decompile(
		ActionKind::Ligature,
		&mut OtReader::new(&data, Tag::new(b"morx")),
		Some(&OtReader::new(&actions, Tag::new(b"morx"))),
	)?;
	assert_eq!(
		action_xml(&mut font, &a),
		[
			r#"<Transition Test="Foo">"#,
			r#"  <NewState value="4660"/>"#,
			r#"  <Flags value="SetComponent,DontAdvance"/>"#,
			r#"  <ReservedFlags value="0x1AB3"/>"#,
			r#"  <Action GlyphIndexDelta="-2" Flags="Store"/>"#,
			r#"  <Action GlyphIndexDelta="3"/>"#,
			"</Transition>",
		]
	);
	Ok(())
}

#[test]
fn ligature_morph_action_test_compile_actions_empty() -> TestResult {
	let mut font = fake_font(".notdef A B C");
	let index = compile_actions(ActionKind::Ligature, &mut font, &[])?.ok_or("no actions")?;
	assert_eq!(index, ActionIndex::default());
	Ok(())
}

#[test]
fn ligature_morph_action_test_compile_actions_should_share_subsequences() -> TestResult {
	let mut font = fake_font(".notdef A B C");
	let ligs: Vec<LigAction> = (0..3)
		.map(|i| LigAction {
			store: false,
			glyph_index_delta: i,
		})
		.collect();
	let mut state = AatState::default();
	for (i, actions) in [&ligs[1..2], &ligs[0..3], &ligs[1..3]]
		.into_iter()
		.enumerate()
	{
		let mut t = AatAction::new(ActionKind::Ligature);
		t.actions = actions.to_vec();
		state.transitions.insert(i as i64, t);
	}
	let index = compile_actions(ActionKind::Ligature, &mut font, &[state])?.ok_or("no actions")?;
	assert_eq!(index.data, hex("00000000 00000001 80000002 80000001"));
	assert_eq!(
		index.ligatures,
		[
			(hex("00000000 00000001 80000002"), 0),
			(hex("00000001 80000002"), 1),
			(hex("80000002"), 2),
			(hex("80000001"), 3),
		]
		.into_iter()
		.collect()
	);
	Ok(())
}

const INSERTION_GLYPHS: &str = ".notdef A B C D";

const MORPH_ACTION_XML: [&str; 9] = [
	r#"<Transition Test="Foo">"#,
	r#"  <NewState value="4660"/>"#,
	r#"  <Flags value="SetMark,DontAdvance,CurrentIsKashidaLike,MarkedIsKashidaLike,CurrentInsertBefore,MarkedInsertBefore"/>"#,
	r#"  <CurrentInsertionAction glyph="B"/>"#,
	r#"  <CurrentInsertionAction glyph="C"/>"#,
	r#"  <MarkedInsertionAction glyph="B"/>"#,
	r#"  <MarkedInsertionAction glyph="A"/>"#,
	r#"  <MarkedInsertionAction glyph="D"/>"#,
	"</Transition>",
];

#[test]
fn insertion_morph_action_test_decompile_to_xml() -> TestResult {
	let mut font = fake_font(INSERTION_GLYPHS);
	let actions = hex("DEAD BEEF 0002 0001 0004 0002 0003 DEAD BEEF");
	let data = hex("1234 FC43 0005 0002");
	let a = AatAction::decompile(
		ActionKind::Insertion,
		&mut OtReader::new(&data, Tag::new(b"morx")),
		Some(&OtReader::new(&actions, Tag::new(b"morx"))),
	)?;
	assert_eq!(action_xml(&mut font, &a), MORPH_ACTION_XML);
	Ok(())
}

#[test]
fn insertion_morph_action_test_compile_from_xml() -> TestResult {
	let mut font = fake_font(INSERTION_GLYPHS);
	let element = xml_reader::parse(&MORPH_ACTION_XML.join("\n"))?;
	let a = AatAction::from_xml(ActionKind::Insertion, &element, &mut font)?;
	let index = ActionIndex {
		insertions: [
			(gids(&mut font, &["B", "C"]), 9),
			(gids(&mut font, &["B", "A", "D"]), 7),
		]
		.into_iter()
		.collect(),
		..Default::default()
	};
	assert_eq!(hex_str(&a.compile(Some(&index))?), "1234fc4300090007");
	Ok(())
}

#[test]
fn insertion_morph_action_test_compile_actions_empty() -> TestResult {
	let mut font = fake_font(INSERTION_GLYPHS);
	let index = compile_actions(ActionKind::Insertion, &mut font, &[])?.ok_or("no actions")?;
	assert_eq!(index, ActionIndex::default());
	Ok(())
}

#[test]
fn insertion_morph_action_test_compile_actions_should_share_subsequences() -> TestResult {
	let mut font = fake_font(INSERTION_GLYPHS);
	let mut state = AatState::default();
	let actions: [(&[&str], &[&str]); 3] = [
		(&[], &["A"]),
		(&["C", "D"], &["B"]),
		(&["B", "C", "D"], &["C", "D"]),
	];
	for (i, (current, marked)) in actions.into_iter().enumerate() {
		let mut t = AatAction::new(ActionKind::Insertion);
		t.current_insertion_action = gids(&mut font, current);
		t.marked_insertion_action = gids(&mut font, marked);
		state.transitions.insert(i as i64, t);
	}
	let index = compile_actions(ActionKind::Insertion, &mut font, &[state])?.ok_or("no actions")?;
	assert_eq!(index.data, hex("0002 0003 0004 0001"));
	let expected: Vec<(&[&str], usize)> = vec![
		(&["A"], 3),
		(&["B"], 0),
		(&["B", "C"], 0),
		(&["B", "C", "D"], 0),
		(&["C"], 1),
		(&["C", "D"], 1),
		(&["D"], 2),
	];
	assert_eq!(
		index.insertions,
		expected
			.into_iter()
			.map(|(g, i)| (gids(&mut font, g), i))
			.collect()
	);
	Ok(())
}

// SplitMultipleSubstTest: `buildMultipleSubstSubtable({"e": 1, "a": 2, ...})`,
// with sequences of one glyph standing for the integers.

fn split_multiple(
	item_name: &str,
	item_index: Option<usize>,
) -> TestResult<(Vec<String>, Vec<String>)> {
	let mut font = fake_font(".notdef a b c d e");
	let mut old = ot::MultipleSubst {
		mapping: multiple_map(
			&mut font,
			&[
				("e", &["a"]),
				("a", &["b"]),
				("b", &["c"]),
				("c", &["d"]),
				("d", &["e"]),
			],
		),
		..Default::default()
	};
	let mut new = ot::MultipleSubst::default();
	let record = OverflowErrorRecord {
		table_type: Tag::new(b"GSUB"),
		lookup_list_index: None,
		sub_table_index: None,
		item_name: Some(item_name.to_owned()),
		item_index,
	};
	assert!(split_multiple_subst(
		&mut old, &mut new, &record, &mut font
	)?);
	let old_keys: Vec<u32> = old.mapping.keys().copied().collect();
	let new_keys: Vec<u32> = new.mapping.keys().copied().collect();
	Ok((
		glyph_names(&mut font, &old_keys),
		glyph_names(&mut font, &new_keys),
	))
}

#[test]
fn split_multiple_subst_test_coverage() -> TestResult {
	let (old, new) = split_multiple("Coverage", None)?;
	assert_eq!(old, ["a", "b"]);
	assert_eq!(new, ["c", "d", "e"]);
	Ok(())
}

#[test]
fn split_multiple_subst_test_range_record() -> TestResult {
	let (old, new) = split_multiple("RangeRecord", None)?;
	assert_eq!(old, ["a", "b"]);
	assert_eq!(new, ["c", "d", "e"]);
	Ok(())
}

#[test]
fn split_multiple_subst_test_sequence() -> TestResult {
	let (old, new) = split_multiple("Sequence", Some(4))?;
	assert_eq!(old, ["a", "b", "c"]);
	assert_eq!(new, ["d", "e"]);
	Ok(())
}

fn record(tag: &[u8; 4], lookup: usize, sub_table: Option<usize>) -> OverflowErrorRecord {
	OverflowErrorRecord {
		table_type: Tag::new(tag),
		lookup_list_index: Some(lookup),
		sub_table_index: sub_table,
		item_name: None,
		item_index: None,
	}
}

fn lookup(lookup_type: u16, sub_table: ot::LookupSubTable) -> Option<Box<ot::Lookup>> {
	Some(Box::new(ot::Lookup {
		lookup_type,
		sub_table: vec![Some(Box::new(sub_table))],
		..Default::default()
	}))
}

#[test]
#[allow(non_snake_case)]
fn test_fixLookupOverFlows_promotes_lookup_to_extension() -> TestResult {
	let sub_table = ot::LookupSubTable::SingleSubst(ot::SingleSubst::default());
	let mut lookups = vec![lookup(1, sub_table.clone())];
	assert!(fix_lookup_overflows(
		&mut lookups,
		Tag::new(b"GSUB"),
		&record(b"GSUB", 0, Some(0))
	)?);
	let lookup = lookups[0].as_deref().ok_or("no lookup")?;
	assert_eq!(lookup.lookup_type, 7);
	let Some(ot::LookupSubTable::ExtensionSubst(ot::ExtensionSubst::Format1(ext))) =
		lookup.sub_table[0].as_deref()
	else {
		panic!("not an ExtensionSubst");
	};
	assert_eq!(ext.ext_sub_table.as_deref(), Some(&sub_table));
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_fixLookupOverFlows_returns_false_without_progress() -> TestResult {
	let sub_table = ot::LookupSubTable::SingleSubst(ot::SingleSubst::default());
	let mut lookups = vec![lookup(7, sub_table.clone())];
	assert!(!fix_lookup_overflows(
		&mut lookups,
		Tag::new(b"GSUB"),
		&record(b"GSUB", 0, Some(0))
	)?);
	assert_eq!(
		lookups[0].as_deref().ok_or("no lookup")?.sub_table[0].as_deref(),
		Some(&sub_table)
	);
	Ok(())
}

/// `_allExtensionGPOS`.
fn all_extension_gpos(num_lookups: usize) -> Vec<Option<Box<ot::Lookup>>> {
	(0..num_lookups)
		.map(|_| {
			lookup(
				9,
				ot::LookupSubTable::ExtensionPos(ot::ExtensionPos::Format1(
					ot::ExtensionPosFormat1 {
						extension_lookup_type: 1,
						ext_sub_table: Some(Box::new(ot::LookupSubTable::SinglePos(
							ot::SinglePos::default(),
						))),
						dont_share: false,
					},
				)),
			)
		})
		.collect()
}

#[test]
#[allow(non_snake_case)]
fn test_fixLookupOverFlows_all_extension_logs_error() -> TestResult {
	// The HarfBuzz repacker is always available (`have_uharfbuzz` is True), so
	// the message never suggests installing it.
	let mut lookups = all_extension_gpos(2);
	let (logs, ok) = capture(|| {
		fix_lookup_overflows(&mut lookups, Tag::new(b"GPOS"), &record(b"GPOS", 1, None))
	});
	assert!(!ok?);
	assert_eq!(logs.len(), 1, "{logs:?}");
	assert!(logs[0].contains("already Extension"));
	assert!(!logs[0].contains("install uharfbuzz"));
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_fixLookupOverFlows_subtable_overflow_does_not_log_lookuplist_message() -> TestResult {
	let mut lookups = all_extension_gpos(2);
	let (logs, ok) = capture(|| {
		fix_lookup_overflows(
			&mut lookups,
			Tag::new(b"GPOS"),
			&record(b"GPOS", 1, Some(0)),
		)
	});
	assert!(!ok?);
	assert!(
		!logs
			.iter()
			.any(|m| m.contains("LookupList offset overflowed"))
	);
	Ok(())
}

fn anchor(x: i16, y: i16) -> Option<Box<ot::Anchor>> {
	Some(Box::new(ot::Anchor::Format1(ot::AnchorFormat1 {
		x_coordinate: x,
		y_coordinate: y,
	})))
}

fn mark_base_xml(font: &mut Font, table: &ot::MarkBasePos) -> Vec<String> {
	get_xml(|xw| table.to_xml(xw, font, &[], "MarkBasePos"))
}

#[test]
#[allow(non_snake_case)]
fn test_splitMarkBasePos() -> TestResult {
	// `buildMarkBasePosSubtable(marks, bases, glyphMap)`.
	let mut font = fake_font("a c acutecomb gravecomb cedillacomb");
	let mut old = ot::MarkBasePos::Format1(ot::MarkBasePosFormat1 {
		mark_coverage: coverage(&mut font, &["acutecomb", "gravecomb", "cedillacomb"]),
		base_coverage: coverage(&mut font, &["a", "c"]),
		class_count: Some(2),
		mark_array: Some(Box::new(ot::MarkArray {
			mark_count: Some(3),
			mark_record: vec![
				ot::MarkRecord {
					class: 0,
					mark_anchor: anchor(0, 600),
				},
				ot::MarkRecord {
					class: 0,
					mark_anchor: anchor(0, 590),
				},
				ot::MarkRecord {
					class: 1,
					mark_anchor: anchor(0, 0),
				},
			],
		})),
		base_array: Some(Box::new(ot::BaseArray {
			base_count: Some(2),
			base_record: vec![
				ot::BaseRecord {
					base_anchor: vec![anchor(350, 500), None],
				},
				ot::BaseRecord {
					base_anchor: vec![anchor(300, 700), anchor(300, 0)],
				},
			],
		})),
		dont_share: false,
	});
	let mut new = ot::MarkBasePos::default();
	assert!(split_mark_base_pos(&mut old, &mut new)?);

	assert_eq!(
		mark_base_xml(&mut font, &old),
		[
			r#"<MarkBasePos Format="1">"#,
			"  <MarkCoverage>",
			r#"    <Glyph value="acutecomb"/>"#,
			r#"    <Glyph value="gravecomb"/>"#,
			"  </MarkCoverage>",
			"  <BaseCoverage>",
			r#"    <Glyph value="a"/>"#,
			r#"    <Glyph value="c"/>"#,
			"  </BaseCoverage>",
			"  <!-- ClassCount=1 -->",
			"  <MarkArray>",
			"    <!-- MarkCount=2 -->",
			r#"    <MarkRecord index="0">"#,
			r#"      <Class value="0"/>"#,
			r#"      <MarkAnchor Format="1">"#,
			r#"        <XCoordinate value="0"/>"#,
			r#"        <YCoordinate value="600"/>"#,
			"      </MarkAnchor>",
			"    </MarkRecord>",
			r#"    <MarkRecord index="1">"#,
			r#"      <Class value="0"/>"#,
			r#"      <MarkAnchor Format="1">"#,
			r#"        <XCoordinate value="0"/>"#,
			r#"        <YCoordinate value="590"/>"#,
			"      </MarkAnchor>",
			"    </MarkRecord>",
			"  </MarkArray>",
			"  <BaseArray>",
			"    <!-- BaseCount=2 -->",
			r#"    <BaseRecord index="0">"#,
			r#"      <BaseAnchor index="0" Format="1">"#,
			r#"        <XCoordinate value="350"/>"#,
			r#"        <YCoordinate value="500"/>"#,
			"      </BaseAnchor>",
			"    </BaseRecord>",
			r#"    <BaseRecord index="1">"#,
			r#"      <BaseAnchor index="0" Format="1">"#,
			r#"        <XCoordinate value="300"/>"#,
			r#"        <YCoordinate value="700"/>"#,
			"      </BaseAnchor>",
			"    </BaseRecord>",
			"  </BaseArray>",
			"</MarkBasePos>",
		]
	);
	assert_eq!(
		mark_base_xml(&mut font, &new),
		[
			r#"<MarkBasePos Format="1">"#,
			"  <MarkCoverage>",
			r#"    <Glyph value="cedillacomb"/>"#,
			"  </MarkCoverage>",
			"  <BaseCoverage>",
			r#"    <Glyph value="a"/>"#,
			r#"    <Glyph value="c"/>"#,
			"  </BaseCoverage>",
			"  <!-- ClassCount=1 -->",
			"  <MarkArray>",
			"    <!-- MarkCount=1 -->",
			r#"    <MarkRecord index="0">"#,
			r#"      <Class value="0"/>"#,
			r#"      <MarkAnchor Format="1">"#,
			r#"        <XCoordinate value="0"/>"#,
			r#"        <YCoordinate value="0"/>"#,
			"      </MarkAnchor>",
			"    </MarkRecord>",
			"  </MarkArray>",
			"  <BaseArray>",
			"    <!-- BaseCount=2 -->",
			r#"    <BaseRecord index="0">"#,
			r#"      <BaseAnchor index="0" empty="1"/>"#,
			"    </BaseRecord>",
			r#"    <BaseRecord index="1">"#,
			r#"      <BaseAnchor index="0" Format="1">"#,
			r#"        <XCoordinate value="300"/>"#,
			r#"        <YCoordinate value="0"/>"#,
			"      </BaseAnchor>",
			"    </BaseRecord>",
			"  </BaseArray>",
			"</MarkBasePos>",
		]
	);
	Ok(())
}

fn x_placement(x: i16) -> Option<ValueRecord> {
	Some(ValueRecord {
		x_placement: Some(x),
		..Default::default()
	})
}

fn x_placements(values: &[Option<ValueRecord>]) -> Vec<i16> {
	values
		.iter()
		.map(|v| v.as_ref().and_then(|v| v.x_placement).unwrap_or_default())
		.collect()
}

#[test]
#[allow(non_snake_case)]
fn test_splitSinglePos() -> TestResult {
	// `buildSinglePosSubtable(mapping, glyphMap)`.
	let mut font = fake_font("a b c");
	let mut old = ot::SinglePos::Format2(ot::SinglePosFormat2 {
		coverage: coverage(&mut font, &["a", "b", "c"]),
		value_format: 1,
		value_count: Some(3),
		value: vec![x_placement(-10), x_placement(-20), x_placement(-30)],
		dont_share: false,
	});
	let mut new = ot::SinglePos::default();
	assert!(split_single_pos(&mut old, &mut new)?);

	let ot::SinglePos::Format2(old) = old else {
		panic!("not format 2");
	};
	assert_eq!(old.coverage.as_deref().ok_or("no coverage")?.glyphs, [0]);
	assert_eq!(x_placements(&old.value), [-10]);
	assert_eq!(old.value_count, Some(1));

	let ot::SinglePos::Format2(new) = new else {
		panic!("not format 2");
	};
	assert_eq!(new.coverage.as_deref().ok_or("no coverage")?.glyphs, [1, 2]);
	assert_eq!(x_placements(&new.value), [-20, -30]);
	assert_eq!(new.value_count, Some(2));
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_splitSinglePos_format1() -> TestResult {
	// Format 1 has a single shared Value with nothing to split.
	let mut font = fake_font("a b");
	let mut table = ot::SinglePos::Format1(ot::SinglePosFormat1 {
		coverage: coverage(&mut font, &["a", "b"]),
		..Default::default()
	});
	assert!(!split_single_pos(
		&mut table,
		&mut ot::SinglePos::default()
	)?);
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_splitSinglePos_overflow_end_to_end() -> TestResult {
	// A SinglePos Format 2 whose ValueArray pushes its own Coverage offset past
	// uint16 must be split when packing.
	// https://github.com/fonttools/fonttools/issues/4091
	const N: u32 = 40000;
	let glyphs: Vec<String> = (0..N).map(|i| format!("g{i}")).collect();
	let mut font = Font::new();
	let mut order = vec![".notdef".to_owned()];
	order.extend(glyphs.iter().cloned());
	font.set_glyph_order(order);
	// Exercise the pure-fontTools overflow recovery, not the HarfBuzz repacker.
	font.use_harfbuzz_repacker = Some(false);

	let values: Vec<Option<ValueRecord>> = (0..N)
		.map(|i| x_placement(-((i % 100 + 1) as i16)))
		.collect();
	let sub_table = ot::SinglePos::Format2(ot::SinglePosFormat2 {
		coverage: Some(Box::new(ot::Coverage {
			glyphs: (1..=N).collect(),
		})),
		value_format: 1,
		value_count: None,
		value: values,
		dont_share: false,
	});
	let gpos = ot::GPOS {
		version: 0x00010000,
		script_list: Some(Box::new(ot::ScriptList {
			script_count: None,
			script_record: vec![ot::ScriptRecord {
				script_tag: Tag::new(b"DFLT"),
				script: Some(Box::new(ot::Script {
					default_lang_sys: Some(Box::new(ot::LangSys {
						lookup_order: None,
						req_feature_index: 0xFFFF,
						feature_count: None,
						feature_index: vec![0],
					})),
					lang_sys_count: None,
					lang_sys_record: Vec::new(),
				})),
			}],
		})),
		feature_list: Some(Box::new(ot::FeatureList {
			feature_count: None,
			feature_record: vec![ot::FeatureRecord {
				feature_tag: Tag::new(b"kern"),
				feature: Some(Box::new(ot::Feature {
					feature_params: None,
					lookup_count: None,
					lookup_list_index: vec![0],
				})),
			}],
		})),
		lookup_list: Some(Box::new(ot::LookupList {
			lookup_count: None,
			lookup: vec![lookup(1, ot::LookupSubTable::SinglePos(sub_table))],
		})),
		..Default::default()
	};
	font.insert(Box::new(Gpos { table: gpos }));
	let data = font.table_data(Gpos::TAG)?;

	let rebuilt = Gpos::decompile(&data, &mut font)?;
	let lookup = rebuilt
		.lookup_list
		.as_deref()
		.ok_or("no lookup list")?
		.lookup[0]
		.as_deref()
		.ok_or("no lookup")?;
	// The oversized subtable was split.
	assert!(lookup.sub_table.len() > 1);
	let mut coverage = Vec::new();
	let mut values = Vec::new();
	for st in &lookup.sub_table {
		let Some(ot::LookupSubTable::SinglePos(ot::SinglePos::Format2(st))) = st.as_deref() else {
			panic!("not a SinglePos format 2");
		};
		coverage.extend(glyph_names(
			&mut font,
			&st.coverage.as_deref().ok_or("no coverage")?.glyphs,
		));
		values.extend(x_placements(&st.value));
	}
	assert_eq!(coverage, glyphs);
	assert_eq!(
		values,
		(0..N).map(|i| -((i % 100 + 1) as i16)).collect::<Vec<_>>()
	);
	Ok(())
}

// ColrV1Test::test_traverseEmptyPaintColrLayersNeedsNoLayerList is not ported:
// `Paint.traverse` is not part of ttlib (fontoxide-subset has its own traversal,
// L21).

#[test]
#[allow(non_snake_case)]
fn colr_v1_test_PaintGlyph2_round_trip() -> TestResult {
	let mut font = sparse_font(&[("high", 0x10000)]);
	let paint = ot::Paint::Format33(ot::PaintFormat33 {
		glyph: gid(&mut font, "high"),
		paint: Some(Box::new(ot::Paint::Format2(ot::PaintFormat2 {
			palette_index: 3,
			alpha: 1.0,
		}))),
	});
	let data = "210000070100000200034000";
	assert_eq!(compile_table(&paint, &mut font, false)?, data);

	let rebuilt: ot::Paint = decompile_table(data, &mut font)?;
	let ot::Paint::Format33(rebuilt) = rebuilt else {
		panic!("not PaintGlyph2");
	};
	assert_eq!(glyph_names(&mut font, &[rebuilt.glyph]), ["high"]);
	assert!(matches!(
		rebuilt.paint.as_deref(),
		Some(ot::Paint::Format2(_))
	));
	Ok(())
}

const DEVICE_GPOS_XML: &str = r#"<Version value="0x00010000"/>
<ScriptList>
  <!-- ScriptCount=1 -->
  <ScriptRecord index="0">
    <ScriptTag value="DFLT"/>
    <Script>
      <DefaultLangSys>
        <ReqFeatureIndex value="65535"/>
        <!-- FeatureCount=1 -->
        <FeatureIndex index="0" value="0"/>
      </DefaultLangSys>
      <!-- LangSysCount=0 -->
    </Script>
  </ScriptRecord>
</ScriptList>
<FeatureList>
  <!-- FeatureCount=1 -->
  <FeatureRecord index="0">
    <FeatureTag value="curs"/>
    <Feature>
      <!-- LookupCount=1 -->
      <LookupListIndex index="0" value="0"/>
    </Feature>
  </FeatureRecord>
</FeatureList>
<LookupList>
  <!-- LookupCount=1 -->
  <Lookup index="0">
    <LookupType value="3"/>
    <LookupFlag value="0"/>
    <!-- SubTableCount=1 -->
    <CursivePos index="0" Format="1">
      <Coverage>
        <Glyph value="five"/>
      </Coverage>
      <!-- EntryExitCount=1 -->
      <EntryExitRecord index="0">
        <EntryAnchor Format="3">
          <XCoordinate value="124"/>
          <YCoordinate value="-4"/>
          <XDeviceTable>
            <StartSize value="8"/>
            <EndSize value="9"/>
            <DeltaFormat value="2"/>
            <DeltaValue value="[1, 2]"/>
          </XDeviceTable>
          <YDeviceTable>
            <StartSize value="7"/>
            <EndSize value="7"/>
            <DeltaFormat value="2"/>
            <DeltaValue value="[3]"/>
          </YDeviceTable>
        </EntryAnchor>
        <ExitAnchor Format="2">
          <XCoordinate value="3"/>
          <YCoordinate value="4"/>
          <AnchorPoint value="2"/>
        </ExitAnchor>
      </EntryExitRecord>
    </CursivePos>
  </Lookup>
</LookupList>"#;

#[test]
#[allow(non_snake_case)]
fn test_parse_Device_DeltaValue_from_XML_and_compile() -> TestResult {
	// https://github.com/fonttools/fonttools/pull/3757
	let mut font = fake_font(".notdef five");
	font.import_ttx(&format!("<ttFont><GPOS>{DEVICE_GPOS_XML}</GPOS></ttFont>"))?;
	{
		let gpos = font.require::<Gpos>()?;
		let lookup = gpos.lookup_list.as_deref().ok_or("no lookup list")?.lookup[0]
			.as_deref()
			.ok_or("no lookup")?;
		let Some(ot::LookupSubTable::CursivePos(ot::CursivePos::Format1(cursive))) =
			lookup.sub_table[0].as_deref()
		else {
			panic!("not a CursivePos");
		};
		let Some(ot::Anchor::Format3(anchor)) =
			cursive.entry_exit_record[0].entry_anchor.as_deref()
		else {
			panic!("not a format 3 anchor");
		};
		assert_eq!(
			anchor
				.x_device_table
				.as_deref()
				.ok_or("no device")?
				.delta_value,
			[1, 2]
		);
		assert_eq!(
			anchor
				.y_device_table
				.as_deref()
				.ok_or("no device")?
				.delta_value,
			[3]
		);
	}
	let data = font.table_data(Gpos::TAG)?;
	let gpos2 = Gpos::decompile(&data, &mut font)?;
	let lines = get_xml(|xw| gpos2.to_xml(xw, &mut font));
	assert_eq!(lines.join("\n"), DEVICE_GPOS_XML);
	Ok(())
}

// FeatureVariations with FeatureParams

/// What the tests need of `GSUB` and `GPOS`.
trait Layout: TableType + Table + TableXml + Clone + 'static {
	const NAME: &'static str;
	fn layout(&self) -> LayoutRef<'_>;
	fn feature_list_mut(&mut self, extended: bool) -> Option<&mut ot::FeatureList>;
	fn feature_variations_mut(&mut self) -> Option<&mut ot::FeatureVariations>;
	fn new_table(version: u32, feature_variations: Option<Box<ot::FeatureVariations>>) -> Self;
}

struct LayoutRef<'a> {
	feature_list: Option<&'a ot::FeatureList>,
	feature_list2: Option<&'a ot::FeatureList>,
	feature_variations: Option<&'a ot::FeatureVariations>,
}

macro_rules! layout_impl {
	($ty:ty, $ot:ident, $name:literal) => {
		impl Layout for $ty {
			const NAME: &'static str = $name;
			fn layout(&self) -> LayoutRef<'_> {
				LayoutRef {
					feature_list: self.feature_list.as_deref(),
					feature_list2: self.feature_list2.as_deref(),
					feature_variations: self.feature_variations.as_deref(),
				}
			}
			fn feature_list_mut(&mut self, extended: bool) -> Option<&mut ot::FeatureList> {
				if extended {
					self.feature_list2.as_deref_mut()
				} else {
					self.feature_list.as_deref_mut()
				}
			}
			fn feature_variations_mut(&mut self) -> Option<&mut ot::FeatureVariations> {
				self.feature_variations.as_deref_mut()
			}
			fn new_table(
				version: u32,
				feature_variations: Option<Box<ot::FeatureVariations>>,
			) -> Self {
				Self {
					table: ot::$ot {
						version,
						script_list: Some(Box::default()),
						feature_list: Some(Box::default()),
						lookup_list: Some(Box::default()),
						feature_variations,
						..Default::default()
					},
				}
			}
		}
	};
}

layout_impl!(Gsub, GSUB, "GSUB");
layout_impl!(Gpos, GPOS, "GPOS");

/// `FEATURE_PARAMS`: tag, TTX and class of the parameters.
const FEATURE_PARAMS: [(&str, &str, Option<&str>); 4] = [
	("kern", "", None),
	(
		"size",
		r#"<FeatureParamsSize><DesignSize value="12.0"/><SubfamilyID value="1"/><SubfamilyNameID value="256"/><RangeStart value="10.0"/><RangeEnd value="14.0"/></FeatureParamsSize>"#,
		Some("FeatureParamsSize"),
	),
	(
		"ss02",
		r#"<FeatureParamsStylisticSet><Version value="0"/><UINameID value="270"/></FeatureParamsStylisticSet>"#,
		Some("FeatureParamsStylisticSet"),
	),
	(
		"cv01",
		r#"<FeatureParamsCharacterVariants><Format value="0"/><FeatUILabelNameID value="256"/><FeatUITooltipTextNameID value="257"/><SampleTextNameID value="258"/><NumNamedParameters value="2"/><FirstParamUILabelNameID value="259"/><Character index="0" value="0x0041"/><Character index="1" value="0x10000"/></FeatureParamsCharacterVariants>"#,
		Some("FeatureParamsCharacterVariants"),
	),
];

fn layout_font() -> Font {
	fake_font(".notdef a b")
}

/// `_feature_variations_table`: a font with the table.
fn feature_variations_font<L: Layout>(
	features: &[&str],
	substitutions: &[(u32, &str)],
	extended: bool,
) -> TestResult<Font> {
	let lookup = if L::NAME == "GSUB" {
		r#"<SingleSubst><Substitution in="a" out="b"/></SingleSubst>"#
	} else {
		r#"<SinglePos Format="1"><Coverage><Glyph value="a"/></Coverage><ValueFormat value="1"/><Value XPlacement="10"/></SinglePos>"#
	};
	let lookups: String = (0..2)
		.map(|i| {
			format!(
				r#"<Lookup index="{i}"><LookupType value="1"/><LookupFlag value="0"/>{lookup}</Lookup>"#
			)
		})
		.collect();
	let feature_records: String = features
		.iter()
		.enumerate()
		.map(|(i, tag)| {
			format!(
				r#"<FeatureRecord index="{i}"><FeatureTag value="{tag}"/><Feature><LookupListIndex index="0" value="0"/></Feature></FeatureRecord>"#
			)
		})
		.collect();
	let substitution_records: String = substitutions
		.iter()
		.enumerate()
		.map(|(i, (index, params))| {
			format!(
				r#"<SubstitutionRecord index="{i}"><FeatureIndex value="{index}"/><Feature>{params}<LookupListIndex index="0" value="1"/></Feature></SubstitutionRecord>"#
			)
		})
		.collect();
	let name = L::NAME;
	let mut font = layout_font();
	font.import_ttx(&format!(
		r#"<ttFont><{name}><Version value="0x00010001"/><ScriptList/><LookupList>{lookups}</LookupList><FeatureList>{feature_records}</FeatureList><FeatureVariations><Version value="0x00010000"/><FeatureVariationRecord index="0"><ConditionSet/><FeatureTableSubstitution><Version value="0x00010000"/>{substitution_records}</FeatureTableSubstitution></FeatureVariationRecord></FeatureVariations></{name}></ttFont>"#
	))?;
	if extended {
		upper_tables(
			&mut font,
			&ConvertOptions {
				tables: Some(&[name]),
				validate: false,
				..ConvertOptions::default()
			},
		)?;
	}
	Ok(font)
}

fn substitution_records(table: &impl Layout) -> &[ot::FeatureTableSubstitutionRecord] {
	&table
		.layout()
		.feature_variations
		.unwrap()
		.feature_variation_record[0]
		.feature_table_substitution
		.as_deref()
		.unwrap()
		.substitution_record
}

fn params_of(record: &ot::FeatureTableSubstitutionRecord) -> Option<&ot::FeatureParamsAny> {
	record.feature.as_deref().unwrap().feature_params.as_deref()
}

fn params_class(params: Option<&ot::FeatureParamsAny>) -> Option<&'static str> {
	params.map(|p| p.class_name())
}

fn params_xml(font: &mut Font, params: &ot::FeatureParamsAny) -> Vec<String> {
	get_xml(|xw| params.to_xml(xw, font, &[], params.class_name()))
}

/// The table compiled, then decompiled.
fn compile_and_decompile<L: Layout>(font: &mut Font) -> TestResult<(Vec<u8>, L)> {
	let data = font.table_data(L::TAG)?;
	let rebuilt = L::decompile(&data, font)?;
	Ok((data, rebuilt))
}

/// The table written as TTX, read back into a new font and compiled.
fn xml_round_trip<L: Layout>(table: &L, font: &mut Font) -> TestResult<Vec<u8>> {
	let xml = get_xml(|xw| table.to_xml(xw, font)).join("\n");
	let name = L::NAME;
	let mut font2 = layout_font();
	font2.import_ttx(&format!("<ttFont><{name}>{xml}</{name}></ttFont>"))?;
	Ok(font2.table_data(L::TAG)?)
}

fn feature_variations_params_roundtrip<L: Layout>() -> TestResult {
	for extended in [false, true] {
		for feature_count in [1usize, 10] {
			for (feature_tag, params, param_class) in FEATURE_PARAMS {
				// Unrelated features precede the referenced feature.
				let mut features = vec!["aalt"; feature_count - 1];
				features.push(feature_tag);
				let mut font = feature_variations_font::<L>(
					&features,
					&[(feature_count as u32 - 1, params)],
					extended,
				)?;
				let (data, rebuilt) = compile_and_decompile::<L>(&mut font)?;
				let record = &substitution_records(&rebuilt)[0];
				assert_eq!(record.feature_index as usize, feature_count - 1);
				assert_eq!(record.feature.as_deref().unwrap().lookup_list_index, [1]);
				let layout = rebuilt.layout();
				let feature_list = if extended {
					layout.feature_list2
				} else {
					layout.feature_list
				}
				.ok_or("no feature list")?;
				let last = feature_list.feature_record.last().ok_or("no feature")?;
				assert_eq!(last.feature.as_deref().unwrap().lookup_list_index, [0]);
				let actual = params_of(record);
				assert_eq!(params_class(actual), param_class, "{feature_tag}");
				let original = font.require::<L>()?.clone();
				let expected = params_of(&substitution_records(&original)[0]);
				if let (Some(actual), Some(expected)) = (actual, expected) {
					assert_eq!(
						params_xml(&mut font, actual),
						params_xml(&mut font, expected)
					);
				}
				assert_eq!(xml_round_trip(&rebuilt, &mut font)?, data);
			}
		}
	}
	Ok(())
}

#[test]
fn test_feature_variations_params_roundtrip() -> TestResult {
	feature_variations_params_roundtrip::<Gsub>()?;
	feature_variations_params_roundtrip::<Gpos>()
}

fn feature_variations_params_mixed_tags<L: Layout>() -> TestResult {
	for extended in [false, true] {
		let mut cases: Vec<_> = FEATURE_PARAMS.iter().flat_map(|c| [*c, *c, *c]).collect();
		cases.sort_by_key(|c| c.0);
		let mut features = vec!["aalt"; 8];
		features.extend(cases.iter().map(|c| c.0));
		let substitutions: Vec<(u32, &str)> = cases
			.iter()
			.enumerate()
			.map(|(i, c)| (8 + i as u32, c.1))
			.collect();
		let mut font = feature_variations_font::<L>(&features, &substitutions, extended)?;
		let (data, mut rebuilt) = compile_and_decompile::<L>(&mut font)?;
		let original = font.require::<L>()?.clone();
		let records = substitution_records(&rebuilt);
		for i in (0..cases.len()).rev() {
			let record = &records[i];
			assert_eq!(record.feature_index as usize, 8 + i);
			let actual = params_of(record);
			assert_eq!(params_class(actual), cases[i].2);
			if let (Some(actual), Some(expected)) =
				(actual, params_of(&substitution_records(&original)[i]))
			{
				assert_eq!(
					params_xml(&mut font, actual),
					params_xml(&mut font, expected)
				);
			}
		}
		assert_eq!(rebuilt.compile(&mut font)?, data);
	}
	Ok(())
}

#[test]
fn test_feature_variations_params_mixed_tags() -> TestResult {
	feature_variations_params_mixed_tags::<Gsub>()?;
	feature_variations_params_mixed_tags::<Gpos>()
}

fn feature_variations_params_wrong_type<L: Layout>() -> TestResult {
	for extended in [false, true] {
		let mut font =
			feature_variations_font::<L>(&["cv01"], &[(0, FEATURE_PARAMS[2].1)], extended)?;
		let error = font.table_data(L::TAG).unwrap_err();
		assert!(
			format!("{error:?}").contains("Wrong FeatureParams type for feature 'cv01'"),
			"{error:?}"
		);
	}
	Ok(())
}

#[test]
fn test_feature_variations_params_wrong_type() -> TestResult {
	feature_variations_params_wrong_type::<Gsub>()?;
	feature_variations_params_wrong_type::<Gpos>()
}

fn feature_variations_params_after_feature_list_mutation<L: Layout>() -> TestResult {
	for extended in [false, true] {
		let mut font =
			feature_variations_font::<L>(&["aalt", "ss02"], &[(1, FEATURE_PARAMS[2].1)], extended)?;
		let (_, mut rebuilt) = compile_and_decompile::<L>(&mut font)?;
		// Subsetting edits FeatureList before it remaps FeatureVariations.
		rebuilt
			.feature_list_mut(extended)
			.ok_or("no feature list")?
			.feature_record
			.remove(0);
		let record = &substitution_records(&rebuilt)[0];
		assert_eq!(record.feature_index, 1);
		let Some(ot::FeatureParamsAny::FeatureParamsStylisticSet(params)) = params_of(record)
		else {
			panic!("not FeatureParamsStylisticSet");
		};
		assert_eq!(params.ui_name_id, 270);
		rebuilt
			.feature_variations_mut()
			.ok_or("no variations")?
			.feature_variation_record[0]
			.feature_table_substitution
			.as_deref_mut()
			.ok_or("no substitution")?
			.substitution_record[0]
			.feature_index = 0;
		assert!(!rebuilt.compile(&mut font)?.is_empty());
	}
	Ok(())
}

#[test]
fn test_feature_variations_params_after_feature_list_mutation() -> TestResult {
	feature_variations_params_after_feature_list_mutation::<Gsub>()?;
	feature_variations_params_after_feature_list_mutation::<Gpos>()
}

fn empty_feature_list_compile<L: Layout>() -> TestResult {
	for (version, has_variations) in [(0x00010000, false), (0x00010001, false), (0x00010001, true)]
	{
		let mut font = fake_font(".notdef");
		// Missing repeated fields compile as empty arrays.
		let variations = has_variations.then(|| {
			Box::new(ot::FeatureVariations {
				version: 0x00010000,
				..Default::default()
			})
		});
		font.insert(Box::new(L::new_table(version, variations)));
		let (_, rebuilt) = compile_and_decompile::<L>(&mut font)?;
		let layout = rebuilt.layout();
		let feature_list = layout.feature_list.ok_or("no feature list")?;
		assert_eq!(feature_list.feature_count, Some(0));
		assert!(feature_list.feature_record.is_empty());
		if has_variations {
			assert!(
				layout
					.feature_variations
					.ok_or("no variations")?
					.feature_variation_record
					.is_empty()
			);
		}
	}
	Ok(())
}

#[test]
fn test_empty_feature_list_compile() -> TestResult {
	empty_feature_list_compile::<Gsub>()?;
	empty_feature_list_compile::<Gpos>()
}

// `test_feature_list_stays_lazy_without_variations` is not ported: it checks
// that FeatureList is still lazy (`reader` in `__dict__`).

#[test]
fn test_feature_variations_params_null_offset() -> TestResult {
	// Null offsets don't require a referenced FeatureRecord or tag context.
	let mut font = Font::new();
	let data = hex("FFFF 00000000");
	let record = ot::FeatureTableSubstitutionRecord::decompile(
		&mut OtReader::new(&data, Tag::new(b"GSUB")),
		&mut font,
	)?;
	assert!(record.feature.is_none());
	assert_eq!(compile_table(&record, &mut font, false)?, "ffff00000000");
	Ok(())
}

#[test]
fn test_feature_variations_params_without_context() -> TestResult {
	// A standalone record without parameters does not need a FeatureList.
	let data = hex("0000 00000006 0000 0000");
	let mut font = Font::new();
	// read
	let record = ot::FeatureTableSubstitutionRecord::decompile(
		&mut OtReader::new(&data, Tag::new(b"GSUB")),
		&mut font,
	)?;
	assert_eq!(record.feature_index, 0);
	let feature = record.feature.as_deref().ok_or("no feature")?;
	assert!(feature.feature_params.is_none());
	assert!(feature.lookup_list_index.is_empty());
	// write
	let record = ot::FeatureTableSubstitutionRecord {
		feature_index: 0,
		feature: Some(Box::default()),
	};
	assert_eq!(compile_table(&record, &mut font, false)?, hex_str(&data));
	Ok(())
}

fn feature_variations_params_decompile<L: Layout>() -> TestResult {
	for extended in [false, true] {
		// Independently encoded: ss02 FeatureParams is in an alternate Feature
		// table only.
		let header = if extended {
			"00010002 0000 0000 0000 0000002a 0000001a 0000001c 00000028"
		} else {
			"00010001 000e 0010 001c 0000001e"
		};
		let data = hex(&format!(
			"{header} 0000 \
			 0001 73733032 0008 0000 0000 \
			 0000 \
			 00010000 00000001 00000010 00000012 \
			 0000 \
			 00010000 0001 0000 0000000c \
			 0004 0000 0000 010e"
		));
		let mut font = layout_font();
		let table = L::decompile(&data, &mut font)?;
		let Some(ot::FeatureParamsAny::FeatureParamsStylisticSet(params)) =
			params_of(&substitution_records(&table)[0])
		else {
			panic!("not FeatureParamsStylisticSet");
		};
		assert_eq!(params.version, 0);
		assert_eq!(params.ui_name_id, 270);
	}
	Ok(())
}

#[test]
fn test_feature_variations_params_decompile() -> TestResult {
	feature_variations_params_decompile::<Gsub>()?;
	feature_variations_params_decompile::<Gpos>()
}

fn feature_variations_out_of_range_index_decompile<L: Layout>() -> TestResult {
	for feature_index in [1u16, 0xFFFF] {
		let data = hex(&format!(
			"00010001 000e 0010 001c 0000001e 0000 \
			 0001 73733032 0008 0000 0000 \
			 0000 \
			 00010000 00000001 00000010 00000012 \
			 0000 \
			 00010000 0001 {feature_index:04x} 0000000c \
			 0000 0000"
		));
		let mut font = layout_font();
		let table = L::decompile(&data, &mut font)?;
		let record = &substitution_records(&table)[0];
		assert_eq!(record.feature_index, feature_index);
		assert!(params_of(record).is_none());
		assert!(
			record
				.feature
				.as_deref()
				.ok_or("no feature")?
				.lookup_list_index
				.is_empty()
		);
	}
	Ok(())
}

#[test]
fn test_feature_variations_out_of_range_index_decompile() -> TestResult {
	feature_variations_out_of_range_index_decompile::<Gsub>()?;
	feature_variations_out_of_range_index_decompile::<Gpos>()
}

fn feature_variations_out_of_range_index_compile<L: Layout>() -> TestResult {
	// `omit_params` (deleting the attribute instead of leaving it None) is the
	// same case here.
	for features in [&[][..], &["ss02"][..]] {
		for max_index in [false, true] {
			let feature_index = if max_index {
				0xFFFF
			} else {
				features.len() as u32
			};
			let mut font = feature_variations_font::<L>(features, &[(feature_index, "")], false)?;
			let (data, rebuilt) = compile_and_decompile::<L>(&mut font)?;
			let record = &substitution_records(&rebuilt)[0];
			assert_eq!(record.feature_index as u32, feature_index);
			assert!(params_of(record).is_none());
			assert_eq!(record.feature.as_deref().unwrap().lookup_list_index, [1]);
			assert_eq!(xml_round_trip(&rebuilt, &mut font)?, data);
		}
	}
	Ok(())
}

#[test]
fn test_feature_variations_out_of_range_index_compile() -> TestResult {
	feature_variations_out_of_range_index_compile::<Gsub>()?;
	feature_variations_out_of_range_index_compile::<Gpos>()
}
