//! Port of `fontTools.varLib.mutator`: instantiates a variable font at a location.
//!
//! fontTools deprecates this module in favor of [`crate::instancer`], which also
//! makes full instances, and supports partial instancing.

use std::collections::{BTreeSet, HashMap};

use fontoxide_misc::fixed_tools::{float_to_fixed, float_to_fixed_to_float};
use fontoxide_misc::py::{self, str_repr};
use fontoxide_misc::round_tools::ot_round;
use fontoxide_ttlib::binary::fit;
use fontoxide_ttlib::cff::charstring::Token;
use fontoxide_ttlib::cff::dict::Value;
use fontoxide_ttlib::cff::font::{TopDict, num_regions};
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::tt_program::Program;
use fontoxide_ttlib::tables::tuple_variation::{Delta, TupleVariation};
use fontoxide_ttlib::tables::{
	Avar, Cff2, Cvar, Cvt, Fpgm, Fvar, Gdef, Glyf, Gpos, Gsub, Gvar, Hmtx, Hvar, Ltag, Maxp, Mvar,
	Name, Os2, Post, Vmtx,
};
use fontoxide_ttlib::var::{Location, VarStoreInstancer, iup_delta, support_scalar};
use fontoxide_ttlib::{Error, Font, ReadOptions, Result, Tag, WriteOptions};

use crate::instancer::metrics::{apply_mvar_delta, is_mvar_entry};
use crate::instancer::otl::merge_gdef_gpos;
use crate::models::{normalize_location, piecewise_linear_map};

const AVAR: Tag = Tag::new(b"avar");
const CFF2: Tag = Tag::new(b"CFF2");
const CVAR: Tag = Tag::new(b"cvar");
const DSIG: Tag = Tag::new(b"DSIG");
const FPGM: Tag = Tag::new(b"fpgm");
const FVAR: Tag = Tag::new(b"fvar");
const GDEF: Tag = Tag::new(b"GDEF");
const GPOS: Tag = Tag::new(b"GPOS");
const GSUB: Tag = Tag::new(b"GSUB");
const GVAR: Tag = Tag::new(b"gvar");
const HVAR: Tag = Tag::new(b"HVAR");
const LTAG: Tag = Tag::new(b"ltag");
const MAXP: Tag = Tag::new(b"maxp");
const MVAR: Tag = Tag::new(b"MVAR");
const NAME: Tag = Tag::new(b"name");
const OS2: Tag = Tag::new(b"OS/2");
const POST: Tag = Tag::new(b"post");

/// The variable tables, which are removed.
const VARIABLE_TABLES: [&[u8; 4]; 8] = [
	b"avar", b"cvar", b"fvar", b"gvar", b"HVAR", b"MVAR", b"VVAR", b"STAT",
];

const OVERLAP_COMPOUND: i64 = 0x0400;
const FLAG_OVERLAP_SIMPLE: u8 = 0x40;

/// `OS2_WIDTH_CLASS_VALUES`: maps the `wdth` axis (1..200) to `OS/2.usWidthClass`
/// (1..9), rounding to the closest: the midpoints between the percentages of the
/// classes, in order.
fn os2_width_class(wdth: f64) -> u16 {
	const PERCENTS: [f64; 9] = [50.0, 62.5, 75.0, 87.5, 100.0, 112.5, 125.0, 150.0, 200.0];
	for (i, pair) in PERCENTS.windows(2).enumerate() {
		let half = (pair[0] + pair[1]) / 2.0;
		if wdth < half {
			return i as u16 + 1;
		}
	}
	9
}

fn key_error(key: &str) -> Error {
	Error::ttlib(format!("KeyError: {}", str_repr(key)))
}

fn index_error() -> Error {
	Error::ttlib("IndexError: list index out of range")
}

fn type_error(message: &str) -> Error {
	Error::ttlib(format!("TypeError: {message}"))
}

/// A number of a DICT value (`int` or `float`).
fn dict_number(value: &Value) -> Result<f64> {
	match value {
		Value::Int(v) => Ok(*v as f64),
		Value::Float(v) => Ok(*v),
		_ => Err(type_error("unsupported operand type(s)")),
	}
}

/// `value + delta`, with an `int` delta: an `int` stays one.
fn dict_add(value: &Value, delta: i64) -> Result<Value> {
	match value {
		Value::Int(v) => Ok(Value::Int(v + delta)),
		Value::Float(v) => Ok(Value::Float(v + delta as f64)),
		_ => Err(type_error("unsupported operand type(s)")),
	}
}

