//! `Tests/svgLib/path/parser_test.py`.
#![allow(non_snake_case)]

use fontoxide_pens::recording_pen::PenOp;
use fontoxide_pens::{Pen, Point, RecordingPen, Transform};
use fontoxide_svglib::{ArcPen, Error, parse_path, parse_path_arcs};

fn m(x: f64, y: f64) -> PenOp {
	PenOp::MoveTo((x, y))
}
fn l(x: f64, y: f64) -> PenOp {
	PenOp::LineTo((x, y))
}
fn c(a: Point, b: Point, e: Point) -> PenOp {
	PenOp::CurveTo(vec![a, b, e])
}
fn q(a: Point, e: Point) -> PenOp {
	PenOp::QCurveTo(vec![a], Some(e))
}
const Z: PenOp = PenOp::ClosePath;
const E: PenOp = PenOp::EndPath;

fn record(pathdef: &str) -> Vec<PenOp> {
	let mut pen = RecordingPen::new();
	parse_path(pathdef, &mut pen, (0.0, 0.0)).expect("valid path");
	pen.value
}

#[test]
fn test_parse_path() {
	let cases: Vec<(&str, Vec<PenOp>)> = vec![
		// Examples from the SVG spec
		(
			"M 100 100 L 300 100 L 200 300 z",
			vec![
				m(100.0, 100.0),
				l(300.0, 100.0),
				l(200.0, 300.0),
				l(100.0, 100.0),
				Z,
			],
		),
		// for Z command behavior when there is multiple subpaths
		(
			"M 0 0 L 50 20 M 100 100 L 300 100 L 200 300 z",
			vec![
				m(0.0, 0.0),
				l(50.0, 20.0),
				E,
				m(100.0, 100.0),
				l(300.0, 100.0),
				l(200.0, 300.0),
				l(100.0, 100.0),
				Z,
			],
		),
		// a redundant Z after a subpath is already closed is a no-op
		(
			"M 100 100 L 300 100 L 200 300 z z",
			vec![
				m(100.0, 100.0),
				l(300.0, 100.0),
				l(200.0, 300.0),
				l(100.0, 100.0),
				Z,
			],
		),
		// a redundant Z following an empty (moveto-only) closed subpath
		("M 100 100 Z Z", vec![m(100.0, 100.0), Z]),
		(
			"M100,200 C100,100 250,100 250,200 S400,300 400,200",
			vec![
				m(100.0, 200.0),
				c((100.0, 100.0), (250.0, 100.0), (250.0, 200.0)),
				c((250.0, 300.0), (400.0, 300.0), (400.0, 200.0)),
				E,
			],
		),
		(
			"M100,200 C100,100 400,100 400,200",
			vec![
				m(100.0, 200.0),
				c((100.0, 100.0), (400.0, 100.0), (400.0, 200.0)),
				E,
			],
		),
		(
			"M100,500 C25,400 475,400 400,500",
			vec![
				m(100.0, 500.0),
				c((25.0, 400.0), (475.0, 400.0), (400.0, 500.0)),
				E,
			],
		),
		(
			"M100,800 C175,700 325,700 400,800",
			vec![
				m(100.0, 800.0),
				c((175.0, 700.0), (325.0, 700.0), (400.0, 800.0)),
				E,
			],
		),
		(
			"M600,200 C675,100 975,100 900,200",
			vec![
				m(600.0, 200.0),
				c((675.0, 100.0), (975.0, 100.0), (900.0, 200.0)),
				E,
			],
		),
		(
			"M600,500 C600,350 900,650 900,500",
			vec![
				m(600.0, 500.0),
				c((600.0, 350.0), (900.0, 650.0), (900.0, 500.0)),
				E,
			],
		),
		(
			"M600,800 C625,700 725,700 750,800 S875,900 900,800",
			vec![
				m(600.0, 800.0),
				c((625.0, 700.0), (725.0, 700.0), (750.0, 800.0)),
				c((775.0, 900.0), (875.0, 900.0), (900.0, 800.0)),
				E,
			],
		),
		(
			"M200,300 Q400,50 600,300 T1000,300",
			vec![
				m(200.0, 300.0),
				q((400.0, 50.0), (600.0, 300.0)),
				q((800.0, 550.0), (1000.0, 300.0)),
				E,
			],
		),
		// End examples from SVG spec
		// Relative moveto
		(
			"M 0 0 L 50 20 m 50 80 L 300 100 L 200 300 z",
			vec![
				m(0.0, 0.0),
				l(50.0, 20.0),
				E,
				m(100.0, 100.0),
				l(300.0, 100.0),
				l(200.0, 300.0),
				l(100.0, 100.0),
				Z,
			],
		),
		// Initial smooth and relative curveTo
		(
			"M100,200 s 150,-100 150,0",
			vec![
				m(100.0, 200.0),
				c((100.0, 200.0), (250.0, 100.0), (250.0, 200.0)),
				E,
			],
		),
		// Initial smooth and relative qCurveTo
		(
			"M100,200 t 150,0",
			vec![m(100.0, 200.0), q((100.0, 200.0), (250.0, 200.0)), E],
		),
		// relative l command
		(
			"M 100 100 L 300 100 l -100 200 z",
			vec![
				m(100.0, 100.0),
				l(300.0, 100.0),
				l(200.0, 300.0),
				l(100.0, 100.0),
				Z,
			],
		),
		// relative q command
		(
			"M200,300 q200,-250 400,0",
			vec![m(200.0, 300.0), q((400.0, 50.0), (600.0, 300.0)), E],
		),
		// absolute H command
		(
			"M 100 100 H 300 L 200 300 z",
			vec![
				m(100.0, 100.0),
				l(300.0, 100.0),
				l(200.0, 300.0),
				l(100.0, 100.0),
				Z,
			],
		),
		// relative h command
		(
			"M 100 100 h 200 L 200 300 z",
			vec![
				m(100.0, 100.0),
				l(300.0, 100.0),
				l(200.0, 300.0),
				l(100.0, 100.0),
				Z,
			],
		),
		// absolute V command
		(
			"M 100 100 V 300 L 200 300 z",
			vec![
				m(100.0, 100.0),
				l(100.0, 300.0),
				l(200.0, 300.0),
				l(100.0, 100.0),
				Z,
			],
		),
		// relative v command
		(
			"M 100 100 v 200 L 200 300 z",
			vec![
				m(100.0, 100.0),
				l(100.0, 300.0),
				l(200.0, 300.0),
				l(100.0, 100.0),
				Z,
			],
		),
		// a drawto command following a closepath starts a new subpath at the
		// initial point of the just-closed subpath
		(
			"M0,0 L10,10 Z L5,5",
			vec![
				m(0.0, 0.0),
				l(10.0, 10.0),
				l(0.0, 0.0),
				Z,
				m(0.0, 0.0),
				l(5.0, 5.0),
				E,
			],
		),
		// a redundant Z before the drawto must not emit a second closePath
		// nor a spurious moveTo
		(
			"M0,0 L10,10 Z Z L5,5",
			vec![
				m(0.0, 0.0),
				l(10.0, 10.0),
				l(0.0, 0.0),
				Z,
				m(0.0, 0.0),
				l(5.0, 5.0),
				E,
			],
		),
		// a relative drawto after closepath is relative to the initial point
		// of the just-closed subpath
		(
			"M10,10 L20,20 Z l5,5",
			vec![
				m(10.0, 10.0),
				l(20.0, 20.0),
				l(10.0, 10.0),
				Z,
				m(10.0, 10.0),
				l(15.0, 15.0),
				E,
			],
		),
		// the implicitly-opened subpath can itself be closed
		(
			"M0,0 L10,0 L10,10 Z L5,5 Z",
			vec![
				m(0.0, 0.0),
				l(10.0, 0.0),
				l(10.0, 10.0),
				l(0.0, 0.0),
				Z,
				m(0.0, 0.0),
				l(5.0, 5.0),
				l(0.0, 0.0),
				Z,
			],
		),
		// S/T immediately after a closepath
		(
			"M0,0 C0,5 5,5 5,0 Z S10,5 10,0",
			vec![
				m(0.0, 0.0),
				c((0.0, 5.0), (5.0, 5.0), (5.0, 0.0)),
				l(0.0, 0.0),
				Z,
				m(0.0, 0.0),
				c((0.0, 0.0), (10.0, 5.0), (10.0, 0.0)),
				E,
			],
		),
		(
			"M0,0 Q2.5,5 5,0 Z T10,0",
			vec![
				m(0.0, 0.0),
				q((2.5, 5.0), (5.0, 0.0)),
				l(0.0, 0.0),
				Z,
				m(0.0, 0.0),
				q((0.0, 0.0), (10.0, 0.0)),
				E,
			],
		),
	];
	for (pathdef, expected) in cases {
		assert_eq!(record(pathdef), expected, "{pathdef}");
	}
}

