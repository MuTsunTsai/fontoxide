//! `Tests/merge/merge_test.py`.
//!
//! The fonts are built with `FontBuilder`, feaLib and otlLib, as in fontTools.
//! fontTools refers to glyphs by name; here the `cmap` unit tests use glyph IDs
//! for the names.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use fontoxide::font_builder::{FontBuilder, NameKey, NameValue, Value};
use fontoxide::otllib::GlyphMap;
use fontoxide::otllib::builder::{
	ChainContextualRule, ChainContextualRuleset, LayoutTable, LookupBuilder, MathKernSide,
	MathTableArgs, RuleLookup, build_format1_subtable, build_format2_subtable,
	build_format3_subtable, build_lookup, build_mark_glyph_sets_def, build_math_table,
};
use fontoxide_merge::{Error, Item, Merger, Options, main as merge_main};
use fontoxide_ttlib::beyond64k::{ConvertOptions, convert_layout_formats, upper_tables};
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::cmap::{CharMap, DEFAULT_CMAP_PREFERENCES, UvsDict};
use fontoxide_ttlib::tables::glyf::Glyph;
use fontoxide_ttlib::tables::{Cmap, CmapSubtable, Dmap, Gasp, Gdef, Gpos, Gsub, Head, Math, Os2};
use fontoxide_ttlib::{Font, ReadOptions, Table, Tag, TtxOptions, WriteOptions};
use indexmap::IndexMap;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn s(name: &str) -> String {
	name.to_owned()
}

/// A table to merge.
fn some<T: Table>(table: &mut T) -> Option<&mut dyn Table> {
	Some(table)
}

// MergeIntegrationTest

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

/// `tempdir` and `temp_path`; the directory is removed at the end.
struct TempDir {
	dir: PathBuf,
	files: u32,
}

impl TempDir {
	fn new() -> TestResult<Self> {
		let n = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
		let dir = std::env::temp_dir().join(format!("fontoxide-merge-{}-{n}", std::process::id()));
		std::fs::create_dir_all(&dir)?;
		Ok(TempDir { dir, files: 0 })
	}

	fn path(&mut self, suffix: &str) -> String {
		self.files += 1;
		self.dir
			.join(format!("tmp{}{suffix}", self.files))
			.to_string_lossy()
			.into_owned()
	}
}

impl Drop for TempDir {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.dir);
	}
}

fn getpath(testfile: &str) -> String {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/merge/data")
		.join(testfile)
		.to_string_lossy()
		.into_owned()
}

/// `read_ttx`: the lines, without the data that often changes.
fn read_ttx(text: &str) -> Vec<String> {
	text.split_inclusive('\n')
		.map(|line| {
			let ignored = line.starts_with("<ttFont ")
				|| ["checkSumAdjustment", "created", "modified"]
					.iter()
					.any(|t| line.starts_with(&format!("    <{t} ")));
			if ignored {
				"\n".to_owned()
			} else {
				format!("{}\n", line.trim_end())
			}
		})
		.collect()
}

fn expect_ttx(font: &mut Font, expected_ttx: &str) -> TestResult {
	let actual = read_ttx(&font.to_ttx(&TtxOptions::default())?);
	let expected = read_ttx(&std::fs::read_to_string(expected_ttx)?);
	if actual != expected {
		for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
			if a != e {
				eprintln!("line {}:\n- {e}+ {a}", i + 1);
				break;
			}
		}
		panic!(
			"TTX output is different from expected ({} lines, {} expected)",
			actual.len(),
			expected.len()
		);
	}
	Ok(())
}

/// `compile_font`.
fn compile_font(tmp: &mut TempDir, path: &str, suffix: &str) -> TestResult<String> {
	let savepath = tmp.path(suffix);
	let mut font = Font::new();
	font.recalc_bboxes = false;
	font.recalc_timestamp = false;
	font.import_ttx(&std::fs::read_to_string(path)?)?;
	std::fs::write(
		&savepath,
		font.write(&WriteOptions {
			reorder_tables: None,
			..Default::default()
		})?,
	)?;
	Ok(savepath)
}

#[test]
fn test_merge_cff() -> TestResult {
	let mut tmp = TempDir::new()?;
	let fontpath1 = compile_font(&mut tmp, &getpath("CFFFont1.ttx"), ".otf")?;
	let fontpath2 = compile_font(&mut tmp, &getpath("CFFFont2.ttx"), ".otf")?;
	let mergedpath = tmp.path(".otf");
	let output = format!("--output-file={mergedpath}");
	assert_eq!(merge_main(&[&fontpath1, &fontpath2, &output])?, 0);
	let mut mergedfont = Font::read(std::fs::read(&mergedpath)?, ReadOptions::default())?;
	expect_ttx(&mut mergedfont, &getpath("CFFFont_expected.ttx"))
}

// gaspMergeUnitTest

mod gasp_merge_unit_test {
	use super::*;

	fn tables() -> (Gasp, Gasp) {
		let table1 = Gasp {
			version: 1,
			gasp_range: [(0x8, 0xA), (0x10, 0x5)].into_iter().collect(),
		};
		let table2 = Gasp {
			version: 1,
			gasp_range: [(0x6, 0xB), (0xFF, 0x4)].into_iter().collect(),
		};
		(table1, table2)
	}

	fn merge(tables: &mut [Option<&mut dyn Table>]) -> TestResult<Option<Gasp>> {
		let result = Merger::default().merge_table(Tag::new(b"gasp"), tables)?;
		Ok(result.and_then(|t| (*t).as_any().downcast_ref::<Gasp>().cloned()))
	}

