//! Port of `fontTools.cffLib`: CFF and CFF2 font data, and Type 2 charstrings
//! (`fontTools.misc.psCharStrings`).

pub mod cff2_to_cff;
pub mod cff_to_cff2;
pub mod charstring;
pub mod compile;
pub mod dict;
pub mod font;
mod from_xml;
pub mod specializer;
pub mod transforms;
pub mod width;
mod xml;
