//! Port of `fontTools.varLib.avar.plan` (and of `fontTools.varLib.avarPlanner`,
//! which runs its [`main`]): planning the standard axis mappings of a variable
//! font (weight, width, slant and optical size) by measuring its glyphs, as an
//! `avar` table and a designspace snippet.
//!
//! fontTools passes a `glyphSetFunc` to the planning functions; here they take
//! the font and draw its glyph sets ([`GlyphSet::with_user_location`]), which is
//! what `font.getGlyphSet` does. Plotting the mapping (`--plot`) is not
//! ported: the normalized mapping is returned instead.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;

use fontoxide_misc::argparse::{Argument, Command, Ty};
use fontoxide_misc::cli_tools::make_output_file_name;
use fontoxide_misc::py::{float_repr, format_g_lower, parse_float};
use fontoxide_pens::NullPen;
use fontoxide_pens::area_pen::AreaPen;
use fontoxide_pens::statistics_pen::StatisticsPen;
use fontoxide_ttlib::tables::{Avar, Fvar};
use fontoxide_ttlib::{Font, GlyphSet, ReadOptions, TableType, Tag, WriteOptions};

use crate::errors::{Result, VarLibError};
use crate::models::{normalize_value, piecewise_linear_map};

/// A mapping of numbers, as a Python `dict`: in insertion order, with the keys
/// compared with `==` (see [`dict_set`]).
pub type Mapping = Vec<(f64, f64)>;

/// The glyphs to measure, in the order they are measured, with their
/// frequencies (the `glyphs` argument: a list has a frequency of 1 for each of
/// its glyphs, repeated glyphs included; a `dict` maps glyphs to their
/// frequencies).
pub type GlyphFrequencies = Vec<(String, f64)>;

/// `WEIGHTS`: the default weights to plan for.
pub const WEIGHTS: &[f64] = &[
	50.0, 100.0, 150.0, 200.0, 250.0, 300.0, 350.0, 400.0, 450.0, 500.0, 550.0, 600.0, 650.0,
	700.0, 750.0, 800.0, 850.0, 900.0, 950.0,
];

/// `WIDTHS`: the default widths (percentages) to plan for.
pub const WIDTHS: &[f64] = &[
	25.0, 37.5, 50.0, 62.5, 75.0, 87.5, 100.0, 112.5, 125.0, 137.5, 150.0, 162.5, 175.0, 187.5,
	200.0,
];

/// `SIZES`: the default optical sizes to plan for.
pub const SIZES: &[f64] = &[
	5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 18.0, 24.0, 30.0, 36.0, 48.0, 60.0, 72.0,
	96.0, 120.0, 144.0, 192.0, 240.0, 288.0,
];

/// `SAMPLES`: the default number of samples.
pub const SAMPLES: i64 = 8;

/// `SLANTS`: the default slant angles to plan for, those of the slopes −1 to 1
/// in steps of 1/20.
pub fn slants() -> Vec<f64> {
	(-20..=20)
		.map(|d| (f64::from(d) / 20.0).atan().to_degrees())
		.collect()
}

/// Sets `key` in a mapping as a Python `dict` does: an equal key keeps its
/// place (and its own value of the key), with the new value.
pub fn dict_set(mapping: &mut Mapping, key: f64, value: f64) {
	match mapping.iter_mut().find(|(k, _)| *k == key) {
		Some(entry) => entry.1 = value,
		None => mapping.push((key, value)),
	}
}

/// `mapping[key]`, a `KeyError` if there is no such key.
fn dict_get(mapping: &Mapping, key: f64) -> Result<f64> {
	mapping
		.iter()
		.find(|(k, _)| *k == key)
		.map(|e| e.1)
		.ok_or_else(|| {
			fontoxide_ttlib::Error::ttlib(format!("KeyError: {}", float_repr(key))).into()
		})
}

/// The items of a mapping sorted by key, then value (`sorted(d.items())`).
fn sorted_items(mapping: &Mapping) -> Mapping {
	let mut items = mapping.clone();
	items.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
	items
}

fn zero_division() -> VarLibError {
	fontoxide_ttlib::Error::ttlib("ZeroDivisionError: float division by zero").into()
}

/// `a / b`, a `ZeroDivisionError` if `b` is zero.
fn div(a: f64, b: f64) -> Result<f64> {
	if b == 0.0 {
		return Err(zero_division());
	}
	Ok(a / b)
}

/// `math.log`: a `ValueError` outside its domain.
fn log(x: f64) -> Result<f64> {
	if x > 0.0 || x.is_nan() || x == f64::INFINITY {
		Ok(x.ln())
	} else {
		Err(fontoxide_ttlib::Error::value("math domain error").into())
	}
}

/// `math.exp`: an `OverflowError` if the result is too large.
fn exp(x: f64) -> Result<f64> {
	let y = x.exp();
	if y.is_infinite() && x.is_finite() {
		return Err(fontoxide_ttlib::Error::ttlib("OverflowError: math range error").into());
	}
	Ok(y)
}

/// `math.tan`: a `ValueError` for infinities.
fn tan(x: f64) -> Result<f64> {
	if x.is_infinite() {
		return Err(fontoxide_ttlib::Error::value("math domain error").into());
	}
	Ok(x.tan())
}

/// `normalizeLinear`: linearly normalizes `value` in `[range_min, range_max]`
/// to `[0, 1]`, with extrapolation.
pub fn normalize_linear(value: f64, range_min: f64, range_max: f64) -> Result<f64> {
	div(value - range_min, range_max - range_min)
}

