//! CFF font sets and their DICTs (`fontTools.cffLib`): reading.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use fontoxide_misc::Tag;
use fontoxide_misc::array_tools::{Point, Rect, int_rect, union_rect};
use fontoxide_misc::py::latin1_decode;
use fontoxide_misc::transform::Transform;
use fontoxide_pens::{BoundsPen, GlyphSet, Pen};

use super::charstring::{Blender, Context, Interpreter, T2CharString, index_error};
use super::dict::{
	Conv, DictDecompiler, Operator, PRIVATE_DICT, PRIVATE_DICT2, TOP_DICT, TOP_DICT2, Value,
	operator,
};
use crate::generated::cff_strings::{EXPERT, EXPERT_SUBSET, ISO_ADOBE, STANDARD_STRINGS};
use crate::generated::ot;
use crate::ot::OtReader;
use crate::ot::table::OtTable;
use crate::{Error, Font, Result};

/// The number of standard strings (`cffStandardStringCount`).
pub const STANDARD_STRING_COUNT: usize = 391;

fn struct_error(n: usize) -> Error {
	Error::Struct(format!("unpack requires a buffer of {n} bytes"))
}

/// A position in the CFF data (`file`).
pub(crate) struct Reader<'a> {
	pub data: &'a [u8],
	pub pos: usize,
}

impl<'a> Reader<'a> {
	fn take(&mut self, n: usize) -> Result<&'a [u8]> {
		let b = self
			.data
			.get(self.pos..self.pos + n)
			.ok_or_else(|| struct_error(n))?;
		self.pos += n;
		Ok(b)
	}

	/// `file.read(n)`: as many bytes as there are, up to `n`.
	fn read(&mut self, n: usize) -> &'a [u8] {
		let start = self.pos.min(self.data.len());
		let end = self.pos.saturating_add(n).min(self.data.len());
		self.pos = end.max(start);
		&self.data[start..end]
	}

	pub fn card8(&mut self) -> Result<u8> {
		let b = self.data.get(self.pos).copied().ok_or_else(|| {
			Error::ttlib("TypeError: ord() expected a character, but string of length 0 found")
		})?;
		self.pos += 1;
		Ok(b)
	}

	pub fn card16(&mut self) -> Result<u16> {
		let b = self.take(2)?;
		Ok(u16::from_be_bytes([b[0], b[1]]))
	}

	pub fn card32(&mut self) -> Result<u32> {
		let b = self.take(4)?;
		Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
	}
}

/// Reads an INDEX at the current position (`Index`); the items, and the
/// position after it.
pub(crate) fn read_index(data: &[u8], pos: usize, cff2: bool) -> Result<(Vec<Vec<u8>>, usize)> {
	let mut r = Reader { data, pos };
	let count = if cff2 {
		r.card32()? as usize
	} else {
		r.card16()? as usize
	};
	if count == 0 {
		return Ok((Vec::new(), r.pos));
	}
	let off_size = r.card8()? as usize;
	if off_size > 4 {
		return Err(Error::Assertion(format!("offSize too large: {off_size}")));
	}
	let mut offsets = Vec::with_capacity(count + 1);
	for _ in 0..=count {
		let chunk = r.read(off_size);
		if chunk.len() + (4 - off_size) != 4 {
			return Err(struct_error(4));
		}
		offsets.push(chunk.iter().fold(0usize, |a, &b| a << 8 | b as usize));
	}
	let base = r.pos - 1;
	let mut items = Vec::with_capacity(count);
	for i in 0..count {
		let start = base + offsets[i];
		let size = offsets[i + 1] as i64 - offsets[i] as i64;
		let item = if size < 0 {
			// `file.read(negative)` reads to the end.
			data.get(start.min(data.len())..).unwrap_or_default()
		} else {
			let s = start.min(data.len());
			&data[s..(start + size as usize).min(data.len())]
		};
		if item.len() as i64 != size {
			return Err(Error::Assertion("len(data) == size".into()));
		}
		items.push(item.to_vec());
	}
	Ok((items, base + offsets[count]))
}

/// The custom strings of a CFF font (`IndexedStrings`), after the standard ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Strings {
	/// The strings, by SID minus 391 (`strings`).
	pub strings: Vec<String>,
	mapping: Option<HashMap<String, usize>>,
}

fn standard_mapping() -> &'static HashMap<&'static str, usize> {
	static MAP: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
	MAP.get_or_init(|| {
		STANDARD_STRINGS
			.iter()
			.enumerate()
			.map(|(i, s)| (*s, i))
			.collect()
	})
}

impl Strings {
	/// The string of a SID (`__getitem__`).
	pub fn get(&self, sid: i64) -> Result<String> {
		let index = || Error::ttlib("IndexError: list index out of range");
		if sid < STANDARD_STRING_COUNT as i64 {
			let at = if sid < 0 {
				sid + STANDARD_STRING_COUNT as i64
			} else {
				sid
			};
			return STANDARD_STRINGS
				.get(at as usize)
				.map(|s| (*s).to_owned())
				.ok_or_else(index);
		}
		self.strings
			.get(sid as usize - STANDARD_STRING_COUNT)
			.cloned()
			.ok_or_else(index)
	}

