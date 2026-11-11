//! Zipped UFOs (`.ufoz`) on top of the `rawzip` crate, in place of Python's
//! `zipfile` module. `rawzip` handles the container only: members are
//! deflated and inflated with `flate2`.
//!
//! Written archives hold the same entries as the ones `shutil.make_archive`
//! writes (a directory entry for each directory, deflated files), but the
//! compressed data and the container bytes differ.
//!
//! `rawzip` leaves the protections against malicious archives to its users;
//! [`read_zip`] rejects member names that escape the root (as `ReadZipFS`
//! does), overlapping member data (as `zipfile` does since Python 3.12), and
//! archives that inflate to more than [`MAX_UNCOMPRESSED_SIZE`].

use std::collections::BTreeMap;
use std::io::{Read, Write};

use rawzip::time::UtcDateTime;
use rawzip::{CompressionMethod, ZipArchive, ZipArchiveWriter};

use crate::fs::{FsError, FsResult, MemEntry, MemFs};

/// The signature of the end of central directory record.
const END_OF_CENTRAL_DIRECTORY: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];

/// The largest total size of the members of an archive once inflated: 1 GiB.
///
/// LENIENT(L50): `zipfile` has no limit; an archive that would inflate to more is
/// rejected instead of being read into memory (a zip bomb).
pub const MAX_UNCOMPRESSED_SIZE: u64 = 1 << 30;

/// The highest compression ratio of a deflated member. DEFLATE cannot do
/// better than 1032:1, so a member that claims more has a forged size.
const MAX_DEFLATE_RATIO: u64 = 1032;

/// The midnight of 1980-01-01, the earliest time of a zip archive.
const MIN_TIMESTAMP: f64 = 315_532_800.0;

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

/// The timestamp of the MS-DOS date and time of an entry (`ZipInfo.date_time`),
/// or `None` if it is not a valid date.
///
/// LENIENT(L35): `datetime(*zip_info.date_time).timestamp()` takes the time as
/// local time; here it is UTC.
fn to_timestamp(t: rawzip::time::DosDateTime) -> Option<f64> {
	let (y, m, d) = (
		i64::from(t.year()),
		i64::from(t.month()),
		i64::from(t.day()),
	);
	if !(1..=12).contains(&m) || !(1..=31).contains(&d) || t.hour() > 23 || t.minute() > 59 {
		return None;
	}
	let days = days_from_civil(y, m, d);
	let secs = i64::from(t.hour()) * 3600 + i64::from(t.minute()) * 60 + i64::from(t.second());
	Some((days * 86400 + secs) as f64)
}

/// The date and time of a timestamp, within the range of zip archives
/// (1980 to 2107).
fn from_timestamp(t: f64) -> UtcDateTime {
	let max = (days_from_civil(2108, 1, 1) * 86400 - 1) as f64;
	UtcDateTime::from_unix(t.clamp(MIN_TIMESTAMP, max) as i64)
}

