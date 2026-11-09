//! The other tests of fontTools' `Tests/varLib/instancer/instancer_test.py`: whole fonts,
//! limits of tuple variations and of axes, and the command line.

#![allow(clippy::type_complexity)]

mod common;
mod instancer_common;

use std::collections::BTreeMap;

use common::*;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::glyf::Number;
use fontoxide_ttlib::tables::tuple_variation::{Delta, TupleVariation};
use fontoxide_ttlib::tables::{Avar, Fvar, Head, Name, Os2};
use fontoxide_ttlib::{Font, Tag, WriteOptions};
use fontoxide_varlib::instancer::cli::{main, parse_limits};
use fontoxide_varlib::instancer::{
	AxisTriple, InstancerOptions, NormalizedAxisTriple, OverlapMode, change_axis_limit,
	limit_feature_variation_condition_range, sanity_check_variable_tables,
};
use instancer_common::*;

fn tag(text: &str) -> Tag {
	Tag::from_str_padded(text).expect("a tag")
}

/// `TupleVariation(axes, values)`, of possibly fractional deltas.
fn float_variation(pairs: &[(&str, (f64, f64, f64))], values: &[f64]) -> TupleVariation {
	TupleVariation {
		axes: region(pairs),
		coordinates: values
			.iter()
			.map(|v| Some(Delta::Value(Number::Float(*v))))
			.collect(),
	}
}

mod instantiate_variable_font_test {
	use super::*;

	#[test]
	fn test_multiple_instancing() -> TestResult {
		let cases = [
			(100.0, 100.0, "100", "100"),
			(400.0, 100.0, "400", "100"),
			(900.0, 100.0, "900", "100"),
			(100.0, 62.5, "100", "62.5"),
			(400.0, 62.5, "400", "62.5"),
			(900.0, 62.5, "900", "62.5"),
		];
		for (wght, wdth, wght_text, wdth_text) in cases {
			let font = varfont2()?;
			let partial = instantiated(&font, &[("wght", pin(wght))])?;
			let mut instance = instantiated(&partial, &[("wdth", pin(wdth))])?;

			let expected =
				expected_instance_ttx("PartialInstancerTest2", &[wght_text, wdth_text], "")?;

			assert_same_ttx(&dump_ttx(&mut instance)?, &expected, "instance");
		}
		Ok(())
	}

	#[test]
	fn test_default_instance() -> TestResult {
		let font = varfont2()?;
		let mut instance = instantiated(&font, &[("wght", None), ("wdth", None)])?;

		let expected = expected_instance_ttx("PartialInstancerTest2", &["400", "100"], "")?;

		assert_same_ttx(&dump_ttx(&mut instance)?, &expected, "instance");
		Ok(())
	}

	#[test]
	fn test_move_weight_width_axis_default() -> TestResult {
		// https://github.com/fonttools/fonttools/issues/2885
		let mut font = varfont2()?;
		let os2 = font.require::<Os2>()?;
		assert_eq!(os2.us_weight_class, 400);
		assert_eq!(os2.us_width_class, 5);

		let mut varfont = instantiated(
			&font,
			&[("wght", triple(100.0, 500.0, 900.0)), ("wdth", pin(87.5))],
		)?;

		let os2 = varfont.require::<Os2>()?;
		assert_eq!(os2.us_weight_class, 500);
		assert_eq!(os2.us_width_class, 4);
		Ok(())
	}

	/// The cases of `OverlapMode.REMOVE` need removing overlaps, which is not ported yet.
	#[test]
	fn test_overlap() -> TestResult {
		let font = varfont3()?;
		let mut instance = instantiated_with(
			&font,
			&[("wght", pin(400.0))],
			&InstancerOptions {
				overlap: OverlapMode::KeepAndDontSetFlags,
				..Default::default()
			},
		)?;

		let expected =
			expected_instance_ttx("PartialInstancerTest3", &["400"], "-no-overlap-flags")?;

		assert_same_ttx(&dump_ttx(&mut instance)?, &expected, "instance");
		Ok(())
	}

