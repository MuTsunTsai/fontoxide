//! `Tests/designspaceLib/split_test.py`.

mod common;

use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard};

use common::{copy_data, datadir, fail, read_text, tmpdir};
use fontoxide_designspacelib::plistlib::{Dict, Value};
use fontoxide_designspacelib::split::{
	SplitOptions, condition_set_from, convert5to4, split_interpolable, split_variable_fonts,
};
use fontoxide_designspacelib::types::Range;
use fontoxide_designspacelib::{
	AxisDescriptor, Condition, DesignSpaceDocument, DiscreteAxisDescriptor, InstanceDescriptor,
	Number, RangeAxisSubsetDescriptor, Result, RuleDescriptor, SimpleLocation, SourceDescriptor,
	VariableFontDescriptor, anisotropic_location, simple_location,
};

fn n(v: i64) -> Number {
	Number::Int(v)
}

fn stem(name: &str) -> &str {
	name.strip_suffix(".designspace").unwrap_or(name)
}

#[test]
#[allow(clippy::type_complexity)]
fn test_split() -> Result<()> {
	let _guard = caplog();
	let cases: [(&str, Vec<(SimpleLocation, Vec<&str>)>); 3] = [
		(
			"test_v5_aktiv.designspace",
			vec![(
				SimpleLocation::new(),
				vec![
					"AktivGroteskVF_Italics_Wght",
					"AktivGroteskVF_Italics_WghtWdth",
					"AktivGroteskVF_Wght",
					"AktivGroteskVF_WghtWdth",
					"AktivGroteskVF_WghtWdthItal",
				],
			)],
		),
		(
			"test_v5_sourceserif.designspace",
			vec![
				(
					simple_location([("italic", 0)]),
					vec!["SourceSerif4Variable-Roman"],
				),
				(
					simple_location([("italic", 1)]),
					vec!["SourceSerif4Variable-Italic"],
				),
			],
		),
		(
			"test_v5_MutatorSans_and_Serif.designspace",
			vec![
				(
					simple_location([("serif", 0)]),
					vec![
						"MutatorSansVariable_Weight_Width",
						"MutatorSansVariable_Weight",
						"MutatorSansVariable_Width",
					],
				),
				(
					simple_location([("serif", 1)]),
					vec!["MutatorSerifVariable_Width"],
				),
			],
		),
	];
	for (test_ds, expected_interpolable_spaces) in cases {
		let tmp = tmpdir(&format!("split-{test_ds}"))?;
		let temp_in = copy_data(test_ds, &tmp)?;
		let doc = DesignSpaceDocument::from_file(&temp_in)?;
		let out_dir = tmp.join("out");
		std::fs::create_dir_all(&out_dir)?;

		let spaces = split_interpolable(&doc, SplitOptions::interpolable())?;
		assert_eq!(spaces.len(), expected_interpolable_spaces.len());
		for ((location, mut sub_doc), (expected_location, expected_vf_names)) in
			spaces.into_iter().zip(expected_interpolable_spaces)
		{
			assert_eq!(location, expected_location);
			let vfs = split_variable_fonts(&sub_doc, SplitOptions::variable_fonts())?;
			let vf_names: BTreeSet<&str> = vfs.iter().map(|(name, _)| name.as_str()).collect();
			assert_eq!(vf_names, expected_vf_names.into_iter().collect());

			let mut items: Vec<(&String, &Number)> = location.iter().collect();
			items.sort_by(|a, b| a.0.cmp(b.0));
			let loc_str = items
				.iter()
				.map(|(name, value)| format!("{name}_{value}"))
				.collect::<Vec<_>>()
				.join("_");
			let file_name = format!("{}_{loc_str}.designspace", stem(test_ds));
			let temp_out = out_dir.join(&file_name);
			sub_doc.write(&temp_out)?;
			assert_eq!(
				read_text(&datadir().join("split_output").join(&file_name))?,
				read_text(&temp_out)?,
				"{file_name}"
			);

			for (vf_name, mut vf_doc) in vfs {
				let file_name = format!("{vf_name}.designspace");
				let temp_out = out_dir.join(&file_name);
				vf_doc.write(&temp_out)?;
				assert_eq!(
					read_text(&datadir().join("split_output").join(&file_name))?,
					read_text(&temp_out)?,
					"{file_name}"
				);
			}
		}
	}
	Ok(())
}

