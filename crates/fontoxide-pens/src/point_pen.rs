//! Point pens and the adapters between them and segment pens
//! (`fontTools.pens.pointPen`).

use crate::{
	DecomposedTransform, Error, Location, Pen, Point, PointPen, Result, SegmentType, Transform,
};

/// A point as a point pen receives it.
#[derive(Debug, Clone, PartialEq)]
pub struct PenPoint {
	/// `pt`; `None` only for the implied end of a contour without on-curve
	/// points.
	pub pt: Option<Point>,
	/// `segmentType`.
	pub segment_type: Option<SegmentType>,
	/// `smooth`.
	pub smooth: bool,
	/// `name`.
	pub name: Option<String>,
	/// `identifier`.
	pub identifier: Option<String>,
}

/// A segment of a contour: its type and its points (`(segmentType, points)`).
pub type Segment = (SegmentType, Vec<PenPoint>);

/// `BasePointToSegmentPen.endPath`: the segments of a contour given as
/// points. A closed contour is rotated to end with an on-curve point; an open
/// one starts with a `move` segment. A contour without on-curve points gets a
/// `None` point ending a `qcurve` segment.
pub fn contour_segments(mut points: Vec<PenPoint>) -> Vec<Segment> {
	if points.is_empty() {
		return Vec::new();
	}
	if points.len() == 1 {
		// Not much more to do than output a single move segment.
		return vec![(SegmentType::Move, points)];
	}
	let mut segments = Vec::new();
	if points[0].segment_type == Some(SegmentType::Move) {
		// An open contour: a "move" segment for the first point.
		let first = points.remove(0);
		segments.push((SegmentType::Move, vec![first]));
	} else {
		// A closed contour: rotate the points to end with an on-curve one.
		match points.iter().position(|p| p.segment_type.is_some()) {
			None => points.push(PenPoint {
				pt: None,
				segment_type: Some(SegmentType::QCurve),
				smooth: false,
				name: None,
				identifier: None,
			}),
			Some(first_on_curve) => points.rotate_left(first_on_curve + 1),
		}
	}
	let mut current = Vec::new();
	for p in points {
		let st = p.segment_type;
		current.push(p);
		if let Some(st) = st {
			segments.push((st, std::mem::take(&mut current)));
		}
	}
	segments
}

/// The contour being collected by a point pen that works by contour.
#[derive(Debug, Default)]
struct ContourBuffer {
	current: Option<Vec<PenPoint>>,
}

impl ContourBuffer {
	fn begin(&mut self) -> Result<()> {
		if self.current.is_some() {
			return Err(Error::Pen("Path already begun.".into()));
		}
		self.current = Some(Vec::new());
		Ok(())
	}

	fn end(&mut self) -> Result<Vec<PenPoint>> {
		self.current
			.take()
			.ok_or_else(|| Error::Pen("Path not begun.".into()))
	}

	fn add(&mut self, p: PenPoint) -> Result<()> {
		self.current
			.as_mut()
			.ok_or_else(|| Error::Pen("Path not begun".into()))?
			.push(p);
		Ok(())
	}
}

/// Draws what it receives as a point pen onto a segment pen
/// (`PointToSegmentPen`). Point names and identifiers are dropped.
pub struct PointToSegmentPen<'p> {
	pen: &'p mut dyn Pen,
	/// Whether the line closing a closed contour is drawn even when it is
	/// implied (`outputImpliedClosingLine`).
	pub output_implied_closing_line: bool,
	buffer: ContourBuffer,
}

impl std::fmt::Debug for PointToSegmentPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("PointToSegmentPen")
			.field(
				"output_implied_closing_line",
				&self.output_implied_closing_line,
			)
			.finish_non_exhaustive()
	}
}

impl<'p> PointToSegmentPen<'p> {
	/// `PointToSegmentPen(segmentPen, outputImpliedClosingLine)`.
	pub fn new(pen: &'p mut dyn Pen, output_implied_closing_line: bool) -> Self {
		PointToSegmentPen {
			pen,
			output_implied_closing_line,
			buffer: ContourBuffer::default(),
		}
	}

