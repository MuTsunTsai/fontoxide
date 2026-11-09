//! Helpers of the tests ported from fontTools' `Tests/varLib/instancer/instancer_test.py`.
//!
//! The fonts it builds with `feaLib`, `FontBuilder` and `varLib`, which are not ported yet,
//! are in `tests/data/instancer`, made with the same code by
//! `tools/reference/instancer_fixtures.py`.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Once;

use fontoxide_misc::fixed_tools::float_to_fixed_to_float;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::glyf::{ComponentPlacement, Number};
use fontoxide_ttlib::tables::tuple_variation::{Delta, TupleVariation};
use fontoxide_ttlib::tables::{Avar, Fvar, Glyf, Gsub, Hmtx, Vmtx};
use fontoxide_ttlib::var::Location;
use fontoxide_ttlib::{Font, ReadOptions, Tag, TtxOptions, WriteOptions};
use fontoxide_varlib::instancer::{
	AxisTriple, InstancerOptions, Limit, NormalizedAxisTriple, NormalizedLimits, UserLimits,
	instantiate_variable_font, renormalize_location,
};
use fontoxide_varlib::models::{normalize_value, piecewise_linear_map};

use super::common::*;

pub const NO_VARIATION_INDEX: u64 = 0xFFFF_FFFF;

/// A font of `tests/data/instancer`.
pub fn fixture(name: &str) -> TestResult<Font> {
	font_from_ttx(&format!(
		"{}/tests/data/instancer/{name}.ttx",
		env!("CARGO_MANIFEST_DIR")
	))
}

/// The `varfont` fixture of `conftest.py`.
pub fn varfont() -> TestResult<Font> {
	font_from_ttx(&instancer_data("PartialInstancerTest-VF.ttx"))
}

pub fn varfont2() -> TestResult<Font> {
	font_from_ttx(&instancer_data("PartialInstancerTest2-VF.ttx"))
}

pub fn varfont3() -> TestResult<Font> {
	font_from_ttx(&instancer_data("PartialInstancerTest3-VF.ttx"))
}

pub fn avar2_path() -> String {
	tests_path("ttLib/tables/data/Amstelvar-avar2.subset.ttf")
}

pub fn avar2_varfont() -> TestResult<Font> {
	open(&avar2_path())
}

/// `NormalizedAxisLimits`: one value pins an axis, and three or five are the triple and
/// its distances.
pub fn normalized(pairs: &[(&str, &[f64])]) -> NormalizedLimits {
	let mut out = BTreeMap::new();
	for (tag, values) in pairs {
		let (minimum, default, maximum, negative, positive) = match **values {
			[v] => (v, v, v, 1.0, 1.0),
			[a, b, c] => (a, b, c, 1.0, 1.0),
			[a, b, c, d, e] => (a, b, c, d, e),
			_ => panic!("expected 1, 3 or 5 values"),
		};
		out.insert(
			(*tag).to_owned(),
			NormalizedAxisTriple::new(minimum, default, maximum, negative, positive)
				.expect("in range"),
		);
	}
	NormalizedLimits(out)
}

/// `AxisLimits(...)`, as far as the functions of the instancer read them: not limited to
/// the range of `fvar`, with a missing default being that of `fvar`, as in `normalize`.
pub fn user_limits(
	font: &mut Font,
	pairs: &[(&str, Option<AxisTriple>)],
) -> TestResult<UserLimits> {
	let fvar = font.require::<Fvar>()?;
	let mut out = BTreeMap::new();
	for (tag, triple) in pairs {
		let axis = fvar.axes.iter().find(|a| a.tag_text() == *tag);
		let default = axis.map_or(0.0, |a| a.default_value);
		let triple = triple.expect("not dropped");
		out.insert(
			(*tag).to_owned(),
			Limit {
				minimum: triple.minimum.expect("minimum"),
				default: triple.default.unwrap_or(default),
				maximum: triple.maximum.expect("maximum"),
			},
		);
	}
	Ok(UserLimits(out))
}

