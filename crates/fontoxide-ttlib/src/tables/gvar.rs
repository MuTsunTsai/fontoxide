//! `gvar` and `cvar`: Glyph Variations and CVT Variations tables
//! (`fontTools.ttLib.tables._g_v_a_r`, `_c_v_a_r`).

use std::collections::HashMap;

use fontoxide_misc::Tag;
use fontoxide_misc::text_tools::{parse_int_radix, safe_eval_int};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::sstruct::eval;
use crate::table::{Table, TableType};
use crate::tables::glyf::{Glyf, Glyph};
use crate::tables::tuple_variation::{
	TupleVariation, compile_shared_tuples, compile_tuple_variation_store, decompile_shared_tuples,
	decompile_tuple_variation_store,
};
use crate::tables::{Cvt, Fvar};
use crate::{Error, Font, Result};

const FVAR: Tag = Tag::new(b"fvar");

fn ushort(v: i64, name: &str) -> Result<[u8; 2]> {
	u16::try_from(v)
		.map(u16::to_be_bytes)
		.map_err(|_| Error::Struct(format!("'H' format requires 0 <= number <= 65535 ({name})")))
}

fn ulong(v: usize) -> Result<[u8; 4]> {
	u32::try_from(v)
		.map(u32::to_be_bytes)
		.map_err(|_| Error::Struct("'I' format requires 0 <= number <= 4294967295".into()))
}

fn be16(data: &[u8], at: usize) -> Result<u16> {
	data.get(at..at + 2)
		.map(|b| u16::from_be_bytes([b[0], b[1]]))
		.ok_or_else(|| Error::Struct("unpack requires a buffer of 2 bytes".into()))
}

fn be32(data: &[u8], at: usize) -> Result<u32> {
	data.get(at..at + 4)
		.map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
		.ok_or_else(|| Error::Struct("unpack requires a buffer of 4 bytes".into()))
}

fn fvar_axis_tags(font: &mut Font) -> Result<Vec<String>> {
	Ok(font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| a.tag_text())
		.collect())
}

/// The number of points of a glyph including the four phantom points
/// (`getNumPoints_`).
fn num_points(glyph: &Glyph) -> usize {
	if glyph.is_composite() {
		glyph.components.len() + 4
	} else {
		glyph.coordinates.len() + 4
	}
}

fn glyph_points(font: &mut Font, gid: u32, extended: bool) -> Result<usize> {
	let order = font.glyph_order()?;
	let glyf = font.require_locked::<Glyf>(extended)?;
	let glyph = glyf
		.glyphs
		.get(&gid)
		.ok_or_else(|| Error::ttlib(format!("KeyError: {:?}", order.name(gid))))?;
	Ok(num_points(glyph))
}

/// The size of the header, with its glyph count: 16 bits in `gvar`, 24 bits in `GVAR`.
fn header_size(extended: bool) -> usize {
	if extended { 21 } else { 20 }
}

/// Glyph Variations table (`table__g_v_a_r`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/gvar>.
#[derive(Debug, Clone, PartialEq)]
pub struct Gvar {
	/// `version`.
	pub version: i64,
	/// `reserved`.
	pub reserved: i64,
	/// The number of axes, used when there is no `fvar` (`axisCount`).
	pub axis_count: i64,
	/// The variations of each glyph, in the order fontTools' dict keeps them
	/// (`variations`, keyed by glyph name there).
	pub variations: Vec<(u32, Vec<TupleVariation>)>,
	/// Whether the TTX numbered the axes, which it does without `fvar`.
	pub numeric_axis_tags: bool,
	/// Whether this is `GVAR`, the beyond-64k companion of the table.
	pub extended: bool,
}

impl Default for Gvar {
	fn default() -> Self {
		Gvar {
			version: 1,
			reserved: 0,
			axis_count: 0,
			variations: Vec::new(),
			numeric_axis_tags: false,
			extended: false,
		}
	}
}

impl Gvar {
	/// The axis tags: from `fvar`, or numbers (`getAxisTags_`).
	fn axis_tags(&self, font: &mut Font) -> Result<Vec<String>> {
		if font.contains(FVAR) {
			return fvar_axis_tags(font);
		}
		Ok((0..self.axis_count.max(0)).map(|i| i.to_string()).collect())
	}

	/// The variations of a glyph.
	pub fn get(&self, gid: u32) -> Option<&[TupleVariation]> {
		self.variations
			.iter()
			.find(|(g, _)| *g == gid)
			.map(|(_, v)| v.as_slice())
	}

	/// Sets the variations of a glyph, keeping its place if it has some.
	pub fn set(&mut self, gid: u32, variations: Vec<TupleVariation>) {
		match self.variations.iter_mut().find(|(g, _)| *g == gid) {
			Some(entry) => entry.1 = variations,
			None => self.variations.push((gid, variations)),
		}
	}
}

