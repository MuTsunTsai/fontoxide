//! Value conversions of `fontTools.ttLib.tables.otConverters` that the generated
//! code calls, and `ValueRecord` (`otBase`).

use fontoxide_misc::Tag;
use fontoxide_misc::fixed_tools::{
	fixed_to_float, float_to_fixed, float_to_fixed_to_str, str_to_fixed_to_float,
};
use fontoxide_misc::round_tools::{nearest_multiple_shortest_repr, ot_round};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use super::table::{Attrs, OtTable, XmlCtx, simpletag, xml_simple};
use super::{OtReader, OtWriter, StateValue};
use crate::{Error, Font, Result};

/// A field whose converter fontoxide does not implement yet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Unsupported;

impl Unsupported {
	fn error(converter: &str) -> Error {
		Error::ttlib(format!("the {converter} converter is not implemented yet"))
	}

	/// Fails: reading is not implemented.
	pub fn read(converter: &str) -> Result<Self> {
		Err(Self::error(converter))
	}

	/// Fails: writing is not implemented.
	pub fn write(converter: &str) -> Result<()> {
		Err(Self::error(converter))
	}

	/// Fails: reading TTX is not implemented.
	pub fn xml_read(converter: &str) -> Result<Self> {
		Err(Self::error(converter))
	}
}

const ANGLE_FACTOR: f64 = 1.0 / (1 << 14) as f64 * 180.0;

fn angle_bias(kind: &str) -> f64 {
	if kind == "BiasedAngle" { 1.0 } else { 0.0 }
}

/// Reads a `Fixed`, `F2Dot14`, `Angle`, `BiasedAngle` or `DeciPoints` value.
pub fn read_float(r: &mut OtReader<'_>, kind: &str) -> Result<f64> {
	Ok(match kind {
		"Fixed" => fixed_to_float(r.read_i32()? as i64, 16),
		"F2Dot14" => fixed_to_float(r.read_i16()? as i64, 14),
		"Angle" | "BiasedAngle" => {
			(fixed_to_float(r.read_i16()? as i64, 14) + angle_bias(kind)) * 180.0
		}
		"DeciPoints" => r.read_u16()? as f64 / 10.0,
		k => return Err(Error::value(format!("unknown float kind {k}"))),
	})
}

/// Writes a `Fixed`, `F2Dot14`, `Angle`, `BiasedAngle` or `DeciPoints` value.
pub fn write_float(w: &mut OtWriter<'_>, value: f64, kind: &str) -> Result<()> {
	let fit16 =
		|v: i64| i16::try_from(v).map_err(|_| Error::Assertion(format!("-32768 <= {v} < 32768")));
	match kind {
		"Fixed" => {
			let v = float_to_fixed(value, 16);
			w.write_i32(i32::try_from(v).map_err(|_| {
				Error::Struct(format!(
					"'i' format requires -2147483648 <= number <= 2147483647: {v}"
				))
			})?);
		}
		"F2Dot14" => w.write_i16(fit16(float_to_fixed(value, 14))?),
		"Angle" | "BiasedAngle" => {
			w.write_i16(fit16(float_to_fixed(value / 180.0 - angle_bias(kind), 14))?)
		}
		"DeciPoints" => {
			let v = fontoxide_misc::py::round(value * 10.0) as i64;
			w.write_u16(
				u16::try_from(v).map_err(|_| Error::Assertion(format!("0 <= {v} < 0x10000")))?,
			);
		}
		k => return Err(Error::value(format!("unknown float kind {k}"))),
	}
	Ok(())
}

/// `toString` of a float converter.
pub fn float_to_string(value: f64, kind: &str) -> String {
	match kind {
		"Fixed" => float_to_fixed_to_str(value, 16),
		"F2Dot14" => float_to_fixed_to_str(value, 14),
		"Angle" | "BiasedAngle" => nearest_multiple_shortest_repr(value, ANGLE_FACTOR),
		_ => fontoxide_misc::py::float_repr(value),
	}
}