	#[test]
	fn test_gasp_merge_basic() -> TestResult {
		let (mut table1, mut table2) = tables();
		let (t1, t2) = tables();
		let result = merge(&mut [some(&mut table1), some(&mut table2)])?;
		assert_eq!(result, Some(t1.clone()));
		let result = merge(&mut [some(&mut table2), some(&mut table1)])?;
		assert_eq!(result, Some(t2));
		Ok(())
	}

	#[test]
	fn test_gasp_merge_not_implemented() -> TestResult {
		let (mut table1, _) = tables();
		let (t1, _) = tables();
		let result = merge(&mut [None, some(&mut table1)])?;
		assert_eq!(result, None);
		let result = merge(&mut [some(&mut table1), None])?;
		assert_eq!(result, Some(t1));
		Ok(())
	}
}

// CmapMergeUnitTest

mod cmap_merge_unit_test {
	use super::*;

	fn make_subtable(
		format: u16,
		platform_id: u16,
		plat_enc_id: u16,
		cmap: &[(u32, u32)],
	) -> CmapSubtable {
		let mut subtable = CmapSubtable::new(format);
		subtable.platform_id = platform_id;
		subtable.plat_enc_id = plat_enc_id;
		subtable.language = Some(0);
		subtable.cmap = std::sync::Arc::new(cmap.iter().copied().collect());
		subtable
	}

	fn make_uvs_subtable(
		format: u16,
		platform_id: u16,
		plat_enc_id: u16,
		uvs_dict: UvsDict,
	) -> CmapSubtable {
		let mut subtable = make_subtable(format, platform_id, plat_enc_id, &[]);
		subtable.uvs_dict = Some(uvs_dict);
		subtable
	}

	fn cmap(tables: Vec<CmapSubtable>) -> Cmap {
		Cmap {
			table_version: 0,
			tables,
		}
	}

	fn merge(merger: &mut Merger, table1: &mut Cmap, table2: &mut Cmap) -> TestResult<Cmap> {
		let merged = merger.merge_table(Tag::new(b"cmap"), &mut [some(table1), some(table2)])?;
		Ok(merged
			.and_then(|t| (*t).as_any().downcast_ref::<Cmap>().cloned())
			.ok_or("no cmap")?)
	}

	fn properties(t: &CmapSubtable) -> (u16, u16, u16, Option<u32>) {
		(t.format, t.platform_id, t.plat_enc_id, t.language)
	}

	fn map(entries: &[(u32, u32)]) -> CharMap {
		entries.iter().copied().collect()
	}

	/// 4-3-1 table merged with 12-3-10 table with no dupes with codepoints
	/// outside BMP.
	#[test]
	fn test_cmap_merge_no_dupes() -> TestResult {
		// Glyph IDs: SNOWMAN 1, SNOWMAN WITHOUT SNOW 2, WRESTLERS 3.
		let cmap1 = [(0x2603, 1)];
		let mut table1 = cmap(vec![make_subtable(4, 3, 1, &cmap1)]);
		let cmap2 = [(0x26C4, 2)];
		let cmap2_extended = [(0x1F93C, 3), (0x26C4, 2)];
		let mut table2 = cmap(vec![
			make_subtable(4, 3, 1, &cmap2),
			make_subtable(12, 3, 10, &cmap2_extended),
		]);

		// fontTools sets `alternateGlyphsPerFont`, which nothing uses.
		let mut merger = Merger::default();
		let merged = merge(&mut merger, &mut table1, &mut table2)?;

		assert_eq!(merged.tables.len(), 2);
		assert_eq!(
			merged.tables.iter().map(properties).collect::<Vec<_>>(),
			[(4, 3, 1, Some(0)), (12, 3, 10, Some(0))]
		);
		assert_eq!(*merged.tables[0].cmap, map(&[(0x26C4, 2), (0x2603, 1)]));
		assert_eq!(
			*merged.tables[1].cmap,
			map(&[(0x1F93C, 3), (0x26C4, 2), (0x2603, 1)])
		);
		Ok(())
	}

	/// Tests Issue #322.
	#[test]
	fn test_cmap_merge_three_dupes() -> TestResult {
		// Glyph IDs: space#0 1, space#1 2, uni00A0#1 3.
		let cmap1 = [(0x20, 1), (0xA0, 1)];
		let mut table1 = cmap(vec![make_subtable(4, 3, 1, &cmap1)]);
		let cmap2 = [(0x20, 2), (0xA0, 3)];
		let mut table2 = cmap(vec![make_subtable(4, 3, 1, &cmap2)]);

		let mut merger = Merger::default();
		merger.duplicate_glyphs_per_font = vec![BTreeMap::new(), BTreeMap::new()];
		let merged = merge(&mut merger, &mut table1, &mut table2)?;

		assert_eq!(merged.tables.len(), 1);
		let table = &merged.tables[0];
		assert_eq!(properties(table), (4, 3, 1, Some(0)));
		assert_eq!(*table.cmap, map(&cmap1));
		assert_eq!(
			merger.duplicate_glyphs_per_font,
			[BTreeMap::new(), [(1, 2)].into_iter().collect()]
		);
		Ok(())
	}

