//! Port of `fontTools.ttLib.ttFont`: the [`Font`] object (`TTFont`).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use fontoxide_misc::Tag;
use fontoxide_misc::xml_writer::{INDENT, XmlWriter};

use crate::glyph_order::{GlyphOrder, make_glyph_name};
use crate::sfnt::{
	Flavor, FlavorData, SfntReader, SfntReaderOptions, SfntWriter, reorder_font_tables,
};
use crate::table::{Table, TableType, decompile_table, dependencies};
use crate::{Error, Result};

/// Options for [`Font::read`], corresponding to the `TTFont` constructor arguments.
#[derive(Debug, Clone, Copy)]
pub struct ReadOptions {
	/// The font in a collection to read (`fontNumber`). Required for collections.
	pub font_number: Option<usize>,
	/// How to treat wrong checksums (`checkChecksums`): 0 ignores them, 1 warns, 2
	/// fails.
	pub check_checksums: u8,
	/// Recalculate bounding boxes and related values when compiling
	/// (`recalcBBoxes`).
	pub recalc_bboxes: bool,
	/// Set `head.modified` to the current time when compiling (`recalcTimestamp`).
	pub recalc_timestamp: bool,
	/// Keep tables that fail to decompile as raw data instead of failing
	/// (`ignoreDecompileErrors`).
	pub ignore_decompile_errors: bool,
	/// Use the original offset table of a version 1.1/2.1 collection
	/// (`_useLegacyTTC`).
	pub use_legacy_ttc: bool,
	/// See [`Font::use_harfbuzz_repacker`].
	pub use_harfbuzz_repacker: Option<bool>,
	/// See [`Font::optimize_font_speed`].
	pub optimize_font_speed: bool,
}

impl Default for ReadOptions {
	fn default() -> Self {
		ReadOptions {
			font_number: None,
			check_checksums: 0,
			recalc_bboxes: true,
			recalc_timestamp: true,
			ignore_decompile_errors: false,
			use_legacy_ttc: false,
			use_harfbuzz_repacker: None,
			optimize_font_speed: false,
		}
	}
}

/// A progress callback, called with the fraction done, from 0.0 to 1.0. It need not
/// be `Send`, so that it can call into JavaScript on wasm.
pub type Progress = std::rc::Rc<dyn Fn(f64)>;

/// Options for [`Font::write`] (`TTFont.save`).
#[derive(Clone)]
pub struct WriteOptions {
	/// `Some(true)` (the default) sorts tables as the OpenType specification
	/// recommends; `Some(false)` keeps the original order; `None` writes them in
	/// dependency order (`reorderTables`).
	pub reorder_tables: Option<bool>,
	/// The Brotli quality of WOFF 2.0 output, from 0 to 11. fontTools always uses
	/// 11 (the default); lower values are much faster, but the file is larger.
	pub woff2_quality: u32,
	/// Called as the font data of WOFF 2.0 output is compressed, which takes nearly
	/// all of the time, with the fraction of the data read by Brotli so far. It is
	/// called thousands of times for a large font, so throttle in the callback if
	/// needed. Not called for other flavors.
	pub woff2_progress: Option<Progress>,
	/// Compress WOFF 1.0 with zopfli instead of zlib (fontTools'
	/// `sfnt.USE_ZOPFLI`, which `--with-zopfli` sets): smaller and much slower. The
	/// compressed bytes may differ from fontTools', the decompressed data does not.
	pub use_zopfli: bool,
}

impl Default for WriteOptions {
	fn default() -> Self {
		WriteOptions {
			reorder_tables: Some(true),
			woff2_quality: 11,
			woff2_progress: None,
			use_zopfli: false,
		}
	}
}

impl std::fmt::Debug for WriteOptions {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("WriteOptions")
			.field("reorder_tables", &self.reorder_tables)
			.field("woff2_quality", &self.woff2_quality)
			.field("use_zopfli", &self.use_zopfli)
			.field(
				"woff2_progress",
				&self.woff2_progress.as_ref().map(|_| ".."),
			)
			.finish()
	}
}

/// A table name in TTX: the `GlyphOrder` pseudo-table or a real table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TableKey {
	/// The `GlyphOrder` pseudo-table.
	GlyphOrder,
	/// A table.
	Table(Tag),
}

impl From<Tag> for TableKey {
	fn from(tag: Tag) -> Self {
		TableKey::Table(tag)
	}
}

/// Options for [`Font::to_ttx`] (`TTFont.saveXML`).
#[derive(Debug, Clone)]
pub struct TtxOptions {
	/// The tables to dump; empty dumps all (`tables`).
	pub tables: Vec<TableKey>,
	/// Tables to skip when dumping all (`skipTables`).
	pub skip_tables: Vec<TableKey>,
	/// Write the `ttLibVersion` attribute (`writeVersion`).
	pub write_version: bool,
	/// Line ending (`newlinestr`).
	pub newline: String,
	/// Disassemble TrueType instructions (`disassembleInstructions`).
	pub disassemble_instructions: bool,
	/// How to dump bitmap glyph data (`bitmapGlyphDataFormat`): `raw`, `row`,
	/// `bitwise` or `extfile`.
	pub bitmap_glyph_data_format: String,
	/// Write each table to a file of its own, and a main file that refers to
	/// them (`splitTables`). Only [`Font::to_ttx_files`] can do this.
	pub split_tables: bool,
	/// Also write each glyph of `glyf` to a file of its own (`splitGlyphs`);
	/// implies [`TtxOptions::split_tables`].
	pub split_glyphs: bool,
}

impl Default for TtxOptions {
	fn default() -> Self {
		TtxOptions {
			tables: Vec::new(),
			skip_tables: Vec::new(),
			write_version: true,
			newline: "\n".into(),
			disassemble_instructions: true,
			bitmap_glyph_data_format: "raw".into(),
			split_tables: false,
			split_glyphs: false,
		}
	}
}

/// Tables already written to a collection, for sharing identical ones
/// (`tableCache`).
pub(crate) type TableCache = HashMap<(Tag, Vec<u8>), crate::sfnt::DirectoryEntry>;

#[derive(Debug)]
enum Slot {
	Loaded(Box<dyn Table>),
	/// The table is detached while one of its methods runs.
	Busy,
}

