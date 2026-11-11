//! A Rust port of [fontTools](https://github.com/fonttools/fonttools).
//!
//! This umbrella crate re-exports the individual `fontoxide-*` crates, each of
//! which corresponds to a subpackage of `fontTools`. The small top-level modules
//! [`afmlib`], [`agl`] and [`tfmlib`] live in `fontoxide-misc`:
//!
//! | fontoxide            | fontTools          | Feature |
//! |----------------------|--------------------|---------|
//! | [`afmlib`]           | `fontTools.afmLib` | always |
//! | [`agl`]              | `fontTools.agl` | always |
//! | [`colorlib`]         | `fontTools.colorLib` | `colorlib` |
//! | [`cu2qu`]            | `fontTools.cu2qu` | `cu2qu` |
//! | [`designspacelib`]   | `fontTools.designspaceLib` | `designspacelib` |
//! | [`fealib`]           | `fontTools.feaLib` | `fealib` |
//! | [`merge`]            | `fontTools.merge` | `merge` |
//! | [`misc`]             | `fontTools.misc` | always |
//! | [`mtilib`]           | `fontTools.mtiLib` | `mtilib` |
//! | [`otllib`]           | `fontTools.otlLib` | `otllib` |
//! | [`pens`]             | `fontTools.pens` | always |
//! | [`qu2cu`]            | `fontTools.qu2cu` | `qu2cu` |
//! | [`repack`]           | the HarfBuzz repacker (uharfbuzz's `serialize_with_tag`) | `repacker` |
//! | [`subset`]           | `fontTools.subset` | `subset` |
//! | [`svglib`]           | `fontTools.svgLib` | `svglib` |
//! | [`t1lib`]            | `fontTools.t1Lib` | `t1lib` |
//! | [`tfmlib`]           | `fontTools.tfmLib` | always |
//! | [`ttlib`]            | `fontTools.ttLib` | always |
//! | [`ufolib`]           | `fontTools.ufoLib` | `ufolib` |
//! | [`unicodedata`]      | `fontTools.unicodedata` | always |
//! | [`varlib`]           | `fontTools.varLib` | `varlib` |
//! | [`voltlib`]          | `fontTools.voltLib` | `voltlib` |
//!
//! [`font_builder`] is `fontTools.fontBuilder`; its methods that build tables
//! with another subpackage need that feature (for example
//! [`add_open_type_features`](font_builder::FontBuilder::add_open_type_features)
//! needs `fealib`).
//!
//! `fontTools.pens.ttGlyphPen` and `fontTools.pens.t2CharStringPen` are in
//! [`ttlib::pens`], as they build `glyf` glyphs and CFF charstrings.
//!
//! # Features
//!
//! By default, everything is included, as with `pip install fonttools[all]`.
//! With `default-features = false`, only [`ttlib`], [`misc`], [`pens`] and
//! [`unicodedata`] are; each subpackage above is a feature of the same name.
//!
//! The features `woff`, `graphite`, `pathops` and `repacker` are fontTools'
//! extras (the optional Python packages). Without one, fontoxide behaves as
//! fontTools does without that package; see the
//! [features of `ttlib`](ttlib#features).
//!
//! Each `fontoxide-*` crate enables only the extras it cannot do without (none,
//! currently), so that leaving one out here drops its dependencies. Users of the
//! individual crates enable the extras with the features of `fontoxide-ttlib`.

#[cfg(feature = "colorlib")]
pub use fontoxide_colorlib as colorlib;
#[cfg(feature = "cu2qu")]
pub use fontoxide_cu2qu as cu2qu;
#[cfg(feature = "designspacelib")]
pub use fontoxide_designspacelib as designspacelib;
#[cfg(feature = "fealib")]
pub use fontoxide_fealib as fealib;
#[cfg(feature = "merge")]
pub use fontoxide_merge as merge;
pub use fontoxide_misc as misc;
pub use fontoxide_misc::afm_lib as afmlib;
pub use fontoxide_misc::agl;
pub use fontoxide_misc::tfm_lib as tfmlib;
#[cfg(feature = "mtilib")]
pub use fontoxide_mtilib as mtilib;
#[cfg(feature = "otllib")]
pub use fontoxide_otllib as otllib;
pub use fontoxide_pens as pens;
#[cfg(feature = "qu2cu")]
pub use fontoxide_qu2cu as qu2cu;
#[cfg(feature = "repacker")]
pub use fontoxide_repack as repack;
#[cfg(feature = "subset")]
pub use fontoxide_subset as subset;
#[cfg(feature = "svglib")]
pub use fontoxide_svglib as svglib;
#[cfg(feature = "t1lib")]
pub use fontoxide_t1lib as t1lib;
pub use fontoxide_ttlib as ttlib;
#[cfg(feature = "ufolib")]
pub use fontoxide_ufolib as ufolib;
pub use fontoxide_unicodedata as unicodedata;
#[cfg(feature = "varlib")]
pub use fontoxide_varlib as varlib;
#[cfg(feature = "voltlib")]
pub use fontoxide_voltlib as voltlib;

pub mod font_builder;

/// The fontTools release whose behavior this version of fontoxide matches.
pub use fontoxide_ttlib::FONTTOOLS_VERSION;
