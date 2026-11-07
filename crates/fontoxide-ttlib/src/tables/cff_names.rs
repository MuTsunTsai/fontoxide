//! Glyph names and the font bounding box from a `CFF ` table, as `TTFont` needs
//! them, read directly from the table data.
//!
//! This is a stopgap until `fontoxide-cfflib` (the port of `fontTools.cffLib`)
//! exists; it follows `CharsetConverter`, `parseCharset0`/`parseCharset` and
//! `TopDict.FontBBox`. Note that fontTools decompiles the whole `CFF ` table to get
//! these, which also makes it recompile the table on save; here the table stays
//! raw.

use crate::binary::Reader;
use crate::generated::cff_strings::{EXPERT, EXPERT_SUBSET, ISO_ADOBE, STANDARD_STRINGS};
use crate::{Error, Font, Result, Tag};

const CFF: Tag = Tag::new(b"CFF ");

/// A CFF INDEX: the data of each item.
fn read_index<'a>(r: &mut Reader<'a>) -> Result<Vec<&'a [u8]>> {
	let count = r.u16()? as usize;
	if count == 0 {
		return Ok(Vec::new());
	}
	let off_size = r.u8()? as usize;
	if !(1..=4).contains(&off_size) {
		return Err(Error::ttlib(format!(
			"invalid CFF INDEX offSize {off_size}"
		)));
	}
	let mut offsets = Vec::with_capacity(count + 1);
	for _ in 0..=count {
		let bytes = r.bytes(off_size)?;
		offsets.push(bytes.iter().fold(0usize, |acc, &b| acc << 8 | b as usize));
	}
	let base = r.pos() - 1;
	let data = r.data();
	let mut items = Vec::with_capacity(count);
	for w in offsets.windows(2) {
		let (start, end) = (base + w[0], base + w[1]);
		items.push(
			data.get(start..end)
				.ok_or_else(|| Error::ttlib("CFF INDEX item out of range"))?,
		);
	}
	r.seek(base + offsets[count]);
	Ok(items)
}

/// A DICT operand.
#[derive(Debug, Clone, Copy)]
enum Operand {
	Int(i64),
	Real(f64),
}

impl Operand {
	fn as_f64(self) -> f64 {
		match self {
			Operand::Int(i) => i as f64,
			Operand::Real(f) => f,
		}
	}
}

/// Parses a DICT into (operator, operands) pairs. Two-byte operators are
/// `1200 + b1`.
fn parse_dict(data: &[u8]) -> Result<Vec<(u16, Vec<Operand>)>> {
	let mut r = Reader::new(data);
	let mut out = Vec::new();
	let mut operands = Vec::new();
	while r.remaining() > 0 {
		let b0 = r.u8()?;
		match b0 {
			0..=21 => {
				let op = if b0 == 12 {
					1200 + r.u8()? as u16
				} else {
					b0 as u16
				};
				out.push((op, std::mem::take(&mut operands)));
			}
			28 => operands.push(Operand::Int(r.i16()? as i64)),
			29 => operands.push(Operand::Int(r.i32()? as i64)),
			30 => operands.push(Operand::Real(parse_real(&mut r)?)),
			32..=246 => operands.push(Operand::Int(b0 as i64 - 139)),
			247..=250 => {
				let b1 = r.u8()? as i64;
				operands.push(Operand::Int((b0 as i64 - 247) * 256 + b1 + 108));
			}
			251..=254 => {
				let b1 = r.u8()? as i64;
				operands.push(Operand::Int(-(b0 as i64 - 251) * 256 - b1 - 108));
			}
			_ => return Err(Error::ttlib(format!("invalid CFF DICT byte {b0}"))),
		}
	}
	Ok(out)
}

/// A real number operand (`parseReal`).
fn parse_real(r: &mut Reader<'_>) -> Result<f64> {
	let mut s = String::new();
	'outer: loop {
		let b = r.u8()?;
		for nibble in [b >> 4, b & 0xF] {
			match nibble {
				0..=9 => s.push((b'0' + nibble) as char),
				0xA => s.push('.'),
				0xB => s.push('E'),
				0xC => s.push_str("E-"),
				0xE => s.push('-'),
				0xF => break 'outer,
				_ => {}
			}
		}
	}
	fontoxide_misc::py::parse_float(&s)
		.ok_or_else(|| Error::value(format!("invalid real number: {s:?}")))
}

struct TopDict {
	charset: Option<i64>,
	char_strings: Option<i64>,
	is_cid: bool,
	font_bbox: [f64; 4],
}

struct Cff<'a> {
	data: &'a [u8],
	top: TopDict,
	strings: Vec<&'a [u8]>,
}