	#[test]
	fn test_singlepos() -> TestResult {
		let font = font_from_ttx(&instancer_data("SinglePos.ttx"))?;

		let mut instance = instantiated(&font, &[("wght", pin(280.0)), ("opsz", pin(18.0))])?;

		let expected = expected_instance_ttx("SinglePos", &["280", "18"], "")?;

		assert_same_ttx(&dump_ttx(&mut instance)?, &expected, "instance");
		Ok(())
	}

	#[test]
	fn test_var_composite() -> TestResult {
		let font = open(&tests_path("ttLib/data/varc-6868.ttf"))?;

		// We currently do not allow this either; although in theory it should be possible.
		for location in ["wght", "0000"] {
			let value = if location == "wght" { 600.0 } else { 0.5 };
			let error = instantiated(&font, &[(location, pin(value))])
				.err()
				.ok_or("no error")?
				.to_string();
			assert!(error.contains("is not supported."), "{error}");
		}
		Ok(())
	}
}

mod limit_tuple_variation_axis_ranges_test {
	use super::*;

	fn check_limit_single_var_axis_range(
		var: TupleVariation,
		axis_tag: &str,
		axis_range: NormalizedAxisTriple,
		expected: &[TupleVariation],
	) -> TestResult {
		let result = change_axis_limit(var, axis_tag, axis_range)?;

		assert_eq!(result.len(), expected.len());
		for (v1, v2) in result.iter().zip(expected) {
			assert_eq!(v1.coordinates.len(), v2.coordinates.len());
			for (a, b) in v1.coordinates.iter().zip(&v2.coordinates) {
				let (Some(Delta::Value(a)), Some(Delta::Value(b))) = (a, b) else {
					return Err("not values".into());
				};
				assert!(approx(a.value(), b.value()), "{a:?} {b:?}");
			}
			assert_eq!(
				v1.axes.keys().collect::<Vec<_>>(),
				v2.axes.keys().collect::<Vec<_>>()
			);
			for (k, p) in &v1.axes {
				let q = v2.axes[k];
				assert!(
					approx(p.0, q.0) && approx(p.1, q.1) && approx(p.2, q.2),
					"{p:?} {q:?}"
				);
			}
		}
		Ok(())
	}

	type Tent = (f64, f64, f64);

	fn var(pairs: &[(&str, Tent)], value: f64) -> TupleVariation {
		float_variation(pairs, &[value, value])
	}

	#[test]
	fn test_positive_var() -> TestResult {
		const FULL: Tent = (0.0, 1.0, 1.0);
		const MID: Tent = (0.0, 0.5, 1.0);
		const LOW: Tent = (0.0, 0.2, 1.0);
		let wght = |tent: Tent, value: f64| var(&[("wght", tent)], value);
		let cases: Vec<(TupleVariation, &str, f64, Vec<TupleVariation>)> = vec![
			(wght(FULL, 100.0), "wdth", 0.5, vec![wght(FULL, 100.0)]),
			(wght(FULL, 100.0), "wght", 0.5, vec![wght(FULL, 50.0)]),
			(wght(FULL, 100.0), "wght", 0.8, vec![wght(FULL, 80.0)]),
			(wght(FULL, 100.0), "wght", 1.0, vec![wght(FULL, 100.0)]),
			(wght(FULL, 100.0), "wght", 0.0, vec![]),
			(wght((0.5, 1.0, 1.0), 100.0), "wght", 0.4, vec![]),
			(wght(MID, 100.0), "wght", 0.5, vec![wght(FULL, 100.0)]),
			(wght(MID, 100.0), "wght", 0.4, vec![wght(FULL, 80.0)]),
			(
				wght(MID, 100.0),
				"wght",
				0.6,
				vec![
					wght((0.0, 0.833334, 1.0), 100.0),
					wght((0.833334, 1.0, 1.0), 80.0),
				],
			),
			(
				wght(LOW, 100.0),
				"wght",
				0.4,
				vec![wght((0.0, 0.5, 1.0), 100.0), wght((0.5, 1.0, 1.0), 75.0)],
			),
			(
				wght(LOW, 100.0),
				"wght",
				0.5,
				vec![wght((0.0, 0.4, 1.0), 100.0), wght((0.4, 1.0, 1.0), 62.5)],
			),
			(
				wght((0.5, 0.5, 1.0), 100.0),
				"wght",
				0.5,
				vec![wght((1.0, 1.0, 1.0), 100.0)],
			),
			// test case from https://github.com/fonttools/fonttools/issues/3453
			(
				var(&[("wght", FULL), ("ital", (0.0, 0.0, 1.0))], 100.0), // no-op axis gets dropped
				"ital",
				0.0,
				vec![wght(FULL, 100.0)],
			),
		];
		for (variation, axis_tag, new_max, expected) in cases {
			let axis_range = NormalizedAxisTriple::new(0.0, 0.0, new_max, 1.0, 1.0)?;
			check_limit_single_var_axis_range(variation, axis_tag, axis_range, &expected)?;
		}
		Ok(())
	}

