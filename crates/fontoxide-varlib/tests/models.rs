//! Port of fontTools' `Tests/varLib/models_test.py`, and the doctests of
//! `Lib/fontTools/varLib/models.py`.

#![allow(clippy::type_complexity)]

use std::collections::BTreeMap;

use fontoxide_ttlib::{Error, Result};
use fontoxide_varlib::models::{
	DictLocation, Support, VariationModel, normalize_location, normalize_value,
	normalize_value_decreasing, support_scalar, support_scalar_with,
};

fn loc(items: &[(&str, f64)]) -> DictLocation {
	items.iter().map(|&(k, v)| (k.to_owned(), v)).collect()
}

fn axes(items: &[(&str, (f64, f64, f64))]) -> Support {
	items.iter().map(|&(k, v)| (k.to_owned(), v)).collect()
}

fn ranges(items: &[(&str, (f64, f64))]) -> Vec<(String, (f64, f64))> {
	items.iter().map(|&(k, v)| (k.to_owned(), v)).collect()
}

fn model(locations: &[DictLocation]) -> Result<VariationModel> {
	VariationModel::new(locations, None, false, None)
}

fn tags(order: &[&str]) -> Vec<String> {
	order.iter().map(|t| (*t).to_owned()).collect()
}

fn scalar_extrapolate(
	location: &[(&str, f64)],
	support: &[(&str, (f64, f64, f64))],
	axis_ranges: &[(&str, (f64, f64))],
) -> Result<f64> {
	support_scalar_with(
		&loc(location),
		&axes(support),
		true,
		true,
		Some(&ranges(axis_ranges)),
	)
}

/// `dict == dict` of locations.
fn location_map(location: &DictLocation) -> BTreeMap<String, f64> {
	location.iter().cloned().collect()
}

fn assert_locations(actual: &[DictLocation], expected: &[DictLocation]) {
	let map = |l: &[DictLocation]| l.iter().map(location_map).collect::<Vec<_>>();
	assert_eq!(map(actual), map(expected));
}

fn assert_supports(actual: &[Support], expected: &[Support]) {
	let map = |l: &[Support]| {
		l.iter()
			.map(|s| s.iter().cloned().collect::<BTreeMap<_, _>>())
			.collect::<Vec<_>>()
	};
	assert_eq!(map(actual), map(expected));
}

#[test]
fn test_normalize_location() -> Result<()> {
	let check = |axes_: &Support, value: f64, expected: f64| -> Result<()> {
		let actual = normalize_location(&loc(&[("wght", value)]), axes_, false)?;
		assert_eq!(actual, loc(&[("wght", expected)]), "{value} in {axes_:?}");
		Ok(())
	};

	let a = axes(&[("wght", (100.0, 400.0, 900.0))]);
	check(&a, 400.0, 0.0)?;
	check(&a, 100.0, -1.0)?;
	check(&a, 900.0, 1.0)?;
	check(&a, 650.0, 0.5)?;
	check(&a, 1000.0, 1.0)?;
	check(&a, 0.0, -1.0)?;

	let a = axes(&[("wght", (0.0, 0.0, 1000.0))]);
	check(&a, 0.0, 0.0)?;
	check(&a, -1.0, 0.0)?;
	check(&a, 1000.0, 1.0)?;
	check(&a, 500.0, 0.5)?;
	check(&a, 1001.0, 1.0)?;

	let a = axes(&[("wght", (0.0, 1000.0, 1000.0))]);
	check(&a, 0.0, -1.0)?;
	check(&a, -1.0, -1.0)?;
	check(&a, 500.0, -0.5)?;
	check(&a, 1000.0, 0.0)?;
	check(&a, 1001.0, 0.0)?;
	Ok(())
}

