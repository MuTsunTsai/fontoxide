//! Port of `fontTools.ttx`: the `ttx` command line, which dumps fonts to TTX
//! and compiles TTX to fonts.
//!
//! Standard input and output (the file name `-`) go through an [`Io`], so
//! that they can be replaced; [`main`] uses the process's.

use std::io::{Read, Write};

use fontoxide_misc::cli_tools::make_output_file_name;
use fontoxide_ttlib::binary::calc_checksum;
use fontoxide_ttlib::tables::Head;
use fontoxide_ttlib::{
	Flavor, Font, ReadOptions, TableKey, Tag, TtxImportOptions, TtxOptions, WriteOptions,
};

use crate::getopt::{GetoptError, gnu_getopt};

/// The module's documentation, which `-h` prints (`__doc__`).
pub const DOC: &str = r#"usage: ttx [options] inputfile1 [... inputfileN]

TTX -- From OpenType To XML And Back

If an input file is a TrueType or OpenType font file, it will be
decompiled to a TTX file (an XML-based text format).
If an input file is a TTX file, it will be compiled to whatever
format the data is in, a TrueType or OpenType/CFF font file.
A special input value of - means read from the standard input.

Output files are created so they are unique: an existing file is
never overwritten.

General options
===============

-h Help            print this message.
--version          show version and exit.
-d <outputfolder>  Specify a directory where the output files are
                   to be created.
-o <outputfile>    Specify a file to write the output to. A special
                   value of - would use the standard output.
-f                 Overwrite existing output file(s), ie. don't append
                   numbers.
-v                 Verbose: more messages will be written to stdout
                   about what is being done.
-q                 Quiet: No messages will be written to stdout about
                   what is being done.
-a                 allow virtual glyphs ID's on compile or decompile.

Dump options
============

-l           List table info: instead of dumping to a TTX file, list
             some minimal info about each table.
-t <table>   Specify a table to dump. Multiple -t options
             are allowed. When no -t option is specified, all tables
             will be dumped.
-x <table>   Specify a table to exclude from the dump. Multiple
             -x options are allowed. -t and -x are mutually exclusive.
-s           Split tables: save the TTX data into separate TTX files per
             table and write one small TTX file that contains references
             to the individual table dumps. This file can be used as
             input to ttx, as long as the table files are in the
             same directory.
-g           Split glyf table: Save the glyf data into separate TTX files
             per glyph and write a small TTX for the glyf table which
             contains references to the individual TTGlyph elements.
             NOTE: specifying -g implies -s (no need for -s together
             with -g)
-i           Do NOT disassemble TT instructions: when this option is
             given, all TrueType programs (glyph programs, the font
             program and the pre-program) will be written to the TTX
             file as hex data instead of assembly. This saves some time
             and makes the TTX file smaller.
-z <format>  Specify a bitmap data export option for EBDT:
             {'raw', 'row', 'bitwise', 'extfile'} or for the CBDT:
             {'raw', 'extfile'} Each option does one of the following:

             -z raw
               export the bitmap data as a hex dump
             -z row
               export each row as hex data
             -z bitwise
               export each row as binary in an ASCII art style
             -z extfile
               export the data as external files with XML references

             If no export format is specified 'raw' format is used.
-e           Don't ignore decompilation errors, but show a full traceback
             and abort.
-y <number>  Select font number for TrueType Collection (.ttc/.otc),
             starting from 0.
--unicodedata <UnicodeData.txt>
             Use custom database file to write character names in the
             comments of the cmap TTX output.
--newline <value>
             Control how line endings are written in the XML file. It
             can be 'LF', 'CR', or 'CRLF'. If not specified, the
             default platform-specific line endings are used.

Compile options
===============

-m           Merge with TrueType-input-file: specify a TrueType or
             OpenType font file to be merged with the TTX file. This
             option is only valid when at most one TTX file is specified.
-b           Don't recalc glyph bounding boxes: use the values in the
             TTX file as-is.
--recalc-timestamp
             Set font 'modified' timestamp to current time.
             By default, the modification time of the TTX file will be
             used.
--no-recalc-timestamp
             Keep the original font 'modified' timestamp.
--flavor <type>
             Specify flavor of output font file. May be 'woff' or 'woff2'.
             Note that WOFF2 requires the Brotli Python extension,
             available at https://github.com/google/brotli
