//! Pens that pass what they receive on to another pen, possibly changed
//! (`fontTools.pens.filterPen`).

use std::collections::HashSet;

use crate::point_pen::PenPoint;
use crate::recording_pen::{PenOp, replay_recording};
use crate::{
	DecomposedTransform, Decomposer, Error, Location, Pen, Point, PointPen, Result, SegmentType,
	Transform,
};

/// Passes everything on unchanged, keeping track of the current point
/// (`FilterPen`).
pub struct FilterPen<'p> {
	out: &'p mut dyn Pen,
	/// `current_pt`.
	pub current_pt: Option<Point>,
}

impl std::fmt::Debug for FilterPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("FilterPen")
			.field("current_pt", &self.current_pt)
			.finish_non_exhaustive()
	}
}

impl<'p> FilterPen<'p> {
	/// `FilterPen(outPen)`.
	pub fn new(out: &'p mut dyn Pen) -> Self {
		FilterPen {
			out,
			current_pt: None,
		}
	}
}

impl Pen for FilterPen<'_> {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.out.move_to(pt)?;
		self.current_pt = Some(pt);
		Ok(())
	}
	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.out.line_to(pt)?;
		self.current_pt = Some(pt);
		Ok(())
	}
	fn curve_to(&mut self, points: &[Point]) -> Result<()> {
		self.out.curve_to(points)?;
		self.current_pt = points.last().copied();
		Ok(())
	}
	fn q_curve_to(&mut self, off_curves: &[Point], on_curve: Option<Point>) -> Result<()> {
		self.out.q_curve_to(off_curves, on_curve)?;
		self.current_pt = on_curve;
		Ok(())
	}
	fn close_path(&mut self) -> Result<()> {
		self.out.close_path()?;
		self.current_pt = None;
		Ok(())
	}
	fn end_path(&mut self) -> Result<()> {
		self.out.end_path()?;
		self.current_pt = None;
		Ok(())
	}
	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> Result<()> {
		self.out.add_component(glyph_name, transformation)
	}
}

/// Buffers each contour, passes it through `filter` when it is closed or
/// ended, and draws the result onto another pen (`ContourFilterPen`).
/// Components are passed on unchanged.
pub struct ContourFilterPen<'p, F> {
	out: &'p mut dyn Pen,
	value: Vec<PenOp>,
	filter: F,
}

impl<F> std::fmt::Debug for ContourFilterPen<'_, F> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ContourFilterPen")
			.field("value", &self.value)
			.finish_non_exhaustive()
	}
}

impl<'p, F: FnMut(Vec<PenOp>) -> Result<Vec<PenOp>>> ContourFilterPen<'p, F> {
	/// A pen filtering each contour with `filter` (`filterContour`), which
	/// receives the recorded calls of the contour.
	pub fn new(out: &'p mut dyn Pen, filter: F) -> Self {
		ContourFilterPen {
			out,
			value: Vec::new(),
			filter,
		}
	}

	fn flush_contour(&mut self) -> Result<()> {
		let contour = std::mem::take(&mut self.value);
		let result = (self.filter)(contour)?;
		replay_recording(&result, self.out)
	}
}

impl<F: FnMut(Vec<PenOp>) -> Result<Vec<PenOp>>> Pen for ContourFilterPen<'_, F> {
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

/// Passes everything on to another point pen unchanged (`FilterPointPen`).
pub struct FilterPointPen<'p> {
	out: &'p mut dyn PointPen,
}

impl std::fmt::Debug for FilterPointPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("FilterPointPen").finish_non_exhaustive()
	}
}

impl<'p> FilterPointPen<'p> {
	/// `FilterPointPen(outPen)`.
	pub fn new(out: &'p mut dyn PointPen) -> Self {
		FilterPointPen { out }
	}
}

