//! Port of fontTools' `Tests/pens/reverseContourPen_test.py`.

mod common;

use common::ops;
use fontoxide_pens::point_pen::ReverseContourPointPen;
use fontoxide_pens::recording_pen::{RecordingPen, replay_recording};
use fontoxide_pens::reverse_contour_pen::ReverseContourPen;
use fontoxide_pens::{Pen, PointToSegmentPen, Result, SegmentToPointPen};

/// `TEST_DATA`: the contour, `outputImpliedClosingLine`, and the expected
/// result. Transcribed from fontTools.
const TEST_DATA: &[(&str, bool, &str)] = &[
	(
		"M 0.0 0.0; L 1.0 1.0; L 2.0 2.0; L 3.0 3.0; Z",
		false,
		"M 0.0 0.0; L 3.0 3.0; L 2.0 2.0; L 1.0 1.0; Z",
	),
	(
		"M 0.0 0.0; L 1.0 1.0; L 2.0 2.0; L 3.0 3.0; Z",
		true,
		"M 0.0 0.0; L 3.0 3.0; L 2.0 2.0; L 1.0 1.0; L 0.0 0.0; Z",
	),
	(
		"M 0.0 0.0; L 1.0 1.0; L 2.0 2.0; L 0.0 0.0; Z",
		false,
		"M 0.0 0.0; L 2.0 2.0; L 1.0 1.0; Z",
	),
	(
		"M 0.0 0.0; L 1.0 1.0; L 2.0 2.0; L 0.0 0.0; Z",
		true,
		"M 0.0 0.0; L 2.0 2.0; L 1.0 1.0; L 0.0 0.0; Z",
	),
	(
		"M 0.0 0.0; L 0.0 0.0; L 1.0 1.0; L 2.0 2.0; Z",
		false,
		"M 0.0 0.0; L 2.0 2.0; L 1.0 1.0; L 0.0 0.0; L 0.0 0.0; Z",
	),
	(
		"M 0.0 0.0; L 0.0 0.0; L 1.0 1.0; L 2.0 2.0; Z",
		true,
		"M 0.0 0.0; L 2.0 2.0; L 1.0 1.0; L 0.0 0.0; L 0.0 0.0; Z",
	),
	("M 0.0 0.0; L 1.0 1.0; Z", false, "M 0.0 0.0; L 1.0 1.0; Z"),
	(
		"M 0.0 0.0; L 1.0 1.0; Z",
		true,
		"M 0.0 0.0; L 1.0 1.0; L 0.0 0.0; Z",
	),
	(
		"M 0.0 0.0; C 1.0 1.0 2.0 2.0 3.0 3.0; C 4.0 4.0 5.0 5.0 0.0 0.0; Z",
		false,
		"M 0.0 0.0; C 5.0 5.0 4.0 4.0 3.0 3.0; C 2.0 2.0 1.0 1.0 0.0 0.0; Z",
	),
	(
		"M 0.0 0.0; C 1.0 1.0 2.0 2.0 3.0 3.0; C 4.0 4.0 5.0 5.0 0.0 0.0; Z",
		true,
		"M 0.0 0.0; C 5.0 5.0 4.0 4.0 3.0 3.0; C 2.0 2.0 1.0 1.0 0.0 0.0; Z",
	),
	(
		"M 0.0 0.0; C 1.0 1.0 2.0 2.0 3.0 3.0; C 4.0 4.0 5.0 5.0 6.0 6.0; Z",
		false,
		"M 0.0 0.0; L 6.0 6.0; C 5.0 5.0 4.0 4.0 3.0 3.0; C 2.0 2.0 1.0 1.0 0.0 0.0; Z",
	),
	(
		"M 0.0 0.0; C 1.0 1.0 2.0 2.0 3.0 3.0; C 4.0 4.0 5.0 5.0 6.0 6.0; Z",
		true,
		"M 0.0 0.0; L 6.0 6.0; C 5.0 5.0 4.0 4.0 3.0 3.0; C 2.0 2.0 1.0 1.0 0.0 0.0; Z",
	),
	(
		"M 0.0 0.0; L 1.0 1.0; C 2.0 2.0 3.0 3.0 4.0 4.0; C 5.0 5.0 6.0 6.0 7.0 7.0; Z",
		false,
		"M 0.0 0.0; L 7.0 7.0; C 6.0 6.0 5.0 5.0 4.0 4.0; C 3.0 3.0 2.0 2.0 1.0 1.0; Z",
	),
	(
		"M 0.0 0.0; L 1.0 1.0; C 2.0 2.0 3.0 3.0 4.0 4.0; C 5.0 5.0 6.0 6.0 7.0 7.0; Z",
		true,
		"M 0.0 0.0; L 7.0 7.0; C 6.0 6.0 5.0 5.0 4.0 4.0; C 3.0 3.0 2.0 2.0 1.0 1.0; L 0.0 0.0; Z",
	),
	(
		"M 0.0 0.0; Q 1.0 1.0 2.0 2.0; Q 3.0 3.0 0.0 0.0; Z",
		false,
		"M 0.0 0.0; Q 3.0 3.0 2.0 2.0; Q 1.0 1.0 0.0 0.0; Z",
	),
	(
		"M 0.0 0.0; Q 1.0 1.0 2.0 2.0; Q 3.0 3.0 0.0 0.0; Z",
		true,
		"M 0.0 0.0; Q 3.0 3.0 2.0 2.0; Q 1.0 1.0 0.0 0.0; Z",
	),
	(
		"M 0.0 0.0; Q 1.0 1.0 2.0 2.0; Q 3.0 3.0 4.0 4.0; Z",
		false,
		"M 0.0 0.0; L 4.0 4.0; Q 3.0 3.0 2.0 2.0; Q 1.0 1.0 0.0 0.0; Z",
	),
	(
		"M 0.0 0.0; Q 1.0 1.0 2.0 2.0; Q 3.0 3.0 4.0 4.0; Z",
		true,
		"M 0.0 0.0; L 4.0 4.0; Q 3.0 3.0 2.0 2.0; Q 1.0 1.0 0.0 0.0; Z",
	),
	(
		"M 0.0 0.0; L 1.0 1.0; Q 2.0 2.0 3.0 3.0; Z",
		false,
		"M 0.0 0.0; L 3.0 3.0; Q 2.0 2.0 1.0 1.0; Z",
	),
	(
		"A a 1.0 0.0 0.0 1.0 0.0 0.0",
		false,
		"A a 1.0 0.0 0.0 1.0 0.0 0.0",
	),
	("", false, ""),
	("M 0.0 0.0; E", false, "M 0.0 0.0; E"),
	("M 0.0 0.0; Z", false, "M 0.0 0.0; E"),
	("M 0.0 0.0; L 1.0 1.0; E", false, "M 1.0 1.0; L 0.0 0.0; E"),
	(
		"M 0.0 0.0; C 1.0 1.0 2.0 2.0 3.0 3.0; E",
		false,
		"M 3.0 3.0; C 2.0 2.0 1.0 1.0 0.0 0.0; E",
	),
	(
		"M 0.0 0.0; C 1.0 1.0 2.0 2.0 3.0 3.0; L 4.0 4.0; E",
		false,
		"M 4.0 4.0; L 3.0 3.0; C 2.0 2.0 1.0 1.0 0.0 0.0; E",
	),
	(
		"M 0.0 0.0; L 1.0 1.0; C 2.0 2.0 3.0 3.0 4.0 4.0; E",
		false,
		"M 4.0 4.0; C 3.0 3.0 2.0 2.0 1.0 1.0; L 0.0 0.0; E",
	),
	(
		"Q 0.0 0.0 1.0 1.0 2.0 2.0 _; Z",
		false,
		"Q 0.0 0.0 2.0 2.0 1.0 1.0 _; Z",
	),
	(
		"Q 0.0 0.0 1.0 1.0 2.0 2.0 _; E",
		false,
		"Q 0.0 0.0 2.0 2.0 1.0 1.0 _; Z",
	),
	(
		"M 848.0 348.0; L 848.0 348.0; Q 848.0 526.0 649.0 704.0 449.0 704.0; Q 449.0 704.0 248.0 704.0 50.0 526.0 50.0 348.0; L 50.0 348.0; Q 50.0 348.0 50.0 171.0 248.0 -3.0 449.0 -3.0; Q 449.0 -3.0 649.0 -3.0 848.0 171.0 848.0 348.0; Z",
		false,
		"M 848.0 348.0; Q 848.0 171.0 649.0 -3.0 449.0 -3.0 449.0 -3.0; Q 248.0 -3.0 50.0 171.0 50.0 348.0 50.0 348.0; L 50.0 348.0; Q 50.0 526.0 248.0 704.0 449.0 704.0 449.0 704.0; Q 649.0 704.0 848.0 526.0 848.0 348.0; L 848.0 348.0; Z",
	),
	(
		"M 0.0 651.0; L 0.0 101.0; L 0.0 101.0; L 0.0 651.0; L 0.0 651.0; Z",
		false,
		"M 0.0 651.0; L 0.0 651.0; L 0.0 101.0; L 0.0 101.0; Z",
	),
	(
		"M 0.0 651.0; L 0.0 101.0; L 0.0 101.0; L 0.0 651.0; L 0.0 651.0; Z",
		true,
		"M 0.0 651.0; L 0.0 651.0; L 0.0 101.0; L 0.0 101.0; L 0.0 651.0; Z",
	),
];

