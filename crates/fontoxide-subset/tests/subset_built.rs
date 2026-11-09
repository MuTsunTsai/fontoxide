//! The tests of fontTools' `Tests/subset/subset_test.py` that build their fonts
//! with `FontBuilder` and `feaLib`, which are not ported yet: the fonts are in
//! `tests/data`, made with the same code by `tools/reference/subset_fixtures.py`.
//!
//! fontTools runs some of these on lazily and fully loaded fonts, and on fonts in
//! memory and read back from TTX; these all end up in the same state here.

mod common;

use common::*;
use fontoxide_misc::xml_reader::{self, Element};
use fontoxide_pens::{Pen, Point};
use fontoxide_subset::{Item, Options, Subsetter};
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::cmap::DEFAULT_CMAP_PREFERENCES;
use fontoxide_ttlib::tables::otl::{Gdef, Gpos, Gsub};
use fontoxide_ttlib::tables::{Cmap, CmapSubtable, Colr, Cpal, Dmap, Glyf, Gvar, Name};
use fontoxide_ttlib::{Font, GlyphSet, Tag};

const NO_VARIATION_INDEX: u32 = 0xFFFF_FFFF;

fn subset_with(font: &mut Font, options: Options, unicodes: &[u32], glyphs: &[&str]) -> TestResult {
	let mut subsetter = Subsetter::new(options);
	subsetter.populate(glyphs, &[], unicodes, "");
	subsetter.subset(font)?;
	Ok(())
}

fn without_feature(feature: &str) -> Options {
	let mut options = Options::default();
	let item = Item::from(feature);
	options.layout_features.retain(|f| *f != item);
	options
}

fn all_features() -> Options {
	Options {
		layout_features: vec![Item::from("*")],
		..Default::default()
	}
}

fn feature_tags(records: &[ot::FeatureRecord]) -> Vec<String> {
	records.iter().map(|r| r.feature_tag.to_string()).collect()
}

fn gsub_feature_tags(font: &mut Font) -> TestResult<Vec<String>> {
	let gsub = font.require::<Gsub>()?;
	let list = gsub.feature_list.as_ref().ok_or("no FeatureList")?;
	Ok(feature_tags(&list.feature_record))
}

fn has_glyph(font: &mut Font, name: &str) -> TestResult<bool> {
	Ok(glyph_names(font)?.iter().any(|n| n == name))
}

#[test]
fn test_subset_feature_variations_keep_all() -> TestResult {
	let mut font = fixture("feature_vars")?;
	subset_with(&mut font, Options::default(), &[0x66, 0x24], &[])?;
	let tags = gsub_feature_tags(&mut font)?;
	// 'dlig' is discretionary so it is dropped by default.
	assert!(!tags.iter().any(|t| t == "dlig"));
	assert!(!has_glyph(&mut font, "f_f")?);
	// 'rvrn' is required so it is kept by default.
	assert!(tags.iter().any(|t| t == "rvrn"));
	assert!(has_glyph(&mut font, "dollar.rvrn")?);
	Ok(())
}

#[test]
fn test_subset_feature_variations_drop_all() -> TestResult {
	let mut font = fixture("feature_vars")?;
	subset_with(&mut font, without_feature("rvrn"), &[0x66, 0x24], &[])?;
	let tags = gsub_feature_tags(&mut font)?;
	assert!(!tags.iter().any(|t| t == "rvrn"));
	let mut names = glyph_names(&mut font)?;
	names.sort();
	assert_eq!(names, [".notdef", "dollar", "f"]);
	// All FeatureVariationRecords were dropped.
	let gsub = font.require::<Gsub>()?;
	assert!(gsub.feature_variations.is_none());
	assert_eq!(gsub.version, 0x0001_0000);
	Ok(())
}

#[test]
fn test_subset_lookup_variations_remaps_features_and_lookups() -> TestResult {
	let mut font = fixture("lookup_variation")?;
	subset_with(&mut font, Options::default(), &[0x66, 0x24], &[])?;
	let tags = gsub_feature_tags(&mut font)?;
	assert!(!tags.iter().any(|t| t == "dlig"));
	assert!(has_glyph(&mut font, "f_f")?);
	let rvrn = tags.iter().position(|t| t == "rvrn").ok_or("no rvrn")?;
	let gsub = font.require::<Gsub>()?;
	let lookup_count = gsub
		.lookup_list
		.as_ref()
		.ok_or("no LookupList")?
		.lookup
		.len();
	let variations = gsub.feature_variations.as_ref().ok_or("no variations")?;
	let record = &variations.lookup_variation_record[0];
	assert_eq!(usize::from(record.feature_index), rvrn);
	let table = record.feature_lookups_table.as_ref().ok_or("no table")?;
	let indices = &table.lookup_condition_record[0]
		.lookup_index_list
		.as_ref()
		.ok_or("no list")?
		.lookup_index;
	assert_eq!(indices.len(), 1);
	assert!(usize::from(indices[0]) < lookup_count);

	let mut roundtripped = save_and_read(&mut font)?;
	let gsub = roundtripped.require::<Gsub>()?;
	let variations = gsub.feature_variations.as_ref().ok_or("no variations")?;
	assert_eq!(variations.lookup_variation_count, Some(1));
	let table = variations.lookup_variation_record[0]
		.feature_lookups_table
		.as_ref()
		.ok_or("no table")?;
	assert_eq!(table.lookup_condition_count, Some(1));
	Ok(())
}

#[test]
fn test_subset_lookup_variations_keeps_empty_override() -> TestResult {
	let mut font = fixture("lookup_variation")?;
	subset_with(&mut font, Options::default(), &[0x24], &[])?;
	let gsub = font.require::<Gsub>()?;
	let variations = gsub.feature_variations.as_ref().ok_or("no variations")?;
	assert_eq!(variations.lookup_variation_record.len(), 1);
	let table = variations.lookup_variation_record[0]
		.feature_lookups_table
		.as_ref()
		.ok_or("no table")?;
	assert!(table.lookup_condition_record.is_empty());
	Ok(())
}

