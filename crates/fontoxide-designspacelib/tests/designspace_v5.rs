//! `Tests/designspaceLib/designspace_v5_test.py`.

mod common;

use common::{copy_data, datadir, expected_text, fail, read_text, remove_between, tmpdir};
use fontoxide_designspacelib::plistlib::Value;
use fontoxide_designspacelib::types::{Range, RegionValue};
use fontoxide_designspacelib::{
	AnisotropicLocation, Axis, AxisDescriptor, AxisLabelDescriptor, AxisSubset,
	DesignSpaceDocument, DiscreteAxisDescriptor, GlyphData, GlyphMaster, InstanceDescriptor,
	LocalizedNames, LocationLabelDescriptor, LocationValue, Number, RangeAxisSubsetDescriptor,
	Result, SourceDescriptor, ValueAxisSubsetDescriptor, VariableFontDescriptor,
	anisotropic_location, get_vf_user_region, simple_location,
};

fn n(v: i64) -> Number {
	Number::Int(v)
}

fn names(items: &[(&str, &str)]) -> LocalizedNames {
	items
		.iter()
		.map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
		.collect()
}

fn map(pairs: &[(i64, i64)]) -> Vec<(Number, Number)> {
	pairs.iter().map(|&(a, b)| (n(a), n(b))).collect()
}

/// `AxisLabelDescriptor(name=..., userValue=...)` with the other fields set
/// by the caller.
fn label(name: &str, user_value: i64) -> AxisLabelDescriptor {
	AxisLabelDescriptor::new(name, user_value)
}

fn ranged(name: &str, minimum: i64, value: i64, maximum: i64) -> AxisLabelDescriptor {
	AxisLabelDescriptor {
		user_minimum: Some(n(minimum)),
		user_maximum: Some(n(maximum)),
		..label(name, value)
	}
}

fn linked(name: &str, value: i64, linked: i64) -> AxisLabelDescriptor {
	AxisLabelDescriptor {
		linked_user_value: Some(n(linked)),
		..label(name, value)
	}
}

fn elidable(l: AxisLabelDescriptor) -> AxisLabelDescriptor {
	AxisLabelDescriptor {
		elidable: true,
		..l
	}
}

fn continuous(tag: &str, name: &str, minimum: i64, default: i64, maximum: i64) -> AxisDescriptor {
	AxisDescriptor {
		tag: Some(tag.into()),
		name: Some(name.into()),
		minimum: n(minimum),
		default: n(default),
		maximum: n(maximum),
		..Default::default()
	}
}

fn discrete(tag: &str, name: &str, values: &[i64], default: i64) -> DiscreteAxisDescriptor {
	DiscreteAxisDescriptor {
		tag: Some(tag.into()),
		name: Some(name.into()),
		values: values.iter().map(|&v| n(v)).collect(),
		default: n(default),
		..Default::default()
	}
}

fn vf(name: &str, axis_subsets: Vec<AxisSubset>) -> VariableFontDescriptor {
	VariableFontDescriptor {
		name: name.into(),
		axis_subsets,
		..Default::default()
	}
}

fn whole(name: &str) -> AxisSubset {
	RangeAxisSubsetDescriptor::new(name).into()
}

fn value(name: &str, v: i64) -> AxisSubset {
	ValueAxisSubsetDescriptor::new(name, v).into()
}

/// `assert_descriptors_equal`: `asdict()` skips the private `_filename` and
/// `_path` of the path properties, so those are not compared.
fn sources_without_paths(sources: &[SourceDescriptor]) -> Vec<SourceDescriptor> {
	sources
		.iter()
		.map(|s| SourceDescriptor {
			filename: None,
			path: None,
			..s.clone()
		})
		.collect()
}

fn instances_without_paths(instances: &[InstanceDescriptor]) -> Vec<InstanceDescriptor> {
	instances
		.iter()
		.map(|s| InstanceDescriptor {
			filename: None,
			path: None,
			..s.clone()
		})
		.collect()
}

