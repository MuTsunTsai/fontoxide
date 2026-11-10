//! `buildStatTable`.

use fontoxide_misc::Tag;
use fontoxide_ttlib::Font;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::Stat;
use fontoxide_ttlib::tables::name::{Name, add_multilingual_name};

use crate::{Error, Result};

/// The lowest value a Fixed number can have (`AXIS_VALUE_NEGATIVE_INFINITY`).
pub const AXIS_VALUE_NEGATIVE_INFINITY: f64 = -32768.0;

/// The highest value a Fixed number can have (`AXIS_VALUE_POSITIVE_INFINITY`):
/// `0x7FFFFFFF / 65536`.
pub const AXIS_VALUE_POSITIVE_INFINITY: f64 = 0x7FFF_FFFF as f64 / 65536.0;

/// A name for the STAT table (`STATName`, or a list of `STATNameStatement`s).
#[derive(Debug, Clone, PartialEq)]
pub enum StatName {
	/// A name ID that exists already.
	Id(u16),
	/// An English name.
	English(String),
	/// A name in several languages: BCP 47 language tags and strings.
	Multilingual(Vec<(String, String)>),
	/// Name records for a new name ID (feaLib's `STATNameStatement`s).
	Records(Vec<StatNameRecord>),
}

impl From<u16> for StatName {
	fn from(id: u16) -> Self {
		StatName::Id(id)
	}
}

impl From<&str> for StatName {
	fn from(s: &str) -> Self {
		StatName::English(s.to_owned())
	}
}

/// A name record for [`StatName::Records`].
#[derive(Debug, Clone, PartialEq)]
pub struct StatNameRecord {
	/// The string.
	pub string: String,
	/// The platform ID.
	pub platform_id: u16,
	/// The platform-specific encoding ID.
	pub plat_enc_id: u16,
	/// The language ID.
	pub lang_id: u16,
}

/// An axis of the STAT table (the axis dicts of `buildStatTable`).
#[derive(Debug, Clone, PartialEq)]
pub struct StatAxis {
	/// The axis tag (`tag`).
	pub tag: Tag,
	/// The axis name (`name`).
	pub name: StatName,
	/// The axis ordering (`ordering`); the index of the axis if `None`.
	pub ordering: Option<u16>,
	/// The axis values of the axis (`values`).
	pub values: Vec<StatAxisValue>,
}

/// An axis value on one axis (the value dicts of `buildStatTable`).
#[derive(Debug, Clone, PartialEq)]
pub struct StatAxisValue {
	/// The name of the value (`name`).
	pub name: StatName,
	/// The flags (`flags`).
	pub flags: u16,
	/// The value, which determines the format.
	pub kind: StatAxisValueKind,
}

/// The value of an axis value, which determines its format.
#[derive(Debug, Clone, PartialEq)]
pub enum StatAxisValueKind {
	/// `value`, with `linkedValue` for format 3, otherwise format 1.
	Value {
		/// `value`.
		value: f64,
		/// `linkedValue`.
		linked_value: Option<f64>,
	},
	/// `nominalValue` with `rangeMinValue` and `rangeMaxValue` (format 2); the
	/// range is unbounded where they are missing.
	Range {
		/// `nominalValue`.
		nominal_value: f64,
		/// `rangeMinValue`.
		range_min_value: Option<f64>,
		/// `rangeMaxValue`.
		range_max_value: Option<f64>,
	},
}

/// A named location on several axes (the `locations` of `buildStatTable`),
/// which becomes a format 4 axis value.
#[derive(Debug, Clone, PartialEq)]
pub struct StatLocation {
	/// The name (`name`).
	pub name: StatName,
	/// The flags (`flags`).
	pub flags: u16,
	/// The value on each axis, by tag (`location`).
	pub location: Vec<(Tag, f64)>,
}

/// Adds a `STAT` table to the font, replacing any existing one
/// (`buildStatTable`). The font must have a `name` table; names that are not
/// name IDs are added to it, for Windows and Macintosh as requested.
pub fn build_stat_table(
	font: &mut Font,
	axes: &[StatAxis],
	locations: Option<&[StatLocation]>,
	elided_fallback_name: &StatName,
	windows_names: bool,
	mac_names: bool,
) -> Result<()> {
	let mut stat = ot::STAT {
		elided_fallback_name_id: add_name(font, elided_fallback_name, 0, windows_names, mac_names)?,
		..Default::default()
	};
	let (axis_records, mut axis_values) = build_axis_records(axes, font, windows_names, mac_names)?;
	match locations.filter(|l| !l.is_empty()) {
		None => stat.version = 0x0001_0001,
		Some(locations) => {
			// Format 4 axis values need a higher version.
			stat.version = 0x0001_0002;
			let mut values =
				build_axis_values_format4(locations, axes, font, windows_names, mac_names)?;
			values.append(&mut axis_values);
			axis_values = values;
		}
	}
	font.require::<Name>()?.sort();
	stat.design_axis_record_size = Some(8);
	stat.design_axis_count = Some(axis_records.len() as u32);
	stat.design_axis_record = Some(Box::new(ot::AxisRecordArray { axis: axis_records }));
	stat.axis_value_count = Some(0);
	stat.axis_value_array = None;
	if !axis_values.is_empty() {
		stat.axis_value_count = Some(axis_values.len() as u32);
		stat.axis_value_array = Some(Box::new(ot::AxisValueArray {
			axis_value: axis_values.into_iter().map(|v| Some(Box::new(v))).collect(),
		}));
	}
	font.insert(Box::new(Stat { table: stat }));
	Ok(())
}

