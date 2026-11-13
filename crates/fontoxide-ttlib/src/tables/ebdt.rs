//! `EBDT`, `CBDT` and `bdat`: bitmap data (`fontTools.ttLib.tables.E_B_D_T_`,
//! `C_B_D_T_`, `_b_d_a_t`).

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use fontoxide_misc::Tag;
use fontoxide_misc::text_tools::{de_hex_str, hex_str};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes, fit};
use crate::sstruct::{F16Dot16, I8, U16, Value, eval};
use crate::table::{Table, TableType, TableXml, read_hex};
use crate::tables::eblc::{
	BigGlyphMetrics, BitmapLocator, SmallGlyphMetrics, clip, int_attr, literal, metrics_to_xml,
	required_attr,
};
use crate::{Error, Font, Result};

crate::sstruct! {
	/// The header (`ebdtTableVersionFormat`).
	pub struct EbdtHeader = "ebdtTableVersionFormat" {
		/// The table version.
		pub version: f64 = "version" as F16Dot16,
	}
}

crate::sstruct! {
	/// A component of a composite bitmap, as stored (`ebdtComponentFormat`).
	pub struct EbdtComponentRecord = "ebdtComponentFormat" {
		/// The glyph ID of the component.
		pub glyph_code: u16 = "glyphCode" as U16,
		/// Horizontal offset.
		pub x_offset: i8 = "xOffset" as I8,
		/// Vertical offset.
		pub y_offset: i8 = "yOffset" as I8,
	}
}

/// A component of a composite bitmap (`EbdtComponent`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EbdtComponent {
	/// The glyph ID of the component (`name`, a glyph name in fontTools).
	pub glyph: u32,
	/// Horizontal offset (`xOffset`).
	pub x_offset: i8,
	/// Vertical offset (`yOffset`).
	pub y_offset: i8,
}

/// The metrics of a parsed bitmap glyph.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum GlyphMetrics {
	/// The format has no metrics, or a TTX document did not give them.
	#[default]
	None,
	/// `SmallGlyphMetrics`.
	Small(SmallGlyphMetrics),
	/// `BigGlyphMetrics`.
	Big(BigGlyphMetrics),
}

/// A bitmap glyph with its data parsed (`BitmapGlyph` after `decompile`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParsedBitmapGlyph {
	/// The image format.
	pub format: u16,
	/// The metrics (`metrics`).
	pub metrics: GlyphMetrics,
	/// The image (`imageData`).
	pub image_data: Option<Vec<u8>>,
	/// The components, in formats 8 and 9 (`componentArray`).
	pub components: Option<Vec<EbdtComponent>>,
}

/// A bitmap glyph (`BitmapGlyph` and its subclasses).
#[derive(Debug, Clone, PartialEq)]
pub enum BitmapGlyph {
	/// The data as read, parsed only when the glyph is used.
	///
	/// As in fontTools, malformed data only fails if the glyph is used.
	///
	/// UPSTREAM-BUG(B22): the components of formats 8 and 9 are only resolved then
	/// too, with the current glyph order, so that after subsetting the glyph IDs
	/// stay as they are, whichever glyphs they now refer to. Here they are numbers,
	/// and [`Table::remap_glyphs`] leaves this data alone.
	Raw {
		/// The image format.
		format: u16,
		/// The data.
		data: Vec<u8>,
	},
	/// The parsed glyph; read from TTX.
	Parsed(ParsedBitmapGlyph),
}

fn missing(attribute: &str) -> Error {
	Error::ttlib(format!("AttributeError: {attribute}"))
}

/// The error of `sstruct.getformat(ebdtComponentFormat)[1][1:]`.
fn component_format_error() -> Error {
	Error::ttlib("KeyError: slice(1, None, None)")
}

/// Whether a format is one of `ebdt_bitmap_classes`.
fn is_ebdt_format(format: u16) -> bool {
	matches!(format, 1 | 2 | 5..=9)
}

/// Whether a format is one of `cbdt_bitmap_classes`.
fn is_cbdt_format(format: u16) -> bool {
	matches!(format, 17..=19)
}

fn check_format<const COLOR: bool>(format: u16) -> Result<()> {
	if is_ebdt_format(format) || (COLOR && is_cbdt_format(format)) {
		Ok(())
	} else {
		Err(Error::ttlib(format!("KeyError: {format}")))
	}
}

/// Formats that use `BitAlignedBitmapMixin`.
fn is_bit_aligned(format: u16) -> bool {
	matches!(format, 2 | 5 | 7)
}

/// Formats that use `ByteAlignedBitmapMixin`.
fn is_byte_aligned(format: u16) -> bool {
	matches!(format, 1 | 6)
}

