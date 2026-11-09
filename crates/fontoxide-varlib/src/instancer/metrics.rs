//! Instancing of `MVAR`, `HVAR` and `VVAR`.

use std::collections::{BTreeMap, HashMap};

use fontoxide_misc::round_tools::ot_round;
use fontoxide_ttlib::binary::fit;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::var_store;
use fontoxide_ttlib::tables::{Hhea, Hmtx, Hvar, Mvar, Os2, Post, Vhea, Vmtx, Vorg, Vvar};
use fontoxide_ttlib::{Error, Font, Result, Tag};

use super::item_store::{DefaultDeltas, instantiate_item_variation_store};
use super::limits::{NormalizedLimits, fvar_axes};

const GLYF: Tag = Tag::new(b"glyf");
const HMTX: Tag = Tag::new(b"hmtx");
const HVAR: Tag = Tag::new(b"HVAR");
const MVAR: Tag = Tag::new(b"MVAR");
const VMTX: Tag = Tag::new(b"vmtx");
const VORG: Tag = Tag::new(b"VORG");
const VVAR: Tag = Tag::new(b"VVAR");

/// Adds a delta to a field that fits in `T`.
fn add<T: Copy + Into<i64> + TryFrom<i64>>(field: &mut T, delta: i64, name: &str) -> Result<()> {
	*field = fit((*field).into() + delta, name)?;
	Ok(())
}

/// `MVAR_ENTRIES` and `setMvarDeltas`, for one value. Returns false for tags
/// that are not known.
fn apply_mvar_delta(font: &mut Font, tag: &Tag, delta: i64) -> Result<()> {
	let no_attribute = |name: &str| {
		Error::ttlib(format!(
			"AttributeError: 'table_O_S_2f_2' object has no attribute '{name}'"
		))
	};
	match &tag.0 {
		b"hasc" | b"hdsc" | b"hlgp" | b"hcla" | b"hcld" | b"xhgt" | b"cpht" | b"sbxs" | b"sbys"
		| b"sbxo" | b"sbyo" | b"spxs" | b"spys" | b"spxo" | b"spyo" | b"strs" | b"stro" => {
			let os2 = font.require::<Os2>()?;
			match &tag.0 {
				b"hasc" => add(&mut os2.s_typo_ascender, delta, "sTypoAscender"),
				b"hdsc" => add(&mut os2.s_typo_descender, delta, "sTypoDescender"),
				b"hlgp" => add(&mut os2.s_typo_line_gap, delta, "sTypoLineGap"),
				b"hcla" => add(&mut os2.us_win_ascent, delta, "usWinAscent"),
				b"hcld" => add(&mut os2.us_win_descent, delta, "usWinDescent"),
				b"xhgt" if os2.version < 2 => Err(no_attribute("sxHeight")),
				b"xhgt" => add(&mut os2.sx_height, delta, "sxHeight"),
				b"cpht" if os2.version < 2 => Err(no_attribute("sCapHeight")),
				b"cpht" => add(&mut os2.s_cap_height, delta, "sCapHeight"),
				b"sbxs" => add(&mut os2.y_subscript_x_size, delta, "ySubscriptXSize"),
				b"sbys" => add(&mut os2.y_subscript_y_size, delta, "ySubscriptYSize"),
				b"sbxo" => add(&mut os2.y_subscript_x_offset, delta, "ySubscriptXOffset"),
				b"sbyo" => add(&mut os2.y_subscript_y_offset, delta, "ySubscriptYOffset"),
				b"spxs" => add(&mut os2.y_superscript_x_size, delta, "ySuperscriptXSize"),
				b"spys" => add(&mut os2.y_superscript_y_size, delta, "ySuperscriptYSize"),
				b"spxo" => add(
					&mut os2.y_superscript_x_offset,
					delta,
					"ySuperscriptXOffset",
				),
				b"spyo" => add(
					&mut os2.y_superscript_y_offset,
					delta,
					"ySuperscriptYOffset",
				),
				b"strs" => add(&mut os2.y_strikeout_size, delta, "yStrikeoutSize"),
				_ => add(&mut os2.y_strikeout_position, delta, "yStrikeoutPosition"),
			}
		}
		b"vasc" | b"vdsc" | b"vlgp" | b"vcrs" | b"vcrn" | b"vcof" => {
			let vhea = font.require::<Vhea>()?;
			match &tag.0 {
				b"vasc" => add(&mut vhea.ascent, delta, "ascent"),
				b"vdsc" => add(&mut vhea.descent, delta, "descent"),
				b"vlgp" => add(&mut vhea.line_gap, delta, "lineGap"),
				b"vcrs" => add(&mut vhea.caret_slope_rise, delta, "caretSlopeRise"),
				b"vcrn" => add(&mut vhea.caret_slope_run, delta, "caretSlopeRun"),
				_ => add(&mut vhea.caret_offset, delta, "caretOffset"),
			}
		}
		b"hcrs" | b"hcrn" | b"hcof" => {
			let hhea = font.require::<Hhea>()?;
			match &tag.0 {
				b"hcrs" => add(&mut hhea.caret_slope_rise, delta, "caretSlopeRise"),
				b"hcrn" => add(&mut hhea.caret_slope_run, delta, "caretSlopeRun"),
				_ => add(&mut hhea.caret_offset, delta, "caretOffset"),
			}
		}
		b"unds" | b"undo" => {
			let post = font.require::<Post>()?;
			if tag.0 == *b"unds" {
				add(&mut post.underline_thickness, delta, "underlineThickness")
			} else {
				add(&mut post.underline_position, delta, "underlinePosition")
			}
		}
		_ => Ok(()),
	}
}

