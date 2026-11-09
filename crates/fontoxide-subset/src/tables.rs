//! The per-table methods of `fontTools.subset` (`prune_pre_subset`,
//! `closure_glyphs`, `subset_glyphs`, `prune_post_subset`), dispatched on the
//! table tag. A method returns `None` when fontTools' table class does not have
//! it.

use std::collections::BTreeSet;

use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::name::{NameRecord, NameString};
use fontoxide_ttlib::tables::{
	Cmap, Cpal, Fvar, Gpos, Gsub, Gvar, Hdmx, Head, Hmtx, Maxp, Name, Os2, Post, Stat, Vmtx, Vorg,
};
use fontoxide_ttlib::{Font, Tag};

use crate::options::{has_int, has_str};
use crate::{Error, Result, Subsetter, cmap, glyf, layout, strip};

fn unsupported<T>(tag: Tag) -> Result<T> {
	Err(Error::Unsupported(strip(tag)))
}

/// `prune_pre_subset`.
pub(crate) fn prune_pre_subset(s: &mut Subsetter, font: &mut Font, tag: Tag) -> Result<Option<bool>> {
	Ok(Some(match &tag.0 {
		b"cmap" => cmap::prune_pre_subset(s, font)?,
		b"glyf" => glyf::prune_pre_subset(s, font)?,
		b"gvar" => {
			if s.options.notdef_glyph && !s.options.notdef_outline {
				let gvar = font.require::<Gvar>()?;
				match gvar.variations.iter_mut().find(|(g, _)| *g == 0) {
					Some((_, v)) => v.clear(),
					None => {
						gvar.variations.insert(0, (0, Vec::new()));
					}
				}
			}
			true
		}
		b"maxp" => {
			let maxp = font.require::<Maxp>()?;
			if !s.options.hinting && maxp.table_version == 0x0001_0000 {
				let v1 = &mut maxp.v1;
				v1.max_zones = 1;
				v1.max_twilight_points = 0;
				v1.max_storage = 0;
				v1.max_function_defs = 0;
				v1.max_instruction_defs = 0;
				v1.max_stack_elements = 0;
				v1.max_size_of_instructions = 0;
			}
			true
		}
		b"post" => {
			let post = font.require::<Post>()?;
			if !s.options.glyph_names {
				post.format_type = 3.0;
			}
			// A required table.
			true
		}
		b"GSUB" | b"GPOS" => layout::prune_pre_subset(s, font, tag)?,
		b"DMAP" | b"DSIG" | b"kern" | b"CFF " => return unsupported(tag),
		_ => return Ok(None),
	}))
}

/// `closure_glyphs`.
pub(crate) fn closure_glyphs(s: &mut Subsetter, font: &mut Font, tag: Tag) -> Result<()> {
	match &tag.0 {
		b"glyf" => glyf::closure_glyphs(s, font),
		b"GSUB" => layout::gsub_closure_glyphs(s, font),
		_ => unsupported(tag),
	}
}

/// `_dict_subset` for glyph-keyed maps.
fn retain_glyphs<V>(map: &mut std::collections::BTreeMap<u32, V>, glyphs: &BTreeSet<u32>) {
	map.retain(|g, _| glyphs.contains(g));
}

