//! Big-endian binary reading and writing, the equivalent of `struct.unpack` /
//! `struct.pack` with `>` formats.

use crate::{Error, Result};

/// A cursor over big-endian data.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
	data: &'a [u8],
	pos: usize,
}

macro_rules! read_int {
	($($name:ident: $t:ty),*) => {$(
		#[doc = concat!("Reads a big-endian `", stringify!($t), "`.")]
		pub fn $name(&mut self) -> Result<$t> {
			Ok(<$t>::from_be_bytes(self.array()?))
		}
	)*};
}

impl<'a> Reader<'a> {
	/// A reader at the start of `data`.
	pub fn new(data: &'a [u8]) -> Self {
		Reader { data, pos: 0 }
	}

	/// The current position.
	pub fn pos(&self) -> usize {
		self.pos
	}

	/// Moves to an absolute position.
	pub fn seek(&mut self, pos: usize) {
		self.pos = pos;
	}

	/// The whole underlying data.
	pub fn data(&self) -> &'a [u8] {
		self.data
	}

	/// The data from the current position on.
	pub fn rest(&self) -> &'a [u8] {
		self.data.get(self.pos..).unwrap_or_default()
	}

	/// The number of bytes left.
	pub fn remaining(&self) -> usize {
		self.data.len().saturating_sub(self.pos)
	}

	/// Reads `n` bytes.
	pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
		let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len());
		match end {
			Some(end) => {
				let s = &self.data[self.pos..end];
				self.pos = end;
				Ok(s)
			}
			None => Err(Error::Struct(format!(
				"unpack requires a buffer of {n} bytes"
			))),
		}
	}

	/// Reads a fixed-size array.
	pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
		let s = self.bytes(N)?;
		let mut a = [0; N];
		a.copy_from_slice(s);
		Ok(a)
	}

	/// Skips `n` bytes.
	pub fn skip(&mut self, n: usize) -> Result<()> {
		self.bytes(n).map(|_| ())
	}

	read_int!(u8: u8, i8: i8, u16: u16, i16: i16, u32: u32, i32: i32, u64: u64, i64: i64);

	/// Reads a 24-bit unsigned integer.
	pub fn u24(&mut self) -> Result<u32> {
		let [a, b, c] = self.array()?;
		Ok(u32::from_be_bytes([0, a, b, c]))
	}

	/// Reads a tag.
	pub fn tag(&mut self) -> Result<fontoxide_misc::Tag> {
		Ok(fontoxide_misc::Tag(self.array()?))
	}

	/// Reads `count` big-endian `u16`s.
	pub fn u16_array(&mut self, count: usize) -> Result<Vec<u16>> {
		let s = self.bytes(count.checked_mul(2).ok_or_else(overflow)?)?;
		Ok(s.as_chunks::<2>()
			.0
			.iter()
			.map(|&c| u16::from_be_bytes(c))
			.collect())
	}

	/// Reads `count` big-endian `i16`s.
	pub fn i16_array(&mut self, count: usize) -> Result<Vec<i16>> {
		let s = self.bytes(count.checked_mul(2).ok_or_else(overflow)?)?;
		Ok(s.as_chunks::<2>()
			.0
			.iter()
			.map(|&c| i16::from_be_bytes(c))
			.collect())
	}

	/// Reads `count` big-endian `u32`s.
	pub fn u32_array(&mut self, count: usize) -> Result<Vec<u32>> {
		let s = self.bytes(count.checked_mul(4).ok_or_else(overflow)?)?;
		Ok(s.as_chunks::<4>()
			.0
			.iter()
			.map(|&c| u32::from_be_bytes(c))
			.collect())
	}
}

fn overflow() -> Error {
	Error::Struct("count too large".into())
}

