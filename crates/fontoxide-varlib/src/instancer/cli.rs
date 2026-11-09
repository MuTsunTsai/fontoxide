//! The command line of the instancer (`parseLimits`, `parseArgs` and `main` of
//! `fontTools.varLib.instancer`).

use std::path::Path;

use fontoxide_misc::cli_tools::make_output_file_name;
use fontoxide_misc::py::{self, str_repr};
use fontoxide_ttlib::tables::Fvar;
use fontoxide_ttlib::{Error, Font, ReadOptions, Result, WriteOptions};

use super::limits::{AxisLimits, AxisTriple};
use super::{InstancerOptions, OverlapMode, instantiate_variable_font};

fn usage_error(message: &str) -> Error {
	Error::value(format!("error: {message}"))
}

/// `parseLimits`: limits like `wdth=100`, `wght=75.0:125.0`, `wght=100:400:700`,
/// `wght=:500:` or `wght=drop`.
pub fn parse_limits(limits: &[&str]) -> Result<AxisLimits> {
	let mut result = AxisLimits::new();
	for limit in limits {
		let invalid = || Error::value(format!("invalid location format: {}", str_repr(limit)));
		let (tag, value) = limit.split_once('=').ok_or_else(invalid)?;
		let is_word = |c: char| c.is_alphanumeric() || c == '_';
		let count = tag.chars().count();
		if !(1..=4).contains(&count) || !tag.chars().all(is_word) {
			return Err(invalid());
		}
		let mut tag = tag.to_owned();
		for _ in count..4 {
			tag.push(' ');
		}
		if value == "drop" {
			result.insert(tag, None);
			continue;
		}
		if value.contains('\n') {
			return Err(invalid());
		}
		let parts: Vec<&str> = value.split(':').collect();
		let (minimum, default, maximum) = match parts.as_slice() {
			// "value" syntax
			[v] => (*v, *v, *v),
			// "min:max" syntax
			[min, max] => (*min, "", *max),
			[min, default, max] => (*min, *default, *max),
			_ => return Err(invalid()),
		};
		let number = |s: &str| -> Result<Option<f64>> {
			if s.is_empty() {
				return Ok(None);
			}
			py::parse_float(s).map(Some).ok_or_else(|| {
				Error::value(format!(
					"could not convert string to float: {}",
					str_repr(s)
				))
			})
		};
		let triple = AxisTriple::new(number(minimum)?, number(default)?, number(maximum)?)?;
		result.insert(tag, Some(triple));
	}
	Ok(result)
}

/// The options of the command line.
#[derive(Debug, Default)]
struct Args<'a> {
	input: Option<&'a str>,
	locations: Vec<&'a str>,
	output: Option<&'a str>,
	make_static: bool,
	no_optimize: bool,
	no_overlap_flag: bool,
	remove_overlaps: bool,
	ignore_overlap_errors: bool,
	update_name_table: bool,
	downgrade_cff2: bool,
	no_recalc_timestamp: bool,
	no_recalc_bounds: bool,
}

/// The value of an option, either after `=` or as the next argument.
fn option_value<'a>(
	name: &str,
	inline: Option<&'a str>,
	rest: &mut std::slice::Iter<'_, &'a str>,
) -> Result<&'a str> {
	inline
		.or_else(|| rest.next().copied())
		.ok_or_else(|| usage_error(&format!("argument {name}: expected one argument")))
}

/// `parseArgs`.
fn parse_args<'a>(args: &[&'a str]) -> Result<Args<'a>> {
	let mut parsed = Args::default();
	let mut it = args.iter();
	let mut only_positionals = false;
	while let Some(&arg) = it.next() {
		if only_positionals || !arg.starts_with('-') || arg == "-" {
			if parsed.input.is_none() {
				parsed.input = Some(arg);
			} else {
				parsed.locations.push(arg);
			}
			continue;
		}
		let (name, inline) = match arg.split_once('=') {
			Some((name, value)) if name.starts_with("--") => (name, Some(value)),
			_ => (arg, None),
		};
		match name {
			"--" => only_positionals = true,
			"-o" | "--output" => {
				parsed.output = Some(option_value("-o/--output", inline, &mut it)?)
			}
			"--static" => parsed.make_static = true,
			"--no-optimize" => parsed.no_optimize = true,
			"--no-overlap-flag" => parsed.no_overlap_flag = true,
			"--remove-overlaps" => parsed.remove_overlaps = true,
			"--ignore-overlap-errors" => parsed.ignore_overlap_errors = true,
			"--update-name-table" => parsed.update_name_table = true,
			"--downgrade-cff2" => parsed.downgrade_cff2 = true,
			"--no-recalc-timestamp" => parsed.no_recalc_timestamp = true,
			"--no-recalc-bounds" => parsed.no_recalc_bounds = true,
			"-v" | "--verbose" | "-q" | "--quiet" => {}
			_ => return Err(usage_error(&format!("unrecognized arguments: {arg}"))),
		}
	}
	Ok(parsed)
}

/// `main`: partially instantiates a variable font; returns the exit code.
pub fn main(args: &[&str]) -> Result<i32> {
	let args = parse_args(args)?;
	let input = args
		.input
		.ok_or_else(|| usage_error("the following arguments are required: INPUT.ttf"))?;
	if !Path::new(input).is_file() {
		return Err(usage_error(&format!("No such file '{input}'")));
	}
	let limits = parse_limits(&args.locations).map_err(|e| usage_error(&e.to_string()))?;
	if limits.len() != args.locations.len() {
		return Err(usage_error("Specified multiple limits for the same axis"));
	}

	log::info!("Restricting axes: {limits:?}");
	log::info!("Loading variable font");
	let mut font = Font::read(
		std::fs::read(input)?,
		ReadOptions {
			recalc_timestamp: !args.no_recalc_timestamp,
			recalc_bboxes: !args.no_recalc_bounds,
			..Default::default()
		},
	)?;
	super::sanity_check_variable_tables(&font)?;

	let is_full_instance = args.make_static
		|| font.require::<Fvar>()?.axes.iter().all(|axis| {
			limits.get(&axis.tag_text()).is_some_and(|limit| {
				limit
					.as_ref()
					.is_none_or(|limit| limit.minimum == limit.maximum)
			})
		});

	let overlap = if args.remove_overlaps {
		if args.ignore_overlap_errors {
			OverlapMode::RemoveAndIgnoreErrors
		} else {
			OverlapMode::Remove
		}
	} else if args.no_overlap_flag {
		OverlapMode::KeepAndDontSetFlags
	} else {
		OverlapMode::KeepAndSetFlags
	};
	instantiate_variable_font(
		&mut font,
		&limits,
		&InstancerOptions {
			optimize: !args.no_optimize,
			overlap,
			make_static: args.make_static,
			update_font_names: args.update_name_table,
			downgrade_cff2: args.downgrade_cff2,
		},
	)?;

	let suffix = if is_full_instance {
		"-instance"
	} else {
		"-partial"
	};
	let output = match args.output {
		Some(output) => output.to_owned(),
		None => make_output_file_name(input, None, None, true, suffix),
	};
	log::info!(
		"Saving {} font {output}",
		if is_full_instance {
			"instance"
		} else {
			"partial variable"
		}
	);
	std::fs::write(output, font.write(&WriteOptions::default())?)?;
	Ok(0)
}
