//! `Tests/ttLib/tables/TupleVariation_test.py`.
//!
//! `test_compileTupleVariationStore_roundTrip_GVAR` is ported in `gvar.rs`.

#![allow(non_snake_case, clippy::unwrap_used)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Once;

use fontoxide_misc::text_tools::de_hex_str;
use fontoxide_misc::xml_reader;
use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_ttlib::Result;
use fontoxide_ttlib::tables::glyf::Number;
use fontoxide_ttlib::tables::tuple_variation::{
	Delta, Points, Region, TupleVariation, compile_delta_values, compile_points,
	compile_shared_tuples, compile_tuple_variation_store, decompile_coord, decompile_deltas,
	decompile_points, decompile_shared_tuples, decompile_tuple_variation_store, infer_region,
	tuple_size,
};

// --- Helpers ---------------------------------------------------------------

fn hexencode(data: &[u8]) -> String {
	data.iter()
		.map(|b| format!("{b:02X}"))
		.collect::<Vec<_>>()
		.join(" ")
}

fn de_hex(s: &str) -> Vec<u8> {
	de_hex_str(s).unwrap()
}

fn tags(list: &[&str]) -> Vec<String> {
	list.iter().map(|s| (*s).to_owned()).collect()
}

fn axes(list: &[(&str, Region)]) -> BTreeMap<String, Region> {
	list.iter().map(|(t, r)| ((*t).to_owned(), *r)).collect()
}

/// The `AXES` of the upstream test.
fn upstream_axes() -> BTreeMap<String, Region> {
	axes(&[
		("wdth", (0.25, 0.375, 0.5)),
		("wght", (0.0, 1.0, 1.0)),
		("opsz", (-0.75, -0.75, 0.0)),
	])
}

fn int(v: i64) -> Number {
	Number::Int(v)
}

fn pt(x: i64, y: i64) -> Option<Delta> {
	Some(Delta::Point(int(x), int(y)))
}

fn cvt(v: i64) -> Option<Delta> {
	Some(Delta::Value(int(v)))
}

fn fpt(x: f64, y: f64) -> Option<Delta> {
	Some(Delta::Point(Number::Float(x), Number::Float(y)))
}

fn fcvt(v: f64) -> Option<Delta> {
	Some(Delta::Value(Number::Float(v)))
}

fn var(axes: BTreeMap<String, Region>, coordinates: Vec<Option<Delta>>) -> TupleVariation {
	TupleVariation { axes, coordinates }
}

/// The deltas as numbers, comparing like Python does (`1 == 1.0`).
fn values(coordinates: &[Option<Delta>]) -> Vec<Option<Vec<f64>>> {
	coordinates
		.iter()
		.map(|d| {
			d.map(|d| match d {
				Delta::Point(x, y) => vec![x.value(), y.value()],
				Delta::Value(v) => vec![v.value()],
			})
		})
		.collect()
}

fn xml_lines(var: &TupleVariation, axis_tags: &[&str]) -> Vec<String> {
	let mut writer = XmlWriter::new();
	var.to_xml(&mut writer, &tags(axis_tags));
	let text = writer.into_string();
	text.lines().skip(1).map(|l| l.trim().to_owned()).collect()
}

fn from_xml(var: &mut TupleVariation, xml: &str) -> Result<()> {
	let root = xml_reader::parse(&format!("<root>{xml}</root>")).unwrap();
	for e in root.elements() {
		var.from_xml(e)?;
	}
	Ok(())
}

/// A small deterministic generator in place of `random`.
struct Rng(u64);

impl Rng {
	fn next(&mut self) -> u64 {
		self.0 ^= self.0 << 13;
		self.0 ^= self.0 >> 7;
		self.0 ^= self.0 << 17;
		self.0
	}

	fn below(&mut self, n: usize) -> usize {
		(self.next() % n as u64) as usize
	}

	/// `random.sample(range(start, stop), k)`.
	fn sample(&mut self, start: i64, stop: i64, k: usize) -> Vec<i64> {
		let mut pool: Vec<i64> = (start..stop).collect();
		let mut out = Vec::with_capacity(k);
		for _ in 0..k {
			let i = self.below(pool.len());
			out.push(pool.swap_remove(i));
		}
		out
	}

	fn shuffle<T>(&mut self, list: &mut [T]) {
		for i in (1..list.len()).rev() {
			let j = self.below(i + 1);
			list.swap(i, j);
		}
	}
}

// --- Log capture -----------------------------------------------------------

thread_local! {
	static CAPTURED: RefCell<Option<(log::Level, Vec<String>)>> = const { RefCell::new(None) };
}

struct Capture;

impl log::Log for Capture {
	fn enabled(&self, _: &log::Metadata<'_>) -> bool {
		true
	}

	fn log(&self, record: &log::Record<'_>) {
		CAPTURED.with(|c| {
			if let Some((level, records)) = c.borrow_mut().as_mut()
				&& record.level() <= *level
			{
				records.push(record.args().to_string());
			}
		});
	}

	fn flush(&self) {}
}

static CAPTURE: Capture = Capture;
static INIT: Once = Once::new();

/// The messages of `level` and above that `f` logs on this thread
/// (`CapturingLogHandler(log, level)`).
fn capture<R>(level: log::Level, f: impl FnOnce() -> R) -> (Vec<String>, R) {
	INIT.call_once(|| {
		let _ = log::set_logger(&CAPTURE);
		log::set_max_level(log::LevelFilter::Trace);
	});
	CAPTURED.with(|c| *c.borrow_mut() = Some((level, Vec::new())));
	let result = f();
	let records = CAPTURED
		.with(|c| c.borrow_mut().take())
		.map(|(_, r)| r)
		.unwrap_or_default();
	(records, result)
}

// Shared tuples in the 'gvar' table of the Skia font, as printed in Apple's
// TrueType specification.
const SKIA_GVAR_SHARED_TUPLES_DATA: &str = "40 00 00 00 C0 00 00 00 00 00 40 00 00 00 C0 00 \
	C0 00 C0 00 40 00 C0 00 40 00 40 00 C0 00 40 00";

