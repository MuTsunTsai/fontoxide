//! Small tables: `VORG` (`V_O_R_G_`), `ltag` (`_l_t_a_g`) and `FFTM`
//! (`F_F_T_M_`).

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::time_tools::{timestamp_from_string, timestamp_to_string};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes, fit};
use crate::sstruct::{U32, U64, Value, eval};
use crate::table::{Table, TableType, TableXml};
use crate::{Error, Font, Result};

fn int_attr<T>(element: &Element, name: &str) -> Result<T>
where
	T: TryFrom<i128>,
{
	let text = element
		.attr(name)
		.ok_or_else(|| Error::ttx(format!("missing '{name}' on <{}>", element.name)))?;
	let v = eval(text)?
		.as_int()
		.ok_or_else(|| Error::value(format!("invalid {name}: {text:?}")))?;
	T::try_from(v).map_err(|_| Error::Struct(format!("{name} out of range: {v}")))
}

/// Vertical Origin table (`table_V_O_R_G_`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/vorg>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Vorg {
	/// 1, or 2 for 24-bit glyph IDs.
	pub major_version: u16,
	/// 0.
	pub minor_version: u16,
	/// The vertical origin of glyphs without a record.
	pub default_vert_origin_y: i16,
	/// The record count as read (or set from TTX); recalculated when compiling.
	pub num_vert_origin_y_metrics: u32,
	/// Vertical origins by glyph ID (`VOriginRecords`, keyed by glyph name in
	/// fontTools).
	pub v_origin_records: BTreeMap<u32, i16>,
}

impl Vorg {
	/// The vertical origin of a glyph (`table[glyph]`).
	pub fn get(&self, gid: u32) -> i16 {
		self.v_origin_records
			.get(&gid)
			.copied()
			.unwrap_or(self.default_vert_origin_y)
	}

	/// Sets the vertical origin of a glyph; a value equal to the default removes
	/// the record (`table[glyph] = value`).
	pub fn set(&mut self, gid: u32, value: i16) {
		if value != self.default_vert_origin_y {
			self.v_origin_records.insert(gid, value);
		} else {
			self.v_origin_records.remove(&gid);
		}
	}

	fn glyph_id_size(&self) -> Result<usize> {
		match self.major_version {
			1 => Ok(2),
			2 => Ok(3),
			_ => Err(Error::Assertion(
				"Unsupported major version of VORG table".into(),
			)),
		}
	}
}

fn uint(bytes: &[u8]) -> u32 {
	bytes.iter().fold(0u32, |acc, &b| acc << 8 | b as u32)
}

impl TableType for Vorg {
	const TAG: Tag = Tag::new(b"VORG");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let mut r = Reader::new(data);
		let mut vorg = Vorg {
			major_version: r.u16()?,
			minor_version: r.u16()?,
			default_vert_origin_y: r.i16()?,
			..Default::default()
		};
		let size = vorg.glyph_id_size()?;
		let slice = |start: usize, len: usize| {
			data.get(start..(start + len).min(data.len()))
				.unwrap_or_default()
		};
		vorg.num_vert_origin_y_metrics = uint(slice(6, size));
		let records = data.get(6 + size..).unwrap_or_default();
		let mut pos = 0;
		for _ in 0..vorg.num_vert_origin_y_metrics {
			let gid = uint(
				records
					.get(pos..(pos + size).min(records.len()))
					.unwrap_or_default(),
			);
			let v = records
				.get(pos + size..pos + size + 2)
				.ok_or_else(|| Error::Struct("unpack requires a buffer of 2 bytes".into()))?;
			pos += size + 2;
			vorg.v_origin_records
				.insert(gid, i16::from_be_bytes([v[0], v[1]]));
		}
		let _ = slice;
		Ok(vorg)
	}
}