	/// The SID of a string, adding it if needed (`getSID`).
	pub fn sid(&mut self, s: &str) -> usize {
		if let Some(&sid) = standard_mapping().get(s) {
			return sid;
		}
		let strings = &self.strings;
		let mapping = self.mapping.get_or_insert_with(|| {
			strings
				.iter()
				.enumerate()
				.map(|(i, s)| (s.clone(), i + STANDARD_STRING_COUNT))
				.collect()
		});
		if let Some(&sid) = mapping.get(s) {
			return sid;
		}
		let sid = self.strings.len() + STANDARD_STRING_COUNT;
		self.strings.push(s.to_owned());
		mapping.insert(s.to_owned(), sid);
		sid
	}
}

/// The encoding of a Top DICT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Encoding {
	/// `StandardEncoding`.
	Standard,
	/// `ExpertEncoding`.
	Expert,
	/// Another name, from TTX; written as the standard encoding.
	Named(String),
	/// The glyph name of each code.
	Custom(Vec<String>),
}

/// The font DICT of each glyph of a CID-keyed font (`FDSelect`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FdSelect {
	/// The format, if known (`format`).
	pub format: Option<i64>,
	/// The font DICT index of each glyph (`gidArray`).
	pub gid_array: Vec<Option<i64>>,
	/// The number of CharStrings, set when writing TTX (`_numCharStrings`).
	pub num_char_strings: Option<usize>,
}

/// Which private DICT a charstring uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateRef {
	/// The Top DICT's.
	Top,
	/// A font DICT's, by index in the FDArray.
	Fd(usize),
	/// None.
	None,
}

/// The glyphs' charstrings (`CharStrings`), in their order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CharStrings {
	/// The glyph names.
	pub names: Vec<String>,
	/// The charstrings.
	pub items: Vec<T2CharString>,
	/// The private DICT of each charstring.
	pub privates: Vec<PrivateRef>,
	/// Read from a font, where the font DICT of a glyph comes from the FDSelect.
	pub indexed: bool,
	/// The FDSelect used when reading (`charStringsIndex.fdSelect`).
	pub fd_select: Option<Vec<Option<i64>>>,
	/// Whether there is an FDArray (`hasattr(self, "fdArray")`).
	pub has_fd_array: bool,
	/// Whether there is an FDSelect (`hasattr(self, "fdSelect")`).
	pub has_fd_select: bool,
}

impl CharStrings {
	/// The index of a glyph.
	pub fn index(&self, name: &str) -> Option<usize> {
		self.names.iter().position(|n| n == name)
	}

	/// Sets a glyph's charstring (`__setitem__`), keeping its place.
	pub fn set(&mut self, name: String, cs: T2CharString, private: PrivateRef) {
		match self.index(&name) {
			Some(i) => {
				self.items[i] = cs;
				self.privates[i] = private;
			}
			None => {
				self.names.push(name);
				self.items.push(cs);
				self.privates.push(private);
			}
		}
	}

	/// The FD of a glyph for TTX (`getItemAndSelector`).
	pub fn selector(&self, i: usize) -> Result<Option<i64>> {
		if self.indexed {
			return Ok(match &self.fd_select {
				Some(fd) => fd.get(i).copied().ok_or_else(index_error)?,
				None => None,
			});
		}
		if !self.has_fd_array {
			return Ok(None);
		}
		if !self.has_fd_select {
			return Ok(Some(0));
		}
		self.items[i].fd_select_index.map(Some).ok_or_else(|| {
			Error::ttlib("AttributeError: 'T2CharString' object has no attribute 'fdSelectIndex'")
		})
	}
}

/// What kind of DICT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictKind {
	/// A Top DICT.
	Top,
	/// A font DICT of the FDArray.
	Font,
	/// A Private DICT.
	Private,
}

/// A Private DICT (`PrivateDict`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrivateDict {
	/// Whether this is CFF2.
	pub cff2: bool,
	/// The entries as read (`rawDict`).
	pub raw: Vec<(&'static str, Value)>,
	/// The values.
	pub values: BTreeMap<&'static str, Value>,
	/// The local subroutines (`Subrs`).
	pub subrs: Option<Vec<T2CharString>>,
}

/// A font DICT of the FDArray (`FontDict`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FontDict {
	/// Whether this is CFF2.
	pub cff2: bool,
	/// The entries as read (`rawDict`).
	pub raw: Vec<(&'static str, Value)>,
	/// The values.
	pub values: BTreeMap<&'static str, Value>,
	/// The Private DICT (`Private`).
	pub private: Option<Box<PrivateDict>>,
}

/// A Top DICT (`TopDict`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TopDict {
	/// Whether this is CFF2.
	pub cff2: bool,
	/// The entries as read (`rawDict`).
	pub raw: Vec<(&'static str, Value)>,
	/// The plain values.
	pub values: BTreeMap<&'static str, Value>,
	/// The glyph names (`charset`).
	pub charset: Option<Vec<String>>,
	/// `Encoding`.
	pub encoding: Option<Encoding>,
	/// `Private`.
	pub private: Option<Box<PrivateDict>>,
	/// `FDArray`.
	pub fd_array: Option<Vec<FontDict>>,
	/// `FDSelect`.
	pub fd_select: Option<FdSelect>,
	/// `CharStrings`.
	pub char_strings: Option<CharStrings>,
	/// `VarStore`.
	pub var_store: Option<Box<ot::VarStore>>,
	/// The number of glyphs, from the CharStrings INDEX (`numGlyphs`).
	pub num_glyphs: Option<usize>,
	/// From TTX of CFF2: the glyph order is the font's.
	pub uses_font_glyph_order: bool,
}

