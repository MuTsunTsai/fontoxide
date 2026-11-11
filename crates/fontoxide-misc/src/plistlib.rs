//! Port of `fontTools.misc.plistlib`: property lists in XML.
//!
//! [`Value`] keeps the distinctions of the Python objects a plist is read into
//! (`bool`, `int`, `float`, `str`, `bytes`, `datetime`, `list`, `dict`), and
//! dictionaries keep their insertion order like Python dicts.

use std::fmt;

use indexmap::IndexMap;

use crate::etree::{self, Element};
use crate::py;
use crate::xml_reader::{self, Content};

/// A plist dictionary (`dict`).
pub type Dict = IndexMap<String, Value>;

/// An error reading or writing a plist.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlistError {
	/// Malformed plist structure or value (`ValueError`).
	#[error("{0}")]
	Value(String),
	/// An integer outside the range plists can store (`OverflowError`).
	#[error("{0}")]
	Overflow(String),
	/// Invalid base64 data (`binascii.Error`).
	#[error("{0}")]
	Base64(String),
	/// An XML syntax or serialization error.
	#[error("{0}")]
	Xml(String),
}

type Result<T> = std::result::Result<T, PlistError>;

/// A plist date (`datetime.datetime`, without microseconds or time zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
	/// The year, 1 to 9999.
	pub year: u16,
	/// The month, 1 to 12.
	pub month: u8,
	/// The day of the month.
	pub day: u8,
	/// The hour, 0 to 23.
	pub hour: u8,
	/// The minute, 0 to 59.
	pub minute: u8,
	/// The second, 0 to 59.
	pub second: u8,
}

/// A plist value.
#[derive(Debug, Clone)]
pub enum Value {
	/// `<true/>` or `<false/>` (`bool`).
	Bool(bool),
	/// `<integer>` (`int`; plists store 64-bit signed or unsigned values).
	Integer(i128),
	/// `<real>` (`float`).
	Real(f64),
	/// `<string>` (`str`).
	String(String),
	/// `<data>` (`bytes`).
	Data(Vec<u8>),
	/// `<date>` (`datetime`).
	Date(Date),
	/// `<array>` (`list`).
	Array(Vec<Value>),
	/// `<dict>` (`dict`).
	Dict(Dict),
}

impl Value {
	/// The dictionary, if this is one.
	pub fn as_dict(&self) -> Option<&Dict> {
		match self {
			Value::Dict(d) => Some(d),
			_ => None,
		}
	}

	/// The string, if this is one.
	pub fn as_str(&self) -> Option<&str> {
		match self {
			Value::String(s) => Some(s),
			_ => None,
		}
	}

	/// The number as Python compares it (`bool` is an `int`).
	fn as_number(&self) -> Option<Number> {
		match *self {
			Value::Bool(b) => Some(Number::Int(i128::from(b))),
			Value::Integer(i) => Some(Number::Int(i)),
			Value::Real(f) => Some(Number::Float(f)),
			_ => None,
		}
	}
}

#[derive(Clone, Copy)]
enum Number {
	Int(i128),
	Float(f64),
}

impl PartialEq for Number {
	fn eq(&self, other: &Self) -> bool {
		match (*self, *other) {
			(Number::Int(a), Number::Int(b)) => a == b,
			(Number::Float(a), Number::Float(b)) => a == b,
			(Number::Int(i), Number::Float(f)) | (Number::Float(f), Number::Int(i)) => {
				f.fract() == 0.0 && f.is_finite() && (i as f64) == f && i == f as i128
			}
		}
	}
}

/// Python's `==`: numbers compare by value across `bool`, `int` and `float`;
/// dictionaries compare regardless of order.
impl PartialEq for Value {
	fn eq(&self, other: &Self) -> bool {
		if let (Some(a), Some(b)) = (self.as_number(), other.as_number()) {
			return a == b;
		}
		match (self, other) {
			(Value::String(a), Value::String(b)) => a == b,
			(Value::Data(a), Value::Data(b)) => a == b,
			(Value::Date(a), Value::Date(b)) => a == b,
			(Value::Array(a), Value::Array(b)) => a == b,
			(Value::Dict(a), Value::Dict(b)) => a == b,
			_ => false,
		}
	}
}

