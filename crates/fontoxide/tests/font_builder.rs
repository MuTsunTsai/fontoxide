//! `Tests/fontBuilder/fontBuilder_test.py`.

#![cfg(all(
	feature = "designspacelib",
	feature = "fealib",
	feature = "otllib",
	feature = "varlib",
	feature = "repacker"
))]

use std::collections::BTreeMap;

use fontoxide::font_builder::{FontBuilder, FvarAxis, FvarInstance, NameKey, NameValue, Value};
use fontoxide::misc::Tag;
use fontoxide::otllib::builder::{StatAxis, StatAxisValue, StatAxisValueKind, StatName};
use fontoxide::pens::recording_pen::{PenOp, RecordingPen};
use fontoxide::pens::{BoundsPen, Pen};
use fontoxide::ttlib::cff::charstring::{T2CharString, Token};
use fontoxide::ttlib::pens::{T2CharStringPen, TtGlyphPen};
use fontoxide::ttlib::tables::cmap::DEFAULT_CMAP_PREFERENCES;
use fontoxide::ttlib::tables::glyf::{Glyph, Number};
use fontoxide::ttlib::tables::os2::Panose;
use fontoxide::ttlib::tables::tuple_variation::{Delta, TupleVariation};
use fontoxide::ttlib::tables::{Cmap, Glyf, Hmtx, Name, Os2, Post};
use fontoxide::ttlib::{Font, ReadOptions, TableKey, TtxOptions};
use fontoxide::varlib::avar_builder::AvarAxis;
use indexmap::IndexMap;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn data_path(name: &str) -> std::path::PathBuf {
	std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/fontBuilder/data")
		.join(name)
}

fn s(v: &str) -> String {
	v.to_owned()
}

fn draw_test_glyph(pen: &mut dyn Pen, cubic: bool) -> fontoxide::pens::Result<()> {
	pen.move_to((100.0, 100.0))?;
	pen.line_to((100.0, 1000.0))?;
	if cubic {
		pen.curve_to(&[(200.0, 900.0), (400.0, 900.0), (500.0, 1000.0)])?;
	} else {
		pen.q_curve_to(&[(200.0, 900.0), (400.0, 900.0)], Some((500.0, 1000.0)))?;
	}
	pen.line_to((500.0, 100.0))?;
	pen.close_path()
}

/// `_setupFontBuilder`.
/// The builder, the advance widths and the names of `_setupFontBuilder`.
type Setup = (
	FontBuilder,
	IndexMap<String, f64>,
	Vec<(NameKey, NameValue)>,
);

fn setup_font_builder(
	is_ttf: bool,
	units_per_em: u16,
) -> Result<Setup, Box<dyn std::error::Error>> {
	let mut fb = FontBuilder::new(units_per_em, is_ttf, false)?;
	fb.setup_glyph_order(vec![s(".notdef"), s(".null"), s("A"), s("a")]);
	fb.setup_character_map(
		&[(65, s("A")), (97, s("a"))].into_iter().collect(),
		None,
		false,
	)?;
	let advance_widths: IndexMap<String, f64> = [
		(s(".notdef"), 600.0),
		(s("A"), 600.0),
		(s("a"), 600.0),
		(s(".null"), 600.0),
	]
	.into_iter()
	.collect();
	let name_strings = vec![
		(
			NameKey::from("familyName"),
			NameValue::Multilingual(vec![
				(s("en"), s("HelloTestFont")),
				(s("nl"), s("HalloTestFont")),
			]),
		),
		(
			NameKey::from("styleName"),
			NameValue::Multilingual(vec![
				(s("en"), s("TotallyNormal")),
				(s("nl"), s("TotaalNormaal")),
			]),
		),
		(
			NameKey::from("psName"),
			NameValue::from("HelloTestFont-TotallyNormal"),
		),
	];
	Ok((fb, advance_widths, name_strings))
}

/// `_setupFontBuilderFvar`.
fn setup_font_builder_fvar(fb: &mut FontBuilder) -> TestResult {
	let axis = AvarAxis {
		tag: Tag::new(b"TEST"),
		name: s("Test Axis"),
		minimum: 0.0,
		default: 0.0,
		maximum: 100.0,
		map: vec![(0.0, 0.0), (40.0, 60.0), (100.0, 100.0)],
	};
	let axes = [FvarAxis {
		tag: axis.tag,
		minimum: axis.minimum,
		default: axis.default,
		maximum: axis.maximum,
		name: NameValue::from("Test Axis"),
		hidden: false,
	}];
	let instances = [
		FvarInstance {
			location: vec![(Tag::new(b"TEST"), 0.0)],
			style_name: NameValue::from("TotallyNormal"),
			postscript_font_name: None,
		},
		FvarInstance {
			location: vec![(Tag::new(b"TEST"), 100.0)],
			style_name: NameValue::from("TotallyTested"),
			postscript_font_name: None,
		},
	];
	fb.setup_fvar(&axes, &instances)?;
	fb.setup_avar(&[axis])?;
	Ok(())
}

