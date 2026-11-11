//! Port of `fontTools.designspaceLib.split`: splitting a format 5 document
//! into interpolable sub-spaces, then into each variable font.
//!
//! fontTools returns generators; here the results are collected (the
//! warnings about locations outside the axes are logged before any result).

use indexmap::IndexMap;

use fontoxide_misc::plistlib;
use fontoxide_misc::py;

use crate::descriptors::{
	AnisotropicLocation, Axis, AxisDescriptor, AxisMappingDescriptor, Condition,
	InstanceDescriptor, LocalizedNames, RuleDescriptor, SimpleLocation, SourceDescriptor,
	VariableFontDescriptor,
};
use crate::document::{DesignSpaceDocument, product};
use crate::number::Number;
use crate::stat_names::{StatNames, get_stat_names};
use crate::types::{
	ConditionSet, Range, Region, RegionValue, get_vf_user_region, location_in_region,
	region_in_region, user_region_to_design_region,
};
use crate::{DesignSpaceDocumentError, Result};

/// `MakeInstanceFilenameCallable`.
pub type MakeInstanceFilename<'a> =
	&'a dyn Fn(&DesignSpaceDocument, &InstanceDescriptor, &StatNames) -> String;

/// `defaultMakeInstanceFilename`: `"{familyName}-{styleName}.ttf"` for
/// instances without file name when `makeNames` is set.
pub fn default_make_instance_filename(
	_doc: &DesignSpaceDocument,
	instance: &InstanceDescriptor,
	stat_names: &StatNames,
) -> String {
	let family_name = non_empty(&instance.family_name)
		.or_else(|| stat_names.family_names.get("en").map(String::as_str))
		.unwrap_or("None");
	let style_name = non_empty(&instance.style_name)
		.or_else(|| stat_names.style_names.get("en").map(String::as_str))
		.unwrap_or("None");
	format!("{family_name}-{style_name}.ttf")
}

/// `value or None` for an optional string.
fn non_empty(value: &Option<String>) -> Option<&str> {
	value.as_deref().filter(|s| !s.is_empty())
}

/// `_localisedNames`: the localised STAT names if they translate `name`.
fn localised_names(stat_names: &LocalizedNames, name: Option<&str>) -> LocalizedNames {
	if stat_names.get("en").map(String::as_str) == name {
		stat_names.clone()
	} else {
		LocalizedNames::new()
	}
}

/// The `repr` of a simple location, as in Python log messages.
fn location_repr(location: &SimpleLocation) -> String {
	let items: Vec<String> = location
		.iter()
		.map(|(k, v)| format!("{}: {}", py::str_repr(k), v.repr()))
		.collect();
	format!("{{{}}}", items.join(", "))
}

fn doc_user_region(doc: &DesignSpaceDocument) -> Result<IndexMap<String, DocRegion>> {
	Ok(doc
		.axes
		.iter()
		.map(|axis| {
			let value = match axis {
				Axis::Discrete(a) => DocRegion::Values(a.values.clone()),
				Axis::Continuous(a) => {
					DocRegion::Range(Range::new(a.minimum, a.maximum, a.default))
				}
			};
			(axis.name_str(), value)
		})
		.collect())
}

/// The region of `_warnOutOfAxes`: a discrete axis is a tuple of values.
enum DocRegion {
	Values(Vec<Number>),
	Range(Range),
}

fn location_in_doc_region(location: &SimpleLocation, region: &IndexMap<String, DocRegion>) -> bool {
	location.iter().all(|(name, value)| match region.get(name) {
		None => false,
		// `value in tuple(axis.values)`.
		Some(DocRegion::Values(values)) => values.contains(value),
		Some(DocRegion::Range(r)) => r.contains(*value),
	})
}

