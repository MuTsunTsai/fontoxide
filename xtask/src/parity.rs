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

use anyhow::{Context, Result, bail};
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
	/// Instead of comparing fonts, check that the harness itself reports failures:
	/// partial or missing reference outputs, changed tables, different kinds of
	/// errors and empty subset requests.
	#[arg(long)]
	self_test: bool,
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
	/// Instantiate a variable font with `varLib.mutator`, at the location of
	/// `--instance-limits`: the default of each limit, or its minimum; dropped axes
	/// stay at their defaults. Only `--instance-option no-overlap-flag` applies. Not
	/// run unless asked for (`--op mutator`).
	Mutator,
	/// Convert to the beyond-64k companion tables (`ttLib.beyond64k.upper_tables`). Not
	/// run unless asked for (`--op upper`).
	Upper,
	/// Convert to the beyond-64k companion tables and back. Not run unless asked for
	/// (`--op lower`).
	Lower,
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
			Op::Mutator => "mutator",
			Op::Upper => "upper",
			Op::Lower => "lower",
		}
	}

	fn extension(self) -> &'static str {
		match self {
			Op::Ttx | Op::TtcTtx => "ttx",
			Op::Roundtrip
			| Op::TtcRoundtrip
			| Op::Fromttx
			| Op::Subset
			| Op::Instance
			| Op::Mutator
			| Op::Upper
			| Op::Lower => "bin",
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
	/// Both sides rejected the font, with the same kind of error.
	BothFailed,
	/// Both sides rejected the font, but with different kinds of errors, so the
	/// failures may have nothing to do with each other.
	FailedDifferently,
	/// Both sides succeeded, but the comparison checked nothing of substance: every
	/// table is unimplemented, or the subset request keeps no glyphs.
	Vacuous,
	/// The case has no input: fromttx of a font that fontTools cannot dump as TTX.
	Skipped,
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

/// A font that fontoxide rejected: the Python exceptions that its error corresponds
/// to, and the message.
#[derive(Debug, Clone)]
struct Failure {
	kinds: Vec<String>,
	message: String,
}

impl Failure {
	fn new(kinds: &[&str], message: impl ToString) -> Self {
		Failure {
			kinds: kinds.iter().map(|k| (*k).to_owned()).collect(),
			message: message.to_string(),
		}
	}

	/// Whether fontTools failed with the same kind of error, given its error as
	/// `reference.py` records it (`<exception class>: <message>`).
	fn same_kind_as(&self, reference: &str) -> bool {
		let kind = reference
			.split_once(':')
			.map_or(reference, |(k, _)| k)
			.trim();
		self.kinds.iter().any(|k| k == kind)
	}
}

impl fmt::Display for Failure {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "[{}] {}", self.kinds.join("|"), self.message)
	}
}

/// The class name at the start of a message that copies a Python exception, as in
/// `KeyError: 'head'`.
fn python_exception_prefix(message: &str) -> Option<&str> {
	let (name, _) = message.split_once(':')?;
	let valid = name.starts_with(|c: char| c.is_ascii_uppercase())
		&& name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
		&& (name.ends_with("Error") || name.ends_with("Exception"));
	valid.then_some(name)
}

impl From<fontoxide_ttlib::Error> for Failure {
	fn from(e: fontoxide_ttlib::Error) -> Self {
		use fontoxide_ttlib::Error as E;
		let message = e.to_string();
		let root = e.root_cause();
		let root_message = root.to_string();
		let inner = match root {
			E::TtLib(m) | E::Struct(m) | E::Value(m) | E::Assertion(m) | E::TtxParse(m) => {
				m.as_str()
			}
			_ => root_message.as_str(),
		};
		if let Some(name) = python_exception_prefix(inner) {
			return Failure::new(&[name], message);
		}
		let kinds: &[&str] = match root {
			E::TtLib(_) => &["TTLibError"],
			E::FileIsCollection(_) => &["TTLibFileIsCollectionError"],
			E::TableNotFound(_) | E::GlyphNotFound(_) => &["KeyError"],
			E::Struct(_) => &["error"],
			E::Assertion(_) => &["AssertionError"],
			E::Value(_) => &["ValueError", "TypeError"],
			E::TtxParse(_) => &["TTXParseError", "ExpatError"],
			E::OffsetOverflow(_) => &["OTLOffsetOverflowError"],
			E::Io(_) => &["OSError"],
			// No fontTools counterpart: never the same kind.
			_ => &[],
		};
		Failure::new(kinds, message)
	}
}

