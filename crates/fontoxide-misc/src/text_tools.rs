//! Port of `fontTools.misc.textTools`: tags, hex dumps, binary strings, and the
//! literal parsing that fontTools does with `safeEval` (`ast.literal_eval`).

use std::fmt;

/// A four-byte OpenType tag (`Tag`).
///
/// fontTools represents tags as `str` decoded from Latin-1; here they are the raw
/// bytes. Ordering is bytewise, which matches sorting the Latin-1 strings.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Tag(pub [u8; 4]);

impl Tag {
	/// Creates a tag from four bytes.
	pub const fn new(bytes: &[u8; 4]) -> Self {
		Tag(*bytes)
	}

	/// Creates a tag from a string of up to four Latin-1 characters, padded with
	/// spaces. Returns `None` if the string is too long or not Latin-1.
	pub fn from_str_padded(s: &str) -> Option<Self> {
		let bytes = crate::py::latin1_encode(s)?;
		if bytes.len() > 4 {
			return None;
		}
		let mut tag = [b' '; 4];
		tag[..bytes.len()].copy_from_slice(&bytes);
		Some(Tag(tag))
	}

	/// Creates a tag from a string of exactly four Latin-1 characters.
	pub fn from_str_exact(s: &str) -> Option<Self> {
		let bytes = crate::py::latin1_encode(s)?;
		Some(Tag(bytes.try_into().ok()?))
	}

	/// The tag bytes.
	pub const fn to_bytes(self) -> [u8; 4] {
		self.0
	}

	/// The tag as a Latin-1 decoded string, as fontTools holds it.
	pub fn as_string(&self) -> String {
		crate::py::latin1_decode(&self.0)
	}
}

impl fmt::Display for Tag {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		for &b in &self.0 {
			fmt::Write::write_char(f, b as char)?;
		}
		Ok(())
	}
}

impl fmt::Debug for Tag {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Tag({})", crate::py::str_repr(&self.as_string()))
	}
}

impl From<[u8; 4]> for Tag {
	fn from(bytes: [u8; 4]) -> Self {
		Tag(bytes)
	}
}

impl From<&[u8; 4]> for Tag {
	fn from(bytes: &[u8; 4]) -> Self {
		Tag(*bytes)
	}
}

impl PartialEq<&str> for Tag {
	fn eq(&self, other: &&str) -> bool {
		Tag::from_str_exact(other).is_some_and(|t| t == *self)
	}
}

impl PartialEq<[u8; 4]> for Tag {
	fn eq(&self, other: &[u8; 4]) -> bool {
		self.0 == *other
	}
}

/// Converts a list of hex chunks to binary data (`readHex`). Whitespace inside the
/// chunks is ignored; an odd number of digits is padded with a trailing `0`.
pub fn read_hex<S: AsRef<str>>(content: impl IntoIterator<Item = S>) -> Option<Vec<u8>> {
	let mut joined = String::new();
	for chunk in content {
		joined.push_str(chunk.as_ref());
	}
	de_hex_str(&joined)
}

/// Converts a hex string to binary data (`deHexStr`).
pub fn de_hex_str(hexdata: &str) -> Option<Vec<u8>> {
	let digits: Vec<u8> = hexdata.split_whitespace().flat_map(|s| s.bytes()).collect();
	let mut out = Vec::with_capacity(digits.len().div_ceil(2));
	for pair in digits.chunks(2) {
		let hi = hex_value(pair[0])?;
		let lo = match pair.get(1) {
			Some(&c) => hex_value(c)?,
			None => 0,
		};
		out.push(hi << 4 | lo);
	}
	Some(out)
}

fn hex_value(c: u8) -> Option<u8> {
	(c as char).to_digit(16).map(|d| d as u8)
}

/// Converts binary data to a lowercase hex string (`hexStr`).
pub fn hex_str(data: &[u8]) -> String {
	const DIGITS: &[u8; 16] = b"0123456789abcdef";
	let mut out = String::with_capacity(data.len() * 2);
	for &b in data {
		out.push(DIGITS[(b >> 4) as usize] as char);
		out.push(DIGITS[(b & 0xF) as usize] as char);
	}
	out
}

