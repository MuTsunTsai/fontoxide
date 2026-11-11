//! The command line of `fontTools.pens.svgPathPen` (`main`): SVG paths of the
//! glyphs of a text or a list of glyphs. It is here rather than in
//! `fontoxide-pens`, which does not depend on `fontoxide-ttlib`.

use std::collections::BTreeMap;
use std::io::Write;

use fontoxide_pens::svg_path_pen::{SvgPathPen, py_str};
use fontoxide_ttlib::tables::cmap::DEFAULT_CMAP_PREFERENCES;
use fontoxide_ttlib::tables::{Cmap, Hhea};
use fontoxide_ttlib::{Error, Font, GlyphSet, ReadOptions, Result, Tag};

/// The help (`-h`).
const HELP: &str = "\
usage: fonttools pens.svgPathPen [-h] [-y <number>]
                                 [--glyphs whitespace-separated list of glyph names]
                                 [--variations AXIS=LOC]
                                 font.ttf [text]

Generate SVG from text

positional arguments:
  font.ttf              Font file.
  text                  Text string.

options:
  -h, --help            show this help message and exit
  -y <number>           Face index into a collection to open. Zero based.
  --glyphs whitespace-separated list of glyph names
                        Glyphs to show. Exclusive with text option
  --variations AXIS=LOC
                        List of space separated locations. A location consist
                        in the name of a variation axis, followed by '=' and a
                        number. E.g.: wght=700 wdth=80. The default is the
                        location of the base master.
";

/// An `argparse` error.
fn usage(message: String) -> Error {
	Error::value(
		fontoxide_misc::cli_tools::program_text(
			&format!("fonttools pens.svgPathPen: error: {message}"),
			"fonttools pens.svgPathPen",
			"pens.svgPathPen",
		)
		.into_owned(),
	)
}

/// The command line `fonttools pens.svgPathPen` (`main`), "Generate
/// per-character SVG from font and text", printing to standard output.
pub fn main(args: &[&str]) -> Result<i32> {
	let stdout = std::io::stdout();
	run(args, &mut stdout.lock())
}

/// [`main`], printing to `out`.
pub fn run(args: &[&str], out: &mut dyn Write) -> Result<i32> {
	let mut positional = Vec::new();
	let mut y: Option<&str> = None;
	let mut glyphs: Option<&str> = None;
	let mut variations = "";
	let mut it = args.iter().copied();
	let mut only_positional = false;
	while let Some(arg) = it.next() {
		if only_positional || arg == "-" || !arg.starts_with('-') {
			positional.push(arg);
			continue;
		}
		let (name, inline) = match arg.split_once('=') {
			Some((name, value)) if name.starts_with("--") => (name, Some(value)),
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
				out.write_all(
					fontoxide_misc::cli_tools::program_text(
						HELP,
						"fonttools pens.svgPathPen",
						"pens.svgPathPen",
					)
					.as_bytes(),
				)?;
				return Ok(0);
			}
			"-y" => y = Some(value("-y")?),
			"--glyphs" | "--glyph" | "--glyp" | "--gly" | "--gl" | "--g" => {
				glyphs = Some(value("--glyphs")?)
			}
			"--variations" | "--variation" | "--variatio" | "--variati" | "--variat"
			| "--varia" | "--vari" | "--var" | "--va" | "--v" => variations = value("--variations")?,
			_ => return Err(usage(format!("unrecognized arguments: {arg}"))),
		}
	}
	let (font_path, text) = match positional.as_slice() {
		[] => {
			return Err(usage(
				"the following arguments are required: font.ttf".into(),
			));
		}
		[font] => (*font, None),
		[font, text] => (*font, Some(*text)),
		[_, _, rest @ ..] => {
			return Err(usage(format!("unrecognized arguments: {}", rest.join(" "))));
		}
	};

	let font_number = match y {
		Some(y) => fontoxide_misc::py::parse_int(y).ok_or_else(|| {
			Error::value(format!(
				"invalid literal for int() with base 10: {}",
				fontoxide_misc::py::str_repr(y)
			))
		})?,
		None => 0,
	};
	let mut font = Font::read(
		std::fs::read(font_path)?,
		ReadOptions {
			font_number: usize::try_from(font_number).ok(),
			..Default::default()
		},
	)?;

	let mut location = BTreeMap::new();
	for tag_v in variations.split_whitespace() {
		let fields: Vec<&str> = tag_v.split('=').collect();
		let tag = fields[0].trim();
		let v = fields
			.get(1)
			.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
		let v = fontoxide_misc::py::parse_float(v).ok_or_else(|| {
			Error::value(format!(
				"could not convert string to float: {}",
				fontoxide_misc::py::str_repr(v)
			))
		})?;
		location.insert(tag.to_owned(), v);
	}

	let upper = Tag::new(b"HHEA");
	let hhea = if font.contains(upper) {
		font.get_at::<Hhea>(upper)?
	} else {
		font.get_at::<Hhea>(Tag::new(b"hhea"))?
	}
	.ok_or(Error::TableNotFound(Tag::new(b"hhea")))?;
	let (ascent, descent) = (i64::from(hhea.ascent), i64::from(hhea.descent));

	let cmap = font
		.require::<Cmap>()?
		.get_best_cmap(&DEFAULT_CMAP_PREFERENCES)
		.cloned();

	if glyphs.is_some() && text.is_some() {
		return Err(Error::value("Options --glyphs and --text are exclusive"));
	}

	let glyph_set = GlyphSet::with_user_location(&mut font, &location)?;
	let glyphs: Vec<String> = match glyphs {
		Some(glyphs) => glyphs.split_whitespace().map(str::to_owned).collect(),
		None => {
			let cmap = cmap
				.ok_or_else(|| Error::ttlib("TypeError: 'NoneType' object is not subscriptable"))?;
			let mut names = Vec::new();
			for c in text.unwrap_or_default().chars() {
				let gid = cmap
					.get(&(c as u32))
					.ok_or_else(|| Error::ttlib(format!("KeyError: {}", c as u32)))?;
				names.push(glyph_set.glyph_order().name(*gid).into_owned());
			}
			names
		}
	};

	let mut s = String::new();
	let mut width = 0.0;
	for g in &glyphs {
		if !glyph_set.contains(g) {
			return Err(Error::GlyphNotFound(g.clone()));
		}
		let mut pen = SvgPathPen::new_svg_path(Some(&glyph_set), py_str);
		glyph_set.draw(g, &mut pen)?;
		let commands = pen.get_commands();
		// "%d" of a number truncates it.
		s.push_str(&format!(
			"<g transform=\"translate({} {ascent}) scale(1 -1)\"><path d=\"{commands}\"/></g>\n",
			width as i64
		));
		width += glyph_set.metrics(g)?.width;
	}

	writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>")?;
	writeln!(
		out,
		"<svg width=\"{}\" height=\"{}\" xmlns=\"http://www.w3.org/2000/svg\">",
		width as i64,
		ascent - descent
	)?;
	write!(out, "{s}")?;
	writeln!(out, "</svg>")?;
	Ok(0)
}
