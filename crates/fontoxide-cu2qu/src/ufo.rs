//! Converts the cubic curves of UFO glyphs to quadratic splines
//! (`fontTools.cu2qu.ufo`).
//!
//! Conversion is performed such that the quadratic splines keep the same
//! end-curve tangents as the original cubics. The approach is iterative,
//! increasing the number of segments for a spline until the error gets below
//! a bound.
//!
//! Respective curves from multiple fonts will be converted at once to ensure
//! that the resulting splines are interpolation-compatible.
//!
//! fontTools works on ufoLib2 (or defcon) objects; here they are those of
//! [`fontoxide_ufolib::objects`]. Coordinates keep the `int` or `float` type
//! they have in Python: points that are not converted stay as they are, and
//! converted curves are made of `float`s.

use std::collections::{BTreeMap, BTreeSet};

use fontoxide_pens::point_pen::GuessSmoothPointPen;
use fontoxide_pens::recording_pen::{PointOp, RecordingPointPen};
use fontoxide_pens::{PointPen, SegmentType};
use fontoxide_ufolib::objects::{Contour, Font, Glyph, Point};
use fontoxide_ufolib::{Number, UfoLibError, Value};

use crate::curves_to_quadratic;
use crate::errors::{Incompatibility, IncompatibleFontsError, IncompatibleGlyphsError};

/// `DEFAULT_MAX_ERR`: the default approximation error, relative to the em
/// (1/1000 of it); [`fonts_to_quadratic`] multiplies it by each font's
/// `unitsPerEm`.
pub const DEFAULT_MAX_ERR: f64 = 0.001;

/// `CURVE_TYPE_LIB_KEY`: the font lib key where the curve type is
/// remembered.
pub const CURVE_TYPE_LIB_KEY: &str = "com.github.googlei18n.cu2qu.curve_type";

/// The number of quadratic splines made, by the number of their off-curve
/// points as a string (`stats`).
pub type Stats = BTreeMap<String, usize>;

