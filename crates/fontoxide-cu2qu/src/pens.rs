//! Pens converting cubic curves to quadratic splines: a port of
//! `fontTools.pens.cu2quPen`, which fontTools has among its pens; here it is
//! with the conversion it uses, so that `fontoxide-pens` does not depend on
//! this crate.

use std::collections::BTreeMap;

use fontoxide_pens::base_pen::decompose_super_bezier_segment;
use fontoxide_pens::point_pen::{PenPoint, ReverseContourPointPen, Segment, contour_segments};
use fontoxide_pens::reverse_contour_pen::ReverseContourPen;
use fontoxide_pens::{Error, Pen, Point, PointPen, Result, SegmentType, Transform};

use crate::{curve_to_quadratic, curves_to_quadratic};

fn cu2qu_error(e: crate::Error) -> Error {
	Error::Glyph(Box::new(e))
}

/// The pen a filter pen draws onto, possibly through a reversing pen.
enum Out<'p> {
	Plain(&'p mut dyn Pen),
	Reverse(ReverseContourPen<'p>),
}

impl Out<'_> {
	fn pen(&mut self) -> &mut dyn Pen {
		match self {
			Out::Plain(p) => *p,
			Out::Reverse(p) => p,
		}
	}
}

/// Converts cubic curves to quadratic splines within `max_err`
/// (`Cu2QuPen`). For best results, `max_err` should be about a thousandth
/// of the em.
pub struct Cu2QuPen<'p> {
	out: Out<'p>,
	current_pt: Option<Point>,
	/// `max_err`.
	pub max_err: f64,
	/// The number of quadratic segments made, by the number of off-curve
	/// points as a string (`stats`); counted if `Some`.
	pub stats: Option<BTreeMap<String, usize>>,
	/// `all_quadratic`: if false, a curve becomes a single quadratic or
	/// stays cubic, whichever is more economical.
	pub all_quadratic: bool,
}

impl std::fmt::Debug for Cu2QuPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Cu2QuPen")
			.field("max_err", &self.max_err)
			.field("stats", &self.stats)
			.field("all_quadratic", &self.all_quadratic)
			.finish_non_exhaustive()
	}
}

fn count(stats: &mut Option<BTreeMap<String, usize>>, n: usize) {
	if let Some(stats) = stats {
		*stats.entry(n.to_string()).or_insert(0) += 1;
	}
}

impl<'p> Cu2QuPen<'p> {
	/// `Cu2QuPen(other_pen, max_err, reverse_direction)`, with all
	/// quadratic curves and without statistics.
	pub fn new(other_pen: &'p mut dyn Pen, max_err: f64, reverse_direction: bool) -> Self {
		Cu2QuPen {
			out: if reverse_direction {
				Out::Reverse(ReverseContourPen::new(other_pen, false))
			} else {
				Out::Plain(other_pen)
			},
			current_pt: None,
			max_err,
			stats: None,
			all_quadratic: true,
		}
	}

	fn convert_curve(&mut self, pt1: Point, pt2: Point, pt3: Point) -> Result<()> {
		let current = self
			.current_pt
			.ok_or_else(|| Error::Usage("TypeError: 'NoneType' object is not iterable".into()))?;
		let result =
			curve_to_quadratic(&[current, pt1, pt2, pt3], self.max_err, self.all_quadratic)
				.map_err(cu2qu_error)?;
		count(&mut self.stats, result.len() - 2);
		if self.all_quadratic || result.len() == 3 {
			let (on, off) = result[1..]
				.split_last()
				.map_or((None, &[][..]), |(l, r)| (Some(*l), r));
			self.q_curve_to(off, on)
		} else {
			self.out.pen().curve_to(&result[1..])?;
			self.current_pt = result.last().copied();
			Ok(())
		}
	}
}

impl Pen for Cu2QuPen<'_> {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.out.pen().move_to(pt)?;
		self.current_pt = Some(pt);
		Ok(())
	}

	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.out.pen().line_to(pt)?;
		self.current_pt = Some(pt);
		Ok(())
	}

	fn curve_to(&mut self, points: &[Point]) -> Result<()> {
		match points.len() {
			3 => self.convert_curve(points[0], points[1], points[2]),
			n if n > 3 => {
				for (a, b, c) in decompose_super_bezier_segment(points)? {
					self.convert_curve(a, b, c)?;
				}
				Ok(())
			}
			_ => {
				let (on, off) = points
					.split_last()
					.map_or((None, &[][..]), |(l, r)| (Some(*l), r));
				self.q_curve_to(off, on)
			}
		}
	}

	fn q_curve_to(&mut self, off_curves: &[Point], on_curve: Option<Point>) -> Result<()> {
		self.out.pen().q_curve_to(off_curves, on_curve)?;
		self.current_pt = on_curve;
		Ok(())
	}

	fn close_path(&mut self) -> Result<()> {
		self.out.pen().close_path()?;
		self.current_pt = None;
		Ok(())
	}

	fn end_path(&mut self) -> Result<()> {
		self.out.pen().end_path()?;
		self.current_pt = None;
		Ok(())
	}

	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> Result<()> {
		self.out.pen().add_component(glyph_name, transformation)
	}
}