fn ops(names: &[&str]) -> Vec<Token> {
	names
		.iter()
		.map(|n| match n.parse::<i64>() {
			Ok(v) => Token::Int(v),
			Err(_) => Token::Op(match *n {
				"blend" => "blend",
				"rmoveto" => "rmoveto",
				"hlineto" => "hlineto",
				"vlineto" => "vlineto",
				other => panic!("{other}"),
			}),
		})
		.collect()
}

/// `_setupFontBuilderCFF2`.
fn setup_font_builder_cff2(fb: &mut FontBuilder) -> TestResult {
	let mut pen = T2CharStringPen::new(None, None, 0.5, true)?;
	draw_test_glyph(&mut pen, false)?;
	let char_string = pen.get_char_string(true)?;
	let program = ops(&[
		"200", "200", "-200", "-200", "2", "blend", "rmoveto", "400", "400", "1", "blend",
		"hlineto", "400", "400", "1", "blend", "vlineto", "-400", "-400", "1", "blend", "hlineto",
	]);
	let variable = T2CharString {
		bytecode: None,
		program,
		fd_select_index: None,
	};
	let char_strings: IndexMap<String, T2CharString> = [
		(s(".notdef"), char_string.clone()),
		(s("A"), char_string.clone()),
		(s("a"), variable),
		(s(".null"), char_string),
	]
	.into_iter()
	.collect();
	let region: BTreeMap<Tag, (f64, f64, f64)> =
		[(Tag::new(b"TEST"), (0.0, 1.0, 1.0))].into_iter().collect();
	fb.setup_cff2(char_strings, None, Some(&[region]))?;
	Ok(())
}

/// `stripVariableItemsFromTTX`.
fn strip_variable_items(ttx: &str) -> String {
	let mut out = String::new();
	for line in ttx.split_inclusive('\n') {
		let mut l = line.to_owned();
		if let Some(start) = l.find(" ttLibVersion=\"") {
			let end = l[start + 15..]
				.find('"')
				.map_or(l.len(), |e| start + 15 + e + 1);
			l.replace_range(start..end, "");
		}
		for tag in ["checkSumAdjustment", "modified", "created"] {
			let open = format!("<{tag} value=\"");
			if let Some(start) = l.find(&open) {
				let end = l[start..].find("/>").map_or(l.len(), |e| start + e + 2);
				l.replace_range(start..end, "");
			}
		}
		out.push_str(&l);
	}
	out
}

/// `_verifyOutput`: the saved font, read back, dumps the TTX of the test data.
fn verify_output(fb: &mut FontBuilder, file: &str, tables: Option<&[&str]>) -> TestResult {
	let data = fb.save()?;
	let mut font = Font::read(data, ReadOptions::default())?;
	let options = TtxOptions {
		tables: tables
			.unwrap_or(&[])
			.iter()
			.map(|t| TableKey::Table(Tag::from_str_padded(t).expect("tag")))
			.collect(),
		..Default::default()
	};
	let ttx = strip_variable_items(&font.to_ttx(&options)?);
	let expected = std::fs::read_to_string(data_path(file))?.replace("\r\n", "\n");
	let expected = strip_variable_items(&expected);
	if ttx != expected {
		let a: Vec<&str> = ttx.lines().collect();
		let b: Vec<&str> = expected.lines().collect();
		let i = a
			.iter()
			.zip(&b)
			.position(|(x, y)| x != y)
			.unwrap_or(a.len().min(b.len()));
		let lo = i.saturating_sub(3);
		panic!(
			"{file}: first difference at line {i}
--- got:
{}
--- expected:
{}",
			a[lo..(i + 4).min(a.len())].join(
				"
"
			),
			b[lo..(i + 4).min(b.len())].join(
				"
"
			)
		);
	}
	Ok(())
}

