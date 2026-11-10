//! Port of `fontTools.ttLib.sfnt`: reading and writing the sfnt container (plain
//! sfnt, TrueType collections and WOFF 1.0). WOFF 2.0 lives in [`crate::woff2`].

use std::io::{Read as _, Write as _};
use std::sync::Arc;

use fontoxide_misc::Tag;

use crate::binary::{Reader, WriteBytes, calc_checksum, get_search_range, pad4};
use crate::{Error, Result};

/// Size of the sfnt header (`sfntDirectorySize`).
pub const SFNT_DIRECTORY_SIZE: usize = 12;
/// Size of an sfnt table record (`sfntDirectoryEntrySize`).
pub const SFNT_DIRECTORY_ENTRY_SIZE: usize = 16;
/// Size of the WOFF header (`woffDirectorySize`).
pub const WOFF_DIRECTORY_SIZE: usize = 44;
/// Size of a WOFF table record (`woffDirectoryEntrySize`).
pub const WOFF_DIRECTORY_ENTRY_SIZE: usize = 20;

/// Default zlib level for WOFF 1.0 tables and metadata (`ZLIB_COMPRESSION_LEVEL`).
pub const ZLIB_COMPRESSION_LEVEL: u32 = 6;

/// TTC header versions.
pub const TTC_V1: u32 = 0x0001_0000;
/// TTC version 1.1: version 1 plus a second offset table.
pub const TTC_V1_1: u32 = 0x0001_0001;
/// TTC version 2: adds the DSIG fields.
pub const TTC_V2: u32 = 0x0002_0000;
/// TTC version 2.1: version 2 plus a second offset table.
pub const TTC_V2_1: u32 = 0x0002_0001;

/// A web font flavor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flavor {
	/// WOFF 1.0.
	Woff,
	/// WOFF 2.0.
	Woff2,
}

impl Flavor {
	/// The name fontTools uses (`"woff"`, `"woff2"`).
	pub fn as_str(self) -> &'static str {
		match self {
			Flavor::Woff => "woff",
			Flavor::Woff2 => "woff2",
		}
	}
}

/// Metadata and private data of a WOFF file (`WOFFFlavorData`, and the WOFF 2.0
/// additions of `WOFF2FlavorData`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlavorData {
	/// The flavor this data belongs to.
	pub flavor: Option<Flavor>,
	/// WOFF major version.
	pub major_version: Option<u16>,
	/// WOFF minor version.
	pub minor_version: Option<u16>,
	/// Uncompressed extended metadata (XML).
	pub meta_data: Option<Vec<u8>>,
	/// Private data.
	pub priv_data: Option<Vec<u8>>,
	/// WOFF 2.0: the tables to transform (`transformedTables`). `None` means the
	/// default (`glyf` and `loca`).
	pub transformed_tables: Option<Vec<Tag>>,
}

impl FlavorData {
	/// Empty flavor data for `flavor`.
	pub fn new(flavor: Flavor) -> Self {
		FlavorData {
			flavor: Some(flavor),
			..Default::default()
		}
	}
}

/// A table record (`SFNTDirectoryEntry` / `WOFFDirectoryEntry`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
	/// The table tag.
	pub tag: Tag,
	/// The checksum of the (uncompressed) table data.
	pub check_sum: u32,
	/// The offset of the (possibly compressed) table data.
	pub offset: u32,
	/// The length of the (possibly compressed) table data.
	pub length: u32,
	/// WOFF: the uncompressed length.
	pub orig_length: u32,
	/// WOFF: the offset in the equivalent sfnt (used for the master checksum).
	pub orig_offset: u32,
}

/// The kind of container being read.
#[derive(Debug, Clone)]
enum Container {
	Sfnt,
	Woff,
	Woff2(Arc<crate::woff2::Woff2Data>),
}

/// Reads tables from an sfnt, TTC or WOFF/WOFF2 file (`SFNTReader`).
#[derive(Debug, Clone)]
pub struct SfntReader {
	file: Arc<[u8]>,
	/// The sfnt version: `\0\x01\0\0`, `OTTO` or `true`.
	pub sfnt_version: Tag,
	/// The web font flavor, if any.
	pub flavor: Option<Flavor>,
	/// WOFF metadata and private data.
	pub flavor_data: Option<FlavorData>,
	/// For collections, the number of fonts.
	pub num_fonts: Option<u32>,
	/// The table records, ordered by offset.
	entries: Vec<DirectoryEntry>,
	container: Container,
	check_checksums: u8,
}

