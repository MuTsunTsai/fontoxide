//! `EBLC`, `CBLC` and `bloc`: bitmap locations (`fontTools.ttLib.tables.E_B_L_C_`,
//! `C_B_L_C_`, `_b_l_o_c`, `BitmapGlyphMetrics`).

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes, fit};
use crate::sstruct::{F16Dot16, I8, U8, U16, U32, Value, eval};
use crate::table::{Table, TableType, TableXml};
use crate::{Error, Font, Result};

crate::sstruct! {
	/// The header (`eblcHeaderFormat`).
	pub struct EblcHeader = "eblcHeaderFormat" {
		/// The table version.
		pub version: f64 = "version" as F16Dot16,
		/// The number of strikes.
		pub num_sizes: u32 = "numSizes" as U32,
	}
}

crate::sstruct! {
	/// The part of a bitmap size table before the line metrics
	/// (`bitmapSizeTableFormatPart1`).
	pub struct BitmapSizePart1 = "bitmapSizeTableFormatPart1" {
		/// Offset of the index subtable array.
		pub index_sub_table_array_offset: u32 = "indexSubTableArrayOffset" as U32,
		/// Size of the index subtable array and the index subtables.
		pub index_tables_size: u32 = "indexTablesSize" as U32,
		/// The number of index subtables.
		pub number_of_index_sub_tables: u32 = "numberOfIndexSubTables" as U32,
		/// Not used.
		pub color_ref: u32 = "colorRef" as U32,
	}
}

crate::sstruct! {
	/// The line metrics of a strike (`sbitLineMetricsFormat`).
	pub struct SbitLineMetrics = "sbitLineMetricsFormat" {
		/// Ascender.
		pub ascender: i8 = "ascender" as I8,
		/// Descender.
		pub descender: i8 = "descender" as I8,
		/// Maximum width.
		pub width_max: u8 = "widthMax" as U8,
		/// Caret slope numerator.
		pub caret_slope_numerator: i8 = "caretSlopeNumerator" as I8,
		/// Caret slope denominator.
		pub caret_slope_denominator: i8 = "caretSlopeDenominator" as I8,
		/// Caret offset.
		pub caret_offset: i8 = "caretOffset" as I8,
		/// Minimum origin side bearing.
		pub min_origin_sb: i8 = "minOriginSB" as I8,
		/// Minimum advance side bearing.
		pub min_advance_sb: i8 = "minAdvanceSB" as I8,
		/// Maximum extent before the baseline.
		pub max_before_bl: i8 = "maxBeforeBL" as I8,
		/// Minimum extent after the baseline.
		pub min_after_bl: i8 = "minAfterBL" as I8,
		/// Padding.
		pub pad1: i8 = "pad1" as I8,
		/// Padding.
		pub pad2: i8 = "pad2" as I8,
	}
}

crate::sstruct! {
	/// The part of a bitmap size table after the line metrics
	/// (`bitmapSizeTableFormatPart2`).
	pub struct BitmapSizePart2 = "bitmapSizeTableFormatPart2" {
		/// The lowest glyph ID of the strike.
		pub start_glyph_index: u16 = "startGlyphIndex" as U16,
		/// The highest glyph ID of the strike.
		pub end_glyph_index: u16 = "endGlyphIndex" as U16,
		/// Horizontal pixels per em.
		pub ppem_x: u8 = "ppemX" as U8,
		/// Vertical pixels per em.
		pub ppem_y: u8 = "ppemY" as U8,
		/// Bits per pixel.
		pub bit_depth: u8 = "bitDepth" as U8,
		/// Flags.
		pub flags: i8 = "flags" as I8,
	}
}

