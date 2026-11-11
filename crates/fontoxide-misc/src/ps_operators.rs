//! Port of `fontTools.misc.psOperators`: the objects of the minimal PostScript
//! interpreter of [`ps_lib`](crate::ps_lib), and the key orders of Type 1 font
//! dictionaries.
//!
//! The operators themselves (`PSOperators`) are methods of
//! [`PsInterpreter`](crate::ps_lib::PsInterpreter).

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;

/// A shared, mutable PostScript object: Python objects are references, and
/// operators like `cvx`, `readonly` and `readstring` change them in place.
pub type ObjRef = Rc<RefCell<PsObject>>;

/// A shared dictionary (`ps_dict.value`, `ps_font.value`, the dictionaries of
/// the dictionary stack).
pub type DictRef = Rc<RefCell<IndexMap<Key, ObjRef>>>;

/// A dictionary key: the hashable Python values that PostScript objects carry.
///
/// Numbers that compare equal in Python are the same key (`1 == 1.0 == True`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
	/// `None` (the value of operators and `null`).
	None,
	/// An integer, a boolean, or a float with an integral value.
	Int(i64),
	/// A float without an integral value, by its bits.
	Float(u64),
	/// A string: names, literals and text strings.
	Str(String),
	/// A byte string, as `readstring` makes them.
	Bytes(Vec<u8>),
	/// The file object of a tokenizer, by identity.
	File(u64),
}

impl Key {
	/// The name, for string keys.
	pub fn as_str(&self) -> Option<&str> {
		match self {
			Key::Str(s) => Some(s),
			_ => None,
		}
	}
}

impl fmt::Display for Key {
	/// `str(key)`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Key::None => f.write_str("None"),
			Key::Int(i) => write!(f, "{i}"),
			Key::Float(bits) => f.write_str(&crate::py::float_repr(f64::from_bits(*bits))),
			Key::Str(s) => f.write_str(s),
			Key::Bytes(b) => f.write_str(&crate::py::bytes_repr(b)),
			Key::File(_) => f.write_str("<fontTools.misc.psLib.PSTokenizer object>"),
		}
	}
}

impl From<&str> for Key {
	fn from(s: &str) -> Self {
		Key::Str(s.to_owned())
	}
}

/// The value of a string object (`ps_string.value`): text, or the bytes that
/// `readstring` reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PsString {
	/// A `str`; characters of hexadecimal strings are in `0..=255`.
	Text(String),
	/// A `bytes`.
	Bytes(Vec<u8>),
}

impl PsString {
	/// `len(value)`: characters or bytes.
	pub fn len(&self) -> usize {
		match self {
			PsString::Text(s) => s.chars().count(),
			PsString::Bytes(b) => b.len(),
		}
	}

	/// Whether it is empty.
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}
}

/// What a PostScript object is, with its value.
#[derive(Clone)]
pub enum Kind {
	/// `ps_operator`, by name; the interpreter runs it.
	Operator(&'static str),
	/// `ps_procedure`.
	Procedure(Vec<ObjRef>),
	/// `ps_name`.
	Name(String),
	/// `ps_literal`.
	Literal(String),
	/// `ps_array`; `None` for the elements `array` leaves unset.
	Array(Vec<Option<ObjRef>>),
	/// `ps_dict`.
	Dict(DictRef),
	/// `ps_font`.
	Font(DictRef),
	/// `ps_mark`.
	Mark,
	/// `ps_procmark`.
	ProcMark,
	/// `ps_null`.
	Null,
	/// `ps_file`: the tokenizer of the interpreter, by identity.
	File(u64),
	/// `ps_boolean`. `true` and `false` of `systemdict` hold the integers 1 and 0
	/// in fontTools (`py_bool` false); the comparison operators hold Python
	/// booleans.
	Boolean {
		/// The truth value.
		value: bool,
		/// Whether the value is a Python `bool` rather than an `int`.
		py_bool: bool,
	},
	/// `ps_string`.
	String(PsString),
	/// `ps_integer`.
	Integer(i64),
	/// `ps_real`.
	Real(f64),
}

impl fmt::Debug for Kind {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Kind::Operator(n) => write!(f, "<operator {n}>"),
			Kind::Procedure(_) => f.write_str("<procedure>"),
			Kind::Name(n) => write!(f, "<name {}>", crate::py::str_repr(n)),
			Kind::Literal(n) => write!(f, "<literal {}>", crate::py::str_repr(n)),
			Kind::Array(_) => f.write_str("<array>"),
			Kind::Dict(_) => f.write_str("<dict>"),
			Kind::Font(_) => f.write_str("<font>"),
			Kind::Mark => f.write_str("<mark 'mark'>"),
			Kind::ProcMark => f.write_str("<procmark 'procmark'>"),
			Kind::Null => f.write_str("<null None>"),
			Kind::File(_) => f.write_str("<file>"),
			Kind::Boolean { value, .. } => write!(f, "<boolean {value}>"),
			Kind::String(s) => write!(f, "<string {s:?}>"),
			Kind::Integer(i) => write!(f, "<integer {i}>"),
			Kind::Real(r) => write!(f, "<real {}>", crate::py::float_repr(*r)),
		}
	}
}

