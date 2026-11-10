//! Code coverage of the tests and of parity runs, measured with `cargo-llvm-cov`.
//!
//! The parity runs matter most: code that no comparison with fontTools ever runs is
//! where silent bugs can hide.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::root;

#[derive(Debug, clap::Args)]
pub struct Args {
	/// Only measure the tests, without parity runs (which need the local corpus),
	/// e.g. in CI.
	#[arg(long)]
	no_parity: bool,
	/// Also write an HTML report.
	#[arg(long)]
	html: bool,
	/// How many files with the most uncovered lines to list.
	#[arg(long, default_value_t = 25)]
	worst: usize,
}

/// The parity runs measured: the arguments of `cargo xtask parity`.
const PARITY_RUNS: &[&[&str]] = &[
	&[],
	&["--repacker"],
	&["--op", "subset"],
	&["--op", "instance"],
	&["--op", "mutator"],
	&["--op", "upper"],
	&["--op", "lower"],
];

/// Files left out of the report: the development tasks themselves.
const IGNORE_FILENAME_REGEX: &str = r"(^|[\\/])xtask[\\/]";

/// The build directory of the instrumented code.
///
/// On Windows it is outside the workspace by default: instrumented test binaries
/// write their profiles into it, and Controlled Folder Access, which guards
/// `Documents`, cannot allow those binaries, whose names change with every build.
/// `CARGO_LLVM_COV_TARGET_DIR` overrides it.
fn target_dir() -> PathBuf {
	if let Some(dir) = std::env::var_os("CARGO_LLVM_COV_TARGET_DIR") {
		return dir.into();
	}
	if cfg!(windows)
		&& let Some(local) = std::env::var_os("LOCALAPPDATA")
	{
		return Path::new(&local).join("fontoxide").join("llvm-cov-target");
	}
	root().join("target").join("llvm-cov-target")
}

/// Runs `cargo` with the given arguments and environment.
fn cargo(args: &[&str], env: &[(String, String)]) -> Result<()> {
	println!("> cargo {}", args.join(" "));
	let status = Command::new("cargo")
		.args(args)
		.envs(env.iter().map(|(k, v)| (k, v)))
		.current_dir(root())
		.status()
		.context("failed to run cargo")?;
	if !status.success() {
		bail!("cargo {} failed ({status})", args.join(" "));
	}
	Ok(())
}

/// Runs `cargo llvm-cov` with the given arguments.
fn llvm_cov(target_dir: &Path, args: &[&str]) -> Result<()> {
	let mut argv = vec!["llvm-cov"];
	argv.extend_from_slice(args);
	cargo(&argv, &base_env(target_dir))
}

fn base_env(target_dir: &Path) -> Vec<(String, String)> {
	vec![(
		"CARGO_LLVM_COV_TARGET_DIR".into(),
		target_dir.to_string_lossy().into_owned(),
	)]
}

/// The environment that builds and runs instrumented code into `target_dir`
/// (`cargo llvm-cov show-env`), so that the tests and parity runs are plain cargo
/// commands.
///
/// On Windows, the profile path is made verbatim (`\\?\C:\…`): the profiling
/// runtime creates every directory of the path, starting with `C:`, which Windows
/// takes for the current directory of the drive. Each test runs in its crate
/// directory, so that is an attempt to create a directory in `Documents`, which
/// Controlled Folder Access blocks and reports.
fn instrumented_env(target_dir: &Path) -> Result<Vec<(String, String)>> {
	let output = Command::new("cargo")
		.args(["llvm-cov", "show-env"])
		.envs(base_env(target_dir))
		.current_dir(root())
		.output()
		.context("failed to run cargo llvm-cov show-env")?;
	if !output.status.success() {
		bail!(
			"cargo llvm-cov show-env failed:\n{}",
			String::from_utf8_lossy(&output.stderr)
		);
	}
	let mut env: Vec<(String, String)> = String::from_utf8(output.stdout)?
		.lines()
		.filter_map(|line| line.split_once('='))
		.map(|(k, v)| (k.to_owned(), v.to_owned()))
		.collect();
	for (key, value) in &mut env {
		if key == "LLVM_PROFILE_FILE" && cfg!(windows) && !value.starts_with(r"\\") {
			*value = format!(r"\\?\{value}");
		}
	}
	// Build into the instrumented target directory, not the workspace's.
	env.retain(|(k, _)| k != "CARGO_LLVM_COV_TARGET_DIR" && k != "CARGO_LLVM_COV_BUILD_DIR");
	env.extend(base_env(target_dir));
	env.push((
		"CARGO_TARGET_DIR".into(),
		target_dir.to_string_lossy().into_owned(),
	));
	if !env.iter().any(|(k, _)| k == "LLVM_PROFILE_FILE") {
		bail!("cargo llvm-cov show-env did not set LLVM_PROFILE_FILE");
	}
	Ok(env)
}

