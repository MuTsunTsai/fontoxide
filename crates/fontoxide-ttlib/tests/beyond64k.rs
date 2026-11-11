//! The tests of fontTools' `Tests/ttLib/beyond64k_test.py`, but those of subsetting and
//! `maxContextCalc`, which are in `fontoxide-subset`, and the tests of beyond-64k fonts of
//! `Tests/ttLib/woff2_test.py` and `Tests/ttLib/tables/otTables_test.py`.
//!
//! The tables these tests build with `feaLib` and `otlLib`, which are not ported yet, are in
//! `tests/data/beyond64k`, made with the same code by `tools/reference/beyond64k_fixtures.py`.
//! The tests with a `lazy` parameter only have the fully loaded case, as nothing is lazy here.
//! `glyph_paths` uses a recording pen where fontTools uses `SVGPathPen`, not ported yet.

use std::collections::BTreeMap;
use std::path::PathBuf;

use fontoxide_pens::{BasePen, Pen, Point, SegmentPen};
use fontoxide_ttlib::beyond64k::{
	ConvertOptions, convert_layout_formats, help, lower_tables, main, upper_tables,
};
use fontoxide_ttlib::ot::reader::OtReader;
use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::varmap::VarIdxMap;
use fontoxide_ttlib::ot::writer::WriterGraph;
use fontoxide_ttlib::tables::cmap::{CharMap, UvsDict};
use fontoxide_ttlib::tables::colr::Colr;
use fontoxide_ttlib::tables::glyf::{
	ComponentPlacement, FLAG_CUBIC, FLAG_ON_CURVE, Glyph, GlyphComponent, Number,
};
use fontoxide_ttlib::tables::otl::Jstf;
use fontoxide_ttlib::tables::tt_program::Program;
use fontoxide_ttlib::tables::{
	Base, Cmap, CmapSubtable, Dmap, Gdef, Glyf, Gpos, Gsub, Head, Hhea, Hmtx, Loca, Maxp, Post,
	Vhea, Vmtx, Vorg,
};
use fontoxide_ttlib::{
	Error, Flavor, FlavorData, Font, GlyphSet, ReadOptions, TableType, Tag, WriteOptions,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const COMPANIONS_LOWER: [&[u8; 4]; 5] = [b"glyf", b"loca", b"maxp", b"hhea", b"hmtx"];
const COMPANIONS_UPPER: [&[u8; 4]; 5] = [b"GLYF", b"LOCA", b"MAXP", b"HHEA", b"HMTX"];

fn upstream(relative: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests")
		.join(relative)
}

fn fixture(name: &str) -> String {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data/beyond64k")
		.join(format!("{name}.ttx"));
	std::fs::read_to_string(path).expect("a fixture")
}

/// `font.importXML(DATA_DIR / "TestTTF-Regular.ttx")`.
fn test_ttf() -> TestResult<Font> {
	let mut font = Font::new();
	font.import_ttx(&std::fs::read_to_string(upstream(
		"ttLib/data/TestTTF-Regular.ttx",
	))?)?;
	Ok(font)
}

/// `TestTTF-Regular.ttx` with the tables of a fixture.
fn test_ttf_with(name: &str) -> TestResult<Font> {
	let mut font = test_ttf()?;
	font.import_ttx(&fixture(name))?;
	Ok(font)
}

fn save_and_read(font: &mut Font) -> TestResult<Font> {
	Ok(Font::read(
		font.write(&WriteOptions::default())?,
		ReadOptions::default(),
	)?)
}

fn names(names: &[&str]) -> Vec<String> {
	names.iter().map(|n| (*n).to_owned()).collect()
}

/// `[".notdef", "glyph1", ..., last]`, of `count` glyphs.
fn glyph_order(count: u32, last: &str) -> Vec<String> {
	let mut order = vec![".notdef".to_owned()];
	order.extend((1..count - 1).map(|i| format!("glyph{i}")));
	order.push(last.to_owned());
	order
}

fn tables<'a>(tables: &'a [&'a str]) -> ConvertOptions<'a> {
	ConvertOptions {
		tables: Some(tables),
		..Default::default()
	}
}

fn no_validate<'a>(tables: &'a [&'a str]) -> ConvertOptions<'a> {
	ConvertOptions {
		tables: Some(tables),
		validate: false,
		..Default::default()
	}
}

fn all_no_validate() -> ConvertOptions<'static> {
	ConvertOptions {
		validate: false,
		..Default::default()
	}
}

fn tag_names(font: &Font) -> Vec<String> {
	font.tags().iter().map(|t| t.to_string()).collect()
}

fn assert_error_contains<T: std::fmt::Debug>(result: fontoxide_ttlib::Result<T>, text: &str) {
	let error: Error = result.expect_err(text);
	assert!(error.to_string().contains(text), "{error}");
}

fn draw_paths(font: &mut Font) -> TestResult<BTreeMap<String, Outline>> {
	let order = font.glyph_order()?;
	let glyph_set = GlyphSet::new(font)?;
	let mut paths = BTreeMap::new();
	for name in order.names() {
		let mut pen = BasePen::with_inner(Recording::default(), Some(&glyph_set));
		glyph_set.draw(name, &mut pen as &mut dyn Pen)?;
		paths.insert(name.clone(), pen.inner.0);
	}
	Ok(paths)
}

type Outline = Vec<(&'static str, Vec<Point>)>;

/// A segment pen that records what it gets, standing in for `SVGPathPen`.
#[derive(Default)]
struct Recording(Outline);

impl SegmentPen for Recording {
	fn move_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.0.push(("moveTo", vec![pt]));
		Ok(())
	}
	fn line_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.0.push(("lineTo", vec![pt]));
		Ok(())
	}
	fn curve_to_one(
		&mut self,
		_current: Option<Point>,
		pt1: Point,
		pt2: Point,
		pt3: Point,
	) -> fontoxide_pens::Result<()> {
		self.0.push(("curveTo", vec![pt1, pt2, pt3]));
		Ok(())
	}
	fn close_path(&mut self) -> fontoxide_pens::Result<()> {
		self.0.push(("closePath", Vec::new()));
		Ok(())
	}
	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		self.0.push(("endPath", Vec::new()));
		Ok(())
	}
}

#[test]
fn test_cff2_metric_header_recalculation() -> TestResult {
	for fontfile in ["ttLib/data/I.otf", "cffLib/data/varc-short-cff2.otf"] {
		for uppercase in [false, true] {
			let mut font = Font::read(std::fs::read(upstream(fontfile))?, ReadOptions::default())?;
			let count = font.glyph_order()?.len() as u32;
			let mut vmtx = Vmtx::default();
			for gid in 0..count {
				vmtx.set(gid, (800, 20));
			}
			font.insert(Box::new(vmtx));
			// `FontBuilder.setupVerticalHeader`.
			font.insert(Box::new(Vhea {
				table_version: 0x0001_1000,
				..Default::default()
			}));
			let mut hhea = font.require::<Hhea>()?.clone();
			hhea.recalc(&mut font)?;
			font.insert(Box::new(hhea));
			let mut vhea = font.require::<Vhea>()?.clone();
			vhea.recalc(&mut font)?;
			font.insert(Box::new(vhea));
			if uppercase {
				upper_tables(&mut font, &ConvertOptions::default())?;
			}
			assert!(font.require::<Hhea>()?.advance_width_max < 5000);
			assert!(font.require::<Vhea>()?.advance_height_max < 6000);
			font.require::<Hmtx>()?.metrics = (0..count).map(|g| (g, (5000, 10))).collect();
			font.require::<Vmtx>()?.metrics = (0..count).map(|g| (g, (6000, 20))).collect();

			let mut reloaded = save_and_read(&mut font)?;
			let hhea = reloaded.require::<Hhea>()?;
			assert_eq!(hhea.extended, uppercase);
			assert_eq!(hhea.advance_width_max, 5000);
			assert_eq!(hhea.min_left_side_bearing, 10);
			let vhea = reloaded.require::<Vhea>()?;
			assert_eq!(vhea.extended, uppercase);
			assert_eq!(vhea.advance_height_max, 6000);
			assert_eq!(vhea.min_top_side_bearing, 20);
		}
	}
	Ok(())
}