/// The parsed header of a TrueType collection (`readTTCHeader`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtcHeader {
	/// `TTCTag`, always `ttcf`.
	pub ttc_tag: Tag,
	/// The header version.
	pub version: u32,
	/// The number of fonts.
	pub num_fonts: u32,
	/// The offsets of the fonts' table directories.
	pub offset_table: Vec<u32>,
	/// Version 2: the DSIG table tag, length and offset.
	pub dsig: Option<(u32, u32, u32)>,
	/// Versions 1.1 and 2.1: the second offset table.
	pub offset_table2: Option<Vec<u32>>,
}

/// Parses a TTC header (`readTTCHeader`).
pub fn read_ttc_header(data: &[u8]) -> Result<TtcHeader> {
	let not_enough = || Error::ttlib("Not a Font Collection (not enough data)");
	let mut r = Reader::new(data);
	let ttc_tag = r.tag().map_err(|_| not_enough())?;
	let version = r.u32().map_err(|_| not_enough())?;
	let num_fonts = r.u32().map_err(|_| not_enough())?;
	if ttc_tag != *b"ttcf" {
		return Err(Error::ttlib("Not a Font Collection"));
	}
	if ![TTC_V1, TTC_V1_1, TTC_V2, TTC_V2_1].contains(&version) {
		return Err(Error::Assertion(format!(
			"unrecognized TTC version 0x{version:08x}"
		)));
	}
	let offset_table = r.u32_array(num_fonts as usize)?;
	let mut header = TtcHeader {
		ttc_tag,
		version,
		num_fonts,
		offset_table,
		dsig: None,
		offset_table2: None,
	};
	if matches!(version, TTC_V2 | TTC_V2_1) {
		let a = r.u32().map_err(|_| not_enough())?;
		let b = r.u32().map_err(|_| not_enough())?;
		let c = r.u32().map_err(|_| not_enough())?;
		header.dsig = Some((a, b, c));
	}
	if matches!(version, TTC_V1_1 | TTC_V2_1) {
		let n = r.u32().map_err(|_| not_enough())?;
		header.offset_table2 = Some(r.u32_array(n as usize).map_err(|_| not_enough())?);
	}
	Ok(header)
}

/// Options for [`SfntReader::new`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SfntReaderOptions {
	/// How to treat wrong checksums (`checkChecksums`): 0 ignores them, 1 warns, 2
	/// fails.
	pub check_checksums: u8,
	/// The font in a collection to read (`fontNumber`); required for collections.
	pub font_number: Option<usize>,
	/// Use the original offset table of a version 1.1/2.1 collection
	/// (`_useLegacyTTC`).
	pub use_legacy_ttc: bool,
}

