//! `Tests/ttLib/ttFont_test.py`.
//!
//! The tests of the uppercase companion tables are in `uppercase_tables.rs`.

#![allow(non_snake_case)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use fontoxide_ttlib::ot::types::LookupSubTable;
use fontoxide_ttlib::tables::cmap::DEFAULT_CMAP_PREFERENCES;
use fontoxide_ttlib::tables::{Cmap, CmapSubtable, Dmap, Gsub, Hmtx, Post};
use fontoxide_ttlib::{Error, Font, ReadOptions, TableKey, Tag, TtxOptions, WriteOptions};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn data_path(name: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttLib/data")
		.join(name)
}

fn read_font(name: &str) -> fontoxide_ttlib::Result<Font> {
	Font::read(std::fs::read(data_path(name))?, ReadOptions::default())
}

/// `TTFont()` followed by `importXML(path)`.
fn import_ttx(name: &str) -> fontoxide_ttlib::Result<Font> {
	Font::from_ttx(&std::fs::read_to_string(data_path(name))?)
}

fn names(names: &[&str]) -> Vec<String> {
	names.iter().map(|n| (*n).to_owned()).collect()
}

fn subtable(format: u16, mapping: &[(u32, u32)]) -> CmapSubtable {
	let mut table = CmapSubtable::new(format);
	table.platform_id = 3;
	table.plat_enc_id = if format == 4 { 1 } else { 10 };
	table.language = Some(0);
	table.cmap = Arc::new(mapping.iter().copied().collect());
	table
}

#[test]
fn test_getBestCmap_DMAP_zero_fallback() -> TestResult {
	// The glyph names do not matter here: fontoxide maps to glyph IDs.
	for glyph_zero in [".notdef", "zero"] {
		for dmap_format in [4, 12, 13] {
			let mut font = Font::new();
			font.set_glyph_order(names(&[glyph_zero, "baseA", "baseB", "dmapB"]));
			font.insert(Box::new(Cmap {
				table_version: 0,
				tables: vec![subtable(4, &[(0x41, 1), (0x42, 2)])],
			}));
			font.insert(Box::new(Dmap(Cmap {
				table_version: 0,
				tables: vec![subtable(dmap_format, &[(0x41, 0), (0x42, 3)])],
			})));

			let best = font.get_best_cmap(&DEFAULT_CMAP_PREFERENCES)?;
			assert_eq!(best, Some([(0x41, 1), (0x42, 3)].into()));
			let dmap = font.require::<Dmap>()?;
			assert_eq!(
				dmap.0.get_best_cmap(&DEFAULT_CMAP_PREFERENCES),
				Some(&[(0x41, 0), (0x42, 3)].into())
			);
			font.remove(Tag::new(b"cmap"));
			let best = font.get_best_cmap(&DEFAULT_CMAP_PREFERENCES)?;
			assert_eq!(best, Some([(0x42, 3)].into()));
			font.require::<Dmap>()?.0.tables[0].cmap = Arc::new([(0x41, 0)].into());
			let best = font.get_best_cmap(&DEFAULT_CMAP_PREFERENCES)?;
			assert_eq!(best, Some(BTreeMap::new()));
		}
	}
	Ok(())
}

const TTX_TTF: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ttFont sfntVersion="\x00\x01\x00\x00" ttLibVersion="4.9.0">
  <hmtx>
    <mtx name=".notdef" width="300" lsb="0"/>
  </hmtx>
</ttFont>
"#;

/// fontoxide's `hmtx` is keyed by glyph ID, so the glyph order is needed to
/// import it (fontTools keys it by glyph name).
const TTX_OTF: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ttFont sfntVersion="OTTO" ttLibVersion="4.9.0">
  <GlyphOrder>
    <GlyphID id="0" name=".notdef"/>
  </GlyphOrder>
  <hmtx>
    <mtx name=".notdef" width="300" lsb="0"/>
  </hmtx>
</ttFont>
"#;

#[test]
fn test_sfntVersionFromTTX() -> TestResult {
	let mut font = Font::new();
	assert_eq!(font.sfnt_version, Tag::new(b"\0\x01\0\0"));
	// The font is empty: the TTX file determines sfntVersion.
	font.import_ttx(TTX_OTF)?;
	assert_eq!(font.sfnt_version, Tag::new(b"OTTO"));
	// The font is not empty: the sfntVersion of the TTX file is ignored.
	font.import_ttx(TTX_TTF)?;
	assert_eq!(font.sfnt_version, Tag::new(b"OTTO"));
	Ok(())
}

