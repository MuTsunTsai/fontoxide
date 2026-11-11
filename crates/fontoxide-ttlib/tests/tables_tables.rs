//! `Tests/ttLib/tables/tables_test.py`: the TTX of each table of the fonts in
//! `data/aots` and `data/graphite`, compared with the expected `<font>.ttx.<table>`
//! files, both decompiled from the font and read back from the TTX.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use common::*;
use fontoxide_ttlib::font::xml_to_tag;
use fontoxide_ttlib::{Font, ReadOptions, TableKey, TtxOptions};

/// `(font file, table)` for every expected TTX file (`TESTS`).
fn cases() -> Vec<(PathBuf, TableKey, PathBuf)> {
	let mut cases = Vec::new();
	for dir in ["aots", "graphite"] {
		let mut entries: Vec<PathBuf> = std::fs::read_dir(tables_data(dir))
			.unwrap()
			.map(|e| e.unwrap().path())
			.collect();
		entries.sort();
		for path in entries {
			let name = path.file_name().unwrap().to_str().unwrap().to_owned();
			let Some((stem, table)) = name.split_once(".ttx.") else {
				continue;
			};
			if table.ends_with(".setup") {
				continue;
			}
			let font = ["otf", "ttf"]
				.iter()
				.map(|ext| path.with_file_name(format!("{stem}.{ext}")))
				.find(|p| p.exists())
				.unwrap();
			cases.push((font, xml_to_tag(table).unwrap(), path));
		}
	}
	assert!(cases.len() > 300, "{}", cases.len());
	cases
}

/// The TTX with the `ttLibVersion` attribute removed (`ttLibVersion_RE.sub`) and
/// universal newlines.
fn normalize(ttx: &str) -> String {
	let ttx = ttx.replace("\r\n", "\n");
	match ttx.find(" ttLibVersion=\"") {
		Some(i) => {
			let rest = &ttx[i + " ttLibVersion=\"".len()..];
			let end = rest.find('"').map_or(rest.len(), |j| j + 1);
			format!("{}{}", &ttx[..i], &rest[end..])
		}
		None => ttx,
	}
}

fn dump_ttx(font: &mut Font, table: TableKey) -> fontoxide_ttlib::Result<String> {
	let options = TtxOptions {
		tables: vec![table],
		..Default::default()
	};
	Ok(normalize(&font.to_ttx(&options)?))
}

fn implemented(table: TableKey) -> bool {
	match table {
		TableKey::Table(tag) => fontoxide_ttlib::table::is_implemented(tag),
		TableKey::GlyphOrder => true,
	}
}

fn check_all(check: impl Fn(&Path, TableKey, &str) -> Result<String, String>) {
	let mut failures = Vec::new();
	for (font, table, expected_path) in cases() {
		// Tables that fontoxide does not implement yet.
		if !implemented(table) {
			continue;
		}
		let expected = normalize(&std::fs::read_to_string(&expected_path).unwrap());
		match check(&font, table, &expected) {
			Ok(actual) if actual == expected => {}
			Ok(_) => failures.push(format!("{}: mismatch", expected_path.display())),
			Err(e) => failures.push(format!("{}: {e}", expected_path.display())),
		}
	}
	assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn test_xml_from_binary() {
	check_all(|font, table, _| {
		let data = std::fs::read(font).map_err(|e| e.to_string())?;
		let mut font = Font::read(data, ReadOptions::default()).map_err(|e| e.to_string())?;
		dump_ttx(&mut font, table).map_err(|e| e.to_string())
	});
}

#[test]
fn test_xml_from_xml() {
	check_all(|font_path, table, expected| {
		// The expected files have no glyph order, and fontoxide refers to glyphs by
		// ID, so the glyph order of the font is set first. (fontTools keeps the
		// glyph names it reads.)
		let data = std::fs::read(font_path).map_err(|e| e.to_string())?;
		let mut binary = Font::read(data, ReadOptions::default()).map_err(|e| e.to_string())?;
		let order = binary.glyph_order().map_err(|e| e.to_string())?;
		let mut font = Font::new();
		font.set_glyph_order(order.names().to_vec());
		font.import_ttx(expected).map_err(|e| e.to_string())?;
		// `<name>.ttx.<table>.setup`.
		let name = font_path.file_stem().unwrap().to_str().unwrap();
		let tag = match table {
			TableKey::Table(tag) => fontoxide_ttlib::font::tag_to_xml(tag),
			TableKey::GlyphOrder => "GlyphOrder".to_owned(),
		};
		let setup = font_path.with_file_name(format!("{name}.ttx.{tag}.setup"));
		if setup.exists() {
			let text = std::fs::read_to_string(&setup).map_err(|e| e.to_string())?;
			font.import_ttx(&text).map_err(|e| e.to_string())?;
		}
		dump_ttx(&mut font, table).map_err(|e| e.to_string())
	});
}