#[test]
fn test_equivalent_paths() {
	for (pathdef1, pathdef2) in [
		// don't need spaces between numbers and commands
		("M 100 100 L 200 200", "M100 100L200 200"),
		// repeated implicit command
		(
			"M 100 200 L 200 100 L -100 -200",
			"M 100 200 L 200 100 -100 -200",
		),
		// don't need spaces before a minus-sign
		(
			"M100,200c10-5,20-10,30-20",
			"M 100 200 c 10 -5 20 -10 30 -20",
		),
		// closed paths have an implicit lineTo if they don't end on the same
		// point as the initial moveTo
		(
			"M 100 100 L 300 100 L 200 300 z",
			"M 100 100 L 300 100 L 200 300 L 100 100 z",
		),
	] {
		assert_eq!(record(pathdef1), record(pathdef2), "{pathdef1}");
	}
}

#[test]
fn test_closepath_ignores_floating_point_rounding() {
	let ops: Vec<&str> = record("M 0.1 0 l 0.2 0 l -0.2 0 z")
		.iter()
		.map(PenOp::operator)
		.collect();
	assert_eq!(ops, ["moveTo", "lineTo", "lineTo", "closePath"]);
}

#[test]
fn test_implicit_moveto_after_drift_suppressed_closepath() {
	// the drift this test relies on
	assert_ne!(0.1 + 0.2 - 0.2, 0.1);
	let value = record("M 0.1 0 l 0.2 0 l -0.2 0 z l 0.1 0.1");
	assert_eq!(value[3], Z);
	assert_eq!(value[4], m(0.1, 0.0));
	// the relative lineTo resolves against the exact initial point too
	assert_eq!(value[5], l(0.1 + 0.1, 0.1));
	assert_eq!(value[6], E);
}

