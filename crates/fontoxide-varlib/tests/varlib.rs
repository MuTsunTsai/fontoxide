//! `Tests/varLib/varLib_test.py`.

#![allow(clippy::type_complexity, clippy::field_reassign_with_default)]

mod common;

use std::path::{Path, PathBuf};

use common::{TestResult, tests_path};
use fontoxide_designspacelib::{AxisDescriptor, DesignSpaceDocument, Number, SourceDescriptor};
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::{Gpos, Os2, Post};
use fontoxide_ttlib::{Font, ReadOptions, TableKey, Tag, TtxOptions, WriteOptions};
use fontoxide_varlib::build::{
	Axes, BuildOptions, SourceFonts, add_avar, add_gsub_feature_variations_from_designspace, build,
	build_many, identity_finder, load_designspace, load_masters, set_default_weight_width_slant,
};
use fontoxide_varlib::errors::{MergeErrorKind, VarLibError};
use indexmap::IndexMap;

/// A unique directory under the temporary one, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
	fn new(name: &str) -> TestResult<Self> {
		// Tests run in parallel and may share a name (two builds of the same
		// expected font), so each directory gets a number of its own.
		static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
		let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
		let path = std::env::temp_dir().join(format!(
			"fontoxide-varlib-{}-{n}-{name}",
			std::process::id()
		));
		let _ = std::fs::remove_dir_all(&path);
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

/// `get_test_input`.
fn data(name: &str) -> String {
	tests_path(&format!("varLib/data/{name}"))
}

/// `get_test_output`.
fn test_output(name: &str) -> String {
	tests_path(&format!("varLib/data/test_results/{name}"))
}

/// `get_file_list(folder, suffix, prefix)`.
fn file_list(folder: &str, suffix: &str, prefix: &str) -> TestResult<Vec<String>> {
	let mut out = Vec::new();
	for entry in std::fs::read_dir(folder)? {
		let name = entry?.file_name().to_string_lossy().into_owned();
		if name.starts_with(prefix) && name.ends_with(suffix) {
			out.push(Path::new(folder).join(&name).to_string_lossy().into_owned());
		}
	}
	out.sort();
	Ok(out)
}

/// `compile_font`: a TTX file saved as a binary font in `dir`.
fn compile_font(path: &str, suffix: &str, dir: &str) -> TestResult<(Font, String)> {
	let name = Path::new(path)
		.file_name()
		.map(|n| n.to_string_lossy().replace(".ttx", suffix))
		.unwrap_or_default();
	let save_path = Path::new(dir).join(name).to_string_lossy().into_owned();
	let mut font = Font::new();
	font.recalc_bboxes = false;
	font.recalc_timestamp = false;
	font.import_ttx(&std::fs::read_to_string(path)?)?;
	let data = font.write(&WriteOptions {
		reorder_tables: None,
		..Default::default()
	})?;
	std::fs::write(&save_path, data)?;
	Ok((font, save_path))
}

/// `TTFont(path)`.
fn open(path: &str) -> TestResult<Font> {
	Ok(Font::read(std::fs::read(path)?, ReadOptions::default())?)
}

/// `reload_font`: the font saved and read back.
fn reload_font(font: &mut Font) -> TestResult<Font> {
	Ok(Font::read(
		font.write(&WriteOptions::default())?,
		ReadOptions::default(),
	)?)
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

fn table_key(name: &str) -> TableKey {
	if name == "GlyphOrder" {
		TableKey::GlyphOrder
	} else {
		TableKey::Table(Tag::from_str_padded(name).expect("a table tag"))
	}
}

/// The tags of a font except `head`.
fn tables_except_head(font: &Font) -> Vec<String> {
	let mut tables = vec!["GlyphOrder".to_owned()];
	tables.extend(
		font.tags()
			.into_iter()
			.filter(|t| *t != Tag::new(b"head"))
			.map(|t| t.to_string()),
	);
	tables
}

/// `expect_ttx`.
fn expect_ttx(font: &mut Font, expected: &str, tables: &[&str]) -> TestResult {
	let ttx = font.to_ttx(&TtxOptions {
		tables: tables.iter().map(|t| table_key(t)).collect(),
		..Default::default()
	})?;
	let actual = read_ttx(&ttx);
	let expected_lines = read_ttx(&std::fs::read_to_string(expected)?);
	if actual != expected_lines {
		for (i, (a, e)) in actual.iter().zip(&expected_lines).enumerate() {
			if a != e {
				panic!(
					"{expected}, line {}:\n  actual   {a}\n  expected {e}",
					i + 1
				);
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

/// `check_ttx_dump`: the same after saving and reading back the font.
fn check_ttx_dump(font: &mut Font, expected: &str, tables: &[&str]) -> TestResult {
	let mut reloaded = reload_font(font)?;
	expect_ttx(&mut reloaded, expected, tables)
}

/// A master finder that maps the UFO sources to the fonts compiled in `dir`.
fn ufo_finder(dir: &str, suffix: &str) -> impl Fn(&str) -> String {
	let dir = dir.to_owned();
	let suffix = suffix.to_owned();
	move |s: &str| {
		let stem = Path::new(s)
			.file_stem()
			.map(|n| n.to_string_lossy().into_owned())
			.unwrap_or_default();
		Path::new(&dir)
			.join(format!("{stem}{suffix}"))
			.to_string_lossy()
			.into_owned()
	}
}

/// `_run_varlib_build_test`.
fn run_varlib_build_test(
	designspace_name: &str,
	font_name: &str,
	tables: &[&str],
	expected_ttx_name: &str,
	save_before_dump: bool,
	post_process_master: Option<&dyn Fn(&mut Font, &str) -> TestResult>,
) -> TestResult {
	let suffix = ".ttf";
	let ds_path = data(&format!("{designspace_name}.designspace"));
	let ttx_dir = data("master_ttx_interpolatable_ttf");
	let dir = TempDir::new(&format!("build-{designspace_name}-{expected_ttx_name}"))?;
	let tempdir = dir.0.to_string_lossy().into_owned();
	for path in file_list(&ttx_dir, ".ttx", &format!("{font_name}-"))? {
		let (mut font, save_path) = compile_font(&path, suffix, &tempdir)?;
		if let Some(post) = post_process_master {
			post(&mut font, &save_path)?;
		}
	}
	let finder = ufo_finder(&tempdir, suffix);
	let mut ds = DesignSpaceDocument::from_file(&ds_path)?;
	let (mut varfont, _model, _) = build(
		&mut ds,
		&mut SourceFonts::new(),
		&finder,
		&BuildOptions::default(),
	)?;
	if save_before_dump {
		// Some data (e.g. counts printed in TTX inline comments) is only
		// calculated at compile time.
		varfont = reload_font(&mut varfont)?;
	}
	let expected = test_output(&format!("{expected_ttx_name}.ttx"));
	expect_ttx(&mut varfont, &expected, tables)?;
	check_ttx_dump(&mut varfont, &expected, tables)
}

/// A designspace built with the default options.
fn build_doc(ds: &mut DesignSpaceDocument) -> Result<Font, VarLibError> {
	Ok(build(
		ds,
		&mut SourceFonts::new(),
		&identity_finder,
		&BuildOptions::default(),
	)?
	.0)
}

fn weight_axis(map: Vec<(i64, i64)>) -> AxisDescriptor {
	AxisDescriptor {
		name: Some("Weight".into()),
		tag: Some("wght".into()),
		minimum: Number::Int(100),
		default: Number::Int(400),
		maximum: Number::Int(900),
		map: map
			.into_iter()
			.map(|(a, b)| (Number::Int(a), Number::Int(b)))
			.collect(),
		..Default::default()
	}
}

#[test]
fn test_varlib_avar_accepts_identical_duplicate_axis_map_input() -> TestResult {
	let axis = weight_axis(vec![
		(100, 80),
		(400, 450),
		(400, 450),
		(600, 700),
		(900, 1000),
	]);
	let mut axes = Axes::new();
	axes.insert("Weight".into(), axis);
	let mut font = Font::new();
	assert!(add_avar(&mut font, &axes, &[], &["wght".to_owned()])?);
	let avar = font.require::<fontoxide_ttlib::tables::Avar>()?;
	let mut segment = avar.segments[&Tag::new(b"wght")].clone();
	segment.sort_by(|a, b| a.0.total_cmp(&b.0));
	assert_eq!(
		segment,
		vec![(-1.0, -1.0), (0.0, 0.0), (0.4, 5.0 / 11.0), (1.0, 1.0)]
	);
	Ok(())
}

#[test]
fn test_varlib_avar_rejects_conflicting_duplicate_axis_map_input() {
	let axis = weight_axis(vec![
		(100, 80),
		(400, 450),
		(600, 700),
		(400, 500),
		(900, 1000),
	]);
	let mut axes = Axes::new();
	axes.insert("Weight".into(), axis);
	let err = add_avar(&mut Font::new(), &axes, &[], &["wght".to_owned()]).expect_err("an error");
	assert!(matches!(err, VarLibError::DesignSpace(_)), "{err:?}");
	assert!(err.to_string().contains("Axis 'Weight'"), "{err}");
}

#[test]
fn test_varlib_build_conflicting_default_map_raises_designspace_error() -> TestResult {
	let mut ds = DesignSpaceDocument::from_file(data("Build.designspace"))?;
	for axis in &mut ds.axes {
		if axis.name() == Some("weight") {
			*axis.map_mut() = [(0, 0), (368, 368), (368, 369), (1000, 1000)]
				.into_iter()
				.map(|(a, b)| (Number::Int(a), Number::Int(b)))
				.collect();
		}
	}
	let err = build_doc(&mut ds).expect_err("an error");
	assert!(matches!(err, VarLibError::DesignSpace(_)), "{err:?}");
	assert!(err.to_string().contains("input value 368"), "{err}");
	assert!(!err.to_string().contains("Base master not found"));
	Ok(())
}

#[test]
fn test_varlib_build_ttf() -> TestResult {
	run_varlib_build_test(
		"Build",
		"TestFamily",
		&["GDEF", "HVAR", "MVAR", "fvar", "gvar"],
		"Build",
		false,
		None,
	)
}

#[test]
fn test_varlib_build_ttf_beyond64k() -> TestResult {
	let dir = TempDir::new("beyond64k")?;
	let tempdir = dir.0.to_string_lossy().into_owned();
	for path in file_list(
		&data("master_ttx_interpolatable_ttf"),
		".ttx",
		"TestFamily-",
	)? {
		let (mut font, save_path) = compile_font(&path, ".ttf", &tempdir)?;
		fontoxide_ttlib::beyond64k::upper_tables(&mut font, &Default::default())?;
		let bytes = font.write(&WriteOptions {
			reorder_tables: None,
			..Default::default()
		})?;
		std::fs::write(&save_path, bytes)?;
	}
	let finder = ufo_finder(&tempdir, ".ttf");
	let mut ds = DesignSpaceDocument::from_file(data("Build.designspace"))?;
	let (varfont, _, _) = build(
		&mut ds,
		&mut SourceFonts::new(),
		&finder,
		&BuildOptions::default(),
	)?;
	let tags: Vec<String> = varfont.tags().iter().map(|t| t.to_string()).collect();
	for t in ["GLYF", "LOCA", "MAXP", "HHEA", "HMTX", "GVAR"] {
		assert!(tags.iter().any(|x| x == t), "{t} in {tags:?}");
	}
	for t in ["glyf", "loca", "maxp", "hhea", "hmtx", "gvar"] {
		assert!(!tags.iter().any(|x| x == t), "{t} in {tags:?}");
	}
	Ok(())
}

#[test]
fn test_varlib_build_ttf_mixed_beyond64k_masters() -> TestResult {
	let dir = TempDir::new("mixed64k")?;
	let tempdir = dir.0.to_string_lossy().into_owned();
	for (i, path) in file_list(
		&data("master_ttx_interpolatable_ttf"),
		".ttx",
		"TestFamily-",
	)?
	.iter()
	.enumerate()
	{
		let (mut font, save_path) = compile_font(path, ".ttf", &tempdir)?;
		if i == 0 {
			fontoxide_ttlib::beyond64k::upper_tables(&mut font, &Default::default())?;
			let bytes = font.write(&WriteOptions {
				reorder_tables: None,
				..Default::default()
			})?;
			std::fs::write(&save_path, bytes)?;
		}
	}
	let finder = ufo_finder(&tempdir, ".ttf");
	let mut ds = DesignSpaceDocument::from_file(data("Build.designspace"))?;
	let err = build(
		&mut ds,
		&mut SourceFonts::new(),
		&finder,
		&BuildOptions::default(),
	)
	.expect_err("an error");
	assert!(matches!(err, VarLibError::Validation(_)));
	assert!(err.to_string().contains("same glyf/GLYF table family"));
	Ok(())
}

#[test]
fn test_varlib_build_ttf_reuse_nameid_2() -> TestResult {
	run_varlib_build_test(
		"BuildReuseNameId2",
		"TestFamily",
		&["fvar"],
		"BuildReuseNameId2",
		false,
		None,
	)
}

#[test]
fn test_varlib_build_no_axes_ttf() {
	let err = DesignSpaceDocument::from_file(data("InterpolateLayout3.designspace"))
		.map_err(VarLibError::from)
		.and_then(|mut ds| build_doc(&mut ds).map(|_| ()))
		.expect_err("an error");
	assert!(matches!(err, VarLibError::DesignSpace(_)), "{err:?}");
	assert!(err.to_string().contains("No axes defined"), "{err}");
}

#[test]
fn test_varlib_avar_single_axis() -> TestResult {
	run_varlib_build_test(
		"BuildAvarSingleAxis",
		"TestFamily3",
		&["avar"],
		"BuildAvarSingleAxis",
		false,
		None,
	)
}

#[test]
fn test_varlib_avar_with_identity_maps() -> TestResult {
	run_varlib_build_test(
		"BuildAvarIdentityMaps",
		"TestFamily3",
		&["avar"],
		"BuildAvarIdentityMaps",
		false,
		None,
	)
}

#[test]
fn test_varlib_avar_empty_axis() -> TestResult {
	run_varlib_build_test(
		"BuildAvarEmptyAxis",
		"TestFamily3",
		&["avar"],
		"BuildAvarEmptyAxis",
		false,
		None,
	)
}

#[test]
fn test_varlib_avar2() -> TestResult {
	run_varlib_build_test(
		"BuildAvar2",
		"TestFamily3",
		&["avar"],
		"BuildAvar2",
		false,
		None,
	)
}

#[test]
fn test_varlib_build_feature_variations() -> TestResult {
	run_varlib_build_test(
		"FeatureVars",
		"TestFamily",
		&["fvar", "GSUB"],
		"FeatureVars",
		true,
		None,
	)
}

#[test]
fn test_varlib_build_feature_variations_custom_tag() -> TestResult {
	run_varlib_build_test(
		"FeatureVarsCustomTag",
		"TestFamily",
		&["fvar", "GSUB"],
		"FeatureVarsCustomTag",
		true,
		None,
	)
}

#[test]
fn test_varlib_build_feature_variations_whole_range() -> TestResult {
	run_varlib_build_test(
		"FeatureVarsWholeRange",
		"TestFamily",
		&["fvar", "GSUB"],
		"FeatureVarsWholeRange",
		true,
		None,
	)
}

#[test]
fn test_varlib_build_feature_variations_whole_range_empty() -> TestResult {
	run_varlib_build_test(
		"FeatureVarsWholeRangeEmpty",
		"TestFamily",
		&["fvar", "GSUB"],
		"FeatureVarsWholeRange",
		true,
		None,
	)
}

/// Adds features to a master and saves it again.
fn add_features(features: &'static str) -> impl Fn(&mut Font, &str) -> TestResult {
	move |font: &mut Font, save_path: &str| {
		fontoxide_fealib::builder::add_open_type_features_from_string(
			font, features, None, None, false,
		)?;
		let bytes = font.write(&WriteOptions::default())?;
		std::fs::write(save_path, bytes)?;
		Ok(())
	}
}

#[test]
fn test_varlib_build_feature_variations_with_existing_rclt() -> TestResult {
	let add_rclt = add_features(
		"
            languagesystem DFLT dflt;
            languagesystem latn dflt;
            languagesystem latn NLD;

            feature rclt {
                script latn;
                language NLD;
                lookup A {
                    sub uni0041 by uni0061;
                } A;
                language dflt;
                lookup B {
                    sub uni0041 by uni0061;
                } B;
            } rclt;
            ",
	);
	run_varlib_build_test(
		"FeatureVars",
		"TestFamily",
		&["fvar", "GSUB"],
		"FeatureVars_rclt",
		true,
		Some(&add_rclt),
	)
}

#[test]
fn test_varlib_build_feature_variations_without_latn_dflt_feature() -> TestResult {
	let add = add_features(
		"
            languagesystem DFLT dflt;
            languagesystem latn dflt;
            languagesystem latn CAT;

            feature locl {
                script latn;
                # Intentionally skip defining anything for `language dflt;`.
                language CAT;
                sub uni0061 by uni0061;
            } locl;
            ",
	);
	run_varlib_build_test(
		"FeatureVars",
		"TestFamily",
		&["GSUB"],
		"FeatureVars_latn_dflt_var",
		true,
		Some(&add),
	)
}

#[test]
fn test_varlib_gvar_explicit_delta() -> TestResult {
	run_varlib_build_test(
		"BuildGvarCompositeExplicitDelta",
		"TestFamily4",
		&["gvar"],
		"BuildGvarCompositeExplicitDelta",
		false,
		None,
	)
}

/// The sources of a designspace pointed to fonts compiled from TTX
/// (`source.path = ...; ds.updatePaths()`).
fn designspace_with_compiled_sources(
	ds_name: &str,
	ttx_dir: &str,
	prefix: &str,
	suffix: &str,
	dir: &TempDir,
) -> TestResult<DesignSpaceDocument> {
	let tempdir = dir.0.to_string_lossy().into_owned();
	let ds_path = dir.file(ds_name);
	std::fs::copy(data(ds_name), &ds_path)?;
	for path in file_list(&data(ttx_dir), ".ttx", prefix)? {
		compile_font(&path, suffix, &tempdir)?;
	}
	let mut ds = DesignSpaceDocument::from_file(&ds_path)?;
	for source in &mut ds.sources {
		let name = Path::new(source.filename.as_deref().unwrap_or_default())
			.file_name()
			.map(|n| n.to_string_lossy().replace(".ufo", suffix))
			.unwrap_or_default();
		source.path = Some(dir.file(&name));
	}
	ds.update_paths()?;
	Ok(ds)
}

/// The CFF tests: masters compiled from TTX as `.otf`, the variable font saved and
/// read back, then compared with the expected TTX (and again after a roundtrip
/// with `check_dump`).
fn run_otf_build_test(
	ds_name: &str,
	ttx_dir: &str,
	prefix: &str,
	expected: &str,
	tables: &[&str],
	check_dump: bool,
) -> TestResult {
	let dir = TempDir::new(&format!("otf-{expected}"))?;
	let mut ds = designspace_with_compiled_sources(ds_name, ttx_dir, prefix, ".otf", &dir)?;
	let mut varfont = build_doc(&mut ds)?;
	let mut varfont = reload_font(&mut varfont)?;
	let expected = test_output(expected);
	expect_ttx(&mut varfont, &expected, tables)?;
	if check_dump {
		check_ttx_dump(&mut varfont, &expected, tables)?;
	}
	Ok(())
}

#[test]
fn test_varlib_nonmarking_cff2() -> TestResult {
	run_otf_build_test(
		"TestNonMarkingCFF2.designspace",
		"master_non_marking_cff2",
		"TestNonMarkingCFF2_",
		"TestNonMarkingCFF2.ttx",
		&["CFF2"],
		false,
	)
}

#[test]
fn test_varlib_build_cff2() -> TestResult {
	run_otf_build_test(
		"TestCFF2.designspace",
		"master_cff2",
		"TestCFF2_",
		"BuildTestCFF2.ttx",
		&["fvar", "CFF2"],
		false,
	)
}

#[test]
fn test_varlib_build_cff2_from_cff2() -> TestResult {
	run_otf_build_test(
		"TestCFF2Input.designspace",
		"master_cff2_input",
		"TestCFF2_",
		"BuildTestCFF2.ttx",
		&["fvar", "CFF2"],
		false,
	)
}

#[test]
fn test_varlib_build_sparse_cff2() -> TestResult {
	run_otf_build_test(
		"TestSparseCFF2VF.designspace",
		"master_sparse_cff2",
		"MasterSet_Kanji-",
		"TestSparseCFF2VF.ttx",
		&["fvar", "CFF2"],
		false,
	)
}

#[test]
fn test_varlib_build_vpal() -> TestResult {
	run_otf_build_test(
		"test_vpal.designspace",
		"master_vpal_test",
		"master_vpal_test_",
		"test_vpal.ttx",
		&["GPOS"],
		false,
	)
}

#[test]
fn test_varlib_build_vvar_cff2() -> TestResult {
	run_otf_build_test(
		"TestVVAR.designspace",
		"master_vvar_cff2",
		"TestVVAR",
		"TestVVAR.ttx",
		&["VVAR"],
		true,
	)
}

#[test]
fn test_varlib_build_base() -> TestResult {
	run_otf_build_test(
		"TestBASE.designspace",
		"master_base_test",
		"TestBASE",
		"TestBASE.ttx",
		&["BASE"],
		true,
	)
}

#[test]
fn test_varlib_build_variable_cff2_with_empty_sparse_glyph() -> TestResult {
	// https://github.com/fonttools/fonttools/issues/3233
	run_varlib_build_test(
		"SparseCFF2",
		"SparseCFF2",
		&["GlyphOrder", "CFF2", "fvar", "hmtx", "HVAR"],
		"SparseCFF2-VF",
		true,
		None,
	)
}

#[test]
fn test_varlib_build_from_ttf_paths() -> TestResult {
	let dir = TempDir::new("from-ttf-paths")?;
	let mut ds = designspace_with_compiled_sources(
		"Build.designspace",
		"master_ttx_interpolatable_ttf",
		"TestFamily-",
		".ttf",
		&dir,
	)?;
	let mut varfont = build_doc(&mut ds)?;
	let mut varfont = reload_font(&mut varfont)?;
	let tables = tables_except_head(&varfont);
	let tables: Vec<&str> = tables.iter().map(String::as_str).collect();
	expect_ttx(&mut varfont, &test_output("BuildMain.ttx"), &tables)
}

#[test]
fn test_varlib_build_from_ttx_paths() -> TestResult {
	let ttx_dir = data("master_ttx_interpolatable_ttf");
	let mut ds = DesignSpaceDocument::from_file(data("Build.designspace"))?;
	for source in &mut ds.sources {
		let name = Path::new(source.filename.as_deref().unwrap_or_default())
			.file_name()
			.map(|n| n.to_string_lossy().replace(".ufo", ".ttx"))
			.unwrap_or_default();
		source.path = Some(
			Path::new(&ttx_dir)
				.join(name)
				.to_string_lossy()
				.into_owned(),
		);
	}
	ds.update_paths()?;
	let mut varfont = build_doc(&mut ds)?;
	let mut varfont = reload_font(&mut varfont)?;
	let tables = tables_except_head(&varfont);
	let tables: Vec<&str> = tables.iter().map(String::as_str).collect();
	expect_ttx(&mut varfont, &test_output("BuildMain.ttx"), &tables)
}

#[test]
fn test_varlib_build_from_ds_object_in_memory_ttfonts() -> TestResult {
	let dir = TempDir::new("in-memory")?;
	let tempdir = dir.0.to_string_lossy().into_owned();
	for path in file_list(
		&data("master_ttx_interpolatable_ttf"),
		".ttx",
		"TestFamily-",
	)? {
		compile_font(&path, ".ttf", &tempdir)?;
	}
	let mut ds = DesignSpaceDocument::from_file(data("Build.designspace"))?;
	let mut fonts = SourceFonts::new();
	for source in &mut ds.sources {
		let name = Path::new(source.filename.as_deref().unwrap_or_default())
			.file_name()
			.map(|n| n.to_string_lossy().replace(".ufo", ".ttf"))
			.unwrap_or_default();
		let font = Font::read(
			std::fs::read(dir.file(&name))?,
			ReadOptions {
				recalc_bboxes: false,
				recalc_timestamp: false,
				..Default::default()
			},
		)?;
		fonts.push(Some(font));
		// Make sure no file path gets into build().
		source.filename = None;
	}
	let (mut varfont, _, _) = build(
		&mut ds,
		&mut fonts,
		&identity_finder,
		&BuildOptions::default(),
	)?;
	let mut varfont = reload_font(&mut varfont)?;
	let tables = tables_except_head(&varfont);
	let tables: Vec<&str> = tables.iter().map(String::as_str).collect();
	expect_ttx(&mut varfont, &test_output("BuildMain.ttx"), &tables)
}

#[test]
fn test_varlib_build_sparse_masters() -> TestResult {
	let mut ds = DesignSpaceDocument::from_file(data("SparseMasters.designspace"))?;
	let mut varfont = build_doc(&mut ds)?;
	let mut varfont = reload_font(&mut varfont)?;
	let tables = tables_except_head(&varfont);
	let tables: Vec<&str> = tables.iter().map(String::as_str).collect();
	expect_ttx(&mut varfont, &test_output("SparseMasters.ttx"), &tables)
}

#[test]
fn test_varlib_build_lazy_masters() -> TestResult {
	// See https://github.com/fonttools/fonttools/issues/1808: the masters are
	// read from binaries, without being loaded first.
	let mut ds = DesignSpaceDocument::from_file(data("SparseMasters.designspace"))?;
	let mut fonts = SourceFonts::new();
	for source in &ds.sources {
		let mut font = Font::new();
		font.import_ttx(&std::fs::read_to_string(
			source.path.as_deref().unwrap_or_default(),
		)?)?;
		let bytes = font.write(&WriteOptions {
			reorder_tables: Some(false),
			..Default::default()
		})?;
		fonts.push(Some(Font::read(bytes, ReadOptions::default())?));
	}
	let (mut varfont, _, _) = build(
		&mut ds,
		&mut fonts,
		&identity_finder,
		&BuildOptions::default(),
	)?;
	let mut varfont = reload_font(&mut varfont)?;
	let tables = tables_except_head(&varfont);
	let tables: Vec<&str> = tables.iter().map(String::as_str).collect();
	expect_ttx(&mut varfont, &test_output("SparseMasters.ttx"), &tables)
}

#[test]
fn test_varlib_build_sparse_masters_mvar() -> TestResult {
	use fontoxide_ttlib::tables::{Hhea, Vhea};
	let mut ds = DesignSpaceDocument::from_file(data("SparseMasters.designspace"))?;
	let mut fonts = SourceFonts::new();
	load_masters(&ds, &mut fonts, &identity_finder)?;

	// Trigger MVAR generation so varLib is forced to create deltas with a sparse
	// master inbetween.
	for (i, v) in [(0usize, 1i16), (2, 800)] {
		let font = fonts[i].as_mut().expect("loaded");
		let os2 = font.require::<Os2>()?;
		os2.base.s_typo_ascender = v;
		os2.base.s_typo_descender = v;
		os2.base.s_typo_line_gap = v;
		os2.base.us_win_ascent = v as u16;
		os2.base.us_win_descent = v as u16;
		os2.sx_height = v;
		os2.s_cap_height = v;
		os2.base.y_subscript_x_size = v;
		os2.base.y_subscript_y_size = v;
		os2.base.y_subscript_x_offset = v;
		os2.base.y_subscript_y_offset = v;
		os2.base.y_superscript_x_size = v;
		os2.base.y_superscript_y_size = v;
		os2.base.y_superscript_x_offset = v;
		os2.base.y_superscript_y_offset = v;
		os2.base.y_strikeout_size = v;
		os2.base.y_strikeout_position = v;
		let mut vhea = Vhea::default();
		vhea.ascent = v;
		vhea.descent = v;
		vhea.line_gap = v;
		vhea.caret_slope_rise = v;
		vhea.caret_slope_run = v;
		vhea.caret_offset = v;
		font.insert(Box::new(vhea));
		let hhea = font.require::<Hhea>()?;
		hhea.caret_slope_rise = v;
		hhea.caret_slope_run = v;
		hhea.caret_offset = v;
		let post = font.require::<Post>()?;
		post.header.underline_thickness = v;
		post.header.underline_position = v;
	}

	let (mut varfont, _, _) = build(
		&mut ds,
		&mut fonts,
		&identity_finder,
		&BuildOptions::default(),
	)?;
	let mvar = varfont.require::<fontoxide_ttlib::tables::Mvar>()?;
	let tags: Vec<String> = mvar
		.table
		.value_record
		.iter()
		.map(|r| r.value_tag.to_string())
		.collect();
	for (t, _) in fontoxide_varlib::mvar::MVAR_ENTRIES {
		assert!(tags.iter().any(|x| x == t), "{t} in {tags:?}");
	}
	Ok(())
}

#[test]
fn test_varlib_build_single_master() -> TestResult {
	run_varlib_build_test(
		"SingleMaster",
		"TestFamily",
		&[
			"GDEF", "HVAR", "MVAR", "STAT", "fvar", "cvar", "gvar", "name",
		],
		"SingleMaster",
		true,
		None,
	)
}

/// `_extract_flat_kerning`.
fn extract_flat_kerning(
	font: &mut Font,
	st: &ot::PairPosFormat2,
) -> TestResult<Vec<((String, String), i64)>> {
	let order = font.glyph_order()?;
	let mut out = Vec::new();
	let c1 = st.class_def1.as_deref().expect("ClassDef1");
	let c2 = st.class_def2.as_deref().expect("ClassDef2");
	for g1 in &st.coverage.as_deref().expect("Coverage").glyphs {
		let k1 = c1.class_defs.get(g1).copied().unwrap_or(0) as usize;
		for (g2, name2) in order.names().iter().enumerate() {
			let k2 = c2.class_defs.get(&(g2 as u32)).copied().unwrap_or(0) as usize;
			let v = st.class1_record[k1].class2_record[k2]
				.value1
				.as_ref()
				.and_then(|v| v.x_advance)
				.unwrap_or(0);
			out.push(((order.name(*g1).into_owned(), name2.clone()), i64::from(v)));
		}
	}
	out.sort();
	Ok(out)
}

fn kerning(pairs: &[(&str, &str, i64)]) -> Vec<((String, String), i64)> {
	let mut out: Vec<_> = pairs
		.iter()
		.map(|(a, b, v)| (((*a).to_owned(), (*b).to_owned()), *v))
		.collect();
	out.sort();
	out
}

fn class_kerning_tables(font: &mut Font) -> TestResult<Vec<ot::PairPosFormat2>> {
	let gpos = font.require::<Gpos>()?;
	let mut out = Vec::new();
	for lookup in gpos
		.table
		.lookup_list
		.as_deref()
		.expect("LookupList")
		.lookup
		.iter()
		.flatten()
	{
		for st in lookup.sub_table.iter().flatten() {
			if let ot::LookupSubTable::PairPos(ot::PairPos::Format2(t)) = st.as_ref() {
				out.push(t.clone());
			}
		}
	}
	Ok(out)
}

#[test]
fn test_kerning_merging() -> TestResult {
	let ttx_dir = data("master_kerning_merging");
	let mut ds = DesignSpaceDocument::from_file(data("KerningMerging.designspace"))?;
	let mut fonts = SourceFonts::new();
	for source in &ds.sources {
		let name = Path::new(source.filename.as_deref().unwrap_or_default())
			.file_name()
			.map(|n| n.to_string_lossy().replace(".ttf", ".ttx"))
			.unwrap_or_default();
		let mut font = Font::new();
		font.import_ttx(&std::fs::read_to_string(Path::new(&ttx_dir).join(name))?)?;
		fonts.push(Some(reload_font(&mut font)?));
	}
	let (mut varfont, _, _) = build(
		&mut ds,
		&mut fonts,
		&identity_finder,
		&BuildOptions::default(),
	)?;
	let mut varfont = reload_font(&mut varfont)?;

	let tables = class_kerning_tables(&mut varfont)?;
	assert_eq!(tables.len(), 1);
	let table = &tables[0];
	// No class kerned against class zero has an XAdvDevice table.
	for row in &table.class1_record {
		let zero = &row.class2_record[0];
		assert!(
			zero.value1
				.as_ref()
				.is_none_or(|v| v.x_adv_device.clone().flatten().is_none())
		);
	}
	assert_eq!(
		extract_flat_kerning(&mut varfont, table)?,
		kerning(&[
			("A", ".notdef", 0),
			("A", "A", 0),
			("A", "B", -20),
			("A", "C", 0),
			("A", "D", -20),
			("B", ".notdef", 0),
			("B", "A", 0),
			("B", "B", 0),
			("B", "C", 0),
			("B", "D", 0),
		])
	);

	let mut thin = fontoxide_varlib::mutator::instantiate_variable_font_copy(
		&mut varfont,
		&[("wght".to_owned(), 100.0)],
		false,
	)?;
	let st = class_kerning_tables(&mut thin)?.remove(0);
	assert_eq!(
		extract_flat_kerning(&mut thin, &st)?,
		kerning(&[
			("A", ".notdef", 0),
			("A", "A", 0),
			("A", "B", 0),
			("A", "C", 10),
			("A", "D", 0),
			("B", ".notdef", 0),
			("B", "A", 0),
			("B", "B", 0),
			("B", "C", 10),
			("B", "D", 0),
		])
	);

	let mut black = fontoxide_varlib::mutator::instantiate_variable_font_copy(
		&mut varfont,
		&[("wght".to_owned(), 900.0)],
		false,
	)?;
	let st = class_kerning_tables(&mut black)?.remove(0);
	assert_eq!(
		extract_flat_kerning(&mut black, &st)?,
		kerning(&[
			("A", ".notdef", 0),
			("A", "A", 0),
			("A", "B", 0),
			("A", "C", 0),
			("A", "D", 40),
			("B", ".notdef", 0),
			("B", "A", 0),
			("B", "B", 0),
			("B", "C", 0),
			("B", "D", 40),
		])
	);
	Ok(())
}

#[test]
fn test_designspace_fill_in_location() -> TestResult {
	let mut ds = DesignSpaceDocument::from_file(data("VarLibLocationTest.designspace"))?;
	let loaded = load_designspace(&mut ds, true, true)?;
	let location: Vec<(String, f64)> = loaded.instances[0]
		.design_location
		.iter()
		.map(|(k, v)| (k.clone(), v.x().to_f64()))
		.collect();
	assert_eq!(
		location,
		vec![("weight".to_owned(), 0.0), ("width".to_owned(), 50.0)]
	);
	Ok(())
}

fn merge_error(designspace_name: &str, font_name: &str) -> fontoxide_varlib::MergeError {
	let err = run_varlib_build_test(
		designspace_name,
		font_name,
		&["GPOS"],
		designspace_name,
		true,
		None,
	)
	.expect_err("an error");
	let err = err.downcast::<VarLibError>().expect("a VarLibError");
	err.as_merge().expect("a merge error").clone()
}

#[test]
fn test_varlib_build_incompatible_features() {
	let err = merge_error("IncompatibleFeatures", "IncompatibleFeatures");
	assert_eq!(err.kind, MergeErrorKind::ShouldBeConstant);
	assert_eq!(
		err.to_string(),
		"

Couldn't merge the fonts, because some values were different, but should have
been the same. This happened while performing the following operation:
GPOS.table.FeatureList.FeatureCount

The problem is likely to be in Simple Two Axis Bold:
Expected to see .FeatureCount==2, instead saw 1

Incompatible features between masters.
Expected: kern, mark.
Got: kern.
"
	);
}

#[test]
fn test_varlib_build_incompatible_lookup_types() {
	let err = merge_error("IncompatibleLookupTypes", "IncompatibleLookupTypes");
	assert_eq!(err.kind, MergeErrorKind::MismatchedTypes);
	assert!(
		err.to_string()
			.contains("'MarkBasePos', instead saw 'PairPos'"),
		"{err}"
	);
}

#[test]
fn test_varlib_build_incompatible_arrays() {
	let err = merge_error("IncompatibleArrays", "IncompatibleArrays");
	assert_eq!(err.kind, MergeErrorKind::ShouldBeConstant);
	assert!(err.to_string().starts_with(
		"

Couldn't merge the fonts, because some values were different, but should have
been the same. This happened while performing the following operation:
GPOS.table.ScriptList.ScriptCount

The problem is likely to be in Simple Two Axis Bold:
Expected to see .ScriptCount==1, instead saw 0"
	));
}

#[test]
fn test_varlib_add_gsub_feature_variations() -> TestResult {
	let ttx_dir = data("master_ttx_interpolatable_ttf");
	let mut ds = DesignSpaceDocument::from_file(data("FeatureVars.designspace"))?;
	let mut fonts = SourceFonts::new();
	for source in &ds.sources {
		let name = Path::new(source.filename.as_deref().unwrap_or_default())
			.file_name()
			.map(|n| n.to_string_lossy().replace(".ufo", ".ttx"))
			.unwrap_or_default();
		let mut font = Font::new();
		font.import_ttx(&std::fs::read_to_string(Path::new(&ttx_dir).join(name))?)?;
		fonts.push(Some(font));
	}
	let options = BuildOptions {
		exclude: vec!["GSUB".into()],
		..Default::default()
	};
	let (mut varfont, _, _) = build(&mut ds, &mut fonts, &identity_finder, &options)?;
	assert!(!varfont.contains(Tag::new(b"GSUB")));
	add_gsub_feature_variations_from_designspace(&mut varfont, &mut ds, &[], false)?;
	assert!(varfont.contains(Tag::new(b"GSUB")));
	let expected = test_output("FeatureVars.ttx");
	expect_ttx(&mut varfont, &expected, &["fvar", "GSUB"])?;
	check_ttx_dump(&mut varfont, &expected, &["fvar", "GSUB"])
}

#[test]
fn test_varlib_build_inconsistent_use_my_metrics_flags() -> TestResult {
	run_varlib_build_test(
		"InconsistentUseMyMetrics",
		"InconsistentUseMyMetrics",
		&["GlyphOrder", "hmtx", "glyf", "fvar", "HVAR"],
		"InconsistentUseMyMetrics-VF",
		true,
		None,
	)
}

#[test]
fn test_varlib_build_many_no_overwrite_stat() -> TestResult {
	let mut base = Font::new();
	base.import_ttx(&std::fs::read_to_string(data(
		"master_no_overwrite_stat/Test-CondensedThin.ttx",
	))?)?;
	assert!(base.contains(Tag::new(b"STAT")));
	let mut ds = DesignSpaceDocument::from_file(data("TestNoOverwriteSTAT.designspace"))?;
	let mut vfs = build_many(
		&mut ds,
		&mut SourceFonts::new(),
		&identity_finder,
		&BuildOptions::default(),
		&|_| false,
	)?;
	let (_, vf) = vfs.first_mut().expect("a variable font");
	let stat = vf.require::<fontoxide_ttlib::tables::Stat>()?.table.clone();
	assert_eq!(stat, base.require::<fontoxide_ttlib::tables::Stat>()?.table);
	Ok(())
}

#[test]
fn test_load_masters_layer_name_without_required_font() {
	let mut ds = DesignSpaceDocument::new();
	ds.add_source(SourceDescriptor {
		layer_name: Some("Medium".into()),
		..Default::default()
	});
	let err = load_masters(&ds, &mut SourceFonts::new(), &identity_finder).expect_err("an error");
	assert!(matches!(err, VarLibError::Validation(_)));
	assert!(
		err.to_string()
			.contains("specified a layer name but lacks the required TTFont object")
	);
}

/// The `ttFont` fixture.
fn tt_font() -> TestResult<Font> {
	let mut f = Font::new();
	let mut os2 = Os2::default();
	os2.base.us_weight_class = 400;
	os2.base.us_width_class = 100;
	f.insert(Box::new(os2));
	f.insert(Box::new(Post::default()));
	f.require::<Post>()?.header.italic_angle = 0.0;
	Ok(f)
}

fn location(tag: &str, value: f64) -> Vec<(String, f64)> {
	vec![(tag.to_owned(), value)]
}

#[test]
fn set_default_weight_width_slant_test_wght() -> TestResult {
	for (v, expected) in [
		(0.0, 1),
		(1.0, 1),
		(100.0, 100),
		(1000.0, 1000),
		(1001.0, 1000),
	] {
		let mut font = tt_font()?;
		set_default_weight_width_slant(&mut font, &location("wght", v))?;
		assert_eq!(font.require::<Os2>()?.base.us_weight_class, expected, "{v}");
	}
	Ok(())
}

#[test]
fn set_default_weight_width_slant_test_wdth() -> TestResult {
	for (v, expected) in [
		(0.0, 1),
		(56.0, 1),
		(57.0, 2),
		(62.5, 2),
		(75.0, 3),
		(87.5, 4),
		(100.0, 5),
		(112.5, 6),
		(125.0, 7),
		(150.0, 8),
		(200.0, 9),
		(201.0, 9),
		(1000.0, 9),
	] {
		let mut font = tt_font()?;
		set_default_weight_width_slant(&mut font, &location("wdth", v))?;
		assert_eq!(font.require::<Os2>()?.base.us_width_class, expected, "{v}");
	}
	Ok(())
}

#[test]
fn set_default_weight_width_slant_test_slnt() -> TestResult {
	for (v, expected) in [
		(-91.0, -90.0),
		(-90.0, -90.0),
		(0.0, 0.0),
		(11.5, 11.5),
		(90.0, 90.0),
		(91.0, 90.0),
	] {
		let mut font = tt_font()?;
		set_default_weight_width_slant(&mut font, &location("slnt", v))?;
		assert_eq!(font.require::<Post>()?.header.italic_angle, expected, "{v}");
	}
	Ok(())
}

#[test]
fn set_default_weight_width_slant_test_all() -> TestResult {
	let mut font = tt_font()?;
	set_default_weight_width_slant(
		&mut font,
		&[
			("wght".to_owned(), 500.0),
			("wdth".to_owned(), 150.0),
			("slnt".to_owned(), -12.0),
		],
	)?;
	assert_eq!(font.require::<Os2>()?.base.us_weight_class, 500);
	assert_eq!(font.require::<Os2>()?.base.us_width_class, 8);
	assert_eq!(font.require::<Post>()?.header.italic_angle, -12.0);
	Ok(())
}

#[allow(dead_code)]
fn unused(_: IndexMap<String, String>) {}

fn main(args: &[&str]) -> Result<i32, VarLibError> {
	fontoxide_varlib::build::main(args)
}

#[test]
fn test_varlib_main_ttf() -> TestResult {
	let dir = TempDir::new("main-ttf")?;
	let ttf_dir = dir.file("master_ttf_interpolatable");
	std::fs::create_dir_all(&ttf_dir)?;
	for path in file_list(
		&data("master_ttx_interpolatable_ttf"),
		".ttx",
		"TestFamily-",
	)? {
		compile_font(&path, ".ttf", &ttf_dir)?;
	}
	let ds_copy = dir.file("BuildMain.designspace");
	std::fs::copy(data("Build.designspace"), &ds_copy)?;

	// fontTools first runs with the default master finder, relative to the current
	// directory; changing it would affect the other tests, so the finder is
	// always given here.
	let varfont_path = dir.file("BuildMain-VF.ttf");
	let finder = format!(
		"{}/master_ttf_interpolatable/{{stem}}.ttf",
		dir.0.to_string_lossy()
	);
	main(&[&ds_copy, "--master-finder", &finder])?;
	assert!(Path::new(&varfont_path).exists());

	// And also with explicit -o output option.
	std::fs::remove_file(&varfont_path)?;
	let varfont_path = dir.file("BuildMain-VF-o.ttf");
	main(&[&ds_copy, "-o", &varfont_path, "--master-finder", &finder])?;
	assert!(Path::new(&varfont_path).exists());

	let mut varfont = open(&varfont_path)?;
	let tables = tables_except_head(&varfont);
	let tables: Vec<&str> = tables.iter().map(String::as_str).collect();
	expect_ttx(&mut varfont, &test_output("BuildMain.ttx"), &tables)
}

#[test]
fn test_varlib_main_output_dir() -> TestResult {
	let dir = TempDir::new("main-output-dir")?;
	let outdir = dir.file("output_dir_test");
	assert!(!Path::new(&outdir).exists());
	let ds_path = dir.file("BuildMain.designspace");
	std::fs::copy(data("Build.designspace"), &ds_path)?;
	let master_dir = Path::new(&outdir).join("master_ttx");
	std::fs::create_dir_all(&master_dir)?;
	for path in file_list(&data("master_ttx_interpolatable_ttf"), ".ttx", "")? {
		let name = Path::new(&path)
			.file_name()
			.expect("a file name")
			.to_owned();
		std::fs::copy(&path, master_dir.join(name))?;
	}
	let finder = format!("{outdir}/master_ttx/{{stem}}.ttx");
	main(&[
		&ds_path,
		"--output-dir",
		&outdir,
		"--master-finder",
		&finder,
	])?;
	assert!(Path::new(&outdir).is_dir());
	assert!(Path::new(&outdir).join("BuildMain-VF.ttf").exists());
	Ok(())
}

#[test]
fn test_varlib_main_filter_variable_fonts() -> TestResult {
	let dir = TempDir::new("main-filter")?;
	let outdir = dir.file("filter_variable_fonts_test");
	let ds_path = dir.file("BuildMain.designspace");
	std::fs::copy(data("Build.designspace"), &ds_path)?;
	let master_dir = Path::new(&outdir).join("master_ttx");
	std::fs::create_dir_all(&master_dir)?;
	for path in file_list(&data("master_ttx_interpolatable_ttf"), ".ttx", "")? {
		let name = Path::new(&path)
			.file_name()
			.expect("a file name")
			.to_owned();
		std::fs::copy(&path, master_dir.join(name))?;
	}
	let finder = format!("{outdir}/master_ttx/{{stem}}.ttx");
	let cmd = [
		ds_path.as_str(),
		"--output-dir",
		&outdir,
		"--master-finder",
		&finder,
	];
	// No font matches.
	let mut args = cmd.to_vec();
	args.extend(["--variable-fonts", "FooBar"]);
	assert!(matches!(main(&args), Err(VarLibError::Usage(_))));
	// This does match.
	let mut args = cmd.to_vec();
	args.extend(["--variable-fonts", "Build.*"]);
	main(&args)?;
	assert!(Path::new(&outdir).join("BuildMain-VF.ttf").exists());
	Ok(())
}

#[test]
fn test_varlib_main_drop_implied_oncurves() -> TestResult {
	let dir = TempDir::new("main-drop-oncurves")?;
	let outdir = dir.file("drop_implied_oncurves_test");
	let ttf_dir = Path::new(&outdir).join("master_ttf_interpolatable");
	std::fs::create_dir_all(&ttf_dir)?;
	for path in file_list(&data("master_ttx_drop_oncurves"), ".ttx", "TestFamily-")? {
		compile_font(&path, ".ttf", &ttf_dir.to_string_lossy())?;
	}
	let ds_copy = Path::new(&outdir)
		.join("DropOnCurves.designspace")
		.to_string_lossy()
		.into_owned();
	std::fs::copy(data("DropOnCurves.designspace"), &ds_copy)?;
	let finder = format!("{outdir}/master_ttf_interpolatable/{{stem}}.ttf");
	main(&[
		&ds_copy,
		"--master-finder",
		&finder,
		"--drop-implied-oncurves",
	])?;
	let mut varfont = open(
		&Path::new(&outdir)
			.join("DropOnCurves-VF.ttf")
			.to_string_lossy(),
	)?;
	let tables = tables_except_head(&varfont);
	let tables: Vec<&str> = tables.iter().map(String::as_str).collect();
	expect_ttx(&mut varfont, &test_output("DropOnCurves.ttx"), &tables)
}

#[test]
fn test_varlib_path_traversal_sanitized() -> TestResult {
	let dir = TempDir::new("path-traversal")?;
	let ttf_dir = dir.file("masters");
	std::fs::create_dir_all(&ttf_dir)?;
	for path in file_list(
		&data("master_ttx_interpolatable_ttf"),
		".ttx",
		"TestFamily-",
	)? {
		compile_font(&path, ".ttf", &ttf_dir)?;
	}
	let ds_path = dir.file("test.designspace");
	std::fs::write(
		&ds_path,
		r#"<?xml version='1.0' encoding='UTF-8'?>
<designspace format="5.0">
    <axes>
        <axis tag="wght" name="Weight" minimum="300" maximum="700" default="300"/>
    </axes>
    <sources>
        <source filename="masters/TestFamily-Master0.ttf" name="Light">
            <location><dimension name="Weight" xvalue="300"/></location>
        </source>
        <source filename="masters/TestFamily-Master2.ttf" name="Bold">
            <location><dimension name="Weight" xvalue="700"/></location>
        </source>
    </sources>
    <variable-fonts>
        <variable-font name="TestFamily" filename="../forbidden/evil.ttf">
            <axis-subsets>
                <axis-subset name="Weight"/>
            </axis-subsets>
        </variable-font>
    </variable-fonts>
</designspace>"#,
	)?;
	// Run without --output-dir (defaults to designspace directory).
	main(&[&ds_path])?;
	assert!(Path::new(&dir.file("evil.ttf")).is_file());
	assert!(!dir.0.join("..").join("forbidden").join("evil.ttf").exists());
	Ok(())
}

#[test]
fn test_varlib_main_output_dir_contains_traversal_name() -> TestResult {
	// A designspace can name a variable font with path separators; that name is
	// used to build the output file name, which must stay in --output-dir.
	// fontTools stubs out the compile step with a mock; here the sources are real.
	let dir = TempDir::new("traversal-name")?;
	let outdir = dir.file("output_dir_traversal_test");
	std::fs::create_dir_all(&outdir)?;
	let ttf_dir = dir.file("masters");
	std::fs::create_dir_all(&ttf_dir)?;
	for path in file_list(
		&data("master_ttx_interpolatable_ttf"),
		".ttx",
		"TestFamily-Master0",
	)? {
		compile_font(&path, ".ttf", &ttf_dir)?;
	}
	let ds_path = dir.file("traversal.designspace");
	std::fs::write(
		&ds_path,
		r#"<?xml version='1.0' encoding='UTF-8'?>
<designspace format="5.0">
  <axes>
    <axis tag="wght" name="Weight" minimum="400" maximum="700" default="400"/>
  </axes>
  <sources>
    <source filename="masters/TestFamily-Master0.ttf" name="Regular">
      <location><dimension name="Weight" xvalue="400"/></location>
    </source>
  </sources>
  <variable-fonts>
    <variable-font name="../escaped">
      <axis-subsets>
        <axis-subset name="Weight"/>
      </axis-subsets>
    </variable-font>
  </variable-fonts>
</designspace>"#,
	)?;
	main(&[&ds_path, "--output-dir", &outdir])?;
	assert!(Path::new(&outdir).join("escaped.ttf").exists());
	assert!(!Path::new(&dir.file("escaped.ttf")).exists());
	Ok(())
}

#[test]
fn test_varlib_build_variable_colr() -> TestResult {
	let ds_path = data("TestVariableCOLR.designspace");
	let mut ds = DesignSpaceDocument::from_file(&ds_path)?;
	let mut varfont = build_doc(&mut ds)?;
	let mut varfont = reload_font(&mut varfont)?;
	let tables = ["GlyphOrder", "fvar", "glyf", "COLR", "CPAL"];
	let expected = test_output("TestVariableCOLR-VF.ttx");
	expect_ttx(&mut varfont, &expected, &tables)?;
	check_ttx_dump(&mut varfont, &expected, &tables)
}

#[test]
fn test_variable_colr_without_var_index_map() -> TestResult {
	use fontoxide_colorlib::Value;
	use fontoxide_ttlib::tables::Colr;
	use fontoxide_varlib::models::VariationModel;
	// We don't add a no-op VarIndexMap to variable COLR when not needed
	// (https://github.com/fonttools/fonttools/issues/2800).
	let mut font1 = Font::new();
	font1.set_glyph_order(vec![".notdef".into(), "A".into()]);
	let mut glyphs = indexmap::IndexMap::new();
	glyphs.insert(
		"A".to_owned(),
		Value::List(vec![Value::Int(2), Value::Int(0), Value::Float(1.0)]),
	);
	let ids = font1.glyph_order()?.reverse_map().clone();
	let colr = fontoxide_colorlib::builder::build_colr(
		glyphs,
		&ids,
		fontoxide_colorlib::builder::BuildColrOptions {
			glyph_map: true,
			..Default::default()
		},
	)?;
	font1.insert(Box::new(colr));
	// font2 == font1 except for PaintSolid.Alpha.
	let mut font2 = font1.clone();
	{
		let colr = font2.require::<Colr>()?.table.as_mut().expect("COLR v1");
		let rec = &mut colr
			.base_glyph_list
			.as_mut()
			.expect("BaseGlyphList")
			.base_glyph_paint_record[0];
		if let Some(ot::Paint::Format2(p)) = rec.paint.as_deref_mut() {
			p.alpha = 0.0;
		}
	}
	let mut masters = vec![font1.clone(), font2];
	let mut varfont = font1;
	let axis_order = vec!["XXXX".to_owned()];
	let model = VariationModel::new(
		&[Vec::new(), vec![("XXXX".to_owned(), 1.0)]],
		Some(&axis_order),
		false,
		None,
	)?;
	let infos = vec![Default::default(), Default::default()];
	fontoxide_varlib::build::add_colr(
		&mut varfont,
		&model,
		&mut masters,
		&axis_order,
		true,
		&infos,
	)?;

	let colr = varfont.require::<Colr>()?.table.clone().expect("COLR v1");
	let records = &colr
		.base_glyph_list
		.as_ref()
		.expect("BaseGlyphList")
		.base_glyph_paint_record;
	assert_eq!(records.len(), 1);
	let Some(ot::Paint::Format3(p)) = records[0].paint.as_deref() else {
		panic!("not a PaintVarSolid: {:?}", records[0].paint);
	};
	assert_eq!(p.var_index_base, 0);
	let store = colr.var_store.as_ref().expect("a VarStore");
	assert_eq!(store.var_data.len(), 1);
	let data = store.var_data[0].as_ref().expect("VarData");
	assert_eq!(data.item, vec![vec![-16384]]);
	assert!(colr.var_index_map.is_none());
	Ok(())
}

/// `_make_beyond64k_master`: a font with 65537 glyphs (g00001..g65536, so the
/// highest glyph ID is 0x10000) and the given features. fontTools builds it with
/// FontBuilder; here the other tables come from a test master. With `upper`, the
/// tables are converted to their uppercase beyond-64k companions.
fn make_beyond64k_master(features: &str, upper: bool) -> TestResult<Font> {
	use fontoxide_ttlib::tables::glyf::Glyph;
	use fontoxide_ttlib::tables::{Glyf, Hmtx};
	let mut font = Font::new();
	font.import_ttx(&std::fs::read_to_string(data(
		"master_ttx_interpolatable_ttf/TestFamily-Master0.ttx",
	))?)?;
	for t in [
		b"cmap", b"GSUB", b"GPOS", b"GDEF", b"kern", b"STAT", b"fpgm", b"prep", b"cvt ", b"gasp",
		b"vmtx", b"vhea",
	] {
		font.remove(Tag::new(t));
	}
	let mut order = vec![".notdef".to_owned()];
	order.extend((1..=0x10000).map(|i| format!("g{i:05}")));
	let n = order.len() as u32;
	font.set_glyph_order(order.clone());
	{
		let glyf = font.require::<Glyf>()?;
		glyf.glyphs = (0..n).map(|g| (g, Glyph::default())).collect();
	}
	{
		let hmtx = font.require::<Hmtx>()?;
		hmtx.metrics = (0..n).map(|g| (g, (500, 0))).collect();
	}
	{
		let post = font.require::<Post>()?;
		post.header.format_type = 3.0;
		post.extra_names = None;
		post.mapping = None;
	}
	font.set_glyph_order(order);
	fontoxide_fealib::builder::add_open_type_features_from_string(
		&mut font, features, None, None, false,
	)?;
	if upper {
		fontoxide_ttlib::beyond64k::upper_tables(&mut font, &Default::default())?;
	}
	Ok(font)
}

/// `_build_beyond64k_vf`: a weight-variable font from (location, master) pairs.
fn build_beyond64k_vf(masters: Vec<(i64, Font)>) -> TestResult<Font> {
	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(AxisDescriptor {
		tag: Some("wght".into()),
		name: Some("Weight".into()),
		minimum: Number::Int(0),
		default: Number::Int(0),
		maximum: Number::Int(1000),
		..Default::default()
	});
	let mut fonts = SourceFonts::new();
	for (loc, font) in masters {
		let mut source = SourceDescriptor::default();
		source.design_location.insert(
			"Weight".into(),
			fontoxide_designspacelib::LocationValue::Value(Number::Int(loc)),
		);
		doc.add_source(source);
		fonts.push(Some(font));
	}
	Ok(build(
		&mut doc,
		&mut fonts,
		&identity_finder,
		&BuildOptions::default(),
	)?
	.0)
}

/// `_interpolated_xadvance`.
fn interpolated_xadvance(
	font: &mut Font,
	value: &fontoxide_ttlib::ot::conv::ValueRecord,
	wght: f64,
) -> TestResult<f64> {
	use fontoxide_ttlib::tables::Gdef;
	let dev = value.x_adv_device.clone().flatten().expect("XAdvDevice");
	assert_eq!(dev.delta_format, 0x8000);
	let store = font.require::<Gdef>()?.table.var_store.clone();
	let axes = vec!["wght".to_owned()];
	let inst = fontoxide_ttlib::var::VarStoreInstancer::new(
		store.as_deref(),
		&axes,
		[("wght".to_owned(), wght)].into_iter().collect(),
	);
	let idx = (u64::from(dev.start_size) << 16) + u64::from(dev.end_size);
	Ok(f64::from(value.x_advance.unwrap_or(0)) + inst.get(idx)?)
}

fn lookups_of(gpos: &ot::GPOS) -> Vec<ot::Lookup> {
	let lookups = match (&gpos.lookup_list2, &gpos.lookup_list) {
		(Some(l), _) => &l.lookup,
		(None, Some(l)) => &l.lookup,
		(None, None) => return Vec::new(),
	};
	lookups.iter().flatten().map(|b| (**b).clone()).collect()
}

#[test]
fn test_varlib_merge_pair_pos_beyond64k_disjoint_masters() -> TestResult {
	let hi = "g65536";
	let hi2 = "g65535";
	let master = |second: &str, value: i64| {
		make_beyond64k_master(
			&format!("feature kern {{ pos g00001 {second} {value}; }} kern;"),
			false,
		)
	};
	let mut varfont = build_beyond64k_vf(vec![(0, master(hi, -50)?), (1000, master(hi2, -70)?)])?;
	let gpos = varfont.require::<Gpos>()?.table.clone();
	let lookups = lookups_of(&gpos);
	let st = lookups[0].sub_table[0].as_deref().expect("a subtable");
	let ot::LookupSubTable::PairPos(ot::PairPos::Format3(st)) = st else {
		panic!("not an extended pair-based PairPos: {st:?}");
	};
	let g1 = varfont.glyph_id("g00001")?;
	let index = st
		.coverage
		.as_deref()
		.expect("Coverage")
		.glyphs
		.iter()
		.position(|g| *g == g1);
	let set = st.pair_set[index.expect("g00001")]
		.as_deref()
		.expect("a PairSet");
	let hi_id = varfont.glyph_id(hi)?;
	let hi2_id = varfont.glyph_id(hi2)?;
	assert_eq!(hi_id, 0x10000);
	assert_eq!(hi2_id, 0xFFFF);
	let record = |g: u32| {
		set.pair_value_record
			.iter()
			.find(|r| r.second_glyph == g)
			.expect("a record")
			.clone()
	};
	let (r1, r2) = (record(hi_id), record(hi2_id));
	assert_eq!(set.pair_value_record.len(), 2);
	let (v1, v2) = (r1.value1.expect("Value1"), r2.value1.expect("Value1"));
	assert_eq!(v1.x_advance, Some(-50));
	assert_eq!(v2.x_advance, Some(0));
	assert_eq!(interpolated_xadvance(&mut varfont, &v1, 0.5)?, -25.0);
	assert_eq!(interpolated_xadvance(&mut varfont, &v2, 0.5)?, -35.0);
	Ok(())
}

#[test]
fn test_varlib_merge_gdef_glyph_class_def_beyond64k_divergent_masters() -> TestResult {
	use fontoxide_ttlib::tables::Gdef;
	let hi = "g65536";
	let master = |base: &str| {
		make_beyond64k_master(
			&format!("table GDEF {{ GlyphClassDef [{base}], [], [{hi}], []; }} GDEF;\n"),
			false,
		)
	};
	let mut varfont = build_beyond64k_vf(vec![(0, master("g00001")?), (1000, master("g00002")?)])?;
	let gdef = varfont.require::<Gdef>()?.table.clone();
	assert_eq!(gdef.version, 0x0001_0004);
	assert!(gdef.glyph_class_def.is_none());
	let defs = &gdef
		.glyph_class_def2
		.as_deref()
		.expect("GlyphClassDef2")
		.class_defs;
	let expected: std::collections::BTreeMap<u32, u32> = [
		(varfont.glyph_id("g00001")?, 1),
		(varfont.glyph_id("g00002")?, 1),
		(varfont.glyph_id(hi)?, 3),
	]
	.into_iter()
	.collect();
	assert_eq!(*defs, expected);
	assert_eq!(varfont.glyph_id(hi)?, 0x10000);
	Ok(())
}

#[test]
fn test_varlib_build_gpos_gdef_beyond64k() -> TestResult {
	use ot::LookupSubTable as St;
	let hi = "g65536";
	let acute = "g65530";
	let master = |scale: i64| {
		let (a, k) = (10 * scale, -50 * scale);
		let fea = format!(
			"markClass {acute} <anchor {a} {a}> @TOP;\nfeature kern {{\n  pos g00001 {hi} {k};\n  pos [g00002 g00003] [g65535 {hi}] {k};\n}} kern;\nfeature cpsp {{ pos {hi} <{k} 0 {k} 0>; }} cpsp;\nfeature mark {{ pos base {hi} <anchor {a} {a}> mark @TOP; }} mark;\nfeature mkmk {{ pos mark {acute} <anchor {a} {a}> mark @TOP; }} mkmk;\nfeature curs {{ pos cursive {hi} <anchor {a} {a}> <anchor {a} {a}>; }} curs;\n"
		);
		make_beyond64k_master(&fea, true)
	};
	let mut varfont = build_beyond64k_vf(vec![(0, master(1)?), (1000, master(2)?)])?;
	let mut varfont = reload_font(&mut varfont)?;
	for t in [b"GPOS", b"GDEF", b"GLYF", b"MAXP"] {
		assert!(varfont.contains(Tag::new(t)));
	}
	let gdef = varfont
		.require::<fontoxide_ttlib::tables::Gdef>()?
		.table
		.clone();
	assert_eq!(gdef.version, 0x0001_0004);
	assert!(gdef.var_store.is_some());
	let gpos = varfont.require::<Gpos>()?.table.clone();
	// Glyph names are synthesized on reload since beyond-64k drops post names, so
	// the glyphs are found by ID.
	let first = 1;
	let mut present = std::collections::BTreeSet::new();
	let mut found = None;
	for lookup in lookups_of(&gpos) {
		present.insert(lookup.lookup_type);
		for st in lookup.sub_table.iter().flatten() {
			let ok = match st.as_ref() {
				St::SinglePos(p) => {
					matches!(p, ot::SinglePos::Format3(_) | ot::SinglePos::Format4(_))
				}
				St::PairPos(p) => matches!(p, ot::PairPos::Format3(_) | ot::PairPos::Format4(_)),
				St::CursivePos(p) => matches!(p, ot::CursivePos::Format2(_)),
				St::MarkBasePos(p) => matches!(p, ot::MarkBasePos::Format2(_)),
				St::MarkMarkPos(p) => matches!(p, ot::MarkMarkPos::Format2(_)),
				_ => false,
			};
			assert!(ok, "{} {st:?}", lookup.lookup_type);
			if let St::PairPos(ot::PairPos::Format3(p)) = st.as_ref()
				&& let Some(i) = p
					.coverage
					.as_deref()
					.and_then(|c| c.glyphs.iter().position(|g| *g == first))
			{
				let set = p.pair_set[i].as_deref().expect("a PairSet");
				found = set
					.pair_value_record
					.iter()
					.find(|r| r.second_glyph == 0x10000)
					.cloned();
			}
		}
	}
	assert_eq!(present, [1, 2, 3, 4, 6].into_iter().collect());
	let value = found
		.expect("the pair with the glyph above 0xFFFF")
		.value1
		.expect("Value1");
	assert_eq!(value.x_advance, Some(-50));
	assert_eq!(interpolated_xadvance(&mut varfont, &value, 0.5)?, -75.0);
	Ok(())
}
