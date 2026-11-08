//! CFF DICT data: operators, operand values, and their encoding
//! (`cffLib` operator tables, `psCharStrings.DictDecompiler`,
//! `cffLib.DictCompiler`).

use fontoxide_misc::py::{float_repr, format_g, parse_float};

use super::charstring::{encode_int_cff, index_error};
use crate::{Error, Result};

/// An operand or a value of a DICT entry, as fontTools holds them.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
	/// An `int`.
	Int(i64),
	/// A `float`.
	Float(f64),
	/// A string (from a SID, or a converted value).
	Str(String),
	/// A list: an array, a delta array, or a blend list (default value, then
	/// the deltas).
	List(Vec<Value>),
	/// A tuple: `(size, offset)` of `Private`, `(registry, order, supplement)` of
	/// `ROS`.
	Tuple(Vec<Value>),
}

impl Value {
	/// Python equality (`==`), where `1 == 1.0`.
	pub fn py_eq(&self, other: &Value) -> bool {
		match (self, other) {
			(Value::List(a), Value::List(b)) | (Value::Tuple(a), Value::Tuple(b)) => {
				a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.py_eq(y))
			}
			(Value::Str(a), Value::Str(b)) => a == b,
			_ => match (self.number(), other.number()) {
				(Some(a), Some(b)) => a == b,
				_ => false,
			},
		}
	}

	/// The value of a number.
	pub fn number(&self) -> Option<f64> {
		match *self {
			Value::Int(i) => Some(i as f64),
			Value::Float(f) => Some(f),
			_ => None,
		}
	}

	/// The integer of an `int`.
	pub fn int(&self) -> Result<i64> {
		match *self {
			Value::Int(i) => Ok(i),
			_ => Err(Error::ttlib(format!(
				"TypeError: expected an int, got {self:?}"
			))),
		}
	}

	/// `str(value)`.
	pub fn repr(&self) -> String {
		match self {
			Value::Int(i) => i.to_string(),
			Value::Float(f) => float_repr(*f),
			Value::Str(s) => s.clone(),
			Value::List(items) => format!(
				"[{}]",
				items
					.iter()
					.map(Value::py_repr)
					.collect::<Vec<_>>()
					.join(", ")
			),
			Value::Tuple(items) => format!(
				"({}{})",
				items
					.iter()
					.map(Value::py_repr)
					.collect::<Vec<_>>()
					.join(", "),
				if items.len() == 1 { "," } else { "" }
			),
		}
	}

	/// `repr(value)`, as inside a list.
	fn py_repr(&self) -> String {
		match self {
			Value::Str(s) => fontoxide_misc::py::str_repr(s),
			v => v.repr(),
		}
	}

	fn sub(&self, other: &Value) -> Value {
		match (self, other) {
			(Value::Int(a), Value::Int(b)) => Value::Int(a - b),
			(a, b) => Value::Float(a.number().unwrap_or(0.0) - b.number().unwrap_or(0.0)),
		}
	}

	fn add(&self, other: &Value) -> Value {
		match (self, other) {
			(Value::Int(a), Value::Int(b)) => Value::Int(a + b),
			(a, b) => Value::Float(a.number().unwrap_or(0.0) + b.number().unwrap_or(0.0)),
		}
	}
}

/// The type of a DICT entry's operands (`argument type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arg {
	/// `number`.
	Number,
	/// `SID`.
	Sid,
	/// `array`.
	Array,
	/// `delta`.
	Delta,
	/// `blendList`.
	BlendList,
	/// `("number", "number")` (`Private`).
	NumberPair,
	/// `("SID", "SID", "number")` (`ROS`).
	Ros,
}

/// How a DICT entry is converted (`converter`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conv {
	/// `NumberConverter`.
	Number,
	/// `ArrayConverter`.
	Array,
	/// `ASCIIConverter`.
	Ascii,
	/// `Latin1Converter`.
	Latin1,
	/// `ROSConverter`.
	Ros,
	/// `CharsetConverter`.
	Charset,
	/// `EncodingConverter`.
	Encoding,
	/// `PrivateDictConverter`.
	Private,
	/// `FDSelectConverter`.
	FdSelect,
	/// `FDArrayConverter`.
	FdArray,
	/// `CharStringsConverter`.
	CharStrings,
	/// `VarStoreConverter`.
	VarStore,
	/// `SubrsConverter`.
	Subrs,
	/// None (`blend`).
	None,
}