/// The value condition under the negation of the `None, negate` condition.
fn nested_var_idx(condition: &ot::ConditionTable) -> TestResult<(i16, u32)> {
	let ot::ConditionTable::Format3(all) = condition else {
		return Err("not format 3".into());
	};
	assert!(all.condition_table[0].is_none());
	let Some(ot::ConditionTable::Format5(negate)) = all.condition_table[1].as_deref() else {
		return Err("not format 5".into());
	};
	let Some(ot::ConditionTable::Format2(value)) = negate.condition_table.as_deref() else {
		return Err("not format 2".into());
	};
	Ok((value.default_value, value.var_idx))
}

#[test]
fn test_subset_preserves_condition_varstore() -> TestResult {
	for lookup_variations in [false, true] {
		for var_idx in [1, 7, 0x10000] {
			let name = format!(
				"condition_varstore_{}_{var_idx}",
				u8::from(lookup_variations)
			);
			let mut font = fixture(&name)?;
			subset_with(&mut font, Options::default(), &[0x66, 0x24], &[])?;
			if var_idx == 1 {
				let gdef = font.require::<Gdef>()?;
				let store = gdef.var_store.as_ref().ok_or("no VarStore")?;
				let data = store.var_data[0].as_ref().ok_or("no VarData")?;
				assert_eq!(data.item, [vec![2]], "{name}");
			} else {
				assert!(!font.contains(Tag::new(b"GDEF")), "{name}");
			}
			let mut roundtripped = save_and_read(&mut font)?;
			let gsub = roundtripped.require::<Gsub>()?;
			let variations = gsub.feature_variations.as_ref().ok_or("no variations")?;
			let condition = if lookup_variations {
				variations.lookup_variation_record[0]
					.feature_lookups_table
					.as_ref()
					.ok_or("no table")?
					.lookup_condition_record[0]
					.condition_table
					.as_deref()
			} else {
				variations.feature_variation_record[0]
					.condition_set
					.as_ref()
					.ok_or("no set")?
					.condition_table[0]
					.as_deref()
			};
			let (default, index) = nested_var_idx(condition.ok_or("no condition")?)?;
			assert_eq!(default, -1);
			let expected = if var_idx == 1 { 0 } else { NO_VARIATION_INDEX };
			assert_eq!(index, expected, "{name}");
		}
	}
	Ok(())
}

#[test]
fn test_subset_lookup_variations_prevents_feature_dedup() -> TestResult {
	let mut font = fixture("feature_dedup")?;
	subset_with(&mut font, all_features(), &[0x66, 0x24], &[])?;
	let tags = gsub_feature_tags(&mut font)?;
	assert_eq!(tags.iter().filter(|t| *t == "dlig").count(), 2);
	Ok(())
}

#[test]
fn test_subset_feature_variations_null_substitution() -> TestResult {
	for explicit_empty in [false, true] {
		for drop_feature in [false, true] {
			let mut font = fixture(&format!("null_substitution_{}", u8::from(explicit_empty)))?;
			let mut options = Options {
				glyph_names: true,
				..Default::default()
			};
			if drop_feature {
				let item = Item::from("rvrn");
				options.layout_features.retain(|f| *f != item);
			}
			subset_with(&mut font, options, &[0x66, 0x24], &[])?;
			let mut font = save_and_read(&mut font)?;
			let names = glyph_names(&mut font)?;
			let gsub = font.require::<Gsub>()?;
			if drop_feature {
				assert!(gsub.feature_variations.is_none());
				assert_eq!(gsub.version, 0x0001_0000);
				continue;
			}
			// The first empty record blocks the later substitution; only the last
			// empty record can be removed without changing first-match behavior.
			let variations = gsub.feature_variations.as_ref().ok_or("no variations")?;
			assert_eq!(variations.feature_variation_record.len(), 2);
			let first = &variations.feature_variation_record[0];
			let second = &variations.feature_variation_record[1];
			let condition = first
				.condition_set
				.as_ref()
				.ok_or("no set")?
				.condition_table[0]
				.as_deref()
				.ok_or("no condition")?;
			let ot::ConditionTable::Format1(condition) = condition else {
				return Err("not format 1".into());
			};
			assert_eq!(condition.filter_range_min_value, 0.5);
			if explicit_empty {
				let substitution = first
					.feature_table_substitution
					.as_ref()
					.ok_or("no substitution")?;
				assert_eq!(substitution.substitution_count, Some(0));
			} else {
				assert!(first.feature_table_substitution.is_none());
			}
			let substitution = second
				.feature_table_substitution
				.as_ref()
				.ok_or("no substitution")?;
			let feature = substitution.substitution_record[0]
				.feature
				.as_ref()
				.ok_or("no feature")?;
			let lookup = gsub.lookup_list.as_ref().ok_or("no list")?.lookup
				[usize::from(feature.lookup_list_index[0])]
			.as_ref()
			.ok_or("no lookup")?;
			let Some(ot::LookupSubTable::SingleSubst(subtable)) = lookup.sub_table[0].as_deref()
			else {
				return Err("not SingleSubst".into());
			};
			let mapping: Vec<(&str, &str)> = subtable
				.mapping
				.iter()
				.map(|(&a, &b)| (names[a as usize].as_str(), names[b as usize].as_str()))
				.collect();
			assert_eq!(mapping, [("dollar", "dollar.rvrn")]);
		}
	}
	Ok(())
}

/// `getXML` of the first subtable of the first lookup of `GPOS`.
fn single_pos(font: &mut Font) -> TestResult<ot::SinglePos> {
	let gpos = font.require::<Gpos>()?;
	let lookup = gpos.lookup_list.as_ref().ok_or("no list")?.lookup[0]
		.as_ref()
		.ok_or("no lookup")?;
	let Some(ot::LookupSubTable::SinglePos(subtable)) = lookup.sub_table[0].as_deref() else {
		return Err("not SinglePos".into());
	};
	Ok(subtable.clone())
}

#[test]
fn test_subset_single_pos_format() -> TestResult {
	let mut font = fixture("singlepos2")?;
	// Each glyph of the SinglePos Format 2 subtable has a different ValueRecord.
	let ot::SinglePos::Format2(before) = single_pos(&mut font)? else {
		return Err("not format 2".into());
	};
	assert_eq!(before.value_format, 4);
	assert_eq!(before.value.len(), 3);
	subset_with(&mut font, Options::default(), &[0x61, 0x63], &[])?;
	// The subset glyphs have the same ValueRecord: Format 1 is more compact.
	let ot::SinglePos::Format1(after) = single_pos(&mut font)? else {
		return Err("not format 1".into());
	};
	assert_eq!(after.coverage.as_ref().ok_or("no coverage")?.glyphs, [1, 2]);
	assert_eq!(after.value_format, 4);
	assert_eq!(after.value.as_ref().ok_or("no value")?.x_advance, Some(-50));
	Ok(())
}