/// Formats the low `bits` bits of `value` as binary digits in groups of eight
/// (`num2binary`), e.g. `"00000000 00001011"`.
pub fn num2binary(value: i64, bits: u32) -> String {
	let mut groups = Vec::new();
	let mut group = String::new();
	let mut l = value;
	for i in 0..bits {
		group.insert(0, if l & 1 != 0 { '1' } else { '0' });
		l >>= 1;
		if (i + 1) % 8 == 0 {
			groups.push(std::mem::take(&mut group));
		}
	}
	if !group.is_empty() {
		groups.push(group);
	}
	groups.reverse();
	groups.join(" ")
}

/// Parses binary digits, ignoring whitespace (`binary2num`). Any digit other than
/// `0` counts as `1`, as in fontTools.
pub fn binary2num(bin: &str) -> i64 {
	let mut l: i64 = 0;
	for c in bin.chars().filter(|c| !c.is_whitespace()) {
		l <<= 1;
		if c != '0' {
			l |= 1;
		}
	}
	l
}

/// Sorts strings ignoring case, with ties broken by the original string
/// (`caselessSort`).
pub fn caseless_sort<S: AsRef<str> + Clone>(list: &[S]) -> Vec<S> {
	let mut sorted = list.to_vec();
	sorted.sort_by(|a, b| {
		let (a, b) = (a.as_ref(), b.as_ref());
		a.to_lowercase()
			.cmp(&b.to_lowercase())
			.then_with(|| a.cmp(b))
	});
	sorted
}

/// Pads `data` with NUL bytes to a multiple of `size` (`pad`).
pub fn pad(data: &[u8], size: usize) -> Vec<u8> {
	let mut out = data.to_vec();
	if size > 1 {
		let remainder = out.len() % size;
		if remainder != 0 {
			out.resize(out.len() + size - remainder, 0);
		}
	}
	out
}

/// A Python literal, as produced by `safeEval` (`ast.literal_eval`).
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
	/// `int`
	Int(i128),
	/// `float`
	Float(f64),
	/// `True` / `False`
	Bool(bool),
	/// `None`
	None,
	/// `str`
	Str(String),
	/// `bytes`
	Bytes(Vec<u8>),
	/// `tuple`
	Tuple(Vec<Literal>),
	/// `list`
	List(Vec<Literal>),
}

impl Literal {
	/// The value as an integer, if it is an `int` or a `bool` (which is an `int`
	/// subclass in Python).
	pub fn as_int(&self) -> Option<i128> {
		match *self {
			Literal::Int(i) => Some(i),
			Literal::Bool(b) => Some(b as i128),
			_ => None,
		}
	}

	/// The value as a float, if it is numeric.
	pub fn as_float(&self) -> Option<f64> {
		match *self {
			Literal::Float(f) => Some(f),
			Literal::Int(i) => Some(i as f64),
			Literal::Bool(b) => Some(b as i64 as f64),
			_ => None,
		}
	}

	/// Python truthiness.
	pub fn is_truthy(&self) -> bool {
		match self {
			Literal::Int(i) => *i != 0,
			Literal::Float(f) => *f != 0.0,
			Literal::Bool(b) => *b,
			Literal::None => false,
			Literal::Str(s) => !s.is_empty(),
			Literal::Bytes(b) => !b.is_empty(),
			Literal::Tuple(v) | Literal::List(v) => !v.is_empty(),
		}
	}
}

/// Evaluates a Python literal (`safeEval`, i.e. `ast.literal_eval`).
///
/// Supports numbers (with unary signs), `True`/`False`/`None`, single- and
/// double-quoted strings and bytes with common escapes, tuples and lists. Returns
/// `None` where Python would raise.
pub fn safe_eval(s: &str) -> Option<Literal> {
	let mut p = LiteralParser {
		s: s.trim_start_matches([' ', '\t']).as_bytes(),
		pos: 0,
	};
	let value = p.expr()?;
	p.skip_ws();
	(p.pos == p.s.len()).then_some(value)
}

