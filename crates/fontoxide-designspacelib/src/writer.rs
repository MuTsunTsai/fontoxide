//! `BaseDocWriter`: writes format 4 and 5 documents.

use fontoxide_misc::etree::{self, Element, XML_NS};
use fontoxide_misc::plistlib;

use crate::descriptors::{
	AnisotropicLocation, Axis, AxisLabelDescriptor, AxisMappingDescriptor, AxisSubset, GlyphData,
	LocalizedNames, LocationLabelDescriptor, LocationValue, RuleDescriptor, SimpleLocation,
	VariableFontDescriptor, to_anisotropic,
};
use crate::document::DesignSpaceDocument;
use crate::number::Number;
use crate::{DesignSpaceDocumentError, Result};

fn xml_lang() -> String {
	format!("{XML_NS}lang")
}

/// `intOrFloat`: integral values without decimals, others with up to six.
pub fn int_or_float(num: Number) -> Result<String> {
	match num {
		Number::Int(i) => Ok(i.to_string()),
		Number::Float(f) => {
			if f.is_nan() {
				return Err(DesignSpaceDocumentError::Value(
					"cannot convert float NaN to integer".into(),
				));
			}
			if f.is_infinite() {
				return Err(DesignSpaceDocumentError::Value(
					"cannot convert float infinity to integer".into(),
				));
			}
			if f.trunc() == f {
				if f == 0.0 {
					return Ok("0".into());
				}
				return Ok(format!("{f:.0}"));
			}
			let s = format!("{f:.6}");
			Ok(s.trim_end_matches('0').trim_end_matches('.').to_owned())
		}
	}
}

fn none_attr(what: &str) -> DesignSpaceDocumentError {
	DesignSpaceDocumentError::Value(format!(
		"Argument must be bytes or unicode, got 'NoneType' ({what})"
	))
}

struct Writer {
	effective_format_tuple: (i128, i128),
	/// `newDefaultLocation()`, for format 4 locations.
	default_location: AnisotropicLocation,
	/// The axis names in order, for format 5 locations.
	axis_names: Vec<String>,
}

/// Writes the document as XML; format 4 locations of sources, instances and
/// instance glyphs are filled in with the axis defaults, as fontTools does.
pub(crate) fn write(
	doc: &mut DesignSpaceDocument,
	pretty: bool,
	xml_declaration: bool,
) -> Result<String> {
	let writer = Writer {
		effective_format_tuple: get_effective_format_tuple(doc)?,
		default_location: to_anisotropic(&doc.new_default_location()?),
		axis_names: doc.axes.iter().map(Axis::name_str).collect(),
	};
	let root = writer.build(doc)?;
	etree::tostring(&root, pretty, xml_declaration)
		.map_err(|e| DesignSpaceDocumentError::Value(e.0))
}

/// `_getEffectiveFormatTuple`: the document version, or a recent enough one
/// for what it contains.
fn get_effective_format_tuple(doc: &DesignSpaceDocument) -> Result<(i128, i128)> {
	let mut min_version = doc.format_tuple()?;
	if doc.axes.iter().any(|axis| {
		matches!(axis, Axis::Discrete(_))
			|| axis.axis_ordering().is_some()
			|| !axis.axis_labels().is_empty()
	}) || !doc.location_labels.is_empty()
		|| doc
			.sources
			.iter()
			.any(|s| !s.localised_family_name.is_empty())
		|| !doc.variable_fonts.is_empty()
		|| doc.instances.iter().any(|i| {
			i.location_label.as_deref().is_some_and(|l| !l.is_empty())
				|| !i.user_location.is_empty()
		}) {
		min_version = min_version.max((5, 0));
	}
	if !doc.axis_mappings.is_empty() {
		min_version = min_version.max((5, 1));
	}
	Ok(min_version)
}

