//! Calls into the Python reference environment in `tools/reference`.

use std::ffi::OsStr;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::root;

#[derive(Debug, Deserialize)]
pub struct Info {
	pub fonttools_version: String,
	pub python_version: String,
	pub have_uharfbuzz: bool,
}

/// Runs `reference.py` with the given arguments and returns its stdout.
pub fn run<I, S>(args: I) -> Result<String>
where
	I: IntoIterator<Item = S>,
	S: AsRef<OsStr>,
{
	let project = root().join("tools").join("reference");
	let output = Command::new("uv")
		.arg("run")
		.arg("--quiet")
		.arg("--project")
		.arg(&project)
		.arg("python")
		.arg(project.join("reference.py"))
		.args(args)
		.current_dir(root())
		.output()
		.context("failed to run `uv`; is it installed and on PATH?")?;
	if !output.status.success() {
		bail!(
			"reference.py failed ({}):\n{}",
			output.status,
			String::from_utf8_lossy(&output.stderr)
		);
	}
	Ok(String::from_utf8(output.stdout)?)
}

pub fn info() -> Result<Info> {
	Ok(serde_json::from_str(&run(["info"])?)?)
}

pub fn print_info() -> Result<()> {
	let info = info()?;
	println!("fontTools  {}", info.fonttools_version);
	println!("Python     {}", info.python_version);
	println!(
		"uharfbuzz  {}",
		if info.have_uharfbuzz {
			"available"
		} else {
			"missing"
		}
	);
	Ok(())
}