#[test]
fn test_subset_single_pos_format2_all_none() -> TestResult {
	let mut font = fixture("singlepos2")?;
	// A SinglePosFormat2 with ValueFormat 0, which can be found in the wild.
	let gpos = font.require::<Gpos>()?;
	let lookup = gpos.lookup_list.as_mut().ok_or("no list")?.lookup[0]
		.as_mut()
		.ok_or("no lookup")?;
	let Some(ot::LookupSubTable::SinglePos(ot::SinglePos::Format2(subtable))) =
		lookup.sub_table[0].as_deref_mut()
	else {
		return Err("not format 2".into());
	};
	subtable.value = vec![None; subtable.value.len()];
	subtable.value_format = 0;
	subset_with(&mut font, Options::default(), &[0x61, 0x63], &[])?;
	// It was downgraded to Format 1.
	let ot::SinglePos::Format1(after) = single_pos(&mut font)? else {
		return Err("not format 1".into());
	};
	assert_eq!(after.coverage.as_ref().ok_or("no coverage")?.glyphs, [1, 2]);
	assert_eq!(after.value_format, 0);
	Ok(())
}

#[test]
fn test_subset_null_gdef_varstore() -> TestResult {
	for keep_classes in [false, true] {
		let mut font = fixture(&format!("null_gdef_varstore_{}", u8::from(keep_classes)))?;
		assert!(font.require::<Gdef>()?.var_store.is_none());
		let mut subsetter = Subsetter::new(Options::default());
		subsetter.populate(&[], &[], &[], "a");
		subsetter.subset(&mut font)?;
		let gdef_tag = Tag::new(b"GDEF");
		assert_eq!(font.contains(gdef_tag), keep_classes);
		if keep_classes {
			let a = font.glyph_id("a")?;
			let gdef = font.require::<Gdef>()?;
			let classes = gdef.glyph_class_def.as_ref().ok_or("no classes")?;
			assert_eq!(classes.class_defs.iter().collect::<Vec<_>>(), [(&a, &1)]);
			assert_eq!(gdef.version, 0x0001_0000);
		}
		assert_eq!(save_and_read(&mut font)?.contains(gdef_tag), keep_classes);
	}
	Ok(())
}

#[test]
fn test_subset_variable_notdef_table_order() -> TestResult {
	let mut font = fixture("variable_notdef")?;
	let deltas: Vec<(i32, i32)> = vec![
		(-20, 0),
		(-20, 0),
		(20, 0),
		(20, 0),
		(-10, -5),
		(10, -5),
		(10, 5),
		(-10, 5),
		(0, 0),
		(40, 0),
		(0, 0),
		(0, 0),
	];
	subset_with(&mut font, Options::default(), &[0x41], &[])?;
	let mut result = save_and_read(&mut font)?;
	assert_eq!(result.require::<Glyf>()?.glyphs[&0].number_of_contours, 0);
	let gvar = result.require::<Gvar>()?;
	assert!(
		gvar.variations
			.iter()
			.find(|(g, _)| *g == 0)
			.ok_or("no .notdef")?
			.1
			.is_empty()
	);
	let (_, a) = gvar
		.variations
		.iter()
		.find(|(g, _)| *g == 1)
		.ok_or("no A")?;
	let coordinates: Vec<(i32, i32)> = a[0]
		.coordinates
		.iter()
		.map(|d| d.as_ref().map(delta_pair).ok_or("no delta"))
		.collect::<Result<_, _>>()?;
	assert_eq!(coordinates, deltas);
	Ok(())
}

fn delta_pair(delta: &fontoxide_ttlib::tables::tuple_variation::Delta) -> (i32, i32) {
	match *delta {
		fontoxide_ttlib::tables::tuple_variation::Delta::Point(x, y) => {
			(x.value() as i32, y.value() as i32)
		}
		fontoxide_ttlib::tables::tuple_variation::Delta::Value(v) => (v.value() as i32, 0),
	}
}

type Outline = Vec<(&'static str, Vec<Point>)>;

/// `DecomposingRecordingPen`.
#[derive(Default)]
struct Recording(Outline);

impl fontoxide_pens::SegmentPen for Recording {
	fn move_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.0.push(("moveTo", vec![pt]));
		Ok(())
	}
	fn line_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.0.push(("lineTo", vec![pt]));
		Ok(())
	}
	fn curve_to_one(
		&mut self,
		_current: Option<Point>,
		pt1: Point,
		pt2: Point,
		pt3: Point,
	) -> fontoxide_pens::Result<()> {
		self.0.push(("curveTo", vec![pt1, pt2, pt3]));
		Ok(())
	}
	fn close_path(&mut self) -> fontoxide_pens::Result<()> {
		self.0.push(("closePath", Vec::new()));
		Ok(())
	}
	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		self.0.push(("endPath", Vec::new()));
		Ok(())
	}
}

fn outline(font: &mut Font, glyph: &str) -> TestResult<Outline> {
	let glyph_set = GlyphSet::new(font)?;
	let mut pen = fontoxide_pens::BasePen::with_inner(Recording::default(), Some(&glyph_set));
	glyph_set.draw(glyph, &mut pen as &mut dyn Pen)?;
	Ok(pen.inner.0)
}

#[test]
fn test_subset_composite_closure() -> TestResult {
	for retain_gids in [false, true] {
		for nested in [false, true] {
			for varc in [false, true] {
				let name = format!("composite_closure_{}_{}", u8::from(nested), u8::from(varc));
				let mut font = fixture(&name)?;
				let expected = outline(&mut font, "composite")?;
				assert!(!expected.is_empty());
				let options = Options {
					glyph_names: true,
					retain_gids,
					..Default::default()
				};
				subset_with(&mut font, options, &[65], &[])?;
				assert!(has_glyph(&mut font, "leaf")?, "{name}");
				if nested {
					assert!(has_glyph(&mut font, "unused")?, "{name}");
				}
				let mut font = save_and_read(&mut font)?;
				assert_eq!(outline(&mut font, "composite")?, expected, "{name}");
			}
		}
	}
	Ok(())
}

