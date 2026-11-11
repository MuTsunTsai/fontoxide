//! `Tests/designspaceLib/designspace_test.py`.

mod common;

use common::{
	assert_equals_test_file, assert_err_contains, copy_data, datadir, fail, read_text, s, tmpdir,
};
use fontoxide_designspacelib::paths::{abspath_str, join, relpath};
use fontoxide_designspacelib::plistlib::{self, Value};
use fontoxide_designspacelib::types::Range;
use fontoxide_designspacelib::{
	Axis, AxisDescriptor, AxisMappingDescriptor, Condition, DesignSpaceDocument,
	DiscreteAxisDescriptor, GlyphData, GlyphMaster, InstanceDescriptor, LocationValue, Number,
	Result, RuleDescriptor, SourceDescriptor, anisotropic_location, evaluate_rule, posix,
	process_rules, simple_location,
};

fn n(v: i64) -> Number {
	Number::Int(v)
}

fn f(v: f64) -> Number {
	Number::Float(v)
}

fn axis(name: &str, tag: &str, minimum: i64, default: i64, maximum: i64) -> AxisDescriptor {
	AxisDescriptor {
		name: Some(name.into()),
		tag: Some(tag.into()),
		minimum: n(minimum),
		default: n(default),
		maximum: n(maximum),
		..Default::default()
	}
}

fn map(pairs: &[(f64, f64)]) -> Vec<(Number, Number)> {
	pairs.iter().map(|&(a, b)| (num(a), num(b))).collect()
}

/// An `int` for integral values, as the Python literals in the tests.
fn num(v: f64) -> Number {
	if v.fract() == 0.0 { n(v as i64) } else { f(v) }
}

fn cond(name: &str, minimum: Option<f64>, maximum: Option<f64>) -> Condition {
	Condition::new(name, minimum.map(num), maximum.map(num))
}

fn names(items: &[(&str, &str)]) -> fontoxide_designspacelib::LocalizedNames {
	items
		.iter()
		.map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
		.collect()
}

fn rel(target: &str, doc_path: &str) -> Result<String> {
	relpath(target, &fontoxide_designspacelib::paths::dirname(doc_path))
}

#[test]
fn test_fill_document() -> Result<()> {
	let tmp = tmpdir("fill_document")?;
	let test_doc_path = s(&tmp.join("test_v4.designspace"));
	let test_doc_path5 = s(&tmp.join("test_v5.designspace"));
	let master_path1 = s(&tmp.join("masters").join("masterTest1.ufo"));
	let master_path2 = s(&tmp.join("masters").join("masterTest2.ufo"));
	let instance_path1 = s(&tmp.join("instances").join("instanceTest1.ufo"));
	let instance_path2 = s(&tmp.join("instances").join("instanceTest2.ufo"));
	let mut doc = DesignSpaceDocument::new();
	doc.rules_processing_last = true;

	// write some axes
	let mut a1 = axis("weight", "wght", 0, 0, 1000);
	// note: just to test the element language, not an actual label name recommendations.
	a1.label_names.insert("fa-IR".into(), "قطر".into());
	a1.label_names.insert("en".into(), "Wéíght".into());
	doc.add_axis(a1);
	let mut a2 = axis("width", "wdth", 0, 15, 1000);
	a2.map = vec![
		(f(0.0), f(10.0)),
		(f(15.0), f(20.0)),
		(f(401.0), f(66.0)),
		(f(1000.0), f(990.0)),
	];
	a2.hidden = true;
	a2.label_names.insert("fr".into(), "Chasse".into());
	doc.add_axis(a2);

	// add master 1
	let mut s1 = SourceDescriptor::new();
	s1.set_filename(Some(&rel(&master_path1, &test_doc_path)?));
	s1.name = Some("master.ufo1".into());
	s1.copy_lib = true;
	s1.copy_info = true;
	s1.copy_features = true;
	s1.design_location = anisotropic_location([("weight", 0)]);
	s1.family_name = Some("MasterFamilyName".into());
	s1.style_name = Some("MasterStyleNameOne".into());
	s1.muted_glyph_names.push("A".into());
	s1.muted_glyph_names.push("Z".into());
	doc.add_source(s1);
	// add master 2
	let mut s2 = SourceDescriptor::new();
	s2.set_filename(Some(&rel(&master_path2, &test_doc_path)?));
	s2.name = Some("master.ufo2".into());
	s2.mute_kerning = true;
	s2.design_location = anisotropic_location([("weight", 1000)]);
	s2.family_name = Some("MasterFamilyName".into());
	s2.style_name = Some("MasterStyleNameTwo".into());
	doc.add_source(s2);
	// add master 3 from a different layer
	let mut s3 = SourceDescriptor::new();
	s3.set_filename(Some(&rel(&master_path2, &test_doc_path)?));
	s3.name = Some("master.ufo2".into());
	s3.layer_name = Some("supports".into());
	s3.design_location = anisotropic_location([("weight", 1000)]);
	s3.family_name = Some("MasterFamilyName".into());
	s3.style_name = Some("Supports".into());
	doc.add_source(s3);
	// add instance 1
	let mut i1 = InstanceDescriptor::new();
	i1.set_filename(Some(&rel(&instance_path1, &test_doc_path)?));
	i1.family_name = Some("InstanceFamilyName".into());
	i1.style_name = Some("InstanceStyleName".into());
	i1.name = Some("instance.ufo1".into());
	// this adds a dimension that is not defined.
	i1.design_location = anisotropic_location([("weight", 500), ("spooky", 666)]);
	i1.post_script_font_name = Some("InstancePostscriptName".into());
	i1.style_map_family_name = Some("InstanceStyleMapFamilyName".into());
	i1.style_map_style_name = Some("InstanceStyleMapStyleName".into());
	i1.localised_style_name = names(&[("fr", "Demigras"), ("ja", "半ば")]);
	i1.localised_family_name = names(&[("fr", "Montserrat"), ("ja", "モンセラート")]);
	i1.localised_style_map_style_name = names(&[("de", "Standard")]);
	i1.localised_style_map_family_name = names(&[
		("de", "Montserrat Halbfett"),
		("ja", "モンセラート SemiBold"),
	]);
	i1.glyphs.insert(
		"arrow".into(),
		GlyphData {
			mute: true,
			unicodes: Some(vec![0x123, 0x124, 0x125]),
			..Default::default()
		},
	);
	i1.lib.insert(
		"com.coolDesignspaceApp.binaryData".into(),
		Value::Data(b"<binary gunk>".to_vec()),
	);
	i1.lib.insert(
		"com.coolDesignspaceApp.specimenText".into(),
		"Hamburgerwhatever".into(),
	);
	doc.add_instance(i1);
	// add instance 2
	let mut i2 = InstanceDescriptor::new();
	i2.set_filename(Some(&rel(&instance_path2, &test_doc_path)?));
	i2.family_name = Some("InstanceFamilyName".into());
	i2.style_name = Some("InstanceStyleName".into());
	i2.name = Some("instance.ufo2".into());
	// anisotropic location
	i2.design_location = anisotropic_location([
		("weight", LocationValue::from(500)),
		("width", LocationValue::from((400, 300))),
	]);
	i2.post_script_font_name = Some("InstancePostscriptName".into());
	i2.style_map_family_name = Some("InstanceStyleMapFamilyName".into());
	i2.style_map_style_name = Some("InstanceStyleMapStyleName".into());
	let glyph_masters = vec![
		GlyphMaster {
			font: Some("master.ufo1".into()),
			glyph_name: Some("BB".into()),
			location: Some(anisotropic_location([("width", 20), ("weight", 20)])),
		},
		GlyphMaster {
			font: Some("master.ufo2".into()),
			glyph_name: Some("CC".into()),
			location: Some(anisotropic_location([("width", 900), ("weight", 900)])),
		},
	];
	i2.glyphs.insert(
		"arrow".into(),
		GlyphData {
			unicodes: Some(vec![101, 201, 301]),
			masters: Some(glyph_masters),
			note: Some("A note about this glyph".into()),
			instance_location: Some(anisotropic_location([("width", 100), ("weight", 120)])),
			..Default::default()
		},
	);
	i2.glyphs.insert("arrow2".into(), GlyphData::default());
	doc.add_instance(i2);

	doc.filename = Some("suggestedFileName.designspace".into());
	doc.lib.insert(
		"com.coolDesignspaceApp.previewSize".into(),
		Value::Integer(30),
	);

	// write some rules
	let mut r1 = RuleDescriptor {
		name: Some("named.rule.1".into()),
		..Default::default()
	};
	r1.condition_sets.push(vec![
		cond("axisName_a", Some(0.0), Some(1.0)),
		cond("axisName_b", Some(2.0), Some(3.0)),
	]);
	r1.subs.push(("a".into(), "a.alt".into()));
	doc.add_rule(r1);
	// write the document; without an explicit format it will be 5.0 by default
	doc.write(&test_doc_path5)?;
	assert!(std::path::Path::new(&test_doc_path5).exists());
	assert_equals_test_file(
		std::path::Path::new(&test_doc_path5),
		"test_v5_original.designspace",
	)?;
	// write again with an explicit format = 4.1
	doc.format_version = Some("4.1".into());
	doc.write(&test_doc_path)?;
	assert!(std::path::Path::new(&test_doc_path).exists());
	assert_equals_test_file(
		std::path::Path::new(&test_doc_path),
		"test_v4_original.designspace",
	)?;
	// import it again
	let mut new = DesignSpaceDocument::new();
	new.read(&test_doc_path)?;

	let default = new.default_source().ok_or_else(|| fail("no default"))?;
	assert_eq!(
		default.design_location,
		anisotropic_location([("width", 20.0), ("weight", 0.0)])
	);
	assert_eq!(new.filename.as_deref(), Some("test_v4.designspace"));
	assert_eq!(new.lib, doc.lib);
	assert_eq!(new.instances[0].lib, doc.instances[0].lib);

	// test roundtrip for the axis attributes and data
	assert_eq!(new.axes, doc.axes);
	Ok(())
}

