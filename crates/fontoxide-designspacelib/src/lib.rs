//! Designspace documents; a port of `fontTools.designspaceLib`.
//!
//! | fontoxide       | fontTools                          |
//! |-----------------|------------------------------------|
//! | crate root      | `fontTools.designspaceLib`         |
//! | [`cli`]         | `fontTools.designspaceLib.main` (`__main__`) |
//! | [`split`]       | `fontTools.designspaceLib.split`   |
//! | [`stat_names`]  | `fontTools.designspaceLib.statNames` |
//! | [`types`]       | `fontTools.designspaceLib.types`   |
//!
//! Python keeps whatever `int` or `float` it is given in a document, and that
//! shows in some output (for example `str(value)` in variable font names), so
//! values are [`Number`]s. Read documents always contain floats.

#![forbid(unsafe_code)]

pub mod cli;
mod descriptors;
mod document;
pub mod models;
mod number;
pub mod paths;
mod reader;
pub mod split;
pub mod stat_names;
pub mod types;
mod writer;

pub use descriptors::{
	AnisotropicLocation, Axis, AxisDescriptor, AxisLabelDescriptor, AxisMappingDescriptor,
	AxisSubset, Condition, DiscreteAxisDescriptor, GlyphData, GlyphMaster, InstanceDescriptor,
	LocalizedNames, LocationLabelDescriptor, LocationValue, RangeAxisSubsetDescriptor,
	RuleDescriptor, SimpleLocation, SourceDescriptor, ValueAxisSubsetDescriptor,
	VariableFontDescriptor, evaluate_conditions, evaluate_rule, process_rules, tag_for_axis_name,
	to_anisotropic,
};
pub use document::DesignSpaceDocument;
pub use number::Number;
pub use paths::posix;
pub use types::get_vf_user_region;

pub use fontoxide_misc::plistlib;

/// `DesignSpaceDocumentError`, and the Python exceptions designspaceLib lets
/// through.
#[derive(Debug, thiserror::Error)]
pub enum DesignSpaceDocumentError {
	/// `DesignSpaceDocumentError` proper.
	#[error("{0}")]
	Document(String),
	/// A Python `ValueError`, `TypeError`, `KeyError` or `AttributeError` that
	/// fontTools would raise on bad data.
	#[error("{0}")]
	Value(String),
	/// An XML syntax error (`etree.ParseError`).
	#[error("{0}")]
	Xml(String),
	/// A plist error in a `lib` element.
	#[error(transparent)]
	Plist(#[from] plistlib::PlistError),
	/// A file could not be read or written.
	#[error(transparent)]
	Io(#[from] std::io::Error),
}

/// The result type of this crate.
pub type Result<T> = std::result::Result<T, DesignSpaceDocumentError>;

/// Builds a [`SimpleLocation`] from `(name, value)` pairs.
pub fn simple_location<K: Into<String>, N: Into<Number>>(
	items: impl IntoIterator<Item = (K, N)>,
) -> SimpleLocation {
	items
		.into_iter()
		.map(|(k, v)| (k.into(), v.into()))
		.collect()
}

/// Builds an [`AnisotropicLocation`] from `(name, value)` pairs.
pub fn anisotropic_location<K: Into<String>, V: Into<LocationValue>>(
	items: impl IntoIterator<Item = (K, V)>,
) -> AnisotropicLocation {
	items
		.into_iter()
		.map(|(k, v)| (k.into(), v.into()))
		.collect()
}