/// The deltas of a blend (`value[1:]`).
fn dict_deltas(values: &[Value]) -> Result<Vec<f64>> {
	values.iter().skip(1).map(dict_number).collect()
}

/// `interpolate_cff2_PrivateDict`.
fn interpolate_cff2_private_dict(
	top: &mut TopDict,
	instancer: &VarStoreInstancer<'_>,
) -> Result<()> {
	const BLEND_LISTS: [&str; 6] = [
		"BlueValues",
		"OtherBlues",
		"FamilyBlues",
		"FamilyOtherBlues",
		"StemSnapH",
		"StemSnapV",
	];
	const BLEND_VALUES: [&str; 5] = ["BlueScale", "BlueShift", "BlueFuzz", "StdHW", "StdVW"];
	let font_dicts = top.fd_array.as_mut().ok_or_else(|| {
		Error::ttlib("AttributeError: 'TopDict' object has no attribute 'FDArray'")
	})?;
	for font_dict in font_dicts {
		let pd = font_dict.private.as_deref_mut().ok_or_else(|| {
			Error::ttlib("AttributeError: 'FontDict' object has no attribute 'Private'")
		})?;
		let vs_index = match pd.get("vsindex") {
			Some(v) => v.int()?,
			None => 0,
		};
		let keys: Vec<&'static str> = pd.raw.iter().map(|(k, _)| *k).collect();
		for key in keys {
			let Some(value) = pd.values.get(key) else {
				continue;
			};
			if BLEND_VALUES.contains(&key) {
				if let Value::List(items) = value {
					let default = dict_number(items.first().ok_or_else(index_error)?)?;
					let delta =
						instancer.interpolate_from_deltas(vs_index, &dict_deltas(items)?)?;
					pd.values.insert(key, Value::Int(ot_round(default + delta)));
				}
			} else if BLEND_LISTS.contains(&key) {
				let Value::List(items) = value else {
					return Err(type_error("object is not subscriptable"));
				};
				if !matches!(items.first().ok_or_else(index_error)?, Value::List(_)) {
					continue;
				}
				// If any argument in a BlueValues list is a blend list, then they all are.
				// The first value of each list is an absolute value. The delta tuples are
				// calculated from relative master values, hence we need to append all the
				// deltas to date to each successive absolute value.
				let mut delta = 0;
				let mut new = Vec::with_capacity(items.len());
				for item in items {
					let Value::List(blend) = item else {
						return Err(type_error("object is not subscriptable"));
					};
					delta += ot_round(
						instancer.interpolate_from_deltas(vs_index, &dict_deltas(blend)?)?,
					);
					new.push(dict_add(blend.first().ok_or_else(index_error)?, delta)?);
				}
				pd.values.insert(key, Value::List(new));
			}
		}
	}
	Ok(())
}

/// The index of `i - n` in a program (Python slices and indices wrap around below 0;
/// a valid charstring never needs that).
fn back(i: usize, n: i64) -> Result<usize> {
	usize::try_from(i as i64 - n).map_err(|_| index_error())
}

fn token_int(token: Option<&Token>) -> Result<i64> {
	match token {
		Some(Token::Int(v)) => Ok(*v),
		Some(_) => Err(type_error("an integer is required")),
		None => Err(index_error()),
	}
}

fn token_number(token: &Token) -> Result<f64> {
	match token {
		Token::Int(v) => Ok(*v as f64),
		Token::Float(v) => Ok(*v),
		_ => Err(type_error("unsupported operand type(s)")),
	}
}