#[test]
fn test_exponents() {
	// It can be e or E, the plus is optional, and a minimum of +/-3.4e38 must
	// be supported.
	assert_eq!(
		record("M-3.4e38 3.4E+38L-3.4E-38,3.4e-38"),
		[m(-3.4e38, 3.4e38), l(-3.4e-38, 3.4e-38), E]
	);
	assert_eq!(
		record("M-3e38 3E+38L-3E-38,3e-38"),
		[m(-3e38, 3e38), l(-3e-38, 3e-38), E]
	);
}

fn parse_err(pathdef: &str) -> (Error, Vec<PenOp>) {
	let mut pen = RecordingPen::new();
	let err = parse_path(pathdef, &mut pen, (0.0, 0.0)).expect_err("invalid path");
	(err, pen.value)
}

#[test]
fn test_invalid_implicit_command() {
	let (err, _) = parse_err("M 100 100 L 200 200 Z 100 200");
	assert!(matches!(err, Error::Value(_)));
	assert!(
		err.to_string().contains("Unallowed implicit command"),
		"{err}"
	);
	// Checked against fontTools.
	assert_eq!(
		err.to_string(),
		"Unallowed implicit command in M 100 100 L 200 200 Z 100 200, position 7"
	);
}

#[test]
fn test_too_few_coordinates() {
	for pathdef in [
		"M",
		"M0,0 L5",
		"M0,0 L1,1 2",
		"M0,0 H",
		"M0,0 V",
		"M0,0 C1,1 2,2",
		"M0,0 S1,1",
		"M0,0 Q1,1",
		"M0,0 T",
	] {
		let (err, _) = parse_err(pathdef);
		assert!(matches!(err, Error::Value(_)), "{pathdef}");
		assert!(err.to_string().contains("Not enough coordinates"), "{err}");
	}
	// Checked against fontTools.
	assert_eq!(
		parse_err("M0,0 L5").0.to_string(),
		"Not enough coordinates for command 'L' in path: 'M0,0 L5'"
	);
}

#[test]
fn test_path_must_start_with_moveto() {
	for pathdef in ["L 5 5", "Z", "5 5", "L 5 5 M 0 0"] {
		let (err, value) = parse_err(pathdef);
		assert!(
			err.to_string().contains("must start with a moveto"),
			"{err}"
		);
		assert_eq!(value, []);
	}
}