	#[test]
	fn test_cmap_merge_format_14() -> TestResult {
		// Glyph IDs: uni3404 1, uni0030.FE00 2, u2F803 3, uni34FF.var2 4.
		let cmap1 = [(0x3404, 1)];
		let uvs_dict1: UvsDict = [
			(0xFE00, vec![(0x0030, Some(2))]),
			(0xE0101, vec![(0x3404, Some(1))]),
		]
		.into_iter()
		.collect();
		let mut table1 = cmap(vec![
			make_uvs_subtable(14, 0, 5, uvs_dict1),
			make_subtable(4, 3, 1, &cmap1),
		]);
		let uvs_dict2: UvsDict = [
			(0xFE00, vec![(0x20122, Some(3))]),
			(0xE0102, vec![(0x34FF, Some(4))]),
		]
		.into_iter()
		.collect();
		let mut table2 = cmap(vec![make_uvs_subtable(14, 0, 5, uvs_dict2)]);

		let mut merger = Merger::default();
		merger.duplicate_glyphs_per_font = vec![BTreeMap::new(), BTreeMap::new()];
		let merged = merge(&mut merger, &mut table1, &mut table2)?;

		assert_eq!(merged.tables.len(), 2);
		let (uvs_table, cmap_table) = (&merged.tables[0], &merged.tables[1]);
		assert_eq!(properties(uvs_table), (14, 0, 5, Some(0)));
		assert_eq!(properties(cmap_table), (4, 3, 1, Some(0)));
		let expected_uvs_dict: UvsDict = [
			(0xFE00, vec![(0x0030, Some(2)), (0x20122, Some(3))]),
			(0xE0101, vec![(0x3404, None)]),
			(0xE0102, vec![(0x34FF, Some(4))]),
		]
		.into_iter()
		.collect();
		assert_eq!(uvs_table.uvs_dict, Some(expected_uvs_dict));
		Ok(())
	}

	#[test]
	fn test_cmap_merge_format_14_dupes() -> TestResult {
		// Glyph IDs: u2F803#0 1, uni3404 2, u2F803#1 3, uni34FF.var2 4.
		let cmap1 = [(0x2F803, 1)];
		let uvs_dict1: UvsDict = [
			(0xFE00, vec![(0x20122, Some(1))]),
			(0xE0101, vec![(0x3404, Some(2))]),
		]
		.into_iter()
		.collect();
		let mut table1 = cmap(vec![
			make_uvs_subtable(14, 0, 5, uvs_dict1),
			make_subtable(12, 3, 10, &cmap1),
		]);
		let cmap2 = [(0x2F803, 3)];
		let uvs_dict2: UvsDict = [
			(0xFE00, vec![(0x20122, Some(3))]),
			(0xE0102, vec![(0x34FF, Some(4))]),
		]
		.into_iter()
		.collect();
		let mut table2 = cmap(vec![
			make_uvs_subtable(14, 0, 5, uvs_dict2),
			make_subtable(12, 3, 10, &cmap2),
		]);

		let mut merger = Merger::default();
		merger.duplicate_glyphs_per_font = vec![BTreeMap::new(), BTreeMap::new()];
		let merged = merge(&mut merger, &mut table1, &mut table2)?;

		assert_eq!(merged.tables.len(), 3);
		assert_eq!(
			merged.tables.iter().map(properties).collect::<Vec<_>>(),
			[
				(14, 0, 5, Some(0)),
				(4, 3, 1, Some(0)),
				(12, 3, 10, Some(0))
			]
		);
		let expected_uvs_dict: UvsDict = [
			(0xFE00, vec![(0x20122, Some(1))]),
			(0xE0101, vec![(0x3404, Some(2))]),
			(0xE0102, vec![(0x34FF, Some(4))]),
		]
		.into_iter()
		.collect();
		assert_eq!(merged.tables[0].uvs_dict, Some(expected_uvs_dict));
		assert_eq!(
			merger.duplicate_glyphs_per_font,
			[BTreeMap::new(), [(1, 3)].into_iter().collect()]
		);
		Ok(())
	}
}

// The module-level tests.

/// `_compile`.
fn compile(font: &mut Font) -> TestResult<Vec<u8>> {
	Ok(font.write(&WriteOptions::default())?)
}

fn read(data: Vec<u8>) -> TestResult<Font> {
	Ok(Font::read(data, ReadOptions::default())?)
}

fn names(family: &str) -> Vec<(NameKey, NameValue)> {
	vec![
		(NameKey::from("familyName"), NameValue::from(family)),
		(NameKey::from("styleName"), NameValue::from("Regular")),
	]
}

/// A font with empty glyphs of advance 500.
fn setup_font(
	glyph_order: &[String],
	cmap: &BTreeMap<u32, String>,
	family: &str,
) -> TestResult<FontBuilder> {
	let mut fb = FontBuilder::new(1000, true, false)?;
	fb.setup_glyph_order(glyph_order.to_vec());
	fb.setup_character_map(cmap, None, false)?;
	let glyphs: IndexMap<String, Glyph> = glyph_order
		.iter()
		.map(|g| (g.clone(), Glyph::default()))
		.collect();
	fb.setup_glyf(glyphs, true)?;
	let hmtx: IndexMap<String, (f64, f64)> = glyph_order
		.iter()
		.map(|g| (g.clone(), (500.0, 0.0)))
		.collect();
	fb.setup_horizontal_metrics(&hmtx)?;
	fb.setup_horizontal_header(&[])?;
	fb.setup_name_table(&names(family), true, true)?;
	Ok(fb)
}

