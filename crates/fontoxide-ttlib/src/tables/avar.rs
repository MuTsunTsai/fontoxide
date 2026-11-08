//! `avar`: Axis Variations table (`fontTools.ttLib.tables._a_v_a_r`).

use std::cmp::Ordering;
use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::fixed_tools::{float_to_fixed_to_str, str_to_fixed_to_float};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::fit;
use crate::generated::ot;
use crate::ot::table::{OtTable, XmlCtx, xml_struct};
use crate::ot::{OtReader, WriterGraph};
use crate::sstruct::eval;
use crate::table::{Table, TableType};
use crate::tables::Fvar;
use crate::{Error, Font, Result};

/// The segment map of one axis: normalized coordinates mapped from → to, as a
/// Python `dict` holds them (no two entries with equal `from`).
pub type SegmentMap = Vec<(f64, f64)>;

/// Sets `map[from] = to`, replacing an entry with an equal `from`.
fn set_mapping(map: &mut SegmentMap, from: f64, to: f64) -> bool {
	match map.iter_mut().find(|(f, _)| *f == from) {
		Some(entry) => {
			entry.1 = to;
			true
		}
		None => {
			map.push((from, to));
			false
		}
	}
}

/// The entries sorted by `from` (`sorted(mappings.items())`).
fn sorted(map: &SegmentMap) -> SegmentMap {
	let mut v = map.clone();
	v.sort_by(|a, b| {
		a.0.partial_cmp(&b.0)
			.unwrap_or(Ordering::Equal)
			.then(a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal))
	});
	v
}

fn fvar_axis_tags(font: &mut Font) -> Result<Vec<(Tag, String)>> {
	let fvar = font.require::<Fvar>()?;
	Ok(fvar.axes.iter().map(|a| (a.tag(), a.tag_text())).collect())
}

fn parse_f2dot14(s: &str) -> Result<f64> {
	str_to_fixed_to_float(s, 14)
		.ok_or_else(|| Error::value(format!("could not convert string to float: {s:?}")))
}

/// Axis Variations table (`table__a_v_a_r`).
///
/// The substance is [`Avar::segments`], the segment maps by axis tag; for version
/// 2, the variation data is in [`Avar::table`]. See
/// <https://learn.microsoft.com/en-us/typography/opentype/spec/avar>.
#[derive(Debug, Clone, PartialEq)]
pub struct Avar {
	/// Major version, 1 or 2 (`majorVersion`).
	pub major_version: u16,
	/// Minor version (`minorVersion`).
	pub minor_version: u16,
	/// The segment maps by axis tag (`segments`). Compiling needs one for every
	/// `fvar` axis.
	pub segments: BTreeMap<Tag, SegmentMap>,
	/// The underlying otData table (`table`); for version 2, it holds `VarIdxMap`
	/// and `VarStore`.
	pub table: ot::Avar,
}

impl Default for Avar {
	fn default() -> Self {
		Avar {
			major_version: 1,
			minor_version: 0,
			segments: BTreeMap::new(),
			table: ot::Avar::default(),
		}
	}
}

impl TableType for Avar {
	const TAG: Tag = Tag::new(b"avar");

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		let mut reader = OtReader::new(data, Self::TAG);
		let table = ot::Avar::decompile(&mut reader, font)?;
		let major_version = (table.version >> 16) as u16;
		let minor_version = (table.version & 0xFFFF) as u16;
		if !matches!(major_version, 1 | 2) {
			return Err(Error::ttlib("Unknown avar table version"));
		}
		let tags = fvar_axis_tags(font)?;
		let mut segments: BTreeMap<Tag, SegmentMap> =
			tags.iter().map(|(t, _)| (*t, SegmentMap::new())).collect();
		for ((tag, _), segment_map) in tags.iter().zip(&table.axis_segment_map) {
			let mut map = SegmentMap::new();
			for m in &segment_map.axis_value_map {
				set_mapping(&mut map, m.from_coordinate, m.to_coordinate);
			}
			segments.insert(*tag, map);
		}
		Ok(Avar {
			major_version,
			minor_version,
			segments,
			table,
		})
	}
}

