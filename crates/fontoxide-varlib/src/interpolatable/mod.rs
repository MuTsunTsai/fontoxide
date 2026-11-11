//! Port of `fontTools.varLib.interpolatable`: finds wrong contour order
//! between masters, and other interpolatability (or lack thereof) issues.
//!
//! | fontoxide                 | fontTools                                          |
//! |---------------------------|----------------------------------------------------|
//! | this module               | `fontTools.varLib.interpolatable`                  |
//! | [`helpers`]               | `fontTools.varLib.interpolatableHelpers`           |
//! | [`test_contour_order()`]  | `fontTools.varLib.interpolatableTestContourOrder`  |
//! | [`test_starting_point()`] | `fontTools.varLib.interpolatableTestStartingPoint` |
//! | [`scipy`]                 | the SciPy routines fontTools uses                  |
//!
//! fontTools uses SciPy, when installed, for the minimum-cost matching of
//! contours and for ordering the masters; the `interpolatable` extra of
//! fontTools installs it. Like the rest of fontoxide, this module matches
//! fontTools with all its extras: both SciPy routines are ported exactly (see
//! [`scipy`]), and the fallbacks fontTools takes without SciPy are not.
//!
//! The PDF, PostScript and HTML reports (`interpolatablePlot`, which needs
//! cairo) are not ported; the text and JSON reports are.

mod cli;
pub mod helpers;
pub mod scipy;
mod test_contour_order;
mod test_starting_point;

use std::collections::BTreeSet;
use std::fmt::Write as _;

use fontoxide_misc::complex::Complex;
use fontoxide_misc::py;
use fontoxide_pens::point_pen::SegmentToPointPen;
use fontoxide_pens::recording_pen::{PenOp, RecordingPen, lerp_recordings};
use fontoxide_pens::statistics_pen::{Statistics, StatisticsControlPen, StatisticsPen};
use fontoxide_pens::{Pen, Point};
use indexmap::IndexMap;

pub use cli::{main, run};
pub use helpers::{
	ContourVector, Isomorphism, Location, PerContourOrComponentPen, SimpleRecordingPointPen,
	add_isomorphisms, contour_vector_from_stats, find_parents_and_order, matching_for_vectors,
	sort_problems,
};
pub use test_contour_order::test_contour_order;
pub use test_starting_point::test_starting_point;

/// `DEFAULT_TOLERANCE`.
pub const DEFAULT_TOLERANCE: f64 = 0.95;
/// `DEFAULT_KINKINESS`.
pub const DEFAULT_KINKINESS: f64 = 0.5;
/// `DEFAULT_KINKINESS_LENGTH`: a ratio of the UPEM.
pub const DEFAULT_KINKINESS_LENGTH: f64 = 0.002;
/// `DEFAULT_UPEM`.
pub const DEFAULT_UPEM: f64 = 1000.0;

