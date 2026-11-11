//! Port of `fontTools.merge.layout`: merging `GDEF`, `GSUB`, `GPOS`, `BASE`,
//! `JSTF` and `MATH`.
//!
//! fontTools replaces the lookup and feature indices of `GSUB` and `GPOS` by
//! references to the objects before merging (`layoutPreMerge`), merges the
//! objects, then turns the references back into indices, dropping the features
//! and lookups no longer used (`layoutPostMerge`). Here the features and lookups
//! of all the fonts go to an arena, and references are arena indices; both steps
//! happen while merging the table.

use std::collections::{BTreeMap, HashMap, HashSet};

use fontoxide_ttlib::Tag;
use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types as ot;

use crate::{Error, Merger, Result};

/// The mark glyph sets of the fonts, as `layoutPreMerge` and `layoutPostMerge`
/// see them in the `GDEF` tables.
#[derive(Debug, Clone, Default)]
pub(crate) struct MarkSets {
	/// For each font whose lookups refer to their mark filtering set by
	/// reference (its `GDEF` is version 1.2 or later and has
	/// `MarkGlyphSetsDef`): the index of its first set in the merged `GDEF`, and
	/// the number of its sets.
	pub bases: Vec<Option<(usize, usize)>>,
	/// Whether the merged font has a `GDEF` of version 1.2 or later with
	/// `MarkGlyphSetsDef`, whose sets are then mapped back to indices.
	pub merged: bool,
}

impl MarkSets {
	/// `layoutPreMerge`'s view of the compact `GDEF` tables of the fonts, and
	/// whether the merged font keeps `GDEF`.
	pub(crate) fn new(gdefs: &[Option<&ot::GDEF>], keep_gdef: bool) -> Self {
		let mut bases = Vec::with_capacity(gdefs.len());
		let mut next = 0;
		let mut version = None;
		let mut any_sets = false;
		for &gdef in gdefs {
			let sets = gdef.and_then(mark_glyph_sets_def);
			bases.push(match (gdef, sets) {
				(Some(g), Some(sets)) if g.version >= 0x0001_0002 => {
					Some((next, sets.coverage.len()))
				}
				_ => None,
			});
			if let Some(sets) = sets {
				next += sets.coverage.len();
				any_sets = true;
			}
			if let Some(g) = gdef {
				version = version.max(Some(g.version));
			}
		}
		MarkSets {
			bases,
			merged: keep_gdef && version.is_some_and(|v| v >= 0x0001_0002) && any_sets,
		}
	}
}

/// `_get_mark_glyph_sets_def`.
fn mark_glyph_sets_def(gdef: &ot::GDEF) -> Option<&ot::MarkGlyphSetsDef> {
	gdef.mark_glyph_sets_def2
		.as_deref()
		.or(gdef.mark_glyph_sets_def.as_deref())
}

fn attribute_error(attr: &str) -> Error {
	Error::Type(format!(
		"AttributeError: 'NoneType' object has no attribute '{attr}'"
	))
}

fn key_error(key: impl std::fmt::Display) -> Error {
	Error::Type(format!("KeyError: {key}"))
}

fn no_merge_map(class: &str) -> Error {
	Error::Type(format!(
		"AttributeError: type object '{class}' has no attribute 'mergeMap'"
	))
}

// mergeObjects and the merge logic of the otTables classes.

/// `mergeObjects` of optional subtables: `None` if none is set.
fn merge_opt<'a, T: 'a, R>(
	items: impl IntoIterator<Item = &'a Option<Box<T>>>,
	f: impl FnOnce(Vec<&'a T>) -> Result<R>,
) -> Result<Option<Box<R>>> {
	let items: Vec<&T> = items.into_iter().filter_map(|t| t.as_deref()).collect();
	if items.is_empty() {
		return Ok(None);
	}
	f(items).map(|r| Some(Box::new(r)))
}

/// `Coverage.mergeMap`: the glyphs one after the other (`sumLists`).
fn merge_coverage(items: Vec<&ot::Coverage>) -> Result<ot::Coverage> {
	Ok(ot::Coverage {
		glyphs: items
			.iter()
			.flat_map(|c| c.glyphs.iter().copied())
			.collect(),
	})
}

fn merge_opt_coverage<'a>(
	items: impl IntoIterator<Item = &'a Option<Box<ot::Coverage>>>,
) -> Result<Option<Box<ot::Coverage>>> {
	merge_opt(items, merge_coverage)
}

/// `ClassDef.mergeMap`: the classes of all (`sumDicts`).
fn merge_class_def(items: Vec<&ot::ClassDef>) -> Result<ot::ClassDef> {
	let mut class_defs = BTreeMap::new();
	for item in items {
		class_defs.extend(item.class_defs.iter().map(|(&g, &c)| (g, c)));
	}
	Ok(ot::ClassDef { class_defs })
}

/// `sum` of computed counts, which fontTools has from reading.
fn sum_counts(counts: impl IntoIterator<Item = (Option<u32>, usize)>) -> Option<u32> {
	Some(
		counts
			.into_iter()
			.map(|(c, len)| c.unwrap_or(len as u32))
			.sum(),
	)
}

