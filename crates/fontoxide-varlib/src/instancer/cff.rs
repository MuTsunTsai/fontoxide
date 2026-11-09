//! Instancing of `CFF2` (`instantiateCFF2` and `fontTools.varLib.instancer.cff`).
//!
//! The algorithm is rather simple: take all blend operations and store their deltas in
//! the (otherwise empty) `CFF2` variation store. Then, instantiate the store with the
//! given axis limits, and read back the new deltas. This is done for both the
//! charstrings and the Private DICTs. Then prune unused things and possibly drop the
//! store if it's empty.
//!
//! Charstring coordinates are relative, so rounding them one by one lets the errors
//! add up and leaves contours open. In charstrings with blends they are made absolute
//! before instancing and relative again after, so they round like `gvar` points.

use fontoxide_misc::round_tools::ot_round;
use fontoxide_ttlib::cff::charstring::Token;
use fontoxide_ttlib::cff::dict::{Arg as DictArg, Value};
use fontoxide_ttlib::cff::font::{DictKind, dict_order, dict_table, num_regions};
use fontoxide_ttlib::cff::specializer::{
	Arg, Command, SpecializeOptions, blend_arg_values, commands_to_program, generalize_commands,
	num_blend_arg_values, program_to_commands, specialize_commands,
};
use fontoxide_ttlib::cff::transforms::{all_privates, glyph_private, private_of, private_of_mut};
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::var_store::region_support;
use fontoxide_ttlib::tables::Cff2;
use fontoxide_ttlib::tables::glyf::Number;
use fontoxide_ttlib::var::{Location, support_scalar};
use fontoxide_ttlib::{Error, Font, Result};

use super::avar::{ReachableRanges, cull_tuple_var_store};
use super::item_store::TupleVarStore;
use super::limits::{NormalizedLimits, fvar_axes};

fn assertion(what: &str) -> Error {
	Error::Assertion(what.into())
}

fn index_error() -> Error {
	Error::ttlib("IndexError: list index out of range")
}

fn nested_blends() -> Error {
	Error::ttlib("NotImplementedError: Nested blend lists not supported (yet)")
}

/// `getRegionScalarRanges`: each region's scalar range after pinning the given axes.
/// These ranges let flex1 generalization determine whether its direction is fixed
/// throughout the remaining design space.
fn region_scalar_ranges(
	store: &ot::VarStore,
	axes: &[String],
	pinned: &Location,
) -> Result<Vec<(f64, f64)>> {
	let list = store
		.var_region_list
		.as_ref()
		.ok_or_else(|| Error::ttlib("AttributeError: no attribute 'Region'"))?;
	let mut ranges = Vec::with_capacity(list.region.len());
	for region in &list.region {
		let support = region_support(region, axes)?;
		let pinned_support: Vec<(&str, (f64, f64, f64))> = axes
			.iter()
			.filter(|tag| pinned.contains_key(*tag))
			.filter_map(|tag| support.get(tag).map(|t| (tag.as_str(), *t)))
			.collect();
		let scalar = support_scalar(pinned, pinned_support.iter().copied());
		// Invalid tents are left unknown: supportScalar ignores what instancing zeroes.
		ranges.push(
			if pinned_support.iter().any(|(_, (lower, peak, upper))| {
				lower > peak || peak > upper || (*lower < 0.0 && 0.0 < *upper)
			}) {
				(0.0, 1.0)
			} else if pinned_support.len() == support.len() {
				(scalar, scalar)
			} else {
				(0.0, scalar)
			},
		);
	}
	Ok(ranges)
}

