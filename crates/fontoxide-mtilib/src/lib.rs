//! Building OpenType Layout tables from Monotype/FontDame layout source files: a
//! port of `fontTools.mtiLib`.
//!
//! The source language is described at
//! <http://monotype.github.io/OpenType_Table_Source/otl_source.html>.
//!
//! | Item | fontTools |
//! |---|---|
//! | [`build`], [`build_with`] | `fontTools.mtiLib.build` |
//! | [`cli::main`] | `fontTools.mtiLib.main` (`fonttools mtiLib`) |
//! | [`Error`] | `MtiLibError` and the exceptions the parser raises |
//! | [`make_glyph`] | `makeGlyph` |
//!
//! The tables are built as `GSUB`, `GPOS`, `GDEF` or `cmap` tables of
//! `fontoxide_ttlib`, mostly with the builders of `fontoxide_otllib`, as in
//! fontTools.

#![forbid(unsafe_code)]

mod argparse;
pub mod cli;
mod context;
mod glyphs;
mod parser;
mod tokenizer;

use fontoxide_ttlib::{Font, Table};

pub use parser::make_glyph;

/// The key of the lookup debugging information in the `Debg` table
/// (`fontTools.feaLib.lookupDebugInfo.LOOKUP_DEBUG_INFO_KEY`).
pub use fontoxide_ttlib::ot::custom::LOOKUP_DEBUG_INFO_KEY;

/// The environment variable that makes [`build`] add lookup debugging
/// information to the `Debg` table of the font
/// (`fontTools.feaLib.lookupDebugInfo.LOOKUP_DEBUG_ENV_VAR`).
pub const LOOKUP_DEBUG_ENV_VAR: &str = "FONTTOOLS_LOOKUP_DEBUGGING";

/// An error building a table.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// `FeatureNotFoundError`: a reference to a feature name that the feature
	/// table does not define.
	#[error("{0}")]
	FeatureNotFound(String),
	/// `LookupNotFoundError`: a reference to a lookup name that no lookup has.
	#[error("{0}")]
	LookupNotFound(String),
	/// `AssertionError`: the source breaks an assumption of the parser.
	#[error("AssertionError: {0}")]
	Assertion(String),
	/// `ValueError`, `KeyError`, `IndexError` and such: the source is
	/// malformed, or a value does not fit its field.
	#[error("{0}")]
	Value(String),
	/// An error from otlLib.
	#[error(transparent)]
	Otl(#[from] fontoxide_otllib::Error),
	/// An error from ttLib.
	#[error(transparent)]
	TtLib(#[from] fontoxide_ttlib::Error),
}

/// The result type of this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// The options of [`build_with`].
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
	/// The table the source must be for (`tableTag`); without it, the source
	/// must name its table in its first line.
	pub table_tag: Option<String>,
	/// Adds lookup debugging information to the `Debg` table of the font, as
	/// the [`LOOKUP_DEBUG_ENV_VAR`] environment variable does.
	pub debug: bool,
	/// Glyphs that are not in the font's glyph order are added to it, as with
	/// fontTools' `MockFont` (which the command line uses without `--font`); a
	/// font without a glyph order starts with `.notdef`. Without this, glyphs
	/// must be in the font (or be `glyphNNNNN` names).
	pub add_glyphs: bool,
}

/// Converts a Monotype font layout file to an OpenType layout table (`build`).
///
/// The font is used for the glyph IDs, which also sort coverage tables, and
/// receives the lookup debugging information when the
/// [`LOOKUP_DEBUG_ENV_VAR`] environment variable is set. `table_tag`, if
/// given, is the table the file must be for.
///
/// Returns the table (`GSUB`, `GPOS`, `GDEF` or `cmap`); it is not added to
/// the font.
pub fn build(text: &str, font: &mut Font, table_tag: Option<&str>) -> Result<Box<dyn Table>> {
	build_with(
		text,
		font,
		&BuildOptions {
			table_tag: table_tag.map(str::to_owned),
			..BuildOptions::default()
		},
	)
}

/// [`build`] with more options.
pub fn build_with(text: &str, font: &mut Font, options: &BuildOptions) -> Result<Box<dyn Table>> {
	let debug =
		options.debug || std::env::var_os(LOOKUP_DEBUG_ENV_VAR).is_some_and(|v| !v.is_empty());
	parser::build(
		text,
		font,
		options.table_tag.as_deref(),
		debug,
		options.add_glyphs,
	)
}