fn concat<T: Clone>(items: &[impl AsRef<[T]>]) -> Vec<T> {
	items
		.iter()
		.flat_map(|i| i.as_ref().iter().cloned())
		.collect()
}

/// `equal`.
fn equal<T: PartialEq + Copy + std::fmt::Debug>(items: &[T]) -> Result<T> {
	let first = *items
		.first()
		.ok_or_else(|| Error::Type("StopIteration".into()))?;
	if items.iter().any(|i| *i != first) {
		return Err(Error::Assertion(format!(
			"Expected all items to be equal: {items:?}"
		)));
	}
	Ok(first)
}

/// `GDEF.mergeMap` (`mergeObjects` of the tables).
pub(crate) fn merge_gdef(tables: &[&ot::GDEF]) -> Result<ot::GDEF> {
	if tables.iter().any(|t| t.var_store.is_some()) {
		return Err(no_merge_map("VarStore"));
	}
	if tables.iter().any(|t| {
		t.glyph_class_def2.is_some()
			|| t.attach_list2.is_some()
			|| t.lig_caret_list2.is_some()
			|| t.mark_attach_class_def2.is_some()
			|| t.mark_glyph_sets_def2.is_some()
	}) {
		// The tables are made compact before merging.
		return Err(Error::Type(
			"extended GDEF subtables are not supported here".into(),
		));
	}
	Ok(ot::GDEF {
		version: tables.iter().map(|t| t.version).max().unwrap_or(0),
		glyph_class_def: merge_opt(tables.iter().map(|t| &t.glyph_class_def), merge_class_def)?,
		attach_list: merge_opt(tables.iter().map(|t| &t.attach_list), |l| {
			Ok(ot::AttachList {
				coverage: merge_opt_coverage(l.iter().map(|t| &t.coverage))?,
				glyph_count: sum_counts(l.iter().map(|t| (t.glyph_count, t.attach_point.len()))),
				attach_point: concat(&l.iter().map(|t| &t.attach_point[..]).collect::<Vec<_>>()),
			})
		})?,
		lig_caret_list: merge_opt(tables.iter().map(|t| &t.lig_caret_list), |l| {
			Ok(ot::LigCaretList {
				coverage: merge_opt_coverage(l.iter().map(|t| &t.coverage))?,
				lig_glyph_count: sum_counts(
					l.iter().map(|t| (t.lig_glyph_count, t.lig_glyph.len())),
				),
				lig_glyph: concat(&l.iter().map(|t| &t.lig_glyph[..]).collect::<Vec<_>>()),
			})
		})?,
		mark_attach_class_def: merge_opt(
			tables.iter().map(|t| &t.mark_attach_class_def),
			merge_class_def,
		)?,
		// XXX Renumber MarkFilterSets of lookups (fontTools' note)
		mark_glyph_sets_def: merge_opt(tables.iter().map(|t| &t.mark_glyph_sets_def), |l| {
			Ok(ot::MarkGlyphSetsDef {
				mark_set_table_format: equal(
					&l.iter()
						.map(|t| t.mark_set_table_format)
						.collect::<Vec<_>>(),
				)?,
				mark_set_count: sum_counts(l.iter().map(|t| (t.mark_set_count, t.coverage.len()))),
				coverage: concat(&l.iter().map(|t| &t.coverage[..]).collect::<Vec<_>>()),
			})
		})?,
		..Default::default()
	})
}