/// `subset_glyphs`.
pub(crate) fn subset_glyphs(s: &mut Subsetter, font: &mut Font, tag: Tag) -> Result<Option<bool>> {
	Ok(Some(match &tag.0 {
		b"cmap" => cmap::subset_glyphs(s, font)?,
		b"glyf" => glyf::subset_glyphs(s, font)?,
		b"hmtx" => {
			let t = font.require::<Hmtx>()?;
			retain_glyphs(&mut t.metrics, &s.glyphs);
			for &g in &s.glyphs_emptied {
				t.metrics.insert(g, (0, 0));
			}
			// A required table.
			true
		}
		b"vmtx" => {
			let t = font.require::<Vmtx>()?;
			retain_glyphs(&mut t.metrics, &s.glyphs);
			for &g in &s.glyphs_emptied {
				t.metrics.insert(g, (0, 0));
			}
			!t.metrics.is_empty()
		}
		b"hdmx" => {
			let t = font.require::<Hdmx>()?;
			for widths in t.hdmx.values_mut() {
				retain_glyphs(widths, &s.glyphs);
				for &g in &s.glyphs_emptied {
					widths.insert(g, 0);
				}
			}
			!t.hdmx.is_empty()
		}
		b"gvar" => {
			let t = font.require::<Gvar>()?;
			t.variations.retain(|(g, _)| s.glyphs.contains(g));
			!t.variations.is_empty()
		}
		b"VORG" => {
			let t = font.require::<Vorg>()?;
			retain_glyphs(&mut t.v_origin_records, &s.glyphs);
			t.num_vert_origin_y_metrics = t.v_origin_records.len() as u32;
			// Never dropped: it has default metrics.
			true
		}
		b"post" => {
			font.require::<Post>()?.extra_names = Some(Vec::new());
			// A required table.
			true
		}
		b"GSUB" | b"GPOS" => layout::subset_glyphs(s, font, tag)?,
		b"GDEF" => layout::gdef_subset_glyphs(s, font)?,
		b"EBLC" | b"EBDT" | b"CBLC" | b"CBDT" | b"sbix" | b"kern"
		| b"ankr" | b"bsln" | b"lcar" | b"HVAR" | b"VVAR" | b"opbd" | b"prop" | b"COLR"
		| b"MATH" | b"VARC" | b"DMAP" | b"CFF " | b"CFF2" | b"SVG " => return unsupported(tag),
		_ => return Ok(None),
	}))
}

/// `prune_post_subset`.
pub(crate) fn prune_post_subset(
	s: &mut Subsetter,
	font: &mut Font,
	tag: Tag,
) -> Result<Option<bool>> {
	Ok(Some(match &tag.0 {
		b"glyf" => glyf::prune_post_subset(s, font)?,
		b"name" => name_prune_post_subset(s, font)?,
		b"head" => {
			// Loaded so that it is compiled again with the recalculated values.
			font.require::<Head>()?;
			true
		}
		b"GSUB" | b"GPOS" => layout::prune_post_subset(s, font, tag)?,
		b"GDEF" => layout::gdef_prune_post_subset(font)?,
		b"CPAL" | b"VARC" | b"CFF " | b"CFF2" => return unsupported(tag),
		_ => return Ok(None),
	}))
}

/// The code points of the Unicode `cmap` subtables (`recalcUnicodeRanges`
/// reads `ttFont["cmap"]`).
fn cmap_unicodes(font: &mut Font) -> Result<BTreeSet<u32>> {
	let cmap = font
		.get::<Cmap>()?
		.ok_or_else(|| fontoxide_ttlib::Error::ttlib("KeyError: 'cmap'"))?;
	Ok(cmap
		.tables
		.iter()
		.filter(|t| t.is_unicode())
		.flat_map(|t| t.cmap.keys().copied())
		.collect())
}

/// The `OS/2` part of `_prune_post_subset`.
pub(crate) fn os2_prune(s: &Subsetter, font: &mut Font) -> Result<()> {
	if s.options.prune_unicode_ranges {
		let unicodes = cmap_unicodes(font)?;
		let os2 = font.require::<Os2>()?;
		let old = os2.unicode_ranges();
		let new = os2.recalc_unicode_ranges(&unicodes, true)?;
		if old != new {
			log::info!("OS/2 Unicode ranges pruned: {new:?}");
		}
	}
	if s.options.prune_codepage_ranges && font.require::<Os2>()?.version >= 1 {
		let unicodes = cmap_unicodes(font)?;
		let os2 = font.require::<Os2>()?;
		let old = os2.code_page_ranges();
		let new = os2.recalc_code_page_ranges(&unicodes, true)?;
		if old != new {
			log::info!("OS/2 CodePage ranges pruned: {new:?}");
		}
	}
	if s.options.recalc_average_width {
		let widths: Vec<u16> = match font.get::<Hmtx>()? {
			Some(h) => h.metrics.values().map(|m| m.0).collect(),
			None => Vec::new(),
		};
		let avg = font.require::<Os2>()?.recalc_avg_char_width(widths);
		log::info!("OS/2 xAvgCharWidth: {avg}");
	}
	if s.options.recalc_max_context {
		return Err(Error::Unsupported("recalc_max_context".into()));
	}
	Ok(())
}

