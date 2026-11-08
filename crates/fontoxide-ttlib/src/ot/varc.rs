//! Variable composite glyphs (`VarComponent` and `VarCompositeGlyph` of
//! `fontTools.ttLib.tables.otTables`) and the CFF2-style INDEX converters of the
//! `VARC` table (`CFF2Index`, `TupleListOffset`).

use std::rc::Rc;

use fontoxide_misc::fixed_tools::{
	fixed_to_float, float_to_fixed, float_to_fixed_to_float, float_to_fixed_to_str,
};
use fontoxide_misc::py::float_repr;
use fontoxide_misc::text_tools::{Literal, safe_eval};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::{AttrValue, XmlWriter};

use super::conv::parse_int_list;
use super::state::StateValue;
use super::table::{int_list_repr, simpletag, xml_simple};
use super::{OtReader, OtWriter};
use crate::tables::tuple_variation::{compile_delta_values, decompile_deltas};
use crate::{Error, Font, Result};

/// `NO_VARIATION_INDEX`.
pub const NO_VARIATION_INDEX: u64 = 0xFFFF_FFFF;

/// The bits of `VarComponent.flags` (`VarComponentFlags`).
pub mod flags {
	#![allow(missing_docs)]
	pub const RESET_UNSPECIFIED_AXES: u64 = 1 << 0;
	pub const HAVE_AXES: u64 = 1 << 1;
	pub const AXIS_VALUES_HAVE_VARIATION: u64 = 1 << 2;
	pub const TRANSFORM_HAS_VARIATION: u64 = 1 << 3;
	pub const HAVE_TRANSLATE_X: u64 = 1 << 4;
	pub const HAVE_TRANSLATE_Y: u64 = 1 << 5;
	pub const HAVE_ROTATION: u64 = 1 << 6;
	pub const HAVE_CONDITION: u64 = 1 << 7;
	pub const HAVE_SCALE_X: u64 = 1 << 8;
	pub const HAVE_SCALE_Y: u64 = 1 << 9;
	pub const HAVE_TCENTER_X: u64 = 1 << 10;
	pub const HAVE_TCENTER_Y: u64 = 1 << 11;
	pub const GID_IS_24BIT: u64 = 1 << 12;
	pub const HAVE_SKEW_X: u64 = 1 << 13;
	pub const HAVE_SKEW_Y: u64 = 1 << 14;
	pub const RESERVED_MASK: u64 = (1 << 32) - (1 << 15);
}

/// `flags & ~flag` with a `VarComponentFlags` member, which inverts within 32
/// bits.
///
/// UPSTREAM-BUG(B4): so flag bits above bit 31 that were read are silently
/// dropped on compile.
fn clear(value: u64, flag: u64) -> u64 {
	value & (0xFFFF_FFFF ^ flag)
}

/// The transform of a component.
pub use fontoxide_misc::transform::DecomposedTransform;

/// How a transform component is stored (`VAR_TRANSFORM_MAPPING`).
struct Mapping {
	name: &'static str,
	flag: u64,
	fractional_bits: u32,
	scale: f64,
	field: fn(&mut DecomposedTransform) -> &mut f64,
}

const MAPPING: [Mapping; 9] = [
	Mapping {
		name: "translateX",
		flag: flags::HAVE_TRANSLATE_X,
		fractional_bits: 0,
		scale: 1.0,
		field: |t| &mut t.translate_x,
	},
	Mapping {
		name: "translateY",
		flag: flags::HAVE_TRANSLATE_Y,
		fractional_bits: 0,
		scale: 1.0,
		field: |t| &mut t.translate_y,
	},
	Mapping {
		name: "rotation",
		flag: flags::HAVE_ROTATION,
		fractional_bits: 12,
		scale: 180.0,
		field: |t| &mut t.rotation,
	},
	Mapping {
		name: "scaleX",
		flag: flags::HAVE_SCALE_X,
		fractional_bits: 10,
		scale: 1.0,
		field: |t| &mut t.scale_x,
	},
	Mapping {
		name: "scaleY",
		flag: flags::HAVE_SCALE_Y,
		fractional_bits: 10,
		scale: 1.0,
		field: |t| &mut t.scale_y,
	},
	Mapping {
		name: "skewX",
		flag: flags::HAVE_SKEW_X,
		fractional_bits: 12,
		scale: -180.0,
		field: |t| &mut t.skew_x,
	},
	Mapping {
		name: "skewY",
		flag: flags::HAVE_SKEW_Y,
		fractional_bits: 12,
		scale: 180.0,
		field: |t| &mut t.skew_y,
	},
	Mapping {
		name: "tCenterX",
		flag: flags::HAVE_TCENTER_X,
		fractional_bits: 0,
		scale: 1.0,
		field: |t| &mut t.t_center_x,
	},
	Mapping {
		name: "tCenterY",
		flag: flags::HAVE_TCENTER_Y,
		fractional_bits: 0,
		scale: 1.0,
		field: |t| &mut t.t_center_y,
	},
];

