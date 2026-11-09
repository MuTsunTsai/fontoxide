//! `InstantiateAvar2Test` of fontTools' `Tests/varLib/instancer/instancer_test.py`.

#![allow(clippy::type_complexity)]

mod common;
mod instancer_common;

use std::collections::{BTreeMap, HashMap};

use common::*;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::var_store::{build_var_data, build_var_region_list, build_var_store};
use fontoxide_ttlib::ot::varmap::DeltaSetIndexMap;
use fontoxide_ttlib::tables::fvar::Axis;
use fontoxide_ttlib::tables::glyf::Number;
use fontoxide_ttlib::tables::tuple_variation::{Delta, TupleVariation};
use fontoxide_ttlib::tables::{Avar, Fvar, Gvar};
use fontoxide_ttlib::var::{Location, support_scalar};
use fontoxide_ttlib::{Font, Tag};
use fontoxide_varlib::instancer::{AxisTriple, ReachableRanges, cull_variations, is_dead};
use instancer_common::*;

const UNIT: f64 = 1.0 / 16384.0;

const AVAR2_FULL_INSTANCE_LOCATION: [(&str, f64); 12] = [
	("GRAD", 0.0),
	("XTRA", 562.0),
	("XOPQ", 176.0),
	("YOPQ", 124.0),
	("YTLC", 500.0),
	("YTUC", 750.0),
	("YTAS", 767.0),
	("YTDE", -240.0),
	("YTFI", 760.0),
	("wght", 650.0),
	("wdth", 90.0),
	("opsz", 32.0),
];

fn tag(text: &str) -> Tag {
	Tag::from_str_padded(text).expect("a tag")
}

fn support_of(variation: &TupleVariation) -> Vec<(&str, (f64, f64, f64))> {
	variation
		.axes
		.iter()
		.map(|(t, r)| (t.as_str(), *r))
		.collect()
}

/// `_set_private_avar2_axis_deltas`.
fn set_private_avar2_axis_deltas(
	font: &mut Font,
	output_tag: &str,
	regions: &[Region],
	deltas: &[i64],
) -> TestResult {
	let axis_tags = axis_tags(font)?;
	let output_index = axis_tags
		.iter()
		.position(|t| t == output_tag)
		.ok_or("no axis")?;
	let region_list = build_var_region_list(regions, &axis_tags)?;
	let var_data = build_var_data(
		(0..regions.len() as u16).collect(),
		vec![deltas.to_vec()],
		false,
	);
	let avar = font.require::<Avar>()?;
	avar.table.var_store = Some(Box::new(build_var_store(region_list, vec![var_data])));
	let mut mapping = vec![NO_VARIATION_INDEX; axis_tags.len()];
	mapping[output_index] = 0;
	avar.table.var_idx_map = Some(Box::new(DeltaSetIndexMap {
		format: Some(0),
		mapping,
	}));
	font.require::<Fvar>()?.axes[output_index].flags |= 0x0001;
	Ok(())
}

fn glyph_variations(font: &mut Font, gid: u32) -> TestResult<&mut Vec<TupleVariation>> {
	Ok(font
		.require::<Gvar>()?
		.variations
		.iter_mut()
		.find(|e| e.0 == gid)
		.map(|e| &mut e.1)
		.ok_or("no variations")?)
}

/// `_add_gvar_sentinel`.
fn add_gvar_sentinel(
	font: &mut Font,
	axis_tag: &str,
	support: (f64, f64, f64),
	sentinel: (i64, i64),
) -> TestResult {
	let gid = font.glyph_id("T")?;
	let variations = glyph_variations(font, gid)?;
	let coordinate_count = variations[0].coordinates.len();
	variations.push(TupleVariation {
		axes: region(&[(axis_tag, support)]),
		coordinates: vec![point_delta(sentinel.0, sentinel.1); coordinate_count],
	});
	Ok(())
}

/// `_has_gvar_sentinel`.
fn has_gvar_sentinel(
	font: &mut Font,
	axis_tag: &str,
	support: (f64, f64, f64),
	sentinel: (i64, i64),
) -> TestResult<bool> {
	let gid = font.glyph_id("T")?;
	let expected = region(&[(axis_tag, support)]);
	Ok(glyph_variations(font, gid)?.iter().any(|variation| {
		variation.axes == expected
			&& matches!(
				variation.coordinates.first(),
				Some(Some(Delta::Point(x, y)))
					if x.value() == sentinel.0 as f64 && y.value() == sentinel.1 as f64
			)
	}))
}