/// `MATH.mergeMap`.
pub(crate) fn merge_math(tables: &[&ot::MATH]) -> Result<ot::MATH> {
	Ok(ot::MATH {
		version: tables.iter().map(|t| t.version).max().unwrap_or(0),
		// `MathConstants.mergeMap`: everything from the first.
		math_constants: merge_opt(tables.iter().map(|t| &t.math_constants), |l| {
			Ok(l[0].clone())
		})?,
		math_glyph_info: merge_opt(tables.iter().map(|t| &t.math_glyph_info), |l| {
			Ok(ot::MathGlyphInfo {
				math_italics_correction_info: merge_opt(
					l.iter().map(|t| &t.math_italics_correction_info),
					|l| {
						Ok(ot::MathItalicsCorrectionInfo {
							coverage: merge_opt_coverage(l.iter().map(|t| &t.coverage))?,
							italics_correction_count: sum_counts(
								l.iter().map(|t| {
									(t.italics_correction_count, t.italics_correction.len())
								}),
							),
							italics_correction: concat(
								&l.iter()
									.map(|t| &t.italics_correction[..])
									.collect::<Vec<_>>(),
							),
						})
					},
				)?,
				math_top_accent_attachment: merge_opt(
					l.iter().map(|t| &t.math_top_accent_attachment),
					|l| {
						Ok(ot::MathTopAccentAttachment {
							top_accent_coverage: merge_opt_coverage(
								l.iter().map(|t| &t.top_accent_coverage),
							)?,
							top_accent_attachment_count: sum_counts(l.iter().map(|t| {
								(t.top_accent_attachment_count, t.top_accent_attachment.len())
							})),
							top_accent_attachment: concat(
								&l.iter()
									.map(|t| &t.top_accent_attachment[..])
									.collect::<Vec<_>>(),
							),
						})
					},
				)?,
				extended_shape_coverage: merge_opt_coverage(
					l.iter().map(|t| &t.extended_shape_coverage),
				)?,
				math_kern_info: merge_opt(l.iter().map(|t| &t.math_kern_info), |l| {
					Ok(ot::MathKernInfo {
						math_kern_coverage: merge_opt_coverage(
							l.iter().map(|t| &t.math_kern_coverage),
						)?,
						math_kern_count: sum_counts(
							l.iter()
								.map(|t| (t.math_kern_count, t.math_kern_info_records.len())),
						),
						math_kern_info_records: concat(
							&l.iter()
								.map(|t| &t.math_kern_info_records[..])
								.collect::<Vec<_>>(),
						),
					})
				})?,
			})
		})?,
		math_variants: merge_opt(tables.iter().map(|t| &t.math_variants), |l| {
			Ok(ot::MathVariants {
				min_connector_overlap: l[0].min_connector_overlap,
				vert_glyph_coverage: merge_opt_coverage(l.iter().map(|t| &t.vert_glyph_coverage))?,
				horiz_glyph_coverage: merge_opt_coverage(
					l.iter().map(|t| &t.horiz_glyph_coverage),
				)?,
				vert_glyph_count: sum_counts(
					l.iter()
						.map(|t| (t.vert_glyph_count, t.vert_glyph_construction.len())),
				),
				horiz_glyph_count: sum_counts(
					l.iter()
						.map(|t| (t.horiz_glyph_count, t.horiz_glyph_construction.len())),
				),
				vert_glyph_construction: concat(
					&l.iter()
						.map(|t| &t.vert_glyph_construction[..])
						.collect::<Vec<_>>(),
				),
				horiz_glyph_construction: concat(
					&l.iter()
						.map(|t| &t.horiz_glyph_construction[..])
						.collect::<Vec<_>>(),
				),
			})
		})?,
	})
}

/// `BASE.mergeMap` ("XXX Fix BASE table merging", says fontTools).
pub(crate) fn merge_base(tables: &[&ot::BASE]) -> Result<ot::BASE> {
	if tables.iter().any(|t| t.var_store.is_some()) {
		return Err(no_merge_map("VarStore"));
	}
	// `Axis.mergeMap`.
	let axis = |l: Vec<&ot::Axis>| -> Result<ot::Axis> {
		Ok(ot::Axis {
			base_tag_list: merge_opt(l.iter().map(|t| &t.base_tag_list), |l| {
				Ok(ot::BaseTagList {
					base_tag_count: sum_counts(
						l.iter().map(|t| (t.base_tag_count, t.baseline_tag.len())),
					),
					baseline_tag: concat(
						&l.iter().map(|t| &t.baseline_tag[..]).collect::<Vec<_>>(),
					),
				})
			})?,
			base_script_list: merge_opt(l.iter().map(|t| &t.base_script_list), |l| {
				// TODO (fontTools): Merge duplicate entries.
				let mut records = concat(
					&l.iter()
						.map(|t| &t.base_script_record[..])
						.collect::<Vec<_>>(),
				);
				records.sort_by_key(|r| r.base_script_tag);
				Ok(ot::BaseScriptList {
					base_script_count: None,
					base_script_record: records,
				})
			})?,
		})
	};
	Ok(ot::BASE {
		version: tables.iter().map(|t| t.version).max().unwrap_or(0),
		horiz_axis: merge_opt(tables.iter().map(|t| &t.horiz_axis), axis)?,
		vert_axis: merge_opt(tables.iter().map(|t| &t.vert_axis), axis)?,
		var_store: None,
	})
}

/// `JSTF.mergeMap`: fontTools fails on any `JSTF` table, as `mergeObjects`
/// does not know how to merge its script count.
pub(crate) fn merge_jstf(tables: &[&ot::JSTF]) -> Result<Option<ot::JSTF>> {
	if tables.is_empty() {
		return Ok(None);
	}
	Err(no_merge_map("int"))
}

// GSUB and GPOS.

/// The lookups of a lookup list and its `LookupCount`.
pub(crate) type Lookups = (Vec<Option<Box<ot::Lookup>>>, Option<u32>);

/// The parts of a `GSUB` or `GPOS` table that merging looks at.
#[derive(Debug, Default)]
pub(crate) struct LayoutParts {
	pub version: u32,
	pub script_list: Option<Box<ot::ScriptList>>,
	pub feature_list: Option<Box<ot::FeatureList>>,
	/// The lookups and `LookupCount`.
	pub lookup_list: Option<Lookups>,
	pub feature_variations: Option<Box<ot::FeatureVariations>>,
}

/// `GSUB` and `GPOS`, which have the same fields.
pub(crate) trait LayoutTable: Default {
	/// Takes the parts out of the table (`_get_layout_lists`).
	fn take_parts(&mut self) -> LayoutParts;
	/// A table of the parts.
	fn from_parts(parts: LayoutParts) -> Self;
}

