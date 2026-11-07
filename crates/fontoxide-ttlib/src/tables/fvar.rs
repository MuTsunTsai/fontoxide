//! `fvar`: Font Variations table (`fontTools.ttLib.tables._f_v_a_r`).

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::fixed_tools::{float_to_fixed, float_to_fixed_to_str, str_to_fixed_to_float};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes, fit};
use crate::sstruct::{Bytes, F16Dot16, U16, U32, eval};
use crate::table::{Table, TableType};
use crate::tables::Name;
use crate::{Error, Font, Result};

crate::sstruct! {
	/// The `fvar` header (`FVAR_HEADER_FORMAT`).
	pub struct FvarHeader = "FVAR_HEADER_FORMAT" {
		/// 0x00010000.
		pub version: u32 = "version" as U32,
		/// Offset to the axis records.
		pub offset_to_data: u16 = "offsetToData" as U16,
		/// 2.
		pub count_size_pairs: u16 = "countSizePairs" as U16,
		/// Number of axes.
		pub axis_count: u16 = "axisCount" as U16,
		/// Size of an axis record.
		pub axis_size: u16 = "axisSize" as U16,
		/// Number of named instances.
		pub instance_count: u16 = "instanceCount" as U16,
		/// Size of an instance record.
		pub instance_size: u16 = "instanceSize" as U16,
	}
}

crate::sstruct! {
	/// A variation axis (`Axis`, `FVAR_AXIS_FORMAT`).
	pub struct Axis = "FVAR_AXIS_FORMAT" {
		/// Axis tag.
		pub axis_tag: [u8; 4] = "axisTag" as Bytes<4>,
		/// Minimum value.
		pub min_value: f64 = "minValue" as F16Dot16,
		/// Default value.
		pub default_value: f64 = "defaultValue" as F16Dot16,
		/// Maximum value.
		pub max_value: f64 = "maxValue" as F16Dot16,
		/// Flags; bit 0 hides the axis.
		pub flags: u16 = "flags" as U16,
		/// Name ID of the axis name.
		pub axis_name_id: u16 = "axisNameID" as U16,
	}
}

crate::sstruct! {
	/// The fixed part of a named instance (`FVAR_INSTANCE_FORMAT`).
	pub struct InstanceHeader = "FVAR_INSTANCE_FORMAT" {
		/// Name ID of the subfamily name.
		pub subfamily_name_id: u16 = "subfamilyNameID" as U16,
		/// Flags.
		pub flags: u16 = "flags" as U16,
	}
}

impl Axis {
	/// An axis with fontTools' defaults (`Axis()`): range −1…1, default 0.
	pub fn new(tag: Tag) -> Self {
		Axis {
			axis_tag: tag.0,
			min_value: -1.0,
			default_value: 0.0,
			max_value: 1.0,
			flags: 0,
			axis_name_id: 0,
		}
	}

	/// The axis tag.
	pub fn tag(&self) -> Tag {
		Tag(self.axis_tag)
	}

	/// The tag as fontTools holds it after unpacking: a `str` if ASCII.
	fn tag_text(&self) -> String {
		crate::sstruct::bytes_to_py_str(&self.axis_tag)
	}

	fn to_xml(&self, writer: &mut XmlWriter, names: Option<&Name>) {
		if let Some(name) = names.and_then(|n| n.get_debug_name(self.axis_name_id)) {
			writer.newline();
			writer.comment(&name);
			writer.newline();
		}
		writer.begintag("Axis", &[]);
		writer.newline();
		for (tag, value) in [
			("AxisTag", self.tag_text()),
			("Flags", format!("0x{:X}", self.flags)),
			("MinValue", float_to_fixed_to_str(self.min_value, 16)),
			(
				"DefaultValue",
				float_to_fixed_to_str(self.default_value, 16),
			),
			("MaxValue", float_to_fixed_to_str(self.max_value, 16)),
			("AxisNameID", self.axis_name_id.to_string()),
		] {
			writer.begintag(tag, &[]);
			writer.write(&value);
			writer.endtag(tag);
			writer.newline();
		}
		writer.endtag("Axis");
		writer.newline();
	}

