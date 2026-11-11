//! `BaseDocReader`: reads format 3 to 5 documents.

use indexmap::IndexMap;

use fontoxide_misc::plistlib;
use fontoxide_misc::py;
use fontoxide_misc::xml_reader::{self, Element};

use crate::descriptors::{
	AnisotropicLocation, Axis, AxisDescriptor, AxisLabelDescriptor, AxisMappingDescriptor,
	AxisSubset, Condition, DiscreteAxisDescriptor, GlyphData, GlyphMaster, InstanceDescriptor,
	LocationLabelDescriptor, LocationValue, RangeAxisSubsetDescriptor, RuleDescriptor,
	SimpleLocation, SourceDescriptor, ValueAxisSubsetDescriptor, VariableFontDescriptor,
};
use crate::document::DesignSpaceDocument;
use crate::number::Number;
use crate::paths;
use crate::{DesignSpaceDocumentError, Result};

const XML_LANG: &str = "xml:lang";

fn err(msg: impl Into<String>) -> DesignSpaceDocumentError {
	DesignSpaceDocumentError::Document(msg.into())
}

/// `element.findall(name)`: the child elements with this name.
fn children<'a>(element: &'a Element, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
	element.elements().filter(move |e| e.name == name)
}

/// `element.find(name)`.
fn find<'a>(element: &'a Element, name: &str) -> Option<&'a Element> {
	element.elements().find(|e| e.name == name)
}

/// `element.text`.
fn text(element: &Element) -> Option<String> {
	plistlib::element_text(element)
}

/// The `xml:lang` attribute values of an element (`for key, lang in
/// element.items(): if key == XML_LANG`).
fn langs(element: &Element) -> impl Iterator<Item = &str> {
	element
		.attrs
		.iter()
		.filter(|(k, _)| k == XML_LANG)
		.map(|(_, v)| v.as_str())
}

/// `float(value)`.
fn float(value: &str) -> Result<Number> {
	py::parse_float(value).map(Number::Float).ok_or_else(|| {
		DesignSpaceDocumentError::Value(format!(
			"could not convert string to float: {}",
			py::str_repr(value)
		))
	})
}

/// `float(element.attrib.get(name))`: a missing attribute is a `TypeError`.
fn float_attr(element: &Element, name: &str) -> Result<Number> {
	match element.attr(name) {
		Some(v) => float(v),
		None => Err(DesignSpaceDocumentError::Value(
			"float() argument must be a string or a real number, not 'NoneType'".into(),
		)),
	}
}

/// `element.attrib[name]`.
fn required_attr<'a>(element: &'a Element, name: &str) -> Result<&'a str> {
	element
		.attr(name)
		.ok_or_else(|| DesignSpaceDocumentError::Value(format!("KeyError: '{name}'")))
}

/// `int(value, 16)`.
fn parse_hex(s: &str) -> Option<i64> {
	let t = s.trim();
	let (negative, t) = match t.strip_prefix('-') {
		Some(rest) => (true, rest),
		None => (false, t.strip_prefix('+').unwrap_or(t)),
	};
	let t = t
		.strip_prefix("0x")
		.or_else(|| t.strip_prefix("0X"))
		.map_or(t, |rest| rest.strip_prefix('_').unwrap_or(rest));
	if t.is_empty() || t.starts_with('_') || t.ends_with('_') || t.contains("__") {
		return None;
	}
	let v = i64::from_str_radix(&t.replace('_', ""), 16).ok()?;
	Some(if negative { -v } else { v })
}

fn check_unknown_attrs(element: &Element, known: &[&str], what: &str) -> Result<()> {
	let unknown: Vec<&str> = element
		.attrs
		.iter()
		.map(|(k, _)| k.as_str())
		.filter(|k| !known.contains(k))
		.collect();
	if !unknown.is_empty() {
		return Err(err(format!(
			"{what} element contains unknown attributes: {}",
			unknown.join(", ")
		)));
	}
	Ok(())
}

/// Python's dict comprehension over the `labelname` children.
fn label_names(element: &Element) -> IndexMap<String, String> {
	let mut names = IndexMap::new();
	for label_name in children(element, "labelname") {
		for lang in langs(label_name) {
			names.insert(lang.to_owned(), text(label_name).unwrap_or_default());
		}
	}
	names
}