/// `SKIA_GVAR_SHARED_TUPLES`, as (wght, wdth).
const SKIA_GVAR_SHARED_TUPLES: [[f64; 2]; 8] = [
	[1.0, 0.0],
	[-1.0, 0.0],
	[0.0, 1.0],
	[0.0, -1.0],
	[-1.0, -1.0],
	[1.0, -1.0],
	[1.0, 1.0],
	[-1.0, 1.0],
];

// Tuple Variation Store of uppercase I in the Skia font, as printed in Apple's
// TrueType spec.
const SKIA_GVAR_I_DATA: &str = "00 08 00 24 00 33 20 00 00 15 20 01 00 1B 20 02 \
	00 24 20 03 00 15 20 04 00 26 20 07 00 0D 20 06 \
	00 1A 20 05 00 40 01 01 01 81 80 43 FF 7E FF 7E \
	FF 7E FF 7E 00 81 45 01 01 01 03 01 04 01 04 01 \
	04 01 02 80 40 00 82 81 81 04 3A 5A 3E 43 20 81 \
	04 0E 40 15 45 7C 83 00 0D 9E F3 F2 F0 F0 F0 F0 \
	F3 9E A0 A1 A1 A1 9F 80 00 91 81 91 00 0D 0A 0A \
	09 0A 0A 0A 0A 0A 0A 0A 0A 0A 0A 0B 80 00 15 81 \
	81 00 C4 89 00 C4 83 00 0D 80 99 98 96 96 96 96 \
	99 80 82 83 83 83 81 80 40 FF 18 81 81 04 E6 F9 \
	10 21 02 81 04 E8 E5 EB 4D DA 83 00 0D CE D3 D4 \
	D3 D3 D3 D5 D2 CE CC CD CD CD CD 80 00 A1 81 91 \
	00 0D 07 03 04 02 02 02 03 03 07 07 08 08 08 07 \
	80 00 09 81 81 00 28 40 00 A4 02 24 24 66 81 04 \
	08 FA FA FA 28 83 00 82 02 FF FF FF 83 02 01 01 \
	01 84 91 00 80 06 07 08 08 08 08 0A 07 80 03 FE \
	FF FF FF 81 00 08 81 82 02 EE EE EE 8B 6D 00";

fn wght_1() -> BTreeMap<String, Region> {
	axes(&[("wght", (0.0, 1.0, 1.0))])
}

fn three_points() -> Vec<Option<Delta>> {
	vec![pt(0, 0), pt(9, 8), pt(7, 6)]
}

// --- Equality and impact ---------------------------------------------------

#[test]
fn test_equal() {
	assert_eq!(var(wght_1(), three_points()), var(wght_1(), three_points()));
}

#[test]
fn test_equal_differentAxes() {
	let var2 = var(axes(&[("wght", (0.7, 0.8, 0.9))]), three_points());
	assert_ne!(var(wght_1(), three_points()), var2);
}

#[test]
fn test_equal_differentCoordinates() {
	let var2 = var(wght_1(), vec![pt(0, 0), pt(9, 8)]);
	assert_ne!(var(wght_1(), three_points()), var2);
}

#[test]
fn test_hasImpact_someDeltasNotZero() {
	assert!(var(wght_1(), three_points()).has_impact());
}

#[test]
fn test_hasImpact_allDeltasZero() {
	assert!(var(wght_1(), vec![pt(0, 0), pt(0, 0), pt(0, 0)]).has_impact());
}

#[test]
fn test_hasImpact_allDeltasNone() {
	assert!(!var(wght_1(), vec![None; 3]).has_impact());
}

// --- TTX -------------------------------------------------------------------

#[test]
fn test_toXML_badDeltaFormat() {
	// A float `cvar` delta is a bad format as the string of the upstream test
	// (which cannot be expressed in Rust): `type(delta) == int` fails for both.
	let g = var(upstream_axes(), vec![fcvt(1.5)]);
	let (records, lines) = capture(log::Level::Error, || xml_lines(&g, &["wdth"]));
	assert!(
		records.iter().any(|r| r == "bad delta format"),
		"{records:?}"
	);
	assert_eq!(
		lines,
		[
			"<tuple>",
			r#"<coord axis="wdth" min="0.25" value="0.375" max="0.5"/>"#,
			"<!-- bad delta #0 -->",
			"</tuple>",
		]
	);
}

#[test]
fn test_toXML_constants() {
	let g = var(
		upstream_axes(),
		vec![cvt(42), None, cvt(23), cvt(0), cvt(-17), None],
	);
	assert_eq!(
		xml_lines(&g, &["wdth", "wght", "opsz"]),
		[
			"<tuple>",
			r#"<coord axis="wdth" min="0.25" value="0.375" max="0.5"/>"#,
			r#"<coord axis="wght" value="1.0"/>"#,
			r#"<coord axis="opsz" value="-0.75"/>"#,
			r#"<delta cvt="0" value="42"/>"#,
			r#"<delta cvt="2" value="23"/>"#,
			r#"<delta cvt="3" value="0"/>"#,
			r#"<delta cvt="4" value="-17"/>"#,
			"</tuple>",
		]
	);
}

#[test]
fn test_toXML_points() {
	let g = var(
		upstream_axes(),
		vec![pt(9, 8), None, pt(7, 6), pt(0, 0), pt(-1, -2), None],
	);
	assert_eq!(
		xml_lines(&g, &["wdth", "wght", "opsz"]),
		[
			"<tuple>",
			r#"<coord axis="wdth" min="0.25" value="0.375" max="0.5"/>"#,
			r#"<coord axis="wght" value="1.0"/>"#,
			r#"<coord axis="opsz" value="-0.75"/>"#,
			r#"<delta pt="0" x="9" y="8"/>"#,
			r#"<delta pt="2" x="7" y="6"/>"#,
			r#"<delta pt="3" x="0" y="0"/>"#,
			r#"<delta pt="4" x="-1" y="-2"/>"#,
			"</tuple>",
		]
	);
}

#[test]
fn test_toXML_allDeltasNone() {
	let g = var(wght_1(), vec![None; 5]);
	assert_eq!(
		xml_lines(&g, &["wght", "wdth"]),
		[
			"<tuple>",
			r#"<coord axis="wght" value="1.0"/>"#,
			"<!-- no deltas -->",
			"</tuple>",
		]
	);
}