fn vfs_without_paths(vfs: &[VariableFontDescriptor]) -> Vec<VariableFontDescriptor> {
	vfs.iter()
		.map(|s| VariableFontDescriptor {
			filename: None,
			..s.clone()
		})
		.collect()
}

fn source(
	name: &str,
	location: &[(&str, f64)],
	family_name: &str,
	style_name: &str,
) -> SourceDescriptor {
	SourceDescriptor {
		name: Some(name.into()),
		design_location: anisotropic_location(location.iter().copied()),
		family_name: Some(family_name.into()),
		style_name: Some(style_name.into()),
		..Default::default()
	}
}

#[test]
fn test_read_v5_document_simple() -> Result<()> {
	let doc = DesignSpaceDocument::from_file(datadir().join("test_v5.designspace"))?;

	let expected_axes: Vec<Axis> = vec![
		AxisDescriptor {
			label_names: names(&[("en", "Wéíght"), ("fa-IR", "قطر")]),
			map: map(&[
				(200, 0),
				(300, 100),
				(400, 368),
				(600, 600),
				(700, 824),
				(900, 1000),
			]),
			axis_ordering: None,
			axis_labels: vec![
				AxisLabelDescriptor {
					label_names: names(&[("de", "Extraleicht"), ("fr", "Extra léger")]),
					..ranged("Extra Light", 200, 200, 250)
				},
				ranged("Light", 250, 300, 350),
				elidable(ranged("Regular", 350, 400, 450)),
				ranged("Semi Bold", 450, 600, 650),
				ranged("Bold", 650, 700, 850),
				ranged("Black", 850, 900, 900),
				elidable(linked("Regular", 400, 700)),
				linked("Bold", 700, 400),
			],
			..continuous("wght", "Weight", 200, 200, 1000)
		}
		.into(),
		AxisDescriptor {
			hidden: true,
			label_names: names(&[("fr", "Chasse")]),
			map: map(&[(50, 10), (100, 20), (125, 66), (150, 990)]),
			axis_ordering: Some(1),
			axis_labels: vec![
				label("Condensed", 50),
				AxisLabelDescriptor {
					elidable: true,
					older_sibling: true,
					..label("Normal", 100)
				},
				label("Wide", 125),
				AxisLabelDescriptor {
					user_minimum: Some(n(150)),
					..label("Extra Wide", 150)
				},
			],
			..continuous("wdth", "Width", 50, 100, 150)
		}
		.into(),
		DiscreteAxisDescriptor {
			axis_ordering: None,
			axis_labels: vec![elidable(linked("Roman", 0, 1)), label("Italic", 1)],
			..discrete("ital", "Italic", &[0, 1], 0)
		}
		.into(),
	];
	assert_eq!(doc.axes, expected_axes);

	assert_eq!(
		doc.location_labels,
		vec![
			LocationLabelDescriptor {
				label_names: names(&[("fr", "Un Style")]),
				..LocationLabelDescriptor::new(
					"Some Style",
					simple_location([("Weight", 300), ("Width", 50), ("Italic", 0)])
				)
			},
			LocationLabelDescriptor::new(
				"Other",
				simple_location([("Weight", 700), ("Width", 100), ("Italic", 1)])
			),
		]
	);

	let expected_sources = vec![
		SourceDescriptor {
			copy_lib: true,
			copy_info: true,
			copy_features: true,
			muted_glyph_names: vec!["A".into(), "Z".into()],
			localised_family_name: names(&[("fr", "Montserrat"), ("ja", "モンセラート")]),
			..source(
				"master.ufo1",
				&[("Italic", 0.0), ("Weight", 0.0), ("Width", 20.0)],
				"MasterFamilyName",
				"MasterStyleNameOne",
			)
		},
		SourceDescriptor {
			mute_kerning: true,
			..source(
				"master.ufo2",
				&[("Italic", 0.0), ("Weight", 1000.0), ("Width", 20.0)],
				"MasterFamilyName",
				"MasterStyleNameTwo",
			)
		},
		SourceDescriptor {
			layer_name: Some("supports".into()),
			..source(
				"master.ufo2",
				&[("Italic", 0.0), ("Weight", 1000.0), ("Width", 20.0)],
				"MasterFamilyName",
				"Supports",
			)
		},
		source(
			"master.ufo3",
			&[("Italic", 1.0), ("Weight", 0.0), ("Width", 100.0)],
			"MasterFamilyName",
			"FauxItalic",
		),
	];
	assert_eq!(sources_without_paths(&doc.sources), expected_sources);

	let lib = |path: &str| {
		let mut lib = fontoxide_designspacelib::plistlib::Dict::new();
		lib.insert("com.vtt.source".into(), Value::String(path.into()));
		lib
	};
	let expected_vfs = vec![
		VariableFontDescriptor {
			filename: Some("Test_WghtWdth_different_from_name.ttf".into()),
			lib: lib("sources/vtt/Test_WghtWdth.vtt"),
			..vf("Test_WghtWdth", vec![whole("Weight"), whole("Width")])
		},
		VariableFontDescriptor {
			lib: lib("sources/vtt/Test_Wght.vtt"),
			..vf("Test_Wght", vec![whole("Weight")])
		},
		vf("TestCd_Wght", vec![whole("Weight"), value("Width", 0)]),
		vf("TestWd_Wght", vec![whole("Weight"), value("Width", 1000)]),
		vf("TestItalic_Wght", vec![whole("Weight"), value("Italic", 1)]),
		vf(
			"TestRB_Wght",
			vec![
				RangeAxisSubsetDescriptor {
					user_minimum: n(400),
					user_default: Some(n(400)),
					user_maximum: n(700),
					..RangeAxisSubsetDescriptor::new("Weight")
				}
				.into(),
				value("Italic", 0),
			],
		),
	];
	assert_eq!(
		vfs_without_paths(&doc.variable_fonts),
		vfs_without_paths(&expected_vfs)
	);

	let mut instance1_lib = fontoxide_designspacelib::plistlib::Dict::new();
	instance1_lib.insert(
		"com.coolDesignspaceApp.binaryData".into(),
		Value::Data(b"<binary gunk>".to_vec()),
	);
	instance1_lib.insert(
		"com.coolDesignspaceApp.specimenText".into(),
		"Hamburgerwhatever".into(),
	);
	let full_names = |i: InstanceDescriptor| InstanceDescriptor {
		family_name: Some("InstanceFamilyName".into()),
		style_name: Some("InstanceStyleName".into()),
		post_script_font_name: Some("InstancePostscriptName".into()),
		style_map_family_name: Some("InstanceStyleMapFamilyName".into()),
		style_map_style_name: Some("InstanceStyleMapStyleName".into()),
		..i
	};
	let expected_instances = vec![
		full_names(InstanceDescriptor {
			name: Some("instance.ufo1".into()),
			design_location: anisotropic_location([("Weight", 500.0), ("Width", 20.0)]),
			localised_family_name: names(&[("fr", "Montserrat"), ("ja", "モンセラート")]),
			localised_style_name: names(&[("fr", "Demigras"), ("ja", "半ば")]),
			localised_style_map_family_name: names(&[
				("de", "Montserrat Halbfett"),
				("ja", "モンセラート SemiBold"),
			]),
			localised_style_map_style_name: names(&[("de", "Standard")]),
			glyphs: [(
				"arrow".to_owned(),
				GlyphData {
					mute: true,
					unicodes: Some(vec![291, 292, 293]),
					..Default::default()
				},
			)]
			.into_iter()
			.collect(),
			lib: instance1_lib,
			..Default::default()
		}),
		full_names(InstanceDescriptor {
			name: Some("instance.ufo2".into()),
			design_location: anisotropic_location([
				("Weight", LocationValue::from(500.0)),
				("Width", LocationValue::from((400.0, 300.0))),
			]),
			glyphs: [
				(
					"arrow".to_owned(),
					GlyphData {
						unicodes: Some(vec![101, 201, 301]),
						note: Some("A note about this glyph".into()),
						instance_location: Some(anisotropic_location([
							("Weight", 120.0),
							("Width", 100.0),
						])),
						masters: Some(vec![
							GlyphMaster {
								font: Some("master.ufo1".into()),
								location: Some(anisotropic_location([
									("Weight", 20.0),
									("Width", 20.0),
								])),
								glyph_name: Some("BB".into()),
							},
							GlyphMaster {
								font: Some("master.ufo2".into()),
								location: Some(anisotropic_location([
									("Weight", 900.0),
									("Width", 900.0),
								])),
								glyph_name: Some("CC".into()),
							},
						]),
						..Default::default()
					},
				),
				("arrow2".to_owned(), GlyphData::default()),
			]
			.into_iter()
			.collect(),
			..Default::default()
		}),
		InstanceDescriptor {
			location_label: Some("Some Style".into()),
			..Default::default()
		},
		InstanceDescriptor {
			design_location: anisotropic_location([
				("Weight", LocationValue::from(600.0)),
				("Width", LocationValue::from((401.0, 420.0))),
			]),
			..Default::default()
		},
		InstanceDescriptor {
			design_location: anisotropic_location([("Weight", 10.0), ("Italic", 0.0)]),
			user_location: simple_location([("Width", 100.0)]),
			..Default::default()
		},
		InstanceDescriptor {
			user_location: simple_location([("Weight", 300.0), ("Width", 130.0), ("Italic", 1.0)]),
			..Default::default()
		},
	];
	assert_eq!(instances_without_paths(&doc.instances), expected_instances);
	Ok(())
}