/// The point pen a filter point pen draws onto, possibly through a reversing
/// pen.
enum PointOut<'p> {
	Plain(&'p mut dyn PointPen),
	Reverse(ReverseContourPointPen<'p>),
}

impl PointOut<'_> {
	fn pen(&mut self) -> &mut dyn PointPen {
		match self {
			PointOut::Plain(p) => *p,
			PointOut::Reverse(p) => p,
		}
	}
}

/// The point pen version of [`Cu2QuPen`] (`Cu2QuPointPen`).
pub struct Cu2QuPointPen<'p> {
	pen: PointOut<'p>,
	current: Option<Vec<PenPoint>>,
	/// `max_err`.
	pub max_err: f64,
	/// `stats`, counted if `Some`.
	pub stats: Option<BTreeMap<String, usize>>,
	/// `all_quadratic`.
	pub all_quadratic: bool,
}

impl std::fmt::Debug for Cu2QuPointPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Cu2QuPointPen")
			.field("max_err", &self.max_err)
			.field("stats", &self.stats)
			.field("all_quadratic", &self.all_quadratic)
			.finish_non_exhaustive()
	}
}

fn plain(pt: Point) -> PenPoint {
	PenPoint {
		pt: Some(pt),
		segment_type: None,
		smooth: false,
		name: None,
		identifier: None,
	}
}

impl<'p> Cu2QuPointPen<'p> {
	/// `Cu2QuPointPen(other_point_pen, max_err, reverse_direction)`, with all
	/// quadratic curves and without statistics.
	pub fn new(
		other_point_pen: &'p mut dyn PointPen,
		max_err: f64,
		reverse_direction: bool,
	) -> Self {
		Cu2QuPointPen {
			pen: if reverse_direction {
				PointOut::Reverse(ReverseContourPointPen::new(other_point_pen))
			} else {
				PointOut::Plain(other_point_pen)
			},
			current: None,
			max_err,
			stats: None,
			all_quadratic: true,
		}
	}

	/// `_flushContour`.
	fn flush_contour(&mut self, segments: Vec<Segment>) -> Result<()> {
		if segments.is_empty() {
			return Err(Error::Usage("AssertionError".into()));
		}
		let closed = segments[0].0 != SegmentType::Move;
		let mut new_segments: Vec<Segment> = Vec::new();
		let mut prev_on_curve = segments[segments.len() - 1].1.last().and_then(|p| p.pt);
		for (segment_type, points) in segments {
			if segment_type == SegmentType::Curve {
				for sub_points in split_super_bezier_segments(&points)? {
					let last = sub_points[sub_points.len() - 1].clone();
					let (Some(bcp1), Some(bcp2), Some(on_curve), Some(prev)) =
						(sub_points[0].pt, sub_points[1].pt, last.pt, prev_on_curve)
					else {
						return Err(Error::Usage("TypeError: a point is None".into()));
					};
					let quad = curve_to_quadratic(
						&[prev, bcp1, bcp2, on_curve],
						self.max_err,
						self.all_quadratic,
					)
					.map_err(cu2qu_error)?;
					count(&mut self.stats, quad.len() - 2);
					let mut new_points: Vec<PenPoint> =
						quad[1..quad.len() - 1].iter().map(|&p| plain(p)).collect();
					new_points.push(last);
					let st = if self.all_quadratic || new_points.len() == 2 {
						SegmentType::QCurve
					} else {
						SegmentType::Curve
					};
					new_segments.push((st, new_points));
					prev_on_curve = Some(on_curve);
				}
			} else {
				prev_on_curve = points.last().and_then(|p| p.pt);
				new_segments.push((segment_type, points));
			}
		}
		if closed {
			// `BasePointToSegmentPen.endPath` rotated the closed contour to end
			// with its first on-curve point: restore the original start.
			new_segments.rotate_right(1);
		}
		self.draw_points(new_segments)
	}

