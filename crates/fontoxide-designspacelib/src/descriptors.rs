//! The descriptors: plain containers for the data of a designspace document.

use indexmap::IndexMap;

use fontoxide_misc::plistlib;

use crate::document::DesignSpaceDocument;
use crate::models::piecewise_linear_map;
use crate::number::Number;
use crate::paths::posix;
use crate::{DesignSpaceDocumentError, Result};

/// `SimpleLocationDict`: axis name to value.
pub type SimpleLocation = IndexMap<String, Number>;

/// `AnisotropicLocationDict`: axis name to value or `(x, y)` pair.
pub type AnisotropicLocation = IndexMap<String, LocationValue>;

/// Localized names, keyed by language code.
pub type LocalizedNames = IndexMap<String, String>;

/// A value of an [`AnisotropicLocation`]: `float` or `Tuple[float, float]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LocationValue {
	/// A single value.
	Value(Number),
	/// An anisotropic `(x, y)` value.
	Anisotropic(Number, Number),
}

impl LocationValue {
	/// The value, or the x value of an anisotropic one (`value[0]` if tuple).
	pub fn x(self) -> Number {
		match self {
			LocationValue::Value(v) | LocationValue::Anisotropic(v, _) => v,
		}
	}
}

impl From<Number> for LocationValue {
	fn from(v: Number) -> Self {
		LocationValue::Value(v)
	}
}

impl From<i32> for LocationValue {
	fn from(v: i32) -> Self {
		LocationValue::Value(v.into())
	}
}

impl From<i64> for LocationValue {
	fn from(v: i64) -> Self {
		LocationValue::Value(v.into())
	}
}

impl From<f64> for LocationValue {
	fn from(v: f64) -> Self {
		LocationValue::Value(v.into())
	}
}

impl<A: Into<Number>, B: Into<Number>> From<(A, B)> for LocationValue {
	fn from((x, y): (A, B)) -> Self {
		LocationValue::Anisotropic(x.into(), y.into())
	}
}

/// A simple location as an anisotropic one.
pub fn to_anisotropic(location: &SimpleLocation) -> AnisotropicLocation {
	location
		.iter()
		.map(|(k, v)| (k.clone(), LocationValue::Value(*v)))
		.collect()
}

/// `SourceDescriptor`: a source (master) of the designspace.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourceDescriptor {
	/// A relative path to the source file, as it is in the document
	/// (`filename`). Stored with forward slashes ([`posix`]); use
	/// [`set_filename`](Self::set_filename) to keep it that way.
	pub filename: Option<String>,
	/// The absolute path, calculated from the filename (`path`), with forward
	/// slashes.
	pub path: Option<String>,
	/// Unique identifier name for this source (`name`).
	pub name: Option<String>,
	/// Axis values for this source, in design space coordinates
	/// (`designLocation`, alias `location`). May be only part of the full
	/// location.
	pub design_location: AnisotropicLocation,
	/// The layer in the source to look for outline data (`layerName`).
	pub layer_name: Option<String>,
	/// Family name of this source (`familyName`).
	pub family_name: Option<String>,
	/// Style name of this source (`styleName`).
	pub style_name: Option<String>,
	/// Localised family names, keyed by language code (`localisedFamilyName`).
	pub localised_family_name: LocalizedNames,
	/// Copy the font lib to the instances (`copyLib`, deprecated).
	pub copy_lib: bool,
	/// Copy the non-interpolating font info (`copyInfo`, deprecated).
	pub copy_info: bool,
	/// Copy the groups (`copyGroups`, deprecated).
	pub copy_groups: bool,
	/// Copy the feature text (`copyFeatures`, deprecated).
	pub copy_features: bool,
	/// Mute the kerning of this source (`muteKerning`).
	pub mute_kerning: bool,
	/// Mute the interpolating font info (`muteInfo`).
	pub mute_info: bool,
	/// Glyphs to mute (`mutedGlyphNames`).
	pub muted_glyph_names: Vec<String>,
}

impl SourceDescriptor {
	/// `SourceDescriptor()`.
	pub fn new() -> Self {
		Self::default()
	}

	/// `source.filename = value`: stored with forward slashes.
	pub fn set_filename(&mut self, value: Option<&str>) {
		self.filename = value.map(posix);
	}

	/// `source.path = value`: stored with forward slashes.
	pub fn set_path(&mut self, value: Option<&str>) {
		self.path = value.map(posix);
	}