/// An error of converting glyphs or fonts.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// `ApproxNotFoundError` and the `ValueError` of
	/// [`crate::curves_to_quadratic`].
	#[error(transparent)]
	Cu2Qu(#[from] crate::Error),
	/// `IncompatibleSegmentNumberError` and `IncompatibleSegmentTypesError`.
	#[error(transparent)]
	IncompatibleGlyphs(#[from] IncompatibleGlyphsError),
	/// `IncompatibleFontsError`.
	#[error(transparent)]
	IncompatibleFonts(#[from] IncompatibleFontsError),
	/// `ValueError`: a bad tolerance.
	#[error("{0}")]
	Value(String),
	/// `TypeError`: both `max_err` and `max_err_em`.
	#[error("{0}")]
	Type(String),
	/// `NotImplementedError`: an unknown curve type in a font's lib.
	#[error("NotImplementedError: {0}")]
	NotImplemented(String),
	/// A malformed outline (`PenError`, `AssertionError` or `IndexError` in
	/// fontTools).
	#[error("{0}")]
	Outline(String),
	/// An error reading glyphs.
	#[error(transparent)]
	Ufo(#[from] UfoLibError),
}

/// A `Result` with an [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// A tolerance: one value for all, or one per glyph or font (`float` or a
/// list).
#[derive(Debug, Clone, PartialEq)]
pub enum MaxErr {
	/// The same tolerance for all.
	Value(f64),
	/// One tolerance each.
	List(Vec<f64>),
}

impl From<f64> for MaxErr {
	fn from(v: f64) -> Self {
		MaxErr::Value(v)
	}
}

impl From<Vec<f64>> for MaxErr {
	fn from(v: Vec<f64>) -> Self {
		MaxErr::List(v)
	}
}

fn validate_positive_tolerance(value: f64, name: &str) -> Result<()> {
	// `value <= 0` (false for NaN).
	if value <= 0.0 {
		return Err(Error::Value(format!("{name} must be greater than zero")));
	}
	Ok(())
}

fn validate_positive_tolerances(values: &[f64], name: &str) -> Result<()> {
	values
		.iter()
		.try_for_each(|&v| validate_positive_tolerance(v, name))
}

fn validate_length(values: &[f64], expected: usize, name: &str) -> Result<()> {
	if values.len() != expected {
		return Err(Error::Value(format!(
			"{name} must match the number of inputs"
		)));
	}
	Ok(())
}

/// A point with Python's numbers.
type NPt = (Number, Number);

fn npt(p: &Point) -> NPt {
	(p.x, p.y)
}

fn f64_pt(p: NPt) -> (f64, f64) {
	(p.0.to_f64(), p.1.to_f64())
}

/// The kind of a segment of `GetSegmentsPen` (its `tag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tag {
	Move,
	Line,
	QCurve,
	Curve,
	Close,
	End,
}

impl Tag {
	fn name(self) -> &'static str {
		match self {
			Tag::Move => "move",
			Tag::Line => "line",
			Tag::QCurve => "qcurve",
			Tag::Curve => "curve",
			Tag::Close => "close",
			Tag::End => "end",
		}
	}
}

/// A segment as `GetSegmentsPen` collects it: curves include their initial
/// on-curve point, so some points are duplicated between segments. A point
/// is `None` for the missing on-curve point of a contour of off-curve points,
/// and for the start of a curve with nothing before it.
#[derive(Debug, Clone, PartialEq)]
struct Segment {
	tag: Tag,
	args: Vec<Option<NPt>>,
}

/// `GetSegmentsPen`: collects segments into lists of points for conversion.
#[derive(Debug, Default)]
struct GetSegmentsPen {
	last_pt: Option<NPt>,
	segments: Vec<Segment>,
}

impl GetSegmentsPen {
	fn add_segment(&mut self, tag: Tag, args: Vec<Option<NPt>>) {
		if matches!(tag, Tag::Move | Tag::Line | Tag::QCurve | Tag::Curve) {
			self.last_pt = args.last().copied().flatten();
		}
		self.segments.push(Segment { tag, args });
	}

	fn with_start(&self, points: &[Option<NPt>]) -> Vec<Option<NPt>> {
		let mut args = vec![self.last_pt];
		args.extend_from_slice(points);
		args
	}
}

fn pen_error(message: impl Into<String>) -> Error {
	Error::Outline(message.into())
}

/// `_get_segments`: a glyph's segments, drawn through
/// `PointToSegmentPen(outputImpliedClosingLine=True)` (so that the closing
/// line of a closed contour is a segment whether or not its last and first
/// points overlap). Components are ignored.
fn get_segments(glyph: &Glyph) -> Result<Vec<Segment>> {
	let mut pen = GetSegmentsPen::default();
	for contour in &glyph.contours {
		flush_point_contour(&mut pen, contour)?;
	}
	Ok(pen.segments)
}

/// `BasePointToSegmentPen.endPath` and `PointToSegmentPen._flushContour`.
fn flush_point_contour(pen: &mut GetSegmentsPen, contour: &Contour) -> Result<()> {
	let mut points: Vec<(Option<NPt>, Option<SegmentType>)> = contour
		.points
		.iter()
		.map(|p| (Some(npt(p)), p.segment_type))
		.collect();
	if points.is_empty() {
		return Ok(());
	}
	let mut segments: Vec<(SegmentType, Vec<Option<NPt>>)> = Vec::new();
	if points.len() == 1 {
		segments.push((SegmentType::Move, vec![points[0].0]));
	} else {
		if points[0].1 == Some(SegmentType::Move) {
			segments.push((SegmentType::Move, vec![points[0].0]));
			points.remove(0);
		} else {
			match points.iter().position(|p| p.1.is_some()) {
				None => points.push((None, Some(SegmentType::QCurve))),
				Some(first_on_curve) => points.rotate_left(first_on_curve + 1),
			}
		}
		let mut current = Vec::new();
		for (pt, segment_type) in points {
			current.push(pt);
			if let Some(st) = segment_type {
				segments.push((st, std::mem::take(&mut current)));
			}
		}
	}
	// `PointToSegmentPen._flushContour`.
	if segments.is_empty() {
		return Err(pen_error("Must have at least one segment."));
	}
	let closed;
	let move_pt;
	if segments[0].0 == SegmentType::Move {
		closed = false;
		let points = &segments[0].1;
		if points.len() != 1 {
			return Err(pen_error(format!(
				"Illegal move segment point count: {}",
				points.len()
			)));
		}
		move_pt = points[0];
		segments.remove(0);
	} else {
		closed = true;
		move_pt = segments.last().and_then(|s| s.1.last().copied()).flatten();
	}
	if let Some(pt) = move_pt {
		pen.add_segment(Tag::Move, vec![Some(pt)]);
	}
	for (segment_type, points) in segments {
		match segment_type {
			SegmentType::Line => {
				if points.len() != 1 {
					return Err(pen_error(format!(
						"Illegal line segment point count: {}",
						points.len()
					)));
				}
				// `outputImpliedClosingLine`: always drawn.
				pen.add_segment(Tag::Line, points);
			}
			SegmentType::Curve => {
				let args = pen.with_start(&points);
				pen.add_segment(Tag::Curve, args);
			}
			SegmentType::QCurve => {
				let args = pen.with_start(&points);
				pen.add_segment(Tag::QCurve, args);
			}
			SegmentType::Move => return Err(pen_error("Illegal segmentType: move")),
		}
	}
	pen.add_segment(if closed { Tag::Close } else { Tag::End }, Vec::new());
	Ok(())
}

/// A drawing call of a contour (`(operator, operands)` of a `RecordingPen`).
#[derive(Debug, Clone, PartialEq)]
enum Op {
	MoveTo(Vec<Option<NPt>>),
	LineTo(Vec<Option<NPt>>),
	CurveTo(Vec<Option<NPt>>),
	QCurveTo(Vec<Option<NPt>>),
	ClosePath,
	EndPath,
}

impl Op {
	fn points(&self) -> &[Option<NPt>] {
		match self {
			Op::MoveTo(p) | Op::LineTo(p) | Op::CurveTo(p) | Op::QCurveTo(p) => p,
			Op::ClosePath | Op::EndPath => &[],
		}
	}

	fn with_points(&self, points: Vec<Option<NPt>>) -> Op {
		match self {
			Op::MoveTo(_) => Op::MoveTo(points),
			Op::LineTo(_) => Op::LineTo(points),
			Op::CurveTo(_) => Op::CurveTo(points),
			Op::QCurveTo(_) => Op::QCurveTo(points),
			Op::ClosePath => Op::ClosePath,
			Op::EndPath => Op::EndPath,
		}
	}
}

/// `reversedContour(contour, outputImpliedClosingLine=False)` of
/// `fontTools.pens.reverseContourPen`.
fn reversed_contour(mut contour: Vec<Op>) -> Result<Vec<Op>> {
	let mut out = Vec::new();
	if contour.is_empty() {
		return Ok(out);
	}
	if contour.len() <= 1 {
		return Err(pen_error("invalid contour"));
	}
	let mut closed = match contour.pop() {
		Some(Op::ClosePath) => true,
		Some(Op::EndPath) => false,
		_ => return Err(pen_error("invalid contour")),
	};
	let first = contour.remove(0);
	let mut first_pts = first.points().to_vec();
	let first_on_curve = first_pts.last().copied().flatten();
	match &first {
		Op::MoveTo(_) => {}
		Op::QCurveTo(pts) => {
			// special case for TrueType paths containing only off-curve points
			if first_on_curve.is_some() {
				return Err(pen_error("off-curve only paths must end with 'None'"));
			}
			if !contour.is_empty() {
				return Err(pen_error("only one qCurveTo allowed per off-curve path"));
			}
			let n = pts.len();
			let mut p = vec![pts[0]];
			if n >= 2 {
				p.extend(pts[1..n - 1].iter().rev().copied());
			}
			p.push(None);
			first_pts = p;
		}
		other => {
			return Err(pen_error(format!(
				"invalid initial segment type: {}",
				op_name(other)
			)));
		}
	}
	let first = first.with_points(first_pts.clone());
	if contour.is_empty() {
		// contour contains only one segment, nothing to reverse
		closed = !matches!(first, Op::MoveTo(_));
		out.push(first);
	} else {
		let last_index = contour.len() - 1;
		let last = contour[last_index].clone();
		let last_pts = last.points().to_vec();
		let last_on_curve = last_pts.last().copied().flatten();
		if closed {
			// for closed paths, we keep the starting point
			out.push(first.clone());
			let mut last_pts_now = last_pts.clone();
			if first_on_curve != last_on_curve {
				// emit an implied line between the last and first points
				out.push(Op::LineTo(vec![last_on_curve]));
				last_pts_now.pop();
				last_pts_now.push(first_on_curve);
				contour[last_index] = last.with_points(last_pts_now.clone());
			}
			let second = if contour.len() > 1 {
				contour[0].clone()
			} else {
				// contour has only two points, the second and last are the same
				contour[last_index].clone()
			};
			// if a lineTo follows the initial moveTo, after reversing it will
			// be implied by the closePath, so we don't emit one; unless the
			// lineTo and moveTo overlap, in which case we keep the duplicate
			// points
			if let Op::LineTo(second_pts) = &second
				&& first_pts != *second_pts
			{
				contour.remove(0);
				if !contour.is_empty() {
					// `lastPts` as it was before the implied line.
					let mut p = last_pts[..last_pts.len() - 1].to_vec();
					p.extend(second_pts.iter().copied());
					let n = contour.len() - 1;
					contour[n] = last.with_points(p);
				}
			}
		} else {
			// for open paths, the last point will become the first
			out.push(first.with_points(vec![last_on_curve]));
			let mut p = last_pts[..last_pts.len() - 1].to_vec();
			p.push(first_on_curve);
			contour[last_index] = last.with_points(p);
		}
		// we iterate over all segment pairs in reverse order, and yield each
		// one with the off-curve points reversed (if any), and with the
		// on-curve point of the following segment
		let n = contour.len();
		for k in 0..n {
			let cur = &contour[n - 1 - k];
			let next = if k + 1 < n {
				&contour[n - 2 - k]
			} else {
				&contour[n - 1]
			};
			let cur_pts = cur.points();
			let mut p: Vec<Option<NPt>> = cur_pts[..cur_pts.len().saturating_sub(1)]
				.iter()
				.rev()
				.copied()
				.collect();
			p.push(next.points().last().copied().flatten());
			out.push(cur.with_points(p));
		}
	}
	out.push(if closed { Op::ClosePath } else { Op::EndPath });
	Ok(out)
}

fn op_name(op: &Op) -> &'static str {
	match op {
		Op::MoveTo(_) => "'moveTo'",
		Op::LineTo(_) => "'lineTo'",
		Op::CurveTo(_) => "'curveTo'",
		Op::QCurveTo(_) => "'qCurveTo'",
		Op::ClosePath => "'closePath'",
		Op::EndPath => "'endPath'",
	}
}