/// A PostScript object (`ps_object`).
#[derive(Debug, Clone)]
pub struct PsObject {
	/// The kind and value.
	pub kind: Kind,
	/// Whether it is literal rather than executable (`literal`).
	pub literal: bool,
	/// The access level: 0 unlimited, 1 `readonly`, 2 `executeonly`, 3
	/// `noaccess` (`access`).
	pub access: u8,
}

impl PsObject {
	/// A new object, literal or executable as its class is in fontTools.
	pub fn new(kind: Kind) -> ObjRef {
		let literal = !matches!(kind, Kind::Operator(_) | Kind::Procedure(_) | Kind::Name(_));
		Rc::new(RefCell::new(PsObject {
			kind,
			literal,
			access: 0,
		}))
	}

	/// The type name (`type`), e.g. `"integertype"`.
	pub fn type_name(&self) -> &'static str {
		match self.kind {
			Kind::Operator(_) => "operatortype",
			Kind::Procedure(_) => "proceduretype",
			Kind::Name(_) => "nametype",
			Kind::Literal(_) => "literaltype",
			Kind::Array(_) => "arraytype",
			Kind::Dict(_) => "dicttype",
			Kind::Font(_) => "fonttype",
			Kind::Mark => "marktype",
			Kind::ProcMark => "procmarktype",
			Kind::Null => "nulltype",
			Kind::File(_) => "filetype",
			Kind::Boolean { .. } => "booleantype",
			Kind::String(_) => "stringtype",
			Kind::Integer(_) => "integertype",
			Kind::Real(_) => "realtype",
		}
	}
}

/// The access names (`_accessstrings`).
pub const ACCESS_STRINGS: [&str; 4] = ["", "readonly", "executeonly", "noaccess"];

/// The keys of a Type 1 font dictionary before the `eexec` part, in order
/// (`_type1_pre_eexec_order`).
pub const TYPE1_PRE_EEXEC_ORDER: [&str; 10] = [
	"FontInfo",
	"FontName",
	"Encoding",
	"PaintType",
	"FontType",
	"FontMatrix",
	"FontBBox",
	"UniqueID",
	"Metrics",
	"StrokeWidth",
];

/// The keys of a Type 1 `FontInfo` dictionary, in order (`_type1_fontinfo_order`).
pub const TYPE1_FONTINFO_ORDER: [&str; 9] = [
	"version",
	"Notice",
	"FullName",
	"FamilyName",
	"Weight",
	"ItalicAngle",
	"isFixedPitch",
	"UnderlinePosition",
	"UnderlineThickness",
];

/// The keys of a Type 1 font dictionary in the `eexec` part, in order
/// (`_type1_post_eexec_order`).
pub const TYPE1_POST_EEXEC_ORDER: [&str; 3] = ["Private", "CharStrings", "FID"];