#[test]
fn test_unicodes() -> Result<()> {
	let tmp = tmpdir("unicodes")?;
	let test_doc_path = s(&tmp.join("testUnicodes.designspace"));
	let test_doc_path2 = s(&tmp.join("testUnicodes_roundtrip.designspace"));
	let master_path1 = s(&tmp.join("masters").join("masterTest1.ufo"));
	let master_path2 = s(&tmp.join("masters").join("masterTest2.ufo"));
	let instance_path1 = s(&tmp.join("instances").join("instanceTest1.ufo"));
	let mut doc = DesignSpaceDocument::new();
	doc.format_version = Some("4.1".into()); // This test about instance glyphs is deprecated in v5
	let mut s1 = SourceDescriptor::new();
	s1.set_filename(Some(&rel(&master_path1, &test_doc_path)?));
	s1.name = Some("master.ufo1".into());
	s1.copy_info = true;
	s1.design_location = anisotropic_location([("weight", 0)]);
	doc.add_source(s1);
	let mut s2 = SourceDescriptor::new();
	s2.set_filename(Some(&rel(&master_path2, &test_doc_path)?));
	s2.name = Some("master.ufo2".into());
	s2.design_location = anisotropic_location([("weight", 1000)]);
	doc.add_source(s2);
	let mut i1 = InstanceDescriptor::new();
	i1.set_filename(Some(&rel(&instance_path1, &test_doc_path)?));
	i1.name = Some("instance.ufo1".into());
	i1.design_location = anisotropic_location([("weight", 500)]);
	i1.glyphs.insert(
		"arrow".into(),
		GlyphData {
			mute: true,
			unicodes: Some(vec![100, 200, 300]),
			..Default::default()
		},
	);
	doc.add_instance(i1);
	// now we have sources and instances, but no axes yet.
	doc.axes.clear(); // clear the axes
	doc.add_axis(axis("weight", "wght", 0, 0, 1000));
	doc.write(&test_doc_path)?;
	assert!(std::path::Path::new(&test_doc_path).exists());
	// import it again
	let mut new = DesignSpaceDocument::new();
	new.read(&test_doc_path)?;
	new.write(&test_doc_path2)?;
	// compare the file contents
	let t1 = read_text(std::path::Path::new(&test_doc_path))?;
	let t2 = read_text(std::path::Path::new(&test_doc_path2))?;
	assert_eq!(t1, t2);
	// check the unicode values read from the document
	assert_eq!(
		new.instances[0].glyphs["arrow"].unicodes,
		Some(vec![100, 200, 300])
	);
	Ok(())
}

#[test]
fn test_localised_names() -> Result<()> {
	let tmp = tmpdir("localised_names")?;
	let test_doc_path = s(&tmp.join("testLocalisedNames.designspace"));
	let test_doc_path2 = s(&tmp.join("testLocalisedNames_roundtrip.designspace"));
	let master_path1 = s(&tmp.join("masters").join("masterTest1.ufo"));
	let master_path2 = s(&tmp.join("masters").join("masterTest2.ufo"));
	let instance_path1 = s(&tmp.join("instances").join("instanceTest1.ufo"));
	let mut doc = DesignSpaceDocument::new();
	let mut s1 = SourceDescriptor::new();
	s1.set_filename(Some(&rel(&master_path1, &test_doc_path)?));
	s1.name = Some("master.ufo1".into());
	s1.copy_info = true;
	s1.design_location = anisotropic_location([("weight", 0)]);
	doc.add_source(s1);
	let mut s2 = SourceDescriptor::new();
	s2.set_filename(Some(&rel(&master_path2, &test_doc_path)?));
	s2.name = Some("master.ufo2".into());
	s2.design_location = anisotropic_location([("weight", 1000)]);
	doc.add_source(s2);
	let mut i1 = InstanceDescriptor::new();
	i1.set_filename(Some(&rel(&instance_path1, &test_doc_path)?));
	i1.family_name = Some("Montserrat".into());
	i1.style_name = Some("SemiBold".into());
	i1.style_map_family_name = Some("Montserrat SemiBold".into());
	i1.style_map_style_name = Some("Regular".into());
	i1.set_family_name("Montserrat", "fr");
	i1.set_family_name("モンセラート", "ja");
	i1.set_style_name("Demigras", "fr");
	i1.set_style_name("半ば", "ja");
	i1.set_style_map_style_name("Standard", "de");
	i1.set_style_map_family_name("Montserrat Halbfett", "de");
	i1.set_style_map_family_name("モンセラート SemiBold", "ja");
	i1.name = Some("instance.ufo1".into());
	// this adds a dimension that is not defined.
	i1.design_location = anisotropic_location([("weight", 500), ("spooky", 666)]);
	i1.post_script_font_name = Some("InstancePostscriptName".into());
	i1.glyphs.insert(
		"arrow".into(),
		GlyphData {
			mute: true,
			unicodes: Some(vec![0x123]),
			..Default::default()
		},
	);
	doc.add_instance(i1);
	// now we have sources and instances, but no axes yet.
	doc.axes.clear();
	let mut a1 = axis("weight", "wght", 0, 0, 1000);
	// note: just to test the element language, not an actual label name recommendations.
	a1.label_names.insert("fa-IR".into(), "قطر".into());
	a1.label_names.insert("en".into(), "Wéíght".into());
	doc.add_axis(a1);
	let mut a2 = axis("width", "wdth", 0, 0, 1000);
	a2.map = vec![
		(f(0.0), f(10.0)),
		(f(401.0), f(66.0)),
		(f(1000.0), f(990.0)),
	];
	a2.label_names.insert("fr".into(), "Poids".into());
	doc.add_axis(a2);
	// write some rules
	doc.add_rule(RuleDescriptor {
		name: Some("named.rule.1".into()),
		condition_sets: vec![vec![
			cond("weight", Some(200.0), Some(500.0)),
			cond("width", Some(0.0), Some(150.0)),
		]],
		subs: vec![("a".into(), "a.alt".into())],
	});
	doc.write(&test_doc_path)?;
	let mut new = DesignSpaceDocument::new();
	new.read(&test_doc_path)?;
	new.write(&test_doc_path2)?;
	let t1 = read_text(std::path::Path::new(&test_doc_path))?;
	let t2 = read_text(std::path::Path::new(&test_doc_path2))?;
	assert_eq!(t1, t2);
	Ok(())
}