struct Reader<'a> {
	path: Option<&'a str>,
	doc: &'a mut DesignSpaceDocument,
	root: &'a Element,
	axis_defaults: IndexMap<Option<String>, Number>,
	strict_axis_names: bool,
}

/// Reads the document text into `doc`; `path` is the document path, used to
/// resolve the source paths.
pub(crate) fn read(doc: &mut DesignSpaceDocument, text: &str, path: Option<&str>) -> Result<()> {
	let text = text.strip_prefix('\u{feff}').unwrap_or(text);
	let root = xml_reader::parse(text).map_err(|e| DesignSpaceDocumentError::Xml(e.0))?;
	doc.format_version = Some(root.attr("format").unwrap_or("3.0").to_owned());
	let mut reader = Reader {
		path,
		doc,
		root: &root,
		axis_defaults: IndexMap::new(),
		strict_axis_names: true,
	};
	reader.read_axes()?;
	reader.read_labels()?;
	reader.read_rules()?;
	reader.read_variable_fonts()?;
	reader.read_sources()?;
	reader.read_instances()?;
	reader.read_lib()?;
	Ok(())
}

fn lib_dict(element: &Element) -> Result<plistlib::Dict> {
	match plistlib::fromtree(element)? {
		plistlib::Value::Dict(d) => Ok(d),
		// LENIENT(L31): fontTools stores any plist value as the lib; the descriptors
		// here only hold dictionaries.
		other => Err(err(format!(
			"lib element must contain a dict, not {other:?}"
		))),
	}
}