	/// `_flushContour`.
	fn flush_contour(&mut self, mut segments: Vec<Segment>) -> Result<()> {
		if segments.is_empty() {
			return Err(Error::Pen("Must have at least one segment.".into()));
		}
		let pen = &mut *self.pen;
		let closed;
		let move_pt;
		if segments[0].0 == SegmentType::Move {
			// An open path.
			closed = false;
			let points = &segments[0].1;
			if points.len() != 1 {
				return Err(Error::Pen(format!(
					"Illegal move segment point count: {}",
					points.len()
				)));
			}
			move_pt = points[0].pt;
			segments.remove(0);
		} else {
			// A closed path: a moveTo to the last point of the last segment.
			closed = true;
			move_pt = segments.last().and_then(|s| s.1.last()).and_then(|p| p.pt);
		}
		if let Some(pt) = move_pt {
			// For a contour without on-curve points, there is no moveTo.
			pen.move_to(pt)?;
		}
		let n_segments = segments.len();
		let mut last_pt = move_pt;
		for (i, (segment_type, points)) in segments.into_iter().enumerate() {
			let pts: Vec<Option<Point>> = points.iter().map(|p| p.pt).collect();
			match segment_type {
				SegmentType::Line => {
					if pts.len() != 1 {
						return Err(Error::Pen(format!(
							"Illegal line segment point count: {}",
							pts.len()
						)));
					}
					let pt = pts[0];
					// For closed contours, a lineTo from the last on-curve point to
					// the start is implied, unless both are the same point: then it
					// is drawn to keep the duplicate point (fontmake#572).
					if i + 1 != n_segments
						|| self.output_implied_closing_line
						|| !closed || pt == last_pt
					{
						if let Some(pt) = pt {
							pen.line_to(pt)?;
						}
						last_pt = pt;
					}
				}
				SegmentType::Curve => {
					let pts: Vec<Point> = pts.iter().flatten().copied().collect();
					pen.curve_to(&pts)?;
					last_pt = pts.last().copied();
				}
				SegmentType::QCurve => {
					let (on, off) = pts.split_last().map_or((None, &[][..]), |(l, r)| (*l, r));
					let off: Vec<Point> = off.iter().flatten().copied().collect();
					pen.q_curve_to(&off, on)?;
					last_pt = on;
				}
				SegmentType::Move => {
					return Err(Error::Pen("Illegal segmentType: move".into()));
				}
			}
		}
		if closed {
			pen.close_path()
		} else {
			pen.end_path()
		}
	}
}

impl PointPen for PointToSegmentPen<'_> {
	fn begin_path(&mut self, _: Option<&str>) -> Result<()> {
		self.buffer.begin()
	}

	fn end_path(&mut self) -> Result<()> {
		let points = self.buffer.end()?;
		if points.is_empty() {
			return Ok(());
		}
		let segments = contour_segments(points);
		self.flush_contour(segments)
	}

	fn add_point(
		&mut self,
		pt: Point,
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		self.buffer.add(PenPoint {
			pt: Some(pt),
			segment_type,
			smooth,
			name: name.map(str::to_owned),
			identifier: identifier.map(str::to_owned),
		})
	}

	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: Transform,
		_: Option<&str>,
	) -> Result<()> {
		self.pen.add_component(base_glyph_name, transformation)
	}
}

/// Draws what it receives as a segment pen onto a point pen
/// (`SegmentToPointPen`).
pub struct SegmentToPointPen<'p> {
	pen: GuessOrNot<'p>,
	contour: Option<Vec<(Point, Option<SegmentType>)>>,
}

enum GuessOrNot<'p> {
	Guess(GuessSmoothPointPen<'p>),
	Plain(&'p mut dyn PointPen),
}

impl GuessOrNot<'_> {
	fn pen(&mut self) -> &mut dyn PointPen {
		match self {
			GuessOrNot::Guess(p) => p,
			GuessOrNot::Plain(p) => *p,
		}
	}
}

impl std::fmt::Debug for SegmentToPointPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("SegmentToPointPen")
			.field("contour", &self.contour)
			.finish_non_exhaustive()
	}
}

impl<'p> SegmentToPointPen<'p> {
	/// `SegmentToPointPen(pointPen, guessSmooth)`: with `guess_smooth`, the
	/// points go through a [`GuessSmoothPointPen`].
	pub fn new(pen: &'p mut dyn PointPen, guess_smooth: bool) -> Self {
		SegmentToPointPen {
			pen: if guess_smooth {
				GuessOrNot::Guess(GuessSmoothPointPen::new(pen, 0.05))
			} else {
				GuessOrNot::Plain(pen)
			},
			contour: None,
		}
	}

	fn flush_contour(&mut self, contour: Vec<(Point, Option<SegmentType>)>) -> Result<()> {
		let pen = self.pen.pen();
		pen.begin_path(None)?;
		for (pt, segment_type) in contour {
			pen.add_point(pt, segment_type, false, None, None)?;
		}
		pen.end_path()
	}

	fn contour(&mut self) -> Result<&mut Vec<(Point, Option<SegmentType>)>> {
		self.contour
			.as_mut()
			.ok_or_else(|| Error::Pen("Contour missing required initial moveTo".into()))
	}
}

