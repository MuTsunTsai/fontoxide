//! `InstantiateOTLTest` of fontTools' `Tests/varLib/instancer/instancer_test.py`.
//!
//! The tests of beyond-64k fonts (`test_full_instance_save_beyond64k` and
//! `test_partial_instance_save_beyond64k`) need the tables of those, which are not ported yet.

#![allow(clippy::type_complexity)]

mod common;
mod instancer_common;

use common::*;
use fontoxide_ttlib::ot::conv::ValueRecord;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::{Base, Gdef, Gpos};
use fontoxide_ttlib::{Font, Tag};
use fontoxide_varlib::instancer::{NormalizedLimits, instantiate_otl};
use instancer_common::*;

fn varfont_gdef() -> TestResult<Font> {
	fixture("otl_gdef")
}

fn varfont_gpos() -> TestResult<Font> {
	fixture("otl_gpos")
}

fn varfont_gpos2() -> TestResult<Font> {
	fixture("otl_gpos2")
}

fn varfont_base() -> TestResult<Font> {
	fixture("otl_base")
}

fn caret(font: &mut Font) -> TestResult<&mut ot::CaretValue> {
	Ok(font
		.require::<Gdef>()?
		.lig_caret_list
		.as_mut()
		.ok_or("no LigCaretList")?
		.lig_glyph[0]
		.as_mut()
		.ok_or("no LigGlyph")?
		.caret_value[0]
		.as_mut()
		.ok_or("no CaretValue")?)
}

/// `getBaseCoord`.
fn base_coord(font: &mut Font) -> TestResult<&mut ot::BaseCoord> {
	Ok(font
		.require::<Base>()?
		.horiz_axis
		.as_mut()
		.ok_or("no HorizAxis")?
		.base_script_list
		.as_mut()
		.ok_or("no BaseScriptList")?
		.base_script_record[0]
		.base_script
		.as_mut()
		.ok_or("no BaseScript")?
		.base_values
		.as_mut()
		.ok_or("no BaseValues")?
		.base_coord[0]
		.as_mut()
		.ok_or("no BaseCoord")?)
}

fn lookup(font: &mut Font, index: usize) -> TestResult<&mut ot::Lookup> {
	Ok(font
		.require::<Gpos>()?
		.lookup_list
		.as_mut()
		.ok_or("no LookupList")?
		.lookup[index]
		.as_mut()
		.ok_or("no Lookup")?)
}

fn pair_pos(font: &mut Font, index: usize) -> TestResult<&mut ot::PairPosFormat1> {
	let lookup = lookup(font, index)?;
	assert_eq!(lookup.lookup_type, 2); // PairPos
	match lookup.sub_table[0].as_deref_mut() {
		Some(ot::LookupSubTable::PairPos(ot::PairPos::Format1(st))) => Ok(st),
		_ => Err("not PairPos format 1".into()),
	}
}

fn value1(font: &mut Font, index: usize) -> TestResult<&mut ValueRecord> {
	Ok(pair_pos(font, index)?.pair_set[0]
		.as_mut()
		.ok_or("no PairSet")?
		.pair_value_record[0]
		.value1
		.as_mut()
		.ok_or("no Value1")?)
}

fn base_anchor(font: &mut Font) -> TestResult<&mut ot::Anchor> {
	let lookup = lookup(font, 0)?;
	assert_eq!(lookup.lookup_type, 4); // MarkBasePos
	match lookup.sub_table[0].as_deref_mut() {
		Some(ot::LookupSubTable::MarkBasePos(ot::MarkBasePos::Format1(st))) => {
			Ok(
				st.base_array.as_mut().ok_or("no BaseArray")?.base_record[0].base_anchor[0]
					.as_mut()
					.ok_or("no BaseAnchor")?,
			)
		}
		_ => Err("not MarkBasePos format 1".into()),
	}
}

