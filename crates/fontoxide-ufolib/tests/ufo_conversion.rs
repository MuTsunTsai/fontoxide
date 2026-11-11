//! Port of fontTools' `Tests/ufoLib/UFOConversion_test.py`.

mod common;

use std::path::Path;

use common::{py_dict, py_value, read_plist, tempdir, write_plist};
use fontoxide_ufolib::converters::GroupRenameMaps;
use fontoxide_ufolib::{
	Groups, Kerning, Result, UfoReader, UfoWriter, UfoWriterOptions, Value, groups_to_value,
};
use indexmap::IndexMap;

/// `_makeKerningConversionUFO`: a minimal UFO for kerning conversion tests.
fn make_kerning_conversion_ufo(ufo_path: &Path, format_version: i64) {
	if ufo_path.exists() {
		std::fs::remove_dir_all(ufo_path).expect("rmtree");
	}
	std::fs::create_dir(ufo_path).expect("mkdir");
	let glyphs_path = ufo_path.join("glyphs");
	std::fs::create_dir(&glyphs_path).expect("mkdir");
	write_plist(
		&glyphs_path.join("contents.plist"),
		&py_value("dict(X='X_.glif')"),
	);
	std::fs::write(
		glyphs_path.join("X_.glif"),
		"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
	)
	.expect("write");
	write_plist(
		&ufo_path.join("metainfo.plist"),
		&py_value(&format!(
			"dict(creator='test', formatVersion={format_version})"
		)),
	);
	write_plist(
		&ufo_path.join("kerning.plist"),
		&py_value(
			r#"{
				"A": {"A": 1, "B": 2, "CGroup": 3, "DGroup": 4},
				"BGroup": {"A": 5, "B": 6, "CGroup": 7, "DGroup": 8},
				"CGroup": {"A": 9, "B": 10, "CGroup": 11, "DGroup": 12},
				"X": {"A": 13, "CGroup": 14},
			}"#,
		),
	);
	write_plist(
		&ufo_path.join("groups.plist"),
		&py_value(
			r#"{
				"BGroup": ["B"],
				"CGroup": ["C", "Ccedilla"],
				"DGroup": ["D"],
				"Not A Kerning Group": ["A"],
				"X": ["X", "X.sc"],
			}"#,
		),
	);
	write_plist(
		&ufo_path.join("fontinfo.plist"),
		&py_value("{'familyName': 'Test'}"),
	);
}

/// A kerning dictionary from `(first, second, value)` triples.
fn kerning(pairs: &[(&str, &str, i64)]) -> Kerning {
	pairs
		.iter()
		.map(|(a, b, v)| (((*a).to_owned(), (*b).to_owned()), Value::from(*v)))
		.collect()
}

fn groups(text: &str) -> Groups {
	py_dict(text)
		.into_iter()
		.map(|(k, v)| {
			let Value::Array(a) = v else {
				panic!("list");
			};
			(
				k,
				a.into_iter()
					.map(|g| g.as_str().expect("str").to_owned())
					.collect(),
			)
		})
		.collect()
}

/// Dictionaries compare regardless of order.
fn sorted_kerning(k: &Kerning) -> Vec<((String, String), Value)> {
	let mut v: Vec<_> = k.iter().map(|(a, b)| (a.clone(), b.clone())).collect();
	v.sort_by(|a, b| a.0.cmp(&b.0));
	v
}

// KerningUpConversionTestCase

fn expected_up_kerning() -> Kerning {
	kerning(&[
		("public.kern1.BGroup", "public.kern2.CGroup", 7),
		("public.kern1.BGroup", "public.kern2.DGroup", 8),
		("public.kern1.BGroup", "A", 5),
		("public.kern1.BGroup", "B", 6),
		("public.kern1.CGroup", "public.kern2.CGroup", 11),
		("public.kern1.CGroup", "public.kern2.DGroup", 12),
		("public.kern1.CGroup", "A", 9),
		("public.kern1.CGroup", "B", 10),
		("A", "public.kern2.CGroup", 3),
		("A", "public.kern2.DGroup", 4),
		("A", "A", 1),
		("A", "B", 2),
		("X", "A", 13),
		("X", "public.kern2.CGroup", 14),
	])
}