/// `safeEval` for a value that must be an integer.
pub fn safe_eval_int(s: &str) -> Option<i128> {
	safe_eval(s)?.as_int()
}

/// `safeEval` for a value that must be numeric.
pub fn safe_eval_float(s: &str) -> Option<f64> {
	safe_eval(s)?.as_float()
}

/// `int(s, 0)`: an integer literal with an optional base prefix and sign.
pub fn parse_int_literal(s: &str) -> Option<i128> {
	let t = s.trim();
	let (negative, t) = match t.as_bytes().first() {
		Some(b'-') => (true, &t[1..]),
		Some(b'+') => (false, &t[1..]),
		_ => (false, t),
	};
	let v = parse_unsigned_int(t)?;
	Some(if negative { -v } else { v })
}

fn parse_unsigned_int(t: &str) -> Option<i128> {
	let lower = t.to_ascii_lowercase();
	let (radix, digits) = if let Some(d) = lower.strip_prefix("0x") {
		(16, d)
	} else if let Some(d) = lower.strip_prefix("0o") {
		(8, d)
	} else if let Some(d) = lower.strip_prefix("0b") {
		(2, d)
	} else {
		// Decimal literals may not have leading zeros, except for zero itself.
		if lower.len() > 1
			&& lower.starts_with('0')
			&& lower.bytes().any(|b| b != b'0' && b != b'_')
		{
			return None;
		}
		(10, lower.as_str())
	};
	let digits = digits.strip_prefix('_').unwrap_or(digits);
	if digits.is_empty()
		|| digits.starts_with('_')
		|| digits.ends_with('_')
		|| digits.contains("__")
	{
		return None;
	}
	i128::from_str_radix(&digits.replace('_', ""), radix).ok()
}

struct LiteralParser<'a> {
	s: &'a [u8],
	pos: usize,
}