--with-zopfli
             Use Zopfli instead of Zlib to compress WOFF. The Python
             extension is available at https://pypi.python.org/pypi/zopfli
--optimize-font-speed
             Enable optimizations that prioritize speed over file size.
             This mainly affects how glyf t able and gvar / VARC tables are
             compiled. The produced fonts will be larger, but rendering
             performance will be improved with HarfBuzz and other text
             layout engines.
"#;

/// The errors of the command line.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
	/// `getopt.GetoptError`: a usage error, reported with the help.
	#[error("{0}")]
	Getopt(String),
	/// `SystemExit` after printing to standard output (`-h`, `--version`).
	#[error("SystemExit: {code}")]
	Exit {
		/// The exit code.
		code: i32,
		/// What was printed to standard output.
		output: String,
	},
	/// `ValueError`, such as a font number that is not an integer.
	#[error("{0}")]
	Value(String),
	/// An error of `fontTools.ttLib`, `TTLibError` or another one.
	#[error(transparent)]
	TtLib(#[from] fontoxide_ttlib::Error),
	/// An I/O error.
	#[error(transparent)]
	Io(#[from] std::io::Error),
}

impl From<GetoptError> for Error {
	fn from(e: GetoptError) -> Self {
		Error::Getopt(e.0)
	}
}

/// Shorthand for `Result<T, fontoxide_cli::ttx::Error>`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

fn getopt_error(message: impl Into<String>) -> Error {
	Error::Getopt(message.into())
}

/// Standard input, output and error, for the file name `-` and for messages.
pub struct Io<'a> {
	stdin: &'a mut dyn Read,
	/// Standard input read so far, for peeking (`sys.stdin.buffer.peek`).
	stdin_data: Option<Vec<u8>>,
	stdout: &'a mut dyn Write,
	stderr: &'a mut dyn Write,
}

impl std::fmt::Debug for Io<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Io").finish_non_exhaustive()
	}
}

impl<'a> Io<'a> {
	/// Streams to use as standard input, output and error.
	pub fn new(
		stdin: &'a mut dyn Read,
		stdout: &'a mut dyn Write,
		stderr: &'a mut dyn Write,
	) -> Self {
		Io {
			stdin,
			stdin_data: None,
			stdout,
			stderr,
		}
	}

	/// Standard input, without consuming it.
	fn peek_stdin(&mut self) -> std::io::Result<&[u8]> {
		if self.stdin_data.is_none() {
			let mut data = Vec::new();
			self.stdin.read_to_end(&mut data)?;
			self.stdin_data = Some(data);
		}
		Ok(self.stdin_data.as_deref().unwrap_or_default())
	}

	/// Reads all of standard input.
	fn read_stdin(&mut self) -> std::io::Result<Vec<u8>> {
		self.peek_stdin()?;
		Ok(self.stdin_data.replace(Vec::new()).unwrap_or_default())
	}

	/// Writes text to standard output, as Python's text-mode `sys.stdout`.
	fn print(&mut self, text: &str) -> std::io::Result<()> {
		self.stdout.write_all(text_mode(text).as_bytes())
	}

	/// Writes text to standard error, as Python's text-mode `sys.stderr`.
	fn eprint(&mut self, text: &str) -> std::io::Result<()> {
		self.stderr.write_all(text_mode(text).as_bytes())
	}
}

/// Text written to Python's standard streams, which translate `\n` to `\r\n`
/// on Windows.
fn text_mode(text: &str) -> std::borrow::Cow<'_, str> {
	if cfg!(windows) {
		std::borrow::Cow::Owned(text.replace('\n', "\r\n"))
	} else {
		std::borrow::Cow::Borrowed(text)
	}
}

