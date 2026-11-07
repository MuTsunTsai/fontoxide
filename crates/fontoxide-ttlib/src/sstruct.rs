//! Fixed-layout binary structures, the equivalent of `fontTools.misc.sstruct`.
//!
//! fontTools describes fixed-size records with format strings such as
//! `"tableVersion: 16.16F"`. Here they are declared with [`sstruct!`], which
//! generates the struct, binary packing and unpacking, and generic access to the
//! fields by their fontTools names (for TTX). Every declaration names the upstream
//! format it mirrors, and a test checks it against `codegen/sstruct.json`, so that a
//! change in fontTools is caught.

use fontoxide_misc::fixed_tools::{fixed_to_float, float_to_fixed};
use fontoxide_misc::xml_writer::AttrValue;
use std::borrow::Cow;

use crate::binary::{Reader, WriteBytes};
use crate::{Error, Result};

/// A field value, for generic TTX output and input.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
	/// An integer field.
	Int(i64),
	/// A fixed-point field, held as a float.
	Float(f64),
	/// A byte-string field.
	Bytes(Vec<u8>),
}

impl AttrValue for Value {
	/// `str(value)` of the value as fontTools holds it after `sstruct.unpack`:
	/// byte strings that are valid ASCII become `str`, others stay `bytes`.
	fn attr_string(&self) -> Cow<'_, str> {
		match self {
			Value::Int(i) => Cow::Owned(i.to_string()),
			Value::Float(f) => Cow::Owned(fontoxide_misc::py::float_repr(*f)),
			Value::Bytes(b) => Cow::Owned(bytes_to_py_str(b)),
		}
	}
}

/// `str()` of an unpacked byte-string field: the ASCII text if it decodes, or the
/// `bytes` repr otherwise.
pub fn bytes_to_py_str(b: &[u8]) -> String {
	if b.is_ascii() {
		b.iter().map(|&c| c as char).collect()
	} else {
		fontoxide_misc::py::bytes_repr(b)
	}
}

