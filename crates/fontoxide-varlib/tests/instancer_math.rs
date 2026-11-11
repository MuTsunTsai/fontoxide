//! Port of fontTools' `Tests/varLib/instancer/math_test.py`.
//!
//! The font of the `font` fixture is made by
//! `tools/reference/instancer_module_fixtures.py`.

#![allow(clippy::type_complexity)]

mod common;
mod module_support;

use common::{instantiate, pin};
use fontoxide_misc::round_tools::ot_round;
use fontoxide_subset::{Options, Subsetter};
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::{Fvar, Gdef, Math};
use fontoxide_ttlib::var::VarStoreInstancer;
use fontoxide_ttlib::{Font, ReadOptions, Tag, WriteOptions};
use fontoxide_varlib::instancer::AxisTriple;
use module_support::*;

const MATH: Tag = Tag::new(b"MATH");

/// `BASES`: the value, and its deltas on `wght` and `wdth`.
fn base(name: &str) -> (f64, f64, f64) {
	match name {
		"leading" => (100.0, 10.0, 20.0),
		"axis" => (200.0, -20.0, 30.0),
		"italic_a" => (10.0, 30.0, -10.0),
		"italic_b" => (20.0, 40.0, -20.0),
		"accent_a" => (300.0, 50.0, 5.0),
		"accent_b" => (400.0, 60.0, -5.0),
		"height" => (50.0, 70.0, 7.0),
		"kern_0" => (70.0, 80.0, 8.0),
		"kern_1" => (80.0, 90.0, 9.0),
		"assembly" => (40.0, 100.0, 10.0),
		_ => panic!("{name}"),
	}
}

fn glyph_names(font: &mut Font, coverage: Option<&ot::Coverage>) -> TestResult<Vec<String>> {
	let order = font.glyph_order()?;
	Ok(coverage
		.ok_or("no Coverage")?
		.glyphs
		.iter()
		.map(|&g| order.name(g).into_owned())
		.collect())
}

/// `values`.
fn values(font: &mut Font) -> TestResult<Vec<(String, ot::MathValueRecord)>> {
	let math = font.require::<Math>()?.table.clone();
	let constants = math.math_constants.as_ref().ok_or("no MathConstants")?;
	let mut result = vec![
		("leading".to_owned(), constants.math_leading.clone()),
		("axis".to_owned(), constants.axis_height.clone()),
	];
	let glyphs = math.math_glyph_info.as_ref().ok_or("no MathGlyphInfo")?;
	let info = glyphs
		.math_italics_correction_info
		.as_ref()
		.ok_or("no MathItalicsCorrectionInfo")?;
	let names = glyph_names(font, info.coverage.as_deref())?;
	for (name, value) in names.iter().zip(&info.italics_correction) {
		result.push((format!("italic_{name}"), value.clone()));
	}
	let info = glyphs
		.math_top_accent_attachment
		.as_ref()
		.ok_or("no MathTopAccentAttachment")?;
	let names = glyph_names(font, info.top_accent_coverage.as_deref())?;
	for (name, value) in names.iter().zip(&info.top_accent_attachment) {
		result.push((format!("accent_{name}"), value.clone()));
	}
	let kern_info = glyphs.math_kern_info.as_ref().ok_or("no MathKernInfo")?;
	let kern = kern_info.math_kern_info_records[0]
		.top_right_math_kern
		.as_ref()
		.ok_or("no TopRightMathKern")?;
	result.push(("height".to_owned(), kern.correction_height[0].clone()));
	for (i, value) in kern.kern_value.iter().enumerate() {
		result.push((format!("kern_{i}"), value.clone()));
	}
	let variants = math.math_variants.as_ref().ok_or("no MathVariants")?;
	let assembly = variants.vert_glyph_construction[0]
		.as_ref()
		.ok_or("no MathGlyphConstruction")?
		.glyph_assembly
		.as_ref()
		.ok_or("no GlyphAssembly")?;
	result.push(("assembly".to_owned(), assembly.italics_correction.clone()));
	Ok(result)
}

fn font_fixture() -> TestResult<Font> {
	fixture("math")
}

/// `roundtrip(instantiateVariableFont(font, limits))`.
fn instance(font: &Font, pairs: &[(&str, Option<AxisTriple>)]) -> TestResult<Font> {
	let mut font = font.clone();
	instantiate(&mut font, pairs)?;
	roundtrip(&mut font)
}