fn colr_xml(font: &mut Font) -> TestResult<Element> {
	Ok(xml_reader::parse(&to_ttx(font, &["COLR"])?)?)
}

/// The descendants with a name, in document order.
fn all_named<'a>(e: &'a Element, name: &str, out: &mut Vec<&'a Element>) {
	for child in e.elements() {
		if child.name == name {
			out.push(child);
		}
		all_named(child, name, out);
	}
}

fn child<'a>(e: &'a Element, name: &str) -> TestResult<&'a Element> {
	Ok(e.elements()
		.find(|c| c.name == name)
		.ok_or_else(|| format!("no {name} in {}", e.name))?)
}

fn value_of<'a>(e: &'a Element, name: &str) -> TestResult<&'a str> {
	Ok(child(e, name)?.attr("value").ok_or("no value")?)
}

fn subset_colrv1(fixture_name: &str, args: &[&str]) -> TestResult<Font> {
	let mut temp = TempDir::new()?;
	let mut font = fixture(fixture_name)?;
	let path = temp.path(".ttf");
	std::fs::write(
		&path,
		font.write(&fontoxide_ttlib::WriteOptions::default())?,
	)?;
	let subset_path = temp.path(".subset");
	let output = format!("--output-file={subset_path}");
	let mut argv = vec![path.as_str(), "--glyph-names", output.as_str()];
	argv.extend(args);
	subset_main(&argv)?;
	open(&subset_path)
}

#[test]
fn test_subset_colrv1_and_cpal() -> TestResult {
	let mut font = subset_colrv1("colrv1", &["--unicodes=E002,E003,E004"])?;
	let names = glyph_names(&mut font)?;
	let has = |n: &str| names.iter().any(|g| g == n);
	// uniE000 and its children are excluded from the subset.
	for glyph in ["uniE000", "glyph00010", "glyph00011"] {
		assert!(!has(glyph), "{glyph}");
	}
	// uniE001 and its children are pulled in as PaintColrGlyph by uniE003.
	for glyph in [
		"uniE001",
		"glyph00012",
		"glyph00013",
		"uniE002",
		"glyph00014",
		"glyph00015",
		"uniE003",
		"uniE004",
		"glyph00016",
		"glyph00017",
		"glyph00018",
	] {
		assert!(has(glyph), "{glyph}");
	}

	let xml = colr_xml(&mut font)?;
	let colr = child(&xml, "COLR")?;
	let mut records = Vec::new();
	all_named(colr, "BaseGlyphRecord", &mut records);
	assert_eq!(records.len(), 1);
	let mut paints = Vec::new();
	all_named(colr, "BaseGlyphPaintRecord", &mut paints);
	assert_eq!(paints.len(), 3);

	let base = paints[0];
	assert_eq!(value_of(base, "BaseGlyph")?, "uniE001");
	let paint = child(base, "Paint")?;
	let first: usize = value_of(paint, "FirstLayerIndex")?.parse()?;
	let count: usize = value_of(paint, "NumLayers")?.parse()?;
	assert_eq!(count, 2);
	let layer_list = child(colr, "LayerList")?;
	let layers: Vec<&Element> = layer_list
		.elements()
		.filter(|e| e.name == "Paint")
		.skip(first)
		.take(count)
		.collect();
	// The v1 palette indices were remapped.
	let gradient = child(child(layers[0], "Paint")?, "Paint")?;
	let mut stops = Vec::new();
	all_named(gradient, "ColorStop", &mut stops);
	assert_eq!(value_of(stops[0], "PaletteIndex")?, "0");
	assert_eq!(value_of(stops[1], "PaletteIndex")?, "1");
	assert_eq!(value_of(child(layers[1], "Paint")?, "PaletteIndex")?, "0");

	assert_eq!(value_of(records[0], "BaseGlyph")?, "uniE004");
	let mut layers_v0 = Vec::new();
	all_named(colr, "LayerRecord", &mut layers_v0);
	assert_eq!(layers_v0.len(), 3);
	// The v0 palette indices were remapped, except for 0xFFFF.
	assert_eq!(value_of(layers_v0[0], "PaletteIndex")?, "0");
	assert_eq!(value_of(layers_v0[1], "PaletteIndex")?, "65535");
	assert_eq!(value_of(layers_v0[2], "PaletteIndex")?, "1");

	let e001 = font.glyph_id("uniE001")?;
	let e002 = font.glyph_id("uniE002")?;
	let e003 = font.glyph_id("uniE003")?;
	let table = font.require::<Colr>()?;
	let table = table.table.as_ref().ok_or("no COLR table")?;
	let clips = &table.clip_list.as_ref().ok_or("no ClipList")?.clips;
	assert_eq!(
		clips.keys().copied().collect::<Vec<_>>(),
		[e001, e002, e003]
	);
	assert_eq!(clips[&e002], clips[&e003]);

	let cpal = font.require::<Cpal>()?;
	let colors: Vec<(u8, u8, u8, u8)> = cpal.palettes[0]
		.iter()
		.map(|c| (c.red, c.green, c.blue, c.alpha))
		.collect();
	// The first color, red, was pruned.
	assert_eq!(colors, [(0, 255, 0, 255), (0, 0, 255, 255)]);
	Ok(())
}

fn entry_label_names(font: &mut Font) -> TestResult<Vec<Option<String>>> {
	let labels = font.require::<Cpal>()?.palette_entry_labels.clone();
	let name = font.require::<Name>()?;
	Ok(labels.iter().map(|&id| name.get_debug_name(id)).collect())
}

fn id_of_name(font: &mut Font, text: &str) -> TestResult<u16> {
	let name = font.require::<Name>()?;
	Ok(name
		.names
		.iter()
		.find(|n| n.to_unicode().is_ok_and(|s| s == text))
		.ok_or_else(|| format!("no name {text}"))?
		.name_id)
}

const REMAINING_LABELS: [Option<&str>; 2] = [Some("second color"), Some("third color")];