#[test]
fn test_handle_no_axes() -> Result<()> {
	let tmp = tmpdir("handle_no_axes")?;
	// test what happens if the designspacedocument has no axes element.
	let test_doc_path = s(&tmp.join("testNoAxes_source.designspace"));
	let test_doc_path2 = s(&tmp.join("testNoAxes_recontructed.designspace"));
	let master_path1 = s(&tmp.join("masters").join("masterTest1.ufo"));
	let master_path2 = s(&tmp.join("masters").join("masterTest2.ufo"));
	let instance_path1 = s(&tmp.join("instances").join("instanceTest1.ufo"));

	// Case 1: No axes element in the document, but there are sources and instances
	let mut doc = DesignSpaceDocument::new();
	for (name, value) in [("One", 1), ("Two", 2), ("Three", 3)] {
		doc.add_axis(axis(
			&format!("axisName{name}"),
			&format!("ax_{value}"),
			0,
			0,
			1000,
		));
	}
	let mut s1 = SourceDescriptor::new();
	s1.set_filename(Some(&rel(&master_path1, &test_doc_path)?));
	s1.name = Some("master.ufo1".into());
	s1.copy_lib = true;
	s1.copy_info = true;
	s1.copy_features = true;
	s1.design_location = anisotropic_location([
		("axisNameOne", -1000),
		("axisNameTwo", 0),
		("axisNameThree", 1000),
	]);
	s1.family_name = Some("MasterFamilyName".into());
	s1.style_name = Some("MasterStyleNameOne".into());
	doc.add_source(s1);
	let mut s2 = SourceDescriptor::new();
	s2.set_filename(Some(&rel(&master_path2, &test_doc_path)?));
	s2.name = Some("master.ufo1".into());
	s2.design_location = anisotropic_location([
		("axisNameOne", 1000),
		("axisNameTwo", 1000),
		("axisNameThree", 0),
	]);
	s2.family_name = Some("MasterFamilyName".into());
	s2.style_name = Some("MasterStyleNameTwo".into());
	doc.add_source(s2);
	let mut i1 = InstanceDescriptor::new();
	i1.set_filename(Some(&rel(&instance_path1, &test_doc_path)?));
	i1.family_name = Some("InstanceFamilyName".into());
	i1.style_name = Some("InstanceStyleName".into());
	i1.name = Some("instance.ufo1".into());
	i1.design_location = anisotropic_location([
		("axisNameOne", LocationValue::from((-1000, 500))),
		("axisNameTwo", LocationValue::from(100)),
	]);
	i1.post_script_font_name = Some("InstancePostscriptName".into());
	i1.style_map_family_name = Some("InstanceStyleMapFamilyName".into());
	i1.style_map_style_name = Some("InstanceStyleMapStyleName".into());
	doc.add_instance(i1);

	doc.write(&test_doc_path)?;
	let mut verify = DesignSpaceDocument::new();
	verify.read(&test_doc_path)?;
	verify.write(&test_doc_path2)?;
	Ok(())
}

fn path_source(filename: Option<&str>, path: Option<&str>) -> SourceDescriptor {
	let mut s = SourceDescriptor::new();
	s.set_filename(filename);
	s.set_path(path);
	s.copy_info = true;
	s.design_location = anisotropic_location([("weight", 0)]);
	s.family_name = Some("MasterFamilyName".into());
	s.style_name = Some("MasterStyleNameOne".into());
	s
}

#[test]
fn test_path_name_resolve() -> Result<()> {
	let tmp = tmpdir("path_name_resolve")?;
	// test how descriptor.path and descriptor.filename are resolved
	let test_doc_path1 = s(&tmp.join("testPathName_case1.designspace"));
	let test_doc_path2 = s(&tmp.join("testPathName_case2.designspace"));
	let test_doc_path3 = s(&tmp.join("testPathName_case3.designspace"));
	let test_doc_path4 = s(&tmp.join("testPathName_case4.designspace"));
	let test_doc_path5 = s(&tmp.join("testPathName_case5.designspace"));
	let master_path1 = s(&tmp.join("masters").join("masterTest1.ufo"));

	let a1 = axis("axisName_a", "TAGA", 0, 0, 1000);

	// Case 1: filename and path are both empty. Nothing to calculate, nothing to put in the file.
	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(a1.clone());
	doc.add_source(path_source(None, None));
	doc.write(&test_doc_path1)?;
	let verify = DesignSpaceDocument::from_file(&test_doc_path1)?;
	assert_eq!(verify.sources[0].filename, None);
	assert_eq!(verify.sources[0].path, None);

	// Case 2: filename is empty, path points somewhere: calculate a new filename.
	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(a1.clone());
	doc.add_source(path_source(None, Some(&master_path1)));
	doc.write(&test_doc_path2)?;
	let verify = DesignSpaceDocument::from_file(&test_doc_path2)?;
	assert_eq!(
		verify.sources[0].filename.as_deref(),
		Some("masters/masterTest1.ufo")
	);
	assert_eq!(verify.sources[0].path, Some(posix(&master_path1)));

	// Case 3: the filename is set, the path is None.
	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(a1.clone());
	doc.add_source(path_source(Some("../somewhere/over/the/rainbow.ufo"), None));
	doc.write(&test_doc_path3)?;
	let verify = DesignSpaceDocument::from_file(&test_doc_path3)?;
	let filename = verify.sources[0]
		.filename
		.clone()
		.ok_or_else(|| fail("no filename"))?;
	assert_eq!(filename, "../somewhere/over/the/rainbow.ufo");
	// make the absolute path for filename so we can see if it matches the path
	let p = abspath_str(&join(
		&fontoxide_designspacelib::paths::dirname(&test_doc_path3),
		&filename,
	))?;
	assert_eq!(verify.sources[0].path, Some(posix(&p)));

	// Case 4: the filename points to one file, the path points to another. The path takes precedence.
	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(a1.clone());
	doc.add_source(path_source(
		Some("../somewhere/over/the/rainbow.ufo"),
		Some(&master_path1),
	));
	doc.write(&test_doc_path4)?;
	let verify = DesignSpaceDocument::from_file(&test_doc_path4)?;
	assert_eq!(
		verify.sources[0].filename.as_deref(),
		Some("masters/masterTest1.ufo")
	);

	// Case 5: the filename is None, path has a value, update the filename
	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(a1.clone());
	doc.add_source(path_source(None, Some(&master_path1)));
	doc.write(&test_doc_path5)?; // so that the document has a path
	doc.update_filename_from_path(true, true, false)?;
	assert_eq!(
		doc.sources[0].filename.as_deref(),
		Some("masters/masterTest1.ufo")
	);

	// Case 6: the filename has a value, path has a value, update the filenames with force
	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(a1);
	let source = path_source(
		Some("../somewhere/over/the/rainbow.ufo"),
		Some(&master_path1),
	);
	doc.write(&test_doc_path5)?; // so that the document has a path
	doc.add_source(source);
	assert_eq!(
		doc.sources[0].filename.as_deref(),
		Some("../somewhere/over/the/rainbow.ufo")
	);
	doc.update_filename_from_path(true, true, true)?;
	assert_eq!(
		doc.sources[0].filename.as_deref(),
		Some("masters/masterTest1.ufo")
	);
	Ok(())
}

