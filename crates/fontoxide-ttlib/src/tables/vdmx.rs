//! `VDMX`: the Vertical Device Metrics table
//! (`fontTools.ttLib.tables.V_D_M_X_`).
//!
//! See <https://learn.microsoft.com/en-us/typography/opentype/spec/vdmx>.

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes, fit};
use crate::sstruct::eval;
use crate::table::{Table, TableType, TableXml};
use crate::{Error, Font, Result};

/// An aspect ratio range (`ratRange`), with the group it uses.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RatRange {
	/// Character set (`bCharSet`).
	pub b_char_set: i64,
	/// The x ratio (`xRatio`).
	pub x_ratio: i64,
	/// The starting y ratio (`yStartRatio`).
	pub y_start_ratio: i64,
	/// The ending y ratio (`yEndRatio`).
	pub y_end_ratio: i64,
	/// The index of the group in [`Vdmx::groups`] (`groupIndex`).
	pub group_index: Option<i64>,
}

/// A group: (yMax, yMin) by yPelHeight.
pub type VdmxGroup = BTreeMap<i64, (i64, i64)>;

/// Vertical Device Metrics table (`table_V_D_M_X_`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Vdmx {
	/// The version (0 or 1).
	pub version: i64,
	/// The number of groups, as read; recalculated when compiling.
	pub num_recs: usize,
	/// The number of ratio ranges, as read; recalculated when compiling.
	pub num_ratios: usize,
	/// The aspect ratio ranges (`ratRanges`).
	pub rat_ranges: Vec<RatRange>,
	/// The groups (`groups`).
	pub groups: Vec<VdmxGroup>,
}

fn struct_error(size: usize) -> Error {
	Error::Struct(format!("unpack requires a buffer of {size} bytes"))
}

fn empty_min() -> Error {
	Error::value("min() iterable argument is empty")
}

/// The next `n` bytes, as `sstruct.unpack2` takes them.
fn take<'a>(r: &mut Reader<'a>, n: usize) -> Result<Reader<'a>> {
	let bytes = r.bytes(n).map_err(|_| struct_error(n))?;
	Ok(Reader::new(bytes))
}

impl TableType for Vdmx {
	const TAG: Tag = Tag::new(b"VDMX");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let mut r = Reader::new(data);
		let mut h = take(&mut r, 6)?;
		let mut table = Vdmx {
			version: i64::from(h.u16()?),
			num_recs: usize::from(h.u16()?),
			num_ratios: usize::from(h.u16()?),
			..Default::default()
		};
		let mut pos = 6;
		for _ in 0..table.num_ratios {
			let mut b = take(&mut r, 4)?;
			table.rat_ranges.push(RatRange {
				b_char_set: i64::from(b.u8()?),
				x_ratio: i64::from(b.u8()?),
				y_start_ratio: i64::from(b.u8()?),
				y_end_ratio: i64::from(b.u8()?),
				group_index: None,
			});
			pos += 4;
		}
		let mut offsets = Vec::with_capacity(table.num_ratios);
		for _ in 0..table.num_ratios {
			offsets.push(usize::from(take(&mut r, 2)?.u16()?));
			pos += 2;
		}
		for group_index in 0..table.num_recs {
			let curr_offset = pos;
			let mut g = take(&mut r, 4)?;
			let recs = g.u16()?;
			let startsz = i64::from(g.u8()?);
			let endsz = i64::from(g.u8()?);
			pos += 4;
			let mut group = VdmxGroup::new();
			for _ in 0..recs {
				let mut v = take(&mut r, 6)?;
				let y_pel_height = i64::from(v.u16()?);
				let y_max = i64::from(v.i16()?);
				let y_min = i64::from(v.i16()?);
				pos += 6;
				group.insert(y_pel_height, (y_max, y_min));
			}
			let min_size = *group.keys().next().ok_or_else(empty_min)?;
			let max_size = *group.keys().next_back().ok_or_else(empty_min)?;
			// UPSTREAM-BUG(B41): the assertion messages read `group.startsz` of a dict,
			// so a failed check raises AttributeError instead.
			if startsz != min_size || endsz != max_size {
				let name = if startsz != min_size {
					"startsz"
				} else {
					"endsz"
				};
				return Err(Error::ttlib(format!(
					"AttributeError: 'dict' object has no attribute '{name}'"
				)));
			}
			table.groups.push(group);
			for (offset_index, &offset) in offsets.iter().enumerate() {
				// Several ratio ranges can share a group.
				if curr_offset == offset {
					table.rat_ranges[offset_index].group_index = Some(group_index as i64);
				}
			}
		}
		for (i, ratio) in table.rat_ranges.iter().enumerate() {
			if ratio.group_index.is_none() {
				return Err(Error::ttlib(format!("no group defined for ratRange {i}")));
			}
		}
		Ok(table)
	}
}