/// `convertPrivateDictDeltas`: converts a Private DICT delta array's region deltas.
///
/// Defaults are already absolute; region deltas are relative to the preceding value.
/// Make those absolute before instancing, and relative again afterwards.
fn convert_private_dict_deltas(
	values: &[Arg],
	regions: usize,
	to_absolute: bool,
) -> Result<Vec<Arg>> {
	let mut previous = vec![Arg::Int(0); regions];
	let mut result = Vec::with_capacity(values.len());
	for value in values {
		let (default, mut deltas) = match value {
			Arg::Blend(v) => {
				let (first, rest) = v
					.split_first()
					.ok_or_else(|| Error::value("not enough values to unpack"))?;
				(first.clone(), rest.to_vec())
			}
			value => (value.clone(), vec![Arg::Int(0); regions]),
		};
		if deltas.len() != regions {
			return Err(assertion("len(deltas) == numRegions"));
		}
		let mut item = vec![default];
		if to_absolute {
			for (d, p) in deltas.iter_mut().zip(&previous) {
				*d = p.plus(d)?;
			}
			item.extend_from_slice(&deltas);
		} else {
			for (d, p) in deltas.iter().zip(&previous) {
				item.push(d.minus(p)?);
			}
		}
		result.push(Arg::Blend(item));
		previous = deltas;
	}
	Ok(result)
}

/// `_isStemCommand`.
fn is_stem_command(commands: &[Command], i: usize) -> bool {
	let command = &commands[i];
	if matches!(command.op, "hstem" | "hstemhm" | "vstem" | "vstemhm") {
		return true;
	}
	// Implicit vstems before hintmask/cntrmask.
	command.op.is_empty()
		&& commands
			.get(i + 1)
			.is_some_and(|c| matches!(c.op, "hintmask" | "cntrmask"))
		&& !command.args.iter().any(|a| matches!(a, Arg::Mask(_)))
}

fn is_path_operator(op: &str) -> bool {
	matches!(op, "rmoveto" | "rlineto" | "rrcurveto" | "flex")
}

fn any_nonzero(values: &[Arg]) -> bool {
	values.iter().any(Arg::is_truthy)
}

/// `convertToAbsoluteCoordinates`: converts generalized `CFF2` outline and stem
/// coordinates in place. On failure, the reason the commands cannot safely be
/// converted, before changing anything. All flex operators must already have been
/// generalized to `flex`; an ambiguous variable `flex1` cannot be made absolute.
fn convert_to_absolute_coordinates(
	commands: &mut [Command],
	mut vs_index: i64,
	get_num_regions: &dyn Fn(i64) -> Result<i64>,
) -> Result<Option<&'static str>> {
	let mut seen_blend = false;
	for command in commands.iter() {
		if command.op == "vsindex" {
			if seen_blend {
				return Ok(Some("vsindex after a blend"));
			}
		} else if command.op == "flex1" {
			return Ok(Some("flex1 changes direction across the design space"));
		}
		if matches!(command.op, "hflex" | "hflex1") {
			return Err(assertion("op not in {\"hflex\", \"hflex1\"}"));
		}
		seen_blend = seen_blend || command.args.iter().any(Arg::is_blend);
	}

	let mut num_values = get_num_regions(vs_index)? as usize + 1;
	let mut outline = [vec![Arg::Int(0); num_values], vec![Arg::Int(0); num_values]];
	for i in 0..commands.len() {
		if commands[i].op == "vsindex" {
			// Before any blend, so no deltas to carry over.
			vs_index = match commands[i].args.first() {
				Some(Arg::Int(v)) => *v,
				_ => return Err(index_error()),
			};
			num_values = get_num_regions(vs_index)? as usize + 1;
			for axis in &mut outline {
				axis.truncate(1);
				axis.resize(num_values, Arg::Int(0));
			}
			continue;
		}
		let is_path = is_path_operator(commands[i].op);
		let mut stem = [vec![Arg::Int(0); num_values]];
		let positions: &mut [Vec<Arg>] = if is_path {
			&mut outline
		} else if is_stem_command(commands, i) {
			&mut stem
		} else {
			continue;
		};
		let count = if commands[i].op == "flex" {
			12
		} else {
			commands[i].args.len()
		};
		for j in 0..count {
			let k = j % positions.len();
			let arg = commands[i].args.get(j).ok_or_else(index_error)?;
			let values = blend_arg_values(arg, num_values)?;
			let position = positions[k]
				.iter()
				.zip(&values)
				.map(|(p, v)| p.plus(v))
				.collect::<Result<Vec<_>>>()?;
			commands[i].args[j] = if any_nonzero(&position[1..]) {
				let mut blend = position.clone();
				blend.push(Arg::Int(1));
				Arg::Blend(blend)
			} else {
				position[0].clone()
			};
			positions[k] = position;
		}
	}
	Ok(None)
}