/// `interpolate_cff2_charstrings`: replaces the blends by their values at the
/// location, and drops the `vsindex` operators.
fn interpolate_cff2_charstrings(
	top: &mut TopDict,
	instancer: &VarStoreInstancer<'_>,
	glyph_order: &[String],
) -> Result<()> {
	let store = top.var_store.clone();
	let char_strings = top.char_strings.as_mut().ok_or_else(|| {
		Error::ttlib("AttributeError: 'TopDict' object has no attribute 'CharStrings'")
	})?;
	for name in glyph_order {
		let gid = char_strings.index(name).ok_or_else(|| key_error(name))?;
		let char_string = &mut char_strings.items[gid];
		let program = &mut char_string.program;
		let mut new_program = Vec::new();
		let mut vs_index = 0;
		let mut last_i = 0;
		for i in 0..program.len() {
			match program[i] {
				Token::Op("vsindex") => {
					vs_index = token_int(program.get(back(i, 1)?))?;
					if last_i != 0 {
						let end = back(i, 1)?;
						new_program.extend_from_slice(&program[last_i.min(end)..end]);
					}
					last_i = i + 1;
				}
				Token::Op("blend") => {
					let num_regions = num_regions(store.as_deref(), vs_index)?;
					let num_masters = 1 + num_regions;
					let num_args = token_int(program.get(back(i, 1)?))?;
					// The program list starting at program[i] is now:
					// ..args for following operations
					// num_args values  from the default font
					// num_args tuples, each with numMasters-1 delta values
					// num_blend_args
					// 'blend'
					let mut argi = back(i, num_args * num_masters + 1)?;
					let end_args = argi + num_args.max(0) as usize;
					let mut tuplei = end_args;
					while argi < end_args {
						let next_ti = tuplei + num_regions.max(0) as usize;
						let deltas: Vec<f64> = program
							.get(tuplei..next_ti.min(program.len()))
							.unwrap_or_default()
							.iter()
							.map(token_number)
							.collect::<Result<_>>()?;
						let delta = ot_round(instancer.interpolate_from_deltas(vs_index, &deltas)?);
						program[argi] = match program[argi] {
							Token::Int(v) => Token::Int(v + delta),
							Token::Float(v) => Token::Float(v + delta as f64),
							_ => return Err(type_error("unsupported operand type(s)")),
						};
						tuplei = next_ti;
						argi += 1;
					}
					new_program.extend_from_slice(&program[last_i.min(end_args)..end_args]);
					last_i = i + 1;
				}
				_ => {}
			}
		}
		if last_i != 0 {
			new_program.extend_from_slice(&program[last_i.min(program.len())..]);
			*program = new_program;
			char_string.bytecode = None;
		}
	}
	Ok(())
}

/// `interpolate_cff2_metrics`: neither advance widths nor bounding boxes are in
/// CFF2 charstrings. The widths come from `hmtx` and `HVAR`; since left side
/// bearings can't be interpolated reliably from those of the masters in `hmtx`,
/// they come from the bounding boxes of the charstrings.
fn interpolate_cff2_metrics(
	font: &mut Font,
	glyph_order: &[String],
	axes: &[String],
	location: &Location,
) -> Result<()> {
	// The left side bearings.
	let mut lsbs = Vec::with_capacity(glyph_order.len());
	{
		let cff = &mut font.require::<Cff2>()?.cff;
		for name in glyph_order {
			let gid = cff.glyph_index(name).ok_or_else(|| key_error(name))?;
			// Non-marking glyphs have no bounds.
			lsbs.push(cff.calc_bounds(gid)?.map(|b| ot_round(b[0])));
		}
	}
	// The width deltas.
	let mut width_deltas = vec![0; glyph_order.len()];
	if font.contains(HVAR) {
		let table = font.require::<Hvar>()?.table.clone();
		let instancer = VarStoreInstancer::new(table.var_store.as_deref(), axes, location.clone());
		for (gid, (delta, name)) in width_deltas.iter_mut().zip(glyph_order).enumerate() {
			let index = match &table.adv_width_map {
				Some(map) => *map
					.mapping
					.get(&(gid as u32))
					.ok_or_else(|| key_error(name))?,
				None => gid as u64,
			};
			*delta = ot_round(instancer.get(index)?);
		}
	}
	let hmtx = font.require_locked::<Hmtx>(false)?;
	for (gid, name) in glyph_order.iter().enumerate() {
		let entry = hmtx
			.metrics
			.get_mut(&(gid as u32))
			.ok_or_else(|| key_error(name))?;
		let width_delta = width_deltas[gid];
		let lsb_delta = match lsbs[gid] {
			None => 0,
			Some(lsb) => i64::from(entry.1) - lsb,
		};
		if width_delta != 0 {
			entry.0 = fit((i64::from(entry.0) + width_delta).max(0), "advanceWidth")?;
		}
		if lsb_delta != 0
			&& let Some(lsb) = lsbs[gid]
		{
			entry.1 = fit(lsb, "leftSideBearing")?;
		}
	}
	Ok(())
}

/// `supportScalar(loc, var.axes)`, with the axes in the order of `fvar`, as they
/// are read.
fn variation_scalar(location: &Location, axes: &[String], var: &TupleVariation) -> f64 {
	support_scalar(
		location,
		axes.iter()
			.filter_map(|tag| var.axes.get(tag).map(|r| (tag.as_str(), *r))),
	)
}

