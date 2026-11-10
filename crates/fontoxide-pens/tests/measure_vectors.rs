//! A differential check of the measuring pens (moments, statistics, area,
//! perimeter and bounds) against vectors made by
//! `tools/reference/pen_fixtures.py`, bit for bit.

mod common;

use common::ops;
use fontoxide_misc::py::float_repr;
use fontoxide_pens::area_pen::AreaPen;
use fontoxide_pens::moments_pen::MomentsPen;
use fontoxide_pens::perimeter_pen::PerimeterPen;
use fontoxide_pens::recording_pen::{PenOp, replay_recording};
use fontoxide_pens::statistics_pen::{Statistics, StatisticsControlPen, StatisticsPen};
use fontoxide_pens::{BoundsPen, ControlBoundsPen, Pen, Result};

fn fmt(values: Result<Vec<f64>>) -> String {
	match values {
		Ok(v) => {
			let s: Vec<String> = v.into_iter().map(float_repr).collect();
			s.join(" ")
		}
		Err(_) => "!E".into(),
	}
}

fn run<P: Pen>(mut pen: P, outline: &[PenOp], get: impl Fn(&P) -> Vec<f64>) -> Result<Vec<f64>> {
	replay_recording(outline, &mut pen)?;
	Ok(get(&pen))
}

fn stats(s: Statistics) -> Vec<f64> {
	vec![
		s.area,
		s.mean_x,
		s.mean_y,
		s.variance_x,
		s.variance_y,
		s.stddev_x,
		s.stddev_y,
		s.covariance,
		s.correlation,
		s.slant,
	]
}

fn bounds(b: Result<Option<[f64; 4]>>) -> String {
	match b {
		Ok(Some(b)) => fmt(Ok(b.to_vec())),
		Ok(None) | Err(_) => "None".into(),
	}
}

#[test]
fn measure_vectors() {
	let vectors = include_str!("data/measure_vectors.txt");
	let mut failures = Vec::new();
	for line in vectors.lines() {
		let f: Vec<&str> = line.split('\t').collect();
		let outline = ops(f[0]);
		let got = [
			fmt(run(MomentsPen::new_moments(None), &outline, |p| {
				let m = &p.inner;
				vec![
					m.area,
					m.moment_x,
					m.moment_y,
					m.moment_xx,
					m.moment_xy,
					m.moment_yy,
				]
			})),
			fmt(run(StatisticsPen::new_statistics(None), &outline, |p| {
				stats(p.statistics())
			})),
			fmt(run(
				StatisticsControlPen::new_statistics_control(None),
				&outline,
				|p| stats(p.statistics()),
			)),
			fmt(run(AreaPen::new_area(None), &outline, |p| vec![p.value()])),
			fmt(run(
				PerimeterPen::new_perimeter(None, 0.005),
				&outline,
				|p| vec![p.value()],
			)),
			fmt(run(
				PerimeterPen::new_perimeter(None, 0.0001),
				&outline,
				|p| vec![p.value()],
			)),
			bounds(
				BoundsPen::new_bounds(None)
					.pipe(|mut p| replay_recording(&outline, &mut p).map(|_| p.bounds())),
			),
			bounds(
				ControlBoundsPen::new_control_bounds(None, false)
					.pipe(|mut p| replay_recording(&outline, &mut p).map(|_| p.bounds())),
			),
		];
		let names = [
			"moments",
			"statistics",
			"control statistics",
			"area",
			"perimeter",
			"perimeter (exact)",
			"bounds",
			"control bounds",
		];
		for ((name, g), e) in names.iter().zip(&got).zip(&f[1..]) {
			if g != e {
				failures.push(format!(
					"{name}: {}\n  expected {e}\n  got      {g}",
					&f[0][..f[0].len().min(80)]
				));
			}
		}
	}
	assert!(
		failures.is_empty(),
		"{} failures in {} outlines:\n{}",
		failures.len(),
		vectors.lines().count(),
		failures[..failures.len().min(8)].join("\n")
	);
}

/// `x.pipe(f)`: `f(x)`.
trait Pipe: Sized {
	fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R {
		f(self)
	}
}

impl<T> Pipe for T {}