/// The order of a DICT's entries (`order`).
pub fn dict_order(kind: DictKind, cff2: bool) -> Vec<&'static str> {
	match (kind, cff2) {
		(DictKind::Top, false) => TOP_DICT.iter().map(|o| o.name).collect(),
		(DictKind::Top, true) => TOP_DICT2.to_vec(),
		(DictKind::Font, false) => vec!["FontName", "FontMatrix", "Weight", "Private"],
		(DictKind::Font, true) => vec!["Private"],
		(DictKind::Private, false) => PRIVATE_DICT.iter().map(|o| o.name).collect(),
		(DictKind::Private, true) => PRIVATE_DICT2.to_vec(),
	}
}

/// The operator table of a DICT kind.
pub fn dict_table(kind: DictKind) -> &'static [Operator] {
	match kind {
		DictKind::Private => PRIVATE_DICT,
		_ => TOP_DICT,
	}
}

/// The default of an entry (`defaults`).
pub fn dict_default(kind: DictKind, cff2: bool, name: &str) -> Option<Value> {
	if kind == DictKind::Font {
		return None;
	}
	if !dict_order(kind, cff2).contains(&name) {
		return None;
	}
	operator(dict_table(kind), name).and_then(|o| o.default.map(|f| f()))
}

fn raw_get<'v>(raw: &'v [(&'static str, Value)], name: &str) -> Option<&'v Value> {
	raw.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
}

fn as_offset(v: &Value) -> Result<usize> {
	usize::try_from(v.int()?).map_err(|_| Error::ttlib("ValueError: negative seek position"))
}

/// The number of regions of a variation data (`VarStoreData.getNumRegions`).
pub fn num_regions(store: Option<&ot::VarStore>, vsindex: i64) -> Result<i64> {
	let store = store.ok_or_else(|| {
		Error::ttlib("AttributeError: 'NoneType' object has no attribute 'getNumRegions'")
	})?;
	let data = usize::try_from(vsindex)
		.ok()
		.and_then(|i| store.var_data.get(i))
		.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?
		.as_deref()
		.ok_or_else(|| {
			Error::ttlib("AttributeError: 'NoneType' object has no attribute 'VarRegionCount'")
		})?;
	let count = data
		.var_region_count
		.unwrap_or(data.var_region_index.len() as u32);
	Ok(count as i64)
}

/// The values of the entries of `order` that are plain values.
fn plain_values(
	raw: &[(&'static str, Value)],
	kind: DictKind,
	cff2: bool,
) -> BTreeMap<&'static str, Value> {
	let table = dict_table(kind);
	let mut values = BTreeMap::new();
	for name in dict_order(kind, cff2) {
		let Some(op) = operator(table, name) else {
			continue;
		};
		if matches!(
			op.conv,
			Conv::Number | Conv::Array | Conv::Ascii | Conv::Latin1 | Conv::Ros
		) && let Some(v) = raw_get(raw, name)
		{
			values.insert(name, v.clone());
		}
	}
	values
}

impl PrivateDict {
	/// Reads a Private DICT at `offset` (`PrivateDictConverter._read`).
	pub(crate) fn read(
		data: &[u8],
		size: usize,
		offset: usize,
		strings: Option<&Strings>,
		cff2: bool,
		vstore: Option<&ot::VarStore>,
	) -> Result<Self> {
		let dict = data
			.get(offset..offset + size)
			.ok_or_else(|| Error::Assertion("len(data) == size".into()))?;
		let sid = |s: i64| {
			strings
				.ok_or_else(|| Error::ttlib("TypeError: 'NoneType' object is not subscriptable"))?
				.get(s)
		};
		let regions = |vi: i64| num_regions(vstore, vi);
		let raw = DictDecompiler::new(PRIVATE_DICT, Some(&sid), Some(&regions)).decompile(dict)?;
		let values = plain_values(&raw, DictKind::Private, cff2);
		let subrs = match raw_get(&raw, "Subrs") {
			Some(v) if dict_order(DictKind::Private, cff2).contains(&"Subrs") => {
				let at = offset + as_offset(v)?;
				let (items, _) = read_index(data, at, cff2)?;
				Some(items.into_iter().map(T2CharString::from_bytecode).collect())
			}
			_ => None,
		};
		Ok(PrivateDict {
			cff2,
			raw,
			values,
			subrs,
		})
	}

	/// A value, or its default (`getattr`).
	pub fn get(&self, name: &str) -> Option<Value> {
		self.values
			.get(name)
			.cloned()
			.or_else(|| dict_default(DictKind::Private, self.cff2, name))
	}

	fn number(&self, name: &str) -> Option<f64> {
		self.get(name).and_then(|v| v.number())
	}
}

/// Makes glyph names unique by appending `.n` (`CharsetConverter._read`).
fn unique_names(charset: Vec<String>) -> Vec<String> {
	let mut all: HashMap<String, usize> = HashMap::new();
	let mut out = Vec::with_capacity(charset.len());
	for name in &charset {
		let mut name = name.clone();
		if let Some(&n) = all.get(&name) {
			let mut n = n;
			let names: HashSet<&str> = all
				.keys()
				.map(String::as_str)
				.chain(charset.iter().map(String::as_str))
				.collect();
			while names.contains(format!("{name}.{n}").as_str()) {
				n += 1;
			}
			all.insert(name.clone(), n + 1);
			name = format!("{name}.{n}");
		}
		all.insert(name.clone(), 1);
		out.push(name);
	}
	out
}

fn cid_name(cid: u32) -> String {
	format!("cid{cid:05}")
}

impl TopDict {
	/// A plain value, or its default (`getattr`).
	pub fn get(&self, name: &str) -> Option<Value> {
		self.values
			.get(name)
			.cloned()
			.or_else(|| dict_default(DictKind::Top, self.cff2, name))
	}

	/// Whether the font is CID-keyed (`hasattr(self, "ROS")`).
	pub fn is_cid(&self) -> bool {
		self.values.contains_key("ROS")
	}

	/// Reads a Top DICT (`TopDict.decompile` and its converters).
	///
	/// LENIENT(L17): fontTools reads the entries when they are first accessed;
	/// here all of them are read at once.
	pub(crate) fn read(
		data: &[u8],
		dict: &[u8],
		strings: Option<&Strings>,
		cff2: bool,
		glyph_order: Option<&[String]>,
	) -> Result<Self> {
		let sid = |s: i64| {
			strings
				.ok_or_else(|| Error::ttlib("TypeError: 'NoneType' object is not subscriptable"))?
				.get(s)
		};
		let raw = DictDecompiler::new(TOP_DICT, Some(&sid), None).decompile(dict)?;
		let mut top = TopDict {
			cff2,
			values: plain_values(&raw, DictKind::Top, cff2),
			raw,
			..Default::default()
		};
		if cff2 {
			top.charset = glyph_order.map(<[String]>::to_vec);
		}
		// `postDecompile`: the number of glyphs.
		if let Some(v) = raw_get(&top.raw, "CharStrings") {
			let mut r = Reader {
				data,
				pos: as_offset(v)?,
			};
			if cff2 {
				let n = r.card32()? as usize;
				let charset = top.charset.as_mut().ok_or_else(|| {
					Error::ttlib("TypeError: object of type 'NoneType' has no len()")
				})?;
				if n > charset.len() {
					return Err(Error::value(
						"CFF2 CharStrings count exceeds the font glyph count",
					));
				}
				charset.truncate(n);
				top.num_glyphs = Some(n);
			} else {
				top.num_glyphs = Some(r.card16()? as usize);
			}
		}
		let order = dict_order(DictKind::Top, cff2);
		let has = |name: &str| order.contains(&name);
		if has("VarStore")
			&& let Some(v) = raw_get(&top.raw, "VarStore")
		{
			let mut r = Reader {
				data,
				pos: as_offset(v)?,
			};
			let length = r.card16()? as usize;
			let store = if length == 65535 {
				r.read(usize::MAX)
			} else {
				r.read(length)
			};
			let mut reader = OtReader::new(store, Tag::new(b"CFF2"));
			let mut dummy = Font::new();
			top.var_store = Some(Box::new(ot::VarStore::decompile(&mut reader, &mut dummy)?));
		}
		if has("charset") {
			top.charset = top.read_charset(data, strings)?;
		}
		if has("Encoding") {
			top.encoding = Some(top.read_encoding(data, strings)?);
		}
		if has("FDArray")
			&& let Some(v) = raw_get(&top.raw, "FDArray")
		{
			let (items, _) = read_index(data, as_offset(v)?, cff2)?;
			let mut fds = Vec::with_capacity(items.len());
			for item in &items {
				let raw = DictDecompiler::new(TOP_DICT, Some(&sid), None).decompile(item)?;
				let values = plain_values(&raw, DictKind::Font, cff2);
				let private = match raw_get(&raw, "Private") {
					Some(Value::Tuple(t)) if t.len() == 2 => Some(Box::new(PrivateDict::read(
						data,
						as_offset(&t[0])?,
						as_offset(&t[1])?,
						strings,
						cff2,
						top.var_store.as_deref(),
					)?)),
					Some(_) => return Err(Error::ttlib("ValueError: too many values to unpack")),
					None => None,
				};
				fds.push(FontDict {
					cff2,
					raw,
					values,
					private,
				});
			}
			top.fd_array = Some(fds);
		}
		if has("FDSelect")
			&& let Some(v) = raw_get(&top.raw, "FDSelect")
			&& let Some(n) = top.num_glyphs
		{
			let max = if cff2 {
				glyph_order.map_or(0, <[String]>::len)
			} else {
				n
			};
			top.fd_select = Some(read_fd_select(data, as_offset(v)?, n, max)?);
		}
		if has("Private")
			&& let Some(v) = raw_get(&top.raw, "Private")
		{
			let Value::Tuple(t) = v else {
				return Err(Error::ttlib("TypeError: cannot unpack non-iterable object"));
			};
			if t.len() != 2 {
				return Err(Error::ttlib("ValueError: too many values to unpack"));
			}
			// The Top DICT has no `vstore`.
			top.private = Some(Box::new(PrivateDict::read(
				data,
				as_offset(&t[0])?,
				as_offset(&t[1])?,
				strings,
				cff2,
				None,
			)?));
		}
		if has("CharStrings")
			&& let Some(v) = raw_get(&top.raw, "CharStrings")
		{
			top.char_strings = Some(top.read_char_strings(data, as_offset(v)?)?);
		}
		Ok(top)
	}

	/// `CharsetConverter._read`.
	fn read_charset(&self, data: &[u8], strings: Option<&Strings>) -> Result<Option<Vec<String>>> {
		let Some(v) = raw_get(&self.raw, "charset") else {
			return Ok(None);
		};
		let value = v.int()?;
		let cid = self.is_cid();
		let charset: Option<Vec<String>> = if value > 2 {
			let Some(n) = self.num_glyphs else {
				// `parent.numGlyphs` is missing: `getattr` gives None.
				return Ok(None);
			};
			let mut r = Reader {
				data,
				pos: value as usize,
			};
			let format = r.card8()?;
			let sid = |s: u16| -> Result<String> {
				strings
					.ok_or_else(|| {
						Error::ttlib("TypeError: 'NoneType' object is not subscriptable")
					})?
					.get(s as i64)
			};
			let mut charset = vec![".notdef".to_owned()];
			match format {
				0 => {
					for _ in 1..n.max(1) {
						let id = r.card16()?;
						charset.push(if cid { cid_name(id as u32) } else { sid(id)? });
					}
				}
				1 | 2 => {
					let mut count = 1;
					while count < n {
						let first = r.card16()? as u32;
						let left = if format == 1 {
							r.card8()? as u32
						} else {
							r.card16()? as u32
						};
						for id in first..=first + left {
							charset.push(if cid { cid_name(id) } else { sid(id as u16)? });
						}
						count += left as usize + 1;
					}
				}
				_ => return Err(Error::ttlib("NotImplementedError")),
			}
			if charset.len() != n {
				return Err(Error::Assertion("len(charset) == numGlyphs".into()));
			}
			Some(unique_names(charset))
		} else if cid || raw_get(&self.raw, "CharStrings").is_none() {
			if value != 0 {
				return Err(Error::Assertion("value == 0".into()));
			}
			None
		} else {
			let list: &[&str] = match value {
				0 => &ISO_ADOBE,
				1 => &EXPERT,
				_ => &EXPERT_SUBSET,
			};
			Some(list.iter().map(|s| (*s).to_owned()).collect())
		};
		Ok(charset.map(|mut c| {
			if let Some(n) = self.num_glyphs
				&& c.len() != n
			{
				c.truncate(n);
			}
			c
		}))
	}

	/// `EncodingConverter._read`.
	fn read_encoding(&self, data: &[u8], strings: Option<&Strings>) -> Result<Encoding> {
		let value = match raw_get(&self.raw, "Encoding") {
			Some(v) => v.int()?,
			None => 0,
		};
		match value {
			0 => return Ok(Encoding::Standard),
			1 => return Ok(Encoding::Expert),
			v if v < 0 => return Err(Error::Assertion("value > 1".into())),
			_ => {}
		}
		let charset = self.charset.as_deref();
		let glyph = |i: usize| -> Result<String> {
			charset
				.ok_or_else(|| Error::ttlib("TypeError: 'NoneType' object is not subscriptable"))?
				.get(i)
				.cloned()
				.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))
		};
		let mut r = Reader {
			data,
			pos: value as usize,
		};
		let format = r.card8()?;
		let supplement = format & 0x80 != 0;
		let mut encoding = vec![".notdef".to_owned(); 256];
		match format & 0x7F {
			0 => {
				let n = r.card8()? as usize;
				for gid in 1..=n {
					let code = r.card8()? as usize;
					if code != 0 {
						encoding[code] = glyph(gid)?;
					}
				}
			}
			1 => {
				let ranges = r.card8()?;
				let mut gid = 1;
				for _ in 0..ranges {
					let first = r.card8()? as usize;
					let left = r.card8()? as usize;
					for code in first..=first + left {
						*encoding.get_mut(code).ok_or_else(|| {
							Error::ttlib("IndexError: list assignment index out of range")
						})? = glyph(gid)?;
						gid += 1;
					}
				}
			}
			f => return Err(Error::value(format!("Unknown Encoding format: {f}"))),
		}
		if supplement {
			let n = r.card8()?;
			for _ in 0..n {
				let code = r.card8()? as usize;
				let sid = r.card16().map_err(|_| {
					Error::ttlib("EOFError: Unexpected end of file while reading SID")
				})?;
				let name = strings
					.ok_or_else(|| {
						Error::ttlib("TypeError: 'NoneType' object is not subscriptable")
					})?
					.get(sid as i64)?;
				encoding[code] = name;
			}
		}
		Ok(Encoding::Custom(encoding))
	}

	/// `CharStringsConverter._read`.
	fn read_char_strings(&self, data: &[u8], offset: usize) -> Result<CharStrings> {
		let (items, _) = read_index(data, offset, self.cff2)?;
		let charset = self
			.charset
			.as_ref()
			.ok_or_else(|| Error::ttlib("TypeError: object of type 'NoneType' has no len()"))?;
		let fd_select = self
			.fd_select
			.as_ref()
			.filter(|s| !s.gid_array.is_empty())
			.map(|s| s.gid_array.clone());
		let mut privates = Vec::with_capacity(items.len());
		for i in 0..items.len() {
			let p = if self.fd_array.is_none() {
				if self.private.is_some() {
					PrivateRef::Top
				} else {
					PrivateRef::None
				}
			} else {
				let fd = match &fd_select {
					Some(s) => s.get(i).copied().ok_or_else(index_error)?.ok_or_else(|| {
						Error::ttlib(
							"TypeError: list indices must be integers or slices, not NoneType",
						)
					})?,
					None => 0,
				};
				let fds = self.fd_array.as_deref().unwrap_or_default();
				let at = if fd < 0 { fd + fds.len() as i64 } else { fd };
				let at = usize::try_from(at)
					.ok()
					.filter(|&a| a < fds.len())
					.ok_or_else(index_error)?;
				if fds[at].private.is_none() {
					return Err(Error::ttlib("AttributeError: Private"));
				}
				PrivateRef::Fd(at)
			};
			privates.push(p);
		}
		let names: Vec<String> = charset.clone();
		if names.len() > items.len() {
			return Err(index_error());
		}
		let items: Vec<T2CharString> = items.into_iter().map(T2CharString::from_bytecode).collect();
		Ok(CharStrings {
			names,
			items,
			privates,
			indexed: true,
			fd_select,
			has_fd_array: self.fd_array.is_some(),
			has_fd_select: self.fd_select.is_some(),
		})
	}
}

/// Reads an FDSelect (`FDSelect`).
fn read_fd_select(data: &[u8], offset: usize, num_glyphs: usize, max: usize) -> Result<FdSelect> {
	let mut r = Reader { data, pos: offset };
	let format = r.card8()?;
	let gid_array = match format {
		0 => r.read(num_glyphs).iter().map(|&b| Some(b as i64)).collect(),
		3 | 4 => {
			let glyph = |r: &mut Reader<'_>| -> Result<usize> {
				Ok(if format == 3 {
					r.card16()? as usize
				} else {
					r.card32()? as usize
				})
			};
			let ranges = glyph(&mut r)?;
			let mut gids = Vec::new();
			let mut fd: Option<i64> = None;
			let mut prev = 0;
			for i in 0..ranges {
				let first = glyph(&mut r)?;
				if (i == 0 && first != 0) || first < prev || first > max {
					return Err(Error::value("Invalid FDSelect range"));
				}
				gids.extend(std::iter::repeat_n(fd, first - prev));
				prev = first;
				fd = Some(if format == 3 {
					r.card8()? as i64
				} else {
					r.card16()? as i64
				});
			}
			let sentinel = glyph(&mut r)?;
			if ranges == 0 || !(num_glyphs <= sentinel && sentinel <= max) || sentinel <= prev {
				return Err(Error::value("Invalid FDSelect sentinel"));
			}
			gids.extend(std::iter::repeat_n(fd, sentinel - prev));
			gids
		}
		f => {
			return Err(Error::Assertion(format!(
				"unsupported FDSelect format: {f}"
			)));
		}
	};
	Ok(FdSelect {
		format: Some(format as i64),
		gid_array,
		num_char_strings: None,
	})
}