#[test]
fn test_normalize_location_extrapolate() -> Result<()> {
	type Case<'a> = (
		&'a [(&'a str, (f64, f64, f64))],
		&'a [(&'a str, f64)],
		&'a [(&'a str, f64)],
	);
	let cases: &[Case<'_>] = &[
		// lower != default != upper
		(
			&[("wght", (100.0, 400.0, 900.0))],
			&[("wght", 1000.0)],
			&[("wght", 1.2)],
		),
		(
			&[("wght", (100.0, 400.0, 900.0))],
			&[("wght", 900.0)],
			&[("wght", 1.0)],
		),
		(
			&[("wght", (100.0, 400.0, 900.0))],
			&[("wght", 650.0)],
			&[("wght", 0.5)],
		),
		(
			&[("wght", (100.0, 400.0, 900.0))],
			&[("wght", 400.0)],
			&[("wght", 0.0)],
		),
		(
			&[("wght", (100.0, 400.0, 900.0))],
			&[("wght", 250.0)],
			&[("wght", -0.5)],
		),
		(
			&[("wght", (100.0, 400.0, 900.0))],
			&[("wght", 100.0)],
			&[("wght", -1.0)],
		),
		(
			&[("wght", (100.0, 400.0, 900.0))],
			&[("wght", 25.0)],
			&[("wght", -1.25)],
		),
		// lower == default != upper
		(
			&[
				("wght", (400.0, 400.0, 900.0)),
				("wdth", (100.0, 100.0, 150.0)),
			],
			&[("wght", 1000.0), ("wdth", 200.0)],
			&[("wght", 1.2), ("wdth", 2.0)],
		),
		(
			&[
				("wght", (400.0, 400.0, 900.0)),
				("wdth", (100.0, 100.0, 150.0)),
			],
			&[("wght", 25.0), ("wdth", 25.0)],
			&[("wght", -0.75), ("wdth", -1.5)],
		),
		// lower != default == upper
		(
			&[
				("wght", (100.0, 400.0, 400.0)),
				("wdth", (50.0, 100.0, 100.0)),
			],
			&[("wght", 700.0), ("wdth", 150.0)],
			&[("wght", 1.0), ("wdth", 1.0)],
		),
		(
			&[
				("wght", (100.0, 400.0, 400.0)),
				("wdth", (50.0, 100.0, 100.0)),
			],
			&[("wght", -50.0), ("wdth", 25.0)],
			&[("wght", -1.5), ("wdth", -1.5)],
		),
		// degenerate case with lower == default == upper, normalized location always 0
		(
			&[("wght", (400.0, 400.0, 400.0))],
			&[("wght", 100.0)],
			&[("wght", 0.0)],
		),
		(
			&[("wght", (400.0, 400.0, 400.0))],
			&[("wght", 400.0)],
			&[("wght", 0.0)],
		),
		(
			&[("wght", (400.0, 400.0, 400.0))],
			&[("wght", 700.0)],
			&[("wght", 0.0)],
		),
	];
	for (axes_, location, expected) in cases {
		let actual = normalize_location(&loc(location), &axes(axes_), true)?;
		assert_eq!(location_map(&actual), location_map(&loc(expected)));
	}
	Ok(())
}

#[test]
fn test_support_scalar() -> Result<()> {
	assert_eq!(support_scalar(&loc(&[]), &axes(&[])), 1.0);
	assert_eq!(support_scalar(&loc(&[("wght", 0.2)]), &axes(&[])), 1.0);
	assert_eq!(
		support_scalar(&loc(&[("wght", 0.2)]), &axes(&[("wght", (0.0, 2.0, 3.0))])),
		0.1
	);
	assert_eq!(
		support_scalar(&loc(&[("wght", 2.5)]), &axes(&[("wght", (0.0, 2.0, 4.0))])),
		0.75
	);
	assert_eq!(
		support_scalar(&loc(&[("wght", 3.0)]), &axes(&[("wght", (0.0, 2.0, 2.0))])),
		0.0
	);
	let wght2 = [("wght", (0.0, 2.0))];
	assert_eq!(
		scalar_extrapolate(&[("wght", 3.0)], &[("wght", (0.0, 2.0, 2.0))], &wght2)?,
		1.5
	);
	assert_eq!(
		scalar_extrapolate(&[("wght", -1.0)], &[("wght", (0.0, 2.0, 2.0))], &wght2)?,
		-0.5
	);
	assert_eq!(
		scalar_extrapolate(&[("wght", 3.0)], &[("wght", (0.0, 1.0, 2.0))], &wght2)?,
		-1.0
	);
	assert_eq!(
		scalar_extrapolate(&[("wght", -1.0)], &[("wght", (0.0, 1.0, 2.0))], &wght2)?,
		-1.0
	);
	assert_eq!(
		scalar_extrapolate(
			&[("wght", 2.0)],
			&[("wght", (0.0, 0.75, 1.0))],
			&[("wght", (0.0, 1.0))]
		)?,
		-4.0
	);
	// TypeError.
	let error = support_scalar_with(
		&loc(&[("wght", 2.0)]),
		&axes(&[("wght", (0.0, 0.75, 1.0))]),
		true,
		true,
		None,
	);
	assert!(error.is_err_and(|e| e.to_string().contains("TypeError")));
	Ok(())
}