/// `glyph.getPen()`: a `SegmentToPointPen` (guessing smooth points) drawing
/// into a glyph, with Python's numbers.
#[derive(Debug)]
struct GlyphSegmentPen<'g> {
	glyph: &'g mut Glyph,
	contour: Option<Vec<(Option<NPt>, Option<SegmentType>)>>,
}

impl GlyphSegmentPen<'_> {
	fn contour(&mut self) -> Result<&mut Vec<(Option<NPt>, Option<SegmentType>)>> {
		self.contour
			.as_mut()
			.ok_or_else(|| pen_error("Contour missing required initial moveTo"))
	}

	fn draw(&mut self, op: &Op) -> Result<()> {
		match op {
			Op::MoveTo(p) => {
				self.contour = Some(vec![(
					p.first().copied().flatten(),
					Some(SegmentType::Move),
				)]);
			}
			Op::LineTo(p) => {
				let pt = p.first().copied().flatten();
				self.contour()?.push((pt, Some(SegmentType::Line)));
			}
			Op::CurveTo(pts) => {
				let Some((&last, off)) = pts.split_last() else {
					return Err(pen_error("TypeError: Must pass in at least one point"));
				};
				let contour = self.contour()?;
				contour.extend(off.iter().map(|&p| (p, None)));
				contour.push((last, Some(SegmentType::Curve)));
			}
			Op::QCurveTo(pts) => {
				let Some((&last, off)) = pts.split_last() else {
					return Err(pen_error("TypeError: Must pass in at least one point"));
				};
				if last.is_none() {
					self.contour = Some(Vec::new());
				}
				let contour = self.contour()?;
				contour.extend(off.iter().map(|&p| (p, None)));
				if last.is_some() {
					contour.push((last, Some(SegmentType::QCurve)));
				}
			}
			Op::ClosePath => {
				let mut contour = self
					.contour
					.take()
					.ok_or_else(|| pen_error("Contour missing required initial moveTo"))?;
				// Remove the last point if it's a duplicate of the first, but
				// only if both are on-curve points (fonttools#4014).
				let n = contour.len();
				if n > 1
					&& contour[0].0 == contour[n - 1].0
					&& contour[0].1.is_some()
					&& contour[n - 1].1.is_some()
				{
					contour[0] = contour[n - 1];
					contour.pop();
				} else {
					// There's an implied line at the end, replace "move" with
					// "line" for the first point
					let Some(first) = contour.first_mut() else {
						return Err(pen_error("IndexError: list index out of range"));
					};
					if first.1 == Some(SegmentType::Move) {
						first.1 = Some(SegmentType::Line);
					}
				}
				self.flush_contour(contour)?;
			}
			Op::EndPath => {
				let contour = self
					.contour
					.take()
					.ok_or_else(|| pen_error("Contour missing required initial moveTo"))?;
				self.flush_contour(contour)?;
			}
		}
		Ok(())
	}

	/// `_flushContour` through `GuessSmoothPointPen` into the glyph.
	fn flush_contour(&mut self, contour: Vec<(Option<NPt>, Option<SegmentType>)>) -> Result<()> {
		// The smooth flags, as `GuessSmoothPointPen` finds them.
		let mut rec = RecordingPointPen::default();
		{
			let mut guess = GuessSmoothPointPen::new(&mut rec, 0.05);
			let outline = |e: fontoxide_pens::Error| pen_error(e.to_string());
			guess.begin_path(None).map_err(outline)?;
			for (pt, st) in &contour {
				// `None` points cannot be drawn into a glyph (`TypeError` when
				// ufoLib2 makes a point of them).
				let Some(pt) = pt else {
					return Err(pen_error(
						"TypeError: 'NoneType' object is not subscriptable",
					));
				};
				guess
					.add_point(f64_pt(*pt), *st, false, None, None)
					.map_err(outline)?;
			}
			guess.end_path().map_err(outline)?;
		}
		let smooth: Vec<bool> = rec
			.value
			.iter()
			.filter_map(|op| match op {
				PointOp::AddPoint { smooth, .. } => Some(*smooth),
				_ => None,
			})
			.collect();
		let points = contour
			.into_iter()
			.zip(smooth)
			.filter_map(|((pt, segment_type), smooth)| {
				pt.map(|(x, y)| Point {
					x,
					y,
					segment_type,
					smooth,
					name: None,
					identifier: None,
				})
			})
			.collect();
		self.glyph.contours.push(Contour {
			points,
			identifier: None,
		});
		Ok(())
	}
}