fn normalized(
	doc: &DesignSpaceDocument,
	name: &str,
	value: impl Into<LocationValue>,
) -> Result<String> {
	let loc = doc.normalize_location(&anisotropic_location([(name, value.into())]))?;
	Ok(loc
		.iter()
		.map(|(k, v)| format!("{k}={v}"))
		.collect::<Vec<_>>()
		.join(","))
}

fn axis_ranges(doc: &DesignSpaceDocument) -> Vec<(String, Number, Number, Number)> {
	let mut r: Vec<_> = doc
		.axes
		.iter()
		.filter_map(Axis::as_continuous)
		.map(|a| {
			(
				a.name.clone().unwrap_or_default(),
				a.minimum,
				a.default,
				a.maximum,
			)
		})
		.collect();
	r.sort_by(|a, b| a.0.cmp(&b.0));
	r
}

#[test]
fn test_normalise1() -> Result<()> {
	// normalisation of anisotropic locations, clipping
	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(axis("axisName_a", "TAGA", -1000, 0, 1000));
	assert_eq!(normalized(&doc, "axisName_a", 0)?, "axisName_a=0.0");
	assert_eq!(normalized(&doc, "axisName_a", 1000)?, "axisName_a=1.0");
	// clipping beyond max values:
	assert_eq!(normalized(&doc, "axisName_a", 1001)?, "axisName_a=1.0");
	assert_eq!(normalized(&doc, "axisName_a", 500)?, "axisName_a=0.5");
	assert_eq!(normalized(&doc, "axisName_a", -1000)?, "axisName_a=-1.0");
	assert_eq!(normalized(&doc, "axisName_a", -1001)?, "axisName_a=-1.0");
	// anisotropic coordinates normalise to isotropic
	assert_eq!(
		normalized(&doc, "axisName_a", (1000, -1000))?,
		"axisName_a=1.0"
	);
	doc.normalize()?;
	assert_eq!(
		axis_ranges(&doc),
		vec![("axisName_a".into(), f(-1.0), f(0.0), f(1.0))]
	);
	Ok(())
}

#[test]
fn test_normalize_location_decreasing_axis_map() -> Result<()> {
	let mut doc = DesignSpaceDocument::new();
	let mut a = axis("Optical size", "opsz", 9, 144, 144);
	a.map = map(&[(9.0, 38.0), (42.0, 33.0), (72.0, 28.0), (144.0, 23.0)]);
	doc.add_axis(a);
	// normalized in user axis direction: -1 is the user minimum (design value 38)
	for (design, expected) in [(38, -1.0), (33, -2.0 / 3.0), (28, -1.0 / 3.0), (23, 0.0)] {
		let location = doc.normalize_location(&anisotropic_location([("Optical size", design)]))?;
		assert!((location["Optical size"].to_f64() - expected).abs() < 1e-6);
	}
	Ok(())
}

#[test]
fn test_normalise2() -> Result<()> {
	// normalisation with minimum > 0
	let mut doc = DesignSpaceDocument::new();
	let a2 = AxisDescriptor {
		name: Some("axisName_b".into()),
		minimum: n(100),
		maximum: n(1000),
		default: n(100),
		..Default::default()
	};
	doc.add_axis(a2);
	assert_eq!(normalized(&doc, "axisName_b", 0)?, "axisName_b=0.0");
	assert_eq!(normalized(&doc, "axisName_b", 1000)?, "axisName_b=1.0");
	// clipping beyond max values:
	assert_eq!(normalized(&doc, "axisName_b", 1001)?, "axisName_b=1.0");
	assert_eq!(
		normalized(&doc, "axisName_b", 500)?,
		"axisName_b=0.4444444444444444"
	);
	assert_eq!(normalized(&doc, "axisName_b", -1000)?, "axisName_b=0.0");
	assert_eq!(normalized(&doc, "axisName_b", -1001)?, "axisName_b=0.0");
	// anisotropic coordinates normalise to isotropic
	assert_eq!(
		normalized(&doc, "axisName_b", (1000, -1000))?,
		"axisName_b=1.0"
	);
	assert_eq!(normalized(&doc, "axisName_b", 1001)?, "axisName_b=1.0");
	doc.normalize()?;
	assert_eq!(
		axis_ranges(&doc),
		vec![("axisName_b".into(), f(0.0), f(0.0), f(1.0))]
	);
	Ok(())
}

#[test]
fn test_normalise3() -> Result<()> {
	// normalisation of negative values, with default == maximum
	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(AxisDescriptor {
		name: Some("ccc".into()),
		minimum: n(-1000),
		maximum: n(0),
		default: n(0),
		..Default::default()
	});
	assert_eq!(normalized(&doc, "ccc", 0)?, "ccc=0.0");
	assert_eq!(normalized(&doc, "ccc", 1)?, "ccc=0.0");
	assert_eq!(normalized(&doc, "ccc", -1000)?, "ccc=-1.0");
	assert_eq!(normalized(&doc, "ccc", -1001)?, "ccc=-1.0");
	doc.normalize()?;
	assert_eq!(
		axis_ranges(&doc),
		vec![("ccc".into(), f(-1.0), f(0.0), f(0.0))]
	);
	Ok(())
}

fn ddd_doc() -> DesignSpaceDocument {
	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(AxisDescriptor {
		name: Some("ddd".into()),
		minimum: n(0),
		maximum: n(1000),
		default: n(0),
		map: map(&[
			(0.0, 100.0),
			(300.0, 500.0),
			(600.0, 500.0),
			(1000.0, 900.0),
		]),
		..Default::default()
	});
	doc
}

#[test]
fn test_normalise4() -> Result<()> {
	// normalisation with a map
	let mut doc = ddd_doc();
	doc.normalize()?;
	assert_eq!(
		doc.axes[0].map(),
		&[
			(n(0), f(0.0)),
			(n(300), f(0.5)),
			(n(600), f(0.5)),
			(n(1000), f(1.0))
		]
	);
	Ok(())
}

#[test]
#[allow(clippy::type_complexity)]
fn test_normalize_mapped_axis_bounds() -> Result<()> {
	// ids=["offcentre-default", "shifted-bounds", "default-at-maximum"]
	let cases: [(i64, i64, i64, &[(f64, f64)], (f64, f64, f64), &[(f64, f64)]); 3] = [
		(
			0,
			400,
			1000,
			&[(0.0, 0.0), (400.0, 100.0), (1000.0, 1000.0)],
			(-1.0, 0.0, 1.0),
			&[(0.0, -1.0), (400.0, 0.0), (1000.0, 1.0)],
		),
		(
			100,
			400,
			900,
			&[(100.0, 0.0), (400.0, 100.0), (900.0, 1000.0)],
			(-1.0, 0.0, 1.0),
			&[(100.0, -1.0), (400.0, 0.0), (900.0, 1.0)],
		),
		(
			0,
			1000,
			1000,
			&[(0.0, 100.0), (1000.0, 900.0)],
			(-1.0, 0.0, 0.0),
			&[(0.0, -1.0), (1000.0, 0.0)],
		),
	];
	for (minimum, default, maximum, mapping, normalized_range, normalized_map) in cases {
		let mut doc = DesignSpaceDocument::new();
		let mut a = axis("Weight", "wght", minimum, default, maximum);
		a.map = map(mapping);
		doc.add_axis(a);
		doc.normalize()?;
		let a = doc.axes[0].as_continuous().ok_or_else(|| fail("axis"))?;
		assert_eq!(
			(a.minimum, a.default, a.maximum),
			(
				f(normalized_range.0),
				f(normalized_range.1),
				f(normalized_range.2)
			)
		);
		assert_eq!(a.map, map(normalized_map));
	}
	Ok(())
}

