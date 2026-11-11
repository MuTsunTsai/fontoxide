//! Port of `fontTools.merge`: merges several fonts into one.
//!
//! ```no_run
//! use fontoxide_merge::{Merger, Options};
//! use fontoxide_ttlib::WriteOptions;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut merger = Merger::new(Options::default());
//! let mut font = merger.merge_files(&["Latin.ttf", "Arabic.ttf"])?;
//! std::fs::write("merged.ttf", font.write(&WriteOptions::default())?)?;
//! # Ok(())
//! # }
//! ```
//!
//! fontTools refers to glyphs by name, and renames the glyphs whose names are
//! taken by an earlier font. Here each font is first moved to the glyph IDs of
//! its glyphs in the merged font (its glyph IDs plus the number of glyphs of the
//! fonts before it), so that its tables can be merged as they are.

#![forbid(unsafe_code)]

mod cli;
pub mod cmap;
mod layout;
pub mod options;
mod tables;
pub mod unicode;

use std::collections::BTreeSet;
use std::sync::Arc;

use fontoxide_ttlib::beyond64k::{ConvertOptions, upper_tables, with_compact_layout_tables};
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::table::AsAny;
use fontoxide_ttlib::tables::cmap::UvsDict;
use fontoxide_ttlib::tables::hhea::{Hhea, Vhea};
use fontoxide_ttlib::tables::{
	Base, Cff, Cmap, Cvt, Fpgm, Gasp, Gdef, Glyf, Gpos, Gsub, Head, Hmtx, Jstf, Loca, Math, Maxp,
	Name, Os2, Post, Prep, Vmtx,
};
use fontoxide_ttlib::{Font, ReadOptions, Table, Tag};

pub use cli::{USAGE, main};
pub use cmap::{DuplicateGlyphs, compute_mega_glyph_order, flatten_dmap, rename_cff_char_strings};
pub use options::{Item, Options};
pub use tables::merge_os2_fs_type;
pub use unicode::is_default_ignorable;