fn input_axis(font: &mut Font, tag: &str) -> TestResult<Axis> {
	Ok(font
		.require::<Fvar>()?
		.axes
		.iter()
		.find(|axis| axis.tag_text() == tag)
		.ok_or("no axis")?
		.clone())
}

/// The `(min, default, max)` of an axis, as a limit.
fn own_range(axis: &Axis) -> Option<AxisTriple> {
	triple(axis.min_value, axis.default_value, axis.max_value)
}

fn assert_final_coords_close(a: &Location, b: &Location) {
	assert_eq!(
		a.keys().collect::<Vec<_>>(),
		b.keys().collect::<Vec<_>>(),
		"the same axes"
	);
	for (tag, value) in a {
		assert!(
			(value - b[tag]).abs() <= 2.0 * UNIT,
			"{tag}: {value} != {}",
			b[tag]
		);
	}
}

fn has_avar2_store(font: &mut Font) -> TestResult<bool> {
	Ok(font.require::<Avar>()?.table.var_store.is_some())
}

mod instantiate_avar2_test {
	use super::*;

	#[test]
	fn test_partial_limit_axis_roundtrip_matches_direct_instance() -> TestResult {
		let avar2 = avar2_varfont()?;
		let mut partial = instantiated(&avar2, &[("wght", triple(300.0, 400.0, 700.0))])?;

		let wght = input_axis(&mut partial, "wght")?;
		assert_eq!(
			(wght.min_value, wght.default_value, wght.max_value),
			(300.0, 400.0, 700.0)
		);
		let avar = partial.require::<Avar>()?;
		assert_eq!(avar.major_version, 2);
		assert!(avar.table.var_store.is_some());
		assert!(avar.table.var_idx_map.is_some());
		for t in [b"gvar", b"HVAR", b"MVAR"] {
			assert!(partial.contains(Tag::new(t)));
		}

		let partial = reload(&mut partial)?;

		let location: Vec<(&str, Option<AxisTriple>)> = AVAR2_FULL_INSTANCE_LOCATION
			.iter()
			.map(|(t, v)| (*t, pin(*v)))
			.collect();
		let mut staged = instantiated(&partial, &location)?;
		let mut direct = instantiated(&avar2_varfont()?, &location)?;

		assert_same_ttx(&dump_ttx(&mut staged)?, &dump_ttx(&mut direct)?, "staged");
		Ok(())
	}

	#[test]
	fn test_partial_pin_self_contained_axis_preserves_avar2() -> TestResult {
		let avar2 = avar2_varfont()?;
		let mut partial = instantiated(&avar2, &[("wght", pin(650.0))])?;

		assert!(!axis_tags(&mut partial)?.iter().any(|t| t == "wght"));
		let avar = partial.require::<Avar>()?;
		assert!(!avar.segments.contains_key(&tag("wght")));
		assert_eq!(avar.major_version, 2);
		assert!(avar.table.var_store.is_some());
		assert!(avar.table.var_idx_map.is_some());
		for t in [b"gvar", b"HVAR", b"MVAR"] {
			assert!(partial.contains(Tag::new(t)));
		}

		let mut partial = reload(&mut partial)?;

		assert!(!axis_tags(&mut partial)?.iter().any(|t| t == "wght"));
		let avar = partial.require::<Avar>()?;
		assert!(!avar.segments.contains_key(&tag("wght")));
		assert_eq!(avar.major_version, 2);
		assert!(avar.table.var_store.is_some());
		Ok(())
	}

