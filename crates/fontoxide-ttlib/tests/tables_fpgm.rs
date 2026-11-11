//! `Tests/ttLib/tables/_f_p_g_m_test.py`, and the doctest of
//! `Lib/fontTools/ttLib/tables/_f_p_g_m.py`.

mod common;

use common::TestResult;
use fontoxide_ttlib::tables::Fpgm;
use fontoxide_ttlib::tables::tt_program::Program;
use fontoxide_ttlib::{Font, Table, TableType};

/// `bool(fpgm)` is `not fpgm.is_empty()`.
#[test]
fn test_bool() {
	let mut fpgm = Fpgm::default();
	assert!(fpgm.is_empty());

	fpgm.program = Some(Program::default());
	assert!(fpgm.is_empty());

	fpgm.program = Some(Program::from_bytecode(&[0]));
	assert!(!fpgm.is_empty());

	if let Some(p) = &mut fpgm.program {
		assert_eq!(p.bytecode.pop(), Some(0));
	}
	assert!(fpgm.is_empty());
}

/// The `table__f_p_g_m.__bool__` doctest, which is the same as `test__bool__`.
#[test]
fn fpgm_bool_doctest() {
	test_bool();
}

#[test]
fn test_compile_without_program() -> TestResult {
	// When read from XML, in an empty fpgm table, the program is missing.
	let mut fpgm = Fpgm::default();
	assert_eq!(fpgm.compile(&mut Font::new())?, b"");
	Ok(())
}

#[test]
fn test_decompile_without_program() -> TestResult {
	// VTT adds a zero-length table to a font if the program is empty.
	let fpgm = Fpgm::decompile(b"", &mut Font::new())?;
	assert!(fpgm.is_empty());
	Ok(())
}