/// `interpolateLinear`: linear interpolation between `a` and `b`, with `t`
/// typically in `[0, 1]`.
pub fn interpolate_linear(t: f64, a: f64, b: f64) -> Result<f64> {
	Ok(a + t * (b - a))
}

/// `normalizeLog`: logarithmically normalizes `value` in
/// `[range_min, range_max]` to `[0, 1]`, with extrapolation.
pub fn normalize_log(value: f64, range_min: f64, range_max: f64) -> Result<f64> {
	let log_min = log(range_min)?;
	let log_max = log(range_max)?;
	div(log(value)? - log_min, log_max - log_min)
}

/// `interpolateLog`: logarithmic interpolation between `a` and `b`, with `t`
/// typically in `[0, 1]`.
pub fn interpolate_log(t: f64, a: f64, b: f64) -> Result<f64> {
	let log_a = log(a)?;
	let log_b = log(b)?;
	exp(log_a + t * (log_b - log_a))
}

/// `normalizeDegrees`: angularly normalizes `value` in `[range_min, range_max]`
/// to `[0, 1]`, with extrapolation.
pub fn normalize_degrees(value: f64, range_min: f64, range_max: f64) -> Result<f64> {
	let tan_min = tan(range_min.to_radians())?;
	let tan_max = tan(range_max.to_radians())?;
	div(tan(value.to_radians())? - tan_min, tan_max - tan_min)
}

/// `measureWeight`: the perceptual average weight of the glyphs: their ink
/// area, weighted by their advance widths and frequencies.
pub fn measure_weight(glyph_set: &GlyphSet<'_>, glyphs: &[(String, f64)]) -> Result<f64> {
	let components: &dyn fontoxide_pens::GlyphSet = glyph_set;
	let mut wght_sum = 0.0;
	let mut wdth_sum = 0.0;
	for (glyph_name, frequency) in glyphs {
		if *frequency == 0.0 {
			continue;
		}
		let mut pen = AreaPen::new_area(Some(components));
		glyph_set.draw(glyph_name, &mut pen)?;
		// The width is read after drawing, as fontTools does.
		let mult = glyph_set.metrics(glyph_name)?.width * frequency;
		wght_sum += mult * pen.value().abs();
		wdth_sum += mult;
	}
	div(wght_sum, wdth_sum)
}

/// `measureWidth`: the average advance width of the glyphs, weighted by their
/// frequencies.
pub fn measure_width(glyph_set: &GlyphSet<'_>, glyphs: &[(String, f64)]) -> Result<f64> {
	let mut wdth_sum = 0.0;
	let mut freq_sum = 0.0;
	for (glyph_name, frequency) in glyphs {
		if *frequency == 0.0 {
			continue;
		}
		glyph_set.draw(glyph_name, &mut NullPen)?;
		wdth_sum += glyph_set.metrics(glyph_name)?.width * frequency;
		freq_sum += frequency;
	}
	div(wdth_sum, freq_sum)
}

/// `measureSlant`: the perceptual average slant angle of the glyphs, in
/// degrees, weighted by their advance widths and frequencies.
pub fn measure_slant(glyph_set: &GlyphSet<'_>, glyphs: &[(String, f64)]) -> Result<f64> {
	let components: &dyn fontoxide_pens::GlyphSet = glyph_set;
	let mut slnt_sum = 0.0;
	let mut freq_sum = 0.0;
	for (glyph_name, frequency) in glyphs {
		if *frequency == 0.0 {
			continue;
		}
		let mut pen = StatisticsPen::new_statistics(Some(components));
		glyph_set.draw(glyph_name, &mut pen)?;
		let mult = glyph_set.metrics(glyph_name)?.width * frequency;
		slnt_sum += mult * pen.statistics().slant;
		freq_sum += mult;
	}
	Ok(-div(slnt_sum, freq_sum)?.atan().to_degrees())
}

/// A triple of minimum, default and maximum.
pub type Triple = (f64, f64, f64);

/// `"%g:%g:%g" % triple`.
fn g3(a: f64, b: f64, c: f64) -> String {
	format!("{}:{}:{}", g(a), g(b), g(c))
}

/// `"%g" % v`.
fn g(v: f64) -> String {
	format_g_lower(v, 6)
}

/// The measurements at the minimum, default and maximum of `design_triple`.
fn measured(design_triple: Triple, measurements: &Mapping) -> Result<Triple> {
	Ok((
		dict_get(measurements, design_triple.0)?,
		dict_get(measurements, design_triple.1)?,
		dict_get(measurements, design_triple.2)?,
	))
}

/// `sanitizeWidth`: checks the width axis limits against the measured widths;
/// warns and returns `false` if they do not match.
pub fn sanitize_width(
	user_triple: Triple,
	design_triple: Triple,
	_pins: &Mapping,
	measurements: &Mapping,
) -> Result<bool> {
	let (min_val, default_val, max_val) = measured(design_triple, measurements)?;
	let calculated_min_val = user_triple.1 * div(min_val, default_val)?;
	let calculated_max_val = user_triple.1 * div(max_val, default_val)?;

	log::info!(
		"Original width axis limits: {}",
		g3(user_triple.0, user_triple.1, user_triple.2)
	);
	log::info!(
		"Calculated width axis limits: {}",
		g3(calculated_min_val, user_triple.1, calculated_max_val)
	);

	if div((calculated_min_val - user_triple.0).abs(), user_triple.1)? > 0.05
		|| div((calculated_max_val - user_triple.2).abs(), user_triple.1)? > 0.05
	{
		log::warn!("Calculated width axis min/max do not match user input.");
		log::warn!(
			"  Current width axis limits: {}",
			g3(user_triple.0, user_triple.1, user_triple.2)
		);
		log::warn!(
			"  Suggested width axis limits: {}",
			g3(calculated_min_val, user_triple.1, calculated_max_val)
		);
		return Ok(false);
	}
	Ok(true)
}