	#[test]
	fn test_negative_var() -> TestResult {
		const FULL: Tent = (-1.0, -1.0, 0.0);
		const MID: Tent = (-1.0, -0.5, 0.0);
		const LOW: Tent = (-1.0, -0.2, 0.0);
		let wght = |tent: Tent, value: f64| var(&[("wght", tent)], value);
		let cases: Vec<(TupleVariation, &str, f64, Vec<TupleVariation>)> = vec![
			(wght(FULL, 100.0), "wdth", -0.5, vec![wght(FULL, 100.0)]),
			(wght(FULL, 100.0), "wght", -0.5, vec![wght(FULL, 50.0)]),
			(wght(FULL, 100.0), "wght", -0.8, vec![wght(FULL, 80.0)]),
			(wght(FULL, 100.0), "wght", -1.0, vec![wght(FULL, 100.0)]),
			(wght(FULL, 100.0), "wght", 0.0, vec![]),
			(wght((-1.0, -1.0, -0.5), 100.0), "wght", -0.4, vec![]),
			(wght(MID, 100.0), "wght", -0.5, vec![wght(FULL, 100.0)]),
			(wght(MID, 100.0), "wght", -0.4, vec![wght(FULL, 80.0)]),
			(
				wght(MID, 100.0),
				"wght",
				-0.6,
				vec![
					wght((-1.0, -0.833334, 0.0), 100.0),
					wght((-1.0, -1.0, -0.833334), 80.0),
				],
			),
			(
				wght(LOW, 100.0),
				"wght",
				-0.4,
				vec![
					wght((-1.0, -0.5, -0.0), 100.0),
					wght((-1.0, -1.0, -0.5), 75.0),
				],
			),
			(
				wght(LOW, 100.0),
				"wght",
				-0.5,
				vec![
					wght((-1.0, -0.4, 0.0), 100.0),
					wght((-1.0, -1.0, -0.4), 62.5),
				],
			),
			(
				wght((-1.0, -0.5, -0.5), 100.0),
				"wght",
				-0.5,
				vec![wght((-1.0, -1.0, -1.0), 100.0)],
			),
		];
		for (variation, axis_tag, new_min, expected) in cases {
			let axis_range = NormalizedAxisTriple::new(new_min, 0.0, 0.0, 1.0, 1.0)?;
			check_limit_single_var_axis_range(variation, axis_tag, axis_range, &expected)?;
		}
		Ok(())
	}
}

#[test]
fn test_limit_feature_variation_condition_range() -> TestResult {
	type Case = ((f64, f64), (f64, f64, f64), Option<(f64, f64)>);
	let cases: [Case; 10] = [
		((1.0, -1.0), (-1.0, 0.0, 1.0), None), // invalid oldRange min > max
		((0.6, 1.0), (0.0, 0.0, 0.5), None),
		((-1.0, -0.6), (-0.5, 0.0, 0.0), None),
		((0.4, 1.0), (0.0, 0.0, 0.5), Some((0.8, 1.0))),
		((-1.0, -0.4), (-0.5, 0.0, 0.0), Some((-1.0, -0.8))),
		((0.4, 1.0), (0.0, 0.0, 0.4), Some((1.0, 1.0))),
		((-1.0, -0.4), (-0.4, 0.0, 0.0), Some((-1.0, -1.0))),
		((-0.5, 0.5), (-0.4, 0.0, 0.4), Some((-1.0, 1.0))),
		((0.0, 1.0), (-1.0, 0.0, 0.0), Some((0.0, 0.0))), // or None?
		((-1.0, 0.0), (0.0, 0.0, 1.0), Some((0.0, 0.0))), // or None?
	];
	for (old_range, new_limit, expected) in cases {
		let condition = ot::ConditionTableFormat1 {
			axis_index: 0,
			filter_range_min_value: old_range.0,
			filter_range_max_value: old_range.1,
		};

		let result = limit_feature_variation_condition_range(
			&condition,
			NormalizedAxisTriple::new(new_limit.0, new_limit.1, new_limit.2, 1.0, 1.0)?,
		);

		assert_eq!(result, expected, "{old_range:?} {new_limit:?}");
	}
	Ok(())
}