macro_rules! layout_table {
	($t:ty) => {
		impl LayoutTable for $t {
			fn take_parts(&mut self) -> LayoutParts {
				let lookup_list = match (self.lookup_list2.take(), self.lookup_list.take()) {
					(Some(l), _) => Some((l.lookup, l.lookup_count)),
					(None, Some(l)) => Some((l.lookup, l.lookup_count)),
					(None, None) => None,
				};
				let script_list2 = self.script_list2.take();
				let feature_list2 = self.feature_list2.take();
				LayoutParts {
					version: self.version,
					script_list: script_list2.or(self.script_list.take()),
					feature_list: feature_list2.or(self.feature_list.take()),
					lookup_list,
					feature_variations: self.feature_variations.take(),
				}
			}

			fn from_parts(parts: LayoutParts) -> Self {
				Self {
					version: parts.version,
					script_list: parts.script_list,
					feature_list: parts.feature_list,
					lookup_list: parts.lookup_list.map(|(lookup, lookup_count)| {
						Box::new(ot::LookupList {
							lookup_count,
							lookup,
						})
					}),
					feature_variations: parts.feature_variations,
					..Default::default()
				}
			}
		}
	};
}

layout_table!(ot::GSUB);
layout_table!(ot::GPOS);

/// The mark filtering set of a lookup.
#[derive(Debug, Clone, Copy)]
enum MarkSet {
	/// The lookup does not use one.
	None,
	/// A set of the lookup's font, by reference (fontTools maps the index to
	/// the `Coverage`).
	Mapped(usize),
	/// An index left as it was, because the font's `GDEF` has no sets.
	Unmapped(u16),
}

#[derive(Debug)]
struct LookupEntry {
	lookup: Option<Box<ot::Lookup>>,
	font: usize,
	/// The lookups that context subtables refer to.
	refs: Vec<usize>,
	mark_set: MarkSet,
}

#[derive(Debug, Clone)]
struct FeatureData {
	params: Option<Box<ot::FeatureParamsAny>>,
	lookups: Vec<usize>,
}

#[derive(Debug, Clone)]
struct FeatureEntry {
	tag: Tag,
	feature: Option<FeatureData>,
}

#[derive(Debug, Clone)]
struct LangSysData {
	lookup_order: Option<Box<ot::LookupOrder>>,
	req_feature: Option<usize>,
	features: Vec<usize>,
}

#[derive(Debug, Clone)]
struct ScriptData {
	default: Option<LangSysData>,
	langs: Vec<(Tag, Option<LangSysData>)>,
}

/// The `GSUB` or `GPOS` table of a font, with references.
#[derive(Debug, Default)]
struct FontLayout {
	version: u32,
	scripts: Option<Vec<(Tag, Option<ScriptData>)>>,
	/// The feature records and `FeatureCount`.
	features: Option<(Vec<usize>, Option<u32>)>,
	/// The lookups and `LookupCount`.
	lookups: Option<(Vec<usize>, Option<u32>)>,
	feature_variations: Option<Box<ot::FeatureVariations>>,
}

#[derive(Debug, Default)]
struct Arena {
	lookups: Vec<LookupEntry>,
	features: Vec<FeatureEntry>,
}

/// Calls `f` on the lookup indices of the context subtables of a lookup
/// (`mapLookups`).
fn for_each_lookup_ref(
	lookup: &mut ot::Lookup,
	f: &mut dyn FnMut(&mut u16) -> Result<()>,
) -> Result<()> {
	let mut error = None;
	for st in lookup.sub_table.iter_mut().flatten() {
		st.visit_mut(&mut |any| {
			let index = if let Some(r) = any.downcast_mut::<ot::SubstLookupRecord>() {
				&mut r.lookup_list_index
			} else if let Some(r) = any.downcast_mut::<ot::PosLookupRecord>() {
				&mut r.lookup_list_index
			} else {
				return Ok(());
			};
			if error.is_none()
				&& let Err(e) = f(index)
			{
				error = Some(e);
			}
			Ok(())
		})?;
	}
	error.map_or(Ok(()), Err)
}

