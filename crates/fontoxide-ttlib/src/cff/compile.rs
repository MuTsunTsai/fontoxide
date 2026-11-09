//! Compiling CFF font sets (`CFFFontSet.compile`, `CFFWriter` and the
//! compilers of `cffLib`).

use std::collections::HashMap;

use fontoxide_misc::Tag;

use super::charstring::{T2CharString, encode_int_cff, index_error};
use super::dict::{
	Arg, Conv, Operator, PRIVATE_DICT, TOP_DICT, Value, encode_arg_delta, encode_arg_number,
	opcode_bytes, operator,
};
use super::font::{
	CffFontSet, DictKind, Encoding, FdSelect, FontDict, PrivateDict, Strings, TopDict,
	dict_default, dict_order, recalc_font_bbox,
};
use crate::generated::cff_strings::{EXPERT, EXPERT_SUBSET, ISO_ADOBE};
use crate::ot::table::OtTable;
use crate::ot::writer::WriterGraph;
use crate::{Error, Font, Result};

/// `maxStackLimit`.
const MAX_STACK_LIMIT: i64 = 513;

/// `calcOffSize`.
fn calc_off_size(largest: usize) -> usize {
	match largest {
		0..0x100 => 1,
		0x100..0x10000 => 2,
		0x10000..0x1000000 => 3,
		_ => 4,
	}
}

/// `IndexCompiler.getDataLength` for items of these lengths.
fn index_length(lengths: impl IntoIterator<Item = usize>, cff2: bool) -> usize {
	let count_size = if cff2 { 4 } else { 2 };
	let mut n = 0;
	let mut last = 1;
	for l in lengths {
		n += 1;
		last += l;
	}
	if n == 0 {
		return count_size;
	}
	count_size + 1 + (n + 1) * calc_off_size(last) + last - 1
}

fn struct_error(what: &str) -> Error {
	Error::Struct(format!("argument out of range ({what})"))
}

/// `IndexCompiler.toFile`: the count, offsets and data of the items. An empty
/// INDEX of an FDArray has no offsets to size.
fn write_index(out: &mut Vec<u8>, items: &[Vec<u8>], cff2: bool, fd_array: bool) -> Result<()> {
	if cff2 {
		let n = u32::try_from(items.len()).map_err(|_| struct_error("card32"))?;
		out.extend_from_slice(&n.to_be_bytes());
	} else {
		let n = u16::try_from(items.len()).map_err(|_| struct_error("card16"))?;
		out.extend_from_slice(&n.to_be_bytes());
	}
	if items.is_empty() {
		if fd_array {
			return Err(Error::ttlib("IndexError: list index out of range"));
		}
		return Ok(());
	}
	let mut offsets = Vec::with_capacity(items.len() + 1);
	let mut pos = 1usize;
	offsets.push(pos);
	for item in items {
		pos += item.len();
		offsets.push(pos);
	}
	let off_size = calc_off_size(pos);
	out.push(off_size as u8);
	for offset in offsets {
		let bytes = (offset as i32).to_be_bytes();
		out.extend_from_slice(&bytes[4 - off_size..]);
	}
	for item in items {
		out.extend_from_slice(item);
	}
	Ok(())
}

/// A DICT being compiled (`DictCompiler`).
#[derive(Debug)]
struct DictComp {
	table: &'static [Operator],
	order: Vec<&'static str>,
	raw: HashMap<&'static str, Value>,
	max_blend_stack: i64,
	pos: usize,
}