/// A CFF font set (`CFFFontSet`): the fonts of a `CFF ` or `CFF2` table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CffFontSet {
	/// `major`.
	pub major: i64,
	/// `minor`.
	pub minor: i64,
	/// `hdrSize`.
	pub hdr_size: u8,
	/// `offSize`.
	pub off_size: u8,
	/// The font names (`fontNames`).
	pub font_names: Vec<String>,
	/// The Top DICTs (`topDictIndex`).
	pub top_dicts: Vec<TopDict>,
	/// The custom strings (`strings`); `None` in CFF2.
	pub strings: Option<Strings>,
	/// The global subroutines (`GlobalSubrs`).
	pub global_subrs: Vec<T2CharString>,
	/// Whether TTX was read into the set (`fromXML` set its defaults).
	pub(crate) read_from_xml: bool,
	/// Whether the font names were set by TTX (`hasattr(self, "fontNames")`).
	pub(crate) has_font_names: bool,
}

impl CffFontSet {
	/// Reads a CFF font set (`decompile`). For CFF2, the glyph order comes from
	/// the font.
	pub fn decompile(data: &[u8], cff2: bool, glyph_order: Option<&[String]>) -> Result<Self> {
		let mut r = Reader { data, pos: 0 };
		let header = r.take(3)?;
		let (major, minor, hdr_size) = (header[0] as i64, header[1] as i64, header[2]);
		let expected = if cff2 { 2 } else { 1 };
		if major != expected {
			return Err(Error::value(format!(
				"Invalid CFF 'major' version: expected {expected}, found {major}"
			)));
		}
		let mut set = CffFontSet {
			major,
			minor,
			hdr_size,
			..Default::default()
		};
		if !cff2 {
			set.off_size = r.card8()?;
			let (names, after) = read_index(data, hdr_size as usize, false)?;
			set.font_names = names.iter().map(|n| latin1_decode(n)).collect();
			let (tops, after) = read_index(data, after, false)?;
			let (strings, after) = read_index(data, after, false)?;
			set.strings = Some(Strings {
				strings: strings.iter().map(|s| latin1_decode(s)).collect(),
				mapping: None,
			});
			let (gsubrs, _) = read_index(data, after, false)?;
			set.global_subrs = gsubrs
				.into_iter()
				.map(T2CharString::from_bytecode)
				.collect();
			for top in &tops {
				set.top_dicts
					.push(TopDict::read(data, top, set.strings.as_ref(), false, None)?);
			}
		} else {
			let size = r.card16()? as usize;
			set.font_names = vec!["CFF2Font".to_owned()];
			let pos = hdr_size as usize;
			let top = data
				.get(pos..pos + size)
				.ok_or_else(|| Error::Assertion("len(data) == size".into()))?;
			let (gsubrs, _) = read_index(data, pos + size, true)?;
			set.global_subrs = gsubrs
				.into_iter()
				.map(T2CharString::from_bytecode)
				.collect();
			set.top_dicts
				.push(TopDict::read(data, top, None, true, glyph_order)?);
		}
		Ok(set)
	}

