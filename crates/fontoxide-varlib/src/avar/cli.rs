//! The command lines of `fontTools.varLib.avar.build` and
//! `fontTools.varLib.avar.unbuild` (their `main`).

use std::io::Write;

use fontoxide_designspacelib::DesignSpaceDocument;
use fontoxide_misc::argparse::{Argument, Command};
use fontoxide_misc::cli_tools::make_output_file_name;
use fontoxide_ttlib::{Font, ReadOptions, Tag, WriteOptions};

use super::{build, unbuild};
use crate::errors::Result;

const BUILD_HELP: &str = "\
usage: fonttools varLib.avar.build [-h] [-o OUTPUT_FILE] [-v]
                                   varfont.ttf family.designspace

Add `avar` table from designspace file to variable font.

positional arguments:
  varfont.ttf           Variable-font file.
  family.designspace    Designspace file.

options:
  -h, --help            show this help message and exit
  -o, --output-file OUTPUT_FILE
                        Output font file name.
  -v, --verbose         Run more verbosely.
";

const BUILD: Command = Command {
	prog: "fonttools varLib.avar.build",
	module: "varLib.avar.build",
	help: BUILD_HELP,
	arguments: &[
		Argument::help(),
		Argument::positional("varfont.ttf"),
		Argument::positional("family.designspace"),
		Argument::option(&["-o", "--output-file"]),
		Argument::flag(&["-v", "--verbose"]),
	],
	required_groups: &[],
};

const UNBUILD_HELP: &str = "\
usage: fonttools varLib.avar.unbuild [-h] varfont.ttf

Print `avar` table as a designspace snippet.

positional arguments:
  varfont.ttf  Variable-font file.

options:
  -h, --help   show this help message and exit
";

const UNBUILD: Command = Command {
	prog: "fonttools varLib.avar.unbuild",
	module: "varLib.avar.unbuild",
	help: UNBUILD_HELP,
	arguments: &[Argument::help(), Argument::positional("varfont.ttf")],
	required_groups: &[],
};

/// `fontTools.varLib.avar.build.main`: adds an `avar` table from a designspace
/// file to a variable font, saving it to `-o` or `<font>.avar.<ext>`. Returns
/// the exit code.
///
/// `-v` is accepted; logging is up to the caller's `log` implementation.
pub fn build_main(args: &[&str]) -> Result<i32> {
	build_run(
		args,
		&mut std::io::stdout().lock(),
		&mut std::io::stderr().lock(),
	)
}

/// [`build_main`], writing the help to `out` and the usage errors to `err`.
pub fn build_run(args: &[&str], out: &mut dyn Write, err: &mut dyn Write) -> Result<i32> {
	let options = match BUILD.parse_args(args, out, err)? {
		Ok(options) => options,
		Err(code) => return Ok(code),
	};
	let font_path = options.str("varfont.ttf").unwrap_or_default();
	let designspace_path = options.str("family.designspace").unwrap_or_default();

	let mut font = Font::read(std::fs::read(font_path)?, ReadOptions::default())?;
	let mut designspace = DesignSpaceDocument::from_file(designspace_path)?;
	build(&mut font, &mut designspace)?;

	let outfile = match options.str("--output-file") {
		None => make_output_file_name(font_path, None, None, true, ".avar"),
		Some(outfile) => outfile.to_owned(),
	};
	if !outfile.is_empty() {
		log::info!("Saving {outfile}");
		std::fs::write(&outfile, font.write(&WriteOptions::default())?)?;
	}
	Ok(0)
}

/// `fontTools.varLib.avar.unbuild.main`: prints the `avar` table of a variable
/// font as a designspace snippet. Returns the exit code: 1 if the font is not
/// variable.
pub fn unbuild_main(args: &[&str]) -> Result<i32> {
	unbuild_run(
		args,
		&mut std::io::stdout().lock(),
		&mut std::io::stderr().lock(),
	)
}

/// [`unbuild_main`], writing the snippet and the help to `out`, and the errors
/// to `err`.
pub fn unbuild_run(args: &[&str], out: &mut dyn Write, err: &mut dyn Write) -> Result<i32> {
	let options = match UNBUILD.parse_args(args, out, err)? {
		Ok(options) => options,
		Err(code) => return Ok(code),
	};
	let font_path = options.str("varfont.ttf").unwrap_or_default();
	let mut font = Font::read(std::fs::read(font_path)?, ReadOptions::default())?;
	if !font.contains(Tag::new(b"fvar")) {
		writeln!(err, "Not a variable font.")?;
		return Ok(1);
	}
	out.write_all(unbuild(&mut font)?.as_bytes())?;
	Ok(0)
}
