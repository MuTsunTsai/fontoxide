//! `Options` of `fontTools.subset`, with the command-line syntax of
//! `parse_opts`.

use crate::{Error, Result};

/// An item of a list option. As in fontTools, values that start with a digit
/// are integers (in any base Python's `int(x, 0)` accepts), others strings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Item {
	#[allow(missing_docs)]
	Int(i64),
	#[allow(missing_docs)]
	Str(String),
}

impl From<&str> for Item {
	fn from(s: &str) -> Self {
		Item::Str(s.to_owned())
	}
}

impl From<i64> for Item {
	fn from(v: i64) -> Self {
		Item::Int(v)
	}
}

/// A list option.
pub type List = Vec<Item>;

/// Whether a list has a string item.
pub fn has_str(list: &[Item], s: &str) -> bool {
	list.iter().any(|i| matches!(i, Item::Str(v) if v == s))
}

/// Whether a list has an integer item.
pub fn has_int(list: &[Item], v: i64) -> bool {
	list.iter().any(|i| matches!(i, Item::Int(x) if *x == v))
}

fn strs(items: &[&str]) -> List {
	items.iter().map(|&s| Item::from(s)).collect()
}

/// Spaces in tags (`SVG `, `cvt `) are stripped by the argument parser.
const DROP_TABLES_DEFAULT: &[&str] = &[
	"JSTF", "DSIG", "EBDT", "EBLC", "EBSC", "PCLT", "LTSH", "Feat", "Glat", "Gloc", "Silf",
	"Sill",
];

const NO_SUBSET_TABLES_DEFAULT: &[&str] = &[
	"avar", "BASE", "fvar", "gasp", "head", "hhea", "HHEA", "maxp", "MAXP", "vhea", "VHEA",
	"OS/2", "loca", "LOCA", "name", "cvt", "fpgm", "prep", "VDMX", "DSIG", "CPAL", "MVAR",
	"cvar", "STAT",
];

const HINTING_TABLES_DEFAULT: &[&str] = &["cvt", "cvar", "fpgm", "prep", "hdmx", "VDMX"];

/// The layout feature groups, based on the HarfBuzz shapers
/// (`_layout_features_groups`).
pub const LAYOUT_FEATURES_GROUPS: &[(&str, &[&str])] = &[
	// Default shaper.
	("common", &["rvrn", "ccmp", "liga", "locl", "mark", "mkmk", "rlig"]),
	("fractions", &["frac", "numr", "dnom"]),
	("horizontal", &["calt", "clig", "curs", "kern", "rclt"]),
	("vertical", &["valt", "vert", "vkrn", "vpal", "vrt2"]),
	("ltr", &["ltra", "ltrm"]),
	("rtl", &["rtla", "rtlm"]),
	("rand", &["rand"]),
	("justify", &["jalt"]),
	("private", &["Harf", "HARF", "Buzz", "BUZZ"]),
	("east_asian_spacing", &["chws", "vchw", "halt", "vhal", "palt"]),
	// Complex shapers.
	(
		"arabic",
		&["init", "medi", "fina", "isol", "med2", "fin2", "fin3", "cswh", "mset", "stch"],
	),
	("hangul", &["ljmo", "vjmo", "tjmo"]),
	("tibetan", &["abvs", "blws", "abvm", "blwm"]),
	(
		"indic",
		&[
			"nukt", "akhn", "rphf", "rkrf", "pref", "blwf", "half", "abvf", "pstf", "cfar", "vatu",
			"cjct", "init", "pres", "abvs", "blws", "psts", "haln", "dist", "abvm", "blwm",
		],
	),
];

/// `_layout_features_default`: all features of the groups, sorted and unique.
fn layout_features_default() -> List {
	let mut v: Vec<&str> = LAYOUT_FEATURES_GROUPS
		.iter()
		.flat_map(|(_, f)| f.iter().copied())
		.collect();
	v.sort_unstable();
	v.dedup();
	strs(&v)
}

