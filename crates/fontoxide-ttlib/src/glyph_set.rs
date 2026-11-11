//! Drawing the glyphs of a font (`fontTools.ttLib.ttGlyphSet`).
//!
//! A [`GlyphSet`] draws glyphs from `glyf` (with `gvar`), `CFF ` or `CFF2`, and
//! the variable composites of `VARC` on top of them, at a location. It is what
//! `TTFont.getGlyphSet()` returns.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use fontoxide_misc::array_tools::{Point, calc_bounds};
use fontoxide_misc::round_tools::ot_round;
use fontoxide_pens::{Pen, PointPen, SegmentToPointPen, TransformPen, TransformPointPen};

use crate::cff::charstring::Blender;
use crate::cff::font::{CffFontSet, DeferComponents, run_char_string};
use crate::generated::ot;
use crate::glyph_order::GlyphOrder;
use crate::ot::varc::{NO_VARIATION_INDEX, VarComponent, flags};
use crate::table::TableType;
use crate::tables::glyf::{ComponentPlacement, Glyph, Number};
use crate::tables::metrics::{Hmtx, Metric, Vmtx};
use crate::tables::otl::{Hvar, Varc, Vvar};
use crate::tables::tuple_variation::Delta;
use crate::tables::{Cff, Cff2, Fvar, Glyf, Gvar};
use crate::var::{Location, MultiVarStoreInstancer, VarStoreInstancer, iup_delta, support_scalar};
use crate::{Error, Font, Result, Tag};

/// The nesting limit for drawing components, in place of Python's recursion
/// limit.
const MAX_NESTING: usize = 100;

/// Where the outlines come from.
enum Outlines<'f> {
	Glyf {
		glyf: &'f Glyf,
		gvar: Option<&'f Gvar>,
	},
	/// A copy of the font set, since drawing decompiles charstrings, and of its
	/// variation store, for blending.
	Cff {
		cff: RefCell<CffFontSet>,
		var_store: Option<Box<ot::VarStore>>,
	},
}

/// The location being drawn, changed while drawing variable components.
#[derive(Debug, Default)]
struct State {
	location: Location,
	/// How deep `glyf` glyphs are being drawn (`depth`): the offset to the
	/// left side bearing only applies at the top.
	glyf_depth: usize,
}

/// The pen a glyph is drawn onto: `draw` or `drawPoints`.
enum DrawPen<'p> {
	Segment(&'p mut dyn Pen),
	Point(&'p mut dyn PointPen),
}

/// Whether a pen error is an `AttributeError`, which fontTools catches around
/// `addVarComponent` to decompose the component instead.
fn is_attribute_error(e: &fontoxide_pens::Error) -> bool {
	matches!(e, fontoxide_pens::Error::Usage(m) if m.starts_with("AttributeError"))
}

/// The metrics of a glyph in a [`GlyphSet`] (the `width`, `lsb`, `height` and
/// `tsb` attributes of `_TTGlyph`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphMetrics {
	/// The advance width: from `hmtx`, plus the `HVAR` delta at the location.
	/// Without `HVAR`, a `glyf` glyph at a location gets the width of its
	/// `gvar` phantom points.
	pub width: f64,
	/// The left side bearing.
	pub lsb: f64,
	/// The advance height, if the font has `vmtx`: from `vmtx`, plus the `VVAR`
	/// delta at the location.
	pub height: Option<f64>,
	/// The top side bearing, if the font has `vmtx`.
	pub tsb: Option<f64>,
}

/// The glyphs of a font, drawable by name (`_TTGlyphSet` and its subclasses).
pub struct GlyphSet<'f> {
	order: Arc<GlyphOrder>,
	axes: Vec<String>,
	h_metrics: &'f BTreeMap<u32, Metric>,
	v_metrics: Option<&'f BTreeMap<u32, Metric>>,
	/// `HVAR` and `VVAR`, which only apply at a location (`hvarTable`,
	/// `vvarTable`).
	hvar: Option<&'f ot::HVAR>,
	vvar: Option<&'f ot::VVAR>,
	outlines: Outlines<'f>,
	varc: Option<&'f ot::VARC>,
	/// The coverage index of each glyph of `VARC`.
	varc_index: HashMap<u32, usize>,
	state: RefCell<State>,
	/// The location of the glyph set (`originalLocation`).
	original_location: Location,
	nesting: Cell<usize>,
}

impl std::fmt::Debug for GlyphSet<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("GlyphSet")
			.field("axes", &self.axes)
			.field("varc", &self.varc.is_some())
			.field("state", &self.state)
			.finish_non_exhaustive()
	}
}

const FVAR: Tag = Tag::new(b"fvar");
const VARC: Tag = Tag::new(b"VARC");

