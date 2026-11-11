//! The parts of Python's `json` module that the JSON tables (`Debg`, `bgcl`)
//! use: `json.loads` with CPython's C scanner, and `json.dumps` with the
//! options those tables pass.
//!
//! Python values are modelled by [`JsonValue`]. Objects keep their insertion
//! order, as Python dicts do; integers keep their decimal digits, since Python
//! integers are unbounded.

use fontoxide_misc::py::float_repr;

/// A value decoded from JSON, as Python holds it.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
	/// `None`.
	Null,
	/// `True` or `False`.
	Bool(bool),
	/// An `int`, as its canonical decimal digits.
	Int(String),
	/// A `float`.
	Float(f64),
	/// A `str`.
	Str(String),
	/// A `list`.
	Array(Vec<JsonValue>),
	/// A `dict`, in insertion order.
	Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
	/// An integer value.
	pub fn int(v: i64) -> Self {
		JsonValue::Int(v.to_string())
	}

	/// A string value.
	pub fn str(v: &str) -> Self {
		JsonValue::Str(v.to_owned())
	}

	/// `dict.get(key)` of an object.
	pub fn get(&self, key: &str) -> Option<&JsonValue> {
		match self {
			JsonValue::Object(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
			_ => None,
		}
	}
}

/// A `json.JSONDecodeError` (or another exception `json.loads` raises).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
	/// `repr()` of the exception, e.g.
	/// `JSONDecodeError('Expecting value: line 1 column 1 (char 0)')`.
	pub repr: String,
}

impl std::fmt::Display for JsonError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.repr)
	}
}

impl std::error::Error for JsonError {}

fn decode_error(msg: &str, doc: &[char], pos: usize) -> JsonError {
	let lineno = doc[..pos].iter().filter(|&&c| c == '\n').count() + 1;
	let colno = match doc[..pos].iter().rposition(|&c| c == '\n') {
		Some(nl) => pos - nl,
		None => pos + 1,
	};
	let message = format!("{msg}: line {lineno} column {colno} (char {pos})");
	JsonError {
		repr: format!(
			"JSONDecodeError({})",
			fontoxide_misc::py::str_repr(&message)
		),
	}
}

/// Nesting deeper than this fails, where CPython would raise `RecursionError`.
const MAX_DEPTH: usize = 2000;

/// `json.loads(s)` of a `str`.
pub fn loads(s: &str) -> Result<JsonValue, JsonError> {
	let doc: Vec<char> = s.chars().collect();
	if doc.first() == Some(&'\u{feff}') {
		return Err(decode_error(
			"Unexpected UTF-8 BOM (decode using utf-8-sig)",
			&doc,
			0,
		));
	}
	let mut scanner = Scanner { s: &doc, depth: 0 };
	let idx = scanner.skip_ws(0);
	let (value, end) = match scanner.scan_once(idx) {
		Ok(v) => v,
		Err(Stop::Iteration(pos)) => return Err(decode_error("Expecting value", &doc, pos)),
		Err(Stop::Error(e)) => return Err(e),
	};
	let end = scanner.skip_ws(end);
	if end != doc.len() {
		return Err(decode_error("Extra data", &doc, end));
	}
	Ok(value)
}

/// `json.loads(b)` of `bytes`: decoded with the encoding that
/// `json.detect_encoding` guesses.
pub fn loads_bytes(b: &[u8]) -> Result<JsonValue, JsonError> {
	let text = decode_detected(b)?;
	loads(&text)
}

fn unicode_error(encoding: &str) -> JsonError {
	JsonError {
		repr: format!("UnicodeDecodeError('{encoding}', ...)"),
	}
}

