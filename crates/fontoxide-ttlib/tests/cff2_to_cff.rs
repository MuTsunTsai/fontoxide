//! Port of the class `CFF2ToCFFTest` of fontTools' `Tests/cffLib/cffLib_test.py`, and of
//! `Tests/cffLib/width_test.py`.
//!
//! `_makeCFF2Font` is in `tools/reference/cff2_to_cff_fixtures.py`, as FontBuilder is not
//! ported. It also saves what fontTools converts the fonts to, for comparing.

use std::path::PathBuf;

use fontoxide_misc::transform::Transform;
use fontoxide_pens::{Pen, Point};
use fontoxide_ttlib::cff::cff2_to_cff::{convert_cff2_to_cff, get_hmtx_tag};
use fontoxide_ttlib::cff::dict::Value;
use fontoxide_ttlib::tables::{Cff, Hmtx};
use fontoxide_ttlib::{
	DefaultTable, Error, Font, GlyphSet, ReadOptions, Result, TableKey, Tag, TtxOptions,
	WriteOptions,
};

fn data(name: &str) -> String {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data")
		.join(name);
	std::fs::read_to_string(path)
		.expect("a fixture")
		.replace("\r\n", "\n")
}

fn save_and_load(font: &mut Font, options: ReadOptions) -> Result<Font> {
	Font::read(font.write(&WriteOptions::default())?, options)
}

/// `_makeCFF2Font`: the fixture is loaded from binary, not from XML, as
/// `convertCFF2ToCFF` needs.
fn make_cff2_font(fixture: &str) -> Result<Font> {
	let mut font = Font::new();
	font.recalc_bboxes = false;
	font.recalc_timestamp = false;
	font.import_ttx(&data(&format!("{fixture}.ttx")))?;
	save_and_load(
		&mut font,
		ReadOptions {
			recalc_bboxes: false,
			recalc_timestamp: false,
			..Default::default()
		},
	)
}

/// A pen that records what is drawn (`RecordingPen`).
#[derive(Default)]
struct RecordingPen {
	value: Vec<(&'static str, Vec<Point>)>,
}

impl Pen for RecordingPen {
	fn move_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.value.push(("moveTo", vec![pt]));
		Ok(())
	}

	fn line_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.value.push(("lineTo", vec![pt]));
		Ok(())
	}

	fn curve_to(&mut self, points: &[Point]) -> fontoxide_pens::Result<()> {
		self.value.push(("curveTo", points.to_vec()));
		Ok(())
	}

	fn q_curve_to(
		&mut self,
		off_curves: &[Point],
		on_curve: Option<Point>,
	) -> fontoxide_pens::Result<()> {
		let mut points = off_curves.to_vec();
		points.extend(on_curve);
		self.value.push(("qCurveTo", points));
		Ok(())
	}

	fn close_path(&mut self) -> fontoxide_pens::Result<()> {
		self.value.push(("closePath", Vec::new()));
		Ok(())
	}

	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		self.value.push(("endPath", Vec::new()));
		Ok(())
	}

	fn add_component(
		&mut self,
		_glyph_name: &str,
		_transformation: Transform,
	) -> fontoxide_pens::Result<()> {
		self.value.push(("addComponent", Vec::new()));
		Ok(())
	}
}