fn zip_error(e: rawzip::Error) -> FsError {
	FsError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn bad_zip_file(message: String) -> FsError {
	FsError::Io(std::io::Error::new(
		std::io::ErrorKind::InvalidData,
		message,
	))
}

/// The characters 0x80 to 0xff of code page 437.
const CP437_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

/// A member name as `zipfile` decodes it: UTF-8 if the entry says so, else
/// code page 437.
fn decode_name(bytes: &[u8], utf8: bool) -> String {
	if utf8 {
		return String::from_utf8_lossy(bytes).into_owned();
	}
	bytes
		.iter()
		.map(|&b| match b {
			0..=0x7f => char::from(b),
			_ => CP437_HIGH.chars().nth(usize::from(b - 0x80)).unwrap_or('?'),
		})
		.collect()
}

/// Whether a member name leaves the root of the archive (`escapes_root`).
fn escapes_root(name: &str) -> bool {
	let mut depth = 0i64;
	for part in name.trim_start_matches('/').split('/') {
		match part {
			"" | "." => {}
			".." => {
				depth -= 1;
				if depth < 0 {
					return true;
				}
			}
			_ => depth += 1,
		}
	}
	false
}

/// A member of an archive, from the central directory.
struct Member {
	name: String,
	wayfinder: rawzip::ZipArchiveEntryWayfinder,
	method: CompressionMethod,
	modified: Option<f64>,
}

/// Reads a zip archive into an in-memory tree (`ReadZipFS`).
pub(crate) fn read_zip(data: &[u8], label: String) -> FsResult<MemFs> {
	let archive = ZipArchive::from_slice(data).map_err(zip_error)?;
	let mut members = Vec::new();
	for entry in archive.entries() {
		let entry = entry.map_err(zip_error)?;
		members.push(Member {
			name: decode_name(entry.file_path().as_bytes(), entry.flags().is_utf8()),
			wayfinder: entry.wayfinder(),
			method: entry.compression_method(),
			modified: to_timestamp(entry.last_modified_dos()),
		});
	}
	// Reject escaping member names up front, so the error names the zip entry.
	if let Some(m) = members.iter().find(|m| escapes_root(&m.name)) {
		return Err(FsError::IllegalBackReference(m.name.clone()));
	}

	// Zip bombs: overlapping member data ("Overlapped entries", as `zipfile`
	// raises), forged sizes and too much data in all.
	let mut ranges = Vec::new();
	let mut total: u64 = 0;
	for m in &members {
		if m.name.ends_with('/') {
			continue;
		}
		let entry = archive.get_entry(m.wayfinder).map_err(zip_error)?;
		ranges.push((entry.compressed_data_range(), &m.name));
		let size = m.wayfinder.uncompressed_size_hint();
		let compressed = m.wayfinder.compressed_size_hint();
		if m.method == CompressionMethod::DEFLATE
			&& size > compressed.saturating_mul(MAX_DEFLATE_RATIO)
		{
			return Err(bad_zip_file(format!(
				"Bad compression ratio for file {}",
				fontoxide_misc::py::str_repr(&m.name)
			)));
		}
		total = total.saturating_add(size);
		if total > MAX_UNCOMPRESSED_SIZE {
			return Err(bad_zip_file(format!(
				"The archive inflates to more than {MAX_UNCOMPRESSED_SIZE} bytes"
			)));
		}
	}
	ranges.sort_by_key(|((start, _), _)| *start);
	for pair in ranges.windows(2) {
		let (((_, end), _), ((start, _), name)) = (pair[0], pair[1]);
		if end > start {
			return Err(bad_zip_file(format!(
				"Overlapped entries: {} (possible zip bomb)",
				fontoxide_misc::py::str_repr(name)
			)));
		}
	}

	// The directory mirror, as `ReadZipFS._directory` builds it: `makedirs`
	// fails where a file is in the way, and `create` keeps what is already
	// there, so a directory hides a file of the same name. A file named twice
	// reads as its last member (`ZipFile.NameToInfo`).
	let mut entries: BTreeMap<String, MemEntry> = BTreeMap::new();
	for m in &members {
		let parts: Vec<&str> = m
			.name
			.trim_start_matches('/')
			.split('/')
			.filter(|p| !p.is_empty() && *p != ".")
			.collect();
		let is_dir = m.name.ends_with('/');
		let dir_count = if is_dir {
			parts.len()
		} else {
			parts.len().saturating_sub(1)
		};
		for i in 1..=dir_count {
			let key = parts[..i].join("/");
			match entries.get(&key) {
				Some(MemEntry::File(..)) => {
					return Err(FsError::Io(std::io::Error::new(
						std::io::ErrorKind::AlreadyExists,
						format!(
							"[Errno 17] File exists: {}",
							fontoxide_misc::py::str_repr(&key)
						),
					)));
				}
				Some(MemEntry::Dir) => {}
				None => {
					entries.insert(key, MemEntry::Dir);
				}
			}
		}
		if is_dir || parts.is_empty() {
			continue;
		}
		let key = parts.join("/");
		if matches!(entries.get(&key), Some(MemEntry::Dir)) {
			continue;
		}
		let content = read_member(&archive, m)?;
		entries.insert(key, MemEntry::File(content, m.modified));
	}
	Ok(MemFs {
		entries,
		read_only: true,
		label,
	})
}

/// The inflated data of a member, checked against its size and CRC-32.
fn read_member(archive: &rawzip::ZipSliceArchive<&[u8]>, m: &Member) -> FsResult<Vec<u8>> {
	let entry = archive.get_entry(m.wayfinder).map_err(zip_error)?;
	let size = m.wayfinder.uncompressed_size_hint();
	// One byte more than the size, so that the verifier sees too long data,
	// without inflating more than that.
	let limit = size.saturating_add(1);
	let mut content = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
	match m.method {
		CompressionMethod::STORE => entry
			.verifying_reader(entry.data().take(limit))
			.read_to_end(&mut content),
		CompressionMethod::DEFLATE => entry
			.verifying_reader(flate2::bufread::DeflateDecoder::new(entry.data()).take(limit))
			.read_to_end(&mut content),
		_ => {
			return Err(FsError::Io(std::io::Error::new(
				std::io::ErrorKind::Unsupported,
				"That compression method is not supported",
			)));
		}
	}
	.map_err(FsError::Io)?;
	Ok(content)
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
	let rz = |e: rawzip::Error| std::io::Error::new(std::io::ErrorKind::InvalidData, e);
	let earliest = from_timestamp(MIN_TIMESTAMP);
	let mut writer = ZipArchiveWriter::new(Vec::new());
	if let Some(dest) = dest_root {
		writer
			.new_dir(format!("{dest}/").as_str())
			.last_modified(earliest)
			.create()
			.map_err(rz)?;
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
			MemEntry::Dir => {
				writer
					.new_dir(format!("{name}/").as_str())
					.last_modified(earliest)
					.create()
					.map_err(rz)?;
			}
			MemEntry::File(data, t) => {
				let (mut file, config) = writer
					.new_file(name.as_str())
					.compression_method(CompressionMethod::DEFLATE)
					.last_modified(t.map_or(earliest, from_timestamp))
					.start()
					.map_err(rz)?;
				let encoder =
					flate2::write::DeflateEncoder::new(&mut file, flate2::Compression::default());
				let mut data_writer = config.wrap(encoder);
				data_writer.write_all(data)?;
				let (encoder, descriptor) = data_writer.finish().map_err(rz)?;
				encoder.finish()?;
				file.finish(descriptor).map_err(rz)?;
			}
		}
	}
	writer.finish().map_err(rz)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// An archive of `(name, data)` members, stored, with the names as they are.
	pub(crate) fn raw_zip(members: &[(&[u8], &[u8])]) -> Vec<u8> {
		let mut writer = ZipArchiveWriter::new(Vec::new());
		for &(name, data) in members {
			let (mut file, config) = writer
				.new_file(rawzip::path::EntryPath::verbatim(name))
				.start()
				.expect("entry");
			let mut w = config.wrap(&mut file);
			w.write_all(data).expect("data");
			let (_, descriptor) = w.finish().expect("finish");
			file.finish(descriptor).expect("finish entry");
		}
		writer.finish().expect("archive")
	}

	#[test]
	fn dates() {
		let dos = |t: f64| {
			let mut out = Vec::new();
			let mut writer = ZipArchiveWriter::new(&mut out);
			writer
				.new_dir("d/")
				.last_modified(from_timestamp(t))
				.create()
				.expect("dir");
			writer.finish().expect("archive");
			let archive = ZipArchive::from_slice(&out).expect("read");
			let entry = archive.entries().next_entry().expect("entry").expect("one");
			to_timestamp(entry.last_modified_dos())
		};
		assert_eq!(dos(1_000_000_000.0), Some(1_000_000_000.0));
		assert_eq!(dos(0.0), Some(MIN_TIMESTAMP));
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
			Some(MemEntry::File(d, Some(t))) if d == b"utf-8" && *t == MIN_TIMESTAMP
		));
		Ok(())
	}

	#[test]
	fn not_a_zip_file() {
		assert!(!is_zipfile(b"not a zip file, but long enough to be one"));
		assert!(read_zip(b"PK\x05\x06", String::new()).is_err());
	}

	#[test]
	fn back_references_are_rejected() {
		let data = raw_zip(&[(b"a.txt", b"x"), (b"../evil.txt", b"x")]);
		assert!(matches!(
			read_zip(&data, String::new()),
			Err(FsError::IllegalBackReference(name)) if name == "../evil.txt"
		));
	}

	#[test]
	fn cp437_names() -> FsResult<()> {
		let data = raw_zip(&[(b"\x82t\xe9.txt", b"x")]);
		let read = read_zip(&data, String::new())?;
		assert!(read.entries.contains_key("ét\u{398}.txt"));
		Ok(())
	}

	#[test]
	fn duplicates_read_as_the_last_member() -> FsResult<()> {
		let data = raw_zip(&[(b"a.txt", b"first"), (b"a.txt", b"second")]);
		let read = read_zip(&data, String::new())?;
		assert!(matches!(
			read.entries.get("a.txt"),
			Some(MemEntry::File(d, _)) if d == b"second"
		));
		Ok(())
	}

	#[test]
	fn directory_hides_a_file_of_the_same_name() -> FsResult<()> {
		let data = raw_zip(&[(b"node/child.txt", b"x"), (b"node", b"file")]);
		let read = read_zip(&data, String::new())?;
		assert!(matches!(read.entries.get("node"), Some(MemEntry::Dir)));
		Ok(())
	}

	#[test]
	fn file_in_the_way_of_a_directory() {
		let data = raw_zip(&[(b"node", b"file"), (b"node/child.txt", b"x")]);
		assert!(matches!(
			read_zip(&data, String::new()),
			Err(FsError::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists
		));
	}

	/// The members of an archive with a deflated `a.txt`, its local header
	/// (30 bytes and the name) at offset 0.
	fn deflated(data: &[u8]) -> Vec<u8> {
		let mut tree = MemFs::default();
		tree.entries
			.insert("a.txt".into(), MemEntry::File(data.to_vec(), None));
		write_zip(&tree, &[], None).expect("zip")
	}

	#[test]
	fn forged_sizes_are_rejected() {
		let mut data = deflated(&[0; 100]);
		// The uncompressed size in the central directory (offset 24 of the
		// header, which follows the local entry and its data descriptor).
		let cd = data
			.windows(4)
			.position(|w| w == [0x50, 0x4b, 0x01, 0x02])
			.expect("central directory");
		data[cd + 24..cd + 28].copy_from_slice(&0x7fff_ffffu32.to_le_bytes());
		assert!(matches!(
			read_zip(&data, String::new()),
			Err(FsError::Io(e)) if e.to_string().contains("ratio")
		));
		// The size is within the ratio, but it is not the data's.
		let mut data = deflated(&[0; 100]);
		data[cd + 24..cd + 28].copy_from_slice(&50u32.to_le_bytes());
		assert!(read_zip(&data, String::new()).is_err());
	}

	#[test]
	fn overlapping_members_are_rejected() {
		let data = raw_zip(&[(b"a.txt", b"x"), (b"b.txt", b"y")]);
		let read = read_zip(&data, String::new());
		assert!(read.is_ok());
		// Point the second central directory entry at the first local header.
		let mut data = data;
		let cds: Vec<usize> = data
			.windows(4)
			.enumerate()
			.filter(|(_, w)| *w == [0x50, 0x4b, 0x01, 0x02])
			.map(|(i, _)| i)
			.collect();
		let first = data[cds[0] + 42..cds[0] + 46].to_vec();
		data[cds[1] + 42..cds[1] + 46].copy_from_slice(&first);
		let err = read_zip(&data, String::new()).err().map(|e| e.to_string());
		assert!(
			err.as_ref().is_some_and(|e| e.contains("Overlapped")),
			"{err:?}"
		);
	}

	#[test]
	fn too_large_archives_are_rejected() {
		// A stored member that claims 2 GiB.
		let mut data = raw_zip(&[(b"a.txt", b"x")]);
		let cd = data
			.windows(4)
			.position(|w| w == [0x50, 0x4b, 0x01, 0x02])
			.expect("central directory");
		data[cd + 24..cd + 28].copy_from_slice(&0x8000_0000u32.to_le_bytes());
		assert!(matches!(
			read_zip(&data, String::new()),
			Err(FsError::Io(e)) if e.to_string().contains("inflates to more")
		));
	}
}