impl Reader<'_> {
	fn read_rules(&mut self) -> Result<()> {
		// we also need to read any conditions that are outside of a condition set.
		let mut rules = Vec::new();
		if let Some(rules_element) = find(self.root, "rules") {
			let processing_value = rules_element.attr("processing").unwrap_or("first");
			if processing_value != "first" && processing_value != "last" {
				return Err(err(format!(
					"<rules> processing attribute value is not valid: {}, expected 'first' or 'last'",
					py::str_repr(processing_value)
				)));
			}
			self.doc.rules_processing_last = processing_value == "last";
		}
		for rule_element in children(self.root, "rules").flat_map(|r| children(r, "rule")) {
			let mut rule = RuleDescriptor {
				name: rule_element.attr("name").map(str::to_owned),
				..Default::default()
			};
			let rule_name = rule.name.as_deref();
			// read any stray conditions outside a condition set
			let external_conditions = read_condition_elements(rule_element, rule_name)?;
			if !external_conditions.is_empty() {
				rule.condition_sets.push(external_conditions);
				log::info!(
					"Found stray rule conditions outside a conditionset. Wrapped them in a new conditionset."
				);
			}
			// read the conditionsets
			for condition_set_element in children(rule_element, "conditionset") {
				rule.condition_sets
					.push(read_condition_elements(condition_set_element, rule_name)?);
			}
			for sub_element in children(rule_element, "sub") {
				let a = required_attr(sub_element, "name")?;
				let b = required_attr(sub_element, "with")?;
				rule.subs.push((a.to_owned(), b.to_owned()));
			}
			rules.push(rule);
		}
		self.doc.rules = rules;
		Ok(())
	}

	fn read_axes(&mut self) -> Result<()> {
		// read the axes elements, including the warp map.
		let axes_element = find(self.root, "axes");
		if let Some(name) = axes_element.and_then(|a| a.attr("elidedfallbackname")) {
			self.doc.elided_fallback_name = Some(name.to_owned());
		}
		let axis_elements: Vec<&Element> = children(self.root, "axes")
			.flat_map(|a| children(a, "axis"))
			.collect();
		if axis_elements.is_empty() {
			return Ok(());
		}
		for axis_element in axis_elements {
			let format_tuple = self.doc.format_tuple()?;
			let discrete_values = match axis_element.attr("values") {
				Some(values) if format_tuple >= (5, 0) => Some(
					values
						.split(' ')
						.map(float)
						.collect::<Result<Vec<Number>>>()?,
				),
				_ => None,
			};
			let (minimum, maximum) = if discrete_values.is_none() {
				(
					float_attr(axis_element, "minimum")?,
					float_attr(axis_element, "maximum")?,
				)
			} else {
				(Number::default(), Number::default())
			};
			let default = float_attr(axis_element, "default")?;
			let name = axis_element.attr("name").map(str::to_owned);
			let hidden = axis_element.attr("hidden").is_some_and(|h| !h.is_empty());
			let tag = axis_element.attr("tag").map(str::to_owned);
			let mut map = Vec::new();
			for map_element in children(axis_element, "map") {
				let a = float(required_attr(map_element, "input")?)?;
				let b = float(required_attr(map_element, "output")?)?;
				map.push((a, b));
			}
			let mut label_names = IndexMap::new();
			for label_name_element in children(axis_element, "labelname") {
				for lang in langs(label_name_element) {
					// LENIENT(L31): an empty element is an AttributeError in fontTools
					// (`tostr(None)`); here it is an empty name.
					label_names.insert(
						lang.to_owned(),
						text(label_name_element).unwrap_or_default(),
					);
				}
			}
			let mut axis_ordering = None;
			let mut axis_labels = Vec::new();
			if let Some(labels_element) = find(axis_element, "labels") {
				if let Some(ordering) = labels_element.attr("ordering") {
					axis_ordering = Some(
						py::parse_int(ordering)
							.and_then(|v| i64::try_from(v).ok())
							.ok_or_else(|| {
								DesignSpaceDocumentError::Value(format!(
									"invalid literal for int() with base 10: {}",
									py::str_repr(ordering)
								))
							})?,
					);
				}
				for label in children(labels_element, "label") {
					axis_labels.push(read_axis_label(label)?);
				}
			}
			let axis = match discrete_values {
				Some(values) => Axis::Discrete(DiscreteAxisDescriptor {
					tag,
					name: name.clone(),
					label_names,
					hidden,
					map,
					axis_ordering,
					axis_labels,
					values,
					default,
				}),
				None => Axis::Continuous(AxisDescriptor {
					tag,
					name: name.clone(),
					label_names,
					hidden,
					map,
					axis_ordering,
					axis_labels,
					minimum,
					maximum,
					default,
				}),
			};
			self.doc.axes.push(axis);
			self.axis_defaults.insert(name, default);
		}

		self.doc.axis_mappings = Vec::new();
		for mappings_element in children(self.root, "axes").flat_map(|a| children(a, "mappings")) {
			let group_description = mappings_element.attr("description").map(str::to_owned);
			for mapping_element in children(mappings_element, "mapping") {
				let description = mapping_element.attr("description").map(str::to_owned);
				let no_attr = |what: &str| {
					DesignSpaceDocumentError::Value(format!(
						"'NoneType' object has no attribute 'findall' (missing <{what}>)"
					))
				};
				let input_element =
					find(mapping_element, "input").ok_or_else(|| no_attr("input"))?;
				let output_element =
					find(mapping_element, "output").ok_or_else(|| no_attr("output"))?;
				let mut input_location = SimpleLocation::new();
				let mut output_location = SimpleLocation::new();
				for dim_element in children(input_element, "dimension") {
					let name = required_attr(dim_element, "name")?;
					let value = float(required_attr(dim_element, "xvalue")?)?;
					input_location.insert(name.to_owned(), value);
				}
				for dim_element in children(output_element, "dimension") {
					let name = required_attr(dim_element, "name")?;
					let value = float(required_attr(dim_element, "xvalue")?)?;
					output_location.insert(name.to_owned(), value);
				}
				self.doc.axis_mappings.push(AxisMappingDescriptor {
					input_location,
					output_location,
					description,
					group_description: group_description.clone(),
				});
			}
		}
		Ok(())
	}

	fn read_labels(&mut self) -> Result<()> {
		if self.doc.format_tuple()? < (5, 0) {
			return Ok(());
		}
		for label_element in children(self.root, "labels").flat_map(|l| children(l, "label")) {
			check_unknown_attrs(
				label_element,
				&["name", "elidable", "oldersibling"],
				"Label",
			)?;
			let name = label_element
				.attr("name")
				.ok_or_else(|| err("label element must have a name attribute."))?;
			let (design_location, user_location) = self.location_from_element(label_element)?;
			if design_location.is_some_and(|d| !d.is_empty()) {
				return Err(err(format!(
					"<label> element \"{name}\" must only have user locations (using uservalue=\"\")."
				)));
			}
			self.doc.location_labels.push(LocationLabelDescriptor {
				name: name.to_owned(),
				user_location: user_location.unwrap_or_default(),
				elidable: label_element.attr("elidable") == Some("true"),
				older_sibling: label_element.attr("oldersibling") == Some("true"),
				label_names: label_names(label_element),
			});
		}
		Ok(())
	}

	fn read_variable_fonts(&mut self) -> Result<()> {
		if self.doc.format_tuple()? < (5, 0) {
			return Ok(());
		}
		for vf_element in
			children(self.root, "variable-fonts").flat_map(|v| children(v, "variable-font"))
		{
			check_unknown_attrs(vf_element, &["name", "filename"], "variable-font")?;
			let name = vf_element
				.attr("name")
				.ok_or_else(|| err("variable-font element must have a name attribute."))?;
			let filename = vf_element.attr("filename");
			let axis_subsets_element = find(vf_element, "axis-subsets").ok_or_else(|| {
				err("variable-font element must contain an axis-subsets element.")
			})?;
			let mut axis_subsets = Vec::new();
			for axis_subset in children(axis_subsets_element, "axis-subset") {
				axis_subsets.push(read_axis_subset(axis_subset)?);
			}
			let lib = match find(vf_element, "lib") {
				Some(lib_element) => match lib_element.elements().next() {
					Some(first) => lib_dict(first)?,
					None => {
						return Err(DesignSpaceDocumentError::Value(
							"IndexError: list index out of range".into(),
						));
					}
				},
				None => plistlib::Dict::new(),
			};
			let mut vf = VariableFontDescriptor {
				name: name.to_owned(),
				filename: None,
				axis_subsets,
				lib,
			};
			vf.set_filename(filename);
			self.doc.variable_fonts.push(vf);
		}
		Ok(())
	}

	fn read_sources(&mut self) -> Result<()> {
		let source_elements: Vec<&Element> = children(self.root, "sources")
			.flat_map(|s| children(s, "source"))
			.collect();
		for (source_count, source_element) in source_elements.into_iter().enumerate() {
			let filename = source_element.attr("filename");
			let source_path = match (filename, self.path) {
				(Some(filename), Some(path)) => Some(paths::abspath_str(&paths::join(
					&paths::dirname(path),
					filename,
				))?),
				_ => None,
			};
			let source_name = match source_element.attr("name") {
				Some(name) => name.to_owned(),
				// add a temporary source name
				None => format!("temp_master.{source_count}"),
			};
			let mut source = SourceDescriptor::new();
			source.set_path(source_path.as_deref());
			source.set_filename(filename);
			source.name = Some(source_name.clone());
			if let Some(family_name) = source_element.attr("familyname") {
				source.family_name = Some(family_name.to_owned());
			}
			if let Some(style_name) = source_element.attr("stylename") {
				source.style_name = Some(style_name.to_owned());
			}
			for family_name_element in children(source_element, "familyname") {
				for lang in langs(family_name_element) {
					// LENIENT(L31): no text is an error in fontTools (`tostr(None)`).
					source.set_family_name(&text(family_name_element).unwrap_or_default(), lang);
				}
			}
			let (design_location, user_location) = self.location_from_element(source_element)?;
			if user_location.is_some_and(|u| !u.is_empty()) {
				return Err(err(format!(
					"<source> element \"{source_name}\" must only have design locations (using xvalue=\"\")."
				)));
			}
			source.design_location = design_location.unwrap_or_default();
			if let Some(layer_name) = source_element.attr("layer") {
				source.layer_name = Some(layer_name.to_owned());
			}
			for lib_element in children(source_element, "lib") {
				if lib_element.attr("copy") == Some("1") {
					source.copy_lib = true;
				}
			}
			for groups_element in children(source_element, "groups") {
				if groups_element.attr("copy") == Some("1") {
					source.copy_groups = true;
				}
			}
			for info_element in children(source_element, "info") {
				if info_element.attr("copy") == Some("1") {
					source.copy_info = true;
				}
				if info_element.attr("mute") == Some("1") {
					source.mute_info = true;
				}
			}
			for features_element in children(source_element, "features") {
				if features_element.attr("copy") == Some("1") {
					source.copy_features = true;
				}
			}
			for glyph_element in children(source_element, "glyph") {
				let Some(glyph_name) = glyph_element.attr("name") else {
					continue;
				};
				if glyph_element.attr("mute") == Some("1") {
					source.muted_glyph_names.push(glyph_name.to_owned());
				}
			}
			for kerning_element in children(source_element, "kerning") {
				if kerning_element.attr("mute") == Some("1") {
					source.mute_kerning = true;
				}
			}
			self.doc.sources.push(source);
		}
		Ok(())
	}

	/// `locationFromElement`: the first `<location>` child as `(designLocation,
	/// userLocation)`, both `None` without one.
	fn location_from_element(
		&self,
		element: &Element,
	) -> Result<(Option<AnisotropicLocation>, Option<SimpleLocation>)> {
		match find(element, "location") {
			Some(location_element) => {
				let (design, user) = self.read_location_element(location_element)?;
				Ok((Some(design), Some(user)))
			}
			None => Ok((None, None)),
		}
	}

	/// `readLocationElement`.
	fn read_location_element(
		&self,
		location_element: &Element,
	) -> Result<(AnisotropicLocation, SimpleLocation)> {
		if self.strict_axis_names && self.doc.axes.is_empty() {
			return Err(err("No axes defined"));
		}
		let mut user_loc = SimpleLocation::new();
		let mut design_loc = AnisotropicLocation::new();
		for dimension_element in children(location_element, "dimension") {
			let dim_name = dimension_element.attr("name");
			if self.strict_axis_names
				&& !self
					.axis_defaults
					.contains_key(&dim_name.map(str::to_owned))
			{
				// In case the document contains no axis definitions,
				log::warn!(
					"Location with undefined axis: \"{}\".",
					dim_name.unwrap_or("None")
				);
				continue;
			}
			let dim_name = dim_name.unwrap_or("None");
			// LENIENT(L31): fontTools logs a warning for a value that is not a number
			// but keeps the string; here the value is dropped.
			let parse = |name: &str| -> Option<Number> {
				let raw = dimension_element.attr(name)?;
				match float(raw) {
					Ok(v) => Some(v),
					Err(_) => {
						log::warn!("ValueError in readLocation {name} {raw}");
						None
					}
				}
			};
			let user_value = parse("uservalue");
			let x_value = parse("xvalue");
			let y_value = parse("yvalue");
			// UPSTREAM-BUG(B30): `userValue is None == xValue is None` is a chained
			// comparison that is only true when both are missing, so a dimension
			// with both values is accepted (the design value wins).
			if user_value.is_none() && x_value.is_none() {
				return Err(err(format!(
					"Exactly one of uservalue=\"\" or xvalue=\"\" must be provided for location dimension \"{dim_name}\""
				)));
			}
			if let Some(y) = y_value {
				let Some(x) = x_value else {
					return Err(err(format!(
						"Missing xvalue=\"\" for the location dimension \"{dim_name}\"\" with yvalue=\"{y}\""
					)));
				};
				design_loc.insert(dim_name.to_owned(), LocationValue::Anisotropic(x, y));
			} else if let Some(x) = x_value {
				design_loc.insert(dim_name.to_owned(), LocationValue::Value(x));
			} else if let Some(u) = user_value {
				user_loc.insert(dim_name.to_owned(), u);
			}
		}
		Ok((design_loc, user_loc))
	}

	fn read_instances(&mut self) -> Result<()> {
		let instance_elements: Vec<&Element> = children(self.root, "instances")
			.flat_map(|s| children(s, "instance"))
			.collect();
		for instance_element in instance_elements {
			self.read_single_instance_element(instance_element)?;
		}
		Ok(())
	}

	fn read_single_instance_element(&mut self, instance_element: &Element) -> Result<()> {
		let filename = instance_element.attr("filename");
		let instance_path = match (filename, &self.doc.path) {
			(Some(filename), Some(path)) => Some(paths::join(&paths::dirname(path), filename)),
			_ => None,
		};
		let mut instance = InstanceDescriptor::new();
		instance.set_path(instance_path.as_deref());
		instance.set_filename(filename);
		let get = |name: &str| instance_element.attr(name).map(str::to_owned);
		instance.name = get("name");
		instance.family_name = get("familyname");
		instance.style_name = get("stylename");
		instance.post_script_font_name = get("postscriptfontname");
		instance.style_map_family_name = get("stylemapfamilyname");
		instance.style_map_style_name = get("stylemapstylename");
		// read localised names
		// LENIENT(L31): elements without text are errors in fontTools (`tostr(None)`).
		for element in children(instance_element, "stylename") {
			for lang in langs(element) {
				instance.set_style_name(&text(element).unwrap_or_default(), lang);
			}
		}
		for element in children(instance_element, "familyname") {
			for lang in langs(element) {
				instance.set_family_name(&text(element).unwrap_or_default(), lang);
			}
		}
		for element in children(instance_element, "stylemapstylename") {
			for lang in langs(element) {
				instance.set_style_map_style_name(&text(element).unwrap_or_default(), lang);
			}
		}
		for element in children(instance_element, "stylemapfamilyname") {
			for lang in langs(element) {
				instance.set_style_map_family_name(&text(element).unwrap_or_default(), lang);
			}
		}
		let (design_location, user_location) = self.location_from_element(instance_element)?;
		let location_label = instance_element.attr("location");
		let has_location = design_location.as_ref().is_some_and(|d| !d.is_empty())
			|| user_location.as_ref().is_some_and(|u| !u.is_empty());
		if has_location && location_label.is_some() {
			return Err(err(
				"instance element must have at most one of the location=\"...\" attribute or the nested location element",
			));
		}
		instance.location_label = location_label.map(str::to_owned);
		instance.user_location = user_location.unwrap_or_default();
		instance.design_location = design_location.unwrap_or_default();
		for glyph_element in children(instance_element, "glyphs").flat_map(|g| children(g, "glyph"))
		{
			self.read_glyph_element(glyph_element, &mut instance)?;
		}
		for _info_element in children(instance_element, "info") {
			instance.info = true;
		}
		for lib_element in children(instance_element, "lib") {
			// an empty <lib> element is equivalent to no lib at all
			if let Some(first) = lib_element.elements().next() {
				instance.lib = lib_dict(first)?;
			}
		}
		self.doc.instances.push(instance);
		Ok(())
	}

	/// `readGlyphElement`.
	fn read_glyph_element(
		&self,
		glyph_element: &Element,
		instance: &mut InstanceDescriptor,
	) -> Result<()> {
		let mut glyph_data = GlyphData::default();
		let glyph_name = glyph_element
			.attr("name")
			.ok_or_else(|| err("Glyph object without name attribute"))?;
		if glyph_element.attr("mute") == Some("1") {
			glyph_data.mute = true;
		}
		if let Some(unicodes) = glyph_element.attr("unicode") {
			let values: Option<Vec<i64>> = unicodes.split(' ').map(parse_hex).collect();
			match values {
				Some(values) => glyph_data.unicodes = Some(values),
				None => {
					return Err(err(format!("unicode values {unicodes} are not integers")));
				}
			}
		}
		if let Some(note_element) = find(glyph_element, "note") {
			glyph_data.note = text(note_element);
		}
		let (design_location, user_location) = self.location_from_element(glyph_element)?;
		if user_location.is_some_and(|u| !u.is_empty()) {
			return Err(err(format!(
				"<glyph> element \"{glyph_name}\" must only have design locations (using xvalue=\"\")."
			)));
		}
		glyph_data.instance_location = design_location;
		let mut glyph_sources: Option<Vec<GlyphMaster>> = None;
		for master_element in children(glyph_element, "masters").flat_map(|m| children(m, "master"))
		{
			let font_source_name = master_element.attr("source");
			let (design_location, user_location) = self.location_from_element(master_element)?;
			if user_location.is_some_and(|u| !u.is_empty()) {
				return Err(err(format!(
					"<master> element \"{}\" must only have design locations (using xvalue=\"\").",
					font_source_name.unwrap_or("None")
				)));
			}
			// if we don't read a glyphname, use the one we have
			let master_glyph_name = master_element.attr("glyphname").unwrap_or(glyph_name);
			glyph_sources
				.get_or_insert_with(Vec::new)
				.push(GlyphMaster {
					font: font_source_name.map(str::to_owned),
					location: design_location,
					glyph_name: Some(master_glyph_name.to_owned()),
				});
		}
		glyph_data.masters = glyph_sources;
		instance.glyphs.insert(glyph_name.to_owned(), glyph_data);
		Ok(())
	}

	/// `readLib`: the document lib.
	fn read_lib(&mut self) -> Result<()> {
		for lib_element in children(self.root, "lib") {
			// an empty <lib> element is equivalent to no lib at all
			if let Some(first) = lib_element.elements().next() {
				self.doc.lib = lib_dict(first)?;
			}
		}
		Ok(())
	}
}