/// `round`, or nothing.
fn round_number(arg: &Arg, round: bool) -> Result<Arg> {
	Ok(if round {
		Arg::Int(ot_round(arg.number()?))
	} else {
		arg.clone()
	})
}

/// `roundArg`.
fn round_arg(arg: &Arg, round: bool) -> Result<Arg> {
	match arg {
		Arg::Blend(items) => {
			let mut new = items.clone();
			*new.first_mut().ok_or_else(index_error)? = round_number(&items[0], round)?;
			Ok(Arg::Blend(new))
		}
		arg => round_number(arg, round),
	}
}

/// `convertToRelativeCoordinates`: rounds absolute `CFF2` coordinates and makes them
/// relative again in place.
fn convert_to_relative_coordinates(commands: &mut [Command], round: bool) -> Result<()> {
	let mut outline = [vec![Arg::Int(0)], vec![Arg::Int(0)]];
	for i in 0..commands.len() {
		let is_path = is_path_operator(commands[i].op);
		let mut stem = [vec![Arg::Int(0)]];
		let positions: &mut [Vec<Arg>] = if is_path {
			&mut outline
		} else if is_stem_command(commands, i) {
			&mut stem
		} else {
			continue;
		};
		let count = if commands[i].op == "flex" {
			12
		} else {
			commands[i].args.len()
		};
		for j in 0..count {
			let k = j % positions.len();
			let arg = round_arg(commands[i].args.get(j).ok_or_else(index_error)?, round)?;
			let num_values = positions[k].len().max(num_blend_arg_values(&arg));
			let values = blend_arg_values(&arg, num_values)?;
			let mut previous = positions[k].clone();
			previous.resize(num_values, Arg::Int(0));
			let diff = values
				.iter()
				.zip(&previous)
				.map(|(v, p)| v.minus(p))
				.collect::<Result<Vec<_>>>()?;
			positions[k] = values;
			commands[i].args[j] = if any_nonzero(&diff[1..]) {
				let mut blend = diff;
				blend.push(Arg::Int(1));
				Arg::Blend(blend)
			} else {
				diff[0].clone()
			};
		}
		if commands[i].op == "flex" {
			let fd = commands[i].args.get(12).ok_or_else(index_error)?;
			commands[i].args[12] = round_arg(fd, round)?;
		}
	}
	Ok(())
}

fn to_number(arg: &Arg) -> Result<Number> {
	match arg {
		Arg::Int(v) => Ok(Number::Int(*v)),
		Arg::Float(v) => Ok(Number::Float(*v)),
		_ => Err(nested_blends()),
	}
}

fn from_number(number: Number) -> Arg {
	match number {
		Number::Int(v) => Arg::Int(v),
		Number::Float(v) => Arg::Float(v),
	}
}

fn value_to_arg(value: &Value) -> Result<Arg> {
	match value {
		Value::Int(v) => Ok(Arg::Int(*v)),
		Value::Float(v) => Ok(Arg::Float(*v)),
		Value::List(items) => Ok(Arg::Blend(
			items.iter().map(value_to_arg).collect::<Result<_>>()?,
		)),
		_ => Err(Error::value("TypeError: not a number")),
	}
}

