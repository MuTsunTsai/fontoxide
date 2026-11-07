//! The machinery behind OpenType Layout and related tables
//! (`fontTools.ttLib.tables.otBase` and `otConverters`).
//!
//! The table types themselves are generated from fontTools' `otData` into
//! `crate::generated::ot`; this module provides what the generated code uses.

pub mod conv;
pub mod custom;
pub mod gsub;
pub mod reader;
pub mod state;
pub mod table;
pub mod writer;

pub use reader::OtReader;
pub use state::{CountRef, State, StateValue};
pub use writer::{OtWriter, OverflowErrorRecord, WriterGraph};
