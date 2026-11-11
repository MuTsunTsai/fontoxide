//! Port of `fontTools.ufoLib.errors`.

use fontoxide_misc::plistlib::PlistError;

/// The errors of ufoLib, and the Python exceptions it lets through.
///
/// fontTools has a small exception hierarchy: `UnsupportedUFOFormat` and
/// `GlifLibError` are `UFOLibError`s, and `UnsupportedGLIFFormat` is a
/// `GlifLibError`. [`UfoLibError::is_ufo_lib_error`] and
/// [`UfoLibError::is_glif_lib_error`] answer `isinstance` questions.
#[derive(Debug, thiserror::Error)]
pub enum UfoLibError {
	/// `UFOLibError` proper.
	#[error("{0}")]
	UfoLib(String),
	/// `UnsupportedUFOFormat`.
	#[error("{0}")]
	UnsupportedUfoFormat(String),
	/// `GlifLibError` proper.
	#[error("{0}")]
	GlifLib(String),
	/// `UnsupportedGLIFFormat`.
	#[error("{0}")]
	UnsupportedGlifFormat(String),
	/// `NameTranslationError` of `fontTools.ufoLib.filenames`.
	#[error("{0}")]
	NameTranslation(String),
	/// A Python `ValueError` or `TypeError`, raised on bad arguments.
	#[error("{0}")]
	Value(String),
	/// A Python `KeyError`: a glyph or layer that is not there.
	#[error("KeyError: {0}")]
	Key(String),
	/// `NotImplementedError`.
	#[error("NotImplementedError: {0}")]
	NotImplemented(String),
	/// An error of the file system layer (`fontTools.misc.filesystem.errors`)
	/// that ufoLib does not convert, such as `IllegalBackReference` for a path
	/// leaving the UFO, or `ResourceReadOnly` when writing into a zipped UFO
	/// opened for reading.
	#[error("{0}")]
	Filesystem(String),
	/// An XML error outside the places where ufoLib wraps it: a syntax error
	/// found by the quick GLIF scans (`getUnicodes` and the like, an
	/// `ExpatError` in fontTools), or a string that cannot be written to XML
	/// (a `ValueError` of lxml).
	#[error("{0}")]
	Xml(String),
	/// An error raised by a pen while drawing a glyph.
	#[error(transparent)]
	Pen(fontoxide_pens::Error),
	/// A plist error outside the places where ufoLib wraps it.
	#[error(transparent)]
	Plist(#[from] PlistError),
	/// An operating system error.
	#[error(transparent)]
	Io(#[from] std::io::Error),
}

impl UfoLibError {
	/// `isinstance(error, UFOLibError)`.
	pub fn is_ufo_lib_error(&self) -> bool {
		matches!(
			self,
			UfoLibError::UfoLib(_)
				| UfoLibError::UnsupportedUfoFormat(_)
				| UfoLibError::GlifLib(_)
				| UfoLibError::UnsupportedGlifFormat(_)
		)
	}

	/// `isinstance(error, GlifLibError)`.
	pub fn is_glif_lib_error(&self) -> bool {
		matches!(
			self,
			UfoLibError::GlifLib(_) | UfoLibError::UnsupportedGlifFormat(_)
		)
	}

	/// `GlifLibError._add_note`: appends a line of context to the message of a
	/// `GlifLibError`; other errors are returned unchanged.
	pub(crate) fn add_note(self, note: &str) -> Self {
		match self {
			UfoLibError::GlifLib(m) => UfoLibError::GlifLib(format!("{m}\n{note}")),
			UfoLibError::UnsupportedGlifFormat(m) => {
				UfoLibError::UnsupportedGlifFormat(format!("{m}\n{note}"))
			}
			other => other,
		}
	}
}

/// Pen errors carrying a ufoLib error (raised by the GLIF point pen while a
/// glyph draws itself into it) are unwrapped.
impl From<fontoxide_pens::Error> for UfoLibError {
	fn from(e: fontoxide_pens::Error) -> Self {
		match e {
			fontoxide_pens::Error::Glyph(inner) => match inner.downcast::<UfoLibError>() {
				Ok(err) => *err,
				Err(other) => UfoLibError::Pen(fontoxide_pens::Error::Glyph(other)),
			},
			other => UfoLibError::Pen(other),
		}
	}
}

impl From<UfoLibError> for fontoxide_pens::Error {
	fn from(e: UfoLibError) -> Self {
		match e {
			UfoLibError::Pen(p) => p,
			other => fontoxide_pens::Error::Glyph(Box::new(other)),
		}
	}
}

/// The result type of this crate.
pub type Result<T> = std::result::Result<T, UfoLibError>;

/// `UFOLibError(message)`.
pub(crate) fn ufo_err<T>(message: impl Into<String>) -> Result<T> {
	Err(UfoLibError::UfoLib(message.into()))
}

/// `GlifLibError(message)`.
pub(crate) fn glif_err<T>(message: impl Into<String>) -> Result<T> {
	Err(UfoLibError::GlifLib(message.into()))
}