fn ttglyph(
	order: &fontoxide::ttlib::GlyphOrder,
	draw: impl FnOnce(&mut TtGlyphPen<'_>) -> fontoxide::pens::Result<()>,
) -> Result<Glyph, Box<dyn std::error::Error>> {
	let mut pen = TtGlyphPen::new(None);
	draw(&mut pen)?;
	Ok(pen.glyph(order)?)
}

fn metrics_from_glyf(
	fb: &mut FontBuilder,
	widths: &IndexMap<String, f64>,
) -> Result<IndexMap<String, (f64, f64)>, Box<dyn std::error::Error>> {
	let order = fb.font.glyph_order()?;
	let glyf = fb.font.require::<Glyf>()?;
	let mut metrics = IndexMap::new();
	for (gn, &w) in widths {
		let gid = order.id(gn).expect("glyph");
		let x_min = glyf.glyphs.get(&gid).map_or(0, |g| g.x_min);
		metrics.insert(gn.clone(), (w, x_min as f64));
	}
	Ok(metrics)
}

fn ascent_descent() -> Vec<(&'static str, Value)> {
	vec![("ascent", Value::Int(824)), ("descent", Value::Int(200))]
}

#[test]
fn test_build_ttf() -> TestResult {
	let (mut fb, advance_widths, name_strings) = setup_font_builder(true, 1024)?;
	let order = fb.font.glyph_order()?;
	let glyph = ttglyph(&order, |p| draw_test_glyph(p, false))?;
	let glyphs: IndexMap<String, Glyph> = [".notdef", "A", "a", ".null"]
		.iter()
		.map(|n| (s(n), glyph.clone()))
		.collect();
	fb.setup_glyf(glyphs, true)?;
	let metrics = metrics_from_glyf(&mut fb, &advance_widths)?;
	fb.setup_horizontal_metrics(&metrics)?;
	fb.setup_horizontal_header(&ascent_descent())?;
	fb.setup_name_table(&name_strings, true, true)?;
	fb.setup_os2(&[])?;
	fb.add_open_type_features("feature salt { sub A by a; } salt;", None, None, false)?;
	fb.setup_post(true, &[])?;
	fb.setup_dummy_dsig();
	verify_output(&mut fb, "test.ttf.ttx", None)
}

/// `_setupBeyond64kFontBuilder`.
fn setup_beyond64k_font_builder() -> Result<FontBuilder, Box<dyn std::error::Error>> {
	let mut fb = FontBuilder::new(1024, true, true)?;
	fb.setup_glyph_order(vec![s(".notdef"), s("A")]);
	fb.setup_character_map(&[(65, s("A"))].into_iter().collect(), None, false)?;
	let order = fb.font.glyph_order()?;
	let empty = ttglyph(&order, |_| Ok(()))?;
	let glyph = ttglyph(&order, |p| {
		p.move_to((50.0, 0.0))?;
		p.line_to((50.0, 200.0))?;
		p.line_to((250.0, 200.0))?;
		p.line_to((250.0, 0.0))?;
		p.close_path()
	})?;
	fb.setup_glyf(
		[(s(".notdef"), empty), (s("A"), glyph)]
			.into_iter()
			.collect(),
		true,
	)?;
	let x_min = fb.font.require::<Glyf>()?.glyphs[&1].x_min;
	fb.setup_horizontal_metrics(
		&[
			(s(".notdef"), (600.0, 0.0)),
			(s("A"), (500.0, x_min as f64)),
		]
		.into_iter()
		.collect(),
	)?;
	fb.setup_horizontal_header(&ascent_descent())?;
	fb.setup_name_table(
		&[
			(NameKey::from("familyName"), NameValue::from("Upper")),
			(NameKey::from("styleName"), NameValue::from("Regular")),
		],
		true,
		true,
	)?;
	fb.setup_os2(&[])?;
	fb.setup_post(true, &[])?;
	Ok(fb)
}

fn tags(font: &Font) -> Vec<String> {
	font.tags().iter().map(|t| t.as_string()).collect()
}

fn check_upper(font: &Font) {
	let tags = tags(font);
	for t in ["GLYF", "LOCA", "MAXP", "HHEA", "HMTX"] {
		assert!(tags.contains(&s(t)), "{t} in {tags:?}");
	}
	for t in ["glyf", "loca", "maxp", "hhea", "hmtx"] {
		assert!(!tags.contains(&s(t)), "{t} in {tags:?}");
	}
}

#[test]
fn test_build_beyond64k_ttf() -> TestResult {
	let mut fb = setup_beyond64k_font_builder()?;
	check_upper(&fb.font);
	assert_eq!(fb.font.require::<Os2>()?.x_avg_char_width, 550);
	let data = fb.save()?;
	let font = Font::read(data, ReadOptions::default())?;
	check_upper(&font);
	Ok(())
}

#[test]
fn test_build_beyond64k_ttf_xml_round_trip() -> TestResult {
	let mut fb = setup_beyond64k_font_builder()?;
	let ttx = fb.font.to_ttx(&TtxOptions::default())?;
	let mut font = Font::new();
	font.import_ttx(&ttx)?;
	check_upper(&font);
	let data = font.write(&Default::default())?;
	let mut font = Font::read(data, ReadOptions::default())?;
	assert_eq!(font.glyph_order()?.names(), [s(".notdef"), s("A")]);
	assert_eq!(font.require::<Hmtx>()?.get(1), Some((500, 50)));
	assert_eq!(font.require::<Glyf>()?.glyphs[&1].x_min, 50);
	Ok(())
}

#[test]
fn test_setup_gvar_uses_selected_table_family() -> TestResult {
	let mut fb = FontBuilder::new(1024, true, false)?;
	fb.setup_gvar(IndexMap::new())?;
	assert!(tags(&fb.font).contains(&s("gvar")));
	assert!(!tags(&fb.font).contains(&s("GVAR")));

	let mut fb = FontBuilder::new(1024, true, true)?;
	fb.setup_gvar(IndexMap::new())?;
	assert!(tags(&fb.font).contains(&s("GVAR")));
	assert!(!tags(&fb.font).contains(&s("gvar")));
	Ok(())
}

fn cubic_glyph(
	order: &fontoxide::ttlib::GlyphOrder,
	points: [(f64, f64); 4],
) -> Result<Glyph, Box<dyn std::error::Error>> {
	ttglyph(order, |p| {
		p.move_to(points[0])?;
		p.curve_to(&points[1..])?;
		p.close_path()
	})
}

#[test]
fn test_build_cubic_ttf() -> TestResult {
	// Cubic outlines are not allowed in the glyf table.
	let mut fb = FontBuilder::new(1000, true, false)?;
	fb.setup_glyph_order(vec![s("A")]);
	let order = fb.font.glyph_order()?;
	let glyph = cubic_glyph(
		&order,
		[
			(100.0, 100.0),
			(200.0, 200.0),
			(300.0, 300.0),
			(400.0, 400.0),
		],
	)?;
	let err = fb
		.setup_glyf([(s("A"), glyph)].into_iter().collect(), true)
		.expect_err("cubic");
	assert!(
		err.to_string()
			.contains("Glyph 'A' has cubic Bezier outlines"),
		"{err}"
	);
	Ok(())
}

#[test]
fn test_build_cubic_beyond64k_ttf() -> TestResult {
	let mut fb = setup_beyond64k_font_builder()?;
	let order = fb.font.glyph_order()?;
	let notdef = fb.font.require::<Glyf>()?.glyphs[&0].clone();
	let glyph = cubic_glyph(
		&order,
		[
			(100.0, 100.0),
			(200.0, 300.0),
			(300.0, 300.0),
			(400.0, 100.0),
		],
	)?;
	fb.setup_glyf(
		[(s(".notdef"), notdef), (s("A"), glyph)]
			.into_iter()
			.collect(),
		true,
	)?;
	let data = fb.save()?;
	let mut font = Font::read(data, ReadOptions::default())?;
	let order = font.glyph_order()?;
	let mut pen = RecordingPen::new();
	let glyph = font.require::<Glyf>()?.glyphs[&1].clone();
	glyph.draw(&order, &mut pen, 0.0)?;
	assert_eq!(
		pen.value,
		[
			PenOp::MoveTo((100.0, 100.0)),
			PenOp::CurveTo(vec![(200.0, 300.0), (300.0, 300.0), (400.0, 100.0)]),
			PenOp::ClosePath,
		]
	);
	Ok(())
}

#[test]
fn test_build_otf() -> TestResult {
	let (mut fb, advance_widths, name_strings) = setup_font_builder(false, 1024)?;
	let mut pen = T2CharStringPen::new(Some(600.0), None, 0.5, false)?;
	draw_test_glyph(&mut pen, false)?;
	let char_string = pen.get_char_string(true)?;
	let char_strings: IndexMap<String, T2CharString> = [".notdef", "A", "a", ".null"]
		.iter()
		.map(|n| (s(n), char_string.clone()))
		.collect();
	fb.setup_cff(
		"HelloTestFont-TotallyNormal",
		&[(
			"FullName",
			fontoxide::ttlib::cff::dict::Value::Str(s("HelloTestFont-TotallyNormal")),
		)],
		char_strings,
		&[],
	)?;
	// `cs.calcBounds(None)[0]`: the bounds of the drawing.
	let mut bounds = BoundsPen::new_bounds(None);
	draw_test_glyph(&mut bounds, false)?;
	let lsb = bounds.bounds().expect("bounds")[0];
	let metrics: IndexMap<String, (f64, f64)> = advance_widths
		.iter()
		.map(|(gn, &w)| (gn.clone(), (w, lsb)))
		.collect();
	fb.setup_horizontal_metrics(&metrics)?;
	fb.setup_horizontal_header(&ascent_descent())?;
	fb.setup_name_table(&name_strings, true, true)?;
	fb.setup_os2(&[])?;
	fb.add_open_type_features("feature kern { pos A a -50; } kern;", None, None, false)?;
	fb.setup_post(true, &[])?;
	fb.setup_dummy_dsig();
	verify_output(&mut fb, "test.otf.ttx", None)
}

#[test]
fn test_build_cff_to_cff2() -> TestResult {
	let (mut fb, _, _) = setup_font_builder(false, 1000)?;
	let mut pen = T2CharStringPen::new(Some(600.0), None, 0.5, false)?;
	draw_test_glyph(&mut pen, false)?;
	let char_string = pen.get_char_string(true)?;
	let char_strings: IndexMap<String, T2CharString> = [".notdef", "A", "a", ".null"]
		.iter()
		.map(|n| (s(n), char_string.clone()))
		.collect();
	fb.setup_cff("TestFont", &[], char_strings, &[])?;

	fontoxide::ttlib::cff::cff_to_cff2::convert_cff_to_cff2(&mut fb.font)?;
	Ok(())
}

/// Not an upstream test: `setupAvar` with mappings, as fontTools builds it (the
/// mappings keyed by axis index, see `setup_avar_with_mappings`).
#[test]
fn setup_avar_with_mappings_builds_avar2() -> TestResult {
	use fontoxide::designspacelib::{AxisDescriptor, AxisMappingDescriptor, Number};
	let mut fb = FontBuilder::new(1000, true, false)?;
	fb.setup_glyph_order(vec![s(".notdef")]);
	fb.setup_name_table(
		&[
			(NameKey::from("familyName"), NameValue::from("T")),
			(NameKey::from("styleName"), NameValue::from("R")),
		],
		true,
		true,
	)?;
	let fvar_axis = FvarAxis {
		tag: Tag::new(b"wght"),
		minimum: 100.0,
		default: 400.0,
		maximum: 900.0,
		name: NameValue::from("Weight"),
		hidden: false,
	};
	fb.setup_fvar(&[fvar_axis], &[])?;
	let axis = AxisDescriptor {
		tag: Some(s("wght")),
		name: Some(s("Weight")),
		minimum: Number::Int(100),
		default: Number::Int(400),
		maximum: Number::Int(900),
		map: vec![
			(Number::Int(100), Number::Int(100)),
			(Number::Int(400), Number::Int(450)),
			(Number::Int(900), Number::Int(900)),
		],
		..Default::default()
	};
	let location = |v: i64| [(s("0"), Number::Int(v))].into_iter().collect();
	let mapping = AxisMappingDescriptor {
		input_location: location(600),
		output_location: location(700),
		..Default::default()
	};
	// By axis name, as in fontTools, the axis is not found.
	let mut by_name = mapping.clone();
	by_name.input_location = [(s("Weight"), Number::Int(600))].into_iter().collect();
	let err = fb
		.setup_avar_with_mappings(std::slice::from_ref(&axis), &[by_name])
		.expect_err("a KeyError");
	assert!(err.to_string().contains("KeyError"), "{err}");

	fb.setup_avar_with_mappings(&[axis], &[mapping])?;
	let ttx = fb.font.to_ttx(&TtxOptions {
		tables: vec![TableKey::Table(Tag::new(b"avar"))],
		newline: "\n".into(),
		..Default::default()
	})?;
	let expected = r#"  <avar>
    <version major="2" minor="0"/>
    <segment axis="wght">
      <mapping from="-1.0" to="-1.0"/>
      <mapping from="0.0" to="0.0"/>
      <mapping from="1.0" to="1.0"/>
    </segment>
    <VarStore Format="1">
      <Format value="1"/>
      <VarRegionList>
        <!-- RegionAxisCount=1 -->
        <!-- RegionCount=1 -->
        <Region index="0">
          <VarRegionAxis index="0">
            <StartCoord value="0.0"/>
            <PeakCoord value="0.3333"/>
            <EndCoord value="1.0"/>
          </VarRegionAxis>
        </Region>
      </VarRegionList>
      <!-- VarDataCount=1 -->
      <VarData index="0">
        <!-- ItemCount=1 -->
        <NumShorts value="1"/>
        <!-- VarRegionCount=1 -->
        <VarRegionIndex index="0" value="0"/>
        <Item index="0" value="[3641]"/>
      </VarData>
    </VarStore>
  </avar>
"#;
	assert!(ttx.contains(expected), "{ttx}");
	Ok(())
}

#[test]
fn test_build_var() -> TestResult {
	let (mut fb, advance_widths, name_strings) = setup_font_builder(true, 1024)?;
	let order = fb.font.glyph_order()?;
	let rect = |p: &mut TtGlyphPen<'_>, x0: f64, x1: f64, y1: f64| {
		p.move_to((x0, 0.0))?;
		p.line_to((x0, y1))?;
		p.line_to((x1, y1))?;
		p.line_to((x1, 0.0))?;
		p.close_path()
	};
	let glyph1 = ttglyph(&order, |p| rect(p, 100.0, 500.0, 400.0))?;
	let glyph2 = ttglyph(&order, |p| rect(p, 50.0, 250.0, 200.0))?;
	let empty = ttglyph(&order, |_| Ok(()))?;
	fb.setup_glyf(
		[
			(s(".notdef"), empty.clone()),
			(s("A"), glyph1),
			(s("a"), glyph2),
			(s(".null"), empty),
		]
		.into_iter()
		.collect(),
		true,
	)?;
	let metrics = metrics_from_glyf(&mut fb, &advance_widths)?;
	fb.setup_horizontal_metrics(&metrics)?;
	fb.setup_horizontal_header(&ascent_descent())?;
	fb.setup_name_table(&name_strings, true, false)?;

	let axes = [
		("LEFT", "Left"),
		("RGHT", "Right"),
		("UPPP", "Up"),
		("DOWN", "Down"),
	];
	let fvar_axes: Vec<FvarAxis> = axes
		.iter()
		.map(|(tag, name)| FvarAxis {
			tag: Tag::from_str_exact(tag).expect("tag"),
			minimum: 0.0,
			default: 0.0,
			maximum: 100.0,
			name: NameValue::from(*name),
			hidden: false,
		})
		.collect();
	let location = |v: [f64; 4]| -> Vec<(Tag, f64)> {
		axes.iter()
			.zip(v)
			.map(|((t, _), v)| (Tag::from_str_exact(t).expect("tag"), v))
			.collect()
	};
	let instances = [
		FvarInstance {
			location: location([0.0, 0.0, 0.0, 0.0]),
			style_name: NameValue::from("TotallyNormal"),
			postscript_font_name: None,
		},
		FvarInstance {
			location: location([0.0, 100.0, 100.0, 0.0]),
			style_name: NameValue::from("Right Up"),
			postscript_font_name: None,
		},
	];
	fb.setup_fvar(&fvar_axes, &instances)?;
	// Four (x, y) pairs and four phantom points.
	let deltas = |d: [(i64, i64); 4]| -> Vec<Option<Delta>> {
		d.iter()
			.map(|&(x, y)| Some(Delta::Point(Number::Int(x), Number::Int(y))))
			.chain([None, None, None, None])
			.collect()
	};
	let variation = |axis: &str, coordinates: Vec<Option<Delta>>| TupleVariation {
		axes: [(s(axis), (0.0, 1.0, 1.0))].into_iter().collect(),
		coordinates,
	};
	let variations: IndexMap<String, Vec<TupleVariation>> = [(
		s("a"),
		vec![
			variation("RGHT", deltas([(0, 0), (0, 0), (200, 0), (200, 0)])),
			variation("LEFT", deltas([(-200, 0), (-200, 0), (0, 0), (0, 0)])),
			variation("UPPP", deltas([(0, 0), (0, 200), (0, 200), (0, 0)])),
			variation("DOWN", deltas([(0, -200), (0, 0), (0, 0), (0, -200)])),
		],
	)]
	.into_iter()
	.collect();
	fb.setup_gvar(variations)?;
	let condition = |a: &str, b: &str| -> IndexMap<String, (f64, f64)> {
		[(s(a), (0.8, 1.0)), (s(b), (0.8, 1.0))]
			.into_iter()
			.collect()
	};
	fb.add_feature_variations(
		&[(
			vec![condition("LEFT", "DOWN"), condition("RGHT", "UPPP")],
			[(s("A"), s("a"))].into_iter().collect(),
		)],
		Tag::new(b"rclt"),
	)?;

	let stat_axes: Vec<StatAxis> = axes
		.iter()
		.map(|(tag, name)| StatAxis {
			tag: Tag::from_str_exact(tag).expect("tag"),
			name: StatName::from(*name),
			ordering: None,
			values: vec![
				StatAxisValue {
					name: StatName::from("Neutral"),
					flags: 0x2,
					kind: StatAxisValueKind::Value {
						value: 0.0,
						linked_value: None,
					},
				},
				StatAxisValue {
					name: StatName::from(*name),
					flags: 0,
					kind: StatAxisValueKind::Value {
						value: 100.0,
						linked_value: None,
					},
				},
			],
		})
		.collect();
	fb.setup_stat(&stat_axes, None, &StatName::Id(2))?;
	fb.setup_os2(&[])?;
	fb.setup_post(true, &[])?;
	fb.setup_dummy_dsig();
	verify_output(&mut fb, "test_var.ttf.ttx", None)
}