/// `check_values`.
fn check_values(font: &mut Font, weight: f64, width: f64) -> TestResult {
	let axes: Vec<String> = match font.get::<Fvar>()? {
		Some(fvar) => fvar.axes.iter().map(|a| a.tag_text()).collect(),
		None => Vec::new(),
	};
	let store = match font.get::<Gdef>()? {
		Some(gdef) => gdef.var_store.clone(),
		None => None,
	};
	let instancer = store
		.as_deref()
		.map(|s| VarStoreInstancer::new(Some(s), &axes, loc(&[("wdth", width)])));
	for (name, record) in values(font)? {
		let (value, dw, dd) = base(&name);
		let expected = value + ot_round(weight * dw) as f64 + width * dd;
		let mut value = f64::from(record.value);
		if let Some(dev) = &record.device_table {
			assert_eq!(dev.delta_format, 0x8000);
			let instancer = instancer.as_ref().ok_or("no store")?;
			value += instancer.get((u64::from(dev.start_size) << 16) | u64::from(dev.end_size))?;
		}
		assert_eq!(value, expected, "{name}");
	}
	let math = font.require::<Math>()?;
	let constants = math.math_constants.as_ref().ok_or("no MathConstants")?;
	assert_eq!(constants.accent_base_height.value, 500);
	let device = constants
		.flattened_accent_base_height
		.device_table
		.as_ref()
		.ok_or("no DeviceTable")?;
	assert_eq!(device.delta_format, 1);
	assert_eq!(device.delta_value, [1]);
	Ok(())
}

#[test]
fn test_full_math_instance_folds_all_record_types() -> TestResult {
	for weight in [0.0, 0.5, 1.0] {
		let font = font_fixture()?;
		let mut result = instance(&font, &[("wght", pin(weight)), ("wdth", pin(0.0))])?;
		assert!(!result.contains(Tag::new(b"fvar")) && !result.contains(Tag::new(b"GDEF")));
		assert!(
			values(&mut result)?
				.iter()
				.all(|(_, v)| v.device_table.is_none())
		);
		let math = result.require::<Math>()?;
		let constants = math.math_constants.as_ref().ok_or("no MathConstants")?;
		assert!(constants.accent_base_height.device_table.is_none());
		check_values(&mut result, weight, 0.0)?;
	}
	Ok(())
}

#[test]
fn test_partial_math_instance_remaps_indices_and_composes() -> TestResult {
	for weight in [0.0, 0.5, 1.0] {
		let font = font_fixture()?;
		let mut result = instance(&font, &[("wght", pin(weight))])?;
		let axes: Vec<String> = result
			.require::<Fvar>()?
			.axes
			.iter()
			.map(|a| a.tag_text())
			.collect();
		assert_eq!(axes, ["wdth"]);
		for width in [0.0, 0.5, 1.0] {
			check_values(&mut result, weight, width)?;
			let mut full = instance(&result, &[("wdth", pin(width))])?;
			let mut direct = instance(&font, &[("wght", pin(weight)), ("wdth", pin(width))])?;
			assert_eq!(full.table_data(MATH)?, direct.table_data(MATH)?);
		}
	}
	Ok(())
}

#[test]
fn test_subset_retains_math_only_variation_rows() -> TestResult {
	let mut font = font_fixture()?;
	let options = Options {
		glyph_names: true,
		..Default::default()
	};
	let mut subsetter = Subsetter::new(options);
	subsetter.populate(&["a"], &[], &[], "");
	subsetter.subset(&mut font)?;
	let mut font = roundtrip(&mut font)?;
	let order = font.glyph_order()?;
	assert!(order.id("b").is_none());
	let mut result = instance(&font, &[("wght", pin(1.0))])?;
	check_values(&mut result, 1.0, 1.0)
}

#[test]
fn test_varstore_cli_remaps_math_indices() -> TestResult {
	let mut font = font_fixture()?;
	let tmp_path = std::env::temp_dir().join(format!(
		"fontoxide-instancer-math-varstore-cli-{}",
		std::process::id()
	));
	std::fs::create_dir_all(&tmp_path)?;
	let (source, output) = (tmp_path.join("source.ttf"), tmp_path.join("optimized.ttf"));
	std::fs::write(&source, font.write(&WriteOptions::default())?)?;
	let mut out = Vec::new();
	let code = fontoxide_varlib::var_store::run(
		&[&source.to_string_lossy(), &output.to_string_lossy()],
		&mut out,
		&mut std::io::sink(),
	)?;
	assert_eq!(code, 0);
	let optimized = Font::read(std::fs::read(&output)?, ReadOptions::default())?;
	let _ = std::fs::remove_dir_all(&tmp_path);
	let mut result = instance(&optimized, &[("wght", pin(1.0))])?;
	check_values(&mut result, 1.0, 1.0)
}

#[test]
fn test_in_memory_records_without_device_attribute() -> TestResult {
	let mut font = font_fixture()?;
	let math = font.require::<Math>()?;
	math.math_constants
		.as_mut()
		.ok_or("no MathConstants")?
		.fraction_rule_thickness
		.device_table = None;
	let mut result = instance(&font, &[("wght", pin(1.0)), ("wdth", pin(0.0))])?;
	check_values(&mut result, 1.0, 0.0)
}