/// `instantiateVariableFont(font, limits)`: not in place.
pub fn instantiated(font: &Font, pairs: &[(&str, Option<AxisTriple>)]) -> TestResult<Font> {
	let mut font = font.clone();
	instantiate(&mut font, pairs)?;
	Ok(font)
}

/// `instantiateVariableFont(font, limits, **options)`: not in place.
pub fn instantiated_with(
	font: &Font,
	pairs: &[(&str, Option<AxisTriple>)],
	options: &InstancerOptions,
) -> TestResult<Font> {
	let mut font = font.clone();
	instantiate_variable_font(&mut font, &limits(pairs), options)?;
	Ok(font)
}

/// `font.save(buf); TTFont(buf)`, recalculating the timestamp as by default, which makes saving
/// compile `head` even if nothing changed it.
pub fn reload_default(font: &mut Font) -> TestResult<Font> {
	Ok(Font::read(
		font.write(&WriteOptions::default())?,
		ReadOptions::default(),
	)?)
}

/// The axis tags of `fvar`.
pub fn axis_tags(font: &mut Font) -> TestResult<Vec<String>> {
	Ok(font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| a.tag_text())
		.collect())
}

/// `_get_coordinates`: the coordinates are not rounded (`round=noRound`). The phantom points
/// are made from integers.
pub fn get_coordinates(font: &mut Font, name: &str) -> TestResult<Vec<(f64, f64)>> {
	let order = font.glyph_order()?;
	let gid = font.glyph_id(name)?;
	let h = font.require::<Hmtx>()?.metrics.clone();
	let v = font.require::<Vmtx>()?.metrics.clone();
	let glyf = font.require::<Glyf>()?;
	let (mut coords, _) = glyf.coordinates_and_controls(gid, &order, &h, Some(&v))?;
	let glyph = &glyf.glyphs[&gid];
	if glyph.is_composite() {
		for (c, component) in coords.iter_mut().zip(&glyph.components) {
			if let ComponentPlacement::Offset { x, y } = component.placement {
				*c = (x.value(), y.value());
			}
		}
	} else if glyph.number_of_contours > 0 {
		coords[..glyph.coordinates.len()].copy_from_slice(&glyph.coordinates);
	}
	Ok(coords)
}

fn remove_between(s: &str, start: &str, end: &str) -> String {
	let mut out = String::with_capacity(s.len());
	let mut rest = s;
	while let Some(i) = rest.find(start) {
		let after = &rest[i + start.len()..];
		let Some(j) = after.find(end) else {
			break;
		};
		out.push_str(&rest[..i]);
		rest = &after[j + end.len()..];
	}
	out.push_str(rest);
	out
}

/// `stripVariableItemsFromTTX`, which keeps `sfntVersion` by default, unlike `strip_ttx`.
pub fn strip_variable_items(ttx: &str) -> Vec<String> {
	let s = ttx.replace("\r\n", "\n");
	let s = remove_between(&s, " ttLibVersion=\"", "\"");
	let s = remove_between(&s, "<checkSumAdjustment value=\"", "\"/>");
	let s = remove_between(&s, "<modified value=\"", "\"/>");
	let s = remove_between(&s, "<created value=\"", "\"/>");
	s.split_inclusive('\n').map(str::to_owned).collect()
}

/// `font.saveXML` of everything, with the stripping of `stripVariableItemsFromTTX`.
pub fn stripped_ttx(font: &mut Font) -> TestResult<Vec<String>> {
	Ok(strip_variable_items(&font.to_ttx(&TtxOptions {
		newline: "\n".into(),
		..Default::default()
	})?))
}

/// `_dump_ttx`.
pub fn dump_ttx(font: &mut Font) -> TestResult<Vec<String>> {
	let mut reloaded = Font::read(
		font.write(&WriteOptions::default())?,
		ReadOptions {
			recalc_bboxes: false,
			recalc_timestamp: false,
			..Default::default()
		},
	)?;
	stripped_ttx(&mut reloaded)
}