fn index_error() -> Error {
	Error::ttlib("IndexError: index out of range")
}

/// Reads a variable-length number (`_read_uint32var`); returns it and the next
/// index.
pub fn read_uint32var(data: &[u8], i: usize) -> Result<(u64, usize)> {
	let b = |k: usize| data.get(i + k).map(|&v| v as u64).ok_or_else(index_error);
	let b0 = b(0)?;
	Ok(if b0 < 0x80 {
		(b0, i + 1)
	} else if b0 < 0xC0 {
		((b0 - 0x80) << 8 | b(1)?, i + 2)
	} else if b0 < 0xE0 {
		((b0 - 0xC0) << 16 | b(1)? << 8 | b(2)?, i + 3)
	} else if b0 < 0xF0 {
		((b0 - 0xE0) << 24 | b(1)? << 16 | b(2)? << 8 | b(3)?, i + 4)
	} else {
		(
			(b0 - 0xF0) << 32 | b(1)? << 24 | b(2)? << 16 | b(3)? << 8 | b(4)?,
			i + 5,
		)
	})
}

/// Writes a variable-length number (`_write_uint32var`).
pub fn write_uint32var(v: u64, out: &mut Vec<u8>) -> Result<()> {
	let v32 = || {
		u32::try_from(v)
			.map_err(|_| Error::Struct("'L' format requires 0 <= number <= 4294967295".into()))
	};
	if v < 0x80 {
		out.push(v as u8);
	} else if v < 0x4000 {
		out.extend_from_slice(&((v | 0x8000) as u16).to_be_bytes());
	} else if v < 0x20_0000 {
		out.extend_from_slice(&((v | 0xC0_0000) as u32).to_be_bytes()[1..]);
	} else if v < 0x1000_0000 {
		out.extend_from_slice(&((v | 0xE000_0000) as u32).to_be_bytes());
	} else {
		out.push(0xF0);
		out.extend_from_slice(&v32()?.to_be_bytes());
	}
	Ok(())
}

/// One component of a variable composite glyph (`VarComponent`).
#[derive(Debug, Clone, PartialEq)]
pub struct VarComponent {
	/// The flags (`VarComponentFlags`); compiling recomputes all but
	/// `RESET_UNSPECIFIED_AXES`, the reserved bits and the transform bits.
	pub flags: u64,
	/// The component glyph.
	pub glyph: u32,
	/// Index into the `ConditionList`.
	pub condition_index: Option<u64>,
	/// Index into the `AxisIndicesList`.
	pub axis_indices_index: Option<u64>,
	/// The normalized axis values.
	pub axis_values: Vec<f64>,
	/// Variation index of the axis values, or [`NO_VARIATION_INDEX`].
	pub axis_values_var_index: u64,
	/// Variation index of the transform, or [`NO_VARIATION_INDEX`].
	pub transform_var_index: u64,
	/// The transform; only components whose flag is set are stored.
	pub transform: DecomposedTransform,
}

impl Default for VarComponent {
	fn default() -> Self {
		VarComponent {
			flags: 0,
			glyph: 0,
			condition_index: None,
			axis_indices_index: None,
			axis_values: Vec::new(),
			axis_values_var_index: NO_VARIATION_INDEX,
			transform_var_index: NO_VARIATION_INDEX,
			transform: DecomposedTransform::default(),
		}
	}
}

