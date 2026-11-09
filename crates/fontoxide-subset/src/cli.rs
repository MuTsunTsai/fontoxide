//! The command-line entry point of `fontTools.subset` and its helpers.

use fontoxide_misc::cli_tools::make_output_file_name;
use fontoxide_misc::py;
use fontoxide_ttlib::tables::{Cmap, Dmap};
use fontoxide_ttlib::{Flavor, Font, ReadOptions, Tag, TtxOptions, WriteOptions};

use crate::{Error, Options, Result, Subsetter};

const USAGE: &str = "fonttools subset font-file [glyph...] [--option=value]...";

fn usage() {
	eprintln!("usage: {USAGE}");
	eprintln!("Try fonttools subset --help for more information.\n");
}

fn value_error(message: String) -> Error {
	fontoxide_ttlib::Error::value(message).into()
}

/// `str.split()`.
fn split_whitespace(s: &str) -> impl Iterator<Item = &str> {
	s.split(py::is_space).filter(|item| !item.is_empty())
}

/// `int(s, 16)`, for a string without a sign or prefix.
fn parse_hex(s: &str) -> Result<u32> {
	let invalid = || value_error(format!("invalid literal for int() with base 16: '{s}'"));
	let bytes = s.as_bytes();
	let valid = !bytes.is_empty()
		&& bytes.iter().enumerate().all(|(i, &b)| {
			b.is_ascii_hexdigit()
				|| (b == b'_'
					&& i > 0 && bytes[i - 1].is_ascii_hexdigit()
					&& bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit))
		});
	if !valid {
		return Err(invalid());
	}
	u32::from_str_radix(&s.replace('_', ""), 16).map_err(|_| invalid())
}

/// `int(s)`.
fn parse_decimal(s: &str) -> Result<u32> {
	let value = py::parse_int(s)
		.ok_or_else(|| value_error(format!("invalid literal for int() with base 10: '{s}'")))?;
	u32::try_from(value).map_err(|_| value_error(format!("glyph ID out of range: {value}")))
}

/// `parse_unicodes`: code points and ranges, in the many ways they are written.
pub fn parse_unicodes(s: &str) -> Result<Vec<u32>> {
	let mut cleaned = String::with_capacity(s.len());
	let mut chars = s.chars().peekable();
	while let Some(c) = chars.next() {
		if c == '0' && matches!(chars.peek(), Some('x' | 'X')) {
			chars.next();
			cleaned.push(' ');
		} else {
			cleaned.push(c);
		}
	}
	let cleaned: String = cleaned
		.chars()
		.map(|c| {
			if "<+>,;&#\\xXuU\n\t".contains(c) {
				' '
			} else {
				c
			}
		})
		.collect();
	let mut result = Vec::new();
	for item in split_whitespace(&cleaned) {
		let fields: Vec<&str> = item.split('-').collect();
		match fields[..] {
			[value] => result.push(parse_hex(value)?),
			[start, end] => {
				let (start, end) = (parse_hex(start)?, parse_hex(end)?);
				result.extend(start..=end);
			}
			_ => {
				return Err(value_error(format!(
					"too many values to unpack (expected 2, got {})",
					fields.len()
				)));
			}
		}
	}
	Ok(result)
}

/// `parse_gids`: glyph IDs and ranges.
pub fn parse_gids(s: &str) -> Result<Vec<u32>> {
	let replaced = s.replace(',', " ");
	let mut result = Vec::new();
	for item in split_whitespace(&replaced) {
		let fields: Vec<&str> = item.split('-').collect();
		if let [value] = fields[..] {
			result.push(parse_decimal(value)?);
		} else {
			// Other fields are ignored.
			let (start, end) = (parse_decimal(fields[0])?, parse_decimal(fields[1])?);
			result.extend(start..=end);
		}
	}
	Ok(result)
}

/// `parse_glyphs`: glyph names.
pub fn parse_glyphs(s: &str) -> Vec<String> {
	split_whitespace(&s.replace(',', " "))
		.map(str::to_owned)
		.collect()
}

/// `load_font`.
pub fn load_font(
	data: Vec<u8>,
	options: &Options,
	check_checksums: u8,
	dont_load_glyph_names: bool,
) -> Result<Font> {
	let mut font = Font::read(
		data,
		ReadOptions {
			font_number: usize::try_from(options.font_number).ok(),
			check_checksums,
			recalc_bboxes: options.recalc_bounds,
			recalc_timestamp: options.recalc_timestamp,
			..Default::default()
		},
	)?;
	if dont_load_glyph_names {
		font.load_post_without_glyph_names()?;
	}
	Ok(font)
}