#[test]
fn test_partial_axis_subset_attributes() -> Result<()> {
	let cases = [
		("", Range::new(100, 900, 400)),
		("userminimum=\"700\"", Range::new(700, 900, 700)),
		("usermaximum=\"300\"", Range::new(100, 300, 300)),
		(
			"userminimum=\"200\" usermaximum=\"800\"",
			Range::new(200, 800, 400),
		),
		("userdefault=\"500\"", Range::new(100, 900, 500)),
	];
	for (attributes, expected_region) in cases {
		let doc = DesignSpaceDocument::from_string(&format!(
			r#"
        <designspace format="5.0">
          <axes>
            <axis name="Weight" tag="wght" minimum="100" default="400" maximum="900"/>
          </axes>
          <variable-fonts>
            <variable-font name="WeightSubset">
              <axis-subsets>
                <axis-subset name="Weight" {attributes}/>
              </axis-subsets>
            </variable-font>
          </variable-fonts>
        </designspace>
        "#
		))?;
		let region = get_vf_user_region(&doc, &doc.variable_fonts[0])?;
		assert_eq!(region["Weight"], RegionValue::Range(expected_region));
	}
	Ok(())
}

#[test]
fn test_get_vf_user_region_is_public() {
	// `getVFUserRegion` is exported at the crate root.
	let _ = get_vf_user_region;
}

