//! Shared helpers of the designspaceLib tests.

#![allow(dead_code)]

use std::path::PathBuf;

use fontoxide_designspacelib::{DesignSpaceDocumentError, Result};

/// `datadir`: `Tests/designspaceLib/data`.
pub fn datadir() -> PathBuf {
	let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/designspaceLib/data");
	// Like `Path(__file__).parent / "data"`, without `..` components.
	fontoxide_designspacelib::paths::abspath(&s(&dir)).unwrap_or(dir)
}

/// `tmpdir`: a fresh directory for one test.
pub fn tmpdir(name: &str) -> Result<PathBuf> {
	let dir = std::env::temp_dir().join(format!(
		"fontoxide-designspacelib-{}-{name}",
		std::process::id()
	));
	if dir.exists() {
		std::fs::remove_dir_all(&dir)?;
	}
	std::fs::create_dir_all(&dir)?;
	Ok(dir)
}

/// `str(path)`.
pub fn s(path: &std::path::Path) -> String {
	path.to_string_lossy().into_owned()
}

/// Reads a text file the way Python's `open(..., encoding="utf-8")` does,
/// with universal newlines.
pub fn read_text(path: &std::path::Path) -> Result<String> {
	Ok(std::fs::read_to_string(path)?
		.replace("\r\n", "\n")
		.replace('\r', "\n"))
}

/// `re.sub(pattern, "", text)` for `start(.|\n)*?end`.
pub fn remove_between(text: &str, start: &str, end: &str) -> String {
	let mut out = String::new();
	let mut rest = text;
	while let Some(i) = rest.find(start) {
		let Some(j) = rest[i + start.len()..].find(end) else {
			break;
		};
		out.push_str(&rest[..i]);
		rest = &rest[i + start.len() + j + end.len()..];
	}
	out.push_str(rest);
	out
}

/// `re.sub(r"\s*\n+", "\n", text)`.
pub fn collapse_blank_lines(text: &str) -> String {
	let chars: Vec<char> = text.chars().collect();
	let mut out = String::new();
	let mut i = 0;
	while i < chars.len() {
		if chars[i].is_whitespace() {
			let mut j = i;
			while j < chars.len() && chars[j].is_whitespace() {
				j += 1;
			}
			match chars[i..j].iter().rposition(|&c| c == '\n') {
				Some(k) => {
					out.push('\n');
					out.extend(&chars[i + k + 1..j]);
				}
				None => out.extend(&chars[i..j]),
			}
			i = j;
		} else {
			out.push(chars[i]);
			i += 1;
		}
	}
	out
}

/// The expected content of a test file: without comments and blank lines.
pub fn expected_text(name: &str) -> Result<String> {
	let expected = read_text(&datadir().join(name))?;
	Ok(collapse_blank_lines(&remove_between(
		&expected, "<!--", "-->",
	)))
}

/// `assert_equals_test_file`.
pub fn assert_equals_test_file(path: &std::path::Path, test_filename: &str) -> Result<()> {
	let actual = read_text(path)?;
	let expected = expected_text(test_filename)?;
	assert_eq!(actual, expected);
	Ok(())
}

/// Copies a data file into a directory (`shutil.copy`).
pub fn copy_data(name: &str, dir: &std::path::Path) -> Result<PathBuf> {
	let target = dir.join(name);
	std::fs::copy(datadir().join(name), &target)?;
	Ok(target)
}

/// Asserts that a result is an error whose message contains `text`.
pub fn assert_err_contains<T: std::fmt::Debug>(result: Result<T>, text: &str) {
	match result {
		Err(e) => assert!(e.to_string().contains(text), "{e} does not contain {text}"),
		Ok(v) => panic!("expected an error containing {text:?}, got {v:?}"),
	}
}

/// An error for a failed expectation.
pub fn fail(msg: &str) -> DesignSpaceDocumentError {
	DesignSpaceDocumentError::Document(msg.to_owned())
}
