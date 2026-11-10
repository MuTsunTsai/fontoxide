//! WOFF 2.0 (`fontTools.ttLib.woff2`).
//!
//! Reading decompresses the font data and rebuilds the transformed `glyf`,
//! `loca` and `hmtx` tables on demand; writing transforms and compresses them.
//!
//! The Brotli compressor is the pure-Rust `brotli` crate. It produces the same
//! bytes as the C library that fontTools uses for small inputs, but not always
//! for larger ones; the decompressed data is identical.

use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};

use fontoxide_misc::Tag;
use fontoxide_misc::array_tools::calc_bounds;
use fontoxide_misc::round_tools::ot_round;

use crate::binary::{WriteBytes, calc_checksum, pad4};
use crate::font::Progress;
use crate::sfnt::{
	DirectoryEntry, Flavor, FlavorData, SFNT_DIRECTORY_ENTRY_SIZE, SFNT_DIRECTORY_SIZE, SfntReader,
	SfntReaderOptions, TableData, sfnt_directory,
};
use crate::table::{Table, TableType};
use crate::tables::glyf::{FLAG_CUBIC, FLAG_ON_CURVE, FLAG_OVERLAP_SIMPLE, Glyph, GlyphComponent};
use crate::tables::metrics::Hmtx;
use crate::tables::tt_program::Program;
use crate::tables::{Glyf, Head, Hhea, Loca, Maxp};
use crate::{Error, Font, Result};

/// The size of the WOFF 2.0 header (`woff2DirectorySize`).
const WOFF2_DIRECTORY_SIZE: usize = 48;

/// The tags with a one-byte code in the table directory (`woff2KnownTags`).
const KNOWN_TAGS: [&[u8; 4]; 63] = [
	b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post", b"cvt ", b"fpgm",
	b"glyf", b"loca", b"prep", b"CFF ", b"VORG", b"EBDT", b"EBLC", b"gasp", b"hdmx", b"kern",
	b"LTSH", b"PCLT", b"VDMX", b"vhea", b"vmtx", b"BASE", b"GDEF", b"GPOS", b"GSUB", b"EBSC",
	b"JSTF", b"MATH", b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar",
	b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar", b"gvar", b"hsty",
	b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop", b"trak", b"Zapf", b"Silf", b"Glat",
	b"Gloc", b"Feat", b"Sill",
];

/// The flags value of a tag that is not one of [`KNOWN_TAGS`].
const UNKNOWN_TAG_INDEX: u8 = 0x3F;

/// The size of the header of the transformed `glyf` table.
const GLYF_HEADER_SIZE: usize = 36;

/// `optionFlags`: the transformed `glyf` table has an overlapSimpleBitmap.
const OVERLAP_SIMPLE_BITMAP_FLAG: u16 = 0x0001;

const GLYF: Tag = Tag::new(b"glyf");
const LOCA: Tag = Tag::new(b"loca");
const HMTX: Tag = Tag::new(b"hmtx");
const HEAD: Tag = Tag::new(b"head");
const DSIG: Tag = Tag::new(b"DSIG");

/// `getKnownTagIndex`.
fn known_tag_index(tag: Tag) -> u8 {
	KNOWN_TAGS
		.iter()
		.position(|t| **t == tag.0)
		.map_or(UNKNOWN_TAG_INDEX, |i| i as u8)
}

// --- Variable-length numbers

/// `unpackBase128`: a UIntBase128 number and the bytes read.
pub fn unpack_base128(data: &[u8]) -> Result<(u32, usize)> {
	let not_enough = || Error::ttlib("not enough data to unpack UIntBase128");
	let first = *data.first().ok_or_else(not_enough)?;
	if first == 0x80 {
		return Err(Error::ttlib(
			"UIntBase128 value must not start with leading zeros",
		));
	}
	let mut result: u32 = 0;
	for i in 0..5 {
		let code = *data.get(i).ok_or_else(not_enough)?;
		if result & 0xFE00_0000 != 0 {
			return Err(Error::ttlib("UIntBase128 value exceeds 2**32-1"));
		}
		result = (result << 7) | (code & 0x7F) as u32;
		if code & 0x80 == 0 {
			return Ok((result, i + 1));
		}
	}
	Err(Error::ttlib(
		"UIntBase128-encoded sequence is longer than 5 bytes",
	))
}

/// `packBase128`: the shortest UIntBase128 encoding.
pub fn pack_base128(n: u32) -> Vec<u8> {
	let mut size = 1;
	let mut m = n;
	while m >= 128 {
		size += 1;
		m >>= 7;
	}
	(0..size)
		.map(|i| {
			let b = ((n >> (7 * (size - i - 1))) & 0x7F) as u8;
			if i < size - 1 { b | 0x80 } else { b }
		})
		.collect()
}

/// `unpack255UShort`: a 255UInt16 number and the bytes read.
pub fn unpack_255_ushort(data: &[u8]) -> Result<(u16, usize)> {
	let not_enough = || Error::ttlib("not enough data to unpack 255UInt16");
	let code = *data.first().ok_or_else(|| {
		Error::ttlib("TypeError: ord() expected a character, but string of length 0 found")
	})?;
	Ok(match code {
		253 => {
			let b = data.get(1..3).ok_or_else(not_enough)?;
			(u16::from_be_bytes([b[0], b[1]]), 3)
		}
		254 => (*data.get(1).ok_or_else(not_enough)? as u16 + 506, 2),
		255 => (*data.get(1).ok_or_else(not_enough)? as u16 + 253, 2),
		c => (c as u16, 1),
	})
}

/// `pack255UShort`.
pub fn pack_255_ushort(value: i64) -> Result<Vec<u8>> {
	if !(0..=0xFFFF).contains(&value) {
		return Err(Error::ttlib(
			"255UInt16 format requires 0 <= integer <= 65535",
		));
	}
	Ok(if value < 253 {
		vec![value as u8]
	} else if value < 506 {
		vec![255, (value - 253) as u8]
	} else if value < 762 {
		vec![254, (value - 506) as u8]
	} else {
		let v = (value as u16).to_be_bytes();
		vec![253, v[0], v[1]]
	})
}

// --- Brotli

fn brotli_decompress(data: &[u8]) -> Result<Vec<u8>> {
	let mut out = Vec::new();
	brotli::BrotliDecompress(&mut &data[..], &mut out)
		.map_err(|e| Error::ttlib(format!("brotli error: {e}")))?;
	Ok(out)
}

/// A reader that reports the fraction of `data` read so far.
struct ProgressReader<'a> {
	data: &'a [u8],
	read: usize,
	progress: &'a Progress,
}

impl Read for ProgressReader<'_> {
	fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
		let n = (&self.data[self.read..]).read(buf)?;
		self.read += n;
		(self.progress)(self.read as f64 / self.data.len().max(1) as f64);
		Ok(n)
	}
}

/// Compresses `data` (the font data or, if not `font`, the metadata), reporting the
/// fraction read to `progress`.
fn brotli_compress(
	data: &[u8],
	font: bool,
	quality: u32,
	progress: Option<&Progress>,
) -> Result<Vec<u8>> {
	use brotli::enc::backward_references::BrotliEncoderMode;
	let params = brotli::enc::BrotliEncoderParams {
		quality: quality.min(11) as i32,
		lgwin: 22,
		mode: if font {
			BrotliEncoderMode::BROTLI_MODE_FONT
		} else {
			BrotliEncoderMode::BROTLI_MODE_TEXT
		},
		..Default::default()
	};
	let mut out = Vec::new();
	match progress {
		None => brotli::BrotliCompress(&mut &data[..], &mut out, &params),
		Some(progress) => {
			progress(0.0);
			let mut reader = ProgressReader {
				data,
				read: 0,
				progress,
			};
			brotli::BrotliCompress(&mut reader, &mut out, &params)
		}
	}
	.map_err(|e| Error::ttlib(format!("brotli error: {e}")))?;
	if let Some(progress) = progress {
		progress(1.0);
	}
	Ok(out)
}