/// `compileOffsets_`: the packed offsets, as short offsets (halved) if they fit, and the
/// table format that goes in the flags of the header: 0 for short, 1 for long.
pub fn compile_offsets(offsets: &[usize]) -> Result<(Vec<u8>, u16)> {
	let long = offsets.iter().any(|&o| o > 0xFFFF * 2);
	let mut packed = Vec::with_capacity(offsets.len() * if long { 4 } else { 2 });
	for &o in offsets {
		if long {
			packed.extend_from_slice(&ulong(o)?);
		} else {
			packed.extend_from_slice(&((o >> 1) as u16).to_be_bytes());
		}
	}
	Ok((packed, u16::from(long)))
}

/// `decompileOffsets_`: the offsets of `glyph_count` glyphs and the end, as far as `data`
/// has them.
pub fn decompile_offsets(data: &[u8], table_format: u16, glyph_count: usize) -> Vec<usize> {
	let count = glyph_count + 1;
	if table_format == 0 {
		// In the short format, offsets need to be multiplied by 2.
		data.as_chunks::<2>()
			.0
			.iter()
			.take(count)
			.map(|&b| 2 * u16::from_be_bytes(b) as usize)
			.collect()
	} else {
		data.as_chunks::<4>()
			.0
			.iter()
			.take(count)
			.map(|&b| u32::from_be_bytes(b) as usize)
			.collect()
	}
}

/// `compileGlyph_`.
fn compile_glyph(
	variations: &[TupleVariation],
	axis_tags: &[String],
	shared: &HashMap<Vec<u8>, usize>,
	extended: bool,
) -> Result<Vec<u8>> {
	let (count, tuples, data) =
		compile_tuple_variation_store(variations, axis_tags, shared, true, true)?;
	if count == 0 {
		return Ok(Vec::new());
	}
	let offset_size = if extended { 3 } else { 2 };
	let offset_to_data = 2 + offset_size + tuples.len();
	let mut out = Vec::with_capacity(offset_to_data + data.len() + 1);
	let int = |v: usize| {
		u16::try_from(v)
			.map(u16::to_be_bytes)
			.map_err(|_| Error::value("OverflowError: int too big to convert"))
	};
	out.extend_from_slice(&int(count as usize)?);
	if extended {
		if offset_to_data > 0xFF_FFFF {
			return Err(Error::value("OverflowError: int too big to convert"));
		}
		out.extend_from_slice(&(offset_to_data as u32).to_be_bytes()[1..]);
	} else {
		out.extend_from_slice(&int(offset_to_data)?);
	}
	out.extend(tuples);
	let odd = (offset_to_data + data.len()) % 2 != 0;
	out.extend(data);
	if odd {
		out.push(0);
	}
	Ok(out)
}

impl TableType for Gvar {
	const TAG: Tag = Tag::new(b"gvar");
	const COMPANION: Option<Tag> = Some(Tag::new(b"GVAR"));

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		Self::decompile_as(Self::TAG, data, font)
	}

	fn new_as(tag: Tag) -> Self {
		Gvar {
			extended: tag == Tag::new(b"GVAR"),
			..Default::default()
		}
	}

	fn decompile_as(tag: Tag, data: &[u8], font: &mut Font) -> Result<Self> {
		let extended = tag == Tag::new(b"GVAR");
		// What follows the glyph count moves with its size.
		let gid_size = if extended { 3 } else { 2 };
		let tail = 12 + gid_size;
		let order = font.glyph_order()?;
		let version = be16(data, 0)? as i64;
		let reserved = be16(data, 2)? as i64;
		let axis_count = be16(data, 4)? as i64;
		let shared_count = be16(data, 6)? as usize;
		let shared_offset = be32(data, 8)? as usize;
		let glyph_count = data
			.get(12..tail)
			.map_or(0, |b| b.iter().fold(0usize, |a, &x| a << 8 | x as usize));
		let flags = be16(data, tail)?;
		let data_offset = be32(data, tail + 2)? as usize;
		let mut table = Gvar {
			version,
			reserved,
			axis_count,
			extended,
			..Default::default()
		};
		let axis_tags = table.axis_tags(font)?;
		if order.len() != glyph_count {
			return Err(Error::Assertion(format!(
				"({}, {glyph_count})",
				order.len()
			)));
		}
		if axis_tags.len() as i64 != axis_count {
			return Err(Error::Assertion(format!(
				"({}, {axis_count})",
				axis_tags.len()
			)));
		}
		let shared = decompile_shared_tuples(&axis_tags, shared_count, data, shared_offset)?;
		font.require_locked::<Glyf>(extended)?;
		let offsets = decompile_offsets(
			data.get(header_size(extended)..).unwrap_or_default(),
			flags & 1,
			glyph_count,
		);
		for gid in 0..glyph_count {
			let (Some(&o0), Some(&o1)) = (offsets.get(gid), offsets.get(gid + 1)) else {
				return Err(Error::ttlib("IndexError: array index out of range"));
			};
			let from = (data_offset + o0).min(data.len());
			let to = (data_offset + o1).min(data.len()).max(from);
			let glyph_data = &data[from..to];
			let variations = if glyph_data.is_empty() {
				Vec::new()
			} else {
				let points = glyph_points(font, gid as u32, extended)?;
				if glyph_data.len() < 2 + gid_size {
					Vec::new()
				} else {
					let count = u16::from_be_bytes([glyph_data[0], glyph_data[1]]);
					let offset = glyph_data[2..2 + gid_size]
						.iter()
						.fold(0usize, |a, &x| a << 8 | x as usize);
					decompile_tuple_variation_store(
						"gvar",
						&axis_tags,
						count,
						points,
						Some(&shared),
						glyph_data,
						2 + gid_size,
						offset,
					)?
				}
			};
			table.variations.push((gid as u32, variations));
		}
		Ok(table)
	}
}