	/// The glyph order (`getGlyphOrder` of the table).
	pub fn glyph_order(&self) -> Result<Vec<String>> {
		let top = self.top_dicts.first().ok_or_else(index_error)?;
		top.charset
			.clone()
			.ok_or_else(|| Error::ttlib("the CFF font has no charset"))
	}

	/// Runs a charstring of the first font, decompiling it and the subroutines it
	/// calls; draws it onto `pen` if given.
	pub fn run_char_string(&mut self, gid: usize, pen: Option<&mut dyn Pen>) -> Result<()> {
		let top = self.top_dicts.first_mut().ok_or_else(index_error)?;
		run_char_string(top, &mut self.global_subrs, gid, pen, None)
	}

	/// Decompiles all charstrings of the first font (`decompileAllCharStrings`).
	pub fn decompile_all_char_strings(&mut self) -> Result<()> {
		for t in 0..self.top_dicts.len() {
			let n = self.top_dicts[t]
				.char_strings
				.as_ref()
				.map_or(0, |c| c.items.len());
			for gid in 0..n {
				run_char_string(
					&mut self.top_dicts[t],
					&mut self.global_subrs,
					gid,
					None,
					None,
				)
				.inspect_err(|_| {
					log::error!("Error in charstring {gid}");
				})?;
			}
		}
		Ok(())
	}
}