	/// `location` (deprecated alias of `designLocation`).
	pub fn location(&self) -> &AnisotropicLocation {
		&self.design_location
	}

	/// `setFamilyName`.
	pub fn set_family_name(&mut self, family_name: &str, language_code: &str) {
		self.localised_family_name
			.insert(language_code.to_owned(), family_name.to_owned());
	}

	/// `getFamilyName`.
	pub fn get_family_name(&self, language_code: &str) -> Option<&str> {
		self.localised_family_name
			.get(language_code)
			.map(String::as_str)
	}

	/// `getFullDesignLocation`: the design location, completed with the
	/// mapped axis defaults.
	pub fn get_full_design_location(
		&self,
		doc: &DesignSpaceDocument,
	) -> Result<AnisotropicLocation> {
		let mut result = AnisotropicLocation::new();
		for axis in &doc.axes {
			let name = axis.name_str();
			match self.design_location.get(&name) {
				Some(v) => {
					result.insert(name, *v);
				}
				None => {
					result.insert(name, axis.map_forward(axis.default())?.into());
				}
			}
		}
		Ok(result)
	}
}

/// A condition of a rule: `dict(name=..., minimum=..., maximum=...)`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Condition {
	/// The axis name (`name`).
	pub name: String,
	/// The minimum, or `None` for no minimum (`minimum`).
	pub minimum: Option<Number>,
	/// The maximum, or `None` for no maximum (`maximum`).
	pub maximum: Option<Number>,
}

impl Condition {
	/// A condition with both bounds optional.
	pub fn new(name: &str, minimum: Option<Number>, maximum: Option<Number>) -> Self {
		Condition {
			name: name.to_owned(),
			minimum,
			maximum,
		}
	}
}

/// `RuleDescriptor`: glyph substitutions in parts of the designspace.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RuleDescriptor {
	/// Unique name for this rule (`name`).
	pub name: Option<String>,
	/// The condition sets (`conditionSets`): the rule applies if any matches.
	pub condition_sets: Vec<Vec<Condition>>,
	/// The substitutions, as `(glyph, replacement)` pairs (`subs`).
	pub subs: Vec<(String, String)>,
}

/// `evaluateRule`: whether any of the rule's condition sets matches.
pub fn evaluate_rule(rule: &RuleDescriptor, location: &SimpleLocation) -> Result<bool> {
	for conditions in &rule.condition_sets {
		if evaluate_conditions(conditions, location)? {
			return Ok(true);
		}
	}
	Ok(false)
}

fn key_error(name: &str) -> DesignSpaceDocumentError {
	DesignSpaceDocumentError::Value(format!("KeyError: {}", fontoxide_misc::py::str_repr(name)))
}

/// `evaluateConditions`: whether all the conditions match the location.
pub fn evaluate_conditions(conditions: &[Condition], location: &SimpleLocation) -> Result<bool> {
	for cd in conditions {
		let value = *location.get(&cd.name).ok_or_else(|| key_error(&cd.name))?;
		match (cd.minimum, cd.maximum) {
			(None, Some(maximum)) => {
				if value > maximum {
					return Ok(false);
				}
			}
			(Some(minimum), None) => {
				if minimum > value {
					return Ok(false);
				}
			}
			(Some(minimum), Some(maximum)) => {
				if !(minimum <= value && value <= maximum) {
					return Ok(false);
				}
			}
			(None, None) => {
				return Err(DesignSpaceDocumentError::Value(
					"'>' not supported between instances of 'float' and 'NoneType'".into(),
				));
			}
		}
	}
	Ok(true)
}

/// `processRules`: the glyph names with the substitutions of the matching
/// rules applied, in rule order.
pub fn process_rules(
	rules: &[RuleDescriptor],
	location: &SimpleLocation,
	glyph_names: &[String],
) -> Result<Vec<String>> {
	let mut glyph_names = glyph_names.to_vec();
	for rule in rules {
		if evaluate_rule(rule, location)? {
			glyph_names = glyph_names
				.into_iter()
				.map(|name| match rule.subs.iter().find(|(a, _)| *a == name) {
					Some((_, b)) => b.clone(),
					None => name,
				})
				.collect();
		}
	}
	Ok(glyph_names)
}