/// An operator: opcode (12 x for two-byte ones), name, operands, default and
/// converter.
#[derive(Debug)]
pub struct Operator {
	/// The opcode.
	pub code: u16,
	/// The name.
	pub name: &'static str,
	/// The operands.
	pub arg: Arg,
	/// The default value.
	pub default: Option<fn() -> Value>,
	/// The converter.
	pub conv: Conv,
}

const fn op(
	code: u16,
	name: &'static str,
	arg: Arg,
	default: Option<fn() -> Value>,
	conv: Conv,
) -> Operator {
	Operator {
		code,
		name,
		arg,
		default,
		conv,
	}
}

const fn esc(b: u16) -> u16 {
	12 << 8 | b
}

fn int0() -> Value {
	Value::Int(0)
}
fn int1() -> Value {
	Value::Int(1)
}
fn int2() -> Value {
	Value::Int(2)
}
fn int7() -> Value {
	Value::Int(7)
}
fn int50() -> Value {
	Value::Int(50)
}
fn int_minus_100() -> Value {
	Value::Int(-100)
}
fn int8720() -> Value {
	Value::Int(8720)
}
fn font_matrix() -> Value {
	Value::List(vec![
		Value::Float(0.001),
		Value::Int(0),
		Value::Int(0),
		Value::Float(0.001),
		Value::Int(0),
		Value::Int(0),
	])
}
fn font_bbox() -> Value {
	Value::List(vec![
		Value::Int(0),
		Value::Int(0),
		Value::Int(0),
		Value::Int(0),
	])
}
fn blue_scale() -> Value {
	Value::Float(0.039625)
}
fn expansion_factor() -> Value {
	Value::Float(0.06)
}

use Arg as A;
use Conv as C;

/// `topDictOperators`.
pub const TOP_DICT: &[Operator] = &[
	op(25, "maxstack", A::Number, None, C::Number),
	op(esc(30), "ROS", A::Ros, None, C::Ros),
	op(esc(20), "SyntheticBase", A::Number, None, C::Number),
	op(0, "version", A::Sid, None, C::Ascii),
	op(1, "Notice", A::Sid, None, C::Latin1),
	op(esc(0), "Copyright", A::Sid, None, C::Latin1),
	op(2, "FullName", A::Sid, None, C::Latin1),
	op(esc(38), "FontName", A::Sid, None, C::Latin1),
	op(3, "FamilyName", A::Sid, None, C::Latin1),
	op(4, "Weight", A::Sid, None, C::Ascii),
	op(esc(1), "isFixedPitch", A::Number, Some(int0), C::Number),
	op(esc(2), "ItalicAngle", A::Number, Some(int0), C::Number),
	op(
		esc(3),
		"UnderlinePosition",
		A::Number,
		Some(int_minus_100),
		C::Number,
	),
	op(
		esc(4),
		"UnderlineThickness",
		A::Number,
		Some(int50),
		C::Number,
	),
	op(esc(5), "PaintType", A::Number, Some(int0), C::Number),
	op(esc(6), "CharstringType", A::Number, Some(int2), C::Number),
	op(esc(7), "FontMatrix", A::Array, Some(font_matrix), C::Array),
	op(13, "UniqueID", A::Number, None, C::Number),
	op(5, "FontBBox", A::Array, Some(font_bbox), C::Array),
	op(esc(8), "StrokeWidth", A::Number, Some(int0), C::Number),
	op(14, "XUID", A::Array, None, C::Array),
	op(esc(21), "PostScript", A::Sid, None, C::Ascii),
	op(esc(22), "BaseFontName", A::Sid, None, C::Ascii),
	op(esc(23), "BaseFontBlend", A::Delta, None, C::Array),
	op(esc(31), "CIDFontVersion", A::Number, Some(int0), C::Number),
	op(esc(32), "CIDFontRevision", A::Number, Some(int0), C::Number),
	op(esc(33), "CIDFontType", A::Number, Some(int0), C::Number),
	op(esc(34), "CIDCount", A::Number, Some(int8720), C::Number),
	op(15, "charset", A::Number, None, C::Charset),
	op(esc(35), "UIDBase", A::Number, None, C::Number),
	op(16, "Encoding", A::Number, Some(int0), C::Encoding),
	op(18, "Private", A::NumberPair, None, C::Private),
	op(esc(37), "FDSelect", A::Number, None, C::FdSelect),
	op(esc(36), "FDArray", A::Number, None, C::FdArray),
	op(17, "CharStrings", A::Number, None, C::CharStrings),
	op(24, "VarStore", A::Number, None, C::VarStore),
];

