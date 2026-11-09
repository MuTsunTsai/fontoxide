//! Delta-set index maps: `DeltaSetIndexMap` (index-keyed) and `VarIdxMap`
//! (glyph-keyed, in `HVAR` / `VVAR`). Both share one binary layout, which
//! fontTools reads with the `VarIdxMapValue` converter.

use std::collections::BTreeMap;

use fontoxide_misc::text_tools::safe_eval_int;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use super::conv::NO_VARIATION_INDEX;
use super::table::{OtTable, XmlCtx, simpletag};
use super::{OtReader, OtWriter};
use crate::{Error, Font, Result};

/// Reads the format, entry format and entries; the values are
/// `outer << 16 | inner`.
fn read_map(r: &mut OtReader<'_>) -> Result<(u16, Vec<u64>)> {
	let format = r.read_u8()? as u16;
	let (entry_format, count) = match format {
		0 => (r.read_u8()?, r.read_u16()? as usize),
		1 => (r.read_u8()?, r.read_u32()? as usize),
		// fontTools reads no fields for an unknown format.
		_ => return Err(Error::ttlib("KeyError: 'EntryFormat'")),
	};
	let fmt = entry_format as u64;
	let inner_bits = 1 + (fmt & 0x000F);
	let inner_mask = (1u64 << inner_bits) - 1;
	let outer_mask = 0xFFFF_FFFF - inner_mask;
	let outer_shift = 16 - inner_bits;
	let raw: Vec<u64> = match 1 + ((fmt & 0x0030) >> 4) {
		1 => r.read_u8_array(count)?.into_iter().map(u64::from).collect(),
		2 => r
			.read_u16_array(count)?
			.into_iter()
			.map(u64::from)
			.collect(),
		3 => r
			.read_u24_array(count)?
			.into_iter()
			.map(u64::from)
			.collect(),
		_ => r
			.read_u32_array(count)?
			.into_iter()
			.map(u64::from)
			.collect(),
	};
	if entry_format & 0xC0 != 0 {
		return Err(Error::Assertion(
			"(rawTable[\"EntryFormat\"] & 0xFFC0) == 0".into(),
		));
	}
	let mapping = raw
		.into_iter()
		.map(|v| ((v & outer_mask) << outer_shift) | (v & inner_mask))
		.collect();
	Ok((format, mapping))
}

/// `DeltaSetIndexMap.getEntryFormat`.
fn entry_format(mapping: &[u64]) -> u64 {
	let ored = mapping.iter().fold(0u64, |a, &v| a | v);
	let inner_bits = (64 - (ored & 0xFFFF).leading_zeros() as u64).max(1);
	let ored = (ored >> (16 - inner_bits)) | (ored & ((1 << inner_bits) - 1));
	let entry_size = match ored {
		0..=0xFF => 1,
		0x100..=0xFFFF => 2,
		0x1_0000..=0xFF_FFFF => 3,
		_ => 4,
	};
	((entry_size - 1) << 4) | (inner_bits - 1)
}

/// Writes the map (`preWrite` and the `VarIdxMapValue` converter).
fn write_map(w: &mut OtWriter<'_>, mapping: &[u64]) -> Result<()> {
	let fmt = entry_format(mapping);
	let format1 = mapping.len() > 0xFFFF;
	w.write_u8(format1 as u8);
	w.write_u8(fmt as u8);
	if format1 {
		w.write_u32(
			u32::try_from(mapping.len()).map_err(|_| {
				Error::Struct("'L' format requires 0 <= number <= 4294967295".into())
			})?,
		);
	} else {
		w.write_u16(mapping.len() as u16);
	}
	let inner_bits = 1 + (fmt & 0x000F);
	let inner_mask = (1u64 << inner_bits) - 1;
	let outer_shift = 16 - inner_bits;
	let entry_size = 1 + ((fmt & 0x0030) >> 4);
	for &idx in mapping {
		let v = ((idx & 0xFFFF_0000) >> outer_shift) | (idx & inner_mask);
		let too_big = || Error::Struct(format!("value {v} does not fit in {entry_size} bytes"));
		match entry_size {
			1 => w.write_u8(u8::try_from(v).map_err(|_| too_big())?),
			2 => w.write_u16(u16::try_from(v).map_err(|_| too_big())?),
			3 => w.write_u24(u32::try_from(v).map_err(|_| too_big())?)?,
			_ => w.write_u32(u32::try_from(v).map_err(|_| too_big())?),
		}
	}
	Ok(())
}

fn eval_attr(element: &Element, name: &str, default: Option<&str>) -> Result<i128> {
	let s = match element.attr(name) {
		Some(s) => s,
		None => default.ok_or_else(|| Error::ttx(format!("KeyError: {name:?}")))?,
	};
	safe_eval_int(s).ok_or_else(|| Error::value(format!("malformed node or string: {s:?}")))
}

/// `(outer << 16) | inner` from TTX attributes.
fn map_value(element: &Element, default: Option<&str>) -> Result<u64> {
	let outer = eval_attr(element, "outer", default)?;
	let inner = eval_attr(element, "inner", default)?;
	if inner > 0xFFFF {
		return Err(Error::Assertion("inner <= 0xFFFF".into()));
	}
	u64::try_from((outer << 16) | inner)
		.map_err(|_| Error::value(format!("unsupported variation index {outer}/{inner}")))
}