/// An OpenType font (`TTFont`).
///
/// Tables are decompiled on first access and recompiled on [`Font::write`]; tables
/// that are never accessed are copied through unchanged, as in fontTools.
#[derive(Debug)]
pub struct Font {
	reader: Option<SfntReader>,
	tables: BTreeMap<Tag, Slot>,
	glyph_order: Option<Arc<GlyphOrder>>,
	/// The sfnt version: `\0\x01\0\0` for TrueType outlines, `OTTO` for CFF.
	pub sfnt_version: Tag,
	/// The web font flavor to write (`flavor`).
	pub flavor: Option<Flavor>,
	/// WOFF metadata and private data (`flavorData`).
	pub flavor_data: Option<FlavorData>,
	/// See [`ReadOptions::recalc_bboxes`].
	pub recalc_bboxes: bool,
	/// See [`ReadOptions::recalc_timestamp`].
	pub recalc_timestamp: bool,
	/// See [`ReadOptions::ignore_decompile_errors`].
	pub ignore_decompile_errors: bool,
	/// Set while dumping TTX (`disassembleInstructions`).
	pub disassemble_instructions: bool,
	/// Set while dumping TTX (`bitmapGlyphDataFormat`).
	pub bitmap_glyph_data_format: String,
	/// Whether `GSUB` and `GPOS` are packed with the HarfBuzz repacker
	/// (`fontTools.ttLib.tables.otBase:USE_HARFBUZZ_REPACKER`): `None` (the
	/// default) uses it for tables before version 1.2, `Some(true)` always,
	/// `Some(false)` never. fontTools does this when uharfbuzz is installed.
	pub use_harfbuzz_repacker: Option<bool>,
	/// How much `otlLib` compacts the class-based PairPos subtables it builds
	/// (`fontTools.otlLib.optimize.gpos:COMPRESSION_LEVEL`), 0 to 9: 0 (the
	/// default) does not compact them. See [`Font::set_gpos_compression_level`].
	gpos_compression_level: u8,
	/// Whether `otlLib` writes contextual positioning without context as GPOS
	/// lookup type 7 (`fontTools.otlLib.builder:WRITE_GPOS7`). The default,
	/// `false`, always writes type 8, for older macOS versions.
	pub write_gpos7: bool,
	/// Whether tables are compiled for rendering speed rather than size
	/// (`fontTools.ttLib:OPTIMIZE_FONT_SPEED`): `glyf` coordinates use the widest
	/// representation needed by any point of a glyph, and the deltas of `gvar` and
	/// `VARC` are not packed for size. The default, `false`, optimizes for size.
	/// Only tables that are decompiled are recompiled, as always.
	pub optimize_font_speed: bool,
	/// Set while [`Font::load_post_without_glyph_names`] loads `post`.
	pub(crate) skip_post_glyph_names: bool,
}

impl Clone for Font {
	fn clone(&self) -> Self {
		Font {
			reader: self.reader.clone(),
			tables: self
				.tables
				.iter()
				.filter_map(|(tag, slot)| match slot {
					Slot::Loaded(t) => Some((*tag, Slot::Loaded(t.clone_table()))),
					Slot::Busy => None,
				})
				.collect(),
			glyph_order: self.glyph_order.clone(),
			sfnt_version: self.sfnt_version,
			flavor: self.flavor,
			flavor_data: self.flavor_data.clone(),
			recalc_bboxes: self.recalc_bboxes,
			recalc_timestamp: self.recalc_timestamp,
			ignore_decompile_errors: self.ignore_decompile_errors,
			disassemble_instructions: self.disassemble_instructions,
			bitmap_glyph_data_format: self.bitmap_glyph_data_format.clone(),
			use_harfbuzz_repacker: self.use_harfbuzz_repacker,
			gpos_compression_level: self.gpos_compression_level,
			write_gpos7: self.write_gpos7,
			optimize_font_speed: self.optimize_font_speed,
			skip_post_glyph_names: self.skip_post_glyph_names,
		}
	}
}

impl Default for Font {
	fn default() -> Self {
		Self::new()
	}
}

const MAXP: Tag = Tag::new(b"maxp");
const MAXP_32: Tag = Tag::new(b"MAXP");
const HEAD: Tag = Tag::new(b"head");
const POST: Tag = Tag::new(b"post");
const CFF: Tag = Tag::new(b"CFF ");
const CMAP: Tag = Tag::new(b"cmap");
const DMAP: Tag = Tag::new(b"DMAP");
const FVAR: Tag = Tag::new(b"fvar");
const AVAR: Tag = Tag::new(b"avar");

/// `varLib.models.normalizeValue(v, triple)`, without extrapolation.
fn normalize_value(v: f64, (lower, default, upper): (f64, f64, f64)) -> Result<f64> {
	if !(lower <= default && default <= upper) {
		return Err(Error::value(format!(
			"Invalid axis values, must be minimum, default, maximum: {lower:.3}, {default:.3}, {upper:.3}"
		)));
	}
	let v = v.min(upper).max(lower);
	if v == default || lower == upper {
		return Ok(0.0);
	}
	if (v < default && lower != default) || (v > default && upper == default) {
		Ok((v - default) / (default - lower))
	} else {
		Ok((v - default) / (upper - default))
	}
}

/// `varLib.models.piecewiseLinearMap`, for a mapping in the order of a Python
/// `dict`.
pub(crate) fn piecewise_linear_map(v: f64, mapping: &[(f64, f64)]) -> f64 {
	if mapping.is_empty() {
		return v;
	}
	if let Some(&(_, to)) = mapping.iter().find(|(from, _)| *from == v) {
		return to;
	}
	let extreme = |pick: fn(f64, f64) -> f64| {
		mapping
			.iter()
			.copied()
			.reduce(|a, b| if pick(a.0, b.0) == a.0 { a } else { b })
	};
	if let Some((k, to)) = extreme(f64::min)
		&& v < k
	{
		return v + to - k;
	}
	if let Some((k, to)) = extreme(f64::max)
		&& v > k
	{
		return v + to - k;
	}
	let below = mapping
		.iter()
		.copied()
		.filter(|(k, _)| *k < v)
		.reduce(|a, b| if a.0 >= b.0 { a } else { b });
	let above = mapping
		.iter()
		.copied()
		.filter(|(k, _)| *k > v)
		.reduce(|a, b| if a.0 <= b.0 { a } else { b });
	match (below, above) {
		(Some((a, va)), Some((b, vb))) => va + (vb - va) * (v - a) / (b - a),
		_ => v,
	}
}

impl Font {
	/// An empty TrueType font (`TTFont()`).
	pub fn new() -> Self {
		Font {
			reader: None,
			tables: BTreeMap::new(),
			glyph_order: None,
			sfnt_version: Tag::new(b"\0\x01\0\0"),
			flavor: None,
			flavor_data: None,
			recalc_bboxes: true,
			recalc_timestamp: true,
			ignore_decompile_errors: false,
			disassemble_instructions: true,
			bitmap_glyph_data_format: "raw".into(),
			use_harfbuzz_repacker: None,
			gpos_compression_level: 0,
			write_gpos7: false,
			optimize_font_speed: false,
			skip_post_glyph_names: false,
		}
	}