/// The variation device of a value: format `0x8000`.
fn is_variation_device(device: &Option<Box<ot::Device>>) -> bool {
	device.as_ref().is_some_and(|d| d.delta_format == 0x8000)
}

fn one_axis(axis: &str, value: f64) -> NormalizedLimits {
	normalized(&[(axis, &[value])])
}

fn two_axes(wght: f64, wdth: f64) -> NormalizedLimits {
	normalized(&[("wght", &[wght]), ("wdth", &[wdth])])
}

/// `limits = {"wght": 1.0}`, with `limits["wdth"] = 0.0` if `full`.
fn limits_of(full: bool) -> NormalizedLimits {
	if full {
		two_axes(1.0, 0.0)
	} else {
		one_axis("wght", 1.0)
	}
}

mod instantiate_otl_test {
	use super::*;

	#[test]
	fn test_preserves_ppem_devices() -> TestResult {
		for full in [false, true] {
			for delta in [1, 3, 7] {
				// GDEF
				let mut vf = varfont_gdef()?;
				let hint = build_device(10, delta);
				let ot::CaretValue::Format3(record) = caret(&mut vf)? else {
					return Err("not format 3".into());
				};
				record.device_table = Some(hint.clone());
				let original = record.coordinate;
				instantiate_otl(&mut vf, &limits_of(full))?;
				let ot::CaretValue::Format3(record) = caret(&mut vf)? else {
					return Err("not format 3".into());
				};
				assert_eq!(record.coordinate, original);
				let device = record.device_table.as_ref().ok_or("no device")?;
				assert_eq!(device.delta_format, hint.delta_format);
				assert_eq!(device.delta_value, [delta]);

				// BASE
				let mut vf = varfont_base()?;
				let ot::BaseCoord::Format3(record) = base_coord(&mut vf)? else {
					return Err("not format 3".into());
				};
				record.device_table = Some(hint.clone());
				let original = record.coordinate;
				instantiate_otl(&mut vf, &limits_of(full))?;
				let ot::BaseCoord::Format3(record) = base_coord(&mut vf)? else {
					return Err("not format 3".into());
				};
				assert_eq!(record.coordinate, original);
				let device = record.device_table.as_ref().ok_or("no device")?;
				assert_eq!(device.delta_format, hint.delta_format);
				assert_eq!(device.delta_value, [delta]);

				// GPOS
				let mut vf = varfont_gpos()?;
				let record = value1(&mut vf, 0)?;
				record.x_adv_device = Some(Some(hint.clone()));
				let original = record.x_advance;
				instantiate_otl(&mut vf, &limits_of(full))?;
				let record = value1(&mut vf, 0)?.clone();
				assert_eq!(record.x_advance, original);
				assert_eq!(pair_pos(&mut vf, 0)?.value_format1, 0x44);
				let device = record.x_adv_device.flatten().ok_or("no device")?;
				assert_eq!(device.delta_format, hint.delta_format);
				assert_eq!(device.delta_value, [delta]);
			}
		}
		Ok(())
	}

	#[test]
	fn test_preserves_mixed_anchor_devices() -> TestResult {
		for full in [false, true] {
			for delta in [1, 3, 7] {
				for hint_axis_is_x in [true, false] {
					let mut vf = varfont_gpos2()?;
					let hint = build_device(10, delta);
					let ot::Anchor::Format3(anchor) = base_anchor(&mut vf)? else {
						return Err("not format 3".into());
					};
					if hint_axis_is_x {
						anchor.x_device_table = Some(hint);
					} else {
						anchor.y_device_table = Some(hint);
					}
					instantiate_otl(&mut vf, &limits_of(full))?;
					let ot::Anchor::Format3(anchor) = base_anchor(&mut vf)? else {
						return Err("not format 3".into());
					};
					assert_eq!(anchor.x_coordinate, if hint_axis_is_x { 270 } else { 330 });
					assert_eq!(anchor.y_coordinate, 450);
					let device = if hint_axis_is_x {
						&anchor.x_device_table
					} else {
						&anchor.y_device_table
					};
					assert_eq!(device.as_ref().ok_or("no device")?.delta_value, [delta]);
					if full && !hint_axis_is_x {
						assert!(anchor.x_device_table.is_none());
					}
				}
			}
		}
		Ok(())
	}