/// `AxisMappingDescriptor`: maps an input location to an output location
/// (`avar` version 2).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AxisMappingDescriptor {
	/// Input axis values, in design space coordinates (`inputLocation`).
	pub input_location: SimpleLocation,
	/// Output axis values, in design space coordinates (`outputLocation`).
	pub output_location: SimpleLocation,
	/// A description of the mapping (`description`).
	pub description: Option<String>,
	/// A description of the group of mappings (`groupDescription`).
	pub group_description: Option<String>,
}

/// The data of a glyph of an instance (`InstanceDescriptor.glyphs` values,
/// deprecated since format 5).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GlyphData {
	/// `mute`.
	pub mute: bool,
	/// `unicodes`.
	pub unicodes: Option<Vec<i64>>,
	/// `note`.
	pub note: Option<String>,
	/// `instanceLocation`.
	pub instance_location: Option<AnisotropicLocation>,
	/// `masters`.
	pub masters: Option<Vec<GlyphMaster>>,
}

/// A master of [`GlyphData`]: `dict(font=..., location=..., glyphName=...)`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GlyphMaster {
	/// The source name (`font`).
	pub font: Option<String>,
	/// `location`.
	pub location: Option<AnisotropicLocation>,
	/// `glyphName`.
	pub glyph_name: Option<String>,
}

/// `InstanceDescriptor`: an instance of the designspace.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceDescriptor {
	/// Relative path to the instance file, as it is in the document
	/// (`filename`), with forward slashes.
	pub filename: Option<String>,
	/// Absolute path to the instance file (`path`), with forward slashes.
	pub path: Option<String>,
	/// Unique identifier name of the instance (`name`).
	pub name: Option<String>,
	/// Name of a [`LocationLabelDescriptor`] giving the location
	/// (`locationLabel`).
	pub location_label: Option<String>,
	/// Axis values in design space coordinates (`designLocation`, alias
	/// `location`).
	pub design_location: AnisotropicLocation,
	/// Axis values in user space coordinates (`userLocation`).
	pub user_location: SimpleLocation,
	/// `familyName`.
	pub family_name: Option<String>,
	/// `styleName`.
	pub style_name: Option<String>,
	/// `postScriptFontName`.
	pub post_script_font_name: Option<String>,
	/// `styleMapFamilyName`.
	pub style_map_family_name: Option<String>,
	/// `styleMapStyleName`.
	pub style_map_style_name: Option<String>,
	/// `localisedFamilyName`.
	pub localised_family_name: LocalizedNames,
	/// `localisedStyleName`.
	pub localised_style_name: LocalizedNames,
	/// `localisedStyleMapFamilyName`.
	pub localised_style_map_family_name: LocalizedNames,
	/// `localisedStyleMapStyleName`.
	pub localised_style_map_style_name: LocalizedNames,
	/// Special master definitions for glyphs (`glyphs`, deprecated).
	pub glyphs: IndexMap<String, GlyphData>,
	/// Whether the kerning is calculated (`kerning`, deprecated).
	pub kerning: bool,
	/// Whether the font info is calculated (`info`, deprecated).
	pub info: bool,
	/// Custom data (`lib`).
	pub lib: plistlib::Dict,
}

impl Default for InstanceDescriptor {
	fn default() -> Self {
		InstanceDescriptor {
			filename: None,
			path: None,
			name: None,
			location_label: None,
			design_location: AnisotropicLocation::new(),
			user_location: SimpleLocation::new(),
			family_name: None,
			style_name: None,
			post_script_font_name: None,
			style_map_family_name: None,
			style_map_style_name: None,
			localised_family_name: LocalizedNames::new(),
			localised_style_name: LocalizedNames::new(),
			localised_style_map_family_name: LocalizedNames::new(),
			localised_style_map_style_name: LocalizedNames::new(),
			glyphs: IndexMap::new(),
			kerning: true,
			info: true,
			lib: plistlib::Dict::new(),
		}
	}
}

impl InstanceDescriptor {
	/// `InstanceDescriptor()`.
	pub fn new() -> Self {
		Self::default()
	}

	/// `instance.filename = value`: stored with forward slashes.
	pub fn set_filename(&mut self, value: Option<&str>) {
		self.filename = value.map(posix);
	}

	/// `instance.path = value`: stored with forward slashes.
	pub fn set_path(&mut self, value: Option<&str>) {
		self.path = value.map(posix);
	}

