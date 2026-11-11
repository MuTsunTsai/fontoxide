//! The command line of `fontTools.varLib.interpolatable` (`main`).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Write;

use fontoxide_designspacelib::{Axis, DesignSpaceDocument};
use fontoxide_misc::fixed_tools::float_to_fixed_to_str;
use fontoxide_misc::py;
use fontoxide_pens::Pen;
use fontoxide_ttlib::cff::charstring::Token;
use fontoxide_ttlib::cff::font::PrivateRef;
use fontoxide_ttlib::tables::{Avar, Cff2, Fvar, Glyf, Gvar, Head};
use fontoxide_ttlib::{Font, GlyphSet as TtGlyphSet, ReadOptions, TableType, Tag};
use fontoxide_ufolib::{UfoReader, Value};
use indexmap::IndexSet;

use super::helpers::Location;
use super::{
	DEFAULT_KINKINESS, DEFAULT_TOLERANCE, DEFAULT_UPEM, InterpolatableError,
	InterpolatableGlyphSet, Problem, Problems, Result, TestOptions, problems_to_json,
	sort_problems, test_gen,
};
use crate::models::{normalize_value_decreasing, piecewise_linear_map};

const HELP: &str = "\
usage: fonttools varLib.interpolatable [-h] [--glyphs GLYPHS] [--show-all]
                                       [--tolerance TOLERANCE]
                                       [--kinkiness KINKINESS] [--json]
                                       [--pdf PDF] [--ps PS] [--html HTML]
                                       [--quiet] [--output OUTPUT]
                                       [--ignore-missing] [--name NAME] [-v]
                                       [--debug]
                                       FILE [FILE ...]

Test for interpolatability issues between fonts

positional arguments:
  FILE                  Input a single variable font / DesignSpace / Glyphs
                        file, or multiple TTF/UFO files

options:
  -h, --help            show this help message and exit
  --glyphs GLYPHS       Space-separate name of glyphs to check
  --show-all            Show all glyph pairs, even if no problems are found
  --tolerance TOLERANCE
                        Error tolerance. Between 0 and 1. Default 0.95
  --kinkiness KINKINESS
                        How aggressively report kinks. Default 0.5
  --json                Output report in JSON format
  --pdf PDF             Output report in PDF format
  --ps PS               Output report in PostScript format
  --html HTML           Output report in HTML format
  --quiet               Only exit with code 1 or 0, no output
  --output OUTPUT       Output file for the problem report; Default: stdout
  --ignore-missing      Will not report glyphs missing from sparse masters as
                        errors
  --name NAME           Name of the master to use in the report. If not
                        provided, all are used.
  -v, --verbose         Run verbosely.
  --debug               Run with debug output.
";

/// The parsed command line.
#[derive(Debug, Default)]
struct Args {
	glyphs: Option<String>,
	show_all: bool,
	tolerance: Option<f64>,
	kinkiness: Option<f64>,
	json: bool,
	pdf: Option<String>,
	ps: Option<String>,
	html: Option<String>,
	quiet: bool,
	output: Option<String>,
	ignore_missing: bool,
	inputs: Vec<String>,
	name: Option<Vec<String>>,
	help: bool,
}

/// The long options, and whether they take a value.
const LONG_OPTIONS: &[(&str, bool)] = &[
	("--help", false),
	("--glyphs", true),
	("--show-all", false),
	("--tolerance", true),
	("--kinkiness", true),
	("--json", false),
	("--pdf", true),
	("--ps", true),
	("--html", true),
	("--quiet", false),
	("--output", true),
	("--ignore-missing", false),
	("--name", true),
	("--verbose", false),
	("--debug", false),
];

fn usage(message: String) -> InterpolatableError {
	InterpolatableError::Usage(format!(
		"usage: fonttools varLib.interpolatable [-h] ... FILE [FILE ...]\nfonttools varLib.interpolatable: error: {message}"
	))
}

/// Whether `argparse` takes an argument as a negative number rather than an
/// option.
fn is_negative_number(arg: &str) -> bool {
	// `^-\d+$|^-\d*\.\d+$`
	let Some(s) = arg.strip_prefix('-') else {
		return false;
	};
	let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
	match s.split_once('.') {
		None => !s.is_empty() && digits(s),
		Some((a, b)) => digits(a) && !b.is_empty() && digits(b),
	}
}

