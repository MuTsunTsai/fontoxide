//! Port of `fontTools.ufoLib.utils`: format versions, plus the Python number
//! and value helpers that ufoLib relies on.
//!
//! The `deprecated` decorator has no Rust counterpart.

use std::fmt;

use fontoxide_misc::plistlib::Value;
use fontoxide_misc::py;

/// A Python `int` or `float`, as read from a GLIF file or given to the GLIF
/// point pen: the difference shows when the number is written back
/// (`repr(1)` is `1`, `repr(1.0)` is `1.0`).
///
/// Equality is by value, so `Int(1) == Float(1.0)`.
#[derive(Debug, Clone, Copy)]
pub enum Number {
	/// An `int`.
	Int(i64),
	/// A `float`.
	Float(f64),
}

impl Number {
	/// `float(value)`.
	pub fn to_f64(self) -> f64 {
		match self {
			Number::Int(i) => i as f64,
			Number::Float(f) => f,
		}
	}

	/// `repr(value)`.
	pub fn repr(self) -> String {
		match self {
			Number::Int(i) => i.to_string(),
			Number::Float(f) => py::float_repr(f),
		}
	}

	/// The number as a plist value (`int` or `float`).
	pub fn to_value(self) -> Value {
		match self {
			Number::Int(i) => Value::Integer(i128::from(i)),
			Number::Float(f) => Value::Real(f),
		}
	}

	/// A plist value that is a Python number (`isinstance(value, (int,
	/// float))`, where `bool` is an `int`).
	pub fn from_value(value: &Value) -> Option<Number> {
		match *value {
			Value::Bool(b) => Some(Number::Int(i64::from(b))),
			Value::Integer(i) => Some(match i64::try_from(i) {
				Ok(i) => Number::Int(i),
				// LENIENT(L35): Python ints are unbounded; larger ones become floats.
				Err(_) => Number::Float(i as f64),
			}),
			Value::Real(f) => Some(Number::Float(f)),
			_ => None,
		}
	}

	/// A coordinate given as `f64`: integral values are taken as `int`s, as
	/// fonts mostly store them.
	pub fn from_f64(value: f64) -> Number {
		if value.fract() == 0.0
			&& value.abs() < 9.0e15
			&& !(value == 0.0 && value.is_sign_negative())
		{
			Number::Int(value as i64)
		} else {
			Number::Float(value)
		}
	}
}

impl PartialEq for Number {
	fn eq(&self, other: &Self) -> bool {
		match (*self, *other) {
			(Number::Int(a), Number::Int(b)) => a == b,
			(a, b) => a.to_f64() == b.to_f64(),
		}
	}
}

impl PartialOrd for Number {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		match (*self, *other) {
			(Number::Int(a), Number::Int(b)) => a.partial_cmp(&b),
			(a, b) => a.to_f64().partial_cmp(&b.to_f64()),
		}
	}
}

impl fmt::Display for Number {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.repr())
	}
}

impl From<i64> for Number {
	fn from(v: i64) -> Self {
		Number::Int(v)
	}
}

impl From<i32> for Number {
	fn from(v: i32) -> Self {
		Number::Int(i64::from(v))
	}
}

impl From<f64> for Number {
	fn from(v: f64) -> Self {
		Number::Float(v)
	}
}

/// `isinstance(value, int)` (`bool` is an `int`).
pub(crate) fn is_int(value: &Value) -> bool {
	matches!(value, Value::Bool(_) | Value::Integer(_))
}

/// `isinstance(value, numberTypes)`.
pub(crate) fn is_number(value: &Value) -> bool {
	matches!(value, Value::Bool(_) | Value::Integer(_) | Value::Real(_))
}

/// `bool(value)`.
pub(crate) fn truthy(value: &Value) -> bool {
	match value {
		Value::Bool(b) => *b,
		Value::Integer(i) => *i != 0,
		Value::Real(f) => *f != 0.0,
		Value::String(s) => !s.is_empty(),
		Value::Data(d) => !d.is_empty(),
		Value::Date(_) => true,
		Value::Array(a) => !a.is_empty(),
		Value::Dict(d) => !d.is_empty(),
	}
}

