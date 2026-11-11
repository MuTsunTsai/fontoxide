//! `Tests/unicodedata_test.py`, and the doctests of `fontTools.unicodedata`.

use std::collections::BTreeSet;

use fontoxide_unicodedata::{
	HorizDirection, block, mirrored, ot_tag_to_script, ot_tags_from_script, script, script_code,
	script_extension, script_horizontal_direction, script_name,
};

fn set<const N: usize>(items: [&'static str; N]) -> BTreeSet<&'static str> {
	BTreeSet::from(items)
}

#[test]
fn test_script() {
	assert_eq!(script('a' as u32), "Latn");
	// these were randomly sampled, one character per script
	let cases: &[(u32, &str)] = &[
		(0x0378, "Zzzz"),
		(0x10FFFF, "Zzzz"),
		(0x1E918, "Adlm"),
		(0x1170D, "Ahom"),
		(0x145A0, "Hluw"),
		(0x0607, "Arab"),
		(0x056C, "Armn"),
		(0x10B27, "Avst"),
		(0x1B41, "Bali"),
		(0x168AD, "Bamu"),
		(0x16ADD, "Bass"),
		(0x1BE5, "Batk"),
		(0x09F3, "Beng"),
		(0x11C5B, "Bhks"),
		(0x3126, "Bopo"),
		(0x1103B, "Brah"),
		(0x2849, "Brai"),
		(0x1A0A, "Bugi"),
		(0x174E, "Buhd"),
		(0x18EE, "Cans"),
		(0x102B7, "Cari"),
		(0x1053D, "Aghb"),
		(0x11123, "Cakm"),
		(0xAA1F, "Cham"),
		(0xAB95, "Cher"),
		(0x1F0C7, "Zyyy"),
		(0x2C85, "Copt"),
		(0x12014, "Xsux"),
		(0x1082E, "Cprt"),
		(0xA686, "Cyrl"),
		(0x10417, "Dsrt"),
		(0x093E, "Deva"),
		(0x1BC4B, "Dupl"),
		(0x1310C, "Egyp"),
		(0x1051C, "Elba"),
		(0x2DA6, "Ethi"),
		(0x10AD, "Geor"),
		(0x2C52, "Glag"),
		(0x10343, "Goth"),
		(0x11371, "Gran"),
		(0x03D0, "Grek"),
		(0x0AAA, "Gujr"),
		(0x0A4C, "Guru"),
		(0x23C9F, "Hani"),
		(0xC259, "Hang"),
		(0x1722, "Hano"),
		(0x108F5, "Hatr"),
		(0x05C2, "Hebr"),
		(0x1B072, "Hira"),
		(0x10847, "Armi"),
		(0x033A, "Zinh"),
		(0x10B66, "Phli"),
		(0x10B4B, "Prti"),
		(0xA98A, "Java"),
		(0x110B2, "Kthi"),
		(0x0CC6, "Knda"),
		(0x3337, "Kana"),
		(0xA915, "Kali"),
		(0x10A2E, "Khar"),
		(0x17AA, "Khmr"),
		(0x11225, "Khoj"),
		(0x112B6, "Sind"),
		(0x0ED7, "Laoo"),
		(0xAB3C, "Latn"),
		(0x1C48, "Lepc"),
		(0x1923, "Limb"),
		(0x1071D, "Lina"),
		(0x100EC, "Linb"),
		(0xA4E9, "Lisu"),
		(0x10284, "Lyci"),
		(0x10926, "Lydi"),
		(0x11161, "Mahj"),
		(0x0D56, "Mlym"),
		(0x0856, "Mand"),
		(0x10AF0, "Mani"),
		(0x11CB0, "Marc"),
		(0x11D28, "Gonm"),
		(0xABDD, "Mtei"),
		(0x1E897, "Mend"),
		(0x109B0, "Merc"),
		(0x10993, "Mero"),
		(0x16F5D, "Plrd"),
		(0x1160B, "Modi"),
		(0x18A8, "Mong"),
		(0x16A48, "Mroo"),
		(0x1128C, "Mult"),
		(0x105B, "Mymr"),
		(0x108AF, "Nbat"),
		(0x19B3, "Talu"),
		(0x1143D, "Newa"),
		(0x07F4, "Nkoo"),
		(0x1B192, "Nshu"),
		(0x169C, "Ogam"),
		(0x1C56, "Olck"),
		(0x10CE9, "Hung"),
		(0x10316, "Ital"),
		(0x10A93, "Narb"),
		(0x1035A, "Perm"),
		(0x103D5, "Xpeo"),
		(0x10A65, "Sarb"),
		(0x10C09, "Orkh"),
		(0x0B60, "Orya"),
		(0x104CF, "Osge"),
		(0x104A8, "Osma"),
		(0x16B12, "Hmng"),
		(0x10879, "Palm"),
		(0x11AF1, "Pauc"),
		(0xA869, "Phag"),
		(0x10909, "Phnx"),
		(0x10B81, "Phlp"),
		(0xA941, "Rjng"),
		(0x16C3, "Runr"),
		(0x0814, "Samr"),
		(0xA88C, "Saur"),
		(0x111C8, "Shrd"),
		(0x1045F, "Shaw"),
		(0x115AD, "Sidd"),
		(0x1D8C0, "Sgnw"),
		(0x0DB9, "Sinh"),
		(0x110F9, "Sora"),
		(0x11A60, "Soyo"),
		(0x1B94, "Sund"),
		(0xA81F, "Sylo"),
		(0x0740, "Syrc"),
		(0x1714, "Tglg"),
		(0x1761, "Tagb"),
		(0x1965, "Tale"),
		(0x1A32, "Lana"),
		(0xAA86, "Tavt"),
		(0x116A5, "Takr"),
		(0x0B8E, "Taml"),
		(0x1754D, "Tang"),
		(0x0C40, "Telu"),
		(0x07A4, "Thaa"),
		(0x0E42, "Thai"),
		(0x0F09, "Tibt"),
		(0x2D3A, "Tfng"),
		(0x114B0, "Tirh"),
		(0x1038B, "Ugar"),
		(0xA585, "Vaii"),
		(0x118CF, "Wara"),
		(0xA066, "Yiii"),
		(0x11A31, "Zanb"),
		(0x11F00, "Kawi"),
	];
	for &(code, expected) in cases {
		assert_eq!(script(code), expected, "U+{code:04X}");
	}
}

#[test]
fn test_unicode_18_scripts() {
	for (first, last, code, name, tag) in [
		(0x125A8, 0x1264B, "Pcun", "Proto Cuneiform", "pcun"),
		(0x18E00, 0x19191, "Jurc", "Jurchen", "jurc"),
		(0x191A0, 0x191D2, "Jurc", "Jurchen", "jurc"),
		(0x3D000, 0x3FC3F, "Seal", "Seal", "seal"),
	] {
		assert_eq!(script(first), code);
		assert_eq!(script(last), code);
		assert_ne!(script(first - 1), code);
		assert_ne!(script(last + 1), code);
		assert_eq!(script_extension(first), set([code]));
		assert_eq!(script_name(code).as_deref(), Some(name));
		assert_eq!(script_code(name), Some(code));
		assert_eq!(script_horizontal_direction(code), Some(HorizDirection::Ltr));
		assert_eq!(ot_tags_from_script(code), [tag]);
		assert_eq!(ot_tag_to_script(tag), Ok(Some(code)));
	}
}

#[test]
fn test_script_extension() {
	assert_eq!(
		script_extension(0x00B7),
		set([
			"Avst", "Cari", "Copt", "Dupl", "Elba", "Geor", "Glag", "Gong", "Goth", "Grek", "Hani",
			"Latn", "Lydi", "Mahj", "Perm", "Shaw",
		])
	);
	assert_eq!(
		script_extension(0x02BC),
		set(["Beng", "Cyrl", "Deva", "Latn", "Lisu", "Thai", "Toto"])
	);
	assert_eq!(script_extension('a' as u32), set(["Latn"]));
	assert_eq!(script_extension(0), set(["Zyyy"]));
	assert_eq!(script_extension(0x0378), set(["Zzzz"]));
	assert_eq!(script_extension(0x10FFFF), set(["Zzzz"]));

	assert_eq!(script_extension(0x0660), set(["Arab", "Thaa", "Yezi"]));
	assert_eq!(
		script_extension(0x0964),
		set([
			"Beng", "Deva", "Dogr", "Gong", "Gonm", "Gran", "Gujr", "Guru", "Knda", "Mahj", "Mlym",
			"Nand", "Orya", "Sind", "Sinh", "Sylo", "Takr", "Taml", "Telu", "Tirh", "Onao",
		])
	);

	assert_eq!(
		script_extension(0x0B83),
		set(["Knda", "Mlym", "Taml", "Telu"])
	);
	assert_eq!(script_extension(0x1CF5), set(["Beng"]));
	assert_eq!(script_extension(0x12550), set(["Pcun", "Xsux"]));
}

#[test]
fn test_script_name() {
	assert_eq!(script_name("Latn").as_deref(), Some("Latin"));
	assert_eq!(script_name("Zyyy").as_deref(), Some("Common"));
	assert_eq!(script_name("Zzzz").as_deref(), Some("Unknown"));
	// underscores in long names are replaced by spaces
	assert_eq!(script_name("Egyp").as_deref(), Some("Egyptian Hieroglyphs"));

	// KeyError, or the default
	assert_eq!(script_name("QQQQ"), None);
	assert_eq!(
		script_name("QQQQ").unwrap_or_else(|| "Unknown".into()),
		"Unknown"
	);
}

#[test]
fn test_script_code() {
	assert_eq!(script_code("Latin"), Some("Latn"));
	assert_eq!(script_code("Common"), Some("Zyyy"));
	assert_eq!(script_code("Unknown"), Some("Zzzz"));
	// case, whitespace, underscores and hyphens are ignored
	assert_eq!(script_code("Egyptian Hieroglyphs"), Some("Egyp"));
	assert_eq!(script_code("Egyptian_Hieroglyphs"), Some("Egyp"));
	assert_eq!(script_code("egyptianhieroglyphs"), Some("Egyp"));
	assert_eq!(script_code("Egyptian-Hieroglyphs"), Some("Egyp"));

	// KeyError, or the default
	assert_eq!(script_code("Does not exist"), None);
	assert_eq!(script_code("Does not exist").unwrap_or("Zzzz"), "Zzzz");
}

#[test]
fn test_block() {
	assert_eq!(block(0x00), "Basic Latin");
	assert_eq!(block(0x7F), "Basic Latin");
	assert_eq!(block(0x80), "Latin-1 Supplement");
	assert_eq!(block(0x1C90), "Georgian Extended");
	assert_eq!(block(0x0870), "Arabic Extended-B");
	assert_eq!(block(0x11B00), "Devanagari Extended-A");
}

#[test]
fn test_unicode_18_blocks() {
	for (first, last, name) in [
		(0x11DF0, 0x11DFF, "Bengali Supplement"),
		(0x12550, 0x1268F, "Archaic Cuneiform Numerals"),
		(0x18E00, 0x1919F, "Jurchen"),
		(0x191A0, 0x191DF, "Jurchen Radicals"),
		(0x1D250, 0x1D28F, "Musical Symbols Supplement"),
		(
			0x1DB00,
			0x1DBFF,
			"Miscellaneous Symbols and Arrows Extended",
		),
		(0x3D000, 0x3FC3F, "Seal"),
	] {
		assert_eq!(block(first), name);
		assert_eq!(block(last), name);
		assert_ne!(block(first - 1), name);
		assert_ne!(block(last + 1), name);
	}
}

#[test]
fn test_unicode_18_mirrored() {
	for (c, m) in [
		(0x221D, 0x1DB10),
		(0x2E62, 0x2E63),
		(0x1DB03, 0x1DB04),
		(0x1DB05, 0x1DB06),
		(0x1DB08, 0x1DB09),
	] {
		assert_eq!(mirrored(c), Some(m));
		assert_eq!(mirrored(m), Some(c));
	}
}

#[test]
fn test_ot_tags_from_script() {
	// simple
	assert_eq!(ot_tags_from_script("Latn"), ["latn"]);
	// script mapped to multiple new and old script tags
	assert_eq!(ot_tags_from_script("Deva"), ["dev2", "deva"]);
	// exceptions
	assert_eq!(ot_tags_from_script("Hira"), ["kana"]);
	assert_eq!(ot_tags_from_script("Zmth"), ["math"]);
	// special script codes map to DFLT
	assert_eq!(ot_tags_from_script("Zinh"), ["DFLT"]);
	assert_eq!(ot_tags_from_script("Zyyy"), ["DFLT"]);
	assert_eq!(ot_tags_from_script("Zzzz"), ["DFLT"]);
	// this is invalid or unknown
	assert_eq!(ot_tags_from_script("Aaaa"), ["DFLT"]);
}

#[test]
fn test_ot_tag_to_script() {
	let ok = |tag: &str| ot_tag_to_script(tag).expect("valid tag");
	assert_eq!(ok("latn"), Some("Latn"));
	assert_eq!(ok("kana"), Some("Kana"));
	assert_eq!(ok("DFLT"), None);
	assert_eq!(ok("aaaa"), None);
	assert_eq!(ok("beng"), Some("Beng"));
	assert_eq!(ok("bng2"), Some("Beng"));
	assert_eq!(ok("dev2"), Some("Deva"));
	assert_eq!(ok("gjr2"), Some("Gujr"));
	assert_eq!(ok("yi  "), Some("Yiii"));
	assert_eq!(ok("nko "), Some("Nkoo"));
	assert_eq!(ok("vai "), Some("Vaii"));
	assert_eq!(ok("lao "), Some("Laoo"));
	assert_eq!(ok("yi"), Some("Yiii"));
	assert_eq!(ok("math"), Some("Zmth"));
	// both 'hang' and 'jamo' tags map to the Hangul script
	assert_eq!(ok("hang"), Some("Hang"));
	assert_eq!(ok("jamo"), Some("Hang"));

	for invalid_value in ["", " ", "z zz", "zzzzz"] {
		let err = ot_tag_to_script(invalid_value).expect_err("invalid tag");
		assert!(err.to_string().contains("invalid OpenType tag"), "{err}");
	}
	// Checked against fontTools: the message has the repr() of the stripped
	// tag, and uppercasing is Unicode-aware ('ı'.upper() == 'I').
	assert_eq!(
		ot_tag_to_script(" z zz ").map_err(|e| e.to_string()),
		Err("invalid OpenType tag: 'z zz'".to_owned())
	);
	assert_eq!(ok("\u{131}tal"), Some("Ital"));
}

#[test]
fn test_script_horizontal_direction() {
	let dir = |code: &str| script_horizontal_direction(code).map(HorizDirection::as_str);
	assert_eq!(dir("Latn"), Some("LTR"));
	assert_eq!(dir("Arab"), Some("RTL"));
	assert_eq!(dir("Thaa"), Some("RTL"));
	assert_eq!(dir("Ougr"), Some("RTL"));
	assert_eq!(dir("Gara"), Some("RTL"));

	// KeyError, or the default
	assert_eq!(dir("Azzz"), None);
	assert_eq!(dir("Azzz").unwrap_or("LTR"), "LTR");
}

// The doctests of `fontTools.unicodedata`.

#[test]
fn doctest_script() {
	assert_eq!(script('a' as u32), "Latn");
	assert_eq!(script(',' as u32), "Zyyy");
	assert_eq!(script(0x10FFFF), "Zzzz");
}

#[test]
fn doctest_script_extension() {
	assert_eq!(script_extension('a' as u32), set(["Latn"]));
	assert_eq!(
		script_extension(0x060C),
		set(["Nkoo", "Arab", "Rohg", "Thaa", "Syrc", "Gara", "Yezi"])
	);
	assert_eq!(script_extension(0x10FFFF), set(["Zzzz"]));
}

#[test]
fn doctest_block() {
	assert_eq!(block('a' as u32), "Basic Latin");
	assert_eq!(block(0x060C), "Arabic");
	assert_eq!(block(0xEFFFF), "No_Block");
}
