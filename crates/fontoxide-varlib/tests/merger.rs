//! `Tests/varLib/merger_test.py`: the class `COLRVariationMergerTest` and
//! `test_mutator_merge_small_extended_gpos`.
//!
//! Its parameters are generated into `merger_colr/cases.rs` by
//! `tools/reference/merger_colr_fixtures.py`.

#[path = "merger_colr/cases.rs"]
mod cases;

use std::collections::HashMap;

use fontoxide_colorlib::Value;
use fontoxide_colorlib::builder::{
	BuildColrOptions, LayerListBuilder, build_clip_list, build_colr,
};
use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_ttlib::beyond64k::{ConvertOptions, upper_tables};
use fontoxide_ttlib::ot::conv::ValueRecord;
use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::{OtReader, WriterGraph};
use fontoxide_ttlib::tables::{Colr, Gpos};
use fontoxide_ttlib::{Font, TableType, Tag};
use fontoxide_varlib::builder::build_var_dev_table;
use fontoxide_varlib::instancer::mutator_merge_gpos;
use fontoxide_varlib::merger::{MasterInfo, Merger, expand_paint_colr_layers};
use fontoxide_varlib::models::VariationModel;
use indexmap::IndexMap;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const NO_VARIATION_INDEX: u32 = 0xFFFF_FFFF;

pub(crate) struct MergePaintCase {
	pub id: &'static str,
	pub paints: Vec<Value>,
	pub expected_xml: &'static [&'static str],
	pub expected_var_idxes: &'static [u32],
}

pub(crate) struct FullTableCase {
	pub id: &'static str,
	pub color_glyphs: Vec<Vec<(&'static str, Value)>>,
	pub output_layer_reuse: bool,
	pub expected_xml: &'static [&'static str],
	pub expected_var_idxes: &'static [u32],
}

pub(crate) struct ExpandCase {
	pub id: &'static str,
	pub color_glyphs: Vec<(&'static str, Value)>,
	pub before_xml: &'static [&'static str],
	pub expected_xml: &'static [&'static str],
}

pub(crate) fn i(v: i64) -> Value {
	Value::Int(v)
}

pub(crate) fn f(v: f64) -> Value {
	Value::Float(v)
}

pub(crate) fn s(v: &str) -> Value {
	Value::Str(v.to_owned())
}

pub(crate) fn l(items: Vec<Value>) -> Value {
	Value::List(items)
}

