//! AAT converters (`otConverters`): `AATLookup`, `AATLookupWithDataOffset`,
//! `StructWithLength`, `CIDGlyphMap` and `GlyphCIDMap`.

use std::collections::{BTreeMap, HashMap};

use fontoxide_misc::text_tools::safe_eval_int;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use super::table::{
	OtTable, XmlCtx, read_offset, simpletag, value_attr, write_offset, xml_read_struct, xml_struct,
};
use super::{OtReader, OtWriter, WriterGraph};
use crate::binary::get_search_range;
use crate::{Error, Font, Result};

/// A glyph-keyed AAT lookup (`AATLookup`), by glyph ID.
pub type AatLookup<V> = BTreeMap<u32, V>;

/// A value of an [`AatLookup`] whose glyph IDs can be remapped.
pub trait AatRemap {
	/// Maps the glyph IDs in the value.
	fn remap_value(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()>;
}

impl AatRemap for u16 {
	fn remap_value(&mut self, _f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		Ok(())
	}
}

/// `GlyphID` values.
impl AatRemap for u32 {
	fn remap_value(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		*self = f(*self)?;
		Ok(())
	}
}

impl<T: OtTable> AatRemap for Option<Box<T>> {
	fn remap_value(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		match self {
			Some(t) => t.remap_glyphs(f),
			None => Ok(()),
		}
	}
}

/// Maps the keys of a glyph-keyed map.
pub fn remap_keys<V>(
	map: BTreeMap<u32, V>,
	f: &mut dyn FnMut(u32) -> Result<u32>,
) -> Result<BTreeMap<u32, V>> {
	map.into_iter().map(|(g, v)| Ok((f(g)?, v))).collect()
}

/// Maps the glyphs of an AAT lookup: its keys and glyph values.
pub fn remap_aat_lookup<V: AatRemap>(
	lookup: &mut AatLookup<V>,
	f: &mut dyn FnMut(u32) -> Result<u32>,
) -> Result<()> {
	let mut out = BTreeMap::new();
	for (g, mut v) in std::mem::take(lookup) {
		v.remap_value(f)?;
		out.insert(f(g)?, v);
	}
	*lookup = out;
	Ok(())
}

/// A value type of an [`AatLookup`]: what `AATLookup.converter` reads.
pub trait AatValue: Sized + Clone + PartialEq {
	/// The size in the lookup (`converter.staticSize`).
	const SIZE: usize;
	/// Reads a value (`converter.read`).
	fn read(r: &mut OtReader<'_>, font: &mut Font) -> Result<Self>;
	/// Writes a value (`converter.write`).
	fn write(&self, w: &mut OtWriter<'_>, font: &mut Font) -> Result<()>;
	/// Writes the `Lookup` element of a glyph (`converter.xmlWrite`).
	fn xml_write(&self, xw: &mut XmlWriter, font: &mut Font, glyph: String) -> Result<()>;
	/// Reads a `Lookup` element (`converter.xmlRead`).
	fn xml_read(e: &Element, font: &mut Font, ctx: &mut XmlCtx) -> Result<Self>;
}

impl AatValue for u16 {
	const SIZE: usize = 2;
	fn read(r: &mut OtReader<'_>, _font: &mut Font) -> Result<Self> {
		r.read_u16()
	}
	fn write(&self, w: &mut OtWriter<'_>, _font: &mut Font) -> Result<()> {
		w.write_u16(*self);
		Ok(())
	}
	fn xml_write(&self, xw: &mut XmlWriter, _font: &mut Font, glyph: String) -> Result<()> {
		simpletag(
			xw,
			"Lookup",
			&[("glyph", glyph), ("value", self.to_string())],
		);
		xw.newline();
		Ok(())
	}
	fn xml_read(e: &Element, _font: &mut Font, _ctx: &mut XmlCtx) -> Result<Self> {
		// LENIENT(L9): range-checked when read rather than when compiled.
		let s = value_attr(e)?;
		let v = safe_eval_int(s)
			.ok_or_else(|| Error::value(format!("malformed node or string: {s:?}")))?;
		u16::try_from(v).map_err(|_| Error::Struct(format!("value {v} out of range")))
	}
}

/// A glyph value (`GlyphID`), by glyph ID.
impl AatValue for u32 {
	const SIZE: usize = 2;
	fn read(r: &mut OtReader<'_>, _font: &mut Font) -> Result<Self> {
		Ok(r.read_u16()? as u32)
	}
	fn write(&self, w: &mut OtWriter<'_>, _font: &mut Font) -> Result<()> {
		let v = u16::try_from(*self)
			.map_err(|_| Error::Struct("'H' format requires 0 <= number <= 65535".into()))?;
		w.write_u16(v);
		Ok(())
	}
	fn xml_write(&self, xw: &mut XmlWriter, font: &mut Font, glyph: String) -> Result<()> {
		let value = font.glyph_order()?.name(*self).into_owned();
		simpletag(xw, "Lookup", &[("glyph", glyph), ("value", value)]);
		xw.newline();
		Ok(())
	}
	fn xml_read(e: &Element, font: &mut Font, _ctx: &mut XmlCtx) -> Result<Self> {
		font.glyph_id(value_attr(e)?)
	}
}

/// A subtable behind a 16-bit offset (`Table`).
impl<T: OtTable + PartialEq> AatValue for Option<Box<T>> {
	const SIZE: usize = 2;
	fn read(r: &mut OtReader<'_>, font: &mut Font) -> Result<Self> {
		read_offset(r, font, 2)
	}
	fn write(&self, w: &mut OtWriter<'_>, font: &mut Font) -> Result<()> {
		write_offset(w, font, self.as_deref(), 2, "Value", None)
	}
	fn xml_write(&self, xw: &mut XmlWriter, font: &mut Font, glyph: String) -> Result<()> {
		xml_struct(
			xw,
			font,
			self.as_deref(),
			Some("Lookup"),
			&[("glyph", glyph)],
		)
	}
	fn xml_read(e: &Element, font: &mut Font, ctx: &mut XmlCtx) -> Result<Self> {
		Ok(xml_read_struct(e, font, ctx)?.map(Box::new))
	}
}

fn assertion(msg: impl Into<String>) -> Error {
	Error::Assertion(msg.into())
}

/// Reads a lookup (`AATLookup.read`).
pub fn read_aat_lookup<V: AatValue>(r: &mut OtReader<'_>, font: &mut Font) -> Result<AatLookup<V>> {
	let mut mapping = AatLookup::new();
	// The start of the lookup, at its format.
	let pos = r.pos();
	match r.read_u16()? {
		0 => {
			let n = font.glyph_order()?.len();
			for gid in 0..n as u32 {
				mapping.insert(gid, V::read(r, font)?);
			}
		}
		2 => {
			let unit_size = r.read_u16()? as usize;
			let num_units = r.read_u16()? as usize;
			if unit_size < 4 + V::SIZE {
				return Err(assertion(unit_size.to_string()));
			}
			for i in 0..num_units {
				r.seek(pos + i * unit_size + 12);
				let last = r.read_u16()? as u32;
				let first = r.read_u16()? as u32;
				let value = V::read(r, font)?;
				if last != 0xFFFF {
					for gid in first..=last {
						mapping.insert(gid, value.clone());
					}
				}
			}
		}
		4 => {
			let unit_size = r.read_u16()? as usize;
			if unit_size < 6 {
				return Err(assertion(unit_size.to_string()));
			}
			let num_units = r.read_u16()? as usize;
			for i in 0..num_units {
				r.seek(pos + i * unit_size + 12);
				let last = r.read_u16()? as u32;
				let first = r.read_u16()? as u32;
				let offset = r.read_u16()? as usize;
				if last != 0xFFFF {
					let mut data = r.sub_reader(0);
					data.seek(pos + offset);
					// `range(first, last + 1)`: empty when `last < first`.
					for gid in first..=last {
						mapping.insert(gid, V::read(&mut data, font)?);
					}
				}
			}
		}
		6 => {
			let unit_size = r.read_u16()? as usize;
			if unit_size < 2 + V::SIZE {
				return Err(assertion(unit_size.to_string()));
			}
			let num_units = r.read_u16()? as usize;
			for i in 0..num_units {
				r.seek(pos + i * unit_size + 12);
				let gid = r.read_u16()? as u32;
				let value = V::read(r, font)?;
				if gid != 0xFFFF {
					mapping.insert(gid, value);
				}
			}
		}
		8 => {
			let first = r.read_u16()? as u32;
			let count = r.read_u16()? as u32;
			for k in 0..count {
				mapping.insert(first + k, V::read(r, font)?);
			}
		}
		f => return Err(assertion(format!("unsupported lookup format: {f}"))),
	}
	Ok(mapping)
}

fn write_bin_search_header(w: &mut OtWriter<'_>, num_units: usize, unit_size: usize) -> Result<()> {
	let n = u16::try_from(num_units)
		.map_err(|_| Error::Struct("'H' format requires 0 <= number <= 65535".into()))?;
	w.write_u16(unit_size as u16);
	w.write_u16(n);
	let (range, selector, shift) = get_search_range(n as u32, unit_size as u32);
	for v in [range, selector, shift] {
		w.write_u16(
			u16::try_from(v)
				.map_err(|_| Error::Struct("'H' format requires 0 <= number <= 65535".into()))?,
		);
	}
	Ok(())
}

fn write_glyph_id(w: &mut OtWriter<'_>, gid: u32) -> Result<()> {
	w.write_u16(
		u16::try_from(gid)
			.map_err(|_| Error::Struct("'H' format requires 0 <= number <= 65535".into()))?,
	);
	Ok(())
}

/// Writes a lookup in the format that takes the fewest bytes, the lowest format
/// on ties (`AATLookup.write`).
pub fn write_aat_lookup<V: AatValue>(
	w: &mut OtWriter<'_>,
	font: &mut Font,
	lookup: &AatLookup<V>,
) -> Result<()> {
	let values: Vec<(u32, &V)> = lookup.iter().map(|(&g, v)| (g, v)).collect();
	// UPSTREAM-BUG(B10): `buildFormat2` takes `values[0]` unconditionally, so an
	// empty lookup fails with IndexError (reading one gives an empty lookup).
	let (&(first_gid, _), &(last_gid, _)) = values
		.first()
		.zip(values.last())
		.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
	let vs = V::SIZE;
	let mut segments: Vec<(u32, u32, &V)> = Vec::new();
	for &(gid, value) in &values {
		match segments.last_mut() {
			Some(s) if gid == s.1 + 1 && value == s.2 => s.1 = gid,
			_ => segments.push((gid, gid, value)),
		}
	}
	let mut formats = vec![
		(2 + 10 + (segments.len() + 1) * (vs + 4), 2),
		(2 + 10 + (values.len() + 1) * (vs + 2), 6),
	];
	if values.len() == font.glyph_order()?.len() {
		formats.push((2 + values.len() * vs, 0));
	}
	if values.len() as u64 == u64::from(last_gid - first_gid) + 1 {
		formats.push((6 + values.len() * vs, 8));
	}
	formats.sort();
	let (size, format) = formats[0];
	let before = w.data_length();
	match format {
		0 => {
			w.write_u16(0);
			for (_, v) in &values {
				v.write(w, font)?;
			}
		}
		2 => {
			w.write_u16(2);
			write_bin_search_header(w, segments.len(), vs + 4)?;
			for (first, last, v) in &segments {
				write_glyph_id(w, *last)?;
				write_glyph_id(w, *first)?;
				v.write(w, font)?;
			}
			w.write_u16(0xFFFF);
			w.write_u16(0xFFFF);
			w.write_data(&vec![0; vs]);
		}
		6 => {
			w.write_u16(6);
			write_bin_search_header(w, values.len(), vs + 2)?;
			for (gid, v) in &values {
				write_glyph_id(w, *gid)?;
				v.write(w, font)?;
			}
			w.write_u16(0xFFFF);
			w.write_data(&vec![0; vs]);
		}
		_ => {
			w.write_u16(8);
			write_glyph_id(w, first_gid)?;
			write_glyph_id(w, values.len() as u32)?;
			for (_, v) in &values {
				v.write(w, font)?;
			}
		}
	}
	let actual = w.data_length() - before;
	if actual != size {
		return Err(assertion(format!(
			"AATLookup format {format} claimed to write {size} bytes, but wrote {actual}"
		)));
	}
	Ok(())
}

/// Writes a lookup as TTX, sorted by glyph name (`AATLookup.xmlWrite`).
pub fn xml_aat_lookup<V: AatValue>(
	xw: &mut XmlWriter,
	font: &mut Font,
	name: &str,
	attrs: &[(&'static str, String)],
	lookup: &AatLookup<V>,
) -> Result<()> {
	super::table::begintag(xw, name, attrs);
	xw.newline();
	let order = font.glyph_order()?.clone();
	let mut items: Vec<(String, &V)> = lookup
		.iter()
		.map(|(&g, v)| (order.name(g).into_owned(), v))
		.collect();
	items.sort_by(|a, b| a.0.cmp(&b.0));
	for (glyph, v) in items {
		v.xml_write(xw, font, glyph)?;
	}
	xw.endtag(name);
	xw.newline();
	Ok(())
}

/// Reads a lookup from its `Lookup` children (`AATLookup.xmlRead`).
pub fn read_aat_lookup_xml<V: AatValue>(
	element: &Element,
	font: &mut Font,
	ctx: &mut XmlCtx,
) -> Result<AatLookup<V>> {
	let mut lookup = AatLookup::new();
	for e in element.elements().filter(|e| e.name == "Lookup") {
		let glyph = e
			.attr("glyph")
			.ok_or_else(|| Error::ttx("KeyError: 'glyph'"))?;
		let gid = font.glyph_id(glyph)?;
		lookup.insert(gid, V::xml_read(e, font, ctx)?);
	}
	Ok(lookup)
}

/// Reads an `ankr`-style lookup whose offsets are relative to a data table
/// (`AATLookupWithDataOffset.read`).
pub fn read_aat_lookup_with_data_offset<T: OtTable>(
	r: &mut OtReader<'_>,
	font: &mut Font,
) -> Result<AatLookup<Option<Box<T>>>> {
	let lookup_offset = r.read_u32()? as usize;
	let data_offset = r.read_u32()? as usize;
	let offsets: AatLookup<u16> = read_aat_lookup(&mut r.sub_reader(lookup_offset), font)?;
	let mut result = AatLookup::new();
	for (gid, offset) in offsets {
		let mut data = r.sub_reader(offset as usize + data_offset);
		result.insert(gid, Some(Box::new(T::decompile(&mut data, font)?)));
	}
	Ok(result)
}

/// Writes an `ankr`-style lookup: the glyph data is compiled separately and
/// shared when identical (`AATLookupWithDataOffset.write`).
pub fn write_aat_lookup_with_data_offset<T: OtTable>(
	w: &mut OtWriter<'_>,
	font: &mut Font,
	value: &AatLookup<Option<Box<T>>>,
) -> Result<()> {
	let tag = w.table_tag();
	let mut offsets: AatLookup<u16> = AatLookup::new();
	let mut by_data: HashMap<Vec<u8>, u16> = HashMap::new();
	let mut compiled = Vec::new();
	let mut len = 0usize;
	for (&gid, item) in value {
		let item = item.as_deref().ok_or_else(|| {
			Error::ttlib("AttributeError: 'NoneType' object has no attribute 'compile'")
		})?;
		let mut graph = WriterGraph::new(tag);
		item.compile(&mut graph.root(), font)?;
		let data = graph.get_all_data()?;
		let offset = match by_data.get(&data) {
			Some(&o) => o,
			None => {
				let o = u16::try_from(len).map_err(|_| {
					Error::Struct("'H' format requires 0 <= number <= 65535".into())
				})?;
				len += data.len();
				by_data.insert(data.clone(), o);
				compiled.push(data);
				o
			}
		};
		offsets.insert(gid, offset);
	}
	{
		let mut lookup = w.sub_table("DataOffsets", None, 4);
		write_aat_lookup(&mut lookup, font, &offsets)?;
	}
	let mut data = w.sub_table("DataOffsets", None, 4);
	for d in compiled {
		data.write_data(&d);
	}
	Ok(())
}

/// Where a `StructLength` field is and its value (see [`OtTable::struct_length`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructLength {
	/// The value read, if any.
	pub value: Option<u32>,
	/// The index of its item among those the table writes.
	pub item: usize,
	/// Its size in bytes.
	pub size: usize,
}

fn no_struct_length<T: OtTable>(value: &T) -> Error {
	Error::ttlib(format!(
		"AttributeError: '{}' object has no attribute 'StructLength'",
		value.class_name()
	))
}

/// Reads a struct and skips to its end, as given by its `StructLength`
/// (`StructWithLength.read`).
pub fn read_struct_with_length<T: OtTable>(r: &mut OtReader<'_>, font: &mut Font) -> Result<T> {
	let pos = r.pos();
	let value = T::decompile(r, font)?;
	let length = value
		.struct_length()
		.and_then(|s| s.value)
		.ok_or_else(|| no_struct_length(&value))?;
	r.seek(pos + length as usize);
	Ok(value)
}

/// Writes a struct, then fills in its `StructLength` (`StructWithLength.write`).
pub fn write_struct_with_length<T: OtTable>(
	w: &mut OtWriter<'_>,
	font: &mut Font,
	value: &T,
) -> Result<()> {
	let info = value
		.struct_length()
		.ok_or_else(|| no_struct_length(value))?;
	let index = w.item_count() + info.item;
	let before = w.data_length();
	value.compile(w, font)?;
	let length = w.data_length() - before;
	let data = match info.size {
		1 => vec![
			u8::try_from(length)
				.map_err(|_| Error::Struct("ubyte format requires 0 <= number <= 255".into()))?,
		],
		2 => u16::try_from(length)
			.map_err(|_| Error::Struct("'H' format requires 0 <= number <= 65535".into()))?
			.to_be_bytes()
			.to_vec(),
		_ => u32::try_from(length)
			.map_err(|_| Error::Struct("'L' format requires 0 <= number <= 4294967295".into()))?
			.to_be_bytes()
			.to_vec(),
	};
	w.replace_item(index, data);
	Ok(())
}

fn ushort(v: u32) -> Result<u16> {
	u16::try_from(v).map_err(|_| Error::Struct("'H' format requires 0 <= number <= 65535".into()))
}

/// Reads a CID to glyph map (`CIDGlyphMap.read`): glyph IDs by CID.
pub fn read_cid_glyph_map(r: &mut OtReader<'_>) -> Result<BTreeMap<u32, u32>> {
	let count = r.read_u16()? as usize;
	let gids = r.read_u16_array(count)?;
	Ok(gids
		.into_iter()
		.enumerate()
		.filter(|&(_, g)| g != 0xFFFF)
		.map(|(cid, g)| (cid as u32, g as u32))
		.collect())
}

/// Writes a CID to glyph map (`CIDGlyphMap.write`).
pub fn write_cid_glyph_map(w: &mut OtWriter<'_>, map: &BTreeMap<u32, u32>) -> Result<()> {
	let count = map.keys().next_back().map_or(0, |&c| c + 1);
	w.write_u16(ushort(count)?);
	for cid in 0..count {
		w.write_u16(ushort(map.get(&cid).copied().unwrap_or(0xFFFF))?);
	}
	Ok(())
}

/// Writes a CID to glyph map as TTX (`CIDGlyphMap.xmlWrite`).
pub fn xml_cid_glyph_map(
	xw: &mut XmlWriter,
	font: &mut Font,
	name: &str,
	attrs: &[(&'static str, String)],
	map: &BTreeMap<u32, u32>,
) -> Result<()> {
	super::table::begintag(xw, name, attrs);
	xw.newline();
	let order = font.glyph_order()?.clone();
	for (&cid, &gid) in map {
		xw.simpletag("CID", &[("cid", &cid), ("glyph", &order.name(gid))]);
		xw.newline();
	}
	xw.endtag(name);
	xw.newline();
	Ok(())
}

/// Reads a CID to glyph map from TTX (`CIDGlyphMap.xmlRead`).
pub fn read_cid_glyph_map_xml(element: &Element, font: &mut Font) -> Result<BTreeMap<u32, u32>> {
	let mut map = BTreeMap::new();
	for e in element.elements().filter(|e| e.name == "CID") {
		let cid = e.attr("cid").ok_or_else(|| Error::ttx("KeyError: 'cid'"))?;
		let cid = safe_eval_int(cid)
			.and_then(|v| u32::try_from(v).ok())
			.ok_or_else(|| Error::value(format!("invalid cid: {cid:?}")))?;
		let glyph = e
			.attr("glyph")
			.ok_or_else(|| Error::ttx("KeyError: 'glyph'"))?;
		map.insert(cid, font.glyph_id(glyph.trim())?);
	}
	Ok(map)
}

/// Reads a glyph to CID map (`GlyphCIDMap.read`): CIDs by glyph ID.
pub fn read_glyph_cid_map(r: &mut OtReader<'_>, font: &mut Font) -> Result<BTreeMap<u32, u32>> {
	let order = font.glyph_order()?.clone();
	let count = r.read_u16()? as usize;
	let cids = r.read_u16_array(count)?;
	if count > order.len() {
		log::warn!(
			"GlyphCIDMap has {count} elements, but the font has only {} glyphs; ignoring the rest",
			order.len()
		);
	}
	let mut map = BTreeMap::new();
	for (gid, &cid) in cids.iter().enumerate().take(order.len()) {
		if cid != 0xFFFF {
			// Keyed by name in fontTools: the last of duplicate names wins.
			let gid = order.id(&order.name(gid as u32)).unwrap_or(gid as u32);
			map.insert(gid, cid as u32);
		}
	}
	Ok(map)
}

/// Writes a glyph to CID map (`GlyphCIDMap.write`).
pub fn write_glyph_cid_map(w: &mut OtWriter<'_>, map: &BTreeMap<u32, u32>) -> Result<()> {
	let items: BTreeMap<u32, u32> = map
		.iter()
		.filter(|&(_, &c)| c != 0xFFFF)
		.map(|(&g, &c)| (g, c))
		.collect();
	let count = items.keys().next_back().map_or(0, |&g| g + 1);
	w.write_u16(ushort(count)?);
	for gid in 0..count {
		w.write_u16(ushort(items.get(&gid).copied().unwrap_or(0xFFFF))?);
	}
	Ok(())
}

/// Writes a glyph to CID map as TTX, sorted by glyph name (`GlyphCIDMap.xmlWrite`).
pub fn xml_glyph_cid_map(
	xw: &mut XmlWriter,
	font: &mut Font,
	name: &str,
	attrs: &[(&'static str, String)],
	map: &BTreeMap<u32, u32>,
) -> Result<()> {
	super::table::begintag(xw, name, attrs);
	xw.newline();
	let order = font.glyph_order()?.clone();
	let mut items: Vec<(String, u32)> = map
		.iter()
		.filter(|&(_, &c)| c != 0xFFFF)
		.map(|(&g, &c)| (order.name(g).into_owned(), c))
		.collect();
	items.sort();
	for (glyph, cid) in items {
		xw.simpletag("CID", &[("glyph", &glyph), ("value", &cid)]);
		xw.newline();
	}
	xw.endtag(name);
	xw.newline();
	Ok(())
}

/// Reads a glyph to CID map from TTX (`GlyphCIDMap.xmlRead`).
pub fn read_glyph_cid_map_xml(element: &Element, font: &mut Font) -> Result<BTreeMap<u32, u32>> {
	let mut map = BTreeMap::new();
	for e in element.elements().filter(|e| e.name == "CID") {
		let glyph = e
			.attr("glyph")
			.ok_or_else(|| Error::ttx("KeyError: 'glyph'"))?;
		let value = value_attr(e)?;
		let cid = safe_eval_int(value)
			.and_then(|v| u32::try_from(v).ok())
			.ok_or_else(|| Error::value(format!("invalid CID: {value:?}")))?;
		map.insert(font.glyph_id(glyph)?, cid);
	}
	Ok(map)
}