#[test]
fn test_convert5to4() -> Result<()> {
	let _guard = caplog();
	let cases: [(&str, &[&str]); 2] = [
		(
			"test_v5_aktiv.designspace",
			&[
				"AktivGroteskVF_Italics_Wght",
				"AktivGroteskVF_Italics_WghtWdth",
				"AktivGroteskVF_Wght",
				"AktivGroteskVF_WghtWdth",
				"AktivGroteskVF_WghtWdthItal",
			],
		),
		(
			"test_v5_sourceserif.designspace",
			&["SourceSerif4Variable-Italic", "SourceSerif4Variable-Roman"],
		),
	];
	for (test_ds, expected_vfs) in cases {
		let tmp = tmpdir(&format!("convert5to4-{test_ds}"))?;
		let temp_in = copy_data(test_ds, &tmp)?;
		let doc = DesignSpaceDocument::from_file(&temp_in)?;
		let out_dir = tmp.join("out");
		std::fs::create_dir_all(&out_dir)?;

		let variable_fonts = convert5to4(&doc)?;
		let names: BTreeSet<&str> = variable_fonts.keys().map(String::as_str).collect();
		assert_eq!(names, expected_vfs.iter().copied().collect());
		for (vf_name, mut vf) in variable_fonts {
			let file_name = format!("{vf_name}.designspace");
			let temp_out = out_dir.join(&file_name);
			vf.write(&temp_out)?;
			assert_eq!(
				read_text(&datadir().join("convert5to4_output").join(&file_name))?,
				read_text(&temp_out)?,
				"{file_name}"
			);
		}
	}
	Ok(())
}

fn weight_doc(minimum: i64, default: i64, maximum: i64) -> DesignSpaceDocument {
	let mut doc = DesignSpaceDocument::new();
	doc.add_axis(AxisDescriptor {
		name: Some("Weight".into()),
		tag: Some("wght".into()),
		minimum: n(minimum),
		default: n(default),
		maximum: n(maximum),
		..Default::default()
	});
	doc
}

#[test]
fn test_optional_min_max() -> Result<()> {
	let _guard = caplog();
	// Check that split functions can handle conditions that are partially
	// unbounded without tripping over None values and missing keys.
	for unbounded_condition in [
		Condition::new("Weight", Some(n(500)), None),
		Condition::new("Weight", None, Some(n(500))),
	] {
		let mut doc = weight_doc(400, 400, 1000);
		doc.add_rule(RuleDescriptor {
			name: Some("unbounded".into()),
			condition_sets: vec![vec![unbounded_condition]],
			..Default::default()
		});
		assert_eq!(
			split_interpolable(&doc, SplitOptions::interpolable())?.len(),
			1
		);
		assert_eq!(
			split_variable_fonts(&doc, SplitOptions::variable_fonts())?.len(),
			1
		);
	}
	Ok(())
}

/// Check that split's internal helper functions produce the correct output
/// for conditions that are partially unbounded. A missing bound and `None`
/// are both `None` here.
#[test]
fn test_optional_min_max_internal() {
	let f = Number::Float;
	for (condition, expected) in [
		(
			Condition::new("axis", Some(f(0.5)), None),
			Range::without_default(f(0.5), f(f64::INFINITY)),
		),
		(
			Condition::new("axis", None, Some(f(0.5))),
			Range::without_default(f(f64::NEG_INFINITY), f(0.5)),
		),
	] {
		let set = condition_set_from(&[condition]);
		assert_eq!(set.len(), 1);
		assert_eq!(set["axis"], expected);
	}
}

