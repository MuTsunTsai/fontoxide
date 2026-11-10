//! Port of fontTools' `Tests/misc/arrayTools_test.py`.

use fontoxide_misc::array_tools::*;
use fontoxide_misc::round_tools::ot_round;

#[test]
fn test_calc_bounds() {
	assert_eq!(calc_bounds(&[]), [0.0; 4]);
	assert_eq!(
		calc_bounds(&[(0.0, 40.0), (0.0, 100.0), (50.0, 50.0), (80.0, 10.0)]),
		[0.0, 10.0, 80.0, 100.0]
	);
}

#[test]
fn test_calc_int_bounds() {
	let points = [(0.1, 40.1), (0.1, 100.1), (49.9, 49.9), (78.5, 9.5)];
	assert_eq!(calc_int_bounds(&points, ot_round), [0, 10, 79, 100]);
	assert_eq!(
		calc_int_bounds(&points, |v| v.round_ties_even() as i64),
		[0, 10, 78, 100]
	);
}

#[test]
fn test_update_bounds() {
	assert_eq!(
		update_bounds([0.0; 4], (100.0, 100.0)),
		[0.0, 0.0, 100.0, 100.0]
	);
}

#[test]
fn test_point_in_rect() {
	let r = [0.0, 0.0, 100.0, 100.0];
	assert!(point_in_rect((50.0, 50.0), r));
	assert!(point_in_rect((0.0, 0.0), r));
	assert!(point_in_rect((100.0, 100.0), r));
	assert!(!point_in_rect((101.0, 100.0), r));
}

#[test]
fn test_points_in_rect() {
	let r = [0.0, 0.0, 100.0, 100.0];
	assert!(points_in_rect(&[], r).is_empty());
	assert_eq!(
		points_in_rect(
			&[(50.0, 50.0), (0.0, 0.0), (100.0, 100.0), (101.0, 100.0)],
			r
		),
		[true, true, true, false]
	);
}

#[test]
fn test_vector_length() {
	assert_eq!(vector_length((1.0, 1.0)), 2f64.sqrt());
}

#[test]
fn test_as_int16() {
	assert_eq!(as_int16(&[0.0, 0.1, 0.5, 0.9]), [0, 0, 1, 1]);
}

#[test]
fn test_norm_rect() {
	assert_eq!(
		norm_rect([0.0, 10.0, 100.0, 200.0]),
		[0.0, 10.0, 100.0, 200.0]
	);
	assert_eq!(
		norm_rect([100.0, 200.0, 0.0, 10.0]),
		[0.0, 10.0, 100.0, 200.0]
	);
}

#[test]
fn test_scale_offset_inset_rect() {
	assert_eq!(
		scale_rect([10.0, 20.0, 50.0, 150.0], 1.5, 2.0),
		[15.0, 40.0, 75.0, 300.0]
	);
	assert_eq!(
		offset_rect([10.0, 20.0, 30.0, 40.0], 5.0, 6.0),
		[15.0, 26.0, 35.0, 46.0]
	);
	assert_eq!(
		inset_rect([10.0, 20.0, 50.0, 60.0], 5.0, 10.0),
		[15.0, 30.0, 45.0, 50.0]
	);
	assert_eq!(
		inset_rect([10.0, 20.0, 50.0, 60.0], -5.0, -10.0),
		[5.0, 10.0, 55.0, 70.0]
	);
}

#[test]
fn test_sect_rect() {
	assert_eq!(
		sect_rect([0.0, 10.0, 20.0, 30.0], [0.0, 40.0, 20.0, 50.0]),
		None
	);
	assert_eq!(
		sect_rect([0.0, 10.0, 20.0, 30.0], [5.0, 20.0, 35.0, 50.0]),
		Some([5.0, 20.0, 20.0, 30.0])
	);
}

#[test]
fn test_union_rect() {
	assert_eq!(
		union_rect([0.0, 10.0, 20.0, 30.0], [0.0, 40.0, 20.0, 50.0]),
		[0.0, 10.0, 20.0, 50.0]
	);
}

#[test]
fn test_rect_center() {
	assert_eq!(rect_center([0.0, 0.0, 100.0, 200.0]), (50.0, 100.0));
	assert_eq!(rect_center([0.0, 0.0, 100.0, 199.0]), (50.0, 99.5));
}

#[test]
fn test_int_rect() {
	assert_eq!(int_rect([0.9, 2.9, 3.1, 4.1]), [0, 2, 4, 5]);
}