impl LiteralParser<'_> {
	fn skip_ws(&mut self) {
		while self
			.s
			.get(self.pos)
			.is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r'))
		{
			self.pos += 1;
		}
	}

	fn peek(&self) -> Option<u8> {
		self.s.get(self.pos).copied()
	}

	fn expr(&mut self) -> Option<Literal> {
		self.skip_ws();
		match self.peek()? {
			b'-' | b'+' => {
				let negative = self.peek() == Some(b'-');
				self.pos += 1;
				match self.expr()? {
					Literal::Int(i) => Some(Literal::Int(if negative { -i } else { i })),
					Literal::Float(f) => Some(Literal::Float(if negative { -f } else { f })),
					Literal::Bool(b) => {
						let i = b as i128;
						Some(Literal::Int(if negative { -i } else { i }))
					}
					_ => None,
				}
			}
			b'(' => {
				self.pos += 1;
				let (items, trailing_comma) = self.sequence(b')')?;
				if items.len() == 1 && !trailing_comma {
					items.into_iter().next()
				} else {
					Some(Literal::Tuple(items))
				}
			}
			b'[' => {
				self.pos += 1;
				Some(Literal::List(self.sequence(b']')?.0))
			}
			b'\'' | b'"' => self.string(false).map(Literal::Str),
			b'b' | b'B' if matches!(self.s.get(self.pos + 1), Some(b'\'' | b'"')) => {
				self.pos += 1;
				let s = self.string(true)?;
				Some(Literal::Bytes(s.chars().map(|c| c as u8).collect()))
			}
			c if c.is_ascii_digit() || c == b'.' => self.number(),
			c if c.is_ascii_alphabetic() || c == b'_' => {
				let start = self.pos;
				while self
					.peek()
					.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
				{
					self.pos += 1;
				}
				match &self.s[start..self.pos] {
					b"True" => Some(Literal::Bool(true)),
					b"False" => Some(Literal::Bool(false)),
					b"None" => Some(Literal::None),
					_ => None,
				}
			}
			_ => None,
		}
	}

	/// Parses comma-separated items up to `close`; returns whether the last item
	/// was followed by a comma.
	fn sequence(&mut self, close: u8) -> Option<(Vec<Literal>, bool)> {
		let mut items = Vec::new();
		let mut trailing_comma = false;
		loop {
			self.skip_ws();
			if self.peek()? == close {
				self.pos += 1;
				return Some((items, trailing_comma));
			}
			items.push(self.expr()?);
			self.skip_ws();
			match self.peek()? {
				b',' => {
					self.pos += 1;
					trailing_comma = true;
				}
				c if c == close => {
					trailing_comma = false;
				}
				_ => return None,
			}
		}
	}

	fn number(&mut self) -> Option<Literal> {
		let start = self.pos;
		while self
			.peek()
			.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.')
		{
			let c = self.peek()?;
			self.pos += 1;
			// Exponent sign, e.g. 1e-5 (not in hex literals).
			if matches!(c, b'e' | b'E')
				&& matches!(self.peek(), Some(b'+' | b'-'))
				&& !self.s[start..].starts_with(b"0x")
				&& !self.s[start..].starts_with(b"0X")
			{
				self.pos += 1;
			}
		}
		let text = std::str::from_utf8(&self.s[start..self.pos]).ok()?;
		if let Some(i) = parse_unsigned_int(text) {
			return Some(Literal::Int(i));
		}
		let lower = text.to_ascii_lowercase();
		if lower.bytes().all(|c| c.is_ascii_digit() || c == b'_') {
			// An integer literal that parse_unsigned_int rejected, e.g. "012".
			return None;
		}
		if lower.starts_with("0x") || lower.starts_with("0o") || lower.starts_with("0b") {
			return None;
		}
		if lower.contains(|c: char| c.is_ascii_alphabetic() && c != 'e') {
			return None;
		}
		crate::py::parse_float(text).map(Literal::Float)
	}

	fn string(&mut self, bytes: bool) -> Option<String> {
		let quote = self.peek()? as char;
		// Triple-quoted strings are not used in TTX.
		self.pos += 1;
		let rest = std::str::from_utf8(&self.s[self.pos..]).ok()?;
		let (out, consumed) = unescape(rest, Some(quote), bytes)?;
		self.pos += consumed;
		Some(out)
	}
}

/// Processes Python string escapes in `s` up to the closing `quote` (or to the end
/// of `s` if `quote` is `None`). Returns the text and the number of bytes consumed,
/// including the closing quote.
fn unescape(s: &str, quote: Option<char>, bytes: bool) -> Option<(String, usize)> {
	let mut out = String::new();
	let mut chars = s.char_indices().peekable();
	while let Some((i, c)) = chars.next() {
		if Some(c) == quote {
			return Some((out, i + 1));
		}
		if c != '\\' {
			if bytes && !c.is_ascii() {
				return None;
			}
			out.push(c);
			continue;
		}
		let (_, e) = chars.next()?;
		match e {
			'\n' => {} // line continuation
			'\\' => out.push('\\'),
			'\'' => out.push('\''),
			'"' => out.push('"'),
			'a' => out.push('\x07'),
			'b' => out.push('\x08'),
			'f' => out.push('\x0c'),
			'v' => out.push('\x0b'),
			'n' => out.push('\n'),
			'r' => out.push('\r'),
			't' => out.push('\t'),
			'0'..='7' => {
				let mut n = e.to_digit(8)?;
				for _ in 0..2 {
					match chars.peek() {
						Some(&(_, d)) if d.is_digit(8) => {
							n = n * 8 + d.to_digit(8)?;
							chars.next();
						}
						_ => break,
					}
				}
				out.push(char::from_u32(n)?);
			}
			'x' => {
				let h: String = (0..2)
					.filter_map(|_| chars.next().map(|(_, c)| c))
					.collect();
				if h.len() != 2 {
					return None;
				}
				out.push(char::from_u32(u32::from_str_radix(&h, 16).ok()?)?);
			}
			'u' | 'U' if !bytes => {
				let len = if e == 'u' { 4 } else { 8 };
				let h: String = (0..len)
					.filter_map(|_| chars.next().map(|(_, c)| c))
					.collect();
				if h.len() != len {
					return None;
				}
				out.push(char::from_u32(u32::from_str_radix(&h, 16).ok()?)?);
			}
			other => {
				out.push('\\');
				out.push(other);
			}
		}
	}
	match quote {
		None => Some((out, s.len())),
		Some(_) => None,
	}
}

