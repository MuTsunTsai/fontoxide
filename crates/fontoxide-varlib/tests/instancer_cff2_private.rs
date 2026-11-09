//! Port of fontTools' `Tests/varLib/instancer/cff2_private_test.py`.
//!
//! The fonts are built with `FontBuilder` by
//! `tools/reference/instancer_module_fixtures.py`.

#![allow(clippy::type_complexity)]

mod common;
mod module_support;

use std::collections::BTreeMap;

use common::{instantiate, pin, triple};
use fontoxide_ttlib::Font;
use fontoxide_ttlib::cff::dict::Value;
use fontoxide_ttlib::cff::transforms::{all_privates, private_of};
use fontoxide_ttlib::ot::var_store::region_support;
use fontoxide_ttlib::tables::{Cff2, Fvar};
use fontoxide_ttlib::var::support_scalar;
use fontoxide_varlib::instancer::test_support::instantiate_cff2;
use fontoxide_varlib::instancer::{AxisTriple, NormalizedLimits};
use module_support::*;

/// The `makeFont` font.
fn make_font() -> TestResult<Font> {
	fixture("cff2_private")
}

/// `reload(instancer.instantiateVariableFont(font, limits))`.
fn instance(font: &Font, pairs: &[(&str, Option<AxisTriple>)]) -> TestResult<Font> {
	let mut font = font.clone();
	instantiate(&mut font, pairs)?;
	roundtrip(&mut font)
}

fn number(value: &Value) -> TestResult<f64> {
	match value {
		Value::Int(v) => Ok(*v as f64),
		Value::Float(v) => Ok(*v),
		_ => Err(format!("not a number: {value:?}").into()),
	}
}

/// `privateValues`.
fn private_values(font: &mut Font, name: &str, location: &Location) -> TestResult<Vec<f64>> {
	// A static instance has no `fvar`, and no `VarStore` that needs it.
	let axes: Vec<String> = match font.get::<Fvar>()? {
		Some(fvar) => fvar.axes.iter().map(|a| a.tag_text()).collect(),
		None => Vec::new(),
	};
	let top = &font.require::<Cff2>()?.cff.top_dicts[0];
	let private = private_of(top, all_privates(top)[0]).ok_or("no Private")?;
	let is_array = matches!(name, "StemSnapH" | "StemSnapV" | "BlueValues");
	let value = private.get(name).ok_or("no such field")?;
	let values = match (is_array, value) {
		(true, Value::List(items)) => items,
		(true, _) => return Err("not an array".into()),
		(false, value) => vec![value],
	};
	let mut scalars = Vec::new();
	if let Some(store) = &top.var_store {
		let vs_index = match private.get("vsindex") {
			Some(v) => v.int()? as usize,
			None => 0,
		};
		let data = store.var_data[vs_index].as_ref().ok_or("no VarData")?;
		let regions = store.var_region_list.as_ref().ok_or("no regions")?;
		for &i in &data.var_region_index {
			let support: BTreeMap<String, (f64, f64, f64)> =
				region_support(&regions.region[usize::from(i)], &axes)?;
			scalars.push(support_scalar(
				location,
				support.iter().map(|(t, r)| (t.as_str(), *r)),
			));
		}
	}
	let mut result = Vec::new();
	let mut adjustment = 0.0;
	for value in &values {
		match value {
			Value::List(items) => {
				let mut delta = 0.0;
				for (d, scalar) in items[1..].iter().zip(&scalars) {
					delta += number(d)? * scalar;
				}
				if is_array {
					adjustment += delta;
				} else {
					adjustment = delta;
				}
				result.push(number(&items[0])? + adjustment);
			}
			value => result.push(number(value)?),
		}
	}
	Ok(result)
}

/// `== pytest.approx(expected, rel=0, abs=1e-10)`.
fn assert_approx(actual: &[f64], expected: &[f64], what: &str) {
	assert_eq!(actual.len(), expected.len(), "{what}");
	for (a, e) in actual.iter().zip(expected) {
		assert!(
			(a - e).abs() <= 1e-10,
			"{what}: {actual:?}, expected {expected:?}"
		);
	}
}

const REAL_FIELDS: [&str; 6] = [
	"BlueScale",
	"ExpansionFactor",
	"StdHW",
	"StdVW",
	"StemSnapH",
	"StemSnapV",
];

