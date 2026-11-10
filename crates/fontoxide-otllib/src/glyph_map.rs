//! Glyph name to glyph ID maps (`font.getReverseGlyphMap()`), which the builders
//! take to sort glyphs and to produce the glyph IDs of the tables.

use std::collections::HashMap;
use std::sync::Arc;

use fontoxide_ttlib::Font;

use crate::{Error, Result};

/// A glyph name to glyph ID map. Cloning it is cheap.
#[derive(Debug, Clone)]
pub struct GlyphMap {
	ids: Arc<HashMap<String, u32>>,
	len: usize,
}

impl GlyphMap {
	/// A map from names to IDs.
	pub fn new(ids: HashMap<String, u32>) -> Self {
		let len = ids.len();
		GlyphMap {
			ids: Arc::new(ids),
			len,
		}
	}

	/// A map where each name has its index as the ID, like a glyph order.
	pub fn from_names<S: AsRef<str>>(names: &[S]) -> Self {
		Self::new(
			names
				.iter()
				.enumerate()
				.map(|(i, n)| (n.as_ref().to_owned(), i as u32))
				.collect(),
		)
	}

	/// The map of a font's glyph order (`font.getReverseGlyphMap()`).
	pub fn from_font(font: &mut Font) -> Result<Self> {
		let order = font.glyph_order()?;
		let ids = order.reverse_map().clone();
		Ok(GlyphMap {
			len: order.len().max(ids.len()),
			ids: Arc::new(ids),
		})
	}

	/// The same map, but reporting `len` glyphs. Some builders choose the
	/// formats for fonts with more than 65536 glyphs from the number of glyphs;
	/// this lets them be tested without that many glyphs (like the
	/// `ExtendedGlyphMap` of fontTools' tests).
	pub fn with_len(mut self, len: usize) -> Self {
		self.len = len;
		self
	}

	/// The ID of a glyph, if it is in the map.
	pub fn get(&self, name: &str) -> Option<u32> {
		self.ids.get(name).copied()
	}

	/// The ID of a glyph, failing like a Python `KeyError` if it is not in the map.
	pub fn id(&self, name: &str) -> Result<u32> {
		self.get(name)
			.ok_or_else(|| Error::Value(format!("KeyError: {}", py_repr(name))))
	}

	/// The number of glyphs (`len(glyphMap)`).
	pub fn len(&self) -> usize {
		self.len
	}

	/// Whether the map is empty.
	pub fn is_empty(&self) -> bool {
		self.len == 0
	}

	/// Glyph IDs to names (the inverse of the map).
	pub fn names_by_id(&self) -> HashMap<u32, String> {
		self.ids.iter().map(|(n, &id)| (id, n.clone())).collect()
	}

	/// Whether the font has glyph IDs above 65535, which need the extended
	/// formats of the beyond-64k specification (`_glyphMapHasExtendedGlyphIDs`).
	pub fn has_extended_glyph_ids(&self) -> bool {
		self.len > 0x10000
	}
}

/// `repr(str)`, for error messages.
pub(crate) fn py_repr(s: &str) -> String {
	fontoxide_misc::py::str_repr(s)
}