impl SfntReader {
	/// Parses the table directory of `file`.
	pub fn new(file: impl Into<Arc<[u8]>>, options: SfntReaderOptions) -> Result<Self> {
		let file: Arc<[u8]> = file.into();
		let signature = file.get(..4).unwrap_or_default();
		if signature == b"wOF2" {
			return crate::woff2::read(file, options);
		}

		let mut reader = SfntReader {
			file: file.clone(),
			sfnt_version: Tag::default(),
			flavor: None,
			flavor_data: None,
			num_fonts: None,
			entries: Vec::new(),
			container: Container::Sfnt,
			check_checksums: options.check_checksums,
		};
		let entry_size;
		let mut r;
		if signature == b"ttcf" {
			let header = read_ttc_header(&file)?;
			let offsets = match header.offset_table2 {
				Some(t) if !options.use_legacy_ttc => t,
				_ => header.offset_table,
			};
			let num_fonts = offsets.len();
			let font_number = options
				.font_number
				.filter(|&n| n < num_fonts)
				.ok_or_else(|| {
					Error::FileIsCollection(format!(
						"specify a font number between 0 and {} (inclusive)",
						num_fonts as i64 - 1
					))
				})?;
			reader.num_fonts = Some(num_fonts as u32);
			r = Reader::new(&file);
			r.seek(offsets[font_number] as usize);
			let header = r
				.bytes(SFNT_DIRECTORY_SIZE)
				.map_err(|_| Error::ttlib("Not a Font Collection (not enough data)"))?;
			reader.sfnt_version = Tag(header[..4].try_into().unwrap_or_default());
			entry_size = SFNT_DIRECTORY_ENTRY_SIZE;
			let num_tables = u16::from_be_bytes([header[4], header[5]]);
			reader.read_entries(&mut r, num_tables, entry_size)?;
		} else if signature == b"wOFF" {
			reader.flavor = Some(Flavor::Woff);
			reader.container = Container::Woff;
			r = Reader::new(&file);
			let header = r
				.bytes(WOFF_DIRECTORY_SIZE)
				.map_err(|_| Error::ttlib("Not a WOFF font (not enough data)"))?;
			let mut h = Reader::new(header);
			h.skip(4)?;
			reader.sfnt_version = h.tag()?;
			let _length = h.u32()?;
			let num_tables = h.u16()?;
			let _reserved = h.u16()?;
			let _total_sfnt_size = h.u32()?;
			let major_version = h.u16()?;
			let minor_version = h.u16()?;
			let meta_offset = h.u32()?;
			let meta_length = h.u32()?;
			let meta_orig_length = h.u32()?;
			let priv_offset = h.u32()?;
			let priv_length = h.u32()?;
			entry_size = WOFF_DIRECTORY_ENTRY_SIZE;
			reader.check_sfnt_version()?;
			reader.read_entries(&mut r, num_tables, entry_size)?;
			reader.flavor_data = Some(read_woff_flavor_data(
				&file,
				major_version,
				minor_version,
				(meta_offset, meta_length, meta_orig_length),
				(priv_offset, priv_length),
			)?);
			return Ok(reader);
		} else {
			r = Reader::new(&file);
			let header = r
				.bytes(SFNT_DIRECTORY_SIZE)
				.map_err(|_| Error::ttlib("Not a TrueType or OpenType font (not enough data)"))?;
			reader.sfnt_version = Tag(header[..4].try_into().unwrap_or_default());
			entry_size = SFNT_DIRECTORY_ENTRY_SIZE;
			let num_tables = u16::from_be_bytes([header[4], header[5]]);
			reader.check_sfnt_version()?;
			reader.read_entries(&mut r, num_tables, entry_size)?;
			return Ok(reader);
		}
		reader.check_sfnt_version()?;
		Ok(reader)
	}

	fn check_sfnt_version(&self) -> Result<()> {
		if ![*b"\0\x01\0\0", *b"OTTO", *b"true"].contains(&self.sfnt_version.0) {
			return Err(Error::ttlib(
				"Not a TrueType or OpenType font (bad sfntVersion)",
			));
		}
		Ok(())
	}

	fn read_entries(
		&mut self,
		r: &mut Reader<'_>,
		num_tables: u16,
		entry_size: usize,
	) -> Result<()> {
		let woff = entry_size == WOFF_DIRECTORY_ENTRY_SIZE;
		let mut entries: Vec<DirectoryEntry> = Vec::with_capacity(num_tables as usize);
		for _ in 0..num_tables {
			let data = r.bytes(entry_size).map_err(|_| {
				Error::ttlib(format!(
					"unexpected end of table directory: expected {entry_size} bytes but got {}",
					r.remaining()
				))
			})?;
			let mut e = Reader::new(data);
			let entry = if woff {
				let tag = e.tag()?;
				let offset = e.u32()?;
				let length = e.u32()?;
				let orig_length = e.u32()?;
				let check_sum = e.u32()?;
				DirectoryEntry {
					tag,
					check_sum,
					offset,
					length,
					orig_length,
					orig_offset: 0,
				}
			} else {
				let tag = e.tag()?;
				let check_sum = e.u32()?;
				let offset = e.u32()?;
				let length = e.u32()?;
				DirectoryEntry {
					tag,
					check_sum,
					offset,
					length,
					orig_length: length,
					orig_offset: 0,
				}
			};
			// A dict keyed by tag: a repeated tag replaces the earlier record but
			// keeps its position.
			match entries.iter_mut().find(|x| x.tag == entry.tag) {
				Some(existing) => *existing = entry,
				None => entries.push(entry),
			}
		}
		entries.sort_by_key(|e| e.offset);
		self.entries = entries;
		Ok(())
	}

	/// Creates a reader over already-decoded WOFF 2.0 data.
	pub(crate) fn from_woff2(
		file: Arc<[u8]>,
		sfnt_version: Tag,
		flavor_data: FlavorData,
		entries: Vec<DirectoryEntry>,
		data: Arc<crate::woff2::Woff2Data>,
		check_checksums: u8,
	) -> Self {
		SfntReader {
			file,
			sfnt_version,
			flavor: Some(Flavor::Woff2),
			flavor_data: Some(flavor_data),
			num_fonts: None,
			entries,
			container: Container::Woff2(data),
			check_checksums,
		}
	}

