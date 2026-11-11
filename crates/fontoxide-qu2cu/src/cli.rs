//! The `qu2cu` command line (`fontTools.qu2cu.cli`): converts TrueType
//! fonts from quadratic to cubic curves.

use std::collections::BTreeMap;
use std::path::Path;

use fontoxide_misc::Tag;
use fontoxide_misc::cli_tools::make_output_file_name;
use fontoxide_ttlib::beyond64k::{ConvertOptions, upper_tables};
use fontoxide_ttlib::pens::{GlyphOptions, TtGlyphPen};
use fontoxide_ttlib::tables::{Glyf, Head};
use fontoxide_ttlib::{Font, GlyphSet, ReadOptions, WriteOptions};

use crate::argparse::{self, Kind, Spec, Ty};
use crate::pens::Qu2CuPen;

/// The version of fontTools this is a port of (`fontTools.__version__`).
const VERSION: &str = fontoxide_ttlib::FONTTOOLS_VERSION;

/// The usage message.
pub const USAGE: &str = "\
usage: qu2cu [-h] [--version] [-v] [-e ERROR] [-c] [-o OUTPUT | -d DIRECTORY]
             INPUT [INPUT ...]";

/// The help message (`-h`).
pub const HELP: &str = "\
usage: qu2cu [-h] [--version] [-v] [-e ERROR] [-c] [-o OUTPUT | -d DIRECTORY]
             INPUT [INPUT ...]

positional arguments:
  INPUT                 one or more input TTF source file(s).

options:
  -h, --help            show this help message and exit
  --version             show program's version number and exit
  -v, --verbose
  -e, --conversion-error ERROR
                        maxiumum approximation error measured in EM (default:
                        0.001)
  -c, --all-cubic       whether to only use cubic curves
  -o, --output-file OUTPUT
                        output filename for the converted TTF.
  -d, --output-dir DIRECTORY
                        output directory where to save converted TTFs
";

const SPECS: &[Spec] = &[
	Spec {
		short: Some("-h"),
		long: "--help",
		kind: Kind::Flag,
		group: None,
		ty: Ty::Str,
	},
	Spec {
		short: None,
		long: "--version",
		kind: Kind::Flag,
		group: None,
		ty: Ty::Str,
	},
	Spec {
		short: Some("-v"),
		long: "--verbose",
		kind: Kind::Count,
		group: None,
		ty: Ty::Str,
	},
	Spec {
		short: Some("-e"),
		long: "--conversion-error",
		kind: Kind::Value,
		group: None,
		ty: Ty::Float,
	},
	Spec {
		short: Some("-c"),
		long: "--all-cubic",
		kind: Kind::Flag,
		group: None,
		ty: Ty::Str,
	},
	Spec {
		short: Some("-o"),
		long: "--output-file",
		kind: Kind::Value,
		group: Some(0),
		ty: Ty::Str,
	},
	Spec {
		short: Some("-d"),
		long: "--output-dir",
		kind: Kind::Value,
		group: Some(0),
		ty: Ty::Str,
	},
];

/// An error of the command line.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// `parser.error`: the usage and the message, for exit status 2.
	#[error("{0}")]
	Usage(String),
	/// `ValueError`: a variable font.
	#[error("{0}")]
	Value(String),
	/// An error reading, converting or writing a font.
	#[error(transparent)]
	TtLib(#[from] fontoxide_ttlib::Error),
	/// An operating system error.
	#[error(transparent)]
	Io(#[from] std::io::Error),
}

fn usage(message: impl std::fmt::Display) -> Error {
	Error::Usage(format!("{USAGE}\nqu2cu: error: {message}"))
}

/// The options of [`font_to_cubic`] (the keyword arguments of
/// `_font_to_cubic`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
	/// `dump_stats`: logs the number of curves made.
	pub dump_stats: bool,
	/// `max_err_em`: the tolerance relative to the em.
	pub max_err_em: f64,
	/// `all_cubic`: only cubic curves.
	pub all_cubic: bool,
}

impl Default for Options {
	fn default() -> Self {
		Options {
			dump_stats: false,
			max_err_em: 0.001,
			all_cubic: false,
		}
	}
}

/// `repr` of the statistics dict.
fn stats_repr(stats: &BTreeMap<String, usize>) -> String {
	let items: Vec<String> = stats
		.iter()
		.map(|(k, v)| format!("{}: {v}", fontoxide_misc::py::str_repr(k)))
		.collect();
	format!("{{{}}}", items.join(", "))
}

