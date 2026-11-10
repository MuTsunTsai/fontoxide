//! Port of fontTools' `Tests/cu2qu/cu2qu_test.py`, and a differential check
//! against vectors made by `tools/reference/curve_vectors.py`.

use std::collections::BTreeMap;

use fontoxide_cu2qu::{Error, curve_to_quadratic, curves_to_quadratic};
use fontoxide_misc::py::float_repr;

type Point = (f64, f64);

const MAX_ERR: f64 = 5.0;

/// The curves of `Tests/cu2qu/data/curves.json`: every number in order,
/// eight per curve.
fn curves() -> Vec<[Point; 4]> {
	let path = concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/../../upstream/fonttools/Tests/cu2qu/data/curves.json"
	);
	let text = std::fs::read_to_string(path).unwrap_or_default();
	let numbers: Vec<f64> = text
		.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' || c == '+'))
		.filter(|s| !s.is_empty())
		.filter_map(|s| s.parse().ok())
		.collect();
	assert_eq!(numbers.len(), 8000, "curves.json not found or malformed");
	numbers
		.as_chunks::<8>()
		.0
		.iter()
		.map(|c| [(c[0], c[1]), (c[2], c[3]), (c[4], c[5]), (c[6], c[7])])
		.collect()
}

fn lerp((x1, y1): Point, (x2, y2): Point, t: f64) -> Point {
	(x1 + (x2 - x1) * t, y1 + (y2 - y1) * t)
}

fn dist((x1, y1): Point, (x2, y2): Point) -> f64 {
	(x1 - x2).hypot(y1 - y2)
}

fn quadratic_bezier_at(b: [Point; 3], t: f64) -> Point {
	let [(x1, y1), (x2, y2), (x3, y3)] = b;
	let _t = 1.0 - t;
	let t2 = t * t;
	let _t2 = _t * _t;
	let _2_t_t = 2.0 * t * _t;
	(
		_t2 * x1 + _2_t_t * x2 + t2 * x3,
		_t2 * y1 + _2_t_t * y2 + t2 * y3,
	)
}

fn cubic_bezier_at(b: &[Point; 4], t: f64) -> Point {
	let [(x1, y1), (x2, y2), (x3, y3), (x4, y4)] = *b;
	let _t = 1.0 - t;
	let t2 = t * t;
	let _t2 = _t * _t;
	let t3 = t * t2;
	let _t3 = _t * _t2;
	let _3_t2_t = 3.0 * t2 * _t;
	let _3_t_t2 = 3.0 * t * _t2;
	(
		_t3 * x1 + _3_t_t2 * x2 + _3_t2_t * x3 + t3 * x4,
		_t3 * y1 + _3_t_t2 * y2 + _3_t2_t * y3 + t3 * y4,
	)
}

/// `curve_spline_dist`: the largest distance between a Bézier and a
/// quadratic spline at sampled points.
fn curve_spline_dist(bezier: &[Point; 4], spline: &[Point]) -> f64 {
	let total_steps = 20;
	let mut error = 0.0f64;
	let n = spline.len() - 2;
	let steps = total_steps / n;
	let mut p3 = spline[0];
	for i in 0..n - 1 {
		let p1 = if i == 0 { spline[0] } else { p3 };
		let p2 = spline[i + 1];
		p3 = if i < n - 1 {
			lerp(spline[i + 1], spline[i + 2], 0.5)
		} else {
			spline[n + 2]
		};
		let segment = [p1, p2, p3];
		for j in 0..steps {
			let d = dist(
				cubic_bezier_at(bezier, (j as f64 / steps as f64 + i as f64) / n as f64),
				quadratic_bezier_at(segment, j as f64 / steps as f64),
			);
			if d > error {
				error = d;
			}
		}
	}
	error
}