// --- Directory entries (`WOFF2DirectoryEntry`)

/// A table of the WOFF 2.0 directory.
#[derive(Debug, Clone)]
struct Entry {
	tag: Tag,
	flags: u8,
	/// The offset in the decompressed data.
	offset: usize,
	/// The length in the decompressed data, transformed or not.
	length: usize,
	orig_length: u32,
}

impl Entry {
	/// Whether the table is transformed: transform version 0 for `glyf` and
	/// `loca`, any but 0 for other tables.
	fn transformed(&self) -> bool {
		let version = self.flags >> 6;
		if self.tag == GLYF || self.tag == LOCA {
			version != 3
		} else {
			version != 0
		}
	}

	fn set_transformed(&mut self, transformed: bool) {
		let version = if self.tag == GLYF || self.tag == LOCA {
			if transformed { 0 } else { 3 }
		} else {
			transformed as u8
		};
		self.flags |= version << 6;
	}

	/// `fromString`: the entry and the bytes read.
	fn read(data: &[u8]) -> Result<(Self, usize)> {
		let flags = *data
			.first()
			.ok_or_else(|| Error::ttlib("can't read table 'flags': not enough data"))?;
		let mut pos = 1;
		let tag = if flags & 0x3F == 0x3F {
			let b = data
				.get(1..5)
				.ok_or_else(|| Error::ttlib("can't read table 'tag': not enough data"))?;
			pos += 4;
			Tag([b[0], b[1], b[2], b[3]])
		} else {
			Tag(*KNOWN_TAGS[(flags & 0x3F) as usize])
		};
		let (orig_length, n) = unpack_base128(&data[pos..])?;
		pos += n;
		let mut entry = Entry {
			tag,
			flags,
			offset: 0,
			length: orig_length as usize,
			orig_length,
		};
		if entry.transformed() {
			let (length, n) = unpack_base128(&data[pos..])?;
			pos += n;
			entry.length = length as usize;
			if tag == LOCA && length != 0 {
				return Err(Error::ttlib(
					"the transformLength of the 'loca' table must be 0",
				));
			}
		}
		Ok((entry, pos))
	}

	/// `toString`.
	fn to_bytes(&self) -> Vec<u8> {
		let mut data = vec![self.flags];
		if self.flags & 0x3F == 0x3F {
			data.extend_from_slice(&self.tag.0);
		}
		data.extend(pack_base128(self.orig_length));
		if self.transformed() {
			data.extend(pack_base128(self.length as u32));
		}
		data
	}
}

// --- Reading (`WOFF2Reader`)

/// A transformed `glyf` table, decoded (`WOFF2GlyfTable.reconstruct`).
#[derive(Debug, Clone)]
struct DecodedGlyf {
	glyf: Vec<u8>,
	locations: Vec<u32>,
	index_format: u16,
	/// The `xMin` of each glyph, 0 for empty glyphs.
	x_mins: Vec<i64>,
}

/// The decompressed table data of a WOFF 2.0 file.
#[derive(Debug)]
pub struct Woff2Data {
	stream: Vec<u8>,
	entries: Vec<Entry>,
	/// Rebuilt tables (`entry.data`).
	cache: Mutex<Cache>,
}

#[derive(Debug, Default)]
struct Cache {
	tables: HashMap<Tag, Vec<u8>>,
	glyf: Option<DecodedGlyf>,
}

/// Reads a WOFF 2.0 file (`WOFF2Reader`).
pub(crate) fn read(file: Arc<[u8]>, options: SfntReaderOptions) -> Result<SfntReader> {
	let header = file
		.get(..WOFF2_DIRECTORY_SIZE)
		.ok_or_else(|| Error::ttlib("Not a WOFF2 font (not enough data)"))?;
	if &header[..4] != b"wOF2" {
		return Err(Error::ttlib("Not a WOFF2 font (bad signature)"));
	}
	let u32_at =
		|i: usize| u32::from_be_bytes([header[i], header[i + 1], header[i + 2], header[i + 3]]);
	let u16_at = |i: usize| u16::from_be_bytes([header[i], header[i + 1]]);
	let sfnt_version = Tag([header[4], header[5], header[6], header[7]]);
	let length = u32_at(8);
	let num_tables = u16_at(12);
	let total_compressed_size = u32_at(20) as usize;
	let major_version = u16_at(24);
	let minor_version = u16_at(26);
	let (meta_offset, meta_length, meta_orig_length) = (u32_at(28), u32_at(32), u32_at(36));
	let (priv_offset, priv_length) = (u32_at(40), u32_at(44));

	let mut pos = WOFF2_DIRECTORY_SIZE;
	let mut entries: Vec<Entry> = Vec::new();
	let mut offset = 0;
	for _ in 0..num_tables {
		// `fromFile` reads at most the largest entry.
		let end = (pos + 15).min(file.len());
		let (mut entry, used) = Entry::read(&file[pos.min(end)..end])?;
		pos += used;
		entry.offset = offset;
		offset += entry.length;
		// A dict keyed by tag: a repeated tag replaces the earlier entry but keeps
		// its position.
		match entries.iter_mut().find(|e| e.tag == entry.tag) {
			Some(e) => *e = entry,
			None => entries.push(entry),
		}
	}
	let total_uncompressed_size = offset;
	let compressed = &file[pos.min(file.len())..(pos + total_compressed_size).min(file.len())];
	let stream = brotli_decompress(compressed)?;
	if stream.len() != total_uncompressed_size {
		return Err(Error::ttlib(format!(
			"unexpected size for decompressed font data: expected {total_uncompressed_size}, found {}",
			stream.len()
		)));
	}
	if length as usize != file.len() {
		return Err(Error::ttlib(
			"reported 'length' doesn't match the actual file size",
		));
	}

	// `WOFF2FlavorData(reader)`.
	let mut flavor_data = FlavorData::new(Flavor::Woff2);
	flavor_data.major_version = Some(major_version);
	flavor_data.minor_version = Some(minor_version);
	if meta_length != 0 {
		let raw = slice(&file, meta_offset, meta_length).ok_or_else(|| {
			Error::ttlib(format!(
				"unexpected end of WOFF metadata: expected {meta_length} bytes but got {}",
				file.len()
					.saturating_sub(meta_offset as usize)
					.min(meta_length as usize)
			))
		})?;
		let meta = brotli_decompress(raw)?;
		if meta.len() != meta_orig_length as usize {
			return Err(Error::ttlib(format!(
				"unexpected size for decompressed WOFF metadata: expected {meta_orig_length} but got {}",
				meta.len()
			)));
		}
		flavor_data.meta_data = Some(meta);
	}
	if priv_length != 0 {
		let raw = slice(&file, priv_offset, priv_length).ok_or_else(|| {
			Error::ttlib(format!(
				"unexpected end of WOFF private data: expected {priv_length} bytes but got {}",
				file.len()
					.saturating_sub(priv_offset as usize)
					.min(priv_length as usize)
			))
		})?;
		flavor_data.priv_data = Some(raw.to_vec());
	}
	flavor_data.transformed_tables = Some(
		entries
			.iter()
			.filter(|e| e.transformed())
			.map(|e| e.tag)
			.collect(),
	);

	let directory = entries
		.iter()
		.map(|e| DirectoryEntry {
			tag: e.tag,
			check_sum: 0,
			offset: e.offset as u32,
			length: e.length as u32,
			orig_length: e.orig_length,
			orig_offset: 0,
		})
		.collect();
	let data = Woff2Data {
		stream,
		entries,
		cache: Mutex::new(Cache::default()),
	};
	Ok(SfntReader::from_woff2(
		file,
		sfnt_version,
		flavor_data,
		directory,
		Arc::new(data),
		options.check_checksums,
	))
}