/// `_warnOutOfAxes`: warns about sources and instances outside the axes.
fn warn_out_of_axes(doc: &DesignSpaceDocument) -> Result<()> {
	let doc_user_region = doc_user_region(doc)?;
	for source in &doc.sources {
		let source_user_location = doc.map_backward(&source.design_location)?;
		if !location_in_doc_region(&source_user_location, &doc_user_region) {
			log::warn!(
				"Source '{}' has location outside the document's axes, dropping: {}",
				non_empty(&source.name)
					.or(non_empty(&source.filename))
					.unwrap_or("<unnamed source>"),
				location_repr(&source_user_location)
			);
		}
	}
	for instance in &doc.instances {
		let instance_user_location = instance.get_full_user_location(doc)?;
		if !location_in_doc_region(&instance_user_location, &doc_user_region) {
			let names = format!(
				"{} {}",
				instance.family_name.as_deref().unwrap_or(""),
				instance.style_name.as_deref().unwrap_or("")
			);
			let names = py::strip(&names);
			log::warn!(
				"Instance '{}' has location outside the document's axes, dropping: {}",
				non_empty(&instance.name)
					.or(non_empty(&instance.filename))
					.or(Some(names).filter(|s| !s.is_empty()))
					.unwrap_or("<unnamed instance>"),
				location_repr(&instance_user_location)
			);
		}
	}
	Ok(())
}

/// The options of [`split_interpolable`] and [`split_variable_fonts`].
#[derive(Clone, Copy)]
pub struct SplitOptions<'a> {
	/// Whether to compute the instance family and style names using the STAT
	/// data (`makeNames`).
	pub make_names: bool,
	/// Whether to turn all locations into full locations, including implicit
	/// default axis values (`expandLocations`).
	pub expand_locations: bool,
	/// Synthesizes an instance file name when `make_names` is set, for
	/// instances without one (`makeInstanceFilename`).
	pub make_instance_filename: MakeInstanceFilename<'a>,
}

impl std::fmt::Debug for SplitOptions<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("SplitOptions")
			.field("make_names", &self.make_names)
			.field("expand_locations", &self.expand_locations)
			.finish_non_exhaustive()
	}
}

impl SplitOptions<'_> {
	/// The defaults of `splitInterpolable`: names and full locations.
	pub fn interpolable() -> Self {
		SplitOptions {
			make_names: true,
			expand_locations: true,
			make_instance_filename: &default_make_instance_filename,
		}
	}

	/// The defaults of `splitVariableFonts`: neither.
	pub fn variable_fonts() -> Self {
		SplitOptions {
			make_names: false,
			expand_locations: false,
			make_instance_filename: &default_make_instance_filename,
		}
	}
}

/// `splitInterpolable`: one sub-document per combination of discrete axis
/// values, in which the continuous axes interpolate.
///
/// The sub-documents still include the full axis definitions and STAT data,
/// but the rules, sources, variable fonts and instances are trimmed down to
/// those within the interpolable sub-space.
pub fn split_interpolable(
	doc: &DesignSpaceDocument,
	options: SplitOptions<'_>,
) -> Result<Vec<(SimpleLocation, DesignSpaceDocument)>> {
	warn_out_of_axes(doc)?;
	let mut discrete_axes = Vec::new();
	let mut interpolable_user_region = Region::new();
	for axis in &doc.axes {
		match axis {
			Axis::Discrete(a) => discrete_axes.push(a),
			Axis::Continuous(a) => {
				interpolable_user_region.insert(
					axis.name_str(),
					RegionValue::Range(Range::new(a.minimum, a.maximum, a.default)),
				);
			}
		}
	}
	let value_lists: Vec<&[Number]> = discrete_axes.iter().map(|a| a.values.as_slice()).collect();
	let mut out = Vec::new();
	for values in product(&value_lists) {
		let discrete_user_location: SimpleLocation = discrete_axes
			.iter()
			.zip(&values)
			.map(|(axis, value)| (axis.name.clone().unwrap_or_else(|| "None".into()), *value))
			.collect();
		let mut region = interpolable_user_region.clone();
		for (k, v) in &discrete_user_location {
			region.insert(k.clone(), RegionValue::Value(*v));
		}
		let sub_doc = extract_sub_space(doc, &region, true, options)?;
		out.push((discrete_user_location, sub_doc));
	}
	Ok(out)
}

