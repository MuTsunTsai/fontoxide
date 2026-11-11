//! Port of `fontTools.ufoLib.validators`: low level data validators.
//!
//! The validators take plist [`Value`]s, which keep the distinctions between
//! the Python types that the checks look at. Python's `None` values in
//! dictionaries are represented by leaving the key out, which the checks
//! treat the same way.
//!
//! Validators that return `(valid, message)` in fontTools return a
//! [`Validation`]: `Ok(())` or `Err(message)`.

use std::collections::HashSet;

use fontoxide_misc::plistlib::{Dict, Value};

use crate::errors::Result;
use crate::utils::{Number, is_int, is_number, parse_float, parse_int, type_name};

/// The result of a validator that explains what is wrong: `(True, None)` is
/// `Ok(())`, `(False, message)` is `Err(message)`.
pub type Validation = std::result::Result<(), String>;

/// The Python types that `genericTypeValidator` and `genericDictValidator`
/// check with `isinstance`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeSpec {
	/// `str`.
	Str,
	/// `int` (which includes `bool`).
	Int,
	/// `(int, float)`.
	Number,
	/// `bool`.
	Bool,
	/// `dict`.
	Dict,
	/// `list`.
	List,
}

impl TypeSpec {
	/// `isinstance(value, typ)`.
	pub fn matches(self, value: &Value) -> bool {
		match self {
			TypeSpec::Str => matches!(value, Value::String(_)),
			TypeSpec::Int => is_int(value),
			TypeSpec::Number => is_number(value),
			TypeSpec::Bool => matches!(value, Value::Bool(_)),
			TypeSpec::Dict => matches!(value, Value::Dict(_)),
			TypeSpec::List => matches!(value, Value::Array(_)),
		}
	}
}

