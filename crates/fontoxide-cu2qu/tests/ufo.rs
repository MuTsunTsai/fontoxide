//! `Tests/cu2qu/ufo_test.py`.

mod common;

use std::collections::BTreeMap;

use common::{capture_logs, test_ufos};
use fontoxide_cu2qu::ufo::{
	CURVE_TYPE_LIB_KEY, Error, MaxErr, Options, Stats, font_to_quadratic, fonts_to_quadratic,
	glyph_to_quadratic, glyphs_to_quadratic,
};
use fontoxide_cu2qu::{Incompatibility, IncompatibleGlyphsError};
use fontoxide_pens::{Pen, PointPen, SegmentType, Transform};
use fontoxide_ufolib::objects::{Font, Glyph};
use fontoxide_ufolib::{Number, Value};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The `fonts` fixture.
fn fonts() -> Result<Vec<Font>> {
	Ok(test_ufos()
		.iter()
		.map(|p| Font::open(p, true))
		.collect::<std::result::Result<_, _>>()?)
}

fn stats_of(pairs: &[(&str, usize)]) -> Stats {
	pairs.iter().map(|&(k, v)| (k.to_owned(), v)).collect()
}

/// Glyphs of the same name in the fonts, mutably.
fn glyphs<'f>(fonts: &'f mut [Font], name: &str) -> Result<Vec<&'f mut Glyph>> {
	let mut out = Vec::new();
	for f in fonts.iter_mut() {
		out.push(f.get_mut(name)?.ok_or("missing glyph")?);
	}
	Ok(out)
}

fn lib_str<'f>(font: &'f Font, key: &str) -> Option<&'f str> {
	match font.lib.get(key) {
		Some(Value::String(s)) => Some(s),
		_ => None,
	}
}

// FontsToQuadraticTest

#[test]
fn fonts_test_modified() -> Result {
	let mut fonts = fonts()?;
	let modified = fonts_to_quadratic(&mut fonts, &Options::default(), None)?;
	assert!(!modified.is_empty());
	assert!(modified.contains("B"));
	Ok(())
}

#[test]
fn fonts_test_stats() -> Result {
	let mut fonts = fonts()?;
	let mut stats = Stats::new();
	fonts_to_quadratic(&mut fonts, &Options::default(), Some(&mut stats))?;
	assert_eq!(
		stats,
		stats_of(&[("1", 1), ("2", 79), ("3", 130), ("4", 2)])
	);
	Ok(())
}

#[test]
fn fonts_test_dump_stats() -> Result {
	let mut fonts = fonts()?;
	let options = Options {
		dump_stats: true,
		..Options::default()
	};
	let (logs, result) = capture_logs(|| fonts_to_quadratic(&mut fonts, &options, None));
	result?;
	assert!(
		logs.iter()
			.any(|(l, m)| *l == log::Level::Info && m.starts_with("New spline lengths:")),
		"{logs:?}"
	);
	Ok(())
}

#[test]
fn fonts_test_remember_curve_type_quadratic() -> Result {
	let mut fonts = fonts()?;
	fonts_to_quadratic(&mut fonts, &Options::default(), None)?;
	assert_eq!(lib_str(&fonts[0], CURVE_TYPE_LIB_KEY), Some("quadratic"));
	let (logs, result) = capture_logs(|| fonts_to_quadratic(&mut fonts, &Options::default(), None));
	assert!(result?.is_empty());
	assert!(logs.iter().any(|(_, m)| m.contains("already converted")));
	Ok(())
}

#[test]
fn fonts_test_remember_curve_type_mixed() -> Result {
	let mut fonts = fonts()?;
	let mixed = Options {
		all_quadratic: false,
		..Options::default()
	};
	fonts_to_quadratic(&mut fonts, &mixed, None)?;
	assert_eq!(lib_str(&fonts[0], CURVE_TYPE_LIB_KEY), Some("mixed"));
	let (logs, result) = capture_logs(|| fonts_to_quadratic(&mut fonts, &Options::default(), None));
	result?;
	assert!(logs.iter().any(|(_, m)| m.contains("already converted")));
	Ok(())
}

