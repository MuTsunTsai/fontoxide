//! Port of `fontTools.ttLib.ttCollection`: TrueType / OpenType collections.

use std::sync::Arc;

use fontoxide_misc::xml_writer::{INDENT, XmlWriter};

use crate::binary::WriteBytes;
use crate::font::{TTLIB_VERSION, TableCache};
use crate::sfnt::{TTC_V1, TTC_V1_1, TTC_V2, TTC_V2_1, read_ttc_header};
use crate::{Font, ReadOptions, Result, TtxOptions};

/// A TrueType or OpenType collection (`TTCollection`).
#[derive(Debug, Clone, Default)]
pub struct TtCollection {
	/// The fonts (`fonts`). For version 1.1 and 2.1 collections, the preferred,
	/// extended list.
	pub fonts: Vec<Font>,
	/// For version 1.1 and 2.1 collections, the compatibility list
	/// (`legacyFonts`). fontoxide does not derive it.
	pub legacy_fonts: Option<Vec<Font>>,
	/// The collection's digital signature (`dsig`). `Some(None)` writes a version
	/// 2 header without a signature; `None` writes version 1.
	pub dsig: Option<Option<Vec<u8>>>,
}

impl TtCollection {
	/// Reads every font of a collection (`TTCollection(file)`). The font number in
	/// `options` is ignored.
	pub fn read(data: impl Into<Arc<[u8]>>, options: ReadOptions) -> Result<Self> {
		let data: Arc<[u8]> = data.into();
		let header = read_ttc_header(&data)?;
		let read = |i: usize, use_legacy_ttc: bool| {
			Font::read(
				data.clone(),
				ReadOptions {
					font_number: Some(i),
					use_legacy_ttc,
					..options
				},
			)
		};
		let mut collection = TtCollection::default();
		let num_fonts = header
			.offset_table2
			.as_ref()
			.map_or(header.num_fonts as usize, Vec::len);
		for i in 0..num_fonts {
			collection.fonts.push(read(i, false)?);
		}
		if header.offset_table2.is_some() {
			let mut legacy = Vec::new();
			for i in 0..header.num_fonts as usize {
				legacy.push(read(i, true)?);
			}
			collection.legacy_fonts = Some(legacy);
		}
		if let Some((_tag, length, offset)) = header.dsig {
			collection.dsig = Some(None);
			if offset != 0 {
				let start = offset as usize;
				let end = (start + length as usize).min(data.len());
				collection.dsig = Some(Some(data.get(start..end).unwrap_or_default().to_vec()));
			}
		}
		Ok(collection)
	}

	/// Compiles the collection (`save`). With `share_tables`, identical tables are
	/// stored once.
	pub fn write(&mut self, share_tables: bool) -> Result<Vec<u8>> {
		let mut cache = TableCache::new();
		let has_legacy = self.legacy_fonts.is_some();
		let version = match (self.dsig.is_some(), has_legacy) {
			(true, true) => TTC_V2_1,
			(true, false) => TTC_V2,
			(false, true) => TTC_V1_1,
			(false, false) => TTC_V1,
		};

		// Pin one 'modified' timestamp so that the fonts' 'head' tables can be
		// shared (`_sharedModifiedTimestamp`).
		let now = fontoxide_misc::time_tools::timestamp_now();
		let mut restore = Vec::new();
		for (i, font) in self
			.legacy_fonts
			.iter_mut()
			.flatten()
			.chain(self.fonts.iter_mut())
			.enumerate()
		{
			if font.recalc_timestamp && font.contains(crate::Tag::new(b"head")) {
				restore.push(i);
				let now =
					now.ok_or_else(|| crate::Error::value(crate::tables::head::NO_TIMESTAMP))?;
				font.require::<crate::tables::Head>()?.modified =
					crate::binary::fit(now, "modified")?;
				font.recalc_timestamp = false;
			}
		}
		let result = self.write_fonts(version, share_tables.then_some(&mut cache));
		for (i, font) in self
			.legacy_fonts
			.iter_mut()
			.flatten()
			.chain(self.fonts.iter_mut())
			.enumerate()
		{
			if restore.contains(&i) {
				font.recalc_timestamp = true;
			}
		}
		result
	}

	fn write_fonts(&mut self, version: u32, mut cache: Option<&mut TableCache>) -> Result<Vec<u8>> {
		let has_legacy = self.legacy_fonts.is_some();
		let num_fonts = self
			.legacy_fonts
			.as_ref()
			.map_or(self.fonts.len(), Vec::len);
		// writeTTCHeader
		let mut file = Vec::new();
		file.extend_from_slice(b"ttcf");
		file.put_u32(version);
		file.put_u32(num_fonts as u32);
		let offsets_offset = file.len();
		file.resize(file.len() + 4 * num_fonts, 0);
		if matches!(version, TTC_V2 | TTC_V2_1) {
			file.resize(file.len() + 12, 0);
		}
		let mut offsets2_offset = None;
		if matches!(version, TTC_V1_1 | TTC_V2_1) {
			file.put_u32(self.fonts.len() as u32);
			offsets2_offset = Some(file.len());
			file.resize(file.len() + 4 * self.fonts.len(), 0);
		}

		let mut legacy_offsets = Vec::new();
		for font in self.legacy_fonts.iter_mut().flatten() {
			legacy_offsets.push(file.len() as u32);
			file = font.save_into(file, cache.as_deref_mut(), None)?.0;
		}
		let mut offsets = Vec::new();
		for font in &mut self.fonts {
			offsets.push(file.len() as u32);
			file = font.save_into(file, cache.as_deref_mut(), None)?.0;
		}

		let write_u32s = |file: &mut Vec<u8>, at: usize, values: &[u32]| {
			for (i, v) in values.iter().enumerate() {
				file[at + 4 * i..at + 4 * i + 4].copy_from_slice(&v.to_be_bytes());
			}
		};
		if has_legacy {
			write_u32s(&mut file, offsets_offset, &legacy_offsets);
			if let Some(at) = offsets2_offset {
				write_u32s(&mut file, at, &offsets);
			}
		} else {
			write_u32s(&mut file, offsets_offset, &offsets);
		}
		if let Some(Some(dsig)) = &self.dsig {
			let header_at = offsets_offset
				+ 4 * if has_legacy {
					legacy_offsets.len()
				} else {
					offsets.len()
				};
			let dsig_offset = file.len() as u32;
			file.extend_from_slice(dsig);
			file[header_at..header_at + 4].copy_from_slice(b"DSIG");
			write_u32s(&mut file, header_at + 4, &[dsig.len() as u32, dsig_offset]);
		}
		Ok(file)
	}

	/// Dumps the collection as TTX (`saveXML`). `options.tables` and the other
	/// options apply to every font; `write_version` controls the
	/// `ttCollection` element.
	pub fn to_ttx(&mut self, options: &TtxOptions) -> Result<String> {
		let mut writer = XmlWriter::with_options(INDENT, &options.newline);
		if options.write_version {
			writer.begintag("ttCollection", &[("ttLibVersion", &TTLIB_VERSION)]);
		} else {
			writer.begintag("ttCollection", &[]);
		}
		writer.newline();
		writer.newline();
		let font_options = TtxOptions {
			write_version: false,
			..options.clone()
		};
		for font in &mut self.fonts {
			font.save_xml(&mut writer, &font_options)?;
			writer.newline();
		}
		writer.endtag("ttCollection");
		writer.newline();
		Ok(writer.into_string())
	}
}