#[test]
fn test_read_v5_document_decovar() -> Result<()> {
	let doc = DesignSpaceDocument::from_file(datadir().join("test_v5_decovar.designspace"))?;

	assert!(doc.variable_fonts.is_empty());

	let axes: Vec<Axis> = [
		("Inline", "BLDA"),
		("Shearded", "TRMD"),
		("Rounded Slab", "TRMC"),
		("Stripes", "SKLD"),
		("Worm Terminal", "TRML"),
		("Inline Skeleton", "SKLA"),
		("Open Inline Terminal", "TRMF"),
		("Inline Terminal", "TRMK"),
		("Worm", "BLDB"),
		("Weight", "WMX2"),
		("Flared", "TRMB"),
		("Rounded", "TRMA"),
		("Worm Skeleton", "SKLB"),
		("Slab", "TRMG"),
		("Bifurcated", "TRME"),
	]
	.iter()
	.map(|(name, tag)| continuous(tag, name, 0, 0, 1000).into())
	.collect();
	assert_eq!(doc.axes, axes);

	let l = |name: &str, loc: &[(&str, i64)]| {
		LocationLabelDescriptor::new(name, simple_location(loc.iter().copied()))
	};
	assert_eq!(
		doc.location_labels,
		vec![
			LocationLabelDescriptor {
				elidable: true,
				..l("Default", &[])
			},
			LocationLabelDescriptor {
				label_names: names(&[("de", "Offen")]),
				..l("Open", &[("Inline", 1000)])
			},
			l("Worm", &[("Worm", 1000)]),
			l("Checkered", &[("Inline Skeleton", 1000)]),
			l("Checkered Reverse", &[("Inline Terminal", 1000)]),
			l("Striped", &[("Stripes", 500)]),
			l("Rounded", &[("Rounded", 1000)]),
			l("Flared", &[("Flared", 1000)]),
			l(
				"Flared Open",
				&[("Inline Skeleton", 1000), ("Flared", 1000)]
			),
			l("Rounded Slab", &[("Rounded Slab", 1000)]),
			l("Sheared", &[("Shearded", 1000)]),
			l("Bifurcated", &[("Bifurcated", 1000)]),
			l(
				"Inline",
				&[("Inline Skeleton", 500), ("Open Inline Terminal", 500)]
			),
			l("Slab", &[("Slab", 1000)]),
			l("Contrast", &[("Weight", 1000)]),
			l(
				"Fancy",
				&[
					("Inline Skeleton", 1000),
					("Flared", 1000),
					("Weight", 1000)
				]
			),
			l(
				"Mayhem",
				&[
					("Inline Skeleton", 1000),
					("Worm Skeleton", 1000),
					("Rounded", 500),
					("Flared", 500),
					("Rounded Slab", 750),
					("Bifurcated", 500),
					("Open Inline Terminal", 250),
					("Slab", 750),
					("Inline Terminal", 250),
					("Worm Terminal", 250),
					("Weight", 750),
					("Worm", 1000),
				]
			),
		]
	);

	let labels: Vec<Option<&str>> = doc
		.instances
		.iter()
		.map(|i| i.location_label.as_deref())
		.collect();
	assert_eq!(
		labels,
		[
			"Default",
			"Open",
			"Worm",
			"Checkered",
			"Checkered Reverse",
			"Striped",
			"Rounded",
			"Flared",
			"Flared Open",
			"Rounded Slab",
			"Sheared",
			"Bifurcated",
			"Inline",
			"Slab",
			"Contrast",
			"Fancy",
			"Mayhem",
		]
		.map(Some)
	);
	Ok(())
}

