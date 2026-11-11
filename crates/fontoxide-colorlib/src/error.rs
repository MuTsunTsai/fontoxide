//! Errors (`fontTools.colorLib.errors`), and the Python exceptions that the
//! builders raise on bad input.

use indexmap::IndexMap;

/// A colorLib error. Each variant stands for the Python exception that
/// fontTools raises; [`Error::py_type_name`] gives its name.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// `ColorLibError`.
	#[error("{0}")]
	ColorLib(String),
	/// The `ColorLibError` of `buildColrV1` collecting the errors of the color
	/// glyphs that failed to build (its `errors` attribute), by base glyph.
	#[error("Failed to build BaseGlyphList:\n{}", format_glyph_errors(.0))]
	BaseGlyphList(IndexMap<String, Error>),
	/// `ValueError`.
	#[error("{0}")]
	Value(String),
	/// `TypeError`.
	#[error("{0}")]
	Type(String),
	/// `OverflowError`.
	#[error("{0}")]
	Overflow(String),
	/// `KeyError`, with the repr of the missing key.
	#[error("{0}")]
	Key(String),
	/// `AttributeError`.
	#[error("{0}")]
	Attribute(String),
	/// `AssertionError`.
	#[error("{0}")]
	Assertion(String),
	/// `NotImplementedError`.
	#[error("{0}")]
	NotImplemented(String),
	/// `RecursionError`: paints nest too deeply (or in a cycle) to unbuild.
	#[error("{0}")]
	Recursion(String),
	/// An error of the font library, such as a value that does not fit its field.
	#[error(transparent)]
	TtLib(#[from] fontoxide_ttlib::Error),
}

impl Error {
	/// The name of the Python exception class (`type(error).__name__`).
	pub fn py_type_name(&self) -> &'static str {
		match self {
			Error::ColorLib(_) | Error::BaseGlyphList(_) => "ColorLibError",
			Error::Value(_) => "ValueError",
			Error::Type(_) => "TypeError",
			Error::Overflow(_) => "OverflowError",
			Error::Key(_) => "KeyError",
			Error::Attribute(_) => "AttributeError",
			Error::Assertion(_) => "AssertionError",
			Error::NotImplemented(_) => "NotImplementedError",
			Error::Recursion(_) => "RecursionError",
			Error::TtLib(_) => "Error",
		}
	}

	/// Whether `buildColrV1` collects the error of a color glyph instead of
	/// raising it: `except (ColorLibError, OverflowError, ValueError, TypeError)`.
	pub(crate) fn is_glyph_error(&self) -> bool {
		matches!(
			self,
			Error::ColorLib(_)
				| Error::BaseGlyphList(_)
				| Error::Overflow(_)
				| Error::Value(_)
				| Error::Type(_)
		)
	}

	/// A `KeyError` for a missing string key.
	pub(crate) fn key(key: &str) -> Self {
		Error::Key(fontoxide_misc::py::str_repr(key))
	}
}

/// `_format_glyph_errors`: one line per glyph, sorted by glyph name.
fn format_glyph_errors(errors: &IndexMap<String, Error>) -> String {
	let mut items: Vec<(&String, &Error)> = errors.iter().collect();
	items.sort_by(|a, b| a.0.cmp(b.0));
	items
		.iter()
		.map(|(glyph, error)| format!("    {glyph} => {}: {error}", error.py_type_name()))
		.collect::<Vec<_>>()
		.join("\n")
}

/// The result type of this crate.
pub type Result<T> = std::result::Result<T, Error>;