impl Writer {
	fn build(&self, doc: &mut DesignSpaceDocument) -> Result<Element> {
		let mut root = Element::new("designspace");
		let (major, minor) = self.effective_format_tuple;
		root.set("format", format!("{major}.{minor}"));

		if !doc.axes.is_empty()
			|| !doc.axis_mappings.is_empty()
			|| doc.elided_fallback_name.is_some()
		{
			let mut axes_element = Element::new("axes");
			if let Some(name) = &doc.elided_fallback_name {
				axes_element.set("elidedfallbackname", name.clone());
			}
			for axis in &doc.axes {
				axes_element.append(self.axis_element(axis)?);
			}
			if !doc.axis_mappings.is_empty() {
				let mut mappings_element: Option<Element> = None;
				// `object()`: different from any description.
				let mut last_group: Option<Option<&String>> = None;
				for mapping in &doc.axis_mappings {
					let group = mapping.group_description.as_ref();
					if last_group != Some(group) {
						if let Some(element) = mappings_element.take() {
							axes_element.append(element);
						}
						last_group = Some(group);
						let mut element = Element::new("mappings");
						if let Some(group) = group {
							element.set("description", group.clone());
						}
						mappings_element = Some(element);
					}
					if let Some(element) = &mut mappings_element {
						element.append(self.axis_mapping_element(mapping)?);
					}
				}
				if let Some(element) = mappings_element {
					axes_element.append(element);
				}
			}
			root.append(axes_element);
		}

		if !doc.location_labels.is_empty() {
			let mut labels_element = Element::new("labels");
			for label in &doc.location_labels {
				labels_element.append(self.location_label_element(label)?);
			}
			root.append(labels_element);
		}

		if !doc.rules.is_empty() {
			let mut rules_element = Element::new("rules");
			if doc.rules_processing_last {
				rules_element.set("processing", "last");
			}
			for rule in &doc.rules {
				let rule_element = self.rule_element(rule)?;
				if !rule_element.is_empty() {
					rules_element.append(rule_element);
				}
			}
			root.append(rules_element);
		}

		if !doc.sources.is_empty() {
			let mut sources_element = Element::new("sources");
			for i in 0..doc.sources.len() {
				sources_element.append(self.source_element(doc, i)?);
			}
			root.append(sources_element);
		}

		if !doc.variable_fonts.is_empty() {
			let mut vfs_element = Element::new("variable-fonts");
			for vf in &doc.variable_fonts {
				vfs_element.append(self.variable_font_element(vf)?);
			}
			root.append(vfs_element);
		}

		if !doc.instances.is_empty() {
			let mut instances_element = Element::new("instances");
			for i in 0..doc.instances.len() {
				instances_element.append(self.instance_element(doc, i)?);
			}
			root.append(instances_element);
		}

		add_lib(&mut root, &doc.lib, 2)?;
		Ok(root)
	}

	/// `_makeLocationElement`: a format 4 location, completed with the axis
	/// defaults; values of unknown axes are dropped.
	fn make_location_element(
		&self,
		location: &AnisotropicLocation,
		name: Option<&str>,
	) -> Result<(Element, AnisotropicLocation)> {
		let mut loc_element = Element::new("location");
		if let Some(name) = name {
			loc_element.set("name", name);
		}
		let mut validated = self.default_location.clone();
		for (axis_name, axis_value) in location {
			if let Some(slot) = validated.get_mut(axis_name) {
				// only accept values we know
				*slot = *axis_value;
			}
		}
		for (dimension_name, dimension_value) in &validated {
			loc_element.append(dimension_element(dimension_name, *dimension_value)?);
		}
		Ok((loc_element, validated))
	}

