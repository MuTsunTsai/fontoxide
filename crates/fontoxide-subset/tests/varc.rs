//! The tests of fontTools' `Tests/subset/subset_test.py` that modify the `VARC`
//! table of a test font before subsetting it. `test_varComposite_closure_visited`
//! is a unit test of the `varc` module.

mod common;

use common::*;
use fontoxide_pens::BoundsPen;
use fontoxide_subset::{Options, Subsetter};
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::varc::{NO_VARIATION_INDEX, VarComponent, VarCompositeGlyph};
use fontoxide_ttlib::tables::otl::Varc;
use fontoxide_ttlib::{Font, GlyphSet, WriteOptions};

fn composites(varc: &mut Varc) -> TestResult<&mut Vec<VarCompositeGlyph>> {
	Ok(&mut varc
		.var_composite_glyphs
		.as_mut()
		.ok_or("no VarCompositeGlyphs")?
		.var_composite_glyph)
}

/// `test_varComposite_self_reference_subset`.
#[test]
fn test_var_composite_self_reference_subset() -> TestResult {
	let mut font = open(tests_path("cffLib/data/varc-short-cff2.otf"))?;
	let leaf = font.glyph_id("leaf")?;
	let varc = font.require::<Varc>()?;
	varc.coverage
		.as_mut()
		.ok_or("no Coverage")?
		.glyphs
		.insert(0, leaf);
	composites(varc)?.insert(
		0,
		VarCompositeGlyph {
			components: vec![VarComponent {
				glyph: leaf,
				..Default::default()
			}],
		},
	);

	let mut subsetter = Subsetter::new(Options::default());
	subsetter.populate(&["composite"], &[], &[], "");
	subsetter.subset(&mut font)?;
	let mut font = save_and_read(&mut font)?;

	let best = font
		.get_best_cmap(&[(3, 10), (3, 1), (0, 3)])?
		.ok_or("no cmap")?;
	let composite = font.glyph_name(best[&65])?;
	{
		let glyph_set = GlyphSet::new(&mut font)?;
		let mut pen = BoundsPen::new_bounds(Some(&glyph_set));
		glyph_set.draw(&composite, &mut pen)?;
		assert_eq!(pen.bounds(), Some([600.0, 0.0, 800.0, 200.0]));
	}
	let varc = font.require::<Varc>()?;
	assert_eq!(varc.coverage.as_ref().ok_or("no Coverage")?.glyphs.len(), 2);
	Ok(())
}

fn subset_ac00(font: &mut Font) -> TestResult<Font> {
	let mut temp = TempDir::new()?;
	let input = temp.path(".ttf");
	std::fs::write(&input, font.write(&WriteOptions::default())?)?;
	let subset = temp.path(".ttf");
	let output = format!("--output-file={subset}");
	subset_main(&[&input, "--unicodes=ac00", &output])?;
	open(&subset)
}

fn value_condition(var_idx: u32) -> Box<ot::ConditionTable> {
	Box::new(ot::ConditionTable::Format2(ot::ConditionTableFormat2 {
		default_value: 0,
		var_idx,
	}))
}

#[test]
fn test_var_composite_condition_varidx() -> TestResult {
	let mut font = open(tests_path("ttLib/data/varc-ac00-ac01.ttf"))?;
	let varc = font.require::<Varc>()?;

	let unused = value_condition(5);
	let negated = Box::new(ot::ConditionTable::Format5(ot::ConditionTableFormat5 {
		condition_table: Some(value_condition(4)),
	}));
	let negated_null = Box::new(ot::ConditionTable::Format5(ot::ConditionTableFormat5 {
		condition_table: None,
	}));
	let used = Box::new(ot::ConditionTable::Format3(ot::ConditionTableFormat3 {
		condition_count: None,
		condition_table: vec![
			None,
			Some(value_condition(6)),
			Some(negated),
			Some(negated_null),
		],
	}));
	varc.condition_list = Some(Box::new(ot::ConditionList {
		condition_count: None,
		condition_table: vec![Some(unused), Some(used), None],
	}));
	let components = &mut composites(varc)?[0].components;
	components[0].condition_index = Some(1);
	components[1].condition_index = Some(2);

	let mut subset = subset_ac00(&mut font)?;
	let varc = subset.require::<Varc>()?;
	let list = varc.condition_list.as_ref().ok_or("no ConditionList")?;
	assert_eq!(list.condition_table.len(), 2);
	assert!(list.condition_table[1].is_none());
	let Some(ot::ConditionTable::Format3(condition)) = list.condition_table[0].as_deref() else {
		return Err("not format 3".into());
	};
	assert!(condition.condition_table[0].is_none());
	let Some(ot::ConditionTable::Format2(value)) = condition.condition_table[1].as_deref() else {
		return Err("not format 2".into());
	};
	assert_eq!(value.var_idx, 3);
	let Some(ot::ConditionTable::Format5(negated)) = condition.condition_table[2].as_deref() else {
		return Err("not format 5".into());
	};
	let Some(ot::ConditionTable::Format2(value)) = negated.condition_table.as_deref() else {
		return Err("not format 2".into());
	};
	assert_eq!(value.var_idx, 2);
	let Some(ot::ConditionTable::Format5(negated)) = condition.condition_table[3].as_deref() else {
		return Err("not format 5".into());
	};
	assert!(negated.condition_table.is_none());
	let store = varc.multi_var_store.as_ref().ok_or("no MultiVarStore")?;
	assert_eq!(store.multi_var_data.len(), 1);
	assert_eq!(
		store.multi_var_data[0]
			.as_ref()
			.ok_or("no data")?
			.item
			.len(),
		4
	);
	Ok(())
}

#[test]
fn test_var_composite_drops_empty_auxiliary_data() -> TestResult {
	let mut font = open(tests_path("ttLib/data/varc-ac00-ac01.ttf"))?;
	let varc = font.require::<Varc>()?;
	for glyph in composites(varc)?.iter_mut() {
		for component in &mut glyph.components {
			component.axis_indices_index = None;
			component.axis_values = Vec::new();
			component.axis_values_var_index = NO_VARIATION_INDEX;
			component.transform_var_index = NO_VARIATION_INDEX;
		}
	}
	let condition = Box::new(ot::ConditionTable::Format1(ot::ConditionTableFormat1 {
		axis_index: 0,
		filter_range_min_value: 0.0,
		filter_range_max_value: 1.0,
	}));
	varc.condition_list = Some(Box::new(ot::ConditionList {
		condition_count: None,
		condition_table: vec![Some(condition)],
	}));

	let mut subset = subset_ac00(&mut font)?;
	let varc = subset.require::<Varc>()?;
	assert!(varc.multi_var_store.is_none());
	assert!(varc.condition_list.is_none());
	assert!(varc.axis_indices_list.is_none());
	Ok(())
}