#[test]
fn fonts_test_no_remember_curve_type() -> Result {
	let mut fonts = fonts()?;
	assert!(!fonts[0].lib.contains_key(CURVE_TYPE_LIB_KEY));
	let options = Options {
		remember_curve_type: false,
		..Options::default()
	};
	fonts_to_quadratic(&mut fonts, &options, None)?;
	assert!(!fonts[0].lib.contains_key(CURVE_TYPE_LIB_KEY));
	Ok(())
}

#[test]
fn fonts_test_different_glyphsets() -> Result {
	let mut fonts = fonts()?;
	fonts[0].remove("a")?;
	assert!(!fonts[0].contains("a"));
	assert!(fonts[1].contains("a"));
	assert!(!fonts_to_quadratic(&mut fonts, &Options::default(), None)?.is_empty());
	Ok(())
}

fn check_max_err(options: Options) -> Result {
	let mut fonts = fonts()?;
	let mut stats = Stats::new();
	fonts_to_quadratic(&mut fonts, &options, Some(&mut stats))?;
	assert_eq!(stats, stats_of(&[("1", 5), ("2", 193), ("3", 14)]));
	Ok(())
}

#[test]
fn fonts_test_max_err_em_float() -> Result {
	check_max_err(Options {
		max_err_em: Some(MaxErr::Value(0.002)),
		..Options::default()
	})
}

#[test]
fn fonts_test_max_err_em_list() -> Result {
	check_max_err(Options {
		max_err_em: Some(MaxErr::List(vec![0.002, 0.002])),
		..Options::default()
	})
}

#[test]
fn fonts_test_max_err_float() -> Result {
	check_max_err(Options {
		max_err: Some(MaxErr::Value(4.096)),
		..Options::default()
	})
}

#[test]
fn fonts_test_max_err_list() -> Result {
	check_max_err(Options {
		max_err: Some(MaxErr::List(vec![4.096, 4.096])),
		..Options::default()
	})
}

fn value_error(result: std::result::Result<impl std::fmt::Debug, Error>, message: &str) {
	match result {
		Err(Error::Value(m)) => assert!(m.contains(message), "{m}"),
		other => panic!("expected ValueError, got {other:?}"),
	}
}

#[test]
fn fonts_test_reject_zero_tolerance() -> Result {
	for options in [
		Options {
			max_err: Some(MaxErr::Value(0.0)),
			..Options::default()
		},
		Options {
			max_err_em: Some(MaxErr::Value(0.0)),
			..Options::default()
		},
	] {
		let mut fonts = fonts()?;
		value_error(
			fonts_to_quadratic(&mut fonts, &options, None),
			"must be greater than zero",
		);
	}
	Ok(())
}

#[test]
fn fonts_test_reject_mismatched_tolerance_list_lengths() -> Result {
	for options in [
		Options {
			max_err: Some(MaxErr::List(vec![1.0])),
			..Options::default()
		},
		Options {
			max_err_em: Some(MaxErr::List(vec![0.001])),
			..Options::default()
		},
	] {
		let mut fonts = fonts()?;
		value_error(
			fonts_to_quadratic(&mut fonts, &options, None),
			"must match the number of inputs",
		);
	}
	Ok(())
}

#[test]
fn fonts_test_both_max_err_and_max_err_em() -> Result {
	let mut fonts = fonts()?;
	let options = Options {
		max_err: Some(MaxErr::Value(1.0)),
		max_err_em: Some(MaxErr::Value(0.001)),
		..Options::default()
	};
	match fonts_to_quadratic(&mut fonts, &options, None) {
		Err(Error::Type(m)) => {
			assert!(m.starts_with("Only one ") && m.ends_with(" can be specified."));
		}
		other => panic!("expected TypeError, got {other:?}"),
	}
	Ok(())
}

#[test]
fn fonts_test_single_font() -> Result {
	let mut fonts = fonts()?;
	let options = Options {
		max_err_em: Some(MaxErr::Value(0.002)),
		reverse_direction: true,
		..Options::default()
	};
	assert!(!font_to_quadratic(&mut fonts[0], &options, None)?.is_empty());
	let options = Options {
		all_quadratic: false,
		..options
	};
	assert!(!font_to_quadratic(&mut fonts[1], &options, None)?.is_empty());
	Ok(())
}

