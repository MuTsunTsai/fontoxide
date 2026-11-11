//! The command line of `fontTools.ttLib.woff2` (`main`, `compress` and
//! `decompress`): compresses fonts to WOFF 2.0 and decompresses them.

use std::io::Write;

use fontoxide_misc::cli_tools::make_output_file_name;

use crate::{Error, Flavor, FlavorData, Font, ReadOptions, Result, Tag, WriteOptions};

/// The help of the parser (`parser.print_help()`).
const HELP: &str = "\
usage: fonttools ttLib.woff2 [-h] {compress,decompress} ...

Compress and decompress WOFF2 fonts

options:
  -h, --help            show this help message and exit

sub-commands:
  {compress,decompress}
";

/// The help of the `compress` sub-command.
const COMPRESS_HELP: &str = "\
usage: fonttools ttLib.woff2 compress [-h] [-v | -q] [-o OUTPUT]
                                      [--no-glyf-transform] [--hmtx-transform]
                                      INPUT

Compress a TTF or OTF font to WOFF2

positional arguments:
  INPUT                 the input OpenType font (.ttf or .otf)

options:
  -h, --help            show this help message and exit
  -v, --verbose         print more messages to console
  -q, --quiet           do not print messages to console
  -o, --output-file OUTPUT
                        the output WOFF2 font

  --no-glyf-transform   Do not transform glyf (and loca) tables
  --hmtx-transform      Enable optional transformation for 'hmtx' table
";

/// The help of the `decompress` sub-command.
const DECOMPRESS_HELP: &str = "\
usage: fonttools ttLib.woff2 decompress [-h] [-v | -q] [-o OUTPUT] INPUT

Decompress a WOFF2 font to OTF

positional arguments:
  INPUT                 the input WOFF2 font

options:
  -h, --help            show this help message and exit
  -v, --verbose         print more messages to console
  -q, --quiet           do not print messages to console
  -o, --output-file OUTPUT
                        the output OpenType font
";

const GLYF: Tag = Tag::new(b"glyf");
const LOCA: Tag = Tag::new(b"loca");
const HMTX: Tag = Tag::new(b"hmtx");

/// Compresses an OpenType font (CFF- or TrueType-flavored) to WOFF 2.0
/// (`compress`), transforming `transform_tables` (by default, `glyf` and
/// `loca`; an empty list disables all transformations). The metadata and
/// private data of a WOFF or WOFF 2.0 input are kept.
pub fn compress(
	input_file: &str,
	output_file: &str,
	transform_tables: Option<&[Tag]>,
) -> Result<()> {
	log::info!("Processing {input_file} => {output_file}");
	let mut font = Font::read(std::fs::read(input_file)?, no_recalc())?;
	font.flavor = Some(Flavor::Woff2);
	if let Some(tables) = transform_tables {
		// `WOFF2FlavorData(data=font.flavorData, transformedTables=...)`.
		let mut data = FlavorData::woff2(tables)?;
		if let Some(old) = font.flavor_data.take() {
			data.major_version = old.major_version;
			data.minor_version = old.minor_version;
			data.meta_data = old.meta_data;
			data.priv_data = old.priv_data;
		}
		font.flavor_data = Some(data);
	}
	let data = font.write(&WriteOptions {
		reorder_tables: Some(false),
		..Default::default()
	})?;
	std::fs::write(output_file, data)?;
	Ok(())
}

/// Decompresses a WOFF 2.0 font to an OpenType font (`decompress`).
pub fn decompress(input_file: &str, output_file: &str) -> Result<()> {
	log::info!("Processing {input_file} => {output_file}");
	let mut font = Font::read(std::fs::read(input_file)?, no_recalc())?;
	font.flavor = None;
	font.flavor_data = None;
	let data = font.write(&WriteOptions {
		reorder_tables: Some(true),
		..Default::default()
	})?;
	std::fs::write(output_file, data)?;
	Ok(())
}

/// `TTFont(input_file, recalcBBoxes=False, recalcTimestamp=False)`.
fn no_recalc() -> ReadOptions {
	ReadOptions {
		recalc_bboxes: false,
		recalc_timestamp: false,
		..Default::default()
	}
}