	// In Amstelvar-avar2.subset several fvar axes share a single avar2 varIdx (GRAD & XOPQ,
	// YOPQ & YTLC & YTUC, ...). Offset compensation must not leak between axes that share a
	// delta row. Each case below restricts/pins one or more shared-varIdx axes and checks that
	// the final normalized coordinates of every surviving axis still match the original font
	// at the same user location (the spec-pipeline oracle above).
	#[test]
	fn test_partial_instance_final_coords_match_original() -> TestResult {
		fn xopq_grad(x: f64, grads: &[f64]) -> Vec<Vec<(&'static str, f64)>> {
			grads
				.iter()
				.map(|g| vec![("XOPQ", x), ("GRAD", *g)])
				.collect()
		}
		let cases: Vec<(Vec<(&str, Option<AxisTriple>)>, Vec<Vec<(&str, f64)>>)> = vec![
			// two axes sharing a varIdx, both restricted
			(
				vec![
					("GRAD", triple(-100.0, 0.0, 300.0)),
					("XOPQ", triple(50.0, 176.0, 220.0)),
				],
				vec![
					vec![("GRAD", -100.0), ("XOPQ", 220.0)],
					vec![("GRAD", 300.0), ("XOPQ", 50.0)],
					vec![("GRAD", 150.0), ("XOPQ", 176.0)],
				],
			),
			// three axes sharing a varIdx, all restricted
			(
				vec![
					("YOPQ", triple(30.0, 124.0, 130.0)),
					("YTLC", triple(450.0, 500.0, 560.0)),
					("YTUC", triple(600.0, 750.0, 900.0)),
				],
				vec![
					vec![("YOPQ", 30.0), ("YTLC", 560.0), ("YTUC", 600.0)],
					vec![("YOPQ", 130.0), ("YTLC", 450.0), ("YTUC", 900.0)],
				],
			),
			// restrict one axis of a shared pair; the free partner (GRAD) must stay uncorrupted
			// even when the restricted axis is off its default
			(
				vec![("XOPQ", triple(100.0, 176.0, 250.0))],
				xopq_grad(250.0, &[-300.0, 0.0, 300.0, 500.0]),
			),
			// restricted axis whose default MOVES: exercises the empty-region bias, which
			// contaminated the free partner unconditionally
			(
				vec![("XOPQ", triple(100.0, 220.0, 250.0))],
				xopq_grad(220.0, &[-300.0, 0.0, 500.0]),
			),
			// moved default sampled at INTERIOR points (unique varIdx axis): the inverse
			// renormalization kinks where the old default lands in the new space, so a plain
			// two-tent offset is wrong between the anchors even though it is exact at
			// min/default/max.
			(
				vec![("XTRA", triple(421.0, 586.0, 617.0))],
				[450.0, 470.0, 519.0, 560.0, 600.0]
					.iter()
					.map(|x| vec![("XTRA", *x)])
					.collect(),
			),
			// moved default on a shared-varIdx axis, sampled at interior points
			(
				vec![("XOPQ", triple(100.0, 220.0, 250.0))],
				[130.0, 176.0, 210.0]
					.iter()
					.flat_map(|x| xopq_grad(*x, &[-200.0, 300.0]))
					.collect(),
			),
			// pin one axis of a shared pair (kept hidden as it is not self-contained); its free
			// partner must be unaffected
			(
				vec![("XOPQ", pin(200.0))],
				xopq_grad(200.0, &[-300.0, 0.0, 500.0]),
			),
		];
		for (limits, samples) in cases {
			let mut original = avar2_varfont()?;
			let mut partial = instantiated(&avar2_varfont()?, &limits)?;
			let mut partial = reload(&mut partial)?;
			let partial_axes = axis_tags(&mut partial)?;
			for user_loc in &samples {
				let orig_final = avar2_final_coords(&mut original, user_loc)?;
				let part_loc: Vec<(&str, f64)> = user_loc
					.iter()
					.filter(|(k, _)| partial_axes.iter().any(|a| a == k))
					.copied()
					.collect();
				let part_final = avar2_final_coords(&mut partial, &part_loc)?;
				for tag in &partial_axes {
					let o = py_round(orig_final.get(tag).copied().unwrap_or(0.0) * 16384.0);
					let p = py_round(part_final.get(tag).copied().unwrap_or(0.0) * 16384.0);
					// tolerance: a couple of F2Dot14 units of rounding noise
					assert!(
						(o - p).abs() <= 2.0,
						"axis {tag} at {user_loc:?}: original final {o} != partial {p}"
					);
				}
			}
		}
		Ok(())
	}