crate::sstruct! {
	/// Metrics for horizontal and vertical layout (`bigGlyphMetricsFormat`).
	pub struct BigGlyphMetrics = "bigGlyphMetricsFormat" {
		/// Height in pixels.
		pub height: u8 = "height" as U8,
		/// Width in pixels.
		pub width: u8 = "width" as U8,
		/// Horizontal left side bearing.
		pub hori_bearing_x: i8 = "horiBearingX" as I8,
		/// Horizontal top side bearing.
		pub hori_bearing_y: i8 = "horiBearingY" as I8,
		/// Horizontal advance.
		pub hori_advance: u8 = "horiAdvance" as U8,
		/// Vertical left side bearing.
		pub vert_bearing_x: i8 = "vertBearingX" as I8,
		/// Vertical top side bearing.
		pub vert_bearing_y: i8 = "vertBearingY" as I8,
		/// Vertical advance.
		pub vert_advance: u8 = "vertAdvance" as U8,
	}
}

crate::sstruct! {
	/// Metrics for one layout direction (`smallGlyphMetricsFormat`).
	pub struct SmallGlyphMetrics = "smallGlyphMetricsFormat" {
		/// Height in pixels.
		pub height: u8 = "height" as U8,
		/// Width in pixels.
		pub width: u8 = "width" as U8,
		/// Left side bearing.
		pub bearing_x: i8 = "BearingX" as I8,
		/// Top side bearing.
		pub bearing_y: i8 = "BearingY" as I8,
		/// Advance.
		pub advance: u8 = "Advance" as U8,
	}
}

/// `BitmapGlyphMetrics.toXML`, for the [`sstruct!`](crate::sstruct) values of
/// metrics.
pub(crate) fn metrics_to_xml(writer: &mut XmlWriter, tag: &str, values: Vec<(&str, Value)>) {
	writer.begintag(tag, &[]);
	writer.newline();
	for (name, value) in values {
		writer.simpletag(name, &[("value", &value)]);
		writer.newline();
	}
	writer.endtag(tag);
	writer.newline();
}

/// The `value` attribute of a TTX element, evaluated.
pub(crate) fn literal(element: &Element) -> Result<fontoxide_misc::text_tools::Literal> {
	eval(required_attr(element, "value")?)
}

/// An attribute that fontTools reads with `attrs[name]`.
pub(crate) fn required_attr<'a>(element: &'a Element, name: &str) -> Result<&'a str> {
	element
		.attr(name)
		.ok_or_else(|| Error::ttx(format!("missing '{name}' on <{}>", element.name)))
}

/// An integer attribute that fontTools reads with `safeEval(attrs[name])`.
pub(crate) fn int_attr<T: TryFrom<i128>>(element: &Element, name: &str) -> Result<T> {
	let value = eval(required_attr(element, name)?)?
		.as_int()
		.ok_or_else(|| Error::value(format!("invalid value for {name}")))?;
	fit(value, name)
}

/// The size of an entry of `indexSubTableArray` and of an index subtable header.
const INDEX_SUB_HEADER_SIZE: usize = 8;

/// A slice with Python's clipping of out-of-range bounds.
pub(crate) fn clip(data: &[u8], start: u64, end: u64) -> &[u8] {
	let len = data.len() as u64;
	let end = end.min(len);
	let start = start.min(end);
	&data[start as usize..end as usize]
}

/// An index subtable (`EblcIndexSubTable` and its subclasses).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IndexSubTable {
	/// The format, 1 to 5 (`indexFormat`).
	pub index_format: u16,
	/// The format of the bitmap data of the glyphs (`imageFormat`).
	pub image_format: u16,
	/// Offset of the bitmap data (`imageDataOffset`).
	pub image_data_offset: u32,
	/// The lowest glyph ID (`firstGlyphIndex`), recalculated when compiling.
	pub first_glyph_index: u32,
	/// The highest glyph ID (`lastGlyphIndex`), recalculated when compiling.
	pub last_glyph_index: u32,
	/// Offset from the index subtable array (`additionalOffsetToIndexSubtable`).
	pub additional_offset: u32,
	/// The size of each glyph's data, in formats 2 and 5 (`imageSize`).
	pub image_size: Option<u32>,
	/// The metrics of all glyphs, in formats 2 and 5 (`metrics`).
	pub metrics: Option<BigGlyphMetrics>,
	/// The glyphs, by ID (`names`, which are glyph names in fontTools).
	pub names: Vec<u32>,
	/// The start and end of each glyph's data in the bitmap data table
	/// (`locations`), set again when that table is compiled.
	pub locations: Vec<(u64, u64)>,
}