impl VarComponent {
	/// Reads a component from `data[i..]` (`decompile`); returns the index after
	/// it. `axis_counts` is the length of each `AxisIndicesList` item.
	fn decompile(
		data: &[u8],
		mut i: usize,
		num_glyphs: usize,
		axis_counts: Option<&[usize]>,
	) -> Result<(Self, usize)> {
		let mut c = VarComponent::default();
		let flags;
		(flags, i) = read_uint32var(data, i)?;
		c.flags = flags;

		let gid_size = if flags & flags::GID_IS_24BIT != 0 {
			3
		} else {
			2
		};
		let gid = data
			.get(i..i + gid_size)
			.ok_or_else(|| Error::Struct(format!("unpack requires a buffer of {gid_size} bytes")))?
			.iter()
			.fold(0u32, |acc, &b| acc << 8 | b as u32);
		i += gid_size;
		if gid as usize >= num_glyphs {
			return Err(Error::ttlib("IndexError: list index out of range"));
		}
		c.glyph = gid;

		if flags & flags::HAVE_CONDITION != 0 {
			let v;
			(v, i) = read_uint32var(data, i)?;
			c.condition_index = Some(v);
		}
		if flags & flags::HAVE_AXES != 0 {
			let v;
			(v, i) = read_uint32var(data, i)?;
			c.axis_indices_index = Some(v);
			let counts = axis_counts.ok_or_else(|| Error::ttlib("KeyError: 'AxisIndicesList'"))?;
			let num_axes = *counts
				.get(v as usize)
				.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
			let values;
			(values, i) = decompile_deltas(Some(num_axes), data, i)?;
			c.axis_values = values.iter().map(|&v| fixed_to_float(v, 14)).collect();
		}
		if flags & flags::AXIS_VALUES_HAVE_VARIATION != 0 {
			(c.axis_values_var_index, i) = read_uint32var(data, i)?;
		}
		if flags & flags::TRANSFORM_HAS_VARIATION != 0 {
			(c.transform_var_index, i) = read_uint32var(data, i)?;
		}

		for m in &MAPPING {
			if flags & m.flag != 0 {
				let b = data
					.get(i..i + 2)
					.ok_or_else(|| Error::Struct("unpack requires a buffer of 2 bytes".into()))?;
				let raw = i16::from_be_bytes([b[0], b[1]]) as i64;
				*(m.field)(&mut c.transform) = fixed_to_float(raw, m.fractional_bits) * m.scale;
				i += 2;
			}
		}
		if flags & flags::HAVE_SCALE_Y == 0 {
			c.transform.scale_y = c.transform.scale_x;
		}

		// UPSTREAM-BUG(B3): the values of reserved flags are skipped and lost, but
		// the flags themselves are kept, so `compile` writes flags without values.
		let mut n = flags & flags::RESERVED_MASK;
		while n != 0 {
			(_, i) = read_uint32var(data, i)?;
			n &= n - 1;
		}
		Ok((c, i))
	}

	/// Adds the transform deltas of a variation (`applyTransformDeltas`), in the
	/// units stored in the font.
	pub fn apply_transform_deltas(&mut self, deltas: &[f64]) -> Result<()> {
		let mut i = 0;
		for m in &MAPPING {
			let value = if self.flags & m.flag != 0 {
				let d = deltas.get(i).ok_or_else(index_error)?;
				i += 1;
				d / (1u64 << m.fractional_bits) as f64 * m.scale
			} else {
				0.0
			};
			*(m.field)(&mut self.transform) += value;
		}
		if self.flags & flags::HAVE_SCALE_Y == 0 {
			self.transform.scale_y = self.transform.scale_x;
		}
		if i != deltas.len() {
			return Err(Error::Assertion(format!("({i}, {})", deltas.len())));
		}
		Ok(())
	}