	fn rule_element(&self, rule: &RuleDescriptor) -> Result<Element> {
		let mut rule_element = Element::new("rule");
		if let Some(name) = &rule.name {
			rule_element.set("name", name.clone());
		}
		for conditions in &rule.condition_sets {
			let mut conditionset_element = Element::new("conditionset");
			for cond in conditions {
				if cond.minimum.is_none() && cond.maximum.is_none() {
					// neither is defined, don't add this condition
					continue;
				}
				let mut condition_element = Element::new("condition");
				condition_element.set("name", cond.name.clone());
				if let Some(minimum) = cond.minimum {
					condition_element.set("minimum", int_or_float(minimum)?);
				}
				if let Some(maximum) = cond.maximum {
					condition_element.set("maximum", int_or_float(maximum)?);
				}
				conditionset_element.append(condition_element);
			}
			// Serialize the conditionset even if it is empty, as this is the
			// canonical way of defining a rule that is always true.
			rule_element.append(conditionset_element);
		}
		for (a, b) in &rule.subs {
			let mut sub_element = Element::new("sub");
			sub_element.set("name", a.clone());
			sub_element.set("with", b.clone());
			rule_element.append(sub_element);
		}
		Ok(rule_element)
	}

	fn axis_element(&self, axis: &Axis) -> Result<Element> {
		let mut axis_element = Element::new("axis");
		axis_element.set("tag", axis.tag().ok_or_else(|| none_attr("tag"))?);
		axis_element.set("name", axis.name().ok_or_else(|| none_attr("name"))?);
		add_label_names(&mut axis_element, axis.label_names());
		for &(input_value, output_value) in axis.map() {
			let mut map_element = Element::new("map");
			map_element.set("input", int_or_float(input_value)?);
			map_element.set("output", int_or_float(output_value)?);
			axis_element.append(map_element);
		}
		if axis.axis_ordering().is_some() || !axis.axis_labels().is_empty() {
			let mut labels_element = Element::new("labels");
			if let Some(ordering) = axis.axis_ordering() {
				labels_element.set("ordering", ordering.to_string());
			}
			for label in axis.axis_labels() {
				labels_element.append(axis_label_element(label)?);
			}
			axis_element.append(labels_element);
		}
		match axis {
			Axis::Continuous(a) => {
				axis_element.set("minimum", int_or_float(a.minimum)?);
				axis_element.set("maximum", int_or_float(a.maximum)?);
			}
			Axis::Discrete(a) => {
				let values = a
					.values
					.iter()
					.map(|v| int_or_float(*v))
					.collect::<Result<Vec<_>>>()?;
				axis_element.set("values", values.join(" "));
			}
		}
		axis_element.set("default", int_or_float(axis.default())?);
		if axis.hidden() {
			axis_element.set("hidden", "1");
		}
		Ok(axis_element)
	}

	fn axis_mapping_element(&self, mapping: &AxisMappingDescriptor) -> Result<Element> {
		let mut mapping_element = Element::new("mapping");
		if let Some(description) = &mapping.description {
			mapping_element.set("description", description.clone());
		}
		for (tag, location) in [
			("input", &mapping.input_location),
			("output", &mapping.output_location),
		] {
			let mut what_element = Element::new(tag);
			for (name, value) in location {
				let mut dimension_element = Element::new("dimension");
				dimension_element.set("name", name.clone());
				dimension_element.set("xvalue", int_or_float(*value)?);
				what_element.append(dimension_element);
			}
			mapping_element.append(what_element);
		}
		Ok(mapping_element)
	}

	fn location_label_element(&self, label: &LocationLabelDescriptor) -> Result<Element> {
		let mut label_element = Element::new("label");
		label_element.set("name", label.name.clone());
		if label.elidable {
			label_element.set("elidable", "true");
		}
		if label.older_sibling {
			label_element.set("oldersibling", "true");
		}
		add_label_names(&mut label_element, &label.label_names);
		self.add_location_element(&mut label_element, None, Some(&label.user_location))?;
		Ok(label_element)
	}

	/// `_addLocationElement`: a format 5 location; per axis, the design value
	/// wins over the user value.
	fn add_location_element(
		&self,
		parent: &mut Element,
		design_location: Option<&AnisotropicLocation>,
		user_location: Option<&SimpleLocation>,
	) -> Result<()> {
		let mut loc_element = Element::new("location");
		for name in &self.axis_names {
			if let Some(value) = design_location.and_then(|d| d.get(name)) {
				loc_element.append(dimension_element(name, *value)?);
			} else if let Some(value) = user_location.and_then(|u| u.get(name)) {
				let mut dim_element = Element::new("dimension");
				dim_element.set("name", name.clone());
				dim_element.set("uservalue", int_or_float(*value)?);
				loc_element.append(dim_element);
			}
		}
		if !loc_element.is_empty() {
			parent.append(loc_element);
		}
		Ok(())
	}

