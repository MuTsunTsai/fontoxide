//! Port of `fontTools.misc.fixedTools`: conversions between fixed-point numbers,
//! floats and strings.
//!
//! The OpenType specification defines `Fixed` (16.16) and `F2DOT14` (2.14). As in
//! fontTools, fixed-point values are held as `f64` in table objects and converted
//! when compiling.

use crate::round_tools::{nearest_multiple_shortest_repr, ot_round};

/// The largest value that still fits in an F2Dot14 (`MAX_F2DOT14`).
pub const MAX_F2DOT14: f64 = 0x7FFF as f64 / (1 << 14) as f64;

fn scale(precision_bits: u32) -> f64 {
	(1u64 << precision_bits) as f64
}

/// Converts a fixed-point number to a float (`fixedToFloat`).
pub fn fixed_to_float(value: i64, precision_bits: u32) -> f64 {
	value as f64 / scale(precision_bits)
}

/// Converts a float to a fixed-point number (`floatToFixed`).
pub fn float_to_fixed(value: f64, precision_bits: u32) -> i64 {
	ot_round(value * scale(precision_bits))
}

/// Rounds a float to the nearest value representable in fixed point
/// (`floatToFixedToFloat`).
pub fn float_to_fixed_to_float(value: f64, precision_bits: u32) -> f64 {
	let scale = scale(precision_bits);
	ot_round(value * scale) as f64 / scale
}

/// Converts a fixed-point number to the shortest decimal string that identifies it
/// (`fixedToStr`).
pub fn fixed_to_str(value: i64, precision_bits: u32) -> String {
	let scale = scale(precision_bits);
	nearest_multiple_shortest_repr(value as f64 / scale, 1.0 / scale)
}

/// Converts a decimal string to a fixed-point number (`strToFixed`).
pub fn str_to_fixed(string: &str, precision_bits: u32) -> Option<i64> {
	let value = crate::py::parse_float(string)?;
	Some(ot_round(value * scale(precision_bits)))
}

/// Converts a decimal string to a float, rounded to fixed-point precision
/// (`strToFixedToFloat`).
pub fn str_to_fixed_to_float(string: &str, precision_bits: u32) -> Option<f64> {
	let value = crate::py::parse_float(string)?;
	let scale = scale(precision_bits);
	Some(ot_round(value * scale) as f64 / scale)
}

/// Converts a float to the shortest decimal string identifying the nearest
/// fixed-point value (`floatToFixedToStr`).
pub fn float_to_fixed_to_str(value: f64, precision_bits: u32) -> String {
	nearest_multiple_shortest_repr(value, 1.0 / scale(precision_bits))
}

/// Ensures a table version is a 32-bit fixed number (`ensureVersionIsLong`): values
/// below 0x10000 are taken as floats and converted, with a warning.
pub fn ensure_version_is_long(value: f64) -> i64 {
	if value < 65536.0 {
		let new_value = float_to_fixed(value, 16);
		log::warn!(
			"Table version value is a float: {value:.4}; fix to use hex instead: 0x{new_value:08x}"
		);
		new_value
	} else {
		value as i64
	}
}

/// Parses a table version from TTX (`versionToFixed`): strings starting with `0`
/// are integers (e.g. `0x00010000`), anything else is a float.
pub fn version_to_fixed(value: &str) -> Option<i64> {
	let v = if value.starts_with('0') {
		crate::text_tools::parse_int_literal(value)? as f64
	} else {
		crate::py::parse_float(value)?
	};
	Some(ensure_version_is_long(v))
}

#[cfg(test)]
mod tests {
	//! Port of Tests/misc/fixedTools_test.py.

	use super::*;

	fn assert_almost_eq(a: f64, b: f64) {
		// unittest's assertAlmostEqual: round(a - b, 7) == 0
		assert!((a - b).abs() < 5e-8, "{a} != {b}");
	}

	#[test]
	fn test_roundtrip() {
		for bits in 0..15 {
			for value in -(1i64 << (bits + 1))..(1i64 << (bits + 1)) {
				assert_eq!(value, float_to_fixed(fixed_to_float(value, bits), bits));
			}
		}
	}

