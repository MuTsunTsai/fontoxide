//! The errors of converting glyphs and fonts (`fontTools.cu2qu.errors`,
//! besides [`crate::Error`]).

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use fontoxide_misc::py::str_repr;

/// What is incompatible in an [`IncompatibleGlyphsError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incompatibility {
	/// `IncompatibleSegmentNumberError`: the glyphs have different numbers of
	/// segments.
	SegmentNumber,
	/// `IncompatibleSegmentTypesError`: the segments at some indices have
	/// different types (`segments`: the index to the segment types, such as
	/// `"line"` and `"curve"`).
	SegmentTypes(BTreeMap<usize, Vec<String>>),
}

/// `IncompatibleGlyphsError`: glyphs that cannot be converted together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncompatibleGlyphsError {
	/// The names of the glyphs (`glyphs`, by their `name`).
	pub glyph_names: Vec<Option<String>>,
	/// What is incompatible.
	pub kind: Incompatibility,
}

impl IncompatibleGlyphsError {
	/// `combined_name`: the `repr` of the glyph name, or the sorted set of
	/// them in braces if they differ.
	pub fn combined_name(&self) -> String {
		let names: BTreeSet<String> = self
			.glyph_names
			.iter()
			.map(|n| match n {
				Some(n) => str_repr(n),
				None => "None".to_owned(),
			})
			.collect();
		if names.len() > 1 {
			let names: Vec<String> = names.into_iter().collect();
			format!("{{{}}}", names.join(", "))
		} else {
			names.into_iter().next().unwrap_or_default()
		}
	}

	/// `segments` of an `IncompatibleSegmentTypesError`.
	pub fn segments(&self) -> Option<&BTreeMap<usize, Vec<String>>> {
		match &self.kind {
			Incompatibility::SegmentTypes(s) => Some(s),
			Incompatibility::SegmentNumber => None,
		}
	}
}

impl std::fmt::Display for IncompatibleGlyphsError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let name = self.combined_name();
		match &self.kind {
			Incompatibility::SegmentNumber => {
				write!(f, "Glyphs named {name} have different number of segments")
			}
			Incompatibility::SegmentTypes(segments) => {
				let ndigits = segments.keys().max().map_or(1, |m| m.to_string().len());
				let lines: Vec<String> = segments
					.iter()
					.map(|(i, tags)| {
						let tags: Vec<String> = tags.iter().map(|t| str_repr(t)).collect();
						format!("{i:>ndigits$}: ({})", tags.join(", "))
					})
					.collect();
				write!(
					f,
					"Glyphs named {name} have incompatible segment types:\n  {}",
					lines.join("\n  ")
				)
			}
		}
	}
}

impl std::error::Error for IncompatibleGlyphsError {}

/// `IncompatibleFontsError`: same-named glyphs of the fonts that cannot be
/// converted together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncompatibleFontsError {
	/// `glyph_errors`: the error of each glyph name.
	pub glyph_errors: BTreeMap<String, IncompatibleGlyphsError>,
}

impl std::fmt::Display for IncompatibleFontsError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let names: Vec<String> = self.glyph_errors.keys().map(|n| str_repr(n)).collect();
		write!(
			f,
			"fonts contains incompatible glyphs: {}",
			names.join(", ")
		)
	}
}

impl std::error::Error for IncompatibleFontsError {}
