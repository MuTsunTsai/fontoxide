//! `Tests/ttLib/tables/C_P_A_L_test.py`.

mod common;

use common::{TestResult, from_xml, get_xml, hex};
use fontoxide_ttlib::tables::cpal::Color;
use fontoxide_ttlib::tables::{Cpal, Name};
use fontoxide_ttlib::{Font, Table, TableType};

const CPAL_DATA_V0: &str =
	"0000 0002 0002 0004 00000010 0000 0002 000000FF FFCC66FF 000000FF 000080FF";

const CPAL_DATA_V0_SHARING_COLORS: &str = "0000 0003 0004 0006 00000014 0000 0000 0003 0000 \
	443322FF 77889911 55555555 443322FF 77889911 FFFFFFFF";

const CPAL_DATA_V1_NOLABELS_NOTYPES: &str = "0001 0003 0002 0006 0000001C 0000 0003 \
	00000000 00000000 00000000 \
	CAFECAFE 00112233 44556677 31415927 42424242 00331337";

const CPAL_DATA_V1: &str = "0001 0003 0002 0006 0000001C 0000 0003 \
	00000034 0000003C 00000040 \
	CAFECAFE 00112233 44556677 31415927 42424242 00331337 \
	00000001 00000002 0102 0103 0201 0202 0203";

/// `repr(palette)`.
fn repr(palette: &[Color]) -> String {
	let colors: Vec<String> = palette.iter().map(Color::hex).collect();
	format!("[{}]", colors.join(", "))
}

fn reprs(cpal: &Cpal) -> Vec<String> {
	cpal.palettes.iter().map(|p| repr(p)).collect()
}

fn decompile(data: &str) -> fontoxide_ttlib::Result<Cpal> {
	Cpal::decompile(&hex(data), &mut Font::new())
}

#[test]
fn test_decompile_v0() -> TestResult {
	let cpal = decompile(CPAL_DATA_V0)?;
	assert_eq!(cpal.version, 0);
	assert_eq!(cpal.num_palette_entries, 2);
	assert_eq!(
		reprs(&cpal),
		["[#000000FF, #66CCFFFF]", "[#000000FF, #800000FF]"]
	);
	Ok(())
}

#[test]
fn test_decompile_v0_sharing_colors() -> TestResult {
	let cpal = decompile(CPAL_DATA_V0_SHARING_COLORS)?;
	assert_eq!(cpal.version, 0);
	assert_eq!(cpal.num_palette_entries, 3);
	assert_eq!(
		reprs(&cpal),
		[
			"[#223344FF, #99887711, #55555555]",
			"[#223344FF, #99887711, #55555555]",
			"[#223344FF, #99887711, #FFFFFFFF]",
			"[#223344FF, #99887711, #55555555]",
		]
	);
	Ok(())
}

#[test]
fn test_decompile_v1_no_labels_no_types() -> TestResult {
	let cpal = decompile(CPAL_DATA_V1_NOLABELS_NOTYPES)?;
	assert_eq!(cpal.version, 1);
	assert_eq!(cpal.num_palette_entries, 3);
	assert_eq!(
		reprs(&cpal),
		[
			"[#CAFECAFE, #22110033, #66554477]",
			"[#59413127, #42424242, #13330037]",
		]
	);
	assert_eq!(cpal.palette_labels, [Cpal::NO_NAME_ID; 2]);
	assert_eq!(cpal.palette_types, [0, 0]);
	assert_eq!(cpal.palette_entry_labels, [Cpal::NO_NAME_ID; 3]);
	Ok(())
}

#[test]
fn test_decompile_v1() -> TestResult {
	let cpal = decompile(CPAL_DATA_V1)?;
	assert_eq!(cpal.version, 1);
	assert_eq!(cpal.num_palette_entries, 3);
	assert_eq!(
		reprs(&cpal),
		[
			"[#CAFECAFE, #22110033, #66554477]",
			"[#59413127, #42424242, #13330037]",
		]
	);
	assert_eq!(cpal.palette_types, [1, 2]);
	assert_eq!(cpal.palette_labels, [258, 259]);
	assert_eq!(cpal.palette_entry_labels, [513, 514, 515]);
	Ok(())
}

#[test]
fn test_compile_v0() -> TestResult {
	let mut cpal = decompile(CPAL_DATA_V0)?;
	assert_eq!(cpal.compile(&mut Font::new())?, hex(CPAL_DATA_V0));
	Ok(())
}

#[test]
fn test_compile_v0_sharing_colors() -> TestResult {
	let color = |red, green, blue, alpha| Color {
		red,
		green,
		blue,
		alpha,
	};
	let palette1 = vec![
		color(0x22, 0x33, 0x44, 0xFF),
		color(0x99, 0x88, 0x77, 0x11),
		color(0x55, 0x55, 0x55, 0x55),
	];
	let palette2 = vec![
		color(0x22, 0x33, 0x44, 0xFF),
		color(0x99, 0x88, 0x77, 0x11),
		color(0xFF, 0xFF, 0xFF, 0xFF),
	];
	let mut cpal = Cpal::new_as(Cpal::TAG);
	cpal.version = 0;
	cpal.num_palette_entries = 3;
	cpal.palettes = vec![palette1.clone(), palette1.clone(), palette2, palette1];
	assert_eq!(
		cpal.compile(&mut Font::new())?,
		hex(CPAL_DATA_V0_SHARING_COLORS)
	);
	Ok(())
}