fn read_components(mut data: &[u8]) -> Result<Vec<EbdtComponent>> {
	let mut r = Reader::new(data);
	let count = r.u16()?;
	data = r.rest();
	let mut components = Vec::new();
	for _ in 0..count {
		let (c, rest) = EbdtComponentRecord::unpack2(data)?;
		data = rest;
		components.push(EbdtComponent {
			glyph: u32::from(c.glyph_code),
			x_offset: c.x_offset,
			y_offset: c.y_offset,
		});
	}
	Ok(components)
}

/// The length-prefixed image of formats 17 to 19.
fn read_prefixed(data: &[u8], format: u16) -> Result<Vec<u8>> {
	let mut r = Reader::new(data);
	let len = r.u32()? as usize;
	let rest = r.rest();
	if len > rest.len() {
		return Err(Error::Assertion(format!("Data overun in format {format}")));
	}
	Ok(rest[..len].to_vec())
}

impl ParsedBitmapGlyph {
	/// Parses the data of a glyph in an image format (`decompile`).
	pub fn parse(format: u16, data: &[u8]) -> Result<Self> {
		let mut glyph = ParsedBitmapGlyph {
			format,
			..Default::default()
		};
		match format {
			1 | 2 => {
				let (metrics, rest) = SmallGlyphMetrics::unpack2(data)?;
				glyph.metrics = GlyphMetrics::Small(metrics);
				glyph.image_data = Some(rest.to_vec());
			}
			5 => glyph.image_data = Some(data.to_vec()),
			6 | 7 => {
				let (metrics, rest) = BigGlyphMetrics::unpack2(data)?;
				glyph.metrics = GlyphMetrics::Big(metrics);
				glyph.image_data = Some(rest.to_vec());
			}
			8 => {
				let (metrics, rest) = SmallGlyphMetrics::unpack2(data)?;
				glyph.metrics = GlyphMetrics::Small(metrics);
				glyph.components = Some(read_components(rest.get(1..).unwrap_or_default())?);
			}
			9 => {
				let (metrics, rest) = BigGlyphMetrics::unpack2(data)?;
				glyph.metrics = GlyphMetrics::Big(metrics);
				glyph.components = Some(read_components(rest)?);
			}
			17 => {
				let (metrics, rest) = SmallGlyphMetrics::unpack2(data)?;
				glyph.metrics = GlyphMetrics::Small(metrics);
				glyph.image_data = Some(read_prefixed(rest, format)?);
			}
			18 => {
				let (metrics, rest) = BigGlyphMetrics::unpack2(data)?;
				glyph.metrics = GlyphMetrics::Big(metrics);
				glyph.image_data = Some(read_prefixed(rest, format)?);
			}
			19 => glyph.image_data = Some(read_prefixed(data, format)?),
			f => return Err(Error::ttlib(format!("KeyError: {f}"))),
		}
		Ok(glyph)
	}

	fn pack_metrics(&self, out: &mut Vec<u8>) -> Result<()> {
		match (&self.metrics, self.format) {
			(GlyphMetrics::Small(m), 1 | 2 | 8 | 17) => m.pack(out),
			(GlyphMetrics::Big(m), 6 | 7 | 9 | 18) => m.pack(out),
			(_, 5 | 19) => Ok(()),
			_ => Err(missing("metrics")),
		}
	}

	fn image(&self) -> Result<&[u8]> {
		self.image_data
			.as_deref()
			.ok_or_else(|| missing("imageData"))
	}

	/// The data of the glyph (`compile`).
	pub fn compile(&self) -> Result<Vec<u8>> {
		let mut out = Vec::new();
		self.pack_metrics(&mut out)?;
		match self.format {
			1 | 2 | 5 | 6 | 7 => out.extend_from_slice(self.image()?),
			8 | 9 => {
				if self.format == 8 {
					out.put_u8(0);
				}
				let components = self
					.components
					.as_ref()
					.ok_or_else(|| missing("componentArray"))?;
				out.put_u16(fit(components.len(), "numComponents")?);
				for c in components {
					EbdtComponentRecord {
						glyph_code: fit(c.glyph, "glyphCode")?,
						x_offset: c.x_offset,
						y_offset: c.y_offset,
					}
					.pack(&mut out)?;
				}
			}
			17..=19 => {
				let image = self.image()?;
				out.put_u32(fit(image.len(), "dataLen")?);
				out.extend_from_slice(image);
			}
			f => return Err(Error::ttlib(format!("KeyError: {f}"))),
		}
		Ok(out)
	}