impl Table for Avar {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		let tags = fvar_axis_tags(font)?;
		self.table.version = (u32::from(self.major_version) << 16) | u32::from(self.minor_version);
		self.table.axis_count = Some(fit(tags.len(), "AxisCount")?);
		self.table.axis_segment_map = tags
			.iter()
			.map(|(tag, text)| {
				let map = self
					.segments
					.get(tag)
					.ok_or_else(|| Error::ttlib(format!("KeyError: {text:?}")))?;
				Ok(ot::AxisSegmentMap {
					position_map_count: Some(fit(map.len(), "PositionMapCount")?),
					axis_value_map: sorted(map)
						.into_iter()
						.map(|(from, to)| ot::AxisValueMap {
							from_coordinate: from,
							to_coordinate: to,
						})
						.collect(),
				})
			})
			.collect::<Result<_>>()?;
		let mut graph = WriterGraph::new(Self::TAG);
		self.table.compile(&mut graph.root(), font)?;
		graph.get_all_data()
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		writer.simpletag(
			"version",
			&[
				("major", &self.major_version),
				("minor", &self.minor_version),
			],
		);
		writer.newline();
		for (tag, text) in fvar_axis_tags(font)? {
			let map = self
				.segments
				.get(&tag)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {text:?}")))?;
			writer.begintag("segment", &[("axis", &text)]);
			writer.newline();
			for (from, to) in sorted(map) {
				let from = float_to_fixed_to_str(from, 14);
				let to = float_to_fixed_to_str(to, 14);
				writer.simpletag("mapping", &[("from", &from), ("to", &to)]);
				writer.newline();
			}
			writer.endtag("segment");
			writer.newline();
		}
		// LENIENT(L1): fontTools fails with AttributeError when a version 2 table
		// read from TTX has no `VarIdxMap` / `VarStore`; they are skipped here, and
		// compiled as null offsets.
		if self.major_version >= 2 {
			if let Some(m) = self.table.var_idx_map.as_deref() {
				xml_struct(writer, font, Some(m), Some("VarIdxMap"), &[])?;
			}
			if let Some(s) = self.table.var_store.as_deref() {
				xml_struct(writer, font, Some(s), Some("VarStore"), &[])?;
			}
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"version" => {
				let int = |name: &str| -> Result<u16> {
					let text = element
						.attr(name)
						.ok_or_else(|| Error::ttx(format!("KeyError: {name:?}")))?;
					let v = eval(text)?
						.as_int()
						.ok_or_else(|| Error::value(format!("invalid {name}")))?;
					fit(v, name)
				};
				self.major_version = int("major")?;
				self.minor_version = int("minor")?;
				self.table.version =
					(u32::from(self.major_version) << 16) | u32::from(self.minor_version);
			}
			"segment" => {
				let axis = element
					.attr("axis")
					.ok_or_else(|| Error::ttx("KeyError: 'axis'"))?;
				let tag = Tag::from_str_exact(axis)
					.ok_or_else(|| Error::value(format!("invalid axis tag: {axis:?}")))?;
				let mut map = SegmentMap::new();
				for e in element.elements().filter(|e| e.name == "mapping") {
					let attr = |name: &str| {
						e.attr(name)
							.ok_or_else(|| Error::ttx(format!("KeyError: {name:?}")))
					};
					let from = parse_f2dot14(attr("from")?)?;
					let to = parse_f2dot14(attr("to")?)?;
					if set_mapping(&mut map, from, to) {
						log::warn!("duplicate entry for {from:?} in axis '{axis}'");
					}
				}
				self.segments.insert(tag, map);
			}
			_ => {
				let mut ctx = XmlCtx::default();
				self.table.from_xml(element, font, &mut ctx)?;
				self.table.populate_defaults(&mut ctx)?;
			}
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
	fn mappings_are_a_dict() {
		let mut map = SegmentMap::new();
		assert!(!set_mapping(&mut map, 0.5, 0.25));
		assert!(!set_mapping(&mut map, -1.0, -1.0));
		assert!(set_mapping(&mut map, 0.5, 0.75));
		assert_eq!(sorted(&map), vec![(-1.0, -1.0), (0.5, 0.75)]);
	}
}