fn arg_to_value(arg: &Arg) -> Result<Value> {
	match arg {
		Arg::Int(v) => Ok(Value::Int(*v)),
		Arg::Float(v) => Ok(Value::Float(*v)),
		Arg::Blend(items) => Ok(Value::List(
			items.iter().map(arg_to_value).collect::<Result<_>>()?,
		)),
		Arg::Mask(_) | Arg::Op(_) => Err(Error::value("TypeError: not a number")),
	}
}

/// The rows of the items of each variation data, while they are in the store.
struct Rows {
	rows: Vec<Vec<Vec<Number>>>,
}

impl Rows {
	/// `storeBlendsToVarStore`.
	fn store(&mut self, arg: &[Arg], vs_index: i64, regions: i64) -> Result<()> {
		let (last, values) = arg.split_last().ok_or_else(index_error)?;
		if values.iter().any(Arg::is_blend) {
			return Err(nested_blends());
		}
		let Arg::Int(count) = *last else {
			return Err(Error::value("TypeError: not an integer"));
		};
		if count == 0 {
			return Err(Error::value("ZeroDivisionError: integer modulo by zero"));
		}
		let count = count as usize;
		if values.len() % count != 0 {
			return Err(assertion("(len(arg) - 1) % count == 0"));
		}
		let n_regions = values.len() / count - 1;
		if n_regions as i64 != regions {
			return Err(assertion("nRegions == getNumRegions(vsindex)"));
		}
		if n_regions == 0 {
			if count < values.len() {
				return Err(Error::value("range() arg 3 must not be zero"));
			}
			// `range(count, count, 0)` also raises.
			return Err(Error::value("range() arg 3 must not be zero"));
		}
		let data = usize::try_from(vs_index)
			.ok()
			.and_then(|i| self.rows.get_mut(i))
			.ok_or_else(index_error)?;
		for deltas in values[count..].chunks(n_regions) {
			data.push(deltas.iter().map(to_number).collect::<Result<_>>()?);
		}
		Ok(())
	}
}

/// What reading back from the instantiated store needs.
struct Fetcher<'a> {
	rows: Vec<Vec<Vec<Number>>>,
	defaults: Vec<Vec<Number>>,
	cursor: Vec<usize>,
	regions: &'a dyn Fn(i64) -> Result<i64>,
}

impl Fetcher<'_> {
	/// `fetchBlendsFromVarStore`.
	fn fetch(
		&mut self,
		arg: &Arg,
		vs_index: i64,
		round_default: bool,
		round_deltas: bool,
	) -> Result<Vec<Arg>> {
		let Arg::Blend(items) = arg else {
			return Ok(vec![arg.clone()]);
		};
		let (last, values) = items.split_last().ok_or_else(index_error)?;
		if values.iter().any(Arg::is_blend) {
			return Err(nested_blends());
		}
		let Arg::Int(count) = *last else {
			return Err(Error::value("TypeError: not an integer"));
		};
		let count = count as usize;
		if count == 0 {
			return Err(Error::value("ZeroDivisionError: integer modulo by zero"));
		}
		if values.len() % count != 0 {
			return Err(assertion("(len(arg) - 1) % count == 0"));
		}
		let num_regions = (self.regions)(vs_index)? as usize;
		let major = usize::try_from(vs_index).map_err(|_| index_error())?;
		let mut new_defaults = Vec::with_capacity(count);
		let mut new_deltas = Vec::with_capacity(count * num_regions);
		for default in &values[..count] {
			let cursor = self.cursor.get_mut(major).ok_or_else(index_error)?;
			let minor = *cursor;
			*cursor += 1;
			let delta = self
				.defaults
				.get(major)
				.and_then(|d| d.get(minor))
				.ok_or_else(|| Error::ttlib(format!("KeyError: {minor}")))?;
			let delta = round_number(&from_number(*delta), round_default)?;
			new_defaults.push(default.plus(&delta)?);
			let deltas = self
				.rows
				.get(major)
				.and_then(|d| d.get(minor))
				.ok_or_else(index_error)?;
			if deltas.len() != num_regions {
				return Err(assertion("len(deltas) == numRegions"));
			}
			for delta in deltas {
				new_deltas.push(round_number(&from_number(*delta), round_deltas)?);
			}
		}
		if num_regions == 0 {
			return Ok(new_defaults); // No deltas, just return the defaults
		}
		new_defaults.extend(new_deltas);
		new_defaults.push(Arg::Int(count as i64));
		Ok(vec![Arg::Blend(new_defaults)])
	}
}