/// An index-keyed delta-set index map (`DeltaSetIndexMap`, kept by fontTools as
/// `mapping`): item index to variation index (`outer << 16 | inner`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeltaSetIndexMap {
	/// The `Format` attribute, as read (rewritten from the mapping size).
	pub format: Option<u16>,
	/// The variation index of each item.
	pub mapping: Vec<u64>,
}

impl OtTable for DeltaSetIndexMap {
	/// The mapping is by position, not by glyph: nothing to map.
	fn remap_glyphs(&mut self, _f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		Ok(())
	}

	fn class_name(&self) -> &'static str {
		"DeltaSetIndexMap"
	}

	fn decompile(r: &mut OtReader<'_>, _font: &mut Font) -> Result<Self> {
		let (format, mapping) = read_map(r)?;
		Ok(DeltaSetIndexMap {
			format: Some(format),
			mapping,
		})
	}

	fn compile(&self, w: &mut OtWriter<'_>, _font: &mut Font) -> Result<()> {
		write_map(w, &self.mapping)
	}

	fn format(&self) -> Option<u16> {
		self.format
	}

	fn with_format(format: u16) -> Self {
		DeltaSetIndexMap {
			format: Some(format),
			mapping: Vec::new(),
		}
	}

	fn to_xml2(&self, xw: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		xw.comment("Omitted values default to 0xFFFF/0xFFFF (no variations)");
		xw.newline();
		for (i, &value) in self.mapping.iter().enumerate() {
			let mut attrs = vec![("index", i.to_string())];
			if value != NO_VARIATION_INDEX as u64 {
				attrs.push(("outer", (value >> 16).to_string()));
				attrs.push(("inner", (value & 0xFFFF).to_string()));
			}
			simpletag(xw, "Map", &attrs);
			xw.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font, _ctx: &mut XmlCtx) -> Result<()> {
		let index = eval_attr(element, "index", None)?;
		let value = map_value(element, Some("0xFFFF"))?;
		// `list.insert`: the index is clamped to the list.
		let len = self.mapping.len() as i128;
		let at = if index < 0 {
			(len + index).max(0)
		} else {
			index.min(len)
		};
		self.mapping.insert(at as usize, value);
		Ok(())
	}

	fn populate_defaults(&mut self, _ctx: &mut XmlCtx) -> Result<()> {
		Ok(())
	}
}

/// A glyph-keyed delta-set index map (`VarIdxMap`, kept by fontTools as a
/// `mapping` dict by glyph name).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VarIdxMap {
	/// The variation index (`outer << 16 | inner`) of each glyph.
	pub mapping: BTreeMap<u32, u64>,
}

impl OtTable for VarIdxMap {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		self.mapping = super::aat::remap_keys(std::mem::take(&mut self.mapping), f)?;
		Ok(())
	}

	fn class_name(&self) -> &'static str {
		"VarIdxMap"
	}

	fn decompile(r: &mut OtReader<'_>, font: &mut Font) -> Result<Self> {
		let (_, mut list) = read_map(r)?;
		let n = font.glyph_order()?.len();
		// UPSTREAM-BUG(B6): `mapList[-1]` is evaluated even when nothing is added,
		// so an empty map fails to decompile.
		let last = *list
			.last()
			.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
		if list.len() < n {
			list.resize(n, last);
		}
		let mapping = list
			.into_iter()
			.take(n)
			.enumerate()
			.map(|(gid, v)| (gid as u32, v))
			.collect();
		Ok(VarIdxMap { mapping })
	}

	fn compile(&self, w: &mut OtWriter<'_>, font: &mut Font) -> Result<()> {
		let n = font.glyph_order()?.len();
		let mut list = Vec::with_capacity(n);
		for gid in 0..n as u32 {
			match self.mapping.get(&gid) {
				Some(&v) => list.push(v),
				None => {
					let name = font.glyph_order()?.name(gid).into_owned();
					return Err(Error::ttlib(format!("KeyError: {name:?}")));
				}
			}
		}
		while list.len() > 1 && list[list.len() - 2] == list[list.len() - 1] {
			list.pop();
		}
		write_map(w, &list)
	}

	fn to_xml2(&self, xw: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let order = font.glyph_order()?;
		let mut items: Vec<(String, u64)> = self
			.mapping
			.iter()
			.map(|(&gid, &v)| (order.name(gid).into_owned(), v))
			.collect();
		items.sort();
		for (glyph, value) in items {
			simpletag(
				xw,
				"Map",
				&[
					("glyph", glyph),
					("outer", (value >> 16).to_string()),
					("inner", (value & 0xFFFF).to_string()),
				],
			);
			xw.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font, _ctx: &mut XmlCtx) -> Result<()> {
		// Without `glyph`, fontTools indexes the glyph order with a string and fails.
		let glyph = element
			.attr("glyph")
			.ok_or_else(|| Error::ttx("TypeError: list indices must be integers or slices"))?;
		let value = map_value(element, None)?;
		self.mapping.insert(font.glyph_id(glyph)?, value);
		Ok(())
	}

	fn populate_defaults(&mut self, _ctx: &mut XmlCtx) -> Result<()> {
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::entry_format;

	#[test]
	fn entry_formats() {
		assert_eq!(entry_format(&[]), 0x00);
		assert_eq!(entry_format(&[0x0000_0003]), 0x01);
		assert_eq!(entry_format(&[0x0001_0000, 0x0000_0001]), 0x00);
		assert_eq!(entry_format(&[0xFFFF_FFFF]), 0x3F);
		assert_eq!(entry_format(&[0x0000_FFFF]), 0x1F);
	}
}