/// The subsetting options (`Options`). The defaults are those of fontTools.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
	/// Tables to drop.
	pub drop_tables: List,
	/// Tables that need no subsetting.
	pub no_subset_tables: List,
	/// Keep tables that cannot be subset (else they are dropped).
	pub passthrough_tables: bool,
	/// Tables dropped when not keeping hinting.
	pub hinting_tables: List,
	/// Keep the `kern` table even when there is `GPOS`.
	pub legacy_kern: bool,
	/// Close the glyph set over `GSUB`.
	pub layout_closure: bool,
	/// Layout features to keep; `*` for all.
	pub layout_features: List,
	/// Layout scripts to keep; `*` for all.
	pub layout_scripts: List,
	/// Don't fail for requested glyphs the font does not have.
	pub ignore_missing_glyphs: bool,
	/// Don't fail for requested characters the font does not map.
	pub ignore_missing_unicodes: bool,
	/// Keep hinting.
	pub hinting: bool,
	/// Keep glyph names (`post` format 2).
	pub glyph_names: bool,
	/// Keep non-Unicode `cmap` subtables.
	pub legacy_cmap: bool,
	/// Keep Symbol `cmap` subtables.
	pub symbol_cmap: bool,
	/// Name IDs to keep (`name_IDs`); `*` for all.
	pub name_ids: List,
	/// Keep non-Unicode names.
	pub name_legacy: bool,
	/// Name languages to keep; `*` for all.
	pub name_languages: List,
	/// Scramble the names, to make a web font unusable as a system font.
	pub obfuscate_names: bool,
	/// Keep glyph IDs, emptying the removed glyphs.
	pub retain_gids: bool,
	/// Keep the `.notdef` glyph (glyph 0 for TrueType).
	pub notdef_glyph: bool,
	/// Keep the outline of `.notdef`.
	pub notdef_outline: bool,
	/// Keep glyphs 1, 2 and 3 for TrueType.
	pub recommended_glyphs: bool,
	/// Recalculate bounding boxes.
	pub recalc_bounds: bool,
	/// Set the modification timestamp.
	pub recalc_timestamp: bool,
	/// Clear `ulUnicodeRange` bits of blocks without characters.
	pub prune_unicode_ranges: bool,
	/// Clear `ulCodePageRange` bits without characters.
	pub prune_codepage_ranges: bool,
	/// Recalculate `xAvgCharWidth`.
	pub recalc_average_width: bool,
	/// Recalculate `usMaxContext`.
	pub recalc_max_context: bool,
	/// Table order when saving: `Some(true)` as recommended, `Some(false)` as is,
	/// `None` (the default) unspecified.
	pub canonical_order: Option<bool>,
	/// `woff`, `woff2`, or `None` for uncompressed.
	pub flavor: Option<String>,
	/// Use zopfli for WOFF 1.0.
	pub with_zopfli: bool,
	/// Desubroutinize CFF charstrings.
	pub desubroutinize: bool,
	/// Pack `GSUB`/`GPOS` with the HarfBuzz repacker (`USE_HARFBUZZ_REPACKER`).
	pub harfbuzz_repacker: Option<bool>,
	#[allow(missing_docs)]
	pub verbose: bool,
	#[allow(missing_docs)]
	pub timing: bool,
	/// Write TTX instead of a font.
	pub xml: bool,
	/// The font of a collection; -1 for none.
	pub font_number: i64,
	/// Pretty-print SVG documents.
	pub pretty_svg: bool,
	/// Load tables lazily.
	pub lazy: bool,
	/// Add bidi-mirrored characters of the requested ones.
	pub bidi_closure: bool,
}

impl Default for Options {
	fn default() -> Self {
		Options {
			drop_tables: strs(DROP_TABLES_DEFAULT),
			no_subset_tables: strs(NO_SUBSET_TABLES_DEFAULT),
			passthrough_tables: false,
			hinting_tables: strs(HINTING_TABLES_DEFAULT),
			legacy_kern: false,
			layout_closure: true,
			layout_features: layout_features_default(),
			layout_scripts: strs(&["*"]),
			ignore_missing_glyphs: false,
			ignore_missing_unicodes: true,
			hinting: true,
			glyph_names: false,
			legacy_cmap: false,
			symbol_cmap: false,
			name_ids: (0..=6).map(Item::Int).collect(),
			name_legacy: false,
			name_languages: vec![Item::Int(0x0409)],
			obfuscate_names: false,
			retain_gids: false,
			notdef_glyph: true,
			notdef_outline: false,
			recommended_glyphs: false,
			recalc_bounds: false,
			recalc_timestamp: false,
			prune_unicode_ranges: true,
			prune_codepage_ranges: true,
			recalc_average_width: false,
			recalc_max_context: false,
			canonical_order: None,
			flavor: None,
			with_zopfli: false,
			desubroutinize: false,
			harfbuzz_repacker: None,
			verbose: false,
			timing: false,
			xml: false,
			font_number: -1,
			pretty_svg: false,
			lazy: true,
			bidi_closure: true,
		}
	}
}

/// A value given on the command line.
enum Value {
	Bool(Option<bool>),
	Str(String),
}

