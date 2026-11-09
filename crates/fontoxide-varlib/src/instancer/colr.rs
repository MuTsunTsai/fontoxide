//! Port of `fontTools.varLib.instancer.colr`: instantiates the COLRv1 paint values
//! and their item variation store.

use fontoxide_misc::round_tools::ot_round;
use fontoxide_ttlib::binary::fit;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::types::{ClipBox, Paint};
use fontoxide_ttlib::ot::var_store;
use fontoxide_ttlib::ot::varc::NO_VARIATION_INDEX;
use fontoxide_ttlib::tables::colr::Colr;
use fontoxide_ttlib::{Error, Font, Result};

use super::item_store::{DefaultDeltas, instantiate_item_variation_store};
use super::limits::{NormalizedLimits, fvar_axes};

/// The converter of a fractional field.
#[derive(Clone, Copy, PartialEq)]
enum Fraction {
	/// `Fixed`.
	Fixed,
	/// `F2Dot14`.
	F2Dot14,
	/// `F2Dot14`, for the `Alpha` fields.
	Alpha,
	/// `Angle`: rotation and skew, which are periodic.
	Angle,
	/// `BiasedAngle`: sweep angles, which aren't.
	BiasedAngle,
}

struct Visitor<'a> {
	deltas: &'a DefaultDeltas,
	mapping: &'a [u64],
	is_static: bool,
}

