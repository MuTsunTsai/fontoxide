//! `OTTableReader` (`fontTools.ttLib.tables.otBase`).

use std::rc::Rc;

use fontoxide_misc::Tag;

use super::state::{State, StateValue};
use crate::{Error, Result};

/// Reads OpenType Layout data. Offsets are relative to `offset`, the start of the
/// current subtable; `state` carries values propagated from parent tables (counts,
/// value formats, the lookup type, …).
#[derive(Debug, Clone)]
pub struct OtReader<'a> {
	data: &'a [u8],
	offset: usize,
	pos: usize,
	state: Rc<State>,
	/// The table being read (`tableTag`), e.g. `GSUB`.
	pub table_tag: Tag,
}

fn short(n: usize) -> Error {
	Error::Struct(format!("unpack requires a buffer of {n} bytes"))
}

macro_rules! read_int {
	($name:ident, $array:ident, $t:ty, $n:literal) => {
		#[doc = concat!("Reads a big-endian `", stringify!($t), "`.")]
		pub fn $name(&mut self) -> Result<$t> {
			let bytes = self.take($n)?;
			let mut a = [0u8; $n];
			a.copy_from_slice(bytes);
			Ok(<$t>::from_be_bytes(a))
		}

		#[doc = concat!("Reads `count` big-endian `", stringify!($t), "`s (`read…Array`).")]
		///
		/// Like `array.array(...).frombytes()`, a truncated array is read as far as
		/// it goes, unless the data ends inside an item.
		#[allow(clippy::modulo_one)]
		pub fn $array(&mut self, count: usize) -> Result<Vec<$t>> {
			let bytes = self.take_partial(count * $n);
			if bytes.len() % $n != 0 {
				return Err(Error::value("bytes length not a multiple of item size"));
			}
			Ok(bytes
				.chunks_exact($n)
				.map(|c| {
					let mut a = [0u8; $n];
					a.copy_from_slice(c);
					<$t>::from_be_bytes(a)
				})
				.collect())
		}
	};
}

impl<'a> OtReader<'a> {
	/// A reader at the start of a table.
	pub fn new(data: &'a [u8], table_tag: Tag) -> Self {
		OtReader {
			data,
			offset: 0,
			pos: 0,
			state: Rc::new(State::new()),
			table_tag,
		}
	}

	/// The current position, from the start of the table data.
	pub fn pos(&self) -> usize {
		self.pos
	}

	/// Moves to an absolute position.
	pub fn seek(&mut self, pos: usize) {
		self.pos = pos;
	}

	/// Skips bytes.
	pub fn advance(&mut self, count: usize) {
		self.pos += count;
	}

	/// The whole table data.
	pub fn data(&self) -> &'a [u8] {
		self.data
	}

	/// A reader for the subtable at `offset` from the current subtable
	/// (`getSubReader`). It shares the propagated state.
	pub fn sub_reader(&self, offset: usize) -> Self {
		let offset = self.offset + offset;
		OtReader {
			data: self.data,
			offset,
			pos: offset,
			state: self.state.clone(),
			table_tag: self.table_tag,
		}
	}

	fn take(&mut self, n: usize) -> Result<&'a [u8]> {
		let s = self
			.data
			.get(self.pos..self.pos + n)
			.ok_or_else(|| short(n))?;
		self.pos += n;
		Ok(s)
	}

	/// The bytes from the current position, as many as are available up to `n`
	/// (Python slicing); the position advances by `n` regardless.
	fn take_partial(&mut self, n: usize) -> &'a [u8] {
		let start = self.pos.min(self.data.len());
		let end = (self.pos + n).min(self.data.len());
		self.pos += n;
		&self.data[start..end]
	}

	read_int!(read_u8, read_u8_array, u8, 1);
	read_int!(read_i8, read_i8_array, i8, 1);
	read_int!(read_u16, read_u16_array, u16, 2);
	read_int!(read_i16, read_i16_array, i16, 2);
	read_int!(read_u32, read_u32_array, u32, 4);
	read_int!(read_i32, read_i32_array, i32, 4);

	/// Reads an unsigned 24-bit integer (`readUInt24`).
	pub fn read_u24(&mut self) -> Result<u32> {
		let b = self.take(3)?;
		Ok(u32::from_be_bytes([0, b[0], b[1], b[2]]))
	}

	/// Reads a signed 24-bit integer (`readInt24`).
	pub fn read_i24(&mut self) -> Result<i32> {
		let b = self.take(3)?;
		let pad = if b[0] & 0x80 != 0 { 0xFF } else { 0 };
		Ok(i32::from_be_bytes([pad, b[0], b[1], b[2]]))
	}

	/// Reads `count` unsigned 24-bit integers (`readUInt24Array`).
	pub fn read_u24_array(&mut self, count: usize) -> Result<Vec<u32>> {
		(0..count).map(|_| self.read_u24()).collect()
	}

	/// Reads a tag (`readTag`).
	pub fn read_tag(&mut self) -> Result<Tag> {
		let b = self.take(4)?;
		Ok(Tag([b[0], b[1], b[2], b[3]]))
	}

	/// Reads `count` bytes (`readData`); fewer at the end of the data.
	pub fn read_data(&mut self, count: usize) -> &'a [u8] {
		self.take_partial(count)
	}

	/// A propagated value (`reader[name]`).
	pub fn get(&self, name: &str) -> Option<&StateValue> {
		self.state.get(name)
	}

	/// Whether no value is propagated (`not reader.localState`).
	pub fn state_is_empty(&self) -> bool {
		self.state.is_empty()
	}

	/// Whether a value is propagated (`name in reader`).
	pub fn contains(&self, name: &str) -> bool {
		self.state.contains(name)
	}

	/// Propagates a value to subtables read from now on (`reader[name] = value`).
	pub fn set(&mut self, name: &'static str, value: StateValue) {
		Rc::make_mut(&mut self.state).set(name, value);
	}
}
