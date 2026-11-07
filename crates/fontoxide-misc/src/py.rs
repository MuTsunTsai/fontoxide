//! Python formatting semantics that fontTools' output depends on.
//!
//! TTX files contain values formatted with `str()` and `repr()` of Python objects.
//! To produce byte-identical output, those conversions are reproduced here.

use std::fmt::Write as _;

/// `repr(float)` (equivalently `str(float)`) in Python 3: the shortest string that
/// round-trips, with Python's choice between positional and exponent notation.
///
/// ```
/// use fontoxide_misc::py::float_repr;
/// assert_eq!(float_repr(1.0), "1.0");
/// assert_eq!(float_repr(0.1), "0.1");
/// assert_eq!(float_repr(1e16), "1e+16");
/// assert_eq!(float_repr(1e-5), "1e-05");
/// assert_eq!(float_repr(-0.0), "-0.0");
/// ```
pub fn float_repr(value: f64) -> String {
	if value.is_nan() {
		return "nan".into();
	}
	if value.is_infinite() {
		return if value > 0.0 { "inf" } else { "-inf" }.into();
	}
	if value == 0.0 {
		return if value.is_sign_negative() {
			"-0.0"
		} else {
			"0.0"
		}
		.into();
	}

	// Rust's `{:e}` gives the shortest round-tripping digits, e.g. "-1.2345e-7".
	let sci = format!("{:e}", value.abs());
	let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
	let exponent: i32 = exponent.parse().unwrap_or(0);
	let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
	// Position of the decimal point relative to the digits: value = 0.DIGITS * 10^decpt.
	let decpt = exponent + 1;

	let mut out = String::new();
	if value < 0.0 {
		out.push('-');
	}
	if decpt <= -4 || decpt > 16 {
		out.push_str(&digits[..1]);
		if digits.len() > 1 {
			out.push('.');
			out.push_str(&digits[1..]);
		}
		let _ = write!(
			out,
			"e{}{:02}",
			if exponent < 0 { '-' } else { '+' },
			exponent.abs()
		);
	} else if decpt <= 0 {
		out.push_str("0.");
		out.extend(std::iter::repeat_n('0', (-decpt) as usize));
		out.push_str(&digits);
	} else {
		let decpt = decpt as usize;
		if digits.len() <= decpt {
			out.push_str(&digits);
			out.extend(std::iter::repeat_n('0', decpt - digits.len()));
			out.push_str(".0");
		} else {
			out.push_str(&digits[..decpt]);
			out.push('.');
			out.push_str(&digits[decpt..]);
		}
	}
	out
}

/// Python's `round(x)` for floats: round half to even.
pub fn round(value: f64) -> f64 {
	value.round_ties_even()
}

/// Whether `str.isprintable()` holds for a single character.
///
/// Exact for Latin-1 (which covers everything fontTools passes through `repr()` in
/// TTX output: tags and sfnt versions are decoded as Latin-1). Above U+00FF only the
/// common non-printable classes are recognized.
fn is_printable(c: char) -> bool {
	let n = c as u32;
	match n {
		0x00..=0x1F | 0x7F..=0xA0 | 0xAD => false,
		0x20..=0x7E | 0xA1..=0xFF => true,
		// Separators other than U+0020, format characters and private use.
		0x2000..=0x200F | 0x2028..=0x202F | 0x205F..=0x206F | 0x3000 | 0xFEFF => false,
		0x1680 | 0x180E | 0xFFF9..=0xFFFB => false,
		0xE000..=0xF8FF | 0xF0000.. => false,
		_ => true,
	}
}

/// `repr(str)` in Python 3, including the surrounding quotes.
///
/// ```
/// use fontoxide_misc::py::str_repr;
/// assert_eq!(str_repr("OTTO"), "'OTTO'");
/// assert_eq!(str_repr("\0\x01\0\0"), r"'\x00\x01\x00\x00'");
/// assert_eq!(str_repr("it's"), "\"it's\"");
/// ```
pub fn str_repr(s: &str) -> String {
	let quote = if s.contains('\'') && !s.contains('"') {
		'"'
	} else {
		'\''
	};
	let mut out = String::with_capacity(s.len() + 2);
	out.push(quote);
	for c in s.chars() {
		match c {
			'\\' => out.push_str("\\\\"),
			'\t' => out.push_str("\\t"),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			c if c == quote => {
				out.push('\\');
				out.push(c);
			}
			c if is_printable(c) => out.push(c),
			c => {
				let n = c as u32;
				let _ = if n < 0x100 {
					write!(out, "\\x{n:02x}")
				} else if n < 0x10000 {
					write!(out, "\\u{n:04x}")
				} else {
					write!(out, "\\U{n:08x}")
				};
			}
		}
	}
	out.push(quote);
	out
}

/// `repr(bytes)` in Python 3, including the `b` prefix and quotes.
///
/// ```
/// use fontoxide_misc::py::bytes_repr;
/// assert_eq!(bytes_repr(b"ab\x00\xff"), r"b'ab\x00\xff'");
/// ```
pub fn bytes_repr(data: &[u8]) -> String {
	let quote = if data.contains(&b'\'') && !data.contains(&b'"') {
		'"'
	} else {
		'\''
	};
	let mut out = String::with_capacity(data.len() + 3);
	out.push('b');
	out.push(quote);
	for &b in data {
		match b {
			b'\\' => out.push_str("\\\\"),
			b'\t' => out.push_str("\\t"),
			b'\n' => out.push_str("\\n"),
			b'\r' => out.push_str("\\r"),
			b if b as char == quote => {
				out.push('\\');
				out.push(b as char);
			}
			0x20..=0x7E => out.push(b as char),
			_ => {
				let _ = write!(out, "\\x{b:02x}");
			}
		}
	}
	out.push(quote);
	out
}