#[test]
fn test_results_unchanged() -> Result<(), Error> {
	let mut results = BTreeMap::new();
	for c in curves() {
		let spline = curve_to_quadratic(&c, MAX_ERR, true)?;
		*results.entry(spline.len() - 2).or_insert(0) += 1;
	}
	let expected = BTreeMap::from([
		(2, 6),
		(3, 26),
		(4, 82),
		(5, 232),
		(6, 360),
		(7, 266),
		(8, 28),
	]);
	assert_eq!(results, expected);
	Ok(())
}

#[test]
fn test_results_unchanged_multiple() -> Result<(), Error> {
	let curves = curves();
	let mut results = BTreeMap::new();
	for group in curves[..300].chunks(3) {
		let splines = curves_to_quadratic(group, &[MAX_ERR; 3], true)?;
		let n = splines[0].len() - 2;
		for spline in &splines[1..] {
			assert_eq!(spline.len() - 2, n, "Got incompatible conversion results");
		}
		*results.entry(n).or_insert(0) += 1;
	}
	assert_eq!(results, BTreeMap::from([(5, 11), (6, 35), (7, 49), (8, 5)]));
	Ok(())
}

#[test]
fn test_does_not_exceed_tolerance() -> Result<(), Error> {
	for c in curves() {
		let spline = curve_to_quadratic(&c, MAX_ERR, true)?;
		assert!(curve_spline_dist(&c, &spline) <= MAX_ERR);
	}
	Ok(())
}

#[test]
fn test_does_not_exceed_tolerance_multiple() -> Result<(), Error> {
	let curves = curves();
	for group in curves[..300].chunks(3) {
		for (c, s) in group
			.iter()
			.zip(curves_to_quadratic(group, &[MAX_ERR; 3], true)?)
		{
			assert!(curve_spline_dist(c, &s) <= MAX_ERR);
		}
	}
	Ok(())
}

#[test]
fn test_all_quadratic_false() -> Result<(), Error> {
	// `AllQuadraticFalseTest.test_cubic`.
	let cubic = [(0.0, 0.0), (0.0, 1.0), (2.0, 1.0), (2.0, 0.0)];
	assert_eq!(curve_to_quadratic(&cubic, 0.1, false)?, cubic);
	// `AllQuadraticFalseTest.test_quadratic`.
	let cubic = [(0.0, 0.0), (2.0, 2.0), (4.0, 2.0), (6.0, 0.0)];
	assert_eq!(
		curve_to_quadratic(&cubic, 0.1, false)?,
		[(0.0, 0.0), (3.0, 3.0), (6.0, 0.0)]
	);
	Ok(())
}

#[test]
fn test_cu2qu_degenerate_all_points_equal() -> Result<(), Error> {
	// https://github.com/fonttools/fonttools/pull/3903
	let cubic = [(5.0, 5.0); 4];
	assert_eq!(curve_to_quadratic(&cubic, 0.1, true)?, [(5.0, 5.0); 3]);
	Ok(())
}

#[test]
fn test_cu2qu_degenerate_3_points_equal() -> Result<(), Error> {
	let cubic = [(5.0, 5.0), (5.0, 5.0), (5.0, 5.0), (5.0, 5.1)];
	// A single quadratic is within the tolerance.
	assert_eq!(
		curve_to_quadratic(&cubic, 0.1, true)?,
		[(5.0, 5.0), (5.0, 5.0), (5.0, 5.1)]
	);
	// Two off-curve points are needed for the smaller tolerance.
	assert_eq!(
		curve_to_quadratic(&cubic, 0.01, true)?,
		[(5.0, 5.0), (5.0, 5.0), (5.0, 5.025), (5.0, 5.1)]
	);
	Ok(())
}