impl Table for Vorg {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		self.v_origin_records =
			crate::ot::aat::remap_keys(std::mem::take(&mut self.v_origin_records), f)?;
		Ok(())
	}

	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		self.num_vert_origin_y_metrics = fit(self.v_origin_records.len(), "numVertOriginYMetrics")?;
		let size = self.glyph_id_size()?;
		let put_uint = |data: &mut Vec<u8>, v: u32| -> Result<()> {
			if size == 2 {
				data.put_u16(fit(v, "glyphID")?);
			} else {
				if v > 0xFF_FFFF {
					return Err(Error::Struct("int too big to convert".into()));
				}
				data.put_u24(v);
			}
			Ok(())
		};
		let mut data = Vec::new();
		data.put_u16(self.major_version);
		data.put_u16(self.minor_version);
		data.put_i16(self.default_vert_origin_y);
		put_uint(&mut data, self.num_vert_origin_y_metrics)?;
		for (&gid, &v) in &self.v_origin_records {
			put_uint(&mut data, gid)?;
			data.put_i16(v);
		}
		Ok(data)
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}

impl TableXml for Vorg {
	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		writer.simpletag("majorVersion", &[("value", &self.major_version)]);
		writer.newline();
		writer.simpletag("minorVersion", &[("value", &self.minor_version)]);
		writer.newline();
		writer.simpletag(
			"defaultVertOriginY",
			&[("value", &self.default_vert_origin_y)],
		);
		writer.newline();
		writer.simpletag(
			"numVertOriginYMetrics",
			&[("value", &self.num_vert_origin_y_metrics)],
		);
		writer.newline();
		let order = font.glyph_order()?;
		for (&gid, v) in &self.v_origin_records {
			writer.begintag("VOriginRecord", &[]);
			writer.newline();
			writer.simpletag("glyphName", &[("value", &order.name(gid))]);
			writer.newline();
			writer.simpletag("vOrigin", &[("value", v)]);
			writer.newline();
			writer.endtag("VOriginRecord");
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"VOriginRecord" => {
				let mut name = None;
				let mut v_origin = None;
				for e in element.elements() {
					match e.name.as_str() {
						"glyphName" => name = e.attr("value").map(str::to_owned),
						"vOrigin" => v_origin = Some(int_attr::<i16>(e, "value")?),
						_ => {}
					}
				}
				let name = name.ok_or_else(|| Error::ttx("VOriginRecord without a glyphName"))?;
				let v = v_origin.ok_or_else(|| Error::ttx("VOriginRecord without a vOrigin"))?;
				let gid = font.glyph_id(&name)?;
				self.v_origin_records.insert(gid, v);
			}
			"majorVersion" => self.major_version = int_attr(element, "value")?,
			"minorVersion" => self.minor_version = int_attr(element, "value")?,
			"defaultVertOriginY" => self.default_vert_origin_y = int_attr(element, "value")?,
			"numVertOriginYMetrics" => self.num_vert_origin_y_metrics = int_attr(element, "value")?,
			_ => {}
		}
		Ok(())
	}
}

/// Language Tag table (`table__l_t_a_g`).
///
/// See <https://developer.apple.com/fonts/TrueType-Reference-Manual/RM06/Chap6ltag.html>.
#[derive(Debug, Clone, PartialEq)]
pub struct Ltag {
	/// 1.
	pub version: u32,
	/// 0.
	pub flags: u32,
	/// IETF BCP 47 language tags.
	pub tags: Vec<String>,
}

impl Default for Ltag {
	fn default() -> Self {
		Ltag {
			version: 1,
			flags: 0,
			tags: Vec::new(),
		}
	}
}

impl Ltag {
	/// The index of a tag, adding it if needed (`addTag`).
	pub fn add_tag(&mut self, tag: &str) -> usize {
		match self.tags.iter().position(|t| t == tag) {
			Some(i) => i,
			None => {
				self.tags.push(tag.to_owned());
				self.tags.len() - 1
			}
		}
	}
}

