//! Lists what changed in fontTools between two tags, grouped by the fontoxide crate
//! that ports each file (according to `upstream/map.toml`). This is the to-do list
//! for syncing with a new fontTools release.

use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::{root, upstream};

#[derive(Deserialize)]
struct Map {
	#[serde(rename = "map")]
	entries: Vec<MapEntry>,
}

#[derive(Deserialize)]
struct MapEntry {
	/// Path prefix inside the fontTools repository.
	upstream: String,
	/// The fontoxide crate(s) that port (or will port) it; empty if it is not ported.
	#[serde(default)]
	rust: Vec<String>,
	/// The fontTools tag this part was last synced to; absent if not ported yet.
	#[serde(default)]
	synced: Option<String>,
}

fn git(args: &[&str]) -> Result<String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(upstream())
		.args(args)
		.output()
		.context("failed to run git")?;
	if !output.status.success() {
		bail!(
			"git {args:?} failed:\n{}",
			String::from_utf8_lossy(&output.stderr)
		);
	}
	Ok(String::from_utf8(output.stdout)?)
}

pub fn run(from: &str, to: &str) -> Result<()> {
	git(&["fetch", "--quiet", "--tags", "--filter=blob:none", "origin"])?;

	let map_path = root().join("upstream").join("map.toml");
	let map: Map = toml::from_str(&std::fs::read_to_string(&map_path)?)
		.with_context(|| format!("parsing {}", map_path.display()))?;

	// Group changed files by the longest matching map prefix.
	let diff = git(&["diff", "--name-status", "--no-renames", from, to])?;
	let mut groups: BTreeMap<Option<usize>, Vec<(String, String)>> = BTreeMap::new();
	for line in diff.lines() {
		let Some((status, path)) = line.split_once('\t') else {
			continue;
		};
		let entry = map
			.entries
			.iter()
			.enumerate()
			.filter(|(_, e)| path.starts_with(&e.upstream))
			.max_by_key(|(_, e)| e.upstream.len())
			.map(|(i, _)| i);
		groups
			.entry(entry)
			.or_default()
			.push((status.to_owned(), path.to_owned()));
	}

	println!("# fontTools {from} → {to}\n");
	println!("Compare: https://github.com/fonttools/fonttools/compare/{from}...{to}\n");

	println!("## Changed files\n");
	for (entry, files) in &groups {
		match entry.map(|i| &map.entries[i]) {
			Some(e) => {
				let rust = if e.rust.is_empty() {
					"n/a".to_owned()
				} else {
					e.rust.join(", ")
				};
				let synced = e.synced.as_deref().unwrap_or("not ported yet");
				println!("### `{}` → {rust} (synced: {synced})\n", e.upstream);
			}
			None => println!("### Unmapped\n"),
		}
		for (status, path) in files {
			println!("- {status} `{path}`");
		}
		println!();
	}

	println!("## NEWS\n");
	let news = git(&["show", &format!("{to}:NEWS.rst")])?;
	let stop = format!("{from} (released");
	for line in news.lines().take_while(|l| !l.starts_with(&stop)) {
		println!("{line}");
	}
	Ok(())
}