	/// The raw file data.
	pub fn file(&self) -> &Arc<[u8]> {
		&self.file
	}

	/// The table tags, ordered by offset (`keys()`).
	pub fn tags(&self) -> impl Iterator<Item = Tag> + '_ {
		self.entries.iter().map(|e| e.tag)
	}

	/// The table records, ordered by offset.
	pub fn entries(&self) -> &[DirectoryEntry] {
		&self.entries
	}

	/// Whether the font has the table.
	pub fn contains(&self, tag: Tag) -> bool {
		self.entries.iter().any(|e| e.tag == tag)
	}

	/// Removes a table from the directory (`__delitem__`).
	pub fn remove(&mut self, tag: Tag) -> bool {
		let len = self.entries.len();
		self.entries.retain(|e| e.tag != tag);
		self.entries.len() != len
	}

	/// The (uncompressed) data of a table (`__getitem__`).
	pub fn table_data(&self, tag: Tag) -> Result<TableData> {
		let entry = self
			.entries
			.iter()
			.find(|e| e.tag == tag)
			.ok_or(Error::TableNotFound(tag))?;
		let data = match &self.container {
			Container::Woff2(w) => w.table_data(entry, self)?,
			_ => {
				let start = entry.offset as usize;
				let end = start + entry.length as usize;
				if end > self.file.len() {
					return Err(Error::ttlib(format!(
						"unexpected end of '{tag}' table data: expected {} bytes but got {} at offset {}",
						entry.length,
						self.file
							.len()
							.saturating_sub(start)
							.min(entry.length as usize),
						entry.offset
					)));
				}
				match self.container {
					Container::Woff if entry.length != entry.orig_length => {
						TableData::owned(decode_woff_table(entry, &self.file[start..end])?)
					}
					_ => TableData {
						buf: self.file.clone(),
						start,
						end,
					},
				}
			}
		};
		// WOFF 2.0 has no table checksums.
		if self.check_checksums > 0 && !matches!(self.container, Container::Woff2(_)) {
			let checksum = if tag == *b"head" {
				head_checksum(&data)
			} else {
				calc_checksum(&data)
			};
			if checksum != entry.check_sum {
				if self.check_checksums > 1 {
					return Err(Error::Assertion(format!("bad checksum for '{tag}' table")));
				}
				log::warn!("bad checksum for '{tag}' table");
			}
		}
		Ok(data)
	}
}

/// Table data: a shared slice of the file, or decompressed bytes.
#[derive(Clone)]
pub struct TableData {
	buf: Arc<[u8]>,
	start: usize,
	end: usize,
}

impl TableData {
	/// Wraps owned bytes.
	pub fn owned(data: Vec<u8>) -> Self {
		let end = data.len();
		TableData {
			buf: data.into(),
			start: 0,
			end,
		}
	}
}

impl std::ops::Deref for TableData {
	type Target = [u8];
	fn deref(&self) -> &[u8] {
		&self.buf[self.start..self.end]
	}
}

impl std::fmt::Debug for TableData {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "TableData({} bytes)", self.end - self.start)
	}
}

/// The checksum of a `head` table, with `checkSumAdjustment` taken as zero.
pub fn head_checksum(data: &[u8]) -> u32 {
	if data.len() < 12 {
		return calc_checksum(data);
	}
	let mut copy = data.to_vec();
	copy[8..12].fill(0);
	calc_checksum(&copy)
}

fn decode_woff_table(entry: &DirectoryEntry, raw: &[u8]) -> Result<Vec<u8>> {
	if entry.length >= entry.orig_length {
		return Err(Error::ttlib(format!(
			"corrupt WOFF table '{}': compressed length {} is not smaller than original length {}",
			entry.tag, entry.length, entry.orig_length
		)));
	}
	let data = zlib_decompress(raw)?;
	if data.len() != entry.orig_length as usize {
		return Err(Error::ttlib(format!(
			"unexpected size for decompressed WOFF table '{}': expected {} but got {}",
			entry.tag,
			entry.orig_length,
			data.len()
		)));
	}
	Ok(data)
}