/// `type(value).__name__`.
pub(crate) fn type_name(value: &Value) -> &'static str {
	match value {
		Value::Bool(_) => "bool",
		Value::Integer(_) => "int",
		Value::Real(_) => "float",
		Value::String(_) => "str",
		Value::Data(_) => "bytes",
		Value::Date(_) => "datetime",
		Value::Array(_) => "list",
		Value::Dict(_) => "dict",
	}
}

/// `repr(value)` of a plist value, as Python prints it.
pub fn value_repr(value: &Value) -> String {
	match value {
		Value::Bool(b) => if *b { "True" } else { "False" }.to_owned(),
		Value::Integer(i) => i.to_string(),
		Value::Real(f) => py::float_repr(*f),
		Value::String(s) => py::str_repr(s),
		Value::Data(d) => py::bytes_repr(d),
		Value::Date(d) => {
			format!(
				"datetime.datetime({}, {}, {}, {}, {})",
				d.year, d.month, d.day, d.hour, d.minute
			) + &if d.second != 0 {
				format!(", {}", d.second)
			} else {
				String::new()
			}
		}
		Value::Array(a) => format!(
			"[{}]",
			a.iter().map(value_repr).collect::<Vec<_>>().join(", ")
		),
		Value::Dict(d) => format!(
			"{{{}}}",
			d.iter()
				.map(|(k, v)| format!("{}: {}", py::str_repr(k), value_repr(v)))
				.collect::<Vec<_>>()
				.join(", ")
		),
	}
}

/// `str(value)` of a plist value: like [`value_repr`], but strings are not
/// quoted.
pub(crate) fn value_str(value: &Value) -> String {
	match value {
		Value::String(s) => s.clone(),
		other => value_repr(other),
	}
}

/// Python's `int(s, 16)`.
pub(crate) fn parse_hex(s: &str) -> Option<i128> {
	let t = py::strip(s);
	let (negative, t) = match t.as_bytes().first() {
		Some(b'-') => (true, &t[1..]),
		Some(b'+') => (false, &t[1..]),
		_ => (false, t),
	};
	let t = t
		.strip_prefix("0x")
		.or_else(|| t.strip_prefix("0X"))
		.map(|rest| rest.strip_prefix('_').unwrap_or(rest))
		.unwrap_or(t);
	if t.is_empty() || t.starts_with('_') || t.ends_with('_') || t.contains("__") {
		return None;
	}
	let digits: String = t.chars().filter(|&c| c != '_').collect();
	if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
		return None;
	}
	let v = i128::from_str_radix(&digits, 16).ok()?;
	Some(if negative { -v } else { v })
}

/// Python's `int(s)` of a string.
pub(crate) fn parse_int(s: &str) -> Option<i128> {
	py::parse_int(py::strip(s))
}

/// Python's `float(s)` of a string.
pub(crate) fn parse_float(s: &str) -> Option<f64> {
	py::parse_float(py::strip(s))
}

/// A requested format version: `(major, minor)`, or a major version alone
/// with an implied minor version of 0 (`FormatVersionInput` without `None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FormatVersionInput {
	/// The major version.
	pub major: i64,
	/// The minor version.
	pub minor: i64,
}

impl From<i64> for FormatVersionInput {
	fn from(major: i64) -> Self {
		FormatVersionInput { major, minor: 0 }
	}
}

impl From<i32> for FormatVersionInput {
	fn from(major: i32) -> Self {
		FormatVersionInput {
			major: i64::from(major),
			minor: 0,
		}
	}
}

impl From<(i64, i64)> for FormatVersionInput {
	fn from((major, minor): (i64, i64)) -> Self {
		FormatVersionInput { major, minor }
	}
}

impl From<(i32, i32)> for FormatVersionInput {
	fn from((major, minor): (i32, i32)) -> Self {
		FormatVersionInput {
			major: i64::from(major),
			minor: i64::from(minor),
		}
	}
}

impl fmt::Display for FormatVersionInput {
	/// `repr` of the tuple, as in the error messages.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "({}, {})", self.major, self.minor)
	}
}

/// The UFO format versions (`UFOFormatVersion`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UfoFormatVersion {
	/// `FORMAT_1_0`.
	Format1_0,
	/// `FORMAT_2_0`.
	Format2_0,
	/// `FORMAT_3_0`.
	Format3_0,
}

/// The GLIF format versions (`GLIFFormatVersion`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GlifFormatVersion {
	/// `FORMAT_1_0`.
	Format1_0,
	/// `FORMAT_2_0`.
	Format2_0,
}