fn is_fixed_size(index_format: u16) -> bool {
	matches!(index_format, 2 | 5)
}

fn offset_size(index_format: u16) -> Option<usize> {
	match index_format {
		1 => Some(4),
		3 => Some(2),
		_ => None,
	}
}

impl IndexSubTable {
	fn decompile(&mut self, data: &[u8]) -> Result<()> {
		match self.index_format {
			1 | 3 => self.decompile_offsets(data),
			2 => self.decompile_format2(data),
			4 => self.decompile_format4(data),
			5 => self.decompile_format5(data),
			f => Err(Error::ttlib(format!("KeyError: {f}"))),
		}
	}

	/// `OffsetArrayIndexSubTableMixin.decompile`.
	fn decompile_offsets(&mut self, data: &[u8]) -> Result<()> {
		let num_glyphs = i64::from(self.last_glyph_index) - i64::from(self.first_glyph_index) + 1;
		let count = (num_glyphs + 1).max(0) as usize;
		let mut r = Reader::new(data);
		let offsets: Vec<u64> = if self.index_format == 1 {
			r.u32_array(count)?.into_iter().map(u64::from).collect()
		} else {
			r.u16_array(count)?.into_iter().map(u64::from).collect()
		};
		let base = u64::from(self.image_data_offset);
		// `removeSkipGlyphs`.
		let mut names = Vec::new();
		let mut locations = Vec::new();
		for (gid, pair) in (self.first_glyph_index..=self.last_glyph_index).zip(offsets.windows(2))
		{
			let (start, end) = (pair[0] + base, pair[1] + base);
			if start < end {
				names.push(gid);
				locations.push((start, end));
			}
		}
		if names.is_empty() {
			return Err(Error::value(
				"not enough values to unpack (expected 2, got 0)",
			));
		}
		self.names = names;
		self.locations = locations;
		Ok(())
	}

	/// The locations of `count` glyphs of the fixed size.
	fn fixed_locations(&self, count: usize) -> Vec<(u64, u64)> {
		let size = u64::from(self.image_size.unwrap_or_default());
		let base = u64::from(self.image_data_offset);
		(0..count as u64)
			.map(|i| (size * i + base, size * (i + 1) + base))
			.collect()
	}

	fn decompile_format2(&mut self, data: &[u8]) -> Result<()> {
		let mut r = Reader::new(data);
		self.image_size = Some(r.u32()?);
		let (metrics, _) = BigGlyphMetrics::unpack2(r.rest())?;
		self.metrics = Some(metrics);
		self.names = (self.first_glyph_index..=self.last_glyph_index).collect();
		self.locations = self.fixed_locations(self.names.len());
		Ok(())
	}

	fn decompile_format4(&mut self, data: &[u8]) -> Result<()> {
		let mut r = Reader::new(data);
		let num_glyphs = r.u32()? as usize;
		let pairs = r.u16_array((num_glyphs + 1).saturating_mul(2))?;
		let base = u64::from(self.image_data_offset);
		let pairs = pairs.as_chunks::<2>().0;
		let offsets: Vec<u64> = pairs.iter().map(|p| u64::from(p[1]) + base).collect();
		self.locations = offsets.windows(2).map(|p| (p[0], p[1])).collect();
		self.names = pairs
			.iter()
			.take(num_glyphs)
			.map(|p| u32::from(p[0]))
			.collect();
		Ok(())
	}

	fn decompile_format5(&mut self, data: &[u8]) -> Result<()> {
		let mut r = Reader::new(data);
		self.image_size = Some(r.u32()?);
		self.metrics = Some(BigGlyphMetrics::unpack(&mut r)?);
		let num_glyphs = r.u32()? as usize;
		self.names = r
			.u16_array(num_glyphs)?
			.into_iter()
			.map(u32::from)
			.collect();
		self.locations = self.fixed_locations(self.names.len());
		Ok(())
	}

