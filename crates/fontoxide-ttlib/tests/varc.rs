//! Round trips of `VARC` tables in fontTools' test fonts, compared alone, and
//! `Tests/ttLib/tables/V_A_R_C_test.py`.

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use fontoxide_misc::text_tools::de_hex_str;
use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types::{MultiVarData, SparseVarRegion, SparseVarRegionList};
use fontoxide_ttlib::ot::{OtReader, WriterGraph};
use fontoxide_ttlib::tables::otl::Varc;
use fontoxide_ttlib::{
	Font, ReadOptions, Result, TableKey, Tag, TtxImportOptions, TtxOptions, WriteOptions,
};

const VARC: Tag = Tag::new(b"VARC");

const FONTS: &[&str] = &[
	"ttLib/data/varc-6868.ttf",
	"ttLib/data/varc-ac00-ac01.ttf",
	"ttLib/data/varc-ac01-conditional.ttf",
	"ttLib/data/varc-static-gvar.ttf",
	"cffLib/data/varc-short-cff2.otf",
];

fn fonts() -> Vec<(PathBuf, Vec<u8>)> {
	let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../upstream/fonttools/Tests");
	let fonts: Vec<_> = FONTS
		.iter()
		.filter_map(|f| {
			let path = dir.join(f);
			std::fs::read(&path).ok().map(|data| (path, data))
		})
		.collect();
	assert_eq!(
		fonts.len(),
		FONTS.len(),
		"fontTools' test fonts are missing"
	);
	fonts
}

fn read(data: Vec<u8>) -> Result<Font> {
	Font::read(
		data,
		ReadOptions {
			recalc_timestamp: false,
			..Default::default()
		},
	)
}

#[test]
fn binary_round_trip() -> Result<()> {
	for (path, data) in fonts() {
		let mut font = read(data)?;
		let original = font.table_data(VARC)?;
		font.load(VARC)?;
		assert_eq!(font.table_data(VARC)?, original, "{}", path.display());
	}
	Ok(())
}

#[test]
fn ttx_round_trip() -> Result<()> {
	for (path, data) in fonts() {
		let mut font = read(data)?;
		let original = font.table_data(VARC)?;
		let ttx = font.to_ttx(&TtxOptions::default())?;
		let mut imported = Font::new();
		imported.import_ttx_with(
			&ttx,
			&TtxImportOptions {
				skip_unimplemented: true,
			},
		)?;
		assert_eq!(imported.table_data(VARC)?, original, "{}", path.display());
	}
	Ok(())
}

fn hex(data: &str) -> Vec<u8> {
	de_hex_str(data).unwrap()
}

fn decompile<T: OtTable>(data: &[u8], font: &mut Font) -> Result<T> {
	T::decompile(&mut OtReader::new(data, VARC), font)
}

fn compile<T: OtTable>(table: &T, font: &mut Font) -> Result<Vec<u8>> {
	let mut graph = WriterGraph::new(VARC);
	table.compile(&mut graph.root(), font)?;
	graph.get_all_data()
}

fn region_axis(regions: &SparseVarRegionList, region: usize, axis: usize) -> (u16, f64) {
	let axis = regions.region[region]
		.as_ref()
		.unwrap()
		.sparse_var_region_axis[axis]
		.as_ref()
		.unwrap();
	(axis.axis_index, axis.peak_coord)
}

/// `test_sparse_region_axis_offsets`; fontoxide has no lazy loading.
#[test]
fn test_sparse_region_axis_offsets() -> Result<()> {
	// Two differently sized region headers share one axis record. Axis offsets
	// are relative to their own region, not the enclosing region list.
	let data = hex(
		"00000002 0000000C 00000012 		0001 00000010 		0002 0000000A 00000012 		0000 0000 4000 4000 		0001 C000 C000 0000",
	);
	let mut font = Font::new();
	let regions: SparseVarRegionList = decompile(&data, &mut font)?;
	assert_eq!(regions.region_count, Some(2));
	assert_eq!(region_axis(&regions, 0, 0).1, 1.0);
	assert_eq!(region_axis(&regions, 1, 0).1, 1.0);
	assert_eq!(region_axis(&regions, 1, 1), (1, -1.0));

	let reloaded: SparseVarRegionList = decompile(&compile(&regions, &mut font)?, &mut font)?;
	assert_eq!(region_axis(&reloaded, 1, 1).1, -1.0);
	assert_eq!(region_axis(&reloaded, 0, 0).1, 1.0);
	Ok(())
}

