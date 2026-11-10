//! Port of fontTools' `Tests/pens/pointPen_test.py`.
//!
//! The test pens print numbers as integers when they are integral, which is
//! what Python prints for the integer coordinates of the tests.

use fontoxide_pens::point_pen::{GuessSmoothPointPen, ReverseContourPointPen};
use fontoxide_pens::{
	Pen, Point, PointPen, PointToSegmentPen, Result, SegmentToPointPen, SegmentType, Transform,
};

fn num(x: f64) -> String {
	if x.fract() == 0.0 {
		format!("{}", x as i64)
	} else {
		format!("{x}")
	}
}

fn transform(t: &Transform) -> String {
	let v: Vec<String> = t.to_array().iter().map(|&x| num(x)).collect();
	format!("[{}]", v.join(", "))
}

/// `_TestSegmentPen`.
#[derive(Default)]
struct TestSegmentPen(Vec<String>);

impl TestSegmentPen {
	fn repr(&self) -> String {
		self.0.join(" ")
	}
}

impl Pen for TestSegmentPen {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.0.push(format!("{} {} moveto", num(pt.0), num(pt.1)));
		Ok(())
	}
	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.0.push(format!("{} {} lineto", num(pt.0), num(pt.1)));
		Ok(())
	}
	fn curve_to(&mut self, points: &[Point]) -> Result<()> {
		let v: Vec<String> = points
			.iter()
			.map(|p| format!("{} {}", num(p.0), num(p.1)))
			.collect();
		self.0.push(format!("{} curveto", v.join(" ")));
		Ok(())
	}
	fn q_curve_to(&mut self, off: &[Point], on: Option<Point>) -> Result<()> {
		let mut v: Vec<String> = off
			.iter()
			.map(|p| format!("{} {}", num(p.0), num(p.1)))
			.collect();
		v.push(on.map_or("None".into(), |p| format!("{} {}", num(p.0), num(p.1))));
		self.0.push(format!("{} qcurveto", v.join(" ")));
		Ok(())
	}
	fn close_path(&mut self) -> Result<()> {
		self.0.push("closepath".into());
		Ok(())
	}
	fn end_path(&mut self) -> Result<()> {
		self.0.push("endpath".into());
		Ok(())
	}
	fn add_component(&mut self, glyph_name: &str, t: Transform) -> Result<()> {
		self.0
			.push(format!("'{glyph_name}' {} addcomponent", transform(&t)));
		Ok(())
	}
}

/// `_TestPointPen`.
#[derive(Default)]
struct TestPointPen(Vec<String>);

impl TestPointPen {
	fn repr(&self) -> String {
		self.0.join(" ")
	}
}

impl PointPen for TestPointPen {
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()> {
		let items = identifier
			.map(|i| format!("identifier='{i}'"))
			.unwrap_or_default();
		self.0.push(format!("beginPath({items})"));
		Ok(())
	}
	fn end_path(&mut self) -> Result<()> {
		self.0.push("endPath()".into());
		Ok(())
	}
	fn add_point(
		&mut self,
		pt: Point,
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		let mut items = vec![format!("({}, {})", num(pt.0), num(pt.1))];
		if let Some(st) = segment_type {
			items.push(format!("segmentType='{}'", st.as_str()));
		}
		if smooth {
			items.push("smooth=True".into());
		}
		if let Some(name) = name {
			items.push(format!("name='{name}'"));
		}
		if let Some(id) = identifier {
			items.push(format!("identifier='{id}'"));
		}
		self.0.push(format!("addPoint({})", items.join(", ")));
		Ok(())
	}
	fn add_component(
		&mut self,
		glyph_name: &str,
		t: Transform,
		identifier: Option<&str>,
	) -> Result<()> {
		let mut items = vec![format!("'{glyph_name}'"), transform(&t)];
		if let Some(id) = identifier {
			items.push(format!("identifier='{id}'"));
		}
		self.0.push(format!("addComponent({})", items.join(", ")));
		Ok(())
	}
}

