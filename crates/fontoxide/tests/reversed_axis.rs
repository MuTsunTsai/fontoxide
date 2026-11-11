//! `Tests/varLib/reversed_axis_test.py`: the tests whose masters are built with
//! FontBuilder and TTGlyphPen. The others are in
//! `crates/fontoxide-varlib/tests/reversed_axis.rs`.

use std::collections::BTreeMap;

use fontoxide::designspacelib::{
	AxisDescriptor, AxisMappingDescriptor, Condition, DesignSpaceDocument, LocationValue, Number,
	RuleDescriptor, SimpleLocation, SourceDescriptor, evaluate_conditions,
};
use fontoxide::font_builder::{FontBuilder, NameKey, NameValue, Value};
use fontoxide::pens::Pen;
use fontoxide::ttlib::ot::types as ot;
use fontoxide::ttlib::pens::TtGlyphPen;
use fontoxide::ttlib::tables::glyf::Glyph;
use fontoxide::ttlib::tables::{Avar, Fvar, Glyf, Gsub, Hmtx};
use fontoxide::ttlib::{Font, ReadOptions, WriteOptions};
use fontoxide::varlib::build::{
	BuildOptions, SourceFonts, build, identity_finder, load_designspace,
};
use fontoxide::varlib::errors::VarLibError;
use fontoxide::varlib::instancer::{
	AxisLimits, AxisTriple, InstancerOptions, instantiate_variable_font, renormalize_location,
};
use indexmap::IndexMap;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const AXIS_NAME: &str = "Optical size";

fn s(v: &str) -> String {
	v.to_owned()
}

fn int_map(map: &[(i64, i64)]) -> Vec<(Number, Number)> {
	map.iter()
		.map(|(a, b)| (Number::Int(*a), Number::Int(*b)))
		.collect()
}

fn approx(a: f64, b: f64, tolerance: f64) -> bool {
	(a - b).abs() <= tolerance
}

/// `pytest.approx` with its default tolerances.
fn approx_default(a: f64, b: f64) -> bool {
	(a - b).abs() <= (1e-6 * b.abs()).max(1e-12)
}

/// `make_axis`.
fn make_axis() -> AxisDescriptor {
	AxisDescriptor {
		name: Some(s(AXIS_NAME)),
		tag: Some(s("opsz")),
		minimum: Number::Int(0),
		default: Number::Int(50),
		maximum: Number::Int(100),
		map: int_map(&[(0, 100), (25, 80), (50, 40), (75, 10), (100, 0)]),
		..Default::default()
	}
}

/// `make_master`.
fn make_master(width: f64) -> Result<Font, Box<dyn std::error::Error>> {
	let mut builder = FontBuilder::new(1000, true, false)?;
	let names = [".notdef", "a", "a.alt"];
	builder.setup_glyph_order(names.iter().map(|n| s(n)).collect());
	builder.setup_character_map(&[(0x61, s("a"))].into_iter().collect(), None, false)?;
	let order = builder.font.glyph_order()?;
	let mut glyphs: IndexMap<String, Glyph> = IndexMap::new();
	for name in names {
		let mut pen = TtGlyphPen::new(None);
		pen.move_to((0.0, 0.0))?;
		pen.line_to((width - 100.0, 0.0))?;
		pen.line_to((width - 100.0, 700.0))?;
		pen.line_to((0.0, 700.0))?;
		pen.close_path()?;
		glyphs.insert(s(name), pen.glyph(&order)?);
	}
	builder.setup_glyf(glyphs, true)?;
	let metrics: IndexMap<String, (f64, f64)> =
		names.iter().map(|n| (s(n), (width, 0.0))).collect();
	builder.setup_horizontal_metrics(&metrics)?;
	builder
		.setup_horizontal_header(&[("ascent", Value::Int(800)), ("descent", Value::Int(-200))])?;
	builder.setup_name_table(
		&[
			(NameKey::from("familyName"), NameValue::from("Reversed")),
			(NameKey::from("styleName"), NameValue::from("Regular")),
		],
		true,
		true,
	)?;
	builder.setup_os2(&[
		("sTypoAscender", Value::Int(800)),
		("sTypoDescender", Value::Int(-200)),
	])?;
	builder.setup_post(true, &[])?;
	Ok(builder.font)
}

/// `make_designspace`: the document and the master fonts of its sources.
fn make_designspace(
	axis: AxisDescriptor,
) -> Result<(DesignSpaceDocument, SourceFonts), Box<dyn std::error::Error>> {
	let mut document = DesignSpaceDocument::new();
	let values: Vec<Number> = axis.map.iter().map(|(_, v)| *v).collect();
	let name = axis.name.clone().unwrap_or_default();
	document.add_axis(axis);
	let mut fonts = SourceFonts::new();
	for value in values {
		let mut source = SourceDescriptor::default();
		source
			.design_location
			.insert(name.clone(), LocationValue::Value(value));
		document.add_source(source);
		fonts.push(Some(make_master(value.to_f64() * 10.0 + 500.0)?));
	}
	Ok((document, fonts))
}