	/// Loads `post` without decoding the names of a format 2 table, which becomes
	/// format 3. This is the hack of `fontTools.subset.load_font`, which avoids
	/// trouble with broken fonts when glyph names are not needed.
	pub fn load_post_without_glyph_names(&mut self) -> Result<()> {
		self.skip_post_glyph_names = true;
		let post = self.require::<crate::tables::Post>().map(|post| {
			if post.format_type == 2.0 {
				post.format_type = 3.0;
			}
		});
		self.skip_post_glyph_names = false;
		post
	}

	/// Maps the glyph IDs of every loaded table with `f`, then sets the glyph
	/// order to `new_order` (`setGlyphOrder`, where fontTools' tables follow by
	/// glyph name). Tables that were never loaded are written unchanged, as in
	/// fontTools.
	pub fn remap_glyphs(
		&mut self,
		f: &mut dyn FnMut(u32) -> Result<u32>,
		new_order: Vec<String>,
	) -> Result<()> {
		for slot in self.tables.values_mut() {
			if let Slot::Loaded(t) = slot {
				t.remap_glyphs(f)?;
			}
		}
		self.set_glyph_order(new_order);
		Ok(())
	}

	/// Reads a font from sfnt, TTC, WOFF or WOFF2 data (`TTFont(file)`).
	pub fn read(data: impl Into<Arc<[u8]>>, options: ReadOptions) -> Result<Self> {
		let reader = SfntReader::new(
			data,
			SfntReaderOptions {
				check_checksums: options.check_checksums,
				font_number: options.font_number,
				use_legacy_ttc: options.use_legacy_ttc,
			},
		)?;
		let mut font = Font::new();
		font.sfnt_version = reader.sfnt_version;
		font.flavor = reader.flavor;
		font.flavor_data = reader.flavor_data.clone();
		font.recalc_bboxes = options.recalc_bboxes;
		font.recalc_timestamp = options.recalc_timestamp;
		font.ignore_decompile_errors = options.ignore_decompile_errors;
		font.use_harfbuzz_repacker = options.use_harfbuzz_repacker;
		font.optimize_font_speed = options.optimize_font_speed;
		font.reader = Some(reader);
		Ok(font)
	}

	/// The reader for the original file, if the font was read from one.
	pub fn reader(&self) -> Option<&SfntReader> {
		self.reader.as_ref()
	}

	/// Whether the font has a table (`tag in font`).
	pub fn contains(&self, tag: Tag) -> bool {
		self.tables.contains_key(&tag) || self.reader.as_ref().is_some_and(|r| r.contains(tag))
	}

	/// Whether a table has been decompiled (`isLoaded`).
	pub fn is_loaded(&self, tag: Tag) -> bool {
		self.tables.contains_key(&tag)
	}

	/// The tables in the font, sorted as the OpenType specification recommends
	/// (`keys()`, without `GlyphOrder`).
	pub fn tags(&self) -> Vec<Tag> {
		let mut tags: Vec<Tag> = self.tables.keys().copied().collect();
		if let Some(r) = &self.reader {
			for tag in r.tags() {
				if !self.tables.contains_key(&tag) {
					tags.push(tag);
				}
			}
		}
		sorted_tag_list(&tags, None)
	}

	/// Decompiles a table if it is not loaded yet. Returns whether the font has it.
	pub fn load(&mut self, tag: Tag) -> Result<bool> {
		match self.tables.get(&tag) {
			Some(Slot::Loaded(_)) => return Ok(true),
			Some(Slot::Busy) => return Err(Error::TableBusy(tag)),
			None => {}
		}
		let Some(reader) = &self.reader else {
			return Ok(false);
		};
		if !reader.contains(tag) {
			return Ok(false);
		}
		let data = reader.table_data(tag)?;
		self.tables.insert(tag, Slot::Busy);
		let table = match decompile_table(tag, &data, self) {
			Ok(t) => t,
			Err(e) if self.ignore_decompile_errors => {
				log::error!(
					"An exception occurred during the decompilation of the '{tag}' table: {e}"
				);
				let mut t = crate::table::DefaultTable::new(tag, data.to_vec());
				t.error = Some(e.to_string());
				Box::new(t)
			}
			Err(e) => {
				self.tables.remove(&tag);
				return Err(Error::Decompile {
					tag,
					source: Box::new(e),
				});
			}
		};
		self.tables.insert(tag, Slot::Loaded(table));
		Ok(true)
	}

	/// The tag of a table type in this font: its beyond-64k companion if the font has
	/// that (`"GLYF" if "GLYF" in font else "glyf"`).
	pub fn family_tag<T: TableType>(&self) -> Tag {
		match T::COMPANION {
			Some(upper) if self.contains(upper) => upper,
			_ => T::TAG,
		}
	}

	/// Whether the font has a table of a type, under either of its tags.
	pub fn has<T: TableType>(&self) -> bool {
		self.contains(self.family_tag::<T>())
	}

	/// A table by type, decompiling it if necessary (`font[tag]`); `None` if the
	/// font does not have it. A table with a beyond-64k companion can be either.
	pub fn get<T: TableType>(&mut self) -> Result<Option<&mut T>> {
		let tag = self.family_tag::<T>();
		self.get_at::<T>(tag)
	}

	/// A table of a type under a tag.
	pub fn get_at<T: TableType>(&mut self, tag: Tag) -> Result<Option<&mut T>> {
		if !self.load(tag)? {
			return Ok(None);
		}
		match self.tables.get_mut(&tag) {
			Some(Slot::Loaded(t)) => (**t)
				.as_any_mut()
				.downcast_mut::<T>()
				.map(Some)
				.ok_or(Error::WrongTableType(tag)),
			Some(Slot::Busy) => Err(Error::TableBusy(tag)),
			None => Ok(None),
		}
	}

	/// The tag of a table type in the compact or the extended family, as the
	/// `locaTag` and such of a table of that family.
	pub fn locked_tag<T: TableType>(extended: bool) -> Tag {
		match T::COMPANION {
			Some(upper) if extended => upper,
			_ => T::TAG,
		}
	}

	/// A table of a type in the compact or the extended family.
	pub fn get_locked<T: TableType>(&mut self, extended: bool) -> Result<Option<&mut T>> {
		self.get_at::<T>(Self::locked_tag::<T>(extended))
	}

