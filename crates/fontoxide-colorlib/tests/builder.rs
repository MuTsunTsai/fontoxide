//! `Tests/colorLib/builder_test.py`.
//!
//! fontTools keeps glyph names in the tables; here they are glyph IDs of a
//! glyph order with all the names the tests use, and the attributes of the
//! built paints are checked through their unbuilt dicts.

#![recursion_limit = "512"]

mod common;

use std::collections::HashMap;

use common::*;
use fontoxide_colorlib::builder::{
	DEFAULT_ALPHA, build_palette_labels, split_color_glyphs_by_version,
};
use fontoxide_colorlib::geometry::{Circle, round_start_circle_stable_containment};
use fontoxide_colorlib::table_builder::{NoCallbacks, TableUnbuilder};
use fontoxide_colorlib::{
	BuildColrOptions, ColorPaletteType, CompositeMode, Error, ExtendMode, FromTable, GlyphIds,
	LayerListBuilder, LocalizedString, PaintFormat as F, Result, Table, TableRef, Value,
	build_colr, build_colr_v1, build_cpal, value,
};
use fontoxide_ttlib::GlyphOrder;
use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types::{self as ot, Paint};
use fontoxide_ttlib::tables::cpal::Cpal;
use fontoxide_ttlib::tables::name::Name;
use indexmap::IndexMap;

fn glyph_order() -> GlyphOrder {
	let mut names: Vec<String> = [
		"a", "b", "c", "d", "e", "f", "g", "h", "i", "bb", "back", "a_fore", "b_fore", "back1",
		"back2", "b_back", "c_back", "a.color0", "a.color1", "b.color0", "b.color1",
	]
	.iter()
	.map(|s| (*s).to_owned())
	.collect();
	names.extend((0..364).map(|i| format!("glyph{i}")));
	let names: Vec<&str> = names.iter().map(String::as_str).collect();
	order(&names)
}

fn glyphs() -> GlyphIds {
	ids(&glyph_order())
}

fn gid(name: &str) -> u32 {
	glyphs()[name]
}

/// `_build(cls, source)`.
fn build<T: FromTable>(source: Value) -> Result<T> {
	let glyphs = glyphs();
	let mut builder = LayerListBuilder::new(&glyphs, false, true);
	builder.table_builder().build_as::<T>(&mut builder, source)
}

/// `_buildPaint(source)`.
fn build_paint(source: Value) -> Result<Paint> {
	let glyphs = glyphs();
	LayerListBuilder::new(&glyphs, false, true).build_paint(source)
}

/// The attributes of a paint, as a dict.
fn view(paint: &Paint) -> Result<Value> {
	attrs(paint, &glyph_order())
}

/// The attributes of a table, as a dict.
fn view_table(table: TableRef<'_>) -> Result<Value> {
	TableUnbuilder::new(&glyph_order()).unbuild(table, &mut NoCallbacks)
}

fn format(paint: &Paint) -> u16 {
	paint.format().unwrap_or(0)
}

fn map(value: Value) -> IndexMap<String, Value> {
	match value {
		Value::Map(map) => map,
		other => panic!("not a dict: {other:?}"),
	}
}

fn colr_glyphs(value: Value) -> IndexMap<String, Value> {
	map(value)
}

fn msg<T: std::fmt::Debug>(result: Result<T>) -> String {
	match result {
		Ok(v) => panic!("expected an error, got {v:?}"),
		Err(e) => e.to_string(),
	}
}

#[test]
fn test_build_colr_v0() -> Result<()> {
	let color_layer_lists = colr_glyphs(value!({
		"a": [("a.color0", 0), ("a.color1", 1)],
		"b": [("b.color1", 1), ("b.color0", 0)],
	}));

	let colr = build_colr(color_layer_lists, &glyphs(), BuildColrOptions::default())?;

	assert_eq!(colr.version, 0);
	let layers = colr.color_layers.as_ref().map(|l| &l[&gid("a")]);
	let layers = layers.map(|l| l.iter().map(|r| (r.glyph, r.color_id)).collect::<Vec<_>>());
	assert_eq!(
		layers,
		Some(vec![(gid("a.color0"), 0), (gid("a.color1"), 1)])
	);
	let layers = colr.color_layers.as_ref().map(|l| &l[&gid("b")]);
	let layers = layers.map(|l| l.iter().map(|r| (r.glyph, r.color_id)).collect::<Vec<_>>());
	assert_eq!(
		layers,
		Some(vec![(gid("b.color1"), 1), (gid("b.color0"), 0)])
	);
	Ok(())
}

/// When COLRv0 layers are encoded as plist in UFO lib, tuples become lists;
/// either are accepted as individual top-level layers.
/// <https://github.com/googlefonts/ufo2ft/issues/426>
#[test]
fn test_build_colr_v0_layer_as_list() -> Result<()> {
	let color_layer_lists = colr_glyphs(value!({
		"a": [["a.color0", 0], ["a.color1", 1]],
		"b": [["b.color1", 1], ["b.color0", 0]],
	}));

	let colr = build_colr(color_layer_lists, &glyphs(), BuildColrOptions::default())?;

	assert_eq!(colr.version, 0);
	let layers = colr.color_layers.as_ref().map(|l| &l[&gid("b")]);
	let layers = layers.map(|l| l.iter().map(|r| (r.glyph, r.color_id)).collect::<Vec<_>>());
	assert_eq!(
		layers,
		Some(vec![(gid("b.color1"), 1), (gid("b.color0"), 0)])
	);
	Ok(())
}

fn colors(cpal: &Cpal, i: usize) -> Vec<(u8, u8, u8, u8)> {
	cpal.palettes[i]
		.iter()
		.map(|c| (c.blue, c.green, c.red, c.alpha))
		.collect()
}

#[test]
fn test_build_cpal_v0() -> Result<()> {
	let palettes = vec![
		vec![[0.68, 0.20, 0.32, 1.0], [0.45, 0.68, 0.21, 1.0]],
		vec![[0.68, 0.20, 0.32, 0.6], [0.45, 0.68, 0.21, 0.6]],
		vec![[0.68, 0.20, 0.32, 0.3], [0.45, 0.68, 0.21, 0.3]],
	];

	let cpal = build_cpal(&palettes, None, None, None, None)?;

	assert_eq!(cpal.version, 0);
	assert_eq!(cpal.num_palette_entries, 2);

	assert_eq!(cpal.palettes.len(), 3);
	assert_eq!(colors(&cpal, 0), [(82, 51, 173, 255), (54, 173, 115, 255)]);
	assert_eq!(colors(&cpal, 1), [(82, 51, 173, 153), (54, 173, 115, 153)]);
	assert_eq!(colors(&cpal, 2), [(82, 51, 173, 76), (54, 173, 115, 76)]);
	Ok(())
}

#[test]
fn test_build_cpal_palettes_different_lengths() {
	let palettes = vec![
		vec![[1.0, 1.0, 1.0, 1.0]],
		vec![[0.0, 0.0, 0.0, 1.0], [0.5, 0.5, 0.5, 1.0]],
	];
	let error = build_cpal(&palettes, None, None, None, None);
	assert!(matches!(error, Err(Error::ColorLib(_))));
	assert!(msg(error).contains("have different lengths"));
}

fn localized(pairs: &[(&str, &str)]) -> Option<LocalizedString> {
	Some(LocalizedString::Localized(
		pairs
			.iter()
			.map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
			.collect(),
	))
}

fn name_string(name: &Name, i: usize) -> Option<String> {
	name.names[i].to_unicode().ok()
}

#[test]
fn test_build_palette_labels() -> Result<()> {
	let mut name_table = Name::default();

	let name_ids = build_palette_labels(
		&[
			None,
			Some("hi".into()),
			localized(&[("en", "hello"), ("de", "hallo")]),
		],
		&mut name_table,
	)?;

	assert_eq!(name_ids, [0xFFFF, 256, 257]);

	assert_eq!(name_table.names.len(), 3);
	assert_eq!(name_string(&name_table, 0).as_deref(), Some("hi"));
	assert_eq!(name_table.names[0].name_id, 256);

	assert_eq!(name_string(&name_table, 1).as_deref(), Some("hallo"));
	assert_eq!(name_table.names[1].name_id, 257);

	assert_eq!(name_string(&name_table, 2).as_deref(), Some("hello"));
	assert_eq!(name_table.names[2].name_id, 257);
	Ok(())
}

fn sample_palettes() -> Vec<Vec<[f64; 4]>> {
	vec![
		vec![[0.1, 0.2, 0.3, 1.0], [0.4, 0.5, 0.6, 1.0]],
		vec![[0.1, 0.2, 0.3, 0.6], [0.4, 0.5, 0.6, 0.6]],
		vec![[0.1, 0.2, 0.3, 0.3], [0.4, 0.5, 0.6, 0.3]],
	]
}

#[test]
fn test_build_cpal_v1_types_no_labels() -> Result<()> {
	let palettes = sample_palettes();
	let palette_types = [
		ColorPaletteType::USABLE_WITH_LIGHT_BACKGROUND,
		ColorPaletteType::USABLE_WITH_DARK_BACKGROUND,
		ColorPaletteType::USABLE_WITH_LIGHT_BACKGROUND
			| ColorPaletteType::USABLE_WITH_DARK_BACKGROUND,
	];

	let cpal = build_cpal(&palettes, Some(&palette_types), None, None, None)?;

	assert_eq!(cpal.version, 1);
	assert_eq!(cpal.num_palette_entries, 2);
	assert_eq!(cpal.palettes.len(), 3);

	assert_eq!(cpal.palette_types, [1, 2, 3]);
	assert_eq!(cpal.palette_labels, vec![Cpal::NO_NAME_ID; palettes.len()]);
	assert_eq!(cpal.palette_entry_labels, vec![Cpal::NO_NAME_ID; 2]);
	Ok(())
}

#[test]
fn test_build_cpal_v1_labels() -> Result<()> {
	let palettes = sample_palettes();
	let palette_labels = [
		Some("First".into()),
		localized(&[("en", "Second"), ("it", "Seconda")]),
		None,
	];
	let palette_entry_labels = [Some("Foo".into()), Some("Bar".into())];

	let error = build_cpal(&palettes, None, Some(&palette_labels), None, None);
	assert!(matches!(&error, Err(Error::Type(m)) if m.contains("nameTable is required")));
	let error = build_cpal(&palettes, None, None, Some(&palette_entry_labels), None);
	assert!(matches!(&error, Err(Error::Type(m)) if m.contains("nameTable is required")));

	let mut name_table = Name::default();

	let cpal = build_cpal(
		&palettes,
		None,
		Some(&palette_labels),
		Some(&palette_entry_labels),
		Some(&mut name_table),
	)?;

	assert_eq!(cpal.version, 1);
	assert_eq!(cpal.num_palette_entries, 2);
	assert_eq!(cpal.palettes.len(), 3);

	assert_eq!(
		cpal.palette_types,
		vec![Cpal::DEFAULT_PALETTE_TYPE; palettes.len()]
	);
	assert_eq!(cpal.palette_labels, [256, 257, Cpal::NO_NAME_ID]);
	assert_eq!(cpal.palette_entry_labels, [258, 259]);

	assert_eq!(name_table.get_debug_name(256).as_deref(), Some("First"));
	assert_eq!(name_table.get_debug_name(257).as_deref(), Some("Second"));
	assert_eq!(name_table.get_debug_name(258).as_deref(), Some("Foo"));
	assert_eq!(name_table.get_debug_name(259).as_deref(), Some("Bar"));
	Ok(())
}

