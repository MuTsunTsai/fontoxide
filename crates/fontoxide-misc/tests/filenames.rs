//! `Tests/misc/filenames_test.py`, and the doctests of
//! `fontTools.misc.filenames` (the same examples).
#![allow(non_snake_case)]

use std::collections::HashSet;

use fontoxide_misc::filenames::{
	FilenamesError, handle_clash1, handle_clash2, user_name_to_file_name,
};

const NONE: [&str; 0] = [];

fn name(user_name: &str) -> String {
	user_name_to_file_name(user_name, &NONE, "", "").expect("valid name")
}

#[test]
fn test_names() {
	assert_eq!(name("a"), "a");
	assert_eq!(name("A"), "A_");
	assert_eq!(name("AE"), "A_E_");
	assert_eq!(name("Ae"), "A_e");
	assert_eq!(name("ae"), "ae");
	assert_eq!(name("aE"), "aE_");
	assert_eq!(name("a.alt"), "a.alt");
	assert_eq!(name("A.alt"), "A_.alt");
	assert_eq!(name("A.Alt"), "A_.A_lt");
	assert_eq!(name("A.aLt"), "A_.aL_t");
	assert_eq!(name("A.alT"), "A_.alT_");
	assert_eq!(name("T_H"), "T__H_");
	assert_eq!(name("T_h"), "T__h");
	assert_eq!(name("t_h"), "t_h");
	assert_eq!(name("F_F_I"), "F__F__I_");
	assert_eq!(name("f_f_i"), "f_f_i");
	assert_eq!(name("Aacute_V.swash"), "A_acute_V_.swash");
	assert_eq!(name(".notdef"), "_notdef");
	assert_eq!(name("con"), "_con");
	assert_eq!(name("CON"), "C_O_N_");
	assert_eq!(name("con.alt"), "_con.alt");
	assert_eq!(name("alt.con"), "alt._con");
}

#[test]
fn test_prefix_suffix() {
	let prefix = "TEST_PREFIX";
	let suffix = "TEST_SUFFIX";
	let name = "NAME";
	let name_file = "N_A_M_E_";
	assert_eq!(
		user_name_to_file_name(name, &NONE, prefix, suffix),
		Ok(format!("{prefix}{name_file}{suffix}"))
	);
}

#[test]
fn test_collide() {
	let prefix = "TEST_PREFIX";
	let suffix = "TEST_SUFFIX";
	let name = "NAME";
	let name_file = "N_A_M_E_";
	let collision_avoidance1 = "000000000000001";
	let collision_avoidance2 = "000000000000002";
	let mut exist: HashSet<String> = HashSet::new();
	let generated = user_name_to_file_name(name, &exist, prefix, suffix).expect("name");
	exist.insert(generated.to_lowercase());
	assert_eq!(generated, format!("{prefix}{name_file}{suffix}"));
	let generated = user_name_to_file_name(name, &exist, prefix, suffix).expect("name");
	exist.insert(generated.to_lowercase());
	assert_eq!(
		generated,
		format!("{prefix}{name_file}{collision_avoidance1}{suffix}")
	);
	let generated = user_name_to_file_name(name, &exist, prefix, suffix).expect("name");
	assert_eq!(
		generated,
		format!("{prefix}{name_file}{collision_avoidance2}{suffix}")
	);
}

#[test]
fn test_ValueError() {
	// The other cases pass objects that are not strings, or bytes that are not
	// valid UTF-8, which `&str` rules out. An empty name is the one error left.
	assert!(matches!(
		user_name_to_file_name("", &NONE, "", ""),
		Err(FilenamesError::Index(_))
	));
}

#[test]
fn test_handleClash1() {
	let prefix = format!("{}.", "0".repeat(5));
	let suffix = format!(".{}", "0".repeat(10));
	let existing = vec!["a".repeat(5)];

	let e = existing.clone();
	assert_eq!(
		handle_clash1(&"A".repeat(5), &e, &prefix, &suffix),
		Ok("00000.AAAAA000000000000001.0000000000".to_owned())
	);

	let mut e = existing.clone();
	e.push(format!("{prefix}aaaaa{:0>15}{suffix}", "1"));
	assert_eq!(
		handle_clash1(&"A".repeat(5), &e, &prefix, &suffix),
		Ok("00000.AAAAA000000000000002.0000000000".to_owned())
	);

	let mut e = existing;
	e.push(format!("{prefix}AAAAA{:0>15}{suffix}", "2"));
	assert_eq!(
		handle_clash1(&"A".repeat(5), &e, &prefix, &suffix),
		Ok("00000.AAAAA000000000000001.0000000000".to_owned())
	);
}

#[test]
fn test_handleClash2() {
	let prefix = format!("{}.", "0".repeat(5));
	let suffix = format!(".{}", "0".repeat(10));
	let existing: Vec<String> = (0..100).map(|i| format!("{prefix}{i}{suffix}")).collect();

	let e = existing.clone();
	assert_eq!(
		handle_clash2(&e, &prefix, &suffix),
		Ok("00000.100.0000000000".to_owned())
	);

	let mut e = existing.clone();
	e.retain(|n| *n != format!("{prefix}1{suffix}"));
	assert_eq!(
		handle_clash2(&e, &prefix, &suffix),
		Ok("00000.1.0000000000".to_owned())
	);

	let mut e = existing;
	e.retain(|n| *n != format!("{prefix}2{suffix}"));
	assert_eq!(
		handle_clash2(&e, &prefix, &suffix),
		Ok("00000.2.0000000000".to_owned())
	);
}

/// Checked against fontTools: the double quote and NUL are not illegal in
/// `fontTools.misc.filenames` (unlike in `fontTools.ufoLib.filenames`), nor
/// are parentheses; `com5` and `lpt4` are not reserved.
#[test]
fn differences_from_ufolib() {
	assert_eq!(name("a\"b\0c(d)*"), "a\"b\0c(d)_");
	assert_eq!(name("com5.lpt4.a:-z:.nul"), "com5.lpt4.a_-z_._nul");
	// No unique name: NameTranslationError.
	let prefix = "p".repeat(200);
	let suffix = "s".repeat(54);
	let existing: Vec<String> = (0..10).map(|i| format!("{prefix}{i}{suffix}")).collect();
	assert_eq!(
		handle_clash2(&existing, &prefix, &suffix),
		Err(FilenamesError::NameTranslation(
			"No unique name could be found.".into()
		))
	);
}