	/// `location` (deprecated alias of `designLocation`).
	pub fn location(&self) -> &AnisotropicLocation {
		&self.design_location
	}

	/// `setStyleName`.
	pub fn set_style_name(&mut self, style_name: &str, language_code: &str) {
		self.localised_style_name
			.insert(language_code.to_owned(), style_name.to_owned());
	}

	/// `getStyleName`.
	pub fn get_style_name(&self, language_code: &str) -> Option<&str> {
		self.localised_style_name
			.get(language_code)
			.map(String::as_str)
	}

	/// `setFamilyName`.
	pub fn set_family_name(&mut self, family_name: &str, language_code: &str) {
		self.localised_family_name
			.insert(language_code.to_owned(), family_name.to_owned());
	}

	/// `getFamilyName`.
	pub fn get_family_name(&self, language_code: &str) -> Option<&str> {
		self.localised_family_name
			.get(language_code)
			.map(String::as_str)
	}

	/// `setStyleMapStyleName`.
	pub fn set_style_map_style_name(&mut self, name: &str, language_code: &str) {
		self.localised_style_map_style_name
			.insert(language_code.to_owned(), name.to_owned());
	}

	/// `getStyleMapStyleName`.
	pub fn get_style_map_style_name(&self, language_code: &str) -> Option<&str> {
		self.localised_style_map_style_name
			.get(language_code)
			.map(String::as_str)
	}

	/// `setStyleMapFamilyName`.
	pub fn set_style_map_family_name(&mut self, name: &str, language_code: &str) {
		self.localised_style_map_family_name
			.insert(language_code.to_owned(), name.to_owned());
	}

	/// `getStyleMapFamilyName`.
	pub fn get_style_map_family_name(&self, language_code: &str) -> Option<&str> {
		self.localised_style_map_family_name
			.get(language_code)
			.map(String::as_str)
	}

	/// `clearLocation`: clears all location fields, or only those of one axis.
	pub fn clear_location(&mut self, axis_name: Option<&str>) {
		self.location_label = None;
		match axis_name {
			None => {
				self.design_location.clear();
				self.user_location.clear();
			}
			Some(name) => {
				self.design_location.shift_remove(name);
				self.user_location.shift_remove(name);
			}
		}
	}

	/// `getLocationLabelDescriptor`: the label named by `locationLabel`.
	pub fn get_location_label_descriptor<'a>(
		&self,
		doc: &'a DesignSpaceDocument,
	) -> Result<Option<&'a LocationLabelDescriptor>> {
		let Some(location_label) = &self.location_label else {
			return Ok(None);
		};
		match doc.get_location_label(location_label) {
			Some(label) => Ok(Some(label)),
			None => Err(DesignSpaceDocumentError::Document(format!(
				"InstanceDescriptor.getLocationLabelDescriptor(): unknown location label `{location_label}` in instance `{}`.",
				self.name.as_deref().unwrap_or("None")
			))),
		}
	}

	/// `getFullDesignLocation`: per axis, the first of the location label, the
	/// design location, the mapped user location and the mapped default.
	pub fn get_full_design_location(
		&self,
		doc: &DesignSpaceDocument,
	) -> Result<AnisotropicLocation> {
		if let Some(label) = self.get_location_label_descriptor(doc)? {
			return Ok(to_anisotropic(&doc.map_forward(&label.user_location)?));
		}
		let mut result = AnisotropicLocation::new();
		for axis in &doc.axes {
			let name = axis.name_str();
			let value = if let Some(v) = self.design_location.get(&name) {
				*v
			} else if let Some(v) = self.user_location.get(&name) {
				axis.map_forward(*v)?.into()
			} else {
				axis.map_forward(axis.default())?.into()
			};
			result.insert(name, value);
		}
		Ok(result)
	}

	/// `getFullUserLocation`.
	pub fn get_full_user_location(&self, doc: &DesignSpaceDocument) -> Result<SimpleLocation> {
		doc.map_backward(&self.get_full_design_location(doc)?)
	}
}