/// `sanitizeWeight`: checks the default of the weight axis against the measured
/// weights; warns, suggesting other limits, and returns `false` if it does not
/// match.
pub fn sanitize_weight(
	user_triple: Triple,
	design_triple: Triple,
	_pins: &Mapping,
	measurements: &Mapping,
) -> Result<bool> {
	let (u0, u1, u2) = user_triple;
	// `len(set(userTriple)) < 3`.
	if u0 == u1 || u1 == u2 || u0 == u2 {
		return Ok(true);
	}
	let (min_val, default_val, max_val) = measured(design_triple, measurements)?;
	let log_min = log(min_val)?;
	let log_default = log(default_val)?;
	let log_max = log(max_val)?;

	let t = div(u1 - u0, u2 - u0)?;
	let y = exp(log_min + t * (log_max - log_min))?;
	let t = div(y - min_val, max_val - min_val)?;
	let calculated_default_val = u0 + t * (u2 - u0);

	log::info!("Original weight axis limits: {}", g3(u0, u1, u2));
	log::info!(
		"Calculated weight axis limits: {}",
		g3(u0, calculated_default_val, u2)
	);

	if div((calculated_default_val - u1).abs(), u1)? > 0.05 {
		log::warn!("Calculated weight axis default does not match user input.");
		log::warn!("  Current weight axis limits: {}", g3(u0, u1, u2));
		log::warn!(
			"  Suggested weight axis limits, changing default: {}",
			g3(u0, calculated_default_val, u2)
		);

		let t = div(u2 - u0, u1 - u0)?;
		let y = exp(log_min + t * (log_default - log_min))?;
		let t = div(y - min_val, default_val - min_val)?;
		let calculated_max_val = u0 + t * (u1 - u0);
		log::warn!(
			"  Suggested weight axis limits, changing maximum: {}",
			g3(u0, u1, calculated_max_val)
		);

		let t = div(u0 - u2, u1 - u2)?;
		let y = exp(log_max + t * (log_default - log_max))?;
		let t = div(y - max_val, default_val - max_val)?;
		let calculated_min_val = u2 + t * (u1 - u2);
		log::warn!(
			"  Suggested weight axis limits, changing minimum: {}",
			g3(calculated_min_val, u1, u2)
		);
		return Ok(false);
	}
	Ok(true)
}

/// `sanitizeSlant`: checks the slant axis limits against the measured slants;
/// warns and returns `false` if they differ by more than a degree.
pub fn sanitize_slant(
	user_triple: Triple,
	design_triple: Triple,
	_pins: &Mapping,
	measurements: &Mapping,
) -> Result<bool> {
	let (m0, m1, m2) = measured(design_triple, measurements)?;
	log::info!(
		"Original slant axis limits: {}",
		g3(user_triple.0, user_triple.1, user_triple.2)
	);
	log::info!("Calculated slant axis limits: {}", g3(m0, m1, m2));

	if (m0 - user_triple.0).abs() > 1.0
		|| (m1 - user_triple.1).abs() > 1.0
		|| (m2 - user_triple.2).abs() > 1.0
	{
		log::warn!("Calculated slant axis min/default/max do not match user input.");
		log::warn!(
			"  Current slant axis limits: {}",
			g3(user_triple.0, user_triple.1, user_triple.2)
		);
		log::warn!("  Suggested slant axis limits: {}", g3(m0, m1, m2));
		return Ok(false);
	}
	Ok(true)
}