	#[test]
	fn test_fixed_to_float_precision14() {
		assert_almost_eq(0.7999878, fixed_to_float(13107, 14));
		assert_eq!(0.0, fixed_to_float(0, 14));
		assert_eq!(1.0, fixed_to_float(16384, 14));
		assert_eq!(-1.0, fixed_to_float(-16384, 14));
		assert_almost_eq(0.999939, fixed_to_float(16383, 14));
		assert_almost_eq(-0.999939, fixed_to_float(-16383, 14));
	}

	#[test]
	fn test_fixed_to_float_precision6() {
		assert_almost_eq(-9.984375, fixed_to_float(-639, 6));
		assert_almost_eq(-10.0, fixed_to_float(-640, 6));
		assert_almost_eq(9.984375, fixed_to_float(639, 6));
		assert_almost_eq(10.0, fixed_to_float(640, 6));
	}

	#[test]
	fn test_fixed_to_str_precision14() {
		assert_eq!("0.8", fixed_to_str(13107, 14));
		assert_eq!("0.0", fixed_to_str(0, 14));
		assert_eq!("1.0", fixed_to_str(16384, 14));
		assert_eq!("-1.0", fixed_to_str(-16384, 14));
		assert_eq!("0.99994", fixed_to_str(16383, 14));
		assert_eq!("-0.99994", fixed_to_str(-16383, 14));
	}

	#[test]
	fn test_fixed_to_str_precision6() {
		assert_eq!("-9.98", fixed_to_str(-639, 6));
		assert_eq!("-10.0", fixed_to_str(-640, 6));
		assert_eq!("9.98", fixed_to_str(639, 6));
		assert_eq!("10.0", fixed_to_str(640, 6));
	}

	#[test]
	fn test_float_to_fixed_precision14() {
		assert_eq!(13107, float_to_fixed(0.8, 14));
		assert_eq!(16384, float_to_fixed(1.0, 14));
		assert_eq!(-16384, float_to_fixed(-1.0, 14));
		assert_eq!(0, float_to_fixed(0.0, 14));
	}

	#[test]
	fn test_str_to_fixed_precision14() {
		assert_eq!(Some(13107), str_to_fixed("0.8", 14));
		assert_eq!(Some(16384), str_to_fixed("1.0", 14));
		assert_eq!(Some(16384), str_to_fixed("1", 14));
		assert_eq!(Some(-16384), str_to_fixed("-1.0", 14));
		assert_eq!(Some(-16384), str_to_fixed("-1", 14));
		assert_eq!(Some(0), str_to_fixed("0", 14));
	}

	#[test]
	fn test_str_to_fixed_to_float_precision14() {
		assert_almost_eq(
			0.7999878,
			str_to_fixed_to_float("0.8", 14).unwrap_or_default(),
		);
		assert_eq!(Some(0.0), str_to_fixed_to_float("0", 14));
		assert_eq!(Some(1.0), str_to_fixed_to_float("1.0", 14));
		assert_eq!(Some(-1.0), str_to_fixed_to_float("-1.0", 14));
		assert_almost_eq(
			0.999939,
			str_to_fixed_to_float("0.99994", 14).unwrap_or_default(),
		);
		assert_almost_eq(
			-0.999939,
			str_to_fixed_to_float("-0.99994", 14).unwrap_or_default(),
		);
	}

	#[test]
	fn test_float_to_fixed_to_str_precision14() {
		assert_eq!("0.8", float_to_fixed_to_str(0.7999878, 14));
		assert_eq!("1.0", float_to_fixed_to_str(1.0, 14));
		assert_eq!("-1.0", float_to_fixed_to_str(-1.0, 14));
		assert_eq!("0.0", float_to_fixed_to_str(0.0, 14));
	}

	// Doctests in Lib/fontTools/misc/fixedTools.py.
	#[test]
	fn doctests() {
		assert_eq!(float_to_fixed(-0.61883544921875, 14), -10139);
		assert_eq!(float_to_fixed(-0.61884, 14), -10139);
		assert_eq!(fixed_to_str(-10139, 14), "-0.61884");
		assert_eq!(str_to_fixed("-0.61884", 14), Some(-10139));
		assert_eq!(float_to_fixed_to_str(-0.61883544921875, 14), "-0.61884");
		let f2 = float_to_fixed_to_float(-0.61884, 14);
		assert_ne!(f2, -0.61884);
		assert_almost_eq(f2, -0.61883544921875);
	}
}