impl From<fontoxide_subset::Error> for Failure {
	fn from(e: fontoxide_subset::Error) -> Self {
		use fontoxide_subset::Error as E;
		match e {
			E::Ttlib(e) => e.into(),
			E::MissingGlyphs(_) => Failure::new(&["MissingGlyphsSubsettingError"], e),
			E::MissingUnicodes(_) => Failure::new(&["MissingUnicodesSubsettingError"], e),
			E::Option(_) => Failure::new(&["OptionError", "UnknownOptionError"], e),
			E::Io(_) => Failure::new(&["OSError"], e),
			_ => Failure::new(&[], e),
		}
	}
}

impl From<std::str::Utf8Error> for Failure {
	fn from(e: std::str::Utf8Error) -> Self {
		Failure::new(&["UnicodeDecodeError"], e)
	}
}

const FONT_EXTENSIONS: &[&str] = &["ttf", "otf", "ttc", "otc", "woff", "woff2"];
const FILE: &str = "<file>";
const FONT: &str = "<font>";

pub fn run(args: &Args) -> Result<()> {
	if args.self_test {
		return self_test();
	}
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

	// Reference outputs are keyed by the fontTools version, the script that makes
	// them and its environment, the font content and every parameter of the job, so
	// that a change to any of them never reuses a stale output.
	let mut script = std::fs::read(reference::script())?;
	script.extend(format!("{:?}", reference::ENV).bytes());
	let ref_dir = root()
		.join("target")
		.join("parity")
		.join("ref")
		.join(format!(
			"{}-r{}",
			info.fonttools_version,
			short_hash(&script)
		));

	// Plan reference jobs for everything not cached yet.
	let mut cases = Vec::new();
	let mut jobs = Vec::new();
	for font in &fonts {
		let key = content_key(font)?;
		let collection = is_collection(font);
		let ttx_input = is_ttx_input(font);
		let instance_limits =
			if (ops.contains(&Op::Instance) || ops.contains(&Op::Mutator)) && !ttx_input {
				args.instance_limits.for_font(&std::fs::read(font)?)
			} else {
				Vec::new()
			};
		let ttx_params = job_params(Op::Ttx, args.repacker);
		let ttx_ref = reference_output(&ref_dir, Op::Ttx, &key, &ttx_params);
		let mut scheduled_ttx = false;
		for &op in ops
			.iter()
			.filter(|op| !op.is_collection_op() || collection)
			.filter(|&&op| !ttx_input || op == Op::Fromttx)
			// Only variable fonts can be instantiated.
			.filter(|&&op| !matches!(op, Op::Instance | Op::Mutator) || !instance_limits.is_empty())
		{
			let mut params = job_params(op, args.repacker);
			let mut vacuous = None;
			if op == Op::Subset {
				let (unicodes, gids) = subset_request(&std::fs::read(font)?);
				vacuous = subset_request_is_vacuous(&unicodes, &gids)
					.then(|| "the subset request keeps no glyph but .notdef".to_owned());
				params["unicodes"] = serde_json::json!(unicodes);
				params["gids"] = serde_json::json!(gids);
				params["options"] = serde_json::json!(args.subset_options);
			}
			if matches!(op, Op::Instance | Op::Mutator) {
				params["options"] = serde_json::json!(
					args.instance_options
						.iter()
						.map(|o| o.name())
						.collect::<Vec<_>>()
				);
			}
			if op == Op::Mutator {
				params["location"] = serde_json::json!(
					mutator_location(&instance_limits)
						.into_iter()
						.collect::<serde_json::Map<String, serde_json::Value>>()
				);
			}
			if op == Op::Instance {
				let limits: serde_json::Map<String, serde_json::Value> = instance_limits
					.iter()
					.map(|(tag, value)| (tag.clone(), serde_json::json!(value)))
					.collect();
				params["limits"] = serde_json::Value::Object(limits);
			}
			let out = reference_output(&ref_dir, op, &key, &params);
			let err = out.with_extension("err");
			// fromttx reads fontTools' TTX dump, which must be produced first.
			let input = if ttx_input {
				// A TTX file of its own is compiled directly.
				font.clone()
			} else if op == Op::Fromttx {
				let ttx_err = ttx_ref.with_extension("err");
				if !scheduled_ttx && !(ttx_ref.exists() || ttx_err.exists()) {
					jobs.push(job(&ttx_params, font, &ttx_ref));
				}
				ttx_ref.clone()
			} else {
				font.clone()
			};
			if args.rebuild_reference || !(out.exists() || err.exists()) {
				jobs.push(job(&params, &input, &out));
				if op == Op::Ttx {
					scheduled_ttx = true;
				}
			}
			cases.push(Case {
				font: font.clone(),
				op,
				input,
				out,
				err,
				vacuous,
			});
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

	println!("Comparing {} case(s)…", cases.len());
	let mut records = Vec::new();
	for result in par_map(&cases, |case| evaluate(case, args)) {
		records.extend(result?);
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

/// One font and operation to compare.
struct Case {
	font: PathBuf,
	op: Op,
	/// What both sides read: the font, or fontTools' TTX dump of it for fromttx.
	input: PathBuf,
	/// The reference output, or its error (`err`).
	out: PathBuf,
	err: PathBuf,
	/// Why the case checks nothing, if known before running it.
	vacuous: Option<String>,
}

/// Runs fontoxide's side of a case and compares it with the reference output.
fn evaluate(case: &Case, args: &Args) -> Result<Vec<Record>> {
	let mut records = Vec::new();
	let mut push = |table: &str, outcome, detail| {
		records.push(Record {
			font: case.font.clone(),
			op: case.op,
			table: table.to_owned(),
			outcome,
			detail,
		});
	};
	if case.op == Op::Fromttx
		&& case.input != case.font
		&& read_reference(&case.input, &case.input.with_extension("err"))?.is_err()
	{
		// fontTools could not dump the TTX that the case compiles; the failure is
		// reported by the ttx op.
		push(
			FONT,
			Outcome::Skipped,
			Some("fontTools could not dump the font as TTX".into()),
		);
		return Ok(records);
	}
	let reference = read_reference(&case.out, &case.err)?;
	let input =
		std::fs::read(&case.input).with_context(|| format!("reading {}", case.input.display()))?;
	match (
		ours(
			case.op,
			&input,
			args.repacker,
			&args.subset_options,
			args.instance_limits,
			&args.instance_options,
		),
		reference,
	) {
		(Ok(a), Ok(b)) => {
			let results = compare(case.op, &a, &b);
			if args.save_mismatches && results.iter().any(|r| r.1 == Outcome::Mismatch) {
				save_mismatch(&case.font, case.op, &a, &b)?;
			}
			let vacuous = case.vacuous.clone().or_else(|| {
				results
					.iter()
					.all(|r| r.1 == Outcome::Unimplemented)
					.then(|| "every table is unimplemented".to_owned())
			});
			for (table, outcome, detail) in results {
				push(&table, outcome, detail);
			}
			if let Some(reason) = vacuous {
				push(FONT, Outcome::Vacuous, Some(reason));
			}
		}
		(Err(e), Err(r)) => push(
			FONT,
			both_failed(&e, &r),
			Some(format!("fontoxide {e} / fontTools {r}")),
		),
		(Ok(_), Err(e)) => push(FONT, Outcome::ReferenceFailed, Some(e)),
		(Err(e), Ok(_)) => push(FONT, Outcome::OursFailed, Some(e.to_string())),
	}
	Ok(records)
}

/// Maps `f` over `items` on every core, keeping their order.
fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
	use std::sync::Mutex;
	use std::sync::atomic::{AtomicUsize, Ordering};

	let threads = std::thread::available_parallelism()
		.map_or(1, |n| n.get())
		.min(items.len());
	let next = AtomicUsize::new(0);
	let slots: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
	std::thread::scope(|scope| {
		for _ in 0..threads {
			scope.spawn(|| {
				loop {
					let i = next.fetch_add(1, Ordering::Relaxed);
					let Some(item) = items.get(i) else { break };
					let result = f(item);
					*slots[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
				}
			});
		}
	});
	// A panicking thread re-raises its panic at the end of the scope, so every
	// slot is filled here.
	slots
		.into_iter()
		.filter_map(|slot| slot.into_inner().unwrap_or_else(|e| e.into_inner()))
		.collect()
}

/// Negative controls: checks that the harness reports each kind of failure it is
/// meant to catch, rather than passing it off as a match.
fn self_test() -> Result<()> {
	use anyhow::ensure;

	let dir = root().join("target").join("parity").join("self-test");
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir)?;
	let font_path = root().join("upstream/fonttools/Tests/ttLib/data/Test-Regular.ttf");
	let font = std::fs::read(&font_path).with_context(|| {
		format!(
			"reading {}; is the submodule checked out?",
			font_path.display()
		)
	})?;
	let ours = |op: Op, input: &[u8]| {
		ours(op, input, false, &[], InstanceLimits::Pin, &[]).map_err(|e| anyhow::anyhow!("{e}"))
	};
	let outcomes = |results: &[(String, Outcome, Option<String>)]| -> BTreeMap<String, Outcome> {
		results.iter().map(|(t, o, _)| (t.clone(), *o)).collect()
	};

	// Reference jobs leave their output only when they succeed.
	let ttx_params = job_params(Op::Ttx, false);
	let ok_out = dir.join("ok.ttx");
	let fail_out = dir.join("fail.bin");
	let missing_out = dir.join("missing.ttx");
	run_reference_jobs(
		&dir,
		&[
			job(&ttx_params, &font_path, &ok_out),
			job(
				&serde_json::json!({"op": "self-test-fail"}),
				&font_path,
				&fail_out,
			),
			job(&ttx_params, &dir.join("no-such-font.ttf"), &missing_out),
		],
	)?;
	ensure!(
		read_reference(&ok_out, &ok_out.with_extension("err"))?.is_ok(),
		"a successful reference job left no output"
	);
	match read_reference(&fail_out, &fail_out.with_extension("err"))? {
		Ok(_) => bail!("the partial output of a failed reference job was kept"),
		Err(e) => ensure!(e.starts_with("ValueError:"), "unexpected error: {e}"),
	}
	ensure!(
		read_reference(&missing_out, &missing_out.with_extension("err"))?.is_err(),
		"a reference job without input succeeded"
	);

	// A reference process that dies leaves neither output nor error, and the run
	// fails.
	let crash_out = dir.join("crash.bin");
	ensure!(
		run_reference_jobs(
			&dir,
			&[job(
				&serde_json::json!({"op": "self-test-crash"}),
				&font_path,
				&crash_out
			)]
		)
		.is_err(),
		"a crashed reference process went unnoticed"
	);
	ensure!(
		read_reference(&crash_out, &crash_out.with_extension("err")).is_err(),
		"a crashed reference job left a usable output"
	);

	// Positive control: fontoxide's TTX matches fontTools'.
	let ttx = ours(Op::Ttx, &font)?;
	let results = outcomes(&compare(Op::Ttx, &ttx, &std::fs::read(&ok_out)?));
	ensure!(
		results.values().all(|o| *o == Outcome::Match) && results.contains_key("head"),
		"the TTX of {} does not match: {results:?}",
		font_path.display()
	);

	// A changed TTX line is a mismatch of its table only.
	let changed = String::from_utf8(ttx.clone())?.replacen(
		"<unitsPerEm value=\"",
		"<unitsPerEm value=\"1",
		1,
	);
	let results = outcomes(&compare(Op::Ttx, changed.as_bytes(), &ttx));
	ensure!(
		results.get("head") == Some(&Outcome::Mismatch)
			&& results
				.iter()
				.all(|(t, o)| t == "head" || *o == Outcome::Match),
		"a changed TTX table was not reported: {results:?}"
	);

	// A changed binary table is a mismatch, and a removed one is missing.
	let compiled = ours(Op::Roundtrip, &font)?;
	let read = || {
		Font::read(
			compiled.clone(),
			ReadOptions {
				recalc_timestamp: false,
				..Default::default()
			},
		)
	};
	let mut changed = read()?;
	changed
		.require::<fontoxide_ttlib::tables::Head>()?
		.font_revision += 1.0;
	let changed = changed.write(&WriteOptions::default())?;
	let results = outcomes(&compare(Op::Roundtrip, &changed, &compiled));
	ensure!(
		results.get("head") == Some(&Outcome::Mismatch),
		"a changed table was not reported: {results:?}"
	);
	let mut removed = read()?;
	removed.remove(Tag::new(b"name"));
	let removed = removed.write(&WriteOptions::default())?;
	let results = outcomes(&compare(Op::Roundtrip, &removed, &compiled));
	ensure!(
		results.get("name") == Some(&Outcome::Missing),
		"a removed table was not reported: {results:?}"
	);

	// Failures of different kinds are told apart.
	let not_found = Failure::from(fontoxide_ttlib::Error::TableNotFound(Tag::new(b"head")));
	ensure!(both_failed(&not_found, "KeyError: 'head'") == Outcome::BothFailed);
	ensure!(both_failed(&not_found, "ValueError: bad") == Outcome::FailedDifferently);
	let copied = Failure::from(fontoxide_ttlib::Error::ttlib("AttributeError: x"));
	ensure!(both_failed(&copied, "AttributeError: x") == Outcome::BothFailed);
	ensure!(both_failed(&copied, "TTLibError: x") == Outcome::FailedDifferently);

	// A font without a cmap is subset by glyph ID, not to nothing.
	let mut no_cmap = read()?;
	no_cmap.remove(Tag::new(b"cmap"));
	let no_cmap = no_cmap.write(&WriteOptions::default())?;
	for data in [&compiled, &no_cmap] {
		let (unicodes, gids) = subset_request(data);
		ensure!(
			!subset_request_is_vacuous(&unicodes, &gids),
			"the subset request keeps nothing"
		);
	}
	ensure!(subset_request_is_vacuous(&[], &[0]));

	let _ = std::fs::remove_dir_all(&dir);
	println!("Self-test passed.");
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
) -> Result<Vec<u8>, Failure> {
	let options = ReadOptions {
		font_number: Some(0),
		recalc_timestamp: false,
		use_harfbuzz_repacker: Some(repacker),
		..Default::default()
	};
	if op.is_collection_op() {
		let mut collection =
			fontoxide_ttlib::TtCollection::read(input.to_vec(), options).map_err(Failure::from)?;
		return match op {
			Op::TtcTtx => collection
				.to_ttx(&TtxOptions::default())
				.map(String::into_bytes)
				.map_err(Failure::from),
			_ => {
				for font in &mut collection.fonts {
					font.ensure_decompiled().map_err(Failure::from)?;
				}
				collection.write(true).map_err(Failure::from)
			}
		};
	}
	if op == Op::Fromttx {
		let text = std::str::from_utf8(input).map_err(Failure::from)?;
		let mut font = Font::new();
		font.recalc_timestamp = false;
		font.use_harfbuzz_repacker = Some(repacker);
		font.import_ttx_with(
			text,
			&fontoxide_ttlib::TtxImportOptions {
				skip_unimplemented: true,
			},
		)
		.map_err(Failure::from)?;
		return font.write(&WriteOptions::default()).map_err(Failure::from);
	}
	if op == Op::Subset {
		let mut font = Font::read(
			input.to_vec(),
			ReadOptions {
				recalc_bboxes: false,
				..options
			},
		)
		.map_err(Failure::from)?;
		let mut subset_opts = fontoxide_subset::Options::default();
		let argv: Vec<&str> = subset_options.iter().map(String::as_str).collect();
		subset_opts.parse_opts(&argv, &[]).map_err(Failure::from)?;
		let mut subsetter = fontoxide_subset::Subsetter::new(subset_opts);
		let (unicodes, gids) = subset_request(input);
		subsetter.populate(&[], &gids, &unicodes, "");
		subsetter.subset(&mut font).map_err(Failure::from)?;
		return font.write(&WriteOptions::default()).map_err(Failure::from);
	}
	if matches!(op, Op::Upper | Op::Lower) {
		use fontoxide_ttlib::beyond64k::{ConvertOptions, lower_tables, upper_tables};
		let mut font = Font::read(input.to_vec(), options).map_err(Failure::from)?;
		upper_tables(&mut font, &ConvertOptions::default()).map_err(Failure::from)?;
		if op == Op::Lower {
			lower_tables(&mut font, &ConvertOptions::default()).map_err(Failure::from)?;
		}
		return font.write(&WriteOptions::default()).map_err(Failure::from);
	}
	if op == Op::Mutator {
		let mut font = Font::read(input.to_vec(), options).map_err(Failure::from)?;
		let location: Vec<(String, f64)> = mutator_location(&instance_limits.for_font(input))
			.into_iter()
			.map(|(tag, v)| (tag, v.as_f64().unwrap_or_default()))
			.collect();
		fontoxide_varlib::mutator::instantiate_variable_font(
			&mut font,
			&location,
			!instance_options.contains(&InstanceOption::NoOverlapFlag),
		)
		.map_err(Failure::from)?;
		return font.write(&WriteOptions::default()).map_err(Failure::from);
	}
	if op == Op::Instance {
		let mut font = Font::read(input.to_vec(), options).map_err(Failure::from)?;
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
		.map_err(Failure::from)?;
		return font.write(&WriteOptions::default()).map_err(Failure::from);
	}
	let mut font = Font::read(input.to_vec(), options).map_err(Failure::from)?;
	match op {
		Op::Ttx => font
			.to_ttx(&TtxOptions::default())
			.map(String::into_bytes)
			.map_err(Failure::from),
		Op::Roundtrip
		| Op::TtcTtx
		| Op::TtcRoundtrip
		| Op::Fromttx
		| Op::Subset
		| Op::Instance
		| Op::Mutator
		| Op::Upper
		| Op::Lower => {
			font.ensure_decompiled().map_err(Failure::from)?;
			font.write(&WriteOptions::default()).map_err(Failure::from)
		}
	}
}

/// The location of the `mutator` op: the default of each limit, or its minimum.
fn mutator_location(
	limits: &[(String, Option<[Option<f64>; 3]>)],
) -> Vec<(String, serde_json::Value)> {
	limits
		.iter()
		.filter_map(|(tag, value)| {
			let [minimum, default, _] = (*value)?;
			Some((tag.clone(), serde_json::json!(default.or(minimum)?)))
		})
		.collect()
}

/// The characters and glyphs the `subset` op keeps: every third code point of the
/// Unicode `cmap` subtables or, without any, every third glyph ID. Empty if the
/// font cannot be read.
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
		// From glyph 1: `.notdef` is always kept, and a font of two or three glyphs
		// would otherwise keep nothing else.
		let num_glyphs = font.glyph_order()?.len() as u32;
		Ok((Vec::new(), (1..num_glyphs).step_by(3).collect()))
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
		Op::Roundtrip
		| Op::TtcRoundtrip
		| Op::Fromttx
		| Op::Subset
		| Op::Instance
		| Op::Mutator
		| Op::Upper
		| Op::Lower => match (split_sfnt(ours), split_sfnt(reference)) {
			(Ok(a), Ok(b)) => (a, b),
			(Err(e), _) | (_, Err(e)) => {
				return vec![(FILE.into(), Outcome::OursFailed, Some(e))];
			}
		},
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
					Op::Roundtrip
					| Op::TtcRoundtrip
					| Op::Fromttx
					| Op::Subset
					| Op::Instance
					| Op::Mutator
					| Op::Upper
					| Op::Lower => first_difference(x, y),
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

/// The parameters of a reference job: everything but its input and output paths.
fn job_params(op: Op, repacker: bool) -> serde_json::Value {
	serde_json::json!({
		"op": op.name(),
		"font_number": 0,
		"repacker": repacker,
	})
}

/// A reference job: its parameters, input and output.
fn job(params: &serde_json::Value, input: &Path, output: &Path) -> serde_json::Value {
	let mut job = params.clone();
	job["input"] = serde_json::json!(input);
	job["output"] = serde_json::json!(output);
	job
}

/// Where the reference output of a job goes: keyed by the font content and the
/// job parameters. Its error goes next to it, with the extension `err`.
fn reference_output(ref_dir: &Path, op: Op, key: &str, params: &serde_json::Value) -> PathBuf {
	let params = short_hash(params.to_string().as_bytes());
	ref_dir
		.join(op.name())
		.join(format!("{key}-{params}.{}", op.extension()))
}

/// The reference output of a job, or its error. Exactly one of them must exist:
/// `reference.py` renames the output into place only when the job succeeds.
fn read_reference(out: &Path, err: &Path) -> Result<Result<Vec<u8>, String>> {
	match (out.exists(), err.exists()) {
		(true, false) => Ok(Ok(std::fs::read(out)?)),
		(false, true) => Ok(Err(std::fs::read_to_string(err)?)),
		(true, true) => bail!(
			"both an output and an error for {}; delete the reference cache",
			out.display()
		),
		(false, false) => bail!("no reference output for {}", out.display()),
	}
}

/// The outcome when both sides reject a font.
fn both_failed(ours: &Failure, reference: &str) -> Outcome {
	if ours.same_kind_as(reference) {
		Outcome::BothFailed
	} else {
		Outcome::FailedDifferently
	}
}

fn job_path(job: &serde_json::Value, key: &str) -> PathBuf {
	PathBuf::from(job[key].as_str().unwrap_or_default())
}

/// Runs the reference jobs, and checks that each left exactly one of its output
/// and its error. fromttx jobs compile the output of ttx jobs, so they run
/// afterwards; those whose TTX dump failed are left out, and leave nothing.
fn run_reference_jobs(ref_dir: &Path, jobs: &[serde_json::Value]) -> Result<()> {
	std::fs::create_dir_all(ref_dir)?;
	for job in jobs {
		let out = job_path(job, "output");
		let _ = std::fs::remove_file(&out);
		let _ = std::fs::remove_file(out.with_extension("err"));
	}
	let (second, first): (Vec<_>, Vec<_>) = jobs
		.iter()
		.cloned()
		.partition(|job| job["op"] == Op::Fromttx.name());
	run_reference_batches(ref_dir, "a", &first)?;
	let second: Vec<_> = second
		.into_iter()
		.filter(|job| job_path(job, "input").exists())
		.collect();
	run_reference_batches(ref_dir, "b", &second)
}

/// Runs jobs that do not depend on each other, spread over as many `reference.py`
/// processes as there are cores.
fn run_reference_batches(ref_dir: &Path, name: &str, jobs: &[serde_json::Value]) -> Result<()> {
	/// Fewer jobs than this per process are not worth the start-up of another one.
	const MIN_JOBS_PER_BATCH: usize = 8;
	if jobs.is_empty() {
		return Ok(());
	}
	let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
	let count = cores.min(jobs.len().div_ceil(MIN_JOBS_PER_BATCH));
	// Interleaved, so that the slow jobs of one directory spread over the batches.
	let batches: Vec<Vec<serde_json::Value>> = (0..count)
		.map(|i| jobs.iter().skip(i).step_by(count).cloned().collect())
		.collect();
	let indexed: Vec<(usize, &Vec<serde_json::Value>)> = batches.iter().enumerate().collect();
	let results = par_map(&indexed, |&(i, batch)| -> Result<()> {
		let jobs_path = ref_dir.join(format!("jobs-{name}{i}.json"));
		let results_path = ref_dir.join(format!("results-{name}{i}.json"));
		let _ = std::fs::remove_file(&results_path);
		std::fs::write(&jobs_path, serde_json::to_string(batch)?)?;
		reference::run([
			"batch".as_ref(),
			jobs_path.as_os_str(),
			results_path.as_os_str(),
		])?;

		#[derive(serde::Deserialize)]
		struct JobResult {
			ok: bool,
		}
		let results: Vec<JobResult> =
			serde_json::from_str(&std::fs::read_to_string(&results_path)?)?;
		if results.len() != batch.len() {
			bail!(
				"reference.py returned {} results for {} jobs",
				results.len(),
				batch.len()
			);
		}
		for (job, result) in batch.iter().zip(results) {
			let out = job_path(job, "output");
			let found = (out.exists(), out.with_extension("err").exists());
			if found != (result.ok, !result.ok) {
				bail!(
					"reference job left {found:?} (output, error) but reported ok = {}: {}",
					result.ok,
					out.display()
				);
			}
		}
		Ok(())
	});
	results.into_iter().collect()
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

/// A shorter hash, for the parts of cache keys besides the font.
fn short_hash(data: &[u8]) -> String {
	Sha256::digest(data)[..4]
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect()
}

/// Whether a subset request keeps nothing: no characters, and no glyph but
/// `.notdef`, which is always kept.
fn subset_request_is_vacuous(unicodes: &[u32], gids: &[u32]) -> bool {
	unicodes.is_empty() && gids.iter().all(|&g| g == 0)
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
