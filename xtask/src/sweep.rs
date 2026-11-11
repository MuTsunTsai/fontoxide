//! `cargo xtask sweep`: removes stale build artifacts that Cargo never deletes.
//!
//! Every change to a crate gives its test binaries a new hash, and Cargo keeps
//! the old ones (with their PDBs, often 100–200 MB each here, because of the
//! generated OpenType code). Incremental caches pile up the same way. This
//! keeps, for each test or binary target, only the newest build, and drops
//! incremental caches that have not been used for a while. Libraries are left
//! alone, since different feature sets of a crate are legitimately built side
//! by side.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Result;

#[derive(clap::Args)]
pub struct Args {
	/// Remove incremental caches not used for this many days.
	#[arg(long, default_value_t = 3)]
	days: u64,
	/// Only print what would be removed.
	#[arg(long)]
	dry_run: bool,
}

/// `name-0123456789abcdef.ext` → (`name`, `0123456789abcdef`).
fn split_hash(file_name: &str) -> Option<(&str, &str)> {
	let base = file_name.split('.').next()?;
	let (stem, hash) = base.rsplit_once('-')?;
	(hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then_some((stem, hash))
}

fn modified(path: &Path) -> SystemTime {
	fs::metadata(path)
		.and_then(|m| m.modified())
		.unwrap_or(SystemTime::UNIX_EPOCH)
}

fn size(path: &Path) -> u64 {
	match fs::symlink_metadata(path) {
		Ok(m) if m.is_dir() => fs::read_dir(path)
			.into_iter()
			.flatten()
			.flatten()
			.map(|e| size(&e.path()))
			.sum(),
		Ok(m) => m.len(),
		Err(_) => 0,
	}
}

struct Sweeper {
	dry_run: bool,
	freed: u64,
	removed: usize,
}

impl Sweeper {
	fn remove(&mut self, path: &Path) {
		let bytes = size(path);
		if self.dry_run {
			println!("would remove {}", path.display());
		} else {
			let result = if path.is_dir() {
				fs::remove_dir_all(path)
			} else {
				fs::remove_file(path)
			};
			if let Err(e) = result {
				eprintln!("cannot remove {}: {e}", path.display());
				return;
			}
		}
		self.freed += bytes;
		self.removed += 1;
	}

	/// Keeps only the newest build of each executable target in `deps`.
	fn sweep_deps(&mut self, deps: &Path) -> Result<()> {
		// (stem, hash) → files; stems with an executable are tests or binaries.
		let mut groups: HashMap<(String, String), Vec<PathBuf>> = HashMap::new();
		for entry in fs::read_dir(deps)? {
			let path = entry?.path();
			let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
				continue;
			};
			if let Some((stem, hash)) = split_hash(name) {
				groups
					.entry((stem.to_owned(), hash.to_owned()))
					.or_default()
					.push(path);
			}
		}
		let is_exe = |files: &[PathBuf]| {
			files
				.iter()
				.any(|f| f.extension().is_some_and(|e| e == "exe") || f.extension().is_none())
		};
		let mut by_stem: HashMap<&str, Vec<(&String, SystemTime)>> = HashMap::new();
		for ((stem, hash), files) in &groups {
			if is_exe(files) {
				let newest = files
					.iter()
					.map(|f| modified(f))
					.max()
					.unwrap_or(SystemTime::UNIX_EPOCH);
				by_stem.entry(stem).or_default().push((hash, newest));
			}
		}
		let mut stale = Vec::new();
		for (stem, mut builds) in by_stem {
			builds.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
			for (hash, _) in builds.into_iter().skip(1) {
				stale.push((stem.to_owned(), hash.clone()));
			}
		}
		for key in stale {
			for f in &groups[&key] {
				self.remove(f);
			}
		}
		Ok(())
	}

	/// Removes incremental caches not used for `days` days.
	fn sweep_incremental(&mut self, incremental: &Path, days: u64) -> Result<()> {
		let cutoff = SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60);
		for entry in fs::read_dir(incremental)? {
			let path = entry?.path();
			// A cache is used when Cargo writes a new session into it.
			let last_used = fs::read_dir(&path)
				.into_iter()
				.flatten()
				.flatten()
				.map(|e| modified(&e.path()))
				.max()
				.unwrap_or_else(|| modified(&path));
			if last_used < cutoff {
				self.remove(&path);
			}
		}
		Ok(())
	}
}

pub fn run(args: &Args) -> Result<()> {
	let target = crate::root().join("target");
	let mut sweeper = Sweeper {
		dry_run: args.dry_run,
		freed: 0,
		removed: 0,
	};
	// target/<profile> and target/<triple>/<profile>.
	let mut profiles = Vec::new();
	for entry in fs::read_dir(&target).into_iter().flatten().flatten() {
		let path = entry.path();
		if path.join("deps").is_dir() {
			profiles.push(path.clone());
		}
		for sub in fs::read_dir(&path).into_iter().flatten().flatten() {
			if sub.path().join("deps").is_dir() {
				profiles.push(sub.path());
			}
		}
	}
	for profile in profiles {
		sweeper.sweep_deps(&profile.join("deps"))?;
		let incremental = profile.join("incremental");
		if incremental.is_dir() {
			sweeper.sweep_incremental(&incremental, args.days)?;
		}
	}
	println!(
		"{} {} items, {:.1} GB",
		if args.dry_run {
			"would remove"
		} else {
			"removed"
		},
		sweeper.removed,
		sweeper.freed as f64 / 1e9
	);
	Ok(())
}