impl UfoFormatVersion {
	/// All versions (`__members__`).
	pub const ALL: [UfoFormatVersion; 3] = [
		UfoFormatVersion::Format1_0,
		UfoFormatVersion::Format2_0,
		UfoFormatVersion::Format3_0,
	];

	/// `major`.
	pub fn major(self) -> i64 {
		match self {
			UfoFormatVersion::Format1_0 => 1,
			UfoFormatVersion::Format2_0 => 2,
			UfoFormatVersion::Format3_0 => 3,
		}
	}

	/// `minor`.
	pub fn minor(self) -> i64 {
		0
	}

	/// `(major, minor)`.
	pub fn as_tuple(self) -> (i64, i64) {
		(self.major(), self.minor())
	}

	/// `default()`: the latest version.
	pub fn default_version() -> Self {
		UfoFormatVersion::Format3_0
	}

	/// `UFOFormatVersion(value)`: `None` if the version is not defined
	/// (`ValueError`).
	pub fn new(value: impl Into<FormatVersionInput>) -> Option<Self> {
		let v = value.into();
		Self::ALL
			.into_iter()
			.find(|f| f.major() == v.major && f.minor() == v.minor)
	}

	/// `normalizeFormatVersion(value, UFOFormatVersion)`: the default version
	/// for `None`.
	pub fn normalize(value: Option<FormatVersionInput>) -> Option<Self> {
		match value {
			None => Some(Self::default_version()),
			Some(v) => Self::new(v),
		}
	}
}

impl GlifFormatVersion {
	/// All versions (`__members__`).
	pub const ALL: [GlifFormatVersion; 2] =
		[GlifFormatVersion::Format1_0, GlifFormatVersion::Format2_0];

	/// `major`.
	pub fn major(self) -> i64 {
		match self {
			GlifFormatVersion::Format1_0 => 1,
			GlifFormatVersion::Format2_0 => 2,
		}
	}

	/// `minor`.
	pub fn minor(self) -> i64 {
		0
	}

	/// `(major, minor)`.
	pub fn as_tuple(self) -> (i64, i64) {
		(self.major(), self.minor())
	}

	/// `GLIFFormatVersion(value)`: `None` if the version is not defined.
	pub fn new(value: impl Into<FormatVersionInput>) -> Option<Self> {
		let v = value.into();
		Self::ALL
			.into_iter()
			.find(|f| f.major() == v.major && f.minor() == v.minor)
	}

	/// `normalizeFormatVersion(value, GLIFFormatVersion)`.
	pub fn normalize(value: Option<FormatVersionInput>) -> Option<Self> {
		match value {
			None => Some(Self::default_for(None)),
			Some(v) => Self::new(v),
		}
	}

	/// `default(ufoFormatVersion)`: the latest GLIF version, or the latest one
	/// that the UFO version supports.
	pub fn default_for(ufo_format_version: Option<UfoFormatVersion>) -> Self {
		Self::supported_versions(ufo_format_version)
			.into_iter()
			.max()
			.unwrap_or(GlifFormatVersion::Format1_0)
	}

	/// `supported_versions(ufoFormatVersion)`: all versions, or the versions
	/// that the UFO version supports.
	pub fn supported_versions(ufo_format_version: Option<UfoFormatVersion>) -> Vec<Self> {
		match ufo_format_version {
			None => Self::ALL.to_vec(),
			Some(v) if v >= UfoFormatVersion::Format3_0 => Self::ALL.to_vec(),
			Some(_) => vec![GlifFormatVersion::Format1_0],
		}
	}
}

impl fmt::Display for UfoFormatVersion {
	/// `str(version)`: `"3.0"`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}.{}", self.major(), self.minor())
	}
}

impl fmt::Display for GlifFormatVersion {
	/// `str(version)`: `"2.0"`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}.{}", self.major(), self.minor())
	}
}

impl From<UfoFormatVersion> for FormatVersionInput {
	fn from(v: UfoFormatVersion) -> Self {
		FormatVersionInput {
			major: v.major(),
			minor: v.minor(),
		}
	}
}

impl From<GlifFormatVersion> for FormatVersionInput {
	fn from(v: GlifFormatVersion) -> Self {
		FormatVersionInput {
			major: v.major(),
			minor: v.minor(),
		}
	}
}
