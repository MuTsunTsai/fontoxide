//! Port of fontTools' `Tests/varLib/instancer/cff2_test.py`: CFF2 instancing rounds
//! absolute positions, so contours stay closed.
//!
//! The fonts are built with `FontBuilder` and `varLib.build` by
//! `tools/reference/instancer_module_fixtures.py`.

#![allow(clippy::type_complexity)]

mod common;
mod module_support;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Once;

use common::{instantiate, pin, triple};
use fontoxide_pens::Point;
use fontoxide_ttlib::cff::dict::Value;
use fontoxide_ttlib::cff::transforms::{all_privates, private_of};
use fontoxide_ttlib::tables::{Cff2, Fvar};
use fontoxide_ttlib::{Font, Tag};
use fontoxide_varlib::instancer::test_support::instantiate_cff2;
use fontoxide_varlib::instancer::{AxisTriple, NormalizedLimits};
use module_support::*;

type Pairs<'a> = &'a [(&'a str, Option<AxisTriple>)];

/// `getContours`.
fn get_contours(font: &mut Font, location: &Location) -> TestResult<Vec<Vec<Point>>> {
	let mut contours: Vec<Vec<Point>> = Vec::new();
	for (op, args) in draw(font, "o", location)? {
		if op == "moveTo" {
			contours.push(vec![args[0]]);
		} else if op != "closePath" {
			contours.last_mut().ok_or("no contour")?.extend(args);
		}
	}
	Ok(contours)
}

/// Float noise of drawing at a location.
const EPSILON: f64 = 0.01;

/// `assertClosed`.
fn assert_closed(font: &mut Font, location: &Location) -> TestResult {
	for contour in get_contours(font, location)? {
		let (start, end) = (contour[0], contour[contour.len() - 1]);
		assert!(
			(start.0 - end.0).abs() <= 1e-6 && (start.1 - end.1).abs() <= 1e-6,
			"{location:?}: {start:?}, {end:?}"
		);
	}
	Ok(())
}

/// `assertCloseTo`.
fn assert_close_to(
	font: &mut Font,
	location: &Location,
	reference: &mut Font,
	reference_location: &Location,
	tolerance: f64,
) -> TestResult {
	let got = get_contours(font, location)?;
	let expected = get_contours(reference, reference_location)?;
	assert_eq!(
		got.iter().map(Vec::len).collect::<Vec<_>>(),
		expected.iter().map(Vec::len).collect::<Vec<_>>()
	);
	for (got_contour, expected_contour) in got.iter().zip(&expected) {
		for (g, e) in got_contour.iter().zip(expected_contour) {
			assert!(
				(g.0 - e.0).abs() <= tolerance + EPSILON,
				"{location:?}: {g:?}, {e:?}"
			);
			assert!(
				(g.1 - e.1).abs() <= tolerance + EPSILON,
				"{location:?}: {g:?}, {e:?}"
			);
		}
	}
	Ok(())
}

/// `getProgram`.
fn get_program(font: &mut Font) -> TestResult<Vec<String>> {
	charstring_program(font, "CFF2", "o")
}

/// `reload(instancer.instantiateVariableFont(vf, limits))`.
fn instance(vf: &Font, pairs: Pairs<'_>) -> TestResult<Font> {
	let mut font = vf.clone();
	instantiate(&mut font, pairs)?;
	roundtrip(&mut font)
}

/// `instancer.NormalizedAxisLimits`, from `(tag, (min, default, max))` pairs.
fn normalized_limits(pairs: &[(&str, (f64, f64, f64))]) -> NormalizedLimits {
	NormalizedLimits(
		pairs
			.iter()
			.map(|(tag, (a, b, c))| ((*tag).to_owned(), normalized(*a, *b, *c)))
			.collect::<BTreeMap<_, _>>(),
	)
}

fn position(program: &[String], token: &str) -> usize {
	program
		.iter()
		.position(|t| t == token)
		.unwrap_or_else(|| panic!("no {token} in {program:?}"))
}

// `caplog`: the warnings logged by this thread.
thread_local! {
	static CAPTURED: RefCell<String> = const { RefCell::new(String::new()) };
}

struct Capture;

impl log::Log for Capture {
	fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
		metadata.level() <= log::Level::Warn
	}

	fn log(&self, record: &log::Record<'_>) {
		if self.enabled(record.metadata()) {
			CAPTURED.with(|c| {
				let mut text = c.borrow_mut();
				text.push_str(&record.args().to_string());
				text.push('\n');
			});
		}
	}

	fn flush(&self) {}
}

static CAPTURE: Capture = Capture;
static INIT: Once = Once::new();

fn caplog<R>(f: impl FnOnce() -> R) -> (R, String) {
	INIT.call_once(|| {
		log::set_logger(&CAPTURE).expect("the only logger");
		log::set_max_level(log::LevelFilter::Warn);
	});
	CAPTURED.with(|c| c.borrow_mut().clear());
	let result = f();
	(result, CAPTURED.with(|c| c.borrow().clone()))
}

#[test]
fn test_rebased_default_contours_stay_closed() -> TestResult {
	let mut vf = fixture("cff2_two")?;

	let mut rebased = instance(&vf, &[("wght", triple(300.0, 400.0, 900.0))])?;

	assert_closed(&mut rebased, &loc(&[]))?;
	assert_close_to(
		&mut rebased,
		&loc(&[]),
		&mut vf,
		&loc(&[("wght", 400.0)]),
		0.5,
	)?;
	for weight in (300..=900).step_by(50) {
		let location = loc(&[("wght", f64::from(weight))]);
		assert_closed(&mut rebased, &location)?;
		assert_close_to(&mut rebased, &location, &mut vf, &location, 0.5)?;
	}
	Ok(())
}

#[test]
fn test_static_instance_contours_stay_closed() -> TestResult {
	// https://github.com/fonttools/fonttools/issues/3649
	let mut vf = fixture("cff2_six")?;

	for weight in [350.0, 400.0, 450.0, 500.0, 750.0] {
		for slant in [0.0, -5.5] {
			let location = loc(&[("wght", weight), ("slnt", slant)]);
			let mut instance = instance(&vf, &[("wght", pin(weight)), ("slnt", pin(slant))])?;

			assert!(instance.contains(Tag::new(b"CFF2")));
			assert!(
				instance.require::<Cff2>()?.cff.top_dicts[0]
					.var_store
					.is_none()
			);
			assert_closed(&mut instance, &loc(&[]))?;
			assert_close_to(&mut instance, &loc(&[]), &mut vf, &location, 0.5)?;
		}
	}
	Ok(())
}

#[test]
fn test_overlapping_regions() -> TestResult {
	let mut vf = fixture("cff2_six")?;

	let mut rebased = instance(&vf, &[("wght", triple(300.0, 400.0, 900.0))])?;

	assert_close_to(
		&mut rebased,
		&loc(&[]),
		&mut vf,
		&loc(&[("wght", 400.0)]),
		0.5,
	)?;
	for weight in [300.0, 400.0, 450.0, 600.0, 750.0, 900.0] {
		for slant in [0.0, -5.5, -11.0] {
			let location = loc(&[("wght", weight), ("slnt", slant)]);
			assert_closed(&mut rebased, &location)?;
			// Regions round separately, as in gvar
			assert_close_to(&mut rebased, &location, &mut vf, &location, 1.0)?;
		}
	}
	Ok(())
}

#[test]
fn test_pinned_axis_and_rebased_default() -> TestResult {
	let mut vf = fixture("cff2_six")?;

	let mut italic = instance(
		&vf,
		&[("slnt", pin(-11.0)), ("wght", triple(300.0, 400.0, 900.0))],
	)?;

	let axes: Vec<String> = italic
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| a.tag_text())
		.collect();
	assert_eq!(axes, ["wght"]);
	for weight in [300.0, 400.0, 600.0, 900.0] {
		let location = loc(&[("wght", weight)]);
		assert_closed(&mut italic, &location)?;
		assert_close_to(
			&mut italic,
			&location,
			&mut vf,
			&loc(&[("wght", weight), ("slnt", -11.0)]),
			0.5,
		)?;
	}
	Ok(())
}