/// `tagForAxisName`: a tag and English name for an axis name.
pub fn tag_for_axis_name(name: &str) -> (String, LocalizedNames) {
	let en = |s: &str| {
		let mut names = LocalizedNames::new();
		names.insert("en".to_owned(), s.to_owned());
		names
	};
	match name.to_lowercase().as_str() {
		"weight" => return ("wght".into(), en("Weight")),
		"width" => return ("wdth".into(), en("Width")),
		"optical" => return ("opsz".into(), en("Optical Size")),
		"slant" => return ("slnt".into(), en("Slant")),
		"italic" => return ("ital".into(), en("Italic")),
		_ => {}
	}
	let len = name.chars().count();
	let tag = if len < 4 {
		format!("{name}{}", "*".repeat(4 - len))
	} else {
		name.chars().take(4).collect()
	};
	(tag, en(name))
}

/// `AxisDescriptor`: a continuous axis.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AxisDescriptor {
	/// Four letter tag for this axis (`tag`).
	pub tag: Option<String>,
	/// Name of the axis as it is used in the location dicts (`name`).
	pub name: Option<String>,
	/// User-facing names, keyed by `xml:lang` code (`labelNames`).
	pub label_names: LocalizedNames,
	/// Whether the axis is hidden in user interfaces (`hidden`).
	pub hidden: bool,
	/// User to design space mapping (`map`).
	pub map: Vec<(Number, Number)>,
	/// STAT `axisOrdering`.
	pub axis_ordering: Option<i64>,
	/// STAT axis value tables format 1, 2, 3 (`axisLabels`).
	pub axis_labels: Vec<AxisLabelDescriptor>,
	/// The minimum user value (`minimum`).
	pub minimum: Number,
	/// The maximum user value (`maximum`).
	pub maximum: Number,
	/// The default user value (`default`).
	pub default: Number,
}

/// `DiscreteAxisDescriptor`: an axis that does not interpolate.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DiscreteAxisDescriptor {
	/// Four letter tag for this axis (`tag`).
	pub tag: Option<String>,
	/// Name of the axis as it is used in the location dicts (`name`).
	pub name: Option<String>,
	/// User-facing names, keyed by `xml:lang` code (`labelNames`).
	pub label_names: LocalizedNames,
	/// Whether the axis is hidden in user interfaces (`hidden`).
	pub hidden: bool,
	/// User to design space mapping (`map`).
	pub map: Vec<(Number, Number)>,
	/// STAT `axisOrdering`.
	pub axis_ordering: Option<i64>,
	/// STAT axis value tables format 1, 2, 3 (`axisLabels`).
	pub axis_labels: Vec<AxisLabelDescriptor>,
	/// The possible user values (`values`).
	pub values: Vec<Number>,
	/// The default user value (`default`).
	pub default: Number,
}

/// `AbstractAxisDescriptor.get_validated_map`: the map without repeated
/// pairs; an input with different outputs is an error.
fn get_validated_map(
	name: &Option<String>,
	map: &[(Number, Number)],
) -> Result<Vec<(Number, Number)>> {
	let mut validated: Vec<(Number, Number)> = Vec::new();
	for &(input_value, output_value) in map {
		if let Some(&(_, previous_output)) = validated.iter().find(|(i, _)| *i == input_value) {
			if previous_output != output_value {
				return Err(DesignSpaceDocumentError::Document(format!(
					"Axis '{}': mapping input value {input_value} has conflicting outputs {previous_output} and {output_value}.",
					name.as_deref().unwrap_or("None")
				)));
			}
			continue;
		}
		validated.push((input_value, output_value));
	}
	Ok(validated)
}

impl AxisDescriptor {
	/// `get_validated_map`.
	pub fn get_validated_map(&self) -> Result<Vec<(Number, Number)>> {
		get_validated_map(&self.name, &self.map)
	}

	/// `map_forward`: user to design value.
	pub fn map_forward(&self, v: Number) -> Result<Number> {
		let axis_map = self.get_validated_map()?;
		if axis_map.is_empty() {
			return Ok(v);
		}
		Ok(piecewise_linear_map(v, &axis_map))
	}

	/// `map_backward`: design to user value; only the x value of an
	/// anisotropic value is used.
	pub fn map_backward(&self, v: LocationValue) -> Result<Number> {
		let v = v.x();
		let axis_map = self.get_validated_map()?;
		if axis_map.is_empty() {
			return Ok(v);
		}
		// Build (design, user) pairs sorted by design then user, keeping both
		// endpoints of any many-to-one (flat) segments so we can invert them and
		// interpolation is correct on both sides.
		let mut backward: Vec<(Number, Number)> = axis_map
			.iter()
			.map(|&(user, design)| (design, user))
			.collect();
		backward.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
		let (design0, user0) = backward[0];
		if v <= design0 {
			return Ok(v + user0 - design0);
		}
		for pair in backward.windows(2) {
			let ((design1, user1), (design2, user2)) = (pair[0], pair[1]);
			if design1 <= v && v <= design2 {
				if design1 == design2 {
					return Ok(user1);
				}
				return Ok(user1 + (user2 - user1) * (v - design1) / (design2 - design1));
			}
		}
		let (design_n, user_n) = backward[backward.len() - 1];
		Ok(v + user_n - design_n)
	}
}