/// A dictionary prototype of `genericDictValidator`: the keys, their types
/// and whether they are required (`GenericDict`).
pub type GenericDict = [(&'static str, TypeSpec, bool)];

// -------
// Generic
// -------

/// `isDictEnough`.
pub fn is_dict_enough(value: &Value) -> bool {
	matches!(value, Value::Dict(_))
}

/// `genericTypeValidator`.
pub fn generic_type_validator(value: &Value, typ: TypeSpec) -> bool {
	typ.matches(value)
}

/// `genericIntListValidator`: a list of `int`s, all in `valid_values`.
///
/// LENIENT(L35): fontTools builds a `set` of the values first, which raises a
/// `TypeError` for lists or dictionaries in the list; here they are invalid.
pub fn generic_int_list_validator(values: &Value, valid_values: &[i64]) -> bool {
	let Value::Array(values) = values else {
		return false;
	};
	// `valuesSet - validValuesSet`: equality is by value, so `1.0` is in the set.
	let in_valid = |v: &Value| {
		Number::from_value(v).is_some_and(|n| valid_values.iter().any(|&o| Number::Int(o) == n))
	};
	if !values.iter().all(in_valid) {
		return false;
	}
	values.iter().all(is_int)
}

/// `genericNonNegativeIntValidator`.
pub fn generic_non_negative_int_validator(value: &Value) -> bool {
	is_int(value) && Number::from_value(value).is_some_and(|n| n >= Number::Int(0))
}

/// `genericNonNegativeNumberValidator`.
pub fn generic_non_negative_number_validator(value: &Value) -> bool {
	// `value < 0` is false for NaN, which is valid.
	Number::from_value(value)
		.is_some_and(|n| n.partial_cmp(&Number::Int(0)) != Some(std::cmp::Ordering::Less))
}

/// `genericDictValidator`.
pub fn generic_dict_validator(value: &Value, prototype: &GenericDict) -> bool {
	let Value::Dict(value) = value else {
		return false;
	};
	// missing required keys
	for &(key, _, required) in prototype {
		if required && !value.contains_key(key) {
			return false;
		}
	}
	// unknown keys and incorrect types
	for (key, v) in value {
		match prototype.iter().find(|(k, _, _)| k == key) {
			None => return false,
			Some(&(_, typ, _)) => {
				if !typ.matches(v) {
					return false;
				}
			}
		}
	}
	true
}

// --------------
// fontinfo.plist
// --------------

/// `fontInfoStyleMapStyleNameValidator`.
pub fn font_info_style_map_style_name_validator(value: &Value) -> bool {
	matches!(value, Value::String(s) if ["regular", "italic", "bold", "bold italic"].contains(&s.as_str()))
}

/// `fontInfoOpenTypeGaspRangeRecordsValidator`.
pub fn font_info_open_type_gasp_range_records_validator(value: &Value) -> bool {
	let Value::Array(records) = value else {
		return false;
	};
	const PROTOTYPE: &GenericDict = &[
		("rangeMaxPPEM", TypeSpec::Int, true),
		("rangeGaspBehavior", TypeSpec::List, true),
	];
	let mut ppem_order = Vec::new();
	for record in records {
		if !generic_dict_validator(record, PROTOTYPE) {
			return false;
		}
		let Value::Dict(record) = record else {
			return false;
		};
		let (Some(ppem), Some(behavior)) =
			(record.get("rangeMaxPPEM"), record.get("rangeGaspBehavior"))
		else {
			return false;
		};
		if !generic_non_negative_int_validator(ppem) {
			return false;
		}
		if !generic_int_list_validator(behavior, &[0, 1, 2, 3]) {
			return false;
		}
		ppem_order.push(Number::from_value(ppem).map_or(0.0, Number::to_f64));
	}
	ppem_order.windows(2).all(|w| w[0] <= w[1])
}

/// `calendar.monthrange(year, month)[1]`.
fn days_in_month(year: i128, month: i128) -> i128 {
	match month {
		1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
		4 | 6 | 9 | 11 => 30,
		_ => {
			let leap =
				year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0);
			if leap { 29 } else { 28 }
		}
	}
}

/// `fontInfoOpenTypeHeadCreatedValidator`: `YYYY/MM/DD HH:MM:SS`.
pub fn font_info_open_type_head_created_validator(value: &Value) -> bool {
	let Value::String(value) = value else {
		return false;
	};
	if value.chars().count() != 19 {
		return false;
	}
	if value.matches(' ').count() != 1 {
		return false;
	}
	let Some((str_date, str_time)) = value.split_once(' ') else {
		return false;
	};
	if str_date.matches('/').count() != 2 || str_time.matches(':').count() != 2 {
		return false;
	}
	let date: Vec<&str> = str_date.split('/').collect();
	let time: Vec<&str> = str_time.split(':').collect();
	let len = |s: &str| s.chars().count();
	if len(date[0]) != 4 || len(date[1]) != 2 || len(date[2]) != 2 {
		return false;
	}
	let (Some(year), Some(month), Some(day)) =
		(parse_int(date[0]), parse_int(date[1]), parse_int(date[2]))
	else {
		return false;
	};
	if !(1..=12).contains(&month) {
		return false;
	}
	if day < 1 || day > days_in_month(year, month) {
		return false;
	}
	if len(time[0]) != 2 || len(time[1]) != 2 || len(time[2]) != 2 {
		return false;
	}
	let (Some(hour), Some(minute), Some(second)) =
		(parse_int(time[0]), parse_int(time[1]), parse_int(time[2]))
	else {
		return false;
	};
	(0..=23).contains(&hour) && (0..=59).contains(&minute) && (0..=59).contains(&second)
}

/// `fontInfoOpenTypeNameRecordsValidator`.
pub fn font_info_open_type_name_records_validator(value: &Value) -> bool {
	let Value::Array(records) = value else {
		return false;
	};
	const PROTOTYPE: &GenericDict = &[
		("nameID", TypeSpec::Int, true),
		("platformID", TypeSpec::Int, true),
		("encodingID", TypeSpec::Int, true),
		("languageID", TypeSpec::Int, true),
		("string", TypeSpec::Str, true),
	];
	records.iter().all(|r| generic_dict_validator(r, PROTOTYPE))
}

/// `fontInfoOpenTypeOS2WeightClassValidator`.
pub fn font_info_open_type_os2_weight_class_validator(value: &Value) -> bool {
	is_int(value) && Number::from_value(value).is_some_and(|n| n >= Number::Int(0))
}

/// `fontInfoOpenTypeOS2WidthClassValidator`.
pub fn font_info_open_type_os2_width_class_validator(value: &Value) -> bool {
	is_int(value)
		&& Number::from_value(value).is_some_and(|n| n >= Number::Int(1) && n <= Number::Int(9))
}

fn ints(values: &Value) -> Option<Vec<Number>> {
	let Value::Array(values) = values else {
		return None;
	};
	values
		.iter()
		.map(|v| {
			if is_int(v) {
				Number::from_value(v)
			} else {
				None
			}
		})
		.collect()
}

/// `fontInfoVersion2OpenTypeOS2PanoseValidator`.
pub fn font_info_version2_open_type_os2_panose_validator(values: &Value) -> bool {
	matches!(values, Value::Array(a) if a.len() == 10) && ints(values).is_some()
}

/// `fontInfoVersion3OpenTypeOS2PanoseValidator`.
pub fn font_info_version3_open_type_os2_panose_validator(values: &Value) -> bool {
	if !matches!(values, Value::Array(a) if a.len() == 10) {
		return false;
	}
	ints(values).is_some_and(|v| v.iter().all(|&n| n >= Number::Int(0)))
}

/// `fontInfoOpenTypeOS2FamilyClassValidator`.
pub fn font_info_open_type_os2_family_class_validator(values: &Value) -> bool {
	if !matches!(values, Value::Array(a) if a.len() == 2) {
		return false;
	}
	let Some(v) = ints(values) else {
		return false;
	};
	let (class_id, subclass_id) = (v[0], v[1]);
	!(class_id < Number::Int(0) || class_id > Number::Int(14))
		&& !(subclass_id < Number::Int(0) || subclass_id > Number::Int(15))
}

fn number_list(values: &Value, max_len: usize, even: bool) -> bool {
	let Value::Array(values) = values else {
		return false;
	};
	if values.len() > max_len {
		return false;
	}
	if even && values.len() % 2 != 0 {
		return false;
	}
	values.iter().all(is_number)
}

/// `fontInfoPostscriptBluesValidator`.
pub fn font_info_postscript_blues_validator(values: &Value) -> bool {
	number_list(values, 14, true)
}

/// `fontInfoPostscriptOtherBluesValidator`.
pub fn font_info_postscript_other_blues_validator(values: &Value) -> bool {
	number_list(values, 10, true)
}

/// `fontInfoPostscriptStemsValidator`.
pub fn font_info_postscript_stems_validator(values: &Value) -> bool {
	number_list(values, 12, false)
}

/// `fontInfoPostscriptWindowsCharacterSetValidator`: `value in range(1, 21)`.
pub fn font_info_postscript_windows_character_set_validator(value: &Value) -> bool {
	Number::from_value(value).is_some_and(|n| (1..=20).any(|i| Number::Int(i) == n))
}

fn dir_is_valid(value: &Value) -> bool {
	match value {
		Value::Dict(d) => match d.get("dir") {
			None => true,
			Some(dir) => matches!(dir, Value::String(s) if s == "ltr" || s == "rtl"),
		},
		_ => true,
	}
}

fn dict_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
	match value {
		Value::Dict(d) => d.get(key),
		_ => None,
	}
}

