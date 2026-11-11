//! Port of `fontTools.varLib.mvar`: the font-wide metrics that `MVAR` varies.

use fontoxide_ttlib::tables::{Hhea, Os2, Post, Vhea};
use fontoxide_ttlib::{Font, Result, Tag};

/// `MVAR_ENTRIES`: the value tags of `MVAR` and the table fields they vary.
pub const MVAR_ENTRIES: [(&str, (&str, &str)); 28] = [
	("hasc", ("OS/2", "sTypoAscender")),       // horizontal ascender
	("hdsc", ("OS/2", "sTypoDescender")),      // horizontal descender
	("hlgp", ("OS/2", "sTypoLineGap")),        // horizontal line gap
	("hcla", ("OS/2", "usWinAscent")),         // horizontal clipping ascent
	("hcld", ("OS/2", "usWinDescent")),        // horizontal clipping descent
	("vasc", ("vhea", "ascent")),              // vertical ascender
	("vdsc", ("vhea", "descent")),             // vertical descender
	("vlgp", ("vhea", "lineGap")),             // vertical line gap
	("hcrs", ("hhea", "caretSlopeRise")),      // horizontal caret rise
	("hcrn", ("hhea", "caretSlopeRun")),       // horizontal caret run
	("hcof", ("hhea", "caretOffset")),         // horizontal caret offset
	("vcrs", ("vhea", "caretSlopeRise")),      // vertical caret rise
	("vcrn", ("vhea", "caretSlopeRun")),       // vertical caret run
	("vcof", ("vhea", "caretOffset")),         // vertical caret offset
	("xhgt", ("OS/2", "sxHeight")),            // x height
	("cpht", ("OS/2", "sCapHeight")),          // cap height
	("sbxs", ("OS/2", "ySubscriptXSize")),     // subscript em x size
	("sbys", ("OS/2", "ySubscriptYSize")),     // subscript em y size
	("sbxo", ("OS/2", "ySubscriptXOffset")),   // subscript em x offset
	("sbyo", ("OS/2", "ySubscriptYOffset")),   // subscript em y offset
	("spxs", ("OS/2", "ySuperscriptXSize")),   // superscript em x size
	("spys", ("OS/2", "ySuperscriptYSize")),   // superscript em y size
	("spxo", ("OS/2", "ySuperscriptXOffset")), // superscript em x offset
	("spyo", ("OS/2", "ySuperscriptYOffset")), // superscript em y offset
	("strs", ("OS/2", "yStrikeoutSize")),      // strikeout size
	("stro", ("OS/2", "yStrikeoutPosition")),  // strikeout offset
	("unds", ("post", "underlineThickness")),  // underline size
	("undo", ("post", "underlinePosition")),   // underline offset
];

/// `MVAR_COMPANION_TABLES`.
const MVAR_COMPANION_TABLES: [(&str, &str); 2] = [("hhea", "HHEA"), ("vhea", "VHEA")];

/// `getMVARTableTag`: the beyond-64k companion of a table if the font has it.
pub fn get_mvar_table_tag(font: &Font, table_tag: &str) -> String {
	if let Some((_, companion)) = MVAR_COMPANION_TABLES.iter().find(|(t, _)| *t == table_tag)
		&& Tag::from_str_padded(companion).is_some_and(|t| font.contains(t))
	{
		return (*companion).to_owned();
	}
	table_tag.to_owned()
}

fn tag(s: &str) -> Tag {
	Tag::from_str_padded(s).unwrap_or(Tag::new(b"    "))
}