	/// Writes the component (`compile`).
	fn compile(&self, out: &mut Vec<u8>) -> Result<()> {
		let mut data = Vec::new();
		let mut flags = self.flags;

		let gid = self.glyph;
		if gid > 65535 {
			flags |= flags::GID_IS_24BIT;
			data.extend_from_slice(&gid.to_be_bytes()[1..]);
		} else {
			flags = clear(flags, flags::GID_IS_24BIT);
			data.extend_from_slice(&(gid as u16).to_be_bytes());
		}

		match self.condition_index {
			Some(v) => {
				flags |= flags::HAVE_CONDITION;
				write_uint32var(v, &mut data)?;
			}
			None => flags = clear(flags, flags::HAVE_CONDITION),
		}

		if !self.axis_values.is_empty() {
			flags |= flags::HAVE_AXES;
			let index = self.axis_indices_index.ok_or_else(|| {
				Error::ttlib("TypeError: '<' not supported between 'NoneType' and 'int'")
			})?;
			write_uint32var(index, &mut data)?;
			let values: Vec<i64> = self
				.axis_values
				.iter()
				.map(|&v| float_to_fixed(v, 14))
				.collect();
			data.extend(compile_delta_values(&values, true)?);
		} else {
			flags = clear(flags, flags::HAVE_AXES);
		}

		if self.axis_values_var_index != NO_VARIATION_INDEX {
			flags |= flags::AXIS_VALUES_HAVE_VARIATION;
			write_uint32var(self.axis_values_var_index, &mut data)?;
		} else {
			flags = clear(flags, flags::AXIS_VALUES_HAVE_VARIATION);
		}
		if self.transform_var_index != NO_VARIATION_INDEX {
			flags |= flags::TRANSFORM_HAS_VARIATION;
			write_uint32var(self.transform_var_index, &mut data)?;
		} else {
			flags = clear(flags, flags::TRANSFORM_HAS_VARIATION);
		}

		let mut transform = self.transform;
		for m in &MAPPING {
			if flags & m.flag != 0 {
				let v = float_to_fixed(*(m.field)(&mut transform) / m.scale, m.fractional_bits);
				let v = i16::try_from(v).map_err(|_| {
					Error::Struct("'h' format requires -32768 <= number <= 32767".into())
				})?;
				data.extend_from_slice(&v.to_be_bytes());
			}
		}

		write_uint32var(flags, out)?;
		out.extend(data);
		Ok(())
	}

