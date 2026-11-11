//! `Tests/ttLib/tables/M_V_A_R_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use fontoxide_ttlib::tables::Mvar;
use fontoxide_ttlib::{Font, Table, TableType};

const MVAR_DATA: &str = concat!(
	"000100000000000800090054686173630000000368636c610000000368636c64000000036864736300000000686c6770",
	"000000027362796f000000017370796f000000027465737400000002746573320000000200010000000c000100000016",
	"000100010000400040000004000100010000ff38ffce006400c8",
);

const MVAR_XML: &[&str] = &[
	"<Version value=\"0x00010000\"/>",
	"<Reserved value=\"0\"/>",
	"<ValueRecordSize value=\"8\"/>",
	"<!-- ValueRecordCount=9 -->",
	"<VarStore Format=\"1\">",
	"  <Format value=\"1\"/>",
	"  <VarRegionList>",
	"    <!-- RegionAxisCount=1 -->",
	"    <!-- RegionCount=1 -->",
	"    <Region index=\"0\">",
	"      <VarRegionAxis index=\"0\">",
	"        <StartCoord value=\"0.0\"/>",
	"        <PeakCoord value=\"1.0\"/>",
	"        <EndCoord value=\"1.0\"/>",
	"      </VarRegionAxis>",
	"    </Region>",
	"  </VarRegionList>",
	"  <!-- VarDataCount=1 -->",
	"  <VarData index=\"0\">",
	"    <!-- ItemCount=4 -->",
	"    <NumShorts value=\"1\"/>",
	"    <!-- VarRegionCount=1 -->",
	"    <VarRegionIndex index=\"0\" value=\"0\"/>",
	"    <Item index=\"0\" value=\"[-200]\"/>",
	"    <Item index=\"1\" value=\"[-50]\"/>",
	"    <Item index=\"2\" value=\"[100]\"/>",
	"    <Item index=\"3\" value=\"[200]\"/>",
	"  </VarData>",
	"</VarStore>",
	"<ValueRecord index=\"0\">",
	"  <ValueTag value=\"hasc\"/>",
	"  <VarIdx value=\"3\"/>",
	"</ValueRecord>",
	"<ValueRecord index=\"1\">",
	"  <ValueTag value=\"hcla\"/>",
	"  <VarIdx value=\"3\"/>",
	"</ValueRecord>",
	"<ValueRecord index=\"2\">",
	"  <ValueTag value=\"hcld\"/>",
	"  <VarIdx value=\"3\"/>",
	"</ValueRecord>",
	"<ValueRecord index=\"3\">",
	"  <ValueTag value=\"hdsc\"/>",
	"  <VarIdx value=\"0\"/>",
	"</ValueRecord>",
	"<ValueRecord index=\"4\">",
	"  <ValueTag value=\"hlgp\"/>",
	"  <VarIdx value=\"2\"/>",
	"</ValueRecord>",
	"<ValueRecord index=\"5\">",
	"  <ValueTag value=\"sbyo\"/>",
	"  <VarIdx value=\"1\"/>",
	"</ValueRecord>",
	"<ValueRecord index=\"6\">",
	"  <ValueTag value=\"spyo\"/>",
	"  <VarIdx value=\"2\"/>",
	"</ValueRecord>",
	"<ValueRecord index=\"7\">",
	"  <ValueTag value=\"test\"/>",
	"  <VarIdx value=\"2\"/>",
	"</ValueRecord>",
	"<ValueRecord index=\"8\">",
	"  <ValueTag value=\"tes2\"/>",
	"  <VarIdx value=\"2\"/>",
	"</ValueRecord>",
];

#[test]
fn test_decompile_to_xml() -> TestResult {
	let mut font = Font::new();
	let mvar = Mvar::decompile(&hex(MVAR_DATA), &mut font)?;
	assert_eq!(get_xml(&mvar, &mut font)?, MVAR_XML);
	Ok(())
}

#[test]
fn test_compile_from_xml() -> TestResult {
	let mut font = Font::new();
	let mut mvar = Mvar::default();
	from_xml(&mut mvar, &MVAR_XML.concat(), &mut font)?;
	assert_eq!(hex_str(&mvar.compile(&mut font)?), MVAR_DATA);
	Ok(())
}