/// An error while checking interpolatability.
#[derive(Debug, thiserror::Error)]
pub enum InterpolatableError {
	/// A font could not be read.
	#[error(transparent)]
	Font(#[from] fontoxide_ttlib::Error),
	/// A UFO could not be read (`UFOLibError`).
	#[error(transparent)]
	Ufo(#[from] fontoxide_ufolib::UfoLibError),
	/// A designspace could not be read.
	#[error(transparent)]
	DesignSpace(#[from] fontoxide_designspacelib::DesignSpaceDocumentError),
	/// A glyph could not be drawn.
	#[error(transparent)]
	Pen(#[from] fontoxide_pens::Error),
	/// A file could not be read or written.
	#[error(transparent)]
	Io(#[from] std::io::Error),
	/// A wrong command line (`argparse` errors).
	#[error("{0}")]
	Usage(String),
	/// `AssertionError`.
	#[error("AssertionError: {0}")]
	Assertion(String),
	/// `ValueError`.
	#[error("{0}")]
	Value(String),
	/// `TypeError`, `KeyError`, `AttributeError`, `ZeroDivisionError` and the
	/// like: the input is not what fontTools expects.
	#[error("{0}")]
	Type(String),
	/// A feature that is not ported: the PDF, PostScript and HTML reports, and
	/// `.glyphs` sources.
	#[error("{0}")]
	Unsupported(String),
}

/// A `Result` with an [`InterpolatableError`].
pub type Result<T> = std::result::Result<T, InterpolatableError>;

/// The kinds of problems (`InterpolatableProblem`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProblemType {
	/// `NOTHING`: shown with `--show-all` only.
	Nothing,
	/// `MISSING`.
	Missing,
	/// `OPEN_PATH`.
	OpenPath,
	/// `PATH_COUNT`.
	PathCount,
	/// `NODE_COUNT`.
	NodeCount,
	/// `NODE_INCOMPATIBILITY`.
	NodeIncompatibility,
	/// `CONTOUR_ORDER`.
	ContourOrder,
	/// `WRONG_START_POINT`.
	WrongStartPoint,
	/// `KINK`.
	Kink,
	/// `UNDERWEIGHT`.
	Underweight,
	/// `OVERWEIGHT`: never reported; the check is disabled in fontTools.
	Overweight,
}

impl ProblemType {
	/// The name fontTools uses, such as `"wrong_start_point"`.
	pub fn as_str(self) -> &'static str {
		match self {
			ProblemType::Nothing => "nothing",
			ProblemType::Missing => "missing",
			ProblemType::OpenPath => "open_path",
			ProblemType::PathCount => "path_count",
			ProblemType::NodeCount => "node_count",
			ProblemType::NodeIncompatibility => "node_incompatibility",
			ProblemType::ContourOrder => "contour_order",
			ProblemType::WrongStartPoint => "wrong_start_point",
			ProblemType::Kink => "kink",
			ProblemType::Underweight => "underweight",
			ProblemType::Overweight => "overweight",
		}
	}
}

/// The two masters a problem is about: `master_1`, `master_2`,
/// `master_1_idx` and `master_2_idx`.
#[derive(Debug, Clone, PartialEq)]
pub struct MasterPair {
	/// `master_1`: the name of the parent master.
	pub master_1: String,
	/// `master_2`: the name of the master compared to it.
	pub master_2: String,
	/// `master_1_idx`.
	pub master_1_idx: usize,
	/// `master_2_idx`.
	pub master_2_idx: usize,
}

/// A problem: one of the dictionaries that fontTools reports, with its keys
/// in the same order.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
	/// `nothing`: a pair of masters, with `--show-all`.
	Nothing {
		/// The masters.
		masters: MasterPair,
	},
	/// `missing`: the glyph is not in a master.
	Missing {
		/// `master`.
		master: String,
		/// `master_idx`.
		master_idx: usize,
	},
	/// `open_path`: a contour of the glyph is open.
	OpenPath {
		/// `master`.
		master: String,
		/// `master_idx`.
		master_idx: usize,
		/// `contour`.
		contour: usize,
	},
	/// `path_count`: the numbers of contours (and components) differ.
	PathCount {
		/// The masters.
		masters: MasterPair,
		/// `value_1`.
		value_1: usize,
		/// `value_2`.
		value_2: usize,
	},
	/// `node_count`: the numbers of segments of a path differ.
	NodeCount {
		/// `path`.
		path: usize,
		/// The masters.
		masters: MasterPair,
		/// `value_1`.
		value_1: usize,
		/// `value_2`.
		value_2: usize,
	},
	/// `node_incompatibility`: the types of a segment differ.
	NodeIncompatibility {
		/// `path`.
		path: usize,
		/// `node`.
		node: usize,
		/// The masters.
		masters: MasterPair,
		/// `value_1`: the pen method, such as `"lineTo"`.
		value_1: String,
		/// `value_2`.
		value_2: String,
	},
	/// `contour_order`: the contours seem to be in a different order.
	ContourOrder {
		/// The masters.
		masters: MasterPair,
		/// `value_1`: `0..n`.
		value_1: Vec<usize>,
		/// `value_2`: the matching contours of the second master.
		value_2: Vec<usize>,
		/// `tolerance`.
		tolerance: f64,
	},
	/// `wrong_start_point`: a contour seems to start at another point.
	WrongStartPoint {
		/// `contour`.
		contour: usize,
		/// The masters.
		masters: MasterPair,
		/// `value_1`: always 0.
		value_1: usize,
		/// `value_2`: the proposed starting point.
		value_2: usize,
		/// `reversed`.
		reversed: bool,
		/// `tolerance`.
		tolerance: f64,
	},
	/// `kink`: a smooth point becomes a corner halfway.
	Kink {
		/// `contour`.
		contour: usize,
		/// The masters.
		masters: MasterPair,
		/// `value`: the point.
		value: usize,
		/// `tolerance`.
		tolerance: f64,
	},
	/// `underweight`: the contour is thinner halfway.
	Underweight {
		/// `contour`.
		contour: usize,
		/// The masters.
		masters: MasterPair,
		/// `tolerance`.
		tolerance: f64,
	},
	/// `overweight`: the contour is thicker halfway (never reported).
	Overweight {
		/// `contour`.
		contour: usize,
		/// The masters.
		masters: MasterPair,
		/// `tolerance`.
		tolerance: f64,
	},
}

/// The problems by glyph name, in the order they were found (or sorted by
/// [`sort_problems`]).
pub type Problems = IndexMap<String, Vec<Problem>>;

impl Problem {
	/// `type`.
	pub fn kind(&self) -> ProblemType {
		match self {
			Problem::Nothing { .. } => ProblemType::Nothing,
			Problem::Missing { .. } => ProblemType::Missing,
			Problem::OpenPath { .. } => ProblemType::OpenPath,
			Problem::PathCount { .. } => ProblemType::PathCount,
			Problem::NodeCount { .. } => ProblemType::NodeCount,
			Problem::NodeIncompatibility { .. } => ProblemType::NodeIncompatibility,
			Problem::ContourOrder { .. } => ProblemType::ContourOrder,
			Problem::WrongStartPoint { .. } => ProblemType::WrongStartPoint,
			Problem::Kink { .. } => ProblemType::Kink,
			Problem::Underweight { .. } => ProblemType::Underweight,
			Problem::Overweight { .. } => ProblemType::Overweight,
		}
	}

	/// `tolerance`, if the problem has one.
	pub fn tolerance(&self) -> Option<f64> {
		match self {
			Problem::ContourOrder { tolerance, .. }
			| Problem::WrongStartPoint { tolerance, .. }
			| Problem::Kink { tolerance, .. }
			| Problem::Underweight { tolerance, .. }
			| Problem::Overweight { tolerance, .. } => Some(*tolerance),
			_ => None,
		}
	}

	/// The masters of a problem about a pair of masters.
	pub fn masters(&self) -> Option<&MasterPair> {
		match self {
			Problem::Missing { .. } | Problem::OpenPath { .. } => None,
			Problem::Nothing { masters }
			| Problem::PathCount { masters, .. }
			| Problem::NodeCount { masters, .. }
			| Problem::NodeIncompatibility { masters, .. }
			| Problem::ContourOrder { masters, .. }
			| Problem::WrongStartPoint { masters, .. }
			| Problem::Kink { masters, .. }
			| Problem::Underweight { masters, .. }
			| Problem::Overweight { masters, .. } => Some(masters),
		}
	}

	/// The indices of the masters (`(master_idx,)` or `(master_1_idx,
	/// master_2_idx)`).
	pub fn master_indices(&self) -> Vec<usize> {
		match self {
			Problem::Missing { master_idx, .. } | Problem::OpenPath { master_idx, .. } => {
				vec![*master_idx]
			}
			_ => self
				.masters()
				.map(|m| vec![m.master_1_idx, m.master_2_idx])
				.unwrap_or_default(),
		}
	}

	/// The names of the masters (`(master,)` or `(master_1, master_2)`).
	pub fn master_names(&self) -> Vec<&str> {
		match self {
			Problem::Missing { master, .. } | Problem::OpenPath { master, .. } => vec![master],
			_ => self
				.masters()
				.map(|m| vec![m.master_1.as_str(), m.master_2.as_str()])
				.unwrap_or_default(),
		}
	}

	/// The problem as `json.dumps` writes the dictionary.
	pub fn to_json(&self) -> String {
		let mut out = String::from("{");
		let mut first = true;
		let mut key = |out: &mut String, k: &str| {
			if !first {
				out.push_str(", ");
			}
			first = false;
			out.push_str(&json_string(k));
			out.push_str(": ");
		};
		key(&mut out, "type");
		out.push_str(&json_string(self.kind().as_str()));
		let masters = |out: &mut String, key: &mut dyn FnMut(&mut String, &str), m: &MasterPair| {
			key(out, "master_1");
			out.push_str(&json_string(&m.master_1));
			key(out, "master_2");
			out.push_str(&json_string(&m.master_2));
			key(out, "master_1_idx");
			let _ = write!(out, "{}", m.master_1_idx);
			key(out, "master_2_idx");
			let _ = write!(out, "{}", m.master_2_idx);
		};
		let int = |out: &mut String, key: &mut dyn FnMut(&mut String, &str), k: &str, v: usize| {
			key(out, k);
			let _ = write!(out, "{v}");
		};
		let float = |out: &mut String, key: &mut dyn FnMut(&mut String, &str), k: &str, v: f64| {
			key(out, k);
			out.push_str(&json_float(v));
		};
		let list =
			|out: &mut String, key: &mut dyn FnMut(&mut String, &str), k: &str, v: &[usize]| {
				key(out, k);
				out.push('[');
				for (i, x) in v.iter().enumerate() {
					if i > 0 {
						out.push_str(", ");
					}
					let _ = write!(out, "{x}");
				}
				out.push(']');
			};
		match self {
			Problem::Nothing { masters: m } => masters(&mut out, &mut key, m),
			Problem::Missing { master, master_idx } => {
				key(&mut out, "master");
				out.push_str(&json_string(master));
				int(&mut out, &mut key, "master_idx", *master_idx);
			}
			Problem::OpenPath {
				master,
				master_idx,
				contour,
			} => {
				key(&mut out, "master");
				out.push_str(&json_string(master));
				int(&mut out, &mut key, "master_idx", *master_idx);
				int(&mut out, &mut key, "contour", *contour);
			}
			Problem::PathCount {
				masters: m,
				value_1,
				value_2,
			} => {
				masters(&mut out, &mut key, m);
				int(&mut out, &mut key, "value_1", *value_1);
				int(&mut out, &mut key, "value_2", *value_2);
			}
			Problem::NodeCount {
				path,
				masters: m,
				value_1,
				value_2,
			} => {
				int(&mut out, &mut key, "path", *path);
				masters(&mut out, &mut key, m);
				int(&mut out, &mut key, "value_1", *value_1);
				int(&mut out, &mut key, "value_2", *value_2);
			}
			Problem::NodeIncompatibility {
				path,
				node,
				masters: m,
				value_1,
				value_2,
			} => {
				int(&mut out, &mut key, "path", *path);
				int(&mut out, &mut key, "node", *node);
				masters(&mut out, &mut key, m);
				key(&mut out, "value_1");
				out.push_str(&json_string(value_1));
				key(&mut out, "value_2");
				out.push_str(&json_string(value_2));
			}
			Problem::ContourOrder {
				masters: m,
				value_1,
				value_2,
				tolerance,
			} => {
				masters(&mut out, &mut key, m);
				list(&mut out, &mut key, "value_1", value_1);
				list(&mut out, &mut key, "value_2", value_2);
				float(&mut out, &mut key, "tolerance", *tolerance);
			}
			Problem::WrongStartPoint {
				contour,
				masters: m,
				value_1,
				value_2,
				reversed,
				tolerance,
			} => {
				int(&mut out, &mut key, "contour", *contour);
				masters(&mut out, &mut key, m);
				int(&mut out, &mut key, "value_1", *value_1);
				int(&mut out, &mut key, "value_2", *value_2);
				key(&mut out, "reversed");
				out.push_str(if *reversed { "true" } else { "false" });
				float(&mut out, &mut key, "tolerance", *tolerance);
			}
			Problem::Kink {
				contour,
				masters: m,
				value,
				tolerance,
			} => {
				int(&mut out, &mut key, "contour", *contour);
				masters(&mut out, &mut key, m);
				int(&mut out, &mut key, "value", *value);
				float(&mut out, &mut key, "tolerance", *tolerance);
			}
			Problem::Underweight {
				contour,
				masters: m,
				tolerance,
			}
			| Problem::Overweight {
				contour,
				masters: m,
				tolerance,
			} => {
				int(&mut out, &mut key, "contour", *contour);
				masters(&mut out, &mut key, m);
				float(&mut out, &mut key, "tolerance", *tolerance);
			}
		}
		out.push('}');
		out
	}
}

/// `json.dumps(problems)`: the problems by glyph name.
pub fn problems_to_json(problems: &Problems) -> String {
	let mut out = String::from("{");
	for (i, (name, list)) in problems.iter().enumerate() {
		if i > 0 {
			out.push_str(", ");
		}
		out.push_str(&json_string(name));
		out.push_str(": [");
		for (j, p) in list.iter().enumerate() {
			if j > 0 {
				out.push_str(", ");
			}
			out.push_str(&p.to_json());
		}
		out.push(']');
	}
	out.push('}');
	out
}

/// A string as `json.dumps` writes it, with `ensure_ascii`.
fn json_string(s: &str) -> String {
	let mut out = String::with_capacity(s.len() + 2);
	out.push('"');
	for c in s.chars() {
		match c {
			'"' => out.push_str("\\\""),
			'\\' => out.push_str("\\\\"),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\t' => out.push_str("\\t"),
			'\u{8}' => out.push_str("\\b"),
			'\u{c}' => out.push_str("\\f"),
			' '..='~' => out.push(c),
			_ => {
				let mut buf = [0u16; 2];
				for unit in c.encode_utf16(&mut buf) {
					let _ = write!(out, "\\u{unit:04x}");
				}
			}
		}
	}
	out.push('"');
	out
}

/// A float as `json.dumps` writes it.
fn json_float(v: f64) -> String {
	if v.is_nan() {
		"NaN".into()
	} else if v.is_infinite() {
		if v > 0.0 { "Infinity" } else { "-Infinity" }.into()
	} else {
		py::float_repr(v)
	}
}

/// A glyph set of a master, as `interpolatable` uses it: a mapping of glyph
/// names to glyphs, where a glyph may be missing (`None`).
pub trait InterpolatableGlyphSet: fontoxide_pens::GlyphSet {
	/// The glyph names (`keys()`).
	fn glyph_names(&self) -> Vec<String>;

	/// Draws a glyph onto `pen` the way `Glyph` does:
	/// `glyph.draw(pen, outputImpliedClosingLine=True)`, or `glyph.draw(pen)`
	/// for glyphs that do not take the argument. Returns `Ok(false)` if the
	/// glyph is missing.
	fn draw_outline(&self, glyph_name: &str, pen: &mut dyn Pen) -> fontoxide_pens::Result<bool> {
		self.draw_glyph(glyph_name, pen)
	}
}

impl InterpolatableGlyphSet for fontoxide_ttlib::GlyphSet<'_> {
	fn glyph_names(&self) -> Vec<String> {
		self.keys().to_vec()
	}
}

impl InterpolatableGlyphSet for fontoxide_ufolib::GlyphSet {
	fn glyph_names(&self) -> Vec<String> {
		self.keys()
	}

