//! `Tests/varLib/avar_test.py`.

mod common;

use common::{TestResult, tests_path};
use fontoxide_designspacelib::DesignSpaceDocument;
use fontoxide_ttlib::tables::fvar::Axis;
use fontoxide_ttlib::tables::{Fvar, Name};
use fontoxide_ttlib::{Font, ReadOptions};
use fontoxide_varlib::avar::{build, map, map_main, mappings_from_avar, prune_locations, unbuild};
use fontoxide_varlib::errors::VarLibError;
use fontoxide_varlib::models::{DictLocation, VariationModel};

type Case = (Vec<DictLocation>, Vec<DictLocation>, Vec<DictLocation>);

fn loc(items: &[(&str, f64)]) -> DictLocation {
	items.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
}

fn tests() -> Vec<Case> {
	vec![
		(
			vec![loc(&[("wght", 1.0)]), loc(&[("wght", 0.5)])],
			vec![loc(&[("wght", 0.5)])],
			vec![loc(&[("wght", 0.5)])],
		),
		(
			vec![
				loc(&[("wght", 1.0), ("wdth", 1.0)]),
				loc(&[("wght", 0.5), ("wdth", 1.0)]),
			],
			vec![loc(&[("wght", 1.0), ("wdth", 1.0)])],
			vec![
				loc(&[("wght", 1.0), ("wdth", 1.0)]),
				loc(&[("wght", 0.5), ("wdth", 1.0)]),
			],
		),
		(
			vec![
				loc(&[("wght", 1.0)]),
				loc(&[("wdth", 1.0)]),
				loc(&[("wght", 0.5), ("wdth", 0.5)]),
			],
			vec![loc(&[("wght", 0.5), ("wdth", 0.5)])],
			vec![loc(&[("wght", 0.5), ("wdth", 0.5)])],
		),
	]
}

fn axis_tags(locations: &[DictLocation]) -> Vec<String> {
	let mut tags: Vec<String> = locations.iter().flatten().map(|(k, _)| k.clone()).collect();
	tags.sort();
	tags.dedup();
	tags
}

/// `dict == dict`.
fn same(a: &DictLocation, b: &DictLocation) -> bool {
	a.len() == b.len()
		&& a.iter()
			.all(|(k, v)| b.iter().any(|(k2, v2)| k == k2 && v == v2))
}

#[test]
fn test_prune_locations() -> TestResult {
	for (locations, poles, expected) in tests() {
		let tags = axis_tags(&locations);
		let mut all = vec![DictLocation::new()];
		all.extend(locations);
		let pruned = prune_locations(&all, &poles, &tags)?;
		assert_eq!(pruned.len(), expected.len(), "{pruned:?}");
		for (p, e) in pruned.iter().zip(&expected) {
			assert!(same(p, e), "{pruned:?} {expected:?}");
		}
	}
	Ok(())
}

#[test]
fn test_roundtrip() -> TestResult {
	for (locations, poles, expected) in tests() {
		let tags = axis_tags(&locations);
		let mut l1 = vec![DictLocation::new()];
		l1.extend(locations);
		let mut l2 = vec![DictLocation::new()];
		l2.extend(expected);
		let model1 = VariationModel::new(&l1, Some(&tags), false, None)?;
		let model2 = VariationModel::new(&l2, Some(&tags), false, None)?;
		for location in &poles {
			let i = model1
				.locations
				.iter()
				.position(|l| same(l, location))
				.expect("pole");
			let j = model2
				.locations
				.iter()
				.position(|l| same(l, location))
				.expect("pole");
			let mut s1 = model1.supports[i].clone();
			let mut s2 = model2.supports[j].clone();
			s1.sort_by(|a, b| a.0.cmp(&b.0));
			s2.sort_by(|a, b| a.0.cmp(&b.0));
			assert_eq!(s1, s2);
		}
	}
	Ok(())
}

#[test]
fn test_mappings_from_avar() -> TestResult {
	let mut font = Font::read(
		std::fs::read(tests_path("ttLib/tables/data/Amstelvar-avar2.subset.ttf"))?,
		ReadOptions::default(),
	)?;
	let mappings = mappings_from_avar(&mut font, true)?;
	// `len((axisMaps, mappings)) == 2`.
	let _ = mappings;
	Ok(())
}

