//! Port of `fontTools.varLib.instancer.names`: updates the name table of an
//! instance from the axis values of `STAT`.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use fontoxide_misc::py;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::name::Name;
use fontoxide_ttlib::tables::{Head, Os2, Stat};
use fontoxide_ttlib::{Error, Font, Result};

use super::limits::{UserLimits, fvar_axes};
use super::{axis_value_is_kept, design_axis_tags};

const FAMILY_NAME: u16 = 1;
const SUBFAMILY_NAME: u16 = 2;
const UNIQUE_FONT_IDENTIFIER: u16 = 3;
const FULL_FONT_NAME: u16 = 4;
const VERSION_STRING: u16 = 5;
const POSTSCRIPT_NAME: u16 = 6;
const TYPOGRAPHIC_FAMILY_NAME: u16 = 16;
const TYPOGRAPHIC_SUBFAMILY_NAME: u16 = 17;
const VARIATIONS_POSTSCRIPT_NAME_PREFIX: u16 = 25;

pub const ELIDABLE_AXIS_VALUE_NAME: u16 = 2;

/// `(platformID, platEncID, langID)`.
type Platform = (u16, u16, u16);

fn no_attribute(name: &str) -> Error {
	Error::ttlib(format!(
		"AttributeError: 'NoneType' object has no attribute '{name}'"
	))
}

fn index_error() -> Error {
	Error::ttlib("IndexError: list index out of range")
}

/// `getName(...)`, then its `toUnicode()` if there is such a record.
fn find_name(name: &Name, name_id: u16, platform: Platform) -> Result<Option<String>> {
	name.get_name(name_id, platform.0, platform.1, Some(platform.2))
		.map(|r| {
			r.to_unicode()
				.map_err(|e| Error::ttlib(format!("UnicodeDecodeError: {e}")))
		})
		.transpose()
}

/// `getName(...).toUnicode()`.
fn get_name(name: &Name, name_id: u16, platform: Platform) -> Result<String> {
	find_name(name, name_id, platform)?.ok_or_else(|| no_attribute("toUnicode"))
}

/// `_isRibbi`.
fn is_ribbi(name: &Name, name_id: u16) -> Result<bool> {
	Ok(find_name(name, name_id, (3, 1, 0x409))?
		.is_some_and(|s| matches!(s.as_str(), "Regular" | "Italic" | "Bold" | "Bold Italic")))
}

/// The axis index of a value of format 1, 2 or 3.
fn axis_index(value: &ot::AxisValue) -> Result<u16> {
	match value {
		ot::AxisValue::Format1(v) => Ok(v.axis_index),
		ot::AxisValue::Format2(v) => Ok(v.axis_index),
		ot::AxisValue::Format3(v) => Ok(v.axis_index),
		_ => Err(Error::ttlib(
			"AttributeError: 'AxisValue' object has no attribute 'AxisIndex'",
		)),
	}
}

fn value_name_id(value: &ot::AxisValue) -> Result<u16> {
	match value {
		ot::AxisValue::Format1(v) => Ok(v.value_name_id),
		ot::AxisValue::Format2(v) => Ok(v.value_name_id),
		ot::AxisValue::Format3(v) => Ok(v.value_name_id),
		ot::AxisValue::Format4(v) => Ok(v.value_name_id),
		ot::AxisValue::Unknown(_) => Err(Error::ttlib(
			"AttributeError: 'AxisValue' object has no attribute 'ValueNameID'",
		)),
	}
}

fn flags(value: &ot::AxisValue) -> Result<u16> {
	match value {
		ot::AxisValue::Format1(v) => Ok(v.flags),
		ot::AxisValue::Format2(v) => Ok(v.flags),
		ot::AxisValue::Format3(v) => Ok(v.flags),
		ot::AxisValue::Format4(v) => Ok(v.flags),
		ot::AxisValue::Unknown(_) => Err(Error::ttlib(
			"AttributeError: 'AxisValue' object has no attribute 'Flags'",
		)),
	}
}