	/// `_drawPoints`.
	fn draw_points(&mut self, segments: Vec<Segment>) -> Result<()> {
		let pen = self.pen.pen();
		pen.begin_path(None)?;
		let mut last_offcurves: Vec<PenPoint> = Vec::new();
		for (i, (segment_type, mut points)) in segments.into_iter().enumerate() {
			let ok = match segment_type {
				SegmentType::Move | SegmentType::Line => points.len() == 1,
				SegmentType::QCurve => points.len() >= 2,
				SegmentType::Curve => points.len() == 3,
			};
			if !ok {
				let n = match segment_type {
					SegmentType::QCurve => 2,
					SegmentType::Curve => 3,
					_ => 1,
				};
				return Err(Error::Usage(format!(
					"AssertionError: illegal '{}' segment point count: expected {n}, got {}",
					segment_type.as_str(),
					points.len()
				)));
			}
			let Some(last) = points.pop() else {
				continue;
			};
			if i == 0 {
				// Off-curve points before the first on-curve one go at the end.
				last_offcurves = points;
			} else {
				for p in points {
					if let Some(pt) = p.pt {
						pen.add_point(
							pt,
							None,
							p.smooth,
							p.name.as_deref(),
							p.identifier.as_deref(),
						)?;
					}
				}
			}
			// A quadratic contour without on-curve points ends with `None`,
			// which is skipped.
			if let Some(pt) = last.pt {
				pen.add_point(
					pt,
					Some(segment_type),
					last.smooth,
					last.name.as_deref(),
					last.identifier.as_deref(),
				)?;
			}
		}
		for p in last_offcurves {
			if let Some(pt) = p.pt {
				pen.add_point(
					pt,
					None,
					p.smooth,
					p.name.as_deref(),
					p.identifier.as_deref(),
				)?;
			}
		}
		pen.end_path()
	}
}

/// `_split_super_bezier_segments`.
fn split_super_bezier_segments(points: &[PenPoint]) -> Result<Vec<Vec<PenPoint>>> {
	// The number of control points.
	let n = points.len() as isize - 1;
	if n == 2 {
		// A plain Bézier segment.
		return Ok(vec![points.to_vec()]);
	}
	if n < 2 {
		return Err(Error::Usage(format!(
			"AssertionError: expected 2 control points, found: {n}"
		)));
	}
	// A "super" Bézier: decomposed.
	let last = points[points.len() - 1].clone();
	let num_sub_segments = n - 1;
	let pts: Vec<Point> = points.iter().filter_map(|p| p.pt).collect();
	let mut sub_segments = Vec::new();
	for (i, (a, b, c)) in decompose_super_bezier_segment(&pts)?
		.into_iter()
		.enumerate()
	{
		let mut seg = vec![plain(a), plain(b)];
		if i as isize == num_sub_segments - 1 {
			// The last on-curve point keeps its attributes.
			seg.push(last.clone());
		} else {
			// The on-curve points of the sub-segments are smooth.
			let mut on = plain(c);
			on.smooth = true;
			seg.push(on);
		}
		sub_segments.push(seg);
	}
	Ok(sub_segments)
}

impl PointPen for Cu2QuPointPen<'_> {
	fn begin_path(&mut self, _: Option<&str>) -> Result<()> {
		if self.current.is_some() {
			return Err(Error::Pen("Path already begun.".into()));
		}
		self.current = Some(Vec::new());
		Ok(())
	}

	fn end_path(&mut self) -> Result<()> {
		let points = self
			.current
			.take()
			.ok_or_else(|| Error::Pen("Path not begun.".into()))?;
		if points.is_empty() {
			return Ok(());
		}
		self.flush_contour(contour_segments(points))
	}

	fn add_point(
		&mut self,
		pt: Point,
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		self.current
			.as_mut()
			.ok_or_else(|| Error::Pen("Path not begun".into()))?
			.push(PenPoint {
				pt: Some(pt),
				segment_type,
				smooth,
				name: name.map(str::to_owned),
				identifier: identifier.map(str::to_owned),
			});
		Ok(())
	}

	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: Transform,
		_: Option<&str>,
	) -> Result<()> {
		if self.current.is_some() {
			return Err(Error::Usage("AssertionError".into()));
		}
		self.pen
			.pen()
			.add_component(base_glyph_name, transformation, None)
	}
}

/// Converts the cubic curves of several interpolation-compatible outlines
/// to quadratic splines with the same number of points (`Cu2QuMultiPen`).
/// Each call takes one argument per pen.
pub struct Cu2QuMultiPen<'p> {
	pens: Vec<Out<'p>>,
	/// `max_err`.
	pub max_err: f64,
	start_pts: Option<Vec<Point>>,
	current_pts: Option<Vec<Point>>,
}