	/// The width and height, if the glyph has metrics.
	fn size(&self) -> Option<(usize, usize)> {
		match &self.metrics {
			GlyphMetrics::None => None,
			GlyphMetrics::Small(m) => Some((usize::from(m.width), usize::from(m.height))),
			GlyphMetrics::Big(m) => Some((usize::from(m.width), usize::from(m.height))),
		}
	}

	/// `getRow` of `BitAlignedBitmapMixin` and `ByteAlignedBitmapMixin`.
	fn row(&self, row: usize, bit_depth: usize, width: usize, reverse: bool) -> Result<Vec<u8>> {
		let image = self.image()?;
		if is_byte_aligned(self.format) {
			let row_bytes = (bit_depth * width).div_ceil(8);
			let start = (row * row_bytes) as u64;
			let data = clip(image, start, start + row_bytes as u64);
			return Ok(if reverse {
				reverse_bytes(data)
			} else {
				data.to_vec()
			});
		}
		if !is_bit_aligned(self.format) {
			return Err(missing("getRow"));
		}
		let row_bits = bit_depth * width;
		let start = row * row_bits;
		let end = start + row_bits;
		let mut out = Vec::with_capacity(row_bits.div_ceil(8));
		for cur_bit in (start..end).step_by(8) {
			let end_bit = (cur_bit + 8).min(end);
			let num_bits = end_bit - cur_bit;
			let cut_point = cur_bit % 8;
			let first = cur_bit / 8;
			let second = end_bit / 8;
			let num_bits_cut = if first < second {
				8 - cut_point
			} else {
				end_bit - cur_bit
			};
			let byte = image
				.get(first)
				.ok_or_else(|| Error::ttlib("IndexError: index out of range"))?
				.reverse_bits();
			let first_half = (u32::from(byte) >> cut_point) & ((1 << num_bits_cut) - 1);
			let mut new_byte = first_half;
			if first < second
				&& let Some(next) = image.get(second)
			{
				let second_half = u32::from(next.reverse_bits()) << num_bits_cut;
				new_byte = (first_half | second_half) & ((1 << num_bits) - 1);
			}
			out.push(new_byte as u8);
		}
		Ok(if reverse { out } else { reverse_bytes(&out) })
	}

	/// `setRows` of `BitAlignedBitmapMixin` and `ByteAlignedBitmapMixin`.
	fn set_rows(
		&mut self,
		rows: Vec<Vec<u8>>,
		bit_depth: usize,
		width: usize,
		reverse: bool,
	) -> Result<()> {
		if is_byte_aligned(self.format) {
			let image = rows
				.iter()
				.flat_map(|row| {
					if reverse {
						reverse_bytes(row)
					} else {
						row.clone()
					}
				})
				.collect();
			self.image_data = Some(image);
			return Ok(());
		}
		if !is_bit_aligned(self.format) {
			return Err(missing("setRows"));
		}
		let row_bits = bit_depth * width;
		let num_bytes = (rows.len() * row_bits).div_ceil(8);
		let mut image = vec![0u8; num_bytes];
		for (row, data) in rows.iter().enumerate() {
			let start = row * row_bits;
			let end = start + row_bits;
			for (cur_bit, &byte) in (start..end).step_by(8).zip(data) {
				let byte = if reverse { byte } else { byte.reverse_bits() };
				let end_bit = (cur_bit + 8).min(end);
				let cut_point = cur_bit % 8;
				let first = cur_bit / 8;
				let second = end_bit / 8;
				let num_bits_cut = if first < second {
					8 - cut_point
				} else {
					end_bit - cur_bit
				};
				let first_byte = u32::from(byte) & ((1 << num_bits_cut) - 1);
				image[first] |= (first_byte << cut_point) as u8;
				if first < second && second < num_bytes {
					let second_byte =
						(u32::from(byte) >> num_bits_cut) & ((1 << (8 - num_bits_cut)) - 1);
					image[second] |= second_byte as u8;
				}
			}
		}
		self.image_data = Some(reverse_bytes(&image));
		Ok(())
	}

	fn metrics_to_xml(&self, writer: &mut XmlWriter) {
		match &self.metrics {
			GlyphMetrics::None => {}
			GlyphMetrics::Small(m) => metrics_to_xml(writer, "SmallGlyphMetrics", m.values()),
			GlyphMetrics::Big(m) => metrics_to_xml(writer, "BigGlyphMetrics", m.values()),
		}
	}