/// A field kind: how a format character maps to a Rust value.
pub trait Kind {
	/// The Rust type of the field.
	type Value: Clone + Default + std::fmt::Debug + PartialEq;
	/// The `struct` format code(s), e.g. `"H"`, or `"h"` for a 2.14 fixed number.
	const CODE: &'static str;
	/// Fraction bits for fixed-point fields.
	const FRACTION_BITS: Option<u32> = None;
	/// The packed size.
	const SIZE: usize;
	/// Reads the value.
	fn read(r: &mut Reader<'_>) -> Result<Self::Value>;
	/// Writes the value; fails if it does not fit.
	fn write(v: &Self::Value, out: &mut Vec<u8>, field: &str) -> Result<()>;
	/// The value for TTX.
	fn value(v: &Self::Value) -> Value;
	/// Sets the value from a Python literal, as `safeEval` would produce it.
	fn from_literal(lit: &fontoxide_misc::text_tools::Literal, field: &str) -> Result<Self::Value>;
}

fn bad_literal(field: &str) -> Error {
	Error::value(format!("invalid value for {field}"))
}

macro_rules! int_kind {
	($name:ident, $t:ty, $code:literal, $read:ident, $put:ident) => {
		#[doc = concat!("`", $code, "`: `", stringify!($t), "`.")]
		#[derive(Debug)]
		pub enum $name {}
		impl Kind for $name {
			type Value = $t;
			const CODE: &'static str = $code;
			const SIZE: usize = std::mem::size_of::<$t>();
			fn read(r: &mut Reader<'_>) -> Result<$t> {
				r.$read()
			}
			fn write(v: &$t, out: &mut Vec<u8>, _field: &str) -> Result<()> {
				out.$put(*v);
				Ok(())
			}
			fn value(v: &$t) -> Value {
				Value::Int(*v as i64)
			}
			fn from_literal(lit: &fontoxide_misc::text_tools::Literal, field: &str) -> Result<$t> {
				let i = lit.as_int().ok_or_else(|| bad_literal(field))?;
				crate::binary::fit(i, field)
			}
		}
	};
}

int_kind!(U8, u8, "B", u8, put_u8);
int_kind!(I8, i8, "b", i8, put_i8);
int_kind!(U16, u16, "H", u16, put_u16);
int_kind!(I16, i16, "h", i16, put_i16);
int_kind!(U32, u32, "L", u32, put_u32);
int_kind!(I32, i32, "l", i32, put_i32);
int_kind!(U64, u64, "Q", u64, put_u64);
int_kind!(I64, i64, "q", i64, put_i64);

macro_rules! fixed_kind {
	($name:ident, $code:literal, $bits:literal, $raw:ty, $read:ident, $put:ident) => {
		#[doc = concat!("A fixed-point number with ", stringify!($bits), " fraction bits, held as `f64`.")]
		#[derive(Debug)]
		pub enum $name {}
		impl Kind for $name {
			type Value = f64;
			const CODE: &'static str = $code;
			const FRACTION_BITS: Option<u32> = Some($bits);
			const SIZE: usize = std::mem::size_of::<$raw>();
			fn read(r: &mut Reader<'_>) -> Result<f64> {
				Ok(fixed_to_float(r.$read()? as i64, $bits))
			}
			fn write(v: &f64, out: &mut Vec<u8>, field: &str) -> Result<()> {
				let raw: $raw = crate::binary::fit(float_to_fixed(*v, $bits), field)?;
				out.$put(raw);
				Ok(())
			}
			fn value(v: &f64) -> Value {
				Value::Float(*v)
			}
			fn from_literal(lit: &fontoxide_misc::text_tools::Literal, field: &str) -> Result<f64> {
				lit.as_float().ok_or_else(|| bad_literal(field))
			}
		}
	};
}

fixed_kind!(F2Dot14, "h", 14, i16, i16, put_i16);
fixed_kind!(F16Dot16, "l", 16, i32, i32, put_i32);

/// `Ns`: a byte string of `N` bytes.
#[derive(Debug)]
pub enum Bytes<const N: usize> {}

impl<const N: usize> Kind for Bytes<N>
where
	[u8; N]: Default,
{
	type Value = [u8; N];
	const CODE: &'static str = "s";
	const SIZE: usize = N;
	fn read(r: &mut Reader<'_>) -> Result<[u8; N]> {
		r.array()
	}
	fn write(v: &[u8; N], out: &mut Vec<u8>, _field: &str) -> Result<()> {
		out.extend_from_slice(v);
		Ok(())
	}
	fn value(v: &[u8; N]) -> Value {
		Value::Bytes(v.to_vec())
	}
	fn from_literal(lit: &fontoxide_misc::text_tools::Literal, field: &str) -> Result<[u8; N]> {
		use fontoxide_misc::text_tools::Literal;
		let bytes = match lit {
			Literal::Bytes(b) => b.clone(),
			Literal::Str(s) => s.bytes().collect::<Vec<u8>>(),
			_ => return Err(bad_literal(field)),
		};
		// struct.pack("Ns") truncates or pads with NUL bytes.
		let mut out = [0u8; N];
		let n = bytes.len().min(N);
		out[..n].copy_from_slice(&bytes[..n]);
		Ok(out)
	}
}

/// A field of an [`sstruct!`] declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldInfo {
	/// The fontTools attribute name.
	pub name: &'static str,
	/// The `struct` format code.
	pub code: &'static str,
	/// For byte strings, the length.
	pub size: usize,
	/// Fraction bits for fixed-point fields.
	pub fraction_bits: Option<u32>,
}