/// `b.decode(detect_encoding(b), 'surrogatepass')`.
fn decode_detected(b: &[u8]) -> Result<String, JsonError> {
	let utf16 = |data: &[u8], be: bool, name: &str| -> Result<String, JsonError> {
		if !data.len().is_multiple_of(2) {
			return Err(unicode_error(name));
		}
		let units = data.as_chunks::<2>().0.iter().map(|&c| {
			if be {
				u16::from_be_bytes(c)
			} else {
				u16::from_le_bytes(c)
			}
		});
		char::decode_utf16(units)
			.collect::<Result<String, _>>()
			.map_err(|_| unicode_error(name))
	};
	let utf32 = |data: &[u8], be: bool, name: &str| -> Result<String, JsonError> {
		if !data.len().is_multiple_of(4) {
			return Err(unicode_error(name));
		}
		data.as_chunks::<4>()
			.0
			.iter()
			.map(|&c| {
				let v = if be {
					u32::from_be_bytes(c)
				} else {
					u32::from_le_bytes(c)
				};
				char::from_u32(v).ok_or_else(|| unicode_error(name))
			})
			.collect()
	};
	let utf8 = |data: &[u8], name: &str| -> Result<String, JsonError> {
		String::from_utf8(data.to_vec()).map_err(|_| unicode_error(name))
	};
	// The 'utf-32' and 'utf-16' codecs consume the byte order mark.
	if let Some(rest) = b.strip_prefix(&[0, 0, 0xFE, 0xFF]) {
		return utf32(rest, true, "utf-32");
	}
	if let Some(rest) = b.strip_prefix(&[0xFF, 0xFE, 0, 0]) {
		return utf32(rest, false, "utf-32");
	}
	if let Some(rest) = b.strip_prefix(&[0xFE, 0xFF]) {
		return utf16(rest, true, "utf-16");
	}
	if let Some(rest) = b.strip_prefix(&[0xFF, 0xFE]) {
		return utf16(rest, false, "utf-16");
	}
	if let Some(rest) = b.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
		return utf8(rest, "utf-8-sig");
	}
	if b.len() >= 4 {
		if b[0] == 0 {
			return if b[1] != 0 {
				utf16(b, true, "utf-16-be")
			} else {
				utf32(b, true, "utf-32-be")
			};
		}
		if b[1] == 0 {
			return if b[2] != 0 || b[3] != 0 {
				utf16(b, false, "utf-16-le")
			} else {
				utf32(b, false, "utf-32-le")
			};
		}
	} else if b.len() == 2 {
		if b[0] == 0 {
			return utf16(b, true, "utf-16-be");
		}
		if b[1] == 0 {
			return utf16(b, false, "utf-16-le");
		}
	}
	utf8(b, "utf-8")
}

enum Stop {
	/// `StopIteration(idx)`: no value at `idx`.
	Iteration(usize),
	/// A `JSONDecodeError` raised directly.
	Error(JsonError),
}

impl From<JsonError> for Stop {
	fn from(e: JsonError) -> Self {
		Stop::Error(e)
	}
}

struct Scanner<'a> {
	s: &'a [char],
	depth: usize,
}

fn is_ws(c: char) -> bool {
	matches!(c, ' ' | '\t' | '\n' | '\r')
}

