//! Port of `fontTools.varLib.cff`: building `CFF2` from CFF or CFF2 masters.
//!
//! The charstrings of the masters are drawn with their hints into a
//! [`Cff2CharStringMergePen`], which turns the coordinates that differ into
//! `blend` arguments; the blue zones and stems of the Private DICTs are blended
//! too, and the regions go into the `VarStore` of the Top DICT.

use std::collections::HashMap;

use fontoxide_misc::Tag;
use fontoxide_misc::transform::Transform;
use fontoxide_pens::{Pen, Point};
use fontoxide_ttlib::Font;
use fontoxide_ttlib::cff::cff_to_cff2::convert_cff_to_cff2;
use fontoxide_ttlib::cff::charstring::{HintPen, T2CharString, Token};
use fontoxide_ttlib::cff::dict::Value;
use fontoxide_ttlib::cff::font::{CffFontSet, PrivateDict, TopDict};
use fontoxide_ttlib::cff::specializer::{
	Arg, Command, SpecializeOptions, commands_to_program, specialize_commands,
};
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::{Cff, Cff2, Fvar};

use crate::builder::{build_var_data, build_var_region_list, build_var_store};
use crate::errors::{Result, VarLibError};
use crate::models::{Support, VariationModel};

/// `maxStackLimit`.
const MAX_STACK_LIMIT: i64 = 513;

const CFF: Tag = Tag::new(b"CFF ");
const CFF2: Tag = Tag::new(b"CFF2");

fn ttlib_error(message: impl Into<String>) -> VarLibError {
	fontoxide_ttlib::Error::ttlib(message).into()
}

fn list_index_error() -> VarLibError {
	ttlib_error("IndexError: list index out of range")
}

fn key_error(key: impl std::fmt::Display) -> VarLibError {
	ttlib_error(format!("KeyError: {key}"))
}

fn model_error(e: fontoxide_ttlib::Error) -> VarLibError {
	let message = e.to_string();
	match message.strip_prefix("VariationModelError: ") {
		Some(m) => VarLibError::VariationModel(m.to_owned()),
		None => e.into(),
	}
}

/// `_add_CFF2`: converts the default master's `CFF ` to `CFF2` if needed, and
/// merges the charstrings and Private DICTs of the masters into it.
///
/// As in fontTools, the model is left with the masters in the order of its
/// locations (`reorderMasters`).
pub fn add_cff2(font: &mut Font, model: &mut VariationModel, masters: &mut [Font]) -> Result<()> {
	let glyph_order = font.glyph_order()?.names().to_vec();
	if !font.contains(CFF2) {
		convert_cff_to_cff2(font)?;
	}
	let indices: Vec<usize> = (0..masters.len()).collect();
	let reverse_mapping = model.reverse_mapping.clone();
	// Re-ordering the master list simplifies building the CFF2 data item lists.
	let order = model.reorder_masters(&indices, &reverse_mapping);
	merge_region_fonts(font, model, masters, &order, &glyph_order)
}

/// Takes the CFF font set out of a font (`_cff_or_cff2`), to put it back with
/// [`put_cff`].
fn take_cff(font: &mut Font) -> Result<(Tag, CffFontSet)> {
	if font.contains(CFF) {
		Ok((CFF, std::mem::take(&mut font.require::<Cff>()?.cff)))
	} else {
		Ok((CFF2, std::mem::take(&mut font.require::<Cff2>()?.cff)))
	}
}

fn put_cff(font: &mut Font, tag: Tag, cff: CffFontSet) -> Result<()> {
	if tag == CFF {
		font.require::<Cff>()?.cff = cff;
	} else {
		font.require::<Cff2>()?.cff = cff;
	}
	Ok(())
}

/// A master's CFF font set and glyph order.
struct MasterCff {
	cff: CffFontSet,
	glyph_order: Vec<String>,
	/// Glyph name to charstring index.
	index: HashMap<String, usize>,
}

impl MasterCff {
	fn new(cff: CffFontSet, glyph_order: Vec<String>) -> Self {
		let index = cff
			.top_dicts
			.first()
			.and_then(|t| t.char_strings.as_ref())
			.map(|cs| {
				let mut index = HashMap::with_capacity(cs.names.len());
				for (i, n) in cs.names.iter().enumerate() {
					index.entry(n.clone()).or_insert(i);
				}
				index
			})
			.unwrap_or_default();
		MasterCff {
			cff,
			glyph_order,
			index,
		}
	}

