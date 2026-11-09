//! Helpers of the tests ported from fontTools' `Tests/varLib`.

#![allow(dead_code)]

use std::path::PathBuf;

use fontoxide_ttlib::{Font, ReadOptions, TableKey, Tag, TtxOptions, WriteOptions};
use fontoxide_varlib::instancer::{
	AxisLimits, AxisTriple, InstancerOptions, instantiate_variable_font,
};

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// A file of fontTools' `Tests` directory.
pub fn tests_path(relative: &str) -> String {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests")
		.join(relative)
		.to_string_lossy()
		.into_owned()
}

/// A file of `Tests/varLib/instancer/data`.
pub fn instancer_data(name: &str) -> String {
	tests_path(&format!("varLib/instancer/data/{name}"))
}

/// A font from a TTX file (`TTFont(recalcTimestamp=False); importXML(path)`).
pub fn font_from_ttx(path: &str) -> TestResult<Font> {
	let mut font = Font::new();
	font.recalc_timestamp = false;
	font.import_ttx(&std::fs::read_to_string(path)?)?;
	Ok(font)
}

/// `TTFont(path, recalcTimestamp=False)`.
pub fn open(path: &str) -> TestResult<Font> {
	Ok(Font::read(
		std::fs::read(path)?,
		ReadOptions {
			recalc_timestamp: false,
			..Default::default()
		},
	)?)
}

/// Saves a font to memory and reloads it (`font.save(buf); TTFont(buf)`).
pub fn reload(font: &mut Font) -> TestResult<Font> {
	Ok(Font::read(
		font.write(&WriteOptions::default())?,
		ReadOptions {
			recalc_timestamp: false,
			..Default::default()
		},
	)?)
}

/// `AxisTriple(value, value, value)`.
pub fn pin(value: f64) -> Option<AxisTriple> {
	Some(AxisTriple::pin(value))
}

/// `(minimum, maximum)`.
pub fn range(minimum: f64, maximum: f64) -> Option<AxisTriple> {
	Some(AxisTriple::range(minimum, maximum).expect("sorted"))
}

/// `(minimum, default, maximum)`.
pub fn triple(minimum: f64, default: f64, maximum: f64) -> Option<AxisTriple> {
	Some(AxisTriple::new(Some(minimum), Some(default), Some(maximum)).expect("sorted"))
}

/// Axis limits from `(tag, limit)` pairs.
pub fn limits(pairs: &[(&str, Option<AxisTriple>)]) -> AxisLimits {
	pairs
		.iter()
		.map(|(tag, limit)| ((*tag).to_owned(), *limit))
		.collect()
}

/// `instantiateVariableFont(font, limits, inplace=True)`.
pub fn instantiate(font: &mut Font, pairs: &[(&str, Option<AxisTriple>)]) -> TestResult {
	instantiate_variable_font(font, &limits(pairs), &InstancerOptions::default())?;
	Ok(())
}

fn key(name: &str) -> TableKey {
	if name == "GlyphOrder" {
		TableKey::GlyphOrder
	} else {
		TableKey::Table(Tag::from_str_padded(name).expect("a table tag"))
	}
}

/// The TTX of some tables, or all if there are none (`font.saveXML(tables=tables)`).
pub fn to_ttx(font: &mut Font, tables: &[&str]) -> TestResult<String> {
	Ok(font.to_ttx(&TtxOptions {
		tables: tables.iter().map(|t| key(t)).collect(),
		newline: "\n".into(),
		..Default::default()
	})?)
}

fn remove_between(s: &str, start: &str, end: &str) -> String {
	let mut out = String::with_capacity(s.len());
	let mut rest = s;
	while let Some(i) = rest.find(start) {
		let after = &rest[i + start.len()..];
		let Some(j) = after.find(end) else {
			break;
		};
		out.push_str(&rest[..i]);
		rest = &after[j + end.len()..];
	}
	out.push_str(rest);
	out
}

/// `_dump_ttx` of `instancer_test.py`: the lines of the TTX, without the items that vary.
pub fn strip_ttx(ttx: &str) -> Vec<String> {
	let s = ttx.replace("\r\n", "\n");
	let s = remove_between(&s, " ttLibVersion=\"", "\"");
	let s = remove_between(&s, " sfntVersion=\"", "\"");
	let s = remove_between(&s, "<checkSumAdjustment value=\"", "\"/>");
	let s = remove_between(&s, "<modified value=\"", "\"/>");
	let s = remove_between(&s, "<created value=\"", "\"/>");
	s.split_inclusive('\n').map(str::to_owned).collect()
}

/// Compares some tables of a font with an expected TTX file.
pub fn expect_ttx(font: &mut Font, expected: &str, tables: &[&str]) -> TestResult {
	let actual = strip_ttx(&to_ttx(font, tables)?);
	let expected_lines = strip_ttx(&std::fs::read_to_string(expected)?);
	if actual != expected_lines {
		for (i, (a, e)) in actual.iter().zip(&expected_lines).enumerate() {
			if a != e {
				panic!("{expected}, line {}:\n  actual   {a}  expected {e}", i + 1);
			}
		}
		panic!(
			"{expected}: {} lines, expected {}",
			actual.len(),
			expected_lines.len()
		);
	}
	Ok(())
}
