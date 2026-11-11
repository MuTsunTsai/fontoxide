//! Port of fontTools' `Tests/ufoLib/filenames_test.py`, with the doctests of
//! `fontTools.ufoLib.filenames`.
//!
//! `test_userNameToFileName_ValueError` is not ported: the user name is a
//! `&str`.

use std::collections::HashSet;

use fontoxide_ufolib::Result;
use fontoxide_ufolib::filenames::{handle_clash1, handle_clash2, user_name_to_file_name};

fn name(user_name: &str) -> Result<String> {
	let none: [&str; 0] = [];
	user_name_to_file_name(user_name, &none, "", "")
}

#[test]
fn test_user_name_to_file_name() -> Result<()> {
	for (a, b) in [
		("a", "a"),
		("A", "A_"),
		("AE", "A_E_"),
		("Ae", "A_e"),
		("ae", "ae"),
		("aE", "aE_"),
		("a.alt", "a.alt"),
		("A.alt", "A_.alt"),
		("A.Alt", "A_.A_lt"),
		("A.aLt", "A_.aL_t"),
		("A.alT", "A_.alT_"),
		("T_H", "T__H_"),
		("T_h", "T__h"),
		("t_h", "t_h"),
		("F_F_I", "F__F__I_"),
		("f_f_i", "f_f_i"),
		("Aacute_V.swash", "A_acute_V_.swash"),
		(".notdef", "_notdef"),
		("con", "_con"),
		("CON", "C_O_N_"),
		("con.alt", "_con.alt"),
		("alt.con", "alt._con"),
	] {
		assert_eq!(name(a)?, b, "{a}");
	}
	// Test output for ASCII range (up until 0x7F), except for illegal
	// characters.
	let ascii: String = (0u8..0x80).map(char::from).collect();
	assert_eq!(
		name(&ascii)?,
		concat!(
			"________________________________",
			" !_#$%&'____,-._0123456789_;_=__",
			"@A_B_C_D_E_F_G_H_I_J_K_L_M_N_O_P_Q_R_S_T_U_V_W_X_Y_Z_",
			"___^_`abcdefghijklmnopqrstuvwxyz{_}~_"
		)
	);
	Ok(())
}

#[test]
fn test_empty_user_name() {
	// Not in fontTools: `userNameToFileName("")` fails there too
	// (`userName[0]`).
	assert!(name("").is_err());
}

#[test]
fn test_handle_clash1() -> Result<()> {
	let prefix = format!("{}.", "0".repeat(5));
	let suffix = format!(".{}", "0".repeat(10));
	let existing = vec!["a".repeat(5)];

	let e = existing.clone();
	assert_eq!(
		handle_clash1(&"A".repeat(5), &e, &prefix, &suffix)?,
		"00000.AAAAA000000000000001.0000000000"
	);

	let mut e = existing.clone();
	e.push(format!("{prefix}aaaaa{:0>15}{suffix}", "1"));
	assert_eq!(
		handle_clash1(&"A".repeat(5), &e, &prefix, &suffix)?,
		"00000.AAAAA000000000000002.0000000000"
	);

	let mut e = existing;
	e.push(format!("{prefix}AAAAA{:0>15}{suffix}", "2"));
	assert_eq!(
		handle_clash1(&"A".repeat(5), &e, &prefix, &suffix)?,
		"00000.AAAAA000000000000001.0000000000"
	);
	Ok(())
}

#[test]
fn test_handle_clash2() -> Result<()> {
	let prefix = format!("{}.", "0".repeat(5));
	let suffix = format!(".{}", "0".repeat(10));
	let existing: Vec<String> = (0..100).map(|i| format!("{prefix}{i}{suffix}")).collect();

	let e: HashSet<String> = existing.iter().cloned().collect();
	assert_eq!(handle_clash2(&e, &prefix, &suffix)?, "00000.100.0000000000");

	let mut e = e.clone();
	e.remove(&format!("{prefix}1{suffix}"));
	assert_eq!(handle_clash2(&e, &prefix, &suffix)?, "00000.1.0000000000");

	let mut e: HashSet<String> = existing.iter().cloned().collect();
	e.remove(&format!("{prefix}2{suffix}"));
	assert_eq!(handle_clash2(&e, &prefix, &suffix)?, "00000.2.0000000000");
	Ok(())
}