/// The names of `topDictOperators2`, the CFF2 Top DICT.
pub const TOP_DICT2: &[&str] = &[
	"maxstack",
	"FontMatrix",
	"FDSelect",
	"FDArray",
	"CharStrings",
	"VarStore",
];

/// `privateDictOperators`.
pub const PRIVATE_DICT: &[Operator] = &[
	op(22, "vsindex", A::Number, None, C::Number),
	op(23, "blend", A::BlendList, None, C::None),
	op(6, "BlueValues", A::Delta, None, C::Array),
	op(7, "OtherBlues", A::Delta, None, C::Array),
	op(8, "FamilyBlues", A::Delta, None, C::Array),
	op(9, "FamilyOtherBlues", A::Delta, None, C::Array),
	op(esc(9), "BlueScale", A::Number, Some(blue_scale), C::Number),
	op(esc(10), "BlueShift", A::Number, Some(int7), C::Number),
	op(esc(11), "BlueFuzz", A::Number, Some(int1), C::Number),
	op(10, "StdHW", A::Number, None, C::Number),
	op(11, "StdVW", A::Number, None, C::Number),
	op(esc(12), "StemSnapH", A::Delta, None, C::Array),
	op(esc(13), "StemSnapV", A::Delta, None, C::Array),
	op(esc(14), "ForceBold", A::Number, Some(int0), C::Number),
	op(esc(15), "ForceBoldThreshold", A::Number, None, C::Number),
	op(esc(16), "lenIV", A::Number, None, C::Number),
	op(esc(17), "LanguageGroup", A::Number, Some(int0), C::Number),
	op(
		esc(18),
		"ExpansionFactor",
		A::Number,
		Some(expansion_factor),
		C::Number,
	),
	op(
		esc(19),
		"initialRandomSeed",
		A::Number,
		Some(int0),
		C::Number,
	),
	op(20, "defaultWidthX", A::Number, Some(int0), C::Number),
	op(21, "nominalWidthX", A::Number, Some(int0), C::Number),
	op(19, "Subrs", A::Number, None, C::Subrs),
];

/// The names of `privateDictOperators2`, the CFF2 Private DICT.
pub const PRIVATE_DICT2: &[&str] = &[
	"vsindex",
	"blend",
	"BlueValues",
	"OtherBlues",
	"FamilyBlues",
	"FamilyOtherBlues",
	"BlueScale",
	"BlueShift",
	"BlueFuzz",
	"StdHW",
	"StdVW",
	"StemSnapH",
	"StemSnapV",
	"LanguageGroup",
	"ExpansionFactor",
	"Subrs",
];