/// Runs `f` with the process's standard streams.
fn with_std_io<R>(f: impl FnOnce(&mut Io<'_>) -> R) -> R {
	let stdin = std::io::stdin();
	let stdout = std::io::stdout();
	let stderr = std::io::stderr();
	let mut stdin = stdin.lock();
	let mut stdout = stdout.lock();
	let mut stderr = stderr.lock();
	f(&mut Io::new(&mut stdin, &mut stdout, &mut stderr))
}

/// The command line options (`Options`).
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
	/// `-l`: list the tables instead of dumping them (`listTables`).
	pub list_tables: bool,
	/// `-d`: the directory of the output files (`outputDir`).
	pub output_dir: Option<String>,
	/// `-o`: the output file (`outputFile`); `-` is standard output.
	pub output_file: Option<String>,
	/// `-f`: overwrite existing output files (`overWrite`).
	pub over_write: bool,
	/// `-v` (`verbose`).
	pub verbose: bool,
	/// `-q` (`quiet`).
	pub quiet: bool,
	/// `-s`: a TTX file per table (`splitTables`).
	pub split_tables: bool,
	/// `-g`: a TTX file per glyph of `glyf` (`splitGlyphs`).
	pub split_glyphs: bool,
	/// Not `-i`: disassemble TrueType instructions (`disassembleInstructions`).
	pub disassemble_instructions: bool,
	/// `-m`: the font to merge the TTX into (`mergeFile`).
	pub merge_file: Option<String>,
	/// Not `-b`: recalculate bounding boxes (`recalcBBoxes`).
	pub recalc_bboxes: bool,
	/// Not `-e`: dump tables that fail to decompile as raw data
	/// (`ignoreDecompileErrors`).
	pub ignore_decompile_errors: bool,
	/// `-z`: the format of bitmap glyph data (`bitmapGlyphDataFormat`).
	pub bitmap_glyph_data_format: String,
	/// `--unicodedata`: a custom `UnicodeData.txt` (`unicodedata`).
	pub unicodedata: Option<String>,
	/// `--newline`: the line ending of TTX output (`newlinestr`).
	pub newlinestr: String,
	/// `--recalc-timestamp` / `--no-recalc-timestamp` (`recalcTimestamp`).
	pub recalc_timestamp: Option<bool>,
	/// `--flavor`: `woff` or `woff2` (`flavor`).
	pub flavor: Option<String>,
	/// `--with-zopfli` (`useZopfli`).
	pub use_zopfli: bool,
	/// `--optimize-font-speed` (`optimizeFontSpeed`).
	pub optimize_font_speed: bool,
	/// `-t`: the tables to dump, padded to four characters (`onlyTables`).
	pub only_tables: Vec<String>,
	/// `-x`: the tables not to dump, padded to four characters (`skipTables`).
	pub skip_tables: Vec<String>,
	/// `-y`: the font of a collection, or -1 (`fontNumber`).
	pub font_number: i64,
	/// The logging level from `-v` and `-q` (`logLevel`).
	pub log_level: log::LevelFilter,
}

impl Default for Options {
	/// The options without any command line option.
	fn default() -> Self {
		Options {
			list_tables: false,
			output_dir: None,
			output_file: None,
			over_write: false,
			verbose: false,
			quiet: false,
			split_tables: false,
			split_glyphs: false,
			disassemble_instructions: true,
			merge_file: None,
			recalc_bboxes: true,
			ignore_decompile_errors: true,
			bitmap_glyph_data_format: "raw".into(),
			unicodedata: None,
			newlinestr: "\n".into(),
			recalc_timestamp: None,
			flavor: None,
			use_zopfli: false,
			optimize_font_speed: false,
			only_tables: Vec::new(),
			skip_tables: Vec::new(),
			font_number: -1,
			log_level: log::LevelFilter::Info,
		}
	}
}

/// `str.ljust(4)`.
fn ljust4(value: &str) -> String {
	format!("{value:<4}")
}