	fn to_xml(&self, xw: &mut XmlWriter, font: &mut Font, index: usize) -> Result<()> {
		begintag_indexed(xw, "VarComponent", index);
		let write =
			|xw: &mut XmlWriter, name: &str, value: String, extra: &[(&'static str, String)]| {
				let mut attrs = vec![("value", value)];
				attrs.extend_from_slice(extra);
				simpletag(xw, name, &attrs);
				xw.newline();
			};
		write(xw, "glyphName", font.glyph_name(self.glyph)?, &[]);
		if let Some(v) = self.condition_index {
			write(xw, "conditionIndex", v.to_string(), &[]);
		}
		if let Some(v) = self.axis_indices_index {
			write(xw, "axisIndicesIndex", v.to_string(), &[]);
		}
		let reset = self.flags & flags::RESET_UNSPECIFIED_AXES != 0;
		if self.axis_indices_index.is_some() || reset {
			let values: Vec<String> = self
				.axis_values
				.iter()
				.map(|&v| {
					let s = float_to_fixed_to_str(v, 14);
					float_repr(s.parse::<f64>().unwrap_or(v))
				})
				.collect();
			let extra = if reset {
				vec![("resetUnspecifiedAxes", "1".to_owned())]
			} else {
				Vec::new()
			};
			write(xw, "axisValues", format!("[{}]", values.join(", ")), &extra);
		}
		if self.axis_values_var_index != NO_VARIATION_INDEX {
			write(
				xw,
				"axisValuesVarIndex",
				self.axis_values_var_index.to_string(),
				&[],
			);
		}
		if self.transform_var_index != NO_VARIATION_INDEX {
			write(
				xw,
				"transformVarIndex",
				self.transform_var_index.to_string(),
				&[],
			);
		}
		let mut transform = self.transform;
		for m in &MAPPING {
			if self.flags & m.flag != 0 {
				let v = *(m.field)(&mut transform);
				write(xw, m.name, float_to_fixed_to_str(v, m.fractional_bits), &[]);
			}
		}
		xw.endtag("VarComponent");
		xw.newline();
		Ok(())
	}

	fn from_xml(element: &Element, font: &mut Font) -> Result<Self> {
		let mut c = VarComponent::default();
		for e in element.elements() {
			if e.elements().next().is_some() {
				return Err(Error::Assertion(format!("<{}> has content", e.name)));
			}
			let v = e
				.attr("value")
				.ok_or_else(|| Error::ttx("KeyError: 'value'"))?;
			let int = || -> Result<u64> {
				safe_eval(v)
					.and_then(|l| l.as_int())
					.and_then(|i| u64::try_from(i).ok())
					.ok_or_else(|| Error::value(format!("invalid integer {v:?}")))
			};
			match e.name.as_str() {
				"glyphName" => c.glyph = font.glyph_id(v)?,
				"conditionIndex" => c.condition_index = Some(int()?),
				"axisIndicesIndex" => c.axis_indices_index = Some(int()?),
				"axisValues" => {
					let items = match safe_eval(v) {
						Some(Literal::List(items) | Literal::Tuple(items)) => items,
						_ => return Err(Error::value(format!("malformed axisValues: {v:?}"))),
					};
					c.axis_values = items
						.iter()
						.map(|l| {
							l.as_float()
								.map(|f| float_to_fixed_to_float(f, 14))
								.ok_or_else(|| Error::value("expected a number"))
						})
						.collect::<Result<_>>()?;
					let reset = e
						.attr("resetUnspecifiedAxes")
						.map(|s| safe_eval(s).is_some_and(|l| l.is_truthy()))
						.unwrap_or(false);
					if reset {
						c.flags |= flags::RESET_UNSPECIFIED_AXES;
					}
				}
				"axisValuesVarIndex" => c.axis_values_var_index = int()?,
				"transformVarIndex" => c.transform_var_index = int()?,
				name => {
					let m = MAPPING
						.iter()
						.find(|m| m.name == name)
						.ok_or_else(|| Error::Assertion(name.to_owned()))?;
					*(m.field)(&mut c.transform) = safe_eval(v)
						.and_then(|l| l.as_float())
						.ok_or_else(|| Error::value(format!("invalid number {v:?}")))?;
					c.flags |= m.flag;
				}
			}
		}
		if c.flags & flags::HAVE_SCALE_Y == 0 {
			c.transform.scale_y = c.transform.scale_x;
		}
		Ok(c)
	}
}

/// `begintag(name, [("index", index)])`.
fn begintag_indexed(xw: &mut XmlWriter, name: &str, index: usize) {
	let index = index.to_string();
	let attrs: [(&str, &dyn AttrValue); 1] = [("index", &index)];
	xw.begintag_ordered(name, &attrs);
	xw.newline();
}

/// A variable composite glyph: a list of components (`VarCompositeGlyph`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VarCompositeGlyph {
	/// The components.
	pub components: Vec<VarComponent>,
}

impl VarCompositeGlyph {
	fn decompile(data: &[u8], num_glyphs: usize, axis_counts: Option<&[usize]>) -> Result<Self> {
		let mut components = Vec::new();
		let mut i = 0;
		while i < data.len() {
			let (c, next) = VarComponent::decompile(data, i, num_glyphs, axis_counts)?;
			components.push(c);
			i = next;
		}
		Ok(VarCompositeGlyph { components })
	}

	fn compile(&self) -> Result<Vec<u8>> {
		let mut out = Vec::new();
		for c in &self.components {
			c.compile(&mut out)?;
		}
		Ok(out)
	}

	fn to_xml(&self, xw: &mut XmlWriter, font: &mut Font, index: usize) -> Result<()> {
		begintag_indexed(xw, "VarCompositeGlyph", index);
		for (i, c) in self.components.iter().enumerate() {
			c.to_xml(xw, font, i)?;
		}
		xw.endtag("VarCompositeGlyph");
		xw.newline();
		Ok(())
	}

