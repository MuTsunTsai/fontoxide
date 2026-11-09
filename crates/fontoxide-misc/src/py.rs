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

/// `"%.{precision}G" % value`: `precision` significant digits, in positional
/// notation unless the exponent is below -4 or not below `precision`, without
/// trailing zeros.
///
/// ```
/// use fontoxide_misc::py::format_g;
/// assert_eq!(format_g(0.001, 8), "0.001");
/// assert_eq!(format_g(123456789.0, 8), "1.2345679E+08");
/// assert_eq!(format_g(1e-5, 8), "1E-05");
/// assert_eq!(format_g(-2.5, 8), "-2.5");
/// ```
pub fn format_g(value: f64, precision: usize) -> String {
	if value.is_nan() {
		return "NAN".into();
	}
	if value.is_infinite() {
		return if value > 0.0 { "INF" } else { "-INF" }.into();
	}
	let p = precision.max(1);
	let e = format!("{:.*e}", p - 1, value);
	let (mantissa, exponent) = e.split_once('e').unwrap_or((&e, "0"));
	let exponent: i64 = exponent.parse().unwrap_or(0);
	let strip = |s: &str| -> String {
		if s.contains('.') {
			s.trim_end_matches('0').trim_end_matches('.').to_owned()
		} else {
			s.to_owned()
		}
	};
	if -4 <= exponent && exponent < p as i64 {
		strip(&format!("{:.*}", (p as i64 - 1 - exponent) as usize, value))
	} else {
		let sign = if exponent < 0 { '-' } else { '+' };
		format!("{}E{sign}{:02}", strip(mantissa), exponent.abs())
	}
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

/// `hash(tuple)`, for a tuple of integers whose hash is the value itself: small
/// integers other than -1.
pub fn tuple_hash(items: &[i64]) -> i64 {
	const XXPRIME_1: u64 = 11400714785074694791;
	const XXPRIME_2: u64 = 14029467366897019727;
	const XXPRIME_5: u64 = 2870177450012600261;
	let mut acc = XXPRIME_5;
	for &item in items {
		acc = acc.wrapping_add((item as u64).wrapping_mul(XXPRIME_2));
		acc = acc.rotate_left(31);
		acc = acc.wrapping_mul(XXPRIME_1);
	}
	acc = acc.wrapping_add(items.len() as u64 ^ (XXPRIME_5 ^ 3527539));
	if acc == u64::MAX {
		return 1546275796;
	}
	acc as i64
}

/// The iteration order of a Python `set` to which the keys were added in the given
/// order, each with its hash, without removals. Equal keys must have equal hashes.
pub fn set_order<K: Copy + Eq>(keys: impl IntoIterator<Item = (K, i64)>) -> Vec<K> {
	const LINEAR_PROBES: usize = 9;
	const PERTURB_SHIFT: u32 = 5;
	/// `set_insert_clean`.
	fn insert_clean<K: Copy>(table: &mut [Option<(K, i64)>], key: K, hash: i64) {
		let mask = table.len() - 1;
		let mut perturb = hash as u64;
		let mut i = hash as u64 as usize & mask;
		loop {
			if table[i].is_none() {
				table[i] = Some((key, hash));
				return;
			}
			if i + LINEAR_PROBES <= mask {
				for j in 1..=LINEAR_PROBES {
					if table[i + j].is_none() {
						table[i + j] = Some((key, hash));
						return;
					}
				}
			}
			perturb >>= PERTURB_SHIFT;
			i = i
				.wrapping_mul(5)
				.wrapping_add(1)
				.wrapping_add(perturb as usize)
				& mask;
		}
	}
	let mut table: Vec<Option<(K, i64)>> = vec![None; 8];
	let mut used = 0;
	'keys: for (key, hash) in keys {
		let mask = table.len() - 1;
		let mut perturb = hash as u64;
		let mut i = hash as u64 as usize & mask;
		// `set_add_entry`.
		let slot = 'probe: loop {
			let probes = if i + LINEAR_PROBES <= mask {
				LINEAR_PROBES
			} else {
				0
			};
			for j in 0..=probes {
				match &table[i + j] {
					None => break 'probe i + j,
					Some((k, h)) if *h == hash && *k == key => continue 'keys,
					Some(_) => {}
				}
			}
			perturb >>= PERTURB_SHIFT;
			i = i
				.wrapping_mul(5)
				.wrapping_add(1)
				.wrapping_add(perturb as usize)
				& mask;
		};
		table[slot] = Some((key, hash));
		used += 1;
		if used * 5 >= mask * 3 {
			// `set_table_resize`.
			let min_used = if used > 50000 { used * 2 } else { used * 4 };
			let mut size = 8;
			while size <= min_used {
				size <<= 1;
			}
			let mut new_table = vec![None; size];
			for (k, h) in table.iter().flatten() {
				insert_clean(&mut new_table, *k, *h);
			}
			table = new_table;
		}
	}
	table.into_iter().flatten().map(|e| e.0).collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The order of `set` in CPython 3.13 (`tools/reference`).
	#[test]
	fn set_order_of_int_tuples() {
		type Case<'a> = (&'a [(i64, i64, i64)], &'a [(i64, i64, i64)]);
		let cases: &[Case<'_>] = &[
			(&[(3, 1, 1033), (1, 0, 0)], &[(3, 1, 1033), (1, 0, 0)]),
			(
				&[(1, 0, 0), (3, 1, 1033), (0, 3, 0), (3, 1, 1041)],
				&[(1, 0, 0), (0, 3, 0), (3, 1, 1041), (3, 1, 1033)],
			),
			(
				&[
					(3, 1, 1033),
					(3, 1, 1041),
					(3, 1, 1042),
					(3, 1, 2052),
					(3, 1, 1028),
					(1, 0, 0),
					(1, 1, 11),
					(0, 4, 0),
					(3, 10, 1033),
				],
				&[
					(3, 1, 1033),
					(3, 1, 1042),
					(3, 1, 1041),
					(0, 4, 0),
					(1, 0, 0),
					(3, 1, 1028),
					(1, 1, 11),
					(3, 1, 2052),
					(3, 10, 1033),
				],
			),
			(
				&[
					(0, 9, 258),
					(1, 1, 2029),
					(1, 7, 1554),
					(0, 1, 1998),
					(0, 6, 1772),
					(3, 0, 1824),
				],
				&[
					(1, 7, 1554),
					(3, 0, 1824),
					(0, 6, 1772),
					(0, 1, 1998),
					(1, 1, 2029),
					(0, 9, 258),
				],
			),
			(
				&[
					(1, 11, 937),
					(3, 1, 1300),
					(0, 0, 104),
					(3, 8, 37),
					(1, 10, 887),
					(1, 11, 118),
					(3, 3, 1793),
					(1, 8, 954),
					(1, 3, 896),
					(1, 4, 88),
					(1, 8, 409),
					(0, 10, 1214),
					(0, 11, 1362),
					(3, 11, 2051),
				],
				&[
					(1, 4, 88),
					(0, 11, 1362),
					(3, 1, 1300),
					(3, 11, 2051),
					(0, 10, 1214),
					(1, 10, 887),
					(1, 8, 409),
					(1, 8, 954),
					(3, 3, 1793),
					(1, 11, 118),
					(1, 3, 896),
					(3, 8, 37),
					(1, 11, 937),
					(0, 0, 104),
				],
			),
			(
				&[
					(1, 8, 777),
					(1, 4, 2045),
					(3, 6, 141),
					(1, 3, 1655),
					(1, 10, 708),
					(1, 8, 1534),
					(0, 7, 2082),
					(0, 2, 2133),
					(1, 5, 2005),
					(3, 0, 1922),
					(0, 4, 1612),
					(3, 2, 690),
					(3, 3, 50),
					(0, 8, 2245),
					(0, 6, 2104),
					(1, 9, 1447),
					(1, 4, 2244),
					(3, 11, 23),
					(1, 11, 2099),
					(0, 8, 2299),
					(0, 6, 229),
					(1, 5, 2270),
					(0, 8, 1693),
					(1, 5, 1697),
					(1, 0, 2205),
					(3, 9, 1356),
					(1, 9, 114),
					(0, 10, 725),
					(3, 9, 740),
					(0, 8, 1045),
					(0, 10, 288),
					(0, 0, 1855),
					(0, 4, 1022),
					(1, 1, 756),
					(1, 4, 284),
					(0, 2, 1045),
					(3, 2, 1117),
					(3, 11, 1206),
					(1, 11, 1318),
					(1, 7, 467),
				],
				&[
					(0, 8, 2245),
					(0, 6, 2104),
					(1, 10, 708),
					(0, 2, 1045),
					(1, 8, 1534),
					(1, 5, 1697),
					(1, 5, 2005),
					(1, 9, 114),
					(0, 10, 725),
					(1, 8, 777),
					(0, 4, 1612),
					(1, 4, 2244),
					(1, 9, 1447),
					(3, 0, 1922),
					(0, 8, 2299),
					(1, 5, 2270),
					(0, 8, 1045),
					(3, 6, 141),
					(3, 2, 690),
					(3, 2, 1117),
					(1, 7, 467),
					(3, 3, 50),
					(0, 2, 2133),
					(1, 1, 756),
					(0, 10, 288),
					(0, 0, 1855),
					(1, 4, 2045),
					(1, 11, 1318),
					(0, 6, 229),
					(1, 11, 2099),
					(0, 4, 1022),
					(3, 9, 740),
					(1, 3, 1655),
					(1, 4, 284),
					(3, 11, 23),
					(3, 9, 1356),
					(0, 7, 2082),
					(1, 0, 2205),
					(0, 8, 1693),
					(3, 11, 1206),
				],
			),
			(
				&[
					(0, 4, 1583),
					(1, 6, 770),
					(1, 1, 1038),
					(3, 8, 856),
					(3, 6, 85),
					(0, 0, 1627),
					(0, 0, 656),
					(1, 11, 2073),
					(3, 6, 2231),
					(0, 10, 2115),
					(1, 3, 2145),
					(3, 0, 1617),
					(3, 9, 1315),
					(3, 10, 1746),
					(0, 11, 1223),
					(0, 3, 194),
					(1, 1, 313),
					(1, 4, 648),
					(1, 9, 1033),
					(0, 0, 2296),
					(0, 9, 891),
					(3, 7, 702),
					(3, 9, 2084),
					(0, 6, 820),
					(1, 1, 842),
					(3, 10, 1773),
					(3, 3, 2016),
					(0, 10, 1597),
					(1, 8, 2047),
					(0, 5, 1647),
					(1, 0, 642),
					(0, 5, 553),
					(1, 6, 872),
					(1, 10, 394),
					(1, 8, 1408),
					(3, 8, 1984),
					(3, 3, 267),
					(3, 0, 346),
					(0, 2, 682),
					(3, 3, 1097),
					(1, 9, 2072),
					(1, 5, 1387),
					(1, 1, 1192),
					(0, 9, 2002),
					(0, 9, 2257),
					(0, 5, 160),
					(1, 1, 1557),
					(0, 2, 1396),
					(0, 9, 1548),
					(0, 9, 2253),
					(0, 9, 334),
					(1, 5, 1210),
					(3, 8, 468),
					(1, 4, 441),
					(0, 4, 50),
					(3, 10, 59),
					(0, 6, 471),
					(0, 3, 981),
					(3, 6, 663),
					(0, 7, 685),
					(3, 3, 651),
					(3, 1, 1782),
					(1, 8, 1204),
					(3, 4, 1953),
					(1, 1, 850),
					(3, 5, 162),
					(0, 0, 1210),
					(3, 9, 1311),
					(1, 6, 1283),
					(1, 1, 262),
					(1, 9, 1867),
					(0, 4, 881),
					(3, 8, 1920),
					(3, 5, 1061),
					(0, 8, 851),
					(1, 3, 1009),
					(1, 1, 1150),
					(0, 7, 370),
					(3, 9, 1388),
					(0, 6, 1256),
					(0, 5, 765),
					(1, 9, 1240),
					(0, 5, 413),
					(3, 9, 377),
					(0, 3, 83),
					(0, 6, 296),
					(1, 8, 290),
					(3, 1, 88),
					(3, 0, 1191),
					(1, 7, 1920),
					(0, 1, 2053),
					(1, 1, 2085),
					(3, 2, 735),
					(0, 2, 1309),
					(1, 1, 2106),
					(3, 4, 517),
					(0, 2, 2234),
					(3, 0, 1294),
					(3, 10, 2264),
					(3, 11, 841),
					(0, 4, 1772),
					(3, 2, 198),
					(3, 10, 1012),
					(1, 1, 1829),
					(1, 8, 1024),
					(3, 7, 2203),
					(1, 0, 1620),
					(1, 2, 1056),
					(1, 0, 1706),
					(3, 0, 255),
					(3, 5, 566),
					(3, 2, 567),
					(1, 4, 1629),
					(3, 6, 705),
					(3, 1, 956),
					(1, 0, 727),
					(3, 5, 2051),
					(3, 7, 924),
					(0, 5, 2027),
					(3, 7, 921),
					(3, 6, 1380),
					(3, 9, 1127),
					(3, 3, 197),
					(0, 8, 1510),
					(0, 8, 834),
					(1, 4, 1227),
					(3, 5, 676),
					(3, 11, 1903),
					(3, 1, 504),
					(3, 8, 1545),
					(0, 2, 1026),
					(1, 3, 213),
					(1, 10, 1612),
					(3, 10, 1425),
					(1, 8, 675),
					(3, 11, 166),
					(3, 1, 1045),
					(3, 1, 1095),
					(3, 1, 569),
					(3, 10, 335),
					(1, 3, 1566),
					(1, 6, 674),
					(1, 7, 517),
					(3, 7, 868),
					(0, 6, 2187),
					(1, 1, 1210),
					(1, 3, 1551),
					(3, 8, 16),
					(0, 8, 1797),
					(3, 0, 126),
					(3, 9, 992),
					(1, 3, 708),
					(1, 2, 2221),
					(0, 4, 1274),
					(3, 4, 1828),
					(0, 8, 1462),
					(1, 6, 498),
					(0, 9, 1569),
					(0, 4, 442),
					(0, 1, 54),
					(3, 4, 559),
					(0, 8, 1530),
					(3, 4, 1790),
					(3, 10, 1461),
					(3, 5, 3),
					(0, 7, 1841),
					(1, 4, 2208),
					(1, 5, 2016),
					(0, 10, 1546),
					(1, 3, 2281),
					(0, 4, 2092),
					(0, 7, 2117),
					(1, 11, 1250),
					(3, 2, 1840),
					(3, 10, 2174),
					(0, 5, 2155),
					(0, 10, 1594),
					(3, 6, 1659),
					(1, 9, 277),
					(1, 11, 1014),
					(3, 10, 1191),
					(3, 0, 1667),
					(3, 10, 639),
					(3, 6, 1106),
					(0, 1, 41),
					(1, 4, 1684),
					(3, 8, 1243),
					(0, 7, 1062),
					(1, 2, 1913),
					(3, 0, 1109),
					(3, 1, 1730),
					(0, 5, 274),
					(3, 7, 80),
					(0, 8, 662),
					(3, 1, 1646),
					(3, 11, 1129),
					(3, 4, 855),
					(3, 3, 971),
					(1, 4, 280),
					(0, 11, 2143),
				],
				&[
					(1, 9, 1033),
					(0, 2, 1396),
					(3, 4, 517),
					(1, 1, 1038),
					(1, 4, 280),
					(0, 9, 1548),
					(3, 9, 2084),
					(1, 6, 872),
					(0, 4, 50),
					(1, 11, 1014),
					(3, 0, 1617),
					(3, 1, 569),
					(3, 1, 1646),
					(3, 3, 267),
					(1, 1, 1150),
					(0, 1, 54),
					(0, 10, 1594),
					(3, 5, 676),
					(0, 7, 2117),
					(1, 6, 1283),
					(3, 6, 705),
					(3, 3, 651),
					(3, 9, 1311),
					(0, 11, 1223),
					(0, 3, 981),
					(0, 0, 656),
					(0, 2, 1309),
					(1, 3, 2281),
					(1, 5, 2016),
					(3, 2, 735),
					(3, 3, 971),
					(0, 10, 2115),
					(0, 6, 296),
					(0, 8, 1462),
					(1, 9, 1867),
					(1, 1, 850),
					(1, 8, 675),
					(1, 1, 313),
					(1, 10, 1612),
					(1, 5, 1387),
					(3, 1, 1730),
					(3, 0, 1109),
					(3, 4, 1790),
					(0, 8, 662),
					(3, 0, 1667),
					(3, 0, 346),
					(0, 8, 1510),
					(3, 10, 1461),
					(3, 0, 126),
					(0, 6, 1256),
					(1, 10, 394),
					(0, 2, 682),
					(0, 7, 370),
					(0, 9, 334),
					(1, 9, 1240),
					(1, 3, 708),
					(3, 0, 1294),
					(3, 8, 1545),
					(0, 10, 1546),
					(3, 7, 924),
					(3, 10, 2174),
					(1, 8, 1024),
					(3, 8, 1920),
					(0, 0, 1627),
					(3, 8, 1984),
					(0, 9, 2002),
					(3, 4, 855),
					(0, 2, 2234),
					(0, 8, 1530),
					(3, 8, 16),
					(1, 1, 262),
					(1, 8, 1408),
					(3, 10, 1191),
					(1, 9, 2072),
					(0, 9, 891),
					(1, 0, 1706),
					(0, 3, 194),
					(3, 5, 566),
					(3, 2, 567),
					(3, 1, 1782),
					(3, 6, 85),
					(0, 4, 1583),
					(0, 7, 685),
					(1, 3, 1551),
					(0, 4, 881),
					(1, 8, 290),
					(0, 5, 160),
					(0, 8, 851),
					(1, 7, 517),
					(3, 0, 1191),
					(3, 1, 88),
					(0, 11, 2143),
					(3, 7, 702),
					(0, 0, 2296),
					(0, 9, 2253),
					(0, 4, 1274),
					(0, 5, 553),
					(3, 5, 3),
					(3, 10, 1012),
					(1, 3, 2145),
					(1, 4, 2208),
					(0, 1, 2053),
					(3, 8, 1243),
					(3, 8, 468),
					(0, 7, 1062),
					(3, 11, 1129),
					(1, 8, 1204),
					(3, 6, 663),
					(3, 9, 1315),
					(0, 6, 471),
					(3, 3, 197),
					(3, 6, 2231),
					(1, 4, 441),
					(3, 9, 1388),
					(1, 0, 727),
					(3, 4, 1828),
					(3, 2, 1840),
					(3, 10, 59),
					(1, 3, 1009),
					(0, 5, 274),
					(3, 10, 1746),
					(1, 1, 1192),
					(3, 10, 1773),
					(3, 1, 504),
					(1, 6, 770),
					(0, 1, 41),
					(3, 11, 841),
					(1, 2, 1056),
					(1, 4, 1227),
					(1, 1, 1210),
					(1, 5, 1210),
					(1, 2, 1913),
					(0, 4, 442),
					(3, 6, 1659),
					(3, 5, 2051),
					(1, 9, 277),
					(0, 4, 1772),
					(1, 6, 498),
					(0, 8, 1797),
					(3, 9, 1127),
					(1, 4, 1684),
					(3, 7, 80),
					(1, 4, 1629),
					(0, 7, 1841),
					(1, 11, 1250),
					(0, 5, 2027),
					(0, 5, 413),
					(0, 4, 2092),
					(1, 8, 2047),
					(3, 7, 2203),
					(0, 0, 1210),
					(3, 10, 335),
					(0, 5, 2155),
					(1, 1, 2106),
					(3, 8, 856),
					(3, 3, 2016),
					(1, 7, 1920),
					(3, 6, 1106),
					(3, 9, 992),
					(1, 6, 674),
					(1, 11, 2073),
					(1, 0, 642),
					(3, 7, 921),
					(3, 6, 1380),
					(0, 9, 1569),
					(3, 3, 1097),
					(3, 1, 1045),
					(0, 2, 1026),
					(3, 5, 1061),
					(3, 4, 559),
					(1, 1, 842),
					(3, 10, 639),
					(3, 4, 1953),
					(1, 3, 213),
					(1, 4, 648),
					(0, 10, 1597),
					(0, 5, 1647),
					(3, 1, 956),
					(0, 8, 834),
					(3, 11, 1903),
					(3, 7, 868),
					(0, 5, 765),
					(3, 2, 198),
					(1, 0, 1620),
					(3, 10, 1425),
					(3, 10, 2264),
					(3, 9, 377),
					(3, 0, 255),
					(0, 9, 2257),
					(1, 1, 1829),
					(1, 2, 2221),
					(0, 6, 2187),
					(0, 6, 820),
					(0, 3, 83),
					(1, 1, 2085),
					(3, 11, 166),
					(3, 1, 1095),
					(3, 5, 162),
					(1, 3, 1566),
					(1, 1, 1557),
				],
			),
		];
		for (input, expected) in cases {
			let order = set_order(input.iter().map(|&k| (k, tuple_hash(&[k.0, k.1, k.2]))));
			assert_eq!(order.as_slice(), *expected);
		}
	}

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
