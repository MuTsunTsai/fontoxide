//! Recordings written compactly: `M x y; L x y; C x y x y x y; Q x y … [_];
//! Z; E; A name xx xy yx yy dx dy` (`_` is a missing on-curve point).

#![allow(dead_code)]

use fontoxide_pens::recording_pen::PenOp;
use fontoxide_pens::{Point, Transform};

/// The calls of a compact recording.
pub fn ops(s: &str) -> Vec<PenOp> {
	s.split(';')
		.map(str::trim)
		.filter(|op| !op.is_empty())
		.map(|op| {
			let mut words = op.split_whitespace();
			let kind = words.next().unwrap_or_default();
			if kind == "A" {
				let name = words.next().unwrap_or_default().to_owned();
				let v: Vec<f64> = words.map(|w| w.parse().unwrap_or(f64::NAN)).collect();
				return PenOp::AddComponent(
					name,
					Transform::new(v[0], v[1], v[2], v[3], v[4], v[5]),
				);
			}
			let words: Vec<&str> = words.collect();
			let mut pts: Vec<Option<Point>> = Vec::new();
			let mut i = 0;
			while i < words.len() {
				if words[i] == "_" {
					pts.push(None);
					i += 1;
				} else {
					let x = words[i].parse().unwrap_or(f64::NAN);
					let y = words[i + 1].parse().unwrap_or(f64::NAN);
					pts.push(Some((x, y)));
					i += 2;
				}
			}
			let all =
				|pts: &[Option<Point>]| -> Vec<Point> { pts.iter().flatten().copied().collect() };
			match kind {
				"M" => PenOp::MoveTo(all(&pts)[0]),
				"L" => PenOp::LineTo(all(&pts)[0]),
				"C" => PenOp::CurveTo(all(&pts)),
				"Q" => {
					let on = pts.pop().flatten();
					PenOp::QCurveTo(all(&pts), on)
				}
				"Z" => PenOp::ClosePath,
				"E" => PenOp::EndPath,
				_ => panic!("unknown operator {kind}"),
			}
		})
		.collect()
}
