//! Ports of fontTools' `Tests/pens/basePen_test.py`,
//! `Tests/pens/pointInsidePen_test.py` and `Tests/pens/recordingPen_test.py`.

use fontoxide_pens::base_pen::{
	BasePen, SegmentPen, decompose_quadratic_segment, decompose_super_bezier_segment,
};
use fontoxide_pens::point_inside_pen::PointInsidePen;
use fontoxide_pens::recording_pen::{
	DecomposingRecordingPen, PenOp, PointOp, RecordingPen, RecordingPointPen,
};
use fontoxide_pens::{
	DecomposedTransform, Error, GlyphSet, Pen, Point, PointPen, Result, SegmentType, Transform,
};

/// `_TestPen`: records the segments, as numbers rather than `%s` strings
/// (fontTools prints integers and floats differently).
#[derive(Debug, Default)]
struct TestPen(Vec<(&'static str, Vec<f64>)>);

impl SegmentPen for TestPen {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.0.push(("moveto", vec![pt.0, pt.1]));
		Ok(())
	}
	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.0.push(("lineto", vec![pt.0, pt.1]));
		Ok(())
	}
	fn curve_to_one(&mut self, _: Option<Point>, a: Point, b: Point, c: Point) -> Result<()> {
		self.0.push(("curveto", vec![a.0, a.1, b.0, b.1, c.0, c.1]));
		Ok(())
	}
	fn close_path(&mut self) -> Result<()> {
		self.0.push(("closepath", vec![]));
		Ok(())
	}
	fn end_path(&mut self) -> Result<()> {
		self.0.push(("endpath", vec![]));
		Ok(())
	}
}

/// The commands of a `repr(pen)` string of fontTools.
fn parse(s: &str) -> Vec<(&'static str, Vec<f64>)> {
	let mut out = Vec::new();
	let mut nums = Vec::new();
	for word in s.split_whitespace() {
		let op = match word {
			"moveto" => "moveto",
			"lineto" => "lineto",
			"curveto" => "curveto",
			"closepath" => "closepath",
			"endpath" => "endpath",
			_ => {
				nums.push(word.parse().unwrap_or(f64::NAN));
				continue;
			}
		};
		out.push((op, std::mem::take(&mut nums)));
	}
	out
}

fn test_pen() -> BasePen<'static, TestPen> {
	BasePen::with_inner(TestPen::default(), None)
}

#[test]
fn test_move_to_line_to() -> Result<()> {
	let mut pen = test_pen();
	pen.move_to((0.5, -4.3))?;
	assert_eq!(pen.inner.0, parse("0.5 -4.3 moveto"));
	assert_eq!(pen.current_point(), Some((0.5, -4.3)));

	let mut pen = test_pen();
	pen.move_to((4.0, 5.0))?;
	pen.line_to((7.0, 8.0))?;
	assert_eq!(pen.inner.0, parse("4 5 moveto 7 8 lineto"));
	assert_eq!(pen.current_point(), Some((7.0, 8.0)));
	Ok(())
}

#[test]
fn test_curve_to() -> Result<()> {
	let mut pen = test_pen();
	pen.move_to((0.0, 0.0))?;
	assert!(matches!(pen.curve_to(&[]), Err(Error::Usage(_))));

	let mut pen = test_pen();
	pen.move_to((0.0, 0.0))?;
	pen.curve_to(&[(1.0, 1.1)])?;
	assert_eq!(pen.inner.0, parse("0.0 0.0 moveto 1.0 1.1 lineto"));
	assert_eq!(pen.current_point(), Some((1.0, 1.1)));

	let mut pen = test_pen();
	pen.move_to((0.0, 0.0))?;
	pen.curve_to(&[(6.0, 3.0), (3.0, 6.0)])?;
	assert_eq!(
		pen.inner.0,
		parse("0.0 0.0 moveto 4.0 2.0 5.0 4.0 3.0 6.0 curveto")
	);
	assert_eq!(pen.current_point(), Some((3.0, 6.0)));

	let mut pen = test_pen();
	pen.move_to((0.0, 0.0))?;
	pen.curve_to(&[(1.0, 1.1), (2.0, 2.1), (3.0, 3.1), (4.0, 4.1)])?;
	assert_eq!(
		pen.inner.0,
		parse("0.0 0.0 moveto 1.0 1.1 1.5 1.6 2.0 2.1 curveto 2.5 2.6 3.0 3.1 4.0 4.1 curveto")
	);
	assert_eq!(pen.current_point(), Some((4.0, 4.1)));
	Ok(())
}