/// Parses the command line as `argparse` does: long options may be
/// abbreviated, and the input files come in one run.
fn parse_args(args: &[&str]) -> Result<Args> {
	let mut parsed = Args::default();
	let mut positional_runs: Vec<Vec<String>> = Vec::new();
	let mut in_run = false;
	let mut only_positional = false;
	let mut i = 0;
	while i < args.len() {
		let arg = args[i];
		i += 1;
		if only_positional || arg == "-" || !arg.starts_with('-') || is_negative_number(arg) {
			if !in_run {
				positional_runs.push(Vec::new());
				in_run = true;
			}
			if let Some(run) = positional_runs.last_mut() {
				run.push(arg.to_owned());
			}
			continue;
		}
		in_run = false;
		if arg == "--" {
			only_positional = true;
			continue;
		}
		let (option, inline_value) = match arg.split_once('=') {
			Some((o, v)) if o.starts_with("--") => (o, Some(v.to_owned())),
			_ => (arg, None),
		};
		let (name, takes_value) = match option {
			"-h" => ("--help", false),
			"-v" => ("--verbose", false),
			_ if option.starts_with("--") => {
				let exact = LONG_OPTIONS.iter().find(|(n, _)| *n == option);
				let candidates: Vec<&(&str, bool)> = match exact {
					Some(e) => vec![e],
					None => LONG_OPTIONS
						.iter()
						.filter(|(n, _)| n.starts_with(option))
						.collect(),
				};
				match candidates[..] {
					[&(n, v)] => (n, v),
					[] => return Err(usage(format!("unrecognized arguments: {arg}"))),
					_ => {
						let names: Vec<&str> = candidates.iter().map(|(n, _)| *n).collect();
						return Err(usage(format!(
							"ambiguous option: {option} could match {}",
							names.join(", ")
						)));
					}
				}
			}
			_ => return Err(usage(format!("unrecognized arguments: {arg}"))),
		};
		let value = if takes_value {
			match inline_value {
				Some(v) => Some(v),
				None => match args.get(i) {
					Some(v) if !v.starts_with('-') || *v == "-" || is_negative_number(v) => {
						i += 1;
						Some((*v).to_owned())
					}
					_ => {
						let metavar = name.trim_start_matches('-').to_uppercase();
						return Err(usage(format!(
							"argument {name} {metavar}: expected one argument"
						)));
					}
				},
			}
		} else {
			if inline_value.is_some() {
				return Err(usage(format!("argument {name}: ignored explicit argument")));
			}
			None
		};
		let float = |v: Option<String>| -> Result<Option<f64>> {
			let v = v.unwrap_or_default();
			match py::parse_float(&v) {
				Some(f) => Ok(Some(f)),
				None => Err(usage(format!(
					"argument {name} {}: invalid float value: {}",
					name.trim_start_matches('-').to_uppercase(),
					py::str_repr(&v)
				))),
			}
		};
		match name {
			"--help" => parsed.help = true,
			"--glyphs" => parsed.glyphs = value,
			"--show-all" => parsed.show_all = true,
			"--tolerance" => parsed.tolerance = float(value)?,
			"--kinkiness" => parsed.kinkiness = float(value)?,
			"--json" => parsed.json = true,
			"--pdf" => parsed.pdf = value,
			"--ps" => parsed.ps = value,
			"--html" => parsed.html = value,
			"--quiet" => parsed.quiet = true,
			"--output" => parsed.output = value,
			"--ignore-missing" => parsed.ignore_missing = true,
			"--name" => parsed.name.get_or_insert_with(Vec::new).extend(value),
			// Logging is up to the caller.
			_ => {}
		}
	}
	if parsed.help {
		return Ok(parsed);
	}
	let mut runs = positional_runs.into_iter();
	match runs.next() {
		Some(run) => parsed.inputs = run,
		None => return Err(usage("the following arguments are required: FILE".into())),
	}
	let extra: Vec<String> = runs.flatten().collect();
	if !extra.is_empty() {
		return Err(usage(format!(
			"unrecognized arguments: {}",
			extra.join(" ")
		)));
	}
	Ok(parsed)
}

/// `os.path.basename` (of `ntpath`, as on Windows).
fn basename(path: &str) -> &str {
	let path = match path.as_bytes() {
		[d, b':', ..] if d.is_ascii_alphabetic() => &path[2..],
		_ => path,
	};
	path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// A Python `dict` built from pairs: the first position of each key, with the
/// last value.
fn dict_from_pairs(pairs: impl IntoIterator<Item = (f64, f64)>) -> Vec<(f64, f64)> {
	let mut out: Vec<(f64, f64)> = Vec::new();
	for (k, v) in pairs {
		dict_set(&mut out, k, v);
	}
	out
}

/// `d[k] = v` on a `dict` of floats.
fn dict_set(dict: &mut Vec<(f64, f64)>, k: f64, v: f64) {
	match dict.iter_mut().find(|(key, _)| *key == k) {
		Some(entry) => entry.1 = v,
		None => dict.push((k, v)),
	}
}

/// A glyph set of a variable font at a location (`font.getGlyphSet(location=...,
/// normalized=True)`), sharing the glyph set of the default location.
struct LocatedGlyphSet<'a, 'f> {
	glyph_set: &'a TtGlyphSet<'f>,
	location: BTreeMap<String, f64>,
}

impl fontoxide_pens::GlyphSet for LocatedGlyphSet<'_, '_> {
	fn draw_glyph(&self, glyph_name: &str, pen: &mut dyn Pen) -> fontoxide_pens::Result<bool> {
		self.glyph_set.push_location(&self.location, true, || {
			self.glyph_set.draw_glyph(glyph_name, pen)
		})
	}

	fn contains(&self, glyph_name: &str) -> bool {
		fontoxide_pens::GlyphSet::contains(self.glyph_set, glyph_name)
	}
}

impl InterpolatableGlyphSet for LocatedGlyphSet<'_, '_> {
	fn glyph_names(&self) -> Vec<String> {
		self.glyph_set.keys().to_vec()
	}
}

/// The glyph set of a master as `main` passes it to `test_gen`: a `dict` of
/// some glyphs of a glyph set; the others are missing (`None`).
struct MasterGlyphSet<'a> {
	inner: &'a dyn InterpolatableGlyphSet,
	keys: Vec<String>,
	present: HashSet<String>,
}

