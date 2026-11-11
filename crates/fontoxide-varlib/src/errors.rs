//! Port of `fontTools.varLib.errors`.

use fontoxide_designspacelib::DesignSpaceDocumentError;

/// `VarLibError` and its subclasses, plus the errors of the libraries varLib
/// uses.
#[derive(Debug, thiserror::Error)]
pub enum VarLibError {
	/// `VarLibValidationError`: the input data is invalid from varLib's point of
	/// view.
	#[error("{0}")]
	Validation(String),
	/// `VarLibMergeError` and its subclasses: the masters cannot be merged.
	#[error("{0}")]
	Merge(Box<MergeError>),
	/// `VarLibCFFDictMergeError`: a CFF Private dict cannot be merged.
	#[error("{0}")]
	CffDictMerge(String),
	/// `VarLibCFFPointTypeMergeError`: a CFF glyph has different point types.
	#[error("{0}")]
	CffPointTypeMerge(String),
	/// `VarLibCFFHintTypeMergeError`: a CFF glyph has different hint types.
	#[error("{0}")]
	CffHintTypeMerge(String),
	/// `VariationModelError`.
	#[error("{0}")]
	VariationModel(String),
	/// A `DesignSpaceDocumentError` (or a Python error designspaceLib lets through).
	#[error(transparent)]
	DesignSpace(#[from] DesignSpaceDocumentError),
	/// An error of fontoxide-ttlib, or a Python error such as `KeyError` or
	/// `ValueError` that fontTools would raise.
	#[error(transparent)]
	Ttlib(#[from] fontoxide_ttlib::Error),
	/// An error of fontoxide-otllib.
	#[error(transparent)]
	Otllib(#[from] fontoxide_otllib::Error),
	/// A file could not be read or written.
	#[error(transparent)]
	Io(#[from] std::io::Error),
	/// A command line error (`parser.error`, which exits with status 2).
	#[error("{0}")]
	Usage(String),
}

impl From<MergeError> for VarLibError {
	fn from(e: MergeError) -> Self {
		VarLibError::Merge(Box::new(e))
	}
}

impl VarLibError {
	/// Appends to the stack of a merge error (`e.stack.append(...)`); other
	/// errors pass unchanged.
	pub(crate) fn push_stack(mut self, item: impl Into<String>) -> Self {
		if let VarLibError::Merge(e) = &mut self {
			e.stack.push(item.into());
		}
		self
	}

	/// The merge error, if this is one.
	pub fn as_merge(&self) -> Option<&MergeError> {
		match self {
			VarLibError::Merge(e) => Some(e),
			_ => None,
		}
	}
}

/// The result type of this crate's varLib functions.
pub type Result<T, E = VarLibError> = std::result::Result<T, E>;

/// The subclasses of `VarLibMergeError`.
#[derive(Debug, Clone, PartialEq)]
pub enum MergeErrorKind {
	/// `ShouldBeConstant`.
	ShouldBeConstant,
	/// `FoundANone`.
	FoundANone,
	/// `NotANone`.
	NotANone,
	/// `MismatchedTypes`.
	MismatchedTypes,
	/// `LengthsDiffer`.
	LengthsDiffer,
	/// `KeysDiffer`.
	KeysDiffer,
	/// `InconsistentGlyphOrder`.
	InconsistentGlyphOrder,
	/// `InconsistentExtensions`.
	InconsistentExtensions,
	/// `UnsupportedFormat`, for a subtable, with the format if known.
	UnsupportedFormat {
		/// `cause["subtable"]`.
		subtable: String,
		/// `repr(cause["value"])`.
		value: Option<String>,
	},
	/// `InconsistentFormats`.
	InconsistentFormats {
		/// `cause["subtable"]`.
		subtable: String,
		/// `repr(cause["value"])`.
		value: Option<String>,
	},
}

impl MergeErrorKind {
	/// The Python class name.
	pub fn class_name(&self) -> &'static str {
		match self {
			MergeErrorKind::ShouldBeConstant => "ShouldBeConstant",
			MergeErrorKind::FoundANone => "FoundANone",
			MergeErrorKind::NotANone => "NotANone",
			MergeErrorKind::MismatchedTypes => "MismatchedTypes",
			MergeErrorKind::LengthsDiffer => "LengthsDiffer",
			MergeErrorKind::KeysDiffer => "KeysDiffer",
			MergeErrorKind::InconsistentGlyphOrder => "InconsistentGlyphOrder",
			MergeErrorKind::InconsistentExtensions => "InconsistentExtensions",
			MergeErrorKind::UnsupportedFormat { .. } => "UnsupportedFormat",
			MergeErrorKind::InconsistentFormats { .. } => "InconsistentFormats",
		}
	}

	/// `reason`: the docstring of the class.
	fn reason(&self) -> String {
		match self {
			MergeErrorKind::ShouldBeConstant => {
				"some values were different, but should have been the same".into()
			}
			MergeErrorKind::FoundANone => {
				"one of the values in a list was empty when it shouldn't have been".into()
			}
			MergeErrorKind::NotANone => {
				"one of the values in a list was not empty when it should have been".into()
			}
			MergeErrorKind::MismatchedTypes => "data had inconsistent types".into(),
			MergeErrorKind::LengthsDiffer => "a list of objects had inconsistent lengths".into(),
			MergeErrorKind::KeysDiffer => "a list of objects had different keys".into(),
			MergeErrorKind::InconsistentGlyphOrder => {
				"the glyph order was inconsistent between masters".into()
			}
			MergeErrorKind::InconsistentExtensions => {
				"the masters use extension lookups in inconsistent ways".into()
			}
			MergeErrorKind::UnsupportedFormat { subtable, value } => {
				let mut s =
					format!("an OpenType subtable ({subtable}) had a format I didn't expect");
				if let Some(v) = value {
					s.push_str(&format!(" ({v})"));
				}
				s
			}
			MergeErrorKind::InconsistentFormats { subtable, value } => {
				let mut s = format!(
					"an OpenType subtable ({subtable}) had inconsistent formats between masters"
				);
				if let Some(v) = value {
					s.push_str(&format!(" ({v})"));
				}
				s
			}
		}
	}
}

/// `VarLibMergeError`: what failed, where (`stack`, innermost first) and in
/// which master.
///
/// fontTools computes the offending master and the message from the merger
/// when the error is printed; they are computed when the error is raised here,
/// which gives the same result as the merger state does not change after that.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeError {
	/// The kind of error.
	pub kind: MergeErrorKind,
	/// The attributes and indices being merged, innermost first (`stack`).
	pub stack: Vec<String>,
	/// The index and name of the offending master (`offender`), if known.
	pub offender: Option<(usize, String)>,
	/// `repr(cause["expected"])` and the `repr` of the offender's value
	/// (`cause["got"][offender]`), if the cause has both.
	pub expected_got: Option<(String, String)>,
	/// `repr(cause["got"])`, for `FoundANone` and `NotANone`.
	pub got_list: Option<String>,
	/// More details: the features of the masters for a different
	/// `FeatureCount`.
	pub extra_details: String,
	/// The expected value, as an integer, for the `FeatureCount` details.
	pub(crate) expected_int: Option<i64>,
	/// The masters of the merger when raised (`merger.ttfs`), as indices.
	pub(crate) ttfs: Vec<usize>,
}

impl MergeError {
	/// An error without a merger (`InconsistentGlyphOrder()`).
	pub fn new(kind: MergeErrorKind) -> Self {
		let stack = match kind {
			MergeErrorKind::UnsupportedFormat { .. }
			| MergeErrorKind::InconsistentFormats { .. } => {
				vec![".Format".to_owned()]
			}
			_ => Vec::new(),
		};
		MergeError {
			kind,
			stack,
			offender: None,
			expected_got: None,
			got_list: None,
			extra_details: String::new(),
			expected_int: None,
			ttfs: Vec::new(),
		}
	}

	/// `details`.
	fn details(&self) -> String {
		let first = self.stack.first().map_or("", String::as_str);
		match self.kind {
			MergeErrorKind::FoundANone | MergeErrorKind::NotANone => {
				format!("{first}=={}\n", self.got_list.as_deref().unwrap_or("[]"))
			}
			_ => {
				let mut s = match &self.expected_got {
					Some((expected, got)) if self.offender.is_some() => {
						format!("Expected to see {first}=={expected}, instead saw {got}\n")
					}
					_ => String::new(),
				};
				s.push_str(&self.extra_details);
				s
			}
		}
	}
}

impl std::fmt::Display for MergeError {
	/// `VarLibMergeError.__str__`.
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let location = match &self.offender {
			Some((_, name)) if !name.is_empty() => {
				format!("\n\nThe problem is likely to be in {name}:\n")
			}
			_ => String::new(),
		};
		let context: String = self.stack.iter().rev().map(String::as_str).collect();
		let basic = textwrap_fill(
			&format!(
				"Couldn't merge the fonts, because {}. This happened while performing the following operation: {context}",
				self.kind.reason()
			),
			78,
		);
		write!(f, "\n\n{basic}{location}{}", self.details())
	}
}

/// The chunks of `textwrap.TextWrapper._split`: words, whitespace, and the
/// parts of hyphenated words.
fn textwrap_chunks(text: &str) -> Vec<String> {
	let mut chunks: Vec<String> = Vec::new();
	let mut current = String::new();
	let mut in_space = false;
	for c in text.chars() {
		let space = c.is_whitespace();
		if space != in_space && !current.is_empty() {
			chunks.push(std::mem::take(&mut current));
		}
		in_space = space;
		// `expand_tabs` and `replace_whitespace`.
		current.push(if space { ' ' } else { c });
		if c == '-' && !space {
			// Break after a hyphen between letters.
			let letters = current.chars().filter(|c| c.is_alphanumeric()).count();
			if letters > 1 {
				chunks.push(std::mem::take(&mut current));
			}
		}
	}
	if !current.is_empty() {
		chunks.push(current);
	}
	chunks
}

/// `textwrap.fill(text, width)`, for the messages of merge errors.
pub(crate) fn textwrap_fill(text: &str, width: usize) -> String {
	let mut chunks: Vec<String> = textwrap_chunks(text);
	chunks.reverse();
	let mut lines: Vec<String> = Vec::new();
	while !chunks.is_empty() {
		let mut line: Vec<String> = Vec::new();
		let mut len = 0;
		// Drop leading whitespace, except on the first line.
		if !lines.is_empty() && chunks.last().is_some_and(|c| c.trim().is_empty()) {
			chunks.pop();
		}
		while let Some(chunk) = chunks.last() {
			let l = chunk.chars().count();
			if len + l <= width {
				len += l;
				line.extend(chunks.pop());
			} else {
				break;
			}
		}
		// A chunk longer than the width is broken.
		if let Some(chunk) = chunks.last()
			&& chunk.chars().count() > width
		{
			let space_left = if width > len { width - len } else { 1 };
			let chunk = chunks.pop().unwrap_or_default();
			let head: String = chunk.chars().take(space_left).collect();
			let tail: String = chunk.chars().skip(space_left).collect();
			line.push(head);
			chunks.push(tail);
		}
		// Drop trailing whitespace.
		if line.last().is_some_and(|c| c.trim().is_empty()) {
			line.pop();
		}
		if !line.is_empty() {
			lines.push(line.concat());
		}
	}
	lines.join("\n")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn fill() {
		assert_eq!(
			textwrap_fill(
				"Couldn't merge the fonts, because some values were different, but should have been the same. This happened while performing the following operation: GPOS.table.FeatureList.FeatureCount",
				78
			),
			"Couldn't merge the fonts, because some values were different, but should have\nbeen the same. This happened while performing the following operation:\nGPOS.table.FeatureList.FeatureCount"
		);
	}
}