/// `fromString` of a float converter.
pub fn float_from_string(s: &str, kind: &str) -> Result<f64> {
	let bad = || Error::value(format!("could not convert string to float: {s:?}"));
	Ok(match kind {
		"Fixed" => str_to_fixed_to_float(s, 16).ok_or_else(bad)?,
		"F2Dot14" => str_to_fixed_to_float(s, 14).ok_or_else(bad)?,
		"Angle" | "BiasedAngle" => {
			let v = fontoxide_misc::py::parse_float(s).ok_or_else(bad)?;
			ot_round(v / ANGLE_FACTOR) as f64 * ANGLE_FACTOR
		}
		_ => fontoxide_misc::py::parse_float(s).ok_or_else(bad)?,
	})
}

/// `Int24.write`.
pub fn write_i24(w: &mut OtWriter<'_>, value: i32) -> Result<()> {
	if !(-(1 << 23)..(1 << 23)).contains(&value) {
		return Err(Error::Assertion(format!("{value}")));
	}
	w.write_u24((value as u32) & 0xFF_FFFF)
}

/// Writes a glyph ID of the given size.
pub fn write_glyph(w: &mut OtWriter<'_>, gid: u32, size: usize) -> Result<()> {
	match size {
		2 => w.write_u16(u16::try_from(gid).map_err(|_| {
			Error::Struct(format!("'H' format requires 0 <= number <= 65535: {gid}"))
		})?),
		3 => w.write_u24(gid)?,
		_ => w.write_u32(gid),
	}
	Ok(())
}

/// `Char64.read`: an ASCII string of up to 64 bytes, NUL-terminated.
pub fn read_char64(r: &mut OtReader<'_>) -> String {
	let data = r.read_data(64);
	let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
	let data = &data[..end];
	if !data.is_ascii() {
		log::warn!(
			"replaced non-ASCII characters in \"{}\"",
			String::from_utf8_lossy(data)
		);
	}
	data.iter()
		.map(|&b| if b.is_ascii() { b as char } else { '\u{FFFD}' })
		.collect()
}

/// `Char64.write`.
pub fn write_char64(w: &mut OtWriter<'_>, value: &str) {
	let mut data: Vec<u8> = value
		.chars()
		.map(|c| if c.is_ascii() { c as u8 } else { b'?' })
		.collect();
	if !value.is_ascii() {
		log::warn!("replacing non-ASCII characters in \"{value}\"");
	}
	if data.len() > 64 {
		log::warn!("truncating overlong \"{value}\" to 64 bytes");
	}
	data.resize(64, 0);
	w.write_data(&data);
}

/// `fi2ve`: a version below 0x10000 is taken as a float.
pub fn version_as_long(value: u32) -> Result<u32> {
	let v = fontoxide_misc::fixed_tools::ensure_version_is_long(value as f64);
	u32::try_from(v).map_err(|_| Error::Struct(format!("version {v} out of range")))
}

/// `Version.fromString` (`versionToFixed`).
pub fn parse_version(s: &str) -> Result<u32> {
	let v = fontoxide_misc::fixed_tools::version_to_fixed(s)
		.ok_or_else(|| Error::value(format!("invalid version: {s:?}")))?;
	u32::try_from(v).map_err(|_| Error::Struct(format!("version {v} out of range")))
}

/// `NO_VARIATION_INDEX`.
pub const NO_VARIATION_INDEX: u32 = 0xFFFF_FFFF;

