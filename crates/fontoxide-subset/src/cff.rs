//! Subsetting of `CFF ` and `CFF2` (`fontTools.subset.cff`).

use std::collections::{BTreeMap, HashSet};

use fontoxide_ttlib::cff::font::{CffFontSet, Encoding, PrivateRef};
use fontoxide_ttlib::tables::{Cff, Cff2};
use fontoxide_ttlib::{Font, Tag};

use crate::{GlyphSet, Result, Subsetter};

fn font_set(font: &mut Font, tag: Tag) -> Result<&mut CffFontSet> {
	Ok(if tag == *b"CFF2" {
		&mut font.require::<Cff2>()?.cff
	} else {
		&mut font.require::<Cff>()?.cff
	})
}

fn key_error(name: &str) -> fontoxide_ttlib::Error {
	fontoxide_ttlib::Error::ttlib(format!("KeyError: {name:?}"))
}

/// `closure_glyphs` of `CFF `: adds the glyphs that `seac` uses, recursively.
pub(crate) fn closure_glyphs(s: &mut Subsetter, font: &mut Font) -> Result<()> {
	let order = s.orig_glyph_order.clone();
	let cff = font_set(font, Tag::new(b"CFF "))?;
	let mut decompose = s.glyphs.clone();
	while !decompose.is_empty() {
		let mut components = GlyphSet::new();
		for &g in &decompose {
			let Some(gid) = cff.glyph_index_hint(&order.name(g), g as usize) else {
				continue;
			};
			for name in cff.seac_components(gid)? {
				// Glyphs that the font does not have are dropped after the
				// closure.
				if let Some(c) = order.id(name) {
					components.insert(c);
				}
			}
		}
		components.retain(|g| !s.glyphs.contains(g));
		s.glyphs.extend(&components);
		decompose = components;
	}
	Ok(())
}

/// `prune_pre_subset`: empties `.notdef` unless its outline is kept, and clears
/// the encoding.
pub(crate) fn prune_pre_subset(s: &mut Subsetter, font: &mut Font, tag: Tag) -> Result<bool> {
	let cff = font_set(font, tag)?;
	// CFF table must have one font only
	cff.font_names.truncate(1);
	cff.top_dicts.truncate(1);

	if s.options.notdef_glyph && !s.options.notdef_outline {
		for i in 0..cff.top_dicts.len() {
			let gid = cff.top_dicts[i]
				.char_strings
				.as_ref()
				.and_then(|c| c.index(".notdef"))
				.ok_or_else(|| key_error(".notdef"))?;
			cff.empty_char_string(i, gid, false)?;
		}
	}

	// Clear useless Encoding
	for top in &mut cff.top_dicts {
		// https://github.com/fonttools/fonttools/issues/620
		top.encoding = Some(Encoding::Standard);
	}
	Ok(true)
}

/// `subset_glyphs`.
pub(crate) fn subset_glyphs(s: &mut Subsetter, font: &mut Font, tag: Tag) -> Result<bool> {
	let order = s.orig_glyph_order.clone();
	let kept: GlyphSet = s.glyphs.union(&s.glyphs_emptied).copied().collect();
	let names: HashSet<String> = kept.iter().map(|&g| order.name(g).into_owned()).collect();
	let emptied: Vec<String> = s
		.glyphs_emptied
		.iter()
		.map(|&g| order.name(g).into_owned())
		.collect();
	let retain_gids = s.options.retain_gids;
	let cff = font_set(font, tag)?;
	for i in 0..cff.top_dicts.len() {
		let top = &mut cff.top_dicts[i];
		let charset = top.charset.clone().unwrap_or_default();
		if let Some(cs) = &mut top.char_strings {
			// Load all glyphs
			for g in &charset {
				if names.contains(g) && cs.index(g).is_none() {
					return Err(key_error(g).into());
				}
			}
			let keep: Vec<bool> = cs.names.iter().map(|n| names.contains(n)).collect();
			let mut k = keep.iter();
			cs.names.retain(|_| *k.next().unwrap_or(&false));
			let mut k = keep.iter();
			cs.items.retain(|_| *k.next().unwrap_or(&false));
			let mut k = keep.iter();
			cs.privates.retain(|_| *k.next().unwrap_or(&false));
		}
		if let Some(sel) = &mut top.fd_select {
			sel.format = None;
			// FDSelect may also map VARC-only glyphs beyond the CFF2 charset.
			let len = sel.gid_array.len();
			sel.gid_array = kept
				.iter()
				.map(|&g| g as usize)
				.filter(|&g| g < len)
				.map(|g| sel.gid_array[g])
				.collect();
			if let Some(cs) = &mut top.char_strings
				&& cs.fd_select.is_some()
			{
				cs.fd_select = Some(sel.gid_array.clone());
			}
		}
		let charset: Vec<String> = charset.into_iter().filter(|g| names.contains(g)).collect();
		top.num_glyphs = Some(charset.len());
		top.charset = Some(charset);

		if retain_gids {
			for g in &emptied {
				let Some(gid) = cff.top_dicts[i]
					.char_strings
					.as_ref()
					.and_then(|c| c.index(g))
				else {
					continue;
				};
				cff.empty_char_string(i, gid, true)?;
			}
		}
	}
	Ok(true)
}

/// `prune_post_subset`: drops unused font DICTs, then subroutines and hints as
/// asked for.
pub(crate) fn prune_post_subset(s: &mut Subsetter, font: &mut Font, tag: Tag) -> Result<bool> {
	let cff = font_set(font, tag)?;
	for top in &mut cff.top_dicts {
		// Drop unused FontDictionaries
		let Some(sel) = &mut top.fd_select else {
			continue;
		};
		let indices: Vec<Option<i64>> = {
			let mut v = sel.gid_array.clone();
			v.sort();
			v.dedup();
			v
		};
		let position = |ss: &Option<i64>| indices.iter().position(|i| i == ss).unwrap_or(0) as i64;
		sel.gid_array = sel.gid_array.iter().map(|ss| Some(position(ss))).collect();
		let arr = top.fd_array.as_mut().ok_or_else(|| {
			fontoxide_ttlib::Error::ttlib(
				"AttributeError: 'TopDict' object has no attribute 'FDArray'",
			)
		})?;
		let mut remap = BTreeMap::new();
		let mut items = Vec::with_capacity(indices.len());
		for (new, old) in indices.iter().enumerate() {
			let old = old.ok_or_else(|| {
				fontoxide_ttlib::Error::ttlib(
					"TypeError: list indices must be integers or slices, not NoneType",
				)
			})?;
			let len = arr.len() as i64;
			let at = if old < 0 { old + len } else { old };
			let at = usize::try_from(at)
				.ok()
				.filter(|&a| a < arr.len())
				.ok_or_else(|| {
					fontoxide_ttlib::Error::ttlib("IndexError: list index out of range")
				})?;
			items.push(arr[at].clone());
			remap.insert(at, new);
		}
		*arr = items;
		if let Some(cs) = &mut top.char_strings {
			for p in &mut cs.privates {
				if let PrivateRef::Fd(i) = p {
					*p = remap
						.get(i)
						.map_or(PrivateRef::None, |&n| PrivateRef::Fd(n));
				}
			}
			if cs.fd_select.is_some() {
				cs.fd_select = Some(sel.gid_array.clone());
			}
		}
	}

	// Desubroutinize if asked for
	if s.options.desubroutinize {
		cff.desubroutinize()?;
	}

	// Drop hints if not needed
	if !s.options.hinting {
		cff.remove_hints()?;
	} else if !s.options.desubroutinize {
		cff.remove_unused_subroutines()?;
	}
	Ok(true)
}