#[test]
fn test_fractional_deltas() -> TestResult {
	// Off-end masters leave fractional deltas
	let mut vf = fixture("cff2_off_end")?;

	let mut italic = instance(
		&vf,
		&[("slnt", pin(-11.0)), ("wght", triple(300.0, 400.0, 900.0))],
	)?;

	for weight in (300..=900).step_by(2) {
		let weight = f64::from(weight);
		let location = loc(&[("wght", weight)]);
		assert_closed(&mut italic, &location)?;
		assert_close_to(
			&mut italic,
			&location,
			&mut vf,
			&loc(&[("wght", weight), ("slnt", -11.0)]),
			1.0,
		)?;
	}
	Ok(())
}

#[test]
fn test_no_round() -> TestResult {
	let mut vf = fixture("cff2_six")?;
	let mut reference = roundtrip(&mut vf)?;
	let limits = normalized_limits(&[("wght", (0.0, 0.5, 1.0)), ("slnt", (-1.0, -1.0, -1.0))]);

	instantiate_cff2(&mut vf, &limits, false, None)?;

	// wght 600 is the new default, slnt -11 is pinned; nothing is rounded.
	assert_close_to(
		&mut vf,
		&loc(&[]),
		&mut reference,
		&loc(&[("wght", 600.0), ("slnt", -11.0)]),
		0.0,
	)
}