	fn compile(&mut self) -> Result<Vec<u8>> {
		match self.index_format {
			1 | 3 => self.compile_offsets(),
			2 => self.compile_format2(),
			4 => self.compile_format4(),
			5 => self.compile_format5(),
			f => Err(Error::ttlib(format!("KeyError: {f}"))),
		}
	}

	/// `EblcIndexSubTable.compile`: the header.
	fn compile_header(&self, out: &mut Vec<u8>) {
		out.put_u16(self.index_format);
		out.put_u16(self.image_format);
		out.put_u32(self.image_data_offset);
	}

	fn check_consecutive(&self) -> Result<()> {
		if self.locations.windows(2).any(|l| l[0].1 != l[1].0) {
			return Err(Error::Assertion(
				"Data must be consecutive in indexSubTable".into(),
			));
		}
		Ok(())
	}

	/// `min(next(iter(zip(*self.locations))))`.
	fn min_start(&self) -> Result<u32> {
		let min = self
			.locations
			.iter()
			.map(|l| l.0)
			.min()
			.ok_or_else(|| Error::ttlib("StopIteration"))?;
		fit(min, "imageDataOffset")
	}

	/// `OffsetArrayIndexSubTableMixin.compile`.
	fn compile_offsets(&mut self) -> Result<Vec<u8>> {
		self.check_consecutive()?;
		if self.names.windows(2).any(|g| g[0] >= g[1]) {
			return Err(Error::Assertion("glyph IDs must increase".into()));
		}
		let index_error = || Error::ttlib("IndexError: deque index out of range");
		// Add skip glyphs where an ID is not present.
		let mut all = Vec::new();
		let (mut next_name, mut next_location) = (0, 0);
		for gid in self.first_glyph_index..=self.last_glyph_index {
			let location = *self.locations.get(next_location).ok_or_else(index_error)?;
			if gid == *self.names.get(next_name).ok_or_else(index_error)? {
				next_name += 1;
				next_location += 1;
				all.push(location);
			} else {
				all.push((location.0, location.0));
			}
		}
		let first = all.first().ok_or_else(index_error)?;
		let mut offsets = vec![first.0, first.1];
		offsets.extend(all[1..].iter().map(|l| l.1));
		let min = offsets.iter().copied().min().unwrap_or_default();
		self.image_data_offset = fit(min, "imageDataOffset")?;

		let mut out = Vec::new();
		self.compile_header(&mut out);
		let size = offset_size(self.index_format).unwrap_or(4);
		for offset in &offsets {
			let value = offset - min;
			if size == 4 {
				out.put_u32(fit(value, "offset")?);
			} else {
				out.put_u16(fit(value, "offset")?);
			}
		}
		if !(size * offsets.len()).is_multiple_of(4) {
			out.put_u16(0);
		}
		Ok(out)
	}

	fn compile_metrics(&self, out: &mut Vec<u8>) -> Result<()> {
		out.put_u32(self.image_size.ok_or_else(|| missing("imageSize"))?);
		self.metrics
			.as_ref()
			.ok_or_else(|| missing("metrics"))?
			.pack(out)
	}

	fn compile_format2(&mut self) -> Result<Vec<u8>> {
		let expected = (self.first_glyph_index..=self.last_glyph_index).collect::<Vec<_>>();
		if self.names != expected {
			return Err(Error::Assertion("Format 2 ids must be consecutive.".into()));
		}
		self.image_data_offset = self.min_start()?;
		let mut out = Vec::new();
		self.compile_header(&mut out);
		self.compile_metrics(&mut out)?;
		Ok(out)
	}

