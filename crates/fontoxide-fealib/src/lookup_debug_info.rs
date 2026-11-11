//! Lookup debugging information (`fontTools.feaLib.lookupDebugInfo`).

/// The key of the lookup debugging information in the `Debg` table
/// (`LOOKUP_DEBUG_INFO_KEY`).
pub const LOOKUP_DEBUG_INFO_KEY: &str = "com.github.fonttools.feaLib";

/// The environment variable that turns on lookup debugging
/// (`LOOKUP_DEBUG_ENV_VAR`).
pub const LOOKUP_DEBUG_ENV_VAR: &str = "FONTTOOLS_LOOKUP_DEBUGGING";

/// Information about where a lookup came from, to be embedded in a font
/// (`LookupDebugInfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupDebugInfo {
	/// `str()` of the lookup's location.
	pub location: String,
	/// The lookup name, or `None` for a lookup outside a named lookup block.
	pub name: Option<String>,
	/// The script, language and feature tag the lookup is used in, or `None`.
	pub feature: Option<Vec<String>>,
}
