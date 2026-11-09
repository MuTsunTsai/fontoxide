//! Port of fontTools' `Tests/varLib/instancer/solver_test.py`.

#![allow(clippy::type_complexity)]

use fontoxide_varlib::instancer::{NormalizedAxisTriple, Tent, rebase_tent};

type Expected = &'static [(f64, Option<Tent>)];

fn triple(a: f64, b: f64, c: f64) -> NormalizedAxisTriple {
	NormalizedAxisTriple::new(a, b, c, 1.0, 1.0).expect("sorted")
}

/// `pytest.approx`.
fn approx(a: f64, b: f64) -> bool {
	(a - b).abs() <= (1e-6 * b.abs()).max(1e-12)
}

fn cases() -> Vec<(Tent, NormalizedAxisTriple, Expected)> {
	let mut cases = Vec::new();
	let mut case = |tent: Tent, range: NormalizedAxisTriple, expected: Expected| {
		cases.push((tent, range, expected));
	};
	// Case 1: Pin at default
	case((0.0, 1.0, 1.0), triple(0.0, 0.0, 0.0), &[]);
	// Case 1:
	case((0.3, 0.5, 0.8), triple(0.1, 0.2, 0.3), &[]);
	// Pin axis
	case((0.0, 1.0, 1.0), triple(0.5, 0.5, 0.5), &[(0.5, None)]);
	// Case 2:
	case(
		(0.0, 1.0, 1.0),
		triple(-1.0, 0.0, 0.5),
		&[(0.5, Some((0.0, 1.0, 1.0)))],
	);
	// Case 2:
	case(
		(0.0, 1.0, 1.0),
		triple(-1.0, 0.0, 0.75),
		&[(0.75, Some((0.0, 1.0, 1.0)))],
	);
	//
	// Without gain:
	//
	// Case 3
	case(
		(0.0, 0.2, 1.0),
		triple(-1.0, 0.0, 0.8),
		&[
			(1.0, Some((0.0, 0.25, 1.0))),
			(0.25, Some((0.25, 1.0, 1.0))),
		],
	);
	// Case 3 boundary
	case(
		(0.0, 0.4, 1.0),
		triple(-1.0, 0.0, 0.5),
		&[
			(1.0, Some((0.0, 0.8, 1.0))),
			(2.5 / 3.0, Some((0.8, 1.0, 1.0))),
		],
	);
	// Case 4
	case(
		(0.0, 0.25, 1.0),
		triple(-1.0, 0.0, 0.4),
		&[
			(1.0, Some((0.0, 0.625, 1.0))),
			(0.8, Some((0.625, 1.0, 1.0))),
		],
	);
	case(
		(0.25, 0.3, 1.05),
		triple(0.0, 0.2, 0.4),
		&[
			(1.0, Some((0.25, 0.5, 1.0))),
			(2.6 / 3.0, Some((0.5, 1.0, 1.0))),
		],
	);
	// Case 4 boundary
	case(
		(0.25, 0.5, 1.0),
		triple(0.0, 0.25, 0.5),
		&[(1.0, Some((0.0, 1.0, 1.0)))],
	);
	//
	// With gain:
	//
	// Case 3a/1neg
	case(
		(0.0, 0.5, 1.0),
		triple(0.0, 0.5, 1.0),
		&[
			(1.0, None),
			(-1.0, Some((0.0, 1.0, 1.0))),
			(-1.0, Some((-1.0, -1.0, 0.0))),
		],
	);
	case(
		(0.0, 0.5, 1.0),
		triple(0.0, 0.5, 0.75),
		&[
			(1.0, None),
			(-0.5, Some((0.0, 1.0, 1.0))),
			(-1.0, Some((-1.0, -1.0, 0.0))),
		],
	);
	case(
		(0.0, 0.5, 1.0),
		triple(0.0, 0.25, 0.8),
		&[
			(0.5, None),
			(0.5, Some((0.0, 0.45454545, 0.9090909090))),
			(-0.1, Some((0.9090909090, 1.0, 1.0))),
			(-0.5, Some((-1.0, -1.0, 0.0))),
		],
	);
	// Case 3a/1neg
	case(
		(0.0, 0.5, 2.0),
		triple(0.2, 0.5, 0.8),
		&[
			(1.0, None),
			(-0.2, Some((0.0, 1.0, 1.0))),
			(-0.6, Some((-1.0, -1.0, 0.0))),
		],
	);
	// Case 3a/1neg
	case(
		(0.0, 0.5, 2.0),
		triple(0.2, 0.5, 1.0),
		&[
			(1.0, None),
			(-1.0 / 3.0, Some((0.0, 1.0, 1.0))),
			(-0.6, Some((-1.0, -1.0, 0.0))),
		],
	);
	// Case 3
	case(
		(0.0, 0.5, 1.0),
		triple(0.25, 0.25, 0.75),
		&[(0.5, None), (0.5, Some((0.0, 0.5, 1.0)))],
	);
	// Case 1neg
	case(
		(0.0, 0.5, 1.0),
		triple(0.0, 0.25, 0.5),
		&[
			(0.5, None),
			(0.5, Some((0.0, 1.0, 1.0))),
			(-0.5, Some((-1.0, -1.0, 0.0))),
		],
	);
	// Case 2neg
	case(
		(0.05, 0.55, 1.0),
		triple(0.0, 0.25, 0.5),
		&[
			(0.4, None),
			(0.5, Some((0.0, 1.0, 1.0))),
			(-0.4, Some((-1.0, -0.8, 0.0))),
			(-0.4, Some((-1.0, -1.0, -0.8))),
		],
	);
	// Case 2neg, other side
	case(
		(-1.0, -0.55, -0.05),
		triple(-0.5, -0.25, 0.0),
		&[
			(0.4, None),
			(0.5, Some((-1.0, -1.0, 0.0))),
			(-0.4, Some((0.0, 0.8, 1.0))),
			(-0.4, Some((0.8, 1.0, 1.0))),
		],
	);
	//
	// Misc corner cases
	//
	case((0.5, 0.5, 0.5), triple(0.5, 0.5, 0.5), &[(1.0, None)]);
	case(
		(0.3, 0.5, 0.7),
		triple(0.1, 0.5, 0.9),
		&[
			(1.0, None),
			(-1.0, Some((0.0, 0.5, 1.0))),
			(-1.0, Some((0.5, 1.0, 1.0))),
			(-1.0, Some((-1.0, -0.5, 0.0))),
			(-1.0, Some((-1.0, -1.0, -0.5))),
		],
	);
	case(
		(0.5, 0.5, 0.5),
		triple(0.25, 0.25, 0.5),
		&[(1.0, Some((1.0, 1.0, 1.0)))],
	);
	case(
		(0.5, 0.5, 0.5),
		triple(0.25, 0.35, 0.5),
		&[(1.0, Some((1.0, 1.0, 1.0)))],
	);
	case(
		(0.5, 0.5, 0.55),
		triple(0.25, 0.35, 0.5),
		&[(1.0, Some((1.0, 1.0, 1.0)))],
	);
	case(
		(0.5, 0.5, 1.0),
		triple(0.5, 0.5, 1.0),
		&[(1.0, None), (-1.0, Some((0.0, 1.0, 1.0)))],
	);
	case(
		(0.25, 0.5, 1.0),
		triple(0.5, 0.5, 1.0),
		&[(1.0, None), (-1.0, Some((0.0, 1.0, 1.0)))],
	);
	case(
		(0.0, 0.2, 1.0),
		triple(0.0, 0.0, 0.5),
		&[(1.0, Some((0.0, 0.4, 1.0))), (0.625, Some((0.4, 1.0, 1.0)))],
	);
	// https://github.com/fonttools/fonttools/issues/3139
	case(
		(0.0, 0.5, 1.0),
		triple(-1.0, 0.25, 1.0),
		&[
			(0.5, None),
			(0.5, Some((0.0, 1.0 / 3.0, 2.0 / 3.0))),
			(-0.5, Some((2.0 / 3.0, 1.0, 1.0))),
			(-0.5, Some((-1.0, -0.2, 0.0))),
			(-0.5, Some((-1.0, -1.0, -0.2))),
		],
	);
	// Dirac delta at new default. Fancy!
	case(
		(0.5, 0.5, 0.5),
		triple(0.0, 0.5, 1.0),
		&[
			(1.0, None),
			(-1.0, Some((0.0, 0.0001220703, 1.0))),
			(-1.0, Some((0.0001220703, 1.0, 1.0))),
			(-1.0, Some((-1.0, -0.0001220703, 0.0))),
			(-1.0, Some((-1.0, -1.0, -0.0001220703))),
		],
	);
	// https://github.com/fonttools/fonttools/issues/3177
	case(
		(0.0, 1.0, 1.0),
		NormalizedAxisTriple::new(-1.0, -0.5, 1.0, 1.0, 1.0).expect("sorted"),
		&[(1.0, Some((1.0 / 3.0, 1.0, 1.0)))],
	);
	case(
		(0.0, 1.0, 1.0),
		NormalizedAxisTriple::new(-1.0, -0.5, 1.0, 2.0, 1.0).expect("sorted"),
		&[(1.0, Some((0.5, 1.0, 1.0)))],
	);
	// https://github.com/fonttools/fonttools/issues/3291
	case(
		(0.6, 0.7, 0.8),
		NormalizedAxisTriple::new(-1.0, 0.2, 1.0, 1.0, 1.0).expect("sorted"),
		&[(1.0, Some((0.5, 0.625, 0.75)))],
	);
	cases
}

/// `RebaseTentTest.test_rebaseTent`.
#[test]
fn test_rebase_tent() {
	for (tent, axis_range, expected) in cases() {
		let sol = rebase_tent(tent, axis_range).expect("a valid range");
		let matches = sol.len() == expected.len()
			&& sol.iter().zip(expected).all(|((s, v), (es, ev))| {
				approx(*s, *es)
					&& match (v, ev) {
						(None, None) => true,
						(Some(v), Some(e)) => {
							approx(v.0, e.0) && approx(v.1, e.1) && approx(v.2, e.2)
						}
						_ => false,
					}
			});
		assert!(
			matches,
			"{tent:?}, {axis_range:?}: {sol:?}, expected {expected:?}"
		);
	}
}
