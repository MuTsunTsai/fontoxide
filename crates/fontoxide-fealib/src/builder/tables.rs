//! Building the tables other than the lookups: `head`, `hhea`, `vhea`, `name`,
//! `OS/2`, `STAT`, `BASE`, `GDEF`, and the feature parameters.

use std::collections::{BTreeMap, HashMap, HashSet};

use fontoxide_misc::Tag;
use fontoxide_otllib::builder::{
	MaybeExtended, StatAxis, StatAxisValue, StatAxisValueKind, StatLocation, StatName,
	StatNameRecord, build_attach_list, build_lig_caret_list, build_mark_glyph_sets_def,
	build_stat_table,
};
use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::{Base, Gdef, Gpos, Gsub, Head, Hhea, Name, Os2, Vhea};
use indexmap::IndexMap;

use super::{BaseAxisData, Builder, NameKey, to_i16, to_tag, to_u16};
use crate::ast::{OS2Value, STATAxisValueStatement, STATDesignAxisStatement, STATNameStatement};
use crate::error::{Error, Result};
use crate::location::Location;

/// By convention, a missing name ID is represented by 0xFFFF. The specification
/// says that these fields can be NULL, but NULL is not well defined for name
/// IDs.
const NO_NAME_ID: u16 = 0xFFFF;

/// The Windows code pages and their bits in `ulCodePageRange`
/// (`build_codepages_`).
const PAGES_TO_BITS: [(i64, u32); 31] = [
	(1252, 0),
	(1250, 1),
	(1251, 2),
	(1253, 3),
	(1254, 4),
	(1255, 5),
	(1256, 6),
	(1257, 7),
	(1258, 8),
	(874, 16),
	(932, 17),
	(936, 18),
	(949, 19),
	(950, 20),
	(1361, 21),
	(869, 48),
	(866, 49),
	(865, 50),
	(864, 51),
	(863, 52),
	(862, 53),
	(861, 54),
	(860, 55),
	(857, 56),
	(855, 57),
	(852, 58),
	(775, 59),
	(737, 60),
	(708, 61),
	(850, 62),
	(437, 63),
];

/// The name records of a STAT name.
fn stat_records(names: &[STATNameStatement]) -> Result<StatName> {
	Ok(StatName::Records(
		names
			.iter()
			.map(|n| {
				Ok(StatNameRecord {
					string: n.string.clone(),
					platform_id: to_u16(n.platform_id)?,
					plat_enc_id: to_u16(n.plat_enc_id)?,
					lang_id: to_u16(n.lang_id)?,
				})
			})
			.collect::<Result<_>>()?,
	))
}

/// Maps the variation indices of the device tables (and of format 2 condition
/// tables) (`remap_device_varidxes`).
fn remap_device_varidxes<T: OtTable>(table: &mut T, map: &HashMap<u64, u64>) -> Result<()> {
	table.visit_mut(&mut |any| {
		if let Some(device) = any.downcast_mut::<ot::Device>() {
			if device.delta_format == 0x8000 {
				let old = (u64::from(device.start_size) << 16) + u64::from(device.end_size);
				let new = *map
					.get(&old)
					.ok_or_else(|| fontoxide_ttlib::Error::value(format!("KeyError: {old}")))?;
				device.start_size = (new >> 16) as u16;
				device.end_size = (new & 0xFFFF) as u16;
			}
		} else if let Some(c) = any.downcast_mut::<ot::ConditionTableFormat2>() {
			c.var_idx = map
				.get(&u64::from(c.var_idx))
				.map_or(0xFFFF_FFFF, |&v| v as u32);
		}
		Ok(())
	})?;
	Ok(())
}

