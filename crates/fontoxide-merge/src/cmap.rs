//! Port of `fontTools.merge.cmap`: the glyph order and the character map of the
//! merged font.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use fontoxide_ttlib::tables::cmap::{CharMap, DEFAULT_CMAP_PREFERENCES, UvsDict};
use fontoxide_ttlib::tables::{Cff, Cmap, CmapSubtable, Dmap};
use fontoxide_ttlib::{Font, Tag};
use indexmap::IndexMap;

use crate::unicode::is_default_ignorable;
use crate::{Error, Merger, Result};

/// Makes the glyph names of the fonts unique, renaming duplicates `name.1`,
/// `name.2` and so on, in place, and returns the glyph order of the merged font:
/// all the glyphs of the fonts, in order (`computeMegaGlyphOrder`, which stores
/// it as `merger.glyphOrder`).
pub fn compute_mega_glyph_order(glyph_orders: &mut [Vec<String>]) -> Vec<String> {
	// Name to the next suffix to try; the names in order.
	let mut mega_order: HashMap<String, u64> = HashMap::new();
	let mut names = Vec::new();
	for glyph_order in glyph_orders.iter_mut() {
		for glyph_name in glyph_order.iter_mut() {
			if let Some(&n) = mega_order.get(glyph_name.as_str()) {
				let mut n = n;
				while mega_order.contains_key(&format!("{glyph_name}.{n}")) {
					n += 1;
				}
				mega_order.insert(glyph_name.clone(), n);
				*glyph_name = format!("{glyph_name}.{n}");
			}
			mega_order.insert(glyph_name.clone(), 1);
			names.push(glyph_name.clone());
		}
	}
	names
}

/// The (format, platform ID, encoding ID) of the subtables with Unicode BMP-only
/// semantics (`_CmapUnicodePlatEncodings.BMP`).
const BMP: [(u16, u16, u16); 4] = [(4, 3, 1), (4, 0, 3), (4, 0, 4), (4, 0, 6)];
/// … with Unicode full repertoire semantics (`FullRepertoire`).
const FULL_REPERTOIRE: [(u16, u16, u16); 3] = [(12, 3, 10), (12, 0, 4), (12, 0, 6)];
/// … of Unicode variation sequences (`UVS`).
const UVS: [(u16, u16, u16); 2] = [(14, 0, 5), (15, 0, 5)];

fn properties(subtable: &CmapSubtable) -> (u16, u16, u16) {
	(subtable.format, subtable.platform_id, subtable.plat_enc_id)
}

/// The variation sequence subtable of a `cmap`: the format 15 one if there is
/// one, else the last format 14 one (`_getUvsTable`).
fn get_uvs_table(cmap: Option<&Cmap>) -> Option<&CmapSubtable> {
	let mut format14 = None;
	for subtable in cmap.into_iter().flat_map(|c| &c.tables) {
		if !UVS.contains(&properties(subtable)) {
			continue;
		}
		if subtable.format == 15 {
			return Some(subtable);
		}
		format14 = Some(subtable);
	}
	format14
}

/// Folds the `DMAP` overrides into a new `cmap`, and removes `DMAP`
/// (`_flattenDmap`).
pub fn flatten_dmap(font: &mut Font) -> Result<()> {
	let mut nominal = CmapSubtable::new(12);
	nominal.platform_id = 3;
	nominal.plat_enc_id = 10;
	nominal.language = Some(0);
	// LENIENT(L37): fontTools iterates over this mapping in the order of `{**cmap,
	// **dmap}` when it resolves duplicate code points; here it is always in code
	// point order (see `compute_mega_cmap`).
	nominal.cmap = Arc::new(
		font.get_best_cmap(&DEFAULT_CMAP_PREFERENCES)?
			.unwrap_or_default(),
	);
	let mut tables = vec![nominal];

	// Glyph zero is a miss of DMAP; None is a default variation sequence.
	let glyph_zero = Some(0);
	let mut uvs_dict: IndexMap<u32, IndexMap<u32, Option<u32>>> = IndexMap::new();
	for is_dmap in [false, true] {
		let table = if is_dmap {
			font.get::<Dmap>()?.map(|d| &d.0)
		} else {
			font.get::<Cmap>()?.map(|c| &*c)
		};
		let Some(uvs_table) = get_uvs_table(table) else {
			continue;
		};
		for (&selector, mappings) in uvs_table.uvs_dict.iter().flatten() {
			let target = uvs_dict.entry(selector).or_default();
			for &(codepoint, glyph) in mappings {
				if !is_dmap || glyph != glyph_zero {
					target.insert(codepoint, glyph);
				}
			}
		}
	}
	if !uvs_dict.is_empty() {
		let mut uvs = CmapSubtable::new(14);
		uvs.platform_id = 0;
		uvs.plat_enc_id = 5;
		uvs.language = Some(0);
		uvs.uvs_dict = Some(
			uvs_dict
				.into_iter()
				.map(|(selector, mappings)| (selector, mappings.into_iter().collect()))
				.collect(),
		);
		tables.insert(0, uvs);
	}
	font.insert(Box::new(Cmap {
		table_version: 0,
		tables,
	}));
	font.remove(Tag::new(b"DMAP"));
	Ok(())
}

