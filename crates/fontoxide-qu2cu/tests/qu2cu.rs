//! Port of fontTools' `Tests/qu2cu/qu2cu_test.py`, and a differential check
//! against vectors made by `tools/reference/curve_vectors.py`.

use fontoxide_misc::py::float_repr;
use fontoxide_qu2cu::{Error, quadratic_to_curves};

type Point = (f64, f64);

fn approx_curves(got: &[Vec<Point>], expected: &[Vec<Point>]) {
	assert_eq!(got.len(), expected.len(), "{got:?} != {expected:?}");
	for (g, e) in got.iter().zip(expected) {
		assert_eq!(g.len(), e.len(), "{got:?} != {expected:?}");
		for (p, q) in g.iter().zip(e) {
			let close = |a: f64, b: f64| (a - b).abs() <= (1e-6 * b.abs()).max(1e-12);
			assert!(
				close(p.0, q.0) && close(p.1, q.1),
				"{got:?} != {expected:?}"
			);
		}
	}
}

#[test]
fn test_empty_input() -> Result<(), Error> {
	assert!(quadratic_to_curves(&[], 0.5, false)?.is_empty());
	Ok(())
}

#[test]
fn test_tuple_spline() -> Result<(), Error> {
	let spline = vec![(0.0, 0.0), (0.0, 1.0), (1.0, 1.0)];
	assert_eq!(
		quadratic_to_curves(std::slice::from_ref(&spline), 0.5, false)?,
		[spline]
	);
	Ok(())
}

#[test]
fn test_simple() -> Result<(), Error> {
	// The quadratics, the expected curves, the tolerance and cubic_only.
	type Case = (Vec<Vec<Point>>, Vec<Vec<Point>>, f64, bool);
	let cases: [Case; 4] = [
		(
			vec![vec![(0.0, 0.0), (0.0, 1.0), (2.0, 1.0), (2.0, 0.0)]],
			vec![vec![
				(0.0, 0.0),
				(0.0, 4.0 / 3.0),
				(2.0, 4.0 / 3.0),
				(2.0, 0.0),
			]],
			0.1,
			true,
		),
		(
			vec![vec![(0.0, 0.0), (0.0, 1.0), (2.0, 1.0), (2.0, 2.0)]],
			vec![vec![
				(0.0, 0.0),
				(0.0, 4.0 / 3.0),
				(2.0, 2.0 / 3.0),
				(2.0, 2.0),
			]],
			0.2,
			true,
		),
		(
			vec![
				vec![(0.0, 0.0), (0.0, 1.0), (1.0, 1.0)],
				vec![(1.0, 1.0), (3.0, 1.0), (3.0, 0.0)],
			],
			vec![
				vec![(0.0, 0.0), (0.0, 1.0), (1.0, 1.0)],
				vec![(1.0, 1.0), (3.0, 1.0), (3.0, 0.0)],
			],
			0.2,
			false,
		),
		(
			vec![
				vec![(0.0, 0.0), (0.0, 1.0), (1.0, 1.0)],
				vec![(1.0, 1.0), (3.0, 1.0), (3.0, 0.0)],
			],
			vec![
				vec![(0.0, 0.0), (0.0, 2.0 / 3.0), (1.0 / 3.0, 1.0), (1.0, 1.0)],
				vec![(1.0, 1.0), (7.0 / 3.0, 1.0), (3.0, 2.0 / 3.0), (3.0, 0.0)],
			],
			0.2,
			true,
		),
	];
	for (quadratics, expected, tolerance, cubic_only) in cases {
		approx_curves(
			&quadratic_to_curves(&quadratics, tolerance, cubic_only)?,
			&expected,
		);
	}
	Ok(())
}

#[test]
fn test_reject_disconnected_splines() {
	let e = quadratic_to_curves(
		&[
			vec![(0.0, 0.0), (0.0, 1.0), (1.0, 1.0)],
			vec![(2.0, 2.0), (3.0, 3.0), (4.0, 4.0)],
		],
		0.5,
		false,
	);
	assert!(matches!(e, Err(Error(m)) if m.contains("must connect end-to-start")));
}

#[test]
fn test_reject_short_spline() {
	let e = quadratic_to_curves(&[vec![(0.0, 0.0), (1.0, 1.0)]], 0.5, false);
	assert!(matches!(e, Err(Error(m)) if m.contains("at least 3 points")));
}

#[test]
fn test_reject_non_positive_tolerance() {
	for tolerance in [0.0, -1.0] {
		let e = quadratic_to_curves(
			&[vec![(0.0, 0.0), (0.0, 1.0), (1.0, 1.0)]],
			tolerance,
			false,
		);
		assert!(matches!(e, Err(Error(m)) if m.contains("max_err must be greater than zero")));
	}
}

fn parse_points(s: &str) -> Vec<Point> {
	s.split(' ')
		.filter(|p| !p.is_empty())
		.map(|p| {
			let (x, y) = p.split_once(',').unwrap_or_default();
			(x.parse().unwrap_or(f64::NAN), y.parse().unwrap_or(f64::NAN))
		})
		.collect()
}

fn format_points(points: &[Point]) -> String {
	let v: Vec<String> = points
		.iter()
		.map(|p| format!("{},{}", float_repr(p.0), float_repr(p.1)))
		.collect();
	v.join(" ")
}

/// Each vector against fontTools: bit for bit on the platform the vectors
/// were made on, approximately elsewhere (see `fontoxide_misc::float_vectors`;
/// the fitting amplifies the platform differences of `hypot`). The vectors include the
/// roundtrip of `test_roundtrip` (cu2qu, then qu2cu, at tolerance 1).
#[test]
fn differential_vectors() {
	let vectors =
		fontoxide_misc::float_vectors::Vectors::new(include_str!("data/qu2cu_vectors.txt"));
	let mut failures = Vec::new();
	for line in vectors.lines() {
		let fields: Vec<&str> = line.split('\t').collect();
		let [_, tol, flag, input, expected] = fields[..] else {
			failures.push(format!("malformed: {line}"));
			continue;
		};
		let tol: f64 = tol.parse().unwrap_or(f64::NAN);
		let splines: Vec<Vec<Point>> = input.split(" | ").map(parse_points).collect();
		let got = quadratic_to_curves(&splines, tol, flag == "c")
			.map(|r| {
				let v: Vec<String> = r.iter().map(|c| format_points(c)).collect();
				v.join(" | ")
			})
			.unwrap_or_else(|_| "!E".into());
		if !vectors.matches(expected, &got) {
			failures.push(format!("{line}\n  got {got}"));
		}
	}
	assert!(
		failures.is_empty(),
		"{} of {} vectors failed:\n{}",
		failures.len(),
		vectors.lines().count(),
		failures[..failures.len().min(5)].join("\n")
	);
}