#[test]
fn test_q_curve_to() -> Result<()> {
	let mut pen = test_pen();
	pen.move_to((0.0, 0.0))?;
	assert!(pen.q_curve_to(&[], None).is_err());

	let mut pen = test_pen();
	pen.move_to((0.0, 0.0))?;
	pen.q_curve_to(&[], Some((77.7, 99.9)))?;
	assert_eq!(pen.inner.0, parse("0.0 0.0 moveto 77.7 99.9 lineto"));
	assert_eq!(pen.current_point(), Some((77.7, 99.9)));

	let mut pen = test_pen();
	pen.move_to((0.0, 0.0))?;
	pen.q_curve_to(&[(6.0, 3.0)], Some((3.0, 6.0)))?;
	assert_eq!(
		pen.inner.0,
		parse("0.0 0.0 moveto 4.0 2.0 5.0 4.0 3.0 6.0 curveto")
	);
	assert_eq!(pen.current_point(), Some((3.0, 6.0)));

	let mut pen = test_pen();
	pen.move_to((0.0, 0.0))?;
	pen.q_curve_to(&[(6.0, -6.0), (12.0, 12.0), (18.0, -18.0)], None)?;
	assert_eq!(
		pen.inner.0,
		parse(
			"0.0 0.0 moveto 12.0 -12.0 moveto 8.0 -8.0 7.0 -3.0 9.0 3.0 curveto \
			 11.0 9.0 13.0 7.0 15.0 -3.0 curveto 17.0 -13.0 16.0 -16.0 12.0 -12.0 curveto"
		)
	);
	assert_eq!(pen.current_point(), Some((12.0, -12.0)));
	Ok(())
}

#[test]
fn test_close_and_end_path() -> Result<()> {
	let mut pen = test_pen();
	pen.line_to((3.0, 4.0))?;
	pen.close_path()?;
	assert_eq!(pen.inner.0, parse("3 4 lineto closepath"));
	assert_eq!(pen.current_point(), None);

	let mut pen = test_pen();
	pen.line_to((3.0, 4.0))?;
	pen.end_path()?;
	assert_eq!(pen.inner.0, parse("3 4 lineto endpath"));
	assert_eq!(pen.current_point(), None);
	Ok(())
}

/// The glyph set of the tests: `_TestGlyph`, drawn by any name in `names`.
struct Glyphs(&'static [&'static str]);

impl GlyphSet for Glyphs {
	fn draw_glyph(&self, name: &str, pen: &mut dyn Pen) -> Result<bool> {
		if !self.0.contains(&name) {
			return Ok(false);
		}
		pen.move_to((0.0, 0.0))?;
		pen.line_to((0.0, 100.0))?;
		pen.curve_to(&[(50.0, 75.0), (60.0, 50.0), (50.0, 25.0), (0.0, 0.0)])?;
		pen.close_path()?;
		Ok(true)
	}
}

#[test]
fn test_add_component() -> Result<()> {
	let glyphs = Glyphs(&["oslash"]);
	let mut pen = BasePen::with_inner(TestPen::default(), Some(&glyphs));
	pen.add_component("oslash", Transform::new(2.0, 3.0, 0.5, 2.0, -10.0, 0.0))?;
	assert_eq!(
		pen.inner.0,
		parse(
			"-10.0 0.0 moveto 40.0 200.0 lineto 127.5 300.0 131.25 290.0 125.0 265.0 curveto \
			 118.75 240.0 102.5 200.0 -10.0 0.0 curveto closepath"
		)
	);
	assert_eq!(pen.current_point(), None);
	Ok(())
}

#[test]
fn test_add_component_skip_missing() -> Result<()> {
	// A warning is logged, and nothing is drawn.
	let glyphs = Glyphs(&[]);
	let mut pen = BasePen::with_inner(TestPen::default(), Some(&glyphs));
	pen.add_component("nonexistent", Transform::default())?;
	assert!(pen.inner.0.is_empty());
	Ok(())
}

#[test]
fn test_decompose_super_bezier_segment() -> Result<()> {
	assert!(decompose_super_bezier_segment(&[]).is_err());
	assert!(decompose_super_bezier_segment(&[(0.0, 0.0)]).is_err());
	assert!(decompose_super_bezier_segment(&[(0.0, 0.0), (1.0, 1.0)]).is_err());
	assert_eq!(
		decompose_super_bezier_segment(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)])?,
		[((0.0, 0.0), (1.0, 1.0), (2.0, 2.0))]
	);
	assert_eq!(
		decompose_super_bezier_segment(&[(0.0, 0.0), (4.0, -4.0), (8.0, 8.0), (12.0, -12.0)])?,
		[
			((0.0, 0.0), (2.0, -2.0), (4.0, 0.0)),
			((6.0, 2.0), (8.0, 8.0), (12.0, -12.0))
		]
	);
	Ok(())
}