#[test]
fn test_model_extrapolate() -> Result<()> {
	let locations = [
		loc(&[]),
		loc(&[("a", 1.0)]),
		loc(&[("b", 1.0)]),
		loc(&[("a", 1.0), ("b", 1.0)]),
	];
	let model = VariationModel::new(&locations, None, true, None)?;
	let master_values = [100.0, 200.0, 300.0, 400.0];
	let cases = [
		(-1.0, -1.0, -200.0),
		(-1.0, 0.0, 0.0),
		(-1.0, 1.0, 200.0),
		(-1.0, 2.0, 400.0),
		(0.0, -1.0, -100.0),
		(0.0, 0.0, 100.0),
		(0.0, 1.0, 300.0),
		(0.0, 2.0, 500.0),
		(1.0, -1.0, 0.0),
		(1.0, 0.0, 200.0),
		(1.0, 1.0, 400.0),
		(1.0, 2.0, 600.0),
		(2.0, -1.0, 100.0),
		(2.0, 0.0, 300.0),
		(2.0, 1.0, 500.0),
		(2.0, 2.0, 700.0),
	];
	for (a, b, expected) in cases {
		let location = loc(&[("a", a), ("b", b)]);
		assert_eq!(
			model.interpolate_from_masters(&location, &master_values)?,
			Some(expected),
			"{location:?}"
		);
	}
	Ok(())
}

fn modeling_error(num_locations: usize, num_samples: usize) -> Result<()> {
	// https://github.com/fonttools/fonttools/issues/2213
	let locations: Vec<DictLocation> = (0..num_locations)
		.map(|i| loc(&[("axis", i as f64 / num_locations as f64)]))
		.collect();
	let master_values: Vec<f64> = (0..num_locations)
		.map(|i| if i != 0 { 100.0 } else { 0.0 })
		.collect();

	let model = model(&locations)?;

	for i in 0..num_samples {
		let location = loc(&[("axis", i as f64 / num_samples as f64)]);
		let scalars = model.get_scalars(&location)?;

		let deltas_float = model.get_deltas(&master_values)?;
		let deltas_round = model.get_deltas_rounded(&master_values, f64::round_ties_even)?;

		let expected =
			VariationModel::interpolate_from_deltas_and_scalars(&deltas_float, &scalars)?
				.ok_or_else(|| Error::ttlib("None"))?;
		let actual = VariationModel::interpolate_from_deltas_and_scalars(&deltas_round, &scalars)?
			.ok_or_else(|| Error::ttlib("None"))?;

		let err = (actual - expected).abs();
		assert!(err <= 0.5, "({i}, {err})");
	}
	Ok(())
}

#[test]
fn test_modeling_error() -> Result<()> {
	modeling_error(127, 509)?;
	modeling_error(31, 251)
}

fn weights(items: &[&[(usize, f64)]]) -> Vec<Vec<(usize, f64)>> {
	items.iter().map(|w| w.to_vec()).collect()
}

fn check_init(
	locations: &[DictLocation],
	axis_order: Option<&[String]>,
	sorted_locs: &[DictLocation],
	supports: &[Support],
	delta_weights: &[Vec<(usize, f64)>],
) -> Result<()> {
	let model = VariationModel::new(locations, axis_order, false, None)?;
	assert_locations(&model.locations, sorted_locs);
	assert_supports(&model.supports, supports);
	assert_eq!(model.delta_weights, delta_weights);
	Ok(())
}

