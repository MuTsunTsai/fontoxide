//! Affine transformations (`fontTools.misc.transform`).
//!
//! The arithmetic follows fontTools operation by operation, so that results
//! are bit-identical.

use crate::array_tools::Point;

const EPSILON: f64 = 1e-15;
const ONE_EPSILON: f64 = 1.0 - EPSILON;
const MINUS_ONE_EPSILON: f64 = -1.0 + EPSILON;

/// `_normSinCos`: snaps values very close to 0, 1 or -1.
fn norm_sin_cos(v: f64) -> f64 {
	if v.abs() < EPSILON {
		0.0
	} else if v > ONE_EPSILON {
		1.0
	} else if v < MINUS_ONE_EPSILON {
		-1.0
	} else {
		v
	}
}

/// A 2×2 matrix plus an offset (`Transform`). Transforms are immutable: the
/// transforming methods return a new one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
	#[allow(missing_docs)]
	pub xx: f64,
	#[allow(missing_docs)]
	pub xy: f64,
	#[allow(missing_docs)]
	pub yx: f64,
	#[allow(missing_docs)]
	pub yy: f64,
	#[allow(missing_docs)]
	pub dx: f64,
	#[allow(missing_docs)]
	pub dy: f64,
}

/// The identity transformation (`Identity`).
pub const IDENTITY: Transform = Transform::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);

impl Default for Transform {
	fn default() -> Self {
		IDENTITY
	}
}

impl From<[f64; 6]> for Transform {
	fn from([xx, xy, yx, yy, dx, dy]: [f64; 6]) -> Self {
		Transform::new(xx, xy, yx, yy, dx, dy)
	}
}

impl Transform {
	/// `Transform(xx, xy, yx, yy, dx, dy)`.
	pub const fn new(xx: f64, xy: f64, yx: f64, yy: f64, dx: f64, dy: f64) -> Self {
		Transform {
			xx,
			xy,
			yx,
			yy,
			dx,
			dy,
		}
	}

	/// `Offset(x, y)`: a translation.
	pub const fn offset(x: f64, y: f64) -> Self {
		Transform::new(1.0, 0.0, 0.0, 1.0, x, y)
	}

	/// The six values `[xx, xy, yx, yy, dx, dy]`.
	pub fn to_array(self) -> [f64; 6] {
		[self.xx, self.xy, self.yx, self.yy, self.dx, self.dy]
	}

	/// Whether this is the identity (`not transform`).
	pub fn is_identity(&self) -> bool {
		*self == IDENTITY
	}

	/// `transformPoint`.
	pub fn transform_point(&self, (x, y): Point) -> Point {
		(
			self.xx * x + self.yx * y + self.dx,
			self.xy * x + self.yy * y + self.dy,
		)
	}

	/// `transformPoints`.
	pub fn transform_points(&self, points: &[Point]) -> Vec<Point> {
		points.iter().map(|&p| self.transform_point(p)).collect()
	}

	/// `Scale(x, y)`: a scaling; `y` defaults to `x`.
	pub fn scaling(x: f64, y: Option<f64>) -> Self {
		Transform::new(x, 0.0, 0.0, y.unwrap_or(x), 0.0, 0.0)
	}

	/// `transformVector`: the point transformed without the offset.
	pub fn transform_vector(&self, (dx, dy): Point) -> Point {
		(self.xx * dx + self.yx * dy, self.xy * dx + self.yy * dy)
	}

	/// `transformVectors`.
	pub fn transform_vectors(&self, vectors: &[Point]) -> Vec<Point> {
		vectors.iter().map(|&v| self.transform_vector(v)).collect()
	}

	/// `toPS()`: `[xx xy yx yy dx dy]`, formatted with `%s`.
	pub fn to_ps(&self) -> String {
		let v: Vec<String> = self.to_array().iter().map(|&x| py_num(x)).collect();
		format!("[{}]", v.join(" "))
	}

	/// `translate(x, y)`.
	pub fn translate(&self, x: f64, y: f64) -> Self {
		self.transform(&Transform::new(1.0, 0.0, 0.0, 1.0, x, y))
	}

	/// `scale(x, y)`; `y` defaults to `x`.
	pub fn scale(&self, x: f64, y: Option<f64>) -> Self {
		let y = y.unwrap_or(x);
		self.transform(&Transform::new(x, 0.0, 0.0, y, 0.0, 0.0))
	}

