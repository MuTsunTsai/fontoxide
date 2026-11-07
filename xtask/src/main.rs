//! Development tasks for fontoxide. Run with `cargo xtask <command>`.

mod codegen;
mod parity;
mod reference;
mod test_manifest;
mod upstream_report;

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Development tasks for fontoxide")]
struct Cli {
	#[command(subcommand)]
	command: Command,
}

#[derive(Subcommand)]
enum Command {
	/// Print information about the fontTools reference environment.
	Reference,
	/// Export table definitions from fontTools and regenerate the Rust code built from them.
	Codegen,
	/// Sync upstream/tests.toml with the test functions in upstream/fonttools/Tests.
	TestManifest {
		/// Fail instead of writing if the manifest is out of date (for CI).
		#[arg(long)]
		check: bool,
	},
	/// Compare fontoxide against fontTools on a font corpus.
	Parity(parity::Args),
	/// List upstream changes between two fontTools tags, grouped by fontoxide crate.
	UpstreamReport {
		/// The tag fontoxide is currently synced to, e.g. 4.68.0.
		from: String,
		/// The tag to update to.
		to: String,
	},
}

fn main() -> Result<()> {
	match Cli::parse().command {
		Command::Reference => reference::print_info(),
		Command::Codegen => codegen::run(),
		Command::TestManifest { check } => test_manifest::run(check),
		Command::Parity(args) => parity::run(&args),
		Command::UpstreamReport { from, to } => upstream_report::run(&from, &to),
	}
}

/// The workspace root.
pub fn root() -> &'static Path {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.expect("xtask lives inside the workspace")
}

/// The fontTools submodule checkout.
pub fn upstream() -> PathBuf {
	root().join("upstream").join("fonttools")
}
