//! Port of `fontTools.ttLib`: the [`Font`] object (`TTFont`), sfnt/TTC/WOFF/WOFF2
//! containers, table implementations, and TTX serialization.
//!
//! ```no_run
//! use fontoxide_ttlib::{Font, ReadOptions, TtxOptions, tables::Head};
//!
//! let data = std::fs::read("font.ttf")?;
//! let mut font = Font::read(data, ReadOptions::default())?;
//! let head: &mut Head = font.require::<Head>()?;
//! println!("{}", head.units_per_em);
//! let ttx = font.to_ttx(&TtxOptions::default())?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Features
//!
//! The features are fontTools' extras, all on by default. Without one,
//! fontoxide behaves as fontTools does without that Python package:
//!
//! | Feature    | fontTools extra (package) | Without it |
//! |------------|---------------------------|------------|
//! | `woff`     | `woff` (brotli, zopfli)   | reading and writing WOFF 2.0, and WOFF 1.0 with [`WriteOptions::use_zopfli`], fail with [`Error::Import`] |
//! | `graphite` | `graphite` (lz4)          | LZ4-compressed `Silf` and `Glat` tables are kept compressed, with a warning |
//! | `pathops`  | `pathops` (skia-pathops)  | [`remove_overlaps`](remove_overlaps::remove_overlaps) and the command line [`remove_overlaps::main`] fail with [`Error::Import`] |
//! | `repacker` | `repacker` (uharfbuzz)    | `GSUB` and `GPOS` are packed with fontTools' serializer; [`Font::use_harfbuzz_repacker`] `Some(true)` fails with [`Error::Import`] |

#![forbid(unsafe_code)]

pub mod beyond64k;
pub mod binary;
pub mod cff;
pub mod cli;
pub mod collection;
mod error;
pub mod font;
mod generated;
pub mod glyph_order;
pub mod glyph_set;
pub mod iup;
pub mod mac_utils;
pub mod ot;
pub mod pens;
pub mod remove_overlaps;
pub mod reorder_glyphs;
pub mod scale_upem;
pub mod sfnt;
pub mod sstruct;
pub mod table;
pub mod tables;
pub mod ttx;
pub mod var;
pub mod woff2;
pub mod woff2_cli;

pub use collection::TtCollection;
pub use error::{Error, Result};
pub use font::{Font, Progress, ReadOptions, TableKey, TtxOptions, WriteOptions};
pub use fontoxide_misc::Tag;
pub use fontoxide_misc::time_tools::set_clock;
pub use glyph_order::GlyphOrder;
pub use glyph_set::GlyphSet;
pub use sfnt::{Flavor, FlavorData};
pub use table::{DefaultTable, Table, TableType};
pub use ttx::{TtxFile, TtxImportOptions};

/// The fontTools release whose behavior this version matches.
pub const FONTTOOLS_VERSION: &str = "4.68.0";

/// Facts about fontTools' table classes, generated from upstream.
pub mod fonttools_tables {
	pub use crate::generated::tables::{FONTTOOLS_TABLES, dependencies};
}