#[test]
fn test_invalid_color_palette_type() {
	for error in [
		ColorPaletteType::new(-1),
		ColorPaletteType::new(4),
		ColorPaletteType::from_value(&value!("abc")),
	] {
		assert!(
			matches!(&error, Err(Error::Value(m)) if m.contains("not a valid ColorPaletteType"))
		);
	}
}

#[test]
fn test_build_cpal_v1_invalid_args_length() -> Result<()> {
	let palettes = vec![vec![[0.0, 0.0, 0.0, 0.0]], vec![[1.0, 1.0, 1.0, 1.0]]];
	let error = build_cpal(
		&palettes,
		Some(&[ColorPaletteType::new(1)?]),
		None,
		None,
		None,
	);
	assert!(msg(error).contains("Expected 2 paletteTypes, got 1"));

	let mut name = Name::default();
	let error = build_cpal(
		&palettes,
		None,
		Some(&[Some("foo".into())]),
		None,
		Some(&mut name),
	);
	assert!(msg(error).contains("Expected 2 paletteLabels, got 1"));

	let mut name = Name::default();
	let error = build_cpal(&palettes, None, None, Some(&[]), Some(&mut name));
	assert!(msg(error).contains("Expected 1 paletteEntryLabels, got 0"));
	Ok(())
}

/// The first case of fontTools (a color that is not a 4-tuple) cannot be
/// written in Rust; colors are written as floats.
#[test]
fn test_build_cpal_invalid_color() {
	let error = build_cpal(
		&[vec![[0.0, 0.0, 0.0, 0.0]], vec![[1.0, 1.0, -1.0, 2.0]]],
		None,
		None,
		None,
		None,
	);
	assert!(matches!(&error, Err(Error::ColorLib(_))));
	assert!(
		msg(error)
			.contains("palette[1][0] has invalid out-of-range [0..1] color: (1.0, 1.0, -1.0, 2.0)")
	);
}

#[test]
fn test_build_paint_solid() -> Result<()> {
	let p = view(&build_paint(value!((F::PaintSolid, 0)))?)?;
	assert_eq!(at(&p, "Format"), &value!(F::PaintSolid));
	assert_eq!(at(&p, "PaletteIndex"), &value!(0));
	assert_eq!(at(&p, "Alpha"), &value!(1.0));
	Ok(())
}

#[test]
fn test_build_paint_solid_alpha() -> Result<()> {
	let p = view(&build_paint(value!((F::PaintSolid, 1, 0.5)))?)?;
	assert_eq!(at(&p, "Format"), &value!(F::PaintSolid));
	assert_eq!(at(&p, "PaletteIndex"), &value!(1));
	assert_eq!(at(&p, "Alpha"), &value!(0.5));
	Ok(())
}

#[test]
fn test_build_paint_var_solid() -> Result<()> {
	let p = view(&build_paint(value!((F::PaintVarSolid, 3, 0.5, 2)))?)?;
	assert_eq!(at(&p, "Format"), &value!(F::PaintVarSolid));
	assert_eq!(at(&p, "PaletteIndex"), &value!(3));
	assert_eq!(at(&p, "Alpha"), &value!(0.5));
	assert_eq!(at(&p, "VarIndexBase"), &value!(2));
	Ok(())
}

/// `test_buildVarColorStop_DefaultAlpha`, whose first definition (of a
/// `ColorStop`) upstream is shadowed by the second one.
#[test]
fn test_build_color_stop_default_alpha() -> Result<()> {
	let s: ot::ColorStop = build(value!((0.1, 2)))?;
	assert_eq!(s.stop_offset, 0.1);
	assert_eq!(s.palette_index, 2);
	assert_eq!(s.alpha, DEFAULT_ALPHA);
	Ok(())
}

#[test]
fn test_build_var_color_stop_default_alpha() -> Result<()> {
	let s: ot::VarColorStop = build(value!((0.1, 2)))?;
	assert_eq!(s.stop_offset, 0.1);
	assert_eq!(s.palette_index, 2);
	assert_eq!(s.alpha, DEFAULT_ALPHA);
	Ok(())
}

#[test]
fn test_build_color_stop() -> Result<()> {
	let s: ot::ColorStop = build(value!({"StopOffset": 0.2, "PaletteIndex": 3, "Alpha": 0.4}))?;
	assert_eq!(s.stop_offset, 0.2);
	assert_eq!(s.palette_index, 3);
	assert_eq!(s.alpha, 0.4);
	Ok(())
}

#[test]
fn test_build_color_stop_variable() -> Result<()> {
	let s: ot::VarColorStop = build(value!({
		"StopOffset": 0.0,
		"PaletteIndex": 0,
		"Alpha": 0.3,
		"VarIndexBase": 1,
	}))?;
	assert_eq!(s.stop_offset, 0.0);
	assert_eq!(s.palette_index, 0);
	assert_eq!(s.alpha, 0.3);
	assert_eq!(s.var_index_base, 1);
	Ok(())
}

fn stops_of(line: &ot::ColorLine) -> Vec<(f64, u16)> {
	line.color_stop
		.iter()
		.map(|cs| (cs.stop_offset, cs.palette_index))
		.collect()
}

#[test]
fn test_build_color_line_stop_list() -> Result<()> {
	let stops = value!([(0.0, 0), (0.5, 1), (1.0, 2)]);
	let expected = [(0.0, 0), (0.5, 1), (1.0, 2)];

	let cline: ot::ColorLine = build(value!({"ColorStop": stops.clone()}))?;
	assert_eq!(cline.extend, ExtendMode::Pad as u8);
	assert_eq!(cline.stop_count, Some(3));
	assert_eq!(stops_of(&cline), expected);

	let cline: ot::ColorLine = build(value!({"Extend": "pad", "ColorStop": stops.clone()}))?;
	assert_eq!(cline.extend, ExtendMode::Pad as u8);

	let cline: ot::ColorLine =
		build(value!({"ColorStop": stops.clone(), "Extend": ExtendMode::Repeat}))?;
	assert_eq!(cline.extend, ExtendMode::Repeat as u8);

	let cline: ot::ColorLine =
		build(value!({"ColorStop": stops.clone(), "Extend": ExtendMode::Reflect}))?;
	assert_eq!(cline.extend, ExtendMode::Reflect as u8);

	let built: Vec<Value> = stops
		.clone()
		.py_iter()?
		.into_iter()
		.map(|s| build::<ot::ColorStop>(s).map(Value::from))
		.collect::<Result<_>>()?;
	let cline: ot::ColorLine = build(value!({"ColorStop": built}))?;
	assert_eq!(stops_of(&cline), expected);
	Ok(())
}

#[test]
fn test_build_var_color_line_stop_map() -> Result<()> {
	let stops = value!([
		{"StopOffset": 0.0, "PaletteIndex": 0, "Alpha": 0.5, "VarIndexBase": 1},
		{"StopOffset": 1.0, "PaletteIndex": 1, "Alpha": 0.3, "VarIndexBase": 3},
	]);
	let cline: ot::VarColorLine = build(value!({"ColorStop": stops.clone()}))?;
	let built: Vec<Value> = cline
		.color_stop
		.iter()
		.map(|cs| {
			value!({
				"StopOffset": cs.stop_offset,
				"PaletteIndex": cs.palette_index,
				"Alpha": cs.alpha,
				"VarIndexBase": cs.var_index_base,
			})
		})
		.collect();
	assert_eq!(Value::List(built), stops);
	Ok(())
}

fn check_build_affine2x3(variable: bool) -> Result<()> {
	let source = value!((1.5, 0, 0.5, 2.0, 1.0, -3.0));
	let (m, var_index_base) = if variable {
		let m: ot::VarAffine2x3 = build(source)?;
		(
			ot::Affine2x3 {
				xx: m.xx,
				yx: m.yx,
				xy: m.xy,
				yy: m.yy,
				dx: m.dx,
				dy: m.dy,
			},
			Some(m.var_index_base),
		)
	} else {
		(build::<ot::Affine2x3>(source)?, None)
	};
	assert_eq!(m.xx, 1.5);
	assert_eq!(m.yx, 0.0);
	assert_eq!(m.xy, 0.5);
	assert_eq!(m.yy, 2.0);
	assert_eq!(m.dx, 1.0);
	assert_eq!(m.dy, -3.0);
	if variable {
		assert_eq!(var_index_base, Some(0xFFFFFFFF));
	}
	Ok(())
}

#[test]
fn test_build_affine2x3() -> Result<()> {
	check_build_affine2x3(false)
}

#[test]
fn test_build_var_affine2x3() -> Result<()> {
	check_build_affine2x3(true)
}

/// `_sample_stops`: built color stops.
fn sample_stops(variable: bool) -> Result<Vec<Value>> {
	let mut stop_sources = vec![
		value!({"StopOffset": 0.0, "PaletteIndex": 0}),
		value!({"StopOffset": 0.5, "PaletteIndex": 1}),
		value!({"StopOffset": 1.0, "PaletteIndex": 2, "Alpha": 0.8}),
	];
	if variable {
		for (i, src) in (123..).zip(stop_sources.iter_mut()) {
			if let Value::Map(m) = src {
				m.insert("VarIndexBase".into(), value!(i));
			}
		}
	}
	stop_sources
		.into_iter()
		.map(|src| {
			if variable {
				build::<ot::VarColorStop>(src).map(Value::from)
			} else {
				build::<ot::ColorStop>(src).map(Value::from)
			}
		})
		.collect()
}

fn is_around_center(fmt: F) -> bool {
	fmt.py_name().ends_with("AroundCenter")
}

fn is_uniform_scale(fmt: F) -> bool {
	fmt.py_name().contains("ScaleUniform")
}

/// The unbuilt dicts of built tables.
fn view_values(values: &[Value]) -> Result<Value> {
	let mut out = Vec::new();
	for v in values {
		match v {
			Value::Table(t) => out.push(view_table(TableRef::from(t))?),
			other => out.push(other.clone()),
		}
	}
	Ok(Value::List(out))
}