#[test]
fn test_init_wght_wdth() -> Result<()> {
	check_init(
		&[
			loc(&[("wght", 0.55), ("wdth", 0.0)]),
			loc(&[("wght", -0.55), ("wdth", 0.0)]),
			loc(&[("wght", -1.0), ("wdth", 0.0)]),
			loc(&[("wght", 0.0), ("wdth", 1.0)]),
			loc(&[("wght", 0.66), ("wdth", 1.0)]),
			loc(&[("wght", 0.66), ("wdth", 0.66)]),
			loc(&[("wght", 0.0), ("wdth", 0.0)]),
			loc(&[("wght", 1.0), ("wdth", 1.0)]),
			loc(&[("wght", 1.0), ("wdth", 0.0)]),
		],
		Some(&tags(&["wght"])),
		&[
			loc(&[]),
			loc(&[("wght", -0.55)]),
			loc(&[("wght", -1.0)]),
			loc(&[("wght", 0.55)]),
			loc(&[("wght", 1.0)]),
			loc(&[("wdth", 1.0)]),
			loc(&[("wdth", 1.0), ("wght", 1.0)]),
			loc(&[("wdth", 1.0), ("wght", 0.66)]),
			loc(&[("wdth", 0.66), ("wght", 0.66)]),
		],
		&[
			axes(&[]),
			axes(&[("wght", (-1.0, -0.55, 0.0))]),
			axes(&[("wght", (-1.0, -1.0, -0.55))]),
			axes(&[("wght", (0.0, 0.55, 1.0))]),
			axes(&[("wght", (0.55, 1.0, 1.0))]),
			axes(&[("wdth", (0.0, 1.0, 1.0))]),
			axes(&[("wdth", (0.0, 1.0, 1.0)), ("wght", (0.0, 1.0, 1.0))]),
			axes(&[("wdth", (0.0, 1.0, 1.0)), ("wght", (0.0, 0.66, 1.0))]),
			axes(&[("wdth", (0.0, 0.66, 1.0)), ("wght", (0.0, 0.66, 1.0))]),
		],
		&weights(&[
			&[],
			&[(0, 1.0)],
			&[(0, 1.0)],
			&[(0, 1.0)],
			&[(0, 1.0)],
			&[(0, 1.0)],
			&[(0, 1.0), (4, 1.0), (5, 1.0)],
			&[
				(0, 1.0),
				(3, 0.7555555555555555),
				(4, 0.24444444444444444),
				(5, 1.0),
				(6, 0.66),
			],
			&[
				(0, 1.0),
				(3, 0.7555555555555555),
				(4, 0.24444444444444444),
				(5, 0.66),
				(6, 0.43560000000000004),
				(7, 0.66),
			],
		]),
	)
}

#[test]
fn test_init_foo_bar() -> Result<()> {
	check_init(
		&[
			loc(&[]),
			loc(&[("bar", 0.5)]),
			loc(&[("bar", 1.0)]),
			loc(&[("foo", 1.0)]),
			loc(&[("bar", 0.5), ("foo", 1.0)]),
			loc(&[("bar", 1.0), ("foo", 1.0)]),
		],
		None,
		&[
			loc(&[]),
			loc(&[("bar", 0.5)]),
			loc(&[("bar", 1.0)]),
			loc(&[("foo", 1.0)]),
			loc(&[("bar", 0.5), ("foo", 1.0)]),
			loc(&[("bar", 1.0), ("foo", 1.0)]),
		],
		&[
			axes(&[]),
			axes(&[("bar", (0.0, 0.5, 1.0))]),
			axes(&[("bar", (0.5, 1.0, 1.0))]),
			axes(&[("foo", (0.0, 1.0, 1.0))]),
			axes(&[("bar", (0.0, 0.5, 1.0)), ("foo", (0.0, 1.0, 1.0))]),
			axes(&[("bar", (0.5, 1.0, 1.0)), ("foo", (0.0, 1.0, 1.0))]),
		],
		&weights(&[
			&[],
			&[(0, 1.0)],
			&[(0, 1.0)],
			&[(0, 1.0)],
			&[(0, 1.0), (1, 1.0), (3, 1.0)],
			&[(0, 1.0), (2, 1.0), (3, 1.0)],
		]),
	)
}