/// The Python type of an option, which decides how its value is converted.
enum Field<'a> {
	Bool(&'a mut bool),
	Int(&'a mut i64),
	List(&'a mut List),
	/// `None` by default: the value is stored as given.
	OptBool(&'a mut Option<bool>),
	OptStr(&'a mut Option<String>),
}

impl Options {
	fn field(&mut self, name: &str) -> Option<Field<'_>> {
		use Field::*;
		Some(match name {
			"drop_tables" => List(&mut self.drop_tables),
			"no_subset_tables" => List(&mut self.no_subset_tables),
			"passthrough_tables" => Bool(&mut self.passthrough_tables),
			"hinting_tables" => List(&mut self.hinting_tables),
			"legacy_kern" => Bool(&mut self.legacy_kern),
			"layout_closure" => Bool(&mut self.layout_closure),
			"layout_features" => List(&mut self.layout_features),
			"layout_scripts" => List(&mut self.layout_scripts),
			"ignore_missing_glyphs" => Bool(&mut self.ignore_missing_glyphs),
			"ignore_missing_unicodes" => Bool(&mut self.ignore_missing_unicodes),
			"hinting" => Bool(&mut self.hinting),
			"glyph_names" => Bool(&mut self.glyph_names),
			"legacy_cmap" => Bool(&mut self.legacy_cmap),
			"symbol_cmap" => Bool(&mut self.symbol_cmap),
			"name_IDs" => List(&mut self.name_ids),
			"name_legacy" => Bool(&mut self.name_legacy),
			"name_languages" => List(&mut self.name_languages),
			"obfuscate_names" => Bool(&mut self.obfuscate_names),
			"retain_gids" => Bool(&mut self.retain_gids),
			"notdef_glyph" => Bool(&mut self.notdef_glyph),
			"notdef_outline" => Bool(&mut self.notdef_outline),
			"recommended_glyphs" => Bool(&mut self.recommended_glyphs),
			"recalc_bounds" => Bool(&mut self.recalc_bounds),
			"recalc_timestamp" => Bool(&mut self.recalc_timestamp),
			"prune_unicode_ranges" => Bool(&mut self.prune_unicode_ranges),
			"prune_codepage_ranges" => Bool(&mut self.prune_codepage_ranges),
			"recalc_average_width" => Bool(&mut self.recalc_average_width),
			"recalc_max_context" => Bool(&mut self.recalc_max_context),
			"canonical_order" => OptBool(&mut self.canonical_order),
			"flavor" => OptStr(&mut self.flavor),
			"with_zopfli" => Bool(&mut self.with_zopfli),
			"desubroutinize" => Bool(&mut self.desubroutinize),
			"harfbuzz_repacker" => OptBool(&mut self.harfbuzz_repacker),
			"verbose" => Bool(&mut self.verbose),
			"timing" => Bool(&mut self.timing),
			"xml" => Bool(&mut self.xml),
			"font_number" => Int(&mut self.font_number),
			"pretty_svg" => Bool(&mut self.pretty_svg),
			"lazy" => Bool(&mut self.lazy),
			"bidi_closure" => Bool(&mut self.bidi_closure),
			_ => return None,
		})
	}

	/// `parse_opts`: applies `--option`, `--no-option`, `--option=value`,
	/// `--option+=values` and `--option-=values` arguments, and returns the
	/// others (positional arguments, then unknown options listed in
	/// `ignore_unknown`, or all unknown ones if it contains `"*"`).
	pub fn parse_opts(&mut self, argv: &[&str], ignore_unknown: &[&str]) -> Result<Vec<String>> {
		let mut posargs = Vec::new();
		let mut passthru = Vec::new();
		for &orig in argv {
			let Some(a) = orig.strip_prefix("--") else {
				posargs.push(orig.to_owned());
				continue;
			};
			let mut op = "=";
			let (k, v) = match a.find('=') {
				None => {
					let (k, v) = match a.strip_prefix("no-") {
						// `reorderTables=None` is faster than False.
						Some("canonical-order") => ("canonical-order", Value::Bool(None)),
						Some(k) => (k, Value::Bool(Some(false))),
						None => (a, Value::Bool(Some(true))),
					};
					if let Some(k) = k.strip_suffix('?') {
						(k.to_owned(), Value::Str("?".into()))
					} else {
						(k.to_owned(), v)
					}
				}
				Some(i) => {
					let mut k = &a[..i];
					if let Some(stripped) = k.strip_suffix(['-', '+']) {
						op = if k.ends_with('-') { "-=" } else { "+=" };
						k = stripped;
					}
					(k.to_owned(), Value::Str(a[i + 1..].to_owned()))
				}
			};
			let ok = k.clone();
			let name = k.replace('-', "_");
			let Some(field) = self.field(&name) else {
				if ignore_unknown.contains(&"*") || ignore_unknown.contains(&ok.as_str()) {
					passthru.push(orig.to_owned());
					continue;
				}
				return Err(Error::Option(format!("Unknown option '{a}'")));
			};
			if matches!(&v, Value::Str(s) if s == "?") {
				// fontTools prints the current setting.
				continue;
			}
			match field {
				Field::Bool(b) => {
					*b = match v {
						Value::Bool(b) => b.unwrap_or(false),
						Value::Str(s) => !s.is_empty(),
					}
				}
				Field::Int(i) => {
					*i = match v {
						Value::Bool(b) => i64::from(b.unwrap_or(false)),
						Value::Str(s) => s.trim().parse().map_err(|_| {
							Error::Option(format!("invalid literal for int() with base 10: '{s}'"))
						})?,
					}
				}
				Field::List(list) => {
					let Value::Str(s) = v else {
						return Err(Error::Option(format!(
							"Option '{a}' requires values to be specified using '='"
						)));
					};
					let vv = s
						.replace(',', " ")
						.split_whitespace()
						.map(parse_item)
						.collect::<Result<List>>()?;
					match op {
						"=" => *list = vv,
						"+=" => list.extend(vv),
						_ => {
							for x in vv {
								if let Some(i) = list.iter().position(|y| *y == x) {
									list.remove(i);
								}
							}
						}
					}
				}
				Field::OptBool(b) => {
					*b = match v {
						Value::Bool(b) => b,
						// Any other value is stored as is in fontTools; only booleans are
						// meaningful here.
						Value::Str(s) => Some(!s.is_empty()),
					}
				}
				Field::OptStr(o) => {
					*o = match v {
						Value::Bool(Some(true)) => Some("True".into()),
						Value::Bool(_) => None,
						Value::Str(s) => Some(s),
					}
				}
			}
		}
		posargs.extend(passthru);
		Ok(posargs)
	}
}

/// A list item: an integer if it starts with a digit (`int(x, 0)`).
fn parse_item(x: &str) -> Result<Item> {
	if !x.starts_with(|c: char| c.is_ascii_digit()) {
		return Ok(Item::Str(x.to_owned()));
	}
	let s = x.replace('_', "");
	let lower = s.to_ascii_lowercase();
	let parsed = if let Some(h) = lower.strip_prefix("0x") {
		i64::from_str_radix(h, 16)
	} else if let Some(o) = lower.strip_prefix("0o") {
		i64::from_str_radix(o, 8)
	} else if let Some(b) = lower.strip_prefix("0b") {
		i64::from_str_radix(b, 2)
	} else if lower.len() > 1 && lower.starts_with('0') && lower.bytes().any(|c| c != b'0') {
		// `int("010", 0)` is an error in Python 3.
		return Err(Error::Option(format!("invalid literal for int() with base 0: '{x}'")));
	} else {
		lower.parse()
	};
	parsed
		.map(Item::Int)
		.map_err(|_| Error::Option(format!("invalid literal for int() with base 0: '{x}'")))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn defaults() {
		let o = Options::default();
		assert!(has_str(&o.layout_features, "liga"));
		assert!(has_str(&o.layout_features, "init"));
		assert_eq!(o.layout_features.len(), 68);
		assert!(has_int(&o.name_ids, 6));
	}

	#[test]
	fn parse() -> Result<()> {
		let mut o = Options::default();
		let rest = o.parse_opts(
			&[
				"font.ttf",
				"--no-hinting",
				"--layout-features+=ss01,smcp",
				"--name-IDs=*",
				"--drop-tables-=DSIG",
				"--no-canonical-order",
				"--flavor=woff2",
				"--font-number=2",
			],
			&[],
		)?;
		assert_eq!(rest, ["font.ttf"]);
		assert!(!o.hinting);
		assert!(has_str(&o.layout_features, "ss01"));
		assert_eq!(o.name_ids, [Item::from("*")]);
		assert!(!has_str(&o.drop_tables, "DSIG"));
		assert_eq!(o.canonical_order, None);
		assert_eq!(o.flavor.as_deref(), Some("woff2"));
		assert_eq!(o.font_number, 2);
		assert!(o.parse_opts(&["--bogus"], &[]).is_err());
		assert_eq!(o.parse_opts(&["--bogus"], &["*"])?, ["--bogus"]);
		Ok(())
	}
}
