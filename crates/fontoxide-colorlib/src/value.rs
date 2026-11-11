//! The dynamic input of the builders: what fontTools' `TableBuilder` takes as
//! dicts, tuples, numbers, enum names and already-built tables.

use fontoxide_misc::py;
use fontoxide_ttlib::ot::types as ot;
use indexmap::IndexMap;

use crate::generated::Table;
use crate::{Error, Result};

/// A Python value as the builders take it (`_PaintInput` and friends).
///
/// Tuples and lists are both [`Value::List`]; fontTools treats them alike,
/// except that a `(layerGlyph, paletteIndex)` pair of either kind is a COLRv0
/// layer. The [`value!`](crate::value!) macro writes values like Python
/// literals.
#[derive(Debug, Clone)]
pub enum Value {
	/// `int` (and `IntEnum` members such as `PaintFormat.PaintSolid`).
	Int(i64),
	/// `float`.
	Float(f64),
	/// `str`, such as a glyph name or an enum name like `"pad"`.
	Str(String),
	/// `list` or `tuple`.
	List(Vec<Value>),
	/// `dict`, in insertion order.
	Map(IndexMap<String, Value>),
	/// An already-built table, used as it is.
	Table(Table),
}

impl Value {
	/// `type(value).__name__`.
	pub fn py_type_name(&self) -> &'static str {
		match self {
			Value::Int(_) => "int",
			Value::Float(_) => "float",
			Value::Str(_) => "str",
			Value::List(_) => "list",
			Value::Map(_) => "dict",
			Value::Table(t) => t.class().name(),
		}
	}

	/// `repr(value)`. Lists are written as lists, also where the Python value
	/// would be a tuple.
	pub fn py_repr(&self) -> String {
		match self {
			Value::Int(v) => v.to_string(),
			Value::Float(v) => py::float_repr(*v),
			Value::Str(s) => py::str_repr(s),
			Value::List(items) => {
				let items: Vec<String> = items.iter().map(Value::py_repr).collect();
				format!("[{}]", items.join(", "))
			}
			Value::Map(map) => {
				let items: Vec<String> = map
					.iter()
					.map(|(k, v)| format!("{}: {}", py::str_repr(k), v.py_repr()))
					.collect();
				format!("{{{}}}", items.join(", "))
			}
			Value::Table(t) => {
				format!(
					"<fontTools.ttLib.tables.otTables.{} object>",
					t.class().name()
				)
			}
		}
	}

	/// The items of `iter(value)`: the elements of a list, the keys of a dict,
	/// the characters of a string; `TypeError` for anything else.
	pub fn py_iter(self) -> Result<Vec<Value>> {
		match self {
			Value::List(items) => Ok(items),
			Value::Map(map) => Ok(map.into_keys().map(Value::Str).collect()),
			Value::Str(s) => Ok(s.chars().map(|c| Value::Str(c.to_string())).collect()),
			other => Err(Error::Type(format!(
				"'{}' object is not iterable",
				other.py_type_name()
			))),
		}
	}

	/// The number, for arithmetic (`TypeError` for other values).
	pub(crate) fn as_number(&self) -> Result<f64> {
		match self {
			Value::Int(v) => Ok(*v as f64),
			Value::Float(v) => Ok(*v),
			other => Err(Error::Type(format!(
				"must be real number, not {}",
				other.py_type_name()
			))),
		}
	}
}

/// Python's `==`: numbers compare by value whatever their type, dicts
/// regardless of their order.
impl PartialEq for Value {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Value::Int(a), Value::Int(b)) => a == b,
			(Value::Float(a), Value::Float(b)) => a == b,
			(Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => {
				// Python compares the exact values.
				b.fract() == 0.0
					&& b.is_finite()
					&& *b >= -(2f64.powi(63))
					&& *b < 2f64.powi(63)
					&& *b as i64 == *a
			}
			(Value::Str(a), Value::Str(b)) => a == b,
			(Value::List(a), Value::List(b)) => a == b,
			(Value::Map(a), Value::Map(b)) => {
				a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|w| v == w))
			}
			(Value::Table(a), Value::Table(b)) => a == b,
			_ => false,
		}
	}
}

macro_rules! from_int {
	($($t:ty),*) => {
		$(impl From<$t> for Value {
			fn from(v: $t) -> Self {
				Value::Int(v as i64)
			}
		})*
	};
}
from_int!(i8, i16, i32, i64, u8, u16, u32, usize);

impl From<f64> for Value {
	fn from(v: f64) -> Self {
		Value::Float(v)
	}
}

impl From<f32> for Value {
	fn from(v: f32) -> Self {
		Value::Float(f64::from(v))
	}
}

impl From<&str> for Value {
	fn from(v: &str) -> Self {
		Value::Str(v.to_owned())
	}
}

impl From<String> for Value {
	fn from(v: String) -> Self {
		Value::Str(v)
	}
}

impl From<&String> for Value {
	fn from(v: &String) -> Self {
		Value::Str(v.clone())
	}
}