impl<'f> GlyphSet<'f> {
	/// The glyph set of a font at its default location
	/// (`TTFont.getGlyphSet()`): the CFF outlines if there are any, else those
	/// of `glyf`, and the variable composites of `VARC`.
	pub fn new(font: &'f mut Font) -> Result<Self> {
		Self::build(font, Location::new())
	}

	/// The glyph set of a font at a location in user coordinates
	/// (`TTFont.getGlyphSet(location=location)`), normalized with
	/// [`Font::normalize_location`]. The location is ignored if it is empty or the
	/// font has no `fvar`.
	pub fn with_user_location(
		font: &'f mut Font,
		location: &BTreeMap<String, f64>,
	) -> Result<Self> {
		if location.is_empty() || !font.contains(FVAR) {
			return Self::new(font);
		}
		let location = font.normalize_location(location)?;
		Self::build(font, location)
	}

	/// The glyph set of a font at a normalized location
	/// (`TTFont.getGlyphSet(location=location, normalized=True)`). The location
	/// is ignored if the font has no `fvar`.
	pub fn with_location(font: &'f mut Font, location: Location) -> Result<Self> {
		let location = if font.contains(FVAR) {
			location
		} else {
			Location::new()
		};
		Self::build(font, location)
	}

	fn build(font: &'f mut Font, location: Location) -> Result<Self> {
		let order = font.glyph_order()?;
		let cff = if font.contains(Cff2::TAG) {
			Some(font.require::<Cff2>()?.cff.clone())
		} else if font.contains(Cff::TAG) {
			Some(font.require::<Cff>()?.cff.clone())
		} else {
			None
		};
		if cff.is_none() {
			if !font.has::<Glyf>() {
				return Err(Error::ttlib("Font contains no outlines"));
			}
			font.load(font.family_tag::<Glyf>())?;
			font.load(font.family_tag::<Gvar>())?;
		}
		for tag in [
			font.family_tag::<Hmtx>(),
			font.family_tag::<Vmtx>(),
			FVAR,
			VARC,
		] {
			font.load(tag)?;
		}
		if !location.is_empty() {
			font.load(Hvar::TAG)?;
			font.load(Vvar::TAG)?;
		}
		let gvar_axis_count = match font.get::<Gvar>()? {
			Some(gvar) => gvar.axis_count,
			None => 0,
		};
		let font: &'f Font = font;

		let var_store = cff
			.as_ref()
			.and_then(|c| c.top_dicts.first())
			.and_then(|t| t.var_store.clone());
		let axes = match font.loaded::<Fvar>() {
			Some(fvar) => fvar.axes.iter().map(|a| a.tag_text()).collect(),
			None => {
				// VARC allows static fonts to keep the variations of `gvar` or
				// `CFF2` for the coordinates of components, without `fvar` axes.
				let mut count = gvar_axis_count.max(0) as usize;
				if let Some(regions) = var_store.as_ref().and_then(|s| s.var_region_list.as_ref()) {
					let n = regions.region_axis_count.map_or_else(
						|| {
							regions
								.region
								.first()
								.map_or(0, |r| r.var_region_axis.len())
						},
						|n| n as usize,
					);
					count = count.max(n);
				}
				(0..count).map(|i| i.to_string()).collect()
			}
		};
		let h_metrics = &font
			.loaded::<Hmtx>()
			.ok_or(Error::TableNotFound(Hmtx::TAG))?
			.metrics;
		let v_metrics = font.loaded::<Vmtx>().map(|t| &t.metrics);
		let outlines = match cff {
			Some(cff) => Outlines::Cff {
				cff: RefCell::new(cff),
				var_store,
			},
			None => Outlines::Glyf {
				glyf: font
					.loaded::<Glyf>()
					.ok_or(Error::TableNotFound(Glyf::TAG))?,
				gvar: font.loaded::<Gvar>(),
			},
		};
		let varc = font.loaded::<Varc>().map(|t| &t.table);
		let mut varc_index = HashMap::new();
		if let Some(coverage) = varc.and_then(|v| v.coverage.as_deref()) {
			for (i, &gid) in coverage.glyphs.iter().enumerate() {
				varc_index.entry(gid).or_insert(i);
			}
		}
		let (hvar, vvar) = if location.is_empty() {
			(None, None)
		} else {
			(
				font.loaded::<Hvar>().map(|t| &t.table),
				font.loaded::<Vvar>().map(|t| &t.table),
			)
		};
		Ok(GlyphSet {
			order,
			axes,
			h_metrics,
			v_metrics,
			hvar,
			vvar,
			outlines,
			varc,
			varc_index,
			state: RefCell::new(State {
				location: location.clone(),
				glyf_depth: 0,
			}),
			original_location: location,
			nesting: Cell::new(0),
		})
	}

	/// The names of the glyphs, in glyph order (`keys()`).
	pub fn keys(&self) -> &[String] {
		self.order.names()
	}

