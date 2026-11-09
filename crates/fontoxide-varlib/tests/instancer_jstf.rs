//! Port of fontTools' `Tests/varLib/instancer/jstf_test.py`.
//!
//! The font of the `font` fixture is made by
//! `tools/reference/instancer_module_fixtures.py`.

#![allow(clippy::type_complexity)]

mod common;
mod module_support;

use common::{instantiate, pin};
use fontoxide_misc::round_tools::ot_round;
use fontoxide_subset::{Item, Options, Subsetter};
use fontoxide_ttlib::ot::conv::ValueRecord;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::types::{Anchor, CursivePos, LookupSubTable, SinglePos};
use fontoxide_ttlib::ot::var_store;
use fontoxide_ttlib::tables::{Fvar, Gdef, Jstf};
use fontoxide_ttlib::var::VarStoreInstancer;
use fontoxide_ttlib::{Font, Tag};
use fontoxide_varlib::instancer::AxisTriple;
use module_support::*;

const JSTF: Tag = Tag::new(b"JSTF");

fn font_fixture() -> TestResult<Font> {
	fixture("jstf")
}

/// `roundtrip(instantiateVariableFont(font, limits))`.
fn instance(font: &Font, pairs: &[(&str, Option<AxisTriple>)]) -> TestResult<Font> {
	let mut font = font.clone();
	instantiate(&mut font, pairs)?;
	roundtrip(&mut font)
}

/// `values`: calls `f` with the value records and anchors of the JSTF table.
fn for_values(
	font: &mut Font,
	f: &mut dyn FnMut(&mut ValueRecord, &mut Anchor) -> TestResult,
) -> TestResult {
	let jstf = font.require::<Jstf>()?;
	let script = jstf.table.jstf_script_record[0]
		.jstf_script
		.as_mut()
		.ok_or("no JstfScript")?;
	let mut langs = vec![script.def_jstf_lang_sys.as_mut().ok_or("no JstfLangSys")?];
	langs.push(
		script.jstf_lang_sys_record[0]
			.jstf_lang_sys
			.as_mut()
			.ok_or("no JstfLangSys")?,
	);
	for lang in langs {
		let p = lang.jstf_priority[0].as_mut().ok_or("no JstfPriority")?;
		for max in [&mut p.shrinkage_jstf_max, &mut p.extension_jstf_max] {
			let max = max.as_mut().ok_or("no JstfMax")?;
			let [single, cursive, ..] = &mut max.lookup[..] else {
				return Err("not two lookups".into());
			};
			let single = &mut single.as_mut().ok_or("no Lookup")?.sub_table[0];
			let cursive = &mut cursive.as_mut().ok_or("no Lookup")?.sub_table[0];
			let Some(LookupSubTable::SinglePos(SinglePos::Format1(single))) = single.as_deref_mut()
			else {
				return Err("not a SinglePos of format 1".into());
			};
			let Some(LookupSubTable::CursivePos(CursivePos::Format1(cursive))) =
				cursive.as_deref_mut()
			else {
				return Err("not a CursivePos".into());
			};
			let value = single.value.as_mut().ok_or("no Value")?;
			let anchor = cursive.entry_exit_record[0]
				.entry_anchor
				.as_mut()
				.ok_or("no EntryAnchor")?;
			f(value, anchor)?;
		}
	}
	Ok(())
}

/// `check_values`.
fn check_values(font: &mut Font, weight: f64, width: f64) -> TestResult {
	let axes: Vec<String> = match font.get::<Fvar>()? {
		Some(fvar) => fvar.axes.iter().map(|a| a.tag_text()).collect(),
		None => Vec::new(),
	};
	let store = match font.get::<Gdef>()? {
		Some(gdef) => gdef.var_store.clone(),
		None => None,
	};
	let instancer = store
		.as_deref()
		.map(|s| VarStoreInstancer::new(Some(s), &axes, loc(&[("wdth", width)])));
	let check =
		|value: i16, device: Option<&ot::Device>, base: f64, dw: f64, dd: f64| -> TestResult {
			let mut value = f64::from(value);
			if let Some(device) = device {
				let instancer = instancer.as_ref().ok_or("no store")?;
				value += instancer
					.get((u64::from(device.start_size) << 16) | u64::from(device.end_size))?;
			}
			assert_eq!(value, base + ot_round(weight * dw) as f64 + width * dd);
			Ok(())
		};
	for_values(font, &mut |record, anchor| {
		assert_eq!(record.x_advance, Some(30));
		check(
			record.x_placement.ok_or("no XPlacement")?,
			record.x_pla_device.as_ref().and_then(|d| d.as_deref()),
			10.0,
			10.0,
			20.0,
		)?;
		check(
			record.y_placement.ok_or("no YPlacement")?,
			record.y_pla_device.as_ref().and_then(|d| d.as_deref()),
			20.0,
			-20.0,
			30.0,
		)?;
		let (x, y, x_device, y_device) = match anchor {
			Anchor::Format1(a) => (a.x_coordinate, a.y_coordinate, None, None),
			Anchor::Format3(a) => (
				a.x_coordinate,
				a.y_coordinate,
				a.x_device_table.as_deref(),
				a.y_device_table.as_deref(),
			),
			_ => return Err("an unexpected anchor format".into()),
		};
		check(x, x_device, 100.0, 30.0, -10.0)?;
		check(y, y_device, 200.0, 40.0, -20.0)
	})
}

