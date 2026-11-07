//! A Rust port of [fontTools](https://github.com/fonttools/fonttools).
//!
//! This umbrella crate re-exports the individual `fontoxide-*` crates, each of
//! which corresponds to a subpackage of `fontTools`:
//!
//! | fontoxide            | fontTools          |
//! |----------------------|--------------------|
//! | [`misc`]             | `fontTools.misc`   |
//! | [`ttlib`]            | `fontTools.ttLib`  |

pub use fontoxide_misc as misc;
pub use fontoxide_ttlib as ttlib;

/// The fontTools release whose behavior this version of fontoxide matches.
pub const FONTTOOLS_VERSION: &str = "4.68.0";