	fn instance_element(&self, doc: &mut DesignSpaceDocument, index: usize) -> Result<Element> {
		let mut instance_element = Element::new("instance");
		let instance = &doc.instances[index];
		if let Some(name) = &instance.name {
			instance_element.set("name", name.clone());
		}
		if let Some(label) = &instance.location_label {
			instance_element.set("location", label.clone());
		}
		if let Some(name) = &instance.family_name {
			instance_element.set("familyname", name.clone());
		}
		if let Some(name) = &instance.style_name {
			instance_element.set("stylename", name.clone());
		}
		// add localisations
		for (tag, names) in [
			("stylename", &instance.localised_style_name),
			("familyname", &instance.localised_family_name),
			(
				"stylemapstylename",
				&instance.localised_style_map_style_name,
			),
			(
				"stylemapfamilyname",
				&instance.localised_style_map_family_name,
			),
		] {
			add_localised_names(&mut instance_element, tag, names);
		}

		if self.effective_format_tuple >= (5, 0) {
			if instance.location_label.is_none() {
				self.add_location_element(
					&mut instance_element,
					Some(&instance.design_location),
					Some(&instance.user_location),
				)?;
			}
		} else {
			// Pre-version 5.0 code was validating and filling in the location
			// dict while writing it out, as preserved below.
			let (location_element, validated) =
				self.make_location_element(&instance.design_location, None)?;
			instance_element.append(location_element);
			doc.instances[index].design_location = validated;
		}
		let instance = &doc.instances[index];
		if let Some(filename) = &instance.filename {
			instance_element.set("filename", filename.clone());
		}
		if let Some(name) = &instance.post_script_font_name {
			instance_element.set("postscriptfontname", name.clone());
		}
		if let Some(name) = &instance.style_map_family_name {
			instance_element.set("stylemapfamilyname", name.clone());
		}
		if let Some(name) = &instance.style_map_style_name {
			instance_element.set("stylemapstylename", name.clone());
		}
		if self.effective_format_tuple < (5, 0) {
			// Deprecated members as of version 5.0
			if !doc.instances[index].glyphs.is_empty() {
				let mut glyphs_element = Element::new("glyphs");
				let mut names: Vec<String> = doc.instances[index].glyphs.keys().cloned().collect();
				names.sort();
				for glyph_name in names {
					if let Some(data) = doc.instances[index].glyphs.get_mut(&glyph_name) {
						glyphs_element.append(self.write_glyph_element(&glyph_name, data)?);
					}
				}
				instance_element.append(glyphs_element);
			}
			let instance = &doc.instances[index];
			if instance.kerning {
				instance_element.append(Element::new("kerning"));
			}
			if instance.info {
				instance_element.append(Element::new("info"));
			}
		}
		add_lib(&mut instance_element, &doc.instances[index].lib, 4)?;
		Ok(instance_element)
	}

