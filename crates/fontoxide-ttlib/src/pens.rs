//! Pens that build glyphs: `fontTools.pens.ttGlyphPen` and
//! `fontTools.pens.t2CharStringPen`. They live here rather than in
//! fontoxide-pens because they make `glyf` glyphs and CFF charstrings.

use fontoxide_misc::fixed_tools::float_to_fixed_to_float;
use fontoxide_misc::round_tools::{ot_round, ot_round_f64, round_func};
use fontoxide_pens::base_pen::{BasePen, SegmentPen};
use fontoxide_pens::{
	Error as PenError, GlyphSet, Pen, Point, PointPen, Result as PenResult, SegmentType, Transform,
	TransformPen, TransformPointPen,
};

use crate::cff::charstring::{T2CharString, Token};
use crate::cff::specializer::{
	Arg, Command, SpecializeOptions, commands_to_program, specialize_commands,
};
use crate::glyph_order::GlyphOrder;
use crate::tables::glyf::{
	ComponentPlacement, FLAG_CUBIC, FLAG_ON_CURVE, Glyph, GlyphComponent, Number,
	drop_implied_on_curve_points,
};
use crate::tables::tt_program::Program;
use crate::{Error, Result};

/// `MAX_F2DOT14`: the largest F2Dot14 value, 0x7FFF / 0x4000.
pub const MAX_F2DOT14: f64 = 0x7FFF as f64 / (1 << 14) as f64;

/// The options of [`TtGlyphPen::glyph`] (`glyph()`'s arguments).
#[derive(Debug, Clone, Copy)]
pub struct GlyphOptions {
	/// The flags of the components (`componentFlags`, 0x04 by default:
	/// `ROUND_XY_TO_GRID`).
	pub component_flags: i64,
	/// Whether on-curve points that are implied are removed
	/// (`dropImpliedOnCurves`).
	pub drop_implied_on_curves: bool,
	/// How the coordinates are rounded (`round`, `otRound` by default).
	pub round: fn(f64) -> f64,
}

impl Default for GlyphOptions {
	fn default() -> Self {
		GlyphOptions {
			component_flags: 0x04,
			drop_implied_on_curves: false,
			round: ot_round_f64,
		}
	}
}

/// The state shared by [`TtGlyphPen`] and [`TtGlyphPointPen`]
/// (`_TTGlyphBasePen`).
#[derive(Debug, Default, Clone)]
struct Buffer {
	points: Vec<Point>,
	end_pts: Vec<i64>,
	/// The point flags; `None` for the `"curve"` placeholder of the point pen.
	types: Vec<Option<u8>>,
	components: Vec<(String, Transform)>,
}

/// `_TTGlyphBasePen.glyph`, after `_buildComponents`.
fn make_glyph(
	buffer: Buffer,
	components: Vec<GlyphComponent>,
	options: &GlyphOptions,
) -> Result<Glyph> {
	let mut glyph = Glyph {
		coordinates: buffer.points,
		end_pts_of_contours: buffer.end_pts,
		flags: buffer.types.iter().map(|t| t.unwrap_or(0)).collect(),
		..Glyph::default()
	};
	if !components.is_empty() {
		// Contours and components have been decomposed together by now.
		glyph.components = components;
		glyph.number_of_contours = -1;
		glyph.coordinates.clear();
		glyph.flags.clear();
		glyph.end_pts_of_contours.clear();
	} else {
		glyph.number_of_contours = glyph.end_pts_of_contours.len() as i64;
		glyph.program = Some(Program::from_bytecode(&[]));
		if options.drop_implied_on_curves {
			drop_implied_on_curve_points(&mut [&mut glyph])?;
		}
		for p in &mut glyph.coordinates {
			*p = ((options.round)(p.0), (options.round)(p.1));
		}
	}
	Ok(glyph)
}

/// `_buildComponents` for one component that is kept: the component, with
/// its transformation quantized to F2Dot14.
fn make_component(
	order: &GlyphOrder,
	name: &str,
	t: &Transform,
	flags: i64,
	handle_overflowing: bool,
) -> Result<GlyphComponent> {
	let glyph = order
		.id(name)
		.ok_or_else(|| Error::ttlib(format!("KeyError: '{name}'")))?;
	// Quantize to F2Dot14, to get the values that a binary glyf table gives.
	let mut m = [t.xx, t.xy, t.yx, t.yy].map(|v| float_to_fixed_to_float(v, 14));
	let transform = if m != [1.0, 0.0, 0.0, 1.0] {
		if handle_overflowing && m.iter().any(|&s| MAX_F2DOT14 < s && s <= 2.0) {
			// Clamp values about +2.0 to keep the component.
			for s in &mut m {
				if MAX_F2DOT14 < *s && *s <= 2.0 {
					*s = MAX_F2DOT14;
				}
			}
		}
		Some([[m[0], m[1]], [m[2], m[3]]])
	} else {
		None
	};
	Ok(GlyphComponent {
		glyph,
		flags,
		placement: ComponentPlacement::Offset {
			x: Number::Int(ot_round(t.dx)),
			y: Number::Int(ot_round(t.dy)),
		},
		transform,
	})
}