/// `str.isspace()` for a single character.
pub fn is_space(c: char) -> bool {
	matches!(
		c,
		'\t' | '\n'
			| '\x0b' | '\x0c'
			| '\r' | '\x1c'..='\x1f'
			| ' ' | '\u{85}'
			| '\u{a0}' | '\u{1680}'
			| '\u{2000}'..='\u{200a}'
			| '\u{2028}' | '\u{2029}'
			| '\u{202f}' | '\u{205f}'
			| '\u{3000}'
	)
}

/// `str.strip()`: removes leading and trailing whitespace as Python defines it
/// (which differs from [`str::trim`], e.g. for U+001C–U+001F).
pub fn strip(s: &str) -> &str {
	s.trim_matches(is_space)
}

/// Decodes bytes as Latin-1, as `bytes.decode("latin-1")` does.
pub fn latin1_decode(data: &[u8]) -> String {
	data.iter().map(|&b| b as char).collect()
}

/// Encodes a string as Latin-1, as `str.encode("latin-1")` does. Returns `None` if
/// a character is outside Latin-1.
pub fn latin1_encode(s: &str) -> Option<Vec<u8>> {
	s.chars().map(|c| u8::try_from(c as u32).ok()).collect()
}

/// Python's `float(str)`: accepts surrounding whitespace, underscores between
/// digits, `inf`/`nan` spellings, and everything Rust's parser accepts.
pub fn parse_float(s: &str) -> Option<f64> {
	let t = s.trim();
	if t.is_empty() {
		return None;
	}
	let cleaned;
	let t = if t.contains('_') {
		cleaned = remove_digit_underscores(t)?;
		cleaned.as_str()
	} else {
		t
	};
	let lower = t.to_ascii_lowercase();
	let unsigned = lower.trim_start_matches(['+', '-']);
	if matches!(unsigned, "inf" | "infinity" | "nan") {
		let negative = lower.starts_with('-');
		let v = if unsigned == "nan" {
			f64::NAN
		} else {
			f64::INFINITY
		};
		return Some(if negative { -v } else { v });
	}
	t.parse().ok()
}

/// Python's `int(str)` (base 10): accepts surrounding whitespace, a sign and
/// underscores between digits.
pub fn parse_int(s: &str) -> Option<i128> {
	let t = s.trim();
	let cleaned;
	let t = if t.contains('_') {
		cleaned = remove_digit_underscores(t)?;
		cleaned.as_str()
	} else {
		t
	};
	t.parse().ok()
}

fn remove_digit_underscores(s: &str) -> Option<String> {
	let bytes = s.as_bytes();
	for (i, &b) in bytes.iter().enumerate() {
		if b == b'_' {
			let before = i.checked_sub(1).map(|j| bytes[j]);
			let after = bytes.get(i + 1);
			if !before.is_some_and(|c| c.is_ascii_digit())
				|| !after.is_some_and(|c| c.is_ascii_digit())
			{
				return None;
			}
		}
	}
	Some(s.replace('_', ""))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn float_repr_matches_python() {
		let cases = [
			(1.0, "1.0"),
			(-1.5, "-1.5"),
			(0.5, "0.5"),
			(100.0, "100.0"),
			(1234567890123456.0, "1234567890123456.0"),
			(1e16, "1e+16"),
			(1.5e16, "1.5e+16"),
			(0.0001, "0.0001"),
			(0.00012, "0.00012"),
			(0.00001, "1e-05"),
			(1.25e-7, "1.25e-07"),
			(1e100, "1e+100"),
			(0.1 + 0.2, "0.30000000000000004"),
			(0.61884, "0.61884"),
			(f64::MAX, "1.7976931348623157e+308"),
			(5e-324, "5e-324"),
			(f64::INFINITY, "inf"),
			(f64::NEG_INFINITY, "-inf"),
		];
		for (value, expected) in cases {
			assert_eq!(float_repr(value), expected, "{value:e}");
		}
		assert_eq!(float_repr(f64::NAN), "nan");
	}

	#[test]
	fn str_repr_matches_python() {
		assert_eq!(str_repr("a\"b"), "'a\"b'");
		assert_eq!(str_repr("a'b\"c"), r#"'a\'b"c'"#);
		assert_eq!(str_repr("\u{7f}\u{a0}\u{a1}\u{ad}é"), r"'\x7f\xa0¡\xadé'");
		assert_eq!(str_repr("tab\there"), r"'tab\there'");
	}

	#[test]
	fn parse_numbers() {
		assert_eq!(parse_float(" 1.5 "), Some(1.5));
		assert_eq!(parse_float("1_000.5"), Some(1000.5));
		assert_eq!(parse_float("1__0"), None);
		assert_eq!(parse_float("-Infinity"), Some(f64::NEG_INFINITY));
		assert_eq!(parse_int("-12"), Some(-12));
		assert_eq!(parse_int("+1_2"), Some(12));
		assert_eq!(parse_int("1.0"), None);
	}
}
