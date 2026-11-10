//! Port of fontTools' `Tests/misc/bezierTools_test.py`.

use fontoxide_misc::array_tools::Point;
use fontoxide_misc::bezier_tools::*;
use fontoxide_misc::complex::Complex;

/// `pytest.approx`: a relative tolerance of 1e-6 and an absolute one of 1e-12.
fn approx(a: f64, e: f64) -> bool {
	(a - e).abs() <= (1e-6 * e.abs()).max(1e-12)
}

fn assert_curves_approx_equal<const N: usize>(actual: &[[Point; N]], expected: &[[Point; N]]) {
	assert_eq!(actual.len(), expected.len());
	for (a, e) in actual.iter().zip(expected) {
		for (ap, ep) in a.iter().zip(e) {
			assert!(approx(ap.0, ep.0) && approx(ap.1, ep.1), "{ap:?} != {ep:?}");
		}
	}
}

#[test]
fn test_calc_quadratic_bounds() {
	assert_eq!(
		calc_quadratic_bounds((0.0, 0.0), (50.0, 100.0), (100.0, 0.0)),
		[0.0, 0.0, 100.0, 50.0]
	);
	assert_eq!(
		calc_quadratic_bounds((0.0, 0.0), (100.0, 0.0), (100.0, 100.0)),
		[0.0, 0.0, 100.0, 100.0]
	);
}

#[test]
fn test_calc_cubic_bounds() {
	assert_eq!(
		calc_cubic_bounds((0.0, 0.0), (25.0, 100.0), (75.0, 100.0), (100.0, 0.0)),
		[0.0, 0.0, 100.0, 75.0]
	);
	assert_eq!(
		calc_cubic_bounds((0.0, 0.0), (50.0, 0.0), (100.0, 50.0), (100.0, 100.0)),
		[0.0, 0.0, 100.0, 100.0]
	);
	let b = calc_cubic_bounds((50.0, 0.0), (0.0, 100.0), (100.0, 100.0), (50.0, 0.0));
	for (a, e) in b.iter().zip([35.566243, 0.0, 64.433757, 75.0]) {
		assert!((a - e).abs() < 1e-6);
	}
}

#[test]
fn test_split_line() {
	let o = (0.0, 0.0);
	let p = (100.0, 100.0);
	assert_eq!(
		split_line(o, p, 50.0, true),
		[[o, (50.0, 50.0)], [(50.0, 50.0), p]]
	);
	assert_eq!(split_line(o, p, 100.0, true), [[o, p]]);
	assert_eq!(split_line(o, p, 0.0, true), [[o, o], [o, p]]);
	assert_eq!(split_line(o, p, 0.0, false), [[o, o], [o, p]]);
	assert_eq!(
		split_line((100.0, 0.0), o, 50.0, false),
		[[(100.0, 0.0), (50.0, 0.0)], [(50.0, 0.0), o]]
	);
	assert_eq!(
		split_line((0.0, 100.0), o, 50.0, true),
		[[(0.0, 100.0), (0.0, 50.0)], [(0.0, 50.0), o]]
	);
	assert_eq!(
		split_line((0.0, 100.0), (100.0, 100.0), 50.0, true),
		[[(0.0, 100.0), (100.0, 100.0)]]
	);
}

#[test]
fn test_split_quadratic() {
	let (a, b, c) = ((0.0, 0.0), (50.0, 100.0), (100.0, 0.0));
	assert_eq!(split_quadratic(a, b, c, 150.0, false), [[a, b, c]]);
	assert_eq!(
		split_quadratic(a, b, c, 50.0, false),
		[
			[a, (25.0, 50.0), (50.0, 50.0)],
			[(50.0, 50.0), (75.0, 50.0), c]
		]
	);
	assert_eq!(
		split_quadratic(a, b, c, 25.0, false),
		[
			[a, (12.5, 25.0), (25.0, 37.5)],
			[(25.0, 37.5), (62.5, 75.0), c]
		]
	);
	assert_curves_approx_equal(
		&split_quadratic(a, b, c, 25.0, true),
		&[
			[a, (7.32233, 14.64466), (14.64466, 25.0)],
			[(14.64466, 25.0), (50.0, 75.0), (85.3553, 25.0)],
			[(85.3553, 25.0), (92.6777, 14.64466), (100.0, -7.10543e-15)],
		],
	);
	assert_eq!(
		split_quadratic(a, b, c, 50.0, true),
		[
			[a, (25.0, 50.0), (50.0, 50.0)],
			[(50.0, 50.0), (50.0, 50.0), (50.0, 50.0)],
			[(50.0, 50.0), (75.0, 50.0), c],
		]
	);
}