#[test]
fn test_decompose_quadratic_segment() -> Result<()> {
	assert!(decompose_quadratic_segment(&[]).is_err());
	assert!(decompose_quadratic_segment(&[(0.0, 0.0)]).is_err());
	assert_eq!(
		decompose_quadratic_segment(&[(0.0, 0.0), (4.0, 8.0)])?,
		[((0.0, 0.0), (4.0, 8.0))]
	);
	assert_eq!(
		decompose_quadratic_segment(&[(0.0, 0.0), (4.0, 8.0), (9.0, -9.0)])?,
		[((0.0, 0.0), (2.0, 4.0)), ((4.0, 8.0), (9.0, -9.0))]
	);
	assert_eq!(
		decompose_quadratic_segment(&[(0.0, 0.0), (4.0, 8.0), (9.0, -9.0), (10.0, 10.0)])?,
		[
			((0.0, 0.0), (2.0, 4.0)),
			((4.0, 8.0), (6.5, -0.5)),
			((9.0, -9.0), (10.0, 10.0))
		]
	);
	Ok(())
}

// `PointInsidePenTest`.

fn render(draw: fn(&mut dyn Pen) -> Result<()>, even_odd: bool) -> Result<String> {
	let mut result = String::new();
	for y in 0..5 {
		for x in 0..10 {
			let mut pen =
				PointInsidePen::new_point_inside(None, (x as f64 + 0.5, y as f64 + 0.5), even_odd);
			draw(&mut pen)?;
			result.push(if pen.get_result()? { '*' } else { ' ' });
		}
	}
	Ok(result)
}

#[test]
fn test_point_inside_line() -> Result<()> {
	fn draw(pen: &mut dyn Pen) -> Result<()> {
		pen.move_to((0.0, 0.0))?;
		pen.line_to((10.0, 5.0))?;
		pen.line_to((10.0, 0.0))?;
		pen.move_to((9.0, 1.0))?;
		pen.line_to((4.0, 1.0))?;
		pen.line_to((9.0, 4.0))?;
		pen.close_path()
	}
	assert_eq!(
		render(draw, true)?,
		concat!(
			" *********",
			"   **    *",
			"     **  *",
			"       * *",
			"         *"
		)
	);
	assert_eq!(
		render(draw, false)?,
		concat!(
			" *********",
			"   *******",
			"     *****",
			"       ***",
			"         *"
		)
	);
	Ok(())
}

#[test]
fn test_point_inside_curve() -> Result<()> {
	fn draw(pen: &mut dyn Pen) -> Result<()> {
		pen.move_to((0.0, 0.0))?;
		pen.curve_to(&[(9.0, 1.0), (9.0, 4.0), (0.0, 5.0)])?;
		pen.move_to((10.0, 5.0))?;
		pen.curve_to(&[(1.0, 4.0), (1.0, 1.0), (10.0, 0.0)])?;
		pen.close_path()
	}
	assert_eq!(
		render(draw, true)?,
		concat!(
			"***    ***",
			"****  ****",
			"***    ***",
			"****  ****",
			"***    ***"
		)
	);
	assert_eq!(
		render(draw, false)?,
		concat!(
			"***    ***",
			"**********",
			"**********",
			"**********",
			"***    ***"
		)
	);
	Ok(())
}

#[test]
fn test_point_inside_q_curve() -> Result<()> {
	fn draw(pen: &mut dyn Pen) -> Result<()> {
		pen.move_to((0.0, 0.0))?;
		pen.q_curve_to(&[(15.0, 2.0)], Some((0.0, 5.0)))?;
		pen.move_to((10.0, 5.0))?;
		pen.q_curve_to(&[(-5.0, 3.0)], Some((10.0, 0.0)))?;
		pen.close_path()
	}
	assert_eq!(
		render(draw, true)?,
		concat!(
			"***     **",
			"****   ***",
			"***    ***",
			"***   ****",
			"**     ***"
		)
	);
	assert_eq!(
		render(draw, false)?,
		concat!(
			"***     **",
			"**********",
			"**********",
			"**********",
			"**     ***"
		)
	);
	Ok(())
}

fn winding(test_point: Point, draw: fn(&mut dyn Pen) -> Result<()>) -> Result<(i64, bool)> {
	let mut pen = PointInsidePen::new_point_inside(None, test_point, false);
	draw(&mut pen)?;
	let w = pen.get_winding()?;
	Ok((w, pen.get_result()?))
}

