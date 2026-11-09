//! A Rust port of [fontTools](https://github.com/fonttools/fonttools).
//!
//! This umbrella crate re-exports the individual `fontoxide-*` crates, each of
//! which corresponds to a subpackage of `fontTools`:
//!
//! | fontoxide            | fontTools          |
//! |----------------------|--------------------|
//! | [`agl`]              | `fontTools.agl`    |
//! | [`misc`]             | `fontTools.misc`   |
//! | [`pens`]             | `fontTools.pens`   |
//! | [`repack`]           | the HarfBuzz repacker (uharfbuzz's `serialize_with_tag`) |
//! | [`subset`]           | `fontTools.subset` |
//! | [`ttlib`]            | `fontTools.ttLib`  |
//! | [`unicodedata`]      | `fontTools.unicodedata` |
//! | [`varlib`]           | `fontTools.varLib` |

pub use fontoxide_agl as agl;
pub use fontoxide_misc as misc;
pub use fontoxide_pens as pens;
pub use fontoxide_repack as repack;
pub use fontoxide_subset as subset;
pub use fontoxide_ttlib as ttlib;
pub use fontoxide_unicodedata as unicodedata;
pub use fontoxide_varlib as varlib;

/// The fontTools release whose behavior this version of fontoxide matches.
pub use fontoxide_ttlib::FONTTOOLS_VERSION;