	/// `rotate(angle)`, in radians.
	pub fn rotate(&self, angle: f64) -> Self {
		let c = norm_sin_cos(angle.cos());
		let s = norm_sin_cos(angle.sin());
		self.transform(&Transform::new(c, s, -s, c, 0.0, 0.0))
	}

	/// `skew(x, y)`, in radians.
	pub fn skew(&self, x: f64, y: f64) -> Self {
		self.transform(&Transform::new(1.0, y.tan(), x.tan(), 1.0, 0.0, 0.0))
	}

	/// `transform(other)`: `other` applied first, then `self`.
	pub fn transform(&self, other: &Transform) -> Self {
		let (xx1, xy1, yx1, yy1, dx1, dy1) =
			(other.xx, other.xy, other.yx, other.yy, other.dx, other.dy);
		let (xx2, xy2, yx2, yy2, dx2, dy2) = (self.xx, self.xy, self.yx, self.yy, self.dx, self.dy);
		Transform::new(
			xx1 * xx2 + xy1 * yx2,
			xx1 * xy2 + xy1 * yy2,
			yx1 * xx2 + yy1 * yx2,
			yx1 * xy2 + yy1 * yy2,
			xx2 * dx1 + yx2 * dy1 + dx2,
			xy2 * dx1 + yy2 * dy1 + dy2,
		)
	}

	/// `reverseTransform(other)`: `self` applied first, then `other`.
	pub fn reverse_transform(&self, other: &Transform) -> Self {
		other.transform(self)
	}

	/// `inverse()`.
	pub fn inverse(&self) -> Self {
		if self.is_identity() {
			return *self;
		}
		let (xx, xy, yx, yy, dx, dy) = (self.xx, self.xy, self.yx, self.yy, self.dx, self.dy);
		let det = xx * yy - yx * xy;
		let (xx, xy, yx, yy) = (yy / det, -xy / det, -yx / det, xx / det);
		let (dx, dy) = (-xx * dx - yx * dy, -xy * dx - yy * dy);
		Transform::new(xx, xy, yx, yy, dx, dy)
	}

	/// `toDecomposed()`.
	pub fn to_decomposed(&self) -> DecomposedTransform {
		DecomposedTransform::from_transform(self)
	}
}

/// `str()` of a number that fontTools keeps as an `int` when it is integral.
fn py_num(x: f64) -> String {
	if x.fract() == 0.0 && x.abs() < 1e16 {
		format!("{}", x as i64)
	} else {
		crate::py::float_repr(x)
	}
}

/// `repr(transform)`: `<Transform [xx xy yx yy dx dy]>`, each with `%g`.
impl std::fmt::Display for Transform {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let v: Vec<String> = self
			.to_array()
			.iter()
			.map(|&x| crate::py::format_g_lower(x, 6))
			.collect();
		write!(f, "<Transform [{}]>", v.join(" "))
	}
}

/// A transformation as translate, rotation, scale, skew and center
/// (`DecomposedTransform`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecomposedTransform {
	#[allow(missing_docs)]
	pub translate_x: f64,
	#[allow(missing_docs)]
	pub translate_y: f64,
	/// In degrees, counter-clockwise.
	pub rotation: f64,
	#[allow(missing_docs)]
	pub scale_x: f64,
	#[allow(missing_docs)]
	pub scale_y: f64,
	/// In degrees, clockwise.
	pub skew_x: f64,
	/// In degrees, counter-clockwise.
	pub skew_y: f64,
	#[allow(missing_docs)]
	pub t_center_x: f64,
	#[allow(missing_docs)]
	pub t_center_y: f64,
}

impl Default for DecomposedTransform {
	fn default() -> Self {
		DecomposedTransform {
			translate_x: 0.0,
			translate_y: 0.0,
			rotation: 0.0,
			scale_x: 1.0,
			scale_y: 1.0,
			skew_x: 0.0,
			skew_y: 0.0,
			t_center_x: 0.0,
			t_center_y: 0.0,
		}
	}
}

impl DecomposedTransform {
	/// Whether this is the identity (`not decomposed`).
	pub fn is_identity(&self) -> bool {
		*self == DecomposedTransform::default()
	}

