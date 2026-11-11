//! `Tests/ttLib/tables/E_B_D_T_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use common::{TestResult, get_xml};
use fontoxide_ttlib::Font;
use fontoxide_ttlib::tables::ebdt::{BitmapGlyph, GlyphMetrics, ParsedBitmapGlyph, StrikeData};
use fontoxide_ttlib::tables::eblc::{BigGlyphMetrics, SmallGlyphMetrics};
use fontoxide_ttlib::tables::{Ebdt, Eblc};

/// The "extfile" export writes relative to the working directory (L23), which
/// the tests of this file change; one at a time.
static CWD: Mutex<()> = Mutex::new(());

fn temp_dir(name: &str) -> PathBuf {
	let dir = std::env::temp_dir().join(format!("fontoxide-ebdt-{}-{name}", std::process::id()));
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir).unwrap();
	dir
}

fn small_metrics() -> GlyphMetrics {
	GlyphMetrics::Small(SmallGlyphMetrics {
		height: 1,
		width: 8,
		bearing_x: -1,
		bearing_y: 1,
		advance: 9,
	})
}

/// `_writeExtFileImageData` for glyphs named `names` (each with its data), all
/// in strike 0, run in `dir/out`. Returns the strike folder.
fn export(dir: &Path, glyphs: &[(&str, &[u8])]) -> fontoxide_ttlib::Result<PathBuf> {
	let out = dir.join("out");
	std::fs::create_dir_all(&out)?;
	let mut font = Font::new();
	let mut order = vec![".notdef".to_owned()];
	order.extend(glyphs.iter().map(|(n, _)| (*n).to_owned()));
	font.set_glyph_order(order);
	font.bitmap_glyph_data_format = "extfile".into();
	font.insert(Box::new(Eblc::default()));
	let mut dict = StrikeData::default();
	let mut ebdt = Ebdt {
		version: 2.0,
		..Default::default()
	};
	for (i, (_, data)) in glyphs.iter().enumerate() {
		ebdt.glyphs.push(BitmapGlyph::Parsed(ParsedBitmapGlyph {
			format: 1,
			metrics: small_metrics(),
			image_data: Some(data.to_vec()),
			components: None,
		}));
		dict.insert(i as u32 + 1, i);
	}
	ebdt.strike_data.push(Some(dict));

	let saved = std::env::current_dir()?;
	std::env::set_current_dir(&out)?;
	let result = get_xml(&ebdt, &mut font);
	std::env::set_current_dir(saved)?;
	result?;
	Ok(out.join("bitmaps").join("strike0"))
}

fn files(dir: &Path) -> Vec<PathBuf> {
	let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
		.unwrap()
		.map(|e| e.unwrap().path())
		.collect();
	files.sort();
	files
}

#[test]
fn test_extfile_export_contains_crafted_glyph_name() -> TestResult {
	let _lock = CWD.lock().unwrap_or_else(|e| e.into_inner());
	let tmp = temp_dir("crafted");
	let strike = export(&tmp, &[("../../../pwned", b"\xde\xad\xbe\xef")])?;
	assert!(!tmp.join("pwned.bin").exists());
	let written = files(&strike);
	assert_eq!(written.len(), 1);
	assert_eq!(std::fs::read(&written[0])?, b"\xde\xad\xbe\xef");
	std::fs::remove_dir_all(tmp)?;
	Ok(())
}

#[test]
fn test_extfile_export_plain_names_are_unchanged() -> TestResult {
	let _lock = CWD.lock().unwrap_or_else(|e| e.into_inner());
	let tmp = temp_dir("plain");
	let strike = export(&tmp, &[("uni0041", b"\xde\xad\xbe\xef")])?;
	assert!(strike.join("uni0041.bin").exists());
	std::fs::remove_dir_all(tmp)?;
	Ok(())
}

#[test]
fn test_extfile_export_distinct_names_dont_collide() -> TestResult {
	let _lock = CWD.lock().unwrap_or_else(|e| e.into_inner());
	// The last but two pair is ("a/b", _extFileName("a/b")).
	let pairs = [
		("a/b", "b"),
		("../b", "b"),
		("x/", "y/"),
		(".", ".."),
		("a/b", "%a%2Fb"),
		("%2Fb", "/b"),
		("%", ""),
	];
	for (i, (first, second)) in pairs.into_iter().enumerate() {
		let tmp = temp_dir(&format!("collide{i}"));
		let strike = export(&tmp, &[(first, b"FIRST"), (second, b"SECOND")])?;
		let mut written: Vec<Vec<u8>> = files(&strike)
			.iter()
			.map(|p| std::fs::read(p).unwrap())
			.collect();
		written.sort();
		assert_eq!(
			written,
			[b"FIRST".to_vec(), b"SECOND".to_vec()],
			"{first:?} {second:?}"
		);
		std::fs::remove_dir_all(tmp)?;
	}
	Ok(())
}

/// The `bitmap_glyph` fixture: (format, data, metrics) for each format.
fn bitmap_glyphs() -> Vec<(u16, Vec<u8>, GlyphMetrics)> {
	let mut cases = Vec::new();
	for format in [1u16, 2, 6, 7, 8, 9, 17, 18] {
		let (header, metrics): (Vec<u8>, GlyphMetrics) = if matches!(format, 1 | 2 | 8 | 17) {
			(vec![1, 8, 0xFF, 1, 9], small_metrics())
		} else {
			(
				vec![1, 8, 0xFF, 1, 9, 0xFC, 2, 10],
				GlyphMetrics::Big(BigGlyphMetrics {
					height: 1,
					width: 8,
					hori_bearing_x: -1,
					hori_bearing_y: 1,
					hori_advance: 9,
					vert_bearing_x: -4,
					vert_bearing_y: 2,
					vert_advance: 10,
				}),
			)
		};
		let payload: Vec<u8> = match format {
			8 => vec![0, 0, 1, 0, 1, 0xFE, 3],
			9 => vec![0, 1, 0, 1, 0xFE, 3],
			17 | 18 => vec![0, 0, 0, 1, 0x81],
			_ => vec![0x81],
		};
		cases.push((format, [header, payload].concat(), metrics));
	}
	cases
}

/// `test_missing_attribute_preserves_bitmap_metrics`: fontoxide parses glyphs
/// as a whole, so there is no attribute probing to restart a lazy
/// decompilation; the metrics and data of every format survive repeated use.
#[test]
fn test_missing_attribute_preserves_bitmap_metrics() -> TestResult {
	for (format, data, metrics) in bitmap_glyphs() {
		let glyph = BitmapGlyph::Raw {
			format,
			data: data.clone(),
		};
		for _ in 0..2 {
			let parsed = glyph.parsed()?;
			assert_eq!(parsed.metrics, metrics, "format {format}");
			assert_eq!(parsed.compile()?, data, "format {format}");
		}
	}
	Ok(())
}

#[test]
fn test_lazy_bitmap_metrics_and_edited_metrics_are_preserved() -> TestResult {
	for (format, data, metrics) in bitmap_glyphs() {
		let mut glyph = ParsedBitmapGlyph::parse(format, &data)?;
		assert_eq!(glyph.metrics, metrics, "format {format}");
		assert_eq!(glyph.compile()?, data, "format {format}");
		match &mut glyph.metrics {
			GlyphMetrics::Small(m) => m.width = 7,
			GlyphMetrics::Big(m) => m.width = 7,
			GlyphMetrics::None => unreachable!(),
		}
		let mut expected = data.clone();
		expected[1] = 7;
		assert_eq!(glyph.compile()?, expected, "format {format}");
	}
	Ok(())
}