impl Options {
	/// Interprets the parsed options (`Options(rawOptions, numFiles)`). `-h`
	/// and `--version` fail with [`Error::Exit`], with the text to print.
	pub fn new(raw_options: &[(&str, &str)], num_files: usize) -> Result<Self> {
		let mut o = Options::default();
		for &(option, value) in raw_options {
			match option {
				// General options.
				"-h" => {
					return Err(Error::Exit {
						code: 0,
						output: format!("{DOC}\n"),
					});
				}
				"--version" => {
					return Err(Error::Exit {
						code: 0,
						output: format!("{}\n", fontoxide_ttlib::FONTTOOLS_VERSION),
					});
				}
				"-d" => {
					if !std::path::Path::new(value).is_dir() {
						return Err(getopt_error(
							"The -d option value must be an existing directory",
						));
					}
					o.output_dir = Some(value.to_owned());
				}
				"-o" => o.output_file = Some(value.to_owned()),
				"-f" => o.over_write = true,
				"-v" => o.verbose = true,
				"-q" => o.quiet = true,
				// Dump options.
				"-l" => o.list_tables = true,
				// Pad with space if the table tag is shorter than 4.
				"-t" => o.only_tables.push(ljust4(value)),
				"-x" => o.skip_tables.push(ljust4(value)),
				"-s" => o.split_tables = true,
				"-g" => {
					// -g implies (and forces) splitTables.
					o.split_glyphs = true;
					o.split_tables = true;
				}
				"-i" => o.disassemble_instructions = false,
				"-z" => {
					const VALID: [&str; 4] = ["raw", "row", "bitwise", "extfile"];
					if !VALID.contains(&value) {
						// UPSTREAM-BUG(B53): the message names the option, not the value.
						return Err(getopt_error(format!(
							"-z does not allow {option} as a format. Use ('raw', 'row', 'bitwise', 'extfile')"
						)));
					}
					o.bitmap_glyph_data_format = value.to_owned();
				}
				"-y" => {
					o.font_number = fontoxide_misc::py::parse_int(value)
						.and_then(|n| i64::try_from(n).ok())
						.ok_or_else(|| {
							Error::Value(format!(
								"invalid literal for int() with base 10: {}",
								fontoxide_misc::py::str_repr(value)
							))
						})?;
				}
				// Compile options.
				"-m" => o.merge_file = Some(value.to_owned()),
				"-b" => o.recalc_bboxes = false,
				"-e" => o.ignore_decompile_errors = false,
				"--unicodedata" => o.unicodedata = Some(value.to_owned()),
				"--newline" => {
					o.newlinestr = match value {
						"LF" => "\n",
						"CR" => "\r",
						"CRLF" => "\r\n",
						_ => {
							return Err(getopt_error(format!(
								"Invalid choice for --newline: {} (choose from 'LF', 'CR', 'CRLF')",
								fontoxide_misc::py::str_repr(value)
							)));
						}
					}
					.to_owned();
				}
				"--recalc-timestamp" => o.recalc_timestamp = Some(true),
				"--no-recalc-timestamp" => o.recalc_timestamp = Some(false),
				"--flavor" => o.flavor = Some(value.to_owned()),
				"--with-zopfli" => o.use_zopfli = true,
				"--optimize-font-speed" => o.optimize_font_speed = true,
				_ => {}
			}
		}
		if o.verbose && o.quiet {
			return Err(getopt_error("-q and -v options are mutually exclusive"));
		}
		o.log_level = if o.verbose {
			log::LevelFilter::Debug
		} else if o.quiet {
			log::LevelFilter::Warn
		} else {
			log::LevelFilter::Info
		};
		if o.merge_file.is_some() && o.flavor.is_some() {
			return Err(getopt_error(
				"-m and --flavor options are mutually exclusive",
			));
		}
		if !o.only_tables.is_empty() && !o.skip_tables.is_empty() {
			return Err(getopt_error("-t and -x options are mutually exclusive"));
		}
		if o.merge_file.is_some() && num_files > 1 {
			return Err(getopt_error(
				"Must specify exactly one TTX source file when using -m",
			));
		}
		if o.flavor.as_deref() != Some("woff") && o.use_zopfli {
			return Err(getopt_error(
				"--with-zopfli option requires --flavor 'woff'",
			));
		}
		Ok(o)
	}
}

/// What a job does with its input file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
	/// [`tt_list`].
	TtList,
	/// [`tt_dump`].
	TtDump,
	/// [`tt_compile`].
	TtCompile,
}

impl Action {
	/// The name of the function in fontTools (`__name__`).
	pub fn name(self) -> &'static str {
		match self {
			Action::TtList => "ttList",
			Action::TtDump => "ttDump",
			Action::TtCompile => "ttCompile",
		}
	}
}

/// A job: an action with its input and output files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
	/// The action.
	pub action: Action,
	/// The input file.
	pub input: String,
	/// The output file.
	pub output: String,
}

/// The type of a file, from its first bytes or its extension (the strings of
/// `guessFileType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
	/// `TTF`: a TrueType font, or a `.dfont` file.
	Ttf,
	/// `OTF`: an OpenType font with CFF outlines.
	Otf,
	/// `TTC`: a font collection.
	Ttc,
	/// `WOFF`.
	Woff,
	/// `WOFF2`.
	Woff2,
	/// `TTX`: a TTX file of a TrueType font.
	Ttx,
	/// `OTX`: a TTX file of a CFF font.
	Otx,
}