fn slice(file: &[u8], offset: u32, length: u32) -> Option<&[u8]> {
	let start = offset as usize;
	file.get(start..start.checked_add(length as usize)?)
}

/// Generated glyph names, as a font without names gets them.
fn glyph_names(count: usize) -> Vec<String> {
	let mut names = Vec::with_capacity(count);
	names.push(".notdef".to_owned());
	names.extend((1..count).map(|i| format!("glyph{i:05}")));
	names.truncate(count);
	names
}

/// A font to decompile and compile tables in, without recalculations (the
/// `ttFont` of `WOFF2Reader` and `WOFF2Writer`).
fn scratch_font() -> Font {
	let mut font = Font::new();
	font.recalc_bboxes = false;
	font.recalc_timestamp = false;
	font
}

impl Woff2Data {
	fn entry(&self, tag: Tag) -> Result<&Entry> {
		self.entries
			.iter()
			.find(|e| e.tag == tag)
			.ok_or(Error::TableNotFound(tag))
	}

	/// The data of a table as stored (`loadData`).
	fn raw(&self, tag: Tag) -> Result<&[u8]> {
		let e = self.entry(tag)?;
		self.stream
			.get(e.offset..e.offset + e.length)
			.ok_or_else(|| Error::Assertion("len(data) == self.length".into()))
	}

	/// The data of a table, rebuilt if it is transformed (`__getitem__`).
	pub(crate) fn table_data(
		&self,
		entry: &DirectoryEntry,
		_reader: &SfntReader,
	) -> Result<TableData> {
		self.get(entry.tag).map(TableData::owned)
	}

	fn get(&self, tag: Tag) -> Result<Vec<u8>> {
		let entry = self.entry(tag)?;
		if !entry.transformed() {
			return Ok(self.raw(tag)?.to_vec());
		}
		if let Some(data) = self.lock().tables.get(&tag) {
			return Ok(data.clone());
		}
		let data = if tag == GLYF {
			self.decoded_glyf()?.glyf
		} else if tag == LOCA {
			let glyf = self.decoded_glyf()?;
			let data = compile_woff2_loca(&glyf.locations, glyf.index_format)?;
			if data.len() != entry.orig_length as usize {
				return Err(Error::ttlib(format!(
					"reconstructed 'loca' table doesn't match original size: expected {}, found {}",
					entry.orig_length,
					data.len()
				)));
			}
			data
		} else if tag == HMTX {
			self.reconstruct_hmtx()?
		} else {
			return Err(Error::ttlib(format!(
				"transform for table '{tag}' is unknown"
			)));
		};
		self.lock().tables.insert(tag, data.clone());
		Ok(data)
	}

	fn lock(&self) -> std::sync::MutexGuard<'_, Cache> {
		self.cache.lock().unwrap_or_else(|e| e.into_inner())
	}

	/// The transformed `glyf` table, decoded and compiled (`_reconstructGlyf`).
	fn decoded_glyf(&self) -> Result<DecodedGlyf> {
		if let Some(g) = &self.lock().glyf {
			return Ok(g.clone());
		}
		let (glyphs, index_format) = decode_glyf(self.raw(GLYF)?)?;
		let x_mins = glyphs
			.iter()
			.map(|g| {
				if g.number_of_contours == 0 {
					0
				} else {
					g.x_min
				}
			})
			.collect();
		let mut font = scratch_font();
		font.set_glyph_order(glyph_names(glyphs.len()));
		font.insert(Box::new(Loca::default()));
		let mut glyf = Glyf {
			glyphs: glyphs
				.into_iter()
				.enumerate()
				.map(|(i, g)| (i as u32, g))
				.collect(),
			..Default::default()
		};
		let data = glyf.compile(&mut font)?;
		let locations = font.require::<Loca>()?.locations().to_vec();
		let decoded = DecodedGlyf {
			glyf: data,
			locations,
			index_format,
			x_mins,
		};
		self.lock().glyf = Some(decoded.clone());
		Ok(decoded)
	}

	/// `_reconstructHmtx` and `WOFF2HmtxTable.reconstruct`.
	fn reconstruct_hmtx(&self) -> Result<Vec<u8>> {
		let mut font = scratch_font();
		let maxp = Maxp::decompile(&self.get(Tag::new(b"maxp"))?, &mut font)?;
		let num_names = maxp.num_glyphs as usize;
		font.insert(Box::new(maxp));
		let glyf_transformed = self.entry(GLYF).is_ok_and(Entry::transformed);
		if !glyf_transformed {
			let head = Head::decompile(&self.get(HEAD)?, &mut font)?;
			font.insert(Box::new(head));
		}
		let hhea = Hhea::decompile(&self.get(Tag::new(b"hhea"))?, &mut font)?;
		font.insert(Box::new(hhea.clone()));
		font.set_glyph_order(glyph_names(num_names));
		let x_mins: Vec<i64> = if glyf_transformed {
			self.decoded_glyf()?.x_mins
		} else {
			let loca = Loca::decompile(&self.get(LOCA)?, &mut font)?;
			font.insert(Box::new(loca));
			let glyf = Glyf::decompile(&self.get(GLYF)?, &mut font)?;
			(0..num_names as u32)
				.map(|gid| {
					glyf.glyphs
						.get(&gid)
						.map(|g| g.x_min)
						.ok_or_else(|| Error::ttlib(format!("KeyError: glyph {gid}")))
				})
				.collect::<Result<_>>()?
		};

		let data = self.raw(HMTX)?;
		let flags = *data
			.first()
			.ok_or_else(|| Error::Struct("unpack requires a buffer of 1 bytes".into()))?;
		let mut data = &data[1..];
		if flags & 0b1111_1100 != 0 {
			return Err(Error::ttlib("Bits 2-7 of 'hmtx' flags are reserved"));
		}
		let has_lsb_array = flags & 1 == 0;
		let has_left_side_bearing_array = flags & 2 == 0;
		if has_lsb_array && has_left_side_bearing_array {
			return Err(Error::ttlib(
				"either bits 0 or 1 (or both) must set in transformed 'hmtx' flags",
			));
		}
		let num_glyphs = x_mins.len();
		let number_of_h_metrics = (hhea.number_of_h_metrics as usize).min(num_glyphs);
		let mut take = |n: usize| -> Result<Vec<u16>> {
			if data.len() < 2 * n {
				return Err(Error::Assertion("len(data) >= 2 * numberOfHMetrics".into()));
			}
			let (a, rest) = data.split_at(2 * n);
			data = rest;
			Ok(a.chunks(2)
				.map(|c| u16::from_be_bytes([c[0], c[1]]))
				.collect())
		};
		let advances = take(number_of_h_metrics)?;
		let lsbs: Vec<i16> = if has_lsb_array {
			take(number_of_h_metrics)?
				.into_iter()
				.map(|v| v as i16)
				.collect()
		} else {
			x_mins[..number_of_h_metrics]
				.iter()
				.map(|&x| crate::binary::fit(x, "lsb"))
				.collect::<Result<_>>()?
		};
		let number_of_side_bearings = num_glyphs - number_of_h_metrics;
		let side_bearings: Vec<i16> = if has_left_side_bearing_array {
			take(number_of_side_bearings)?
				.into_iter()
				.map(|v| v as i16)
				.collect()
		} else {
			x_mins[number_of_h_metrics..]
				.iter()
				.map(|&x| crate::binary::fit(x, "leftSideBearing"))
				.collect::<Result<_>>()?
		};
		if !data.is_empty() {
			return Err(Error::ttlib("too much 'hmtx' table data"));
		}
		let mut hmtx = Hmtx::default();
		for i in 0..number_of_h_metrics {
			hmtx.set(i as u32, (advances[i], lsbs[i]));
		}
		let last_advance = *advances
			.last()
			.ok_or_else(|| Error::ttlib("IndexError: array index out of range"))?;
		for (i, &lsb) in side_bearings.iter().enumerate() {
			hmtx.set((i + number_of_h_metrics) as u32, (last_advance, lsb));
		}
		hmtx.compile(&mut font)
	}
}

