//! Port of `fontTools.diff`: comparing two fonts table by table, as a unified
//! diff of their TTX dumps, or as a summary of which tables are present and
//! binary identical.
//!
//! | fontoxide-diff | fontTools                 |
//! |----------------|---------------------------|
//! | [`summarize`], [`get_binary_exclude_tables`], [`main`], [`run`], [`pipe_output`] | `fontTools.diff` (`__init__`, `__main__`) |
//! | [`color`]      | `fontTools.diff.color`    |
//! | [`diff`]       | `fontTools.diff.diff`     |
//! | [`utils`]      | `fontTools.diff.utils`    |
//! | [`difflib`]    | Python's `difflib` (`SequenceMatcher`, `unified_diff`) |
//!
//! The diff tool, the pager and the modification times use the operating
//! system; the TTX dumps are made in memory (and saved to a temporary directory
//! only for an external diff tool).

#![forbid(unsafe_code)]

pub mod color;
pub mod diff;
pub mod difflib;
pub mod utils;

use std::collections::BTreeSet;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use fontoxide_misc::argparse::{Argument, Command, Nargs, Ty};
use fontoxide_ttlib::Font;

pub use diff::{DiffOptions, run_external_diff, u_diff};

/// The errors of `fontTools.diff`.
#[derive(Debug, thiserror::Error)]
pub enum DiffError {
	/// An error reading or dumping a font.
	#[error(transparent)]
	Ttlib(#[from] fontoxide_ttlib::Error),
	/// A file could not be read or written, or a program run.
	#[error(transparent)]
	Io(#[from] std::io::Error),
	/// What the external diff tool wrote to standard error (`IOError(err)`).
	#[error("{0}")]
	External(String),
	/// Another error.
	#[error("{0}")]
	Message(String),
}

/// The result type of this crate.
pub type Result<T, E = DiffError> = std::result::Result<T, E>;

/// `_iter_filtered_table_tags`: the tags not excluded, and included if there
/// is a list of tables to include.
fn filtered_table_tags<'a>(
	tags: impl IntoIterator<Item = &'a String>,
	include_tables: Option<&[String]>,
	exclude_tables: Option<&[String]>,
) -> Vec<&'a String> {
	tags.into_iter()
		.filter(|tag| {
			!exclude_tables.is_some_and(|t| !t.is_empty() && t.contains(tag))
				&& !include_tables.is_some_and(|t| !t.is_empty() && !t.contains(tag))
		})
		.collect()
}

/// The tables of the two fonts, as `TTFont(file, lazy=True, fontNumber=...)`
/// has them: the fonts, and their tags.
fn read_tables(
	file1: &str,
	file2: &str,
	font_number_1: i64,
	font_number_2: i64,
) -> Result<[(Font, BTreeSet<String>); 2]> {
	let font1 = diff::open_font(file1, font_number_1)?;
	let font2 = diff::open_font(file2, font_number_2)?;
	let tags = |font: &Font| -> BTreeSet<String> {
		font.reader()
			.map(|r| r.tags().map(|t| t.to_string()).collect())
			.unwrap_or_default()
	};
	let (tags1, tags2) = (tags(&font1), tags(&font2));
	Ok([(font1, tags1), (font2, tags2)])
}

/// The data of a table (`font.reader[tag]`).
fn table_data(font: &Font, tag: &str) -> Result<Vec<u8>> {
	let reader = font
		.reader()
		.ok_or_else(|| DiffError::Message(format!("KeyError: '{tag}'")))?;
	let tag = fontoxide_misc::py::latin1_encode(tag)
		.and_then(|b| <[u8; 4]>::try_from(b).ok())
		.map(|b| fontoxide_ttlib::Tag::new(&b))
		.ok_or_else(|| DiffError::Message(format!("KeyError: '{tag}'")))?;
	Ok(reader.table_data(tag)?.to_vec())
}

