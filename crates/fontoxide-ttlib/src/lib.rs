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

#![forbid(unsafe_code)]

pub mod binary;
pub mod collection;
mod error;
pub mod font;
mod generated;
pub mod glyph_order;
pub mod sfnt;
pub mod sstruct;
pub mod table;
pub mod tables;
pub mod ttx;
pub mod woff2;

pub use collection::TtCollection;
pub use error::{Error, Result};
pub use font::{Font, ReadOptions, TableKey, TtxOptions, WriteOptions};
pub use fontoxide_misc::Tag;
pub use glyph_order::GlyphOrder;
pub use sfnt::{Flavor, FlavorData};
pub use table::{DefaultTable, Table, TableType};
pub use ttx::TtxImportOptions;

/// The fontTools release whose behavior this version matches.
pub const FONTTOOLS_VERSION: &str = "4.68.0";

/// Facts about fontTools' table classes, generated from upstream.
pub mod fonttools_tables {
	pub use crate::generated::tables::{FONTTOOLS_TABLES, dependencies};
}
