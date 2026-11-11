//! Port of `fontTools.designspaceLib.statNames`: the names of a location in
//! user-space coordinates, computed from the STAT data.

use indexmap::IndexMap;

use crate::descriptors::{Axis, AxisLabelDescriptor, LocalizedNames, SimpleLocation};
use crate::document::DesignSpaceDocument;
use crate::{DesignSpaceDocumentError, Result};

/// `StatNames`: name data generated from the STAT table information.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatNames {
	/// `familyNames`.
	pub family_names: LocalizedNames,
	/// `styleNames`.
	pub style_names: LocalizedNames,
	/// `postScriptFontName`.
	pub post_script_font_name: Option<String>,
	/// `styleMapFamilyNames`.
	pub style_map_family_names: LocalizedNames,
	/// `styleMapStyleName`: `"regular"`, `"bold"`, `"italic"` or
	/// `"bold italic"`.
	pub style_map_style_name: Option<String>,
}

fn key_error(name: &str) -> DesignSpaceDocumentError {
	DesignSpaceDocumentError::Value(format!("KeyError: {}", fontoxide_misc::py::str_repr(name)))
}

/// `getStatNames`: the family, style, PostScript and style map names of a
/// user location, from the document's STAT information.
///
/// If not enough STAT data is available for a name, its dict of localized
/// names is empty (family and style names), or it is `None` (PostScript
/// name).
///
/// fontTools collects the languages of the style names in a `set`, so the
/// order of the localized names varies between runs; here it is the order in
/// which the languages first appear.
pub fn get_stat_names(
	doc: &DesignSpaceDocument,
	user_location: &SimpleLocation,
) -> Result<StatNames> {
	let mut family_names = LocalizedNames::new();
	match doc.compute_default()?.map(|i| &doc.sources[i]) {
		None => log::warn!("Cannot determine default source to look up family name."),
		Some(source) => match &source.family_name {
			None => log::warn!(
				"Cannot look up family name, assign the 'familyname' attribute to the default source."
			),
			Some(family_name) => {
				family_names.insert("en".to_owned(), family_name.clone());
				for (k, v) in &source.localised_family_name {
					family_names.insert(k.clone(), v.clone());
				}
			}
		},
	}

	let mut style_names = LocalizedNames::new();
	// If a free-standing label matches the location, use it for name generation.
	if let Some(label) = doc.label_for_user_location(user_location) {
		style_names.insert("en".to_owned(), label.name.clone());
		for (k, v) in &label.label_names {
			style_names.insert(k.clone(), v.clone());
		}
	} else {
		// Otherwise, scour the axis labels for matches.
		// Gather all languages in which at least one translation is provided
		// Then build names for all these languages, but fallback to English
		// whenever a translation is missing.
		let labels = get_axis_labels_for_user_location(&doc.axes, user_location)?;
		if !labels.is_empty() {
			let mut languages: Vec<&str> = Vec::new();
			for label in &labels {
				for language in label.label_names.keys() {
					if !languages.contains(&language.as_str()) {
						languages.push(language);
					}
				}
			}
			if !languages.contains(&"en") {
				languages.push("en");
			}
			for language in languages {
				let mut style_name = labels
					.iter()
					.filter(|label| !label.elidable)
					.map(|label| {
						label
							.label_names
							.get(language)
							.map_or_else(|| label.default_name(), String::as_str)
					})
					.collect::<Vec<_>>()
					.join(" ");
				if style_name.is_empty()
					&& let Some(fallback) = &doc.elided_fallback_name
				{
					style_name = fallback.clone();
				}
				style_names.insert(language.to_owned(), style_name);
			}
		}
	}

	let (Some(family_en), Some(style_en)) = (family_names.get("en"), style_names.get("en")) else {
		// Not enough information to compute PS names of styleMap names
		return Ok(StatNames {
			family_names,
			style_names,
			post_script_font_name: None,
			style_map_family_names: LocalizedNames::new(),
			style_map_style_name: None,
		});
	};

	let post_script_font_name = format!("{family_en}-{style_en}").replace(' ', "");

	let (style_map_style_name, regular_user_location) = get_ribbi_style(doc, user_location)?;

	let regular_stat_names;
	let mut style_names_for_style_map = &style_names;
	if regular_user_location != *user_location {
		regular_stat_names = get_stat_names(doc, &regular_user_location)?;
		style_names_for_style_map = &regular_stat_names.style_names;
	}

	let mut style_map_family_names = LocalizedNames::new();
	let mut languages: Vec<&String> = family_names.keys().collect();
	for language in style_names.keys() {
		if !languages.contains(&language) {
			languages.push(language);
		}
	}
	for language in languages {
		let family_name = family_names.get(language).unwrap_or(family_en);
		let style_name = match style_names_for_style_map.get(language) {
			Some(name) => name,
			None => style_names_for_style_map
				.get("en")
				.ok_or_else(|| key_error("en"))?,
		};
		style_map_family_names.insert(
			language.clone(),
			fontoxide_misc::py::strip(&format!("{family_name} {style_name}")).to_owned(),
		);
	}

	Ok(StatNames {
		post_script_font_name: Some(post_script_font_name),
		style_map_family_names,
		style_map_style_name: Some(style_map_style_name.to_owned()),
		family_names,
		style_names,
	})
}

