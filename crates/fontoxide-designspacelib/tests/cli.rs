//! fontoxide's own tests (no upstream test covers `main`):
//! [`fontoxide_designspacelib::cli`] compared with what fontTools'
//! `python -m fontTools.designspaceLib` writes (tools/reference/cli_mains_fixtures.py).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_designspacelib::cli::run;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn data(name: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data/cli")
		.join(name)
}

/// A new empty temporary directory.
fn temp_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
	static COUNTER: AtomicUsize = AtomicUsize::new(0);
	let dir = std::env::temp_dir().join(format!(
		"fontoxide-designspacelib-cli-{}-{}",
		std::process::id(),
		COUNTER.fetch_add(1, Ordering::Relaxed)
	));
	if dir.exists() {
		std::fs::remove_dir_all(&dir)?;
	}
	std::fs::create_dir_all(&dir)?;
	Ok(dir)
}

#[test]
fn roundtrip_matches_fonttools() -> TestResult {
	let tests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../upstream/fonttools/Tests");
	for path in [
		"varLib/data/Build.designspace",
		"designspaceLib/data/test_v4_original.designspace",
		"designspaceLib/data/test_v5.designspace",
	] {
		let source = tests.join(path);
		let name = source.file_name().ok_or("name")?;
		// In one directory, as the fixtures: the relative paths of the output
		// depend on its directory.
		let dir = temp_dir()?;
		let input = dir.join(name);
		let output = dir.join("output.designspace");
		std::fs::copy(&source, &input)?;
		let code = run(
			&[
				input.to_str().ok_or("path")?,
				output.to_str().ok_or("path")?,
			],
			&mut Vec::new(),
			&mut Vec::new(),
		)?;
		assert_eq!(code, 0);
		assert_eq!(
			std::fs::read_to_string(&output)?,
			std::fs::read_to_string(data(&name.to_string_lossy()))?,
			"{path}"
		);
		std::fs::remove_dir_all(dir)?;
	}
	Ok(())
}

#[test]
fn help_and_usage_errors() -> TestResult {
	let mut out = Vec::new();
	let mut err = Vec::new();
	assert_eq!(run(&["-h"], &mut out, &mut err)?, 0);
	assert_eq!(
		String::from_utf8(out)?,
		std::fs::read_to_string(data("help.txt"))?
	);
	assert!(err.is_empty());
	let errors: [&[&str]; 2] = [&[], &["a", "b", "c"]];
	for (i, args) in errors.into_iter().enumerate() {
		let mut out = Vec::new();
		let mut err = Vec::new();
		let code = run(args, &mut out, &mut err)?;
		assert!(out.is_empty());
		assert_eq!(
			format!("{code}\n{}", String::from_utf8(err)?),
			std::fs::read_to_string(data(&format!("error-{i}.txt")))?,
			"{args:?}"
		);
	}
	Ok(())
}
