//! `fontTools.otlLib.maxContextCalc`: the `OS/2.usMaxContext` value of a font.

use fontoxide_ttlib::Font;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::types::LookupSubTable as St;
use fontoxide_ttlib::tables::{Gpos, Gsub};

use crate::Result;

fn attribute_error(attr: &str) -> crate::Error {
	fontoxide_ttlib::Error::ttlib(format!(
		"AttributeError: 'NoneType' object has no attribute '{attr}'"
	))
	.into()
}

fn count(value: Option<u32>, attr: &str) -> Result<u32> {
	value.ok_or_else(|| attribute_error(attr))
}

/// The kind of table a subtable belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
	Gsub,
	Gpos,
}

/// The `usMaxContext` value of a font (`maxCtxFont`): the longest context of
/// its GSUB and GPOS rules.
pub fn max_ctx_font(font: &mut Font) -> Result<u32> {
	let mut max_ctx = 0;
	if let Some(table) = font.get::<Gsub>()? {
		// `_getLookupList`: `LookupList2` takes precedence.
		if let Some(list) = &table.lookup_list2 {
			max_ctx = lookups(max_ctx, Tag::Gsub, &list.lookup)?;
		} else if let Some(list) = &table.lookup_list {
			max_ctx = lookups(max_ctx, Tag::Gsub, &list.lookup)?;
		}
	}
	if let Some(table) = font.get::<Gpos>()? {
		if let Some(list) = &table.lookup_list2 {
			max_ctx = lookups(max_ctx, Tag::Gpos, &list.lookup)?;
		} else if let Some(list) = &table.lookup_list {
			max_ctx = lookups(max_ctx, Tag::Gpos, &list.lookup)?;
		}
	}
	Ok(max_ctx)
}

fn lookups(mut max_ctx: u32, tag: Tag, lookups: &[Option<Box<ot::Lookup>>]) -> Result<u32> {
	for lookup in lookups {
		let lookup = lookup
			.as_deref()
			.ok_or_else(|| attribute_error("SubTable"))?;
		for st in &lookup.sub_table {
			max_ctx = subtable(max_ctx, tag, lookup.lookup_type, st.as_deref())?;
		}
	}
	Ok(max_ctx)
}

/// `maxCtxSubtable`.
fn subtable(max_ctx: u32, tag: Tag, lookup_type: u16, st: Option<&St>) -> Result<u32> {
	Ok(match (tag, lookup_type) {
		// Single positioning, single / multiple substitution.
		(Tag::Gpos, 1) | (Tag::Gsub, 1..=3) => max_ctx.max(1),
		// Pair positioning.
		(Tag::Gpos, 2) => max_ctx.max(2),
		// Ligatures.
		(Tag::Gsub, 4) => {
			let Some(St::LigatureSubst(st)) = st else {
				return Err(attribute_error("ligatures"));
			};
			let mut max_ctx = max_ctx;
			for ligatures in st.ligatures.values() {
				for ligature in ligatures {
					max_ctx = max_ctx.max(count(ligature.comp_count, "CompCount")?);
				}
			}
			max_ctx
		}
		// Context and chained context.
		(Tag::Gpos, 7 | 8) | (Tag::Gsub, 5 | 6) => match st {
			Some(St::ContextSubst(st)) => context_subst(max_ctx, st)?,
			Some(St::ChainContextSubst(st)) => chain_context_subst(max_ctx, st)?,
			Some(St::ContextPos(st)) => context_pos(max_ctx, st)?,
			Some(St::ChainContextPos(st)) => chain_context_pos(max_ctx, st)?,
			_ => return Err(attribute_error("Format")),
		},
		// Extensions.
		(Tag::Gsub, 7) => {
			let Some(St::ExtensionSubst(ot::ExtensionSubst::Format1(st))) = st else {
				return Err(attribute_error("ExtensionLookupType"));
			};
			subtable(
				max_ctx,
				tag,
				st.extension_lookup_type,
				st.ext_sub_table.as_deref(),
			)?
		}
		(Tag::Gpos, 9) => {
			let Some(St::ExtensionPos(ot::ExtensionPos::Format1(st))) = st else {
				return Err(attribute_error("ExtensionLookupType"));
			};
			subtable(
				max_ctx,
				tag,
				st.extension_lookup_type,
				st.ext_sub_table.as_deref(),
			)?
		}
		// Reverse-chained context.
		(Tag::Gsub, 8) => {
			let look_ahead = match st {
				Some(St::ReverseChainSingleSubst(ot::ReverseChainSingleSubst::Format1(st))) => {
					st.look_ahead_glyph_count
				}
				Some(St::ReverseChainSingleSubst(ot::ReverseChainSingleSubst::Format2(st))) => {
					st.look_ahead_glyph_count
				}
				_ => None,
			};
			max_ctx.max(1 + count(look_ahead, "LookAheadGlyphCount")?)
		}
		_ => max_ctx,
	})
}