pub fn run(args: &Args) -> Result<()> {
	let installed = Command::new("cargo")
		.args(["llvm-cov", "--version"])
		.output()
		.is_ok_and(|o| o.status.success());
	if !installed {
		bail!(
			"cargo-llvm-cov is missing; install it with `rustup component add \
			 llvm-tools-preview` and `cargo install cargo-llvm-cov --locked`"
		);
	}
	let target_dir = target_dir();
	let report_dir = target_dir.join("report");
	println!("Instrumented build: {}", target_dir.display());

	let env = instrumented_env(&target_dir)?;
	llvm_cov(&target_dir, &["clean", "--workspace"])?;
	cargo(&["test", "--workspace"], &env)?;
	if !args.no_parity {
		for run in PARITY_RUNS {
			let mut argv = vec!["run", "--package", "xtask", "--", "parity"];
			argv.extend_from_slice(run);
			cargo(&argv, &env)?;
		}
	}

	std::fs::create_dir_all(&report_dir)?;
	let summary_path = report_dir.join("summary.json");
	llvm_cov(
		&target_dir,
		&[
			"report",
			"--json",
			"--summary-only",
			"--ignore-filename-regex",
			IGNORE_FILENAME_REGEX,
			"--output-path",
			&summary_path.to_string_lossy(),
		],
	)?;
	if args.html {
		llvm_cov(
			&target_dir,
			&[
				"report",
				"--html",
				"--ignore-filename-regex",
				IGNORE_FILENAME_REGEX,
				"--output-dir",
				&report_dir.to_string_lossy(),
			],
		)?;
	}

	let summary: serde_json::Value = serde_json::from_str(
		&std::fs::read_to_string(&summary_path)
			.with_context(|| format!("reading {}", summary_path.display()))?,
	)?;
	print_summary(&summary, args.worst);
	println!();
	println!("Summary: {}", summary_path.display());
	if args.html {
		println!(
			"HTML:    {}",
			report_dir.join("html").join("index.html").display()
		);
	}
	Ok(())
}

/// Line counts of a file or a total: (covered, count).
fn lines(summary: &serde_json::Value) -> (u64, u64) {
	let lines = &summary["lines"];
	(
		lines["covered"].as_u64().unwrap_or_default(),
		lines["count"].as_u64().unwrap_or_default(),
	)
}

fn percent((covered, count): (u64, u64)) -> f64 {
	if count == 0 {
		100.0
	} else {
		covered as f64 * 100.0 / count as f64
	}
}

/// The crate of a source file: the directory under `crates/`.
fn crate_of(path: &str) -> String {
	let parts: Vec<&str> = path.split(['/', '\\']).collect();
	parts
		.iter()
		.position(|p| *p == "crates")
		.and_then(|i| parts.get(i + 1))
		.map_or_else(|| "(other)".to_owned(), |c| (*c).to_owned())
}

/// The path of a source file relative to the workspace.
fn relative(path: &str) -> String {
	let root = root().to_string_lossy().into_owned();
	path.strip_prefix(&root)
		.unwrap_or(path)
		.trim_start_matches(['/', '\\'])
		.replace('\\', "/")
}

/// Prints the line coverage in total, per crate, and the files with the most
/// uncovered lines.
fn print_summary(summary: &serde_json::Value, worst: usize) {
	let data = &summary["data"][0];
	let files: Vec<(String, (u64, u64))> = data["files"]
		.as_array()
		.into_iter()
		.flatten()
		.map(|f| {
			(
				relative(f["filename"].as_str().unwrap_or_default()),
				lines(&f["summary"]),
			)
		})
		.collect();

	let mut crates: BTreeMap<String, (u64, u64)> = BTreeMap::new();
	for (path, (covered, count)) in &files {
		let entry = crates.entry(crate_of(path)).or_default();
		entry.0 += covered;
		entry.1 += count;
	}
	println!();
	println!("{:<28} {:>9} {:>9} {:>7}", "crate", "covered", "lines", "%");
	for (name, counts) in &crates {
		println!(
			"{name:<28} {:>9} {:>9} {:>6.1}%",
			counts.0,
			counts.1,
			percent(*counts)
		);
	}
	let total = lines(&data["totals"]);
	println!(
		"{:<28} {:>9} {:>9} {:>6.1}%",
		"total",
		total.0,
		total.1,
		percent(total)
	);

	let mut by_uncovered = files.clone();
	by_uncovered
		.sort_by_key(|(path, (covered, count))| (std::cmp::Reverse(count - covered), path.clone()));
	println!();
	println!("Most uncovered lines:");
	for (path, counts) in by_uncovered.iter().take(worst) {
		println!(
			"{:>6} uncovered {:>6.1}%  {path}",
			counts.1 - counts.0,
			percent(*counts)
		);
	}
}