fn read_woff_flavor_data(
	file: &[u8],
	major_version: u16,
	minor_version: u16,
	(meta_offset, meta_length, meta_orig_length): (u32, u32, u32),
	(priv_offset, priv_length): (u32, u32),
) -> Result<FlavorData> {
	let mut data = FlavorData::new(Flavor::Woff);
	data.major_version = Some(major_version);
	data.minor_version = Some(minor_version);
	if meta_length != 0 {
		let raw = slice(file, meta_offset, meta_length).ok_or_else(|| {
			Error::ttlib(format!(
				"unexpected end of WOFF metadata: expected {meta_length} bytes but got {}",
				file.len().saturating_sub(meta_offset as usize)
			))
		})?;
		let meta = zlib_decompress(raw)?;
		if meta.len() != meta_orig_length as usize {
			return Err(Error::ttlib(format!(
				"unexpected size for decompressed WOFF metadata: expected {meta_orig_length} but got {}",
				meta.len()
			)));
		}
		data.meta_data = Some(meta);
	}
	if priv_length != 0 {
		let raw = slice(file, priv_offset, priv_length).ok_or_else(|| {
			Error::ttlib(format!(
				"unexpected end of WOFF private data: expected {priv_length} bytes but got {}",
				file.len().saturating_sub(priv_offset as usize)
			))
		})?;
		data.priv_data = Some(raw.to_vec());
	}
	Ok(data)
}

fn slice(file: &[u8], offset: u32, length: u32) -> Option<&[u8]> {
	let start = offset as usize;
	file.get(start..start.checked_add(length as usize)?)
}

/// zlib-decompresses data (`zlib.decompress`).
pub fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>> {
	let mut out = Vec::new();
	flate2::read::ZlibDecoder::new(data)
		.read_to_end(&mut out)
		.map_err(|e| Error::ttlib(format!("zlib error: {e}")))?;
	Ok(out)
}

/// zlib-compresses data at the given level (`compress`).
pub fn zlib_compress(data: &[u8], level: u32) -> Vec<u8> {
	let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(level));
	// Writing to a Vec cannot fail.
	let _ = encoder.write_all(data);
	encoder.finish().unwrap_or_default()
}

/// Writes an sfnt or WOFF 1.0 file (`SFNTWriter`). Tables are added with
/// [`SfntWriter::add`]; [`SfntWriter::close`] writes the directory and returns the
/// file.
#[derive(Debug)]
pub struct SfntWriter {
	file: Vec<u8>,
	/// Where the table directory goes; non-zero for fonts in a collection.
	directory_offset: usize,
	num_tables: usize,
	sfnt_version: Tag,
	flavor: Option<Flavor>,
	flavor_data: Option<FlavorData>,
	next_table_offset: usize,
	orig_next_table_offset: u32,
	head_table: Option<Vec<u8>>,
	tables: Vec<DirectoryEntry>,
	woff2: Option<crate::woff2::Woff2Writer>,
}

impl SfntWriter {
	/// A writer for `num_tables` tables.
	pub fn new(
		num_tables: usize,
		sfnt_version: Tag,
		flavor: Option<Flavor>,
		flavor_data: Option<FlavorData>,
	) -> Self {
		Self::with_buffer(Vec::new(), num_tables, sfnt_version, flavor, flavor_data)
	}

	/// A writer that appends to `file`, as fontTools does when saving the fonts of
	/// a collection into one stream. Table offsets are relative to the start of
	/// `file`.
	pub fn with_buffer(
		mut file: Vec<u8>,
		num_tables: usize,
		sfnt_version: Tag,
		flavor: Option<Flavor>,
		flavor_data: Option<FlavorData>,
	) -> Self {
		let directory_size = match flavor {
			Some(Flavor::Woff) => WOFF_DIRECTORY_SIZE + num_tables * WOFF_DIRECTORY_ENTRY_SIZE,
			_ => SFNT_DIRECTORY_SIZE + num_tables * SFNT_DIRECTORY_ENTRY_SIZE,
		};
		let woff2 = (flavor == Some(Flavor::Woff2))
			.then(|| crate::woff2::Woff2Writer::new(num_tables, sfnt_version, flavor_data.clone()));
		let directory_offset = file.len();
		file.resize(directory_offset + directory_size, 0);
		SfntWriter {
			file,
			directory_offset,
			num_tables,
			sfnt_version,
			flavor,
			flavor_data,
			next_table_offset: directory_offset + directory_size,
			orig_next_table_offset: (SFNT_DIRECTORY_SIZE + num_tables * SFNT_DIRECTORY_ENTRY_SIZE)
				as u32,
			head_table: None,
			tables: Vec::new(),
			woff2,
		}
	}

