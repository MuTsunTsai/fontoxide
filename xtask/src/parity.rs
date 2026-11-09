//! Differential testing against fontTools, in the spirit of fontc's `ttx_diff`.
//!
//! For every font in the corpus and every operation, the output of fontTools
//! (produced by `tools/reference/reference.py batch`) is compared with the output of
//! fontoxide, table by table. Reference outputs are cached under
//! `target/parity/ref`, keyed by fontTools version and font content, so only new
//! fonts hit Python.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::ValueEnum;
use fontoxide_ttlib::{Font, ReadOptions, Tag, TtxOptions, WriteOptions};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{reference, root};

#[derive(Debug, clap::Args)]
pub struct Args {
	/// Operations to compare (default: all).
	#[arg(long = "op", value_enum)]
	ops: Vec<Op>,
	/// Compare against fontTools with the HarfBuzz repacker enabled.
	#[arg(long)]
	repacker: bool,
	/// Options of the `subset` op, as `pyftsubset` takes them (e.g.
	/// `--subset-option=--desubroutinize`).
	#[arg(long = "subset-option", allow_hyphen_values = true)]
	subset_options: Vec<String>,
	/// The limits of the `instance` op: `pin` pins all axes halfway between their
	/// default and maximum, `drop` pins them at their defaults, `first` pins the
	/// first axis only, `range` limits the upper half of each axis, and `move`
	/// also moves the default of each axis.
	#[arg(long = "instance-limits", value_enum, default_value = "pin")]
	instance_limits: InstanceLimits,
	/// An option of the `instance` op, in addition to the defaults; repeatable.
	#[arg(long = "instance-option", value_enum)]
	instance_options: Vec<InstanceOption>,
	/// Ignore cached reference outputs and regenerate them.
	#[arg(long)]
	rebuild_reference: bool,
	/// Also list every mismatching font and table.
	#[arg(long, short)]
	verbose: bool,
	/// Only report these tables (e.g. `--table head --table OS/2`).
	#[arg(long = "table")]
	tables: Vec<String>,
	/// Save fontoxide's and fontTools' outputs of every mismatching case under
	/// `target/parity/mismatches`, for inspection.
	#[arg(long)]
	save_mismatches: bool,
	/// Font files or directories to test (default: corpus/).
	paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
	/// Decompile every table and dump TTX.
	Ttx,
	/// Decompile every table and compile the font again.
	Roundtrip,
	/// Collections only: dump every font of the collection as TTX.
	TtcTtx,
	/// Collections only: decompile every font and compile the collection again.
	TtcRoundtrip,
	/// Read fontTools' TTX dump of the font and compile it. Tables that fontoxide
	/// does not implement are skipped.
	Fromttx,
	/// Subset the font to every third character of its `cmap`, with the default
	/// options. Not run unless asked for (`--op subset`).
	Subset,
	/// Instantiate a variable font at `--instance-limits`. Not run unless asked for
	/// (`--op instance`).
	Instance,
}

/// See [`Args::instance_options`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum InstanceOption {
	/// `updateFontNames=True`.
	UpdateNames,
	/// `optimize=False`.
	NoOptimize,
	/// `overlap=OverlapMode.KEEP_AND_DONT_SET_FLAGS`.
	NoOverlapFlag,
	/// `static=True`.
	Static,
	/// `downgradeCFF2=True`.
	DowngradeCff2,
}

impl InstanceOption {
	fn name(self) -> &'static str {
		match self {
			InstanceOption::UpdateNames => "update-names",
			InstanceOption::NoOptimize => "no-optimize",
			InstanceOption::NoOverlapFlag => "no-overlap-flag",
			InstanceOption::Static => "static",
			InstanceOption::DowngradeCff2 => "downgrade-cff2",
		}
	}
}

/// See [`Args::instance_limits`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InstanceLimits {
	Pin,
	Drop,
	First,
	Range,
	Move,
}