impl Scanner<'_> {
	fn at(&self, idx: usize) -> Option<char> {
		self.s.get(idx).copied()
	}

	fn skip_ws(&self, mut idx: usize) -> usize {
		while self.at(idx).is_some_and(is_ws) {
			idx += 1;
		}
		idx
	}

	fn matches(&self, idx: usize, word: &str) -> bool {
		let n = word.chars().count();
		idx + n <= self.s.len() && self.s[idx..idx + n].iter().copied().eq(word.chars())
	}

	fn err(&self, msg: &str, pos: usize) -> Stop {
		Stop::Error(decode_error(msg, self.s, pos))
	}

	/// `scan_once_unicode`.
	fn scan_once(&mut self, idx: usize) -> Result<(JsonValue, usize), Stop> {
		let Some(c) = self.at(idx) else {
			return Err(Stop::Iteration(idx));
		};
		match c {
			'"' => {
				let (s, end) = self.scanstring(idx + 1)?;
				Ok((JsonValue::Str(s), end))
			}
			'{' | '[' => {
				self.depth += 1;
				if self.depth > MAX_DEPTH {
					return Err(Stop::Error(JsonError {
						repr: "RecursionError('maximum recursion depth exceeded while decoding a JSON document')".into(),
					}));
				}
				let result = if c == '{' {
					self.parse_object(idx + 1)
				} else {
					self.parse_array(idx + 1)
				};
				self.depth -= 1;
				result
			}
			'n' if self.matches(idx, "null") => Ok((JsonValue::Null, idx + 4)),
			't' if self.matches(idx, "true") => Ok((JsonValue::Bool(true), idx + 4)),
			'f' if self.matches(idx, "false") => Ok((JsonValue::Bool(false), idx + 5)),
			'N' if self.matches(idx, "NaN") => Ok((JsonValue::Float(f64::NAN), idx + 3)),
			'I' if self.matches(idx, "Infinity") => Ok((JsonValue::Float(f64::INFINITY), idx + 8)),
			'-' if self.matches(idx, "-Infinity") => {
				Ok((JsonValue::Float(f64::NEG_INFINITY), idx + 9))
			}
			_ => self.match_number(idx),
		}
	}

	/// `_parse_object_unicode`; `idx` is just after `{`.
	fn parse_object(&mut self, idx: usize) -> Result<(JsonValue, usize), Stop> {
		let mut items: Vec<(String, JsonValue)> = Vec::new();
		let mut idx = self.skip_ws(idx);
		if self.at(idx) != Some('}') {
			loop {
				if self.at(idx) != Some('"') {
					return Err(self.err("Expecting property name enclosed in double quotes", idx));
				}
				let (key, next) = self.scanstring(idx + 1)?;
				idx = self.skip_ws(next);
				if self.at(idx) != Some(':') {
					return Err(self.err("Expecting ':' delimiter", idx));
				}
				idx = self.skip_ws(idx + 1);
				let (value, next) = self.scan_once(idx)?;
				// dict semantics: a repeated key keeps its first position.
				match items.iter_mut().find(|(k, _)| *k == key) {
					Some(item) => item.1 = value,
					None => items.push((key, value)),
				}
				idx = self.skip_ws(next);
				if self.at(idx) == Some('}') {
					break;
				}
				if self.at(idx) != Some(',') {
					return Err(self.err("Expecting ',' delimiter", idx));
				}
				let comma = idx;
				idx = self.skip_ws(idx + 1);
				if self.at(idx) == Some('}') {
					return Err(self.err("Illegal trailing comma before end of object", comma));
				}
			}
		}
		Ok((JsonValue::Object(items), idx + 1))
	}

	/// `_parse_array_unicode`; `idx` is just after `[`.
	fn parse_array(&mut self, idx: usize) -> Result<(JsonValue, usize), Stop> {
		let mut items = Vec::new();
		let mut idx = self.skip_ws(idx);
		if self.at(idx) != Some(']') {
			loop {
				let (value, next) = self.scan_once(idx)?;
				items.push(value);
				idx = self.skip_ws(next);
				if self.at(idx) == Some(']') {
					break;
				}
				if self.at(idx) != Some(',') {
					return Err(self.err("Expecting ',' delimiter", idx));
				}
				let comma = idx;
				idx = self.skip_ws(idx + 1);
				if self.at(idx) == Some(']') {
					return Err(self.err("Illegal trailing comma before end of array", comma));
				}
			}
		}
		Ok((JsonValue::Array(items), idx + 1))
	}

	/// `scanstring_unicode` with `strict=True`; `end` is just after the quote.
	fn scanstring(&self, mut end: usize) -> Result<(String, usize), Stop> {
		let begin = end - 1;
		let len = self.s.len();
		let mut out = String::new();
		loop {
			let mut next = end;
			let mut c = '\0';
			while next < len {
				c = self.s[next];
				if c == '"' || c == '\\' {
					break;
				}
				if c <= '\u{1f}' {
					return Err(self.err("Invalid control character at", next));
				}
				next += 1;
			}
			if next >= len || !(c == '"' || c == '\\') {
				return Err(self.err("Unterminated string starting at", begin));
			}
			out.extend(&self.s[end..next]);
			next += 1;
			if c == '"' {
				end = next;
				break;
			}
			if next == len {
				return Err(self.err("Unterminated string starting at", begin));
			}
			let e = self.s[next];
			if e != 'u' {
				end = next + 1;
				let ch = match e {
					'"' => '"',
					'\\' => '\\',
					'/' => '/',
					'b' => '\u{8}',
					'f' => '\u{c}',
					'n' => '\n',
					'r' => '\r',
					't' => '\t',
					_ => return Err(self.err("Invalid \\escape", end - 2)),
				};
				out.push(ch);
			} else {
				next += 1;
				end = next + 4;
				if end >= len {
					return Err(self.err("Invalid \\uXXXX escape", next - 1));
				}
				let mut code = self
					.hex4(next)
					.ok_or_else(|| self.err("Invalid \\uXXXX escape", end - 5))?;
				next = end;
				if (0xD800..0xDC00).contains(&code)
					&& end + 6 < len
					&& self.s[next] == '\\'
					&& self.s[next + 1] == 'u'
				{
					end += 6;
					let c2 = self
						.hex4(next + 2)
						.ok_or_else(|| self.err("Invalid \\uXXXX escape", end - 5))?;
					if (0xDC00..0xE000).contains(&c2) {
						code = 0x10000 + (((code - 0xD800) << 10) | (c2 - 0xDC00));
					} else {
						end -= 6;
					}
				}
				// Python strings can hold lone surrogates; Rust strings cannot, so
				// fontoxide fails instead.
				let ch = char::from_u32(code).ok_or_else(|| {
					Stop::Error(JsonError {
						repr: format!("ValueError('lone surrogate \\\\u{code:04x}')"),
					})
				})?;
				out.push(ch);
			}
		}
		Ok((out, end))
	}

	fn hex4(&self, idx: usize) -> Option<u32> {
		let mut v = 0;
		for i in idx..idx + 4 {
			v = (v << 4) | self.at(i)?.to_digit(16)?;
		}
		Some(v)
	}

	/// `_match_number_unicode`.
	fn match_number(&self, start: usize) -> Result<(JsonValue, usize), Stop> {
		let len = self.s.len();
		let digit = |i: usize| self.at(i).is_some_and(|c| c.is_ascii_digit());
		let mut idx = start;
		if self.at(idx) == Some('-') {
			idx += 1;
			if idx >= len {
				return Err(Stop::Iteration(start));
			}
		}
		match self.at(idx) {
			Some('1'..='9') => {
				idx += 1;
				while digit(idx) {
					idx += 1;
				}
			}
			Some('0') => idx += 1,
			_ => return Err(Stop::Iteration(start)),
		}
		let mut is_float = false;
		if idx + 1 < len && self.s[idx] == '.' && digit(idx + 1) {
			is_float = true;
			idx += 2;
			while digit(idx) {
				idx += 1;
			}
		}
		if idx + 1 < len && matches!(self.s[idx], 'e' | 'E') {
			let e_start = idx;
			idx += 1;
			if idx + 1 < len && matches!(self.s[idx], '-' | '+') {
				idx += 1;
			}
			while digit(idx) {
				idx += 1;
			}
			if digit(idx - 1) {
				is_float = true;
			} else {
				idx = e_start;
			}
		}
		let text: String = self.s[start..idx].iter().collect();
		if is_float {
			let v: f64 = text.parse().map_err(|_| Stop::Iteration(start))?;
			Ok((JsonValue::Float(v), idx))
		} else {
			let digits = text.trim_start_matches('-');
			let canonical = if text.starts_with('-') && digits != "0" {
				text.clone()
			} else {
				digits.to_owned()
			};
			Ok((JsonValue::Int(canonical), idx))
		}
	}
}