fn array_items(value: Option<&Value>) -> &[Value] {
	match value {
		Some(Value::Array(a)) => a,
		_ => &[],
	}
}

/// `fontInfoWOFFMetadataUniqueIDValidator`.
pub fn font_info_woff_metadata_unique_id_validator(value: &Value) -> bool {
	generic_dict_validator(value, &[("id", TypeSpec::Str, true)])
}

/// `fontInfoWOFFMetadataVendorValidator`.
pub fn font_info_woff_metadata_vendor_validator(value: &Value) -> bool {
	const PROTOTYPE: &GenericDict = &[
		("name", TypeSpec::Str, true),
		("url", TypeSpec::Str, false),
		("dir", TypeSpec::Str, false),
		("class", TypeSpec::Str, false),
	];
	generic_dict_validator(value, PROTOTYPE) && dir_is_valid(value)
}

/// `fontInfoWOFFMetadataCreditsValidator`.
pub fn font_info_woff_metadata_credits_validator(value: &Value) -> bool {
	if !generic_dict_validator(value, &[("credits", TypeSpec::List, true)]) {
		return false;
	}
	let credits = array_items(dict_get(value, "credits"));
	if credits.is_empty() {
		return false;
	}
	const PROTOTYPE: &GenericDict = &[
		("name", TypeSpec::Str, true),
		("url", TypeSpec::Str, false),
		("role", TypeSpec::Str, false),
		("dir", TypeSpec::Str, false),
		("class", TypeSpec::Str, false),
	];
	credits
		.iter()
		.all(|c| generic_dict_validator(c, PROTOTYPE) && dir_is_valid(c))
}