	/// [`Font::get_locked`], failing if the font does not have it.
	pub fn require_locked<T: TableType>(&mut self, extended: bool) -> Result<&mut T> {
		let tag = Self::locked_tag::<T>(extended);
		self.get_at::<T>(tag)?.ok_or(Error::TableNotFound(tag))
	}

	/// A table by type, failing if the font does not have it (`font[tag]`).
	pub fn require<T: TableType>(&mut self) -> Result<&mut T> {
		let tag = self.family_tag::<T>();
		self.get_at::<T>(tag)?.ok_or(Error::TableNotFound(tag))
	}

	/// An already decompiled table by type, without decompiling.
	pub fn loaded<T: TableType>(&self) -> Option<&T> {
		match self.tables.get(&self.family_tag::<T>()) {
			Some(Slot::Loaded(t)) => (**t).as_any().downcast_ref::<T>(),
			_ => None,
		}
	}

	/// A table by tag, decompiling it if necessary.
	pub fn get_dyn(&mut self, tag: Tag) -> Result<Option<&mut dyn Table>> {
		if !self.load(tag)? {
			return Ok(None);
		}
		match self.tables.get_mut(&tag) {
			Some(Slot::Loaded(t)) => Ok(Some(t.as_mut())),
			Some(Slot::Busy) => Err(Error::TableBusy(tag)),
			None => Ok(None),
		}
	}

	/// Adds or replaces a table (`font[tag] = table`).
	pub fn insert(&mut self, table: Box<dyn Table>) {
		self.tables.insert(table.tag(), Slot::Loaded(table));
	}

	/// Removes a table (`del font[tag]`). Returns whether the font had it.
	pub fn remove(&mut self, tag: Tag) -> bool {
		let loaded = self.tables.remove(&tag).is_some();
		let read = self.reader.as_mut().is_some_and(|r| r.remove(tag));
		loaded || read
	}

	/// Runs `f` with a loaded table detached from the font, so that `f` can also
	/// access the rest of the font.
	pub fn with_table<R>(
		&mut self,
		tag: Tag,
		f: impl FnOnce(&mut dyn Table, &mut Font) -> Result<R>,
	) -> Result<R> {
		if !self.load(tag)? {
			return Err(Error::TableNotFound(tag));
		}
		let mut table = match self.tables.insert(tag, Slot::Busy) {
			Some(Slot::Loaded(t)) => t,
			_ => return Err(Error::TableBusy(tag)),
		};
		let result = f(table.as_mut(), self);
		self.tables.insert(tag, Slot::Loaded(table));
		result
	}

	/// The binary data of a table: compiled if it is loaded, otherwise as read
	/// (`getTableData`).
	pub fn table_data(&mut self, tag: Tag) -> Result<Vec<u8>> {
		if self.is_loaded(tag) {
			return self
				.with_table(tag, |t, font| t.compile(font))
				.map_err(|e| match e {
					e @ (Error::Compile { .. } | Error::TableBusy(_)) => e,
					e => Error::Compile {
						tag,
						source: Box::new(e),
					},
				});
		}
		match &self.reader {
			Some(r) if r.contains(tag) => Ok(r.table_data(tag)?.to_vec()),
			_ => Err(Error::TableNotFound(tag)),
		}
	}

	/// Decompiles every table (`ensureDecompiled`).
	pub fn ensure_decompiled(&mut self) -> Result<()> {
		self.glyph_order()?;
		for tag in self.tags() {
			self.load(tag)?;
			if let Some(t) = self.get_dyn(tag)? {
				t.ensure_decompiled();
			}
		}
		Ok(())
	}

	/// The number of glyphs, from `maxp` (`getGlyphCount`).
	pub fn glyph_count(&mut self) -> Result<usize> {
		Ok(self.require::<crate::tables::maxp::Maxp>()?.num_glyphs as usize)
	}

	/// Whether the glyph order is set, or has been derived already.
	pub fn has_glyph_order(&self) -> bool {
		self.glyph_order.is_some()
	}

	/// How much `otlLib` compacts the class-based PairPos subtables it builds
	/// (`font.cfg["fontTools.otlLib.optimize.gpos:COMPRESSION_LEVEL"]`), 0 to 9:
	/// 0 (the default) does not compact them.
	pub fn gpos_compression_level(&self) -> u8 {
		self.gpos_compression_level
	}

	/// Sets [`Font::gpos_compression_level`]
	/// (`font.cfg["fontTools.otlLib.optimize.gpos:COMPRESSION_LEVEL"] = level`).
	/// A level above 9 is rejected, as the validation of the config option does
	/// (`ConfigValueValidationError`).
	pub fn set_gpos_compression_level(&mut self, level: u8) -> Result<()> {
		if level > 9 {
			return Err(Error::value(format!(
				"Config option fontTools.otlLib.optimize.gpos:COMPRESSION_LEVEL: value is invalid (given {level})"
			)));
		}
		self.gpos_compression_level = level;
		Ok(())
	}

	/// Sets the glyph order (`setGlyphOrder`).
	pub fn set_glyph_order(&mut self, names: Vec<String>) {
		self.glyph_order = Some(Arc::new(GlyphOrder::new(names)));
	}

	/// The glyph order, deriving it from `CFF `, `post` or `cmap` if it has not
	/// been set (`getGlyphOrder`).
	pub fn glyph_order(&mut self) -> Result<Arc<GlyphOrder>> {
		if let Some(order) = &self.glyph_order {
			return Ok(order.clone());
		}
		if self.contains(CFF) {
			let names = self.require::<crate::tables::Cff>()?.cff.glyph_order()?;
			self.set_glyph_order(names);
		} else if self.contains(POST) {
			let names = self
				.require::<crate::tables::post::Post>()?
				.take_glyph_order()?;
			match names {
				None => self.glyph_names_from_cmap()?,
				Some(names) if names.len() < self.glyph_count()? => {
					log::warn!(
						"Not enough names found in the 'post' table, generating them from cmap instead"
					);
					self.glyph_names_from_cmap()?;
				}
				Some(names) => self.set_glyph_order(names),
			}
		} else {
			self.glyph_names_from_cmap()?;
		}
		Ok(self.glyph_order.clone().unwrap_or_default())
	}

	/// The effective best Unicode character map of the font (`getBestCmap`): the
	/// mappings of `DMAP`, except to glyph zero, over those of `cmap`. `None` if
	/// neither has a Unicode subtable.
	pub fn get_best_cmap(
		&mut self,
		preferences: &[(u16, u16)],
	) -> Result<Option<crate::tables::cmap::CharMap>> {
		let cmap = match self.get::<crate::tables::Cmap>()? {
			Some(t) => t.get_best_cmap(preferences).cloned(),
			None => None,
		};
		let dmap = match self.get::<crate::tables::Dmap>()? {
			Some(t) => t.0.get_best_cmap(preferences).cloned(),
			None => None,
		};
		let Some(mut dmap) = dmap else {
			return Ok(cmap);
		};
		// Glyph zero is a miss and must allow cmap fallback.
		dmap.retain(|_, g| *g != 0);
		let Some(mut cmap) = cmap else {
			return Ok(Some(dmap));
		};
		cmap.extend(dmap);
		Ok(Some(cmap))
	}