impl Pen for SegmentToPointPen<'_> {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.contour = Some(vec![(pt, Some(SegmentType::Move))]);
		Ok(())
	}

	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.contour()?.push((pt, Some(SegmentType::Line)));
		Ok(())
	}

	fn curve_to(&mut self, points: &[Point]) -> Result<()> {
		let Some((&last, off)) = points.split_last() else {
			return Err(Error::Usage(
				"TypeError: Must pass in at least one point".into(),
			));
		};
		let contour = self.contour()?;
		contour.extend(off.iter().map(|&p| (p, None)));
		contour.push((last, Some(SegmentType::Curve)));
		Ok(())
	}

	fn q_curve_to(&mut self, off_curves: &[Point], on_curve: Option<Point>) -> Result<()> {
		if off_curves.is_empty() && on_curve.is_none() {
			return Err(Error::Usage(
				"TypeError: Must pass in at least one point".into(),
			));
		}
		if on_curve.is_none() {
			self.contour = Some(Vec::new());
		}
		let contour = self.contour()?;
		contour.extend(off_curves.iter().map(|&p| (p, None)));
		if let Some(on) = on_curve {
			contour.push((on, Some(SegmentType::QCurve)));
		}
		Ok(())
	}

	fn close_path(&mut self) -> Result<()> {
		let mut contour = self
			.contour
			.take()
			.ok_or_else(|| Error::Pen("Contour missing required initial moveTo".into()))?;
		// Remove the last point if it duplicates the first, but only if both
		// are on-curve points; off-curve-only contours keep every point
		// (fonttools#4014).
		let n = contour.len();
		if n > 1
			&& contour[0].0 == contour[n - 1].0
			&& contour[0].1.is_some()
			&& contour[n - 1].1.is_some()
		{
			contour[0] = contour[n - 1];
			contour.pop();
		} else if contour[0].1 == Some(SegmentType::Move) {
			// The closing line is implied: the first point ends a line.
			contour[0].1 = Some(SegmentType::Line);
		}
		self.flush_contour(contour)
	}

	fn end_path(&mut self) -> Result<()> {
		let contour = self
			.contour
			.take()
			.ok_or_else(|| Error::Pen("Contour missing required initial moveTo".into()))?;
		self.flush_contour(contour)
	}

	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> Result<()> {
		if self.contour.is_some() {
			return Err(Error::Pen(
				"Components must be added before or after contours".into(),
			));
		}
		self.pen
			.pen()
			.add_component(glyph_name, transformation, None)
	}
}

/// Passes points on, marking the on-curve points next to an off-curve one as
/// smooth where the contour does not turn (`GuessSmoothPointPen`).
pub struct GuessSmoothPointPen<'p> {
	out: &'p mut dyn PointPen,
	error: f64,
	points: Option<Vec<PenPoint>>,
}

impl std::fmt::Debug for GuessSmoothPointPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("GuessSmoothPointPen")
			.field("error", &self.error)
			.finish_non_exhaustive()
	}
}

impl<'p> GuessSmoothPointPen<'p> {
	/// `GuessSmoothPointPen(outPen, error)`; the angle tolerance `error` is
	/// 0.05 in fontTools.
	pub fn new(out: &'p mut dyn PointPen, error: f64) -> Self {
		GuessSmoothPointPen {
			out,
			error,
			points: None,
		}
	}

	fn flush_contour(&mut self) -> Result<()> {
		let mut points = self
			.points
			.take()
			.ok_or_else(|| Error::Pen("Path not begun".into()))?;
		let n = points.len() as isize;
		if n == 0 {
			return Ok(());
		}
		let indices = if points[0].segment_type == Some(SegmentType::Move) {
			// An open path.
			1..n - 1
		} else if n > 1 {
			// A closed path; Python starts at index -1.
			-1..n - 1
		} else {
			// A closed path with a single point: ignored.
			0..0
		};
		let at = |i: isize| ((i + n) % n) as usize;
		for i in indices {
			let pi = at(i);
			if points[pi].segment_type.is_none() {
				continue;
			}
			let (prev, next) = (at(i - 1), at(i + 1));
			if points[prev].segment_type.is_some() && points[next].segment_type.is_some() {
				continue;
			}
			// At least one of the neighbors is an off-curve point.
			let (Some(pt), Some(prev_pt), Some(next_pt)) =
				(points[pi].pt, points[prev].pt, points[next].pt)
			else {
				continue;
			};
			if pt != prev_pt && pt != next_pt {
				let (dx1, dy1) = (pt.0 - prev_pt.0, pt.1 - prev_pt.1);
				let (dx2, dy2) = (next_pt.0 - pt.0, next_pt.1 - pt.1);
				let a1 = dy1.atan2(dx1);
				let a2 = dy2.atan2(dx2);
				if (a1 - a2).abs() < self.error {
					points[pi].smooth = true;
				}
			}
		}
		for p in points {
			if let Some(pt) = p.pt {
				self.out.add_point(
					pt,
					p.segment_type,
					p.smooth,
					p.name.as_deref(),
					p.identifier.as_deref(),
				)?;
			}
		}
		Ok(())
	}
}

