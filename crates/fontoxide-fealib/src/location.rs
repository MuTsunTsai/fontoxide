//! Locations in feature files (`fontTools.feaLib.location`).

use std::fmt;
use std::sync::Arc;

/// A location in a feature file (`FeatureLibLocation`).
///
/// The file name is shared between all locations of one file, so cloning a
/// location is cheap.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FeatureLibLocation {
	/// The file name, or `<features>` for text without a file name.
	pub file: Arc<str>,
	/// The line, starting at 1.
	pub line: usize,
	/// The column in characters, starting at 1.
	pub column: usize,
}

impl FeatureLibLocation {
	/// `FeatureLibLocation(file, line, column)`.
	pub fn new(file: impl Into<Arc<str>>, line: usize, column: usize) -> Self {
		FeatureLibLocation {
			file: file.into(),
			line,
			column,
		}
	}
}

impl fmt::Display for FeatureLibLocation {
	/// `FeatureLibLocation.__str__`: `file:line:column`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}:{}:{}", self.file, self.line, self.column)
	}
}

impl From<(&str, usize, usize)> for FeatureLibLocation {
	/// `Element.__init__` accepts a plain `(file, line, column)` tuple.
	fn from((file, line, column): (&str, usize, usize)) -> Self {
		FeatureLibLocation::new(file, line, column)
	}
}

/// The optional location carried by every AST node (`Element.location`).
pub type Location = Option<FeatureLibLocation>;

/// `str(location)` for an optional location: `None` when there is none.
pub(crate) fn location_str(location: &Location) -> String {
	match location {
		Some(l) => l.to_string(),
		None => "None".to_owned(),
	}
}
