//! Simplifies glyphs by merging overlapping contours and components (a port of
//! `fontTools.ttLib.removeOverlaps`).
//!
//! fontTools uses skia-pathops for the boolean operations; here it is `linesweeper`,
//! which is pure Rust. The results are geometrically equivalent, not identical: the
//! contours can start at other points or come in another order, as they also do between
//! versions of Skia.

use fontoxide_misc::transform::Transform;
use fontoxide_pens::base_pen::{decompose_quadratic_segment, decompose_super_bezier_segment};
use fontoxide_pens::transform_pen::TransformPen;
use fontoxide_pens::{Pen, Point};
use kurbo::{BezPath, PathEl, Shape};
use linesweeper::topology::Contours;
use linesweeper::{BinaryOp, FillRule, binary_op};

use crate::cff::charstring::{T2CharString, Token};
use crate::cff::font::CffFontSet;
use crate::cff::specializer::{
	Arg, Command, SpecializeOptions, commands_to_program, specialize_commands,
};
use crate::glyph_set::GlyphSet;
use crate::table::TableType;
use crate::tables::glyf::Glyph;
use crate::tables::{Cff, Cff2, Glyf, Hmtx};
use crate::{Error, Font, Result, Tag};

const CFF: Tag = Tag::new(b"CFF ");
const CFF2: Tag = Tag::new(b"CFF2");

/// The options of [`remove_overlaps`].
#[derive(Debug, Clone)]
pub struct RemoveOverlapsOptions {
	/// The glyphs to remove overlaps from; by default, all (`glyphNames`).
	pub glyph_names: Option<Vec<String>>,
	/// Drop the hinting of all glyphs, whether or not overlaps are removed from them, as
	/// it would look weird if only some glyphs are left (un)hinted (`removeHinting`).
	pub remove_hinting: bool,
	/// Keep the tricky glyphs unchanged (`ignoreErrors`).
	pub ignore_errors: bool,
	/// Remove the unused subroutines of CFF if any glyphs are modified
	/// (`removeUnusedSubroutines`).
	pub remove_unused_subroutines: bool,
}

impl Default for RemoveOverlapsOptions {
	fn default() -> Self {
		RemoveOverlapsOptions {
			glyph_names: None,
			remove_hinting: true,
			ignore_errors: false,
			remove_unused_subroutines: true,
		}
	}
}

fn pen_error(e: fontoxide_pens::Error) -> Error {
	Error::from(e)
}

/// A pen collecting a path, with the components decomposed (`pathops.Path.getPen`).
struct PathPen<'a, 'f> {
	path: BezPath,
	glyph_set: &'a GlyphSet<'f>,
	current: Option<Point>,
	open: bool,
}

impl<'a, 'f> PathPen<'a, 'f> {
	fn new(glyph_set: &'a GlyphSet<'f>) -> Self {
		PathPen {
			path: BezPath::new(),
			glyph_set,
			current: None,
			open: false,
		}
	}

	fn finish(mut self) -> BezPath {
		if self.open {
			self.path.close_path();
		}
		self.path
	}
}

impl Pen for PathPen<'_, '_> {
	fn move_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		if self.open {
			self.path.close_path();
		}
		self.path.move_to(pt);
		self.current = Some(pt);
		self.open = true;
		Ok(())
	}

	fn line_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.path.line_to(pt);
		self.current = Some(pt);
		Ok(())
	}

	fn curve_to(&mut self, points: &[Point]) -> fontoxide_pens::Result<()> {
		for (a, b, c) in decompose_super_bezier_segment(points) {
			self.path.curve_to(a, b, c);
			self.current = Some(c);
		}
		Ok(())
	}

	fn q_curve_to(
		&mut self,
		off_curves: &[Point],
		on_curve: Option<Point>,
	) -> fontoxide_pens::Result<()> {
		let mut points = off_curves.to_vec();
		match on_curve {
			Some(p) => points.push(p),
			None => {
				// A contour of off-curve points only: it starts and ends halfway between
				// the last and the first.
				let (Some(&last), Some(&first)) = (off_curves.last(), off_curves.first()) else {
					return Ok(());
				};
				let start = ((last.0 + first.0) * 0.5, (last.1 + first.1) * 0.5);
				self.move_to(start)?;
				points.push(start);
			}
		}
		if points.len() == 1 {
			self.line_to(points[0])?;
			return Ok(());
		}
		for (a, b) in decompose_quadratic_segment(&points) {
			self.path.quad_to(a, b);
			self.current = Some(b);
		}
		Ok(())
	}

	fn close_path(&mut self) -> fontoxide_pens::Result<()> {
		self.path.close_path();
		self.open = false;
		Ok(())
	}

	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		// The boolean operations need closed contours.
		self.path.close_path();
		self.open = false;
		Ok(())
	}

	fn add_component(
		&mut self,
		glyph_name: &str,
		transformation: Transform,
	) -> fontoxide_pens::Result<()> {
		let glyph_set = self.glyph_set;
		let mut pen = TransformPen::new(self, transformation);
		glyph_set
			.draw(glyph_name, &mut pen)
			.map_err(|e| fontoxide_pens::Error::MissingComponent(e.to_string()))
	}
}

