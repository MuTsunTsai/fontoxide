//! The `COLR` classes written by hand: `ClipList` (kept by fontTools as a map
//! from glyph to clip box) and the TTX form of `Paint`.

use std::collections::BTreeMap;

use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use super::table::{OtTable, XmlCtx, begintag, parse_int, simpletag, value_attr};
use super::{OtReader, OtWriter};
use crate::generated::ot::{ClipBox, ClipListRaw, ClipListRawFormat1, ClipRecord, Paint};
use crate::{Error, Font, Result};

/// The names of the `Paint` formats (`PaintFormat`).
const PAINT_FORMATS: [&str; 33] = [
	"PaintColrLayers",
	"PaintSolid",
	"PaintVarSolid",
	"PaintLinearGradient",
	"PaintVarLinearGradient",
	"PaintRadialGradient",
	"PaintVarRadialGradient",
	"PaintSweepGradient",
	"PaintVarSweepGradient",
	"PaintGlyph",
	"PaintColrGlyph",
	"PaintTransform",
	"PaintVarTransform",
	"PaintTranslate",
	"PaintVarTranslate",
	"PaintScale",
	"PaintVarScale",
	"PaintScaleAroundCenter",
	"PaintVarScaleAroundCenter",
	"PaintScaleUniform",
	"PaintVarScaleUniform",
	"PaintScaleUniformAroundCenter",
	"PaintVarScaleUniformAroundCenter",
	"PaintRotate",
	"PaintVarRotate",
	"PaintRotateAroundCenter",
	"PaintVarRotateAroundCenter",
	"PaintSkew",
	"PaintVarSkew",
	"PaintSkewAroundCenter",
	"PaintVarSkewAroundCenter",
	"PaintComposite",
	"PaintGlyph2",
];

/// The name of a `Paint` format (`Paint.getFormatName`).
pub fn paint_format_name(format: u16) -> Option<&'static str> {
	PAINT_FORMATS
		.get((format as usize).checked_sub(1)?)
		.copied()
}