	/// Whether the writer orders tables itself, so that the result must not be
	/// reordered afterwards (`reordersTables`).
	pub fn reorders_tables(&self) -> bool {
		self.woff2.is_some()
	}

	/// Sets the Brotli quality and the progress callback of WOFF 2.0 output; see
	/// [`crate::WriteOptions`]. Does nothing for other flavors.
	pub fn set_woff2_compression(&mut self, quality: u32, progress: Option<crate::font::Progress>) {
		if let Some(w) = &mut self.woff2 {
			w.quality = quality;
			w.progress = progress;
		}
	}

	/// Adds a directory entry for table data that is already in the file, written
	/// for another font of the collection (`setEntry`).
	pub fn set_entry(&mut self, entry: DirectoryEntry) -> Result<()> {
		if self.tables.iter().any(|e| e.tag == entry.tag) {
			return Err(Error::ttlib(format!(
				"cannot rewrite '{}' table",
				entry.tag
			)));
		}
		self.tables.push(entry);
		Ok(())
	}

	/// The entry of a table added so far (`__getitem__`).
	pub fn entry(&self, tag: Tag) -> Option<&DirectoryEntry> {
		self.tables.iter().find(|e| e.tag == tag)
	}

	/// Adds a table (`__setitem__`).
	pub fn add(&mut self, tag: Tag, data: &[u8]) -> Result<()> {
		if let Some(w) = &mut self.woff2 {
			return w.add(tag, data);
		}
		if self.tables.iter().any(|e| e.tag == tag) {
			return Err(Error::ttlib(format!("cannot rewrite '{tag}' table")));
		}
		let is_head = tag == *b"head";
		let check_sum = if is_head {
			self.head_table = Some(data.to_vec());
			head_checksum(data)
		} else {
			calc_checksum(data)
		};
		let mut entry = DirectoryEntry {
			tag,
			check_sum,
			offset: self.next_table_offset as u32,
			length: data.len() as u32,
			orig_length: data.len() as u32,
			orig_offset: 0,
		};
		let stored;
		let raw: &[u8] = if self.flavor == Some(Flavor::Woff) {
			// Compress unless that does not help; 'head' is always stored raw.
			let compressed = (!is_head).then(|| zlib_compress(data, ZLIB_COMPRESSION_LEVEL));
			match compressed {
				Some(c) if c.len() < data.len() => {
					stored = c;
					entry.length = stored.len() as u32;
					&stored
				}
				_ => data,
			}
		} else {
			data
		};
		debug_assert_eq!(self.file.len(), self.next_table_offset);
		self.file.extend_from_slice(raw);
		if self.flavor == Some(Flavor::Woff) {
			entry.orig_offset = self.orig_next_table_offset;
			self.orig_next_table_offset += (entry.orig_length + 3) & !3;
		}
		self.next_table_offset += (entry.length as usize + 3) & !3;
		self.file.resize(self.next_table_offset, 0);
		self.tables.push(entry);
		Ok(())
	}

