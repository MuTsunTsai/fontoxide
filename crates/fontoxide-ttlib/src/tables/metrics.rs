//! `hmtx` and `vmtx`: Horizontal and Vertical Metrics tables
//! (`fontTools.ttLib.tables._h_m_t_x`, `_v_m_t_x`), and the metric
//! recalculations shared with `hhea`, `vhea` and `maxp`.

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_pens::BoundsPen;

use crate::binary::{Reader, WriteBytes};
use crate::glyph_set::GlyphSet;
use crate::sstruct::eval;
use crate::table::{Table, TableType};
use crate::tables::cff::{Cff, Cff2};
use crate::tables::glyf::Glyf;
use crate::tables::hhea::{Hhea, Vhea};
use crate::tables::maxp::Maxp;
use crate::{Error, Font, Result};

/// An advance and a side bearing.
pub type Metric = (u16, i16);

macro_rules! metrics_table {
	(
		$(#[$meta:meta])*
		$ty:ident, $tag:literal, $header:ty, $header_field:ident, $advance:literal, $side_bearing:literal
	) => {
		$(#[$meta])*
		#[derive(Debug, Clone, PartialEq, Default)]
		pub struct $ty {
			/// Advance and side bearing by glyph ID (`metrics`, which fontTools keys
			/// by glyph name).
			pub metrics: BTreeMap<u32, Metric>,
		}

		impl $ty {
			/// The metrics of a glyph (`table[glyphName]`).
			pub fn get(&self, gid: u32) -> Option<Metric> {
				self.metrics.get(&gid).copied()
			}

			/// Sets the metrics of a glyph (`table[glyphName] = (advance, sb)`).
			pub fn set(&mut self, gid: u32, metric: Metric) {
				self.metrics.insert(gid, metric);
			}
		}

		impl TableType for $ty {
			const TAG: Tag = Tag::new($tag);

			fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
				let num_glyphs = font.require::<Maxp>()?.num_glyphs as usize;
				let mut number_of_metrics = match font.get::<$header>()? {
					Some(h) => h.$header_field as usize,
					None => num_glyphs,
				};
				if number_of_metrics > num_glyphs {
					log::warn!(
						"The {}.{} exceeds the maxp.numGlyphs",
						<$header>::TAG,
						stringify!($header_field)
					);
					number_of_metrics = num_glyphs;
				}
				let number_of_side_bearings = num_glyphs - number_of_metrics;
				let table_size = 4 * number_of_metrics + 2 * number_of_side_bearings;
				if data.len() < table_size {
					return Err(Error::ttlib(format!(
						"not enough '{}' table data: expected {table_size} bytes, got {}",
						Self::TAG,
						data.len()
					)));
				}
				let mut r = Reader::new(data);
				let mut metrics = BTreeMap::new();
				let mut last_advance = None;
				for gid in 0..number_of_metrics {
					let advance = r.u16()?;
					let side_bearing = r.i16()?;
					if advance > 32767 {
						log::warn!(
							"Glyph {gid} has a huge advance {} ({advance}); is it intentional or an (invalid) negative value?",
							$advance
						);
					}
					metrics.insert(gid as u32, (advance, side_bearing));
					last_advance = Some(advance);
				}
				// fontTools reads metrics[-2], which fails without long metrics.
				let last_advance = last_advance
					.ok_or_else(|| Error::ttlib(format!("'{}' table has no long metrics", Self::TAG)))?;
				for i in 0..number_of_side_bearings {
					let side_bearing = r.i16()?;
					metrics.insert((number_of_metrics + i) as u32, (last_advance, side_bearing));
				}
				if r.remaining() > 0 {
					log::warn!("too much '{}' table data", Self::TAG);
				}
				Ok($ty { metrics })
			}
		}

		impl Table for $ty {
			fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
				self.metrics = crate::ot::aat::remap_keys(std::mem::take(&mut self.metrics), f)?;
				Ok(())
			}

			fn tag(&self) -> Tag {
				Self::TAG
			}

			fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
				let order = font.glyph_order()?;
				let mut metrics: Vec<Metric> = Vec::with_capacity(order.len());
				for gid in 0..order.len() as u32 {
					let m = self
						.metrics
						.get(&gid)
						.ok_or_else(|| Error::GlyphNotFound(order.name(gid).into_owned()))?;
					metrics.push(*m);
				}
				let number_of_metrics;
				let mut additional: Vec<i16> = Vec::new();
				if let Some(header) = font.get::<$header>()? {
					let len = metrics.len();
					let last_advance = metrics
						.last()
						.ok_or_else(|| Error::ttlib("list index out of range"))?
						.0;
					// Python indexing, where -1 is the last element.
					let at = |i: isize| metrics[i.rem_euclid(len as isize) as usize].0;
					let mut last_index = len;
					while at(last_index as isize - 2) == last_advance {
						last_index -= 1;
						if last_index <= 1 {
							// All advances are equal.
							last_index = 1;
							break;
						}
					}
					additional = metrics[last_index..].iter().map(|m| m.1).collect();
					metrics.truncate(last_index);
					number_of_metrics = metrics.len();
					header.$header_field = crate::binary::fit(number_of_metrics, stringify!($header_field))?;
				} else {
					// Without a header, numberOfMetrics is assumed to equal numGlyphs.
					number_of_metrics = font.require::<Maxp>()?.num_glyphs as usize;
				}
				if number_of_metrics != metrics.len() {
					return Err(Error::Struct(format!(
						"pack expected {} items for packing (got {})",
						2 * number_of_metrics,
						2 * metrics.len()
					)));
				}
				let mut data = Vec::with_capacity(4 * metrics.len() + 2 * additional.len());
				for (advance, side_bearing) in metrics {
					data.put_u16(advance);
					data.put_i16(side_bearing);
				}
				for side_bearing in additional {
					data.put_i16(side_bearing);
				}
				Ok(data)
			}

			fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
				let order = font.glyph_order()?;
				let mut entries: Vec<(String, Metric)> = self
					.metrics
					.iter()
					.map(|(&gid, &m)| (order.name(gid).into_owned(), m))
					.collect();
				entries.sort_by(|a, b| a.0.cmp(&b.0));
				for (name, (advance, side_bearing)) in &entries {
					writer.simpletag_ordered(
						"mtx",
						&[("name", name), ($advance, advance), ($side_bearing, side_bearing)],
					);
					writer.newline();
				}
				Ok(())
			}

			fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
				if element.name != "mtx" {
					return Ok(());
				}
				let attr = |name: &str| {
					element
						.attr(name)
						.ok_or_else(|| Error::ttx(format!("missing '{name}' on <mtx>")))
				};
				let gid = font.glyph_id(attr("name")?)?;
				let advance = eval(attr($advance)?)?
					.as_int()
					.ok_or_else(|| Error::value(concat!("invalid ", $advance)))?;
				let side_bearing = eval(attr($side_bearing)?)?
					.as_int()
					.ok_or_else(|| Error::value(concat!("invalid ", $side_bearing)))?;
				self.metrics.insert(
					gid,
					(
						crate::binary::fit(advance, $advance)?,
						crate::binary::fit(side_bearing, $side_bearing)?,
					),
				);
				Ok(())
			}

			fn clone_table(&self) -> Box<dyn Table> {
				Box::new(self.clone())
			}
		}
	};
}