/// `_buildAxisRecords`.
fn build_axis_records(
	axes: &[StatAxis],
	font: &mut Font,
	windows: bool,
	mac: bool,
) -> Result<(Vec<ot::AxisRecord>, Vec<ot::AxisValue>)> {
	let mut records = Vec::new();
	let mut values = Vec::new();
	for (index, axis) in axes.iter().enumerate() {
		let index = u16::try_from(index).map_err(|_| Error::Value("too many axes".into()))?;
		records.push(ot::AxisRecord {
			axis_tag: axis.tag,
			axis_name_id: add_name(font, &axis.name, 256, windows, mac)?,
			axis_ordering: axis.ordering.unwrap_or(index),
			more_bytes: Vec::new(),
		});
		for value in &axis.values {
			let value_name_id = add_name(font, &value.name, 0, windows, mac)?;
			let flags = value.flags;
			values.push(match value.kind {
				StatAxisValueKind::Value {
					value,
					linked_value: Some(linked_value),
				} => ot::AxisValue::Format3(ot::AxisValueFormat3 {
					axis_index: index,
					flags,
					value_name_id,
					value,
					linked_value,
				}),
				StatAxisValueKind::Value {
					value,
					linked_value: None,
				} => ot::AxisValue::Format1(ot::AxisValueFormat1 {
					axis_index: index,
					flags,
					value_name_id,
					value,
				}),
				StatAxisValueKind::Range {
					nominal_value,
					range_min_value,
					range_max_value,
				} => ot::AxisValue::Format2(ot::AxisValueFormat2 {
					axis_index: index,
					flags,
					value_name_id,
					nominal_value,
					range_min_value: range_min_value.unwrap_or(AXIS_VALUE_NEGATIVE_INFINITY),
					range_max_value: range_max_value.unwrap_or(AXIS_VALUE_POSITIVE_INFINITY),
				}),
			});
		}
	}
	Ok((records, values))
}

/// `_buildAxisValuesFormat4`.
fn build_axis_values_format4(
	locations: &[StatLocation],
	axes: &[StatAxis],
	font: &mut Font,
	windows: bool,
	mac: bool,
) -> Result<Vec<ot::AxisValue>> {
	let mut values = Vec::new();
	for location in locations {
		let value_name_id = add_name(font, &location.name, 0, windows, mac)?;
		let mut records = Vec::new();
		for &(tag, value) in &location.location {
			// The last axis with the tag, like the dict of fontTools.
			let index = axes
				.iter()
				.rposition(|a| a.tag == tag)
				.ok_or_else(|| Error::Value(format!("KeyError: '{tag}'")))?;
			records.push(ot::AxisValueRecord {
				axis_index: index as u16,
				value,
			});
		}
		records.sort_by_key(|r| r.axis_index);
		values.push(ot::AxisValue::Format4(ot::AxisValueFormat4 {
			axis_count: Some(records.len() as u32),
			flags: location.flags,
			value_name_id,
			axis_value_record: records,
		}));
	}
	Ok(values)
}

/// `_addName`: the name ID of a name, adding the name to the `name` table.
fn add_name(
	font: &mut Font,
	value: &StatName,
	min_name_id: u16,
	windows: bool,
	mac: bool,
) -> Result<u16> {
	let names: Vec<(&str, &str)> = match value {
		StatName::Id(id) => return Ok(*id),
		StatName::English(s) => vec![("en", s.as_str())],
		StatName::Multilingual(names) => names
			.iter()
			.map(|(l, s)| (l.as_str(), s.as_str()))
			.collect(),
		StatName::Records(records) => {
			let name = font.require::<Name>()?;
			let name_id = name.find_unused_name_id(256)?;
			for r in records {
				name.set_name(
					r.string.as_str(),
					name_id,
					r.platform_id,
					r.plat_enc_id,
					r.lang_id,
				);
			}
			return Ok(name_id);
		}
	};
	Ok(add_multilingual_name(
		font,
		&names,
		None,
		windows,
		mac,
		min_name_id,
	)?)
}
