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
use fontoxide_pens::{Pen, TransformPen};

use crate::cff::charstring::Blender;
use crate::cff::font::{CffFontSet, DeferComponents, run_char_string};
use crate::generated::ot;
use crate::glyph_order::GlyphOrder;
use crate::ot::varc::{NO_VARIATION_INDEX, VarComponent, flags};
use crate::table::TableType;
use crate::tables::glyf::{ComponentPlacement, Glyph, Number};
use crate::tables::metrics::{Hmtx, Metric, Vmtx};
use crate::tables::otl::Varc;
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

/// The glyphs of a font, drawable by name (`_TTGlyphSet` and its subclasses).
pub struct GlyphSet<'f> {
	order: Arc<GlyphOrder>,
	axes: Vec<String>,
	h_metrics: &'f BTreeMap<u32, Metric>,
	v_metrics: Option<&'f BTreeMap<u32, Metric>>,
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

const HMTX: Tag = Tag::new(b"hmtx");
const VMTX: Tag = Tag::new(b"vmtx");
const FVAR: Tag = Tag::new(b"fvar");
const GVAR: Tag = Tag::new(b"gvar");
const VARC: Tag = Tag::new(b"VARC");

impl<'f> GlyphSet<'f> {
	/// The glyph set of a font at its default location
	/// (`TTFont.getGlyphSet()`): the CFF outlines if there are any, else those
	/// of `glyf`, and the variable composites of `VARC`.
	pub fn new(font: &'f mut Font) -> Result<Self> {
		let order = font.glyph_order()?;
		let cff = if font.contains(Cff2::TAG) {
			Some(font.require::<Cff2>()?.cff.clone())
		} else if font.contains(Cff::TAG) {
			Some(font.require::<Cff>()?.cff.clone())
		} else {
			None
		};
		if cff.is_none() {
			if !font.contains(Glyf::TAG) {
				return Err(Error::ttlib("Font contains no outlines"));
			}
			font.load(Glyf::TAG)?;
			font.load(GVAR)?;
		}
		for tag in [HMTX, VMTX, FVAR, VARC] {
			font.load(tag)?;
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
			.ok_or(Error::TableNotFound(HMTX))?
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
		Ok(GlyphSet {
			order,
			axes,
			h_metrics,
			v_metrics,
			outlines,
			varc,
			varc_index,
			state: RefCell::new(State::default()),
			original_location: Location::new(),
			nesting: Cell::new(0),
		})
	}

	/// The glyph set of a font at a normalized location
	/// (`TTFont.getGlyphSet(location=location, normalized=True)`).
	pub fn with_location(font: &'f mut Font, location: Location) -> Result<Self> {
		let mut glyph_set = Self::new(font)?;
		glyph_set.state.borrow_mut().location = location.clone();
		glyph_set.original_location = location;
		Ok(glyph_set)
	}

	/// The `glyf` table that this draws from, if it does.
	pub fn glyf_table(&self) -> Option<&'f Glyf> {
		match self.outlines {
			Outlines::Glyf { glyf, .. } => Some(glyf),
			Outlines::Cff { .. } => None,
		}
	}

	/// The glyph order.
	pub fn glyph_order(&self) -> &GlyphOrder {
		&self.order
	}

	/// Draws a glyph onto `pen` (`glyphSet[name].draw(pen)`).
	pub fn draw(&self, glyph_name: &str, pen: &mut dyn Pen) -> Result<()> {
		let gid = self.lookup(glyph_name)?;
		self.draw_gid(gid, pen, false)
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
	fn draw_gid(&self, gid: u32, pen: &mut dyn Pen, base: bool) -> Result<()> {
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
			_ => match &self.outlines {
				Outlines::Glyf { glyf, gvar } => self.draw_glyf(glyf, *gvar, gid, pen),
				Outlines::Cff { cff, var_store } => {
					self.draw_cff(cff, var_store.as_deref(), gid, pen)
				}
			},
		};
		self.nesting.set(nesting);
		result
	}

	// --- glyf (`_TTGlyphGlyf`)

	fn draw_glyf(
		&self,
		glyf: &Glyf,
		gvar: Option<&Gvar>,
		gid: u32,
		pen: &mut dyn Pen,
	) -> Result<()> {
		let mut lsb = self.metric(gid)?.1 as i64;
		let location_set = !self.state.borrow().location.is_empty();
		let instance;
		let glyph = match gvar {
			Some(gvar) if location_set => {
				let (g, new_lsb) = self.glyf_instance(glyf, gvar, gid)?;
				instance = g;
				lsb = new_lsb;
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
		let result = glyph.draw(&self.order, pen, offset);
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

	/// The glyph at the current location, and its left side bearing
	/// (`_getGlyphInstance`).
	fn glyf_instance(&self, glyf: &Glyf, gvar: &Gvar, gid: u32) -> Result<(Glyph, i64)> {
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

	fn draw_varc(&self, gid: u32, index: usize, pen: &mut dyn Pen) -> Result<()> {
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
			let mut comp_location = Location::new();
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
					comp_location.insert(tag.clone(), v);
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
			// `pushLocation`.
			let saved = {
				let mut state = self.state.borrow_mut();
				let mut new = if reset {
					self.original_location.clone()
				} else {
					state.location.clone()
				};
				new.extend(comp_location);
				std::mem::replace(&mut state.location, new)
			};
			// Pens cannot take variable components (`addVarComponent`), so they
			// are always decomposed.
			let t = comp.transform.to_transform();
			let mut tpen = TransformPen::new(pen, t);
			let result = self.draw_component(comp.glyph, gid, &mut tpen);
			self.state.borrow_mut().location = saved;
			result?;
		}
		Ok(())
	}

	/// Draws a component of a variable composite glyph: from the base outlines
	/// if it is the glyph itself.
	fn draw_component(&self, comp_gid: u32, gid: u32, pen: &mut dyn Pen) -> Result<()> {
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
		self.draw_gid(gid, pen, false)
			.map_err(Error::into_pen_error)?;
		Ok(true)
	}
}

/// A copy of `glyph` with new coordinates, including the phantom points, and its
/// left side bearing (`_setCoordinates`).
///
/// Only the bounds of simple glyphs are recalculated: those of composite glyphs
/// only matter for their side bearing, which drawing does not use.
fn set_coordinates(glyph: &Glyph, mut coords: Vec<(f64, f64)>) -> Result<(Glyph, i64)> {
	let n = coords.len();
	if n < 4 {
		return Err(Error::Assertion("len(coord) >= 4".into()));
	}
	let left = coords[n - 4].0;
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
		glyph.x_min = 0;
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
	let lsb = ot_round(glyph.x_min as f64 - left);
	Ok((glyph, lsb))
}