#[test]
fn test_read_v5_document_discrete() -> Result<()> {
	let doc = DesignSpaceDocument::from_file(datadir().join("test_v5_discrete.designspace"))?;

	assert!(doc.location_labels.is_empty());
	assert!(doc.variable_fonts.is_empty());

	let expected: Vec<Axis> = vec![
		DiscreteAxisDescriptor {
			axis_labels: vec![
				elidable(linked("Regular", 400, 700)),
				label("Bold", 700),
				label("Black", 900),
			],
			..discrete("wght", "Weight", &[400, 700, 900], 400)
		}
		.into(),
		DiscreteAxisDescriptor {
			axis_labels: vec![label("Narrow", 75), elidable(label("Normal", 100))],
			..discrete("wdth", "Width", &[75, 100], 100)
		}
		.into(),
		DiscreteAxisDescriptor {
			axis_labels: vec![elidable(linked("Roman", 0, 1)), label("Italic", 1)],
			..discrete("ital", "Italic", &[0, 1], 0)
		}
		.into(),
	];
	assert_eq!(doc.axes, expected);
	Ok(())
}

#[test]
fn test_read_v5_document_aktiv() -> Result<()> {
	let doc = DesignSpaceDocument::from_file(datadir().join("test_v5_aktiv.designspace"))?;

	assert!(doc.location_labels.is_empty());

	let expected: Vec<Axis> = vec![
		AxisDescriptor {
			map: map(&[
				(100, 22),
				(200, 38),
				(300, 57),
				(400, 84),
				(500, 98),
				(600, 115),
				(700, 133),
				(800, 158),
				(900, 185),
			]),
			axis_ordering: Some(1),
			axis_labels: vec![
				label("Hair", 100),
				label("Thin", 200),
				label("Light", 300),
				elidable(linked("Regular", 400, 700)),
				label("Medium", 500),
				label("SemiBold", 600),
				label("Bold", 700),
				label("XBold", 800),
				label("Black", 900),
			],
			..continuous("wght", "Weight", 100, 400, 900)
		}
		.into(),
		AxisDescriptor {
			axis_ordering: Some(0),
			axis_labels: vec![
				label("Cd", 75),
				elidable(label("Normal", 100)),
				label("Ex", 125),
			],
			..continuous("wdth", "Width", 75, 100, 125)
		}
		.into(),
		AxisDescriptor {
			axis_ordering: Some(2),
			axis_labels: vec![elidable(linked("Upright", 0, 1)), label("Italic", 1)],
			..continuous("ital", "Italic", 0, 0, 1)
		}
		.into(),
	];
	assert_eq!(doc.axes, expected);

	assert_eq!(
		vfs_without_paths(&doc.variable_fonts),
		vec![
			vf(
				"AktivGroteskVF_WghtWdthItal",
				vec![whole("Weight"), whole("Width"), whole("Italic")]
			),
			vf(
				"AktivGroteskVF_WghtWdth",
				vec![whole("Weight"), whole("Width")]
			),
			vf("AktivGroteskVF_Wght", vec![whole("Weight")]),
			vf(
				"AktivGroteskVF_Italics_WghtWdth",
				vec![whole("Weight"), whole("Width"), value("Italic", 1)]
			),
			vf(
				"AktivGroteskVF_Italics_Wght",
				vec![whole("Weight"), value("Italic", 1)]
			),
		]
	);
	Ok(())
}