impl TableType for Ltag {
	const TAG: Tag = Tag::new(b"ltag");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let mut r = Reader::new(data);
		let version = r.u32()?;
		let flags = r.u32()?;
		let num_tags = r.u32()?;
		if version != 1 {
			return Err(Error::Assertion("self.version == 1".into()));
		}
		let mut tags = Vec::new();
		for i in 0..num_tags as usize {
			let mut e = Reader::new(data);
			e.seek(12 + i * 4);
			let offset = e.u16()? as usize;
			let length = e.u16()? as usize;
			let bytes = data
				.get(offset..(offset + length).min(data.len()))
				.unwrap_or_default();
			if !bytes.is_ascii() {
				return Err(Error::value("'ascii' codec can't decode the language tag"));
			}
			tags.push(bytes.iter().map(|&b| b as char).collect());
		}
		Ok(Ltag {
			version,
			flags,
			tags,
		})
	}
}

impl Table for Ltag {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Ok(())
	}

	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let mut data = Vec::new();
		data.put_u32(self.version);
		data.put_u32(self.flags);
		data.put_u32(fit(self.tags.len(), "numTags")?);
		let mut pool = String::new();
		for tag in &self.tags {
			let offset = match pool.find(tag.as_str()) {
				Some(o) => o,
				None => {
					let o = pool.len();
					pool.push_str(tag);
					o
				}
			};
			data.put_u16(fit(offset + 12 + self.tags.len() * 4, "offset")?);
			data.put_u16(fit(tag.len(), "length")?);
		}
		if !pool.is_ascii() {
			return Err(Error::value("'ascii' codec can't encode the language tags"));
		}
		data.extend_from_slice(pool.as_bytes());
		Ok(data)
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}

impl TableXml for Ltag {
	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		writer.simpletag("version", &[("value", &self.version)]);
		writer.newline();
		writer.simpletag("flags", &[("value", &self.flags)]);
		writer.newline();
		for tag in &self.tags {
			writer.simpletag("LanguageTag", &[("tag", tag)]);
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"LanguageTag" => {
				let tag = element
					.attr("tag")
					.ok_or_else(|| Error::ttx("missing 'tag' on <LanguageTag>"))?;
				self.tags.push(tag.to_owned());
			}
			"version" => self.version = int_attr(element, "value")?,
			"flags" => self.flags = int_attr(element, "value")?,
			_ => {}
		}
		Ok(())
	}
}

crate::sstruct! {
	/// FontForge Time Stamp table (`table_F_F_T_M_`).
	///
	/// See <https://fontforge.org/docs/techref/non-standard.html>.
	pub struct Fftm = "FFTMFormat" {
		/// 1.
		pub version: u32 = "version" as U32,
		/// FontForge's own build time.
		pub ff_time_stamp: u64 = "FFTimeStamp" as U64,
		/// Creation time of the font source.
		pub source_created: u64 = "sourceCreated" as U64,
		/// Modification time of the font source.
		pub source_modified: u64 = "sourceModified" as U64,
	}
}

impl TableType for Fftm {
	const TAG: Tag = Tag::new(b"FFTM");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		Ok(Fftm::unpack2(data)?.0)
	}
}

impl Table for Fftm {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Ok(())
	}

	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let mut data = Vec::with_capacity(Self::SIZE);
		self.pack(&mut data)?;
		Ok(data)
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}

impl TableXml for Fftm {
	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		writer.comment("FontForge's timestamp, font source creation and modification dates");
		writer.newline();
		for (name, value) in self.values() {
			match (name, value) {
				("version", v) => writer.simpletag(name, &[("value", &v)]),
				(_, Value::Int(i)) => writer.simpletag(name, &[("value", &timestamp_to_string(i))]),
				(_, v) => writer.simpletag(name, &[("value", &v)]),
			}
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		let name = element.name.as_str();
		let value = element
			.attr("value")
			.ok_or_else(|| Error::ttx(format!("missing 'value' on <{name}>")))?;
		let lit = if name == "version" {
			eval(value)?
		} else {
			let t = timestamp_from_string(value)
				.ok_or_else(|| Error::value(format!("invalid timestamp: {value:?}")))?;
			fontoxide_misc::text_tools::Literal::Int(t as i128)
		};
		if !self.set_literal(name, &lit)? {
			return Err(Error::ttx(format!("unknown 'FFTM' field: {name}")));
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn format_matches_upstream() {
		crate::sstruct::tests::check(Fftm::FORMAT_NAME, Fftm::FIELDS);
	}
}