fn parse(data: &[u8]) -> Result<Cff<'_>> {
	let mut r = Reader::new(data);
	let _major = r.u8()?;
	let _minor = r.u8()?;
	let hdr_size = r.u8()? as usize;
	r.seek(hdr_size);
	let _names = read_index(&mut r)?;
	let top_dicts = read_index(&mut r)?;
	let strings = read_index(&mut r)?;
	let top_data = top_dicts
		.first()
		.ok_or_else(|| Error::ttlib("CFF table has no top DICT"))?;
	let mut top = TopDict {
		charset: None,
		char_strings: None,
		is_cid: false,
		font_bbox: [0.0; 4],
	};
	for (op, operands) in parse_dict(top_data)? {
		let first = operands.first().copied();
		match op {
			5 if operands.len() == 4 => {
				for (d, v) in top.font_bbox.iter_mut().zip(&operands) {
					*d = v.as_f64();
				}
			}
			15 => top.charset = first.map(|v| v.as_f64() as i64),
			17 => top.char_strings = first.map(|v| v.as_f64() as i64),
			1230 => top.is_cid = true,
			_ => {}
		}
	}
	Ok(Cff { data, top, strings })
}

impl Cff<'_> {
	/// `strings[SID]` (`IndexedStrings.__getitem__`).
	fn string(&self, sid: usize) -> Result<String> {
		if sid < STANDARD_STRINGS.len() {
			return Ok(STANDARD_STRINGS[sid].to_owned());
		}
		let s = self
			.strings
			.get(sid - STANDARD_STRINGS.len())
			.ok_or_else(|| Error::ttlib(format!("CFF string index {sid} out of range")))?;
		// fontTools decodes CFF strings as Latin-1.
		Ok(fontoxide_misc::py::latin1_decode(s))
	}

	fn num_glyphs(&self) -> Result<usize> {
		let offset = self
			.top
			.char_strings
			.ok_or_else(|| Error::ttlib("CFF top DICT has no CharStrings"))?;
		let mut r = Reader::new(self.data);
		r.seek(offset as usize);
		Ok(r.u16()? as usize)
	}

	/// `TopDict.charset` (`CharsetConverter._read`).
	fn charset(&self) -> Result<Vec<String>> {
		let num_glyphs = self.num_glyphs()?;
		let value = self.top.charset.unwrap_or(0);
		let mut charset: Vec<String> = if value > 2 {
			let mut r = Reader::new(self.data);
			r.seek(value as usize);
			let format = r.u8()?;
			let name = |id: u16| -> Result<String> {
				if self.top.is_cid {
					Ok(format!("cid{id:05}"))
				} else {
					self.string(id as usize)
				}
			};
			let mut charset = vec![".notdef".to_owned()];
			match format {
				0 => {
					for _ in 1..num_glyphs {
						charset.push(name(r.u16()?)?);
					}
				}
				1 | 2 => {
					let mut count = 1;
					while count < num_glyphs {
						let first = r.u16()? as u32;
						let n_left = if format == 1 {
							r.u8()? as u32
						} else {
							r.u16()? as u32
						};
						for id in first..=first + n_left {
							charset.push(name(id as u16)?);
						}
						count += n_left as usize + 1;
					}
				}
				f => return Err(Error::ttlib(format!("unsupported CFF charset format {f}"))),
			}
			if charset.len() != num_glyphs {
				return Err(Error::Assertion("len(charset) == numGlyphs".into()));
			}
			// Make sure glyph names are unique.
			make_unique(charset)
		} else if self.top.is_cid {
			return Err(Error::ttlib("CID-keyed CFF font without a charset"));
		} else {
			let predefined: &[&str] = match value {
				0 => &ISO_ADOBE,
				1 => &EXPERT,
				_ => &EXPERT_SUBSET,
			};
			predefined.iter().map(|s| (*s).to_owned()).collect()
		};
		charset.truncate(num_glyphs);
		Ok(charset)
	}
}

fn make_unique(charset: Vec<String>) -> Vec<String> {
	use std::collections::{HashMap, HashSet};
	let all: HashSet<String> = charset.iter().cloned().collect();
	let mut seen: HashMap<String, usize> = HashMap::new();
	let mut out = Vec::with_capacity(charset.len());
	for name in charset {
		let mut glyph_name = name;
		if let Some(&count) = seen.get(&glyph_name) {
			let mut n = count;
			while all.contains(&format!("{glyph_name}.{n}"))
				|| seen.contains_key(&format!("{glyph_name}.{n}"))
			{
				n += 1;
			}
			seen.insert(glyph_name.clone(), n + 1);
			glyph_name = format!("{glyph_name}.{n}");
		}
		seen.insert(glyph_name.clone(), 1);
		out.push(glyph_name);
	}
	out
}

fn cff_data(font: &mut Font) -> Result<Vec<u8>> {
	match font.reader() {
		Some(r) if r.contains(CFF) && !font.is_loaded(CFF) => Ok(r.table_data(CFF)?.to_vec()),
		_ => font.table_data(CFF),
	}
}

/// The glyph order from the CFF charset (`table_C_F_F_.getGlyphOrder`).
pub fn glyph_order(font: &mut Font) -> Result<Vec<String>> {
	let data = cff_data(font)?;
	parse(&data)?.charset()
}

/// The `FontBBox` of the first top dict, rounded outwards (`intRect`).
pub fn font_bbox(font: &mut Font) -> Result<Option<[i64; 4]>> {
	let data = cff_data(font)?;
	let [x_min, y_min, x_max, y_max] = parse(&data)?.top.font_bbox;
	Ok(Some([
		x_min.floor() as i64,
		y_min.floor() as i64,
		x_max.ceil() as i64,
		y_max.ceil() as i64,
	]))
}
