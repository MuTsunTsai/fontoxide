//! The command line (`fontTools.voltLib.__main__`): builds the OpenType layout
//! tables of a MS VOLT project into a font.

use std::collections::HashSet;
use std::path::Path;

use fontoxide_fealib::builder::add_open_type_features_from_string;
use fontoxide_misc::cli_tools::make_output_file_name;
use fontoxide_ttlib::tables::Post;
use fontoxide_ttlib::{Font, ReadOptions, Tag, WriteOptions};

use crate::error::{Error, Result};
use crate::parser::Parser;
use crate::volt_to_fea::{TABLES, VoltToFea, log_not_implemented, read_input};

/// The tables that can be built (`SUPPORTED_TABLES`).
pub const SUPPORTED_TABLES: [&str; 4] = [TABLES[0], TABLES[1], TABLES[2], "cmap"];

/// `Lexer.CHAR_NAME_START_` of feaLib.
const FEA_NAME_START: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_+*:.^~!\\";
/// `Lexer.CHAR_NAME_CONTINUATION_` of feaLib.
const FEA_NAME_CONTINUATION: &str =
	"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_.+*:^~!/-";

/// Whether the glyph name is invalid in feature files (`invalid_fea_glyph_name`).
pub fn invalid_fea_glyph_name(name: &str) -> Result<bool> {
	let mut chars = name.chars();
	let Some(first) = chars.next() else {
		return Err(Error::Python(
			"IndexError: string index out of range".to_owned(),
		));
	};
	if !FEA_NAME_START.contains(first) {
		return Ok(true);
	}
	Ok(chars.any(|c| !FEA_NAME_CONTINUATION.contains(c)))
}

/// Makes the glyph name valid in feature files (`sanitize_glyph_name`).
pub fn sanitize_glyph_name(name: &str) -> String {
	let mut sanitized = String::new();
	for (i, c) in name.chars().enumerate() {
		if i == 0 && !FEA_NAME_START.contains(c) {
			sanitized.push('a');
			sanitized.push(c);
		} else if !FEA_NAME_CONTINUATION.contains(c) {
			sanitized.push('_');
		} else {
			sanitized.push(c);
		}
	}
	sanitized
}

const HELP: &str = "\
usage: fonttools voltLib [-h] [-f INPUT_FONT] [-o OUTPUT] [-t TABLE_TAG [TABLE_TAG ...]] [-F] [--ship] [-v] [-T] INPUT

Use fontTools to compile MS VOLT projects.

positional arguments:
  INPUT                 Path to the input font/VTP file to process

options:
  -h, --help            show this help message and exit
  -f, --font INPUT_FONT
                        Path to the input font (if INPUT is a VTP file)
  -o, --output OUTPUT   Path to the output font.
  -t, --tables TABLE_TAG [TABLE_TAG ...]
                        Specify the table(s) to be built.
  -F, --debug-feature-file
                        Write the generated feature file to disk.
  --ship                Remove source VOLT tables from output font.
  -v, --verbose         Increase the logger verbosity. Multiple -v options are allowed.
  -T, --traceback       show traceback for exceptions.
";

fn usage(message: String) -> Error {
	Error::Usage(format!("error: {message}"))
}

fn read_font(path: &str) -> Result<Font> {
	let data = std::fs::read(path).map_err(|e| Error::io(path, e))?;
	Ok(Font::read(data, ReadOptions::default())?)
}

/// `Path(p).with_suffix(".fea")`.
fn with_fea_suffix(path: &str) -> String {
	Path::new(path).with_extension("fea").display().to_string()
}