/// `_make_fontfile_with_OS2`.
fn make_fontfile_with_os2(version: i64) -> TestResult<Vec<u8>> {
	let glyph_order = [s(".notdef"), s("a")];
	let cmap = [(0x61, s("a"))].into_iter().collect();
	let mut fb = setup_font(&glyph_order, &cmap, "TestOS2")?;
	fb.setup_os2(&[("version", Value::Int(version))])?;
	compile(&mut fb.font)
}

/// `_make_fontfile_with_glyphs`.
fn make_fontfile_with_glyphs(
	first_gid: u32,
	glyph_count: u32,
	add_layout: bool,
) -> TestResult<Vec<u8>> {
	let mut glyph_order = vec![s(".notdef")];
	glyph_order
		.extend((first_gid..first_gid + glyph_count - 1).map(|gid| format!("glyph{gid:05}")));
	let cmap: BTreeMap<u32, String> = glyph_order[1..]
		.iter()
		.enumerate()
		.map(|(codepoint, name)| (first_gid + codepoint as u32, name.clone()))
		.collect();
	let mut fb = setup_font(&glyph_order, &cmap, "TestMerge")?;
	fb.setup_os2(&[])?;
	if add_layout {
		let (glyph1, glyph2) = (&glyph_order[1], &glyph_order[2]);
		fb.add_open_type_features(
			&format!("feature salt {{ sub {glyph1} by {glyph2}; }} salt;"),
			None,
			None,
			false,
		)?;
	}
	compile(&mut fb.font)
}

/// `_upper_fontfile`.
fn upper_fontfile(fontfile: Vec<u8>) -> TestResult<Vec<u8>> {
	let mut font = read(fontfile)?;
	upper_tables(&mut font, &ConvertOptions::default())?;
	compile(&mut font)
}

/// `_merge_and_recompile`.
fn merge_and_recompile(fontfiles: &[Vec<u8>], options: Option<Options>) -> TestResult<Font> {
	let mut merger = Merger::new(options.unwrap_or_default());
	let mut merged = merger.merge(fontfiles)?;
	read(compile(&mut merged)?)
}

fn new_uvs_subtable(format: u16, uvs_dict: UvsDict) -> CmapSubtable {
	let mut uvs = CmapSubtable::new(format);
	uvs.platform_id = 0;
	uvs.plat_enc_id = 5;
	uvs.language = Some(0);
	uvs.uvs_dict = Some(uvs_dict);
	uvs
}

fn best_cmap(font: &mut Font) -> TestResult<CharMap> {
	Ok(font
		.get_best_cmap(&DEFAULT_CMAP_PREFERENCES)?
		.ok_or("no Unicode cmap")?)
}

#[test]
fn test_merge_extended_uvs() -> TestResult {
	for first_format in [14, 15] {
		for second_format in [14, 15] {
			for high_glyph_ids in [false, true] {
				let mut fontfiles = Vec::new();
				let count = if high_glyph_ids { 0x8001 } else { 4 };
				for (index, format) in [first_format, second_format].into_iter().enumerate() {
					let index = index as u32;
					let mut font = read(make_fontfile_with_glyphs(
						0x10000 * (index + 1),
						count,
						false,
					)?)?;
					let last = font.glyph_order()?.len() as u32 - 1;
					let uvs = new_uvs_subtable(
						format,
						[
							(0xFE00, vec![(0x10000 * (index + 1), Some(last))]),
							(0xE0100 + index, vec![(0x10000 * (index + 1) + 1, None)]),
						]
						.into_iter()
						.collect(),
					);
					font.require::<Cmap>()?.tables.push(uvs);
					fontfiles.push(compile(&mut font)?);
				}

				let mut merger = Merger::default();
				let mut merged = merger.merge(&fontfiles)?;
				let merged_cmap = merged.require::<Cmap>()?;
				assert_eq!(
					merged_cmap.tables[0].format,
					if high_glyph_ids { 15 } else { 14 }
				);
				let total = merger.glyph_order.as_ref().ok_or("no glyph order")?.len() as u32;
				let expected: UvsDict = [
					(
						0xFE00,
						vec![(0x10000, Some(count - 1)), (0x20000, Some(total - 1))],
					),
					(0xE0100, vec![(0x10001, None)]),
					(0xE0101, vec![(0x20001, None)]),
				]
				.into_iter()
				.collect();
				assert_eq!(merged_cmap.tables[0].uvs_dict, Some(expected.clone()));
				let mut roundtripped = read(compile(&mut merged)?)?;
				let actual = roundtripped.require::<Cmap>()?.tables[0]
					.uvs_dict
					.clone()
					.ok_or("no UVS")?;
				for (selector, mappings) in &expected {
					assert_eq!(actual[selector], *mappings);
				}
			}
		}
	}
	Ok(())
}

#[test]
fn test_merge_prefers_format15_uvs() -> TestResult {
	for reverse_order in [false, true] {
		let mut font = read(make_fontfile_with_glyphs(0x10000, 4, false)?)?;
		let glyph_order = font.glyph_order()?.names().to_vec();
		let mut subtables = Vec::new();
		for (format, glyph) in [(14, 2), (15, 3)] {
			subtables.push(new_uvs_subtable(
				format,
				[(0xFE00, vec![(0x10000, Some(glyph))])]
					.into_iter()
					.collect(),
			));
		}
		if reverse_order {
			subtables.reverse();
		}
		font.require::<Cmap>()?.tables.extend(subtables);

		let mut merger = Merger::default();
		merger.glyph_order = Some(glyph_order);
		merger.duplicate_glyphs_per_font = vec![BTreeMap::new()];
		let cmap = font.require::<Cmap>()?;
		let merged = merger
			.merge_table(Tag::new(b"cmap"), &mut [some(cmap)])?
			.ok_or("no cmap")?;
		let merged = (*merged)
			.as_any()
			.downcast_ref::<Cmap>()
			.ok_or("not a cmap")?;
		let expected: UvsDict = [(0xFE00, vec![(0x10000, Some(3))])].into_iter().collect();
		assert_eq!(merged.tables[0].uvs_dict, Some(expected));
	}
	Ok(())
}

