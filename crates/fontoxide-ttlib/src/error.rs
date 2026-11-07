//! Error type.

use fontoxide_misc::Tag;

/// Errors from reading, writing or converting fonts.
///
/// The variants correspond to the exceptions fontTools raises, so that callers can
/// tell them apart the same way.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
	/// `TTLibError`: malformed or unsupported font data, or a misuse of the API.
	#[error("{0}")]
	TtLib(String),
	/// `TTLibFileIsCollectionError`: a collection was opened without a valid font
	/// number.
	#[error("{0}")]
	FileIsCollection(String),
	/// `KeyError` for a missing table.
	#[error("'{0}' table not found")]
	TableNotFound(Tag),
	/// `KeyError` for an unknown glyph name.
	#[error("glyph not found: {0}")]
	GlyphNotFound(String),
	/// `struct.error`: data too short to unpack, or a value that does not fit its
	/// binary field.
	#[error("{0}")]
	Struct(String),
	/// `AssertionError` raised by fontTools on inconsistent data.
	#[error("assertion failed: {0}")]
	Assertion(String),
	/// `ValueError` / `TypeError` for invalid values.
	#[error("{0}")]
	Value(String),
	/// An error while decompiling a table, with the table it occurred in.
	#[error("error decompiling '{tag}' table: {source}")]
	Decompile {
		/// The table being decompiled.
		tag: Tag,
		/// The underlying error.
		#[source]
		source: Box<Error>,
	},
	/// An error while compiling a table, with the table it occurred in.
	#[error("error compiling '{tag}' table: {source}")]
	Compile {
		/// The table being compiled.
		tag: Tag,
		/// The underlying error.
		#[source]
		source: Box<Error>,
	},
	/// `TTXParseError` or an XML syntax error.
	#[error("TTX parse error: {0}")]
	TtxParse(String),
	/// A required table is currently being decompiled, compiled or dumped, so it
	/// cannot be accessed (a circular dependency between tables).
	#[error("'{0}' table is in use (circular table dependency)")]
	TableBusy(Tag),
	/// A table exists but is not of the requested type (for example, it could not
	/// be decompiled and is kept as raw data).
	#[error("'{0}' table has an unexpected type")]
	WrongTableType(Tag),
	/// `OTLOffsetOverflowError`: a 16-bit offset does not fit while compiling an
	/// OpenType Layout table.
	#[error("offset overflow: {0:?}")]
	OffsetOverflow(Box<crate::ot::writer::OverflowErrorRecord>),
	/// An I/O error.
	#[error(transparent)]
	Io(#[from] std::io::Error),
}

/// Shorthand for `Result<T, fontoxide_ttlib::Error>`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
	/// A `TTLibError` with the given message.
	pub fn ttlib(msg: impl Into<String>) -> Self {
		Error::TtLib(msg.into())
	}

	/// A `ValueError` with the given message.
	pub fn value(msg: impl Into<String>) -> Self {
		Error::Value(msg.into())
	}

	/// A TTX parse error with the given message.
	pub fn ttx(msg: impl Into<String>) -> Self {
		Error::TtxParse(msg.into())
	}

	/// The innermost error, skipping the `Decompile`/`Compile` wrappers.
	pub fn root_cause(&self) -> &Error {
		match self {
			Error::Decompile { source, .. } | Error::Compile { source, .. } => source.root_cause(),
			other => other,
		}
	}
}
