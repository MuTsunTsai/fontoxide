//! Table code generation.
//!
//! fontTools describes most of its tables declaratively: OpenType Layout and related
//! tables in `otData.py`, and fixed-size tables as `sstruct` format strings. Those
//! definitions are exported to JSON (committed, so upstream changes show up as
//! reviewable diffs) and Rust code is generated from the JSON.

use std::path::PathBuf;

use anyhow::Result;
use serde::Deserialize;

use crate::{reference, root};

/// Where the exported definitions live.
fn data_dir() -> PathBuf {
	root()
		.join("crates")
		.join("fontoxide-ttlib")
		.join("codegen")
}

#[derive(Deserialize)]
struct OtData {
	fonttools_version: String,
	tables: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct Sstruct {
	formats: Vec<serde_json::Value>,
}

pub fn run() -> Result<()> {
	let dir = data_dir();
	let otdata = dir.join("otdata.json");
	let sstruct = dir.join("sstruct.json");

	reference::run(["export-otdata".as_ref(), otdata.as_os_str()])?;
	reference::run(["export-sstruct".as_ref(), sstruct.as_os_str()])?;

	let otdata: OtData = serde_json::from_str(&std::fs::read_to_string(&otdata)?)?;
	let sstruct: Sstruct = serde_json::from_str(&std::fs::read_to_string(&sstruct)?)?;
	println!(
		"Exported fontTools {} definitions: {} otData structs, {} sstruct formats.",
		otdata.fonttools_version,
		otdata.tables.len(),
		sstruct.formats.len()
	);

	// The Rust generators are added together with the tables they produce (phases 1–2).
	Ok(())
}