	#[test]
	fn test_offset_compensation_exhaustive() -> TestResult {
		// Exhaustive offset-compensation check: every single-axis and pairwise restriction
		// (with a MOVED default, the hardest case) must reproduce the original font's final
		// normalized coordinates at interior user locations for every surviving axis. Also
		// doubles as an integration smoke test that the full partial-instancing path (including
		// region culling / getExtremes) runs cleanly across many axis combinations.
		let mut original = avar2_varfont()?;
		let axes: Vec<(String, f64, f64, f64)> = original
			.require::<Fvar>()?
			.axes
			.iter()
			.map(|a| (a.tag_text(), a.min_value, a.default_value, a.max_value))
			.collect();

		let moved = |i: usize| {
			let (_, lo, d, hi) = axes[i];
			(
				py_round(lo + (d - lo) * 0.3),
				py_round(d + (hi - d) * 0.3),
				py_round(hi),
			)
		};

		let mut combos: Vec<Vec<usize>> = (0..axes.len()).map(|i| vec![i]).collect();
		for i in 0..axes.len() {
			for j in i + 1..axes.len() {
				combos.push(vec![i, j]);
			}
		}
		let base = avar2_varfont()?;
		for combo in combos {
			let limits: Vec<(&str, Option<AxisTriple>)> = combo
				.iter()
				.map(|&i| {
					let (a, b, c) = moved(i);
					(axes[i].0.as_str(), triple(a, b, c))
				})
				.collect();
			let mut partial = instantiated(&base, &limits)?;
			let partial_axes = axis_tags(&mut partial)?;
			for i in 0..5 {
				let user_loc: Vec<(&str, f64)> = combo
					.iter()
					.map(|&a| {
						let (lo, _, hi) = moved(a);
						(axes[a].0.as_str(), lo + (hi - lo) * f64::from(i) / 4.0)
					})
					.collect();
				let orig_final = avar2_final_coords(&mut original, &user_loc)?;
				let part_loc: Vec<(&str, f64)> = user_loc
					.iter()
					.filter(|(k, _)| partial_axes.iter().any(|a| a == k))
					.copied()
					.collect();
				let part_final = avar2_final_coords(&mut partial, &part_loc)?;
				for tag in &partial_axes {
					let o = py_round(orig_final.get(tag).copied().unwrap_or(0.0) * 16384.0);
					let p = py_round(part_final.get(tag).copied().unwrap_or(0.0) * 16384.0);
					assert!(
						(o - p).abs() <= 2.0,
						"{combo:?} axis {tag} at {user_loc:?}: original final {o} != partial {p}"
					);
				}
			}
		}
		Ok(())
	}

