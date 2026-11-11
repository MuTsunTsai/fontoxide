//! Graphite tables: `Feat` (`F__e_a_t`), `Sill` (`S__i_l_l`), `Gloc`
//! (`G__l_o_c`), `Glat` (`G__l_a_t`) and `Silf` (`S__i_l_f`), with the helpers of
//! `grUtils`.
//!
//! See <https://graphite.sil.org/graphite_techAbout#graphite-font-tables>.
//!
//! fontTools refers to glyphs by name in these tables; here they are glyph IDs.
//!
//! # Compression
//!
//! `Silf` 5.0 and `Glat` 3.0 may be compressed with LZ4 (scheme 1). fontTools uses
//! the `lz4` module (LZ4 HC at its highest level); fontoxide uses `lz4_flex`, which
//! has no HC mode. Both read each other's output, and the decompressed tables are
//! identical, but the compressed bytes differ, as with brotli in WOFF2.
//!
//! # Missing TTX elements
//!
//! LENIENT(L49): fontTools leaves the attributes of these tables unset until a TTX
//! element or the binary sets them, so incomplete TTX fails with `AttributeError`
//! when compiling or dumping. Here such fields default to zero (or empty).

mod silf;

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::fixed_tools::{fixed_to_float, float_to_fixed, float_to_fixed_to_str};
use fontoxide_misc::text_tools::{Literal, parse_int_radix, safe_eval};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes};
use crate::table::{Table, TableType};
use crate::{Error, Font, Result};

pub use silf::{Classes, Justify, Pass, Silf, SilfSubtable, assemble, disassemble};

// ---------------------------------------------------------------------------
// Python helpers

/// Python's index into a sequence of `len` items, clamped as slicing does.
fn py_index(len: usize, i: i64) -> usize {
	if i < 0 {
		(len as i64 + i).max(0) as usize
	} else {
		(i as usize).min(len)
	}
}

/// `data[start:end]`, with Python's negative indices and clamping.
pub(crate) fn py_slice<T>(data: &[T], start: i64, end: i64) -> &[T] {
	let s = py_index(data.len(), start);
	let e = py_index(data.len(), end);
	if s >= e { &[] } else { &data[s..e] }
}

/// `data[start:]`.
pub(crate) fn py_tail<T>(data: &[T], start: i64) -> &[T] {
	py_slice(data, start, data.len() as i64)
}

/// `seq[i]`, with Python's negative indices.
pub(crate) fn py_get<T>(seq: &[T], i: i64) -> Result<&T> {
	let j = if i < 0 { seq.len() as i64 + i } else { i };
	usize::try_from(j)
		.ok()
		.and_then(|j| seq.get(j))
		.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))
}

fn buffer_error(n: usize) -> Error {
	Error::Struct(format!("unpack requires a buffer of {n} bytes"))
}

/// A reader over `data`, which `struct.unpack` requires to be exactly `n` bytes.
pub(crate) fn exact(data: &[u8], n: usize) -> Result<Reader<'_>> {
	if data.len() == n {
		Ok(Reader::new(data))
	} else {
		Err(buffer_error(n))
	}
}

/// A reader over `data`, which `struct.unpack_from` / `sstruct.unpack2` require
/// to be at least `n` bytes.
pub(crate) fn at_least(data: &[u8], n: usize) -> Result<Reader<'_>> {
	if data.len() >= n {
		Ok(Reader::new(data))
	} else {
		Err(buffer_error(n))
	}
}

/// A count for a `struct` format (`"%dH" % n`): negative counts are errors.
pub(crate) fn count(n: i64) -> Result<usize> {
	usize::try_from(n).map_err(|_| Error::Struct("bad char in struct format".into()))
}

/// Packing with the range checks of `struct.pack`.
pub(crate) trait Pack {
	/// `B`.
	fn pack_u8(&mut self, v: i64) -> Result<()>;
	/// `b`.
	fn pack_i8(&mut self, v: i64) -> Result<()>;
	/// `H`.
	fn pack_u16(&mut self, v: i64) -> Result<()>;
	/// `h`.
	fn pack_i16(&mut self, v: i64) -> Result<()>;
	/// `L`.
	fn pack_u32(&mut self, v: i64) -> Result<()>;
	/// `16.16F` of `sstruct`.
	fn pack_fixed(&mut self, v: f64) -> Result<()>;
	/// `4s`: encoded, truncated or padded with NUL bytes.
	fn pack_4s(&mut self, v: &[u8]);
}

impl Pack for Vec<u8> {
	fn pack_u8(&mut self, v: i64) -> Result<()> {
		let v = u8::try_from(v)
			.map_err(|_| Error::Struct("ubyte format requires 0 <= number <= 255".into()))?;
		self.put_u8(v);
		Ok(())
	}

	fn pack_i8(&mut self, v: i64) -> Result<()> {
		let v = i8::try_from(v)
			.map_err(|_| Error::Struct("byte format requires -128 <= number <= 127".into()))?;
		self.put_i8(v);
		Ok(())
	}

	fn pack_u16(&mut self, v: i64) -> Result<()> {
		let v = u16::try_from(v)
			.map_err(|_| Error::Struct("'H' format requires 0 <= number <= 65535".into()))?;
		self.put_u16(v);
		Ok(())
	}

	fn pack_i16(&mut self, v: i64) -> Result<()> {
		let v = i16::try_from(v)
			.map_err(|_| Error::Struct("short format requires -32768 <= number <= 32767".into()))?;
		self.put_i16(v);
		Ok(())
	}

	fn pack_u32(&mut self, v: i64) -> Result<()> {
		let v = u32::try_from(v)
			.map_err(|_| Error::Struct("'L' format requires 0 <= number <= 4294967295".into()))?;
		self.put_u32(v);
		Ok(())
	}

