//! Port of the `fontTools.varLib.avar` package: building `avar` from a
//! designspace (`build`), mapping coordinates through it (`map`), and writing it
//! back as a designspace snippet (`unbuild`); and planning the standard axis
//! mappings of a variable font ([`plan`]).

pub mod cli;
pub mod plan;

use std::fmt::Write as _;

use fontoxide_designspacelib::DesignSpaceDocument;
use fontoxide_misc::fixed_tools::{fixed_to_float, float_to_fixed};
use fontoxide_misc::py::float_repr;
use fontoxide_misc::round_tools::ot_round;
use fontoxide_misc::xml_writer::escape_attr;
use fontoxide_ttlib::tables::{Avar, Fvar, Name};
use fontoxide_ttlib::var::{Location, VarStoreInstancer};
use fontoxide_ttlib::{Font, Tag};

use crate::build::{add_avar, add_fvar, load_designspace};
use crate::errors::{Result, VarLibError};
use crate::models::{DictLocation, VariationModel, normalize_value, piecewise_linear_map};

/// `fontTools.varLib.avar.build.build`: adds an `avar` table to a font from the
/// axes and mappings of a designspace, and an `fvar` table if it has none.
pub fn build(font: &mut Font, designspace: &mut DesignSpaceDocument) -> Result<()> {
	let ds = load_designspace(designspace, true, false)?;
	if !font.contains(Tag::new(b"fvar")) {
		if !font.contains(Tag::new(b"name")) {
			font.insert(Box::new(Name::default()));
		}
		add_fvar(font, &ds.axes, &ds.instances)?;
	}
	let axis_tags = fvar_tags(font)?;
	if font.contains(Tag::new(b"avar")) {
		log::warn!("avar table already present, overwriting.");
		font.remove(Tag::new(b"avar"));
	}
	add_avar(font, &ds.axes, &ds.axis_mappings, &axis_tags)?;
	Ok(())
}

fn fvar_tags(font: &mut Font) -> Result<Vec<String>> {
	Ok(font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| Tag::new(&a.axis_tag).to_string())
		.collect())
}

/// The minimum, default and maximum of the `fvar` axes, by tag.
#[allow(clippy::type_complexity)]
fn fvar_triples(font: &mut Font) -> Result<Vec<(String, (f64, f64, f64))>> {
	Ok(font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| {
			(
				Tag::new(&a.axis_tag).to_string(),
				(a.min_value, a.default_value, a.max_value),
			)
		})
		.collect())
}

/// `_denormalize`.
fn denormalize(v: f64, triple: (f64, f64, f64)) -> f64 {
	if v >= 0.0 {
		triple.1 + v * (triple.2 - triple.1)
	} else {
		triple.1 + v * (triple.1 - triple.0)
	}
}

/// `avar.renormalizeLocation(location, font, dropZeroes)`.
fn renormalize_location(
	font: &mut Font,
	location: &DictLocation,
	drop_zeroes: bool,
) -> Result<DictLocation> {
	let triples = fvar_triples(font)?;
	let axes: Vec<String> = triples.iter().map(|t| t.0.clone()).collect();
	let avar = font.require::<Avar>()?;
	if !matches!(avar.major_version, 1 | 2) {
		return Err(fontoxide_ttlib::Error::ttlib(
			"NotImplementedError: Unknown avar table version",
		)
		.into());
	}
	let mut mapped = DictLocation::new();
	for (tag, value) in location {
		let value = match Tag::from_str_padded(tag).and_then(|t| avar.segments.get(&t)) {
			Some(mapping) => piecewise_linear_map(*value, mapping),
			None => *value,
		};
		mapped.push((tag.clone(), value));
	}
	if avar.major_version < 2 {
		return Ok(mapped);
	}
	let store = avar.table.var_store.as_deref();
	let map = avar.table.var_idx_map.as_deref();
	let instancer =
		VarStoreInstancer::new(store, &axes, mapped.iter().cloned().collect::<Location>());
	let mut out = DictLocation::new();
	for (index, axis) in axes.iter().enumerate() {
		let mut v = float_to_fixed(
			mapped.iter().find(|(k, _)| k == axis).map_or(0.0, |e| e.1),
			14,
		);
		let var_idx = match map {
			Some(map) => map
				.mapping
				.get(index)
				.or(map.mapping.last())
				.copied()
				.unwrap_or(0xFFFF_FFFF),
			None => index as u64,
		};
		if store.is_some() {
			v += ot_round(instancer.get(var_idx)?);
			v = v.clamp(-(1 << 14), 1 << 14);
		}
		if v != 0 || !drop_zeroes {
			out.push((axis.clone(), fixed_to_float(v, 14)));
		}
	}
	Ok(out)
}

