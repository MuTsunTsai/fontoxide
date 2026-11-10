//! Pens that record what is drawn (`fontTools.pens.recordingPen`).

use crate::{
	DecomposedTransform, Decomposer, Error, GlyphSet, Location, Pen, Point, PointPen, Result,
	SegmentType, Transform,
};

/// A recorded pen call: an entry of `RecordingPen.value`.
#[derive(Debug, Clone, PartialEq)]
pub enum PenOp {
	/// `("moveTo", (pt,))`.
	MoveTo(Point),
	/// `("lineTo", (pt,))`.
	LineTo(Point),
	/// `("curveTo", points)`.
	CurveTo(Vec<Point>),
	/// `("qCurveTo", points)`: the off-curve points, and the on-curve point or
	/// `None`.
	QCurveTo(Vec<Point>, Option<Point>),
	/// `("closePath", ())`.
	ClosePath,
	/// `("endPath", ())`.
	EndPath,
	/// `("addComponent", (glyphName, transformation))`.
	AddComponent(String, Transform),
	/// `("addVarComponent", (glyphName, transformation, location))`, with
	/// `{"resetUnspecifiedAxes": True}` if the flag is set.
	AddVarComponent(String, DecomposedTransform, Location, bool),
}

impl PenOp {
	/// The name of the pen method.
	pub fn operator(&self) -> &'static str {
		match self {
			PenOp::MoveTo(_) => "moveTo",
			PenOp::LineTo(_) => "lineTo",
			PenOp::CurveTo(_) => "curveTo",
			PenOp::QCurveTo(..) => "qCurveTo",
			PenOp::ClosePath => "closePath",
			PenOp::EndPath => "endPath",
			PenOp::AddComponent(..) => "addComponent",
			PenOp::AddVarComponent(..) => "addVarComponent",
		}
	}

	/// The point operands, with `None` for a missing on-curve point of
	/// `qCurveTo`. Empty for the other calls.
	pub fn points(&self) -> Vec<Option<Point>> {
		match self {
			PenOp::MoveTo(p) | PenOp::LineTo(p) => vec![Some(*p)],
			PenOp::CurveTo(pts) => pts.iter().copied().map(Some).collect(),
			PenOp::QCurveTo(off, on) => off.iter().copied().map(Some).chain([*on]).collect(),
			_ => Vec::new(),
		}
	}

	/// Calls the pen method (`replayRecording` for one entry).
	pub fn replay(&self, pen: &mut dyn Pen) -> Result<()> {
		match self {
			PenOp::MoveTo(p) => pen.move_to(*p),
			PenOp::LineTo(p) => pen.line_to(*p),
			PenOp::CurveTo(pts) => pen.curve_to(pts),
			PenOp::QCurveTo(off, on) => pen.q_curve_to(off, *on),
			PenOp::ClosePath => pen.close_path(),
			PenOp::EndPath => pen.end_path(),
			PenOp::AddComponent(name, t) => pen.add_component(name, *t),
			PenOp::AddVarComponent(name, t, loc, reset) => {
				pen.add_var_component(name, *t, loc, *reset)
			}
		}
	}
}

/// `replayRecording`: draws the recorded calls onto `pen`.
pub fn replay_recording(recording: &[PenOp], pen: &mut dyn Pen) -> Result<()> {
	recording.iter().try_for_each(|op| op.replay(pen))
}

/// Records the calls it receives (`RecordingPen`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordingPen {
	/// `value`.
	pub value: Vec<PenOp>,
}

impl RecordingPen {
	/// An empty recording.
	pub fn new() -> Self {
		Self::default()
	}

	/// `replay(pen)` (also `draw`).
	pub fn replay(&self, pen: &mut dyn Pen) -> Result<()> {
		replay_recording(&self.value, pen)
	}
}

impl Pen for RecordingPen {
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
		Ok(())
	}

	fn end_path(&mut self) -> Result<()> {
		self.value.push(PenOp::EndPath);
		Ok(())
	}

	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> Result<()> {
		self.value
			.push(PenOp::AddComponent(glyph_name.to_owned(), transformation));
		Ok(())
	}

	fn add_var_component(
		&mut self,
		glyph_name: &str,
		transformation: DecomposedTransform,
		location: &Location,
		reset_unspecified_axes: bool,
	) -> Result<()> {
		self.value.push(PenOp::AddVarComponent(
			glyph_name.to_owned(),
			transformation,
			location.clone(),
			reset_unspecified_axes,
		));
		Ok(())
	}
}

/// A [`RecordingPen`] that draws components decomposed
/// (`DecomposingRecordingPen`). Missing components are errors unless
/// `skip_missing_components` is set.
#[derive(Debug)]
pub struct DecomposingRecordingPen<'g> {
	/// The recording.
	pub recording: RecordingPen,
	/// How components are decomposed.
	pub decomposer: Decomposer<'g>,
}