#[test]
fn test_private_dict_vsindex() -> TestResult {
	// Blends use VarData 1 via the Private DICT; VarData 0 differs
	let mut vf = fixture("cff2_private_dict_vsindex")?;

	let mut rebased = instance(&vf, &[("wght", triple(300.0, 400.0, 900.0))])?;

	assert_closed(&mut rebased, &loc(&[]))?;
	assert_close_to(
		&mut rebased,
		&loc(&[]),
		&mut vf,
		&loc(&[("wght", 400.0)]),
		0.5,
	)?;
	let location = loc(&[("wght", 900.0)]);
	assert_close_to(&mut rebased, &location, &mut vf, &location, 0.5)
}

#[test]
fn test_hinted_charstrings_keep_their_hints() -> TestResult {
	let mut vf = fixture("cff2_two_hinted")?;

	let mut rebased = instance(&vf, &[("wght", triple(300.0, 400.0, 900.0))])?;

	let program = get_program(&mut rebased)?;
	assert!(program.iter().any(|t| t == "hstemhm"));
	assert_eq!(program[position(&program, "hintmask") + 1], "10000000");
	assert_closed(&mut rebased, &loc(&[]))?;
	assert_close_to(
		&mut rebased,
		&loc(&[]),
		&mut vf,
		&loc(&[("wght", 400.0)]),
		0.5,
	)
}

#[test]
fn test_stem_edges_stay_on_the_outline() -> TestResult {
	// At wght 570 a relative stem would end a unit off the ring's top
	let vf = fixture("cff2_two_hinted")?;

	let mut instance = instance(&vf, &[("wght", pin(570.0))])?;

	let program = get_program(&mut instance)?;
	assert_eq!(program[2], "hstemhm");
	let bottom: f64 = program[0].parse()?;
	let height: f64 = program[1].parse()?;
	let ys: Vec<f64> = get_contours(&mut instance, &loc(&[]))?
		.iter()
		.flatten()
		.map(|p| p.1)
		.collect();
	let min = ys.iter().copied().fold(f64::INFINITY, f64::min);
	let max = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
	assert_eq!((bottom, bottom + height), (min, max));
	Ok(())
}

#[test]
fn test_ghost_hint() -> TestResult {
	// Ghost hint edges at .5 after rebasing; the width must stay -21
	let vf = fixture("cff2_ghost_hint")?;

	let mut rebased = instance(&vf, &[("wght", triple(300.0, 600.0, 900.0))])?;

	let program = get_program(&mut rebased)?;
	let end = position(&program, "hstemhm");
	assert_eq!(program[end - 1..=end], ["-21", "hstemhm"]);
	Ok(())
}

#[test]
fn test_rounding_keeps_distances() -> TestResult {
	// Edges 51 apart at .5: half-to-even would give 50 or 52
	let vf = fixture("cff2_rounding_keeps_distances")?;

	let mut rebased = instance(&vf, &[("wght", triple(300.0, 600.0, 900.0))])?;

	let contours = get_contours(&mut rebased, &loc(&[]))?;
	assert_eq!(contours[0].len(), 3);
	assert_eq!((contours[0][0].0, contours[0][1].0), (102.0, 153.0));
	Ok(())
}

#[test]
fn test_fractional_outline() -> TestResult {
	let mut vf = fixture("cff2_fractional_outline")?;
	let limits = normalized_limits(&[("wght", (0.0, 0.5, 1.0)), ("slnt", (0.0, 0.0, 0.0))]);
	let mut unrounded = vf.clone();

	// Points that did not move are rounded along with those that did.
	instantiate_cff2(&mut vf, &limits, true, None)?;
	let points = get_contours(&mut vf, &loc(&[]))?;
	assert_eq!(points[0][..2], [(102.0, 101.0), (153.0, 101.0)]);

	instantiate_cff2(&mut unrounded, &limits, false, None)?;
	let points = get_contours(&mut unrounded, &loc(&[]))?;
	assert_eq!(points[0][..2], [(101.75, 100.5), (152.75, 100.5)]);
	Ok(())
}