const LINE: Option<SegmentType> = Some(SegmentType::Line);
const MOVE: Option<SegmentType> = Some(SegmentType::Move);
const CURVE: Option<SegmentType> = Some(SegmentType::Curve);
const QCURVE: Option<SegmentType> = Some(SegmentType::QCurve);

fn add(pen: &mut dyn PointPen, points: &[(Point, Option<SegmentType>)]) -> Result<()> {
	points
		.iter()
		.try_for_each(|&(pt, st)| pen.add_point(pt, st, false, None, None))
}

/// A contour drawn onto a `PointToSegmentPen`.
fn to_segments(
	identifier: Option<&str>,
	points: &[(Point, Option<SegmentType>)],
	implied: bool,
) -> Result<String> {
	let mut pen = TestSegmentPen::default();
	{
		let mut ppen = PointToSegmentPen::new(&mut pen, implied);
		ppen.begin_path(identifier)?;
		add(&mut ppen, points)?;
		ppen.end_path()?;
	}
	Ok(pen.repr())
}

#[test]
fn test_point_to_segment_pen() -> Result<()> {
	assert_eq!(
		to_segments(None, &[((10.0, 10.0), MOVE), ((10.0, 20.0), LINE)], false)?,
		"10 10 moveto 10 20 lineto endpath"
	);
	assert_eq!(
		to_segments(
			None,
			&[
				((10.0, 10.0), LINE),
				((10.0, 20.0), LINE),
				((20.0, 20.0), LINE)
			],
			false
		)?,
		"10 10 moveto 10 20 lineto 20 20 lineto closepath"
	);
	assert_eq!(
		to_segments(
			None,
			&[
				((10.0, 10.0), LINE),
				((10.0, 20.0), None),
				((20.0, 20.0), None),
				((20.0, 40.0), CURVE)
			],
			false
		)?,
		"10 10 moveto 10 20 20 20 20 40 curveto closepath"
	);
	assert_eq!(
		to_segments(
			Some("foo"),
			&[
				((10.0, 10.0), LINE),
				((10.0, 40.0), None),
				((40.0, 40.0), None),
				((10.0, 40.0), QCURVE)
			],
			false
		)?,
		"10 10 moveto 10 40 40 40 10 40 qcurveto closepath"
	);
	assert_eq!(
		to_segments(
			None,
			&[
				((10.0, 10.0), None),
				((10.0, 40.0), None),
				((40.0, 40.0), None)
			],
			false
		)?,
		"10 10 10 40 40 40 None qcurveto closepath"
	);
	assert_eq!(
		to_segments(
			None,
			&[
				((10.0, 10.0), LINE),
				((10.0, 20.0), LINE),
				((20.0, 20.0), LINE)
			],
			true
		)?,
		"10 10 moveto 10 20 lineto 20 20 lineto 10 10 lineto closepath"
	);
	// fontmake#572: an explicit closing line when the last point overlaps the
	// first.
	let overlapping = [
		((0.0, 651.0), LINE),
		((0.0, 101.0), LINE),
		((0.0, 101.0), LINE),
		((0.0, 651.0), LINE),
	];
	assert_eq!(
		to_segments(None, &overlapping, false)?,
		"0 651 moveto 0 101 lineto 0 101 lineto 0 651 lineto 0 651 lineto closepath"
	);
	Ok(())
}