	fn pack_fixed(&mut self, v: f64) -> Result<()> {
		if !v.is_finite() {
			return Err(Error::value("cannot convert float to integer"));
		}
		let v = i32::try_from(float_to_fixed(v, 16)).map_err(|_| {
			Error::Struct("'l' format requires -2147483648 <= number <= 2147483647".into())
		})?;
		self.put_i32(v);
		Ok(())
	}

	fn pack_4s(&mut self, v: &[u8]) {
		let mut b = [0u8; 4];
		for (d, s) in b.iter_mut().zip(v) {
			*d = *s;
		}
		self.extend_from_slice(&b);
	}
}

/// Reads a `16.16F` field (`fi2fl`).
pub(crate) fn read_fixed(r: &mut Reader<'_>) -> Result<f64> {
	Ok(fixed_to_float(r.i32()? as i64, 16))
}

/// `float(floatToFixedToStr(v, precisionBits=16))`.
pub(crate) fn round_version(v: f64) -> Result<f64> {
	float_to_fixed_to_str(v, 16)
		.parse()
		.map_err(|_| Error::value("could not convert string to float"))
}

fn eval_literal(s: &str) -> Result<Literal> {
	safe_eval(s).ok_or_else(|| Error::value(format!("malformed node or string: {s:?}")))
}

/// `int(s)` for a string (base 10).
pub(crate) fn py_int(s: &str) -> Result<i64> {
	fontoxide_misc::py::parse_int(s)
		.and_then(|v| i64::try_from(v).ok())
		.ok_or_else(|| Error::value(format!("invalid literal for int() with base 10: {s:?}")))
}

/// `int(float(s))`.
fn float_to_int(f: f64) -> Result<i64> {
	if f.is_nan() {
		return Err(Error::value("cannot convert float NaN to integer"));
	}
	if f.is_infinite() || f.abs() >= 9.2e18 {
		return Err(Error::value("cannot convert float infinity to integer"));
	}
	Ok(f.trunc() as i64)
}

/// `int(safeEval(s))`.
pub(crate) fn eval_int(s: &str) -> Result<i64> {
	match eval_literal(s)? {
		Literal::Int(i) => {
			i64::try_from(i).map_err(|_| Error::value(format!("integer out of range: {s}")))
		}
		Literal::Bool(b) => Ok(b as i64),
		Literal::Float(f) => float_to_int(f),
		Literal::Str(t) => py_int(&t),
		_ => Err(Error::value(format!(
			"int() argument must be a number: {s:?}"
		))),
	}
}

/// `float(safeEval(s))`.
pub(crate) fn eval_float(s: &str) -> Result<f64> {
	match eval_literal(s)? {
		Literal::Str(t) => fontoxide_misc::py::parse_float(&t)
			.ok_or_else(|| Error::value(format!("could not convert string to float: {t:?}"))),
		lit => lit
			.as_float()
			.ok_or_else(|| Error::value(format!("float() argument must be a number: {s:?}"))),
	}
}

/// An attribute that fontTools reads with `attrs[name]`.
pub(crate) fn attr<'a>(e: &'a Element, name: &str) -> Result<&'a str> {
	e.attr(name)
		.ok_or_else(|| Error::ttx(format!("KeyError: {name:?}")))
}

/// `content_string`: the text of an element, stripped.
pub(crate) fn content_string(e: &Element) -> String {
	fontoxide_misc::py::strip(&e.text()).to_owned()
}

/// Python's `str.split()`.
pub(crate) fn py_split(s: &str) -> impl Iterator<Item = &str> {
	s.split(fontoxide_misc::py::is_space)
		.filter(|w| !w.is_empty())
}

// ---------------------------------------------------------------------------
// grUtils

/// `grUtils.decompress`: the table data, decompressed if it is compressed, and
/// the compression scheme.
///
/// Without the `graphite` feature (fontTools without the lz4 module), LZ4 data
/// is kept compressed, with a warning, as the other unknown schemes.
pub fn decompress(data: &[u8]) -> Result<(Vec<u8>, i64)> {
	let compression = exact(py_slice(data, 4, 8), 4)?.u32()?;
	let scheme = compression >> 27;
	#[cfg(feature = "graphite")]
	let size = (compression & 0x07FF_FFFF) as usize;
	match scheme {
		0 => Ok((data.to_vec(), 0)),
		#[cfg(feature = "graphite")]
		1 => {
			let res = lz4_flex::block::decompress(&data[8..], size).map_err(|e| {
				Error::ttlib(format!(
					"LZ4BlockError: Decompression failed: corrupt input or insufficient space in destination buffer ({e})"
				))
			})?;
			if res.len() != size {
				return Err(Error::ttlib(format!(
					"LZ4BlockError: Decompressor wrote {} bytes, but {size} bytes expected from header",
					res.len()
				)));
			}
			Ok((res, 1))
		}
		_ => {
			log::warn!("Table is compressed with an unsupported compression scheme");
			Ok((data.to_vec(), scheme as i64))
		}
	}
}

/// `grUtils.compress`.
///
/// Scheme 1 compresses with `lz4_flex`, whose output differs from that of LZ4 HC
/// (see the [module documentation](self)). Without the `graphite` feature
/// (fontTools without the lz4 module), the data is left uncompressed, with a
/// warning, as for the other unknown schemes.
pub fn compress(scheme: i64, data: &[u8]) -> Result<Vec<u8>> {
	let field = scheme
		.checked_mul(1 << 27)
		.and_then(|v| v.checked_add(data.len() as i64 & 0x07FF_FFFF))
		.unwrap_or(-1);
	let mut hdr = py_slice(data, 0, 4).to_vec();
	hdr.pack_u32(field)?;
	match scheme {
		0 => Ok(data.to_vec()),
		#[cfg(feature = "graphite")]
		1 => {
			hdr.extend(lz4_flex::block::compress(data));
			Ok(hdr)
		}
		_ => {
			log::warn!("Table failed to compress by unsupported compression scheme");
			Ok(data.to_vec())
		}
	}
}