#[test]
fn test_merge_dmap() -> TestResult {
	for uvs_format in [14, 15] {
		for dmap_uvs in ["default", "nondefault", "zero"] {
			for dmap_only in [false, true] {
				for high_glyph_ids in [false, true] {
					merge_dmap(uvs_format, dmap_uvs, dmap_only, high_glyph_ids)?;
				}
			}
		}
	}
	Ok(())
}

fn merge_dmap(
	uvs_format: u16,
	dmap_uvs: &str,
	dmap_only: bool,
	high_glyph_ids: bool,
) -> TestResult {
	let mut font = read(make_fontfile_with_glyphs(0x10000, 4, false)?)?;
	let mut nominal = CmapSubtable::new(12);
	nominal.platform_id = 3;
	nominal.plat_enc_id = 10;
	nominal.language = Some(0);
	nominal.cmap = std::sync::Arc::new(
		[(0x10000, 2), (0x10001, 0), (0x30000, 3)]
			.into_iter()
			.collect(),
	);
	let base_uvs = new_uvs_subtable(
		uvs_format,
		[
			(0xFE00, vec![(0x10000, Some(3)), (0x10001, None)]),
			(0xE0100, vec![(0x10000, None)]),
		]
		.into_iter()
		.collect(),
	);
	font.require::<Cmap>()?.tables.push(base_uvs);
	let delta = match dmap_uvs {
		"default" => None,
		"nondefault" => Some(1),
		_ => Some(0),
	};
	let delta_uvs = new_uvs_subtable(
		uvs_format,
		[(0xFE00, vec![(0x10000, delta), (0x30000, Some(2))])]
			.into_iter()
			.collect(),
	);
	font.insert(Box::new(Dmap(Cmap {
		table_version: 0,
		tables: vec![delta_uvs, nominal],
	})));
	if dmap_only {
		font.remove(Tag::new(b"cmap"));
	}

	let second =
		make_fontfile_with_glyphs(0x20000, if high_glyph_ids { 0xFFFF } else { 4 }, false)?;
	let first = compile(&mut font)?;
	let fontfiles = if high_glyph_ids {
		[second, first]
	} else {
		[first, second]
	};
	let mut merged = merge_and_recompile(&fontfiles, None)?;
	let glyph_offset = if high_glyph_ids { 0xFFFF } else { 0 };
	let case = format!("{uvs_format} {dmap_uvs} {dmap_only} {high_glyph_ids}");
	assert!(!merged.contains(Tag::new(b"DMAP")), "{case}");
	let cmap = best_cmap(&mut merged)?;
	assert_eq!(cmap[&0x10000], glyph_offset + 2, "{case}");
	assert_eq!(cmap[&0x30000], glyph_offset + 3, "{case}");
	assert_eq!(cmap[&0x20000], if high_glyph_ids { 1 } else { 5 }, "{case}");
	if dmap_only {
		assert!(!cmap.contains_key(&0x10001), "{case}");
	} else {
		assert_eq!(cmap[&0x10001], glyph_offset + 2, "{case}");
	}
	let merged_cmap = merged.require::<Cmap>()?;
	assert_eq!(
		merged_cmap.tables[0].format,
		if high_glyph_ids { 15 } else { 14 },
		"{case}"
	);
	let uvs: BTreeMap<u32, BTreeMap<u32, Option<u32>>> = merged_cmap.tables[0]
		.uvs_dict
		.iter()
		.flatten()
		.map(|(selector, entries)| (*selector, entries.iter().copied().collect()))
		.collect();
	let expected = match dmap_uvs {
		"default" => None,
		"nondefault" => Some(1),
		_ if dmap_only => None,
		_ => Some(3),
	};
	if dmap_uvs == "zero" && dmap_only {
		assert!(!uvs[&0xFE00].contains_key(&0x10000), "{case}");
	} else {
		match expected {
			None => assert_eq!(uvs[&0xFE00][&0x10000], None, "{case}"),
			Some(e) => assert_eq!(uvs[&0xFE00][&0x10000], Some(glyph_offset + e), "{case}"),
		}
	}
	assert_eq!(uvs[&0xFE00][&0x30000], Some(glyph_offset + 2), "{case}");
	if !dmap_only {
		assert_eq!(uvs[&0xFE00][&0x10001], None, "{case}");
		assert_eq!(uvs[&0xE0100][&0x10000], None, "{case}");
	}
	Ok(())
}

#[test]
fn test_merge_dmap_respects_drop_tables() -> TestResult {
	let mut font = read(make_fontfile_with_glyphs(0x10000, 4, false)?)?;
	let mut subtable = CmapSubtable::new(12);
	subtable.platform_id = 3;
	subtable.plat_enc_id = 10;
	subtable.language = Some(0);
	subtable.cmap = std::sync::Arc::new([(0x10000, 2)].into_iter().collect());
	font.insert(Box::new(Dmap(Cmap {
		table_version: 0,
		tables: vec![subtable],
	})));
	let mut options = Merger::default().options;
	options.drop_tables = vec![Item::from("DMAP")];
	let mut merged = merge_and_recompile(&[compile(&mut font)?], Some(options))?;
	assert!(!merged.contains(Tag::new(b"DMAP")));
	assert_eq!(best_cmap(&mut merged)?[&0x10000], 1);
	Ok(())
}