impl Arena {
	/// `layoutPreMerge` for a table of font `font`: indices become references.
	fn add_font(
		&mut self,
		font: usize,
		parts: LayoutParts,
		mark_sets: Option<(usize, usize)>,
	) -> Result<FontLayout> {
		let mut layout = FontLayout {
			version: parts.version,
			feature_variations: parts.feature_variations,
			..Default::default()
		};
		if let Some((lookups, count)) = parts.lookup_list {
			let first = self.lookups.len();
			let ids: Vec<usize> = (first..first + lookups.len()).collect();
			for mut lookup in lookups {
				let mut refs = Vec::new();
				let mut mark_set = MarkSet::None;
				if let Some(l) = lookup.as_deref_mut() {
					for_each_lookup_ref(l, &mut |i| {
						refs.push(*ids.get(*i as usize).ok_or_else(|| key_error(*i))?);
						Ok(())
					})?;
					if l.lookup_flag & 0x0010 != 0 {
						mark_set = match mark_sets {
							Some((_, count)) => {
								let i = l.mark_filtering_set as usize;
								if i >= count {
									return Err(key_error(i));
								}
								MarkSet::Mapped(i)
							}
							None => MarkSet::Unmapped(l.mark_filtering_set),
						};
					}
				}
				self.lookups.push(LookupEntry {
					lookup,
					font,
					refs,
					mark_set,
				});
			}
			layout.lookups = Some((ids, count));
		}
		let lookup_ids = layout.lookups.as_ref().map(|l| &l.0[..]);
		if let Some(list) = parts.feature_list {
			let mut ids = Vec::with_capacity(list.feature_record.len());
			for record in list.feature_record {
				let feature = match record.feature {
					Some(f) => {
						let lookups = f
							.lookup_list_index
							.iter()
							.map(|&i| {
								lookup_ids
									.and_then(|ids| ids.get(i as usize))
									.copied()
									.ok_or_else(|| key_error(i))
							})
							.collect::<Result<_>>()?;
						Some(FeatureData {
							params: f.feature_params,
							lookups,
						})
					}
					None => None,
				};
				ids.push(self.features.len());
				self.features.push(FeatureEntry {
					tag: record.feature_tag,
					feature,
				});
			}
			layout.features = Some((ids, list.feature_count));
		}
		let feature_ids = layout.features.as_ref().map(|l| &l.0[..]);
		if let Some(list) = parts.script_list {
			let map = |i: u16| -> Result<usize> {
				feature_ids
					.and_then(|ids| ids.get(i as usize))
					.copied()
					.ok_or_else(|| key_error(i))
			};
			let lang_sys = |l: ot::LangSys| -> Result<LangSysData> {
				Ok(LangSysData {
					lookup_order: l.lookup_order,
					req_feature: match l.req_feature_index {
						0xFFFF => None,
						i => Some(map(i)?),
					},
					features: l
						.feature_index
						.into_iter()
						.map(map)
						.collect::<Result<_>>()?,
				})
			};
			let mut scripts = Vec::with_capacity(list.script_record.len());
			for record in list.script_record {
				let script = match record.script {
					Some(s) => Some(ScriptData {
						default: s.default_lang_sys.map(|l| lang_sys(*l)).transpose()?,
						langs: s
							.lang_sys_record
							.into_iter()
							.map(|r| {
								Ok((
									r.lang_sys_tag,
									r.lang_sys.map(|l| lang_sys(*l)).transpose()?,
								))
							})
							.collect::<Result<_>>()?,
					}),
					None => None,
				};
				scripts.push((record.script_tag, script));
			}
			layout.scripts = Some(scripts);
		}
		Ok(layout)
	}

	fn feature_data(&mut self, id: usize) -> Result<&mut FeatureData> {
		self.features[id]
			.feature
			.as_mut()
			.ok_or_else(|| attribute_error("LookupListIndex"))
	}

	/// `mergeFeatures`.
	fn merge_features(&mut self, tag: Tag, features: &[usize]) -> Result<usize> {
		let mut lookups = Vec::new();
		for &f in features {
			lookups.extend(self.feature_data(f)?.lookups.iter().copied());
		}
		self.features.push(FeatureEntry {
			tag,
			feature: Some(FeatureData {
				params: None,
				lookups,
			}),
		});
		Ok(self.features.len() - 1)
	}

	/// `mergeFeatureLists`: new features, merging those of the same tag.
	fn merge_feature_lists(&mut self, lists: &[&[usize]]) -> Result<Vec<usize>> {
		let mut d: BTreeMap<Tag, Vec<usize>> = BTreeMap::new();
		for l in lists {
			for &f in *l {
				d.entry(self.features[f].tag).or_default().push(f);
			}
		}
		d.into_iter()
			.map(|(tag, features)| self.merge_features(tag, &features))
			.collect()
	}

	/// `mergeLangSyses`.
	fn merge_lang_syses(&mut self, list: &[Option<&LangSysData>]) -> Result<LangSysData> {
		let list = list
			.iter()
			.map(|l| l.ok_or_else(|| attribute_error("ReqFeatureIndex")))
			.collect::<Result<Vec<_>>>()?;
		// TODO (fontTools): Support merging ReqFeatureIndex.
		if list.iter().any(|l| l.req_feature.is_some()) {
			return Err(Error::Assertion(
				"all(l.ReqFeatureIndex == 0xFFFF for l in lst)".into(),
			));
		}
		let lists: Vec<&[usize]> = list
			.iter()
			.map(|l| &l.features[..])
			.filter(|f| !f.is_empty())
			.collect();
		Ok(LangSysData {
			lookup_order: None,
			req_feature: None,
			features: self.merge_feature_lists(&lists)?,
		})
	}