/// `map_doc`: a document with a few axes to test the mapping functions.
fn map_doc() -> DesignSpaceDocument {
	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(AxisDescriptor {
		map: map(&[(100, 10), (900, 90)]),
		..continuous("wght", "Weight", 100, 100, 900)
	});
	doc.add_axis(AxisDescriptor {
		map: map(&[(75, 7500), (100, 10000), (200, 20000)]),
		..continuous("wdth", "Width", 75, 100, 200)
	});
	doc.add_axis(AxisDescriptor {
		default: Number::Float(1.5),
		..continuous("CUST", "Custom", 1, 0, 2)
	});
	doc.add_location_label(LocationLabelDescriptor::new(
		"Wonky",
		[
			("Weight".to_owned(), n(800)),
			("Custom".to_owned(), Number::Float(1.2)),
		]
		.into_iter()
		.collect(),
	));
	doc
}

fn user(items: &[(&str, f64)]) -> fontoxide_designspacelib::SimpleLocation {
	simple_location(items.iter().copied())
}

fn design(items: &[(&str, LocationValue)]) -> AnisotropicLocation {
	anisotropic_location(items.iter().copied())
}

#[test]
fn test_doc_location_map_forward() -> Result<()> {
	let doc = map_doc();
	assert_eq!(
		doc.map_forward(&simple_location([
			("Weight", 400),
			("Width", 150),
			("Custom", 2)
		]))?,
		user(&[("Weight", 40.0), ("Width", 15000.0), ("Custom", 2.0)]),
		"The mappings should be used to compute the design locations"
	);
	assert_eq!(
		doc.map_forward(&simple_location([("Weight", 400)]))?,
		user(&[("Weight", 40.0), ("Width", 10000.0), ("Custom", 1.5)]),
		"Missing user locations should be assumed equal to the axis's default"
	);
	Ok(())
}