	/// The number of glyphs (`len(glyphSet)`).
	pub fn len(&self) -> usize {
		self.order.len()
	}

	/// Whether the glyph set is empty.
	pub fn is_empty(&self) -> bool {
		self.order.is_empty()
	}

	/// Whether there is a glyph of that name (`glyphName in glyphSet`).
	pub fn contains(&self, glyph_name: &str) -> bool {
		self.order.reverse_map().contains_key(glyph_name)
	}

	/// The current normalized location (`location`): that of the glyph set,
	/// unless changed by [`GlyphSet::push_location`].
	pub fn location(&self) -> Location {
		self.state.borrow().location.clone()
	}

	/// The variation axes (`axes`): the tags of `fvar`, or, without it, the
	/// indices of the axes of `gvar` or `CFF2`, as strings.
	pub fn axes(&self) -> &[String] {
		&self.axes
	}

	/// Runs `f` at the current location updated with `location`, or with the
	/// location of the glyph set updated with it if `reset` is set
	/// (`pushLocation`), and restores the current location afterwards.
	pub fn push_location<R>(&self, location: &Location, reset: bool, f: impl FnOnce() -> R) -> R {
		let saved = {
			let mut state = self.state.borrow_mut();
			let mut new = if reset {
				self.original_location.clone()
			} else {
				state.location.clone()
			};
			new.extend(location.iter().map(|(k, v)| (k.clone(), *v)));
			std::mem::replace(&mut state.location, new)
		};
		let result = f();
		self.state.borrow_mut().location = saved;
		result
	}

	/// The metrics of a glyph (`glyphSet[glyphName].width` and so on), at the
	/// current location.
	///
	/// When the width of a `glyf` glyph comes from its phantom points (at a
	/// location, without `HVAR`), the other metrics are also those of the
	/// glyph instance, as fontTools has them after reading `width`. Otherwise
	/// the side bearings are those of `hmtx` and `vmtx`, as fontTools has them
	/// before the glyph is drawn.
	pub fn metrics(&self, glyph_name: &str) -> Result<GlyphMetrics> {
		let gid = self.lookup(glyph_name)?;
		let (width, lsb) = self.metric(gid)?;
		let mut metrics = GlyphMetrics {
			width: f64::from(width),
			lsb: f64::from(lsb),
			height: None,
			tsb: None,
		};
		if let Some(v) = self.v_metrics {
			let (height, tsb) = v.get(&gid).copied().ok_or_else(|| self.key_error(gid))?;
			metrics.height = Some(f64::from(height));
			metrics.tsb = Some(f64::from(tsb));
		}
		let location_set = !self.state.borrow().location.is_empty();
		if location_set
			&& let Some(hvar) = self.hvar
			&& let Some(var_idx) = self.advance_var_index(hvar.adv_width_map.as_deref(), gid)?
		{
			let instancer = VarStoreInstancer::new(
				hvar.var_store.as_deref(),
				&self.axes,
				self.original_location.clone(),
			);
			metrics.width += instancer.get(var_idx)?;
		}
		if let (Some(height), Some(vvar)) = (&mut metrics.height, self.vvar)
			&& let Some(var_idx) = self.advance_var_index(vvar.adv_height_map.as_deref(), gid)?
		{
			let instancer = VarStoreInstancer::new(
				vvar.var_store.as_deref(),
				&self.axes,
				self.original_location.clone(),
			);
			*height += instancer.get(var_idx)?;
		}
		// `_TTGlyphGlyf.width`, without `HVAR`: from the phantom points, if the
		// glyph varies.
		if location_set
			&& self.hvar.is_none()
			&& !self.varc_index.contains_key(&gid)
			&& let Outlines::Glyf {
				glyf,
				gvar: Some(gvar),
			} = &self.outlines
			&& gvar.get(gid).is_some()
		{
			let instance = self.glyf_instance(glyf, gvar, gid)?;
			metrics.width = instance.width as f64;
			metrics.lsb = instance.lsb as f64;
			metrics.tsb = Some(instance.tsb as f64);
			if self.vvar.is_none() {
				metrics.height = Some(instance.height as f64);
			}
		}
		Ok(metrics)
	}

	/// The variation index of the advance of a glyph in `HVAR` or `VVAR`: from
	/// the map if there is one, else the glyph ID, which only addresses the
	/// first item variation data (an implicit mapping).
	fn advance_var_index(
		&self,
		map: Option<&crate::ot::varmap::VarIdxMap>,
		gid: u32,
	) -> Result<Option<u64>> {
		match map {
			Some(map) => map
				.mapping
				.get(&gid)
				.copied()
				.map(Some)
				.ok_or_else(|| self.key_error(gid)),
			None => Ok((gid <= 0xFFFF).then_some(u64::from(gid))),
		}
	}