/// `fontInfoWOFFMetadataDescriptionValidator`.
pub fn font_info_woff_metadata_description_validator(value: &Value) -> bool {
	const PROTOTYPE: &GenericDict = &[
		("url", TypeSpec::Str, false),
		("text", TypeSpec::List, true),
	];
	generic_dict_validator(value, PROTOTYPE)
		&& array_items(dict_get(value, "text"))
			.iter()
			.all(font_info_woff_metadata_text_value)
}

/// `fontInfoWOFFMetadataLicenseValidator`.
pub fn font_info_woff_metadata_license_validator(value: &Value) -> bool {
	const PROTOTYPE: &GenericDict = &[
		("url", TypeSpec::Str, false),
		("text", TypeSpec::List, false),
		("id", TypeSpec::Str, false),
	];
	generic_dict_validator(value, PROTOTYPE)
		&& array_items(dict_get(value, "text"))
			.iter()
			.all(font_info_woff_metadata_text_value)
}

/// `fontInfoWOFFMetadataTrademarkValidator`.
pub fn font_info_woff_metadata_trademark_validator(value: &Value) -> bool {
	generic_dict_validator(value, &[("text", TypeSpec::List, true)])
		&& array_items(dict_get(value, "text"))
			.iter()
			.all(font_info_woff_metadata_text_value)
}

/// `fontInfoWOFFMetadataCopyrightValidator`.
pub fn font_info_woff_metadata_copyright_validator(value: &Value) -> bool {
	font_info_woff_metadata_trademark_validator(value)
}

/// `fontInfoWOFFMetadataLicenseeValidator`.
pub fn font_info_woff_metadata_licensee_validator(value: &Value) -> bool {
	const PROTOTYPE: &GenericDict = &[
		("name", TypeSpec::Str, true),
		("dir", TypeSpec::Str, false),
		("class", TypeSpec::Str, false),
	];
	generic_dict_validator(value, PROTOTYPE) && dir_is_valid(value)
}

const TEXT_PROTOTYPE: &GenericDict = &[
	("text", TypeSpec::Str, true),
	("language", TypeSpec::Str, false),
	("dir", TypeSpec::Str, false),
	("class", TypeSpec::Str, false),
];

/// `fontInfoWOFFMetadataTextValue`.
pub fn font_info_woff_metadata_text_value(value: &Value) -> bool {
	generic_dict_validator(value, TEXT_PROTOTYPE) && dir_is_valid(value)
}

/// `fontInfoWOFFMetadataExtensionsValidator`.
pub fn font_info_woff_metadata_extensions_validator(value: &Value) -> bool {
	match value {
		Value::Array(a) if !a.is_empty() => {
			a.iter().all(font_info_woff_metadata_extension_validator)
		}
		_ => false,
	}
}

/// `fontInfoWOFFMetadataExtensionValidator`.
pub fn font_info_woff_metadata_extension_validator(value: &Value) -> bool {
	const PROTOTYPE: &GenericDict = &[
		("names", TypeSpec::List, false),
		("items", TypeSpec::List, true),
		("id", TypeSpec::Str, false),
	];
	generic_dict_validator(value, PROTOTYPE)
		&& array_items(dict_get(value, "names"))
			.iter()
			.all(font_info_woff_metadata_extension_name_validator)
		&& array_items(dict_get(value, "items"))
			.iter()
			.all(font_info_woff_metadata_extension_item_validator)
}

/// `fontInfoWOFFMetadataExtensionItemValidator`.
pub fn font_info_woff_metadata_extension_item_validator(value: &Value) -> bool {
	const PROTOTYPE: &GenericDict = &[
		("id", TypeSpec::Str, false),
		("names", TypeSpec::List, true),
		("values", TypeSpec::List, true),
	];
	generic_dict_validator(value, PROTOTYPE)
		&& array_items(dict_get(value, "names"))
			.iter()
			.all(font_info_woff_metadata_extension_name_validator)
		&& array_items(dict_get(value, "values"))
			.iter()
			.all(font_info_woff_metadata_extension_value_validator)
}

/// `fontInfoWOFFMetadataExtensionNameValidator`.
pub fn font_info_woff_metadata_extension_name_validator(value: &Value) -> bool {
	generic_dict_validator(value, TEXT_PROTOTYPE) && dir_is_valid(value)
}

/// `fontInfoWOFFMetadataExtensionValueValidator`.
pub fn font_info_woff_metadata_extension_value_validator(value: &Value) -> bool {
	generic_dict_validator(value, TEXT_PROTOTYPE) && dir_is_valid(value)
}