	#[test]
	fn test_pin_and_drop_axis_gdef() -> TestResult {
		let cases = [
			("wght", -1.0, 110), // -60
			("wght", 0.0, 170),
			("wght", 0.5, 200),  // +30
			("wght", 1.0, 230),  // +60
			("wdth", -1.0, 160), // -10
			("wdth", -0.3, 167), // -3
			("wdth", 0.0, 170),
			("wdth", 1.0, 180), // +10
		];
		for (axis, value, expected) in cases {
			let mut vf = varfont_gdef()?;
			assert!(vf.contains(Tag::new(b"GDEF")));

			instantiate_otl(&mut vf, &one_axis(axis, value))?;

			assert!(vf.contains(Tag::new(b"GDEF")));
			let gdef = vf.require::<Gdef>()?;
			assert_eq!(gdef.version, 0x0001_0003);
			assert!(gdef.var_store.is_some());
			assert!(gdef.lig_caret_list.is_some());
			let ot::CaretValue::Format3(caret_value) = caret(&mut vf)? else {
				return Err("not format 3".into());
			};
			assert!(is_variation_device(&caret_value.device_table));
			assert_eq!(caret_value.coordinate, expected);
		}
		Ok(())
	}

	#[test]
	fn test_full_instance_gdef() -> TestResult {
		let cases = [
			(-1.0, -1.0, 100), // -60 - 10
			(-1.0, 0.0, 110),  // -60
			(-1.0, 1.0, 120),  // -60 + 10
			(0.0, -1.0, 160),  // -10
			(0.0, 0.0, 170),
			(0.0, 1.0, 180),  // +10
			(1.0, -1.0, 220), // +60 - 10
			(1.0, 0.0, 230),  // +60
			(1.0, 1.0, 240),  // +60 + 10
		];
		for (wght, wdth, expected) in cases {
			let mut vf = varfont_gdef()?;
			assert!(vf.contains(Tag::new(b"GDEF")));

			instantiate_otl(&mut vf, &two_axes(wght, wdth))?;

			assert!(vf.contains(Tag::new(b"GDEF")));
			let gdef = vf.require::<Gdef>()?;
			assert_eq!(gdef.version, 0x0001_0000);
			assert!(gdef.var_store.is_none());
			assert!(gdef.lig_caret_list.is_some());
			let ot::CaretValue::Format1(caret_value) = caret(&mut vf)? else {
				return Err("not format 1".into());
			};
			assert_eq!(caret_value.coordinate, expected);
		}
		Ok(())
	}

	#[test]
	fn test_pin_and_drop_axis_gpos_kern() -> TestResult {
		let cases = [
			("wght", -1.0, -85), // +25
			("wght", 0.0, -110),
			("wght", 1.0, -135),  // -25
			("wdth", -1.0, -105), // +5
			("wdth", 0.0, -110),
			("wdth", 1.0, -115), // -5
		];
		for (axis, value, expected) in cases {
			let mut vf = varfont_gpos()?;
			assert!(vf.contains(Tag::new(b"GDEF")));
			assert!(vf.contains(Tag::new(b"GPOS")));

			instantiate_otl(&mut vf, &one_axis(axis, value))?;

			let gdef = vf.require::<Gdef>()?;
			assert_eq!(gdef.version, 0x0001_0003);
			assert!(gdef.var_store.is_some());

			let value_rec1 = value1(&mut vf, 0)?;
			assert!(is_variation_device(
				value_rec1.x_adv_device.as_ref().ok_or("no device")?
			));
			assert_eq!(value_rec1.x_advance, Some(expected));
		}
		Ok(())
	}

