//! Port of `fontTools.varLib.interpolate_layout`: interpolates the OpenType Layout
//! tables of the masters of a designspace at a location.

use fontoxide_designspacelib::{DesignSpaceDocument, Number};
use fontoxide_ttlib::Font;

use crate::build::{MasterFinder, SourceFonts, load_designspace, load_masters};
use crate::errors::{Result, VarLibError};
use crate::merger::{MasterInfo, Merger};
use crate::models::{VariationModel, normalize_value_decreasing};

/// `interpolate_layout`: a copy of the default master with its `GPOS`
/// interpolated at `loc`, by axis name. Unless `mapped`, the location is in
/// user space and mapped with the `map` of the axes; otherwise it is in the
/// designspace's internal space.
pub fn interpolate_layout(
	designspace: &mut DesignSpaceDocument,
	fonts: &mut SourceFonts,
	loc: &[(String, f64)],
	master_finder: MasterFinder<'_>,
	mapped: bool,
) -> Result<Font> {
	let ds = load_designspace(designspace, true, true)?;
	log::info!("Building interpolated font");

	log::info!("Loading master fonts");
	let paths = load_masters(designspace, fonts, master_finder)?;
	let mut master_fonts: Vec<Font> = fonts
		.iter()
		.take(designspace.sources.len())
		.map(|f| {
			f.clone()
				.ok_or_else(|| VarLibError::Validation("master font not loaded".into()))
		})
		.collect::<Result<_>>()?;
	let base_idx = ds
		.base_idx
		.ok_or_else(|| VarLibError::Validation("Base master not found".into()))?;
	let mut font = master_fonts[base_idx].clone();

	log::info!("Location: {loc:?}");
	let mut loc: Vec<(String, f64)> = loc.to_vec();
	if !mapped {
		loc =
			loc.into_iter()
				.map(|(name, v)| {
					let axis = ds.axes.get(&name).ok_or_else(|| {
						fontoxide_ttlib::Error::ttlib(format!("KeyError: {name:?}"))
					})?;
					Ok((name, axis.map_forward(Number::Float(v))?.to_f64()))
				})
				.collect::<Result<_>>()?;
	}
	log::info!("Internal location: {loc:?}");
	// `models.normalizeLocation(loc, internal_axis_supports, allow_decreasing=True)`.
	let mut normalized = Vec::with_capacity(ds.internal_axis_supports.len());
	for (name, triple) in &ds.internal_axis_supports {
		let v = loc
			.iter()
			.find(|(k, _)| k == name)
			.map_or(triple.1, |e| e.1);
		normalized.push((name.clone(), normalize_value_decreasing(v, *triple, false)?));
	}
	log::info!("Normalized location: {normalized:?}");

	// Assume single-model for now.
	let model = VariationModel::new(&ds.normalized_master_locs, None, false, None)?;
	if model.mapping[base_idx] != 0 {
		return Err(
			fontoxide_ttlib::Error::Assertion("0 == model.mapping[ds.base_idx]".into()).into(),
		);
	}

	let infos: Vec<MasterInfo> = master_fonts
		.iter_mut()
		.enumerate()
		.map(|(i, m)| MasterInfo::of(m, paths.get(i).and_then(|p| p.as_deref())))
		.collect::<Result<_>>()?;
	let mut merger = Merger::instancer(&mut font, &model, &normalized, &infos)?;

	log::info!("Building interpolated tables");
	// TODO GSUB/GDEF
	let mut refs: Vec<&mut Font> = master_fonts.iter_mut().collect();
	merger.merge_tables(&mut font, &mut refs, &["GPOS"])?;
	Ok(font)
}

/// `main`: interpolates the layout tables for a point on a designspace.
pub fn main(args: &[&str]) -> Result<i32> {
	let mut designspace_filename = None;
	let mut locations = Vec::new();
	let mut output: Option<String> = None;
	let mut it = args.iter();
	while let Some(&arg) = it.next() {
		match arg {
			"-o" | "--output" => {
				output = Some(
					it.next()
						.ok_or_else(|| {
							VarLibError::Usage("argument -o/--output: expected one argument".into())
						})?
						.to_string(),
				);
			}
			"-l" | "--loglevel" => {
				it.next();
			}
			a if designspace_filename.is_none() => designspace_filename = Some(a),
			a => locations.push(a),
		}
	}
	let designspace_filename = designspace_filename.ok_or_else(|| {
		VarLibError::Usage("the following arguments are required: DESIGNSPACE, LOCATION".into())
	})?;
	if locations.is_empty() {
		return Err(VarLibError::Usage(
			"the following arguments are required: LOCATION".into(),
		));
	}
	let output = output.unwrap_or_else(|| {
		let stem = match designspace_filename.rfind('.') {
			Some(i) if !designspace_filename[i..].contains(['/', '\\']) => {
				&designspace_filename[..i]
			}
			_ => designspace_filename,
		};
		format!("{stem}-instance.ttf")
	});
	let finder = |s: &str| {
		s.replace("master_ufo", "master_ttf_interpolatable")
			.replace(".ufo", ".ttf")
	};
	let mut loc = Vec::new();
	for arg in locations {
		let (tag, val) = arg.split_once('=').ok_or_else(|| {
			fontoxide_ttlib::Error::value("not enough values to unpack (expected 2, got 1)")
		})?;
		let v = fontoxide_misc::py::parse_float(val).ok_or_else(|| {
			fontoxide_ttlib::Error::value(format!("could not convert string to float: {val:?}"))
		})?;
		loc.push((tag.to_owned(), v));
	}
	let mut ds = DesignSpaceDocument::from_file(designspace_filename)?;
	let mut font = interpolate_layout(&mut ds, &mut SourceFonts::new(), &loc, &finder, false)?;
	log::info!("Saving font {output}");
	std::fs::write(
		&output,
		font.write(&fontoxide_ttlib::WriteOptions::default())?,
	)?;
	Ok(0)
}
