//! Port of `fontTools.varLib.stat`: the `STAT` table data of a designspace
//! document.

use fontoxide_designspacelib::types::{Region, get_vf_user_region, location_in_region};
use fontoxide_designspacelib::{
	AxisLabelDescriptor, DesignSpaceDocument, DesignSpaceDocumentError, LocalizedNames,
	LocationLabelDescriptor, SimpleLocation,
};
use fontoxide_otllib::builder::{
	StatAxis, StatAxisValue, StatAxisValueKind, StatLocation, StatName, build_stat_table,
};
use fontoxide_ttlib::tables::Name;
use fontoxide_ttlib::{Font, Tag};

use crate::errors::Result;

/// `buildVFStatTable`: builds the `STAT` table of the variable font of a
/// document by name, with the axis and location labels within its region.
pub fn build_vf_stat_table(
	font: &mut Font,
	doc: &DesignSpaceDocument,
	vf_name: &str,
) -> Result<()> {
	let vf = doc
		.get_variable_fonts()
		.into_iter()
		.find(|vf| vf.name == vf_name)
		.ok_or_else(|| {
			DesignSpaceDocumentError::Document(format!(
				"Cannot find the variable font by name {vf_name}"
			))
		})?;
	let region = get_vf_user_region(doc, &vf)?;

	// If there are not currently any mac names don't add them here, that's
	// inconsistent (https://github.com/fonttools/fonttools/issues/683).
	let mac_names = match font.get::<Name>()? {
		Some(name) => name.names.iter().any(|n| n.platform_id == 1),
		None => false,
	};

	let elided = match &doc.elided_fallback_name {
		Some(name) => StatName::English(name.clone()),
		None => StatName::Id(2),
	};
	build_stat_table(
		font,
		&get_stat_axes(doc, &region)?,
		Some(&get_stat_locations(doc, &region)?),
		&elided,
		true,
		mac_names,
	)?;
	Ok(())
}

/// `{"en": name, **labelNames}`.
fn names(name: &str, label_names: &LocalizedNames) -> StatName {
	let mut out: Vec<(String, String)> = vec![("en".to_owned(), name.to_owned())];
	for (lang, s) in label_names {
		match out.iter_mut().find(|(l, _)| l == lang) {
			Some(entry) => entry.1 = s.clone(),
			None => out.push((lang.clone(), s.clone())),
		}
	}
	StatName::Multilingual(out)
}

fn tag(axis_tag: Option<&str>) -> Result<Tag> {
	let t = axis_tag.unwrap_or("None");
	Ok(Tag::from_str_padded(t)
		.ok_or_else(|| fontoxide_ttlib::Error::value(format!("invalid tag: {t:?}")))?)
}

/// `getStatAxes`: the axes for `buildStatTable`, with their labels in the
/// region.
pub fn get_stat_axes(doc: &DesignSpaceDocument, user_region: &Region) -> Result<Vec<StatAxis>> {
	// First, get the axis labels with explicit ordering, then append the others in
	// the order they appear.
	let mut max_ordering = doc
		.axes
		.iter()
		.filter_map(|a| a.axis_ordering())
		.max()
		.unwrap_or(-1);
	let mut axes = Vec::with_capacity(doc.axes.len());
	for axis in &doc.axes {
		let ordering = match axis.axis_ordering() {
			Some(o) => o,
			None => {
				max_ordering += 1;
				max_ordering
			}
		};
		let name = axis.name().unwrap_or("None");
		let mut values = Vec::new();
		for label in axis.axis_labels() {
			let location: SimpleLocation =
				[(name.to_owned(), label.user_value)].into_iter().collect();
			if location_in_region(&location, user_region) {
				values.push(axis_label_to_stat_location(label));
			}
		}
		axes.push(StatAxis {
			tag: tag(axis.tag())?,
			name: names(name, axis.label_names()),
			ordering: Some(fontoxide_ttlib::binary::fit(ordering, "ordering")?),
			values,
		});
	}
	Ok(axes)
}

/// `getStatLocations`: the locations for `buildStatTable`: the location labels
/// in the region.
pub fn get_stat_locations(
	doc: &DesignSpaceDocument,
	user_region: &Region,
) -> Result<Vec<StatLocation>> {
	let mut out = Vec::new();
	for label in &doc.location_labels {
		let location = label.get_full_user_location(doc);
		if !location_in_region(&location, user_region) {
			continue;
		}
		let mut tags = Vec::with_capacity(location.len());
		for (name, value) in &location {
			// Location in the designspace is keyed by axis name, location in
			// buildStatTable by axis tag.
			let axis = doc
				.get_axis(name)
				.ok_or_else(|| fontoxide_ttlib::Error::ttlib(format!("KeyError: {name:?}")))?;
			tags.push((tag(axis.tag())?, value.to_f64()));
		}
		out.push(StatLocation {
			name: names(&label.name, &label.label_names),
			location: tags,
			flags: location_label_flags(label),
		});
	}
	Ok(out)
}

/// `_labelToFlags`.
fn flags(older_sibling: bool, elidable: bool) -> u16 {
	let mut flags = 0;
	if older_sibling {
		flags |= 1;
	}
	if elidable {
		flags |= 2;
	}
	flags
}

fn location_label_flags(label: &LocationLabelDescriptor) -> u16 {
	flags(label.older_sibling, label.elidable)
}

/// `_axisLabelToStatLocation`.
fn axis_label_to_stat_location(label: &AxisLabelDescriptor) -> StatAxisValue {
	let name = names(&label.name, &label.label_names);
	let flags = flags(label.older_sibling, label.elidable);
	let kind = match label.get_format() {
		2 => StatAxisValueKind::Range {
			nominal_value: label.user_value.to_f64(),
			range_min_value: label.user_minimum.map(|v| v.to_f64()),
			range_max_value: label.user_maximum.map(|v| v.to_f64()),
		},
		3 => StatAxisValueKind::Value {
			value: label.user_value.to_f64(),
			linked_value: label.linked_user_value.map(|v| v.to_f64()),
		},
		_ => StatAxisValueKind::Value {
			value: label.user_value.to_f64(),
			linked_value: None,
		},
	};
	StatAxisValue { name, flags, kind }
}