/// A `measureFunc`: the glyph-set-wide measurement of an axis.
pub type MeasureFn = fn(&GlyphSet<'_>, &[(String, f64)]) -> Result<f64>;
/// A `normalizeFunc` or `interpolateFunc`: `(value, min, max)` or `(t, a, b)`.
pub type ScaleFn = fn(f64, f64, f64) -> Result<f64>;
/// A `sanitizeFunc`: `(userTriple, designTriple, pins, measurements)`.
pub type SanitizeFn = fn(Triple, Triple, &Mapping, &Mapping) -> Result<bool>;

/// The optional arguments of the planning functions.
#[derive(Debug, Clone, Default)]
pub struct PlanOptions {
	/// The number of samples (`samples`); [`SAMPLES`] if `None`.
	pub samples: Option<i64>,
	/// The glyphs to measure (`glyphs`); all glyphs of the font if `None`.
	pub glyphs: Option<GlyphFrequencies>,
	/// The design limits of the axis (`designLimits`), the axis limits if
	/// `None`.
	pub design_limits: Option<Triple>,
	/// Mappings to pin in the output (`pins`).
	pub pins: Option<Mapping>,
	/// Whether to check the axis limits (`sanitize`; not for the optical size
	/// axis).
	pub sanitize: bool,
}

/// `pformat` of a mapping: its items sorted by key, on one line if it fits in
/// 80 columns, else one per line. A key for which `int_key` is true is
/// formatted as a Python `int`.
fn pformat(mapping: &Mapping, int_key: &dyn Fn(f64) -> bool) -> String {
	let items: Vec<String> = sorted_items(mapping)
		.iter()
		.map(|(k, v)| {
			let key = if int_key(*k) {
				format!("{}", *k as i64)
			} else {
				float_repr(*k)
			};
			format!("{key}: {}", float_repr(*v))
		})
		.collect();
	let line = format!("{{{}}}", items.join(", "));
	if line.chars().count() <= 80 {
		line
	} else {
		format!("{{{}}}", items.join(",\n "))
	}
}

/// The repr of a sorted list of values (`sorted(targetValues)`).
fn list_repr(values: &[f64], ints: bool) -> String {
	let items: Vec<String> = values
		.iter()
		.map(|v| {
			if ints {
				format!("{}", *v as i64)
			} else {
				float_repr(*v)
			}
		})
		.collect();
	format!("[{}]", items.join(", "))
}

/// Measures the glyphs at `{axis_tag: value}`.
fn measure_at(
	font: &mut Font,
	measure: MeasureFn,
	axis_tag: &str,
	value: f64,
	glyphs: &[(String, f64)],
) -> Result<f64> {
	let location = BTreeMap::from([(axis_tag.to_owned(), value)]);
	let glyph_set = GlyphSet::with_user_location(font, &location)?;
	measure(&glyph_set, glyphs)
}

/// `planAxis`: plans an axis, mapping `values` (in user coordinates) to the
/// values at which the measurements interpolate (logarithmically, linearly...)
/// between those at the limits and pins of the axis. Returns the mapping and
/// the normalized mapping; both are empty if the normalized mapping is about
/// the identity.
///
/// `measure` takes a glyph set and the glyphs, and returns the measurement;
/// `normalize` normalizes a value between a minimum and a maximum to 0..1;
/// `interpolate` interpolates a measurement for a normalized value between the
/// measurements at the minimum and the maximum. The glyph sets are those of
/// `font` (`glyphSetFunc`), at the axis values.
#[allow(clippy::too_many_arguments)]
pub fn plan_axis(
	measure: MeasureFn,
	normalize: ScaleFn,
	interpolate: ScaleFn,
	font: &mut Font,
	axis_tag: &str,
	axis_limits: Triple,
	values: &[f64],
	options: &PlanOptions,
	sanitize: Option<SanitizeFn>,
) -> Result<(Mapping, Mapping)> {
	plan_axis_impl(
		measure,
		normalize,
		interpolate,
		font,
		axis_tag,
		axis_limits,
		values,
		false,
		options,
		sanitize,
	)
}

/// [`plan_axis`]; `int_values` says whether the values are Python `int`s, which
/// only changes the logs.
#[allow(clippy::too_many_arguments)]
fn plan_axis_impl(
	measure: MeasureFn,
	normalize: ScaleFn,
	interpolate: ScaleFn,
	font: &mut Font,
	axis_tag: &str,
	axis_limits: Triple,
	values: &[f64],
	int_values: bool,
	options: &PlanOptions,
	sanitize: Option<SanitizeFn>,
) -> Result<(Mapping, Mapping)> {
	let (min_value, default_value, max_value) = axis_limits;
	let samples = options.samples.unwrap_or(SAMPLES);
	let glyphs: GlyphFrequencies = match &options.glyphs {
		Some(glyphs) => glyphs.clone(),
		None => GlyphSet::new(font)?
			.keys()
			.iter()
			.map(|g| (g.clone(), 1.0))
			.collect(),
	};
	let mut pins = options.pins.clone().unwrap_or_default();

	log::info!(
		"Axis limits min {} / default {} / max {}",
		g(min_value),
		g(default_value),
		g(max_value)
	);
	let triple = axis_limits;

	let design_limits = match options.design_limits {
		Some(d) => {
			log::info!(
				"Axis design-limits min {} / default {} / max {}",
				g(d.0),
				g(d.1),
				g(d.2)
			);
			d
		}
		None => triple,
	};

	if !pins.is_empty() {
		let items: Vec<String> = sorted_items(&pins)
			.iter()
			.map(|(k, v)| format!("({}, {})", float_repr(*k), float_repr(*v)))
			.collect();
		log::info!("Pins [{}]", items.join(", "));
	}
	// `pins.update({minValue: ..., defaultValue: ..., maxValue: ...})`: the
	// dict literal first keeps the last value of equal keys.
	let mut update = Mapping::new();
	dict_set(&mut update, min_value, design_limits.0);
	dict_set(&mut update, default_value, design_limits.1);
	dict_set(&mut update, max_value, design_limits.2);
	for (k, v) in update {
		dict_set(&mut pins, k, v);
	}
	let pin_keys: Vec<f64> = pins.iter().map(|e| e.0).collect();
	let int_key = |k: f64| int_values && !pin_keys.contains(&k);

	let mut out = Mapping::new();
	let mut out_normalized = Mapping::new();

	let mut axis_measurements = Mapping::new();
	let mut keys: Vec<f64> = vec![min_value, default_value, max_value];
	keys.extend(pins.iter().map(|e| e.0));
	keys.sort_by(f64::total_cmp);
	keys.dedup_by(|a, b| a == b);
	for value in keys {
		let design_value = dict_get(&pins, value)?;
		let m = measure_at(font, measure, axis_tag, value, &glyphs)?;
		dict_set(&mut axis_measurements, design_value, m);
	}

	if let Some(sanitize) = sanitize {
		log::info!("Sanitizing axis limit values for the `{axis_tag}` axis.");
		sanitize(triple, design_limits, &pins, &axis_measurements)?;
	}

	log::debug!(
		"Calculated average value:\n{}",
		pformat(&axis_measurements, &|_| false)
	);

	let sorted_pins = sorted_items(&pins);
	for pair in sorted_pins.windows(2) {
		let (range_min, target_min) = pair[0];
		let (range_max, target_max) = pair[1];
		let mut target_values: Vec<f64> = values
			.iter()
			.copied()
			.filter(|w| range_min < *w && *w < range_max)
			.collect();
		if target_values.is_empty() {
			continue;
		}
		target_values.sort_by(f64::total_cmp);
		target_values.dedup_by(|a, b| a == b);

		let normalized_min = normalize_value(range_min, triple, false)?;
		let normalized_max = normalize_value(range_max, triple, false)?;
		let normalized_target_min = normalize_value(target_min, design_limits, false)?;
		let normalized_target_max = normalize_value(target_max, design_limits, false)?;

		log::info!(
			"Planning target values {}.",
			list_repr(&target_values, int_values)
		);
		log::info!(
			"Sampling {samples} points in range {},{}.",
			g(range_min),
			g(range_max)
		);
		let mut value_measurements = axis_measurements.clone();
		for sample in 1..=samples {
			let value = range_min + (range_max - range_min) * sample as f64 / (samples + 1) as f64;
			log::debug!("Sampling value {}.", g(value));
			let design_value = piecewise_linear_map(value, &pins);
			let m = measure_at(font, measure, axis_tag, value, &glyphs)?;
			dict_set(&mut value_measurements, design_value, m);
		}
		log::debug!(
			"Sampled average value:\n{}",
			pformat(&value_measurements, &|_| false)
		);

		let mut measurement_value = Mapping::new();
		for (value, m) in sorted_items(&value_measurements) {
			dict_set(&mut measurement_value, m, value);
		}

		dict_set(&mut out, range_min, target_min);
		dict_set(&mut out_normalized, normalized_min, normalized_target_min);
		for &value in &target_values {
			let t = normalize(value, range_min, range_max)?;
			let target_measurement = interpolate(
				t,
				dict_get(&value_measurements, target_min)?,
				dict_get(&value_measurements, target_max)?,
			)?;
			let target_value = piecewise_linear_map(target_measurement, &measurement_value);
			log::debug!("Planned mapping value {} to {}.", g(value), g(target_value));
			dict_set(&mut out, value, target_value);
			let value_normalized = normalized_min
				+ div(value - range_min, range_max - range_min)?
					* (normalized_max - normalized_min);
			dict_set(
				&mut out_normalized,
				value_normalized,
				normalized_target_min
					+ div(target_value - target_min, target_max - target_min)?
						* (normalized_target_max - normalized_target_min),
			);
		}
		dict_set(&mut out, range_max, target_max);
		dict_set(&mut out_normalized, normalized_max, normalized_target_max);
	}

	log::info!(
		"Planned mapping for the `{axis_tag}` axis:\n{}",
		pformat(&out, &int_key)
	);
	log::info!(
		"Planned normalized mapping for the `{axis_tag}` axis:\n{}",
		pformat(&out_normalized, &|_| false)
	);

	if out_normalized.iter().all(|(k, v)| (k - v).abs() < 0.01) {
		log::info!("Detected identity mapping for the `{axis_tag}` axis. Dropping.");
		out.clear();
		out_normalized.clear();
	}

	Ok((out, out_normalized))
}

/// The planning function of an axis (`planWeightAxis` and so on): takes the
/// font, the axis limits and the values to plan for (the defaults if `None`).
pub type PlanFn = fn(&mut Font, Triple, Option<&[f64]>, &PlanOptions) -> Result<(Mapping, Mapping)>;

/// `planWeightAxis`: plans a weight (`wght`) axis for `weights`
/// ([`WEIGHTS`] if `None`): the weights interpolate the measured
/// [`measure_weight`] logarithmically. See [`plan_axis`].
pub fn plan_weight_axis(
	font: &mut Font,
	axis_limits: Triple,
	weights: Option<&[f64]>,
	options: &PlanOptions,
) -> Result<(Mapping, Mapping)> {
	plan_axis_impl(
		measure_weight,
		normalize_linear,
		interpolate_log,
		font,
		"wght",
		axis_limits,
		weights.unwrap_or(WEIGHTS),
		weights.is_none(),
		options,
		options.sanitize.then_some(sanitize_weight as SanitizeFn),
	)
}

/// `planWidthAxis`: plans a width (`wdth`) axis for `widths` (percentages;
/// [`WIDTHS`] if `None`): the widths interpolate the measured
/// [`measure_width`] linearly. See [`plan_axis`].
pub fn plan_width_axis(
	font: &mut Font,
	axis_limits: Triple,
	widths: Option<&[f64]>,
	options: &PlanOptions,
) -> Result<(Mapping, Mapping)> {
	plan_axis_impl(
		measure_width,
		normalize_linear,
		interpolate_linear,
		font,
		"wdth",
		axis_limits,
		widths.unwrap_or(WIDTHS),
		false,
		options,
		options.sanitize.then_some(sanitize_width as SanitizeFn),
	)
}

/// `planSlantAxis`: plans a slant (`slnt`) axis for `slants` (angles;
/// [`slants`] if `None`): the tangents of the slants interpolate the measured
/// [`measure_slant`] linearly. See [`plan_axis`].
pub fn plan_slant_axis(
	font: &mut Font,
	axis_limits: Triple,
	slants: Option<&[f64]>,
	options: &PlanOptions,
) -> Result<(Mapping, Mapping)> {
	let defaults;
	let values = match slants {
		Some(s) => s,
		None => {
			defaults = self::slants();
			&defaults
		}
	};
	plan_axis_impl(
		measure_slant,
		normalize_degrees,
		interpolate_linear,
		font,
		"slnt",
		axis_limits,
		values,
		false,
		options,
		options.sanitize.then_some(sanitize_slant as SanitizeFn),
	)
}

/// `planOpticalSizeAxis`: plans an optical size (`opsz`) axis for `sizes`
/// ([`SIZES`] if `None`): the logarithms of the sizes interpolate the measured
/// [`measure_weight`] logarithmically. See [`plan_axis`]. There is no
/// sanitizing for this axis.
pub fn plan_optical_size_axis(
	font: &mut Font,
	axis_limits: Triple,
	sizes: Option<&[f64]>,
	options: &PlanOptions,
) -> Result<(Mapping, Mapping)> {
	plan_axis_impl(
		measure_weight,
		normalize_log,
		interpolate_log,
		font,
		"opsz",
		axis_limits,
		sizes.unwrap_or(SIZES),
		sizes.is_none(),
		options,
		None,
	)
}

/// `makeDesignspaceSnippet`: the designspace `<axis>` element of an axis, with
/// its mapping.
pub fn make_designspace_snippet(
	axis_tag: &str,
	axis_name: &str,
	axis_limit: Triple,
	mapping: &Mapping,
) -> String {
	let mut snippet = format!(
		"    <axis tag=\"{axis_tag}\" name=\"{axis_name}\" minimum=\"{}\" default=\"{}\" maximum=\"{}\"",
		g(axis_limit.0),
		g(axis_limit.1),
		g(axis_limit.2)
	);
	if mapping.is_empty() {
		snippet.push_str("/>");
		return snippet;
	}
	snippet.push_str(">\n");
	for (key, value) in mapping {
		let _ = writeln!(
			snippet,
			"      <map input=\"{}\" output=\"{}\"/>",
			g(*key),
			g(*value)
		);
	}
	snippet.push_str("    </axis>");
	snippet
}

/// `addEmptyAvar`: adds an `avar` table with empty segment maps for the axes of
/// `fvar`.
pub fn add_empty_avar(font: &mut Font) -> Result<()> {
	let tags: Vec<Tag> = font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| Tag::new(&a.axis_tag))
		.collect();
	let mut avar = Avar::default();
	for tag in tags {
		avar.segments.insert(tag, Vec::new());
	}
	font.insert(Box::new(avar));
	Ok(())
}

