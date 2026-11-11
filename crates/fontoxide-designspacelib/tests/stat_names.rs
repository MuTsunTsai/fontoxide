//! `Tests/designspaceLib/statNames_test.py`.

mod common;

use common::datadir;
use fontoxide_designspacelib::stat_names::{StatNames, get_stat_names};
use fontoxide_designspacelib::{DesignSpaceDocument, LocalizedNames, Result, simple_location};

fn names(items: &[(&str, &str)]) -> LocalizedNames {
	items
		.iter()
		.map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
		.collect()
}

fn stat_names(
	family_names: &[(&str, &str)],
	style_names: &[(&str, &str)],
	post_script_font_name: Option<&str>,
	style_map_family_names: &[(&str, &str)],
	style_map_style_name: Option<&str>,
) -> StatNames {
	StatNames {
		family_names: names(family_names),
		style_names: names(style_names),
		post_script_font_name: post_script_font_name.map(str::to_owned),
		style_map_family_names: names(style_map_family_names),
		style_map_style_name: style_map_style_name.map(str::to_owned),
	}
}

#[test]
fn test_instance_get_stat_names() -> Result<()> {
	let doc = DesignSpaceDocument::from_file(datadir().join("test_v5_sourceserif.designspace"))?;
	assert_eq!(
		get_stat_names(&doc, &doc.instances[0].get_full_user_location(&doc)?)?,
		stat_names(
			&[("en", "Source Serif 4")],
			&[("en", "Caption ExtraLight")],
			Some("SourceSerif4-CaptionExtraLight"),
			&[("en", "Source Serif 4 Caption ExtraLight")],
			Some("regular"),
		)
	);
	Ok(())
}

#[test]
fn test_not_all_ordering_specified_and_translations() -> Result<()> {
	let doc = DesignSpaceDocument::from_file(datadir().join("test_v5.designspace"))?;
	assert_eq!(
		get_stat_names(
			&doc,
			&simple_location([("Weight", 200), ("Width", 125), ("Italic", 1)])
		)?,
		stat_names(
			&[
				("en", "MasterFamilyName"),
				("fr", "Montserrat"),
				("ja", "モンセラート")
			],
			&[
				("fr", "Wide Extra léger Italic"),
				("de", "Wide Extraleicht Italic"),
				("en", "Wide Extra Light Italic"),
			],
			Some("MasterFamilyName-WideExtraLightItalic"),
			&[
				("en", "MasterFamilyName Wide Extra Light"),
				("fr", "Montserrat Wide Extra léger"),
				("de", "MasterFamilyName Wide Extraleicht"),
				("ja", "モンセラート Wide Extra Light"),
			],
			Some("italic"),
		)
	);
	Ok(())
}

#[test]
fn test_detect_ribbi_aktiv() -> Result<()> {
	let doc = DesignSpaceDocument::from_file(datadir().join("test_v5_aktiv.designspace"))?;
	assert_eq!(
		get_stat_names(
			&doc,
			&simple_location([("Weight", 600), ("Width", 125), ("Italic", 1)])
		)?,
		stat_names(
			&[("en", "Aktiv Grotesk")],
			&[("en", "Ex SemiBold Italic")],
			Some("AktivGrotesk-ExSemiBoldItalic"),
			&[("en", "Aktiv Grotesk Ex SemiBold")],
			Some("italic"),
		)
	);
	assert_eq!(
		get_stat_names(
			&doc,
			&simple_location([("Weight", 700), ("Width", 75), ("Italic", 1)])
		)?,
		stat_names(
			&[("en", "Aktiv Grotesk")],
			&[("en", "Cd Bold Italic")],
			Some("AktivGrotesk-CdBoldItalic"),
			&[("en", "Aktiv Grotesk Cd")],
			Some("bold italic"),
		)
	);
	Ok(())
}

#[test]
fn test_detect_ribbi_recursive() -> Result<()> {
	let doc = DesignSpaceDocument::from_file(datadir().join("test_v5.designspace"))?;
	assert_eq!(
		get_stat_names(
			&doc,
			&simple_location([("Weight", 700), ("Width", 125), ("Italic", 1)])
		)?,
		stat_names(
			&[
				("en", "MasterFamilyName"),
				("fr", "Montserrat"),
				("ja", "モンセラート")
			],
			&[("en", "Wide Bold Italic")],
			Some("MasterFamilyName-WideBoldItalic"),
			&[
				("en", "MasterFamilyName Wide"),
				("fr", "Montserrat Wide"),
				("ja", "モンセラート Wide"),
			],
			Some("bold italic"),
		)
	);
	Ok(())
}

/// See https://github.com/googlefonts/ufo2ft/issues/630: when there's no STAT
/// data present, getStatNames shouldn't try making up a PostScript name.
#[test]
fn test_get_stat_names_on_ds4_doesnt_make_up_bad_names() -> Result<()> {
	let doc = DesignSpaceDocument::from_file(datadir().join("DS5BreakTest.designspace"))?;
	assert_eq!(
		get_stat_names(
			&doc,
			&simple_location([("Weight", 600), ("Width", 125), ("Italic", 1)])
		)?,
		stat_names(&[("en", "DS5BreakTest")], &[], None, &[], None)
	);
	Ok(())
}