	fn key_error(&self, gid: u32) -> Error {
		Error::ttlib(format!("KeyError: {:?}", self.order.name(gid)))
	}

	/// The `glyf` table that this draws from, if it does.
	pub fn glyf_table(&self) -> Option<&'f Glyf> {
		match self.outlines {
			Outlines::Glyf { glyf, .. } => Some(glyf),
			Outlines::Cff { .. } => None,
		}
	}

	/// The `gvar` table that this draws from, if it does (`gvarTable`).
	pub fn gvar_table(&self) -> Option<&'f Gvar> {
		match self.outlines {
			Outlines::Glyf { gvar, .. } => gvar,
			Outlines::Cff { .. } => None,
		}
	}

	/// The horizontal metrics by glyph ID (`hMetrics`).
	pub fn h_metrics(&self) -> &'f BTreeMap<u32, Metric> {
		self.h_metrics
	}

	/// The glyph order.
	pub fn glyph_order(&self) -> &GlyphOrder {
		&self.order
	}

	/// Draws a glyph onto `pen` (`glyphSet[name].draw(pen)`).
	pub fn draw(&self, glyph_name: &str, pen: &mut dyn Pen) -> Result<()> {
		let gid = self.lookup(glyph_name)?;
		self.draw_gid(gid, DrawPen::Segment(pen), false)
	}

	/// Draws a glyph onto a point pen (`glyphSet[name].drawPoints(pen)`).
	pub fn draw_points(&self, glyph_name: &str, pen: &mut dyn PointPen) -> Result<()> {
		let gid = self.lookup(glyph_name)?;
		self.draw_gid(gid, DrawPen::Point(pen), false)
	}

	/// The glyph ID of a glyph that can be drawn; a `KeyError` if it has no
	/// metrics (creating `_TTGlyph`).
	fn lookup(&self, glyph_name: &str) -> Result<u32> {
		let key_error = || Error::ttlib(format!("KeyError: {glyph_name:?}"));
		let gid = self.order.id(glyph_name).ok_or_else(key_error)?;
		if !self.h_metrics.contains_key(&gid)
			|| self.v_metrics.is_some_and(|m| !m.contains_key(&gid))
		{
			return Err(key_error());
		}
		Ok(gid)
	}

	/// The horizontal metrics of a glyph.
	fn metric(&self, gid: u32) -> Result<Metric> {
		self.h_metrics
			.get(&gid)
			.copied()
			.ok_or_else(|| Error::ttlib(format!("KeyError: {:?}", self.order.name(gid))))
	}

	/// Draws a glyph; with `base`, its outline even if it is a variable
	/// composite.
	fn draw_gid(&self, gid: u32, pen: DrawPen<'_>, base: bool) -> Result<()> {
		let nesting = self.nesting.get();
		if nesting >= MAX_NESTING {
			// LENIENT(L18): fontTools runs into Python's recursion limit.
			return Err(Error::ttlib(
				"RecursionError: maximum recursion depth exceeded",
			));
		}
		self.nesting.set(nesting + 1);
		let result = match (base, self.varc_index.get(&gid)) {
			(false, Some(&index)) => self.draw_varc(gid, index, pen),
			_ => match (&self.outlines, pen) {
				(Outlines::Glyf { glyf, gvar }, DrawPen::Segment(pen)) => {
					self.with_glyf_glyph(glyf, *gvar, gid, &mut |glyph, order, offset| {
						glyph.draw(order, pen, offset)
					})
				}
				(Outlines::Glyf { glyf, gvar }, DrawPen::Point(pen)) => {
					self.with_glyf_glyph(glyf, *gvar, gid, &mut |glyph, order, offset| {
						glyph.draw_points(order, pen, offset)
					})
				}
				(Outlines::Cff { cff, var_store }, DrawPen::Segment(pen)) => {
					self.draw_cff(cff, var_store.as_deref(), gid, pen)
				}
				// `_TTGlyph.drawPoints`.
				(Outlines::Cff { cff, var_store }, DrawPen::Point(pen)) => {
					let mut seg = SegmentToPointPen::new(pen, true);
					self.draw_cff(cff, var_store.as_deref(), gid, &mut seg)
				}
			},
		};
		self.nesting.set(nesting);
		result
	}

	// --- glyf (`_TTGlyphGlyf`)

	/// The glyph to draw and its offset (`_getGlyphAndOffset`), passed to
	/// `draw`, with the nesting depth kept as `pushDepth` does.
	fn with_glyf_glyph(
		&self,
		glyf: &Glyf,
		gvar: Option<&Gvar>,
		gid: u32,
		draw: &mut dyn FnMut(&Glyph, &GlyphOrder, f64) -> Result<()>,
	) -> Result<()> {
		let mut lsb = self.metric(gid)?.1 as i64;
		let location_set = !self.state.borrow().location.is_empty();
		let instance;
		let glyph = match gvar {
			Some(gvar) if location_set => {
				let i = self.glyf_instance(glyf, gvar, gid)?;
				lsb = i.lsb;
				instance = i.glyph;
				&instance
			}
			_ => glyf
				.glyphs
				.get(&gid)
				.ok_or_else(|| Error::ttlib(format!("KeyError: {:?}", self.order.name(gid))))?,
		};
		let depth = {
			let mut state = self.state.borrow_mut();
			state.glyf_depth += 1;
			state.glyf_depth - 1
		};
		// Only simple glyphs use the offset; composite and empty ones ignore it.
		let offset = if depth == 0 && glyph.number_of_contours > 0 {
			lsb as f64 - glyph.x_min as f64
		} else {
			0.0
		};
		let result = draw(glyph, &self.order, offset);
		self.state.borrow_mut().glyf_depth -= 1;
		result
	}

	/// The points of a glyph for `gvar`, with the four phantom points
	/// (`_getCoordinatesAndControls`), and the contour ends for IUP.
	fn coordinates_and_ends(&self, glyph: &Glyph, gid: u32) -> Result<(Vec<Point>, Vec<usize>)> {
		let (mut coords, ends): (Vec<(f64, f64)>, Vec<usize>) = if glyph.is_composite() {
			let coords = glyph
				.components
				.iter()
				.map(|c| match c.placement {
					ComponentPlacement::Offset { x, y } => (x.value(), y.value()),
					ComponentPlacement::Points { .. } => (0.0, 0.0),
				})
				.collect::<Vec<_>>();
			let ends = (0..coords.len()).collect();
			(coords, ends)
		} else if glyph.number_of_contours > 0 {
			let ends = glyph
				.end_pts_of_contours
				.iter()
				.map(|&e| usize::try_from(e).unwrap_or(0))
				.collect();
			(glyph.coordinates.clone(), ends)
		} else {
			(Vec::new(), Vec::new())
		};
		// `_getPhantomPoints`; an empty glyph has no bounds, which are then
		// calculated as (0, 0, 0, 0).
		let empty = glyph.number_of_contours == 0;
		let (x_min, y_max) = if empty {
			(0, 0)
		} else {
			(glyph.x_min, glyph.y_max)
		};
		let (advance, lsb) = self.metric(gid)?;
		let left = x_min as f64 - lsb as f64;
		let right = left + advance as f64;
		let (top, bottom) = match self.v_metrics {
			Some(v) if !v.is_empty() => {
				let (advance, tsb) = v
					.get(&gid)
					.copied()
					.ok_or_else(|| Error::ttlib(format!("KeyError: {:?}", self.order.name(gid))))?;
				let top = tsb as f64 + y_max as f64;
				(top, top - advance as f64)
			}
			_ => (0.0, 0.0),
		};
		coords.extend([(left, 0.0), (right, 0.0), (0.0, top), (0.0, bottom)]);
		for p in &mut coords {
			*p = (ot_round(p.0) as f64, ot_round(p.1) as f64);
		}
		Ok((coords, ends))
	}

	/// The glyph at the current location, and its metrics
	/// (`_getGlyphInstance`).
	fn glyf_instance(&self, glyf: &Glyf, gvar: &Gvar, gid: u32) -> Result<Instance> {
		let name = || self.order.name(gid);
		let variations = gvar
			.get(gid)
			.ok_or_else(|| Error::ttlib(format!("KeyError: {:?}", name())))?;
		let glyph = glyf.glyphs.get(&gid).ok_or_else(|| {
			Error::ttlib(format!(
				"TypeError: cannot unpack non-iterable NoneType object ({:?})",
				name()
			))
		})?;
		let (mut coordinates, ends) = self.coordinates_and_ends(glyph, gid)?;
		let original = coordinates.clone();
		let location = self.state.borrow().location.clone();
		for var in variations {
			// The axes of the variation, in the order of the font's axes.
			let mut support: Vec<(&str, (f64, f64, f64))> = self
				.axes
				.iter()
				.filter_map(|a| var.axes.get(a).map(|r| (a.as_str(), *r)))
				.collect();
			for (a, r) in &var.axes {
				if !self.axes.contains(a) {
					support.push((a.as_str(), *r));
				}
			}
			let scalar = support_scalar(&location, support);
			if scalar == 0.0 {
				continue;
			}
			let mut deltas = Vec::with_capacity(var.coordinates.len());
			for d in &var.coordinates {
				deltas.push(match d {
					None => None,
					Some(Delta::Point(x, y)) => Some((x.value(), y.value())),
					Some(Delta::Value(_)) => {
						return Err(Error::ttlib("TypeError: a 'gvar' delta is not a point"));
					}
				});
			}
			let deltas: Vec<(f64, f64)> = if deltas.iter().any(Option::is_none) {
				iup_delta(&deltas, &original, &ends)?
			} else {
				deltas.into_iter().flatten().collect()
			};
			if deltas.len() != coordinates.len() {
				return Err(Error::Assertion("len(a) == len(other)".into()));
			}
			for (c, d) in coordinates.iter_mut().zip(deltas) {
				// `coordinates += GlyphCoordinates(delta) * scalar`.
				let (dx, dy) = if scalar == 1.0 {
					d
				} else {
					(d.0 * scalar, d.1 * scalar)
				};
				c.0 += dx;
				c.1 += dy;
			}
		}
		set_coordinates(glyph, coordinates)
	}

	// --- CFF (`_TTGlyphCFF`)

	fn draw_cff(
		&self,
		cff: &RefCell<CffFontSet>,
		var_store: Option<&ot::VarStore>,
		gid: u32,
		pen: &mut dyn Pen,
	) -> Result<()> {
		let location = self.state.borrow().location.clone();
		let instancer = match var_store {
			Some(store) if !location.is_empty() => {
				Some(VarStoreInstancer::new(Some(store), &self.axes, location))
			}
			_ => None,
		};
		let blend = |vs_index: i64, deltas: &[f64]| -> Result<f64> {
			instancer
				.as_ref()
				.ok_or_else(|| Error::ttlib("no blender"))?
				.interpolate_from_deltas(vs_index, deltas)
		};
		let blender: Option<Blender<'_>> = match instancer {
			Some(_) => Some(&blend),
			None => None,
		};
		let mut deferred = DeferComponents {
			out: pen,
			components: Vec::new(),
		};
		{
			let mut cff = cff.borrow_mut();
			let CffFontSet {
				top_dicts,
				global_subrs,
				..
			} = &mut *cff;
			let top = top_dicts
				.first_mut()
				.ok_or_else(crate::cff::charstring::index_error)?;
			let name = self.order.name(gid);
			let index = top
				.char_strings
				.as_ref()
				.and_then(|c| c.index(&name))
				.ok_or_else(|| Error::ttlib(format!("KeyError: {name:?}")))?;
			run_char_string(top, global_subrs, index, Some(&mut deferred), blender)?;
		}
		let DeferComponents { out, components } = deferred;
		for (name, t) in components {
			out.add_component(&name, t)?;
		}
		Ok(())
	}

	// --- VARC (`_TTGlyphVARC`)

	fn draw_varc(&self, gid: u32, index: usize, mut pen: DrawPen<'_>) -> Result<()> {
		let varc = self.varc.ok_or_else(|| Error::ttlib("no VARC"))?;
		let none = |what: &str| {
			Error::ttlib(format!(
				"AttributeError: 'NoneType' object has no attribute '{what}'"
			))
		};
		let index_error = || Error::ttlib("IndexError: list index out of range");
		let glyph = varc
			.var_composite_glyphs
			.as_deref()
			.ok_or_else(|| none("VarCompositeGlyph"))?
			.var_composite_glyph
			.get(index)
			.ok_or_else(index_error)?;
		let location = self.state.borrow().location.clone();
		let instancer = MultiVarStoreInstancer::new(
			varc.multi_var_store.as_deref(),
			&self.axes,
			location.clone(),
		);
		for comp in &glyph.components {
			if let Some(ci) = comp.condition_index {
				let conditions = varc
					.condition_list
					.as_deref()
					.ok_or_else(|| none("ConditionTable"))?;
				let condition = conditions
					.condition_table
					.get(ci as usize)
					.ok_or_else(index_error)?;
				if !self.evaluate_condition(condition.as_deref(), &location, &instancer)? {
					continue;
				}
			}
			// The location of the component, in the order of its axes
			// (`rawLocation`, a `dict`).
			let mut raw_location: fontoxide_pens::Location = Vec::new();
			if let Some(ai) = comp.axis_indices_index {
				let indices = varc
					.axis_indices_list
					.as_deref()
					.ok_or_else(|| none("Item"))?
					.item
					.get(ai as usize)
					.ok_or_else(index_error)?;
				let mut values = comp.axis_values.clone();
				if comp.axis_values_var_index != NO_VARIATION_INDEX {
					let deltas = instancer.get(comp.axis_values_var_index)?;
					if deltas.len() != values.len() {
						return Err(Error::Assertion("len(self) == len(other)".into()));
					}
					for (v, d) in values.iter_mut().zip(deltas) {
						*v += d / 16384.0;
					}
				}
				if indices.len() != values.len() {
					return Err(Error::Assertion(format!(
						"({}, {})",
						indices.len(),
						values.len()
					)));
				}
				for (&i, v) in indices.iter().zip(values) {
					let tag = usize::try_from(i)
						.ok()
						.and_then(|i| self.axes.get(i))
						.ok_or_else(index_error)?;
					match raw_location.iter_mut().find(|(t, _)| t == tag) {
						Some(entry) => entry.1 = v,
						None => raw_location.push((tag.clone(), v)),
					}
				}
			}
			let transformed;
			let comp = if comp.transform_var_index != NO_VARIATION_INDEX {
				let deltas = instancer.get(comp.transform_var_index)?;
				let mut c: VarComponent = comp.clone();
				c.apply_transform_deltas(&deltas)?;
				transformed = c;
				&transformed
			} else {
				comp
			};
			let reset = comp.flags & flags::RESET_UNSPECIFIED_AXES != 0;
			let comp_location: Location = raw_location.iter().cloned().collect();
			self.push_location(&comp_location, reset, || {
				// The component is passed on to pens that take variable
				// components, and decomposed for the others.
				if comp.glyph != gid {
					let name = self.order.name(comp.glyph);
					let added = match &mut pen {
						DrawPen::Segment(pen) => {
							pen.add_var_component(&name, comp.transform, &raw_location, reset)
						}
						// The flag cannot be passed: the point pens of
						// `fontoxide_pens` take no `resetUnspecifiedAxes` argument.
						DrawPen::Point(pen) => {
							pen.add_var_component(&name, comp.transform, &raw_location, None)
						}
					};
					match added {
						Ok(()) => return Ok(()),
						Err(e) if is_attribute_error(&e) => {}
						Err(e) => return Err(e.into()),
					}
				}
				let t = comp.transform.to_transform();
				match &mut pen {
					DrawPen::Segment(pen) => {
						let mut tpen = TransformPen::new(&mut **pen, t);
						self.draw_component(comp.glyph, gid, DrawPen::Segment(&mut tpen))
					}
					DrawPen::Point(pen) => {
						let mut tpen = TransformPointPen::new(&mut **pen, t);
						self.draw_component(comp.glyph, gid, DrawPen::Point(&mut tpen))
					}
				}
			})?;
		}
		Ok(())
	}

	/// Draws a component of a variable composite glyph: from the base outlines
	/// if it is the glyph itself.
	fn draw_component(&self, comp_gid: u32, gid: u32, pen: DrawPen<'_>) -> Result<()> {
		let name = self.order.name(comp_gid);
		self.lookup(&name)?;
		self.draw_gid(comp_gid, pen, comp_gid == gid)
	}

	/// `_evaluateCondition`.
	fn evaluate_condition(
		&self,
		condition: Option<&ot::ConditionTable>,
		location: &Location,
		instancer: &MultiVarStoreInstancer<'_>,
	) -> Result<bool> {
		let Some(condition) = condition else {
			// A null condition offset denotes True.
			return Ok(true);
		};
		Ok(match condition {
			ot::ConditionTable::Format1(c) => {
				let tag = self
					.axes
					.get(c.axis_index as usize)
					.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
				let v = location.get(tag).copied().unwrap_or(0.0);
				c.filter_range_min_value <= v && v <= c.filter_range_max_value
			}
			ot::ConditionTable::Format2(c) => {
				let mut value = c.default_value as f64;
				if c.var_idx as u64 != NO_VARIATION_INDEX {
					let deltas = instancer.get(c.var_idx as u64)?;
					if let Some(d) = deltas.first() {
						value += d;
					}
				}
				value > 0.0
			}
			ot::ConditionTable::Format3(c) => {
				for sub in &c.condition_table {
					if !self.evaluate_condition(sub.as_deref(), location, instancer)? {
						return Ok(false);
					}
				}
				true
			}
			ot::ConditionTable::Format4(c) => {
				for sub in &c.condition_table {
					if self.evaluate_condition(sub.as_deref(), location, instancer)? {
						return Ok(true);
					}
				}
				false
			}
			ot::ConditionTable::Format5(c) => {
				!self.evaluate_condition(c.condition_table.as_deref(), location, instancer)?
			}
			ot::ConditionTable::Unknown(_) => false,
		})
	}
}