	fn top(&self) -> Result<&TopDict> {
		self.cff.top_dicts.first().ok_or_else(list_index_error)
	}
}

/// `merge_region_fonts`.
fn merge_region_fonts(
	font: &mut Font,
	model: &VariationModel,
	masters: &mut [Font],
	order: &[usize],
	glyph_order: &[String],
) -> Result<()> {
	let mut ordered: Vec<MasterCff> = Vec::with_capacity(order.len());
	let mut taken: Vec<(usize, Tag)> = Vec::with_capacity(order.len());
	let mut result = Ok(());
	for &i in order {
		let master = masters.get_mut(i).ok_or_else(list_index_error)?;
		let names = master.glyph_order()?.names().to_vec();
		match take_cff(master) {
			Ok((tag, cff)) => {
				taken.push((i, tag));
				ordered.push(MasterCff::new(cff, names));
			}
			Err(e) => {
				result = Err(e);
				break;
			}
		}
	}
	let axis_keys: Vec<String> = font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| Tag::new(&a.axis_tag).to_string())
		.collect();
	let vf_names = font.glyph_order()?.names().to_vec();
	let (_, vf_cff) = take_cff(font)?;
	let mut vf = MasterCff::new(vf_cff, vf_names);
	if result.is_ok() {
		result = merge_sets(model, &mut vf, &mut ordered, glyph_order, &axis_keys);
	}
	put_cff(font, CFF2, vf.cff)?;
	for ((i, tag), m) in taken.into_iter().zip(ordered) {
		put_cff(&mut masters[i], tag, m.cff)?;
	}
	result
}

fn merge_sets(
	model: &VariationModel,
	vf: &mut MasterCff,
	ordered: &mut [MasterCff],
	glyph_order: &[String],
	axis_keys: &[String],
) -> Result<()> {
	let num_masters = model.mapping.len();
	let regions = ordered.get_mut(1..).unwrap_or_default();
	let cv_data = merge_charstrings(glyph_order, num_masters, vf, regions, model)?;
	let fd_map = getfd_map(ordered)?;
	let regions = ordered.get(1..).unwrap_or_default();
	merge_private_dicts(vf, regions, &cv_data.vsindex_dict, model, &fd_map)?;
	add_cff_var_store(
		vf,
		cv_data.var_data_list,
		&cv_data.master_supports,
		axis_keys,
	)
}

/// `addCFFVarStore`.
fn add_cff_var_store(
	vf: &mut MasterCff,
	var_data_list: Vec<ot::VarData>,
	master_supports: &[Support],
	axis_keys: &[String],
) -> Result<()> {
	let supports: Vec<_> = master_supports
		.iter()
		.map(|s| s.iter().cloned().collect())
		.collect();
	let var_tuple_list = build_var_region_list(&supports, axis_keys)?;
	let store = build_var_store(var_tuple_list, var_data_list);
	let top = vf.cff.top_dicts.first_mut().ok_or_else(list_index_error)?;
	top.var_store = Some(Box::new(store));
	Ok(())
}

/// `pd_blend_fields`.
const PD_BLEND_FIELDS: &[&str] = &[
	"BlueValues",
	"OtherBlues",
	"FamilyBlues",
	"FamilyOtherBlues",
	"BlueScale",
	"BlueShift",
	"BlueFuzz",
	"StdHW",
	"StdVW",
	"StemSnapH",
	"StemSnapV",
];

/// The value of a number of a DICT.
fn number(v: &Value) -> Result<f64> {
	v.number()
		.ok_or_else(|| ttlib_error(format!("TypeError: unsupported operand type: {v:?}")))
}

/// `conv_to_int`: numbers with no decimal part as `int`s, as all the numbers
/// computed here end up.
fn conv_to_int(v: f64) -> Value {
	if v.is_finite() && v.fract() == 0.0 && v.abs() < 9.0e18 {
		Value::Int(v as i64)
	} else {
		Value::Float(v)
	}
}

/// `allEqual`.
fn all_equal(values: &[f64]) -> bool {
	values.windows(2).all(|w| w[0] == w[1])
}

/// Where a Private DICT of [`merge_private_dicts`] is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PdRef {
	/// The variable font's.
	Vf,
	/// A region font's, by region index and font DICT index.
	Region(usize, usize),
}

