//! Port of fontTools' `Tests/varLib/mutator_test.py`.
//!
//! The fonts of `test_mutator_null_condition_set` and
//! `test_mutator_null_substitution_first_match` are built by
//! `tools/reference/mutator_fixtures.py`, as FontBuilder and featureVars are not
//! ported.

mod common;

use std::path::PathBuf;

use common::{TestResult, font_from_ttx, tests_path, to_ttx};
use fontoxide_ttlib::tables::Gsub;
use fontoxide_ttlib::{Font, ReadOptions, Tag, WriteOptions};
use fontoxide_varlib::mutator::{
	instantiate_variable_font, instantiate_variable_font_copy, main as mutator,
};

/// A unique directory under the temporary one, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
	fn new(name: &str) -> TestResult<Self> {
		let path =
			std::env::temp_dir().join(format!("fontoxide-mutator-{}-{name}", std::process::id()));
		std::fs::create_dir_all(&path)?;
		Ok(TempDir(path))
	}

	fn file(&self, name: &str) -> String {
		self.0.join(name).to_string_lossy().into_owned()
	}
}

impl Drop for TempDir {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

fn data(name: &str) -> String {
	tests_path(&format!("varLib/data/{name}"))
}

fn test_output(name: &str) -> String {
	tests_path(&format!("varLib/data/test_results/{name}"))
}

fn fixture(name: &str) -> String {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data/mutator")
		.join(name)
		.to_string_lossy()
		.into_owned()
}

/// `read_ttx`: the lines, without the attributes of `ttFont` and trailing spaces.
fn read_ttx(text: &str) -> Vec<String> {
	text.replace("\r\n", "\n")
		.lines()
		.map(|line| {
			if line.starts_with("<ttFont ") {
				"<ttFont>".to_owned()
			} else {
				line.trim_end().to_owned()
			}
		})
		.collect()
}

/// `expect_ttx`.
fn expect_ttx(font: &mut Font, expected: &str, tables: &[&str]) -> TestResult {
	let actual = read_ttx(&to_ttx(font, tables)?);
	let expected_lines = read_ttx(&std::fs::read_to_string(expected)?);
	for (i, (a, e)) in actual.iter().zip(&expected_lines).enumerate() {
		assert_eq!(a, e, "{expected}, line {}", i + 1);
	}
	assert_eq!(actual.len(), expected_lines.len(), "{expected}");
	Ok(())
}

/// `compile_font`: compiles a TTX file as `TTFont(recalcBBoxes=False,
/// recalcTimestamp=False)` does.
fn compile_font(path: &str, out: &str) -> TestResult {
	let mut font = Font::new();
	font.recalc_bboxes = false;
	font.recalc_timestamp = false;
	font.import_ttx(&std::fs::read_to_string(path)?)?;
	std::fs::write(out, font.write(&WriteOptions::default())?)?;
	Ok(())
}

/// The tags of every table but `head`, as `saveXML` names them.
fn tables_but_head(font: &mut Font) -> Vec<String> {
	let mut tables = vec!["GlyphOrder".to_owned()];
	tables.extend(
		font.tags()
			.into_iter()
			.filter(|t| *t != Tag::new(b"head"))
			.map(|t| t.to_string()),
	);
	tables
}

/// Runs the command line on a compiled TTX file, and compares the instance with an
/// expected TTX file.
fn run_main(ttx: &str, name: &str, args: &[&str], expected: &str) -> TestResult {
	let dir = TempDir::new(name)?;
	let varfont_path = dir.file(&format!("{name}.ttf"));
	compile_font(&data(ttx), &varfont_path)?;
	let mut argv = vec![varfont_path.as_str()];
	argv.extend_from_slice(args);
	assert_eq!(mutator(&argv)?, 0);

	let instfont_path = dir.file(&format!("{name}-instance.ttf"));
	let mut instfont = Font::read(std::fs::read(&instfont_path)?, ReadOptions::default())?;
	let tables = tables_but_head(&mut instfont);
	let tables: Vec<&str> = tables.iter().map(String::as_str).collect();
	expect_ttx(&mut instfont, &test_output(expected), &tables)
}

#[test]
fn test_requires_fvar_table() {
	let error = instantiate_variable_font(&mut Font::new(), &[], true).expect_err("no fvar");
	assert!(error.to_string().contains("Missing required table fvar"));
}

#[test]
fn test_varlib_mutator_getvar_ttf() -> TestResult {
	run_main(
		"master_ttx_getvar_ttf/Mutator_Getvar.ttx",
		"Mutator_Getvar",
		&["wdth=80", "ASCN=628"],
		"Mutator_Getvar-instance.ttx",
	)
}

#[test]
fn test_varlib_mutator_iup_ttf() -> TestResult {
	run_main(
		"master_ttx_varfont_ttf/Mutator_IUP.ttx",
		"Mutator_IUP",
		&["wdth=80", "ASCN=628"],
		"Mutator_IUP-instance.ttx",
	)
}

#[test]
fn test_varlib_mutator_cff2() -> TestResult {
	let dir = TempDir::new("cff2")?;
	let varfont_path = dir.file("TestCFF2VF.otf");
	compile_font(
		&data("master_ttx_varfont_otf/TestCFF2VF.ttx"),
		&varfont_path,
	)?;
	let mut varfont = Font::read(std::fs::read(&varfont_path)?, ReadOptions::default())?;
	let mut new_font =
		instantiate_variable_font_copy(&mut varfont, &[("wght".to_owned(), 200.0)], true)?;
	expect_ttx(
		&mut new_font,
		&test_output("InterpolateTestCFF2VF.ttx"),
		&["hmtx", "CFF2"],
	)
}

/// The lookup indices of the first feature of `GSUB`.
fn first_feature_lookups(font: &mut Font) -> TestResult<Vec<u16>> {
	let gsub = &font.require::<Gsub>()?.table;
	assert!(gsub.feature_variations.is_none());
	let list = gsub.feature_list.as_deref().ok_or("no FeatureList")?;
	let feature = list.feature_record[0]
		.feature
		.as_deref()
		.ok_or("no Feature")?;
	Ok(feature.lookup_list_index.clone())
}

#[test]
fn test_mutator_null_condition_set() -> TestResult {
	let mut font = font_from_ttx(&fixture("null_condition_set.ttx"))?;
	instantiate_variable_font(&mut font, &[("wght".to_owned(), 700.0)], true)?;
	assert_eq!(first_feature_lookups(&mut font)?, [0]);
	let ttx = to_ttx(&mut font, &["GSUB"])?;
	assert!(ttx.contains(r#"<Substitution in="A" out="A.alt"/>"#));
	Ok(())
}

#[test]
fn test_mutator_null_substitution_first_match() -> TestResult {
	let mut font = font_from_ttx(&fixture("null_substitution_first_match.ttx"))?;
	instantiate_variable_font(&mut font, &[("wght".to_owned(), 700.0)], true)?;
	assert_eq!(first_feature_lookups(&mut font)?, Vec::<u16>::new());
	Ok(())
}

/// The instance can be saved, and the variable tables are gone.
#[test]
fn instance_has_no_variable_tables() -> TestResult {
	let mut font = font_from_ttx(&fixture("null_condition_set.ttx"))?;
	instantiate_variable_font(&mut font, &[], true)?;
	assert!(!font.contains(Tag::new(b"fvar")));
	let font = Font::read(
		font.write(&WriteOptions::default())?,
		ReadOptions::default(),
	)?;
	assert!(!font.contains(Tag::new(b"fvar")));
	Ok(())
}

#[test]
fn invalid_location_argument() {
	for arg in ["wght", "wght=1=2", "wghts=1", "wght=x"] {
		let error = mutator(&["font.ttf", arg]).expect_err("invalid");
		assert!(
			error
				.to_string()
				.contains("invalid location argument format"),
			"{arg}: {error}"
		);
	}
}