impl Vdmx {
	/// The offsets of the groups of the ratio ranges (`_getOffsets`).
	fn offsets(&self) -> Result<Vec<usize>> {
		let mut pos = 6 + self.rat_ranges.len() * 4 + self.rat_ranges.len() * 2;
		let mut group_offsets = Vec::with_capacity(self.groups.len());
		for group in &self.groups {
			group_offsets.push(pos);
			pos += 4 + group.len() * 6;
		}
		self.rat_ranges
			.iter()
			.map(|ratio| {
				let index = ratio.group_index.ok_or_else(|| {
					Error::value("list indices must be integers or slices, not NoneType")
				})?;
				let index = if index < 0 {
					index + group_offsets.len() as i64
				} else {
					index
				};
				usize::try_from(index)
					.ok()
					.and_then(|i| group_offsets.get(i).copied())
					.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))
			})
			.collect()
	}
}

impl Table for Vdmx {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		if self.version != 0 && self.version != 1 {
			return Err(Error::ttlib(format!(
				"unknown format for VDMX table: version {}",
				self.version
			)));
		}
		self.num_ratios = self.rat_ranges.len();
		self.num_recs = self.groups.len();
		let mut out = Vec::new();
		out.put_u16(fit(self.version, "version")?);
		out.put_u16(fit(self.num_recs, "numRecs")?);
		out.put_u16(fit(self.num_ratios, "numRatios")?);
		for ratio in &self.rat_ranges {
			out.put_u8(fit(ratio.b_char_set, "bCharSet")?);
			out.put_u8(fit(ratio.x_ratio, "xRatio")?);
			out.put_u8(fit(ratio.y_start_ratio, "yStartRatio")?);
			out.put_u8(fit(ratio.y_end_ratio, "yEndRatio")?);
		}
		for offset in self.offsets()? {
			out.put_u16(fit(offset, "offset")?);
		}
		for group in &self.groups {
			let startsz = *group.keys().next().ok_or_else(empty_min)?;
			let endsz = *group.keys().next_back().ok_or_else(empty_min)?;
			out.put_u16(fit(group.len(), "recs")?);
			out.put_u8(fit(startsz, "startsz")?);
			out.put_u8(fit(endsz, "endsz")?);
			for (&y_pel_height, &(y_max, y_min)) in group {
				out.put_u16(fit(y_pel_height, "yPelHeight")?);
				out.put_i16(fit(y_max, "yMax")?);
				out.put_i16(fit(y_min, "yMin")?);
			}
		}
		Ok(out)
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Ok(())
	}
}

impl TableXml for Vdmx {
	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		writer.simpletag("version", &[("value", &self.version)]);
		writer.newline();
		writer.begintag("ratRanges", &[]);
		writer.newline();
		for ratio in &self.rat_ranges {
			let group_index = match ratio.group_index {
				Some(i) => i.to_string(),
				None => "None".to_owned(),
			};
			writer.simpletag(
				"ratRange",
				&[
					("bCharSet", &ratio.b_char_set),
					("xRatio", &ratio.x_ratio),
					("yStartRatio", &ratio.y_start_ratio),
					("yEndRatio", &ratio.y_end_ratio),
					("groupIndex", &group_index),
				],
			);
			writer.newline();
		}
		writer.endtag("ratRanges");
		writer.newline();
		writer.begintag("groups", &[]);
		writer.newline();
		for (index, group) in self.groups.iter().enumerate() {
			let startsz = *group.keys().next().ok_or_else(empty_min)?;
			let endsz = *group.keys().next_back().ok_or_else(empty_min)?;
			writer.begintag("group", &[("index", &index)]);
			writer.newline();
			writer.comment(&format!(
				"recs={}, startsz={startsz}, endsz={endsz}",
				group.len()
			));
			writer.newline();
			for (y_pel_height, (y_max, y_min)) in group {
				writer.simpletag_ordered(
					"record",
					&[
						("yPelHeight", y_pel_height),
						("yMax", y_max),
						("yMin", y_min),
					],
				);
				writer.newline();
			}
			writer.endtag("group");
			writer.newline();
		}
		writer.endtag("groups");
		writer.newline();
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		let int = |e: &Element, name: &str| -> Result<i64> {
			let text = e
				.attr(name)
				.ok_or_else(|| Error::ttx(format!("missing '{name}' on <{}>", e.name)))?;
			let v = eval(text)?
				.as_int()
				.ok_or_else(|| Error::value(format!("invalid value for {name}")))?;
			fit(v, name)
		};
		match element.name.as_str() {
			"version" => self.version = int(element, "value")?,
			"ratRanges" => {
				for child in element.elements().filter(|e| e.name == "ratRange") {
					self.num_ratios += 1;
					let group_index = match child.attr("groupIndex") {
						Some("None") => None,
						_ => Some(int(child, "groupIndex")?),
					};
					self.rat_ranges.push(RatRange {
						b_char_set: int(child, "bCharSet")?,
						x_ratio: int(child, "xRatio")?,
						y_start_ratio: int(child, "yStartRatio")?,
						y_end_ratio: int(child, "yEndRatio")?,
						group_index,
					});
				}
			}
			"groups" => {
				for child in element.elements().filter(|e| e.name == "group") {
					self.num_recs += 1;
					let mut group = VdmxGroup::new();
					for record in child.elements().filter(|e| e.name == "record") {
						let y_pel_height = int(record, "yPelHeight")?;
						let y_max = int(record, "yMax")?;
						let y_min = int(record, "yMin")?;
						group.insert(y_pel_height, (y_max, y_min));
					}
					self.groups.push(group);
				}
			}
			_ => {}
		}
		Ok(())
	}
}