fn is_mvar_entry(tag: &Tag) -> bool {
	matches!(
		&tag.0,
		b"hasc"
			| b"hdsc" | b"hlgp"
			| b"hcla" | b"hcld"
			| b"vasc" | b"vdsc"
			| b"vlgp" | b"hcrs"
			| b"hcrn" | b"hcof"
			| b"vcrs" | b"vcrn"
			| b"vcof" | b"xhgt"
			| b"cpht" | b"sbxs"
			| b"sbys" | b"sbxo"
			| b"sbyo" | b"spxs"
			| b"spys" | b"spxo"
			| b"spyo" | b"strs"
			| b"stro" | b"unds"
			| b"undo"
	)
}

/// `verticalMetricsKeptInSync`: the OS/2 metrics, if the same as those of `hhea`.
fn synced_vertical_metrics(font: &mut Font) -> Result<Option<[i16; 3]>> {
	let os2 = font.require::<Os2>()?;
	let metrics = [
		os2.s_typo_ascender,
		os2.s_typo_descender,
		os2.s_typo_line_gap,
	];
	let hhea = font.require::<Hhea>()?;
	Ok((metrics == [hhea.ascent, hhea.descent, hhea.line_gap]).then_some(metrics))
}

/// `instantiateMVAR`.
pub fn instantiate_mvar(font: &mut Font, limits: &NormalizedLimits) -> Result<()> {
	log::info!("Instantiating MVAR table");
	let axes: Vec<String> = fvar_axes(font)?.into_iter().map(|a| a.0).collect();
	let mvar = &mut font.require::<Mvar>()?.table;
	let store = mvar
		.var_store
		.as_mut()
		.ok_or_else(|| Error::ttlib("AttributeError: no attribute 'VarRegionList'"))?;
	let deltas = instantiate_item_variation_store(store, &axes, limits)?;
	let records: Vec<(Tag, u64)> = mvar
		.value_record
		.iter()
		.map(|r| (r.value_tag, u64::from(r.var_idx)))
		.collect();

	let synced = synced_vertical_metrics(font)?;
	for (tag, var_idx) in records {
		if !is_mvar_entry(&tag) {
			continue;
		}
		let delta = deltas.get(var_idx)?;
		if delta != 0.0 {
			apply_mvar_delta(font, &tag, ot_round(delta))?;
		}
	}
	if let Some(old) = synced {
		let os2 = font.require::<Os2>()?;
		let new = [
			os2.s_typo_ascender,
			os2.s_typo_descender,
			os2.s_typo_line_gap,
		];
		if new != old {
			let hhea = font.require::<Hhea>()?;
			[hhea.ascent, hhea.descent, hhea.line_gap] = new;
		}
	}

	let mvar = &mut font.require::<Mvar>()?.table;
	let store = mvar
		.var_store
		.as_mut()
		.ok_or_else(|| Error::ttlib("AttributeError: no attribute 'VarRegionList'"))?;
	if has_regions(store) {
		let map = var_store::optimize(store, true)?;
		for record in &mut mvar.value_record {
			let new = *map
				.get(&u64::from(record.var_idx))
				.ok_or_else(|| Error::ttlib(format!("KeyError: {}", record.var_idx)))?;
			record.var_idx = fit(new, "VarIdx")?;
		}
	} else {
		font.remove(MVAR);
	}
	Ok(())
}

