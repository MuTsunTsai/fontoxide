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
//! | [`ttlib`]            | `fontTools.ttLib`  |
//! | [`unicodedata`]      | `fontTools.unicodedata` |

pub use fontoxide_agl as agl;
pub use fontoxide_misc as misc;
pub use fontoxide_pens as pens;
pub use fontoxide_repack as repack;
pub use fontoxide_ttlib as ttlib;
pub use fontoxide_unicodedata as unicodedata;

/// The fontTools release whose behavior this version of fontoxide matches.
pub use fontoxide_ttlib::FONTTOOLS_VERSION;