#[test]
fn test_split_cubic() {
	let (a, b, c, d) = ((0.0, 0.0), (25.0, 100.0), (75.0, 100.0), (100.0, 0.0));
	assert_eq!(split_cubic(a, b, c, d, 150.0, false), [[a, b, c, d]]);
	assert_eq!(
		split_cubic(a, b, c, d, 50.0, false),
		[
			[a, (12.5, 50.0), (31.25, 75.0), (50.0, 75.0)],
			[(50.0, 75.0), (68.75, 75.0), (87.5, 50.0), d],
		]
	);
	assert_curves_approx_equal(
		&split_cubic(a, b, c, d, 25.0, true),
		&[
			[a, (2.293792, 9.17517), (4.798045, 17.5085), (7.47414, 25.0)],
			[
				(7.47414, 25.0),
				(31.2886, 91.6667),
				(68.7114, 91.6667),
				(92.5259, 25.0),
			],
			[
				(92.5259, 25.0),
				(95.202, 17.5085),
				(97.7062, 9.17517),
				(100.0, 1.77636e-15),
			],
		],
	);
}

#[test]
fn test_split_quadratic_at_t() {
	let (a, b, c) = ((0.0, 0.0), (50.0, 100.0), (100.0, 0.0));
	assert_eq!(
		split_quadratic_at_t(a, b, c, &[0.5]),
		[
			[a, (25.0, 50.0), (50.0, 50.0)],
			[(50.0, 50.0), (75.0, 50.0), c]
		]
	);
	assert_eq!(
		split_quadratic_at_t(a, b, c, &[0.5, 0.75]),
		[
			[a, (25.0, 50.0), (50.0, 50.0)],
			[(50.0, 50.0), (62.5, 50.0), (75.0, 37.5)],
			[(75.0, 37.5), (87.5, 25.0), c],
		]
	);
}

#[test]
fn test_split_cubic_at_t() {
	let (a, b, c, d) = ((0.0, 0.0), (25.0, 100.0), (75.0, 100.0), (100.0, 0.0));
	assert_eq!(
		split_cubic_at_t(a, b, c, d, &[0.5]),
		[
			[a, (12.5, 50.0), (31.25, 75.0), (50.0, 75.0)],
			[(50.0, 75.0), (68.75, 75.0), (87.5, 50.0), d],
		]
	);
	assert_eq!(
		split_cubic_at_t(a, b, c, d, &[0.5, 0.75]),
		[
			[a, (12.5, 50.0), (31.25, 75.0), (50.0, 75.0)],
			[
				(50.0, 75.0),
				(59.375, 75.0),
				(68.75, 68.75),
				(77.34375, 56.25)
			],
			[(77.34375, 56.25), (85.9375, 43.75), (93.75, 25.0), d],
		]
	);
}

const CUBIC: [Point; 4] = [
	(-103.0, -231.0),
	(-61.0, -240.0),
	(-31.009, -245.0),
	(6.0, -245.0),
];
const QUADRATIC: [Point; 3] = [(69.996, 92.927), (286.522, 231.449), (-143.86, -158.184)];

#[test]
fn test_split_cubic_at_t_robustness() {
	let [a, b, c, d] = CUBIC;
	let split = split_cubic_at_t(a, b, c, d, &[0.386637]);
	assert_eq!(split[1][3], d);
}

#[test]
fn test_split_cubic_at_t_c_robustness() {
	// Without the fixup the tail ends at (5.999999999999993-245j).
	let [a, b, c, d] = CUBIC.map(Complex::from_point);
	let split = split_cubic_at_t_c(a, b, c, d, &[0.386637]);
	assert_eq!(split[0][0], a);
	assert_eq!(split[1][3], d);
}

#[test]
fn test_split_quadratic_at_t_robustness() {
	let [a, b, c] = QUADRATIC;
	let split = split_quadratic_at_t(a, b, c, &[0.14299]);
	assert_eq!(split[0][0], a);
	assert_eq!(split[1][2], c);
}

#[test]
fn test_split_quadratic_robustness() {
	let [a, b, c] = QUADRATIC;
	let split = split_quadratic(a, b, c, -120.0, false);
	assert_eq!(split.len(), 2);
	assert_eq!(split[0][0], a);
	assert_eq!(split[1][2], c);
}

#[test]
fn test_split_cubic_robustness() {
	let [a, b, c, d] = CUBIC;
	let split = split_cubic(a, b, c, d, -98.8, false);
	assert_eq!(split.len(), 2);
	assert_eq!(split[0][0], a);
	assert_eq!(split[1][3], d);
}

#[test]
fn test_solve_cubic() {
	assert_eq!(solve_cubic(1.0, 1.0, -6.0, 0.0), [-3.0, -0.0, 2.0]);
	assert_eq!(solve_cubic(-10.0, -9.0, 48.0, -29.0), [-2.9, 1.0, 1.0]);
	assert_eq!(
		solve_cubic(-9.875, -9.0, 47.625, -28.75),
		[-2.911392, 1.0, 1.0]
	);
	assert_eq!(solve_cubic(1.0, -4.5, 6.75, -3.375), [1.5, 1.5, 1.5]);
	assert_eq!(
		solve_cubic(-12.0, 18.0, -9.0, 1.50023651123),
		[0.5, 0.5, 0.5]
	);
	assert_eq!(
		solve_cubic(9.0, 0.0, 0.0, -7.62939453125e-05),
		[-0.0, -0.0, -0.0]
	);
}