fn assert_remaining_labels(font: &mut Font) -> TestResult {
	let labels = entry_label_names(font)?;
	let expected: Vec<Option<String>> = REMAINING_LABELS
		.iter()
		.map(|l| l.map(str::to_owned))
		.collect();
	assert_eq!(labels, expected);
	Ok(())
}

#[test]
fn test_subset_colrv1_and_cpalv1() -> TestResult {
	let mut font = subset_colrv1("colrv1_cpalv1", &["--unicodes=E002,E003,E004"])?;
	assert_remaining_labels(&mut font)?;
	// The "first color" name is dropped from the name table.
	let first = id_of_name(&mut fixture("colrv1_cpalv1")?, "first color")?;
	assert!(
		font.require::<Name>()?
			.names
			.iter()
			.all(|n| n.name_id != first)
	);
	Ok(())
}

#[test]
fn test_subset_colrv1_and_cpalv1_keep_name_id() -> TestResult {
	let first = id_of_name(&mut fixture("colrv1_cpalv1")?, "first color")?;
	let option = format!("--name-IDs={first}");
	let mut font = subset_colrv1("colrv1_cpalv1", &["--unicodes=E002,E003,E004", &option])?;
	assert_remaining_labels(&mut font)?;
	// The name ID is kept.
	assert!(
		font.require::<Name>()?
			.names
			.iter()
			.any(|n| n.name_id == first)
	);
	Ok(())
}

#[test]
fn test_subset_colrv1_and_cpalv1_share_name_id() -> TestResult {
	let mut font = subset_colrv1(
		"colrv1_cpalv1_share_name_id",
		&["--unicodes=E002,E003,E004"],
	)?;
	assert_remaining_labels(&mut font)?;
	// Name ID 1 is kept.
	assert!(font.require::<Name>()?.names.iter().any(|n| n.name_id == 1));
	Ok(())
}

#[test]
fn test_subset_colrv1_and_cpal_drop_empty() -> TestResult {
	let mut font = subset_colrv1("colrv1", &["--glyphs=glyph00010"])?;
	assert!(has_glyph(&mut font, "glyph00010")?);
	assert!(!has_glyph(&mut font, "uniE000")?);
	assert!(!font.contains(Tag::new(b"COLR")));
	assert!(!font.contains(Tag::new(b"CPAL")));
	Ok(())
}

#[test]
fn test_subset_colrv1_downgrade_version() -> TestResult {
	let mut font = subset_colrv1("colrv1", &["--unicodes=E004"])?;
	let mut names = glyph_names(&mut font)?;
	names.sort();
	assert_eq!(
		names,
		[
			".notdef",
			"glyph00016",
			"glyph00017",
			"glyph00018",
			"uniE004"
		]
	);
	assert_eq!(font.require::<Colr>()?.version, 0);
	Ok(())
}

#[test]
fn test_subset_colrv1_drop_all_v0_glyphs() -> TestResult {
	let mut font = subset_colrv1("colrv1", &["--unicodes=E003"])?;
	let mut names = glyph_names(&mut font)?;
	names.sort();
	assert_eq!(
		names,
		[".notdef", "glyph00012", "glyph00013", "uniE001", "uniE003"]
	);
	let colr = font.require::<Colr>()?;
	assert_eq!(colr.version, 1);
	let table = colr.table.as_ref().ok_or("no table")?;
	assert_eq!(table.base_glyph_record_count, Some(0));
	assert!(table.base_glyph_record_array.is_none());
	assert!(table.layer_record_array.is_none());
	assert_eq!(table.layer_record_count, Some(0));
	Ok(())
}

#[test]
fn test_subset_colrv1_no_clip_list() -> TestResult {
	let mut temp = TempDir::new()?;
	let mut font = fixture("colrv1")?;
	let colr = font.require::<Colr>()?;
	colr.table.as_mut().ok_or("no table")?.clip_list = None;
	let path = temp.path(".ttf");
	std::fs::write(
		&path,
		font.write(&fontoxide_ttlib::WriteOptions::default())?,
	)?;
	let subset_path = temp.path(".subset");
	let output = format!("--output-file={subset_path}");
	subset_main(&[&path, &output, "--unicodes=*"])?;
	let mut subset = open(&subset_path)?;
	let colr = subset.require::<Colr>()?;
	assert!(colr.table.as_ref().ok_or("no table")?.clip_list.is_none());
	Ok(())
}

#[test]
fn test_subset_keep_size_drop_empty_stylistic_set() -> TestResult {
	let mut font = fixture("keep_size")?;
	let first_params = |font: &mut Font| -> TestResult<(String, usize, bool)> {
		let gpos = font.require::<Gpos>()?;
		let record = &gpos.feature_list.as_ref().ok_or("no list")?.feature_record[0];
		let feature = record.feature.as_ref().ok_or("no feature")?;
		Ok((
			record.feature_tag.to_string(),
			feature.lookup_list_index.len(),
			matches!(
				feature.feature_params.as_deref(),
				Some(ot::FeatureParamsAny::FeatureParamsSize(_))
			),
		))
	};
	assert_eq!(first_params(&mut font)?, ("size".to_owned(), 0, true));
	{
		let gsub = font.require::<Gsub>()?;
		let record = &gsub.feature_list.as_ref().ok_or("no list")?.feature_record[0];
		assert_eq!(record.feature_tag.to_string(), "ss01");
		let feature = record.feature.as_ref().ok_or("no feature")?;
		assert!(matches!(
			feature.feature_params.as_deref(),
			Some(ot::FeatureParamsAny::FeatureParamsStylisticSet(_))
		));
	}
	subset_with(&mut font, all_features(), &[0x61], &[])?;
	// The empty size feature was kept.
	assert_eq!(first_params(&mut font)?, ("size".to_owned(), 0, true));
	// The empty ss01 feature was dropped.
	assert!(gsub_feature_tags(&mut font)?.is_empty());
	Ok(())
}

type LookupMarks = Vec<(u16, u16)>;