/// The lookups of a layout table, in whichever list they are.
fn lookups_of(
	list: Option<&ot::LookupList>,
	list2: Option<&ot::LookupList2>,
) -> Vec<ot::LookupSubTable> {
	let lookups = match (list, list2) {
		(_, Some(list)) => &list.lookup,
		(Some(list), None) => &list.lookup,
		(None, None) => return Vec::new(),
	};
	lookups
		.iter()
		.flatten()
		.flat_map(|lookup| lookup.sub_table.iter().flatten())
		.map(|subtable| (**subtable).clone())
		.collect()
}

fn gsub_subtables(font: &mut Font) -> TestResult<Vec<ot::LookupSubTable>> {
	let table = &font.require::<Gsub>()?.table;
	Ok(lookups_of(
		table.lookup_list.as_deref(),
		table.lookup_list2.as_deref(),
	))
}

fn gpos_subtables(font: &mut Font) -> TestResult<Vec<ot::LookupSubTable>> {
	let table = &font.require::<Gpos>()?.table;
	Ok(lookups_of(
		table.lookup_list.as_deref(),
		table.lookup_list2.as_deref(),
	))
}

/// `assert_layout_formats`, for the subtables of `test_layout_header_round_trip`.
fn assert_layout_formats(font: &mut Font, extended: bool) -> TestResult {
	let coverage_formats: [u16; 2] = if extended { [3, 4] } else { [1, 2] };
	let ligature_format = if extended { 2 } else { 1 };
	let mut checked = 0;
	let mut check_coverage = |font: &mut Font, coverage: &ot::Coverage| -> TestResult {
		let format = coverage.to_raw(font)?.format().ok_or("no format")?;
		assert!(coverage_formats.contains(&format), "{format}");
		checked += 1;
		Ok(())
	};
	for subtable in gsub_subtables(font)? {
		let ot::LookupSubTable::LigatureSubst(ligature) = subtable else {
			return Err("not a LigatureSubst".into());
		};
		let mut graph = WriterGraph::new(Tag::new(b"GSUB"));
		let mut writer = graph.root();
		let raw = ligature.to_raw(&mut writer, font)?;
		assert_eq!(raw.format(), Some(ligature_format));
		match &raw {
			ot::LigatureSubstRaw::Format1(raw) => {
				check_coverage(font, raw.coverage.as_ref().ok_or("no coverage")?)?;
			}
			ot::LigatureSubstRaw::Format2(raw) => {
				check_coverage(font, raw.coverage.as_ref().ok_or("no coverage")?)?;
			}
			ot::LigatureSubstRaw::Unknown(_) => return Err("unknown format".into()),
		}
	}
	for subtable in gpos_subtables(font)? {
		let coverage = match &subtable {
			ot::LookupSubTable::PairPos(ot::PairPos::Format1(pair)) => &pair.coverage,
			ot::LookupSubTable::PairPos(ot::PairPos::Format3(pair)) => &pair.coverage,
			_ => return Err("not a PairPos of a glyph pair format".into()),
		};
		check_coverage(font, coverage.as_ref().ok_or("no coverage")?)?;
	}
	assert_eq!(checked, 2);
	Ok(())
}

/// The types among those of `_EXPLICIT_LAYOUT_FORMATS` below a table (`nested_types`).
fn nested_types<T: OtTable>(table: &mut T) -> TestResult<Vec<&'static str>> {
	macro_rules! name_of {
		($any:ident, $($ty:ident),* $(,)?) => {
			$(if $any.is::<ot::$ty>() {
				return Some(stringify!($ty));
			})*
		};
	}
	fn name_of(any: &dyn std::any::Any) -> Option<&'static str> {
		name_of!(
			any,
			PairSet,
			PairValueRecord,
			PairSet2,
			PairValue2,
			EntryExitRecord,
			EntryExit2,
			MarkArray,
			MarkRecord,
			BaseArray,
			BaseRecord,
			MarkArray2,
			MarkRecord2,
			BaseArray2,
			BaseRecord2,
			LigatureArray,
			LigatureAttach,
			ComponentRecord,
			LigatureArray2,
			LigatureAttach2,
			ComponentRecord2,
			Mark2Array,
			Mark2Record,
			Mark2Array2,
			Mark2Record2,
		);
		None
	}
	let mut types = Vec::new();
	table.visit_mut(&mut |any| {
		types.extend(name_of(&*any));
		Ok(())
	})?;
	Ok(types)
}

fn convert_explicit<T: OtTable>(
	mut table: T,
	compact_format: u16,
	extended_format: u16,
	compact_nested: &[&str],
	extended_nested: &[&str],
) -> TestResult {
	convert_layout_formats(&mut table, true)?;
	assert_eq!(table.format(), Some(extended_format));
	assert_eq!(nested_types(&mut table)?, extended_nested);
	convert_layout_formats(&mut table, false)?;
	assert_eq!(table.format(), Some(compact_format));
	assert_eq!(nested_types(&mut table)?, compact_nested);
	Ok(())
}

fn boxed<T>(value: T) -> Option<Box<T>> {
	Some(Box::new(value))
}

#[test]
fn test_convert_explicit_layout_format() -> TestResult {
	convert_explicit(ot::SinglePos::Format1(Default::default()), 1, 3, &[], &[])?;
	convert_explicit(ot::SinglePos::Format2(Default::default()), 2, 4, &[], &[])?;
	convert_explicit(
		ot::PairPos::Format1(ot::PairPosFormat1 {
			pair_set: vec![boxed(ot::PairSet {
				pair_value_record: vec![Default::default()],
				..Default::default()
			})],
			..Default::default()
		}),
		1,
		3,
		&["PairSet", "PairValueRecord"],
		&["PairSet2", "PairValue2"],
	)?;
	convert_explicit(ot::PairPos::Format2(Default::default()), 2, 4, &[], &[])?;
	convert_explicit(
		ot::CursivePos::Format1(ot::CursivePosFormat1 {
			entry_exit_record: vec![Default::default()],
			..Default::default()
		}),
		1,
		2,
		&["EntryExitRecord"],
		&["EntryExit2"],
	)?;
	convert_explicit(
		ot::MarkBasePos::Format1(ot::MarkBasePosFormat1 {
			mark_array: boxed(ot::MarkArray {
				mark_record: vec![Default::default()],
				..Default::default()
			}),
			base_array: boxed(ot::BaseArray {
				base_record: vec![Default::default()],
				..Default::default()
			}),
			..Default::default()
		}),
		1,
		2,
		&["MarkArray", "MarkRecord", "BaseArray", "BaseRecord"],
		&["MarkArray2", "MarkRecord2", "BaseArray2", "BaseRecord2"],
	)?;
	convert_explicit(
		ot::MarkLigPos::Format1(ot::MarkLigPosFormat1 {
			mark_array: boxed(ot::MarkArray {
				mark_record: vec![Default::default()],
				..Default::default()
			}),
			ligature_array: boxed(ot::LigatureArray {
				ligature_attach: vec![boxed(ot::LigatureAttach {
					component_record: vec![Default::default()],
					..Default::default()
				})],
				..Default::default()
			}),
			..Default::default()
		}),
		1,
		2,
		&[
			"MarkArray",
			"MarkRecord",
			"LigatureArray",
			"LigatureAttach",
			"ComponentRecord",
		],
		&[
			"MarkArray2",
			"MarkRecord2",
			"LigatureArray2",
			"LigatureAttach2",
			"ComponentRecord2",
		],
	)?;
	convert_explicit(
		ot::MarkMarkPos::Format1(ot::MarkMarkPosFormat1 {
			mark1_array: boxed(ot::MarkArray {
				mark_record: vec![Default::default()],
				..Default::default()
			}),
			mark2_array: boxed(ot::Mark2Array {
				mark2_record: vec![Default::default()],
				..Default::default()
			}),
			..Default::default()
		}),
		1,
		2,
		&["MarkArray", "MarkRecord", "Mark2Array", "Mark2Record"],
		&["MarkArray2", "MarkRecord2", "Mark2Array2", "Mark2Record2"],
	)?;
	convert_explicit(
		ot::ReverseChainSingleSubst::Format1(Default::default()),
		1,
		2,
		&[],
		&[],
	)?;
	Ok(())
}

/// `assert_contextual_layout_attributes` for a rule of a plain subtable: its lookup count and
/// record, and its input.
macro_rules! check_rule {
	($table:expr, $set:ident, $rule:ident, $count:ident, $record:ident, $input:ident) => {{
		let set = $table.$set.iter().flatten().next().ok_or("no rule set")?;
		let rule = set.$rule[0].as_ref().ok_or("no rule")?;
		check_record!(rule, $count, $record);
		assert!(!rule.$input.is_empty());
		rule
	}};
}