/// `WOFF2LocaTable.compile` with the `indexFormat` of the transformed `glyf`.
fn compile_woff2_loca(locations: &[u32], index_format: u16) -> Result<Vec<u8>> {
	let max = locations.iter().copied().max().unwrap_or(0);
	let mut data = Vec::new();
	if index_format == 0 {
		if max >= 0x20000 {
			return Err(Error::ttlib("indexFormat is 0 but local offsets > 0x20000"));
		}
		if !locations.iter().all(|l| l % 2 == 0) {
			return Err(Error::ttlib(
				"indexFormat is 0 but local offsets not multiples of 2",
			));
		}
		for &l in locations {
			data.put_u16((l / 2) as u16);
		}
	} else {
		for &l in locations {
			data.put_u32(l);
		}
	}
	Ok(data)
}

/// A stream of the transformed `glyf` table, read from the front.
struct Stream<'a>(&'a [u8]);

impl<'a> Stream<'a> {
	fn take(&mut self, n: usize) -> &'a [u8] {
		let (a, rest) = self.0.split_at(n.min(self.0.len()));
		self.0 = rest;
		a
	}

	fn read_255_ushort(&mut self) -> Result<u16> {
		let (v, n) = unpack_255_ushort(self.0)?;
		self.0 = &self.0[n..];
		Ok(v)
	}
}

/// Decodes a transformed `glyf` table (`WOFF2GlyfTable.reconstruct`): the glyphs
/// and the `indexFormat`.
fn decode_glyf(data: &[u8]) -> Result<(Vec<Glyph>, u16)> {
	if data.len() < GLYF_HEADER_SIZE {
		return Err(Error::ttlib("not enough 'glyf' data"));
	}
	let u16_at = |i: usize| u16::from_be_bytes([data[i], data[i + 1]]);
	let u32_at =
		|i: usize| u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
	let option_flags = u16_at(2);
	let num_glyphs = u16_at(4) as usize;
	let index_format = u16_at(6);
	let mut rest = Stream(&data[GLYF_HEADER_SIZE..]);
	let mut offset = GLYF_HEADER_SIZE;
	let mut streams = Vec::with_capacity(7);
	for i in 0..7 {
		let size = u32_at(8 + 4 * i);
		streams.push(rest.take(size));
		offset += size;
	}
	let overlap_bitmap = if option_flags & OVERLAP_SIMPLE_BITMAP_FLAG != 0 {
		let size = (num_glyphs + 7) >> 3;
		offset += size;
		Some(rest.take(size))
	} else {
		None
	};
	if offset != data.len() {
		return Err(Error::ttlib(format!(
			"incorrect size of transformed 'glyf' table: expected {offset}, received {} bytes",
			data.len()
		)));
	}
	let [
		n_contour,
		n_points,
		flag,
		glyph,
		composite,
		bbox,
		instruction,
	] = streams[..]
	else {
		return Err(Error::ttlib("internal error: wrong number of streams"));
	};
	let bbox_bitmap_size = ((num_glyphs + 31) >> 5) << 2;
	let mut bbox_stream = Stream(bbox);
	let bbox_bitmap = bbox_stream.take(bbox_bitmap_size);
	if !n_contour.len().is_multiple_of(2) {
		return Err(Error::value("bytes length not a multiple of item size"));
	}
	let n_contours: Vec<i16> = n_contour
		.chunks(2)
		.map(|c| i16::from_be_bytes([c[0], c[1]]))
		.collect();
	if n_contours.len() != num_glyphs {
		return Err(Error::Assertion(
			"len(self.nContourStream) == self.numGlyphs".into(),
		));
	}
	let mut d = Decoder {
		n_points: Stream(n_points),
		flags: Stream(flag),
		glyphs: Stream(glyph),
		composites: Stream(composite),
		bboxes: bbox_stream,
		instructions: Stream(instruction),
		num_glyphs,
	};
	let mut glyphs = Vec::with_capacity(num_glyphs);
	for (gid, &n) in n_contours.iter().enumerate() {
		let mut g = Glyph {
			number_of_contours: n as i64,
			..Default::default()
		};
		if n == 0 {
			glyphs.push(g);
			continue;
		}
		if g.is_composite() {
			d.decode_components(&mut g)?;
		} else {
			d.decode_coordinates(&mut g)?;
			if let Some(bitmap) = overlap_bitmap
				&& g.number_of_contours > 0
				&& bitmap.get(gid >> 3).ok_or_else(index_error)? & (0x80 >> (gid & 7)) != 0
			{
				*g.flags.first_mut().ok_or_else(index_error)? |= FLAG_OVERLAP_SIMPLE;
			}
		}
		let have_bbox =
			bbox_bitmap.get(gid >> 3).ok_or_else(index_error)? & (0x80 >> (gid & 7)) != 0;
		if g.is_composite() && !have_bbox {
			return Err(Error::ttlib(format!(
				"no bbox values for composite glyph {gid}"
			)));
		}
		if have_bbox {
			let b = d.bboxes.take(8);
			if b.len() != 8 {
				return Err(Error::Struct("unpack requires a buffer of 8 bytes".into()));
			}
			let h = |i: usize| i16::from_be_bytes([b[i], b[i + 1]]) as i64;
			(g.x_min, g.y_min, g.x_max, g.y_max) = (h(0), h(2), h(4), h(6));
		} else {
			let b = calc_bounds(&g.coordinates);
			(g.x_min, g.y_min, g.x_max, g.y_max) = (
				ot_round(b[0]),
				ot_round(b[1]),
				ot_round(b[2]),
				ot_round(b[3]),
			);
		}
		glyphs.push(g);
	}
	Ok((glyphs, index_format))
}

fn index_error() -> Error {
	Error::ttlib("IndexError: array index out of range")
}

/// The streams of a transformed `glyf` table being decoded.
struct Decoder<'a> {
	n_points: Stream<'a>,
	flags: Stream<'a>,
	glyphs: Stream<'a>,
	composites: Stream<'a>,
	bboxes: Stream<'a>,
	instructions: Stream<'a>,
	num_glyphs: usize,
}