	/// `fromTransform`: always with `skew_y` 0 and the rotation in (-180, 180].
	pub fn from_transform(transform: &Transform) -> Self {
		let (mut a, mut b, c, d, x, y) = (
			transform.xx,
			transform.xy,
			transform.yx,
			transform.yy,
			transform.dx,
			transform.dy,
		);
		let sx = 1f64.copysign(a);
		if sx < 0.0 {
			a *= sx;
			b *= sx;
		}
		let delta = a * d - b * c;
		let mut rotation = 0.0;
		let (mut scale_x, mut scale_y) = (0.0, 0.0);
		let mut skew_x = 0.0;
		if a != 0.0 || b != 0.0 {
			let r = (a * a + b * b).sqrt();
			rotation = if b >= 0.0 {
				(a / r).acos()
			} else {
				-(a / r).acos()
			};
			(scale_x, scale_y) = (r, delta / r);
			skew_x = ((a * c + b * d) / (r * r)).atan();
		} else if c != 0.0 || d != 0.0 {
			let s = (c * c + d * d).sqrt();
			rotation = (-c).atan2(d);
			if rotation == -std::f64::consts::PI {
				rotation = std::f64::consts::PI;
			}
			(scale_x, scale_y) = (delta / s, s);
		}
		DecomposedTransform {
			translate_x: x,
			translate_y: y,
			rotation: rotation.to_degrees(),
			scale_x: scale_x * sx,
			scale_y,
			skew_x: skew_x.to_degrees() * sx,
			skew_y: 0.0,
			t_center_x: 0.0,
			t_center_y: 0.0,
		}
	}

	/// `toTransform()`.
	pub fn to_transform(&self) -> Transform {
		IDENTITY
			.translate(
				self.translate_x + self.t_center_x,
				self.translate_y + self.t_center_y,
			)
			.rotate(self.rotation.to_radians())
			.scale(self.scale_x, Some(self.scale_y))
			.skew(self.skew_x.to_radians(), self.skew_y.to_radians())
			.translate(-self.t_center_x, -self.t_center_y)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// Cases from fontTools' doctests.
	#[test]
	fn doctests() {
		let t = IDENTITY.scale(2.0, Some(3.0)).translate(4.0, 5.0);
		assert_eq!(t.transform_point((0.0, 0.0)), (8.0, 15.0));
		let t = Transform::new(2.0, 0.0, 0.0, 3.0, 0.0, 0.0);
		assert_eq!(t.transform_point((100.0, 100.0)), (200.0, 300.0));
		assert_eq!(
			t.transform(&Transform::new(4.0, 3.0, 2.0, 1.0, 5.0, 6.0)),
			Transform::new(8.0, 9.0, 4.0, 3.0, 10.0, 18.0)
		);
		assert_eq!(
			IDENTITY.rotate(std::f64::consts::PI / 2.0),
			Transform::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0)
		);
		assert_eq!(
			IDENTITY.skew(std::f64::consts::PI / 4.0, 0.0),
			Transform::new(1.0, 0.0, 0.9999999999999999, 1.0, 0.0, 0.0)
		);
		let t = Transform::new(2.0, 0.0, 0.0, 3.0, 8.0, 15.0);
		let it = t.inverse();
		assert_eq!(t.transform_point((10.0, 20.0)), (28.0, 75.0));
		assert_eq!(it.transform_point((28.0, 75.0)), (10.0, 20.0));
	}

	#[test]
	fn decomposed() {
		let d = DecomposedTransform {
			scale_x: 2.0,
			scale_y: 2.0,
			..Default::default()
		};
		assert_eq!(
			d.to_transform(),
			Transform::new(2.0, 0.0, 0.0, 2.0, 0.0, 0.0)
		);
		let d = DecomposedTransform::from_transform(&IDENTITY.scale(3.0, Some(2.0)));
		assert_eq!((d.rotation, d.scale_x, d.scale_y), (0.0, 3.0, 2.0));
		let d = DecomposedTransform::from_transform(&Transform::new(0.0, 0.0, 0.0, 1.0, 0.0, 0.0));
		assert_eq!((d.scale_x, d.scale_y), (0.0, 1.0));
		let d = DecomposedTransform::from_transform(&Transform::new(0.0, 0.0, 1.0, 1.0, 0.0, 0.0));
		assert_eq!(
			(d.rotation, d.scale_x, d.scale_y),
			(-45.0, 0.0, std::f64::consts::SQRT_2)
		);
	}
}