/// `merge_PrivateDicts`: blends the values of the Private DICTs.
fn merge_private_dicts(
	vf: &mut MasterCff,
	regions: &[MasterCff],
	vsindex_dict: &[(VariationModel, Vec<Vec<bool>>)],
	var_model: &VariationModel,
	fd_map: &FdMap,
) -> Result<()> {
	let first = regions.first().ok_or_else(list_index_error)?;
	let use_fd_array = first.top()?.fd_array.is_some();
	let region_private = |ri: usize, fd: usize| -> Result<&PrivateDict> {
		let top = regions.get(ri).ok_or_else(list_index_error)?.top()?;
		let no_private =
			|| ttlib_error("AttributeError: 'FontDict' object has no attribute 'Private'");
		if use_fd_array {
			let fds = top
				.fd_array
				.as_ref()
				.ok_or_else(|| ttlib_error("AttributeError: FDArray"))?;
			fds.get(fd)
				.ok_or_else(list_index_error)?
				.private
				.as_deref()
				.ok_or_else(no_private)
		} else if fd == 0 {
			top.private.as_deref().ok_or_else(no_private)
		} else {
			Err(list_index_error())
		}
	};

	let top = vf.cff.top_dicts.first_mut().ok_or_else(list_index_error)?;
	let fds = top
		.fd_array
		.as_mut()
		.ok_or_else(|| ttlib_error("AttributeError: FDArray"))?;
	for (fd_index, font_dict) in fds.iter_mut().enumerate() {
		let private_dict = font_dict.private.as_deref_mut().ok_or_else(|| {
			ttlib_error("AttributeError: 'FontDict' object has no attribute 'Private'")
		})?;
		let vsindex = match private_dict.get("vsindex") {
			Some(v) => v.int()?,
			None => 0,
		};
		let (sub_model, _) = usize::try_from(vsindex)
			.ok()
			.and_then(|i| vsindex_dict.get(i))
			.ok_or_else(|| key_error(vsindex))?;
		let mut master_indices = Vec::new();
		for loc in sub_model.locations.iter().skip(1) {
			let i = var_model
				.locations
				.iter()
				.position(|l| same_location(l, loc))
				.ok_or_else(|| ttlib_error("ValueError: location is not in list"))?;
			master_indices.push(i.wrapping_sub(1));
		}
		let mut pds = vec![PdRef::Vf];
		let mut last_pd = PdRef::Vf;
		let region_fd_map = fd_map
			.get(&(fd_index as i64))
			.ok_or_else(|| key_error(fd_index))?;
		for &ri in &master_indices {
			let pd = match region_fd_map.get(&ri) {
				Some(&region_fd) => {
					let region_fd = usize::try_from(region_fd).map_err(|_| list_index_error())?;
					region_private(ri, region_fd)?;
					PdRef::Region(ri, region_fd)
				}
				// If the region font doesn't have this FontDict, just reference the
				// last one used.
				None => last_pd,
			};
			last_pd = pd;
			pds.push(pd);
		}
		let num_masters = pds.len();

		let snapshot = private_dict.raw.clone();
		let raw_of = |pd: PdRef, key: &str| -> Result<Option<Value>> {
			let raw = match pd {
				PdRef::Vf => &snapshot,
				PdRef::Region(ri, fd) => &region_private(ri, fd)?.raw,
			};
			Ok(raw.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone()))
		};
		for (key, value) in &snapshot {
			if !PD_BLEND_FIELDS.contains(key) {
				continue;
			}
			let data = if let Value::List(_) = value {
				let mut values = Vec::with_capacity(num_masters);
				let mut missing = false;
				for &pd in &pds {
					match raw_of(pd, key)? {
						Some(v) => values.push(v),
						None => missing = true,
					}
				}
				if missing {
					// fontTools prints this.
					log::warn!(
						"Warning: {key} in default font Private dict is missing from another font, and was discarded."
					);
					continue;
				}
				let lists = values
					.iter()
					.map(|v| match v {
						Value::List(l) => Ok(l.as_slice()),
						_ => Err(ttlib_error(
							"TypeError: zip argument must support iteration",
						)),
					})
					.collect::<Result<Vec<_>>>()?;
				let rows = lists.iter().map(|l| l.len()).min().unwrap_or(0);
				// Row 0 contains the first value from each master. Each row is
				// converted from absolute values to values relative to the previous
				// row.
				let mut prev_val_list = vec![0.0; num_masters];
				let mut any_points_differ = false;
				let mut data_list: Vec<Vec<f64>> = Vec::with_capacity(rows);
				for r in 0..rows {
					let val_list = lists
						.iter()
						.map(|l| number(&l[r]))
						.collect::<Result<Vec<f64>>>()?;
					let rel_list: Vec<f64> = val_list
						.iter()
						.zip(&prev_val_list)
						.map(|(v, p)| v - p)
						.collect();
					if !any_points_differ && !all_equal(&rel_list) {
						any_points_differ = true;
					}
					let mut deltas = sub_model.get_deltas(&rel_list).map_err(model_error)?;
					// For PrivateDict BlueValues, the default font values are
					// absolute, not relative to the prior value.
					deltas[0] = val_list[0];
					prev_val_list = val_list;
					data_list.push(deltas);
				}
				if any_points_differ {
					Value::List(
						data_list
							.into_iter()
							.map(|d| Value::List(d.into_iter().map(conv_to_int).collect()))
							.collect(),
					)
				} else {
					// If there are no blend values, then we can collapse the blend lists.
					Value::List(data_list.into_iter().map(|d| conv_to_int(d[0])).collect())
				}
			} else {
				let mut values = Vec::with_capacity(num_masters);
				for &pd in &pds {
					let v = raw_of(pd, key)?.ok_or_else(|| key_error(format!("'{key}'")))?;
					values.push(number(&v)?);
				}
				if all_equal(&values) {
					conv_to_int(values[0])
				} else {
					let deltas = sub_model.get_deltas(&values).map_err(model_error)?;
					Value::List(deltas.into_iter().map(conv_to_int).collect())
				}
			};
			if let Some(entry) = private_dict.raw.iter_mut().find(|(k, _)| k == key) {
				entry.1 = data.clone();
			}
			private_dict.values.insert(key, data);
		}
	}
	Ok(())
}