impl<'g> DecomposingRecordingPen<'g> {
	/// `DecomposingRecordingPen(glyphSet)`: missing components are errors.
	pub fn new(glyph_set: &'g dyn GlyphSet) -> Self {
		DecomposingRecordingPen {
			recording: RecordingPen::new(),
			decomposer: Decomposer::new(glyph_set, false),
		}
	}

	/// `value`.
	pub fn value(&self) -> &[PenOp] {
		&self.recording.value
	}
}

impl Pen for DecomposingRecordingPen<'_> {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.recording.move_to(pt)
	}
	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.recording.line_to(pt)
	}
	fn curve_to(&mut self, points: &[Point]) -> Result<()> {
		self.recording.curve_to(points)
	}
	fn q_curve_to(&mut self, off_curves: &[Point], on_curve: Option<Point>) -> Result<()> {
		self.recording.q_curve_to(off_curves, on_curve)
	}
	fn close_path(&mut self) -> Result<()> {
		self.recording.close_path()
	}
	fn end_path(&mut self) -> Result<()> {
		self.recording.end_path()
	}
	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> Result<()> {
		let decomposer = self.decomposer;
		decomposer.draw_component(self, glyph_name, transformation)
	}
}

/// A recorded point pen call: an entry of `RecordingPointPen.value`.
#[derive(Debug, Clone, PartialEq)]
pub enum PointOp {
	/// `("beginPath", (), {"identifier": …})`.
	BeginPath(Option<String>),
	/// `("endPath", (), {})`.
	EndPath,
	/// `("addPoint", (pt, segmentType, smooth, name), {"identifier": …})`.
	AddPoint {
		/// `pt`.
		pt: Point,
		/// `segmentType`.
		segment_type: Option<SegmentType>,
		/// `smooth`.
		smooth: bool,
		/// `name`.
		name: Option<String>,
		/// `identifier`.
		identifier: Option<String>,
	},
	/// `("addComponent", (baseGlyphName, transformation), {"identifier": …})`.
	AddComponent(String, Transform, Option<String>),
	/// `("addVarComponent", (glyphName, transformation, location), {…})`.
	AddVarComponent(String, DecomposedTransform, Location, Option<String>),
}

impl PointOp {
	/// An `addPoint` entry without a name or an identifier.
	pub fn point(pt: Point, segment_type: Option<SegmentType>, smooth: bool) -> Self {
		PointOp::AddPoint {
			pt,
			segment_type,
			smooth,
			name: None,
			identifier: None,
		}
	}

	/// Calls the point pen method.
	pub fn replay(&self, pen: &mut dyn PointPen) -> Result<()> {
		match self {
			PointOp::BeginPath(id) => pen.begin_path(id.as_deref()),
			PointOp::EndPath => pen.end_path(),
			PointOp::AddPoint {
				pt,
				segment_type,
				smooth,
				name,
				identifier,
			} => pen.add_point(
				*pt,
				*segment_type,
				*smooth,
				name.as_deref(),
				identifier.as_deref(),
			),
			PointOp::AddComponent(name, t, id) => pen.add_component(name, *t, id.as_deref()),
			PointOp::AddVarComponent(name, t, loc, id) => {
				pen.add_var_component(name, *t, loc, id.as_deref())
			}
		}
	}
}

/// Records the point pen calls it receives (`RecordingPointPen`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordingPointPen {
	/// `value`.
	pub value: Vec<PointOp>,
}

impl RecordingPointPen {
	/// An empty recording.
	pub fn new() -> Self {
		Self::default()
	}

	/// `replay(pointPen)` (also `drawPoints`).
	pub fn replay(&self, pen: &mut dyn PointPen) -> Result<()> {
		self.value.iter().try_for_each(|op| op.replay(pen))
	}
}

impl PointPen for RecordingPointPen {
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()> {
		self.value
			.push(PointOp::BeginPath(identifier.map(str::to_owned)));
		Ok(())
	}

	fn end_path(&mut self) -> Result<()> {
		self.value.push(PointOp::EndPath);
		Ok(())
	}