#[test]
fn test_point_inside_contours() -> Result<()> {
	fn no_solutions(pen: &mut dyn Pen) -> Result<()> {
		pen.move_to((969.0, 230.0))?;
		pen.curve_to(&[(825.0, 348.0), (715.0, 184.0), (614.0, 202.0)])?;
		pen.line_to((614.0, 160.0))?;
		pen.line_to((969.0, 160.0))?;
		pen.close_path()
	}
	assert_eq!(winding((750.0, 295.0), no_solutions)?, (0, false));
	assert_eq!(winding((835.0, 190.0), no_solutions)?, (1, true));

	fn square_closed(pen: &mut dyn Pen) -> Result<()> {
		square_opened(pen)?;
		pen.close_path()
	}
	fn square_opened(pen: &mut dyn Pen) -> Result<()> {
		pen.move_to((100.0, 100.0))?;
		pen.line_to((-100.0, 100.0))?;
		pen.line_to((-100.0, -100.0))?;
		pen.line_to((100.0, -100.0))
	}
	assert_eq!(winding((0.0, 0.0), square_closed)?, (1, true));
	assert_eq!(winding((0.0, 0.0), square_opened)?, (1, true));

	fn circle(pen: &mut dyn Pen) -> Result<()> {
		pen.move_to((0.0, 100.0))?;
		pen.curve_to(&[(-55.0, 100.0), (-100.0, 55.0), (-100.0, 0.0)])?;
		pen.curve_to(&[(-100.0, -55.0), (-55.0, -100.0), (0.0, -100.0)])?;
		pen.curve_to(&[(55.0, -100.0), (100.0, -55.0), (100.0, 0.0)])?;
		pen.curve_to(&[(100.0, 55.0), (55.0, 100.0), (0.0, 100.0)])
	}
	assert!(winding((50.0, 50.0), circle)?.1);
	assert!(winding((50.0, -50.0), circle)?.1);

	fn diamond(pen: &mut dyn Pen) -> Result<()> {
		pen.move_to((0.0, 100.0))?;
		pen.line_to((100.0, 0.0))?;
		pen.line_to((0.0, -100.0))?;
		pen.line_to((-100.0, 0.0))?;
		pen.close_path()
	}
	for pt in [
		(-200.0, 0.0),
		(-200.0, 100.0),
		(-200.0, -100.0),
		(-200.0, 50.0),
	] {
		assert_eq!(winding(pt, diamond)?.0, 0);
	}

	fn integers(pen: &mut dyn Pen) -> Result<()> {
		pen.move_to((728.0, 697.0))?;
		pen.line_to((504.0, 699.0))?;
		pen.curve_to(&[(487.0, 719.0), (508.0, 783.0), (556.0, 783.0)])?;
		pen.line_to((718.0, 783.0))?;
		pen.curve_to(&[(739.0, 783.0), (749.0, 712.0), (728.0, 697.0)])?;
		pen.close_path()
	}
	assert_eq!(winding((416.0, 783.0), integers)?.0, 0);

	fn decimals(pen: &mut dyn Pen) -> Result<()> {
		pen.move_to((727.546875, 697.0))?;
		pen.line_to((504.375, 698.515625))?;
		pen.curve_to(&[
			(487.328125, 719.359375),
			(507.84375, 783.140625),
			(555.796875, 783.140625),
		])?;
		pen.line_to((717.96875, 783.140625))?;
		pen.curve_to(&[
			(738.890625, 783.140625),
			(748.796875, 711.5),
			(727.546875, 697.0),
		])?;
		pen.close_path()
	}
	assert_eq!(winding((416.625, 783.140625), decimals)?.0, 0);

	fn integers2(pen: &mut dyn Pen) -> Result<()> {
		pen.move_to((51.0, 22.0))?;
		pen.line_to((51.0, 74.0))?;
		pen.line_to((83.0, 50.0))?;
		pen.curve_to(&[(83.0, 49.0), (82.0, 48.0), (82.0, 47.0)])?;
		pen.close_path()
	}
	assert_eq!(winding((21.0, 50.0), integers2)?.0, 0);

	fn decimals2(pen: &mut dyn Pen) -> Result<()> {
		pen.move_to((51.25, 21.859375))?;
		pen.line_to((51.25, 73.828125))?;
		pen.line_to((82.5, 50.0))?;
		pen.curve_to(&[
			(82.5, 49.09375),
			(82.265625, 48.265625),
			(82.234375, 47.375),
		])?;
		pen.close_path()
	}
	assert_eq!(winding((21.25, 50.0), decimals2)?.0, 0);
	Ok(())
}

// `recordingPen_test.py`.

