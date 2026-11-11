//! The `VariableScalarBuilder` tests of `Tests/feaLib/variableScalar_test.py`
//! (the value container is tested in `ast.rs`).

use std::collections::HashMap;

use fontoxide_designspacelib::{AxisDescriptor, DesignSpaceDocument, Number};
use fontoxide_fealib::variable_scalar::{VariableScalar, VariableScalarBuilder};
use fontoxide_misc::Tag;
use fontoxide_ttlib::Font;
use fontoxide_ttlib::tables::fvar::Axis;
use fontoxide_ttlib::tables::{Avar, Fvar};
use fontoxide_varlib::models::{VariationModel, normalize_value};
use fontoxide_varlib::var_store::OnlineVarStoreBuilder;
use indexmap::IndexMap;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn approx(a: f64, b: f64) -> bool {
	(a - b).abs() <= 1e-6 * b.abs().max(1e-12)
}

fn wght_scalar() -> VariableScalar {
	let mut scalar = VariableScalar::new();
	scalar.add_value([("wght", 200.0)], 10);
	scalar.add_value([("wght", 400.0)], 40);
	scalar.add_value([("wght", 800.0)], 80);
	scalar
}

/// `test_variable_scalar_interpolation_with_avar[increasing]` and
/// `[decreasing]`: the avar mapping of a designspace is applied when
/// interpolating.
#[test]
fn test_variable_scalar_interpolation_with_avar() -> TestResult {
	let maps: [&[(i64, i64)]; 2] = [
		&[(200, 50), (300, 90), (400, 100), (800, 150)],
		// The same axis with design values decreasing as user values increase.
		&[(200, 150), (300, 110), (400, 100), (800, 50)],
	];
	for axis_map in maps {
		let scalar = wght_scalar();
		let mut doc = DesignSpaceDocument::default();
		doc.add_axis(AxisDescriptor {
			tag: Some("wght".to_owned()),
			minimum: Number::Int(200),
			default: Number::Int(400),
			maximum: Number::Int(800),
			map: axis_map
				.iter()
				.map(|&(u, d)| (Number::Int(u), Number::Int(d)))
				.collect(),
			..Default::default()
		});
		let mut builder = VariableScalarBuilder::from_designspace(&doc)?;
		// With avar, user 300 maps near the design default -> value 34.
		let value = builder.value_at_location(&scalar, &[("wght".to_owned(), 300.0)])?;
		assert!(approx(value, 34.0), "{value}");
		// Without avar, user 300 is halfway between min and default -> value 25.
		let mut builder_no_avar = VariableScalarBuilder {
			axis_triples: builder.axis_triples.clone(),
			axis_mappings: IndexMap::new(),
			model_cache: HashMap::new(),
		};
		let value_no_avar =
			builder_no_avar.value_at_location(&scalar, &[("wght".to_owned(), 300.0)])?;
		assert!(approx(value_no_avar, 25.0), "{value_no_avar}");
	}
	Ok(())
}

/// `test_from_ttf_with_avar`: `from_ttf` reads the `fvar` axes and the `avar`
/// segments, and interpolates like `from_designspace`.
#[test]
fn test_from_ttf_with_avar() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(vec![".notdef".to_owned()]);
	font.insert(Box::new(Fvar {
		axes: vec![Axis {
			axis_tag: *b"wght",
			min_value: 200.0,
			default_value: 400.0,
			max_value: 800.0,
			axis_name_id: 256,
			..Default::default()
		}],
		instances: Vec::new(),
	}));
	let mut avar = Avar::default();
	// Equivalent normalized avar segments for the designspace map
	// [(200, 50), (300, 90), (400, 100), (800, 150)]
	avar.segments.insert(
		Tag::new(b"wght"),
		vec![(-1.0, -1.0), (-0.5, -0.2), (0.0, 0.0), (1.0, 1.0)],
	);
	font.insert(Box::new(avar));
	let mut builder = VariableScalarBuilder::from_ttf(&mut font)?;
	let scalar = wght_scalar();
	// Same result as the from_designspace test above.
	let value = builder.value_at_location(&scalar, &[("wght".to_owned(), 300.0)])?;
	assert!(approx(value, 34.0), "{value}");
	Ok(())
}