#[test]
fn test_initial_relative_moveto_with_current_pos() {
	// a caller-supplied current_pos affects an initial relative moveto...
	let mut pen = RecordingPen::new();
	parse_path("m 5 5", &mut pen, (10.0, 20.0)).expect("valid path");
	assert_eq!(pen.value, [m(15.0, 25.0), E]);

	// ...but does not legalize a leading drawto
	let mut pen = RecordingPen::new();
	let err = parse_path("l 5 5", &mut pen, (10.0, 20.0)).expect_err("invalid path");
	assert!(
		err.to_string().contains("must start with a moveto"),
		"{err}"
	);
	assert_eq!(pen.value, []);
}

#[test]
fn test_empty_path_is_noop() {
	let mut pen = RecordingPen::new();
	parse_path("", &mut pen, (0.0, 0.0)).expect("valid path");
	parse_path("   ", &mut pen, (0.0, 0.0)).expect("valid path");
	assert_eq!(pen.value, []);
}

#[test]
fn test_arc_to_cubic_bezier() {
	let result = record("M300,200 h-150 a150,150 0 1,0 150,-150 z");
	let expected = [
		m(300.0, 200.0),
		l(150.0, 200.0),
		c((150.0, 282.842), (217.157, 350.0), (300.0, 350.0)),
		c((382.842, 350.0), (450.0, 282.842), (450.0, 200.0)),
		c((450.0, 117.157), (382.842, 50.0), (300.0, 50.0)),
		l(300.0, 200.0),
		Z,
	];
	assert_eq!(result.len(), expected.len());
	for (a, b) in result.iter().zip(&expected) {
		assert_eq!(a.operator(), b.operator());
		let (pa, pb) = (a.points(), b.points());
		assert_eq!(pa.len(), pb.len());
		for (p1, p2) in pa.iter().flatten().zip(pb.iter().flatten()) {
			// `pytest.approx(rel=1e-5)`
			for (v1, v2) in [(p1.0, p2.0), (p1.1, p2.1)] {
				assert!((v1 - v2).abs() <= 1e-5 * v2.abs(), "{v1} != {v2}");
			}
		}
	}
}

/// A recorded call of [`ArcRecordingPen`].
#[derive(Debug, Clone, PartialEq)]
enum Op {
	Pen(PenOp),
	ArcTo(f64, f64, f64, bool, bool, Point),
}

/// `ArcRecordingPen`: a recording pen with an `arcTo` method.
#[derive(Debug, Default)]
struct ArcRecordingPen {
	value: Vec<Op>,
}

impl Pen for ArcRecordingPen {
	fn move_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.value.push(Op::Pen(PenOp::MoveTo(pt)));
		Ok(())
	}
	fn line_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.value.push(Op::Pen(PenOp::LineTo(pt)));
		Ok(())
	}
	fn curve_to(&mut self, points: &[Point]) -> fontoxide_pens::Result<()> {
		self.value.push(Op::Pen(PenOp::CurveTo(points.to_vec())));
		Ok(())
	}
	fn q_curve_to(
		&mut self,
		off_curves: &[Point],
		on_curve: Option<Point>,
	) -> fontoxide_pens::Result<()> {
		self.value
			.push(Op::Pen(PenOp::QCurveTo(off_curves.to_vec(), on_curve)));
		Ok(())
	}
	fn close_path(&mut self) -> fontoxide_pens::Result<()> {
		self.value.push(Op::Pen(PenOp::ClosePath));
		Ok(())
	}
	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		self.value.push(Op::Pen(PenOp::EndPath));
		Ok(())
	}
	fn add_component(
		&mut self,
		glyph_name: &str,
		transformation: Transform,
	) -> fontoxide_pens::Result<()> {
		self.value.push(Op::Pen(PenOp::AddComponent(
			glyph_name.to_owned(),
			transformation,
		)));
		Ok(())
	}
}

impl ArcPen for ArcRecordingPen {
	fn arc_to(
		&mut self,
		rx: f64,
		ry: f64,
		rotation: f64,
		arc_large: bool,
		arc_sweep: bool,
		end: Point,
	) -> fontoxide_pens::Result<()> {
		self.value
			.push(Op::ArcTo(rx, ry, rotation, arc_large, arc_sweep, end));
		Ok(())
	}
}

fn record_arcs(pathdef: &str) -> Vec<Op> {
	let mut pen = ArcRecordingPen::default();
	parse_path_arcs(pathdef, &mut pen, (0.0, 0.0)).expect("valid path");
	pen.value
}