/// `_set_segments`: draws segments as collected by `GetSegmentsPen` back
/// into a glyph, replacing its contours.
fn set_segments(glyph: &mut Glyph, segments: &[Segment], reverse_direction: bool) -> Result<()> {
	glyph.clear_contours();
	let mut pen = GlyphSegmentPen {
		glyph,
		contour: None,
	};
	// `ReverseContourPen` buffers each contour and reverses it.
	let mut buffer: Vec<Op> = Vec::new();
	for segment in segments {
		let args = &segment.args;
		let op = match segment.tag {
			Tag::Move => Op::MoveTo(args.clone()),
			Tag::Line => Op::LineTo(args.clone()),
			Tag::Curve => Op::CurveTo(args.get(1..).unwrap_or_default().to_vec()),
			Tag::QCurve => Op::QCurveTo(args.get(1..).unwrap_or_default().to_vec()),
			Tag::Close => Op::ClosePath,
			Tag::End => Op::EndPath,
		};
		if !reverse_direction {
			pen.draw(&op)?;
			continue;
		}
		let ends = matches!(op, Op::ClosePath | Op::EndPath);
		buffer.push(op);
		if ends {
			for op in reversed_contour(std::mem::take(&mut buffer))? {
				pen.draw(&op)?;
			}
		}
	}
	Ok(())
}