fn read_condition_elements(parent: &Element, rule_name: Option<&str>) -> Result<Vec<Condition>> {
	let mut cds = Vec::new();
	for condition_element in children(parent, "condition") {
		let minimum = match condition_element.attr("minimum") {
			Some(v) => Some(float(v)?),
			// will allow these to be None, assume axis.minimum
			None => None,
		};
		let maximum = match condition_element.attr("maximum") {
			Some(v) => Some(float(v)?),
			// will allow these to be None, assume axis.maximum
			None => None,
		};
		if minimum.is_none() && maximum.is_none() {
			return Err(err(format!(
				"condition missing required minimum or maximum in rule{}",
				rule_name.map_or(String::new(), |n| format!(" '{n}'"))
			)));
		}
		// LENIENT(L31): fontTools keeps a condition without name as `None` and fails
		// when it is used; here it is an error when reading.
		let name = condition_element
			.attr("name")
			.ok_or_else(|| err("condition element must have a name attribute."))?;
		cds.push(Condition {
			name: name.to_owned(),
			minimum,
			maximum,
		});
	}
	Ok(cds)
}

/// `readAxisLabel`.
fn read_axis_label(element: &Element) -> Result<AxisLabelDescriptor> {
	check_unknown_attrs(
		element,
		&[
			"userminimum",
			"uservalue",
			"usermaximum",
			"name",
			"elidable",
			"oldersibling",
			"linkeduservalue",
		],
		"label",
	)?;
	let name = element
		.attr("name")
		.ok_or_else(|| err("label element must have a name attribute."))?;
	let value_str = element
		.attr("uservalue")
		.ok_or_else(|| err("label element must have a uservalue attribute."))?;
	let opt = |name: &str| element.attr(name).map(float).transpose();
	Ok(AxisLabelDescriptor {
		user_minimum: opt("userminimum")?,
		user_value: float(value_str)?,
		user_maximum: opt("usermaximum")?,
		name: name.to_owned(),
		elidable: element.attr("elidable") == Some("true"),
		older_sibling: element.attr("oldersibling") == Some("true"),
		linked_user_value: opt("linkeduservalue")?,
		label_names: label_names(element),
	})
}