#[test]
fn test_compile_v1() -> TestResult {
	let mut cpal = decompile(CPAL_DATA_V1)?;
	assert_eq!(cpal.compile(&mut Font::new())?, hex(CPAL_DATA_V1));
	Ok(())
}

#[test]
fn test_compile_v1_no_labels_no_types() -> TestResult {
	let mut cpal = decompile(CPAL_DATA_V1_NOLABELS_NOTYPES)?;
	assert_eq!(
		cpal.compile(&mut Font::new())?,
		hex(CPAL_DATA_V1_NOLABELS_NOTYPES)
	);
	Ok(())
}

#[test]
fn test_to_xml_v0() -> TestResult {
	let cpal = decompile(CPAL_DATA_V0)?;
	assert_eq!(
		get_xml(&cpal, &mut Font::new())?,
		[
			"<version value=\"0\"/>",
			"<numPaletteEntries value=\"2\"/>",
			"<palette index=\"0\">",
			"  <color index=\"0\" value=\"#000000FF\"/>",
			"  <color index=\"1\" value=\"#66CCFFFF\"/>",
			"</palette>",
			"<palette index=\"1\">",
			"  <color index=\"0\" value=\"#000000FF\"/>",
			"  <color index=\"1\" value=\"#800000FF\"/>",
			"</palette>",
		]
	);
	Ok(())
}

#[test]
fn test_to_xml_v1() -> TestResult {
	// `FakeNameTable` answers `getDebugName` from a dict; an English Windows
	// record is what `getDebugName` finds first.
	let mut name = Name::default();
	for (id, text) in [
		(258, "Spring theme"),
		(259, "Winter theme"),
		(513, "darks"),
		(515, "lights"),
	] {
		name.set_name(text, id, 3, 1, 0x409);
	}
	let mut font = Font::new();
	font.insert(Box::new(name));
	let cpal = Cpal::decompile(&hex(CPAL_DATA_V1), &mut font)?;
	assert_eq!(
		get_xml(&cpal, &mut font)?,
		[
			"<version value=\"1\"/>",
			"<numPaletteEntries value=\"3\"/>",
			"<palette index=\"0\" label=\"258\" type=\"1\">",
			"  <!-- Spring theme -->",
			"  <color index=\"0\" value=\"#CAFECAFE\"/>",
			"  <color index=\"1\" value=\"#22110033\"/>",
			"  <color index=\"2\" value=\"#66554477\"/>",
			"</palette>",
			"<palette index=\"1\" label=\"259\" type=\"2\">",
			"  <!-- Winter theme -->",
			"  <color index=\"0\" value=\"#59413127\"/>",
			"  <color index=\"1\" value=\"#42424242\"/>",
			"  <color index=\"2\" value=\"#13330037\"/>",
			"</palette>",
			"<paletteEntryLabels>",
			"  <label index=\"0\" value=\"513\"/><!-- darks -->",
			"  <label index=\"1\" value=\"514\"/>",
			"  <label index=\"2\" value=\"515\"/><!-- lights -->",
			"</paletteEntryLabels>",
		]
	);
	Ok(())
}

#[test]
fn test_from_xml_v0() -> TestResult {
	let mut cpal = Cpal::new_as(Cpal::TAG);
	from_xml(
		&mut cpal,
		"<version value=\"0\"/>\
		<numPaletteEntries value=\"2\"/>\
		<palette index=\"0\">\
		  <color index=\"0\" value=\"#12345678\"/>\
		  <color index=\"1\" value=\"#FEDCBA98\"/>\
		</palette>",
		&mut Font::new(),
	)?;
	assert_eq!(cpal.version, 0);
	assert_eq!(cpal.num_palette_entries, 2);
	assert_eq!(reprs(&cpal), ["[#12345678, #FEDCBA98]"]);
	Ok(())
}

#[test]
fn test_from_xml_v1() -> TestResult {
	let mut cpal = Cpal::new_as(Cpal::TAG);
	from_xml(
		&mut cpal,
		"<version value=\"1\"/>\
		<numPaletteEntries value=\"3\"/>\
		<palette index=\"0\" label=\"259\" type=\"2\">\
		  <color index=\"0\" value=\"#12345678\"/>\
		  <color index=\"1\" value=\"#FEDCBA98\"/>\
		  <color index=\"2\" value=\"#CAFECAFE\"/>\
		</palette>\
		<paletteEntryLabels>\
		  <label index=\"1\" value=\"262\"/>\
		</paletteEntryLabels>",
		&mut Font::new(),
	)?;
	assert_eq!(cpal.version, 1);
	assert_eq!(cpal.num_palette_entries, 3);
	assert_eq!(reprs(&cpal), ["[#12345678, #FEDCBA98, #CAFECAFE]"]);
	assert_eq!(cpal.palette_labels, [259]);
	assert_eq!(cpal.palette_types, [2]);
	assert_eq!(
		cpal.palette_entry_labels,
		[Cpal::NO_NAME_ID, 262, Cpal::NO_NAME_ID]
	);
	Ok(())
}
