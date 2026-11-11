//! Port of `fontTools.varLib.merger`: merges the OpenType Layout tables (`GDEF`,
//! `GPOS`, `GSUB`, `BASE`) and `COLR` of several masters.
//!
//! fontTools walks the tables generically, by their Python attributes; here the
//! walk is generated for each table type (`walk_generated.rs`), and the
//! mergers that fontTools registers for some types are in `special.rs`.
//!
//! Tables refer to glyphs by ID: the tables of the masters are remapped to the
//! glyph order of the font being built before they are merged, which is what
//! fontTools' glyph names give implicitly.

mod colr;
pub(crate) mod special;
pub(crate) mod walk;
mod walk_generated;

use std::sync::Arc;

use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::{Base, Gdef, Gpos, Gsub, Name};
use fontoxide_ttlib::{Font, GlyphOrder, Tag};

use crate::errors::{MergeError, MergeErrorKind, VarLibError};
use crate::models::VariationModel;
use crate::var_store::OnlineVarStoreBuilder;

pub use colr::{ColrMergeResult, expand_paint_colr_layers};

/// The result type of the mergers.
pub(crate) type MResult<T = ()> = Result<T, VarLibError>;

/// Which merger: the classes of `fontTools.varLib.merger`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergerKind {
	/// `VariationMerger`: builds a variable font from masters.
	Variation,
	/// `COLRVariationMerger`: the same for `COLR`.
	ColrVariation,
	/// `InstancerMerger`: interpolates an instance of masters.
	Instancer,
}

/// What the error messages need of each master.
#[derive(Debug, Clone, Default)]
pub struct MasterInfo {
	/// The best full name, else the file path (`_master_name`).
	pub name: Option<String>,
	/// The feature tags of `GSUB` and `GPOS`, for the details of a
	/// `FeatureCount` that differs.
	pub features: Vec<(Tag, Vec<Tag>)>,
}

impl MasterInfo {
	/// The information of a master font.
	pub fn of(font: &mut Font, path: Option<&str>) -> crate::errors::Result<Self> {
		let name = match font.get::<Name>()? {
			Some(name) => name.best_full_name().filter(|n| !n.is_empty()),
			None => None,
		}
		.or_else(|| path.map(str::to_owned));
		let mut features = Vec::new();
		if let Some(t) = font.get::<Gsub>()? {
			features.push((
				Tag::new(b"GSUB"),
				feature_tags(t.table.feature_list.as_deref()),
			));
		}
		if let Some(t) = font.get::<Gpos>()? {
			features.push((
				Tag::new(b"GPOS"),
				feature_tags(t.table.feature_list.as_deref()),
			));
		}
		Ok(MasterInfo { name, features })
	}
}

fn feature_tags(list: Option<&ot::FeatureList>) -> Vec<Tag> {
	list.map(|l| l.feature_record.iter().map(|r| r.feature_tag).collect())
		.unwrap_or_default()
}

/// The state of `COLRVariationMerger`.
#[derive(Debug, Default)]
pub(crate) struct ColrState {
	/// `varIndexCache`.
	pub var_index_cache: std::collections::HashMap<Vec<u32>, u32>,
	/// `varIdxes`.
	pub var_idxes: Vec<u32>,
	/// `layers`.
	pub layers: Vec<ot::Paint>,
	/// `layerReuseCache`.
	pub layer_reuse_cache: Option<fontoxide_colorlib::builder::LayerReuseCache>,
	/// `_doneBaseGlyphs`.
	pub done_base_glyphs: bool,
	/// The layer lists of the output font and of the masters, for
	/// `_flatten_layers`.
	pub out_layers: Vec<Option<Box<ot::Paint>>>,
	pub master_layers: Vec<Vec<Option<Box<ot::Paint>>>>,
}

/// `Merger` and its subclasses.
#[derive(Debug)]
pub struct Merger<'a> {
	pub(crate) kind: MergerKind,
	/// The glyph order of the font being built.
	pub(crate) order: Arc<GlyphOrder>,
	/// `font.hasExtendedGlyphIDs()`.
	pub(crate) extended: bool,
	/// `fontTools.otlLib.optimize.gpos:COMPRESSION_LEVEL` of the font.
	pub(crate) compression_level: u8,
	/// `ttfs`: the masters being merged, as indices into `masters`.
	pub(crate) ttfs: Vec<usize>,
	masters: &'a [MasterInfo],
	/// `model`.
	pub(crate) model: VariationModel,
	/// `store_builder` of the variation mergers.
	pub(crate) store_builder: OnlineVarStoreBuilder,
	/// `masterScalars` of `InstancerMerger`.
	pub(crate) master_scalars: Vec<f64>,
	/// `lookup_subtables`, set while merging a lookup.
	pub(crate) lookup_subtables: Vec<Vec<Option<Box<ot::LookupSubTable>>>>,
	/// `valueFormat1` and `valueFormat2`, set while merging a PairPos.
	pub(crate) value_formats: (u16, u16),
	/// `_firstGlyph` of the PairSet being merged.
	pub(crate) first_glyph: Option<u32>,
	/// The state of `COLRVariationMerger`.
	pub(crate) colr: ColrState,
}

