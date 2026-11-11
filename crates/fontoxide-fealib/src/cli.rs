//! The command line (`fontTools.feaLib.__main__`): adds features from a feature
//! file into a font.

use std::path::PathBuf;

use fontoxide_misc::cli_tools::make_output_file_name;
use fontoxide_ttlib::{Font, ReadOptions, WriteOptions};

use crate::builder::{SUPPORTED_TABLES, add_open_type_features};
use crate::error::{Error, Result};

const HELP: &str = "\
usage: fonttools feaLib [-h] [-o OUTPUT_FONT] [-t TABLE_TAG [TABLE_TAG ...]] [-d] [-v] [--traceback] FEATURES INPUT_FONT

Use fontTools to compile OpenType feature files (*.fea).

positional arguments:
  FEATURES              Path to the feature file
  INPUT_FONT            Path to the input font

options:
  -h, --help            show this help message and exit
  -o OUTPUT_FONT, --output OUTPUT_FONT
                        Path to the output font.
  -t TABLE_TAG [TABLE_TAG ...], --tables TABLE_TAG [TABLE_TAG ...]
                        Specify the table(s) to be built.
  -d, --debug           Add source-level debugging information to font.
  -v, --verbose         Increase the logger verbosity. Multiple -v options are allowed.
  --traceback           show traceback for exceptions.
";

/// `argparse` errors.
fn usage(message: String) -> Error {
	Error::Value(format!("error: {message}"))
}

/// The command line `fonttools feaLib` (`main`): adds the features of a
/// feature file into a font and saves it. Returns the exit code: 1 if the
/// feature file has an error (which is logged), unless `--traceback` asks for
/// the error to be returned.
///
/// The verbosity options are accepted; the caller configures logging.
pub fn main(args: &[&str]) -> Result<i32> {
	let mut positional: Vec<&str> = Vec::new();
	let mut output_font: Option<&str> = None;
	let mut tables: Option<Vec<&str>> = None;
	let mut debug = false;
	let mut traceback = false;
	let mut i = 0;
	while i < args.len() {
		let arg = args[i];
		i += 1;
		let (name, inline) = match arg.split_once('=') {
			Some((name, value)) if name.starts_with("--") => (name, Some(value)),
			_ => (arg, None),
		};
		match name {
			"-h" | "--help" => {
				print!("{HELP}");
				return Ok(0);
			}
			"-o" | "--output" => {
				let value = match inline {
					Some(v) => v,
					None => {
						let v = args.get(i).copied().ok_or_else(|| {
							usage("argument -o/--output: expected one argument".to_owned())
						})?;
						i += 1;
						v
					}
				};
				output_font = Some(value);
			}
			"-t" | "--tables" => {
				let mut values: Vec<&str> = inline.into_iter().collect();
				if inline.is_none() {
					// nargs="+": the arguments up to the next option.
					while let Some(v) = args.get(i).filter(|v| !(v.starts_with('-') && v.len() > 1))
					{
						values.push(v);
						i += 1;
					}
				}
				if values.is_empty() {
					return Err(usage(
						"argument -t/--tables: expected at least one argument".to_owned(),
					));
				}
				for v in &values {
					if !SUPPORTED_TABLES.contains(v) {
						let choices: Vec<String> = SUPPORTED_TABLES
							.iter()
							.map(|t| fontoxide_misc::py::str_repr(t))
							.collect();
						return Err(usage(format!(
							"argument -t/--tables: invalid choice: {} (choose from {})",
							fontoxide_misc::py::str_repr(v),
							choices.join(", ")
						)));
					}
				}
				tables = Some(values);
			}
			"-d" | "--debug" => debug = true,
			"--traceback" => traceback = true,
			"-v" | "--verbose" | "-vv" | "-vvv" => {}
			_ if arg.len() > 1 && arg.starts_with('-') => {
				return Err(usage(format!("unrecognized arguments: {arg}")));
			}
			_ => positional.push(arg),
		}
	}
	let (input_fea, input_font) = match positional.as_slice() {
		[fea, font] => (*fea, *font),
		[] => {
			return Err(usage(
				"the following arguments are required: FEATURES, INPUT_FONT".to_owned(),
			));
		}
		[_] => {
			return Err(usage(
				"the following arguments are required: INPUT_FONT".to_owned(),
			));
		}
		[_, _, rest @ ..] => {
			return Err(usage(format!("unrecognized arguments: {}", rest.join(" "))));
		}
	};
	let output_font = match output_font {
		Some(o) => o.to_owned(),
		None => make_output_file_name(input_font, None, None, false, ""),
	};
	log::info!("Compiling features to '{output_font}'");
	let data = std::fs::read(input_font).map_err(fontoxide_ttlib::Error::from)?;
	let mut font = Font::read(data, ReadOptions::default())?;
	match add_open_type_features(
		&mut font,
		PathBuf::from(input_fea),
		tables.as_deref(),
		debug,
	) {
		Ok(()) => {}
		Err(e @ (Error::FeatureLib { .. } | Error::IncludedFeaNotFound { .. })) => {
			if traceback {
				return Err(e);
			}
			log::error!("{e}");
			return Ok(1);
		}
		Err(e) => return Err(e),
	}
	let out = font.write(&WriteOptions::default())?;
	std::fs::write(&output_font, out).map_err(fontoxide_ttlib::Error::from)?;
	Ok(0)
}
