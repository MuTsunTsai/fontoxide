//! Bounds of outlines (`fontTools.pens.boundsPen.BoundsPen`, with
//! `misc.bezierTools.calcCubicBounds` and `misc.arrayTools`).

use super::charstring::Pen;
use crate::Result;

/// A rectangle: xMin, yMin, xMax, yMax.
pub type Rect = [f64; 4];

const EPSILON: f64 = 1e-10;

/// `updateBounds`.
pub fn update_bounds(bounds: Option<Rect>, (x, y): (f64, f64)) -> Rect {
	match bounds {
		None => [x, y, x, y],
		Some(b) => [
			py_min(b[0], x),
			py_min(b[1], y),
			py_max(b[2], x),
			py_max(b[3], y),
		],
	}
}

/// Python's `min(a, b)`: `a` unless `b` is smaller.
fn py_min(a: f64, b: f64) -> f64 {
	if b < a { b } else { a }
}

/// Python's `max(a, b)`: `a` unless `b` is larger.
fn py_max(a: f64, b: f64) -> f64 {
	if b > a { b } else { a }
}

/// `pointInRect`.
fn point_in_rect((x, y): (f64, f64), r: Rect) -> bool {
	r[0] <= x && x <= r[2] && r[1] <= y && y <= r[3]
}

/// `unionRect`.
pub fn union_rect(a: Rect, b: Rect) -> Rect {
	[
		py_min(a[0], b[0]),
		py_min(a[1], b[1]),
		py_max(a[2], b[2]),
		py_max(a[3], b[3]),
	]
}

/// `intRect`: the smallest integer rectangle around `r`.
pub fn int_rect(r: Rect) -> [i64; 4] {
	[
		r[0].floor() as i64,
		r[1].floor() as i64,
		r[2].ceil() as i64,
		r[3].ceil() as i64,
	]
}

/// `solveQuadratic`.
fn solve_quadratic(a: f64, b: f64, c: f64) -> Vec<f64> {
	if a.abs() < EPSILON {
		if b.abs() < EPSILON {
			Vec::new()
		} else {
			vec![-c / b]
		}
	} else {
		let dd = b * b - 4.0 * a * c;
		if dd >= 0.0 {
			let r = dd.sqrt();
			vec![(-b + r) / 2.0 / a, (-b - r) / 2.0 / a]
		} else {
			Vec::new()
		}
	}
}

/// `calcCubicBounds`.
pub fn calc_cubic_bounds(p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), p4: (f64, f64)) -> Rect {
	// `calcCubicParameters`.
	let (dx, dy) = p1;
	let cx = (p2.0 - dx) * 3.0;
	let cy = (p2.1 - dy) * 3.0;
	let bx = (p3.0 - p2.0) * 3.0 - cx;
	let by = (p3.1 - p2.1) * 3.0 - cy;
	let ax = p4.0 - dx - cx - bx;
	let ay = p4.1 - dy - cy - by;
	let in_range = |t: &f64| (0.0..1.0).contains(t);
	let mut roots: Vec<f64> = solve_quadratic(ax * 3.0, bx * 2.0, cx)
		.into_iter()
		.filter(in_range)
		.collect();
	roots.extend(
		solve_quadratic(ay * 3.0, by * 2.0, cy)
			.into_iter()
			.filter(in_range),
	);
	let mut points: Vec<(f64, f64)> = roots
		.iter()
		.map(|&t| {
			(
				ax * t * t * t + bx * t * t + cx * t + dx,
				ay * t * t * t + by * t * t + cy * t + dy,
			)
		})
		.collect();
	points.push(p1);
	points.push(p4);
	calc_bounds(&points)
}

/// `calcBounds`.
fn calc_bounds(points: &[(f64, f64)]) -> Rect {
	let (x0, y0) = points[0];
	points[1..].iter().fold([x0, y0, x0, y0], |b, &(x, y)| {
		[
			py_min(b[0], x),
			py_min(b[1], y),
			py_max(b[2], x),
			py_max(b[3], y),
		]
	})
}

/// Collects the bounds of what is drawn (`BoundsPen`). Components are recorded
/// for the caller to draw.
#[derive(Debug, Default)]
pub struct BoundsPen {
	/// The bounds so far.
	pub bounds: Option<Rect>,
	current: Option<(f64, f64)>,
	/// The components to draw, with their offsets.
	pub components: Vec<(String, (f64, f64))>,
	/// The offset applied to drawn points (`TransformPen`).
	pub offset: (f64, f64),
}

impl BoundsPen {
	fn transform(&self, (x, y): (f64, f64)) -> (f64, f64) {
		if self.offset == (0.0, 0.0) {
			(x, y)
		} else {
			// `Transform(1, 0, 0, 1, dx, dy).transformPoint`.
			(
				x * 1.0 + y * 0.0 + self.offset.0,
				x * 0.0 + y * 1.0 + self.offset.1,
			)
		}
	}
}

impl Pen for BoundsPen {
	fn move_to(&mut self, p: (f64, f64)) {
		let p = self.transform(p);
		self.bounds = Some(update_bounds(self.bounds, p));
		self.current = Some(p);
	}

	fn line_to(&mut self, p: (f64, f64)) {
		let p = self.transform(p);
		self.bounds = Some(update_bounds(self.bounds, p));
		self.current = Some(p);
	}

	fn curve_to(&mut self, p1: (f64, f64), p2: (f64, f64), p3: (f64, f64)) {
		let (p1, p2, p3) = (self.transform(p1), self.transform(p2), self.transform(p3));
		let mut b = update_bounds(self.bounds, p3);
		if !point_in_rect(p1, b) || !point_in_rect(p2, b) {
			let start = self.current.unwrap_or((0.0, 0.0));
			b = union_rect(b, calc_cubic_bounds(start, p1, p2, p3));
		}
		self.bounds = Some(b);
		self.current = Some(p3);
	}

	fn close_path(&mut self) {
		self.current = None;
	}

	fn add_component(&mut self, glyph: &str, offset: (f64, f64)) -> Result<()> {
		let offset = (offset.0 + self.offset.0, offset.1 + self.offset.1);
		self.components.push((glyph.to_owned(), offset));
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// Cases from fontTools' bezierTools doctests.
	#[test]
	fn cubic_bounds() {
		assert_eq!(
			calc_cubic_bounds((0.0, 0.0), (25.0, 100.0), (75.0, 100.0), (100.0, 0.0)),
			[0.0, 0.0, 100.0, 75.0]
		);
		let b = calc_cubic_bounds((50.0, 0.0), (0.0, 100.0), (100.0, 100.0), (50.0, 0.0));
		assert_eq!(
			format!("{:.6} {:.6} {:.6} {:.6}", b[0], b[1], b[2], b[3]),
			"35.566243 0.000000 64.433757 75.000000"
		);
	}
}