/// `grUtils.entries`: runs of consecutive keys (and, with `same_value`, equal
/// values), as `(first key, count, values)`. As in fontTools, the last run is
/// always yielded, even if empty.
pub fn entries(attributes: &BTreeMap<i64, i64>, same_value: bool) -> Vec<(i64, usize, Vec<i64>)> {
	let mut out = Vec::new();
	let mut ak = 0;
	let mut vals: Vec<i64> = Vec::new();
	let mut lastv = 0;
	for (&k, &v) in attributes {
		if !vals.is_empty() && (k != ak + 1 || (same_value && v != lastv)) {
			out.push((
				ak - vals.len() as i64 + 1,
				vals.len(),
				std::mem::take(&mut vals),
			));
		}
		ak = k;
		vals.push(v);
		lastv = v;
	}
	out.push((ak - vals.len() as i64 + 1, vals.len(), vals));
	out
}

/// `grUtils.bininfo`: the binary search header of `num` items of `size` bytes.
pub fn bininfo(num: i64, size: i64) -> Result<Vec<u8>> {
	let mut out = Vec::with_capacity(8);
	if num == 0 {
		out.extend_from_slice(&[0; 8]);
		return Ok(out);
	}
	let mut srange: i64 = 1;
	let mut select: i64 = 0;
	while srange <= num {
		srange *= 2;
		select += 1;
	}
	select -= 1;
	srange /= 2;
	srange *= size;
	let shift = num * size - srange;
	for v in [num, srange, select, shift] {
		out.pack_u16(v)?;
	}
	Ok(out)
}

/// `grUtils.num2tag`: small numbers as decimal, others as tags.
pub fn num2tag(n: u32) -> Result<String> {
	if n < 0x20_0000 {
		Ok(n.to_string())
	} else {
		let bytes: Vec<u8> = n.to_be_bytes().into_iter().filter(|&b| b != 0).collect();
		String::from_utf8(bytes).map_err(|_| Error::value("UnicodeDecodeError: invalid utf-8"))
	}
}

/// `grUtils.tag2num`: the inverse of [`num2tag`].
pub fn tag2num(n: &str) -> Result<i64> {
	if let Ok(v) = py_int(n) {
		return Ok(v);
	}
	let chars: Vec<char> = n.chars().chain("    ".chars()).take(4).collect();
	let mut v: i64 = 0;
	for c in chars {
		if !c.is_ascii() {
			return Err(Error::value(
				"UnicodeEncodeError: 'ascii' codec can't encode character",
			));
		}
		v = v << 8 | c as i64;
	}
	Ok(v)
}

fn version_from_header(data: &[u8]) -> Result<f64> {
	read_fixed(&mut at_least(data, 4)?)
}

fn name_id(label: i64) -> Option<u16> {
	u16::try_from(label).ok()
}

/// Inserts into an insertion-ordered dict (`d[k] = v`).
pub(crate) fn dict_set<K: PartialEq, V>(d: &mut Vec<(K, V)>, k: K, v: V) {
	match d.iter_mut().find(|(key, _)| *key == k) {
		Some(slot) => slot.1 = v,
		None => d.push((k, v)),
	}
}

// ---------------------------------------------------------------------------
// Feat

/// A Graphite feature (`Feature`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Feature {
	/// `flags`.
	pub flags: i64,
	/// The name ID of the feature (`label`).
	pub label: i64,
	/// The name ID of each setting (`settings`).
	pub settings: BTreeMap<i64, i64>,
	/// The default setting (`default`); `None` when read from a feature without
	/// settings.
	pub default: Option<i64>,
	/// The position of the feature (`index`).
	pub index: i64,
}

/// Graphite Feature table (`table_F__e_a_t`); not to be confused with the AAT
/// `feat` table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Feat {
	/// The table version (`version`).
	pub version: f64,
	/// The features by ID (`num2tag`), in insertion order (`features`).
	pub features: Vec<(String, Feature)>,
}

impl TableType for Feat {
	const TAG: Tag = Tag::new(b"Feat");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let version = round_version(version_from_header(data)?)?;
		let data = py_tail(data, 4);
		let num_feats = exact(py_slice(data, 0, 2), 2)?.u16()? as i64;
		let data = py_tail(data, 8);
		let mut allfeats = Vec::new();
		let mut maxsetting = 0;
		for i in 0..num_feats {
			let (fid, nums, offset, flags, lid) = if version >= 2.0 {
				let mut r = exact(py_slice(data, 16 * i, 16 * (i + 1)), 16)?;
				let fid = r.u32()?;
				let nums = r.u16()? as i64;
				r.u16()?;
				let offset = r.u32()? as i64;
				(
					fid,
					nums,
					(offset - 12 - 16 * num_feats) / 4,
					r.u16()?,
					r.u16()?,
				)
			} else {
				let mut r = exact(py_slice(data, 12 * i, 12 * (i + 1)), 12)?;
				let fid = r.u16()? as u32;
				let nums = r.u16()? as i64;
				let offset = r.u32()? as i64;
				(
					fid,
					nums,
					(offset - 12 - 12 * num_feats) / 4,
					r.u16()?,
					r.u16()?,
				)
			};
			allfeats.push((fid, nums, offset, flags, lid));
			maxsetting = i64::max(maxsetting, offset + nums);
		}
		// UPSTREAM-BUG(B68): the settings are taken to start after 16-byte feature
		// records even in version 1, whose records are 12 bytes.
		let data = py_tail(data, 16 * num_feats);
		let mut allsettings = Vec::new();
		for i in 0..maxsetting {
			if data.len() as i64 >= 4 * (i + 1) {
				let mut r = Reader::new(py_slice(data, 4 * i, 4 * (i + 1)));
				allsettings.push((r.u16()? as i64, r.u16()? as i64));
			}
		}
		let mut feat = Feat {
			version,
			features: Vec::new(),
		};
		for (i, &(fid, nums, offset, flags, lid)) in allfeats.iter().enumerate() {
			let mut f = Feature {
				flags: flags as i64,
				label: lid as i64,
				settings: BTreeMap::new(),
				default: None,
				index: i as i64,
			};
			for j in offset..offset + nums {
				if j >= allsettings.len() as i64 {
					continue;
				}
				let &(vid, vlid) = py_get(&allsettings, j)?;
				f.settings.insert(vid, vlid);
				if f.default.is_none() {
					f.default = Some(vid);
				}
			}
			dict_set(&mut feat.features, num2tag(fid)?, f);
		}
		Ok(feat)
	}
}