	fn compile_format4(&mut self) -> Result<Vec<u8>> {
		self.check_consecutive()?;
		let first = self
			.locations
			.first()
			.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
		let mut offsets = vec![first.0, first.1];
		offsets.extend(self.locations[1..].iter().map(|l| l.1));
		let min = offsets.iter().copied().min().unwrap_or_default();
		self.image_data_offset = fit(min, "imageDataOffset")?;

		let mut out = Vec::new();
		self.compile_header(&mut out);
		out.put_u32(fit(self.names.len(), "numGlyphs")?);
		let ids = self.names.iter().copied().chain([0]);
		for (gid, offset) in ids.zip(&offsets) {
			out.put_u16(fit(gid, "glyphCode")?);
			out.put_u16(fit(offset - min, "offset")?);
		}
		Ok(out)
	}

	fn compile_format5(&mut self) -> Result<Vec<u8>> {
		self.image_data_offset = self.min_start()?;
		let mut out = Vec::new();
		self.compile_header(&mut out);
		self.compile_metrics(&mut out)?;
		out.put_u32(fit(self.names.len(), "numGlyphs")?);
		for &gid in &self.names {
			out.put_u16(fit(gid, "glyphCode")?);
		}
		if self.names.len() % 2 == 1 {
			out.put_u16(0);
		}
		Ok(out)
	}

	/// `padBitmapData`: fixed-size formats pad the data to the fixed size.
	pub(crate) fn pad_bitmap_data(&self, mut data: Vec<u8>) -> Result<Vec<u8>> {
		if is_fixed_size(self.index_format) {
			let size = self.image_size.ok_or_else(|| missing("imageSize"))? as usize;
			if data.len() > size {
				return Err(Error::Assertion(format!(
					"Data in indexSubTable format {} must be less than the fixed size.",
					self.index_format
				)));
			}
			data.resize(size, 0);
		}
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let tag = format!("eblc_index_sub_table_{}", self.index_format);
		writer.begintag_ordered(
			&tag,
			&[
				("imageFormat", &self.image_format),
				("firstGlyphIndex", &self.first_glyph_index),
				("lastGlyphIndex", &self.last_glyph_index),
			],
		);
		writer.newline();
		if is_fixed_size(self.index_format) {
			let size = self.image_size.ok_or_else(|| missing("imageSize"))?;
			writer.simpletag("imageSize", &[("value", &size)]);
			writer.newline();
			let metrics = self.metrics.as_ref().ok_or_else(|| missing("metrics"))?;
			metrics_to_xml(writer, "BigGlyphMetrics", metrics.values());
		}
		let order = font.glyph_order()?;
		for &gid in &self.names {
			writer.simpletag("glyphLoc", &[("name", &order.name(gid)), ("id", &gid)]);
			writer.newline();
		}
		writer.endtag(&tag);
		writer.newline();
		Ok(())
	}

	fn from_xml(index_format: u16, element: &Element, font: &mut Font) -> Result<Self> {
		if !matches!(index_format, 1..=5) {
			return Err(Error::ttlib(format!("KeyError: {index_format}")));
		}
		let mut t = IndexSubTable {
			index_format,
			image_format: int_attr(element, "imageFormat")?,
			first_glyph_index: int_attr(element, "firstGlyphIndex")?,
			last_glyph_index: int_attr(element, "lastGlyphIndex")?,
			..Default::default()
		};
		for child in element.elements() {
			match child.name.as_str() {
				"imageSize" if is_fixed_size(index_format) => {
					t.image_size = Some(int_attr(child, "value")?);
				}
				"BigGlyphMetrics" if is_fixed_size(index_format) => {
					let mut metrics = BigGlyphMetrics::default();
					for m in child.elements() {
						if !metrics.set_literal(&m.name, &literal(m)?)? {
							log::warn!(
								"unknown name '{}' being ignored in BigGlyphMetrics.",
								m.name
							);
						}
					}
					t.metrics = Some(metrics);
				}
				"SmallGlyphMetrics" if is_fixed_size(index_format) => {
					log::warn!("SmallGlyphMetrics being ignored in format {index_format}.");
				}
				"glyphLoc" => t.names.push(font.glyph_id(required_attr(child, "name")?)?),
				_ => {}
			}
		}
		Ok(t)
	}
}

fn missing(attribute: &str) -> Error {
	Error::ttlib(format!("AttributeError: {attribute}"))
}