	fn source_element(&self, doc: &mut DesignSpaceDocument, index: usize) -> Result<Element> {
		let mut source_element = Element::new("source");
		let source = &doc.sources[index];
		if let Some(filename) = &source.filename {
			source_element.set("filename", filename.clone());
		}
		if let Some(name) = &source.name
			&& !name.starts_with("temp_master")
		{
			// do not save temporary source names
			source_element.set("name", name.clone());
		}
		if let Some(name) = &source.family_name {
			source_element.set("familyname", name.clone());
		}
		if let Some(name) = &source.style_name {
			source_element.set("stylename", name.clone());
		}
		if let Some(name) = &source.layer_name {
			source_element.set("layer", name.clone());
		}
		add_localised_names(
			&mut source_element,
			"familyname",
			&source.localised_family_name,
		);
		if source.copy_lib {
			let mut lib_element = Element::new("lib");
			lib_element.set("copy", "1");
			source_element.append(lib_element);
		}
		if source.copy_groups {
			let mut groups_element = Element::new("groups");
			groups_element.set("copy", "1");
			source_element.append(groups_element);
		}
		if source.copy_features {
			let mut features_element = Element::new("features");
			features_element.set("copy", "1");
			source_element.append(features_element);
		}
		if source.copy_info || source.mute_info {
			let mut info_element = Element::new("info");
			if source.copy_info {
				info_element.set("copy", "1");
			}
			if source.mute_info {
				info_element.set("mute", "1");
			}
			source_element.append(info_element);
		}
		if source.mute_kerning {
			let mut kerning_element = Element::new("kerning");
			kerning_element.set("mute", "1");
			source_element.append(kerning_element);
		}
		for name in &source.muted_glyph_names {
			let mut glyph_element = Element::new("glyph");
			glyph_element.set("name", name.clone());
			glyph_element.set("mute", "1");
			source_element.append(glyph_element);
		}
		if self.effective_format_tuple >= (5, 0) {
			self.add_location_element(&mut source_element, Some(&source.design_location), None)?;
		} else {
			// Pre-version 5.0 code was validating and filling in the location
			// dict while writing it out, as preserved below.
			let (location_element, validated) =
				self.make_location_element(&source.design_location, None)?;
			source_element.append(location_element);
			doc.sources[index].design_location = validated;
		}
		Ok(source_element)
	}

	fn variable_font_element(&self, vf: &VariableFontDescriptor) -> Result<Element> {
		let mut vf_element = Element::new("variable-font");
		vf_element.set("name", vf.name.clone());
		if let Some(filename) = &vf.filename {
			vf_element.set("filename", filename.clone());
		}
		if !vf.axis_subsets.is_empty() {
			let mut subsets_element = Element::new("axis-subsets");
			for subset in &vf.axis_subsets {
				let mut subset_element = Element::new("axis-subset");
				subset_element.set("name", subset.name());
				match subset {
					AxisSubset::Range(s) => {
						if s.user_minimum != Number::Float(f64::NEG_INFINITY) {
							subset_element.set("userminimum", int_or_float(s.user_minimum)?);
						}
						if s.user_maximum != Number::Float(f64::INFINITY) {
							subset_element.set("usermaximum", int_or_float(s.user_maximum)?);
						}
						if let Some(user_default) = s.user_default {
							subset_element.set("userdefault", int_or_float(user_default)?);
						}
					}
					AxisSubset::Value(s) => {
						subset_element.set("uservalue", int_or_float(s.user_value)?);
					}
				}
				subsets_element.append(subset_element);
			}
			vf_element.append(subsets_element);
		}
		add_lib(&mut vf_element, &vf.lib, 4)?;
		Ok(vf_element)
	}

	/// `_writeGlyphElement`; the locations are filled in like format 4
	/// locations.
	fn write_glyph_element(&self, glyph_name: &str, data: &mut GlyphData) -> Result<Element> {
		let mut glyph_element = Element::new("glyph");
		if data.mute {
			glyph_element.set("mute", "1");
		}
		if let Some(unicodes) = &data.unicodes {
			let hex: Vec<String> = unicodes
				.iter()
				.map(|&u| {
					if u < 0 {
						format!("-0x{:x}", u.unsigned_abs())
					} else {
						format!("0x{u:x}")
					}
				})
				.collect();
			glyph_element.set("unicode", hex.join(" "));
		}
		if let Some(location) = &data.instance_location {
			let (location_element, validated) = self.make_location_element(location, None)?;
			glyph_element.append(location_element);
			data.instance_location = Some(validated);
		}
		glyph_element.set("name", glyph_name);
		if let Some(note) = &data.note {
			let mut note_element = Element::new("note");
			note_element.text = Some(note.clone());
			glyph_element.append(note_element);
		}
		if let Some(masters) = &mut data.masters {
			let mut masters_element = Element::new("masters");
			for m in masters {
				let mut master_element = Element::new("master");
				if let Some(glyph_name) = &m.glyph_name {
					master_element.set("glyphname", glyph_name.clone());
				}
				if let Some(font) = &m.font {
					master_element.set("source", font.clone());
				}
				if let Some(location) = &m.location {
					let (location_element, validated) =
						self.make_location_element(location, None)?;
					master_element.append(location_element);
					m.location = Some(validated);
				}
				masters_element.append(master_element);
			}
			glyph_element.append(masters_element);
		}
		Ok(glyph_element)
	}
}