#[test]
fn test_segment_point_at_t() {
	let line: &[Point] = &[(0.0, 10.0), (200.0, 100.0)];
	let quad: &[Point] = &[(0.0, 10.0), (100.0, 100.0), (200.0, 50.0)];
	let cubic: &[Point] = &[(0.0, 10.0), (100.0, 100.0), (200.0, 100.0), (300.0, 0.0)];
	let cases: [(&[Point], f64, Point); 9] = [
		(line, 0.0, (0.0, 10.0)),
		(line, 0.5, (100.0, 55.0)),
		(line, 1.0, (200.0, 100.0)),
		(quad, 0.0, (0.0, 10.0)),
		(quad, 0.5, (100.0, 65.0)),
		(quad, 1.0, (200.0, 50.0)),
		(cubic, 0.0, (0.0, 10.0)),
		(cubic, 0.5, (150.0, 76.25)),
		(cubic, 1.0, (300.0, 0.0)),
	];
	for (segment, t, expected) in cases {
		assert_eq!(segment_point_at_t(segment, t), Ok(expected));
	}
}

#[test]
fn test_intersections_straight_line() -> Result<(), ValueError> {
	let curve = [
		(548.0, 183.0),
		(548.0, 289.0),
		(450.0, 366.0),
		(315.0, 366.0),
	];
	let line1 = [(330.0, 376.0), (330.0, 286.0)];
	let pt = curve_line_intersections(&curve, &line1)?[0].pt;
	assert_eq!(pt.0, 330.0);
	let pt2 = (330.0001018806911, 295.5635754579425);
	assert!(line_t_of_pt(pt, (330.0, 286.0), pt2) > 0.0);
	let s = (19.0, 0.0);
	let e = (110.0, 0.0);
	let pt = (109.05194805194802, 0.0);
	assert!(approx(line_t_of_pt(s, e, pt), 0.98958184));
	Ok(())
}

#[test]
fn test_collinear_vertical_lines_do_not_intersect() {
	assert!(
		line_line_intersections((310.0, 0.0), (310.0, 10.0), (310.0, 800.0), (310.0, 810.0))
			.is_empty()
	);
}

#[test]
fn test_calc_quadratic_arc_length() {
	// https://github.com/fonttools/fonttools/issues/3287
	assert!(approx(
		calc_quadratic_arc_length((210.0, 333.0), (289.0, 333.0), (326.5, 290.5)),
		127.9225
	));
}

#[test]
fn test_calc_cubic_arc_length() {
	let cases: [([Point; 4], f64); 3] = [
		// https://github.com/fonttools/fonttools/issues/3502
		(
			[
				(377.0, 469.0),
				(377.0, 468.0),
				(377.0, 472.0),
				(377.0, 472.0),
			],
			3.32098765445,
		),
		(
			[
				(242.0, 402.0),
				(242.0, 403.0),
				(242.0, 399.0),
				(242.0, 399.0),
			],
			3.32098765445,
		),
		// https://github.com/fonttools/fonttools/issues/3514
		(
			[
				(626.9918761593156, 1000.0),
				(639.133178223544, 1000.0),
				(650.1152019577394, 1000.0),
				(626.9918761593156, 1000.0),
			],
			27.06159516422008,
		),
	];
	for ([a, b, c, d], expected) in cases {
		assert!(approx(calc_cubic_arc_length(a, b, c, d, 0.005), expected));
	}
}

#[test]
fn test_intersections_linelike() -> Result<(), ValueError> {
	let seg1 = [(0.0, 0.0), (0.0, 0.25), (0.0, 0.75), (0.0, 1.0)];
	let seg2 = [(0.0, 0.5), (0.25, 0.5), (0.75, 0.5), (1.0, 0.5)];
	let pt = curve_curve_intersections(&seg1, &seg2)?[0].pt;
	assert_eq!(pt, (0.0, 0.5));
	Ok(())
}

// See https://github.com/fonttools/fonttools/issues/3962.
#[test]
fn test_linelike_curve_curve_regression() -> Result<(), ValueError> {
	let seg1 = [(330.0, -1.0), (337.0, -1.0), (344.0, -1.0), (351.0, -1.0)];
	let seg2 = [
		(342.0, -13.0),
		(341.0, 57.0),
		(381.0, 146.0),
		(472.0, 151.0),
	];
	assert!(is_linelike(&seg1));
	assert!(!is_linelike(&seg2));
	let hits = curve_curve_intersections(&seg1, &seg2)?;
	assert_eq!(hits.len(), 1);
	let hit = hits[0];
	let pt1 = segment_point_at_t(&seg1, hit.t1)?;
	let pt2 = segment_point_at_t(&seg2, hit.t2)?;
	let dist = (pt1.0 - pt2.0).hypot(pt1.1 - pt2.1);
	// Before the fix, the distance was about 100.
	assert!(dist.abs() < 0.001);
	Ok(())
}