#[test]
fn test_doc_location_map_backward() -> Result<()> {
	let doc = map_doc();
	assert_eq!(
		doc.map_backward(&anisotropic_location([
			("Weight", 40),
			("Width", 15000),
			("Custom", 2)
		]))?,
		user(&[("Weight", 400.0), ("Width", 150.0), ("Custom", 2.0)]),
		"The mappings should be used to compute the user locations"
	);
	assert_eq!(
		doc.map_backward(&anisotropic_location([("Weight", 40)]))?,
		user(&[("Weight", 400.0), ("Width", 100.0), ("Custom", 1.5)]),
		"Missing design locations should be assumed equal to the axis's default"
	);
	assert_eq!(
		doc.map_backward(&design(&[
			("Weight", (40, 50).into()),
			("Width", (15000, 100000).into()),
			("Custom", (2.0, 1.5).into()),
		]))?,
		user(&[("Weight", 400.0), ("Width", 150.0), ("Custom", 2.0)]),
		"Only the xvalue of anisotropic locations is used"
	);
	Ok(())
}

#[test]
fn test_instance_location_from_label() -> Result<()> {
	let doc = map_doc();
	let inst = InstanceDescriptor {
		location_label: Some("Wonky".into()),
		..Default::default()
	};
	assert_eq!(
		inst.get_full_user_location(&doc)?,
		user(&[("Weight", 800.0), ("Width", 100.0), ("Custom", 1.2)])
	);
	assert_eq!(
		inst.get_full_design_location(&doc)?,
		design(&[
			("Weight", 80.into()),
			("Width", 10000.into()),
			("Custom", 1.2.into())
		])
	);

	let inst = InstanceDescriptor {
		location_label: Some("Wonky".into()),
		user_location: simple_location([("Width", 200)]),
		..Default::default()
	};
	assert_eq!(
		inst.get_full_user_location(&doc)?,
		user(&[("Weight", 800.0), ("Width", 100.0), ("Custom", 1.2)])
	);
	assert_eq!(
		inst.get_full_design_location(&doc)?,
		design(&[
			("Weight", 80.into()),
			("Width", 10000.into()),
			("Custom", 1.2.into())
		])
	);
	Ok(())
}

#[test]
fn test_instance_location_no_data() -> Result<()> {
	let doc = map_doc();
	let inst = InstanceDescriptor::new();
	assert_eq!(
		inst.get_full_user_location(&doc)?,
		user(&[("Weight", 100.0), ("Width", 100.0), ("Custom", 1.5)])
	);
	assert_eq!(
		inst.get_full_design_location(&doc)?,
		design(&[
			("Weight", 10.into()),
			("Width", 10000.into()),
			("Custom", 1.5.into())
		])
	);
	Ok(())
}

#[test]
fn test_instance_location_design_first() -> Result<()> {
	let doc = map_doc();
	let inst = InstanceDescriptor {
		design_location: design(&[
			("Weight", (60, 61).into()),
			("Width", 11000.into()),
			("Custom", 1.2.into()),
		]),
		user_location: user(&[("Weight", 700.0), ("Width", 180.0), ("Custom", 1.4)]),
		..Default::default()
	};
	assert_eq!(
		inst.get_full_user_location(&doc)?,
		user(&[("Weight", 600.0), ("Width", 110.0), ("Custom", 1.2)])
	);
	assert_eq!(
		inst.get_full_design_location(&doc)?,
		design(&[
			("Weight", (60, 61).into()),
			("Width", 11000.into()),
			("Custom", 1.2.into()),
		])
	);
	Ok(())
}

#[test]
fn test_instance_location_mix() -> Result<()> {
	let doc = map_doc();
	let inst = InstanceDescriptor {
		design_location: design(&[("Weight", (60, 61).into())]),
		user_location: user(&[("Width", 180.0)]),
		..Default::default()
	};
	assert_eq!(
		inst.get_full_user_location(&doc)?,
		user(&[("Weight", 600.0), ("Width", 180.0), ("Custom", 1.5)])
	);
	assert_eq!(
		inst.get_full_design_location(&doc)?,
		design(&[
			("Weight", (60, 61).into()),
			("Width", 18000.into()),
			("Custom", 1.5.into()),
		])
	);
	Ok(())
}