fn has_regions(store: &ot::VarStore) -> bool {
	store
		.var_region_list
		.as_ref()
		.is_some_and(|l| !l.region.is_empty())
}

/// `_remapVarIdxMap`.
fn remap_var_idx_map(
	map: &mut ot::VarIdxMap,
	var_index_mapping: &HashMap<u64, u64>,
	num_glyphs: u32,
) -> Result<()> {
	let mut new = BTreeMap::new();
	for gid in 0..num_glyphs {
		let old = map
			.mapping
			.get(&gid)
			.ok_or_else(|| Error::ttlib(format!("KeyError: glyph {gid}")))?;
		let index = var_index_mapping
			.get(old)
			.ok_or_else(|| Error::ttlib(format!("KeyError: {old}")))?;
		new.insert(gid, *index);
	}
	map.mapping = new;
	Ok(())
}

/// The metrics of `hmtx` or `vmtx`, for CFF2 fonts.
fn apply_metric_deltas(
	metrics: &mut BTreeMap<u32, (u16, i16)>,
	advance_map: Option<&ot::VarIdxMap>,
	deltas: &DefaultDeltas,
) -> Result<()> {
	for (&gid, metric) in metrics.iter_mut() {
		let var_idx = match advance_map {
			Some(map) => *map
				.mapping
				.get(&gid)
				.ok_or_else(|| Error::ttlib(format!("KeyError: glyph {gid}")))?,
			None => u64::from(gid),
		};
		let delta = ot_round(deltas.get(var_idx)?);
		metric.0 = fit((i64::from(metric.0) + delta).max(0), "advance")?;
	}
	Ok(())
}

/// `_instantiateVHVAR`, for `HVAR`.
pub fn instantiate_hvar(font: &mut Font, limits: &NormalizedLimits) -> Result<()> {
	log::info!("Instantiating HVAR table");
	let all_axes = fvar_axes(font)?;
	let pinned = limits.pinned_location();
	let full = all_axes.iter().all(|(t, _)| pinned.contains_key(t));
	let axes: Vec<String> = all_axes.into_iter().map(|a| a.0).collect();
	let has_glyf = font.contains(GLYF);
	// Deltas from gvar have already been applied to hmtx: full instances can drop
	// the table.
	if has_glyf && full {
		log::info!("Dropping HVAR table");
		font.remove(HVAR);
		return Ok(());
	}
	let table = &mut font.require::<Hvar>()?.table;
	let store = table
		.var_store
		.as_mut()
		.ok_or_else(|| Error::ttlib("AttributeError: no attribute 'VarRegionList'"))?;
	let deltas = instantiate_item_variation_store(store, &axes, limits)?;
	if !has_glyf && font.contains(HMTX) {
		let advance_map = font.require::<Hvar>()?.adv_width_map.clone();
		let hmtx = font.require::<Hmtx>()?;
		apply_metric_deltas(&mut hmtx.metrics, advance_map.as_deref(), &deltas)?;
		if full {
			log::info!("Dropping HVAR table");
			font.remove(HVAR);
			return Ok(());
		}
	}
	let num_glyphs = font.glyph_order()?.len() as u32;
	let table = &mut font.require::<Hvar>()?.table;
	let store = table
		.var_store
		.as_mut()
		.ok_or_else(|| Error::ttlib("AttributeError: no attribute 'VarRegionList'"))?;
	// Only re-optimize if the table uses an indirect mapping: a direct, implicit
	// glyph ID to variation index mapping must keep the original indices.
	if has_regions(store) && table.adv_width_map.is_some() {
		let map = var_store::optimize(store, false)?;
		for field in [
			&mut table.adv_width_map,
			&mut table.lsb_map,
			&mut table.rsb_map,
		]
		.into_iter()
		.flatten()
		{
			remap_var_idx_map(field, &map, num_glyphs)?;
		}
	}
	Ok(())
}