/// An error while merging.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
	/// An error of the font library.
	#[error(transparent)]
	Ttlib(#[from] fontoxide_ttlib::Error),
	/// An error reading or writing a file.
	#[error(transparent)]
	Io(#[from] std::io::Error),
	/// `Options.UnknownOptionError`.
	#[error("UnknownOptionError: {0}")]
	UnknownOption(String),
	/// `ValueError`, as for fonts with different units per em.
	#[error("{0}")]
	Value(String),
	/// `AssertionError`: values that fontTools requires to be equal are not.
	#[error("AssertionError: {0}")]
	Assertion(String),
	/// `NotImplementedError`, as for CID-keyed CFF fonts.
	#[error("NotImplementedError: {0}")]
	NotImplemented(String),
	/// Data that fontTools fails on with another exception (named in the
	/// message), such as a table that only some of the fonts have, or a table
	/// that `mergeObjects` does not know how to merge.
	#[error("{0}")]
	Type(String),
}

/// Shorthand for `Result<T, fontoxide_merge::Error>`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The beyond-64k tables (`_UPPER_TABLES`).
const UPPER_TABLES: [&[u8; 4]; 8] = [
	b"GLYF", b"GVAR", b"HHEA", b"HMTX", b"LOCA", b"MAXP", b"VHEA", b"VMTX",
];

/// The tables with a beyond-64k companion (`_COMPANION_TABLES`).
const COMPANION_TABLES: [&str; 8] = [
	"glyf", "gvar", "hhea", "hmtx", "loca", "maxp", "vhea", "vmtx",
];

/// The tables that refer to glyphs and that are merged; they are loaded before
/// the fonts move to the glyph IDs of the merged font.
const GLYPH_TABLES: [&[u8; 4]; 13] = [
	b"cmap", b"GDEF", b"GSUB", b"GPOS", b"glyf", b"GLYF", b"hmtx", b"HMTX", b"vmtx", b"VMTX",
	b"BASE", b"JSTF", b"MATH",
];

/// The tables that have merge logic. Others are dropped.
const MERGEABLE: [&[u8; 4]; 30] = [
	b"maxp", b"MAXP", b"head", b"hhea", b"HHEA", b"vhea", b"VHEA", b"OS/2", b"post", b"hmtx",
	b"HMTX", b"vmtx", b"VMTX", b"name", b"loca", b"LOCA", b"glyf", b"GLYF", b"prep", b"fpgm",
	b"cvt ", b"gasp", b"CFF ", b"cmap", b"GDEF", b"GSUB", b"GPOS", b"BASE", b"JSTF", b"MATH",
];

/// `_fontHasUpperTables`.
fn font_has_upper_tables(font: &mut Font) -> Result<bool> {
	if UPPER_TABLES.iter().any(|t| font.contains(Tag::new(t))) {
		return Ok(true);
	}
	if let Some(t) = font.get::<Gdef>()?
		&& t.version >= 0x0001_0004
	{
		return Ok(true);
	}
	if let Some(t) = font.get::<Gsub>()?
		&& t.version >= 0x0001_0002
	{
		return Ok(true);
	}
	if let Some(t) = font.get::<Gpos>()?
		&& t.version >= 0x0001_0002
	{
		return Ok(true);
	}
	Ok(false)
}

/// The font merger (`Merger`).
///
/// This merges multiple fonts into a single OpenType font, taking into account
/// complexities such as OpenType layout (`GSUB`/`GPOS`) tables and cross-font
/// metrics (for example `hhea.ascent` is set to the maximum value across all
/// the fonts).
///
/// If multiple glyphs map to the same Unicode value, and the glyphs are
/// considered sufficiently different (that is, they differ in any of paths,
/// widths, or height), then subsequent glyphs are renamed and a lookup in the
/// `locl` feature will be created to disambiguate them. For example, if the
/// arguments are an Arabic font and a Latin font and both contain a set of
/// parentheses, the Latin glyphs will be renamed to `parenleft.1` and
/// `parenright.1`, and a lookup will be inserted into the `locl` feature
/// (creating it if necessary) under the `latn` script to substitute
/// `parenleft` with `parenleft.1` etc.
///
/// Restrictions:
///
/// - All fonts must have the same units per em.
/// - If duplicate glyph disambiguation takes place as described above then the
///   fonts must have a `GSUB` table.
#[derive(Debug, Clone, Default)]
pub struct Merger {
	/// The options (`options`).
	pub options: Options,
	/// The glyph order of the merged font (`glyphOrder`), once computed.
	pub glyph_order: Option<Vec<String>>,
	/// Whether the merged font needs the beyond-64k tables (`beyond64k`).
	pub beyond64k: bool,
	/// The Unicode mapping of the merged font, to its glyph IDs (`cmap`), once
	/// computed.
	pub cmap: Option<fontoxide_ttlib::tables::cmap::CharMap>,
	/// The variation sequences of the merged font (`uvsDict`).
	pub uvs_dict: UvsDict,
	/// For each font, the glyphs to disambiguate in `locl`: a glyph of an
	/// earlier font to the font's glyph for the same code point
	/// (`duplicateGlyphsPerFont`). Only set while merging.
	pub duplicate_glyphs_per_font: Vec<DuplicateGlyphs>,
	/// The `name` ID 4 of each font, for messages (`_merger__name`).
	font_names: Vec<Option<String>>,
	/// What `layoutPreMerge` found in the `GDEF` tables.
	mark_sets: layout::MarkSets,
}

impl Merger {
	/// A merger with the given options (`Merger(options)`).
	pub fn new(options: Options) -> Self {
		Merger {
			options,
			..Default::default()
		}
	}

	/// `_openFonts`.
	fn open_fonts(&mut self, fontfiles: &[Arc<[u8]>]) -> Result<Vec<Font>> {
		let mut fonts = Vec::with_capacity(fontfiles.len());
		self.font_names.clear();
		for data in fontfiles {
			let mut font = Font::read(data.clone(), ReadOptions::default())?;
			self.font_names
				.push(font.require::<Name>()?.get_debug_name(4));
			fonts.push(font);
		}
		Ok(fonts)
	}

	/// Merges the font files at `paths` (`merge`).
	pub fn merge_files<P: AsRef<std::path::Path>>(&mut self, paths: &[P]) -> Result<Font> {
		let fontfiles = paths
			.iter()
			.map(std::fs::read)
			.collect::<std::io::Result<Vec<_>>>()?;
		self.merge(&fontfiles)
	}

	/// Merges fonts, given as the data of font files (`merge`). Returns the
	/// merged font; call its `write` method to compile it.
	///
	/// `DMAP` overrides are folded into the merged `cmap` table.
	pub fn merge<D: AsRef<[u8]>>(&mut self, fontfiles: &[D]) -> Result<Font> {
		let fontfiles: Vec<Arc<[u8]>> = fontfiles.iter().map(|d| Arc::from(d.as_ref())).collect();

		// Settle on a mega glyph order.
		let mut fonts = self.open_fonts(&fontfiles)?;
		let mut glyph_orders = Vec::with_capacity(fonts.len());
		for font in &mut fonts {
			glyph_orders.push(font.glyph_order()?.names().to_vec());
		}
		let glyph_order = compute_mega_glyph_order(&mut glyph_orders);
		// maxp.numGlyphs is uint16: needs upper tables above 0xFFFF, not 0x10000.
		self.beyond64k = glyph_order.len() > 0xFFFF;
		for font in &mut fonts {
			if self.beyond64k {
				break;
			}
			self.beyond64k = font_has_upper_tables(font)?;
		}
		self.glyph_order = Some(glyph_order.clone());

		// Take first input file sfntVersion.
		let sfnt_version = fonts
			.first()
			.ok_or_else(|| Error::Type("IndexError: list index out of range".into()))?
			.sfnt_version;

		// Reload fonts and set new glyph names on them.
		let mut fonts = self.open_fonts(&fontfiles)?;
		for (font, order) in fonts.iter_mut().zip(&glyph_orders) {
			font.set_glyph_order(order.clone());
			if font.contains(Tag::new(b"CFF ")) {
				rename_cff_char_strings(order, font.require::<Cff>()?)?;
			}
			if self.beyond64k {
				upper_tables(
					font,
					&ConvertOptions {
						tables: Some(&COMPANION_TABLES),
						..Default::default()
					},
				)?;
			}
			if font.contains(Tag::new(b"DMAP")) && !self.options.drops("DMAP") {
				flatten_dmap(font)?;
			}
		}

		// Here the fonts move to the glyph IDs of the merged font.
		let mut offset = 0u32;
		for (font, order) in fonts.iter_mut().zip(&glyph_orders) {
			self.load_glyph_tables(font)?;
			let count =
				u32::try_from(order.len()).map_err(|_| Error::Type("too many glyphs".into()))?;
			for glyf in [Tag::new(b"glyf"), Tag::new(b"GLYF")] {
				if font.is_loaded(glyf)
					&& let Some(t) = font.get_dyn(glyf)?
					&& let Some(t) = t.as_any_mut().downcast_mut::<Glyf>()
				{
					// fontTools names them `ttxautoglyph…`, and does not write them.
					t.glyphs.retain(|&g, _| g < count);
				}
			}
			let base = offset;
			// A glyph ID past the glyph order is named `glyphNNNNN` in fontTools,
			// which `getGlyphID` turns back into the same glyph ID of the merged
			// font.
			font.remap_glyphs(
				&mut |g| Ok::<u32, fontoxide_ttlib::Error>(if g < count { g + base } else { g }),
				glyph_order.clone(),
			)?;
			offset += count;
		}

		let mut cmaps = Vec::with_capacity(fonts.len());
		for font in &mut fonts {
			cmaps.push(font.require::<Cmap>()?.clone());
		}
		self.duplicate_glyphs_per_font = vec![DuplicateGlyphs::new(); fonts.len()];
		self.cmap = None;
		self.compute_mega_cmap(&cmaps.iter().collect::<Vec<_>>())?;
		drop(cmaps);

		let mut mega = Font::new();
		mega.sfnt_version = sfnt_version;
		mega.set_glyph_order(glyph_order);

		self.pre_merge(&mut fonts)?;

		let mut all_tags = BTreeSet::new();
		for font in &fonts {
			all_tags.extend(font.tags());
		}
		for tag in all_tags {
			if self.options.drops(&tag.as_string()) {
				continue;
			}
			if !MERGEABLE.contains(&&tag.0) {
				// LENIENT(L37): fontTools decompiles the table first, failing if it
				// cannot.
				log::info!("Don't know how to merge '{tag}'.");
				log::info!("Dropped '{tag}'.");
				continue;
			}
			let mut tables = fonts
				.iter_mut()
				.map(|f| f.get_dyn(tag))
				.collect::<fontoxide_ttlib::Result<Vec<_>>>()?;
			log::info!("Merging '{tag}'.");
			match self.merge_table(tag, &mut tables)? {
				Some(table) => {
					mega.insert(table);
					log::info!("Merged '{tag}'.");
				}
				None => log::info!("Dropped '{tag}'."),
			}
		}

		self.duplicate_glyphs_per_font.clear();
		self.font_names.clear();
		self.mark_sets = layout::MarkSets::default();

		self.post_merge(&mut mega)?;
		if self.beyond64k {
			upper_tables(&mut mega, &ConvertOptions::default())?;
		}
		Ok(mega)
	}

	/// Loads the tables of a font that refer to glyphs and are merged (or, for
	/// `cmap` and the layout tables, used).
	fn load_glyph_tables(&self, font: &mut Font) -> Result<()> {
		for tag in GLYPH_TABLES {
			let tag = Tag::new(tag);
			let always = matches!(&tag.0, b"cmap" | b"GDEF" | b"GSUB" | b"GPOS");
			if always || !self.options.drops(&tag.as_string()) {
				font.load(tag)?;
			}
		}
		Ok(())
	}

	/// `_preMerge`: the layout tables of the fonts in their compact forms
	/// (`_compact_layout_tables` without restoring them), and what
	/// `layoutPreMerge` needs from `GDEF`.
	fn pre_merge(&mut self, fonts: &mut [Font]) -> Result<()> {
		let mut gdefs = Vec::with_capacity(fonts.len());
		for font in fonts.iter_mut() {
			// Inputs are owned by the merger; normalize without restoring them.
			// The output is upgraded before serialization when needed.
			let (gdef, gsub, gpos) = with_compact_layout_tables(font, |font| -> Result<_> {
				Ok((
					font.get::<Gdef>()?.cloned(),
					font.get::<Gsub>()?.cloned(),
					font.get::<Gpos>()?.cloned(),
				))
			})?;
			if let Some(t) = gsub {
				font.insert(Box::new(t));
			}
			if let Some(t) = gpos {
				font.insert(Box::new(t));
			}
			if let Some(t) = &gdef {
				font.insert(Box::new(t.clone()));
			}
			gdefs.push(gdef.map(|g| g.table));
		}
		self.mark_sets = layout::MarkSets::new(
			&gdefs.iter().map(Option::as_ref).collect::<Vec<_>>(),
			!self.options.drops("GDEF"),
		);
		Ok(())
	}

	/// `_postMerge`. The layout tables were made whole when merged
	/// (`layoutPostMerge`).
	fn post_merge(&mut self, font: &mut Font) -> Result<()> {
		// _dropPostGlyphNamesIfNeeded
		if font.contains(Tag::new(b"post"))
			&& font.glyph_order()?.len() > 0xFFFF
			&& let Some(post) = font.get::<Post>()?
			&& post.format_type == 2.0
		{
			post.format_type = 3.0;
			post.extra_names = Some(Vec::new());
			post.mapping = Some(Default::default());
		}
		if font.contains(Tag::new(b"OS/2")) {
			// https://github.com/fonttools/fonttools/issues/2538
			// TODO (fontTools): Add an option to disable this?
			let widths: Vec<u16> = match font.get::<Hmtx>()? {
				Some(hmtx) => hmtx.metrics.values().map(|m| m.0).collect(),
				None => Vec::new(),
			};
			font.require::<Os2>()?.recalc_avg_char_width(widths);
		}
		Ok(())
	}

	/// Merges the tables of a tag (`getTableClass(tag)().merge(merger, tables)`),
	/// one per font (`None` where a font does not have it). Returns `None` if the
	/// table is dropped: fontoxide, like fontTools, only merges some tables.
	///
	/// [`Merger::merge`] calls this with the fonts in the glyph IDs of the
	/// merged font. The tables may be modified.
	pub fn merge_table(
		&mut self,
		tag: Tag,
		tables: &mut [Option<&mut (dyn Table + 'static)>],
	) -> Result<Option<Box<dyn Table>>> {
		let upper = UPPER_TABLES.contains(&&tag.0);
		Ok(match &tag.0 {
			b"maxp" | b"MAXP" => {
				let t = all(tag, downcast::<Maxp>(tag, tables)?)?;
				Some(Box::new(tables::merge_maxp(&shared(&t), upper)?))
			}
			b"head" => {
				let t = all(tag, downcast::<Head>(tag, tables)?)?;
				Some(Box::new(tables::merge_head(&shared(&t))?))
			}
			b"hhea" | b"HHEA" => {
				let t = all(tag, downcast::<Hhea>(tag, tables)?)?;
				Some(Box::new(tables::merge_hhea(&shared(&t), upper)?))
			}
			b"vhea" | b"VHEA" => {
				let t = all(tag, downcast::<Vhea>(tag, tables)?)?;
				Some(Box::new(tables::merge_vhea(&shared(&t), upper)?))
			}
			b"OS/2" => {
				let t = all(tag, downcast::<Os2>(tag, tables)?)?;
				Some(Box::new(tables::merge_os2(&shared(&t))?))
			}
			b"post" => {
				let t = all(tag, downcast::<Post>(tag, tables)?)?;
				Some(Box::new(tables::merge_post(&shared(&t))?))
			}
			b"hmtx" | b"HMTX" => {
				let mut t = all(tag, downcast::<Hmtx>(tag, tables)?)?;
				Some(Box::new(tables::merge_hmtx(&mut t, upper)))
			}
			b"vmtx" | b"VMTX" => {
				let mut t = all(tag, downcast::<Vmtx>(tag, tables)?)?;
				Some(Box::new(tables::merge_vmtx(&mut t, upper)))
			}
			b"name" => {
				let t = all(tag, downcast::<Name>(tag, tables)?)?;
				Some(Box::new(tables::merge_name(&shared(&t))))
			}
			b"loca" | b"LOCA" => {
				all(tag, downcast::<Loca>(tag, tables)?)?;
				Some(Box::new(tables::merge_loca(upper)))
			}
			b"glyf" | b"GLYF" => {
				let mut t = all(tag, downcast::<Glyf>(tag, tables)?)?;
				let num_glyphs = self.glyph_order.as_ref().map_or(0, Vec::len);
				Some(Box::new(tables::merge_glyf(&mut t, upper, num_glyphs)?))
			}
			b"prep" | b"fpgm" | b"cvt " | b"gasp" => {
				// The first font's, if it has one. FIXME (fontTools): `gasp` appears
				// irreconcilable.
				match tables.first_mut() {
					Some(Some(t)) => {
						let t: &dyn Table = &**t;
						let ok = match &tag.0 {
							b"prep" => t.as_any().is::<Prep>(),
							b"fpgm" => t.as_any().is::<Fpgm>(),
							b"cvt " => t.as_any().is::<Cvt>(),
							_ => t.as_any().is::<Gasp>(),
						};
						if !ok {
							return Err(fontoxide_ttlib::Error::WrongTableType(tag).into());
						}
						Some(t.clone_table())
					}
					_ => None,
				}
			}
			b"CFF " => {
				let mut t = all(tag, downcast::<Cff>(tag, tables)?)?;
				Some(Box::new(tables::merge_cff(&mut t)?))
			}
			b"cmap" => {
				let t = downcast::<Cmap>(tag, tables)?;
				let t: Vec<Option<&Cmap>> = t.into_iter().map(|t| t.map(|t| &*t)).collect();
				Some(Box::new(self.merge_cmap(&t)?))
			}
			b"GDEF" => {
				let t = downcast::<Gdef>(tag, tables)?;
				let t: Vec<&ot::GDEF> = t.into_iter().flatten().map(|t| &t.table).collect();
				if t.is_empty() {
					None
				} else {
					Some(Box::new(Gdef {
						table: layout::merge_gdef(&t)?,
					}))
				}
			}
			b"GSUB" => {
				let t = downcast::<Gsub>(tag, tables)?;
				let t = t.into_iter().map(|t| t.map(|t| &mut t.table)).collect();
				self.merge_layout::<ot::GSUB>(tag, t)?
					.map(|table| Box::new(Gsub { table }) as Box<dyn Table>)
			}
			b"GPOS" => {
				let t = downcast::<Gpos>(tag, tables)?;
				let t = t.into_iter().map(|t| t.map(|t| &mut t.table)).collect();
				self.merge_layout::<ot::GPOS>(tag, t)?
					.map(|table| Box::new(Gpos { table }) as Box<dyn Table>)
			}
			b"BASE" => {
				let t = downcast::<Base>(tag, tables)?;
				let t: Vec<&ot::BASE> = t.into_iter().flatten().map(|t| &t.table).collect();
				if t.is_empty() {
					None
				} else {
					Some(Box::new(Base {
						table: layout::merge_base(&t)?,
					}))
				}
			}
			b"JSTF" => {
				let t = downcast::<Jstf>(tag, tables)?;
				let t: Vec<&ot::JSTF> = t.into_iter().flatten().map(|t| &t.table).collect();
				layout::merge_jstf(&t)?.map(|table| Box::new(Jstf { table }) as Box<dyn Table>)
			}
			b"MATH" => {
				let t = downcast::<Math>(tag, tables)?;
				let t: Vec<&ot::MATH> = t.into_iter().flatten().map(|t| &t.table).collect();
				if t.is_empty() {
					None
				} else {
					Some(Box::new(Math {
						table: layout::merge_math(&t)?,
					}))
				}
			}
			_ => {
				log::info!("Don't know how to merge '{tag}'.");
				None
			}
		})
	}
}

/// The tables of a type.
fn downcast<'a, 'b: 'a, T: 'static>(
	tag: Tag,
	tables: &'a mut [Option<&'b mut (dyn Table + 'static)>],
) -> Result<Vec<Option<&'a mut T>>> {
	tables
		.iter_mut()
		.map(|t| match t {
			Some(t) => {
				let t: &'a mut (dyn Table + 'static) = &mut **t;
				AsAny::as_any_mut(t)
					.downcast_mut::<T>()
					.map(Some)
					.ok_or_else(|| fontoxide_ttlib::Error::WrongTableType(tag).into())
			}
			None => Ok(None),
		})
		.collect()
}

/// The tables, which all fonts must have: fontTools fails to merge the values of
/// the fonts that do not have the table (`NotImplemented`).
fn all<T>(tag: Tag, tables: Vec<Option<&mut T>>) -> Result<Vec<&mut T>> {
	tables
		.into_iter()
		.collect::<Option<Vec<_>>>()
		.ok_or_else(|| {
			Error::Type(format!(
				"TypeError: cannot merge '{tag}', which some of the fonts do not have"
			))
		})
}

fn shared<'a, T>(tables: &'a [&mut T]) -> Vec<&'a T> {
	tables.iter().map(|t| &**t).collect()
}