impl DiscreteAxisDescriptor {
	/// `get_validated_map`.
	pub fn get_validated_map(&self) -> Result<Vec<(Number, Number)>> {
		get_validated_map(&self.name, &self.map)
	}

	/// `map_forward`: the mapped value, or the value itself if it has no
	/// mapping entry.
	pub fn map_forward(&self, value: Number) -> Result<Number> {
		Ok(self
			.get_validated_map()?
			.into_iter()
			.find(|(k, _)| *k == value)
			.map_or(value, |(_, v)| v))
	}

	/// `map_backward`: the input value of a mapped value, or the value itself.
	pub fn map_backward(&self, value: LocationValue) -> Result<Number> {
		let value = value.x();
		Ok(self
			.get_validated_map()?
			.into_iter()
			.find(|(_, v)| *v == value)
			.map_or(value, |(k, _)| k))
	}
}

/// An axis of the document: `AxisDescriptor` or `DiscreteAxisDescriptor`.
#[derive(Debug, Clone, PartialEq)]
pub enum Axis {
	/// A continuous axis.
	Continuous(AxisDescriptor),
	/// A discrete axis (`hasattr(axis, "values")`).
	Discrete(DiscreteAxisDescriptor),
}

macro_rules! axis_field {
	($self:ident, $field:ident) => {
		match $self {
			Axis::Continuous(a) => &a.$field,
			Axis::Discrete(a) => &a.$field,
		}
	};
}

macro_rules! axis_field_mut {
	($self:ident, $field:ident) => {
		match $self {
			Axis::Continuous(a) => &mut a.$field,
			Axis::Discrete(a) => &mut a.$field,
		}
	};
}

impl Axis {
	/// `tag`.
	pub fn tag(&self) -> Option<&str> {
		axis_field!(self, tag).as_deref()
	}

	/// `name`.
	pub fn name(&self) -> Option<&str> {
		axis_field!(self, name).as_deref()
	}

	/// The name as a location key. Python uses `None` as a key for an axis
	/// without name; here that is `"None"`.
	pub(crate) fn name_str(&self) -> String {
		self.name().unwrap_or("None").to_owned()
	}

	/// `labelNames`.
	pub fn label_names(&self) -> &LocalizedNames {
		axis_field!(self, label_names)
	}

	/// `labelNames`, mutable.
	pub fn label_names_mut(&mut self) -> &mut LocalizedNames {
		axis_field_mut!(self, label_names)
	}

	/// `hidden`.
	pub fn hidden(&self) -> bool {
		*axis_field!(self, hidden)
	}

	/// `map`.
	pub fn map(&self) -> &[(Number, Number)] {
		axis_field!(self, map)
	}

	/// `map`, mutable.
	pub fn map_mut(&mut self) -> &mut Vec<(Number, Number)> {
		axis_field_mut!(self, map)
	}

	/// `axisOrdering`.
	pub fn axis_ordering(&self) -> Option<i64> {
		*axis_field!(self, axis_ordering)
	}

	/// `axisLabels`.
	pub fn axis_labels(&self) -> &[AxisLabelDescriptor] {
		axis_field!(self, axis_labels)
	}

	/// `default`.
	pub fn default(&self) -> Number {
		*axis_field!(self, default)
	}

	/// `default`, mutable.
	pub fn default_mut(&mut self) -> &mut Number {
		axis_field_mut!(self, default)
	}

	/// The continuous axis, if this is one (`hasattr(axis, "minimum")`).
	pub fn as_continuous(&self) -> Option<&AxisDescriptor> {
		match self {
			Axis::Continuous(a) => Some(a),
			Axis::Discrete(_) => None,
		}
	}

	/// The discrete axis, if this is one (`hasattr(axis, "values")`).
	pub fn as_discrete(&self) -> Option<&DiscreteAxisDescriptor> {
		match self {
			Axis::Discrete(a) => Some(a),
			Axis::Continuous(_) => None,
		}
	}