pub(crate) fn m(items: Vec<(&str, Value)>) -> Value {
	Value::Map(items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// The `ttFont` fixture.
fn tt_font() -> Font {
	let mut font = Font::new();
	let mut order = vec![".notdef".to_owned()];
	// `string.ascii_letters`.
	order.extend(('a'..='z').chain('A'..='Z').map(|c| c.to_string()));
	font.set_glyph_order(order);
	font
}

fn glyph_ids(font: &mut Font) -> Result<HashMap<String, u32>, Box<dyn std::error::Error>> {
	let order = font.glyph_order()?;
	Ok(order
		.names()
		.iter()
		.enumerate()
		.map(|(i, n)| (n.clone(), i as u32))
		.collect())
}

/// `build_paint`.
fn build_paint(
	glyphs: &HashMap<String, u32>,
	data: Value,
) -> Result<ot::Paint, Box<dyn std::error::Error>> {
	Ok(LayerListBuilder::new(glyphs, false, true).build_paint(data)?)
}

/// `dump_xml`: `getXML(table.toXML, ttFont)`.
fn dump_xml<T: OtTable>(
	table: &T,
	name: &str,
	font: &mut Font,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
	let mut writer = XmlWriter::with_options("  ", "\n");
	table.to_xml(&mut writer, font, &[], name)?;
	let text = writer.into_string();
	let body = text.split_once('\n').map_or(text.as_str(), |(_, b)| b);
	Ok(body.lines().map(str::to_owned).collect())
}

/// `compile_decompile`.
fn compile_decompile<T: OtTable>(
	table: &T,
	font: &mut Font,
) -> Result<T, Box<dyn std::error::Error>> {
	let mut graph = WriterGraph::new(Tag::new(b"COLR"));
	table.clone().compile(&mut graph.root(), font)?;
	let data = graph.get_all_data()?;
	let mut reader = OtReader::new(&data, Tag::new(b"COLR"));
	Ok(T::decompile(&mut reader, font)?)
}

fn model() -> Result<VariationModel, Box<dyn std::error::Error>> {
	Ok(VariationModel::new(
		&[vec![], vec![("ZZZZ".to_owned(), 1.0)]],
		None,
		false,
		None,
	)?)
}

fn assert_lines(actual: &[String], expected: &[&str], id: &str) {
	let expected: Vec<String> = expected.iter().map(|s| (*s).to_owned()).collect();
	assert_eq!(actual, expected, "{id}");
}

#[test]
fn test_merge_paint() -> TestResult {
	for case in cases::merge_paint_cases() {
		let mut font = tt_font();
		let glyphs = glyph_ids(&mut font)?;
		let paints = case
			.paints
			.into_iter()
			.map(|p| build_paint(&glyphs, p))
			.collect::<Result<Vec<_>, _>>()?;
		let mut out = paints[0].clone();

		let model = model()?;
		let masters = vec![MasterInfo::default(); paints.len()];
		let mut merger =
			Merger::colr_variation(&model, &["ZZZZ".to_owned()], &mut font, &masters, true)?;

		let refs: Vec<&ot::Paint> = paints.iter().collect();
		merger.merge_paint(&mut out, &refs)?;

		assert!(compile_decompile(&out, &mut font)? == out, "{}", case.id);
		assert_lines(
			&dump_xml(&out, "Paint", &mut font)?,
			case.expected_xml,
			case.id,
		);
		assert_eq!(
			merger.colr_var_idxes(),
			case.expected_var_idxes,
			"{}",
			case.id
		);
	}
	Ok(())
}

#[test]
fn test_merge_clip_list() -> TestResult {
	let mut font = tt_font();
	let glyphs = glyph_ids(&mut font)?;
	let clip = |b: [i64; 4]| l(b.iter().map(|&v| i(v)).collect());
	let clip_lists = [
		vec![
			("A", clip([0, 0, 1000, 1000])),
			("B", clip([0, 0, 1000, 1000])),
			("C", clip([0, 0, 1000, 1000])),
			("D", clip([0, 0, 1000, 1000])),
		],
		vec![
			// non-default masters' clip boxes can be 'sparse'
			// (i.e. can omit explicit clip box for some glyphs)
			("B", clip([10, 0, 1000, 1000])),
			("C", clip([20, 20, 1020, 1020])),
			("D", clip([20, 20, 1020, 1020])),
		],
	]
	.into_iter()
	.map(|clips| {
		let clips: IndexMap<String, Value> =
			clips.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
		build_clip_list(clips, &glyphs)
	})
	.collect::<Result<Vec<_>, _>>()?;
	let mut out = clip_lists[0].clone();

	let model = model()?;
	let masters = vec![MasterInfo::default(); 2];
	let mut merger =
		Merger::colr_variation(&model, &["ZZZZ".to_owned()], &mut font, &masters, true)?;
	let refs: Vec<&ot::ClipList> = clip_lists.iter().collect();
	merger.merge_clip_list(&mut out, &refs)?;

	assert!(compile_decompile(&out, &mut font)? == out);
	assert_lines(
		&dump_xml(&out, "ClipList", &mut font)?,
		&[
			"<ClipList Format=\"1\">",
			"  <Clip>",
			"    <Glyph value=\"A\"/>",
			"    <ClipBox Format=\"1\">",
			"      <xMin value=\"0\"/>",
			"      <yMin value=\"0\"/>",
			"      <xMax value=\"1000\"/>",
			"      <yMax value=\"1000\"/>",
			"    </ClipBox>",
			"  </Clip>",
			"  <Clip>",
			"    <Glyph value=\"B\"/>",
			"    <ClipBox Format=\"2\">",
			"      <xMin value=\"0\"/>",
			"      <yMin value=\"0\"/>",
			"      <xMax value=\"1000\"/>",
			"      <yMax value=\"1000\"/>",
			"      <VarIndexBase value=\"0\"/>",
			"    </ClipBox>",
			"  </Clip>",
			"  <Clip>",
			"    <Glyph value=\"C\"/>",
			"    <Glyph value=\"D\"/>",
			"    <ClipBox Format=\"2\">",
			"      <xMin value=\"0\"/>",
			"      <yMin value=\"0\"/>",
			"      <xMax value=\"1000\"/>",
			"      <yMax value=\"1000\"/>",
			"      <VarIndexBase value=\"4\"/>",
			"    </ClipBox>",
			"  </Clip>",
			"</ClipList>",
		],
		"ClipList",
	);
	assert_eq!(
		merger.colr_var_idxes(),
		[
			0,
			NO_VARIATION_INDEX,
			NO_VARIATION_INDEX,
			NO_VARIATION_INDEX,
			1,
			1,
			1,
			1,
		]
	);
	Ok(())
}

/// `buildCOLR(color_glyphs, allowLayerReuse=...)`.
fn colr_of(
	glyphs: &HashMap<String, u32>,
	color_glyphs: &[(&'static str, Value)],
	allow_layer_reuse: bool,
) -> Result<Colr, Box<dyn std::error::Error>> {
	let color_glyphs: IndexMap<String, Value> = color_glyphs
		.iter()
		.map(|(k, v)| ((*k).to_owned(), v.clone()))
		.collect();
	Ok(build_colr(
		color_glyphs,
		glyphs,
		BuildColrOptions {
			allow_layer_reuse,
			..Default::default()
		},
	)?)
}

#[test]
fn test_merge_full_table() -> TestResult {
	for master_layer_reuse in [false, true] {
		for case in cases::full_table_cases() {
			let id = format!("{}-{}", case.id, master_layer_reuse);
			let mut font = tt_font();
			let glyphs = glyph_ids(&mut font)?;
			let mut master_ttfs = Vec::new();
			for color_glyphs in &case.color_glyphs {
				// The merge algorithm is expected to work the same even if the master
				// COLRs may differ as to the layer reuse, hence we try both ways.
				let mut ttf = font.clone();
				ttf.insert(Box::new(colr_of(
					&glyphs,
					color_glyphs,
					master_layer_reuse,
				)?));
				master_ttfs.push(ttf);
			}
			let mut vf = master_ttfs[0].clone();

			let model = model()?;
			let masters = vec![MasterInfo::default(); master_ttfs.len()];
			let mut merger = Merger::colr_variation(
				&model,
				&["ZZZZ".to_owned()],
				&mut vf,
				&masters,
				case.output_layer_reuse,
			)?;
			let mut refs: Vec<&mut Font> = master_ttfs.iter_mut().collect();
			merger.merge_colr(&mut vf, &mut refs)?;

			let out = vf.require::<Colr>()?.table.clone().expect("a COLRv1 table");
			assert!(compile_decompile(&out, &mut vf)? == out, "{id}");
			assert_lines(&dump_xml(&out, "COLR", &mut vf)?, case.expected_xml, &id);
			assert_eq!(merger.colr_var_idxes(), case.expected_var_idxes, "{id}");
		}
	}
	Ok(())
}

fn colr_layers_reused(table: &ot::COLR) -> bool {
	table.layer_list.as_deref().is_some_and(|l| {
		l.paint
			.iter()
			.flatten()
			.any(|p| matches!(p.as_ref(), ot::Paint::Format1(_)))
	})
}

fn layer_count(table: &ot::COLR) -> usize {
	table.layer_list.as_deref().map_or(0, |l| l.paint.len())
}

#[test]
fn test_expand_paint_colr_layers() -> TestResult {
	for case in cases::expand_cases() {
		let mut font = tt_font();
		let glyphs = glyph_ids(&mut font)?;
		let colr = colr_of(&glyphs, &case.color_glyphs, true)?;
		let mut table = colr.table.expect("a COLRv1 table");

		assert_lines(
			&dump_xml(&table, "COLR", &mut font)?,
			case.before_xml,
			case.id,
		);

		let before_layer_count = layer_count(&table);
		let reuses_colr_layers = colr_layers_reused(&table);

		expand_paint_colr_layers(&mut table)?;

		assert_lines(
			&dump_xml(&table, "COLR", &mut font)?,
			case.expected_xml,
			case.id,
		);

		let after_layer_count = layer_count(&table);
		if reuses_colr_layers {
			assert!(!colr_layers_reused(&table), "{}", case.id);
			assert!(after_layer_count > before_layer_count, "{}", case.id);
		} else {
			assert_eq!(after_layer_count, before_layer_count, "{}", case.id);
		}
		// fontTools also checks that no layer is shared by identity, which Rust's
		// owned layers can't be.
	}
	Ok(())
}

// `test_mutator_merge_small_extended_gpos`.

enum VariableValue<'a> {
	Anchor(&'a mut ot::Anchor),
	Value(&'a mut ValueRecord),
}

impl VariableValue<'_> {
	fn value(&self) -> i64 {
		match self {
			VariableValue::Anchor(a) => i64::from(anchor_coordinates(a).0),
			VariableValue::Value(v) => i64::from(v.x_advance.unwrap_or(0)),
		}
	}

	fn has_device(&self) -> bool {
		match self {
			VariableValue::Anchor(a) => {
				matches!(&**a, ot::Anchor::Format3(a) if a.x_device_table.is_some())
			}
			VariableValue::Value(v) => matches!(v.x_adv_device, Some(Some(_))),
		}
	}
}

fn anchor_coordinates(a: &ot::Anchor) -> (i16, i16) {
	match a {
		ot::Anchor::Format1(a) => (a.x_coordinate, a.y_coordinate),
		ot::Anchor::Format2(a) => (a.x_coordinate, a.y_coordinate),
		ot::Anchor::Format3(a) => (a.x_coordinate, a.y_coordinate),
		ot::Anchor::Unknown(_) => (0, 0),
	}
}

/// `variable_values`: the `XCoordinate` of the anchors and the `XAdvance` of the
/// value records that have one.
fn variable_values(
	table: &mut ot::GPOS,
	mut f: impl FnMut(&mut VariableValue<'_>),
) -> Result<(), Box<dyn std::error::Error>> {
	table.visit_mut(&mut |any| {
		if let Some(anchor) = any.downcast_mut::<ot::Anchor>() {
			f(&mut VariableValue::Anchor(anchor));
			return Ok(());
		}
		let mut visit = |records: Vec<&mut ValueRecord>| {
			for record in records {
				if record.x_advance.is_some() {
					f(&mut VariableValue::Value(record));
				}
			}
		};
		if let Some(t) = any.downcast_mut::<ot::SinglePosFormat1>() {
			visit(t.value.as_mut().into_iter().collect());
		} else if let Some(t) = any.downcast_mut::<ot::SinglePosFormat3>() {
			visit(t.value.as_mut().into_iter().collect());
		} else if let Some(t) = any.downcast_mut::<ot::SinglePosFormat2>() {
			visit(t.value.iter_mut().flatten().collect());
		} else if let Some(t) = any.downcast_mut::<ot::SinglePosFormat4>() {
			visit(t.value.iter_mut().flatten().collect());
		} else if let Some(t) = any.downcast_mut::<ot::PairValueRecord>() {
			visit(
				t.value1
					.as_mut()
					.into_iter()
					.chain(t.value2.as_mut())
					.collect(),
			);
		} else if let Some(t) = any.downcast_mut::<ot::PairValue2>() {
			visit(
				t.value1
					.as_mut()
					.into_iter()
					.chain(t.value2.as_mut())
					.collect(),
			);
		} else if let Some(t) = any.downcast_mut::<ot::Class2Record>() {
			visit(
				t.value1
					.as_mut()
					.into_iter()
					.chain(t.value2.as_mut())
					.collect(),
			);
		}
		Ok(())
	})?;
	Ok(())
}

fn gpos(font: &mut Font) -> Result<&mut ot::GPOS, Box<dyn std::error::Error>> {
	Ok(&mut font.require::<Gpos>()?.table)
}

fn values_of(font: &mut Font) -> Result<Vec<(i64, bool)>, Box<dyn std::error::Error>> {
	let mut out = Vec::new();
	variable_values(gpos(font)?, |v| out.push((v.value(), v.has_device())))?;
	Ok(out)
}

/// `roundtrip_gpos`.
fn roundtrip_gpos(font: &mut Font) -> TestResult {
	let tag = Tag::new(b"GPOS");
	let data = font.table_data(tag)?;
	font.remove(tag);
	let table = Gpos::decompile(&data, font)?;
	font.insert(Box::new(table));
	Ok(())
}

fn subtable_format(st: &ot::LookupSubTable) -> Option<u16> {
	use ot::LookupSubTable as St;
	match st {
		St::SinglePos(t) => t.format(),
		St::PairPos(t) => t.format(),
		St::CursivePos(t) => t.format(),
		St::MarkBasePos(t) => t.format(),
		St::MarkLigPos(t) => t.format(),
		St::MarkMarkPos(t) => t.format(),
		_ => None,
	}
}

/// Adds the device tables (`XAdvDevice` and Format 3 anchors) for variation
/// index 0, and `XAdvDevice` to the value formats.
fn add_devices(font: &mut Font) -> TestResult {
	let device = || Box::new(build_var_dev_table(0));
	variable_values(gpos(font)?, |v| match v {
		VariableValue::Anchor(a) => {
			let (x, y) = anchor_coordinates(a);
			**a = ot::Anchor::Format3(ot::AnchorFormat3 {
				x_coordinate: x,
				y_coordinate: y,
				x_device_table: Some(device()),
				y_device_table: None,
			});
		}
		VariableValue::Value(r) => r.x_adv_device = Some(Some(device())),
	})?;
	gpos(font)?.visit_mut(&mut |any| {
		if let Some(t) = any.downcast_mut::<ot::SinglePosFormat1>() {
			t.value_format |= 0x40;
		} else if let Some(t) = any.downcast_mut::<ot::SinglePosFormat2>() {
			t.value_format |= 0x40;
		} else if let Some(t) = any.downcast_mut::<ot::PairPosFormat1>() {
			t.value_format1 |= 0x40;
		} else if let Some(t) = any.downcast_mut::<ot::PairPosFormat2>() {
			t.value_format1 |= 0x40;
		}
		Ok(())
	})?;
	Ok(())
}

#[test]
fn test_mutator_merge_small_extended_gpos() -> TestResult {
	let features_list = [
		"pos A 10;",
		"pos A 10; pos C 20;",
		"pos A B -20;",
		"pos [A C] [B D] -40;",
		"pos cursive A <anchor 10 20> <anchor 30 40>;",
		"markClass B <anchor 0 0> @TOP; pos base A <anchor 10 20> mark @TOP;",
		"markClass B <anchor 0 0> @TOP; pos mark A <anchor 10 20> mark @TOP;",
		"markClass B <anchor 0 0> @TOP; pos ligature A <anchor 10 20> mark @TOP;",
	];
	let mut count = 0;
	for features in features_list {
		for delete_variations in [false, true] {
			for extended in [false, true] {
				let id = format!("{features} delete={delete_variations} extended={extended}");
				let mut font = Font::new();
				font.set_glyph_order(
					[".notdef", "A", "B", "C", "D"]
						.iter()
						.map(|n| (*n).to_owned())
						.collect(),
				);
				fontoxide_fealib::builder::add_open_type_features_from_string(
					&mut font,
					&format!("feature kern {{ {features} }} kern;"),
					None,
					None,
					false,
				)?;

				let mut expected: Vec<i64> =
					values_of(&mut font)?.iter().map(|(v, _)| v + 50).collect();
				expected.sort_unstable();
				add_devices(&mut font)?;
				if extended {
					upper_tables(
						&mut font,
						&ConvertOptions {
							tables: Some(&["GPOS"]),
							..Default::default()
						},
					)?;
				}
				assert!(font.glyph_order()?.len() <= 0x10000);

				roundtrip_gpos(&mut font)?;
				let deltas = |i: u64| -> fontoxide_ttlib::Result<f64> {
					if i == 0 {
						Ok(50.0)
					} else {
						Err(fontoxide_ttlib::Error::ttlib(format!("KeyError: {i}")))
					}
				};
				mutator_merge_gpos(&mut font, &deltas, delete_variations)?;
				roundtrip_gpos(&mut font)?;
				let values = values_of(&mut font)?;
				assert!(!values.is_empty(), "{id}");
				let mut actual: Vec<i64> = values.iter().map(|(v, _)| *v).collect();
				actual.sort_unstable();
				assert_eq!(actual, expected, "{id}");
				for (_, has_device) in &values {
					assert_eq!(*has_device, !delete_variations, "{id}");
				}
				let table = gpos(&mut font)?;
				let lookups = if extended {
					table.lookup_list2.as_ref().map(|l| &l.lookup)
				} else {
					table.lookup_list.as_ref().map(|l| &l.lookup)
				}
				.expect("a lookup list");
				for lookup in lookups.iter().flatten() {
					for st in lookup.sub_table.iter().flatten() {
						let format = subtable_format(st).expect("a format");
						let formats: &[u16] = if !extended {
							&[1, 2]
						} else if matches!(lookup.lookup_type, 1 | 2) {
							&[3, 4]
						} else {
							&[2]
						};
						assert!(formats.contains(&format), "{id}: {format}");
					}
				}
				count += 1;
			}
		}
	}
	assert_eq!(count, 32);
	Ok(())
}