impl Visitor<'_> {
	fn delta(&self, base: u32, offset: u64) -> Result<f64> {
		let base = u64::from(base);
		if base == NO_VARIATION_INDEX {
			return Ok(0.0);
		}
		let mut index = base + offset;
		if !self.mapping.is_empty() {
			index = self.mapping[(index as usize).min(self.mapping.len() - 1)];
		}
		self.deltas.get(index)
	}

	/// A `BaseFixedValue` field.
	fn fraction(&self, value: f64, kind: Fraction, base: u32, offset: u64) -> Result<f64> {
		let delta = self.delta(base, offset)?;
		const F2DOT14: f64 = 16384.0;
		Ok(match kind {
			Fraction::Fixed => ot_round(ot_round(value * 65536.0) as f64 + delta) as f64 / 65536.0,
			Fraction::F2Dot14 => {
				ot_round(ot_round(value * F2DOT14) as f64 + delta) as f64 / F2DOT14
			}
			Fraction::Alpha => {
				let mut bits = ot_round(ot_round(value * F2DOT14) as f64 + delta);
				if self.is_static {
					bits = bits.clamp(0, 1 << 14);
				}
				bits as f64 / F2DOT14
			}
			Fraction::Angle => {
				let bits = ot_round(ot_round(value / 180.0 * F2DOT14) as f64 + delta);
				let bits = (bits + 32768).rem_euclid(65536) - 32768;
				bits as f64 / F2DOT14 * 180.0
			}
			Fraction::BiasedAngle => {
				let bits = ot_round(ot_round((value / 180.0 - 1.0) * F2DOT14) as f64 + delta);
				(bits as f64 / F2DOT14 + 1.0) * 180.0
			}
		})
	}

	/// An integer field. Its value can be out of range for a while.
	fn int(&self, value: impl Into<i64>, base: u32, offset: u64) -> Result<i64> {
		Ok(value.into() + ot_round(self.delta(base, offset)?))
	}

	/// An `r0` or `r1` field.
	fn radius(&self, value: u16, base: u32, offset: u64) -> Result<i64> {
		let value = self.int(value, base, offset)?;
		Ok(if self.is_static {
			value
		} else {
			value.clamp(0, 65535)
		})
	}

	fn color_line(&self, line: &mut Option<Box<ot::VarColorLine>>) -> Result<()> {
		if let Some(line) = line {
			for stop in &mut line.color_stop {
				let base = stop.var_index_base;
				stop.stop_offset = self.fraction(stop.stop_offset, Fraction::F2Dot14, base, 0)?;
				stop.alpha = self.fraction(stop.alpha, Fraction::Alpha, base, 1)?;
			}
		}
		Ok(())
	}

	fn child(&self, paint: &mut Option<Box<Paint>>) -> Result<()> {
		match paint {
			Some(paint) => self.paint(paint),
			None => Ok(()),
		}
	}

	/// `visit`, for a paint: first the subtables, then the paint itself.
	#[allow(clippy::too_many_lines)]
	fn paint(&self, paint: &mut Paint) -> Result<()> {
		let is_static = self.is_static;
		let replacement = match paint {
			Paint::Format3(p) => {
				p.alpha = self.fraction(p.alpha, Fraction::Alpha, p.var_index_base, 0)?;
				is_static.then_some({
					Paint::Format2(ot::PaintFormat2 {
						palette_index: p.palette_index,
						alpha: p.alpha,
					})
				})
			}
			Paint::Format5(p) => {
				self.color_line(&mut p.color_line)?;
				let base = p.var_index_base;
				p.x0 = fit(self.int(p.x0, base, 0)?, "x0")?;
				p.y0 = fit(self.int(p.y0, base, 1)?, "y0")?;
				p.x1 = fit(self.int(p.x1, base, 2)?, "x1")?;
				p.y1 = fit(self.int(p.y1, base, 3)?, "y1")?;
				p.x2 = fit(self.int(p.x2, base, 4)?, "x2")?;
				p.y2 = fit(self.int(p.y2, base, 5)?, "y2")?;
				is_static.then(|| {
					Paint::Format4(ot::PaintFormat4 {
						color_line: no_var_line(p.color_line.take()),
						x0: p.x0,
						y0: p.y0,
						x1: p.x1,
						y1: p.y1,
						x2: p.x2,
						y2: p.y2,
					})
				})
			}
			Paint::Format7(p) => {
				self.color_line(&mut p.color_line)?;
				let base = p.var_index_base;
				let geometry = Radial {
					x0: self.int(p.x0, base, 0)?,
					y0: self.int(p.y0, base, 1)?,
					r0: self.radius(p.r0, base, 2)?,
					x1: self.int(p.x1, base, 3)?,
					y1: self.int(p.y1, base, 4)?,
					r1: self.radius(p.r1, base, 5)?,
				};
				if is_static {
					Some(geometry.instantiate(no_var_line(p.color_line.take()))?)
				} else {
					p.x0 = fit(geometry.x0, "x0")?;
					p.y0 = fit(geometry.y0, "y0")?;
					p.r0 = fit(geometry.r0, "r0")?;
					p.x1 = fit(geometry.x1, "x1")?;
					p.y1 = fit(geometry.y1, "y1")?;
					p.r1 = fit(geometry.r1, "r1")?;
					None
				}
			}
			Paint::Format9(p) => {
				self.color_line(&mut p.color_line)?;
				let base = p.var_index_base;
				p.center_x = fit(self.int(p.center_x, base, 0)?, "centerX")?;
				p.center_y = fit(self.int(p.center_y, base, 1)?, "centerY")?;
				p.start_angle = self.fraction(p.start_angle, Fraction::BiasedAngle, base, 2)?;
				p.end_angle = self.fraction(p.end_angle, Fraction::BiasedAngle, base, 3)?;
				is_static.then(|| {
					Paint::Format8(ot::PaintFormat8 {
						color_line: no_var_line(p.color_line.take()),
						center_x: p.center_x,
						center_y: p.center_y,
						start_angle: p.start_angle,
						end_angle: p.end_angle,
					})
				})
			}
			Paint::Format13(p) => {
				self.child(&mut p.paint)?;
				if let Some(t) = &mut p.transform {
					let base = t.var_index_base;
					t.xx = self.fraction(t.xx, Fraction::Fixed, base, 0)?;
					t.yx = self.fraction(t.yx, Fraction::Fixed, base, 1)?;
					t.xy = self.fraction(t.xy, Fraction::Fixed, base, 2)?;
					t.yy = self.fraction(t.yy, Fraction::Fixed, base, 3)?;
					t.dx = self.fraction(t.dx, Fraction::Fixed, base, 4)?;
					t.dy = self.fraction(t.dy, Fraction::Fixed, base, 5)?;
				}
				is_static.then(|| {
					Paint::Format12(ot::PaintFormat12 {
						paint: p.paint.take(),
						transform: p.transform.take().map(|t| {
							Box::new(ot::Affine2x3 {
								xx: t.xx,
								yx: t.yx,
								xy: t.xy,
								yy: t.yy,
								dx: t.dx,
								dy: t.dy,
							})
						}),
					})
				})
			}
			Paint::Format15(p) => {
				self.child(&mut p.paint)?;
				p.dx = fit(self.int(p.dx, p.var_index_base, 0)?, "dx")?;
				p.dy = fit(self.int(p.dy, p.var_index_base, 1)?, "dy")?;
				is_static.then(|| {
					Paint::Format14(ot::PaintFormat14 {
						paint: p.paint.take(),
						dx: p.dx,
						dy: p.dy,
					})
				})
			}
			Paint::Format17(p) => {
				self.child(&mut p.paint)?;
				let base = p.var_index_base;
				p.scale_x = self.fraction(p.scale_x, Fraction::F2Dot14, base, 0)?;
				p.scale_y = self.fraction(p.scale_y, Fraction::F2Dot14, base, 1)?;
				is_static.then(|| match scale_transform(p.scale_x, p.scale_y, 0, 0) {
					Some(transform) => Paint::Format12(ot::PaintFormat12 {
						paint: p.paint.take(),
						transform,
					}),
					None => Paint::Format16(ot::PaintFormat16 {
						paint: p.paint.take(),
						scale_x: p.scale_x,
						scale_y: p.scale_y,
					}),
				})
			}
			Paint::Format19(p) => {
				self.child(&mut p.paint)?;
				let base = p.var_index_base;
				p.scale_x = self.fraction(p.scale_x, Fraction::F2Dot14, base, 0)?;
				p.scale_y = self.fraction(p.scale_y, Fraction::F2Dot14, base, 1)?;
				let center_x = self.int(p.center_x, base, 2)?;
				let center_y = self.int(p.center_y, base, 3)?;
				let transform = if is_static {
					scale_transform(p.scale_x, p.scale_y, center_x, center_y)
				} else {
					None
				};
				match transform {
					Some(transform) => Some(Paint::Format12(ot::PaintFormat12 {
						paint: p.paint.take(),
						transform,
					})),
					None => {
						p.center_x = fit(center_x, "centerX")?;
						p.center_y = fit(center_y, "centerY")?;
						is_static.then(|| {
							Paint::Format18(ot::PaintFormat18 {
								paint: p.paint.take(),
								scale_x: p.scale_x,
								scale_y: p.scale_y,
								center_x: p.center_x,
								center_y: p.center_y,
							})
						})
					}
				}
			}
			Paint::Format21(p) => {
				self.child(&mut p.paint)?;
				p.scale = self.fraction(p.scale, Fraction::F2Dot14, p.var_index_base, 0)?;
				is_static.then(|| match scale_transform(p.scale, p.scale, 0, 0) {
					Some(transform) => Paint::Format12(ot::PaintFormat12 {
						paint: p.paint.take(),
						transform,
					}),
					None => Paint::Format20(ot::PaintFormat20 {
						paint: p.paint.take(),
						scale: p.scale,
					}),
				})
			}
			Paint::Format23(p) => {
				self.child(&mut p.paint)?;
				let base = p.var_index_base;
				p.scale = self.fraction(p.scale, Fraction::F2Dot14, base, 0)?;
				let center_x = self.int(p.center_x, base, 1)?;
				let center_y = self.int(p.center_y, base, 2)?;
				let transform = if is_static {
					scale_transform(p.scale, p.scale, center_x, center_y)
				} else {
					None
				};
				match transform {
					Some(transform) => Some(Paint::Format12(ot::PaintFormat12 {
						paint: p.paint.take(),
						transform,
					})),
					None => {
						p.center_x = fit(center_x, "centerX")?;
						p.center_y = fit(center_y, "centerY")?;
						is_static.then(|| {
							Paint::Format22(ot::PaintFormat22 {
								paint: p.paint.take(),
								scale: p.scale,
								center_x: p.center_x,
								center_y: p.center_y,
							})
						})
					}
				}
			}
			Paint::Format25(p) => {
				self.child(&mut p.paint)?;
				p.angle = self.fraction(p.angle, Fraction::Angle, p.var_index_base, 0)?;
				is_static.then(|| {
					Paint::Format24(ot::PaintFormat24 {
						paint: p.paint.take(),
						angle: p.angle,
					})
				})
			}
			Paint::Format27(p) => {
				self.child(&mut p.paint)?;
				let base = p.var_index_base;
				p.angle = self.fraction(p.angle, Fraction::Angle, base, 0)?;
				p.center_x = fit(self.int(p.center_x, base, 1)?, "centerX")?;
				p.center_y = fit(self.int(p.center_y, base, 2)?, "centerY")?;
				is_static.then(|| {
					Paint::Format26(ot::PaintFormat26 {
						paint: p.paint.take(),
						angle: p.angle,
						center_x: p.center_x,
						center_y: p.center_y,
					})
				})
			}
			Paint::Format29(p) => {
				self.child(&mut p.paint)?;
				let base = p.var_index_base;
				p.x_skew_angle = self.fraction(p.x_skew_angle, Fraction::Angle, base, 0)?;
				p.y_skew_angle = self.fraction(p.y_skew_angle, Fraction::Angle, base, 1)?;
				is_static.then(|| {
					Paint::Format28(ot::PaintFormat28 {
						paint: p.paint.take(),
						x_skew_angle: p.x_skew_angle,
						y_skew_angle: p.y_skew_angle,
					})
				})
			}
			Paint::Format31(p) => {
				self.child(&mut p.paint)?;
				let base = p.var_index_base;
				p.x_skew_angle = self.fraction(p.x_skew_angle, Fraction::Angle, base, 0)?;
				p.y_skew_angle = self.fraction(p.y_skew_angle, Fraction::Angle, base, 1)?;
				p.center_x = fit(self.int(p.center_x, base, 2)?, "centerX")?;
				p.center_y = fit(self.int(p.center_y, base, 3)?, "centerY")?;
				is_static.then(|| {
					Paint::Format30(ot::PaintFormat30 {
						paint: p.paint.take(),
						x_skew_angle: p.x_skew_angle,
						y_skew_angle: p.y_skew_angle,
						center_x: p.center_x,
						center_y: p.center_y,
					})
				})
			}
			// Paints that aren't variable themselves.
			Paint::Format10(p) => {
				self.child(&mut p.paint)?;
				None
			}
			Paint::Format12(p) => {
				self.child(&mut p.paint)?;
				None
			}
			Paint::Format14(p) => {
				self.child(&mut p.paint)?;
				None
			}
			Paint::Format16(p) => {
				self.child(&mut p.paint)?;
				None
			}
			Paint::Format18(p) => {
				self.child(&mut p.paint)?;
				None
			}
			Paint::Format20(p) => {
				self.child(&mut p.paint)?;
				None
			}
			Paint::Format22(p) => {
				self.child(&mut p.paint)?;
				None
			}
			Paint::Format24(p) => {
				self.child(&mut p.paint)?;
				None
			}
			Paint::Format26(p) => {
				self.child(&mut p.paint)?;
				None
			}
			Paint::Format28(p) => {
				self.child(&mut p.paint)?;
				None
			}
			Paint::Format30(p) => {
				self.child(&mut p.paint)?;
				None
			}
			Paint::Format32(p) => {
				self.child(&mut p.source_paint)?;
				self.child(&mut p.backdrop_paint)?;
				None
			}
			Paint::Format33(p) => {
				self.child(&mut p.paint)?;
				None
			}
			_ => None,
		};
		if let Some(replacement) = replacement {
			*paint = replacement;
		}
		Ok(())
	}

	/// `visit`, for a clip box.
	fn clip_box(&self, clip_box: &mut ClipBox) -> Result<()> {
		let ClipBox::Format2(b) = clip_box else {
			return Ok(());
		};
		let base = b.var_index_base;
		let value = |v: i16, offset: u64, round: fn(f64) -> f64| -> Result<i16> {
			let delta = round(self.delta(base, offset)?);
			fit(i64::from(v) + ot_round(delta), "ClipBox")
		};
		b.x_min = value(b.x_min, 0, f64::floor)?;
		b.y_min = value(b.y_min, 1, f64::floor)?;
		b.x_max = value(b.x_max, 2, f64::ceil)?;
		b.y_max = value(b.y_max, 3, f64::ceil)?;
		if self.is_static {
			*clip_box = ClipBox::Format1(ot::ClipBoxFormat1 {
				x_min: b.x_min,
				y_min: b.y_min,
				x_max: b.x_max,
				y_max: b.y_max,
			});
		}
		Ok(())
	}
}

