//! Port of fontTools' `Tests/varLib/instancer/colr_test.py`.
//!
//! `buildCOLR` isn't ported, so the tables that fontTools builds with it are written
//! out here.

#![allow(clippy::type_complexity)]

mod common;
mod module_support;

use std::collections::BTreeMap;

use common::{font_from_ttx, instancer_data, limits, pin, triple};
use fontoxide_ttlib::Font;
use fontoxide_ttlib::Tag;
use fontoxide_ttlib::ot::colr::ClipList;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::types::{ClipBox, Paint};
use fontoxide_ttlib::ot::var_store::{build_var_data, build_var_region_list, build_var_store};
use fontoxide_ttlib::ot::varmap::DeltaSetIndexMap;
use fontoxide_ttlib::tables::Fvar;
use fontoxide_ttlib::tables::colr::Colr;
use fontoxide_varlib::instancer::{
	AxisTriple, InstancerOptions, OverlapMode, instantiate_variable_font,
};
use module_support::*;

const COLR: Tag = Tag::new(b"COLR");
const NO_VARIATION_INDEX: u32 = 0xFFFF_FFFF;
const NO_VARIATION: u64 = 0xFFFF_FFFF;

type Pairs<'a> = &'a [(&'a str, Option<AxisTriple>)];

/// The `font` fixture.
fn font_fixture() -> TestResult<Font> {
	font_from_ttx(&instancer_data("COLRv1-alpha.ttx"))
}

fn table(font: &mut Font) -> TestResult<&mut ot::COLR> {
	Ok(font
		.require::<Colr>()?
		.table
		.as_mut()
		.ok_or("no otData table")?)
}

/// `instantiateVariableFont(font, limits, overlap=False)`, in a copy.
fn instantiate_copy(font: &Font, pairs: Pairs<'_>) -> TestResult<Font> {
	let mut font = font.clone();
	instantiate_variable_font(
		&mut font,
		&limits(pairs),
		&InstancerOptions {
			overlap: OverlapMode::KeepAndDontSetFlags,
			..Default::default()
		},
	)?;
	Ok(font)
}

/// `instance`.
fn instance(font: &Font, pairs: Pairs<'_>) -> TestResult<Font> {
	roundtrip(&mut instantiate_copy(font, pairs)?)
}

/// `paints`.
fn paints(font: &mut Font) -> TestResult<Vec<&mut Paint>> {
	let list = table(font)?
		.base_glyph_list
		.as_mut()
		.ok_or("no BaseGlyphList")?;
	list.base_glyph_paint_record
		.iter_mut()
		.map(|r| Ok(r.paint.as_deref_mut().ok_or("no Paint")?))
		.collect()
}

fn alpha(paint: &Paint) -> f64 {
	match paint {
		Paint::Format2(p) => p.alpha,
		Paint::Format3(p) => p.alpha,
		p => panic!("not a solid: {p:?}"),
	}
}

fn var_index_base_mut(paint: &mut Paint) -> &mut u32 {
	match paint {
		Paint::Format3(p) => &mut p.var_index_base,
		p => panic!("not a PaintVarSolid: {p:?}"),
	}
}

fn var_store(font: &mut Font) -> TestResult<&mut ot::VarStore> {
	Ok(table(font)?.var_store.as_deref_mut().ok_or("no VarStore")?)
}

fn region_axis_count(font: &mut Font) -> TestResult<u32> {
	var_store(font)?
		.var_region_list
		.as_ref()
		.ok_or("no VarRegionList")?
		.region_axis_count
		.ok_or_else(|| "no RegionAxisCount".into())
}

fn axis_tags(font: &mut Font) -> TestResult<Vec<String>> {
	Ok(font
		.require::<Fvar>()?
		.axes
		.iter()
		.map(|a| a.tag_text())
		.collect())
}

/// `buildDeltaSetIndexMap`.
fn build_delta_set_index_map(mapping: &[u64]) -> Option<Box<DeltaSetIndexMap>> {
	if mapping.iter().enumerate().all(|(i, &v)| i as u64 == v) {
		return None;
	}
	Some(Box::new(DeltaSetIndexMap {
		format: Some(u16::from(mapping.len() > 0xFFFF)),
		mapping: mapping.to_vec(),
	}))
}

/// `buildVarStore` of a single region on `wght`, with a row of each delta.
fn wght_store(deltas: &[i64]) -> TestResult<ot::VarStore> {
	let support: BTreeMap<String, (f64, f64, f64)> =
		[("wght".to_owned(), (0.0, 1.0, 1.0))].into_iter().collect();
	let axes = ["wght".to_owned(), "wdth".to_owned()];
	let rows = deltas.iter().map(|&d| vec![d]).collect();
	Ok(build_var_store(
		build_var_region_list(&[support], &axes)?,
		vec![build_var_data(vec![0], rows, true)],
	))
}

/// `font["COLR"] = buildCOLR({"a": paint}, version=1, ...)`: `paint` for the glyph "a" and
/// the layers it refers to, the store and the clip box of "a".
fn set_colr(
	font: &mut Font,
	paint: Paint,
	layers: Vec<Paint>,
	store: ot::VarStore,
	clip: Option<ClipBox>,
) -> TestResult {
	let a = font.glyph_id("a")?;
	let layer_list = (!layers.is_empty()).then(|| {
		Box::new(ot::LayerList {
			layer_count: None,
			paint: layers.into_iter().map(|p| Some(Box::new(p))).collect(),
		})
	});
	let clip_list = clip.map(|clip| {
		Box::new(ClipList {
			format: Some(1),
			clips: [(a, clip)].into_iter().collect(),
		})
	});
	font.insert(Box::new(Colr {
		version: 1,
		color_layers: None,
		table: Some(ot::COLR {
			version: 1,
			base_glyph_list: Some(Box::new(ot::BaseGlyphList {
				base_glyph_count: None,
				base_glyph_paint_record: vec![ot::BaseGlyphPaintRecord {
					base_glyph: a,
					paint: Some(Box::new(paint)),
				}],
			})),
			layer_list,
			clip_list,
			var_store: Some(Box::new(store)),
			..Default::default()
		}),
	}));
	Ok(())
}

/// A `PaintSolid` of the palette index 0, opaque.
fn solid() -> Option<Box<Paint>> {
	Some(Box::new(Paint::Format2(ot::PaintFormat2 {
		palette_index: 0,
		alpha: 1.0,
	})))
}

#[test]
fn test_full_instance_clamps_solid_and_gradient_alpha() -> TestResult {
	for (weight, width, expected) in [(900.0, 100.0, 1.0), (650.0, 125.0, 0.5)] {
		let font = font_fixture()?;
		let mut result = instance(&font, &[("wght", pin(weight)), ("wdth", pin(width))])?;
		assert!(!result.contains(Tag::new(b"fvar")));
		let colr = table(&mut result)?;
		assert!(colr.var_store.is_none());
		assert!(colr.var_index_map.is_none());
		let paints = paints(&mut result)?;
		let [first, second, gradient] = &paints[..] else {
			return Err("not three paints".into());
		};
		assert!(matches!(first, Paint::Format2(_)));
		assert!(matches!(second, Paint::Format2(_)));
		assert_eq!(alpha(first), expected);
		assert_eq!(alpha(second), 0.0);
		// A `PaintLinearGradient`, whose `ColorLine` and `ColorStop` aren't the variable ones.
		let Paint::Format4(gradient) = gradient else {
			return Err("not a PaintLinearGradient".into());
		};
		let line = gradient.color_line.as_ref().ok_or("no ColorLine")?;
		let alphas: Vec<f64> = line.color_stop.iter().map(|s| s.alpha).collect();
		assert_eq!(alphas, [expected, 0.0]);
	}
	Ok(())
}

#[test]
fn test_partial_and_composed_instances() -> TestResult {
	let font = font_fixture()?;
	let mut partial = instance(&font, &[("wght", pin(650.0))])?;
	{
		let paints = paints(&mut partial)?;
		let [first, second, gradient] = &paints[..] else {
			return Err("not three paints".into());
		};
		assert!(matches!(first, Paint::Format3(_)));
		assert!(matches!(second, Paint::Format3(_)));
		assert_eq!(alpha(first), 1.5);
		assert_eq!(alpha(second), -1.5);
		let Paint::Format5(gradient) = gradient else {
			return Err("not a PaintVarLinearGradient".into());
		};
		let line = gradient.color_line.as_ref().ok_or("no ColorLine")?;
		assert_eq!(line.color_stop[0].alpha, 1.5);
	}
	assert_eq!(region_axis_count(&mut partial)?, 1);
	assert_eq!(axis_tags(&mut partial)?, ["wdth"]);
	let mut composed = instance(&partial, &[("wdth", pin(125.0))])?;
	let mut direct = instance(&font, &[("wght", pin(650.0)), ("wdth", pin(125.0))])?;
	assert_eq!(composed.table_data(COLR)?, direct.table_data(COLR)?);
	Ok(())
}

#[test]
fn test_optimizes_mapped_partial_store() -> TestResult {
	for split_store in [false, true] {
		for move_default in [false, true] {
			let mut font = font_fixture()?;
			let mut mapping: Vec<u64> = (0..6).collect();
			if split_store {
				// Reverse the rows and split them across two outer indices. The map
				// restores their original meanings, including consecutive stop fields.
				let store = var_store(&mut font)?;
				let mut rows = store.var_data[0].as_ref().ok_or("no VarData")?.item.clone();
				rows.reverse();
				let rest = rows.split_off(3);
				store.var_data = vec![
					Some(Box::new(build_var_data(vec![0, 1], rows, true))),
					Some(Box::new(build_var_data(vec![0, 1], rest, true))),
				];
				store.var_data_count = Some(2);
				mapping = vec![0x10002, 0x10001, 0x10000, 2, 1, 0];
			}
			mapping.push(NO_VARIATION);
			table(&mut font)?.var_index_map = build_delta_set_index_map(&mapping);
			let wght = if move_default {
				triple(400.0, 650.0, 900.0)
			} else {
				pin(650.0)
			};
			let mut partial = instance(&font, &[("wght", wght)])?;
			// Duplicate and zero rows are removed, while the mapping slots stay intact.
			let items: usize = var_store(&mut partial)?
				.var_data
				.iter()
				.flatten()
				.map(|d| d.item.len())
				.sum();
			assert_eq!(items, if move_default { 2 } else { 1 });
			let colr = table(&mut partial)?;
			let mapping = &colr.var_index_map.as_ref().ok_or("no VarIndexMap")?.mapping;
			assert_eq!(mapping.len(), 7);
			assert_eq!(mapping[0], mapping[3]);
			assert_ne!(mapping[0], NO_VARIATION);
			assert_eq!(mapping[1], mapping[5]);
			assert_eq!(mapping[2], NO_VARIATION);
			assert_eq!(mapping[4], NO_VARIATION);
			assert_eq!(mapping[6], NO_VARIATION);
			{
				let paints = paints(&mut partial)?;
				let [first, second, gradient] = &paints[..] else {
					return Err("not three paints".into());
				};
				let (Paint::Format3(first), Paint::Format3(second)) = (first, second) else {
					return Err("not PaintVarSolid".into());
				};
				assert_eq!((first.var_index_base, second.var_index_base), (0, 1));
				let Paint::Format5(gradient) = gradient else {
					return Err("not a PaintVarLinearGradient".into());
				};
				let line = gradient.color_line.as_ref().ok_or("no ColorLine")?;
				let bases: Vec<u32> = line.color_stop.iter().map(|s| s.var_index_base).collect();
				assert_eq!(bases, [2, 4]);
				assert_eq!(gradient.var_index_base, NO_VARIATION_INDEX);
			}
			let weights: &[f64] = if move_default {
				&[400.0, 650.0, 900.0]
			} else {
				&[650.0]
			};
			for &weight in weights {
				for width in [75.0, 100.0, 125.0] {
					let mut pairs = vec![("wdth", pin(width))];
					if move_default {
						pairs.push(("wght", pin(weight)));
					}
					let mut composed = instance(&partial, &pairs)?;
					let mut direct =
						instance(&font, &[("wght", pin(weight)), ("wdth", pin(width))])?;
					assert_eq!(composed.table_data(COLR)?, direct.table_data(COLR)?);
				}
			}
		}
	}
	Ok(())
}

#[test]
fn test_partial_implicit_mapping_preserves_consecutive_rows() -> TestResult {
	for empty_map in [false, true] {
		let mut font = font_fixture()?;
		if empty_map {
			table(&mut font)?.var_index_map = Some(Box::new(DeltaSetIndexMap {
				format: None,
				mapping: Vec::new(),
			}));
		}
		let mut partial = instance(&font, &[("wght", pin(650.0))])?;
		let data = var_store(&mut partial)?.var_data[0]
			.as_ref()
			.ok_or("no VarData")?;
		assert_eq!(data.item.len(), 6);
		for width in [75.0, 100.0, 125.0] {
			let mut composed = instance(&partial, &[("wdth", pin(width))])?;
			let mut direct = instance(&font, &[("wght", pin(650.0)), ("wdth", pin(width))])?;
			assert_eq!(composed.table_data(COLR)?, direct.table_data(COLR)?);
		}
	}
	Ok(())
}

#[test]
fn test_optimized_short_map_repeats_last_entry() -> TestResult {
	let mappings: [&[u64]; 3] = [&[3], &[NO_VARIATION, 3], &[3, NO_VARIATION]];
	for mapping in mappings {
		let mut font = font_fixture()?;
		table(&mut font)?.var_index_map = build_delta_set_index_map(mapping);
		*var_index_base_mut(paints(&mut font)?.remove(0)) = 3;
		let mut partial = instance(&font, &[("wght", pin(650.0))])?;
		let items: usize = var_store(&mut partial)?
			.var_data
			.iter()
			.flatten()
			.map(|d| d.item.len())
			.sum();
		assert_eq!(items, 1);
		let expected: Vec<u64> = mapping
			.iter()
			.map(|&index| if index == 3 { 0 } else { NO_VARIATION })
			.collect();
		let colr = table(&mut partial)?;
		let map = colr.var_index_map.as_ref().ok_or("no VarIndexMap")?;
		assert_eq!(map.mapping, expected);
		for width in [75.0, 100.0, 125.0] {
			let mut composed = instance(&partial, &[("wdth", pin(width))])?;
			let mut direct = instance(&font, &[("wght", pin(650.0)), ("wdth", pin(width))])?;
			assert_eq!(composed.table_data(COLR)?, direct.table_data(COLR)?);
		}
	}
	Ok(())
}

#[test]
fn test_moved_default_and_axis_range() -> TestResult {
	let font = font_fixture()?;
	let mut result = instance(&font, &[("wght", triple(400.0, 650.0, 900.0))])?;
	assert_eq!(alpha(paints(&mut result)?[0]), 1.5);
	assert_eq!(region_axis_count(&mut result)?, 2);
	for weight in [400.0, 650.0, 900.0] {
		let pairs = [("wght", pin(weight)), ("wdth", pin(125.0))];
		let mut actual = instance(&result, &pairs)?;
		let mut expected = instance(&font, &pairs)?;
		assert_eq!(actual.table_data(COLR)?, expected.table_data(COLR)?);
	}
	Ok(())
}

#[test]
fn test_implicit_empty_short_and_sentinel_maps() -> TestResult {
	let mappings: [Option<&[u64]>; 4] = [None, Some(&[]), Some(&[0]), Some(&[NO_VARIATION])];
	for mapping in mappings {
		let mut font = font_fixture()?;
		*var_index_base_mut(paints(&mut font)?.remove(0)) = if mapping.is_none() {
			NO_VARIATION_INDEX
		} else {
			3
		};
		table(&mut font)?.var_index_map = mapping.and_then(build_delta_set_index_map);
		// Other fields may address later mapping entries, which repeat the last.
		let mut result = instance(&font, &[("wght", pin(650.0))])?;
		let expected = if matches!(mapping, Some([] | [0])) {
			1.5
		} else {
			1.0
		};
		assert_eq!(alpha(paints(&mut result)?[0]), expected, "{mapping:?}");
		assert_eq!(region_axis_count(&mut result)?, 1);
	}
	Ok(())
}

fn var_color_line(extend: u8, stops: &[(f64, f64)]) -> Option<Box<ot::VarColorLine>> {
	Some(Box::new(ot::VarColorLine {
		extend,
		stop_count: None,
		color_stop: stops
			.iter()
			.map(|&(stop_offset, alpha)| ot::VarColorStop {
				stop_offset,
				palette_index: 0,
				alpha,
				var_index_base: NO_VARIATION_INDEX,
			})
			.collect(),
	}))
}

#[test]
fn test_layer_paints_transforms_angles_and_clips() -> TestResult {
	let mut font = font_fixture()?;
	let store = wght_store(&[
		32768, 0, 0, 0, 65536, -65536, 1, -1, 3, -3, 5, -5, 8192, 0, 100, -100,
	])?;
	let color_line = var_color_line(0, &[(0.0, 1.0)]);
	let transform = Paint::Format13(ot::PaintFormat13 {
		paint: solid(),
		transform: Some(Box::new(ot::VarAffine2x3 {
			xx: 1.0,
			yx: 0.0,
			xy: 0.0,
			yy: 1.0,
			dx: 0.0,
			dy: 0.0,
			var_index_base: 0,
		})),
	});
	let linear = Paint::Format5(ot::PaintFormat5 {
		color_line: color_line.clone(),
		x0: 0,
		y0: 0,
		x1: 100,
		y1: 100,
		x2: 200,
		y2: 200,
		var_index_base: 6,
	});
	let rotate = Paint::Format25(ot::PaintFormat25 {
		paint: solid(),
		angle: 270.0,
		var_index_base: 12,
	});
	let sweep = Paint::Format9(ot::PaintFormat9 {
		color_line,
		center_x: 100,
		center_y: 100,
		start_angle: 0.0,
		end_angle: 90.0,
		var_index_base: NO_VARIATION_INDEX,
	});
	let layers = Paint::Format1(ot::PaintFormat1 {
		num_layers: 4,
		first_layer_index: 0,
	});
	set_colr(
		&mut font,
		layers,
		vec![transform, linear, rotate, sweep],
		store,
		Some(ClipBox::Format2(ot::ClipBoxFormat2 {
			x_min: 0,
			y_min: 0,
			x_max: 400,
			y_max: 700,
			var_index_base: 12,
		})),
	)?;
	let font = roundtrip(&mut font)?;
	let mut result = instance(&font, &[("wght", pin(900.0)), ("wdth", pin(100.0))])?;
	let a = result.glyph_id("a")?;
	let colr = table(&mut result)?;
	let list = colr.layer_list.as_ref().ok_or("no LayerList")?;
	let [Some(transform), Some(linear), Some(rotate), Some(sweep)] = &list.paint[..] else {
		return Err("not four layers".into());
	};
	let Paint::Format12(transform) = &**transform else {
		return Err("not a PaintTransform".into());
	};
	let matrix = transform.transform.as_ref().ok_or("no Transform")?;
	assert_eq!((matrix.xx, matrix.dx, matrix.dy), (1.5, 1.0, -1.0));
	let Paint::Format4(linear) = &**linear else {
		return Err("not a PaintLinearGradient".into());
	};
	assert_eq!(
		(
			linear.x0, linear.y0, linear.x1, linear.y1, linear.x2, linear.y2
		),
		(1, -1, 103, 97, 205, 195)
	);
	let Paint::Format24(rotate) = &**rotate else {
		return Err("not a PaintRotate".into());
	};
	assert_eq!(rotate.angle, -360.0);
	let Paint::Format8(sweep) = &**sweep else {
		return Err("not a PaintSweepGradient".into());
	};
	assert_eq!((sweep.start_angle, sweep.end_angle), (0.0, 90.0));
	let clips = &colr.clip_list.as_ref().ok_or("no ClipList")?.clips;
	let ClipBox::Format1(clip) = &clips[&a] else {
		return Err("not a format 1 clip box".into());
	};
	assert_eq!(
		(clip.x_min, clip.y_min, clip.x_max, clip.y_max),
		(8192, 0, 500, 600)
	);
	Ok(())
}

/// The object identity of the shared paint in fontTools has no equivalent here, where
/// paints are owned: each gets the value that the shared one must have.
#[test]
fn test_shared_paints_are_instanced_once() -> TestResult {
	let mut font = font_fixture()?;
	let first = paints(&mut font)?[0].clone();
	let list = table(&mut font)?
		.base_glyph_list
		.as_mut()
		.ok_or("no BaseGlyphList")?;
	list.base_glyph_paint_record[1].paint = Some(Box::new(first));
	let mut partial = instantiate_copy(&font, &[("wght", pin(650.0))])?;
	let paints = paints(&mut partial)?;
	assert_eq!(paints[0], paints[1]);
	assert_eq!(alpha(paints[0]), 1.5);
	Ok(())
}

#[test]
fn test_constant_store_becomes_static_with_an_axis_remaining() -> TestResult {
	let mut font = font_fixture()?;
	for data in var_store(&mut font)?.var_data.iter_mut().flatten() {
		for row in &mut data.item {
			row[1] = 0;
		}
	}
	let mut result = instance(&font, &[("wght", pin(650.0))])?;
	assert_eq!(axis_tags(&mut result)?, ["wdth"]);
	assert!(table(&mut result)?.var_store.is_none());
	assert!(
		paints(&mut result)?
			.iter()
			.all(|p| matches!(p, Paint::Format2(_) | Paint::Format4(_)))
	);
	Ok(())
}

#[test]
fn test_scale_outside_f2dot14_becomes_transform() -> TestResult {
	for format in [17, 19, 21, 23] {
		for sign in [1.0_f64, -1.0] {
			let mut font = font_fixture()?;
			let delta = (sign * 16384.0) as i64;
			let mut rows = vec![delta, delta, 0, 0];
			if format == 23 {
				rows[1] = 0;
			}
			let store = wght_store(&rows)?;
			let scale = sign * 1.5;
			let paint = match format {
				17 => Paint::Format17(ot::PaintFormat17 {
					paint: solid(),
					scale_x: scale,
					scale_y: scale,
					var_index_base: 0,
				}),
				19 => Paint::Format19(ot::PaintFormat19 {
					paint: solid(),
					scale_x: scale,
					scale_y: scale,
					center_x: 37,
					center_y: -29,
					var_index_base: 0,
				}),
				21 => Paint::Format21(ot::PaintFormat21 {
					paint: solid(),
					scale,
					var_index_base: 0,
				}),
				_ => Paint::Format23(ot::PaintFormat23 {
					paint: solid(),
					scale,
					center_x: 37,
					center_y: -29,
					var_index_base: 0,
				}),
			};
			set_colr(&mut font, paint, Vec::new(), store, None)?;
			let font = roundtrip(&mut font)?;
			let mut result = instance(&font, &[("wght", pin(900.0)), ("wdth", pin(100.0))])?;
			let paints = paints(&mut result)?;
			let Paint::Format12(paint) = &*paints[0] else {
				return Err("not a PaintTransform".into());
			};
			let matrix = paint.transform.as_ref().ok_or("no Transform")?;
			assert_eq!(matrix.xx, sign * 2.5);
			assert_eq!(matrix.yy, sign * 2.5);
			assert_eq!(matrix.xy, 0.0);
			assert_eq!(matrix.yx, 0.0);
			let (cx, cy) = if matches!(format, 19 | 23) {
				(37.0, -29.0)
			} else {
				(0.0, 0.0)
			};
			assert_eq!(
				(matrix.dx, matrix.dy),
				(cx * (1.0 - sign * 2.5), cy * (1.0 - sign * 2.5))
			);
		}
	}
	Ok(())
}

/// A paint as `_radial_alpha` sees it: unlike the tables, it can have negative radii.
#[derive(Clone)]
enum Painted {
	Solid(f64),
	Radial {
		x0: f64,
		y0: f64,
		r0: f64,
		x1: f64,
		y1: f64,
		r1: f64,
		extend: u8,
		stops: Vec<(f64, f64)>,
	},
}

fn painted(paint: &Paint) -> TestResult<Painted> {
	Ok(match paint {
		Paint::Format2(p) => Painted::Solid(p.alpha),
		Paint::Format6(p) => {
			let line = p.color_line.as_ref().ok_or("no ColorLine")?;
			Painted::Radial {
				x0: f64::from(p.x0),
				y0: f64::from(p.y0),
				r0: f64::from(p.r0),
				x1: f64::from(p.x1),
				y1: f64::from(p.y1),
				r1: f64::from(p.r1),
				extend: line.extend,
				stops: line
					.color_stop
					.iter()
					.map(|s| (s.stop_offset, s.alpha))
					.collect(),
			}
		}
		Paint::Format7(p) => {
			let line = p.color_line.as_ref().ok_or("no ColorLine")?;
			Painted::Radial {
				x0: f64::from(p.x0),
				y0: f64::from(p.y0),
				r0: f64::from(p.r0),
				x1: f64::from(p.x1),
				y1: f64::from(p.y1),
				r1: f64::from(p.r1),
				extend: line.extend,
				stops: line
					.color_stop
					.iter()
					.map(|s| (s.stop_offset, s.alpha))
					.collect(),
			}
		}
		p => return Err(format!("an unexpected paint: {p:?}").into()),
	})
}

/// `_radial_alpha`.
fn radial_alpha(paint: &Painted, x: f64, y: f64) -> Option<f64> {
	let Painted::Radial {
		x0,
		y0,
		r0,
		x1,
		y1,
		r1,
		extend,
		stops,
	} = paint
	else {
		let Painted::Solid(alpha) = paint else {
			unreachable!()
		};
		return (*alpha != 0.0).then_some(*alpha);
	};
	let (dx, dy, dr) = (x1 - x0, y1 - y0, r1 - r0);
	let (px, py) = (x - x0, y - y0);
	let a = dx * dx + dy * dy - dr * dr;
	let b = -2.0 * (px * dx + py * dy + r0 * dr);
	let c = px * px + py * py - r0 * r0;
	let discriminant = b * b - 4.0 * a * c;
	if discriminant < 0.0 {
		return None;
	}
	let roots = [
		(-b - discriminant.sqrt()) / (2.0 * a),
		(-b + discriminant.sqrt()) / (2.0 * a),
	];
	let mut t = roots
		.into_iter()
		.filter(|root| r0 + root * dr >= -1e-8)
		.reduce(f64::max)?;
	let mut stops = stops.clone();
	// A stable sort, as Python's.
	stops.sort_by(|p, q| p.0.total_cmp(&q.0));
	let (first, last) = (stops[0].0, stops[stops.len() - 1].0);
	let width = last - first;
	if *extend == 1 {
		// REPEAT
		t = first + (t - first).rem_euclid(width);
	} else if *extend == 2 {
		// REFLECT
		let q = (t - first).rem_euclid(2.0 * width);
		t = first + if q <= width { q } else { 2.0 * width - q };
	}
	if t < first {
		return Some(stops[0].1);
	}
	for pair in stops.windows(2) {
		let (left, right) = (pair[0], pair[1]);
		if t < right.0 {
			let q = (t - left.0) / (right.0 - left.0);
			return Some(left.1 + q * (right.1 - left.1));
		}
	}
	Some(stops[stops.len() - 1].1)
}

/// `_negative_radial_font`.
fn negative_radial_font(
	font: &mut Font,
	radii: (i64, i64),
	extend: u8,
	middle: f64,
) -> TestResult<Font> {
	let paint = Paint::Format7(ot::PaintFormat7 {
		color_line: var_color_line(extend, &[(0.0, 0.25), (middle, 0.5), (1.0, 1.0)]),
		x0: 10,
		y0: 15,
		r0: 100,
		x1: 410,
		y1: 215,
		r1: 100,
		var_index_base: 0,
	});
	let store = wght_store(&[0, 0, radii.0 - 100, 0, 0, radii.1 - 100])?;
	set_colr(font, paint, Vec::new(), store, None)?;
	roundtrip(font)
}

fn with_radii(paint: &Painted, radii: (i64, i64)) -> Painted {
	let mut paint = paint.clone();
	if let Painted::Radial { r0, r1, .. } = &mut paint {
		*r0 = radii.0 as f64;
		*r1 = radii.1 as f64;
	}
	paint
}

fn assert_same_radial_alpha(actual: &Painted, expected: &Painted, xs: &[f64], ys: &[f64]) {
	for &x in xs {
		for &y in ys {
			match (radial_alpha(actual, x, y), radial_alpha(expected, x, y)) {
				(None, None) => {}
				(Some(a), Some(e)) => assert!(approx(a, e), "({x}, {y}): {a}, {e}"),
				(a, e) => panic!("({x}, {y}): {a:?}, {e:?}"),
			}
		}
	}
}

/// That instancing each axis in turn gives the same table as `result`.
fn assert_composes(font: &Font, result: &mut Font) -> TestResult {
	for (pinned, rest) in [
		(("wght", pin(900.0)), ("wdth", pin(100.0))),
		(("wdth", pin(100.0)), ("wght", pin(900.0))),
	] {
		let partial = instance(font, &[pinned])?;
		let mut composed = instance(&partial, &[rest])?;
		assert_eq!(composed.table_data(COLR)?, result.table_data(COLR)?);
	}
	Ok(())
}

#[test]
fn test_full_negative_radial_preserves_geometry_and_color_line() -> TestResult {
	for radii in [
		(-100, 500),
		(500, -100),
		(-100, -200),
		(-200, -100),
		(-100, -100),
	] {
		// pad, repeat, reflect
		for extend in [0, 1, 2] {
			for unordered in [false, true] {
				let mut font = negative_radial_font(&mut font_fixture()?, radii, extend, 0.5)?;
				if unordered {
					let mut paints = paints(&mut font)?;
					let Paint::Format7(paint) = &mut *paints[0] else {
						return Err("not a PaintVarRadialGradient".into());
					};
					let line = paint.color_line.as_mut().ok_or("no ColorLine")?;
					line.color_stop.rotate_right(1);
				}
				let expected = with_radii(&painted(paints(&mut font)?[0])?, radii);
				let mut result = instance(&font, &[("wght", pin(900.0)), ("wdth", pin(100.0))])?;
				assert!(table(&mut result)?.var_store.is_none());
				let actual = paints(&mut result)?[0].clone();
				match &actual {
					// The radii of the format can't be negative.
					Paint::Format6(_) => {}
					Paint::Format2(p) => {
						assert_eq!(radii.0, radii.1);
						assert_eq!(p.alpha, 0.0);
					}
					p => panic!("{p:?}"),
				}
				assert_same_radial_alpha(
					&painted(&actual)?,
					&expected,
					&[-2000.0, -1000.0, -200.0, 0.0, 100.0, 400.0, 800.0, 1500.0],
					&[-1200.0, -100.0, 0.0, 100.0, 300.0, 1500.0],
				);
				assert_composes(&font, &mut result)?;
			}
		}
	}
	Ok(())
}

#[test]
fn test_full_negative_radial_preserves_fractional_stop_precision() -> TestResult {
	let radii = (500, -100);
	let step = 1.0 / 16384.0;
	let mut font = negative_radial_font(&mut font_fixture()?, radii, 0, step)?;
	let mut result = instance(&font, &[("wght", pin(900.0)), ("wdth", pin(100.0))])?;
	let actual = paints(&mut result)?[0].clone();
	let Paint::Format6(paint) = &actual else {
		return Err("not a PaintRadialGradient".into());
	};
	assert_eq!((paint.r0, paint.r1), (650, 50));
	let line = paint.color_line.as_ref().ok_or("no ColorLine")?;
	let offsets: Vec<f64> = line.color_stop.iter().map(|s| s.stop_offset).collect();
	assert_eq!(offsets, [0.25, 0.25 + step, 1.25]);
	let expected = with_radii(&painted(paints(&mut font)?[0])?, radii);
	assert_same_radial_alpha(
		&painted(&actual)?,
		&expected,
		&[-200.0, 0.0, 100.0, 400.0, 800.0],
		&[-100.0, 0.0, 300.0],
	);
	assert_composes(&font, &mut result)
}

#[test]
fn test_negative_radial_keeps_shared_color_lines_unchanged() -> TestResult {
	let mut font = negative_radial_font(&mut font_fixture()?, (-100, 500), 0, 0.5)?;
	let b = font.glyph_id("b")?;
	let mut other = paints(&mut font)?[0].clone();
	let Paint::Format7(p) = &mut other else {
		return Err("not a PaintVarRadialGradient".into());
	};
	p.var_index_base = NO_VARIATION_INDEX;
	table(&mut font)?
		.base_glyph_list
		.as_mut()
		.ok_or("no BaseGlyphList")?
		.base_glyph_paint_record
		.push(ot::BaseGlyphPaintRecord {
			base_glyph: b,
			paint: Some(Box::new(other)),
		});
	let mut result = instance(&font, &[("wght", pin(900.0)), ("wdth", pin(100.0))])?;
	let paints = paints(&mut result)?;
	let offsets = |paint: &Paint| -> TestResult<Vec<f64>> {
		let Paint::Format6(paint) = paint else {
			return Err(format!("not a PaintRadialGradient: {paint:?}").into());
		};
		let line = paint.color_line.as_ref().ok_or("no ColorLine")?;
		Ok(line.color_stop.iter().map(|s| s.stop_offset).collect())
	};
	assert_eq!(offsets(paints[0])?, [-1.0, -0.5, 0.0]);
	assert_eq!(offsets(paints[1])?, [0.0, 0.5, 1.0]);
	Ok(())
}

#[test]
fn test_clip_edges_round_outward_in_full_and_composed_instances() -> TestResult {
	for deltas in [
		[-1, -1, 1, 1],
		[1, 1, -1, -1],
		[1, -1, 1, -1],
		[-1, 1, -1, 1],
	] {
		for weight in [400.0, 525.0, 650.0, 775.0, 900.0] {
			let mut font = font_fixture()?;
			let paint = Paint::Format2(ot::PaintFormat2 {
				palette_index: 0,
				alpha: 1.0,
			});
			set_colr(
				&mut font,
				paint,
				Vec::new(),
				wght_store(&deltas)?,
				Some(ClipBox::Format2(ot::ClipBoxFormat2 {
					x_min: 50,
					y_min: 50,
					x_max: 500,
					y_max: 500,
					var_index_base: 0,
				})),
			)?;
			let font = roundtrip(&mut font)?;
			let a = font.clone().glyph_id("a")?;
			let scalar = (weight - 400.0) / 500.0;
			let edge = |base: f64, delta: i64| base + scalar * delta as f64;
			let expected = (
				edge(50.0, deltas[0]).floor() as i16,
				edge(50.0, deltas[1]).floor() as i16,
				edge(500.0, deltas[2]).ceil() as i16,
				edge(500.0, deltas[3]).ceil() as i16,
			);
			let wght = ("wght", pin(weight));
			let wdth = ("wdth", pin(100.0));
			let steps: [(
				&[(&str, Option<AxisTriple>)],
				Vec<(&str, Option<AxisTriple>)>,
			); 3] = [
				(&[], vec![wght, wdth]),
				(&[wght], vec![wdth]),
				(&[wdth], vec![wght]),
			];
			for (partial_limits, final_limits) in &steps {
				let partial = if partial_limits.is_empty() {
					font.clone()
				} else {
					instance(&font, partial_limits)?
				};
				let mut result = instance(&partial, final_limits)?;
				let colr = table(&mut result)?;
				let clips = &colr.clip_list.as_ref().ok_or("no ClipList")?.clips;
				let ClipBox::Format1(clip) = &clips[&a] else {
					return Err("not a format 1 clip box".into());
				};
				assert_eq!(
					(clip.x_min, clip.y_min, clip.x_max, clip.y_max),
					expected,
					"{deltas:?}, {weight}"
				);
			}
		}
	}
	Ok(())
}