impl FileType {
	/// The name fontTools uses.
	pub fn as_str(self) -> &'static str {
		match self {
			FileType::Ttf => "TTF",
			FileType::Otf => "OTF",
			FileType::Ttc => "TTC",
			FileType::Woff => "WOFF",
			FileType::Woff2 => "WOFF2",
			FileType::Ttx => "TTX",
			FileType::Otx => "OTX",
		}
	}
}

/// Guesses the type of a file (`guessFileType`); `None` if it is none of the
/// known types or cannot be read. `-` (standard input) is not supported here:
/// see [`guess_file_type_with_io`].
///
/// fontTools also takes Mac files of type `sfnt` or `FFIL` as TrueType fonts,
/// on macOS, where it can read the type; fontoxide reads no file types, as
/// fontTools elsewhere.
pub fn guess_file_type(file_name: &str) -> Option<FileType> {
	let mut header = Vec::with_capacity(256);
	let file = std::fs::File::open(file_name).ok()?;
	file.take(256).read_to_end(&mut header).ok()?;
	guess_from_header(&header, extension(file_name))
}

/// [`guess_file_type`], where `-` peeks at the standard input of `io`.
pub fn guess_file_type_with_io(file_name: &str, io: &mut Io<'_>) -> Option<FileType> {
	if file_name != "-" {
		return guess_file_type(file_name);
	}
	let data = io.peek_stdin().ok()?;
	let header = &data[..data.len().min(256)];
	guess_from_header(header, "")
}

/// The extension of a file name (`os.path.splitext`).
fn extension(file_name: &str) -> &str {
	let separators: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
	let name = file_name.rsplit(separators).next().unwrap_or(file_name);
	let leading = name.len() - name.trim_start_matches('.').len();
	match name[leading..].rfind('.') {
		Some(at) => &name[leading + at..],
		None => "",
	}
}

fn guess_from_header(header: &[u8], ext: &str) -> Option<FileType> {
	let header = match header.strip_prefix(b"\xef\xbb\xbf") {
		Some(rest) if rest.starts_with(b"<?xml") => rest,
		_ => header,
	};
	if ext == ".dfont" {
		return Some(FileType::Ttf);
	}
	match header.get(..4).unwrap_or(header) {
		b"OTTO" => Some(FileType::Otf),
		b"ttcf" => Some(FileType::Ttc),
		b"\0\x01\0\0" | b"true" => Some(FileType::Ttf),
		b"wOFF" => Some(FileType::Woff),
		b"wOF2" => Some(FileType::Woff2),
		b"<?xm" => {
			// re.compile("""sfntVersion=['"]OTTO["']"""), on the Latin-1 text.
			let otto = header.windows(18).any(|w| {
				w.starts_with(b"sfntVersion=")
					&& matches!(w[12], b'\'' | b'"')
					&& &w[13..17] == b"OTTO"
					&& matches!(w[17], b'\'' | b'"')
			});
			Some(if otto { FileType::Otx } else { FileType::Ttx })
		}
		_ => None,
	}
}

/// The getopt specification of `ttx`.
const SHORT_OPTIONS: &str = "ld:o:fvqht:x:sgim:z:baey:";
const LONG_OPTIONS: [&str; 8] = [
	"unicodedata=",
	"recalc-timestamp",
	"no-recalc-timestamp",
	"flavor=",
	"version",
	"with-zopfli",
	"newline=",
	"optimize-font-speed",
];

/// Parses the command line into jobs (`parseOptions`). Without `-o`, the
/// output files are named after the input files, with a number added to avoid
/// overwriting existing files (unless `-f`), and created empty at once, so
/// that two jobs do not choose the same name.
pub fn parse_options(args: &[&str]) -> Result<(Vec<Job>, Options)> {
	with_std_io(|io| parse_options_with_io(args, io))
}