/// `Paint.toXML`: the format name follows the start tag as a comment.
pub fn paint_to_xml(
	paint: &Paint,
	xw: &mut XmlWriter,
	font: &mut Font,
	attrs: &[(&'static str, String)],
	name: &str,
) -> Result<()> {
	let format = paint.format().unwrap_or(0);
	let mut attrs = attrs.to_vec();
	attrs.push(("Format", format.to_string()));
	let format_name = paint_format_name(format).ok_or_else(|| {
		Error::ttlib(format!(
			"NotImplementedError: Unknown Paint format: {format}"
		))
	})?;
	begintag(xw, name, &attrs);
	xw.comment(format_name);
	xw.newline();
	paint.to_xml2(xw, font)?;
	xw.endtag(name);
	xw.newline();
	Ok(())
}

/// The values of a clip box, by which `ClipList` groups glyphs
/// (`ClipBox.as_tuple`).
fn clip_box_key(clip_box: &ClipBox) -> Vec<i64> {
	match clip_box {
		ClipBox::Format1(b) => vec![
			b.x_min as i64,
			b.y_min as i64,
			b.x_max as i64,
			b.y_max as i64,
		],
		ClipBox::Format2(b) => vec![
			b.x_min as i64,
			b.y_min as i64,
			b.x_max as i64,
			b.y_max as i64,
			b.var_index_base as i64,
		],
		ClipBox::Unknown(_) => Vec::new(),
	}
}

/// The clip boxes of `COLR` glyphs (`ClipList`, kept by fontTools as `clips`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClipList {
	/// The `Format` attribute: 1 when read from a font.
	pub format: Option<u16>,
	/// The clip box of each glyph.
	pub clips: BTreeMap<u32, ClipBox>,
}

impl ClipList {
	/// `ClipList.groups`: the glyphs sharing each clip box, in glyph order.
	pub fn groups(&self) -> Vec<(Vec<u32>, &ClipBox)> {
		let mut groups: Vec<(Vec<i64>, Vec<u32>, &ClipBox)> = Vec::new();
		for (&gid, clip_box) in &self.clips {
			let key = clip_box_key(clip_box);
			match groups.iter_mut().find(|g| g.0 == key) {
				Some(g) => g.1.push(gid),
				None => groups.push((key, vec![gid], clip_box)),
			}
		}
		groups.into_iter().map(|(_, g, b)| (g, b)).collect()
	}

	/// `postRead`.
	fn from_raw(raw: ClipListRaw, font: &mut Font) -> Result<Self> {
		let ClipListRaw::Format1(raw) = raw else {
			// fontTools reads no fields for an unknown format.
			return Err(Error::ttlib("KeyError: 'ClipRecord'"));
		};
		let num_glyphs = font.glyph_order()?.len() as u32;
		let mut clips = BTreeMap::new();
		for (i, rec) in raw.clip_record.into_iter().enumerate() {
			let (start, end) = (rec.start_glyph_id as u32, rec.end_glyph_id as u32);
			if start > end {
				log::warn!(
					"invalid ClipRecord[{i}].StartGlyphID ({start}) > EndGlyphID ({end}); skipped"
				);
				continue;
			}
			// LENIENT(L5): fontTools keeps a null clip box and fails when writing.
			let clip_box = rec.clip_box.ok_or_else(|| {
				Error::ttlib("AttributeError: 'NoneType' object has no attribute 'as_tuple'")
			})?;
			let mut redefined = Vec::new();
			let mut missing = Vec::new();
			for gid in start..=end {
				if gid >= num_glyphs {
					missing.push(gid);
					continue;
				}
				match clips.entry(gid) {
					std::collections::btree_map::Entry::Occupied(_) => redefined.push(gid),
					std::collections::btree_map::Entry::Vacant(v) => {
						v.insert((*clip_box).clone());
					}
				}
			}
			if let (Some(lo), Some(hi)) = (redefined.first(), redefined.last()) {
				log::warn!(
					"ClipRecord[{i}] overlaps previous records; ignoring redefined clip boxes for the following glyph ID range: [{lo}-{hi}]"
				);
			}
			if let (Some(lo), Some(hi)) = (missing.first(), missing.last()) {
				log::warn!("ClipRecord[{i}] range references missing glyph IDs: [{lo}-{hi}]");
			}
		}
		Ok(ClipList {
			format: Some(1),
			clips,
		})
	}

	/// `preWrite`: one record per range of consecutive glyphs sharing a clip box.
	fn to_raw(&self) -> Result<ClipListRawFormat1> {
		let mut ranges: Vec<(u32, u32, &ClipBox)> = Vec::new();
		for (gids, clip_box) in self.groups() {
			let mut start = gids[0];
			let mut last = start;
			for &gid in &gids[1..] {
				if gid != last + 1 {
					ranges.push((start, last, clip_box));
					start = gid;
				}
				last = gid;
			}
			ranges.push((start, last, clip_box));
		}
		ranges.sort_by_key(|r| (r.0, r.1));
		let glyph_id = |gid: u32| {
			u16::try_from(gid)
				.map_err(|_| Error::Struct("'H' format requires 0 <= number <= 65535".into()))
		};
		let clip_record = ranges
			.into_iter()
			.map(|(start, end, clip_box)| {
				Ok(ClipRecord {
					start_glyph_id: glyph_id(start)?,
					end_glyph_id: glyph_id(end)?,
					clip_box: Some(Box::new(clip_box.clone())),
				})
			})
			.collect::<Result<_>>()?;
		Ok(ClipListRawFormat1 {
			clip_count: None,
			clip_record,
		})
	}
}

impl OtTable for ClipList {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		self.clips = super::aat::remap_keys(std::mem::take(&mut self.clips), f)?;
		Ok(())
	}

	fn class_name(&self) -> &'static str {
		"ClipList"
	}

	fn decompile(r: &mut OtReader<'_>, font: &mut Font) -> Result<Self> {
		let raw = ClipListRaw::decompile(r, font)?;
		Self::from_raw(raw, font)
	}

	fn compile(&self, w: &mut OtWriter<'_>, font: &mut Font) -> Result<()> {
		let format = self.format.ok_or_else(|| {
			Error::ttlib("AttributeError: 'ClipList' object has no attribute 'Format'")
		})?;
		if format != 1 {
			// fontTools writes the format and no fields.
			w.write_u8(format as u8);
			return Ok(());
		}
		ClipListRaw::Format1(self.to_raw()?).compile(w, font)
	}

	fn format(&self) -> Option<u16> {
		self.format
	}

	fn with_format(format: u16) -> Self {
		ClipList {
			format: Some(format),
			clips: BTreeMap::new(),
		}
	}

	fn to_xml2(&self, xw: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let order = font.glyph_order()?.clone();
		let mut groups: Vec<(Vec<String>, &ClipBox)> = self
			.groups()
			.into_iter()
			.map(|(gids, clip_box)| {
				let mut names: Vec<String> =
					gids.iter().map(|&g| order.name(g).into_owned()).collect();
				names.sort();
				(names, clip_box)
			})
			.collect();
		// Sorted by the alphabetically first glyph.
		groups.sort_by(|a, b| a.0[0].cmp(&b.0[0]));
		for (names, clip_box) in groups {
			xw.begintag("Clip", &[]);
			xw.newline();
			for name in names {
				simpletag(xw, "Glyph", &[("value", name)]);
				xw.newline();
			}
			clip_box.to_xml(xw, font, &[], "ClipBox")?;
			xw.endtag("Clip");
			xw.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font, ctx: &mut XmlCtx) -> Result<()> {
		if element.name != "Clip" {
			return Err(Error::Assertion("name == \"Clip\"".into()));
		}
		let mut glyphs = Vec::new();
		let mut clip_box = None;
		for child in element.elements() {
			match child.name.as_str() {
				"Glyph" => glyphs.push(value_attr(child)?.to_owned()),
				"ClipBox" => {
					let format = child
						.attr("Format")
						.ok_or_else(|| Error::ttx("KeyError: 'Format'"))?;
					let mut b = ClipBox::with_format(parse_int::<u16>(format)?);
					for e in child.elements() {
						b.from_xml(e, font, ctx)?;
					}
					clip_box = Some(b);
				}
				_ => {}
			}
		}
		if let Some(clip_box) = clip_box {
			// LENIENT(L6): fontTools keeps unknown glyph names (and dumps them again)
			// but leaves them out when writing; they are dropped here.
			let order = font.glyph_order()?;
			for name in glyphs {
				if let Some(&gid) = order.reverse_map().get(&name) {
					self.clips.insert(gid, clip_box.clone());
				}
			}
		}
		Ok(())
	}

	fn populate_defaults(&mut self, _ctx: &mut XmlCtx) -> Result<()> {
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn paint_format_names() {
		assert_eq!(paint_format_name(0), None);
		assert_eq!(paint_format_name(1), Some("PaintColrLayers"));
		assert_eq!(paint_format_name(33), Some("PaintGlyph2"));
		assert_eq!(paint_format_name(34), None);
	}
}