/// `splitVariableFonts`: one standalone document per variable font.
pub fn split_variable_fonts(
	doc: &DesignSpaceDocument,
	options: SplitOptions<'_>,
) -> Result<Vec<(String, DesignSpaceDocument)>> {
	warn_out_of_axes(doc)?;
	// Make one DesignspaceDoc v5 for each variable font
	let mut out = Vec::new();
	for vf in doc.get_variable_fonts() {
		let vf_user_region = get_vf_user_region(doc, &vf)?;
		let mut vf_doc = extract_sub_space(doc, &vf_user_region, false, options)?;
		for (k, v) in &vf.lib {
			vf_doc.lib.insert(k.clone(), v.clone());
		}
		if let (Some(doc_info), Some(vf_info)) = (
			doc.lib.get("public.fontInfo"),
			vf.lib.get("public.fontInfo"),
		) {
			let (Some(doc_info), Some(vf_info)) = (doc_info.as_dict(), vf_info.as_dict()) else {
				return Err(DesignSpaceDocumentError::Value(
					"'public.fontInfo' must be a mapping".into(),
				));
			};
			let mut merged = doc_info.clone();
			for (k, v) in vf_info {
				merged.insert(k.clone(), v.clone());
			}
			vf_doc
				.lib
				.insert("public.fontInfo".to_owned(), plistlib::Value::Dict(merged));
		}
		out.push((vf.name.clone(), vf_doc));
	}
	Ok(out)
}

/// `convert5to4`: each variable font as a standalone format 4 document.
pub fn convert5to4(doc: &DesignSpaceDocument) -> Result<IndexMap<String, DesignSpaceDocument>> {
	let mut vfs = IndexMap::new();
	for (_location, sub_doc) in split_interpolable(doc, SplitOptions::interpolable())? {
		for (vf_name, mut vf_doc) in split_variable_fonts(&sub_doc, SplitOptions::variable_fonts())?
		{
			vf_doc.format_version = Some("4.1".to_owned());
			vfs.insert(vf_name, vf_doc);
		}
	}
	Ok(vfs)
}