/// `fontTools.varLib.avar.map.map`: maps a location (user coordinates unless
/// `input_normalized`) through `avar`. `None` if the font has no `fvar`.
pub fn map(
	font: &mut Font,
	location: &[(String, f64)],
	input_normalized: bool,
	output_normalized: bool,
	drop_zeroes: bool,
) -> Result<Option<DictLocation>> {
	if !font.contains(Tag::new(b"fvar")) {
		return Ok(None);
	}
	let axes = fvar_triples(font)?;
	let triple = |tag: &str| axes.iter().find(|(t, _)| t == tag).map(|e| e.1);
	let mut unknown: Vec<&str> = location
		.iter()
		.filter(|(t, _)| triple(t).is_none())
		.map(|(t, _)| t.as_str())
		.collect();
	unknown.sort_unstable();
	if !unknown.is_empty() {
		return Err(fontoxide_ttlib::Error::value(format!(
			"Unknown axis tag(s): {}",
			unknown.join(", ")
		))
		.into());
	}
	let mut location: DictLocation = location.to_vec();
	if !input_normalized {
		location = location
			.into_iter()
			.map(|(t, v)| {
				let tr = triple(&t).unwrap_or_default();
				Ok((t, normalize_value(v, tr, false)?))
			})
			.collect::<Result<_>>()?;
	}
	if font.contains(Tag::new(b"avar")) {
		location = renormalize_location(font, &location, drop_zeroes)?;
	}
	if !output_normalized {
		location = location
			.into_iter()
			.map(|(t, v)| {
				let tr = triple(&t).unwrap_or_default();
				(t, denormalize(v, tr))
			})
			.collect();
	}
	Ok(Some(location))
}

/// `%g` of a mapped value.
fn format_g(v: f64) -> String {
	fontoxide_misc::py::format_g(v, 6)
}

/// `fontTools.varLib.avar.map.main`.
pub fn map_main(args: &[&str]) -> Result<Vec<String>> {
	let usage = |m: String| {
		VarLibError::Usage(
			fontoxide_misc::cli_tools::program_text(
				&format!("fonttools varLib.avar.map: error: {m}"),
				"fonttools varLib.avar.map",
				"varLib.avar.map",
			)
			.into_owned(),
		)
	};
	let (mut f, mut i, mut o) = (false, false, false);
	let mut positional = Vec::new();
	for &a in args {
		match a {
			"-f" => f = true,
			"-i" => i = true,
			"-o" => o = true,
			a => positional.push(a),
		}
	}
	let Some((font_path, coords)) = positional.split_first() else {
		return Err(usage(
			"the following arguments are required: varfont.ttf".into(),
		));
	};
	if coords.is_empty() {
		return Err(usage(
			"No coordinates provided. Please specify at least one axis coordinate (e.g., wght=500)"
				.into(),
		));
	}
	let mut font = if font_path.ends_with(".designspace") {
		let mut font = Font::new();
		let mut ds = DesignSpaceDocument::from_file(font_path)?;
		build(&mut font, &mut ds)?;
		font
	} else {
		let mut font = Font::read(std::fs::read(font_path)?, Default::default())?;
		if !font.contains(Tag::new(b"fvar")) {
			return Err(usage(format!(
				"Font '{font_path}' does not contain an 'fvar' table."
			)));
		}
		let _ = &mut font;
		font
	};
	let mut location = Vec::new();
	for item in coords {
		let (tag, value) = item.split_once('=').unwrap_or((item, ""));
		if tag.is_empty() || value.is_empty() {
			return Err(usage(format!(
				"Invalid coordinate {}. Expected AXIS=value, e.g. wght=500",
				fontoxide_misc::py::str_repr(item)
			)));
		}
		let v = fontoxide_misc::py::parse_float(value).ok_or_else(|| {
			usage(format!(
				"Invalid coordinate value in {}. Expected a number after '='",
				fontoxide_misc::py::str_repr(item)
			))
		})?;
		location.push((tag.to_owned(), v));
	}
	let mapped = match map(&mut font, &location, i, o, !f) {
		Ok(m) => m,
		Err(VarLibError::Ttlib(e)) => return Err(usage(e.to_string())),
		Err(e) => return Err(e),
	};
	let mapped =
		mapped.ok_or_else(|| fontoxide_ttlib::Error::Assertion("mapped is not None".into()))?;
	Ok(mapped
		.iter()
		.map(|(t, v)| format!("{t}={}", format_g(*v)))
		.collect())
}

