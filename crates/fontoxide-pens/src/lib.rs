//! Port of `fontTools.pens`: drawing glyph outlines through pens.
//!
//! A glyph draws itself by calling the methods of a [`Pen`]: `moveTo`,
//! `lineTo`, `curveTo`, `qCurveTo`, `closePath`, `endPath` and `addComponent`.
//! Pens that only handle plain segments implement [`SegmentPen`] and are
//! wrapped in a [`BasePen`], which splits the curves into segments and draws
//! components from a [`GlyphSet`]. A [`PointPen`] receives the points of each
//! contour one by one instead (`drawPoints`).
//!
//! | fontoxide                     | fontTools                              |
//! |-------------------------------|----------------------------------------|
//! | [`area_pen`]                  | `fontTools.pens.areaPen`               |
//! | [`base_pen`]                  | `fontTools.pens.basePen`               |
//! | [`bounds_pen`]                | `fontTools.pens.boundsPen`             |
//! | [`cu2qu_pen`]                 | `fontTools.pens.cu2quPen`              |
//! | [`explicit_closing_line_pen`] | `fontTools.pens.explicitClosingLinePen` |
//! | [`filter_pen`]                | `fontTools.pens.filterPen`             |
//! | [`hash_point_pen`]            | `fontTools.pens.hashPointPen`          |
//! | [`moments_pen`]               | `fontTools.pens.momentsPen`            |
//! | [`perimeter_pen`]             | `fontTools.pens.perimeterPen`          |
//! | [`point_inside_pen`]          | `fontTools.pens.pointInsidePen`        |
//! | [`point_pen`]                 | `fontTools.pens.pointPen`              |
//! | [`qu2cu_pen`]                 | `fontTools.pens.qu2cuPen`              |
//! | [`recording_pen`]             | `fontTools.pens.recordingPen`          |
//! | [`reverse_contour_pen`]       | `fontTools.pens.reverseContourPen`     |
//! | [`rounding_pen`]              | `fontTools.pens.roundingPen`           |
//! | [`statistics_pen`]            | `fontTools.pens.statisticsPen`         |
//! | [`svg_path_pen`]              | `fontTools.pens.svgPathPen`            |
//! | [`tee_pen`]                   | `fontTools.pens.teePen`                |
//! | [`transform_pen`]             | `fontTools.pens.transformPen`          |
//!
//! `ttGlyphPen` and `t2CharStringPen` are in `fontoxide_ttlib::pens`, as they
//! build `glyf` glyphs and CFF charstrings.
//!
//! Python's pens accept arbitrary keyword arguments in the point pen methods
//! and pass them on; here only `identifier` is carried.

#![forbid(unsafe_code)]

pub mod area_pen;
pub mod base_pen;
pub mod bounds_pen;
pub mod cu2qu_pen;
pub mod explicit_closing_line_pen;
pub mod filter_pen;
pub mod hash_point_pen;
pub mod moments_pen;
pub mod perimeter_pen;
pub mod point_inside_pen;
pub mod point_pen;
pub mod qu2cu_pen;
pub mod recording_pen;
pub mod reverse_contour_pen;
pub mod rounding_pen;
pub mod statistics_pen;
pub mod svg_path_pen;
pub mod tee_pen;
pub mod transform_pen;

pub use base_pen::{BasePen, SegmentPen};
pub use bounds_pen::{BoundsPen, ControlBoundsPen};
pub use fontoxide_misc::array_tools::{Point, Rect};
pub use fontoxide_misc::transform::{DecomposedTransform, Transform};
pub use point_pen::{PointToSegmentPen, SegmentToPointPen};
pub use recording_pen::{RecordingPen, RecordingPointPen};
pub use transform_pen::{TransformPen, TransformPointPen};

/// An error while drawing.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// `MissingComponentError`: a component's base glyph is not in the glyph
	/// set, and the pen does not skip missing components.
	#[error("{0}")]
	MissingComponent(String),
	/// `PenError`: the pen methods were called in a wrong order, or with
	/// wrong points.
	#[error("{0}")]
	Pen(String),
	/// `AssertionError`, `TypeError` and the like: the pen was used wrongly.
	#[error("{0}")]
	Usage(String),
	/// An error of the glyph being drawn, such as a malformed outline.
	#[error(transparent)]
	Glyph(Box<dyn std::error::Error + Send + Sync>),
}

/// A `Result` with an [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// The location of a variable component: axis tags and their values, in the
/// order they were given (a `dict`).
pub type Location = Vec<(String, f64)>;

/// The pen protocol (`AbstractPen`).
pub trait Pen {
	/// Starts a contour at `pt` (`moveTo`).
	fn move_to(&mut self, pt: Point) -> Result<()>;

	/// Draws a straight line to `pt` (`lineTo`).
	fn line_to(&mut self, pt: Point) -> Result<()>;

	/// Draws a cubic curve (`curveTo(*points)`): the off-curve points, then the
	/// on-curve end point. More than two off-curve points form a "super
	/// Bézier", split as [`base_pen::decompose_super_bezier_segment`] does.
	fn curve_to(&mut self, points: &[Point]) -> Result<()>;

