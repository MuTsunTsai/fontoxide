//! Port of `fontTools.cffLib`: CFF and CFF2 font data, and Type 2 charstrings
//! (`fontTools.misc.psCharStrings`).

pub mod charstring;
pub mod compile;
pub mod dict;
pub mod font;
mod from_xml;
mod xml;
