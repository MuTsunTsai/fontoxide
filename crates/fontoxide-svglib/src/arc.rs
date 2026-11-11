//! Converting SVG elliptical arcs to cubic Bézier curves
//! (`fontTools.svgLib.path.arc`), adapted from Blink's
//! `SVGPathNormalizer::DecomposeArcToCubic`.

use std::f64::consts::PI;

use fontoxide_misc::complex::Complex;
use fontoxide_misc::transform::{IDENTITY, Transform};
use fontoxide_pens::Pen;

use crate::{Error, Result};

const TWO_PI: f64 = 2.0 * PI;
const PI_OVER_TWO: f64 = 0.5 * PI;

/// Applies a transformation to a point represented as a complex number
/// (`_map_point`).
fn map_point(matrix: &Transform, pt: Complex) -> Complex {
	Complex::from_point(matrix.transform_point(pt.to_point()))
}

/// `max(x, 0.0)`: `x` unless `0.0 > x` (so NaN passes through).
fn py_max_zero(x: f64) -> f64 {
	if 0.0 > x { 0.0 } else { x }
}

/// An elliptical arc in endpoint parametrization (`EllipticalArc`).
#[derive(Debug, Clone, PartialEq)]
pub struct EllipticalArc {
	/// `current_point`.
	pub current_point: Complex,
	/// `rx`.
	pub rx: f64,
	/// `ry`.
	pub ry: f64,
	/// `rotation`, in degrees.
	pub rotation: f64,
	/// `large`.
	pub large: bool,
	/// `sweep`.
	pub sweep: bool,
	/// `target_point`.
	pub target_point: Complex,
	/// `angle`: the rotation in radians.
	pub angle: f64,
	/// The center parametrization, computed by [`EllipticalArc::parametrize`]:
	/// `center_point`, `theta1`, `theta2` and `theta_arc`.
	center: Option<(Complex, f64, f64, f64)>,
}

impl EllipticalArc {
	/// `EllipticalArc(current_point, rx, ry, rotation, large, sweep,
	/// target_point)`.
	pub fn new(
		current_point: Complex,
		rx: f64,
		ry: f64,
		rotation: f64,
		large: bool,
		sweep: bool,
		target_point: Complex,
	) -> Self {
		EllipticalArc {
			current_point,
			rx,
			ry,
			rotation,
			large,
			sweep,
			target_point,
			// SVG arc's rotation angle is expressed in degrees, whereas
			// Transform.rotate uses radians
			angle: rotation.to_radians(),
			center: None,
		}
	}

	/// The center point, if the arc has been parametrized.
	pub fn center_point(&self) -> Option<Complex> {
		self.center.map(|c| c.0)
	}