#[test]
fn test_merge_dmap_uvs_only() -> TestResult {
	for uvs_format in [14, 15] {
		let mut font = read(make_fontfile_with_glyphs(0x10000, 4, false)?)?;
		let uvs = new_uvs_subtable(
			uvs_format,
			[(0xFE00, vec![(0x10000, Some(2))])].into_iter().collect(),
		);
		font.insert(Box::new(Dmap(Cmap {
			table_version: 0,
			tables: vec![uvs],
		})));
		let mut merged = merge_and_recompile(&[compile(&mut font)?], None)?;
		assert!(!merged.contains(Tag::new(b"DMAP")));
		assert_eq!(best_cmap(&mut merged)?[&0x10000], 1);
		let glyph = merged.require::<Cmap>()?.tables[0]
			.uvs_dict
			.as_ref()
			.ok_or("no UVS")?[&0xFE00][0]
			.1;
		assert_eq!(glyph, Some(2));
	}
	Ok(())
}

#[test]
fn test_merge_extended_layout() -> TestResult {
	for upper_family in [false, true] {
		for extended_header in [false, true] {
			for extended_formats in [false, true] {
				for context_format in [1, 2, 3] {
					for mark_filtering in [false, true] {
						for chaining in [false, true] {
							merge_extended_layout(
								upper_family,
								extended_header,
								extended_formats,
								context_format,
								mark_filtering,
								chaining,
							)?;
						}
					}
				}
			}
		}
	}
	Ok(())
}