/// `dict == dict` of locations.
fn same_location(a: &[(String, f64)], b: &[(String, f64)]) -> bool {
	a.len() == b.len()
		&& a.iter()
			.all(|(k, v)| b.iter().any(|(k2, v2)| k2 == k && v2 == v))
}

/// `dict == dict` of supports.
fn same_support(a: &Support, b: &Support) -> bool {
	a.len() == b.len()
		&& a.iter()
			.all(|(k, v)| b.iter().any(|(k2, v2)| k2 == k && v2 == v))
}

/// For each font DICT index of the default font, the font DICT index of each
/// region font, by region index (`getfd_map`).
type FdMap = HashMap<i64, HashMap<usize, i64>>;

/// `getfd_map`: since a subset source font may have fewer FontDicts in their
/// FDArray than the default font, we have to match up the FontDicts in the
/// different fonts. We do this with the FDSelect array, and by assuming that the
/// same glyph will reference matching FontDicts in each source font.
fn getfd_map(fonts: &[MasterCff]) -> Result<FdMap> {
	let mut fd_map = FdMap::new();
	let default_font = fonts.first().ok_or_else(list_index_error)?;
	let regions = fonts.get(1..).unwrap_or_default();
	let top = default_font.top()?;
	let Some(default_fd_select) = &top.fd_select else {
		// All glyphs reference only one FontDict. Map the FD index for regions to
		// index 0.
		fd_map.insert(0, (0..regions.len()).map(|ri| (ri, 0)).collect());
		return Ok(fd_map);
	};
	let fd = |v: Option<i64>| -> Result<i64> {
		v.ok_or_else(|| ttlib_error("TypeError: FDSelect has no FD index"))
	};
	let mut gname_mapping: HashMap<&str, i64> = HashMap::new();
	for (gid, &fd_index) in default_fd_select.gid_array.iter().enumerate() {
		let fd_index = fd(fd_index)?;
		let name = default_font
			.glyph_order
			.get(gid)
			.ok_or_else(list_index_error)?;
		gname_mapping.insert(name, fd_index);
		fd_map.entry(fd_index).or_default();
	}
	for (ri, region_font) in regions.iter().enumerate() {
		let region_top = region_font.top()?;
		let default_fd_index = |name: Option<&String>| -> Result<i64> {
			let name = name.ok_or_else(list_index_error)?;
			gname_mapping
				.get(name.as_str())
				.copied()
				.ok_or_else(|| key_error(fontoxide_misc::py::str_repr(name)))
		};
		match &region_top.fd_select {
			None => {
				// All the glyphs share the same FontDict. Pick any glyph.
				let default_fd = default_fd_index(region_font.glyph_order.first())?;
				fd_map.entry(default_fd).or_default().insert(ri, 0);
			}
			Some(region_fd_select) => {
				for (gid, &fd_index) in region_fd_select.gid_array.iter().enumerate() {
					let default_fd = default_fd_index(region_font.glyph_order.get(gid))?;
					let region_map = fd_map
						.get_mut(&default_fd)
						.ok_or_else(|| key_error(default_fd))?;
					region_map.entry(ri).or_insert(fd(fd_index)?);
				}
			}
		}
	}
	Ok(fd_map)
}