fn insert(source: &mut Value, key: &str, value: Value) {
	if let Value::Map(m) = source {
		m.insert(key.to_owned(), value);
	}
}

fn check_build_paint_linear_gradient(fmt: F) -> Result<()> {
	let variable = fmt.is_variable();
	let color_stops = sample_stops(variable)?;

	let (x0, y0, x1, y1, x2, y2) = (1, 2, 3, 4, 5, 6);
	let mut source = value!({
		"Format": fmt,
		"ColorLine": {"ColorStop": color_stops.clone()},
		"x0": x0,
		"y0": y0,
		"x1": x1,
		"y1": y1,
		"x2": x2,
		"y2": y2,
	});
	if variable {
		insert(&mut source, "VarIndexBase", value!(7));
	}
	let gradient = build_paint(source)?;
	let g = view(&gradient)?;
	assert_eq!(path(&g, &["ColorLine", "Extend"]), &value!("pad"));
	assert_eq!(
		path(&g, &["ColorLine", "ColorStop"]),
		&view_values(&color_stops)?
	);

	let gradient = view(&build_paint(Value::from(gradient))?)?;
	assert_eq!(
		(at(&gradient, "x0"), at(&gradient, "y0")),
		(&value!(1), &value!(2))
	);
	assert_eq!(
		(at(&gradient, "x1"), at(&gradient, "y1")),
		(&value!(3), &value!(4))
	);
	assert_eq!(
		(at(&gradient, "x2"), at(&gradient, "y2")),
		(&value!(5), &value!(6))
	);
	if variable {
		assert_eq!(at(&gradient, "VarIndexBase"), &value!(7));
	}
	Ok(())
}

#[test]
fn test_build_paint_linear_gradient() -> Result<()> {
	assert!(!F::PaintLinearGradient.is_variable());
	check_build_paint_linear_gradient(F::PaintLinearGradient)
}

#[test]
fn test_build_paint_var_linear_gradient() -> Result<()> {
	assert!(F::PaintVarLinearGradient.is_variable());
	check_build_paint_linear_gradient(F::PaintVarLinearGradient)
}

fn check_build_paint_radial_gradient(fmt: F) -> Result<()> {
	let variable = fmt.is_variable();
	let color_stops = sample_stops(variable)?;
	let line_source = value!({"ColorStop": color_stops.clone(), "Extend": ExtendMode::Repeat});
	let color_line = if variable {
		Value::from(build::<ot::VarColorLine>(line_source)?)
	} else {
		Value::from(build::<ot::ColorLine>(line_source)?)
	};
	let (c0, c1, r0, r1, var_index_base) = ((100, 200), (150, 250), 10, 5, 0);

	let mut source = vec![
		value!(fmt),
		color_line.clone(),
		value!(c0.0),
		value!(c0.1),
		value!(r0),
		value!(c1.0),
		value!(c1.1),
		value!(r1),
	];
	if variable {
		source.push(value!(var_index_base));
	}

	let gradient = build::<Paint>(Value::List(source))?;
	assert_eq!(format(&gradient), fmt as u16);
	let g = view(&gradient)?;
	assert_eq!(
		at(&g, "ColorLine"),
		&view_values(&[color_line])?.py_iter()?[0]
	);
	assert_eq!((at(&g, "x0"), at(&g, "y0")), (&value!(c0.0), &value!(c0.1)));
	assert_eq!((at(&g, "x1"), at(&g, "y1")), (&value!(c1.0), &value!(c1.1)));
	assert_eq!(at(&g, "r0"), &value!(r0));
	assert_eq!(at(&g, "r1"), &value!(r1));
	if variable {
		assert_eq!(at(&g, "VarIndexBase"), &value!(var_index_base));
	}

	let mut source = value!({
		"Format": fmt,
		"ColorLine": {"ColorStop": color_stops.clone()},
		"x0": c0.0,
		"y0": c0.1,
		"x1": c1.0,
		"y1": c1.1,
		"r0": r0,
		"r1": r1,
	});
	if variable {
		insert(&mut source, "VarIndexBase", value!(var_index_base));
	}
	let g = view(&build::<Paint>(source)?)?;
	assert_eq!(path(&g, &["ColorLine", "Extend"]), &value!("pad"));
	assert_eq!(
		path(&g, &["ColorLine", "ColorStop"]),
		&view_values(&color_stops)?
	);
	assert_eq!((at(&g, "x0"), at(&g, "y0")), (&value!(c0.0), &value!(c0.1)));
	assert_eq!((at(&g, "x1"), at(&g, "y1")), (&value!(c1.0), &value!(c1.1)));
	assert_eq!(at(&g, "r0"), &value!(r0));
	assert_eq!(at(&g, "r1"), &value!(r1));
	if variable {
		assert_eq!(at(&g, "VarIndexBase"), &value!(var_index_base));
	}
	Ok(())
}

#[test]
fn test_build_paint_radial_gradient() -> Result<()> {
	assert!(!F::PaintRadialGradient.is_variable());
	check_build_paint_radial_gradient(F::PaintRadialGradient)
}

#[test]
fn test_build_paint_var_radial_gradient() -> Result<()> {
	assert!(F::PaintVarRadialGradient.is_variable());
	check_build_paint_radial_gradient(F::PaintVarRadialGradient)
}

fn check_paint_sweep_gradient(fmt: F) -> Result<()> {
	let variable = fmt.is_variable();
	let mut source = value!({
		"Format": fmt,
		"ColorLine": {"ColorStop": sample_stops(variable)?},
		"centerX": 127,
		"centerY": 129,
		"startAngle": 15,
		"endAngle": 42,
	});
	if variable {
		insert(&mut source, "VarIndexBase", value!(666));
	}
	let paint = build_paint(source)?;
	assert_eq!(format(&paint), fmt as u16);
	let p = view(&paint)?;
	assert_eq!(at(&p, "centerX"), &value!(127));
	assert_eq!(at(&p, "centerY"), &value!(129));
	assert_eq!(at(&p, "startAngle"), &value!(15));
	assert_eq!(at(&p, "endAngle"), &value!(42));
	if variable {
		assert_eq!(at(&p, "VarIndexBase"), &value!(666));
	}
	Ok(())
}

#[test]
fn test_build_paint_sweep_gradient() -> Result<()> {
	assert!(!F::PaintSweepGradient.is_variable());
	check_paint_sweep_gradient(F::PaintSweepGradient)
}

#[test]
fn test_build_paint_var_sweep_gradient() -> Result<()> {
	assert!(F::PaintVarSweepGradient.is_variable());
	check_paint_sweep_gradient(F::PaintVarSweepGradient)
}

#[test]
fn test_build_paint_glyph_solid() -> Result<()> {
	let layer = build::<Paint>(value!((F::PaintGlyph, (F::PaintSolid, 2), "a")))?;
	let l = view(&layer)?;
	assert_eq!(format(&layer), F::PaintGlyph as u16);
	assert_eq!(at(&l, "Glyph"), &value!("a"));
	assert_eq!(path(&l, &["Paint", "Format"]), &value!(F::PaintSolid));
	assert_eq!(path(&l, &["Paint", "PaletteIndex"]), &value!(2));

	let layer = build::<Paint>(value!((F::PaintGlyph, (F::PaintSolid, 3, 0.9), "a")))?;
	let l = view(&layer)?;
	assert_eq!(path(&l, &["Paint", "Format"]), &value!(F::PaintSolid));
	assert_eq!(path(&l, &["Paint", "PaletteIndex"]), &value!(3));
	assert_eq!(path(&l, &["Paint", "Alpha"]), &value!(0.9));
	Ok(())
}

#[test]
fn test_build_paint_glyph2_solid() -> Result<()> {
	let layer = build::<Paint>(value!((F::PaintGlyph2, (F::PaintSolid, 2), "a")))?;
	let l = view(&layer)?;
	assert_eq!(format(&layer), F::PaintGlyph2 as u16);
	assert_eq!(at(&l, "Glyph"), &value!("a"));
	assert_eq!(path(&l, &["Paint", "Format"]), &value!(F::PaintSolid));
	Ok(())
}

#[test]
fn test_build_paint_glyph_promotes_high_gid() -> Result<()> {
	let glyph_map: GlyphIds = HashMap::from([("a".to_owned(), 0x10000)]);
	let layer = LayerListBuilder::new(&glyph_map, true, true).build_paint(value!((
		F::PaintGlyph,
		(F::PaintSolid, 2),
		"a"
	)))?;
	assert_eq!(format(&layer), F::PaintGlyph2 as u16);
	Ok(())
}

#[test]
fn test_build_paint_glyph_var_linear_gradient() -> Result<()> {
	let layer = build::<Paint>(value!({
		"Format": F::PaintGlyph,
		"Glyph": "a",
		"Paint": {
			"Format": F::PaintVarLinearGradient,
			"ColorLine": {"ColorStop": [(0.0, 3), (1.0, 4)]},
			"x0": 100,
			"y0": 200,
			"x1": 150,
			"y1": 250,
		},
	}));
	// LENIENT(L32): fontTools leaves x2, y2 unset and fails only when compiling or
	// dumping the paint; here building fails.
	assert!(matches!(&layer, Err(Error::Attribute(m)) if m.contains("'x2'")));

	let layer = build::<Paint>(value!({
		"Format": F::PaintGlyph,
		"Glyph": "a",
		"Paint": {
			"Format": F::PaintVarLinearGradient,
			"ColorLine": {"ColorStop": [(0.0, 3), (1.0, 4)]},
			"x0": 100,
			"y0": 200,
			"x1": 150,
			"y1": 250,
			"x2": 0,
			"y2": 0,
		},
	}))?;
	let l = view(&layer)?;
	assert_eq!(format(&layer), F::PaintGlyph as u16);
	assert_eq!(at(&l, "Glyph"), &value!("a"));
	assert_eq!(
		path(&l, &["Paint", "Format"]),
		&value!(F::PaintVarLinearGradient)
	);
	let stops = path(&l, &["Paint", "ColorLine", "ColorStop"])
		.clone()
		.py_iter()?;
	assert_eq!(at(&stops[0], "StopOffset"), &value!(0.0));
	assert_eq!(at(&stops[0], "PaletteIndex"), &value!(3));
	assert_eq!(at(&stops[1], "StopOffset"), &value!(1.0));
	assert_eq!(at(&stops[1], "PaletteIndex"), &value!(4));
	assert_eq!(path(&l, &["Paint", "x0"]), &value!(100));
	assert_eq!(path(&l, &["Paint", "y0"]), &value!(200));
	assert_eq!(path(&l, &["Paint", "x1"]), &value!(150));
	assert_eq!(path(&l, &["Paint", "y1"]), &value!(250));
	Ok(())
}