/// The value of a Python string literal with the given body, as in
/// `safeEval("'''" + body + "'''")`: escapes are processed, quotes are kept.
pub fn parse_string_body(body: &str) -> Option<String> {
	if body.contains("'''") {
		return None;
	}
	unescape(body, None, false).map(|(s, _)| s)
}

#[cfg(test)]
mod tests {
	//! Port of Tests/misc/textTools_test.py (the parts that apply to a byte-array
	//! tag) and the doctests in Lib/fontTools/misc/textTools.py.

	use super::*;

	#[test]
	fn test_tag_comparison() {
		assert_eq!(Tag::new(b"head"), "head");
		assert_eq!(Tag::new(b"head"), *b"head");
		assert_ne!(Tag::new(b"head"), "name");
		assert_eq!(Tag::from_str_exact("h\u{e9}ad"), Some(Tag::new(b"h\xe9ad")));
		assert_ne!(Tag::new(b"h\xe9ad"), "head");
	}

	#[test]
	fn test_pad() {
		assert_eq!(pad(b"abcd", 4).len(), 4);
		assert_eq!(pad(b"abcde", 2).len(), 6);
		assert_eq!(pad(b"abcde", 4).len(), 8);
		assert_eq!(pad(b"abcdef", 4), b"abcdef\x00\x00");
		assert_eq!(pad(b"abcdef", 1), b"abcdef");
	}

	#[test]
	fn binary_strings() {
		assert_eq!(num2binary(11, 16), "00000000 00001011");
		assert_eq!(num2binary(-1, 4), "1111");
		assert_eq!(binary2num("00000000 00001011"), 11);
	}

	#[test]
	fn hex() {
		assert_eq!(hex_str(b"\x01\xab"), "01ab");
		assert_eq!(de_hex_str("01 ab\nc"), Some(vec![0x01, 0xab, 0xc0]));
		assert_eq!(read_hex(["0001", "02"]), Some(vec![0, 1, 2]));
	}

	#[test]
	fn literals() {
		assert_eq!(safe_eval("12"), Some(Literal::Int(12)));
		assert_eq!(safe_eval(" -0x1F"), Some(Literal::Int(-31)));
		assert_eq!(safe_eval("1.5"), Some(Literal::Float(1.5)));
		assert_eq!(safe_eval("1e-5"), Some(Literal::Float(1e-5)));
		assert_eq!(safe_eval("True"), Some(Literal::Bool(true)));
		assert_eq!(safe_eval("012"), None);
		assert_eq!(safe_eval("00"), Some(Literal::Int(0)));
		assert_eq!(safe_eval("'a\\x41'"), Some(Literal::Str("aA".into())));
		assert_eq!(
			safe_eval("(1, 2)"),
			Some(Literal::Tuple(vec![Literal::Int(1), Literal::Int(2)]))
		);
		assert_eq!(
			safe_eval("(1,)"),
			Some(Literal::Tuple(vec![Literal::Int(1)]))
		);
		assert_eq!(safe_eval("(1)"), Some(Literal::Int(1)));
		assert_eq!(safe_eval("[]"), Some(Literal::List(vec![])));
		assert_eq!(safe_eval("foo"), None);
		assert_eq!(parse_int_literal("0x00010000"), Some(0x10000));
		assert_eq!(parse_int_literal("010"), None);
		assert_eq!(parse_string_body(r"\x00A'B").as_deref(), Some("\0A'B"));
	}

	#[test]
	fn caseless() {
		assert_eq!(caseless_sort(&["b", "A", "a", "B"]), ["A", "a", "B", "b"]);
	}
}
