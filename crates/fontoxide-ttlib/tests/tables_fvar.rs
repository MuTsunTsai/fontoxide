//! `Tests/ttLib/tables/_f_v_a_r_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;

use common::*;
use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_ttlib::tables::Fvar;
use fontoxide_ttlib::tables::fvar::{Axis, NamedInstance};
use fontoxide_ttlib::tables::name::{Name, NameRecord};
use fontoxide_ttlib::{Font, Table, TableType, Tag};

const FVAR_DATA: &str = "00 01 00 00 00 10 00 02 00 02 00 14 00 02 00 0C \
	77 67 68 74 00 64 00 00 01 90 00 00 03 84 00 00 00 00 01 01 \
	77 64 74 68 00 32 00 00 00 64 00 00 00 c8 00 00 00 00 01 02 \
	01 03 00 00 01 2c 00 00 00 64 00 00 \
	01 04 00 00 01 2c 00 00 00 4b 00 00";

const FVAR_AXIS_DATA: &str = "6F 70 73 7a ff ff 80 00 00 01 4c cd 00 01 80 00 00 00 01 59";

const FVAR_INSTANCE_DATA_WITHOUT_PSNAME: &str = "01 59 00 00 00 00 b3 33 00 00 80 00";

fn instance_data_with_psname() -> Vec<u8> {
	let mut data = hex(FVAR_INSTANCE_DATA_WITHOUT_PSNAME);
	data.extend(hex("02 34"));
	data
}

fn tag(s: &str) -> Tag {
	Tag::from_str_exact(s).unwrap()
}

/// The lines of the writer's output, stripped (`xml_lines`).
fn xml_lines(writer: XmlWriter) -> Vec<String> {
	writer_lines(writer)
		.iter()
		.map(|l| l.trim().to_owned())
		.collect()
}

/// `AddName(font, name)`: a Macintosh name with the next name ID above 256.
fn add_name(font: &mut Font, name: &str) -> u16 {
	if !font.contains(Name::TAG) {
		font.insert(Box::new(Name::default()));
	}
	let table = font.require::<Name>().unwrap();
	let name_id = 1 + table
		.names
		.iter()
		.map(|n| n.name_id)
		.chain([256])
		.max()
		.unwrap();
	let string = fontoxide_encodings::codecs::encode("mac_roman", name).unwrap();
	table.names.push(NameRecord::new(string, name_id, 1, 0, 0));
	name_id
}

/// `MakeFont()`.
fn make_font() -> Font {
	let mut font = Font::new();
	let mut fvar = Fvar::default();
	for (t, name, min_value, default_value, max_value) in [
		("wght", "Weight", 100.0, 400.0, 900.0),
		("wdth", "Width", 50.0, 100.0, 200.0),
	] {
		let mut axis = Axis::new(tag(t));
		axis.default_value = default_value;
		axis.min_value = min_value;
		axis.max_value = max_value;
		axis.axis_name_id = add_name(&mut font, name);
		fvar.axes.push(axis);
	}
	for (name, weight, width) in [("Light", 300.0, 100.0), ("Light Condensed", 300.0, 75.0)] {
		fvar.instances.push(NamedInstance {
			subfamily_name_id: add_name(&mut font, name),
			coordinates: BTreeMap::from([(tag("wght"), weight), (tag("wdth"), width)]),
			..Default::default()
		});
	}
	font.insert(Box::new(fvar));
	font
}

fn coordinates(wght: f64, wdth: f64) -> BTreeMap<Tag, f64> {
	BTreeMap::from([(tag("wght"), wght), (tag("wdth"), wdth)])
}

fn assert_dict_almost_equal(dict1: &BTreeMap<Tag, f64>, dict2: &BTreeMap<Tag, f64>) {
	assert_eq!(
		dict1.keys().collect::<Vec<_>>(),
		dict2.keys().collect::<Vec<_>>()
	);
	for (key, v) in dict1 {
		assert!((v - dict2[key]).abs() < 5e-8, "{v} != {}", dict2[key]);
	}
}

#[test]
fn test_compile() -> TestResult {
	let mut font = make_font();
	let mut fvar = font.require::<Fvar>()?.clone();
	assert_eq!(fvar.compile(&mut font)?, hex(FVAR_DATA));
	Ok(())
}

#[test]
fn test_decompile() -> TestResult {
	let fvar = Fvar::decompile(&hex(FVAR_DATA), &mut Font::new())?;
	assert_eq!(
		fvar.axes.iter().map(Axis::tag_text).collect::<Vec<_>>(),
		["wght", "wdth"]
	);
	assert_eq!(
		fvar.instances
			.iter()
			.map(|i| i.subfamily_name_id)
			.collect::<Vec<_>>(),
		[259, 260]
	);
	Ok(())
}

#[test]
fn test_to_xml() -> TestResult {
	let mut font = make_font();
	let fvar = font.require::<Fvar>()?.clone();
	let xml = get_xml(&fvar, &mut font)?.join("\n");
	assert_eq!(xml.matches("<Axis>").count(), 2);
	assert!(xml.contains("<AxisTag>wght</AxisTag>"));
	assert!(xml.contains("<AxisTag>wdth</AxisTag>"));
	assert_eq!(xml.matches("<NamedInstance ").count(), 2);
	assert!(xml.contains("<!-- Light -->"));
	assert!(xml.contains("<!-- Light Condensed -->"));
	Ok(())
}