/// The lookup count and the lookup records, whose type the field already says. The count is
/// only set when read from a file; otherwise it comes from the records when compiling.
macro_rules! check_record {
	($rule:expr, $count:ident, $record:ident) => {
		assert_eq!($rule.$record.len(), 1);
		assert!($rule.$count.is_none_or(|count| count == 1));
	};
}

/// `check_rule!` for a chaining subtable, which also has a backtrack and a look-ahead.
macro_rules! check_chain_rule {
	(
		$table:expr, $set:ident, $rule:ident, $count:ident, $record:ident,
		$backtrack:ident, $input:ident, $look_ahead:ident
	) => {{
		let rule = check_rule!($table, $set, $rule, $count, $record, $input);
		assert!(!rule.$backtrack.is_empty());
		assert!(!rule.$look_ahead.is_empty());
	}};
}

/// `assert_contextual_layout_attributes`, and that the format is that of the attributes.
fn assert_contextual_attributes(
	subtable: &ot::LookupSubTable,
	format: u16,
	chaining: bool,
	extended: bool,
) -> TestResult {
	let expected = if extended { format + 3 } else { format };
	use ot::LookupSubTable as St;
	match subtable {
		St::ContextSubst(table) => {
			assert!(!chaining);
			assert_eq!(table.format(), Some(expected));
			match table {
				ot::ContextSubst::Format1(t) => {
					check_rule!(
						t,
						sub_rule_set,
						sub_rule,
						subst_count,
						subst_lookup_record,
						input
					);
				}
				ot::ContextSubst::Format2(t) => {
					check_rule!(
						t,
						sub_class_set,
						sub_class_rule,
						subst_count,
						subst_lookup_record,
						class
					);
				}
				ot::ContextSubst::Format3(t) => {
					check_record!(t, subst_count, subst_lookup_record);
				}
				ot::ContextSubst::Format4(t) => {
					check_rule!(
						t,
						seq_rule_set,
						seq_rule,
						seq_lookup_count,
						seq_lookup_record,
						input_sequence
					);
				}
				ot::ContextSubst::Format5(t) => {
					check_rule!(
						t,
						class_seq_rule_set,
						class_seq_rule,
						seq_lookup_count,
						seq_lookup_record,
						input_sequence
					);
				}
				ot::ContextSubst::Format6(t) => {
					check_record!(t, seq_lookup_count, seq_lookup_record);
				}
				ot::ContextSubst::Unknown(_) => return Err("unknown format".into()),
			}
		}
		St::ContextPos(table) => {
			assert!(!chaining);
			assert_eq!(table.format(), Some(expected));
			match table {
				ot::ContextPos::Format1(t) => {
					check_rule!(
						t,
						pos_rule_set,
						pos_rule,
						pos_count,
						pos_lookup_record,
						input
					);
				}
				ot::ContextPos::Format2(t) => {
					check_rule!(
						t,
						pos_class_set,
						pos_class_rule,
						pos_count,
						pos_lookup_record,
						class
					);
				}
				ot::ContextPos::Format3(t) => {
					check_record!(t, pos_count, pos_lookup_record);
				}
				ot::ContextPos::Format4(t) => {
					check_rule!(
						t,
						seq_rule_set,
						seq_rule,
						seq_lookup_count,
						seq_lookup_record,
						input_sequence
					);
				}
				ot::ContextPos::Format5(t) => {
					check_rule!(
						t,
						class_seq_rule_set,
						class_seq_rule,
						seq_lookup_count,
						seq_lookup_record,
						input_sequence
					);
				}
				ot::ContextPos::Format6(t) => {
					check_record!(t, seq_lookup_count, seq_lookup_record);
				}
				ot::ContextPos::Unknown(_) => return Err("unknown format".into()),
			}
		}
		St::ChainContextSubst(table) => {
			assert!(chaining);
			assert_eq!(table.format(), Some(expected));
			match table {
				ot::ChainContextSubst::Format1(t) => check_chain_rule!(
					t,
					chain_sub_rule_set,
					chain_sub_rule,
					subst_count,
					subst_lookup_record,
					backtrack,
					input,
					look_ahead
				),
				ot::ChainContextSubst::Format2(t) => check_chain_rule!(
					t,
					chain_sub_class_set,
					chain_sub_class_rule,
					subst_count,
					subst_lookup_record,
					backtrack,
					input,
					look_ahead
				),
				ot::ChainContextSubst::Format4(t) => check_chain_rule!(
					t,
					chained_seq_rule_set,
					chained_seq_rule,
					seq_lookup_count,
					seq_lookup_record,
					backtrack_sequence,
					input_sequence,
					look_ahead_sequence
				),
				ot::ChainContextSubst::Format5(t) => check_chain_rule!(
					t,
					chained_class_seq_rule_set,
					chained_class_seq_rule,
					seq_lookup_count,
					seq_lookup_record,
					backtrack_sequence,
					input_sequence,
					look_ahead_sequence
				),
				_ => return Err("unexpected format".into()),
			}
		}
		St::ChainContextPos(table) => {
			assert!(chaining);
			assert_eq!(table.format(), Some(expected));
			match table {
				ot::ChainContextPos::Format1(t) => check_chain_rule!(
					t,
					chain_pos_rule_set,
					chain_pos_rule,
					pos_count,
					pos_lookup_record,
					backtrack,
					input,
					look_ahead
				),
				ot::ChainContextPos::Format2(t) => check_chain_rule!(
					t,
					chain_pos_class_set,
					chain_pos_class_rule,
					pos_count,
					pos_lookup_record,
					backtrack,
					input,
					look_ahead
				),
				ot::ChainContextPos::Format4(t) => check_chain_rule!(
					t,
					chained_seq_rule_set,
					chained_seq_rule,
					seq_lookup_count,
					seq_lookup_record,
					backtrack_sequence,
					input_sequence,
					look_ahead_sequence
				),
				ot::ChainContextPos::Format5(t) => check_chain_rule!(
					t,
					chained_class_seq_rule_set,
					chained_class_seq_rule,
					seq_lookup_count,
					seq_lookup_record,
					backtrack_sequence,
					input_sequence,
					look_ahead_sequence
				),
				_ => return Err("unexpected format".into()),
			}
		}
		_ => return Err("not a contextual subtable".into()),
	}
	Ok(())
}

/// The formats of the subtables of the five lookups of the fixture: formats 1, 2 and 3 of the
/// plain ones, then formats 1 and 2 of the chaining ones.
const CONTEXTUAL_CASES: [(usize, u16, bool); 5] = [
	(0, 1, false),
	(1, 2, false),
	(2, 3, false),
	(3, 1, true),
	(4, 2, true),
];

macro_rules! convert_contextual_table {
	($font:ident, $table:ty, $tag:literal) => {
		for (index, compact_format, chaining) in CONTEXTUAL_CASES {
			for extended in [true, false] {
				let table = $font.require::<$table>()?;
				let subtable = table.lookup_list.as_mut().ok_or("no LookupList")?.lookup[index]
					.as_mut()
					.ok_or("no Lookup")?
					.sub_table[0]
					.as_mut()
					.ok_or("no subtable")?;
				convert_layout_formats(&mut **subtable, extended)?;
				assert_contextual_attributes(subtable, compact_format, chaining, extended)?;
				// `compile_layout_subtable`: the subtable is part of the table here.
				$font.table_data(Tag::new($tag))?;
			}
		}
	};
}

#[test]
fn test_convert_contextual_layout_formats() -> TestResult {
	let mut font = Font::new();
	font.import_ttx(&fixture("contextual_formats"))?;
	convert_contextual_table!(font, Gsub, b"GSUB");
	convert_contextual_table!(font, Gpos, b"GPOS");
	Ok(())
}