impl Table for Feat {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let mut order: Vec<usize> = (0..self.features.len()).collect();
		order.sort_by_key(|&i| self.features[i].1.index);
		'restart: loop {
			let mut fdat = Vec::new();
			let mut vdat = Vec::new();
			let mut offset: i64 = 0;
			let n = self.features.len() as i64;
			for &i in &order {
				let (f, v) = &self.features[i];
				let fnum = tag2num(f)?;
				let num_settings = v.settings.len() as i64;
				if self.version >= 2.0 {
					fdat.pack_u32(fnum)?;
					fdat.pack_u16(num_settings)?;
					fdat.pack_u16(0)?;
					fdat.pack_u32(offset * 4 + 12 + 16 * n)?;
				} else if fnum > 65535 {
					// Self healing for alphabetic IDs.
					self.version = 2.0;
					continue 'restart;
				} else {
					fdat.pack_u16(fnum)?;
					fdat.pack_u16(num_settings)?;
					fdat.pack_u32(offset * 4 + 12 + 12 * n)?;
				}
				fdat.pack_u16(v.flags)?;
				fdat.pack_u16(v.label)?;
				let mut settings: Vec<(i64, i64)> =
					v.settings.iter().map(|(&s, &l)| (s, l)).collect();
				settings.sort_by_key(|&(s, l)| {
					if Some(s) == v.default {
						(-1, l)
					} else {
						(s, l)
					}
				});
				for (s, l) in settings {
					vdat.pack_u16(s)?;
					vdat.pack_u16(l)?;
				}
				offset += num_settings;
			}
			let mut out = Vec::new();
			out.pack_fixed(self.version)?;
			out.pack_u16(n)?;
			out.pack_u16(0)?;
			out.pack_u32(0)?;
			out.extend(fdat);
			out.extend(vdat);
			return Ok(out);
		}
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		writer.simpletag("version", &[("version", &self.version)]);
		writer.newline();
		let has_name = font.contains(Tag::new(b"name"));
		let debug_name = |font: &mut Font, label: i64| -> Result<Option<String>> {
			if !has_name {
				return Ok(None);
			}
			let name = font.require::<crate::tables::Name>()?;
			Ok(name_id(label).and_then(|id| name.get_debug_name(id)))
		};
		let mut features: Vec<&(String, Feature)> = self.features.iter().collect();
		features.sort_by_key(|(_, v)| v.index);
		for (f, v) in features {
			if let Some(name) = debug_name(font, v.label)? {
				writer.newline();
				writer.comment(&name);
				writer.newline();
			}
			writer.begintag(
				"feature",
				&[
					("fid", f),
					("label", &v.label),
					("flags", &v.flags),
					("default", &v.default.filter(|&d| d != 0).unwrap_or(0)),
				],
			);
			writer.newline();
			for (s, l) in &v.settings {
				writer.simpletag("setting", &[("value", s), ("label", l)]);
				if let Some(name) = debug_name(font, *l)? {
					writer.write("  ");
					writer.comment(&name);
				}
				writer.newline();
			}
			writer.endtag("feature");
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"version" => self.version = eval_float(attr(element, "version")?)?,
			"feature" => {
				let fid = attr(element, "fid")?.to_owned();
				let mut f = Feature {
					flags: eval_int(attr(element, "flags")?)?,
					label: eval_int(attr(element, "label")?)?,
					default: Some(eval_int(element.attr("default").unwrap_or("0"))?),
					index: self.features.len() as i64,
					settings: BTreeMap::new(),
				};
				for e in element.elements() {
					if e.name == "setting" {
						f.settings
							.insert(eval_int(attr(e, "value")?)?, eval_int(attr(e, "label")?)?);
					}
				}
				dict_set(&mut self.features, fid, f);
			}
			_ => {}
		}
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn remap_glyphs(&mut self, _f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		Ok(())
	}
}

// ---------------------------------------------------------------------------
// Sill

/// Graphite Languages table (`table_S__i_l_l`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sill {
	/// The table version (`version`).
	pub version: f64,
	/// The `(feature ID, value)` settings of each language (`langs`).
	pub langs: BTreeMap<String, Vec<(i64, i64)>>,
}

impl TableType for Sill {
	const TAG: Tag = Tag::new(b"Sill");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let version = round_version(version_from_header(data)?)?;
		let data = py_tail(data, 4);
		let num_langs = exact(py_slice(data, 0, 2), 2)?.u16()? as i64;
		let data = py_tail(data, 8);
		let mut maxsetting = 0;
		let mut langinfo = Vec::new();
		for i in 0..num_langs {
			let mut r = exact(py_slice(data, i * 8, (i + 1) * 8), 8)?;
			let code: Vec<u8> = r.bytes(4)?.iter().copied().filter(|&b| b != 0).collect();
			let num_settings = r.u16()? as i64;
			let offset = r.u16()? as i64 / 8 - (num_langs + 1);
			let code = String::from_utf8(code)
				.map_err(|_| Error::value("UnicodeDecodeError: invalid utf-8"))?;
			langinfo.push((code, num_settings, offset));
			maxsetting = i64::max(maxsetting, offset + num_settings);
		}
		let data = py_tail(data, num_langs * 8);
		let mut finfo = Vec::new();
		for i in 0..maxsetting {
			let mut r = exact(py_slice(data, i * 8, (i + 1) * 8), 8)?;
			let fid = r.u32()? as i64;
			finfo.push((fid, r.u16()? as i64));
		}
		let mut langs = BTreeMap::new();
		for (c, n, o) in langinfo {
			let mut settings = Vec::new();
			for i in o..o + n {
				settings.push(*py_get(&finfo, i)?);
			}
			langs.insert(c, settings);
		}
		Ok(Sill { version, langs })
	}
}

