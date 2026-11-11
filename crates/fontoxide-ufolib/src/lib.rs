//! Reading and writing UFO fonts: a port of `fontTools.ufoLib`.
//!
//! | fontoxide         | fontTools                       |
//! |-------------------|---------------------------------|
//! | crate root        | `fontTools.ufoLib`              |
//! | [`converters`]    | `fontTools.ufoLib.converters`   |
//! | [`filenames`]     | `fontTools.ufoLib.filenames`    |
//! | [`fontinfo`]      | the `fontinfo.plist` support of `fontTools.ufoLib` |
//! | [`glif_lib`]      | `fontTools.ufoLib.glifLib`      |
//! | [`kerning`]       | `fontTools.ufoLib.kerning`      |
//! | [`objects`]       | a small part of ufoLib2's `ufoLib2.objects` |
//! | [`validators`]    | `fontTools.ufoLib.validators`   |
//! | [`UfoLibError`]   | `fontTools.ufoLib.errors`       |
//!
//! [`UfoReader`] reads UFO 1, 2 and 3 fonts (directories or zip files),
//! converting the kerning groups of UFO 1 and 2 to UFO 3; [`UfoWriter`]
//! writes UFO 1, 2 and 3. Glyphs are read and written through
//! [`GlyphSet`]s:
//!
//! ```no_run
//! # fn main() -> fontoxide_ufolib::Result<()> {
//! use fontoxide_pens::RecordingPointPen;
//! use fontoxide_ufolib::UfoReader;
//!
//! let reader = UfoReader::new("Font.ufo", true)?;
//! let glyph_set = reader.get_glyph_set(None, None, None)?;
//! let mut pen = RecordingPointPen::default();
//! glyph_set.draw_points("a", &mut pen)?;
//! let glyph = glyph_set.glyph("a")?; // width, unicodes, anchors, ...
//! # Ok(())
//! # }
//! ```
//!
//! fontTools reads font info and glyph data into arbitrary objects with
//! `setattr`; here font info is a [`Dict`] of attribute names to values, and
//! glyphs implement [`GlyphObject`] (for example [`Glyph`]). Values are plist
//! [`Value`]s, which keep the Python types that validation looks at.

#![forbid(unsafe_code)]

mod base_io;
pub mod converters;
mod errors;
pub mod filenames;
pub mod fontinfo;
mod fs;
pub mod glif_lib;
pub mod kerning;
pub mod objects;
mod ufo;
mod utils;
pub mod validators;
mod zip;

use indexmap::IndexMap;

pub use errors::{Result, UfoLibError};
pub use fontoxide_misc::plistlib::{self, Dict, Value};
pub use fs::Fs;
pub use glif_lib::{
	DrawPointsFunc, GlifPointPen, Glyph, GlyphNameToFileNameFunc, GlyphObject, GlyphSet,
	GlyphSetOptions, NumberPointPen, PointPenAdapter, glyph_name_to_file_name,
	read_glyph_from_string, write_glyph_to_string,
};
pub use ufo::{
	DATA_DIRNAME, DEFAULT_GLYPHS_DIRNAME, DEFAULT_LAYER_NAME, FEATURES_FILENAME, FONTINFO_FILENAME,
	GROUPS_FILENAME, IMAGES_DIRNAME, KERNING_FILENAME, LAYERCONTENTS_FILENAME, LAYERINFO_FILENAME,
	LIB_FILENAME, METAINFO_FILENAME, UfoFile, UfoFileStructure, UfoReader, UfoWriter,
	UfoWriterOptions, WriterGlyphSetOptions, groups_to_value, kerning_nested_to_value,
	make_ufo_path,
};
pub use utils::{FormatVersionInput, GlifFormatVersion, Number, UfoFormatVersion, value_repr};

/// Groups: group names to glyph names (`groups.plist`).
pub type Groups = IndexMap<String, Vec<String>>;

/// Kerning pairs to values (`dict[tuple[str, str], int | float]`).
pub type Kerning = IndexMap<(String, String), Value>;

/// Kerning as stored in `kerning.plist`: first to second to value.
pub type KerningNested = IndexMap<String, IndexMap<String, Value>>;

#[cfg(test)]
mod tests {
	fn send_sync<T: Send + Sync>() {}
	fn send<T: Send>() {}

	#[test]
	fn thread_safety() {
		send_sync::<crate::GlyphSet>();
		send_sync::<crate::Glyph>();
		send::<crate::UfoReader>();
		send::<crate::UfoWriter>();
	}
}