#[test]
fn test_build_paint_glyph_radial_gradient() -> Result<()> {
	let layer = build::<Paint>(value!((
		F::PaintGlyph as i64,
		(
			F::PaintRadialGradient,
			(
				"pad",
				[
					(0.0, 5),
					{"StopOffset": 0.5, "PaletteIndex": 6, "Alpha": 0.8},
					(1.0, 7),
				],
			),
			50,
			50,
			30,
			75,
			75,
			10,
		),
		"a",
	)))?;
	let l = view(&layer)?;
	assert_eq!(format(&layer), F::PaintGlyph as u16);
	assert_eq!(
		path(&l, &["Paint", "Format"]),
		&value!(F::PaintRadialGradient)
	);
	let stops = path(&l, &["Paint", "ColorLine", "ColorStop"])
		.clone()
		.py_iter()?;
	assert_eq!(at(&stops[0], "StopOffset"), &value!(0.0));
	assert_eq!(at(&stops[0], "PaletteIndex"), &value!(5));
	assert_eq!(at(&stops[1], "StopOffset"), &value!(0.5));
	assert_eq!(at(&stops[1], "PaletteIndex"), &value!(6));
	assert_eq!(at(&stops[1], "Alpha"), &value!(0.8));
	assert_eq!(at(&stops[2], "StopOffset"), &value!(1.0));
	assert_eq!(at(&stops[2], "PaletteIndex"), &value!(7));
	assert_eq!(path(&l, &["Paint", "x0"]), &value!(50));
	assert_eq!(path(&l, &["Paint", "y0"]), &value!(50));
	assert_eq!(path(&l, &["Paint", "r0"]), &value!(30));
	assert_eq!(path(&l, &["Paint", "x1"]), &value!(75));
	assert_eq!(path(&l, &["Paint", "y1"]), &value!(75));
	assert_eq!(path(&l, &["Paint", "r1"]), &value!(10));
	Ok(())
}

#[test]
fn test_build_paint_glyph_dict_solid() -> Result<()> {
	let layer = build::<Paint>(value!((
		F::PaintGlyph as i64,
		(F::PaintSolid as i64, 1),
		"a"
	)))?;
	let l = view(&layer)?;
	assert_eq!(format(&layer), F::PaintGlyph as u16);
	assert_eq!(at(&l, "Glyph"), &value!("a"));
	assert_eq!(path(&l, &["Paint", "Format"]), &value!(F::PaintSolid));
	assert_eq!(path(&l, &["Paint", "PaletteIndex"]), &value!(1));
	Ok(())
}

#[test]
fn test_build_paint_glyph_dict_var_linear_gradient() -> Result<()> {
	let layer = build::<Paint>(value!({
		"Format": F::PaintGlyph,
		"Glyph": "a",
		"Paint": {
			"Format": F::PaintVarLinearGradient as i64,
			"ColorLine": {"ColorStop": [(0.0, 0), (1.0, 1)]},
			"x0": 0,
			"y0": 0,
			"x1": 10,
			"y1": 10,
			// LENIENT(L32): unset upstream, which only fails when compiling.
			"x2": 0,
			"y2": 0,
		},
	}))?;
	let l = view(&layer)?;
	assert_eq!(format(&layer), F::PaintGlyph as u16);
	assert_eq!(at(&l, "Glyph"), &value!("a"));
	assert_eq!(
		path(&l, &["Paint", "Format"]),
		&value!(F::PaintVarLinearGradient)
	);
	let stops = path(&l, &["Paint", "ColorLine", "ColorStop"])
		.clone()
		.py_iter()?;
	assert_eq!(at(&stops[0], "StopOffset"), &value!(0.0));
	Ok(())
}

#[test]
fn test_build_paint_glyph_dict_radial_gradient() -> Result<()> {
	let layer = build_paint(value!({
		"Glyph": "a",
		"Paint": {
			"Format": F::PaintRadialGradient as i64,
			"ColorLine": {"ColorStop": [(0.0, 0), (1.0, 1)]},
			"x0": 0,
			"y0": 0,
			"r0": 4,
			"x1": 10,
			"y1": 10,
			"r1": 0,
		},
		"Format": F::PaintGlyph as i64,
	}))?;
	let l = view(&layer)?;
	assert_eq!(
		path(&l, &["Paint", "Format"]),
		&value!(F::PaintRadialGradient)
	);
	assert_eq!(path(&l, &["Paint", "r0"]), &value!(4));
	Ok(())
}

#[test]
fn test_build_paint_colr_glyph() -> Result<()> {
	let paint = build_paint(value!((F::PaintColrGlyph as i64, "a")))?;
	assert_eq!(format(&paint), F::PaintColrGlyph as u16);
	assert_eq!(at(&view(&paint)?, "Glyph"), &value!("a"));
	Ok(())
}

fn check_build_paint_transform(fmt: F) -> Result<()> {
	let variable = fmt.is_variable();
	let mut affine_src = vec![
		value!(1),
		value!(2),
		value!(3),
		value!(4),
		value!(5),
		value!(6),
	];
	if variable {
		affine_src.push(value!(7));
	}
	let affine = if variable {
		Value::from(build::<ot::VarAffine2x3>(Value::List(affine_src))?)
	} else {
		Value::from(build::<ot::Affine2x3>(Value::List(affine_src))?)
	};

	let paint = build_paint(value!((
		fmt as i64,
		(F::PaintGlyph, (F::PaintSolid, 0, 1.0), "a"),
		affine,
	)))?;
	let p = view(&paint)?;

	assert_eq!(format(&paint), fmt as u16);
	assert_eq!(path(&p, &["Paint", "Format"]), &value!(F::PaintGlyph));
	assert_eq!(
		path(&p, &["Paint", "Paint", "Format"]),
		&value!(F::PaintSolid)
	);

	assert_eq!(path(&p, &["Transform", "xx"]), &value!(1.0));
	assert_eq!(path(&p, &["Transform", "yx"]), &value!(2.0));
	assert_eq!(path(&p, &["Transform", "xy"]), &value!(3.0));
	assert_eq!(path(&p, &["Transform", "yy"]), &value!(4.0));
	assert_eq!(path(&p, &["Transform", "dx"]), &value!(5.0));
	assert_eq!(path(&p, &["Transform", "dy"]), &value!(6.0));
	if variable {
		assert_eq!(path(&p, &["Transform", "VarIndexBase"]), &value!(7));
	}

	let mut affine_src = vec![
		value!(1),
		value!(2),
		value!(3),
		value!(0.3333),
		value!(10),
		value!(10),
	];
	if variable {
		affine_src.push(value!(456)); // VarIndexBase
	}
	let paint = build::<Paint>(value!({
		"Format": fmt,
		"Transform": Value::List(affine_src),
		"Paint": {
			"Format": F::PaintRadialGradient as i64,
			"ColorLine": {"ColorStop": [(0.0, 0), (1.0, 1)]},
			"x0": 100,
			"y0": 101,
			"x1": 102,
			"y1": 103,
			"r0": 0,
			"r1": 50,
		},
	}))?;
	let p = view(&paint)?;

	assert_eq!(format(&paint), fmt as u16);
	assert_eq!(path(&p, &["Transform", "xx"]), &value!(1.0));
	assert_eq!(path(&p, &["Transform", "yx"]), &value!(2.0));
	assert_eq!(path(&p, &["Transform", "xy"]), &value!(3.0));
	assert_eq!(path(&p, &["Transform", "yy"]), &value!(0.3333));
	assert_eq!(path(&p, &["Transform", "dx"]), &value!(10));
	assert_eq!(path(&p, &["Transform", "dy"]), &value!(10));
	if variable {
		assert_eq!(path(&p, &["Transform", "VarIndexBase"]), &value!(456));
	}
	assert_eq!(
		path(&p, &["Paint", "Format"]),
		&value!(F::PaintRadialGradient)
	);
	Ok(())
}

#[test]
fn test_build_paint_transform() -> Result<()> {
	assert!(!F::PaintTransform.is_variable());
	check_build_paint_transform(F::PaintTransform)
}

#[test]
fn test_build_paint_var_transform() -> Result<()> {
	assert!(F::PaintVarTransform.is_variable());
	check_build_paint_transform(F::PaintVarTransform)
}

#[test]
fn test_build_paint_composite() -> Result<()> {
	let composite = build::<Paint>(value!({
		"Format": F::PaintComposite as i64,
		"CompositeMode": "src_over",
		"SourcePaint": {
			"Format": F::PaintComposite,
			"CompositeMode": "src_over",
			"SourcePaint": {
				"Format": F::PaintGlyph as i64,
				"Glyph": "c",
				"Paint": (F::PaintSolid, 2),
			},
			"BackdropPaint": {
				"Format": F::PaintGlyph as i64,
				"Glyph": "b",
				"Paint": (F::PaintSolid, 1),
			},
		},
		"BackdropPaint": {
			"Format": F::PaintGlyph,
			"Glyph": "a",
			"Paint": {
				"Format": F::PaintSolid,
				"PaletteIndex": 0,
				"Alpha": 0.5,
			},
		},
	}))?;
	let c = view(&composite)?;
	let src_over = value!(CompositeMode::SrcOver.py_name().to_lowercase());

	assert_eq!(format(&composite), F::PaintComposite as u16);
	assert_eq!(
		path(&c, &["SourcePaint", "Format"]),
		&value!(F::PaintComposite)
	);
	assert_eq!(
		path(&c, &["SourcePaint", "SourcePaint", "Format"]),
		&value!(F::PaintGlyph)
	);
	assert_eq!(
		path(&c, &["SourcePaint", "SourcePaint", "Glyph"]),
		&value!("c")
	);
	assert_eq!(
		path(&c, &["SourcePaint", "SourcePaint", "Paint", "Format"]),
		&value!(F::PaintSolid)
	);
	assert_eq!(
		path(&c, &["SourcePaint", "SourcePaint", "Paint", "PaletteIndex"]),
		&value!(2)
	);
	assert_eq!(path(&c, &["SourcePaint", "CompositeMode"]), &src_over);
	assert_eq!(
		path(&c, &["SourcePaint", "BackdropPaint", "Format"]),
		&value!(F::PaintGlyph)
	);
	assert_eq!(
		path(&c, &["SourcePaint", "BackdropPaint", "Glyph"]),
		&value!("b")
	);
	assert_eq!(
		path(&c, &["SourcePaint", "BackdropPaint", "Paint", "Format"]),
		&value!(F::PaintSolid)
	);
	assert_eq!(
		path(
			&c,
			&["SourcePaint", "BackdropPaint", "Paint", "PaletteIndex"]
		),
		&value!(1)
	);
	assert_eq!(path(&c, &["CompositeMode"]), &src_over);
	assert_eq!(
		path(&c, &["BackdropPaint", "Format"]),
		&value!(F::PaintGlyph)
	);
	assert_eq!(path(&c, &["BackdropPaint", "Glyph"]), &value!("a"));
	assert_eq!(
		path(&c, &["BackdropPaint", "Paint", "Format"]),
		&value!(F::PaintSolid)
	);
	assert_eq!(
		path(&c, &["BackdropPaint", "Paint", "PaletteIndex"]),
		&value!(0)
	);
	assert_eq!(path(&c, &["BackdropPaint", "Paint", "Alpha"]), &value!(0.5));
	if let Paint::Format32(t) = &composite {
		assert_eq!(t.composite_mode, CompositeMode::SrcOver as u8);
	}
	Ok(())
}