impl<'a> Merger<'a> {
	/// `VariationMerger(model, axisTags, font)`.
	pub fn variation(
		model: &VariationModel,
		axis_tags: &[String],
		font: &mut Font,
		masters: &'a [MasterInfo],
	) -> crate::errors::Result<Self> {
		let mut store_builder = OnlineVarStoreBuilder::new(axis_tags.to_vec())?;
		store_builder.set_model(model);
		Self::new(MergerKind::Variation, model, store_builder, font, masters)
	}

	/// `InstancerMerger(font, model, location)`.
	pub fn instancer(
		font: &mut Font,
		model: &VariationModel,
		location: &[(String, f64)],
		masters: &'a [MasterInfo],
	) -> crate::errors::Result<Self> {
		let store_builder = OnlineVarStoreBuilder::new(Vec::new())?;
		let mut m = Self::new(MergerKind::Instancer, model, store_builder, font, masters)?;
		m.master_scalars = model.get_master_scalars(location)?;
		Ok(m)
	}

	/// `COLRVariationMerger(model, axisTags, font, allowLayerReuse)`.
	pub fn colr_variation(
		model: &VariationModel,
		axis_tags: &[String],
		font: &mut Font,
		masters: &'a [MasterInfo],
		allow_layer_reuse: bool,
	) -> crate::errors::Result<Self> {
		let mut m = Self::variation(model, axis_tags, font, masters)?;
		m.kind = MergerKind::ColrVariation;
		if allow_layer_reuse {
			m.colr.layer_reuse_cache = Some(Default::default());
		}
		Ok(m)
	}

	fn new(
		kind: MergerKind,
		model: &VariationModel,
		store_builder: OnlineVarStoreBuilder,
		font: &mut Font,
		masters: &'a [MasterInfo],
	) -> crate::errors::Result<Self> {
		let order = font.glyph_order()?;
		Ok(Merger {
			kind,
			extended: order.len() > 0x10000,
			order,
			compression_level: font.gpos_compression_level,
			ttfs: (0..masters.len()).collect(),
			masters,
			model: model.clone(),
			store_builder,
			master_scalars: Vec::new(),
			lookup_subtables: Vec::new(),
			value_formats: (0, 0),
			first_glyph: None,
			colr: ColrState::default(),
		})
	}

	/// Whether this is a `VariationMerger`, which lets masters with `None`
	/// values sit out.
	pub(crate) fn is_variation(&self) -> bool {
		matches!(self.kind, MergerKind::Variation | MergerKind::ColrVariation)
	}

	/// `setModel`.
	pub(crate) fn set_model(&mut self, model: VariationModel) {
		self.store_builder.set_model(&model);
		self.model = model;
	}

	/// The sub-model of the masters in `mask`, as `VariationMerger.mergeThings`
	/// sets it, with the masters (`ttfs`) subset alike. Returns what
	/// [`Merger::pop_sub_model`] restores.
	pub(crate) fn push_sub_model(
		&mut self,
		mask: &[bool],
	) -> MResult<(VariationModel, Vec<usize>)> {
		let sub = self.model.sub_model(mask)?;
		let saved_model = std::mem::replace(&mut self.model, sub);
		self.store_builder.set_model(&self.model);
		let saved_ttfs = self.ttfs.clone();
		if !self.ttfs.is_empty() {
			self.ttfs = self
				.ttfs
				.iter()
				.zip(mask)
				.filter(|(_, keep)| **keep)
				.map(|(t, _)| *t)
				.collect();
		}
		Ok((saved_model, saved_ttfs))
	}

	/// Restores the model and masters after [`Merger::push_sub_model`].
	pub(crate) fn pop_sub_model(&mut self, saved: (VariationModel, Vec<usize>)) {
		self.set_model(saved.0);
		self.ttfs = saved.1;
	}

