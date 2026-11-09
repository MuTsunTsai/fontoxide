//! The `EBLC`, `EBDT`, `CBLC`, `CBDT` and `sbix` parts of `fontTools.subset`.

use fontoxide_ttlib::Font;
use fontoxide_ttlib::tables::{BitmapData, BitmapLocator, Sbix};

use crate::{Result, Subsetter};

/// `EBLC.subset_glyphs`, which `CBLC` inherits.
///
/// The locations are not touched: compiling the bitmap data table sets them.
pub(crate) fn locator_subset_glyphs<const COLOR: bool>(
	s: &Subsetter,
	font: &mut Font,
) -> Result<bool> {
	let table = font.require::<BitmapLocator<COLOR>>()?;
	for strike in table.strikes.iter_mut().flatten() {
		for sub_table in &mut strike.index_sub_tables {
			sub_table.names.retain(|n| s.glyphs.contains(n));
		}
		strike.index_sub_tables.retain(|t| !t.names.is_empty());
	}
	table.strikes.retain(|strike| {
		strike
			.as_ref()
			.is_none_or(|s| !s.index_sub_tables.is_empty())
	});
	Ok(true)
}

/// `EBDT.subset_glyphs`, which `CBDT` inherits.
pub(crate) fn data_subset_glyphs<const COLOR: bool>(
	s: &Subsetter,
	font: &mut Font,
) -> Result<bool> {
	let table = font.require::<BitmapData<COLOR>>()?;
	for strike in table.strike_data.iter_mut().flatten() {
		strike.retain(&s.glyphs);
	}
	// Prune empty strikes: https://github.com/fonttools/fonttools/issues/1633
	table
		.strike_data
		.retain(|strike| strike.as_ref().is_none_or(|s| !s.is_empty()));
	Ok(true)
}

/// `sbix.subset_glyphs`. Strikes that become empty are kept.
pub(crate) fn sbix_subset_glyphs(s: &Subsetter, font: &mut Font) -> Result<bool> {
	for strike in font.require::<Sbix>()?.strikes.values_mut() {
		strike.glyphs.retain(|g, _| s.glyphs.contains(g));
	}
	Ok(true)
}