	fn from_xml(format: u16, element: &Element) -> Result<Self> {
		let mut glyph = ParsedBitmapGlyph {
			format,
			..Default::default()
		};
		let small = matches!(format, 1 | 2 | 8 | 17);
		let big = matches!(format, 6 | 7 | 9 | 18);
		for child in element.elements() {
			match child.name.as_str() {
				"SmallGlyphMetrics" if small => {
					let mut metrics = SmallGlyphMetrics::default();
					for m in child.elements() {
						if !metrics.set_literal(&m.name, &literal(m)?)? {
							log::warn!(
								"unknown name '{}' being ignored in SmallGlyphMetrics.",
								m.name
							);
						}
					}
					glyph.metrics = GlyphMetrics::Small(metrics);
				}
				"BigGlyphMetrics" if big => {
					let mut metrics = BigGlyphMetrics::default();
					for m in child.elements() {
						if !metrics.set_literal(&m.name, &literal(m)?)? {
							log::warn!(
								"unknown name '{}' being ignored in BigGlyphMetrics.",
								m.name
							);
						}
					}
					glyph.metrics = GlyphMetrics::Big(metrics);
				}
				"SmallGlyphMetrics" | "BigGlyphMetrics" if small || big => {
					log::warn!("Warning: {} being ignored in format {format}.", child.name);
				}
				_ => {}
			}
		}
		if matches!(format, 8 | 9) {
			for child in element.elements() {
				if child.name != "components" {
					continue;
				}
				for c in child.elements() {
					if c.name != "ebdtComponent" {
						log::warn!("'{}' being ignored in component array.", c.name);
						continue;
					}
					// UPSTREAM-BUG(B21): `EbdtComponent.fromXML` slices a dict, so a
					// composite with components cannot be read.
					return Err(component_format_error());
				}
				glyph.components = Some(Vec::new());
			}
			return Ok(glyph);
		}
		for child in element.elements() {
			let Some(option) = child.name.strip_suffix("imagedata") else {
				continue;
			};
			match option {
				"raw" => glyph.image_data = Some(read_hex(child)?),
				"row" if !is_cbdt_format(format) => glyph.read_rows(child)?,
				"bitwise" => glyph.read_bitwise(child)?,
				"extfile" => {
					glyph.image_data = Some(std::fs::read(required_attr(child, "value")?)?);
				}
				_ => {
					return Err(Error::Assertion(format!(
						"unknown image data option: {option}"
					)));
				}
			}
		}
		Ok(glyph)
	}

	/// `_readRowImageData`.
	fn read_rows(&mut self, element: &Element) -> Result<()> {
		let bit_depth: usize = int_attr(element, "bitDepth")?;
		let width: usize = int_attr(element, "width")?;
		let _: i64 = int_attr(element, "height")?;
		let mut rows = Vec::new();
		for child in element.elements() {
			if child.name == "row" {
				let value = required_attr(child, "value")?;
				rows.push(de_hex_str(value).ok_or_else(|| Error::value("invalid hex data"))?);
			}
		}
		self.set_rows(rows, bit_depth, width, false)
	}

	/// `_readBitwiseImageData`.
	fn read_bitwise(&mut self, element: &Element) -> Result<()> {
		let bit_depth: usize = int_attr(element, "bitDepth")?;
		let width: usize = int_attr(element, "width")?;
		let _: i64 = int_attr(element, "height")?;
		let mut rows = Vec::new();
		for child in element.elements() {
			if child.name == "row" {
				let bits: Vec<bool> = required_attr(child, "value")?
					.chars()
					.map(|c| !matches!(c, ' ' | '.' | '0'))
					.collect();
				// `_binary2data`.
				let row = bits
					.chunks(8)
					.map(|chunk| {
						chunk
							.iter()
							.rev()
							.fold(0u8, |byte, &bit| (byte << 1) | u8::from(bit))
					})
					.collect();
				rows.push(row);
			}
		}
		self.set_rows(rows, bit_depth, width, true)
	}
}

/// `_reverseBytes`: reverses the bits of each byte.
fn reverse_bytes(data: &[u8]) -> Vec<u8> {
	data.iter().map(|b| b.reverse_bits()).collect()
}

/// `_extFileName`: the file name for a glyph name, which may come straight from
/// the font and contain path separators.
fn ext_file_name(glyph_name: &str) -> String {
	if !matches!(glyph_name, "" | "." | "..") && !glyph_name.contains(['%', '/', '\\']) {
		return glyph_name.to_owned();
	}
	let mut out = String::from("%");
	for &b in glyph_name.as_bytes() {
		if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~') {
			out.push(b as char);
		} else {
			out.push_str(&format!("%{b:02X}"));
		}
	}
	out
}

/// The glyphs of a strike (the dictionaries of `strikeData`): glyph IDs in order
/// of insertion, and the glyph of each in [`BitmapData::glyphs`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StrikeData {
	order: Vec<u32>,
	glyphs: HashMap<u32, usize>,
}

