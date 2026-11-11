//! The doctests of `fontTools.ufoLib` (`makeUFOPath`), `converters`,
//! `glifLib`, `kerning` and `validators`. Those of `filenames` are in
//! `filenames.rs`; the `deprecated` decorator of `utils` has no Rust
//! counterpart.

mod common;

use common::{py_dict, py_value};
use fontoxide_ufolib::converters::{
	convert_ufo1_or_ufo2_kerning_to_ufo3_kerning, find_known_kerning_groups,
};
use fontoxide_ufolib::glif_lib::number;
use fontoxide_ufolib::kerning::{glyphs_to_groups, lookup_kerning_value};
use fontoxide_ufolib::validators::{
	color_validator, font_lib_validator, glyph_lib_validator, groups_validator,
	identifier_validator, kerning_validator,
};
use fontoxide_ufolib::{
	Groups, Kerning, KerningNested, Number, Result, Value, groups_to_value,
	kerning_nested_to_value, make_ufo_path,
};

fn groups(text: &str) -> Groups {
	py_dict(text)
		.into_iter()
		.map(|(k, v)| {
			let Value::Array(a) = v else {
				panic!("list");
			};
			(
				k,
				a.iter()
					.map(|g| g.as_str().expect("str").to_owned())
					.collect(),
			)
		})
		.collect()
}

fn kerning_nested(text: &str) -> KerningNested {
	py_dict(text)
		.into_iter()
		.map(|(k, v)| {
			let Value::Dict(d) = v else {
				panic!("dict");
			};
			(k, d.into_iter().collect())
		})
		.collect()
}

#[test]
fn doctest_make_ufo_path() {
	let join = |a: &str, b: &str| {
		std::path::Path::new(a)
			.join(b)
			.to_string_lossy()
			.into_owned()
	};
	assert_eq!(
		make_ufo_path("directory/something.ext"),
		join("directory", "something.ufo")
	);
	assert_eq!(
		make_ufo_path("directory/something.another.thing.ext"),
		join("directory", "something.another.thing.ufo")
	);
}

#[test]
fn doctest_find_known_kerning_groups() {
	let test_groups: indexmap::IndexMap<String, ()> = [
		"@MMK_L_1", "@MMK_L_2", "@MMK_L_3", "@MMK_R_1", "@MMK_R_2", "@MMK_R_3", "@MMK_l_1",
		"@MMK_r_1", "@MMK_X_1", "foo",
	]
	.iter()
	.map(|k| ((*k).to_owned(), ()))
	.collect();
	let (first, second) = find_known_kerning_groups(&test_groups);
	let mut first: Vec<_> = first.into_iter().collect();
	let mut second: Vec<_> = second.into_iter().collect();
	first.sort();
	second.sort();
	assert_eq!(first, ["@MMK_L_1", "@MMK_L_2", "@MMK_L_3"]);
	assert_eq!(second, ["@MMK_R_1", "@MMK_R_2", "@MMK_R_3"]);
}

fn check_conversion(
	kerning: &str,
	test_groups: &str,
	expected_kerning: &str,
	expected_groups: &str,
) -> KerningNested {
	let (kerning, groups, _maps) = convert_ufo1_or_ufo2_kerning_to_ufo3_kerning(
		&kerning_nested(kerning),
		groups(test_groups),
		&|_| false,
	);
	assert_eq!(
		kerning_nested_to_value(&kerning),
		py_value(expected_kerning)
	);
	assert_eq!(groups_to_value(&groups), py_value(expected_groups));
	kerning
}

const EXPECTED_KERNING: &str = r#"{
	"A" : {"A": 1, "B": 2, "public.kern2.CGroup": 3, "public.kern2.DGroup": 4},
	"public.kern1.BGroup": {"A": 5, "B": 6, "public.kern2.CGroup": 7, "public.kern2.DGroup": 8},
	"public.kern1.CGroup": {"A": 9, "B": 10, "public.kern2.CGroup": 11, "public.kern2.DGroup": 12},
}"#;