	fn from_xml(element: &Element) -> Result<Self> {
		let mut axis = Axis::new(Tag::default());
		axis.axis_tag = [0; 4];
		for e in element.elements() {
			let value = e.text();
			let fixed = || {
				str_to_fixed_to_float(&value, 16).ok_or_else(|| {
					Error::value(format!("could not convert string to float: {value:?}"))
				})
			};
			let int = || -> Result<u16> {
				let v = eval(&value)?
					.as_int()
					.ok_or_else(|| Error::value(format!("invalid {}", e.name)))?;
				fit(v, &e.name)
			};
			match e.name.as_str() {
				"AxisTag" => {
					// Packed as "4s": truncated or padded with NUL bytes.
					let bytes = fontoxide_misc::py::latin1_encode(&value)
						.ok_or_else(|| Error::value("'ascii' codec can't encode the axis tag"))?;
					let mut tag = [0u8; 4];
					for (d, b) in tag.iter_mut().zip(bytes) {
						*d = b;
					}
					axis.axis_tag = tag;
				}
				"Flags" => axis.flags = int()?,
				"MinValue" => axis.min_value = fixed()?,
				"DefaultValue" => axis.default_value = fixed()?,
				"MaxValue" => axis.max_value = fixed()?,
				"AxisNameID" => axis.axis_name_id = int()?,
				_ => {}
			}
		}
		Ok(axis)
	}
}

/// A named instance (`NamedInstance`).
#[derive(Debug, Clone, PartialEq)]
pub struct NamedInstance {
	/// Name ID of the subfamily name.
	pub subfamily_name_id: u16,
	/// Name ID of the PostScript name, or 0xFFFF for none.
	pub postscript_name_id: u16,
	/// Flags.
	pub flags: u16,
	/// The location, by axis tag.
	pub coordinates: BTreeMap<Tag, f64>,
}

impl Default for NamedInstance {
	fn default() -> Self {
		NamedInstance {
			subfamily_name_id: 0,
			postscript_name_id: 0xFFFF,
			flags: 0,
			coordinates: BTreeMap::new(),
		}
	}
}

impl NamedInstance {
	fn decompile(data: &[u8], axis_tags: &[Tag]) -> Result<Self> {
		let (header, _) = InstanceHeader::unpack2(data)?;
		let mut instance = NamedInstance {
			subfamily_name_id: header.subfamily_name_id,
			flags: header.flags,
			..Default::default()
		};
		let mut r = Reader::new(data);
		r.seek(InstanceHeader::SIZE);
		for &tag in axis_tags {
			let value = r.i32()?;
			instance.coordinates.insert(
				tag,
				fontoxide_misc::fixed_tools::fixed_to_float(value as i64, 16),
			);
		}
		instance.postscript_name_id = if r.remaining() >= 2 { r.u16()? } else { 0xFFFF };
		Ok(instance)
	}

	fn compile(
		&self,
		axis_tags: &[Tag],
		include_postscript_name: bool,
		out: &mut Vec<u8>,
	) -> Result<()> {
		InstanceHeader {
			subfamily_name_id: self.subfamily_name_id,
			flags: self.flags,
		}
		.pack(out)?;
		for tag in axis_tags {
			let v = self.coordinates.get(tag).ok_or_else(|| {
				Error::value(format!("named instance has no coordinate for '{tag}'"))
			})?;
			out.put_i32(fit(float_to_fixed(*v, 16), "coordinate")?);
		}
		if include_postscript_name {
			out.put_u16(self.postscript_name_id);
		}
		Ok(())
	}

	fn to_xml(&self, writer: &mut XmlWriter, names: Option<&Name>, axes: &[Axis]) -> Result<()> {
		if let Some(name) = names.and_then(|n| n.get_debug_name(self.subfamily_name_id)) {
			writer.newline();
			writer.comment(&name);
			writer.newline();
		}
		if let Some(ps) = names.and_then(|n| n.get_debug_name(self.postscript_name_id)) {
			writer.comment(&format!("PostScript: {ps}"));
			writer.newline();
		}
		let flags = format!("0x{:X}", self.flags);
		if self.postscript_name_id == 0xFFFF {
			writer.begintag(
				"NamedInstance",
				&[
					("flags", &flags),
					("subfamilyNameID", &self.subfamily_name_id),
				],
			);
		} else {
			writer.begintag(
				"NamedInstance",
				&[
					("flags", &flags),
					("subfamilyNameID", &self.subfamily_name_id),
					("postscriptNameID", &self.postscript_name_id),
				],
			);
		}
		writer.newline();
		for axis in axes {
			let v = self.coordinates.get(&axis.tag()).ok_or_else(|| {
				Error::value(format!(
					"named instance has no coordinate for '{}'",
					axis.tag()
				))
			})?;
			let value = float_to_fixed_to_str(*v, 16);
			writer.simpletag("coord", &[("axis", &axis.tag_text()), ("value", &value)]);
			writer.newline();
		}
		writer.endtag("NamedInstance");
		writer.newline();
		Ok(())
	}