metrics_table!(
	/// Horizontal Metrics table (`table__h_m_t_x`).
	///
	/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/hmtx>.
	Hmtx, b"hmtx", Hhea, number_of_h_metrics, "width", "lsb"
);

metrics_table!(
	/// Vertical Metrics table (`table__v_m_t_x`).
	///
	/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/vmtx>.
	Vmtx, b"vmtx", Vhea, number_of_v_metrics, "height", "tsb"
);

/// Results of `hhea.recalc` / `vhea.recalc`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HeaderRecalc {
	pub advance_max: u16,
	pub min_start_side_bearing: i16,
	pub min_end_side_bearing: i16,
	pub max_extent: i16,
}

/// The shared part of `hhea.recalc` and `vhea.recalc`. Returns `None` when there is
/// nothing to recalculate.
///
/// The extents come from the outlines: of the glyph set with `VARC`, else of
/// `glyf`, `CFF ` or `CFF2`.
pub(crate) fn recalc_header(font: &mut Font, vertical: bool) -> Result<Option<HeaderRecalc>> {
	let metrics_tag = Tag::new(if vertical { b"vmtx" } else { b"hmtx" });
	if !font.contains(metrics_tag) {
		return Ok(None);
	}
	let order = font.glyph_order()?;
	let mut extents = Vec::new();
	if font.contains(Tag::new(b"VARC")) {
		let glyph_set = GlyphSet::new(font)?;
		for (gid, name) in order.names().iter().enumerate() {
			let mut pen = BoundsPen::new_bounds(Some(&glyph_set));
			glyph_set.draw(name, &mut pen)?;
			if let Some(b) = pen.bounds() {
				let size = if vertical {
					b[3].ceil() as i64 - b[1].floor() as i64
				} else {
					b[2].ceil() as i64 - b[0].floor() as i64
				};
				extents.push((gid as u32, size));
			}
		}
	} else if !font.contains(Glyf::TAG) {
		let cff = if font.contains(Cff::TAG) {
			&mut font.require::<Cff>()?.cff
		} else if font.contains(Cff2::TAG) {
			&mut font.require::<Cff2>()?.cff
		} else {
			return Ok(None);
		};
		for (gid, name) in order.names().iter().enumerate() {
			let index = cff
				.glyph_index_hint(name, gid)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {name:?}")))?;
			if let Some(b) = cff.calc_bounds(index)? {
				let size = if vertical {
					b[3].ceil() as i64 - b[1].floor() as i64
				} else {
					b[2].ceil() as i64 - b[0].floor() as i64
				};
				extents.push((gid as u32, size));
			}
		}
	} else {
		let glyf = font.require::<Glyf>()?;
		for gid in 0..order.len() as u32 {
			let g = glyf
				.glyphs
				.get(&gid)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {:?}", order.name(gid))))?;
			if g.number_of_contours == 0 {
				continue;
			}
			extents.push((
				gid,
				if vertical {
					g.y_max - g.y_min
				} else {
					g.x_max - g.x_min
				},
			));
		}
	}
	let metrics: BTreeMap<u32, Metric> = if vertical {
		font.require::<Vmtx>()?.metrics.clone()
	} else {
		font.require::<Hmtx>()?.metrics.clone()
	};
	let advance_max = metrics
		.values()
		.map(|m| m.0)
		.max()
		.ok_or_else(|| Error::value("max() arg is an empty sequence"))?;
	if extents.is_empty() {
		// No glyph has outlines.
		return Ok(Some(HeaderRecalc {
			advance_max,
			min_start_side_bearing: 0,
			min_end_side_bearing: 0,
			max_extent: 0,
		}));
	}
	let (mut min_start, mut min_end, mut max_extent) = (i64::MAX, i64::MAX, i64::MIN);
	for (gid, size) in extents {
		let (advance, start) = metrics
			.get(&gid)
			.copied()
			.ok_or_else(|| Error::ttlib(format!("KeyError: {:?}", order.name(gid))))?;
		let (advance, start) = (advance as i64, start as i64);
		min_start = min_start.min(start);
		min_end = min_end.min(advance - start - size);
		max_extent = max_extent.max(start + size);
	}
	let fit = |v: i64, name: &str| crate::binary::fit::<i16, _>(v, name);
	Ok(Some(HeaderRecalc {
		advance_max,
		min_start_side_bearing: fit(min_start, "minStartSideBearing")?,
		min_end_side_bearing: fit(min_end, "minEndSideBearing")?,
		max_extent: fit(max_extent, "maxExtent")?,
	}))
}

