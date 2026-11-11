//! The `os.path` functions designspaceLib uses, with Python's string semantics
//! for the platform (`ntpath` on Windows, `posixpath` elsewhere).

use std::path::{Component, Path, PathBuf};

use crate::{DesignSpaceDocumentError, Result};

/// `os.path.sep`.
#[cfg(windows)]
pub const SEP: char = '\\';
/// `os.path.sep`.
#[cfg(not(windows))]
pub const SEP: char = '/';

fn is_sep(c: char) -> bool {
	c == '/' || (cfg!(windows) && c == '\\')
}

/// `posixpath.join(a, b)`.
fn posix_join(path: &mut String, b: &str) {
	if b.starts_with('/') {
		*path = b.to_owned();
	} else if path.is_empty() || path.ends_with('/') {
		path.push_str(b);
	} else {
		path.push('/');
		path.push_str(b);
	}
}

/// `posix`: normalizes a path to forward slashes, also on Windows.
pub fn posix(path: &str) -> String {
	let mut new_path = String::new();
	for part in path.split(SEP) {
		posix_join(&mut new_path, part);
	}
	// UPSTREAM-BUG(B31): on Windows, a path that already starts with "/" is not
	// split, so this adds a second slash ("/a/b" becomes "//a/b").
	if path.starts_with('/') {
		// The above transformation loses absolute paths
		new_path = format!("/{new_path}");
	} else if path.starts_with(r"\\") {
		// The above transformation loses leading slashes of UNC path mounts
		new_path = format!("//{new_path}");
	}
	new_path
}

/// `os.path.splitdrive(p)` (only Windows has drives).
fn splitdrive(p: &str) -> (&str, &str) {
	if cfg!(windows) {
		let chars: Vec<char> = p.chars().take(2).collect();
		if chars.len() == 2 && chars[1] == ':' && chars[0].is_ascii() {
			return p.split_at(2);
		}
		if chars.len() == 2 && is_sep(chars[0]) && is_sep(chars[1]) {
			// UNC path: \\server\share
			let rest = &p[2..];
			if let Some(i) = rest.find(is_sep) {
				let after = &rest[i + 1..];
				let j = after.find(is_sep).unwrap_or(after.len());
				let end = 2 + i + 1 + j;
				return p.split_at(end);
			}
			return (p, "");
		}
	}
	("", p)
}

/// `os.path.split(p)`.
fn split(p: &str) -> (String, String) {
	let (drive, rest) = splitdrive(p);
	let i = rest.rfind(is_sep).map_or(0, |i| i + 1);
	let (head, tail) = rest.split_at(i);
	let trimmed = head.trim_end_matches(is_sep);
	let head = if trimmed.is_empty() { head } else { trimmed };
	(format!("{drive}{head}"), tail.to_owned())
}

/// `os.path.dirname(p)`.
pub fn dirname(p: &str) -> String {
	split(p).0
}

/// `os.path.basename(p)`.
pub fn basename(p: &str) -> String {
	split(p).1
}

/// `os.path.splitext(p)[0]`.
pub fn splitext_root(p: &str) -> &str {
	let sep_index = p.rfind(is_sep).map_or(0, |i| i + 1);
	if let Some(dot) = p.rfind('.')
		&& dot > sep_index
	{
		// Leading dots of the file name don't start an extension.
		if p[sep_index..dot].chars().any(|c| c != '.') {
			return &p[..dot];
		}
	}
	p
}

/// `os.path.isabs(p)`.
fn isabs(p: &str) -> bool {
	if cfg!(windows) {
		let (drive, rest) = splitdrive(p);
		(!drive.is_empty() && rest.starts_with(is_sep))
			|| p.starts_with(r"\\")
			|| p.starts_with("//")
	} else {
		p.starts_with('/')
	}
}