	/// `get_validated_map`.
	pub fn get_validated_map(&self) -> Result<Vec<(Number, Number)>> {
		get_validated_map(axis_field!(self, name), self.map())
	}

	/// `map_forward`.
	pub fn map_forward(&self, v: Number) -> Result<Number> {
		match self {
			Axis::Continuous(a) => a.map_forward(v),
			Axis::Discrete(a) => a.map_forward(v),
		}
	}

	/// `map_backward`.
	pub fn map_backward(&self, v: LocationValue) -> Result<Number> {
		match self {
			Axis::Continuous(a) => a.map_backward(v),
			Axis::Discrete(a) => a.map_backward(v),
		}
	}

	/// `axis.minimum`: an error for discrete axes, which have none.
	pub(crate) fn minimum(&self) -> Result<Number> {
		match self {
			Axis::Continuous(a) => Ok(a.minimum),
			Axis::Discrete(_) => Err(no_attribute("minimum")),
		}
	}

	/// `axis.maximum`: an error for discrete axes, which have none.
	pub(crate) fn maximum(&self) -> Result<Number> {
		match self {
			Axis::Continuous(a) => Ok(a.maximum),
			Axis::Discrete(_) => Err(no_attribute("maximum")),
		}
	}
}

fn no_attribute(name: &str) -> DesignSpaceDocumentError {
	DesignSpaceDocumentError::Value(format!(
		"'DiscreteAxisDescriptor' object has no attribute '{name}'"
	))
}

impl From<AxisDescriptor> for Axis {
	fn from(a: AxisDescriptor) -> Self {
		Axis::Continuous(a)
	}
}

impl From<DiscreteAxisDescriptor> for Axis {
	fn from(a: DiscreteAxisDescriptor) -> Self {
		Axis::Discrete(a)
	}
}

/// `AxisLabelDescriptor`: STAT data for a single axis (formats 1, 2, 3), in
/// user values.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AxisLabelDescriptor {
	/// STAT `rangeMinValue` (format 2) (`userMinimum`).
	pub user_minimum: Option<Number>,
	/// STAT `value` (format 1, 3) or `nominalValue` (format 2) (`userValue`).
	pub user_value: Number,
	/// STAT `rangeMaxValue` (format 2) (`userMaximum`).
	pub user_maximum: Option<Number>,
	/// Label for this axis location (`name`).
	pub name: String,
	/// STAT flag `ELIDABLE_AXIS_VALUE_NAME` (`elidable`).
	pub elidable: bool,
	/// STAT flag `OLDER_SIBLING_FONT_ATTRIBUTE` (`olderSibling`).
	pub older_sibling: bool,
	/// STAT `linkedValue` (format 3) (`linkedUserValue`).
	pub linked_user_value: Option<Number>,
	/// Translations of the label, keyed by `xml:lang` code (`labelNames`).
	pub label_names: LocalizedNames,
}

impl AxisLabelDescriptor {
	/// A label with a name and user value.
	pub fn new(name: &str, user_value: impl Into<Number>) -> Self {
		AxisLabelDescriptor {
			name: name.to_owned(),
			user_value: user_value.into(),
			..Default::default()
		}
	}

	/// `getFormat`: the STAT axis value format for this label.
	pub fn get_format(&self) -> u8 {
		if self.linked_user_value.is_some() {
			return 3;
		}
		if self.user_minimum.is_some() || self.user_maximum.is_some() {
			return 2;
		}
		1
	}

	/// `defaultName`: the English name, or the name.
	pub fn default_name(&self) -> &str {
		match self.label_names.get("en") {
			Some(en) if !en.is_empty() => en,
			_ => &self.name,
		}
	}
}

/// `LocationLabelDescriptor`: STAT data for a free-floating location (format
/// 4), in user values.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LocationLabelDescriptor {
	/// Label for this named location (`name`).
	pub name: String,
	/// The location in user coordinates; missing axes are at their default
	/// (`userLocation`).
	pub user_location: SimpleLocation,
	/// STAT flag `ELIDABLE_AXIS_VALUE_NAME` (`elidable`).
	pub elidable: bool,
	/// STAT flag `OLDER_SIBLING_FONT_ATTRIBUTE` (`olderSibling`).
	pub older_sibling: bool,
	/// Translations of the label, keyed by `xml:lang` code (`labelNames`).
	pub label_names: LocalizedNames,
}

