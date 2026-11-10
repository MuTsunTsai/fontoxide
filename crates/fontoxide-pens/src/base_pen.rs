//! The base for drawing pens (`fontTools.pens.basePen`).

use fontoxide_misc::transform::Transform;

use crate::{Decomposer, Error, GlyphSet, Pen, Point, Result, ReverseFlipped};

/// `0.66666666666666667` in fontTools: the double just below 2/3. (The literal
/// `0.6666666666666667` would be the one just above.)
const TWO_THIRDS: f64 = 0.666_666_666_666_666_6;

/// The plain segments a [`BasePen`] reduces drawing to: what a `BasePen`
/// subclass overrides (`_moveTo`, `_lineTo`, `_curveToOne`, …).
pub trait SegmentPen {
	/// `_moveTo`.
	fn move_to(&mut self, pt: Point) -> Result<()>;

	/// `_lineTo`.
	fn line_to(&mut self, pt: Point) -> Result<()>;

	/// `_curveToOne`: a cubic segment from `current` (`_getCurrentPoint()`).
	fn curve_to_one(
		&mut self,
		current: Option<Point>,
		pt1: Point,
		pt2: Point,
		pt3: Point,
	) -> Result<()>;

	/// `_qCurveToOne`: a quadratic segment from `current`. By default it is drawn
	/// as the equivalent cubic segment.
	fn q_curve_to_one(&mut self, current: Option<Point>, pt1: Point, pt2: Point) -> Result<()> {
		let (pt0x, pt0y) = current.ok_or_else(|| {
			Error::Usage("TypeError: cannot unpack non-iterable NoneType object".into())
		})?;
		let (pt1x, pt1y) = pt1;
		let (pt2x, pt2y) = pt2;
		let mid1x = pt0x + TWO_THIRDS * (pt1x - pt0x);
		let mid1y = pt0y + TWO_THIRDS * (pt1y - pt0y);
		let mid2x = pt2x + TWO_THIRDS * (pt1x - pt2x);
		let mid2y = pt2y + TWO_THIRDS * (pt1y - pt2y);
		self.curve_to_one(current, (mid1x, mid1y), (mid2x, mid2y), pt2)
	}

	/// `_closePath`.
	fn close_path(&mut self) -> Result<()> {
		Ok(())
	}

	/// `_endPath`.
	fn end_path(&mut self) -> Result<()> {
		Ok(())
	}
}

/// A pen that splits curves into plain segments for a [`SegmentPen`], and draws
/// components from a glyph set (`BasePen`, with `DecomposingPen`).
pub struct BasePen<'g, S> {
	/// The pen the segments go to.
	pub inner: S,
	/// Where components are looked up (`glyphSet`).
	pub glyph_set: Option<&'g dyn GlyphSet>,
	/// Whether a missing component is skipped with a warning rather than an
	/// error (`skipMissingComponents`, true by default).
	pub skip_missing_components: bool,
	/// Whether flipped components are drawn with their contours reversed
	/// (`reverseFlipped`).
	pub reverse_flipped: bool,
	current: Option<Point>,
}

impl<S: std::fmt::Debug> std::fmt::Debug for BasePen<'_, S> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("BasePen")
			.field("inner", &self.inner)
			.field("has_glyph_set", &self.glyph_set.is_some())
			.field("skip_missing_components", &self.skip_missing_components)
			.field("current", &self.current)
			.finish()
	}
}

impl<'g, S> BasePen<'g, S> {
	/// A pen drawing to `inner`, with components from `glyph_set`.
	pub fn with_inner(inner: S, glyph_set: Option<&'g dyn GlyphSet>) -> Self {
		BasePen {
			inner,
			glyph_set,
			skip_missing_components: true,
			reverse_flipped: false,
			current: None,
		}
	}

	/// The current point (`_getCurrentPoint`).
	pub fn current_point(&self) -> Option<Point> {
		self.current
	}
}

impl<S: SegmentPen> Pen for BasePen<'_, S> {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.inner.move_to(pt)?;
		self.current = Some(pt);
		Ok(())
	}

	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.inner.line_to(pt)?;
		self.current = Some(pt);
		Ok(())
	}

	fn curve_to(&mut self, points: &[Point]) -> Result<()> {
		match points.len() {
			0 => Err(Error::Usage("AssertionError: n >= 0".into())),
			1 => self.line_to(points[0]),
			2 => self.q_curve_to(&points[..1], Some(points[1])),
			3 => {
				self.inner
					.curve_to_one(self.current, points[0], points[1], points[2])?;
				self.current = Some(points[2]);
				Ok(())
			}
			_ => {
				for (pt1, pt2, pt3) in decompose_super_bezier_segment(points)? {
					self.inner.curve_to_one(self.current, pt1, pt2, pt3)?;
					self.current = Some(pt3);
				}
				Ok(())
			}
		}
	}

	fn q_curve_to(&mut self, off_curves: &[Point], on_curve: Option<Point>) -> Result<()> {
		let mut points = off_curves.to_vec();
		match on_curve {
			Some(p) => points.push(p),
			None => {
				let (x, y) = *off_curves
					.last()
					.ok_or_else(|| Error::Usage("IndexError: tuple index out of range".into()))?;
				let (nx, ny) = off_curves[0];
				let implied = (0.5 * (x + nx), 0.5 * (y + ny));
				self.current = Some(implied);
				self.inner.move_to(implied)?;
				points.push(implied);
			}
		}
		if off_curves.is_empty() {
			return self.line_to(points[0]);
		}
		for (pt1, pt2) in decompose_quadratic_segment(&points)? {
			self.inner.q_curve_to_one(self.current, pt1, pt2)?;
			self.current = Some(pt2);
		}
		Ok(())
	}

	fn close_path(&mut self) -> Result<()> {
		self.inner.close_path()?;
		self.current = None;
		Ok(())
	}

	fn end_path(&mut self) -> Result<()> {
		self.inner.end_path()?;
		self.current = None;
		Ok(())
	}

	/// Draws the base glyph onto this pen, transformed (`DecomposingPen`).
	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> Result<()> {
		let glyph_set = self.glyph_set.ok_or_else(|| {
			Error::Usage("TypeError: 'NoneType' object is not subscriptable".into())
		})?;
		Decomposer {
			glyph_set,
			skip_missing_components: self.skip_missing_components,
			reverse_flipped: if self.reverse_flipped {
				ReverseFlipped::KeepStart
			} else {
				ReverseFlipped::No
			},
		}
		.draw_component(self, glyph_name, transformation)
	}
}

