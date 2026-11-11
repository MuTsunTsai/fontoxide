//! `fontinfo.plist` support of `fontTools.ufoLib`: the attributes of each
//! format version, their validation, and the conversions between versions.
//!
//! Font info is a [`Dict`] of attribute names to values: fontTools sets and
//! gets the attributes of an arbitrary object instead.

use fontoxide_misc::plistlib::{Dict, Value};
use indexmap::IndexMap;

use crate::errors::{Result, UfoLibError, ufo_err};
use crate::utils::{Number, value_repr};
use crate::validators::{self as v, TypeSpec};

/// How the value of an attribute is validated (`valueValidator` with `type`
/// and `valueOptions`).
#[derive(Clone, Copy)]
pub enum InfoValidator {
	/// `genericTypeValidator` with the type.
	Type(TypeSpec),
	/// `genericIntListValidator` with the options.
	IntList(&'static [i64]),
	/// A specific validator.
	Custom(fn(&Value) -> bool),
}

impl std::fmt::Debug for InfoValidator {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			InfoValidator::Type(t) => write!(f, "Type({t:?})"),
			InfoValidator::IntList(o) => write!(f, "IntList({o:?})"),
			InfoValidator::Custom(_) => f.write_str("Custom"),
		}
	}
}

impl InfoValidator {
	/// Whether `value` is valid.
	pub fn validate(self, value: &Value) -> bool {
		match self {
			InfoValidator::Type(t) => v::generic_type_validator(value, t),
			InfoValidator::IntList(options) => v::generic_int_list_validator(value, options),
			InfoValidator::Custom(f) => f(value),
		}
	}
}

/// `fontInfoOpenTypeHeadFlagsOptions`.
pub const OPEN_TYPE_HEAD_FLAGS_OPTIONS: &[i64] =
	&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
/// `fontInfoOpenTypeOS2SelectionOptions`.
pub const OPEN_TYPE_OS2_SELECTION_OPTIONS: &[i64] = &[1, 2, 3, 4, 7, 8, 9];
/// `fontInfoOpenTypeOS2UnicodeRangesOptions`: 0 to 127.
pub const OPEN_TYPE_OS2_UNICODE_RANGES_OPTIONS: &[i64] = &{
	let mut a = [0i64; 128];
	let mut i = 0;
	while i < 128 {
		a[i] = i as i64;
		i += 1;
	}
	a
};
/// `fontInfoOpenTypeOS2CodePageRangesOptions`: 0 to 63.
pub const OPEN_TYPE_OS2_CODE_PAGE_RANGES_OPTIONS: &[i64] = &{
	let mut a = [0i64; 64];
	let mut i = 0;
	while i < 64 {
		a[i] = i as i64;
		i += 1;
	}
	a
};
/// `fontInfoOpenTypeOS2TypeOptions`.
pub const OPEN_TYPE_OS2_TYPE_OPTIONS: &[i64] = &[0, 1, 2, 3, 8, 9];

/// `fontInfoAttributesVersion1`, in the order of the fontTools source (it is
/// a `set` there).
pub const FONT_INFO_ATTRIBUTES_VERSION1: &[&str] = &[
	"familyName",
	"styleName",
	"fullName",
	"fontName",
	"menuName",
	"fontStyle",
	"note",
	"versionMajor",
	"versionMinor",
	"year",
	"copyright",
	"notice",
	"trademark",
	"license",
	"licenseURL",
	"createdBy",
	"designer",
	"designerURL",
	"vendorURL",
	"unitsPerEm",
	"ascender",
	"descender",
	"capHeight",
	"xHeight",
	"defaultWidth",
	"slantAngle",
	"italicAngle",
	"widthName",
	"weightName",
	"weightValue",
	"fondName",
	"otFamilyName",
	"otStyleName",
	"otMacName",
	"msCharSet",
	"fondID",
	"uniqueID",
	"ttVendor",
	"ttUniqueID",
	"ttVersion",
];

use InfoValidator::{Custom, IntList, Type};
use TypeSpec::{Bool, Int, Number as Num, Str};

