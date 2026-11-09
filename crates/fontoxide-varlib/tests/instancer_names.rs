//! Port of fontTools' `Tests/varLib/instancer/names_test.py`.

#![allow(clippy::type_complexity)]

mod common;

use std::collections::BTreeMap;

use common::*;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::{Fvar, Name, Stat};
use fontoxide_ttlib::{Font, Tag};
use fontoxide_varlib::instancer::test_support::{
	ELIDABLE_AXIS_VALUE_NAME, prune_unused_names, update_name_table, variation_name_ids,
};
use fontoxide_varlib::instancer::{AxisTriple, InstancerOptions, instantiate_variable_font};

const WIN: u16 = 0x409;

type Limits<'a> = &'a [(&'a str, Option<AxisTriple>)];
type Expected<'a> = &'a [((u16, u16, u16, u16), &'a str)];

/// The `varfont` fixture.
fn varfont() -> TestResult<Font> {
	font_from_ttx(&instancer_data("PartialInstancerTest-VF.ttx"))
}

fn update(font: &mut Font, pairs: Limits<'_>) -> fontoxide_ttlib::Result<()> {
	update_name_table(font, &limits(pairs))
}

fn win(name_id: u16) -> (u16, u16, u16, u16) {
	(name_id, 3, 1, WIN)
}

/// `_test_name_records`.
fn test_name_records(
	font: &mut Font,
	expected: Expected<'_>,
	is_non_ribbi: bool,
	platforms: &[u16],
) -> TestResult {
	let has_fvar = font.contains(Tag::new(b"fvar"));
	let names = font.require::<Name>()?;
	let mut font_names = BTreeMap::new();
	for r in &names.names {
		font_names.insert(
			(r.name_id, r.platform_id, r.plat_enc_id, r.lang_id),
			r.to_unicode()?,
		);
	}
	for (k, v) in expected {
		if !platforms.contains(&k.3) {
			continue;
		}
		assert_eq!(font_names[k], *v, "{k:?}");
	}
	let has_id = |id: u16| font_names.keys().any(|k| k.0 == id);
	if is_non_ribbi {
		assert!(has_id(16));
		assert!(has_id(17));
	}
	if !has_fvar {
		assert!(!has_id(25));
	}
	Ok(())
}

#[test]
fn test_pruning_unused_names() -> TestResult {
	let mut font = varfont()?;
	let var_name_ids = variation_name_ids(&mut font)?;
	assert_eq!(var_name_ids, (256..=297).collect());

	let original = variation_name_ids(&mut font)?;
	font.require::<Fvar>()?.axes.remove(0); // Weight (nameID=256)
	font.require::<Fvar>()?.instances.remove(0); // Thin (nameID=258)
	let stat = font.require::<Stat>()?;
	stat.design_axis_record
		.as_mut()
		.ok_or("no axes")?
		.axis
		.remove(0); // Weight (nameID=256)
	stat.axis_value_array
		.as_mut()
		.ok_or("no values")?
		.axis_value
		.remove(0); // Thin (nameID=258)
	prune_unused_names(&mut font, &original)?;

	let names = &font.require::<Name>()?.names;
	assert!(!names.iter().any(|n| n.name_id == 256 || n.name_id == 258));

	let original = variation_name_ids(&mut font)?;
	font.remove(Tag::new(b"fvar"));
	font.remove(Tag::new(b"STAT"));
	prune_unused_names(&mut font, &original)?;

	let names = &font.require::<Name>()?.names;
	assert!(!names.iter().any(|n| var_name_ids.contains(&n.name_id)));
	assert!(!font.contains(Tag::new(b"ltag")));
	Ok(())
}

#[test]
fn test_update_name_table_with_registered_axes_ribbi() -> TestResult {
	let cases: &[(Vec<(&str, Option<AxisTriple>)>, Expected<'_>, bool)] = &[
		// Regular
		(
			vec![("wght", pin(400.0))],
			&[
				(win(1), "Test Variable Font"),
				(win(2), "Regular"),
				(win(3), "2.001;GOOG;TestVariableFont-Regular"),
				(win(6), "TestVariableFont-Regular"),
			],
			false,
		),
		// Regular Normal (width axis Normal isn't included since it is elided)
		(
			vec![("wght", pin(400.0)), ("wdth", pin(100.0))],
			&[
				(win(1), "Test Variable Font"),
				(win(2), "Regular"),
				(win(3), "2.001;GOOG;TestVariableFont-Regular"),
				(win(6), "TestVariableFont-Regular"),
			],
			false,
		),
		// Black
		(
			vec![("wght", pin(900.0))],
			&[
				(win(1), "Test Variable Font Black"),
				(win(2), "Regular"),
				(win(3), "2.001;GOOG;TestVariableFont-Black"),
				(win(6), "TestVariableFont-Black"),
				(win(16), "Test Variable Font"),
				(win(17), "Black"),
			],
			true,
		),
		// Thin
		(
			vec![("wght", pin(100.0))],
			&[
				(win(1), "Test Variable Font Thin"),
				(win(2), "Regular"),
				(win(3), "2.001;GOOG;TestVariableFont-Thin"),
				(win(6), "TestVariableFont-Thin"),
				(win(16), "Test Variable Font"),
				(win(17), "Thin"),
			],
			true,
		),
		// Thin Condensed
		(
			vec![("wght", pin(100.0)), ("wdth", pin(79.0))],
			&[
				(win(1), "Test Variable Font Thin Condensed"),
				(win(2), "Regular"),
				(win(3), "2.001;GOOG;TestVariableFont-ThinCondensed"),
				(win(6), "TestVariableFont-ThinCondensed"),
				(win(16), "Test Variable Font"),
				(win(17), "Thin Condensed"),
			],
			true,
		),
		// Condensed with unpinned weights
		(
			vec![("wdth", pin(79.0)), ("wght", range(400.0, 900.0))],
			&[
				(win(1), "Test Variable Font Condensed"),
				(win(2), "Regular"),
				(win(3), "2.001;GOOG;TestVariableFont-Condensed"),
				(win(6), "TestVariableFont-Condensed"),
				(win(16), "Test Variable Font"),
				(win(17), "Condensed"),
			],
			true,
		),
		// Restrict weight and move default, new minimum (500) > old default (400)
		(
			vec![("wght", range(500.0, 900.0))],
			&[
				(win(1), "Test Variable Font Medium"),
				(win(2), "Regular"),
				(win(3), "2.001;GOOG;TestVariableFont-Medium"),
				(win(6), "TestVariableFont-Medium"),
				(win(16), "Test Variable Font"),
				(win(17), "Medium"),
			],
			true,
		),
	];
	for (pairs, expected, is_non_ribbi) in cases {
		let mut font = varfont()?;
		update(&mut font, pairs)?;
		test_name_records(&mut font, expected, *is_non_ribbi, &[WIN])?;
	}
	Ok(())
}

#[test]
fn test_update_name_table_stat_version_1_0() -> TestResult {
	let mut font = varfont()?;
	// `stat.decompile(stat.compile(varfont), varfont)`.
	font.require::<Stat>()?.version = 0x0001_0000;
	let mut font = reload(&mut font)?;

	update(&mut font, &[("wght", pin(100.0))])?;

	let expected: Expected<'_> = &[
		(win(1), "Test Variable Font Thin"),
		(win(2), "Regular"),
		(win(6), "TestVariableFont-Thin"),
		(win(16), "Test Variable Font"),
		(win(17), "Thin"),
	];
	test_name_records(&mut font, expected, true, &[WIN])
}

/// `addMultilingualName(windows=True, mac=True, minNameID=255)` of a name, as `_addName`.
fn add_name(font: &mut Font, string: &str, min: u16) -> TestResult<u16> {
	Ok(font
		.require::<Name>()?
		.add_name(string, &[(1, 0, 0), (3, 1, WIN)], min)?)
}

/// `buildStatTable(varfont, axes)`, for axes with values of format 1:
/// `(tag, name, [(value, name)])`.
fn build_stat_table(font: &mut Font, axes: &[(&str, &str, &[(f64, &str)])]) -> TestResult {
	let mut records = Vec::new();
	let mut values = Vec::new();
	for (i, (tag, name, axis_values)) in axes.iter().enumerate() {
		records.push(ot::AxisRecord {
			axis_tag: Tag::from_str_padded(tag).ok_or("a tag")?,
			axis_name_id: add_name(font, name, 256)?,
			axis_ordering: i as u16,
			..Default::default()
		});
		for (value, name) in *axis_values {
			values.push(Some(Box::new(ot::AxisValue::Format1(
				ot::AxisValueFormat1 {
					axis_index: i as u16,
					flags: 0,
					value_name_id: add_name(font, name, 255)?,
					value: *value,
				},
			))));
		}
	}
	let stat = font.require::<Stat>()?;
	stat.version = 0x0001_0001;
	stat.elided_fallback_name_id = 2;
	stat.design_axis_record = Some(Box::new(ot::AxisRecordArray { axis: records }));
	stat.axis_value_array = Some(Box::new(ot::AxisValueArray { axis_value: values }));
	Ok(())
}

fn name_of(font: &mut Font, name_id: u16) -> TestResult<String> {
	let names = font.require::<Name>()?;
	let record = names
		.get_name(name_id, 3, 1, Some(WIN))
		.ok_or("no such name")?;
	Ok(record.to_unicode()?)
}

#[test]
fn test_updatet_name_table_axis_order() -> TestResult {
	let mut font = varfont()?;
	let wght: (&str, &str, &[(f64, &str)]) = ("wght", "Weight", &[(400.0, "Regular")]);
	let wdth: (&str, &str, &[(f64, &str)]) = ("wdth", "Width", &[(75.0, "Condensed")]);
	let limits: Limits<'_> = &[("wdth", pin(75.0)), ("wght", pin(400.0))];

	build_stat_table(&mut font, &[wght, wdth])?;
	update(&mut font, limits)?;
	assert_eq!(name_of(&mut font, 17)?, "Regular Condensed");

	// Swap the axes so the names get swapped
	build_stat_table(&mut font, &[wdth, wght])?;
	update(&mut font, limits)?;
	assert_eq!(name_of(&mut font, 17)?, "Condensed Regular");
	Ok(())
}

#[test]
fn test_update_name_table_with_multilingual_names() -> TestResult {
	let cases: &[(Vec<(&str, Option<AxisTriple>)>, Expected<'_>, bool)] = &[
		// Regular | Normal
		(
			vec![("wght", pin(400.0))],
			&[(win(1), "Test Variable Font"), (win(2), "Normal")],
			false,
		),
		// Black | Negreta
		(
			vec![("wght", pin(900.0))],
			&[
				(win(1), "Test Variable Font Negreta"),
				(win(2), "Normal"),
				(win(16), "Test Variable Font"),
				(win(17), "Negreta"),
			],
			true,
		),
		// Black Condensed | Negreta Zhuštěné
		(
			vec![("wght", pin(900.0)), ("wdth", pin(79.0))],
			&[
				(win(1), "Test Variable Font Negreta Zhuštěné"),
				(win(2), "Normal"),
				(win(16), "Test Variable Font"),
				(win(17), "Negreta Zhuštěné"),
			],
			true,
		),
	];
	for (pairs, expected, is_non_ribbi) in cases {
		let mut font = varfont()?;
		let name = font.require::<Name>()?;
		// langID 0x405 is the Czech Windows langID
		name.set_name("Test Variable Font".to_owned(), 1, 3, 1, 0x405);
		name.set_name("Normal".to_owned(), 2, 3, 1, 0x405);
		name.set_name("Normal".to_owned(), 261, 3, 1, 0x405); // nameID 261=Regular STAT entry
		name.set_name("Negreta".to_owned(), 266, 3, 1, 0x405); // nameID 266=Black STAT entry
		name.set_name("Zhuštěné".to_owned(), 279, 3, 1, 0x405); // nameID 279=Condensed STAT entry

		update(&mut font, pairs)?;
		// As upstream: the expected keys are of 0x409, so with only 0x405 as the platform they
		// are all skipped, and only the presence of the IDs 16 and 17 is checked.
		test_name_records(&mut font, expected, *is_non_ribbi, &[0x405])?;
	}
	Ok(())
}

#[test]
fn test_update_name_table_missing_axis_values() -> TestResult {
	let mut font = varfont()?;
	let error = update(&mut font, &[("wght", pin(200.0))]).expect_err("no values");
	let message = error.to_string();
	assert!(
		message.contains("Cannot find Axis Values {'wght': 200"),
		"{message}"
	);
	Ok(())
}

#[test]
fn test_update_name_table_missing_stat() -> TestResult {
	let mut font = varfont()?;
	font.remove(Tag::new(b"STAT"));
	let error = update(&mut font, &[("wght", pin(400.0))]).expect_err("no STAT");
	let message = error.to_string();
	assert!(
		message.contains("Cannot update name table since there is no STAT table."),
		"{message}"
	);
	Ok(())
}

fn axis_value(font: &mut Font, index: usize) -> TestResult<&mut ot::AxisValue> {
	Ok(font
		.require::<Stat>()?
		.axis_value_array
		.as_mut()
		.ok_or("no values")?
		.axis_value
		.get_mut(index)
		.ok_or("no value")?
		.as_deref_mut()
		.ok_or("a null value")?)
}

#[test]
fn test_update_name_table_vf_with_italic_attribute() -> TestResult {
	let cases: &[(Vec<(&str, Option<AxisTriple>)>, Expected<'_>, bool)] = &[
		// Regular | Normal
		(
			vec![("wght", pin(400.0))],
			&[
				(win(1), "Test Variable Font"),
				(win(2), "Italic"),
				(win(6), "TestVariableFont-Italic"),
			],
			false,
		),
		// Black Condensed Italic
		(
			vec![("wght", pin(900.0)), ("wdth", pin(79.0))],
			&[
				(win(1), "Test Variable Font Black Condensed"),
				(win(2), "Italic"),
				(win(6), "TestVariableFont-BlackCondensedItalic"),
				(win(16), "Test Variable Font"),
				(win(17), "Black Condensed Italic"),
			],
			true,
		),
	];
	for (pairs, expected, is_non_ribbi) in cases {
		let mut font = varfont()?;
		let font_link_axis_value = axis_value(&mut font, 5)?;
		let ot::AxisValue::Format3(value) = font_link_axis_value else {
			return Err("not a format 3 value".into());
		};
		// Unset ELIDABLE_AXIS_VALUE_NAME flag
		value.flags &= !ELIDABLE_AXIS_VALUE_NAME;
		value.value_name_id = 294; // Roman --> Italic

		update(&mut font, pairs)?;
		test_name_records(&mut font, expected, *is_non_ribbi, &[WIN])?;
	}
	Ok(())
}

#[test]
fn test_update_name_table_format4_axis_values() -> TestResult {
	// format 4 axisValues should dominate the other axisValues
	let mut font = varfont()?;
	font.require::<Name>()?
		.set_name("Dominant Value".to_owned(), 297, 3, 1, WIN);
	let stat = font.require::<Stat>()?;
	let axes = &stat.design_axis_record.as_ref().ok_or("no axes")?.axis;
	let mut records = Vec::new();
	for (tag, value) in [("wght", 900.0), ("wdth", 79.0)] {
		records.push(ot::AxisValueRecord {
			axis_index: axes
				.iter()
				.position(|a| a.axis_tag == tag)
				.ok_or("no axis")? as u16,
			value,
		});
	}
	stat.axis_value_array
		.as_mut()
		.ok_or("no values")?
		.axis_value
		.push(Some(Box::new(ot::AxisValue::Format4(
			ot::AxisValueFormat4 {
				axis_count: None,
				flags: 0,
				value_name_id: 297,
				axis_value_record: records,
			},
		))));

	update(&mut font, &[("wdth", pin(79.0)), ("wght", pin(900.0))])?;
	let expected: Expected<'_> = &[
		(win(1), "Test Variable Font Dominant Value"),
		(win(2), "Regular"),
		(win(16), "Test Variable Font"),
		(win(17), "Dominant Value"),
	];
	test_name_records(&mut font, expected, true, &[WIN])
}

#[test]
fn test_update_name_table_elided_axis_values() -> TestResult {
	let mut font = varfont()?;
	let stat = font.require::<Stat>()?;
	// set ELIDABLE_AXIS_VALUE_NAME flag for all axisValues
	for value in stat
		.axis_value_array
		.as_mut()
		.ok_or("no values")?
		.axis_value
		.iter_mut()
		.flatten()
	{
		match &mut **value {
			ot::AxisValue::Format1(v) => v.flags |= ELIDABLE_AXIS_VALUE_NAME,
			ot::AxisValue::Format2(v) => v.flags |= ELIDABLE_AXIS_VALUE_NAME,
			ot::AxisValue::Format3(v) => v.flags |= ELIDABLE_AXIS_VALUE_NAME,
			ot::AxisValue::Format4(v) => v.flags |= ELIDABLE_AXIS_VALUE_NAME,
			ot::AxisValue::Unknown(_) => {}
		}
	}
	stat.elided_fallback_name_id = 266; // Regular --> Black

	update(&mut font, &[("wght", pin(400.0))])?;
	// Since all axis values are elided, the elided fallback name
	// must be used to construct the style names. Since we
	// changed it to Black, we need both a typoSubFamilyName and
	// the subFamilyName set so it conforms to the RIBBI model.
	let expected: Expected<'_> = &[(win(2), "Regular"), (win(17), "Black")];
	test_name_records(&mut font, expected, true, &[WIN])
}

#[test]
fn test_update_name_table_existing_subfamily_name_is_not_regular() -> TestResult {
	// Check the subFamily name will be set to Regular when we update a name
	// table to a non-RIBBI style and the current subFamily name is a RIBBI
	// style which isn't Regular.
	let mut font = varfont()?;
	font.require::<Name>()?
		.set_name("Bold".to_owned(), 2, 3, 1, WIN); // subFamily Regular --> Bold

	update(&mut font, &[("wght", pin(100.0))])?;
	let expected: Expected<'_> = &[(win(2), "Regular"), (win(17), "Thin")];
	test_name_records(&mut font, expected, true, &[WIN])
}

#[test]
fn test_name_irrelevant_axes() -> TestResult {
	let options = InstancerOptions {
		update_font_names: true,
		..Default::default()
	};
	let location = limits(&[("wght", pin(400.0)), ("wdth", pin(90.0))]);

	// Cannot update name table if not on a named axis value location
	let mut font = varfont()?;
	let error = instantiate_variable_font(&mut font.clone(), &location, &options)
		.expect_err("not on a named location");
	assert!(
		error.to_string().contains("Cannot find Axis Values"),
		"{error}"
	);

	// Now let's make the wdth axis "irrelevant" to naming (no axis values)
	let values = &mut font
		.require::<Stat>()?
		.axis_value_array
		.as_mut()
		.ok_or("no values")?
		.axis_value;
	values.remove(6);
	values.remove(4);
	instantiate_variable_font(&mut font, &location, &options)?;
	Ok(())
}