impl<T: Into<Value>> From<Vec<T>> for Value {
	fn from(v: Vec<T>) -> Self {
		Value::List(v.into_iter().map(Into::into).collect())
	}
}

impl From<IndexMap<String, Value>> for Value {
	fn from(v: IndexMap<String, Value>) -> Self {
		Value::Map(v)
	}
}

impl From<Table> for Value {
	fn from(v: Table) -> Self {
		Value::Table(v)
	}
}

macro_rules! from_table {
	($($t:ident),*) => {
		$(impl From<ot::$t> for Value {
			fn from(v: ot::$t) -> Self {
				Value::Table(Table::$t(v))
			}
		})*
	};
}
from_table!(
	Paint,
	ColorLine,
	VarColorLine,
	ColorStop,
	VarColorStop,
	Affine2x3,
	VarAffine2x3,
	ClipBox
);

/// Writes a [`Value`] like a Python literal: `{"key": value, ...}` for a dict,
/// `[...]` or `(...)` for a list, and any other expression for what converts
/// into a value (numbers, strings, enums, tables, other values).
///
/// ```
/// use fontoxide_colorlib::{PaintFormat, Value, value};
/// let paint = value!({"Format": PaintFormat::PaintGlyph, "Paint": (2, 0, 0.5), "Glyph": "a"});
/// let Value::Map(map) = &paint else { unreachable!() };
/// assert_eq!(map["Paint"], value!([2, 0, 0.5]));
/// assert_eq!(value!(-1), Value::Int(-1));
/// ```
#[macro_export]
macro_rules! value {
	({ $($tt:tt)* }) => {
		$crate::Value::Map($crate::__value_map!([] $($tt)*))
	};
	([ $($tt:tt)* ]) => {
		$crate::Value::List($crate::__value_list!([] () $($tt)*))
	};
	(( $($tt:tt)* )) => {
		$crate::Value::List($crate::__value_list!([] () $($tt)*))
	};
	($($e:tt)+) => {
		$crate::Value::from($($e)+)
	};
}

/// The items of a list for [`value!`].
#[doc(hidden)]
#[macro_export]
macro_rules! __value_list {
	([$($out:expr,)*] ()) => {
		vec![$($out,)*]
	};
	([$($out:expr,)*] ($($cur:tt)+)) => {
		vec![$($out,)* $crate::value!($($cur)+)]
	};
	([$($out:expr,)*] ($($cur:tt)+) , $($rest:tt)*) => {
		$crate::__value_list!([$($out,)* $crate::value!($($cur)+),] () $($rest)*)
	};
	([$($out:expr,)*] ($($cur:tt)*) $t:tt $($rest:tt)*) => {
		$crate::__value_list!([$($out,)*] ($($cur)* $t) $($rest)*)
	};
}

/// The entries of a dict for [`value!`].
#[doc(hidden)]
#[macro_export]
macro_rules! __value_map {
	([$($out:expr,)*]) => {
		$crate::__private::IndexMap::<String, $crate::Value>::from_iter([$($out,)*])
	};
	([$($out:expr,)*] $k:tt : $($rest:tt)*) => {
		$crate::__value_map_item!([$($out,)*] $k () $($rest)*)
	};
}

/// The value of a dict entry for [`value!`].
#[doc(hidden)]
#[macro_export]
macro_rules! __value_map_item {
	([$($out:expr,)*] $k:tt ($($cur:tt)+)) => {
		$crate::__value_map!([$($out,)* (String::from($k), $crate::value!($($cur)+)),])
	};
	([$($out:expr,)*] $k:tt ($($cur:tt)+) , $($rest:tt)*) => {
		$crate::__value_map!([$($out,)* (String::from($k), $crate::value!($($cur)+)),] $($rest)*)
	};
	([$($out:expr,)*] $k:tt ($($cur:tt)*) $t:tt $($rest:tt)*) => {
		$crate::__value_map_item!([$($out,)*] $k ($($cur)* $t) $($rest)*)
	};
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::PaintFormat;

	#[test]
	fn python_equality() {
		assert_eq!(Value::Int(1), Value::Float(1.0));
		assert_ne!(Value::Int(1), Value::Float(1.5));
		assert_eq!(value!({"a": 1, "b": 2}), value!({"b": 2.0, "a": 1}));
		assert_ne!(value!([1, 2]), value!([2, 1]));
	}

	#[test]
	fn macro_forms() {
		let v = value!((PaintFormat::PaintSolid, -1, [0.5, "x"], {"k": -2.5}));
		assert_eq!(v.py_repr(), "[2, -1, [0.5, 'x'], {'k': -2.5}]");
		assert_eq!(value!([]), Value::List(Vec::new()));
		assert_eq!(value!({}), Value::Map(IndexMap::new()));
	}

	#[test]
	fn iteration() -> Result<()> {
		assert_eq!(value!("ab").py_iter()?, vec![value!("a"), value!("b")]);
		assert_eq!(value!({"x": 1}).py_iter()?, vec![value!("x")]);
		assert_eq!(
			value!(5).py_iter().err().map(|e| e.to_string()),
			Some("'int' object is not iterable".into())
		);
		Ok(())
	}
}