fn dict(items: Vec<(&str, Value)>) -> Dict {
	items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

#[test]
fn test_split_variable_font_font_info() -> Result<()> {
	let _guard = caplog();
	let info = |items: Vec<(&str, Value)>| Some(dict(items));
	let cases: Vec<(Option<Dict>, Option<Dict>, Option<Dict>)> = vec![
		(None, None, None),
		(
			info(vec![("versionMajor", 5.into())]),
			None,
			info(vec![("versionMajor", 5.into())]),
		),
		(
			None,
			info(vec![("styleName", "Regular".into())]),
			info(vec![("styleName", "Regular".into())]),
		),
		(
			info(vec![("versionMajor", 5.into())]),
			info(vec![]),
			info(vec![("versionMajor", 5.into())]),
		),
		(
			info(vec![
				("versionMajor", 5.into()),
				("versionMinor", 55.into()),
				("styleName", "Root".into()),
			]),
			info(vec![
				("versionMinor", 56.into()),
				("styleName", "Regular".into()),
			]),
			info(vec![
				("versionMajor", 5.into()),
				("versionMinor", 56.into()),
				("styleName", "Regular".into()),
			]),
		),
	];
	for (root_info, vf_info, expected_info) in cases {
		let mut doc = weight_doc(400, 400, 700);
		doc.lib = dict(vec![
			("shared", true.into()),
			("custom", Value::Dict(dict(vec![("root", true.into())]))),
		]);
		let vf = doc.add_variable_font(VariableFontDescriptor {
			lib: dict(vec![(
				"custom",
				Value::Dict(dict(vec![("vf", true.into())])),
			)]),
			..VariableFontDescriptor::new("TestVF")
		});
		vf.axis_subsets = vec![RangeAxisSubsetDescriptor::new("Weight").into()];
		if let Some(root_info) = root_info {
			doc.lib
				.insert("public.fontInfo".into(), Value::Dict(root_info));
		}
		if let Some(vf_info) = vf_info {
			doc.variable_fonts[0]
				.lib
				.insert("public.fontInfo".into(), Value::Dict(vf_info));
		}
		let (original_root_lib, original_vf_lib) =
			(doc.lib.clone(), doc.variable_fonts[0].lib.clone());

		let vfs = split_variable_fonts(&doc, SplitOptions::variable_fonts())?;
		let (_, vf_doc) = vfs.first().ok_or_else(|| fail("no vf"))?;

		let mut expected_lib = dict(vec![
			("shared", true.into()),
			("custom", Value::Dict(dict(vec![("vf", true.into())]))),
		]);
		if let Some(expected_info) = expected_info {
			expected_lib.insert("public.fontInfo".into(), Value::Dict(expected_info));
		}
		assert_eq!(vf_doc.lib, expected_lib);
		assert_eq!(doc.lib, original_root_lib);
		assert_eq!(doc.variable_fonts[0].lib, original_vf_lib);
	}
	Ok(())
}

#[test]
fn test_split_variable_font_preserves_zero_axis_default() -> Result<()> {
	let _guard = caplog();
	let doc = DesignSpaceDocument::from_string(
		r#"<designspace format="5.0">
          <axes>
            <axis name="Weight" tag="wght" minimum="-100" default="50" maximum="100"/>
          </axes>
          <variable-fonts>
            <variable-font name="WeightSubset">
              <axis-subsets>
                <axis-subset name="Weight" userminimum="-20" userdefault="0" usermaximum="20"/>
              </axis-subsets>
            </variable-font>
          </variable-fonts>
        </designspace>
        "#,
	)?;
	let vfs = split_variable_fonts(&doc, SplitOptions::variable_fonts())?;
	let (name, sub_doc) = vfs.first().ok_or_else(|| fail("no vf"))?;
	assert_eq!(name, "WeightSubset");
	let axis = sub_doc
		.get_axis("Weight")
		.and_then(|a| a.as_continuous())
		.ok_or_else(|| fail("Weight"))?;
	assert_eq!(axis.minimum, n(-20));
	assert_eq!(axis.default, n(0));
	assert_eq!(axis.maximum, n(20));
	Ok(())
}

#[test]
fn test_avar2() -> Result<()> {
	let _guard = caplog();
	let ds = DesignSpaceDocument::from_file(datadir().join("test_avar2.designspace"))?;
	let spaces = split_interpolable(&ds, SplitOptions::interpolable())?;
	let (_, sub_doc) = spaces.first().ok_or_else(|| fail("no space"))?;
	assert_eq!(sub_doc.axis_mappings.len(), 2);

	let sub_docs = split_variable_fonts(&ds, SplitOptions::variable_fonts())?;
	assert_eq!(sub_docs.len(), 5);
	for (i, (_, sub_doc)) in sub_docs.iter().enumerate() {
		// Only the first one should have a mapping, according to the document
		if i == 0 {
			assert_eq!(sub_doc.axis_mappings.len(), 2);
		} else {
			assert_eq!(sub_doc.axis_mappings.len(), 0);
		}
	}
	Ok(())
}

fn make_names_doc(axes: &str, instances: &str) -> Result<DesignSpaceDocument> {
	DesignSpaceDocument::from_string(&format!(
		r#"<?xml version='1.0' encoding='UTF-8'?>
        <designspace format="5.0">
          <axes>{axes}</axes>
          <sources>
            <source filename="m400.ttf" familyname="Bug">
              <location><dimension name="Weight" xvalue="400"/></location>
            </source>
            <source filename="m700.ttf">
              <location><dimension name="Weight" xvalue="700"/></location>
            </source>
          </sources>
          <instances>{instances}</instances>
        </designspace>
        "#
	))
}

const WGHT_AXIS: &str =
	r#"<axis tag="wght" name="Weight" minimum="400" default="400" maximum="700"/>"#;
const WGHT_AXIS_LABELS: &str = r#"
    <axis tag="wght" name="Weight" minimum="400" default="400" maximum="700">
      <labels>
        <label uservalue="400" name="Regular" elidable="true"/>
        <label uservalue="700" name="Bold">
          <labelname xml:lang="de">Fett</labelname>
        </label>
      </labels>
    </axis>
"#;
// an italic-only family: the STAT names compose to "Italic" and "Bold Italic"
const ITALIC_FAMILY_AXES_LABELS: &str = r#"
    <axis tag="wght" name="Weight" minimum="400" default="400" maximum="700">
      <labels>
        <label uservalue="400" name="Regular" elidable="true"/>
        <label uservalue="700" name="Bold"/>
      </labels>
    </axis>
    <axis tag="ital" name="Italic" values="1" default="1">
      <labels>
        <label uservalue="1" name="Italic"/>
      </labels>
    </axis>
"#;

fn named_instances(style_names: &[&str]) -> String {
	style_names
		.iter()
		.zip([400, 700])
		.map(|(style_name, xvalue)| {
			format!(
				r#"
    <instance familyname="Bug" stylename="{style_name}">
      <location><dimension name="Weight" xvalue="{xvalue}"/></location>
    </instance>
    "#
			)
		})
		.collect()
}

#[test]
fn test_make_names_explicit_instance_names_take_precedence() -> Result<()> {
	let _guard = caplog();
	// https://github.com/fonttools/fonttools/issues/4206
	let options = SplitOptions {
		make_names: true,
		..SplitOptions::variable_fonts()
	};
	for (axes, style_names) in [
		// no STAT labels
		(WGHT_AXIS, ["Italic", "Bold Italic"]),
		// italic-only family with weight labels only: STAT would give "" and "Bold"
		(WGHT_AXIS_LABELS, ["Italic", "Bold Italic"]),
		// the elidable "Regular" is dropped from the STAT name, not from the
		// instance's own: STAT would give "Italic" and "Bold Italic"
		(ITALIC_FAMILY_AXES_LABELS, ["Regular Italic", "Bold Italic"]),
	] {
		let doc = make_names_doc(axes, &named_instances(&style_names))?;
		let vfs = split_variable_fonts(&doc, options)?;
		let (_, sub_doc) = vfs.first().ok_or_else(|| fail("no vf"))?;
		let got: Vec<Option<&str>> = sub_doc
			.instances
			.iter()
			.map(|i| i.style_name.as_deref())
			.collect();
		assert_eq!(got, style_names.map(Some));
		let families: Vec<Option<&str>> = sub_doc
			.instances
			.iter()
			.map(|i| i.family_name.as_deref())
			.collect();
		assert_eq!(families, [Some("Bug"), Some("Bug")]);
		// localised STAT names are only kept when they agree with the instance name
		for instance in &sub_doc.instances {
			assert_eq!(
				instance
					.localised_style_name
					.get("en")
					.or(instance.style_name.as_ref()),
				instance.style_name.as_ref()
			);
		}
		// no PostScript names are made up for instances with their own style name
		let ps: Vec<Option<&str>> = sub_doc
			.instances
			.iter()
			.map(|i| i.post_script_font_name.as_deref())
			.collect();
		assert_eq!(ps, [None, None]);
	}
	Ok(())
}

#[test]
fn test_make_names_from_stat_labels_when_instance_has_no_names() -> Result<()> {
	let _guard = caplog();
	let instances = r#"
    <instance>
      <location><dimension name="Weight" xvalue="700"/></location>
    </instance>
    <instance stylename="Bold">
      <location><dimension name="Weight" xvalue="700"/></location>
    </instance>
    "#;
	let doc = make_names_doc(WGHT_AXIS_LABELS, instances)?;
	let options = SplitOptions {
		make_names: true,
		..SplitOptions::variable_fonts()
	};
	let vfs = split_variable_fonts(&doc, options)?;
	let (_, sub_doc) = vfs.first().ok_or_else(|| fail("no vf"))?;
	let [unnamed, named] = &sub_doc.instances[..] else {
		return Err(fail("expected two instances"));
	};
	assert_eq!(unnamed.family_name.as_deref(), Some("Bug"));
	assert_eq!(named.family_name.as_deref(), Some("Bug"));
	assert_eq!(unnamed.style_name.as_deref(), Some("Bold"));
	assert_eq!(named.style_name.as_deref(), Some("Bold"));
	assert_eq!(unnamed.post_script_font_name.as_deref(), Some("Bug-Bold"));
	assert_eq!(named.post_script_font_name, None);
	// the localised STAT names apply when they translate the name in use
	let expected: fontoxide_designspacelib::LocalizedNames = [("en", "Bold"), ("de", "Fett")]
		.into_iter()
		.map(|(k, v)| (k.to_owned(), v.to_owned()))
		.collect();
	assert_eq!(unnamed.localised_style_name, expected);
	assert_eq!(named.localised_style_name, expected);
	Ok(())
}

/// `caplog`: the messages logged while running a function. The logger is
/// global, so tests using it run one at a time.
struct CapLog;

static RECORDS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static CAPTURE: Mutex<()> = Mutex::new(());

impl log::Log for CapLog {
	fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
		metadata.level() <= log::Level::Warn
	}

	fn log(&self, record: &log::Record<'_>) {
		if self.enabled(record.metadata())
			&& let Ok(mut records) = RECORDS.lock()
		{
			records.push(record.args().to_string());
		}
	}

	fn flush(&self) {}
}