	/// Normalizes a location in user coordinates (the axis ranges of `fvar`) to
	/// the normalized −1…+1 space, with the mapping of `avar` if the font has one
	/// (`normalizeLocation`). Every axis of `fvar` is in the result, except that a
	/// version 2 `avar` drops the zeroes. Fails with "Not a variable font" if the
	/// font has no `fvar`.
	pub fn normalize_location(
		&mut self,
		location: &BTreeMap<String, f64>,
	) -> Result<crate::var::Location> {
		if !self.contains(FVAR) {
			return Err(Error::ttlib("Not a variable font"));
		}
		let axes: Vec<(String, (f64, f64, f64))> = self
			.require::<crate::tables::Fvar>()?
			.axes
			.iter()
			.map(|a| (a.tag_text(), (a.min_value, a.default_value, a.max_value)))
			.collect();
		// `varLib.models.normalizeLocation`.
		let mut out = crate::var::Location::new();
		for (tag, triple) in &axes {
			let v = location.get(tag).copied().unwrap_or(triple.1);
			out.insert(tag.clone(), normalize_value(v, *triple)?);
		}
		if !self.contains(AVAR) {
			return Ok(out);
		}
		// `avar.renormalizeLocation(location, font)`.
		let avar = self.require::<crate::tables::Avar>()?;
		if !matches!(avar.major_version, 1 | 2) {
			return Err(Error::ttlib(
				"NotImplementedError: Unknown avar table version",
			));
		}
		for (tag, value) in &mut out {
			let mapping = Tag::from_str_padded(tag).and_then(|t| avar.segments.get(&t));
			if let Some(mapping) = mapping {
				*value = piecewise_linear_map(*value, mapping);
			}
		}
		if avar.major_version < 2 {
			return Ok(out);
		}
		let tags: Vec<String> = axes.into_iter().map(|(tag, _)| tag).collect();
		let store = avar.table.var_store.as_deref();
		let map = avar.table.var_idx_map.as_deref();
		let instancer = crate::var::VarStoreInstancer::new(store, &tags, out.clone());
		let mut renormalized = crate::var::Location::new();
		for (index, tag) in tags.iter().enumerate() {
			let mut v = fontoxide_misc::fixed_tools::float_to_fixed(
				out.get(tag).copied().unwrap_or(0.0),
				14,
			);
			let var_idx = match map {
				// A compiled map may be truncated: reads clamp to the last entry.
				Some(map) => map
					.mapping
					.get(index)
					.or(map.mapping.last())
					.copied()
					.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?,
				None => index as u64,
			};
			if store.is_some() {
				v += fontoxide_misc::round_tools::ot_round(instancer.get(var_idx)?);
				v = v.clamp(-(1 << 14), 1 << 14);
			}
			// `dropZeroes`.
			if v != 0 {
				renormalized.insert(
					tag.clone(),
					fontoxide_misc::fixed_tools::fixed_to_float(v, 14),
				);
			}
		}
		Ok(renormalized)
	}

	/// The name of a glyph (`getGlyphName`).
	pub fn glyph_name(&mut self, gid: u32) -> Result<String> {
		Ok(self.glyph_order()?.name(gid).into_owned())
	}

	/// The glyph ID for a name (`getGlyphID`).
	pub fn glyph_id(&mut self, name: &str) -> Result<u32> {
		self.glyph_order()?
			.id(name)
			.ok_or_else(|| Error::GlyphNotFound(name.to_owned()))
	}

	/// Names glyphs after the Unicode code points they are mapped to
	/// (`_getGlyphNamesFromCmap`).
	///
	/// As in fontTools, only the `cmap` and `DMAP` tables as stored in the file are
	/// used; tables that were added or modified in memory are ignored.
	fn glyph_names_from_cmap(&mut self) -> Result<()> {
		let num_glyphs = self.glyph_count()?;
		let mut names: Vec<String> = (0..num_glyphs)
			.map(|i| crate::glyph_order::synthesized_name(i as u32))
			.collect();
		if let Some(first) = names.first_mut() {
			*first = ".notdef".into();
		}
		// Temporary names, so that tables decompiled meanwhile see a glyph order.
		self.set_glyph_order(names.clone());

		let read =
			|font: &Font, tag: Tag| -> Result<Option<Vec<crate::tables::cmap::UnicodeMapping>>> {
				match &font.reader {
					Some(r) if r.contains(tag) => {
						let data = r.table_data(tag)?;
						Ok(Some(crate::tables::cmap::unicode_mappings(tag, &data)?))
					}
					_ => Ok(None),
				}
			};
		let cmap = read(self, CMAP)?;
		let dmap = read(self, DMAP)?;
		let dmap_codepoints: std::collections::HashSet<u32> = dmap
			.iter()
			.flatten()
			.flat_map(|m| m.cmap.keys().copied())
			.collect();
		// Glyph ID -> lowest code point mapped to it.
		let mut reverse: HashMap<u32, u32> = HashMap::new();
		for (is_cmap, tables) in [(true, &cmap), (false, &dmap)] {
			for table in tables.iter().flatten() {
				for (&codepoint, &gid) in &table.cmap {
					if is_cmap && dmap_codepoints.contains(&codepoint) {
						continue;
					}
					reverse
						.entry(gid)
						.and_modify(|c| *c = (*c).min(codepoint))
						.or_insert(codepoint);
				}
			}
		}
		let mut use_count: HashMap<String, usize> = HashMap::new();
		for (gid, name) in names.iter_mut().enumerate() {
			if let Some(&codepoint) = reverse.get(&(gid as u32)) {
				let mut glyph_name = make_glyph_name(codepoint);
				let n = use_count.entry(glyph_name.clone()).or_default();
				*n += 1;
				if *n > 1 {
					glyph_name = format!("{glyph_name}.alt{}", *n - 1);
				}
				*name = glyph_name;
			}
		}
		self.set_glyph_order(names);
		Ok(())
	}

