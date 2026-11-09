//! Port of `fontTools.subset`: removes the glyphs, characters, layout
//! features and other data a font does not need.
//!
//! ```no_run
//! use fontoxide_subset::{Options, Subsetter};
//! use fontoxide_ttlib::{Font, ReadOptions, WriteOptions};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut font = Font::read(std::fs::read("font.ttf")?, ReadOptions::default())?;
//! let mut subsetter = Subsetter::new(Options::default());
//! subsetter.populate(&[], &[], &[], "Hello");
//! subsetter.subset(&mut font)?;
//! std::fs::write("subset.ttf", font.write(&WriteOptions::default())?)?;
//! # Ok(())
//! # }
//! ```
//!
//! fontTools refers to glyphs by name; here glyphs are glyph IDs of the original
//! font until the end, when every table is remapped to the new glyph order.

#![forbid(unsafe_code)]

mod aat;
mod bitmap;
mod cff;
mod cli;
mod cmap;
mod colr;
mod glyf;
mod layout;
mod math;
mod max_context;
pub mod options;
mod svg;
mod tables;
mod varc;
#[doc(hidden)]
pub mod varstore;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use fontoxide_ttlib::{Font, GlyphOrder, Tag};

pub use cli::{load_font, main, parse_gids, parse_glyphs, parse_unicodes, save_font};
pub use layout::prune_table_lookups as prune_lookups;
pub use options::{Item, Options};

