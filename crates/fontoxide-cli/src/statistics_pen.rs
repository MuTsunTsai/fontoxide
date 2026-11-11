//! The command line of `fontTools.pens.statisticsPen` (`main` and `_test`):
//! the glyph shape statistics of a font. It is here rather than in
//! `fontoxide-pens`, which does not depend on `fontoxide-ttlib`.

use std::collections::BTreeMap;
use std::io::Write;

use fontoxide_misc::argparse::{Argument, Command, Nargs};
use fontoxide_misc::py::{format_g_lower, parse_int, str_repr};
use fontoxide_misc::transform::IDENTITY;
use fontoxide_pens::statistics_pen::{Statistics, StatisticsControlPen, StatisticsPen};
use fontoxide_pens::transform_pen::TransformPen;
use fontoxide_ttlib::tables::Head;
use fontoxide_ttlib::{Error, Font, GlyphSet, ReadOptions, Result};

/// The help (`-h`).
const HELP: &str = "\
usage: fonttools pens.statisticsPen [-h] [-y <number>] [-c] [-q]
                                    [--variations AXIS=LOC]
                                    font.ttf [glyph-name ...]

Report font glyph shape geometricsl statistics

positional arguments:
  font.ttf              Font file.
  glyph-name            Glyph names.

options:
  -h, --help            show this help message and exit
  -y <number>           Face index into a collection to open. Zero based.
  -c, --control         Use the control-box pen instead of the Green therem.
  -q, --quiet           Only report font-wide statistics.
  --variations AXIS=LOC
                        List of space separated locations. A location consist
                        in the name of a variation axis, followed by '=' and a
                        number. E.g.: wght=700 wdth=80. The default is the
                        location of the base master.
";

const COMMAND: Command = Command {
	prog: "fonttools pens.statisticsPen",
	module: "pens.statisticsPen",
	help: HELP,
	arguments: &[
		Argument::help(),
		Argument::positional("font.ttf"),
		Argument::positional("glyph-name").nargs(Nargs::ZeroOrMore),
		Argument::option(&["-y"]),
		Argument::flag(&["-c", "--control"]),
		Argument::flag(&["-q", "--quiet"]),
		Argument::option(&["--variations"]),
	],
	required_groups: &[],
};

/// The command line `fonttools pens.statisticsPen` (`main`), "Report font
/// glyph shape geometricsl statistics", printing to standard output. Returns
/// the exit code.
pub fn main(args: &[&str]) -> Result<i32> {
	run(
		args,
		&mut std::io::stdout().lock(),
		&mut std::io::stderr().lock(),
	)
}

/// [`main`], printing to `out`, and the usage errors to `err`.
pub fn run(args: &[&str], out: &mut dyn Write, err: &mut dyn Write) -> Result<i32> {
	let options = match COMMAND.parse_args(args, out, err)? {
		Ok(options) => options,
		Err(code) => return Ok(code),
	};
	let font_path = options.str("font.ttf").unwrap_or_default();
	let mut glyphs: Vec<String> = options.list("glyph-name").to_vec();
	let font_number = match options.str("-y") {
		Some(y) => parse_int(y).ok_or_else(|| invalid_int(y))?,
		None => 0,
	};

	let mut location = BTreeMap::new();
	for tag_v in options.str("--variations").unwrap_or("").split_whitespace() {
		let fields: Vec<&str> = tag_v.split('=').collect();
		let tag = fields[0].trim();
		let v = fields
			.get(1)
			.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
		let v = parse_int(v).ok_or_else(|| invalid_int(v))?;
		location.insert(tag.to_owned(), v as f64);
	}

	let mut font = Font::read(
		std::fs::read(font_path)?,
		ReadOptions {
			font_number: usize::try_from(font_number).ok(),
			..Default::default()
		},
	)?;
	if glyphs.is_empty() {
		glyphs = font.glyph_order()?.names().to_vec();
	}
	let upem = f64::from(font.require::<Head>()?.units_per_em);
	let glyph_set = GlyphSet::with_user_location(&mut font, &location)?;
	test(
		&glyph_set,
		upem,
		&glyphs,
		options.flag("--quiet"),
		options.flag("--control"),
		out,
	)?;
	Ok(0)
}

/// `int(value)` failing.
fn invalid_int(value: &str) -> Error {
	Error::value(format!(
		"invalid literal for int() with base 10: {}",
		str_repr(value)
	))
}