/// `normalize_TTX`: `re.sub(pattern + '.*"', "", line)` for each pattern; `.*` is
/// greedy, so this removes up to the last quote of the line.
fn normalize_ttx(text: &str) -> Vec<String> {
	text.lines()
		.map(|line| {
			let mut line = line.to_owned();
			for pattern in [
				" ttLibVersion=\"",
				"checkSumAdjustment value=\"",
				"modified value=\"",
			] {
				if let Some(start) = line.find(pattern) {
					let after = start + pattern.len() - 1;
					if let Some(end) = line[after + 1..].rfind('"') {
						line.replace_range(start..after + 1 + end + 1, "");
					}
				}
			}
			line
		})
		.collect()
}

#[test]
fn test_virtualGlyphId() -> TestResult {
	let ttx_text = std::fs::read_to_string(data_path("TestVGID-Regular.ttx"))?;
	let xml = normalize_ttx(&ttx_text);
	let otf = read_font("TestVGID-Regular.otf")?;
	let ttx = Font::from_ttx(&ttx_text)?;

	for mut font in [otf, ttx] {
		// "glyph00453" is a virtual glyph, beyond the glyph order.
		assert!(font.glyph_order()?.len() <= 453);
		check_vgid_lookup(&font.require::<Gsub>()?.table)?;

		let out = font.to_ttx(&TtxOptions::default())?;
		assert_eq!(normalize_ttx(&out), xml);
	}
	Ok(())
}

fn check_vgid_lookup(gsub: &fontoxide_ttlib::ot::types::GSUB) -> TestResult {
	{
		let lookups = &gsub.lookup_list.as_ref().ok_or("no LookupList")?.lookup;
		assert_eq!(lookups.len(), 37);
		let lookup = lookups[32].as_ref().ok_or("no lookup")?;
		assert_eq!(lookup.lookup_type, 8);
		let Some(LookupSubTable::ReverseChainSingleSubst(subtable)) =
			lookup.sub_table[0].as_deref()
		else {
			panic!("not a ReverseChainSingleSubst");
		};
		let fontoxide_ttlib::ot::types::ReverseChainSingleSubst::Format1(subtable) = subtable
		else {
			panic!("not format 1");
		};
		assert_eq!(subtable.look_ahead_coverage.len(), 1);
		let lookahead = subtable.look_ahead_coverage[0]
			.as_ref()
			.ok_or("no coverage")?;
		assert_eq!(lookahead.glyphs.len(), 46);
		assert!(lookahead.glyphs.contains(&453));
	}
	Ok(())
}

/// Captures the log records of the test that is running, as pytest's `caplog`.
struct CapLog {
	records: Mutex<Vec<(log::Level, String)>>,
}

impl log::Log for CapLog {
	fn enabled(&self, _: &log::Metadata<'_>) -> bool {
		true
	}

	fn log(&self, record: &log::Record<'_>) {
		if let Ok(mut records) = self.records.lock() {
			records.push((record.level(), record.args().to_string()));
		}
	}

	fn flush(&self) {}
}

static CAPLOG: CapLog = CapLog {
	records: Mutex::new(Vec::new()),
};

#[test]
fn test_getGlyphOrder_not_true_post_format_1() -> TestResult {
	// Only this test logs at the warning level here.
	let _ = log::set_logger(&CAPLOG);
	log::set_max_level(log::LevelFilter::Warn);
	let mut font = read_font("bogus_post_format_1.ttf")?;
	// fontTools' `hmtx` is keyed by glyph name, so decompiling it derives the
	// glyph order; fontoxide's is keyed by glyph ID.
	let order_len = font.glyph_order()?.len();
	let hmtx = font.require::<Hmtx>()?;
	// More names than the 258 of the standard glyph order.
	assert!(hmtx.metrics.len() > 258);
	assert_eq!(order_len, hmtx.metrics.len());
	let records = CAPLOG.records.lock().map_err(|e| e.to_string())?;
	// The other tests of this file may log too.
	let warned = records.iter().any(|(level, message)| {
		*level == log::Level::Warn && message.contains("Not enough names found in the 'post' table")
	});
	assert!(warned, "{records:?}");
	Ok(())
}

