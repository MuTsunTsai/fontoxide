//! Helpers of the pen tests: compact recordings (`M x y; L x y; C x y x y x y;
//! Q x y … [_]; Z; E; A name xx xy yx yy dx dy`, `_` being a missing on-curve
//! point), and the UFO glyphs of fontTools' `Tests/pens/data`, recorded in
//! `data/cu2qu_glyphs.txt` by `tools/reference/pen_fixtures.py`.

#![allow(dead_code)]

use std::collections::BTreeMap;

use fontoxide_misc::py::is_close_tol;
use fontoxide_pens::recording_pen::{PenOp, PointOp};
use fontoxide_pens::{Point, SegmentType, Transform};

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

/// The recordings of a glyph: `draw` and `drawPoints`.
pub struct Glyph {
	pub segments: Vec<PenOp>,
	pub points: Vec<PointOp>,
}

pub fn point_ops(s: &str) -> Vec<PointOp> {
	s.split("; ")
		.filter(|op| !op.is_empty())
		.map(|op| {
			let w: Vec<&str> = op.split(' ').collect();
			match w[0] {
				"B" => PointOp::BeginPath(None),
				"E" => PointOp::EndPath,
				"P" => PointOp::AddPoint {
					pt: (
						w[1].parse().unwrap_or(f64::NAN),
						w[2].parse().unwrap_or(f64::NAN),
					),
					segment_type: SegmentType::parse(w[3]),
					smooth: w[4] == "1",
					name: (w[5] != "-").then(|| w[5].to_owned()),
					identifier: None,
				},
				_ => {
					let v: Vec<f64> = w[2..]
						.iter()
						.map(|s| s.parse().unwrap_or(f64::NAN))
						.collect();
					PointOp::AddComponent(
						w[1].to_owned(),
						Transform::new(v[0], v[1], v[2], v[3], v[4], v[5]),
						None,
					)
				}
			}
		})
		.collect()
}

/// `CUBIC_GLYPHS` (`"cubic"`) and `QUAD_GLYPHS` (`"quadratic"`).
pub fn glyphs(kind: &str) -> BTreeMap<String, Glyph> {
	let mut out: BTreeMap<String, Glyph> = BTreeMap::new();
	for line in include_str!("../data/cu2qu_glyphs.txt").lines() {
		let f: Vec<&str> = line.split('\t').collect();
		if f[0] != kind {
			continue;
		}
		let g = out.entry(f[1].to_owned()).or_insert(Glyph {
			segments: Vec::new(),
			points: Vec::new(),
		});
		if f[2] == "S" {
			g.segments = ops(f[3]);
		} else {
			g.points = point_ops(f[3]);
		}
	}
	out
}

pub fn close(a: Point, b: Point) -> bool {
	is_close_tol(a.0, b.0, 1e-12, 0.0) && is_close_tol(a.1, b.1, 1e-12, 0.0)
}

/// `DummyGlyph.approx`.
pub fn approx_segments(a: &[PenOp], b: &[PenOp]) -> bool {
	a.len() == b.len()
		&& a.iter().zip(b).all(|(x, y)| {
			let (px, py) = (x.points(), y.points());
			x.operator() == y.operator()
				&& px.len() == py.len()
				&& px.iter().zip(&py).all(|(p, q)| match (p, q) {
					(Some(p), Some(q)) => close(*p, *q),
					(None, None) => true,
					_ => false,
				}) && match (x, y) {
				(PenOp::AddComponent(n, t), PenOp::AddComponent(m, u)) => n == m && t == u,
				_ => true,
			}
		})
}

/// `DummyPointGlyph.approx`.
pub fn approx_points(a: &[PointOp], b: &[PointOp]) -> bool {
	a.len() == b.len()
		&& a.iter().zip(b).all(|(x, y)| match (x, y) {
			(
				PointOp::AddPoint {
					pt: p,
					segment_type: s1,
					smooth: m1,
					name: n1,
					..
				},
				PointOp::AddPoint {
					pt: q,
					segment_type: s2,
					smooth: m2,
					name: n2,
					..
				},
			) => close(*p, *q) && s1 == s2 && m1 == m2 && n1 == n2,
			_ => x == y,
		})
}