/// `_font_to_cubic` on a font in memory: converts the quadratic curves of
/// its `glyf` table to cubic ones in a `GLYF` table (the beyond-64k companion
/// of `glyf`, which can hold cubic curves).
pub fn font_to_cubic(font: &mut Font, options: &Options) -> Result<(), Error> {
	let mut stats = options.dump_stats.then(BTreeMap::new);
	let units_per_em = f64::from(font.require::<Head>()?.units_per_em);
	let max_err = options.max_err_em * units_per_em;

	if font.contains(Tag::new(b"gvar")) || font.contains(Tag::new(b"GVAR")) {
		return Err(Error::Value("Cannot convert variable font".into()));
	}
	upper_tables(font, &ConvertOptions::default())?;
	if !font.contains(Tag::new(b"GLYF")) {
		// `font["GLYF"]`, before any glyph is drawn.
		return Err(fontoxide_ttlib::Error::TableNotFound(Tag::new(b"GLYF")).into());
	}
	// LENIENT(L44): fontTools replaces each glyph as soon as it is converted, so a
	// component that is decomposed (its transformation does not fit F2Dot14)
	// is drawn converted if its base glyph comes earlier in the glyph order;
	// here all glyphs are drawn from the original outlines.
	let mut new_glyphs = Vec::new();
	{
		let glyph_set = GlyphSet::new(font)?;
		let order = glyph_set.glyph_order().clone();
		for gid in 0..order.len() as u32 {
			let glyph_name = order.name(gid);
			let mut ttpen = TtGlyphPen::new(Some(&glyph_set));
			{
				let mut pen = Qu2CuPen::new(&mut ttpen, max_err, options.all_cubic, false);
				pen.stats = stats.take();
				glyph_set.draw(&glyph_name, &mut pen)?;
				stats = pen.stats.take();
			}
			let glyph = ttpen.glyph_with(
				&order,
				&GlyphOptions {
					drop_implied_on_curves: true,
					..GlyphOptions::default()
				},
			)?;
			new_glyphs.push((gid, glyph));
		}
	}
	let glyf = font.require::<Glyf>()?;
	for (gid, glyph) in new_glyphs {
		glyf.glyphs.insert(gid, glyph);
	}

	if let Some(stats) = &stats {
		log::info!("Stats: {}", stats_repr(stats));
	}
	Ok(())
}

/// `_font_to_cubic`: converts a font file and saves it to `output_path`.
fn convert_file(input_path: &str, output_path: &str, options: &Options) -> Result<(), Error> {
	let mut font = Font::read(std::fs::read(input_path)?, ReadOptions::default())?;
	log::info!("Converting curves for {input_path}");
	font_to_cubic(&mut font, options)?;
	log::info!("Saving {output_path}");
	let data = font.write(&WriteOptions::default())?;
	std::fs::write(output_path, data)?;
	Ok(())
}

/// `os.path.basename`.
fn basename(path: &str) -> &str {
	let cut = if cfg!(windows) {
		path.rfind(['/', '\\'])
	} else {
		path.rfind('/')
	};
	cut.map_or(path, |i| &path[i + 1..])
}

/// `_main`: converts TrueType fonts from quadratic to cubic curves, with the
/// arguments of the `qu2cu` command line. Returns the exit status; a usage
/// error is [`Error::Usage`] (exit status 2 in fontTools).
///
/// The `-v` option is accepted, but logging is up to the caller's `log`
/// implementation.
pub fn main(args: &[&str]) -> Result<i32, Error> {
	let parsed = argparse::parse(args, SPECS, "INPUT").map_err(usage)?;
	if parsed.flag("--help") {
		print!("{HELP}");
		return Ok(0);
	}
	if parsed.flag("--version") {
		println!("{VERSION}");
		return Ok(0);
	}
	let conversion_error = match parsed.value("--conversion-error") {
		Some(seen) => argparse::parse_float(
			"-e/--conversion-error",
			seen.value.as_deref().unwrap_or_default(),
		)
		.map_err(usage)?,
		None => 0.001,
	};
	// `<= 0` (false for NaN).
	if conversion_error <= 0.0 {
		return Err(usage("--conversion-error must be greater than zero"));
	}
	let infiles = &parsed.positional;
	let output_file = parsed
		.value("--output-file")
		.and_then(|s| s.value.clone())
		.filter(|f| !f.is_empty());
	let output_dir = parsed
		.value("--output-dir")
		.and_then(|s| s.value.clone())
		.filter(|d| !d.is_empty());

	if infiles.len() > 1 && output_file.is_some() {
		return Err(usage("-o/--output-file can't be used with multile inputs"));
	}

	let output_paths: Vec<String> = if let Some(output_dir) = output_dir {
		let dir = Path::new(&output_dir);
		if !dir.exists() {
			std::fs::create_dir(dir)?;
		} else if !dir.is_dir() {
			return Err(usage(format!("'{output_dir}' is not a directory")));
		}
		infiles
			.iter()
			.map(|p| dir.join(basename(p)).to_string_lossy().into_owned())
			.collect()
	} else if let Some(output_file) = output_file {
		vec![output_file]
	} else {
		infiles
			.iter()
			.map(|p| make_output_file_name(p, None, None, true, ".cubic"))
			.collect()
	};

	let options = Options {
		dump_stats: parsed.count("--verbose") > 0,
		max_err_em: conversion_error,
		all_cubic: parsed.flag("--all-cubic"),
	};

	for (input_path, output_path) in infiles.iter().zip(&output_paths) {
		convert_file(input_path, output_path, &options)?;
	}
	Ok(0)
}
