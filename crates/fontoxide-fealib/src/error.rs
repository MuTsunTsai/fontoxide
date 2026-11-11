//! Errors (`fontTools.feaLib.error`).

use crate::location::{FeatureLibLocation, Location};

/// A feaLib error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// `FeatureLibError`: the feature file is invalid, with where.
	#[error("{}", with_location(.location, .message))]
	FeatureLib {
		/// The message.
		message: String,
		/// Where the problem is.
		location: Location,
	},
	/// `IncludedFeaNotFound`: a file named by an `include` statement does not
	/// exist.
	#[error(
		"{location}: The following feature file should be included but cannot be found: {filename}"
	)]
	IncludedFeaNotFound {
		/// The file name as written in the `include` statement.
		filename: String,
		/// Where the file name is.
		location: FeatureLibLocation,
		/// The underlying error.
		#[source]
		source: std::io::Error,
	},
	/// A file cannot be read (`OSError`), or is not valid UTF-8
	/// (`UnicodeDecodeError`).
	#[error("{path}: {source}")]
	Io {
		/// The path.
		path: String,
		/// The underlying error.
		#[source]
		source: std::io::Error,
	},
	/// A Python `ValueError`, `TypeError`, `KeyError`, `AttributeError` or
	/// `UnboundLocalError` that fontTools lets escape on unusual input; the
	/// message is Python's.
	#[error("{0}")]
	Value(String),
	/// An `AssertionError` for arguments that break an assumption.
	#[error("assertion failed: {0}")]
	Assertion(String),
	/// `NotImplementedError`: tables that the builder does not support were
	/// requested.
	#[error("{0}")]
	NotImplemented(String),
	/// An error from otlLib that the builder lets escape.
	#[error(transparent)]
	OtlLib(#[from] fontoxide_otllib::Error),
	/// An error from ttLib or varLib (whose errors are ttLib's).
	#[error(transparent)]
	TtLib(#[from] fontoxide_ttlib::Error),
}

impl Error {
	/// `FeatureLibError(message, location)`.
	pub fn feature_lib(message: impl Into<String>, location: Location) -> Self {
		Error::FeatureLib {
			message: message.into(),
			location,
		}
	}

	/// The location of a `FeatureLibError` or `IncludedFeaNotFound`.
	pub fn location(&self) -> Option<&FeatureLibLocation> {
		match self {
			Error::FeatureLib { location, .. } => location.as_ref(),
			Error::IncludedFeaNotFound { location, .. } => Some(location),
			_ => None,
		}
	}
}

/// `FeatureLibError.__str__`: the location, if there is one, before the message.
fn with_location(location: &Location, message: &str) -> String {
	match location {
		Some(l) => format!("{l}: {message}"),
		None => message.to_owned(),
	}
}

/// The result type of this crate.
pub type Result<T> = std::result::Result<T, Error>;