/// `assert_contextual_formats`.
fn assert_contextual_formats(font: &mut Font, extended: bool) -> TestResult {
	use ot::LookupSubTable as St;
	let plain: Vec<Option<u16>> = (if extended { 4..7 } else { 1..4 }).map(Some).collect();
	let chain: Vec<Option<u16>> = (if extended { 4..6 } else { 1..3 }).map(Some).collect();
	let gsub = gsub_subtables(font)?;
	let gpos = gpos_subtables(font)?;
	let context_subst: Vec<_> = gsub
		.iter()
		.filter_map(|s| match s {
			St::ContextSubst(t) => Some(t.format()),
			_ => None,
		})
		.collect();
	assert_eq!(context_subst, plain);
	let chain_subst: Vec<_> = gsub
		.iter()
		.filter_map(|s| match s {
			St::ChainContextSubst(t) => Some(t.format()),
			_ => None,
		})
		.collect();
	assert_eq!(chain_subst, chain);
	let context_pos: Vec<_> = gpos
		.iter()
		.filter_map(|s| match s {
			St::ContextPos(t) => Some(t.format()),
			_ => None,
		})
		.collect();
	assert_eq!(context_pos, plain);
	let chain_pos: Vec<_> = gpos
		.iter()
		.filter_map(|s| match s {
			St::ChainContextPos(t) => Some(t.format()),
			_ => None,
		})
		.collect();
	assert_eq!(chain_pos, chain);
	Ok(())
}

#[test]
fn test_contextual_layout_end_to_end_round_trip() -> TestResult {
	let mut font = test_ttf_with("contextual_layout")?;

	upper_tables(&mut font, &tables(&["GSUB", "GPOS"]))?;
	let mut font = save_and_read(&mut font)?;
	assert_contextual_formats(&mut font, true)?;

	lower_tables(&mut font, &tables(&["GSUB", "GPOS"]))?;
	let mut font = save_and_read(&mut font)?;
	assert_contextual_formats(&mut font, false)?;
	Ok(())
}

#[test]
fn test_gdef_end_to_end_round_trip() -> TestResult {
	let mut font = test_ttf_with("gdef")?;

	upper_tables(&mut font, &tables(&["GDEF"]))?;
	let table = font.require::<Gdef>()?;
	assert_eq!(table.version, 0x0001_0004);
	assert!(table.glyph_class_def.is_none());
	assert!(table.glyph_class_def2.is_some());
	assert!(table.lig_caret_list.is_none());
	assert!(table.lig_caret_list2.is_some());

	let mut font = save_and_read(&mut font)?;
	let table = font.require::<Gdef>()?;
	assert_eq!(table.version, 0x0001_0004);
	assert!(table.glyph_class_def2.is_some());
	assert!(table.lig_caret_list2.is_some());

	lower_tables(&mut font, &tables(&["GDEF"]))?;
	let table = font.require::<Gdef>()?;
	assert_eq!(table.version, 0x0001_0000);
	assert!(table.glyph_class_def.is_some());
	assert!(table.glyph_class_def2.is_none());
	assert!(table.lig_caret_list.is_some());
	assert!(table.lig_caret_list2.is_none());

	let mut font = save_and_read(&mut font)?;
	let table = font.require::<Gdef>()?;
	assert_eq!(table.version, 0x0001_0000);
	assert!(table.glyph_class_def.is_some());
	assert!(table.lig_caret_list.is_some());
	Ok(())
}

fn base_coord_format(font: &mut Font) -> TestResult<Option<u16>> {
	let coord = font
		.require::<Base>()?
		.table
		.horiz_axis
		.as_ref()
		.ok_or("no HorizAxis")?
		.base_script_list
		.as_ref()
		.ok_or("no BaseScriptList")?
		.base_script_record[0]
		.base_script
		.as_ref()
		.ok_or("no BaseScript")?
		.base_values
		.as_ref()
		.ok_or("no BaseValues")?
		.base_coord[0]
		.as_ref()
		.ok_or("no BaseCoord")?;
	Ok(coord.format())
}

#[test]
fn test_base_end_to_end_round_trip() -> TestResult {
	let mut font = test_ttf_with("base")?;

	upper_tables(&mut font, &tables(&["BASE"]))?;
	assert_eq!(base_coord_format(&mut font)?, Some(4));
	let mut font = save_and_read(&mut font)?;
	assert_eq!(base_coord_format(&mut font)?, Some(4));

	lower_tables(&mut font, &tables(&["BASE"]))?;
	assert_eq!(base_coord_format(&mut font)?, Some(2));
	let mut font = save_and_read(&mut font)?;
	assert_eq!(base_coord_format(&mut font)?, Some(2));
	Ok(())
}

#[test]
fn test_jstf_end_to_end_round_trip() -> TestResult {
	let mut font = test_ttf()?;
	let period = font.glyph_id("period")?;
	font.insert(Box::new(Jstf {
		table: ot::JSTF {
			version: 0x0001_0000,
			jstf_script_count: Some(1),
			jstf_script_record: vec![ot::JstfScriptRecord {
				jstf_script_tag: Tag::new(b"latn"),
				jstf_script: boxed(ot::JstfScript {
					extender_glyph: boxed(ot::ExtenderGlyph {
						glyph_count: Some(1),
						extender_glyph: vec![period],
					}),
					def_jstf_lang_sys: None,
					jstf_lang_sys_count: Some(0),
					jstf_lang_sys_record: Vec::new(),
				}),
			}],
			..Default::default()
		},
	}));

	upper_tables(&mut font, &tables(&["JSTF"]))?;
	let table = font.require::<Jstf>()?;
	assert_eq!(table.version, 0x0001_0001);
	assert!(table.jstf_script_record.is_empty());
	// The types of the fields are `JstfScriptRecord2`, `JstfScript2` and `ExtenderGlyph2`.
	let script: &ot::JstfScript2 = table.jstf_script_record2[0]
		.jstf_script
		.as_ref()
		.ok_or("no JstfScript2")?;
	let _: &ot::ExtenderGlyph2 = script.extender_glyph.as_ref().ok_or("no ExtenderGlyph2")?;

	let mut font = save_and_read(&mut font)?;
	let table = font.require::<Jstf>()?;
	let script = table.jstf_script_record2[0]
		.jstf_script
		.as_ref()
		.ok_or("no JstfScript2")?;
	assert_eq!(
		script
			.extender_glyph
			.as_ref()
			.ok_or("no ExtenderGlyph2")?
			.extender_glyph,
		[period]
	);

	lower_tables(&mut font, &tables(&["JSTF"]))?;
	let table = font.require::<Jstf>()?;
	assert_eq!(table.version, 0x0001_0000);
	let script: &ot::JstfScript = table.jstf_script_record[0]
		.jstf_script
		.as_ref()
		.ok_or("no JstfScript")?;
	let _: &ot::ExtenderGlyph = script.extender_glyph.as_ref().ok_or("no ExtenderGlyph")?;

	let mut font = save_and_read(&mut font)?;
	let table = font.require::<Jstf>()?;
	let script = table.jstf_script_record[0]
		.jstf_script
		.as_ref()
		.ok_or("no JstfScript")?;
	assert_eq!(
		script
			.extender_glyph
			.as_ref()
			.ok_or("no ExtenderGlyph")?
			.extender_glyph,
		[period]
	);
	Ok(())
}

fn assert_companions(font: &Font, present: [&[u8; 4]; 5], absent: [&[u8; 4]; 5]) {
	for tag in present {
		assert!(font.contains(Tag::new(tag)), "{:?}", tag_names(font));
	}
	for tag in absent {
		assert!(!font.contains(Tag::new(tag)), "{:?}", tag_names(font));
	}
}

#[test]
fn test_round_trip_companion_tables() -> TestResult {
	let mut font = test_ttf()?;
	let mut font = save_and_read(&mut font)?;
	let expected_order = font.glyph_order()?;
	let expected_paths = draw_paths(&mut font)?;

	upper_tables(&mut font, &ConvertOptions::default())?;
	assert_companions(&font, COMPANIONS_UPPER, COMPANIONS_LOWER);

	let mut font = save_and_read(&mut font)?;
	assert_eq!(draw_paths(&mut font)?, expected_paths);

	lower_tables(&mut font, &ConvertOptions::default())?;
	assert_companions(&font, COMPANIONS_LOWER, COMPANIONS_UPPER);

	let mut font = save_and_read(&mut font)?;
	assert_eq!(font.glyph_order()?.names(), expected_order.names());
	assert_eq!(draw_paths(&mut font)?, expected_paths);
	Ok(())
}

/// `big_glyph`.
fn big_glyph() -> Glyph {
	let mut coordinates = vec![(0.0, 0.0)];
	for i in 1..12000u32 {
		coordinates.push((f64::from((i * 977) % 20000), f64::from((i * 1013) % 20000)));
	}
	Glyph {
		number_of_contours: 1,
		end_pts_of_contours: vec![coordinates.len() as i64 - 1],
		flags: vec![FLAG_ON_CURVE; coordinates.len()],
		coordinates,
		program: Some(Program::default()),
		..Default::default()
	}
}