impl<'a> MasterGlyphSet<'a> {
	fn new(inner: &'a dyn InterpolatableGlyphSet, keys: Vec<String>) -> Self {
		let present = keys.iter().cloned().collect();
		MasterGlyphSet {
			inner,
			keys,
			present,
		}
	}
}

impl fontoxide_pens::GlyphSet for MasterGlyphSet<'_> {
	/// LENIENT(L40): a component that is missing from a sparse master (`None`) is
	/// skipped with a warning, as one that is not in the master at all;
	/// fontTools fails with an `AttributeError`.
	fn draw_glyph(&self, glyph_name: &str, pen: &mut dyn Pen) -> fontoxide_pens::Result<bool> {
		if !self.present.contains(glyph_name) {
			return Ok(false);
		}
		self.inner.draw_glyph(glyph_name, pen)
	}

	fn contains(&self, glyph_name: &str) -> bool {
		self.present.contains(glyph_name)
	}
}

impl InterpolatableGlyphSet for MasterGlyphSet<'_> {
	fn glyph_names(&self) -> Vec<String> {
		self.keys.clone()
	}

	fn draw_outline(&self, glyph_name: &str, pen: &mut dyn Pen) -> fontoxide_pens::Result<bool> {
		if !self.present.contains(glyph_name) {
			return Ok(false);
		}
		self.inner.draw_outline(glyph_name, pen)
	}
}

/// The masters of a variable font: their names and normalized locations, and
/// the glyphs that vary at each location.
struct VariableFont {
	upem: f64,
	names: Vec<String>,
	locations: Vec<Location>,
	/// The location and the glyphs of each master after the default one.
	masters: Vec<(BTreeMap<String, f64>, IndexSet<String>)>,
	axis_tags: Vec<String>,
}

fn key_error(key: &str) -> InterpolatableError {
	InterpolatableError::Type(format!("KeyError: {}", py::str_repr(key)))
}

/// A location tuple (`locTuple`) and the glyphs of the master there.
type LocatedGlyphs = (Vec<(String, f64)>, IndexSet<String>);