fn caplog() -> MutexGuard<'static, ()> {
	let guard = CAPTURE
		.lock()
		.unwrap_or_else(std::sync::PoisonError::into_inner);
	if log::set_logger(&CapLog).is_ok() {
		log::set_max_level(log::LevelFilter::Warn);
	}
	take_records();
	guard
}

fn take_records() -> Vec<String> {
	RECORDS
		.lock()
		.map(|mut r| std::mem::take(&mut *r))
		.unwrap_or_default()
}

fn mapped_weight_doc() -> DesignSpaceDocument {
	let mut doc = weight_doc(400, 400, 700);
	if let Some(fontoxide_designspacelib::Axis::Continuous(a)) = doc.axes.first_mut() {
		a.map = vec![(n(400), n(80)), (n(700), n(200))];
	}
	doc
}

fn named_source(name: &str, w: i64) -> SourceDescriptor {
	let mut s = SourceDescriptor {
		name: Some(name.into()),
		design_location: anisotropic_location([("Weight", w)]),
		..Default::default()
	};
	s.set_filename(Some(&format!("{name}.ufo")));
	s
}

fn named_instance(name: &str, design: Option<i64>, user: Option<i64>) -> InstanceDescriptor {
	let mut i = InstanceDescriptor {
		name: Some(name.into()),
		..Default::default()
	};
	if let Some(w) = design {
		i.design_location = anisotropic_location([("Weight", w)]);
	}
	if let Some(w) = user {
		i.user_location = simple_location([("Weight", w)]);
	}
	i.set_filename(Some(&format!("{name}.ttf")));
	i
}