impl StrikeData {
	/// Adds a glyph, by its index in [`BitmapData::glyphs`]; a glyph that is
	/// already there keeps its place.
	pub fn insert(&mut self, gid: u32, index: usize) {
		if self.glyphs.insert(gid, index).is_none() {
			self.order.push(gid);
		}
	}

	/// The glyph IDs of the strike, in order.
	pub fn glyph_ids(&self) -> &[u32] {
		&self.order
	}

	/// Whether the strike has no glyphs.
	pub fn is_empty(&self) -> bool {
		self.order.is_empty()
	}

	/// Keeps only the given glyphs.
	pub fn retain(&mut self, glyphs: &BTreeSet<u32>) {
		self.order.retain(|g| glyphs.contains(g));
		self.glyphs.retain(|g, _| glyphs.contains(g));
	}
}

/// The size and bit depth used to dump a glyph (`exportMetrics`, `exportBitDepth`).
#[derive(Clone, Copy)]
struct Export {
	width: usize,
	height: usize,
	bit_depth: usize,
}

/// Embedded (`COLOR` false) or Color (`COLOR` true) Bitmap Data table
/// (`table_E_B_D_T_`, `table_C_B_D_T_`), or with `APPLE`, Apple's `bdat`
/// (`table__b_d_a_t`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/ebdt> and
/// <https://learn.microsoft.com/en-us/typography/opentype/spec/cbdt>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BitmapData<const COLOR: bool, const APPLE: bool = false> {
	/// The table version.
	pub version: f64,
	/// All glyphs. Strikes refer to them by index, so that glyphs at the same
	/// location share an entry (object identity in fontTools).
	pub glyphs: Vec<BitmapGlyph>,
	/// The glyphs by strike (`strikeData`). Only a TTX document with gaps in its
	/// strike indices leaves `None`s.
	pub strike_data: Vec<Option<StrikeData>>,
}

/// `EBDT`.
pub type Ebdt = BitmapData<false>;
/// `CBDT`.
pub type Cbdt = BitmapData<true>;
/// `bdat`: Apple's `EBDT` (`table__b_d_a_t`), with `bloc`.
pub type Bdat = BitmapData<false, true>;

impl BitmapGlyph {
	/// The glyph with its data parsed, which fails on malformed data.
	pub fn parsed(&self) -> Result<Cow<'_, ParsedBitmapGlyph>> {
		match self {
			BitmapGlyph::Raw { format, data } => {
				Ok(Cow::Owned(ParsedBitmapGlyph::parse(*format, data)?))
			}
			BitmapGlyph::Parsed(p) => Ok(Cow::Borrowed(p)),
		}
	}
}

impl<const COLOR: bool, const APPLE: bool> TableType for BitmapData<COLOR, APPLE> {
	const TAG: Tag = if COLOR {
		Tag::new(b"CBDT")
	} else if APPLE {
		Tag::new(b"bdat")
	} else {
		Tag::new(b"EBDT")
	};

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		let (header, _) = EbdtHeader::unpack2(data)?;
		let locator = font.require::<BitmapLocator<COLOR, APPLE>>()?;
		let mut seen: HashMap<(u64, u64), usize> = HashMap::new();
		let mut glyphs = Vec::new();
		let mut strike_data = Vec::with_capacity(locator.strikes.len());
		for strike in locator.strikes.iter().flatten() {
			let mut dict = StrikeData::default();
			for table in &strike.index_sub_tables {
				for (&gid, &location) in table.names.iter().zip(&table.locations) {
					let index = match seen.get(&location) {
						Some(&i) => i,
						None => {
							check_format::<COLOR>(table.image_format)?;
							glyphs.push(BitmapGlyph::Raw {
								format: table.image_format,
								data: clip(data, location.0, location.1).to_vec(),
							});
							seen.insert(location, glyphs.len() - 1);
							glyphs.len() - 1
						}
					};
					dict.insert(gid, index);
				}
			}
			strike_data.push(Some(dict));
		}
		Ok(BitmapData {
			version: header.version,
			glyphs,
			strike_data,
		})
	}
}

