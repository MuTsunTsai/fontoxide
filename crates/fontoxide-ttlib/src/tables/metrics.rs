//! `hmtx` and `vmtx`: Horizontal and Vertical Metrics tables
//! (`fontTools.ttLib.tables._h_m_t_x`, `_v_m_t_x`), and the metric
//! recalculations shared with `hhea`, `vhea` and `maxp`.

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes};
use crate::sstruct::eval;
use crate::table::{Table, TableType};
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
/// The extents come from the outlines (`glyf`, `CFF `, `CFF2` or `VARC`), which are
/// not implemented yet; until they are, the header is left unchanged.
pub(crate) fn recalc_header(font: &mut Font, vertical: bool) -> Result<Option<HeaderRecalc>> {
	let metrics_tag = Tag::new(if vertical { b"vmtx" } else { b"hmtx" });
	if !font.contains(metrics_tag) {
		return Ok(None);
	}
	let _ = font;
	Ok(None)
}

/// `maxp.recalc`: needs the `glyf` implementation; a no-op until it exists.
pub(crate) fn recalc_maxp(_maxp: &mut Maxp, _font: &mut Font) -> Result<()> {
	Ok(())
}