/// `skPathFromGlyph`.
fn path_from_glyph(name: &str, glyph_set: &GlyphSet<'_>) -> Result<BezPath> {
	let mut pen = PathPen::new(glyph_set);
	glyph_set.draw(name, &mut pen)?;
	Ok(pen.finish())
}

fn boolean_error(name: &str) -> Error {
	Error::ttlib(format!(
		"RemoveOverlapsError: Failed to remove overlaps from glyph {name:?}"
	))
}

/// `pathops.simplify(path, clockwise=path.clockwise)`: the contours of the union, with the
/// direction of the outer ones as that of the whole path, and alternating when nested.
fn simplify(path: &BezPath, name: &str) -> Result<Vec<BezPath>> {
	let contours: Contours = binary_op(path, &BezPath::new(), FillRule::NonZero, BinaryOp::Union)
		.map_err(|_| boolean_error(name))?;
	let clockwise = path.area() < 0.0;
	let mut out = Vec::new();
	for group in contours.grouped() {
		for index in group {
			let contour = &contours[index];
			let mut depth = 0;
			let mut parent = contour.parent;
			while let Some(p) = parent {
				depth += 1;
				parent = contours[p].parent;
			}
			let wants_clockwise = clockwise == (depth % 2 == 0);
			let is_clockwise = contour.path.area() < 0.0;
			out.push(if wants_clockwise == is_clockwise {
				contour.path.clone()
			} else {
				contour.path.reverse_subpaths()
			});
		}
	}
	Ok(out)
}

/// The segments of a contour, as lists of points relative to the start of each, with the
/// numbers that compare equal when they are the same point up to rounding errors.
fn contour_segments(path: &BezPath) -> Vec<Vec<(i64, i64)>> {
	let quantize = |p: kurbo::Point| ((p.x * 1000.0).round() as i64, (p.y * 1000.0).round() as i64);
	let mut segments = Vec::new();
	let mut start = None;
	let mut current = None;
	for el in path.elements() {
		match *el {
			PathEl::MoveTo(p) => {
				start = Some(p);
				current = Some(p);
			}
			PathEl::LineTo(p) => {
				if let Some(c) = current {
					segments.push(vec![quantize(c), quantize(p)]);
				}
				current = Some(p);
			}
			PathEl::QuadTo(a, p) => {
				if let Some(c) = current {
					segments.push(vec![quantize(c), quantize(a), quantize(p)]);
				}
				current = Some(p);
			}
			PathEl::CurveTo(a, b, p) => {
				if let Some(c) = current {
					segments.push(vec![quantize(c), quantize(a), quantize(b), quantize(p)]);
				}
				current = Some(p);
			}
			PathEl::ClosePath => {
				if let (Some(c), Some(s)) = (current, start)
					&& quantize(c) != quantize(s)
				{
					segments.push(vec![quantize(c), quantize(s)]);
				}
			}
		}
	}
	segments
}

/// A form of a contour that doesn't depend on its start and its direction.
fn canonical_contour(path: &BezPath) -> Vec<Vec<(i64, i64)>> {
	let forward = contour_segments(path);
	let backward: Vec<Vec<(i64, i64)>> = forward
		.iter()
		.rev()
		.map(|s| s.iter().rev().copied().collect())
		.collect();
	let smallest = |segments: &Vec<Vec<(i64, i64)>>| -> Vec<Vec<(i64, i64)>> {
		(0..segments.len().max(1))
			.map(|i| {
				let mut rotated = segments.clone();
				if !rotated.is_empty() {
					rotated.rotate_left(i);
				}
				rotated
			})
			.min()
			.unwrap_or_default()
	};
	smallest(&forward).min(smallest(&backward))
}