impl PointPen for FilterPointPen<'_> {
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()> {
		self.out.begin_path(identifier)
	}
	fn end_path(&mut self) -> Result<()> {
		self.out.end_path()
	}
	fn add_point(
		&mut self,
		pt: Point,
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		self.out
			.add_point(pt, segment_type, smooth, name, identifier)
	}
	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: Transform,
		identifier: Option<&str>,
	) -> Result<()> {
		self.out
			.add_component(base_glyph_name, transformation, identifier)
	}
}

/// Which components a decomposing filter pen decomposes (`include` and
/// `decomposeNested`).
#[derive(Debug, Clone, Default)]
pub struct DecomposeFilter {
	/// The base glyphs whose components are decomposed (`include`); `None`
	/// for all of them.
	pub include: Option<HashSet<String>>,
	/// Whether the components of decomposed components are decomposed too,
	/// even if not included (`decomposeNested`, true by default).
	pub decompose_nested: bool,
}

impl DecomposeFilter {
	/// Decomposes everything.
	pub fn all() -> Self {
		DecomposeFilter {
			include: None,
			decompose_nested: true,
		}
	}

	/// Whether a component of the base glyph is decomposed.
	fn includes(&self, base_glyph_name: &str) -> bool {
		self.include
			.as_ref()
			.is_none_or(|set| set.contains(base_glyph_name))
	}

	/// Before decomposing a component (`_DecomposingFilterMixinBase`): with
	/// `decompose_nested`, the nested components are decomposed regardless of
	/// `include`, which is cleared and returned to be restored afterwards.
	fn enter(&mut self) -> Option<HashSet<String>> {
		if self.decompose_nested && self.include.as_ref().is_some_and(|s| !s.is_empty()) {
			self.include.take()
		} else {
			None
		}
	}

	fn leave(&mut self, backup: Option<HashSet<String>>) {
		if backup.is_some() {
			self.include = backup;
		}
	}
}

/// A [`FilterPen`] that draws components decomposed (`DecomposingFilterPen`).
/// Missing components are errors unless the decomposer skips them.
pub struct DecomposingFilterPen<'p, 'g> {
	filter: FilterPen<'p>,
	/// How components are decomposed.
	pub decomposer: Decomposer<'g>,
	/// Which components are decomposed.
	pub components: DecomposeFilter,
}

impl std::fmt::Debug for DecomposingFilterPen<'_, '_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DecomposingFilterPen")
			.field("decomposer", &self.decomposer)
			.field("components", &self.components)
			.finish_non_exhaustive()
	}
}

impl<'p, 'g> DecomposingFilterPen<'p, 'g> {
	/// `DecomposingFilterPen(outPen, glyphSet)`.
	pub fn new(out: &'p mut dyn Pen, glyph_set: &'g dyn crate::GlyphSet) -> Self {
		DecomposingFilterPen {
			filter: FilterPen::new(out),
			decomposer: Decomposer::new(glyph_set, false),
			components: DecomposeFilter::all(),
		}
	}
}

impl Pen for DecomposingFilterPen<'_, '_> {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.filter.move_to(pt)
	}
	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.filter.line_to(pt)
	}
	fn curve_to(&mut self, points: &[Point]) -> Result<()> {
		self.filter.curve_to(points)
	}
	fn q_curve_to(&mut self, off_curves: &[Point], on_curve: Option<Point>) -> Result<()> {
		self.filter.q_curve_to(off_curves, on_curve)
	}
	fn close_path(&mut self) -> Result<()> {
		self.filter.close_path()
	}
	fn end_path(&mut self) -> Result<()> {
		self.filter.end_path()
	}
	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> Result<()> {
		if !self.components.includes(glyph_name) {
			return self.filter.out.add_component(glyph_name, transformation);
		}
		let backup = self.components.enter();
		let decomposer = self.decomposer;
		let result = decomposer.draw_component(self, glyph_name, transformation);
		self.components.leave(backup);
		result
	}
}

/// A [`FilterPointPen`] that draws components decomposed
/// (`DecomposingFilterPointPen`).
pub struct DecomposingFilterPointPen<'p, 'g> {
	out: &'p mut dyn PointPen,
	/// How components are decomposed.
	pub decomposer: Decomposer<'g>,
	/// Which components are decomposed.
	pub components: DecomposeFilter,
}