fn fvar_font(axis_name_id: u16) -> Font {
	let mut font = Font::new();
	let mut fvar = Fvar::default();
	fvar.axes.push(Axis {
		axis_tag: *b"wght",
		min_value: 100.0,
		default_value: 400.0,
		max_value: 900.0,
		flags: 0,
		axis_name_id,
	});
	font.insert(Box::new(fvar));
	font
}

#[test]
fn test_mappings_from_avar_without_avar_table() -> TestResult {
	let mut font = Font::new();
	font.insert(Box::new(Fvar::default()));
	let (axis_maps, mappings) = mappings_from_avar(&mut font, true)?;
	assert!(axis_maps.is_empty());
	assert!(mappings.is_empty());
	Ok(())
}

#[test]
fn test_unbuild_falls_back_to_axis_tag_when_name_missing() -> TestResult {
	let mut font = fvar_font(999);
	font.insert(Box::new(Name::default()));
	assert!(unbuild(&mut font)?.contains("name=\"wght\""));
	Ok(())
}

#[test]
fn test_map_rejects_unknown_axis() {
	let mut font = fvar_font(256);
	let err =
		map(&mut font, &[("wdth".to_owned(), 100.0)], false, false, true).expect_err("an error");
	assert!(err.to_string().contains("Unknown axis tag"), "{err}");
}

const DESIGNSPACE: &str = "<?xml version='1.0' encoding='UTF-8'?>
<designspace format=\"5.0\">
  <axes>
    <axis tag=\"wght\" name=\"Weight\" minimum=\"100\" maximum=\"900\" default=\"400\"/>
  </axes>
</designspace>
";

fn temp_designspace(name: &str) -> TestResult<std::path::PathBuf> {
	let dir = std::env::temp_dir().join(format!("fontoxide-avar-{}-{name}", std::process::id()));
	std::fs::create_dir_all(&dir)?;
	let path = dir.join("Test.designspace");
	std::fs::write(&path, DESIGNSPACE)?;
	Ok(path)
}

#[test]
fn test_map_main_rejects_malformed_coordinate() -> TestResult {
	let path = temp_designspace("malformed")?;
	let err = map_main(&[&path.to_string_lossy(), "wght"]).expect_err("an error");
	let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
	assert!(matches!(err, VarLibError::Usage(_)), "{err:?}");
	Ok(())
}

#[test]
fn test_build_preserves_existing_name_table() -> TestResult {
	let path = temp_designspace("preserve")?;
	let mut font = Font::new();
	let mut name = Name::default();
	name.set_name("Existing Name", 320, 3, 1, 0x409);
	font.insert(Box::new(name));
	let mut ds = DesignSpaceDocument::from_file(&path)?;
	build(&mut font, &mut ds)?;
	let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
	assert_eq!(
		font.require::<Name>()?.get_debug_name(320).as_deref(),
		Some("Existing Name")
	);
	Ok(())
}

#[test]
fn test_unbuild_escapes_axis_name_in_designspace() -> TestResult {
	let mut font = fvar_font(256);
	let mut name = Name::default();
	name.names.clear();
	let hostile = "W\" /><!DOCTYPE x><axis tag=\"z";
	name.set_name(hostile, 256, 3, 1, 0x409);
	font.insert(Box::new(name));
	let out = unbuild(&mut font)?;
	// No unescaped markup leaked through.
	assert!(!out.contains("<!DOCTYPE"));
	// The output is well-formed and the hostile string survives as the value of
	// the attribute of the single <axis> element.
	let root = fontoxide_misc::xml_reader::parse(&out)?;
	let mut axes = Vec::new();
	fn find<'a>(
		e: &'a fontoxide_misc::xml_reader::Element,
		out: &mut Vec<&'a fontoxide_misc::xml_reader::Element>,
	) {
		if e.name == "axis" {
			out.push(e);
		}
		for c in e.elements() {
			find(c, out);
		}
	}
	find(&root, &mut axes);
	assert_eq!(axes.len(), 1);
	assert_eq!(axes[0].attr("name"), Some(hostile));
	Ok(())
}