/// `_get_expected_instance_ttx`, with the suffix of the overlap mode.
pub fn expected_instance_ttx(
	name: &str,
	locations: &[&str],
	suffix: &str,
) -> TestResult<Vec<String>> {
	let path = instancer_data(&format!(
		"test_results/{name}-VF-instance-{}{suffix}.ttx",
		locations.join(",")
	));
	Ok(strip_variable_items(&std::fs::read_to_string(path)?))
}

/// Compares TTX lines, reporting the first difference only.
pub fn assert_same_ttx(actual: &[String], expected: &[String], what: &str) {
	if actual != expected {
		for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
			assert_eq!(a, e, "{what}, line {}", i + 1);
		}
		panic!(
			"{what}: {} lines, expected {}",
			actual.len(),
			expected.len()
		);
	}
}

/// The default tolerance of `pytest.approx`.
pub fn approx(a: f64, b: f64) -> bool {
	a == b || (a - b).abs() <= (1e-6 * b.abs()).max(1e-12)
}

/// `round`: to the even number on ties.
pub fn py_round(v: f64) -> f64 {
	v.round_ties_even()
}

pub fn value_delta(v: i64) -> Option<Delta> {
	Some(Delta::Value(Number::Int(v)))
}

pub fn point_delta(x: i64, y: i64) -> Option<Delta> {
	Some(Delta::Point(Number::Int(x), Number::Int(y)))
}

pub type Region = BTreeMap<String, (f64, f64, f64)>;

/// A region from `(tag, (start, peak, end))` pairs.
pub fn region(pairs: &[(&str, (f64, f64, f64))]) -> Region {
	pairs.iter().map(|(t, r)| ((*t).to_owned(), *r)).collect()
}

/// `TupleVariation(axes, values)`, of single deltas.
pub fn tuple_variation(pairs: &[(&str, (f64, f64, f64))], values: &[i64]) -> TupleVariation {
	TupleVariation {
		axes: region(pairs),
		coordinates: values.iter().map(|v| value_delta(*v)).collect(),
	}
}

/// `TupleVariation.__eq__`: where `1 == 1.0`.
pub fn same_variation(a: &TupleVariation, b: &TupleVariation) -> bool {
	fn delta(d: &Option<Delta>) -> Option<(f64, Option<f64>)> {
		match d {
			Some(Delta::Value(v)) => Some((v.value(), None)),
			Some(Delta::Point(x, y)) => Some((x.value(), Some(y.value()))),
			None => None,
		}
	}
	a.axes == b.axes
		&& a.coordinates.iter().map(delta).collect::<Vec<_>>()
			== b.coordinates.iter().map(delta).collect::<Vec<_>>()
}

/// `buildDevice({size: delta})`.
pub fn build_device(size: u16, delta: i64) -> Box<ot::Device> {
	let delta_format = if delta > -3 && delta < 2 {
		1
	} else if delta > -9 && delta < 8 {
		2
	} else {
		3
	};
	Box::new(ot::Device {
		start_size: size,
		end_size: size,
		delta_format,
		delta_value: vec![delta],
	})
}

/// `buildVarDevTable(varIdx)`.
pub fn build_var_dev_table(var_idx: u32) -> Box<ot::Device> {
	Box::new(ot::Device {
		start_size: (var_idx >> 16) as u16,
		end_size: (var_idx & 0xFFFF) as u16,
		delta_format: 0x8000,
		delta_value: Vec::new(),
	})
}

/// `_avar2_final_coords`: the final normalized coordinates per the avar2 spec pipeline.
pub fn avar2_final_coords(font: &mut Font, user_loc: &[(&str, f64)]) -> TestResult<Location> {
	let mut norm = Location::new();
	let axes: Vec<_> = font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| (a.tag_text(), a.min_value, a.default_value, a.max_value))
		.collect();
	for (tag, minimum, default, maximum) in axes {
		let value = user_loc
			.iter()
			.find(|(t, _)| *t == tag)
			.map_or(default, |(_, v)| *v);
		norm.insert(
			tag,
			normalize_value(value, (minimum, default, maximum), false)?,
		);
	}
	if !font.contains(Tag::new(b"avar")) {
		return Ok(norm);
	}
	if font.require::<Avar>()?.major_version < 2 {
		let avar = font.require::<Avar>()?;
		let mut mapped = Location::new();
		for (tag, value) in norm {
			let t = Tag::from_str_padded(&tag).expect("a tag");
			let value = avar
				.segments
				.get(&t)
				.map_or(value, |m| piecewise_linear_map(value, m));
			mapped.insert(tag, value);
		}
		return Ok(mapped);
	}
	Ok(renormalize_location(&norm, font)?)
}

