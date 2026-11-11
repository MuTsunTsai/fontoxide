//! Port of `fontTools.misc`: low-level helpers shared by the other crates.
//!
//! | fontoxide           | fontTools                     |
//! |---------------------|-------------------------------|
//! | [`array_tools`]     | `fontTools.misc.arrayTools`   |
//! | [`bezier_tools`]    | `fontTools.misc.bezierTools`  |
//! | [`cli_tools`]       | `fontTools.misc.cliTools`     |
//! | [`encoding_tools`]  | `fontTools.misc.encodingTools` |
//! | [`etree`]           | `fontTools.misc.etree` (lxml serialization) |
//! | [`fixed_tools`]     | `fontTools.misc.fixedTools`   |
//! | [`ift_sparse_bit_set`] | `fontTools.misc.iftSparseBitSet` |
//! | [`plistlib`]        | `fontTools.misc.plistlib`     |
//! | [`round_tools`]     | `fontTools.misc.roundTools`   |
//! | [`text_tools`]      | `fontTools.misc.textTools`    |
//! | [`time_tools`]      | `fontTools.misc.timeTools`    |
//! | [`transform`]       | `fontTools.misc.transform`    |
//! | [`tree_tools`]      | `fontTools.misc.treeTools`    |
//! | [`xml_writer`]      | `fontTools.misc.xmlWriter`    |
//! | [`xml_reader`]      | `fontTools.misc.xmlReader`    |
//!
//! [`py`] reproduces the Python formatting rules that fontTools output relies on,
//! and [`complex`] Python's complex arithmetic, which the curve math uses.

pub mod array_tools;
pub mod bezier_tools;
pub mod cli_tools;
pub mod complex;
pub mod encoding_tools;
pub mod etree;
pub mod fixed_tools;
pub mod ift_sparse_bit_set;
pub mod plistlib;
pub mod py;
pub mod round_tools;
pub mod text_tools;
pub mod time_tools;
pub mod transform;
pub mod tree_tools;
pub mod xml_reader;
pub mod xml_writer;

pub use text_tools::Tag;
