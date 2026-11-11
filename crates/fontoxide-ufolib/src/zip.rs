//! Zipped UFOs (`.ufoz`) on top of the `zip` crate, in place of Python's
//! `zipfile` module.
//!
//! Written archives hold the same entries as the ones `shutil.make_archive`
//! writes (a directory entry for each directory, deflated files), but the
//! compressed data and the container bytes differ.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use ::zip::write::SimpleFileOptions;
use ::zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use crate::fs::{FsError, FsResult, MemEntry, MemFs};

/// The signature of the end of central directory record.
const END_OF_CENTRAL_DIRECTORY: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];

/// `zipfile.is_zipfile`: whether the data has an end of central directory
/// record, the only thing Python checks. A damaged archive is still a zip
/// file, and fails when it is read.
pub(crate) fn is_zipfile(data: &[u8]) -> bool {
	if data.len() < 22 {
		return false;
	}
	// The record is 22 bytes, followed by a comment of up to 0xffff bytes.
	let lowest = data.len().saturating_sub(22 + 0xffff);
	data[lowest..data.len() - 18]
		.windows(4)
		.any(|w| w == END_OF_CENTRAL_DIRECTORY)
}

/// Days since 1970-01-01 of a civil date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
	let y = if m <= 2 { y - 1 } else { y };
	let era = y.div_euclid(400);
	let yoe = y - era * 400;
	let mp = (m + 9) % 12;
	let doy = (153 * mp + 2) / 5 + d - 1;
	let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
	era * 146_097 + doe - 719_468
}

