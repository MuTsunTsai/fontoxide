//! Port of fontTools' `Tests/ufoLib/UFO2_test.py`.
//!
//! The font info read and write tests are tables of the attribute values
//! that fontTools' tests set, as Python literals, and whether reading or
//! writing must fail.

mod common;

use common::{check_read, check_write, font_info_version2, read_plist, tempdir, write_plist};
use fontoxide_ufolib::{Dict, Result, UfoReader, UfoWriter, UfoWriterOptions, Value};

// ReadFontInfoVersion2TestCase

#[test]
fn test_read() -> Result<()> {
	let dir = tempdir("ufo2read");
	let mut meta = Dict::new();
	meta.insert("creator".into(), Value::from("test"));
	meta.insert("formatVersion".into(), Value::from(2));
	write_plist(&dir.join("metainfo.plist"), &Value::Dict(meta));
	let original = font_info_version2();
	write_plist(&dir.join("fontinfo.plist"), &Value::Dict(original.clone()));
	let reader = UfoReader::new(dir.path(), true)?;
	let info = reader.read_info(None)?;
	let read: Dict = original
		.keys()
		.map(|attr| (attr.clone(), info.get(attr).cloned().expect(attr)))
		.collect();
	assert_eq!(original, read);
	Ok(())
}

// WriteFontInfoVersion2TestCase

#[test]
fn test_write() -> Result<()> {
	let dir = tempdir("ufo2write");
	let ufo = dir.join("test.ufo");
	let writer = UfoWriter::new(
		&ufo,
		UfoWriterOptions {
			format_version: Some(2.into()),
			..Default::default()
		},
	)?;
	writer.write_info(&font_info_version2(), None)?;
	let written = read_plist(&ufo.join("fontinfo.plist"));
	let written = written.as_dict().expect("dict");
	for (attr, original) in font_info_version2() {
		assert_eq!(written.get(&attr), Some(&original), "{attr}");
	}
	Ok(())
}

#[test]
fn test_generic_read() -> Result<()> {
	check_read(
		2,
		&font_info_version2(),
		&[
			(&[("familyName", "123")], true),
			(&[("styleName", "123")], true),
			(&[("styleMapFamilyName", "123")], true),
			(&[("styleMapStyleName", "123")], true),
			(&[("styleMapStyleName", "'REGULAR'")], true),
			(&[("versionMajor", "'1'")], true),
			(&[("versionMinor", "'0'")], true),
			(&[("copyright", "123")], true),
			(&[("trademark", "123")], true),
			(&[("unitsPerEm", "'abc'")], true),
			(&[("descender", "'abc'")], true),
			(&[("xHeight", "'abc'")], true),
			(&[("capHeight", "'abc'")], true),
			(&[("ascender", "'abc'")], true),
			(&[("italicAngle", "'abc'")], true),
		],
	)
}

#[test]
fn test_head_read() -> Result<()> {
	check_read(
		2,
		&font_info_version2(),
		&[
			(&[("openTypeHeadCreated", "123")], true),
			(&[("openTypeHeadCreated", "'2000-Jan-01 00:00:00'")], true),
			(&[("openTypeHeadLowestRecPPEM", "'abc'")], true),
			(&[("openTypeHeadFlags", "[-1]")], true),
		],
	)
}

#[test]
fn test_hhea_read() -> Result<()> {
	check_read(
		2,
		&font_info_version2(),
		&[
			(&[("openTypeHheaAscender", "'abc'")], true),
			(&[("openTypeHheaDescender", "'abc'")], true),
			(&[("openTypeHheaLineGap", "'abc'")], true),
			(&[("openTypeHheaCaretSlopeRise", "'abc'")], true),
			(&[("openTypeHheaCaretSlopeRun", "'abc'")], true),
			(&[("openTypeHheaCaretOffset", "'abc'")], true),
		],
	)
}