/// An `argparse` error.
fn usage(message: String) -> Error {
	Error::value(format!("error: {message}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Subcommand {
	Compress,
	Decompress,
}

/// The command line `fonttools ttLib.woff2` (`main`), printing to standard
/// output. Returns the exit code.
pub fn main(args: &[&str]) -> Result<i32> {
	let stdout = std::io::stdout();
	run(args, &mut stdout.lock())
}

/// [`main`], printing the help to `out`.
pub fn run(args: &[&str], out: &mut dyn Write) -> Result<i32> {
	let mut it = args.iter().copied();
	// The main parser: `-h` and the sub-command.
	let Some(arg) = it.next() else {
		out.write_all(HELP.as_bytes())?;
		return Ok(0);
	};
	let subcommand = {
		match arg {
			"compress" => Subcommand::Compress,
			"decompress" => Subcommand::Decompress,
			_ if is_long(arg, "--help") || arg == "-h" => {
				// `_HelpAction`: the help of every sub-command.
				writeln!(out, "{COMPRESS_HELP}")?;
				writeln!(out, "{DECOMPRESS_HELP}")?;
				return Ok(0);
			}
			_ if arg.starts_with('-') && arg != "-" => {
				return Err(usage(format!("unrecognized arguments: {arg}")));
			}
			_ => {
				return Err(usage(format!(
					"argument {{compress,decompress}}: invalid choice: {} (choose from compress, decompress)",
					fontoxide_misc::py::str_repr(arg)
				)));
			}
		}
	};
	let compress = subcommand == Subcommand::Compress;
	let mut verbose = false;
	let mut quiet = false;
	let mut output_file: Option<String> = None;
	let mut input_file: Option<&str> = None;
	let mut transform_tables = vec![GLYF, LOCA];
	let mut only_positional = false;
	while let Some(arg) = it.next() {
		if only_positional || arg == "-" || !arg.starts_with('-') {
			if input_file.is_some() {
				return Err(usage(format!("unrecognized arguments: {arg}")));
			}
			input_file = Some(arg);
			continue;
		}
		let (name, inline) = match arg.split_once('=') {
			Some((name, value)) if name.starts_with("--") => (name, Some(value)),
			// `-oFILE`.
			_ if !arg.starts_with("--") && arg.len() > 2 && arg.starts_with("-o") => {
				("-o", Some(&arg[2..]))
			}
			_ => (arg, None),
		};
		if name == "--" {
			only_positional = true;
		} else if name == "-h" || is_long(name, "--help") {
			let help = if compress {
				COMPRESS_HELP
			} else {
				DECOMPRESS_HELP
			};
			out.write_all(help.as_bytes())?;
			return Ok(0);
		} else if name == "-v" || is_long(name, "--verbose") {
			if quiet {
				return Err(usage(
					"argument -v/--verbose: not allowed with argument -q/--quiet".into(),
				));
			}
			verbose = true;
		} else if name == "-q" || is_long(name, "--quiet") {
			if verbose {
				return Err(usage(
					"argument -q/--quiet: not allowed with argument -v/--verbose".into(),
				));
			}
			quiet = true;
		} else if name == "-o" || is_long(name, "--output-file") {
			let value = match inline {
				Some(v) => v,
				None => it.next().ok_or_else(|| {
					usage("argument -o/--output-file: expected one argument".into())
				})?,
			};
			output_file = Some(value.to_owned());
		} else if compress && inline.is_none() && is_long(name, "--no-glyf-transform") {
			transform_tables.retain(|t| *t != GLYF && *t != LOCA);
		} else if compress && inline.is_none() && is_long(name, "--hmtx-transform") {
			if !transform_tables.contains(&HMTX) {
				transform_tables.push(HMTX);
			}
		} else {
			return Err(usage(format!("unrecognized arguments: {arg}")));
		}
	}
	let Some(input_file) = input_file else {
		return Err(usage("the following arguments are required: INPUT".into()));
	};

	log::set_max_level(if quiet {
		log::LevelFilter::Error
	} else if verbose {
		log::LevelFilter::Debug
	} else {
		log::LevelFilter::Info
	});

	let output_file = match output_file.filter(|o| !o.is_empty()) {
		Some(o) => o,
		None => {
			let extension = if compress {
				".woff2"
			} else {
				// Choose the .ttf/.otf file extension depending on sfntVersion.
				let data = std::fs::read(input_file)?;
				let sfnt_version = data
					.get(4..8)
					.ok_or_else(|| Error::Assertion("not enough data".into()))?;
				if sfnt_version == b"OTTO" {
					".otf"
				} else {
					".ttf"
				}
			};
			make_output_file_name(input_file, None, Some(extension), false, "")
		}
	};

	if compress {
		self::compress(input_file, &output_file, Some(&transform_tables))?;
	} else {
		decompress(input_file, &output_file)?;
	}
	Ok(0)
}

/// Whether `arg` is `option` or a prefix of it that `argparse` accepts (at
/// least `--` and one more character; the prefixes here are unique).
fn is_long(arg: &str, option: &str) -> bool {
	arg.len() > 2 && arg.starts_with("--") && option.starts_with(arg)
}