impl LocationLabelDescriptor {
	/// A label with a name and user location.
	pub fn new(name: &str, user_location: SimpleLocation) -> Self {
		LocationLabelDescriptor {
			name: name.to_owned(),
			user_location,
			..Default::default()
		}
	}

	/// `defaultName`: the English name, or the name.
	pub fn default_name(&self) -> &str {
		match self.label_names.get("en") {
			Some(en) if !en.is_empty() => en,
			_ => &self.name,
		}
	}

	/// `getFullUserLocation`: the user location completed with the axis
	/// defaults.
	pub fn get_full_user_location(&self, doc: &DesignSpaceDocument) -> SimpleLocation {
		doc.axes
			.iter()
			.map(|axis| {
				let name = axis.name_str();
				let value = self
					.user_location
					.get(&name)
					.copied()
					.unwrap_or(axis.default());
				(name, value)
			})
			.collect()
	}
}

/// `VariableFontDescriptor`: a variable font, a sub-space of the designspace.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VariableFontDescriptor {
	/// Name of this variable font (`name`).
	pub name: String,
	/// Relative path to the variable font file (`filename`), with forward
	/// slashes.
	pub filename: Option<String>,
	/// Axis subsets to include; unmentioned axes are at their default
	/// (`axisSubsets`).
	pub axis_subsets: Vec<AxisSubset>,
	/// Custom data (`lib`).
	pub lib: plistlib::Dict,
}

impl VariableFontDescriptor {
	/// A variable font with a name.
	pub fn new(name: &str) -> Self {
		VariableFontDescriptor {
			name: name.to_owned(),
			..Default::default()
		}
	}

	/// `vf.filename = value`: stored with forward slashes.
	pub fn set_filename(&mut self, value: Option<&str>) {
		self.filename = value.map(posix);
	}
}

/// `RangeAxisSubsetDescriptor`: a subset of a continuous axis.
#[derive(Debug, Clone, PartialEq)]
pub struct RangeAxisSubsetDescriptor {
	/// Name of the axis (`name`).
	pub name: String,
	/// New minimum user value; `-inf` for the axis minimum (`userMinimum`).
	pub user_minimum: Number,
	/// New default user value; `None` for the axis default (`userDefault`).
	pub user_default: Option<Number>,
	/// New maximum user value; `inf` for the axis maximum (`userMaximum`).
	pub user_maximum: Number,
}

impl RangeAxisSubsetDescriptor {
	/// `RangeAxisSubsetDescriptor(name=name)`: the whole axis.
	pub fn new(name: &str) -> Self {
		RangeAxisSubsetDescriptor {
			name: name.to_owned(),
			user_minimum: Number::Float(f64::NEG_INFINITY),
			user_default: None,
			user_maximum: Number::Float(f64::INFINITY),
		}
	}
}

/// `ValueAxisSubsetDescriptor`: a single value of an axis.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueAxisSubsetDescriptor {
	/// Name of the axis (`name`).
	pub name: String,
	/// The user value to freeze the axis at (`userValue`).
	pub user_value: Number,
}

impl ValueAxisSubsetDescriptor {
	/// A value subset.
	pub fn new(name: &str, user_value: impl Into<Number>) -> Self {
		ValueAxisSubsetDescriptor {
			name: name.to_owned(),
			user_value: user_value.into(),
		}
	}
}

/// An axis subset of a [`VariableFontDescriptor`].
#[derive(Debug, Clone, PartialEq)]
pub enum AxisSubset {
	/// A range of a continuous axis.
	Range(RangeAxisSubsetDescriptor),
	/// A single value.
	Value(ValueAxisSubsetDescriptor),
}

impl AxisSubset {
	/// `name`.
	pub fn name(&self) -> &str {
		match self {
			AxisSubset::Range(s) => &s.name,
			AxisSubset::Value(s) => &s.name,
		}
	}
}

impl From<RangeAxisSubsetDescriptor> for AxisSubset {
	fn from(s: RangeAxisSubsetDescriptor) -> Self {
		AxisSubset::Range(s)
	}
}

impl From<ValueAxisSubsetDescriptor> for AxisSubset {
	fn from(s: ValueAxisSubsetDescriptor) -> Self {
		AxisSubset::Value(s)
	}
}