type Drawing = Vec<(u16, Vec<(&'static str, Vec<Point>)>)>;

/// `_drawGlyphs`.
fn draw_glyphs(font: &mut Font) -> Result<Drawing> {
	let order = font.glyph_order()?;
	let names = order.names().to_vec();
	let widths = font.require::<Hmtx>()?.metrics.clone();
	let glyph_set = GlyphSet::new(font)?;
	let mut result = Vec::new();
	for (gid, name) in names.iter().enumerate() {
		let mut pen = RecordingPen::default();
		glyph_set.draw(name, &mut pen)?;
		result.push((widths[&(gid as u32)].0, pen.value));
	}
	Ok(result)
}

/// `_convertAndReload`.
fn convert_and_reload(font: &mut Font) -> Result<Font> {
	convert_cff2_to_cff(font, true)?;
	save_and_load(font, ReadOptions::default())
}

fn glyph_order(font: &mut Font) -> Result<Vec<String>> {
	Ok(font.glyph_order()?.names().to_vec())
}

/// What fontTools converts the font to.
fn assert_same_as_font_tools(font: &mut Font, fixture: &str) -> Result<()> {
	let ttx = font.to_ttx(&TtxOptions {
		tables: vec![TableKey::Table(Tag::new(b"CFF "))],
		newline: "\n".into(),
		..Default::default()
	})?;
	// `ttLibVersion` and `sfntVersion` are not of interest.
	let lines = |s: &str| {
		s.lines()
			.filter(|l| !l.starts_with("<ttFont "))
			.map(str::to_owned)
			.collect::<Vec<_>>()
	};
	assert_eq!(
		lines(&ttx),
		lines(&data(&format!("{fixture}.converted.ttx")))
	);
	Ok(())
}

#[test]
fn test_single_fontdict_converts_to_name_keyed() -> Result<()> {
	let glyphs = [".notdef", "A", "B"];
	let fixture = "cff2_single_fontdict";
	let mut font = make_cff2_font(fixture)?;
	let expected = draw_glyphs(&mut font)?;

	let mut font = convert_and_reload(&mut font)?;

	let top = &font.require::<Cff>()?.cff.top_dicts[0];
	assert!(top.get("ROS").is_none());
	assert!(top.fd_array.is_none());
	assert!(top.fd_select.is_none());
	assert!(top.private.is_some());
	assert_eq!(glyph_order(&mut font)?, glyphs);
	assert_eq!(draw_glyphs(&mut font)?, expected);
	assert_same_as_font_tools(&mut font, fixture)
}

#[test]
fn test_multiple_fontdicts_converts_to_cid_keyed() -> Result<()> {
	let fixture = "cff2_multiple_fontdicts";
	let mut font = make_cff2_font(fixture)?;
	let expected = draw_glyphs(&mut font)?;

	let mut font = convert_and_reload(&mut font)?;

	let top = &font.require::<Cff>()?.cff.top_dicts[0];
	assert!(top.is_cid());
	assert_eq!(
		top.get("ROS"),
		Some(Value::Tuple(vec![
			Value::Str("Adobe".to_owned()),
			Value::Str("Identity".to_owned()),
			Value::Int(0),
		]))
	);
	assert_eq!(top.fd_array.as_ref().map(Vec::len), Some(2));
	assert_eq!(glyph_order(&mut font)?, [".notdef", "cid00001", "cid00002"]);
	assert_eq!(draw_glyphs(&mut font)?, expected);
	assert_same_as_font_tools(&mut font, fixture)
}

// `Tests/cffLib/width_test.py`.

const HMTX: Tag = Tag::new(b"hmtx");
const UPPERCASE_HMTX: Tag = Tag::new(b"HMTX");

#[test]
fn test_get_hmtx_prefers_uppercase_companion() {
	let mut font = Font::new();
	font.insert(Box::new(DefaultTable::new(HMTX, Vec::new())));
	font.insert(Box::new(DefaultTable::new(UPPERCASE_HMTX, Vec::new())));

	assert_eq!(get_hmtx_tag(&font), UPPERCASE_HMTX);
}

#[test]
fn test_get_hmtx_uses_lowercase_without_companion() {
	let mut font = Font::new();
	font.insert(Box::new(DefaultTable::new(HMTX, Vec::new())));

	assert_eq!(get_hmtx_tag(&font), HMTX);
}

/// The metrics of a font with `HMTX` can't be read yet: not the wrong ones either.
#[test]
fn uppercase_companion_is_not_ignored() -> Result<()> {
	let mut font = make_cff2_font("cff2_single_fontdict")?;
	font.insert(Box::new(DefaultTable::new(UPPERCASE_HMTX, Vec::new())));
	assert!(matches!(
		convert_cff2_to_cff(&mut font, true),
		Err(Error::TtLib(_))
	));
	Ok(())
}