/// The arguments of [`process_axis`] given as strings on the command line.
#[derive(Debug, Clone, Default)]
pub struct AxisArgs<'a> {
	/// The values to plan for, separated by whitespace (`values`).
	pub values: Option<&'a str>,
	/// The number of samples (`samples`).
	pub samples: Option<i64>,
	/// The glyphs to measure (`glyphs`).
	pub glyphs: Option<GlyphFrequencies>,
	/// `min:default:max` in design units (`designLimits`).
	pub design_limits: Option<&'a str>,
	/// `before:after` pins, separated by whitespace (`pins`).
	pub pins: Option<&'a str>,
	/// Whether to check the axis limits (`sanitize`).
	pub sanitize: bool,
}

fn float(s: &str) -> Result<f64> {
	parse_float(s).ok_or_else(|| {
		fontoxide_ttlib::Error::value(format!(
			"could not convert string to float: {}",
			fontoxide_misc::py::str_repr(s)
		))
		.into()
	})
}

/// `processAxis`: plans the axis `axis_tag` of the font with `plan`, sets its
/// normalized mapping in `avar` (adding the table if needed), and returns its
/// designspace snippet; an empty string if the font has no such axis.
pub fn process_axis(
	font: &mut Font,
	plan: PlanFn,
	axis_tag: &str,
	axis_name: &str,
	args: &AxisArgs<'_>,
) -> Result<String> {
	let Some(axis_limits) = font
		.require::<Fvar>()?
		.axes
		.iter()
		.find(|a| a.tag_text() == axis_tag)
		.map(|a| (a.min_value, a.default_value, a.max_value))
	else {
		return Ok(String::new());
	};
	let tag = Tag::from_str_padded(axis_tag)
		.ok_or_else(|| fontoxide_ttlib::Error::value(format!("invalid tag {axis_tag:?}")))?;

	log::info!("Planning {axis_name} axis.");

	let existing_mapping = match font.get::<Avar>()? {
		Some(avar) => {
			let existing = avar.segments.get(&tag).cloned().ok_or_else(|| {
				fontoxide_ttlib::Error::ttlib(format!(
					"KeyError: {}",
					fontoxide_misc::py::str_repr(axis_tag)
				))
			})?;
			avar.segments.insert(tag, Vec::new());
			Some(existing)
		}
		None => None,
	};

	let values = match args.values {
		Some(v) => Some(
			v.split_whitespace()
				.map(float)
				.collect::<Result<Vec<f64>>>()?,
		),
		None => None,
	};

	let design_limits = match args.design_limits {
		Some(d) => {
			let limits = d.split(':').map(float).collect::<Result<Vec<f64>>>()?;
			if !(limits.len() == 3 && limits[0] <= limits[1] && limits[1] <= limits[2]) {
				return Err(fontoxide_ttlib::Error::Assertion(
					"len(designLimits) == 3 and designLimits[0] <= designLimits[1] <= designLimits[2]"
						.into(),
				)
				.into());
			}
			Some((limits[0], limits[1], limits[2]))
		}
		None => None,
	};

	let pins = match args.pins {
		Some(p) => {
			let mut new_pins = Mapping::new();
			for pin in p.split_whitespace() {
				let parts: Vec<&str> = pin.split(':').collect();
				let [before, after] = parts[..] else {
					let message = if parts.len() < 2 {
						format!(
							"not enough values to unpack (expected 2, got {})",
							parts.len()
						)
					} else {
						format!(
							"too many values to unpack (expected 2, got {})",
							parts.len()
						)
					};
					return Err(fontoxide_ttlib::Error::value(message).into());
				};
				dict_set(&mut new_pins, float(before)?, float(after)?);
			}
			Some(new_pins)
		}
		None => None,
	};

	let options = PlanOptions {
		samples: args.samples,
		glyphs: args.glyphs.clone(),
		design_limits,
		pins,
		sanitize: args.sanitize,
	};
	let (mapping, mapping_normalized) = plan(font, axis_limits, values.as_deref(), &options)?;

	if let Some(existing) = existing_mapping {
		log::info!(
			"Existing {axis_name} mapping:\n{}",
			pformat(&existing, &|_| false)
		);
	}

	if !mapping.is_empty() {
		if !font.contains(Avar::TAG) {
			add_empty_avar(font)?;
		}
		font.require::<Avar>()?
			.segments
			.insert(tag, mapping_normalized);
	} else if let Some(avar) = font.get::<Avar>()? {
		avar.segments.insert(tag, Vec::new());
	}

	Ok(make_designspace_snippet(
		axis_tag,
		axis_name,
		axis_limits,
		&mapping,
	))
}

