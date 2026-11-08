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
		}
	}

	fn extension(self) -> &'static str {
		match self {
			Op::Ttx | Op::TtcTtx => "ttx",
			Op::Roundtrip | Op::TtcRoundtrip | Op::Fromttx => "bin",
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

	let flavor = if args.repacker { "-hb" } else { "" };
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
		let ttx_ref = ref_dir.join(Op::Ttx.name()).join(format!("{key}.ttx"));
		let mut scheduled_ttx = false;
		for &op in ops
			.iter()
			.filter(|op| !op.is_collection_op() || collection)
			.filter(|&&op| !ttx_input || op == Op::Fromttx)
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
				jobs.push(serde_json::json!({
					"op": op.name(),
					"input": input,
					"output": out,
					"font_number": 0,
					"repacker": args.repacker,
				}));
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
		let reference = if out.exists() {
			Ok(std::fs::read(&out)?)
		} else {
			Err(std::fs::read_to_string(&err).unwrap_or_default())
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
		match (ours(op, &input), reference) {
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
fn ours(op: Op, input: &[u8]) -> Result<Vec<u8>, String> {
	let options = ReadOptions {
		font_number: Some(0),
		recalc_timestamp: false,
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
	let mut font = Font::read(input.to_vec(), options).map_err(|e| e.to_string())?;
	match op {
		Op::Ttx => font
			.to_ttx(&TtxOptions::default())
			.map(String::into_bytes)
			.map_err(|e| e.to_string()),
		Op::Roundtrip | Op::TtcTtx | Op::TtcRoundtrip | Op::Fromttx => {
			font.ensure_decompiled().map_err(|e| e.to_string())?;
			font.write(&WriteOptions::default())
				.map_err(|e| e.to_string())
		}
	}
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
		Op::Roundtrip | Op::TtcRoundtrip | Op::Fromttx => {
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
					Op::Roundtrip | Op::TtcRoundtrip | Op::Fromttx => first_difference(x, y),
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
	out.insert(FILE.into(), data.to_vec());
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