/// The third and fourth cases of `test_init` (they only differ by `0` and `0.0`).
#[test]
fn test_init_separate_axes() -> Result<()> {
	check_init(
		&[
			loc(&[]),
			loc(&[("foo", 0.25)]),
			loc(&[("foo", 0.5)]),
			loc(&[("foo", 0.75)]),
			loc(&[("foo", 1.0)]),
			loc(&[("bar", 0.25)]),
			loc(&[("bar", 0.75)]),
			loc(&[("bar", 1.0)]),
		],
		None,
		&[
			loc(&[]),
			loc(&[("bar", 0.25)]),
			loc(&[("bar", 0.75)]),
			loc(&[("bar", 1.0)]),
			loc(&[("foo", 0.25)]),
			loc(&[("foo", 0.5)]),
			loc(&[("foo", 0.75)]),
			loc(&[("foo", 1.0)]),
		],
		&[
			axes(&[]),
			axes(&[("bar", (0.0, 0.25, 1.0))]),
			axes(&[("bar", (0.25, 0.75, 1.0))]),
			axes(&[("bar", (0.75, 1.0, 1.0))]),
			axes(&[("foo", (0.0, 0.25, 1.0))]),
			axes(&[("foo", (0.25, 0.5, 1.0))]),
			axes(&[("foo", (0.5, 0.75, 1.0))]),
			axes(&[("foo", (0.75, 1.0, 1.0))]),
		],
		&weights(&[
			&[],
			&[(0, 1.0)],
			&[(0, 1.0), (1, 0.3333333333333333)],
			&[(0, 1.0)],
			&[(0, 1.0)],
			&[(0, 1.0), (4, 0.6666666666666666)],
			&[(0, 1.0), (4, 0.3333333333333333), (5, 0.5)],
			&[(0, 1.0)],
		]),
	)
}

fn assert_not_unique(result: Result<VariationModel>) {
	match result {
		Err(e) => assert!(e.to_string().contains("Locations must be unique."), "{e}"),
		Ok(_) => panic!("expected a VariationModelError"),
	}
}

#[test]
fn test_init_duplicate_locations() {
	assert_not_unique(model(&[
		loc(&[("foo", 0.0), ("bar", 0.0)]),
		loc(&[("foo", 1.0), ("bar", 1.0)]),
		loc(&[("bar", 1.0), ("foo", 1.0)]),
	]));
}

#[test]
fn test_init_duplicate_locations_after_stripping_zero_axes() {
	assert_not_unique(model(&[loc(&[]), loc(&[("foo", 0.0)])]));
}

#[test]
fn test_get_sub_model_preserves_extrapolate_and_axis_ranges() -> Result<()> {
	let axis_ranges = ranges(&[("foo", (-2.0, 2.0))]);
	let model = VariationModel::new(
		&[loc(&[]), loc(&[("foo", 1.0)]), loc(&[("foo", 2.0)])],
		None,
		true,
		Some(&axis_ranges),
	)?;

	let (sub_model, items) = model.get_sub_model(&[Some(1.0), None, Some(3.0)])?;

	assert_eq!(items, [1.0, 3.0]);
	assert!(sub_model.extrapolate);
	assert_eq!(sub_model.axis_ranges, axis_ranges);
	assert_eq!(
		sub_model.interpolate_from_masters(&loc(&[("foo", 3.0)]), &[100.0, 200.0])?,
		Some(250.0)
	);
	Ok(())
}

#[test]
fn test_interpolation() -> Result<()> {
	let locations = [
		loc(&[]),
		loc(&[("axis_A", 1.0)]),
		loc(&[("axis_B", 1.0)]),
		loc(&[("axis_A", 1.0), ("axis_B", 1.0)]),
		loc(&[("axis_A", 0.5), ("axis_B", 1.0)]),
		loc(&[("axis_A", 1.0), ("axis_B", 0.5)]),
	];
	let order = tags(&["axis_A", "axis_B"]);
	let master_values = [0.0, 10.0, 20.0, 70.0, 50.0, 60.0];
	let instance_location = loc(&[("axis_A", 0.5), ("axis_B", 0.5)]);
	let expected_value = 37.5;
	let master_scalars = [0.25, 0.0, 0.0, -0.25, 0.5, 0.5];

	let model = VariationModel::new(&locations, Some(&order), false, None)?;

	let interpolated = model.interpolate_from_masters(&instance_location, &master_values)?;
	assert_eq!(interpolated, Some(expected_value));

	assert_eq!(
		model.get_master_scalars(&instance_location)?,
		master_scalars
	);

	// pytest.approx.
	let actual =
		VariationModel::interpolate_from_values_and_scalars(&master_values, &master_scalars)?
			.ok_or_else(|| Error::ttlib("None"))?;
	assert!((actual - expected_value).abs() <= expected_value * 1e-6);
	Ok(())
}

