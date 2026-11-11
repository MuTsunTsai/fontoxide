//! `Tests/ttLib/tables/V_D_M_X__test.py`.

mod common;

use common::{TestResult, from_xml, get_xml, hex};
use fontoxide_ttlib::tables::Vdmx;
use fontoxide_ttlib::tables::vdmx::{RatRange, VdmxGroup};
use fontoxide_ttlib::{Font, Table, TableType};

fn vdmx_data() -> Vec<u8> {
	hex(concat!(
		"0001 0001 0001 ", // version=1, numRecs=1, numRatios=1
		"01 00 00 00 ",    // ratRange: bCharSet=1, xRatio=0, yStartRatio=0, yEndRatio=0
		"000C ",           // offset to group 0
		"0002 08 09 ",     // group 0: recs=2, startsz=8, endsz=9
		"0008 0007 FFFE ", // yPelHeight=8, yMax=7, yMin=-2
		"0009 0008 FFFE",  // yPelHeight=9, yMax=8, yMin=-2
	))
}

fn vdmx_data_empty() -> Vec<u8> {
	hex("0001 0000 0000")
}

fn xml_round_trip(table: &Vdmx) -> fontoxide_ttlib::Result<Vdmx> {
	let mut font = Font::new();
	let mut result = Vdmx::default();
	from_xml(
		&mut result,
		&get_xml(table, &mut font)?.join("\n"),
		&mut font,
	)?;
	Ok(result)
}

fn group(items: &[(i64, (i64, i64))]) -> VdmxGroup {
	items.iter().copied().collect()
}

#[test]
fn test_decompile_compile() -> TestResult {
	let mut font = Font::new();
	let mut table = Vdmx::decompile(&vdmx_data(), &mut font)?;
	assert_eq!(
		table.rat_ranges,
		[RatRange {
			b_char_set: 1,
			x_ratio: 0,
			y_start_ratio: 0,
			y_end_ratio: 0,
			group_index: Some(0),
		}]
	);
	assert_eq!(table.groups, [group(&[(8, (7, -2)), (9, (8, -2))])]);
	assert_eq!(table.compile(&mut font)?, vdmx_data());
	Ok(())
}

#[test]
fn test_compile_updates_counts() -> TestResult {
	let mut font = Font::new();
	let mut table = Vdmx::decompile(&vdmx_data(), &mut font)?;
	table.rat_ranges.push(RatRange {
		b_char_set: 0,
		x_ratio: 1,
		y_start_ratio: 1,
		y_end_ratio: 1,
		group_index: Some(1),
	});
	table.groups.push(group(&[(10, (9, -3))]));

	let data = table.compile(&mut font)?;

	let table2 = Vdmx::decompile(&data, &mut font)?;
	assert_eq!(table2.num_ratios, 2);
	assert_eq!(table2.num_recs, 2);
	assert_eq!(table2.rat_ranges, table.rat_ranges);
	assert_eq!(table2.groups, table.groups);
	Ok(())
}

#[test]
fn test_compile_empty() -> TestResult {
	let mut font = Font::new();
	let mut table = Vdmx::decompile(&vdmx_data(), &mut font)?;
	table.rat_ranges.clear();
	table.groups.clear();
	assert_eq!(table.compile(&mut font)?, vdmx_data_empty());
	Ok(())
}

#[test]
fn test_xml_round_trip() -> TestResult {
	let mut font = Font::new();
	let table = Vdmx::decompile(&vdmx_data(), &mut font)?;
	assert_eq!(xml_round_trip(&table)?.compile(&mut font)?, vdmx_data());
	Ok(())
}

#[test]
fn test_xml_round_trip_empty() -> TestResult {
	let mut font = Font::new();
	let table = Vdmx::decompile(&vdmx_data_empty(), &mut font)?;
	assert_eq!(
		get_xml(&table, &mut font)?,
		[
			"<version value=\"1\"/>",
			"<ratRanges>",
			"</ratRanges>",
			"<groups>",
			"</groups>",
		]
	);
	assert_eq!(
		xml_round_trip(&table)?.compile(&mut font)?,
		vdmx_data_empty()
	);
	Ok(())
}

/// The TTX of a table with a group, as fontTools writes it.
#[test]
fn xml_matches_fonttools() -> TestResult {
	let mut font = Font::new();
	let table = Vdmx::decompile(&vdmx_data(), &mut font)?;
	assert_eq!(
		get_xml(&table, &mut font)?,
		[
			"<version value=\"1\"/>",
			"<ratRanges>",
			"  <ratRange bCharSet=\"1\" groupIndex=\"0\" xRatio=\"0\" yEndRatio=\"0\" yStartRatio=\"0\"/>",
			"</ratRanges>",
			"<groups>",
			"  <group index=\"0\">",
			"    <!-- recs=2, startsz=8, endsz=9 -->",
			"    <record yPelHeight=\"8\" yMax=\"7\" yMin=\"-2\"/>",
			"    <record yPelHeight=\"9\" yMax=\"8\" yMin=\"-2\"/>",
			"  </group>",
			"</groups>",
		]
	);
	Ok(())
}
