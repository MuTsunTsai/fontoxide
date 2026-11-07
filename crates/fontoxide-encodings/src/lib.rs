//! Port of `fontTools.encodings`, plus the Python codecs that fontTools relies on
//! for `name` table records.
//!
//! [`codecs`] decodes and encodes text with the codecs named by
//! `fontTools.misc.encodingTools.getEncoding`, matching CPython's codecs exactly
//! (the tables in `data/codecs` are exported from the fontTools reference
//! environment), including fontTools' own `x_mac_*_ttx` codecs.

pub mod codecs;