/// `_instantiateVHVAR`, for `VVAR`.
pub fn instantiate_vvar(font: &mut Font, limits: &NormalizedLimits) -> Result<()> {
	log::info!("Instantiating VVAR table");
	let all_axes = fvar_axes(font)?;
	let pinned = limits.pinned_location();
	let full = all_axes.iter().all(|(t, _)| pinned.contains_key(t));
	let axes: Vec<String> = all_axes.into_iter().map(|a| a.0).collect();
	let has_glyf = font.contains(GLYF);
	if has_glyf && full {
		log::info!("Dropping VVAR table");
		font.remove(VVAR);
		return Ok(());
	}
	let table = &mut font.require::<Vvar>()?.table;
	let store = table
		.var_store
		.as_mut()
		.ok_or_else(|| Error::ttlib("AttributeError: no attribute 'VarRegionList'"))?;
	let deltas = instantiate_item_variation_store(store, &axes, limits)?;
	if !has_glyf && font.contains(VMTX) {
		let table = &mut font.require::<Vvar>()?.table;
		let advance_map = table.adv_height_map.clone();
		let origin_map = table.v_org_map.clone();
		let vmtx = font.require::<Vmtx>()?;
		apply_metric_deltas(&mut vmtx.metrics, advance_map.as_deref(), &deltas)?;
		if let (true, Some(origin_map)) = (font.contains(VORG), origin_map) {
			let num_glyphs = font.glyph_order()?.len() as u32;
			let vorg = font.require::<Vorg>()?;
			let mut origins = Vec::with_capacity(num_glyphs as usize);
			for gid in 0..num_glyphs {
				let var_idx = origin_map
					.mapping
					.get(&gid)
					.ok_or_else(|| Error::ttlib(format!("KeyError: glyph {gid}")))?;
				let base = vorg
					.v_origin_records
					.get(&gid)
					.copied()
					.unwrap_or(vorg.default_vert_origin_y);
				origins.push((gid, i64::from(base) + ot_round(deltas.get(*var_idx)?)));
			}
			// `Counter.most_common(1)`: the first among the most common.
			let mut counts: Vec<(i64, usize)> = Vec::new();
			for &(_, origin) in &origins {
				match counts.iter_mut().find(|c| c.0 == origin) {
					Some(c) => c.1 += 1,
					None => counts.push((origin, 1)),
				}
			}
			let mut best: Option<(i64, usize)> = None;
			for &(origin, count) in &counts {
				if best.is_none_or(|b| count > b.1) {
					best = Some((origin, count));
				}
			}
			if let Some((origin, count)) = best {
				let current = counts
					.iter()
					.find(|c| c.0 == i64::from(vorg.default_vert_origin_y))
					.map_or(0, |c| c.1);
				// Keep the old default when it is equally common.
				if current != count {
					vorg.default_vert_origin_y = fit(origin, "defaultVertOriginY")?;
				}
			}
			let default = i64::from(vorg.default_vert_origin_y);
			let mut records = BTreeMap::new();
			for (gid, origin) in origins {
				if origin != default {
					records.insert(gid, fit(origin, "vertOriginY")?);
				}
			}
			vorg.v_origin_records = records;
		}
		if full {
			log::info!("Dropping VVAR table");
			font.remove(VVAR);
			return Ok(());
		}
	}
	let num_glyphs = font.glyph_order()?.len() as u32;
	let table = &mut font.require::<Vvar>()?.table;
	let store = table
		.var_store
		.as_mut()
		.ok_or_else(|| Error::ttlib("AttributeError: no attribute 'VarRegionList'"))?;
	if has_regions(store) && table.adv_height_map.is_some() {
		let map = var_store::optimize(store, false)?;
		for field in [
			&mut table.adv_height_map,
			&mut table.tsb_map,
			&mut table.bsb_map,
			&mut table.v_org_map,
		]
		.into_iter()
		.flatten()
		{
			remap_var_idx_map(field, &map, num_glyphs)?;
		}
	}
	Ok(())
}