/// The operator of a table by name.
pub fn operator(table: &'static [Operator], name: &str) -> Option<&'static Operator> {
	table.iter().find(|o| o.name == name)
}

fn operator_by_code(table: &'static [Operator], code: u16) -> Option<&'static Operator> {
	table.iter().find(|o| o.code == code)
}

/// The real number nibbles (`realNibbles`); `None` for the unused 0xD.
const REAL_NIBBLES: [Option<&str>; 15] = [
	Some("0"),
	Some("1"),
	Some("2"),
	Some("3"),
	Some("4"),
	Some("5"),
	Some("6"),
	Some("7"),
	Some("8"),
	Some("9"),
	Some("."),
	Some("E"),
	Some("E-"),
	None,
	Some("-"),
];

/// `read_realNumber`.
fn read_real(data: &[u8], mut index: usize) -> Result<(f64, usize)> {
	let mut number = String::new();
	loop {
		let b = *data.get(index).ok_or_else(index_error)?;
		index += 1;
		for nibble in [b >> 4, b & 0x0F] {
			if nibble == 0xF {
				let v = parse_float(&number).ok_or_else(|| {
					Error::value(format!("could not convert string to float: {number:?}"))
				})?;
				return Ok((v, index));
			}
			let s = REAL_NIBBLES[nibble as usize].ok_or_else(|| {
				Error::ttlib("TypeError: can only concatenate str (not \"NoneType\") to str")
			})?;
			number.push_str(s);
		}
	}
}

/// What a DICT is parsed for: the Top / Font DICT, or a Private DICT, whose
/// blend lists need the number of regions.
#[allow(missing_debug_implementations)]
pub struct DictDecompiler<'a> {
	table: &'static [Operator],
	stack: Vec<Value>,
	/// The entries, in order of first appearance (`dict`).
	pub entries: Vec<(&'static str, Value)>,
	strings: Option<&'a dyn Fn(i64) -> Result<String>>,
	num_regions: Option<&'a dyn Fn(i64) -> Result<i64>>,
}

impl<'a> DictDecompiler<'a> {
	/// A decompiler for `table`.
	pub fn new(
		table: &'static [Operator],
		strings: Option<&'a dyn Fn(i64) -> Result<String>>,
		num_regions: Option<&'a dyn Fn(i64) -> Result<i64>>,
	) -> Self {
		DictDecompiler {
			table,
			stack: Vec::new(),
			entries: Vec::new(),
			strings,
			num_regions,
		}
	}

	fn set(&mut self, name: &'static str, value: Value) {
		match self.entries.iter_mut().find(|(n, _)| *n == name) {
			Some(e) => e.1 = value,
			None => self.entries.push((name, value)),
		}
	}

	fn get(&self, name: &str) -> Option<&Value> {
		self.entries
			.iter()
			.find(|(n, _)| *n == name)
			.map(|(_, v)| v)
	}

	/// `decompile` + `getDict`.
	pub fn decompile(mut self, data: &[u8]) -> Result<Vec<(&'static str, Value)>> {
		let mut i = 0;
		while i < data.len() {
			let b0 = data[i];
			i += 1;
			let byte = |i: usize| data.get(i).copied().ok_or_else(index_error);
			let be = |i: usize, n: usize| {
				data.get(i..i + n)
					.ok_or_else(|| Error::Struct(format!("unpack requires a buffer of {n} bytes")))
			};
			match b0 {
				28 => {
					let b = be(i, 2)?;
					self.stack
						.push(Value::Int(i16::from_be_bytes([b[0], b[1]]) as i64));
					i += 2;
				}
				29 => {
					let b = be(i, 4)?;
					self.stack.push(Value::Int(
						i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as i64
					));
					i += 4;
				}
				30 => {
					let (v, next) = read_real(data, i)?;
					self.stack.push(Value::Float(v));
					i = next;
				}
				0..=31 => {
					let code = if b0 == 12 {
						let b1 = byte(i)?;
						i += 1;
						12 << 8 | b1 as u16
					} else {
						b0 as u16
					};
					// Unknown operators are skipped, leaving their operands on the stack.
					if let Some(op) = operator_by_code(self.table, code) {
						self.operator(op)?;
					}
				}
				32..=246 => self.stack.push(Value::Int(b0 as i64 - 139)),
				247..=250 => {
					let b1 = byte(i)?;
					i += 1;
					self.stack
						.push(Value::Int((b0 as i64 - 247) * 256 + b1 as i64 + 108));
				}
				251..=254 => {
					let b1 = byte(i)?;
					i += 1;
					self.stack
						.push(Value::Int(-(b0 as i64 - 251) * 256 - b1 as i64 - 108));
				}
				255 => {
					// LENIENT(L16): fontTools pushes `NotImplemented` for this reserved
					// byte.
					return Err(Error::ttlib("reserved byte 255 in CFF DICT"));
				}
			}
		}
		if !self.stack.is_empty() {
			return Err(Error::Assertion("non-empty stack".into()));
		}
		Ok(self.entries)
	}

	fn pop(&mut self) -> Result<Value> {
		self.stack.pop().ok_or_else(index_error)
	}

	fn operator(&mut self, op: &'static Operator) -> Result<()> {
		let value = match op.arg {
			A::Number => self.arg_number()?,
			A::Sid => self.arg_sid()?,
			A::Array => Value::List(std::mem::take(&mut self.stack)),
			A::Delta => self.arg_delta(),
			A::BlendList => {
				let list = self.arg_blend_list()?;
				self.stack.extend(list);
				return Ok(());
			}
			A::NumberPair => {
				let b = self.arg_number()?;
				let a = self.arg_number()?;
				Value::Tuple(vec![a, b])
			}
			A::Ros => {
				let c = self.arg_number()?;
				let b = self.arg_sid()?;
				let a = self.arg_sid()?;
				Value::Tuple(vec![a, b, c])
			}
		};
		self.set(op.name, value);
		Ok(())
	}

	fn arg_number(&mut self) -> Result<Value> {
		let first = self.stack.first().ok_or_else(index_error)?;
		if matches!(first, Value::List(_)) {
			// A blended number: the blend list, the rest is dropped.
			let blend = self.pop()?;
			self.stack.clear();
			return Ok(blend);
		}
		self.pop()
	}

	fn arg_sid(&mut self) -> Result<Value> {
		let sid = self.pop()?.int()?;
		let strings = self
			.strings
			.ok_or_else(|| Error::ttlib("TypeError: 'NoneType' object is not subscriptable"))?;
		Ok(Value::Str(strings(sid)?))
	}

	fn arg_delta(&mut self) -> Value {
		let values = std::mem::take(&mut self.stack);
		let mut out = Vec::with_capacity(values.len());
		if let Some(Value::List(first)) = values.first() {
			// Blended groups are absolute already; promote trailing plain values.
			let deltas = first.len() - 1;
			let mut prev = Value::Int(0);
			for v in values {
				match v {
					Value::List(l) => {
						prev = l[0].clone();
						out.push(Value::List(l));
					}
					v => {
						prev = prev.add(&v);
						let mut l = vec![prev.clone()];
						l.extend(std::iter::repeat_n(Value::Int(0), deltas));
						out.push(Value::List(l));
					}
				}
			}
		} else {
			let mut current = Value::Int(0);
			for v in values {
				current = current.add(&v);
				out.push(current.clone());
			}
		}
		Value::List(out)
	}

	fn arg_blend_list(&mut self) -> Result<Vec<Value>> {
		let vsindex = match self.get("vsindex") {
			Some(v) => v.int()?,
			None => 0,
		};
		let num_regions = self.num_regions.ok_or_else(|| {
			Error::ttlib("AttributeError: 'NoneType' object has no attribute 'getNumRegions'")
		})?;
		let masters = num_regions(vsindex)? + 1;
		let blends = self.pop()?.int()?;
		if blends < 0 {
			return Err(Error::Assertion("numBlends >= 0".into()));
		}
		if blends == 0 {
			return Ok(Vec::new());
		}
		let start = self.stack.len() as i64 - masters * blends;
		if start < 0 {
			return Err(Error::Assertion("start >= 0".into()));
		}
		let args = self.stack.split_off(start as usize);
		let deltas = (masters - 1) as usize;
		// Earlier groups stay on the stack: keep the default values continuous
		// and promote plain values before them.
		let mut prev = Value::Int(0);
		for v in self.stack.iter_mut() {
			if let Value::List(l) = v {
				prev = l[0].clone();
			} else {
				prev = prev.add(v);
				let mut l = vec![prev.clone()];
				l.extend(std::iter::repeat_n(Value::Int(0), deltas));
				*v = Value::List(l);
			}
		}
		let blends = blends as usize;
		let mut out = Vec::with_capacity(blends);
		for i in 0..blends {
			let new = args[i].add(&prev);
			prev = new.clone();
			let at = blends + i * deltas;
			let mut l = vec![new];
			l.extend(
				args[at.min(args.len())..(at + deltas).min(args.len())]
					.iter()
					.cloned(),
			);
			out.push(Value::List(l));
		}
		Ok(out)
	}
}

/// `encodeFloat`: a CFF real number, with 8 significant digits.
pub fn encode_float(f: f64, out: &mut Vec<u8>) {
	if f == 0.0 {
		out.extend_from_slice(&[30, 0x0F]);
		return;
	}
	let mut s = format_g(f, 8);
	if let Some(rest) = s.strip_prefix("0.") {
		s = format!(".{rest}");
	} else if let Some(rest) = s.strip_prefix("-0.") {
		s = format!("-.{rest}");
	} else if s.ends_with("000") {
		let significant = s.trim_end_matches('0');
		s = format!("{significant}E{}", s.len() - significant.len());
	} else if let (Some(dot), Some(e)) = (s.find('.'), s.find('E')) {
		let integer = &s[..dot];
		let fraction = &s[dot + 1..e];
		let exponent: i64 = s[e + 1..].parse().unwrap_or(0);
		let new_exponent = exponent - fraction.len() as i64;
		s = if new_exponent == 1 {
			format!("{integer}{fraction}0")
		} else {
			format!("{integer}{fraction}E{new_exponent}")
		};
	}
	if s.starts_with(".0") || s.starts_with("-.0") {
		let (sign, rest) = s.split_once('.').unwrap_or(("", &s));
		let digits = rest.trim_start_matches('0');
		s = format!("{sign}{digits}E-{}", rest.len());
	}
	let mut nibbles: Vec<u8> = Vec::new();
	let mut chars = s.as_str();
	while let Some(c) = chars.chars().next() {
		chars = &chars[c.len_utf8()..];
		let nibble = match c {
			'0'..='9' => c as u8 - b'0',
			'.' => 0xA,
			'-' => 0xE,
			'E' => {
				let mut n = 0xB;
				if let Some(rest) = chars.strip_prefix('-') {
					chars = rest;
					n = 0xC;
				} else if let Some(rest) = chars.strip_prefix('+') {
					chars = rest;
				}
				if let Some(rest) = chars.strip_prefix('0') {
					chars = rest;
				}
				n
			}
			_ => 0xF,
		};
		nibbles.push(nibble);
	}
	nibbles.push(0xF);
	if nibbles.len() % 2 == 1 {
		nibbles.push(0xF);
	}
	out.push(30);
	for pair in nibbles.chunks(2) {
		out.push(pair[0] << 4 | pair[1]);
	}
}

/// `encodeNumber`.
pub fn encode_number(v: &Value, out: &mut Vec<u8>) -> Result<()> {
	match *v {
		Value::Float(f) => encode_float(f, out),
		Value::Int(i) => encode_int_cff(i, out),
		_ => return Err(Error::ttlib(format!("TypeError: not a number: {v:?}"))),
	}
	Ok(())
}

/// The blend operator.
const BLEND_OP: u8 = 23;

/// `arg_number`: a number, or a blend list.
pub fn encode_arg_number(v: &Value, out: &mut Vec<u8>) -> Result<()> {
	if let Value::List(items) = v {
		for item in items {
			encode_number(item, out)?;
		}
		encode_int_cff(1, out);
		out.push(BLEND_OP);
		return Ok(());
	}
	encode_number(v, out)
}

/// `arg_delta`: absolute values as deltas, or blend lists.
pub fn encode_arg_delta(v: &Value, max_blend_stack: i64, out: &mut Vec<u8>) -> Result<()> {
	let Value::List(values) = v else {
		return Err(Error::ttlib("TypeError: delta value is not a list"));
	};
	if values.is_empty() {
		return Ok(());
	}
	if matches!(values[0], Value::List(_)) {
		return encode_delta_blend(values, Value::Int(0), 0, max_blend_stack, out);
	}
	let mut last = Value::Int(0);
	for v in values {
		encode_number(&v.sub(&last), out)?;
		last = v.clone();
	}
	Ok(())
}

/// `arg_delta_blend`.
fn encode_delta_blend(
	values: &[Value],
	mut prev: Value,
	mut depth: i64,
	max_blend_stack: i64,
	out: &mut Vec<u8>,
) -> Result<()> {
	let list = |v: &Value| -> Result<Vec<Value>> {
		match v {
			Value::List(l) => Ok(l.clone()),
			_ => Err(Error::ttlib("TypeError: 'int' object is not subscriptable")),
		}
	};
	let masters = list(&values[0])?.len() as i64;
	let blends = values.len() as i64;
	if depth + blends * masters + 1 > max_blend_stack {
		let mut rest = values;
		while !rest.is_empty() {
			let n = (max_blend_stack - depth - 1).div_euclid(masters);
			if n <= 0 {
				return Err(Error::value("CFF2 blend does not fit the operand stack"));
			}
			let n = (n as usize).min(rest.len());
			let chunk = &rest[..n];
			encode_delta_blend(chunk, prev.clone(), depth, max_blend_stack, out)?;
			prev = list(&chunk[n - 1])?[0].clone();
			depth += n as i64;
			rest = &rest[n..];
		}
		return Ok(());
	}
	let mut firsts = Vec::with_capacity(values.len());
	let mut deltas = Vec::new();
	for v in values {
		let l = list(v)?;
		let default = l.first().cloned().ok_or_else(index_error)?;
		firsts.push(default.sub(&prev));
		prev = default;
		deltas.extend(l[1..].iter().cloned());
	}
	for v in firsts.iter().chain(&deltas) {
		encode_number(v, out)?;
	}
	encode_int_cff(blends, out);
	out.push(BLEND_OP);
	Ok(())
}

/// The opcode bytes of an operator.
pub fn opcode_bytes(code: u16, out: &mut Vec<u8>) {
	if code >> 8 == 12 {
		out.push(12);
	}
	out.push(code as u8);
}

#[cfg(test)]
mod tests {
	use super::*;

	fn real(f: f64) -> String {
		let mut out = Vec::new();
		encode_float(f, &mut out);
		out.iter().map(|b| format!("{b:02x}")).collect()
	}

	// Expected values from fontTools' `encodeFloat`.
	#[test]
	fn reals() {
		let cases: &[(f64, &str)] = &[
			(-9.399999999999999, "1ee9a4ff"),
			(9.4, "1e9a4f"),
			(456.8, "1e456a8f"),
			(0.0, "1e0f"),
			(-0.0, "1e0f"),
			(1.0, "1e1f"),
			(-1.0, "1ee1ff"),
			(9876537.0, "1e9876537f"),
			(1234567890.0, "1e12345679b2ff"),
			(0.0009876537, "1e9876537c10ff"),
			(98765370000.0, "1e9876537b4f"),
			(0.001, "1e1c3f"),
			(0.039625, "1e39625c6f"),
			(1e-07, "1e1c7f"),
			(123456789.0, "1e123456790f"),
			(-0.5, "1eea5f"),
			(100000.0, "1e1b5f"),
			(0.06, "1e6c2f"),
		];
		for &(f, hex) in cases {
			assert_eq!(real(f), hex, "{f}");
		}
	}

	#[test]
	fn reals_read_back() -> Result<()> {
		for f in [0.001, -2.5, 1e-7, 123456789.0, 0.039625] {
			let mut out = Vec::new();
			encode_float(f, &mut out);
			let (v, _) = read_real(&out, 1)?;
			assert_eq!(format_g(v, 8), format_g(f, 8));
		}
		Ok(())
	}
}