impl Table for Sill {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let mut ldat = Vec::new();
		let mut fdat = Vec::new();
		let mut offset = self.langs.len() as i64;
		for (c, inf) in &self.langs {
			ldat.pack_4s(c.as_bytes());
			ldat.pack_u16(inf.len() as i64)?;
			ldat.pack_u16(8 * offset + 20)?;
			for &(fid, val) in inf {
				fdat.pack_u32(fid)?;
				fdat.pack_u16(val)?;
				fdat.pack_u16(0)?;
			}
			offset += inf.len() as i64;
		}
		ldat.pack_u32(0x8080_8080)?;
		ldat.pack_u16(0)?;
		ldat.pack_u16(8 * offset + 20)?;
		let mut out = Vec::new();
		out.pack_fixed(self.version)?;
		out.extend(bininfo(self.langs.len() as i64, 1)?);
		out.extend(ldat);
		out.extend(fdat);
		Ok(out)
	}

	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		writer.simpletag("version", &[("version", &self.version)]);
		writer.newline();
		for (c, inf) in &self.langs {
			writer.begintag("lang", &[("name", c)]);
			writer.newline();
			for &(fid, val) in inf {
				let fid = u32::try_from(fid)
					.map_err(|_| Error::Struct("argument out of range".into()))?;
				writer.simpletag("feature", &[("fid", &num2tag(fid)?), ("val", &val)]);
				writer.newline();
			}
			writer.endtag("lang");
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"version" => self.version = eval_float(attr(element, "version")?)?,
			"lang" => {
				let c = attr(element, "name")?.to_owned();
				let mut settings = Vec::new();
				for e in element.elements() {
					if e.name == "feature" {
						settings.push((tag2num(attr(e, "fid")?)?, eval_int(attr(e, "val")?)?));
					}
				}
				self.langs.insert(c, settings);
			}
			_ => {}
		}
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn remap_glyphs(&mut self, _f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		Ok(())
	}
}

// ---------------------------------------------------------------------------
// Gloc

/// Graphite Index to Glyph Attributes table (`table_G__l_o_c`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Gloc {
	/// The version read from the binary; 1.0 is always written.
	pub version: f64,
	/// The number of attributes (`numAttribs`).
	pub num_attribs: i64,
	/// The offset of each glyph's attributes in `Glat`, and the end
	/// (`locations`); set when `Glat` is compiled.
	pub locations: Option<Vec<u32>>,
	/// Whether the locations are 32-bit (the `I` type code of `locations`).
	pub long_format: bool,
	/// The attribute IDs (`attribIds`), if any.
	pub attrib_ids: Option<Vec<u16>>,
}

impl Gloc {
	/// Sets the locations, choosing their size (`set`).
	pub fn set(&mut self, locations: Vec<u32>) {
		self.long_format = locations.iter().max().is_some_and(|&m| m >= 65536);
		self.locations = Some(locations);
	}

	/// The locations (`iter(gloc)`).
	pub fn locations(&self) -> Result<&[u32]> {
		self.locations.as_deref().ok_or_else(|| {
			Error::ttlib("AttributeError: 'table_G__l_o_c' object has no attribute 'locations'")
		})
	}
}

fn from_bytes_error() -> Error {
	Error::value("bytes length not a multiple of item size")
}

impl TableType for Gloc {
	const TAG: Tag = Tag::new(b"Gloc");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let mut r = at_least(data, 8)?;
		let version = read_fixed(&mut r)?;
		let flags = r.u16()? as i64;
		let num_attribs = r.u16()? as i64;
		let data = r.rest();
		let long_format = flags & 1 != 0;
		let loc = py_slice(data, 0, data.len() as i64 - num_attribs * (flags & 2));
		let size = if long_format { 4 } else { 2 };
		if loc.len() % size != 0 {
			return Err(from_bytes_error());
		}
		let mut lr = Reader::new(loc);
		let locations = if long_format {
			lr.u32_array(loc.len() / 4)?
		} else {
			lr.u16_array(loc.len() / 2)?
				.into_iter()
				.map(u32::from)
				.collect()
		};
		let mut attrib_ids = Vec::new();
		if flags & 2 != 0 {
			let ids = py_tail(data, -num_attribs * 2);
			if ids.len() % 2 != 0 {
				return Err(from_bytes_error());
			}
			attrib_ids = Reader::new(ids).u16_array(ids.len() / 2)?;
		}
		Ok(Gloc {
			version,
			num_attribs,
			locations: Some(locations),
			long_format,
			attrib_ids: Some(attrib_ids),
		})
	}
}

impl Table for Gloc {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let ids = self.attrib_ids.as_deref().filter(|ids| !ids.is_empty());
		let locations = self.locations()?;
		let mut data = Vec::new();
		data.pack_fixed(1.0)?;
		data.pack_u16(((ids.is_some() as i64) << 1) + self.long_format as i64)?;
		data.pack_u16(self.num_attribs)?;
		for &l in locations {
			if self.long_format {
				data.put_u32(l);
			} else {
				data.pack_u16(l as i64)?;
			}
		}
		for &id in ids.unwrap_or_default() {
			data.put_u16(id);
		}
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		writer.simpletag("attributes", &[("number", &self.num_attribs)]);
		writer.newline();
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		if element.name == "attributes" {
			self.num_attribs = eval_int(attr(element, "number")?)?;
		}
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn remap_glyphs(&mut self, _f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		Ok(())
	}
}