// GlyphsToQuadraticTest

#[test]
fn glyphs_test_modified() -> Result {
	// "lines-only": contains no curves, it is not modified; "has-curves".
	for (glyph, expected) in [("A", false), ("a", true)] {
		let mut fonts = fonts()?;
		let mut glyphs = glyphs(&mut fonts, glyph)?;
		assert_eq!(
			glyphs_to_quadratic(&mut glyphs, None, false, None, true)?,
			expected,
			"{glyph}"
		);
	}
	Ok(())
}

#[test]
fn glyphs_test_stats() -> Result {
	let mut fonts = fonts()?;
	let mut stats = Stats::new();
	glyphs_to_quadratic(
		&mut glyphs(&mut fonts, "a")?,
		None,
		false,
		Some(&mut stats),
		true,
	)?;
	assert_eq!(stats, stats_of(&[("2", 1), ("3", 7), ("4", 3), ("5", 1)]));
	Ok(())
}

#[test]
fn glyphs_test_max_err_float() -> Result {
	let mut fonts = fonts()?;
	let mut stats = Stats::new();
	glyphs_to_quadratic(
		&mut glyphs(&mut fonts, "a")?,
		Some(MaxErr::Value(4.096)),
		false,
		Some(&mut stats),
		true,
	)?;
	assert_eq!(stats, stats_of(&[("2", 11), ("3", 1)]));
	Ok(())
}

#[test]
fn glyphs_test_max_err_list() -> Result {
	let mut fonts = fonts()?;
	let mut stats = Stats::new();
	glyphs_to_quadratic(
		&mut glyphs(&mut fonts, "a")?,
		Some(MaxErr::List(vec![4.096, 4.096])),
		false,
		Some(&mut stats),
		true,
	)?;
	assert_eq!(stats, stats_of(&[("2", 11), ("3", 1)]));
	Ok(())
}

#[test]
fn glyphs_test_reject_zero_tolerance() -> Result {
	let mut fonts = fonts()?;
	value_error(
		glyphs_to_quadratic(
			&mut glyphs(&mut fonts, "a")?,
			Some(MaxErr::Value(0.0)),
			false,
			None,
			true,
		),
		"must be greater than zero",
	);
	Ok(())
}

#[test]
fn glyphs_test_reject_mismatched_tolerance_list_lengths() -> Result {
	let mut fonts = fonts()?;
	value_error(
		glyphs_to_quadratic(
			&mut glyphs(&mut fonts, "a")?,
			Some(MaxErr::List(vec![4.096])),
			false,
			None,
			true,
		),
		"must match the number of inputs",
	);
	Ok(())
}

#[test]
fn glyphs_test_reverse_direction() -> Result {
	let mut fonts = fonts()?;
	assert!(glyphs_to_quadratic(
		&mut glyphs(&mut fonts, "A")?,
		None,
		true,
		None,
		true
	)?);
	Ok(())
}

#[test]
fn glyphs_test_single_glyph() -> Result {
	let mut fonts = fonts()?;
	let glyph = fonts[0].get_mut("a")?.ok_or("missing glyph")?;
	assert!(glyph_to_quadratic(
		glyph,
		Some(MaxErr::Value(4.096)),
		true,
		None,
		true
	)?);
	Ok(())
}

/// A glyph drawn with `getPen()`.
fn drawn(
	name: Option<&str>,
	draw: impl FnOnce(&mut dyn Pen) -> fontoxide_pens::Result<()>,
) -> Result<Glyph> {
	let mut glyph = Glyph::new(name);
	glyph.draw_with_pen(draw)?;
	Ok(glyph)
}