#[test]
fn test_split_source_outside_axes_warns() -> Result<()> {
	let _guard = caplog();
	let mut doc = mapped_weight_doc();
	for (name, w) in [("Light", 30), ("Regular", 80), ("Bold", 200)] {
		doc.add_source(named_source(name, w));
	}

	let sub_docs = split_interpolable(&doc, SplitOptions::interpolable())?;
	assert_eq!(sub_docs.len(), 1);
	let names: Vec<Option<&str>> = sub_docs[0]
		.1
		.sources
		.iter()
		.map(|s| s.name.as_deref())
		.collect();
	assert_eq!(names, [Some("Regular"), Some("Bold")]);
	let text = take_records().join("\n");
	assert!(text.contains(
		"Source 'Light' has location outside the document's axes, dropping: {'Weight': 350}"
	));

	let vf_docs = split_variable_fonts(&doc, SplitOptions::variable_fonts())?;
	assert_eq!(vf_docs.len(), 1);
	let names: Vec<Option<&str>> = vf_docs[0]
		.1
		.sources
		.iter()
		.map(|s| s.name.as_deref())
		.collect();
	assert_eq!(names, [Some("Regular"), Some("Bold")]);
	let text = take_records().join("\n");
	assert!(text.contains(
		"Source 'Light' has location outside the document's axes, dropping: {'Weight': 350}"
	));
	Ok(())
}