impl std::fmt::Debug for DecomposingFilterPointPen<'_, '_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DecomposingFilterPointPen")
			.field("decomposer", &self.decomposer)
			.field("components", &self.components)
			.finish_non_exhaustive()
	}
}

impl<'p, 'g> DecomposingFilterPointPen<'p, 'g> {
	/// `DecomposingFilterPointPen(outPen, glyphSet)`.
	pub fn new(out: &'p mut dyn PointPen, glyph_set: &'g dyn crate::GlyphSet) -> Self {
		DecomposingFilterPointPen {
			out,
			decomposer: Decomposer::new(glyph_set, false),
			components: DecomposeFilter::all(),
		}
	}
}

impl PointPen for DecomposingFilterPointPen<'_, '_> {
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()> {
		self.out.begin_path(identifier)
	}
	fn end_path(&mut self) -> Result<()> {
		self.out.end_path()
	}
	fn add_point(
		&mut self,
		pt: Point,
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		self.out
			.add_point(pt, segment_type, smooth, name, identifier)
	}
	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: Transform,
		identifier: Option<&str>,
	) -> Result<()> {
		if !self.components.includes(base_glyph_name) {
			return self
				.out
				.add_component(base_glyph_name, transformation, identifier);
		}
		let backup = self.components.enter();
		let decomposer = self.decomposer;
		let result = decomposer.draw_component_points(self, base_glyph_name, transformation);
		self.components.leave(backup);
		result
	}
	fn add_var_component(
		&mut self,
		glyph_name: &str,
		transformation: DecomposedTransform,
		location: &Location,
		identifier: Option<&str>,
	) -> Result<()> {
		self.out
			.add_var_component(glyph_name, transformation, location, identifier)
	}
}

/// Buffers each contour of a point pen, passes it through `filter` when it
/// ends, and draws the result onto another point pen
/// (`ContourFilterPointPen`). Components are passed on unchanged.
pub struct ContourFilterPointPen<'p, F> {
	out: &'p mut dyn PointPen,
	current: Option<(Vec<PenPoint>, Option<String>)>,
	filter: F,
}

impl<F> std::fmt::Debug for ContourFilterPointPen<'_, F> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ContourFilterPointPen")
			.field("current", &self.current)
			.finish_non_exhaustive()
	}
}

impl<'p, F: FnMut(Vec<PenPoint>) -> Vec<PenPoint>> ContourFilterPointPen<'p, F> {
	/// A point pen filtering each contour with `filter` (`filterContour`).
	pub fn new(out: &'p mut dyn PointPen, filter: F) -> Self {
		ContourFilterPointPen {
			out,
			current: None,
			filter,
		}
	}
}

impl<F: FnMut(Vec<PenPoint>) -> Vec<PenPoint>> PointPen for ContourFilterPointPen<'_, F> {
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()> {
		if self.current.is_some() {
			return Err(Error::Usage("ValueError: Path already begun".into()));
		}
		self.current = Some((Vec::new(), identifier.map(str::to_owned)));
		Ok(())
	}

	fn end_path(&mut self) -> Result<()> {
		let (contour, identifier) = self
			.current
			.take()
			.ok_or_else(|| Error::Usage("ValueError: Path not begun".into()))?;
		let contour = (self.filter)(contour);
		self.out.begin_path(identifier.as_deref())?;
		for p in contour {
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
		self.out.end_path()
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
			.ok_or_else(|| Error::Usage("ValueError: Path not begun".into()))?
			.0
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
		self.out
			.add_component(base_glyph_name, transformation, identifier)
	}
}

/// `OnCurveFirstPointPen.filterContour`: a closed contour starting with an
/// off-curve point, rotated to start with its first on-curve point.
pub fn on_curve_first(contour: Vec<PenPoint>) -> Vec<PenPoint> {
	let Some(first) = contour.first() else {
		return contour;
	};
	if first.segment_type.is_none()
		&& let Some(i) = contour.iter().position(|p| p.segment_type.is_some())
	{
		let mut contour = contour;
		contour.rotate_left(i);
		return contour;
	}
	contour
}