#[test]
#[allow(non_snake_case)]
fn test_head_written_after_LOCA_with_long_loca() -> TestResult {
	// A beyond-64k font with long loca must write 'head' after the uppercase 'LOCA'
	// companion (head.dependencies), otherwise head freezes a stale indexToLocFormat
	// before LOCA is compiled and the saved font is corrupt. Build a font whose glyf
	// exceeds the short-loca limit, uppercase it, and round-trip.
	let mut font = Font::from_ttx(&fixture("head_after_loca"))?;
	let order = font.glyph_order()?;
	let a = order.id("A").ok_or("no A")?;
	for name in ["A", "B", "C"] {
		let gid = order.id(name).ok_or("no glyph")?;
		font.require::<Glyf>()?.glyphs.insert(gid, big_glyph());
	}

	let glyf_data = font.table_data(Tag::new(b"glyf"))?;
	assert!(glyf_data.len() > 0x1FFFE, "test font must need long loca");
	let expected_coords = font.require::<Glyf>()?.glyphs[&a].coordinates.clone();

	upper_tables(&mut font, &all_no_validate())?;
	assert!(font.contains(Tag::new(b"LOCA")));

	let mut font = save_and_read(&mut font)?;

	// Without the head.dependencies fix this is 0 (stale), making the reloaded loca
	// offsets garbage.
	assert_eq!(font.require::<Head>()?.index_to_loc_format, 1);
	// 'A' is the second glyph; names are kept (< 64k glyphs). Its outline must
	// survive the round-trip intact.
	assert_eq!(
		font.require::<Glyf>()?.glyphs[&a].coordinates,
		expected_coords
	);
	Ok(())
}

/// `CmapSubtable.newSubtable(4)`, without `language`.
fn subtable(
	format: u16,
	platform_id: u16,
	plat_enc_id: u16,
	language: u32,
	cmap: &[(u32, u32)],
) -> CmapSubtable {
	let mut subtable = CmapSubtable::new(format);
	subtable.platform_id = platform_id;
	subtable.plat_enc_id = plat_enc_id;
	subtable.language = Some(language);
	*subtable.cmap_mut() = cmap.iter().copied().collect();
	subtable
}

fn uvs_subtable(format: u16, uvs: &[(u32, Option<u32>)]) -> CmapSubtable {
	let mut subtable = subtable(format, 0, 5, 0xFF, &[]);
	subtable.uvs_dict = Some([(0xFE00, uvs.to_vec())].into_iter().collect());
	subtable
}

fn cmap_table(tables: Vec<CmapSubtable>) -> Cmap {
	Cmap {
		table_version: 0,
		tables,
	}
}

/// `font[tag]`.
fn character_map<'a>(font: &'a mut Font, tag: &str) -> TestResult<&'a mut Cmap> {
	Ok(if tag == "cmap" {
		font.require::<Cmap>()?
	} else {
		&mut font.require::<Dmap>()?.0
	})
}

fn char_map(pairs: &[(u32, u32)]) -> CharMap {
	pairs.iter().copied().collect()
}

fn uvs_dict(uvs: &[(u32, Option<u32>)]) -> UvsDict {
	[(0xFE00, uvs.to_vec())].into_iter().collect()
}

#[test]
fn test_upper_tables_replaces_bmp_format4_cmap_with_format12() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(glyph_order(0x10001, "highGlyph"));
	font.insert(Box::new(cmap_table(vec![subtable(
		4,
		3,
		1,
		0,
		&[(0x41, 0x10000)],
	)])));

	upper_tables(&mut font, &ConvertOptions::default())?;

	let cmap = font.require::<Cmap>()?;
	assert_eq!(cmap.tables.len(), 1);
	assert_eq!(cmap.tables[0].format, 12);
	assert_eq!(cmap.tables[0].platform_id, 3);
	assert_eq!(cmap.tables[0].plat_enc_id, 10);
	assert_eq!(*cmap.tables[0].cmap, char_map(&[(0x41, 0x10000)]));
	font.table_data(Tag::new(b"cmap"))?;
	Ok(())
}

#[test]
fn test_upper_tables_preserves_nonunicode_format4() -> TestResult {
	for tag in ["cmap", "DMAP"] {
		for encoding in [0, 2] {
			for unicode_companion in [false, true] {
				let mut font = Font::new();
				font.set_glyph_order(glyph_order(0x10001, "glyph65536"));
				let legacy = subtable(4, 3, encoding, 0, &[(0xF041, 1)]);
				let mut subtables = vec![legacy.clone()];
				if unicode_companion {
					subtables.push(subtable(4, 3, 1, 0, &[(0x42, 0x10000)]));
				}
				if tag == "cmap" {
					font.insert(Box::new(cmap_table(subtables)));
				} else {
					font.insert(Box::new(Dmap(cmap_table(subtables))));
				}

				upper_tables(&mut font, &ConvertOptions::default())?;

				let map = character_map(&mut font, tag)?;
				assert!(map.tables.iter().any(|s| s.format == 4
					&& s.plat_enc_id == encoding
					&& *s.cmap == *legacy.cmap));
				let unicode_tables: Vec<_> = map.tables.iter().filter(|s| s.format == 12).collect();
				if unicode_companion {
					assert_eq!(unicode_tables.len(), 1);
					assert_eq!(*unicode_tables[0].cmap, char_map(&[(0x42, 0x10000)]));
				} else {
					assert!(unicode_tables.is_empty());
				}
				let tag = Tag::from_str_padded(tag).ok_or("not a tag")?;
				let data = font.table_data(tag)?;
				let reloaded = if tag == *b"cmap" {
					Cmap::decompile(&data, &mut font)?
				} else {
					Dmap::decompile(&data, &mut font)?.0
				};
				let reloaded = reloaded
					.getcmap(3, encoding)
					.ok_or("no subtable")?
					.cmap
					.clone();
				assert_eq!(*reloaded, *legacy.cmap);
			}
		}
	}
	Ok(())
}

/// `normalized_uvs_dict`.
fn normalized_uvs_dict(uvs: &UvsDict) -> UvsDict {
	uvs.iter()
		.map(|(selector, entries)| {
			let mut entries = entries.clone();
			entries.sort_by_key(|item| (item.1.is_some(), item.0));
			(*selector, entries)
		})
		.collect()
}

#[test]
fn test_upper_tables_converts_high_uvs_format14_to_format15() -> TestResult {
	let mut font = Font::new();
	let mut order = glyph_order(0x10001, "highGlyph");
	order[1] = "lowGlyph".to_owned();
	font.set_glyph_order(order);
	let uvs = [(0x41, Some(0x10000)), (0x42, None)];
	font.insert(Box::new(cmap_table(vec![uvs_subtable(14, &uvs)])));

	upper_tables(&mut font, &ConvertOptions::default())?;

	let compiled = font.require::<Cmap>()?.tables[0].clone();
	assert_eq!(compiled.format, 15);
	assert_eq!(compiled.uvs_dict, Some(uvs_dict(&uvs)));
	let data = compiled.compile(&mut font)?;
	let mut cmap15 = CmapSubtable::new(15);
	cmap15.decompile(&data)?;
	assert_eq!(
		normalized_uvs_dict(cmap15.uvs_dict.as_ref().ok_or("no uvs")?),
		normalized_uvs_dict(compiled.uvs_dict.as_ref().ok_or("no uvs")?)
	);
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_upper_tables_converts_DMAP_formats() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(glyph_order(0x10001, "highGlyph"));
	font.insert(Box::new(Dmap(cmap_table(vec![
		subtable(4, 3, 1, 0, &[(0x41, 0x10000)]),
		uvs_subtable(14, &[(0x42, Some(0x10000))]),
	]))));

	upper_tables(&mut font, &ConvertOptions::default())?;

	let dmap = &font.require::<Dmap>()?.0;
	let mut formats: Vec<u16> = dmap.tables.iter().map(|t| t.format).collect();
	formats.sort();
	assert_eq!(formats, [12, 15]);
	let format12 = dmap.tables.iter().find(|t| t.format == 12).ok_or("no 12")?;
	assert_eq!(*format12.cmap, char_map(&[(0x41, 0x10000)]));
	let format15 = dmap.tables.iter().find(|t| t.format == 15).ok_or("no 15")?;
	assert_eq!(format15.uvs_dict, Some(uvs_dict(&[(0x42, Some(0x10000))])));
	font.table_data(Tag::new(b"DMAP"))?;
	Ok(())
}