fn build_font(
	document: &mut DesignSpaceDocument,
	fonts: &mut SourceFonts,
) -> Result<Font, VarLibError> {
	Ok(build(document, fonts, &identity_finder, &BuildOptions::default())?.0)
}

/// `reload_font`.
fn reload_font(font: &mut Font) -> Result<Font, Box<dyn std::error::Error>> {
	for t in [b"hmtx", b"vmtx"] {
		let tag = fontoxide::misc::Tag::new(t);
		if font.contains(tag) {
			font.load(tag)?;
		}
	}
	Ok(Font::read(
		font.write(&WriteOptions::default())?,
		ReadOptions::default(),
	)?)
}

#[test]
fn test_master_normalization_preserves_user_axis_direction() -> TestResult {
	type Case = (i64, i64, i64, &'static [(i64, i64)], &'static [f64]);
	let cases: [Case; 4] = [
		(
			0,
			50,
			100,
			&[(0, 100), (25, 80), (50, 40), (100, 0)],
			&[-1.0, -2.0 / 3.0, 0.0, 1.0],
		),
		(
			9,
			144,
			144,
			&[(9, 38), (42, 33), (72, 28), (144, 23)],
			&[-1.0, -2.0 / 3.0, -1.0 / 3.0, 0.0],
		),
		(0, 0, 100, &[(0, 100), (25, 80), (100, 0)], &[0.0, 0.2, 1.0]),
		(
			0,
			50,
			100,
			&[(0, 0), (25, 10), (50, 40), (100, 100)],
			&[-1.0, -0.75, 0.0, 1.0],
		),
	];
	for (minimum, default, maximum, mapping, expected) in cases {
		let mut axis = make_axis();
		axis.minimum = Number::Int(minimum);
		axis.default = Number::Int(default);
		axis.maximum = Number::Int(maximum);
		axis.map = int_map(mapping);
		let (mut document, _) = make_designspace(axis)?;
		document.add_axis(AxisDescriptor {
			name: Some(s("Weight")),
			tag: Some(s("wght")),
			minimum: Number::Int(0),
			default: Number::Int(0),
			maximum: Number::Int(1),
			..Default::default()
		});
		for (source, &value) in document.sources.iter_mut().zip(expected) {
			let weight = if value == 0.0 {
				Number::Int(0)
			} else {
				Number::Float(0.5)
			};
			source
				.design_location
				.insert(s("Weight"), LocationValue::Value(weight));
		}

		let data = load_designspace(&mut document, true, true)?;

		let get = |loc: &[(String, f64)], name: &str| {
			loc.iter().find(|(k, _)| k == name).map_or(0.0, |(_, v)| *v)
		};
		let actual: Vec<f64> = data
			.normalized_master_locs
			.iter()
			.map(|l| get(l, AXIS_NAME))
			.collect();
		assert_eq!(actual.len(), expected.len());
		for (a, e) in actual.iter().zip(expected) {
			assert!(approx_default(*a, *e), "{actual:?} != {expected:?}");
		}
		let weights: Vec<f64> = data
			.normalized_master_locs
			.iter()
			.map(|l| get(l, "Weight"))
			.collect();
		let expected_weights: Vec<f64> = expected
			.iter()
			.map(|&v| if v == 0.0 { 0.0 } else { 0.5 })
			.collect();
		assert_eq!(weights, expected_weights);
		let map = document.axes[0]
			.as_continuous()
			.map(|a| a.map.clone())
			.unwrap_or_default();
		assert_eq!(map, int_map(mapping));
	}
	Ok(())
}

#[test]
fn test_reversed_axis_build_and_interpolation() -> TestResult {
	let (mut document, mut fonts) = make_designspace(make_axis())?;
	let mut font = build_font(&mut document, &mut fonts)?;
	let mut font = reload_font(&mut font)?;

	let axis = &font.require::<Fvar>()?.axes[0];
	assert_eq!(
		(axis.min_value, axis.default_value, axis.max_value),
		(0.0, 50.0, 100.0)
	);
	for (user_value, width) in [
		(0.0, 1500),
		(25.0, 1300),
		(50.0, 900),
		(62.5, 750),
		(75.0, 600),
		(100.0, 500),
	] {
		let mut instance = font.clone();
		let limits: AxisLimits = [(s("opsz"), Some(AxisTriple::pin(user_value)))]
			.into_iter()
			.collect();
		instantiate_variable_font(&mut instance, &limits, &InstancerOptions::default())?;
		let gid = instance.glyph_id("a")?;
		assert_eq!(instance.require::<Hmtx>()?.metrics[&gid].0, width);
		let x_max = instance.require::<Glyf>()?.glyphs[&gid].x_max;
		assert_eq!(x_max, i64::from(width) - 100);
	}
	Ok(())
}