/// The `glyf`/`gvar` part of `instantiateVariableFont`.
fn mutate_gvar(font: &mut Font, axes: &[String], location: &Location) -> Result<()> {
	log::info!("Mutating glyf/gvar tables");
	let variations = std::mem::take(&mut font.require_locked::<Gvar>(false)?.variations);
	font.require_locked::<Glyf>(false)?;
	let order = font.glyph_order()?;
	let mut h_metrics = std::mem::take(&mut font.require_locked::<Hmtx>(false)?.metrics);
	let mut v_metrics = match font.get_locked::<Vmtx>(false)? {
		Some(vmtx) => Some(std::mem::take(&mut vmtx.metrics)),
		None => None,
	};
	let glyf = font.require_locked::<Glyf>(false)?;
	// The glyphs of gvar, sorted by component depth.
	let mut cache = HashMap::new();
	let mut sorted = Vec::with_capacity(variations.len());
	for (index, (gid, _)) in variations.iter().enumerate() {
		let depth = match glyf.glyphs.get(gid) {
			Some(g) if g.is_composite() => {
				glyf.composite_maxp_values(*gid, &order, 1, &mut cache, &mut vec![*gid])?
					.2
			}
			_ => 0,
		};
		sorted.push((depth, order.name(*gid).into_owned(), index));
	}
	sorted.sort();
	for (_, _, index) in sorted {
		let (gid, vars) = &variations[index];
		let (mut coordinates, end_pts) =
			glyf.coordinates_and_controls(*gid, &order, &h_metrics, v_metrics.as_ref())?;
		let original = coordinates.clone();
		let ends: Vec<usize> = end_pts.iter().map(|&e| e as usize).collect();
		for var in vars {
			let scalar = variation_scalar(location, axes, var);
			if scalar == 0.0 {
				continue;
			}
			let deltas: Vec<Option<(f64, f64)>> = var
				.coordinates
				.iter()
				.map(|d| match d {
					Some(Delta::Point(x, y)) => Some((x.value(), y.value())),
					_ => None,
				})
				.collect();
			let deltas: Vec<(f64, f64)> = if deltas.iter().any(Option::is_none) {
				iup_delta(&deltas, &original, &ends)?
			} else {
				deltas.into_iter().flatten().collect()
			};
			if deltas.len() != coordinates.len() {
				return Err(Error::Assertion("len(a) == len(other)".into()));
			}
			for (c, d) in coordinates.iter_mut().zip(&deltas) {
				c.0 += d.0 * scalar;
				c.1 += d.1 * scalar;
			}
		}
		glyf.set_coordinates(
			*gid,
			&order,
			&coordinates,
			&mut h_metrics,
			v_metrics.as_mut(),
		)?;
	}
	font.require_locked::<Hmtx>(false)?.metrics = h_metrics;
	if let Some(metrics) = v_metrics {
		font.require_locked::<Vmtx>(false)?.metrics = metrics;
	}
	Ok(())
}

/// The `cvt `/`cvar` part of `instantiateVariableFont`.
fn mutate_cvar(font: &mut Font, axes: &[String], location: &Location) -> Result<()> {
	log::info!("Mutating cvt/cvar tables");
	let cvar = font.require::<Cvar>()?;
	// The deltas, in the order of a Python dict.
	let mut order = Vec::new();
	let mut deltas: HashMap<usize, f64> = HashMap::new();
	for var in &cvar.variations {
		let scalar = variation_scalar(location, axes, var);
		if scalar == 0.0 {
			continue;
		}
		for (i, c) in var.coordinates.iter().enumerate() {
			let value = match c {
				Some(Delta::Value(n)) => n.value(),
				Some(Delta::Point(..)) => return Err(type_error("can't multiply sequence")),
				None => continue,
			};
			let entry = deltas.entry(i).or_insert_with(|| {
				order.push(i);
				0.0
			});
			*entry += scalar * value;
		}
	}
	let cvt = font.require::<Cvt>()?;
	for i in order {
		let value = cvt
			.values
			.get_mut(i)
			.ok_or_else(|| Error::ttlib("IndexError: array index out of range"))?;
		*value = fit(i64::from(*value) + ot_round(deltas[&i]), "cvt")?;
	}
	Ok(())
}