fn glyph_paint() -> Value {
	value!((F::PaintGlyph, (F::PaintSolid, 0, 1.0), "a"))
}

fn check_build_paint_translate(fmt: F) -> Result<()> {
	let variable = fmt.is_variable();

	let mut source = value!({
		"Format": fmt,
		"Paint": glyph_paint(),
		"dx": 123,
		"dy": -345,
	});
	if variable {
		insert(&mut source, "VarIndexBase", value!(678));
	}

	let paint = build::<Paint>(source)?;
	let p = view(&paint)?;

	assert_eq!(format(&paint), fmt as u16);
	assert_eq!(path(&p, &["Paint", "Format"]), &value!(F::PaintGlyph));
	assert_eq!(at(&p, "dx"), &value!(123));
	assert_eq!(at(&p, "dy"), &value!(-345));
	if variable {
		assert_eq!(at(&p, "VarIndexBase"), &value!(678));
	}
	Ok(())
}

#[test]
fn test_build_paint_translate() -> Result<()> {
	assert!(!F::PaintTranslate.is_variable());
	check_build_paint_translate(F::PaintTranslate)
}

#[test]
fn test_build_paint_var_translate() -> Result<()> {
	assert!(F::PaintVarTranslate.is_variable());
	check_build_paint_translate(F::PaintVarTranslate)
}

fn check_build_paint_scale(fmt: F) -> Result<()> {
	let variable = fmt.is_variable();
	let around_center = is_around_center(fmt);
	let uniform = is_uniform_scale(fmt);

	let mut source = value!({"Format": fmt, "Paint": glyph_paint()});
	if uniform {
		insert(&mut source, "scale", value!(1.5));
	} else {
		insert(&mut source, "scaleX", value!(1.0));
		insert(&mut source, "scaleY", value!(2.0));
	}
	if around_center {
		insert(&mut source, "centerX", value!(127));
		insert(&mut source, "centerY", value!(129));
	}
	if variable {
		insert(&mut source, "VarIndexBase", value!(666));
	}

	let paint = build::<Paint>(source)?;
	let p = view(&paint)?;

	assert_eq!(format(&paint), fmt as u16);
	assert_eq!(path(&p, &["Paint", "Format"]), &value!(F::PaintGlyph));
	if uniform {
		assert_eq!(at(&p, "scale"), &value!(1.5));
	} else {
		assert_eq!(at(&p, "scaleX"), &value!(1.0));
		assert_eq!(at(&p, "scaleY"), &value!(2.0));
	}
	if around_center {
		assert_eq!(at(&p, "centerX"), &value!(127));
		assert_eq!(at(&p, "centerY"), &value!(129));
	}
	if variable {
		assert_eq!(at(&p, "VarIndexBase"), &value!(666));
	}
	Ok(())
}

#[test]
fn test_build_paint_scale() -> Result<()> {
	let f = F::PaintScale;
	assert!(!f.is_variable() && !is_uniform_scale(f) && !is_around_center(f));
	check_build_paint_scale(f)
}

#[test]
fn test_build_paint_var_scale() -> Result<()> {
	let f = F::PaintVarScale;
	assert!(f.is_variable() && !is_uniform_scale(f) && !is_around_center(f));
	check_build_paint_scale(f)
}

#[test]
fn test_build_paint_scale_around_center() -> Result<()> {
	let f = F::PaintScaleAroundCenter;
	assert!(!f.is_variable() && !is_uniform_scale(f) && is_around_center(f));
	check_build_paint_scale(f)
}

#[test]
fn test_build_paint_var_scale_around_center() -> Result<()> {
	let f = F::PaintVarScaleAroundCenter;
	assert!(f.is_variable() && !is_uniform_scale(F::PaintScaleAroundCenter) && is_around_center(f));
	check_build_paint_scale(f)
}

#[test]
fn test_build_paint_scale_uniform() -> Result<()> {
	let f = F::PaintScaleUniform;
	assert!(!f.is_variable() && is_uniform_scale(f) && !is_around_center(f));
	check_build_paint_scale(f)
}

#[test]
fn test_build_paint_var_scale_uniform() -> Result<()> {
	let f = F::PaintVarScaleUniform;
	assert!(f.is_variable() && is_uniform_scale(f) && !is_around_center(f));
	check_build_paint_scale(f)
}

#[test]
fn test_build_paint_scale_uniform_around_center() -> Result<()> {
	let f = F::PaintScaleUniformAroundCenter;
	assert!(!f.is_variable() && is_uniform_scale(f) && is_around_center(f));
	check_build_paint_scale(f)
}

#[test]
fn test_build_paint_var_scale_uniform_around_center() -> Result<()> {
	let f = F::PaintVarScaleUniformAroundCenter;
	assert!(f.is_variable() && is_uniform_scale(f) && is_around_center(f));
	check_build_paint_scale(f)
}

fn check_build_paint_rotate(fmt: F) -> Result<()> {
	let variable = fmt.is_variable();
	let around_center = is_around_center(fmt);

	let mut source = value!({"Format": fmt, "Paint": glyph_paint(), "angle": 15});
	if around_center {
		insert(&mut source, "centerX", value!(127));
		insert(&mut source, "centerY", value!(129));
	}

	let paint = build::<Paint>(source)?;
	let p = view(&paint)?;

	assert_eq!(format(&paint), fmt as u16);
	assert_eq!(path(&p, &["Paint", "Format"]), &value!(F::PaintGlyph));
	assert_eq!(at(&p, "angle"), &value!(15));
	if around_center {
		assert_eq!(at(&p, "centerX"), &value!(127));
		assert_eq!(at(&p, "centerY"), &value!(129));
	}
	if variable {
		assert_eq!(at(&p, "VarIndexBase"), &value!(0xFFFFFFFFu32));
	}
	Ok(())
}

#[test]
fn test_build_paint_rotate() -> Result<()> {
	assert!(!F::PaintRotate.is_variable() && !is_around_center(F::PaintRotate));
	check_build_paint_rotate(F::PaintRotate)
}

#[test]
fn test_build_paint_var_rotate() -> Result<()> {
	assert!(F::PaintVarRotate.is_variable() && !is_around_center(F::PaintVarRotate));
	check_build_paint_rotate(F::PaintVarRotate)
}

#[test]
fn test_build_paint_rotate_around_center() -> Result<()> {
	let f = F::PaintRotateAroundCenter;
	assert!(!f.is_variable() && is_around_center(f));
	check_build_paint_rotate(f)
}

#[test]
fn test_build_paint_var_rotate_around_center() -> Result<()> {
	let f = F::PaintVarRotateAroundCenter;
	assert!(f.is_variable() && is_around_center(f));
	check_build_paint_rotate(f)
}

fn check_build_paint_skew(fmt: F) -> Result<()> {
	let variable = fmt.is_variable();
	let around_center = is_around_center(fmt);

	let mut source = value!({
		"Format": fmt,
		"Paint": glyph_paint(),
		"xSkewAngle": 15,
		"ySkewAngle": 42,
	});
	if around_center {
		insert(&mut source, "centerX", value!(127));
		insert(&mut source, "centerY", value!(129));
	}
	if variable {
		insert(&mut source, "VarIndexBase", value!(0));
	}

	let paint = build::<Paint>(source)?;
	let p = view(&paint)?;

	assert_eq!(format(&paint), fmt as u16);
	assert_eq!(path(&p, &["Paint", "Format"]), &value!(F::PaintGlyph));
	assert_eq!(at(&p, "xSkewAngle"), &value!(15));
	assert_eq!(at(&p, "ySkewAngle"), &value!(42));
	if around_center {
		assert_eq!(at(&p, "centerX"), &value!(127));
		assert_eq!(at(&p, "centerY"), &value!(129));
	}
	if variable {
		assert_eq!(at(&p, "VarIndexBase"), &value!(0));
	}
	Ok(())
}

#[test]
fn test_build_paint_skew() -> Result<()> {
	assert!(!F::PaintSkew.is_variable() && !is_around_center(F::PaintSkew));
	check_build_paint_skew(F::PaintSkew)
}

#[test]
fn test_build_paint_var_skew() -> Result<()> {
	assert!(F::PaintVarSkew.is_variable() && !is_around_center(F::PaintVarSkew));
	check_build_paint_skew(F::PaintVarSkew)
}

#[test]
fn test_build_paint_skew_around_center() -> Result<()> {
	let f = F::PaintSkewAroundCenter;
	assert!(!f.is_variable() && is_around_center(f));
	check_build_paint_skew(f)
}

#[test]
fn test_build_paint_var_skew_around_center() -> Result<()> {
	let f = F::PaintVarSkewAroundCenter;
	assert!(f.is_variable() && is_around_center(f));
	check_build_paint_skew(f)
}

fn base_glyph_names(list: &ot::BaseGlyphList, glyphs: &GlyphIds) -> Vec<String> {
	let names: HashMap<u32, &String> = glyphs.iter().map(|(n, &g)| (g, n)).collect();
	list.base_glyph_paint_record
		.iter()
		.map(|r| names[&r.base_glyph].clone())
		.collect()
}