/// Whether a component transformation cannot be encoded as F2Dot14.
fn overflowing(components: &[(String, Transform)]) -> bool {
	components.iter().any(|(_, t)| {
		[t.xx, t.xy, t.yx, t.yy]
			.iter()
			.any(|&s| s > 2.0 || s < -2.0)
	})
}

/// Draws onto a TrueType glyph (`TTGlyphPen`); [`TtGlyphPen::glyph`] returns
/// it. Components whose transformation does not fit F2Dot14 are decomposed
/// (`handleOverflowingTransforms`), and so are all of them in a glyph that
/// also has contours.
pub struct TtGlyphPen<'g> {
	glyph_set: Option<&'g dyn GlyphSet>,
	/// `handleOverflowingTransforms`.
	pub handle_overflowing_transforms: bool,
	/// Whether the closing line of a contour is kept as a point even when it
	/// returns to the start (`outputImpliedClosingLine`).
	pub output_implied_closing_line: bool,
	buffer: Buffer,
}

impl std::fmt::Debug for TtGlyphPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TtGlyphPen")
			.field(
				"handle_overflowing_transforms",
				&self.handle_overflowing_transforms,
			)
			.field(
				"output_implied_closing_line",
				&self.output_implied_closing_line,
			)
			.field("buffer", &self.buffer)
			.finish_non_exhaustive()
	}
}

impl<'g> TtGlyphPen<'g> {
	/// `TTGlyphPen(glyphSet)`.
	pub fn new(glyph_set: Option<&'g dyn GlyphSet>) -> Self {
		TtGlyphPen {
			glyph_set,
			handle_overflowing_transforms: true,
			output_implied_closing_line: false,
			buffer: Buffer::default(),
		}
	}

	/// `points`.
	pub fn points(&self) -> &[Point] {
		&self.buffer.points
	}

	/// `types`: the point flags.
	pub fn types(&self) -> Vec<u8> {
		self.buffer.types.iter().map(|t| t.unwrap_or(0)).collect()
	}

	/// `endPts`.
	pub fn end_pts(&self) -> &[i64] {
		&self.buffer.end_pts
	}

	fn add_point(&mut self, pt: Point, tp: u8) {
		self.buffer.points.push(pt);
		self.buffer.types.push(Some(tp));
	}

	fn pop_point(&mut self) {
		self.buffer.points.pop();
		self.buffer.types.pop();
	}

	/// `_isClosed`.
	fn is_closed(&self) -> bool {
		let b = &self.buffer;
		b.points.is_empty() || b.end_pts.last() == Some(&(b.points.len() as i64 - 1))
	}

	/// `glyph()` with the default options.
	pub fn glyph(&mut self, order: &GlyphOrder) -> Result<Glyph> {
		self.glyph_with(order, &GlyphOptions::default())
	}

	/// `glyph(componentFlags, dropImpliedOnCurves, round=…)`: the glyph drawn
	/// so far; the pen is emptied. Component names are resolved with `order`.
	pub fn glyph_with(&mut self, order: &GlyphOrder, options: &GlyphOptions) -> Result<Glyph> {
		if !self.is_closed() {
			return Err(PenError::Pen("Didn't close last contour.".into()).into());
		}
		let components = self.build_components(order, options.component_flags)?;
		let buffer = std::mem::take(&mut self.buffer);
		make_glyph(buffer, components, options)
	}

