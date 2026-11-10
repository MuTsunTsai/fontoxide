//! Python's `complex`, as fontTools' curve math uses it.
//!
//! The operations follow CPython 3.13 (`Objects/complexobject.c`) so that the
//! results are bit for bit the same, signed zeros included: a real operand is
//! first converted to a complex number with a zero imaginary part, so
//! `2.0 * z` is a full complex product (`_Py_c_prod`), and `z / 2.0` goes
//! through `_Py_c_quot`.

use std::ops::{Add, AddAssign, Mul, Neg, Sub};

/// A complex number (`complex`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Complex {
	/// `real`.
	pub re: f64,
	/// `imag`.
	pub im: f64,
}

/// `1j`.
pub const J: Complex = Complex { re: 0.0, im: 1.0 };

impl Complex {
	/// `complex(re, im)`.
	pub const fn new(re: f64, im: f64) -> Self {
		Complex { re, im }
	}

	/// `complex(re)`, or a real number as an operand of a complex operation.
	pub const fn real(re: f64) -> Self {
		Complex { re, im: 0.0 }
	}

	/// `complex(*pt)`.
	pub const fn from_point((x, y): (f64, f64)) -> Self {
		Complex { re: x, im: y }
	}

	/// `(z.real, z.imag)`.
	pub const fn to_point(self) -> (f64, f64) {
		(self.re, self.im)
	}

	/// `abs(z)`: `hypot(real, imag)`.
	pub fn abs(self) -> f64 {
		self.re.hypot(self.im)
	}

	/// `z.conjugate()`.
	pub const fn conj(self) -> Self {
		Complex {
			re: self.re,
			im: -self.im,
		}
	}

	/// `z * x` or `x * z` for a real `x` (`_Py_c_prod` with `complex(x)`).
	pub fn scale(self, x: f64) -> Self {
		self * Complex::real(x)
	}

	/// `z / x` for a real `x` (`_Py_c_quot` with `complex(x)`). `None` for
	/// `ZeroDivisionError`.
	pub fn div_real(self, x: f64) -> Option<Self> {
		self.quot(Complex::real(x))
	}

	/// `a / b` (`_Py_c_quot`). `None` for `ZeroDivisionError`.
	pub fn quot(self, b: Complex) -> Option<Self> {
		let a = self;
		let abs_breal = if b.re < 0.0 { -b.re } else { b.re };
		let abs_bimag = if b.im < 0.0 { -b.im } else { b.im };
		if abs_breal >= abs_bimag {
			if abs_breal == 0.0 {
				return None;
			}
			let ratio = b.im / b.re;
			let denom = b.re + b.im * ratio;
			Some(Complex {
				re: (a.re + a.im * ratio) / denom,
				im: (a.im - a.re * ratio) / denom,
			})
		} else if abs_bimag >= abs_breal {
			let ratio = b.re / b.im;
			let denom = b.re * ratio + b.im;
			Some(Complex {
				re: (a.re * ratio + a.im) / denom,
				im: (a.im * ratio - a.re) / denom,
			})
		} else {
			// At least one of b.real or b.imag is a NaN.
			Some(Complex {
				re: f64::NAN,
				im: f64::NAN,
			})
		}
	}

	/// Whether either part is a NaN.
	pub fn is_nan(self) -> bool {
		self.re.is_nan() || self.im.is_nan()
	}

	/// `repr(z)`: `(1+2j)`, or `2j` if the real part is a positive zero. The
	/// parts are formatted with `repr`, without a `.0` for integral values.
	pub fn repr(self) -> String {
		fn r(x: f64) -> String {
			let s = crate::py::float_repr(x);
			s.strip_suffix(".0").map(str::to_owned).unwrap_or(s)
		}
		if self.re == 0.0 && self.re.is_sign_positive() {
			return format!("{}j", r(self.im));
		}
		let im = r(self.im);
		let sign = if im.starts_with('-') { "" } else { "+" };
		format!("({}{sign}{im}j)", r(self.re))
	}
}

impl Add for Complex {
	type Output = Complex;

	fn add(self, b: Complex) -> Complex {
		Complex {
			re: self.re + b.re,
			im: self.im + b.im,
		}
	}
}

impl AddAssign for Complex {
	fn add_assign(&mut self, b: Complex) {
		*self = *self + b;
	}
}

impl Sub for Complex {
	type Output = Complex;

	fn sub(self, b: Complex) -> Complex {
		Complex {
			re: self.re - b.re,
			im: self.im - b.im,
		}
	}
}

impl Neg for Complex {
	type Output = Complex;

	fn neg(self) -> Complex {
		Complex {
			re: -self.re,
			im: -self.im,
		}
	}
}

/// `_Py_c_prod`.
impl Mul for Complex {
	type Output = Complex;

	fn mul(self, b: Complex) -> Complex {
		Complex {
			re: self.re * b.re - self.im * b.im,
			im: self.re * b.im + self.im * b.re,
		}
	}
}

/// `z * x` for a real `x`, as a full complex product.
impl Mul<f64> for Complex {
	type Output = Complex;

	fn mul(self, x: f64) -> Complex {
		self.scale(x)
	}
}

/// `x * z` for a real `x`, as a full complex product.
impl Mul<Complex> for f64 {
	type Output = Complex;

	fn mul(self, z: Complex) -> Complex {
		Complex::real(self) * z
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn signed_zeros_follow_cpython() {
		// `-0.0 * complex(1, -2)` is `0j` in Python 3.13: the real operand is
		// promoted, and `-0.0*1 - 0*(-2)` is `+0.0`.
		let z = -0.0 * Complex::new(1.0, -2.0);
		assert!(z.re == 0.0 && z.re.is_sign_positive());
		assert!(z.im == 0.0 && z.im.is_sign_positive());
		assert_eq!(Complex::new(3.0, 4.0).abs(), 5.0);
		assert_eq!(Complex::new(1.0, 2.0).div_real(0.0), None);
		assert_eq!(
			Complex::new(1.0, 2.0).div_real(4.0),
			Some(Complex::new(0.25, 0.5))
		);
		assert_eq!(Complex::new(2.0, 3.0) * J, Complex::new(-3.0, 2.0));
	}
}