/// [`parse_options`], reading standard input from `io`.
pub fn parse_options_with_io(args: &[&str], io: &mut Io<'_>) -> Result<(Vec<Job>, Options)> {
	let (raw_options, files) = gnu_getopt(args, SHORT_OPTIONS, &LONG_OPTIONS)?;
	let raw: Vec<(&str, &str)> = raw_options
		.iter()
		.map(|(o, v)| (o.as_str(), v.as_str()))
		.collect();
	let options = Options::new(&raw, files.len())?;
	if files.is_empty() {
		return Err(getopt_error("Must specify at least one input file"));
	}
	let mut jobs = Vec::new();
	for input in files {
		if input != "-" && !std::path::Path::new(&input).is_file() {
			return Err(getopt_error(format!("File not found: \"{input}\"")));
		}
		let flavor_extension = |default: &str| match &options.flavor {
			Some(flavor) if !flavor.is_empty() => format!(".{flavor}"),
			_ => default.to_owned(),
		};
		let (extension, action) = match guess_file_type_with_io(&input, io) {
			Some(
				FileType::Otf | FileType::Ttf | FileType::Ttc | FileType::Woff | FileType::Woff2,
			) => {
				let action = if options.list_tables {
					Action::TtList
				} else {
					Action::TtDump
				};
				(".ttx".to_owned(), action)
			}
			Some(FileType::Ttx) => (flavor_extension(".ttf"), Action::TtCompile),
			Some(FileType::Otx) => (flavor_extension(".otf"), Action::TtCompile),
			None => return Err(getopt_error(format!("Unknown file type: \"{input}\""))),
		};
		let output = match &options.output_file {
			Some(output) if !output.is_empty() => output.clone(),
			_ => {
				if input == "-" {
					return Err(getopt_error("Must provide -o when reading from stdin"));
				}
				let output = make_output_file_name(
					&input,
					options.output_dir.as_deref(),
					Some(&extension),
					options.over_write,
					"",
				);
				// 'touch' the output file to avoid a race condition in choosing file
				// names.
				if action != Action::TtList {
					std::fs::OpenOptions::new()
						.append(true)
						.create(true)
						.open(&output)?;
				}
				output
			}
		};
		jobs.push(Job {
			action,
			input,
			output,
		});
	}
	Ok((jobs, options))
}

/// Runs the jobs (`process`).
pub fn process(jobs: &[Job], options: &Options) -> Result<()> {
	with_std_io(|io| process_with_io(jobs, options, io))
}

/// [`process`] with standard streams from `io`.
pub fn process_with_io(jobs: &[Job], options: &Options, io: &mut Io<'_>) -> Result<()> {
	for job in jobs {
		match job.action {
			Action::TtList => {
				let mut out = Vec::new();
				let result = tt_list(&job.input, &job.output, options, &mut out);
				io.print(&String::from_utf8_lossy(&out))?;
				result?;
			}
			Action::TtDump => tt_dump_with_io(&job.input, &job.output, options, io)?,
			Action::TtCompile => tt_compile_with_io(&job.input, &job.output, options, io)?,
		}
	}
	Ok(())
}

/// The font number of the options, for [`ReadOptions::font_number`].
fn font_number(options: &Options) -> Option<usize> {
	usize::try_from(options.font_number).ok()
}

/// Lists the tag, checksum, length and offset of each table of a font
/// (`ttList`), writing to `out`. `output` is not used.
pub fn tt_list(input: &str, _output: &str, options: &Options, out: &mut dyn Write) -> Result<()> {
	let font = Font::read(
		std::fs::read(input)?,
		ReadOptions {
			font_number: font_number(options),
			..Default::default()
		},
	)?;
	let Some(reader) = font.reader() else {
		return Ok(());
	};
	let mut entries: Vec<_> = reader.entries().iter().collect();
	entries.sort_by_key(|e| e.tag);
	writeln!(out, "Listing table info for \"{input}\":")?;
	let row = |out: &mut dyn Write, a: &str, b: &str, c: &str, d: &str| {
		writeln!(out, "    {a:>4}  {b:>10}  {c:>8}  {d:>8}")
	};
	row(out, "tag ", "  checksum", "  length", "  offset")?;
	row(out, "----", "----------", "--------", "--------")?;
	for entry in entries {
		let check_sum = if font.flavor == Some(Flavor::Woff2) {
			// WOFF2 doesn't store table checksums, so they must be calculated.
			calc_checksum(&reader.stored_table_data(entry.tag)?)
		} else {
			entry.check_sum
		};
		row(
			out,
			&entry.tag.to_string(),
			&format!("0x{check_sum:08X}"),
			&entry.length.to_string(),
			&entry.offset.to_string(),
		)?;
	}
	writeln!(out)?;
	Ok(())
}

/// Measures the time of a step, for the debug messages of `Timer`.
struct Timer {
	#[cfg(not(target_family = "wasm"))]
	start: std::time::Instant,
}