	/// `_buildComponents`.
	fn build_components(&mut self, order: &GlyphOrder, flags: i64) -> Result<Vec<GlyphComponent>> {
		let overflow = self.handle_overflowing_transforms && overflowing(&self.buffer.components);
		let mut out = Vec::new();
		// Decomposing may add components, which are processed too.
		let mut i = 0;
		while i < self.buffer.components.len() {
			let (name, t) = self.buffer.components[i].clone();
			i += 1;
			let glyph_set = self.glyph_set;
			if !glyph_set.is_some_and(|g| g.contains(&name)) {
				log::warn!("skipped non-existing component '{name}'");
				continue;
			}
			if !self.buffer.points.is_empty() || overflow {
				// A glyph cannot have both contours and components.
				if let Some(gs) = glyph_set {
					let mut tpen = TransformPen::new(self, t);
					gs.draw_glyph(&name, &mut tpen)?;
				}
				continue;
			}
			out.push(make_component(
				order,
				&name,
				&t,
				flags,
				self.handle_overflowing_transforms,
			)?);
		}
		Ok(out)
	}
}

impl Pen for TtGlyphPen<'_> {
	fn move_to(&mut self, pt: Point) -> PenResult<()> {
		if !self.is_closed() {
			return Err(PenError::Pen(
				"\"move\"-type point must begin a new contour.".into(),
			));
		}
		self.add_point(pt, FLAG_ON_CURVE);
		Ok(())
	}

	fn line_to(&mut self, pt: Point) -> PenResult<()> {
		self.add_point(pt, FLAG_ON_CURVE);
		Ok(())
	}

	fn curve_to(&mut self, points: &[Point]) -> PenResult<()> {
		if points.len() % 2 != 1 {
			return Err(PenError::Usage(
				"AssertionError: len(points) % 2 == 1".into(),
			));
		}
		let (last, off) = points
			.split_last()
			.map_or((None, &[][..]), |(l, r)| (Some(*l), r));
		for &pt in off {
			self.add_point(pt, FLAG_CUBIC);
		}
		if let Some(pt) = last {
			self.add_point(pt, 1);
		}
		Ok(())
	}

	fn q_curve_to(&mut self, off_curves: &[Point], on_curve: Option<Point>) -> PenResult<()> {
		if off_curves.is_empty() && on_curve.is_none() {
			return Err(PenError::Usage("AssertionError: len(points) >= 1".into()));
		}
		for &pt in off_curves {
			self.add_point(pt, 0);
		}
		// No on-curve point means a contour without any.
		if let Some(pt) = on_curve {
			self.add_point(pt, 1);
		}
		Ok(())
	}

	fn close_path(&mut self) -> PenResult<()> {
		let mut end_pt = self.buffer.points.len() as i64 - 1;
		// Anchors (one-point paths) are ignored.
		if end_pt == 0 || self.buffer.end_pts.last().is_some_and(|&e| end_pt == e + 1) {
			self.pop_point();
			return Ok(());
		}
		if !self.output_implied_closing_line {
			// The last point goes if it is the first one again.
			let start = self.buffer.end_pts.last().map_or(0, |e| e + 1);
			if end_pt >= 0
				&& self.buffer.points.get(start as usize) == self.buffer.points.get(end_pt as usize)
			{
				self.pop_point();
				end_pt -= 1;
			}
		}
		self.buffer.end_pts.push(end_pt);
		Ok(())
	}

	fn end_path(&mut self) -> PenResult<()> {
		// TrueType contours are always closed.
		self.close_path()
	}

	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> PenResult<()> {
		self.buffer
			.components
			.push((glyph_name.to_owned(), transformation));
		Ok(())
	}
}

/// The point pen version of [`TtGlyphPen`] (`TTGlyphPointPen`).
pub struct TtGlyphPointPen<'g> {
	glyph_set: Option<&'g dyn GlyphSet>,
	/// `handleOverflowingTransforms`.
	pub handle_overflowing_transforms: bool,
	buffer: Buffer,
	current_start: Option<usize>,
}

impl std::fmt::Debug for TtGlyphPointPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TtGlyphPointPen")
			.field(
				"handle_overflowing_transforms",
				&self.handle_overflowing_transforms,
			)
			.field("buffer", &self.buffer)
			.finish_non_exhaustive()
	}
}

impl<'g> TtGlyphPointPen<'g> {
	/// `TTGlyphPointPen(glyphSet)`.
	pub fn new(glyph_set: Option<&'g dyn GlyphSet>) -> Self {
		TtGlyphPointPen {
			glyph_set,
			handle_overflowing_transforms: true,
			buffer: Buffer::default(),
			current_start: None,
		}
	}

	/// `points`.
	pub fn points(&self) -> &[Point] {
		&self.buffer.points
	}

	/// `glyph()` with the default options.
	pub fn glyph(&mut self, order: &GlyphOrder) -> Result<Glyph> {
		self.glyph_with(order, &GlyphOptions::default())
	}