/// Renames the glyphs of a `CFF ` table after `glyph_order`
/// (`renameCFFCharStrings`).
pub fn rename_cff_char_strings(glyph_order: &[String], cff: &mut Cff) -> Result<()> {
	let top = cff
		.cff
		.top_dicts
		.first_mut()
		.ok_or_else(|| Error::Type("IndexError: list index out of range".into()))?;
	if let Some(char_strings) = &mut top.char_strings {
		for (i, name) in char_strings.names.iter_mut().enumerate() {
			*name = glyph_order
				.get(i)
				.cloned()
				.ok_or_else(|| Error::Type("IndexError: list index out of range".into()))?;
		}
	}
	top.charset = Some(glyph_order.to_vec());
	Ok(())
}

impl Merger {
	/// Computes the Unicode mapping of the merged font from the `cmap` tables of
	/// the fonts, and the glyphs to disambiguate with `locl`
	/// (`computeMegaCmap`, which sets `merger.cmap` and `merger.uvsDict`).
	///
	/// A code point mapped by several fonts goes to the first; the glyph of a
	/// later font is recorded in [`Merger::duplicate_glyphs_per_font`], unless
	/// the code point is default ignorable or U+25CC DOTTED CIRCLE.
	pub fn compute_mega_cmap(&mut self, cmap_tables: &[&Cmap]) -> Result<()> {
		// For nominal Unicode mappings, only merge formats 4 and 12. If there is
		// a format 12 table for a font, ignore the format 4 table of it.
		let mut chosen_cmap_tables = Vec::new();
		let mut chosen_uvs_tables = Vec::new();
		for (font_idx, table) in cmap_tables.iter().enumerate() {
			let mut format4 = None;
			let mut format12 = None;
			for subtable in &table.tables {
				let properties = properties(subtable);
				if BMP.contains(&properties) {
					format4 = Some(subtable);
				} else if FULL_REPERTOIRE.contains(&properties) {
					format12 = Some(subtable);
				} else if UVS.contains(&properties) {
					continue;
				} else {
					log::warn!(
						"Dropped cmap subtable from font '{font_idx}':\tformat {:>2}, platformID {:>2}, platEncID {:>2}",
						subtable.format,
						subtable.platform_id,
						subtable.plat_enc_id
					);
				}
			}
			if let Some(t) = format12.or(format4) {
				chosen_cmap_tables.push((t, font_idx));
			}
			if let Some(uvs_table) = get_uvs_table(Some(table)) {
				chosen_uvs_tables.push(uvs_table);
			}
		}

		// Build the Unicode mapping.
		let mut cmap = CharMap::new();
		for (table, font_idx) in chosen_cmap_tables {
			// Handle duplicates.
			for (&uni, &gid) in table.cmap.iter() {
				let Some(&oldgid) = cmap.get(&uni) else {
					cmap.insert(uni, gid);
					continue;
				};
				if is_default_ignorable(uni) || uni == 0x25CC {
					// U+25CC DOTTED CIRCLE
					continue;
				}
				if oldgid == gid {
					continue;
				}
				let dups = self
					.duplicate_glyphs_per_font
					.get_mut(font_idx)
					.ok_or_else(|| {
						Error::Type(
						"AttributeError: 'Merger' object has no attribute 'duplicateGlyphsPerFont'"
							.into(),
					)
					})?;
				match dups.get(&oldgid) {
					// Char previously mapped to oldgid, now to gid. Record, to fix
					// up in GSUB 'locl' later.
					None => {
						dups.insert(oldgid, gid);
					}
					// Char previously mapped to oldgid but oldgid is already
					// remapped to a different gid, because of another Unicode
					// character.
					Some(&other) if other != gid => {
						log::warn!(
							"Dropped mapping from codepoint 0X{uni:04X} to glyphId '{}'",
							self.glyph_name(gid)
						);
					}
					Some(_) => {}
				}
			}
		}
		self.cmap = Some(cmap);
		self.uvs_dict = self.compute_mega_uvs(&chosen_uvs_tables)?;
		Ok(())
	}

