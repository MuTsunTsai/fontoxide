//! `Tests/ttLib/woff2_test.py`, and the doctests of `fontTools.ttLib.woff2`.
//!
//! The tests of the private `WOFF2DirectoryEntry`, `WOFF2LocaTable`,
//! `WOFF2GlyfTable` and `WOFF2HmtxTable` are unit tests in `src/woff2.rs`. The
//! Python tests use a `WOFF2Reader` or `WOFF2Writer` directly; these use
//! [`SfntReader`] and [`SfntWriter`], which wrap them.

#![allow(non_snake_case)]

use std::path::PathBuf;
use std::sync::OnceLock;

use fontoxide_pens::RecordingPen;
use fontoxide_ttlib::beyond64k::{ConvertOptions, upper_tables};
use fontoxide_ttlib::sfnt::{SfntReader, SfntReaderOptions, SfntWriter};
use fontoxide_ttlib::tables::glyf::{FLAG_ON_CURVE, FLAG_OVERLAP_SIMPLE};
use fontoxide_ttlib::tables::{Glyf, Head, Hmtx};
use fontoxide_ttlib::woff2::{pack_255_ushort, pack_base128, unpack_255_ushort, unpack_base128};
use fontoxide_ttlib::{Flavor, FlavorData, Font, GlyphSet, ReadOptions, Tag, WriteOptions};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

const TTX: &str = "ttLib/data/TestTTF-Regular.ttx";
const OTX: &str = "ttLib/data/TestOTF-Regular.otx";
const METADATA: &str = "ttLib/data/test_woff2_metadata.xml";

const GLYF: Tag = Tag::new(b"glyf");
const LOCA: Tag = Tag::new(b"loca");
const HEAD: Tag = Tag::new(b"head");
const HMTX: Tag = Tag::new(b"hmtx");
const HHEA: Tag = Tag::new(b"hhea");
const DSIG: Tag = Tag::new(b"DSIG");

fn upstream(path: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests")
		.join(path)
}

/// `TTFont(recalcBBoxes=False, recalcTimestamp=False)` and `importXML`.
fn import(path: &str) -> TestResult<Font> {
	let mut font = Font::new();
	font.recalc_bboxes = false;
	font.recalc_timestamp = false;
	font.import_ttx(&std::fs::read_to_string(upstream(path))?)?;
	Ok(font)
}

const NO_RECALC: ReadOptions = ReadOptions {
	font_number: None,
	check_checksums: 0,
	recalc_bboxes: false,
	recalc_timestamp: false,
	ignore_decompile_errors: false,
	use_legacy_ttc: false,
	use_harfbuzz_repacker: None,
};

/// `TT_WOFF2` and `CFF_WOFF2` of `setUpModule`.
fn save_woff2(path: &str) -> Vec<u8> {
	let mut font = import(path).expect("the test font");
	font.flavor = Some(Flavor::Woff2);
	font.write(&WriteOptions {
		reorder_tables: None,
		..Default::default()
	})
	.expect("a WOFF2 font")
}

fn tt_woff2() -> &'static [u8] {
	static DATA: OnceLock<Vec<u8>> = OnceLock::new();
	DATA.get_or_init(|| save_woff2(TTX))
}

fn cff_woff2() -> &'static [u8] {
	static DATA: OnceLock<Vec<u8>> = OnceLock::new();
	DATA.get_or_init(|| save_woff2(OTX))
}

/// The test fonts of the CFF and the TrueType flavored test classes.
fn fonts() -> [(&'static str, &'static [u8]); 2] {
	[(OTX, cff_woff2()), (TTX, tt_woff2())]
}

fn err_contains<T, E: std::fmt::Display>(result: Result<T, E>, message: &str) {
	match result {
		Ok(_) => panic!("expected an error with {message:?}"),
		Err(e) => assert!(e.to_string().contains(message), "{e} !~ {message:?}"),
	}
}

fn reader(data: &[u8]) -> fontoxide_ttlib::Result<SfntReader> {
	SfntReader::new(data.to_vec(), SfntReaderOptions::default())
}

