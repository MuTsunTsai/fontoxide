//! Errors (`fontTools.voltLib.error`).

use std::fmt;

/// Where something is in a VOLT project: the file name (`<volt>` when there is
/// none), the line and the column, both 1-based (the `location` tuples of
/// voltLib).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoltLibLocation {
	/// The file name, or `<volt>`.
	pub path: String,
	/// The line, from 1.
	pub line: usize,
	/// The column, from 1, in characters.
	pub column: usize,
}

impl fmt::Display for VoltLibLocation {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}:{}:{}", self.path, self.line, self.column)
	}
}

/// An optional location; `None` where Python has `None`.
pub type Location = Option<VoltLibLocation>;

/// A voltLib error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// `VoltLibError`: the project is invalid, with where.
	#[error("{}", with_location(.location, .message))]
	VoltLib {
		/// The message.
		message: String,
		/// Where the problem is.
		location: Location,
	},
	/// `NotImplementedError(element)`: something `VoltToFea` cannot convert;
	/// `text` is `str(element)` (its VOLT source).
	#[error("{text}")]
	NotImplemented {
		/// `str()` of the element.
		text: String,
		/// The location of the element.
		location: Location,
	},
	/// An `AssertionError` of fontTools on input that breaks an assumption.
	#[error("AssertionError: {0}")]
	Assertion(String),
	/// A Python `KeyError`, `IndexError`, `TypeError`, `AttributeError`,
	/// `ValueError` or `UnboundLocalError` that fontTools lets escape on unusual
	/// input; the message names the Python exception.
	#[error("{0}")]
	Python(String),
	/// A file cannot be read or written.
	#[error("{path}: {source}")]
	Io {
		/// The path.
		path: String,
		/// The underlying error.
		#[source]
		source: std::io::Error,
	},
	/// A command line error (`argparse`).
	#[error("{0}")]
	Usage(String),
	/// An error from feaLib.
	#[error(transparent)]
	FeaLib(#[from] fontoxide_fealib::Error),
	/// An error from ttLib.
	#[error(transparent)]
	TtLib(#[from] fontoxide_ttlib::Error),
}

impl Error {
	/// `VoltLibError(message, location)`.
	pub fn volt_lib(message: impl Into<String>, location: Location) -> Self {
		Error::VoltLib {
			message: message.into(),
			location,
		}
	}

	/// The location of a `VoltLibError` or `NotImplementedError`.
	pub fn location(&self) -> Option<&VoltLibLocation> {
		match self {
			Error::VoltLib { location, .. } | Error::NotImplemented { location, .. } => {
				location.as_ref()
			}
			_ => None,
		}
	}

	pub(crate) fn key(key: &str) -> Self {
		Error::Python(format!("KeyError: {}", fontoxide_misc::py::str_repr(key)))
	}

	pub(crate) fn index() -> Self {
		Error::Python("IndexError: list index out of range".to_owned())
	}

	pub(crate) fn io(path: impl Into<String>, source: std::io::Error) -> Self {
		Error::Io {
			path: path.into(),
			source,
		}
	}
}

/// `VoltLibError.__str__`: the location, if there is one, before the message.
fn with_location(location: &Location, message: &str) -> String {
	match location {
		Some(l) => format!("{l}: {message}"),
		None => message.to_owned(),
	}
}

/// The result type of this crate.
pub type Result<T> = std::result::Result<T, Error>;