impl fontoxide_pens::GlyphSet for GlyphSet<'_> {
	fn draw_glyph(&self, glyph_name: &str, pen: &mut dyn Pen) -> fontoxide_pens::Result<bool> {
		let Ok(gid) = self.lookup(glyph_name) else {
			return Ok(false);
		};
		self.draw_gid(gid, DrawPen::Segment(pen), false)
			.map_err(Error::into_pen_error)?;
		Ok(true)
	}

	/// `drawPoints`: `glyf` glyphs draw their points as they are
	/// (`_TTGlyphGlyf`), variable composites their components
	/// (`_TTGlyphVARC`); CFF glyphs are drawn through a `SegmentToPointPen`
	/// (`_TTGlyph`).
	fn draw_glyph_points(
		&self,
		glyph_name: &str,
		pen: &mut dyn PointPen,
	) -> fontoxide_pens::Result<bool> {
		let Ok(gid) = self.lookup(glyph_name) else {
			return Ok(false);
		};
		self.draw_gid(gid, DrawPen::Point(pen), false)
			.map_err(Error::into_pen_error)?;
		Ok(true)
	}

	fn contains(&self, glyph_name: &str) -> bool {
		self.lookup(glyph_name).is_ok()
	}
}

/// A glyph set that interpolates between two others (`LerpGlyphSet`): 0 is the
/// first, 1 the second; other factors extrapolate. Glyphs are drawn decomposed.
pub struct LerpGlyphSet<'a> {
	/// The first glyph set (`glyphset1`).
	pub glyph_set1: &'a dyn fontoxide_pens::GlyphSet,
	/// The second glyph set (`glyphset2`).
	pub glyph_set2: &'a dyn fontoxide_pens::GlyphSet,
	/// The interpolation factor (`factor`; 0.5 in fontTools).
	pub factor: f64,
}

