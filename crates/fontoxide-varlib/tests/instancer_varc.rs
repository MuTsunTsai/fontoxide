//! Port of fontTools' `Tests/varLib/instancer/varc_test.py`.
//!
//! The fonts of the `varc_font` fixture are made by
//! `tools/reference/instancer_module_fixtures.py`. Each test runs on both, as the
//! parametrized fixture does.

#![allow(clippy::type_complexity)]

mod common;
mod module_support;

use common::{limits, pin, range, triple};
use fontoxide_misc::fixed_tools::float_to_fixed_to_float;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::otl::Varc;
use fontoxide_ttlib::tables::{Fvar, Gvar};
use fontoxide_ttlib::{Font, Tag};
use fontoxide_varlib::instancer::{AxisTriple, InstancerOptions, instantiate_variable_font};
use module_support::*;

const VARC: Tag = Tag::new(b"VARC");
const FVAR: Tag = Tag::new(b"fvar");

type Pairs<'a> = &'a [(&'a str, Option<AxisTriple>)];

/// Both of the parameters of the `varc_font` fixture.
const VARC_FONTS: [&str; 2] = ["varc_no_avar", "varc_avar2"];

/// `_roundtrip(instantiateVariableFont(font, limits))`.
fn instance(font: &Font, pairs: Pairs<'_>) -> TestResult<Font> {
	let mut font = font.clone();
	instantiate_variable_font(&mut font, &limits(pairs), &InstancerOptions::default())?;
	roundtrip(&mut font)
}

/// `_draw`.
fn draw_glyph(
	font: &mut Font,
	location: &Location,
) -> TestResult<Vec<(&'static str, Vec<(f64, f64)>)>> {
	draw(font, "uniAC01", location)
}

fn axis_tags(font: &mut Font) -> TestResult<Vec<String>> {
	Ok(font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| a.tag_text())
		.collect())
}

fn axis_triple(font: &mut Font, tag: &str) -> TestResult<(f64, f64, f64)> {
	let fvar = font.require::<Fvar>()?;
	let axis = fvar
		.axes
		.iter()
		.find(|a| a.tag_text() == tag)
		.ok_or("no axis")?;
	Ok((axis.min_value, axis.default_value, axis.max_value))
}

fn assert_rejected(result: fontoxide_ttlib::Result<()>) {
	let message = result.expect_err("NotImplementedError").to_string();
	assert!(
		message.contains("Instancing across VarComponent axes"),
		"{message}"
	);
}

/// Asserts that two recordings are the same within 0.5 units.
fn assert_close_drawings(
	actual: &[(&'static str, Vec<(f64, f64)>)],
	expected: &[(&'static str, Vec<(f64, f64)>)],
) {
	assert_eq!(actual.len(), expected.len());
	for ((actual_op, actual_points), (expected_op, expected_points)) in actual.iter().zip(expected)
	{
		assert_eq!(actual_op, expected_op);
		assert_eq!(actual_points.len(), expected_points.len());
		for (a, e) in actual_points.iter().zip(expected_points) {
			// Instancing expands and rounds inferred gvar deltas.
			assert!(
				(a.0 - e.0).abs() <= 0.5 && (a.1 - e.1).abs() <= 0.5,
				"{a:?}, {e:?}"
			);
		}
	}
}

/// The locations of the tests that compare fonts.
fn compared_locations() -> [Location; 3] {
	[
		loc(&[]),
		loc(&[("wght", 840.3), ("COND", 0.75)]),
		loc(&[("0000", -0.5)]),
	]
}

fn assert_same_varc_and_fvar(result: &mut Font, expected: &mut Font) -> TestResult {
	assert_eq!(result.table_data(VARC)?, expected.table_data(VARC)?);
	assert_eq!(result.table_data(FVAR)?, expected.table_data(FVAR)?);
	Ok(())
}

#[test]
fn test_instance_unrelated_axis() -> TestResult {
	for name in VARC_FONTS {
		for limit in [(0.5, 0.5, 0.5), (0.0, 0.5, 1.0)] {
			let mut varc_font = fixture(name)?;
			let pinned = limit.0 == limit.2;
			let value = if pinned {
				pin(0.5)
			} else {
				triple(limit.0, limit.1, limit.2)
			};
			let mut result = instance(&varc_font, &[("DUMY", value)])?;
			assert!(result.contains(VARC));
			assert_eq!(
				result.require::<Fvar>()?.axes.len(),
				if pinned { 9 } else { 10 }
			);
			if !pinned {
				let axis = &result.require::<Fvar>()?.axes[0];
				assert_eq!((axis.min_value, axis.default_value, axis.max_value), limit);
			}

			for sample in 0..8 {
				let location = loc(&[
					("DUMY", if !pinned && sample & 1 != 0 { 1.0 } else { 0.5 }),
					("wght", if sample & 2 != 0 { 840.3 } else { 356.5 }),
					("opsz", if sample & 1 != 0 { 1.0 } else { 0.0 }),
					("COND", if sample & 4 != 0 { 1.0 } else { 0.0 }),
				]);
				let expected = draw_glyph(&mut varc_font, &location)?;
				let actual = draw_glyph(&mut result, &location)?;
				assert!(!expected.is_empty());
				assert_close_drawings(&actual, &expected);
			}
		}
	}
	Ok(())
}

#[test]
fn test_reject_referenced_axis() -> TestResult {
	for name in VARC_FONTS {
		for tag in ["wght", "opsz", "0000", "COND"] {
			for restrict_range in [false, true] {
				let mut varc_font = fixture(name)?;
				let (minimum, default, maximum) = axis_triple(&mut varc_font, tag)?;
				let limit = if restrict_range {
					triple(minimum, default, (default + maximum) / 2.0)
				} else {
					pin(default)
				};
				assert_rejected(instantiate_variable_font(
					&mut varc_font,
					&limits(&[(tag, limit)]),
					&InstancerOptions::default(),
				));
			}
		}
	}
	Ok(())
}

#[test]
fn test_reject_full_instance() -> TestResult {
	for name in VARC_FONTS {
		let mut varc_font = fixture(name)?;
		assert_rejected(instantiate_variable_font(
			&mut varc_font,
			&limits(&[]),
			&InstancerOptions {
				make_static: true,
				..Default::default()
			},
		));
	}
	Ok(())
}

#[test]
fn test_original_ranges_preserve_referenced_axes() -> TestResult {
	for name in VARC_FONTS {
		for tag in [Some("wght"), Some("opsz"), Some("0000"), Some("COND"), None] {
			for pin_unrelated in [false, true] {
				let mut varc_font = fixture(name)?;
				let mut pairs: Vec<(String, Option<AxisTriple>)> = Vec::new();
				for axis in axis_tags(&mut varc_font)? {
					if tag.is_none() || tag == Some(axis.as_str()) {
						let (a, b, c) = axis_triple(&mut varc_font, &axis)?;
						pairs.push((axis, triple(a, b, c)));
					}
				}
				let baseline: Vec<(&str, Option<AxisTriple>)> = if pin_unrelated {
					vec![("DUMY", pin(0.5))]
				} else {
					vec![]
				};
				pairs.retain(|(t, _)| !(pin_unrelated && t == "DUMY"));
				pairs.extend(baseline.iter().map(|(t, l)| ((*t).to_owned(), *l)));
				let pairs: Vec<(&str, Option<AxisTriple>)> =
					pairs.iter().map(|(t, l)| (t.as_str(), *l)).collect();

				let mut expected = instance(&varc_font, &baseline)?;
				let mut result = instance(&varc_font, &pairs)?;
				assert_same_varc_and_fvar(&mut result, &mut expected)?;
				for location in compared_locations() {
					assert_eq!(
						draw_glyph(&mut result, &location)?,
						draw_glyph(&mut expected, &location)?
					);
				}
			}
		}
	}
	Ok(())
}

#[test]
fn test_original_decimal_weight_range() -> TestResult {
	for name in VARC_FONTS {
		for limit in [range(356.5, 840.3), triple(356.5, 356.5, 840.3)] {
			let varc_font = fixture(name)?;
			let mut expected = instance(&varc_font, &[])?;
			let mut result = instance(&varc_font, &[("wght", limit)])?;
			assert_same_varc_and_fvar(&mut result, &mut expected)?;
			for location in compared_locations() {
				assert_eq!(
					draw_glyph(&mut result, &location)?,
					draw_glyph(&mut expected, &location)?
				);
			}
		}
	}
	Ok(())
}

#[test]
fn test_original_range_compares_both_sides_at_fixed_precision() -> TestResult {
	for name in VARC_FONTS {
		for quantize_fvar in [false, true] {
			let mut varc_font = fixture(name)?;
			let decimal_range = (-0.3, 0.1, 0.3);
			let fixed_range = (
				float_to_fixed_to_float(decimal_range.0, 16),
				float_to_fixed_to_float(decimal_range.1, 16),
				float_to_fixed_to_float(decimal_range.2, 16),
			);
			let (in_fvar, limit) = if quantize_fvar {
				(fixed_range, decimal_range)
			} else {
				(decimal_range, fixed_range)
			};
			let fvar = varc_font.require::<Fvar>()?;
			let axis = fvar
				.axes
				.iter_mut()
				.find(|a| a.tag_text() == "COND")
				.ok_or("no axis")?;
			(axis.min_value, axis.default_value, axis.max_value) = in_fvar;

			let mut expected = instance(&varc_font, &[])?;
			let mut result = instance(&varc_font, &[("COND", triple(limit.0, limit.1, limit.2))])?;
			assert_same_varc_and_fvar(&mut result, &mut expected)?;
		}
	}
	Ok(())
}

#[test]
fn test_reject_range_changes_at_fixed_precision() -> TestResult {
	for name in VARC_FONTS {
		for index in 0..3 {
			let mut varc_font = fixture(name)?;
			let (a, b, c) = axis_triple(&mut varc_font, "COND")?;
			let mut limit = [a, b, c];
			limit[index] += (if index == 2 { -1.0 } else { 1.0 }) / f64::from(1 << 16);
			assert_rejected(instantiate_variable_font(
				&mut varc_font,
				&limits(&[("COND", triple(limit[0], limit[1], limit[2]))]),
				&InstancerOptions::default(),
			));
		}
	}
	Ok(())
}

#[test]
fn test_degenerate_axis_is_still_removed_when_explicitly_pinned() -> TestResult {
	for name in VARC_FONTS {
		let mut varc_font = fixture(name)?;
		let axis = &mut varc_font.require::<Fvar>()?.axes[0];
		assert_eq!(axis.tag_text(), "DUMY");
		axis.min_value = 0.0;
		axis.default_value = 0.0;
		axis.max_value = 0.0;
		let mut result = instance(&varc_font, &[("DUMY", None)])?;
		assert!(!axis_tags(&mut result)?.iter().any(|t| t == "DUMY"));
	}
	Ok(())
}

#[test]
fn test_nested_conditions() -> TestResult {
	for name in VARC_FONTS {
		let mut varc_font = fixture(name)?;
		let before = draw_glyph(&mut varc_font, &loc(&[("wght", 840.3), ("COND", 0.0)]))?;
		let after = draw_glyph(&mut varc_font, &loc(&[("wght", 840.3), ("COND", 1.0)]))?;
		assert_ne!(after, before);
	}
	Ok(())
}

fn conditions(font: &mut Font) -> TestResult<&mut Vec<Option<Box<ot::ConditionTable>>>> {
	Ok(&mut font
		.require::<Varc>()?
		.table
		.condition_list
		.as_mut()
		.ok_or("no ConditionList")?
		.condition_table)
}

#[test]
fn test_null_conditions() -> TestResult {
	for name in VARC_FONTS {
		for format in [None, Some(3), Some(4), Some(5)] {
			let mut varc_font = fixture(name)?;
			let condition = match format {
				None => None,
				Some(3) => Some(ot::ConditionTable::Format3(ot::ConditionTableFormat3 {
					condition_count: None,
					condition_table: vec![None],
				})),
				Some(4) => Some(ot::ConditionTable::Format4(ot::ConditionTableFormat4 {
					condition_count: None,
					condition_table: vec![None],
				})),
				_ => Some(ot::ConditionTable::Format5(ot::ConditionTableFormat5 {
					condition_table: None,
				})),
			};
			conditions(&mut varc_font)?[0] = condition.map(Box::new);
			let mut varc_font = roundtrip(&mut varc_font)?;
			let mut result = instance(&varc_font, &[("DUMY", pin(0.5))])?;
			let condition = conditions(&mut result)?[0].as_deref();
			match (format, condition) {
				(None, None) => {}
				(Some(3), Some(ot::ConditionTable::Format3(c))) => {
					assert_eq!(c.condition_table, vec![None]);
				}
				(Some(4), Some(ot::ConditionTable::Format4(c))) => {
					assert_eq!(c.condition_table, vec![None]);
				}
				(Some(5), Some(ot::ConditionTable::Format5(c))) => {
					assert_eq!(c.condition_table, None);
				}
				(format, condition) => panic!("{format:?}: {condition:?}"),
			}
			let location = loc(&[("DUMY", 0.5)]);
			assert_eq!(
				draw_glyph(&mut result, &location)?,
				draw_glyph(&mut varc_font, &location)?
			);
		}
	}
	Ok(())
}

#[test]
fn test_remap_negated_condition() -> TestResult {
	for name in VARC_FONTS {
		let mut varc_font = fixture(name)?;
		let list = conditions(&mut varc_font)?;
		let inner = list[0].take();
		list[0] = Some(Box::new(ot::ConditionTable::Format5(
			ot::ConditionTableFormat5 {
				condition_table: inner,
			},
		)));
		let mut result = instance(&varc_font, &[("DUMY", pin(0.5))])?;
		let axes = axis_tags(&mut result)?;
		let negated = conditions(&mut result)?[0].as_deref();
		let Some(ot::ConditionTable::Format5(negated)) = negated else {
			return Err("not format 5".into());
		};
		let Some(ot::ConditionTable::Format3(all)) = negated.condition_table.as_deref() else {
			return Err("not format 3".into());
		};
		let [weight, either] = &all.condition_table[..] else {
			return Err("not two conditions".into());
		};
		let Some(ot::ConditionTable::Format1(weight)) = weight.as_deref() else {
			return Err("not format 1".into());
		};
		assert_eq!(axes[usize::from(weight.axis_index)], "wght");
		let Some(ot::ConditionTable::Format4(either)) = either.as_deref() else {
			return Err("not format 4".into());
		};
		let tags: Vec<&str> = either
			.condition_table
			.iter()
			.map(|c| match c.as_deref() {
				Some(ot::ConditionTable::Format1(c)) => axes[usize::from(c.axis_index)].as_str(),
				c => panic!("{c:?}"),
			})
			.collect();
		assert_eq!(tags, ["COND", "COND"]);
	}
	Ok(())
}

#[test]
fn test_avar2_preserves_component_internal_variation() -> TestResult {
	// Drive hidden axis 0000 from DUMY through avar2. A hidden axis has no
	// identity term, so its font-level reachable range is just the delta range,
	// here [0, 1]. A VARC component still overrides 0000 to about -0.54 and
	// therefore reaches the negative gvar tuple added by the fixture.
	let mut varc_font = fixture("varc_avar2_driven")?;

	let location = loc(&[("DUMY", 0.0), ("wght", 356.5), ("opsz", 0.0), ("COND", 0.0)]);
	let expected = draw_glyph(&mut varc_font, &location)?;
	assert!(!expected.is_empty());
	// DUMY already has this full range, so this instancing request is a no-op.
	let mut result = instance(&varc_font, &[("DUMY", triple(-1.0, 0.0, 1.0))])?;
	let actual = draw_glyph(&mut result, &location)?;

	let gid = result.glyph_id("glyph00003")?;
	let gvar = result.require::<Gvar>()?;
	assert!(
		gvar.get(gid)
			.ok_or("no variations")?
			.iter()
			.any(|v| v.axes.get("0000") == Some(&(-1.0, -0.5, -0.25)))
	);
	assert_close_drawings(&actual, &expected);
	Ok(())
}