/// Appending big-endian values to a byte buffer.
pub trait WriteBytes {
	/// Appends a `u8`.
	fn put_u8(&mut self, v: u8);
	/// Appends an `i8`.
	fn put_i8(&mut self, v: i8);
	/// Appends a big-endian `u16`.
	fn put_u16(&mut self, v: u16);
	/// Appends a big-endian `i16`.
	fn put_i16(&mut self, v: i16);
	/// Appends a big-endian 24-bit integer.
	fn put_u24(&mut self, v: u32);
	/// Appends a big-endian `u32`.
	fn put_u32(&mut self, v: u32);
	/// Appends a big-endian `i32`.
	fn put_i32(&mut self, v: i32);
	/// Appends a big-endian `u64`.
	fn put_u64(&mut self, v: u64);
	/// Appends a big-endian `i64`.
	fn put_i64(&mut self, v: i64);
}

impl WriteBytes for Vec<u8> {
	fn put_u8(&mut self, v: u8) {
		self.push(v);
	}
	fn put_i8(&mut self, v: i8) {
		self.extend_from_slice(&v.to_be_bytes());
	}
	fn put_u16(&mut self, v: u16) {
		self.extend_from_slice(&v.to_be_bytes());
	}
	fn put_i16(&mut self, v: i16) {
		self.extend_from_slice(&v.to_be_bytes());
	}
	fn put_u24(&mut self, v: u32) {
		self.extend_from_slice(&v.to_be_bytes()[1..]);
	}
	fn put_u32(&mut self, v: u32) {
		self.extend_from_slice(&v.to_be_bytes());
	}
	fn put_i32(&mut self, v: i32) {
		self.extend_from_slice(&v.to_be_bytes());
	}
	fn put_u64(&mut self, v: u64) {
		self.extend_from_slice(&v.to_be_bytes());
	}
	fn put_i64(&mut self, v: i64) {
		self.extend_from_slice(&v.to_be_bytes());
	}
}

/// Converts an integer to a narrower type, failing like `struct.pack` does when the
/// value is out of range.
pub fn fit<T, S>(value: S, field: &str) -> Result<T>
where
	S: TryInto<T> + Copy + std::fmt::Display,
{
	value.try_into().map_err(|_| {
		Error::Struct(format!(
			"Value {value} does not fit in format {} for {field}",
			std::any::type_name::<T>()
		))
	})
}

/// The sfnt checksum of a block of data, padded with NUL bytes to a multiple of
/// four (`calcChecksum`).
///
/// ```
/// use fontoxide_ttlib::binary::calc_checksum;
/// assert_eq!(calc_checksum(b"abcd"), 1633837924);
/// assert_eq!(calc_checksum(b"abcdxyz"), 3655064932);
/// ```
pub fn calc_checksum(data: &[u8]) -> u32 {
	let mut value: u32 = 0;
	let (chunks, rest) = data.as_chunks::<4>();
	for &c in chunks {
		value = value.wrapping_add(u32::from_be_bytes(c));
	}
	if !rest.is_empty() {
		let mut last = [0u8; 4];
		last[..rest.len()].copy_from_slice(rest);
		value = value.wrapping_add(u32::from_be_bytes(last));
	}
	value
}

/// The highest exponent such that `2**exponent <= x`, or 0 (`maxPowerOfTwo`).
pub fn max_power_of_two(x: u32) -> u32 {
	if x == 0 { 0 } else { 31 - x.leading_zeros() }
}

/// `searchRange`, `entrySelector` and `rangeShift` for a binary search header
/// (`getSearchRange`).
pub fn get_search_range(n: u32, item_size: u32) -> (u32, u32, u32) {
	let exponent = max_power_of_two(n);
	let search_range = (1u32 << exponent) * item_size;
	let range_shift = (n * item_size).saturating_sub(search_range);
	(search_range, exponent, range_shift)
}

/// Pads `data` with NUL bytes to a multiple of four.
pub fn pad4(data: &mut Vec<u8>) {
	let rem = data.len() % 4;
	if rem != 0 {
		data.resize(data.len() + 4 - rem, 0);
	}
}