	fn add_point(
		&mut self,
		pt: Point,
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		self.value.push(PointOp::AddPoint {
			pt,
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
		self.value.push(PointOp::AddComponent(
			base_glyph_name.to_owned(),
			transformation,
			identifier.map(str::to_owned),
		));
		Ok(())
	}

	fn add_var_component(
		&mut self,
		glyph_name: &str,
		transformation: DecomposedTransform,
		location: &Location,
		identifier: Option<&str>,
	) -> Result<()> {
		self.value.push(PointOp::AddVarComponent(
			glyph_name.to_owned(),
			transformation,
			location.clone(),
			identifier.map(str::to_owned),
		));
		Ok(())
	}
}

/// A [`RecordingPointPen`] that draws components decomposed
/// (`DecomposingRecordingPointPen`).
#[derive(Debug)]
pub struct DecomposingRecordingPointPen<'g> {
	/// The recording.
	pub recording: RecordingPointPen,
	/// How components are decomposed.
	pub decomposer: Decomposer<'g>,
}

impl<'g> DecomposingRecordingPointPen<'g> {
	/// `DecomposingRecordingPointPen(glyphSet)`: missing components are errors.
	pub fn new(glyph_set: &'g dyn GlyphSet) -> Self {
		DecomposingRecordingPointPen {
			recording: RecordingPointPen::new(),
			decomposer: Decomposer::new(glyph_set, false),
		}
	}

	/// `value`.
	pub fn value(&self) -> &[PointOp] {
		&self.recording.value
	}
}

impl PointPen for DecomposingRecordingPointPen<'_> {
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()> {
		self.recording.begin_path(identifier)
	}
	fn end_path(&mut self) -> Result<()> {
		self.recording.end_path()
	}
	fn add_point(
		&mut self,
		pt: Point,
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		self.recording
			.add_point(pt, segment_type, smooth, name, identifier)
	}
	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: Transform,
		_: Option<&str>,
	) -> Result<()> {
		let decomposer = self.decomposer;
		decomposer.draw_component_points(self, base_glyph_name, transformation)
	}
	fn add_var_component(
		&mut self,
		glyph_name: &str,
		transformation: DecomposedTransform,
		location: &Location,
		identifier: Option<&str>,
	) -> Result<()> {
		self.recording
			.add_var_component(glyph_name, transformation, location, identifier)
	}
}