/// The flags and mark filtering sets of the lookups, and the glyph names of the
/// mark glyph sets.
fn marks(font: &mut Font) -> TestResult<(LookupMarks, Vec<Vec<String>>)> {
	let names = glyph_names(font)?;
	let gsub = font.require::<Gsub>()?;
	let lookups = gsub
		.lookup_list
		.as_ref()
		.ok_or("no list")?
		.lookup
		.iter()
		.flatten()
		.map(|l| (l.lookup_flag, l.mark_filtering_set))
		.collect();
	let gdef = font.require::<Gdef>()?;
	let sets = gdef
		.mark_glyph_sets_def
		.as_ref()
		.ok_or("no MarkGlyphSetsDef")?
		.coverage
		.iter()
		.flatten()
		.map(|c| {
			c.glyphs
				.iter()
				.map(|&g| names[g as usize].clone())
				.collect()
		})
		.collect();
	Ok((lookups, sets))
}

#[test]
fn test_subset_prune_gdef_markglyphsetsdef() -> TestResult {
	let mut font = fixture("mark_glyph_sets")?;
	let (lookups, sets) = marks(&mut font)?;
	assert_eq!(lookups, [(16, 0), (16, 1), (16, 2)]);
	assert_eq!(sets, [["acutecomb"], ["circumflexcomb"], ["dieresiscomb"]]);
	subset_with(
		&mut font,
		all_features(),
		&[],
		&["A", "a", "acutecomb", "dieresiscomb"],
	)?;
	assert_eq!(gsub_feature_tags(&mut font)?, ["ccmp"]);
	let (lookups, sets) = marks(&mut font)?;
	assert_eq!(lookups[0], (16, 0));
	assert_eq!(lookups[1], (16, 1));
	assert_eq!(sets, [["acutecomb"], ["dieresiscomb"]]);

	let mut font = fixture("mark_glyph_sets")?;
	let options = Options {
		layout_closure: false,
		..all_features()
	};
	subset_with(&mut font, options, &[], &["A", "acutecomb", "Aacute"])?;
	assert_eq!(gsub_feature_tags(&mut font)?, ["ccmp"]);
	let (lookups, sets) = marks(&mut font)?;
	assert_eq!(lookups[0], (16, 0));
	// The flag without its filtering set; fontTools sets the set to `None`.
	assert_eq!(lookups[1].0, 8);
	assert_eq!(sets, [["acutecomb"]]);
	Ok(())
}

#[test]
fn test_cmap_format15() -> TestResult {
	let mut temp = TempDir::new()?;
	let fontpath = temp.compile_font(&getpath("cmap14_font1.ttx"), ".otf")?;
	let mut font = open(&fontpath)?;
	let cmap = font.require::<Cmap>()?;
	let at = cmap
		.tables
		.iter()
		.position(|t| t.format == 14)
		.ok_or("no 14")?;
	let cmap14 = &cmap.tables[at];
	let mut cmap15 = CmapSubtable::new(15);
	cmap15.platform_id = cmap14.platform_id;
	cmap15.plat_enc_id = cmap14.plat_enc_id;
	cmap15.language = cmap14.language;
	cmap15.uvs_dict = cmap14.uvs_dict.clone();
	cmap.tables[at] = cmap15;
	let input = temp.path(".otf");
	std::fs::write(
		&input,
		font.write(&fontoxide_ttlib::WriteOptions::default())?,
	)?;
	let subsetpath = temp.path(".otf");
	let output = format!("--output-file={subsetpath}");
	subset_main(&[&input, "--unicodes=4e10,e0100", &output])?;
	let mut subset = open(&subsetpath)?;
	let g25 = subset.glyph_id("g25")?;
	let cmap = subset.require::<Cmap>()?;
	let t = cmap.tables.iter().find(|t| t.format == 15).ok_or("no 15")?;
	let uvs = t.uvs_dict.as_ref().ok_or("no uvsDict")?;
	assert_eq!(uvs.len(), 1);
	assert_eq!(uvs[&0xE0100], [(0x4E10, Some(g25))]);
	Ok(())
}

/// A font like the ones the `DMAP` tests build with `FontBuilder`: only what the
/// assertions look at matters.
fn dmap_font(
	order: &[&str],
	cmap: &[(u32, &str)],
	cmap_extra: &str,
	dmap: &str,
) -> TestResult<Font> {
	let mut ttx = String::from("<ttFont sfntVersion=\"\\x00\\x01\\x00\\x00\">\n<GlyphOrder>\n");
	for name in order {
		ttx.push_str(&format!("<GlyphID name=\"{name}\"/>\n"));
	}
	ttx.push_str("</GlyphOrder>\n");
	ttx.push_str(HEAD_TABLES);
	ttx.push_str(&format!(
		"<maxp><tableVersion value=\"0x10000\"/><numGlyphs value=\"{}\"/><maxPoints value=\"0\"/><maxContours value=\"0\"/><maxCompositePoints value=\"0\"/><maxCompositeContours value=\"0\"/><maxZones value=\"2\"/><maxTwilightPoints value=\"0\"/><maxStorage value=\"0\"/><maxFunctionDefs value=\"0\"/><maxInstructionDefs value=\"0\"/><maxStackElements value=\"0\"/><maxSizeOfInstructions value=\"0\"/><maxComponentElements value=\"0\"/><maxComponentDepth value=\"0\"/></maxp>\n",
		order.len()
	));
	ttx.push_str("<hmtx>");
	for name in order {
		ttx.push_str(&format!("<mtx name=\"{name}\" width=\"500\" lsb=\"0\"/>"));
	}
	ttx.push_str("</hmtx>\n<cmap><tableVersion version=\"0\"/>");
	for (format, platform, encoding) in [(4, 0, 3), (4, 3, 1)] {
		ttx.push_str(&format!(
			"<cmap_format_{format} platformID=\"{platform}\" platEncID=\"{encoding}\" language=\"0\">"
		));
		for (code, name) in cmap {
			ttx.push_str(&format!("<map code=\"{code:#x}\" name=\"{name}\"/>"));
		}
		ttx.push_str(&format!("</cmap_format_{format}>"));
	}
	ttx.push_str(cmap_extra);
	ttx.push_str("</cmap>\n<loca/>\n<glyf>");
	for name in order {
		ttx.push_str(&format!("<TTGlyph name=\"{name}\"/>"));
	}
	ttx.push_str("</glyf>\n");
	ttx.push_str("<DMAP><tableVersion version=\"0\"/>");
	ttx.push_str(dmap);
	ttx.push_str("</DMAP>\n</ttFont>\n");
	let mut font = Font::new();
	font.import_ttx(&ttx)?;
	Ok(font)
}