// ----------
// Guidelines
// ----------

/// `guidelinesValidator`: `identifiers` are the identifiers already used in
/// the glyph or font; the ones of the guidelines are added.
pub fn guidelines_validator(value: &Value, identifiers: Option<&mut HashSet<String>>) -> bool {
	let Value::Array(guides) = value else {
		return false;
	};
	let mut own = HashSet::new();
	let identifiers = identifiers.unwrap_or(&mut own);
	for guide in guides {
		if !guideline_validator(guide) {
			return false;
		}
		if let Some(Value::String(identifier)) = dict_get(guide, "identifier") {
			if identifiers.contains(identifier) {
				return false;
			}
			identifiers.insert(identifier.clone());
		}
	}
	true
}

const GUIDELINE_PROTOTYPE: &GenericDict = &[
	("x", TypeSpec::Number, false),
	("y", TypeSpec::Number, false),
	("angle", TypeSpec::Number, false),
	("name", TypeSpec::Str, false),
	("color", TypeSpec::Str, false),
	("identifier", TypeSpec::Str, false),
];

/// `guidelineValidator`.
pub fn guideline_validator(value: &Value) -> bool {
	if !generic_dict_validator(value, GUIDELINE_PROTOTYPE) {
		return false;
	}
	if let Some(angle) = dict_get(value, "angle").and_then(Number::from_value)
		&& (angle < Number::Int(0) || angle > Number::Int(360))
	{
		return false;
	}
	if let Some(identifier) = dict_get(value, "identifier")
		&& !identifier_validator(identifier)
	{
		return false;
	}
	if let Some(color) = dict_get(value, "color")
		&& !color_validator(color)
	{
		return false;
	}
	true
}

// -------
// Anchors
// -------

/// `anchorsValidator`: `identifiers` as in [`guidelines_validator`].
pub fn anchors_validator(value: &Value, identifiers: Option<&mut HashSet<String>>) -> bool {
	let Value::Array(anchors) = value else {
		return false;
	};
	let mut own = HashSet::new();
	let identifiers = identifiers.unwrap_or(&mut own);
	for anchor in anchors {
		if !anchor_validator(anchor) {
			return false;
		}
		if let Some(Value::String(identifier)) = dict_get(anchor, "identifier") {
			if identifiers.contains(identifier) {
				return false;
			}
			identifiers.insert(identifier.clone());
		}
	}
	true
}

const ANCHOR_PROTOTYPE: &GenericDict = &[
	("x", TypeSpec::Number, false),
	("y", TypeSpec::Number, false),
	("name", TypeSpec::Str, false),
	("color", TypeSpec::Str, false),
	("identifier", TypeSpec::Str, false),
];

/// `anchorValidator`.
pub fn anchor_validator(value: &Value) -> bool {
	if !generic_dict_validator(value, ANCHOR_PROTOTYPE) {
		return false;
	}
	if dict_get(value, "x").is_none() || dict_get(value, "y").is_none() {
		return false;
	}
	if let Some(identifier) = dict_get(value, "identifier")
		&& !identifier_validator(identifier)
	{
		return false;
	}
	if let Some(color) = dict_get(value, "color")
		&& !color_validator(color)
	{
		return false;
	}
	true
}

// ----------
// Identifier
// ----------

/// `identifierValidator`: 1 to 100 characters from U+0020 to U+007E.
///
/// ```
/// use fontoxide_ufolib::validators::identifier_validator;
/// use fontoxide_ufolib::Value;
/// assert!(identifier_validator(&Value::from("a")));
/// assert!(!identifier_validator(&Value::from("")));
/// assert!(!identifier_validator(&Value::from("a".repeat(101))));
/// ```
pub fn identifier_validator(value: &Value) -> bool {
	match value {
		Value::String(s) => is_valid_identifier(s),
		_ => false,
	}
}

/// [`identifier_validator`] of a string.
pub fn is_valid_identifier(s: &str) -> bool {
	!s.is_empty() && s.chars().count() <= 100 && s.chars().all(|c| ('\x20'..='\x7e').contains(&c))
}

// -----
// Color
// -----