	/// Draws quadratic curves (`qCurveTo(*points)`): the off-curve points, with
	/// implied on-curve points between them, then the on-curve end point. With
	/// no end point (`None` in fontTools), the contour is made of off-curve
	/// points only and starts and ends halfway between the last and the first.
	fn q_curve_to(&mut self, off_curves: &[Point], on_curve: Option<Point>) -> Result<()>;

	/// Closes the current contour (`closePath`).
	fn close_path(&mut self) -> Result<()>;

	/// Ends the current contour without closing it (`endPath`).
	fn end_path(&mut self) -> Result<()>;

	/// Adds a component: another glyph, transformed (`addComponent`).
	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> Result<()>;

	/// Adds a variable component (`addVarComponent`): another glyph at a
	/// location, transformed. Most pens do not take them: glyph sets
	/// decompose them (`AttributeError` in fontTools).
	fn add_var_component(
		&mut self,
		glyph_name: &str,
		transformation: DecomposedTransform,
		location: &Location,
		reset_unspecified_axes: bool,
	) -> Result<()> {
		let _ = (glyph_name, transformation, location, reset_unspecified_axes);
		Err(Error::Usage("AttributeError: addVarComponent".into()))
	}
}

/// The type of the segment that an on-curve point ends (`segmentType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SegmentType {
	/// `"move"`: the first point of an open contour.
	Move,
	/// `"line"`.
	Line,
	/// `"curve"`: a cubic curve.
	Curve,
	/// `"qcurve"`: quadratic curves.
	QCurve,
}

impl SegmentType {
	/// The name fontTools uses: `"move"`, `"line"`, `"curve"` or `"qcurve"`.
	pub fn as_str(self) -> &'static str {
		match self {
			SegmentType::Move => "move",
			SegmentType::Line => "line",
			SegmentType::Curve => "curve",
			SegmentType::QCurve => "qcurve",
		}
	}

	/// The segment type of a name, if it is one.
	pub fn parse(s: &str) -> Option<Self> {
		Some(match s {
			"move" => SegmentType::Move,
			"line" => SegmentType::Line,
			"curve" => SegmentType::Curve,
			"qcurve" => SegmentType::QCurve,
			_ => return None,
		})
	}
}

/// The point pen protocol (`AbstractPointPen`).
pub trait PointPen {
	/// Starts a contour (`beginPath`).
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()>;

	/// Ends the current contour (`endPath`).
	fn end_path(&mut self) -> Result<()>;

	/// Adds a point to the current contour (`addPoint`): an off-curve point if
	/// `segment_type` is `None`, otherwise the on-curve point ending a segment
	/// of that type.
	fn add_point(
		&mut self,
		pt: Point,
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()>;

	/// Adds a component (`addComponent`).
	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: Transform,
		identifier: Option<&str>,
	) -> Result<()>;

	/// Adds a variable component (`addVarComponent`); most pens do not take
	/// them (`AttributeError` in fontTools).
	fn add_var_component(
		&mut self,
		glyph_name: &str,
		transformation: DecomposedTransform,
		location: &Location,
		identifier: Option<&str>,
	) -> Result<()> {
		let _ = (glyph_name, transformation, location, identifier);
		Err(Error::Usage("AttributeError: addVarComponent".into()))
	}
}

/// Glyphs that can be drawn by name (fontTools' glyph-set mapping, used as
/// `glyphSet[glyphName].draw(pen)`).
pub trait GlyphSet {
	/// Draws a glyph onto `pen`. Returns `Ok(false)`, without drawing anything,
	/// if there is no such glyph (a `KeyError` in fontTools).
	fn draw_glyph(&self, glyph_name: &str, pen: &mut dyn Pen) -> Result<bool>;

	/// Whether there is such a glyph (`glyphName in glyphSet`). By default the
	/// glyph is drawn onto a [`NullPen`] to find out.
	fn contains(&self, glyph_name: &str) -> bool {
		self.draw_glyph(glyph_name, &mut NullPen).unwrap_or(true)
	}

	/// Draws a glyph onto a point pen (`glyph.drawPoints(pen)`). By default the
	/// glyph is drawn through a [`SegmentToPointPen`], as fontTools' `_TTGlyph`
	/// does.
	fn draw_glyph_points(&self, glyph_name: &str, pen: &mut dyn PointPen) -> Result<bool> {
		let mut seg = SegmentToPointPen::new(pen, true);
		self.draw_glyph(glyph_name, &mut seg)
	}
}

/// A pen that does nothing (`NullPen`).
#[derive(Debug, Default, Clone, Copy)]
pub struct NullPen;