/// Options of `json.dumps`.
#[derive(Debug, Clone)]
pub struct DumpOptions<'a> {
	/// `indent`, as a string of spaces (`None` for a single line).
	pub indent: Option<&'a str>,
	/// `separators`: (item separator, key separator).
	pub separators: (&'a str, &'a str),
	/// `ensure_ascii`.
	pub ensure_ascii: bool,
}

impl Default for DumpOptions<'_> {
	/// `json.dumps(obj)`.
	fn default() -> Self {
		DumpOptions {
			indent: None,
			separators: (", ", ": "),
			ensure_ascii: true,
		}
	}
}

impl<'a> DumpOptions<'a> {
	/// `json.dumps(obj, indent=n)`: the item separator becomes `","`.
	pub fn indented(indent: &'a str) -> Self {
		DumpOptions {
			indent: Some(indent),
			separators: (",", ": "),
			ensure_ascii: true,
		}
	}
}

/// `json.dumps(value, ...)`.
pub fn dumps(value: &JsonValue, options: &DumpOptions<'_>) -> String {
	let mut out = String::new();
	encode(value, options, 0, &mut out);
	out
}

fn encode_float(v: f64) -> String {
	if v.is_nan() {
		"NaN".into()
	} else if v == f64::INFINITY {
		"Infinity".into()
	} else if v == f64::NEG_INFINITY {
		"-Infinity".into()
	} else {
		float_repr(v)
	}
}