/// A rule substituting `a.alt` for `a` in a range of the axis.
fn rule(minimum: Option<i64>, maximum: Option<i64>) -> RuleDescriptor {
	RuleDescriptor {
		name: None,
		condition_sets: vec![vec![Condition {
			name: s(AXIS_NAME),
			minimum: minimum.map(Number::Int),
			maximum: maximum.map(Number::Int),
		}]],
		subs: vec![(s("a"), s("a.alt"))],
	}
}

#[test]
fn test_feature_variation_bounds_follow_design_axis_direction() -> TestResult {
	let cases = [
		(true, Some(20), Some(80), (-2.0 / 3.0, 0.5)),
		(true, None, Some(80), (-2.0 / 3.0, 1.0)),
		(true, Some(20), None, (-1.0, 0.5)),
		(false, Some(20), Some(80), (-0.5, 2.0 / 3.0)),
		(false, None, Some(80), (-1.0, 2.0 / 3.0)),
		(false, Some(20), None, (-0.5, 1.0)),
	];
	for (reversed_axis, minimum, maximum, expected) in cases {
		let mut axis = make_axis();
		if !reversed_axis {
			axis.map = int_map(&[(0, 0), (25, 10), (50, 40), (75, 80), (100, 100)]);
		}
		let (mut document, mut fonts) = make_designspace(axis)?;
		let rule = rule(minimum, maximum);
		document.add_rule(rule.clone());

		let mut font = build_font(&mut document, &mut fonts)?;
		let mut font = reload_font(&mut font)?;
		let gsub = font.require::<Gsub>()?;
		let records = &gsub
			.table
			.feature_variations
			.as_ref()
			.expect("FeatureVariations")
			.feature_variation_record;
		assert_eq!(records.len(), 1);
		let condition = records[0]
			.condition_set
			.as_ref()
			.and_then(|c| c.condition_table[0].as_deref())
			.expect("a condition");
		let ot::ConditionTable::Format1(condition) = condition else {
			panic!("{condition:?}");
		};
		assert_eq!(condition.axis_index, 0);
		let bounds = (
			condition.filter_range_min_value,
			condition.filter_range_max_value,
		);
		let tolerance = 1.0 / 16384.0;
		assert!(
			approx(bounds.0, expected.0, tolerance) && approx(bounds.1, expected.1, tolerance),
			"{bounds:?} != {expected:?}"
		);
		let samples: [(i64, f64); 4] = if reversed_axis {
			[(95, -11.0 / 12.0), (65, -5.0 / 12.0), (40, 0.0), (5, 0.875)]
		} else {
			[(5, -0.875), (30, -0.25), (60, 1.0 / 3.0), (95, 11.0 / 12.0)]
		};
		for (design_value, normalized_value) in samples {
			let location: SimpleLocation = [(s(AXIS_NAME), Number::Int(design_value))]
				.into_iter()
				.collect();
			assert_eq!(
				bounds.0 <= normalized_value && normalized_value <= bounds.1,
				evaluate_conditions(&rule.condition_sets[0], &location)?,
				"{design_value}"
			);
		}
	}
	Ok(())
}

#[test]
fn test_invalid_feature_variation_bounds_are_not_sorted() -> TestResult {
	for reversed_axis in [false, true] {
		let mut axis = make_axis();
		if !reversed_axis {
			axis.map = int_map(&[(0, 0), (50, 40), (100, 100)]);
		}
		let (mut document, mut fonts) = make_designspace(axis)?;
		document.add_rule(rule(Some(80), Some(20)));
		let err = build_font(&mut document, &mut fonts).expect_err("a validation error");
		// fontoxide-varlib's `featureVars` raises its `VarLibValidationError`s as
		// errors of fontoxide-ttlib, with the same message.
		let message = "A condition set has a minimum value above the maximum value.";
		assert!(
			matches!(&err, VarLibError::Validation(m) if m == message)
				|| matches!(&err, VarLibError::Ttlib(e) if e.to_string() == message),
			"{err:?}"
		);
	}
	Ok(())
}

#[test]
fn test_reversed_axis_avar2_mapping_uses_user_axis_direction() -> TestResult {
	let (mut document, mut fonts) = make_designspace(make_axis())?;
	let location =
		|v: i64| -> SimpleLocation { [(s(AXIS_NAME), Number::Int(v))].into_iter().collect() };
	document.axis_mappings = vec![
		AxisMappingDescriptor {
			input_location: location(80),
			output_location: location(70),
			..Default::default()
		},
		AxisMappingDescriptor {
			input_location: location(10),
			output_location: location(20),
			..Default::default()
		},
	];

	let mut font = build_font(&mut document, &mut fonts)?;
	let mut font = reload_font(&mut font)?;
	assert_eq!(font.require::<Avar>()?.major_version, 2);
	for (normalized_input, expected) in [(-0.5, -0.5), (0.0, 0.0), (0.5, 0.5)] {
		let location: BTreeMap<String, f64> = [(s("opsz"), normalized_input)].into_iter().collect();
		let result = renormalize_location(&location, &mut font)?;
		assert!(
			approx(result["opsz"], expected, 1.0 / 16384.0),
			"{normalized_input}: {result:?}"
		);
	}
	Ok(())
}