impl std::fmt::Debug for Cu2QuMultiPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Cu2QuMultiPen")
			.field("max_err", &self.max_err)
			.field("current_pts", &self.current_pts)
			.finish_non_exhaustive()
	}
}

impl<'p> Cu2QuMultiPen<'p> {
	/// `Cu2QuMultiPen(other_pens, max_err, reverse_direction)`.
	pub fn new(other_pens: Vec<&'p mut dyn Pen>, max_err: f64, reverse_direction: bool) -> Self {
		Cu2QuMultiPen {
			pens: other_pens
				.into_iter()
				.map(|p| {
					if reverse_direction {
						Out::Reverse(ReverseContourPen::new(p, true))
					} else {
						Out::Plain(p)
					}
				})
				.collect(),
			max_err,
			start_pts: None,
			current_pts: None,
		}
	}

	fn check_open(&self) -> Result<()> {
		if self.current_pts.is_none() {
			return Err(Error::Usage("AssertionError: moveTo is required".into()));
		}
		Ok(())
	}

	fn check_closed(&self) -> Result<()> {
		if self.current_pts.is_some() {
			return Err(Error::Usage(
				"AssertionError: closePath or endPath is required".into(),
			));
		}
		Ok(())
	}

	fn add_move_to(&mut self) -> Result<()> {
		if let Some(start) = self.start_pts.take() {
			for (pt, pen) in start.iter().zip(&mut self.pens) {
				pen.pen().move_to(*pt)?;
			}
		}
		Ok(())
	}

	/// `moveTo(pts)`.
	pub fn move_to(&mut self, pts: Vec<Point>) -> Result<()> {
		self.check_closed()?;
		self.start_pts = Some(pts.clone());
		self.current_pts = Some(pts);
		self.add_move_to()
	}

	/// `lineTo(pts)`.
	pub fn line_to(&mut self, pts: Vec<Point>) -> Result<()> {
		self.check_open()?;
		self.add_move_to()?;
		for (pt, pen) in pts.iter().zip(&mut self.pens) {
			pen.pen().line_to(*pt)?;
		}
		self.current_pts = Some(pts);
		Ok(())
	}

	/// `qCurveTo(pointsList)`: for each pen, the off-curve points and the
	/// on-curve point.
	pub fn q_curve_to(&mut self, points_list: Vec<Vec<Point>>) -> Result<()> {
		self.check_open()?;
		if points_list.first().is_some_and(|p| p.len() == 1) {
			return self.line_to(points_list.into_iter().map(|p| p[0]).collect());
		}
		self.add_move_to()?;
		let mut current = Vec::new();
		for (points, pen) in points_list.iter().zip(&mut self.pens) {
			let (on, off) = points
				.split_last()
				.map_or((None, &[][..]), |(l, r)| (Some(*l), r));
			pen.pen().q_curve_to(off, on)?;
			current.extend(on);
		}
		self.current_pts = Some(current);
		Ok(())
	}

	/// `curveTo(pointsList)`: for each pen, the three points of a cubic curve.
	pub fn curve_to(&mut self, points_list: Vec<[Point; 3]>) -> Result<()> {
		self.check_open()?;
		let current = self.current_pts.clone().unwrap_or_default();
		let curves: Vec<[Point; 4]> = current
			.iter()
			.zip(&points_list)
			.map(|(&c, p)| [c, p[0], p[1], p[2]])
			.collect();
		let max_errors = vec![self.max_err; curves.len()];
		let quadratics = curves_to_quadratic(&curves, &max_errors, true).map_err(cu2qu_error)?;
		self.q_curve_to(quadratics.into_iter().map(|q| q[1..].to_vec()).collect())
	}

	/// `closePath`.
	pub fn close_path(&mut self) -> Result<()> {
		self.check_open()?;
		if self.start_pts.is_none() {
			for pen in &mut self.pens {
				pen.pen().close_path()?;
			}
		}
		self.current_pts = None;
		self.start_pts = None;
		Ok(())
	}

	/// `endPath`.
	pub fn end_path(&mut self) -> Result<()> {
		self.check_open()?;
		if self.start_pts.is_none() {
			for pen in &mut self.pens {
				pen.pen().end_path()?;
			}
		}
		self.current_pts = None;
		self.start_pts = None;
		Ok(())
	}

	/// `addComponent(glyphName, transformations)`.
	pub fn add_component(&mut self, glyph_name: &str, transformations: &[Transform]) -> Result<()> {
		self.check_closed()?;
		for (t, pen) in transformations.iter().zip(&mut self.pens) {
			pen.pen().add_component(glyph_name, *t)?;
		}
		Ok(())
	}
}