fn extract_sub_space(
	doc: &DesignSpaceDocument,
	user_region: &Region,
	keep_vfs: bool,
	options: SplitOptions<'_>,
) -> Result<DesignSpaceDocument> {
	let mut sub_doc = DesignSpaceDocument::new();
	// Don't include STAT info
	// FIXME: (Jany) let's think about it. Not include = OK because the point of
	// the splitting is to build VFs and we'll use the STAT data of the full
	// document to generate the STAT of the VFs, so "no need" to have STAT data in
	// sub-docs. Counterpoint: what if someone wants to split this DS for other
	// purposes?  Maybe for that it would be useful to also subset the STAT data?
	// subDoc.elidedFallbackName = doc.elidedFallbackName

	let region_of = |name: &str| -> Result<&RegionValue> {
		user_region.get(name).ok_or_else(|| {
			DesignSpaceDocumentError::Value(format!("KeyError: {}", py::str_repr(name)))
		})
	};

	for axis in &doc.axes {
		let range = region_of(&axis.name_str())?;
		if let (RegionValue::Range(range), Axis::Continuous(axis)) = (range, axis) {
			sub_doc.add_axis(AxisDescriptor {
				// Same info
				tag: axis.tag.clone(),
				name: axis.name.clone(),
				label_names: axis.label_names.clone(),
				hidden: axis.hidden,
				// Subset range
				minimum: range.minimum.py_max(axis.minimum),
				default: range.default,
				maximum: range.maximum.py_min(axis.maximum),
				map: axis
					.map
					.iter()
					.copied()
					.filter(|&(user, _)| range.minimum <= user && user <= range.maximum)
					.collect(),
				// Don't include STAT info
				axis_ordering: None,
				axis_labels: Vec::new(),
			});
		}
	}

	let sub_doc_axes: Vec<String> = sub_doc.axes.iter().map(Axis::name_str).collect();
	let mut mappings = Vec::new();
	for mapping in &doc.axis_mappings {
		if !mapping
			.input_location
			.keys()
			.all(|a| sub_doc_axes.contains(a))
		{
			continue;
		}
		if !mapping
			.output_location
			.keys()
			.all(|a| sub_doc_axes.contains(a))
		{
			log::error!(
				"In axis mapping from input {}, some output axes are not in the variable-font: {}",
				location_repr(&mapping.input_location),
				location_repr(&mapping.output_location)
			);
			continue;
		}
		for axis in &doc.axes {
			let name = axis.name_str();
			if !mapping.input_location.contains_key(&name)
				&& !mapping.output_location.contains_key(&name)
			{
				continue;
			}
			let RegionValue::Range(range) = region_of(&name)? else {
				return Err(DesignSpaceDocumentError::Value(
					"'float' object has no attribute 'minimum'".into(),
				));
			};
			if range.minimum != axis.minimum()?
				|| range.default != axis.default()
				|| range.maximum != axis.maximum()?
			{
				// UPSTREAM-BUG(B32): the `continue` only skips to the next axis, so the
				// mapping is kept despite the error.
				log::error!(
					"Limiting axis ranges used in <mapping> elements not supported: {name}"
				);
				continue;
			}
		}
		mappings.push(AxisMappingDescriptor {
			input_location: mapping.input_location.clone(),
			output_location: mapping.output_location.clone(),
			description: None,
			group_description: None,
		});
	}
	sub_doc.axis_mappings = mappings;

	// Don't include STAT info
	// subDoc.locationLabels = doc.locationLabels

	// Rules: subset them based on conditions
	let design_region = user_region_to_design_region(doc, user_region)?;
	sub_doc.rules = subset_rules_based_on_conditions(&doc.rules, &design_region);
	sub_doc.rules_processing_last = doc.rules_processing_last;

	let maybe_expand = |design_location: &AnisotropicLocation,
	                    full: &dyn Fn() -> Result<AnisotropicLocation>|
	 -> Result<AnisotropicLocation> {
		if options.expand_locations {
			full()
		} else {
			Ok(design_location.clone())
		}
	};

	// Sources: keep only the ones that fall within the kept axis ranges
	for source in &doc.sources {
		if !location_in_region(&doc.map_backward(&source.design_location)?, user_region) {
			continue;
		}
		let design_location = maybe_expand(&source.design_location, &|| {
			source.get_full_design_location(doc)
		})?;
		sub_doc.add_source(SourceDescriptor {
			filename: source.filename.clone(),
			path: source.path.clone(),
			name: source.name.clone(),
			design_location: filter_location(user_region, &design_location),
			layer_name: source.layer_name.clone(),
			family_name: source.family_name.clone(),
			style_name: source.style_name.clone(),
			mute_kerning: source.mute_kerning,
			mute_info: source.mute_info,
			muted_glyph_names: source.muted_glyph_names.clone(),
			..Default::default()
		});
	}

	// Copy family name translations from the old default source to the new
	// default
	sub_doc.find_default()?;
	let old_default = doc.compute_default()?;
	if let (Some(i), Some(old_default)) = (sub_doc.default, old_default) {
		sub_doc.sources[i].localised_family_name =
			doc.sources[old_default].localised_family_name.clone();
	}

	// Variable fonts: keep only the ones that fall within the kept axis ranges
	if keep_vfs {
		// Note: call getVariableFont() to make the implicit VFs explicit
		for vf in doc.get_variable_fonts() {
			let vf_user_region = get_vf_user_region(doc, &vf)?;
			if region_in_region(&vf_user_region, user_region) {
				let mut axis_subsets = Vec::new();
				for axis_subset in &vf.axis_subsets {
					if matches!(region_of(axis_subset.name())?, RegionValue::Range(_)) {
						axis_subsets.push(axis_subset.clone());
					}
				}
				sub_doc.add_variable_font(VariableFontDescriptor {
					name: vf.name.clone(),
					filename: vf.filename.clone(),
					axis_subsets,
					lib: vf.lib.clone(),
				});
			}
		}
	}

	// Instances: same as Sources + compute missing names
	for instance in &doc.instances {
		if !location_in_region(&instance.get_full_user_location(doc)?, user_region) {
			continue;
		}
		let design_location = maybe_expand(&instance.design_location, &|| {
			instance.get_full_design_location(doc)
		})?;
		let design_location = filter_location(user_region, &design_location);
		let user_location = if options.expand_locations {
			SimpleLocation::new()
		} else {
			instance.user_location.clone()
		};

		let new_instance = if options.make_names {
			let stat_names = get_stat_names(doc, &instance.get_full_user_location(doc)?)?;
			// Names set explicitly on the instance take precedence over the ones
			// computed from the STAT labels, in every language: the localised STAT
			// names only apply when they translate the English name that is
			// actually used, and a PostScript name is only made up for an instance
			// that has no style name of its own.
			let family_name = non_empty(&instance.family_name)
				.or_else(|| stat_names.family_names.get("en").map(String::as_str))
				.map(str::to_owned);
			let style_name = non_empty(&instance.style_name)
				.or_else(|| stat_names.style_names.get("en").map(String::as_str))
				.map(str::to_owned);
			let localised_family_name = if instance.localised_family_name.is_empty() {
				localised_names(&stat_names.family_names, family_name.as_deref())
			} else {
				instance.localised_family_name.clone()
			};
			let localised_style_name = if instance.localised_style_name.is_empty() {
				localised_names(&stat_names.style_names, style_name.as_deref())
			} else {
				instance.localised_style_name.clone()
			};
			let mut post_script_font_name = instance.post_script_font_name.clone();
			if post_script_font_name.is_none()
				&& non_empty(&instance.style_name).is_none()
				&& instance.localised_style_name.is_empty()
			{
				post_script_font_name = stat_names.post_script_font_name.clone();
			}
			let filename = match non_empty(&instance.filename) {
				Some(f) => f.to_owned(),
				None => (options.make_instance_filename)(doc, instance, &stat_names),
			};
			let name = match non_empty(&instance.name) {
				Some(n) => n.to_owned(),
				None => format!(
					"{} {}",
					family_name.as_deref().unwrap_or("None"),
					style_name.as_deref().unwrap_or("None")
				),
			};
			let mut new = InstanceDescriptor::new();
			new.set_filename(Some(&filename));
			new.path = instance.path.clone();
			new.name = Some(name);
			new.user_location = user_location;
			new.design_location = design_location;
			new.family_name = family_name;
			new.style_name = style_name;
			new.post_script_font_name = post_script_font_name;
			new.style_map_family_name = non_empty(&instance.style_map_family_name)
				.or_else(|| {
					stat_names
						.style_map_family_names
						.get("en")
						.map(String::as_str)
				})
				.map(str::to_owned);
			new.style_map_style_name = non_empty(&instance.style_map_style_name)
				.map(str::to_owned)
				.or_else(|| stat_names.style_map_style_name.clone());
			new.localised_family_name = localised_family_name;
			new.localised_style_name = localised_style_name;
			new.localised_style_map_family_name =
				if instance.localised_style_map_family_name.is_empty() {
					stat_names.style_map_family_names.clone()
				} else {
					instance.localised_style_map_family_name.clone()
				};
			new.localised_style_map_style_name = instance.localised_style_map_style_name.clone();
			new.lib = instance.lib.clone();
			new
		} else {
			InstanceDescriptor {
				filename: instance.filename.clone(),
				path: instance.path.clone(),
				name: instance.name.clone(),
				user_location,
				design_location,
				family_name: instance.family_name.clone(),
				style_name: instance.style_name.clone(),
				post_script_font_name: instance.post_script_font_name.clone(),
				style_map_family_name: instance.style_map_family_name.clone(),
				style_map_style_name: instance.style_map_style_name.clone(),
				localised_family_name: instance.localised_family_name.clone(),
				localised_style_name: instance.localised_style_name.clone(),
				localised_style_map_family_name: instance.localised_style_map_family_name.clone(),
				localised_style_map_style_name: instance.localised_style_map_style_name.clone(),
				lib: instance.lib.clone(),
				..Default::default()
			}
		};
		sub_doc.add_instance(new_instance);
	}

	sub_doc.lib = doc.lib.clone();

	Ok(sub_doc)
}

