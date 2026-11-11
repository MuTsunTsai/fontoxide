//! `Tests/misc/py23_test.py`: the Python 3 `round` (`round3`) and
//! `math.isclose` (`isclose`) that the port relies on, as
//! [`fontoxide_misc::py::round`], [`fontoxide_misc::py::round_digits`] and
//! [`fontoxide_misc::py::is_close_tol`].

use fontoxide_misc::py::{is_close_tol, round, round_digits};

/// `assertAlmostEqual`: equal when rounded to 7 decimal places.
fn assert_almost_equal(a: f64, b: f64) {
	assert!(
		a == b || round_digits(a - b, 7) == 0.0,
		"{a} != {b} within 7 places"
	);
}

mod round3_test {
	use super::*;

	#[test]
	fn test_second_argument_type() {
		// Floats as the second argument are ruled out by the types; None is
		// `round(x)`.
		assert_eq!(round(1.0), 1.0);
	}

	#[test]
	fn test_halfway_cases() {
		for (x, n, expected) in [
			(0.125, 2, 0.12),
			(0.375, 2, 0.38),
			(0.625, 2, 0.62),
			(0.875, 2, 0.88),
			(-0.125, 2, -0.12),
			(-0.375, 2, -0.38),
			(-0.625, 2, -0.62),
			(-0.875, 2, -0.88),
			(0.25, 1, 0.2),
			(0.75, 1, 0.8),
			(-0.25, 1, -0.2),
			(-0.75, 1, -0.8),
		] {
			assert_almost_equal(round_digits(x, n), expected);
		}

		for (x, expected) in [
			(-6.5, -6.0),
			(-5.5, -6.0),
			(-1.5, -2.0),
			(-0.5, 0.0),
			(0.5, 0.0),
			(1.5, 2.0),
			(2.5, 2.0),
			(3.5, 4.0),
			(4.5, 4.0),
			(5.5, 6.0),
			(6.5, 6.0),
		] {
			assert_eq!(round_digits(x, 0), expected);
			// same but without an explicit second argument
			assert_eq!(round(x), expected);
		}

		// no ndigits and input is already an integer: output == input
		assert_eq!(round(1.0), 1.0);

		for (x, expected) in [
			(-25.0, -20.0),
			(-15.0, -20.0),
			(-5.0, 0.0),
			(5.0, 0.0),
			(15.0, 20.0),
			(25.0, 20.0),
			(35.0, 40.0),
			(45.0, 40.0),
			(55.0, 60.0),
			(65.0, 60.0),
			(75.0, 80.0),
			(85.0, 80.0),
			(95.0, 100.0),
			(12325.0, 12320.0),
			(0.0, 0.0),
		] {
			assert_eq!(round_digits(x, -1), expected);
		}

		assert_eq!(round_digits(350.0, -2), 400.0);
		assert_eq!(round_digits(450.0, -2), 400.0);

		for (x, n, expected) in [
			(0.5e21, -21, 0.0),
			(1.5e21, -21, 2e21),
			(2.5e21, -21, 2e21),
			(5.5e21, -21, 6e21),
			(8.5e21, -21, 8e21),
			(-1.5e22, -22, -2e22),
			(-0.5e22, -22, 0.0),
			(0.5e22, -22, 0.0),
			(1.5e22, -22, 2e22),
		] {
			assert_almost_equal(round_digits(x, n), expected);
		}
	}
}

mod is_close_tests {
	use super::*;

	const NAN: f64 = f64::NAN;
	const INF: f64 = f64::INFINITY;
	const NINF: f64 = f64::NEG_INFINITY;

	fn all_close(examples: &[(f64, f64)], rel_tol: f64, abs_tol: f64) {
		for &(a, b) in examples {
			assert!(
				is_close_tol(a, b, rel_tol, abs_tol),
				"{a} and {b} should be close!"
			);
		}
	}

	fn all_not_close(examples: &[(f64, f64)], rel_tol: f64, abs_tol: f64) {
		for &(a, b) in examples {
			assert!(
				!is_close_tol(a, b, rel_tol, abs_tol),
				"{a} and {b} should not be close!"
			);
		}
	}

	#[test]
	fn test_identical() {
		// identical values must test as close
		all_close(
			&[
				(2.0, 2.0),
				(0.1e200, 0.1e200),
				(1.123e-300, 1.123e-300),
				(12345.0, 12345.0),
				(0.0, -0.0),
				(345678.0, 345678.0),
			],
			0.0,
			0.0,
		);
	}

	#[test]
	fn test_eight_decimal_places() {
		// examples that are close to 1e-8, but not 1e-9
		let examples = [
			(1e8, 1e8 + 1.0),
			(-1e-8, -1.000000009e-8),
			(1.12345678, 1.12345679),
		];
		all_close(&examples, 1e-8, 0.0);
		all_not_close(&examples, 1e-9, 0.0);
	}

	#[test]
	fn test_near_zero() {
		// values close to zero
		let examples = [(1e-9, 0.0), (-1e-9, 0.0), (-1e-150, 0.0)];
		// these should not be close to any rel_tol
		all_not_close(&examples, 0.9, 0.0);
		// these should be close to abs_tol=1e-8
		all_close(&examples, 1e-9, 1e-8);
	}

	#[test]
	fn test_identical_infinite() {
		// these are close regardless of tolerance -- i.e. they are equal
		all_close(&[(INF, INF), (NINF, NINF)], 1e-9, 0.0);
	}

	#[test]
	fn test_inf_ninf_nan() {
		// these should never be close (following IEEE 754 rules for equality)
		all_not_close(
			&[
				(NAN, NAN),
				(NAN, 1e-100),
				(1e-100, NAN),
				(INF, NAN),
				(NAN, INF),
				(INF, NINF),
				(INF, 1.0),
				(1.0, INF),
				(INF, 1e308),
				(1e308, INF),
			],
			1e-9,
			0.999999999999999,
		);
	}

	#[test]
	fn test_zero_tolerance() {
		// test with zero tolerance
		all_close(&[(1.0, 1.0), (-3.4, -3.4), (-1e-300, -1e-300)], 0.0, 0.0);
		all_not_close(
			&[
				(1.0, 1.000000000000001),
				(0.99999999999999, 1.0),
				(1.0e200, 0.999999999999999e200),
			],
			0.0,
			0.0,
		);
	}

	#[test]
	fn test_assymetry() {
		// test the assymetry example from PEP 485
		all_close(&[(9.0, 10.0), (10.0, 9.0)], 0.1, 0.0);
	}

	#[test]
	fn test_integers() {
		// test with integer values
		let examples = [(100000001.0, 100000000.0), (123456789.0, 123456788.0)];
		all_close(&examples, 1e-8, 0.0);
		all_not_close(&examples, 1e-9, 0.0);
	}
}