/// A bitmap size table (`BitmapSizeTable`).
///
/// LENIENT(L22): values that a TTX document leaves out are zero.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BitmapSizeTable {
	/// The part before the line metrics.
	pub part1: BitmapSizePart1,
	/// Horizontal line metrics (`hori`).
	pub hori: SbitLineMetrics,
	/// Vertical line metrics (`vert`).
	pub vert: SbitLineMetrics,
	/// The part after the line metrics.
	pub part2: BitmapSizePart2,
}

impl BitmapSizeTable {
	fn to_xml(&self, writer: &mut XmlWriter) {
		writer.begintag("bitmapSizeTable", &[]);
		writer.newline();
		for (name, metrics) in [("hori", &self.hori), ("vert", &self.vert)] {
			writer.begintag_ordered("sbitLineMetrics", &[("direction", &name)]);
			writer.newline();
			for (field, value) in metrics.values() {
				writer.simpletag(field, &[("value", &value)]);
				writer.newline();
			}
			writer.endtag("sbitLineMetrics");
			writer.newline();
		}
		// The offsets and counts are not written.
		let part1 = self.part1.values().into_iter().skip(3);
		for (field, value) in part1.chain(self.part2.values()) {
			writer.simpletag(field, &[("value", &value)]);
			writer.newline();
		}
		writer.endtag("bitmapSizeTable");
		writer.newline();
	}

	fn read_xml(&mut self, element: &Element) -> Result<()> {
		for child in element.elements() {
			if child.name == "sbitLineMetrics" {
				let direction = required_attr(child, "direction")?;
				let mut metrics = SbitLineMetrics::default();
				for m in child.elements() {
					if SbitLineMetrics::FIELDS.iter().any(|f| f.name == m.name) {
						metrics.set_literal(&m.name, &literal(m)?)?;
					}
				}
				match direction {
					"hori" => self.hori = metrics,
					"vert" => self.vert = metrics,
					_ => {
						return Err(Error::Assertion(
							"SbitLineMetrics direction specified invalid.".into(),
						));
					}
				}
			} else if child.name == "colorRef" {
				self.part1.color_ref = int_attr(child, "value")?;
			} else if BitmapSizePart2::FIELDS.iter().any(|f| f.name == child.name) {
				self.part2.set_literal(&child.name, &literal(child)?)?;
			} else {
				log::warn!(
					"unknown name '{}' being ignored in BitmapSizeTable.",
					child.name
				);
			}
		}
		Ok(())
	}
}

/// A strike: bitmaps for one size (`Strike`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Strike {
	/// The size and line metrics (`bitmapSizeTable`).
	pub bitmap_size_table: BitmapSizeTable,
	/// The index subtables (`indexSubTables`).
	pub index_sub_tables: Vec<IndexSubTable>,
}

/// Embedded (`COLOR` false) or Color (`COLOR` true) Bitmap Location table
/// (`table_E_B_L_C_`, `table_C_B_L_C_`), or with `APPLE`, Apple's `bloc`
/// (`table__b_l_o_c`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/eblc> and
/// <https://learn.microsoft.com/en-us/typography/opentype/spec/cblc>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BitmapLocator<const COLOR: bool, const APPLE: bool = false> {
	/// The table version.
	pub version: f64,
	/// The strikes. Only a TTX document with gaps in its strike indices leaves
	/// `None`s, which fail when the table is compiled or dumped.
	pub strikes: Vec<Option<Strike>>,
}

/// `EBLC`.
pub type Eblc = BitmapLocator<false>;
/// `CBLC`.
pub type Cblc = BitmapLocator<true>;
/// `bloc`: Apple's `EBLC` (`table__b_l_o_c`), with `bdat`.
pub type Bloc = BitmapLocator<false, true>;