	/// `glyph(componentFlags, dropImpliedOnCurves, round=…)`.
	pub fn glyph_with(&mut self, order: &GlyphOrder, options: &GlyphOptions) -> Result<Glyph> {
		if self.current_start.is_some() {
			return Err(PenError::Pen("Didn't close last contour.".into()).into());
		}
		let components = self.build_components(order, options.component_flags)?;
		let buffer = std::mem::take(&mut self.buffer);
		self.current_start = None;
		make_glyph(buffer, components, options)
	}

	/// `_buildComponents`.
	fn build_components(&mut self, order: &GlyphOrder, flags: i64) -> Result<Vec<GlyphComponent>> {
		let overflow = self.handle_overflowing_transforms && overflowing(&self.buffer.components);
		let mut out = Vec::new();
		let mut i = 0;
		while i < self.buffer.components.len() {
			let (name, t) = self.buffer.components[i].clone();
			i += 1;
			let glyph_set = self.glyph_set;
			if !glyph_set.is_some_and(|g| g.contains(&name)) {
				log::warn!("skipped non-existing component '{name}'");
				continue;
			}
			if !self.buffer.points.is_empty() || overflow {
				if let Some(gs) = glyph_set {
					let mut tpen = TransformPointPen::new(self, t);
					gs.draw_glyph_points(&name, &mut tpen)?;
				}
				continue;
			}
			out.push(make_component(
				order,
				&name,
				&t,
				flags,
				self.handle_overflowing_transforms,
			)?);
		}
		Ok(out)
	}
}

impl PointPen for TtGlyphPointPen<'_> {
	fn begin_path(&mut self, _: Option<&str>) -> PenResult<()> {
		if self.current_start.is_some() {
			return Err(PenError::Pen("Didn't close previous contour.".into()));
		}
		self.current_start = Some(self.buffer.points.len());
		Ok(())
	}

	fn end_path(&mut self) -> PenResult<()> {
		let Some(start) = self.current_start.take() else {
			return Err(PenError::Pen("Contour is already closed.".into()));
		};
		let b = &mut self.buffer;
		if start == b.points.len() {
			// An empty contour is ignored.
			return Ok(());
		}
		let contour_start = b.end_pts.last().map_or(0, |&e| (e + 1) as usize);
		b.end_pts.push(b.points.len() as i64 - 1);
		// Resolve the off-curve points of cubic segments.
		let n = b.types.len();
		for i in contour_start..n {
			if b.types[i].is_none() {
				let prev = |j: usize| {
					if j == 0 || j - 1 < contour_start {
						n - 1
					} else {
						j - 1
					}
				};
				let mut j = prev(i);
				while b.types[j] == Some(0) {
					b.types[j] = Some(FLAG_CUBIC);
					j = prev(j);
				}
				b.types[i] = Some(FLAG_ON_CURVE);
			}
		}
		Ok(())
	}

	fn add_point(
		&mut self,
		pt: Point,
		segment_type: Option<SegmentType>,
		_: bool,
		_: Option<&str>,
		_: Option<&str>,
	) -> PenResult<()> {
		if self.current_start.is_none() {
			return Err(PenError::Pen(
				"Can't add a point to a closed contour.".into(),
			));
		}
		self.buffer.types.push(match segment_type {
			None => Some(0),
			Some(SegmentType::Line | SegmentType::Move | SegmentType::QCurve) => {
				Some(FLAG_ON_CURVE)
			}
			// Resolved in `end_path`.
			Some(SegmentType::Curve) => None,
		});
		self.buffer.points.push(pt);
		Ok(())
	}

	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: Transform,
		_: Option<&str>,
	) -> PenResult<()> {
		self.buffer
			.components
			.push((base_glyph_name.to_owned(), transformation));
		Ok(())
	}
}

/// A coordinate of [`T2CharStringPen`]: its value, and whether it is a
/// Python `int`. Integral values are taken as integers.
#[derive(Debug, Clone, Copy)]
struct Coord(f64);

impl Coord {
	fn is_int(self) -> bool {
		self.0.fract() == 0.0
	}
}

/// The segments of a Type 2 charstring (`T2CharStringPen`'s state).
pub struct T2Segments {
	round: Box<dyn Fn(f64) -> f64>,
	cff2: bool,
	width: Option<f64>,
	commands: Vec<Command>,
	p0: (Coord, Coord),
}

impl std::fmt::Debug for T2Segments {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("T2Segments")
			.field("cff2", &self.cff2)
			.field("width", &self.width)
			.field("commands", &self.commands)
			.finish_non_exhaustive()
	}
}