impl Timer {
	fn start() -> Self {
		Timer {
			#[cfg(not(target_family = "wasm"))]
			start: std::time::Instant::now(),
		}
	}

	fn seconds(&self) -> f64 {
		#[cfg(not(target_family = "wasm"))]
		return self.start.elapsed().as_secs_f64();
		#[cfg(target_family = "wasm")]
		return 0.0;
	}
}

/// The tables of `-t` and `-x`.
fn table_keys(tables: &[String]) -> Result<Vec<TableKey>> {
	tables
		.iter()
		.map(|t| {
			if t == "GlyphOrder" {
				return Ok(TableKey::GlyphOrder);
			}
			// LENIENT(L41): fontTools takes any string, and finds no table for a name
			// longer than four characters.
			Tag::from_str_exact(t).map(TableKey::Table).ok_or_else(|| {
				Error::Value(format!(
					"invalid table tag: {}",
					fontoxide_misc::py::str_repr(t)
				))
			})
		})
		.collect()
}

/// Dumps a font to TTX (`ttDump`); `-` is standard input or output.
pub fn tt_dump(input: &str, output: &str, options: &Options) -> Result<()> {
	with_std_io(|io| tt_dump_with_io(input, output, options, io))
}

/// [`tt_dump`] with standard streams from `io`.
pub fn tt_dump_with_io(
	input: &str,
	output: &str,
	options: &Options,
	io: &mut Io<'_>,
) -> Result<()> {
	let timer = Timer::start();
	let (data, input_name) = if input == "-" {
		(io.read_stdin()?, "<stdin>")
	} else {
		(std::fs::read(input)?, input)
	};
	let output_name = if output == "-" { "<stdout>" } else { output };
	log::info!("Dumping \"{input_name}\" to \"{output_name}\"...");
	if options.unicodedata.is_some() {
		// LENIENT(L41): fontoxide cannot replace its Unicode character names.
		return Err(fontoxide_ttlib::Error::ttlib(
			"--unicodedata is not supported: fontoxide cannot use a custom UnicodeData.txt",
		)
		.into());
	}
	let mut font = Font::read(
		data,
		ReadOptions {
			font_number: font_number(options),
			ignore_decompile_errors: options.ignore_decompile_errors,
			..Default::default()
		},
	)?;
	let ttx_options = TtxOptions {
		tables: table_keys(&options.only_tables)?,
		skip_tables: table_keys(&options.skip_tables)?,
		write_version: true,
		newline: options.newlinestr.clone(),
		disassemble_instructions: options.disassemble_instructions,
		bitmap_glyph_data_format: options.bitmap_glyph_data_format.clone(),
		split_tables: options.split_tables,
		split_glyphs: options.split_glyphs,
	};
	if output == "-" {
		let text = font.to_ttx(&ttx_options)?;
		io.print(&text)?;
	} else {
		for file in font.to_ttx_files(output, &ttx_options)? {
			std::fs::write(&file.path, file.contents)?;
		}
	}
	log::debug!("Done dumping TTX in {:.3} seconds", timer.seconds());
	Ok(())
}

/// Compiles a TTX file to a font (`ttCompile`); `-` is standard input or
/// output. Without `--recalc-timestamp` or `--no-recalc-timestamp`, the
/// modification time of the TTX file becomes the `modified` time of `head`.
pub fn tt_compile(input: &str, output: &str, options: &Options) -> Result<()> {
	with_std_io(|io| tt_compile_with_io(input, output, options, io))
}