impl From<bool> for Value {
	fn from(v: bool) -> Self {
		Value::Bool(v)
	}
}

impl From<i64> for Value {
	fn from(v: i64) -> Self {
		Value::Integer(i128::from(v))
	}
}

impl From<i32> for Value {
	fn from(v: i32) -> Self {
		Value::Integer(i128::from(v))
	}
}

impl From<f64> for Value {
	fn from(v: f64) -> Self {
		Value::Real(v)
	}
}

impl From<&str> for Value {
	fn from(v: &str) -> Self {
		Value::String(v.to_owned())
	}
}

impl From<String> for Value {
	fn from(v: String) -> Self {
		Value::String(v)
	}
}

impl From<Vec<Value>> for Value {
	fn from(v: Vec<Value>) -> Self {
		Value::Array(v)
	}
}

impl From<Dict> for Value {
	fn from(v: Dict) -> Self {
		Value::Dict(v)
	}
}

impl fmt::Display for Date {
	/// `_date_to_string`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
			self.year, self.month, self.day, self.hour, self.minute, self.second
		)
	}
}

fn days_in_month(year: u16, month: u8) -> u8 {
	match month {
		1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
		4 | 6 | 9 | 11 => 30,
		_ => {
			if (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400) {
				29
			} else {
				28
			}
		}
	}
}