#[test]
fn test_roundtrip() -> Result<()> {
	for filename in [
		"test_v4_original.designspace",
		"test_v5_original.designspace",
		"test_v5_aktiv.designspace",
		"test_v5_decovar.designspace",
		"test_v5_discrete.designspace",
		"test_v5_sourceserif.designspace",
		"test_v5.designspace",
	] {
		let tmp = tmpdir(&format!("roundtrip-{filename}"))?;
		// Move the file to the tmpdir so that the filenames stay the same
		// (they're relative to the file's path)
		let output_path = copy_data(filename, &tmp)?;
		let mut doc = DesignSpaceDocument::from_file(&output_path)?;
		doc.write(&output_path)?;
		// The input XML has comments and empty lines for documentation purposes
		let xml = read_text(&datadir().join(filename))?;
		let xml = remove_between(
			&xml,
			"<!-- ROUNDTRIP_TEST_REMOVE_ME_BEGIN -->",
			"<!-- ROUNDTRIP_TEST_REMOVE_ME_END -->",
		);
		let xml = common::collapse_blank_lines(&remove_between(&xml, "<!--", "-->"));
		assert_eq!(read_text(&output_path)?, xml, "{filename}");
	}
	let _ = expected_text;
	Ok(())
}

#[test]
fn test_using_v5_features_upgrades_format() -> Result<()> {
	let tmp = tmpdir("using_v5_features_upgrades_format")?;
	let test_file = copy_data("test_v4_original.designspace", &tmp)?;
	let output_4_path = tmp.join("test_v4.designspace");
	let output_5_path = tmp.join("test_v5.designspace");
	std::fs::rename(&test_file, &output_4_path)?;
	let mut doc = DesignSpaceDocument::from_file(&output_4_path)?;
	doc.write(&output_4_path)?;
	assert!(read_text(&output_4_path)?.contains("format=\"4.1\""));
	doc.add_variable_font(VariableFontDescriptor::new("TestVF"));
	doc.write(&output_5_path)?;
	assert!(read_text(&output_5_path)?.contains("format=\"5.0\""));
	Ok(())
}

#[test]
fn test_add_axis_descriptor_discrete() -> Result<()> {
	let mut ds = DesignSpaceDocument::new();
	let expected = DiscreteAxisDescriptor {
		hidden: true,
		map: map(&[(0, -12), (1, 0)]),
		axis_ordering: Some(3),
		axis_labels: vec![AxisLabelDescriptor {
			elidable: true,
			older_sibling: true,
			linked_user_value: Some(n(1)),
			label_names: names(&[("fr", "Romain")]),
			..label("Roman", 0)
		}],
		..discrete("ital", "Italic", &[0, 1], 0)
	};
	let axis = ds.add_axis(expected.clone()).clone();
	assert_eq!(ds.axes[0], axis);
	assert_eq!(
		axis.as_discrete().ok_or_else(|| fail("discrete"))?,
		&expected
	);
	Ok(())
}

#[test]
fn test_add_location_label_descriptor() {
	let mut ds = DesignSpaceDocument::new();
	let expected = LocationLabelDescriptor {
		elidable: true,
		older_sibling: true,
		label_names: names(&[("fr", "Quelque part")]),
		..LocationLabelDescriptor::new("Somewhere", Default::default())
	};
	let label = ds.add_location_label(expected.clone()).clone();
	assert_eq!(ds.location_labels[0], label);
	assert_eq!(label, expected);
}

#[test]
fn test_add_variable_font_descriptor() {
	let mut ds = DesignSpaceDocument::new();
	let mut expected = VariableFontDescriptor::new("TestVF");
	expected.set_filename(Some("TestVF.ttf"));
	let vf = ds.add_variable_font(expected.clone()).clone();
	assert_eq!(ds.variable_fonts[0], vf);
	assert_eq!(vf, expected);
}