	/// Converts from endpoint to center parametrization (`_parametrize`);
	/// `false` if the arc is drawn as nothing.
	///
	/// UPSTREAM-BUG(B67): an arc with a zero radius is drawn as nothing, while the
	/// SVG specification treats it as a straight line to the end point.
	pub fn parametrize(&mut self) -> Result<bool> {
		// https://www.w3.org/TR/SVG/implnote.html#ArcConversionEndpointToCenter
		let mut rx = self.rx.abs();
		let mut ry = self.ry.abs();
		if rx == 0.0 || ry == 0.0 {
			return Ok(false);
		}
		// If the current point and target point for the arc are identical, it
		// should be treated as a zero length path.
		if self.target_point == self.current_point {
			return Ok(false);
		}
		let mid_point_distance = (self.current_point - self.target_point) * 0.5;
		let point_transform = IDENTITY.rotate(-self.angle);
		let transformed_mid_point = map_point(&point_transform, mid_point_distance);
		let square_rx = rx * rx;
		let square_ry = ry * ry;
		let square_x = transformed_mid_point.re * transformed_mid_point.re;
		let square_y = transformed_mid_point.im * transformed_mid_point.im;

		// Check if the radii are big enough to draw the arc, scale radii if not.
		let radii_scale = square_x / square_rx + square_y / square_ry;
		if radii_scale > 1.0 {
			rx *= radii_scale.sqrt();
			ry *= radii_scale.sqrt();
			self.rx = rx;
			self.ry = ry;
		}

		// `Scale(1 / rx, 1 / ry).rotate(-self.angle)`
		let point_transform =
			Transform::new(1.0 / rx, 0.0, 0.0, 1.0 / ry, 0.0, 0.0).rotate(-self.angle);
		let mut point1 = map_point(&point_transform, self.current_point);
		let mut point2 = map_point(&point_transform, self.target_point);
		let mut delta = point2 - point1;

		let d = delta.re * delta.re + delta.im * delta.im;
		if d == 0.0 {
			return Err(Error::Value("float division by zero".into()));
		}
		let scale_factor_squared = py_max_zero(1.0 / d - 0.25);
		let mut scale_factor = scale_factor_squared.sqrt();
		if self.sweep == self.large {
			scale_factor = -scale_factor;
		}
		delta = delta * scale_factor;
		let mut center_point = (point1 + point2) * 0.5;
		center_point += Complex::new(-delta.im, delta.re);
		point1 = point1 - center_point;
		point2 = point2 - center_point;

		let theta1 = point1.im.atan2(point1.re);
		let theta2 = point2.im.atan2(point2.re);
		let mut theta_arc = theta2 - theta1;
		if theta_arc < 0.0 && self.sweep {
			theta_arc += TWO_PI;
		} else if theta_arc > 0.0 && !self.sweep {
			theta_arc -= TWO_PI;
		}
		self.center = Some((center_point, theta1, theta1 + theta_arc, theta_arc));
		Ok(true)
	}

	/// The cubic Bézier curves of the arc (`_decompose_to_cubic_curves`): the
	/// two control points and the end point of each.
	pub fn decompose_to_cubic_curves(&mut self) -> Result<Vec<(Complex, Complex, Complex)>> {
		if self.center.is_none() && !self.parametrize()? {
			return Ok(Vec::new());
		}
		let Some((center_point, theta1, _, theta_arc)) = self.center else {
			return Ok(Vec::new());
		};
		let point_transform = IDENTITY.rotate(self.angle).scale(self.rx, Some(self.ry));

		// Some results of atan2 on some platform implementations are not exact
		// enough. So that we get more cubic curves than expected here. Adding
		// 0.001f reduces the count of sgements to the correct count.
		let segments = (theta_arc / (PI_OVER_TWO + 0.001)).abs().ceil();
		if !segments.is_finite() {
			return Err(Error::Value(if segments.is_nan() {
				"cannot convert float NaN to integer".into()
			} else {
				"cannot convert float infinity to integer".into()
			}));
		}
		let num_segments = segments as u64;
		let mut out = Vec::new();
		for i in 0..num_segments {
			let start_theta = theta1 + i as f64 * theta_arc / num_segments as f64;
			let end_theta = theta1 + (i + 1) as f64 * theta_arc / num_segments as f64;

			let t = (4.0 / 3.0) * (0.25 * (end_theta - start_theta)).tan();
			if !t.is_finite() {
				return Ok(out);
			}

			let sin_start_theta = start_theta.sin();
			let cos_start_theta = start_theta.cos();
			let sin_end_theta = end_theta.sin();
			let cos_end_theta = end_theta.cos();

			let mut point1 = Complex::new(
				cos_start_theta - t * sin_start_theta,
				sin_start_theta + t * cos_start_theta,
			);
			point1 += center_point;
			let mut target_point = Complex::new(cos_end_theta, sin_end_theta);
			target_point += center_point;
			let mut point2 = target_point;
			point2 += Complex::new(t * sin_end_theta, -t * cos_end_theta);

			out.push((
				map_point(&point_transform, point1),
				map_point(&point_transform, point2),
				map_point(&point_transform, target_point),
			));
		}
		Ok(out)
	}

	/// Draws the arc as cubic curves (`draw`).
	pub fn draw(&mut self, pen: &mut dyn Pen) -> Result<()> {
		for (point1, point2, target_point) in self.decompose_to_cubic_curves()? {
			pen.curve_to(&[
				point1.to_point(),
				point2.to_point(),
				target_point.to_point(),
			])?;
		}
		Ok(())
	}
}