/// The nesting limit for components, in place of Python's recursion limit.
const MAX_COMPONENT_DEPTH: usize = 100;

/// Passes drawing on, but keeps the components for later, so that they can be
/// drawn once the charstring is no longer borrowed. Components only come from
/// `endchar`, at the end of a charstring.
pub(crate) struct DeferComponents<'p> {
	pub out: &'p mut dyn Pen,
	pub components: Vec<(String, Transform)>,
}

impl Pen for DeferComponents<'_> {
	fn move_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.out.move_to(pt)
	}
	fn line_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.out.line_to(pt)
	}
	fn curve_to(&mut self, points: &[Point]) -> fontoxide_pens::Result<()> {
		self.out.curve_to(points)
	}
	fn q_curve_to(
		&mut self,
		off_curves: &[Point],
		on_curve: Option<Point>,
	) -> fontoxide_pens::Result<()> {
		self.out.q_curve_to(off_curves, on_curve)
	}
	fn close_path(&mut self) -> fontoxide_pens::Result<()> {
		self.out.close_path()
	}
	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		self.out.end_path()
	}
	fn add_component(
		&mut self,
		glyph_name: &str,
		transformation: Transform,
	) -> fontoxide_pens::Result<()> {
		self.components
			.push((glyph_name.to_owned(), transformation));
		Ok(())
	}
}