/// `fontInfoAttributesVersion2ValueData`, in order.
pub const FONT_INFO_ATTRIBUTES_VERSION2_VALUE_DATA: &[(&str, InfoValidator)] = &[
	("familyName", Type(Str)),
	("styleName", Type(Str)),
	("styleMapFamilyName", Type(Str)),
	(
		"styleMapStyleName",
		Custom(v::font_info_style_map_style_name_validator),
	),
	("versionMajor", Type(Int)),
	("versionMinor", Type(Int)),
	("year", Type(Int)),
	("copyright", Type(Str)),
	("trademark", Type(Str)),
	("unitsPerEm", Type(Num)),
	("descender", Type(Num)),
	("xHeight", Type(Num)),
	("capHeight", Type(Num)),
	("ascender", Type(Num)),
	("italicAngle", Type(Num)),
	("note", Type(Str)),
	(
		"openTypeHeadCreated",
		Custom(v::font_info_open_type_head_created_validator),
	),
	("openTypeHeadLowestRecPPEM", Type(Num)),
	("openTypeHeadFlags", IntList(OPEN_TYPE_HEAD_FLAGS_OPTIONS)),
	("openTypeHheaAscender", Type(Num)),
	("openTypeHheaDescender", Type(Num)),
	("openTypeHheaLineGap", Type(Num)),
	("openTypeHheaCaretSlopeRise", Type(Int)),
	("openTypeHheaCaretSlopeRun", Type(Int)),
	("openTypeHheaCaretOffset", Type(Num)),
	("openTypeNameDesigner", Type(Str)),
	("openTypeNameDesignerURL", Type(Str)),
	("openTypeNameManufacturer", Type(Str)),
	("openTypeNameManufacturerURL", Type(Str)),
	("openTypeNameLicense", Type(Str)),
	("openTypeNameLicenseURL", Type(Str)),
	("openTypeNameVersion", Type(Str)),
	("openTypeNameUniqueID", Type(Str)),
	("openTypeNameDescription", Type(Str)),
	("openTypeNamePreferredFamilyName", Type(Str)),
	("openTypeNamePreferredSubfamilyName", Type(Str)),
	("openTypeNameCompatibleFullName", Type(Str)),
	("openTypeNameSampleText", Type(Str)),
	("openTypeNameWWSFamilyName", Type(Str)),
	("openTypeNameWWSSubfamilyName", Type(Str)),
	(
		"openTypeOS2WidthClass",
		Custom(v::font_info_open_type_os2_width_class_validator),
	),
	(
		"openTypeOS2WeightClass",
		Custom(v::font_info_open_type_os2_weight_class_validator),
	),
	(
		"openTypeOS2Selection",
		IntList(OPEN_TYPE_OS2_SELECTION_OPTIONS),
	),
	("openTypeOS2VendorID", Type(Str)),
	(
		"openTypeOS2Panose",
		Custom(v::font_info_version2_open_type_os2_panose_validator),
	),
	(
		"openTypeOS2FamilyClass",
		Custom(v::font_info_open_type_os2_family_class_validator),
	),
	(
		"openTypeOS2UnicodeRanges",
		IntList(OPEN_TYPE_OS2_UNICODE_RANGES_OPTIONS),
	),
	(
		"openTypeOS2CodePageRanges",
		IntList(OPEN_TYPE_OS2_CODE_PAGE_RANGES_OPTIONS),
	),
	("openTypeOS2TypoAscender", Type(Num)),
	("openTypeOS2TypoDescender", Type(Num)),
	("openTypeOS2TypoLineGap", Type(Num)),
	("openTypeOS2WinAscent", Type(Num)),
	("openTypeOS2WinDescent", Type(Num)),
	("openTypeOS2Type", IntList(OPEN_TYPE_OS2_TYPE_OPTIONS)),
	("openTypeOS2SubscriptXSize", Type(Num)),
	("openTypeOS2SubscriptYSize", Type(Num)),
	("openTypeOS2SubscriptXOffset", Type(Num)),
	("openTypeOS2SubscriptYOffset", Type(Num)),
	("openTypeOS2SuperscriptXSize", Type(Num)),
	("openTypeOS2SuperscriptYSize", Type(Num)),
	("openTypeOS2SuperscriptXOffset", Type(Num)),
	("openTypeOS2SuperscriptYOffset", Type(Num)),
	("openTypeOS2StrikeoutSize", Type(Num)),
	("openTypeOS2StrikeoutPosition", Type(Num)),
	("openTypeVheaVertTypoAscender", Type(Num)),
	("openTypeVheaVertTypoDescender", Type(Num)),
	("openTypeVheaVertTypoLineGap", Type(Num)),
	("openTypeVheaCaretSlopeRise", Type(Int)),
	("openTypeVheaCaretSlopeRun", Type(Int)),
	("openTypeVheaCaretOffset", Type(Num)),
	("postscriptFontName", Type(Str)),
	("postscriptFullName", Type(Str)),
	("postscriptSlantAngle", Type(Num)),
	("postscriptUniqueID", Type(Int)),
	("postscriptUnderlineThickness", Type(Num)),
	("postscriptUnderlinePosition", Type(Num)),
	("postscriptIsFixedPitch", Type(Bool)),
	(
		"postscriptBlueValues",
		Custom(v::font_info_postscript_blues_validator),
	),
	(
		"postscriptOtherBlues",
		Custom(v::font_info_postscript_other_blues_validator),
	),
	(
		"postscriptFamilyBlues",
		Custom(v::font_info_postscript_blues_validator),
	),
	(
		"postscriptFamilyOtherBlues",
		Custom(v::font_info_postscript_other_blues_validator),
	),
	(
		"postscriptStemSnapH",
		Custom(v::font_info_postscript_stems_validator),
	),
	(
		"postscriptStemSnapV",
		Custom(v::font_info_postscript_stems_validator),
	),
	("postscriptBlueFuzz", Type(Num)),
	("postscriptBlueShift", Type(Num)),
	("postscriptBlueScale", Type(Num)),
	("postscriptForceBold", Type(Bool)),
	("postscriptDefaultWidthX", Type(Num)),
	("postscriptNominalWidthX", Type(Num)),
	("postscriptWeightName", Type(Str)),
	("postscriptDefaultCharacter", Type(Str)),
	(
		"postscriptWindowsCharacterSet",
		Custom(v::font_info_postscript_windows_character_set_validator),
	),
	("macintoshFONDFamilyID", Type(Int)),
	("macintoshFONDName", Type(Str)),
];