#[test]
fn test_build_cff2() -> TestResult {
	let (mut fb, advance_widths, name_strings) = setup_font_builder(false, 1000)?;
	fb.setup_name_table(&name_strings, true, true)?;
	setup_font_builder_fvar(&mut fb)?;
	setup_font_builder_cff2(&mut fb)?;
	let metrics: IndexMap<String, (f64, f64)> = advance_widths
		.iter()
		.map(|(gn, &w)| (gn.clone(), (w, 0.0)))
		.collect();
	fb.setup_horizontal_metrics(&metrics)?;
	fb.setup_horizontal_header(&ascent_descent())?;
	fb.setup_os2(&[
		("sTypoAscender", Value::Int(825)),
		("sTypoDescender", Value::Int(200)),
		("usWinAscent", Value::Int(824)),
		("usWinDescent", Value::Int(200)),
	])?;
	fb.setup_post(true, &[])?;
	verify_output(&mut fb, "test_var.otf.ttx", None)
}

fn platform_ids(fb: &mut FontBuilder) -> Result<Vec<u16>, Box<dyn std::error::Error>> {
	Ok(fb
		.font
		.require::<Name>()?
		.names
		.iter()
		.map(|n| n.platform_id)
		.collect())
}

#[test]
fn test_setup_name_table_no_mac() -> TestResult {
	let (mut fb, _, name_strings) = setup_font_builder(true, 1024)?;
	fb.setup_name_table(&name_strings, true, false)?;
	let ids = platform_ids(&mut fb)?;
	assert!(ids.contains(&3));
	assert!(!ids.contains(&1));
	Ok(())
}