impl Decoder<'_> {
	/// `_decodeComponents`.
	fn decode_components(&mut self, g: &mut Glyph) -> Result<()> {
		let mut have_instructions = false;
		loop {
			let (c, more, instr, used) =
				GlyphComponent::decompile(self.composites.0, self.num_glyphs)?;
			self.composites.take(used);
			have_instructions |= instr;
			g.components.push(c);
			if !more {
				break;
			}
		}
		if have_instructions {
			self.decode_instructions(g)?;
		}
		Ok(())
	}

	/// `_decodeCoordinates`.
	fn decode_coordinates(&mut self, g: &mut Glyph) -> Result<()> {
		let mut end_point: i64 = -1;
		for _ in 0..g.number_of_contours.max(0) {
			end_point += self.n_points.read_255_ushort()? as i64;
			g.end_pts_of_contours.push(end_point);
		}
		self.decode_triplets(g)?;
		self.decode_instructions(g)
	}

	/// `_decodeInstructions`.
	fn decode_instructions(&mut self, g: &mut Glyph) -> Result<()> {
		let length = self.glyphs.read_255_ushort()? as usize;
		g.program = Some(Program::from_bytecode(self.instructions.take(length)));
		Ok(())
	}

	/// `_decodeTriplets`.
	fn decode_triplets(&mut self, g: &mut Glyph) -> Result<()> {
		let last = *g
			.end_pts_of_contours
			.last()
			.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
		let n_points = usize::try_from(last + 1).unwrap_or(0);
		if n_points > self.flags.0.len() {
			return Err(Error::ttlib("not enough 'flagStream' data"));
		}
		let flags = self.flags.take(n_points);
		let triplets = self.glyphs.0;
		if n_points > triplets.len() {
			return Err(Error::Assertion("nPoints <= nTriplets".into()));
		}
		let with_sign = |flag: u8, base: i64| if flag & 1 != 0 { base } else { -base };
		let (mut x, mut y) = (0i64, 0i64);
		let mut i = 0;
		g.coordinates = Vec::with_capacity(n_points);
		g.flags = Vec::with_capacity(n_points);
		for &raw in flags {
			let on_curve = raw >> 7 == 0;
			let flag = raw & 0x7F;
			let n = match flag {
				0..84 => 1,
				84..120 => 2,
				120..124 => 3,
				_ => 4,
			};
			if i + n > triplets.len() {
				return Err(Error::Assertion(
					"(tripletIndex + nBytes) <= nTriplets".into(),
				));
			}
			let t = |k: usize| triplets[i + k] as i64;
			let f = flag as i64;
			let (dx, dy) = if flag < 10 {
				(0, with_sign(flag, ((f & 14) << 7) + t(0)))
			} else if flag < 20 {
				(with_sign(flag, (((f - 10) & 14) << 7) + t(0)), 0)
			} else if flag < 84 {
				let b0 = f - 20;
				let b1 = t(0);
				(
					with_sign(flag, 1 + (b0 & 0x30) + (b1 >> 4)),
					with_sign(flag >> 1, 1 + ((b0 & 0x0C) << 2) + (b1 & 0x0F)),
				)
			} else if flag < 120 {
				let b0 = f - 84;
				(
					with_sign(flag, 1 + ((b0 / 12) << 8) + t(0)),
					with_sign(flag >> 1, 1 + (((b0 % 12) >> 2) << 8) + t(1)),
				)
			} else if flag < 124 {
				let b2 = t(1);
				(
					with_sign(flag, (t(0) << 4) + (b2 >> 4)),
					with_sign(flag >> 1, ((b2 & 0x0F) << 8) + t(2)),
				)
			} else {
				(
					with_sign(flag, (t(0) << 8) + t(1)),
					with_sign(flag >> 1, (t(2) << 8) + t(3)),
				)
			};
			i += n;
			x += dx;
			y += dy;
			g.coordinates.push((x as f64, y as f64));
			g.flags.push(on_curve as u8);
		}
		self.glyphs.take(i);
		Ok(())
	}
}

// --- Writing (`WOFF2Writer`)

/// Writes a WOFF 2.0 file (`WOFF2Writer`).
pub struct Woff2Writer {
	num_tables: usize,
	sfnt_version: Tag,
	flavor_data: FlavorData,
	transformed_tables: Vec<Tag>,
	tables: Vec<(Tag, Vec<u8>)>,
	/// The Brotli quality; see [`crate::WriteOptions::woff2_quality`].
	pub(crate) quality: u32,
	/// See [`crate::WriteOptions::woff2_progress`].
	pub(crate) progress: Option<Progress>,
}

impl std::fmt::Debug for Woff2Writer {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Woff2Writer")
			.field("num_tables", &self.num_tables)
			.field("sfnt_version", &self.sfnt_version)
			.field("flavor_data", &self.flavor_data)
			.field("transformed_tables", &self.transformed_tables)
			.field("tables", &self.tables)
			.field("quality", &self.quality)
			.field("progress", &self.progress.as_ref().map(|_| ".."))
			.finish()
	}
}

impl Woff2Writer {
	pub(crate) fn new(
		num_tables: usize,
		sfnt_version: Tag,
		flavor_data: Option<FlavorData>,
	) -> Self {
		let flavor_data = flavor_data.unwrap_or_default();
		let transformed_tables = flavor_data
			.transformed_tables
			.clone()
			.unwrap_or_else(|| vec![GLYF, LOCA]);
		Woff2Writer {
			num_tables,
			sfnt_version,
			flavor_data,
			transformed_tables,
			tables: Vec::new(),
			quality: 11,
			progress: None,
		}
	}

	/// `__setitem__`.
	pub(crate) fn add(&mut self, tag: Tag, data: &[u8]) -> Result<()> {
		if self.tables.iter().any(|(t, _)| *t == tag) {
			return Err(Error::ttlib(format!("cannot rewrite '{tag}' table")));
		}
		if tag == DSIG {
			// Always dropped, since the encoding can invalidate it.
			self.num_tables -= 1;
			return Ok(());
		}
		self.tables.push((tag, data.to_vec()));
		Ok(())
	}

	fn data(&self, tag: Tag) -> Result<&[u8]> {
		self.tables
			.iter()
			.find(|(t, _)| *t == tag)
			.map(|(_, d)| d.as_slice())
			.ok_or_else(|| Error::ttlib(format!("missing required table: {tag}")))
	}

	fn set_data(&mut self, tag: Tag, data: Vec<u8>) {
		if let Some(entry) = self.tables.iter_mut().find(|(t, _)| *t == tag) {
			entry.1 = data;
		}
	}