/// `_getSortedAxisLabels`: axis labels sorted by their ordering, with
/// unordered ones appended as they are listed.
fn get_sorted_axis_labels(axes: &[Axis]) -> IndexMap<String, &[AxisLabelDescriptor]> {
	// First, get the axis labels with explicit ordering...
	let mut sorted_axes: Vec<&Axis> = axes
		.iter()
		.filter(|a| a.axis_ordering().is_some())
		.collect();
	sorted_axes.sort_by_key(|a| a.axis_ordering());
	let mut sorted_labels: IndexMap<String, &[AxisLabelDescriptor]> = sorted_axes
		.into_iter()
		.map(|axis| (axis.name_str(), axis.axis_labels()))
		.collect();
	// ... then append the others in the order they appear.
	for axis in axes {
		if axis.axis_ordering().is_none() {
			sorted_labels.insert(axis.name_str(), axis.axis_labels());
		}
	}
	sorted_labels
}

fn get_axis_labels_for_user_location<'a>(
	axes: &'a [Axis],
	user_location: &SimpleLocation,
) -> Result<Vec<&'a AxisLabelDescriptor>> {
	let mut labels = Vec::new();
	let all_axis_labels = get_sorted_axis_labels(axes);
	let same_keys = all_axis_labels.len() == user_location.len()
		&& all_axis_labels
			.keys()
			.all(|k| user_location.contains_key(k));
	if !same_keys {
		log::warn!(
			"Mismatch between user location '{:?}' and available labels for '{:?}'.",
			user_location.keys().collect::<Vec<_>>(),
			all_axis_labels.keys().collect::<Vec<_>>()
		);
	}
	for (axis_name, axis_labels) in all_axis_labels {
		let user_value = *user_location
			.get(&axis_name)
			.ok_or_else(|| key_error(&axis_name))?;
		let label = axis_labels.iter().find(|l| {
			l.user_value == user_value
				|| matches!(
					(l.user_minimum, l.user_maximum),
					(Some(min), Some(max)) if min <= user_value && user_value <= max
				)
		});
		match label {
			None => log::debug!(
				"Document needs a label for axis '{axis_name}', user value '{user_value}'."
			),
			Some(label) => labels.push(label),
		}
	}
	Ok(labels)
}

/// `_getRibbiStyle`: the RIBBI style name of the user location, and the
/// location of the matching Regular in the RIBBI group.
fn get_ribbi_style(
	doc: &DesignSpaceDocument,
	user_location: &SimpleLocation,
) -> Result<(&'static str, SimpleLocation)> {
	let mut regular_user_location = SimpleLocation::new();
	let mut axes_by_tag: IndexMap<Option<&str>, &Axis> = IndexMap::new();
	for axis in &doc.axes {
		axes_by_tag.insert(axis.tag(), axis);
	}

	let mut bold = false;
	let mut italic = false;

	if let Some(axis) = axes_by_tag.get(&Some("wght")) {
		let name = axis.name_str();
		for regular_label in axis.axis_labels() {
			let value = *user_location.get(&name).ok_or_else(|| key_error(&name))?;
			// In the "recursive" case where both the Regular has linkedUserValue
			// pointing the Bold, and the Bold has linkedUserValue pointing to the
			// Regular, only consider the first case: Regular (e.g. 400) has
			// linkedUserValue pointing to Bold (e.g. 700, higher than Regular)
			if let Some(linked) = regular_label.linked_user_value
				&& linked == value
				&& regular_label.user_value < linked
			{
				regular_user_location.insert(name.clone(), regular_label.user_value);
				bold = true;
				break;
			}
		}
	}

	let axis = axes_by_tag
		.get(&Some("ital"))
		.or_else(|| axes_by_tag.get(&Some("slnt")));
	if let Some(axis) = axis {
		let name = axis.name_str();
		for upright_label in axis.axis_labels() {
			let value = *user_location.get(&name).ok_or_else(|| key_error(&name))?;
			// In the "recursive" case where both the Upright has linkedUserValue
			// pointing the Italic, and the Italic has linkedUserValue pointing to
			// the Upright, only consider the first case: Upright (e.g. ital=0,
			// slant=0) has linkedUserValue pointing to Italic (e.g ital=1,
			// slant=-12 or slant=12 for backwards italics, in any case higher than
			// Upright in absolute value, hence the abs() below.
			if let Some(linked) = upright_label.linked_user_value
				&& linked == value
				&& upright_label.user_value.abs() < linked.abs()
			{
				regular_user_location.insert(name.clone(), upright_label.user_value);
				italic = true;
				break;
			}
		}
	}

	let style = match (bold, italic) {
		(false, false) => "regular",
		(false, true) => "italic",
		(true, false) => "bold",
		(true, true) => "bold italic",
	};
	let mut location = user_location.clone();
	for (k, v) in regular_user_location {
		location.insert(k, v);
	}
	Ok((style, location))
}
