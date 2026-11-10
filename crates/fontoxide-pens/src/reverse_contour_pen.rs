//! A pen reversing the direction of contours
//! (`fontTools.pens.reverseContourPen`).

use fontoxide_misc::array_tools::pairwise;

use crate::recording_pen::PenOp;
use crate::{Error, Pen, Point, Result, Transform};

/// Passes contours on to another pen with their direction reversed, keeping
/// the first point of closed contours (`ReverseContourPen`). Components are
/// passed on unchanged.
pub struct ReverseContourPen<'p> {
	out: &'p mut dyn Pen,
	/// Whether the line closing a closed contour is drawn even when it is
	/// implied (`outputImpliedClosingLine`).
	pub output_implied_closing_line: bool,
	value: Vec<PenOp>,
}

impl std::fmt::Debug for ReverseContourPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ReverseContourPen")
			.field(
				"output_implied_closing_line",
				&self.output_implied_closing_line,
			)
			.field("value", &self.value)
			.finish_non_exhaustive()
	}
}

impl<'p> ReverseContourPen<'p> {
	/// `ReverseContourPen(outPen, outputImpliedClosingLine)`.
	pub fn new(out: &'p mut dyn Pen, output_implied_closing_line: bool) -> Self {
		ReverseContourPen {
			out,
			output_implied_closing_line,
			value: Vec::new(),
		}
	}

	fn flush_contour(&mut self) -> Result<()> {
		let contour = std::mem::take(&mut self.value);
		for op in reversed_contour(contour, self.output_implied_closing_line)? {
			op.replay(self.out)?;
		}
		Ok(())
	}
}

impl Pen for ReverseContourPen<'_> {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.value.push(PenOp::MoveTo(pt));
		Ok(())
	}
	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.value.push(PenOp::LineTo(pt));
		Ok(())
	}
	fn curve_to(&mut self, points: &[Point]) -> Result<()> {
		self.value.push(PenOp::CurveTo(points.to_vec()));
		Ok(())
	}
	fn q_curve_to(&mut self, off_curves: &[Point], on_curve: Option<Point>) -> Result<()> {
		self.value
			.push(PenOp::QCurveTo(off_curves.to_vec(), on_curve));
		Ok(())
	}
	fn close_path(&mut self) -> Result<()> {
		self.value.push(PenOp::ClosePath);
		self.flush_contour()
	}
	fn end_path(&mut self) -> Result<()> {
		self.value.push(PenOp::EndPath);
		self.flush_contour()
	}
	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> Result<()> {
		self.out.add_component(glyph_name, transformation)
	}
}

/// The points of a segment, with `None` for the missing on-curve point of an
/// off-curve-only `qCurveTo`.
type Pts = Vec<Option<Point>>;

/// A segment from its operator and points.
fn make(op: &'static str, pts: Pts) -> Result<PenOp> {
	let all = |pts: Pts| -> Result<Vec<Point>> {
		pts.into_iter()
			.collect::<Option<Vec<_>>>()
			.ok_or_else(|| Error::Usage("TypeError: a point is None".into()))
	};
	Ok(match op {
		"moveTo" => PenOp::MoveTo(single(pts)?),
		"lineTo" => PenOp::LineTo(single(pts)?),
		"curveTo" => PenOp::CurveTo(all(pts)?),
		"qCurveTo" => {
			let mut pts = pts;
			let on = pts.pop().flatten();
			PenOp::QCurveTo(all(pts)?, on)
		}
		_ => return Err(Error::Usage(format!("AssertionError: {op}"))),
	})
}

fn single(pts: Pts) -> Result<Point> {
	match pts[..] {
		[Some(p)] => Ok(p),
		_ => Err(Error::Usage(format!(
			"TypeError: takes 1 point ({} given)",
			pts.len()
		))),
	}
}