	fn draw_outline(&self, glyph_name: &str, pen: &mut dyn Pen) -> fontoxide_pens::Result<bool> {
		if !self.contains(glyph_name) {
			return Ok(false);
		}
		self.draw(glyph_name, pen, true)?;
		Ok(true)
	}
}

/// Whether an error is fontTools' `OpenContourError`.
fn is_open_contour_error(e: &fontoxide_pens::Error) -> bool {
	e.to_string().ends_with("not defined on open contours.")
}

/// A glyph of a master, analysed (`Glyph`).
///
/// Each contour or component is recorded separately; the lists have an entry
/// per contour or component, `None` where it does not apply (`_fill_in`): the
/// statistics of open contours, and the points of components.
#[derive(Debug, Clone, Default)]
pub struct Glyph {
	/// `name`.
	pub name: String,
	/// `doesnt_exist`: the glyph is missing from the master.
	pub doesnt_exist: bool,
	/// `recordings`: the recording of each contour or component.
	pub recordings: Vec<RecordingPen>,
	/// `greenStats`: the statistics of the outline (`StatisticsPen`).
	pub green_stats: Vec<Option<Statistics>>,
	/// `controlStats`: the statistics of the control polygon
	/// (`StatisticsControlPen`).
	pub control_stats: Vec<Option<Statistics>>,
	/// `greenVectors`.
	pub green_vectors: Vec<Option<ContourVector>>,
	/// `controlVectors`.
	pub control_vectors: Vec<Option<ContourVector>>,
	/// `nodeTypes`: the pen methods of each recording.
	pub node_types: Vec<Vec<&'static str>>,
	/// `isomorphisms`: the possible starting points of each contour.
	pub isomorphisms: Vec<Option<Vec<Isomorphism>>>,
	/// `points`: the points of each contour, and whether they are on the curve.
	pub points: Vec<Option<Vec<(Point, bool)>>>,
	/// `openContours`.
	pub open_contours: Vec<bool>,
}

impl Glyph {
	/// `Glyph(glyphname, glyphset)`.
	pub fn new(glyph_name: &str, glyph_set: &dyn InterpolatableGlyphSet) -> Result<Glyph> {
		let mut glyph = Glyph {
			name: glyph_name.to_owned(),
			..Glyph::default()
		};
		glyph.populate(glyph_set)?;
		Ok(glyph)
	}

