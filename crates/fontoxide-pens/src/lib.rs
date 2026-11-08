//! Port of `fontTools.pens`: drawing glyph outlines through pens.
//!
//! A glyph draws itself by calling the methods of a [`Pen`]: `moveTo`,
//! `lineTo`, `curveTo`, `qCurveTo`, `closePath`, `endPath` and `addComponent`.
//! Pens that only handle plain segments implement [`SegmentPen`] and are
//! wrapped in a [`BasePen`], which splits the curves into segments and draws
//! components from a [`GlyphSet`].
//!
//! | fontoxide           | fontTools                       |
//! |---------------------|---------------------------------|
//! | [`base_pen`]        | `fontTools.pens.basePen`        |
//! | [`bounds_pen`]      | `fontTools.pens.boundsPen`      |
//! | [`transform_pen`]   | `fontTools.pens.transformPen`   |

#![forbid(unsafe_code)]

pub mod base_pen;
pub mod bounds_pen;
pub mod transform_pen;

pub use base_pen::{BasePen, SegmentPen};
pub use bounds_pen::{BoundsPen, ControlBoundsPen};
pub use fontoxide_misc::array_tools::{Point, Rect};
pub use fontoxide_misc::transform::Transform;
pub use transform_pen::TransformPen;

/// An error while drawing.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// `MissingComponentError`: a component's base glyph is not in the glyph
	/// set, and the pen does not skip missing components.
	#[error("{0}")]
	MissingComponent(String),
	/// `AssertionError`, `TypeError` and the like: the pen was used wrongly.
	#[error("{0}")]
	Usage(String),
	/// An error of the glyph being drawn, such as a malformed outline.
	#[error(transparent)]
	Glyph(Box<dyn std::error::Error + Send + Sync>),
}

/// A `Result` with an [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

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
}

/// Glyphs that can be drawn by name (fontTools' glyph-set mapping, used as
/// `glyphSet[glyphName].draw(pen)`).
pub trait GlyphSet {
	/// Draws a glyph onto `pen`. Returns `Ok(false)`, without drawing anything,
	/// if there is no such glyph (a `KeyError` in fontTools).
	fn draw_glyph(&self, glyph_name: &str, pen: &mut dyn Pen) -> Result<bool>;
}