// ---------------------------------------------------------------------------
// Glat

/// One of the subboxes of an [`Octabox`] (`Glat_format_3_subbox_entry`); values
/// are in 255ths of the glyph's bounding box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Subbox {
	/// `left`.
	pub left: i64,
	/// `right`.
	pub right: i64,
	/// `bottom`.
	pub bottom: i64,
	/// `top`.
	pub top: i64,
	/// `diagNegMin`.
	pub diag_neg_min: i64,
	/// `diagNegMax`.
	pub diag_neg_max: i64,
	/// `diagPosMin`.
	pub diag_pos_min: i64,
	/// `diagPosMax`.
	pub diag_pos_max: i64,
}

impl Subbox {
	const NAMES: [&'static str; 8] = [
		"left",
		"right",
		"bottom",
		"top",
		"diagNegMin",
		"diagNegMax",
		"diagPosMin",
		"diagPosMax",
	];

	fn fields_mut(&mut self) -> [&mut i64; 8] {
		[
			&mut self.left,
			&mut self.right,
			&mut self.bottom,
			&mut self.top,
			&mut self.diag_neg_min,
			&mut self.diag_neg_max,
			&mut self.diag_pos_min,
			&mut self.diag_pos_max,
		]
	}

	fn fields(&self) -> [i64; 8] {
		[
			self.left,
			self.right,
			self.bottom,
			self.top,
			self.diag_neg_min,
			self.diag_neg_max,
			self.diag_pos_min,
			self.diag_pos_max,
		]
	}
}

/// The collision octabox of a glyph (`Glat_format_3_octabox_metrics`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Octabox {
	/// Which subboxes exist on a 4×4 grid (`subboxBitmap`).
	pub subbox_bitmap: i64,
	/// `diagNegMin`.
	pub diag_neg_min: i64,
	/// `diagNegMax`.
	pub diag_neg_max: i64,
	/// `diagPosMin`.
	pub diag_pos_min: i64,
	/// `diagPosMax`.
	pub diag_pos_max: i64,
	/// The subboxes (`subboxes`).
	pub subboxes: Vec<Subbox>,
}

impl Octabox {
	const NAMES: [&'static str; 4] = ["diagNegMin", "diagNegMax", "diagPosMin", "diagPosMax"];

	fn diag_mut(&mut self) -> [&mut i64; 4] {
		[
			&mut self.diag_neg_min,
			&mut self.diag_neg_max,
			&mut self.diag_pos_min,
			&mut self.diag_pos_max,
		]
	}

	fn diag(&self) -> [i64; 4] {
		[
			self.diag_neg_min,
			self.diag_neg_max,
			self.diag_pos_min,
			self.diag_pos_max,
		]
	}

	fn num_subboxes(&self) -> usize {
		// `bin(n).count("1")`, which counts the magnitude's bits.
		self.subbox_bitmap.unsigned_abs().count_ones() as usize
	}
}

/// The attributes of a glyph (`_Dict`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GlatAttributes {
	/// The attribute values by attribute number.
	pub values: BTreeMap<i64, i64>,
	/// The octabox (`octabox`), in version 3 tables with octaboxes.
	pub octabox: Option<Octabox>,
}

/// Graphite Glyph Attributes table (`table_G__l_a_t`).
///
/// Compiling it also sets the locations of `Gloc`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Glat {
	/// The table version (`version`).
	pub version: f64,
	/// The compression scheme (`scheme`).
	pub scheme: i64,
	/// The compression field of version 3 (`compression`).
	pub compression: i64,
	/// Whether the glyphs have octaboxes (`hasOctaboxes`); unset until read
	/// from a version 3 table or from TTX with octaboxes.
	pub has_octaboxes: Option<bool>,
	/// The attributes of each glyph (`attributes`).
	pub attributes: BTreeMap<u32, GlatAttributes>,
}

/// Which entry format the attributes use.
#[derive(Clone, Copy, PartialEq)]
enum EntryFormat {
	/// `Glat_format_1_entry`: bytes.
	Bytes,
	/// `Glat_format_23_entry`: shorts.
	Shorts,
}

impl EntryFormat {
	fn size(self) -> usize {
		match self {
			EntryFormat::Bytes => 2,
			EntryFormat::Shorts => 4,
		}
	}
}

/// `decompileAttributes12`.
fn decompile_attributes12(mut data: &[u8], fmt: EntryFormat) -> Result<BTreeMap<i64, i64>> {
	let mut attributes = BTreeMap::new();
	while data.len() > 3 {
		let mut r = at_least(data, fmt.size())?;
		let (att_num, num) = match fmt {
			EntryFormat::Bytes => (r.u8()? as i64, r.u8()? as usize),
			EntryFormat::Shorts => (r.u16()? as i64, r.u16()? as usize),
		};
		data = r.rest();
		if data.len() >= 2 * num {
			let vals = Reader::new(data).i16_array(num)?;
			for (k, v) in (att_num..).zip(vals) {
				attributes.insert(k, v as i64);
			}
			data = &data[2 * num..];
		}
	}
	Ok(attributes)
}

impl Glat {
	fn has_octaboxes(&self) -> Result<bool> {
		self.has_octaboxes.ok_or_else(|| {
			Error::ttlib("AttributeError: 'table_G__l_a_t' object has no attribute 'hasOctaboxes'")
		})
	}

	/// `decompileAttributes3`.
	fn decompile_attributes3(&self, mut data: &[u8]) -> Result<GlatAttributes> {
		let mut octabox = None;
		if self.has_octaboxes()? {
			let mut r = at_least(data, 6)?;
			let mut o = Octabox {
				subbox_bitmap: r.u16()? as i64,
				..Default::default()
			};
			for f in o.diag_mut() {
				*f = r.u8()? as i64;
			}
			data = r.rest();
			for _ in 0..o.num_subboxes() {
				if data.len() >= 8 {
					let mut s = Subbox::default();
					for (f, &b) in s.fields_mut().into_iter().zip(&data[..8]) {
						*f = b as i64;
					}
					o.subboxes.push(s);
					data = &data[8..];
				}
			}
			octabox = Some(o);
		}
		Ok(GlatAttributes {
			values: decompile_attributes12(data, EntryFormat::Shorts)?,
			octabox,
		})
	}