/// `summarize`: which tables are only in one of the fonts, and which of those
/// in both are binary identical. Returns whether the fonts are identical, and
/// the report.
pub fn summarize(
	file1: &str,
	file2: &str,
	include_tables: Option<&[String]>,
	exclude_tables: Option<&[String]>,
	font_number_1: i64,
	font_number_2: i64,
) -> Result<(bool, String)> {
	let [(font1, tags1), (font2, tags2)] = read_tables(file1, file2, font_number_1, font_number_2)?;
	let all: BTreeSet<&String> = tags1.union(&tags2).collect();
	let all_tags = filtered_table_tags(all, include_tables, exclude_tables);

	let only1: Vec<&String> = all_tags
		.iter()
		.copied()
		.filter(|t| tags1.contains(*t) && !tags2.contains(*t))
		.collect();
	let only2: Vec<&String> = all_tags
		.iter()
		.copied()
		.filter(|t| tags2.contains(*t) && !tags1.contains(*t))
		.collect();
	let both: Vec<&String> = all_tags
		.iter()
		.copied()
		.filter(|t| tags1.contains(*t) && tags2.contains(*t))
		.collect();

	let mut identical = true;
	let mut lines = String::new();
	lines.push_str("Binary table summary:\n");
	lines.push_str(&format!("  file1: {file1}\n"));
	lines.push_str(&format!("  file2: {file2}\n"));

	if !only1.is_empty() {
		identical = false;
		lines.push_str(&format!("\nTables only in file1 ({}):\n", only1.len()));
		for tag in only1 {
			lines.push_str(&format!(
				"- {tag} ({} bytes)\n",
				table_data(&font1, tag)?.len()
			));
		}
	}
	if !only2.is_empty() {
		identical = false;
		lines.push_str(&format!("\nTables only in file2 ({}):\n", only2.len()));
		for tag in only2 {
			lines.push_str(&format!(
				"+ {tag} ({} bytes)\n",
				table_data(&font2, tag)?.len()
			));
		}
	}

	lines.push_str(&format!("\nTables in both ({}):\n", both.len()));
	for tag in both {
		let data1 = table_data(&font1, tag)?;
		let data2 = table_data(&font2, tag)?;
		if data1 == data2 {
			lines.push_str(&format!("  {tag}: SAME ({} bytes)\n", data1.len()));
		} else {
			identical = false;
			lines.push_str(&format!(
				"* {tag}: DIFF ({} vs {} bytes)\n",
				data1.len(),
				data2.len()
			));
		}
	}

	lines.push_str(if identical {
		"\nResult: SAME\n"
	} else {
		"\nResult: DIFFERENT\n"
	});
	Ok((identical, lines))
}

/// `get_binary_exclude_tables`: the tables (filtered as in [`summarize`]) that
/// are in both fonts and binary identical.
pub fn get_binary_exclude_tables(
	file1: &str,
	file2: &str,
	include_tables: Option<&[String]>,
	exclude_tables: Option<&[String]>,
	font_number_1: i64,
	font_number_2: i64,
) -> Result<BTreeSet<String>> {
	let [(font1, tags1), (font2, tags2)] = read_tables(file1, file2, font_number_1, font_number_2)?;
	let all: BTreeSet<&String> = tags1.union(&tags2).collect();
	let mut out = BTreeSet::new();
	for tag in filtered_table_tags(all, include_tables, exclude_tables) {
		if tags1.contains(tag)
			&& tags2.contains(tag)
			&& table_data(&font1, tag)? == table_data(&font2, tag)?
		{
			out.insert(tag.clone());
		}
	}
	Ok(out)
}

/// `pipe_output`: writes the output to standard output, through a pager
/// (`$PAGER`, or `less -R`) if standard output is a terminal.
pub fn pipe_output(output: &str) -> Result<()> {
	if output.is_empty() {
		return Ok(());
	}
	let stdout = std::io::stdout();
	if !stdout.is_terminal() {
		stdout.lock().write_all(output.as_bytes())?;
		return Ok(());
	}
	let pager = std::env::var_os("PAGER")
		.filter(|p| !p.is_empty())
		.map(PathBuf::from)
		.or_else(|| utils::which("less"));
	let Some(pager) = pager else {
		stdout.lock().write_all(output.as_bytes())?;
		return Ok(());
	};
	let mut command = std::process::Command::new(&pager);
	if pager
		.file_name()
		.is_some_and(|n| n.to_string_lossy().contains("less"))
	{
		command.arg("-R");
	}
	let mut child = command.stdin(std::process::Stdio::piped()).spawn()?;
	if let Some(mut stdin) = child.stdin.take() {
		// The pager may quit before reading everything: not an error.
		match stdin.write_all(output.as_bytes()) {
			Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
			r => r?,
		}
	}
	child.wait()?;
	Ok(())
}

const HELP: &str = "\
usage: fonttools diff [-h] [-l] [-U LINES] [-t INCLUDE [INCLUDE ...]]
                      [-x EXCLUDE [EXCLUDE ...]] [--diff DIFF]
                      [--diff-arg DIFF_ARG] [--color {auto,never,always}]
                      [--y1 NUMBER] [--y2 NUMBER] [-a] [-b] [-q]
                      FILE1 FILE2