#[test]
fn test_cu2qu_degenerate_curve_with_collinear_points() -> Result<(), Error> {
	// The control points are collinear, and the off-curve points overlap
	// halfway between the on-curve points ('brevecomb_gravecomb.case' of
	// BilboPro.glyphs).
	let cubic = [
		(64.94, 550.998),
		(65.199, 550.032),
		(65.199, 550.032),
		(65.458, 549.066),
	];
	let result = curve_to_quadratic(&cubic, 1.0, true)?;
	let expected = [
		(64.94, 550.998),
		(65.13425, 550.2735),
		(65.26375, 549.7905),
		(65.458, 549.066),
	];
	assert_eq!(result.len(), expected.len());
	for (p1, p2) in result.iter().zip(expected) {
		assert!(
			(p1.0 - p2.0).abs() <= 1e-6 * p2.0.abs() && (p1.1 - p2.1).abs() <= 1e-6 * p2.1.abs()
		);
	}
	Ok(())
}

#[test]
fn test_cu2qu_complex_division_floating_point_precision() -> Result<(), Error> {
	// https://github.com/fonttools/fonttools/issues/3928
	let curves = [
		[(87.0, 738.0), (90.0, 438.0), (90.0, 437.0), (90.0, 277.0)],
		[(64.0, 738.0), (63.0, 586.0), (64.0, 442.0), (64.0, 277.0)],
	];
	let result = curves_to_quadratic(&curves, &[1.0; 2], true)?;
	assert_eq!(result[1][5].1, 326.49999999999994);
	Ok(())
}

#[test]
fn test_cu2qu_split_cubic_into_three_complex_division() -> Result<(), Error> {
	// https://github.com/fonttools/fonttools/issues/3928
	let curves = [
		[
			(217.833, 408.833),
			(299.667, 403.833),
			(372.333, 463.833),
			(382.5, 545.167),
		],
		[
			(217.0, 413.0),
			(303.0, 408.0),
			(374.0, 468.0),
			(385.0, 551.0),
		],
		[
			(213.0, 426.0),
			(313.0, 420.0),
			(381.0, 482.0),
			(392.0, 571.0),
		],
	];
	let result = curves_to_quadratic(&curves, &[1.0; 3], true)?;
	assert_eq!(result[0][3].1, 504.5000000000001);
	Ok(())
}

#[test]
fn test_curves_to_quadratic_rejects_mismatched_max_errors() {
	let curves = [[(0.0, 0.0), (0.0, 1.0), (2.0, 1.0), (2.0, 0.0)]];
	let e = curves_to_quadratic(&curves, &[], true);
	assert!(matches!(e, Err(Error::Value(m)) if m.contains("max_errors must match")));
}

#[test]
fn test_curves_to_quadratic_empty_input() -> Result<(), Error> {
	assert!(curves_to_quadratic(&[], &[], true)?.is_empty());
	Ok(())
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

fn curve4(points: &[Point]) -> [Point; 4] {
	[points[0], points[1], points[2], points[3]]
}

/// Each vector against fontTools, bit for bit.
#[test]
fn differential_vectors() {
	let vectors = include_str!("data/cu2qu_vectors.txt");
	let mut failures = Vec::new();
	for line in vectors.lines() {
		let fields: Vec<&str> = line.split('\t').collect();
		let [kind, tols, flag, input, expected] = fields[..] else {
			failures.push(format!("malformed: {line}"));
			continue;
		};
		let all_quadratic = flag == "a";
		let tols: Vec<f64> = tols.split(',').filter_map(|t| t.parse().ok()).collect();
		let curves: Vec<[Point; 4]> = input
			.split(" | ")
			.map(|c| curve4(&parse_points(c)))
			.collect();
		let got = match kind {
			"S" => {
				curve_to_quadratic(&curves[0], tols[0], all_quadratic).map(|s| format_points(&s))
			}
			_ => curves_to_quadratic(&curves, &tols, all_quadratic).map(|r| {
				let v: Vec<String> = r.iter().map(|s| format_points(s)).collect();
				v.join(" | ")
			}),
		}
		.unwrap_or_else(|_| "!E".into());
		if got != expected {
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
