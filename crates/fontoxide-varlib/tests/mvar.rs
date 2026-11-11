//! `Tests/varLib/mvar_test.py`.

#![allow(clippy::field_reassign_with_default)]

use fontoxide_ttlib::Font;
use fontoxide_ttlib::tables::{Hhea, Vhea};
use fontoxide_varlib::mvar::get_mvar_table_tag;

#[test]
fn test_get_mvar_table_tag_prefers_uppercase_companions() {
	let mut font = Font::new();
	font.insert(Box::new(Hhea::default()));
	let mut upper = Hhea::default();
	upper.extended = true;
	font.insert(Box::new(upper));
	font.insert(Box::new(Vhea::default()));
	let mut upper = Vhea::default();
	upper.extended = true;
	font.insert(Box::new(upper));
	assert_eq!(get_mvar_table_tag(&font, "hhea"), "HHEA");
	assert_eq!(get_mvar_table_tag(&font, "vhea"), "VHEA");
	assert_eq!(get_mvar_table_tag(&font, "OS/2"), "OS/2");
}

#[test]
fn test_get_mvar_table_tag_keeps_lowercase_without_companions() {
	let mut font = Font::new();
	font.insert(Box::new(Hhea::default()));
	font.insert(Box::new(Vhea::default()));
	assert_eq!(get_mvar_table_tag(&font, "hhea"), "hhea");
	assert_eq!(get_mvar_table_tag(&font, "vhea"), "vhea");
}