fn u32_at(data: &[u8], at: usize) -> u32 {
	u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

fn u16_at(data: &[u8], at: usize) -> u16 {
	u16::from_be_bytes([data[at], data[at + 1]])
}

fn set_u32(data: &mut [u8], at: usize, value: u32) {
	data[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

/// A table of the WOFF 2.0 directory.
#[derive(Debug, Clone)]
struct RawEntry {
	flags: u8,
	tag: Tag,
	orig_length: u32,
	transform_length: Option<u32>,
}

impl RawEntry {
	/// The length of the stored data (`entry.length`).
	fn length(&self) -> u32 {
		self.transform_length.unwrap_or(self.orig_length)
	}
}

/// A WOFF 2.0 file taken apart, to make broken ones.
#[derive(Debug, Clone)]
struct Woff2File {
	header: Vec<u8>,
	entries: Vec<RawEntry>,
	compressed: Vec<u8>,
	/// The padding, metadata and private data.
	tail: Vec<u8>,
}

impl Woff2File {
	fn parse(data: &[u8]) -> TestResult<Self> {
		let header = data[..48].to_vec();
		let num_tables = u16_at(&header, 12);
		let mut pos = 48;
		let mut entries = Vec::new();
		for _ in 0..num_tables {
			let flags = data[pos];
			pos += 1;
			let tag = if flags & 0x3F == 0x3F {
				let tag = Tag(data[pos..pos + 4].try_into()?);
				pos += 4;
				tag
			} else {
				Tag(*KNOWN_TAGS[(flags & 0x3F) as usize])
			};
			let (orig_length, n) = unpack_base128(&data[pos..])?;
			pos += n;
			let version = flags >> 6;
			let transformed = if tag == GLYF || tag == LOCA {
				version != 3
			} else {
				version != 0
			};
			let transform_length = if transformed {
				let (length, n) = unpack_base128(&data[pos..])?;
				pos += n;
				Some(length)
			} else {
				None
			};
			entries.push(RawEntry {
				flags,
				tag,
				orig_length,
				transform_length,
			});
		}
		let end = pos + u32_at(&header, 20) as usize;
		Ok(Woff2File {
			header,
			entries,
			compressed: data[pos..end].to_vec(),
			tail: data[end..].to_vec(),
		})
	}

	fn entry(&mut self, tag: Tag) -> &mut RawEntry {
		self.entries
			.iter_mut()
			.find(|e| e.tag == tag)
			.expect("a table entry")
	}

	/// The file, with `length` and `totalCompressedSize` updated.
	fn to_bytes(&self) -> Vec<u8> {
		let mut data = self.header.clone();
		for e in &self.entries {
			data.push(e.flags);
			if e.flags & 0x3F == 0x3F {
				data.extend_from_slice(&e.tag.0);
			}
			data.extend(pack_base128(e.orig_length));
			if let Some(length) = e.transform_length {
				data.extend(pack_base128(length));
			}
		}
		data.extend_from_slice(&self.compressed);
		data.extend_from_slice(&self.tail);
		let length = data.len() as u32;
		set_u32(&mut data, 8, length);
		set_u32(&mut data, 20, self.compressed.len() as u32);
		data
	}
}

const KNOWN_TAGS: [&[u8; 4]; 63] = [
	b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post", b"cvt ", b"fpgm",
	b"glyf", b"loca", b"prep", b"CFF ", b"VORG", b"EBDT", b"EBLC", b"gasp", b"hdmx", b"kern",
	b"LTSH", b"PCLT", b"VDMX", b"vhea", b"vmtx", b"BASE", b"GDEF", b"GPOS", b"GSUB", b"EBSC",
	b"JSTF", b"MATH", b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar",
	b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar", b"gvar", b"hsty",
	b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop", b"trak", b"Zapf", b"Silf", b"Glat",
	b"Gloc", b"Feat", b"Sill",
];

/// The table tags of a test font, but `DSIG`.
fn tags_but_dsig(font: &Font) -> Vec<Tag> {
	let mut tags: Vec<Tag> = font.tags().into_iter().filter(|&t| t != DSIG).collect();
	tags.sort();
	tags
}

// --- WOFF2ReaderTest and WOFF2ReaderTTFTest

/// `test_not_enough_data_header`.
#[test]
fn test_not_enough_data_header() {
	for (_, file) in fonts() {
		err_contains(reader(&file[..47]), "not enough data");
	}
}

/// `test_incorrect_compressed_size`.
#[test]
fn test_incorrect_compressed_size() {
	for (_, file) in fonts() {
		let mut data = file.to_vec();
		set_u32(&mut data, 20, 0);
		assert!(reader(&data).is_err());
	}
}

/// `test_incorrect_uncompressed_size`: fontTools replaces `brotli.decompress`;
/// this replaces the compressed data.
#[test]
fn test_incorrect_uncompressed_size() -> TestResult {
	for (_, file) in fonts() {
		let mut woff2 = Woff2File::parse(file)?;
		woff2.compressed.clear();
		brotli::BrotliCompress(&mut &b""[..], &mut woff2.compressed, &Default::default())?;
		err_contains(
			reader(&woff2.to_bytes()),
			"unexpected size for decompressed",
		);
	}
	Ok(())
}

/// `test_incorrect_file_size`.
#[test]
fn test_incorrect_file_size() {
	for (_, file) in fonts() {
		let mut data = file.to_vec();
		let length = u32_at(&data, 8);
		set_u32(&mut data, 8, length - 1);
		err_contains(reader(&data), "doesn't match the actual file size");
	}
}

/// `test_num_tables`.
#[test]
fn test_num_tables() -> TestResult {
	for (path, file) in fonts() {
		let font = import(path)?;
		assert_eq!(u16_at(file, 12) as usize, tags_but_dsig(&font).len());
	}
	Ok(())
}

/// `test_table_tags`.
#[test]
fn test_table_tags() -> TestResult {
	for (path, file) in fonts() {
		let font = import(path)?;
		let mut tags: Vec<Tag> = reader(file)?.tags().collect();
		tags.sort();
		assert_eq!(tags, tags_but_dsig(&font));
	}
	Ok(())
}

/// `test_get_normal_tables`.
#[test]
fn test_get_normal_tables() -> TestResult {
	for (path, file) in fonts() {
		let mut font = import(path)?;
		let reader = reader(file)?;
		for tag in tags_but_dsig(&font) {
			if [GLYF, LOCA, HEAD].contains(&tag) {
				continue;
			}
			assert_eq!(*reader.table_data(tag)?, font.table_data(tag)?, "{tag}");
		}
	}
	Ok(())
}

/// `test_reconstruct_unknown`: `head` marked as transformed.
#[test]
fn test_reconstruct_unknown() -> TestResult {
	for (_, file) in fonts() {
		let mut woff2 = Woff2File::parse(file)?;
		let head = woff2.entry(HEAD);
		head.flags |= 1 << 6;
		head.transform_length = Some(head.orig_length);
		let reader = reader(&woff2.to_bytes())?;
		err_contains(
			reader.table_data(HEAD),
			"transform for table 'head' is unknown",
		);
	}
	Ok(())
}

/// `WOFF2ReaderTTFTest.test_reconstruct_glyf`.
#[test]
fn test_reconstruct_glyf() -> TestResult {
	let mut font = import(TTX)?;
	assert_eq!(
		*reader(tt_woff2())?.table_data(GLYF)?,
		font.table_data(GLYF)?
	);
	Ok(())
}

/// `WOFF2ReaderTTFTest.test_reconstruct_loca`.
#[test]
fn test_reconstruct_loca() -> TestResult {
	let mut font = import(TTX)?;
	let reader = reader(tt_woff2())?;
	let reconstructed = reader.table_data(LOCA)?;
	font.table_data(GLYF)?;
	assert_eq!(*reconstructed, font.table_data(LOCA)?);
	Ok(())
}

/// `WOFF2ReaderTTFTest.test_reconstruct_loca_not_match_orig_size`.
#[test]
fn test_reconstruct_loca_not_match_orig_size() -> TestResult {
	let mut woff2 = Woff2File::parse(tt_woff2())?;
	woff2.entry(LOCA).orig_length -= 1;
	let reader = reader(&woff2.to_bytes())?;
	err_contains(
		reader.table_data(LOCA),
		"'loca' table doesn't match original size",
	);
	Ok(())
}

// --- WOFF2FlavorDataTest: fontTools reads the flavor data of a dummy reader;
// these write it and read it back.

fn metadata() -> TestResult<Vec<u8>> {
	Ok(std::fs::read(upstream(METADATA))?)
}

const PRIV_DATA: &[u8] =
	b"\x8e\x11\xc7\x02\x5b\xf0\x33\x9a\x10\x44\x7d\xee\x01\xb2\x6c\x29\xd5\x98\x07\x4f";

/// Writes the CFF test font as WOFF 2.0 with `flavor_data`, and reads it back.
fn flavor_round_trip(flavor_data: FlavorData) -> TestResult<FlavorData> {
	let mut font = import(OTX)?;
	font.flavor = Some(Flavor::Woff2);
	font.flavor_data = Some(flavor_data);
	let data = font.write(&WriteOptions::default())?;
	let read = Font::read(data, NO_RECALC)?;
	Ok(read.flavor_data.ok_or("no flavor data")?)
}

/// `test_get_metaData_no_privData`.
#[test]
fn test_get_metaData_no_privData() -> TestResult {
	let read = flavor_round_trip(FlavorData {
		meta_data: Some(metadata()?),
		..FlavorData::new(Flavor::Woff2)
	})?;
	assert_eq!(read.meta_data, Some(metadata()?));
	assert_eq!(read.priv_data, None);
	Ok(())
}

/// `test_get_privData_no_metaData`.
#[test]
fn test_get_privData_no_metaData() -> TestResult {
	let read = flavor_round_trip(FlavorData {
		priv_data: Some(PRIV_DATA.to_vec()),
		..FlavorData::new(Flavor::Woff2)
	})?;
	assert_eq!(read.meta_data, None);
	assert_eq!(read.priv_data.as_deref(), Some(PRIV_DATA));
	Ok(())
}

/// `test_get_metaData_and_privData`.
#[test]
fn test_get_metaData_and_privData() -> TestResult {
	let read = flavor_round_trip(FlavorData {
		meta_data: Some(metadata()?),
		priv_data: Some(PRIV_DATA.to_vec()),
		..FlavorData::new(Flavor::Woff2)
	})?;
	assert_eq!(read.meta_data, Some(metadata()?));
	assert_eq!(read.priv_data.as_deref(), Some(PRIV_DATA));
	Ok(())
}

/// `test_get_major_minorVersion`.
#[test]
fn test_get_major_minorVersion() -> TestResult {
	let read = flavor_round_trip(FlavorData {
		major_version: Some(1),
		minor_version: Some(1),
		..FlavorData::new(Flavor::Woff2)
	})?;
	assert_eq!((read.major_version, read.minor_version), (Some(1), Some(1)));
	Ok(())
}

/// `test_copy_major_minorVersion`: the flavor data of a WOFF font is used for
/// WOFF 2.0 too.
#[test]
fn test_copy_major_minorVersion() -> TestResult {
	for flavor in [Flavor::Woff, Flavor::Woff2] {
		let read = flavor_round_trip(FlavorData {
			major_version: Some(1),
			minor_version: Some(7),
			..FlavorData::new(flavor)
		})?;
		assert_eq!(
			(read.major_version, read.minor_version),
			(Some(1), Some(7)),
			"{flavor:?}"
		);
	}
	Ok(())
}

fn sorted(tags: &[Tag]) -> Vec<Tag> {
	let mut tags = tags.to_vec();
	tags.sort();
	tags
}

/// `test_transformedTables_default`.
#[test]
fn test_transformedTables_default() -> TestResult {
	assert_eq!(FlavorData::new(Flavor::Woff2).transformed_tables, None);
	let reader = reader(tt_woff2())?;
	let flavor_data = reader.flavor_data.as_ref().ok_or("no flavor data")?;
	let transformed = flavor_data
		.transformed_tables
		.as_deref()
		.ok_or("no tables")?;
	assert_eq!(sorted(transformed), [GLYF, LOCA]);
	Ok(())
}

/// `test_transformedTables_invalid`.
#[test]
fn test_transformedTables_invalid() {
	let message = "'glyf' and 'loca' must be transformed (or not) together";
	err_contains(FlavorData::woff2(&[GLYF]), message);
	err_contains(FlavorData::woff2(&[LOCA]), message);
}

// --- WOFF2WriterTest and WOFF2WriterTTFTest

/// A WOFF 2.0 writer for the tables of `font` (`self.tags`, `DSIG` included).
fn new_writer(font: &Font, flavor_data: Option<FlavorData>) -> SfntWriter {
	SfntWriter::new(
		font.tags().len(),
		font.sfnt_version,
		Some(Flavor::Woff2),
		flavor_data,
	)
}

/// Writes all the tables of `font`, sorted, as WOFF 2.0.
fn write_tables(font: &mut Font, flavor_data: Option<FlavorData>) -> TestResult<Vec<u8>> {
	let mut writer = new_writer(font, flavor_data);
	let mut tags = font.tags();
	tags.sort();
	for tag in tags {
		writer.add(tag, &font.table_data(tag)?)?;
	}
	Ok(writer.close()?)
}

/// `test_DSIG_dropped`.
#[test]
fn test_DSIG_dropped() -> TestResult {
	for (path, _) in fonts() {
		let mut font = import(path)?;
		assert!(font.contains(DSIG));
		let data = write_tables(&mut font, None)?;
		assert_eq!(u16_at(&data, 12) as usize, font.tags().len() - 1);
		assert!(!reader(&data)?.contains(DSIG));
	}
	Ok(())
}

/// `test_no_rewrite_table`.
#[test]
fn test_no_rewrite_table() -> TestResult {
	for (path, _) in fonts() {
		let font = import(path)?;
		let mut writer = new_writer(&font, None);
		writer.add(Tag::new(b"ZZZZ"), b"\0")?;
		err_contains(writer.add(Tag::new(b"ZZZZ"), b"\0"), "cannot rewrite");
	}
	Ok(())
}

/// `test_num_tables`.
#[test]
fn test_writer_num_tables() -> TestResult {
	for (path, _) in fonts() {
		let font = import(path)?;
		let mut writer = new_writer(&font, None);
		writer.add(Tag::new(b"ABCD"), b"\0")?;
		err_contains(writer.close(), "wrong number of tables");
	}
	Ok(())
}

/// `test_required_tables`.
#[test]
fn test_required_tables() {
	let mut font = Font::new();
	font.flavor = Some(Flavor::Woff2);
	err_contains(
		font.write(&WriteOptions::default()),
		"missing required table",
	);
}

/// `test_head_transform_flag`.
#[test]
fn test_head_transform_flag() -> TestResult {
	for (path, file) in fonts() {
		let orig_flags = import(path)?.table_data(HEAD)?[16];
		let mut woff2_font = Font::read(file.to_vec(), ReadOptions::default())?;
		let modified_flags = woff2_font.table_data(HEAD)?[16];
		assert_ne!(orig_flags, modified_flags);
		assert_eq!(orig_flags, modified_flags & !0x08);
	}
	Ok(())
}

/// `test_tables_sorted_alphabetically`.
#[test]
fn test_tables_sorted_alphabetically() -> TestResult {
	for (path, file) in fonts() {
		let expected = tags_but_dsig(&import(path)?);
		let woff2_font = Font::read(file.to_vec(), ReadOptions::default())?;
		let tags: Vec<Tag> = woff2_font.reader().ok_or("no reader")?.tags().collect();
		assert_eq!(tags, expected);
	}
	Ok(())
}

fn is_true_type(font: &Font) -> bool {
	font.sfnt_version.0 == *b"\0\x01\0\0" || font.sfnt_version.0 == *b"true"
}

/// `normalise_table`: the data of `glyf`, `loca` or `head` as the WOFF 2.0 writer
/// normalises it.
fn normalise_table(path: &str, tag: Tag, padding: u8) -> TestResult<Vec<u8>> {
	let mut font = import(path)?;
	if tag == HEAD {
		font.require::<Head>()?.flags |= 1 << 11;
	}
	let mut data = Vec::new();
	if is_true_type(&font) {
		font.require::<Glyf>()?.padding = padding;
		let glyf = font.table_data(GLYF)?;
		let loca = font.table_data(LOCA)?;
		if tag == GLYF {
			data = glyf;
		} else if tag == LOCA {
			data = loca;
		}
	}
	if tag == HEAD {
		data = font.table_data(HEAD)?;
	}
	Ok(data)
}

/// `normalise_font`: the font, without `DSIG`, with tables sorted and normalised.
fn normalise_font(path: &str, padding: u8) -> TestResult<Vec<u8>> {
	let mut font = import(path)?;
	font.remove(DSIG);
	let saved = font.write(&WriteOptions::default())?;
	let reader = SfntReader::new(saved, SfntReaderOptions::default())?;
	let mut tags: Vec<Tag> = reader.tags().collect();
	tags.sort();
	let mut writer = SfntWriter::new(tags.len(), reader.sfnt_version, None, None);
	for tag in tags {
		if [GLYF, LOCA, HEAD].contains(&tag) {
			writer.add(tag, &normalise_table(path, tag, padding)?)?;
		} else {
			writer.add(tag, &reader.table_data(tag)?)?;
		}
	}
	Ok(writer.close()?)
}

fn checksum(data: &[u8]) -> u32 {
	fontoxide_ttlib::binary::calc_checksum(data)
}

/// The data of a table of a WOFF 2.0 font, with `glyf` rebuilt with 4-byte padding
/// (`w2font.reader.padding = 4`).
fn padded_table_data(font: &mut Font, tag: Tag) -> TestResult<Vec<u8>> {
	if tag == GLYF || tag == LOCA {
		font.require::<Glyf>()?.padding = 4;
		font.table_data(GLYF)?;
	}
	Ok(font.table_data(tag)?)
}

/// `test_checksums`.
#[test]
fn test_checksums() -> TestResult {
	for (path, file) in fonts() {
		let mut norm_font = Font::read(normalise_font(path, 4)?, NO_RECALC)?;
		let norm_reader = norm_font.reader().ok_or("no reader")?.clone();
		let mut w2_font = Font::read(file.to_vec(), NO_RECALC)?;
		for tag in tags_but_dsig(&import(path)?) {
			let mut w2_data = padded_table_data(&mut w2_font, tag)?;
			let mut norm_data = norm_reader.table_data(tag)?.to_vec();
			if tag == HEAD {
				w2_data[8..12].fill(0);
				norm_data[8..12].fill(0);
			}
			assert_eq!(checksum(&w2_data), checksum(&norm_data), "{path} {tag}");
		}
		assert_eq!(
			norm_font.require::<Head>()?.check_sum_adjustment,
			w2_font.require::<Head>()?.check_sum_adjustment,
			"{path}"
		);
	}
	Ok(())
}

/// `test_calcSFNTChecksumsLengthsAndOffsets`: the original lengths and the total
/// sfnt size (the end of the last original offset) of the normalised font; the
/// checksums give the checksum adjustment of `test_checksums`.
#[test]
fn test_calcSFNTChecksumsLengthsAndOffsets() -> TestResult {
	for (path, file) in fonts() {
		let norm = normalise_font(path, 4)?;
		let norm_reader = reader(&norm)?;
		let woff2 = Woff2File::parse(file)?;
		assert_eq!(u32_at(file, 16) as usize, norm.len(), "{path}");
		for entry in norm_reader.entries() {
			let w2 = woff2
				.entries
				.iter()
				.find(|e| e.tag == entry.tag)
				.ok_or("missing entry")?;
			assert_eq!(w2.orig_length, entry.length, "{path} {}", entry.tag);
		}
	}
	Ok(())
}

/// `test_bad_sfntVersion`.
#[test]
fn test_bad_sfntVersion() -> TestResult {
	for (path, _) in fonts() {
		let n = import(path)?.tags().len();
		let mut writer = SfntWriter::new(n, Tag::new(b"ZZZZ"), Some(Flavor::Woff2), None);
		for i in 0..n {
			writer.add(Tag([65 + i as u8; 4]), b"\0")?;
		}
		err_contains(writer.close(), "bad sfntVersion");
	}
	Ok(())
}

/// The padded length of the WOFF 2.0 test font (`self.length`).
fn padded_length(file: &[u8]) -> usize {
	(file.len() + 3) & !3
}

/// `test_calcTotalSize_no_flavorData`.
#[test]
fn test_calcTotalSize_no_flavorData() -> TestResult {
	for (path, file) in fonts() {
		let data = write_tables(&mut import(path)?, None)?;
		assert_eq!(data.len(), padded_length(file));
		assert_eq!(u32_at(&data, 8) as usize, data.len());
	}
	Ok(())
}

/// `test_calcTotalSize_with_metaData`.
#[test]
fn test_calcTotalSize_with_metaData() -> TestResult {
	for (path, file) in fonts() {
		let flavor_data = FlavorData {
			meta_data: Some(metadata()?),
			..FlavorData::new(Flavor::Woff2)
		};
		let data = write_tables(&mut import(path)?, Some(flavor_data))?;
		let (meta_offset, meta_length) = (u32_at(&data, 28), u32_at(&data, 32));
		assert_eq!(meta_offset as usize, padded_length(file));
		assert_eq!(data.len(), padded_length(file) + meta_length as usize);
		assert_eq!(u32_at(&data, 8) as usize, data.len());
	}
	Ok(())
}

/// `test_calcTotalSize_with_privData`.
#[test]
fn test_calcTotalSize_with_privData() -> TestResult {
	for (path, file) in fonts() {
		let flavor_data = FlavorData {
			priv_data: Some(PRIV_DATA.to_vec()),
			..FlavorData::new(Flavor::Woff2)
		};
		let data = write_tables(&mut import(path)?, Some(flavor_data))?;
		assert_eq!(data.len(), padded_length(file) + PRIV_DATA.len());
		assert_eq!(u32_at(&data, 8) as usize, data.len());
	}
	Ok(())
}

/// `test_calcTotalSize_with_metaData_and_privData`.
#[test]
fn test_calcTotalSize_with_metaData_and_privData() -> TestResult {
	for (path, file) in fonts() {
		let flavor_data = FlavorData {
			meta_data: Some(metadata()?),
			priv_data: Some(PRIV_DATA.to_vec()),
			..FlavorData::new(Flavor::Woff2)
		};
		let data = write_tables(&mut import(path)?, Some(flavor_data))?;
		let meta_length = (u32_at(&data, 32) as usize + 3) & !3;
		let expected = padded_length(file) + meta_length + PRIV_DATA.len();
		assert_eq!(data.len(), expected);
		assert_eq!(u32_at(&data, 8) as usize, data.len());
		assert_eq!(
			u32_at(&data, 40) as usize,
			padded_length(file) + meta_length
		);
	}
	Ok(())
}

/// `test_getVersion`: from `head.fontRevision` and from the flavor data. The
/// first case, a font without `head`, cannot be written.
#[test]
fn test_getVersion() -> TestResult {
	for (path, _) in fonts() {
		let mut font = import(path)?;
		let head = font.table_data(HEAD)?;
		let data = write_tables(&mut font, None)?;
		assert_eq!(data[24..28], head[4..8]);
		assert_eq!((u16_at(&data, 24), u16_at(&data, 26)), (1, 0));
		let flavor_data = FlavorData {
			major_version: Some(10),
			minor_version: Some(11),
			..FlavorData::new(Flavor::Woff2)
		};
		let data = write_tables(&mut font, Some(flavor_data))?;
		assert_eq!((u16_at(&data, 24), u16_at(&data, 26)), (10, 11));
	}
	Ok(())
}

/// `test_roundtrip_flavorData_version`.
#[test]
fn test_roundtrip_flavorData_version() -> TestResult {
	for (path, _) in fonts() {
		let flavor_data = FlavorData {
			major_version: Some(1),
			minor_version: Some(7),
			..FlavorData::new(Flavor::Woff2)
		};
		let data = write_tables(&mut import(path)?, Some(flavor_data))?;
		let reader = reader(&data)?;
		let read = reader.flavor_data.as_ref().ok_or("no flavor data")?;
		assert_eq!((read.major_version, read.minor_version), (Some(1), Some(7)));
	}
	Ok(())
}

/// `WOFF2WriterTest.test_hmtx_trasform`: no effect without `glyf`.
#[test]
fn test_hmtx_trasform_cff() -> TestResult {
	let flavor_data = FlavorData::woff2(&[GLYF, LOCA, HMTX])?;
	let data = write_tables(&mut import(OTX)?, Some(flavor_data))?;
	assert!(data == cff_woff2());
	Ok(())
}

/// `WOFF2WriterTTFTest.test_hmtx_trasform`.
#[test]
fn test_hmtx_trasform_ttf() -> TestResult {
	let hmtx_length = |tables: &[Tag]| -> TestResult<u32> {
		let data = write_tables(&mut import(TTX)?, Some(FlavorData::woff2(tables)?))?;
		Ok(Woff2File::parse(&data)?.entry(HMTX).length())
	};
	let uncompressed = hmtx_length(&[GLYF, LOCA])?;
	let compressed = hmtx_length(&[GLYF, LOCA, HMTX])?;
	assert!(compressed < uncompressed, "{compressed} < {uncompressed}");
	Ok(())
}

/// `WOFF2WriterTest.test_no_transforms`: no effect on a CFF font.
#[test]
fn test_no_transforms_cff() -> TestResult {
	let data = write_tables(&mut import(OTX)?, Some(FlavorData::woff2(&[])?))?;
	assert!(data == cff_woff2());
	Ok(())
}

/// `WOFF2WriterTTFTest.test_no_transforms`.
#[test]
fn test_no_transforms_ttf() -> TestResult {
	let data = write_tables(&mut import(TTX)?, Some(FlavorData::woff2(&[])?))?;
	assert!(data != tt_woff2());
	let reader = reader(&data)?;
	let flavor_data = reader.flavor_data.as_ref().ok_or("no flavor data")?;
	assert_eq!(flavor_data.transformed_tables.as_deref(), Some(&[][..]));
	Ok(())
}

/// `WOFF2WriterTTFTest.test_normaliseGlyfAndLoca`: the normalised tables differ
/// from the originals, and are what the writer stores (`glyf` and `loca`
/// transformed, so rebuilt with the same padding).
#[test]
fn test_normaliseGlyfAndLoca() -> TestResult {
	let mut font = import(TTX)?;
	let mut w2_font = Font::read(tt_woff2().to_vec(), NO_RECALC)?;
	for tag in [HEAD, LOCA, GLYF] {
		let mut norm = normalise_table(TTX, tag, 4)?;
		assert_ne!(font.table_data(tag)?, norm, "{tag}");
		let mut stored = padded_table_data(&mut w2_font, tag)?;
		if tag == HEAD {
			norm[8..12].fill(0);
			stored[8..12].fill(0);
		}
		assert_eq!(stored, norm, "{tag}");
	}
	Ok(())
}

/// fontTools fails on a `numberOfHMetrics` beyond the glyph count only when the
/// `hmtx` transform reaches the missing glyphs; with a proportional glyph that
/// needs the lsb array first, the table is transformed (checked with fontTools
/// 4.68.0).
#[test]
fn hmtx_transform_excess_number_of_h_metrics() -> TestResult {
	for first_lsb in [None, Some(7)] {
		let mut font = import(TTX)?;
		let num_glyphs = font.glyph_order()?.len();
		if let Some(lsb) = first_lsb {
			let hmtx = font.require::<Hmtx>()?;
			let advance = hmtx.get(0).ok_or("no metric")?.0;
			hmtx.set(0, (advance, lsb));
		}
		let mut tags = font.tags();
		tags.sort();
		let flavor_data = FlavorData::woff2(&[GLYF, LOCA, HMTX])?;
		let mut writer = new_writer(&font, Some(flavor_data));
		for tag in tags {
			let mut data = font.table_data(tag)?;
			if tag == HHEA {
				data[34..36].copy_from_slice(&(num_glyphs as u16 + 3).to_be_bytes());
			}
			writer.add(tag, &data)?;
		}
		match first_lsb {
			None => err_contains(writer.close(), "IndexError"),
			Some(_) => {
				let data = writer.close()?;
				let hmtx = Woff2File::parse(&data)?.entry(HMTX).clone();
				assert_eq!(hmtx.transform_length, Some(25));
			}
		}
	}
	Ok(())
}

// --- WOFF2RoundtripTest

/// The `ttFont` fixture: the `fontfile` font built with `FontBuilder`.
fn fontfile() -> TestResult<Font> {
	let path =
		PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/beyond64k/woff2_fontfile.ttx");
	let mut font = Font::from_ttx(&std::fs::read_to_string(path)?)?;
	Ok(Font::read(
		font.write(&WriteOptions::default())?,
		NO_RECALC,
	)?)
}

/// `roundtrip`: reads `data` and writes it again.
fn roundtrip(data: &[u8]) -> TestResult<(Vec<u8>, Font)> {
	let mut font = Font::read(data.to_vec(), NO_RECALC)?;
	let out = font.write(&WriteOptions::default())?;
	Ok((out, font))
}

fn transformed_tables(font: &Font) -> TestResult<Vec<Tag>> {
	let reader = font.reader().ok_or("no reader")?;
	let flavor_data = reader.flavor_data.as_ref().ok_or("no flavor data")?;
	Ok(sorted(
		flavor_data
			.transformed_tables
			.as_deref()
			.ok_or("no tables")?,
	))
}

fn roundtrip_with(flavor_data: Option<FlavorData>, expected: &[Tag]) -> TestResult {
	let mut font = fontfile()?;
	font.flavor = Some(Flavor::Woff2);
	font.flavor_data = flavor_data;
	let tmp = font.write(&WriteOptions::default())?;
	let (tmp2, font2) = roundtrip(&tmp)?;
	assert!(tmp == tmp2);
	assert_eq!(transformed_tables(&font2)?, sorted(expected));
	Ok(())
}

/// `test_roundtrip_default_transforms`.
#[test]
fn test_roundtrip_default_transforms() -> TestResult {
	roundtrip_with(None, &[GLYF, LOCA])
}

/// `test_roundtrip_no_transforms`.
#[test]
fn test_roundtrip_no_transforms() -> TestResult {
	roundtrip_with(Some(FlavorData::woff2(&[])?), &[])
}

/// `test_roundtrip_all_transforms`.
#[test]
fn test_roundtrip_all_transforms() -> TestResult {
	let tables = [GLYF, LOCA, HMTX];
	roundtrip_with(Some(FlavorData::woff2(&tables)?), &tables)
}

/// `test_roundtrip_only_hmtx_no_glyf_transform`.
#[test]
fn test_roundtrip_only_hmtx_no_glyf_transform() -> TestResult {
	roundtrip_with(Some(FlavorData::woff2(&[HMTX])?), &[HMTX])
}

/// `test_roundtrip_no_glyf_and_loca_tables`.
#[test]
fn test_roundtrip_no_glyf_and_loca_tables() -> TestResult {
	let mut font = Font::new();
	font.import_ttx(&std::fs::read_to_string(upstream(
		"subset/data/google_color.ttx",
	))?)?;
	assert!(!font.contains(GLYF));
	assert!(!font.contains(LOCA));
	font.flavor = Some(Flavor::Woff2);
	let tmp = font.write(&WriteOptions::default())?;
	let (tmp2, _) = roundtrip(&tmp)?;
	assert!(tmp == tmp2);
	assert_eq!(font.flavor, Some(Flavor::Woff2));
	Ok(())
}

/// `test_roundtrip_off_curve_despite_overlap_bit`.
#[test]
fn test_roundtrip_off_curve_despite_overlap_bit() -> TestResult {
	let mut font = Font::new();
	font.import_ttx(&std::fs::read_to_string(upstream(
		"ttLib/data/woff2_overlap_offcurve_in.ttx",
	))?)?;
	let gid = font.glyph_id("A")?;
	let first_flag = |font: &mut Font| -> TestResult<u8> {
		let glyf = font.require::<Glyf>()?;
		let glyph = glyf.glyphs.get(&gid).ok_or("no glyph")?;
		Ok(*glyph.flags.first().ok_or("no flags")?)
	};
	assert_eq!(first_flag(&mut font)?, FLAG_OVERLAP_SIMPLE);
	font.flavor = Some(Flavor::Woff2);
	let tmp = font.write(&WriteOptions::default())?;
	let (_, mut font2) = roundtrip(&tmp)?;
	assert_eq!(font2.flavor, Some(Flavor::Woff2));
	let flag = first_flag(&mut font2)?;
	assert_eq!(flag & FLAG_ON_CURVE, 0);
	assert_ne!(flag & FLAG_OVERLAP_SIMPLE, 0);
	Ok(())
}

// --- Base128Test and UShort255Test (also the doctests of `unpackBase128`,
// `base128Size`, `packBase128`, `unpack255UShort` and `pack255UShort`)

/// `test_unpackBase128`.
#[test]
fn test_unpackBase128() -> TestResult {
	assert_eq!(unpack_base128(b"\x3f\x00\x00")?, (63, 1));
	assert_eq!(unpack_base128(b"\x8f\xff\xff\xff\x7f")?.0, 4294967295);
	err_contains(
		unpack_base128(b"\x80\x80\x3f"),
		"UIntBase128 value must not start with leading zeros",
	);
	err_contains(
		unpack_base128(b"\x8f\xff\xff\xff\xff\x7f"),
		"UIntBase128-encoded sequence is longer than 5 bytes",
	);
	err_contains(
		unpack_base128(b"\x90\x80\x80\x80\x00"),
		"UIntBase128 value exceeds 2**32-1",
	);
	err_contains(unpack_base128(b""), "not enough data to unpack UIntBase128");
	Ok(())
}

/// `test_base128Size`: the length of [`pack_base128`].
#[test]
fn test_base128Size() {
	assert_eq!(pack_base128(0).len(), 1);
	assert_eq!(pack_base128(24567).len(), 3);
	assert_eq!(pack_base128(u32::MAX).len(), 5);
}

/// `test_packBase128`. Values outside `0..2**32` cannot be passed.
#[test]
fn test_packBase128() {
	assert_eq!(pack_base128(63), b"\x3f");
	assert_eq!(pack_base128(u32::MAX), b"\x8f\xff\xff\xff\x7f");
}

/// `test_unpack255UShort`.
#[test]
fn test_unpack255UShort() -> TestResult {
	assert_eq!(unpack_255_ushort(&[252])?.0, 252);
	assert_eq!(unpack_255_ushort(&[254, 0])?.0, 506);
	assert_eq!(unpack_255_ushort(&[255, 253])?.0, 506);
	assert_eq!(unpack_255_ushort(&[253, 1, 250])?.0, 506);
	let message = "not enough data to unpack 255UInt16";
	err_contains(unpack_255_ushort(&[253, 0]), message);
	err_contains(unpack_255_ushort(&[254]), message);
	err_contains(unpack_255_ushort(&[255]), message);
	Ok(())
}

/// `test_pack255UShort`.
#[test]
fn test_pack255UShort() -> TestResult {
	assert_eq!(pack_255_ushort(252)?, b"\xfc");
	assert_eq!(pack_255_ushort(505)?, b"\xff\xfc");
	assert_eq!(pack_255_ushort(506)?, b"\xfe\x00");
	assert_eq!(pack_255_ushort(762)?, b"\xfd\x02\xfa");
	let message = "255UInt16 format requires 0 <= integer <= 65535";
	err_contains(pack_255_ushort(-1), message);
	err_contains(pack_255_ushort(0xFFFF + 1), message);
	Ok(())
}

// --- VarCompositeTest and CubicTest

/// `VarCompositeTest.test_var_composite`.
#[test]
fn test_var_composite() -> TestResult {
	let data = std::fs::read(upstream("ttLib/data/varc-ac00-ac01.ttf"))?;
	let mut font = Font::read(data, ReadOptions::default())?;
	font.flavor = Some(Flavor::Woff2);
	let out = font.write(&WriteOptions::default())?;
	let mut font = Font::read(out, ReadOptions::default())?;
	font.flavor = None;
	font.write(&WriteOptions::default())?;
	Ok(())
}

/// `CubicTest.test_cubic`.
#[test]
fn test_cubic() -> TestResult {
	let data = std::fs::read(upstream("ttLib/tables/data/NotoSans-VF-cubic.subset.ttf"))?;
	let mut font = Font::read(data, ReadOptions::default())?;
	upper_tables(
		&mut font,
		&ConvertOptions {
			tables: Some(&["glyf", "loca", "maxp", "hhea", "hmtx", "gvar"]),
			..Default::default()
		},
	)?;
	let mut pen1 = RecordingPen::new();
	GlyphSet::new(&mut font)?.draw("a", &mut pen1)?;
	font.flavor = Some(Flavor::Woff2);
	let out = font.write(&WriteOptions::default())?;

	let mut font = Font::read(out, ReadOptions::default())?;
	font.flavor = None;
	let mut pen2 = RecordingPen::new();
	GlyphSet::new(&mut font)?.draw("a", &mut pen2)?;
	font.write(&WriteOptions::default())?;

	assert_eq!(pen1.value, pen2.value);
	Ok(())
}

// --- MainTest: the command line, `fontoxide_ttlib::woff2_cli`

mod main_test {
	use std::path::{Path, PathBuf};
	use std::sync::atomic::{AtomicUsize, Ordering};

	use fontoxide_ttlib::woff2_cli::{main, run};
	use fontoxide_ttlib::{Font, ReadOptions, Tag, WriteOptions};

	use super::{OTX, TTX, TestResult, cff_woff2, import, tt_woff2};

	/// A temporary directory, removed when dropped (`tmpdir`).
	struct TempDir(PathBuf);

	impl TempDir {
		fn new() -> std::io::Result<Self> {
			static COUNTER: AtomicUsize = AtomicUsize::new(0);
			let path = std::env::temp_dir().join(format!(
				"fontoxide-ttlib-woff2-main-{}-{}",
				std::process::id(),
				COUNTER.fetch_add(1, Ordering::Relaxed)
			));
			std::fs::create_dir_all(&path)?;
			Ok(TempDir(path))
		}

		fn join(&self, name: &str) -> String {
			self.0.join(name).to_string_lossy().into_owned()
		}
	}

	impl Drop for TempDir {
		fn drop(&mut self) {
			let _ = std::fs::remove_dir_all(&self.0);
		}
	}

	/// `make_ttf`, and the same for `OTX`.
	fn make_font(tmpdir: &TempDir, ttx: &str, file_name: &str) -> TestResult<String> {
		let mut font = import(ttx)?;
		let filename = tmpdir.join(file_name);
		std::fs::write(&filename, font.write(&WriteOptions::default())?)?;
		Ok(filename)
	}

	fn make_ttf(tmpdir: &TempDir) -> TestResult<String> {
		make_font(tmpdir, TTX, "TestTTF-Regular.ttf")
	}

	fn is_file(path: &str) -> bool {
		Path::new(path).is_file()
	}

	#[test]
	fn test_compress_ttf() -> TestResult {
		let tmpdir = TempDir::new()?;
		let input_file = make_ttf(&tmpdir)?;
		assert_eq!(main(&["compress", &input_file])?, 0);
		assert!(is_file(&tmpdir.join("TestTTF-Regular.woff2")));
		Ok(())
	}

	#[test]
	fn test_compress_ttf_no_glyf_transform() -> TestResult {
		let tmpdir = TempDir::new()?;
		let input_file = make_ttf(&tmpdir)?;
		assert_eq!(main(&["compress", "--no-glyf-transform", &input_file])?, 0);
		assert!(is_file(&tmpdir.join("TestTTF-Regular.woff2")));
		Ok(())
	}

	#[test]
	fn test_compress_ttf_hmtx_transform() -> TestResult {
		let tmpdir = TempDir::new()?;
		let input_file = make_ttf(&tmpdir)?;
		assert_eq!(main(&["compress", "--hmtx-transform", &input_file])?, 0);
		assert!(is_file(&tmpdir.join("TestTTF-Regular.woff2")));
		Ok(())
	}

	#[test]
	fn test_compress_ttf_no_glyf_transform_hmtx_transform() -> TestResult {
		let tmpdir = TempDir::new()?;
		let input_file = make_ttf(&tmpdir)?;
		let args = [
			"compress",
			"--no-glyf-transform",
			"--hmtx-transform",
			&input_file,
		];
		assert_eq!(main(&args)?, 0);
		let output = tmpdir.join("TestTTF-Regular.woff2");
		assert!(is_file(&output));
		let font = Font::read(std::fs::read(&output)?, ReadOptions::default())?;
		let transformed = font.flavor_data.and_then(|d| d.transformed_tables);
		assert_eq!(transformed, Some(vec![Tag::new(b"hmtx")]));
		Ok(())
	}

	#[test]
	fn test_compress_output_file() -> TestResult {
		let tmpdir = TempDir::new()?;
		let input_file = make_ttf(&tmpdir)?;
		let output_file = tmpdir.join("TestTTF.woff2");
		assert_eq!(main(&["compress", "-o", &output_file, &input_file])?, 0);
		assert!(is_file(&output_file));
		Ok(())
	}

	#[test]
	fn test_compress_otf() -> TestResult {
		let tmpdir = TempDir::new()?;
		let input_file = make_font(&tmpdir, OTX, "TestOTF-Regular.otf")?;
		assert_eq!(main(&["compress", &input_file])?, 0);
		assert!(is_file(&tmpdir.join("TestOTF-Regular.woff2")));
		Ok(())
	}

	fn sorted(mut tags: Vec<Tag>) -> Vec<Tag> {
		tags.sort();
		tags
	}

	#[test]
	fn test_recompress_woff2_keeps_flavorData() -> TestResult {
		let tmpdir = TempDir::new()?;
		let mut woff2_font = Font::read(tt_woff2().to_vec(), ReadOptions::default())?;
		let flavor_data = woff2_font.flavor_data.as_mut().ok_or("no flavorData")?;
		flavor_data.priv_data = Some(b"FOOBAR".to_vec());
		let transformed = flavor_data.transformed_tables.clone().unwrap_or_default();
		let woff2_file = tmpdir.join("TestTTF-Regular.woff2");
		std::fs::write(&woff2_file, woff2_font.write(&WriteOptions::default())?)?;

		assert_eq!(sorted(transformed), [Tag::new(b"glyf"), Tag::new(b"loca")]);

		assert_eq!(main(&["compress", "--hmtx-transform", &woff2_file])?, 0);

		let output_file = tmpdir.join("TestTTF-Regular#1.woff2");
		assert!(is_file(&output_file));

		let new_woff2_font = Font::read(std::fs::read(&output_file)?, ReadOptions::default())?;
		let flavor_data = new_woff2_font.flavor_data.ok_or("no flavorData")?;
		assert_eq!(
			sorted(flavor_data.transformed_tables.unwrap_or_default()),
			[Tag::new(b"glyf"), Tag::new(b"hmtx"), Tag::new(b"loca")]
		);
		assert_eq!(flavor_data.priv_data.as_deref(), Some(&b"FOOBAR"[..]));
		Ok(())
	}

	#[test]
	fn test_decompress_ttf() -> TestResult {
		let tmpdir = TempDir::new()?;
		let input_file = tmpdir.join("TestTTF-Regular.woff2");
		std::fs::write(&input_file, tt_woff2())?;
		assert_eq!(main(&["decompress", &input_file])?, 0);
		assert!(is_file(&tmpdir.join("TestTTF-Regular.ttf")));
		Ok(())
	}

	#[test]
	fn test_decompress_otf() -> TestResult {
		let tmpdir = TempDir::new()?;
		let input_file = tmpdir.join("TestTTF-Regular.woff2");
		std::fs::write(&input_file, cff_woff2())?;
		assert_eq!(main(&["decompress", &input_file])?, 0);
		assert!(is_file(&tmpdir.join("TestTTF-Regular.otf")));
		Ok(())
	}

	#[test]
	fn test_decompress_output_file() -> TestResult {
		let tmpdir = TempDir::new()?;
		let input_file = tmpdir.join("TestTTF-Regular.woff2");
		std::fs::write(&input_file, tt_woff2())?;
		let output_file = tmpdir.join("TestTTF.ttf");
		assert_eq!(main(&["decompress", "-o", &output_file, &input_file])?, 0);
		assert!(is_file(&output_file));
		Ok(())
	}

	#[test]
	fn test_no_subcommand_show_help() -> TestResult {
		let mut out = Vec::new();
		assert_eq!(run(&["--help"], &mut out)?, 0);
		let captured = String::from_utf8(out)?;
		assert!(captured.contains("usage: fonttools ttLib.woff2"));
		Ok(())
	}

	/// Compressing and decompressing round-trips the tables.
	#[test]
	fn compress_decompress_roundtrip() -> TestResult {
		let tmpdir = TempDir::new()?;
		let input_file = make_ttf(&tmpdir)?;
		assert_eq!(main(&["compress", "-q", &input_file])?, 0);
		let woff2 = tmpdir.join("TestTTF-Regular.woff2");
		assert_eq!(main(&["decompress", "-q", &woff2])?, 0);
		let decompressed = tmpdir.join("TestTTF-Regular#1.ttf");
		let mut original = Font::read(std::fs::read(&input_file)?, ReadOptions::default())?;
		let mut roundtrip = Font::read(std::fs::read(&decompressed)?, ReadOptions::default())?;
		for tag in original.tags() {
			// DSIG is dropped; glyf and loca are rebuilt; head is updated.
			if [b"DSIG", b"head", b"glyf", b"loca"].contains(&&tag.0) {
				continue;
			}
			assert_eq!(
				original.table_data(tag)?,
				roundtrip.table_data(tag)?,
				"{tag}"
			);
		}
		Ok(())
	}
}