#[test]
fn test_axis_mapping() -> Result<()> {
	// note: because designspance lib does not do any actual processing of the
	// mapping data, we can only check if there data is there.
	let mut doc = ddd_doc();
	doc.normalize()?;
	assert_eq!(
		doc.axes[0].map(),
		&[
			(n(0), f(0.0)),
			(n(300), f(0.5)),
			(n(600), f(0.5)),
			(n(1000), f(1.0))
		]
	);
	Ok(())
}

#[test]
fn test_axis_mappings_roundtrip() -> Result<()> {
	// tests of axisMappings in a document, roundtripping.
	let tmp = tmpdir("axis_mappings_roundtrip")?;
	let test_doc_path = copy_data("test_avar2.designspace", &tmp)?;
	let test_doc_path2 = tmp.join("test_avar2_roundtrip.designspace");
	let mut doc = DesignSpaceDocument::from_file(&test_doc_path)?;
	assert_eq!(doc.axis_mappings.len(), 2);
	assert_eq!(
		doc.axis_mappings[0].input_location,
		simple_location([("Justify", -100.0), ("Width", 100.0)])
	);

	// This is a bit of a hack, but it's the only way to make sure that the save
	// works on Windows if the tempdir and the data dir are on different drives.
	for source in &mut doc.sources {
		source.path = None;
	}
	for instance in &mut doc.instances {
		instance.path = None;
	}

	doc.write(&test_doc_path2)?;
	// verify these results
	let doc2 = DesignSpaceDocument::from_file(&test_doc_path2)?;
	let fields =
		|d: &DesignSpaceDocument| -> Vec<AxisMappingDescriptor> { d.axis_mappings.clone() };
	assert_eq!(fields(&doc), fields(&doc2));
	Ok(())
}

#[test]
fn test_rules_conditions() -> Result<()> {
	// tests of rules, conditionsets and conditions
	let r1 = RuleDescriptor {
		name: Some("named.rule.1".into()),
		condition_sets: vec![vec![
			cond("axisName_a", Some(0.0), Some(1000.0)),
			cond("axisName_b", Some(0.0), Some(3000.0)),
		]],
		subs: vec![("a".into(), "a.alt".into())],
	};
	let loc = |a: f64, b: f64| simple_location([("axisName_a", num(a)), ("axisName_b", num(b))]);
	assert!(evaluate_rule(&r1, &loc(500.0, 0.0))?);
	assert!(evaluate_rule(&r1, &loc(0.0, 0.0))?);
	assert!(evaluate_rule(&r1, &loc(1000.0, 0.0))?);
	assert!(!evaluate_rule(&r1, &loc(1000.0, -100.0))?);
	assert!(!evaluate_rule(&r1, &loc(1000.0001, 0.0))?);
	assert!(!evaluate_rule(&r1, &loc(-0.0001, 0.0))?);
	assert!(!evaluate_rule(&r1, &loc(-100.0, 0.0))?);
	let glyphs = |g: &[&str]| -> Vec<String> { g.iter().map(|s| (*s).to_owned()).collect() };
	assert_eq!(
		process_rules(
			std::slice::from_ref(&r1),
			&loc(500.0, 0.0),
			&glyphs(&["a", "b", "c"])
		)?,
		glyphs(&["a.alt", "b", "c"])
	);
	assert_eq!(
		process_rules(
			std::slice::from_ref(&r1),
			&loc(500.0, 0.0),
			&glyphs(&["a.alt", "b", "c"])
		)?,
		glyphs(&["a.alt", "b", "c"])
	);
	assert_eq!(
		process_rules(
			std::slice::from_ref(&r1),
			&loc(2000.0, 0.0),
			&glyphs(&["a", "b", "c"])
		)?,
		glyphs(&["a", "b", "c"])
	);

	// rule with only a maximum
	let r2 = RuleDescriptor {
		name: Some("named.rule.2".into()),
		condition_sets: vec![vec![cond("axisName_a", None, Some(500.0))]],
		subs: vec![("b".into(), "b.alt".into())],
	};
	let loc_a = |a: i64| simple_location([("axisName_a", a)]);
	assert!(evaluate_rule(&r2, &loc_a(0))?);
	assert!(evaluate_rule(&r2, &loc_a(-500))?);
	assert!(!evaluate_rule(&r2, &loc_a(1000))?);

	// rule with only a minimum
	let r3 = RuleDescriptor {
		name: Some("named.rule.3".into()),
		condition_sets: vec![vec![cond("axisName_a", Some(500.0), None)]],
		subs: vec![("c".into(), "c.alt".into())],
	};
	assert!(!evaluate_rule(&r3, &loc_a(0))?);
	assert!(evaluate_rule(&r3, &loc_a(1000))?);
	assert!(evaluate_rule(&r3, &loc_a(1000))?);

	// rule with only a minimum, maximum in separate conditions
	let r4 = RuleDescriptor {
		name: Some("named.rule.4".into()),
		condition_sets: vec![vec![
			cond("axisName_a", Some(500.0), None),
			cond("axisName_b", None, Some(500.0)),
		]],
		subs: vec![("c".into(), "c.alt".into())],
	};
	assert!(evaluate_rule(&r4, &loc(1000.0, 0.0))?);
	assert!(!evaluate_rule(&r4, &loc(0.0, 0.0))?);
	assert!(!evaluate_rule(&r4, &loc(1000.0, 1000.0))?);
	Ok(())
}

#[test]
#[allow(clippy::type_complexity)]
fn test_normalize_rule_conditions() -> Result<()> {
	// ids=["unmapped", "mapped"]
	let cases: [(i64, &[(f64, f64)], f64, f64, f64, f64); 2] = [
		(0, &[], 400.0, 600.0, 0.4, 0.6),
		(
			400,
			&[(0.0, 0.0), (400.0, 100.0), (1000.0, 1000.0)],
			50.0,
			550.0,
			-0.5,
			0.5,
		),
	];
	for (default, mapping, minimum, maximum, normalized_minimum, normalized_maximum) in cases {
		let mut doc = DesignSpaceDocument::new();
		let mut a = axis("Weight", "wght", 0, default, 1000);
		a.map = map(mapping);
		doc.add_axis(a);
		doc.add_rule(RuleDescriptor {
			name: Some("r".into()),
			condition_sets: vec![
				vec![cond("Weight", Some(minimum), Some(maximum))],
				vec![
					cond("Weight", None, Some(maximum)),
					cond("Weight", Some(minimum), None),
				],
			],
			subs: vec![("a".into(), "a.alt".into())],
		});
		doc.normalize()?;
		assert_eq!(
			doc.rules[0].condition_sets,
			vec![
				vec![cond(
					"Weight",
					Some(normalized_minimum),
					Some(normalized_maximum)
				)],
				vec![
					cond("Weight", None, Some(normalized_maximum)),
					cond("Weight", Some(normalized_minimum), None),
				],
			]
		);
	}
	Ok(())
}