/// `_TestGlyph` of the recording pen tests.
struct RecGlyph;

impl GlyphSet for RecGlyph {
	fn draw_glyph(&self, name: &str, pen: &mut dyn Pen) -> Result<bool> {
		if name != "a" {
			return Ok(false);
		}
		pen.move_to((0.0, 0.0))?;
		pen.line_to((0.0, 100.0))?;
		pen.curve_to(&[(50.0, 75.0), (60.0, 50.0), (50.0, 0.0)])?;
		pen.close_path()?;
		Ok(true)
	}

	fn draw_glyph_points(&self, name: &str, pen: &mut dyn PointPen) -> Result<bool> {
		if name != "a" {
			return Ok(false);
		}
		pen.begin_path(Some("abc"))?;
		pen.add_point(
			(0.0, 0.0),
			Some(SegmentType::Line),
			false,
			Some("start"),
			Some("0000"),
		)?;
		pen.add_point(
			(0.0, 100.0),
			Some(SegmentType::Line),
			false,
			None,
			Some("0001"),
		)?;
		pen.add_point((50.0, 75.0), None, false, None, Some("0002"))?;
		pen.add_point((60.0, 50.0), None, false, None, Some("0003"))?;
		pen.add_point(
			(50.0, 0.0),
			Some(SegmentType::Curve),
			true,
			Some("last"),
			Some("0004"),
		)?;
		pen.end_path()?;
		Ok(true)
	}
}

#[test]
fn test_recording_pen() -> Result<()> {
	let t = Transform::new(2.0, 0.0, 0.0, 3.0, -10.0, 5.0);
	let mut pen = RecordingPen::new();
	pen.add_component("a", t)?;
	assert_eq!(pen.value, [PenOp::AddComponent("a".into(), t)]);

	// `test_replay_var_component_kwargs`.
	let mut pen = RecordingPen::new();
	pen.add_var_component(
		"a",
		DecomposedTransform::default(),
		&vec![("wght".into(), 0.5)],
		true,
	)?;
	let mut replayed = RecordingPen::new();
	pen.replay(&mut replayed)?;
	assert_eq!(replayed.value, pen.value);
	Ok(())
}

#[test]
fn test_decomposing_recording_pen() -> Result<()> {
	let mut pen = DecomposingRecordingPen::new(&RecGlyph);
	pen.add_component("a", Transform::new(2.0, 0.0, 0.0, 3.0, -10.0, 5.0))?;
	assert_eq!(
		pen.value(),
		[
			PenOp::MoveTo((-10.0, 5.0)),
			PenOp::LineTo((-10.0, 305.0)),
			PenOp::CurveTo(vec![(90.0, 230.0), (110.0, 155.0), (90.0, 5.0)]),
			PenOp::ClosePath,
		]
	);

	struct Empty;
	impl GlyphSet for Empty {
		fn draw_glyph(&self, _: &str, _: &mut dyn Pen) -> Result<bool> {
			Ok(false)
		}
	}
	let mut pen = DecomposingRecordingPen::new(&Empty);
	let e = pen.add_component("a", Transform::default());
	assert!(matches!(e, Err(Error::MissingComponent(name)) if name == "a"));
	Ok(())
}

#[test]
fn test_record_and_replay() -> Result<()> {
	let mut pen = RecordingPointPen::new();
	RecGlyph.draw_glyph_points("a", &mut pen)?;
	let t = Transform::new(2.0, 0.0, 0.0, 2.0, -10.0, 5.0);
	pen.add_component("a", t, None)?;
	let pt = |pt: Point, st: Option<SegmentType>, smooth: bool, name: Option<&str>, id: &str| {
		PointOp::AddPoint {
			pt,
			segment_type: st,
			smooth,
			name: name.map(str::to_owned),
			identifier: Some(id.to_owned()),
		}
	};
	assert_eq!(
		pen.value,
		[
			PointOp::BeginPath(Some("abc".into())),
			pt(
				(0.0, 0.0),
				Some(SegmentType::Line),
				false,
				Some("start"),
				"0000"
			),
			pt((0.0, 100.0), Some(SegmentType::Line), false, None, "0001"),
			pt((50.0, 75.0), None, false, None, "0002"),
			pt((60.0, 50.0), None, false, None, "0003"),
			pt(
				(50.0, 0.0),
				Some(SegmentType::Curve),
				true,
				Some("last"),
				"0004"
			),
			PointOp::EndPath,
			PointOp::AddComponent("a".into(), t, None),
		]
	);
	let mut pen2 = RecordingPointPen::new();
	pen.replay(&mut pen2)?;
	assert_eq!(pen2.value, pen.value);
	Ok(())
}