impl Table for Gvar {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		for (g, _) in &mut self.variations {
			*g = f(*g)?;
		}
		self.variations.sort_by_key(|(g, _)| *g);
		Ok(())
	}

	fn tag(&self) -> Tag {
		Font::locked_tag::<Self>(self.extended)
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		let axis_tags = self.axis_tags(font)?;
		let shared = compile_shared_tuples(
			&axis_tags,
			self.variations.iter().flat_map(|(_, v)| v.iter()),
		)?;
		let indices: HashMap<Vec<u8>, usize> = shared
			.iter()
			.enumerate()
			.map(|(i, c)| (c.clone(), i))
			.collect();
		let shared_size: usize = shared.iter().map(Vec::len).sum();
		let order = font.glyph_order()?;
		font.require_locked::<Glyf>(self.extended)?;
		let by_gid: HashMap<u32, &Vec<TupleVariation>> =
			self.variations.iter().map(|(g, v)| (*g, v)).collect();
		let mut glyphs = Vec::with_capacity(order.len());
		for gid in 0..order.len() as u32 {
			glyphs.push(match by_gid.get(&gid) {
				Some(v) if !v.is_empty() => compile_glyph(v, &axis_tags, &indices, self.extended)?,
				_ => Vec::new(),
			});
		}
		let mut offsets = Vec::with_capacity(glyphs.len() + 1);
		let mut at = 0usize;
		for g in &glyphs {
			offsets.push(at);
			at += g.len();
		}
		offsets.push(at);
		let (packed, table_format) = compile_offsets(&offsets)?;
		let shared_offset = header_size(self.extended) + packed.len();
		let mut out = Vec::new();
		out.extend_from_slice(&ushort(self.version, "version")?);
		out.extend_from_slice(&ushort(self.reserved, "reserved")?);
		out.extend_from_slice(&ushort(axis_tags.len() as i64, "axisCount")?);
		out.extend_from_slice(&ushort(shared.len() as i64, "sharedTupleCount")?);
		out.extend_from_slice(&ulong(shared_offset)?);
		if self.extended {
			if glyphs.len() > 0xFF_FFFF {
				return Err(Error::value("OverflowError: int too big to convert"));
			}
			out.extend_from_slice(&(glyphs.len() as u32).to_be_bytes()[1..]);
		} else {
			out.extend_from_slice(
				&u16::try_from(glyphs.len())
					.map_err(|_| Error::value("OverflowError: int too big to convert"))?
					.to_be_bytes(),
			);
		}
		out.extend_from_slice(&table_format.to_be_bytes());
		out.extend_from_slice(&ulong(shared_offset + shared_size)?);
		out.extend(packed);
		for s in shared {
			out.extend(s);
		}
		for g in glyphs {
			out.extend(g);
		}
		Ok(out)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		writer.simpletag("version", &[("value", &self.version)]);
		writer.newline();
		writer.simpletag("reserved", &[("value", &self.reserved)]);
		writer.newline();
		if !font.contains(FVAR) {
			writer.simpletag("axisCount", &[("value", &self.axis_count)]);
			writer.newline();
		}
		let axis_tags = self.axis_tags(font)?;
		let order = font.glyph_order()?;
		let mut names: Vec<&String> = order.names().iter().collect();
		names.sort();
		for name in names {
			let Some(variations) = order.id(name).and_then(|gid| self.get(gid)) else {
				continue;
			};
			if variations.is_empty() {
				continue;
			}
			writer.begintag("glyphVariations", &[("glyph", name)]);
			writer.newline();
			for v in variations {
				v.to_xml(writer, &axis_tags);
			}
			writer.endtag("glyphVariations");
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		let value = || -> Result<i64> {
			let v = element
				.attr("value")
				.ok_or_else(|| Error::ttx("KeyError: 'value'"))?;
			eval(v)?
				.as_int()
				.and_then(|i| i64::try_from(i).ok())
				.ok_or_else(|| Error::value(format!("invalid value: {v:?}")))
		};
		match element.name.as_str() {
			"version" => self.version = value()?,
			"reserved" => self.reserved = value()?,
			"axisCount" => {
				self.axis_count = value()?;
				self.numeric_axis_tags = true;
			}
			"glyphVariations" => {
				let name = element
					.attr("glyph")
					.ok_or_else(|| Error::ttx("KeyError: 'glyph'"))?;
				let gid = font.glyph_id(name)?;
				let points = glyph_points(font, gid, self.extended)?;
				let mut variations = Vec::new();
				for t in element.elements().filter(|e| e.name == "tuple") {
					let mut v = TupleVariation::new(points);
					for child in t.elements() {
						v.from_xml(child)?;
					}
					if self.numeric_axis_tags {
						v.axes = v
							.axes
							.into_iter()
							.map(|(axis, region)| {
								let n = safe_eval_int(&axis).ok_or_else(|| {
									Error::value(format!("malformed node or string: {axis:?}"))
								})?;
								Ok((n.to_string(), region))
							})
							.collect::<Result<_>>()?;
					}
					variations.push(v);
				}
				self.set(gid, variations);
			}
			_ => {}
		}
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}

/// CVT Variations table (`table__c_v_a_r`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/cvar>.
#[derive(Debug, Clone, PartialEq)]
pub struct Cvar {
	/// `majorVersion`.
	pub major_version: i64,
	/// `minorVersion`.
	pub minor_version: i64,
	/// The variations of the control values (`variations`).
	pub variations: Vec<TupleVariation>,
}

impl Default for Cvar {
	fn default() -> Self {
		Cvar {
			major_version: 1,
			minor_version: 0,
			variations: Vec::new(),
		}
	}
}

fn cvt_count(font: &mut Font) -> Result<usize> {
	Ok(font.require::<Cvt>()?.values.len())
}

impl TableType for Cvar {
	const TAG: Tag = Tag::new(b"cvar");

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		let axis_tags = fvar_axis_tags(font)?;
		let major = be16(data, 0)? as i64;
		let minor = be16(data, 2)? as i64;
		let count = be16(data, 4)?;
		let offset = be16(data, 6)? as usize;
		if major != 1 {
			return Err(Error::Assertion(major.to_string()));
		}
		let points = cvt_count(font)?;
		let variations = decompile_tuple_variation_store(
			"cvar", &axis_tags, count, points, None, data, 8, offset,
		)?;
		Ok(Cvar {
			major_version: major,
			minor_version: minor,
			variations,
		})
	}
}