	/// `close`: writes the file.
	pub(crate) fn close(mut self) -> Result<Vec<u8>> {
		if self.tables.len() != self.num_tables {
			return Err(Error::ttlib(format!(
				"wrong number of tables; expected {}, found {}",
				self.num_tables,
				self.tables.len()
			)));
		}
		let is_true_type = match &self.sfnt_version.0 {
			b"\0\x01\0\0" | b"true" => true,
			b"OTTO" => false,
			_ => {
				return Err(Error::ttlib(
					"Not a TrueType or OpenType font (bad sfntVersion)",
				));
			}
		};
		let mut scratch = Scratch::new();
		// The reference WOFF2 implementation fails to rebuild unpadded `glyf`
		// tables, so glyphs are padded to 4 bytes.
		if is_true_type && self.transformed_tables.contains(&GLYF) && self.data(GLYF).is_ok() {
			self.normalise_glyf_and_loca(&mut scratch)?;
		}
		// `_setHeadTransformFlag`: the font has undergone a lossless transform.
		scratch.load(&self, HEAD)?;
		let head = scratch.font.require::<Head>()?;
		head.flags |= 1 << 11;
		let head_data = scratch.compile(HEAD)?;
		self.set_data(HEAD, head_data);

		// Sorted, as the legacy OpenType Sanitiser requires; `loca` then comes
		// after `glyf`.
		self.tables.sort_by_key(|t| t.0);

		// `_calcSFNTChecksumsLengthsAndOffsets`.
		let mut sfnt_entries = Vec::with_capacity(self.tables.len());
		let mut orig_offset = SFNT_DIRECTORY_SIZE + SFNT_DIRECTORY_ENTRY_SIZE * self.tables.len();
		for (tag, data) in &self.tables {
			let check_sum = if *tag == HEAD {
				let mut d = data.clone();
				if d.len() >= 12 {
					d[8..12].fill(0);
				} else {
					d.truncate(8);
					d.extend_from_slice(&[0; 4]);
				}
				calc_checksum(&d)
			} else {
				calc_checksum(data)
			};
			sfnt_entries.push(DirectoryEntry {
				tag: *tag,
				check_sum,
				offset: 0,
				length: 0,
				orig_length: data.len() as u32,
				orig_offset: orig_offset as u32,
			});
			orig_offset += (data.len() + 3) & !3;
		}

		// `_transformTables`.
		let mut entries = Vec::with_capacity(self.tables.len());
		let mut stream = Vec::new();
		let mut head_offset = None;
		for i in 0..self.tables.len() {
			let tag = self.tables[i].0;
			let mut entry = Entry {
				tag,
				flags: known_tag_index(tag),
				offset: stream.len(),
				length: 0,
				orig_length: self.tables[i].1.len() as u32,
			};
			let mut data = None;
			if self.transformed_tables.contains(&tag) {
				data = self.transform_table(&mut scratch, tag)?;
				if data.is_some() {
					entry.set_transformed(true);
				}
			}
			let data = match data {
				Some(d) => d,
				None => {
					if tag == GLYF {
						self.transformed_tables.retain(|t| *t != LOCA);
					}
					entry.set_transformed(false);
					self.tables[i].1.clone()
				}
			};
			if tag == HEAD {
				head_offset = Some(stream.len());
			}
			entry.length = data.len();
			stream.extend_from_slice(&data);
			entries.push(entry);
		}
		// `writeMasterChecksum`.
		let directory = sfnt_directory(self.sfnt_version, &sfnt_entries, true);
		let mut checksum: u32 = 0;
		for e in &sfnt_entries {
			checksum = checksum.wrapping_add(e.check_sum);
		}
		checksum = checksum.wrapping_add(calc_checksum(&directory));
		let adjustment = 0xB1B0_AFBA_u32.wrapping_sub(checksum);
		let at = head_offset.ok_or_else(|| Error::ttlib("KeyError: 'head'"))? + 8;
		if stream.len() < at + 4 {
			stream.resize(at + 4, 0);
		}
		stream[at..at + 4].copy_from_slice(&adjustment.to_be_bytes());

		let compressed = brotli_compress(&stream, true, self.quality, self.progress.as_ref())?;
		let total_sfnt_size = orig_offset;

		// `_calcTotalSize` and `_calcFlavorDataOffsetsAndSize`.
		let entry_bytes: Vec<Vec<u8>> = entries.iter().map(Entry::to_bytes).collect();
		let mut offset = WOFF2_DIRECTORY_SIZE + entry_bytes.iter().map(Vec::len).sum::<usize>();
		offset += compressed.len();
		offset = (offset + 3) & !3;
		let meta = match &self.flavor_data.meta_data {
			Some(m) if !m.is_empty() => {
				Some((m.len(), brotli_compress(m, false, self.quality, None)?))
			}
			_ => None,
		};
		let (meta_offset, meta_length, meta_orig_length) = match &meta {
			Some((orig, c)) => {
				let at = offset;
				offset += c.len();
				(at, c.len(), *orig)
			}
			None => (0, 0, 0),
		};
		let priv_data = self
			.flavor_data
			.priv_data
			.as_deref()
			.filter(|p| !p.is_empty());
		let (priv_offset, priv_length) = match priv_data {
			Some(p) => {
				offset = (offset + 3) & !3;
				let at = offset;
				offset += p.len();
				(at, p.len())
			}
			None => (0, 0),
		};
		let length = offset;
		// `_getVersion`.
		let (major, minor) = match (
			self.flavor_data.major_version,
			self.flavor_data.minor_version,
		) {
			(Some(a), Some(b)) => (a, b),
			_ => match self.data(HEAD) {
				Ok(h) if h.len() >= 8 => (
					u16::from_be_bytes([h[4], h[5]]),
					u16::from_be_bytes([h[6], h[7]]),
				),
				Ok(_) => return Err(Error::Struct("unpack requires a buffer of 4 bytes".into())),
				Err(_) => (0, 0),
			},
		};
		let fit = |v: usize, name: &str| crate::binary::fit::<u32, _>(v, name);
		let mut out = Vec::with_capacity(length);
		out.extend_from_slice(b"wOF2");
		out.extend_from_slice(&self.sfnt_version.0);
		out.put_u32(fit(length, "length")?);
		out.put_u16(crate::binary::fit(self.num_tables, "numTables")?);
		out.put_u16(0);
		out.put_u32(fit(total_sfnt_size, "totalSfntSize")?);
		out.put_u32(fit(compressed.len(), "totalCompressedSize")?);
		out.put_u16(major);
		out.put_u16(minor);
		out.put_u32(fit(meta_offset, "metaOffset")?);
		out.put_u32(fit(meta_length, "metaLength")?);
		out.put_u32(fit(meta_orig_length, "metaOrigLength")?);
		out.put_u32(fit(priv_offset, "privOffset")?);
		out.put_u32(fit(priv_length, "privLength")?);
		for e in entry_bytes {
			out.extend(e);
		}
		out.extend(compressed);
		pad4(&mut out);
		if let Some((_, c)) = meta {
			out.extend(c);
			if priv_data.is_some() {
				pad4(&mut out);
			}
		}
		if let Some(p) = priv_data {
			out.extend_from_slice(p);
		}
		Ok(out)
	}

	/// `_normaliseGlyfAndLoca`: compiles `glyf` with glyphs padded to 4 bytes,
	/// and `loca` to match.
	fn normalise_glyf_and_loca(&mut self, scratch: &mut Scratch) -> Result<()> {
		if self.sfnt_version.0 == *b"OTTO" {
			return Ok(());
		}
		for tag in [Tag::new(b"maxp"), HEAD, LOCA, GLYF, Tag::new(b"fvar")] {
			if self.data(tag).is_ok() {
				scratch.load(self, tag)?;
			}
		}
		scratch.font.require::<Glyf>()?.padding = 4;
		for tag in [GLYF, LOCA] {
			let data = scratch.compile(tag)?;
			self.set_data(tag, data);
		}
		Ok(())
	}