/// `_segments_to_quadratic`: quadratic approximations of cubic segments.
fn segments_to_quadratic(
	segments: &[Segment],
	max_err: &[f64],
	stats: &mut Stats,
	all_quadratic: bool,
) -> Result<Vec<Segment>> {
	if !segments.iter().all(|s| s.tag == Tag::Curve) {
		return Err(pen_error("Non-cubic given to convert"));
	}
	let mut curves = Vec::with_capacity(segments.len());
	for s in segments {
		let pts: Option<Vec<(f64, f64)>> = s.args.iter().map(|p| p.map(f64_pt)).collect();
		let Some(pts) = pts else {
			return Err(pen_error(
				"TypeError: complex() argument must be a number, not 'NoneType'",
			));
		};
		// LENIENT(L44): fontTools only uses the first four points of a curve, and
		// fails with fewer.
		let [a, b, c, d, ..] = pts[..] else {
			return Err(pen_error("IndexError: list index out of range"));
		};
		curves.push([a, b, c, d]);
	}
	let new_points = curves_to_quadratic(&curves, max_err, all_quadratic)?;
	let n = new_points.first().map_or(0, Vec::len);
	if !new_points.iter().all(|s| s.len() == n) {
		return Err(pen_error("Converted incompatibly"));
	}
	let spline_length = (n as i64 - 2).to_string();
	*stats.entry(spline_length).or_insert(0) += 1;
	let tag = if all_quadratic || n == 3 {
		Tag::QCurve
	} else {
		Tag::Curve
	};
	Ok(new_points
		.into_iter()
		.map(|p| Segment {
			tag,
			args: p
				.into_iter()
				.map(|(x, y)| Some((Number::Float(x), Number::Float(y))))
				.collect(),
		})
		.collect())
}