fn dimension_element(name: &str, value: LocationValue) -> Result<Element> {
	let mut dim_element = Element::new("dimension");
	dim_element.set("name", name);
	match value {
		LocationValue::Anisotropic(x, y) => {
			dim_element.set("xvalue", int_or_float(x)?);
			dim_element.set("yvalue", int_or_float(y)?);
		}
		LocationValue::Value(v) => dim_element.set("xvalue", int_or_float(v)?),
	}
	Ok(dim_element)
}

fn axis_label_element(label: &AxisLabelDescriptor) -> Result<Element> {
	let mut label_element = Element::new("label");
	label_element.set("uservalue", int_or_float(label.user_value)?);
	if let Some(v) = label.user_minimum {
		label_element.set("userminimum", int_or_float(v)?);
	}
	if let Some(v) = label.user_maximum {
		label_element.set("usermaximum", int_or_float(v)?);
	}
	label_element.set("name", label.name.clone());
	if label.elidable {
		label_element.set("elidable", "true");
	}
	if label.older_sibling {
		label_element.set("oldersibling", "true");
	}
	if let Some(v) = label.linked_user_value {
		label_element.set("linkeduservalue", int_or_float(v)?);
	}
	add_label_names(&mut label_element, &label.label_names);
	Ok(label_element)
}

/// `_addLabelNames`: `labelname` elements sorted by language.
fn add_label_names(parent: &mut Element, label_names: &LocalizedNames) {
	let mut items: Vec<(&String, &String)> = label_names.iter().collect();
	items.sort();
	for (language_code, label_name) in items {
		let mut language_element = Element::new("labelname");
		language_element.set(xml_lang(), language_code.clone());
		language_element.text = Some(label_name.clone());
		parent.append(language_element);
	}
}

/// The localised names of sources and instances, sorted by language, without
/// English (stored in the attribute).
fn add_localised_names(parent: &mut Element, tag: &str, names: &LocalizedNames) {
	let mut codes: Vec<&String> = names.keys().collect();
	codes.sort();
	for code in codes {
		if code == "en" {
			continue; // already stored in the element attribute
		}
		let mut element = Element::new(tag);
		element.set(xml_lang(), code.clone());
		element.text = names.get(code).cloned();
		parent.append(element);
	}
}

/// `_addLib`.
fn add_lib(parent: &mut Element, data: &plistlib::Dict, indent_level: usize) -> Result<()> {
	if data.is_empty() {
		return Ok(());
	}
	let mut lib_element = Element::new("lib");
	lib_element.append(plistlib::totree(
		&plistlib::Value::Dict(data.clone()),
		plistlib::TreeOptions {
			indent_level,
			..Default::default()
		},
	)?);
	parent.append(lib_element);
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn int_or_float_formats() -> Result<()> {
		assert_eq!(int_or_float(Number::Float(1.0))?, "1");
		assert_eq!(int_or_float(Number::Float(-0.0))?, "0");
		assert_eq!(int_or_float(Number::Float(0.5))?, "0.5");
		assert_eq!(int_or_float(Number::Float(0.1234567))?, "0.123457");
		assert_eq!(int_or_float(Number::Float(-1e-7))?, "-0");
		assert_eq!(int_or_float(Number::Float(1e20))?, "100000000000000000000");
		assert!(int_or_float(Number::Float(f64::INFINITY)).is_err());
		Ok(())
	}
}