	/// `transformTable`: the transformed data, or `None` if the table cannot be
	/// transformed.
	fn transform_table(&self, scratch: &mut Scratch, tag: Tag) -> Result<Option<Vec<u8>>> {
		if tag == LOCA {
			Ok(Some(Vec::new()))
		} else if tag == GLYF {
			for t in [Tag::new(b"maxp"), HEAD, LOCA, GLYF] {
				scratch.load(self, t)?;
			}
			let index_format = scratch.font.require::<Head>()?.index_to_loc_format;
			let order = scratch.font.glyph_order()?;
			let glyf = scratch.font.require::<Glyf>()?;
			transform_glyf(glyf, order.len(), index_format)
		} else if tag == HMTX {
			if self.data(GLYF).is_err() {
				return Ok(None);
			}
			for t in [Tag::new(b"maxp"), HEAD, Tag::new(b"hhea"), LOCA, GLYF, HMTX] {
				scratch.load(self, t)?;
			}
			let order = scratch.font.glyph_order()?;
			let number_of_h_metrics = scratch.font.require::<Hhea>()?.number_of_h_metrics as usize;
			let x_mins: Vec<i64> = {
				let glyf = scratch.font.require::<Glyf>()?;
				(0..order.len() as u32)
					.map(|gid| {
						glyf.glyphs
							.get(&gid)
							.map(|g| {
								if g.number_of_contours == 0 {
									0
								} else {
									g.x_min
								}
							})
							.ok_or_else(|| Error::ttlib(format!("KeyError: {:?}", order.name(gid))))
					})
					.collect::<Result<_>>()?
			};
			let hmtx = scratch.font.require::<Hmtx>()?;
			transform_hmtx(hmtx, &x_mins, number_of_h_metrics)
		} else {
			Err(Error::ttlib(format!(
				"Transform for table '{tag}' is unknown"
			)))
		}
	}
}

/// The tables of the font being written, decompiled as needed (the `ttFont` of
/// `WOFF2Writer`).
struct Scratch {
	font: Font,
}

impl Scratch {
	fn new() -> Self {
		Scratch {
			font: scratch_font(),
		}
	}

	/// `_decompileTable`.
	fn load(&mut self, writer: &Woff2Writer, tag: Tag) -> Result<()> {
		let data = writer.data(tag)?;
		if self.font.is_loaded(tag) {
			return Ok(());
		}
		let table: Box<dyn Table> = match &tag.0 {
			b"maxp" => {
				let maxp = Maxp::decompile(data, &mut self.font)?;
				if !self.font.has_glyph_order() {
					self.font
						.set_glyph_order(glyph_names(maxp.num_glyphs as usize));
				}
				Box::new(maxp)
			}
			b"head" => Box::new(Head::decompile(data, &mut self.font)?),
			b"hhea" => Box::new(Hhea::decompile(data, &mut self.font)?),
			b"loca" => Box::new(Loca::decompile(data, &mut self.font)?),
			b"glyf" => Box::new(Glyf::decompile(data, &mut self.font)?),
			b"hmtx" => Box::new(Hmtx::decompile(data, &mut self.font)?),
			_ => crate::table::decompile_table(tag, data, &mut self.font)?,
		};
		self.font.insert(table);
		Ok(())
	}

	/// `_compileTable`.
	fn compile(&mut self, tag: Tag) -> Result<Vec<u8>> {
		self.font.with_table(tag, |table, font| table.compile(font))
	}
}

/// `WOFF2GlyfTable.transform`.
fn transform_glyf(glyf: &Glyf, num_names: usize, index_format: i16) -> Result<Option<Vec<u8>>> {
	let num_glyphs = glyf.glyphs.len();
	if num_names != num_glyphs {
		return Err(Error::Assertion(
			"len(self.glyphOrder) == self.numGlyphs".into(),
		));
	}
	let mut e = Encoder::default();
	let bbox_bitmap_size = ((num_glyphs + 31) >> 5) << 2;
	e.bbox_bitmap = vec![0; bbox_bitmap_size];
	let mut overlap = vec![0u8; (num_glyphs + 7) >> 3];
	for gid in 0..num_glyphs {
		let g = glyf
			.glyphs
			.get(&(gid as u32))
			.ok_or_else(|| Error::ttlib(format!("KeyError: glyph {gid}")))?;
		e.n_contours.put_i16(crate::binary::fit(
			g.number_of_contours,
			"numberOfContours",
		)?);
		if g.number_of_contours == 0 {
			continue;
		}
		if g.is_composite() {
			let last = g.components.len().saturating_sub(1);
			let have_instructions = g.program.is_some();
			for (i, c) in g.components.iter().enumerate() {
				let (more, instr) = if i == last {
					(false, have_instructions)
				} else {
					(true, false)
				};
				e.composites.extend(c.compile(more, instr, num_glyphs)?);
			}
			if have_instructions {
				e.encode_instructions(g)?;
			}
		} else {
			if g.flags.contains(&FLAG_CUBIC) {
				return Ok(None);
			}
			let mut last_end: i64 = -1;
			for &end in &g.end_pts_of_contours {
				e.n_points.extend(pack_255_ushort(end - last_end)?);
				last_end = end;
			}
			e.encode_triplets(g)?;
			e.encode_instructions(g)?;
			if g.number_of_contours > 0
				&& g.flags
					.first()
					.is_some_and(|f| f & FLAG_OVERLAP_SIMPLE != 0)
			{
				overlap[gid >> 3] |= 0x80 >> (gid & 7);
			}
		}
		// `_encodeBBox`: simple glyphs whose bounds match their points omit them.
		if !g.is_composite() {
			let b = calc_bounds(&g.coordinates);
			let calculated = [
				ot_round(b[0]),
				ot_round(b[1]),
				ot_round(b[2]),
				ot_round(b[3]),
			];
			if [g.x_min, g.y_min, g.x_max, g.y_max] == calculated {
				continue;
			}
		}
		e.bbox_bitmap[gid >> 3] |= 0x80 >> (gid & 7);
		for v in [g.x_min, g.y_min, g.x_max, g.y_max] {
			e.bboxes.put_i16(crate::binary::fit(v, "bbox")?);
		}
	}
	let has_overlap = overlap.iter().any(|&b| b != 0);
	let mut bbox_stream = std::mem::take(&mut e.bbox_bitmap);
	bbox_stream.extend_from_slice(&e.bboxes);
	let streams = [
		&e.n_contours,
		&e.n_points,
		&e.flags,
		&e.glyphs,
		&e.composites,
		&bbox_stream,
		&e.instructions,
	];
	let mut data = Vec::new();
	data.put_u16(0);
	data.put_u16(if has_overlap {
		OVERLAP_SIMPLE_BITMAP_FLAG
	} else {
		0
	});
	data.put_u16(crate::binary::fit(num_glyphs, "numGlyphs")?);
	data.put_u16(index_format as u16);
	for s in streams {
		data.put_u32(crate::binary::fit(s.len(), "streamSize")?);
	}
	for s in streams {
		data.extend_from_slice(s);
	}
	if has_overlap {
		data.extend(overlap);
	}
	Ok(Some(data))
}

/// The streams of a transformed `glyf` table being encoded.
#[derive(Default)]
struct Encoder {
	n_contours: Vec<u8>,
	n_points: Vec<u8>,
	flags: Vec<u8>,
	glyphs: Vec<u8>,
	composites: Vec<u8>,
	bbox_bitmap: Vec<u8>,
	bboxes: Vec<u8>,
	instructions: Vec<u8>,
}

impl Encoder {
	/// `_encodeInstructions`.
	fn encode_instructions(&mut self, g: &Glyph) -> Result<()> {
		let program = g.program.as_ref().ok_or_else(|| {
			Error::ttlib("AttributeError: 'Glyph' object has no attribute 'program'")
		})?;
		self.glyphs
			.extend(pack_255_ushort(program.bytecode.len() as i64)?);
		self.instructions.extend_from_slice(&program.bytecode);
		Ok(())
	}