#[test]
fn test_arc_pen_with_arcTo() {
	assert_eq!(
		record_arcs("M300,200 h-150 a150,150 0 1,0 150,-150 z"),
		[
			Op::Pen(m(300.0, 200.0)),
			Op::Pen(l(150.0, 200.0)),
			Op::ArcTo(150.0, 150.0, 0.0, true, false, (300.0, 50.0)),
			Op::Pen(l(300.0, 200.0)),
			Op::Pen(Z),
		]
	);
}

#[test]
fn test_arc_flags_without_spaces() {
	let cases = [
		(
			"M1-2A3-4-1.0 01.5.7",
			vec![
				Op::Pen(m(1.0, -2.0)),
				Op::ArcTo(3.0, 4.0, -1.0, false, true, (0.5, 0.7)),
				Op::Pen(E),
			],
		),
		(
			"M21.58 7.19a2.51 2.51 0 10-1.77-1.77",
			vec![
				Op::Pen(m(21.58, 7.19)),
				Op::ArcTo(2.51, 2.51, 0.0, true, false, (21.58 - 1.77, 7.19 - 1.77)),
				Op::Pen(E),
			],
		),
		(
			"M22 12a25.87 25.87 0 00-.42-4.81",
			vec![
				Op::Pen(m(22.0, 12.0)),
				Op::ArcTo(25.87, 25.87, 0.0, false, false, (22.0 - 0.42, 12.0 - 4.81)),
				Op::Pen(E),
			],
		),
		(
			"M0,0 A1.2 1.2 0 012 15.8",
			vec![
				Op::Pen(m(0.0, 0.0)),
				Op::ArcTo(1.2, 1.2, 0.0, false, true, (2.0, 15.8)),
				Op::Pen(E),
			],
		),
		(
			"M12 7a5 5 0 105 5 5 5 0 00-5-5",
			vec![
				Op::Pen(m(12.0, 7.0)),
				Op::ArcTo(5.0, 5.0, 0.0, true, false, (17.0, 12.0)),
				Op::ArcTo(5.0, 5.0, 0.0, false, false, (12.0, 7.0)),
				Op::Pen(E),
			],
		),
	];
	for (path, expected) in cases {
		assert_eq!(record_arcs(path), expected, "{path}");
	}
	// The expected values of the relative arcs are exact in Python too:
	// 21.58 - 1.77 == 19.81 and 7.19 - 1.77 == 5.42, 22 - 0.42 == 21.58 and
	// 12 - 4.81 == 7.19.
	assert_eq!((21.58 - 1.77, 7.19 - 1.77), (19.81, 5.42));
	assert_eq!((22.0 - 0.42, 12.0 - 4.81), (21.58, 7.19));
}

fn arc_err(pathdef: &str) -> Error {
	let mut pen = ArcRecordingPen::default();
	parse_path_arcs(pathdef, &mut pen, (0.0, 0.0)).expect_err("invalid arc")
}

#[test]
fn test_invalid_arc_not_enough_args() {
	for path in ["A", "A0,0,0,0,0,0", "A 0 0 0 0 0 0 0 0 0 0 0 0 0"] {
		let err = arc_err(path);
		assert!(err.to_string().contains("Invalid arc command"), "{err}");
		let Error::InvalidArc { cause, .. } = err else {
			panic!("not an arc error: {err}");
		};
		assert!(cause.0.contains("Not enough arguments"), "{cause}");
	}
}

#[test]
fn test_invalid_arc_argument_value() {
	let err = arc_err("M0,0 A0,0,0,2,0,0,0");
	assert!(err.to_string().contains("Invalid arc command"), "{err}");
	let Error::InvalidArc { cause, .. } = err else {
		panic!("not an arc error: {err}");
	};
	assert!(
		cause
			.0
			.contains("Invalid argument for 'large-arc-flag' parameter: '2'"),
		"{cause}"
	);

	let err = arc_err("M0,0 A0,0,0,0,-2.0,0,0");
	assert!(err.to_string().contains("Invalid arc command"), "{err}");
	let Error::InvalidArc { cause, .. } = err else {
		panic!("not an arc error: {err}");
	};
	assert!(
		cause
			.0
			.contains("Invalid argument for 'sweep-flag' parameter: '-2.0'"),
		"{cause}"
	);
}

/// UPSTREAM-BUG(B67): an arc with a zero radius draws nothing, where the SVG
/// specification draws a straight line (checked against fontTools).
#[test]
fn zero_radius_arc_draws_nothing() {
	assert_eq!(
		record("M0 0 A 0 5 0 0 1 10 10 L 20 20"),
		[m(0.0, 0.0), l(20.0, 20.0), E]
	);
}