/// `CVarData`.
struct CVarData {
	var_data_list: Vec<ot::VarData>,
	master_supports: Vec<Support>,
	/// The model and the keys of each `vsindex`.
	vsindex_dict: Vec<(VariationModel, Vec<Vec<bool>>)>,
}

/// `_get_cs`: the index of a glyph's charstring; with `filter_empty`, `None` for
/// an empty one, as a missing glyph of a sparse master.
fn get_cs(m: &mut MasterCff, name: &str, filter_empty: bool) -> Result<Option<usize>> {
	let Some(&gid) = m.index.get(name) else {
		return Ok(None);
	};
	if filter_empty {
		m.cff.run_char_string(gid, None)?;
		let cs = &m
			.top()?
			.char_strings
			.as_ref()
			.ok_or_else(list_index_error)?
			.items[gid];
		let program = &cs.program;
		if program.is_empty() {
			// CFF2 empty charstring
			return Ok(None);
		}
		if program.len() <= 2
			&& program.last() == Some(&Token::Op("endchar"))
			&& (program.len() == 1 || matches!(program[0], Token::Int(_) | Token::Float(_)))
		{
			// CFF1 empty charstring
			return Ok(None);
		}
	}
	Ok(Some(gid))
}

/// `_add_new_vsindex`.
fn add_new_vsindex(
	model: &VariationModel,
	key: Vec<bool>,
	master_supports: &mut Vec<Support>,
	vsindex_dict: &mut Vec<(VariationModel, Vec<Vec<bool>>)>,
	vsindex_by_key: &mut HashMap<Vec<bool>, usize>,
	var_data_list: &mut Vec<ot::VarData>,
) -> Result<usize> {
	let mut var_tuple_indexes = Vec::new();
	for support in model.supports.iter().skip(1) {
		let at = match master_supports
			.iter()
			.position(|s| same_support(s, support))
		{
			Some(at) => at,
			None => {
				master_supports.push(support.clone());
				master_supports.len() - 1
			}
		};
		var_tuple_indexes.push(u16::try_from(at).map_err(|_| {
			ttlib_error("struct.error: ushort format requires 0 <= number <= 65535")
		})?);
	}
	let var_data = build_var_data(var_tuple_indexes, Vec::new(), false);
	let vsindex = vsindex_dict.len();
	vsindex_by_key.insert(key.clone(), vsindex);
	vsindex_dict.push((model.clone(), vec![key]));
	var_data_list.push(var_data);
	Ok(vsindex)
}

/// Draws a charstring of a font set into the merge pen, with the error of the pen
/// if it has one.
fn draw(cff: &mut CffFontSet, gid: usize, pen: &mut Cff2CharStringMergePen) -> Result<()> {
	let result = cff.draw_char_string_with_hints(gid, pen);
	if let Some(e) = pen.error.take() {
		return Err(e);
	}
	result?;
	Ok(())
}