/// The entries of `fontInfoAttributesVersion3ValueData` that differ from
/// version 2, in the order of the `update` call. Where fontTools gives both a
/// `type` and a `valueValidator`, only the validator is used (so for example
/// `openTypeOS2WinAscent`, an `int`, accepts non-negative floats).
const VERSION3_UPDATES: &[(&str, InfoValidator)] = &[
	(
		"versionMinor",
		Custom(v::generic_non_negative_int_validator),
	),
	(
		"unitsPerEm",
		Custom(v::generic_non_negative_number_validator),
	),
	(
		"openTypeHeadLowestRecPPEM",
		Custom(v::generic_non_negative_number_validator),
	),
	("openTypeHheaAscender", Type(Int)),
	("openTypeHheaDescender", Type(Int)),
	("openTypeHheaLineGap", Type(Int)),
	("openTypeHheaCaretOffset", Type(Int)),
	(
		"openTypeOS2Panose",
		Custom(v::font_info_version3_open_type_os2_panose_validator),
	),
	("openTypeOS2TypoAscender", Type(Int)),
	("openTypeOS2TypoDescender", Type(Int)),
	("openTypeOS2TypoLineGap", Type(Int)),
	(
		"openTypeOS2WinAscent",
		Custom(v::generic_non_negative_number_validator),
	),
	(
		"openTypeOS2WinDescent",
		Custom(v::generic_non_negative_number_validator),
	),
	("openTypeOS2SubscriptXSize", Type(Int)),
	("openTypeOS2SubscriptYSize", Type(Int)),
	("openTypeOS2SubscriptXOffset", Type(Int)),
	("openTypeOS2SubscriptYOffset", Type(Int)),
	("openTypeOS2SuperscriptXSize", Type(Int)),
	("openTypeOS2SuperscriptYSize", Type(Int)),
	("openTypeOS2SuperscriptXOffset", Type(Int)),
	("openTypeOS2SuperscriptYOffset", Type(Int)),
	("openTypeOS2StrikeoutSize", Type(Int)),
	("openTypeOS2StrikeoutPosition", Type(Int)),
	(
		"openTypeGaspRangeRecords",
		Custom(v::font_info_open_type_gasp_range_records_validator),
	),
	(
		"openTypeNameRecords",
		Custom(v::font_info_open_type_name_records_validator),
	),
	("openTypeVheaVertTypoAscender", Type(Int)),
	("openTypeVheaVertTypoDescender", Type(Int)),
	("openTypeVheaVertTypoLineGap", Type(Int)),
	("openTypeVheaCaretOffset", Type(Int)),
	(
		"woffMajorVersion",
		Custom(v::generic_non_negative_int_validator),
	),
	(
		"woffMinorVersion",
		Custom(v::generic_non_negative_int_validator),
	),
	(
		"woffMetadataUniqueID",
		Custom(v::font_info_woff_metadata_unique_id_validator),
	),
	(
		"woffMetadataVendor",
		Custom(v::font_info_woff_metadata_vendor_validator),
	),
	(
		"woffMetadataCredits",
		Custom(v::font_info_woff_metadata_credits_validator),
	),
	(
		"woffMetadataDescription",
		Custom(v::font_info_woff_metadata_description_validator),
	),
	(
		"woffMetadataLicense",
		Custom(v::font_info_woff_metadata_license_validator),
	),
	(
		"woffMetadataCopyright",
		Custom(v::font_info_woff_metadata_copyright_validator),
	),
	(
		"woffMetadataTrademark",
		Custom(v::font_info_woff_metadata_trademark_validator),
	),
	(
		"woffMetadataLicensee",
		Custom(v::font_info_woff_metadata_licensee_validator),
	),
	(
		"woffMetadataExtensions",
		Custom(v::font_info_woff_metadata_extensions_validator),
	),
	("guidelines", Custom(guidelines_validator)),
];

