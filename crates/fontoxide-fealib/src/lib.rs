//! Feature file parsing and compiling; a port of `fontTools.feaLib`.
//!
//! | Module | fontTools |
//! |---|---|
//! | [`ast`] | `fontTools.feaLib.ast` |
//! | [`builder`] | `fontTools.feaLib.builder` |
//! | [`cli`] | `fontTools.feaLib.__main__` |
//! | [`lexer`] | `fontTools.feaLib.lexer` |
//! | [`parser`] | `fontTools.feaLib.parser` |
//! | [`variable_scalar`] | `fontTools.feaLib.variableScalar` |
//! | [`lookup_debug_info`] | `fontTools.feaLib.lookupDebugInfo` |
//! | [`FeatureLibLocation`] | `fontTools.feaLib.location` |
//! | [`Error`] | `fontTools.feaLib.error` |
//!
//! ```
//! use fontoxide_fealib::lexer::FeatureSource;
//! use fontoxide_fealib::parser::Parser;
//!
//! let source = FeatureSource::text("feature liga { sub f i by f_i; } liga;");
//! let doc = Parser::new(source, Vec::<String>::new())?.parse()?;
//! assert_eq!(doc.as_fea()?, "feature liga {\n    sub f i by f_i;\n} liga;\n");
//! # Ok::<(), fontoxide_fealib::Error>(())
//! ```
//!
//! Building the features into a font:
//!
//! ```
//! use fontoxide_fealib::builder::add_open_type_features_from_string;
//! use fontoxide_ttlib::{Font, Tag};
//!
//! let mut font = Font::new();
//! font.set_glyph_order([".notdef", "f", "i", "f_i"].map(String::from).to_vec());
//! add_open_type_features_from_string(&mut font, "feature liga { sub f i by f_i; } liga;", None, None, false)?;
//! assert!(font.contains(Tag::new(b"GSUB")));
//! # Ok::<(), fontoxide_fealib::Error>(())
//! ```

#![forbid(unsafe_code)]

pub mod ast;
pub mod builder;
pub mod cli;
mod error;
pub mod lexer;
mod location;
pub mod lookup_debug_info;
pub mod parser;
mod py_set;
pub mod variable_scalar;

pub use error::{Error, Result};
pub use location::{FeatureLibLocation, Location};
pub use py_set::py_int_set_order;
