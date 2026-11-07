//! `upstream/tests.toml`: every test in fontTools' test suite (the test functions in
//! `Tests/` and the doctests in `Lib/fontTools`), with its porting status. This is the
//! feature parity tracker.
//!
//! ```toml
//! [tests]
//! "Tests/subset/subset_test.py::SubsetTest::test_foo" = "pending"
//! "Tests/ttLib/ttFont_test.py::test_bar" = "ported"
//! "Tests/pens/cairoPen_test.py::test_baz" = { status = "n/a", note = "no cairo backend" }
//! ```

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{reference, root};

const HEADER: &str = "\
# Porting status of every test in fontTools' test suite: the test functions in Tests/ and
# the doctests in Lib/fontTools, as collected by pytest. Parametrized cases are folded
# into their test function.
# Maintained by `cargo xtask test-manifest`: new upstream tests are added as \"pending\" and
# removed ones are dropped. Edit the statuses by hand:
#
#   \"pending\"                              not ported yet
#   \"ported\"                               an equivalent test exists in fontoxide
#   { status = \"n/a\", note = \"<reason>\" }  intentionally not ported
";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Status {
	Pending,
	Ported,
	#[serde(rename = "n/a")]
	NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum Entry {
	Status(Status),
	Detailed {
		status: Status,
		#[serde(default)]
		note: Option<String>,
	},
}

impl Entry {
	fn status(&self) -> Status {
		match self {
			Entry::Status(s) | Entry::Detailed { status: s, .. } => *s,
		}
	}
}

#[derive(Debug, Default, Deserialize)]
struct Manifest {
	fonttools: String,
	#[serde(default)]
	tests: BTreeMap<String, Entry>,
}

#[derive(Deserialize)]
struct UpstreamTests {
	fonttools_version: String,
	tests: Vec<UpstreamTest>,
	errors: Vec<CollectionError>,
}

#[derive(Deserialize)]
struct CollectionError {
	id: String,
	error: String,
}

#[derive(Deserialize)]
struct UpstreamTest {
	id: String,
}

fn manifest_path() -> PathBuf {
	root().join("upstream").join("tests.toml")
}

pub fn run(check: bool) -> Result<()> {
	let path = manifest_path();
	let old: Manifest = if path.exists() {
		toml::from_str(&std::fs::read_to_string(&path)?)
			.with_context(|| format!("parsing {}", path.display()))?
	} else {
		Manifest::default()
	};

	let listing = root()
		.join("target")
		.join("xtask")
		.join("upstream-tests.json");
	reference::run(["list-tests".as_ref(), listing.as_os_str()])?;
	let upstream: UpstreamTests = serde_json::from_str(&std::fs::read_to_string(&listing)?)?;
	// A module that fails to import would silently lose all its tests, so refuse.
	if !upstream.errors.is_empty() {
		for e in &upstream.errors {
			eprintln!("collection error: {}: {}", e.id, e.error);
		}
		bail!(
			"pytest could not collect {} module(s); add the missing dependencies to tools/reference",
			upstream.errors.len()
		);
	}

	let mut tests = BTreeMap::new();
	let mut added = 0;
	for t in upstream.tests {
		let entry = old.tests.get(&t.id).cloned().unwrap_or_else(|| {
			added += 1;
			Entry::Status(Status::Pending)
		});
		tests.insert(t.id, entry);
	}
	let removed: Vec<_> = old
		.tests
		.keys()
		.filter(|id| !tests.contains_key(*id))
		.collect();

	let new = Manifest {
		fonttools: upstream.fonttools_version,
		tests,
	};
	let text = render(&new);

	if check {
		if added > 0 || !removed.is_empty() || new.fonttools != old.fonttools {
			bail!(
				"upstream/tests.toml is out of date ({added} added, {} removed); run `cargo xtask test-manifest`",
				removed.len()
			);
		}
	} else {
		std::fs::write(&path, text)?;
		for id in &removed {
			println!("removed: {id}");
		}
		println!("{added} added, {} removed.", removed.len());
	}
	print_summary(&new);
	Ok(())
}

fn quote(s: &str) -> String {
	toml::Value::String(s.to_owned()).to_string()
}

fn render(manifest: &Manifest) -> String {
	let mut out = String::from(HEADER);
	let _ = writeln!(
		out,
		"\nfonttools = {}\n\n[tests]",
		quote(&manifest.fonttools)
	);
	for (id, entry) in &manifest.tests {
		let value = match entry {
			Entry::Status(s) => quote(status_str(*s)),
			Entry::Detailed { status, note } => match note {
				Some(note) => format!(
					"{{ status = {}, note = {} }}",
					quote(status_str(*status)),
					quote(note)
				),
				None => format!("{{ status = {} }}", quote(status_str(*status))),
			},
		};
		let _ = writeln!(out, "{} = {value}", quote(id));
	}
	out
}

fn status_str(s: Status) -> &'static str {
	match s {
		Status::Pending => "pending",
		Status::Ported => "ported",
		Status::NotApplicable => "n/a",
	}
}

/// Prints the porting progress per fontTools subpackage.
fn print_summary(manifest: &Manifest) {
	#[derive(Default)]
	struct Count {
		total: usize,
		ported: usize,
		na: usize,
	}
	let mut groups: BTreeMap<&str, Count> = BTreeMap::new();
	for (id, entry) in &manifest.tests {
		let group = group_of(id);
		let c = groups.entry(group).or_default();
		c.total += 1;
		match entry.status() {
			Status::Ported => c.ported += 1,
			Status::NotApplicable => c.na += 1,
			Status::Pending => {}
		}
	}
	println!(
		"\n{:<16} {:>7} {:>7} {:>7}",
		"subpackage", "ported", "n/a", "total"
	);
	let mut sum = Count::default();
	for (group, c) in &groups {
		println!("{group:<16} {:>7} {:>7} {:>7}", c.ported, c.na, c.total);
		sum.total += c.total;
		sum.ported += c.ported;
		sum.na += c.na;
	}
	println!(
		"{:<16} {:>7} {:>7} {:>7}",
		"all", sum.ported, sum.na, sum.total
	);
}

/// The fontTools subpackage a test belongs to: `Tests/<pkg>/…` or `Lib/fontTools/<pkg>/…`.
fn group_of(id: &str) -> &str {
	let path = id.split("::").next().unwrap_or(id);
	let rest = path
		.strip_prefix("Tests/")
		.or_else(|| path.strip_prefix("Lib/fontTools/"))
		.unwrap_or(path);
	match rest.split_once('/') {
		Some((pkg, _)) => pkg,
		None => rest
			.strip_suffix(".py")
			.map_or(rest, |m| m.strip_suffix("_test").unwrap_or(m)),
	}
}
