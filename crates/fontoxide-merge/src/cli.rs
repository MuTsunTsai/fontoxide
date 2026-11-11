//! `fontTools.merge.main` and `__main__`: the command line.

use fontoxide_ttlib::WriteOptions;

use crate::{Merger, Options, Result};

/// The usage message, printed to standard error without files to merge.
pub const USAGE: &str = "\
usage: fonttools merge [font1 ... fontN] [--input-file=filelist.txt] [--output-file=merged.ttf] [--import-file=tables.ttx]
                                   [--drop-tables=tags] [--verbose] [--timing]

 font1 ... fontN              Files to merge.
 --input-file=<filename>      Read files to merge from a text file, each path new line. # Comment lines allowed.
 --output-file=<filename>     Specify output file name (default: merged.ttf).
 --import-file=<filename>     TTX file to import after merging. This can be used to set metadata.
 --drop-tables=<table tags>   Comma separated list of table tags to skip, case sensitive.
 --verbose                    Output progress information.
 --timing                     Output progress timing.";

/// Merges multiple fonts into one (`main`), with the arguments of
/// `fonttools merge`. Returns the exit code.
///
/// `--verbose` and `--timing` are accepted, but the logging is up to the
/// caller's `log` implementation.
pub fn main(args: &[&str]) -> Result<i32> {
	let mut options = Options::default();
	let args = options.parse_opts(args, &[])?;
	let mut fontfiles = Vec::new();
	if let Some(input_file) = &options.input_file {
		let text = std::fs::read_to_string(input_file)?;
		for line in text.split_inclusive('\n') {
			// Comment lines are allowed.
			if line
				.trim_start_matches(fontoxide_misc::py::is_space)
				.starts_with('#')
			{
				continue;
			}
			fontfiles.push(fontoxide_misc::py::strip(line).to_owned());
		}
	}
	fontfiles.extend(args);

	if fontfiles.is_empty() {
		eprintln!("{USAGE}");
		return Ok(1);
	}

	let mut merger = Merger::new(options.clone());
	let mut font = merger.merge_files(&fontfiles)?;

	if let Some(import_file) = &options.import_file {
		font.import_ttx(&std::fs::read_to_string(import_file)?)?;
	}

	std::fs::write(&options.output_file, font.write(&WriteOptions::default())?)?;
	Ok(0)
}
