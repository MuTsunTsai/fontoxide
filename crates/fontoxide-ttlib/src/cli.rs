//! The command line (`fontTools.ttLib.__main__`): opens fonts and collections,
//! and saves them as one font or a collection.

use crate::{Error, Flavor, Font, ReadOptions, Result, Tag, TtCollection, WriteOptions};

const HELP: &str = "\
usage: fonttools ttLib [-h] [-t table] [-o FILE] [-y NUMBER] [--lazy]
                       [--no-lazy] [--flavor FLAVOR] [--no-recalc-timestamp]
                       [-b] [--optimize-font-speed]
                       [font ...]

Open/save fonts with TTFont() or TTCollection()

positional arguments:
  font                  Font file.

options:
  -h, --help            show this help message and exit
  -t, --table table     Tables to decompile.
  -o, --output FILE     Output file.
  -y NUMBER             Font number to load from collections.
  --lazy                Load fonts lazily.
  --no-lazy             Load fonts immediately.
  --flavor FLAVOR       Flavor of output font. 'woff' or 'woff2'.
  --no-recalc-timestamp
                        Keep the original font 'modified' timestamp.
  -b                    Don't recalc glyph bounding boxes: use the values in
                        the original font.
  --optimize-font-speed
                        Enable optimizations that prioritize speed over file
                        size. This mainly affects how glyf table and gvar /
                        VARC tables are compiled.

If multiple files are given on the command-line, they are each opened (as a
font or collection), and added to the font list. The above, when combined with
-o / --output, allows for extracting a single font from a collection, or
combining multiple fonts into a collection.
";

/// The help text of the command line.
pub fn help() -> &'static str {
	HELP
}

/// `argparse` errors.
fn usage(message: String) -> Error {
	Error::value(format!("error: {message}"))
}

/// The command line `fonttools ttLib` (`main`): opens each file as a font, or as
/// a collection whose fonts are all added, decompiles the requested tables
/// (`-t`, or all of them with `--no-lazy`), and with `-o` saves the fonts: one
/// font as a font, several as a collection. Returns the exit code.
///
/// fontoxide always reads fonts completely, so `--lazy` only changes which
/// tables are decompiled, as in fontTools. `--optimize-font-speed` is not
/// supported: fontoxide has no `OPTIMIZE_FONT_SPEED` option.
pub fn main(args: &[&str]) -> Result<i32> {
	let mut fonts_args: Vec<&str> = Vec::new();
	let mut tables: Option<Vec<&str>> = None;
	let mut output: Option<&str> = None;
	let mut font_number: &str = "-1";
	let mut lazy: Option<bool> = None;
	let mut flavor: Option<&str> = None;
	let mut recalc_timestamp = true;
	let mut recalc_bboxes = true;
	let mut it = args.iter().copied();
	let mut only_positional = false;
	while let Some(arg) = it.next() {
		if only_positional || arg == "-" || !arg.starts_with('-') {
			fonts_args.push(arg);
			continue;
		}
		let (name, inline) = match arg.split_once('=') {
			Some((name, value)) if name.starts_with("--") => (name, Some(value)),
			// `-oFILE`, `-yNUMBER`, `-ttable`.
			_ if !arg.starts_with("--") && arg.len() > 2 => (&arg[..2], Some(&arg[2..])),
			_ => (arg, None),
		};
		let mut value = |option: &str| -> Result<&str> {
			match inline {
				Some(v) => Ok(v),
				None => it
					.next()
					.ok_or_else(|| usage(format!("argument {option}: expected one argument"))),
			}
		};
		match name {
			"--" => only_positional = true,
			"-h" | "--help" => {
				print!("{HELP}");
				return Ok(0);
			}
			"-t" | "--table" => tables
				.get_or_insert_with(Vec::new)
				.push(value("-t/--table")?),
			"-o" | "--output" => output = Some(value("-o/--output")?),
			"-y" => font_number = value("-y")?,
			"--flavor" => flavor = Some(value("--flavor")?),
			"--lazy" if inline.is_none() => lazy = Some(true),
			"--no-lazy" if inline.is_none() => lazy = Some(false),
			"--no-recalc-timestamp" if inline.is_none() => recalc_timestamp = false,
			"-b" if inline.is_none() => recalc_bboxes = false,
			"--optimize-font-speed" if inline.is_none() => {
				return Err(Error::ttlib(
					"--optimize-font-speed is not supported: fontoxide has no OPTIMIZE_FONT_SPEED option",
				));
			}
			_ => return Err(usage(format!("unrecognized arguments: {arg}"))),
		}
	}
	let font_number = fontoxide_misc::py::parse_int(font_number).ok_or_else(|| {
		Error::value(format!(
			"invalid literal for int() with base 10: {}",
			fontoxide_misc::py::str_repr(font_number)
		))
	})?;
	let flavor = match flavor {
		None => None,
		Some("woff") => Some(Flavor::Woff),
		Some("woff2") => Some(Flavor::Woff2),
		Some(other) => return Err(Error::ttlib(format!("Unknown flavor '{other}'"))),
	};

	let mut fonts = Vec::new();
	for path in fonts_args {
		let data: std::sync::Arc<[u8]> = std::fs::read(path)?.into();
		let options = ReadOptions {
			font_number: usize::try_from(font_number).ok(),
			recalc_bboxes,
			recalc_timestamp,
			..Default::default()
		};
		match Font::read(data.clone(), options) {
			Ok(font) => fonts.push(font),
			Err(Error::FileIsCollection(_)) => {
				fonts.extend(TtCollection::read(data, ReadOptions::default())?.fonts);
			}
			Err(e) => return Err(e),
		}
	}

	let tables = match tables {
		Some(tables) => tables,
		None if lazy == Some(false) => vec!["*"],
		None => Vec::new(),
	};
	for font in &mut fonts {
		if tables.contains(&"GlyphOrder") {
			font.glyph_order()?;
		}
		let tags: Vec<Tag> = if tables.contains(&"*") {
			font.tags()
		} else {
			tables
				.iter()
				.filter(|t| **t != "GlyphOrder")
				.map(|t| {
					Tag::from_str_exact(t).ok_or_else(|| {
						Error::ttlib(format!("KeyError: {}", fontoxide_misc::py::str_repr(t)))
					})
				})
				.collect::<Result<_>>()?
		};
		for tag in tags {
			// `font[table]`.
			if !font.load(tag)? {
				return Err(Error::TableNotFound(tag));
			}
		}
	}

	if let Some(output) = output {
		let data = if let [font] = fonts.as_mut_slice() {
			font.flavor = flavor;
			font.write(&WriteOptions::default())?
		} else {
			if flavor.is_some() {
				return Err(Error::ttlib("Cannot set flavor for collections."));
			}
			let mut collection = TtCollection {
				fonts,
				..Default::default()
			};
			collection.write(true)?
		};
		std::fs::write(output, data)?;
	}
	Ok(0)
}