impl std::fmt::Debug for LerpGlyphSet<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("LerpGlyphSet")
			.field("factor", &self.factor)
			.finish_non_exhaustive()
	}
}

impl fontoxide_pens::GlyphSet for LerpGlyphSet<'_> {
	/// `LerpGlyph.draw`.
	fn draw_glyph(&self, glyph_name: &str, pen: &mut dyn Pen) -> fontoxide_pens::Result<bool> {
		if !self.contains(glyph_name) {
			return Ok(false);
		}
		let mut recording1 =
			fontoxide_pens::recording_pen::DecomposingRecordingPen::new(self.glyph_set1);
		self.glyph_set1.draw_glyph(glyph_name, &mut recording1)?;
		let mut recording2 =
			fontoxide_pens::recording_pen::DecomposingRecordingPen::new(self.glyph_set2);
		self.glyph_set2.draw_glyph(glyph_name, &mut recording2)?;
		let lerped = fontoxide_pens::recording_pen::lerp_recordings(
			recording1.value(),
			recording2.value(),
			self.factor,
		)?;
		fontoxide_pens::recording_pen::replay_recording(&lerped, pen)?;
		Ok(true)
	}

	fn contains(&self, glyph_name: &str) -> bool {
		self.glyph_set1.contains(glyph_name) && self.glyph_set2.contains(glyph_name)
	}
}