/// `maxp.recalc`: the outline maxima from `glyf`, and the font bounding box and
/// the "left side bearing at x = 0" flag in `head`.
pub(crate) fn recalc_maxp(maxp: &mut Maxp, font: &mut Font) -> Result<()> {
	let order = font.glyph_order()?;
	let fit = |v: usize, name: &str| crate::binary::fit::<u16, _>(v, name);
	// What each glyph contributes: its bounds, and whether it has outlines.
	let mut glyphs: Vec<(u32, [i64; 4])> = Vec::new();
	let (mut max_points, mut max_contours) = (0, 0);
	let (mut max_composite_points, mut max_composite_contours) = (0, 0);
	let (mut max_component_elements, mut max_component_depth) = (0, 0i64);
	let num_glyphs;
	{
		let glyf = font.require::<Glyf>()?;
		if glyf.glyphs.len() != order.len() {
			return Err(Error::Assertion(
				"len(self.glyphOrder) == len(self.glyphs)".into(),
			));
		}
		num_glyphs = glyf.glyphs.len();
		let mut cache = std::collections::HashMap::new();
		for gid in 0..order.len() as u32 {
			let g = glyf
				.glyphs
				.get(&gid)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {:?}", order.name(gid))))?;
			if g.number_of_contours == 0 {
				continue;
			}
			glyphs.push((gid, [g.x_min, g.y_min, g.x_max, g.y_max]));
			if g.number_of_contours > 0 {
				max_points = max_points.max(g.coordinates.len());
				max_contours = max_contours.max(g.end_pts_of_contours.len());
			} else if g.is_composite() {
				let (p, c, d) =
					glyf.composite_maxp_values(gid, &order, 1, &mut cache, &mut vec![gid])?;
				max_composite_points = max_composite_points.max(p);
				max_composite_contours = max_composite_contours.max(c);
				max_component_elements = max_component_elements.max(g.components.len());
				max_component_depth = max_component_depth.max(d);
			}
		}
	}
	let mut all_x_min_is_lsb = true;
	{
		let hmtx = font.require::<Hmtx>()?;
		for &(gid, b) in &glyphs {
			let (_, lsb) = hmtx
				.get(gid)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {:?}", order.name(gid))))?;
			if lsb as i64 != b[0] {
				all_x_min_is_lsb = false;
			}
		}
	}
	let bounds = glyphs.iter().fold(None, |acc: Option<[i64; 4]>, &(_, b)| {
		Some(match acc {
			None => b,
			Some(a) => [
				a[0].min(b[0]),
				a[1].min(b[1]),
				a[2].max(b[2]),
				a[3].max(b[3]),
			],
		})
	});
	let head = font.require::<crate::tables::Head>()?;
	let [x_min, y_min, x_max, y_max] = bounds.unwrap_or([0; 4]);
	let fit16 = |v: i64, name: &str| crate::binary::fit::<i16, _>(v, name);
	head.x_min = fit16(x_min, "xMin")?;
	head.y_min = fit16(y_min, "yMin")?;
	head.x_max = fit16(x_max, "xMax")?;
	head.y_max = fit16(y_max, "yMax")?;
	if all_x_min_is_lsb {
		head.flags |= 0x2;
	} else {
		head.flags &= !0x2;
	}
	maxp.num_glyphs = fit(num_glyphs, "numGlyphs")?;
	maxp.v1.max_points = fit(max_points, "maxPoints")?;
	maxp.v1.max_contours = fit(max_contours, "maxContours")?;
	maxp.v1.max_composite_points = fit(max_composite_points, "maxCompositePoints")?;
	maxp.v1.max_composite_contours = fit(max_composite_contours, "maxCompositeContours")?;
	maxp.v1.max_component_elements = fit(max_component_elements, "maxComponentElements")?;
	maxp.v1.max_component_depth = fit(max_component_depth.max(0) as usize, "maxComponentDepth")?;
	Ok(())
}