#[test]
fn test_point_to_segment_round_trip() -> Result<()> {
	for (points, expected) in [
		(
			vec![
				((10.0, 10.0), LINE),
				((10.0, 20.0), None),
				((20.0, 20.0), None),
				((20.0, 40.0), CURVE),
			],
			"beginPath() addPoint((10, 10), segmentType='line') addPoint((10, 20)) \
			 addPoint((20, 20)) addPoint((20, 40), segmentType='curve') endPath()",
		),
		(
			vec![
				((0.0, 651.0), LINE),
				((0.0, 101.0), LINE),
				((0.0, 101.0), LINE),
				((0.0, 651.0), LINE),
			],
			"beginPath() addPoint((0, 651), segmentType='line') addPoint((0, 101), segmentType='line') \
			 addPoint((0, 101), segmentType='line') addPoint((0, 651), segmentType='line') endPath()",
		),
	] {
		let mut tpen = TestPointPen::default();
		{
			let mut seg = SegmentToPointPen::new(&mut tpen, true);
			let mut ppen = PointToSegmentPen::new(&mut seg, false);
			ppen.begin_path(None)?;
			add(&mut ppen, &points)?;
			ppen.end_path()?;
		}
		assert_eq!(tpen.repr(), expected);
	}
	Ok(())
}

fn to_points(draw: impl FnOnce(&mut dyn Pen) -> Result<()>) -> Result<String> {
	let mut tpen = TestPointPen::default();
	{
		let mut pen = SegmentToPointPen::new(&mut tpen, true);
		draw(&mut pen)?;
	}
	Ok(tpen.repr())
}

#[test]
fn test_segment_to_point_pen() -> Result<()> {
	assert_eq!(
		to_points(|p| {
			p.move_to((10.0, 10.0))?;
			p.end_path()
		})?,
		"beginPath() addPoint((10, 10), segmentType='move') endPath()"
	);
	assert_eq!(
		to_points(|p| {
			p.move_to((10.0, 10.0))?;
			p.line_to((10.0, 20.0))?;
			p.line_to((20.0, 20.0))?;
			p.close_path()
		})?,
		"beginPath() addPoint((10, 10), segmentType='line') addPoint((10, 20), segmentType='line') \
		 addPoint((20, 20), segmentType='line') endPath()"
	);
	assert_eq!(
		to_points(|p| {
			p.move_to((10.0, 10.0))?;
			p.curve_to(&[(10.0, 20.0), (20.0, 20.0), (20.0, 10.0)])?;
			p.close_path()
		})?,
		"beginPath() addPoint((10, 10), segmentType='line') addPoint((10, 20)) addPoint((20, 20)) \
		 addPoint((20, 10), segmentType='curve') endPath()"
	);
	assert_eq!(
		to_points(|p| {
			p.move_to((10.0, 10.0))?;
			p.q_curve_to(&[(10.0, 20.0), (20.0, 20.0)], Some((20.0, 10.0)))?;
			p.close_path()
		})?,
		"beginPath() addPoint((10, 10), segmentType='line') addPoint((10, 20)) addPoint((20, 20)) \
		 addPoint((20, 10), segmentType='qcurve') endPath()"
	);
	assert_eq!(
		to_points(|p| {
			p.q_curve_to(
				&[(10.0, 20.0), (20.0, 20.0), (20.0, 10.0), (10.0, 10.0)],
				None,
			)?;
			p.close_path()
		})?,
		"beginPath() addPoint((10, 20)) addPoint((20, 20)) addPoint((20, 10)) addPoint((10, 10)) endPath()"
	);
	// fonttools#4014: off-curve-only contours keep every point.
	assert_eq!(
		to_points(|p| {
			p.q_curve_to(
				&[
					(10.0, 20.0),
					(20.0, 20.0),
					(20.0, 10.0),
					(10.0, 10.0),
					(10.0, 20.0),
				],
				None,
			)?;
			p.close_path()
		})?,
		"beginPath() addPoint((10, 20)) addPoint((20, 20)) addPoint((20, 10)) addPoint((10, 10)) \
		 addPoint((10, 20)) endPath()"
	);
	Ok(())
}