/// The charstrings of a Top DICT as a glyph set (`CharStrings`), drawing at the
/// location of `blender`.
pub(crate) struct CharStringsGlyphSet<'a> {
	top: RefCell<(&'a mut TopDict, &'a mut [T2CharString])>,
	blender: Option<Blender<'a>>,
	depth: Cell<usize>,
}

impl<'a> CharStringsGlyphSet<'a> {
	pub(crate) fn new(
		top: &'a mut TopDict,
		global_subrs: &'a mut [T2CharString],
		blender: Option<Blender<'a>>,
	) -> Self {
		CharStringsGlyphSet {
			top: RefCell::new((top, global_subrs)),
			blender,
			depth: Cell::new(0),
		}
	}

	/// The glyph ID of a glyph name.
	pub(crate) fn index(&self, name: &str) -> Option<usize> {
		self.top.borrow().0.char_strings.as_ref()?.index(name)
	}

	/// Draws a charstring (`T2CharString.draw`); components are drawn afterwards.
	pub(crate) fn draw(&self, gid: usize, pen: &mut dyn Pen) -> Result<()> {
		let depth = self.depth.get();
		if depth > MAX_COMPONENT_DEPTH {
			// LENIENT(L15): fontTools runs into Python's recursion limit.
			return Err(Error::ttlib(
				"RecursionError: maximum recursion depth exceeded",
			));
		}
		self.depth.set(depth + 1);
		let result = self.draw_inner(gid, pen);
		self.depth.set(depth);
		result
	}

	fn draw_inner(&self, gid: usize, pen: &mut dyn Pen) -> Result<()> {
		let mut deferred = DeferComponents {
			out: pen,
			components: Vec::new(),
		};
		{
			let mut guard = self.top.borrow_mut();
			let (top, global_subrs) = &mut *guard;
			run_char_string(top, global_subrs, gid, Some(&mut deferred), self.blender)?;
		}
		let DeferComponents { out, components } = deferred;
		for (name, t) in components {
			out.add_component(&name, t)?;
		}
		Ok(())
	}

	/// The bounds of a glyph (`T2CharString.calcBounds`).
	pub(crate) fn calc_bounds(&self, gid: usize) -> Result<Option<Rect>> {
		let mut pen = BoundsPen::new_bounds(Some(self));
		self.draw(gid, &mut pen)?;
		Ok(pen.bounds())
	}

	/// The union of the bounds of all glyphs.
	fn font_bounds(&self) -> Result<Option<Rect>> {
		let n = self
			.top
			.borrow()
			.0
			.char_strings
			.as_ref()
			.ok_or_else(|| Error::ttlib("AttributeError: CharStrings"))?
			.items
			.len();
		let mut bbox: Option<Rect> = None;
		for gid in 0..n {
			if let Some(b) = self.calc_bounds(gid)? {
				bbox = Some(match bbox {
					Some(a) => union_rect(a, b),
					None => b,
				});
			}
		}
		Ok(bbox)
	}
}

