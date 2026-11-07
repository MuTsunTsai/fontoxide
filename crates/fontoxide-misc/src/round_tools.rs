//! Port of `fontTools.misc.roundTools`: round-to-integer helpers.

/// Rounds to the nearest integer, with halves rounded towards +∞, as the OpenType
/// specification requires (`otRound`).
///
/// ```
/// use fontoxide_misc::round_tools::ot_round;
/// assert_eq!(ot_round(0.5), 1);
/// assert_eq!(ot_round(-0.5), 0);
/// assert_eq!(ot_round(-1.5), -1);
/// ```
pub fn ot_round(value: f64) -> i64 {
	(value + 0.5).floor() as i64
}

/// Rounds only if rounding changes the value by no more than `tolerance`
/// (`maybeRound`).
pub fn maybe_round(value: f64, tolerance: f64, round: impl Fn(f64) -> f64) -> f64 {
	let rounded = round(value);
	if (rounded - value).abs() <= tolerance {
		rounded
	} else {
		value
	}
}

/// `otRound` as a float-to-float function, for use with [`maybe_round`] and
/// [`round_func`].
pub fn ot_round_f64(value: f64) -> f64 {
	(value + 0.5).floor()
}

/// A rounding strategy (`roundFunc`): no rounding if `tolerance` is 0, always
/// `ot_round` if it is at least 0.5, otherwise [`maybe_round`].
///
/// Returns `None` for a negative tolerance, where fontTools raises `ValueError`.
pub fn round_func(tolerance: f64) -> Option<Box<dyn Fn(f64) -> f64>> {
	if tolerance < 0.0 {
		None
	} else if tolerance == 0.0 {
		Some(Box::new(|v| v))
	} else if tolerance >= 0.5 {
		Some(Box::new(ot_round_f64))
	} else {
		Some(Box::new(move |v| maybe_round(v, tolerance, ot_round_f64)))
	}
}

/// Rounds `value` to the nearest multiple of `factor` and returns the shortest
/// decimal representation that still identifies that multiple
/// (`nearestMultipleShortestRepr`).
///
/// ```
/// use fontoxide_misc::round_tools::nearest_multiple_shortest_repr;
/// assert_eq!(nearest_multiple_shortest_repr(-0.61883544921875, 1.0 / (1 << 14) as f64), "-0.61884");
/// ```
pub fn nearest_multiple_shortest_repr(value: f64, factor: f64) -> String {
	if value == 0.0 {
		return "0.0".into();
	}
	let value = ot_round(value / factor) as f64 * factor;
	let eps = 0.5 * factor;
	let lo = value - eps;
	let hi = value + eps;
	// If the range of valid choices spans an integer, return the integer.
	if lo.trunc() != hi.trunc() {
		return crate::py::float_repr(crate::py::round(value));
	}
	let lo = format!("{lo:.8}");
	let hi = format!("{hi:.8}");
	debug_assert!(lo.len() == hi.len() && lo != hi);
	let i = lo
		.bytes()
		.zip(hi.bytes())
		.position(|(a, b)| a != b)
		.unwrap_or(lo.len());
	let period = lo.find('.').unwrap_or(lo.len());
	debug_assert!(period < i);
	let precision = i.saturating_sub(period);
	format!("{value:.precision$}")
}

#[cfg(test)]
mod tests {
	use super::*;

	// Tests/misc/roundTools_test.py does not exist upstream; these mirror the
	// doctests in Lib/fontTools/misc/roundTools.py.
	#[test]
	fn nearest_multiple() {
		assert_eq!(
			nearest_multiple_shortest_repr(-0.61883544921875, 1.0 / 16384.0),
			"-0.61884"
		);
		assert_eq!(nearest_multiple_shortest_repr(1.0, 1.0 / 65536.0), "1.0");
		assert_eq!(nearest_multiple_shortest_repr(-2.0, 1.0 / 65536.0), "-2.0");
	}

	#[test]
	fn rounding() {
		assert_eq!(ot_round(2.5), 3);
		assert_eq!(ot_round(-2.5), -2);
		assert_eq!(ot_round(2.4999), 2);
		let f = round_func(0.1).expect("valid tolerance");
		assert_eq!(f(1.05), 1.0);
		assert_eq!(f(1.2), 1.2);
		assert!(round_func(-1.0).is_none());
	}
}