	/// Compiles the font (`save`).
	pub fn write(&mut self, options: &WriteOptions) -> Result<Vec<u8>> {
		let (data, writer_reorders) = self.save_into(Vec::new(), None, Some(options))?;
		match options.reorder_tables {
			None => Ok(data),
			_ if writer_reorders => Ok(data),
			Some(false) => match &self.reader {
				None => Ok(data),
				Some(r) => {
					let order: Vec<Tag> = r.tags().collect();
					reorder_font_tables(data, Some(&order), options.use_zopfli)
				}
			},
			Some(true) => reorder_font_tables(data, None, options.use_zopfli),
		}
	}

	/// Writes the font's tables after the existing contents of `file` (`_save`),
	/// optionally sharing identical tables through `cache`. Returns the file and
	/// whether the writer already ordered the tables. `options` sets the WOFF 2.0
	/// compression.
	pub(crate) fn save_into(
		&mut self,
		file: Vec<u8>,
		mut cache: Option<&mut TableCache>,
		options: Option<&WriteOptions>,
	) -> Result<(Vec<u8>, bool)> {
		if self.recalc_timestamp && self.contains(HEAD) {
			self.load(HEAD)?;
		}
		let tags = self.tags();
		if self.flavor == Some(crate::sfnt::Flavor::Woff2) && !crate::woff2::HAVE_BROTLI {
			// `WOFF2Writer.__init__`.
			crate::woff2::log_no_brotli("encoder");
			return Err(crate::woff2::no_brotli());
		}
		let mut writer = SfntWriter::with_buffer(
			file,
			tags.len(),
			self.sfnt_version,
			self.flavor,
			self.flavor_data.clone(),
		);
		if let Some(options) = options {
			writer.set_woff2_compression(options.woff2_quality, options.woff2_progress.clone());
			writer.set_use_zopfli(options.use_zopfli);
		}
		let mut done = Vec::new();
		for &tag in &tags {
			self.write_table(tag, &mut writer, &mut done, cache.as_deref_mut())?;
		}
		let writer_reorders = writer.reorders_tables();
		Ok((writer.close()?, writer_reorders))
	}

	fn write_table(
		&mut self,
		tag: Tag,
		writer: &mut SfntWriter,
		done: &mut Vec<Tag>,
		mut cache: Option<&mut TableCache>,
	) -> Result<()> {
		if done.contains(&tag) {
			return Ok(());
		}
		for &master in dependencies(tag) {
			if !done.contains(&master) {
				if self.contains(master) {
					self.write_table(master, writer, done, cache.as_deref_mut())?;
				} else {
					done.push(master);
				}
			}
		}
		done.push(tag);
		let data = self.table_data(tag)?;
		if let Some(cache) = cache {
			if let Some(entry) = cache.get(&(tag, data.clone())) {
				log::debug!("reusing '{tag}' table");
				return writer.set_entry(entry.clone());
			}
			writer.add(tag, &data)?;
			if let Some(entry) = writer.entry(tag) {
				cache.insert((tag, data), entry.clone());
			}
			return Ok(());
		}
		writer.add(tag, &data)
	}

	/// Dumps the font as TTX (`saveXML` to a stream). Fails with
	/// [`TtxOptions::split_tables`] or [`TtxOptions::split_glyphs`], which need
	/// a file name: see [`Font::to_ttx_files`].
	pub fn to_ttx(&mut self, options: &TtxOptions) -> Result<String> {
		let mut writer = XmlWriter::with_options(INDENT, &options.newline);
		self.save_xml(&mut writer, options)?;
		Ok(writer.into_string())
	}

	/// Dumps the font as TTX to be saved as `path` (`saveXML(path)`): returns the
	/// file to save there, first, and, with [`TtxOptions::split_tables`] or
	/// [`TtxOptions::split_glyphs`], the table and glyph files to save next to
	/// it, whose names are derived from `path`. Nothing is written to the file
	/// system.
	pub fn to_ttx_files(
		&mut self,
		path: &str,
		options: &TtxOptions,
	) -> Result<Vec<crate::ttx::TtxFile>> {
		let mut writer = XmlWriter::with_options(INDENT, &options.newline);
		let mut files = Vec::new();
		self.save_xml_impl(&mut writer, options, Some(path), &mut files)?;
		files.insert(
			0,
			crate::ttx::TtxFile {
				path: path.to_owned(),
				contents: writer.into_string(),
			},
		);
		Ok(files)
	}

	pub(crate) fn save_xml(&mut self, writer: &mut XmlWriter, options: &TtxOptions) -> Result<()> {
		self.save_xml_impl(writer, options, None, &mut Vec::new())
	}

	/// `_saveXML`, where `path` is the file name of `writer` (`writer.filename`),
	/// and the files of the tables and glyphs of a split dump are added to
	/// `files`.
	fn save_xml_impl(
		&mut self,
		writer: &mut XmlWriter,
		options: &TtxOptions,
		path: Option<&str>,
		files: &mut Vec<crate::ttx::TtxFile>,
	) -> Result<()> {
		self.disassemble_instructions = options.disassemble_instructions;
		self.bitmap_glyph_data_format = options.bitmap_glyph_data_format.clone();
		let tables: Vec<TableKey> = if options.tables.is_empty() {
			std::iter::once(TableKey::GlyphOrder)
				.chain(self.tags().into_iter().map(TableKey::Table))
				.filter(|k| !options.skip_tables.contains(k))
				.collect()
		} else {
			options.tables.clone()
		};
		let sfnt_version = fontoxide_misc::py::str_repr(&self.sfnt_version.as_string());
		let sfnt_version = &sfnt_version[1..sfnt_version.len() - 1];
		if options.write_version {
			writer.begintag(
				"ttFont",
				&[
					("sfntVersion", &sfnt_version),
					("ttLibVersion", &TTLIB_VERSION),
				],
			);
		} else {
			writer.begintag("ttFont", &[("sfntVersion", &sfnt_version)]);
		}
		writer.newline();
		// Always split the tables if the glyphs are split.
		let split_tables = options.split_tables || options.split_glyphs;
		let split_path = if split_tables {
			let Some(path) = path else {
				return Err(Error::ttlib(
					"splitTables requires the file name to be a file system path, not a stream.",
				));
			};
			Some(crate::ttx::splitext(path))
		} else {
			writer.newline();
			None
		};
		for key in tables {
			let Some((stem, ext)) = split_path else {
				self.table_to_xml(writer, key, None, files)?;
				continue;
			};
			let identifier = match key {
				TableKey::GlyphOrder => "GlyphOrder".to_owned(),
				TableKey::Table(tag) => tag_to_identifier(tag),
			};
			let table_path = format!("{stem}.{identifier}{ext}");
			let mut table_writer = XmlWriter::with_options(INDENT, writer.newline_str());
			// UPSTREAM-BUG(B52): without `writeVersion`, fontTools fails here with
			// UnboundLocalError ('version' is only set when writing the version);
			// the version is written anyway.
			table_writer.begintag("ttFont", &[("ttLibVersion", &TTLIB_VERSION)]);
			table_writer.newline();
			table_writer.newline();
			let xml_tag = match key {
				TableKey::GlyphOrder => "GlyphOrder".to_owned(),
				TableKey::Table(tag) => tag_to_xml(tag),
			};
			writer.simpletag(&xml_tag, &[("src", &crate::ttx::basename(&table_path))]);
			writer.newline();
			let glyphs_path = options.split_glyphs.then_some(table_path.as_str());
			self.table_to_xml(&mut table_writer, key, glyphs_path, files)?;
			table_writer.endtag("ttFont");
			table_writer.newline();
			files.push(crate::ttx::TtxFile {
				path: table_path,
				contents: table_writer.into_string(),
			});
		}
		writer.endtag("ttFont");
		writer.newline();
		Ok(())
	}