/// `_pruneLocations`: the input locations that the poles need.
pub fn prune_locations(
	locations: &[DictLocation],
	poles: &[DictLocation],
	axis_tags: &[String],
) -> Result<Vec<DictLocation>> {
	let model = VariationModel::new(locations, Some(axis_tags), false, None)?;
	let same = |a: &DictLocation, b: &DictLocation| {
		a.len() == b.len()
			&& a.iter()
				.all(|(k, v)| b.iter().any(|(k2, v2)| k == k2 && v == v2))
	};
	let mut pins: Vec<DictLocation> = Vec::new();
	for p in poles {
		if !pins.iter().any(|x| x == p) {
			pins.push(p.clone());
		}
	}
	for location in poles {
		let i = locations
			.iter()
			.position(|l| same(l, location))
			.ok_or_else(|| fontoxide_ttlib::Error::value("location is not in list"))?;
		let i = model.mapping[i];
		let support = &model.supports[i];
		let support_axes: std::collections::BTreeSet<&String> =
			support.iter().map(|e| &e.0).collect();
		for (axis_tag, (min_v, _, max_v)) in support {
			for v in [*min_v, *max_v] {
				if v == -1.0 || v == 0.0 || v == 1.0 {
					continue;
				}
				let found = pins.iter().any(|pin| {
					let pin_axes: std::collections::BTreeSet<&String> =
						pin.iter().map(|e| &e.0).collect();
					pin_axes == support_axes
						&& pin
							.iter()
							.find(|(k, _)| k == axis_tag)
							.is_some_and(|e| e.1 == v)
				});
				if found {
					continue;
				}
				// No pin found. Go through the previous masters and find a suitable
				// pin.
				let mut added = false;
				for candidate_idx in (0..i).rev() {
					let candidate = &model.supports[candidate_idx];
					let candidate_axes: std::collections::BTreeSet<&String> =
						candidate.iter().map(|e| &e.0).collect();
					if candidate_axes != support_axes {
						continue;
					}
					let pin: DictLocation = candidate
						.iter()
						.map(|(k, (_, d, _))| (k.clone(), *d))
						.collect();
					if pin
						.iter()
						.find(|(k, _)| k == axis_tag)
						.is_some_and(|e| e.1 == v)
					{
						if !pins.contains(&pin) {
							pins.push(pin);
						}
						added = true;
						break;
					}
				}
				if !added {
					return Err(fontoxide_ttlib::Error::Assertion("No pin found".into()).into());
				}
			}
		}
	}
	Ok(pins)
}

/// A segment map and the axis mappings: maps from input to output locations.
pub type AvarMappings = (
	Vec<(String, Vec<(f64, f64)>)>,
	Vec<(DictLocation, DictLocation)>,
);

