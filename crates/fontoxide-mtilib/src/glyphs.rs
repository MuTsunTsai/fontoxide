//! Glyph names to glyph IDs: the font's glyph order (`font.getGlyphID` and
//! `font.getReverseGlyphMap()`), or a glyph order that grows as glyphs are
//! looked up (fontTools' `MockFont`).

use std::collections::HashMap;

use fontoxide_otllib::GlyphMap;
use fontoxide_ttlib::Font;

use crate::Result;

/// A glyph order that grows as glyphs are looked up (`MockFont`).
#[derive(Debug)]
struct Mock {
	names: Vec<String>,
	ids: HashMap<String, u32>,
	changed: bool,
}

/// Glyph IDs for the parser.
#[derive(Debug)]
pub(crate) struct Glyphs<'a> {
	pub font: &'a mut Font,
	mock: Option<Mock>,
	reverse: Option<GlyphMap>,
}

impl<'a> Glyphs<'a> {
	/// The glyphs of `font`; with `mock`, glyphs not in its glyph order are
	/// appended to it (a font without a glyph order starts with `.notdef`).
	pub fn new(font: &'a mut Font, mock: bool) -> Result<Self> {
		let mock = if mock {
			let names: Vec<String> = if font.has_glyph_order() {
				font.glyph_order()?.names().to_vec()
			} else {
				vec![".notdef".to_owned()]
			};
			let mut ids = HashMap::new();
			for (i, n) in names.iter().enumerate() {
				ids.insert(n.clone(), i as u32);
			}
			Some(Mock {
				changed: !font.has_glyph_order(),
				names,
				ids,
			})
		} else {
			None
		};
		Ok(Glyphs {
			font,
			mock,
			reverse: None,
		})
	}

	/// Whether glyphs are added to the glyph order as they are looked up.
	pub fn is_growing(&self) -> bool {
		self.mock.is_some()
	}

	/// `font.getGlyphID(name)`.
	pub fn id(&mut self, name: &str) -> Result<u32> {
		match &mut self.mock {
			Some(mock) => {
				if let Some(&id) = mock.ids.get(name) {
					return Ok(id);
				}
				let id = mock.ids.len() as u32;
				mock.names.push(name.to_owned());
				mock.ids.insert(name.to_owned(), id);
				mock.changed = true;
				Ok(id)
			}
			None => Ok(self.font.glyph_id(name)?),
		}
	}

	/// `font.getReverseGlyphMap()`, for the otlLib builders that fontTools
	/// passes it to. `names` are the glyphs the builder will look up, which a
	/// growing glyph order adds in that order.
	pub fn reverse_map<'n>(
		&mut self,
		names: impl IntoIterator<Item = &'n str>,
	) -> Result<GlyphMap> {
		if self.mock.is_some() {
			for n in names {
				self.id(n)?;
			}
			let ids = self
				.mock
				.as_ref()
				.map(|m| m.ids.clone())
				.unwrap_or_default();
			return Ok(GlyphMap::new(ids));
		}
		if let Some(map) = &self.reverse {
			return Ok(map.clone());
		}
		let map = GlyphMap::from_font(self.font)?;
		self.reverse = Some(map.clone());
		Ok(map)
	}

	/// A glyph map for the otlLib builders that fontTools calls without one:
	/// the tables they build keep glyph names, which `font.getGlyphID` turns
	/// into IDs when compiling.
	pub fn id_map<'n>(&mut self, names: impl IntoIterator<Item = &'n str>) -> Result<GlyphMap> {
		let mut ids = HashMap::new();
		for n in names {
			let id = self.id(n)?;
			ids.insert(n.to_owned(), id);
		}
		let len = match &self.mock {
			Some(mock) => mock.names.len(),
			None => self.font.glyph_order()?.len(),
		};
		Ok(GlyphMap::new(ids).with_len(len))
	}

	/// Gives the font the glyphs added to a growing glyph order.
	pub fn finish(self) {
		if let Some(mock) = self.mock
			&& mock.changed
		{
			self.font.set_glyph_order(mock.names);
		}
	}
}