#[test]
fn test_setup_name_table_no_windows() -> TestResult {
	let (mut fb, _, name_strings) = setup_font_builder(true, 1024)?;
	fb.setup_name_table(&name_strings, false, true)?;
	let ids = platform_ids(&mut fb)?;
	assert!(ids.contains(&1));
	assert!(!ids.contains(&3));
	Ok(())
}

#[test]
fn test_setup_post() -> TestResult {
	for (is_ttf, keep_glyph_names, make_cff2, post_format) in [
		(true, true, false, 2.0),
		(true, false, false, 3.0),
		(false, true, false, 3.0),
		(false, false, false, 3.0),
		(false, true, true, 2.0),
		(false, false, true, 3.0),
	] {
		let (mut fb, _, name_strings) = setup_font_builder(is_ttf, 1024)?;
		if make_cff2 {
			fb.setup_name_table(&name_strings, true, true)?;
			setup_font_builder_fvar(&mut fb)?;
			setup_font_builder_cff2(&mut fb)?;
		}
		fb.setup_post(keep_glyph_names, &[])?;
		assert_eq!(fb.is_ttf, is_ttf);
		assert_eq!(fb.font.contains(Tag::new(b"CFF2")), make_cff2);
		assert_eq!(fb.font.require::<Post>()?.format_type, post_format);
	}
	Ok(())
}