/// The civil date of days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
	let z = z + 719_468;
	let era = z.div_euclid(146_097);
	let doe = z - era * 146_097;
	let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let d = doy - (153 * mp + 2) / 5 + 1;
	let m = if mp < 10 { mp + 3 } else { mp - 9 };
	(yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The timestamp of the date and time of an entry.
///
/// LENIENT(L35): `datetime(*zip_info.date_time).timestamp()` takes the time as
/// local time; here it is UTC.
fn to_timestamp(t: DateTime) -> f64 {
	let days = days_from_civil(
		i64::from(t.year()),
		i64::from(t.month()),
		i64::from(t.day()),
	);
	let secs = i64::from(t.hour()) * 3600 + i64::from(t.minute()) * 60 + i64::from(t.second());
	(days * 86400 + secs) as f64
}

/// The date and time of a timestamp, within the range of zip archives
/// (1980 to 2107).
fn from_timestamp(t: f64) -> DateTime {
	let secs = t.max(315_532_800.0) as i64;
	let (y, m, d) = civil_from_days(secs.div_euclid(86400));
	let rem = secs.rem_euclid(86400);
	let field = |v: i64| u8::try_from(v).unwrap_or(0);
	DateTime::from_date_and_time(
		u16::try_from(y.min(2107)).unwrap_or(2107),
		field(m),
		field(d),
		field(rem / 3600),
		field((rem / 60) % 60),
		field(rem % 60),
	)
	.unwrap_or_default()
}

fn zip_error(e: ::zip::result::ZipError) -> FsError {
	FsError::Io(e.into())
}

/// Reads a zip archive into an in-memory tree (`ReadZipFS`).
pub(crate) fn read_zip(data: &[u8], label: String) -> FsResult<MemFs> {
	let mut archive = ZipArchive::new(Cursor::new(data)).map_err(zip_error)?;
	let mut entries: BTreeMap<String, MemEntry> = BTreeMap::new();
	for index in 0..archive.len() {
		let mut file = archive.by_index(index).map_err(zip_error)?;
		let name = file.name().map_err(zip_error)?.into_owned();
		// Reject escaping member names up front.
		let normalized = name.trim_start_matches('/');
		let mut depth = 0i64;
		for part in normalized.split('/') {
			match part {
				"" | "." => {}
				".." => {
					depth -= 1;
					if depth < 0 {
						return Err(FsError::IllegalBackReference(name.clone()));
					}
				}
				_ => depth += 1,
			}
		}
		let parts: Vec<&str> = normalized
			.split('/')
			.filter(|p| !p.is_empty() && *p != ".")
			.collect();
		let is_dir = name.ends_with('/');
		let dir_count = if is_dir {
			parts.len()
		} else {
			parts.len().saturating_sub(1)
		};
		for i in 1..=dir_count {
			entries.entry(parts[..i].join("/")).or_insert(MemEntry::Dir);
		}
		if is_dir || parts.is_empty() {
			continue;
		}
		let mut content = Vec::new();
		// The CRC-32 is checked at the end of the data.
		file.read_to_end(&mut content).map_err(FsError::Io)?;
		let modified = file.last_modified().map(to_timestamp);
		entries.insert(parts.join("/"), MemEntry::File(content, modified));
	}
	Ok(MemFs {
		entries,
		read_only: true,
		label,
	})
}

/// Writes the entries of an in-memory tree below `root` (all of it if
/// `root` is empty), placed below `dest_root` in the archive
/// (`shutil.make_archive` of a temporary directory).
pub(crate) fn write_zip(
	tree: &MemFs,
	root: &[String],
	dest_root: Option<&str>,
) -> std::io::Result<Vec<u8>> {
	let source_prefix = if root.is_empty() {
		String::new()
	} else {
		format!("{}/", root.join("/"))
	};
	let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
	let options = SimpleFileOptions::default()
		.compression_method(CompressionMethod::Deflated)
		.last_modified_time(DateTime::default());
	if let Some(dest) = dest_root {
		writer.add_directory(format!("{dest}/"), options)?;
	}
	for (key, entry) in &tree.entries {
		let Some(rel) = key
			.strip_prefix(&source_prefix)
			.or(if source_prefix.is_empty() {
				Some(key.as_str())
			} else {
				None
			})
		else {
			continue;
		};
		if rel.is_empty() {
			continue;
		}
		let name = match dest_root {
			Some(dest) => format!("{dest}/{rel}"),
			None => rel.to_owned(),
		};
		match entry {
			MemEntry::Dir => writer.add_directory(format!("{name}/"), options)?,
			MemEntry::File(data, t) => {
				let options = match t {
					Some(t) => options.last_modified_time(from_timestamp(*t)),
					None => options,
				};
				writer.start_file(name, options)?;
				writer.write_all(data)?;
			}
		}
	}
	Ok(writer.finish()?.into_inner())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn dates() {
		assert_eq!(
			to_timestamp(from_timestamp(1_000_000_000.0)),
			1_000_000_000.0
		);
		assert_eq!(to_timestamp(from_timestamp(0.0)), 315_532_800.0);
	}

	#[test]
	fn roundtrip() -> FsResult<()> {
		let mut tree = MemFs::default();
		tree.entries.insert("a".into(), MemEntry::Dir);
		tree.entries.insert(
			"a/b.txt".into(),
			MemEntry::File(b"hello hello hello".to_vec(), Some(1_000_000_000.0)),
		);
		tree.entries.insert(
			"a/\u{e9}.txt".into(),
			MemEntry::File(b"utf-8".to_vec(), None),
		);
		let data = write_zip(&tree, &[], Some("x.ufo")).map_err(FsError::Io)?;
		assert!(is_zipfile(&data));
		let read = read_zip(&data, String::new())?;
		assert!(matches!(read.entries.get("x.ufo"), Some(MemEntry::Dir)));
		assert!(matches!(read.entries.get("x.ufo/a"), Some(MemEntry::Dir)));
		assert!(matches!(
			read.entries.get("x.ufo/a/b.txt"),
			Some(MemEntry::File(d, Some(t))) if d == b"hello hello hello" && *t == 1_000_000_000.0
		));
		assert!(matches!(
			read.entries.get("x.ufo/a/\u{e9}.txt"),
			Some(MemEntry::File(d, _)) if d == b"utf-8"
		));
		Ok(())
	}

	#[test]
	fn not_a_zip_file() {
		assert!(!is_zipfile(b"not a zip file, but long enough to be one"));
		assert!(read_zip(b"PK\x05\x06", String::new()).is_err());
	}

	#[test]
	fn back_references_are_rejected() -> std::io::Result<()> {
		let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
		writer.start_file("../evil.txt", SimpleFileOptions::default())?;
		writer.write_all(b"x")?;
		let data = writer.finish()?.into_inner();
		assert!(matches!(
			read_zip(&data, String::new()),
			Err(FsError::IllegalBackReference(_))
		));
		Ok(())
	}
}