#[test]
fn test_from_xml() -> TestResult {
	let mut fvar = Fvar::default();
	from_xml(
		&mut fvar,
		concat!(
			"<Axis>",
			"    <AxisTag>opsz</AxisTag>",
			"</Axis>",
			"<Axis>",
			"    <AxisTag>slnt</AxisTag>",
			"    <Flags>0x123</Flags>",
			"</Axis>",
			r#"<NamedInstance subfamilyNameID="765"/>"#,
			r#"<NamedInstance subfamilyNameID="234"/>"#,
		),
		&mut Font::new(),
	)?;
	assert_eq!(
		fvar.axes.iter().map(Axis::tag_text).collect::<Vec<_>>(),
		["opsz", "slnt"]
	);
	assert_eq!(
		fvar.axes.iter().map(|a| a.flags).collect::<Vec<_>>(),
		[0, 0x123]
	);
	assert_eq!(
		fvar.instances
			.iter()
			.map(|i| i.subfamily_name_id)
			.collect::<Vec<_>>(),
		[765, 234]
	);
	Ok(())
}

#[test]
fn test_axis_compile() -> TestResult {
	let mut axis = Axis::new(tag("opsz"));
	axis.axis_name_id = 345;
	axis.min_value = -0.5;
	axis.default_value = 1.3;
	axis.max_value = 1.5;
	let mut data = Vec::new();
	axis.pack(&mut data)?;
	assert_eq!(data, hex(FVAR_AXIS_DATA));
	Ok(())
}

#[test]
fn test_axis_decompile() -> TestResult {
	let (axis, _) = Axis::unpack2(&hex(FVAR_AXIS_DATA))?;
	assert_eq!(axis.tag_text(), "opsz");
	assert_eq!(axis.axis_name_id, 345);
	assert_eq!(axis.min_value, -0.5);
	assert!((axis.default_value - 1.3000031).abs() < 5e-8);
	assert_eq!(axis.max_value, 1.5);
	Ok(())
}

#[test]
fn test_axis_to_xml() -> TestResult {
	let mut font = make_font();
	let (mut axis, _) = Axis::unpack2(&hex(FVAR_AXIS_DATA))?;
	// `AddName(font, "Optical Size").nameID = 256`.
	let name_id = add_name(&mut font, "Optical Size");
	let name = font.require::<Name>()?;
	name.names
		.iter_mut()
		.find(|n| n.name_id == name_id)
		.unwrap()
		.name_id = 256;
	axis.axis_name_id = 256;
	axis.flags = 0xABC;
	let mut writer = XmlWriter::new();
	axis.to_xml(&mut writer, Some(name));
	assert_eq!(
		xml_lines(writer),
		[
			"",
			"<!-- Optical Size -->",
			"<Axis>",
			"<AxisTag>opsz</AxisTag>",
			"<Flags>0xABC</Flags>",
			"<MinValue>-0.5</MinValue>",
			"<DefaultValue>1.3</DefaultValue>",
			"<MaxValue>1.5</MaxValue>",
			"<AxisNameID>256</AxisNameID>",
			"</Axis>",
		]
	);
	Ok(())
}

#[test]
fn test_axis_from_xml() -> TestResult {
	let xml = |flags: &str| {
		format!(
			"<Axis><AxisTag>wght</AxisTag><Flags>{flags}</Flags><MinValue>100</MinValue>\
			<DefaultValue>400</DefaultValue><MaxValue>900</MaxValue>\
			<AxisNameID>256</AxisNameID></Axis>"
		)
	};
	// LENIENT(L38), as L9/L24: the flags are checked to fit 16 bits when reading TTX;
	// fontTools fails only when compiling.
	assert!(Axis::from_xml(&parse_xml(&xml("0x123ABC"))[0]).is_err());
	let axis = Axis::from_xml(&parse_xml(&xml("0x3ABC"))[0])?;
	assert_eq!(axis.tag_text(), "wght");
	assert_eq!(axis.flags, 0x3ABC);
	assert_eq!(axis.min_value, 100.0);
	assert_eq!(axis.default_value, 400.0);
	assert_eq!(axis.max_value, 900.0);
	assert_eq!(axis.axis_name_id, 256);
	Ok(())
}

fn instance() -> NamedInstance {
	NamedInstance {
		subfamily_name_id: 345,
		postscript_name_id: 564,
		coordinates: coordinates(0.7, 0.5),
		..Default::default()
	}
}

const WGHT_WDTH: [Tag; 2] = [Tag::new(b"wght"), Tag::new(b"wdth")];

#[test]
fn test_compile_with_post_script_name() -> TestResult {
	let mut data = Vec::new();
	instance().compile(&WGHT_WDTH, true, &mut data)?;
	assert_eq!(data, instance_data_with_psname());
	Ok(())
}