#[test]
fn test_setup_post_beyond64k_count_limit() -> TestResult {
	for count in [0xFFFF_u32, 0x10000, 0x10001] {
		for cff2 in [false, true] {
			let mut fb = FontBuilder::new(1024, !cff2, true)?;
			let order: Vec<String> = std::iter::once(s(".notdef"))
				.chain((1..count).map(|i| format!("glyph{i}")))
				.collect();
			fb.setup_glyph_order(order.clone());
			if cff2 {
				fb.font
					.insert(Box::new(fontoxide::ttlib::tables::Cff2::default()));
			}
			fb.setup_post(true, &[])?;
			let format = fb.font.require::<Post>()?.format_type;
			assert_eq!(format, if count <= 0xFFFF { 2.0 } else { 3.0 });
			fb.font
				.require::<fontoxide::ttlib::tables::Maxp>()?
				.num_glyphs = count;
			if format == 2.0 {
				// Isolate the glyph count limit from post's separate limit of name
				// indices.
				fb.font.require::<Post>()?.mapping =
					Some(order.iter().map(|n| (n.clone(), s("space"))).collect());
			}
			let data = fb.font.table_data(Tag::new(b"post"))?;
			let reloaded = <Post as fontoxide::ttlib::TableType>::decompile(&data, &mut fb.font)?;
			assert_eq!(reloaded.format_type, format);
		}
	}
	Ok(())
}

