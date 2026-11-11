//! `Tests/varLib/hvar_test.py`.

mod common;

use common::{TestResult, tests_path};
use fontoxide_ttlib::{Font, TableKey, Tag, TtxOptions};
use fontoxide_varlib::hvar::add_hvar;

fn hvar_ttx(font: &mut Font) -> TestResult<String> {
	Ok(font.to_ttx(&TtxOptions {
		tables: vec![TableKey::Table(Tag::new(b"HVAR"))],
		..Default::default()
	})?)
}

fn mutator_sans() -> TestResult<Font> {
	let mut font = Font::new();
	font.import_ttx(&std::fs::read_to_string(tests_path(
		"varLib/data/MutatorSans_All_Variable.ttx",
	))?)?;
	Ok(font)
}

#[test]
fn test_roundtrip() -> TestResult {
	let mut font = mutator_sans()?;
	let hvar1 = hvar_ttx(&mut font)?;
	font.remove(Tag::new(b"HVAR"));
	add_hvar(&mut font)?;
	let hvar2 = hvar_ttx(&mut font)?;
	assert_eq!(hvar1, hvar2);
	Ok(())
}

#[test]
fn test_roundtrip_beyond64k_gvar() -> TestResult {
	let mut font = mutator_sans()?;
	fontoxide_ttlib::beyond64k::upper_tables(&mut font, &Default::default())?;
	font.remove(Tag::new(b"HVAR"));
	add_hvar(&mut font)?;
	assert!(font.contains(Tag::new(b"GVAR")));
	assert!(!font.contains(Tag::new(b"gvar")));
	assert!(hvar_ttx(&mut font)?.contains("<HVAR>"));
	Ok(())
}
