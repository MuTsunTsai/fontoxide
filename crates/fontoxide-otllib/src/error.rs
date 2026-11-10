//! Errors (`fontTools.otlLib.error`).

/// Where a rule came from in the source, such as `file:line:column` of a feature
/// file, for error messages. fontTools takes any object and prints it.
pub type Location = Option<String>;

/// An otlLib error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// `OpenTypeLibError`: the rules cannot be built, with where they came from.
	#[error("{}", with_location(.location, .message))]
	OpenTypeLib {
		/// The message.
		message: String,
		/// Where the rule came from.
		location: Location,
	},
	/// `ValueError`, `KeyError` or `TypeError` for invalid arguments.
	#[error("{0}")]
	Value(String),
	/// `AssertionError` for arguments that break an assumption.
	#[error("assertion failed: {0}")]
	Assertion(String),
	/// An error from ttLib, such as an offset overflow while measuring a lookup.
	#[error(transparent)]
	TtLib(#[from] fontoxide_ttlib::Error),
}

impl Error {
	/// `OpenTypeLibError(message, location)`.
	pub fn otl(message: impl Into<String>, location: &Location) -> Self {
		Error::OpenTypeLib {
			message: message.into(),
			location: location.clone(),
		}
	}
}

/// `OpenTypeLibError.__str__`: the location, if it is truthy, before the message.
fn with_location(location: &Location, message: &str) -> String {
	match location {
		Some(l) if !l.is_empty() => format!("{l}: {message}"),
		_ => message.to_owned(),
	}
}

/// The result type of this crate.
pub type Result<T> = std::result::Result<T, Error>;