impl<const COLOR: bool, const APPLE: bool> BitmapData<COLOR, APPLE> {
	/// `writeData` for a glyph with image data.
	fn write_image(
		&self,
		writer: &mut XmlWriter,
		font: &Font,
		parsed: &ParsedBitmapGlyph,
		export: Option<Export>,
		strike_index: usize,
		glyph_name: &str,
	) -> Result<()> {
		let is_color = is_cbdt_format(parsed.format);
		match font.bitmap_glyph_data_format.as_str() {
			"row" if !is_color => {
				let e = export.ok_or_else(|| missing("exportMetrics"))?;
				writer.begintag(
					"rowimagedata",
					&[
						("bitDepth", &e.bit_depth),
						("width", &e.width),
						("height", &e.height),
					],
				);
				writer.newline();
				for row in 0..e.height {
					let data = parsed.row(row, e.bit_depth, e.width, false)?;
					writer.simpletag("row", &[("value", &hex_str(&data))]);
					writer.newline();
				}
				writer.endtag("rowimagedata");
				writer.newline();
			}
			"bitwise" => {
				let e = export.ok_or_else(|| missing("exportMetrics"))?;
				writer.begintag(
					"bitwiseimagedata",
					&[
						("bitDepth", &e.bit_depth),
						("width", &e.width),
						("height", &e.height),
					],
				);
				writer.newline();
				for row in 0..e.height {
					let data = parsed.row(row, 1, e.width, true)?;
					// `_data2binary`.
					let mut bits = String::with_capacity(e.width);
					let mut num_bits = e.width;
					for byte in data {
						for i in 0..num_bits.min(8) {
							bits.push(if byte >> i & 1 != 0 { '@' } else { '.' });
						}
						num_bits -= num_bits.min(8);
					}
					writer.simpletag("row", &[("value", &bits)]);
					writer.newline();
				}
				writer.endtag("bitwiseimagedata");
				writer.newline();
			}
			"extfile" => {
				// LENIENT(L23): a dump is a string here, so that the files go where
				// fontTools puts them if the output has no file name.
				let extension = if is_color { ".png" } else { ".bin" };
				let folder = Path::new(".")
					.join("bitmaps")
					.join(format!("strike{strike_index}"));
				std::fs::create_dir_all(&folder)?;
				let path: PathBuf =
					folder.join(format!("{}{extension}", ext_file_name(glyph_name)));
				writer.simpletag(
					"extfileimagedata",
					&[("value", &path.to_string_lossy().into_owned())],
				);
				writer.newline();
				std::fs::write(&path, parsed.image()?)?;
			}
			_ => {
				writer.begintag("rawimagedata", &[]);
				writer.newline();
				writer.dumphex(parsed.image()?);
				writer.endtag("rawimagedata");
				writer.newline();
			}
		}
		Ok(())
	}

	/// The sizes and bit depths for the `row` and `bitwise` formats.
	fn exports(&self, font: &mut Font) -> Result<HashMap<usize, Export>> {
		let mut exports = HashMap::new();
		let locator = font.require::<BitmapLocator<COLOR, APPLE>>()?;
		for (strike, dict) in locator.strikes.iter().zip(&self.strike_data) {
			let strike = strike.as_ref().ok_or_else(|| missing("indexSubTables"))?;
			let dict = dict.as_ref().ok_or_else(|| missing("strikeData"))?;
			let bit_depth = usize::from(strike.bitmap_size_table.part2.bit_depth);
			for table in &strike.index_sub_tables {
				for name in &table.names {
					let index = *dict
						.glyphs
						.get(name)
						.ok_or_else(|| Error::ttlib(format!("KeyError: glyph {name}")))?;
					let parsed = self.glyphs[index].parsed()?;
					let (width, height) = match parsed.size() {
						Some(size) => size,
						None => {
							let m = table.metrics.as_ref().ok_or_else(|| missing("metrics"))?;
							(usize::from(m.width), usize::from(m.height))
						}
					};
					exports.insert(
						index,
						Export {
							width,
							height,
							bit_depth,
						},
					);
				}
			}
		}
		Ok(exports)
	}
}

impl<const COLOR: bool, const APPLE: bool> Table for BitmapData<COLOR, APPLE> {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		let mut out = Vec::new();
		EbdtHeader {
			version: self.version,
		}
		.pack(&mut out)?;
		// Glyphs that have been written, by entry in `glyphs`.
		let mut placed: HashMap<usize, (u64, u64)> = HashMap::new();
		let locator = font.require::<BitmapLocator<COLOR, APPLE>>()?;
		for (strike, dict) in locator.strikes.iter_mut().zip(&self.strike_data) {
			let strike = strike.as_mut().ok_or_else(|| missing("indexSubTables"))?;
			for table in &mut strike.index_sub_tables {
				let mut locations = Vec::with_capacity(table.names.len());
				for name in &table.names {
					let dict = dict.as_ref().ok_or_else(|| missing("strikeData"))?;
					let index = *dict
						.glyphs
						.get(name)
						.ok_or_else(|| Error::ttlib(format!("KeyError: glyph {name}")))?;
					let location = match placed.get(&index) {
						Some(&location) => location,
						None => {
							let data = self.glyphs[index].parsed()?.compile()?;
							let data = table.pad_bitmap_data(data)?;
							let start = out.len() as u64;
							out.extend_from_slice(&data);
							let location = (start, out.len() as u64);
							placed.insert(index, location);
							location
						}
					};
					locations.push(location);
				}
				table.locations = locations;
			}
		}
		Ok(out)
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let mut used = BTreeSet::new();
		for dict in self.strike_data.iter_mut().flatten() {
			let mut glyphs = HashMap::with_capacity(dict.glyphs.len());
			for gid in &mut dict.order {
				let index = dict.glyphs[gid];
				used.insert(index);
				*gid = f(*gid)?;
				glyphs.insert(*gid, index);
			}
			dict.glyphs = glyphs;
		}
		for index in used {
			if let BitmapGlyph::Parsed(parsed) = &mut self.glyphs[index] {
				for c in parsed.components.iter_mut().flatten() {
					c.glyph = f(c.glyph)?;
				}
			}
		}
		Ok(())
	}
}