/// The separate contours of a path.
fn split_contours(path: &BezPath) -> Vec<BezPath> {
	let mut out: Vec<BezPath> = Vec::new();
	for el in path.elements() {
		if matches!(el, PathEl::MoveTo(_)) {
			out.push(BezPath::new());
		}
		if let Some(current) = out.last_mut() {
			current.push(*el);
		}
	}
	out
}

/// `_same_path`: whether the contours are the same, ignoring their order. Unlike with
/// Skia's output, their start and direction are ignored, since those aren't kept.
fn same_path(path: &BezPath, simplified: &[BezPath]) -> bool {
	let contours = split_contours(path);
	if contours.len() != simplified.len() {
		return false;
	}
	let mut a: Vec<_> = contours.iter().map(canonical_contour).collect();
	let mut b: Vec<_> = simplified.iter().map(canonical_contour).collect();
	a.sort();
	b.sort();
	a == b
}

/// `ttfGlyphFromSkPath`, with the coordinates rounded (`TTGlyphPen`).
fn glyph_from_contours(contours: &[BezPath]) -> Glyph {
	let round = |p: kurbo::Point| {
		(
			fontoxide_misc::round_tools::ot_round_f64(p.x),
			fontoxide_misc::round_tools::ot_round_f64(p.y),
		)
	};
	let mut coordinates: Vec<(f64, f64)> = Vec::new();
	let mut flags: Vec<u8> = Vec::new();
	let mut end_pts = Vec::new();
	for contour in contours {
		let mut points: Vec<((f64, f64), u8)> = Vec::new();
		for el in contour.elements() {
			match *el {
				PathEl::MoveTo(p) | PathEl::LineTo(p) => points.push((round(p), 1)),
				PathEl::QuadTo(a, p) => {
					points.push((round(a), 0));
					points.push((round(p), 1));
				}
				PathEl::CurveTo(a, b, p) => {
					// Curves stay quadratic in `glyf`, up to the rounding of coordinates.
					let start = points.last().map_or(kurbo::Point::ZERO, |e| e.0.into());
					let cubic = kurbo::CubicBez::new(start, a, b, p);
					for (_, _, quad) in cubic.to_quads(0.1) {
						points.push((round(quad.p1), 0));
						points.push((round(quad.p2), 1));
					}
				}
				PathEl::ClosePath => {}
			}
		}
		// The end of a closed contour is its start.
		if points.len() > 1 && points.first() == points.last() {
			points.pop();
		}
		// Implied on-curve points are dropped, as when drawing the path.
		let n = points.len();
		let keep: Vec<bool> = (0..n)
			.map(|i| {
				let (p, on) = points[i];
				if on == 0 || i == 0 {
					return true;
				}
				let (a, a_on) = points[i - 1];
				let (b, b_on) = points[(i + 1) % n];
				!(a_on == 0 && b_on == 0 && a.0 + b.0 == 2.0 * p.0 && a.1 + b.1 == 2.0 * p.1)
			})
			.collect();
		for (&(p, on), &keep) in points.iter().zip(&keep) {
			if keep {
				coordinates.push(p);
				flags.push(on);
			}
		}
		if !coordinates.is_empty() {
			end_pts.push(coordinates.len() as i64 - 1);
		}
	}
	let mut glyph = Glyph {
		number_of_contours: end_pts.len() as i64,
		end_pts_of_contours: end_pts,
		coordinates,
		flags,
		program: Some(Default::default()),
		..Default::default()
	};
	if let Some(&first) = glyph.coordinates.first() {
		let (mut x_min, mut y_min, mut x_max, mut y_max) = (first.0, first.1, first.0, first.1);
		for &(x, y) in &glyph.coordinates {
			x_min = x_min.min(x);
			y_min = y_min.min(y);
			x_max = x_max.max(x);
			y_max = y_max.max(y);
		}
		glyph.x_min = x_min as i64;
		glyph.y_min = y_min as i64;
		glyph.x_max = x_max as i64;
		glyph.y_max = y_max as i64;
	}
	glyph
}

