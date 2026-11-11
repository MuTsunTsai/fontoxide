//! `Tests/pens/qu2cuPen_test.py`. The UFO glyphs of `Tests/pens/data` are
//! recorded in `data/cu2qu_glyphs.txt` by `tools/reference/pen_fixtures.py`.

mod common;

use std::collections::BTreeMap;

use common::{approx_segments, glyphs, ops};
use fontoxide_pens::Result;
use fontoxide_pens::recording_pen::{PenOp, RecordingPen, replay_recording};
use fontoxide_pens::{Pen, Transform};
use fontoxide_qu2cu::pens::Qu2CuPen;

const MAX_ERR: f64 = 1.0;

/// `convert_glyph` with `Qu2CuPen(all_cubic=True)`.
fn qu2cu_segments(
	source: &[PenOp],
	reverse: bool,
	stats: Option<&mut BTreeMap<String, usize>>,
) -> Result<Vec<PenOp>> {
	let mut rec = RecordingPen::new();
	{
		let mut pen = Qu2CuPen::new(&mut rec, MAX_ERR, true, reverse);
		if stats.is_some() {
			pen.stats = Some(BTreeMap::new());
		}
		replay_recording(source, &mut pen)?;
		if let (Some(out), Some(got)) = (stats, pen.stats.take()) {
			for (k, v) in got {
				*out.entry(k).or_insert(0) += v;
			}
		}
	}
	Ok(rec.value)
}

#[test]
fn test_qu2cu_convert_glyphs() -> Result<()> {
	let cubic = glyphs("cubic");
	let quad = glyphs("quadratic");
	for name in ["a", "A"] {
		let converted = qu2cu_segments(&quad[name].segments, false, None)?;
		assert_ne!(converted, quad[name].segments);
		assert!(approx_segments(&converted, &cubic[name].segments), "{name}");
	}
	// Components do not change.
	assert_eq!(
		qu2cu_segments(&cubic["Aacute"].segments, false, None)?,
		cubic["Aacute"].segments
	);
	Ok(())
}

#[test]
fn test_qu2cu_reverse_direction_and_stats() -> Result<()> {
	let quad = glyphs("quadratic");
	for name in ["a", "A", "Eacute"] {
		let normal = qu2cu_segments(&quad[name].segments, false, None)?;
		let reversed = qu2cu_segments(&quad[name].segments, true, None)?;
		assert_ne!(normal, reversed);
	}
	let mut stats = BTreeMap::new();
	for g in quad.values() {
		qu2cu_segments(&g.segments, false, Some(&mut stats))?;
	}
	assert!(stats.contains_key("2"));

	let t = Transform::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
	let mut rec = RecordingPen::new();
	Qu2CuPen::new(&mut rec, MAX_ERR, false, false).add_component("a", t)?;
	assert_eq!(rec.value, [PenOp::AddComponent("a".into(), t)]);
	Ok(())
}

#[test]
fn test_qu2cu_pen_point_counts() -> Result<()> {
	for (input, expected) in [
		("M 0 0; Q 1 1; Z", "M 0 0; Q 1 1; Z"),
		("M 0 0; Q 1 1 2 2; Z", "M 0 0; Q 1 1 2 2; Z"),
		// Three points: no conversion.
		("M 0 0; Q 0 3 1 3 1 0; Z", "M 0 0; Q 0 3 1 3 1 0; Z"),
		("Q 0 0 1 0 1 1 0 1 _; Z", "Q 0 0 1 0 1 1 0 1 _; Z"),
		("M 0 0; C 1 1; Z", "M 0 0; C 1 1; Z"),
		("M 0 0; C 1 1 2 2; Z", "M 0 0; C 1 1 2 2; Z"),
		("M 0 0; C 1 1 2 2 3 3; Z", "M 0 0; C 1 1 2 2 3 3; Z"),
	] {
		let mut rec = RecordingPen::new();
		{
			let mut pen = Qu2CuPen::new(&mut rec, MAX_ERR, false, false);
			replay_recording(&ops(input), &mut pen)?;
		}
		assert_eq!(rec.value, ops(expected), "{input}");
	}
	Ok(())
}

#[test]
fn test_qu2cu_all_cubic() -> Result<()> {
	let input = ops(
		"M 1204 347; Q 1255 347 1323 433 1323 467; Q 1323 478 1310 492 1302 492; Q 1295 492 1289 484; \
		 L 1272 461; Q 1256 439 1221 416 1200 416; Q 1181 416 1141 440 1141 462; \
		 Q 1141 484 1190 565 1190 594; Q 1190 607 1181 634 1168 634; \
		 Q 1149 634 1146 583 1081 496 1081 463; Q 1081 417 1164 347 1204 347; Z",
	);
	let mut rec = RecordingPen::new();
	{
		let mut pen = Qu2CuPen::new(&mut rec, 1.0, true, false);
		replay_recording(&input, &mut pen)?;
	}
	assert!(rec.value.iter().all(|op| op.operator() != "qCurveTo"));
	Ok(())
}