impl Builder<'_> {
	/// Sets `fontRevision` in `head` (`build_head`).
	pub(super) fn build_head(&mut self) -> Result<()> {
		let Some(revision) = self.font_revision_.filter(|r| *r != 0.0) else {
			return Ok(());
		};
		if self.font.get::<Head>()?.is_none() {
			// This only happens for unit tests.
			let head = Head {
				table_version: 1.0,
				magic_number: 0x5F0F_3CF5,
				created: 3_406_620_153, // 2011-12-13 11:22:33
				modified: 3_406_620_153,
				..Default::default()
			};
			self.font.insert(Box::new(head));
		}
		self.font.require::<Head>()?.font_revision = revision;
		Ok(())
	}

	/// Sets the fields of `hhea` (`build_hhea`).
	pub(super) fn build_hhea(&mut self) -> Result<()> {
		if self.hhea_.is_empty() {
			return Ok(());
		}
		if self.font.get::<Hhea>()?.is_none() {
			// This only happens for unit tests.
			let hhea = Hhea {
				table_version: 0x0001_0000,
				..Default::default()
			};
			self.font.insert(Box::new(hhea));
		}
		let fields = self.hhea_.clone();
		let table = self.font.require::<Hhea>()?;
		if let Some(&v) = fields.get("caretoffset") {
			table.caret_offset = to_i16(v)?;
		}
		if let Some(&v) = fields.get("ascender") {
			table.ascent = to_i16(v)?;
		}
		if let Some(&v) = fields.get("descender") {
			table.descent = to_i16(v)?;
		}
		if let Some(&v) = fields.get("linegap") {
			table.line_gap = to_i16(v)?;
		}
		Ok(())
	}

	/// Sets the fields of `vhea` (`build_vhea`).
	pub(super) fn build_vhea(&mut self) -> Result<()> {
		if self.vhea_.is_empty() {
			return Ok(());
		}
		if self.font.get::<Vhea>()?.is_none() {
			// This only happens for unit tests.
			let vhea = Vhea {
				table_version: 0x0001_1000,
				..Default::default()
			};
			self.font.insert(Box::new(vhea));
		}
		let fields = self.vhea_.clone();
		let table = self.font.require::<Vhea>()?;
		if let Some(&v) = fields.get("verttypoascender") {
			table.ascent = to_i16(v)?;
		}
		if let Some(&v) = fields.get("verttypodescender") {
			table.descent = to_i16(v)?;
		}
		if let Some(&v) = fields.get("verttypolinegap") {
			table.line_gap = to_i16(v)?;
		}
		Ok(())
	}

	/// The feature parameters of a feature (`buildFeatureParams`).
	pub(super) fn build_feature_params(&self, tag: &str) -> Result<Option<ot::FeatureParamsAny>> {
		if tag == "size" {
			let (design_size, subfamily_id, range_start, range_end) =
				self.size_parameters_.ok_or_else(|| {
					Error::Value("TypeError: cannot unpack non-iterable NoneType object".to_owned())
				})?;
			return Ok(Some(ot::FeatureParamsAny::FeatureParamsSize(
				ot::FeatureParamsSize {
					design_size,
					subfamily_id: to_u16(subfamily_id)?,
					subfamily_name_id: self.feature_names_ids_.get(tag).copied().unwrap_or(0),
					range_start,
					range_end,
				},
			)));
		}
		if self.feature_names_.contains(tag) {
			// If the name table wasn't selected among the tables to build, skip.
			if self.feature_names_ids_.is_empty() {
				return Ok(None);
			}
			let ui_name_id = *self
				.feature_names_ids_
				.get(tag)
				.ok_or_else(|| Error::Assertion("tag in self.featureNames_ids_".to_owned()))?;
			return Ok(Some(ot::FeatureParamsAny::FeatureParamsStylisticSet(
				ot::FeatureParamsStylisticSet {
					version: 0,
					ui_name_id,
				},
			)));
		}
		if self.cv_parameters_.contains(tag) {
			let id = |block: &str| {
				self.cv_parameters_ids_
					.get(&(tag.to_owned(), block.to_owned()))
					.copied()
					.unwrap_or(NO_NAME_ID)
			};
			let character = self
				.cv_characters_
				.get(tag)
				.map(|c| {
					c.iter()
						.map(|&c| {
							u32::try_from(c)
								.ok()
								.filter(|c| *c <= 0xFF_FFFF)
								.ok_or_else(|| Error::Value(format!("uint24 out of range: {c}")))
						})
						.collect::<Result<Vec<u32>>>()
				})
				.transpose()?
				.unwrap_or_default();
			return Ok(Some(ot::FeatureParamsAny::FeatureParamsCharacterVariants(
				ot::FeatureParamsCharacterVariants {
					format: 0,
					feat_ui_label_name_id: id("FeatUILabelNameID"),
					feat_ui_tooltip_text_name_id: id("FeatUITooltipTextNameID"),
					sample_text_name_id: id("SampleTextNameID"),
					num_named_parameters: to_u16(
						self.cv_num_named_params_.get(tag).copied().unwrap_or(0),
					)?,
					first_param_ui_label_name_id: id("ParamUILabelNameID_0"),
					char_count: Some(character.len() as u32),
					character,
				},
			)));
		}
		Ok(None)
	}

	/// Adds the name records (`build_name`), giving the names of features
	/// unused font-specific name IDs.
	pub(super) fn build_name(&mut self) -> Result<()> {
		if self.names_.is_empty() {
			return Ok(());
		}
		if self.font.get::<Name>()?.is_none() {
			// This only happens for unit tests.
			self.font.insert(Box::new(Name::default()));
		}
		let names = self.names_.clone();
		for name in names {
			// For featureNames blocks, nameID is the feature tag; for
			// cvParameters blocks, it is the feature tag and the block name.
			let table = self.font.require::<Name>()?;
			let name_id = match &name.name_id {
				NameKey::Id(id) => to_u16(*id)?,
				NameKey::Feature(tag) if self.feature_names_.contains(tag) => {
					match self.feature_names_ids_.get(tag) {
						Some(&id) => id,
						None => {
							let id = get_user_name_id(table)?;
							self.feature_names_ids_.insert(tag.clone(), id);
							id
						}
					}
				}
				NameKey::Cv(tag, block) if self.cv_parameters_.contains(tag) => {
					let key = (tag.clone(), block.clone());
					match self.cv_parameters_ids_.get(&key) {
						Some(&id) => id,
						None => {
							let id = get_user_name_id(table)?;
							self.cv_parameters_ids_.insert(key, id);
							id
						}
					}
				}
				NameKey::Feature(tag) | NameKey::Cv(tag, _) => {
					return Err(Error::Value(format!(
						"TypeError: name ID {} is not an integer",
						fontoxide_misc::py::str_repr(tag)
					)));
				}
			};
			table.set_name(
				name.string.clone(),
				name_id,
				to_u16(name.platform_id)?,
				to_u16(name.plat_enc_id)?,
				to_u16(name.lang_id)?,
			);
		}
		self.font.require::<Name>()?.sort();
		Ok(())
	}

	/// Sets the fields of `OS/2` (`build_OS_2`).
	pub(super) fn build_os_2(&mut self) -> Result<()> {
		if self.os2_.is_empty() {
			return Ok(());
		}
		if self.font.get::<Os2>()?.is_none() {
			// This only happens for unit tests.
			self.font.insert(Box::new(Os2::default()));
		}
		let fields = self.os2_.clone();
		let table = self.font.require::<Os2>()?;
		let int = |key: &str| -> Result<Option<i64>> {
			match fields.get(key) {
				None => Ok(None),
				Some(OS2Value::Int(i)) => Ok(Some(*i)),
				Some(v) => Err(Error::Value(format!("invalid value for {key}: {v:?}"))),
			}
		};
		let array = |key: &str| -> Result<Option<Vec<i64>>> {
			match fields.get(key) {
				None => Ok(None),
				Some(OS2Value::IntArray(a)) => Ok(Some(a.clone())),
				Some(v) => Err(Error::Value(format!("invalid value for {key}: {v:?}"))),
			}
		};
		let mut version = 0;
		if let Some(v) = int("fstype")? {
			table.fs_type = to_u16(v)?;
		}
		if let Some(p) = array("panose")? {
			let [a, b, c, d, e, f, g, h, i, j] = p.as_slice() else {
				return Err(Error::Value(format!(
					"ValueError: expected 10 values to unpack (got {})",
					p.len()
				)));
			};
			let byte = |v: &i64| {
				u8::try_from(*v).map_err(|_| {
					Error::Value(format!("ubyte format requires 0 <= number <= 255: {v}"))
				})
			};
			table.panose = fontoxide_ttlib::tables::os2::Panose {
				b_family_type: byte(a)?,
				b_serif_style: byte(b)?,
				b_weight: byte(c)?,
				b_proportion: byte(d)?,
				b_contrast: byte(e)?,
				b_stroke_variation: byte(f)?,
				b_arm_style: byte(g)?,
				b_letter_form: byte(h)?,
				b_midline: byte(i)?,
				b_x_height: byte(j)?,
			};
		}
		if let Some(v) = int("typoascender")? {
			table.s_typo_ascender = to_i16(v)?;
		}
		if let Some(v) = int("typodescender")? {
			table.s_typo_descender = to_i16(v)?;
		}
		if let Some(v) = int("typolinegap")? {
			table.s_typo_line_gap = to_i16(v)?;
		}
		if let Some(v) = int("winascent")? {
			table.us_win_ascent = to_u16(v)?;
		}
		if let Some(v) = int("windescent")? {
			table.us_win_descent = to_u16(v)?;
		}
		if let Some(vendor) = fields.get("vendor") {
			// A Tag is padded with trailing spaces, not NULs (see #3280).
			let OS2Value::Str(vendor) = vendor else {
				return Err(Error::Value(format!(
					"invalid value for vendor: {vendor:?}"
				)));
			};
			let vendor = fontoxide_misc::text_tools::parse_string_body(vendor)
				.ok_or_else(|| Error::Value(format!("invalid vendor: {vendor}")))?;
			let mut bytes: Vec<u8> = vendor
				.chars()
				.map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
				.collect();
			while bytes.len() < 4 {
				bytes.push(b' ');
			}
			table.ach_vend_id = [bytes[0], bytes[1], bytes[2], bytes[3]];
		}
		if let Some(v) = int("weightclass")? {
			table.us_weight_class = to_u16(v)?;
		}
		if let Some(v) = int("widthclass")? {
			table.us_width_class = to_u16(v)?;
		}
		if let Some(bits) = array("unicoderange")? {
			let bits = bits
				.iter()
				.map(|&b| {
					u32::try_from(b)
						.map_err(|_| Error::Value(format!("expected 0 <= int <= 127: {b}")))
				})
				.collect::<Result<Vec<u32>>>()?;
			table.set_unicode_ranges(bits)?;
		}
		if let Some(pages) = array("codepagerange")? {
			let mut ranges = [0u32; 2];
			for page in pages {
				if let Some(&(_, bit)) = PAGES_TO_BITS.iter().find(|(p, _)| *p == page) {
					ranges[(bit / 32) as usize] |= 1 << (bit % 32);
				}
			}
			table.ul_code_page_range1 = ranges[0];
			table.ul_code_page_range2 = ranges[1];
			version = 1;
		}
		if let Some(v) = int("xheight")? {
			table.sx_height = to_i16(v)?;
			version = 2;
		}
		if let Some(v) = int("capheight")? {
			table.s_cap_height = to_i16(v)?;
			version = 2;
		}
		if let Some(v) = int("loweropsize")? {
			table.us_lower_optical_point_size = v as f64;
			version = 5;
		}
		if let Some(v) = int("upperopsize")? {
			table.us_upper_optical_point_size = v as f64;
			version = 5;
		}
		// The fields of the higher versions are there already (as zeros).
		table.base.version = table.base.version.max(version);
		Ok(())
	}

	/// `setElidedFallbackName`: only one of `ElidedFallbackName` and
	/// `ElidedFallbackNameID` can be set.
	pub fn set_elided_fallback_name_id(&mut self, value: i64, location: &Location) -> Result<()> {
		self.check_elided_fallback_name(location)?;
		self.stat_.elided_fallback_name_id = Some(value);
		Ok(())
	}

	/// `setElidedFallbackName` with name records.
	pub fn set_elided_fallback_name(
		&mut self,
		value: Vec<STATNameStatement>,
		location: &Location,
	) -> Result<()> {
		self.check_elided_fallback_name(location)?;
		self.stat_.elided_fallback_name = Some(value);
		Ok(())
	}

	fn check_elided_fallback_name(&self, location: &Location) -> Result<()> {
		// ElidedFallbackName is a convenience method for setting
		// ElidedFallbackNameID so only one can be allowed.
		let set = [
			(
				"ElidedFallbackName",
				self.stat_.elided_fallback_name.is_some(),
			),
			(
				"ElidedFallbackNameID",
				self.stat_.elided_fallback_name_id.is_some(),
			),
		];
		for (token, is_set) in set {
			if is_set {
				return Err(Error::feature_lib(
					format!("{token} is already set."),
					location.clone(),
				));
			}
		}
		Ok(())
	}

	/// A STAT `DesignAxis` (`addDesignAxis`).
	pub fn add_design_axis(
		&mut self,
		design_axis: &STATDesignAxisStatement,
		location: &Location,
	) -> Result<()> {
		let axes = self.stat_.design_axes.get_or_insert_with(Vec::new);
		if axes.iter().any(|r| r.tag == design_axis.tag) {
			return Err(Error::feature_lib(
				format!(
					"DesignAxis already defined for tag \"{}\".",
					design_axis.tag
				),
				location.clone(),
			));
		}
		if axes.iter().any(|r| r.axis_order == design_axis.axis_order) {
			return Err(Error::feature_lib(
				format!(
					"DesignAxis already defined for axis number {}.",
					design_axis.axis_order
				),
				location.clone(),
			));
		}
		axes.push(design_axis.clone());
		Ok(())
	}

	/// A STAT `AxisValue` (`addAxisValueRecord`).
	pub fn add_axis_value_record(
		&mut self,
		axis_value_record: &STATAxisValueStatement,
		location: &Location,
	) -> Result<()> {
		let records = self.stat_.axis_value_records.get_or_insert_with(Vec::new);
		let names = |r: &STATAxisValueStatement| -> HashSet<String> {
			r.names.iter().map(STATNameStatement::as_fea).collect()
		};
		let locations = |r: &STATAxisValueStatement| -> HashSet<String> {
			r.locations.iter().map(|l| l.as_fea("")).collect()
		};
		// Check for duplicate AxisValueRecords.
		for record in records.iter() {
			if names(record) == names(axis_value_record)
				&& locations(record) == locations(axis_value_record)
				&& record.flags == axis_value_record.flags
			{
				return Err(Error::feature_lib(
					"An AxisValueRecord with these values is already defined.",
					location.clone(),
				));
			}
		}
		records.push(axis_value_record.clone());
		Ok(())
	}

	/// Builds the `STAT` table (`build_STAT`).
	pub(super) fn build_stat(&mut self) -> Result<()> {
		if self.stat_.is_empty() {
			return Ok(());
		}
		let axes = self
			.stat_
			.design_axes
			.clone()
			.filter(|a| !a.is_empty())
			.ok_or_else(|| Error::feature_lib("DesignAxes not defined", None))?;
		let mut axis_values: IndexMap<String, Vec<StatAxisValue>> =
			axes.iter().map(|a| (a.tag.clone(), Vec::new())).collect();
		let mut format4_locations = Vec::new();
		if let Some(records) = &self.stat_.axis_value_records {
			for avr in records {
				let flags = if avr.flags > 0 { to_u16(avr.flags)? } else { 0 };
				let name = stat_records(&avr.names)?;
				if let [location] = avr.locations.as_slice() {
					let values: Vec<f64> = location.values.iter().map(|v| v.as_f64()).collect();
					let kind = match values.as_slice() {
						// format 1
						[value] => StatAxisValueKind::Value {
							value: *value,
							linked_value: None,
						},
						// format 3
						[value, linked] => StatAxisValueKind::Value {
							value: *value,
							linked_value: Some(*linked),
						},
						// format 2
						[nominal, min, max] => StatAxisValueKind::Range {
							nominal_value: *nominal,
							range_min_value: Some(*min),
							range_max_value: Some(*max),
						},
						_ => return Err(Error::Value("KeyError: 'name'".to_owned())),
					};
					axis_values
						.get_mut(&location.tag)
						.ok_or_else(|| {
							Error::Value(format!(
								"KeyError: {}",
								fontoxide_misc::py::str_repr(&location.tag)
							))
						})?
						.push(StatAxisValue { name, flags, kind });
				} else {
					let mut loc: Vec<(Tag, f64)> = Vec::new();
					for l in &avr.locations {
						let tag = to_tag(&l.tag)?;
						let value = l
							.values
							.first()
							.ok_or_else(|| {
								Error::Value("IndexError: list index out of range".to_owned())
							})?
							.as_f64();
						match loc.iter_mut().find(|(t, _)| *t == tag) {
							Some(entry) => entry.1 = value,
							None => loc.push((tag, value)),
						}
					}
					format4_locations.push(StatLocation {
						name,
						flags,
						location: loc,
					});
				}
			}
		}
		let design_axes = axes
			.iter()
			.map(|a| {
				Ok(StatAxis {
					tag: to_tag(&a.tag)?,
					name: stat_records(&a.names)?,
					ordering: Some(to_u16(a.axis_order)?),
					values: axis_values.get(&a.tag).cloned().unwrap_or_default(),
				})
			})
			.collect::<Result<Vec<_>>>()?;
		if self.font.get::<Name>()?.is_none() {
			// This only happens for unit tests.
			self.font.insert(Box::new(Name::default()));
		}
		let elided_fallback_name = if let Some(name_id) = self.stat_.elided_fallback_name_id {
			let name = u16::try_from(name_id).ok().and_then(|id| {
				self.font
					.loaded::<Name>()
					.and_then(|n| n.get_debug_name(id))
			});
			if name.is_none_or(|n| n.is_empty()) {
				return Err(Error::feature_lib(
					format!(
						"ElidedFallbackNameID {name_id} points to a nameID that does not exist in the \"name\" table"
					),
					None,
				));
			}
			StatName::Id(to_u16(name_id)?)
		} else if let Some(names) = &self.stat_.elided_fallback_name {
			stat_records(names)?
		} else {
			return Err(Error::feature_lib(
				"STAT table requires an ElidedFallbackName or ElidedFallbackNameID",
				None,
			));
		};
		build_stat_table(
			self.font,
			&design_axes,
			Some(&format4_locations),
			&elided_fallback_name,
			true,
			true,
		)?;
		Ok(())
	}

	/// Builds the `BASE` table (`buildBASE`).
	pub(super) fn build_base(&self) -> Result<Option<Base>> {
		if self.base_horiz_axis_.is_none() && self.base_vert_axis_.is_none() {
			return Ok(None);
		}
		Ok(Some(Base {
			table: ot::BASE {
				version: 0x0001_0000,
				horiz_axis: build_base_axis(self.base_horiz_axis_.as_ref())?.map(Box::new),
				vert_axis: build_base_axis(self.base_vert_axis_.as_ref())?.map(Box::new),
				var_store: None,
			},
		}))
	}

	/// Builds the `GDEF` table, or `None` if it would be empty (`buildGDEF`).
	pub(super) fn build_gdef(&mut self) -> Result<Option<Gdef>> {
		let gm = self.glyph_map.clone();
		let mut gdef = ot::GDEF {
			glyph_class_def: self.build_gdef_glyph_class_def_()?.map(Box::new),
			..Default::default()
		};
		let attach_points: IndexMap<String, Vec<u16>> = self
			.attach_points_
			.iter()
			.map(|(g, points)| {
				Ok((
					g.clone(),
					points
						.iter()
						.map(|&p| to_u16(p))
						.collect::<Result<Vec<u16>>>()?,
				))
			})
			.collect::<Result<_>>()?;
		gdef.attach_list = build_attach_list(&attach_points, &gm)?.map(Box::new);
		let lig_caret_points: IndexMap<String, Vec<u16>> = self
			.lig_caret_points_
			.iter()
			.map(|(g, points)| {
				Ok((
					g.clone(),
					points
						.iter()
						.map(|&p| to_u16(p))
						.collect::<Result<Vec<u16>>>()?,
				))
			})
			.collect::<Result<_>>()?;
		match build_lig_caret_list(&self.lig_caret_coords_, &lig_caret_points, &gm)? {
			Some(MaybeExtended::Normal(l)) => gdef.lig_caret_list = Some(Box::new(l)),
			Some(MaybeExtended::Extended(l)) => gdef.lig_caret_list2 = Some(Box::new(l)),
			None => {}
		}
		gdef.mark_attach_class_def = self.build_gdef_mark_attach_class_def_()?.map(Box::new);
		gdef.mark_glyph_sets_def = self.build_gdef_mark_glyph_sets_def_()?.map(Box::new);
		gdef.version = if gdef.mark_glyph_sets_def.is_some() {
			0x0001_0002
		} else {
			0x0001_0000
		};
		if let Some(builder) = self.varstorebuilder.take() {
			let mut store = builder.finish(true);
			if !store.var_data.is_empty() {
				gdef.version = 0x0001_0003;
				let varidx_map = fontoxide_ttlib::ot::var_store::optimize(&mut store, true)?;
				gdef.var_store = Some(Box::new(store));
				remap_device_varidxes(&mut gdef, &varidx_map)?;
				if let Some(gsub) = self.font.get::<Gsub>()? {
					remap_device_varidxes(&mut gsub.table, &varidx_map)?;
				}
				if let Some(gpos) = self.font.get::<Gpos>()? {
					remap_device_varidxes(&mut gpos.table, &varidx_map)?;
				}
			}
		}
		let has_data = gdef.glyph_class_def.is_some()
			|| gdef.attach_list.is_some()
			|| gdef.lig_caret_list.is_some()
			|| gdef.lig_caret_list2.is_some()
			|| gdef.mark_attach_class_def.is_some()
			|| gdef.mark_glyph_sets_def.is_some()
			|| gdef.var_store.is_some();
		if !has_data {
			return Ok(None);
		}
		if self.has_extended_glyph_ids() {
			// promoteGDEF_
			gdef.version = 0x0001_0004;
			gdef.glyph_class_def2 = gdef.glyph_class_def.take();
			gdef.attach_list2 = gdef.attach_list.take();
			gdef.mark_attach_class_def2 = gdef.mark_attach_class_def.take();
			gdef.mark_glyph_sets_def2 = gdef.mark_glyph_sets_def.take();
		}
		Ok(Some(Gdef { table: gdef }))
	}

	/// A class definition from glyph names to classes.
	fn class_def(&self, classes: &IndexMap<String, u16>) -> Result<ot::ClassDef> {
		let mut class_defs = BTreeMap::new();
		for (glyph, class) in classes {
			class_defs.insert(self.glyph_map.id(glyph)?, u32::from(*class));
		}
		Ok(ot::ClassDef { class_defs })
	}

	/// `buildGDEFGlyphClassDef_`: the explicit glyph classes, or the classes
	/// that the lookups and mark classes imply.
	fn build_gdef_glyph_class_def_(&mut self) -> Result<Option<ot::ClassDef>> {
		let classes: IndexMap<String, u16> = if !self.glyph_class_defs_.is_empty() {
			self.glyph_class_defs_
				.iter()
				.map(|(g, (c, _))| (g.clone(), *c))
				.collect()
		} else {
			let mut classes = IndexMap::new();
			for lookup in &self.lookups_ {
				classes.extend(lookup.borrow_mut().infer_glyph_classes());
			}
			if let Some(tree) = &self.parse_tree {
				for mark_class in tree.mark_classes.values() {
					for mark_class_def in &mark_class.borrow().definitions {
						for glyph in mark_class_def.glyph_set().iter() {
							classes.insert(glyph.clone(), 3);
						}
					}
				}
			}
			classes
		};
		if classes.is_empty() {
			return Ok(None);
		}
		Ok(Some(self.class_def(&classes)?))
	}

	/// `buildGDEFMarkAttachClassDef_`.
	fn build_gdef_mark_attach_class_def_(&self) -> Result<Option<ot::ClassDef>> {
		let class_defs: IndexMap<String, u16> = self
			.mark_attach_
			.iter()
			.map(|(g, (c, _))| (g.clone(), *c))
			.collect();
		if class_defs.is_empty() {
			return Ok(None);
		}
		Ok(Some(self.class_def(&class_defs)?))
	}

	/// `buildGDEFMarkGlyphSetsDef_`: the mark filtering sets by ID.
	fn build_gdef_mark_glyph_sets_def_(&self) -> Result<Option<ot::MarkGlyphSetsDef>> {
		let mut sets: Vec<(&Vec<String>, u16)> = self
			.mark_filter_sets_
			.iter()
			.map(|(g, &id)| (g, id))
			.collect();
		sets.sort_by_key(|&(_, id)| id);
		let sets: Vec<Vec<String>> = sets.into_iter().map(|(g, _)| g.clone()).collect();
		Ok(build_mark_glyph_sets_def(&sets, &self.glyph_map)?)
	}
}

