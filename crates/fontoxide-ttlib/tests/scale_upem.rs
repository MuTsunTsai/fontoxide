//! `Tests/ttLib/scaleUpem_test.py`.
//!
//! The fonts that the tests build with FontBuilder and otlLib are in
//! `tests/data/scale_upem`, made with the same code by
//! `tools/reference/reorder_scale_fixtures.py`, with fontTools' results to compare
//! with.

use std::collections::BTreeMap;
use std::path::PathBuf;

use fontoxide_pens::RecordingPen;
use fontoxide_pens::recording_pen::PenOp;
use fontoxide_ttlib::beyond64k::{ConvertOptions, upper_tables};
use fontoxide_ttlib::cff::dict::Value;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::varc::NO_VARIATION_INDEX;
use fontoxide_ttlib::scale_upem::{scale_paint, scale_upem};
use fontoxide_ttlib::tables::glyf::Number;
use fontoxide_ttlib::tables::otl::Varc;
use fontoxide_ttlib::tables::tuple_variation::Delta;
use fontoxide_ttlib::tables::{Cff, Fvar, Glyf, Gvar, Head, Hhea, Hmtx, Vorg};
use fontoxide_ttlib::var::{Location, MultiVarStoreInstancer};
use fontoxide_ttlib::{
	Font, GlyphSet, ReadOptions, TableKey, Tag, TtxImportOptions, TtxOptions, WriteOptions,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn get_path(name: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttLib/data")
		.join(name)
}

fn fixture_path(name: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data/scale_upem")
		.join(name)
}

fn read_path(path: PathBuf) -> TestResult<Font> {
	Ok(Font::read(
		std::fs::read(path)?,
		ReadOptions {
			recalc_timestamp: false,
			..Default::default()
		},
	)?)
}

fn read(name: &str) -> TestResult<Font> {
	read_path(get_path(name))
}

fn save_and_read(font: &mut Font) -> TestResult<Font> {
	Ok(Font::read(
		font.write(&WriteOptions::default())?,
		ReadOptions {
			recalc_timestamp: false,
			..Default::default()
		},
	)?)
}

fn scale_and_roundtrip(font: &mut Font) -> TestResult<Font> {
	let upem = font.require::<Head>()?.units_per_em;
	scale_upem(font, upem * 2)?;
	save_and_read(font)
}

fn axis_tags(font: &mut Font) -> TestResult<Vec<String>> {
	Ok(match font.get::<Fvar>()? {
		Some(fvar) => fvar
			.axes
			.iter()
			.map(|a| String::from_utf8_lossy(&a.axis_tag).into_owned())
			.collect(),
		None => Vec::new(),
	})
}

type Outlines = BTreeMap<String, Vec<PenOp>>;

/// `record_varc_outlines`.
fn record_varc_outlines(font: &mut Font, location: &Location) -> TestResult<Outlines> {
	let glyphs = font
		.require::<Varc>()?
		.table
		.coverage
		.as_ref()
		.map(|c| c.glyphs.clone())
		.unwrap_or_default();
	let order = font.glyph_order()?;
	let glyph_set = GlyphSet::with_location(font, location.clone())?;
	let mut result = BTreeMap::new();
	for gid in glyphs {
		let name = order.name(gid).into_owned();
		let mut pen = RecordingPen::new();
		glyph_set.draw(&name, &mut pen)?;
		result.insert(name, pen.value);
	}
	Ok(result)
}

fn approx(a: f64, b: f64) -> bool {
	(a - b).abs() <= 1e-6 * b.abs().max(1e-12)
}

/// `assert_scaled_outlines`.
fn assert_scaled_outlines(before: &Outlines, after: &Outlines) {
	assert_eq!(
		before.keys().collect::<Vec<_>>(),
		after.keys().collect::<Vec<_>>()
	);
	for (name, ops) in before {
		let new_ops = &after[name];
		assert_eq!(ops.len(), new_ops.len(), "{name}");
		for (op, new_op) in ops.iter().zip(new_ops) {
			assert_eq!(op.operator(), new_op.operator(), "{name}");
			let (points, new_points) = (op.points(), new_op.points());
			assert_eq!(points.len(), new_points.len(), "{name}");
			for (point, new_point) in points.iter().zip(&new_points) {
				match (point, new_point) {
					(None, None) => {}
					(Some((x, y)), Some((nx, ny))) => assert!(
						approx(*nx, 2.0 * x) && approx(*ny, 2.0 * y),
						"{name}: {new_point:?} != 2 * {point:?}"
					),
					_ => panic!("{name}: {point:?} vs {new_point:?}"),
				}
			}
		}
	}
}

fn all_axes_at(axes: &[String], value: f64) -> Location {
	axes.iter().map(|a| (a.clone(), value)).collect()
}

fn scale_upem_varc_outlines(filename: &str, uppercase: bool) -> TestResult {
	let mut font = read(filename)?;
	let axes = axis_tags(&mut font)?;
	let has_fvar = font.has::<Fvar>();
	let locations = if has_fvar {
		vec![Location::new(), all_axes_at(&axes, 1.0)]
	} else {
		vec![Location::new()]
	};
	if uppercase {
		upper_tables(&mut font, &ConvertOptions::default())?;
	}
	let before: Vec<Outlines> = locations
		.iter()
		.map(|loc| record_varc_outlines(&mut font, loc))
		.collect::<TestResult<_>>()?;
	let mut font = scale_and_roundtrip(&mut font)?;
	for (loc, expected) in locations.iter().zip(&before) {
		assert_scaled_outlines(expected, &record_varc_outlines(&mut font, loc)?);
	}
	if !has_fvar {
		assert!(font.require::<Varc>()?.table.multi_var_store.is_none());
		assert!(!font.contains(Tag::new(b"fvar")));
	}
	Ok(())
}

#[test]
fn test_scale_upem_varc_outlines() -> TestResult {
	for filename in [
		"varc-ac00-ac01.ttf",
		"varc-6868.ttf",
		"varc-ac01-conditional.ttf",
		"varc-static-gvar.ttf",
	] {
		for uppercase in [false, true] {
			scale_upem_varc_outlines(filename, uppercase)?;
		}
	}
	Ok(())
}

#[test]
fn test_scale_upem_first_variation_is_transform_only() -> TestResult {
	let mut font = read("varc-6868.ttf")?;
	let varc = &mut font.require::<Varc>()?.table;
	let composites = &mut varc
		.var_composite_glyphs
		.as_mut()
		.expect("VarCompositeGlyphs")
		.var_composite_glyph;
	let component = composites
		.iter()
		.flat_map(|g| &g.components)
		.find(|c| {
			c.axis_values_var_index == NO_VARIATION_INDEX
				&& c.transform_var_index != NO_VARIATION_INDEX
		})
		.cloned()
		.expect("a component with transform variations only");
	composites[0].components.insert(0, component);
	let axes = axis_tags(&mut font)?;
	let location = all_axes_at(&axes, 1.0);
	let before = record_varc_outlines(&mut font, &location)?;
	let mut font = scale_and_roundtrip(&mut font)?;
	assert_scaled_outlines(&before, &record_varc_outlines(&mut font, &location)?);
	Ok(())
}

fn condition_of(varc: &ot::VARC, wrapper_format: Option<u16>) -> ot::ConditionTableFormat2 {
	let outer = varc
		.condition_list
		.as_ref()
		.expect("ConditionList")
		.condition_table[0]
		.as_deref()
		.expect("a condition");
	let condition = match (wrapper_format, outer) {
		(None, c) => c,
		(Some(5), ot::ConditionTable::Format5(c)) => {
			c.condition_table.as_deref().expect("a condition")
		}
		(Some(3), ot::ConditionTable::Format3(c)) => {
			c.condition_table[0].as_deref().expect("a condition")
		}
		(Some(4), ot::ConditionTable::Format4(c)) => {
			c.condition_table[0].as_deref().expect("a condition")
		}
		(f, c) => panic!("wrapper {f:?}: {c:?}"),
	};
	let ot::ConditionTable::Format2(c) = condition else {
		panic!("not a format 2 condition: {condition:?}");
	};
	c.clone()
}

fn scale_upem_preserves_condition_variations(
	wrapper_format: Option<u16>,
	delta: i64,
) -> TestResult {
	let mut font = read("varc-ac01-conditional.ttf")?;
	let axes = axis_tags(&mut font)?;
	let varc = &mut font.require::<Varc>()?.table;
	let store = varc.multi_var_store.as_mut().expect("MultiVarStore");
	let data = store.multi_var_data[0].as_mut().expect("MultiVarData");
	let region_count = data.var_region_index.len();
	data.item.push(vec![delta; region_count]);
	let var_idx = (data.item.len() - 1) as u32;
	let condition = ot::ConditionTable::Format2(ot::ConditionTableFormat2 {
		default_value: -1,
		var_idx,
	});
	let outer = match wrapper_format {
		None => condition,
		Some(5) => ot::ConditionTable::Format5(ot::ConditionTableFormat5 {
			condition_table: Some(Box::new(condition)),
		}),
		Some(3) => ot::ConditionTable::Format3(ot::ConditionTableFormat3 {
			condition_count: Some(1),
			condition_table: vec![Some(Box::new(condition))],
		}),
		Some(4) => ot::ConditionTable::Format4(ot::ConditionTableFormat4 {
			condition_count: Some(1),
			condition_table: vec![Some(Box::new(condition))],
		}),
		Some(f) => panic!("format {f}"),
	};
	varc.condition_list
		.as_mut()
		.expect("ConditionList")
		.condition_table[0] = Some(Box::new(outer));
	let locations = [Location::new(), all_axes_at(&axes[..1], 1.0)];
	let before: Vec<Outlines> = locations
		.iter()
		.map(|loc| record_varc_outlines(&mut font, loc))
		.collect::<TestResult<_>>()?;
	let varc = &font.require::<Varc>()?.table;
	let values: Vec<Vec<f64>> = locations
		.iter()
		.map(|loc| {
			MultiVarStoreInstancer::new(varc.multi_var_store.as_deref(), &axes, loc.clone())
				.get(u64::from(var_idx))
		})
		.collect::<Result<_, _>>()?;

	let mut font = scale_and_roundtrip(&mut font)?;
	let axes = axis_tags(&mut font)?;
	let varc = font.require::<Varc>()?.table.clone();
	let condition = condition_of(&varc, wrapper_format);
	assert_eq!(condition.default_value, -1);
	for ((loc, expected), value) in locations.iter().zip(&before).zip(&values) {
		let deltas =
			MultiVarStoreInstancer::new(varc.multi_var_store.as_deref(), &axes, loc.clone())
				.get(u64::from(condition.var_idx))?;
		assert_eq!(
			deltas.first().copied().unwrap_or(0.0),
			value.first().copied().unwrap_or(0.0)
		);
		assert_scaled_outlines(expected, &record_varc_outlines(&mut font, loc)?);
	}
	Ok(())
}

#[test]
fn test_scale_upem_preserves_condition_variations() -> TestResult {
	for wrapper_format in [None, Some(3), Some(4), Some(5)] {
		for delta in [0, 2] {
			scale_upem_preserves_condition_variations(wrapper_format, delta)?;
		}
	}
	Ok(())
}

/// TTX of some tables, without the `ttFont` element's attributes.
fn ttx(font: &mut Font, tables: &[TableKey]) -> TestResult<String> {
	let text = font.to_ttx(&TtxOptions {
		tables: tables.to_vec(),
		..Default::default()
	})?;
	Ok(read_ttx(&text))
}

/// `read_ttx`: elides the `ttFont` attributes and trailing spaces.
fn read_ttx(text: &str) -> String {
	text.lines()
		.map(|line| {
			if line.starts_with("<ttFont ") {
				"<ttFont>".to_owned()
			} else {
				line.trim_end().to_owned()
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
}

/// `expect_ttx`.
fn expect_ttx(font: &mut Font, expected: PathBuf, tables: &[TableKey]) -> TestResult {
	let actual = ttx(font, tables)?;
	let expected = read_ttx(&std::fs::read_to_string(&expected)?);
	if actual != expected {
		for (i, (a, e)) in actual.lines().zip(expected.lines()).enumerate() {
			assert_eq!(a, e, "line {}", i + 1);
		}
		assert_eq!(actual.lines().count(), expected.lines().count());
	}
	Ok(())
}

#[test]
fn test_scale_upem_cff_default_font_matrix() -> TestResult {
	// https://github.com/fonttools/fonttools/issues/4251
	let mut font = read_path(fixture_path("cff_no_font_matrix.otf"))?;
	let top = &font.require::<Cff>()?.cff.top_dicts[0];
	assert!(!top.raw.iter().any(|(k, _)| *k == "FontMatrix"));

	// There is no shared class-level default to leave alone here.
	let mut font = scale_and_roundtrip(&mut font)?;
	assert_eq!(font.require::<Head>()?.units_per_em, 2000);
	let top = &font.require::<Cff>()?.cff.top_dicts[0];
	// The scaled matrix is written explicitly.
	assert!(top.raw.iter().any(|(k, _)| *k == "FontMatrix"));
	let Some(Value::List(matrix)) = top.get("FontMatrix") else {
		panic!("no FontMatrix");
	};
	let matrix: Vec<f64> = matrix
		.iter()
		.map(|v| v.number().expect("a number"))
		.collect();
	assert_eq!(matrix, [0.0005, 0.0, 0.0, 0.0005, 0.0, 0.0]);

	expect_ttx(
		&mut font,
		fixture_path("cff_no_font_matrix.expected.ttx"),
		&[TableKey::Table(Tag::new(b"CFF "))],
	)
}

#[test]
fn test_scale_upem_math() -> TestResult {
	let mut font = read("I.ttf")?;
	font.import_ttx_with(
		&std::fs::read_to_string(fixture_path("math.ttx"))?,
		&TtxImportOptions::default(),
	)?;

	let mut font = scale_and_roundtrip(&mut font)?;

	let math = &font.require::<fontoxide_ttlib::tables::Math>()?.table;
	let constants = math.math_constants.as_ref().expect("MathConstants");
	// Percentages and flags are not design units: they must stay as they are.
	assert_eq!(constants.script_percent_scale_down, 80);
	assert_eq!(constants.script_script_percent_scale_down, 60);
	assert_eq!(constants.radical_degree_bottom_raise_percent, 55);
	assert_eq!(constants.delimited_sub_formula_min_height, 3000);
	assert_eq!(constants.display_operator_min_height, 2600);
	let variants = math.math_variants.as_ref().expect("MathVariants");
	assert_eq!(variants.min_connector_overlap, 100);
	let [construction] = variants.vert_glyph_construction.as_slice() else {
		panic!("not one construction");
	};
	let construction = construction.as_ref().expect("a construction");
	let advances: Vec<u16> = construction
		.math_glyph_variant_record
		.iter()
		.map(|v| v.advance_measurement)
		.collect();
	assert_eq!(advances, [2000, 2800]);
	let assembly = construction.glyph_assembly.as_ref().expect("GlyphAssembly");
	let [part] = assembly.part_records.as_slice() else {
		panic!("not one part");
	};
	assert_eq!(part.start_connector_length, 40);
	assert_eq!(part.end_connector_length, 60);
	assert_eq!(part.full_advance, 800);
	assert_eq!(part.part_flags, 1);

	expect_ttx(
		&mut font,
		fixture_path("math.expected.ttx"),
		&[TableKey::Table(Tag::new(b"MATH"))],
	)
}

/// All tables but `head`, with the glyph order (`font.keys()` but `head`).
fn tables_but_head(font: &Font) -> Vec<TableKey> {
	std::iter::once(TableKey::GlyphOrder)
		.chain(
			font.tags()
				.into_iter()
				.filter(|&t| t != Tag::new(b"head"))
				.map(TableKey::Table),
		)
		.collect()
}

#[test]
fn test_scale_upem_ttf() -> TestResult {
	let mut font = read("I.ttf")?;
	let tables = tables_but_head(&font);

	scale_upem(&mut font, 512)?;

	expect_ttx(&mut font, get_path("I-512upem.ttx"), &tables)
}

#[test]
fn test_scale_upem_beyond64k_ttf() -> TestResult {
	let mut font = read("I.ttf")?;
	upper_tables(&mut font, &ConvertOptions::default())?;

	scale_upem(&mut font, 512)?;

	let i = font.glyph_id("I")?;
	assert_eq!(font.require::<Head>()?.units_per_em, 512);
	let hhea = font.require::<Hhea>()?;
	assert!(hhea.extended);
	assert_eq!((hhea.ascent, hhea.descent), (475, -125));
	assert_eq!(font.require::<Hmtx>()?.get(i), Some((136, 44)));
	let glyph = &font.require::<Glyf>()?.glyphs[&i];
	assert_eq!((glyph.x_min, glyph.x_max), (44, 92));
	let gvar = font.require::<Gvar>()?;
	assert!(gvar.extended);
	assert_eq!(
		gvar.get(i).expect("variations")[0].coordinates[1],
		Some(Delta::Point(Number::Int(-24), Number::Int(0)))
	);

	// Save / load to ensure calculated companion-table values are valid.
	let mut font = save_and_read(&mut font)?;
	for tag in [b"GLYF", b"LOCA", b"MAXP", b"HHEA", b"HMTX"] {
		assert!(font.contains(Tag::new(tag)));
	}
	assert_eq!(font.require::<Hmtx>()?.get(i), Some((136, 44)));
	Ok(())
}

#[test]
fn test_scale_upem_otf() -> TestResult {
	// Just test that it doesn't crash
	let mut font = read("TestVGID-Regular.otf")?;
	scale_upem(&mut font, 500)?;
	Ok(())
}

#[test]
#[allow(non_snake_case)]
fn test_scale_upem_varComposite() -> TestResult {
	let mut font = read("varc-ac00-ac01.ttf")?;
	let tables = tables_but_head(&font);

	scale_upem(&mut font, 500)?;

	// Save / load to ensure calculated values are correct
	font.write(&WriteOptions::default())?;

	expect_ttx(&mut font, get_path("varc-ac00-ac01-500upem.ttx"), &tables)?;

	// Scale our other varComposite font as well; without checking the expected
	let mut font = read("varc-6868.ttf")?;
	scale_upem(&mut font, 500)?;
	Ok(())
}

#[test]
fn test_scale_upem_vorg() -> TestResult {
	let mut font = read("TestVGID-Regular.otf")?;
	let mut vorg = Vorg {
		major_version: 1,
		minor_version: 0,
		default_vert_origin_y: 800,
		..Default::default()
	};
	vorg.v_origin_records = BTreeMap::from([(1, 900), (2, -321)]);
	font.insert(Box::new(vorg));

	let upem = font.require::<Head>()?.units_per_em;
	scale_upem(&mut font, upem * 2)?;

	let mut font = save_and_read(&mut font)?;
	let vorg = font.require::<Vorg>()?;
	assert_eq!(vorg.default_vert_origin_y, 1600);
	assert_eq!(vorg.get(0), 1600);
	assert_eq!(
		vorg.v_origin_records,
		BTreeMap::from([(1, 1800), (2, -642)])
	);
	Ok(())
}

#[test]
fn test_scale_upem_paint_glyph2() {
	let mut paint = ot::Paint::Format33(ot::PaintFormat33 {
		glyph: 70000,
		paint: Some(Box::new(ot::Paint::Format2(ot::PaintFormat2 {
			palette_index: 0,
			alpha: 1.0,
		}))),
	});

	scale_paint(&mut paint, 2.0);

	let ot::Paint::Format20(scale) = paint else {
		panic!("not a PaintScaleUniform: {paint:?}");
	};
	let Some(ot::Paint::Format33(glyph)) = scale.paint.as_deref() else {
		panic!("not a PaintGlyph2");
	};
	assert_eq!(glyph.glyph, 70000);
}