/// Declares a fixed-layout structure mirroring a fontTools `sstruct` format.
///
/// ```ignore
/// sstruct! {
///     /// Docs.
///     pub struct Foo = "fooFormat" {
///         /// Field docs.
///         pub version: f64 = "version" as F16Dot16,
///     }
/// }
/// ```
#[macro_export]
#[doc(hidden)]
macro_rules! sstruct {
	(
		$(#[$meta:meta])*
		$vis:vis struct $name:ident = $format:literal {
			$(
				$(#[$fmeta:meta])*
				$fvis:vis $field:ident : $ty:ty = $py:literal as $kind:ty
			),* $(,)?
		}
	) => {
		$(#[$meta])*
		#[derive(Debug, Clone, PartialEq, Default)]
		$vis struct $name {
			$(
				$(#[$fmeta])*
				#[doc = concat!("\n\nfontTools: `", $py, "`.")]
				$fvis $field: $ty,
			)*
		}

		impl $name {
			/// The fontTools format this structure mirrors.
			pub const FORMAT_NAME: &'static str = $format;
			/// The packed size in bytes.
			pub const SIZE: usize = 0 $(+ <$kind as $crate::sstruct::Kind>::SIZE)*;
			/// The fields, in order.
			pub const FIELDS: &'static [$crate::sstruct::FieldInfo] = &[$(
				$crate::sstruct::FieldInfo {
					name: $py,
					code: <$kind as $crate::sstruct::Kind>::CODE,
					size: <$kind as $crate::sstruct::Kind>::SIZE,
					fraction_bits: <$kind as $crate::sstruct::Kind>::FRACTION_BITS,
				},
			)*];

			/// Reads the structure (`sstruct.unpack`).
			pub fn unpack(r: &mut $crate::binary::Reader<'_>) -> $crate::Result<Self> {
				Ok($name {
					$($field: <$kind as $crate::sstruct::Kind>::read(r)?,)*
				})
			}

			/// Unpacks from the start of `data` (`sstruct.unpack2`); returns the
			/// structure and the remaining data.
			pub fn unpack2(data: &[u8]) -> $crate::Result<(Self, &[u8])> {
				if data.len() < Self::SIZE {
					return Err($crate::Error::Struct(format!(
						"unpack requires a buffer of {} bytes",
						Self::SIZE
					)));
				}
				let mut r = $crate::binary::Reader::new(data);
				let v = Self::unpack(&mut r)?;
				Ok((v, r.rest()))
			}

			/// Appends the packed structure (`sstruct.pack`).
			pub fn pack(&self, out: &mut Vec<u8>) -> $crate::Result<()> {
				$(<$kind as $crate::sstruct::Kind>::write(&self.$field, out, $py)?;)*
				Ok(())
			}

			/// The field values by fontTools name, in order.
			pub fn values(&self) -> Vec<(&'static str, $crate::sstruct::Value)> {
				vec![$(($py, <$kind as $crate::sstruct::Kind>::value(&self.$field)),)*]
			}

			/// Sets a field by fontTools name from a `safeEval` literal. Returns
			/// `false` if there is no such field.
			pub fn set_literal(
				&mut self,
				name: &str,
				literal: &fontoxide_misc::text_tools::Literal,
			) -> $crate::Result<bool> {
				match name {
					$($py => {
						self.$field = <$kind as $crate::sstruct::Kind>::from_literal(literal, $py)?;
						Ok(true)
					})*
					_ => Ok(false),
				}
			}
		}
	};
}

/// Parses a TTX `value` attribute with `safeEval`.
pub fn eval(value: &str) -> Result<fontoxide_misc::text_tools::Literal> {
	fontoxide_misc::text_tools::safe_eval(value)
		.ok_or_else(|| Error::value(format!("malformed value: {value:?}")))
}

#[cfg(test)]
pub(crate) mod tests {
	//! Checks the declarations against the formats exported from fontTools.

	use super::FieldInfo;

	#[derive(serde::Deserialize)]
	struct Export {
		formats: Vec<Format>,
	}

	#[derive(serde::Deserialize)]
	struct Format {
		name: String,
		struct_format: String,
		fields: Vec<String>,
		fixes: std::collections::HashMap<String, u32>,
	}

	/// Asserts that a declaration matches the upstream format of the same name.
	pub(crate) fn check(format_name: &str, fields: &[FieldInfo]) {
		let export: Export = serde_json::from_str(include_str!("../codegen/sstruct.json"))
			.expect("valid sstruct.json");
		let format = export
			.formats
			.iter()
			.find(|f| f.name == format_name)
			.unwrap_or_else(|| panic!("{format_name} not found in sstruct.json"));
		let names: Vec<&str> = fields.iter().map(|f| f.name).collect();
		assert_eq!(names, format.fields, "{format_name}: field names");
		let mut codes = String::new();
		for f in fields {
			if f.code == "s" {
				codes.push_str(&format!("{}s", f.size));
			} else {
				codes.push_str(f.code);
			}
		}
		// 'I'/'i' and 'L'/'l' are the same size with '>'.
		let upstream = format
			.struct_format
			.trim_start_matches(['>', '<', '!', '='])
			.replace('I', "L")
			.replace('i', "l");
		assert_eq!(codes, upstream, "{format_name}: struct format");
		for f in fields {
			assert_eq!(
				f.fraction_bits,
				format.fixes.get(f.name).copied(),
				"{format_name}.{}: fixed point",
				f.name
			);
		}
	}
}