	#[test]
	fn test_offset_compensation_interior_avar1_breakpoints() -> TestResult {
		// An avar v1 map with a breakpoint strictly inside the retained range adds a kink to the
		// offset function that the {-1, 0, +1, z_old} anchors alone miss, so partial instancing
		// was approximate there (worst under a moved default). _instantiateAvarV2 now also
		// collects each retained avar v1 breakpoint, removing that structural error. A smaller
		// residual from F2Dot14 requantization of the retained (steep, near-edge) segment map
		// can remain, so tolerances are per-case rather than the 2-unit quantization floor used
		// elsewhere.
		type Case<'a> = (&'a [(f64, f64)], (f64, f64, f64), f64);
		let cases: [Case<'_>; 2] = [
			// gentle map, moderate moved default: structural error fully removed.
			(
				&[
					(-1.0, -1.0),
					(-0.5, -0.35),
					(0.0, 0.0),
					(0.5, 0.65),
					(1.0, 1.0),
				],
				(8.0, 40.0, 120.0),
				2.0,
			),
			// moved default near the fvar edge + interior breakpoint: the regime that exposed
			// the bug. Structural error removed (was 14); the ~3 residual is retained-map
			// requantization, not the offset math.
			(
				&[(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0), (-0.45, -0.27)],
				(8.0, 89.6, 144.0),
				6.0,
			),
		];
		for (segments, (lo, default, hi), tol) in cases {
			let make = || -> TestResult<Font> {
				let mut font = avar2_varfont()?;
				font.require::<Avar>()?
					.segments
					.insert(tag("opsz"), segments.to_vec());
				Ok(font)
			};

			let mut original = make()?;
			let mut partial = instantiated(&make()?, &[("opsz", triple(lo, default, hi))])?;
			assert!(axis_tags(&mut partial)?.iter().any(|t| t == "opsz"));
			let mut worst: f64 = 0.0;
			for i in 0..401 {
				let u = lo + (hi - lo) * f64::from(i) / 400.0;
				let loc = [("opsz", u)];
				let o = py_round(avar2_final_coords(&mut original, &loc)?["opsz"] * 16384.0);
				let p = py_round(avar2_final_coords(&mut partial, &loc)?["opsz"] * 16384.0);
				worst = worst.max((o - p).abs());
			}
			assert!(
				worst <= tol,
				"{segments:?}: worst {worst} > tolerance {tol}"
			);
		}
		Ok(())
	}

	#[test]
	fn test_truncated_var_idx_map() -> TestResult {
		// A compiled DeltaSetIndexMap may be stored truncated (trailing duplicates trimmed;
		// reads clamp to the last entry). The fixture's mapping ends in NO_VARIATION_INDEX
		// entries for wght/wdth/opsz, like fontations' Amstelvar-avar2.A.ttf; truncating it must
		// not break the offset-compensation write path (which used to IndexError when writing at
		// an axis index beyond the stored length).
		let mut avar2 = avar2_varfont()?;
		let map = avar2
			.require::<Avar>()?
			.table
			.var_idx_map
			.as_mut()
			.ok_or("no VarIdxMap")?;
		assert_eq!(
			map.mapping[map.mapping.len() - 3..],
			[NO_VARIATION_INDEX; 3]
		);
		map.mapping.truncate(10);

		let limits = [("opsz", triple(8.0, 14.0, 30.0))];
		let mut reference = instantiated(&avar2_varfont()?, &limits)?;
		let mut partial = instantiated(&avar2, &limits)?;

		// Same final coordinates as instancing the untruncated font.
		for value in [8.0, 14.0, 22.0, 30.0] {
			let loc = [("opsz", value)];
			assert_final_coords_close(
				&avar2_final_coords(&mut partial, &loc)?,
				&avar2_final_coords(&mut reference, &loc)?,
			);
		}
		Ok(())
	}

	#[test]
	fn test_empty_v1_segments_keeps_avar2() -> TestResult {
		// avar2 fonts need no v1 segment maps (design doc §9.6). An empty (or partial) segments
		// dict must not trigger the "all axes pinned" early drop of the table on a pure range
		// restriction — that would delete the entire avar2 VarStore while gvar stays in
		// old-space coordinates.
		let mut original = avar2_varfont()?;
		original.require::<Avar>()?.segments = BTreeMap::new();
		let mut avar2 = avar2_varfont()?;
		avar2.require::<Avar>()?.segments = BTreeMap::new();

		let mut partial = instantiated(&avar2, &[("wght", triple(300.0, 400.0, 700.0))])?;

		assert!(partial.contains(Tag::new(b"avar")));
		assert!(has_avar2_store(&mut partial)?);

		for wght in [300.0, 400.0, 550.0, 700.0] {
			let loc = [("wght", wght)];
			assert_final_coords_close(
				&avar2_final_coords(&mut partial, &loc)?,
				&avar2_final_coords(&mut original, &loc)?,
			);
		}
		Ok(())
	}

	#[test]
	fn test_avar2_null_varstore_never_downgraded() -> TestResult {
		// An avar 2.0 table with a NULL VarStore is legal and behaves as plain v1 (it used to
		// crash normalize() by entering the v2 path). It is never downgraded to v1: partial
		// instancing takes the avar2 path, creating offset-compensation delta rows on demand so
		// the instance stays bit-exact against the original.
		let without_store = || -> TestResult<Font> {
			let mut font = avar2_varfont()?;
			let avar = font.require::<Avar>()?;
			avar.table.var_store = None;
			avar.table.var_idx_map = None;
			Ok(font)
		};
		let mut original = without_store()?;

		let mut partial =
			instantiated(&without_store()?, &[("wght", triple(300.0, 400.0, 700.0))])?;
		let wght = input_axis(&mut partial, "wght")?;
		assert_eq!(
			(wght.min_value, wght.default_value, wght.max_value),
			(300.0, 400.0, 700.0)
		);
		assert_eq!(partial.require::<Avar>()?.major_version, 2);
		assert!(has_avar2_store(&mut partial)?);

		// Survives compile+decompile, and the final normalized coordinates match the original
		// (v1-behaving) font at the same user locations.
		let mut partial = reload(&mut partial)?;
		assert_eq!(partial.require::<Avar>()?.major_version, 2);

		for wght in [300.0, 350.0, 400.0, 550.0, 700.0] {
			let loc = [("wght", wght)];
			assert_final_coords_close(
				&avar2_final_coords(&mut partial, &loc)?,
				&avar2_final_coords(&mut original, &loc)?,
			);
		}
		Ok(())
	}

	#[test]
	fn test_unpopulated_default_2_tuple_limits() -> TestResult {
		// AxisLimits({"wght": (300, 700)}) leaves default=None; the avar2 offset-compensation
		// path must fall back to the fvar default instead of crashing. Here, that is up to
		// `user_limits`, as the limits have no missing values after that. Also exercises the
		// normalizedLimits made from the limits.
		let mut font = avar2_varfont()?;
		let limits = user_limits(&mut font, &[("wght", range(300.0, 700.0))])?;
		let normalized = limits.normalize(&mut font, true)?;
		fontoxide_varlib::instancer::instantiate_avar(&mut font, &limits, &normalized, None)?;

		assert!(font.contains(Tag::new(b"avar")));
		assert!(has_avar2_store(&mut font)?);
		Ok(())
	}

	#[test]
	fn test_is_tuple_variation_dead_boundary_peak() {
		fn ranges(lo: f64, hi: f64) -> ReachableRanges {
			[("wght".to_owned(), (lo, hi))].into_iter().collect()
		}
		// supportScalar returns 1 (not 0) at v == peak, so a tent whose peak sits exactly on the
		// reachable boundary is still live there.
		let live = tuple_variation(&[("wght", (0.5, 0.5, 1.0))], &[10; 4]);
		assert!(!is_dead(&live, &ranges(0.0, 0.5)));
		let live = tuple_variation(&[("wght", (-1.0, -0.5, -0.5))], &[10; 4]);
		assert!(!is_dead(&live, &ranges(-0.5, 0.0)));
		let dead = tuple_variation(&[("wght", (0.5, 0.75, 1.0))], &[10; 4]);
		assert!(is_dead(&dead, &ranges(0.0, 0.25)));
	}

	#[test]
	fn test_cull_glyph_variations() -> TestResult {
		for extended in [false, true] {
			let mut font = Font::new();
			font.set_glyph_order(vec!["A".to_owned()]);
			let mut fvar = Fvar::default();
			fvar.axes.push(Axis::new(tag("wght")));
			font.insert(Box::new(fvar));
			let variation = |support, (x, y)| TupleVariation {
				axes: region(&[("wght", support)]),
				coordinates: vec![point_delta(x, y); 4],
			};
			let live = variation((0.0, 0.25, 0.5), (10, 20));
			let dead = variation((0.5, 0.75, 1.0), (30, 40));
			let gvar = Gvar {
				variations: vec![(0, vec![live.clone(), dead])],
				extended,
				..Default::default()
			};
			font.insert(Box::new(gvar));

			let ranges: ReachableRanges = [("wght".to_owned(), (0.0, 0.25))].into_iter().collect();
			cull_variations(&mut font, &ranges)?;

			let gvar = font.require_locked::<Gvar>(extended)?;
			assert_eq!(gvar.variations, [(0, vec![live])]);
		}
		Ok(())
	}

	/// The key of a variation (`freeze`), where `1 == 1.0`.
	fn freeze(variation: &TupleVariation) -> Vec<u64> {
		let mut key = Vec::new();
		for (tag, (a, b, c)) in &variation.axes {
			key.extend(tag.bytes().map(u64::from));
			key.extend([a, b, c].map(|v| (v + 0.0).to_bits()));
		}
		for delta in &variation.coordinates {
			let bits = |n: &Number| (n.value() + 0.0).to_bits();
			match delta {
				Some(Delta::Point(x, y)) => key.extend([bits(x), bits(y)]),
				Some(Delta::Value(v)) => key.push(bits(v)),
				None => key.push(u64::MAX),
			}
		}
		key
	}

	#[test]
	fn test_avar2_culling_preserves_live_gvar_regions() -> TestResult {
		// Region culling may only delete TupleVariations whose support scalar is zero at EVERY
		// reachable final coordinate. Sample a grid of user locations, map each through the spec
		// pipeline of the ORIGINAL font, and require every culled gvar variation to be dead at
		// all of them.
		let mut original = avar2_varfont()?;
		let mut partial = instantiated(
			&avar2_varfont()?,
			&[
				("wght", triple(300.0, 400.0, 700.0)),
				("opsz", triple(10.0, 14.0, 20.0)),
			],
		)?;

		let mut grid = Vec::new();
		for w in [300.0, 400.0, 550.0, 700.0] {
			for o in [10.0, 14.0, 20.0] {
				for g in [-300.0, 0.0, 500.0] {
					grid.push([("wght", w), ("opsz", o), ("GRAD", g)]);
				}
			}
		}
		let finals = grid
			.iter()
			.map(|loc| avar2_final_coords(&mut original, loc))
			.collect::<TestResult<Vec<_>>>()?;

		let gvar_old = original.require::<Gvar>()?.variations.clone();
		let gvar_new = &partial.require::<Gvar>()?.variations;
		for (gid, variations) in &gvar_old {
			let mut kept: HashMap<Vec<u64>, usize> = HashMap::new();
			let new = &gvar_new.iter().find(|e| e.0 == *gid).ok_or("no glyph")?.1;
			for tv in new {
				*kept.entry(freeze(tv)).or_default() += 1;
			}
			for tv in variations {
				if let Some(count) = kept.get_mut(&freeze(tv))
					&& *count > 0
				{
					*count -= 1;
					continue;
				}
				// culled: must be unreachable at every sampled location
				for (loc, last) in grid.iter().zip(&finals) {
					let scalar = support_scalar(last, support_of(tv));
					assert_eq!(
						scalar, 0.0,
						"culled variation {:?} of {gid} is live at {loc:?} -> {last:?}",
						tv.axes
					);
				}
			}
		}
		Ok(())
	}

	const OUTPUT_TAG: &str = "XOPQ";
	const INPUT_TAG: &str = "wght";

	/// Instancing the input axis at its own range: `instantiateVariableFont(font, {wght: ...})`.
	fn instantiate_at_own_range(font: &Font, axis: &Axis) -> TestResult<Font> {
		instantiated(font, &[(INPUT_TAG, own_range(axis))])
	}

	#[test]
	fn test_culling_preserves_one_sided_support_sentinel() -> TestResult {
		let support = (0.5, 1.0, 1.0);
		let sentinel = (12345, -6789);
		let mut avar2 = avar2_varfont()?;
		set_private_avar2_axis_deltas(
			&mut avar2,
			OUTPUT_TAG,
			&[
				region(&[(INPUT_TAG, (0.0, 0.25, 0.25))]),
				region(&[(INPUT_TAG, (0.0, 0.25, 0.5))]),
			],
			&[-16384, 16384],
		)?;
		add_gvar_sentinel(&mut avar2, OUTPUT_TAG, support, sentinel)?;

		let input_axis = input_axis(&mut avar2, INPUT_TAG)?;
		let normalized_input = 4097.0 / 16384.0;
		let user_input = input_axis.default_value
			+ normalized_input * (input_axis.max_value - input_axis.default_value);
		let location = [(INPUT_TAG, user_input)];
		let before = avar2_final_coords(&mut avar2, &location)?;
		assert_eq!(before[OUTPUT_TAG], 16380.0 / 16384.0);
		assert!(support_scalar(&before, [(OUTPUT_TAG, support)]) > 0.99);

		let mut partial = instantiate_at_own_range(&avar2, &input_axis)?;

		assert_eq!(
			avar2_final_coords(&mut partial, &location)?[OUTPUT_TAG],
			before[OUTPUT_TAG]
		);
		assert!(has_gvar_sentinel(
			&mut partial,
			OUTPUT_TAG,
			support,
			sentinel
		)?);
		Ok(())
	}

	#[test]
	fn test_culling_bounds_runtime_rounded_avar2_delta() -> TestResult {
		let support = (UNIT, UNIT, 1.0);
		let sentinel = (22222, -3333);
		let mut avar2 = avar2_varfont()?;
		set_private_avar2_axis_deltas(
			&mut avar2,
			OUTPUT_TAG,
			&[
				region(&[(INPUT_TAG, (0.0, 0.125, 0.25))]),
				region(&[(INPUT_TAG, (0.0, 0.125, 0.375))]),
			],
			&[-8, 1],
		)?;
		add_gvar_sentinel(&mut avar2, OUTPUT_TAG, support, sentinel)?;

		let input_axis = input_axis(&mut avar2, INPUT_TAG)?;
		let user_input =
			input_axis.default_value + 0.25 * (input_axis.max_value - input_axis.default_value);
		let location = [(INPUT_TAG, user_input)];
		let before = avar2_final_coords(&mut avar2, &location)?;
		assert_eq!(before[OUTPUT_TAG], UNIT);
		assert_eq!(support_scalar(&before, [(OUTPUT_TAG, support)]), 1.0);

		let mut partial = instantiate_at_own_range(&avar2, &input_axis)?;

		assert_eq!(
			avar2_final_coords(&mut partial, &location)?[OUTPUT_TAG],
			UNIT
		);
		assert!(has_gvar_sentinel(
			&mut partial,
			OUTPUT_TAG,
			support,
			sentinel
		)?);
		Ok(())
	}

	#[test]
	fn test_culling_clamps_overshooting_range_on_both_sides() -> TestResult {
		let cases = [
			(32767, (0.5, 1.0, 1.0), 1.0, (30001, -30001)),
			(-32768, (-1.0, -1.0, -0.5), -1.0, (-30001, 30001)),
		];
		for (delta, support, expected, sentinel) in cases {
			let mut avar2 = avar2_varfont()?;
			set_private_avar2_axis_deltas(&mut avar2, OUTPUT_TAG, &[region(&[])], &[delta])?;
			add_gvar_sentinel(&mut avar2, OUTPUT_TAG, support, sentinel)?;

			let before = avar2_final_coords(&mut avar2, &[])?;
			assert_eq!(before[OUTPUT_TAG], expected);
			assert_eq!(support_scalar(&before, [(OUTPUT_TAG, support)]), 1.0);

			let input_axis = input_axis(&mut avar2, INPUT_TAG)?;
			let mut partial = instantiate_at_own_range(&avar2, &input_axis)?;

			assert_eq!(avar2_final_coords(&mut partial, &[])?[OUTPUT_TAG], expected);
			assert!(has_gvar_sentinel(
				&mut partial,
				OUTPUT_TAG,
				support,
				sentinel
			)?);
		}
		Ok(())
	}

	#[test]
	fn test_full_instance_avar2_malformed_empty_varstore() -> TestResult {
		// With no VarIdxMap, an empty non-NULL VarStore produces implicit indices outside the
		// store. The partial avar2 path treats them as zero deltas, so full instancing should do
		// the same. HarfBuzz corroborates that defensive policy:
		// https://github.com/harfbuzz/harfbuzz/blob/d2df3cdcc0836299a163dc0399a6b047d19ac56c/src/hb-ot-layout-common.hh#L3354-L3363
		let mut avar2 = avar2_varfont()?;
		let tags = axis_tags(&mut avar2)?;
		let store: ot::VarStore = build_var_store(build_var_region_list(&[], &tags)?, Vec::new());
		let avar = avar2.require::<Avar>()?;
		avar.table.var_store = Some(Box::new(store));
		avar.table.var_idx_map = None;
		let defaults: Vec<(String, f64)> = avar2
			.require::<Fvar>()?
			.axes
			.iter()
			.map(|a| (a.tag_text(), a.default_value))
			.collect();
		let location: Vec<(&str, Option<AxisTriple>)> = defaults
			.iter()
			.map(|(t, v)| (t.as_str(), pin(*v)))
			.collect();

		let is_static = instantiated(&avar2, &location)?;

		assert!(!is_static.contains(Tag::new(b"fvar")));
		assert!(!is_static.contains(Tag::new(b"avar")));
		Ok(())
	}
}