/// `_addUnwrappedCondition`: only for testing, so we can make an invalid
/// designspace file. Older designspace files may have conditions that are
/// not wrapped in a conditionset; these can be read into a new conditionset.
fn add_unwrapped_condition(path: &std::path::Path) -> Result<()> {
	let d = read_text(path)?;
	let d = d.replace(
		"<rule name=\"named.rule.1\">",
		"<rule name=\"named.rule.1\">\n\t<condition maximum=\"22\" minimum=\"33\" name=\"axisName_a\" />",
	);
	std::fs::write(path, d)?;
	Ok(())
}

#[test]
fn test_rules_document() -> Result<()> {
	// tests of rules in a document, roundtripping.
	let tmp = tmpdir("rules_document")?;
	let test_doc_path = tmp.join("testRules.designspace");
	let test_doc_path2 = tmp.join("testRules_roundtrip.designspace");
	let mut doc = DesignSpaceDocument::new();
	doc.rules_processing_last = true;
	doc.add_axis(axis("axisName_a", "TAGA", 0, 0, 1000));
	doc.add_axis(axis("axisName_b", "TAGB", 2000, 2000, 3000));
	// rule with minium and maximum
	doc.add_rule(RuleDescriptor {
		name: Some("named.rule.1".into()),
		condition_sets: vec![vec![
			cond("axisName_a", Some(0.0), Some(1000.0)),
			cond("axisName_b", Some(0.0), Some(3000.0)),
		]],
		subs: vec![("a".into(), "a.alt".into())],
	});
	assert_eq!(doc.rules.len(), 1);
	assert_eq!(doc.rules[0].condition_sets.len(), 1);
	assert_eq!(doc.rules[0].condition_sets[0].len(), 2);
	assert_eq!(
		doc.axes,
		vec![
			Axis::Continuous(axis("axisName_a", "TAGA", 0, 0, 1000)),
			Axis::Continuous(axis("axisName_b", "TAGB", 2000, 2000, 3000)),
		]
	);
	assert_eq!(
		doc.rules[0].subs,
		vec![("a".to_owned(), "a.alt".to_owned())]
	);
	doc.normalize()?;
	assert_eq!(doc.rules[0].name.as_deref(), Some("named.rule.1"));
	assert_eq!(
		doc.rules[0].condition_sets,
		vec![vec![
			cond("axisName_a", Some(0.0), Some(1.0)),
			cond("axisName_b", Some(0.0), Some(1.0)),
		]]
	);
	// still one conditionset
	assert_eq!(doc.rules[0].condition_sets.len(), 1);
	doc.write(&test_doc_path)?;
	// add a stray conditionset
	add_unwrapped_condition(&test_doc_path)?;
	let mut doc2 = DesignSpaceDocument::from_file(&test_doc_path)?;
	assert!(doc2.rules_processing_last);
	assert_eq!(doc2.axes.len(), 2);
	assert_eq!(doc2.rules.len(), 1);
	assert_eq!(doc2.rules[0].condition_sets.len(), 2);
	doc2.write(&test_doc_path2)?;
	// verify these results
	// make sure the stray condition is now neatly wrapped in a conditionset.
	let doc3 = DesignSpaceDocument::from_file(&test_doc_path2)?;
	assert_eq!(doc3.rules.len(), 1);
	assert_eq!(doc3.rules[0].condition_sets.len(), 2);
	Ok(())
}

#[test]
fn test_document_lib() -> Result<()> {
	// roundtrip test of the document lib with some nested data
	let tmp = tmpdir("document_lib")?;
	let test_doc_path1 = tmp.join("testDocumentLibTest.designspace");
	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(axis("axisName_a", "TAGA", 0, 0, 1000));
	let mut inner = plistlib::Dict::new();
	inner.insert("a".into(), 123.into());
	let mut dummy_data = plistlib::Dict::new();
	dummy_data.insert("a".into(), 123.into());
	dummy_data.insert("b".into(), "äbc".into());
	dummy_data.insert("c".into(), Value::Array(vec![1.into(), 2.into(), 3.into()]));
	dummy_data.insert("d".into(), Value::Dict(inner));
	let dummy_key = "org.fontTools.designspaceLib";
	doc.lib
		.insert(dummy_key.into(), Value::Dict(dummy_data.clone()));
	doc.write(&test_doc_path1)?;
	let new = DesignSpaceDocument::from_file(&test_doc_path1)?;
	assert_eq!(new.lib.get(dummy_key), Some(&Value::Dict(dummy_data)));
	Ok(())
}

#[test]
fn test_empty_lib() -> Result<()> {
	// an empty <lib> element should read as an empty lib, not raise IndexError
	let tmp = tmpdir("empty_lib")?;
	let test_doc_path = tmp.join("testEmptyLibTest.designspace");
	std::fs::write(
		&test_doc_path,
		concat!(
			"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
			"<designspace format=\"4.1\">\n",
			"  <axes>\n",
			"    <axis tag=\"TAGA\" name=\"axisName_a\" minimum=\"0\" maximum=\"1000\" default=\"0\"/>\n",
			"  </axes>\n",
			"  <instances>\n",
			"    <instance name=\"instance.a\" filename=\"instance.a.ufo\">\n",
			"      <lib/>\n",
			"    </instance>\n",
			"  </instances>\n",
			"  <lib/>\n",
			"</designspace>\n",
		),
	)?;
	let doc = DesignSpaceDocument::from_file(&test_doc_path)?;
	assert!(doc.lib.is_empty());
	assert!(doc.instances[0].lib.is_empty());
	Ok(())
}

#[test]
fn test_update_paths() -> Result<()> {
	let tmp = tmpdir("update_paths")?;
	let mut doc = DesignSpaceDocument::new();
	doc.path = Some(s(&tmp
		.join("foo")
		.join("bar")
		.join("MyDesignspace.designspace")));
	doc.add_source(SourceDescriptor::new());

	doc.update_paths()?;

	// expect no changes
	assert_eq!(doc.sources[0].path, None);
	assert_eq!(doc.sources[0].filename, None);

	let name1 = "../masters/Source1.ufo";
	let path1 = posix(&s(&tmp.join("foo").join("masters").join("Source1.ufo")));

	doc.sources[0].set_path(Some(&path1));
	doc.sources[0].filename = None;

	doc.update_paths()?;

	assert_eq!(doc.sources[0].path.as_deref(), Some(path1.as_str()));
	assert_eq!(doc.sources[0].filename.as_deref(), Some(name1)); // empty filename updated

	let name2 = "../masters/Source2.ufo";
	doc.sources[0].set_filename(Some(name2));

	doc.update_paths()?;

	// conflicting filename discarded, path always gets precedence
	assert_eq!(doc.sources[0].path.as_deref(), Some(path1.as_str()));
	assert_eq!(
		doc.sources[0].filename.as_deref(),
		Some("../masters/Source1.ufo")
	);

	doc.sources[0].path = None;
	doc.sources[0].set_filename(Some(name2));

	doc.update_paths()?;

	// expect no changes
	assert_eq!(doc.sources[0].path, None);
	assert_eq!(doc.sources[0].filename.as_deref(), Some(name2));
	Ok(())
}

#[test]
fn test_read_with_path_object() -> Result<()> {
	let source = datadir().join("test_v4_original.designspace");
	assert!(source.exists());
	let mut doc = DesignSpaceDocument::new();
	doc.read(&source)?;
	Ok(())
}

#[test]
fn test_with_with_path_object() -> Result<()> {
	let tmp = tmpdir("with_with_path_object")?;
	let dest = tmp.join("test_v4_original.designspace");
	let mut doc = DesignSpaceDocument::new();
	doc.write(&dest)?;
	assert!(dest.exists());
	Ok(())
}