/// The `CFF2` part of `instantiateVariableFont`.
fn mutate_cff2(font: &mut Font, axes: &[String], location: &Location) -> Result<()> {
	log::info!("Mutating CFF2 table");
	let glyph_order = font.glyph_order()?.names().to_vec();
	{
		let cff = &mut font.require::<Cff2>()?.cff;
		let top = cff.top_dicts.first_mut().ok_or_else(index_error)?;
		let store = top.var_store.clone().ok_or_else(|| {
			Error::ttlib("AttributeError: 'TopDict' object has no attribute 'VarStore'")
		})?;
		let instancer = VarStoreInstancer::new(Some(&store), axes, location.clone());
		interpolate_cff2_private_dict(top, &instancer)?;
		cff.desubroutinize()?;
		let top = cff.top_dicts.first_mut().ok_or_else(index_error)?;
		interpolate_cff2_charstrings(top, &instancer, &glyph_order)?;
	}
	interpolate_cff2_metrics(font, &glyph_order, axes, location)?;
	let top = font
		.require::<Cff2>()?
		.cff
		.top_dicts
		.first_mut()
		.ok_or_else(index_error)?;
	if !top.raw.iter().any(|(k, _)| *k == "VarStore") {
		return Err(key_error("VarStore"));
	}
	top.raw.retain(|(k, _)| *k != "VarStore");
	top.var_store = None;
	Ok(())
}

/// The `MVAR` part of `instantiateVariableFont`.
fn mutate_mvar(font: &mut Font, axes: &[String], location: &Location) -> Result<()> {
	log::info!("Mutating MVAR table");
	let table = font.require::<Mvar>()?.table.clone();
	let instancer = VarStoreInstancer::new(table.var_store.as_deref(), axes, location.clone());
	for record in &table.value_record {
		if !is_mvar_entry(&record.value_tag) {
			continue;
		}
		let delta = ot_round(instancer.get(u64::from(record.var_idx))?);
		if delta == 0 {
			continue;
		}
		apply_mvar_delta(font, &record.value_tag, delta)?;
	}
	Ok(())
}

/// Applies the first feature variation record whose conditions match, and deletes
/// the feature variations.
fn mutate_feature_variations(
	feature_variations: &mut Option<Box<ot::FeatureVariations>>,
	feature_list: &mut Option<Box<ot::FeatureList>>,
	axes: &[String],
	location: &Location,
) -> Result<()> {
	let Some(variations) = feature_variations.take() else {
		return Ok(());
	};
	for record in &variations.feature_variation_record {
		let mut applies = true;
		let conditions = record
			.condition_set
			.as_ref()
			.map_or(&[][..], |s| &s.condition_table[..]);
		for condition in conditions.iter().flatten() {
			match condition.as_ref() {
				ot::ConditionTable::Format1(c) => {
					let tag = axes
						.get(usize::from(c.axis_index))
						.ok_or_else(index_error)?;
					let v = *location.get(tag).ok_or_else(|| key_error(tag))?;
					if !(c.filter_range_min_value <= v && v <= c.filter_range_max_value) {
						applies = false;
					}
				}
				_ => applies = false,
			}
			if !applies {
				break;
			}
		}
		if applies {
			if let Some(substitution) = &record.feature_table_substitution {
				if substitution.version != 0x0001_0000 {
					return Err(Error::Assertion(
						"record.FeatureTableSubstitution.Version == 0x00010000".into(),
					));
				}
				for rec in &substitution.substitution_record {
					let list = feature_list.as_deref_mut().ok_or_else(|| {
						Error::ttlib(
							"AttributeError: 'NoneType' object has no attribute 'FeatureRecord'",
						)
					})?;
					list.feature_record
						.get_mut(usize::from(rec.feature_index))
						.ok_or_else(index_error)?
						.feature = rec.feature.clone();
				}
			}
			break;
		}
	}
	Ok(())
}

/// The `GDEF`/`GPOS` part of `instantiateVariableFont`.
fn mutate_gdef(font: &mut Font, axes: &[String], location: &Location) -> Result<()> {
	log::info!("Mutating GDEF/GPOS/GSUB tables");
	let store = font.require::<Gdef>()?.var_store.clone();
	let instancer = VarStoreInstancer::new(store.as_deref(), axes, location.clone());
	merge_gdef_gpos(font, &|i| instancer.get(i))?;

	// Downgrade GDEF.
	let gdef = font.require::<Gdef>()?;
	gdef.var_store = None;
	gdef.version = 0x0001_0002;
	if gdef.mark_glyph_sets_def.is_none() {
		gdef.version = 0x0001_0000;
	}
	if gdef.lig_caret_list.is_none()
		&& gdef.mark_attach_class_def.is_none()
		&& gdef.glyph_class_def.is_none()
		&& gdef.attach_list.is_none()
		&& !(gdef.version >= 0x0001_0002 && gdef.mark_glyph_sets_def.is_some())
	{
		font.remove(GDEF);
	}
	Ok(())
}