impl<const COLOR: bool, const APPLE: bool> TableXml for BitmapData<COLOR, APPLE> {
	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let mut exports = if matches!(font.bitmap_glyph_data_format.as_str(), "row" | "bitwise") {
			self.exports(font)?
		} else {
			font.require::<BitmapLocator<COLOR, APPLE>>()?;
			HashMap::new()
		};
		writer.simpletag_ordered("header", &[("version", &Value::Float(self.version))]);
		writer.newline();
		let order = font.glyph_order()?;
		for (strike_index, dict) in self.strike_data.iter().enumerate() {
			let dict = dict.as_ref().ok_or_else(|| missing("strikeData"))?;
			writer.begintag_ordered("strikedata", &[("index", &strike_index)]);
			writer.newline();
			for gid in dict.glyph_ids() {
				let index = dict.glyphs[gid];
				let parsed = self.glyphs[index].parsed()?;
				let tag = if is_cbdt_format(parsed.format) {
					format!("cbdt_bitmap_format_{}", parsed.format)
				} else {
					format!("ebdt_bitmap_format_{}", parsed.format)
				};
				let name = order.name(*gid);
				writer.begintag_ordered(&tag, &[("name", &name)]);
				writer.newline();
				parsed.metrics_to_xml(writer);
				if let Some(components) = &parsed.components {
					writer.begintag("components", &[]);
					writer.newline();
					// UPSTREAM-BUG(B21): `EbdtComponent.toXML` slices a dict, so a
					// composite with components cannot be dumped.
					if !components.is_empty() {
						return Err(component_format_error());
					}
					writer.endtag("components");
					writer.newline();
				} else {
					// The export values are deleted once they are used.
					let export = exports.remove(&index);
					self.write_image(writer, font, &parsed, export, strike_index, &name)?;
				}
				writer.endtag(&tag);
				writer.newline();
			}
			writer.endtag("strikedata");
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"header" => {
				self.version = eval(required_attr(element, "version")?)?
					.as_float()
					.ok_or_else(|| Error::value("invalid value for version"))?;
			}
			"strikedata" => {
				let strike_index: usize = int_attr(element, "index")?;
				let mut dict = StrikeData::default();
				for child in element.elements() {
					let name = child.name.as_str();
					if !name
						.get(4..)
						.is_some_and(|n| n.starts_with("_bitmap_format_"))
					{
						log::warn!("{name} being ignored by {}", Self::TAG);
						continue;
					}
					let format = eval(&name[19..])?
						.as_int()
						.ok_or_else(|| Error::value("invalid image format"))?;
					let format: u16 = fit(format, "imageFormat")?;
					check_format::<COLOR>(format)?;
					let glyph_name = required_attr(child, "name")?;
					let glyph = ParsedBitmapGlyph::from_xml(format, child)?;
					let gid = font.glyph_id(glyph_name)?;
					if dict.glyphs.contains_key(&gid) {
						return Err(Error::Assertion(format!(
							"Duplicate glyphs with the same name '{glyph_name}' in the same strike."
						)));
					}
					self.glyphs.push(BitmapGlyph::Parsed(glyph));
					dict.insert(gid, self.glyphs.len() - 1);
				}
				if strike_index >= self.strike_data.len() {
					self.strike_data.resize(strike_index + 1, None);
				}
				if self.strike_data[strike_index].is_some() {
					return Err(Error::Assertion("Duplicate strike EBDT indices.".into()));
				}
				self.strike_data[strike_index] = Some(dict);
			}
			_ => {}
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn formats_match_upstream() {
		use crate::sstruct::tests::check;
		check(EbdtHeader::FORMAT_NAME, EbdtHeader::FIELDS);
		check(
			EbdtComponentRecord::FORMAT_NAME,
			EbdtComponentRecord::FIELDS,
		);
	}

	/// `test_lazy_bitmap_metrics_and_edited_metrics_are_preserved`, without the
	/// edit.
	#[test]
	fn glyphs_compile_to_their_data() -> Result<()> {
		let small = [1, 8, 0xFF, 1, 9];
		let big = [1, 8, 0xFF, 1, 9, 0xFC, 2, 10];
		let component = [0, 1, 0, 1, 0xFE, 3];
		let prefixed = [0, 0, 0, 1, 0x81];
		let cases: [(u16, Vec<u8>); 8] = [
			(1, [&small[..], &[0x81]].concat()),
			(2, [&small[..], &[0x81]].concat()),
			(6, [&big[..], &[0x81]].concat()),
			(7, [&big[..], &[0x81]].concat()),
			(8, [&small[..], &[0], &component[..]].concat()),
			(9, [&big[..], &component[..]].concat()),
			(17, [&small[..], &prefixed[..]].concat()),
			(18, [&big[..], &prefixed[..]].concat()),
		];
		for (format, data) in cases {
			let glyph = ParsedBitmapGlyph::parse(format, &data)?;
			assert_eq!(glyph.compile()?, data, "format {format}");
		}
		Ok(())
	}

	/// UPSTREAM-BUG(B22): glyphs left as read keep their component IDs.
	#[test]
	fn remap_leaves_raw_components() -> Result<()> {
		let data = [&[1, 8, 0xFF, 1, 9, 0, 0, 1, 0, 5, 1, 2][..]].concat();
		let mut dict = StrikeData::default();
		dict.insert(6, 0);
		let mut table = Ebdt {
			version: 2.0,
			glyphs: vec![BitmapGlyph::Raw { format: 8, data }],
			strike_data: vec![Some(dict)],
		};
		table.remap_glyphs(&mut |g| Ok(g / 2))?;
		let ids = table.strike_data[0].as_ref().map(StrikeData::glyph_ids);
		assert_eq!(ids, Some(&[3][..]));
		let parsed = table.glyphs[0].parsed()?;
		let glyphs: Vec<u32> = parsed
			.components
			.iter()
			.flatten()
			.map(|c| c.glyph)
			.collect();
		assert_eq!(glyphs, [5]);
		Ok(())
	}

	/// UPSTREAM-BUG(B21).
	#[test]
	fn components_cannot_be_read_from_ttx() -> Result<()> {
		let element = |components: &str| {
			let text = format!(
				"<ebdt_bitmap_format_8 name=\"a\"><components>{components}</components></ebdt_bitmap_format_8>"
			);
			fontoxide_misc::xml_reader::parse(&text).map_err(|e| Error::ttx(e.to_string()))
		};
		assert!(ParsedBitmapGlyph::from_xml(8, &element("")?).is_ok());
		assert!(ParsedBitmapGlyph::from_xml(8, &element("<ebdtComponent name=\"b\"/>")?).is_err());
		Ok(())
	}

	/// `test_extfile_name_mapping_is_injective`.
	#[test]
	fn ext_file_names_are_injective() {
		let mut names: Vec<String> = [
			"a", "b", "uni0041", "a.alt", "%", "%25", "%2Fb", "/b", "a/b", "../b", "..", ".", "",
			"x/", "y/", "a\\b", "\u{e9}",
		]
		.map(str::to_owned)
		.to_vec();
		names.extend(["a/b", "", ".."].map(ext_file_name));
		let mapped: BTreeSet<String> = names.iter().map(|n| ext_file_name(n)).collect();
		assert_eq!(mapped.len(), names.iter().collect::<BTreeSet<_>>().len());
		assert!(
			mapped
				.iter()
				.all(|m| !m.contains(['/', '\\']) && !matches!(m.as_str(), "" | "." | ".."))
		);
		assert_eq!(ext_file_name("uni0041"), "uni0041");
		assert_eq!(ext_file_name("../b"), "%..%2Fb");
	}

	#[test]
	fn rows_round_trip() -> Result<()> {
		for format in [1, 2] {
			let mut glyph = ParsedBitmapGlyph {
				format,
				..Default::default()
			};
			let rows = vec![vec![0b1010_0000], vec![0b0111_0000]];
			glyph.set_rows(rows.clone(), 1, 4, false)?;
			for (i, row) in rows.iter().enumerate() {
				assert_eq!(&glyph.row(i, 1, 4, false)?, row, "format {format}");
			}
		}
		Ok(())
	}
}