impl DictComp {
	/// `DictCompiler.__init__`: the entries that differ from the defaults, with
	/// the converters' values.
	fn new(kind: DictKind, cff2: bool, entries: Vec<(&'static str, Value)>) -> Self {
		Self::with_defaults(kind, cff2, cff2, entries)
	}

	/// With the defaults of another format than the order, as in a CFF2 Private DICT
	/// that was converted to CFF.
	fn with_defaults(
		kind: DictKind,
		cff2: bool,
		defaults_cff2: bool,
		entries: Vec<(&'static str, Value)>,
	) -> Self {
		let mut raw = HashMap::new();
		for (name, value) in entries {
			if kind != DictKind::Font
				&& let Some(default) = dict_default(kind, defaults_cff2, name)
				&& default.py_eq(&value)
			{
				continue;
			}
			raw.insert(name, value);
		}
		DictComp {
			table: if kind == DictKind::Private {
				PRIVATE_DICT
			} else {
				TOP_DICT
			},
			order: dict_order(kind, cff2),
			raw,
			max_blend_stack: if kind == DictKind::Private {
				MAX_STACK_LIMIT
			} else {
				0
			},
			pos: 0,
		}
	}

	/// `DictCompiler.compile`.
	fn compile(&self, strings: &mut Option<Strings>) -> Result<Vec<u8>> {
		let mut out = Vec::new();
		for name in &self.order {
			let Some(value) = self.raw.get(name) else {
				continue;
			};
			let op = operator(self.table, name)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {name:?}")))?;
			let parts = |n: usize| -> Result<&[Value]> {
				match value {
					Value::Tuple(t) | Value::List(t) if t.len() == n => Ok(t),
					_ => Err(Error::Assertion("value doesn't match arg type".into())),
				}
			};
			match op.arg {
				Arg::Number => encode_arg_number(value, &mut out)?,
				Arg::Sid => encode_sid(value, strings, &mut out)?,
				Arg::Array => {
					let Value::List(items) = value else {
						return Err(Error::ttlib("TypeError: array value is not iterable"));
					};
					for item in items {
						encode_arg_number(item, &mut out)?;
					}
				}
				Arg::Delta => encode_arg_delta(value, self.max_blend_stack, &mut out)?,
				Arg::NumberPair => {
					for v in parts(2)? {
						encode_arg_number(v, &mut out)?;
					}
				}
				Arg::Ros => {
					let t = parts(3)?;
					encode_sid(&t[0], strings, &mut out)?;
					encode_sid(&t[1], strings, &mut out)?;
					encode_arg_number(&t[2], &mut out)?;
				}
				Arg::BlendList => continue,
			}
			opcode_bytes(op.code, &mut out);
		}
		Ok(out)
	}
}

/// `arg_SID`.
fn encode_sid(value: &Value, strings: &mut Option<Strings>, out: &mut Vec<u8>) -> Result<()> {
	let strings = strings.as_mut().ok_or_else(|| {
		Error::ttlib("AttributeError: 'NoneType' object has no attribute 'getSID'")
	})?;
	let Value::Str(s) = value else {
		return Err(Error::ttlib(format!(
			"TypeError: expected a string, got {value:?}"
		)));
	};
	encode_int_cff(strings.sid(s) as i64, out);
	Ok(())
}

/// Where an item puts its position (`setPos`).
#[derive(Debug, Clone, Copy)]
enum SetPos {
	None,
	/// The offset under a key of a DICT.
	Key(usize, &'static str),
	/// `(size, offset)` of a Private DICT in its parent, and its position.
	Private {
		dict: usize,
		parent: usize,
	},
	/// The offset of local subroutines from their Private DICT.
	Subrs(usize),
}

/// An item of the CFF data (`CFFWriter.data`).
#[derive(Debug)]
enum Item {
	Bytes(Vec<u8>),
	Index {
		items: Vec<Vec<u8>>,
		set_pos: SetPos,
	},
	Strings,
	TopDicts(Vec<usize>),
	FdArray {
		dicts: Vec<usize>,
		parent: usize,
	},
	Data {
		data: Vec<u8>,
		set_pos: SetPos,
	},
	Private {
		dict: usize,
		parent: usize,
	},
}

#[derive(Debug)]
struct Writer {
	cff2: bool,
	items: Vec<Item>,
	dicts: Vec<DictComp>,
	strings: Option<Strings>,
}

impl Writer {
	fn add_dict(&mut self, dict: DictComp) -> usize {
		self.dicts.push(dict);
		self.dicts.len() - 1
	}

	fn dict_length(&mut self, id: usize) -> Result<usize> {
		Ok(self.dicts[id].compile(&mut self.strings)?.len())
	}

	/// `getDataLength` of an item.
	fn length(&mut self, i: usize) -> Result<usize> {
		let cff2 = self.cff2;
		let dicts = match &self.items[i] {
			Item::Bytes(b) => return Ok(b.len()),
			Item::Index { items, .. } => return Ok(index_length(items.iter().map(Vec::len), cff2)),
			Item::Strings => {
				let strings = self.strings.as_ref().map_or(&[][..], |s| &s.strings[..]);
				return Ok(index_length(
					strings.iter().map(|s| s.chars().count()),
					false,
				));
			}
			Item::Data { data, .. } => return Ok(data.len()),
			Item::Private { dict, .. } => return self.dict_length(*dict),
			Item::TopDicts(d) if cff2 => {
				let first = *d.first().ok_or_else(index_error)?;
				return self.dict_length(first);
			}
			Item::TopDicts(d) => d.clone(),
			Item::FdArray { dicts, .. } => dicts.clone(),
		};
		let mut lengths = Vec::with_capacity(dicts.len());
		for d in dicts {
			lengths.push(self.dict_length(d)?);
		}
		Ok(index_length(lengths, cff2))
	}

	fn set_pos(&mut self, i: usize, pos: usize, end: usize) {
		let set_pos = match &self.items[i] {
			Item::Index { set_pos, .. } | Item::Data { set_pos, .. } => *set_pos,
			Item::FdArray { parent, .. } => SetPos::Key(*parent, "FDArray"),
			Item::Private { dict, parent } => SetPos::Private {
				dict: *dict,
				parent: *parent,
			},
			_ => SetPos::None,
		};
		match set_pos {
			SetPos::None => {}
			SetPos::Key(d, key) => {
				self.dicts[d].raw.insert(key, Value::Int(pos as i64));
			}
			SetPos::Private { dict, parent } => {
				self.dicts[parent].raw.insert(
					"Private",
					Value::Tuple(vec![Value::Int((end - pos) as i64), Value::Int(pos as i64)]),
				);
				self.dicts[dict].pos = pos;
			}
			SetPos::Subrs(d) => {
				let offset = pos as i64 - self.dicts[d].pos as i64;
				self.dicts[d].raw.insert("Subrs", Value::Int(offset));
			}
		}
	}
}

impl Writer {
	/// `CFFWriter.toFile`.
	fn into_bytes(mut self) -> Result<Vec<u8>> {
		let mut last: Option<Vec<usize>> = None;
		let mut top_dict_size = 0;
		loop {
			let mut pos = 0;
			let mut list = vec![pos];
			for i in 0..self.items.len() {
				let len = self.length(i)?;
				if self.cff2 && matches!(self.items[i], Item::TopDicts(_)) {
					top_dict_size = len;
				}
				let end = pos + len;
				self.set_pos(i, pos, end);
				pos = end;
				list.push(pos);
			}
			if last.as_ref() == Some(&list) {
				break;
			}
			last = Some(list);
		}
		let last = last.unwrap_or_default();
		if self.cff2 {
			let size = u16::try_from(top_dict_size)
				.map_err(|_| struct_error("'H' format requires 0 <= number <= 65535"))?;
			self.items[1] = Item::Bytes(size.to_be_bytes().to_vec());
		} else {
			let off_size = calc_off_size(*last.last().unwrap_or(&0));
			self.items[1] = Item::Bytes(vec![off_size as u8]);
		}
		let mut out = Vec::with_capacity(*last.last().unwrap_or(&0));
		let mut positions = vec![0];
		for i in 0..self.items.len() {
			self.write_item(i, &mut out)?;
			positions.push(out.len());
		}
		if positions != last {
			return Err(Error::Assertion("posList == lastPosList".into()));
		}
		Ok(out)
	}

	fn write_item(&mut self, i: usize, out: &mut Vec<u8>) -> Result<()> {
		let cff2 = self.cff2;
		let dicts = match &self.items[i] {
			Item::Bytes(b) | Item::Data { data: b, .. } => {
				out.extend_from_slice(b);
				return Ok(());
			}
			Item::Index { items, .. } => return write_index(out, items, cff2, false),
			Item::Strings => {
				let strings = self.strings.as_ref().map_or(&[][..], |s| &s.strings[..]);
				let mut items = Vec::with_capacity(strings.len());
				for s in strings {
					items.push(latin1_encode(s)?);
				}
				return write_index(out, &items, false, false);
			}
			Item::Private { dict, .. } => {
				let data = self.dicts[*dict].compile(&mut self.strings)?;
				out.extend_from_slice(&data);
				return Ok(());
			}
			Item::TopDicts(d) if cff2 => {
				let first = *d.first().ok_or_else(index_error)?;
				let data = self.dicts[first].compile(&mut self.strings)?;
				out.extend_from_slice(&data);
				return Ok(());
			}
			Item::TopDicts(d) => (d.clone(), false),
			Item::FdArray { dicts, .. } => (dicts.clone(), true),
		};
		let (ids, fd_array) = dicts;
		let mut items = Vec::with_capacity(ids.len());
		for d in ids {
			items.push(self.dicts[d].compile(&mut self.strings)?);
		}
		write_index(out, &items, cff2, fd_array)
	}
}

/// `tobytes(s, encoding="latin1")`.
fn latin1_encode(s: &str) -> Result<Vec<u8>> {
	s.chars()
		.map(|c| {
			u8::try_from(c as u32).map_err(|_| {
				Error::ttlib(format!(
					"UnicodeEncodeError: 'latin-1' codec can't encode character {c:?}"
				))
			})
		})
		.collect()
}

/// `getStdCharSet`.
fn std_charset(charset: &[String]) -> Option<i64> {
	let predefined: [(&[&str], i64); 3] = [(&ISO_ADOBE, 0), (&EXPERT, 1), (&EXPERT_SUBSET, 2)];
	for (strings, code) in predefined {
		if charset.len() > strings.len() {
			continue;
		}
		// The first predefined charset that is long enough decides.
		return charset
			.iter()
			.zip(strings)
			.all(|(a, b)| a == b)
			.then_some(code);
	}
	None
}

/// The SID (or CID) of a glyph name in a charset.
fn charset_id(name: &str, cid: bool, strings: &mut Option<Strings>) -> Result<i64> {
	if cid {
		let digits = name.get(3..).unwrap_or_default();
		return fontoxide_misc::py::parse_int(digits)
			.and_then(|v| i64::try_from(v).ok())
			.ok_or_else(|| {
				Error::value(format!(
					"invalid literal for int() with base 10: {digits:?}"
				))
			});
	}
	let strings = strings.as_mut().ok_or_else(|| {
		Error::ttlib("AttributeError: 'NoneType' object has no attribute 'getSID'")
	})?;
	Ok(strings.sid(name) as i64)
}

fn card16(v: i64, out: &mut Vec<u8>) -> Result<()> {
	let v =
		u16::try_from(v).map_err(|_| struct_error("'H' format requires 0 <= number <= 65535"))?;
	out.extend_from_slice(&v.to_be_bytes());
	Ok(())
}

fn card8(v: i64, out: &mut Vec<u8>) -> Result<()> {
	out.push(u8::try_from(v).map_err(|_| Error::value("bytes must be in range(0, 256)"))?);
	Ok(())
}

/// `CharsetCompiler`: the shorter of `packCharset0` and `packCharset`.
fn pack_charset(charset: &[String], cid: bool, strings: &mut Option<Strings>) -> Result<Vec<u8>> {
	if charset.first().map(String::as_str) != Some(".notdef") {
		return Err(Error::Assertion("charset[0] == '.notdef'".into()));
	}
	let mut data0 = vec![0];
	for name in &charset[1..] {
		card16(charset_id(name, cid, strings)?, &mut data0)?;
	}
	let mut format = 1;
	let mut ranges: Vec<(i64, i64)> = Vec::new();
	let mut first: Option<i64> = None;
	let mut end = 0;
	for name in &charset[1..] {
		let id = charset_id(name, cid, strings)?;
		match first {
			None => first = Some(id),
			Some(f) if end + 1 != id => {
				let left = end - f;
				if left > 255 {
					format = 2;
				}
				ranges.push((f, left));
				first = Some(id);
			}
			Some(_) => {}
		}
		end = id;
	}
	// UPSTREAM-BUG(B16): `if end:` drops the last range when its last ID is 0.
	if end != 0 {
		let f = first.unwrap_or(0);
		let left = end - f;
		if left > 255 {
			format = 2;
		}
		ranges.push((f, left));
	}
	let mut data = vec![format];
	for (f, left) in ranges {
		card16(f, &mut data)?;
		if format == 1 {
			card8(left, &mut data)?;
		} else {
			card16(left, &mut data)?;
		}
	}
	Ok(if data.len() < data0.len() {
		data
	} else {
		data0
	})
}

/// `EncodingCompiler`: the shorter of `packEncoding0` and `packEncoding1`.
fn pack_encoding(charset: &[String], encoding: &[String]) -> Result<Vec<u8>> {
	let mut codes_by_name: HashMap<&str, i64> = HashMap::new();
	for (code, name) in encoding.iter().enumerate() {
		if name != ".notdef" {
			codes_by_name.insert(name, code as i64);
		}
	}
	let rest = charset.get(1..).unwrap_or_default();
	// Format 0.
	let mut codes: Vec<Option<i64>> = rest
		.iter()
		.map(|n| codes_by_name.get(n.as_str()).copied())
		.collect();
	while codes.last() == Some(&None) {
		codes.pop();
	}
	let mut data0 = vec![0];
	card8(codes.len() as i64, &mut data0)?;
	for code in codes {
		card8(code.unwrap_or(0), &mut data0)?;
	}
	// Format 1.
	let mut ranges: Vec<(i64, i64)> = Vec::new();
	let mut first: Option<i64> = None;
	let mut end = 0;
	for name in rest {
		let code = codes_by_name.get(name.as_str()).copied().unwrap_or(-1);
		match first {
			None => first = Some(code),
			Some(f) if end + 1 != code => {
				ranges.push((f, end - f));
				first = Some(code);
			}
			Some(_) => {}
		}
		end = code;
	}
	let Some(f) = first else {
		return Err(Error::ttlib(
			"TypeError: unsupported operand type(s) for -: 'int' and 'NoneType'",
		));
	};
	ranges.push((f, end - f));
	while ranges.last().is_some_and(|r| r.0 == -1) {
		ranges.pop();
	}
	let mut data1 = vec![1];
	card8(ranges.len() as i64, &mut data1)?;
	for (first, left) in ranges {
		card8(if first == -1 { 0 } else { first }, &mut data1)?;
		card8(left, &mut data1)?;
	}
	Ok(if data0.len() < data1.len() {
		data0
	} else {
		data1
	})
}

fn fd(v: Option<i64>) -> Result<i64> {
	v.ok_or_else(|| {
		Error::ttlib("TypeError: 'NoneType' object cannot be interpreted as an integer")
	})
}

/// `packFDSelect3` / `packFDSelect4`.
fn pack_fd_select_ranges(array: &[Option<i64>], format: u8) -> Result<Vec<u8>> {
	if array.is_empty() {
		return Err(Error::ttlib(
			"UnboundLocalError: cannot access local variable 'i'",
		));
	}
	let mut ranges: Vec<(usize, Option<i64>)> = Vec::new();
	let mut last: Option<Option<i64>> = None;
	for (i, &v) in array.iter().enumerate() {
		if last != Some(v) {
			ranges.push((i, v));
			last = Some(v);
		}
	}
	let mut data = vec![format];
	let glyph = |v: usize, data: &mut Vec<u8>| -> Result<()> {
		if format == 3 {
			card16(v as i64, data)
		} else {
			let v = u32::try_from(v)
				.map_err(|_| struct_error("'L' format requires 0 <= number <= 4294967295"))?;
			data.extend_from_slice(&v.to_be_bytes());
			Ok(())
		}
	};
	glyph(ranges.len(), &mut data)?;
	for (first, v) in ranges {
		glyph(first, &mut data)?;
		if format == 3 {
			card8(fd(v)?, &mut data)?;
		} else {
			card16(fd(v)?, &mut data)?;
		}
	}
	glyph(array.len(), &mut data)?;
	Ok(data)
}

/// `FDSelectCompiler`; picks the smaller format when there is none, and
/// records it.
fn pack_fd_select(fd_select: &mut FdSelect) -> Result<Vec<u8>> {
	let pack0 = |array: &[Option<i64>]| -> Result<Vec<u8>> {
		let mut data = vec![0];
		for &v in array {
			card8(fd(v)?, &mut data)?;
		}
		Ok(data)
	};
	match fd_select.format {
		Some(0) => pack0(&fd_select.gid_array),
		Some(3) => pack_fd_select_ranges(&fd_select.gid_array, 3),
		Some(4) => pack_fd_select_ranges(&fd_select.gid_array, 4),
		_ => {
			let data0 = pack0(&fd_select.gid_array)?;
			let data3 = pack_fd_select_ranges(&fd_select.gid_array, 3)?;
			if data0.len() < data3.len() {
				fd_select.format = Some(0);
				Ok(data0)
			} else {
				fd_select.format = Some(3);
				Ok(data3)
			}
		}
	}
}

/// The entries of a DICT as `DictCompiler` reads them (`getattr` and
/// `conv.write`), in order.
fn plain_entries(
	kind: DictKind,
	cff2: bool,
	get: impl Fn(&str) -> Option<Value>,
	special: impl Fn(&str, Conv) -> Option<Value>,
) -> Vec<(&'static str, Value)> {
	let table = if kind == DictKind::Private {
		PRIVATE_DICT
	} else {
		TOP_DICT
	};
	let mut out = Vec::new();
	for name in dict_order(kind, cff2) {
		let Some(op) = operator(table, name) else {
			continue;
		};
		let value = match op.conv {
			Conv::Number | Conv::Array | Conv::Ascii | Conv::Latin1 | Conv::Ros => get(name),
			Conv::None => None,
			conv => special(name, conv),
		};
		if let Some(v) = value {
			out.push((name, v));
		}
	}
	out
}

fn private_comp(private: &PrivateDict) -> DictComp {
	let order_cff2 = private.cff2 && !private.cff1_order;
	let entries = plain_entries(
		DictKind::Private,
		order_cff2,
		|n| private.get(n),
		|_, conv| (conv == Conv::Subrs && private.subrs.is_some()).then_some(Value::Int(0)),
	);
	DictComp::with_defaults(DictKind::Private, order_cff2, private.cff2, entries)
}

fn font_dict_comp(fd: &FontDict) -> DictComp {
	let entries = plain_entries(
		DictKind::Font,
		fd.cff2,
		|n| fd.values.get(n).cloned(),
		|_, conv| {
			(conv == Conv::Private && fd.private.is_some())
				.then(|| Value::Tuple(vec![Value::Int(0), Value::Int(0)]))
		},
	);
	DictComp::new(DictKind::Font, fd.cff2, entries)
}

fn top_dict_comp(top: &TopDict) -> DictComp {
	let entries = plain_entries(
		DictKind::Top,
		top.cff2,
		|n| top.get(n),
		|_, conv| match conv {
			Conv::Charset => top.charset.as_ref().map(|_| Value::Int(0)),
			Conv::Encoding => top
				.encoding
				.as_ref()
				.map(|e| Value::Int(if *e == Encoding::Expert { 1 } else { 0 })),
			Conv::Private => top
				.private
				.as_ref()
				.map(|_| Value::Tuple(vec![Value::Int(0), Value::Int(0)])),
			Conv::FdSelect => top.fd_select.as_ref().map(|_| Value::Int(0)),
			Conv::FdArray => top.fd_array.as_ref().map(|_| Value::Int(0)),
			Conv::CharStrings => top.char_strings.as_ref().map(|_| Value::Int(0)),
			Conv::VarStore => top.var_store.as_ref().map(|_| Value::Int(0)),
			_ => None,
		},
	);
	DictComp::new(DictKind::Top, top.cff2, entries)
}

fn compile_char_strings(items: &mut [T2CharString], cff2: bool) -> Result<Vec<Vec<u8>>> {
	let mut out = Vec::with_capacity(items.len());
	for cs in items {
		cs.compile(cff2)?;
		out.push(cs.bytecode.clone().unwrap_or_default());
	}
	Ok(out)
}

/// A Private DICT and its subroutines (`PrivateDictCompiler.getChildren`).
fn add_private(w: &mut Writer, private: &mut PrivateDict, parent: usize) -> Result<()> {
	let id = w.add_dict(private_comp(private));
	w.items.push(Item::Private { dict: id, parent });
	if let Some(subrs) = &mut private.subrs {
		let items = compile_char_strings(subrs, w.cff2)?;
		w.items.push(Item::Index {
			items,
			set_pos: SetPos::Subrs(id),
		});
	}
	Ok(())
}

/// `TopDictCompiler.getChildren`.
fn add_children(w: &mut Writer, top: &mut TopDict, id: usize) -> Result<()> {
	let cff2 = w.cff2;
	if !cff2 {
		if let Some(charset) = top.charset.as_deref().filter(|c| !c.is_empty()) {
			let code = if top.is_cid() {
				None
			} else {
				std_charset(charset)
			};
			match code {
				None => {
					let data = pack_charset(charset, top.is_cid(), &mut w.strings)?;
					w.items.push(Item::Data {
						data,
						set_pos: SetPos::Key(id, "charset"),
					});
				}
				Some(code) => {
					w.dicts[id].raw.insert("charset", Value::Int(code));
				}
			}
		}
		if let Some(Encoding::Custom(encoding)) = &top.encoding {
			let charset = top
				.charset
				.as_deref()
				.ok_or_else(|| Error::ttlib("TypeError: 'NoneType' object is not subscriptable"))?;
			let data = pack_encoding(charset, encoding)?;
			w.items.push(Item::Data {
				data,
				set_pos: SetPos::Key(id, "Encoding"),
			});
		}
	} else if let Some(store) = &top.var_store {
		let mut graph = WriterGraph::new(Tag::new(b"CFF2"));
		let mut dummy = Font::new();
		store.compile(&mut graph.root(), &mut dummy)?;
		let data = graph.get_all_data()?;
		let mut out = Vec::with_capacity(data.len() + 2);
		out.extend_from_slice(&(data.len().min(0xFFFF) as u16).to_be_bytes());
		out.extend(data);
		w.items.push(Item::Data {
			data: out,
			set_pos: SetPos::Key(id, "VarStore"),
		});
	}
	if top.fd_select.is_some() {
		let charset = top.charset.clone().unwrap_or_default();
		let char_strings = top.char_strings.as_ref();
		let fd_select = top.fd_select.as_mut().ok_or_else(index_error)?;
		if fd_select.gid_array.is_empty() {
			let cs = char_strings.ok_or_else(|| Error::ttlib("AttributeError: CharStrings"))?;
			for name in &charset {
				let i = cs
					.index(name)
					.ok_or_else(|| Error::ttlib(format!("KeyError: {name:?}")))?;
				let index = cs.items[i].fd_select_index.ok_or_else(|| {
					Error::ttlib(
						"AttributeError: 'T2CharString' object has no attribute 'fdSelectIndex'",
					)
				})?;
				fd_select.gid_array.push(Some(index));
			}
		}
		let count = char_strings.map_or(0, |cs| cs.names.len());
		if cff2 && fd_select.gid_array.len() > count && !matches!(fd_select.format, Some(3 | 4)) {
			let max = fd_select
				.gid_array
				.iter()
				.map(|v| v.unwrap_or(0))
				.max()
				.unwrap_or(0);
			fd_select.format = Some(if fd_select.gid_array.len() > 0xFFFF || max > 0xFF {
				4
			} else {
				3
			});
		}
		let data = pack_fd_select(fd_select)?;
		w.items.push(Item::Data {
			data,
			set_pos: SetPos::Key(id, "FDSelect"),
		});
	}
	if let Some(cs) = &mut top.char_strings {
		let charset = top.charset.as_deref().unwrap_or_default();
		let mut items = Vec::with_capacity(charset.len());
		for name in charset {
			let i = cs
				.index(name)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {name:?}")))?;
			cs.items[i].compile(cff2)?;
			items.push(cs.items[i].bytecode.clone().unwrap_or_default());
		}
		w.items.push(Item::Index {
			items,
			set_pos: SetPos::Key(id, "CharStrings"),
		});
	}
	if let Some(fds) = &mut top.fd_array {
		let dicts: Vec<usize> = fds
			.iter()
			.map(|fd| w.add_dict(font_dict_comp(fd)))
			.collect();
		w.items.push(Item::FdArray {
			dicts: dicts.clone(),
			parent: id,
		});
		for (fd, d) in fds.iter_mut().zip(dicts) {
			if let Some(p) = &mut fd.private {
				add_private(w, p, d)?;
			}
		}
	}
	if let Some(p) = &mut top.private {
		add_private(w, p, id)?;
	}
	Ok(())
}

/// What compiling needs from the font.
#[derive(Debug)]
pub struct CompileContext<'a> {
	/// The font's glyph order.
	pub glyph_order: &'a [String],
	/// `recalcBBoxes`.
	pub recalc_bboxes: bool,
	/// Whether the font has a `VARC` table.
	pub has_varc: bool,
}

impl CffFontSet {
	/// `CFFFontSet.compile`.
	pub fn compile(&mut self, cff2: bool, ctx: &CompileContext<'_>) -> Result<Vec<u8>> {
		let expected = if cff2 { 2 } else { 1 };
		if self.major != expected {
			return Err(Error::value(format!(
				"Invalid CFF 'major' version: expected {expected}, found {}",
				self.major
			)));
		}
		if ctx.recalc_bboxes && !cff2 {
			for t in 0..self.top_dicts.len() {
				recalc_font_bbox(&mut self.top_dicts[t], &mut self.global_subrs)?;
			}
		}
		let mut w = Writer {
			cff2,
			items: Vec::new(),
			dicts: Vec::new(),
			strings: (!cff2).then(Strings::default),
		};
		let tops: Vec<usize> = self
			.top_dicts
			.iter()
			.map(|t| w.add_dict(top_dict_comp(t)))
			.collect();
		let header = |hdr: u8| -> Result<Vec<u8>> {
			let byte = |v: i64| {
				u8::try_from(v)
					.map_err(|_| struct_error("ubyte format requires 0 <= number <= 255"))
			};
			Ok(vec![byte(self.major)?, byte(self.minor)?, hdr])
		};
		if cff2 {
			self.hdr_size = 5;
			w.items.push(Item::Bytes(header(5)?));
			w.items.push(Item::Bytes(vec![0, 0]));
		} else {
			self.hdr_size = 4;
			self.off_size = 4;
			w.items.push(Item::Bytes(header(4)?));
			w.items.push(Item::Bytes(vec![4]));
			let mut names = Vec::with_capacity(self.font_names.len());
			for n in &self.font_names {
				names.push(latin1_encode(n)?);
			}
			w.items.push(Item::Index {
				items: names,
				set_pos: SetPos::None,
			});
		}
		w.items.push(Item::TopDicts(tops.clone()));
		if !cff2 {
			w.items.push(Item::Strings);
		}
		let gsubrs = compile_char_strings(&mut self.global_subrs, cff2)?;
		w.items.push(Item::Index {
			items: gsubrs,
			set_pos: SetPos::None,
		});
		for top in &mut self.top_dicts {
			if cff2 {
				let cs = top
					.char_strings
					.as_ref()
					.ok_or_else(|| Error::ttlib("AttributeError: CharStrings"))?;
				let count = cs.names.len();
				let order = ctx.glyph_order;
				if count > order.len() || (count != order.len() && !ctx.has_varc) {
					return Err(Error::value(
						"CFF2 CharStrings count does not match the font",
					));
				}
				let prefix = &order[..count];
				let a: std::collections::HashSet<&String> = prefix.iter().collect();
				let b: std::collections::HashSet<&String> = cs.names.iter().collect();
				if a != b {
					return Err(Error::value(
						"CFF2 CharStrings must form a glyph-order prefix",
					));
				}
				top.charset = Some(prefix.to_vec());
			} else if top.charset.is_none() {
				top.charset = Some(ctx.glyph_order.to_vec());
			}
		}
		for (top, id) in self.top_dicts.iter_mut().zip(tops) {
			add_children(&mut w, top, id)?;
		}
		w.into_bytes()
	}
}