/// `maxCtxContextualRule` for rules that are not chained.
fn plain(max_ctx: u32, glyph_count: Option<u32>) -> Result<u32> {
	Ok(max_ctx.max(count(glyph_count, "GlyphCount")?))
}

/// `maxCtxContextualRule` for chained rules.
fn chained(max_ctx: u32, input: Option<u32>, look_ahead: Option<u32>) -> Result<u32> {
	Ok(max_ctx.max(count(input, "InputGlyphCount")? + count(look_ahead, "LookAheadGlyphCount")?))
}

/// The loops of `maxCtxContextualSubtable` over rule sets; `None` entries are
/// skipped.
macro_rules! rules {
	($max:ident, $sets:expr, $rules:ident, |$r:ident| $value:expr) => {
		for set in $sets.iter().flatten() {
			for $r in set.$rules.iter().flatten() {
				$max = $value?;
			}
		}
	};
}

fn context_subst(mut max: u32, st: &ot::ContextSubst) -> Result<u32> {
	use ot::ContextSubst as F;
	match st {
		F::Format1(t) => rules!(max, t.sub_rule_set, sub_rule, |r| plain(max, r.glyph_count)),
		F::Format2(t) => {
			rules!(max, t.sub_class_set, sub_class_rule, |r| plain(
				max,
				r.glyph_count
			))
		}
		F::Format3(t) => max = plain(max, t.glyph_count)?,
		F::Format4(t) => rules!(max, t.seq_rule_set, seq_rule, |r| plain(max, r.glyph_count)),
		F::Format5(t) => {
			rules!(max, t.class_seq_rule_set, class_seq_rule, |r| plain(
				max,
				r.glyph_count
			))
		}
		F::Format6(t) => max = plain(max, t.glyph_count)?,
		F::Unknown(_) => {}
	}
	Ok(max)
}

fn context_pos(mut max: u32, st: &ot::ContextPos) -> Result<u32> {
	use ot::ContextPos as F;
	match st {
		F::Format1(t) => rules!(max, t.pos_rule_set, pos_rule, |r| plain(max, r.glyph_count)),
		F::Format2(t) => {
			rules!(max, t.pos_class_set, pos_class_rule, |r| plain(
				max,
				r.glyph_count
			))
		}
		F::Format3(t) => max = plain(max, t.glyph_count)?,
		F::Format4(t) => rules!(max, t.seq_rule_set, seq_rule, |r| plain(max, r.glyph_count)),
		F::Format5(t) => {
			rules!(max, t.class_seq_rule_set, class_seq_rule, |r| plain(
				max,
				r.glyph_count
			))
		}
		F::Format6(t) => max = plain(max, t.glyph_count)?,
		F::Unknown(_) => {}
	}
	Ok(max)
}

fn chain_context_subst(mut max: u32, st: &ot::ChainContextSubst) -> Result<u32> {
	use ot::ChainContextSubst as F;
	match st {
		F::Format1(t) => rules!(max, t.chain_sub_rule_set, chain_sub_rule, |r| chained(
			max,
			r.input_glyph_count,
			r.look_ahead_glyph_count
		)),
		F::Format2(t) => rules!(max, t.chain_sub_class_set, chain_sub_class_rule, |r| {
			chained(max, r.input_glyph_count, r.look_ahead_glyph_count)
		}),
		F::Format3(t) => max = chained(max, t.input_glyph_count, t.look_ahead_glyph_count)?,
		F::Format4(t) => rules!(max, t.chained_seq_rule_set, chained_seq_rule, |r| chained(
			max,
			r.input_glyph_count,
			r.look_ahead_glyph_count
		)),
		F::Format5(t) => rules!(
			max,
			t.chained_class_seq_rule_set,
			chained_class_seq_rule,
			|r| chained(max, r.input_glyph_count, r.look_ahead_glyph_count)
		),
		F::Unknown(_) => {}
	}
	Ok(max)
}

