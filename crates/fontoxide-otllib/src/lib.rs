//! Building OpenType Layout tables: a port of `fontTools.otlLib`.
//!
//! | Module | fontTools |
//! |---|---|
//! | [`builder`] | `fontTools.otlLib.builder` |
//! | [`max_context_calc`] | `fontTools.otlLib.maxContextCalc` |
//! | [`optimize`] | `fontTools.otlLib.optimize` |
//! | [`Error`] | `fontTools.otlLib.error` |
//!
//! The `otlLib` options of fontTools' `TTFont.cfg` are fields of
//! [`fontoxide_ttlib::Font`]: `gpos_compression_level` and `write_gpos7`.

#![forbid(unsafe_code)]

pub mod builder;
mod error;
mod glyph_map;
pub mod max_context_calc;
pub mod optimize;

pub use error::{Error, Location, Result};
pub use glyph_map::GlyphMap;
