//! `Tests/varLib/stat_test.py`.

mod common;

use common::{TestResult, tests_path};
use fontoxide_designspacelib::types::{Range, Region, RegionValue};
use fontoxide_designspacelib::{DesignSpaceDocument, Number};
use fontoxide_otllib::builder::{
	StatAxis, StatAxisValue, StatAxisValueKind, StatLocation, StatName,
};
use fontoxide_ttlib::tables::Name;
use fontoxide_ttlib::{Font, Tag};
use fontoxide_varlib::stat::{build_vf_stat_table, get_stat_axes, get_stat_locations};

fn doc() -> TestResult<DesignSpaceDocument> {
	Ok(DesignSpaceDocument::from_file(tests_path(
		"designspaceLib/data/test_v5.designspace",
	))?)
}

fn region(italic: f64, width: RegionValue, weight: RegionValue) -> Region {
	let mut r = Region::new();
	r.insert("Italic".into(), RegionValue::Value(Number::Float(italic)));
	r.insert("Width".into(), width);
	r.insert("Weight".into(), weight);
	r
}

fn range(a: i64, b: i64) -> RegionValue {
	RegionValue::Range(Range::without_default(Number::Int(a), Number::Int(b)))
}

fn names(items: &[(&str, &str)]) -> StatName {
	StatName::Multilingual(
		items
			.iter()
			.map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
			.collect(),
	)
}

fn en(name: &str) -> StatName {
	names(&[("en", name)])
}

fn ranged(
	name: StatName,
	flags: u16,
	nominal: f64,
	min: Option<f64>,
	max: Option<f64>,
) -> StatAxisValue {
	StatAxisValue {
		name,
		flags,
		kind: StatAxisValueKind::Range {
			nominal_value: nominal,
			range_min_value: min,
			range_max_value: max,
		},
	}
}

fn value(name: &str, flags: u16, value: f64, linked: Option<f64>) -> StatAxisValue {
	StatAxisValue {
		name: en(name),
		flags,
		kind: StatAxisValueKind::Value {
			value,
			linked_value: linked,
		},
	}
}

fn axis(tag: &[u8; 4], name: StatName, ordering: u16, values: Vec<StatAxisValue>) -> StatAxis {
	StatAxis {
		tag: Tag::new(tag),
		name,
		ordering: Some(ordering),
		values,
	}
}

#[test]
fn test_get_stat_axes() -> TestResult {
	let doc = doc()?;
	let weight_name = || names(&[("en", "Wéíght"), ("fa-IR", "قطر")]);
	let width_name = || names(&[("en", "Width"), ("fr", "Chasse")]);
	assert_eq!(
		get_stat_axes(&doc, &region(0.0, range(50, 150), range(200, 900)))?,
		vec![
			axis(
				b"wght",
				weight_name(),
				2,
				vec![
					ranged(
						names(&[
							("en", "Extra Light"),
							("de", "Extraleicht"),
							("fr", "Extra léger")
						]),
						0,
						200.0,
						Some(200.0),
						Some(250.0)
					),
					ranged(en("Light"), 0, 300.0, Some(250.0), Some(350.0)),
					ranged(en("Regular"), 2, 400.0, Some(350.0), Some(450.0)),
					ranged(en("Semi Bold"), 0, 600.0, Some(450.0), Some(650.0)),
					ranged(en("Bold"), 0, 700.0, Some(650.0), Some(850.0)),
					ranged(en("Black"), 0, 900.0, Some(850.0), Some(900.0)),
					value("Regular", 2, 400.0, Some(700.0)),
					value("Bold", 0, 700.0, Some(400.0)),
				]
			),
			axis(
				b"wdth",
				width_name(),
				1,
				vec![
					value("Condensed", 0, 50.0, None),
					value("Normal", 3, 100.0, None),
					value("Wide", 0, 125.0, None),
					ranged(en("Extra Wide"), 0, 150.0, Some(150.0), None),
				]
			),
			axis(
				b"ital",
				en("Italic"),
				3,
				vec![value("Roman", 2, 0.0, Some(1.0))]
			),
		]
	);
	assert_eq!(
		get_stat_axes(
			&doc,
			&region(1.0, RegionValue::Value(Number::Int(100)), range(400, 700))
		)?,
		vec![
			axis(
				b"wght",
				weight_name(),
				2,
				vec![
					ranged(en("Regular"), 2, 400.0, Some(350.0), Some(450.0)),
					ranged(en("Semi Bold"), 0, 600.0, Some(450.0), Some(650.0)),
					ranged(en("Bold"), 0, 700.0, Some(650.0), Some(850.0)),
					value("Regular", 2, 400.0, Some(700.0)),
					value("Bold", 0, 700.0, Some(400.0)),
				]
			),
			axis(
				b"wdth",
				width_name(),
				1,
				vec![value("Normal", 3, 100.0, None)]
			),
			axis(
				b"ital",
				en("Italic"),
				3,
				vec![value("Italic", 0, 1.0, None)]
			),
		]
	);
	Ok(())
}

fn location(name: StatName, values: [(&[u8; 4], f64); 3]) -> StatLocation {
	StatLocation {
		name,
		flags: 0,
		location: values.iter().map(|(t, v)| (Tag::new(t), *v)).collect(),
	}
}

#[test]
fn test_get_stat_locations() -> TestResult {
	let doc = doc()?;
	let sorted = |r: Region| -> TestResult<Vec<StatLocation>> {
		let mut locs = get_stat_locations(&doc, &r)?;
		for l in &mut locs {
			l.location.sort_by_key(|a| a.0);
		}
		Ok(locs)
	};
	assert_eq!(
		sorted(region(0.0, range(50, 150), range(200, 900)))?,
		vec![location(
			names(&[("en", "Some Style"), ("fr", "Un Style")]),
			[(b"ital", 0.0), (b"wdth", 50.0), (b"wght", 300.0)]
		)]
	);
	assert_eq!(
		sorted(region(1.0, range(50, 150), range(200, 900)))?,
		vec![location(
			en("Other"),
			[(b"ital", 1.0), (b"wdth", 100.0), (b"wght", 700.0)]
		)]
	);
	Ok(())
}

fn build_vf_stat_table_test(with_mac_names: bool) -> TestResult {
	let doc = doc()?;
	let mut font = Font::new();
	let mut name = Name::default();
	name.names.clear();
	if with_mac_names {
		// addName adds a name string for both Macintosh and Windows platforms by
		// default.
		name.add_name_default("Regular")?;
	}
	font.insert(Box::new(name));
	build_vf_stat_table(&mut font, &doc, "Test_WghtWdth")?;
	assert!(font.contains(Tag::new(b"STAT")));
	let names = &font.require::<Name>()?.names;
	let ids: std::collections::BTreeSet<u16> = names.iter().map(|n| n.name_id).collect();
	assert_eq!(ids.len(), 15);
	// Mac names don't get added if there weren't any before.
	let mac = names.iter().filter(|n| n.platform_id == 1).count();
	if with_mac_names {
		assert!(mac > 1);
	} else {
		assert_eq!(mac, 0);
	}
	Ok(())
}

#[test]
fn test_build_vf_stat_table_with_mac_names() -> TestResult {
	build_vf_stat_table_test(true)
}

#[test]
fn test_build_vf_stat_table_without_mac_names() -> TestResult {
	build_vf_stat_table_test(false)
}