#[test]
fn test_split_instance_outside_axes_warns() -> Result<()> {
	let _guard = caplog();
	let mut doc = mapped_weight_doc();
	for (name, w) in [("Regular", 80), ("Bold", 200)] {
		let mut s = named_source(name, w);
		s.family_name = Some("TestFamily".into());
		doc.add_source(s);
	}
	for (name, w) in [
		("Thin", 30),
		("Regular", 80),
		("Bold", 200),
		("ExtraBold", 250),
	] {
		doc.add_instance(named_instance(name, Some(w), None));
	}

	let sub_docs = split_interpolable(&doc, SplitOptions::interpolable())?;
	assert_eq!(sub_docs.len(), 1);
	let names: Vec<Option<&str>> = sub_docs[0]
		.1
		.instances
		.iter()
		.map(|s| s.name.as_deref())
		.collect();
	assert_eq!(names, [Some("Regular"), Some("Bold")]);
	let text = take_records().join("\n");
	assert!(text.contains(
		"Instance 'Thin' has location outside the document's axes, dropping: {'Weight': 350}"
	));
	assert!(text.contains(
		"Instance 'ExtraBold' has location outside the document's axes, dropping: {'Weight': 750}"
	));
	Ok(())
}

#[test]
fn test_split_explicit_variable_font_subspace_silent() -> Result<()> {
	let _guard = caplog();
	let mut doc = weight_doc(100, 400, 900);
	for (name, w) in [
		("Light", 100),
		("Regular", 400),
		("Bold", 900),
		("OutOfRange", 1000),
	] {
		doc.add_source(named_source(name, w));
	}
	for (name, w) in [
		("Light", 100),
		("Regular", 400),
		("Bold", 900),
		("OutOfRange", 1000),
	] {
		doc.add_instance(named_instance(name, None, Some(w)));
	}
	doc.add_variable_font(VariableFontDescriptor {
		axis_subsets: vec![
			RangeAxisSubsetDescriptor {
				user_minimum: n(100),
				user_maximum: n(500),
				..RangeAxisSubsetDescriptor::new("Weight")
			}
			.into(),
		],
		..VariableFontDescriptor::new("VFSub")
	});

	let vfs = split_variable_fonts(&doc, SplitOptions::variable_fonts())?;
	assert_eq!(vfs.len(), 1);
	let (vf_name, vf_doc) = &vfs[0];
	assert_eq!(vf_name, "VFSub");
	let sources: Vec<Option<&str>> = vf_doc.sources.iter().map(|s| s.name.as_deref()).collect();
	assert_eq!(sources, [Some("Light"), Some("Regular")]);
	let instances: Vec<Option<&str>> = vf_doc.instances.iter().map(|s| s.name.as_deref()).collect();
	assert_eq!(instances, [Some("Light"), Some("Regular")]);
	let text = take_records().join("\n");
	// Bold was dropped because it's outside the explicit variable-font sub-space,
	// but since it's within the document's own axes, no warning should be logged for it.
	assert!(!text.contains("Bold"));
	// OutOfRange is outside the document's own axes, so it should warn.
	assert!(text.contains(
		"Source 'OutOfRange' has location outside the document's axes, dropping: {'Weight': 1000}"
	));
	assert!(text.contains(
		"Instance 'OutOfRange' has location outside the document's axes, dropping: {'Weight': 1000}"
	));
	Ok(())
}