#[test]
fn test_full_jstf_instance() -> TestResult {
	for weight in [0.0, 0.5, 1.0] {
		let font = font_fixture()?;
		let mut result = instance(&font, &[("wght", pin(weight)), ("wdth", pin(0.0))])?;
		assert!(!result.contains(Tag::new(b"fvar")) && !result.contains(Tag::new(b"GDEF")));
		check_values(&mut result, weight, 0.0)?;
		for_values(&mut result, &mut |value, anchor| {
			assert!(value.x_pla_device.is_none() && value.y_pla_device.is_none());
			assert!(value.x_adv_device.is_none() && value.y_adv_device.is_none());
			// The other formats have device table attributes.
			assert!(matches!(anchor, Anchor::Format1(_)), "{anchor:?}");
			Ok(())
		})?;
	}
	Ok(())
}

#[test]
fn test_partial_jstf_instance_remaps_and_composes() -> TestResult {
	for weight in [0.0, 0.5, 1.0] {
		let font = font_fixture()?;
		let mut result = instance(&font, &[("wght", pin(weight))])?;
		for width in [0.0, 0.5, 1.0] {
			check_values(&mut result, weight, width)?;
			let mut composed = instance(&result, &[("wdth", pin(width))])?;
			let mut direct = instance(&font, &[("wght", pin(weight)), ("wdth", pin(width))])?;
			assert_eq!(composed.table_data(JSTF)?, direct.table_data(JSTF)?);
		}
	}
	Ok(())
}

#[test]
fn test_optimizer_remaps_jstf_devices() -> TestResult {
	let mut font = font_fixture()?;
	// `main([source, dest])` and `TTFont(dest)`: through files in fontTools.
	let mut source = roundtrip(&mut font)?;
	let gdef = source.require::<Gdef>()?;
	let store = gdef.var_store.as_mut().ok_or("no VarStore")?;
	let map = var_store::optimize(store, true)?.into_iter().collect();
	fontoxide_subset::varstore::remap_device_varidxes(&mut gdef.table, &map)?;
	let jstf = source.require::<Jstf>()?;
	fontoxide_subset::varstore::remap_device_varidxes(&mut jstf.table, &map)?;
	let mut dest = roundtrip(&mut source)?;

	instantiate(&mut dest, &[("wght", pin(1.0))])?;
	check_values(&mut dest, 1.0, 0.5)
}

#[test]
fn test_subset_preserves_passthrough_jstf_variation_rows() -> TestResult {
	let mut font = font_fixture()?;
	let mut options = Options::default();
	let jstf = Item::from("JSTF");
	options.drop_tables.retain(|t| *t != jstf);
	options.passthrough_tables = true;
	options.retain_gids = true;
	options.glyph_names = true;
	let order = font.glyph_order()?;
	let names: Vec<String> = (0..order.len() as u32)
		.map(|g| order.name(g).into_owned())
		.collect();
	let names: Vec<&str> = names.iter().map(String::as_str).collect();
	let mut subsetter = Subsetter::new(options);
	subsetter.populate(&names, &[], &[], "");
	subsetter.subset(&mut font)?;
	let saved = roundtrip(&mut font)?;
	let mut result = saved.clone();
	instantiate(&mut result, &[("wght", pin(1.0))])?;
	check_values(&mut result, 1.0, 0.5)
}

/// `buildDevice({10: delta})`.
fn build_device(delta: i64) -> ot::Device {
	ot::Device {
		start_size: 10,
		end_size: 10,
		// `_deviceDeltaFormat`.
		delta_format: if (-2..=1).contains(&delta) {
			1
		} else if (-8..=7).contains(&delta) {
			2
		} else {
			3
		},
		delta_value: vec![delta],
	}
}

#[test]
fn test_jstf_preserves_ppem_devices() -> TestResult {
	for full in [false, true] {
		for delta in [1, 3, 7] {
			let mut font = font_fixture()?;
			for_values(&mut font, &mut |value, anchor| {
				value.y_pla_device = Some(Some(Box::new(build_device(delta))));
				// The anchors of the fixture have the format 3, with device tables.
				let Anchor::Format3(anchor) = anchor else {
					return Err("not a format 3 anchor".into());
				};
				anchor.y_device_table = Some(Box::new(build_device(delta)));
				Ok(())
			})?;
			let mut limits = vec![("wght", pin(1.0))];
			if full {
				limits.push(("wdth", pin(0.0)));
			}
			let mut result = instance(&font, &limits)?;
			for_values(&mut result, &mut |value, anchor| {
				assert_eq!(value.x_placement, Some(20));
				assert_eq!(value.y_placement, Some(20));
				let device = value.y_pla_device.as_ref().and_then(|d| d.as_deref());
				assert_eq!(device.ok_or("no YPlaDevice")?.delta_value, [delta]);
				let Anchor::Format3(anchor) = anchor else {
					return Err("not a format 3 anchor".into());
				};
				assert_eq!(anchor.x_coordinate, 130);
				assert_eq!(anchor.y_coordinate, 200);
				let device = anchor.y_device_table.as_ref().ok_or("no YDeviceTable")?;
				assert_eq!(device.delta_value, [delta]);
				Ok(())
			})?;
		}
	}
	Ok(())
}