#[test]
fn test_reverse_pen() -> Result<()> {
	for &(contour, output_implied_closing_line, expected) in TEST_DATA {
		let mut rec = RecordingPen::new();
		{
			let mut rev = ReverseContourPen::new(&mut rec, output_implied_closing_line);
			replay_recording(&ops(contour), &mut rev)?;
		}
		assert_eq!(rec.value, ops(expected), "{contour}");
	}
	Ok(())
}

#[test]
fn test_reverse_pen_output_implied_closing_line() -> Result<()> {
	for (output_implied_closing_line, expected) in [
		(false, "M 0 0; L 0 10; L 10 0; Z"),
		(true, "M 0 0; L 0 10; L 10 0; L 0 0; Z"),
	] {
		let mut rec = RecordingPen::new();
		{
			let mut rev = ReverseContourPen::new(&mut rec, output_implied_closing_line);
			rev.move_to((0.0, 0.0))?;
			rev.line_to((10.0, 0.0))?;
			rev.line_to((0.0, 10.0))?;
			rev.line_to((0.0, 0.0))?;
			rev.close_path()?;
		}
		assert_eq!(rec.value, ops(expected));
	}
	Ok(())
}

#[test]
fn test_reverse_point_pen() -> Result<()> {
	for &(contour, output_implied_closing_line, expected) in TEST_DATA {
		let mut rec = RecordingPen::new();
		{
			let mut pt2seg = PointToSegmentPen::new(&mut rec, output_implied_closing_line);
			let mut rev = ReverseContourPointPen::new(&mut pt2seg);
			let mut seg2pt = SegmentToPointPen::new(&mut rev, true);
			replay_recording(&ops(contour), &mut seg2pt)?;
		}
		assert_eq!(rec.value, ops(expected), "{contour}");
	}
	Ok(())
}
