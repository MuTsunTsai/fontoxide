//! Optimizing the layout tables of an existing font (`fontTools.otlLib.optimize`).

pub mod gpos;

use fontoxide_ttlib::{Font, ReadOptions, WriteOptions};

use crate::{Error, Result};

const HELP: &str = "\
usage: otlLib.optimize [-h] [-o OUTPUTFILE] [--gpos-compression-level {0,1,2,3,4,5,6,7,8,9}] [-v | -q] font

Optimize the layout tables of an existing font

positional arguments:
  font

options:
  -h, --help            show this help message and exit
  -o OUTPUTFILE         output file
  --gpos-compression-level {0,1,2,3,4,5,6,7,8,9}
                        GPOS Lookup type 2 (PairPos) compression level:
                            0 = do not attempt to compact PairPos lookups;
                            1 to 8 = create at most 1 to 8 new subtables for each existing
                                subtable, provided that it would yield a 50% file size saving;
                            9 = create as many new subtables as needed to yield a file size saving.
                        Default: 0.
  -v, --verbose         Run more verbosely.
  -q, --quiet           Turn verbosity off.
";

/// The command line of `fonttools otlLib.optimize` (`main`): compacts the GPOS
/// PairPos lookups of a font and saves it. Returns the exit code.
pub fn main(args: &[&str]) -> Result<i32> {
	let usage = |message: String| Error::Value(format!("error: {message}"));
	let mut positional = Vec::new();
	let mut output = None;
	let mut level = 0u8;
	let mut it = args.iter();
	while let Some(&arg) = it.next() {
		let (name, inline) = match arg.split_once('=') {
			Some((name, value)) if name.starts_with("--") => (name, Some(value)),
			_ => (arg, None),
		};
		match name {
			"-h" | "--help" => {
				print!("{HELP}");
				return Ok(0);
			}
			"-o" => {
				output = Some(
					*it.next()
						.ok_or_else(|| usage("argument -o: expected one argument".into()))?,
				);
			}
			"--gpos-compression-level" => {
				let value = inline.or_else(|| it.next().copied()).ok_or_else(|| {
					usage("argument --gpos-compression-level: expected one argument".into())
				})?;
				level = value
					.parse::<u8>()
					.ok()
					.filter(|l| *l <= 9)
					.ok_or_else(|| {
						usage(format!(
							"argument --gpos-compression-level: invalid choice: '{value}' (choose from 0, 1, 2, 3, 4, 5, 6, 7, 8, 9)"
						))
					})?;
			}
			"-v" | "--verbose" | "-q" | "--quiet" => {}
			_ if arg.len() > 1 && arg.starts_with('-') => {
				return Err(usage(format!("unrecognized arguments: {arg}")));
			}
			_ => positional.push(arg),
		}
	}
	let [input] = positional.as_slice() else {
		return Err(usage(if positional.is_empty() {
			"the following arguments are required: font".into()
		} else {
			format!("unrecognized arguments: {}", positional[1..].join(" "))
		}));
	};
	let data = std::fs::read(input).map_err(fontoxide_ttlib::Error::from)?;
	let mut font = Font::read(data, ReadOptions::default())?;
	gpos::compact(&mut font, level)?;
	let out = font.write(&WriteOptions::default())?;
	std::fs::write(output.unwrap_or(input), out).map_err(fontoxide_ttlib::Error::from)?;
	Ok(0)
}
