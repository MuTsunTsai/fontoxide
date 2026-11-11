//! A minimal zip archive reader and writer for zipped UFOs (`.ufoz`), in
//! place of Python's `zipfile` module: stored and deflated entries, no
//! ZIP64, no encryption.
//!
//! Written archives hold the same entries as the ones `shutil.make_archive`
//! writes (a directory entry for each directory, deflated files), but the
//! compressed data and the timestamps differ.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use crate::fs::{FsError, FsResult, MemEntry, MemFs};

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;

fn u16_at(data: &[u8], pos: usize) -> Option<u16> {
	Some(u16::from_le_bytes(data.get(pos..pos + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], pos: usize) -> Option<u32> {
	Some(u32::from_le_bytes(data.get(pos..pos + 4)?.try_into().ok()?))
}

/// The position of the end of central directory record.
fn find_eocd(data: &[u8]) -> Option<usize> {
	if data.len() < 22 {
		return None;
	}
	let lowest = data.len().saturating_sub(22 + 0xffff);
	(lowest..=data.len() - 22)
		.rev()
		.find(|&pos| u32_at(data, pos) == Some(END_OF_CENTRAL_DIRECTORY))
}

/// `zipfile.is_zipfile`: whether the data has an end of central directory
/// record.
pub(crate) fn is_zipfile(data: &[u8]) -> bool {
	find_eocd(data).is_some()
}

/// Code page 437, the encoding of names without the UTF-8 flag.
const CP437_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

fn decode_cp437(bytes: &[u8]) -> String {
	let high: Vec<char> = CP437_HIGH.chars().collect();
	bytes
		.iter()
		.map(|&b| {
			if b < 0x80 {
				b as char
			} else {
				high.get(usize::from(b - 0x80))
					.copied()
					.unwrap_or('\u{fffd}')
			}
		})
		.collect()
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

/// The timestamp of a DOS date and time.
///
/// LENIENT(L35): `datetime(*zip_info.date_time).timestamp()` takes the time as
/// local time; here it is UTC.
fn dos_to_timestamp(date: u16, time: u16) -> f64 {
	let year = 1980 + i64::from(date >> 9);
	let month = i64::from((date >> 5) & 0xf).max(1);
	let day = i64::from(date & 0x1f).max(1);
	let hour = i64::from(time >> 11);
	let minute = i64::from((time >> 5) & 0x3f);
	let second = i64::from(time & 0x1f) * 2;
	(days_from_civil(year, month, day) * 86400 + hour * 3600 + minute * 60 + second) as f64
}

fn timestamp_to_dos(t: f64) -> (u16, u16) {
	let secs = t.max(315_532_800.0) as i64;
	let (y, m, d) = civil_from_days(secs.div_euclid(86400));
	let rem = secs.rem_euclid(86400);
	let date = (((y - 1980).clamp(0, 127) as u16) << 9) | ((m as u16) << 5) | d as u16;
	let time =
		(((rem / 3600) as u16) << 11) | ((((rem / 60) % 60) as u16) << 5) | ((rem % 60) / 2) as u16;
	(date, time)
}

fn bad_zip(message: &str) -> FsError {
	FsError::Io(std::io::Error::new(
		std::io::ErrorKind::InvalidData,
		message.to_owned(),
	))
}

/// Reads a zip archive into an in-memory tree (`ReadZipFS`).
pub(crate) fn read_zip(data: &[u8], label: String) -> FsResult<MemFs> {
	let eocd = find_eocd(data).ok_or_else(|| bad_zip("File is not a zip file"))?;
	let count = u16_at(data, eocd + 10).ok_or_else(|| bad_zip("Truncated zip file"))?;
	let cd_offset = u32_at(data, eocd + 16).ok_or_else(|| bad_zip("Truncated zip file"))? as usize;
	let mut entries: BTreeMap<String, MemEntry> = BTreeMap::new();
	let mut pos = cd_offset;
	for _ in 0..count {
		let field = |o: usize| u16_at(data, pos + o).ok_or_else(|| bad_zip("Truncated zip file"));
		if u32_at(data, pos) != Some(CENTRAL_HEADER) {
			return Err(bad_zip("Bad magic number for central directory"));
		}
		let flags = field(8)?;
		let method = field(10)?;
		let time = field(12)?;
		let date = field(14)?;
		let expected_crc = u32_at(data, pos + 16).ok_or_else(|| bad_zip("Truncated zip file"))?;
		let comp_size =
			u32_at(data, pos + 20).ok_or_else(|| bad_zip("Truncated zip file"))? as usize;
		let size = u32_at(data, pos + 24).ok_or_else(|| bad_zip("Truncated zip file"))? as usize;
		let name_len = usize::from(field(28)?);
		let extra_len = usize::from(field(30)?);
		let comment_len = usize::from(field(32)?);
		let local = u32_at(data, pos + 42).ok_or_else(|| bad_zip("Truncated zip file"))? as usize;
		let name_bytes = data
			.get(pos + 46..pos + 46 + name_len)
			.ok_or_else(|| bad_zip("Truncated zip file"))?;
		let name = if flags & 0x800 != 0 {
			String::from_utf8_lossy(name_bytes).into_owned()
		} else {
			decode_cp437(name_bytes)
		};
		pos += 46 + name_len + extra_len + comment_len;
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
		if u32_at(data, local) != Some(LOCAL_HEADER) {
			return Err(bad_zip("Bad magic number for file header"));
		}
		let lname =
			usize::from(u16_at(data, local + 26).ok_or_else(|| bad_zip("Truncated zip file"))?);
		let lextra =
			usize::from(u16_at(data, local + 28).ok_or_else(|| bad_zip("Truncated zip file"))?);
		let start = local + 30 + lname + lextra;
		let raw = data
			.get(start..start + comp_size)
			.ok_or_else(|| bad_zip("Truncated zip file"))?;
		let content = match method {
			0 => raw.to_vec(),
			8 => {
				let mut out = Vec::with_capacity(size);
				flate2::read::DeflateDecoder::new(raw)
					.read_to_end(&mut out)
					.map_err(FsError::Io)?;
				out
			}
			_ => return Err(bad_zip("That compression method is not supported")),
		};
		let mut crc = flate2::Crc::new();
		crc.update(&content);
		if crc.sum() != expected_crc {
			return Err(bad_zip(&format!(
				"Bad CRC-32 for file {}",
				fontoxide_misc::py::str_repr(&name)
			)));
		}
		entries.insert(
			parts.join("/"),
			MemEntry::File(content, Some(dos_to_timestamp(date, time))),
		);
	}
	Ok(MemFs {
		entries,
		read_only: true,
		label,
	})
}

/// The data and modification time of a file to archive.
type FileData<'a> = (&'a [u8], Option<f64>);

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
	let mut items: Vec<(String, Option<FileData<'_>>)> = Vec::new();
	if let Some(dest) = dest_root {
		items.push((format!("{dest}/"), None));
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
			MemEntry::Dir => items.push((format!("{name}/"), None)),
			MemEntry::File(data, t) => items.push((name, Some((data.as_slice(), *t)))),
		}
	}
	let mut out: Vec<u8> = Vec::new();
	let mut central: Vec<u8> = Vec::new();
	let mut count: u16 = 0;
	for (name, file) in items {
		let offset = out.len() as u32;
		let (data, t) = match file {
			Some((d, t)) => (d, t),
			None => (&[][..], None),
		};
		let (date, time) = timestamp_to_dos(t.unwrap_or(315_532_800.0));
		let is_dir = name.ends_with('/');
		let mut crc = flate2::Crc::new();
		crc.update(data);
		let crc = crc.sum();
		let (method, compressed) = if is_dir {
			(0u16, Vec::new())
		} else {
			let mut enc =
				flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
			enc.write_all(data)?;
			(8u16, enc.finish()?)
		};
		let name_bytes = name.as_bytes();
		let flags: u16 = if name.is_ascii() { 0 } else { 0x800 };
		let header = |sig: u32, central_dir: bool| {
			let mut h = Vec::new();
			h.extend_from_slice(&sig.to_le_bytes());
			if central_dir {
				h.extend_from_slice(&20u16.to_le_bytes()); // version made by
			}
			h.extend_from_slice(&20u16.to_le_bytes()); // version needed
			h.extend_from_slice(&flags.to_le_bytes());
			h.extend_from_slice(&method.to_le_bytes());
			h.extend_from_slice(&time.to_le_bytes());
			h.extend_from_slice(&date.to_le_bytes());
			h.extend_from_slice(&crc.to_le_bytes());
			h.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
			h.extend_from_slice(&(data.len() as u32).to_le_bytes());
			h.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
			h.extend_from_slice(&0u16.to_le_bytes()); // extra
			if central_dir {
				h.extend_from_slice(&0u16.to_le_bytes()); // comment
				h.extend_from_slice(&0u16.to_le_bytes()); // disk
				h.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
				let external: u32 = if is_dir { 0x41ed_0010 } else { 0x81a4_0000 };
				h.extend_from_slice(&external.to_le_bytes());
				h.extend_from_slice(&offset.to_le_bytes());
			}
			h.extend_from_slice(name_bytes);
			h
		};
		out.extend(header(LOCAL_HEADER, false));
		out.extend_from_slice(&compressed);
		central.extend(header(CENTRAL_HEADER, true));
		count = count.saturating_add(1);
	}
	let cd_offset = out.len() as u32;
	let cd_size = central.len() as u32;
	out.extend(central);
	out.extend_from_slice(&END_OF_CENTRAL_DIRECTORY.to_le_bytes());
	out.extend_from_slice(&[0, 0, 0, 0]);
	out.extend_from_slice(&count.to_le_bytes());
	out.extend_from_slice(&count.to_le_bytes());
	out.extend_from_slice(&cd_size.to_le_bytes());
	out.extend_from_slice(&cd_offset.to_le_bytes());
	out.extend_from_slice(&0u16.to_le_bytes());
	Ok(out)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn dates() {
		let (d, t) = timestamp_to_dos(1_000_000_000.0);
		assert_eq!(dos_to_timestamp(d, t), 1_000_000_000.0);
	}

	#[test]
	fn roundtrip() -> FsResult<()> {
		let mut tree = MemFs::default();
		tree.entries.insert("a".into(), MemEntry::Dir);
		tree.entries.insert(
			"a/b.txt".into(),
			MemEntry::File(b"hello hello hello".to_vec(), None),
		);
		let data = write_zip(&tree, &[], Some("x.ufo")).map_err(FsError::Io)?;
		assert!(is_zipfile(&data));
		let read = read_zip(&data, String::new())?;
		assert!(matches!(read.entries.get("x.ufo"), Some(MemEntry::Dir)));
		assert!(
			matches!(read.entries.get("x.ufo/a/b.txt"), Some(MemEntry::File(d, _)) if d == b"hello hello hello")
		);
		Ok(())
	}
}