/// `decomposeSuperBezierSegment`: splits a "super Bézier" (off-curve points,
/// then the end point; at least three points) into cubic segments.
pub fn decompose_super_bezier_segment(points: &[Point]) -> Result<Vec<(Point, Point, Point)>> {
	if points.len() < 3 {
		return Err(Error::Usage("AssertionError: n > 1".into()));
	}
	let n = points.len() - 1;
	let mut segments = Vec::new();
	let mut pt1 = points[0];
	let mut pt2: Option<Point> = None;
	for i in 2..=n {
		let divisions = i.min(3).min(n - i + 2);
		for j in 1..divisions {
			let factor = j as f64 / divisions as f64;
			let temp1 = points[i - 1];
			let temp2 = points[i - 2];
			let temp = (
				temp2.0 + factor * (temp1.0 - temp2.0),
				temp2.1 + factor * (temp1.1 - temp2.1),
			);
			match pt2 {
				None => pt2 = Some(temp),
				Some(p2) => {
					let pt3 = (0.5 * (p2.0 + temp.0), 0.5 * (p2.1 + temp.1));
					segments.push((pt1, p2, pt3));
					pt1 = temp;
					pt2 = None;
				}
			}
		}
	}
	segments.push((pt1, points[n - 1], points[n]));
	Ok(segments)
}

/// `decomposeQuadraticSegment`: splits quadratic curves (off-curve points, then
/// the end point; at least two points) into single quadratic segments.
pub fn decompose_quadratic_segment(points: &[Point]) -> Result<Vec<(Point, Point)>> {
	if points.len() < 2 {
		return Err(Error::Usage("AssertionError: n > 0".into()));
	}
	let n = points.len() - 1;
	let mut segments = Vec::with_capacity(n);
	for i in 0..n - 1 {
		let (x, y) = points[i];
		let (nx, ny) = points[i + 1];
		let implied = (0.5 * (x + nx), 0.5 * (y + ny));
		segments.push((points[i], implied));
	}
	segments.push((points[n - 1], points[n]));
	Ok(segments)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[derive(Debug, Default)]
	struct Log(Vec<String>);

	impl SegmentPen for Log {
		fn move_to(&mut self, pt: Point) -> Result<()> {
			self.0.push(format!("{} {} moveto", pt.0, pt.1));
			Ok(())
		}
		fn line_to(&mut self, pt: Point) -> Result<()> {
			self.0.push(format!("{} {} lineto", pt.0, pt.1));
			Ok(())
		}
		fn curve_to_one(&mut self, _: Option<Point>, a: Point, b: Point, c: Point) -> Result<()> {
			self.0.push(format!(
				"{} {} {} {} {} {} curveto",
				a.0, a.1, b.0, b.1, c.0, c.1
			));
			Ok(())
		}
		fn close_path(&mut self) -> Result<()> {
			self.0.push("closepath".into());
			Ok(())
		}
	}

	// The `_TestPen` demo of fontTools' basePen.
	#[test]
	fn test_pen() -> Result<()> {
		let mut pen = BasePen::with_inner(Log::default(), None);
		pen.move_to((0.0, 0.0))?;
		pen.line_to((0.0, 100.0))?;
		pen.curve_to(&[(50.0, 75.0), (60.0, 50.0), (50.0, 25.0), (0.0, 0.0)])?;
		pen.close_path()?;
		assert_eq!(
			pen.inner.0,
			[
				"0 0 moveto",
				"0 100 lineto",
				"50 75 55 62.5 55 50 curveto",
				"55 37.5 50 25 0 0 curveto",
				"closepath",
			]
		);
		Ok(())
	}

	#[test]
	fn decompose() {
		assert_eq!(
			decompose_quadratic_segment(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]).ok(),
			Some(vec![((0.0, 0.0), (5.0, 0.0)), ((10.0, 0.0), (10.0, 10.0))])
		);
		assert_eq!(
			decompose_super_bezier_segment(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]).ok(),
			Some(vec![((1.0, 1.0), (2.0, 2.0), (3.0, 3.0))])
		);
	}
}