const HEAD_TABLES: &str = "<head><tableVersion value=\"1.0\"/><fontRevision value=\"1.0\"/><checkSumAdjustment value=\"0\"/><magicNumber value=\"0x5f0f3cf5\"/><flags value=\"00000000 00000011\"/><unitsPerEm value=\"1024\"/><created value=\"Thu Jan  1 00:00:00 1970\"/><modified value=\"Thu Jan  1 00:00:00 1970\"/><xMin value=\"0\"/><yMin value=\"0\"/><xMax value=\"0\"/><yMax value=\"0\"/><macStyle value=\"00000000 00000000\"/><lowestRecPPEM value=\"3\"/><fontDirectionHint value=\"2\"/><indexToLocFormat value=\"0\"/><glyphDataFormat value=\"0\"/></head>
<hhea><tableVersion value=\"0x00010000\"/><ascent value=\"0\"/><descent value=\"0\"/><lineGap value=\"0\"/><advanceWidthMax value=\"500\"/><minLeftSideBearing value=\"0\"/><minRightSideBearing value=\"0\"/><xMaxExtent value=\"0\"/><caretSlopeRise value=\"1\"/><caretSlopeRun value=\"0\"/><caretOffset value=\"0\"/><reserved0 value=\"0\"/><reserved1 value=\"0\"/><reserved2 value=\"0\"/><reserved3 value=\"0\"/><metricDataFormat value=\"0\"/><numberOfHMetrics value=\"1\"/></hhea>
<post><formatType value=\"2.0\"/><italicAngle value=\"0.0\"/><underlinePosition value=\"0\"/><underlineThickness value=\"0\"/><isFixedPitch value=\"0\"/><minMemType42 value=\"0\"/><maxMemType42 value=\"0\"/><minMemType1 value=\"0\"/><maxMemType1 value=\"0\"/><psNames/><extraNames/></post>
";

/// `getBestCmap` of a table, with glyph names.
fn best(font: &mut Font, dmap: bool) -> TestResult<Vec<(u32, String)>> {
	let names = glyph_names(font)?;
	let cmap = if dmap {
		&font.require::<Dmap>()?.0
	} else {
		&*font.require::<Cmap>()?
	};
	let Some(best) = cmap.get_best_cmap(&DEFAULT_CMAP_PREFERENCES) else {
		return Ok(Vec::new());
	};
	Ok(best
		.iter()
		.map(|(&u, &g)| (u, names[g as usize].clone()))
		.collect())
}

/// `font.getBestCmap`, with glyph names.
fn font_best(font: &mut Font, preferences: &[(u16, u16)]) -> TestResult<Vec<(u32, String)>> {
	let names = glyph_names(font)?;
	let best = font.get_best_cmap(preferences)?.ok_or("no best cmap")?;
	Ok(best
		.iter()
		.map(|(&u, &g)| (u, names[g as usize].clone()))
		.collect())
}

fn mapping(pairs: &[(u32, &str)]) -> Vec<(u32, String)> {
	pairs.iter().map(|&(u, g)| (u, g.to_owned())).collect()
}

fn glyph_names_options(notdef_glyph: bool) -> Options {
	Options {
		glyph_names: true,
		notdef_glyph,
		..Default::default()
	}
}

#[test]
fn test_dmap_precedes_cmap() -> TestResult {
	let order = [
		".notdef",
		"cmapA",
		"dmapA",
		"baseB",
		"cmapB.var",
		"dmapB.var",
	];
	let mut font = dmap_font(
		&order,
		&[(0x41, "cmapA"), (0x42, "baseB")],
		"<cmap_format_14 platformID=\"0\" platEncID=\"5\"><map uv=\"0x42\" uvs=\"0xfe00\" name=\"cmapB.var\"/></cmap_format_14>",
		"<cmap_format_4 platformID=\"3\" platEncID=\"1\" language=\"0\"><map code=\"0x41\" name=\"dmapA\"/></cmap_format_4><cmap_format_14 platformID=\"0\" platEncID=\"5\"><map uv=\"0x42\" uvs=\"0xfe00\" name=\"dmapB.var\"/></cmap_format_14>",
	)?;
	let mut font = save_and_read(&mut font)?;
	subset_with(&mut font, Options::default(), &[0x41, 0x42, 0xFE00], &[])?;
	assert_eq!(
		glyph_names(&mut font)?,
		[".notdef", "dmapA", "baseB", "dmapB.var"]
	);
	assert_eq!(best(&mut font, false)?, mapping(&[(0x42, "baseB")]));
	assert!(
		!font
			.require::<Cmap>()?
			.tables
			.iter()
			.any(|t| matches!(t.format, 14 | 15))
	);
	assert_eq!(best(&mut font, true)?, mapping(&[(0x41, "dmapA")]));
	let var = font.glyph_id("dmapB.var")?;
	let dmap = &font.require::<Dmap>()?.0;
	let t = dmap
		.tables
		.iter()
		.find(|t| matches!(t.format, 14 | 15))
		.ok_or("no UVS")?;
	let uvs = t.uvs_dict.as_ref().ok_or("no uvsDict")?;
	assert_eq!(uvs[&0xFE00], [(0x42, Some(var))]);
	let mut saved = save_and_read(&mut font)?;
	// Without glyph names, the names come from the effective character map.
	let saved_best = font_best(&mut saved, &DEFAULT_CMAP_PREFERENCES)?;
	assert_eq!(saved_best, mapping(&[(0x41, "A"), (0x42, "B")]));
	assert_eq!(saved.glyph_id("A")?, 1);
	assert_eq!(saved.glyph_id("B")?, 2);
	Ok(())
}