/// A `glyf` glyph at a location and its metrics, from the phantom points.
struct Instance {
	glyph: Glyph,
	width: i64,
	lsb: i64,
	height: i64,
	tsb: i64,
}

/// A copy of `glyph` with new coordinates, including the phantom points, and its
/// metrics (`_setCoordinates`).
///
/// Only the bounds of simple and empty glyphs are recalculated: those of
/// composite glyphs only matter for their side bearings, which drawing does
/// not use.
fn set_coordinates(glyph: &Glyph, mut coords: Vec<(f64, f64)>) -> Result<Instance> {
	let n = coords.len();
	if n < 4 {
		return Err(Error::Assertion("len(coord) >= 4".into()));
	}
	let left = coords[n - 4].0;
	let right = coords[n - 3].0;
	let top = coords[n - 2].1;
	let bottom = coords[n - 1].1;
	coords.truncate(n - 4);
	let mut glyph = glyph.clone();
	if glyph.is_composite() {
		if coords.len() != glyph.components.len() {
			return Err(Error::Assertion(
				"len(coord) == len(glyph.components)".into(),
			));
		}
		for (c, (x, y)) in glyph.components.iter_mut().zip(coords) {
			if let ComponentPlacement::Offset { .. } = c.placement {
				c.placement = ComponentPlacement::Offset {
					x: Number::Float(x),
					y: Number::Float(y),
				};
			}
		}
	} else if glyph.number_of_contours <= 0 {
		if !coords.is_empty() {
			return Err(Error::Assertion("len(coord) == 0".into()));
		}
		// The bounds of no points.
		glyph.x_min = 0;
		glyph.y_min = 0;
		glyph.x_max = 0;
		glyph.y_max = 0;
	} else {
		if coords.len() != glyph.coordinates.len() {
			return Err(Error::Assertion(
				"len(coord) == len(glyph.coordinates)".into(),
			));
		}
		let b = calc_bounds(&coords);
		glyph.x_min = ot_round(b[0]);
		glyph.y_min = ot_round(b[1]);
		glyph.x_max = ot_round(b[2]);
		glyph.y_max = ot_round(b[3]);
		glyph.coordinates = coords;
	}
	Ok(Instance {
		width: ot_round(right - left),
		lsb: ot_round(glyph.x_min as f64 - left),
		height: ot_round(top - bottom),
		tsb: ot_round(top - glyph.y_max as f64),
		glyph,
	})
}