/// The first unused font-specific name ID (`get_user_name_id`).
fn get_user_name_id(table: &Name) -> Result<u16> {
	let name_ids: HashSet<u16> = table.names.iter().map(|n| n.name_id).collect();
	(256..32767)
		.find(|id| !name_ids.contains(id))
		.ok_or_else(|| Error::Assertion("self.featureNames_ids_[tag] is not None".to_owned()))
}

/// `buildBASECoord`.
fn build_base_coord(c: i64) -> Result<ot::BaseCoord> {
	Ok(ot::BaseCoord::Format1(ot::BaseCoordFormat1 {
		coordinate: to_i16(c)?,
	}))
}

/// `buildBASEAxis`.
fn build_base_axis(axis: Option<&BaseAxisData>) -> Result<Option<ot::Axis>> {
	let Some((bases, scripts, minmax)) = axis else {
		return Ok(None);
	};
	let baseline_tag = bases
		.iter()
		.map(|b| to_tag(b))
		.collect::<Result<Vec<_>>>()?;
	let mut base_script_list = ot::BaseScriptList {
		base_script_count: Some(scripts.len() as u32),
		base_script_record: Vec::new(),
	};
	let mut sorted_scripts = scripts.clone();
	sorted_scripts.sort();
	for (script_tag, default_baseline, coords) in sorted_scripts {
		let mut minmax_for_script: Vec<(String, i64, i64)> = minmax
			.iter()
			.filter(|record| record.0 == script_tag)
			.map(|record| (record.1.clone(), record.2, record.3))
			.collect();
		minmax_for_script.sort();
		let default_index = bases
			.iter()
			.position(|b| *b == default_baseline)
			.ok_or_else(|| {
				Error::Value(format!(
					"{} is not in list",
					fontoxide_misc::py::str_repr(&default_baseline)
				))
			})?;
		let mut base_script = ot::BaseScript {
			base_values: Some(Box::new(ot::BaseValues {
				default_index: to_u16(default_index as i64)?,
				base_coord_count: Some(coords.len() as u32),
				base_coord: coords
					.iter()
					.map(|&c| Ok(Some(Box::new(build_base_coord(c)?))))
					.collect::<Result<_>>()?,
			})),
			..Default::default()
		};
		for (language, min_coord, max_coord) in minmax_for_script {
			let minmax_record = ot::MinMax {
				min_coord: Some(Box::new(build_base_coord(min_coord)?)),
				max_coord: Some(Box::new(build_base_coord(max_coord)?)),
				feat_min_max_count: Some(0),
				feat_min_max_record: Vec::new(),
			};
			if language == "dflt" {
				base_script.default_min_max = Some(Box::new(minmax_record));
			} else {
				base_script
					.base_lang_sys_record
					.push(ot::BaseLangSysRecord {
						base_lang_sys_tag: to_tag(&language)?,
						min_max: Some(Box::new(minmax_record)),
					});
			}
		}
		base_script.base_lang_sys_count = Some(base_script.base_lang_sys_record.len() as u32);
		base_script_list
			.base_script_record
			.push(ot::BaseScriptRecord {
				base_script_tag: to_tag(&script_tag)?,
				base_script: Some(Box::new(base_script)),
			});
	}
	Ok(Some(ot::Axis {
		base_tag_list: Some(Box::new(ot::BaseTagList {
			base_tag_count: Some(baseline_tag.len() as u32),
			baseline_tag,
		})),
		base_script_list: Some(Box::new(base_script_list)),
	}))
}