const HELP: &str = "\
usage: fonttools varLib.avar.plan [-h] [-o OUTPUT_FILE] [--weights WEIGHTS]
                                  [--widths WIDTHS] [--slants SLANTS]
                                  [--sizes SIZES] [--samples SAMPLES] [-s]
                                  [-g GLYPHS]
                                  [--weight-design-limits WEIGHT_DESIGN_LIMITS]
                                  [--width-design-limits WIDTH_DESIGN_LIMITS]
                                  [--slant-design-limits SLANT_DESIGN_LIMITS]
                                  [--optical-size-design-limits OPTICAL_SIZE_DESIGN_LIMITS]
                                  [--weight-pins WEIGHT_PINS]
                                  [--width-pins WIDTH_PINS]
                                  [--slant-pins SLANT_PINS]
                                  [--optical-size-pins OPTICAL_SIZE_PINS] [-p]
                                  [-v | -q]
                                  varfont.ttf

Plan `avar` table for variable font

positional arguments:
  varfont.ttf           Variable-font file.

options:
  -h, --help            show this help message and exit
  -o, --output-file OUTPUT_FILE
                        Output font file name.
  --weights WEIGHTS     Space-separate list of weights to generate.
  --widths WIDTHS       Space-separate list of widths to generate.
  --slants SLANTS       Space-separate list of slants to generate.
  --sizes SIZES         Space-separate list of optical-sizes to generate.
  --samples SAMPLES     Number of samples.
  -s, --sanitize        Sanitize axis limits
  -g, --glyphs GLYPHS   Space-separate list of glyphs to use for sampling.
  --weight-design-limits WEIGHT_DESIGN_LIMITS
                        min:default:max in design units for the `wght` axis.
  --width-design-limits WIDTH_DESIGN_LIMITS
                        min:default:max in design units for the `wdth` axis.
  --slant-design-limits SLANT_DESIGN_LIMITS
                        min:default:max in design units for the `slnt` axis.
  --optical-size-design-limits OPTICAL_SIZE_DESIGN_LIMITS
                        min:default:max in design units for the `opsz` axis.
  --weight-pins WEIGHT_PINS
                        Space-separate list of before:after pins for the
                        `wght` axis.
  --width-pins WIDTH_PINS
                        Space-separate list of before:after pins for the
                        `wdth` axis.
  --slant-pins SLANT_PINS
                        Space-separate list of before:after pins for the
                        `slnt` axis.
  --optical-size-pins OPTICAL_SIZE_PINS
                        Space-separate list of before:after pins for the
                        `opsz` axis.
  -p, --plot            Plot the resulting mapping.
  -v, --verbose         Run more verbosely.
  -q, --quiet           Turn verbosity off.