/// Whether the instructions of a glyph use `GETVARIATION`, which then needs an
/// instruction definition.
fn uses_getvariation(glyf: &Glyf) -> Result<bool> {
	for glyph in glyf.glyphs.values() {
		if let Some(program) = &glyph.program
			&& program
				.assembly(true)?
				.iter()
				.any(|op| op.starts_with("GETVARIATION"))
		{
			return Ok(true);
		}
	}
	Ok(false)
}

/// Adds an `IDEF` for the `GETVARIATION` opcode to `fpgm`, which pushes the
/// location.
fn add_getvariation_idef(font: &mut Font, axes: &[String], location: &Location) -> Result<()> {
	log::info!("Adding IDEF to fpgm table for GETVARIATION opcode");
	let mut asm = if font.contains(FPGM) {
		font.require::<Fpgm>()?
			.program
			.as_ref()
			.ok_or_else(|| {
				Error::ttlib("AttributeError: 'table__f_p_g_m' object has no attribute 'program'")
			})?
			.assembly(true)?
	} else {
		font.insert(Box::new(Fpgm {
			program: Some(Program::default()),
		}));
		Vec::new()
	};
	asm.push("PUSHB[000] 145".to_owned());
	asm.push("IDEF[ ]".to_owned());
	let mut args = vec![location.len().to_string()];
	for tag in axes {
		let v = *location.get(tag).ok_or_else(|| key_error(tag))?;
		args.push(float_to_fixed(v, 14).to_string());
	}
	asm.push(format!("NPUSHW[ ] {}", args.join(" ")));
	asm.push("ENDF[ ]".to_owned());
	font.require::<Fpgm>()?.program = Some(Program::from_assembly(&asm)?);

	// Change maxp attributes as IDEF is added.
	if font.contains(MAXP) {
		let maxp = font.require_locked::<Maxp>(false)?;
		maxp.v1.max_instruction_defs = fit(
			1 + i64::from(maxp.v1.max_instruction_defs),
			"maxInstructionDefs",
		)?;
		maxp.v1.max_stack_elements = fit(
			(location.len() as i64).max(i64::from(maxp.v1.max_stack_elements)),
			"maxStackElements",
		)?;
	}
	Ok(())
}

/// Prunes the names of the axes and the named instances.
fn prune_names(font: &mut Font) -> Result<()> {
	log::info!("Pruning name table");
	let fvar = font.require::<Fvar>()?;
	let mut exclude: BTreeSet<u16> = fvar.axes.iter().map(|a| a.axis_name_id).collect();
	for instance in &fvar.instances {
		exclude.insert(instance.subfamily_name_id);
		exclude.insert(instance.postscript_name_id);
	}
	if font.contains(LTAG) {
		// Drop the whole 'ltag' table if all its language tags are referenced by name
		// records to be pruned.
		let excluded: BTreeSet<u16> = font
			.require::<Name>()?
			.names
			.iter()
			.filter(|n| exclude.contains(&n.name_id) && n.platform_id == 0 && n.lang_id != 0xFFFF)
			.map(|n| n.lang_id)
			.collect();
		let count = font.require::<Ltag>()?.tags.len();
		if excluded.len() == count && excluded.iter().all(|&id| usize::from(id) < count) {
			font.remove(LTAG);
		}
	}
	font.require::<Name>()?
		.names
		.retain(|n| n.name_id < 256 || !exclude.contains(&n.name_id));
	Ok(())
}

/// `instantiateVariableFont`: makes a static instance of a variable font in place,
/// at a location in user-space coordinates, e.g. `[("wght", 400.0), ("wdth", 100.0)]`.
///
/// With `overlap`, the `OVERLAP_SIMPLE` and `OVERLAP_COMPOUND` flags of the
/// glyphs are set.
pub fn instantiate_variable_font(
	varfont: &mut Font,
	location: &[(String, f64)],
	overlap: bool,
) -> Result<()> {
	if !varfont.contains(FVAR) {
		return Err(Error::value("Missing required table fvar"));
	}
	mutate(varfont, location, overlap)
}

/// `instantiateVariableFont` with `inplace=False`: the font is saved and read back,
/// and the copy is instantiated.
pub fn instantiate_variable_font_copy(
	varfont: &mut Font,
	location: &[(String, f64)],
	overlap: bool,
) -> Result<Font> {
	if !varfont.contains(FVAR) {
		return Err(Error::value("Missing required table fvar"));
	}
	let mut font = Font::read(
		varfont.write(&WriteOptions::default())?,
		ReadOptions::default(),
	)?;
	mutate(&mut font, location, overlap)?;
	Ok(font)
}