#[test]
fn test_toXML_axes_floats() {
	let g = var(
		axes(&[
			("wght", (0.0, 0.2999878, 0.7000122)),
			("wdth", (0.0, 0.4000244, 0.4000244)),
		]),
		vec![None; 5],
	);
	assert_eq!(
		xml_lines(&g, &["wght", "wdth"])[1..3],
		[
			r#"<coord axis="wght" min="0.0" value="0.3" max="0.7"/>"#,
			r#"<coord axis="wdth" value="0.4"/>"#,
		]
	);
}

#[test]
fn test_fromXML_badDeltaFormat() -> Result<()> {
	let mut g = TupleVariation::new(0);
	let (records, result) = capture(log::Level::Warn, || {
		from_xml(&mut g, r#"<delta a="1" b="2"/>"#)
	});
	result?;
	assert!(
		records.iter().any(|r| r == "bad delta format: a, b"),
		"{records:?}"
	);
	Ok(())
}

#[test]
fn test_fromXML_constants() -> Result<()> {
	let mut g = TupleVariation::new(4);
	from_xml(
		&mut g,
		concat!(
			r#"<coord axis="wdth" min="0.25" value="0.375" max="0.5"/>"#,
			r#"<coord axis="wght" value="1.0"/>"#,
			r#"<coord axis="opsz" value="-0.75"/>"#,
			r#"<delta cvt="1" value="42"/>"#,
			r#"<delta cvt="2" value="-23"/>"#,
		),
	)?;
	assert_eq!(g.axes, upstream_axes());
	assert_eq!(g.coordinates, [None, cvt(42), cvt(-23), None]);
	Ok(())
}

#[test]
fn test_fromXML_points() -> Result<()> {
	let mut g = TupleVariation::new(4);
	from_xml(
		&mut g,
		concat!(
			r#"<coord axis="wdth" min="0.25" value="0.375" max="0.5"/>"#,
			r#"<coord axis="wght" value="1.0"/>"#,
			r#"<coord axis="opsz" value="-0.75"/>"#,
			r#"<delta pt="1" x="33" y="44"/>"#,
			r#"<delta pt="2" x="-2" y="170"/>"#,
		),
	)?;
	assert_eq!(g.axes, upstream_axes());
	assert_eq!(g.coordinates, [None, pt(33, 44), pt(-2, 170), None]);
	Ok(())
}

#[test]
fn test_fromXML_axes_floats() -> Result<()> {
	let mut g = TupleVariation::new(4);
	from_xml(
		&mut g,
		concat!(
			r#"<coord axis="wght" min="0.0" value="0.3" max="0.7"/>"#,
			r#"<coord axis="wdth" value="0.4"/>"#,
		),
	)?;
	let close = |a: f64, b: f64| (a - b).abs() < 1e-7;
	let (min, peak, max) = g.axes["wght"];
	assert_eq!(min, 0.0);
	assert!(close(peak, 0.2999878) && close(max, 0.7000122));
	let (min, peak, max) = g.axes["wdth"];
	assert_eq!(min, 0.0);
	assert!(close(peak, 0.4000244) && close(max, 0.4000244));
	Ok(())
}

// --- compile ---------------------------------------------------------------

fn compile(
	v: &TupleVariation,
	axis_tags: &[&str],
	shared: bool,
	private_points: bool,
) -> Result<(String, String)> {
	let axis_tags = tags(axis_tags);
	let mut shared_coords = HashMap::new();
	if shared {
		shared_coords.insert(v.compile_coord(&axis_tags)?, 0x77);
	}
	// `pointData=None` compiles the used points; `b""` means the shared ones.
	let point_data = if private_points {
		compile_points(&v.used_points().unwrap())
	} else {
		Vec::new()
	};
	let (tup, deltas) = v.compile(&axis_tags, &shared_coords, &point_data, true)?;
	Ok((hexencode(&tup), hexencode(&deltas)))
}

fn wght_wdth(wght: Region, wdth: Region) -> BTreeMap<String, Region> {
	axes(&[("wght", wght), ("wdth", wdth)])
}

fn points_789_456() -> Vec<Option<Delta>> {
	vec![pt(7, 4), pt(8, 5), pt(9, 6)]
}

#[test]
fn test_compile_sharedPeaks_nonIntermediate_sharedPoints() -> Result<()> {
	let v = var(
		wght_wdth((0.0, 0.5, 0.5), (0.0, 0.8, 0.8)),
		points_789_456(),
	);
	let (tup, deltas) = compile(&v, &["wght", "wdth"], true, false)?;
	assert_eq!(tup, "00 08 00 77");
	assert_eq!(deltas, "02 07 08 09 02 04 05 06");
	Ok(())
}

#[test]
fn test_compile_sharedPeaks_intermediate_sharedPoints() -> Result<()> {
	let v = var(
		wght_wdth((0.3, 0.5, 0.7), (0.1, 0.8, 0.9)),
		points_789_456(),
	);
	let (tup, deltas) = compile(&v, &["wght", "wdth"], true, false)?;
	assert_eq!(tup, "00 08 40 77 13 33 06 66 2C CD 39 9A");
	assert_eq!(deltas, "02 07 08 09 02 04 05 06");
	Ok(())
}

#[test]
fn test_compile_sharedPeaks_nonIntermediate_privatePoints() -> Result<()> {
	let v = var(
		wght_wdth((0.0, 0.5, 0.5), (0.0, 0.8, 0.8)),
		points_789_456(),
	);
	let (tup, deltas) = compile(&v, &["wght", "wdth"], true, true)?;
	assert_eq!(tup, "00 09 20 77");
	assert_eq!(deltas, "00 02 07 08 09 02 04 05 06");
	Ok(())
}

#[test]
fn test_compile_sharedPeaks_intermediate_privatePoints() -> Result<()> {
	let v = var(
		wght_wdth((0.0, 0.5, 1.0), (0.0, 0.8, 1.0)),
		points_789_456(),
	);
	let (tup, deltas) = compile(&v, &["wght", "wdth"], true, true)?;
	assert_eq!(tup, "00 09 60 77 00 00 00 00 40 00 40 00");
	assert_eq!(deltas, "00 02 07 08 09 02 04 05 06");
	Ok(())
}

#[test]
fn test_compile_embeddedPeak_nonIntermediate_sharedPoints() -> Result<()> {
	let v = var(
		wght_wdth((0.0, 0.5, 0.5), (0.0, 0.8, 0.8)),
		points_789_456(),
	);
	let (tup, deltas) = compile(&v, &["wght", "wdth"], false, false)?;
	assert_eq!(tup, "00 08 80 00 20 00 33 33");
	assert_eq!(deltas, "02 07 08 09 02 04 05 06");
	Ok(())
}

#[test]
fn test_compile_embeddedPeak_nonIntermediate_sharedConstants() -> Result<()> {
	let v = var(
		wght_wdth((0.0, 0.5, 0.5), (0.0, 0.8, 0.8)),
		vec![cvt(3), cvt(1), cvt(4)],
	);
	let (tup, deltas) = compile(&v, &["wght", "wdth"], false, false)?;
	assert_eq!(tup, "00 04 80 00 20 00 33 33");
	assert_eq!(deltas, "02 03 01 04");
	Ok(())
}

#[test]
fn test_compile_embeddedPeak_intermediate_sharedPoints() -> Result<()> {
	let v = var(
		wght_wdth((0.0, 0.5, 1.0), (0.0, 0.8, 0.8)),
		points_789_456(),
	);
	let (tup, deltas) = compile(&v, &["wght", "wdth"], false, false)?;
	assert_eq!(tup, "00 08 C0 00 20 00 33 33 00 00 00 00 40 00 33 33");
	assert_eq!(deltas, "02 07 08 09 02 04 05 06");
	Ok(())
}

#[test]
fn test_compile_embeddedPeak_nonIntermediate_privatePoints() -> Result<()> {
	let v = var(
		wght_wdth((0.0, 0.5, 0.5), (0.0, 0.8, 0.8)),
		points_789_456(),
	);
	let (tup, deltas) = compile(&v, &["wght", "wdth"], false, true)?;
	assert_eq!(tup, "00 09 A0 00 20 00 33 33");
	assert_eq!(deltas, "00 02 07 08 09 02 04 05 06");
	Ok(())
}

#[test]
fn test_compile_embeddedPeak_nonIntermediate_privateConstants() -> Result<()> {
	let v = var(
		wght_wdth((0.0, 0.5, 0.5), (0.0, 0.8, 0.8)),
		vec![cvt(7), cvt(8), cvt(9)],
	);
	let (tup, deltas) = compile(&v, &["wght", "wdth"], false, true)?;
	assert_eq!(tup, "00 05 A0 00 20 00 33 33");
	assert_eq!(deltas, "00 02 07 08 09");
	Ok(())
}

#[test]
fn test_compile_embeddedPeak_intermediate_privatePoints() -> Result<()> {
	let v = var(
		wght_wdth((0.4, 0.5, 0.6), (0.7, 0.8, 0.9)),
		points_789_456(),
	);
	let (tup, deltas) = compile(&v, &["wght", "wdth"], false, true)?;
	assert_eq!(tup, "00 09 E0 00 20 00 33 33 19 9A 2C CD 26 66 39 9A");
	assert_eq!(deltas, "00 02 07 08 09 02 04 05 06");
	Ok(())
}

#[test]
fn test_compile_embeddedPeak_intermediate_privateConstants() -> Result<()> {
	let v = var(
		wght_wdth((0.4, 0.5, 0.6), (0.7, 0.8, 0.9)),
		vec![cvt(7), cvt(8), cvt(9)],
	);
	let (tup, deltas) = compile(&v, &["wght", "wdth"], false, true)?;
	assert_eq!(tup, "00 05 E0 00 20 00 33 33 19 9A 2C CD 26 66 39 9A");
	assert_eq!(deltas, "00 02 07 08 09");
	Ok(())
}

#[test]
fn test_compileCoord() -> Result<()> {
	let v = var(
		wght_wdth((-1.0, -1.0, -1.0), (0.4, 0.5, 0.6)),
		vec![None; 4],
	);
	assert_eq!(
		hexencode(&v.compile_coord(&tags(&["wght", "wdth"]))?),
		"C0 00 20 00"
	);
	assert_eq!(
		hexencode(&v.compile_coord(&tags(&["wdth", "wght"]))?),
		"20 00 C0 00"
	);
	assert_eq!(hexencode(&v.compile_coord(&tags(&["wght"]))?), "C0 00");
	Ok(())
}

#[test]
fn test_compileIntermediateCoord() -> Result<()> {
	let v = var(wght_wdth((-1.0, -1.0, 0.0), (0.4, 0.5, 0.6)), vec![None; 4]);
	let c = |t: &[&str]| -> Result<Option<String>> {
		Ok(v.compile_intermediate_coord(&tags(t))?
			.map(|d| hexencode(&d)))
	};
	assert_eq!(
		c(&["wght", "wdth"])?.as_deref(),
		Some("C0 00 19 9A 00 00 26 66")
	);
	assert_eq!(
		c(&["wdth", "wght"])?.as_deref(),
		Some("19 9A C0 00 26 66 00 00")
	);
	assert_eq!(c(&["wght"])?, None);
	assert_eq!(c(&["wdth"])?.as_deref(), Some("19 9A 26 66"));
	Ok(())
}

#[test]
fn test_decompileCoord() -> Result<()> {
	let data = de_hex("DE AD C0 00 20 00 DE AD");
	assert_eq!(
		decompile_coord(&tags(&["wght", "wdth"]), &data, 2)?,
		(vec![-1.0, 0.5], 6)
	);
	Ok(())
}

#[test]
fn test_decompileCoord_roundTrip() -> Result<()> {
	// Make sure we are not affected by
	// https://github.com/fonttools/fonttools/issues/286
	let data = de_hex("7F B9 80 35");
	let axis_tags = tags(&["wght", "wdth"]);
	let (values, _) = decompile_coord(&axis_tags, &data, 0)?;
	let v = var(
		axis_tags
			.iter()
			.zip(&values)
			.map(|(t, &v)| (t.clone(), (v, v, v)))
			.collect(),
		vec![None; 4],
	);
	assert_eq!(hexencode(&v.compile_coord(&axis_tags)?), "7F B9 80 35");
	Ok(())
}

// --- Points ----------------------------------------------------------------

#[test]
fn test_compilePoints() {
	let c = |p: &[usize]| hexencode(&compile_points(p));
	assert_eq!(c(&[]), "00"); // all points in glyph
	assert_eq!(c(&[7]), "01 00 07");
	assert_eq!(c(&[65535]), "01 80 FF FF");
	assert_eq!(c(&[9, 15]), "02 01 09 06");
	assert_eq!(c(&[7, 8, 255, 257, 258, 500]), "06 05 07 01 F7 02 01 F2");
	assert_eq!(c(&[7, 8, 500]), "03 01 07 01 80 01 EC");
	assert_eq!(c(&[7, 8, 0xBEEF, 0xCAFE]), "04 01 07 01 81 BE E7 0C 0F");
	let range300: Vec<usize> = (0..300).collect();
	assert_eq!(
		c(&range300),
		format!(
			"81 2C 7F 00{}{}{}{}{}",
			" 01".repeat(127),
			" 7F",
			" 01".repeat(128),
			" 2B",
			" 01".repeat(44)
		)
	);
	let range399: Vec<usize> = (0..399).collect();
	assert_eq!(
		c(&range399),
		format!(
			"81 8F 7F 00{} 7F{} 7F{} 0E{}",
			" 01".repeat(127),
			" 01".repeat(128),
			" 01".repeat(128),
			" 01".repeat(15)
		)
	);
}

#[test]
fn test_decompilePoints() -> Result<()> {
	const N: usize = 65536;
	let d = |data: &str, offset: usize| -> Result<(Vec<i64>, usize)> {
		let (points, offset) = decompile_points(N, &de_hex(data), offset, "gvar")?;
		let list = match points {
			Points::All(n) => (0..n as i64).collect(),
			Points::List(l) => l,
		};
		Ok((list, offset))
	};
	let all: Vec<i64> = (0..N as i64).collect();
	// All points in glyph.
	assert_eq!(d("00", 0)?, (all.clone(), 1));
	// All points in glyph, in an overly verbose encoding.
	assert_eq!(d("80 00", 0)?, (all, 2));
	assert_eq!(d("02 01 09 06", 0)?, (vec![9, 15], 4));
	assert_eq!(d("02 81 BE EF 0C 0F", 0)?, (vec![0xBEEF, 0xCAFE], 6));
	assert_eq!(d("01 00 07", 0)?, (vec![7], 3));
	assert_eq!(d("01 80 00 07", 0)?, (vec![7], 4));
	// Words are unsigned.
	assert_eq!(d("01 80 FF FF", 0)?, (vec![65535], 4));
	assert_eq!(d("04 01 07 01 01 FF 02", 0)?, (vec![7, 8, 263, 265], 7));
	// All encodings, preceded and followed by 4 bytes of unused data.
	let data = "DE AD DE AD 04 01 07 01 81 BE E7 0C 0F DE AD DE AD";
	assert_eq!(d(data, 4)?, (vec![7, 8, 0xBEEF, 0xCAFE], 13));
	let set = |v: Vec<i64>| v.into_iter().collect::<BTreeSet<_>>();
	let data = format!(
		"81 2C 7F 00{} 7F{} AB{}",
		" 01".repeat(127),
		" 01".repeat(128),
		" 00 01".repeat(44)
	);
	assert_eq!(set(d(&data, 0)?.0), (0..300).collect());
	let data = format!(
		"81 8F 7F 00{} 7F{} FF{} 8E{}",
		" 01".repeat(127),
		" 01".repeat(128),
		" 00 01".repeat(128),
		" 00 01".repeat(15)
	);
	assert_eq!(set(d(&data, 0)?.0), (0..399).collect());
	Ok(())
}

#[test]
fn test_decompilePoints_shouldAcceptBadPointNumbers() -> Result<()> {
	let (records, result) = capture(log::Level::Warn, || {
		decompile_points(8, &de_hex("02 01 03 06"), 0, "cvar")
	});
	result?;
	assert!(
		records
			.iter()
			.any(|r| r == "point 9 out of range in 'cvar' table"),
		"{records:?}"
	);
	Ok(())
}

#[test]
fn test_decompilePoints_roundTrip() -> Result<()> {
	const N: usize = 500;
	let decompile = |data: &[u8]| -> Result<BTreeSet<i64>> {
		Ok(match decompile_points(N, data, 0, "gvar")?.0 {
			Points::All(n) => (0..n as i64).collect(),
			Points::List(l) => l.into_iter().collect(),
		})
	};
	let mut rng = Rng(0x2545_F491_4F6C_DD1D);
	for _ in 0..50 {
		let points: BTreeSet<i64> = rng.sample(0, N as i64, 30).into_iter().collect();
		let list: Vec<usize> = points.iter().map(|&p| p as usize).collect();
		assert_eq!(decompile(&compile_points(&list))?, points, "{points:?}");
	}
	let all: Vec<usize> = (0..N).collect();
	let all_set: BTreeSet<i64> = (0..N as i64).collect();
	assert_eq!(decompile(&compile_points(&all))?, all_set);
	assert_eq!(decompile(&compile_points(&[]))?, all_set);
	Ok(())
}

// --- Deltas ----------------------------------------------------------------

#[test]
fn test_compileDeltas_points() -> Result<()> {
	let v = var(
		BTreeMap::new(),
		vec![None, pt(1, 0), pt(2, 0), None, pt(4, 0), None],
	);
	assert_eq!(hexencode(&v.compile_deltas(true)?), "02 01 02 04 82");
	Ok(())
}

#[test]
fn test_compileDeltas_constants() -> Result<()> {
	let v = var(
		BTreeMap::new(),
		vec![None, cvt(1), cvt(2), None, cvt(4), None],
	);
	assert_eq!(hexencode(&v.compile_deltas(true)?), "02 01 02 04");
	assert_eq!(hexencode(&v.compile_deltas(false)?), "02 01 02 04");
	Ok(())
}

#[test]
fn test_compileDeltaValues_empty() -> Result<()> {
	// The output-buffer argument has no equivalent; the result is returned.
	for optimize_size in [false, true] {
		assert!(compile_delta_values(&[], optimize_size)?.is_empty());
	}
	Ok(())
}

#[test]
fn test_compileDeltaValues() -> Result<()> {
	let c = |values: &[i64]| hexencode(&compile_delta_values(values, true).unwrap());
	let speed = |values: &[i64]| hexencode(&compile_delta_values(values, false).unwrap());
	// zeroes
	assert_eq!(c(&[0]), "80");
	assert_eq!(c(&[0; 64]), "BF");
	assert_eq!(c(&[0; 65]), "BF 80");
	assert_eq!(c(&[0; 100]), "BF A3");
	assert_eq!(c(&[0; 256]), "BF BF BF BF");
	// bytes
	assert_eq!(c(&[1]), "00 01");
	assert_eq!(c(&[1, 2, 3, 127, -128, -1, -2]), "06 01 02 03 7F 80 FF FE");
	assert_eq!(c(&[127; 64]), format!("3F{}", " 7F".repeat(64)));
	assert_eq!(c(&[127; 65]), format!("3F{} 00 7F", " 7F".repeat(64)));
	// words
	assert_eq!(c(&[0x6666]), "40 66 66");
	assert_eq!(
		c(&[0x6666, 32767, -1, -32768]),
		"43 66 66 7F FF FF FF 80 00"
	);
	assert_eq!(c(&[0x1122; 64]), format!("7F{}", " 11 22".repeat(64)));
	assert_eq!(
		c(&[0x1122; 65]),
		format!("7F{} 40 11 22", " 11 22".repeat(64))
	);
	// bytes, zeroes, bytes: a single zero is more compact as part of the bytes run
	assert_eq!(c(&[127, 127, 0, 127, 127]), "04 7F 7F 00 7F 7F");
	assert_eq!(c(&[127, 127, 0, 0, 127, 127]), "01 7F 7F 81 01 7F 7F");
	assert_eq!(c(&[127, 127, 0, 0, 0, 127, 127]), "01 7F 7F 82 01 7F 7F");
	assert_eq!(c(&[127, 127, 0, 0, 0, 0, 127, 127]), "01 7F 7F 83 01 7F 7F");
	// bytes, zeroes
	assert_eq!(c(&[1, 0]), "01 01 00");
	assert_eq!(speed(&[1, 0]), "01 01 00");
	assert_eq!(c(&[1, 0, 0]), "00 01 81");
	assert_eq!(speed(&[1, 0, 0]), "02 01 00 00");
	// words, bytes, words: a single byte is more compact as part of the words run
	assert_eq!(c(&[0x6666, 2, 0x7777]), "42 66 66 00 02 77 77");
	assert_eq!(speed(&[0x6666, 2, 0x7777]), "42 66 66 00 02 77 77");
	assert_eq!(c(&[0x6666, 2, 2, 0x7777]), "40 66 66 01 02 02 40 77 77");
	assert_eq!(speed(&[0x6666, 2, 2, 0x7777]), "43 66 66 00 02 00 02 77 77");
	// words, zeroes, words
	assert_eq!(c(&[0x6666, 0, 0x7777]), "40 66 66 80 40 77 77");
	assert_eq!(speed(&[0x6666, 0, 0x7777]), "42 66 66 00 00 77 77");
	assert_eq!(c(&[0x6666, 0, 0, 0x7777]), "40 66 66 81 40 77 77");
	assert_eq!(c(&[0x6666, 0, 0, 0, 0x7777]), "40 66 66 82 40 77 77");
	// words, zeroes, bytes
	assert_eq!(c(&[0x6666, 0, 1, 2, 3]), "40 66 66 80 02 01 02 03");
	assert_eq!(c(&[0x6666, 0, 0, 1, 2, 3]), "40 66 66 81 02 01 02 03");
	assert_eq!(c(&[0x6666, 0, 0, 0, 1, 2, 3]), "40 66 66 82 02 01 02 03");
	// words, zeroes
	assert_eq!(c(&[0x6666, 0]), "40 66 66 80");
	assert_eq!(c(&[0x6666, 0, 0]), "40 66 66 81");
	Ok(())
}

#[test]
fn test_decompileDeltas() -> Result<()> {
	// 83 = zero values (0x80), count = 4 (1 + 0x83 & 0x3F)
	assert_eq!(
		decompile_deltas(Some(4), &de_hex("83"), 0)?,
		(vec![0; 4], 1)
	);
	// 41 01 02 FF FF = signed 16-bit values (0x40), count = 2
	assert_eq!(
		decompile_deltas(Some(2), &de_hex("41 01 02 FF FF"), 0)?,
		(vec![258, -1], 5)
	);
	// 01 81 07 = signed 8-bit values, count = 2
	assert_eq!(
		decompile_deltas(Some(2), &de_hex("01 81 07"), 0)?,
		(vec![-127, 7], 3)
	);
	// All three encodings, preceded and followed by 4 bytes of unused data.
	let data = de_hex("DE AD BE EF 83 40 01 02 01 81 80 DE AD BE EF");
	assert_eq!(
		decompile_deltas(Some(7), &data, 4)?,
		(vec![0, 0, 0, 0, 258, -127, -128], 11)
	);
	Ok(())
}

#[test]
fn test_decompileDeltas_roundTrip() -> Result<()> {
	let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
	for _ in 0..50 {
		let mut deltas = rng.sample(-128, 127, 10);
		deltas.extend(rng.sample(-32768, 32767, 10));
		deltas.extend([0; 10]);
		rng.shuffle(&mut deltas);
		let data = compile_delta_values(&deltas, true)?;
		assert_eq!(decompile_deltas(Some(30), &data, 0)?.0, deltas);
	}
	Ok(())
}

// --- Shared tuples ---------------------------------------------------------

#[test]
fn test_compileSharedTuples() -> Result<()> {
	// {"wght": 1.0, "wdth": 0.8} appears three times (most frequent first);
	// {"wght": 1.0, "wdth": 0.5} and {"wght": 1.0, "wdth": 0.7} both appear
	// twice and are sorted by their bytes. {"wght": 1.0, "wdth": 0.9} appears
	// once and is ignored. Starts and ends are not part of the shared pool.
	let variations: Vec<TupleVariation> = [
		(0.5, 0.7),
		(0.2, 0.7),
		(0.2, 0.8),
		(0.3, 0.5),
		(0.3, 0.8),
		(0.3, 0.9),
		(0.4, 0.8),
		(0.5, 0.5),
	]
	.into_iter()
	.map(|(start, peak)| {
		var(
			wght_wdth((1.0, 1.0, 1.0), (start, peak, 1.0)),
			vec![None; 4],
		)
	})
	.collect();
	let result = compile_shared_tuples(&tags(&["wght", "wdth"]), &variations)?;
	assert_eq!(
		result.iter().map(|c| hexencode(c)).collect::<Vec<_>>(),
		["40 00 33 33", "40 00 20 00", "40 00 2C CD"]
	);
	Ok(())
}

#[test]
fn test_decompileSharedTuples_Skia() -> Result<()> {
	let shared = decompile_shared_tuples(
		&tags(&["wght", "wdth"]),
		8,
		&de_hex(SKIA_GVAR_SHARED_TUPLES_DATA),
		0,
	)?;
	let expected: Vec<Vec<f64>> = SKIA_GVAR_SHARED_TUPLES.iter().map(|t| t.to_vec()).collect();
	assert_eq!(shared, expected);
	Ok(())
}

#[test]
fn test_decompileSharedTuples_empty() -> Result<()> {
	assert!(decompile_shared_tuples(&tags(&["wght"]), 0, b"", 0)?.is_empty());
	Ok(())
}

// --- Tuple variation store -------------------------------------------------

#[test]
fn test_compileTupleVariationStore_allVariationsRedundant() -> Result<()> {
	let a = axes(&[("wght", (0.3, 0.4, 0.5)), ("opsz", (0.7, 0.8, 0.9))]);
	let variations = vec![var(a.clone(), vec![None; 4]); 3];
	assert_eq!(
		compile_tuple_variation_store(
			&variations,
			&tags(&["wght", "opsz"]),
			&HashMap::new(),
			true,
			true
		)?,
		(0, Vec::new(), Vec::new())
	);
	Ok(())
}

#[test]
fn test_compileTupleVariationStore_noVariations() -> Result<()> {
	assert_eq!(
		compile_tuple_variation_store(&[], &tags(&["wght", "opsz"]), &HashMap::new(), true, true)?,
		(0, Vec::new(), Vec::new())
	);
	Ok(())
}

fn store_round_trip(table: &str, deltas: Vec<Option<Delta>>) -> Result<()> {
	let variations = vec![
		var(wght_wdth((0.5, 1.0, 1.0), (1.0, 1.0, 1.0)), deltas.clone()),
		var(wght_wdth((1.0, 1.0, 1.0), (1.0, 1.0, 1.0)), deltas),
	];
	let axis_tags = tags(&["wght", "wdth"]);
	let (count, tuples, data) =
		compile_tuple_variation_store(&variations, &axis_tags, &HashMap::new(), true, true)?;
	let mut all = tuples.clone();
	all.extend(data);
	let decompiled = decompile_tuple_variation_store(
		table,
		&axis_tags,
		u16::try_from(count).unwrap(),
		4,
		Some(&[]),
		&all,
		0,
		tuples.len(),
	)?;
	assert_eq!(decompiled, variations);
	Ok(())
}

#[test]
fn test_compileTupleVariationStore_roundTrip_cvar() -> Result<()> {
	store_round_trip("cvar", vec![cvt(1), cvt(2), cvt(3), cvt(4)])
}

#[test]
fn test_compileTupleVariationStore_roundTrip_gvar() -> Result<()> {
	store_round_trip("gvar", vec![pt(1, 1), pt(2, 2), pt(3, 3), pt(4, 4)])
}

#[test]
fn test_decompileTupleVariationStore_Skia_I() -> Result<()> {
	let shared: Vec<Vec<f64>> = SKIA_GVAR_SHARED_TUPLES.iter().map(|t| t.to_vec()).collect();
	let tvar = decompile_tuple_variation_store(
		"gvar",
		&tags(&["wght", "wdth"]),
		8,
		18,
		Some(&shared),
		&de_hex(SKIA_GVAR_I_DATA),
		4,
		36,
	)?;
	assert_eq!(tvar.len(), 8);
	assert_eq!(tvar[0].axes, wght_1());
	let coords: Vec<String> = tvar[0]
		.coordinates
		.iter()
		.map(|c| match c {
			Some(Delta::Point(x, y)) => format!("{},{}", x.value(), y.value()),
			other => panic!("{other:?}"),
		})
		.collect();
	assert_eq!(
		coords.join(" "),
		"257,0 -127,0 -128,58 -130,90 -130,62 -130,67 -130,32 -127,0 \
		 257,0 259,14 260,64 260,21 260,69 258,124 0,0 130,0 0,0 0,0"
	);
	Ok(())
}

#[test]
fn test_decompileTupleVariationStore_empty() -> Result<()> {
	assert!(decompile_tuple_variation_store("gvar", &[], 0, 5, Some(&[]), b"", 4, 4)?.is_empty());
	Ok(())
}

#[test]
fn test_getTupleSize() {
	let num_axes = 3;
	assert_eq!(tuple_size(0x8042, num_axes), 4 + num_axes * 2);
	assert_eq!(tuple_size(0x4077, num_axes), 4 + num_axes * 4);
	assert_eq!(tuple_size(0x2077, num_axes), 4);
	assert_eq!(tuple_size(11, num_axes), 4);
}

#[test]
fn test_inferRegion() {
	// `inferRegion_({"wght": -0.3, "wdth": 0.7})`, axis by axis.
	assert_eq!(infer_region(-0.3), (-0.3, 0.0));
	assert_eq!(infer_region(0.7), (0.0, 0.7));
}

// --- Delta arithmetic ------------------------------------------------------

#[test]
fn test_getCoordWidth() {
	assert_eq!(TupleVariation::new(0).coord_width(), 0);
	assert_eq!(TupleVariation::new(1).coord_width(), 0);
	assert_eq!(var(BTreeMap::new(), vec![None, pt(0, 0)]).coord_width(), 2);
	let mut cvar_tuple = var(BTreeMap::new(), vec![None, cvt(0)]);
	assert_eq!(cvar_tuple.coord_width(), 1);
	cvar_tuple.coordinates[1] = fcvt(0.0);
	assert_eq!(cvar_tuple.coord_width(), 1);
	// The `TypeError` for a string delta cannot happen with typed deltas.
}

#[test]
fn test_scaleDeltas_cvar() {
	let mut v = var(BTreeMap::new(), vec![cvt(100), None]);
	v.scale_deltas(1.0);
	assert_eq!(v.coordinates, [cvt(100), None]);
	v.scale_deltas(0.333);
	let Some(Delta::Value(n)) = v.coordinates[0] else {
		panic!()
	};
	assert!((n.value() - 33.3).abs() < 1e-7);
	assert_eq!(v.coordinates[1], None);
	v.scale_deltas(0.0);
	assert_eq!(values(&v.coordinates), [Some(vec![0.0]), None]);
}

#[test]
fn test_scaleDeltas_gvar() {
	let mut v = var(BTreeMap::new(), vec![pt(100, 200), None]);
	v.scale_deltas(1.0);
	assert_eq!(v.coordinates, [pt(100, 200), None]);
	v.scale_deltas(0.333);
	let Some(Delta::Point(x, y)) = v.coordinates[0] else {
		panic!()
	};
	assert!((x.value() - 33.3).abs() < 1e-7);
	assert!((y.value() - 66.6).abs() < 1e-7);
	assert_eq!(v.coordinates[1], None);
	v.scale_deltas(0.0);
	assert_eq!(values(&v.coordinates), [Some(vec![0.0, 0.0]), None]);
}

#[test]
fn test_roundDeltas_cvar() {
	let mut v = var(BTreeMap::new(), vec![fcvt(55.5), None, fcvt(99.9)]);
	v.round_deltas();
	assert_eq!(v.coordinates, [cvt(56), None, cvt(100)]);
}

#[test]
fn test_roundDeltas_gvar() {
	let mut v = var(
		BTreeMap::new(),
		vec![fpt(55.5, 100.0), None, fpt(99.9, 100.0)],
	);
	v.round_deltas();
	assert_eq!(v.coordinates, [pt(56, 100), None, pt(100, 100)]);
}

#[test]
fn test_calcInferredDeltas() -> Result<()> {
	let mut v = var(BTreeMap::new(), vec![pt(0, 0), None, None, None]);
	v.calc_inferred_deltas(&[(1.0, 1.0); 4], &[])?;
	assert_eq!(values(&v.coordinates), vec![Some(vec![0.0, 0.0]); 4]);
	Ok(())
}

#[test]
fn test_calcInferredDeltas_invalid() {
	use fontoxide_ttlib::Error;
	// cvar tuples can't have inferred deltas (TypeError)
	let err = var(BTreeMap::new(), vec![cvt(0)])
		.calc_inferred_deltas(&[], &[])
		.unwrap_err();
	assert!(err.to_string().contains("TypeError"), "{err}");
	// origCoords must have the same length as the coordinates (ValueError)
	let err = var(BTreeMap::new(), vec![pt(0, 0), None])
		.calc_inferred_deltas(&[], &[])
		.unwrap_err();
	assert!(
		err.to_string()
			.contains("Expected len(origCoords) == 2; found 0"),
		"{err}"
	);
	// at least 4 phantom points required
	let err = var(BTreeMap::new(), vec![pt(0, 0), None])
		.calc_inferred_deltas(&[(0.0, 0.0); 2], &[])
		.unwrap_err();
	assert!(matches!(err, Error::Assertion(_)), "{err}");
	// endPts not in increasing order
	let mut coordinates = vec![pt(0, 0)];
	coordinates.extend([None; 5]);
	let err = var(BTreeMap::new(), coordinates)
		.calc_inferred_deltas(&[(0.0, 0.0); 6], &[1, 0])
		.unwrap_err();
	assert!(matches!(err, Error::Assertion(_)), "{err}");
}

#[test]
fn test_optimize() -> Result<()> {
	let mut v = var(wght_1(), vec![pt(0, 0); 5]);
	v.optimize(&[(0.0, 0.0); 5], &[0], 0.5)?;
	assert_eq!(v.coordinates, [None; 5]);
	Ok(())
}

#[test]
fn test_sum_deltas_gvar() -> Result<()> {
	let mut var1 = var(
		BTreeMap::new(),
		vec![
			pt(-20, 0),
			pt(-20, 0),
			pt(20, 0),
			pt(20, 0),
			pt(0, 0),
			pt(0, 0),
			pt(0, 0),
			pt(0, 0),
		],
	);
	let var2 = var(
		BTreeMap::new(),
		vec![
			pt(-10, 0),
			pt(-10, 0),
			pt(10, 0),
			pt(10, 0),
			pt(0, 0),
			pt(20, 0),
			pt(0, 0),
			pt(0, 0),
		],
	);
	var1.add_deltas(&var2)?;
	assert_eq!(
		var1.coordinates,
		[
			pt(-30, 0),
			pt(-30, 0),
			pt(30, 0),
			pt(30, 0),
			pt(0, 0),
			pt(20, 0),
			pt(0, 0),
			pt(0, 0),
		]
	);
	Ok(())
}

#[test]
fn test_sum_deltas_gvar_invalid_length() {
	let mut var1 = var(BTreeMap::new(), vec![pt(1, 2)]);
	let var2 = var(BTreeMap::new(), vec![pt(1, 2), pt(3, 4)]);
	let err = var1.add_deltas(&var2).unwrap_err();
	assert!(
		err.to_string().contains("deltas with different lengths"),
		"{err}"
	);
}

#[test]
fn test_sum_deltas_gvar_with_inferred_points() {
	let mut var1 = var(BTreeMap::new(), vec![pt(1, 2), None]);
	let var2 = var(BTreeMap::new(), vec![pt(2, 3), None]);
	let err = var1.add_deltas(&var2).unwrap_err();
	assert!(
		err.to_string().contains("deltas with inferred points"),
		"{err}"
	);
}

#[test]
fn test_sum_deltas_cvar() -> Result<()> {
	let mut var1 = var(wght_1(), vec![cvt(0), cvt(1), None, None]);
	let var2 = var(wght_1(), vec![None, cvt(2), None, cvt(3)]);
	let var3 = var(wght_1(), vec![None, None, None, cvt(4)]);
	var1.add_deltas(&var2)?;
	var1.add_deltas(&var3)?;
	assert_eq!(var1.coordinates, [cvt(0), cvt(3), None, cvt(7)]);
	Ok(())
}