#[test]
fn test_upper_tables_keeps_low_uvs_format14() -> TestResult {
	let mut font = Font::new();
	let mut order = glyph_order(0x10001, "highGlyph");
	order[1] = "lowGlyph".to_owned();
	font.set_glyph_order(order);
	font.insert(Box::new(cmap_table(vec![uvs_subtable(
		14,
		&[(0x41, Some(1)), (0x42, None)],
	)])));

	upper_tables(&mut font, &ConvertOptions::default())?;

	let subtable = font.require::<Cmap>()?.tables[0].clone();
	assert_eq!(font.require::<Cmap>()?.tables.len(), 1);
	assert_eq!(subtable.format, 14);
	subtable.compile(&mut font)?;
	Ok(())
}

#[test]
fn test_lower_tables_converts_format15_to_format14_when_safe() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(names(&[".notdef", "lowGlyph"]));
	let uvs = [(0x41, Some(1)), (0x42, None)];
	font.insert(Box::new(cmap_table(vec![uvs_subtable(15, &uvs)])));

	lower_tables(&mut font, &all_no_validate())?;

	let cmap = font.require::<Cmap>()?;
	assert_eq!(cmap.tables.len(), 1);
	assert_eq!(cmap.tables[0].format, 14);
	assert_eq!(cmap.tables[0].uvs_dict, Some(uvs_dict(&uvs)));
	let subtable = cmap.tables[0].clone();
	subtable.compile(&mut font)?;
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_lower_tables_converts_DMAP_format15_to_format14_when_safe() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(names(&[".notdef", "lowGlyph"]));
	let uvs = [(0x41, Some(1)), (0x42, None)];
	font.insert(Box::new(Dmap(cmap_table(vec![uvs_subtable(15, &uvs)]))));

	lower_tables(&mut font, &all_no_validate())?;

	let dmap = &font.require::<Dmap>()?.0;
	assert_eq!(dmap.tables.len(), 1);
	assert_eq!(dmap.tables[0].format, 14);
	assert_eq!(dmap.tables[0].uvs_dict, Some(uvs_dict(&uvs)));
	font.table_data(Tag::new(b"DMAP"))?;
	Ok(())
}

#[test]
fn test_lower_tables_rejects_high_format15_uvs() {
	let mut font = Font::new();
	font.set_glyph_order(glyph_order(0x10001, "highGlyph"));
	font.insert(Box::new(cmap_table(vec![uvs_subtable(
		15,
		&[(0x41, Some(0x10000))],
	)])));

	assert_error_contains(
		lower_tables(&mut font, &all_no_validate()),
		"format 15 glyph IDs",
	);
}

/// The `post` table of the tests.
fn post_table(
	format_type: f64,
	extra_names: Vec<String>,
	mapping: BTreeMap<String, String>,
) -> Post {
	let mut post = Post::default();
	post.format_type = format_type;
	post.extra_names = Some(extra_names);
	post.mapping = Some(mapping);
	post
}

#[test]
fn test_upper_tables_drops_beyond64k_post_glyph_names() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(glyph_order(0x10001, "highGlyph"));
	font.insert(Box::new(post_table(
		2.0,
		names(&["highGlyph"]),
		[("highGlyph".to_owned(), "highGlyph".to_owned())]
			.into_iter()
			.collect(),
	)));

	upper_tables(&mut font, &ConvertOptions::default())?;

	let post = font.require::<Post>()?;
	assert_eq!(post.format_type, 3.0);
	assert_eq!(post.extra_names, Some(Vec::new()));
	assert_eq!(post.mapping, Some(BTreeMap::new()));
	font.table_data(Tag::new(b"post"))?;
	Ok(())
}

#[test]
fn test_upper_tables_drops_post_glyph_names_at_glyph_count_boundary() -> TestResult {
	// post 2.0 numberOfGlyphs is uint16, so a font with 0x10000 (65536) glyphs
	// cannot keep version 2.0 even though every glyph id (0..0xFFFF) still fits:
	// the count overflows. The drop must trigger at > 0xFFFF, not > 0x10000.
	for (num_glyphs, dropped) in [(0xFFFF, false), (0x10000, true), (0x10001, true)] {
		let mut font = Font::new();
		let mut order = vec![".notdef".to_owned()];
		order.extend((1..num_glyphs).map(|i| format!("glyph{i}")));
		assert_eq!(order.len() as u32, num_glyphs);
		font.set_glyph_order(order);
		font.insert(Box::new(post_table(2.0, Vec::new(), BTreeMap::new())));

		upper_tables(&mut font, &ConvertOptions::default())?;

		assert_eq!(
			font.require::<Post>()?.format_type,
			if dropped { 3.0 } else { 2.0 }
		);
	}
	Ok(())
}

#[test]
fn test_accepts_either_table_spelling() -> TestResult {
	for tag in ["glyf", "GLYF"] {
		let mut font = Font::new();
		font.insert(Box::new(Glyf::default()));

		upper_tables(&mut font, &no_validate(&[tag]))?;

		assert!(font.contains(Tag::new(b"GLYF")));
		assert!(!font.contains(Tag::new(b"glyf")));
	}
	Ok(())
}

#[test]
fn test_validate_rejects_mixed_family_before_mutation() {
	let mut font = Font::new();
	font.insert(Box::new(Glyf::default()));
	font.insert(Box::new(Loca::default()));

	assert_error_contains(
		upper_tables(&mut font, &tables(&["glyf"])),
		"mixed beyond-64k table family",
	);

	assert!(font.contains(Tag::new(b"glyf")));
	assert!(font.contains(Tag::new(b"loca")));
	assert!(!font.contains(Tag::new(b"GLYF")));
}

#[test]
fn test_overwrite_replaces_existing_destination() -> TestResult {
	let mut font = Font::new();
	// `padding` stands in for the `marker` attribute.
	font.insert(Box::new(Glyf {
		padding: 2,
		..Default::default()
	}));
	font.insert(Box::new(Glyf {
		padding: 4,
		extended: true,
		..Default::default()
	}));

	upper_tables(
		&mut font,
		&ConvertOptions {
			tables: Some(&["glyf"]),
			validate: false,
			overwrite: true,
			..Default::default()
		},
	)?;

	assert_eq!(font.require::<Glyf>()?.padding, 2);
	assert!(font.require::<Glyf>()?.extended);
	assert!(!font.contains(Tag::new(b"glyf")));
	Ok(())
}

#[test]
fn test_duplicate_destination_requires_overwrite() {
	let mut font = Font::new();
	font.insert(Box::new(Glyf::default()));
	font.insert(Box::new(Glyf {
		extended: true,
		..Default::default()
	}));

	assert_error_contains(
		upper_tables(&mut font, &no_validate(&["glyf"])),
		"overwrite=True",
	);
}

#[test]
fn test_missing_tables_are_ignored_by_default() -> TestResult {
	upper_tables(&mut Font::new(), &tables(&["glyf"]))?;
	Ok(())
}

#[test]
fn test_missing_tables_can_be_rejected() {
	assert_error_contains(
		upper_tables(
			&mut Font::new(),
			&ConvertOptions {
				tables: Some(&["glyf"]),
				ignore_missing: false,
				..Default::default()
			},
		),
		"KeyError",
	);
}

#[test]
fn test_cli_help_documents_usage() -> TestResult {
	assert_eq!(main(&["--help"])?, 0);
	let help_text = help();
	assert!(help_text.contains("fonttools ttLib.beyond64k upper input.ttf -o upper.ttf"));
	assert!(help_text.contains("fonttools ttLib.beyond64k lower upper.ttf -o lower.ttf"));
	assert!(help_text.contains("--no-validate"));
	assert!(help_text.contains("--overwrite"));
	Ok(())
}

#[test]
fn test_lower_rejects_large_glyph_count() {
	let mut font = Font::new();
	let mut maxp = Maxp::new_as(Tag::new(b"MAXP"));
	maxp.num_glyphs = 0x10000;
	font.insert(Box::new(maxp));

	assert_error_contains(
		lower_tables(&mut font, &no_validate(&["maxp"])),
		"does not fit",
	);
}