#[test]
fn glyphs_test_incompatible_glyphs() -> Result {
	// "unequal-length"
	let mut g0 = drawn(Some("glyph0"), |p| {
		p.move_to((0.0, 0.0))?;
		p.curve_to(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)])?;
		p.curve_to(&[(4.0, 4.0), (5.0, 5.0), (6.0, 6.0)])?;
		p.close_path()
	})?;
	let mut g1 = drawn(Some("glyph1"), |p| {
		p.move_to((7.0, 7.0))?;
		p.curve_to(&[(8.0, 8.0), (9.0, 9.0), (10.0, 10.0)])?;
		p.close_path()
	})?;
	match glyphs_to_quadratic(&mut [&mut g0, &mut g1], None, false, None, true) {
		Err(Error::IncompatibleGlyphs(
			e @ IncompatibleGlyphsError {
				kind: Incompatibility::SegmentNumber,
				..
			},
		)) => assert!(
			e.to_string().contains("have different number of segments"),
			"{e}"
		),
		other => panic!("expected IncompatibleSegmentNumberError, got {other:?}"),
	}

	// "different-segment-types"
	let mut g0 = drawn(Some("glyph0"), |p| {
		p.move_to((0.0, 0.0))?;
		p.curve_to(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)])?;
		p.close_path()
	})?;
	let mut g1 = drawn(Some("glyph1"), |p| {
		p.move_to((4.0, 4.0))?;
		p.line_to((5.0, 5.0))?;
		p.close_path()
	})?;
	match glyphs_to_quadratic(&mut [&mut g0, &mut g1], None, false, None, true) {
		Err(Error::IncompatibleGlyphs(
			e @ IncompatibleGlyphsError {
				kind: Incompatibility::SegmentTypes(_),
				..
			},
		)) => {
			assert!(
				e.to_string().contains("have incompatible segment types"),
				"{e}"
			);
			assert_eq!(
				e.to_string(),
				"Glyphs named {'glyph0', 'glyph1'} have incompatible segment types:\n  1: ('curve', 'line')"
			);
		}
		other => panic!("expected IncompatibleSegmentTypesError, got {other:?}"),
	}
	Ok(())
}

#[test]
fn glyphs_test_incompatible_fonts() -> Result {
	let mut font1 = Font::new();
	font1.info.insert("unitsPerEm".into(), Value::Integer(1000));
	font1.new_glyph("a")?.draw_with_pen(|p| {
		p.move_to((0.0, 0.0))?;
		p.line_to((1.0, 1.0))?;
		p.end_path()
	})?;
	let mut font2 = Font::new();
	font2.info.insert("unitsPerEm".into(), Value::Integer(1000));
	font2.new_glyph("a")?.draw_with_pen(|p| {
		p.move_to((0.0, 0.0))?;
		p.curve_to(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)])?;
		p.end_path()
	})?;
	let mut fonts = [font1, font2];
	match fonts_to_quadratic(&mut fonts, &Options::default(), None) {
		Err(Error::IncompatibleFonts(e)) => {
			assert!(
				e.to_string()
					.contains("fonts contains incompatible glyphs: 'a'")
			);
			let error = &e.glyph_errors["a"];
			assert_eq!(
				error.segments(),
				Some(&BTreeMap::from([(
					1,
					vec!["line".to_owned(), "curve".to_owned()]
				)]))
			);
		}
		other => panic!("expected IncompatibleFontsError, got {other:?}"),
	}
	Ok(())
}

#[test]
fn glyphs_test_already_quadratic() -> Result {
	let mut glyph = drawn(None, |p| {
		p.move_to((0.0, 0.0))?;
		p.q_curve_to(&[(1.0, 1.0)], Some((2.0, 2.0)))?;
		p.close_path()
	})?;
	assert!(!glyph_to_quadratic(&mut glyph, None, false, None, true)?);
	Ok(())
}

#[test]
fn glyphs_test_open_paths() -> Result {
	let mut glyph = drawn(None, |p| {
		p.move_to((0.0, 0.0))?;
		p.line_to((1.0, 1.0))?;
		p.curve_to(&[(2.0, 2.0), (3.0, 3.0), (4.0, 4.0)])?;
		p.end_path()
	})?;
	assert!(glyph_to_quadratic(&mut glyph, None, false, None, true)?);
	// open contour is still open
	assert_eq!(
		glyph.contours.last().ok_or("no contour")?.points[0].segment_type,
		Some(SegmentType::Move)
	);
	Ok(())
}

