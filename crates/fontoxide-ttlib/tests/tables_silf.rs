//! `Tests/ttLib/tables/S__i_l_f_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use fontoxide_ttlib::table::{Table, TableType};
use fontoxide_ttlib::tables::Silf;
use fontoxide_ttlib::{Font, ReadOptions, Result};

fn graphite_font() -> Font {
	let data = std::fs::read(tables_data("graphite").join("graphite_tests.ttf")).unwrap();
	Font::read(data, ReadOptions::default()).unwrap()
}

#[test]
fn test_compile_decompiled_table() -> Result<()> {
	let mut font = graphite_font();
	let mut table = font.require::<Silf>()?.clone();
	let expected = get_xml(&table, &mut font)?;

	let data = table.compile(&mut font)?;
	let table2 = Silf::decompile(&data, &mut font)?;

	assert_eq!(get_xml(&table, &mut font)?, expected);
	assert_eq!(get_xml(&table2, &mut font)?, expected);
	Ok(())
}

#[test]
fn test_compile_decompiled_empty_action() -> Result<()> {
	let mut font = graphite_font();
	let mut table = font.require::<Silf>()?.clone();
	table.silfs[0].passes[0].actions[1] = Vec::new();
	let data = table.compile(&mut font)?;

	let mut table2 = Silf::decompile(&data, &mut font)?;

	assert_eq!(table2.compile(&mut font)?, data);
	Ok(())
}
