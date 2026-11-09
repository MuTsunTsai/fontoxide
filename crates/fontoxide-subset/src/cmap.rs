//! Subsetting of `cmap` (and the glyph closure over it).

use std::collections::{BTreeMap, BTreeSet};

use fontoxide_ttlib::tables::{Cmap, Dmap};
use fontoxide_ttlib::{Font, Tag};

use crate::{Result, Subsetter};

const CMAP: Tag = Tag::new(b"cmap");
const DMAP: Tag = Tag::new(b"DMAP");

/// The `cmap` or `DMAP` table.
fn table(font: &mut Font, tag: Tag) -> Result<&mut Cmap> {
	if tag == CMAP {
		Ok(font.require::<Cmap>()?)
	} else {
		Ok(&mut font.require::<Dmap>()?.0)
	}
}

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

/// What `_cmap_closure_glyphs` returns: the characters that have a glyph, and
/// the characters and variation sequences covered by every Unicode subtable
/// (which are the ones that can suppress the fallback from `DMAP` to `cmap`).
#[derive(Default)]
struct Closure {
	matched: BTreeSet<u32>,
	covered_unicodes: BTreeSet<u32>,
	covered_uvs: BTreeSet<(u32, u32)>,
}

/// `_cmap_closure_glyphs`: adds the glyphs of the requested characters.
fn cmap_closure_glyphs(
	s: &mut Subsetter,
	cmap: &Cmap,
	is_dmap: bool,
	excluded_unicodes: &BTreeSet<u32>,
	excluded_uvs: &BTreeSet<(u32, u32)>,
) -> Closure {
	let mut matched = BTreeSet::new();
	let mut covered_unicodes: Option<BTreeSet<u32>> = None;
	let mut covered_uvs: Option<BTreeSet<(u32, u32)>> = None;
	for table in cmap.tables.iter().filter(|t| t.is_unicode()) {
		if matches!(table.format, 14 | 15) {
			let Some(uvs) = &table.uvs_dict else {
				continue;
			};
			let mut table_uvs = BTreeSet::new();
			for (&vs, records) in uvs {
				if !s.unicodes_requested.contains(&vs) {
					continue;
				}
				for &(u, g) in records {
					if !s.unicodes_requested.contains(&u) || excluded_uvs.contains(&(vs, u)) {
						continue;
					}
					// Glyph zero is a miss, so it must not suppress the `cmap`
					// fallback when this is `DMAP`. A `None` glyph is a default
					// variation: a match, with no glyph to add.
					if g != Some(0) {
						table_uvs.insert((vs, u));
					}
					s.glyphs.extend(g);
				}
			}
			covered_uvs = Some(match covered_uvs {
				None => table_uvs,
				Some(c) => c.intersection(&table_uvs).copied().collect(),
			});
		} else {
			let mut intersection = BTreeSet::new();
			for (&u, &g) in table.cmap.iter() {
				if s.unicodes_requested.contains(&u)
					&& !excluded_unicodes.contains(&u)
					// Glyph zero is a miss, not a replacement for the `cmap` fallback.
					&& !(is_dmap && g == 0)
				{
					intersection.insert(u);
					s.glyphs.insert(g);
				}
			}
			matched.extend(intersection.iter().copied());
			covered_unicodes = Some(match covered_unicodes {
				None => intersection,
				Some(c) => c.intersection(&intersection).copied().collect(),
			});
		}
	}
	Closure {
		matched,
		covered_unicodes: covered_unicodes.unwrap_or_default(),
		covered_uvs: covered_uvs.unwrap_or_default(),
	}
}

/// `_closure_glyphs_cmap_and_dmap`.
pub(crate) fn closure_glyphs_cmap_and_dmap(s: &mut Subsetter, font: &mut Font) -> Result<()> {
	add_bidi_mirroring(s);
	let none = (BTreeSet::new(), BTreeSet::new());
	let dmap = match font.get::<Dmap>()? {
		Some(dmap) => cmap_closure_glyphs(s, &dmap.0, true, &none.0, &none.1),
		None => Closure::default(),
	};
	let cmap = match font.get::<Cmap>()? {
		Some(cmap) => {
			cmap_closure_glyphs(s, cmap, false, &dmap.covered_unicodes, &dmap.covered_uvs)
		}
		None => Closure::default(),
	};
	let mut matched = dmap.matched;
	matched.extend(cmap.matched);
	s.unicodes_dmaped = dmap.covered_unicodes;
	s.uvs_dmaped = dmap.covered_uvs;
	s.unicodes_missing = s.unicodes_requested.difference(&matched).copied().collect();
	Ok(())
}

/// `cmap.prune_pre_subset`: drops the non-Unicode (unless `legacy_cmap`),
/// Symbol (unless `symbol_cmap`) and format 0 subtables.
pub(crate) fn prune_pre_subset(s: &mut Subsetter, font: &mut Font, tag: Tag) -> Result<bool> {
	let cmap = table(font, tag)?;
	if !s.options.legacy_cmap {
		cmap.tables.retain(|t| t.is_unicode() || t.is_symbol());
	}
	if !s.options.symbol_cmap {
		cmap.tables.retain(|t| !t.is_symbol());
	}
	cmap.tables.retain(|t| t.format != 0);
	Ok(tag == CMAP || !cmap.tables.is_empty())
}

/// `cmap.subset_glyphs`: keeps the mappings of the requested characters and
/// glyphs.
pub(crate) fn subset_glyphs(s: &mut Subsetter, font: &mut Font, tag: Tag) -> Result<bool> {
	let is_cmap = tag == CMAP;
	debug_assert!(is_cmap || tag == DMAP);
	// The mappings that `DMAP` provides are not kept in `cmap`.
	let none = (BTreeSet::new(), BTreeSet::new());
	let (excluded_unicodes, excluded_uvs) = if is_cmap {
		(&s.unicodes_dmaped, &s.uvs_dmaped)
	} else {
		(&none.0, &none.1)
	};
	let cmap = table(font, tag)?;
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
				for (v, records) in uvs.iter_mut() {
					records.retain(|&(u, g)| {
						g.is_some_and(|g| s.glyphs_requested.contains(&g))
							|| (s.unicodes_requested.contains(&u)
								&& !excluded_uvs.contains(&(*v, u)))
					});
				}
				uvs.retain(|_, l| !l.is_empty());
			}
		} else if t.is_unicode() {
			t.cmap_mut().retain(|u, g| {
				(is_cmap || *g != 0)
					&& (s.glyphs_requested.contains(g)
						|| (s.unicodes_requested.contains(u) && !excluded_unicodes.contains(u)))
			});
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
	for i in if is_cmap { format12_bmp } else { Vec::new() } {
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
	let is_nonempty = |t: &fontoxide_ttlib::tables::CmapSubtable| {
		if matches!(t.format, 14 | 15) {
			t.uvs_dict.as_ref().is_some_and(|u| !u.is_empty())
		} else {
			!t.cmap.is_empty()
		}
	};
	// Empty `DMAP` subtables are kept while any mappings remain, so that subsetting
	// does not promote another subtable and replace a `cmap` fallback.
	let keep_unicode = !is_cmap && cmap.tables.iter().any(is_nonempty);
	cmap.tables
		.retain(|t| is_nonempty(t) || (keep_unicode && t.is_unicode()));
	Ok(is_cmap || !cmap.tables.is_empty())
}