/// `colorValidator`: four comma-separated numbers from 0 to 1.
///
/// ```
/// use fontoxide_ufolib::validators::color_validator;
/// use fontoxide_ufolib::Value;
/// assert!(color_validator(&Value::from(".5,.5,.5,.5")));
/// assert!(color_validator(&Value::from("1, 1, 1, 1")));
/// assert!(!color_validator(&Value::from("2,0,0,0")));
/// assert!(!color_validator(&Value::from("1 1,1,1")));
/// ```
pub fn color_validator(value: &Value) -> bool {
	match value {
		Value::String(s) => is_valid_color(s),
		_ => false,
	}
}

/// [`color_validator`] of a string.
pub fn is_valid_color(value: &str) -> bool {
	let parts: Vec<&str> = value.split(',').collect();
	if parts.len() != 4 {
		return false;
	}
	for part in parts {
		let number = match parse_int(part) {
			Some(i) => i as f64,
			None => match parse_float(part) {
				Some(f) => f,
				None => return false,
			},
		};
		if !(0.0..=1.0).contains(&number) {
			return false;
		}
	}
	true
}

// -----
// image
// -----

/// `pngSignature`.
pub const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

const IMAGE_PROTOTYPE: &GenericDict = &[
	("fileName", TypeSpec::Str, true),
	("xScale", TypeSpec::Number, false),
	("xyScale", TypeSpec::Number, false),
	("yxScale", TypeSpec::Number, false),
	("yScale", TypeSpec::Number, false),
	("xOffset", TypeSpec::Number, false),
	("yOffset", TypeSpec::Number, false),
	("color", TypeSpec::Str, false),
];

/// `imageValidator`.
pub fn image_validator(value: &Value) -> bool {
	if !generic_dict_validator(value, IMAGE_PROTOTYPE) {
		return false;
	}
	if matches!(dict_get(value, "fileName"), Some(Value::String(s)) if s.is_empty()) {
		return false;
	}
	if let Some(color) = dict_get(value, "color")
		&& !color_validator(color)
	{
		return false;
	}
	true
}

/// `pngValidator(data=...)`: checks the signature of PNG data.
pub fn png_validator(data: &[u8]) -> Validation {
	if data.get(..8) != Some(PNG_SIGNATURE) {
		return Err("Image does not begin with the PNG signature.".into());
	}
	Ok(())
}

// -------------------
// layercontents.plist
// -------------------

/// `layerContentsValidator`: `exists` tells whether a directory exists in the
/// UFO (`fileSystem.exists`).
pub fn layer_contents_validator(
	value: &Value,
	exists: &dyn Fn(&str) -> Result<bool>,
) -> Result<Validation> {
	let bogus = || Err("layercontents.plist in not in the correct format.".to_owned());
	let Value::Array(entries) = value else {
		return Ok(bogus());
	};
	let mut used_layer_names = HashSet::new();
	let mut used_directories = HashSet::new();
	let mut found_default = false;
	for entry in entries {
		let Value::Array(entry) = entry else {
			return Ok(bogus());
		};
		if entry.len() != 2 {
			return Ok(bogus());
		}
		let (Value::String(layer_name), Value::String(directory_name)) = (&entry[0], &entry[1])
		else {
			return Ok(bogus());
		};
		if directory_name != "glyphs" && !directory_name.starts_with("glyphs.") {
			return Ok(Err(format!(
				"Invalid directory name ({directory_name}) in layercontents.plist."
			)));
		}
		if layer_name.is_empty() {
			return Ok(Err("Empty layer name in layercontents.plist.".into()));
		}
		if !exists(directory_name)? {
			return Ok(Err(format!(
				"A glyphset does not exist at {directory_name}."
			)));
		}
		if layer_name == "public.default" && directory_name != "glyphs" {
			return Ok(Err(
				"The name public.default is being used by a layer that is not the default.".into(),
			));
		}
		if !used_layer_names.insert(layer_name.clone()) {
			return Ok(Err(format!(
				"The layer name {layer_name} is used by more than one layer."
			)));
		}
		if !used_directories.insert(directory_name.clone()) {
			return Ok(Err(format!(
				"The directory {directory_name} is used by more than one layer."
			)));
		}
		if directory_name == "glyphs" {
			found_default = true;
		}
	}
	if !found_default {
		return Ok(Err(
			"The required default glyph set is not in the UFO.".into()
		));
	}
	Ok(Ok(()))
}

// ------------
// groups.plist
// ------------

