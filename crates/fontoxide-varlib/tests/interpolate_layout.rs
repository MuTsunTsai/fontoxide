//! `Tests/varLib/interpolate_layout_test.py`.

#![allow(non_snake_case)]

mod common;

use std::path::{Path, PathBuf};

use common::{TestResult, tests_path};
use fontoxide_designspacelib::{DesignSpaceDocument, LocationValue, Number};
use fontoxide_ttlib::{Font, ReadOptions, TableKey, Tag, TtxOptions, WriteOptions};
use fontoxide_varlib::build::{BuildOptions, SourceFonts, build};
use fontoxide_varlib::errors::VarLibError;
use fontoxide_varlib::interpolate_layout::{interpolate_layout, main as interpolate_layout_main};

/// A unique directory under the temporary one, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
	fn new(name: &str) -> TestResult<Self> {
		let path = std::env::temp_dir().join(format!("fontoxide-il-{}-{name}", std::process::id()));
		let _ = std::fs::remove_dir_all(&path);
		std::fs::create_dir_all(&path)?;
		Ok(TempDir(path))
	}

	fn path(&self) -> String {
		self.0.to_string_lossy().into_owned()
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

/// `get_file_list(folder, suffix, prefix)`, sorted.
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

/// `compile_font`, with features added before saving.
fn compile_font(path: &str, suffix: &str, dir: &str, features: Option<&str>) -> TestResult<String> {
	compile_font_cfg(path, suffix, dir, features, false)
}

/// `compile_font` with `cfg={"fontTools.otlLib.builder:WRITE_GPOS7": write_gpos7}`.
fn compile_font_cfg(
	path: &str,
	suffix: &str,
	dir: &str,
	features: Option<&str>,
	write_gpos7: bool,
) -> TestResult<String> {
	let name = Path::new(path)
		.file_name()
		.map(|n| n.to_string_lossy().replace(".ttx", suffix))
		.unwrap_or_default();
	let save_path = Path::new(dir).join(name).to_string_lossy().into_owned();
	let mut font = Font::new();
	font.recalc_bboxes = false;
	font.recalc_timestamp = false;
	font.write_gpos7 = write_gpos7;
	font.import_ttx(&std::fs::read_to_string(path)?)?;
	if let Some(features) = features {
		fontoxide_fealib::builder::add_open_type_features_from_string(
			&mut font, features, None, None, false,
		)?;
	}
	let bytes = font.write(&WriteOptions {
		reorder_tables: None,
		..Default::default()
	})?;
	std::fs::write(&save_path, bytes)?;
	Ok(save_path)
}

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

/// `check_ttx_dump`.
fn check_ttx_dump(font: &mut Font, expected: &str, tables: &[&str]) -> TestResult {
	let mut reloaded = Font::read(
		font.write(&WriteOptions::default())?,
		ReadOptions::default(),
	)?;
	expect_ttx(&mut reloaded, expected, tables)
}

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

fn weight_500() -> Vec<(String, f64)> {
	vec![("weight".to_owned(), 500.0)]
}

/// The masters `TestFamily2-*` compiled with `features`, interpolated at
/// weight 500 with `designspace`, compared with `expected`.
fn run_with(designspace: &str, features: &[&str], expected: &str, tables: &[&str]) -> TestResult {
	run_cfg(designspace, features, expected, tables, false)
}

fn run_cfg(
	designspace: &str,
	features: &[&str],
	expected: &str,
	tables: &[&str],
	write_gpos7: bool,
) -> TestResult {
	let dir = TempDir::new(&format!("{expected}-{}", features.len()))?;
	let ttx_dir = data("master_ttx_interpolatable_ttf");
	for (i, path) in file_list(&ttx_dir, ".ttx", "TestFamily2-")?
		.iter()
		.enumerate()
	{
		compile_font_cfg(
			path,
			".ttf",
			&dir.path(),
			features.get(i).copied(),
			write_gpos7,
		)?;
	}
	let dir_path = dir.path();
	let finder = ufo_finder(&dir_path, ".ttf");
	let mut ds = DesignSpaceDocument::from_file(data(designspace))?;
	let mut font = interpolate_layout(
		&mut ds,
		&mut SourceFonts::new(),
		&weight_500(),
		&finder,
		false,
	)?;
	let expected = test_output(expected);
	expect_ttx(&mut font, &expected, tables)?;
	check_ttx_dump(&mut font, &expected, tables)
}

fn run_gpos7(features: &[&str], expected: &str, tables: &[&str]) -> TestResult {
	run_cfg(
		"InterpolateLayout.designspace",
		features,
		expected,
		tables,
		true,
	)
}

fn run(features: &[&str], expected: &str, tables: &[&str]) -> TestResult {
	run_with("InterpolateLayout.designspace", features, expected, tables)
}

#[test]
fn test_varlib_interpolate_layout_gsub_only_ttf() -> TestResult {
	run_with(
		"InterpolateLayout.designspace",
		&[],
		"InterpolateLayout.ttx",
		&["GSUB"],
	)
}

#[test]
fn test_varlib_interpolate_layout_no_gsub_ttf() -> TestResult {
	run_with(
		"InterpolateLayout2.designspace",
		&[],
		"InterpolateLayout2.ttx",
		&["GSUB"],
	)
}

#[test]
fn test_varlib_interpolate_layout_gsub_only_no_axes_ttf() {
	let err = DesignSpaceDocument::from_file(data("InterpolateLayout3.designspace"))
		.map_err(VarLibError::from)
		.and_then(|mut ds| {
			interpolate_layout(
				&mut ds,
				&mut SourceFonts::new(),
				&weight_500(),
				&|s: &str| s.to_owned(),
				false,
			)
			.map(|_| ())
		})
		.expect_err("an error");
	assert!(err.to_string().contains("No axes defined"), "{err}");
}

#[test]
fn test_varlib_interpolate_layout_gpos_only_decreasing_axis_map_ttf() -> TestResult {
	let dir = TempDir::new("decreasing")?;
	let features = [
		"feature xxxx { pos A <-80 0 -160 0>; } xxxx;",
		"feature xxxx { pos A <-97 0 -195 0>; } xxxx;",
	];
	for (i, path) in file_list(
		&data("master_ttx_interpolatable_ttf"),
		".ttx",
		"TestFamily2-",
	)?
	.iter()
	.enumerate()
	{
		compile_font(path, ".ttf", &dir.path(), Some(features[i]))?;
	}
	let mut doc = DesignSpaceDocument::from_file(data("InterpolateLayout.designspace"))?;
	*doc.axes[0].map_mut() = vec![
		(Number::Int(0), Number::Int(1000)),
		(Number::Int(1000), Number::Int(0)),
	];
	for source in &mut doc.sources {
		let w = source
			.design_location
			.get("weight")
			.map_or(0.0, |v| v.x().to_f64());
		source.design_location.clear();
		source.design_location.insert(
			"weight".into(),
			LocationValue::Value(Number::Float(1000.0 - w)),
		);
	}
	let dir_path = dir.path();
	let finder = ufo_finder(&dir_path, ".ttf");
	let mut font = interpolate_layout(
		&mut doc,
		&mut SourceFonts::new(),
		&weight_500(),
		&finder,
		false,
	)?;
	expect_ttx(
		&mut font,
		&test_output("InterpolateLayoutGPOS_1_diff.ttx"),
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_main_ttf() -> TestResult {
	let dir = TempDir::new("main")?;
	let ttf_dir = dir
		.0
		.join("master_ttf_interpolatable")
		.to_string_lossy()
		.into_owned();
	std::fs::create_dir_all(&ttf_dir)?;
	for path in file_list(
		&data("master_ttx_interpolatable_ttf"),
		".ttx",
		"TestFamily-",
	)? {
		compile_font(&path, ".ttf", &ttf_dir, None)?;
	}
	let finder = ufo_finder(&ttf_dir, ".ttf");
	let mut ds = DesignSpaceDocument::from_file(data("Build.designspace"))?;
	let (mut varfont, _, _) = build(
		&mut ds,
		&mut SourceFonts::new(),
		&finder,
		&BuildOptions::default(),
	)?;
	let varfont_path = dir.0.join("InterpolateLayoutMain.ttf");
	std::fs::write(&varfont_path, varfont.write(&WriteOptions::default())?)?;
	let ds_copy = dir
		.0
		.join("InterpolateLayoutMain.designspace")
		.to_string_lossy()
		.into_owned();
	std::fs::copy(data("Build.designspace"), &ds_copy)?;
	interpolate_layout_main(&[&ds_copy, "weight=500", "contrast=50"])?;
	let inst_path = dir.0.join("InterpolateLayoutMain-instance.ttf");
	let mut inst = Font::read(std::fs::read(inst_path)?, ReadOptions::default())?;
	let mut tables: Vec<String> = vec!["GlyphOrder".to_owned()];
	tables.extend(
		inst.tags()
			.into_iter()
			.filter(|t| *t != Tag::new(b"head"))
			.map(|t| t.to_string()),
	);
	let tables: Vec<&str> = tables.iter().map(String::as_str).collect();
	expect_ttx(
		&mut inst,
		&test_output("InterpolateLayoutMain.ttx"),
		&tables,
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_size_feat_same_val_ttf() -> TestResult {
	run(
		&[
			r#"
        feature size {
            parameters 10.0 0;
        } size;
        "#,
			r#"
        feature size {
            parameters 10.0 0;
        } size;
        "#,
		],
		"InterpolateLayoutGPOS_size_feat_same.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_1_same_val_ttf() -> TestResult {
	run(
		&[
			r#"
        feature xxxx {
            pos A <-80 0 -160 0>;
        } xxxx;
        "#,
			r#"
        feature xxxx {
            pos A <-80 0 -160 0>;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_1_same.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_1_diff_val_ttf() -> TestResult {
	run(
		&[
			r#"
        feature xxxx {
            pos A <-80 0 -160 0>;
        } xxxx;
        "#,
			r#"
        feature xxxx {
            pos A <-97 0 -195 0>;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_1_diff.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_1_diff2_val_ttf() -> TestResult {
	run(
		&[
			r#"
        feature xxxx {
            pos A <-80 0 -160 0>;
            pos a <-55 0 -105 0>;
        } xxxx;
        "#,
			r#"
        feature xxxx {
            pos A <-97 0 -195 0>;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_1_diff2.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_2_spec_pairs_same_val_ttf() -> TestResult {
	run(
		&[
			r#"
        feature xxxx {
            pos A a -53;
        } xxxx;
        "#,
			r#"
        feature xxxx {
            pos A a -53;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_2_spec_same.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_2_spec_pairs_diff_val_ttf() -> TestResult {
	run(
		&[
			r#"
        feature xxxx {
            pos A a -53;
        } xxxx;
        "#,
			r#"
        feature xxxx {
            pos A a -27;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_2_spec_diff.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_2_spec_pairs_diff2_val_ttf() -> TestResult {
	run(
		&[
			r#"
        feature xxxx {
            pos A a -53;
        } xxxx;
        "#,
			r#"
        feature xxxx {
            pos A a -27;
            pos a a 19;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_2_spec_diff2.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_2_class_pairs_same_val_ttf() -> TestResult {
	run(
		&[
			r#"
        feature xxxx {
            pos [A] [a] -53;
        } xxxx;
        "#,
			r#"
        feature xxxx {
            pos [A] [a] -53;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_2_class_same.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_2_class_pairs_diff_val_ttf() -> TestResult {
	run(
		&[
			r#"
        feature xxxx {
            pos [A] [a] -53;
        } xxxx;
        "#,
			r#"
        feature xxxx {
            pos [A] [a] -27;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_2_class_diff.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_2_class_pairs_diff2_val_ttf() -> TestResult {
	run(
		&[
			r#"
        feature xxxx {
            pos [A] [a] -53;
        } xxxx;
        "#,
			r#"
        feature xxxx {
            pos [A] [a] -27;
            pos [a] [a] 19;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_2_class_diff2.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_3_same_val_ttf() -> TestResult {
	run(
		&[
			r#"
        feature xxxx {
            pos cursive a <anchor 60 15> <anchor 405 310>;
        } xxxx;
        "#,
			r#"
        feature xxxx {
            pos cursive a <anchor 60 15> <anchor 405 310>;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_3_same.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_3_diff_val_ttf() -> TestResult {
	run(
		&[
			r#"
        feature xxxx {
            pos cursive a <anchor 60 15> <anchor 405 310>;
        } xxxx;
        "#,
			r#"
        feature xxxx {
            pos cursive a <anchor 38 42> <anchor 483 279>;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_3_diff.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_4_same_val_ttf() -> TestResult {
	run(
		&[
			r#"
        markClass uni0303 <anchor 0 500> @MARKS_ABOVE;
        feature xxxx {
            pos base a <anchor 260 500> mark @MARKS_ABOVE;
        } xxxx;
        "#,
			r#"
        markClass uni0303 <anchor 0 500> @MARKS_ABOVE;
        feature xxxx {
            pos base a <anchor 260 500> mark @MARKS_ABOVE;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_4_same.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_4_diff_val_ttf() -> TestResult {
	run(
		&[
			r#"
        markClass uni0303 <anchor 0 500> @MARKS_ABOVE;
        feature xxxx {
            pos base a <anchor 260 500> mark @MARKS_ABOVE;
        } xxxx;
        "#,
			r#"
        markClass uni0303 <anchor 0 520> @MARKS_ABOVE;
        feature xxxx {
            pos base a <anchor 285 520> mark @MARKS_ABOVE;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_4_diff.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_5_same_val_ttf() -> TestResult {
	run(
		&[
			r#"
        markClass uni0330 <anchor 0 -50> @MARKS_BELOW;
        feature xxxx {
            pos ligature f_t <anchor 115 -50> mark @MARKS_BELOW
                ligComponent <anchor 430 -50> mark @MARKS_BELOW;
        } xxxx;
        "#,
			r#"
        markClass uni0330 <anchor 0 -50> @MARKS_BELOW;
        feature xxxx {
            pos ligature f_t <anchor 115 -50> mark @MARKS_BELOW
                ligComponent <anchor 430 -50> mark @MARKS_BELOW;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_5_same.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_5_diff_val_ttf() -> TestResult {
	run(
		&[
			r#"
        markClass uni0330 <anchor 0 -50> @MARKS_BELOW;
        feature xxxx {
            pos ligature f_t <anchor 115 -50> mark @MARKS_BELOW
                ligComponent <anchor 430 -50> mark @MARKS_BELOW;
        } xxxx;
        "#,
			r#"
        markClass uni0330 <anchor 0 -20> @MARKS_BELOW;
        feature xxxx {
            pos ligature f_t <anchor 173 -20> mark @MARKS_BELOW
                ligComponent <anchor 577 -20> mark @MARKS_BELOW;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_5_diff.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_6_same_val_ttf() -> TestResult {
	run(
		&[
			r#"
        markClass uni0303 <anchor 0 500> @MARKS_ABOVE;
        feature xxxx {
            pos mark uni0308 <anchor 0 675> mark @MARKS_ABOVE;
        } xxxx;
        "#,
			r#"
        markClass uni0303 <anchor 0 500> @MARKS_ABOVE;
        feature xxxx {
            pos mark uni0308 <anchor 0 675> mark @MARKS_ABOVE;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_6_same.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_6_diff_val_ttf() -> TestResult {
	run(
		&[
			r#"
        markClass uni0303 <anchor 0 500> @MARKS_ABOVE;
        feature xxxx {
            pos mark uni0308 <anchor 0 675> mark @MARKS_ABOVE;
        } xxxx;
        "#,
			r#"
        markClass uni0303 <anchor 0 520> @MARKS_ABOVE;
        feature xxxx {
            pos mark uni0308 <anchor 0 730> mark @MARKS_ABOVE;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_6_diff.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_7_same_val_ttf() -> TestResult {
	run_gpos7(
		&[
			r#"
        markClass uni0303 <anchor 0 500> @MARKS_ABOVE;
        lookup CNTXT_PAIR_POS {
            pos A a -23;
        } CNTXT_PAIR_POS;

        lookup CNTXT_MARK_TO_BASE {
            pos base a <anchor 260 500> mark @MARKS_ABOVE;
        } CNTXT_MARK_TO_BASE;

        feature xxxx {
            pos A' lookup CNTXT_PAIR_POS a' @MARKS_ABOVE' lookup CNTXT_MARK_TO_BASE;
        } xxxx;
        "#,
			r#"
        markClass uni0303 <anchor 0 500> @MARKS_ABOVE;
        lookup CNTXT_PAIR_POS {
            pos A a -23;
        } CNTXT_PAIR_POS;

        lookup CNTXT_MARK_TO_BASE {
            pos base a <anchor 260 500> mark @MARKS_ABOVE;
        } CNTXT_MARK_TO_BASE;

        feature xxxx {
            pos A' lookup CNTXT_PAIR_POS a' @MARKS_ABOVE' lookup CNTXT_MARK_TO_BASE;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_7_same.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_7_diff_val_ttf() -> TestResult {
	run_gpos7(
		&[
			r#"
        markClass uni0303 <anchor 0 500> @MARKS_ABOVE;
        lookup CNTXT_PAIR_POS {
            pos A a -23;
        } CNTXT_PAIR_POS;

        lookup CNTXT_MARK_TO_BASE {
            pos base a <anchor 260 500> mark @MARKS_ABOVE;
        } CNTXT_MARK_TO_BASE;

        feature xxxx {
            pos A' lookup CNTXT_PAIR_POS a' @MARKS_ABOVE' lookup CNTXT_MARK_TO_BASE;
        } xxxx;
        "#,
			r#"
        markClass uni0303 <anchor 0 520> @MARKS_ABOVE;
        lookup CNTXT_PAIR_POS {
            pos A a 57;
        } CNTXT_PAIR_POS;

        lookup CNTXT_MARK_TO_BASE {
            pos base a <anchor 285 520> mark @MARKS_ABOVE;
        } CNTXT_MARK_TO_BASE;

        feature xxxx {
            pos A' lookup CNTXT_PAIR_POS a' @MARKS_ABOVE' lookup CNTXT_MARK_TO_BASE;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_7_diff.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_8_same_val_ttf() -> TestResult {
	run(
		&[
			r#"
        markClass uni0303 <anchor 0 500> @MARKS_ABOVE;
        lookup CNTXT_PAIR_POS {
            pos A a -23;
        } CNTXT_PAIR_POS;

        lookup CNTXT_MARK_TO_BASE {
            pos base a <anchor 260 500> mark @MARKS_ABOVE;
        } CNTXT_MARK_TO_BASE;

        feature xxxx {
            pos A' lookup CNTXT_PAIR_POS a' @MARKS_ABOVE' lookup CNTXT_MARK_TO_BASE;
        } xxxx;
        "#,
			r#"
        markClass uni0303 <anchor 0 500> @MARKS_ABOVE;
        lookup CNTXT_PAIR_POS {
            pos A a -23;
        } CNTXT_PAIR_POS;

        lookup CNTXT_MARK_TO_BASE {
            pos base a <anchor 260 500> mark @MARKS_ABOVE;
        } CNTXT_MARK_TO_BASE;

        feature xxxx {
            pos A' lookup CNTXT_PAIR_POS a' @MARKS_ABOVE' lookup CNTXT_MARK_TO_BASE;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_8_same.ttx",
		&["GPOS"],
	)
}

#[test]
fn test_varlib_interpolate_layout_GPOS_only_LookupType_8_diff_val_ttf() -> TestResult {
	run(
		&[
			r#"
        markClass uni0303 <anchor 0 500> @MARKS_ABOVE;
        lookup CNTXT_PAIR_POS {
            pos A a -23;
        } CNTXT_PAIR_POS;

        lookup CNTXT_MARK_TO_BASE {
            pos base a <anchor 260 500> mark @MARKS_ABOVE;
        } CNTXT_MARK_TO_BASE;

        feature xxxx {
            pos A' lookup CNTXT_PAIR_POS a' @MARKS_ABOVE' lookup CNTXT_MARK_TO_BASE;
        } xxxx;
        "#,
			r#"
        markClass uni0303 <anchor 0 520> @MARKS_ABOVE;
        lookup CNTXT_PAIR_POS {
            pos A a 57;
        } CNTXT_PAIR_POS;

        lookup CNTXT_MARK_TO_BASE {
            pos base a <anchor 285 520> mark @MARKS_ABOVE;
        } CNTXT_MARK_TO_BASE;

        feature xxxx {
            pos A' lookup CNTXT_PAIR_POS a' @MARKS_ABOVE' lookup CNTXT_MARK_TO_BASE;
        } xxxx;
        "#,
		],
		"InterpolateLayoutGPOS_8_diff.ttx",
		&["GPOS"],
	)
}
