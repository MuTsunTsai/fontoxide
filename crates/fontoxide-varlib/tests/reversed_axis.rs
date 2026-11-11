//! `Tests/varLib/reversed_axis_test.py`: the tests that need no master fonts.

mod common;

use common::TestResult;
use fontoxide_designspacelib::{AxisDescriptor, Number};
use fontoxide_ttlib::tables::Avar;
use fontoxide_ttlib::{Font, Tag};
use fontoxide_varlib::build::{Axes, add_avar};
use fontoxide_varlib::errors::VarLibError;
use fontoxide_varlib::models::normalize_value;

fn int_map(map: &[(i64, i64)]) -> Vec<(Number, Number)> {
	map.iter()
		.map(|(a, b)| (Number::Int(*a), Number::Int(*b)))
		.collect()
}

/// `make_axis`.
fn make_axis() -> AxisDescriptor {
	AxisDescriptor {
		name: Some("Optical size".into()),
		tag: Some("opsz".into()),
		minimum: Number::Int(0),
		default: Number::Int(50),
		maximum: Number::Int(100),
		map: int_map(&[(0, 100), (25, 80), (50, 40), (75, 10), (100, 0)]),
		..Default::default()
	}
}

fn axes(axis: AxisDescriptor) -> Axes {
	let mut axes = Axes::new();
	axes.insert("Optical size".into(), axis);
	axes
}

#[test]
fn test_reversed_axis_avar_is_increasing_and_retains_plateau() -> TestResult {
	let mut axis = make_axis();
	axis.map.insert(2, (Number::Int(35), Number::Int(80)));
	let original_map = axis.map.clone();
	let mut font = Font::new();
	let axes = axes(axis);
	assert!(add_avar(&mut font, &axes, &[], &["opsz".to_owned()])?);
	let mut segment = font.require::<Avar>()?.segments[&Tag::new(b"opsz")].clone();
	segment.sort_by(|a, b| a.0.total_cmp(&b.0));
	let expected = [
		(-1.0, -1.0),
		(-0.5, -2.0 / 3.0),
		(-0.3, -2.0 / 3.0),
		(0.0, 0.0),
		(0.5, 0.75),
		(1.0, 1.0),
	];
	assert_eq!(segment.len(), expected.len());
	for ((k, v), (ek, ev)) in segment.iter().zip(expected) {
		assert!(
			(k - ek).abs() < 1e-12 && (v - ev).abs() < 1e-12,
			"{segment:?}"
		);
	}
	assert_eq!(axes["Optical size"].map, original_map);
	Ok(())
}

#[test]
fn test_nonmonotonic_axis_map_is_rejected() {
	for mapping in [
		[(0, 100), (25, 20), (50, 40), (100, 0)],
		[(0, 0), (25, 80), (50, 40), (100, 100)],
	] {
		let mut axis = make_axis();
		axis.map = int_map(&mapping);
		let err = add_avar(&mut Font::new(), &axes(axis), &[], &["opsz".to_owned()])
			.expect_err("not monotonic");
		assert!(matches!(err, VarLibError::Validation(_)), "{err:?}");
	}
}

#[test]
fn test_generic_normalize_value_still_rejects_reversed_triple() {
	assert!(normalize_value(80.0, (100.0, 40.0, 0.0), false).is_err());
}