/// The single variable font branch of `main`.
fn read_variable_font(font: &mut Font, glyphs: &mut Option<Vec<String>>) -> Result<VariableFont> {
	let upem = f64::from(font.require::<Head>()?.units_per_em);

	if !font.contains(Fvar::TAG) {
		return Err(key_error("fvar"));
	}
	let fvar_axes: Vec<(String, f64, f64, f64)> = font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| (a.tag_text(), a.min_value, a.default_value, a.max_value))
		.collect();
	// `axisMapping`: normalized to user values, as `dict`s.
	let mut axis_mapping: Vec<(String, Vec<(f64, f64)>)> = Vec::new();
	for (tag, min, default, max) in &fvar_axes {
		let mapping = dict_from_pairs([(-1.0, *min), (0.0, *default), (1.0, *max)]);
		match axis_mapping.iter_mut().find(|(t, _)| t == tag) {
			Some(entry) => entry.1 = mapping,
			None => axis_mapping.push((tag.clone(), mapping)),
		}
	}
	let mut normalized = false;
	if font.contains(Avar::TAG) {
		let avar = font.require::<Avar>()?;
		if avar.table.var_store.is_some() {
			for (_, mapping) in &mut axis_mapping {
				*mapping = dict_from_pairs([(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)]);
			}
			normalized = true;
		} else {
			for (tag, segments) in &avar.segments {
				let tag = tag.as_string();
				let Some((_, mapping)) = axis_mapping.iter_mut().find(|(t, _)| *t == tag) else {
					return Err(key_error(&tag));
				};
				let fvar_mapping = mapping.clone();
				for &(location, value) in segments {
					dict_set(
						mapping,
						value,
						piecewise_linear_map(location, &fvar_mapping),
					);
				}
			}
		}
	}

	// Gather all glyphs at their "master" locations
	let order = font.glyph_order()?;
	// The masters by location tuple, with their glyphs.
	let mut glyph_sets: Vec<LocatedGlyphs> = Vec::new();
	fn add(glyph_sets: &mut Vec<LocatedGlyphs>, loc: Vec<(String, f64)>) -> usize {
		match glyph_sets.iter().position(|(l, _)| *l == loc) {
			Some(i) => i,
			None => {
				glyph_sets.push((loc, IndexSet::new()));
				glyph_sets.len() - 1
			}
		}
	}
	let gvar_tag = Tag::new(b"GVAR");
	let glyf_upper = Tag::new(b"GLYF");
	if font.contains(Gvar::TAG) || font.contains(gvar_tag) {
		let (gvar_tag, glyf_tag) = if font.contains(gvar_tag) {
			(gvar_tag, glyf_upper)
		} else {
			(Gvar::TAG, Glyf::TAG)
		};
		// The location tuples of each glyph's variations.
		let gvar = font
			.get_at::<Gvar>(gvar_tag)?
			.ok_or_else(|| key_error(&gvar_tag.as_string()))?;
		let mut variations: Vec<Vec<Vec<(String, f64)>>> = vec![Vec::new(); order.len()];
		for (gid, vars) in &gvar.variations {
			if let Some(slot) = variations.get_mut(*gid as usize) {
				*slot = vars
					.iter()
					.map(|var| {
						// `sorted(var.axes.items())`
						var.axes
							.iter()
							.map(|(tag, region)| (tag.clone(), region.1))
							.collect()
					})
					.collect();
			}
		}
		let glyf = font
			.get_at::<Glyf>(glyf_tag)?
			.ok_or_else(|| key_error(&glyf_tag.as_string()))?;
		let components = |name: &str| -> Vec<String> {
			order
				.id(name)
				.and_then(|gid| glyf.glyphs.get(&gid))
				.map(|g| {
					g.components
						.iter()
						.map(|c| order.name(c.glyph).into_owned())
						.collect()
				})
				.unwrap_or_default()
		};
		if glyphs.is_none() {
			let mut names = order.names().to_vec();
			names.sort();
			*glyphs = Some(names);
		}
		for glyph_name in glyphs.iter().flatten() {
			let gid = order.id(glyph_name).ok_or_else(|| key_error(glyph_name))?;
			for loc in &variations[gid as usize] {
				let i = add(&mut glyph_sets, loc.clone());
				let set = &mut glyph_sets[i].1;
				// `recursivelyAddGlyph`
				let mut stack = vec![glyph_name.clone()];
				while let Some(name) = stack.pop() {
					if set.contains(&name) {
						continue;
					}
					if order.id(&name).is_none() {
						return Err(key_error(&name));
					}
					set.insert(name.clone());
					// Depth first, in the order of the components.
					let mut comps = components(&name);
					comps.reverse();
					stack.extend(comps);
				}
			}
		}
	} else if font.contains(Cff2::TAG) {
		let fvar_tags: Vec<String> = fvar_axes.iter().map(|a| a.0.clone()).collect();
		let cff2 = font.require::<Cff2>()?;
		let top = cff2.cff.top_dicts.first().ok_or_else(|| {
			InterpolatableError::Type("IndexError: list index out of range".into())
		})?;
		let char_strings = top
			.char_strings
			.as_ref()
			.ok_or_else(|| InterpolatableError::Type("AttributeError: CharStrings".into()))?;
		if glyphs.is_none() {
			let mut names = char_strings.names.clone();
			names.sort();
			*glyphs = Some(names);
		}
		for glyph_name in glyphs.iter().flatten() {
			let index = char_strings
				.index(glyph_name)
				.ok_or_else(|| key_error(glyph_name))?;
			let cs = &char_strings.items[index];
			let private = match char_strings.privates.get(index) {
				Some(PrivateRef::Top) => top.private.as_deref(),
				Some(PrivateRef::Fd(fd)) => top
					.fd_array
					.as_ref()
					.and_then(|a| a.get(*fd))
					.and_then(|f| f.private.as_deref()),
				_ => None,
			};
			let private_vsindex = private
				.and_then(|p| p.get("vsindex"))
				.and_then(|v| v.number())
				.unwrap_or(0.0) as i64;

			// Extract vsindex for the glyph
			let mut vsindices = BTreeSet::from([private_vsindex]);
			let mut vsindex = private_vsindex;
			let mut last_op: Option<i64> = Some(0);
			// The spec says vsindex can only appear once and must be the first
			// operator in the charstring, but we support multiple.
			for token in &cs.program {
				match token {
					Token::Op("blend") => {
						vsindices.insert(vsindex);
					}
					Token::Op("vsindex") => {
						vsindex = last_op.ok_or_else(|| {
							InterpolatableError::Assertion("isinstance(last_op, int)".into())
						})?;
					}
					_ => {}
				}
				last_op = match token {
					Token::Int(v) => Some(*v),
					_ => None,
				};
			}

			// LENIENT(L40): a CFF2 font without a variation store fails with an
			// `AttributeError` in fontTools; its glyphs are skipped here.
			let Some(var_store) = top.var_store.as_deref() else {
				continue;
			};
			for vsindex in vsindices {
				let var_data = usize::try_from(vsindex)
					.ok()
					.and_then(|i| var_store.var_data.get(i))
					.and_then(|d| d.as_deref())
					.ok_or_else(|| {
						InterpolatableError::Type("IndexError: list index out of range".into())
					})?;
				for &region_index in &var_data.var_region_index {
					let region = var_store
						.var_region_list
						.as_ref()
						.and_then(|l| l.region.get(usize::from(region_index)))
						.ok_or_else(|| {
							InterpolatableError::Type("IndexError: list index out of range".into())
						})?;
					let mut loc = Vec::new();
					for (axis_index, axis) in region.var_region_axis.iter().enumerate() {
						let tag = fvar_tags.get(axis_index).ok_or_else(|| {
							InterpolatableError::Type("IndexError: list index out of range".into())
						})?;
						loc.push((tag.clone(), axis.peak_coord));
					}
					let i = add(&mut glyph_sets, loc);
					glyph_sets[i].1.insert(glyph_name.clone());
				}
			}
		}
	}
	let mut sets = glyph_sets;

	let mut names = vec!["''".to_owned()];
	let mut locations: Vec<Location> = vec![Vec::new()];
	let mut axis_tags: Vec<String> = axis_mapping.iter().map(|(t, _)| t.clone()).collect();
	axis_tags.sort();
	// `sorted(glyphsets.keys(), key=lambda v: (len(v), v))`
	sets.sort_by(|(a, _), (b, _)| {
		a.len().cmp(&b.len()).then_with(|| {
			for ((ta, va), (tb, vb)) in a.iter().zip(b) {
				let c = ta
					.cmp(tb)
					.then(va.partial_cmp(vb).unwrap_or(std::cmp::Ordering::Equal));
				if c != std::cmp::Ordering::Equal {
					return c;
				}
			}
			std::cmp::Ordering::Equal
		})
	});
	let mut masters = Vec::new();
	for (loc, glyph_names) in sets {
		let parts: Vec<String> = loc
			.iter()
			.map(|(k, v)| {
				let mapping = axis_mapping
					.iter()
					.find(|(t, _)| t == k)
					.map(|(_, m)| m.as_slice())
					.ok_or_else(|| key_error(k))?;
				Ok(format!(
					"{k}={}",
					float_to_fixed_to_str(piecewise_linear_map(*v, mapping), 14)
				))
			})
			.collect::<Result<_>>()?;
		let mut name = format!("'{}'", parts.join(" "));
		if normalized {
			name.push_str(" (normalized)");
		}
		names.push(name);
		locations.push(dict_from_location(&loc));
		masters.push((loc.into_iter().collect(), glyph_names));
	}
	Ok(VariableFont {
		upem,
		names,
		locations,
		masters,
		axis_tags,
	})
}