#[test]
fn test_fractional_private_blends() -> TestResult {
	let wght_samples: Vec<(Location, Location)> = [-1.0, -0.5, 0.0, 0.5, 1.0]
		.into_iter()
		.map(|weight| {
			(
				loc(&[("wght", weight)]),
				loc(&[("wght", (weight + 1.0) / 2.0)]),
			)
		})
		.collect();
	let cases: Vec<(Vec<(&str, Option<AxisTriple>)>, Vec<(Location, Location)>)> = vec![
		(
			vec![],
			[
				loc(&[]),
				loc(&[("wght", 0.5), ("wdth", 0.5)]),
				loc(&[("wght", 1.0), ("wdth", 1.0)]),
			]
			.into_iter()
			.map(|l| (l.clone(), l))
			.collect(),
		),
		(
			vec![("wght", pin(0.5)), ("wdth", pin(0.5))],
			vec![(loc(&[]), loc(&[("wght", 0.5), ("wdth", 0.5)]))],
		),
		(
			vec![("wght", pin(0.5))],
			[0.0, 0.5, 1.0]
				.into_iter()
				.map(|width| {
					(
						loc(&[("wdth", width)]),
						loc(&[("wght", 0.5), ("wdth", width)]),
					)
				})
				.collect(),
		),
		(vec![("wght", triple(0.0, 0.5, 1.0))], wght_samples.clone()),
		(
			vec![("wght", triple(0.0, 0.5, 1.0)), ("wdth", pin(0.5))],
			wght_samples
				.iter()
				.map(|(l, o)| {
					let mut o = o.clone();
					o.insert("wdth".to_owned(), 0.5);
					(l.clone(), o)
				})
				.collect(),
		),
	];
	for name in REAL_FIELDS {
		for (limits, samples) in &cases {
			let mut original = make_font()?;
			let mut instance = instance(&original, limits)?;
			for (location, original_location) in samples {
				assert_approx(
					&private_values(&mut instance, name, location)?,
					&private_values(&mut original, name, original_location)?,
					name,
				);
			}
		}
	}
	Ok(())
}

#[test]
fn test_integer_private_blends_still_round() -> TestResult {
	let mut font = make_font()?;
	let limits = NormalizedLimits(
		[
			("wght".to_owned(), normalized(0.0, 0.5, 1.0)),
			("wdth".to_owned(), normalized(0.0, 0.0, 0.0)),
		]
		.into_iter()
		.collect(),
	);
	instantiate_cff2(&mut font, &limits, true, None)?;
	let mut font = roundtrip(&mut font)?;

	assert_eq!(
		private_values(&mut font, "BlueValues", &loc(&[]))?,
		[-18.0, 2.0]
	);
	assert_eq!(
		private_values(&mut font, "BlueValues", &loc(&[("wght", 1.0)]))?,
		[-16.0, 4.0]
	);
	Ok(())
}

#[test]
fn test_fractional_private_blends_inherited_vsindex() -> TestResult {
	for name in REAL_FIELDS {
		let mut original = fixture("cff2_private_inherited_vsindex")?;

		let mut instance = instance(
			&original,
			&[("wght", triple(0.0, 0.5, 1.0)), ("wdth", pin(0.5))],
		)?;

		assert_approx(
			&private_values(&mut instance, name, &loc(&[]))?,
			&private_values(&mut original, name, &loc(&[("wght", 0.5), ("wdth", 0.5)]))?,
			name,
		);
	}
	Ok(())
}

#[test]
fn test_fractional_private_blends_repeated_instancing() -> TestResult {
	for name in REAL_FIELDS {
		let mut original = make_font()?;
		let partial = instance(&original, &[("wght", triple(0.0, 0.5, 1.0))])?;
		let mut instance = instance(&partial, &[("wght", pin(0.75)), ("wdth", pin(0.5))])?;

		assert_approx(
			&private_values(&mut instance, name, &loc(&[]))?,
			&private_values(&mut original, name, &loc(&[("wght", 0.75), ("wdth", 0.5)]))?,
			name,
		);
	}
	Ok(())
}

#[test]
fn test_fractional_private_blends_culling() -> TestResult {
	for name in REAL_FIELDS {
		let mut original = make_font()?;
		let mut instance = roundtrip(&mut original)?;
		let ranges: BTreeMap<String, (f64, f64)> =
			[("wdth".to_owned(), (-1.0, -0.5))].into_iter().collect();
		instantiate_cff2(
			&mut instance,
			&NormalizedLimits::default(),
			true,
			Some(&ranges),
		)?;
		let mut instance = roundtrip(&mut instance)?;

		for weight in [0.0, 0.5, 1.0] {
			let location = loc(&[("wght", weight)]);
			assert_approx(
				&private_values(&mut instance, name, &location)?,
				&private_values(&mut original, name, &location)?,
				name,
			);
		}
	}
	Ok(())
}