fn reload_cmap(fb: &mut FontBuilder) -> Result<Cmap, Box<dyn std::error::Error>> {
	let data = fb.font.table_data(Tag::new(b"cmap"))?;
	Ok(<Cmap as fontoxide::ttlib::TableType>::decompile(
		&data,
		&mut fb.font,
	)?)
}

#[test]
fn test_character_map_high_glyph_ids() -> TestResult {
	for supplementary in [false, true] {
		for gid in [0xFFFF_u32, 0x10000, 0x10001] {
			let mut fb = FontBuilder::new(1024, true, true)?;
			let order: Vec<String> = std::iter::once(s(".notdef"))
				.chain((1..=gid).map(|i| format!("glyph{i}")))
				.collect();
			fb.setup_glyph_order(order.clone());
			let mut mapping: BTreeMap<u32, String> =
				[(65, order[gid as usize].clone()), (66, order[1].clone())]
					.into_iter()
					.collect();
			if supplementary {
				mapping.insert(0x10000, order[gid as usize].clone());
			}
			fb.setup_character_map(&mapping, None, false)?;
			let reloaded = reload_cmap(&mut fb)?;
			let best: BTreeMap<u32, u32> = reloaded
				.get_best_cmap(&DEFAULT_CMAP_PREFERENCES)
				.cloned()
				.expect("cmap");
			let expected: BTreeMap<u32, u32> = mapping
				.iter()
				.map(|(&u, n)| (u, order.iter().position(|o| o == n).expect("glyph") as u32))
				.collect();
			assert_eq!(best, expected);
			if gid > 0xFFFF {
				let mut subtables: Vec<(u16, u16, u16)> = fb
					.font
					.require::<Cmap>()?
					.tables
					.iter()
					.map(|t| (t.platform_id, t.plat_enc_id, t.format))
					.collect();
				subtables.sort();
				assert_eq!(subtables, [(0, 4, 12), (3, 10, 12)]);
			}
		}
	}
	Ok(())
}