	/// `_fill_in(ix)`.
	fn fill_in(&mut self, ix: usize) {
		fn fill<T>(list: &mut Vec<Option<T>>, ix: usize) {
			if list.len() == ix {
				list.push(None);
			}
		}
		fill(&mut self.green_stats, ix);
		fill(&mut self.control_stats, ix);
		fill(&mut self.green_vectors, ix);
		fill(&mut self.control_vectors, ix);
		fill(&mut self.isomorphisms, ix);
		fill(&mut self.points, ix);
	}

	/// `_populate(glyphset)`.
	fn populate(&mut self, glyph_set: &dyn InterpolatableGlyphSet) -> Result<()> {
		let mut pen = PerContourOrComponentPen::default();
		if !glyph_set.draw_outline(&self.name, &mut pen)? {
			self.doesnt_exist = true;
			return Ok(());
		}
		self.recordings = pen.into_value();
		let components: &dyn fontoxide_pens::GlyphSet = glyph_set;

		for ix in 0..self.recordings.len() {
			let contour = &self.recordings[ix];
			let node_types: Vec<&'static str> = contour.value.iter().map(PenOp::operator).collect();
			self.node_types.push(node_types.clone());

			let mut green = StatisticsPen::new_statistics(Some(components));
			let mut control = StatisticsControlPen::new_statistics_control(Some(components));
			match contour
				.replay(&mut green)
				.and_then(|()| contour.replay(&mut control))
			{
				Ok(()) => self.open_contours.push(false),
				Err(e) if is_open_contour_error(&e) => {
					self.open_contours.push(true);
					self.fill_in(ix);
					continue;
				}
				Err(e) => return Err(e.into()),
			}
			let green = green.statistics();
			let control = control.statistics();
			self.green_stats.push(Some(green));
			self.control_stats.push(Some(control));
			self.green_vectors
				.push(Some(contour_vector_from_stats(&green)));
			self.control_vectors
				.push(Some(contour_vector_from_stats(&control)));

			// Check starting point
			if node_types.first() == Some(&"addComponent") {
				self.fill_in(ix);
				continue;
			}

			if node_types.first() != Some(&"moveTo") {
				return Err(InterpolatableError::Assertion(
					"nodeTypes[0] == \"moveTo\"".into(),
				));
			}
			if !matches!(node_types.last(), Some(&"closePath") | Some(&"endPath")) {
				return Err(InterpolatableError::Assertion(
					"nodeTypes[-1] in (\"closePath\", \"endPath\")".into(),
				));
			}
			let mut points = SimpleRecordingPointPen::default();
			{
				let mut converter = SegmentToPointPen::new(&mut points, false);
				self.recordings[ix].replay(&mut converter)?;
			}
			// The points are on-curve or off-curve; now check all rotations and
			// mirror-rotations of the contour and build the list of isomorphic
			// possible starting points.
			let mut isomorphisms = Vec::new();
			// Add rotations
			add_isomorphisms(&points.value, &mut isomorphisms, false);
			// Add mirrored rotations
			add_isomorphisms(&points.value, &mut isomorphisms, true);
			self.points.push(Some(points.value));
			self.isomorphisms.push(Some(isomorphisms));
		}
		Ok(())
	}