/// `NAME_IDS_TO_OBFUSCATE`.
const NAME_IDS_TO_OBFUSCATE: [u16; 8] = [1, 2, 3, 4, 6, 16, 17, 18];

/// `NameRecordVisitor`: the name IDs used by `GSUB`, `GPOS`, `fvar`, `CPAL`
/// and `STAT` (which it loads).
fn name_ids_used(font: &mut Font) -> Result<BTreeSet<u16>> {
	let mut seen = BTreeSet::new();
	let features = |list: &Option<Box<ot::FeatureList>>,
	                    variations: &Option<Box<ot::FeatureVariations>>,
	                    seen: &mut BTreeSet<u16>| {
		let mut visit = |f: &Option<Box<ot::Feature>>| {
			let Some(params) = f.as_ref().and_then(|f| f.feature_params.as_deref()) else {
				return;
			};
			match params {
				ot::FeatureParamsAny::FeatureParamsSize(p) => {
					seen.insert(p.subfamily_name_id);
				}
				ot::FeatureParamsAny::FeatureParamsStylisticSet(p) => {
					seen.insert(p.ui_name_id);
				}
				ot::FeatureParamsAny::FeatureParamsCharacterVariants(p) => {
					seen.insert(p.feat_ui_label_name_id);
					seen.insert(p.feat_ui_tooltip_text_name_id);
					seen.insert(p.sample_text_name_id);
					if p.first_param_ui_label_name_id != 0 && p.num_named_parameters != 0 {
						let first = u32::from(p.first_param_ui_label_name_id);
						for id in first..first + u32::from(p.num_named_parameters) {
							seen.insert(id as u16);
						}
					}
				}
				ot::FeatureParamsAny::FeatureParams(_) => {}
			}
		};
		if let Some(list) = list {
			for r in &list.feature_record {
				visit(&r.feature);
			}
		}
		if let Some(v) = variations {
			for r in &v.feature_variation_record {
				if let Some(sub) = &r.feature_table_substitution {
					for r in &sub.substitution_record {
						visit(&r.feature);
					}
				}
			}
		}
	};
	if let Some(t) = font.get::<Gsub>()? {
		features(&t.feature_list, &t.feature_variations, &mut seen);
	}
	if let Some(t) = font.get::<Gpos>()? {
		features(&t.feature_list, &t.feature_variations, &mut seen);
	}
	if let Some(t) = font.get::<Fvar>()? {
		for inst in &t.instances {
			if inst.postscript_name_id != 0xFFFF {
				seen.insert(inst.postscript_name_id);
			}
			seen.insert(inst.subfamily_name_id);
		}
		for axis in &t.axes {
			seen.insert(axis.axis_name_id);
		}
	}
	if let Some(t) = font.get::<Cpal>()?
		&& t.version == 1
	{
		seen.extend(&t.palette_labels);
		seen.extend(&t.palette_entry_labels);
	}
	if let Some(t) = font.get::<Stat>()? {
		seen.insert(t.elided_fallback_name_id);
		if let Some(axes) = &t.design_axis_record {
			seen.extend(axes.axis.iter().map(|a| a.axis_name_id));
		}
		if let Some(values) = &t.axis_value_array {
			for v in values.axis_value.iter().flatten() {
				seen.insert(match &**v {
					ot::AxisValue::Format1(v) => v.value_name_id,
					ot::AxisValue::Format2(v) => v.value_name_id,
					ot::AxisValue::Format3(v) => v.value_name_id,
					ot::AxisValue::Format4(v) => v.value_name_id,
					ot::AxisValue::Unknown(_) => continue,
				});
			}
		}
	}
	Ok(seen)
}