";

const ARGUMENTS: &[Argument] = &[
	Argument::help(),
	Argument::option(&["-o", "--output-file"]),
	Argument::option(&["--weights"]),
	Argument::option(&["--widths"]),
	Argument::option(&["--slants"]),
	Argument::option(&["--sizes"]),
	Argument::option(&["--samples"]).ty(Ty::Int),
	Argument::flag(&["-s", "--sanitize"]),
	Argument::option(&["-g", "--glyphs"]),
	Argument::option(&["--weight-design-limits"]),
	Argument::option(&["--width-design-limits"]),
	Argument::option(&["--slant-design-limits"]),
	Argument::option(&["--optical-size-design-limits"]),
	Argument::option(&["--weight-pins"]),
	Argument::option(&["--width-pins"]),
	Argument::option(&["--slant-pins"]),
	Argument::option(&["--optical-size-pins"]),
	Argument::flag(&["-p", "--plot"]),
	Argument::flag(&["-v", "--verbose"]).group(0),
	Argument::flag(&["-q", "--quiet"]).group(0),
	Argument::positional("varfont.ttf"),
];

/// The command line of `fonttools varLib.avar.plan`.
pub const COMMAND: Command = Command {
	prog: "fonttools varLib.avar.plan",
	module: "varLib.avar.plan",
	help: HELP,
	arguments: ARGUMENTS,
	required_groups: &[],
};