	/// The merged variation sequences (`computeMegaUvs`): the first font that
	/// maps a sequence wins, and sequences to the glyph of the base character
	/// become default ones.
	pub fn compute_mega_uvs(&self, uvs_tables: &[&CmapSubtable]) -> Result<UvsDict> {
		let empty = CharMap::new();
		let cmap = self.cmap.as_ref().unwrap_or(&empty);
		let mut uvs_dict: IndexMap<u32, IndexMap<u32, Option<u32>>> = IndexMap::new();
		for table in uvs_tables {
			for (&variation_selector, uvs_mapping) in table.uvs_dict.iter().flatten() {
				let merged = uvs_dict.entry(variation_selector).or_default();
				for &(unicode_value, glyph) in uvs_mapping {
					// A default variation.
					let glyph = if glyph.is_some() && cmap.get(&unicode_value).copied() == glyph {
						None
					} else {
						glyph
					};
					// Prefer the previous glyph ID if both fonts defined it.
					merged.entry(unicode_value).or_insert(glyph);
				}
			}
		}
		Ok(uvs_dict
			.into_iter()
			.map(|(selector, mappings)| (selector, mappings.into_iter().collect()))
			.collect())
	}

	/// The name of a glyph of the merged font, for messages.
	fn glyph_name(&self, gid: u32) -> String {
		self.glyph_order
			.as_ref()
			.and_then(|order| order.get(gid as usize).cloned())
			.unwrap_or_else(|| format!("glyph{gid:05}"))
	}

	/// The `cmap` table of the merged font (`table__c_m_a_p.merge`): a format 4
	/// subtable, a format 12 one if needed for code points or glyph IDs above
	/// 0xFFFF, and a format 14 one for variation sequences. The mapping is
	/// computed from `tables` if [`Merger::compute_mega_cmap`] was not called.
	pub fn merge_cmap(&mut self, tables: &[Option<&Cmap>]) -> Result<Cmap> {
		if self.cmap.is_none() {
			let tables = tables
				.iter()
				.map(|t| {
					t.ok_or_else(|| {
						Error::Type(
							"AttributeError: 'NotImplementedType' object has no attribute 'tables'"
								.into(),
						)
					})
				})
				.collect::<Result<Vec<_>>>()?;
			self.compute_mega_cmap(&tables)?;
		}
		let cmap = self.cmap.clone().unwrap_or_default();
		// Glyph IDs are those of the merged font already.
		let cmap_bmp_only: CharMap = cmap
			.iter()
			.filter(|&(&uni, &gid)| uni <= 0xFFFF && gid <= 0xFFFF)
			.map(|(&u, &g)| (u, g))
			.collect();
		let mut subtables = Vec::new();
		if cmap_bmp_only.len() != cmap.len() {
			// Format 12 is required for non-BMP code points or glyph IDs beyond 64k.
			let mut cmap_table = CmapSubtable::new(12);
			cmap_table.platform_id = 3;
			cmap_table.plat_enc_id = 10;
			cmap_table.language = Some(0);
			cmap_table.cmap = Arc::new(cmap);
			subtables.push(cmap_table);
		}
		// Always create format 4.
		let mut cmap_table = CmapSubtable::new(4);
		cmap_table.platform_id = 3;
		cmap_table.plat_enc_id = 1;
		cmap_table.language = Some(0);
		cmap_table.cmap = Arc::new(cmap_bmp_only);
		// Ordered by platform, then encoding.
		subtables.insert(0, cmap_table);

		if !self.uvs_dict.is_empty() {
			let mut uvs_table = CmapSubtable::new(14);
			uvs_table.platform_id = 0;
			uvs_table.plat_enc_id = 5;
			uvs_table.language = Some(0);
			uvs_table.uvs_dict = Some(self.uvs_dict.clone());
			subtables.insert(0, uvs_table);
		}
		Ok(Cmap {
			table_version: 0,
			tables: subtables,
		})
	}
}

/// Duplicate glyphs of a font: a glyph of an earlier font, mapped from a code
/// point that this font maps to another glyph, to that glyph.
pub type DuplicateGlyphs = BTreeMap<u32, u32>;

#[cfg(test)]
mod tests {
	use super::*;

	fn names(v: &[&str]) -> Vec<String> {
		v.iter().map(|s| (*s).to_owned()).collect()
	}

	#[test]
	fn mega_glyph_order() {
		let mut orders = vec![
			names(&[".notdef", "a", "a.1"]),
			names(&[".notdef", "a", "b"]),
			names(&[".notdef", "a.1", "a"]),
		];
		let mega = compute_mega_glyph_order(&mut orders);
		assert_eq!(orders[1], names(&[".notdef.1", "a.2", "b"]));
		assert_eq!(orders[2], names(&[".notdef.2", "a.1.1", "a.3"]));
		assert_eq!(mega.len(), 9);
		assert_eq!(&mega[3..6], &orders[1][..]);
	}
}