/// `lerpRecordings`: the recordings interpolated, `factor` of the way from
/// the first to the second (0.5 in fontTools). The recordings must be made of
/// the same calls, without components.
pub fn lerp_recordings(
	recording1: &[PenOp],
	recording2: &[PenOp],
	factor: f64,
) -> Result<Vec<PenOp>> {
	if recording1.len() != recording2.len() {
		return Err(Error::Usage(format!(
			"ValueError: Mismatched lengths: {} and {}",
			recording1.len(),
			recording2.len()
		)));
	}
	let lerp =
		|(x1, y1): Point, (x2, y2): Point| (x1 + (x2 - x1) * factor, y1 + (y2 - y1) * factor);
	let lerp_all = |a: &[Point], b: &[Point]| -> Vec<Point> {
		a.iter().zip(b).map(|(&p, &q)| lerp(p, q)).collect()
	};
	recording1
		.iter()
		.zip(recording2)
		.map(|(op1, op2)| {
			if op1.operator() != op2.operator() {
				return Err(Error::Usage(format!(
					"ValueError: Mismatched operations: {}, {}",
					op1.operator(),
					op2.operator()
				)));
			}
			Ok(match (op1, op2) {
				(PenOp::AddComponent(..), _) | (PenOp::AddVarComponent(..), _) => {
					return Err(Error::Usage(
						"ValueError: Cannot interpolate components".into(),
					));
				}
				(PenOp::MoveTo(a), PenOp::MoveTo(b)) => PenOp::MoveTo(lerp(*a, *b)),
				(PenOp::LineTo(a), PenOp::LineTo(b)) => PenOp::LineTo(lerp(*a, *b)),
				(PenOp::CurveTo(a), PenOp::CurveTo(b)) => PenOp::CurveTo(lerp_all(a, b)),
				(PenOp::QCurveTo(a, ao), PenOp::QCurveTo(b, bo)) => {
					// `zip` stops at the shorter tuple; a `None` point cannot be
					// unpacked.
					let on = match (ao, bo) {
						(Some(p), Some(q)) => Some(lerp(*p, *q)),
						(None, None) if a.len() == b.len() => {
							return Err(Error::Usage(
								"TypeError: cannot unpack non-iterable NoneType object".into(),
							));
						}
						_ => None,
					};
					PenOp::QCurveTo(lerp_all(a, b), on)
				}
				(op, _) => op.clone(),
			})
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ReverseFlipped;

	/// The glyph set of the `DecomposingRecordingPen` doctest.
	struct Glyphs;

	impl GlyphSet for Glyphs {
		fn draw_glyph(&self, name: &str, pen: &mut dyn Pen) -> Result<bool> {
			match name {
				"a" => {
					pen.move_to((0.0, 0.0))?;
					pen.curve_to(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)])?;
					pen.close_path()?;
				}
				"b" => pen.add_component("a", Transform::new(1.0, 0.0, 0.0, 1.0, -1.0, 1.0))?,
				"c" => pen.add_component("foobar", Transform::default())?,
				"d" => pen.add_component("a", Transform::new(-1.0, 0.0, 0.0, 1.0, 0.0, 0.0))?,
				_ => return Ok(false),
			}
			Ok(true)
		}

		fn draw_glyph_points(&self, name: &str, pen: &mut dyn PointPen) -> Result<bool> {
			match name {
				"a" => {
					pen.begin_path(None)?;
					pen.add_point((0.0, 0.0), Some(SegmentType::Line), false, None, None)?;
					pen.add_point((1.0, 1.0), None, false, None, None)?;
					pen.add_point((2.0, 2.0), None, false, None, None)?;
					pen.add_point((3.0, 3.0), Some(SegmentType::Curve), false, None, None)?;
					pen.end_path()?;
				}
				"b" => {
					pen.add_component("a", Transform::new(1.0, 0.0, 0.0, 1.0, -1.0, 1.0), None)?
				}
				"c" => pen.add_component("foobar", Transform::default(), None)?,
				"d" => {
					pen.add_component("a", Transform::new(-1.0, 0.0, 0.0, 1.0, 0.0, 0.0), None)?
				}
				_ => return Ok(false),
			}
			Ok(true)
		}
	}

	fn curve(pts: [Point; 3]) -> PenOp {
		PenOp::CurveTo(pts.to_vec())
	}

	#[test]
	fn decomposing_recording_pen() -> Result<()> {
		let mut out = Vec::new();
		for name in ["a", "b", "c", "d"] {
			let mut pen = DecomposingRecordingPen::new(&Glyphs);
			match Glyphs.draw_glyph(name, &mut pen) {
				Ok(_) | Err(Error::MissingComponent(_)) => {}
				Err(e) => panic!("{e}"),
			}
			out.push(pen.recording.value);
		}
		assert_eq!(
			out,
			[
				vec![
					PenOp::MoveTo((0.0, 0.0)),
					curve([(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]),
					PenOp::ClosePath
				],
				vec![
					PenOp::MoveTo((-1.0, 1.0)),
					curve([(0.0, 2.0), (1.0, 3.0), (2.0, 4.0)]),
					PenOp::ClosePath
				],
				vec![],
				vec![
					PenOp::MoveTo((0.0, 0.0)),
					curve([(-1.0, 1.0), (-2.0, 2.0), (-3.0, 3.0)]),
					PenOp::ClosePath
				],
			]
		);

		let mut pen = DecomposingRecordingPen::new(&Glyphs);
		pen.decomposer.skip_missing_components = true;
		pen.decomposer.reverse_flipped = ReverseFlipped::KeepStart;
		Glyphs.draw_glyph("c", &mut pen)?;
		assert!(pen.value().is_empty());
		Glyphs.draw_glyph("d", &mut pen)?;
		assert_eq!(
			pen.recording.value,
			[
				PenOp::MoveTo((0.0, 0.0)),
				PenOp::LineTo((-3.0, 3.0)),
				curve([(-2.0, 2.0), (-1.0, 1.0), (0.0, 0.0)]),
				PenOp::ClosePath
			]
		);
		Ok(())
	}

	#[test]
	fn decomposing_recording_point_pen() -> Result<()> {
		let line = Some(SegmentType::Line);
		let curve = Some(SegmentType::Curve);
		let mut pen = DecomposingRecordingPointPen::new(&Glyphs);
		Glyphs.draw_glyph_points("b", &mut pen)?;
		assert_eq!(
			pen.recording.value,
			[
				PointOp::BeginPath(None),
				PointOp::point((-1.0, 1.0), line, false),
				PointOp::point((0.0, 2.0), None, false),
				PointOp::point((1.0, 3.0), None, false),
				PointOp::point((2.0, 4.0), curve, false),
				PointOp::EndPath,
			]
		);
		let mut pen = DecomposingRecordingPointPen::new(&Glyphs);
		assert!(matches!(
			Glyphs.draw_glyph_points("c", &mut pen),
			Err(Error::MissingComponent(_))
		));
		assert!(pen.value().is_empty());

		let mut pen = DecomposingRecordingPointPen::new(&Glyphs);
		pen.decomposer.skip_missing_components = true;
		pen.decomposer.reverse_flipped = ReverseFlipped::KeepStart;
		Glyphs.draw_glyph_points("d", &mut pen)?;
		assert_eq!(
			pen.recording.value,
			[
				PointOp::BeginPath(None),
				PointOp::point((0.0, 0.0), curve, false),
				PointOp::point((-3.0, 3.0), line, false),
				PointOp::point((-2.0, 2.0), None, false),
				PointOp::point((-1.0, 1.0), None, false),
				PointOp::EndPath,
			]
		);
		Ok(())
	}
}