/// `_normalize_output_flavor`.
fn normalize_output_flavor(flavor: &str) -> Result<Option<Flavor>> {
	match flavor.to_lowercase().as_str() {
		"" | "none" => Ok(None),
		"woff" => Ok(Some(Flavor::Woff)),
		"woff2" => Ok(Some(Flavor::Woff2)),
		_ => Err(Error::Option(format!(
			"Invalid --flavor {} (use 'woff', 'woff2', or 'none')",
			py::str_repr(flavor)
		))),
	}
}

/// `save_font`: the compiled font, with the given flavor.
pub fn save_font(font: &mut Font, flavor: Option<Flavor>, options: &Options) -> Result<Vec<u8>> {
	if options.with_zopfli && flavor == Some(Flavor::Woff) {
		// LENIENT(L26): as fontTools does without the zopfli module.
		return Err(Error::Unsupported("zopfli".into()));
	}
	font.flavor = flavor;
	font.use_harfbuzz_repacker = options.harfbuzz_repacker;
	Ok(font.write(&WriteOptions {
		reorder_tables: options.canonical_order,
	})?)
}

/// The lines of a text file (`open(path).readlines()`).
///
/// LENIENT(L26): always UTF-8, where Python uses the encoding of the locale.
fn read_text(path: &str) -> Result<String> {
	let text = std::fs::read_to_string(path)?;
	Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

fn before_comment(line: &str) -> &str {
	line.split('#').next().unwrap_or_default()
}

/// The code points of the Unicode subtables, for `--unicodes=*`.
fn all_unicodes(cmap: &Cmap, unicodes: &mut Vec<u32>) {
	for t in cmap.tables.iter().filter(|t| t.is_unicode()) {
		unicodes.extend(t.cmap.keys());
		if matches!(t.format, 14 | 15)
			&& let Some(uvs) = &t.uvs_dict
		{
			unicodes.extend(uvs.keys());
		}
	}
}

/// `main`: the OpenType font subsetter and optimizer. Returns the exit code.
pub fn main(args: &[&str]) -> Result<i32> {
	if args.contains(&"--help") {
		println!("{USAGE}");
		return Ok(0);
	}
	let mut options = Options::default();
	let args = match options.parse_opts(
		args,
		&[
			"gids",
			"gids-file",
			"glyphs",
			"glyphs-file",
			"text",
			"text-file",
			"unicodes",
			"unicodes-file",
			"output-file",
		],
	) {
		Ok(args) => args,
		Err(Error::Option(message)) => {
			usage();
			eprintln!("ERROR: {message}");
			return Ok(2);
		}
		Err(e) => return Err(e),
	};
	if args.len() < 2 {
		usage();
		return Ok(1);
	}
	let fontfile = &args[0];

	let mut outfile = None;
	let mut glyphs = Vec::new();
	let mut gids = Vec::new();
	let mut unicodes = Vec::new();
	let mut wildcard_glyphs = false;
	let mut wildcard_unicodes = false;
	let mut text = String::new();
	for g in &args[1..] {
		if g == "*" {
			wildcard_glyphs = true;
		} else if let Some(v) = g.strip_prefix("--output-file=") {
			outfile = Some(v.to_owned());
		} else if let Some(v) = g.strip_prefix("--text=") {
			text.push_str(v);
		} else if let Some(v) = g.strip_prefix("--text-file=") {
			let content = read_text(v)?;
			text.push_str(
				&content
					.strip_prefix('\u{FEFF}')
					.unwrap_or(&content)
					.replace('\n', ""),
			);
		} else if let Some(v) = g.strip_prefix("--unicodes=") {
			if v == "*" {
				wildcard_unicodes = true;
			} else {
				unicodes.extend(parse_unicodes(v)?);
			}
		} else if let Some(v) = g.strip_prefix("--unicodes-file=") {
			for line in read_text(v)?.lines() {
				unicodes.extend(parse_unicodes(before_comment(line))?);
			}
		} else if let Some(v) = g.strip_prefix("--gids=") {
			gids.extend(parse_gids(v)?);
		} else if let Some(v) = g.strip_prefix("--gids-file=") {
			for line in read_text(v)?.lines() {
				gids.extend(parse_gids(before_comment(line))?);
			}
		} else if let Some(v) = g.strip_prefix("--glyphs=") {
			if v == "*" {
				wildcard_glyphs = true;
			} else {
				glyphs.extend(parse_glyphs(v));
			}
		} else if let Some(v) = g.strip_prefix("--glyphs-file=") {
			for line in read_text(v)?.lines() {
				glyphs.extend(parse_glyphs(before_comment(line)));
			}
		} else {
			glyphs.push(g.clone());
		}
	}

	let dont_load_glyph_names = !options.glyph_names && glyphs.is_empty();
	let mut font = load_font(std::fs::read(fontfile)?, &options, 0, dont_load_glyph_names)?;

	let flavor = match options.flavor.as_deref() {
		None => font.flavor,
		Some(requested) => match normalize_output_flavor(requested) {
			Ok(flavor) => flavor,
			Err(Error::Option(message)) => {
				usage();
				eprintln!("ERROR: {message}");
				return Ok(2);
			}
			Err(e) => return Err(e),
		},
	};

	let outfile = outfile.unwrap_or_else(|| {
		let extension = match flavor {
			Some(flavor) => format!(".{}", flavor.as_str()),
			None if font.sfnt_version == *b"OTTO" => ".otf".to_owned(),
			None => ".ttf".to_owned(),
		};
		make_output_file_name(fontfile, None, Some(&extension), true, ".subset")
	});

	if wildcard_glyphs {
		let order = font.glyph_order()?;
		glyphs.extend((0..order.len() as u32).map(|g| order.name(g).into_owned()));
	}
	if wildcard_unicodes {
		if font.contains(Tag::new(b"cmap")) {
			all_unicodes(font.require::<Cmap>()?, &mut unicodes);
		}
		if font.contains(Tag::new(b"DMAP")) {
			all_unicodes(&font.require::<Dmap>()?.0, &mut unicodes);
		}
	}
	if glyphs.iter().any(String::is_empty) {
		return Err(fontoxide_ttlib::Error::Assertion("\"\" not in glyphs".into()).into());
	}

	log::info!("Text: '{text}'");
	log::info!("Unicodes: {unicodes:?}");
	log::info!("Glyphs: {glyphs:?}");
	log::info!("Gids: {gids:?}");

	let xml = options.xml;
	let save_options = options.clone();
	let mut subsetter = Subsetter::new(options);
	let glyph_names: Vec<&str> = glyphs.iter().map(String::as_str).collect();
	subsetter.populate(&glyph_names, &gids, &unicodes, &text);
	subsetter.subset(&mut font)?;

	let data = save_font(&mut font, flavor, &save_options)?;
	std::fs::write(&outfile, &data)?;

	if xml {
		print!("{}", font.to_ttx(&TtxOptions::default())?);
	}
	Ok(0)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn unicodes() -> Result<()> {
		assert_eq!(parse_unicodes("41")?, [0x41]);
		assert_eq!(parse_unicodes("U+0041-0043")?, [0x41, 0x42, 0x43]);
		assert_eq!(
			parse_unicodes("0x41,0X42;u43 &#x44 <45>")?,
			[0x41, 0x42, 0x43, 0x44, 0x45]
		);
		assert_eq!(parse_unicodes("4_1")?, [0x41]);
		assert_eq!(parse_unicodes("43-41")?, []);
		assert!(parse_unicodes("1-2-3").is_err());
		assert!(parse_unicodes("-5").is_err());
		assert!(parse_unicodes("g").is_err());
		Ok(())
	}

	#[test]
	fn gids() -> Result<()> {
		assert_eq!(parse_gids("2,4-6 8")?, [2, 4, 5, 6, 8]);
		assert_eq!(parse_gids("1-2-9")?, [1, 2]);
		assert_eq!(parse_gids("+1")?, [1]);
		assert!(parse_gids("-1").is_err());
		assert!(parse_gids("x").is_err());
		Ok(())
	}

	#[test]
	fn glyphs() {
		assert_eq!(parse_glyphs("a,b  c\n"), ["a", "b", "c"]);
	}

	#[test]
	fn flavors() -> Result<()> {
		assert_eq!(normalize_output_flavor("WOFF")?, Some(Flavor::Woff));
		assert_eq!(normalize_output_flavor("woff2")?, Some(Flavor::Woff2));
		assert_eq!(normalize_output_flavor("None")?, None);
		assert_eq!(normalize_output_flavor("")?, None);
		assert!(normalize_output_flavor("ttf").is_err());
		Ok(())
	}
}