/// [`tt_compile`] with standard streams from `io`.
pub fn tt_compile_with_io(
	input: &str,
	output: &str,
	options: &Options,
	io: &mut Io<'_>,
) -> Result<()> {
	let timer = Timer::start();
	let input_name = if input == "-" { "<stdin>" } else { input };
	// fontTools formats the output stream itself, not its name.
	let output_desc = if output == "-" {
		"<_io.BufferedWriter name='<stdout>'>"
	} else {
		output
	};
	log::info!("Compiling \"{input_name}\" to \"{output_desc}\"...");
	let recalc_timestamp = options.recalc_timestamp == Some(true);
	let mut font = match &options.merge_file {
		Some(path) => Font::read(
			std::fs::read(path)?,
			ReadOptions {
				recalc_bboxes: options.recalc_bboxes,
				recalc_timestamp,
				..Default::default()
			},
		)?,
		None => {
			let mut font = Font::new();
			font.recalc_bboxes = options.recalc_bboxes;
			font.recalc_timestamp = recalc_timestamp;
			font.flavor = match options.flavor.as_deref() {
				None | Some("") => None,
				Some("woff") => Some(Flavor::Woff),
				Some("woff2") => Some(Flavor::Woff2),
				Some(other) => {
					return Err(fontoxide_ttlib::Error::Assertion(format!(
						"Unknown flavor '{other}'"
					))
					.into());
				}
			};
			font
		}
	};
	if options.optimize_font_speed {
		return Err(fontoxide_ttlib::Error::ttlib(
			"--optimize-font-speed is not supported: fontoxide has no OPTIMIZE_FONT_SPEED option",
		)
		.into());
	}
	if input == "-" {
		let data = io.read_stdin()?;
		let text = String::from_utf8(data).map_err(|e| {
			fontoxide_ttlib::Error::ttx(format!("standard input is not UTF-8: {e}"))
		})?;
		let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
		font.import_ttx_split(
			text,
			None,
			&TtxImportOptions::default(),
			&mut fontoxide_ttlib::ttx::read_ttx_file,
		)?;
	} else {
		font.import_ttx_file(input)?;
	}

	if options.recalc_timestamp.is_none() && font.contains(Tag::new(b"head")) && input != "-" {
		// Use the TTX file modification time for the head "modified" timestamp.
		let mtime = std::fs::metadata(input)?
			.modified()?
			.duration_since(std::time::UNIX_EPOCH)
			.map_or(0.0, |d| d.as_secs_f64());
		let offset = fontoxide_misc::time_tools::timestamp_since_epoch(0) as f64;
		// `int(value - EPOCH_DIFF)`.
		let modified = (mtime + offset) as i64;
		font.require::<Head>()?.modified = u64::try_from(modified)
			.map_err(|_| fontoxide_ttlib::Error::Struct("argument out of range".into()))?;
	}

	// `sfnt.USE_ZOPFLI = True` for `--with-zopfli`.
	let data = font.write(&WriteOptions {
		use_zopfli: options.use_zopfli,
		..WriteOptions::default()
	})?;
	if output == "-" {
		io.stdout.write_all(&data)?;
	} else {
		std::fs::write(output, data)?;
	}
	log::debug!("Done compiling TTX in {:.3} seconds", timer.seconds());
	Ok(())
}

/// Whether fontTools raises this error as a `TTLibError`.
fn is_ttlib_error(e: &fontoxide_ttlib::Error) -> bool {
	matches!(
		e.root_cause(),
		fontoxide_ttlib::Error::TtLib(_) | fontoxide_ttlib::Error::FileIsCollection(_)
	)
}

/// The command line `ttx` (`main`), "Convert OpenType fonts to XML and back".
/// Returns the exit code: 2 for usage errors, which are reported with the help
/// on standard error, and 1 for errors while processing, which are logged.
/// Errors that fontTools does not catch either, such as a font number that is
/// not an integer, are returned.
///
/// Sets the maximum level of the `log` crate from `-v` and `-q`
/// (`configLogger`).
pub fn main(args: &[&str]) -> Result<i32> {
	with_std_io(|io| main_with_io(args, io))
}

/// [`main`] with standard streams from `io`.
pub fn main_with_io(args: &[&str], io: &mut Io<'_>) -> Result<i32> {
	let (jobs, options) = match parse_options_with_io(args, io) {
		Ok(parsed) => parsed,
		Err(Error::Getopt(e)) => {
			io.eprint(&format!("{DOC}\nERROR: {e}\n"))?;
			return Ok(2);
		}
		Err(Error::Exit { code, output }) => {
			io.print(&output)?;
			return Ok(code);
		}
		Err(e) => return Err(e),
	};

	log::set_max_level(options.log_level);

	match process_with_io(&jobs, &options, io) {
		Ok(()) => Ok(0),
		Err(Error::Exit { code, output }) => {
			io.print(&output)?;
			Ok(code)
		}
		Err(Error::TtLib(e)) if is_ttlib_error(&e) => {
			log::error!("{}", e.root_cause());
			Ok(1)
		}
		Err(e) => {
			log::error!("Unhandled exception has occurred\n{e}");
			Ok(1)
		}
	}
}