fn incompatible(glyphs: &[&mut Glyph], kind: Incompatibility) -> Error {
	Error::IncompatibleGlyphs(IncompatibleGlyphsError {
		glyph_names: glyphs.iter().map(|g| g.name.clone()).collect(),
		kind,
	})
}

/// `_glyphs_to_quadratic`: converts a set of compatible glyphs, after the
/// arguments have been set up. Empty glyphs (without contours) are ignored
/// and passed through unchanged. Returns whether the glyphs were modified.
fn glyphs_to_quadratic_impl(
	glyphs: &mut [&mut Glyph],
	max_err: &[f64],
	reverse_direction: bool,
	stats: &mut Stats,
	all_quadratic: bool,
) -> Result<bool> {
	// Skip empty glyphs (with zero contours)
	let mut glyphs: Vec<&mut Glyph> = glyphs.iter_mut().map(|g| &mut **g).collect();
	let max_err: Vec<f64> = glyphs
		.iter()
		.zip(max_err)
		.filter(|(g, _)| !g.is_empty())
		.map(|(_, &e)| e)
		.collect();
	glyphs.retain(|g| !g.is_empty());
	if glyphs.is_empty() {
		return Ok(false);
	}

	let segments_by_glyph = glyphs
		.iter()
		.map(|g| get_segments(g))
		.collect::<Result<Vec<_>>>()?;
	let n = segments_by_glyph[0].len();
	if segments_by_glyph.iter().any(|s| s.len() != n) {
		return Err(incompatible(&glyphs, Incompatibility::SegmentNumber));
	}
	if n == 0 {
		return Ok(false);
	}

	// always modify input glyphs if reverse_direction is True
	let mut glyphs_modified = reverse_direction;

	let mut new_segments_by_location: Vec<Vec<Segment>> = Vec::with_capacity(n);
	let mut incompatible_segments = BTreeMap::new();
	for i in 0..n {
		let mut segments: Vec<Segment> = segments_by_glyph.iter().map(|s| s[i].clone()).collect();
		let tag = segments[0].tag;
		if !segments[1..].iter().all(|s| s.tag == tag) {
			incompatible_segments.insert(
				i,
				segments.iter().map(|s| s.tag.name().to_owned()).collect(),
			);
		} else if tag == Tag::Curve {
			let new_segments = segments_to_quadratic(&segments, &max_err, stats, all_quadratic)?;
			// UPSTREAM-BUG(B61): fontTools tests `all_quadratic or new_segments !=
			// segments`, comparing a list with a tuple, which always differ:
			// glyphs with cubic curves are modified even when the curves stay
			// cubic.
			glyphs_modified = true;
			segments = new_segments;
		}
		new_segments_by_location.push(segments);
	}

	if glyphs_modified {
		for (j, glyph) in glyphs.iter_mut().enumerate() {
			let new_segments: Vec<Segment> = new_segments_by_location
				.iter()
				.map(|l| l[j].clone())
				.collect();
			set_segments(glyph, &new_segments, reverse_direction)?;
		}
	}

	if !incompatible_segments.is_empty() {
		return Err(incompatible(
			&glyphs,
			Incompatibility::SegmentTypes(incompatible_segments),
		));
	}
	Ok(glyphs_modified)
}

/// `glyphs_to_quadratic`: converts the curves of a set of compatible glyphs
/// to quadratic, all at once, ensuring interpolation compatibility. If this
/// is not required, calling it with one glyph at a time may yield slightly
/// more optimized results.
///
/// Empty glyphs (without contours) are ignored and passed through
/// unchanged. `max_err` is 1 (for 1000 units per em) by default. Returns
/// whether the glyphs were modified; fails with
/// [`Error::IncompatibleGlyphs`] if the glyphs have non-interpolatable
/// outlines.
pub fn glyphs_to_quadratic(
	glyphs: &mut [&mut Glyph],
	max_err: Option<MaxErr>,
	reverse_direction: bool,
	stats: Option<&mut Stats>,
	all_quadratic: bool,
) -> Result<bool> {
	let mut own_stats = Stats::new();
	let stats = stats.unwrap_or(&mut own_stats);
	// assume 1000 is the default UPEM
	let max_errors = match max_err.unwrap_or(MaxErr::Value(DEFAULT_MAX_ERR * 1000.0)) {
		MaxErr::List(l) => l,
		MaxErr::Value(v) => vec![v; glyphs.len()],
	};
	validate_length(&max_errors, glyphs.len(), "max_err")?;
	validate_positive_tolerances(&max_errors, "max_err")?;
	glyphs_to_quadratic_impl(glyphs, &max_errors, reverse_direction, stats, all_quadratic)
}