/// `"%g" % value`.
fn g(value: f64) -> String {
	format_g_lower(value, 6)
}

/// `-degrees(atan(value))`.
fn angle(value: f64) -> f64 {
	-(value.atan() * (180.0 / std::f64::consts::PI))
}

/// `a / b` for floats, failing as Python does on a zero divisor.
fn divide(a: f64, b: f64) -> Result<f64> {
	if b == 0.0 {
		return Err(Error::value("ZeroDivisionError: float division by zero"));
	}
	Ok(a / b)
}

/// `_test`: prints the statistics of the glyphs (unless `quiet`) and of the
/// font, measured with the control-box pen if `control`, else with
/// `StatisticsPen`.
fn test(
	glyph_set: &GlyphSet<'_>,
	upem: f64,
	glyphs: &[String],
	quiet: bool,
	control: bool,
	out: &mut dyn Write,
) -> Result<()> {
	let mut wght_sum = 0.0;
	let mut wght_sum_perceptual = 0.0;
	let mut wdth_sum = 0.0;
	let mut slnt_sum = 0.0;
	let mut slnt_sum_perceptual = 0.0;
	let scale = IDENTITY.scale(1.0 / upem, None);
	for glyph_name in glyphs {
		// `glyphset[glyph_name]`.
		if !glyph_set.contains(glyph_name) {
			return Err(Error::GlyphNotFound(glyph_name.clone()));
		}
		let (stats, moments) = if control {
			let mut pen = StatisticsControlPen::new_statistics_control(Some(glyph_set));
			glyph_set.draw(glyph_name, &mut TransformPen::new(&mut pen, scale))?;
			(pen.statistics(), None)
		} else {
			let mut pen = StatisticsPen::new_statistics(Some(glyph_set));
			glyph_set.draw(glyph_name, &mut TransformPen::new(&mut pen, scale))?;
			(pen.statistics(), Some(pen.inner.moments.clone()))
		};
		let width = glyph_set.metrics(glyph_name)?.width;
		wght_sum += stats.area.abs();
		wght_sum_perceptual += stats.area * width;
		wdth_sum += width;
		slnt_sum += stats.slant;
		slnt_sum_perceptual += stats.slant * width;

		if quiet {
			continue;
		}

		writeln!(out)?;
		writeln!(out, "glyph: {glyph_name}")?;
		let Statistics {
			area,
			mean_x,
			mean_y,
			variance_x,
			variance_y,
			stddev_x,
			stddev_y,
			covariance,
			correlation,
			slant,
		} = stats;
		writeln!(out, "area: {}", g(area))?;
		let Some(m) = moments else {
			// `StatisticsControlPen` has no moments.
			return Err(Error::ttlib(
				"AttributeError: 'StatisticsControlPen' object has no attribute 'momentX'",
			));
		};
		for (item, value) in [
			("momentX", m.moment_x),
			("momentY", m.moment_y),
			("momentXX", m.moment_xx),
			("momentYY", m.moment_yy),
			("momentXY", m.moment_xy),
			("meanX", mean_x),
			("meanY", mean_y),
			("varianceX", variance_x),
			("varianceY", variance_y),
			("stddevX", stddev_x),
			("stddevY", stddev_y),
			("covariance", covariance),
			("correlation", correlation),
			("slant", slant),
		] {
			writeln!(out, "{item}: {}", g(value))?;
		}
	}

	if !quiet {
		writeln!(out)?;
		writeln!(out, "font:")?;
	}

	let count = glyphs.len() as f64;
	writeln!(out, "weight: {}", g(divide(wght_sum * upem, wdth_sum)?))?;
	writeln!(
		out,
		"weight (perceptual): {}",
		g(divide(wght_sum_perceptual, wdth_sum)?)
	)?;
	writeln!(
		out,
		"width:  {}",
		g(divide(divide(wdth_sum, upem)?, count)?)
	)?;
	let slant = divide(slnt_sum, count)?;
	writeln!(out, "slant:  {}", g(slant))?;
	writeln!(out, "slant angle:  {}", g(angle(slant)))?;
	let slant_perceptual = divide(slnt_sum_perceptual, wdth_sum)?;
	writeln!(out, "slant (perceptual):  {}", g(slant_perceptual))?;
	writeln!(
		out,
		"slant (perceptual) angle:  {}",
		g(angle(slant_perceptual))
	)?;
	Ok(())
}