// `max(1, min(v, 1000))` is not `clamp` with NaN.
#[allow(clippy::manual_clamp)]
fn mutate(font: &mut Font, location: &[(String, f64)], overlap: bool) -> Result<()> {
	let fvar_axes: Vec<(String, (f64, f64, f64))> = font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| (a.tag_text(), (a.min_value, a.default_value, a.max_value)))
		.collect();
	let axes: Vec<String> = fvar_axes.iter().map(|a| a.0.clone()).collect();
	let mut loc = normalize_location(location, &fvar_axes, false)?;
	if font.contains(AVAR) {
		let avar = font.require::<Avar>()?;
		for (tag, v) in &mut loc {
			let map = Tag::from_str_padded(tag)
				.and_then(|t| avar.segments.get(&t))
				.ok_or_else(|| key_error(tag))?;
			*v = piecewise_linear_map(*v, map);
		}
	}
	// Quantize to F2Dot14, to avoid surprise interpolations.
	let loc: Location = loc
		.into_iter()
		.map(|(tag, v)| (tag, float_to_fixed_to_float(v, 14)))
		.collect();
	// Location is normalized now.
	log::info!("Normalized location: {loc:?}");

	let has_glyf = font.contains(GVAR);
	if has_glyf {
		mutate_gvar(font, &axes, &loc)?;
	}

	font.remove(DSIG);

	if font.contains(CVAR) {
		mutate_cvar(font, &axes, &loc)?;
	}
	if font.contains(CFF2) {
		mutate_cff2(font, &axes, &loc)?;
	}
	if font.contains(MVAR) {
		mutate_mvar(font, &axes, &loc)?;
	}

	log::info!("Mutating FeatureVariations");
	if font.contains(GSUB) {
		let table = &mut font.require::<Gsub>()?.table;
		mutate_feature_variations(
			&mut table.feature_variations,
			&mut table.feature_list,
			&axes,
			&loc,
		)?;
	}
	if font.contains(GPOS) {
		let table = &mut font.require::<Gpos>()?.table;
		mutate_feature_variations(
			&mut table.feature_variations,
			&mut table.feature_list,
			&axes,
			&loc,
		)?;
	}

	if font.contains(GDEF) && font.require::<Gdef>()?.version >= 0x0001_0003 {
		mutate_gdef(font, &axes, &loc)?;
	}

	let mut add_idef = false;
	if has_glyf {
		let glyf = font.require_locked::<Glyf>(false)?;
		// An empty `glyf` is false.
		if !glyf.glyphs.is_empty() {
			add_idef = uses_getvariation(glyf)?;
			if overlap {
				for glyph in glyf.glyphs.values_mut() {
					glyph.data = None;
					if glyph.is_composite() {
						glyph.components.first_mut().ok_or_else(index_error)?.flags |=
							OVERLAP_COMPOUND;
					} else if glyph.number_of_contours > 0 {
						*glyph.flags.first_mut().ok_or_else(index_error)? |= FLAG_OVERLAP_SIMPLE;
					}
				}
			}
		}
	}
	if add_idef {
		add_getvariation_idef(font, &axes, &loc)?;
	}

	if font.contains(NAME) {
		prune_names(font)?;
	}

	let user = |tag: &str| location.iter().rev().find(|(t, _)| t == tag).map(|e| e.1);
	if let Some(wght) = user("wght")
		&& font.contains(OS2)
	{
		font.require::<Os2>()?.us_weight_class =
			fit(ot_round(wght.min(1000.0).max(1.0)), "usWeightClass")?;
	}
	if let Some(wdth) = user("wdth") {
		if !font.contains(OS2) {
			return Err(key_error("OS/2"));
		}
		font.require::<Os2>()?.us_width_class = os2_width_class(wdth);
	}
	if let Some(slnt) = user("slnt")
		&& font.contains(POST)
	{
		font.require::<Post>()?.italic_angle = slnt.min(90.0).max(-90.0);
	}

	log::info!("Removing variable tables");
	for tag in VARIABLE_TABLES {
		font.remove(Tag::new(tag));
	}
	Ok(())
}

/// `os.path.splitext(path)[0]`.
fn strip_extension(path: &str) -> &str {
	let separators: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
	let start = path.rfind(separators).map_or(0, |at| at + 1);
	let name = &path[start..];
	let leading = name.len() - name.trim_start_matches('.').len();
	match name[leading..].rfind('.') {
		Some(at) => &path[..start + leading + at],
		None => path,
	}
}