/// `checkAxisValuesExist`.
fn check_axis_values_exist(
	all_values: &[&ot::AxisValue],
	values: &[&ot::AxisValue],
	tags: &[String],
	coords: &BTreeMap<String, f64>,
) -> Result<()> {
	let tag_of = |index: u16| tags.get(usize::from(index)).ok_or_else(index_error);
	let mut has_values = BTreeSet::new();
	for value in all_values {
		match value {
			ot::AxisValue::Format1(_) | ot::AxisValue::Format2(_) | ot::AxisValue::Format3(_) => {
				has_values.insert(tag_of(axis_index(value)?)?);
			}
			ot::AxisValue::Format4(v) => {
				for record in &v.axis_value_record {
					has_values.insert(tag_of(record.axis_index)?);
				}
			}
			ot::AxisValue::Unknown(_) => {}
		}
	}
	let mut seen = BTreeSet::new();
	let mut see = |index: u16, value: f64| -> Result<()> {
		let tag = tag_of(index)?;
		if coords.get(tag) == Some(&value) {
			seen.insert(tag);
		}
		Ok(())
	};
	for value in values {
		match value {
			ot::AxisValue::Format1(v) => see(v.axis_index, v.value)?,
			ot::AxisValue::Format2(v) => see(v.axis_index, v.nominal_value)?,
			ot::AxisValue::Format3(v) => see(v.axis_index, v.value)?,
			ot::AxisValue::Format4(v) => {
				for record in &v.axis_value_record {
					see(record.axis_index, record.value)?;
				}
			}
			ot::AxisValue::Unknown(_) => {}
		}
	}
	let missing: Vec<String> = coords
		.iter()
		.filter(|(tag, _)| !seen.contains(tag) && has_values.contains(tag))
		.map(|(tag, value)| format!("'{tag}': {}", py::float_repr(*value)))
		.collect();
	if !missing.is_empty() {
		return Err(Error::value(format!(
			"Cannot find Axis Values {{{}}}",
			missing.join(", ")
		)));
	}
	Ok(())
}

/// `_sortAxisValues`: sorted by axis index, without duplicates, with the values of
/// format 4 being dominant.
fn sort_axis_values<'a>(values: &[&'a ot::AxisValue]) -> Result<Vec<&'a ot::AxisValue>> {
	let mut results: Vec<(u16, &ot::AxisValue)> = Vec::new();
	let mut seen_axes = HashSet::new();
	// The values with the most records go first.
	let mut format4: Vec<&ot::AxisValueFormat4> = values
		.iter()
		.filter_map(|v| match v {
			ot::AxisValue::Format4(v) => Some(v),
			_ => None,
		})
		.collect();
	format4.sort_by_key(|v| std::cmp::Reverse(v.axis_value_record.len()));
	for wanted in format4 {
		let indexes: BTreeSet<u16> = wanted
			.axis_value_record
			.iter()
			.map(|r| r.axis_index)
			.collect();
		let min_index = *indexes
			.first()
			.ok_or_else(|| Error::value("min() iterable argument is empty"))?;
		if indexes.iter().all(|i| !seen_axes.contains(i)) {
			seen_axes.extend(indexes);
			let value = values
				.iter()
				.find(|v| matches!(v, ot::AxisValue::Format4(f) if std::ptr::eq(f, wanted)))
				.ok_or_else(index_error)?;
			results.push((min_index, value));
		}
	}
	for value in values {
		if matches!(value, ot::AxisValue::Format4(_)) {
			continue;
		}
		let index = axis_index(value)?;
		if seen_axes.insert(index) {
			results.push((index, value));
		}
	}
	results.sort_by_key(|r| r.0);
	Ok(results.into_iter().map(|r| r.1).collect())
}