/// `NoVarType` of a color line.
fn no_var_line(line: Option<Box<ot::VarColorLine>>) -> Option<Box<ot::ColorLine>> {
	line.map(|line| {
		Box::new(ot::ColorLine {
			extend: line.extend,
			stop_count: line.stop_count,
			color_stop: line
				.color_stop
				.iter()
				.map(|s| ot::ColorStop {
					stop_offset: s.stop_offset,
					palette_index: s.palette_index,
					alpha: s.alpha,
				})
				.collect(),
		})
	})
}

/// `_instantiateScale`: an affine transform when a static scale no longer fits
/// `F2Dot14`.
#[allow(clippy::option_option)]
fn scale_transform(sx: f64, sy: f64, cx: i64, cy: i64) -> Option<Option<Box<ot::Affine2x3>>> {
	if (-2.0..2.0).contains(&sx) && (-2.0..2.0).contains(&sy) {
		return None;
	}
	Some(Some(Box::new(ot::Affine2x3 {
		xx: sx,
		yx: 0.0,
		xy: 0.0,
		yy: sy,
		dx: cx as f64 * (1.0 - sx),
		dy: cy as f64 * (1.0 - sy),
	})))
}

/// The geometry of a radial gradient, whose radii can be negative for a while.
struct Radial {
	x0: i64,
	y0: i64,
	r0: i64,
	x1: i64,
	y1: i64,
	r1: i64,
}

