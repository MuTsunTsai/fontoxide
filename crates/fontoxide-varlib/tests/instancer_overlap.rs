//! `InstantiateVariableFontTest::test_overlap` of fontTools'
//! `Tests/varLib/instancer/instancer_test.py`.

#![allow(clippy::type_complexity)]

mod common;

use common::*;
use fontoxide_ttlib::Font;
use fontoxide_ttlib::tables::Glyf;
use fontoxide_varlib::instancer::{InstancerOptions, OverlapMode, instantiate_variable_font};

fn instance(overlap: OverlapMode, wght: f64) -> TestResult<fontoxide_ttlib::Font> {
	let mut font = font_from_ttx(&instancer_data("PartialInstancerTest3-VF.ttx"))?;
	let mut font = reload(&mut font)?;
	instantiate_variable_font(
		&mut font,
		&limits(&[("wght", pin(wght))]),
		&InstancerOptions {
			overlap,
			..Default::default()
		},
	)?;
	// `_dump_ttx`: compiled, to get recalculated values.
	reload(&mut font)
}

#[test]
fn keep_and_dont_set_flags() -> TestResult {
	let mut font = instance(OverlapMode::KeepAndDontSetFlags, 400.0)?;
	expect_ttx(
		&mut font,
		&instancer_data("test_results/PartialInstancerTest3-VF-instance-400-no-overlap-flags.ttx"),
		&[],
	)
}

/// The contours of each glyph, whatever point they start at and whatever their order: skia
/// and linesweeper differ in that, so fontoxide isn't byte-identical here.
fn normalized_glyphs(font: &mut Font) -> TestResult<Vec<Vec<Vec<((f64, f64), u8)>>>> {
	let mut out = Vec::new();
	for glyph in font.require::<Glyf>()?.glyphs.values() {
		let mut contours = Vec::new();
		let mut start = 0;
		for &end in &glyph.end_pts_of_contours {
			let end = end as usize + 1;
			let points: Vec<((f64, f64), u8)> = glyph.coordinates[start..end]
				.iter()
				.zip(&glyph.flags[start..end])
				.map(|(&p, &f)| (p, f & 1))
				.collect();
			start = end;
			let rotation = (0..points.len())
				.min_by(|&a, &b| {
					points[a]
						.0
						.0
						.total_cmp(&points[b].0.0)
						.then(points[a].0.1.total_cmp(&points[b].0.1))
				})
				.unwrap_or(0);
			let mut rotated = points;
			rotated.rotate_left(rotation);
			contours.push(rotated);
		}
		contours.sort_by(|a, b| {
			a[0].0
				.0
				.total_cmp(&b[0].0.0)
				.then(a[0].0.1.total_cmp(&b[0].0.1))
		});
		out.push(contours);
	}
	Ok(out)
}

/// `test_overlap` with `OverlapMode.REMOVE`: everything but the start and the order of
/// the contours is as in the expected TTX.
fn check_removed(wght: f64, expected: &str) -> TestResult {
	let mut font = instance(OverlapMode::Remove, wght)?;
	let path = instancer_data(expected);
	let mut expected = font_from_ttx(&path)?;
	let mut expected = reload(&mut expected)?;
	assert_eq!(
		normalized_glyphs(&mut font)?,
		normalized_glyphs(&mut expected)?
	);
	let other_tables: Vec<String> = font
		.tags()
		.iter()
		.map(|t| t.to_string())
		.filter(|t| t != "glyf")
		.collect();
	let other_tables: Vec<&str> = other_tables.iter().map(String::as_str).collect();
	let actual = strip_ttx(&to_ttx(&mut font, &other_tables)?);
	assert_eq!(actual, strip_ttx(&to_ttx(&mut expected, &other_tables)?));
	Ok(())
}

#[test]
fn remove_400() -> TestResult {
	check_removed(
		400.0,
		"test_results/PartialInstancerTest3-VF-instance-400-no-overlaps.ttx",
	)
}

#[test]
fn remove_700() -> TestResult {
	check_removed(
		700.0,
		"test_results/PartialInstancerTest3-VF-instance-700-no-overlaps.ttx",
	)
}
