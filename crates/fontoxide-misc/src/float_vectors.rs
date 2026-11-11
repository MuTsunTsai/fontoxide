//! Comparison of test vectors made by fontTools with fontoxide's results, for
//! the tests of several crates.
//!
//! Some results depend on the platform's math library: CPython and Rust both
//! call it for `pow`, `hypot` and the like, which differ by an ulp or so
//! between Windows and Linux, and some algorithms amplify that. fontoxide
//! matches fontTools on the same platform. A vector file records the platform
//! it was made on in a first line `# platform: <sys.platform>`: on that
//! platform, results must be the same text; on another, the same text with
//! each number within [`CROSS_PLATFORM_TOLERANCE`].

/// The relative difference allowed between numbers on another platform than
/// the one the vectors were made on. The largest measured difference between
/// Windows and Linux (qu2cu, whose fitting amplifies it) is about 2e-13.
pub const CROSS_PLATFORM_TOLERANCE: f64 = 1e-9;

/// The `sys.platform` of Python on this platform.
pub fn python_platform() -> &'static str {
	if cfg!(windows) {
		"win32"
	} else if cfg!(target_os = "macos") {
		"darwin"
	} else {
		"linux"
	}
}

/// Vectors: the platform of the `# platform:` line, if any, and whether the
/// results must match exactly (made on this platform, or of unknown platform).
#[derive(Debug, Clone)]
pub struct Vectors<'a> {
	text: &'a str,
	/// Whether results must be the same text.
	pub exact: bool,
}

impl<'a> Vectors<'a> {
	/// Reads the header of vector file contents.
	pub fn new(text: &'a str) -> Self {
		let platform = text
			.lines()
			.next()
			.and_then(|l| l.strip_prefix("# platform:"))
			.map(str::trim);
		Vectors {
			text,
			exact: platform.is_none_or(|p| p == python_platform()),
		}
	}

	/// The lines of vectors, without comments.
	pub fn lines(&self) -> impl Iterator<Item = &'a str> {
		self.text.lines().filter(|l| !l.starts_with('#'))
	}

	/// Whether `got` matches `expected` (see the module documentation).
	pub fn matches(&self, expected: &str, got: &str) -> bool {
		expected == got || (!self.exact && approximately_equal(expected, got))
	}
}

/// Whether two texts are the same except for numbers, which are within
/// [`CROSS_PLATFORM_TOLERANCE`] of each other.
pub fn approximately_equal(a: &str, b: &str) -> bool {
	let (sa, sb) = (segments(a), segments(b));
	sa.len() == sb.len()
		&& sa.iter().zip(&sb).all(|(x, y)| match (x, y) {
			(Segment::Number(x), Segment::Number(y)) => {
				x == y
					|| (x.is_nan() && y.is_nan())
					|| (x - y).abs() <= CROSS_PLATFORM_TOLERANCE * 1f64.max(x.abs()).max(y.abs())
			}
			(Segment::Text(x), Segment::Text(y)) => x == y,
			_ => false,
		})
}

#[derive(Debug, PartialEq)]
enum Segment<'a> {
	Text(&'a str),
	Number(f64),
}

/// Splits text into numbers (as Python's `repr` writes floats and ints) and
/// the text between them.
fn segments(s: &str) -> Vec<Segment<'_>> {
	let bytes = s.as_bytes();
	let mut out = Vec::new();
	let (mut i, mut text_start) = (0, 0);
	while i < bytes.len() {
		let starts_number = bytes[i].is_ascii_digit()
			|| (bytes[i] == b'-' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit));
		if !starts_number {
			i += 1;
			continue;
		}
		let start = i;
		i += 1;
		while i < bytes.len() {
			let c = bytes[i];
			let exponent_sign = (c == b'-' || c == b'+') && matches!(bytes[i - 1], b'e' | b'E');
			if c.is_ascii_digit() || c == b'.' || c == b'e' || c == b'E' || exponent_sign {
				i += 1;
			} else {
				break;
			}
		}
		if let Ok(v) = s[start..i].parse::<f64>() {
			if text_start < start {
				out.push(Segment::Text(&s[text_start..start]));
			}
			out.push(Segment::Number(v));
			text_start = i;
		}
	}
	if text_start < s.len() {
		out.push(Segment::Text(&s[text_start..]));
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn header_and_matching() {
		let other = if python_platform() == "linux" {
			"win32"
		} else {
			"linux"
		};
		let same = format!("# platform: {}\na 1.0\n", python_platform());
		let v = Vectors::new(&same);
		assert!(v.exact);
		assert_eq!(v.lines().collect::<Vec<_>>(), ["a 1.0"]);
		assert!(!v.matches("x 117.99999999999147", "x 118.0"));
		let different = format!("# platform: {other}\n");
		let v = Vectors::new(&different);
		assert!(!v.exact);
		assert!(v.matches("x 117.99999999999147,2.5e-3", "x 118.0,0.0025"));
		assert!(!v.matches("x 117.9", "x 118.0"));
		assert!(!v.matches("x 1.0 y", "x 1.0 z"));
		assert!(!v.matches("x 1.0", "x 1.0 2.0"));
		assert!(Vectors::new("no header\n").exact);
	}
}