#[test]
fn test_segment_to_point_round_trip() -> Result<()> {
	let mut spen = TestSegmentPen::default();
	{
		let mut ppen = PointToSegmentPen::new(&mut spen, false);
		let mut pen = SegmentToPointPen::new(&mut ppen, true);
		pen.move_to((10.0, 10.0))?;
		pen.line_to((10.0, 20.0))?;
		pen.line_to((20.0, 20.0))?;
		pen.close_path()?;
	}
	assert_eq!(
		spen.repr(),
		"10 10 moveto 10 20 lineto 20 20 lineto closepath"
	);

	let mut spen = TestSegmentPen::default();
	{
		let mut ppen = PointToSegmentPen::new(&mut spen, false);
		let mut pen = SegmentToPointPen::new(&mut ppen, true);
		pen.q_curve_to(
			&[(10.0, 20.0), (20.0, 20.0), (20.0, 10.0), (10.0, 10.0)],
			None,
		)?;
		pen.close_path()?;
		pen.add_component("base", Transform::default())?;
	}
	assert_eq!(
		spen.repr(),
		"10 20 20 20 20 10 10 10 None qcurveto closepath 'base' [1, 0, 0, 1, 0, 0] addcomponent"
	);
	Ok(())
}

fn guess_smooth(
	identifier: Option<&str>,
	points: &[(Point, Option<SegmentType>, Option<&str>)],
) -> Result<String> {
	let mut tpen = TestPointPen::default();
	{
		let mut pen = GuessSmoothPointPen::new(&mut tpen, 0.05);
		pen.begin_path(identifier)?;
		for &(pt, st, id) in points {
			pen.add_point(pt, st, false, None, id)?;
		}
		pen.end_path()?;
	}
	Ok(tpen.repr())
}

#[test]
fn test_guess_smooth_point_pen() -> Result<()> {
	assert_eq!(
		guess_smooth(
			Some("foo"),
			&[
				((0.0, 100.0), CURVE, None),
				((0.0, 200.0), None, None),
				((400.0, 200.0), None, Some("bar")),
				((400.0, 100.0), CURVE, None),
				((400.0, 0.0), None, None),
				((0.0, 0.0), None, None),
			]
		)?,
		"beginPath(identifier='foo') addPoint((0, 100), segmentType='curve', smooth=True) \
		 addPoint((0, 200)) addPoint((400, 200), identifier='bar') \
		 addPoint((400, 100), segmentType='curve', smooth=True) addPoint((400, 0)) addPoint((0, 0)) endPath()"
	);
	assert_eq!(
		guess_smooth(
			None,
			&[
				((0.0, 100.0), CURVE, None),
				((1.0, 200.0), None, None),
				((395.0, 200.0), None, None),
				((400.0, 100.0), CURVE, None),
				((400.0, 0.0), None, None),
				((0.0, 0.0), None, None),
			]
		)?,
		"beginPath() addPoint((0, 100), segmentType='curve', smooth=True) addPoint((1, 200)) \
		 addPoint((395, 200)) addPoint((400, 100), segmentType='curve', smooth=True) \
		 addPoint((400, 0)) addPoint((0, 0)) endPath()"
	);
	assert_eq!(
		guess_smooth(
			None,
			&[
				((0.0, 0.0), MOVE, None),
				((0.0, 100.0), LINE, None),
				((3.0, 200.0), None, None),
				((300.0, 200.0), None, None),
				((400.0, 200.0), CURVE, None),
			]
		)?,
		"beginPath() addPoint((0, 0), segmentType='move') addPoint((0, 100), segmentType='line', smooth=True) \
		 addPoint((3, 200)) addPoint((300, 200)) addPoint((400, 200), segmentType='curve') endPath()"
	);
	Ok(())
}

fn reverse(identifier: Option<&str>, points: &[(Point, Option<SegmentType>)]) -> Result<String> {
	let mut tpen = TestPointPen::default();
	{
		let mut pen = ReverseContourPointPen::new(&mut tpen);
		pen.begin_path(identifier)?;
		add(&mut pen, points)?;
		pen.end_path()?;
	}
	Ok(tpen.repr())
}