An OpenType table diff tool for fonts.

positional arguments:
  FILE1                 Font file path 1
  FILE2                 Font file path 2

options:
  -h, --help            show this help message and exit
  -l, --summary         Report table presence and binary equality only
  -U, --lines LINES     Number of context lines for unified diff (default: 3)
  -t, --include INCLUDE [INCLUDE ...]
                        Font tables to include. Multiple options are allowed.
  -x, --exclude EXCLUDE [EXCLUDE ...]
                        Font tables to exclude. Multiple options are allowed.
  --diff DIFF           Run external diff tool command (default: diff)
  --diff-arg DIFF_ARG   External diff tool arguments (default: -u)
  --color {auto,never,always}
                        Whether to colorize output (default: auto)
  --y1 NUMBER           Select font number for TrueType Collection (.ttc/.otc)
                        FILE1, starting from 0
  --y2 NUMBER           Select font number for TrueType Collection (.ttc/.otc)
                        FILE2, starting from 0
  -a, --always          Compare tables even if binary identical
  -b, --binary          Compare tables only if binaries differ (default)
  -q, --quiet           Suppress all output
";

const ARGUMENTS: &[Argument] = &[
	Argument::help(),
	Argument::flag(&["-l", "--summary"]),
	Argument::option(&["-U", "--lines"]).ty(Ty::Int),
	Argument::option(&["-t", "--include"]).nargs(Nargs::OneOrMore),
	Argument::option(&["-x", "--exclude"]).nargs(Nargs::OneOrMore),
	Argument::option(&["--diff"]),
	Argument::option(&["--diff-arg"]),
	Argument::option(&["--color"]),
	Argument::option(&["--y1"]).ty(Ty::Int),
	Argument::option(&["--y2"]).ty(Ty::Int),
	Argument::flag(&["-a", "--always"]),
	Argument::flag(&["-b", "--binary"]),
	Argument::flag(&["-q", "--quiet"]),
	Argument::positional("FILE1"),
	Argument::positional("FILE2"),
];

/// The command line of `fonttools diff`.
pub const COMMAND: Command = Command {
	prog: "fonttools diff",
	module: "diff",
	help: HELP,
	arguments: ARGUMENTS,
	required_groups: &[],
};

/// The choices of `--color`.
const COLORS: [&str; 3] = ["auto", "never", "always"];

/// `main`: compares two fonts (see [`run`]), writing the diff to standard
/// output (through a pager if it is a terminal) and the errors to standard
/// error. Returns the exit code: 0 if the fonts are the same, 1 if they differ,
/// 2 on errors.
pub fn main(args: &[&str]) -> Result<i32> {
	run_with(args, &mut std::io::stdout(), &mut std::io::stderr(), true)
}

/// `run(argv)`: compares two fonts, writing the diff (or the summary, or the
/// help) to `out` and the errors to `err`. Returns the exit code: 0 if the
/// fonts are the same, 1 if they differ, 2 on errors.
pub fn run(args: &[&str], out: &mut dyn Write, err: &mut dyn Write) -> Result<i32> {
	run_with(args, out, err, false)
}