/// `name.prune_post_subset`.
fn name_prune_post_subset(s: &Subsetter, font: &mut Font) -> Result<bool> {
	let seen = name_ids_used(font)?;
	let o = &s.options;
	let all = has_str(&o.name_ids, "*");
	let name = font.require::<Name>()?;
	name.names.retain(|n| {
		has_int(&o.name_ids, i64::from(n.name_id))
			|| seen.contains(&n.name_id)
			|| (all && n.name_id < 256)
	});
	if !o.name_legacy {
		name.names.retain(NameRecord::is_unicode);
	}
	if !has_str(&o.name_languages, "*") {
		name.names
			.retain(|n| has_int(&o.name_languages, i64::from(n.lang_id)));
	}
	if o.obfuscate_names {
		let need_remapping: BTreeSet<u16> = seen
			.iter()
			.copied()
			.filter(|id| NAME_IDS_TO_OBFUSCATE.contains(id))
			.collect();
		if !need_remapping.is_empty() {
			remap_select_name_ids(font, &need_remapping)?;
		}
		let name = font.require::<Name>()?;
		let mut records = Vec::new();
		for mut n in std::mem::take(&mut name.names) {
			let text = match n.name_id {
				1 | 4 => Some(".\u{7f}"),
				2 | 6 => Some("\u{7f}"),
				3 => {
					n.string = NameString::Text(String::new());
					None
				}
				16..=18 => continue,
				_ => None,
			};
			if let Some(text) = text {
				n.string = if n.is_unicode() {
					NameString::Bytes(text.encode_utf16().flat_map(u16::to_be_bytes).collect())
				} else {
					NameString::Text(text.to_owned())
				};
			}
			records.push(n);
		}
		name.names = records;
	}
	// A required table.
	Ok(true)
}

/// `_remap_select_name_ids`: copies the names that are about to be scrambled
/// to new IDs, and points `fvar` and `STAT` at the copies.
fn remap_select_name_ids(font: &mut Font, need_remapping: &BTreeSet<u16>) -> Result<()> {
	if !font.contains(Tag::new(b"fvar")) && !font.contains(Tag::new(b"STAT")) {
		return Ok(());
	}
	let name = font.require::<Name>()?;
	let existing: BTreeSet<u16> = name.names.iter().map(|r| r.name_id).collect();
	let mut remapping = std::collections::BTreeMap::new();
	// `_findUnusedNameID` skips gaps in the name IDs.
	let mut next_id = u32::from(name.find_unused_name_id(256)?) - 1;
	for &id in need_remapping {
		next_id += 1;
		if existing.contains(&(next_id as u16)) {
			return Err(fontoxide_ttlib::Error::Assertion(
				"_findUnusedNameID did not skip gaps".into(),
			)
			.into());
		}
		if next_id > 32767 {
			return Err(fontoxide_ttlib::Error::value(
				"Ran out of name IDs while trying to remap existing ones.",
			)
			.into());
		}
		remapping.insert(id, next_id as u16);
	}
	let copies: Vec<NameRecord> = name
		.names
		.iter()
		.filter_map(|r| {
			remapping.get(&r.name_id).map(|&id| {
				NameRecord::new(r.string.clone(), id, r.platform_id, r.plat_enc_id, r.lang_id)
			})
		})
		.collect();
	name.names.extend(copies);
	let remap = |id: u16| remapping.get(&id).copied().unwrap_or(id);
	if let Some(fvar) = font.get::<Fvar>()? {
		for axis in &mut fvar.axes {
			axis.axis_name_id = remap(axis.axis_name_id);
		}
		for inst in &mut fvar.instances {
			inst.subfamily_name_id = remap(inst.subfamily_name_id);
			inst.postscript_name_id = remap(inst.postscript_name_id);
		}
	}
	if let Some(stat) = font.get::<Stat>()? {
		stat.elided_fallback_name_id = remap(stat.elided_fallback_name_id);
		if let Some(axes) = &mut stat.design_axis_record {
			for a in &mut axes.axis {
				a.axis_name_id = remap(a.axis_name_id);
			}
		}
		if let Some(values) = &mut stat.axis_value_array {
			for v in values.axis_value.iter_mut().flatten() {
				let id = match &mut **v {
					ot::AxisValue::Format1(v) => &mut v.value_name_id,
					ot::AxisValue::Format2(v) => &mut v.value_name_id,
					ot::AxisValue::Format3(v) => &mut v.value_name_id,
					ot::AxisValue::Format4(v) => &mut v.value_name_id,
					ot::AxisValue::Unknown(_) => continue,
				};
				*id = remap(*id);
			}
		}
	}
	Ok(())
}