/// A point pen making closed contours start with an on-curve point
/// (`OnCurveFirstPointPen`).
pub type OnCurveFirstPointPen<'p> = ContourFilterPointPen<'p, fn(Vec<PenPoint>) -> Vec<PenPoint>>;

impl<'p> OnCurveFirstPointPen<'p> {
	/// `OnCurveFirstPointPen(outPen)`.
	pub fn on_curve_first(out: &'p mut dyn PointPen) -> Self {
		ContourFilterPointPen::new(out, on_curve_first)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::recording_pen::{PointOp, RecordingPen, RecordingPointPen};

	// The doctests of `FilterPen`, `FilterPointPen` and `OnCurveFirstPointPen`.
	#[test]
	fn doctests() -> Result<()> {
		let mut rec = RecordingPen::new();
		{
			let mut pen = FilterPen::new(&mut rec);
			pen.move_to((0.0, 0.0))?;
			pen.line_to((1.0, 1.0))?;
			pen.curve_to(&[(2.0, 2.0), (3.0, 3.0), (4.0, 4.0)])?;
			pen.q_curve_to(&[(5.0, 5.0), (6.0, 6.0), (7.0, 7.0)], Some((8.0, 8.0)))?;
			pen.close_path()?;
			pen.move_to((9.0, 9.0))?;
			pen.end_path()?;
			pen.add_component("foo", Transform::default())?;
		}
		assert_eq!(
			rec.value,
			[
				PenOp::MoveTo((0.0, 0.0)),
				PenOp::LineTo((1.0, 1.0)),
				PenOp::CurveTo(vec![(2.0, 2.0), (3.0, 3.0), (4.0, 4.0)]),
				PenOp::QCurveTo(vec![(5.0, 5.0), (6.0, 6.0), (7.0, 7.0)], Some((8.0, 8.0))),
				PenOp::ClosePath,
				PenOp::MoveTo((9.0, 9.0)),
				PenOp::EndPath,
				PenOp::AddComponent("foo".into(), Transform::default()),
			]
		);

		let mut rec = RecordingPointPen::new();
		{
			let mut pen = FilterPointPen::new(&mut rec);
			pen.begin_path(Some("abc"))?;
			pen.add_point((1.0, 2.0), Some(SegmentType::Line), false, None, None)?;
			pen.add_component(
				"a",
				Transform::new(2.0, 0.0, 0.0, 2.0, 10.0, -10.0),
				Some("0001"),
			)?;
			pen.end_path()?;
		}
		assert_eq!(
			rec.value,
			[
				PointOp::BeginPath(Some("abc".into())),
				PointOp::point((1.0, 2.0), Some(SegmentType::Line), false),
				PointOp::AddComponent(
					"a".into(),
					Transform::new(2.0, 0.0, 0.0, 2.0, 10.0, -10.0),
					Some("0001".into())
				),
				PointOp::EndPath,
			]
		);

		let mut rec = RecordingPointPen::new();
		{
			let mut pen = OnCurveFirstPointPen::on_curve_first(&mut rec);
			pen.begin_path(None)?;
			pen.add_point((0.0, 0.0), None, false, None, None)?;
			pen.add_point((100.0, 100.0), Some(SegmentType::Line), false, None, None)?;
			pen.add_point((200.0, 0.0), None, false, None, None)?;
			pen.add_point((300.0, 100.0), Some(SegmentType::Curve), false, None, None)?;
			pen.end_path()?;
		}
		assert_eq!(
			rec.value,
			[
				PointOp::BeginPath(None),
				PointOp::point((100.0, 100.0), Some(SegmentType::Line), false),
				PointOp::point((200.0, 0.0), None, false),
				PointOp::point((300.0, 100.0), Some(SegmentType::Curve), false),
				PointOp::point((0.0, 0.0), None, false),
				PointOp::EndPath,
			]
		);
		Ok(())
	}
}