/// `componentsOverlap`, with the base glyphs drawn from the current state of the font.
fn components_overlap(glyph: &Glyph, glyph_set: &GlyphSet<'_>, name: &str) -> Result<bool> {
	if glyph.components.len() < 2 {
		return Ok(false); // single component, no overlaps
	}
	let mut paths = Vec::with_capacity(glyph.components.len());
	for component in &glyph.components {
		let (gid, t) = Glyph::component_info(component)?;
		let base = glyph_set.glyph_order().name(gid).into_owned();
		let mut path = path_from_glyph(&base, glyph_set)?;
		path.apply_affine(kurbo::Affine::new([t.xx, t.xy, t.yx, t.yy, t.dx, t.dy]));
		paths.push(path);
	}
	for i in 0..paths.len() {
		for j in i + 1..paths.len() {
			let contours = binary_op(
				&paths[i],
				&paths[j],
				FillRule::NonZero,
				BinaryOp::Intersection,
			)
			.map_err(|_| boolean_error(name))?;
			if contours.contours().next().is_some() {
				return Ok(true);
			}
		}
	}
	Ok(false)
}

/// `_remove_glyf_overlaps`.
fn remove_glyf_overlaps(
	font: &mut Font,
	names: &[String],
	options: &RemoveOverlapsOptions,
) -> Result<()> {
	let order = font.glyph_order()?;
	// Process all simple glyphs first, then composites with increasing component depth, so
	// that by the time component intersections are tested the base glyphs have already
	// been simplified.
	let mut cache = std::collections::HashMap::new();
	let mut sorted = Vec::with_capacity(names.len());
	{
		let glyf = font.require::<Glyf>()?;
		for name in names {
			let gid = order
				.id(name)
				.ok_or_else(|| Error::GlyphNotFound(name.clone()))?;
			let depth = match glyf.glyphs.get(&gid) {
				Some(g) if g.is_composite() => {
					glyf.composite_maxp_values(gid, &order, 1, &mut cache, &mut vec![gid])?
						.2
				}
				_ => 0,
			};
			sorted.push((depth, name.clone(), gid));
		}
	}
	sorted.sort();
	font.require::<Hmtx>()?;

	let extended = font.require::<Glyf>()?.extended;
	let mut modified = 0;
	for (_, name, gid) in sorted {
		let new_glyph = {
			let glyph_set = GlyphSet::new(font)?;
			let glyf = font_glyf(&glyph_set)?;
			let glyph = glyf
				.glyphs
				.get(&gid)
				.ok_or_else(|| Error::GlyphNotFound(name.clone()))?;
			// Decompose composite glyphs only if components overlap each other.
			let attempt = (|| -> Result<Option<Glyph>> {
				if glyph.number_of_contours > 0
					|| (glyph.is_composite() && components_overlap(glyph, &glyph_set, &name)?)
				{
					let path = path_from_glyph(&name, &glyph_set)?;
					let simplified = simplify(&path, &name)?;
					// Replace the glyph if the simplified path is different.
					if !same_path(&path, &simplified) {
						return Ok(Some(glyph_from_contours(&simplified)));
					}
				}
				Ok(None)
			})();
			match attempt {
				Ok(glyph) => glyph,
				Err(e) => {
					if !options.ignore_errors {
						return Err(e);
					}
					log::error!("Failed to remove overlaps for '{name}'");
					// fontTools leaves the glyph as it is, including its hinting.
					continue;
				}
			}
		};
		match new_glyph {
			Some(glyph) => {
				// Also ensure hmtx LSB == glyph.xMin so the glyph origin is at x=0.
				let x_min = glyph.x_min;
				font.require::<Glyf>()?.glyphs.insert(gid, glyph);
				let hmtx = font.require::<Hmtx>()?;
				let metric = hmtx
					.metrics
					.get_mut(&gid)
					.ok_or_else(|| Error::GlyphNotFound(name.clone()))?;
				if i64::from(metric.1) != x_min {
					metric.1 = crate::binary::fit(x_min, "lsb")?;
				}
				modified += 1;
			}
			None => {
				if options.remove_hinting
					&& let Some(glyph) = font.require::<Glyf>()?.glyphs.get_mut(&gid)
				{
					glyph.trim_ext(true, extended)?;
				}
			}
		}
	}
	log::debug!("Removed overlaps for {modified} glyphs");
	Ok(())
}