#[test]
fn test_name_read() -> Result<()> {
	check_read(
		2,
		&font_info_version2(),
		&[
			(&[("openTypeNameDesigner", "123")], true),
			(&[("openTypeNameDesignerURL", "123")], true),
			(&[("openTypeNameManufacturer", "123")], true),
			(&[("openTypeNameManufacturerURL", "123")], true),
			(&[("openTypeNameLicense", "123")], true),
			(&[("openTypeNameLicenseURL", "123")], true),
			(&[("openTypeNameVersion", "123")], true),
			(&[("openTypeNameUniqueID", "123")], true),
			(&[("openTypeNameDescription", "123")], true),
			(&[("openTypeNamePreferredFamilyName", "123")], true),
			(&[("openTypeNamePreferredSubfamilyName", "123")], true),
			(&[("openTypeNameCompatibleFullName", "123")], true),
			(&[("openTypeNameSampleText", "123")], true),
			(&[("openTypeNameWWSFamilyName", "123")], true),
			(&[("openTypeNameWWSSubfamilyName", "123")], true),
		],
	)
}

#[test]
fn test_os2_read() -> Result<()> {
	check_read(
		2,
		&font_info_version2(),
		&[
			(&[("openTypeOS2WidthClass", "'abc'")], true),
			(&[("openTypeOS2WidthClass", "15")], true),
			(&[("openTypeOS2WeightClass", "'abc'")], true),
			(
				&[
					("openTypeOS2WeightClass", "'abc'"),
					("openTypeOS2WeightClass", "-50"),
				],
				true,
			),
			(&[("openTypeOS2Selection", "[-1]")], true),
			(&[("openTypeOS2VendorID", "1234")], true),
			(
				&[("openTypeOS2Panose", "[0, 1, 2, 3, 4, 5, 6, 7, 8, '9']")],
				true,
			),
			(&[("openTypeOS2Panose", "[0, 1, 2, 3]")], true),
			(
				&[("openTypeOS2Panose", "[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]")],
				true,
			),
			(&[("openTypeOS2FamilyClass", "[1, '1']")], true),
			(&[("openTypeOS2FamilyClass", "[1]")], true),
			(&[("openTypeOS2FamilyClass", "[1, 1, 1]")], true),
			(&[("openTypeOS2FamilyClass", "[1, 201]")], true),
			(&[("openTypeOS2UnicodeRanges", "['0']")], true),
			(&[("openTypeOS2UnicodeRanges", "[-1]")], true),
			(&[("openTypeOS2CodePageRanges", "['0']")], true),
			(&[("openTypeOS2CodePageRanges", "[-1]")], true),
			(&[("openTypeOS2TypoAscender", "'abc'")], true),
			(&[("openTypeOS2TypoDescender", "'abc'")], true),
			(&[("openTypeOS2TypoLineGap", "'abc'")], true),
			(&[("openTypeOS2WinAscent", "'abc'")], true),
			(&[("openTypeOS2WinDescent", "'abc'")], true),
			(&[("openTypeOS2Type", "['1']")], true),
			(&[("openTypeOS2Type", "[-1]")], true),
			(&[("openTypeOS2SubscriptXSize", "'abc'")], true),
			(&[("openTypeOS2SubscriptYSize", "'abc'")], true),
			(&[("openTypeOS2SubscriptXOffset", "'abc'")], true),
			(&[("openTypeOS2SubscriptYOffset", "'abc'")], true),
			(&[("openTypeOS2SuperscriptXSize", "'abc'")], true),
			(&[("openTypeOS2SuperscriptYSize", "'abc'")], true),
			(&[("openTypeOS2SuperscriptXOffset", "'abc'")], true),
			(&[("openTypeOS2SuperscriptYOffset", "'abc'")], true),
			(&[("openTypeOS2StrikeoutSize", "'abc'")], true),
			(&[("openTypeOS2StrikeoutPosition", "'abc'")], true),
		],
	)
}