/// `mappings_from_avar`: the non-trivial segment maps of `avar` by axis tag, and
/// for version 2, the mappings from input to output locations; denormalized to
/// user coordinates if `denormalize`.
pub fn mappings_from_avar(font: &mut Font, denormalize_values: bool) -> Result<AvarMappings> {
	let axes = fvar_triples(font)?;
	let axis_tags: Vec<String> = axes.iter().map(|a| a.0.clone()).collect();
	if !font.contains(Tag::new(b"avar")) {
		return Ok((Vec::new(), Vec::new()));
	}
	let avar = font.require::<Avar>()?.clone();
	let identity = [(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)];
	let mut axis_maps: Vec<(String, Vec<(f64, f64)>)> = avar
		.segments
		.iter()
		.filter(|(_, seg)| {
			let mut s = (*seg).clone();
			s.sort_by(|a, b| a.0.total_cmp(&b.0));
			!seg.is_empty() && s != identity
		})
		.map(|(t, seg)| (t.to_string(), seg.clone()))
		.collect();
	let mut mappings: Vec<(DictLocation, DictLocation)> = Vec::new();

	if avar.major_version == 2 {
		let store = avar.table.var_store.as_deref().ok_or_else(|| {
			fontoxide_ttlib::Error::ttlib(
				"AttributeError: 'NoneType' object has no attribute 'VarRegionList'",
			)
		})?;
		let regions = &store
			.var_region_list
			.as_deref()
			.ok_or_else(|| fontoxide_ttlib::Error::ttlib("AttributeError: VarRegionList"))?
			.region;

		// Find all the input locations: the "poles" (locations of the peaks) and
		// the "corners" of the regions.
		type Loc = Vec<(String, f64)>;
		let mut poles: Vec<Loc> = vec![Vec::new()];
		let mut input_locations: Vec<Loc> = vec![Vec::new()];
		let add = |list: &mut Vec<Loc>, l: Loc| {
			if !list.contains(&l) {
				list.push(l);
			}
		};
		for var_data in store.var_data.iter().flatten() {
			for &region_index in &var_data.var_region_index {
				let region = regions.get(usize::from(region_index)).ok_or_else(|| {
					fontoxide_ttlib::Error::ttlib("IndexError: list index out of range")
				})?;
				let mut peak = Loc::new();
				let mut corners: Vec<Vec<(String, f64)>> = Vec::new();
				for (axis_index, axis) in region.var_region_axis.iter().enumerate() {
					if axis.peak_coord == 0.0 {
						continue;
					}
					let tag = axis_tags.get(axis_index).cloned().unwrap_or_default();
					peak.push((tag.clone(), axis.peak_coord));
					let mut corner = Vec::new();
					if axis.start_coord != 0.0 {
						corner.push((tag.clone(), axis.start_coord));
					}
					if axis.end_coord != 0.0 {
						corner.push((tag.clone(), axis.end_coord));
					}
					corners.push(corner);
				}
				add(&mut poles, peak.clone());
				add(&mut input_locations, peak);
				// `product(*corners)`.
				let mut product: Vec<Loc> = vec![Vec::new()];
				for c in &corners {
					let mut next = Vec::new();
					for p in &product {
						for item in c {
							let mut q = p.clone();
							q.push(item.clone());
							next.push(q);
						}
					}
					product = next;
				}
				for p in product {
					add(&mut input_locations, p);
				}
			}
		}
		// Sort them by number of axes, then by axis order.
		let index = |t: &str| axis_tags.iter().position(|a| a == t).unwrap_or(usize::MAX);
		input_locations.sort_by(|a, b| {
			a.len().cmp(&b.len()).then_with(|| {
				let ka: Vec<usize> = a.iter().map(|e| index(&e.0)).collect();
				let kb: Vec<usize> = b.iter().map(|e| index(&e.0)).collect();
				ka.cmp(&kb)
			})
		});
		let input_locations = prune_locations(&input_locations, &poles, &axis_tags)?;

		// Find the output locations, at input locations.
		let map = avar.table.var_idx_map.as_deref();
		let mut instancer = VarStoreInstancer::new(Some(store), &axis_tags, Location::new());
		for location in input_locations {
			instancer.set_location(location.iter().cloned().collect());
			let mut output = DictLocation::new();
			for (axis_index, tag) in axis_tags.iter().enumerate() {
				let var_idx = match map {
					Some(map) => map
						.mapping
						.get(axis_index)
						.or(map.mapping.last())
						.copied()
						.ok_or_else(|| {
							fontoxide_ttlib::Error::ttlib("IndexError: list index out of range")
						})?,
					None => axis_index as u64,
				};
				let delta = instancer.get(var_idx)?;
				if delta != 0.0 {
					let v = location.iter().find(|(k, _)| k == tag).map_or(0.0, |e| e.1);
					output.push((tag.clone(), v + delta / f64::from(1 << 14)));
				}
			}
			mappings.push((location, output));
		}
		// Remove base master we added, if it maps to the default location.
		if mappings.first().is_some_and(|m| m.1.is_empty()) {
			mappings.remove(0);
		}
	}

	if denormalize_values {
		let triple = |tag: &str| -> Result<(f64, f64, f64)> {
			axes.iter()
				.find(|(t, _)| t == tag)
				.map(|e| e.1)
				.ok_or_else(|| {
					fontoxide_ttlib::Error::value(format!("Unknown axis tag {tag}")).into()
				})
		};
		for (tag, seg) in &mut axis_maps {
			let tr = triple(tag)?;
			*seg = seg
				.iter()
				.map(|(k, v)| (denormalize(*k, tr), denormalize(*v, tr)))
				.collect();
		}
		for (input, output) in &mut mappings {
			for (t, v) in input.iter_mut().chain(output.iter_mut()) {
				*v = denormalize(*v, triple(t)?);
			}
		}
	}
	Ok((axis_maps, mappings))
}