/// `merge_charstrings`.
fn merge_charstrings(
	glyph_order: &[String],
	num_masters: usize,
	vf: &mut MasterCff,
	regions: &mut [MasterCff],
	master_model: &VariationModel,
) -> Result<CVarData> {
	let mut vsindex_dict: Vec<(VariationModel, Vec<Vec<bool>>)> = Vec::new();
	let mut vsindex_by_key: HashMap<Vec<bool>, usize> = HashMap::new();
	let mut var_data_list = Vec::new();
	let mut master_supports: Vec<Support> = Vec::new();
	let mut sub_models: HashMap<Vec<bool>, VariationModel> = HashMap::new();
	for gname in glyph_order {
		// Interpret empty non-default masters as missing glyphs from a sparse
		// master.
		let default_gid = get_cs(vf, gname, false)?;
		let mut all_cs = vec![default_gid];
		for r in regions.iter_mut() {
			all_cs.push(get_cs(r, gname, true)?);
		}
		let key: Vec<bool> = all_cs.iter().map(Option::is_some).collect();
		let model = if key.iter().all(|p| *p) {
			master_model
		} else {
			if !sub_models.contains_key(&key) {
				let sub = master_model.sub_model(&key).map_err(model_error)?;
				sub_models.insert(key.clone(), sub);
			}
			&sub_models[&key]
		};
		// Create the first pass CFF2 charstring, from the default charstring.
		let default_gid =
			default_gid.ok_or_else(|| key_error(fontoxide_misc::py::str_repr(gname)))?;
		let mut var_pen = Cff2CharStringMergePen::new(gname, num_masters, 0);
		draw(&mut vf.cff, default_gid, &mut var_pen)?;

		// Add the coordinates from all the other regions to the blend lists in the
		// CFF2 charstring.
		let mut region_count = 0;
		for (r, gid) in regions.iter_mut().zip(&all_cs[1..]) {
			let Some(gid) = *gid else {
				continue;
			};
			region_count += 1;
			var_pen.restart(region_count);
			draw(&mut r.cff, gid, &mut var_pen)?;
		}

		// Collapse each coordinate list to a blend operator and its args.
		let mut program = var_pen.get_char_string(model)?;
		let has_blend = program.contains(&Token::Op("blend"));
		if region_count > 0 && var_pen.seen_moveto && has_blend {
			// If the charstring required a new model, create a VarData table to go
			// with, and set vsindex.
			let vsindex = match vsindex_by_key.get(&key) {
				Some(&v) => v,
				None => add_new_vsindex(
					model,
					key,
					&mut master_supports,
					&mut vsindex_dict,
					&mut vsindex_by_key,
					&mut var_data_list,
				)?,
			};
			if vsindex != 0 {
				program.splice(0..0, [Token::Int(vsindex as i64), Token::Op("vsindex")]);
			}
		}
		let top = vf.cff.top_dicts.first_mut().ok_or_else(list_index_error)?;
		let char_strings = top.char_strings.as_mut().ok_or_else(list_index_error)?;
		char_strings.items[default_gid] = T2CharString {
			bytecode: None,
			program,
			fd_select_index: None,
		};
	}

	// If there is no variation in any of the charstrings, then vsindex_dict never
	// gets built. This could still be needed if there is variation in the
	// PrivateDict, so we will build the default data for vsindex = 0.
	if vsindex_dict.is_empty() {
		add_new_vsindex(
			master_model,
			vec![true; num_masters],
			&mut master_supports,
			&mut vsindex_dict,
			&mut vsindex_by_key,
			&mut var_data_list,
		)?;
	}
	Ok(CVarData {
		var_data_list,
		master_supports,
		vsindex_dict,
	})
}

/// `maybeRound(v, 0.01, round=round)`: rounds half to even when that changes the
/// value by no more than 0.01. The result is an `int` exactly when it is
/// integral.
fn round_value(v: f64) -> f64 {
	let rounded = v.round_ties_even();
	if (rounded - v).abs() <= 0.01 {
		rounded
	} else {
		v
	}
}

/// A number from a float that was rounded or computed from `int`s and `float`s:
/// an `int` exactly when integral.
fn rounded_arg(v: f64) -> Arg {
	if v.fract() == 0.0 && v.abs() < 9.0e18 {
		Arg::Int(v as i64)
	} else {
		Arg::Float(v)
	}
}

fn arg_value(a: &Arg) -> Result<f64> {
	a.number().map_err(VarLibError::from)
}

/// `a - b` of Python numbers.
fn arg_sub(a: &Arg, b: &Arg) -> Result<Arg> {
	Ok(match (a, b) {
		(Arg::Int(x), Arg::Int(y)) => Arg::Int(x - y),
		_ => Arg::Float(arg_value(a)? - arg_value(b)?),
	})
}

/// `CFF2CharStringMergePen`: a pen to merge Type 2 charstrings.
#[derive(Debug)]
pub struct Cff2CharStringMergePen {
	/// The commands, with the arguments of each master.
	commands: Vec<(&'static str, Vec<Vec<Arg>>)>,
	pt_index: usize,
	m_index: usize,
	/// Whether the default master has a `moveto` (`seen_moveto`).
	pub seen_moveto: bool,
	glyph_name: String,
	/// `_p0`: the previous point.
	p0: (Arg, Arg),
	/// The error of a pen method, which the interpreter cannot pass on.
	error: Option<VarLibError>,
}

impl Cff2CharStringMergePen {
	/// A pen for the master `master_idx` of a glyph (`CFF2CharStringMergePen`).
	pub fn new(glyph_name: &str, num_masters: usize, master_idx: usize) -> Self {
		let _ = num_masters;
		Cff2CharStringMergePen {
			commands: Vec::new(),
			pt_index: 0,
			m_index: master_idx,
			seen_moveto: false,
			glyph_name: glyph_name.to_owned(),
			p0: (Arg::Int(0), Arg::Int(0)),
			error: None,
		}
	}