#[test]
fn test_get_master_scalars() -> Result<()> {
	let wght = |v: f64, w: f64| loc(&[("wght", v), ("wdth", w)]);
	let locations_a = [loc(&[]), loc(&[("wght", 1.0)]), loc(&[("wdth", 1.0)])];
	let locations_b = [
		loc(&[]),
		loc(&[("wght", 1.0)]),
		loc(&[("wdth", 1.0)]),
		loc(&[("wght", 1.0), ("wdth", 1.0)]),
	];
	let locations_c = [
		loc(&[]),
		loc(&[("wght", 0.5)]),
		loc(&[("wght", 1.0)]),
		loc(&[("wdth", 1.0)]),
		loc(&[("wght", 1.0), ("wdth", 1.0)]),
	];
	let cases: Vec<(&[DictLocation], DictLocation, Vec<f64>)> = vec![
		(&locations_a, wght(0.0, 0.0), vec![1.0, 0.0, 0.0]),
		(&locations_a, wght(0.5, 0.0), vec![0.5, 0.5, 0.0]),
		(&locations_a, wght(1.0, 0.0), vec![0.0, 1.0, 0.0]),
		(&locations_a, wght(0.0, 0.5), vec![0.5, 0.0, 0.5]),
		(&locations_a, wght(0.0, 1.0), vec![0.0, 0.0, 1.0]),
		(&locations_a, wght(1.0, 1.0), vec![-1.0, 1.0, 1.0]),
		(&locations_a, wght(0.5, 0.5), vec![0.0, 0.5, 0.5]),
		(&locations_a, wght(0.75, 0.75), vec![-0.5, 0.75, 0.75]),
		(&locations_b, wght(1.0, 1.0), vec![0.0, 0.0, 0.0, 1.0]),
		(&locations_b, wght(0.5, 0.0), vec![0.5, 0.5, 0.0, 0.0]),
		(&locations_b, wght(1.0, 0.5), vec![0.0, 0.5, 0.0, 0.5]),
		(&locations_b, wght(0.5, 0.5), vec![0.25, 0.25, 0.25, 0.25]),
		(&locations_c, wght(0.5, 0.0), vec![0.0, 1.0, 0.0, 0.0, 0.0]),
		(&locations_c, wght(0.25, 0.0), vec![0.5, 0.5, 0.0, 0.0, 0.0]),
		(&locations_c, wght(0.75, 0.0), vec![0.0, 0.5, 0.5, 0.0, 0.0]),
		(
			&locations_c,
			wght(0.5, 1.0),
			vec![-0.5, 1.0, -0.5, 0.5, 0.5],
		),
		(
			&locations_c,
			wght(0.75, 1.0),
			vec![-0.25, 0.5, -0.25, 0.25, 0.75],
		),
	];
	for (locations, location, expected) in cases {
		let model = model(locations)?;
		assert_eq!(
			model.get_master_scalars(&location)?,
			expected,
			"{location:?}"
		);
	}
	Ok(())
}

// Doctests.

/// `normalizeValue`.
#[test]
fn doctest_normalize_value() -> Result<()> {
	let triple = (100.0, 400.0, 900.0);
	assert_eq!(normalize_value(400.0, triple, false)?, 0.0);
	assert_eq!(normalize_value(100.0, triple, false)?, -1.0);
	assert_eq!(normalize_value(650.0, triple, false)?, 0.5);
	let triple = (38.0, 28.0, 23.0);
	assert_eq!(normalize_value_decreasing(38.0, triple, false)?, -1.0);
	assert_eq!(normalize_value_decreasing(33.0, triple, false)?, -0.5);
	assert_eq!(normalize_value_decreasing(23.0, triple, false)?, 1.0);
	Ok(())
}

/// `normalizeLocation`: the same cases as `test_normalizeLocation`.
#[test]
fn doctest_normalize_location() -> Result<()> {
	test_normalize_location()
}