/// `_date_from_string`: `YYYY[-MM[-DD[THH[:MM[:SS]]]]]Z` at the start of the
/// string, then `datetime(*fields)`, which needs at least the day.
pub fn date_from_string(s: &str) -> Result<Date> {
	let bytes = s.as_bytes();
	let mut pos = 0;
	let mut fields: Vec<u16> = Vec::new();
	let digits = |pos: usize, n: usize| -> Option<u16> {
		let part = bytes.get(pos..pos + n)?;
		if part.iter().all(u8::is_ascii_digit) {
			Some(
				part.iter()
					.fold(0u16, |acc, &d| acc * 10 + u16::from(d - b'0')),
			)
		} else {
			None
		}
	};
	let bad = || PlistError::Value(format!("Expected ISO 8601 date string, but got {s:?}."));
	fields.push(digits(0, 4).ok_or_else(bad)?);
	pos += 4;
	for sep in *b"--T::" {
		if bytes.get(pos) == Some(&sep)
			&& let Some(v) = digits(pos + 1, 2)
		{
			fields.push(v);
			pos += 3;
		} else {
			break;
		}
	}
	if bytes.get(pos) != Some(&b'Z') {
		return Err(bad());
	}
	if fields.len() < 3 {
		return Err(PlistError::Value(format!(
			"function missing required argument '{}'",
			["month", "day"][fields.len() - 1]
		)));
	}
	fields.resize(6, 0);
	let year = fields[0];
	let [month, day, hour, minute, second] =
		[fields[1], fields[2], fields[3], fields[4], fields[5]].map(|v| v as u8);
	if year == 0 {
		return Err(PlistError::Value(format!("year {year} is out of range")));
	}
	if !(1..=12).contains(&month) {
		return Err(PlistError::Value("month must be in 1..12".into()));
	}
	if day == 0 || day > days_in_month(year, month) {
		return Err(PlistError::Value("day is out of range for month".into()));
	}
	if hour > 23 {
		return Err(PlistError::Value("hour must be in 0..23".into()));
	}
	if minute > 59 {
		return Err(PlistError::Value("minute must be in 0..59".into()));
	}
	if second > 59 {
		return Err(PlistError::Value("second must be in 0..59".into()));
	}
	Ok(Date {
		year,
		month,
		day,
		hour,
		minute,
		second,
	})
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `base64.b64encode`.
fn b64encode(data: &[u8]) -> String {
	let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
	for chunk in data.chunks(3) {
		let b = [
			chunk[0],
			*chunk.get(1).unwrap_or(&0),
			*chunk.get(2).unwrap_or(&0),
		];
		let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
		for i in 0..4 {
			if i <= chunk.len() {
				out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
			} else {
				out.push('=');
			}
		}
	}
	out
}

/// `base64.b64decode` (`binascii.a2b_base64` in non-strict mode, as of Python
/// 3.13): characters outside the alphabet are skipped, and padding only matters
/// at the end, where complete padding excuses an unfinished group.
pub fn b64decode(text: &str) -> Result<Vec<u8>> {
	let mut out = Vec::new();
	let mut quad_pos = 0;
	let mut left: u32 = 0;
	let mut pads = 0;
	let mut padded = false;
	let mut data_chars = 0usize;
	for c in text.bytes() {
		if c == b'=' {
			if quad_pos >= 2 {
				pads += 1;
				if quad_pos + pads >= 4 {
					padded = true;
				}
			}
			continue;
		}
		let Some(v) = B64.iter().position(|&b| b == c) else {
			continue;
		};
		let v = v as u32;
		pads = 0;
		padded = false;
		data_chars += 1;
		match quad_pos {
			0 => {
				left = v;
				quad_pos = 1;
			}
			1 => {
				out.push(((left << 2) | (v >> 4)) as u8);
				left = v & 0x0f;
				quad_pos = 2;
			}
			2 => {
				out.push(((left << 4) | (v >> 2)) as u8);
				left = v & 0x03;
				quad_pos = 3;
			}
			_ => {
				out.push(((left << 6) | v) as u8);
				left = 0;
				quad_pos = 0;
			}
		}
	}
	match quad_pos {
		_ if padded => Ok(out),
		0 => Ok(out),
		1 => Err(PlistError::Base64(format!(
			"Invalid base64-encoded string: number of data characters ({data_chars}) cannot be 1 more than a multiple of 4"
		))),
		_ => Err(PlistError::Base64("Incorrect padding".into())),
	}
}

/// `_encode_base64`: base64 lines of at most `maxlinelength` characters
/// including the indentation, each on its own line.
fn encode_base64(data: &[u8], maxlinelength: Option<usize>, indent_level: usize) -> String {
	let encoded = b64encode(data);
	match maxlinelength {
		Some(maxlinelength) if !encoded.is_empty() && maxlinelength > 0 => {
			let indent = format!("\n{}", "  ".repeat(indent_level));
			let max_length = 16.max(maxlinelength.saturating_sub(indent.len()));
			let mut out = String::new();
			for chunk in encoded.as_bytes().chunks(max_length) {
				out.push_str(&indent);
				out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
			}
			out.push_str(&indent);
			out
		}
		_ => encoded,
	}
}

/// The options of [`totree`].
#[derive(Debug, Clone, Copy)]
pub struct TreeOptions {
	/// Whether keys of dictionaries are sorted (`sort_keys`).
	pub sort_keys: bool,
	/// Whether `<data>` is split into indented lines (`pretty_print`).
	pub pretty_print: bool,
	/// The indentation level of the value (`indent_level`).
	pub indent_level: usize,
}

impl Default for TreeOptions {
	fn default() -> Self {
		TreeOptions {
			sort_keys: true,
			pretty_print: true,
			indent_level: 1,
		}
	}
}

fn text_element(tag: &str, text: String) -> Element {
	let mut el = Element::new(tag);
	el.text = Some(text);
	el
}

fn make_element(value: &Value, ctx: &mut TreeOptions) -> Result<Element> {
	Ok(match value {
		Value::String(s) => text_element("string", s.clone()),
		Value::Bool(b) => Element::new(if *b { "true" } else { "false" }),
		Value::Integer(i) => {
			if -(1i128 << 63) <= *i && *i < (1i128 << 64) {
				text_element("integer", i.to_string())
			} else {
				return Err(PlistError::Overflow(i.to_string()));
			}
		}
		Value::Real(f) => text_element("real", py::float_repr(*f)),
		Value::Dict(d) => {
			let mut el = Element::new("dict");
			let mut items: Vec<(&String, &Value)> = d.iter().collect();
			if ctx.sort_keys {
				items.sort_by(|a, b| a.0.cmp(b.0));
			}
			ctx.indent_level += 1;
			for (key, value) in items {
				el.append(text_element("key", key.clone()));
				el.append(make_element(value, ctx)?);
			}
			ctx.indent_level -= 1;
			el
		}
		Value::Array(a) => {
			let mut el = Element::new("array");
			if a.is_empty() {
				return Ok(el);
			}
			ctx.indent_level += 1;
			for value in a {
				el.append(make_element(value, ctx)?);
			}
			ctx.indent_level -= 1;
			el
		}
		Value::Date(d) => text_element("date", d.to_string()),
		Value::Data(data) => text_element(
			"data",
			encode_base64(
				data,
				if ctx.pretty_print { Some(76) } else { None },
				ctx.indent_level,
			),
		),
	})
}

/// `totree`: converts a value to an XML element.
pub fn totree(value: &Value, options: TreeOptions) -> Result<Element> {
	let mut ctx = options;
	make_element(value, &mut ctx)
}

/// `PlistTarget`: builds a value from start/end/data events.
struct PlistTarget {
	stack: Vec<Value>,
	/// For each open container, the key it is stored under in its parent.
	keys: Vec<Option<String>>,
	current_key: Option<String>,
	root: Option<Value>,
	data: Vec<String>,
}

impl PlistTarget {
	fn start(&mut self, tag: &str) -> Result<()> {
		self.data.clear();
		match tag {
			"dict" => self.start_container(Value::Dict(Dict::new())),
			"array" => self.start_container(Value::Array(Vec::new())),
			_ => Ok(()),
		}
	}

	/// `add_object` for a container: it is filled while it is on the stack, then
	/// stored into its parent when it ends.
	fn start_container(&mut self, value: Value) -> Result<()> {
		let key = if self.current_key.is_some() {
			match self.stack.last() {
				Some(Value::Dict(_)) => {}
				Some(top) => return Err(unexpected(top)),
				None => return Err(PlistError::Value("unexpected element".into())),
			}
			self.current_key.take()
		} else {
			if let Some(top) = self.stack.last()
				&& !matches!(top, Value::Array(_))
			{
				return Err(unexpected(top));
			}
			None
		};
		self.stack.push(value);
		self.keys.push(key);
		Ok(())
	}

	fn end(&mut self, tag: &str) -> Result<()> {
		match tag {
			"dict" => {
				if let Some(key) = &self.current_key
					&& !key.is_empty()
				{
					return Err(PlistError::Value(format!("missing value for key '{key}'")));
				}
				self.end_container()
			}
			"array" => self.end_container(),
			"key" => {
				let is_dict = matches!(self.stack.last(), Some(Value::Dict(_)));
				if self.current_key.as_ref().is_some_and(|k| !k.is_empty()) || !is_dict {
					return Err(PlistError::Value("unexpected key".into()));
				}
				self.current_key = Some(self.get_data());
				Ok(())
			}
			"true" => self.add_object(Value::Bool(true)),
			"false" => self.add_object(Value::Bool(false)),
			"integer" => {
				let data = self.get_data();
				let v = py::parse_int(&data).ok_or_else(|| {
					PlistError::Value(format!(
						"invalid literal for int() with base 10: {}",
						py::str_repr(&data)
					))
				})?;
				self.add_object(Value::Integer(v))
			}
			"real" => {
				let data = self.get_data();
				let v = py::parse_float(&data).ok_or_else(|| {
					PlistError::Value(format!(
						"could not convert string to float: {}",
						py::str_repr(&data)
					))
				})?;
				self.add_object(Value::Real(v))
			}
			"string" => {
				let data = self.get_data();
				self.add_object(Value::String(data))
			}
			"data" => {
				let data = self.get_data();
				self.add_object(Value::Data(b64decode(&data)?))
			}
			"date" => {
				let data = self.get_data();
				self.add_object(Value::Date(date_from_string(&data)?))
			}
			_ => Ok(()),
		}
	}

	fn end_container(&mut self) -> Result<()> {
		// The stack can't be empty: every end matches a start.
		let (Some(value), Some(key)) = (self.stack.pop(), self.keys.pop()) else {
			return Err(PlistError::Value("pop from empty list".into()));
		};
		match (self.stack.last_mut(), key) {
			(Some(Value::Dict(d)), Some(key)) => {
				d.insert(key, value);
			}
			(Some(Value::Array(a)), None) => a.push(value),
			(None, _) => self.root = Some(value),
			_ => return Err(PlistError::Value("unexpected element".into())),
		}
		Ok(())
	}

	fn add_object(&mut self, value: Value) -> Result<()> {
		if self.current_key.is_some() {
			match self.stack.last_mut() {
				Some(Value::Dict(d)) => {
					let key = self.current_key.take().unwrap_or_default();
					d.insert(key, value);
				}
				Some(top) => return Err(unexpected(top)),
				None => return Err(PlistError::Value("unexpected element".into())),
			}
		} else if self.stack.is_empty() {
			self.root = Some(value);
		} else {
			match self.stack.last_mut() {
				Some(Value::Array(a)) => a.push(value),
				Some(top) => return Err(unexpected(top)),
				None => {}
			}
		}
		Ok(())
	}

	fn get_data(&mut self) -> String {
		let data = self.data.concat();
		self.data.clear();
		data
	}
}

fn unexpected(top: &Value) -> PlistError {
	PlistError::Value(format!("unexpected element: {top:?}"))
}

/// The text of an element before its first child (`element.text`).
pub fn element_text(element: &xml_reader::Element) -> Option<String> {
	let mut text: Option<String> = None;
	for content in &element.content {
		match content {
			Content::Text(t) => text.get_or_insert_with(String::new).push_str(t),
			Content::Element(_) => break,
		}
	}
	text
}

fn walk(target: &mut PlistTarget, element: &xml_reader::Element) -> Result<()> {
	target.start(&element.name)?;
	let mut has_children = false;
	for child in element.elements() {
		has_children = true;
		walk(target, child)?;
	}
	if !has_children {
		target.data.push(element_text(element).unwrap_or_default());
	}
	target.end(&element.name)
}

/// `fromtree`: converts an XML element to a value.
pub fn fromtree(tree: &xml_reader::Element) -> Result<Value> {
	let mut target = PlistTarget {
		stack: Vec::new(),
		keys: Vec::new(),
		current_key: None,
		root: None,
		data: Vec::new(),
	};
	walk(&mut target, tree)?;
	target
		.root
		.ok_or_else(|| PlistError::Value("No root set.".into()))
}

/// `loads`: reads a plist document.
pub fn loads(text: &str) -> Result<Value> {
	let root = xml_reader::parse(text.strip_prefix('\u{feff}').unwrap_or(text))
		.map_err(|e| PlistError::Xml(e.0))?;
	fromtree(&root)
}

/// `loads` of bytes: the document is decoded as an XML parser does it (see
/// [`decode_xml`]).
pub fn loads_bytes(data: &[u8]) -> Result<Value> {
	loads(&decode_xml(data)?)
}

fn decode_utf16(data: &[u8], big_endian: bool) -> Result<String> {
	let (pairs, rest) = data.as_chunks::<2>();
	if !rest.is_empty() {
		return Err(PlistError::Xml("truncated UTF-16 data".into()));
	}
	let units = pairs.iter().map(|&c| {
		if big_endian {
			u16::from_be_bytes(c)
		} else {
			u16::from_le_bytes(c)
		}
	});
	char::decode_utf16(units)
		.collect::<std::result::Result<String, _>>()
		.map_err(|e| PlistError::Xml(e.to_string()))
}

/// The `encoding` of an XML declaration at the start of ASCII-compatible
/// data.
fn declared_encoding(data: &[u8]) -> Option<String> {
	let head = data.get(..data.len().min(200))?;
	if !head.starts_with(b"<?xml") {
		return None;
	}
	let end = head.windows(2).position(|w| w == b"?>")?;
	let decl = std::str::from_utf8(&head[..end]).ok()?;
	let rest = &decl[decl.find("encoding")? + "encoding".len()..];
	let rest = rest.trim_start().strip_prefix('=')?.trim_start();
	let quote = rest.chars().next()?;
	let value = rest.get(1..)?;
	Some(value[..value.find(quote)?].to_ascii_lowercase())
}

/// Decodes an XML document the way an XML parser does: a UTF-8 or UTF-16
/// byte order mark, otherwise the encoding of the XML declaration (UTF-8,
/// UTF-16, ASCII or Latin-1), and UTF-8 by default.
pub fn decode_xml(data: &[u8]) -> Result<String> {
	if let Some(rest) = data.strip_prefix(b"\xef\xbb\xbf") {
		return String::from_utf8(rest.to_vec()).map_err(|e| PlistError::Xml(e.to_string()));
	}
	if let Some(rest) = data.strip_prefix(b"\xff\xfe") {
		return decode_utf16(rest, false);
	}
	if let Some(rest) = data.strip_prefix(b"\xfe\xff") {
		return decode_utf16(rest, true);
	}
	if data.starts_with(b"<\0?\0") {
		return decode_utf16(data, false);
	}
	if data.starts_with(b"\0<\0?") {
		return decode_utf16(data, true);
	}
	match declared_encoding(data).as_deref() {
		Some("latin-1" | "latin1" | "iso-8859-1" | "iso8859-1") => Ok(py::latin1_decode(data)),
		_ => String::from_utf8(data.to_vec()).map_err(|e| PlistError::Xml(e.to_string())),
	}
}

/// `dumps`: writes a plist document.
pub fn dumps(value: &Value, sort_keys: bool, pretty_print: bool) -> Result<String> {
	let mut root = Element::new("plist");
	root.set("version", "1.0");
	root.append(totree(
		value,
		TreeOptions {
			sort_keys,
			pretty_print,
			indent_level: 1,
		},
	)?);
	let mut out = String::from("<?xml version='1.0' encoding='UTF-8'?>");
	if pretty_print {
		out.push('\n');
	}
	out.push_str(
		"<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">",
	);
	if pretty_print {
		out.push('\n');
	}
	out.push_str(&etree::tostring(&root, pretty_print, false).map_err(|e| PlistError::Xml(e.0))?);
	Ok(out)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn dict(items: Vec<(&str, Value)>) -> Value {
		Value::Dict(items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
	}

	#[test]
	fn target_doctest() -> Result<()> {
		let v = loads("<dict>    <key>something</key>    <string>blah</string></dict>")?;
		assert_eq!(v, dict(vec![("something", "blah".into())]));
		Ok(())
	}

	#[test]
	fn roundtrip() -> Result<()> {
		let value = dict(vec![
			("b", Value::Array(vec![1.into(), 2.5.into(), true.into()])),
			("a", Value::Data(b"<binary gunk>".to_vec())),
			("c", Value::Date(date_from_string("2020-01-02T03:04:05Z")?)),
			("d", dict(vec![("x", Value::Array(vec![]))])),
		]);
		let text = dumps(&value, true, true)?;
		// Checked against fontTools.misc.plistlib.dumps.
		assert_eq!(
			text,
			"<?xml version='1.0' encoding='UTF-8'?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n  <dict>\n    <key>a</key>\n    <data>\n    PGJpbmFyeSBndW5rPg==\n    </data>\n    <key>b</key>\n    <array>\n      <integer>1</integer>\n      <real>2.5</real>\n      <true/>\n    </array>\n    <key>c</key>\n    <date>2020-01-02T03:04:05Z</date>\n    <key>d</key>\n    <dict>\n      <key>x</key>\n      <array/>\n    </dict>\n  </dict>\n</plist>\n"
		);
		assert_eq!(loads(&text)?, value);
		Ok(())
	}

	#[test]
	fn numbers_compare_like_python() {
		assert_eq!(Value::Bool(true), Value::Integer(1));
		assert_eq!(Value::Integer(2), Value::Real(2.0));
		assert_ne!(Value::Integer(2), Value::String("2".into()));
	}

	#[test]
	fn base64_decode() -> Result<()> {
		assert_eq!(
			b64decode("\n  PGJp\n  bmFyeSBndW5rPg==\n  ")?,
			b"<binary gunk>"
		);
		assert!(b64decode("YQ").is_err());
		// Checked against binascii.a2b_base64 of Python 3.13.
		assert_eq!(b64decode("YQ==YQ==")?, b"a\x06\x10");
		assert_eq!(b64decode("YWI=Y")?, b"ab\x18");
		assert_eq!(b64decode("YQ===")?, b"a");
		assert!(b64decode("YQ==Y").is_err());
		assert!(b64decode("YQ=a").is_err());
		assert!(b64decode("YQ==YWI").is_err());
		Ok(())
	}
}