	/// `mergeScripts`.
	fn merge_scripts(&mut self, list: Vec<Option<ScriptData>>) -> Result<Option<ScriptData>> {
		if list.len() == 1 {
			return Ok(list.into_iter().next().flatten());
		}
		let list = list
			.into_iter()
			.map(|s| s.ok_or_else(|| attribute_error("LangSysRecord")))
			.collect::<Result<Vec<_>>>()?;
		let mut lang_syses: BTreeMap<Tag, Vec<Option<&LangSysData>>> = BTreeMap::new();
		for script in &list {
			for (tag, lang_sys) in &script.langs {
				lang_syses.entry(*tag).or_default().push(lang_sys.as_ref());
			}
		}
		let mut langs = Vec::with_capacity(lang_syses.len());
		for (tag, lang_sys_list) in lang_syses {
			langs.push((tag, Some(self.merge_lang_syses(&lang_sys_list)?)));
		}
		let defaults: Vec<Option<&LangSysData>> = list
			.iter()
			.filter_map(|s| s.default.as_ref())
			.map(Some)
			.collect();
		let default = if defaults.is_empty() {
			None
		} else {
			Some(self.merge_lang_syses(&defaults)?)
		};
		Ok(Some(ScriptData { default, langs }))
	}
}

/// The lang systems of a script, the default one first.
fn lang_syses(script: &ScriptData) -> impl Iterator<Item = &LangSysData> {
	script
		.default
		.iter()
		.chain(script.langs.iter().filter_map(|(_, l)| l.as_ref()))
}

