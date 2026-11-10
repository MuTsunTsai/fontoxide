//! A pen making the closing line of contours explicit
//! (`fontTools.pens.explicitClosingLinePen`).

use crate::filter_pen::ContourFilterPen;
use crate::recording_pen::PenOp;
use crate::{Pen, Result};

/// `ExplicitClosingLinePen.filterContour`: a `lineTo` back to the first
/// point is added to a closed contour whose last segment ends elsewhere.
pub fn explicit_closing_line(mut contour: Vec<PenOp>) -> Result<Vec<PenOp>> {
	let n = contour.len();
	if n < 3 || !matches!(contour[0], PenOp::MoveTo(_)) || contour[n - 1] != PenOp::ClosePath {
		return Ok(contour);
	}
	let PenOp::MoveTo(move_pt) = contour[0] else {
		return Ok(contour);
	};
	let last_seg = contour[n - 2].points();
	if let Some(&last) = last_seg.last()
		&& Some(move_pt) != last
	{
		contour.truncate(n - 1);
		contour.push(PenOp::LineTo(move_pt));
		contour.push(PenOp::ClosePath);
	}
	Ok(contour)
}

/// Adds an explicit `lineTo` to the first point of each closed contour that
/// does not end there already (`ExplicitClosingLinePen`).
pub type ExplicitClosingLinePen<'p> = ContourFilterPen<'p, fn(Vec<PenOp>) -> Result<Vec<PenOp>>>;

/// `ExplicitClosingLinePen(outPen)`.
pub fn explicit_closing_line_pen(out: &mut dyn Pen) -> ExplicitClosingLinePen<'_> {
	ContourFilterPen::new(out, explicit_closing_line)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::recording_pen::RecordingPen;

	fn run(draw: impl FnOnce(&mut dyn Pen) -> Result<()>) -> Result<Vec<PenOp>> {
		let mut rec = RecordingPen::new();
		{
			let mut pen = explicit_closing_line_pen(&mut rec);
			draw(&mut pen)?;
		}
		Ok(rec.value)
	}

	// The doctests of `ExplicitClosingLinePen`.
	#[test]
	fn doctests() -> Result<()> {
		use PenOp::*;
		let o = (0.0, 0.0);
		let curve = || CurveTo(vec![(100.0, 0.0), (0.0, 100.0), (100.0, 100.0)]);
		assert_eq!(
			run(|p| {
				p.move_to(o)?;
				p.line_to((100.0, 0.0))?;
				p.line_to((100.0, 100.0))?;
				p.close_path()
			})?,
			[
				MoveTo(o),
				LineTo((100.0, 0.0)),
				LineTo((100.0, 100.0)),
				LineTo(o),
				ClosePath
			]
		);
		assert_eq!(
			run(|p| {
				p.move_to(o)?;
				p.line_to((100.0, 0.0))?;
				p.line_to((100.0, 100.0))?;
				p.line_to(o)?;
				p.close_path()
			})?,
			[
				MoveTo(o),
				LineTo((100.0, 0.0)),
				LineTo((100.0, 100.0)),
				LineTo(o),
				ClosePath
			]
		);
		assert_eq!(
			run(|p| {
				p.move_to(o)?;
				p.curve_to(&[(100.0, 0.0), (0.0, 100.0), (100.0, 100.0)])?;
				p.close_path()
			})?,
			[MoveTo(o), curve(), LineTo(o), ClosePath]
		);
		assert_eq!(
			run(|p| {
				p.move_to(o)?;
				p.curve_to(&[(100.0, 0.0), (0.0, 100.0), (100.0, 100.0)])?;
				p.line_to(o)?;
				p.close_path()
			})?,
			[MoveTo(o), curve(), LineTo(o), ClosePath]
		);
		assert_eq!(
			run(|p| {
				p.move_to(o)?;
				p.curve_to(&[(100.0, 0.0), (0.0, 100.0), o])?;
				p.close_path()
			})?,
			[
				MoveTo(o),
				CurveTo(vec![(100.0, 0.0), (0.0, 100.0), o]),
				ClosePath
			]
		);
		assert_eq!(
			run(|p| {
				p.move_to(o)?;
				p.close_path()
			})?,
			[MoveTo(o), ClosePath]
		);
		assert_eq!(run(|p| p.close_path())?, [ClosePath]);
		assert_eq!(
			run(|p| {
				p.move_to(o)?;
				p.line_to((100.0, 0.0))?;
				p.line_to((100.0, 100.0))?;
				p.end_path()
			})?,
			[
				MoveTo(o),
				LineTo((100.0, 0.0)),
				LineTo((100.0, 100.0)),
				EndPath
			]
		);
		Ok(())
	}
}
