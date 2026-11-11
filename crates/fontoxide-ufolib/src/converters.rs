//! Port of `fontTools.ufoLib.converters`: converting the kerning and kerning
//! groups of UFO 1 and UFO 2 fonts to UFO 3.

use indexmap::{IndexMap, IndexSet};

use crate::{Groups, KerningNested};

/// The group renaming done by the conversion: `{"side1": {old: new},
/// "side2": {old: new}}`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupRenameMaps {
	/// `side1`.
	pub side1: IndexMap<String, String>,
	/// `side2`.
	pub side2: IndexMap<String, String>,
}

/// `convertUFO1OrUFO2KerningToUFO3Kerning`: renames the kerning groups
/// referenced by the kerning (and the groups with the known `@MMK_L_` and
/// `@MMK_R_` prefixes) to `public.kern1.` and `public.kern2.` names.
///
/// Returns the new kerning, `groups` with copies of the renamed groups added
/// under their new names, and the renaming. Names for which `in_glyph_set`
/// is true are glyphs, not groups.
///
/// LENIENT(L35): fontTools collects the groups to rename in sets, so with clashing
/// new names the numbering depends on Python's string hashing; here the groups
/// are renamed in the order they are found (prefixed groups in the order of
/// `groups`, then the others in the order of the kerning).
pub fn convert_ufo1_or_ufo2_kerning_to_ufo3_kerning(
	kerning: &KerningNested,
	mut groups: Groups,
	in_glyph_set: &dyn Fn(&str) -> bool,
) -> (KerningNested, Groups, GroupRenameMaps) {
	// gather known kerning groups based on the prefixes
	let (mut first_referenced, mut second_referenced) = find_known_kerning_groups(&groups);
	// Make lists of groups referenced in kerning pairs.
	for (first, seconds) in kerning {
		if groups.contains_key(first) && !in_glyph_set(first) && !first.starts_with("public.kern1.")
		{
			first_referenced.insert(first.clone());
		}
		for second in seconds.keys() {
			if groups.contains_key(second)
				&& !in_glyph_set(second)
				&& !second.starts_with("public.kern2.")
			{
				second_referenced.insert(second.clone());
			}
		}
	}
	// Create new names for these groups.
	let rename = |referenced: &IndexSet<String>, old_prefix: &str, new_prefix: &str| {
		let mut renamed: IndexMap<String, String> = IndexMap::new();
		for name in referenced {
			let existing: Vec<&String> = groups.keys().chain(renamed.keys()).collect();
			let new_name = format!("{new_prefix}{}", name.replace(old_prefix, ""));
			let new_name = make_unique_group_name(&new_name, &existing);
			renamed.insert(name.clone(), new_name);
		}
		renamed
	};
	let first_renamed = rename(&first_referenced, "@MMK_L_", "public.kern1.");
	let second_renamed = rename(&second_referenced, "@MMK_R_", "public.kern2.");
	// Populate the new group names into the kerning dictionary as needed.
	let mut new_kerning = KerningNested::new();
	for (first, seconds) in kerning {
		let first = first_renamed.get(first).unwrap_or(first).clone();
		let mut new_seconds = IndexMap::new();
		for (second, value) in seconds {
			let second = second_renamed.get(second).unwrap_or(second).clone();
			new_seconds.insert(second, value.clone());
		}
		new_kerning.insert(first, new_seconds);
	}
	// Make copies of the referenced groups and store them under the new names
	// in the overall groups dictionary.
	for (old_name, new_name) in first_renamed.iter().chain(second_renamed.iter()) {
		let group = groups.get(old_name).cloned().unwrap_or_default();
		groups.insert(new_name.clone(), group);
	}
	(
		new_kerning,
		groups,
		GroupRenameMaps {
			side1: first_renamed,
			side2: second_renamed,
		},
	)
}

/// `findKnownKerningGroups`: the groups whose names start with the known
/// first-side (`@MMK_L_`) and second-side (`@MMK_R_`) prefixes.
pub fn find_known_kerning_groups<V>(
	groups: &IndexMap<String, V>,
) -> (IndexSet<String>, IndexSet<String>) {
	let mut first = IndexSet::new();
	let mut second = IndexSet::new();
	for name in groups.keys() {
		if name.starts_with("@MMK_L_") {
			first.insert(name.clone());
		}
		if name.starts_with("@MMK_R_") {
			second.insert(name.clone());
		}
	}
	(first, second)
}

/// `makeUniqueGroupName`: `name`, or `name` followed by the smallest counter
/// that makes it unique among `group_names`.
pub fn make_unique_group_name<S: AsRef<str>>(name: &str, group_names: &[S]) -> String {
	let mut counter = 0u64;
	loop {
		let new_name = if counter > 0 {
			format!("{name}{counter}")
		} else {
			name.to_owned()
		};
		if !group_names.iter().any(|n| n.as_ref() == new_name) {
			return new_name;
		}
		counter += 1;
	}
}