	/// `_tableToXML`; `split_glyphs` is the file name of `writer` if the glyphs
	/// of `glyf` go to files of their own, which are added to `files`.
	fn table_to_xml(
		&mut self,
		writer: &mut XmlWriter,
		key: TableKey,
		split_glyphs: Option<&str>,
		files: &mut Vec<crate::ttx::TtxFile>,
	) -> Result<()> {
		let tag = match key {
			TableKey::GlyphOrder => {
				log::info!("Dumping 'GlyphOrder' table...");
				let order = self.glyph_order()?;
				writer.begintag("GlyphOrder", &[]);
				writer.newline();
				writer.comment("The 'id' attribute is only for humans; it is ignored when parsed.");
				writer.newline();
				for (i, name) in order.names().iter().enumerate() {
					writer.simpletag("GlyphID", &[("id", &i), ("name", name)]);
					writer.newline();
				}
				writer.endtag("GlyphOrder");
				writer.newline();
				writer.newline();
				return Ok(());
			}
			TableKey::Table(tag) => tag,
		};
		if !self.contains(tag) {
			log::info!("No '{tag}' table found.");
			return Ok(());
		}
		log::info!("Dumping '{tag}' table...");
		let xml_tag = tag_to_xml(tag);
		self.with_table(tag, |table, font| {
			let error = table.decompile_error().is_some();
			let raw = table.is_raw();
			match (error, raw) {
				(true, true) => writer.begintag(
					&xml_tag,
					&[("ERROR", &"decompilation error"), ("raw", &true)],
				),
				(true, false) => writer.begintag(&xml_tag, &[("ERROR", &"decompilation error")]),
				(false, true) => writer.begintag(&xml_tag, &[("raw", &true)]),
				(false, false) => writer.begintag(&xml_tag, &[]),
			}
			writer.newline();
			let glyf = table.as_any().downcast_ref::<crate::tables::Glyf>();
			match (split_glyphs, glyf) {
				(Some(path), Some(glyf)) => files.extend(glyf.to_xml_split(writer, font, path)?),
				// A raw table under the `glyf` tag ignores `splitGlyphs`.
				_ => crate::table::table_to_xml(table, writer, font)?,
			}
			writer.endtag(&xml_tag);
			writer.newline();
			writer.newline();
			Ok(())
		})
	}

	/// Reads a TTX document into a new font (`TTFont()` followed by `importXML`).
	pub fn from_ttx(text: &str) -> Result<Self> {
		let mut font = Font::new();
		font.import_ttx(text)?;
		Ok(font)
	}

	/// Merges a TTX document into the font (`importXML`).
	pub fn import_ttx(&mut self, text: &str) -> Result<()> {
		self.import_ttx_with(text, &crate::ttx::TtxImportOptions::default())
	}

	/// [`Font::import_ttx`] with options.
	///
	/// The files that a TTX document split into several files refers to (`src`
	/// attributes) are read from the current directory, as fontTools does for a
	/// stream without a file name.
	pub fn import_ttx_with(
		&mut self,
		text: &str,
		options: &crate::ttx::TtxImportOptions,
	) -> Result<()> {
		self.import_ttx_split(text, None, options, &mut crate::ttx::read_ttx_file)
	}

	/// Merges a TTX file into the font (`importXML(path)`), including the files
	/// it refers to if it was split into several files.
	pub fn import_ttx_file(&mut self, path: impl AsRef<std::path::Path>) -> Result<()> {
		let path = path.as_ref();
		let text = crate::ttx::read_ttx_file(path)?;
		self.import_ttx_split(
			&text,
			Some(path),
			&crate::ttx::TtxImportOptions::default(),
			&mut crate::ttx::read_ttx_file,
		)
	}

	/// Merges a TTX document into the font (`importXML`), where `path` is its
	/// file name, if any, and `load` reads the files it refers to (`src`
	/// attributes, relative to the directory of the file that refers to them, or
	/// to the current directory without a file name), as [`crate::ttx::read_ttx_file`]
	/// does from the file system.
	pub fn import_ttx_split(
		&mut self,
		text: &str,
		path: Option<&std::path::Path>,
		options: &crate::ttx::TtxImportOptions,
		load: &mut crate::ttx::TtxLoader<'_>,
	) -> Result<()> {
		if (self.contains(MAXP_32) || self.contains(MAXP)) && self.contains(POST) {
			// Make sure the glyph order is loaded, as it otherwise gets lost if the
			// XML doesn't contain the glyph order, yet does contain the table which
			// was originally used to extract the glyph names from.
			self.glyph_order()?;
		}
		let root =
			fontoxide_misc::xml_reader::parse(text).map_err(|e| Error::ttx(e.to_string()))?;
		let dir = path
			.and_then(std::path::Path::parent)
			.map(std::path::Path::to_path_buf)
			.unwrap_or_default();
		let mut files = crate::ttx::SubFiles { dir, load };
		crate::ttx::import(self, &root, options, &mut files)
	}
}

/// The `ttLibVersion` written to TTX: the major and minor fontTools version.
pub const TTLIB_VERSION: &str = {
	// "4.68.0" -> "4.68"
	let v = crate::FONTTOOLS_VERSION.as_bytes();
	let mut dots = 0;
	let mut i = 0;
	while i < v.len() {
		if v[i] == b'.' {
			dots += 1;
			if dots == 2 {
				break;
			}
		}
		i += 1;
	}
	match std::str::from_utf8(v.split_at(i).0) {
		Ok(s) => s,
		Err(_) => panic!("FONTTOOLS_VERSION is ASCII"),
	}
};

