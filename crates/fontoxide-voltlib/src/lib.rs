//! Reading MS VOLT projects and converting them to feature files; a port of
//! `fontTools.voltLib`.
//!
//! | Module | fontTools |
//! |---|---|
//! | [`ast`] | `fontTools.voltLib.ast` |
//! | [`lexer`] | `fontTools.voltLib.lexer` |
//! | [`parser`] | `fontTools.voltLib.parser` |
//! | [`volt_to_fea`] | `fontTools.voltLib.voltToFea` |
//! | [`cli`] | `fontTools.voltLib.__main__` |
//! | [`Error`] | `fontTools.voltLib.error` |
//!
//! ```
//! use fontoxide_voltlib::{Parser, ParserInput};
//!
//! let doc = Parser::new(ParserInput::text("DEF_GLYPH \"a\" ID 1 END_GLYPH END"))?.parse()?;
//! assert_eq!(doc.to_string(), "\nDEF_GLYPH \"a\" ID 1 END_GLYPH END\n");
//! # Ok::<(), fontoxide_voltlib::Error>(())
//! ```

#![forbid(unsafe_code)]

pub mod ast;
pub mod cli;
mod error;
pub mod lexer;
pub mod parser;
pub mod volt_to_fea;

pub use error::{Error, Location, Result, VoltLibLocation};
pub use parser::{Parser, ParserInput};
pub use volt_to_fea::{VoltInput, VoltToFea};
