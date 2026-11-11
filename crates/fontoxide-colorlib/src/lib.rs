//! Building COLR and CPAL tables: a port of `fontTools.colorLib`.
//!
//! | Module | fontTools |
//! |---|---|
//! | [`builder`] | `fontTools.colorLib.builder` |
//! | [`table_builder`] | `fontTools.colorLib.table_builder` |
//! | [`unbuilder`] | `fontTools.colorLib.unbuilder` |
//! | [`geometry`] | `fontTools.colorLib.geometry` |
//! | [`Error`] | `fontTools.colorLib.errors` |
//!
//! fontTools builds the tables from dicts, tuples, numbers and enum names,
//! setting their attributes reflectively from the converters of otData. Here
//! such inputs are [`Value`]s, written with the [`value!`] macro, and the
//! converters of the classes that colorLib builds are generated from fontTools
//! (`tools/reference/colorlib_converters.py`).
//!
//! fontTools keeps glyph names in the tables it builds and looks them up when
//! compiling; the tables of fontoxide hold glyph IDs, so the builders take a
//! glyph name to ID map ([`GlyphIds`]).

#![forbid(unsafe_code)]

pub mod builder;
mod enums;
mod error;
#[rustfmt::skip]
mod generated;
pub mod geometry;
pub mod table_builder;
pub mod unbuilder;
mod value;

pub use builder::{
	BuildColrOptions, ColorGlyphs, ColorGlyphsV0, ColorPaletteType, LayerListBuilder,
	LayerReuseCache, LocalizedString, build_colr, build_colr_v1, build_cpal,
};
pub use enums::{CompositeMode, ExtendMode, PaintFormat};
pub use error::{Error, Result};
pub use generated::{Class, FromTable, Table, TableRef};
pub use table_builder::GlyphIds;
pub use value::Value;

#[doc(hidden)]
pub mod __private {
	pub use indexmap::IndexMap;
}