/// `supportScalar`.
#[test]
fn doctest_support_scalar() -> Result<()> {
	let wght_wdth = [("wght", (0.0, 2.0, 4.0)), ("wdth", (-1.0, 0.0, 1.0))];
	assert_eq!(support_scalar(&loc(&[]), &axes(&[])), 1.0);
	assert_eq!(support_scalar(&loc(&[("wght", 0.2)]), &axes(&[])), 1.0);
	assert_eq!(
		support_scalar(&loc(&[("wght", 0.2)]), &axes(&[("wght", (0.0, 2.0, 3.0))])),
		0.1
	);
	assert_eq!(
		support_scalar(&loc(&[("wght", 2.5)]), &axes(&[("wght", (0.0, 2.0, 4.0))])),
		0.75
	);
	assert_eq!(
		support_scalar(&loc(&[("wght", 2.5), ("wdth", 0.0)]), &axes(&wght_wdth)),
		0.75
	);
	assert_eq!(
		support_scalar_with(
			&loc(&[("wght", 2.5), ("wdth", 0.5)]),
			&axes(&wght_wdth),
			false,
			false,
			None
		)?,
		0.375
	);
	assert_eq!(
		support_scalar(&loc(&[("wght", 2.5), ("wdth", 0.5)]), &axes(&wght_wdth)),
		0.75
	);
	let wght2 = [("wght", (0.0, 2.0))];
	assert_eq!(
		scalar_extrapolate(&[("wght", 3.0)], &[("wght", (0.0, 1.0, 2.0))], &wght2)?,
		-1.0
	);
	assert_eq!(
		scalar_extrapolate(&[("wght", -1.0)], &[("wght", (0.0, 1.0, 2.0))], &wght2)?,
		-1.0
	);
	assert_eq!(
		scalar_extrapolate(&[("wght", 3.0)], &[("wght", (0.0, 2.0, 2.0))], &wght2)?,
		1.5
	);
	assert_eq!(
		scalar_extrapolate(&[("wght", -1.0)], &[("wght", (0.0, 2.0, 2.0))], &wght2)?,
		-0.5
	);
	Ok(())
}

/// `VariationModel`.
#[test]
fn doctest_variation_model() -> Result<()> {
	let axis_ranges = ranges(&[("wght", (-180.0, 180.0)), ("wdth", (-1.0, 1.0))]);
	let locations = [
		loc(&[("wght", 100.0)]),
		loc(&[("wght", -100.0)]),
		loc(&[("wght", -180.0)]),
		loc(&[("wdth", 0.3)]),
		loc(&[("wght", 120.0), ("wdth", 0.3)]),
		loc(&[("wght", 120.0), ("wdth", 0.2)]),
		loc(&[]),
		loc(&[("wght", 180.0), ("wdth", 0.3)]),
		loc(&[("wght", 180.0)]),
	];
	let order = tags(&["wght"]);
	let model = VariationModel::new(&locations, Some(&order), false, Some(&axis_ranges))?;
	assert_locations(
		&model.locations,
		&[
			loc(&[]),
			loc(&[("wght", -100.0)]),
			loc(&[("wght", -180.0)]),
			loc(&[("wght", 100.0)]),
			loc(&[("wght", 180.0)]),
			loc(&[("wdth", 0.3)]),
			loc(&[("wdth", 0.3), ("wght", 180.0)]),
			loc(&[("wdth", 0.3), ("wght", 120.0)]),
			loc(&[("wdth", 0.2), ("wght", 120.0)]),
		],
	);
	assert_eq!(
		model.delta_weights,
		weights(&[
			&[],
			&[(0, 1.0)],
			&[(0, 1.0)],
			&[(0, 1.0)],
			&[(0, 1.0)],
			&[(0, 1.0)],
			&[(0, 1.0), (4, 1.0), (5, 1.0)],
			&[
				(0, 1.0),
				(3, 0.75),
				(4, 0.25),
				(5, 1.0),
				(6, 0.6666666666666666)
			],
			&[
				(0, 1.0),
				(3, 0.75),
				(4, 0.25),
				(5, 0.6666666666666667),
				(6, 0.4444444444444445),
				(7, 0.6666666666666667),
			],
		])
	);
	Ok(())
}