/// `getattr(font[table_tag], item_name)` for the fields of [`MVAR_ENTRIES`];
/// `None` if the font has no such table.
pub(crate) fn get_value(font: &mut Font, table_tag: &str, item: &str) -> Result<Option<i64>> {
	let t = tag(table_tag);
	if !font.contains(t) {
		return Ok(None);
	}
	Ok(match table_tag {
		"OS/2" => {
			let os2 = font.require::<Os2>()?;
			Some(match item {
				"sTypoAscender" => os2.base.s_typo_ascender.into(),
				"sTypoDescender" => os2.base.s_typo_descender.into(),
				"sTypoLineGap" => os2.base.s_typo_line_gap.into(),
				"usWinAscent" => os2.base.us_win_ascent.into(),
				"usWinDescent" => os2.base.us_win_descent.into(),
				"sxHeight" => os2.sx_height.into(),
				"sCapHeight" => os2.s_cap_height.into(),
				"ySubscriptXSize" => os2.base.y_subscript_x_size.into(),
				"ySubscriptYSize" => os2.base.y_subscript_y_size.into(),
				"ySubscriptXOffset" => os2.base.y_subscript_x_offset.into(),
				"ySubscriptYOffset" => os2.base.y_subscript_y_offset.into(),
				"ySuperscriptXSize" => os2.base.y_superscript_x_size.into(),
				"ySuperscriptYSize" => os2.base.y_superscript_y_size.into(),
				"ySuperscriptXOffset" => os2.base.y_superscript_x_offset.into(),
				"ySuperscriptYOffset" => os2.base.y_superscript_y_offset.into(),
				"yStrikeoutSize" => os2.base.y_strikeout_size.into(),
				"yStrikeoutPosition" => os2.base.y_strikeout_position.into(),
				_ => return Ok(None),
			})
		}
		"post" => {
			let post = font.require::<Post>()?;
			Some(match item {
				"underlineThickness" => post.header.underline_thickness.into(),
				"underlinePosition" => post.header.underline_position.into(),
				_ => return Ok(None),
			})
		}
		"hhea" | "HHEA" => {
			let hhea = font.get_at::<Hhea>(t)?.ok_or_else(|| missing(table_tag))?;
			Some(match item {
				"caretSlopeRise" => hhea.caret_slope_rise.into(),
				"caretSlopeRun" => hhea.caret_slope_run.into(),
				"caretOffset" => hhea.caret_offset.into(),
				_ => return Ok(None),
			})
		}
		"vhea" | "VHEA" => {
			let vhea = font.get_at::<Vhea>(t)?.ok_or_else(|| missing(table_tag))?;
			Some(match item {
				"ascent" => vhea.ascent.into(),
				"descent" => vhea.descent.into(),
				"lineGap" => vhea.line_gap.into(),
				"caretSlopeRise" => vhea.caret_slope_rise.into(),
				"caretSlopeRun" => vhea.caret_slope_run.into(),
				"caretOffset" => vhea.caret_offset.into(),
				_ => return Ok(None),
			})
		}
		_ => None,
	})
}

fn missing(table_tag: &str) -> fontoxide_ttlib::Error {
	fontoxide_ttlib::Error::ttlib(format!("KeyError: {table_tag:?}"))
}

/// `setattr(font[table_tag], item_name, value)` for the fields of
/// [`MVAR_ENTRIES`].
pub(crate) fn set_value(font: &mut Font, table_tag: &str, item: &str, value: i64) -> Result<()> {
	let t = tag(table_tag);
	let i16v = || -> Result<i16> { fontoxide_ttlib::binary::fit(value, item) };
	let u16v = || -> Result<u16> { fontoxide_ttlib::binary::fit(value, item) };
	match table_tag {
		"OS/2" => {
			let os2 = font.require::<Os2>()?;
			match item {
				"sTypoAscender" => os2.base.s_typo_ascender = i16v()?,
				"sTypoDescender" => os2.base.s_typo_descender = i16v()?,
				"sTypoLineGap" => os2.base.s_typo_line_gap = i16v()?,
				"usWinAscent" => os2.base.us_win_ascent = u16v()?,
				"usWinDescent" => os2.base.us_win_descent = u16v()?,
				"sxHeight" => os2.sx_height = i16v()?,
				"sCapHeight" => os2.s_cap_height = i16v()?,
				"ySubscriptXSize" => os2.base.y_subscript_x_size = i16v()?,
				"ySubscriptYSize" => os2.base.y_subscript_y_size = i16v()?,
				"ySubscriptXOffset" => os2.base.y_subscript_x_offset = i16v()?,
				"ySubscriptYOffset" => os2.base.y_subscript_y_offset = i16v()?,
				"ySuperscriptXSize" => os2.base.y_superscript_x_size = i16v()?,
				"ySuperscriptYSize" => os2.base.y_superscript_y_size = i16v()?,
				"ySuperscriptXOffset" => os2.base.y_superscript_x_offset = i16v()?,
				"ySuperscriptYOffset" => os2.base.y_superscript_y_offset = i16v()?,
				"yStrikeoutSize" => os2.base.y_strikeout_size = i16v()?,
				"yStrikeoutPosition" => os2.base.y_strikeout_position = i16v()?,
				_ => {}
			}
		}
		"post" => {
			let post = font.require::<Post>()?;
			match item {
				"underlineThickness" => post.header.underline_thickness = i16v()?,
				"underlinePosition" => post.header.underline_position = i16v()?,
				_ => {}
			}
		}
		"hhea" | "HHEA" => {
			let hhea = font.get_at::<Hhea>(t)?.ok_or_else(|| missing(table_tag))?;
			match item {
				"caretSlopeRise" => hhea.caret_slope_rise = i16v()?,
				"caretSlopeRun" => hhea.caret_slope_run = i16v()?,
				"caretOffset" => hhea.caret_offset = i16v()?,
				_ => {}
			}
		}
		"vhea" | "VHEA" => {
			let vhea = font.get_at::<Vhea>(t)?.ok_or_else(|| missing(table_tag))?;
			match item {
				"ascent" => vhea.ascent = i16v()?,
				"descent" => vhea.descent = i16v()?,
				"lineGap" => vhea.line_gap = i16v()?,
				"caretSlopeRise" => vhea.caret_slope_rise = i16v()?,
				"caretSlopeRun" => vhea.caret_slope_run = i16v()?,
				"caretOffset" => vhea.caret_offset = i16v()?,
				_ => {}
			}
		}
		_ => {}
	}
	Ok(())
}