/// An error while subsetting.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// An error of the font library.
	#[error(transparent)]
	Ttlib(#[from] fontoxide_ttlib::Error),
	/// An error reading or writing a file.
	#[error(transparent)]
	Io(#[from] std::io::Error),
	/// `MissingGlyphsSubsettingError`: requested glyphs the font does not have.
	#[error("MissingGlyphsSubsettingError: {0:?}")]
	MissingGlyphs(Vec<String>),
	/// `MissingUnicodesSubsettingError`: requested characters without glyphs.
	#[error("MissingUnicodesSubsettingError: {0:?}")]
	MissingUnicodes(Vec<String>),
	/// `OptionError` / `UnknownOptionError`.
	#[error("OptionError: {0}")]
	Option(String),
	/// Subsetting a table that fontoxide does not support yet.
	#[error("subsetting '{0}' is not supported yet")]
	Unsupported(String),
}

/// Shorthand for `Result<T, fontoxide_subset::Error>`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A set of glyph IDs of the original font.
pub type GlyphSet = BTreeSet<u32>;

/// The subsetter (`Subsetter`): [`Subsetter::populate`] with what to keep,
/// then [`Subsetter::subset`] a font.
#[derive(Debug, Default)]
pub struct Subsetter {
	/// The options.
	pub options: Options,
	unicodes_requested: BTreeSet<u32>,
	glyph_names_requested: BTreeSet<String>,
	glyph_ids_requested: BTreeSet<u32>,

	/// The glyph order of the font before subsetting.
	pub(crate) orig_glyph_order: Arc<GlyphOrder>,
	/// The glyph set being closed or subset (`glyphs`).
	pub(crate) glyphs: GlyphSet,
	pub(crate) glyphs_requested: GlyphSet,
	pub(crate) unicodes_missing: BTreeSet<u32>,
	pub(crate) unicodes_dmaped: BTreeSet<u32>,
	pub(crate) uvs_dmaped: BTreeSet<(u32, u32)>,
	pub(crate) glyphs_cmaped: GlyphSet,
	pub(crate) glyphs_wo_math_closure: GlyphSet,
	pub(crate) glyphs_mathed: GlyphSet,
	pub(crate) glyphs_gsubed: GlyphSet,
	pub(crate) glyphs_colred: GlyphSet,
	pub(crate) glyphs_bslned: GlyphSet,
	pub(crate) glyphs_glyfed: GlyphSet,
	pub(crate) glyphs_cffed: GlyphSet,
	/// The glyphs kept (`glyphs_retained`).
	pub(crate) glyphs_retained: GlyphSet,
	/// With `retain_gids`, the removed glyphs before the last kept one, which
	/// are kept empty (`glyphs_emptied`).
	pub(crate) glyphs_emptied: GlyphSet,
	pub(crate) last_retained_order: u32,
	/// The original IDs of the glyphs of the new glyph order.
	pub(crate) new_glyph_order: Vec<u32>,
	/// Old glyph ID to new glyph ID (`glyph_index_map`).
	pub(crate) glyph_index_map: BTreeMap<u32, u32>,
	/// The mark glyph sets used by the kept lookups (`used_mark_sets`).
	pub(crate) used_mark_sets: Vec<u32>,
}

/// `_sort_tables`: the tables in the order they are processed.
fn sort_tables(font: &Font) -> Vec<Tag> {
	const ORDER: [&[u8; 4]; 10] = [
		b"GDEF", b"GPOS", b"GSUB", b"fvar", b"avar", b"gvar", b"GVAR", b"name", b"glyf", b"GLYF",
	];
	let key = |t: &Tag| ORDER.iter().position(|o| t.0 == **o).map_or(0, |i| i + 1);
	let mut tags = font.tags();
	// A stable sort, as Python's `sorted`.
	tags.sort_by_key(key);
	tags
}

/// The tag as fontTools compares it with option lists (`tag.strip()`).
pub(crate) fn strip(tag: Tag) -> String {
	tag.to_string().trim().to_owned()
}

impl Subsetter {
	/// A subsetter with the given options.
	pub fn new(options: Options) -> Self {
		Subsetter {
			options,
			..Default::default()
		}
	}

	/// `populate`: what to keep, by glyph name, glyph ID, character, or text.
	pub fn populate(&mut self, glyphs: &[&str], gids: &[u32], unicodes: &[u32], text: &str) {
		self.unicodes_requested.extend(unicodes);
		self.unicodes_requested.extend(text.chars().map(u32::from));
		self.glyph_names_requested
			.extend(glyphs.iter().map(|g| (*g).to_owned()));
		self.glyph_ids_requested.extend(gids);
	}

	/// The name of a glyph of the original font.
	pub(crate) fn glyph_name(&self, gid: u32) -> String {
		self.orig_glyph_order.name(gid).into_owned()
	}

	/// Subsets the font in place (`subset`).
	pub fn subset(&mut self, font: &mut Font) -> Result<()> {
		fontoxide_ttlib::beyond64k::with_compact_layout_tables(font, |font| {
			self.prune_pre_subset(font)?;
			self.closure_glyphs(font)?;
			self.subset_glyphs(font)?;
			self.prune_post_subset(font)
		})
	}

	/// `_prune_pre_subset`.
	fn prune_pre_subset(&mut self, font: &mut Font) -> Result<()> {
		for tag in sort_tables(font) {
			let name = strip(tag);
			if options::has_str(&self.options.drop_tables, &name)
				|| (options::has_str(&self.options.hinting_tables, &name) && !self.options.hinting)
				|| (tag == *b"kern"
					&& !self.options.legacy_kern
					&& font.contains(Tag::new(b"GPOS")))
			{
				log::info!("{tag} dropped");
				font.remove(tag);
				continue;
			}
			match tables::prune_pre_subset(self, font, tag)? {
				Some(false) => {
					log::info!("{tag} pruned to empty; dropped");
					font.remove(tag);
				}
				Some(true) => log::info!("{tag} pruned"),
				None => {}
			}
		}
		Ok(())
	}

	/// `_closure_glyphs`: the glyphs to keep, closed over the tables that refer
	/// to other glyphs.
	fn closure_glyphs(&mut self, font: &mut Font) -> Result<()> {
		let order = font.glyph_order()?;
		self.orig_glyph_order = order.clone();
		let num_glyphs = order.len() as u32;

		let mut missing = Vec::new();
		self.glyphs_requested = GlyphSet::new();
		for name in &self.glyph_names_requested {
			match order.id(name) {
				Some(g) => {
					self.glyphs_requested.insert(g);
				}
				None => missing.push(name.clone()),
			}
		}
		for &g in &self.glyph_ids_requested {
			if g < num_glyphs {
				self.glyphs_requested.insert(g);
			} else {
				missing.push(g.to_string());
			}
		}
		if !missing.is_empty() {
			log::info!("Missing requested glyphs: {missing:?}");
			if !self.options.ignore_missing_glyphs {
				return Err(Error::MissingGlyphs(missing));
			}
		}

		self.glyphs = self.glyphs_requested.clone();
		self.unicodes_missing = BTreeSet::new();
		if font.contains(Tag::new(b"cmap")) || font.contains(Tag::new(b"DMAP")) {
			cmap::closure_glyphs_cmap_and_dmap(self, font)?;
			self.glyphs.retain(|&g| g < num_glyphs);
		}
		self.glyphs_cmaped = self.glyphs.clone();
		if !self.unicodes_missing.is_empty() {
			let missing: Vec<String> = self
				.unicodes_missing
				.iter()
				.map(|u| format!("U+{u:04X}"))
				.collect();
			log::info!("Missing glyphs for requested Unicodes: {missing:?}");
			if !self.options.ignore_missing_unicodes {
				return Err(Error::MissingUnicodes(missing));
			}
		}

		let has_glyf = font.contains(Tag::new(b"glyf")) || font.contains(Tag::new(b"GLYF"));
		if self.options.notdef_glyph {
			if has_glyf {
				self.glyphs.insert(0);
				log::info!("Added gid0 to subset");
			} else {
				let notdef = order
					.id(".notdef")
					.ok_or_else(|| fontoxide_ttlib::Error::ttlib("KeyError: '.notdef'"))?;
				self.glyphs.insert(notdef);
				log::info!("Added .notdef to subset");
			}
		}
		if self.options.recommended_glyphs && has_glyf {
			self.glyphs.extend(0..num_glyphs.min(4));
			log::info!("Added first four glyphs to subset");
		}

		self.glyphs_wo_math_closure = self.glyphs.clone();
		for tag in [
			b"MATH", b"GSUB", b"COLR", b"bsln", b"VARC", b"glyf", b"GLYF", b"CFF ",
		] {
			let tag = Tag::new(tag);
			let close = font.contains(tag) && (tag != *b"GSUB" || self.options.layout_closure);
			if close {
				log::info!(
					"Closing glyph list over '{tag}': {} glyphs before",
					self.glyphs.len()
				);
				tables::closure_glyphs(self, font, tag)?;
				self.glyphs.retain(|&g| g < num_glyphs);
				log::info!(
					"Closed glyph list over '{tag}': {} glyphs after",
					self.glyphs.len()
				);
			}
			match &tag.0 {
				b"MATH" => self.glyphs_mathed = self.glyphs.clone(),
				b"GSUB" => self.glyphs_gsubed = self.glyphs.clone(),
				b"COLR" => self.glyphs_colred = self.glyphs.clone(),
				b"bsln" => self.glyphs_bslned = self.glyphs.clone(),
				b"VARC" | b"GLYF" => self.glyphs_glyfed = self.glyphs.clone(),
				b"CFF " => self.glyphs_cffed = self.glyphs.clone(),
				_ => {}
			}
		}

		self.glyphs_retained = std::mem::take(&mut self.glyphs);
		self.last_retained_order = self
			.glyphs_retained
			.last()
			.copied()
			.ok_or_else(|| fontoxide_ttlib::Error::value("max() arg is an empty sequence"))?;
		self.glyphs_emptied = GlyphSet::new();
		if self.options.retain_gids {
			self.glyphs_emptied = (0..=self.last_retained_order)
				.filter(|g| !self.glyphs_retained.contains(g))
				.collect();
		}
		self.new_glyph_order = if self.options.retain_gids {
			(0..=self.last_retained_order).collect()
		} else {
			self.glyphs_retained.iter().copied().collect()
		};
		self.glyph_index_map = self
			.new_glyph_order
			.iter()
			.enumerate()
			.map(|(i, &g)| (g, i as u32))
			.collect();
		log::info!("Retaining {} glyphs", self.glyphs_retained.len());
		Ok(())
	}

	/// `_subset_glyphs`: subsets each table to the kept glyphs, then remaps the
	/// tables to the new glyph order.
	fn subset_glyphs(&mut self, font: &mut Font) -> Result<()> {
		self.used_mark_sets = Vec::new();
		for tag in sort_tables(font) {
			let name = strip(tag);
			if options::has_str(&self.options.no_subset_tables, &name) {
				log::info!("{tag} subsetting not needed");
				continue;
			}
			self.glyphs = self.glyphs_retained.clone();
			let retain = tables::subset_glyphs(self, font, tag)?;
			self.glyphs = GlyphSet::new();
			match retain {
				Some(true) => log::info!("{tag} subsetted"),
				Some(false) => {
					log::info!("{tag} subsetted to empty; dropped");
					font.remove(tag);
				}
				None if self.options.passthrough_tables => {
					log::info!("{tag} NOT subset; don't know how to subset");
				}
				None => {
					log::warn!("{tag} NOT subset; don't know how to subset; dropped");
					font.remove(tag);
				}
			}
		}
		let names: Vec<String> = self
			.new_glyph_order
			.iter()
			.map(|&g| self.glyph_name(g))
			.collect();
		let map = &self.glyph_index_map;
		let order = &self.orig_glyph_order;
		font.remap_glyphs(
			&mut |g| {
				// `getGlyphID`: a `glyphXXXXX` name that is no longer in the glyph
				// order is parsed.
				let name = order.name(g);
				map.get(&g)
					.copied()
					.or_else(|| fontoxide_ttlib::GlyphOrder::default().id(&name))
					.ok_or_else(|| fontoxide_ttlib::Error::ttlib(format!("KeyError: {name:?}")))
			},
			names,
		)?;
		Ok(())
	}

	/// `_prune_post_subset`.
	fn prune_post_subset(&mut self, font: &mut Font) -> Result<()> {
		let mut tags = font.tags();
		// The name table is pruned last, as that looks for name IDs used by the
		// other tables.
		if let Some(i) = tags.iter().position(|t| *t == *b"name") {
			let name = tags.remove(i);
			tags.push(name);
		}
		for tag in tags {
			if tag == *b"OS/2" {
				tables::os2_prune(self, font)?;
			}
			match tables::prune_post_subset(self, font, tag)? {
				Some(false) => {
					log::info!("{tag} pruned to empty; dropped");
					font.remove(tag);
				}
				Some(true) => log::info!("{tag} pruned"),
				None => {}
			}
		}
		Ok(())
	}
}