/// `test_model_uses_axes_order`: the model uses the axis order of
/// `axis_triples`, not the alphabetical order (fonttools#4053).
#[test]
fn test_model_uses_axes_order() -> TestResult {
	let mut builder = VariableScalarBuilder {
		axis_triples: [
			("wght".to_owned(), (100.0, 400.0, 900.0)),
			("wdth".to_owned(), (62.5, 100.0, 100.0)),
		]
		.into_iter()
		.collect(),
		axis_mappings: IndexMap::new(),
		model_cache: HashMap::new(),
	};
	let mut scalar = VariableScalar::new();
	scalar.add_value([("wght", 100.0), ("wdth", 100.0)], 151);
	scalar.add_value([("wght", 400.0), ("wdth", 100.0)], 176);
	scalar.add_value([("wght", 900.0), ("wdth", 100.0)], 221);
	scalar.add_value([("wght", 100.0), ("wdth", 62.5)], 98);
	scalar.add_value([("wght", 400.0), ("wdth", 62.5)], 111);
	scalar.add_value([("wght", 900.0), ("wdth", 62.5)], 161);
	scalar.add_value([("wght", 705.0), ("wdth", 62.5)], 136);

	// A reference model with an explicit axisOrder matching the builder's.
	let triples = builder.axis_triples.clone();
	let locations = scalar
		.values
		.iter()
		.map(|(location, _)| {
			location
				.iter()
				.map(|(tag, val)| Ok((tag.clone(), normalize_value(*val, triples[tag], false)?)))
				.collect::<Result<Vec<_>, fontoxide_ttlib::Error>>()
		})
		.collect::<Result<Vec<_>, _>>()?;
	let ref_model = VariationModel::new(
		&locations,
		Some(&["wght".to_owned(), "wdth".to_owned()]),
		false,
		None,
	)?;
	let values: Vec<Option<f64>> = scalar.values.iter().map(|(_, v)| Some(*v as f64)).collect();
	let (ref_deltas, ref_supports) =
		ref_model.get_deltas_and_supports_rounded(&values, f64::round_ties_even)?;

	// VariableScalarBuilder.model() produces the same deltas.
	let (deltas, supports) = builder.get_deltas_and_supports(&scalar)?;
	assert_eq!(deltas, ref_deltas);
	assert_eq!(supports, ref_supports);
	Ok(())
}

/// `test_deprecated_add_to_variation_store`: the deprecated
/// `VariableScalar.add_to_variation_store()` gives the same result as
/// `VariableScalarBuilder`.
#[test]
fn test_deprecated_add_to_variation_store() -> TestResult {
	let axes = [
		("wght".to_owned(), 100.0, 400.0, 900.0),
		("wdth".to_owned(), 75.0, 100.0, 125.0),
	];
	let tags: Vec<String> = axes.iter().map(|a| a.0.clone()).collect();
	let mut scalar = VariableScalar::new();
	scalar.add_value([("wght", 100.0), ("wdth", 100.0)], 10);
	scalar.add_value([("wght", 400.0), ("wdth", 100.0)], 20);
	scalar.add_value([("wght", 900.0), ("wdth", 100.0)], 40);

	// The deprecated shim, as fontFeatures uses it.
	let mut store_builder = OnlineVarStoreBuilder::new(tags.clone())?;
	let (default, index) = scalar.add_to_variation_store(&axes, &mut store_builder, None)?;
	assert_eq!(default, 20);

	// The new API.
	let mut builder = VariableScalarBuilder {
		axis_triples: axes
			.iter()
			.map(|(tag, min, default, max)| (tag.clone(), (*min, *default, *max)))
			.collect(),
		axis_mappings: IndexMap::new(),
		model_cache: HashMap::new(),
	};
	let mut store_builder2 = OnlineVarStoreBuilder::new(tags)?;
	let (default2, index2) = builder.add_to_variation_store(&scalar, &mut store_builder2)?;
	assert_eq!(default, default2);
	assert_eq!(index, index2);
	Ok(())
}
