//! Helpers of the tests ported from fontTools' `Tests/varLib/instancer/*_test.py`,
//! besides `instancer_test.py`.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use fontoxide_misc::xml_reader::{self, Element};
use fontoxide_pens::{Error as PenError, Pen, Point, Transform, TransformPen};
use fontoxide_ttlib::{Font, GlyphSet, ReadOptions, WriteOptions};
use fontoxide_varlib::instancer::NormalizedAxisTriple;
use fontoxide_varlib::instancer::test_support::glyph_set_at;

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub type Location = BTreeMap<String, f64>;

/// A location from `(tag, value)` pairs.
pub fn loc(pairs: &[(&str, f64)]) -> Location {
	pairs.iter().map(|(t, v)| ((*t).to_owned(), *v)).collect()
}

/// A font of `tests/data/instancer_modules`, made by
/// `tools/reference/instancer_module_fixtures.py`, compiled and read back.
pub fn fixture(name: &str) -> TestResult<Font> {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data/instancer_modules")
		.join(format!("{name}.ttx"));
	let mut font = Font::new();
	font.recalc_timestamp = false;
	font.import_ttx(&std::fs::read_to_string(path)?)?;
	roundtrip(&mut font)
}

/// `font.save(stream); TTFont(stream)`.
pub fn roundtrip(font: &mut Font) -> TestResult<Font> {
	Ok(Font::read(
		font.write(&WriteOptions::default())?,
		ReadOptions {
			recalc_timestamp: false,
			..Default::default()
		},
	)?)
}

/// `NormalizedAxisTripleAndDistances(a, b, c)`.
pub fn normalized(a: f64, b: f64, c: f64) -> NormalizedAxisTriple {
	NormalizedAxisTriple::new(a, b, c, 1.0, 1.0).expect("sorted")
}

/// `pytest.approx`.
pub fn approx(actual: f64, expected: f64) -> bool {
	(actual - expected).abs() <= (1e-6 * expected.abs()).max(1e-12)
}

/// `DecomposingRecordingPen`, or `RecordingPen` without a glyph set.
#[derive(Default)]
pub struct RecordingPen<'g, 'f> {
	pub value: Vec<(&'static str, Vec<Point>)>,
	pub glyph_set: Option<&'g GlyphSet<'f>>,
}

impl Pen for RecordingPen<'_, '_> {
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
		glyph_name: &str,
		transformation: Transform,
	) -> fontoxide_pens::Result<()> {
		let glyph_set = self
			.glyph_set
			.ok_or_else(|| PenError::Usage("no glyph set".into()))?;
		let mut pen = TransformPen::new(self, transformation);
		glyph_set
			.draw(glyph_name, &mut pen)
			.map_err(|e| PenError::Glyph(Box::new(e)))
	}
}

/// What a glyph draws at a location (`font.getGlyphSet(location=location)[name].draw(pen)`),
/// decomposed.
pub fn draw(
	font: &mut Font,
	name: &str,
	location: &Location,
) -> TestResult<Vec<(&'static str, Vec<Point>)>> {
	let glyph_set = glyph_set_at(font, location)?;
	let mut pen = RecordingPen {
		value: Vec::new(),
		glyph_set: Some(&glyph_set),
	};
	glyph_set.draw(name, &mut pen)?;
	Ok(pen.value)
}

/// The program of a charstring as dumped to TTX, as tokens.
pub fn charstring_program(font: &mut Font, table: &str, glyph: &str) -> TestResult<Vec<String>> {
	let ttx = font.to_ttx(&fontoxide_ttlib::TtxOptions {
		tables: vec![fontoxide_ttlib::TableKey::Table(
			fontoxide_ttlib::Tag::from_str_padded(table).ok_or("a table tag")?,
		)],
		newline: "\n".into(),
		..Default::default()
	})?;
	let root = xml_reader::parse(&ttx)?;
	fn find<'a>(e: &'a Element, glyph: &str) -> Option<&'a Element> {
		if e.name == "CharString" && e.attr("name") == Some(glyph) {
			return Some(e);
		}
		e.elements().find_map(|c| find(c, glyph))
	}
	let element = find(&root, glyph).ok_or_else(|| format!("no charstring of {glyph}"))?;
	Ok(element
		.text()
		.split_whitespace()
		.map(str::to_owned)
		.collect())
}