impl GlyphSet for CharStringsGlyphSet<'_> {
	fn draw_glyph(&self, glyph_name: &str, pen: &mut dyn Pen) -> fontoxide_pens::Result<bool> {
		let Some(gid) = self.index(glyph_name) else {
			return Ok(false);
		};
		self.draw(gid, pen).map_err(Error::into_pen_error)?;
		Ok(true)
	}
}

/// The bounds of a glyph of a Top DICT (`T2CharString.calcBounds`).
pub(crate) fn calc_bounds(
	top: &mut TopDict,
	global_subrs: &mut [T2CharString],
	gid: usize,
) -> Result<Option<Rect>> {
	CharStringsGlyphSet::new(top, global_subrs, None).calc_bounds(gid)
}

impl CffFontSet {
	/// The bounds of a glyph of the first font (`calcBounds`).
	pub fn calc_bounds(&mut self, gid: usize) -> Result<Option<Rect>> {
		let top = self.top_dicts.first_mut().ok_or_else(index_error)?;
		calc_bounds(top, &mut self.global_subrs, gid)
	}

	/// The union of the bounds of all glyphs of the first font.
	pub fn font_bounds(&mut self) -> Result<Option<Rect>> {
		let top = self.top_dicts.first_mut().ok_or_else(index_error)?;
		font_bounds(top, &mut self.global_subrs)
	}

	/// The glyph ID of a glyph name in the first font's CharStrings.
	pub fn glyph_index(&self, name: &str) -> Option<usize> {
		self.top_dicts.first()?.char_strings.as_ref()?.index(name)
	}

	/// The glyph ID of a glyph name, trying `hint` first.
	pub fn glyph_index_hint(&self, name: &str, hint: usize) -> Option<usize> {
		let cs = self.top_dicts.first()?.char_strings.as_ref()?;
		if cs.names.get(hint).is_some_and(|n| n == name) {
			return Some(hint);
		}
		cs.index(name)
	}
}

fn font_bounds(top: &mut TopDict, global_subrs: &mut [T2CharString]) -> Result<Option<Rect>> {
	CharStringsGlyphSet::new(top, global_subrs, None).font_bounds()
}

/// `TopDict.recalcFontBBox`.
pub(crate) fn recalc_font_bbox(top: &mut TopDict, global_subrs: &mut [T2CharString]) -> Result<()> {
	let bbox = match font_bounds(top, global_subrs)? {
		Some(b) => Value::List(int_rect(b).iter().map(|&v| Value::Int(v)).collect()),
		None => {
			dict_default(DictKind::Top, top.cff2, "FontBBox").unwrap_or(Value::List(Vec::new()))
		}
	};
	top.values.insert("FontBBox", bbox);
	Ok(())
}

/// Runs a charstring of a Top DICT (`T2CharString.decompile` / `draw`).
pub(crate) fn run_char_string(
	top: &mut TopDict,
	global_subrs: &mut [T2CharString],
	gid: usize,
	pen: Option<&mut dyn Pen>,
	blender: Option<Blender<'_>>,
) -> Result<()> {
	let TopDict {
		char_strings,
		private,
		fd_array,
		var_store,
		..
	} = top;
	let char_strings = char_strings.as_mut().ok_or_else(index_error)?;
	let private_ref = char_strings
		.privates
		.get(gid)
		.copied()
		.ok_or_else(index_error)?;
	if pen.is_none() && !char_strings.items[gid].needs_decompilation() {
		return Ok(());
	}
	let private: Option<&mut PrivateDict> = match private_ref {
		PrivateRef::Top => private.as_deref_mut(),
		PrivateRef::Fd(i) => fd_array
			.as_mut()
			.and_then(|f| f.get_mut(i))
			.and_then(|f| f.private.as_deref_mut()),
		PrivateRef::None => None,
	};
	if pen.is_some() && private.is_none() {
		return Err(Error::ttlib(
			"AttributeError: 'NoneType' object has no attribute 'nominalWidthX'",
		));
	}
	// The Private DICT decides: one read from TTX is a CFF one, also in CFF2.
	let (nominal, default, vsindex) = match &private {
		Some(p) if !p.cff2 => (
			p.number("nominalWidthX"),
			p.number("defaultWidthX"),
			p.get("vsindex").and_then(|v| v.int().ok()),
		),
		Some(p) => (None, None, p.get("vsindex").and_then(|v| v.int().ok())),
		None => (Some(0.0), Some(0.0), None),
	};
	let store = var_store.as_deref();
	let regions = |vi: i64| -> Result<i64> {
		if !matches!(private_ref, PrivateRef::Fd(_)) {
			// Only the Private DICTs of the FDArray have a `vstore`.
			return Err(Error::ttlib(
				"AttributeError: 'NoneType' object has no attribute 'getNumRegions'",
			));
		}
		num_regions(store, vi)
	};
	let mut empty = Vec::new();
	let local: &mut [T2CharString] = match private {
		Some(p) => p.subrs.as_deref_mut().unwrap_or(&mut empty),
		None => &mut empty,
	};
	let mut cs = std::mem::take(&mut char_strings.items[gid]);
	let ctx = Context {
		local_subrs: local,
		global_subrs,
		nominal_width_x: nominal,
		default_width_x: default,
		vsindex,
		num_regions: &regions,
		blender,
	};
	let result = Interpreter::new(ctx, pen).execute(&mut cs);
	char_strings.items[gid] = cs;
	result
}
