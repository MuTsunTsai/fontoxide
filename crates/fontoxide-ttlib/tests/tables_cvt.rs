//! `Tests/ttLib/tables/_c_v_t_test.py`.

mod common;

use common::TestResult;
use fontoxide_ttlib::tables::Cvt;
use fontoxide_ttlib::{Font, Table, TableType};

#[test]
fn test_compile_without_values() -> TestResult {
	// When read from XML, in an empty cvt table, there are no values.
	let mut cvt = Cvt::default();
	assert_eq!(cvt.compile(&mut Font::new())?, b"");
	Ok(())
}

#[test]
fn test_decompile_without_values() -> TestResult {
	// VTT adds a zero-length table to a font if the Control Values are empty.
	let cvt = Cvt::decompile(b"", &mut Font::new())?;
	assert!(cvt.values.is_empty());
	Ok(())
}