impl Merger {
	/// Merges `GSUB` or `GPOS` (`table_G_S_U_B_.merge`, `DefaultTable.merge` and
	/// `layoutPostMerge`). For `GSUB`, the glyphs in
	/// [`Merger::duplicate_glyphs_per_font`] are first substituted in a `locl`
	/// lookup of their font.
	pub(crate) fn merge_layout<T: LayoutTable>(
		&mut self,
		tag: Tag,
		tables: Vec<Option<&mut T>>,
	) -> Result<Option<T>> {
		let mut arena = Arena::default();
		let mut layouts: Vec<Option<FontLayout>> = Vec::with_capacity(tables.len());
		for (font, table) in tables.into_iter().enumerate() {
			let Some(table) = table else {
				layouts.push(None);
				continue;
			};
			let mark_sets = self.mark_sets.bases.get(font).copied().flatten();
			layouts.push(Some(arena.add_font(font, table.take_parts(), mark_sets)?));
		}
		if layouts.iter().all(Option::is_none) {
			return Ok(None);
		}
		if tag == *b"GSUB" {
			self.add_locl(&mut arena, &mut layouts)?;
		}
		let layouts: Vec<FontLayout> = layouts.into_iter().flatten().collect();
		if layouts.iter().any(|l| l.feature_variations.is_some()) {
			return Err(no_merge_map("FeatureVariations"));
		}

		// DefaultTable.merge.
		let version = layouts.iter().map(|l| l.version).max().unwrap_or(0);
		let mut scripts = None;
		let mut features: Option<(Vec<usize>, Option<u32>)> = None;
		let mut lookups: Option<(Vec<usize>, Option<u32>)> = None;
		let mut script_lists = Vec::new();
		for l in layouts {
			if let Some(s) = l.scripts {
				script_lists.push(s);
			}
			if let Some((ids, count)) = l.features {
				let f = features.get_or_insert((Vec::new(), Some(0)));
				f.0.extend(ids);
				f.1 = sum_counts([(f.1, 0), (count, 0)]);
			}
			if let Some((ids, count)) = l.lookups {
				let f = lookups.get_or_insert((Vec::new(), Some(0)));
				f.0.extend(ids);
				f.1 = sum_counts([(f.1, 0), (count, 0)]);
			}
		}
		if let Some((ids, _)) = &mut features {
			ids.sort_by_key(|&f| arena.features[f].tag);
		}
		if !script_lists.is_empty() {
			// mergeScriptRecords.
			let mut d: BTreeMap<Tag, Vec<Option<ScriptData>>> = BTreeMap::new();
			for list in script_lists {
				for (tag, script) in list {
					d.entry(tag).or_default().push(script);
				}
			}
			let mut records = Vec::with_capacity(d.len());
			for (tag, list) in d {
				records.push((tag, arena.merge_scripts(list)?));
			}
			scripts = Some(records);
		}

		// layoutPostMerge: references back to indices.
		let mut feature_index: HashMap<usize, u16> = HashMap::new();
		if let (Some((list, count)), Some(scripts)) = (&mut features, &scripts) {
			// Collect unregistered (new) features.
			let mut seen: HashSet<usize> = list.iter().copied().collect();
			for script in scripts.iter().filter_map(|(_, s)| s.as_ref()) {
				for lang_sys in lang_syses(script) {
					for &f in lang_sys.features.iter().chain(&lang_sys.req_feature) {
						if seen.insert(f) {
							list.push(f);
						}
					}
				}
			}
			// Record used features, and remove the unused ones.
			let mut used: HashSet<usize> = HashSet::new();
			for script in scripts.iter().filter_map(|(_, s)| s.as_ref()) {
				for lang_sys in lang_syses(script) {
					used.extend(lang_sys.features.iter().chain(&lang_sys.req_feature));
				}
			}
			list.retain(|f| used.contains(f));
			for (i, &f) in list.iter().enumerate() {
				feature_index.insert(f, index_u16(i)?);
			}
			*count = Some(list.len() as u32);
		}

		let mut lookup_index: HashMap<usize, u16> = HashMap::new();
		if let Some((list, count)) = &mut lookups {
			let Some((feature_list, _)) = &features else {
				return Err(attribute_error("mapLookups"));
			};
			let feature_lookups: Vec<usize> = feature_list
				.iter()
				.filter_map(|&f| arena.features[f].feature.as_ref())
				.flat_map(|f| f.lookups.iter().copied())
				.collect();
			// Collect unregistered (new) lookups.
			let mut seen: HashSet<usize> = list.iter().copied().collect();
			for &l in &feature_lookups {
				if seen.insert(l) {
					list.push(l);
				}
			}
			let mut i = 0;
			while i < list.len() {
				for &r in &arena.lookups[list[i]].refs {
					if seen.insert(r) {
						list.push(r);
					}
				}
				i += 1;
			}
			// Record used lookups, and remove the unused ones.
			let mut used: HashSet<usize> = feature_lookups.into_iter().collect();
			for &l in list.iter() {
				if arena.lookups[l].lookup.is_some() {
					used.extend(arena.lookups[l].refs.iter().copied());
				}
			}
			list.retain(|l| used.contains(l));
			for (i, &l) in list.iter().enumerate() {
				lookup_index.insert(l, index_u16(i)?);
			}
			*count = Some(list.len() as u32);
		}

		// The tables, with indices.
		let lookup_list = match lookups {
			Some((list, count)) => {
				let mut out = Vec::with_capacity(list.len());
				for id in list {
					out.push(self.lookup_with_indices(&mut arena, id, &lookup_index)?);
				}
				Some((out, count))
			}
			None => None,
		};
		let feature_list = match features {
			Some((list, count)) => {
				let mut records = Vec::with_capacity(list.len());
				for id in list {
					let entry = &arena.features[id];
					records.push(ot::FeatureRecord {
						feature_tag: entry.tag,
						feature: match &entry.feature {
							Some(f) => Some(Box::new(ot::Feature {
								feature_params: f.params.clone(),
								lookup_count: Some(f.lookups.len() as u32),
								lookup_list_index: f
									.lookups
									.iter()
									.map(|l| {
										lookup_index.get(l).copied().ok_or_else(|| key_error(l))
									})
									.collect::<Result<_>>()?,
							})),
							None => None,
						},
					});
				}
				Some(Box::new(ot::FeatureList {
					feature_count: count,
					feature_record: records,
				}))
			}
			None => None,
		};
		let lang_sys = |l: &LangSysData| -> Result<Box<ot::LangSys>> {
			let map = |f: &usize| feature_index.get(f).copied().ok_or_else(|| key_error(f));
			Ok(Box::new(ot::LangSys {
				lookup_order: l.lookup_order.clone(),
				req_feature_index: match &l.req_feature {
					Some(f) => map(f)?,
					None => 0xFFFF,
				},
				feature_count: Some(l.features.len() as u32),
				feature_index: l.features.iter().map(map).collect::<Result<_>>()?,
			}))
		};
		let script_list = match scripts {
			Some(records) => {
				let mut out = Vec::with_capacity(records.len());
				for (tag, script) in records {
					out.push(ot::ScriptRecord {
						script_tag: tag,
						script: match script {
							Some(s) => Some(Box::new(ot::Script {
								default_lang_sys: s.default.as_ref().map(lang_sys).transpose()?,
								lang_sys_count: Some(s.langs.len() as u32),
								lang_sys_record: s
									.langs
									.iter()
									.map(|(tag, l)| {
										Ok(ot::LangSysRecord {
											lang_sys_tag: *tag,
											lang_sys: l.as_ref().map(lang_sys).transpose()?,
										})
									})
									.collect::<Result<_>>()?,
							})),
							None => None,
						},
					});
				}
				Some(Box::new(ot::ScriptList {
					script_count: Some(out.len() as u32),
					script_record: out,
				}))
			}
			None => None,
		};
		Ok(Some(T::from_parts(LayoutParts {
			version,
			script_list,
			feature_list,
			lookup_list,
			feature_variations: None,
		})))
	}

	/// A lookup of the merged table, with the indices of the lookups its context
	/// subtables refer to, and of its mark filtering set.
	fn lookup_with_indices(
		&self,
		arena: &mut Arena,
		id: usize,
		lookup_index: &HashMap<usize, u16>,
	) -> Result<Option<Box<ot::Lookup>>> {
		let entry = &mut arena.lookups[id];
		let Some(mut lookup) = entry.lookup.take() else {
			return Ok(None);
		};
		let mut refs = entry.refs.iter();
		for_each_lookup_ref(&mut lookup, &mut |i| {
			let r = refs.next().ok_or_else(|| key_error(*i))?;
			*i = lookup_index.get(r).copied().ok_or_else(|| key_error(r))?;
			Ok(())
		})?;
		if lookup.lookup_flag & 0x0010 != 0 {
			match (self.mark_sets.merged, entry.mark_set) {
				(true, MarkSet::Mapped(i)) => {
					let (base, _) = self.mark_sets.bases[entry.font].ok_or_else(|| key_error(i))?;
					lookup.mark_filtering_set = index_u16(base + i)?;
				}
				(true, MarkSet::Unmapped(i)) => return Err(key_error(i)),
				// A `Coverage` cannot be written as an index.
				(false, MarkSet::Mapped(_)) => {
					return Err(Error::Type(
						"struct.error: required argument is not an integer".into(),
					));
				}
				_ => {}
			}
		}
		Ok(Some(lookup))
	}

