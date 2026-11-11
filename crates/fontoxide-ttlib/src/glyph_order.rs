//! The glyph order: glyph names by glyph ID (`TTFont.getGlyphOrder` and friends).

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Glyph names in glyph ID order, with a lazily built reverse map.
#[derive(Debug, Default)]
pub struct GlyphOrder {
	names: Vec<String>,
	reverse: OnceLock<HashMap<String, u32>>,
}

impl Clone for GlyphOrder {
	fn clone(&self) -> Self {
		GlyphOrder::new(self.names.clone())
	}
}

impl PartialEq for GlyphOrder {
	fn eq(&self, other: &Self) -> bool {
		self.names == other.names
	}
}

impl GlyphOrder {
	/// A glyph order from names.
	pub fn new(names: Vec<String>) -> Self {
		GlyphOrder {
			names,
			reverse: OnceLock::new(),
		}
	}

	/// The names, by glyph ID.
	pub fn names(&self) -> &[String] {
		&self.names
	}

	/// The number of glyphs.
	pub fn len(&self) -> usize {
		self.names.len()
	}

	/// Whether there are no glyphs.
	pub fn is_empty(&self) -> bool {
		self.names.is_empty()
	}

	/// The name of a glyph; IDs past the end get a synthesized `glyphXXXXX` name
	/// (`getGlyphName`).
	pub fn name(&self, gid: u32) -> Cow<'_, str> {
		match self.names.get(gid as usize) {
			Some(n) => Cow::Borrowed(n),
			None => Cow::Owned(synthesized_name(gid)),
		}
	}

	/// The glyph ID for a name; `glyphXXXXX` names that are not in the glyph order
	/// are parsed (`getGlyphID`).
	pub fn id(&self, name: &str) -> Option<u32> {
		let reverse = self.reverse.get_or_init(|| {
			let mut m = HashMap::with_capacity(self.names.len());
			for (i, n) in self.names.iter().enumerate() {
				// Later duplicates win, as in `_buildReverseGlyphOrderDict`.
				m.insert(n.clone(), i as u32);
			}
			m
		});
		if let Some(&id) = reverse.get(name) {
			return Some(id);
		}
		let digits = name.strip_prefix("glyph")?;
		fontoxide_misc::py::parse_int(digits).and_then(|n| u32::try_from(n).ok())
	}

	/// The reverse map, name to glyph ID (`getReverseGlyphMap`).
	pub fn reverse_map(&self) -> &HashMap<String, u32> {
		let _ = self.id("");
		self.reverse.get().expect("initialized by id()")
	}
}

/// `"glyph%.5d" % gid`.
pub fn synthesized_name(gid: u32) -> String {
	format!("glyph{gid:05}")
}

/// A glyph name for a code point (`TTFont._makeGlyphName`): its AGL name if it has
/// one, otherwise `uniXXXX` or `uXXXXX`.
pub fn make_glyph_name(codepoint: u32) -> String {
	if let Some(name) = fontoxide_misc::agl::uv2agl(codepoint) {
		name.to_owned()
	} else if codepoint <= 0xFFFF {
		format!("uni{codepoint:04X}")
	} else {
		format!("u{codepoint:X}")
	}
}