fn chain_context_pos(mut max: u32, st: &ot::ChainContextPos) -> Result<u32> {
	use ot::ChainContextPos as F;
	match st {
		F::Format1(t) => rules!(max, t.chain_pos_rule_set, chain_pos_rule, |r| chained(
			max,
			r.input_glyph_count,
			r.look_ahead_glyph_count
		)),
		F::Format2(t) => rules!(max, t.chain_pos_class_set, chain_pos_class_rule, |r| {
			chained(max, r.input_glyph_count, r.look_ahead_glyph_count)
		}),
		F::Format3(t) => max = chained(max, t.input_glyph_count, t.look_ahead_glyph_count)?,
		F::Format4(t) => rules!(max, t.chained_seq_rule_set, chained_seq_rule, |r| chained(
			max,
			r.input_glyph_count,
			r.look_ahead_glyph_count
		)),
		F::Format5(t) => rules!(
			max,
			t.chained_class_seq_rule_set,
			chained_class_seq_rule,
			|r| chained(max, r.input_glyph_count, r.look_ahead_glyph_count)
		),
		F::Unknown(_) => {}
	}
	Ok(max)
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use super::*;

	fn import(path: PathBuf) -> Result<Font> {
		let text = std::fs::read_to_string(path).map_err(fontoxide_ttlib::Error::from)?;
		let mut font = Font::new();
		font.import_ttx(&text)?;
		Ok(font)
	}

	fn manifest_dir() -> PathBuf {
		PathBuf::from(env!("CARGO_MANIFEST_DIR"))
	}

	#[test]
	fn test_max_ctx_calc_no_features() -> Result<()> {
		assert_eq!(max_ctx_font(&mut Font::new())?, 0);
		let mut font = Font::new();
		font.set_glyph_order(vec![".notdef".to_owned()]);
		assert_eq!(max_ctx_font(&mut font)?, 0);
		Ok(())
	}

	/// The features of the test are compiled by `tools/reference/subset_fixtures.py`.
	/// fontTools' test looks at the tables as `feaLib` builds them; what they have
	/// in common with these is how they are read from binary. From TTX, fontTools
	/// sets `CompCount` one too low, so the value is smaller.
	#[test]
	fn test_max_ctx_calc_features() -> Result<()> {
		let path = manifest_dir().join("tests/data/max_context.ttx");
		let mut font = import(path)?;
		assert_eq!(max_ctx_font(&mut font)?, 2);
		let data = font.write(&fontoxide_ttlib::WriteOptions::default())?;
		let mut font = Font::read(data, fontoxide_ttlib::ReadOptions::default())?;
		assert_eq!(max_ctx_font(&mut font)?, 3);
		Ok(())
	}

	/// `Tests/ttLib/beyond64k_test.py::test_max_context_extended_layout`: the tables that
	/// the test adds to `TestTTF-Regular.ttx` are made by
	/// `tools/reference/beyond64k_fixtures.py`.
	#[test]
	fn test_max_context_extended_layout() -> Result<()> {
		use fontoxide_ttlib::beyond64k::{ConvertOptions, convert_layout_formats, upper_tables};

		for extended_header in [false, true] {
			let mut font = import(
				manifest_dir()
					.join("../../upstream/fonttools/Tests/ttLib/data/TestTTF-Regular.ttx"),
			)?;
			font.import_ttx(
				&std::fs::read_to_string(
					manifest_dir().join("tests/data/beyond64k_contextual_layout.ttx"),
				)
				.map_err(fontoxide_ttlib::Error::from)?,
			)?;
			assert_eq!(max_ctx_font(&mut font)?, 3);
			if extended_header {
				upper_tables(
					&mut font,
					&ConvertOptions {
						tables: Some(&["GSUB", "GPOS"]),
						..Default::default()
					},
				)?;
			} else {
				convert_layout_formats(&mut font.require::<Gsub>()?.table, true)?;
				convert_layout_formats(&mut font.require::<Gpos>()?.table, true)?;
			}
			assert_eq!(max_ctx_font(&mut font)?, 3);
		}
		Ok(())
	}

	#[test]
	fn test_max_ctx_calc_features_ttx() -> Result<()> {
		let data = manifest_dir().join("../../upstream/fonttools/Tests/otlLib/data");
		for (name, expected) in [
			("gsub_51", 2),
			("gsub_52", 2),
			("gsub_71", 1),
			("gpos_91", 1),
		] {
			let mut font = import(data.join(format!("{name}.ttx")))?;
			assert_eq!(max_ctx_font(&mut font)?, expected, "{name}");
		}
		Ok(())
	}
}