/// The entries of a Private DICT that can have blends, in the order of the DICT.
fn blendable_entries() -> Vec<(&'static str, DictArg)> {
	let table = dict_table(DictKind::Private);
	dict_order(DictKind::Private, true)
		.into_iter()
		.filter(|name| *name != "vsindex")
		.filter_map(|name| {
			fontoxide_ttlib::cff::dict::operator(table, name).and_then(|op| {
				matches!(op.arg, DictArg::Number | DictArg::Delta | DictArg::Array)
					.then_some((name, op.arg))
			})
		})
		.collect()
}

fn private_vs_index(value: Option<Value>) -> Result<i64> {
	match value {
		None => Ok(0),
		Some(v) => v.int(),
	}
}

/// `instantiateCFF2`. With `downgrade`, no actual downgrading is done, but this returns
/// whether the store was empty after instantiation, and hence a downgrade to CFF is
/// possible; except that a font that wasn't variable at all is converted right away.
#[allow(clippy::too_many_lines)]
pub fn instantiate_cff2(
	font: &mut Font,
	limits: &NormalizedLimits,
	round: bool,
	downgrade: bool,
	reachable_ranges: Option<&ReachableRanges>,
) -> Result<bool> {
	log::info!("Instantiating CFF2 table");
	let axes: Vec<String> = fvar_axes(font)?.into_iter().map(|a| a.0).collect();
	let has_store = font
		.require::<Cff2>()?
		.cff
		.top_dicts
		.first()
		.ok_or_else(index_error)?
		.var_store
		.is_some();
	if !has_store {
		if downgrade {
			fontoxide_ttlib::cff::cff2_to_cff::convert_cff2_to_cff(font, true)?;
		}
		return Ok(false);
	}

	let cff = &mut font.require::<Cff2>()?.cff;
	cff.desubroutinize()?;
	let top = cff.top_dicts.first_mut().ok_or_else(index_error)?;
	let Some(mut store) = top.var_store.take() else {
		return Err(index_error());
	};

	let pinned: Location = limits.pinned_location().into_iter().collect();
	let scalar_ranges = region_scalar_ranges(&store, &axes, &pinned)?;

	let count = top.char_strings.as_ref().map_or(0, |c| c.items.len());
	if top.fd_array.is_none() {
		return Err(Error::ttlib("AttributeError: FDArray"));
	}
	let privates = all_privates(top);

	// The commands of each glyph, and whether its coordinates are absolute.
	let mut char_string_data: Vec<(usize, Vec<Command>, bool)> = Vec::with_capacity(count);
	{
		let current_store: &ot::VarStore = &store;
		let get_num_regions = |vs: i64| num_regions(Some(current_store), vs);
		let get_scalar_ranges = |vs: i64| -> Result<Vec<(f64, f64)>> {
			let data = usize::try_from(vs)
				.ok()
				.and_then(|i| current_store.var_data.get(i))
				.and_then(|d| d.as_deref())
				.ok_or_else(index_error)?;
			data.var_region_index
				.iter()
				.map(|&i| {
					scalar_ranges
						.get(usize::from(i))
						.copied()
						.ok_or_else(index_error)
				})
				.collect()
		};
		for gid in 0..count {
			let private_ref = glyph_private(top, gid)?;
			let private = private_of(top, private_ref).ok_or_else(|| {
				Error::ttlib("AttributeError: 'NoneType' object has no attribute 'vstore'")
			})?;
			let vs_index = private_vs_index(private.get("vsindex"))?;
			let char_strings = top.char_strings.as_ref().ok_or_else(index_error)?;
			let name = &char_strings.names[gid];
			let mut commands = program_to_commands(
				&char_strings.items[gid].program,
				Some(&get_num_regions),
				vs_index,
			)?;
			let mut absolute = false;
			if commands.iter().any(|c| c.args.iter().any(Arg::is_blend)) {
				let mut generalized =
					generalize_commands(&commands, false, Some(&get_scalar_ranges), vs_index)?;
				match convert_to_absolute_coordinates(&mut generalized, vs_index, &get_num_regions)?
				{
					None => {
						commands = generalized;
						absolute = true;
					}
					Some(reason) => {
						log::warn!("Glyph '{name}': {reason}; rounding its relative coordinates")
					}
				}
			}
			if !absolute {
				commands = specialize_commands(&commands, &SpecializeOptions::default())?;
			}
			char_string_data.push((gid, commands, absolute));
		}
	}

	// Check the VarData's are empty.
	for data in store.var_data.iter().flatten() {
		if !data.item.is_empty() {
			return Err(assertion("varData.Item == []"));
		}
		if data.item_count.unwrap_or(0) != 0 {
			return Err(assertion("varData.ItemCount == 0"));
		}
	}

	// Add the charstring blend lists to the store, so that they can be instantiated.
	let mut rows = Rows {
		rows: vec![Vec::new(); store.var_data.len()],
	};
	for (gid, commands, _) in &char_string_data {
		let private = private_of(top, glyph_private(top, *gid)?).ok_or_else(index_error)?;
		let mut vs_index = private_vs_index(private.get("vsindex"))?;
		for command in commands {
			if command.op == "vsindex" {
				vs_index = match command.args.first() {
					Some(Arg::Int(v)) => *v,
					_ => return Err(index_error()),
				};
				continue;
			}
			for arg in &command.args {
				if let Arg::Blend(items) = arg {
					rows.store(items, vs_index, num_regions(Some(&store), vs_index)?)?;
				}
			}
		}
	}

	// Add the private blend lists to the store, so that values can be instantiated.
	let entries = blendable_entries();
	for &private_ref in &privates {
		let private = private_of_mut(top, private_ref).ok_or_else(|| {
			Error::ttlib("AttributeError: 'FontDict' object has no attribute 'Private'")
		})?;
		let vs_index = private_vs_index(private.get("vsindex"))?;
		for &(name, kind) in &entries {
			let Some(value) = private.get(name) else {
				continue;
			};
			let regions = || num_regions(Some(&store), vs_index);
			let mut values: Vec<Arg> = match (&value, kind) {
				(_, DictArg::Number) => vec![value_to_arg(&value)?],
				(Value::List(items), _) => items.iter().map(value_to_arg).collect::<Result<_>>()?,
				_ => return Err(Error::value("TypeError: 'int' object is not iterable")),
			};
			if kind == DictArg::Delta && values.iter().any(Arg::is_blend) {
				values = convert_private_dict_deltas(&values, regions()? as usize, true)?;
				let items = values.iter().map(arg_to_value).collect::<Result<_>>()?;
				private.values.insert(name, Value::List(items));
			}
			for value in &values {
				let Arg::Blend(items) = value else {
					continue;
				};
				let source_count = regions()? as usize + 1;
				if items.len() % source_count != 0 {
					return Err(assertion("len(value) % (getNumRegions(vsindex) + 1) == 0"));
				}
				let mut with_count = items.clone();
				with_count.push(Arg::Int((items.len() / source_count) as i64));
				rows.store(&with_count, vs_index, regions()?)?;
			}
		}
	}

	// Instantiate the store.
	for (data, rows) in store.var_data.iter_mut().flatten().zip(&rows.rows) {
		data.item_count = Some(rows.len() as u32);
	}
	let mut tuple_store = TupleVarStore::from_rows(&store, &rows.rows, &axes)?;
	let defaults = tuple_store.instantiate_numbers(limits, false)?;
	let mut items = tuple_store.items();
	let new_store = tuple_store.as_item_var_store(false)?;
	store.var_region_list = new_store.var_region_list;
	if store.var_data_count.is_some() && store.var_data_count != new_store.var_data_count {
		return Err(assertion(
			"itemVarStore.VarDataCount == newItemVarStore.VarDataCount",
		));
	}
	store.var_data_count = new_store.var_data_count;
	store.var_data = new_store.var_data;

	// Optionally cull dead regions (for avar2 partial instancing).
	if let Some(ranges) = reachable_ranges.filter(|r| !r.is_empty()) {
		let mut culled = TupleVarStore::from_rows(&store, &items, &axes)?;
		if cull_tuple_var_store(&mut culled, ranges) != 0 {
			culled.rebuild_regions();
			items = culled.items();
			let new_store = culled.as_item_var_store(false)?;
			store.var_region_list = new_store.var_region_list;
			store.var_data = new_store.var_data;
		}
	}

	// Read back the new blends from the instantiated store.
	let store_ref: &ot::VarStore = &store;
	let get_num_regions = |vs: i64| num_regions(Some(store_ref), vs);
	let mut fetcher = Fetcher {
		cursor: vec![0; items.len()],
		rows: items,
		defaults,
		regions: &get_num_regions,
	};
	for (gid, commands, absolute) in &mut char_string_data {
		let private = private_of(top, glyph_private(top, *gid)?).ok_or_else(index_error)?;
		let mut vs_index = private_vs_index(private.get("vsindex"))?;
		let round_default = round && !*absolute;
		for command in commands.iter_mut() {
			if command.op == "vsindex" {
				vs_index = match command.args.first() {
					Some(Arg::Int(v)) => *v,
					_ => return Err(index_error()),
				};
				continue;
			}
			let mut new_args = Vec::with_capacity(command.args.len());
			for arg in &command.args {
				new_args.extend(fetcher.fetch(arg, vs_index, round_default, true)?);
			}
			command.args = new_args;
		}
		if !*absolute {
			continue;
		}
		convert_to_relative_coordinates(commands, round)?;
		*commands = specialize_commands(
			commands,
			&SpecializeOptions {
				generalize_first: false,
				..Default::default()
			},
		)?;
	}

	for &private_ref in &privates {
		let private = private_of_mut(top, private_ref).ok_or_else(index_error)?;
		let vs_index = private_vs_index(private.get("vsindex"))?;
		for &(name, kind) in &entries {
			let Some(value) = private.get(name) else {
				continue;
			};
			let values: Vec<Arg> = match (&value, kind) {
				(_, DictArg::Number) => vec![value_to_arg(&value)?],
				(Value::List(items), _) => items.iter().map(value_to_arg).collect::<Result<_>>()?,
				_ => return Err(Error::value("TypeError: 'int' object is not iterable")),
			};
			let no_round = matches!(
				name,
				"BlueScale" | "ExpansionFactor" | "StdHW" | "StdVW" | "StemSnapH" | "StemSnapV"
			);
			let mut new_values = Vec::with_capacity(values.len());
			for value in values {
				let Arg::Blend(mut items) = value else {
					new_values.push(value);
					continue;
				};
				items.push(Arg::Int(1));
				let fetched =
					fetcher.fetch(&Arg::Blend(items), vs_index, round && !no_round, !no_round)?;
				for v in fetched {
					new_values.push(match v {
						Arg::Blend(mut items) => {
							items.pop();
							Arg::Blend(items)
						}
						v => v,
					});
				}
			}
			let new_value = match kind {
				DictArg::Number => arg_to_value(new_values.first().ok_or_else(index_error)?)?,
				_ => {
					if kind == DictArg::Delta && new_values.iter().any(Arg::is_blend) {
						new_values = convert_private_dict_deltas(
							&new_values,
							get_num_regions(vs_index)? as usize,
							false,
						)?;
					}
					Value::List(new_values.iter().map(arg_to_value).collect::<Result<_>>()?)
				}
			};
			private.values.insert(name, new_value);
		}
	}

	// Empty out the store.
	let cursor = std::mem::take(&mut fetcher.cursor);
	let rows_read = std::mem::take(&mut fetcher.rows);
	let has_regions: Vec<bool> = store_ref
		.var_data
		.iter()
		.map(|d| d.as_deref().is_some_and(|d| !d.var_region_index.is_empty()))
		.collect();
	for (i, used) in cursor.iter().enumerate() {
		if *used != rows_read[i].len() {
			return Err(assertion("varDataCursor[i] == varData.ItemCount"));
		}
	}
	for data in store.var_data.iter_mut().flatten() {
		data.item = Vec::new();
		data.item_count = Some(0);
	}

	// Remove vsindex commands that are no longer needed.
	for (_, commands, _) in &mut char_string_data {
		if !commands.iter().any(|c| c.args.iter().any(Arg::is_blend)) {
			commands.retain(|c| c.op != "vsindex");
		}
	}

	// Remove unused VarData and update vsindex values.
	let vs_index_mapping: Vec<Option<i64>> = {
		let mut next = 0;
		has_regions
			.iter()
			.map(|&used| {
				used.then(|| {
					next += 1;
					next - 1
				})
			})
			.collect()
	};
	let mut kept = has_regions.iter();
	store
		.var_data
		.retain(|_| kept.next().copied().unwrap_or(false));
	store.var_data_count = Some(store.var_data.len() as u32);
	let map_vs_index = |v: i64| -> Option<i64> {
		usize::try_from(v)
			.ok()
			.and_then(|i| vs_index_mapping.get(i).copied().flatten())
	};
	for (_, commands, _) in &mut char_string_data {
		for command in commands.iter_mut() {
			if command.op == "vsindex" {
				let Some(Arg::Int(v)) = command.args.first() else {
					return Err(index_error());
				};
				let new = map_vs_index(*v).ok_or_else(|| Error::ttlib(format!("KeyError: {v}")))?;
				command.args[0] = Arg::Int(new);
			}
		}
	}
	for &private_ref in &privates {
		let private = private_of_mut(top, private_ref).ok_or_else(index_error)?;
		if let Some(value) = private.get("vsindex") {
			match map_vs_index(value.int()?) {
				Some(new) => {
					private.values.insert("vsindex", Value::Int(new));
				}
				None => {
					// The referenced VarData lost all its regions; the dict's blends have
					// dissolved into plain values.
					private.values.remove("vsindex");
					private.raw.retain(|(k, _)| *k != "vsindex");
				}
			}
		}
	}

	// Remove initial vsindex commands that are implied.
	for (gid, commands, _) in &mut char_string_data {
		let private = private_of(top, glyph_private(top, *gid)?).ok_or_else(index_error)?;
		let vs_index = private_vs_index(private.get("vsindex"))?;
		if commands.first().is_some_and(|c| {
			c.op == "vsindex" && c.args.len() == 1 && c.args[0].py_eq(&Arg::Int(vs_index))
		}) {
			commands.remove(0);
		}
	}

	// Ship the charstrings!
	let char_strings = top.char_strings.as_mut().ok_or_else(index_error)?;
	for (gid, commands, _) in &char_string_data {
		let program: Vec<Token> = commands_to_program(commands)?;
		let cs = &mut char_strings.items[*gid];
		cs.bytecode = None;
		cs.program = program;
	}

	// Remove an empty store.
	if store.var_data.is_empty() {
		top.raw.retain(|(k, _)| *k != "VarStore");
		return Ok(downgrade);
	}
	top.var_store = Some(store);
	Ok(false)
}