	/// The `locl` part of `table_G_S_U_B_.merge`: substitutes the duplicate
	/// glyphs of each font in a new lookup of its `locl` features, adding one to
	/// the lang systems that have none.
	fn add_locl(&self, arena: &mut Arena, layouts: &mut [Option<FontLayout>]) -> Result<()> {
		if layouts.len() != self.duplicate_glyphs_per_font.len() {
			return Err(Error::Assertion(
				"len(tables) == len(m.duplicateGlyphsPerFont)".into(),
			));
		}
		let locl = Tag::new(b"locl");
		for (i, (layout, dups)) in layouts
			.iter_mut()
			.zip(&self.duplicate_glyphs_per_font)
			.enumerate()
		{
			if dups.is_empty() {
				continue;
			}
			let Some(layout) = layout else {
				let order = self.glyph_order.as_deref().unwrap_or_default();
				let name = |g: &u32| {
					order
						.get(*g as usize)
						.cloned()
						.unwrap_or_else(|| format!("glyph{g:05}"))
				};
				let dups: Vec<String> = dups
					.iter()
					.map(|(a, b)| format!("'{}': '{}'", name(a), name(b)))
					.collect();
				log::warn!(
					"Have non-identical duplicates to resolve for '{}' but no GSUB. Are duplicates intended?: {{{}}}",
					self.font_names
						.get(i)
						.cloned()
						.flatten()
						.unwrap_or_else(|| "None".into()),
					dups.join(", ")
				);
				continue;
			};
			let FontLayout {
				scripts,
				features,
				lookups,
				..
			} = layout;
			let scripts = scripts
				.as_mut()
				.ok_or_else(|| attribute_error("ScriptRecord"))?;
			let mut synth_feature = None;
			let mut synth_lookup = None;
			for (script_tag, script) in scripts.iter_mut() {
				if *script_tag == *b"DFLT" {
					continue; // XXX
				}
				let script = script
					.as_mut()
					.ok_or_else(|| attribute_error("DefaultLangSys"))?;
				let lang_syses = script
					.default
					.iter_mut()
					.chain(script.langs.iter_mut().filter_map(|(_, l)| l.as_mut()));
				for lang_sys in lang_syses {
					let found: Vec<usize> = lang_sys
						.features
						.iter()
						.copied()
						.filter(|&f| arena.features[f].tag == locl)
						.collect();
					if found.len() > 1 {
						return Err(Error::Assertion("len(feature) <= 1".into()));
					}
					let feature = match found.first() {
						Some(&f) => f,
						None => {
							let f = match synth_feature {
								Some(f) => f,
								None => {
									let (list, count) = features
										.as_mut()
										.ok_or_else(|| attribute_error("FeatureRecord"))?;
									let f = arena.features.len();
									arena.features.push(FeatureEntry {
										tag: locl,
										feature: Some(FeatureData {
											params: None,
											lookups: Vec::new(),
										}),
									});
									list.push(f);
									*count = count.map(|c| c + 1);
									synth_feature = Some(f);
									f
								}
							};
							lang_sys.features.push(f);
							lang_sys.features.sort_by_key(|&f| arena.features[f].tag);
							f
						}
					};
					let lookup = match synth_lookup {
						Some(l) => l,
						None => {
							let subtable = ot::SingleSubst {
								mapping: dups.clone(),
								dont_share: false,
							};
							let lookup = ot::Lookup {
								lookup_type: 1,
								lookup_flag: 0,
								sub_table_count: Some(1),
								sub_table: vec![Some(Box::new(ot::LookupSubTable::SingleSubst(
									subtable,
								)))],
								mark_filtering_set: 0,
							};
							let l = arena.lookups.len();
							arena.lookups.push(LookupEntry {
								lookup: Some(Box::new(lookup)),
								font: i,
								refs: Vec::new(),
								mark_set: MarkSet::None,
							});
							// mtiLib uses None as default value for LookupList, while
							// feaLib points to an empty array with count 0.
							let (list, count) = lookups.get_or_insert((Vec::new(), Some(0)));
							list.push(l);
							*count = count.map(|c| c + 1);
							synth_lookup = Some(l);
							l
						}
					};
					let data = arena.feature_data(feature)?;
					if data.lookups.first() != Some(&lookup) {
						data.lookups.insert(0, lookup);
					}
				}
			}
		}
		Ok(())
	}
}

fn index_u16(i: usize) -> Result<u16> {
	u16::try_from(i)
		.map_err(|_| Error::Type("struct.error: 'H' format requires 0 <= number <= 65535".into()))
}