	/// Writes the directory and returns the file (`close`).
	pub fn close(mut self) -> Result<Vec<u8>> {
		if let Some(w) = self.woff2.take() {
			return w.close();
		}
		let mut tables = std::mem::take(&mut self.tables);
		tables.sort_by_key(|e| e.tag);
		if tables.len() != self.num_tables {
			return Err(Error::ttlib(format!(
				"wrong number of tables; expected {}, found {}",
				self.num_tables,
				tables.len()
			)));
		}
		let mut directory = Vec::new();
		if self.flavor == Some(Flavor::Woff) {
			let mut total_sfnt_size: u32 = 12 + 16 * tables.len() as u32;
			for e in &tables {
				total_sfnt_size += (e.orig_length + 3) & !3;
			}
			let data = self.flavor_data.clone().unwrap_or_default();
			let (major, minor) = match (data.major_version, data.minor_version) {
				(Some(a), Some(b)) => (a, b),
				_ => match &self.head_table {
					Some(h) if h.len() >= 8 => (
						u16::from_be_bytes([h[4], h[5]]),
						u16::from_be_bytes([h[6], h[7]]),
					),
					_ => (0, 0),
				},
			};
			let (meta_offset, meta_length, meta_orig_length) = match &data.meta_data {
				Some(meta) if !meta.is_empty() => {
					let offset = self.file.len() as u32;
					let compressed = zlib_compress(meta, ZLIB_COMPRESSION_LEVEL);
					self.file.extend_from_slice(&compressed);
					(offset, compressed.len() as u32, meta.len() as u32)
				}
				_ => (0, 0, 0),
			};
			let (priv_offset, priv_length) = match &data.priv_data {
				Some(p) if !p.is_empty() => {
					pad4(&mut self.file);
					let offset = self.file.len() as u32;
					self.file.extend_from_slice(p);
					(offset, p.len() as u32)
				}
				_ => (0, 0),
			};
			let length = self.file.len() as u32;
			directory.extend_from_slice(b"wOFF");
			directory.extend_from_slice(&self.sfnt_version.0);
			directory.put_u32(length);
			directory.put_u16(tables.len() as u16);
			directory.put_u16(0);
			directory.put_u32(total_sfnt_size);
			directory.put_u16(major);
			directory.put_u16(minor);
			directory.put_u32(meta_offset);
			directory.put_u32(meta_length);
			directory.put_u32(meta_orig_length);
			directory.put_u32(priv_offset);
			directory.put_u32(priv_length);
			for e in &tables {
				directory.extend_from_slice(&e.tag.0);
				directory.put_u32(e.offset);
				directory.put_u32(e.length);
				directory.put_u32(e.orig_length);
				directory.put_u32(e.check_sum);
			}
		} else {
			directory = sfnt_directory(self.sfnt_version, &tables, false);
		}
		let at = self.directory_offset;
		self.file[at..at + directory.len()].copy_from_slice(&directory);
		if let Some(head) = tables.iter().find(|e| e.tag == *b"head") {
			let adjustment = self.master_checksum(&tables, &directory);
			let at = head.offset as usize + 8;
			self.file[at..at + 4].copy_from_slice(&adjustment.to_be_bytes());
		}
		Ok(self.file)
	}

	fn master_checksum(&self, tables: &[DirectoryEntry], directory: &[u8]) -> u32 {
		let mut checksum: u32 = 0;
		for e in tables {
			checksum = checksum.wrapping_add(e.check_sum);
		}
		let directory = if self.flavor == Some(Flavor::Woff) {
			sfnt_directory(self.sfnt_version, tables, true)
		} else {
			directory.to_vec()
		};
		checksum = checksum.wrapping_add(calc_checksum(&directory));
		0xB1B0_AFBA_u32.wrapping_sub(checksum)
	}
}

/// The sfnt table directory for `tables` (sorted by tag). With `orig`, the WOFF
/// original offsets and lengths are used.
pub(crate) fn sfnt_directory(sfnt_version: Tag, tables: &[DirectoryEntry], orig: bool) -> Vec<u8> {
	let mut d = Vec::with_capacity(SFNT_DIRECTORY_SIZE + tables.len() * SFNT_DIRECTORY_ENTRY_SIZE);
	let (search_range, entry_selector, range_shift) = get_search_range(tables.len() as u32, 16);
	d.extend_from_slice(&sfnt_version.0);
	d.put_u16(tables.len() as u16);
	d.put_u16(search_range as u16);
	d.put_u16(entry_selector as u16);
	d.put_u16(range_shift as u16);
	for e in tables {
		d.extend_from_slice(&e.tag.0);
		d.put_u32(e.check_sum);
		if orig {
			d.put_u32(e.orig_offset);
			d.put_u32(e.orig_length);
		} else {
			d.put_u32(e.offset);
			d.put_u32(e.length);
		}
	}
	d
}

/// Rewrites a font with its tables in the given order, or in the order the
/// OpenType specification recommends (`reorderFontTables`).
pub fn reorder_font_tables(data: Vec<u8>, table_order: Option<&[Tag]>) -> Result<Vec<u8>> {
	let reader = SfntReader::new(data, SfntReaderOptions::default())?;
	let tags: Vec<Tag> = reader.tags().collect();
	let mut writer = SfntWriter::new(
		tags.len(),
		reader.sfnt_version,
		reader.flavor,
		reader.flavor_data.clone(),
	);
	for tag in crate::font::sorted_tag_list(&tags, table_order) {
		writer.add(tag, &reader.table_data(tag)?)?;
	}
	writer.close()
}