/// `glyph_to_quadratic`: [`glyphs_to_quadratic`] for one glyph.
pub fn glyph_to_quadratic(
	glyph: &mut Glyph,
	max_err: Option<MaxErr>,
	reverse_direction: bool,
	stats: Option<&mut Stats>,
	all_quadratic: bool,
) -> Result<bool> {
	glyphs_to_quadratic(
		&mut [glyph],
		max_err,
		reverse_direction,
		stats,
		all_quadratic,
	)
}

/// The options of [`fonts_to_quadratic`] (its keyword arguments besides
/// `stats`).
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
	/// `max_err_em`: the tolerance relative to the em, [`DEFAULT_MAX_ERR`]
	/// if neither this nor `max_err` is given.
	pub max_err_em: Option<MaxErr>,
	/// `max_err`: the tolerance in font units.
	pub max_err: Option<MaxErr>,
	/// `reverse_direction`.
	pub reverse_direction: bool,
	/// `dump_stats`: logs the spline lengths.
	pub dump_stats: bool,
	/// `remember_curve_type`: stores the curve type in the fonts' lib, and
	/// does not convert fonts again whose curve type is already
	/// `"quadratic"` (or `"mixed"`).
	pub remember_curve_type: bool,
	/// `all_quadratic`: if false, a curve becomes a single quadratic or stays
	/// cubic, whichever is more economical.
	pub all_quadratic: bool,
}

impl Default for Options {
	fn default() -> Self {
		Options {
			max_err_em: None,
			max_err: None,
			reverse_direction: false,
			dump_stats: false,
			remember_curve_type: true,
			all_quadratic: true,
		}
	}
}

fn curve_type(font: &Font) -> Value {
	font.lib
		.get(CURVE_TYPE_LIB_KEY)
		.cloned()
		.unwrap_or_else(|| Value::String("cubic".into()))
}