#[test]
fn test_lower_rejects_cubic_glyph() {
	let mut font = Font::new();
	font.set_glyph_order(names(&[".notdef"]));
	let mut glyf = Glyf::new_as(Tag::new(b"GLYF"));
	glyf.glyphs.insert(
		0,
		Glyph {
			number_of_contours: 1,
			flags: vec![FLAG_CUBIC],
			..Default::default()
		},
	);
	font.insert(Box::new(glyf));

	assert_error_contains(
		lower_tables(&mut font, &no_validate(&["glyf"])),
		"cubic outlines",
	);
}

fn glyph_i(font: &mut Font) -> TestResult<&mut Glyph> {
	let gid = font.glyph_id("I")?;
	let glyph = font
		.require::<Glyf>()?
		.glyphs
		.get_mut(&gid)
		.ok_or("no glyph I")?;
	// `glyf["I"]` expands the glyph, which is then written from its fields.
	glyph.data = None;
	Ok(glyph)
}

#[test]
fn test_lower_allows_cubic_flag_on_oncurve_points() -> TestResult {
	for recalc_bboxes in [false, true] {
		let options = ReadOptions {
			recalc_bboxes,
			..Default::default()
		};
		let mut font = Font::read(std::fs::read(upstream("ttLib/data/I.ttf"))?, options)?;
		upper_tables(&mut font, &ConvertOptions::default())?;
		for flag in &mut glyph_i(&mut font)?.flags {
			if *flag & FLAG_ON_CURVE != 0 {
				*flag |= FLAG_CUBIC;
			}
		}
		let mut font = Font::read(font.write(&WriteOptions::default())?, options)?;
		let expected = draw_paths(&mut font)?;
		assert!(
			glyph_i(&mut font)?
				.flags
				.iter()
				.any(|f| f & FLAG_CUBIC != 0)
		);

		lower_tables(&mut font, &ConvertOptions::default())?;

		let mut font = save_and_read(&mut font)?;
		assert!(
			!glyph_i(&mut font)?
				.flags
				.iter()
				.any(|f| f & FLAG_CUBIC != 0)
		);
		assert_eq!(draw_paths(&mut font)?, expected);
	}
	Ok(())
}

#[test]
fn test_lower_rejects_hinted_oncurve_cubic_flags() -> TestResult {
	let mut font = Font::read(
		std::fs::read(upstream("ttLib/data/I.ttf"))?,
		ReadOptions::default(),
	)?;
	upper_tables(&mut font, &ConvertOptions::default())?;
	let glyph = glyph_i(&mut font)?;
	assert!(glyph.flags[0] & FLAG_ON_CURVE != 0 && glyph.flags[1] & FLAG_ON_CURVE != 0);
	glyph.flags[0] |= FLAG_CUBIC;
	glyph.flags[1] |= FLAG_CUBIC;
	glyph.program = Some(Program::from_assembly(&[
		"PUSHB[ ]",
		"2",
		"SLOOP[ ]",
		"PUSHB[ ]",
		"0",
		"1",
		"FLIPPT[ ]",
	])?);

	assert_error_contains(
		lower_tables(&mut font, &ConvertOptions::default()),
		"hinted CUBIC flags",
	);
	assert!(font.contains(Tag::new(b"GLYF")) && !font.contains(Tag::new(b"glyf")));
	Ok(())
}

#[test]
fn test_lower_rejects_large_component_glyph_id() {
	let mut font = Font::new();
	let mut order = vec![".notdef".to_owned()];
	order.extend((0..0x10000).map(|i| format!("glyph{i}")));
	font.set_glyph_order(order);
	let mut glyf = Glyf::new_as(Tag::new(b"GLYF"));
	glyf.glyphs.insert(
		0,
		Glyph {
			number_of_contours: -1,
			components: vec![GlyphComponent {
				// "glyph65535".
				glyph: 0x10000,
				flags: 0,
				placement: ComponentPlacement::Offset {
					x: Number::Int(0),
					y: Number::Int(0),
				},
				transform: None,
			}],
			..Default::default()
		},
	);
	font.insert(Box::new(glyf));

	assert_error_contains(
		lower_tables(&mut font, &no_validate(&["glyf"])),
		"does not fit",
	);
}

#[test]
fn test_vorg_version_conversion() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(names(&[".notdef", "glyph"]));
	font.insert(Box::new(Vorg {
		major_version: 1,
		minor_version: 0,
		default_vert_origin_y: 880,
		num_vert_origin_y_metrics: 1,
		v_origin_records: [(1, 861)].into_iter().collect(),
	}));

	upper_tables(&mut font, &tables(&["VORG"]))?;
	let vorg = font.require::<Vorg>()?;
	assert_eq!((vorg.major_version, vorg.minor_version), (2, 0));

	lower_tables(&mut font, &tables(&["VORG"]))?;
	let vorg = font.require::<Vorg>()?;
	assert_eq!((vorg.major_version, vorg.minor_version), (1, 0));
	Ok(())
}

#[test]
fn test_lower_rejects_large_vorg_glyph_id() {
	let mut font = Font::new();
	// `font.getGlyphID = lambda glyph_name: 0x10000`.
	font.set_glyph_order(glyph_order(0x10001, "highGlyph"));
	font.insert(Box::new(Vorg {
		major_version: 2,
		minor_version: 0,
		default_vert_origin_y: 880,
		num_vert_origin_y_metrics: 1,
		v_origin_records: [(0x10000, 861)].into_iter().collect(),
	}));

	assert_error_contains(lower_tables(&mut font, &tables(&["VORG"])), "does not fit");
}

/// `buildCOLR` of a base glyph 1 with a paint of a format 10 or 33.
fn colr_font(paint: ot::Paint) -> Colr {
	Colr {
		version: 1,
		color_layers: None,
		table: Some(ot::COLR {
			version: 1,
			base_glyph_list: boxed(ot::BaseGlyphList {
				base_glyph_count: Some(1),
				base_glyph_paint_record: vec![ot::BaseGlyphPaintRecord {
					base_glyph: 1,
					paint: boxed(paint),
				}],
			}),
			..Default::default()
		}),
	}
}

fn solid_paint() -> Option<Box<ot::Paint>> {
	boxed(ot::Paint::Format2(ot::PaintFormat2 {
		palette_index: 0,
		alpha: 1.0,
	}))
}

fn colr_paint_format(font: &mut Font) -> TestResult<Option<u16>> {
	let colr = font.require::<Colr>()?;
	Ok(colr
		.table
		.as_ref()
		.ok_or("no table")?
		.base_glyph_list
		.as_ref()
		.ok_or("no list")?
		.base_glyph_paint_record[0]
		.paint
		.as_ref()
		.ok_or("no paint")?
		.format())
}

#[test]
fn test_colr_paint_glyph_conversion() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(names(&[".notdef", "base", "outline"]));
	font.insert(Box::new(colr_font(ot::Paint::Format10(
		ot::PaintFormat10 {
			paint: solid_paint(),
			glyph: 2,
		},
	))));

	upper_tables(&mut font, &tables(&["COLR"]))?;
	assert_eq!(colr_paint_format(&mut font)?, Some(33));

	lower_tables(&mut font, &tables(&["COLR"]))?;
	assert_eq!(colr_paint_format(&mut font)?, Some(10));
	Ok(())
}

#[test]
fn test_lower_rejects_large_colr_paint_glyph() {
	let mut font = Font::new();
	// `font.getGlyphID = glyph_map.__getitem__`.
	font.set_glyph_order(glyph_order(0x10001, "high"));
	font.insert(Box::new(colr_font(ot::Paint::Format33(
		ot::PaintFormat33 {
			paint: solid_paint(),
			glyph: 0x10000,
		},
	))));

	assert_error_contains(lower_tables(&mut font, &tables(&["COLR"])), "does not fit");
}