fn encode_str(s: &str, ensure_ascii: bool, out: &mut String) {
	out.push('"');
	for c in s.chars() {
		match c {
			'"' => out.push_str("\\\""),
			'\\' => out.push_str("\\\\"),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\t' => out.push_str("\\t"),
			'\u{8}' => out.push_str("\\b"),
			'\u{c}' => out.push_str("\\f"),
			c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
			c if ensure_ascii && !(' '..='~').contains(&c) => {
				let mut buf = [0u16; 2];
				for unit in c.encode_utf16(&mut buf) {
					out.push_str(&format!("\\u{unit:04x}"));
				}
			}
			c => out.push(c),
		}
	}
	out.push('"');
}

fn newline_indent(options: &DumpOptions<'_>, level: usize, out: &mut String) {
	if let Some(indent) = options.indent {
		out.push('\n');
		for _ in 0..level {
			out.push_str(indent);
		}
	}
}

fn encode(value: &JsonValue, options: &DumpOptions<'_>, level: usize, out: &mut String) {
	match value {
		JsonValue::Null => out.push_str("null"),
		JsonValue::Bool(true) => out.push_str("true"),
		JsonValue::Bool(false) => out.push_str("false"),
		JsonValue::Int(digits) => out.push_str(digits),
		JsonValue::Float(v) => out.push_str(&encode_float(*v)),
		JsonValue::Str(s) => encode_str(s, options.ensure_ascii, out),
		JsonValue::Array(items) => {
			if items.is_empty() {
				out.push_str("[]");
				return;
			}
			out.push('[');
			for (i, item) in items.iter().enumerate() {
				if i > 0 {
					out.push_str(options.separators.0);
				}
				newline_indent(options, level + 1, out);
				encode(item, options, level + 1, out);
			}
			newline_indent(options, level, out);
			out.push(']');
		}
		JsonValue::Object(items) => {
			if items.is_empty() {
				out.push_str("{}");
				return;
			}
			out.push('{');
			for (i, (key, item)) in items.iter().enumerate() {
				if i > 0 {
					out.push_str(options.separators.0);
				}
				newline_indent(options, level + 1, out);
				encode_str(key, options.ensure_ascii, out);
				out.push_str(options.separators.1);
				encode(item, options, level + 1, out);
			}
			newline_indent(options, level, out);
			out.push('}');
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn errors_match_cpython() {
		// Messages checked against CPython 3.13.
		let cases = [
			("", "Expecting value: line 1 column 1 (char 0)"),
			(
				"[1,]",
				"Illegal trailing comma before end of array: line 1 column 3 (char 2)",
			),
			(
				"{\"a\" 1}",
				"Expecting ':' delimiter: line 1 column 6 (char 5)",
			),
			(
				"{1: 2}",
				"Expecting property name enclosed in double quotes: line 1 column 2 (char 1)",
			),
			("[1 2]", "Expecting ',' delimiter: line 1 column 4 (char 3)"),
			(
				"\"abc",
				"Unterminated string starting at: line 1 column 1 (char 0)",
			),
			("\"\\x\"", "Invalid \\escape: line 1 column 2 (char 1)"),
			("1 2", "Extra data: line 1 column 3 (char 2)"),
			("[\n -]", "Expecting value: line 2 column 2 (char 3)"),
		];
		for (doc, message) in cases {
			let error = loads(doc).expect_err(doc);
			assert_eq!(
				error.repr,
				format!("JSONDecodeError({})", fontoxide_misc::py::str_repr(message)),
				"{doc:?}"
			);
		}
	}

	#[test]
	fn round_trips_values() {
		let doc = r#"{"a": [1, -0, 1.5, 1e400, -Infinity, NaN, true, false, null], "b": "\u00e9\ud83d\ude00", "a": 2}"#;
		let value = loads(doc).unwrap_or(JsonValue::Null);
		assert_eq!(
			dumps(&value, &DumpOptions::default()),
			r#"{"a": 2, "b": "\u00e9\ud83d\ude00"}"#
		);
		let value = loads("[1, -0, 1.5, 1e400, -Infinity, 1E5]").unwrap_or(JsonValue::Null);
		assert_eq!(
			dumps(&value, &DumpOptions::default()),
			"[1, 0, 1.5, Infinity, -Infinity, 100000.0]"
		);
	}
}