/// `_fontVersion`.
fn font_version(font: &mut Font, platform: Platform) -> Result<String> {
	let record = find_name(font.require::<Name>()?, VERSION_STRING, platform)?;
	match record {
		None => Ok(format!("{:.3}", font.require::<Head>()?.font_revision)),
		// "Version 1.101; ttfautohint (v1.8.1.43-b0c9)" --> "1.101"
		Some(version) => {
			let number = version.split(';').next().unwrap_or_default();
			Ok(py::strip(number.trim_start_matches(|c| "Version ".contains(c))).to_owned())
		}
	}
}

/// `_updateUniqueIdNameRecord`, given the new names so far.
fn update_unique_id(
	font: &mut Font,
	full_name: &str,
	ps_name: &str,
	platform: Platform,
) -> Result<Option<String>> {
	let name = font.require::<Name>()?;
	let Some(current) = find_name(name, UNIQUE_FONT_IDENTIFIER, platform)? else {
		return Ok(None);
	};
	// Check if full name and postscript name are a substring of the current one.
	for (name_id, new) in [(FULL_FONT_NAME, full_name), (POSTSCRIPT_NAME, ps_name)] {
		if let Some(old) = find_name(name, name_id, platform)?
			&& current.contains(&old)
		{
			return Ok(Some(current.replace(&old, new)));
		}
	}
	// Create a new string since we couldn't find any substrings.
	let version = font_version(font, platform)?;
	let vendor = font.require::<Os2>()?.ach_vend_id;
	if !vendor.is_ascii() {
		// fontTools keeps the bytes that aren't ASCII as they are.
		return Err(Error::ttlib(
			"TypeError: cannot use a string pattern on a bytes-like object",
		));
	}
	let vendor: String = vendor.iter().map(|&b| char::from(b)).collect();
	Ok(Some(format!("{version};{};{ps_name}", py::strip(&vendor))))
}

/// `_updatePSNameRecord`.
fn update_ps_name(name: &Name, family: &str, style: &str, platform: Platform) -> Result<String> {
	let prefix = find_name(name, VARIATIONS_POSTSCRIPT_NAME_PREFIX, platform)?;
	let prefix = prefix.as_deref().unwrap_or(family);
	// Remove any characters other than Latin letters, digits and hyphens.
	let ps_name: String = format!("{prefix}-{style}")
		.chars()
		.filter(|c| c.is_ascii_alphanumeric() || *c == '-')
		.collect();
	if ps_name.len() > 127 {
		// Abbreviating the style name to conform to every vendor's specification is
		// too complex. Instead simply truncate, and add the required "...".
		return Ok(format!("{}...", &ps_name[..124]));
	}
	Ok(ps_name)
}