	/// Reads a `<VarCompositeGlyph>` element (`fromXML`).
	pub fn from_xml(element: &Element, font: &mut Font) -> Result<Self> {
		let mut components = Vec::new();
		for e in element.elements() {
			if e.name != "VarComponent" {
				return Err(Error::Assertion(format!("unexpected <{}>", e.name)));
			}
			components.push(VarComponent::from_xml(e, font)?);
		}
		Ok(VarCompositeGlyph { components })
	}
}

/// Reads a CFF2 INDEX (`CFF2Index.read`): the data of each item.
fn read_index<'a>(r: &mut OtReader<'a>) -> Result<Vec<&'a [u8]>> {
	let count = r.read_u32()? as usize;
	if count == 0 {
		return Ok(Vec::new());
	}
	let off_size = r.read_u8()? as usize;
	if !(1..=4).contains(&off_size) {
		return Err(Error::ttlib(format!(
			"invalid CFF2 INDEX offSize: {off_size}"
		)));
	}
	let data = r.data();
	if r.pos() + (count + 1) * off_size > data.len() {
		return Err(Error::ttlib(format!(
			"CFF2 INDEX count {count} exceeds available data"
		)));
	}
	let offsets: Vec<usize> = match off_size {
		1 => r
			.read_u8_array(count + 1)?
			.into_iter()
			.map(usize::from)
			.collect(),
		2 => r
			.read_u16_array(count + 1)?
			.into_iter()
			.map(usize::from)
			.collect(),
		3 => r
			.read_u24_array(count + 1)?
			.into_iter()
			.map(|v| v as usize)
			.collect(),
		_ => r
			.read_u32_array(count + 1)?
			.into_iter()
			.map(|v| v as usize)
			.collect(),
	};
	// Item data starts one byte before the first offset of 1.
	let base = r.pos() - 1;
	// UPSTREAM-BUG(B5): fontTools reads more than 8 items lazily (the default
	// `lazy=None` counts as lazy), which skips the ordering check; decreasing
	// offsets then give empty items instead of an error.
	let lazy = count > 8;
	let mut items = Vec::with_capacity(count);
	for w in offsets.windows(2) {
		if !lazy && w[0] > w[1] {
			return Err(Error::Assertion("lastOffset <= offset".into()));
		}
		let start = (base + w[0]).min(data.len());
		let end = (base + w[1]).clamp(start, data.len());
		items.push(&data[start..end]);
	}
	r.seek(base + offsets[count]);
	Ok(items)
}

/// Writes a CFF2 INDEX (`CFF2Index.write`).
///
/// LENIENT(L2): a list read from TTX without any item element is missing in
/// fontTools, whose `compile` then fails (`len(None)`); here it is empty and
/// written as an empty INDEX.
fn write_index(w: &mut OtWriter<'_>, items: &[Vec<u8>]) -> Result<()> {
	w.write_u32(crate::ot::table::fit(items.len() as u64, "count")?);
	if items.is_empty() {
		return Ok(());
	}
	let mut offsets = Vec::with_capacity(items.len() + 1);
	let mut last = 1u64;
	offsets.push(last);
	for item in items {
		last += item.len() as u64;
		offsets.push(last);
	}
	let off_size = if last < 0x100 {
		1
	} else if last < 0x1_0000 {
		2
	} else if last < 0x100_0000 {
		3
	} else {
		4
	};
	w.write_u8(off_size);
	for &o in &offsets {
		match off_size {
			1 => w.write_u8(o as u8),
			2 => w.write_u16(o as u16),
			3 => w.write_u24(o as u32)?,
			_ => w.write_u32(crate::ot::table::fit(o, "offset")?),
		}
	}
	for item in items {
		w.write_data(item);
	}
	Ok(())
}

/// `TupleList` (`CFF2Index` of `TupleValues`): reads the tuples.
pub fn read_tuple_list(r: &mut OtReader<'_>) -> Result<Vec<Vec<i64>>> {
	read_index(r)?
		.into_iter()
		.map(|item| Ok(decompile_deltas(None, item, 0)?.0))
		.collect()
}

/// `TupleListOffset.read`: an offset to a `TupleList`.
pub fn read_tuple_list_offset(r: &mut OtReader<'_>) -> Result<Vec<Vec<i64>>> {
	let offset = r.read_u32()? as usize;
	if offset == 0 {
		return Err(Error::Assertion("offset != 0".into()));
	}
	read_tuple_list(&mut r.sub_reader(offset))
}