impl Pen for NullPen {
	fn move_to(&mut self, _: Point) -> Result<()> {
		Ok(())
	}
	fn line_to(&mut self, _: Point) -> Result<()> {
		Ok(())
	}
	fn curve_to(&mut self, _: &[Point]) -> Result<()> {
		Ok(())
	}
	fn q_curve_to(&mut self, _: &[Point], _: Option<Point>) -> Result<()> {
		Ok(())
	}
	fn close_path(&mut self) -> Result<()> {
		Ok(())
	}
	fn end_path(&mut self) -> Result<()> {
		Ok(())
	}
	fn add_component(&mut self, _: &str, _: Transform) -> Result<()> {
		Ok(())
	}
	fn add_var_component(
		&mut self,
		_: &str,
		_: DecomposedTransform,
		_: &Location,
		_: bool,
	) -> Result<()> {
		Ok(())
	}
}

/// How a decomposing pen draws components whose transformation flips them
/// (`ReverseFlipped`; `reverseFlipped=True` is [`ReverseFlipped::KeepStart`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReverseFlipped {
	/// `NO`: as they are.
	#[default]
	No,
	/// `KEEP_START`: with their contours reversed, keeping the starting point.
	KeepStart,
	/// `ON_CURVE_FIRST`: with their contours reversed, starting at an on-curve
	/// point. Only point pens tell it apart from `KeepStart`.
	OnCurveFirst,
}

/// Whether a transformation flips the outline (a negative determinant).
pub(crate) fn is_flipped(t: &Transform) -> bool {
	t.xx * t.yy - t.xy * t.yx < 0.0
}

/// How a pen decomposes components (`DecomposingPen`'s options).
#[derive(Clone, Copy)]
pub struct Decomposer<'g> {
	/// Where components are looked up (`glyphSet`).
	pub glyph_set: &'g dyn GlyphSet,
	/// Whether a missing component is skipped with a warning rather than an
	/// error (`skipMissingComponents`).
	pub skip_missing_components: bool,
	/// `reverseFlipped`.
	pub reverse_flipped: ReverseFlipped,
}

impl std::fmt::Debug for Decomposer<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Decomposer")
			.field("skip_missing_components", &self.skip_missing_components)
			.field("reverse_flipped", &self.reverse_flipped)
			.finish_non_exhaustive()
	}
}

impl<'g> Decomposer<'g> {
	/// Decomposes with the given glyph set, skipping missing components or
	/// not.
	pub fn new(glyph_set: &'g dyn GlyphSet, skip_missing_components: bool) -> Self {
		Decomposer {
			glyph_set,
			skip_missing_components,
			reverse_flipped: ReverseFlipped::No,
		}
	}

	/// `DecomposingPen.addComponent`: draws the base glyph onto `pen`,
	/// transformed.
	pub fn draw_component(
		&self,
		pen: &mut dyn Pen,
		glyph_name: &str,
		transformation: Transform,
	) -> Result<()> {
		let reverse = self.reverse_flipped != ReverseFlipped::No && is_flipped(&transformation);
		let found = if transformation.is_identity() {
			if reverse {
				let mut rev = reverse_contour_pen::ReverseContourPen::new(pen, false);
				self.glyph_set.draw_glyph(glyph_name, &mut rev)?
			} else {
				self.glyph_set.draw_glyph(glyph_name, pen)?
			}
		} else {
			let mut tp = TransformPen::new(pen, transformation);
			if reverse {
				let mut rev = reverse_contour_pen::ReverseContourPen::new(&mut tp, false);
				self.glyph_set.draw_glyph(glyph_name, &mut rev)?
			} else {
				self.glyph_set.draw_glyph(glyph_name, &mut tp)?
			}
		};
		self.missing(glyph_name, found)
	}

	/// `DecomposingPointPen.addComponent`: draws the base glyph onto `pen`,
	/// transformed.
	pub fn draw_component_points(
		&self,
		pen: &mut dyn PointPen,
		glyph_name: &str,
		transformation: Transform,
	) -> Result<()> {
		let reverse = self.reverse_flipped != ReverseFlipped::No && is_flipped(&transformation);
		let on_curve_first = self.reverse_flipped == ReverseFlipped::OnCurveFirst;
		let gs = self.glyph_set;
		let draw = |pen: &mut dyn PointPen| -> Result<bool> {
			if !reverse {
				return gs.draw_glyph_points(glyph_name, pen);
			}
			let mut rev = point_pen::ReverseContourPointPen::new(pen);
			if on_curve_first {
				let mut first = filter_pen::OnCurveFirstPointPen::on_curve_first(&mut rev);
				gs.draw_glyph_points(glyph_name, &mut first)
			} else {
				gs.draw_glyph_points(glyph_name, &mut rev)
			}
		};
		let found = if transformation.is_identity() {
			draw(pen)?
		} else {
			draw(&mut TransformPointPen::new(pen, transformation))?
		};
		self.missing(glyph_name, found)
	}

	fn missing(&self, glyph_name: &str, found: bool) -> Result<()> {
		if !found {
			if !self.skip_missing_components {
				return Err(Error::MissingComponent(glyph_name.to_owned()));
			}
			log::warn!("glyph '{glyph_name}' is missing from glyphSet; skipped");
		}
		Ok(())
	}
}