	/// `restart`: draws the master `region_idx` next.
	pub fn restart(&mut self, region_idx: usize) {
		self.pt_index = 0;
		self.m_index = region_idx;
		self.p0 = (Arg::Int(0), Arg::Int(0));
	}

	/// Keeps an error to pass on after drawing.
	fn fail(&mut self, e: VarLibError) -> fontoxide_ttlib::Error {
		let message = e.to_string();
		self.error = Some(e);
		fontoxide_ttlib::Error::ttlib(message)
	}

	/// `_p`: the point rounded, relative to the previous one.
	fn p(&mut self, pt: Point) -> Result<Vec<Arg>> {
		let p0 = self.p0.clone();
		let pt = (
			rounded_arg(round_value(pt.0)),
			rounded_arg(round_value(pt.1)),
		);
		self.p0 = pt.clone();
		Ok(vec![arg_sub(&pt.0, &p0.0)?, arg_sub(&pt.1, &p0.1)?])
	}

	fn add_point(&mut self, point_type: &'static str, pt_coords: Vec<Arg>) -> Result<()> {
		if self.m_index == 0 {
			self.commands.push((point_type, vec![pt_coords]));
		} else {
			let cmd = self
				.commands
				.get_mut(self.pt_index)
				.ok_or_else(list_index_error)?;
			if cmd.0 != point_type {
				return Err(VarLibError::CffPointTypeMerge(format!(
					"Glyph '{}': '{point_type}' at point index {} in master index {} differs from the default font point type '{}'",
					self.glyph_name,
					self.pt_index,
					cmd.1.len(),
					cmd.0
				)));
			}
			cmd.1.push(pt_coords);
		}
		self.pt_index += 1;
		Ok(())
	}

	fn hint_type_error(&self, hint_type: &str, cmd_type: &str, m_index: usize) -> VarLibError {
		VarLibError::CffHintTypeMerge(format!(
			"Glyph '{}': '{hint_type}' at index {} in master index {m_index} differs from the default font hint type '{cmd_type}'",
			self.glyph_name, self.pt_index
		))
	}

	fn merge_hint(&mut self, hint_type: &'static str, args: &[Arg]) -> Result<()> {
		if self.m_index == 0 {
			self.commands.push((hint_type, vec![args.to_vec()]));
		} else {
			let (cmd_type, count) = {
				let cmd = self
					.commands
					.get(self.pt_index)
					.ok_or_else(list_index_error)?;
				(cmd.0, cmd.1.len())
			};
			if cmd_type != hint_type {
				return Err(self.hint_type_error(hint_type, cmd_type, count));
			}
			self.commands[self.pt_index].1.push(args.to_vec());
		}
		self.pt_index += 1;
		Ok(())
	}

	fn merge_hintmask(&mut self, hint_type: &'static str, mask: &[u8]) -> Result<()> {
		// For hintmask, the specializer expects each of these to be represented by
		// two sequential commands: first holding only the operator name, with an
		// empty arg list, second with an empty string as the op name, and the mask
		// arg list.
		if self.m_index == 0 {
			self.commands.push((hint_type, Vec::new()));
			self.commands
				.push(("", vec![vec![Arg::Mask(mask.to_vec())]]));
		} else {
			let (cmd_type, count) = {
				let cmd = self
					.commands
					.get(self.pt_index)
					.ok_or_else(list_index_error)?;
				(cmd.0, cmd.1.len())
			};
			if cmd_type != hint_type {
				return Err(self.hint_type_error(hint_type, cmd_type, count));
			}
			self.pt_index += 1;
			let cmd = self
				.commands
				.get_mut(self.pt_index)
				.ok_or_else(list_index_error)?;
			cmd.1.push(vec![Arg::Mask(mask.to_vec())]);
		}
		self.pt_index += 1;
		Ok(())
	}