/// `int(v) if v == int(v) else v`, as printed.
fn number(v: f64) -> String {
	if v.fract() == 0.0 && v.abs() < 1e18 {
		(v as i64).to_string()
	} else {
		float_repr(v)
	}
}

/// `unbuild`: the `avar` table as a designspace snippet.
pub fn unbuild(font: &mut Font) -> Result<String> {
	let fvar = font.require::<Fvar>()?.clone();
	let (segments, mappings) = mappings_from_avar(font, true)?;
	// Axis names come from the font's (untrusted) 'name' table, so escape them;
	// likewise the axis tags.
	let mut axis_names: Vec<(String, String)> = Vec::new();
	let name = font.get::<Name>()?.cloned();
	for axis in &fvar.axes {
		let tag = Tag::new(&axis.axis_tag).to_string();
		let n = match &name {
			Some(name) => name
				.get_debug_name(axis.axis_name_id)
				.unwrap_or_else(|| tag.clone()),
			None => tag.clone(),
		};
		axis_names.push((tag, escape_attr(&n).into_owned()));
	}
	let axis_name = |t: &str| {
		axis_names
			.iter()
			.find(|(k, _)| k == t)
			.map_or("", |e| e.1.as_str())
	};

	let mut f = String::new();
	let _ = writeln!(f, "<?xml version='1.0' encoding='UTF-8'?>");
	let _ = writeln!(f, "<designspace format=\"5.1\">");
	let _ = writeln!(f, "  <axes>");
	for axis in &fvar.axes {
		let tag = Tag::new(&axis.axis_tag).to_string();
		let axis_map = segments.iter().find(|(t, _)| *t == tag).map(|e| &e.1);
		let closing = if axis_map.is_none() { "/>" } else { ">" };
		let _ = writeln!(
			f,
			"    <axis tag=\"{}\" name=\"{}\" minimum=\"{}\" maximum=\"{}\" default=\"{}\"{closing}",
			escape_attr(&tag),
			axis_name(&tag),
			number(axis.min_value),
			number(axis.max_value),
			number(axis.default_value)
		);
		if let Some(map) = axis_map {
			let mut items = map.clone();
			items.sort_by(|a, b| a.0.total_cmp(&b.0));
			for (k, v) in items {
				let _ = writeln!(
					f,
					"      <map input=\"{}\" output=\"{}\"/>",
					number(k),
					number(v)
				);
			}
			let _ = writeln!(f, "    </axis>");
		}
	}
	if !mappings.is_empty() {
		let _ = writeln!(f, "    <mappings>");
		for (input, output) in &mappings {
			let _ = writeln!(f, "      <mapping>");
			for (label, loc) in [("input", input), ("output", output)] {
				let _ = writeln!(f, "        <{label}>");
				let mut items = loc.clone();
				items.sort_by(|a, b| a.0.cmp(&b.0));
				for (t, v) in items {
					let _ = writeln!(
						f,
						"          <dimension name=\"{}\" xvalue=\"{}\"/>",
						axis_name(&t),
						number(v)
					);
				}
				let _ = writeln!(f, "        </{label}>");
			}
			let _ = writeln!(f, "      </mapping>");
		}
		let _ = writeln!(f, "    </mappings>");
	}
	let _ = writeln!(f, "  </axes>");
	let _ = writeln!(f, "</designspace>");
	Ok(f)
}