fn guidelines_validator(value: &Value) -> bool {
	v::guidelines_validator(value, None)
}

/// `fontInfoAttributesVersion3ValueData`, in order: version 2 updated with
/// the version 3 entries.
pub fn font_info_attributes_version3_value_data() -> Vec<(&'static str, InfoValidator)> {
	let mut data: IndexMap<&'static str, InfoValidator> = FONT_INFO_ATTRIBUTES_VERSION2_VALUE_DATA
		.iter()
		.copied()
		.collect();
	for &(attr, validator) in VERSION3_UPDATES {
		data.insert(attr, validator);
	}
	data.into_iter().collect()
}

/// Whether `attr` is a version 2 attribute (`fontInfoAttributesVersion2`).
pub fn is_version2_attribute(attr: &str) -> bool {
	FONT_INFO_ATTRIBUTES_VERSION2_VALUE_DATA
		.iter()
		.any(|(a, _)| *a == attr)
}

/// Whether `attr` is a version 3 attribute (`fontInfoAttributesVersion3`).
pub fn is_version3_attribute(attr: &str) -> bool {
	is_version2_attribute(attr) || VERSION3_UPDATES.iter().any(|(a, _)| *a == attr)
}

fn version2_validator(attr: &str) -> Option<InfoValidator> {
	FONT_INFO_ATTRIBUTES_VERSION2_VALUE_DATA
		.iter()
		.find(|(a, _)| *a == attr)
		.map(|(_, v)| *v)
}

fn version3_validator(attr: &str) -> Option<InfoValidator> {
	VERSION3_UPDATES
		.iter()
		.find(|(a, _)| *a == attr)
		.map(|(_, v)| *v)
		.or_else(|| version2_validator(attr))
}

/// `validateFontInfoVersion2ValueForAttribute`. An unknown attribute is a
/// `KeyError`.
pub fn validate_font_info_version2_value_for_attribute(attr: &str, value: &Value) -> Result<bool> {
	version2_validator(attr)
		.map(|validator| validator.validate(value))
		.ok_or_else(|| UfoLibError::Key(py_str_repr(attr)))
}

/// `validateFontInfoVersion3ValueForAttribute`. An unknown attribute is a
/// `KeyError`.
pub fn validate_font_info_version3_value_for_attribute(attr: &str, value: &Value) -> Result<bool> {
	version3_validator(attr)
		.map(|validator| validator.validate(value))
		.ok_or_else(|| UfoLibError::Key(py_str_repr(attr)))
}

fn py_str_repr(s: &str) -> String {
	fontoxide_misc::py::str_repr(s)
}

/// `validateInfoVersion2Data`.
pub fn validate_info_version2_data(info_data: &Dict) -> Result<Dict> {
	for (attr, value) in info_data {
		if !validate_font_info_version2_value_for_attribute(attr, value)? {
			return ufo_err(format!(
				"Invalid value for attribute {attr} ({}).",
				value_repr(value)
			));
		}
	}
	Ok(info_data.clone())
}