/// `floatToFixedToFloat(v, 14)`.
pub fn f2dot14(v: f64) -> f64 {
	float_to_fixed_to_float(v, 14)
}

/// Sets the conditions back to the exact values that the fonts of the tests have in
/// memory, before TTX quantizes them.
pub fn restore_condition_values(font: &mut Font, exact: &[f64]) -> TestResult {
	fn walk(condition: &mut ot::ConditionTable, exact: &[f64]) {
		match condition {
			ot::ConditionTable::Format1(c) => {
				for v in [&mut c.filter_range_min_value, &mut c.filter_range_max_value] {
					if let Some(e) = exact.iter().find(|e| f2dot14(**e) == *v) {
						*v = *e;
					}
				}
			}
			ot::ConditionTable::Format3(c) => {
				c.condition_table
					.iter_mut()
					.flatten()
					.for_each(|c| walk(c, exact));
			}
			ot::ConditionTable::Format4(c) => {
				c.condition_table
					.iter_mut()
					.flatten()
					.for_each(|c| walk(c, exact));
			}
			ot::ConditionTable::Format5(c) => {
				c.condition_table.iter_mut().for_each(|c| walk(c, exact));
			}
			_ => {}
		}
	}
	let gsub = font.require::<Gsub>()?;
	let variations = gsub.feature_variations.as_mut().ok_or("no variations")?;
	for record in &mut variations.feature_variation_record {
		if let Some(set) = &mut record.condition_set {
			set.condition_table
				.iter_mut()
				.flatten()
				.for_each(|c| walk(c, exact));
		}
	}
	for record in &mut variations.lookup_variation_record {
		let table = record.feature_lookups_table.as_mut().ok_or("no table")?;
		for r in &mut table.lookup_condition_record {
			r.condition_table.iter_mut().for_each(|c| walk(c, exact));
		}
	}
	Ok(())
}

thread_local! {
	static CAPTURED: RefCell<Option<String>> = const { RefCell::new(None) };
}

struct Capture;

impl log::Log for Capture {
	fn enabled(&self, _: &log::Metadata<'_>) -> bool {
		true
	}

	fn log(&self, record: &log::Record<'_>) {
		CAPTURED.with(|c| {
			if let Some(text) = c.borrow_mut().as_mut() {
				text.push_str(&record.args().to_string());
				text.push('\n');
			}
		});
	}

	fn flush(&self) {}
}

static CAPTURE: Capture = Capture;
static INIT: Once = Once::new();

/// `caplog.at_level(logging.WARNING)`: what `f` logs on this thread, and its result.
pub fn capture_warnings<R>(f: impl FnOnce() -> R) -> (String, R) {
	INIT.call_once(|| {
		log::set_logger(&CAPTURE).expect("no other logger");
		log::set_max_level(log::LevelFilter::Warn);
	});
	CAPTURED.with(|c| *c.borrow_mut() = Some(String::new()));
	let result = f();
	let text = CAPTURED.with(|c| c.borrow_mut().take()).unwrap_or_default();
	(text, result)
}

/// A unique directory under the temporary one (`tmpdir`), removed when dropped.
pub struct TempDir(pub std::path::PathBuf);

impl TempDir {
	pub fn new(name: &str) -> TestResult<Self> {
		let path =
			std::env::temp_dir().join(format!("fontoxide-instancer-{}-{name}", std::process::id()));
		std::fs::create_dir_all(&path)?;
		Ok(TempDir(path))
	}

	pub fn file(&self, name: &str) -> String {
		self.0.join(name).to_string_lossy().into_owned()
	}
}

impl Drop for TempDir {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}
