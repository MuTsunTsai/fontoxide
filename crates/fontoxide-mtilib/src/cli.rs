//! `fontTools.mtiLib.main`: the `fonttools mtiLib` command line, which
//! converts FontDame OTL files to TTX.

use std::io::Write;

use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_ttlib::{Font, ReadOptions};

use crate::BuildOptions;
use crate::argparse::{self, Kind, Spec, Ty};

/// The usage message.
pub const USAGE: &str =
	"usage: fonttools mtiLib [-h] [--font FILE] [--table TABLE] FILE [FILE ...]";

/// The help message (`-h`), as argparse formats it for 80 columns.
pub const HELP: &str = "\
usage: fonttools mtiLib [-h] [--font FILE] [--table TABLE] FILE [FILE ...]

Convert a FontDame OTL file to TTX XML Writes XML output to stdout. Args:
args: Command line arguments (``--font``, ``--table``, input files).

positional arguments:
  FILE               Input FontDame .txt files

options:
  -h, --help         show this help message and exit
  --font, -f FILE    Input TTF files (used for glyph classes and sorting
                     coverage tables)
  --table, -t TABLE  Table to fill (sniffed from input file if not provided)
";

const SPECS: &[Spec] = &[
	Spec {
		short: Some("-h"),
		long: "--help",
		kind: Kind::Flag,
		group: None,
		ty: Ty::Str,
	},
	Spec {
		short: Some("-f"),
		long: "--font",
		kind: Kind::Value,
		group: None,
		ty: Ty::Str,
	},
	Spec {
		short: Some("-t"),
		long: "--table",
		kind: Kind::Value,
		group: None,
		ty: Ty::Str,
	},
];

/// An error of the command line.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// Bad arguments: the usage and the message, which argparse prints before
	/// exiting with status 2.
	#[error("{0}")]
	Usage(String),
	/// An error building a table.
	#[error(transparent)]
	Mti(#[from] crate::Error),
	/// An error reading the font or compiling a table.
	#[error(transparent)]
	TtLib(#[from] fontoxide_ttlib::Error),
	/// An error reading a file or writing the output.
	#[error(transparent)]
	Io(#[from] std::io::Error),
}

fn usage(message: impl std::fmt::Display) -> Error {
	Error::Usage(format!("{USAGE}\nfonttools mtiLib: error: {message}"))
}

/// Converts FontDame OTL files to TTX on standard output (`main`), with the
/// arguments of `fonttools mtiLib`. Returns the exit code; a usage error is
/// [`Error::Usage`] (exit status 2 in fontTools).
pub fn main(args: &[&str]) -> Result<i32, Error> {
	let stdout = std::io::stdout();
	let mut out = stdout.lock();
	main_with(args, None, &mut out)
}

/// [`main`] with the font to use (`main(args, font)`), writing to `out`.
///
/// Without a font and without `--font`, glyphs get IDs in the order they are
/// first seen, as with fontTools' `MockFont`.
pub fn main_with(
	args: &[&str],
	font: Option<&mut Font>,
	out: &mut dyn Write,
) -> Result<i32, Error> {
	let parsed = argparse::parse(args, SPECS, "FILE").map_err(usage)?;
	if parsed.flag("--help") {
		out.write_all(HELP.as_bytes())?;
		return Ok(0);
	}
	let table_tag = parsed.value("--table").and_then(|s| s.value.clone());
	let font_path = parsed.value("--font").and_then(|s| s.value.clone());

	let mut own_font;
	let (font, add_glyphs) = match (font, font_path) {
		(Some(font), _) => (font, false),
		(None, Some(path)) => {
			own_font = Font::read(std::fs::read(path)?, ReadOptions::default())?;
			(&mut own_font, false)
		}
		// LENIENT(L43): fontTools uses a `MockFont`, with which compiling the
		// table fails (it has no `cfg`).
		(None, None) => {
			own_font = Font::new();
			(&mut own_font, true)
		}
	};
	let options = BuildOptions {
		table_tag,
		add_glyphs,
		..BuildOptions::default()
	};

	for path in &parsed.positional {
		log::debug!("Processing {path}");
		let data = std::fs::read(path)?;
		let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&data);
		let text = String::from_utf8(data.to_vec()).map_err(|e| {
			crate::Error::Value(format!(
				"UnicodeDecodeError: 'utf-8' codec can't decode: {e}"
			))
		})?;
		let mut table = crate::build_with(&text, font, &options)?;
		let tag = table.tag();
		// Make sure it compiles.
		let blob = table.compile(font)?;
		// Make sure it decompiles!
		let decompiled = fontoxide_ttlib::table::decompile_table(tag, &blob, font)?;

		let mut writer = XmlWriter::new();
		let name = tag.as_string();
		writer.begintag(&name, &[]);
		writer.newline();
		decompiled.to_xml(&mut writer, font)?;
		writer.endtag(&name);
		writer.newline();
		out.write_all(writer.into_string().as_bytes())?;
	}
	Ok(0)
}