#[test]
fn test_dmap_nominal_zero_fallback() -> TestResult {
	for dmap_format in [4, 12, 13] {
		for glyph_zero in [".notdef", "zero"] {
			for notdef_glyph in [false, true] {
				let order = [glyph_zero, "baseA", "baseB", "dmapB"];
				let encoding = if dmap_format == 4 { 1 } else { 10 };
				let open_tag = if dmap_format == 4 {
					"<cmap_format_4 platformID=\"3\" platEncID=\"1\" language=\"0\">".to_owned()
				} else {
					format!(
						"<cmap_format_{dmap_format} platformID=\"3\" platEncID=\"{encoding}\" format=\"{dmap_format}\" reserved=\"0\" length=\"0\" language=\"0\" nGroups=\"0\">"
					)
				};
				let dmap = format!(
					"{open_tag}<map code=\"0x41\" name=\"{glyph_zero}\"/><map code=\"0x42\" name=\"dmapB\"/></cmap_format_{dmap_format}>"
				);
				let mut font = dmap_font(&order, &[(0x41, "baseA"), (0x42, "baseB")], "", &dmap)?;
				subset_with(
					&mut font,
					glyph_names_options(notdef_glyph),
					&[0x41, 0x42],
					&[],
				)?;
				let mut expected_order = Vec::new();
				if notdef_glyph {
					expected_order.push(glyph_zero);
				}
				expected_order.extend(["baseA", "dmapB"]);
				assert_eq!(glyph_names(&mut font)?, expected_order);
				let font_map = font_best(&mut font, &DEFAULT_CMAP_PREFERENCES)?;
				assert_eq!(font_map, mapping(&[(0x41, "baseA"), (0x42, "dmapB")]));
				assert_eq!(best(&mut font, true)?, mapping(&[(0x42, "dmapB")]));
				let mut reloaded = save_and_read(&mut font)?;
				// Without .notdef, baseA becomes glyph zero and binary cmap omits it.
				let expected = if notdef_glyph {
					font_map
				} else {
					mapping(&[(0x42, "dmapB")])
				};
				assert_eq!(
					font_best(&mut reloaded, &DEFAULT_CMAP_PREFERENCES)?,
					expected
				);
			}
		}
	}
	Ok(())
}

#[test]
fn test_dmap_multiple_unicode_subtables() -> TestResult {
	const PREFERENCES: [&[(u16, u16)]; 2] = [&[(3, 10), (3, 1)], &[(3, 1)]];
	for unicodes in [&[0x41, 0x42][..], &[0x42][..]] {
		let order = [".notdef", "baseA", "baseB", "dmapA", "dmapB"];
		let mut font = dmap_font(
			&order,
			&[(0x41, "baseA"), (0x42, "baseB")],
			"",
			"<cmap_format_12 platformID=\"3\" platEncID=\"10\" format=\"12\" reserved=\"0\" length=\"0\" language=\"0\" nGroups=\"0\"><map code=\"0x41\" name=\"dmapA\"/></cmap_format_12><cmap_format_4 platformID=\"3\" platEncID=\"1\" language=\"0\"><map code=\"0x42\" name=\"dmapB\"/></cmap_format_4>",
		)?;
		let mut font = save_and_read(&mut font)?;
		let expected: Vec<Vec<(u32, String)>> = PREFERENCES
			.iter()
			.map(|p| {
				Ok(font_best(&mut font, p)?
					.into_iter()
					.filter(|(u, _)| unicodes.contains(u))
					.collect())
			})
			.collect::<TestResult<_>>()?;
		subset_with(&mut font, glyph_names_options(true), unicodes, &[])?;
		let mut font = save_and_read(&mut font)?;
		for (p, expected) in PREFERENCES.iter().zip(&expected) {
			assert_eq!(&font_best(&mut font, p)?, expected, "{unicodes:x?}");
		}
	}
	Ok(())
}

#[test]
fn test_dmap_uvs_fallback() -> TestResult {
	fn uvs_table(format: u16, glyph: Option<&str>) -> String {
		let name = glyph.map_or(String::new(), |g| format!(" name=\"{g}\""));
		format!(
			"<cmap_format_{format} platformID=\"0\" platEncID=\"5\"><map uv=\"0x41\" uvs=\"0xfe00\"{name}/></cmap_format_{format}>"
		)
	}
	for cmap_format in [14, 15] {
		for dmap_format in [14, 15] {
			for glyph_zero in [".notdef", "zero"] {
				for dmap_mapping in ["zero", "default", "nonzero"] {
					let order = [glyph_zero, "baseA", "cmapA.var", "dmapA.var"];
					let dmap_glyph = match dmap_mapping {
						"zero" => Some(glyph_zero),
						"default" => None,
						_ => Some("dmapA.var"),
					};
					let mut font = dmap_font(
						&order,
						&[(0x41, "baseA")],
						&uvs_table(cmap_format, Some("cmapA.var")),
						&uvs_table(dmap_format, dmap_glyph),
					)?;
					let mut font = save_and_read(&mut font)?;
					subset_with(&mut font, glyph_names_options(true), &[0x41, 0xFE00], &[])?;
					let mut expected = vec![glyph_zero, "baseA"];
					match dmap_mapping {
						"zero" => expected.push("cmapA.var"),
						"nonzero" => expected.push("dmapA.var"),
						_ => {}
					}
					assert_eq!(glyph_names(&mut font)?, expected);
					let mut font = save_and_read(&mut font)?;
					assert_eq!(glyph_names(&mut font)?, expected);
					let cmap_var = glyph_names(&mut font)?
						.iter()
						.position(|n| n == "cmapA.var")
						.map(|g| g as u32);
					let dmap_id = dmap_glyph
						.and_then(|g| glyph_names(&mut font).ok()?.iter().position(|n| n == g))
						.map(|g| g as u32);
					let cmap = font.require::<Cmap>()?;
					let uvs: Vec<_> = cmap
						.tables
						.iter()
						.filter(|t| matches!(t.format, 14 | 15))
						.collect();
					if dmap_mapping == "zero" {
						assert_eq!(uvs.len(), 1);
						let dict = uvs[0].uvs_dict.as_ref().ok_or("no uvsDict")?;
						assert_eq!(dict[&0xFE00], [(0x41, cmap_var)]);
					} else {
						assert!(uvs.is_empty());
					}
					let dmap = &font.require::<Dmap>()?.0;
					let dict = dmap.tables[0].uvs_dict.as_ref().ok_or("no uvsDict")?;
					assert_eq!(dict[&0xFE00], [(0x41, dmap_id)]);
				}
			}
		}
	}
	Ok(())
}