	/// `draw(pen, contour_idx=None)`: draws all the recordings, or one.
	pub fn draw(&self, pen: &mut dyn Pen, contour_idx: Option<usize>) -> Result<()> {
		match contour_idx {
			None => {
				for contour in &self.recordings {
					contour.replay(pen)?;
				}
			}
			Some(i) => {
				self.recordings
					.get(i)
					.ok_or_else(|| {
						InterpolatableError::Type("IndexError: list index out of range".into())
					})?
					.replay(pen)?;
			}
		}
		Ok(())
	}
}

/// The options of [`test_gen`].
#[derive(Debug, Clone)]
pub struct TestOptions {
	/// Do not report glyphs missing from masters (`ignore_missing`).
	pub ignore_missing: bool,
	/// The normalized locations of the masters (`locations`), to order them.
	pub locations: Option<Vec<Location>>,
	/// `tolerance`: between 0 and 1, or a percentage from 10 to 100.
	pub tolerance: f64,
	/// `kinkiness`: or a percentage from 10.
	pub kinkiness: f64,
	/// `upem`.
	pub upem: f64,
	/// Report all pairs of masters, even without problems (`show_all`).
	pub show_all: bool,
	/// The names of the discrete axes (`discrete_axes`).
	pub discrete_axes: BTreeSet<String>,
}

impl Default for TestOptions {
	fn default() -> Self {
		TestOptions {
			ignore_missing: false,
			locations: None,
			tolerance: DEFAULT_TOLERANCE,
			kinkiness: DEFAULT_KINKINESS,
			upem: DEFAULT_UPEM,
			show_all: false,
			discrete_axes: BTreeSet::new(),
		}
	}
}

/// `test(glyphsets, glyphs, names, ...)`: the problems by glyph name.
pub fn test(
	glyph_sets: &[&dyn InterpolatableGlyphSet],
	glyphs: Option<&[String]>,
	names: Option<&[String]>,
	options: &TestOptions,
) -> Result<Problems> {
	let mut problems = Problems::new();
	test_gen(glyph_sets, glyphs, names, options, &mut |name, problem| {
		problems.entry(name.to_owned()).or_default().push(problem);
		Ok(())
	})?;
	Ok(problems)
}

/// `test_gen(glyphsets, glyphs, names, ...)`: checks each glyph and passes the
/// problems to `sink`, with the glyph name, as they are found.
///
/// Without `glyphs`, all the glyphs of the masters are checked, sorted.
/// LENIENT(L40): fontTools checks them in the order of a `set`. Without `names`, the
/// masters are named by their index (fontTools uses the `repr` of the glyph
/// sets).
pub fn test_gen(
	glyph_sets: &[&dyn InterpolatableGlyphSet],
	glyphs: Option<&[String]>,
	names: Option<&[String]>,
	options: &TestOptions,
	sink: &mut dyn FnMut(&str, Problem) -> Result<()>,
) -> Result<()> {
	let mut tolerance = options.tolerance;
	if tolerance >= 10.0 {
		tolerance *= 0.01;
	}
	if !(0.0..=1.0).contains(&tolerance) {
		return Err(InterpolatableError::Assertion("0 <= tolerance <= 1".into()));
	}
	let mut kinkiness = options.kinkiness;
	if kinkiness >= 10.0 {
		kinkiness *= 0.01;
	}
	if kinkiness
		.partial_cmp(&0.0)
		.is_none_or(|o| o == std::cmp::Ordering::Less)
	{
		return Err(InterpolatableError::Assertion("0 <= kinkiness".into()));
	}

	let default_names: Vec<String>;
	let names: &[String] = match names {
		Some(names) if !names.is_empty() => names,
		_ => {
			default_names = (0..glyph_sets.len()).map(|i| i.to_string()).collect();
			&default_names
		}
	};
	let name = |i: usize| names.get(i).cloned().unwrap_or_default();

	let all_glyphs: Vec<String>;
	let glyphs: &[String] = match glyphs {
		Some(glyphs) => glyphs,
		None => {
			let set: BTreeSet<String> = glyph_sets.iter().flat_map(|g| g.glyph_names()).collect();
			all_glyphs = set.into_iter().collect();
			&all_glyphs
		}
	};

	let (parents, order) = find_parents_and_order(
		glyph_sets.len(),
		options.locations.as_deref(),
		&options.discrete_axes,
	)?;

	let pair = |m0: usize, m1: usize| MasterPair {
		master_1: name(m0),
		master_2: name(m1),
		master_1_idx: m0,
		master_2_idx: m1,
	};

	for glyph_name in glyphs {
		log::info!("Testing glyph {glyph_name}");
		let all: Vec<Glyph> = glyph_sets
			.iter()
			.map(|gs| Glyph::new(glyph_name, *gs))
			.collect::<Result<_>>()?;
		if all.iter().filter(|g| !g.doesnt_exist).count() <= 1 {
			continue;
		}
		for (master_idx, glyph) in all.iter().enumerate() {
			if glyph.doesnt_exist {
				if !options.ignore_missing {
					sink(
						glyph_name,
						Problem::Missing {
							master: name(master_idx),
							master_idx,
						},
					)?;
				}
				continue;
			}
			for (ix, &open) in glyph.open_contours.iter().enumerate() {
				if open {
					sink(
						glyph_name,
						Problem::OpenPath {
							master: name(master_idx),
							master_idx,
							contour: ix,
						},
					)?;
				}
			}
		}

		// `grand_parent(i, glyphname)`.
		let grand_parent = |i: usize| -> Option<usize> {
			let mut i = parents.get(i).copied().flatten()?;
			while let Some(p) = parents[i]
				&& all[i].doesnt_exist
			{
				i = p;
			}
			Some(i)
		};

		let mut matchings: Vec<Option<Vec<usize>>> = vec![None; glyph_sets.len()];

		for &m1idx in &order {
			let glyph1 = &all[m1idx];
			if glyph1.node_types.is_empty() {
				continue;
			}
			let Some(m0idx) = grand_parent(m1idx) else {
				continue;
			};
			let glyph0 = &all[m0idx];
			if glyph0.node_types.is_empty() {
				continue;
			}

			//
			// Basic compatibility checks
			//

			let m0 = &glyph0.node_types;
			let m1 = &glyph1.node_types;
			if m0.len() != m1.len() {
				sink(
					glyph_name,
					Problem::PathCount {
						masters: pair(m0idx, m1idx),
						value_1: m0.len(),
						value_2: m1.len(),
					},
				)?;
				continue;
			}

			if m0 != m1 {
				for (path_ix, (nodes1, nodes2)) in m0.iter().zip(m1).enumerate() {
					if nodes1 == nodes2 {
						continue;
					}
					if nodes1.len() != nodes2.len() {
						sink(
							glyph_name,
							Problem::NodeCount {
								path: path_ix,
								masters: pair(m0idx, m1idx),
								value_1: nodes1.len(),
								value_2: nodes2.len(),
							},
						)?;
						continue;
					}
					for (node_ix, (n1, n2)) in nodes1.iter().zip(nodes2).enumerate() {
						if n1 != n2 {
							sink(
								glyph_name,
								Problem::NodeIncompatibility {
									path: path_ix,
									node: node_ix,
									masters: pair(m0idx, m1idx),
									value_1: (*n1).to_owned(),
									value_2: (*n2).to_owned(),
								},
							)?;
						}
					}
				}
			}

			//
			// InterpolatableProblem.CONTOUR_ORDER check
			//

			let (this_tolerance, matching) = test_contour_order(glyph0, glyph1)?;
			if this_tolerance < tolerance
				&& let Some(matching) = matching
			{
				sink(
					glyph_name,
					Problem::ContourOrder {
						masters: pair(m0idx, m1idx),
						value_1: (0..matching.len()).collect(),
						value_2: matching.clone(),
						tolerance: this_tolerance,
					},
				)?;
				matchings[m1idx] = Some(matching);
			}

			//
			// wrong-start-point / weight check
			//

			let m0_isomorphisms = &glyph0.isomorphisms;
			let mut m1_isomorphisms: Vec<&Option<Vec<Isomorphism>>> =
				glyph1.isomorphisms.iter().collect();
			let m0_vectors = &glyph0.green_vectors;
			let mut m1_vectors: Vec<&Option<ContourVector>> = glyph1.green_vectors.iter().collect();
			let recording0 = &glyph0.recordings;
			let mut recording1: Vec<&RecordingPen> = glyph1.recordings.iter().collect();

			// If contour-order is wrong, adjust it
			let matching = matchings[m1idx].as_deref();
			if let Some(matching) = matching
				&& !m1_isomorphisms.is_empty()
			{
				// m1 is empty for composite glyphs
				let reorder = |i: usize| {
					InterpolatableError::Type(format!("IndexError: list index out of range: {i}"))
				};
				m1_isomorphisms = matching
					.iter()
					.map(|&i| glyph1.isomorphisms.get(i).ok_or_else(|| reorder(i)))
					.collect::<Result<_>>()?;
				m1_vectors = matching
					.iter()
					.map(|&i| glyph1.green_vectors.get(i).ok_or_else(|| reorder(i)))
					.collect::<Result<_>>()?;
				recording1 = matching
					.iter()
					.map(|&i| glyph1.recordings.get(i).ok_or_else(|| reorder(i)))
					.collect::<Result<_>>()?;
			}

			let mut mid_recording: Vec<Option<RecordingPen>> = Vec::new();
			for (c0, c1) in recording0.iter().zip(&recording1) {
				match lerp_recordings(&c0.value, &c1.value, 0.5) {
					Ok(value) => mid_recording.push(Some(RecordingPen { value })),
					// Mismatch because of the reordering above
					Err(fontoxide_pens::Error::Usage(m)) if m.starts_with("ValueError") => {
						mid_recording.push(None);
					}
					Err(e) => return Err(e.into()),
				}
			}

			for (ix, (contour0, contour1)) in
				m0_isomorphisms.iter().zip(&m1_isomorphisms).enumerate()
			{
				let (Some(contour0), Some(contour1)) = (contour0, contour1) else {
					continue;
				};
				if contour0.is_empty() || contour0.len() != contour1.len() {
					// We already reported this; or nothing to do; or not compatible
					// after reordering above.
					continue;
				}

				let (this_tolerance, proposed_point, reverse) =
					test_starting_point(glyph0, glyph1, ix, tolerance, matching)?;

				if this_tolerance < tolerance {
					sink(
						glyph_name,
						Problem::WrongStartPoint {
							contour: ix,
							masters: pair(m0idx, m1idx),
							value_1: 0,
							value_2: proposed_point,
							reversed: reverse,
							tolerance: this_tolerance,
						},
					)?;
				}

				// Weight check.
				//
				// If contour could be mid-interpolated, and the two contours have
				// the same area sign, proceeed.
				//
				// The sign difference can happen if it's a weirdo self-intersecting
				// contour; ignore it.
				let Some(Some(contour)) = mid_recording.get(ix) else {
					continue;
				};
				let (Some(Some(m0_vec)), Some(Some(m1_vec))) =
					(m0_vectors.get(ix), m1_vectors.get(ix).copied())
				else {
					return Err(InterpolatableError::Type(
						"TypeError: 'NoneType' object is not subscriptable".into(),
					));
				};
				if (m0_vec[0] < 0.0) == (m1_vec[0] < 0.0) {
					let mut mid_stats = StatisticsPen::new_statistics(None);
					contour.replay(&mut mid_stats)?;
					let mid_vector = contour_vector_from_stats(&mid_stats.statistics());

					let size0 = m0_vec[0] * m0_vec[0];
					let size1 = m1_vec[0] * m1_vec[0];
					let mid_size = mid_vector[0] * mid_vector[0];

					// The overweight check is disabled in fontTools.
					let expected_size = (size0 * size1).sqrt();
					log::debug!(
						"{}: actual size {}; threshold size {}, master sizes: {}, {}",
						ProblemType::Underweight.as_str(),
						py::format_g(mid_size, 6),
						py::format_g(expected_size, 6),
						py::format_g(size0, 6),
						py::format_g(size1, 6),
					);
					if expected_size * tolerance > mid_size + 1e-5 {
						// A zero expected size cannot get here.
						let this_tolerance = if expected_size == 0.0 {
							0.0
						} else {
							mid_size / expected_size
						};
						log::debug!("tolerance {}", py::format_g(this_tolerance, 6));
						sink(
							glyph_name,
							Problem::Underweight {
								contour: ix,
								masters: pair(m0idx, m1idx),
								tolerance: this_tolerance,
							},
						)?;
					}
				}
			}

			//
			// "kink" detector
			//
			let m0 = &glyph0.points;
			let mut m1: Vec<&Option<Vec<(Point, bool)>>> = glyph1.points.iter().collect();

			// If contour-order is wrong, adjust it
			if let Some(matching) = &matchings[m1idx]
				&& !m1.is_empty()
			{
				// m1 is empty for composite glyphs
				m1 = matching
					.iter()
					.map(|&i| {
						glyph1.points.get(i).ok_or_else(|| {
							InterpolatableError::Type("IndexError: list index out of range".into())
						})
					})
					.collect::<Result<_>>()?;
			}

			let t = 0.1; // ~sin(radian(6)) for tolerance 0.95
			if kinkiness == 0.0 {
				return Err(InterpolatableError::Type(
					"ZeroDivisionError: float division by zero".into(),
				));
			}
			let deviation_threshold =
				options.upem * DEFAULT_KINKINESS_LENGTH * DEFAULT_KINKINESS / kinkiness;

			for (ix, (contour0, contour1)) in m0.iter().zip(&m1).enumerate() {
				let (Some(contour0), Some(contour1)) = (contour0, contour1) else {
					continue;
				};
				if contour0.is_empty() || contour0.len() != contour1.len() {
					// We already reported this; or nothing to do; or not compatible
					// after reordering above.
					continue;
				}
				for i in kinks(
					contour0,
					contour1,
					tolerance,
					kinkiness,
					deviation_threshold,
					t,
				)? {
					let (i, this_tolerance) = i;
					sink(
						glyph_name,
						Problem::Kink {
							contour: ix,
							masters: pair(m0idx, m1idx),
							value: i,
							tolerance: this_tolerance,
						},
					)?;
				}
			}

			//
			// --show-all
			//

			if options.show_all {
				sink(
					glyph_name,
					Problem::Nothing {
						masters: pair(m0idx, m1idx),
					},
				)?;
			}
		}
	}
	Ok(())
}

/// The kinks between two compatible contours: the points and tolerances.
fn kinks(
	contour0: &[(Point, bool)],
	contour1: &[(Point, bool)],
	tolerance: f64,
	kinkiness: f64,
	deviation_threshold: f64,
	t: f64,
) -> Result<Vec<(usize, f64)>> {
	let mut out = Vec::new();
	let n = contour0.len();
	// Walk the contour, keeping track of three consecutive points, with middle
	// one being an on-curve. If the three are co-linear then check for
	// kinky-ness.
	for i in 0..n {
		let pt0 = contour0[i];
		let pt1 = contour1[i];
		if !pt0.1 || !pt1.1 {
			// Skip off-curves
			continue;
		}
		let prev = (i + n - 1) % n;
		let pt0_prev = contour0[prev];
		let pt1_prev = contour1[prev];
		let pt0_next = contour0[(i + 1) % n];
		let pt1_next = contour1[(i + 1) % n];

		if pt0_prev.1 && pt1_prev.1 {
			// At least one off-curve is required
			continue;
		}
		if pt0_next.1 && pt1_next.1 {
			// At least one off-curve is required
			continue;
		}

		let pt0 = Complex::from_point(pt0.0);
		let pt1 = Complex::from_point(pt1.0);
		let pt0_prev = Complex::from_point(pt0_prev.0);
		let pt1_prev = Complex::from_point(pt1_prev.0);
		let pt0_next = Complex::from_point(pt0_next.0);
		let pt1_next = Complex::from_point(pt1_next.0);

		// We have three consecutive points. Check whether they are colinear.
		let d0_prev = pt0 - pt0_prev;
		let d0_next = pt0_next - pt0;
		let d1_prev = pt1 - pt1_prev;
		let d1_next = pt1_next - pt1;

		let mut sin0 = d0_prev.re * d0_next.im - d0_prev.im * d0_next.re;
		let mut sin1 = d1_prev.re * d1_next.im - d1_prev.im * d1_next.re;
		let div0 = d0_prev.abs() * d0_next.abs();
		if div0 == 0.0 {
			continue;
		}
		sin0 /= div0;
		let div1 = d1_prev.abs() * d1_next.abs();
		if div1 == 0.0 {
			continue;
		}
		sin1 /= div1;

		if sin0.abs() > t || sin1.abs() > t {
			// Not colinear / not smooth.
			continue;
		}

		// Check the mid-point is actually, well, in the middle.
		let dot0 = d0_prev.re * d0_next.re + d0_prev.im * d0_next.im;
		let dot1 = d1_prev.re * d1_next.re + d1_prev.im * d1_next.im;
		if dot0 < 0.0 || dot1 < 0.0 {
			// Sharp corner.
			continue;
		}

		// Fine, if handle ratios are similar...
		let r0 = py_div(d0_prev.abs(), d0_prev.abs() + d0_next.abs())?;
		let r1 = py_div(d1_prev.abs(), d1_prev.abs() + d1_next.abs())?;
		let r_diff = (r0 - r1).abs();
		if r_diff.abs() < t {
			// Smooth enough.
			continue;
		}

		let half = |z: Complex| z.div_real(2.0).unwrap_or(z);
		let mid = half(pt0 + pt1);
		let mid_prev = half(pt0_prev + pt1_prev);
		let mid_next = half(pt0_next + pt1_next);

		let mid_d0 = mid - mid_prev;
		let mid_d1 = mid_next - mid;

		let mut sin_mid = mid_d0.re * mid_d1.im - mid_d0.im * mid_d1.re;
		let div = mid_d0.abs() * mid_d1.abs();
		if div == 0.0 {
			continue;
		}
		sin_mid /= div;

		// ...or if the angles are similar.
		if sin_mid.abs() * (tolerance * kinkiness) <= t {
			// Smooth enough.
			continue;
		}

		// How visible is the kink?

		let cross = sin_mid * mid_d0.abs() * mid_d1.abs();
		let arc_len = (mid_d0 + mid_d1).abs();
		let deviation = py_div(cross, arc_len)?.abs();
		if deviation < deviation_threshold {
			continue;
		}
		let deviation_ratio = deviation / arc_len;
		if deviation_ratio > t {
			continue;
		}

		let this_tolerance = t / (sin_mid.abs() * kinkiness);

		log::debug!(
			"kink: deviation {}; deviation_ratio {}; sin_mid {}; r_diff {}",
			py::format_g(deviation, 6),
			py::format_g(deviation_ratio, 6),
			py::format_g(sin_mid, 6),
			py::format_g(r_diff, 6),
		);
		log::debug!("tolerance {}", py::format_g(this_tolerance, 6));
		out.push((i, this_tolerance));
	}
	Ok(out)
}

/// `a / b`, with Python's `ZeroDivisionError`.
fn py_div(a: f64, b: f64) -> Result<f64> {
	if b == 0.0 {
		return Err(InterpolatableError::Type(
			"ZeroDivisionError: float division by zero".into(),
		));
	}
	Ok(a / b)
}