/// `_updateNameTableStyleRecords`.
fn update_style_records(
	font: &mut Font,
	family_suffix: &str,
	subfamily: &str,
	typo_subfamily: Option<&str>,
	platform: Platform,
) -> Result<()> {
	let has_fvar = font.contains(fontoxide_ttlib::Tag::new(b"fvar"));
	let name = font.require::<Name>()?;
	let first = |ids: [u16; 2], name: &Name| -> Result<Option<String>> {
		match find_name(name, ids[0], platform)? {
			Some(s) => Ok(Some(s)),
			None => find_name(name, ids[1], platform),
		}
	};
	let current_family = first([TYPOGRAPHIC_FAMILY_NAME, FAMILY_NAME], name)?;
	let current_style = first([TYPOGRAPHIC_SUBFAMILY_NAME, SUBFAMILY_NAME], name)?;
	let (Some(current_family), Some(_)) = (current_family, current_style) else {
		return Err(Error::value(format!(
			"Missing required NameIDs 1 and 2 for platform ({}, {}, {})",
			platform.0, platform.1, platform.2
		)));
	};

	let subfamily = if subfamily.is_empty() {
		"Regular"
	} else {
		subfamily
	};
	let (new_family, new_style, family_name, typo) = match typo_subfamily {
		Some(typo) => {
			let family = format!("{current_family} {family_suffix}");
			(
				// `nameIDs[TYPOGRAPHIC_FAMILY_NAME] or nameIDs[FAMILY_NAME]`.
				if current_family.is_empty() {
					py::strip(&family).to_owned()
				} else {
					current_family.clone()
				},
				typo,
				py::strip(&family).to_owned(),
				Some((current_family.clone(), typo)),
			)
		}
		None => {
			// Remove previous Typographic Family and SubFamily names since they're no
			// longer required.
			name.remove_names(Some(TYPOGRAPHIC_FAMILY_NAME), None, None, None);
			name.remove_names(Some(TYPOGRAPHIC_SUBFAMILY_NAME), None, None, None);
			(
				current_family.clone(),
				subfamily,
				current_family.clone(),
				None,
			)
		}
	};
	let full_name = format!("{new_family} {new_style}");
	let ps_name = update_ps_name(name, &new_family, new_style, platform)?;
	let unique_id = update_unique_id(font, &full_name, &ps_name, platform)?;

	// In the order of the dict of fontTools.
	let mut names: Vec<(u16, &str)> =
		vec![(FAMILY_NAME, &family_name), (SUBFAMILY_NAME, subfamily)];
	if let Some((family, style)) = &typo {
		names.push((TYPOGRAPHIC_FAMILY_NAME, family));
		names.push((TYPOGRAPHIC_SUBFAMILY_NAME, style));
	}
	names.push((FULL_FONT_NAME, &full_name));
	names.push((POSTSCRIPT_NAME, &ps_name));
	if let Some(unique_id) = unique_id.as_deref().filter(|s| !s.is_empty()) {
		names.push((UNIQUE_FONT_IDENTIFIER, unique_id));
	}
	let name = font.require::<Name>()?;
	for (name_id, string) in names {
		if string.is_empty() {
			return Err(Error::Assertion(format!("{name_id}")));
		}
		name.set_name(
			string.to_owned(),
			name_id,
			platform.0,
			platform.1,
			platform.2,
		);
	}
	if !has_fvar {
		name.remove_names(Some(VARIATIONS_POSTSCRIPT_NAME_PREFIX), None, None, None);
	}
	Ok(())
}

/// `_updateNameRecords`.
fn update_name_records(font: &mut Font, values: &[&ot::AxisValue], elided: u16) -> Result<()> {
	let name = font.require::<Name>()?;
	let value_name_ids: Vec<u16> = values
		.iter()
		.map(|v| value_name_id(v))
		.collect::<Result<_>>()?;
	let mut ribbi = Vec::new();
	let mut non_ribbi = Vec::new();
	for &id in &value_name_ids {
		if is_ribbi(name, id)? {
			ribbi.push(id);
		} else {
			non_ribbi.push(id);
		}
	}
	let elided_is_ribbi = is_ribbi(name, elided)?;
	// The order matters, as the records removed for a platform are gone for the next
	// one: this is the order of a set of tuples of integers in Python, which doesn't
	// depend on the hash seed.
	let platforms = py::set_order(name.names.iter().map(|r| {
		let key = (r.platform_id, r.plat_enc_id, r.lang_id);
		(
			key,
			py::tuple_hash(&[i64::from(key.0), i64::from(key.1), i64::from(key.2)]),
		)
	}));
	for platform in platforms {
		let name = font.require::<Name>()?;
		let has = |id: u16| {
			name.get_name(id, platform.0, platform.1, Some(platform.2))
				.is_some()
		};
		if !(has(FAMILY_NAME) && has(SUBFAMILY_NAME) && has(elided)) {
			// Since no family name and subfamily name records were found, we cannot
			// update this set of name records.
			continue;
		}
		let join = |ids: &[u16]| -> Result<String> {
			let parts = ids
				.iter()
				.map(|&id| get_name(name, id, platform))
				.collect::<Result<Vec<_>>>()?;
			Ok(parts.join(" "))
		};
		let mut subfamily = join(&ribbi)?;
		let mut typo = if non_ribbi.is_empty() {
			String::new()
		} else {
			join(&value_name_ids)?
		};
		// If neither exist, use the STAT's elided fallback name.
		if typo.is_empty() && subfamily.is_empty() {
			if elided_is_ribbi {
				subfamily = get_name(name, elided, platform)?;
			} else {
				typo = get_name(name, elided, platform)?;
			}
		}
		let suffix = join(&non_ribbi)?;
		let typo = (!typo.is_empty()).then_some(typo.as_str());
		update_style_records(font, &suffix, &subfamily, typo, platform)?;
	}
	Ok(())
}