fn merge_extended_layout(
	upper_family: bool,
	extended_header: bool,
	extended_formats: bool,
	context_format: u32,
	mark_filtering: bool,
	chaining: bool,
) -> TestResult {
	let mut font = read(make_fontfile_with_glyphs(0xE100, 4, false)?)?;
	let order = font.glyph_order()?.names().to_vec();
	let (first, second, third) = (&order[1], &order[2], &order[3]);
	fontoxide::fealib::builder::add_open_type_features_from_string(
		&mut font,
		&format!(
			"
			lookup Substitute {{ sub {first} by {second}; }} Substitute;
			lookup Position {{ pos {first} -20; }} Position;
			feature calt {{ sub {first}' lookup Substitute {second}; }} calt;
			feature kern {{ pos {first}' lookup Position {second}; }} kern;
			table GDEF {{
				GlyphClassDef [{first}], [], [{second}], [];
				LigatureCaretByPos {first} 0;
			}} GDEF;
			"
		),
		None,
		None,
		false,
	)?;
	for (tag, pos) in [("GSUB", false), ("GPOS", true)] {
		let gm = GlyphMap::from_font(&mut font)?;
		let lookup_builder = if pos {
			LookupBuilder::chain_context_pos(gm.clone(), None)
		} else {
			LookupBuilder::chain_context_subst(gm.clone(), None)
		};
		let common = lookup_builder.common().clone();
		let referenced_lookup = LookupBuilder::single_subst(gm, None).into_ref();
		referenced_lookup.borrow_mut().common_mut().lookup_index = Some(0);
		let context: Vec<Vec<String>> = if chaining {
			vec![vec![third.clone()]]
		} else {
			Vec::new()
		};
		let rule = ChainContextualRule::new(
			context.clone(),
			vec![vec![first.clone()], vec![second.clone()]],
			context,
			vec![Some(RuleLookup::Many(vec![referenced_lookup])), None],
		);
		let mut ruleset = ChainContextualRuleset::new();
		ruleset.add_rule(rule.clone());
		let subtable = match context_format {
			1 => build_format1_subtable(&common, &ruleset, chaining)?,
			2 => {
				let classdefs = ruleset.format2_class_defs().ok_or("no class defs")?;
				build_format2_subtable(&common, &ruleset, &classdefs, chaining)?
			}
			_ => build_format3_subtable(&common, &rule, chaining)?,
		};
		let table = if pos {
			LayoutTable::Gpos
		} else {
			LayoutTable::Gsub
		};
		let lookup = build_lookup(vec![Some(subtable)], 0, None, Some(table), false)?;
		let lookup_list = if pos {
			font.require::<Gpos>()?.table.lookup_list.as_mut()
		} else {
			font.require::<Gsub>()?.table.lookup_list.as_mut()
		}
		.ok_or("no LookupList")?;
		lookup_list.lookup[1] = lookup.map(Box::new);
		let _ = tag;
	}
	if mark_filtering {
		let gm = GlyphMap::from_font(&mut font)?;
		let sets = build_mark_glyph_sets_def(&[vec![second.clone()]], &gm)?;
		let gdef = font.require::<Gdef>()?;
		gdef.table.mark_glyph_sets_def = sets.map(Box::new);
		gdef.table.version = 0x0001_0002;
		for pos in [false, true] {
			let lookup_list = if pos {
				font.require::<Gpos>()?.table.lookup_list.as_mut()
			} else {
				font.require::<Gsub>()?.table.lookup_list.as_mut()
			}
			.ok_or("no LookupList")?;
			let lookup = lookup_list.lookup[0].as_mut().ok_or("no lookup")?;
			lookup.lookup_flag |= 0x0010;
			lookup.mark_filtering_set = 0;
		}
	}

	let other = make_fontfile_with_glyphs(0xE000, 4, true)?;
	let mut reference = merge_and_recompile(&[other.clone(), compile(&mut font)?], None)?;
	if upper_family {
		upper_tables(&mut font, &ConvertOptions::default())?;
	}
	if extended_header {
		upper_tables(
			&mut font,
			&ConvertOptions {
				tables: Some(&["GSUB", "GPOS", "GDEF"]),
				..Default::default()
			},
		)?;
	}
	convert_layout_formats(&mut font.require::<Gsub>()?.table, extended_formats)?;
	convert_layout_formats(&mut font.require::<Gpos>()?.table, extended_formats)?;
	let mut merged = merge_and_recompile(&[other, compile(&mut font)?], None)?;
	let case = format!(
		"{upper_family} {extended_header} {extended_formats} {context_format} {mark_filtering} {chaining}"
	);
	if upper_family || extended_header {
		upper_tables(&mut reference, &ConvertOptions::default())?;
		assert!(merged.contains(Tag::new(b"MAXP")), "{case}");
		assert!(!merged.contains(Tag::new(b"maxp")), "{case}");
	} else {
		assert!(merged.contains(Tag::new(b"maxp")), "{case}");
		assert!(!merged.contains(Tag::new(b"MAXP")), "{case}");
	}
	for tag in [b"GSUB", b"GPOS", b"GDEF"] {
		let tag = Tag::new(tag);
		merged.load(tag)?;
		reference.load(tag)?;
		assert_eq!(
			merged.table_data(tag)?,
			reference.table_data(tag)?,
			"{tag} {case}"
		);
	}
	Ok(())
}

#[test]
fn test_merge_head_different_units_per_em() -> TestResult {
	let mut heads: Vec<Head> = [1000, 2048]
		.into_iter()
		.map(|units_per_em| Head {
			units_per_em,
			..Default::default()
		})
		.collect();
	let mut tables: Vec<Option<&mut dyn Table>> = heads
		.iter_mut()
		.map(|h| Some(h as &mut dyn Table))
		.collect();
	match Merger::default().merge_table(Tag::new(b"head"), &mut tables) {
		Err(Error::Value(message)) => assert_eq!(
			message,
			"Cannot merge fonts with different unitsPerEm values: [1000, 2048]"
		),
		other => panic!("ValueError expected: {other:?}"),
	}
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_merge_OS2_mixed_versions() -> TestResult {
	// https://github.com/fonttools/fonttools/issues/1865
	for v1 in 0..=5 {
		for v2 in 0..=5 {
			if v1 == v2 {
				continue;
			}
			let fontfiles = [make_fontfile_with_os2(v1)?, make_fontfile_with_os2(v2)?];
			let mut merged = merge_and_recompile(&fontfiles, None)?;
			assert_eq!(merged.require::<Os2>()?.version as i64, v1.max(v2));
		}
	}
	Ok(())
}

#[test]
fn test_merge_keeps_compact_tables_below_glyph_limit() -> TestResult {
	let fontfiles = [
		make_fontfile_with_glyphs(0xE000, 10, false)?,
		make_fontfile_with_glyphs(0xE100, 10, false)?,
	];
	let merged = merge_and_recompile(&fontfiles, None)?;
	assert!(merged.contains(Tag::new(b"glyf")));
	assert!(!merged.contains(Tag::new(b"GLYF")));
	assert!(merged.contains(Tag::new(b"maxp")));
	assert!(!merged.contains(Tag::new(b"MAXP")));
	Ok(())
}

#[test]
fn test_merge_uses_upper_tables_above_glyph_limit() -> TestResult {
	let fontfiles = [
		make_fontfile_with_glyphs(0x10000, 0x8001, false)?,
		make_fontfile_with_glyphs(0x20000, 0x8001, false)?,
	];
	let mut merged = merge_and_recompile(&fontfiles, None)?;
	assert!(fontoxide_ttlib::ot::custom::has_extended_glyph_ids(
		&mut merged
	)?);
	for tag in [b"GLYF", b"MAXP", b"HHEA", b"HMTX", b"LOCA"] {
		assert!(merged.contains(Tag::new(tag)));
	}
	assert!(!merged.contains(Tag::new(b"glyf")));
	assert!(!merged.contains(Tag::new(b"maxp")));
	Ok(())
}

#[test]
fn test_merge_uses_upper_layout_above_glyph_limit() -> TestResult {
	let fontfiles = [
		make_fontfile_with_glyphs(0x10000, 0x8001, true)?,
		make_fontfile_with_glyphs(0x20000, 0x8001, false)?,
	];
	let mut merged = merge_and_recompile(&fontfiles, None)?;
	let table = &merged.require::<Gsub>()?.table;
	assert_eq!(table.version, 0x0001_0002);
	assert!(table.script_list.is_none());
	assert!(table.feature_list.is_none());
	assert!(table.lookup_list.is_none());
	assert!(table.script_list2.is_some());
	assert!(table.feature_list2.is_some());
	assert!(table.lookup_list2.is_some());
	Ok(())
}

#[test]
fn test_merge_preserves_upper_input_family_below_glyph_limit() -> TestResult {
	let fontfiles = [
		upper_fontfile(make_fontfile_with_glyphs(0xE000, 10, false)?)?,
		make_fontfile_with_glyphs(0xE100, 10, false)?,
	];
	let merged = merge_and_recompile(&fontfiles, None)?;
	assert!(merged.contains(Tag::new(b"GLYF")));
	assert!(!merged.contains(Tag::new(b"glyf")));
	assert!(merged.contains(Tag::new(b"MAXP")));
	assert!(!merged.contains(Tag::new(b"maxp")));
	Ok(())
}

/// `_make_fontfile_with_MATH`.
fn make_fontfile_with_math(glyphs: &[&str], args: Option<MathTableArgs>) -> TestResult<Vec<u8>> {
	let mut glyph_order = vec![s(".notdef")];
	glyph_order.extend(glyphs.iter().map(|g| s(g)));
	let mut fb = setup_font(&glyph_order, &BTreeMap::new(), "TestMATH")?;
	fb.setup_os2(&[])?;
	fb.setup_post(true, &[])?;
	if let Some(args) = args {
		build_math_table(&mut fb.font, &args)?;
	}
	compile(&mut fb.font)
}

#[test]
#[allow(non_snake_case)]
fn test_merge_MATH() -> TestResult {
	// https://github.com/fonttools/fonttools/issues/3823
	let map = |v: &[(&str, f64)]| -> Option<IndexMap<String, f64>> {
		Some(v.iter().map(|(k, v)| (s(k), *v)).collect())
	};
	let variants = |v: &[(&str, &[(&str, f64)])]| -> Option<IndexMap<String, Vec<(String, f64)>>> {
		Some(
			v.iter()
				.map(|(k, v)| (s(k), v.iter().map(|(g, a)| (s(g), *a)).collect()))
				.collect(),
		)
	};
	let fontfiles = [
		make_fontfile_with_math(
			&["a", "parenleft", "parenleft.size1"],
			Some(MathTableArgs {
				constants: map(&[("AxisHeight", 250.0)]),
				italics_corrections: map(&[("a", 10.0)]),
				math_kerns: Some(
					[(
						s("a"),
						[(MathKernSide::TopRight, (vec![100.0], vec![10.0, 20.0]))]
							.into_iter()
							.collect(),
					)]
					.into_iter()
					.collect(),
				),
				min_connector_overlap: 20,
				vert_glyph_variants: variants(&[(
					"parenleft",
					&[("parenleft", 700.0), ("parenleft.size1", 1000.0)],
				)]),
				..Default::default()
			}),
		)?,
		make_fontfile_with_math(
			&["b", "integral", "minus"],
			Some(MathTableArgs {
				constants: map(&[("AxisHeight", 300.0)]),
				italics_corrections: map(&[("b", 20.0)]),
				top_accent_attachments: map(&[("b", 250.0)]),
				extended_shapes: Some(vec![s("integral")]),
				min_connector_overlap: 30,
				horiz_glyph_variants: variants(&[("minus", &[("minus", 500.0)])]),
				..Default::default()
			}),
		)?,
		make_fontfile_with_math(&["c"], None)?,
	];
	let mut merged = merge_and_recompile(&fontfiles, None)?;
	let order = merged.glyph_order()?;
	let names = |c: &Option<Box<ot::Coverage>>| -> Vec<String> {
		c.as_ref()
			.map(|c| {
				c.glyphs
					.iter()
					.map(|&g| order.name(g).into_owned())
					.collect()
			})
			.unwrap_or_default()
	};

	let table = &merged.require::<Math>()?.table;
	let constants = table.math_constants.as_ref().ok_or("no MathConstants")?;
	assert_eq!(constants.axis_height.value, 250);

	let glyph_info = table.math_glyph_info.as_ref().ok_or("no MathGlyphInfo")?;
	let italics = glyph_info
		.math_italics_correction_info
		.as_ref()
		.ok_or("no italics")?;
	assert_eq!(names(&italics.coverage), ["a", "b"]);
	assert_eq!(
		italics
			.italics_correction
			.iter()
			.map(|v| v.value)
			.collect::<Vec<_>>(),
		[10, 20]
	);
	let top_accent = glyph_info
		.math_top_accent_attachment
		.as_ref()
		.ok_or("no top accents")?;
	assert_eq!(names(&top_accent.top_accent_coverage), ["b"]);
	assert_eq!(
		top_accent
			.top_accent_attachment
			.iter()
			.map(|v| v.value)
			.collect::<Vec<_>>(),
		[250]
	);
	assert_eq!(names(&glyph_info.extended_shape_coverage), ["integral"]);
	let kern_info = glyph_info.math_kern_info.as_ref().ok_or("no kerns")?;
	assert_eq!(names(&kern_info.math_kern_coverage), ["a"]);
	assert_eq!(kern_info.math_kern_info_records.len(), 1);

	let variants = table.math_variants.as_ref().ok_or("no MathVariants")?;
	assert_eq!(variants.min_connector_overlap, 20);
	assert_eq!(names(&variants.vert_glyph_coverage), ["parenleft"]);
	let variant_glyphs = |c: &Option<Box<ot::MathGlyphConstruction>>| -> Vec<String> {
		c.as_ref()
			.map(|c| {
				c.math_glyph_variant_record
					.iter()
					.map(|r| order.name(r.variant_glyph).into_owned())
					.collect()
			})
			.unwrap_or_default()
	};
	assert_eq!(
		variant_glyphs(&variants.vert_glyph_construction[0]),
		["parenleft", "parenleft.size1"]
	);
	assert_eq!(names(&variants.horiz_glyph_coverage), ["minus"]);
	assert_eq!(
		variant_glyphs(&variants.horiz_glyph_construction[0]),
		["minus"]
	);
	Ok(())
}