#[test]
fn glyphs_test_ignore_components() -> Result {
	let mut glyph = drawn(None, |p| {
		p.add_component("a", Transform::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0))?;
		p.move_to((0.0, 0.0))?;
		p.curve_to(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)])?;
		p.close_path()
	})?;
	assert!(glyph_to_quadratic(&mut glyph, None, false, None, true)?);
	assert_eq!(glyph.components.len(), 1);
	Ok(())
}

fn add_line_points(glyph: &mut Glyph, points: &[(f64, f64)]) -> Result {
	let mut pen = glyph.point_pen();
	PointPen::begin_path(&mut pen, None)?;
	for &pt in points {
		PointPen::add_point(&mut pen, pt, Some(SegmentType::Line), false, None, None)?;
	}
	PointPen::end_path(&mut pen)?;
	Ok(())
}

fn coords(glyph: &Glyph) -> Vec<(f64, f64)> {
	glyph.contours[0].points.iter().map(|p| p.pt()).collect()
}

#[test]
fn glyphs_test_overlapping_start_end_points() -> Result {
	// https://github.com/googlefonts/fontmake/issues/572
	let mut glyph1 = Glyph::new(None);
	add_line_points(
		&mut glyph1,
		&[(0.0, 651.0), (0.0, 101.0), (0.0, 101.0), (0.0, 651.0)],
	)?;
	let mut glyph2 = Glyph::new(None);
	add_line_points(
		&mut glyph2,
		&[(1.0, 651.0), (2.0, 101.0), (3.0, 101.0), (4.0, 651.0)],
	)?;
	assert!(glyphs_to_quadratic(
		&mut [&mut glyph1, &mut glyph2],
		None,
		true,
		None,
		true
	)?);
	assert_eq!(
		coords(&glyph1),
		[(0.0, 651.0), (0.0, 651.0), (0.0, 101.0), (0.0, 101.0)]
	);
	assert_eq!(
		coords(&glyph2),
		[(1.0, 651.0), (4.0, 651.0), (3.0, 101.0), (2.0, 101.0)]
	);
	Ok(())
}

#[test]
fn glyphs_test_ignore_empty_glyphs() -> Result {
	let mut non_empty = Glyph::new(Some("sparse"));
	{
		let mut pen = non_empty.point_pen();
		PointPen::begin_path(&mut pen, None)?;
		PointPen::add_point(
			&mut pen,
			(0.0, 0.0),
			Some(SegmentType::Line),
			false,
			None,
			None,
		)?;
		PointPen::add_point(&mut pen, (0.0, 2.0), None, false, None, None)?;
		PointPen::add_point(&mut pen, (1.0, 3.0), None, false, None, None)?;
		PointPen::add_point(
			&mut pen,
			(3.0, 3.0),
			Some(SegmentType::Curve),
			false,
			None,
			None,
		)?;
		PointPen::end_path(&mut pen)?;
	}
	let mut empty = Glyph::new(Some("sparse"));
	assert!(glyphs_to_quadratic(
		&mut [&mut non_empty, &mut empty],
		None,
		false,
		None,
		true
	)?);
	// Verify non-empty glyph was converted to quadratic
	let points: Vec<((Number, Number), Option<SegmentType>)> = non_empty.contours[0]
		.points
		.iter()
		.map(|p| ((p.x, p.y), p.segment_type))
		.collect();
	assert_eq!(
		points,
		[
			((Number::Int(0), Number::Int(0)), Some(SegmentType::Line)),
			((Number::Float(0.0), Number::Float(3.0)), None),
			(
				(Number::Float(3.0), Number::Float(3.0)),
				Some(SegmentType::QCurve)
			),
		]
	);
	// The int and float types are those of Python.
	assert!(matches!(points[0].0.0, Number::Int(0)));
	assert!(matches!(points[1].0.1, Number::Float(_)));
	// Verify empty glyph remains empty
	assert_eq!(empty.len(), 0);
	Ok(())
}