#[test]
fn test_parse_limits() -> TestResult {
	let n = |v: f64| Some(v);
	let single = |tag: &str, a, b, c| vec![(tag.to_owned(), Some((a, b, c)))];
	type Expected = Vec<(String, Option<(Option<f64>, Option<f64>, Option<f64>)>)>;
	let cases: Vec<(Vec<&str>, Expected)> = vec![
		(
			vec!["wght=400", "wdth=100"],
			vec![
				("wght".to_owned(), Some((n(400.0), n(400.0), n(400.0)))),
				("wdth".to_owned(), Some((n(100.0), n(100.0), n(100.0)))),
			],
		),
		(
			vec!["wght=400:900"],
			single("wght", n(400.0), None, n(900.0)),
		),
		(
			vec!["wght=400:700:900"],
			single("wght", n(400.0), n(700.0), n(900.0)),
		),
		(
			vec!["slnt=11.4"],
			single("slnt", n(11.399994), n(11.399994), n(11.399994)),
		),
		(vec!["ABCD=drop"], vec![("ABCD".to_owned(), None)]),
		(vec!["wght=:500:"], single("wght", None, n(500.0), None)),
		(vec!["wght=::700"], single("wght", None, None, n(700.0))),
		(vec!["wght=200::"], single("wght", n(200.0), None, None)),
		(
			vec!["wght=200:300:"],
			single("wght", n(200.0), n(300.0), None),
		),
		(
			vec!["wght=:300:500"],
			single("wght", None, n(300.0), n(500.0)),
		),
		(
			vec!["wght=300::700"],
			single("wght", n(300.0), None, n(700.0)),
		),
		(
			vec!["wght=300:700"],
			single("wght", n(300.0), None, n(700.0)),
		),
		(vec!["wght=:700"], single("wght", None, None, n(700.0))),
		(vec!["wght=200:"], single("wght", n(200.0), None, None)),
	];
	for (limits, expected) in cases {
		let limits_result = parse_limits(&limits)?;
		let expected: BTreeMap<String, _> = expected.into_iter().collect();

		assert_eq!(
			limits_result.keys().collect::<Vec<_>>(),
			expected.keys().collect::<Vec<_>>()
		);
		let same = |a: Option<f64>, b: Option<f64>| match (a, b) {
			(Some(a), Some(b)) => approx(a, b),
			(a, b) => a == b,
		};
		for (axis, triple) in &limits_result {
			match expected[axis] {
				None => assert!(triple.is_none()),
				Some((minimum, default, maximum)) => {
					let triple: AxisTriple = triple.ok_or("a triple")?;
					assert!(
						same(triple.minimum, minimum)
							&& same(triple.default, default)
							&& same(triple.maximum, maximum),
						"{limits:?}: {triple:?}"
					);
				}
			}
		}
	}
	Ok(())
}

#[test]
fn test_parse_limits_invalid() {
	let limits = [
		"abcde=123",
		"=0",
		"wght=:",
		"wght=1:",
		"wght=abcd",
		"wght=x:y",
	];
	let error = parse_limits(&limits).expect_err("not valid").to_string();
	assert!(error.contains("invalid location format"), "{error}");
}