/// The part of `test_ensureDecompiled` that does not depend on lazy loading,
/// which fontoxide does not have: tables are only decompiled on request, and
/// `ensure_decompiled` decompiles them all. (The OpenType Layout features that
/// the test adds with feaLib are left out; the font has its own subtables.)
#[test]
fn test_ensureDecompiled() -> TestResult {
	let mut font = import_ttx("TestTTF-Regular.ttx")?;
	let data = font.write(&WriteOptions::default())?;
	let mut font = Font::read(data, ReadOptions::default())?;
	let tags = font.tags();
	assert!(!tags.is_empty());
	for &tag in &tags {
		assert!(!font.is_loaded(tag), "{tag}");
	}
	font.ensure_decompiled()?;
	for &tag in &tags {
		assert!(font.is_loaded(tag), "{tag}");
	}
	Ok(())
}

#[test]
fn test_font_normalizeLocation() -> TestResult {
	let mut font = import_ttx("TestTTF_normalizeLocation.ttx")?;
	let cases: &[(Option<f64>, f64)] = &[
		(None, 0.0),
		(Some(100.0), -1.0),
		(Some(250.0), -0.75),
		(Some(400.0), 0.0),
		(Some(550.0), 0.75),
		(Some(625.0), 0.875),
		(Some(700.0), 1.0),
	];
	for &(user, expected) in cases {
		let location: BTreeMap<String, f64> =
			user.map(|v| ("wght".to_owned(), v)).into_iter().collect();
		let normalized = font.normalize_location(&location)?;
		assert_eq!(
			normalized,
			[("wght".to_owned(), expected)].into(),
			"{user:?}"
		);
	}
	Ok(())
}

#[test]
fn test_font_normalizeLocation_no_VF() {
	let mut font = Font::new();
	let error = font
		.normalize_location(&BTreeMap::new())
		.expect_err("not a variable font");
	assert!(matches!(error, Error::TtLib(_)));
	assert!(error.to_string().contains("Not a variable font"), "{error}");
}

#[test]
fn test_getGlyphID() -> TestResult {
	let mut font = import_ttx("TestTTF-Regular.ttx")?;
	assert_eq!(font.glyph_id("space")?, 3);
	// A virtual glyph.
	assert_eq!(font.glyph_id("glyph12345")?, 12345);
	assert!(matches!(
		font.glyph_id("non_existent"),
		Err(Error::GlyphNotFound(_))
	));
	assert!(matches!(
		font.glyph_id("glyph_prefix_but_invalid_id"),
		Err(Error::GlyphNotFound(_))
	));
	Ok(())
}

#[test]
fn test_duplicate_glyph_names() -> TestResult {
	for file_name in ["duplicate_glyph_name.ttf", "duplicate_glyph_name.otf"] {
		let mut font = read_font(file_name)?;
		assert_eq!(
			font.glyph_order()?.names(),
			names(&[".notdef", "space", "A", "A.2", "A.1"])
		);
		if !font.contains(Tag::new(b"CFF ")) {
			let post = font.require::<Post>()?;
			assert_eq!(
				post.mapping,
				Some([("A.2".to_owned(), "A".to_owned())].into())
			);
		}
	}
	Ok(())
}

#[test]
fn test_saveXML_default_dumps_all_tables() -> TestResult {
	let mut font = import_ttx("TestTTF-Regular.ttx")?;
	let xml = font.to_ttx(&TtxOptions::default())?;
	assert!(xml.contains("<head>"));
	assert!(xml.contains("<hhea>"));
	Ok(())
}

#[test]
fn test_saveXML_specific_tables() -> TestResult {
	let mut font = import_ttx("TestTTF-Regular.ttx")?;
	let xml = font.to_ttx(&TtxOptions {
		tables: vec![TableKey::Table(Tag::new(b"head"))],
		..Default::default()
	})?;
	assert!(xml.contains("<head>"));
	assert!(!xml.contains("<hhea>"));
	Ok(())
}

#[test]
fn test_saveXML_skipTables() -> TestResult {
	let mut font = import_ttx("TestTTF-Regular.ttx")?;
	let xml = font.to_ttx(&TtxOptions {
		skip_tables: vec![TableKey::Table(Tag::new(b"head"))],
		..Default::default()
	})?;
	assert!(!xml.contains("<head>"));
	assert!(xml.contains("<hhea>"));
	Ok(())
}
