//! Port of `fontTools.ufoLib.kerning`: looking up kerning values with
//! kerning groups.

use indexmap::IndexMap;

use crate::errors::Result;
use crate::{Groups, Kerning};
use fontoxide_misc::plistlib::Value;

/// A map from glyph names to the kerning group they belong to (`StrDict`).
pub type GlyphToGroup = IndexMap<String, String>;

/// `lookupKerningValue`: the kerning value between two glyphs or kerning
/// groups, looking up the groups the glyphs belong to, or `fallback`.
///
/// `glyph_to_groups` are the two maps from glyph names to first- and
/// second-side groups ([`glyphs_to_groups`]); they are built when `None`.
/// (Passing only one of them, a `ValueError` in fontTools, cannot be
/// expressed.)
///
/// ```
/// use fontoxide_ufolib::kerning::lookup_kerning_value;
/// use fontoxide_ufolib::{Groups, Kerning};
/// use fontoxide_misc::plistlib::Value;
///
/// let mut groups = Groups::new();
/// groups.insert("public.kern1.O".into(), vec!["O".into(), "D".into(), "Q".into()]);
/// groups.insert("public.kern2.E".into(), vec!["E".into(), "F".into()]);
/// let mut kerning = Kerning::new();
/// kerning.insert(("public.kern1.O".into(), "public.kern2.E".into()), Value::Integer(-100));
/// kerning.insert(("public.kern1.O".into(), "F".into()), Value::Integer(-200));
/// kerning.insert(("D".into(), "F".into()), Value::Integer(-300));
/// let k = |a: &str, b: &str| lookup_kerning_value((a, b), &kerning, &groups, Value::Integer(0), None).unwrap();
/// assert_eq!(k("D", "F"), Value::Integer(-300));
/// assert_eq!(k("O", "F"), Value::Integer(-200));
/// assert_eq!(k("O", "E"), Value::Integer(-100));
/// assert_eq!(k("O", "O"), Value::Integer(0));
/// ```
pub fn lookup_kerning_value(
	pair: (&str, &str),
	kerning: &Kerning,
	groups: &Groups,
	fallback: Value,
	glyph_to_groups: Option<(&GlyphToGroup, &GlyphToGroup)>,
) -> Result<Value> {
	let lookup = |a: &str, b: &str| kerning.get(&(a.to_owned(), b.to_owned())).cloned();
	// quickly check to see if the pair is in the kerning dictionary
	if let Some(v) = lookup(pair.0, pair.1) {
		return Ok(v);
	}
	let built;
	let (glyph_to_first_group, glyph_to_second_group) = match glyph_to_groups {
		Some(maps) => maps,
		None => {
			built = glyphs_to_groups(groups);
			(&built.0, &built.1)
		}
	};
	let (first, second) = pair;
	let (first_glyph, first_group) = if first.starts_with("public.kern1.") {
		(None, Some(first))
	} else {
		(
			Some(first),
			glyph_to_first_group.get(first).map(String::as_str),
		)
	};
	let (second_glyph, second_group) = if second.starts_with("public.kern2.") {
		(None, Some(second))
	} else {
		(
			Some(second),
			glyph_to_second_group.get(second).map(String::as_str),
		)
	};
	// make an ordered list of pairs to look up
	for a in [first_glyph, first_group].into_iter().flatten() {
		for b in [second_glyph, second_group].into_iter().flatten() {
			if let Some(v) = lookup(a, b) {
				return Ok(v);
			}
		}
	}
	Ok(fallback)
}

/// `glyphsToGroups`: maps glyph names to the first- and second-side kerning
/// groups they belong to. If a glyph is in more than one group of a side, the
/// last group wins.
pub fn glyphs_to_groups(groups: &Groups) -> (GlyphToGroup, GlyphToGroup) {
	let mut first = GlyphToGroup::new();
	let mut second = GlyphToGroup::new();
	for (group, members) in groups {
		if group.starts_with("public.kern1.") {
			for glyph in members {
				first.insert(glyph.clone(), group.clone());
			}
		} else if group.starts_with("public.kern2.") {
			for glyph in members {
				second.insert(glyph.clone(), group.clone());
			}
		}
	}
	(first, second)
}