	#[test]
	fn test_full_instance_gpos_kern() -> TestResult {
		let cases = [
			(-1.0, -1.0, -80), // +25 + 5
			(-1.0, 0.0, -85),  // +25
			(-1.0, 1.0, -90),  // +25 - 5
			(0.0, -1.0, -105), // +5
			(0.0, 0.0, -110),
			(0.0, 1.0, -115),  // -5
			(1.0, -1.0, -130), // -25 + 5
			(1.0, 0.0, -135),  // -25
			(1.0, 1.0, -140),  // -25 - 5
		];
		for (wght, wdth, expected) in cases {
			let mut vf = varfont_gpos()?;
			assert!(vf.contains(Tag::new(b"GDEF")));
			assert!(vf.contains(Tag::new(b"GPOS")));

			instantiate_otl(&mut vf, &two_axes(wght, wdth))?;

			assert!(!vf.contains(Tag::new(b"GDEF")));
			let value_rec1 = value1(&mut vf, 0)?;
			assert!(value_rec1.x_adv_device.is_none());
			assert_eq!(value_rec1.x_advance, Some(expected));
		}
		Ok(())
	}

	#[test]
	fn test_pin_and_drop_axis_gpos_mark_and_kern() -> TestResult {
		let cases = [
			("wght", -1.0, (210, -85)), // -60, +25
			("wght", 0.0, (270, -110)),
			("wght", 0.5, (300, -122)),  // +30, -12
			("wght", 1.0, (330, -135)),  // +60, -25
			("wdth", -1.0, (260, -105)), // -10, +5
			("wdth", -0.3, (267, -108)), // -3, +2
			("wdth", 0.0, (270, -110)),
			("wdth", 1.0, (280, -115)), // +10, -5
		];
		for (axis, value, (v1, v2)) in cases {
			let mut vf = varfont_gpos2()?;
			assert!(vf.contains(Tag::new(b"GDEF")));
			assert!(vf.contains(Tag::new(b"GPOS")));

			instantiate_otl(&mut vf, &one_axis(axis, value))?;

			let gdef = vf.require::<Gdef>()?;
			assert_eq!(gdef.version, 0x0001_0003);
			assert!(gdef.var_store.is_some());
			assert!(gdef.glyph_class_def.is_some());

			let ot::Anchor::Format3(anchor) = base_anchor(&mut vf)? else {
				return Err("not format 3".into());
			};
			assert!(is_variation_device(&anchor.x_device_table));
			assert!(anchor.y_device_table.is_none());
			assert_eq!(anchor.x_coordinate, v1);
			assert_eq!(anchor.y_coordinate, 450);

			let value_rec1 = value1(&mut vf, 1)?;
			assert!(is_variation_device(
				value_rec1.x_adv_device.as_ref().ok_or("no device")?
			));
			assert_eq!(value_rec1.x_advance, Some(v2));
		}
		Ok(())
	}

	#[test]
	fn test_full_instance_gpos_mark_and_kern() -> TestResult {
		let cases = [
			(-1.0, -1.0, (200, -80)), // -60 - 10, +25 + 5
			(-1.0, 0.0, (210, -85)),  // -60, +25
			(-1.0, 1.0, (220, -90)),  // -60 + 10, +25 - 5
			(0.0, -1.0, (260, -105)), // -10, +5
			(0.0, 0.0, (270, -110)),
			(0.0, 1.0, (280, -115)),  // +10, -5
			(1.0, -1.0, (320, -130)), // +60 - 10, -25 + 5
			(1.0, 0.0, (330, -135)),  // +60, -25
			(1.0, 1.0, (340, -140)),  // +60 + 10, -25 - 5
		];
		for (wght, wdth, (v1, v2)) in cases {
			let mut vf = varfont_gpos2()?;
			assert!(vf.contains(Tag::new(b"GDEF")));
			assert!(vf.contains(Tag::new(b"GPOS")));

			instantiate_otl(&mut vf, &two_axes(wght, wdth))?;

			let gdef = vf.require::<Gdef>()?;
			assert_eq!(gdef.version, 0x0001_0000);
			assert!(gdef.var_store.is_none());
			assert!(gdef.glyph_class_def.is_some());

			let ot::Anchor::Format1(anchor) = base_anchor(&mut vf)? else {
				return Err("not format 1".into());
			};
			assert_eq!(anchor.x_coordinate, v1);
			assert_eq!(anchor.y_coordinate, 450);

			let value_rec1 = value1(&mut vf, 1)?;
			assert!(value_rec1.x_adv_device.is_none());
			assert_eq!(value_rec1.x_advance, Some(v2));
		}
		Ok(())
	}