#[test]
fn test_normalize_axis_limits() -> TestResult {
	// 300, 500 come from the font having 100,400,900 fvar axis limits.
	let cases = [
		(range(100.0, 400.0), [-1.0, 0.0, 0.0, 300.0, 500.0]),
		(triple(100.0, 400.0, 400.0), [-1.0, 0.0, 0.0, 300.0, 500.0]),
		(triple(100.0, 300.0, 400.0), [-1.0, -0.5, 0.0, 300.0, 500.0]),
	];
	for (limit, expected) in cases {
		let mut font = varfont()?;
		let limits = user_limits(&mut font, &[("wght", limit)])?;

		let normalized_limits = limits.normalize(&mut font, true)?;

		assert_eq!(normalized_limits, normalized(&[("wght", &expected)]));
	}
	Ok(())
}

#[test]
fn test_normalize_axis_limits_no_avar() -> TestResult {
	let mut font = varfont()?;
	font.remove(Tag::new(b"avar"));

	let limits = user_limits(&mut font, &[("wght", triple(400.0, 400.0, 500.0))])?;
	let normalized_limits = limits.normalize(&mut font, true)?;

	let t = normalized_limits.0["wght"];
	let close = |a: f64, b: f64| a == b || (a - b).abs() <= (1e-4 * b.abs()).max(1e-12);
	assert!(
		close(t.minimum, 0.0)
			&& close(t.default, 0.0)
			&& close(t.maximum, 0.2)
			&& close(t.distance_negative, 300.0)
			&& close(t.distance_positive, 500.0),
		"{t:?}"
	);
	Ok(())
}

#[test]
fn test_normalize_axis_limits_missing_from_fvar() -> TestResult {
	let mut font = varfont()?;
	let limits = user_limits(&mut font, &[("ZZZZ", pin(1000.0))])?;
	let error = limits
		.normalize(&mut font, true)
		.expect_err("not in fvar")
		.to_string();
	assert!(error.contains("not present in fvar"), "{error}");
	Ok(())
}

#[test]
fn test_normalize_axis_limits_quantizes_after_avar_map() -> TestResult {
	// The F2Dot14 quantization must be applied AFTER the avar v1 mapping (and only there), like
	// a shaping engine does: normalize -> map -> quantize. Pinning wght=650 on a 100/400/900 axis
	// through this map yields 5/7 * 0.2 = 0.142857..., which is not F2Dot14-representable; the
	// normalized location must land on the quantized 0.14288330078125.
	let mut font = varfont()?;
	font.require::<Avar>()?.segments.insert(
		tag("wght"),
		vec![(-1.0, -1.0), (0.0, 0.0), (0.7, 0.2), (1.0, 1.0)],
	);

	let limits = user_limits(&mut font, &[("wght", pin(650.0))])?;
	let normalized_limits = limits.normalize(&mut font, true)?;

	let t = normalized_limits.0["wght"];
	for v in [t.minimum, t.default, t.maximum] {
		assert_eq!(v, f2dot14(v));
	}
	assert_eq!(t.default, 0.14288330078125);
	Ok(())
}

#[test]
fn test_sanity_check_variable_tables() -> TestResult {
	let error = sanity_check_variable_tables(&Font::new())
		.expect_err("no fvar")
		.to_string();
	assert!(error.contains("Missing required table fvar"), "{error}");

	let mut font = varfont()?;
	font.remove(Tag::new(b"glyf"));

	let error = sanity_check_variable_tables(&font)
		.expect_err("no glyf")
		.to_string();
	assert!(error.contains("Can't have gvar without glyf"), "{error}");
	Ok(())
}

/// `varfont.save(fontfile)`.
fn save_varfont(dir: &TempDir) -> TestResult<String> {
	let fontfile = dir.file("PartialInstancerTest-VF.ttf");
	let mut font = varfont()?;
	std::fs::write(&fontfile, font.write(&WriteOptions::default())?)?;
	Ok(fontfile)
}

#[test]
fn test_main() -> TestResult {
	let dir = TempDir::new("main")?;
	let fontfile = save_varfont(&dir)?;

	// exits without errors
	assert_eq!(main(&[&fontfile, "wght=400"])?, 0);
	Ok(())
}