	/// `reorder_blend_args`: the arguments of each command by argument, then by
	/// master; the arguments that differ between the masters become blend lists of
	/// the default value, the deltas, and the number of blends (1).
	fn reorder_blend_args(&mut self, model: &VariationModel) -> Result<Vec<Command>> {
		let mut commands = Vec::with_capacity(self.commands.len());
		let mut last_op: Option<&str> = None;
		for (op, masters) in std::mem::take(&mut self.commands) {
			// `zip(*args)`: the arguments of all masters, by argument.
			let count = if masters.is_empty() {
				0
			} else {
				masters.iter().map(Vec::len).min().unwrap_or(0)
			};
			let coords: Vec<Vec<Arg>> = (0..count)
				.map(|i| masters.iter().map(|m| m[i].clone()).collect())
				.collect();
			let args = if matches!(last_op, Some("hintmask" | "cntrmask")) {
				// UPSTREAM-BUG(B49): the check that the masks of all masters are equal
				// compares the list of the mask tuple only, so it never fails: the
				// default master's mask is used.
				let first = coords
					.first()
					.and_then(|c| c.first())
					.cloned()
					.ok_or_else(list_index_error)?;
				vec![first]
			} else {
				let mut new_coords = Vec::with_capacity(coords.len());
				for coord in coords {
					if coord.windows(2).all(|w| w[0].py_eq(&w[1])) {
						new_coords.push(coord.into_iter().next().ok_or_else(list_index_error)?);
					} else {
						// Convert to deltas.
						let values = coord.iter().map(arg_value).collect::<Result<Vec<f64>>>()?;
						let deltas = model
							.get_deltas_rounded(&values, round_value)
							.map_err(model_error)?;
						let mut blend = Vec::with_capacity(coord.len() + 1);
						blend.push(coord[0].clone());
						blend.extend(deltas.into_iter().skip(1).map(rounded_arg));
						blend.push(Arg::Int(1));
						new_coords.push(Arg::Blend(blend));
					}
				}
				new_coords
			};
			commands.push(Command::new(op, args));
			last_op = Some(op);
		}
		Ok(commands)
	}

	/// `getCharString`: the program of the merged charstring, specialized.
	pub fn get_char_string(&mut self, var_model: &VariationModel) -> Result<Vec<Token>> {
		let commands = self.reorder_blend_args(var_model)?;
		let commands = specialize_commands(
			&commands,
			&SpecializeOptions {
				generalize_first: false,
				max_stack: MAX_STACK_LIMIT,
				..Default::default()
			},
		)?;
		Ok(commands_to_program(&commands)?)
	}
}

impl Pen for Cff2CharStringMergePen {
	fn move_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.seen_moveto = true;
		let result = self.p(pt).and_then(|c| self.add_point("rmoveto", c));
		result.map_err(|e| self.fail(e).into_pen_error())
	}

	fn line_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		let result = self.p(pt).and_then(|c| self.add_point("rlineto", c));
		result.map_err(|e| self.fail(e).into_pen_error())
	}

	fn curve_to(&mut self, points: &[Point]) -> fontoxide_pens::Result<()> {
		let result = (|| {
			let [p1, p2, p3] = points else {
				return Err(ttlib_error(
					"NotImplementedError: curveTo with other than 3 points",
				));
			};
			let mut coords = self.p(*p1)?;
			coords.extend(self.p(*p2)?);
			coords.extend(self.p(*p3)?);
			self.add_point("rrcurveto", coords)
		})();
		result.map_err(|e| self.fail(e).into_pen_error())
	}

	fn q_curve_to(&mut self, _: &[Point], _: Option<Point>) -> fontoxide_pens::Result<()> {
		Err(fontoxide_pens::Error::Usage(
			"qCurveTo is not supported by CFF2CharStringMergePen".into(),
		))
	}

	fn close_path(&mut self) -> fontoxide_pens::Result<()> {
		Ok(())
	}

	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		Ok(())
	}

	fn add_component(&mut self, _: &str, _: Transform) -> fontoxide_pens::Result<()> {
		// `BasePen.addComponent` draws the glyph from the pen's glyph set, which is
		// None.
		Err(fontoxide_pens::Error::Usage(
			"TypeError: 'NoneType' object is not subscriptable".into(),
		))
	}
}

impl HintPen for Cff2CharStringMergePen {
	fn add_hint(&mut self, op: &'static str, args: &[Arg]) -> fontoxide_ttlib::Result<()> {
		self.merge_hint(op, args).map_err(|e| self.fail(e))
	}

	fn add_hintmask(&mut self, op: &'static str, mask: &[u8]) -> fontoxide_ttlib::Result<()> {
		self.merge_hintmask(op, mask).map_err(|e| self.fail(e))
	}
}