#[test]
fn test_sparse_region_count_uint32() -> Result<()> {
	let mut font = Font::new();
	// `buildSparseVarRegionList([{}], [])`, with the region repeated.
	let region = SparseVarRegion {
		sparse_region_count: Some(0),
		sparse_var_region_axis: Vec::new(),
	};
	let regions = SparseVarRegionList {
		region_count: Some(1),
		region: vec![Some(Box::new(region)); 0x10000],
	};
	let data = compile(&regions, &mut font)?;
	assert_eq!(data[..4], [0, 1, 0, 0]);
	let reloaded: SparseVarRegionList = decompile(&data, &mut font)?;
	assert_eq!(reloaded.region_count, Some(0x10000));
	assert_eq!(
		reloaded.region[0xFFFF]
			.as_ref()
			.unwrap()
			.sparse_region_count,
		Some(0)
	);
	Ok(())
}

/// `test_delta_set_index_offset`; fontoxide has no lazy loading.
#[test]
fn test_delta_set_index_offset() -> Result<()> {
	// The INDEX is separated from the MultiVarData header by unrelated bytes.
	let data =
		hex("010002 0000 0001 00000013 DEADBEEF DEADBEEF 		00000002 01 01 05 05 01 01 FE 80");
	let mut font = Font::new();
	let mut table: MultiVarData = decompile(&data, &mut font)?;
	assert_eq!(table.item, [vec![1, -2, 0], vec![]]);
	table.item = (0..6).flat_map(|_| table.item.clone()).collect();
	let reloaded: MultiVarData = decompile(&compile(&table, &mut font)?, &mut font)?;
	let expected: Vec<Vec<i64>> = (0..6).flat_map(|_| [vec![1, -2, 0], vec![]]).collect();
	assert_eq!(reloaded.item, expected);
	Ok(())
}

#[test]
fn test_empty_delta_set_index_offset() -> Result<()> {
	let mut font = Font::new();
	let table = MultiVarData {
		format: 1,
		..Default::default()
	};
	let data = compile(&table, &mut font)?;
	assert_eq!(data, hex("01 0000 00000007 00000000"));
	let reloaded: MultiVarData = decompile(&data, &mut font)?;
	assert!(reloaded.item.is_empty());
	Ok(())
}

fn upstream_font(name: &str) -> Result<Font> {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttLib/data")
		.join(name);
	read(std::fs::read(path)?)
}

fn coverage_names(font: &mut Font) -> Result<Vec<String>> {
	let glyphs = font
		.require::<Varc>()?
		.coverage
		.as_ref()
		.unwrap()
		.glyphs
		.clone();
	glyphs.into_iter().map(|g| font.glyph_name(g)).collect()
}

/// `VarCompositeTest::test_basic`.
#[test]
fn var_composite_test_basic() -> Result<()> {
	let mut font = upstream_font("varc-ac00-ac01.ttf")?;
	assert_eq!(
		coverage_names(&mut font)?,
		[
			"uniAC00",
			"uniAC01",
			"glyph00003",
			"glyph00005",
			"glyph00007",
			"glyph00008",
			"glyph00009",
		]
	);
	let mut font = upstream_font("varc-6868.ttf")?;
	assert_eq!(
		coverage_names(&mut font)?,
		["uni6868", "glyph00002", "glyph00005", "glyph00007"]
	);
	Ok(())
}

/// `VarCompositeTest::test_roundtrip`.
#[test]
fn var_composite_test_roundtrip() -> Result<()> {
	for (name, skip) in [
		("varc-ac00-ac01.ttf", &["head", "maxp", "hhea"][..]),
		(
			"varc-6868.ttf",
			&["head", "maxp", "hhea", "name", "fvar"][..],
		),
	] {
		let mut font = upstream_font(name)?;
		// `font.keys()` starts with the glyph order.
		let mut tables = vec![TableKey::GlyphOrder];
		tables.extend(
			font.tags()
				.into_iter()
				.filter(|t| !skip.iter().any(|s| t.0 == s.as_bytes()))
				.map(TableKey::Table),
		);
		let options = TtxOptions {
			tables: tables.clone(),
			..Default::default()
		};
		let xml = font.to_ttx(&TtxOptions::default())?;
		let xml1 = font.to_ttx(&options)?;
		let mut font = Font::from_ttx(&xml)?;
		let ttf = font.write(&WriteOptions::default())?;
		let mut font = read(ttf)?;
		let xml2 = font.to_ttx(&options)?;
		assert_eq!(xml1, xml2, "{name}");
	}
	Ok(())
}