#[test]
fn test_main_missing_fvar() -> TestResult {
	let dir = TempDir::new("main-missing-fvar")?;
	let mut font = font_from_ttx(&tests_path("ttLib/data/TestTTF-Regular.ttx"))?;
	let fontfile = dir.file("static.ttf");
	std::fs::write(&fontfile, font.write(&WriteOptions::default())?)?;

	let error = main(&[&fontfile]).expect_err("no fvar").to_string();
	assert!(error.contains("Missing required table fvar"), "{error}");
	Ok(())
}

#[test]
fn test_main_exit_nonexistent_file() {
	let error = main(&[""]).expect_err("no file").to_string();
	assert!(error.contains("No such file ''"), "{error}");
}

#[test]
fn test_main_exit_invalid_location() -> TestResult {
	let dir = TempDir::new("main-invalid-location")?;
	let fontfile = save_varfont(&dir)?;

	let error = main(&[&fontfile, "wght:100"])
		.expect_err("invalid")
		.to_string();
	assert!(error.contains("invalid location format"), "{error}");
	Ok(())
}

#[test]
fn test_main_exit_multiple_limits() -> TestResult {
	let dir = TempDir::new("main-multiple-limits")?;
	let fontfile = save_varfont(&dir)?;

	let error = main(&[&fontfile, "wght=400", "wght=90"])
		.expect_err("multiple")
		.to_string();
	assert!(
		error.contains("Specified multiple limits for the same axis"),
		"{error}"
	);
	Ok(())
}

#[test]
fn test_set_ribbi_bits() -> TestResult {
	let mut varfont = font_from_ttx(&instancer_data("STATInstancerTest.ttx"))?;
	let coordinates: Vec<_> = varfont
		.require::<Fvar>()?
		.instances
		.iter()
		.map(|i| i.coordinates.clone())
		.collect();

	for location in coordinates {
		let limits: Vec<(String, Option<AxisTriple>)> = location
			.iter()
			.map(|(t, v)| (t.to_string(), pin(*v)))
			.collect();
		let limits: Vec<(&str, Option<AxisTriple>)> =
			limits.iter().map(|(t, l)| (t.as_str(), *l)).collect();
		let mut instance = instantiated_with(
			&varfont,
			&limits,
			&InstancerOptions {
				update_font_names: true,
				..Default::default()
			},
		)?;
		let name_id_2 = instance
			.require::<Name>()?
			.get_debug_name(2)
			.ok_or("no name")?;
		let mac_style = instance.require::<Head>()?.mac_style;
		// Just bits 0, 5, 6
		let fs_selection = instance.require::<Os2>()?.fs_selection & 0b1100001;

		let bold = location[&tag("wght")] == 700.0;
		let (name, mac, fs) = if location[&tag("ital")] == 0.0 {
			if bold {
				("Bold", 0b01, 0b0100000)
			} else {
				("Regular", 0b00, 0b1000000)
			}
		} else if bold {
			("Bold Italic", 0b11, 0b0100001)
		} else {
			("Italic", 0b10, 0b0000001)
		};
		assert_eq!(name_id_2, name, "{location:?}");
		assert_eq!(mac_style, mac, "{location:?}");
		assert_eq!(fs_selection, fs, "{location:?}");
	}
	Ok(())
}

/// Regression test for fonttools/fonttools#3634, with TTX based on reproduction process there.
#[test]
fn test_rounds_before_iup() -> TestResult {
	let varfont = font_from_ttx(&instancer_data("3634-VF.ttx"))?;

	// Instantiate at a new default position, sufficient to cause differences when unrounded but
	// not when rounded.
	let mut partial = instantiated(&varfont, &[("wght", triple(401.0, 401.0, 900.0))])?;

	// Save and reload actual result to recalculate bounding box values, etc.
	let mut partial = reload_default(&mut partial)?;

	// Load expected result, then save and reload to normalise TTX output.
	let mut expected = font_from_ttx(&instancer_data("test_results/3634-VF-partial.ttx"))?;
	let mut expected = reload_default(&mut expected)?;

	// Serialise actual and expected to TTX strings, and compare.
	assert_same_ttx(
		&stripped_ttx(&mut partial)?,
		&stripped_ttx(&mut expected)?,
		"partial",
	);
	Ok(())
}