/// `readAxisSubset`.
fn read_axis_subset(element: &Element) -> Result<AxisSubset> {
	if let Some(user_value) = element.attr("uservalue") {
		check_unknown_attrs(element, &["name", "uservalue"], "axis-subset")?;
		let name = element
			.attr("name")
			.ok_or_else(|| err("axis-subset element must have a name attribute."))?;
		return Ok(AxisSubset::Value(ValueAxisSubsetDescriptor {
			name: name.to_owned(),
			user_value: float(user_value)?,
		}));
	}
	check_unknown_attrs(
		element,
		&["name", "userminimum", "userdefault", "usermaximum"],
		"axis-subset",
	)?;
	let name = element
		.attr("name")
		.ok_or_else(|| err("axis-subset element must have a name attribute."))?;
	// Leave omitted fields at the descriptor defaults; getVFUserRegion resolves
	// them against the full axis bounds and default.
	let mut subset = RangeAxisSubsetDescriptor::new(name);
	if let Some(v) = element.attr("userminimum") {
		subset.user_minimum = float(v)?;
	}
	if let Some(v) = element.attr("userdefault") {
		subset.user_default = Some(float(v)?);
	}
	if let Some(v) = element.attr("usermaximum") {
		subset.user_maximum = float(v)?;
	}
	Ok(AxisSubset::Range(subset))
}