#[test]
fn test_compile_without_post_script_name() -> TestResult {
	let mut data = Vec::new();
	instance().compile(&WGHT_WDTH, false, &mut data)?;
	assert_eq!(data, hex(FVAR_INSTANCE_DATA_WITHOUT_PSNAME));
	Ok(())
}

#[test]
fn test_decompile_with_post_script_name() -> TestResult {
	let inst = NamedInstance::decompile(&instance_data_with_psname(), &WGHT_WDTH)?;
	assert_eq!(inst.postscript_name_id, 564);
	assert_eq!(inst.subfamily_name_id, 345);
	assert_dict_almost_equal(&coordinates(0.6999969, 0.5), &inst.coordinates);
	Ok(())
}

#[test]
fn test_decompile_without_post_script_name() -> TestResult {
	let inst = NamedInstance::decompile(&hex(FVAR_INSTANCE_DATA_WITHOUT_PSNAME), &WGHT_WDTH)?;
	assert_eq!(inst.postscript_name_id, 0xFFFF);
	assert_eq!(inst.subfamily_name_id, 345);
	assert_dict_almost_equal(&coordinates(0.6999969, 0.5), &inst.coordinates);
	Ok(())
}

#[test]
fn test_to_xml_with_post_script_name() -> TestResult {
	let mut font = make_font();
	let inst = NamedInstance {
		flags: 0xE9,
		subfamily_name_id: add_name(&mut font, "Light Condensed"),
		postscript_name_id: add_name(&mut font, "Test-LightCondensed"),
		coordinates: coordinates(0.7, 0.5),
	};
	let axes = font.require::<Fvar>()?.axes.clone();
	let mut writer = XmlWriter::new();
	inst.to_xml(&mut writer, Some(font.require::<Name>()?), &axes)?;
	assert_eq!(
		xml_lines(writer),
		[
			"".to_owned(),
			"<!-- Light Condensed -->".to_owned(),
			"<!-- PostScript: Test-LightCondensed -->".to_owned(),
			format!(
				r#"<NamedInstance flags="0xE9" postscriptNameID="{}" subfamilyNameID="{}">"#,
				inst.postscript_name_id, inst.subfamily_name_id
			),
			r#"<coord axis="wght" value="0.7"/>"#.to_owned(),
			r#"<coord axis="wdth" value="0.5"/>"#.to_owned(),
			"</NamedInstance>".to_owned(),
		]
	);
	Ok(())
}

#[test]
fn test_to_xml_without_post_script_name() -> TestResult {
	let mut font = make_font();
	let inst = NamedInstance {
		flags: 0xABC,
		subfamily_name_id: add_name(&mut font, "Light Condensed"),
		coordinates: coordinates(0.7, 0.5),
		..Default::default()
	};
	let axes = font.require::<Fvar>()?.axes.clone();
	let mut writer = XmlWriter::new();
	inst.to_xml(&mut writer, Some(font.require::<Name>()?), &axes)?;
	assert_eq!(
		xml_lines(writer),
		[
			"".to_owned(),
			"<!-- Light Condensed -->".to_owned(),
			format!(
				r#"<NamedInstance flags="0xABC" subfamilyNameID="{}">"#,
				inst.subfamily_name_id
			),
			r#"<coord axis="wght" value="0.7"/>"#.to_owned(),
			r#"<coord axis="wdth" value="0.5"/>"#.to_owned(),
			"</NamedInstance>".to_owned(),
		]
	);
	Ok(())
}

#[test]
fn test_from_xml_with_post_script_name() -> TestResult {
	let element = &parse_xml(concat!(
		r#"<NamedInstance flags="0x0" postscriptNameID="257" subfamilyNameID="345">"#,
		r#"    <coord axis="wght" value="0.7"/>"#,
		r#"    <coord axis="wdth" value="0.5"/>"#,
		"</NamedInstance>"
	))[0];
	let inst = NamedInstance::from_xml(element)?;
	assert_eq!(inst.postscript_name_id, 257);
	assert_eq!(inst.subfamily_name_id, 345);
	assert_dict_almost_equal(&coordinates(0.6999969, 0.5), &inst.coordinates);
	Ok(())
}

#[test]
fn test_from_xml_without_post_script_name() -> TestResult {
	let xml = |flags: &str| {
		format!(
			r#"<NamedInstance flags="{flags}" subfamilyNameID="345"><coord axis="wght" value="0.7"/><coord axis="wdth" value="0.5"/></NamedInstance>"#
		)
	};
	// LENIENT(L38), as L9/L24: the flags are checked to fit 16 bits when reading TTX;
	// fontTools fails only when compiling.
	assert!(NamedInstance::from_xml(&parse_xml(&xml("0x123ABC"))[0]).is_err());
	let inst = NamedInstance::from_xml(&parse_xml(&xml("0x3ABC"))[0])?;
	assert_eq!(inst.flags, 0x3ABC);
	assert_eq!(inst.subfamily_name_id, 345);
	assert_dict_almost_equal(&coordinates(0.6999969, 0.5), &inst.coordinates);
	Ok(())
}