#[test]
fn test_split_variable_fonts_two_vfs_warns_once() -> Result<()> {
	let _guard = caplog();
	let mut doc = weight_doc(100, 400, 900);
	for (name, w) in [("Light", 100), ("Bold", 900), ("OutOfRange", 1000)] {
		doc.add_source(named_source(name, w));
	}
	for (name, minimum, maximum) in [("VF1", 100, 400), ("VF2", 400, 900)] {
		doc.add_variable_font(VariableFontDescriptor {
			axis_subsets: vec![
				RangeAxisSubsetDescriptor {
					user_minimum: n(minimum),
					user_maximum: n(maximum),
					..RangeAxisSubsetDescriptor::new("Weight")
				}
				.into(),
			],
			..VariableFontDescriptor::new(name)
		});
	}

	let vfs = split_variable_fonts(&doc, SplitOptions::variable_fonts())?;
	assert_eq!(vfs.len(), 2);
	let records = take_records();
	let matching = records
		.iter()
		.filter(|r| r.contains("Source 'OutOfRange'"))
		.count();
	assert_eq!(matching, 1);
	Ok(())
}

#[test]
fn test_split_discrete_axis_source_outside_values_warns() -> Result<()> {
	let _guard = caplog();
	let mut doc = weight_doc(400, 400, 700);
	doc.add_axis(DiscreteAxisDescriptor {
		name: Some("Italic".into()),
		tag: Some("ital".into()),
		values: vec![n(0), n(1)],
		default: n(0),
		..Default::default()
	});
	for (name, italic) in [("Roman", 0), ("Italic", 1), ("BadItalic", 2)] {
		let mut s = named_source(name, 400);
		s.design_location = anisotropic_location([("Weight", 400), ("Italic", italic)]);
		doc.add_source(s);
	}

	let sub_docs = split_interpolable(&doc, SplitOptions::interpolable())?;
	assert_eq!(sub_docs.len(), 2);
	let records = take_records();
	let matching: Vec<&String> = records
		.iter()
		.filter(|r| r.contains("Source 'BadItalic'"))
		.collect();
	assert_eq!(matching.len(), 1);
	assert!(
		matching[0]
			.contains("Source 'BadItalic' has location outside the document's axes, dropping")
	);
	Ok(())
}