	/// `compileAttributes12`.
	fn compile_attributes12(attrs: &BTreeMap<i64, i64>, fmt: EntryFormat) -> Result<Vec<u8>> {
		let mut data = Vec::new();
		for (att_num, num, vals) in entries(attrs, false) {
			match fmt {
				EntryFormat::Bytes => {
					data.pack_u8(att_num)?;
					data.pack_u8(num as i64)?;
				}
				EntryFormat::Shorts => {
					data.pack_u16(att_num)?;
					data.pack_u16(num as i64)?;
				}
			}
			for v in vals {
				data.pack_i16(v)?;
			}
		}
		Ok(data)
	}

	/// `compileAttributes3`.
	fn compile_attributes3(&self, attrs: &GlatAttributes) -> Result<Vec<u8>> {
		let mut data = Vec::new();
		if self.has_octaboxes()? {
			let o = attrs.octabox.as_ref().ok_or_else(|| {
				Error::ttlib("AttributeError: '_Dict' object has no attribute 'octabox'")
			})?;
			data.pack_u16(o.subbox_bitmap)?;
			for v in o.diag() {
				data.pack_u8(v)?;
			}
			for b in 0..o.num_subboxes() {
				let s = o
					.subboxes
					.get(b)
					.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
				for v in s.fields() {
					data.pack_u8(v)?;
				}
			}
		}
		// LENIENT(L49): without octaboxes, fontTools starts from a `str` and fails to
		// add the bytes to it (UPSTREAM-BUG), so it cannot compile version 3
		// tables without octaboxes.
		data.extend(Self::compile_attributes12(
			&attrs.values,
			EntryFormat::Shorts,
		)?);
		Ok(data)
	}
}

fn unbound_decoder() -> Error {
	Error::ttlib("UnboundLocalError: cannot access local variable 'decoder'")
}

impl TableType for Glat {
	const TAG: Tag = Tag::new(b"Glat");

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		let mut glat = Glat {
			version: round_version(version_from_header(data)?)?,
			..Default::default()
		};
		let mut data = data.to_vec();
		let fmt = if glat.version <= 1.9 {
			Some(EntryFormat::Bytes)
		} else if glat.version <= 2.9 {
			Some(EntryFormat::Shorts)
		} else if glat.version >= 3.0 {
			let (d, scheme) = decompress(&data)?;
			data = d;
			glat.scheme = scheme;
			let mut r = at_least(&data, 8)?;
			glat.version = read_fixed(&mut r)?;
			glat.compression = r.u32()? as i64;
			glat.has_octaboxes = Some(glat.compression & 1 == 1);
			None
		} else {
			return Err(unbound_decoder());
		};
		let gloc = font.require::<Gloc>()?.locations()?.to_vec();
		for (count, w) in gloc.windows(2).enumerate() {
			let chunk = py_slice(&data, w[0] as i64, w[1] as i64);
			let attrs = match fmt {
				Some(fmt) => GlatAttributes {
					values: decompile_attributes12(chunk, fmt)?,
					octabox: None,
				},
				None => glat.decompile_attributes3(chunk)?,
			};
			glat.attributes.insert(count as u32, attrs);
		}
		Ok(glat)
	}
}