#[test]
fn test_build_colr_v1() -> Result<()> {
	let color_glyphs = || {
		colr_glyphs(value!({
			"a": (
				F::PaintColrLayers,
				[
					(F::PaintGlyph, (F::PaintSolid, 0), "b"),
					(F::PaintGlyph, (F::PaintVarSolid, 1), "c"),
				],
			),
			"d": (
				F::PaintColrLayers,
				[
					(
						F::PaintGlyph,
						{
							"Format": F::PaintSolid as i64,
							"PaletteIndex": 2,
							"Alpha": 0.8,
						},
						"e",
					),
					(
						F::PaintGlyph,
						{
							"Format": F::PaintVarRadialGradient as i64,
							"ColorLine": {
								"ColorStop": [(0.0, 3), (1.0, 4)],
								"Extend": "reflect",
							},
							"x0": 0,
							"y0": 0,
							"x1": 0,
							"y1": 0,
							"r0": 10,
							"r1": 0,
						},
						"f",
					),
				],
			),
			"g": (F::PaintColrLayers, [(F::PaintGlyph, (F::PaintSolid, 5), "h")]),
		}))
	};
	let glyph_map: GlyphIds = [
		(".notdef", 0),
		("a", 4),
		("b", 3),
		("c", 2),
		("d", 1),
		("e", 5),
		("f", 6),
		("g", 7),
		("h", 8),
	]
	.iter()
	.map(|&(n, g)| (n.to_owned(), g))
	.collect();

	let (_, base_glyphs) = build_colr_v1(color_glyphs(), &glyph_map, true, true)?;
	assert_eq!(base_glyphs.base_glyph_count, Some(3));
	assert_eq!(base_glyph_names(&base_glyphs, &glyph_map), ["d", "a", "g"]);

	let (_, base_glyphs) = build_colr_v1(color_glyphs(), &glyph_map, false, true)?;
	assert_eq!(base_glyphs.base_glyph_count, Some(3));
	assert_eq!(base_glyph_names(&base_glyphs, &glyph_map), ["a", "d", "g"]);
	Ok(())
}

#[test]
fn test_build_colr_v1_more_than_255_paints() -> Result<()> {
	let num_paints = 364;
	let layers: Vec<Value> = (0..num_paints)
		.map(|i| {
			value!({
				"Format": F::PaintGlyph as i64,
				"Paint": (F::PaintSolid, 0),
				"Glyph": format!("glyph{i}"),
			})
		})
		.collect();
	let color_glyphs = colr_glyphs(value!({"a": (F::PaintColrLayers, layers)}));
	let (layers, base_glyphs) = build_colr_v1(color_glyphs, &glyphs(), false, true)?;
	let paints: Vec<&Paint> = layers
		.as_ref()
		.map(|l| l.paint.iter().flatten().map(|p| &**p).collect())
		.unwrap_or_default();

	assert_eq!(paints.len(), num_paints + 1);

	assert!((0..255).all(|i| format(paints[i]) == F::PaintGlyph as u16));

	assert_eq!(
		paints[255],
		&Paint::Format1(ot::PaintFormat1 {
			first_layer_index: 0,
			num_layers: 255
		})
	);

	assert!((256..num_paints + 1).all(|i| format(paints[i]) == F::PaintGlyph as u16));

	assert_eq!(base_glyphs.base_glyph_count, Some(1));
	let rec = &base_glyphs.base_glyph_paint_record[0];
	assert_eq!(rec.base_glyph, gid("a"));
	assert_eq!(
		rec.paint.as_deref(),
		Some(&Paint::Format1(ot::PaintFormat1 {
			first_layer_index: 255,
			num_layers: (num_paints + 1 - 255) as u8,
		}))
	);
	Ok(())
}

#[test]
fn test_split_color_glyphs_by_version() -> Result<()> {
	let color_glyphs = colr_glyphs(value!({"a": [("b", 0), ("c", 1), ("d", 2), ("e", 3)]}));

	let (v0, v1) = split_color_glyphs_by_version(color_glyphs)?;

	let pairs = |p: &[(&str, i64)]| {
		p.iter()
			.map(|&(g, i)| (g.to_owned(), i))
			.collect::<Vec<_>>()
	};
	assert_eq!(
		v0,
		IndexMap::from([(
			"a".to_owned(),
			pairs(&[("b", 0), ("c", 1), ("d", 2), ("e", 3)])
		)])
	);
	assert!(v1.is_empty());

	let color_glyphs = colr_glyphs(value!({"a": (F::PaintGlyph, 0, "b")}));

	let (v0, v1) = split_color_glyphs_by_version(color_glyphs.clone())?;

	assert!(v0.is_empty());
	assert_eq!(Value::Map(v1), Value::Map(color_glyphs));

	let color_glyphs = colr_glyphs(value!({
		"a": [("b", 0)],
		"c": [
			("d", 1),
			(
				"e",
				{
					"format": 3,
					"colorLine": {"stops": [(0.0, 2), (1.0, 3)]},
					"p0": (0, 0),
					"p1": (10, 10),
				},
			),
		],
	}));

	let (v0, v1) = split_color_glyphs_by_version(color_glyphs)?;

	assert_eq!(v0, IndexMap::from([("a".to_owned(), pairs(&[("b", 0)]))]));
	assert!(!v1.contains_key("a"));
	assert!(v1.contains_key("c"));
	assert_eq!(v1["c"].clone().py_iter()?.len(), 2);
	Ok(())
}

fn assert_is_colr_v1(colr: &fontoxide_ttlib::tables::colr::Colr) -> &ot::COLR {
	assert_eq!(colr.version, 1);
	assert!(colr.color_layers.is_none());
	colr.table.as_ref().unwrap_or_else(|| panic!("no table"))
}

fn assert_no_v0_content(table: &ot::COLR) {
	assert_eq!(table.base_glyph_record_count, Some(0));
	assert!(table.base_glyph_record_array.is_none());
	assert_eq!(table.layer_record_count, Some(0));
	assert!(table.layer_record_array.is_none());
}

fn version(v: i64) -> BuildColrOptions {
	BuildColrOptions {
		version: Some(v),
		..BuildColrOptions::default()
	}
}

#[test]
fn test_build_layerv1list_empty() -> Result<()> {
	// Nobody uses PaintColrLayers, no layer list.
	let colr = build_colr(
		colr_glyphs(value!({
			// BaseGlyph, tuple form
			"a": (F::PaintGlyph as i64, (F::PaintSolid as i64, 2, 0.8), "b"),
			// BaseGlyph, map form
			"b": {
				"Format": F::PaintGlyph as i64,
				"Paint": {
					"Format": F::PaintLinearGradient as i64,
					"ColorLine": {
						"ColorStop": [(0.0, 2), (1.0, 3)],
						"Extend": "reflect",
					},
					"x0": 1,
					"y0": 2,
					"x1": 3,
					"y1": 4,
					"x2": 2,
					"y2": 2,
				},
				"Glyph": "bb",
			},
		})),
		&glyphs(),
		version(1),
	)?;

	let table = assert_is_colr_v1(&colr);
	assert_no_v0_content(table);

	// 2 v1 glyphs, none in LayerList.
	let list = table.base_glyph_list.as_deref();
	assert_eq!(list.and_then(|l| l.base_glyph_count), Some(2));
	assert_eq!(list.map(|l| l.base_glyph_paint_record.len()), Some(2));
	assert!(table.layer_list.is_none());
	Ok(())
}

/// `_paint_names`: a predictable string from a paint list to enable
/// semi-readable assertions on a layer list order.
fn paint_names<'p>(paints: impl IntoIterator<Item = &'p Paint>) -> Vec<String> {
	let order = glyph_order();
	let mut result = Vec::new();
	for paint in paints {
		match paint {
			Paint::Format10(t) => result.push(order.name(t.glyph).into_owned()),
			Paint::Format1(t) => result.push(format!(
				"Layers[{}:{}]",
				t.first_layer_index,
				t.first_layer_index + u32::from(t.num_layers)
			)),
			_ => {}
		}
	}
	result
}

fn layer_paints(table: &ot::COLR) -> Vec<&Paint> {
	table
		.layer_list
		.as_ref()
		.map(|l| l.paint.iter().flatten().map(|p| &**p).collect())
		.unwrap_or_default()
}

fn base_paints(table: &ot::COLR) -> Vec<&Paint> {
	table
		.base_glyph_list
		.as_ref()
		.map(|l| {
			l.base_glyph_paint_record
				.iter()
				.filter_map(|r| r.paint.as_deref())
				.collect()
		})
		.unwrap_or_default()
}

#[test]
fn test_build_layerv1list_simple() -> Result<()> {
	// Two colr glyphs, each with two layers the first of which is common. All
	// layers use the same solid paint.
	let solid_paint = value!({"Format": F::PaintSolid as i64, "PaletteIndex": 2, "Alpha": 0.8});
	let backdrop =
		value!({"Format": F::PaintGlyph as i64, "Paint": solid_paint.clone(), "Glyph": "back"});
	let a_foreground =
		value!({"Format": F::PaintGlyph as i64, "Paint": solid_paint.clone(), "Glyph": "a_fore"});
	let b_foreground =
		value!({"Format": F::PaintGlyph as i64, "Paint": solid_paint.clone(), "Glyph": "b_fore"});

	// list => PaintColrLayers, contents should land in LayerList.
	let colr = build_colr(
		colr_glyphs(value!({
			"a": (F::PaintColrLayers, [backdrop.clone(), a_foreground]),
			"b": {
				"Format": F::PaintColrLayers,
				"Layers": [backdrop, b_foreground],
			},
		})),
		&glyphs(),
		version(1),
	)?;

	let table = assert_is_colr_v1(&colr);
	assert_no_v0_content(table);

	// 2 v1 glyphs, 4 paints in LayerList. A single shared backdrop isn't worth
	// accessing by slice.
	let list = table.base_glyph_list.as_deref();
	assert_eq!(list.and_then(|l| l.base_glyph_count), Some(2));
	assert_eq!(list.map(|l| l.base_glyph_paint_record.len()), Some(2));
	assert_eq!(
		table.layer_list.as_ref().and_then(|l| l.layer_count),
		Some(4)
	);
	assert_eq!(
		paint_names(layer_paints(table)),
		["back", "a_fore", "back", "b_fore"]
	);
	Ok(())
}

#[test]
fn test_build_layerv1list_with_sharing() -> Result<()> {
	// Three colr glyphs, each with two layers in common.
	let solid_paint = value!({"Format": F::PaintSolid as i64, "PaletteIndex": 2, "Alpha": 0.8});
	let glyph = |format: Value, name: &str| value!({"Format": format, "Paint": solid_paint.clone(), "Glyph": name});
	let backdrop = vec![
		glyph(value!(F::PaintGlyph as i64), "back1"),
		glyph(value!(F::PaintGlyph), "back2"),
	];
	let a_foreground = glyph(value!(F::PaintGlyph), "a_fore");
	let b_background = glyph(value!(F::PaintGlyph), "b_back");
	let b_foreground = glyph(value!(F::PaintGlyph), "b_fore");
	let c_background = glyph(value!(F::PaintGlyph), "c_back");

	let concat = |parts: &[&[Value]]| Value::List(parts.concat());

	// list => PaintColrLayers, which means contents should be in LayerList.
	let colr = build_colr(
		colr_glyphs(value!({
			"a": (F::PaintColrLayers, concat(&[&backdrop, &[a_foreground]])),
			"b": (
				F::PaintColrLayers,
				concat(&[&[b_background], &backdrop, &[b_foreground]]),
			),
			"c": (F::PaintColrLayers, concat(&[&[c_background], &backdrop])),
		})),
		&glyphs(),
		version(1),
	)?;

	let table = assert_is_colr_v1(&colr);
	assert_no_v0_content(table);

	let list = table.base_glyph_list.as_deref();
	assert_eq!(list.and_then(|l| l.base_glyph_count), Some(3));
	assert_eq!(list.map(|l| l.base_glyph_paint_record.len()), Some(3));
	assert_eq!(
		paint_names(base_paints(table)),
		["Layers[0:3]", "Layers[3:6]", "Layers[6:8]"]
	);
	assert_eq!(
		paint_names(layer_paints(table)),
		[
			"back1",
			"back2",
			"a_fore",
			"b_back",
			"Layers[0:2]",
			"b_fore",
			"c_back",
			"Layers[0:2]"
		]
	);
	assert_eq!(
		table.layer_list.as_ref().and_then(|l| l.layer_count),
		Some(8)
	);
	Ok(())
}

