//! `Tests/ttLib/tables/V_O_R_G__test.py`.

mod common;

use common::{TestResult, hex};
use fontoxide_ttlib::tables::Vorg;
use fontoxide_ttlib::{Font, Table, TableType};

#[test]
fn test_version_2_round_trip() -> TestResult {
	// Glyph 10 and glyph 0x10000 ("highGlyph"); records are keyed by glyph ID.
	let mut font = Font::new();
	let mut vorg = Vorg {
		major_version: 2,
		minor_version: 0,
		default_vert_origin_y: 880,
		..Default::default()
	};
	vorg.v_origin_records.insert(0x10000, 861);
	vorg.v_origin_records.insert(10, 889);

	let data = vorg.compile(&mut font)?;
	assert_eq!(data, hex("0002 0000 0370 000002 00000A 0379 010000 035D"));

	let recompiled = Vorg::decompile(&data, &mut font)?;
	assert_eq!(recompiled.major_version, 2);
	assert_eq!(recompiled.minor_version, 0);
	assert_eq!(recompiled.default_vert_origin_y, 880);
	assert_eq!(
		recompiled.v_origin_records,
		[(10, 889), (0x10000, 861)].into_iter().collect()
	);
	Ok(())
}
