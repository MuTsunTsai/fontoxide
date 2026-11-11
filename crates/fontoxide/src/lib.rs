//! A Rust port of [fontTools](https://github.com/fonttools/fonttools).
//!
//! This umbrella crate re-exports the individual `fontoxide-*` crates, each of
//! which corresponds to a subpackage of `fontTools`:
//!
//! | fontoxide            | fontTools          |
//! |----------------------|--------------------|
//! | [`agl`]              | `fontTools.agl`    |
//! | [`colorlib`]         | `fontTools.colorLib` |
//! | [`cu2qu`]            | `fontTools.cu2qu`  |
//! | [`designspacelib`]   | `fontTools.designspaceLib` |
//! | [`fealib`]           | `fontTools.feaLib` |
//! | [`misc`]             | `fontTools.misc`   |
//! | [`otllib`]           | `fontTools.otlLib` |
//! | [`pens`]             | `fontTools.pens`   |
//! | [`qu2cu`]            | `fontTools.qu2cu`  |
//! | [`repack`]           | the HarfBuzz repacker (uharfbuzz's `serialize_with_tag`) |
//! | [`subset`]           | `fontTools.subset` |
//! | [`ttlib`]            | `fontTools.ttLib`  |
//! | [`unicodedata`]      | `fontTools.unicodedata` |
//! | [`varlib`]           | `fontTools.varLib` |
//!
//! [`font_builder`] is `fontTools.fontBuilder`.
//!
//! `fontTools.pens.ttGlyphPen` and `fontTools.pens.t2CharStringPen` are in
//! [`ttlib::pens`], as they build `glyf` glyphs and CFF charstrings.

pub use fontoxide_agl as agl;
pub use fontoxide_colorlib as colorlib;
pub use fontoxide_cu2qu as cu2qu;
pub use fontoxide_designspacelib as designspacelib;
pub use fontoxide_fealib as fealib;
pub use fontoxide_misc as misc;
pub use fontoxide_otllib as otllib;
pub use fontoxide_pens as pens;
pub use fontoxide_qu2cu as qu2cu;
pub use fontoxide_repack as repack;
pub use fontoxide_subset as subset;
pub use fontoxide_ttlib as ttlib;
pub use fontoxide_unicodedata as unicodedata;
pub use fontoxide_varlib as varlib;

pub mod font_builder;

/// The fontTools release whose behavior this version of fontoxide matches.
pub use fontoxide_ttlib::FONTTOOLS_VERSION;