/// `TupleList` (`CFF2Index` of `TupleValues`): writes the tuples.
pub fn write_tuple_list(w: &mut OtWriter<'_>, tuples: &[Vec<i64>]) -> Result<()> {
	let items = tuples
		.iter()
		.map(|t| compile_delta_values(t, true))
		.collect::<Result<Vec<_>>>()?;
	write_index(w, &items)
}

/// `TupleListOffset.write`: the tuples in a subtable named `name`.
pub fn write_tuple_list_offset(
	w: &mut OtWriter<'_>,
	tuples: &[Vec<i64>],
	name: &'static str,
) -> Result<()> {
	let mut sub = w.sub_table(name, None, 4);
	write_tuple_list(&mut sub, tuples)
}

/// `TupleValues.xmlWrite` for each tuple: `<name index="i" value="[...]"/>`.
pub fn xml_tuple_list(
	xw: &mut XmlWriter,
	name: &str,
	attrs: &[(&'static str, String)],
	tuples: &[Vec<i64>],
) {
	for (i, t) in tuples.iter().enumerate() {
		let mut attrs = attrs.to_vec();
		attrs.push(("index", i.to_string()));
		xml_simple(xw, name, &attrs, int_list_repr(t));
	}
}

/// `TupleValues.xmlRead`.
pub fn parse_tuple(e: &Element) -> Result<Vec<i64>> {
	parse_int_list(super::table::value_attr(e)?)
}

/// The length of each `AxisIndicesList` item, as propagated to the composite
/// glyphs.
pub fn axis_indices_state(items: &[Vec<i64>]) -> StateValue {
	StateValue::Lens(Rc::new(items.iter().map(Vec::len).collect()))
}

/// `VarCompositeGlyphList` (`CFF2Index` of `VarCompositeGlyph`): reads the glyphs.
pub fn read_var_composite_glyphs(
	r: &mut OtReader<'_>,
	font: &mut Font,
) -> Result<Vec<VarCompositeGlyph>> {
	let axis_counts = match r.get("AxisIndicesList") {
		Some(StateValue::Lens(l)) => Some(l.clone()),
		_ => None,
	};
	let items = read_index(r)?;
	let num_glyphs = font.glyph_order()?.len();
	items
		.into_iter()
		.map(|item| {
			VarCompositeGlyph::decompile(
				item,
				num_glyphs,
				axis_counts.as_deref().map(Vec::as_slice),
			)
		})
		.collect()
}

/// `VarCompositeGlyphList`: writes the glyphs.
pub fn write_var_composite_glyphs(
	w: &mut OtWriter<'_>,
	glyphs: &[VarCompositeGlyph],
) -> Result<()> {
	let items = glyphs
		.iter()
		.map(VarCompositeGlyph::compile)
		.collect::<Result<Vec<_>>>()?;
	write_index(w, &items)
}

/// `VarCompositeGlyphList.xmlWrite`.
pub fn xml_var_composite_glyphs(
	xw: &mut XmlWriter,
	font: &mut Font,
	glyphs: &[VarCompositeGlyph],
) -> Result<()> {
	for (i, g) in glyphs.iter().enumerate() {
		g.to_xml(xw, font, i)?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn uint32var_round_trips() -> Result<()> {
		for v in [
			0,
			0x7F,
			0x80,
			0x3FFF,
			0x4000,
			0x1F_FFFF,
			0x20_0000,
			0xFFF_FFFF,
			0x1000_0000,
			0xFFFF_FFFF,
		] {
			let mut data = Vec::new();
			write_uint32var(v, &mut data)?;
			assert_eq!(read_uint32var(&data, 0)?, (v, data.len()), "{v:#x}");
		}
		assert!(write_uint32var(1 << 32, &mut Vec::new()).is_err());
		Ok(())
	}

	#[test]
	fn flag_inversion_is_32_bit() {
		assert_eq!(clear((1 << 35) | 0x1FFF, flags::GID_IS_24BIT), 0x0FFF);
	}
}