/// The command line `fonttools voltLib` (`main`): builds the tables of a VOLT
/// project (a `.vtp` file with `--font`, or a font with a `TSIV` table) into the
/// font and saves it. Returns the exit code: 1 if there is no project or font,
/// or the project cannot be converted or compiled (which is logged), unless
/// `--traceback` asks for the error to be returned.
///
/// The verbosity options are accepted; the caller configures logging.
// LENIENT(L42): the debug feature file is written with "\n" line breaks; Python
// writes in text mode, which is "\r\n" on Windows.
pub fn main(args: &[&str]) -> Result<i32> {
	let mut positional: Vec<&str> = Vec::new();
	let mut input_font: Option<&str> = None;
	let mut output: Option<&str> = None;
	let mut tables: Option<Vec<&str>> = None;
	let mut debug_feature_file = false;
	let mut ship = false;
	let mut traceback = false;
	let mut i = 0;
	while i < args.len() {
		let arg = args[i];
		i += 1;
		let (name, inline) = match arg.split_once('=') {
			Some((name, value)) if name.starts_with("--") => (name, Some(value)),
			_ => (arg, None),
		};
		let mut value = |option: &str| -> Result<&str> {
			match inline {
				Some(v) => Ok(v),
				None => {
					let v = args.get(i).copied().ok_or_else(|| {
						usage(format!("argument {option}: expected one argument"))
					})?;
					i += 1;
					Ok(v)
				}
			}
		};
		match name {
			"-h" | "--help" => {
				print!(
					"{}",
					fontoxide_misc::cli_tools::program_text(HELP, "fonttools voltLib", "voltLib")
				);
				return Ok(0);
			}
			"-f" | "--font" => input_font = Some(value("-f/--font")?),
			"-o" | "--output" => output = Some(value("-o/--output")?),
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
			"-F" | "--debug-feature-file" => debug_feature_file = true,
			"--ship" => ship = true,
			"-T" | "--traceback" => traceback = true,
			"-v" | "--verbose" | "-vv" | "-vvv" => {}
			_ if arg.len() > 1 && arg.starts_with('-') => {
				return Err(usage(format!("unrecognized arguments: {arg}")));
			}
			_ => positional.push(arg),
		}
	}
	let input = match positional.as_slice() {
		[input] => *input,
		[] => {
			return Err(usage(
				"the following arguments are required: INPUT".to_owned(),
			));
		}
		[_, rest @ ..] => {
			return Err(usage(format!("unrecognized arguments: {}", rest.join(" "))));
		}
	};

	let output_font = match output {
		Some(o) => o.to_owned(),
		None => make_output_file_name(input_font.unwrap_or(input), None, None, false, ""),
	};
	log::info!("Compiling MS VOLT to '{output_font}'");

	// If the input is a font file, extract the VOLT data from the "TSIV" table
	let Some((file_or_path, font)) = read_input(input, "\"TSIV\" table is missing")? else {
		return Ok(1);
	};

	// If input is not a font file, the font must be provided
	let mut font = match font {
		Some(font) => font,
		None => match input_font {
			Some(path) => read_font(path)?,
			None => {
				log::error!("Please provide an input font");
				return Ok(1);
			}
		},
	};

	// FEA syntax does not allow some glyph names that VOLT accepts, so if we
	// found such glyph name we will temporarily rename such glyphs.
	let glyph_order = font.glyph_order()?.names().to_vec();
	let mut temp_glyph_order: Option<Vec<String>> = None;
	let mut any_invalid = false;
	for n in &glyph_order {
		if invalid_fea_glyph_name(n)? {
			any_invalid = true;
			break;
		}
	}
	if any_invalid {
		let mut temp: Vec<String> = Vec::new();
		let mut existing: HashSet<String> = glyph_order.iter().cloned().collect();
		for n in &glyph_order {
			let mut n = n.clone();
			if invalid_fea_glyph_name(&n)? {
				n = sanitize_glyph_name(&n);
				while existing.contains(&n) {
					n = format!("a{n}");
				}
			}
			existing.insert(n.clone());
			temp.push(n);
		}
		font.set_glyph_order(temp.clone());
		temp_glyph_order = Some(temp);
	}

	let doc = Parser::new(file_or_path)?.parse()?;

	log::info!("Converting VTP data to FEA");
	let mut converter = VoltToFea::with_font(doc, &mut font)?;
	let fea = match converter.convert(tables.as_deref(), true) {
		Ok(fea) => fea,
		Err(Error::NotImplemented { text, location }) if !traceback => {
			log_not_implemented(&text, &location);
			return Ok(1);
		}
		Err(e) => return Err(e),
	};

	let mut fea_filename = input.to_owned();
	if debug_feature_file {
		fea_filename = with_fea_suffix(&output_font);
		log::info!("Writing FEA to '{fea_filename}'");
		std::fs::write(&fea_filename, &fea).map_err(|e| Error::io(fea_filename.clone(), e))?;
	}

	log::info!("Compiling FEA to OpenType tables");
	match add_open_type_features_from_string(
		&mut font,
		&fea,
		Some(&fea_filename),
		tables.as_deref(),
		false,
	) {
		Ok(()) => {}
		Err(e @ fontoxide_fealib::Error::FeatureLib { .. }) => {
			if traceback {
				return Err(e.into());
			}
			log::error!("{e}");
			return Ok(1);
		}
		Err(e) => return Err(e.into()),
	}

	if ship {
		for tag in [b"TSIV", b"TSIS", b"TSIP", b"TSID"] {
			font.remove(Tag::new(tag));
		}
	}

	// Restore original glyph names.
	if temp_glyph_order.is_some() {
		let data = font.write(&WriteOptions::default())?;
		font = Font::read(data, ReadOptions::default())?;
		font.set_glyph_order(glyph_order);
		font.get::<Post>()?
			.ok_or_else(|| Error::key("post"))?
			.extra_names = Some(Vec::new());
	}

	let out = font.write(&WriteOptions::default())?;
	std::fs::write(&output_font, out).map_err(|e| Error::io(output_font.clone(), e))?;
	Ok(0)
}