/// `updateNameTable`: updates the name table of an instantiated variable font using
/// the axis values of `STAT`.
pub(super) fn update_name_table(font: &mut Font, limits: &UserLimits) -> Result<()> {
	if !font.contains(fontoxide_ttlib::Tag::new(b"STAT")) {
		return Err(Error::value(
			"Cannot update name table since there is no STAT table.",
		));
	}
	let axes = fvar_axes(font)?;
	let stat = font.require::<Stat>()?;
	let Some(array) = stat.axis_value_array.as_ref() else {
		return Err(Error::value(
			"Cannot update name table since there are no STAT Axis Values",
		));
	};
	let array = array.clone();
	let tags = design_axis_tags(stat)?;
	// STAT 1.0 has no elided fallback name; use the existing subfamily name.
	let elided = if stat.version >= 0x0001_0001 {
		stat.elided_fallback_name_id
	} else {
		SUBFAMILY_NAME
	};

	// The updated name table will reflect the new 'zero origin' of the font. If we're
	// instantiating a partial font, populate the unpinned axes with the defaults of
	// fvar.
	let mut coords: BTreeMap<String, f64> =
		axes.into_iter().map(|(tag, axis)| (tag, axis.1)).collect();
	coords.extend(limits.default_location());
	let ranges: BTreeMap<String, (f64, f64)> =
		coords.iter().map(|(k, &v)| (k.clone(), (v, v))).collect();

	let mut all_values = Vec::with_capacity(array.axis_value.len());
	for value in &array.axis_value {
		all_values.push(value.as_deref().ok_or_else(|| no_attribute("Format"))?);
	}
	let mut values = Vec::new();
	for &value in &all_values {
		if axis_value_is_kept(value, &tags, &ranges)? {
			values.push(value);
		}
	}
	check_axis_values_exist(&all_values, &values, &tags, &coords)?;
	// Ignore "elidable" axis values: they should be omitted in font menus.
	let mut named = Vec::with_capacity(values.len());
	for value in values {
		if flags(value)? & ELIDABLE_AXIS_VALUE_NAME == 0 {
			named.push(value);
		}
	}
	let sorted = sort_axis_values(&named)?;
	update_name_records(font, &sorted, elided)
}

/// `setRibbiBits`: sets the `head.macStyle` and `OS/2.fsSelection` style bits.
pub(super) fn set_ribbi_bits(font: &mut Font) -> Result<()> {
	let Some(style) = find_name(font.require::<Name>()?, SUBFAMILY_NAME, (3, 1, 0x409))? else {
		return Ok(());
	};
	let style = style.to_lowercase();
	let (mac_style, selection) = match style.as_str() {
		"regular" => (None, 1 << 6),
		"bold" => (Some(0b01), 1 << 5),
		"italic" => (Some(0b10), 1 << 0),
		"bold italic" => (Some(0b11), (1 << 0) | (1 << 5)),
		_ => return Ok(()),
	};
	if let Some(mac_style) = mac_style {
		font.require::<Head>()?.mac_style = mac_style;
	}
	let os2 = font.require::<Os2>()?;
	// First clear, then re-set the bits.
	os2.fs_selection &= !((1 << 0) | (1 << 5) | (1 << 6));
	os2.fs_selection |= selection;
	Ok(())
}