/// `validateInfoVersion3Data`.
pub fn validate_info_version3_data(info_data: &Dict) -> Result<Dict> {
	for (attr, value) in info_data {
		if !validate_font_info_version3_value_for_attribute(attr, value)? {
			return ufo_err(format!(
				"Invalid value for attribute {attr} ({}).",
				value_repr(value)
			));
		}
	}
	Ok(info_data.clone())
}

// Version Conversion Support

/// `fontInfoAttributesVersion1To2`.
pub const FONT_INFO_ATTRIBUTES_VERSION1_TO2: &[(&str, &str)] = &[
	("menuName", "styleMapFamilyName"),
	("designer", "openTypeNameDesigner"),
	("designerURL", "openTypeNameDesignerURL"),
	("createdBy", "openTypeNameManufacturer"),
	("vendorURL", "openTypeNameManufacturerURL"),
	("license", "openTypeNameLicense"),
	("licenseURL", "openTypeNameLicenseURL"),
	("ttVersion", "openTypeNameVersion"),
	("ttUniqueID", "openTypeNameUniqueID"),
	("notice", "openTypeNameDescription"),
	("otFamilyName", "openTypeNamePreferredFamilyName"),
	("otStyleName", "openTypeNamePreferredSubfamilyName"),
	("otMacName", "openTypeNameCompatibleFullName"),
	("weightName", "postscriptWeightName"),
	("weightValue", "openTypeOS2WeightClass"),
	("ttVendor", "openTypeOS2VendorID"),
	("uniqueID", "postscriptUniqueID"),
	("fontName", "postscriptFontName"),
	("fondID", "macintoshFONDFamilyID"),
	("fondName", "macintoshFONDName"),
	("defaultWidth", "postscriptDefaultWidthX"),
	("slantAngle", "postscriptSlantAngle"),
	("fullName", "postscriptFullName"),
	// require special value conversion
	("fontStyle", "styleMapStyleName"),
	("widthName", "openTypeOS2WidthClass"),
	("msCharSet", "postscriptWindowsCharacterSet"),
];

/// `deprecatedFontInfoAttributesVersion2`.
pub fn deprecated_font_info_attributes_version2() -> Vec<&'static str> {
	FONT_INFO_ATTRIBUTES_VERSION1_TO2
		.iter()
		.map(|(a, _)| *a)
		.collect()
}

const FONT_STYLE_1_TO_2: &[(i64, &str)] = &[
	(64, "regular"),
	(1, "italic"),
	(32, "bold"),
	(33, "bold italic"),
	// Some UFO 1 files have 0
	(0, "regular"),
];

const WIDTH_NAME_1_TO_2: &[(&str, i64)] = &[
	("Ultra-condensed", 1),
	("Extra-condensed", 2),
	("Condensed", 3),
	("Semi-condensed", 4),
	("Medium (normal)", 5),
	("Semi-expanded", 6),
	("Expanded", 7),
	("Extra-expanded", 8),
	("Ultra-expanded", 9),
	// FontLab's default width value is "Normal".
	("Normal", 5),
	// FontLab has an "All" width value. In UFO 1 move this up to "Normal".
	("All", 5),
	// "medium" appears in a lot of UFO 1 files.
	("medium", 5),
	// "Medium" appears in a lot of UFO 1 files.
	("Medium", 5),
];

const MS_CHAR_SET_1_TO_2: &[(i64, i64)] = &[
	(0, 1),
	(1, 2),
	(2, 3),
	(77, 4),
	(128, 5),
	(129, 6),
	(130, 7),
	(134, 8),
	(136, 9),
	(161, 10),
	(162, 11),
	(163, 12),
	(177, 13),
	(178, 14),
	(186, 15),
	(200, 16),
	(204, 17),
	(222, 18),
	(238, 19),
	(255, 20),
];

/// A dictionary lookup with an `int` key: Python compares by value, so
/// `1.0` and `True` find `1`.
fn int_key<T: Copy>(table: &[(i64, T)], value: &Value) -> Option<T> {
	let n = Number::from_value(value)?;
	table
		.iter()
		.find(|(k, _)| Number::Int(*k) == n)
		.map(|(_, v)| *v)
}