fn gcd(a: i64, b: i64) -> i64 {
	let (mut a, mut b) = (a.abs(), b.abs());
	while b != 0 {
		(a, b) = (b, a % b);
	}
	a
}

impl Radial {
	fn into_paint(self, color_line: Option<Box<ot::ColorLine>>) -> Result<Paint> {
		Ok(Paint::Format6(ot::PaintFormat6 {
			color_line,
			x0: fit(self.x0, "x0")?,
			y0: fit(self.y0, "y0")?,
			r0: fit(self.r0, "r0")?,
			x1: fit(self.x1, "x1")?,
			y1: fit(self.y1, "y1")?,
			r1: fit(self.r1, "r1")?,
		}))
	}

	/// `_instantiateRadialGradient`.
	fn instantiate(self, line: Option<Box<ot::ColorLine>>) -> Result<Paint> {
		const PAD: u8 = 0;
		const REFLECT: u8 = 2;
		let (r0, r1) = (self.r0, self.r1);
		if r0 >= 0 && r1 >= 0 {
			return self.into_paint(line);
		}
		let transparent = || {
			Paint::Format2(ot::PaintFormat2 {
				palette_index: 0xFFFF,
				alpha: 0.0,
			})
		};
		let line = line.ok_or_else(|| {
			Error::ttlib("AttributeError: 'NoneType' object has no attribute 'ColorStop'")
		})?;
		if r0 == r1 || line.color_stop.is_empty() {
			// Equal negative radii have no nonnegative-radius intersection.
			return Ok(transparent());
		}
		let dr = r1 - r0;
		let dr_f = dr as f64;
		let mut ordered = line.color_stop.clone();
		ordered.sort_by(|a, b| a.stop_offset.total_cmp(&b.stop_offset));
		let first = ordered[0].stop_offset;
		let last = ordered[ordered.len() - 1].stop_offset;
		// Fractional shifts must preserve both integer circles and F2Dot14 stops.
		let lattice = gcd(gcd(gcd(16384, self.x1 - self.x0), self.y1 - self.y0), dr) as f64;
		let (r0_f, x0, y0) = (r0 as f64, self.x0 as f64, self.y0 as f64);
		let (dx, dy) = ((self.x1 - self.x0) as f64, (self.y1 - self.y0) as f64);
		for shift in 0..16 {
			let b = f64::from(1 << shift);
			let (coarse, fine) = if dr > 0 {
				(
					(-r0_f / dr_f).ceil(),
					(-r0_f / dr_f * lattice).ceil() / lattice,
				)
			} else {
				(
					(r0_f / -dr_f).floor() - b,
					(r0_f / -dr_f * lattice).floor() / lattice - b,
				)
			};
			for a in [coarse, fine] {
				// omega = a + b*q, with positive b, preserves the largest valid root.
				let geometry = [
					x0 + a * dx,
					y0 + a * dy,
					r0_f + a * dr_f,
					x0 + (a + b) * dx,
					y0 + (a + b) * dy,
					r0_f + (a + b) * dr_f,
				];
				if [0, 1, 3, 4]
					.iter()
					.any(|&i| !(-32768.0..=32767.0).contains(&geometry[i]))
					|| [2, 5]
						.iter()
						.any(|&i| !(0.0..=65535.0).contains(&geometry[i]))
				{
					continue;
				}
				let mut stops: &[ot::ColorStop] = &ordered;
				let mut offsets: Vec<f64> = stops.iter().map(|s| (s.stop_offset - a) / b).collect();
				let tip = -geometry[2] / (b * dr_f);
				if line.extend == PAD {
					if dr > 0 && offsets[offsets.len() - 1] <= tip {
						stops = &ordered[ordered.len() - 1..];
						offsets = vec![0.0];
					} else if dr < 0 && offsets[0] > tip {
						stops = &ordered[..1];
						offsets = vec![0.0];
					}
				} else {
					let mut period = (last - first) / b;
					if line.extend == REFLECT {
						period *= 2.0;
					}
					if period <= 0.0 {
						return Ok(transparent());
					}
					// Moving a repeating color line by whole periods changes no colors.
					let n = ((-2.0 - offsets[0]) / period).ceil();
					if n > ((32767.0 / 16384.0 - offsets[offsets.len() - 1]) / period).floor() {
						continue;
					}
					for offset in &mut offsets {
						*offset += n * period;
					}
				}
				if offsets.iter().any(|offset| {
					let v = offset * 16384.0;
					!(-32768.0..=32767.0).contains(&v) || v != ot_round(v) as f64
				}) {
					continue;
				}
				// Color lines can be shared by gradients with different geometries.
				let color_stop: Vec<ot::ColorStop> = stops
					.iter()
					.zip(&offsets)
					.map(|(stop, &stop_offset)| ot::ColorStop {
						stop_offset,
						..stop.clone()
					})
					.collect();
				let color_line = Some(Box::new(ot::ColorLine {
					extend: line.extend,
					stop_count: Some(color_stop.len() as u32),
					color_stop,
				}));
				return Radial {
					x0: geometry[0] as i64,
					y0: geometry[1] as i64,
					r0: geometry[2] as i64,
					x1: geometry[3] as i64,
					y1: geometry[4] as i64,
					r1: geometry[5] as i64,
				}
				.into_paint(color_line);
			}
		}
		Err(Error::value(
			"COLR radial gradient with negative radii cannot be represented without rounding",
		))
	}
}

