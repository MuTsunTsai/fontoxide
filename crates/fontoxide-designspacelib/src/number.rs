//! Python numbers: designspaceLib keeps whatever `int` or `float` it is given,
//! and the difference shows when a value is formatted with `str()` (variable
//! font names, messages).

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

use fontoxide_misc::py;

/// A Python `int` or `float`.
///
/// Arithmetic follows Python: `int` with `int` stays `int` (except for `/`),
/// anything with a `float` is a `float`. Comparisons and equality are by value,
/// so `Int(1) == Float(1.0)`.
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

	/// Whether this is an `int`.
	pub fn is_int(self) -> bool {
		matches!(self, Number::Int(_))
	}

	/// `abs(value)`.
	pub fn abs(self) -> Number {
		match self {
			Number::Int(i) => Number::Int(i.wrapping_abs()),
			Number::Float(f) => Number::Float(f.abs()),
		}
	}

	/// `max(self, other)`: the first argument wins ties, as in Python.
	pub fn py_max(self, other: Number) -> Number {
		if other > self { other } else { self }
	}

	/// `min(self, other)`: the first argument wins ties, as in Python.
	pub fn py_min(self, other: Number) -> Number {
		if other < self { other } else { self }
	}

	/// `repr(value)`, which is also `str(value)` for numbers.
	pub fn repr(self) -> String {
		self.to_string()
	}
}

impl Default for Number {
	fn default() -> Self {
		Number::Int(0)
	}
}

impl fmt::Display for Number {
	/// `str(value)`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match *self {
			Number::Int(i) => write!(f, "{i}"),
			Number::Float(v) => f.write_str(&py::float_repr(v)),
		}
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

impl PartialEq for Number {
	fn eq(&self, other: &Self) -> bool {
		match (*self, *other) {
			(Number::Int(a), Number::Int(b)) => a == b,
			_ => self.partial_cmp(other) == Some(Ordering::Equal),
		}
	}
}

impl PartialOrd for Number {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		match (*self, *other) {
			(Number::Int(a), Number::Int(b)) => Some(a.cmp(&b)),
			(Number::Int(i), Number::Float(f)) => cmp_int_float(i, f),
			(Number::Float(f), Number::Int(i)) => cmp_int_float(i, f).map(Ordering::reverse),
			(Number::Float(a), Number::Float(b)) => a.partial_cmp(&b),
		}
	}
}

/// Python compares `int` with `float` exactly.
fn cmp_int_float(i: i64, f: f64) -> Option<Ordering> {
	if f.is_nan() {
		return None;
	}
	if f >= 9.3e18 {
		return Some(Ordering::Less);
	}
	if f <= -9.3e18 {
		return Some(Ordering::Greater);
	}
	let t = f.trunc();
	match i.cmp(&(t as i64)) {
		Ordering::Equal => 0.0f64.partial_cmp(&(f - t)),
		ord => Some(ord),
	}
}

macro_rules! arith {
	($trait:ident, $method:ident, $int:ident, $op:tt) => {
		impl $trait for Number {
			type Output = Number;
			fn $method(self, rhs: Number) -> Number {
				match (self, rhs) {
					(Number::Int(a), Number::Int(b)) => Number::Int(a.$int(b)),
					_ => Number::Float(self.to_f64() $op rhs.to_f64()),
				}
			}
		}
	};
}

arith!(Add, add, wrapping_add, +);
arith!(Sub, sub, wrapping_sub, -);
arith!(Mul, mul, wrapping_mul, *);

impl Div for Number {
	type Output = Number;
	/// True division: always a `float`.
	fn div(self, rhs: Number) -> Number {
		Number::Float(self.to_f64() / rhs.to_f64())
	}
}

impl Neg for Number {
	type Output = Number;
	fn neg(self) -> Number {
		match self {
			Number::Int(i) => Number::Int(i.wrapping_neg()),
			Number::Float(f) => Number::Float(-f),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn python_semantics() {
		assert_eq!(Number::Int(1), Number::Float(1.0));
		assert!(Number::Int(1) < Number::Float(1.5));
		assert_eq!((Number::Int(3) + Number::Int(4)).to_string(), "7");
		assert_eq!((Number::Int(3) + Number::Float(4.0)).to_string(), "7.0");
		assert_eq!((Number::Int(4) / Number::Int(2)).to_string(), "2.0");
		assert_eq!(Number::Int(1).py_max(Number::Float(1.0)).to_string(), "1");
	}
}