/// `convertFontInfoValueForAttributeFromVersion1ToVersion2`.
pub fn convert_font_info_value_for_attribute_from_version1_to_version2(
	attr: &str,
	value: &Value,
) -> Result<(String, Value)> {
	let mut value = value.clone();
	// convert floats to ints if possible
	if let Value::Real(f) = value
		&& f.is_finite()
		&& f.trunc() == f
		&& f.abs() < 1e30
	{
		value = Value::Integer(f as i128);
	}
	let cannot = |value: &Value| {
		ufo_err(format!(
			"Cannot convert value ({}) for attribute {attr}.",
			value_repr(value)
		))
	};
	match attr {
		"fontStyle" => match int_key(FONT_STYLE_1_TO_2, &value) {
			Some(v) => value = Value::from(v),
			None => return cannot(&value),
		},
		"widthName" => {
			let found = match &value {
				Value::String(s) => WIDTH_NAME_1_TO_2
					.iter()
					.find(|(k, _)| k == s)
					.map(|(_, v)| *v),
				_ => None,
			};
			match found {
				Some(v) => value = Value::from(v),
				None => return cannot(&value),
			}
		}
		"msCharSet" => match int_key(MS_CHAR_SET_1_TO_2, &value) {
			Some(v) => value = Value::from(v),
			None => return cannot(&value),
		},
		_ => {}
	}
	let attr = FONT_INFO_ATTRIBUTES_VERSION1_TO2
		.iter()
		.find(|(a, _)| *a == attr)
		.map_or(attr, |(_, b)| *b);
	Ok((attr.to_owned(), value))
}

/// `convertFontInfoValueForAttributeFromVersion2ToVersion1`: a value that
/// has no version 1 equivalent becomes `None`.
pub fn convert_font_info_value_for_attribute_from_version2_to_version1(
	attr: &str,
	value: &Value,
) -> (String, Option<Value>) {
	let value = match attr {
		"styleMapStyleName" => match value {
			Value::String(s) => FONT_STYLE_1_TO_2[..4]
				.iter()
				.find(|(_, n)| n == s)
				.map(|(k, _)| Value::from(*k)),
			_ => None,
		},
		"openTypeOS2WidthClass" => int_key(
			&WIDTH_NAME_1_TO_2[..9]
				.iter()
				.map(|(n, k)| (*k, *n))
				.collect::<Vec<_>>(),
			value,
		)
		.map(Value::from),
		"postscriptWindowsCharacterSet" => int_key(
			&MS_CHAR_SET_1_TO_2
				.iter()
				.map(|(a, b)| (*b, *a))
				.collect::<Vec<_>>(),
			value,
		)
		.map(Value::from),
		_ => Some(value.clone()),
	};
	let attr = FONT_INFO_ATTRIBUTES_VERSION1_TO2
		.iter()
		.find(|(_, b)| *b == attr)
		.map_or(attr, |(a, _)| *a);
	(attr.to_owned(), value)
}

/// `_convertFontInfoDataVersion1ToVersion2`.
pub(crate) fn convert_font_info_data_version1_to_version2(data: &Dict) -> Result<Dict> {
	let mut converted = Dict::new();
	for (attr, value) in data {
		// FontLab gives -1 for the weightValue for fonts with no defined value.
		if attr == "weightValue" && Number::from_value(value) == Some(Number::Int(-1)) {
			continue;
		}
		let (new_attr, new_value) =
			convert_font_info_value_for_attribute_from_version1_to_version2(attr, value)?;
		// skip if the attribute is not part of version 2
		if !is_version2_attribute(&new_attr) {
			continue;
		}
		converted.insert(new_attr, new_value);
	}
	Ok(converted)
}

/// `_convertFontInfoDataVersion2ToVersion1`: values without a version 1
/// equivalent are `None`.
pub(crate) fn convert_font_info_data_version2_to_version1(
	data: &Dict,
) -> IndexMap<String, Option<Value>> {
	let mut converted = IndexMap::new();
	for (attr, value) in data {
		let (new_attr, new_value) =
			convert_font_info_value_for_attribute_from_version2_to_version1(attr, value);
		// only take attributes that are registered for version 1
		if !FONT_INFO_ATTRIBUTES_VERSION1.contains(&new_attr.as_str()) {
			continue;
		}
		converted.insert(new_attr, new_value);
	}
	converted
}

const UFO2_TO_3_NON_NEGATIVE_INT: &[&str] = &[
	"versionMinor",
	"openTypeHeadLowestRecPPEM",
	"openTypeOS2WinAscent",
	"openTypeOS2WinDescent",
];