#[test]
fn test_build_layerv1list_with_overlaps() -> Result<()> {
	let paints: Vec<Value> = "abcdefghi"
		.chars()
		.map(|c| {
			value!({
				"Format": F::PaintGlyph,
				"Paint": {"Format": F::PaintSolid, "PaletteIndex": 2, "Alpha": 0.8},
				"Glyph": c.to_string(),
			})
		})
		.collect();

	// list => PaintColrLayers, which means contents should be in LayerList.
	let colr = build_colr(
		colr_glyphs(value!({
			"a": (F::PaintColrLayers, paints[0..4].to_vec()),
			"b": (F::PaintColrLayers, paints[0..6].to_vec()),
			"c": (F::PaintColrLayers, paints[2..8].to_vec()),
		})),
		&glyphs(),
		version(1),
	)?;

	let table = assert_is_colr_v1(&colr);
	assert_no_v0_content(table);

	assert_eq!(
		paint_names(layer_paints(table)),
		[
			"a",
			"b",
			"c",
			"d",
			"Layers[0:4]",
			"e",
			"f",
			"Layers[2:4]",
			"Layers[5:7]",
			"g",
			"h"
		]
	);
	assert_eq!(
		paint_names(base_paints(table)),
		["Layers[0:4]", "Layers[4:7]", "Layers[7:11]"]
	);
	assert_eq!(
		table.layer_list.as_ref().and_then(|l| l.layer_count),
		Some(11)
	);
	Ok(())
}

fn two_layers() -> Value {
	value!((
		F::PaintColrLayers,
		[
			(F::PaintGlyph, (F::PaintSolid, 0), "b"),
			(F::PaintGlyph, (F::PaintSolid, 1), "c"),
		],
	))
}

/// `test_explicit_version_1`, and `BuildCOLRTest::test_explicit_version_1`.
#[test]
fn test_explicit_version_1() -> Result<()> {
	let colr = build_colr(
		colr_glyphs(value!({"a": two_layers()})),
		&glyphs(),
		version(1),
	)?;
	assert_eq!(colr.version, 1);
	assert!(colr.color_layers.is_none());
	let table = assert_is_colr_v1(&colr);
	assert!(table.var_store.is_none());
	Ok(())
}

/// `BuildCOLRTest::test_automatic_version_all_solid_color_glyphs`.
#[test]
fn test_automatic_version_all_solid_color_glyphs() -> Result<()> {
	let colr = build_colr(
		colr_glyphs(value!({"a": [("b", 0), ("c", 1)]})),
		&glyphs(),
		BuildColrOptions::default(),
	)?;
	assert_eq!(colr.version, 0);
	let layers = colr.color_layers.as_ref().map(|l| &l[&gid("a")]);
	assert_eq!(layers.map(|l| l[0].glyph), Some(gid("b")));
	assert_eq!(layers.map(|l| l[1].glyph), Some(gid("c")));
	Ok(())
}

/// `BuildCOLRTest::test_automatic_version_no_solid_color_glyphs`.
#[test]
fn test_automatic_version_no_solid_color_glyphs() -> Result<()> {
	let colr = build_colr(
		colr_glyphs(value!({
			"a": (
				F::PaintColrLayers,
				[
					(
						F::PaintGlyph,
						{
							"Format": F::PaintRadialGradient as i64,
							"ColorLine": {
								"ColorStop": [(0.0, 0), (1.0, 1)],
								"Extend": "repeat",
							},
							"x0": 1,
							"y0": 0,
							"x1": 10,
							"y1": 0,
							"r0": 4,
							"r1": 2,
						},
						"b",
					),
					(
						F::PaintGlyph,
						{"Format": F::PaintSolid, "PaletteIndex": 2, "Alpha": 0.8},
						"c",
					),
				],
			),
			"d": (
				F::PaintColrLayers,
				[
					{
						"Format": F::PaintGlyph,
						"Glyph": "e",
						"Paint": {
							"Format": F::PaintLinearGradient,
							"ColorLine": {
								"ColorStop": [(0.0, 2), (1.0, 3)],
								"Extend": "reflect",
							},
							"x0": 1,
							"y0": 2,
							"x1": 3,
							"y1": 4,
							"x2": 2,
							"y2": 2,
						},
					},
				],
			),
		})),
		&glyphs(),
		BuildColrOptions::default(),
	)?;
	let table = assert_is_colr_v1(&colr);
	assert_no_v0_content(table);
	Ok(())
}

/// `BuildCOLRTest::test_automatic_version_mixed_solid_and_gradient_glyphs`.
#[test]
fn test_automatic_version_mixed_solid_and_gradient_glyphs() -> Result<()> {
	let colr = build_colr(
		colr_glyphs(value!({
			"a": [("b", 0), ("c", 1)],
			"d": (
				F::PaintColrLayers,
				[
					(
						F::PaintGlyph,
						{
							"Format": F::PaintLinearGradient,
							"ColorLine": {"ColorStop": [(0.0, 2), (1.0, 3)]},
							"x0": 1,
							"y0": 2,
							"x1": 3,
							"y1": 4,
							"x2": 2,
							"y2": 2,
						},
						"e",
					),
					(F::PaintGlyph, (F::PaintSolid, 2, 0.8), "f"),
				],
			),
		})),
		&glyphs(),
		BuildColrOptions::default(),
	)?;
	let table = assert_is_colr_v1(&colr);
	assert!(table.var_store.is_none());

	assert_eq!(table.base_glyph_record_count, Some(1));
	assert!(table.base_glyph_record_array.is_some());
	assert_eq!(table.layer_record_count, Some(2));
	assert!(table.layer_record_array.is_some());

	let list = table.base_glyph_list.as_deref();
	assert_eq!(list.and_then(|l| l.base_glyph_count), Some(1));
	assert_eq!(
		list.map(|l| l.base_glyph_paint_record[0].base_glyph),
		Some(gid("d"))
	);
	assert_eq!(paint_names(layer_paints(table).into_iter().take(1)), ["e"]);
	Ok(())
}

/// `BuildCOLRTest::test_explicit_version_0`.
#[test]
fn test_explicit_version_0() -> Result<()> {
	let colr = build_colr(
		colr_glyphs(value!({"a": [("b", 0), ("c", 1)]})),
		&glyphs(),
		version(0),
	)?;
	assert_eq!(colr.version, 0);
	assert!(colr.color_layers.is_some());
	Ok(())
}

/// `BuildCOLRTest::test_paint_one_colr_layers`: a set of one layer flips to
/// just that layer.
#[test]
fn test_paint_one_colr_layers() -> Result<()> {
	let colr = build_colr(
		colr_glyphs(value!({
			"a": (F::PaintColrLayers, [(F::PaintGlyph, (F::PaintSolid, 0), "b")]),
		})),
		&glyphs(),
		BuildColrOptions::default(),
	)?;
	let table = assert_is_colr_v1(&colr);
	assert!(table.layer_list.is_none(), "PaintColrLayers should be gone");
	let list = table.base_glyph_list.as_deref();
	assert_eq!(list.and_then(|l| l.base_glyph_count), Some(1));
	let paint = base_paints(table)[0];
	assert_eq!(format(paint), F::PaintGlyph as u16);
	assert_eq!(
		path(&view(paint)?, &["Paint", "Format"]),
		&value!(F::PaintSolid)
	);
	Ok(())
}

fn as_tuple(clip_box: &ot::ClipBox) -> Vec<i64> {
	match clip_box {
		ot::ClipBox::Format1(b) => vec![
			b.x_min.into(),
			b.y_min.into(),
			b.x_max.into(),
			b.y_max.into(),
		],
		ot::ClipBox::Format2(b) => vec![
			b.x_min.into(),
			b.y_min.into(),
			b.x_max.into(),
			b.y_max.into(),
			b.var_index_base.into(),
		],
		ot::ClipBox::Unknown(_) => Vec::new(),
	}
}

/// `BuildCOLRTest::test_build_clip_list`.
#[test]
fn test_build_clip_list() -> Result<()> {
	let colr = build_colr(
		colr_glyphs(value!({
			"a": (F::PaintGlyph, (F::PaintSolid, 0), "b"),
			"c": (F::PaintGlyph, (F::PaintSolid, 1), "d"),
		})),
		&glyphs(),
		BuildColrOptions {
			clip_boxes: Some(map(value!({
				"a": (0, 0, 1000, 1000, 0), // optional 5th: varIndexBase
				"c": (-100.8, -200.4, 1100.1, 1200.5), // floats get rounded
				"e": (0, 0, 10, 10), // 'e' does _not_ get ignored despite being missing
			}))),
			..BuildColrOptions::default()
		},
	)?;

	let table = assert_is_colr_v1(&colr);
	let clip_list = table
		.clip_list
		.as_deref()
		.unwrap_or_else(|| panic!("no clip list"));
	assert_eq!(clip_list.format, Some(1));
	let clips: Vec<(u32, Vec<i64>)> = clip_list
		.clips
		.iter()
		.map(|(&g, b)| (g, as_tuple(b)))
		.collect();
	assert_eq!(
		clips,
		[
			(gid("a"), vec![0, 0, 1000, 1000, 0]),
			(gid("c"), vec![-101, -201, 1101, 1201]),
			(gid("e"), vec![0, 0, 10, 10]),
		]
	);
	assert_eq!(clip_list.clips[&gid("a")].format(), Some(2));
	assert_eq!(clip_list.clips[&gid("c")].format(), Some(1));
	assert_eq!(clip_list.clips[&gid("e")].format(), Some(1));
	Ok(())
}