/// The `glyf` table of a glyph set, which draws from it.
fn font_glyf<'a>(glyph_set: &GlyphSet<'a>) -> Result<&'a Glyf> {
	glyph_set
		.glyf_table()
		.ok_or(Error::TableNotFound(Glyf::TAG))
}

/// The program of `T2CharStringPen.getCharString`.
fn char_string_from_contours(contours: &[BezPath], width: Option<f64>) -> Result<Vec<Token>> {
	let round = |p: kurbo::Point| {
		(
			fontoxide_misc::round_tools::ot_round(p.x),
			fontoxide_misc::round_tools::ot_round(p.y),
		)
	};
	let mut commands = Vec::new();
	let mut p0 = (0, 0);
	let mut relative = |p: kurbo::Point, out: &mut Vec<Arg>| {
		let pt = round(p);
		out.push(Arg::Int(pt.0 - p0.0));
		out.push(Arg::Int(pt.1 - p0.1));
		p0 = pt;
	};
	for contour in contours {
		let mut current = kurbo::Point::ZERO;
		for el in contour.elements() {
			let mut args = Vec::new();
			match *el {
				PathEl::MoveTo(p) => {
					relative(p, &mut args);
					commands.push(Command::new("rmoveto", args));
					current = p;
				}
				PathEl::LineTo(p) => {
					relative(p, &mut args);
					commands.push(Command::new("rlineto", args));
					current = p;
				}
				PathEl::QuadTo(a, p) => {
					// `BasePen._qCurveToOne`.
					let c1 = current + (a - current) * (2.0 / 3.0);
					let c2 = p + (a - p) * (2.0 / 3.0);
					relative(c1, &mut args);
					relative(c2, &mut args);
					relative(p, &mut args);
					commands.push(Command::new("rrcurveto", args));
					current = p;
				}
				PathEl::CurveTo(a, b, p) => {
					relative(a, &mut args);
					relative(b, &mut args);
					relative(p, &mut args);
					commands.push(Command::new("rrcurveto", args));
					current = p;
				}
				PathEl::ClosePath => {}
			}
		}
	}
	let commands = specialize_commands(
		&commands,
		&SpecializeOptions {
			generalize_first: false,
			..Default::default()
		},
	)?;
	let mut program = commands_to_program(&commands)?;
	if let Some(width) = width {
		program.insert(0, Token::Int(fontoxide_misc::round_tools::ot_round(width)));
	}
	program.push(Token::Op("endchar"));
	Ok(program)
}

/// `_remove_cff_overlaps`.
fn remove_cff_overlaps(
	font: &mut Font,
	names: &[String],
	options: &RemoveOverlapsOptions,
) -> Result<()> {
	// What to do is decided on a copy of the outlines, which the glyph set has.
	let mut new_programs = Vec::new();
	{
		let glyph_set = GlyphSet::new(font)?;
		for name in names {
			let attempt = (|| -> Result<Option<Vec<BezPath>>> {
				let path = path_from_glyph(name, &glyph_set)?;
				let simplified = simplify(&path, name)?;
				Ok((!same_path(&path, &simplified)).then_some(simplified))
			})();
			match attempt {
				Ok(Some(contours)) => new_programs.push((name.clone(), contours)),
				Ok(None) => {}
				Err(e) => {
					if !options.ignore_errors {
						return Err(e);
					}
					log::error!("Failed to remove overlaps for '{name}'");
				}
			}
		}
	}
	if new_programs.is_empty() {
		log::debug!("No overlaps found in the specified CFF glyphs");
		return Ok(());
	}
	let cff: &mut CffFontSet = if font.contains(CFF) {
		&mut font.require::<Cff>()?.cff
	} else {
		&mut font.require::<Cff2>()?.cff
	};
	for (name, contours) in &new_programs {
		let gid = cff
			.glyph_index(name)
			.ok_or_else(|| Error::GlyphNotFound(name.clone()))?;
		// `_charString_from_SkPath`: the width stays in the charstring.
		let (width, default, nominal) = cff.char_string_width(gid)?;
		let width = if width == default {
			None
		} else {
			match (width, nominal) {
				(Some(w), Some(n)) => Some(w - n),
				_ => {
					return Err(Error::ttlib(
						"TypeError: unsupported operand type(s) for -: 'NoneType' and 'NoneType'",
					));
				}
			}
		};
		let program = char_string_from_contours(contours, width)?;
		let top = cff
			.top_dicts
			.first_mut()
			.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
		let char_strings = top
			.char_strings
			.as_mut()
			.ok_or_else(|| Error::ttlib("AttributeError: CharStrings"))?;
		char_strings.items[gid] = T2CharString {
			bytecode: None,
			program,
			fd_select_index: char_strings.items[gid].fd_select_index,
		};
	}
	if options.remove_hinting {
		cff.remove_hints()?;
	}
	if options.remove_unused_subroutines {
		cff.remove_unused_subroutines()?;
	}
	log::debug!("Removed overlaps for {} glyphs", new_programs.len());
	Ok(())
}

