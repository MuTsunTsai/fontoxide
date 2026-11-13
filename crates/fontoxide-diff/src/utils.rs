//! Port of `fontTools.diff.utils`, and of the parts of Python's `shutil` that
//! `fontTools.diff` uses.

use std::path::{Path, PathBuf};

/// `file_exists`: whether the path is an existing file (`os.path.isfile`).
pub fn file_exists(path: impl AsRef<Path>) -> bool {
	path.as_ref().is_file()
}

/// `get_file_modtime`: the modification time of the file in ISO format, in the
/// local time zone, e.g. `2024-01-31T12:34:56.789012+08:00`.
///
/// As Python does, the time is the float `st_mtime` rounded to microseconds
/// (half to even), and the offset of the local time zone is that of the system
/// (or of the `TZ` environment variable).
pub fn get_file_modtime(path: impl AsRef<Path>) -> std::io::Result<String> {
	let modified = std::fs::metadata(path)?.modified()?;
	let (sec, nsec) = match modified.duration_since(std::time::UNIX_EPOCH) {
		Ok(d) => (d.as_secs() as i64, i64::from(d.subsec_nanos())),
		Err(e) => {
			let d = e.duration();
			let (s, n) = (d.as_secs() as i64, i64::from(d.subsec_nanos()));
			if n == 0 {
				(-s, 0)
			} else {
				(-s - 1, 1_000_000_000 - n)
			}
		}
	};
	Ok(iso_local_time(sec as f64 + nsec as f64 * 1e-9))
}

/// `datetime.fromtimestamp(timestamp, timezone.utc).astimezone().isoformat()`.
pub fn iso_local_time(timestamp: f64) -> String {
	// `_PyTime_ObjectToTimeval(..., _PyTime_ROUND_HALF_EVEN)`.
	let mut seconds = timestamp.trunc();
	let mut micros = ((timestamp - seconds) * 1e6).round_ties_even();
	if micros >= 1e6 {
		micros -= 1e6;
		seconds += 1.0;
	} else if micros < 0.0 {
		micros += 1e6;
		seconds -= 1.0;
	}
	let Ok(ts) = jiff::Timestamp::new(seconds as i64, micros as i32 * 1000) else {
		return String::new();
	};
	// LENIENT(L53): the offset comes from the time zone database (jiff), with the
	// historical rules; Python on Windows asks the C runtime, which applies
	// the current daylight saving rules to all dates.
	let offset = jiff::tz::TimeZone::system().to_offset(ts);
	let dt = offset.to_datetime(ts);
	let mut text = format!(
		"{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
		dt.year(),
		dt.month(),
		dt.day(),
		dt.hour(),
		dt.minute(),
		dt.second()
	);
	if micros != 0.0 {
		text.push_str(&format!(".{:06}", micros as i64));
	}
	let total = offset.seconds();
	let sign = if total < 0 { '-' } else { '+' };
	let total = total.unsigned_abs();
	text.push_str(&format!("{sign}{:02}:{:02}", total / 3600, total / 60 % 60));
	if !total.is_multiple_of(60) {
		text.push_str(&format!(":{:02}", total % 60));
	}
	text
}

/// `get_tables_argument_list`: the table tags given on the command line,
/// padded with spaces to four characters, or `None` if none were given.
pub fn get_tables_argument_list(table_list: Option<&[String]>) -> Option<Vec<String>> {
	table_list.map(|tables| {
		tables
			.iter()
			.map(|t| {
				let mut t = t.clone();
				for _ in t.chars().count()..4 {
					t.push(' ');
				}
				t
			})
			.collect()
	})
}

/// `shutil.which(cmd)`: the path of an executable, looked up in `PATH` if
/// `cmd` has no directory; on Windows, with the extensions of `PATHEXT`, and
/// in the current directory first.
pub fn which(cmd: &str) -> Option<PathBuf> {
	let windows = cfg!(windows);
	let separators: &[char] = if windows { &['/', '\\'] } else { &['/'] };
	let (dirname, cmd) = match cmd.rfind(separators) {
		Some(at) => (Some(&cmd[..=at]), &cmd[at + 1..]),
		None => (None, cmd),
	};
	let dirs: Vec<PathBuf> = match dirname {
		Some(dir) => vec![PathBuf::from(dir)],
		None => {
			let path = std::env::var_os("PATH")?;
			if path.is_empty() {
				return None;
			}
			let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
			// `_win_path_needs_curdir` (`NeedCurrentDirectoryForExePath`).
			if windows && std::env::var_os("NoDefaultCurrentDirectoryInExePath").is_none() {
				dirs.insert(0, PathBuf::from("."));
			}
			dirs
		}
	};
	let files: Vec<String> = if windows {
		let pathext = std::env::var("PATHEXT")
			.ok()
			.filter(|p| !p.is_empty())
			.unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD;.VBS;.JS;.WS;.MSC".into());
		let pathext: Vec<&str> = pathext
			.split(';')
			.filter(|e| !e.is_empty())
			.map(|e| e.trim_end_matches('.'))
			.collect();
		let mut files: Vec<String> = pathext.iter().map(|e| format!("{cmd}{e}")).collect();
		let upper = cmd.to_uppercase();
		if pathext.iter().any(|e| upper.ends_with(&e.to_uppercase())) {
			files.insert(0, cmd.to_owned());
		}
		files
	} else {
		vec![cmd.to_owned()]
	};
	let mut seen = Vec::new();
	for dir in dirs {
		let norm = if windows {
			dir.to_string_lossy().to_lowercase().replace('/', "\\")
		} else {
			dir.to_string_lossy().into_owned()
		};
		if seen.contains(&norm) {
			continue;
		}
		seen.push(norm);
		for file in &files {
			let name = dir.join(file);
			if is_executable(&name) {
				return Some(name);
			}
		}
	}
	None
}

/// `_access_check(fn, os.F_OK | os.X_OK)`.
fn is_executable(path: &Path) -> bool {
	let Ok(metadata) = std::fs::metadata(path) else {
		return false;
	};
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		!metadata.is_dir() && metadata.permissions().mode() & 0o111 != 0
	}
	#[cfg(not(unix))]
	{
		!metadata.is_dir()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn tables_argument_list() {
		assert_eq!(get_tables_argument_list(None), None);
		let tables = ["cvt".to_owned(), "head".to_owned(), "GlyphOrder".to_owned()];
		assert_eq!(
			get_tables_argument_list(Some(&tables)),
			Some(vec![
				"cvt ".to_owned(),
				"head".to_owned(),
				"GlyphOrder".to_owned()
			])
		);
	}

	#[test]
	fn iso_times() {
		// The offset depends on the time zone of the system: check the rest.
		let text = iso_local_time(1_700_000_000.123_456_7);
		assert!(text.contains(".123457"), "{text}");
		let text = iso_local_time(1_700_000_000.0);
		assert_eq!(text.len(), "2023-11-14T22:13:20+00:00".len(), "{text}");
	}
}