impl PointPen for GuessSmoothPointPen<'_> {
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()> {
		if self.points.is_some() {
			return Err(Error::Pen("Path already begun".into()));
		}
		self.points = Some(Vec::new());
		self.out.begin_path(identifier)
	}

	fn end_path(&mut self) -> Result<()> {
		self.flush_contour()?;
		self.out.end_path()
	}

	fn add_point(
		&mut self,
		pt: Point,
		segment_type: Option<SegmentType>,
		_smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		self.points
			.as_mut()
			.ok_or_else(|| Error::Pen("Path not begun".into()))?
			.push(PenPoint {
				pt: Some(pt),
				segment_type,
				smooth: false,
				name: name.map(str::to_owned),
				identifier: identifier.map(str::to_owned),
			});
		Ok(())
	}

	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: Transform,
		identifier: Option<&str>,
	) -> Result<()> {
		if self.points.is_some() {
			return Err(Error::Pen(
				"Components must be added before or after contours".into(),
			));
		}
		self.out
			.add_component(base_glyph_name, transformation, identifier)
	}

	fn add_var_component(
		&mut self,
		glyph_name: &str,
		transformation: DecomposedTransform,
		location: &Location,
		identifier: Option<&str>,
	) -> Result<()> {
		if self.points.is_some() {
			return Err(Error::Pen(
				"VarComponents must be added before or after contours".into(),
			));
		}
		self.out
			.add_var_component(glyph_name, transformation, location, identifier)
	}
}

/// Passes contours on to another point pen with their direction reversed,
/// keeping the first point of closed contours (`ReverseContourPointPen`).
pub struct ReverseContourPointPen<'p> {
	pen: &'p mut dyn PointPen,
	current: Option<Vec<PenPoint>>,
	identifier: Option<String>,
}

impl std::fmt::Debug for ReverseContourPointPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ReverseContourPointPen")
			.field("current", &self.current)
			.finish_non_exhaustive()
	}
}

impl<'p> ReverseContourPointPen<'p> {
	/// `ReverseContourPointPen(outputPointPen)`.
	pub fn new(pen: &'p mut dyn PointPen) -> Self {
		ReverseContourPointPen {
			pen,
			current: None,
			identifier: None,
		}
	}

	fn flush_contour(&mut self, mut contour: Vec<PenPoint>) -> Result<()> {
		let pen = &mut *self.pen;
		let identifier = self.identifier.as_deref();
		if contour.is_empty() {
			pen.begin_path(identifier)?;
			return pen.end_path();
		}
		let closed = contour[0].segment_type != Some(SegmentType::Move);
		let mut last_segment_type;
		if !closed {
			last_segment_type = Some(SegmentType::Move);
		} else {
			// Move the first point to the end: once reversed, it is first again.
			let first = contour.remove(0);
			contour.push(first);
			last_segment_type = contour.iter().find_map(|p| p.segment_type);
		}
		contour.reverse();
		if !closed {
			// Open paths start with a move: drop the leading off-curve points.
			while contour.first().is_some_and(|p| p.segment_type.is_none()) {
				contour.remove(0);
			}
		}
		pen.begin_path(identifier)?;
		for p in contour {
			let segment_type = if p.segment_type.is_some() {
				let st = last_segment_type;
				last_segment_type = p.segment_type;
				st
			} else {
				None
			};
			if let Some(pt) = p.pt {
				pen.add_point(
					pt,
					segment_type,
					p.smooth,
					p.name.as_deref(),
					p.identifier.as_deref(),
				)?;
			}
		}
		pen.end_path()
	}
}

impl PointPen for ReverseContourPointPen<'_> {
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()> {
		if self.current.is_some() {
			return Err(Error::Pen("Path already begun".into()));
		}
		self.current = Some(Vec::new());
		self.identifier = identifier.map(str::to_owned);
		Ok(())
	}

	fn end_path(&mut self) -> Result<()> {
		let contour = self
			.current
			.take()
			.ok_or_else(|| Error::Pen("Path not begun".into()))?;
		self.flush_contour(contour)
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
		identifier: Option<&str>,
	) -> Result<()> {
		if self.current.is_some() {
			return Err(Error::Pen(
				"Components must be added before or after contours".into(),
			));
		}
		self.pen
			.add_component(base_glyph_name, transformation, identifier)
	}
}
