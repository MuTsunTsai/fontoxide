//! Port of `fontTools.varLib.hvar`: adds `HVAR` or `VVAR` to a variable font from
//! the phantom points of its `gvar`.

use fontoxide_ttlib::tables::tuple_variation::Delta;
use fontoxide_ttlib::tables::{Fvar, Gvar};
use fontoxide_ttlib::{Font, Tag};

use crate::build::{DeltasAndSupports, MetricsFields, add_vhvar};
use crate::errors::Result;

/// `_get_advance_metrics`: the deltas of the advances from the phantom points of
/// the glyph variations, and their supports.
fn get_advance_metrics(font: &mut Font, fields: MetricsFields) -> Result<Vec<DeltasAndSupports>> {
	let order = font.glyph_order()?;
	let gvar_tag = if font.contains(Tag::new(b"GVAR")) {
		Tag::new(b"GVAR")
	} else {
		Tag::new(b"gvar")
	};
	let gvar = font
		.get_at::<Gvar>(gvar_tag)?
		.ok_or_else(|| fontoxide_ttlib::Error::ttlib("KeyError: 'gvar'"))?;
	let phantom_index = match fields {
		MetricsFields::Hvar => 0,
		MetricsFields::Vvar => 1,
	};
	let mut out = Vec::with_capacity(order.len());
	for gid in 0..order.len() as u32 {
		let mut supports = Vec::new();
		let mut deltas = Vec::new();
		for tv in gvar.get(gid).unwrap_or_default() {
			supports.push(tv.axes.iter().map(|(k, v)| (k.clone(), *v)).collect());
			let n = tv.coordinates.len();
			if n < 4 {
				return Err(fontoxide_ttlib::Error::Assertion("len(phantoms) == 2".into()).into());
			}
			let phantoms = &tv.coordinates[n - 4..];
			let value = |d: &Option<Delta>| match d {
				Some(Delta::Point(x, y)) => {
					if phantom_index == 0 {
						x.value()
					} else {
						y.value()
					}
				}
				Some(Delta::Value(v)) => v.value(),
				None => 0.0,
			};
			let first = value(&phantoms[phantom_index * 2]);
			let second = value(&phantoms[phantom_index * 2 + 1]);
			deltas.push(second - first);
		}
		out.push((deltas, supports, Vec::new()));
	}
	Ok(out)
}

fn add(font: &mut Font, fields: MetricsFields, tag: &[u8; 4]) -> Result<()> {
	font.remove(Tag::new(tag));
	let axis_tags: Vec<String> = font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| Tag::new(&a.axis_tag).to_string())
		.collect();
	let advances = get_advance_metrics(font, fields)?;
	// Each glyph has a supports list of its own, so `allEqual(id(...))` holds only
	// for a single glyph.
	// LENIENT(L36): fontTools fails with an IndexError on a glyph without variations
	// (`setSupports([])`); its deltas are stored here.
	let single = advances.len() <= 1;
	add_vhvar(font, &axis_tags, fields, &advances, None, Some(single))
}

/// `add_HVAR`: replaces the `HVAR` of a variable font with one built from the
/// `gvar` phantom points.
pub fn add_hvar(font: &mut Font) -> Result<()> {
	add(font, MetricsFields::Hvar, b"HVAR")
}

/// `add_VVAR`: replaces the `VVAR` of a variable font with one built from the
/// `gvar` phantom points.
pub fn add_vvar(font: &mut Font) -> Result<()> {
	add(font, MetricsFields::Vvar, b"VVAR")
}