impl InstanceLimits {
	fn name(self) -> &'static str {
		match self {
			InstanceLimits::Pin => "pin",
			InstanceLimits::Drop => "drop",
			InstanceLimits::First => "first",
			InstanceLimits::Range => "range",
			InstanceLimits::Move => "move",
		}
	}

	/// The limits for a font, by axis tag; empty if it is not a variable font. A
	/// missing value keeps the one of `fvar`.
	fn for_font(self, data: &[u8]) -> Vec<(String, Option<[Option<f64>; 3]>)> {
		let read = || -> fontoxide_ttlib::Result<Vec<(String, f64, f64, f64)>> {
			let mut font = Font::read(data.to_vec(), ReadOptions::default())?;
			Ok(match font.get::<fontoxide_ttlib::tables::Fvar>()? {
				Some(fvar) => fvar
					.axes
					.iter()
					.map(|a| (a.tag_text(), a.min_value, a.default_value, a.max_value))
					.collect(),
				None => Vec::new(),
			})
		};
		let axes = read().unwrap_or_default();
		let mut limits = Vec::new();
		for (i, (tag, min, default, max)) in axes.into_iter().enumerate() {
			let middle = default + (max - default) / 2.0;
			let value = match self {
				InstanceLimits::Pin => Some([Some(middle); 3]),
				InstanceLimits::Drop => None,
				InstanceLimits::First if i == 0 => Some([Some(middle); 3]),
				InstanceLimits::First => continue,
				InstanceLimits::Range => Some([Some(min), None, Some(middle)]),
				InstanceLimits::Move => Some([Some(min), Some(middle), Some(max)]),
			};
			limits.push((tag, value));
		}
		limits
	}

	fn to_axis_limits(self, data: &[u8]) -> fontoxide_varlib::instancer::AxisLimits {
		self.for_font(data)
			.into_iter()
			.map(|(tag, value)| {
				let triple = value.map(|[minimum, default, maximum]| {
					fontoxide_varlib::instancer::AxisTriple {
						minimum,
						default,
						maximum,
					}
				});
				(tag, triple)
			})
			.collect()
	}
}

impl Op {
	const ALL: [Op; 5] = [
		Op::Ttx,
		Op::Roundtrip,
		Op::TtcTtx,
		Op::TtcRoundtrip,
		Op::Fromttx,
	];

	fn name(self) -> &'static str {
		match self {
			Op::Ttx => "ttx",
			Op::Roundtrip => "roundtrip",
			Op::TtcTtx => "ttc-ttx",
			Op::TtcRoundtrip => "ttc-roundtrip",
			Op::Fromttx => "fromttx",
			Op::Subset => "subset",
			Op::Instance => "instance",
		}
	}

	fn extension(self) -> &'static str {
		match self {
			Op::Ttx | Op::TtcTtx => "ttx",
			Op::Roundtrip | Op::TtcRoundtrip | Op::Fromttx | Op::Subset | Op::Instance => "bin",
		}
	}

	fn is_collection_op(self) -> bool {
		matches!(self, Op::TtcTtx | Op::TtcRoundtrip)
	}
}