/// Table order recommended by the OpenType specification 1.4 for TrueType fonts
/// (`TTFTableOrder`).
pub const TTF_TABLE_ORDER: [Tag; 20] = [
	Tag::new(b"head"),
	Tag::new(b"hhea"),
	Tag::new(b"maxp"),
	Tag::new(b"OS/2"),
	Tag::new(b"hmtx"),
	Tag::new(b"LTSH"),
	Tag::new(b"VDMX"),
	Tag::new(b"hdmx"),
	Tag::new(b"DMAP"),
	Tag::new(b"cmap"),
	Tag::new(b"fpgm"),
	Tag::new(b"prep"),
	Tag::new(b"cvt "),
	Tag::new(b"loca"),
	Tag::new(b"glyf"),
	Tag::new(b"kern"),
	Tag::new(b"name"),
	Tag::new(b"post"),
	Tag::new(b"gasp"),
	Tag::new(b"PCLT"),
];

/// Table order recommended for CFF fonts (`OTFTableOrder`).
pub const OTF_TABLE_ORDER: [Tag; 9] = [
	Tag::new(b"head"),
	Tag::new(b"hhea"),
	Tag::new(b"maxp"),
	Tag::new(b"OS/2"),
	Tag::new(b"name"),
	Tag::new(b"DMAP"),
	Tag::new(b"cmap"),
	Tag::new(b"post"),
	Tag::new(b"CFF "),
];

/// Sorts tags as the OpenType specification recommends, or by a custom order
/// (`sortedTagList`).
pub fn sorted_tag_list(tags: &[Tag], table_order: Option<&[Tag]>) -> Vec<Tag> {
	let mut tags = tags.to_vec();
	tags.sort();
	let order: &[Tag] = match table_order {
		Some(order) => order,
		None => {
			let dsig = Tag::new(b"DSIG");
			if let Some(i) = tags.iter().position(|t| *t == dsig) {
				tags.remove(i);
				tags.push(dsig);
			}
			if tags.contains(&CFF) {
				&OTF_TABLE_ORDER
			} else {
				&TTF_TABLE_ORDER
			}
		}
	};
	let mut ordered = Vec::with_capacity(tags.len());
	for tag in order {
		if let Some(i) = tags.iter().position(|t| t == tag) {
			ordered.push(tags.remove(i));
		}
	}
	ordered.extend(tags);
	ordered
}

/// Converts a tag to a Python identifier that is also a filename unique on
/// case-insensitive file systems (`tagToIdentifier`): `glyf` → `_g_l_y_f`,
/// `OS/2` → `O_S_2f_2`.
pub fn tag_to_identifier(tag: Tag) -> String {
	let mut bytes: &[u8] = &tag.0;
	while bytes.len() > 1 && bytes.last() == Some(&b' ') {
		bytes = &bytes[..bytes.len() - 1];
	}
	let mut ident = String::new();
	for &c in bytes {
		if c.is_ascii_lowercase() || c.is_ascii_digit() {
			ident.push('_');
			ident.push(c as char);
		} else if c.is_ascii_uppercase() {
			ident.push(c as char);
			ident.push('_');
		} else {
			ident.push_str(&format!("{c:x}"));
		}
	}
	if ident.starts_with(|c: char| c.is_ascii_digit()) {
		ident.insert(0, '_');
	}
	ident
}

/// The inverse of [`tag_to_identifier`] (`identifierToTag`).
pub fn identifier_to_tag(ident: &str) -> Option<Tag> {
	let mut ident = ident.as_bytes();
	if ident.len() % 2 == 1 && ident.first() == Some(&b'_') {
		ident = &ident[1..];
	}
	if ident.len() % 2 == 1 {
		return None;
	}
	let mut tag = Vec::new();
	for pair in ident.chunks(2) {
		if pair[0] == b'_' {
			tag.push(pair[1]);
		} else if pair[1] == b'_' {
			tag.push(pair[0]);
		} else {
			tag.push(u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?);
		}
	}
	if tag.len() > 4 {
		return None;
	}
	tag.resize(4, b' ');
	Some(Tag(tag.try_into().ok()?))
}

/// Converts a tag to an XML element name (`tagToXML`).
pub fn tag_to_xml(tag: Tag) -> String {
	if tag == *b"OS/2" {
		return "OS_2".into();
	}
	let s = tag.as_string();
	// [A-Za-z_][A-Za-z_0-9]* *$
	let trimmed = s.trim_end_matches(' ');
	let valid = trimmed
		.bytes()
		.next()
		.is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
		&& trimmed
			.bytes()
			.all(|c| c.is_ascii_alphanumeric() || c == b'_');
	if valid {
		s.trim().to_owned()
	} else {
		tag_to_identifier(tag)
	}
}

/// Converts an XML element name back to a table key (`xmlToTag`).
pub fn xml_to_tag(name: &str) -> Option<TableKey> {
	if name == "GlyphOrder" {
		return Some(TableKey::GlyphOrder);
	}
	if name == "OS_2" {
		return Some(TableKey::Table(Tag::new(b"OS/2")));
	}
	if name.len() == 8 {
		return identifier_to_tag(name).map(TableKey::Table);
	}
	Tag::from_str_padded(name).map(TableKey::Table)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn tag_identifiers() {
		assert_eq!(tag_to_identifier(Tag::new(b"glyf")), "_g_l_y_f");
		assert_eq!(tag_to_identifier(Tag::new(b"cvt ")), "_c_v_t");
		assert_eq!(tag_to_identifier(Tag::new(b"OS/2")), "O_S_2f_2");
		assert_eq!(tag_to_identifier(Tag::new(b"CFF ")), "C_F_F_");
		assert_eq!(identifier_to_tag("O_S_2f_2"), Some(Tag::new(b"OS/2")));
		assert_eq!(identifier_to_tag("_c_v_t"), Some(Tag::new(b"cvt ")));
		assert_eq!(tag_to_xml(Tag::new(b"cvt ")), "cvt");
		assert_eq!(tag_to_xml(Tag::new(b"OS/2")), "OS_2");
		assert_eq!(tag_to_xml(Tag::new(b"CFF ")), "CFF");
		assert_eq!(xml_to_tag("cvt"), Some(TableKey::Table(Tag::new(b"cvt "))));
		assert_eq!(TTLIB_VERSION, "4.68");
	}

	#[test]
	fn table_order() {
		let tags: Vec<Tag> = [b"name", b"DSIG", b"cmap", b"head", b"GSUB"]
			.iter()
			.map(|t| Tag::new(t))
			.collect();
		let sorted = sorted_tag_list(&tags, None);
		let s: Vec<String> = sorted.iter().map(|t| t.to_string()).collect();
		assert_eq!(s, ["head", "cmap", "name", "GSUB", "DSIG"]);
	}
}