/// `converters.test`.
#[test]
fn doctest_converters_test() {
	// No known prefixes.
	let _ = check_conversion(
		r#"{
			"A" : {"A" : 1, "B" : 2, "CGroup" : 3, "DGroup" : 4},
			"BGroup" : {"A" : 5, "B" : 6, "CGroup" : 7, "DGroup" : 8},
			"CGroup" : {"A" : 9, "B" : 10, "CGroup" : 11, "DGroup" : 12},
		}"#,
		r#"{"BGroup" : ["B"], "CGroup" : ["C"], "DGroup" : ["D"]}"#,
		EXPECTED_KERNING,
		r#"{
			"BGroup": ["B"],
			"CGroup": ["C"],
			"DGroup": ["D"],
			"public.kern1.BGroup": ["B"],
			"public.kern1.CGroup": ["C"],
			"public.kern2.CGroup": ["C"],
			"public.kern2.DGroup": ["D"],
		}"#,
	);
	// Known prefixes.
	let converted = check_conversion(
		r#"{
		"A" : {"A" : 1, "B" : 2, "@MMK_R_CGroup" : 3, "@MMK_R_DGroup" : 4},
		"@MMK_L_BGroup" : {"A" : 5, "B" : 6, "@MMK_R_CGroup" : 7, "@MMK_R_DGroup" : 8},
		"@MMK_L_CGroup" : {"A" : 9, "B" : 10, "@MMK_R_CGroup" : 11, "@MMK_R_DGroup" : 12},
	}"#,
		r#"{
			"@MMK_L_BGroup" : ["B"],
			"@MMK_L_CGroup" : ["C"],
			"@MMK_L_XGroup" : ["X"],
			"@MMK_R_CGroup" : ["C"],
			"@MMK_R_DGroup" : ["D"],
			"@MMK_R_XGroup" : ["X"],
		}"#,
		EXPECTED_KERNING,
		r#"{
			"@MMK_L_BGroup": ["B"],
			"@MMK_L_CGroup": ["C"],
			"@MMK_L_XGroup": ["X"],
			"@MMK_R_CGroup": ["C"],
			"@MMK_R_DGroup": ["D"],
			"@MMK_R_XGroup": ["X"],
			"public.kern1.BGroup": ["B"],
			"public.kern1.CGroup": ["C"],
			"public.kern1.XGroup": ["X"],
			"public.kern2.CGroup": ["C"],
			"public.kern2.DGroup": ["D"],
			"public.kern2.XGroup": ["X"],
		}"#,
	);
	assert_eq!(
		kerning_validator(&kerning_nested_to_value(&converted)),
		Ok(())
	);
	// Mixture of known prefixes and groups without prefixes.
	let _ = check_conversion(
		r#"{
			"A" : {"A" : 1, "B" : 2, "@MMK_R_CGroup" : 3, "DGroup" : 4},
			"BGroup" : {"A" : 5, "B" : 6, "@MMK_R_CGroup" : 7, "DGroup" : 8},
			"@MMK_L_CGroup" : {"A" : 9, "B" : 10, "@MMK_R_CGroup" : 11, "DGroup" : 12},
		}"#,
		r#"{"BGroup" : ["B"], "@MMK_L_CGroup" : ["C"], "@MMK_R_CGroup" : ["C"], "DGroup" : ["D"]}"#,
		EXPECTED_KERNING,
		r#"{
			"BGroup": ["B"],
			"@MMK_L_CGroup": ["C"],
			"@MMK_R_CGroup": ["C"],
			"DGroup": ["D"],
			"public.kern1.BGroup": ["B"],
			"public.kern1.CGroup": ["C"],
			"public.kern2.CGroup": ["C"],
			"public.kern2.DGroup": ["D"],
		}"#,
	);
}

/// `glifLib._number`.
#[test]
fn doctest_number() {
	assert!(matches!(number("1"), Ok(Number::Int(1))));
	assert!(matches!(number("1.0"), Ok(Number::Float(f)) if f == 1.0));
	assert!(matches!(number("a"), Err(e) if e.is_glif_lib_error()));
}