/// `-g`/`--glyphs`: the glyphs separated by whitespace, each with a frequency
/// after `:` if any glyph has one (then repeated glyphs count once, with the
/// last frequency).
fn parse_glyphs(glyphs: &str) -> Result<GlyphFrequencies> {
	if !glyphs.contains(':') {
		return Ok(glyphs
			.split_whitespace()
			.map(|g| (g.to_owned(), 1.0))
			.collect());
	}
	let mut result: GlyphFrequencies = Vec::new();
	for g in glyphs.split_whitespace() {
		let (glyph, frequency) = if g.contains(':') {
			let parts: Vec<&str> = g.split(':').collect();
			let [glyph, frequency] = parts[..] else {
				return Err(fontoxide_ttlib::Error::value(format!(
					"too many values to unpack (expected 2, got {})",
					parts.len()
				))
				.into());
			};
			(glyph, float(frequency)?)
		} else {
			(g, 1.0)
		};
		match result.iter_mut().find(|(n, _)| n == glyph) {
			Some(entry) => entry.1 = frequency,
			None => result.push((glyph.to_owned(), frequency)),
		}
	}
	Ok(result)
}

/// `main`: plans the standard axis mappings of a variable font, prints their
/// designspace snippets to standard output and saves the font with the
/// mappings in `avar`. Returns the exit code.
pub fn main(args: &[&str]) -> Result<i32> {
	run(
		args,
		&mut std::io::stdout().lock(),
		&mut std::io::stderr().lock(),
	)
}

/// The `main` of `fonttools varLib.avarPlanner`, which runs [`main`] and
/// ignores its exit code.
///
/// UPSTREAM-BUG(B75): so "Not a variable font." exits 0 here, but 1 with
/// `varLib.avar.plan`.
pub fn avar_planner_main(args: &[&str]) -> Result<i32> {
	main(args)?;
	Ok(0)
}

/// [`main`], printing to `out` and the `argparse` messages to `err`.
pub fn run(args: &[&str], out: &mut dyn Write, err: &mut dyn Write) -> Result<i32> {
	let options = match COMMAND.parse_args(args, out, err)? {
		Ok(options) => options,
		Err(code) => return Ok(code),
	};

	log::set_max_level(if options.flag("--verbose") {
		log::LevelFilter::Debug
	} else if options.flag("--quiet") {
		log::LevelFilter::Warn
	} else {
		log::LevelFilter::Info
	});

	let font_path = options.str("varfont.ttf").unwrap_or_default();
	let mut font = Font::read(std::fs::read(font_path)?, ReadOptions::default())?;
	if !font.contains(Fvar::TAG) {
		log::error!("Not a variable font.");
		return Ok(1);
	}

	let glyphs = options.str("--glyphs").map(parse_glyphs).transpose()?;
	let samples = match options.str("--samples") {
		Some(s) => Some(
			fontoxide_misc::py::parse_int(s)
				.and_then(|n| i64::try_from(n).ok())
				.ok_or_else(|| {
					fontoxide_ttlib::Error::value(format!("invalid int value: {s:?}"))
				})?,
		),
		None => None,
	};
	if options.flag("--plot") {
		// LENIENT(L51): plotting is not ported; the normalized mapping is only in the
		// `avar` table.
		log::warn!("Plotting is not supported; ignoring --plot.");
	}

	let axes: [(PlanFn, &str, &str, &str, &str, &str); 4] = [
		(
			plan_weight_axis,
			"wght",
			"Weight",
			"--weights",
			"--weight-design-limits",
			"--weight-pins",
		),
		(
			plan_width_axis,
			"wdth",
			"Width",
			"--widths",
			"--width-design-limits",
			"--width-pins",
		),
		(
			plan_slant_axis,
			"slnt",
			"Slant",
			"--slants",
			"--slant-design-limits",
			"--slant-pins",
		),
		(
			plan_optical_size_axis,
			"opsz",
			"OpticalSize",
			"--sizes",
			"--optical-size-design-limits",
			"--optical-size-pins",
		),
	];
	let mut snippets = Vec::new();
	for (plan, tag, name, values, design_limits, pins) in axes {
		let args = AxisArgs {
			values: options.str(values),
			samples,
			glyphs: glyphs.clone(),
			design_limits: options.str(design_limits),
			pins: options.str(pins),
			sanitize: options.flag("--sanitize"),
		};
		snippets.push(process_axis(&mut font, plan, tag, name, &args)?);
	}

	log::info!("Designspace snippet:");
	for snippet in snippets {
		if !snippet.is_empty() {
			writeln!(out, "{snippet}")?;
		}
	}

	let outfile = match options.str("--output-file") {
		Some(o) => o.to_owned(),
		None => make_output_file_name(font_path, None, None, true, ".avar"),
	};
	if !outfile.is_empty() {
		log::info!("Saving {outfile}");
		let data = font.write(&WriteOptions::default())?;
		std::fs::write(&outfile, data)?;
	}
	Ok(0)
}
