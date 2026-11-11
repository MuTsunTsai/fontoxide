//! `Tests/otlLib/optimize_test.py`.

use fontoxide_misc::Tag;
use fontoxide_otllib::builder::{LookupBuilder, build_value};
use fontoxide_otllib::{GlyphMap, Result};
use fontoxide_ttlib::Font;
use fontoxide_ttlib::ot::WriterGraph;
use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types as ot;

/// `get_kerning_by_blocks`: a highly compressible font with rectangular blocks
/// of kerning on the diagonal, which can easily be sliced into subtables. Each
/// block is like a script in a multi-script font. Returns the glyphs and the
/// rules: `pos @left @right value;` with a class for each glyph.
fn kerning_by_blocks(blocks: &[(usize, usize)]) -> (Vec<String>, Vec<(String, String, i16)>) {
	let mut value = 0;
	let mut glyphs = Vec::new();
	let mut rules = Vec::new();
	for (script, &(width, height)) in blocks.iter().enumerate() {
		glyphs.extend((0..width.max(height)).map(|i| format!("g_{script}_{i}")));
		for l in 0..height {
			for r in 0..width {
				value += 1;
				rules.push((format!("g_{script}_{l}"), format!("g_{script}_{r}"), value));
			}
		}
	}
	(glyphs, rules)
}

/// The `kern` lookup that feaLib builds for the rules: class pairs whose first
/// value is the advance.
fn build_kern(font: &mut Font, rules: &[(String, String, i16)]) -> Result<ot::Lookup> {
	let mut builder = LookupBuilder::pair_pos(GlyphMap::from_font(font)?, None);
	if let LookupBuilder::PairPos(b) = &mut builder {
		for (l, r, v) in rules {
			b.add_class_pair(
				&None,
				vec![l.clone()],
				Some(build_value(&[("XAdvance", *v)])?),
				vec![r.clone()],
				None,
			);
		}
	}
	Ok(builder.build(font)?.expect("lookup"))
}

/// `count_pairpos_bytes` for one lookup.
fn compiled_bytes(lookup: &ot::Lookup, font: &mut Font) -> Result<usize> {
	let mut graph = WriterGraph::new(Tag::new(b"GPOS"));
	lookup.compile(&mut graph.root(), font)?;
	Ok(graph.get_all_data()?.len())
}

/// Blocks, level, subtables and bytes.
type Case<'a> = (&'a [(usize, usize)], Option<u8>, usize, usize);

/// `test_optimization_mode`: the optimizations are off by default, and higher
/// levels create more subtables and fewer bytes.
#[test]
fn test_optimization_mode() -> Result<()> {
	let diagonal20: Vec<(usize, usize)> = vec![(4, 4); 20];
	let cases: [Case<'_>; 5] = [
		// Level 0, no optimization.
		(&[(15, 3), (2, 10)], None, 1, 602),
		// Level 1 recognizes the 2 blocks and splits into 2 subtables: one more
		// subtable for a size reduction of (602-298)/602 = 50%.
		(&[(15, 3), (2, 10)], Some(1), 2, 298),
		// On a bigger configuration, level 5 doesn't create as many subtables as
		// it could, because of the stop criteria,
		(&diagonal20, Some(5), 14, 2042),
		// while level 9 creates as many subtables as there were blocks on the
		// diagonal and yields a better saving.
		(&diagonal20, Some(9), 20, 1886),
		// On a fully occupied kerning matrix, even level 9 doesn't split anything.
		(&[(10, 10)], Some(9), 1, 304),
	];
	for (blocks, level, expected_subtables, expected_bytes) in cases {
		let (glyphs, rules) = kerning_by_blocks(blocks);
		let mut order = vec![".notdef space".to_owned()];
		order.extend(glyphs);
		let mut font = Font::new();
		if let Some(level) = level {
			font.set_gpos_compression_level(level)?;
		}
		font.set_glyph_order(order);
		let lookup = build_kern(&mut font, &rules)?;
		assert_eq!(
			lookup.sub_table.len(),
			expected_subtables,
			"{blocks:?} {level:?}"
		);
		assert_eq!(
			compiled_bytes(&lookup, &mut font)?,
			expected_bytes,
			"{blocks:?} {level:?}"
		);
	}
	Ok(())
}