/// `BuildCOLRTest::test_duplicate_base_glyphs`: base glyphs with equivalent
/// lists of layers share the same PaintColrLayers.
#[test]
fn test_duplicate_base_glyphs() -> Result<()> {
	let layers = value!({
		"Format": F::PaintColrLayers,
		"Layers": [
			(F::PaintGlyph, (F::PaintSolid, 0), "d"),
			(F::PaintGlyph, (F::PaintSolid, 1), "e"),
		],
	});
	let colr = build_colr(
		colr_glyphs(value!({"a": layers.clone(), "b": layers.clone(), "c": layers})),
		&glyphs(),
		BuildColrOptions::default(),
	)?;
	let table = assert_is_colr_v1(&colr);

	let records = table
		.base_glyph_list
		.as_ref()
		.map(|l| l.base_glyph_paint_record.as_slice())
		.unwrap_or_default();
	assert_eq!(records.len(), 3);

	assert_eq!(records[0].base_glyph, gid("a"));
	assert_eq!(records[1].base_glyph, gid("b"));
	assert_eq!(records[2].base_glyph, gid("c"));

	let expected = Paint::Format1(ot::PaintFormat1 {
		first_layer_index: 0,
		num_layers: 2,
	});
	for record in records {
		assert_eq!(record.paint.as_deref(), Some(&expected));
	}
	Ok(())
}

/// `TrickyRadialGradientTest`.
mod tricky_radial_gradient {
	use super::*;

	fn circle_inside_circle(
		c0: (f64, f64),
		r0: f64,
		c1: (f64, f64),
		r1: f64,
		rounded: bool,
	) -> Result<bool> {
		Ok(if rounded {
			Circle::new(c0, r0)
				.round()?
				.inside(&Circle::new(c1, r1).round()?)
		} else {
			Circle::new(c0, r0).inside(&Circle::new(c1, r1))
		})
	}

	fn round_start_circle(
		c0: (f64, f64),
		r0: f64,
		c1: (f64, f64),
		r1: f64,
		inside: bool,
	) -> Result<((f64, f64), f64)> {
		assert_eq!(circle_inside_circle(c0, r0, c1, r1, false)?, inside);
		assert_ne!(circle_inside_circle(c0, r0, c1, r1, true)?, inside);
		let r = round_start_circle_stable_containment(c0, r0, c1, r1)?;
		assert_eq!(
			circle_inside_circle(r.centre, r.radius, c1, r1, true)?,
			inside
		);
		Ok((r.centre, r.radius))
	}

	/// <https://github.com/googlefonts/picosvg/issues/158>
	#[test]
	fn test_noto_emoji_mosquito_u1f99f() -> Result<()> {
		let c0 = (385.23508, 70.56727999999998);
		let r0 = 0.0;
		let c1 = (642.99108, 104.70327999999995);
		let r1 = 260.0072;
		assert_eq!(
			round_start_circle(c0, r0, c1, r1, true)?,
			((386.0, 71.0), 0.0)
		);
		Ok(())
	}

	/// The radial gradient of noto-emoji's 'SIGNS OF THE HORNS' (1f918_1f3fc):
	/// c0 is inside c1 both before and after rounding.
	#[test]
	fn test_noto_emoji_horns_sign_u1f918_1f3fc() -> Result<()> {
		let c0 = (-437.6789059060543, -2116.9237094478003);
		let r0 = 0.0;
		let c1 = (-488.7330118252256, -1876.5036857045086);
		let r1 = 245.77147821915673;
		assert!(circle_inside_circle(c0, r0, c1, r1, false)?);
		assert!(circle_inside_circle(c0, r0, c1, r1, true)?);
		Ok(())
	}

	#[test]
	fn test_nudge_start_circle_position() -> Result<()> {
		type Case = ((f64, f64), f64, (f64, f64), f64, bool, ((f64, f64), f64));
		let cases: &[Case] = &[
			// inside before round, outside after round
			((1.4, 0.0), 0.0, (2.6, 0.0), 1.3, true, ((2.0, 0.0), 0.0)),
			((1.0, 0.0), 0.6, (2.8, 0.0), 2.45, true, ((2.0, 0.0), 1.0)),
			(
				(6.49, 6.49),
				0.0,
				(0.49, 0.49),
				8.49,
				true,
				((5.0, 5.0), 0.0),
			),
			// outside before round, inside after round
			((0.0, 0.0), 0.0, (2.0, 0.0), 1.5, false, ((-1.0, 0.0), 0.0)),
			((0.0, -0.5), 0.0, (0.0, -2.5), 1.5, false, ((0.0, 1.0), 0.0)),
			// the following ones require two nudges to round correctly
			((0.5, 0.0), 0.0, (9.4, 0.0), 8.8, false, ((-1.0, 0.0), 0.0)),
			((1.5, 1.5), 0.0, (0.49, 0.49), 1.49, true, ((0.0, 0.0), 0.0)),
			// limit case when circle almost exactly overlap
			(
				(0.5000001, 0.0),
				0.5000001,
				(0.499999, 0.0),
				0.4999999,
				true,
				((0.0, 0.0), 0.0),
			),
			// concentrical circles, r0 > r1
			((0.0, 0.0), 1.49, (0.0, 0.0), 1.0, false, ((0.0, 0.0), 2.0)),
		];
		for &(c0, r0, c1, r1, inside, expected) in cases {
			assert_eq!(
				round_start_circle(c0, r0, c1, r1, inside)?,
				expected,
				"{c0:?} {r0} {c1:?} {r1}"
			);
		}
		Ok(())
	}
}

#[test]
fn test_build_colr_missing_base_glyph_in_glyph_map_raises_clear_error() {
	// Pretend the font only contains glyph A.
	let glyph_map: GlyphIds = HashMap::from([("A".to_owned(), 0)]);
	// Base glyph B does not exist in glyphMap.
	let color_glyphs = colr_glyphs(value!({"B": [("A", 0)]}));
	let error = build_colr(
		color_glyphs,
		&glyph_map,
		BuildColrOptions {
			version: Some(0),
			glyph_map: true,
			..BuildColrOptions::default()
		},
	);
	assert!(
		matches!(&error, Err(Error::ColorLib(m)) if m.contains("base glyph(s) not found in glyphMap"))
	);
}

fn v1_with_glyph_map(color_glyphs: Value) -> Result<fontoxide_ttlib::tables::colr::Colr> {
	let glyph_map: GlyphIds = HashMap::from([("A".to_owned(), 0)]);
	build_colr(
		colr_glyphs(color_glyphs),
		&glyph_map,
		BuildColrOptions {
			version: Some(1),
			glyph_map: true,
			..BuildColrOptions::default()
		},
	)
}

#[test]
fn test_build_colr_missing_paint_glyph_in_glyph_map_raises_clear_error() {
	// Base glyph A exists, but its PaintGlyph references glyph "B" which does not.
	let error = v1_with_glyph_map(value!({
		"A": {
			"Format": F::PaintGlyph,
			"Glyph": "B",
			"Paint": {"Format": F::PaintSolid, "PaletteIndex": 0, "Alpha": 1.0},
		},
	}));
	assert!(matches!(&error, Err(Error::BaseGlyphList(_))));
	assert!(msg(error).contains("PaintGlyph: glyph not found in glyphMap: 'B'"));
}

#[test]
fn test_build_colr_missing_paint_colr_glyph_in_glyph_map_raises_clear_error() {
	// Base glyph A exists, but its PaintColrGlyph references glyph "B" which
	// does not.
	let error = v1_with_glyph_map(value!({"A": {"Format": F::PaintColrGlyph, "Glyph": "B"}}));
	assert!(matches!(&error, Err(Error::BaseGlyphList(_))));
	assert!(msg(error).contains("PaintColrGlyph: glyph not found in glyphMap: 'B'"));
}

#[test]
fn test_build_colrv0_missing_layer_glyph_in_glyph_map_raises_clear_error() {
	let glyph_map: GlyphIds = HashMap::from([("A".to_owned(), 0)]);
	// Base glyph A exists, but one of its layers references glyph "B" which does
	// not.
	let error = build_colr(
		colr_glyphs(value!({"A": [("A", 0), ("B", 1)]})),
		&glyph_map,
		BuildColrOptions {
			version: Some(0),
			glyph_map: true,
			..BuildColrOptions::default()
		},
	);
	assert!(
		matches!(&error, Err(Error::ColorLib(m)) if m.contains("layer glyph not found in glyphMap: 'B'"))
	);
}

/// Errors of the color glyphs are collected, sorted by glyph name, with the
/// Python exception names (checked against fontTools).
#[test]
fn glyph_errors_are_collected() {
	let error = build_colr_v1(
		colr_glyphs(value!({
			"b": (F::PaintSolid, "x"),
			"a": {"Format": F::PaintLinearGradient, "ColorLine": {"Extend": 7}},
		})),
		&glyphs(),
		false,
		true,
	);
	assert_eq!(
		msg(error),
		"Failed to build BaseGlyphList:\n    a => ValueError: 7 is not a valid ExtendMode\n    b => TypeError: can only concatenate str (not \"float\") to str"
	);

	// A KeyError is not collected.
	let error = build_colr_v1(
		colr_glyphs(value!({"a": {"Format": F::PaintColrLayers, "NumLayers": 2}})),
		&glyphs(),
		false,
		true,
	);
	assert!(matches!(&error, Err(Error::Key(m)) if m == "'Layers'"));
}

/// Error messages of `TableBuilder`, checked against fontTools.
#[test]
fn table_builder_errors() {
	assert_eq!(
		msg(build_paint(value!({"Format": 2, "Foo": 1}))),
		"Unrecognized field Foo for <class 'fontTools.ttLib.tables.otTables.Paint'>; expected one of ['Alpha', 'PaletteIndex']"
	);
	assert_eq!(
		msg(build_paint(value!({"Format": 34}))),
		"<class 'fontTools.ttLib.tables.otTables.Paint'> invalid Format: 34"
	);
	assert_eq!(
		msg(build_paint(value!(5))),
		"Not sure how to populate <class 'fontTools.ttLib.tables.otTables.Paint'> from 5"
	);
	assert_eq!(
		msg(build_paint(value!((2, 0, 1, 2, 3, 4)))),
		"Sequence of 5 too long for <class 'fontTools.ttLib.tables.otTables.Paint'>; expected <= 2 values"
	);
	assert_eq!(
		msg(build_paint(
			value!({"Format": 4, "ColorLine": {"Extend": "foo"}})
		)),
		"foo is not a valid <enum 'ExtendMode'>"
	);
}

/// A colorLib table is a table whose class is known.
#[test]
fn tables_know_their_class() {
	let t = Table::Paint(Paint::Format1(ot::PaintFormat1::default()));
	assert_eq!(t.class().name(), "Paint");
}