#[test]
fn test_layout_header_round_trip() -> TestResult {
	let mut font = test_ttf_with("layout_header")?;

	upper_tables(&mut font, &tables(&["GSUB", "GPOS"]))?;

	macro_rules! assert_extended {
		($font:ident, $table:ty) => {
			let table = $font.require::<$table>()?;
			assert_eq!(table.version, 0x0001_0002);
			assert!(table.script_list.is_none());
			assert!(table.feature_list.is_none());
			assert!(table.lookup_list.is_none());
			assert!(table.script_list2.is_some());
			assert!(table.feature_list2.is_some());
			assert!(table.lookup_list2.is_some());
		};
	}
	assert_extended!(font, Gsub);
	assert_extended!(font, Gpos);
	assert_layout_formats(&mut font, false)?;

	let mut font = save_and_read(&mut font)?;

	lower_tables(&mut font, &tables(&["GSUB", "GPOS"]))?;

	macro_rules! assert_compact {
		($font:ident, $table:ty) => {
			let table = $font.require::<$table>()?;
			assert_eq!(table.version, 0x0001_0000);
			assert!(table.script_list.is_some());
			assert!(table.feature_list.is_some());
			let lookup_list = table.lookup_list.as_ref().ok_or("no LookupList")?;
			assert_eq!(lookup_list.lookup_count, Some(1));
			assert!(lookup_list.lookup[0].is_some());
			assert!(table.script_list2.is_none());
			assert!(table.feature_list2.is_none());
			assert!(table.lookup_list2.is_none());
		};
	}
	assert_compact!(font, Gsub);
	assert_compact!(font, Gpos);
	assert_layout_formats(&mut font, false)?;

	save_and_read(&mut font)?;
	Ok(())
}

/// `WOFF2RoundtripTest.test_roundtrip_beyond64k_companion_tables`.
#[test]
fn test_roundtrip_beyond64k_companion_tables() -> TestResult {
	// The `ttFont` fixture of the test.
	let options = ReadOptions {
		recalc_bboxes: false,
		recalc_timestamp: false,
		..Default::default()
	};
	let mut font = Font::from_ttx(&fixture("woff2_fontfile"))?;
	let mut font = Font::read(font.write(&WriteOptions::default())?, options)?;

	upper_tables(&mut font, &ConvertOptions::default())?;
	font.flavor = Some(Flavor::Woff2);
	let data = font.write(&WriteOptions::default())?;

	// `roundtrip`.
	let mut font2 = Font::read(data.clone(), options)?;
	let data2 = font2.write(&WriteOptions::default())?;

	assert!(data == data2);
	assert_companions(&font2, COMPANIONS_UPPER, COMPANIONS_LOWER);
	let flavor_data: &FlavorData = font2.flavor_data.as_ref().ok_or("no flavor data")?;
	let transformed = flavor_data.transformed_tables.as_ref().ok_or("no tables")?;
	assert!(transformed.is_empty(), "{transformed:?}");
	Ok(())
}

/// `Tests/ttLib/tables/otTables_test.py::test_VarIdxMap_format1_beyond_64k`.
#[test]
#[allow(non_snake_case)]
fn test_VarIdxMap_format1_beyond_64k() -> TestResult {
	// 65537 glyphs; only the highest (gid 0x10000 > 0xFFFF) varies, so the map
	// cannot be trimmed below 65536 entries and must use Format 1.
	let mut font = Font::new();
	font.set_glyph_order(glyph_order(0x10001, "last"));
	let mut table = VarIdxMap::default();
	for gid in 0..0x10001 {
		table.mapping.insert(gid, 0);
	}
	table.mapping.insert(0x10000, 1);

	let mut graph = WriterGraph::new(Tag::new(b"HVAR"));
	table.compile(&mut graph.root(), &mut font)?;
	let data = graph.get_all_data()?;
	// Format 1 (uint8).
	assert_eq!(data[0], 1);
	// uint32 MappingCount = 0x10001.
	assert_eq!(data[2..6], [0, 1, 0, 1]);

	let mut reader = OtReader::new(&data, Tag::new(b"HVAR"));
	let table2 = VarIdxMap::decompile(&mut reader, &mut font)?;
	assert_eq!(table2.mapping[&0x10000], 1);
	assert_eq!(table2.mapping[&0], 0);
	Ok(())
}

/// A font of `tests/data/reorder_glyphs` (made by `tools/reference/reorder_scale_fixtures.py`).
fn reorder_fixture(name: &str) -> String {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data/reorder_glyphs")
		.join(format!("{name}.ttx"));
	std::fs::read_to_string(path).expect("a fixture")
}

/// `getXML(font[tag].toXML, font)` of some tables, as one TTX document.
fn tables_xml(font: &mut Font, tags: &[&str]) -> TestResult<String> {
	let text = font.to_ttx(&fontoxide_ttlib::TtxOptions {
		tables: tags
			.iter()
			.map(|t| {
				fontoxide_ttlib::TableKey::Table(Tag::new(&t.as_bytes().try_into().expect("a tag")))
			})
			.collect(),
		..Default::default()
	})?;
	// Without the `ttFont` element's attributes.
	Ok(text
		.lines()
		.map(|l| {
			if l.starts_with("<ttFont ") {
				"<ttFont>"
			} else {
				l
			}
		})
		.collect::<Vec<_>>()
		.join("\n"))
}

fn reversed_order(font: &mut Font) -> TestResult<Vec<String>> {
	let order = font.glyph_order()?.names().to_vec();
	Ok(order[..1]
		.iter()
		.chain(order[1..].iter().rev())
		.cloned()
		.collect())
}

/// `test_reorder_extended_layout`, but for the `lazy` parameter. The reference is also
/// compared with fontTools' result.
#[test]
fn test_reorder_extended_layout() -> TestResult {
	use fontoxide_ttlib::reorder_glyphs::reorder_glyphs;

	let layout = ["GDEF", "GSUB", "GPOS"];
	let expected = reorder_fixture("extended_layout.expected");
	for extended_header in [false, true] {
		for extended_formats in [false, true] {
			let mut built = Font::from_ttx(&reorder_fixture("extended_layout"))?;
			let mut reference = save_and_read(&mut built)?;
			let mut font = save_and_read(&mut built)?;
			if extended_header {
				upper_tables(&mut font, &tables(&layout))?;
			}
			convert_layout_formats(&mut font.require::<Gsub>()?.table, extended_formats)?;
			convert_layout_formats(&mut font.require::<Gpos>()?.table, extended_formats)?;
			let mut font = save_and_read(&mut font)?;
			let new_order = reversed_order(&mut font)?;
			reorder_glyphs(&mut reference, new_order.clone())?;
			reorder_glyphs(&mut font, new_order)?;
			let mut font = save_and_read(&mut font)?;
			lower_tables(&mut font, &tables(&layout))?;
			let reference_xml = tables_xml(&mut reference, &layout)?;
			assert_eq!(
				tables_xml(&mut font, &layout)?,
				reference_xml,
				"{extended_header} {extended_formats}"
			);
			let expected_xml = expected
				.lines()
				.map(|l| {
					if l.starts_with("<ttFont ") {
						"<ttFont>"
					} else {
						l
					}
				})
				.collect::<Vec<_>>()
				.join("\n");
			assert_eq!(reference_xml, expected_xml);
		}
	}
	Ok(())
}

/// `test_reorder_extended_contextual_layout`, but for the `lazy` parameter. The
/// reference is also compared with fontTools' result.
#[test]
fn test_reorder_extended_contextual_layout() -> TestResult {
	use fontoxide_ttlib::reorder_glyphs::reorder_glyphs;

	let layout = ["GSUB", "GPOS"];
	let expected = reorder_fixture("extended_contextual_layout.expected");
	for extended_header in [false, true] {
		let mut built = test_ttf()?;
		built.import_ttx(&reorder_fixture("extended_contextual_layout"))?;
		let mut reference = save_and_read(&mut built)?;
		let mut font = built;
		if extended_header {
			upper_tables(&mut font, &tables(&layout))?;
		} else {
			convert_layout_formats(&mut font.require::<Gsub>()?.table, true)?;
			convert_layout_formats(&mut font.require::<Gpos>()?.table, true)?;
		}
		let mut font = save_and_read(&mut font)?;
		let order = reversed_order(&mut font)?;
		reorder_glyphs(&mut reference, order.clone())?;
		reorder_glyphs(&mut font, order)?;
		let mut font = save_and_read(&mut font)?;
		lower_tables(&mut font, &tables(&layout))?;
		let reference_xml = tables_xml(&mut reference, &layout)?;
		assert_eq!(
			tables_xml(&mut font, &layout)?,
			reference_xml,
			"{extended_header}"
		);
		let expected_xml = expected
			.lines()
			.map(|l| {
				if l.starts_with("<ttFont ") {
					"<ttFont>"
				} else {
					l
				}
			})
			.collect::<Vec<_>>()
			.join("\n");
		assert_eq!(reference_xml, expected_xml);
	}
	Ok(())
}