/// `removeOverlaps`: simplifies the glyphs of a font by merging overlapping contours.
/// Overlapping components are first decomposed to simple contours, then merged.
///
/// Currently this only works for fonts with `glyf`, `CFF ` or `CFF2` tables. Note that
/// removing overlaps invalidates the hinting.
pub fn remove_overlaps(font: &mut Font, options: &RemoveOverlapsOptions) -> Result<()> {
	let has_glyf = font.has::<Glyf>();
	let has_cff = font.contains(CFF) || font.contains(CFF2);
	if !has_glyf && !has_cff {
		return Err(Error::ttlib(
			"NotImplementedError: No outline data found in the font: missing 'glyf', 'GLYF', 'CFF ', or 'CFF2' table",
		));
	}
	let names = match &options.glyph_names {
		Some(names) => names.clone(),
		None => font.glyph_order()?.names().to_vec(),
	};
	if has_glyf && !has_cff {
		remove_glyf_overlaps(font, &names, options)?;
	} else if has_glyf {
		// The glyph set only draws the CFF outlines of a font with both.
		return Err(Error::ttlib(
			"NotImplementedError: removing overlaps of a font with both glyf and CFF",
		));
	}
	if has_cff {
		remove_cff_overlaps(font, &names, options)?;
	}
	let _ = pen_error;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn square(x: f64, y: f64, size: f64) -> BezPath {
		let mut path = BezPath::new();
		path.move_to((x, y));
		path.line_to((x, y + size));
		path.line_to((x + size, y + size));
		path.line_to((x + size, y));
		path.close_path();
		path
	}

	#[test]
	fn merged_squares() -> Result<()> {
		let mut path = square(0.0, 0.0, 100.0);
		path.extend(square(50.0, 0.0, 100.0));
		let simplified = simplify(&path, "a")?;
		assert_eq!(simplified.len(), 1);
		let glyph = glyph_from_contours(&simplified);
		assert_eq!(glyph.number_of_contours, 1);
		assert_eq!(
			(glyph.x_min, glyph.y_min, glyph.x_max, glyph.y_max),
			(0, 0, 150, 100)
		);
		assert!(!same_path(&path, &simplified));
		Ok(())
	}

	#[test]
	fn keeps_what_has_no_overlaps() -> Result<()> {
		// A square with a hole, in opposite directions.
		let mut path = square(0.0, 0.0, 100.0);
		let hole = square(25.0, 25.0, 50.0).reverse_subpaths();
		path.extend(hole);
		let simplified = simplify(&path, "a")?;
		assert_eq!(simplified.len(), 2);
		assert!(same_path(&path, &simplified));
		// Clockwise outside, the other way around for the hole.
		assert!(path.area() < 0.0);
		assert!(simplified[0].area() < 0.0);
		assert!(simplified[1].area() > 0.0);
		Ok(())
	}

	#[test]
	fn all_off_curve_points() {
		let order = crate::GlyphOrder::default();
		let _ = order;
		let mut path = BezPath::new();
		path.move_to((0.0, 0.0));
		path.quad_to((50.0, 100.0), (100.0, 0.0));
		path.close_path();
		let glyph = glyph_from_contours(&[path]);
		assert_eq!(glyph.flags, vec![1, 0, 1]);
	}
}