const UFO2_TO_3_FLOAT_TO_INT: &[&str] = &[
	"openTypeHeadLowestRecPPEM",
	"openTypeHheaAscender",
	"openTypeHheaDescender",
	"openTypeHheaLineGap",
	"openTypeHheaCaretOffset",
	"openTypeOS2TypoAscender",
	"openTypeOS2TypoDescender",
	"openTypeOS2TypoLineGap",
	"openTypeOS2WinAscent",
	"openTypeOS2WinDescent",
	"openTypeOS2SubscriptXSize",
	"openTypeOS2SubscriptYSize",
	"openTypeOS2SubscriptXOffset",
	"openTypeOS2SubscriptYOffset",
	"openTypeOS2SuperscriptXSize",
	"openTypeOS2SuperscriptYSize",
	"openTypeOS2SuperscriptXOffset",
	"openTypeOS2SuperscriptYOffset",
	"openTypeOS2StrikeoutSize",
	"openTypeOS2StrikeoutPosition",
	"openTypeVheaVertTypoAscender",
	"openTypeVheaVertTypoDescender",
	"openTypeVheaVertTypoLineGap",
	"openTypeVheaCaretOffset",
];

/// A float that fits an `int` conversion; `int(inf)` and `int(nan)` fail.
fn float_to_int(f: f64) -> Option<i128> {
	(f.is_finite() && f.abs() < 1e37).then_some(f as i128)
}

/// `convertFontInfoValueForAttributeFromVersion2ToVersion3`.
pub fn convert_font_info_value_for_attribute_from_version2_to_version3(
	attr: &str,
	value: &Value,
) -> Result<(String, Value)> {
	let cannot = || ufo_err(format!("Could not convert value for {attr}."));
	let mut value = value.clone();
	if UFO2_TO_3_FLOAT_TO_INT.contains(&attr) {
		// `round(value)`
		value = match value {
			Value::Bool(b) => Value::Integer(i128::from(b)),
			Value::Integer(i) => Value::Integer(i),
			// LENIENT(L35): `round(inf)` raises an OverflowError in fontTools.
			Value::Real(f) => match float_to_int(fontoxide_misc::py::round(f)) {
				Some(i) => Value::Integer(i),
				None => return cannot(),
			},
			_ => return cannot(),
		};
	}
	if UFO2_TO_3_NON_NEGATIVE_INT.contains(&attr) {
		// `int(abs(value))`
		value = match value {
			Value::Bool(b) => Value::Integer(i128::from(b)),
			Value::Integer(i) => Value::Integer(i.abs()),
			Value::Real(f) => match float_to_int(f.abs()) {
				Some(i) => Value::Integer(i),
				None => return cannot(),
			},
			_ => return cannot(),
		};
	} else if attr == "unitsPerEm" {
		// `v = float(abs(value))`, made an `int` if it is integral; the value is
		// replaced if it differs.
		let v = match value {
			Value::Bool(b) => f64::from(u8::from(b)),
			Value::Integer(i) => i.abs() as f64,
			Value::Real(f) => f.abs(),
			_ => return cannot(),
		};
		// LENIENT(L35): `int(inf)` raises an OverflowError in fontTools.
		let Some(as_int) = float_to_int(v) else {
			return cannot();
		};
		let v = if v == as_int as f64 {
			Value::Integer(as_int)
		} else {
			Value::Real(v)
		};
		if v != value {
			value = v;
		}
	}
	Ok((attr.to_owned(), value))
}

/// `convertFontInfoValueForAttributeFromVersion3ToVersion2`: nothing to
/// convert.
pub fn convert_font_info_value_for_attribute_from_version3_to_version2(
	attr: &str,
	value: &Value,
) -> (String, Value) {
	(attr.to_owned(), value.clone())
}

/// `_convertFontInfoDataVersion3ToVersion2`.
pub(crate) fn convert_font_info_data_version3_to_version2(data: &Dict) -> Dict {
	data.iter()
		.filter(|(attr, _)| is_version2_attribute(attr))
		.map(|(a, v)| (a.clone(), v.clone()))
		.collect()
}

/// `_convertFontInfoDataVersion2ToVersion3`.
pub(crate) fn convert_font_info_data_version2_to_version3(data: &Dict) -> Result<Dict> {
	let mut converted = Dict::new();
	for (attr, value) in data {
		let (attr, value) =
			convert_font_info_value_for_attribute_from_version2_to_version3(attr, value)?;
		converted.insert(attr, value);
	}
	Ok(converted)
}