/// `instantiateCOLR`.
pub(crate) fn instantiate_colr(font: &mut Font, limits: &NormalizedLimits) -> Result<()> {
	let axes: Vec<String> = fvar_axes(font)?.into_iter().map(|a| a.0).collect();
	let Some(colr) = font.get::<Colr>()? else {
		return Ok(());
	};
	if colr.version < 1 {
		return Ok(());
	}
	let Some(colr) = colr.table.as_mut() else {
		return Ok(());
	};
	let Some(store) = colr.var_store.as_deref_mut() else {
		return Ok(());
	};
	let deltas = instantiate_item_variation_store(store, &axes, limits)?;
	let is_static = !store
		.var_data
		.iter()
		.flatten()
		.any(|data| data.item.iter().any(|row| row.iter().any(|&d| d != 0)));
	let mapping: Vec<u64> = colr
		.var_index_map
		.as_ref()
		.map_or_else(Vec::new, |m| m.mapping.clone());
	let visitor = Visitor {
		deltas: &deltas,
		mapping: &mapping,
		is_static,
	};
	if let Some(list) = &mut colr.base_glyph_list {
		for record in &mut list.base_glyph_paint_record {
			visitor.child(&mut record.paint)?;
		}
	}
	if let Some(list) = &mut colr.layer_list {
		for paint in &mut list.paint {
			visitor.child(paint)?;
		}
	}
	if let Some(list) = &mut colr.clip_list {
		for clip_box in list.clips.values_mut() {
			visitor.clip_box(clip_box)?;
		}
	}
	if is_static {
		colr.var_store = None;
		colr.var_index_map = None;
	} else if !mapping.is_empty()
		&& let Some(store) = &mut colr.var_store
	{
		// An explicit map keeps VarIndexBase sequences valid when rows move.
		let map = var_store::optimize(store, true)?;
		let new = mapping
			.iter()
			.map(|index| {
				map.get(index)
					.copied()
					.ok_or_else(|| Error::ttlib(format!("KeyError: {index}")))
			})
			.collect::<Result<Vec<u64>>>()?;
		if let Some(index_map) = &mut colr.var_index_map {
			index_map.mapping = new;
		}
	}
	Ok(())
}