/// `os.path.join(a, b)`.
pub fn join(a: &str, b: &str) -> String {
	if isabs(b) {
		return b.to_owned();
	}
	if cfg!(windows) {
		let (b_drive, _) = splitdrive(b);
		if !b_drive.is_empty() {
			return b.to_owned();
		}
	}
	if a.is_empty() || a.ends_with(is_sep) || (cfg!(windows) && a.ends_with(':') && a.len() == 2) {
		format!("{a}{b}")
	} else {
		format!("{a}{SEP}{b}")
	}
}

/// `os.path.abspath(p)`: absolute and lexically normalized.
pub fn abspath(p: &str) -> Result<PathBuf> {
	let absolute = std::path::absolute(if p.is_empty() { "." } else { p })
		.map_err(DesignSpaceDocumentError::Io)?;
	let mut out = PathBuf::new();
	for component in absolute.components() {
		match component {
			Component::ParentDir => {
				if !matches!(
					out.components().next_back(),
					Some(Component::RootDir) | None
				) {
					out.pop();
				}
			}
			Component::CurDir => {}
			c => out.push(c.as_os_str()),
		}
	}
	Ok(out)
}

/// `os.path.abspath(p)` as a string.
pub fn abspath_str(p: &str) -> Result<String> {
	Ok(abspath(p)?.to_string_lossy().into_owned())
}

fn same_component(a: &Component<'_>, b: &Component<'_>) -> bool {
	if cfg!(windows) {
		a.as_os_str().to_string_lossy().to_lowercase()
			== b.as_os_str().to_string_lossy().to_lowercase()
	} else {
		a == b
	}
}

/// `os.path.relpath(path, start)`.
pub fn relpath(path: &str, start: &str) -> Result<String> {
	let path = abspath(path)?;
	let start = abspath(start)?;
	let p: Vec<Component<'_>> = path.components().collect();
	let s: Vec<Component<'_>> = start.components().collect();
	if cfg!(windows) {
		let prefix = |c: &[Component<'_>]| {
			c.first()
				.map(|c| c.as_os_str().to_string_lossy().to_lowercase())
		};
		if prefix(&p) != prefix(&s) {
			return Err(DesignSpaceDocumentError::Value(format!(
				"path is on mount {:?}, start on mount {:?}",
				path.to_string_lossy(),
				start.to_string_lossy()
			)));
		}
	}
	let common = p
		.iter()
		.zip(&s)
		.take_while(|(a, b)| same_component(a, b))
		.count();
	let mut parts: Vec<String> = vec!["..".to_owned(); s.len() - common];
	parts.extend(
		p[common..]
			.iter()
			.map(|c| c.as_os_str().to_string_lossy().into_owned()),
	);
	if parts.is_empty() {
		return Ok(".".to_owned());
	}
	Ok(parts.join(&SEP.to_string()))
}

/// The path as a string, for paths given as `Path`.
pub fn path_str(path: &Path) -> String {
	path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn posix_paths() {
		assert_eq!(posix("a/b"), "a/b");
		#[cfg(not(windows))]
		assert_eq!(posix("/a/b"), "/a/b");
		// Checked against fontTools on Windows.
		#[cfg(windows)]
		assert_eq!(posix("/a/b"), "//a/b");
		#[cfg(windows)]
		{
			assert_eq!(posix(r"C:\a\b"), "C:/a/b");
			assert_eq!(posix(r"\\server\share\x"), "//server/share/x");
			assert_eq!(posix(r"a\b/c"), "a/b/c");
		}
	}

	#[test]
	fn split_paths() {
		assert_eq!(dirname("a/b/c.ufo"), "a/b");
		assert_eq!(basename("a/b/c.ufo"), "c.ufo");
		assert_eq!(dirname("/c.ufo"), "/");
		assert_eq!(splitext_root("a/b.c/d.designspace"), "a/b.c/d");
		assert_eq!(splitext_root(".hidden"), ".hidden");
	}

	#[test]
	fn relative() -> Result<()> {
		let base = std::env::temp_dir();
		let base = path_str(&base);
		let doc = join(&base, "doc");
		let target = join(&join(&base, "masters"), "a.ufo");
		assert_eq!(posix(&relpath(&target, &doc)?), "../masters/a.ufo");
		Ok(())
	}
}
