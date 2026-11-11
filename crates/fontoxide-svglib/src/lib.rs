//! Port of `fontTools.svgLib`: drawing SVG paths and basic shapes with pens.
//!
//! | fontoxide    | fontTools                           |
//! |--------------|-------------------------------------|
//! | [`parser`]   | `fontTools.svgLib.path.parser`      |
//! | [`arc`]      | `fontTools.svgLib.path.arc`         |
//! | [`shapes`]   | `fontTools.svgLib.path.shapes`      |
//! | [`SvgPath`]  | `fontTools.svgLib.path.SVGPath`     |
//!
//! Pens that take elliptical arcs as they are (pens with an `arcTo` method in
//! fontTools) implement [`ArcPen`] and are drawn with the `*_arcs` functions;
//! other pens get the arcs as cubic Bézier curves.

pub mod arc;
pub mod parser;
pub mod shapes;
mod svg_path;

use fontoxide_pens::{Pen, Point};

pub use parser::{parse_path, parse_path_arcs};
pub use svg_path::{Element, SvgPath};

/// The cause of an invalid elliptical arc command (the `ValueError` chained as
/// `__cause__` of the error).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ArcArgumentError(pub String);

/// An error of the module.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// `ValueError` (and the `IndexError` and `TypeError` of malformed
	/// shapes).
	#[error("{0}")]
	Value(String),
	/// `ValueError("Invalid arc command: ...")`, with its cause.
	#[error("Invalid arc command: '{command}'")]
	InvalidArc {
		/// The arc command and its arguments.
		command: String,
		/// What is wrong with the arguments.
		#[source]
		cause: ArcArgumentError,
	},
	/// `NotImplementedError`: a `transform` attribute other than `matrix(...)`.
	#[error("transform functions other than matrix() are not supported: {0:?}")]
	NotImplemented(String),
	/// `ParseError`: malformed XML.
	#[error("{0}")]
	Xml(String),
	/// An error of the pen.
	#[error(transparent)]
	Pen(#[from] fontoxide_pens::Error),
	/// An error reading a file.
	#[error(transparent)]
	Io(#[from] std::io::Error),
}

/// The result type of the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// A pen that takes elliptical arcs (a pen with an `arcTo` method).
pub trait ArcPen: Pen {
	/// Draws an elliptical arc from the current point to `end` with the values
	/// of the SVG arc command (`arcTo(rx, ry, rotation, arc_large, arc_sweep,
	/// end_point)`).
	fn arc_to(
		&mut self,
		rx: f64,
		ry: f64,
		rotation: f64,
		arc_large: bool,
		arc_sweep: bool,
		end: Point,
	) -> fontoxide_pens::Result<()>;
}

/// The pen being drawn to: arcs are passed on to an [`ArcPen`] and
/// approximated for the others (`hasattr(pen, "arcTo")`).
pub(crate) enum Target<'a> {
	Plain(&'a mut dyn Pen),
	Arcs(&'a mut dyn ArcPen),
}

impl Target<'_> {
	pub(crate) fn pen(&mut self) -> &mut dyn Pen {
		match self {
			Target::Plain(p) => &mut **p,
			Target::Arcs(p) => &mut **p,
		}
	}
}