#[test]
fn test_find_default_axis_mapping() -> Result<()> {
	let designspace_string = r#"<?xml version='1.0' encoding='UTF-8'?>
<designspace format="4.0">
  <axes>
    <axis tag="wght" name="Weight" minimum="100" maximum="800" default="400">
      <map input="100" output="20"/>
      <map input="300" output="40"/>
      <map input="400" output="80"/>
      <map input="700" output="126"/>
      <map input="800" output="170"/>
    </axis>
    <axis tag="ital" name="Italic" minimum="0" maximum="1" default="1"/>
  </axes>
  <sources>
    <source filename="Font-Light.ufo">
      <location>
        <dimension name="Weight" xvalue="20"/>
        <dimension name="Italic" xvalue="0"/>
      </location>
    </source>
    <source filename="Font-Regular.ufo">
      <location>
        <dimension name="Weight" xvalue="80"/>
        <dimension name="Italic" xvalue="0"/>
      </location>
    </source>
    <source filename="Font-Bold.ufo">
      <location>
        <dimension name="Weight" xvalue="170"/>
        <dimension name="Italic" xvalue="0"/>
      </location>
    </source>
    <source filename="Font-LightItalic.ufo">
      <location>
        <dimension name="Weight" xvalue="20"/>
        <dimension name="Italic" xvalue="1"/>
      </location>
    </source>
    <source filename="Font-Italic.ufo">
      <location>
        <dimension name="Weight" xvalue="80"/>
        <dimension name="Italic" xvalue="1"/>
      </location>
    </source>
    <source filename="Font-BoldItalic.ufo">
      <location>
        <dimension name="Weight" xvalue="170"/>
        <dimension name="Italic" xvalue="1"/>
      </location>
    </source>
  </sources>
</designspace>
    "#;
	let mut designspace = DesignSpaceDocument::from_string(designspace_string)?;
	let default = designspace
		.find_default()?
		.ok_or_else(|| fail("no default"))?;
	assert_eq!(default.filename.as_deref(), Some("Font-Italic.ufo"));

	*designspace.axes[1].default_mut() = n(0);

	let default = designspace
		.find_default()?
		.ok_or_else(|| fail("no default"))?;
	assert_eq!(default.filename.as_deref(), Some("Font-Regular.ufo"));
	Ok(())
}

#[test]
fn test_add_axis_descriptor() -> Result<()> {
	let mut ds = DesignSpaceDocument::new();
	let axis = ds.add_axis(axis("Weight", "wght", 100, 400, 900)).clone();
	assert_eq!(ds.axes[0], axis);
	let a = axis.as_continuous().ok_or_else(|| fail("not continuous"))?;
	assert_eq!(a.name.as_deref(), Some("Weight"));
	assert_eq!(a.tag.as_deref(), Some("wght"));
	assert_eq!(a.minimum, n(100));
	assert_eq!(a.default, n(400));
	assert_eq!(a.maximum, n(900));

	// The second `test_addAxisDescriptor` of the module, which replaces the
	// first one in pytest.
	let mut ds = DesignSpaceDocument::new();
	let mapping = ds
		.add_axis_mapping(AxisMappingDescriptor {
			input_location: simple_location([("weight", 900), ("width", 150)]),
			output_location: simple_location([("weight", 870)]),
			..Default::default()
		})
		.clone();
	assert_eq!(ds.axis_mappings[0], mapping);
	assert_eq!(
		mapping.input_location,
		simple_location([("weight", 900), ("width", 150)])
	);
	assert_eq!(mapping.output_location, simple_location([("weight", 870)]));
	Ok(())
}

#[test]
fn test_add_source_descriptor() {
	let mut ds = DesignSpaceDocument::new();
	let source = ds
		.add_source(SourceDescriptor {
			name: Some("TestSource".into()),
			design_location: anisotropic_location([("Weight", 400)]),
			..Default::default()
		})
		.clone();
	assert_eq!(ds.sources[0], source);
	assert_eq!(source.name.as_deref(), Some("TestSource"));
	assert_eq!(
		source.design_location,
		anisotropic_location([("Weight", 400)])
	);
}

#[test]
fn test_add_instance_descriptor() {
	let mut ds = DesignSpaceDocument::new();
	let instance = ds
		.add_instance(InstanceDescriptor {
			name: Some("TestInstance".into()),
			design_location: anisotropic_location([("Weight", 400)]),
			style_name: Some("Regular".into()),
			style_map_style_name: Some("regular".into()),
			..Default::default()
		})
		.clone();
	assert_eq!(ds.instances[0], instance);
	assert_eq!(instance.name.as_deref(), Some("TestInstance"));
	assert_eq!(
		instance.design_location,
		anisotropic_location([("Weight", 400)])
	);
	assert_eq!(instance.style_name.as_deref(), Some("Regular"));
	assert_eq!(instance.style_map_style_name.as_deref(), Some("regular"));
}

#[test]
fn test_add_rule_descriptor() -> Result<()> {
	let tmp = tmpdir("add_rule_descriptor")?;
	let mut ds = DesignSpaceDocument::new();
	let condition_sets = vec![vec![
		cond("Weight", Some(100.0), Some(200.0)),
		cond("Weight", Some(700.0), Some(900.0)),
	]];
	let rule = ds
		.add_rule(RuleDescriptor {
			name: Some("TestRule".into()),
			condition_sets: condition_sets.clone(),
			subs: vec![("a".into(), "a.alt".into())],
		})
		.clone();
	assert_eq!(ds.rules[0], rule);
	assert_eq!(rule.name.as_deref(), Some("TestRule"));
	assert_eq!(rule.condition_sets, condition_sets);
	assert_eq!(rule.subs, vec![("a".to_owned(), "a.alt".to_owned())]);

	// Test it doesn't crash.
	ds.write(tmp.join("test.designspace"))?;
	Ok(())
}

#[test]
fn test_deepcopy_except_fonts() -> Result<()> {
	let mut ds = DesignSpaceDocument::new();
	ds.add_source(SourceDescriptor::new());
	ds.add_source(SourceDescriptor::new());
	let mut ds_copy = ds.deepcopy_except_fonts();
	assert_eq!(ds.tostring()?, ds_copy.tostring()?);
	Ok(())
}

#[test]
fn test_range_post_init() {
	// test min and max are sorted and default is clamped to either min/max
	let r = Range::new(2, -1, -2);
	assert_eq!(r.minimum, n(-1));
	assert_eq!(r.maximum, n(2));
	assert_eq!(r.default, n(-1));
}

#[test]
fn test_get_axes() -> Result<()> {
	let ds = DesignSpaceDocument::from_file(datadir().join("test_v5.designspace"))?;
	assert!(std::ptr::eq(
		ds.get_axis("Width").ok_or_else(|| fail("Width"))?,
		ds.get_axis_by_tag("wdth").ok_or_else(|| fail("wdth"))?
	));
	assert!(std::ptr::eq(
		ds.get_axis("Italic").ok_or_else(|| fail("Italic"))?,
		ds.get_axis_by_tag("ital").ok_or_else(|| fail("ital"))?
	));
	Ok(())
}

#[test]
fn test_preserve_empty_condition_set() -> Result<()> {
	// https://github.com/fonttools/fonttools/issues/4000
	let mut ds = DesignSpaceDocument::new();
	ds.rules = vec![RuleDescriptor {
		condition_sets: vec![vec![]], // Empty conditionset (implicit always enabled)
		..Default::default()
	}];
	let ds_str = ds.tostring()?;
	assert!(ds_str.contains("<conditionset/>"));
	let roundtripped_ds = DesignSpaceDocument::from_string(&ds_str)?;
	assert_eq!(roundtripped_ds.rules.len(), 1);
	assert_eq!(roundtripped_ds.rules[0].condition_sets.len(), 1);
	Ok(())
}