/// `dict(locTuple)`.
fn dict_from_location(loc: &[(String, f64)]) -> Location {
	let mut out: Location = Vec::new();
	for (k, v) in loc {
		match out.iter_mut().find(|(key, _)| key == k) {
			Some(entry) => entry.1 = *v,
			None => out.push((k.clone(), *v)),
		}
	}
	out
}

/// `ensure_parent_dir(path)`.
fn ensure_parent_dir(path: &str) -> Result<&str> {
	if let Some(parent) = std::path::Path::new(path).parent()
		&& !parent.as_os_str().is_empty()
	{
		std::fs::create_dir_all(parent)?;
	}
	Ok(path)
}

/// The number of units per em of a UFO's font info (`info.unitsPerEm`).
fn ufo_upem(reader: &UfoReader) -> Result<f64> {
	let info = reader.read_info(None)?;
	match info.get("unitsPerEm") {
		Some(Value::Integer(i)) => Ok(*i as f64),
		Some(Value::Real(f)) => Ok(*f),
		_ => Err(InterpolatableError::Type(
			"AttributeError: 'types.SimpleNamespace' object has no attribute 'unitsPerEm'".into(),
		)),
	}
}

/// `fonttools varLib.interpolatable`: checks the fonts given on the command
/// line and prints the problems found to standard output. Returns the exit
/// code: 1 if there are problems.
pub fn main(args: &[&str]) -> Result<i32> {
	let stdout = std::io::stdout();
	let mut out = stdout.lock();
	let problems = run(args, &mut out)?;
	Ok(i32::from(problems.is_some()))
}