impl<const COLOR: bool, const APPLE: bool> TableType for BitmapLocator<COLOR, APPLE> {
	const TAG: Tag = if COLOR {
		Tag::new(b"CBLC")
	} else if APPLE {
		Tag::new(b"bloc")
	} else {
		Tag::new(b"EBLC")
	};

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let (header, _) = EblcHeader::unpack2(clip(data, 0, 8))?;
		let mut pos = 8;
		let mut strikes = Vec::new();
		for _ in 0..header.num_sizes {
			let rest = clip(data, pos, u64::MAX);
			let (part1, rest) = BitmapSizePart1::unpack2(rest)?;
			let (hori, rest) = SbitLineMetrics::unpack2(rest)?;
			let (vert, rest) = SbitLineMetrics::unpack2(rest)?;
			let (part2, _) = BitmapSizePart2::unpack2(rest)?;
			pos += 48;
			strikes.push(Strike {
				bitmap_size_table: BitmapSizeTable {
					part1,
					hori,
					vert,
					part2,
				},
				index_sub_tables: Vec::new(),
			});
		}
		for strike in &mut strikes {
			let part1 = &strike.bitmap_size_table.part1;
			let array_offset = u64::from(part1.index_sub_table_array_offset);
			for i in 0..u64::from(part1.number_of_index_sub_tables) {
				let entry = array_offset + i * INDEX_SUB_HEADER_SIZE as u64;
				let mut r = Reader::new(clip(data, entry, entry + 8));
				if r.remaining() != INDEX_SUB_HEADER_SIZE {
					return Err(Error::Struct(
						"unpack requires a buffer of 8 bytes".to_owned(),
					));
				}
				let first_glyph_index = u32::from(r.u16()?);
				let last_glyph_index = u32::from(r.u16()?);
				let additional_offset = r.u32()?;

				let start = array_offset + u64::from(additional_offset);
				let mut r = Reader::new(clip(data, start, start + 8));
				if r.remaining() != INDEX_SUB_HEADER_SIZE {
					return Err(Error::Struct(
						"unpack requires a buffer of 8 bytes".to_owned(),
					));
				}
				let mut table = IndexSubTable {
					index_format: r.u16()?,
					image_format: r.u16()?,
					image_data_offset: r.u32()?,
					first_glyph_index,
					last_glyph_index,
					additional_offset,
					..Default::default()
				};
				table.decompile(clip(data, start + 8, u64::MAX))?;
				strike.index_sub_tables.push(table);
			}
		}
		Ok(BitmapLocator {
			version: header.version,
			strikes: strikes.into_iter().map(Some).collect(),
		})
	}
}

impl<const COLOR: bool, const APPLE: bool> BitmapLocator<COLOR, APPLE> {
	fn compile_table(&mut self) -> Result<Vec<u8>> {
		let mut sub_table_data = Vec::new();
		let mut data_size = 8 + self.strikes.len() * 48;
		for strike in &mut self.strikes {
			let strike = strike.as_mut().ok_or_else(|| missing("bitmapSizeTable"))?;
			let count = strike.index_sub_tables.len();
			let array_offset = data_size;
			data_size += count * INDEX_SUB_HEADER_SIZE;

			let mut sub_tables = Vec::with_capacity(count);
			for table in &mut strike.index_sub_tables {
				table.additional_offset =
					fit(data_size - array_offset, "additionalOffsetToIndexSubtable")?;
				let empty = || Error::value("min() arg is an empty sequence");
				table.first_glyph_index = *table.names.iter().min().ok_or_else(empty)?;
				table.last_glyph_index = *table.names.iter().max().ok_or_else(empty)?;
				let data = table.compile()?;
				data_size += data.len();
				sub_tables.push(data);
			}
			let tables = &strike.index_sub_tables;
			let empty = || Error::value("min() arg is an empty sequence");
			let first = tables
				.iter()
				.map(|t| t.first_glyph_index)
				.min()
				.ok_or_else(empty)?;
			let last = tables
				.iter()
				.map(|t| t.last_glyph_index)
				.max()
				.ok_or_else(empty)?;
			let size_table = &mut strike.bitmap_size_table;
			size_table.part1.number_of_index_sub_tables = fit(count, "numberOfIndexSubTables")?;
			size_table.part1.index_sub_table_array_offset =
				fit(array_offset, "indexSubTableArrayOffset")?;
			size_table.part2.start_glyph_index = fit(first, "startGlyphIndex")?;
			size_table.part2.end_glyph_index = fit(last, "endGlyphIndex")?;
			size_table.part1.index_tables_size = fit(data_size - array_offset, "indexTablesSize")?;

			for table in tables {
				sub_table_data.put_u16(fit(table.first_glyph_index, "firstGlyphIndex")?);
				sub_table_data.put_u16(fit(table.last_glyph_index, "lastGlyphIndex")?);
				sub_table_data.put_u32(table.additional_offset);
			}
			sub_table_data.extend(sub_tables.into_iter().flatten());
		}

		let mut out = Vec::with_capacity(data_size);
		EblcHeader {
			version: self.version,
			num_sizes: fit(self.strikes.len(), "numSizes")?,
		}
		.pack(&mut out)?;
		for strike in self.strikes.iter().flatten() {
			let size_table = &strike.bitmap_size_table;
			size_table.part1.pack(&mut out)?;
			size_table.hori.pack(&mut out)?;
			size_table.vert.pack(&mut out)?;
			size_table.part2.pack(&mut out)?;
		}
		out.extend(sub_table_data);
		Ok(out)
	}
}