fn weight_axis(map_pairs: &[(f64, f64)]) -> AxisDescriptor {
	let mut ax = axis("Weight", "wght", 100, 100, 1000);
	ax.map = map(map_pairs);
	ax
}

fn approx(a: Number, b: f64) {
	assert!(
		(a.to_f64() - b).abs() <= 1e-6 * b.abs().max(1e-12),
		"{a} != {b}"
	);
}

#[test]
fn test_map_backward_many_to_one() -> Result<()> {
	// Regression test for https://github.com/googlefonts/ufo2ft/issues/978
	// Many-to-one: both user=900 and user=1000 map to design=1000
	let ax = weight_axis(&[
		(100.0, 100.0),
		(800.0, 780.0),
		(900.0, 1000.0),
		(1000.0, 1000.0),
	]);
	// map_backward(800) should interpolate between (780, 800) and (1000, 900),
	// NOT between (780, 800) and (1000, 1000).
	let result = ax.map_backward(800.into())?;
	approx(
		result,
		800.0 + (800.0 - 780.0) / (1000.0 - 780.0) * (900.0 - 800.0),
	);
	Ok(())
}

#[test]
fn test_axis_map_conflicting_duplicate_input_is_rejected_when_consumed() {
	let raw_map = map(&[
		(900.0, 1000.0),
		(400.0, 450.0),
		(100.0, 100.0),
		(400.0, 500.0),
	]);
	let mut axis = axis("Weight", "wght", 100, 400, 900);
	// Deliberately out of input order: validation must not depend on adjacent
	// entries in the source list.
	axis.map = raw_map.clone();
	let e = axis.map_forward(n(400));
	assert!(e.as_ref().is_err_and(|e| {
		let m = e.to_string();
		m.starts_with("Axis 'Weight': mapping input value 400 ") && m.contains("450 and 500")
	}));
	assert_err_contains(axis.map_backward(450.into()), "Axis 'Weight'");
	assert_eq!(axis.map, raw_map);
}

#[test]
fn test_axis_map_identical_duplicate_input_is_ignored_by_consumers() -> Result<()> {
	let raw_map = map(&[
		(900.0, 1000.0),
		(400.0, 450.0),
		(100.0, 100.0),
		(400.0, 450.0),
	]);
	let mut axis = axis("Weight", "wght", 100, 400, 900);
	axis.map = raw_map.clone();
	assert_eq!(
		axis.get_validated_map()?,
		map(&[(900.0, 1000.0), (400.0, 450.0), (100.0, 100.0)])
	);
	assert_eq!(axis.map_forward(n(400))?, n(450));
	assert_eq!(axis.map_backward(450.into())?, n(400));
	assert_eq!(axis.map, raw_map);
	Ok(())
}

#[test]
fn test_discrete_axis_map_rejects_conflicting_duplicate_inputs_when_consumed() {
	let axis = DiscreteAxisDescriptor {
		name: Some("Style".into()),
		tag: Some("STYL".into()),
		values: vec![n(0), n(1)],
		default: n(0),
		map: map(&[(0.0, 10.0), (1.0, 20.0), (0.0, 11.0)]),
		..Default::default()
	};
	assert_err_contains(axis.map_forward(n(0)), "Axis 'Style'");
	assert_err_contains(axis.map_backward(10.into()), "Axis 'Style'");
}

#[test]
fn test_parsed_axis_map_conflicting_duplicate_input_is_rejected_when_consumed() -> Result<()> {
	let designspace = r#"
    <designspace format="5.0">
      <axes>
        <axis name="Weight" tag="wght" minimum="100" default="400" maximum="900">
          <map input="900" output="1000"/>
          <map input="400" output="450"/>
          <map input="100" output="100"/>
          <map input="400" output="500"/>
        </axis>
      </axes>
    </designspace>
    "#;
	let doc = DesignSpaceDocument::from_string(designspace.trim_start())?;
	let raw_map = map(&[
		(900.0, 1000.0),
		(400.0, 450.0),
		(100.0, 100.0),
		(400.0, 500.0),
	]);
	assert_eq!(doc.axes[0].map(), raw_map.as_slice());
	assert_err_contains(doc.axes[0].map_forward(n(400)), "Axis 'Weight'");
	assert_eq!(doc.axes[0].map(), raw_map.as_slice());
	Ok(())
}

#[test]
fn test_map_backward_many_to_one_mid_range() -> Result<()> {
	// Many-to-one in the middle: both user=400 and user=500 map to design=50
	let ax = weight_axis(&[
		(100.0, 10.0),
		(400.0, 50.0),
		(500.0, 50.0),
		(700.0, 80.0),
		(900.0, 100.0),
	]);
	// Before the flat segment: interpolate between (10, 100) and (50, 400)
	approx(
		ax.map_backward(45.into())?,
		100.0 + (45.0 - 10.0) / (50.0 - 10.0) * (400.0 - 100.0),
	);
	// After the flat segment: interpolate between (50, 500) and (80, 700)
	approx(
		ax.map_backward(65.into())?,
		500.0 + (65.0 - 50.0) / (80.0 - 50.0) * (700.0 - 500.0),
	);
	Ok(())
}

#[test]
fn test_map_backward_many_to_one_at_flat_value() -> Result<()> {
	let ax = weight_axis(&[
		(100.0, 100.0),
		(800.0, 780.0),
		(900.0, 1000.0),
		(1000.0, 1000.0),
	]);
	// map_backward(1000) should return 900 (first user value reaching design=1000)
	assert_eq!(ax.map_backward(1000.into())?, n(900));
	// map_backward(780) should still return 800
	assert_eq!(ax.map_backward(780.into())?, n(800));
	Ok(())
}

#[test]
fn test_map_backward_many_to_one_unsorted_input() -> Result<()> {
	// Same map as the extreme case, but entries shuffled
	let ax = weight_axis(&[
		(1000.0, 1000.0),
		(100.0, 100.0),
		(900.0, 1000.0),
		(800.0, 780.0),
	]);
	approx(
		ax.map_backward(800.into())?,
		800.0 + (800.0 - 780.0) / (1000.0 - 780.0) * (900.0 - 800.0),
	);
	Ok(())
}

#[test]
fn test_map_backward_single_entry() -> Result<()> {
	let ax = weight_axis(&[(100.0, 200.0)]);
	assert_eq!(ax.map_backward(200.into())?, n(100));
	assert_eq!(ax.map_backward(300.into())?, n(200));
	assert_eq!(ax.map_backward(100.into())?, n(0));
	Ok(())
}

#[test]
fn test_no_external_entity_expansion() -> Result<()> {
	let tmp = tmpdir("no_external_entity_expansion")?;
	let secret = tmp.join("secret.txt");
	std::fs::write(&secret, "s3cr3t")?;
	let path = tmp.join("test.designspace");
	let uri = format!(
		"file:///{}",
		s(&secret).replace('\\', "/").trim_start_matches('/')
	);
	std::fs::write(
		&path,
		format!(
			"<?xml version=\"1.0\"?><!DOCTYPE designspace [<!ENTITY xxe SYSTEM \"{uri}\">]><designspace format=\"4.1\"><axes><axis tag=\"wght\" name=\"Weight\" minimum=\"0\" maximum=\"1\" default=\"0\"><labelname xml:lang=\"en\">x&xxe;</labelname></axis></axes></designspace>"
		),
	)?;
	match DesignSpaceDocument::from_file(&path) {
		// the undefined entity is rejected outright
		Err(fontoxide_designspacelib::DesignSpaceDocumentError::Xml(_)) => {}
		Err(e) => return Err(e),
		Ok(doc) => assert!(!doc.axes[0].label_names()["en"].contains("s3cr3t")),
	}
	Ok(())
}