fn kerning_and_groups() -> (Kerning, Groups) {
	let groups = groups(r#"{"public.kern1.O" : ["O", "D", "Q"], "public.kern2.E" : ["E", "F"]}"#);
	let kerning: Kerning = [
		("public.kern1.O", "public.kern2.E", -100),
		("public.kern1.O", "F", -200),
		("D", "F", -300),
	]
	.iter()
	.map(|(a, b, v)| (((*a).to_owned(), (*b).to_owned()), Value::from(*v)))
	.collect();
	(kerning, groups)
}

#[test]
fn doctest_lookup_kerning_value() -> Result<()> {
	let (kerning, groups) = kerning_and_groups();
	let lookup =
		|a: &str, b: &str| lookup_kerning_value((a, b), &kerning, &groups, Value::from(0), None);
	assert_eq!(lookup("D", "F")?, Value::from(-300));
	assert_eq!(lookup("O", "F")?, Value::from(-200));
	assert_eq!(lookup("O", "E")?, Value::from(-100));
	assert_eq!(lookup("O", "O")?, Value::from(0));
	assert_eq!(lookup("E", "E")?, Value::from(0));
	assert_eq!(lookup("E", "O")?, Value::from(0));
	assert_eq!(lookup("X", "X")?, Value::from(0));
	assert_eq!(
		lookup("public.kern1.O", "public.kern2.E")?,
		Value::from(-100)
	);
	assert_eq!(lookup("public.kern1.O", "F")?, Value::from(-200));
	assert_eq!(lookup("O", "public.kern2.E")?, Value::from(-100));
	assert_eq!(lookup("public.kern1.X", "public.kern2.X")?, Value::from(0));
	let (first, second) = glyphs_to_groups(&groups);
	assert_eq!(
		lookup_kerning_value(
			("O", "E"),
			&kerning,
			&groups,
			Value::from(0),
			Some((&first, &second))
		)?,
		Value::from(-100)
	);
	Ok(())
}

#[test]
fn doctest_glyphs_to_groups() {
	let (_, groups) = kerning_and_groups();
	let (first, second) = glyphs_to_groups(&groups);
	let pairs = |m: &indexmap::IndexMap<String, String>| -> Vec<(String, String)> {
		m.iter().map(|(a, b)| (a.clone(), b.clone())).collect()
	};
	let owned = |v: &[(&str, &str)]| -> Vec<(String, String)> {
		v.iter()
			.map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
			.collect()
	};
	assert_eq!(
		pairs(&first),
		owned(&[
			("O", "public.kern1.O"),
			("D", "public.kern1.O"),
			("Q", "public.kern1.O")
		])
	);
	assert_eq!(
		pairs(&second),
		owned(&[("E", "public.kern2.E"), ("F", "public.kern2.E")])
	);
}

#[test]
fn doctest_color_validator() {
	for (color, valid) in [
		("0,0,0,0", true),
		(".5,.5,.5,.5", true),
		("0.5,0.5,0.5,0.5", true),
		("1,1,1,1", true),
		("2,0,0,0", false),
		("0,2,0,0", false),
		("0,0,2,0", false),
		("0,0,0,2", false),
		("1r,1,1,1", false),
		("1,1g,1,1", false),
		("1,1,1b,1", false),
		("1,1,1,1a", false),
		("1 1 1 1", false),
		("1 1,1,1", false),
		("1,1 1,1", false),
		("1,1,1 1", false),
		("1, 1, 1, 1", true),
	] {
		assert_eq!(color_validator(&Value::from(color)), valid, "{color}");
	}
}

#[test]
fn doctest_identifier_validator() {
	assert!(identifier_validator(&Value::from("a")));
	assert!(!identifier_validator(&Value::from("")));
	assert!(!identifier_validator(&Value::from("a".repeat(101))));
}

#[test]
fn doctest_groups_validator() {
	let check = |text: &str| groups_validator(&py_value(text));
	assert_eq!(check(r#"{"A" : ["A", "A"], "A2" : ["A"]}"#), Ok(()));
	assert_eq!(
		check(r#"{"" : ["A"]}"#),
		Err("A group has an empty name.".into())
	);
	assert_eq!(check(r#"{"public.awesome" : ["A"]}"#), Ok(()));
	let incomplete = Err("The group data contains a kerning group with an incomplete name.".into());
	assert_eq!(check(r#"{"public.kern1." : ["A"]}"#), incomplete);
	assert_eq!(check(r#"{"public.kern2." : ["A"]}"#), incomplete);
	assert_eq!(
		check(r#"{"public.kern1.A" : ["A"], "public.kern2.A" : ["A"]}"#),
		Ok(())
	);
	assert_eq!(
		check(r#"{"public.kern1.A1" : ["A"], "public.kern1.A2" : ["A"]}"#),
		Err("The glyph \"A\" occurs in too many kerning groups.".into())
	);
}

#[test]
fn doctest_kerning_validator() {
	assert_eq!(
		kerning_validator(&py_value(r#"{"A" : {"B" : 100}}"#)),
		Ok(())
	);
	let bogus = Err("The kerning data is not in the correct format.".into());
	assert_eq!(kerning_validator(&py_value(r#"{"A" : ["B"]}"#)), bogus);
	assert_eq!(
		kerning_validator(&py_value(r#"{"A" : {"B" : "100"}}"#)),
		bogus
	);
}

#[test]
fn doctest_font_lib_validator() {
	assert_eq!(font_lib_validator(&py_value(r#"{"foo" : "bar"}"#)), Ok(()));
	assert_eq!(
		font_lib_validator(&py_value(r#"{"public.awesome" : "hello"}"#)),
		Ok(())
	);
	assert_eq!(
		font_lib_validator(&py_value(r#"{"public.glyphOrder" : ["A", "C", "B"]}"#)),
		Ok(())
	);
	let message = font_lib_validator(&py_value(r#""hello""#)).expect_err("str");
	assert!(
		message.starts_with("The lib data is not in the correct format: expected a dictionary, ")
	);
	// `{1: "hello"}`: keys are strings in Rust
	let message =
		font_lib_validator(&py_value(r#"{"public.glyphOrder" : "hello"}"#)).expect_err("str");
	assert!(
		message.starts_with("public.glyphOrder is not properly formatted: expected list or tuple,")
	);
	let message =
		font_lib_validator(&py_value(r#"{"public.glyphOrder" : ["A", 1, "B"]}"#)).expect_err("int");
	assert!(message.starts_with("public.glyphOrder is not properly formatted: expected str,"));
}

#[test]
fn doctest_glyph_lib_validator() {
	assert_eq!(glyph_lib_validator(&py_value(r#"{"foo" : "bar"}"#)), Ok(()));
	assert_eq!(
		glyph_lib_validator(&py_value(r#"{"public.awesome" : "hello"}"#)),
		Ok(())
	);
	assert_eq!(
		glyph_lib_validator(&py_value(r#"{"public.markColor" : "1,0,0,0.5"}"#)),
		Ok(())
	);
	assert_eq!(
		glyph_lib_validator(&py_value(r#"{"public.markColor" : 1}"#)),
		Err("public.markColor is not properly formatted.".into())
	);
}