/// `groupsValidator`: the groups are a dictionary of glyph name lists, kerning
/// groups have complete names, and no glyph is in two kerning groups of the
/// same side.
///
/// ```
/// use fontoxide_ufolib::validators::groups_validator;
/// use fontoxide_ufolib::Value;
/// let groups = |items: &[(&str, &[&str])]| Value::Dict(items.iter().map(|(k, v)| {
///     ((*k).to_owned(), Value::Array(v.iter().map(|g| Value::from(*g)).collect()))
/// }).collect());
/// assert_eq!(groups_validator(&groups(&[("A", &["A", "A"]), ("A2", &["A"])])), Ok(()));
/// assert_eq!(
///     groups_validator(&groups(&[("", &["A"])])),
///     Err("A group has an empty name.".to_owned())
/// );
/// assert_eq!(
///     groups_validator(&groups(&[("public.kern1.A1", &["A"]), ("public.kern1.A2", &["A"])])),
///     Err("The glyph \"A\" occurs in too many kerning groups.".to_owned())
/// );
/// ```
pub fn groups_validator(value: &Value) -> Validation {
	let bogus = "The group data is not in the correct format.";
	let Value::Dict(groups) = value else {
		return Err(bogus.into());
	};
	let mut first_side: HashSet<&str> = HashSet::new();
	let mut second_side: HashSet<&str> = HashSet::new();
	for (group_name, glyph_list) in groups {
		let Value::Array(glyph_list) = glyph_list else {
			return Err(bogus.into());
		};
		if group_name.is_empty() {
			return Err("A group has an empty name.".into());
		}
		if group_name.starts_with("public.") {
			if !group_name.starts_with("public.kern1.") && !group_name.starts_with("public.kern2.")
			{
				// unknown public.* name. silently skip.
				continue;
			} else if group_name.len() == "public.kernN.".len() {
				return Err(
					"The group data contains a kerning group with an incomplete name.".into(),
				);
			}
			let d = if group_name.starts_with("public.kern1.") {
				&mut first_side
			} else {
				&mut second_side
			};
			for glyph_name in glyph_list {
				let Value::String(glyph_name) = glyph_name else {
					return Err(format!(
						"The group data {group_name} contains an invalid member."
					));
				};
				if !d.insert(glyph_name) {
					return Err(format!(
						"The glyph \"{glyph_name}\" occurs in too many kerning groups."
					));
				}
			}
		}
	}
	Ok(())
}

// -------------
// kerning.plist
// -------------

/// `kerningValidator`: the kerning is a dictionary of dictionaries of
/// numbers.
pub fn kerning_validator(data: &Value) -> Validation {
	let bogus = || Err("The kerning data is not in the correct format.".to_owned());
	let Value::Dict(data) = data else {
		return bogus();
	};
	for second_dict in data.values() {
		let Value::Dict(second_dict) = second_dict else {
			return bogus();
		};
		if !second_dict.values().all(is_number) {
			return bogus();
		}
	}
	Ok(())
}

// -------------
// lib.plist/lib
// -------------

/// `fontLibValidator`: the lib is a dictionary, and `public.glyphOrder` is a
/// list of strings.
pub fn font_lib_validator(value: &Value) -> Validation {
	let Value::Dict(lib) = value else {
		return Err(format!(
			"The lib data is not in the correct format: expected a dictionary, found {}",
			type_name(value)
		));
	};
	font_lib_dict_validator(lib)
}

/// [`font_lib_validator`] of a dictionary.
pub fn font_lib_dict_validator(lib: &Dict) -> Validation {
	if let Some(value) = lib.get("public.glyphOrder") {
		let bogus = "public.glyphOrder is not properly formatted";
		let Value::Array(glyph_names) = value else {
			return Err(format!(
				"{bogus}: expected list or tuple, found {}",
				type_name(value)
			));
		};
		for glyph_name in glyph_names {
			if !matches!(glyph_name, Value::String(_)) {
				return Err(format!(
					"{bogus}: expected str, found {}",
					type_name(glyph_name)
				));
			}
		}
	}
	Ok(())
}

// --------
// GLIF lib
// --------

/// `glyphLibValidator`: the lib is a dictionary, and `public.markColor` is a
/// color.
pub fn glyph_lib_validator(value: &Value) -> Validation {
	let Value::Dict(lib) = value else {
		return Err(format!(
			"The lib data is not in the correct format: expected a dictionary, found {}",
			type_name(value)
		));
	};
	if let Some(color) = lib.get("public.markColor")
		&& !color_validator(color)
	{
		return Err("public.markColor is not properly formatted.".into());
	}
	Ok(())
}