/// [`run`]; with `pager`, the diff is written with [`pipe_output`] instead.
fn run_with(args: &[&str], out: &mut dyn Write, err: &mut dyn Write, pager: bool) -> Result<i32> {
	let parsed = match COMMAND.parse_args(args, out, err)? {
		Ok(parsed) => parsed,
		Err(code) => return Ok(code),
	};
	let color = parsed.str("--color").unwrap_or("auto");
	// LENIENT(L52): `argparse` checks the choices while parsing, so an invalid
	// choice is reported before the errors found after it (such as missing
	// files); here it is checked afterwards.
	if !COLORS.contains(&color) {
		let message = format!(
			"argument --color: invalid choice: {} (choose from 'auto', 'never', 'always')",
			fontoxide_misc::py::str_repr(color)
		);
		return Ok(COMMAND.error(err, &message)?);
	}
	let int = |key: &str, default: i128| -> i128 {
		parsed
			.str(key)
			.and_then(fontoxide_misc::py::parse_int)
			.unwrap_or(default)
	};
	let lines = int("--lines", 3);
	let y1 = int("--y1", -1).clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64;
	let y2 = int("--y2", -1).clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64;
	let quiet = parsed.flag("--quiet");
	let always = parsed.flag("--always");
	let file1 = parsed.str("FILE1").unwrap_or_default();
	let file2 = parsed.str("FILE2").unwrap_or_default();

	// Incompatible argument validations.
	if always && parsed.flag("--binary") {
		if !quiet {
			err.write_all(
				b"[*] Error: --always and --binary are mutually exclusive options. \
				  Please use ONLY one of these options in your command.\n",
			)?;
		}
		return Ok(2);
	}
	let binary = !always;

	// File path argument validations.
	for file in [file1, file2] {
		if !utils::file_exists(file) {
			if !quiet {
				writeln!(err, "[*] ERROR: The file path '{file}' can not be found.")?;
			}
			return Ok(2);
		}
	}

	let tables = |key: &str| parsed.get(key).map(|s| s.values.clone());
	let mut include_list = utils::get_tables_argument_list(tables("--include").as_deref());
	let mut exclude_list = utils::get_tables_argument_list(tables("--exclude").as_deref());

	if parsed.flag("--summary") {
		return match summarize(
			file1,
			file2,
			include_list.as_deref(),
			exclude_list.as_deref(),
			y1,
			y2,
		) {
			Ok((identical, output)) => {
				if !quiet {
					out.write_all(output.as_bytes())?;
				}
				Ok(if identical { 0 } else { 1 })
			}
			Err(e) => {
				if !quiet {
					writeln!(err, "[*] ERROR: {e}")?;
				}
				Ok(2)
			}
		};
	}

	if binary {
		// UPSTREAM-BUG(B74): not caught, unlike the same errors with `--summary`: they
		// are a traceback (e.g. a collection without `--y1`), returned as errors.
		let excluded_binary_tables = get_binary_exclude_tables(
			file1,
			file2,
			include_list.as_deref(),
			exclude_list.as_deref(),
			y1,
			y2,
		)?;
		// UPSTREAM-BUG(B73): if all the included tables are binary identical, the
		// list becomes empty, and an empty list of tables dumps all of them.
		match &mut include_list {
			Some(include) => include.retain(|t| !excluded_binary_tables.contains(t)),
			None => exclude_list
				.get_or_insert_with(Vec::new)
				.extend(excluded_binary_tables),
		}
	}

	// UPSTREAM-BUG(B72): `sys.stdout.isatty` is not called, so `auto` always
	// colors the output, even when it is not a terminal.
	let color_output = color == "always" || color == "auto";

	let diff_option = parsed.str("--diff");
	let diff_tool: Option<PathBuf> = match diff_option {
		None => utils::which("diff"),
		Some("") => None,
		Some(tool) => match utils::which(tool) {
			Some(path) => Some(path),
			None => {
				if !quiet {
					writeln!(
						err,
						"[*] ERROR: The external diff tool executable '{tool}' was not found."
					)?;
				}
				return Ok(2);
			}
		},
	};

	let options = DiffOptions {
		include_tables: include_list,
		exclude_tables: exclude_list,
		font_number_a: y1,
		font_number_b: y2,
		use_multiprocess: true,
	};
	let result = match &diff_tool {
		Some(tool) => {
			let diff_args: Vec<String> = match parsed.str("--diff-arg") {
				Some(args) => args.split_whitespace().map(str::to_owned).collect(),
				None => {
					let mut args = vec![if lines == 3 {
						"-u".to_owned()
					} else {
						format!("-u{lines}")
					}];
					if is_gnu_diff(tool) {
						args.push(r"-F^\s\s<".to_owned());
					}
					args
				}
			};
			run_external_diff(tool, &diff_args, file1, file2, &options)
		}
		None => u_diff(file1, file2, lines, &options),
	};
	match result {
		Ok(lines) => {
			let output: String = if color_output {
				lines
					.iter()
					.map(|l| color::color_unified_diff_line(l))
					.collect()
			} else {
				lines.concat()
			};
			if !quiet {
				if pager {
					out.flush()?;
					pipe_output(&output)?;
				} else {
					out.write_all(output.as_bytes())?;
				}
			}
			Ok(if output.is_empty() { 0 } else { 1 })
		}
		Err(e) => {
			if !quiet {
				writeln!(err, "[*] ERROR: {e}")?;
			}
			Ok(2)
		}
	}
}

/// `_is_gnu_diff`: whether the diff tool is GNU diff (says so in the output
/// of `--version`).
fn is_gnu_diff(diff_tool: &std::path::Path) -> bool {
	match std::process::Command::new(diff_tool)
		.arg("--version")
		.stdin(std::process::Stdio::null())
		.output()
	{
		Ok(output) => {
			String::from_utf8_lossy(&output.stdout).contains("GNU diffutils")
				|| String::from_utf8_lossy(&output.stderr).contains("GNU diffutils")
		}
		Err(_) => false,
	}
}