	fn from_xml(element: &Element) -> Result<Self> {
		let int = |name: &str, default: Option<&str>| -> Result<u16> {
			let text = element
				.attr(name)
				.or(default)
				.ok_or_else(|| Error::ttx(format!("missing '{name}' on <NamedInstance>")))?;
			let v = eval(text)?
				.as_int()
				.ok_or_else(|| Error::value(format!("invalid {name}")))?;
			fit(v, name)
		};
		let mut instance = NamedInstance {
			subfamily_name_id: int("subfamilyNameID", None)?,
			flags: int("flags", Some("0"))?,
			postscript_name_id: int("postscriptNameID", Some("0xFFFF"))?,
			coordinates: BTreeMap::new(),
		};
		for e in element.elements().filter(|e| e.name == "coord") {
			let value = e
				.attr("value")
				.ok_or_else(|| Error::ttx("missing 'value' on <coord>"))?;
			let axis = e
				.attr("axis")
				.ok_or_else(|| Error::ttx("missing 'axis' on <coord>"))?;
			let value = str_to_fixed_to_float(value, 16).ok_or_else(|| {
				Error::value(format!("could not convert string to float: {value:?}"))
			})?;
			let tag = Tag::from_str_exact(axis)
				.ok_or_else(|| Error::value(format!("invalid axis tag: {axis:?}")))?;
			instance.coordinates.insert(tag, value);
		}
		Ok(instance)
	}
}

/// Font Variations table (`table__f_v_a_r`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/fvar>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fvar {
	/// The axes.
	pub axes: Vec<Axis>,
	/// The named instances.
	pub instances: Vec<NamedInstance>,
}

impl Fvar {
	/// The axes as tag → (min, default, max) (`getAxes`).
	pub fn get_axes(&self) -> BTreeMap<Tag, (f64, f64, f64)> {
		self.axes
			.iter()
			.map(|a| (a.tag(), (a.min_value, a.default_value, a.max_value)))
			.collect()
	}

	fn axis_tags(&self) -> Vec<Tag> {
		self.axes.iter().map(Axis::tag).collect()
	}
}

impl TableType for Fvar {
	const TAG: Tag = Tag::new(b"fvar");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let header_data = data.get(..FvarHeader::SIZE).ok_or_else(|| {
			Error::Struct(format!(
				"unpack requires a buffer of {} bytes",
				FvarHeader::SIZE
			))
		})?;
		let (header, _) = FvarHeader::unpack2(header_data)?;
		if header.version != 0x0001_0000 {
			return Err(Error::ttlib(format!(
				"unsupported 'fvar' version {:04x}",
				header.version
			)));
		}
		let slice = |pos: usize, size: usize| {
			data.get(pos..(pos + size).min(data.len()))
				.unwrap_or_default()
		};
		let mut fvar = Fvar::default();
		let mut pos = header.offset_to_data as usize;
		for _ in 0..header.axis_count {
			let (axis, _) = Axis::unpack2(slice(pos, header.axis_size as usize))?;
			fvar.axes.push(axis);
			pos += header.axis_size as usize;
		}
		let axis_tags = fvar.axis_tags();
		for _ in 0..header.instance_count {
			fvar.instances.push(NamedInstance::decompile(
				slice(pos, header.instance_size as usize),
				&axis_tags,
			)?);
			pos += header.instance_size as usize;
		}
		Ok(fvar)
	}
}

impl Table for Fvar {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let include_postscript_names = self
			.instances
			.iter()
			.any(|i| i.postscript_name_id != 0xFFFF);
		let mut instance_size = InstanceHeader::SIZE + 4 * self.axes.len();
		if include_postscript_names {
			instance_size += 2;
		}
		let mut data = Vec::new();
		FvarHeader {
			version: 0x0001_0000,
			offset_to_data: FvarHeader::SIZE as u16,
			count_size_pairs: 2,
			axis_count: fit(self.axes.len(), "axisCount")?,
			axis_size: Axis::SIZE as u16,
			instance_count: fit(self.instances.len(), "instanceCount")?,
			instance_size: fit(instance_size, "instanceSize")?,
		}
		.pack(&mut data)?;
		for axis in &self.axes {
			axis.pack(&mut data)?;
		}
		let axis_tags = self.axis_tags();
		for instance in &self.instances {
			instance.compile(&axis_tags, include_postscript_names, &mut data)?;
		}
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let names = font.get::<Name>()?.map(|n| n.clone());
		for axis in &self.axes {
			axis.to_xml(writer, names.as_ref());
		}
		for instance in &self.instances {
			instance.to_xml(writer, names.as_ref(), &self.axes)?;
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"Axis" => self.axes.push(Axis::from_xml(element)?),
			"NamedInstance" => self.instances.push(NamedInstance::from_xml(element)?),
			_ => {}
		}
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn formats_match_upstream() {
		use crate::sstruct::tests::check;
		check(FvarHeader::FORMAT_NAME, FvarHeader::FIELDS);
		check(Axis::FORMAT_NAME, Axis::FIELDS);
		check(InstanceHeader::FORMAT_NAME, InstanceHeader::FIELDS);
	}
}
