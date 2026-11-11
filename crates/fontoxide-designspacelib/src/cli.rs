//! The command line of `fontTools.designspaceLib` (`main`, which
//! `python -m fontTools.designspaceLib` runs): reads a designspace file and
//! writes it back.

use std::io::Write;

use fontoxide_misc::argparse::{Argument, Command};

use crate::{DesignSpaceDocument, Result};

const HELP: &str = "\
usage: designspaceLib [-h] input output

Roundtrip .designspace file through the DesignSpaceDocument class

positional arguments:
  input
  output

options:
  -h, --help  show this help message and exit
";

const COMMAND: Command = Command {
	prog: "designspaceLib",
	module: "designspaceLib",
	help: HELP,
	arguments: &[
		Argument::help(),
		Argument::positional("input"),
		Argument::positional("output"),
	],
	required_groups: &[],
};

/// `main`: roundtrips the designspace file `input` through
/// [`DesignSpaceDocument`], writing it to `output`. Returns the exit code.
pub fn main(args: &[&str]) -> Result<i32> {
	run(
		args,
		&mut std::io::stdout().lock(),
		&mut std::io::stderr().lock(),
	)
}

/// [`main`], writing the help to `out` and the usage errors to `err`.
pub fn run(args: &[&str], out: &mut dyn Write, err: &mut dyn Write) -> Result<i32> {
	let options = match COMMAND.parse_args(args, out, err)? {
		Ok(options) => options,
		Err(code) => return Ok(code),
	};
	let input = options.str("input").unwrap_or_default();
	let output = options.str("output").unwrap_or_default();
	let mut ds = DesignSpaceDocument::from_file(input)?;
	ds.write(output)?;
	Ok(0)
}