impl Table for Cvar {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Ok(())
	}

	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		let variations: Vec<TupleVariation> = self
			.variations
			.iter()
			.filter(|v| v.has_impact())
			.cloned()
			.collect();
		cvt_count(font)?;
		let axis_tags = fvar_axis_tags(font)?;
		let (count, tuples, data) =
			compile_tuple_variation_store(&variations, &axis_tags, &HashMap::new(), false, true)?;
		let mut out = Vec::new();
		out.extend_from_slice(&ushort(self.major_version, "majorVersion")?);
		out.extend_from_slice(&ushort(self.minor_version, "minorVersion")?);
		out.extend_from_slice(&ushort(count as i64, "tupleVariationCount")?);
		out.extend_from_slice(&ushort(8 + tuples.len() as i64, "offsetToData")?);
		out.extend(tuples);
		out.extend(data);
		Ok(out)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let axis_tags = fvar_axis_tags(font)?;
		writer.simpletag(
			"version",
			&[
				("major", &self.major_version),
				("minor", &self.minor_version),
			],
		);
		writer.newline();
		for v in &self.variations {
			v.to_xml(writer, &axis_tags);
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"version" => {
				let int = |name: &str, default: &str| -> Result<i64> {
					let s = element.attr(name).unwrap_or(default);
					parse_int_radix(s, 10)
						.and_then(|v| i64::try_from(v).ok())
						.ok_or_else(|| {
							Error::value(format!("invalid literal for int() with base 10: {s:?}"))
						})
				};
				self.major_version = int("major", "1")?;
				self.minor_version = int("minor", "0")?;
			}
			"tuple" => {
				let mut v = TupleVariation::new(cvt_count(font)?);
				for child in element.elements() {
					v.from_xml(child)?;
				}
				self.variations.push(v);
			}
			_ => {}
		}
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}