/// The help of [`main`].
const HELP: &str = "\
usage: fonttools varLib.mutator [-h] [-o OUTPUT.ttf] [--no-recalc-timestamp]
                                [-v | -q] [--no-overlap]
                                INPUT.ttf [AXIS=LOC ...]

Instantiate a variable font

positional arguments:
  INPUT.ttf             Input variable TTF file.
  AXIS=LOC              List of space separated locations. A location consist
                        in the name of a variation axis, followed by '=' and a
                        number. E.g.: wght=700 wdth=80. The default is the
                        location of the base master.

options:
  -h, --help            show this help message and exit
  -o, --output OUTPUT.ttf
                        Output instance TTF file (default: INPUT-
                        instance.ttf).
  --no-recalc-timestamp
                        Don't set the output font's timestamp to the current
                        time.
  -v, --verbose         Run more verbosely.
  -q, --quiet           Turn verbosity off.
  --no-overlap          Don't set OVERLAP_SIMPLE/OVERLAP_COMPOUND glyf flags.
";

/// The help text of the command line.
pub fn help() -> &'static str {
	HELP
}

fn usage_error(message: &str) -> Error {
	Error::value(format!("error: {message}"))
}

/// `main`: instantiates a variable font file; returns the exit code.
pub fn main(args: &[&str]) -> Result<i32> {
	let mut positional = Vec::new();
	let mut output = None;
	let mut recalc_timestamp = true;
	let mut overlap = true;
	let mut verbosity = None;
	let mut only_positionals = false;
	let mut it = args.iter();
	while let Some(&arg) = it.next() {
		if only_positionals || !arg.starts_with('-') || arg == "-" {
			positional.push(arg);
			continue;
		}
		let (name, inline) = match arg.split_once('=') {
			Some((name, value)) if name.starts_with("--") => (name, Some(value)),
			_ => (arg, None),
		};
		match name {
			"--" => only_positionals = true,
			"-h" | "--help" => {
				print!("{HELP}");
				return Ok(0);
			}
			"-o" | "--output" => {
				output =
					Some(inline.or_else(|| it.next().copied()).ok_or_else(|| {
						usage_error("argument -o/--output: expected one argument")
					})?);
			}
			"--no-recalc-timestamp" => recalc_timestamp = false,
			"--no-overlap" => overlap = false,
			"-v" | "--verbose" | "-q" | "--quiet" => {
				let quiet = name.starts_with("-q") || name == "--quiet";
				match verbosity {
					Some(q) if q != quiet => {
						let (this, other) = if quiet {
							("-q/--quiet", "-v/--verbose")
						} else {
							("-v/--verbose", "-q/--quiet")
						};
						return Err(usage_error(&format!(
							"argument {this}: not allowed with argument {other}"
						)));
					}
					_ => verbosity = Some(quiet),
				}
			}
			_ => return Err(usage_error(&format!("unrecognized arguments: {arg}"))),
		}
	}
	let Some((&input, locargs)) = positional.split_first() else {
		return Err(usage_error(
			"the following arguments are required: INPUT.ttf",
		));
	};
	let outfile = match output {
		Some(output) if !output.is_empty() => output.to_owned(),
		_ => format!("{}-instance.ttf", strip_extension(input)),
	};

	let mut loc: Vec<(String, f64)> = Vec::new();
	for arg in locargs {
		let invalid = || {
			usage_error(&format!(
				"invalid location argument format: {}",
				str_repr(arg)
			))
		};
		let parts: Vec<&str> = arg.split('=').collect();
		let [tag, val] = parts.as_slice() else {
			return Err(invalid());
		};
		let count = tag.chars().count();
		if count > 4 {
			return Err(invalid());
		}
		let value = py::parse_float(val).ok_or_else(invalid)?;
		let tag = format!("{tag}{}", " ".repeat(4 - count));
		match loc.iter_mut().find(|(t, _)| *t == tag) {
			Some(entry) => entry.1 = value,
			None => loc.push((tag, value)),
		}
	}
	log::info!("Location: {loc:?}");

	log::info!("Loading variable font");
	let mut varfont = Font::read(
		std::fs::read(input)?,
		ReadOptions {
			recalc_timestamp,
			..Default::default()
		},
	)?;

	instantiate_variable_font(&mut varfont, &loc, overlap)?;

	log::info!("Saving instance font {outfile}");
	std::fs::write(outfile, varfont.write(&WriteOptions::default())?)?;
	Ok(0)
}