/// `reversedContour`: the calls drawing a contour (from `moveTo` or an
/// off-curve-only `qCurveTo` to `closePath` or `endPath`), reversed.
pub fn reversed_contour(
	contour: Vec<PenOp>,
	output_implied_closing_line: bool,
) -> Result<Vec<PenOp>> {
	if contour.is_empty() {
		return Ok(Vec::new());
	}
	let assert = |ok: bool, msg: &str| -> Result<()> {
		if ok {
			Ok(())
		} else {
			Err(Error::Usage(format!("AssertionError: {msg}")))
		}
	};
	// Valid contours have a starting and an ending command.
	assert(contour.len() > 1, "invalid contour")?;
	let mut contour: Vec<(&'static str, Pts)> = contour
		.iter()
		.map(|op| (op.operator(), op.points()))
		.collect();

	// The type of the last command tells whether the contour is closed.
	let contour_type = contour.pop().map(|c| c.0).unwrap_or_default();
	assert(
		matches!(contour_type, "endPath" | "closePath"),
		"contourType",
	)?;
	let mut closed = contour_type == "closePath";

	let (first_type, mut first_pts) = contour.remove(0);
	assert(
		matches!(first_type, "moveTo" | "qCurveTo"),
		&format!("invalid initial segment type: '{first_type}'"),
	)?;
	let first_on_curve = first_pts.last().copied().flatten();
	if first_type == "qCurveTo" {
		// A TrueType path made of off-curve points only.
		assert(
			first_on_curve.is_none(),
			"off-curve only paths must end with 'None'",
		)?;
		assert(
			contour.is_empty(),
			"only one qCurveTo allowed per off-curve path",
		)?;
		let n = first_pts.len();
		let mut pts = vec![first_pts[0]];
		pts.extend(first_pts[1..n - 1].iter().rev());
		pts.push(None);
		first_pts = pts;
	}

	let mut out = Vec::new();
	if contour.is_empty() {
		// A single segment: nothing to reverse.
		// Single-point paths cannot be closed; off-curve paths are closed.
		closed = first_type != "moveTo";
		out.push(make(first_type, first_pts)?);
	} else {
		let (last_type, last_pts) = contour[contour.len() - 1].clone();
		let last_on_curve = last_pts.last().copied().flatten();
		let with_end = |pts: &Pts, end: &[Option<Point>]| -> Pts {
			let mut v = pts[..pts.len() - 1].to_vec();
			v.extend_from_slice(end);
			v
		};
		if closed {
			// Closed paths keep their starting point.
			out.push(make(first_type, first_pts.clone())?);
			if first_on_curve != last_on_curve {
				// An implied line between the last and the first points.
				out.push(make("lineTo", vec![last_on_curve])?);
				let n = contour.len();
				contour[n - 1] = (last_type, with_end(&last_pts, &[first_on_curve]));
			}
			let (second_type, second_pts) = if contour.len() > 1 {
				contour[0].clone()
			} else {
				// Two points: the second and last are the same.
				(last_type, last_pts.clone())
			};
			if !output_implied_closing_line {
				// A lineTo after the initial moveTo is implied by the closePath
				// once reversed, unless both overlap: then the duplicate point
				// is kept.
				if second_type == "lineTo" && first_pts != second_pts {
					contour.remove(0);
					if !contour.is_empty() {
						let n = contour.len();
						contour[n - 1] = (last_type, with_end(&last_pts, &second_pts));
					}
				}
			}
		} else {
			// The last point of an open path becomes the first.
			out.push(make(first_type, vec![last_on_curve])?);
			let n = contour.len();
			contour[n - 1] = (last_type, with_end(&last_pts, &[first_on_curve]));
		}

		// Each segment, in reverse order, with its off-curve points reversed
		// and the on-curve point of the following segment.
		for ((cur_type, cur_pts), (_, next_pts)) in
			pairwise(&contour.iter().collect::<Vec<_>>(), true)
				.into_iter()
				.map(|(a, b)| (a.clone(), b.clone()))
		{
			let mut pts: Pts = cur_pts[..cur_pts.len() - 1].iter().rev().copied().collect();
			pts.push(next_pts.last().copied().flatten());
			out.push(make(cur_type, pts)?);
		}
	}
	out.push(if closed {
		PenOp::ClosePath
	} else {
		PenOp::EndPath
	});
	Ok(out)
}