/// `main(args)`: checks the fonts given on the command line, writes the report
/// to `out` (or to `--output`), and returns the problems, sorted by
/// [`sort_problems`], or `None` if there are none.
///
/// LENIENT(L40): the PDF, PostScript and HTML reports, and `.glyphs` sources, are
/// not supported.
pub fn run(args: &[&str], out: &mut dyn Write) -> Result<Option<Problems>> {
	let args = parse_args(args)?;
	if args.help {
		out.write_all(HELP.as_bytes())?;
		return Ok(None);
	}
	for (option, value) in [
		("--pdf", &args.pdf),
		("--ps", &args.ps),
		("--html", &args.html),
	] {
		if value.is_some() {
			return Err(InterpolatableError::Unsupported(format!(
				"{option}: the PDF, PostScript and HTML reports are not supported"
			)));
		}
	}

	let mut glyphs: Option<Vec<String>> = args
		.glyphs
		.as_deref()
		.filter(|g| !g.is_empty())
		.map(|g| g.split_whitespace().map(str::to_owned).collect());

	let mut names: Vec<String> = Vec::new();
	let mut locations: Vec<Location> = Vec::new();
	let mut discrete_axes: BTreeSet<String> = BTreeSet::new();
	let mut upem = DEFAULT_UPEM;
	let mut ignore_missing = args.ignore_missing;
	let mut axis_triples: Vec<(String, (f64, f64, f64))> = Vec::new();
	let mut inputs: Vec<(String, Option<String>)> =
		args.inputs.iter().map(|i| (i.clone(), None)).collect();
	let mut variable: Option<(Box<Font>, VariableFont)> = None;

	if args.inputs.len() == 1 {
		let input = &args.inputs[0];
		if input.ends_with(".designspace") {
			let designspace = DesignSpaceDocument::from_file(input)?;
			inputs = designspace
				.sources
				.iter()
				.map(|s| {
					s.path
						.clone()
						.map(|p| (p, s.layer_name.clone()))
						.ok_or_else(|| {
							InterpolatableError::Type(
							"TypeError: expected str, bytes or os.PathLike object, not NoneType".into(),
						)
						})
				})
				.collect::<Result<_>>()?;
			// LENIENT(L40): the x value of anisotropic locations is used; fontTools
			// fails with a `TypeError`.
			locations = designspace
				.sources
				.iter()
				.map(|s| {
					s.design_location
						.iter()
						.map(|(k, v)| (k.clone(), v.x().to_f64()))
						.collect()
				})
				.collect();
			for axis in &designspace.axes {
				if let Axis::Discrete(a) = axis {
					discrete_axes.insert(a.name.clone().unwrap_or_default());
				}
			}
			for axis in &designspace.axes {
				let Axis::Continuous(a) = axis else {
					continue;
				};
				let name = a.name.clone().unwrap_or_default();
				if discrete_axes.contains(&name) {
					continue;
				}
				let mapping = dict_from_pairs(a.map.iter().map(|(k, v)| (k.to_f64(), v.to_f64())));
				let triple = (
					piecewise_linear_map(a.minimum.to_f64(), &mapping),
					piecewise_linear_map(a.default.to_f64(), &mapping),
					piecewise_linear_map(a.maximum.to_f64(), &mapping),
				);
				match axis_triples.iter_mut().find(|(n, _)| *n == name) {
					Some(entry) => entry.1 = triple,
					None => axis_triples.push((name, triple)),
				}
			}
		} else if input.ends_with(".glyphs") || input.ends_with(".glyphspackage") {
			return Err(InterpolatableError::Unsupported(
				"Glyphs sources need glyphsLib, which is not available".into(),
			));
		} else if input.ends_with(".ttf") || input.ends_with(".otf") {
			// Is variable font?
			let mut font = Box::new(Font::read(std::fs::read(input)?, ReadOptions::default())?);
			let vf = read_variable_font(&mut font, &mut glyphs)?;
			upem = vf.upem;
			names.clone_from(&vf.names);
			locations.clone_from(&vf.locations);
			axis_triples = vf
				.axis_tags
				.iter()
				.map(|t| (t.clone(), (-1.0, 0.0, 1.0)))
				.collect();
			ignore_missing = true;
			inputs = Vec::new();
			variable = Some((font, vf));
		}
	}

	// `if not locations: locations = [{} for _ in fonts]`: fonts is empty here
	// unless the input is a variable font.

	// The sources: a UFO glyph set or a font for each input.
	let mut ufo_sets: Vec<Option<fontoxide_ufolib::GlyphSet>> = Vec::new();
	let mut fonts: Vec<Option<Font>> = Vec::new();
	for (filename, layer_name) in &inputs {
		if filename.ends_with(".ufo") {
			let reader = UfoReader::new(filename, true)?;
			upem = ufo_upem(&reader)?;
			let glyph_set = reader.get_glyph_set(layer_name.as_deref(), None, None)?;
			ufo_sets.push(Some(glyph_set));
			fonts.push(None);
		} else {
			let mut font = Font::read(std::fs::read(filename)?, ReadOptions::default())?;
			upem = f64::from(font.require::<Head>()?.units_per_em);
			ufo_sets.push(None);
			fonts.push(Some(font));
		}
		let base = basename(filename);
		let mut name = base.rsplit_once('.').map_or(base, |(n, _)| n).to_owned();
		if let Some(layer) = layer_name {
			name.push_str(&format!(" ({layer})"));
		}
		names.push(name);
	}

	// The glyph sets.
	let mut variable_default: Option<TtGlyphSet<'_>> = None;
	let mut variable_masters: Vec<(BTreeMap<String, f64>, IndexSet<String>)> = Vec::new();
	if let Some((font, vf)) = variable.as_mut() {
		variable_default = Some(TtGlyphSet::new(font)?);
		variable_masters = std::mem::take(&mut vf.masters);
	}
	let mut tt_sets: Vec<Option<TtGlyphSet<'_>>> = Vec::new();
	for font in &mut fonts {
		tt_sets.push(match font {
			Some(font) => Some(TtGlyphSet::new(font)?),
			None => None,
		});
	}
	let located: Vec<LocatedGlyphSet<'_, '_>> = match &variable_default {
		Some(default) => variable_masters
			.iter()
			.map(|(loc, _)| LocatedGlyphSet {
				glyph_set: default,
				location: loc.clone(),
			})
			.collect(),
		None => Vec::new(),
	};
	let mut glyph_sets: Vec<MasterGlyphSet<'_>> = Vec::new();
	if let Some(default) = &variable_default {
		glyph_sets.push(MasterGlyphSet::new(default, default.keys().to_vec()));
		for (set, (_, keys)) in located.iter().zip(&variable_masters) {
			glyph_sets.push(MasterGlyphSet::new(set, keys.iter().cloned().collect()));
		}
	}
	for (ufo, tt) in ufo_sets.iter().zip(&tt_sets) {
		let inner: &dyn InterpolatableGlyphSet = match (ufo, tt) {
			(_, Some(tt)) => tt,
			(Some(ufo), None) => ufo,
			(None, None) => continue,
		};
		glyph_sets.push(MasterGlyphSet::new(inner, inner.glyph_names()));
	}

	if glyph_sets.len() < 2 {
		log::warn!("Font file does not seem to be variable. Nothing to check.");
		return Ok(None);
	}

	if let Some(accepted) = &args.name {
		let accepted: HashSet<&String> = accepted.iter().collect();
		let keep: Vec<bool> = names.iter().map(|n| accepted.contains(n)).collect();
		let mut i = 0;
		glyph_sets.retain(|_| {
			let k = keep.get(i).copied().unwrap_or(false);
			i += 1;
			k
		});
		// `zip` stops at the shorter list.
		locations = locations
			.into_iter()
			.zip(&keep)
			.filter(|(_, k)| **k)
			.map(|(l, _)| l)
			.collect();
		names.retain(|n| accepted.contains(n));
	}

	let glyphs: Vec<String> = match glyphs.filter(|g| !g.is_empty()) {
		Some(g) => g,
		None => {
			let set: BTreeSet<String> = glyph_sets
				.iter()
				.flat_map(|g| g.keys.iter().cloned())
				.collect();
			set.into_iter().collect()
		}
	};
	// The glyphs missing from a master are `None`; `MasterGlyphSet` already
	// treats them so.

	// Normalize locations
	let mut normalized_locations: Vec<Location> = Vec::new();
	for loc in &locations {
		let mut out: Location = Vec::new();
		for (tag, triple) in &axis_triples {
			let v = loc
				.iter()
				.find(|(k, _)| k == tag)
				.map_or(triple.1, |(_, v)| *v);
			let v = normalize_value_decreasing(v, *triple, false)
				.map_err(|e| InterpolatableError::Value(e.to_string()))?;
			out.push((tag.clone(), v));
		}
		for (k, v) in loc {
			if discrete_axes.contains(k) {
				match out.iter_mut().find(|(key, _)| key == k) {
					Some(entry) => entry.1 = *v,
					None => out.push((k.clone(), *v)),
				}
			}
		}
		normalized_locations.push(out);
	}
	let tolerance = match args.tolerance {
		Some(t) if t != 0.0 => t,
		_ => DEFAULT_TOLERANCE,
	};
	let kinkiness = args.kinkiness.unwrap_or(DEFAULT_KINKINESS);

	log::info!("Running on {} glyphsets", glyph_sets.len());
	log::info!("Locations: {normalized_locations:?}");
	let options = TestOptions {
		ignore_missing,
		locations: Some(normalized_locations),
		tolerance,
		kinkiness,
		upem,
		show_all: args.show_all,
		discrete_axes,
	};
	let sets: Vec<&dyn InterpolatableGlyphSet> = glyph_sets
		.iter()
		.map(|g| g as &dyn InterpolatableGlyphSet)
		.collect();

	let mut file;
	let f: &mut dyn Write = match &args.output {
		None => out,
		Some(path) => {
			file = std::io::BufWriter::new(std::fs::File::create(ensure_parent_dir(path)?)?);
			&mut file
		}
	};

	let mut problems = Problems::new();
	if !args.quiet {
		if args.json {
			test_gen(
				&sets,
				Some(&glyphs),
				Some(&names),
				&options,
				&mut |glyph_name, problem| {
					problems
						.entry(glyph_name.to_owned())
						.or_default()
						.push(problem);
					Ok(())
				},
			)?;
			writeln!(f, "{}", problems_to_json(&problems))?;
		} else {
			let mut last_glyph_name: Option<String> = None;
			let mut last_master_idxs: Option<Vec<usize>> = None;
			test_gen(
				&sets,
				Some(&glyphs),
				Some(&names),
				&options,
				&mut |glyph_name, p| {
					if last_glyph_name.as_deref() != Some(glyph_name) {
						writeln!(f, "Glyph {glyph_name} was not compatible:")?;
						last_glyph_name = Some(glyph_name.to_owned());
						last_master_idxs = None;
					}
					let master_idxs = p.master_indices();
					if last_master_idxs.as_ref() != Some(&master_idxs) {
						writeln!(f, "  Masters: {}:", p.master_names().join(", "))?;
						last_master_idxs = Some(master_idxs);
					}
					write_problem(f, &p)?;
					problems.entry(glyph_name.to_owned()).or_default().push(p);
					Ok(())
				},
			)?;
		}
	} else {
		test_gen(
			&sets,
			Some(&glyphs),
			Some(&names),
			&options,
			&mut |glyph_name, problem| {
				problems
					.entry(glyph_name.to_owned())
					.or_default()
					.push(problem);
				Ok(())
			},
		)?;
	}
	f.flush()?;

	let problems = sort_problems(problems);
	Ok(if problems.is_empty() {
		None
	} else {
		Some(problems)
	})
}