impl Table for Glat {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		let mut data = Vec::new();
		data.pack_fixed(self.version)?;
		let fmt = if self.version <= 1.9 {
			Some(EntryFormat::Bytes)
		} else if self.version <= 2.9 {
			// UPSTREAM-BUG(B69): version 2 is written with the byte entries of version 1,
			// but read with the short entries.
			Some(EntryFormat::Bytes)
		} else if self.version >= 3.0 {
			let octaboxes = self.has_octaboxes()? as i64;
			self.compression = self
				.scheme
				.checked_mul(1 << 27)
				.and_then(|v| v.checked_add(octaboxes))
				.unwrap_or(-1);
			data.clear();
			data.pack_fixed(self.version)?;
			data.pack_u32(self.compression)?;
			None
		} else {
			return Err(unbound_decoder());
		};
		let mut glocs = Vec::with_capacity(self.attributes.len() + 1);
		let location = |len: usize| {
			u32::try_from(len)
				.map_err(|_| Error::ttlib("OverflowError: unsigned int is greater than maximum"))
		};
		for n in 0..self.attributes.len() as u32 {
			glocs.push(location(data.len())?);
			let attrs = self
				.attributes
				.get(&n)
				.ok_or_else(|| Error::ttlib(format!("KeyError: glyph {n}")))?;
			data.extend(match fmt {
				Some(fmt) => Self::compile_attributes12(&attrs.values, fmt)?,
				None => self.compile_attributes3(attrs)?,
			});
		}
		glocs.push(location(data.len())?);
		font.require::<Gloc>()?.set(glocs);
		if self.version >= 3.0 {
			data = compress(self.scheme, &data)?;
		}
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		writer.simpletag(
			"version",
			&[
				("version", &self.version),
				("compressionScheme", &self.scheme),
			],
		);
		writer.newline();
		let order = font.glyph_order()?;
		for (&gid, a) in &self.attributes {
			writer.begintag("glyph", &[("name", &order.name(gid))]);
			writer.newline();
			if let Some(o) = &a.octabox {
				let percent = |v: i64| format!("{:.3}%", v as f64 * 100.0 / 255.0);
				let diag: Vec<String> = o.diag().into_iter().map(percent).collect();
				let bitmap = if o.subbox_bitmap < 0 {
					format!("-{:X}", o.subbox_bitmap.unsigned_abs())
				} else {
					format!("{:X}", o.subbox_bitmap)
				};
				let mut attrs: Vec<(&str, &dyn fontoxide_misc::xml_writer::AttrValue)> =
					Octabox::NAMES
						.iter()
						.zip(&diag)
						.map(|(&n, v)| (n, v as &dyn fontoxide_misc::xml_writer::AttrValue))
						.collect();
				attrs.push(("bitmap", &bitmap));
				writer.begintag("octaboxes", &attrs);
				writer.newline();
				for s in &o.subboxes {
					let vals: Vec<String> = s.fields().into_iter().map(percent).collect();
					let attrs: Vec<(&str, &dyn fontoxide_misc::xml_writer::AttrValue)> =
						Subbox::NAMES
							.iter()
							.zip(&vals)
							.map(|(&n, v)| (n, v as &dyn fontoxide_misc::xml_writer::AttrValue))
							.collect();
					writer.simpletag("octabox", &attrs);
					writer.newline();
				}
				writer.endtag("octaboxes");
				writer.newline();
			}
			for (k, v) in &a.values {
				writer.simpletag("attribute", &[("index", k), ("value", v)]);
				writer.newline();
			}
			writer.endtag("glyph");
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		if element.name == "version" {
			self.version = eval_float(attr(element, "version")?)?;
			self.scheme = eval_int(attr(element, "compressionScheme")?)?;
		}
		if element.name != "glyph" {
			return Ok(());
		}
		let gid = font.glyph_id(attr(element, "name")?)?;
		// `int(float(v[:-1]) * 255.0 / 100.0 + 0.5)`.
		let from_percent = |v: &str| -> Result<i64> {
			let mut chars = v.chars();
			chars.next_back();
			let f = fontoxide_misc::py::parse_float(chars.as_str())
				.ok_or_else(|| Error::value(format!("could not convert string to float: {v:?}")))?;
			float_to_int(f * 255.0 / 100.0 + 0.5)
		};
		let mut attributes = GlatAttributes::default();
		for e in element.elements() {
			match e.name.as_str() {
				"attribute" => {
					let k = eval_int(attr(e, "index")?)?;
					let v = eval_int(attr(e, "value")?)?;
					attributes.values.insert(k, v);
				}
				"octaboxes" => {
					self.has_octaboxes = Some(true);
					let bitmap = attr(e, "bitmap")?;
					let mut o = Octabox {
						subbox_bitmap: parse_int_radix(bitmap, 16)
							.and_then(|v| i64::try_from(v).ok())
							.ok_or_else(|| {
								Error::value(format!(
									"invalid literal for int() with base 16: {bitmap:?}"
								))
							})?,
						..Default::default()
					};
					for (k, v) in &e.attrs {
						if let Some(i) = Octabox::NAMES.iter().position(|n| n == k) {
							*o.diag_mut()[i] = from_percent(v)?;
						} else if k != "bitmap" {
							from_percent(v)?;
						}
					}
					for sub in e.elements() {
						let mut s = Subbox::default();
						for (k, v) in &sub.attrs {
							let value = from_percent(v)?;
							if let Some(i) = Subbox::NAMES.iter().position(|n| n == k) {
								*s.fields_mut()[i] = value;
							}
						}
						o.subboxes.push(s);
					}
					attributes.octabox = Some(o);
				}
				_ => {}
			}
		}
		self.attributes.insert(gid, attributes);
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		self.attributes = crate::ot::aat::remap_keys(std::mem::take(&mut self.attributes), f)?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn grutils() -> Result<()> {
		assert_eq!(num2tag(1)?, "1");
		assert_eq!(num2tag(u32::from_be_bytes(*b"nom "))?, "nom ");
		assert_eq!(num2tag(u32::from_be_bytes(*b"ab\0\0"))?, "ab");
		assert_eq!(tag2num("1")?, 1);
		assert_eq!(tag2num("ab")?, i64::from(u32::from_be_bytes(*b"ab  ")));
		assert_eq!(bininfo(0, 1)?, [0; 8]);
		assert_eq!(bininfo(5, 6)?, [0, 5, 0, 24, 0, 2, 0, 6]);
		let attrs: BTreeMap<i64, i64> = [(1, 3), (2, 30), (4, 4), (5, 4)].into();
		assert_eq!(
			entries(&attrs, false),
			[(1, 2, vec![3, 30]), (4, 2, vec![4, 4])]
		);
		assert_eq!(entries(&BTreeMap::new(), false), [(1, 0, vec![])]);
		Ok(())
	}

	/// fontTools without the lz4 module: `grUtils` warns and leaves the data as
	/// it is.
	#[test]
	#[cfg(not(feature = "graphite"))]
	fn lz4_without_the_module() -> Result<()> {
		let data = b"\0\x05\0\0\0\0\0\0hello hello hello hello".to_vec();
		assert_eq!(compress(1, &data)?, data);
		let compressed = b"\0\x05\0\0\x08\0\0\x1f\x01\x02".to_vec();
		assert_eq!(decompress(&compressed)?, (compressed, 1));
		Ok(())
	}

	#[test]
	#[cfg(feature = "graphite")]
	fn lz4() -> Result<()> {
		let data = b"\0\x05\0\0\0\0\0\0hello hello hello hello".to_vec();
		let compressed = compress(1, &data)?;
		assert_eq!(&compressed[..8], b"\0\x05\0\0\x08\0\0\x1f");
		assert_eq!(decompress(&compressed)?, (data, 1));
		Ok(())
	}

	#[test]
	fn slices() {
		let d = [1, 2, 3, 4];
		assert_eq!(py_slice(&d, 1, 3), [2, 3]);
		assert_eq!(py_slice(&d, -1, 9), [4]);
		assert_eq!(py_slice(&d, 3, 1), [] as [i32; 0]);
		assert_eq!(py_tail(&d, -0), d);
		assert_eq!(py_slice(&d, 0, -5), [] as [i32; 0]);
	}
}