#[test]
fn test_vhea_read() -> Result<()> {
	check_read(
		2,
		&font_info_version2(),
		&[
			(&[("openTypeVheaVertTypoAscender", "'abc'")], true),
			(&[("openTypeVheaVertTypoDescender", "'abc'")], true),
			(&[("openTypeVheaVertTypoLineGap", "'abc'")], true),
			(&[("openTypeVheaCaretSlopeRise", "'abc'")], true),
			(&[("openTypeVheaCaretSlopeRun", "'abc'")], true),
			(&[("openTypeVheaCaretOffset", "'abc'")], true),
		],
	)
}

#[test]
fn test_fond_read() -> Result<()> {
	check_read(
		2,
		&font_info_version2(),
		&[
			(&[("macintoshFONDFamilyID", "'abc'")], true),
			(&[("macintoshFONDName", "123")], true),
		],
	)
}

#[test]
fn test_postscript_read() -> Result<()> {
	check_read(
		2,
		&font_info_version2(),
		&[
			(&[("postscriptFontName", "123")], true),
			(&[("postscriptFullName", "123")], true),
			(&[("postscriptSlantAngle", "'abc'")], true),
			(&[("postscriptUniqueID", "'abc'")], true),
			(&[("postscriptUnderlineThickness", "'abc'")], true),
			(&[("postscriptUnderlinePosition", "'abc'")], true),
			(&[("postscriptIsFixedPitch", "2")], true),
			(&[("postscriptBlueValues", "'abc'")], true),
			(&[("postscriptBlueValues", "[500]")], true),
			(
				&[(
					"postscriptBlueValues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptOtherBlues", "'abc'")], true),
			(&[("postscriptOtherBlues", "[500]")], true),
			(
				&[(
					"postscriptOtherBlues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptFamilyBlues", "'abc'")], true),
			(&[("postscriptFamilyBlues", "[500]")], true),
			(
				&[(
					"postscriptFamilyBlues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptFamilyOtherBlues", "'abc'")], true),
			(&[("postscriptFamilyOtherBlues", "[500]")], true),
			(
				&[(
					"postscriptFamilyOtherBlues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptStemSnapH", "'abc'")], true),
			(
				&[(
					"postscriptStemSnapH",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptStemSnapV", "'abc'")], true),
			(
				&[(
					"postscriptStemSnapV",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptBlueFuzz", "'abc'")], true),
			(&[("postscriptBlueShift", "'abc'")], true),
			(&[("postscriptBlueScale", "'abc'")], true),
			(&[("postscriptForceBold", "'abc'")], true),
			(&[("postscriptDefaultWidthX", "'abc'")], true),
			(&[("postscriptNominalWidthX", "'abc'")], true),
			(&[("postscriptWeightName", "123")], true),
			(&[("postscriptDefaultCharacter", "123")], true),
			(&[("postscriptWindowsCharacterSet", "-1")], true),
			(&[("macintoshFONDFamilyID", "'abc'")], true),
			(&[("macintoshFONDName", "123")], true),
		],
	)
}

#[test]
fn test_generic_write() -> Result<()> {
	check_write(
		2,
		&font_info_version2(),
		false,
		&[
			(&[("familyName", "123")], true),
			(&[("styleName", "123")], true),
			(&[("styleMapFamilyName", "123")], true),
			(&[("styleMapStyleName", "123")], true),
			(&[("styleMapStyleName", "'REGULAR'")], true),
			(&[("versionMajor", "'1'")], true),
			(&[("versionMinor", "'0'")], true),
			(&[("copyright", "123")], true),
			(&[("trademark", "123")], true),
			(&[("unitsPerEm", "'abc'")], true),
			(&[("descender", "'abc'")], true),
			(&[("xHeight", "'abc'")], true),
			(&[("capHeight", "'abc'")], true),
			(&[("ascender", "'abc'")], true),
			(&[("italicAngle", "'abc'")], true),
		],
	)
}

#[test]
fn test_head_write() -> Result<()> {
	check_write(
		2,
		&font_info_version2(),
		false,
		&[
			(&[("openTypeHeadCreated", "123")], true),
			(&[("openTypeHeadCreated", "'2000-Jan-01 00:00:00'")], true),
			(&[("openTypeHeadLowestRecPPEM", "'abc'")], true),
			(&[("openTypeHeadFlags", "[-1]")], true),
		],
	)
}

#[test]
fn test_hhea_write() -> Result<()> {
	check_write(
		2,
		&font_info_version2(),
		false,
		&[
			(&[("openTypeHheaAscender", "'abc'")], true),
			(&[("openTypeHheaDescender", "'abc'")], true),
			(&[("openTypeHheaLineGap", "'abc'")], true),
			(&[("openTypeHheaCaretSlopeRise", "'abc'")], true),
			(&[("openTypeHheaCaretSlopeRun", "'abc'")], true),
			(&[("openTypeHheaCaretOffset", "'abc'")], true),
		],
	)
}

#[test]
fn test_name_write() -> Result<()> {
	check_write(
		2,
		&font_info_version2(),
		false,
		&[
			(&[("openTypeNameDesigner", "123")], true),
			(&[("openTypeNameDesignerURL", "123")], true),
			(&[("openTypeNameManufacturer", "123")], true),
			(&[("openTypeNameManufacturerURL", "123")], true),
			(&[("openTypeNameLicense", "123")], true),
			(&[("openTypeNameLicenseURL", "123")], true),
			(&[("openTypeNameVersion", "123")], true),
			(&[("openTypeNameUniqueID", "123")], true),
			(&[("openTypeNameDescription", "123")], true),
			(&[("openTypeNamePreferredFamilyName", "123")], true),
			(&[("openTypeNamePreferredSubfamilyName", "123")], true),
			(&[("openTypeNameCompatibleFullName", "123")], true),
			(&[("openTypeNameSampleText", "123")], true),
			(&[("openTypeNameWWSFamilyName", "123")], true),
			(&[("openTypeNameWWSSubfamilyName", "123")], true),
		],
	)
}

#[test]
fn test_os2_write() -> Result<()> {
	check_write(
		2,
		&font_info_version2(),
		false,
		&[
			(&[("openTypeOS2WidthClass", "'abc'")], true),
			(&[("openTypeOS2WidthClass", "15")], true),
			(&[("openTypeOS2WeightClass", "'abc'")], true),
			(
				&[
					("openTypeOS2WeightClass", "'abc'"),
					("openTypeOS2WeightClass", "-50"),
				],
				true,
			),
			(&[("openTypeOS2Selection", "[-1]")], true),
			(&[("openTypeOS2VendorID", "1234")], true),
			(
				&[("openTypeOS2Panose", "[0, 1, 2, 3, 4, 5, 6, 7, 8, '9']")],
				true,
			),
			(&[("openTypeOS2Panose", "[0, 1, 2, 3]")], true),
			(
				&[("openTypeOS2Panose", "[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]")],
				true,
			),
			(&[("openTypeOS2FamilyClass", "[0, '1']")], true),
			(&[("openTypeOS2FamilyClass", "[1]")], true),
			(&[("openTypeOS2FamilyClass", "[1, 1, 1]")], true),
			(&[("openTypeOS2FamilyClass", "[1, 20]")], true),
			(&[("openTypeOS2UnicodeRanges", "['0']")], true),
			(&[("openTypeOS2UnicodeRanges", "[-1]")], true),
			(&[("openTypeOS2CodePageRanges", "['0']")], true),
			(&[("openTypeOS2CodePageRanges", "[-1]")], true),
			(&[("openTypeOS2TypoAscender", "'abc'")], true),
			(&[("openTypeOS2TypoDescender", "'abc'")], true),
			(&[("openTypeOS2TypoLineGap", "'abc'")], true),
			(&[("openTypeOS2WinAscent", "'abc'")], true),
			(&[("openTypeOS2WinDescent", "'abc'")], true),
			(&[("openTypeOS2Type", "['1']")], true),
			(&[("openTypeOS2Type", "[-1]")], true),
			(&[("openTypeOS2SubscriptXSize", "'abc'")], true),
			(&[("openTypeOS2SubscriptYSize", "'abc'")], true),
			(&[("openTypeOS2SubscriptXOffset", "'abc'")], true),
			(&[("openTypeOS2SubscriptYOffset", "'abc'")], true),
			(&[("openTypeOS2SuperscriptXSize", "'abc'")], true),
			(&[("openTypeOS2SuperscriptYSize", "'abc'")], true),
			(&[("openTypeOS2SuperscriptXOffset", "'abc'")], true),
			(&[("openTypeOS2SuperscriptYOffset", "'abc'")], true),
			(&[("openTypeOS2StrikeoutSize", "'abc'")], true),
			(&[("openTypeOS2StrikeoutPosition", "'abc'")], true),
		],
	)
}

#[test]
fn test_vhea_write() -> Result<()> {
	check_write(
		2,
		&font_info_version2(),
		false,
		&[
			(&[("openTypeVheaVertTypoAscender", "'abc'")], true),
			(&[("openTypeVheaVertTypoDescender", "'abc'")], true),
			(&[("openTypeVheaVertTypoLineGap", "'abc'")], true),
			(&[("openTypeVheaCaretSlopeRise", "'abc'")], true),
			(&[("openTypeVheaCaretSlopeRun", "'abc'")], true),
			(&[("openTypeVheaCaretOffset", "'abc'")], true),
		],
	)
}

#[test]
fn test_fond_write() -> Result<()> {
	check_write(
		2,
		&font_info_version2(),
		false,
		&[
			(&[("macintoshFONDFamilyID", "'abc'")], true),
			(&[("macintoshFONDName", "123")], true),
		],
	)
}

#[test]
fn test_postscript_write() -> Result<()> {
	check_write(
		2,
		&font_info_version2(),
		false,
		&[
			(&[("postscriptFontName", "123")], true),
			(&[("postscriptFullName", "123")], true),
			(&[("postscriptSlantAngle", "'abc'")], true),
			(&[("postscriptUniqueID", "'abc'")], true),
			(&[("postscriptUnderlineThickness", "'abc'")], true),
			(&[("postscriptUnderlinePosition", "'abc'")], true),
			(&[("postscriptIsFixedPitch", "2")], true),
			(&[("postscriptBlueValues", "'abc'")], true),
			(&[("postscriptBlueValues", "[500]")], true),
			(
				&[(
					"postscriptBlueValues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptOtherBlues", "'abc'")], true),
			(&[("postscriptOtherBlues", "[500]")], true),
			(
				&[(
					"postscriptOtherBlues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptFamilyBlues", "'abc'")], true),
			(&[("postscriptFamilyBlues", "[500]")], true),
			(
				&[(
					"postscriptFamilyBlues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptFamilyOtherBlues", "'abc'")], true),
			(&[("postscriptFamilyOtherBlues", "[500]")], true),
			(
				&[(
					"postscriptFamilyOtherBlues",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptStemSnapH", "'abc'")], true),
			(
				&[(
					"postscriptStemSnapH",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptStemSnapV", "'abc'")], true),
			(
				&[(
					"postscriptStemSnapV",
					"[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]",
				)],
				true,
			),
			(&[("postscriptBlueFuzz", "'abc'")], true),
			(&[("postscriptBlueShift", "'abc'")], true),
			(&[("postscriptBlueScale", "'abc'")], true),
			(&[("postscriptForceBold", "'abc'")], true),
			(&[("postscriptDefaultWidthX", "'abc'")], true),
			(&[("postscriptNominalWidthX", "'abc'")], true),
			(&[("postscriptWeightName", "123")], true),
			(&[("postscriptDefaultCharacter", "123")], true),
			(&[("postscriptWindowsCharacterSet", "-1")], true),
			(&[("macintoshFONDFamilyID", "'abc'")], true),
			(&[("macintoshFONDName", "123")], true),
		],
	)
}