/// The line of a problem in the text report.
fn write_problem(f: &mut dyn Write, p: &Problem) -> Result<()> {
	let list = |v: &[usize]| {
		let items: Vec<String> = v.iter().map(usize::to_string).collect();
		format!("[{}]", items.join(", "))
	};
	match p {
		Problem::Missing { master, .. } => writeln!(f, "    Glyph was missing in master {master}")?,
		Problem::OpenPath { master, .. } => {
			writeln!(f, "    Glyph has an open path in master {master}")?;
		}
		Problem::PathCount {
			masters,
			value_1,
			value_2,
		} => writeln!(
			f,
			"    Path count differs: {value_1} in {}, {value_2} in {}",
			masters.master_1, masters.master_2
		)?,
		Problem::NodeCount {
			path,
			masters,
			value_1,
			value_2,
		} => writeln!(
			f,
			"    Node count differs in path {path}: {value_1} in {}, {value_2} in {}",
			masters.master_1, masters.master_2
		)?,
		Problem::NodeIncompatibility {
			path,
			node,
			masters,
			value_1,
			value_2,
		} => writeln!(
			f,
			"    Node {node} incompatible in path {path}: {value_1} in {}, {value_2} in {}",
			masters.master_1, masters.master_2
		)?,
		Problem::ContourOrder {
			masters,
			value_1,
			value_2,
			..
		} => writeln!(
			f,
			"    Contour order differs: {} in {}, {} in {}",
			list(value_1),
			masters.master_1,
			list(value_2),
			masters.master_2
		)?,
		Problem::WrongStartPoint {
			contour,
			masters,
			value_1,
			value_2,
			reversed,
			..
		} => writeln!(
			f,
			"    Contour {contour} start point differs: {value_1} in {}, {value_2} in {}; reversed: {}",
			masters.master_1,
			masters.master_2,
			if *reversed { "True" } else { "False" }
		)?,
		Problem::Underweight {
			contour, masters, ..
		} => writeln!(
			f,
			"    Contour {contour} interpolation is underweight: {}, {}",
			masters.master_1, masters.master_2
		)?,
		Problem::Overweight {
			contour, masters, ..
		} => writeln!(
			f,
			"    Contour {contour} interpolation is overweight: {}, {}",
			masters.master_1, masters.master_2
		)?,
		Problem::Kink {
			contour,
			masters,
			value,
			..
		} => writeln!(
			f,
			"    Contour {contour} has a kink at {value}: {}, {}",
			masters.master_1, masters.master_2
		)?,
		Problem::Nothing { masters } => writeln!(
			f,
			"    Showing {} and {}",
			masters.master_1, masters.master_2
		)?,
	}
	Ok(())
}