#[test]
fn test_unicode_variation_sequences_high_glyph_ids() -> TestResult {
	for default in [false, true] {
		for gid in [0xFFFF_u32, 0x10000, 0x10001] {
			let mut fb = FontBuilder::new(1024, true, true)?;
			let order: Vec<String> = std::iter::once(s(".notdef"))
				.chain((1..=gid).map(|i| format!("glyph{i}")))
				.collect();
			fb.setup_glyph_order(order.clone());
			let mapping: BTreeMap<u32, String> = [(
				65,
				if default {
					order[gid as usize].clone()
				} else {
					order[1].clone()
				},
			)]
			.into_iter()
			.collect();
			fb.setup_character_map(
				&mapping,
				Some(&[(65, 0xFE00, Some(order[gid as usize].clone()))]),
				false,
			)?;
			let uvs_format = fb
				.font
				.require::<Cmap>()?
				.tables
				.last()
				.expect("uvs")
				.format;
			assert_eq!(uvs_format, if gid > 0xFFFF && !default { 15 } else { 14 });
			let reloaded = reload_cmap(&mut fb)?;
			let uvs = reloaded
				.tables
				.iter()
				.find(|t| (t.platform_id, t.plat_enc_id) == (0, 5))
				.and_then(|t| t.uvs_dict.clone())
				.expect("uvs");
			let expected: BTreeMap<u32, Vec<(u32, Option<u32>)>> =
				[(0xFE00, vec![(65, if default { None } else { Some(gid) })])]
					.into_iter()
					.collect();
			assert_eq!(uvs, expected);
		}
	}
	Ok(())
}

#[test]
fn test_unicode_variation_sequences() -> TestResult {
	let name_strings = vec![
		(NameKey::from("familyName"), NameValue::from("UVSTestFont")),
		(NameKey::from("styleName"), NameValue::from("Regular")),
		(
			NameKey::from("psName"),
			NameValue::from("UVSTestFont-Regular"),
		),
	];
	let glyph_order = [".notdef", "space", "zero", "zero.slash"];
	let cmap: BTreeMap<u32, String> = [(' ' as u32, s("space")), ('0' as u32, s("zero"))]
		.into_iter()
		.collect();
	let metrics: IndexMap<String, (f64, f64)> =
		glyph_order.iter().map(|g| (s(g), (600.0, 0.0))).collect();

	let mut fb = FontBuilder::new(1024, true, false)?;
	fb.setup_glyph_order(glyph_order.iter().map(|g| s(g)).collect());
	fb.setup_character_map(
		&cmap,
		Some(&[
			(0x0030, 0xFE00, Some(s("zero.slash"))),
			(0x0030, 0xFE01, None),
		]),
		false,
	)?;
	let order = fb.font.glyph_order()?;
	let empty = ttglyph(&order, |_| Ok(()))?;
	fb.setup_glyf(
		glyph_order.iter().map(|g| (s(g), empty.clone())).collect(),
		true,
	)?;
	fb.setup_horizontal_metrics(&metrics)?;
	fb.setup_horizontal_header(&ascent_descent())?;
	fb.setup_name_table(&name_strings, true, true)?;
	fb.setup_os2(&[])?;
	fb.setup_post(true, &[])?;
	verify_output(&mut fb, "test_uvs.ttf.ttx", Some(&["cmap"]))?;

	// The same subtable data, because cmap[0x0030] is "zero".
	fb.setup_character_map(
		&cmap,
		Some(&[
			(0x0030, 0xFE00, Some(s("zero.slash"))),
			(0x0030, 0xFE01, Some(s("zero"))),
		]),
		false,
	)?;
	verify_output(&mut fb, "test_uvs.ttf.ttx", Some(&["cmap"]))
}

#[test]
fn test_setup_panose() -> TestResult {
	let (mut fb, advance_widths, name_strings) = setup_font_builder(true, 1024)?;
	let order = fb.font.glyph_order()?;
	let glyph = ttglyph(&order, |p| draw_test_glyph(p, false))?;
	fb.setup_glyf(
		[".notdef", "A", "a", ".null"]
			.iter()
			.map(|n| (s(n), glyph.clone()))
			.collect(),
		true,
	)?;
	let metrics = metrics_from_glyf(&mut fb, &advance_widths)?;
	fb.setup_horizontal_metrics(&metrics)?;
	fb.setup_horizontal_header(&ascent_descent())?;
	fb.setup_name_table(&name_strings, true, true)?;
	fb.setup_os2(&[])?;
	fb.setup_post(true, &[])?;
	// A sample value of Times New Roman, from https://www.w3.org/Printing/stevahn.html.
	let panose = Panose {
		b_family_type: 2,
		b_serif_style: 2,
		b_weight: 6,
		b_proportion: 3,
		b_contrast: 5,
		b_stroke_variation: 4,
		b_arm_style: 5,
		b_letter_form: 2,
		b_midline: 3,
		b_x_height: 4,
	};
	assert_eq!(fb.font.require::<Os2>()?.panose, Panose::default());
	fb.setup_os2(&[("panose", Value::Panose(panose.clone()))])?;
	fb.setup_post(true, &[])?;
	assert_eq!(fb.font.require::<Os2>()?.panose, panose);
	Ok(())
}