const EXPECTED_UP_GROUPS: &str = r#"{
	"BGroup": ["B"],
	"CGroup": ["C", "Ccedilla"],
	"DGroup": ["D"],
	"public.kern1.BGroup": ["B"],
	"public.kern1.CGroup": ["C", "Ccedilla"],
	"public.kern2.CGroup": ["C", "Ccedilla"],
	"public.kern2.DGroup": ["D"],
	"Not A Kerning Group": ["A"],
	"X": ["X", "X.sc"],
}"#;

fn up_conversion(format_version: i64) -> Result<()> {
	let dir = tempdir("upconversion");
	let ufo_path = dir.join("test.ufo");
	make_kerning_conversion_ufo(&ufo_path, format_version);
	let reader = UfoReader::new(&ufo_path, true)?;
	let kerning = reader.read_kerning(None)?;
	assert_eq!(
		sorted_kerning(&expected_up_kerning()),
		sorted_kerning(&kerning)
	);
	let groups = reader.read_groups(None)?;
	assert_eq!(groups_to_value(&groups), py_value(EXPECTED_UP_GROUPS));
	reader.read_info(None)?;
	Ok(())
}

#[test]
fn test_ufo1() -> Result<()> {
	// as in fontTools, the UFO made is a UFO 2
	up_conversion(2)
}

#[test]
fn test_ufo2() -> Result<()> {
	up_conversion(2)
}

// KerningDownConversionTestCase

fn down_kerning() -> Kerning {
	kerning(&[
		("public.kern1.BGroup", "public.kern2.CGroup", 7),
		("public.kern1.BGroup", "public.kern2.DGroup", 8),
		("public.kern1.BGroup", "A", 5),
		("public.kern1.BGroup", "B", 6),
		("public.kern1.CGroup", "public.kern2.CGroup", 11),
		("public.kern1.CGroup", "public.kern2.DGroup", 12),
		("public.kern1.CGroup", "A", 9),
		("public.kern1.CGroup", "B", 10),
		("A", "public.kern2.CGroup", 3),
		("A", "public.kern2.DGroup", 4),
		("A", "A", 1),
		("A", "B", 2),
	])
}

const DOWN_GROUPS: &str = r#"{
	"BGroup": ["B"],
	"CGroup": ["C"],
	"DGroup": ["D"],
	"public.kern1.BGroup": ["B"],
	"public.kern1.CGroup": ["C", "Ccedilla"],
	"public.kern2.CGroup": ["C", "Ccedilla"],
	"public.kern2.DGroup": ["D"],
	"Not A Kerning Group": ["A"],
}"#;

const EXPECTED_WRITTEN_GROUPS: &str = r#"{
	"BGroup": ["B"],
	"CGroup": ["C", "Ccedilla"],
	"DGroup": ["D"],
	"Not A Kerning Group": ["A"],
}"#;

const EXPECTED_WRITTEN_KERNING: &str = r#"{
	"BGroup": {"CGroup": 7, "DGroup": 8, "A": 5, "B": 6},
	"CGroup": {"CGroup": 11, "DGroup": 12, "A": 9, "B": 10},
	"A": {"CGroup": 3, "DGroup": 4, "A": 1, "B": 2},
}"#;

fn down_conversion_mapping() -> GroupRenameMaps {
	let map = |items: &[(&str, &str)]| -> IndexMap<String, String> {
		items
			.iter()
			.map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
			.collect()
	};
	GroupRenameMaps {
		side1: map(&[
			("BGroup", "public.kern1.BGroup"),
			("CGroup", "public.kern1.CGroup"),
		]),
		side2: map(&[
			("CGroup", "public.kern2.CGroup"),
			("DGroup", "public.kern2.DGroup"),
		]),
	}
}