fn is_collection(path: &Path) -> bool {
	std::fs::read(path).is_ok_and(|d| d.starts_with(b"ttcf"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Outcome {
	/// Identical output.
	Match,
	/// Both sides produced output, but it differs.
	Mismatch,
	/// Both sides rejected the font.
	BothFailed,
	/// fontTools failed but fontoxide succeeded.
	ReferenceFailed,
	/// fontoxide failed but fontTools succeeded.
	OursFailed,
	/// The table is only on one side.
	Missing,
	/// fontoxide does not implement the table yet (it is passed through raw).
	Unimplemented,
}

impl fmt::Display for Outcome {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let s = serde_json::to_value(self).map_err(|_| fmt::Error)?;
		f.write_str(s.as_str().unwrap_or_default())
	}
}

#[derive(Serialize)]
struct Record {
	font: PathBuf,
	op: Op,
	/// The table, `GlyphOrder`, `<file>` for the whole output, or `<font>` when the
	/// font as a whole failed.
	table: String,
	outcome: Outcome,
	#[serde(skip_serializing_if = "Option::is_none")]
	detail: Option<String>,
}

const FONT_EXTENSIONS: &[&str] = &["ttf", "otf", "ttc", "otc", "woff", "woff2"];
const FILE: &str = "<file>";
const FONT: &str = "<font>";

pub fn run(args: &Args) -> Result<()> {
	let info = reference::info()?;
	let ops: Vec<Op> = if args.ops.is_empty() {
		Op::ALL.to_vec()
	} else {
		args.ops.clone()
	};
	let paths = if args.paths.is_empty() {
		vec![root().join("corpus")]
	} else {
		args.paths.clone()
	};

	let mut fonts = Vec::new();
	for path in &paths {
		collect_fonts(path, &mut fonts).with_context(|| format!("scanning {}", path.display()))?;
	}
	fonts.sort();
	if fonts.is_empty() {
		println!("No fonts found in {paths:?}.");
		return Ok(());
	}

	let mut flavor = if args.repacker { "-hb" } else { "" }.to_owned();
	if !args.subset_options.is_empty() {
		// Outputs with other subset options are cached apart.
		let digest = Sha256::digest(args.subset_options.join("\0").as_bytes());
		flavor.push_str("-s");
		flavor.extend(digest[..4].iter().map(|b| format!("{b:02x}")));
	}
	if ops.contains(&Op::Instance) {
		// Outputs with other limits are cached apart.
		flavor.push_str("-i");
		flavor.push_str(args.instance_limits.name());
		for option in &args.instance_options {
			flavor.push('-');
			flavor.push_str(option.name());
		}
	}
	let ref_dir = root()
		.join("target")
		.join("parity")
		.join("ref")
		.join(format!("{}{flavor}", info.fonttools_version));

	// Plan reference jobs for everything not cached yet.
	let mut cases = Vec::new();
	let mut jobs = Vec::new();
	for font in &fonts {
		let key = content_key(font)?;
		let collection = is_collection(font);
		let ttx_input = is_ttx_input(font);
		let instance_limits = if ops.contains(&Op::Instance) && !ttx_input {
			args.instance_limits.for_font(&std::fs::read(font)?)
		} else {
			Vec::new()
		};
		let ttx_ref = ref_dir.join(Op::Ttx.name()).join(format!("{key}.ttx"));
		let mut scheduled_ttx = false;
		for &op in ops
			.iter()
			.filter(|op| !op.is_collection_op() || collection)
			.filter(|&&op| !ttx_input || op == Op::Fromttx)
			// Only variable fonts can be instantiated.
			.filter(|&&op| op != Op::Instance || !instance_limits.is_empty())
		{
			let out = ref_dir
				.join(op.name())
				.join(format!("{key}.{}", op.extension()));
			let err = out.with_extension("err");
			// fromttx reads fontTools' TTX dump, which must be produced first.
			let input = if ttx_input {
				// A TTX file of its own is compiled directly.
				font.clone()
			} else if op == Op::Fromttx {
				let ttx_err = ttx_ref.with_extension("err");
				if !scheduled_ttx && !(ttx_ref.exists() || ttx_err.exists()) {
					jobs.push(serde_json::json!({
						"op": Op::Ttx.name(),
						"input": font,
						"output": ttx_ref,
						"font_number": 0,
						"repacker": args.repacker,
					}));
				}
				ttx_ref.clone()
			} else {
				font.clone()
			};
			if args.rebuild_reference || !(out.exists() || err.exists()) {
				let _ = std::fs::remove_file(&out);
				let _ = std::fs::remove_file(&err);
				let mut job = serde_json::json!({
					"op": op.name(),
					"input": input,
					"output": out,
					"font_number": 0,
					"repacker": args.repacker,
				});
				if op == Op::Subset {
					let (unicodes, gids) = subset_request(&std::fs::read(font)?);
					job["unicodes"] = serde_json::json!(unicodes);
					job["gids"] = serde_json::json!(gids);
					job["options"] = serde_json::json!(args.subset_options);
				}
				if op == Op::Instance {
					job["options"] = serde_json::json!(
						args.instance_options
							.iter()
							.map(|o| o.name())
							.collect::<Vec<_>>()
					);
					let limits: serde_json::Map<String, serde_json::Value> = instance_limits
						.iter()
						.map(|(tag, value)| (tag.clone(), serde_json::json!(value)))
						.collect();
					job["limits"] = serde_json::Value::Object(limits);
				}
				jobs.push(job);
				if op == Op::Ttx {
					scheduled_ttx = true;
				}
			}
			cases.push((font.clone(), op, input, out, err));
		}
	}

	if !jobs.is_empty() {
		println!(
			"Running fontTools {} on {} job(s)…",
			info.fonttools_version,
			jobs.len()
		);
		run_reference_jobs(&ref_dir, &jobs)?;
	}

	let mut records = Vec::new();
	for (font, op, input, out, err) in cases {
		let reference = if err.exists() || !out.exists() {
			Err(std::fs::read_to_string(&err).unwrap_or_default())
		} else {
			Ok(std::fs::read(&out)?)
		};
		let input = std::fs::read(&input).unwrap_or_default();
		let mut push = |table: &str, outcome, detail| {
			records.push(Record {
				font: font.clone(),
				op,
				table: table.to_owned(),
				outcome,
				detail,
			});
		};
		match (
			ours(
				op,
				&input,
				args.repacker,
				&args.subset_options,
				args.instance_limits,
				&args.instance_options,
			),
			reference,
		) {
			(Ok(a), Ok(b)) => {
				let results = compare(op, &a, &b);
				if args.save_mismatches && results.iter().any(|r| r.1 == Outcome::Mismatch) {
					save_mismatch(&font, op, &a, &b)?;
				}
				for (table, outcome, detail) in results {
					push(&table, outcome, detail);
				}
			}
			(Err(e), Err(_)) => push(FONT, Outcome::BothFailed, Some(e)),
			(Ok(_), Err(e)) => push(FONT, Outcome::ReferenceFailed, Some(e)),
			(Err(e), Ok(_)) => push(FONT, Outcome::OursFailed, Some(e)),
		}
	}
	if !args.tables.is_empty() {
		records.retain(|r| {
			args.tables
				.iter()
				.any(|t| t.trim_end() == r.table.trim_end())
		});
	}

	let report = root().join("target").join("parity").join("report.json");
	std::fs::write(&report, serde_json::to_string_pretty(&records)?)?;
	print_summary(&records, args.verbose);
	println!("\nDetails: {}", report.display());
	Ok(())
}

/// Saves both outputs of a mismatching case as `<font>.<op>.{ours,ref}.<ext>`.
fn save_mismatch(font: &Path, op: Op, ours: &[u8], reference: &[u8]) -> Result<()> {
	let dir = root().join("target").join("parity").join("mismatches");
	std::fs::create_dir_all(&dir)?;
	let stem = font.file_name().unwrap_or_default().to_string_lossy();
	for (side, data) in [("ours", ours), ("ref", reference)] {
		let path = dir.join(format!("{stem}.{}.{side}.{}", op.name(), op.extension()));
		std::fs::write(&path, data).with_context(|| format!("writing {}", path.display()))?;
	}
	Ok(())
}

/// fontoxide's side of each operation, with the same options as `reference.py`.
fn ours(
	op: Op,
	input: &[u8],
	repacker: bool,
	subset_options: &[String],
	instance_limits: InstanceLimits,
	instance_options: &[InstanceOption],
) -> Result<Vec<u8>, String> {
	let options = ReadOptions {
		font_number: Some(0),
		recalc_timestamp: false,
		use_harfbuzz_repacker: Some(repacker),
		..Default::default()
	};
	if op.is_collection_op() {
		let mut collection = fontoxide_ttlib::TtCollection::read(input.to_vec(), options)
			.map_err(|e| e.to_string())?;
		return match op {
			Op::TtcTtx => collection
				.to_ttx(&TtxOptions::default())
				.map(String::into_bytes)
				.map_err(|e| e.to_string()),
			_ => {
				for font in &mut collection.fonts {
					font.ensure_decompiled().map_err(|e| e.to_string())?;
				}
				collection.write(true).map_err(|e| e.to_string())
			}
		};
	}
	if op == Op::Fromttx {
		let text = std::str::from_utf8(input).map_err(|e| e.to_string())?;
		let mut font = Font::new();
		font.recalc_timestamp = false;
		font.use_harfbuzz_repacker = Some(repacker);
		font.import_ttx_with(
			text,
			&fontoxide_ttlib::TtxImportOptions {
				skip_unimplemented: true,
			},
		)
		.map_err(|e| e.to_string())?;
		return font
			.write(&WriteOptions::default())
			.map_err(|e| e.to_string());
	}
	if op == Op::Subset {
		let mut font = Font::read(
			input.to_vec(),
			ReadOptions {
				recalc_bboxes: false,
				..options
			},
		)
		.map_err(|e| e.to_string())?;
		let mut subset_opts = fontoxide_subset::Options::default();
		let argv: Vec<&str> = subset_options.iter().map(String::as_str).collect();
		subset_opts
			.parse_opts(&argv, &[])
			.map_err(|e| e.to_string())?;
		let mut subsetter = fontoxide_subset::Subsetter::new(subset_opts);
		let (unicodes, gids) = subset_request(input);
		subsetter.populate(&[], &gids, &unicodes, "");
		subsetter.subset(&mut font).map_err(|e| e.to_string())?;
		return font
			.write(&WriteOptions::default())
			.map_err(|e| e.to_string());
	}
	if op == Op::Instance {
		let mut font = Font::read(input.to_vec(), options).map_err(|e| e.to_string())?;
		fontoxide_varlib::instancer::instantiate_variable_font(
			&mut font,
			&instance_limits.to_axis_limits(input),
			&fontoxide_varlib::instancer::InstancerOptions {
				optimize: !instance_options.contains(&InstanceOption::NoOptimize),
				overlap: if instance_options.contains(&InstanceOption::NoOverlapFlag) {
					fontoxide_varlib::instancer::OverlapMode::KeepAndDontSetFlags
				} else {
					fontoxide_varlib::instancer::OverlapMode::KeepAndSetFlags
				},
				make_static: instance_options.contains(&InstanceOption::Static),
				update_font_names: instance_options.contains(&InstanceOption::UpdateNames),
				downgrade_cff2: instance_options.contains(&InstanceOption::DowngradeCff2),
			},
		)
		.map_err(|e| e.to_string())?;
		return font
			.write(&WriteOptions::default())
			.map_err(|e| e.to_string());
	}
	let mut font = Font::read(input.to_vec(), options).map_err(|e| e.to_string())?;
	match op {
		Op::Ttx => font
			.to_ttx(&TtxOptions::default())
			.map(String::into_bytes)
			.map_err(|e| e.to_string()),
		Op::Roundtrip | Op::TtcTtx | Op::TtcRoundtrip | Op::Fromttx | Op::Subset | Op::Instance => {
			font.ensure_decompiled().map_err(|e| e.to_string())?;
			font.write(&WriteOptions::default())
				.map_err(|e| e.to_string())
		}
	}
}

/// The characters the `subset` op keeps: every third code point of the Unicode
/// `cmap` subtables. Empty if the font cannot be read.
fn subset_request(data: &[u8]) -> (Vec<u32>, Vec<u32>) {
	let read = || -> fontoxide_ttlib::Result<(Vec<u32>, Vec<u32>)> {
		let mut font = Font::read(
			data.to_vec(),
			ReadOptions {
				font_number: Some(0),
				..Default::default()
			},
		)?;
		let all: std::collections::BTreeSet<u32> =
			match font.get::<fontoxide_ttlib::tables::Cmap>()? {
				Some(cmap) => cmap
					.tables
					.iter()
					.filter(|t| t.is_unicode())
					.flat_map(|t| t.cmap.keys().copied())
					.collect(),
				None => Default::default(),
			};
		if !all.is_empty() {
			return Ok((all.into_iter().step_by(3).collect(), Vec::new()));
		}
		let num_glyphs = font.glyph_order()?.len() as u32;
		Ok((Vec::new(), (0..num_glyphs).step_by(3).collect()))
	};
	read().unwrap_or_default()
}

fn implemented(table: &str) -> bool {
	match table {
		"GlyphOrder" | FILE => true,
		t => Tag::from_str_padded(t).is_some_and(fontoxide_ttlib::table::is_implemented),
	}
}

/// Compares two outputs table by table.
fn compare(op: Op, ours: &[u8], reference: &[u8]) -> Vec<(String, Outcome, Option<String>)> {
	if op.is_collection_op() {
		// Compared as a whole: the TTX of a collection repeats table names, and
		// the tables of a compiled collection are shared between fonts.
		return vec![if ours == reference {
			(FILE.into(), Outcome::Match, None)
		} else if op == Op::TtcTtx {
			(
				FILE.into(),
				Outcome::Mismatch,
				Some(first_line_difference(ours, reference)),
			)
		} else {
			(
				FILE.into(),
				Outcome::Mismatch,
				Some(first_difference(ours, reference)),
			)
		}];
	}
	let (a, b) = match op {
		Op::Ttx | Op::TtcTtx => (split_ttx(ours), split_ttx(reference)),
		Op::Roundtrip | Op::TtcRoundtrip | Op::Fromttx | Op::Subset | Op::Instance => {
			match (split_sfnt(ours), split_sfnt(reference)) {
				(Ok(a), Ok(b)) => (a, b),
				(Err(e), _) | (_, Err(e)) => {
					return vec![(FILE.into(), Outcome::OursFailed, Some(e))];
				}
			}
		}
	};
	let mut out = Vec::new();
	let mut keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
	if op == Op::Fromttx {
		// Unimplemented tables are skipped on our side, so the files always differ.
		keys.retain(|k| k.as_str() != FILE);
	}
	for key in keys {
		let outcome = match (a.get(key), b.get(key)) {
			_ if !implemented(key) => (Outcome::Unimplemented, None),
			(Some(x), Some(y)) if x == y => (Outcome::Match, None),
			(Some(x), Some(y)) => (
				Outcome::Mismatch,
				Some(match op {
					Op::Ttx | Op::TtcTtx => first_line_difference(x, y),
					Op::Roundtrip | Op::TtcRoundtrip | Op::Fromttx | Op::Subset | Op::Instance => {
						first_difference(x, y)
					}
				}),
			),
			(Some(_), None) => (Outcome::Missing, Some("only in fontoxide".into())),
			(None, _) => (Outcome::Missing, Some("only in fontTools".into())),
		};
		out.push((key.clone(), outcome.0, outcome.1));
	}
	out
}

/// Splits a TTX dump into its top-level elements. The text before the first
/// table and after the last one is keyed as `<file>`.
fn split_ttx(data: &[u8]) -> BTreeMap<String, Vec<u8>> {
	let text = String::from_utf8_lossy(data);
	let mut chunks: BTreeMap<String, Vec<u8>> = BTreeMap::new();
	let mut current: Option<(String, String)> = None;
	let mut file = String::new();
	for line in text.split_inclusive('\n') {
		if let Some((name, chunk)) = &mut current {
			chunk.push_str(line);
			if line.trim_end() == format!("  </{name}>") {
				let (name, chunk) = current.take().unwrap_or_default();
				chunks.insert(name, chunk.into_bytes());
			}
			continue;
		}
		let starts_table =
			line.starts_with("  <") && !line.starts_with("  </") && !line.starts_with("   ");
		if starts_table {
			let name: String = line[3..]
				.chars()
				.take_while(|c| !c.is_whitespace() && *c != '>' && *c != '/')
				.collect();
			if line.trim_end().ends_with("/>") {
				chunks.insert(name, line.as_bytes().to_vec());
			} else {
				current = Some((name, line.to_owned()));
			}
		} else {
			file.push_str(line);
		}
	}
	if let Some((name, chunk)) = current {
		chunks.insert(name, chunk.into_bytes());
	}
	chunks.insert(FILE.into(), file.into_bytes());
	// TTX element names to tags, so that both ops report the same names.
	chunks
		.into_iter()
		.map(|(k, v)| {
			let name = match fontoxide_ttlib::font::xml_to_tag(&k) {
				Some(fontoxide_ttlib::TableKey::Table(t)) => t.to_string().trim_end().to_owned(),
				_ => k,
			};
			(name, v)
		})
		.collect()
}

/// Splits a compiled font into its tables. `head` is compared with
/// `checkSumAdjustment` zeroed, since that depends on every other table; the whole
/// file is keyed as `<file>`.
fn split_sfnt(data: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
	let reader = fontoxide_ttlib::sfnt::SfntReader::new(data.to_vec(), Default::default())
		.map_err(|e| e.to_string())?;
	let mut out = BTreeMap::new();
	for tag in reader.tags() {
		let mut table = reader.table_data(tag).map_err(|e| e.to_string())?.to_vec();
		if tag == *b"head" && table.len() >= 12 {
			table[8..12].fill(0);
		}
		out.insert(tag.to_string().trim_end().to_owned(), table);
	}
	let file = if data.starts_with(b"wOF2") {
		woff2_canonical(data)?
	} else {
		data.to_vec()
	};
	out.insert(FILE.into(), file);
	Ok(out)
}

/// A WOFF 2.0 file with its Brotli streams decompressed, and the fields that
/// depend on the compressed sizes zeroed: fontoxide's Brotli encoder does not
/// always produce the same bytes as the C library that fontTools uses.
fn woff2_canonical(data: &[u8]) -> Result<Vec<u8>, String> {
	use fontoxide_ttlib::woff2::unpack_base128;
	let short = || "WOFF2 file too short".to_owned();
	let header = data.get(..48).ok_or_else(short)?;
	let u16_at = |i: usize| u16::from_be_bytes([header[i], header[i + 1]]) as usize;
	let u32_at = |i: usize| {
		u32::from_be_bytes([header[i], header[i + 1], header[i + 2], header[i + 3]]) as usize
	};
	let decompress = |raw: &[u8]| -> Result<Vec<u8>, String> {
		let mut out = Vec::new();
		brotli::BrotliDecompress(&mut &raw[..], &mut out).map_err(|e| e.to_string())?;
		Ok(out)
	};
	let mut pos = 48;
	for _ in 0..u16_at(12) {
		let flags = *data.get(pos).ok_or_else(short)?;
		pos += 1;
		if flags & 0x3F == 0x3F {
			pos += 4;
		}
		let tag_is_glyf_or_loca = matches!(flags & 0x3F, 10 | 11);
		let transformed = if tag_is_glyf_or_loca {
			flags >> 6 != 3
		} else {
			flags >> 6 != 0
		};
		for _ in 0..if transformed { 2 } else { 1 } {
			let (_, n) =
				unpack_base128(data.get(pos..).ok_or_else(short)?).map_err(|e| e.to_string())?;
			pos += n;
		}
	}
	let mut out = header.to_vec();
	for (at, len) in [(8, 4), (20, 4), (28, 8), (40, 4)] {
		out[at..at + len].fill(0);
	}
	out.extend_from_slice(data.get(48..pos).ok_or_else(short)?);
	let compressed = data.get(pos..pos + u32_at(20)).ok_or_else(short)?;
	out.extend(decompress(compressed)?);
	let (meta_offset, meta_length) = (u32_at(28), u32_at(32));
	if meta_length != 0 {
		let raw = data
			.get(meta_offset..meta_offset + meta_length)
			.ok_or_else(short)?;
		out.extend(decompress(raw)?);
	}
	let (priv_offset, priv_length) = (u32_at(40), u32_at(44));
	if priv_length != 0 {
		out.extend_from_slice(
			data.get(priv_offset..priv_offset + priv_length)
				.ok_or_else(short)?,
		);
	}
	Ok(out)
}

fn run_reference_jobs(ref_dir: &Path, jobs: &[serde_json::Value]) -> Result<()> {
	std::fs::create_dir_all(ref_dir)?;
	let jobs_path = ref_dir.join("jobs.json");
	let results_path = ref_dir.join("results.json");
	std::fs::write(&jobs_path, serde_json::to_string(jobs)?)?;
	reference::run([
		"batch".as_ref(),
		jobs_path.as_os_str(),
		results_path.as_os_str(),
	])?;

	#[derive(serde::Deserialize)]
	struct JobResult {
		ok: bool,
		#[serde(default)]
		error: String,
	}
	let results: Vec<JobResult> = serde_json::from_str(&std::fs::read_to_string(&results_path)?)?;
	for (job, result) in jobs.iter().zip(results) {
		if !result.ok {
			let out = PathBuf::from(job["output"].as_str().unwrap_or_default());
			// A failed `saveXML` leaves a partial dump behind.
			let _ = std::fs::remove_file(&out);
			std::fs::write(out.with_extension("err"), result.error)?;
		}
	}
	Ok(())
}

/// TTX files made for parity runs (`*.parity.ttx`): only `fromttx` applies.
const TTX_INPUT_SUFFIX: &str = ".parity.ttx";

fn is_ttx_input(path: &Path) -> bool {
	path.to_string_lossy()
		.to_ascii_lowercase()
		.ends_with(TTX_INPUT_SUFFIX)
}

fn collect_fonts(path: &Path, fonts: &mut Vec<PathBuf>) -> Result<()> {
	if path.is_dir() {
		for entry in std::fs::read_dir(path)? {
			collect_fonts(&entry?.path(), fonts)?;
		}
	} else if path
		.extension()
		.and_then(|e| e.to_str())
		.is_some_and(|e| FONT_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
		|| is_ttx_input(path)
	{
		fonts.push(path.to_path_buf());
	}
	Ok(())
}

/// A short hash of the file content, used as the cache key.
fn content_key(path: &Path) -> Result<String> {
	let digest = Sha256::digest(std::fs::read(path)?);
	Ok(digest[..12].iter().map(|b| format!("{b:02x}")).collect())
}

fn first_difference(a: &[u8], b: &[u8]) -> String {
	let at = a
		.iter()
		.zip(b)
		.position(|(x, y)| x != y)
		.unwrap_or(a.len().min(b.len()));
	format!(
		"first difference at byte {at} (fontoxide {} bytes, fontTools {} bytes)",
		a.len(),
		b.len()
	)
}

fn first_line_difference(a: &[u8], b: &[u8]) -> String {
	let a = String::from_utf8_lossy(a);
	let b = String::from_utf8_lossy(b);
	let mut la = a.lines();
	let mut lb = b.lines();
	let mut n = 1;
	loop {
		match (la.next(), lb.next()) {
			(Some(x), Some(y)) if x == y => n += 1,
			(x, y) => {
				return format!(
					"line {n}: fontoxide {:?} / fontTools {:?}",
					x.unwrap_or("<end>").trim(),
					y.unwrap_or("<end>").trim()
				);
			}
		}
	}
}

fn print_summary(records: &[Record], verbose: bool) {
	// Per table: counts of each outcome.
	let mut table: BTreeMap<(Op, &str), BTreeMap<Outcome, usize>> = BTreeMap::new();
	for r in records {
		*table
			.entry((r.op, r.table.as_str()))
			.or_default()
			.entry(r.outcome)
			.or_default() += 1;
	}
	println!();
	println!("{:<10} {:<12} outcomes", "op", "table");
	for ((op, name), outcomes) in &table {
		let parts: Vec<String> = outcomes.iter().map(|(o, n)| format!("{o} {n}")).collect();
		println!("{:<10} {:<12} {}", op.name(), name, parts.join(", "));
	}
	if verbose {
		println!();
		for r in records.iter().filter(|r| {
			!matches!(
				r.outcome,
				Outcome::Match | Outcome::Unimplemented | Outcome::BothFailed
			)
		}) {
			println!(
				"{} {} {} {}: {}",
				r.op.name(),
				r.font.display(),
				r.table,
				r.outcome,
				r.detail.as_deref().unwrap_or_default()
			);
		}
	}
}