#[test]
fn test_reverse_contour_point_pen() -> Result<()> {
	assert_eq!(
		reverse(None, &[((0.0, 0.0), MOVE)])?,
		"beginPath() addPoint((0, 0), segmentType='move') endPath()"
	);
	assert_eq!(
		reverse(None, &[((0.0, 0.0), MOVE), ((0.0, 100.0), LINE)])?,
		"beginPath() addPoint((0, 100), segmentType='move') addPoint((0, 0), segmentType='line') endPath()"
	);
	assert_eq!(
		reverse(
			None,
			&[
				((0.0, 0.0), LINE),
				((0.0, 100.0), LINE),
				((100.0, 100.0), LINE)
			]
		)?,
		"beginPath() addPoint((0, 0), segmentType='line') addPoint((100, 100), segmentType='line') \
		 addPoint((0, 100), segmentType='line') endPath()"
	);
	assert_eq!(
		reverse(
			None,
			&[
				((0.0, 0.0), MOVE),
				((0.0, 100.0), None),
				((100.0, 200.0), None),
				((200.0, 200.0), CURVE)
			]
		)?,
		"beginPath() addPoint((200, 200), segmentType='move') addPoint((100, 200)) addPoint((0, 100)) \
		 addPoint((0, 0), segmentType='curve') endPath()"
	);
	assert_eq!(
		reverse(
			None,
			&[
				((0.0, 0.0), MOVE),
				((0.0, 100.0), None),
				((100.0, 200.0), None),
				((200.0, 200.0), QCURVE)
			]
		)?,
		"beginPath() addPoint((200, 200), segmentType='move') addPoint((100, 200)) addPoint((0, 100)) \
		 addPoint((0, 0), segmentType='qcurve') endPath()"
	);
	assert_eq!(
		reverse(
			None,
			&[
				((0.0, 0.0), LINE),
				((0.0, 100.0), None),
				((100.0, 200.0), None),
				((200.0, 200.0), CURVE)
			]
		)?,
		"beginPath() addPoint((0, 0), segmentType='curve') addPoint((200, 200), segmentType='line') \
		 addPoint((100, 200)) addPoint((0, 100)) endPath()"
	);
	assert_eq!(
		reverse(
			None,
			&[
				((100.0, 200.0), None),
				((200.0, 200.0), QCURVE),
				((0.0, 0.0), LINE),
				((0.0, 100.0), None)
			]
		)?,
		"beginPath() addPoint((100, 200)) addPoint((0, 100)) addPoint((0, 0), segmentType='qcurve') \
		 addPoint((200, 200), segmentType='line') endPath()"
	);
	let overlapping = [
		((0.0, 651.0), LINE),
		((0.0, 101.0), LINE),
		((0.0, 101.0), LINE),
		((0.0, 651.0), LINE),
	];
	assert_eq!(
		reverse(None, &overlapping)?,
		"beginPath() addPoint((0, 651), segmentType='line') addPoint((0, 651), segmentType='line') \
		 addPoint((0, 101), segmentType='line') addPoint((0, 101), segmentType='line') endPath()"
	);
	Ok(())
}

/// `test_quadNoOnCurve`, without the arbitrary keyword arguments, which only
/// Python pens carry.
#[test]
fn test_reverse_quad_no_on_curve() -> Result<()> {
	let mut tpen = TestPointPen::default();
	{
		let mut pen = ReverseContourPointPen::new(&mut tpen);
		pen.begin_path(Some("bar"))?;
		pen.add_point((0.0, 0.0), None, false, None, None)?;
		pen.add_point((0.0, 100.0), None, false, None, Some("foo"))?;
		pen.add_point((100.0, 200.0), None, false, None, None)?;
		pen.add_point((200.0, 200.0), None, false, None, None)?;
		pen.end_path()?;
		pen.add_component("base", Transform::default(), Some("foo"))?;
	}
	assert_eq!(
		tpen.repr(),
		"beginPath(identifier='bar') addPoint((0, 0)) addPoint((200, 200)) addPoint((100, 200)) \
		 addPoint((0, 100), identifier='foo') endPath() addComponent('base', [1, 0, 0, 1, 0, 0], identifier='foo')"
	);
	Ok(())
}