fn writer2(path: &Path) -> Result<UfoWriter> {
	UfoWriter::new(
		path,
		UfoWriterOptions {
			format_version: Some(2.into()),
			..Default::default()
		},
	)
}

#[test]
fn test_write() -> Result<()> {
	let dir = tempdir("downwrite");
	let dst = dir.join("test.ufo");
	let mut writer = writer2(&dst)?;
	writer.set_kerning_group_conversion_rename_maps(&down_conversion_mapping());
	writer.write_kerning(&down_kerning(), None)?;
	writer.write_groups(&groups(DOWN_GROUPS), None)?;
	// test groups
	assert_eq!(
		read_plist(&dst.join("groups.plist")),
		py_value(EXPECTED_WRITTEN_GROUPS)
	);
	// test kerning
	assert_eq!(
		read_plist(&dst.join("kerning.plist")),
		py_value(EXPECTED_WRITTEN_KERNING)
	);
	Ok(())
}

#[test]
fn test_get_kerning_group_conversion_rename_maps() -> Result<()> {
	let dir = tempdir("renamemaps");
	let dst = dir.join("test.ufo");
	make_kerning_conversion_ufo(&dst, 2);
	let reader = UfoReader::new(&dst, true)?;
	let rename_maps = reader.get_kerning_group_conversion_rename_maps(None)?;
	let sorted = |m: &IndexMap<String, String>| {
		let mut v: Vec<_> = m.iter().map(|(a, b)| (a.clone(), b.clone())).collect();
		v.sort();
		v
	};
	let expected = down_conversion_mapping();
	assert_eq!(sorted(&rename_maps.side1), sorted(&expected.side1));
	assert_eq!(sorted(&rename_maps.side2), sorted(&expected.side2));
	Ok(())
}

#[test]
fn test_kerning_group_conversion_round_trip() -> Result<()> {
	let dir = tempdir("roundtrip");
	let dst = dir.join("test.ufo");
	make_kerning_conversion_ufo(&dst, 2);
	// Read legacy UFO2 data and capture the conversion maps.
	let reader = UfoReader::new(&dst, true)?;
	let groups = reader.read_groups(None)?;
	let kerning = reader.read_kerning(None)?;
	let rename_maps = reader.get_kerning_group_conversion_rename_maps(None)?;
	drop(reader);
	std::fs::remove_dir_all(&dst)?;
	// Write the data back out using the captured rename maps.
	let mut writer = writer2(&dst)?;
	writer.set_kerning_group_conversion_rename_maps(&rename_maps);
	writer.write_kerning(&kerning, None)?;
	writer.write_groups(&groups, None)?;
	writer.close()?;
	// Verify the down-converted groups keep their original UFO2 names.
	let written_groups = read_plist(&dst.join("groups.plist"));
	let written_groups = written_groups.as_dict().expect("dict");
	for (key, value) in py_dict(EXPECTED_WRITTEN_GROUPS) {
		assert_eq!(written_groups.get(&key), Some(&value), "{key}");
	}
	for key in written_groups.keys() {
		assert!(!key.starts_with("public.kern"), "{key}");
	}
	// Confirm the kerning pairs were restored to their UFO2 form.
	let written_kerning = read_plist(&dst.join("kerning.plist"));
	let written_kerning = written_kerning.as_dict().expect("dict");
	for (key, value) in py_dict(EXPECTED_WRITTEN_KERNING) {
		assert_eq!(written_kerning.get(&key), Some(&value), "{key}");
	}
	for (key, sub) in written_kerning {
		assert!(!key.starts_with("public.kern"), "{key}");
		let sub = sub.as_dict().expect("dict");
		assert!(sub.keys().all(|k| !k.starts_with("public.kern")));
	}
	Ok(())
}
