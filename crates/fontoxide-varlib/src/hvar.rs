//! Port of `fontTools.varLib.hvar`: adds `HVAR` or `VVAR` to a variable font from
//! the phantom points of its `gvar`; and its command line (`main`).

use std::io::Write;

use fontoxide_misc::argparse::{Argument, Command};
use fontoxide_misc::cli_tools::make_output_file_name;
use fontoxide_ttlib::tables::tuple_variation::Delta;
use fontoxide_ttlib::tables::{Fvar, Gvar};
use fontoxide_ttlib::{Font, ReadOptions, Tag, WriteOptions};

use crate::build::{DeltasAndSupports, MetricsFields, add_vhvar};
use crate::errors::Result;

/// `_get_advance_metrics`: the deltas of the advances from the phantom points of
/// the glyph variations, and their supports.
fn get_advance_metrics(font: &mut Font, fields: MetricsFields) -> Result<Vec<DeltasAndSupports>> {
	let order = font.glyph_order()?;
	let gvar_tag = if font.contains(Tag::new(b"GVAR")) {
		Tag::new(b"GVAR")
	} else {
		Tag::new(b"gvar")
	};
	let gvar = font
		.get_at::<Gvar>(gvar_tag)?
		.ok_or_else(|| fontoxide_ttlib::Error::ttlib("KeyError: 'gvar'"))?;
	let phantom_index = match fields {
		MetricsFields::Hvar => 0,
		MetricsFields::Vvar => 1,
	};
	let mut out = Vec::with_capacity(order.len());
	for gid in 0..order.len() as u32 {
		let mut supports = Vec::new();
		let mut deltas = Vec::new();
		for tv in gvar.get(gid).unwrap_or_default() {
			supports.push(tv.axes.iter().map(|(k, v)| (k.clone(), *v)).collect());
			let n = tv.coordinates.len();
			if n < 4 {
				return Err(fontoxide_ttlib::Error::Assertion("len(phantoms) == 2".into()).into());
			}
			let phantoms = &tv.coordinates[n - 4..];
			let value = |d: &Option<Delta>| match d {
				Some(Delta::Point(x, y)) => {
					if phantom_index == 0 {
						x.value()
					} else {
						y.value()
					}
				}
				Some(Delta::Value(v)) => v.value(),
				None => 0.0,
			};
			let first = value(&phantoms[phantom_index * 2]);
			let second = value(&phantoms[phantom_index * 2 + 1]);
			deltas.push(second - first);
		}
		out.push((deltas, supports, Vec::new()));
	}
	Ok(out)
}

fn add(font: &mut Font, fields: MetricsFields, tag: &[u8; 4]) -> Result<()> {
	font.remove(Tag::new(tag));
	let axis_tags: Vec<String> = font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| Tag::new(&a.axis_tag).to_string())
		.collect();
	let advances = get_advance_metrics(font, fields)?;
	// Each glyph has a supports list of its own, so `allEqual(id(...))` holds only
	// for a single glyph.
	// LENIENT(L36): fontTools fails with an IndexError on a glyph without variations
	// (`setSupports([])`); its deltas are stored here.
	let single = advances.len() <= 1;
	add_vhvar(font, &axis_tags, fields, &advances, None, Some(single))
}

/// `add_HVAR`: replaces the `HVAR` of a variable font with one built from the
/// `gvar` phantom points.
pub fn add_hvar(font: &mut Font) -> Result<()> {
	add(font, MetricsFields::Hvar, b"HVAR")
}

/// `add_VVAR`: replaces the `VVAR` of a variable font with one built from the
/// `gvar` phantom points.
pub fn add_vvar(font: &mut Font) -> Result<()> {
	add(font, MetricsFields::Vvar, b"VVAR")
}

const HELP: &str = "usage: fonttools varLib.hvar [-h] [-o OUTPUT_FILE] varfont.ttf

Add `HVAR` table from to variable font.

positional arguments:
  varfont.ttf           Variable-font file.

options:
  -h, --help            show this help message and exit
  -o, --output-file OUTPUT_FILE
                        Output font file name.
";

const COMMAND: Command = Command {
	prog: "fonttools varLib.hvar",
	module: "varLib.hvar",
	help: HELP,
	arguments: &[
		Argument::help(),
		Argument::positional("varfont.ttf"),
		Argument::option(&["-o", "--output-file"]),
	],
	required_groups: &[],
};

/// `main`: adds `HVAR` (and `VVAR`, if the font has `vmtx`) to a variable font,
/// saving it to `-o` or `<font>.hvar.<ext>`. Returns the exit code: 1 if the
/// font is not variable.
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
	let font_path = options.str("varfont.ttf").unwrap_or_default();
	let mut font = Font::read(std::fs::read(font_path)?, ReadOptions::default())?;
	if !font.contains(Tag::new(b"fvar")) {
		log::error!("Not a variable font.");
		return Ok(1);
	}

	add_hvar(&mut font)?;
	if font.contains(Tag::new(b"vmtx")) || font.contains(Tag::new(b"VMTX")) {
		add_vvar(&mut font)?;
	}

	let outfile = match options.str("--output-file") {
		None => make_output_file_name(font_path, None, None, true, ".hvar"),
		Some(outfile) => outfile.to_owned(),
	};
	if !outfile.is_empty() {
		log::info!("Saving {outfile}");
		std::fs::write(&outfile, font.write(&WriteOptions::default())?)?;
	}
	Ok(0)
}