/// `fonts_to_quadratic`: converts the curves of a collection of fonts to
/// quadratic, all at once, ensuring interpolation compatibility. If this is
/// not required, calling it with one font at a time may yield slightly more
/// optimized results.
///
/// Empty glyphs (without contours) are ignored and passed through
/// unchanged. Returns the names of the modified glyphs (fontTools returns
/// `False` instead of an empty set when the fonts are already converted).
///
/// By default, the curve type is stored in the fonts' lib under
/// [`CURVE_TYPE_LIB_KEY`], and fonts whose curve type is already
/// `"quadratic"` are not converted again. Fails with
/// [`Error::IncompatibleFonts`] if same-named glyphs from different fonts
/// have non-interpolatable outlines.
pub fn fonts_to_quadratic(
	fonts: &mut [Font],
	options: &Options,
	stats: Option<&mut Stats>,
) -> Result<BTreeSet<String>> {
	if options.remember_curve_type {
		let mut curve_types: Vec<Value> = Vec::new();
		for f in fonts.iter() {
			let t = curve_type(f);
			if !curve_types.contains(&t) {
				curve_types.push(t);
			}
		}
		if curve_types.len() == 1 {
			match &curve_types[0] {
				Value::String(t) if t == "quadratic" || t == "mixed" => {
					log::info!("Curves already converted to quadratic");
					return Ok(BTreeSet::new());
				}
				Value::String(t) if t == "cubic" => {} // keep converting
				other => {
					return Err(Error::NotImplemented(match other {
						Value::String(t) => t.clone(),
						other => fontoxide_ufolib::value_repr(other),
					}));
				}
			}
		} else if curve_types.len() > 1 {
			// going to crash later if they do differ
			log::warn!("fonts may contain different curve types");
		}
	}

	let mut own_stats = Stats::new();
	let stats = stats.unwrap_or(&mut own_stats);

	if options.max_err_em.is_some() && options.max_err.is_some() {
		return Err(Error::Type(
			"Only one of max_err and max_err_em can be specified.".into(),
		));
	}
	let max_err_em = match (&options.max_err_em, &options.max_err) {
		(None, None) => Some(MaxErr::Value(DEFAULT_MAX_ERR)),
		(e, _) => e.clone(),
	};

	let mut max_errors: Vec<f64> = Vec::new();
	match &options.max_err {
		Some(MaxErr::List(l)) => {
			validate_length(l, fonts.len(), "max_err")?;
			max_errors = l.clone();
			validate_positive_tolerances(&max_errors, "max_err")?;
		}
		Some(MaxErr::Value(v)) => {
			validate_positive_tolerance(*v, "max_err")?;
			max_errors = vec![*v; fonts.len()];
		}
		None => {}
	}
	let upem = |f: &Font| -> Result<f64> {
		f.units_per_em().map(Number::to_f64).ok_or_else(|| {
			Error::Type("unsupported operand type(s) for *: 'NoneType' and 'float'".into())
		})
	};
	match &max_err_em {
		Some(MaxErr::List(l)) => {
			validate_length(l, fonts.len(), "max_err_em")?;
			validate_positive_tolerances(l, "max_err_em")?;
			max_errors = fonts
				.iter()
				.zip(l)
				.map(|(f, e)| Ok(upem(f)? * e))
				.collect::<Result<_>>()?;
		}
		Some(MaxErr::Value(e)) => {
			validate_positive_tolerance(*e, "max_err_em")?;
			max_errors = fonts
				.iter()
				.map(|f| Ok(upem(f)? * e))
				.collect::<Result<_>>()?;
		}
		None => {}
	}

	let mut modified = BTreeSet::new();
	let mut glyph_errors = BTreeMap::new();
	let names: BTreeSet<String> = fonts
		.iter()
		.flat_map(|f| f.keys().map(str::to_owned).collect::<Vec<_>>())
		.collect();
	for name in names {
		let mut glyphs: Vec<&mut Glyph> = Vec::new();
		let mut cur_max_errors = Vec::new();
		for (font, &error) in fonts.iter_mut().zip(&max_errors) {
			if let Some(glyph) = font.get_mut(&name)? {
				glyphs.push(glyph);
				cur_max_errors.push(error);
			}
		}
		match glyphs_to_quadratic_impl(
			&mut glyphs,
			&cur_max_errors,
			options.reverse_direction,
			stats,
			options.all_quadratic,
		) {
			Ok(true) => {
				modified.insert(name);
			}
			Ok(false) => {}
			Err(Error::IncompatibleGlyphs(e)) => {
				log::error!("{e}");
				glyph_errors.insert(name, e);
			}
			Err(e) => return Err(e),
		}
	}

	if !glyph_errors.is_empty() {
		return Err(IncompatibleFontsError { glyph_errors }.into());
	}

	if !modified.is_empty() && options.dump_stats {
		let lengths: Vec<String> = stats.iter().map(|(l, n)| format!("{l}: {n}")).collect();
		log::info!("New spline lengths: {}", lengths.join(", "));
	}

	if options.remember_curve_type {
		let new_curve_type = if options.all_quadratic {
			"quadratic"
		} else {
			"mixed"
		};
		for font in fonts.iter_mut() {
			let curve_type = curve_type(font);
			if curve_type != Value::String(new_curve_type.into()) {
				font.lib.insert(
					CURVE_TYPE_LIB_KEY.to_owned(),
					Value::String(new_curve_type.into()),
				);
			}
		}
	}
	Ok(modified)
}

/// `font_to_quadratic`: [`fonts_to_quadratic`] for one font.
pub fn font_to_quadratic(
	font: &mut Font,
	options: &Options,
	stats: Option<&mut Stats>,
) -> Result<BTreeSet<String>> {
	fonts_to_quadratic(std::slice::from_mut(font), options, stats)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn n(v: i64) -> Option<NPt> {
		Some((Number::Int(v), Number::Int(v)))
	}

	#[test]
	fn reversed_closed_contour() -> Result<()> {
		let contour = vec![
			Op::MoveTo(vec![n(0)]),
			Op::LineTo(vec![n(1)]),
			Op::LineTo(vec![n(2)]),
			Op::ClosePath,
		];
		assert_eq!(
			reversed_contour(contour)?,
			vec![
				Op::MoveTo(vec![n(0)]),
				Op::LineTo(vec![n(2)]),
				Op::LineTo(vec![n(1)]),
				Op::ClosePath,
			]
		);
		Ok(())
	}
}
