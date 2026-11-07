//! Differential testing against fontTools, in the spirit of fontc's `ttx_diff`.
//!
//! For every font in the corpus and every operation, the output of fontTools
//! (produced by `tools/reference/reference.py batch`) is compared byte for byte with
//! the output of fontoxide. Reference outputs are cached under `target/parity/ref`,
//! keyed by fontTools version and font content, so only new fonts hit Python.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::ValueEnum;
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
}

impl Op {
	const ALL: [Op; 2] = [Op::Ttx, Op::Roundtrip];

	fn name(self) -> &'static str {
		match self {
			Op::Ttx => "ttx",
			Op::Roundtrip => "roundtrip",
		}
	}

	fn extension(self) -> &'static str {
		match self {
			Op::Ttx => "ttx",
			Op::Roundtrip => "bin",
		}
	}
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
	/// fontoxide does not implement this operation yet.
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
	outcome: Outcome,
	#[serde(skip_serializing_if = "Option::is_none")]
	detail: Option<String>,
}

const FONT_EXTENSIONS: &[&str] = &["ttf", "otf", "ttc", "otc", "woff", "woff2"];

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
		for &op in &ops {
			let out = ref_dir
				.join(op.name())
				.join(format!("{key}.{}", op.extension()));
			let err = out.with_extension("err");
			if args.rebuild_reference || !(out.exists() || err.exists()) {
				let _ = std::fs::remove_file(&out);
				let _ = std::fs::remove_file(&err);
				jobs.push(serde_json::json!({
					"op": op.name(),
					"input": font,
					"output": out,
					"font_number": 0,
					"repacker": args.repacker,
				}));
			}
			cases.push((font.clone(), op, out, err));
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
	for (font, op, out, err) in cases {
		let reference = if out.exists() {
			Ok(std::fs::read(&out)?)
		} else {
			Err(std::fs::read_to_string(&err).unwrap_or_default())
		};
		let input = std::fs::read(&font)?;
		let (outcome, detail) = match (ours(op, &input), reference) {
			(None, _) => (Outcome::Unimplemented, None),
			(Some(Ok(a)), Ok(b)) if a == b => (Outcome::Match, None),
			(Some(Ok(a)), Ok(b)) => (Outcome::Mismatch, Some(first_difference(&a, &b))),
			(Some(Err(e)), Err(_)) => (Outcome::BothFailed, Some(e)),
			(Some(Ok(_)), Err(e)) => (Outcome::ReferenceFailed, Some(e)),
			(Some(Err(e)), Ok(_)) => (Outcome::OursFailed, Some(e)),
		};
		records.push(Record {
			font,
			op,
			outcome,
			detail,
		});
	}

	let report = root().join("target").join("parity").join("report.json");
	std::fs::write(&report, serde_json::to_string_pretty(&records)?)?;
	print_summary(&records);
	println!("\nDetails: {}", report.display());
	Ok(())
}

/// fontoxide's side of each operation. `None` means not implemented yet.
fn ours(op: Op, _input: &[u8]) -> Option<Result<Vec<u8>, String>> {
	match op {
		Op::Ttx | Op::Roundtrip => None,
	}
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

fn collect_fonts(path: &Path, fonts: &mut Vec<PathBuf>) -> Result<()> {
	if path.is_dir() {
		for entry in std::fs::read_dir(path)? {
			collect_fonts(&entry?.path(), fonts)?;
		}
	} else if path
		.extension()
		.and_then(|e| e.to_str())
		.is_some_and(|e| FONT_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
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

fn print_summary(records: &[Record]) {
	let mut table: BTreeMap<(Op, Outcome), usize> = BTreeMap::new();
	for r in records {
		*table.entry((r.op, r.outcome)).or_default() += 1;
	}
	println!();
	for ((op, outcome), n) in table {
		println!("{:<10} {:<18} {n:>6}", op.name(), outcome.to_string());
	}
}