#[test]
fn test_flex() -> TestResult {
	let mut vf = fixture("cff2_two_flex")?;
	assert!(get_program(&mut vf)?.iter().any(|t| t == "flex"));

	let mut rebased = instance(&vf, &[("wght", triple(300.0, 400.0, 900.0))])?;

	assert!(get_program(&mut rebased)?.iter().any(|t| t == "flex"));
	assert_closed(&mut rebased, &loc(&[]))?;
	assert_close_to(
		&mut rebased,
		&loc(&[]),
		&mut vf,
		&loc(&[("wght", 400.0)]),
		0.5,
	)?;
	for weight in [300.0, 600.0, 900.0] {
		assert_closed(&mut rebased, &loc(&[("wght", weight)]))?;
	}
	Ok(())
}

#[test]
fn test_flex1_changing_direction() -> TestResult {
	// Direction varies across the remaining wght range: fallback
	let mut vf = fixture("cff2_flex1")?;

	let (instance, log) = caplog(|| instance(&vf, &[("wght", triple(300.0, 300.0, 900.0))]));
	let mut instance = instance?;

	assert!(log.contains("'o': flex1 changes direction"), "{log}");
	assert!(get_program(&mut instance)?.iter().any(|t| t == "flex1"));
	assert_close_to(&mut instance, &loc(&[]), &mut vf, &loc(&[]), 0.0)?;
	let location = loc(&[("wght", 900.0)]);
	assert_close_to(&mut instance, &location, &mut vf, &location, 0.0)
}

#[test]
fn test_flex1_direction_at_pinned_location() -> TestResult {
	// Pinned, the direction is known
	let mut vf = fixture("cff2_flex1")?;

	let (instance, log) = caplog(|| instance(&vf, &[("wght", pin(900.0))]));
	let mut instance = instance?;

	assert!(!log.contains("flex1 changes direction"), "{log}");
	assert_close_to(
		&mut instance,
		&loc(&[]),
		&mut vf,
		&loc(&[("wght", 900.0)]),
		0.0,
	)
}

#[test]
fn test_flex1_direction_with_invalid_tent() -> TestResult {
	// Invalid tent: direction unknown, fallback
	let vf = fixture("cff2_flex1_invalid_tent")?;

	let (instance, log) = caplog(|| instance(&vf, &[("wght", pin(900.0))]));
	let mut instance = instance?;

	assert!(log.contains("'o': flex1 changes direction"), "{log}");
	assert!(get_program(&mut instance)?.iter().any(|t| t == "flex1"));
	Ok(())
}

#[test]
fn test_vsindex_after_blend() -> TestResult {
	// vsindex after a blend is invalid CFF2: fallback, not an error
	let mut vf = fixture("cff2_vsindex_after_blend")?;

	let (rebased, log) = caplog(|| instance(&vf, &[("wght", triple(300.0, 400.0, 900.0))]));
	let mut rebased = rebased?;

	assert!(log.contains("'o': vsindex after a blend"), "{log}");
	assert_close_to(
		&mut rebased,
		&loc(&[]),
		&mut vf,
		&loc(&[("wght", 400.0)]),
		1.0,
	)
}

#[test]
fn test_fractional_private_dict_values() -> TestResult {
	let vf = fixture("cff2_fractional_private")?;

	let mut rebased = vf.clone();
	instantiate(&mut rebased, &[("wght", triple(300.0, 600.0, 900.0))])?;

	let top = &rebased.require::<Cff2>()?.cff.top_dicts[0];
	let private = private_of(top, all_privates(top)[0]).ok_or("no Private")?;
	let Some(Value::List(blue_scale)) = private.get("BlueScale") else {
		return Err("BlueScale isn't a blend".into());
	};
	let Value::Float(first) = blue_scale[0] else {
		return Err("BlueScale's default isn't a float".into());
	};
	// Both the default and its folded delta retain their fractional parts.
	assert!(approx(first, 0.049625), "{first}");
	Ok(())
}

#[test]
fn test_private_dict_vsindex_and_charstring_vsindex() -> TestResult {
	// Private DICT VarData 1 is emptied; the charstring's VarData 0 stays
	let mut vf = fixture("cff2_private_and_charstring_vsindex")?;

	let mut rebased = instance(
		&vf,
		&[("wght", triple(300.0, 400.0, 900.0)), ("slnt", pin(0.0))],
	)?;

	assert_closed(&mut rebased, &loc(&[]))?;
	assert_close_to(
		&mut rebased,
		&loc(&[]),
		&mut vf,
		&loc(&[("wght", 400.0)]),
		0.5,
	)
}