	#[test]
	fn test_gpos_value_record_x_adv_device_wtihout_x_advance() -> TestResult {
		// Test VF contains a PairPos adjustment in which the default instance has no XAdvance
		// but there are deltas in XAdvDevice (VariationIndex).
		let mut vf = font_from_ttx(&instancer_data("PartialInstancerTest4-VF.ttx"))?;
		assert_eq!(pair_pos(&mut vf, 0)?.value_format1, 0x40);
		let value_rec1 = value1(&mut vf, 0)?;
		assert!(value_rec1.x_advance.is_none());
		let device = value_rec1
			.x_adv_device
			.clone()
			.flatten()
			.ok_or("no device")?;
		assert_eq!(device.delta_format, 0x8000);
		let (outer, inner) = (usize::from(device.start_size), usize::from(device.end_size));
		let gdef = vf.require::<Gdef>()?;
		let data = gdef.var_store.as_ref().ok_or("no VarStore")?.var_data[outer]
			.as_ref()
			.ok_or("no VarData")?;
		assert_eq!(data.item[inner], [-50]);

		// check that MutatorMerger for ValueRecord doesn't raise AttributeError when XAdvDevice
		// is present but there's no corresponding XAdvance.
		instantiate_otl(&mut vf, &one_axis("wght", 0.5))?;

		assert_eq!(pair_pos(&mut vf, 0)?.value_format1, 0x4);
		let value_rec1 = value1(&mut vf, 0)?;
		assert!(value_rec1.x_adv_device.is_none());
		assert_eq!(value_rec1.x_advance, Some(-25));
		Ok(())
	}

	#[test]
	fn test_pin_and_drop_axis_base() -> TestResult {
		let cases = [
			("wght", 0.0, -120),
			("wght", 0.5, -110),
			("wght", 1.0, -100),
			("wdth", 1.0, -110),
			("wdth", 0.5, -115),
		];
		for (axis, value, expected) in cases {
			let mut vf = varfont_base()?;

			instantiate_otl(&mut vf, &one_axis(axis, value))?;

			let base = vf.require::<Base>()?;
			assert_eq!(base.version, 0x0001_0001);
			assert!(base.var_store.is_some());
			let ot::BaseCoord::Format3(coord) = base_coord(&mut vf)? else {
				return Err("not format 3".into());
			};
			assert!(is_variation_device(&coord.device_table));
			assert_eq!(coord.coordinate, expected);
		}
		Ok(())
	}

	#[test]
	fn test_full_instance_base() -> TestResult {
		let cases = [
			(0.0, 0.0, -120),
			(1.0, 0.0, -100),
			(0.0, 1.0, -110),
			(1.0, 1.0, -90),
		];
		for (wght, wdth, expected) in cases {
			let mut vf = varfont_base()?;

			instantiate_otl(&mut vf, &two_axes(wght, wdth))?;

			let base = vf.require::<Base>()?;
			assert_eq!(base.version, 0x0001_0000);
			assert!(base.var_store.is_none());
			let ot::BaseCoord::Format1(coord) = base_coord(&mut vf)? else {
				return Err("not format 1".into());
			};
			assert_eq!(coord.coordinate, expected);
		}
		Ok(())
	}
}