/// `_conditionSetFrom`: a condition set as ranges; missing bounds are
/// infinite.
pub fn condition_set_from(condition_set: &[Condition]) -> ConditionSet {
	let mut c = ConditionSet::new();
	for condition in condition_set {
		c.insert(
			condition.name.clone(),
			Range::without_default(
				condition
					.minimum
					.unwrap_or(Number::Float(f64::NEG_INFINITY)),
				condition.maximum.unwrap_or(Number::Float(f64::INFINITY)),
			),
		);
	}
	c
}

/// `_subsetRulesBasedOnConditions`.
fn subset_rules_based_on_conditions(
	rules: &[RuleDescriptor],
	design_region: &Region,
) -> Vec<RuleDescriptor> {
	// What rules to keep:
	//  - Keep the rule if any conditionset is relevant.
	//  - A conditionset is relevant if all conditions are relevant or it is empty.
	//  - A condition is relevant if
	//    - axis is point (C-AP),
	//       - and point in condition's range (C-AP-in)
	//            (in this case remove the condition because it's always true)
	//       - else (C-AP-out) whole conditionset can be discarded (condition false
	//         => conditionset false)
	//    - axis is range (C-AR),
	//       - (C-AR-all) and axis range fully contained in condition range: we can
	//         scrap the condition because it's always true
	//       - (C-AR-inter) and intersection(axis range, condition range) not empty:
	//         keep the condition with the smaller range (= intersection)
	//       - (C-AR-none) else, whole conditionset can be discarded
	let mut new_rules = Vec::new();
	for rule in rules {
		let mut new_rule = RuleDescriptor {
			name: rule.name.clone(),
			condition_sets: Vec::new(),
			subs: rule.subs.clone(),
		};
		for conditionset in &rule.condition_sets {
			let cs = condition_set_from(conditionset);
			let mut new_conditionset = Vec::new();
			let mut discard_conditionset = false;
			for (selection_name, selection_value) in design_region {
				// TODO: Ensure that all(key in conditionset for key in region.keys())?
				let Some(condition) = cs.get(selection_name) else {
					// raise Exception("Selection has different axes than the rules")
					continue;
				};
				match selection_value {
					RegionValue::Value(v) => {
						// is point
						// Case C-AP-in: always matches, conditionset can stay empty
						// for this one. Case C-AP-out:
						if !condition.contains(*v) {
							discard_conditionset = true;
						}
					}
					RegionValue::Range(r) => {
						// is range
						// Case C-AR-all: always matches, conditionset can stay
						// empty for this one.
						if !condition.contains_range(r) {
							match condition.intersection(r) {
								// Case C-AR-inter
								Some(intersection) => new_conditionset.push(Condition {
									name: selection_name.clone(),
									minimum: Some(intersection.minimum),
									maximum: Some(intersection.maximum),
								}),
								// Case C-AR-none
								None => discard_conditionset = true,
							}
						}
					}
				}
			}
			if !discard_conditionset {
				new_rule.condition_sets.push(new_conditionset);
			}
		}
		if !new_rule.condition_sets.is_empty() {
			new_rules.push(new_rule);
		}
	}
	new_rules
}

/// `_filterLocation`: the location on the axes that are ranges in the region.
fn filter_location(user_region: &Region, location: &AnisotropicLocation) -> AnisotropicLocation {
	location
		.iter()
		.filter(|(name, _)| matches!(user_region.get(*name), Some(RegionValue::Range(_))))
		.map(|(k, v)| (k.clone(), *v))
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	/// `test_optional_min_max_internal`.
	#[test]
	fn optional_min_max_internal() {
		let cases = [
			(Some(0.5), None, Range::without_default(0.5, f64::INFINITY)),
			(
				None,
				Some(0.5),
				Range::without_default(f64::NEG_INFINITY, 0.5),
			),
		];
		for (minimum, maximum, expected) in cases {
			let condition = Condition::new(
				"axis",
				minimum.map(Number::Float),
				maximum.map(Number::Float),
			);
			let mut expected_set = ConditionSet::new();
			expected_set.insert("axis".to_owned(), expected);
			assert_eq!(condition_set_from(&[condition]), expected_set);
		}
	}
}