/// Draws a Type 2 charstring (`T2CharStringPen`). `round_tolerance` is the
/// largest difference between a coordinate and the integer it is rounded to
/// (0.5 rounds everything, 0 nothing). Integral coordinates are taken as
/// integers, as fontTools does for the integers it is given.
#[derive(Debug)]
pub struct T2CharStringPen<'g> {
	pen: BasePen<'g, T2Segments>,
}

impl Pen for T2CharStringPen<'_> {
	fn move_to(&mut self, pt: Point) -> PenResult<()> {
		self.pen.move_to(pt)
	}
	fn line_to(&mut self, pt: Point) -> PenResult<()> {
		self.pen.line_to(pt)
	}
	fn curve_to(&mut self, points: &[Point]) -> PenResult<()> {
		self.pen.curve_to(points)
	}
	fn q_curve_to(&mut self, off_curves: &[Point], on_curve: Option<Point>) -> PenResult<()> {
		self.pen.q_curve_to(off_curves, on_curve)
	}
	fn close_path(&mut self) -> PenResult<()> {
		self.pen.close_path()
	}
	fn end_path(&mut self) -> PenResult<()> {
		self.pen.end_path()
	}
	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> PenResult<()> {
		self.pen.add_component(glyph_name, transformation)
	}
}

impl<'g> T2CharStringPen<'g> {
	/// `T2CharStringPen(width, glyphSet, roundTolerance, CFF2)`.
	/// A negative tolerance is an error (`ValueError`).
	pub fn new(
		width: Option<f64>,
		glyph_set: Option<&'g dyn GlyphSet>,
		round_tolerance: f64,
		cff2: bool,
	) -> Result<Self> {
		let round = round_func(round_tolerance).ok_or_else(|| {
			Error::value(format!(
				"Rounding tolerance must be positive: {round_tolerance}"
			))
		})?;
		Ok(T2CharStringPen {
			pen: BasePen::with_inner(
				T2Segments {
					round,
					cff2,
					width,
					commands: Vec::new(),
					p0: (Coord(0.0), Coord(0.0)),
				},
				glyph_set,
			),
		})
	}

	/// `getCharString(optimize=optimize)`: the charstring of what was drawn,
	/// specialized if `optimize`.
	pub fn get_char_string(&self, optimize: bool) -> Result<T2CharString> {
		let s = &self.pen.inner;
		let commands = if optimize {
			let max_stack = if s.cff2 { 513 } else { 48 };
			specialize_commands(
				&s.commands,
				&SpecializeOptions {
					generalize_first: false,
					max_stack,
					..SpecializeOptions::default()
				},
			)?
		} else {
			s.commands.clone()
		};
		let mut program = commands_to_program(&commands)?;
		if let Some(width) = s.width {
			if s.cff2 {
				return Err(Error::Assertion(
					"CFF2 does not allow encoding glyph width in CharString.".into(),
				));
			}
			program.insert(0, Token::Int(ot_round(width)));
		}
		if !s.cff2 {
			program.push(Token::Op("endchar"));
		}
		Ok(T2CharString {
			bytecode: None,
			program,
			fd_select_index: None,
		})
	}
}

impl T2Segments {
	/// `_p`: the point, rounded, relative to the previous one.
	fn p(&mut self, pt: Point) -> [Arg; 2] {
		let p0 = self.p0;
		let pt = (Coord((self.round)(pt.0)), Coord((self.round)(pt.1)));
		self.p0 = pt;
		let d = |a: Coord, b: Coord| {
			if a.is_int() && b.is_int() {
				Arg::Int((a.0 - b.0) as i64)
			} else {
				Arg::Float(a.0 - b.0)
			}
		};
		[d(pt.0, p0.0), d(pt.1, p0.1)]
	}
}

impl SegmentPen for T2Segments {
	fn move_to(&mut self, pt: Point) -> PenResult<()> {
		let args = self.p(pt).to_vec();
		self.commands.push(Command::new("rmoveto", args));
		Ok(())
	}

	fn line_to(&mut self, pt: Point) -> PenResult<()> {
		let args = self.p(pt).to_vec();
		self.commands.push(Command::new("rlineto", args));
		Ok(())
	}

	fn curve_to_one(
		&mut self,
		_: Option<Point>,
		pt1: Point,
		pt2: Point,
		pt3: Point,
	) -> PenResult<()> {
		let mut args = self.p(pt1).to_vec();
		args.extend(self.p(pt2));
		args.extend(self.p(pt3));
		self.commands.push(Command::new("rrcurveto", args));
		Ok(())
	}
}