	/// `_master_name(ix)`.
	fn master_name(&self, ix: usize) -> String {
		self.ttfs
			.get(ix)
			.and_then(|&t| self.masters.get(t))
			.and_then(|m| m.name.clone())
			.unwrap_or_else(|| format!("master number {ix}"))
	}

	/// A merge error raised with this merger. `cause` has `repr(expected)` and,
	/// for each value of `got`, whether it equals the expected value and its
	/// `repr`.
	pub(crate) fn merge_error(
		&self,
		kind: MergeErrorKind,
		cause: Option<(String, Vec<(bool, String)>)>,
	) -> VarLibError {
		let mut e = MergeError::new(kind);
		e.ttfs = self.ttfs.clone();
		if let Some((expected, got)) = cause {
			// `[x == expected for x in got].index(False)`.
			if let Some(index) = got.iter().position(|(equal, _)| !equal) {
				e.offender = Some((index, self.master_name(index)));
				e.expected_got = Some((expected.clone(), got[index].1.clone()));
			}
			e.expected_int = expected.parse().ok();
		}
		e.into()
	}

	/// `ShouldBeConstant(merger, expected=..., got=...)`.
	pub(crate) fn should_be_constant(
		&self,
		expected: String,
		got: Vec<(bool, String)>,
	) -> VarLibError {
		self.merge_error(MergeErrorKind::ShouldBeConstant, Some((expected, got)))
	}

	/// `FoundANone(merger, got=lst)`, with the type names of the values.
	pub(crate) fn found_a_none(&self, got: Vec<Option<&str>>) -> VarLibError {
		self.none_error(MergeErrorKind::FoundANone, got, true)
	}

	/// `NotANone(merger, expected=None, got=lst)`.
	pub(crate) fn not_a_none(&self, got: Vec<Option<&str>>) -> VarLibError {
		self.none_error(MergeErrorKind::NotANone, got, false)
	}

	fn none_error(
		&self,
		kind: MergeErrorKind,
		got: Vec<Option<&str>>,
		find_none: bool,
	) -> VarLibError {
		let mut e = MergeError::new(kind);
		e.ttfs = self.ttfs.clone();
		if let Some(index) = got.iter().position(|g| g.is_none() == find_none) {
			e.offender = Some((index, self.master_name(index)));
		}
		// LENIENT(L36): the objects are shown by type name, without their address.
		let items: Vec<String> = got
			.iter()
			.map(|g| g.map_or_else(|| "None".to_owned(), |t| format!("<{t} object>")))
			.collect();
		e.got_list = Some(format!("[{}]", items.join(", ")));
		e.into()
	}

	/// `UnsupportedFormat(merger, subtable=...)`.
	pub(crate) fn unsupported_format(&self, subtable: &str, value: Option<String>) -> VarLibError {
		let mut e = MergeError::new(MergeErrorKind::UnsupportedFormat {
			subtable: subtable.to_owned(),
			value,
		});
		e.ttfs = self.ttfs.clone();
		e.into()
	}

	/// The name of a glyph of the font.
	pub(crate) fn glyph_name(&self, gid: u32) -> String {
		self.order.name(gid).into_owned()
	}

	/// `Merger.mergeTables(font, master_ttfs, tableTags)` for the OpenType Layout
	/// tables `GSUB`, `GDEF`, `GPOS` and `BASE`; the tables of the masters are in
	/// `master_fonts`.
	pub fn merge_tables(
		&mut self,
		font: &mut Font,
		master_fonts: &mut [&mut Font],
		tags: &[&str],
	) -> crate::errors::Result<()> {
		for tag in tags {
			let result = match *tag {
				"GSUB" => self.merge_otl::<Gsub, _>(font, master_fonts, |t| &mut t.table),
				"GPOS" => self.merge_otl::<Gpos, _>(font, master_fonts, |t| &mut t.table),
				"GDEF" => self.merge_otl::<Gdef, _>(font, master_fonts, |t| &mut t.table),
				"BASE" => self.merge_otl::<Base, _>(font, master_fonts, |t| &mut t.table),
				_ => Ok(()),
			};
			result.map_err(|e| self.finish_error(e.push_stack(*tag), master_fonts))?;
		}
		Ok(())
	}

