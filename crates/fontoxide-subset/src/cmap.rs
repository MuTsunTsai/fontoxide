//! Subsetting of `cmap` (and the glyph closure over it).

use std::collections::{BTreeMap, BTreeSet};

use fontoxide_ttlib::tables::Cmap;
use fontoxide_ttlib::{Font, Tag};

use crate::{Error, Result, Subsetter};

/// `_cmap_add_bidi_mirroring`: adds the mirrored characters of the requested
/// ones.
fn add_bidi_mirroring(s: &mut Subsetter) {
	if s.options.bidi_closure {
		let additional: Vec<u32> = s
			.unicodes_requested
			.iter()
			.filter_map(|&u| fontoxide_unicodedata::mirrored(u))
			.collect();
		s.unicodes_requested.extend(additional);
	}
}

/// `_cmap_closure_glyphs` for `cmap`: adds the glyphs of the requested
/// characters, and returns the characters that have one.
fn cmap_closure_glyphs(s: &mut Subsetter, cmap: &Cmap) -> BTreeSet<u32> {
	let mut matched = BTreeSet::new();
	for table in cmap.tables.iter().filter(|t| t.is_unicode()) {
		if matches!(table.format, 14 | 15) {
			let Some(uvs) = &table.uvs_dict else {
				continue;
			};
			for (vs, records) in uvs {
				if !s.unicodes_requested.contains(vs) {
					continue;
				}
				// A `None` glyph is a default variation: no glyph to add.
				s.glyphs.extend(
					records
						.iter()
						.filter(|(u, _)| s.unicodes_requested.contains(u))
						.filter_map(|(_, g)| *g),
				);
			}
		} else {
			for (&u, &g) in table.cmap.iter() {
				if s.unicodes_requested.contains(&u) {
					matched.insert(u);
					s.glyphs.insert(g);
				}
			}
		}
	}
	matched
}

/// `_closure_glyphs_cmap_and_dmap`.
pub(crate) fn closure_glyphs_cmap_and_dmap(s: &mut Subsetter, font: &mut Font) -> Result<()> {
	add_bidi_mirroring(s);
	if font.contains(Tag::new(b"DMAP")) {
		return Err(Error::Unsupported("DMAP".into()));
	}
	let mut cmap_unicodes = BTreeSet::new();
	if let Some(cmap) = font.get::<Cmap>()? {
		cmap_unicodes = cmap_closure_glyphs(s, cmap);
	}
	s.unicodes_dmaped = BTreeSet::new();
	s.uvs_dmaped = BTreeSet::new();
	s.unicodes_missing = s
		.unicodes_requested
		.difference(&cmap_unicodes)
		.copied()
		.collect();
	Ok(())
}

/// `cmap.prune_pre_subset`: drops the non-Unicode (unless `legacy_cmap`),
/// Symbol (unless `symbol_cmap`) and format 0 subtables.
pub(crate) fn prune_pre_subset(s: &mut Subsetter, font: &mut Font) -> Result<bool> {
	let cmap = font.require::<Cmap>()?;
	if !s.options.legacy_cmap {
		cmap.tables.retain(|t| t.is_unicode() || t.is_symbol());
	}
	if !s.options.symbol_cmap {
		cmap.tables.retain(|t| !t.is_symbol());
	}
	cmap.tables.retain(|t| t.format != 0);
	Ok(true)
}

/// `cmap.subset_glyphs`: keeps the mappings of the requested characters and
/// glyphs.
pub(crate) fn subset_glyphs(s: &mut Subsetter, font: &mut Font) -> Result<bool> {
	let cmap = font.require::<Cmap>()?;
	let mut format12_bmp = Vec::new();
	// The Unicode BMP-only subtables of each language, for platforms 0 and 3.
	let mut plat0_enc3 = BTreeMap::new();
	let mut plat3_enc1 = BTreeMap::new();
	for (i, t) in cmap.tables.iter_mut().enumerate() {
		if t.platform_id == 0 && t.plat_enc_id == 3 {
			plat0_enc3.insert(t.language, i);
		}
		if t.platform_id == 3 && t.plat_enc_id == 1 {
			plat3_enc1.insert(t.language, i);
		}
		if matches!(t.format, 14 | 15) {
			// Default variations of requested glyphs are all dropped: the caller
			// must make sure those are included.
			if let Some(uvs) = &mut t.uvs_dict {
				uvs.retain(|v, _| s.unicodes_requested.contains(v));
				for records in uvs.values_mut() {
					records.retain(|&(u, g)| {
						g.is_some_and(|g| s.glyphs_requested.contains(&g))
							|| s.unicodes_requested.contains(&u)
					});
				}
				uvs.retain(|_, l| !l.is_empty());
			}
		} else if t.is_unicode() {
			t.cmap_mut()
				.retain(|u, g| s.glyphs_requested.contains(g) || s.unicodes_requested.contains(u));
			// Format 12 subtables with only BMP characters.
			if t.format == 12 && t.cmap.last_key_value().is_some_and(|(&u, _)| u < 0x10000) {
				format12_bmp.push(i);
			}
		} else {
			t.cmap_mut().retain(|_, g| s.glyphs_requested.contains(g));
		}
	}
	// A format 12 subtable is redundant if its BMP-only sibling has the same
	// mappings.
	for i in format12_bmp {
		let t = &cmap.tables[i];
		let sibling = if t.platform_id == 0 && t.plat_enc_id == 4 {
			plat0_enc3.get(&t.language)
		} else if t.platform_id == 3 && t.plat_enc_id == 10 {
			plat3_enc1.get(&t.language)
		} else {
			None
		};
		if let Some(&j) = sibling
			&& cmap.tables[j].cmap == t.cmap
		{
			cmap.tables[i].cmap_mut().clear();
		}
	}
	cmap.tables.retain(|t| {
		if matches!(t.format, 14 | 15) {
			t.uvs_dict.as_ref().is_some_and(|u| !u.is_empty())
		} else {
			!t.cmap.is_empty()
		}
	});
	Ok(true)
}