/// `VarIndex.xmlWrite` (an `OptionalValue`): the value is omitted if it is the
/// default.
pub fn xml_var_index(xw: &mut XmlWriter, name: &str, attrs: &[(&'static str, String)], value: u32) {
	let mut attrs = attrs.to_vec();
	if value != NO_VARIATION_INDEX {
		attrs.push(("value", value.to_string()));
	}
	simpletag(xw, name, &attrs);
	xw.newline();
}

/// `VarIndex.xmlRead`.
pub fn parse_var_index(e: &Element) -> Result<u32> {
	match e.attr("value") {
		Some(v) => super::table::parse_int(v),
		None => Ok(NO_VARIATION_INDEX),
	}
}

/// `NameID.xmlWrite`: the value and the name it refers to.
pub fn xml_name_id(
	xw: &mut XmlWriter,
	font: &mut Font,
	name: &str,
	attrs: &[(&'static str, String)],
	value: u16,
) -> Result<()> {
	let mut attrs = attrs.to_vec();
	attrs.push(("value", value.to_string()));
	simpletag(xw, name, &attrs);
	if value != 0
		&& let Some(names) = font.get::<crate::tables::Name>()?
	{
		let debug = names.get_debug_name(value);
		xw.write("  ");
		match debug {
			Some(n) => xw.comment(&n),
			None if value == 0xFFFF => xw.comment("None"),
			None => {
				xw.comment("missing from name table");
				log::warn!("name id {value} missing from name table");
			}
		}
	}
	xw.newline();
	Ok(())
}

/// `STATFlags.xmlWrite`.
pub fn xml_stat_flags(
	xw: &mut XmlWriter,
	name: &str,
	attrs: &[(&'static str, String)],
	value: u16,
) {
	let mut attrs = attrs.to_vec();
	attrs.push(("value", value.to_string()));
	simpletag(xw, name, &attrs);
	let mut flags = Vec::new();
	if value & 0x01 != 0 {
		flags.push("OlderSiblingFontAttribute");
	}
	if value & 0x02 != 0 {
		flags.push("ElidableAxisValueName");
	}
	if !flags.is_empty() {
		xw.write("  ");
		xw.comment(&flags.join(" "));
	}
	xw.newline();
}

/// `LookupFlag.xmlWrite`.
pub fn xml_lookup_flag(
	xw: &mut XmlWriter,
	name: &str,
	attrs: &[(&'static str, String)],
	value: u16,
) {
	let mut attrs = attrs.to_vec();
	attrs.push(("value", value.to_string()));
	simpletag(xw, name, &attrs);
	let mut flags = Vec::new();
	for (bit, flag) in [
		(0x01, "rightToLeft"),
		(0x02, "ignoreBaseGlyphs"),
		(0x04, "ignoreLigatures"),
		(0x08, "ignoreMarks"),
		(0x10, "useMarkFilteringSet"),
	] {
		if value & bit != 0 {
			flags.push(flag.to_owned());
		}
	}
	if value & 0xFF00 != 0 {
		flags.push(format!("markAttachmentType[{}]", value >> 8));
	}
	if !flags.is_empty() {
		xw.comment(&flags.join(" "));
	}
	xw.newline();
}

const EXTEND_MODE: &[&str] = &["PAD", "REPEAT", "REFLECT"];
const COMPOSITE_MODE: &[&str] = &[
	"CLEAR",
	"SRC",
	"DEST",
	"SRC_OVER",
	"DEST_OVER",
	"SRC_IN",
	"DEST_IN",
	"SRC_OUT",
	"DEST_OUT",
	"SRC_ATOP",
	"DEST_ATOP",
	"XOR",
	"PLUS",
	"SCREEN",
	"OVERLAY",
	"DARKEN",
	"LIGHTEN",
	"COLOR_DODGE",
	"COLOR_BURN",
	"HARD_LIGHT",
	"SOFT_LIGHT",
	"DIFFERENCE",
	"EXCLUSION",
	"MULTIPLY",
	"HSL_HUE",
	"HSL_SATURATION",
	"HSL_COLOR",
	"HSL_LUMINOSITY",
];

fn enum_names(name: &str) -> &'static [&'static str] {
	if name == "ExtendMode" {
		EXTEND_MODE
	} else {
		COMPOSITE_MODE
	}
}

/// `_UInt8Enum.toString`: the lowercase member name.
pub fn enum_to_string(name: &str, value: u8) -> Result<String> {
	enum_names(name)
		.get(value as usize)
		.map(|s| s.to_lowercase())
		.ok_or_else(|| Error::value(format!("{value} is not a valid {name}")))
}

/// `_UInt8Enum.fromString`.
pub fn enum_from_string(name: &str, s: &str) -> Result<u8> {
	let upper = s.to_uppercase();
	enum_names(name)
		.iter()
		.position(|n| *n == upper)
		.map(|i| i as u8)
		.ok_or_else(|| Error::value(format!("{name} has no member {s:?}")))
}

/// `Tag.fromString`: packed as four Latin-1 bytes.
pub fn parse_tag(s: &str) -> Result<Tag> {
	Tag::from_str_exact(s)
		.ok_or_else(|| Error::Assertion(format!("tag should be 4 characters long: {s:?}")))
}

/// `safeEval` of a list of integers (`DeltaValue.xmlRead`).
pub fn parse_int_list(s: &str) -> Result<Vec<i64>> {
	use fontoxide_misc::text_tools::Literal;
	match fontoxide_misc::text_tools::safe_eval(s) {
		Some(Literal::List(items) | Literal::Tuple(items)) => items
			.iter()
			.map(|i| {
				i.as_int()
					.map(|v| v as i64)
					.ok_or_else(|| Error::value("expected an integer"))
			})
			.collect(),
		_ => Err(Error::value(format!("malformed DeltaValue: {s:?}"))),
	}
}

/// `DeltaValue.read`.
pub fn read_delta_value(
	r: &mut OtReader<'_>,
	start_size: u16,
	end_size: u16,
	delta_format: u16,
) -> Result<Vec<i64>> {
	if !(1..=3).contains(&delta_format) {
		return Err(Error::Assertion("illegal DeltaFormat".into()));
	}
	let n_items = end_size as i64 - start_size as i64 + 1;
	let n_bits = 1u32 << delta_format;
	let minus_offset = 1i64 << n_bits;
	let mask = (1u32 << n_bits) - 1;
	let sign_mask = 1u32 << (n_bits - 1);
	let mut values = Vec::new();
	let (mut tmp, mut shift) = (0u32, 0u32);
	for _ in 0..n_items.max(0) {
		if shift == 0 {
			tmp = r.read_u16()? as u32;
			shift = 16;
		}
		shift -= n_bits;
		let mut value = ((tmp >> shift) & mask) as i64;
		if value as u32 & sign_mask != 0 {
			value -= minus_offset;
		}
		values.push(value);
	}
	Ok(values)
}

/// `DeltaValue.write`.
pub fn write_delta_value(
	w: &mut OtWriter<'_>,
	values: &[i64],
	start_size: u16,
	end_size: u16,
	delta_format: u16,
) -> Result<()> {
	if !(1..=3).contains(&delta_format) {
		return Err(Error::Assertion("illegal DeltaFormat".into()));
	}
	let n_items = end_size as i64 - start_size as i64 + 1;
	let n_bits = 1u32 << delta_format;
	if values.len() as i64 != n_items {
		return Err(Error::Assertion("len(DeltaValue) == nItems".into()));
	}
	let mask = (1i64 << n_bits) - 1;
	let (mut tmp, mut shift) = (0i64, 16u32);
	for &value in values {
		shift -= n_bits;
		tmp |= (value & mask) << shift;
		if shift == 0 {
			w.write_u16(tmp as u16);
			tmp = 0;
			shift = 16;
		}
	}
	if shift != 16 {
		w.write_u16(tmp as u16);
	}
	Ok(())
}

/// `FeatureVariations.read`: makes the feature tags available to alternate
/// features (`reader["FeatureTags"]`).
pub fn set_feature_tags(
	r: &mut OtReader<'_>,
	font: &mut Font,
	feature_list: Option<&crate::generated::ot::FeatureList>,
	version: u32,
) -> Result<()> {
	let mut peek = r.clone();
	if peek.read_u32()? == 0 {
		return Ok(());
	}
	let mut list2 = None;
	if version >= 0x0001_0002 {
		let mut rr = r.clone();
		rr.advance(8);
		list2 = super::table::read_offset::<crate::generated::ot::FeatureList>(&mut rr, font, 4)?;
	}
	let list = list2.as_deref().or(feature_list);
	let tags: Vec<Tag> = list
		.map(|l| l.feature_record.iter().map(|f| f.feature_tag).collect())
		.unwrap_or_default();
	r.set("FeatureTags", StateValue::Tags(std::rc::Rc::new(tags)));
	Ok(())
}

/// `FeatureVariations.write`.
pub fn set_writer_feature_tags(
	w: &mut OtWriter<'_>,
	feature_list: Option<&crate::generated::ot::FeatureList>,
	_version: u32,
) {
	let tags: Vec<Tag> = feature_list
		.map(|l| l.feature_record.iter().map(|f| f.feature_tag).collect())
		.unwrap_or_default();
	w.set("FeatureTags", StateValue::Tags(std::rc::Rc::new(tags)));
}

/// `AlternateFeature.read`: the tag of the feature that a substitution record
/// replaces (`reader["FeatureTag"]`).
pub fn set_alternate_feature_tag(r: &mut OtReader<'_>, feature_index: u16) {
	let Some(StateValue::Tags(tags)) = r.get("FeatureTags").cloned() else {
		return;
	};
	let mut peek = r.clone();
	if peek.read_u32().map(|o| o != 0).unwrap_or(false) {
		let tag = tags.get(feature_index as usize).copied();
		r.set("FeatureTag", StateValue::Tag(tag));
	}
}

/// `AlternateFeature.write`.
pub fn set_writer_alternate_feature_tag(
	w: &mut OtWriter<'_>,
	has_params: Option<bool>,
	feature_index: u16,
) -> Result<()> {
	if has_params != Some(true) {
		return Ok(());
	}
	if let Some(StateValue::Tags(tags)) = w.get("FeatureTags").cloned() {
		let tag = *tags
			.get(feature_index as usize)
			.ok_or_else(|| Error::ttlib("tuple index out of range"))?;
		w.set("FeatureTag", StateValue::Tag(Some(tag)));
	}
	Ok(())
}

/// The fields of a value record, in format bit order (`valueRecordFormat`):
/// (mask, name, is device, signed).
const VALUE_RECORD_FORMAT: [(u16, &str, bool, bool); 16] = [
	(0x0001, "XPlacement", false, true),
	(0x0002, "YPlacement", false, true),
	(0x0004, "XAdvance", false, true),
	(0x0008, "YAdvance", false, true),
	(0x0010, "XPlaDevice", true, false),
	(0x0020, "YPlaDevice", true, false),
	(0x0040, "XAdvDevice", true, false),
	(0x0080, "YAdvDevice", true, false),
	(0x0100, "Reserved1", false, false),
	(0x0200, "Reserved2", false, false),
	(0x0400, "Reserved3", false, false),
	(0x0800, "Reserved4", false, false),
	(0x1000, "Reserved5", false, false),
	(0x2000, "Reserved6", false, false),
	(0x4000, "Reserved7", false, false),
	(0x8000, "Reserved8", false, false),
];

/// A positioning value record (`ValueRecord`). Like the fontTools object, each
/// field is present or not; `None` means absent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ValueRecord {
	/// Horizontal placement.
	pub x_placement: Option<i16>,
	/// Vertical placement.
	pub y_placement: Option<i16>,
	/// Horizontal advance.
	pub x_advance: Option<i16>,
	/// Vertical advance.
	pub y_advance: Option<i16>,
	/// Horizontal placement device table; `Some(None)` for a null offset.
	pub x_pla_device: Option<Option<Box<crate::generated::ot::Device>>>,
	/// Vertical placement device table.
	pub y_pla_device: Option<Option<Box<crate::generated::ot::Device>>>,
	/// Horizontal advance device table.
	pub x_adv_device: Option<Option<Box<crate::generated::ot::Device>>>,
	/// Vertical advance device table.
	pub y_adv_device: Option<Option<Box<crate::generated::ot::Device>>>,
	/// The reserved fields, by bit (8–15), if present.
	pub reserved: [Option<u16>; 8],
}

impl ValueRecord {
	fn int_field(&mut self, i: usize) -> Option<&mut Option<i16>> {
		match i {
			0 => Some(&mut self.x_placement),
			1 => Some(&mut self.y_placement),
			2 => Some(&mut self.x_advance),
			3 => Some(&mut self.y_advance),
			_ => None,
		}
	}

	fn device_field(
		&mut self,
		i: usize,
	) -> Option<&mut Option<Option<Box<crate::generated::ot::Device>>>> {
		match i {
			4 => Some(&mut self.x_pla_device),
			5 => Some(&mut self.y_pla_device),
			6 => Some(&mut self.x_adv_device),
			7 => Some(&mut self.y_adv_device),
			_ => None,
		}
	}

	/// The format of the fields present (`getFormat`).
	pub fn format(&self) -> u16 {
		let mut c = self.clone();
		let mut f = 0;
		for (i, (mask, ..)) in VALUE_RECORD_FORMAT.iter().enumerate() {
			let present = match i {
				0..4 => c.int_field(i).is_some_and(|v| v.is_some()),
				4..8 => c.device_field(i).is_some_and(|v| v.is_some()),
				_ => c.reserved[i - 8].is_some(),
			};
			if present {
				f |= mask;
			}
		}
		f
	}

	/// `ValueRecordFactory.readValueRecord`; `None` for an empty format.
	pub fn read(r: &mut OtReader<'_>, font: &mut Font, which: &str) -> Result<Option<Self>> {
		let format = match r.get(which) {
			Some(StateValue::ValueFormat(f)) => *f,
			_ => return Err(Error::ttlib(format!("KeyError: {which:?}"))),
		};
		if format == 0 {
			return Ok(None);
		}
		let mut vr = ValueRecord::default();
		for (i, &(mask, _, is_device, signed)) in VALUE_RECORD_FORMAT.iter().enumerate() {
			if format & mask == 0 {
				continue;
			}
			let value = if signed {
				r.read_i16()? as i32
			} else {
				r.read_u16()? as i32
			};
			if is_device {
				let device = if value != 0 {
					let mut sub = r.sub_reader(value as usize);
					Some(Box::new(crate::generated::ot::Device::decompile(
						&mut sub, font,
					)?))
				} else {
					None
				};
				if let Some(f) = vr.device_field(i) {
					*f = Some(device);
				}
			} else if i < 4 {
				if let Some(f) = vr.int_field(i) {
					*f = Some(value as i16);
				}
			} else {
				vr.reserved[i - 8] = Some(value as u16);
			}
		}
		Ok(Some(vr))
	}

	/// `ValueRecordFactory.writeValueRecord`; absent fields are written as 0.
	pub fn write(
		w: &mut OtWriter<'_>,
		font: &mut Font,
		which: &str,
		value: Option<&Self>,
	) -> Result<()> {
		let format = match w.get(which) {
			Some(StateValue::ValueFormat(f)) => *f,
			_ => return Err(Error::ttlib(format!("KeyError: {which:?}"))),
		};
		let mut vr = value.cloned().unwrap_or_default();
		for (i, &(mask, _, is_device, signed)) in VALUE_RECORD_FORMAT.iter().enumerate() {
			if format & mask == 0 {
				continue;
			}
			if is_device {
				match vr.device_field(i).and_then(|d| d.take()).flatten() {
					Some(device) => {
						let mut sub = w.sub_table("<none>", None, 2);
						device.compile(&mut sub, font)?;
					}
					None => w.write_u16(0),
				}
			} else if i < 4 {
				let v = vr.int_field(i).and_then(|f| *f).unwrap_or(0);
				if signed {
					w.write_i16(v);
				} else {
					w.write_u16(v as u16);
				}
			} else {
				w.write_u16(vr.reserved[i - 8].unwrap_or(0));
			}
		}
		Ok(())
	}

	/// `ValueRecord.toXML`.
	pub fn to_xml(
		&self,
		xw: &mut XmlWriter,
		font: &mut Font,
		name: &str,
		attrs: &[(&'static str, String)],
	) -> Result<()> {
		let mut simple: Attrs = attrs.to_vec();
		let mut c = self.clone();
		for (i, &(_, field, ..)) in VALUE_RECORD_FORMAT[..4].iter().enumerate() {
			if let Some(Some(v)) = c.int_field(i).map(|v| *v) {
				simple.push((field, v.to_string()));
			}
		}
		let mut devices = Vec::new();
		for (i, &(_, field, ..)) in VALUE_RECORD_FORMAT.iter().enumerate().take(8).skip(4) {
			if let Some(Some(Some(d))) = c.device_field(i).map(|d| d.clone()) {
				devices.push((field, d));
			}
		}
		if devices.is_empty() {
			simpletag(xw, name, &simple);
			xw.newline();
		} else {
			let refs: Vec<(&str, &dyn fontoxide_misc::xml_writer::AttrValue)> = simple
				.iter()
				.map(|(n, v)| (*n, v as &dyn fontoxide_misc::xml_writer::AttrValue))
				.collect();
			xw.begintag_ordered(name, &refs);
			xw.newline();
			for (field, device) in devices {
				device.to_xml(xw, font, &[], field)?;
			}
			xw.endtag(name);
			xw.newline();
		}
		Ok(())
	}

	/// `ValueRecord.fromXML`.
	pub fn from_xml(e: &Element, font: &mut Font) -> Result<Self> {
		let mut vr = ValueRecord::default();
		for (k, v) in &e.attrs {
			let value: i64 = fontoxide_misc::py::parse_int(v)
				.ok_or_else(|| Error::value(format!("invalid literal for int(): {v:?}")))?
				as i64;
			match VALUE_RECORD_FORMAT.iter().position(|(_, n, ..)| n == k) {
				Some(i) if i < 4 => {
					if let Some(f) = vr.int_field(i) {
						*f = Some(value as i16);
					}
				}
				Some(i) if i >= 8 => vr.reserved[i - 8] = Some(value as u16),
				// Attributes like "index" are kept by fontTools but never written.
				_ => {}
			}
		}
		for child in e.elements() {
			let Some(i) = VALUE_RECORD_FORMAT
				.iter()
				.position(|(_, n, ..)| *n == child.name)
			else {
				return Err(Error::ttx(format!("AttributeError: {}", child.name)));
			};
			let mut device = crate::generated::ot::Device::default();
			let mut ctx = XmlCtx::default();
			for e2 in child.elements() {
				device.from_xml(e2, font, &mut ctx)?;
			}
			if let Some(f) = vr.device_field(i) {
				*f = Some(Some(Box::new(device)));
			}
		}
		Ok(vr)
	}
}

/// `xml_simple` re-exported for the generated code's convenience.
pub fn xml_value(xw: &mut XmlWriter, name: &str, attrs: &[(&'static str, String)], value: String) {
	xml_simple(xw, name, attrs, value);
}

fn fit_array<T: TryFrom<i64>>(values: &[i64], code: char) -> Result<Vec<T>> {
	values
		.iter()
		.map(|&v| {
			T::try_from(v)
				.map_err(|_| Error::Struct(format!("array('{code}') item out of range: {v}")))
		})
		.collect()
}

/// `VarDataValue.read`: one row of deltas of a `VarData` table.
pub fn read_var_data_value(
	r: &mut OtReader<'_>,
	region_count: Option<u32>,
	num_shorts: u16,
) -> Result<Vec<i64>> {
	let region_count =
		region_count.ok_or_else(|| Error::ttlib("KeyError: 'VarRegionCount'"))? as usize;
	let long_words = num_shorts & 0x8000 != 0;
	let word_count = (num_shorts & 0x7FFF) as usize;
	let (n1, n2) = (region_count.min(word_count), region_count.max(word_count));
	let mut values: Vec<i64> = Vec::with_capacity(n2);
	if long_words {
		values.extend(r.read_i32_array(n1)?.into_iter().map(i64::from));
		values.extend(r.read_i16_array(n2 - n1)?.into_iter().map(i64::from));
	} else {
		values.extend(r.read_i16_array(n1)?.into_iter().map(i64::from));
		values.extend(r.read_i8_array(n2 - n1)?.into_iter().map(i64::from));
	}
	values.truncate(region_count);
	Ok(values)
}

/// `VarDataValue.write`.
pub fn write_var_data_value(
	w: &mut OtWriter<'_>,
	values: &[i64],
	region_count: Option<u64>,
	num_shorts: u16,
) -> Result<()> {
	let region_count =
		region_count.ok_or_else(|| Error::ttlib("TypeError: VarRegionCount is None"))? as usize;
	let long_words = num_shorts & 0x8000 != 0;
	let word_count = (num_shorts & 0x7FFF) as usize;
	let (n1, n2) = (region_count.min(word_count), region_count.max(word_count));
	// Python slicing: `values[:n1]` and `values[n1:regionCount]`.
	let mid = n1.min(values.len());
	let end = region_count.min(values.len()).max(mid);
	let (big, small) = (&values[..mid], &values[mid..end]);
	if long_words {
		for v in fit_array::<i32>(big, 'i')? {
			w.write_i32(v);
		}
		for v in fit_array::<i16>(small, 'h')? {
			w.write_i16(v);
		}
	} else {
		for v in fit_array::<i16>(big, 'h')? {
			w.write_i16(v);
		}
		for v in fit_array::<i8>(small, 'b')? {
			w.write_i8(v);
		}
	}
	if n2 > region_count {
		// fontTools calls the nonexistent `writer.writeSmallArray` here.
		return Err(Error::ttlib(
			"AttributeError: 'OTTableWriter' object has no attribute 'writeSmallArray'",
		));
	}
	Ok(())
}

/// `VarRegionList.preWrite`: `RegionAxisCount` is the number of `fvar` axes if
/// there is an `fvar` table, whatever the regions say.
pub fn region_axis_count(font: &mut Font, current: Option<u32>) -> Result<Option<u64>> {
	if let Some(fvar) = font.get::<crate::tables::Fvar>()? {
		return Ok(Some(fvar.axes.len() as u64));
	}
	Ok(current.map(u64::from))
}