	fn merge_otl<T, O>(
		&mut self,
		font: &mut Font,
		master_fonts: &mut [&mut Font],
		inner: impl Fn(&mut T) -> &mut O,
	) -> MResult
	where
		T: fontoxide_ttlib::TableType,
		O: OtTable + walk::Walk,
	{
		if !font.has::<T>() {
			return Ok(());
		}
		self.ttfs = (0..master_fonts.len()).collect();
		let mut tables: Vec<Option<O>> = Vec::with_capacity(master_fonts.len());
		for master in master_fonts.iter_mut() {
			tables.push(master_table(master, &self.order, |m| {
				Ok(m.get::<T>()?.map(|t| inner(t).clone()))
			})?);
		}
		let Some(out) = font.get::<T>()? else {
			return Ok(());
		};
		let out = inner(out);
		// `font[tag]` is a table object whose `table` attribute holds the table.
		let lst: Vec<Option<&O>> = tables.iter().map(Option::as_ref).collect();
		walk::merge_opt(self, Some(out), &lst).map_err(|e| e.push_stack(".table"))
	}

	/// Adds what `ShouldBeConstant.details` shows for a `FeatureCount` that
	/// differs: the features of the masters.
	fn finish_error(&self, e: VarLibError, master_fonts: &[&mut Font]) -> VarLibError {
		let _ = master_fonts;
		let VarLibError::Merge(mut e) = e else {
			return e;
		};
		if e.kind == MergeErrorKind::ShouldBeConstant
			&& e.stack.first().is_some_and(|s| s == ".FeatureCount")
			&& let (Some(tag), Some((offender, _))) = (e.stack.last().cloned(), e.offender.clone())
		{
			let tag = Tag::from_str_padded(&tag);
			let features = |ix: usize| -> Option<&Vec<Tag>> {
				let info = self.masters.get(ix)?;
				info.features
					.iter()
					.find(|(t, _)| Some(*t) == tag)
					.map(|(_, f)| f)
			};
			let bad = e.ttfs.get(offender).and_then(|&ix| features(ix));
			let good = e
				.ttfs
				.iter()
				.filter_map(|&ix| features(ix))
				.find(|f| Some(f.len() as i64) == e.expected_int);
			if let (Some(good), Some(bad)) = (good, bad) {
				let join = |tags: &Vec<Tag>| {
					tags.iter()
						.map(|t| t.to_string())
						.collect::<Vec<_>>()
						.join(", ")
				};
				e.extra_details = format!(
					"\nIncompatible features between masters.\nExpected: {}.\nGot: {}.\n",
					join(good),
					join(bad)
				);
			}
		}
		VarLibError::Merge(e)
	}

	/// `mergeThings(out, lst)` of paints, as the tests of the COLR merger use it.
	pub fn merge_paint(
		&mut self,
		out: &mut ot::Paint,
		lst: &[&ot::Paint],
	) -> crate::errors::Result<()> {
		let lst: Vec<Option<&ot::Paint>> = lst.iter().map(|p| Some(*p)).collect();
		walk::merge_opt(self, Some(out), &lst)
	}

	/// `mergeThings(out, lst)` of clip lists, as the tests of the COLR merger use
	/// it.
	pub fn merge_clip_list(
		&mut self,
		out: &mut ot::ClipList,
		lst: &[&ot::ClipList],
	) -> crate::errors::Result<()> {
		let lst: Vec<Option<&ot::ClipList>> = lst.iter().map(|p| Some(*p)).collect();
		walk::merge_opt(self, Some(out), &lst)
	}

	/// `varIdxes` of `COLRVariationMerger`.
	pub fn colr_var_idxes(&self) -> &[u32] {
		&self.colr.var_idxes
	}

	/// `COLRVariationMerger.mergeTables(font, master_ttfs)`.
	pub fn merge_colr(
		&mut self,
		font: &mut Font,
		master_fonts: &mut [&mut Font],
	) -> crate::errors::Result<()> {
		colr::merge_tables(self, font, master_fonts)
	}
}

/// A table of a master, with its glyph IDs remapped to `order`.
pub(crate) fn master_table<O: OtTable>(
	master: &mut Font,
	order: &Arc<GlyphOrder>,
	get: impl FnOnce(&mut Font) -> fontoxide_ttlib::Result<Option<O>>,
) -> MResult<Option<O>> {
	let master_order = master.glyph_order()?;
	let Some(mut table) = get(master)? else {
		return Ok(None);
	};
	if master_order.names() != order.names() {
		table.remap_glyphs(&mut |gid| {
			let name = master_order.name(gid);
			order
				.id(&name)
				.ok_or_else(|| fontoxide_ttlib::Error::ttlib(format!("KeyError: {name:?}")))
		})?;
	}
	Ok(Some(table))
}