	/// `_encodeTriplets`.
	fn encode_triplets(&mut self, g: &Glyph) -> Result<()> {
		if g.coordinates.len() != g.flags.len() {
			return Err(Error::Assertion(
				"len(glyph.coordinates) == len(glyph.flags)".into(),
			));
		}
		let (mut last_x, mut last_y) = (0i64, 0i64);
		for (i, &(fx, fy)) in g.coordinates.iter().enumerate() {
			if fx.fract() != 0.0 || fy.fract() != 0.0 {
				return Err(Error::ttlib(
					"TypeError: unsupported operand type(s) for &: 'float' and 'int'",
				));
			}
			let (ax, ay) = (fx as i64, fy as i64);
			let (x, y) = (ax - last_x, ay - last_y);
			(last_x, last_y) = (ax, ay);
			let on_curve = g.flags[i] & FLAG_ON_CURVE != 0;
			let (abs_x, abs_y) = (x.abs(), y.abs());
			let on_curve_bit: i64 = if on_curve { 0 } else { 128 };
			let x_sign_bit = if x < 0 { 0 } else { 1 };
			let y_sign_bit = if y < 0 { 0 } else { 1 };
			let xy_sign_bits = x_sign_bit + 2 * y_sign_bit;
			let mut flag = |v: i64| self.flags.push(v as u8);
			if x == 0 && abs_y < 1280 {
				flag(on_curve_bit + ((abs_y & 0xF00) >> 7) + y_sign_bit);
				self.glyphs.push((abs_y & 0xFF) as u8);
			} else if y == 0 && abs_x < 1280 {
				flag(on_curve_bit + 10 + ((abs_x & 0xF00) >> 7) + x_sign_bit);
				self.glyphs.push((abs_x & 0xFF) as u8);
			} else if abs_x < 65 && abs_y < 65 {
				flag(
					on_curve_bit
						+ 20 + ((abs_x - 1) & 0x30)
						+ (((abs_y - 1) & 0x30) >> 2)
						+ xy_sign_bits,
				);
				self.glyphs
					.push(((((abs_x - 1) & 0xF) << 4) | ((abs_y - 1) & 0xF)) as u8);
			} else if abs_x < 769 && abs_y < 769 {
				flag(
					on_curve_bit
						+ 84 + 12 * (((abs_x - 1) & 0x300) >> 8)
						+ (((abs_y - 1) & 0x300) >> 6)
						+ xy_sign_bits,
				);
				self.glyphs.push(((abs_x - 1) & 0xFF) as u8);
				self.glyphs.push(((abs_y - 1) & 0xFF) as u8);
			} else if abs_x < 4096 && abs_y < 4096 {
				flag(on_curve_bit + 120 + xy_sign_bits);
				self.glyphs.push((abs_x >> 4) as u8);
				self.glyphs
					.push((((abs_x & 0xF) << 4) | (abs_y >> 8)) as u8);
				self.glyphs.push((abs_y & 0xFF) as u8);
			} else {
				flag(on_curve_bit + 124 + xy_sign_bits);
				for v in [abs_x >> 8, abs_x & 0xFF, abs_y >> 8, abs_y & 0xFF] {
					let b = u8::try_from(v)
						.map_err(|_| Error::value("byte must be in range(0, 256)"))?;
					self.glyphs.push(b);
				}
			}
		}
		Ok(())
	}
}

/// `WOFF2HmtxTable.transform`: `None` if both side bearing arrays are needed.
fn transform_hmtx(
	hmtx: &Hmtx,
	x_mins: &[i64],
	number_of_h_metrics: usize,
) -> Result<Option<Vec<u8>>> {
	let n = x_mins.len();
	let metric = |gid: usize| {
		hmtx.get(gid as u32)
			.ok_or_else(|| Error::ttlib(format!("KeyError: glyph {gid}")))
	};
	if number_of_h_metrics > n {
		return Err(Error::ttlib("IndexError: list index out of range"));
	}
	let mut has_lsb_array = false;
	for (gid, &x_min) in x_mins.iter().enumerate().take(number_of_h_metrics) {
		if metric(gid)?.1 as i64 != x_min {
			has_lsb_array = true;
			break;
		}
	}
	let mut has_left_side_bearing_array = false;
	for (gid, &x_min) in x_mins.iter().enumerate().skip(number_of_h_metrics) {
		if metric(gid)?.1 as i64 != x_min {
			has_left_side_bearing_array = true;
			break;
		}
	}
	if has_lsb_array && has_left_side_bearing_array {
		return Ok(None);
	}
	let mut flags = 0u8;
	if !has_lsb_array {
		flags |= 1;
	}
	if !has_left_side_bearing_array {
		flags |= 2;
	}
	let mut data = vec![flags];
	for gid in 0..number_of_h_metrics {
		data.put_u16(metric(gid)?.0);
	}
	if has_lsb_array {
		for gid in 0..number_of_h_metrics {
			data.put_i16(metric(gid)?.1);
		}
	}
	if has_left_side_bearing_array {
		for gid in number_of_h_metrics..n {
			data.put_i16(metric(gid)?.1);
		}
	}
	Ok(Some(data))
}

#[cfg(test)]
mod tests {
	use super::*;

	// Cases from fontTools' doctests and Tests/ttLib/woff2_test.py.
	#[test]
	fn base128() -> Result<()> {
		assert_eq!(unpack_base128(b"\x3f\x00\x00")?, (63, 1));
		assert_eq!(unpack_base128(b"\x8f\xff\xff\xff\x7f")?.0, u32::MAX);
		assert!(unpack_base128(b"\x80\x80\x3f").is_err());
		assert!(unpack_base128(b"\x8f\xff\xff\xff\xff\x7f").is_err());
		assert!(unpack_base128(b"\x90\x80\x80\x80\x00").is_err());
		assert_eq!(pack_base128(63), b"\x3f");
		assert_eq!(pack_base128(u32::MAX), b"\x8f\xff\xff\xff\x7f");
		assert_eq!(pack_base128(24567).len(), 3);
		Ok(())
	}

	#[test]
	fn ushort255() -> Result<()> {
		assert_eq!(unpack_255_ushort(&[252])?.0, 252);
		assert_eq!(unpack_255_ushort(&[254, 0])?.0, 506);
		assert_eq!(unpack_255_ushort(&[255, 253])?.0, 506);
		assert_eq!(unpack_255_ushort(&[253, 1, 250])?.0, 506);
		assert_eq!(pack_255_ushort(252)?, [0xFC]);
		assert_eq!(pack_255_ushort(506)?, [0xFE, 0x00]);
		assert_eq!(pack_255_ushort(762)?, [0xFD, 0x02, 0xFA]);
		Ok(())
	}

	fn sample_data() -> Vec<u8> {
		(0..100_000_u32)
			.map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8 & 0x3F)
			.collect()
	}

	/// The progress rises to 1.0, and does not change the output.
	#[test]
	fn brotli_progress() -> Result<()> {
		let data = sample_data();
		let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
		let progress: Progress = {
			let seen = seen.clone();
			std::rc::Rc::new(move |p| seen.borrow_mut().push(p))
		};
		let with = brotli_compress(&data, true, 11, Some(&progress))?;
		assert_eq!(with, brotli_compress(&data, true, 11, None)?);
		let seen = seen.borrow();
		assert!(seen.len() > 2, "{seen:?}");
		assert_eq!(seen.first(), Some(&0.0));
		assert_eq!(seen.last(), Some(&1.0));
		assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
		Ok(())
	}

	/// A lower quality gives other, still valid, data.
	#[test]
	fn brotli_quality() -> Result<()> {
		let data = sample_data();
		let fast = brotli_compress(&data, true, 5, None)?;
		assert_ne!(fast, brotli_compress(&data, true, 11, None)?);
		assert_eq!(brotli_decompress(&fast)?, data);
		Ok(())
	}
}