impl<const COLOR: bool, const APPLE: bool> Table for BitmapLocator<COLOR, APPLE> {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		self.compile_table()
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	/// The first and last glyph are recalculated when compiling.
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		for strike in self.strikes.iter_mut().flatten() {
			for table in &mut strike.index_sub_tables {
				for name in &mut table.names {
					*name = f(*name)?;
				}
			}
		}
		Ok(())
	}
}

impl<const COLOR: bool, const APPLE: bool> TableXml for BitmapLocator<COLOR, APPLE> {
	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		writer.simpletag_ordered("header", &[("version", &Value::Float(self.version))]);
		writer.newline();
		for (index, strike) in self.strikes.iter().enumerate() {
			let strike = strike.as_ref().ok_or_else(|| missing("toXML"))?;
			writer.begintag_ordered("strike", &[("index", &index)]);
			writer.newline();
			strike.bitmap_size_table.to_xml(writer);
			writer.comment(
				"GlyphIds are written but not read. The firstGlyphIndex and\nlastGlyphIndex values will be recalculated by the compiler.",
			);
			writer.newline();
			for table in &strike.index_sub_tables {
				table.to_xml(writer, font)?;
			}
			writer.endtag("strike");
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
			"strike" => {
				let index: usize = int_attr(element, "index")?;
				let mut strike = Strike::default();
				for child in element.elements() {
					if child.name == "bitmapSizeTable" {
						strike.bitmap_size_table.read_xml(child)?;
					} else if let Some(format) = child.name.strip_prefix("eblc_index_sub_table_") {
						let format = eval(format)?
							.as_int()
							.ok_or_else(|| Error::value("invalid index format"))?;
						strike.index_sub_tables.push(IndexSubTable::from_xml(
							fit(format, "indexFormat")?,
							child,
							font,
						)?);
					}
				}
				if index >= self.strikes.len() {
					self.strikes.resize(index + 1, None);
				}
				if self.strikes[index].is_some() {
					return Err(Error::Assertion("Duplicate strike EBLC indices.".into()));
				}
				self.strikes[index] = Some(strike);
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
		check(EblcHeader::FORMAT_NAME, EblcHeader::FIELDS);
		check(BitmapSizePart1::FORMAT_NAME, BitmapSizePart1::FIELDS);
		check(SbitLineMetrics::FORMAT_NAME, SbitLineMetrics::FIELDS);
		check(BitmapSizePart2::FORMAT_NAME, BitmapSizePart2::FIELDS);
		check(BigGlyphMetrics::FORMAT_NAME, BigGlyphMetrics::FIELDS);
		check(SmallGlyphMetrics::FORMAT_NAME, SmallGlyphMetrics::FIELDS);
	}
}
