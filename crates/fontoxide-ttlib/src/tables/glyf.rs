//! `glyf`: Glyph Data table (`fontTools.ttLib.tables._g_l_y_f`).

use std::collections::{BTreeMap, HashMap, HashSet};

use fontoxide_misc::Tag;
use fontoxide_misc::fixed_tools::{
	fixed_to_float, float_to_fixed, float_to_fixed_to_str, str_to_fixed_to_float,
};
use fontoxide_misc::py::float_repr;
use fontoxide_misc::round_tools::ot_round;
use fontoxide_misc::text_tools::Literal;
use fontoxide_misc::transform::Transform;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::{AttrValue, XmlWriter};

use fontoxide_pens::Pen;

use crate::binary::fit;
use crate::glyph_order::GlyphOrder;
use crate::sstruct::eval;
use crate::table::{Table, TableType};
use crate::tables::tt_program::Program;
use crate::tables::{Loca, Maxp};
use crate::{Error, Font, Result};

/// The point is on the curve.
pub const FLAG_ON_CURVE: u8 = 0x01;
const FLAG_X_SHORT: u8 = 0x02;
const FLAG_Y_SHORT: u8 = 0x04;
const FLAG_REPEAT: u8 = 0x08;
const FLAG_X_SAME: u8 = 0x10;
const FLAG_Y_SAME: u8 = 0x20;
/// The contours may overlap (in the first point's flags).
pub const FLAG_OVERLAP_SIMPLE: u8 = 0x40;
/// The point is a cubic off-curve point (`glyf` version 1, `GLYF`).
pub const FLAG_CUBIC: u8 = 0x80;
/// The flags kept after decompiling, for `glyf` (`keepFlags & ~flagCubic`).
const KEEP_FLAGS: u8 = FLAG_ON_CURVE | FLAG_OVERLAP_SIMPLE;

const ARG_1_AND_2_ARE_WORDS: i64 = 0x0001;
const ARGS_ARE_XY_VALUES: i64 = 0x0002;
const ROUND_XY_TO_GRID: i64 = 0x0004;
const WE_HAVE_A_SCALE: i64 = 0x0008;
const NON_OVERLAPPING: i64 = 0x0010;
const MORE_COMPONENTS: i64 = 0x0020;
const WE_HAVE_AN_X_AND_Y_SCALE: i64 = 0x0040;
const WE_HAVE_A_TWO_BY_TWO: i64 = 0x0080;
const WE_HAVE_INSTRUCTIONS: i64 = 0x0100;
const USE_MY_METRICS: i64 = 0x0200;
const OVERLAP_COMPOUND: i64 = 0x0400;
const SCALED_COMPONENT_OFFSET: i64 = 0x0800;
const UNSCALED_COMPONENT_OFFSET: i64 = 0x1000;
const GID_IS_24_BIT: i64 = 0x2000;

/// The component flags that are kept; the others are computed when writing.
const KEPT_COMPONENT_FLAGS: i64 = ROUND_XY_TO_GRID
	| USE_MY_METRICS
	| SCALED_COMPONENT_OFFSET
	| UNSCALED_COMPONENT_OFFSET
	| NON_OVERLAPPING
	| OVERLAP_COMPOUND;

/// The most points a composite glyph may flatten to (`MAX_COMPOSITE_POINTS`).
const MAX_COMPOSITE_POINTS: usize = 0xFFFF;

/// A number that keeps whether it is a Python `int` or `float`, which shows in
/// TTX and decides how it is written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
	/// An `int`.
	Int(i64),
	/// A `float`.
	Float(f64),
}

impl Number {
	/// The value.
	pub fn value(self) -> f64 {
		match self {
			Number::Int(i) => i as f64,
			Number::Float(f) => f,
		}
	}

	/// `str(value)`.
	pub fn repr(self) -> String {
		match self {
			Number::Int(i) => i.to_string(),
			Number::Float(f) => float_repr(f),
		}
	}

	/// `otRound(value)`.
	fn rounded(self) -> i64 {
		match self {
			Number::Int(i) => i,
			Number::Float(f) => ot_round(f),
		}
	}

	fn from_literal(l: &Literal, what: &str) -> Result<Self> {
		match l {
			Literal::Int(i) => Ok(Number::Int(int64(*i)?)),
			Literal::Bool(b) => Ok(Number::Int(*b as i64)),
			Literal::Float(f) => Ok(Number::Float(*f)),
			_ => Err(Error::value(format!("invalid {what}"))),
		}
	}
}

fn int64(v: i128) -> Result<i64> {
	i64::try_from(v).map_err(|_| Error::Struct(format!("value {v} out of range")))
}

/// A coordinate as `GlyphCoordinates.__getitem__` returns it: an `int` if it is
/// integral.
fn coord_repr(v: f64) -> String {
	if v.fract() == 0.0 && v.is_finite() && v.abs() < 1e18 {
		(v as i64).to_string()
	} else {
		float_repr(v)
	}
}

fn i16_of(v: i64) -> Result<i16> {
	i16::try_from(v)
		.map_err(|_| Error::Struct("'h' format requires -32768 <= number <= 32767".into()))
}

fn u16_of(v: i64) -> Result<u16> {
	u16::try_from(v).map_err(|_| Error::Struct("'H' format requires 0 <= number <= 65535".into()))
}

/// How a component is placed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ComponentPlacement {
	/// By an offset (`x`, `y`).
	Offset {
		/// `x`.
		x: Number,
		/// `y`.
		y: Number,
	},
	/// By matching a point of the glyph so far with one of the component
	/// (`firstPt`, `secondPt`).
	Points {
		/// `firstPt`.
		first: i64,
		/// `secondPt`.
		second: i64,
	},
}

/// A component of a composite glyph (`GlyphComponent`).
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphComponent {
	/// The component glyph (`glyphName`).
	pub glyph: u32,
	/// The flags that are kept (`flags`).
	pub flags: i64,
	/// Where the component goes.
	pub placement: ComponentPlacement,
	/// The 2×2 matrix (`transform`), if any.
	pub transform: Option<[[f64; 2]; 2]>,
}

impl GlyphComponent {
	/// `decompile`: the component, whether more follow and whether the glyph has
	/// instructions, and the bytes read.
	pub(crate) fn decompile(
		data: &[u8],
		num_glyph_names: usize,
	) -> Result<(Self, bool, bool, usize)> {
		Self::decompile_ext(data, num_glyph_names, false)
	}

	/// [`GlyphComponent::decompile`] for `glyf` or the `extended` `GLYF`.
	pub fn decompile_ext(
		data: &[u8],
		num_glyph_names: usize,
		extended: bool,
	) -> Result<(Self, bool, bool, usize)> {
		let short = |n: usize| Error::Struct(format!("unpack requires a buffer of {n} bytes"));
		let take = |pos: usize, n: usize| data.get(pos..pos + n).ok_or_else(|| short(n));
		let b = take(0, 2)?;
		let raw_flags = u16::from_be_bytes([b[0], b[1]]) as i64;
		// glyf (not GLYF) always uses 16-bit glyph IDs.
		let gid_size = if extended && raw_flags & GID_IS_24_BIT != 0 {
			3
		} else {
			2
		};
		let gid = data
			.get(2..(2 + gid_size).min(data.len()))
			.unwrap_or_default()
			.iter()
			.fold(0u32, |acc, &b| acc << 8 | u32::from(b));
		if gid as usize >= num_glyph_names {
			return Err(Error::ttlib("IndexError: list index out of range"));
		}
		let mut pos = (2 + gid_size).min(data.len());
		let placement = if raw_flags & ARG_1_AND_2_ARE_WORDS != 0 {
			let b = take(pos, 4)?;
			pos += 4;
			if raw_flags & ARGS_ARE_XY_VALUES != 0 {
				ComponentPlacement::Offset {
					x: Number::Int(i16::from_be_bytes([b[0], b[1]]) as i64),
					y: Number::Int(i16::from_be_bytes([b[2], b[3]]) as i64),
				}
			} else {
				ComponentPlacement::Points {
					first: u16::from_be_bytes([b[0], b[1]]) as i64,
					second: u16::from_be_bytes([b[2], b[3]]) as i64,
				}
			}
		} else {
			let b = take(pos, 2)?;
			pos += 2;
			if raw_flags & ARGS_ARE_XY_VALUES != 0 {
				ComponentPlacement::Offset {
					x: Number::Int(b[0] as i8 as i64),
					y: Number::Int(b[1] as i8 as i64),
				}
			} else {
				ComponentPlacement::Points {
					first: b[0] as i64,
					second: b[1] as i64,
				}
			}
		};
		let f2 =
			|b: &[u8], i: usize| fixed_to_float(i16::from_be_bytes([b[i], b[i + 1]]) as i64, 14);
		let transform = if raw_flags & WE_HAVE_A_SCALE != 0 {
			let b = take(pos, 2)?;
			pos += 2;
			let s = f2(b, 0);
			Some([[s, 0.0], [0.0, s]])
		} else if raw_flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
			let b = take(pos, 4)?;
			pos += 4;
			Some([[f2(b, 0), 0.0], [0.0, f2(b, 2)]])
		} else if raw_flags & WE_HAVE_A_TWO_BY_TWO != 0 {
			let b = take(pos, 8)?;
			pos += 8;
			Some([[f2(b, 0), f2(b, 2)], [f2(b, 4), f2(b, 6)]])
		} else {
			None
		};
		let component = GlyphComponent {
			glyph: gid,
			flags: raw_flags & KEPT_COMPONENT_FLAGS,
			placement,
			transform,
		};
		Ok((
			component,
			raw_flags & MORE_COMPONENTS != 0,
			raw_flags & WE_HAVE_INSTRUCTIONS != 0,
			pos,
		))
	}

	/// `compile`.
	pub(crate) fn compile(
		&self,
		more: bool,
		have_instructions: bool,
		num_glyphs: usize,
	) -> Result<Vec<u8>> {
		self.compile_ext(more, have_instructions, num_glyphs, false)
	}

	/// [`GlyphComponent::compile`] for `glyf` or the `extended` `GLYF`.
	pub fn compile_ext(
		&self,
		more: bool,
		have_instructions: bool,
		num_glyphs: usize,
		extended: bool,
	) -> Result<Vec<u8>> {
		let mut data = Vec::new();
		let mut flags = self.flags & KEPT_COMPONENT_FLAGS;
		if more {
			flags |= MORE_COMPONENTS;
		}
		if have_instructions {
			flags |= WE_HAVE_INSTRUCTIONS;
		}
		match self.placement {
			ComponentPlacement::Points { first, second } => {
				if (0..=255).contains(&first) && (0..=255).contains(&second) {
					data.extend_from_slice(&[first as u8, second as u8]);
				} else {
					data.extend_from_slice(&u16_of(first)?.to_be_bytes());
					data.extend_from_slice(&u16_of(second)?.to_be_bytes());
					flags |= ARG_1_AND_2_ARE_WORDS;
				}
			}
			ComponentPlacement::Offset { x, y } => {
				let (x, y) = (x.rounded(), y.rounded());
				flags |= ARGS_ARE_XY_VALUES;
				if (-128..=127).contains(&x) && (-128..=127).contains(&y) {
					data.extend_from_slice(&[x as i8 as u8, y as i8 as u8]);
				} else {
					data.extend_from_slice(&i16_of(x)?.to_be_bytes());
					data.extend_from_slice(&i16_of(y)?.to_be_bytes());
					flags |= ARG_1_AND_2_ARE_WORDS;
				}
			}
		}
		if let Some(t) = self.transform {
			let f = |v: f64| i16_of(float_to_fixed(v, 14));
			let t = [[f(t[0][0])?, f(t[0][1])?], [f(t[1][0])?, f(t[1][1])?]];
			if t[0][1] != 0 || t[1][0] != 0 {
				flags |= WE_HAVE_A_TWO_BY_TWO;
				for v in [t[0][0], t[0][1], t[1][0], t[1][1]] {
					data.extend_from_slice(&v.to_be_bytes());
				}
			} else if t[0][0] != t[1][1] {
				flags |= WE_HAVE_AN_X_AND_Y_SCALE;
				data.extend_from_slice(&t[0][0].to_be_bytes());
				data.extend_from_slice(&t[1][1].to_be_bytes());
			} else {
				flags |= WE_HAVE_A_SCALE;
				data.extend_from_slice(&t[0][0].to_be_bytes());
			}
		}
		if self.glyph as usize >= num_glyphs {
			return Err(Error::value(format!("{}", self.glyph)));
		}
		let mut out = Vec::with_capacity(5 + data.len());
		if extended && self.glyph > 0xFFFF {
			if self.glyph > 0xFF_FFFF {
				return Err(Error::Struct("int too big to convert".into()));
			}
			flags |= GID_IS_24_BIT;
			out.extend_from_slice(&u16_of(flags)?.to_be_bytes());
			out.extend_from_slice(&self.glyph.to_be_bytes()[1..]);
		} else {
			flags &= !GID_IS_24_BIT;
			let gid = u16::try_from(self.glyph)
				.map_err(|_| Error::Struct("int too big to convert".into()))?;
			out.extend_from_slice(&u16_of(flags)?.to_be_bytes());
			out.extend_from_slice(&gid.to_be_bytes());
		}
		out.extend(data);
		Ok(out)
	}

	/// `toXML`.
	fn to_xml(&self, writer: &mut XmlWriter, order: &GlyphOrder) {
		let name = order.name(self.glyph).into_owned();
		let mut attrs: Vec<(&str, String)> = vec![("glyphName", name)];
		match self.placement {
			ComponentPlacement::Offset { x, y } => {
				attrs.push(("x", x.repr()));
				attrs.push(("y", y.repr()));
			}
			ComponentPlacement::Points { first, second } => {
				attrs.push(("firstPt", first.to_string()));
				attrs.push(("secondPt", second.to_string()));
			}
		}
		if let Some(t) = self.transform {
			let s = |v: f64| float_to_fixed_to_str(v, 14);
			if t[0][1] != 0.0 || t[1][0] != 0.0 {
				attrs.push(("scalex", s(t[0][0])));
				attrs.push(("scale01", s(t[0][1])));
				attrs.push(("scale10", s(t[1][0])));
				attrs.push(("scaley", s(t[1][1])));
			} else if t[0][0] != t[1][1] {
				attrs.push(("scalex", s(t[0][0])));
				attrs.push(("scaley", s(t[1][1])));
			} else {
				attrs.push(("scale", s(t[0][0])));
			}
		}
		attrs.push(("flags", py_hex(self.flags)));
		let refs: Vec<(&str, &dyn AttrValue)> = attrs
			.iter()
			.map(|(n, v)| (*n, v as &dyn AttrValue))
			.collect();
		writer.simpletag_ordered("component", &refs);
		writer.newline();
	}

	/// `fromXML`.
	fn from_xml(e: &Element, font: &mut Font) -> Result<Self> {
		let attr = |name: &str| {
			e.attr(name)
				.ok_or_else(|| Error::ttx(format!("KeyError: {name:?}")))
		};
		let glyph = font.glyph_id(attr("glyphName")?)?;
		let int = |name: &str| -> Result<i64> {
			let v = attr(name)?;
			eval(v)?
				.as_int()
				.ok_or_else(|| Error::value(format!("invalid {name}: {v:?}")))
				.and_then(int64)
		};
		let placement = if e.attr("firstPt").is_some() {
			ComponentPlacement::Points {
				first: int("firstPt")?,
				second: int("secondPt")?,
			}
		} else {
			ComponentPlacement::Offset {
				x: Number::from_literal(&eval(attr("x")?)?, "x")?,
				y: Number::from_literal(&eval(attr("y")?)?, "y")?,
			}
		};
		let fixed = |name: &str| -> Result<f64> {
			let v = attr(name)?;
			str_to_fixed_to_float(v, 14)
				.ok_or_else(|| Error::value(format!("could not convert string to float: {v:?}")))
		};
		let transform = if e.attr("scale01").is_some() {
			Some([
				[fixed("scalex")?, fixed("scale01")?],
				[fixed("scale10")?, fixed("scaley")?],
			])
		} else if e.attr("scalex").is_some() {
			Some([[fixed("scalex")?, 0.0], [0.0, fixed("scaley")?]])
		} else if e.attr("scale").is_some() {
			let s = fixed("scale")?;
			Some([[s, 0.0], [0.0, s]])
		} else {
			None
		};
		Ok(GlyphComponent {
			glyph,
			flags: int("flags")?,
			placement,
			transform,
		})
	}

	/// `_hasOnlyIntegerTranslate`.
	fn has_only_integer_translate(&self) -> bool {
		match self.placement {
			ComponentPlacement::Offset { x, y } => {
				self.transform.is_none() && x.value().fract() == 0.0 && y.value().fract() == 0.0
			}
			ComponentPlacement::Points { .. } => false,
		}
	}
}

/// `hex(value)`.
fn py_hex(v: i64) -> String {
	if v < 0 {
		format!("-{:#x}", v.unsigned_abs())
	} else {
		format!("{v:#x}")
	}
}

/// A glyph (`Glyph`): simple (contours), composite (components) or empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Glyph {
	/// The number of contours; -1 for a composite glyph (`numberOfContours`).
	pub number_of_contours: i64,
	/// `xMin`.
	pub x_min: i64,
	/// `yMin`.
	pub y_min: i64,
	/// `xMax`.
	pub x_max: i64,
	/// `yMax`.
	pub y_max: i64,
	/// The last point of each contour (`endPtsOfContours`).
	pub end_pts_of_contours: Vec<i64>,
	/// The points (`coordinates`).
	pub coordinates: Vec<(f64, f64)>,
	/// The point flags (`flags`).
	pub flags: Vec<u8>,
	/// The components of a composite glyph (`components`).
	pub components: Vec<GlyphComponent>,
	/// The instructions (`program`), if the glyph has them.
	pub program: Option<Program>,
	/// The compact data while the glyph is not expanded (`data`). The other
	/// fields are always parsed, but as in fontTools, a glyph that was read
	/// and not expanded is written as these bytes unless bounding boxes are
	/// recalculated. Clear it when changing the glyph.
	pub data: Option<Vec<u8>>,
}

impl Glyph {
	/// Whether the glyph is composite (`isComposite`).
	pub fn is_composite(&self) -> bool {
		self.number_of_contours == -1
	}

	/// The component glyph and its transformation (`getComponentInfo`).
	pub fn component_info(c: &GlyphComponent) -> Result<(u32, Transform)> {
		let ComponentPlacement::Offset { x, y } = c.placement else {
			return Err(Error::ttlib(
				"AttributeError: 'GlyphComponent' object has no attribute 'x'",
			));
		};
		let [[xx, xy], [yx, yy]] = c.transform.unwrap_or([[1.0, 0.0], [0.0, 1.0]]);
		Ok((
			c.glyph,
			Transform::new(xx, xy, yx, yy, x.value(), y.value()),
		))
	}

	/// Draws the glyph onto `pen`, moved right by `offset` (`draw`). Components
	/// are passed on to the pen.
	pub fn draw(&self, order: &GlyphOrder, pen: &mut dyn Pen, offset: f64) -> Result<()> {
		if self.is_composite() {
			for c in &self.components {
				let (gid, t) = Self::component_info(c)?;
				pen.add_component(&order.name(gid), t)?;
			}
			return Ok(());
		}
		if self.number_of_contours <= 0 {
			return Ok(());
		}
		let mut coordinates = self.coordinates.clone();
		if offset != 0.0 {
			translate(&mut coordinates, (offset, 0.0));
		}
		let assertion = |what: &str| Error::Assertion(what.to_owned());
		let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
		let mut start = 0;
		for &end in &self.end_pts_of_contours {
			let end = usize::try_from(end + 1)
				.unwrap_or(0)
				.clamp(start, coordinates.len());
			let mut contour = coordinates[start..end].to_vec();
			let mut c_flags: Vec<bool> = self.flags
				[start.min(self.flags.len())..end.min(self.flags.len())]
				.iter()
				.map(|f| f & FLAG_ON_CURVE != 0)
				.collect();
			let mut cu_flags: Vec<bool> = self.flags
				[start.min(self.flags.len())..end.min(self.flags.len())]
				.iter()
				.map(|f| f & FLAG_CUBIC != 0)
				.collect();
			start = end;
			match c_flags.iter().position(|&f| f) {
				None => {
					let all = cu_flags.iter().all(|&f| f);
					if !all && cu_flags.iter().any(|&f| f) {
						return Err(assertion("all(cuFlags) or not any(cuFlags)"));
					}
					if all {
						let count = contour.len();
						if !count.is_multiple_of(2) {
							return Err(assertion("Odd number of cubic off-curves undefined"));
						}
						let (Some(&l), Some(&f)) = (contour.last(), contour.first()) else {
							return Err(Error::ttlib("IndexError: array index out of range"));
						};
						pen.move_to(mid(l, f))?;
						for i in (0..count).step_by(2) {
							let p1 = contour[i];
							let p2 = contour[i + 1];
							let p4 = contour[if i + 2 < count { i + 2 } else { 0 }];
							pen.curve_to(&[p1, p2, mid(p2, p4)])?;
						}
					} else {
						// No on-curve point at all: `qCurveTo` with `None` as the
						// on-curve point.
						pen.q_curve_to(&contour, None)?;
					}
				}
				Some(first) => {
					// Rotate the points so that the contour ends in an on-curve
					// point, which is used for the `moveTo`.
					let first = first + 1;
					contour.rotate_left(first);
					c_flags.rotate_left(first);
					cu_flags.rotate_left(first);
					pen.move_to(contour[contour.len() - 1])?;
					while !contour.is_empty() {
						let next = c_flags
							.iter()
							.position(|&f| f)
							.ok_or_else(|| Error::value("ValueError: 1 is not in list"))?
							+ 1;
						if next == 1 {
							// A final `lineTo` is implied by `closePath`.
							if contour.len() > 1 {
								pen.line_to(contour[0])?;
							}
						} else {
							let cubic_flags = &cu_flags[..next - 1];
							let all = cubic_flags.iter().all(|&f| f);
							let any = cubic_flags.iter().any(|&f| f);
							if !all && any {
								return Err(assertion("all(cubicFlags) or not any(cubicFlags)"));
							}
							if any {
								let count = next;
								if count < 3 {
									return Err(assertion(
										"At least two cubic off-curve points required",
									));
								}
								if (count - 1) % 2 != 0 {
									return Err(assertion(
										"Odd number of cubic off-curves undefined",
									));
								}
								for i in (0..count - 3).step_by(2) {
									let p1 = contour[i];
									let p2 = contour[i + 1];
									let p4 = contour[i + 2];
									pen.curve_to(&[p1, p2, mid(p2, p4)])?;
								}
								pen.curve_to(&contour[count - 3..count])?;
							} else {
								pen.q_curve_to(&contour[..next - 1], Some(contour[next - 1]))?;
							}
						}
						contour.drain(..next);
						c_flags.drain(..next);
						cu_flags.drain(..next);
					}
				}
			}
			pen.close_path()?;
		}
		Ok(())
	}

	/// Parses a glyph (`Glyph(data)` + `expand`). Component glyph IDs must be
	/// below `num_glyph_names`.
	pub fn decompile(data: &[u8], num_glyph_names: usize) -> Result<Self> {
		Self::decompile_ext(data, num_glyph_names, false)
	}

	/// [`Glyph::decompile`] for `glyf` or the `extended` `GLYF`.
	pub fn decompile_ext(data: &[u8], num_glyph_names: usize, extended: bool) -> Result<Self> {
		let mut g = Glyph::default();
		if data.is_empty() {
			return Ok(g);
		}
		let header = data
			.get(..10)
			.ok_or_else(|| Error::Struct("unpack requires a buffer of 10 bytes".into()))?;
		let h = |i: usize| i16::from_be_bytes([header[i], header[i + 1]]) as i64;
		g.number_of_contours = h(0);
		g.x_min = h(2);
		g.y_min = h(4);
		g.x_max = h(6);
		g.y_max = h(8);
		let data = &data[10..];
		if g.number_of_contours == 0 {
			// Zero-contour glyphs may have instructions for the phantom points.
			if data.len() >= 2 {
				let n = u16::from_be_bytes([data[0], data[1]]) as usize;
				g.program = Some(Program::from_bytecode(&data[2..(2 + n).min(data.len())]));
			}
			return Ok(g);
		}
		if g.is_composite() {
			g.decompile_components(data, num_glyph_names, extended)?;
		} else if g.number_of_contours < 0 {
			// LENIENT(L11): fontTools reads such a glyph as a simple glyph with
			// Python slicing on negative sizes, which yields garbage or fails.
			return Err(Error::ttlib(format!(
				"unsupported numberOfContours: {}",
				g.number_of_contours
			)));
		} else {
			g.decompile_coordinates(data, extended)?;
		}
		Ok(g)
	}

	fn decompile_components(
		&mut self,
		data: &[u8],
		num_glyph_names: usize,
		extended: bool,
	) -> Result<()> {
		let mut pos = 0;
		let mut have_instructions = false;
		loop {
			let (c, more, instr, used) =
				GlyphComponent::decompile_ext(&data[pos..], num_glyph_names, extended)?;
			pos += used;
			have_instructions |= instr;
			self.components.push(c);
			if !more {
				break;
			}
		}
		if have_instructions {
			let b = data
				.get(pos..pos + 2)
				.ok_or_else(|| Error::Struct("unpack requires a buffer of 2 bytes".into()))?;
			let n = u16::from_be_bytes([b[0], b[1]]) as usize;
			pos += 2;
			let end = (pos + n).min(data.len());
			self.program = Some(Program::from_bytecode(&data[pos..end]));
			let excess = data.len() - end;
			if excess >= 4 {
				log::warn!(
					"too much glyph data at the end of composite glyph: {excess} excess bytes"
				);
			}
		}
		Ok(())
	}

	fn decompile_coordinates(&mut self, data: &[u8], extended: bool) -> Result<()> {
		let n = self.number_of_contours as usize;
		let end_bytes = &data[..(2 * n).min(data.len())];
		if !end_bytes.len().is_multiple_of(2) {
			return Err(Error::value("bytes length not a multiple of item size"));
		}
		self.end_pts_of_contours = end_bytes
			.as_chunks::<2>()
			.0
			.iter()
			.map(|&c| u16::from_be_bytes(c) as i64)
			.collect();
		let mut pos = 2 * n;
		let b = data
			.get(pos..pos + 2)
			.ok_or_else(|| Error::Struct("unpack requires a buffer of 2 bytes".into()))?;
		let instruction_length = u16::from_be_bytes([b[0], b[1]]) as usize;
		let start = (pos + 2).min(data.len());
		let end = (pos + 2 + instruction_length).min(data.len());
		self.program = Some(Program::from_bytecode(&data[start..end]));
		pos += 2 + instruction_length;
		let last = *self
			.end_pts_of_contours
			.last()
			.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
		let count = last as usize + 1;
		let index = || Error::ttlib("IndexError: index out of range");
		// Flags, with repeats expanded.
		let mut flags = vec![0u8; count];
		let mut j = 0;
		let (mut x_len, mut y_len) = (0usize, 0usize);
		loop {
			let flag = *data.get(pos).ok_or_else(index)?;
			pos += 1;
			let mut repeat = 1;
			if flag & FLAG_REPEAT != 0 {
				repeat = *data.get(pos).ok_or_else(index)? as usize + 1;
				pos += 1;
			}
			for _ in 0..repeat {
				if flag & FLAG_X_SHORT != 0 {
					x_len += 1;
				} else if flag & FLAG_X_SAME == 0 {
					x_len += 2;
				}
				if flag & FLAG_Y_SHORT != 0 {
					y_len += 1;
				} else if flag & FLAG_Y_SAME == 0 {
					y_len += 2;
				}
				*flags
					.get_mut(j)
					.ok_or_else(|| Error::ttlib("IndexError: bytearray index out of range"))? = flag;
				j += 1;
			}
			if j >= count {
				break;
			}
		}
		let excess = data.len() as i64 - pos as i64 - (x_len + y_len) as i64;
		if excess >= 4 {
			log::warn!("too much glyph data: {excess} excess bytes");
		}
		let unpack = |at: usize, n: usize| {
			data.get(at..at + n)
				.ok_or_else(|| Error::Struct(format!("unpack requires a buffer of {n} bytes")))
		};
		let xs = unpack(pos, x_len)?;
		let ys = unpack(pos + x_len, y_len)?;
		let (mut xi, mut yi) = (0, 0);
		let mut coordinates = Vec::with_capacity(count);
		let (mut x, mut y) = (0.0, 0.0);
		for &flag in &flags {
			let dx = if flag & FLAG_X_SHORT != 0 {
				let v = xs[xi] as f64;
				xi += 1;
				if flag & FLAG_X_SAME != 0 { v } else { -v }
			} else if flag & FLAG_X_SAME != 0 {
				0.0
			} else {
				let v = i16::from_be_bytes([xs[xi], xs[xi + 1]]) as f64;
				xi += 2;
				v
			};
			let dy = if flag & FLAG_Y_SHORT != 0 {
				let v = ys[yi] as f64;
				yi += 1;
				if flag & FLAG_Y_SAME != 0 { v } else { -v }
			} else if flag & FLAG_Y_SAME != 0 {
				0.0
			} else {
				let v = i16::from_be_bytes([ys[yi], ys[yi + 1]]) as f64;
				yi += 2;
				v
			};
			x += dx;
			y += dy;
			coordinates.push((x, y));
		}
		self.coordinates = coordinates;
		// Discard all flags except these.
		let kept = if extended {
			KEEP_FLAGS | FLAG_CUBIC
		} else {
			KEEP_FLAGS
		};
		self.flags = flags.into_iter().map(|f| f & kept).collect();
		Ok(())
	}

	/// `getMaxpValues`: points and contours.
	fn maxp_values(&self) -> (usize, usize) {
		(self.coordinates.len(), self.end_pts_of_contours.len())
	}

	fn compile_coordinates(&self, extended: bool) -> Result<Vec<u8>> {
		if self.coordinates.len() != self.flags.len() {
			return Err(Error::Assertion(
				"len(self.coordinates) == len(self.flags)".into(),
			));
		}
		let mut data = Vec::new();
		for &e in &self.end_pts_of_contours {
			let e = u16::try_from(e).map_err(|_| {
				Error::value("OverflowError: unsigned short is greater than maximum")
			})?;
			data.extend_from_slice(&e.to_be_bytes());
		}
		// UPSTREAM-BUG(B14): a simple glyph read from TTX without an
		// `<instructions>` element has no program, and writing it fails.
		let program = self.program.as_ref().ok_or_else(|| {
			Error::ttlib("AttributeError: 'Glyph' object has no attribute 'program'")
		})?;
		data.extend_from_slice(&u16_of(program.bytecode.len() as i64)?.to_be_bytes());
		data.extend_from_slice(&program.bytecode);
		if !extended && self.flags.iter().any(|f| f & FLAG_CUBIC != 0) {
			return Err(Error::ttlib("Cubic flags require the extended GLYF table"));
		}
		// Deltas of the rounded coordinates, packed greedily.
		let mut last = (0i64, 0i64);
		let mut out_flags: Vec<u8> = Vec::new();
		let mut xs: Vec<u8> = Vec::new();
		let mut ys: Vec<u8> = Vec::new();
		let mut last_flag: Option<u8> = None;
		let mut repeat = 0;
		for (&flag, &(x, y)) in self.flags.iter().zip(&self.coordinates) {
			let (x, y) = (ot_round(x), ot_round(y));
			let (dx, dy) = (x - last.0, y - last.1);
			last = (x, y);
			let mut flag = flag;
			for (d, short, same, out) in [
				(dx, FLAG_X_SHORT, FLAG_X_SAME, &mut xs),
				(dy, FLAG_Y_SHORT, FLAG_Y_SAME, &mut ys),
			] {
				if d == 0 {
					flag |= same;
				} else if (-255..=255).contains(&d) {
					flag |= short;
					if d > 0 {
						flag |= same;
					}
					out.push(d.unsigned_abs() as u8);
				} else {
					out.extend_from_slice(&i16_of(d)?.to_be_bytes());
				}
			}
			if Some(flag) == last_flag && repeat != 255 {
				repeat += 1;
				if repeat == 1 {
					out_flags.push(flag);
				} else {
					let n = out_flags.len();
					out_flags[n - 2] = flag | FLAG_REPEAT;
					out_flags[n - 1] = repeat as u8;
				}
			} else {
				repeat = 0;
				out_flags.push(flag);
			}
			last_flag = Some(flag);
		}
		data.extend(out_flags);
		data.extend(xs);
		data.extend(ys);
		Ok(data)
	}

	fn compile_components(&self, num_glyphs: usize, extended: bool) -> Result<Vec<u8>> {
		let mut data = Vec::new();
		let mut have_instructions = false;
		let last = self.components.len().wrapping_sub(1);
		for (i, c) in self.components.iter().enumerate() {
			let more = i != last;
			if !more {
				have_instructions = self.program.is_some();
			}
			data.extend(c.compile_ext(more, have_instructions, num_glyphs, extended)?);
		}
		if have_instructions && let Some(p) = &self.program {
			data.extend_from_slice(&u16_of(p.bytecode.len() as i64)?.to_be_bytes());
			data.extend_from_slice(&p.bytecode);
		}
		Ok(data)
	}

	/// `toXML`.
	fn to_xml(&self, writer: &mut XmlWriter, order: &GlyphOrder, extended: bool) -> Result<()> {
		if !extended && self.flags.iter().any(|f| f & FLAG_CUBIC != 0) {
			return Err(Error::ttlib("Cubic flags require the extended GLYF table"));
		}
		if self.is_composite() {
			for c in &self.components {
				c.to_xml(writer, order);
			}
		} else {
			let index = || Error::ttlib("IndexError: list index out of range");
			let mut last = 0i64;
			for i in 0..self.number_of_contours.max(0) as usize {
				writer.begintag("contour", &[]);
				writer.newline();
				let end = *self.end_pts_of_contours.get(i).ok_or_else(index)?;
				for j in last..=end {
					let j = usize::try_from(j).map_err(|_| index())?;
					let (x, y) = *self.coordinates.get(j).ok_or_else(index)?;
					let flag = *self.flags.get(j).ok_or_else(index)?;
					let (x, y, on) = (
						coord_repr(x),
						coord_repr(y),
						(flag & FLAG_ON_CURVE).to_string(),
					);
					let mut attrs: Vec<(&str, &dyn AttrValue)> =
						vec![("x", &x), ("y", &y), ("on", &on)];
					if flag & FLAG_OVERLAP_SIMPLE != 0 {
						attrs.push(("overlap", &1));
					}
					if extended && flag & FLAG_CUBIC != 0 {
						attrs.push(("cubic", &1));
					}
					writer.simpletag_ordered("pt", &attrs);
					writer.newline();
				}
				last = end + 1;
				writer.endtag("contour");
				writer.newline();
			}
		}
		if let Some(p) = &self.program {
			if !p.is_empty() {
				writer.begintag("instructions", &[]);
				writer.newline();
				p.to_xml(writer);
				writer.endtag("instructions");
			} else {
				writer.simpletag("instructions", &[]);
			}
			writer.newline();
		}
		Ok(())
	}

	/// `fromXML` for one child element of `TTGlyph`.
	fn read_xml(&mut self, e: &Element, font: &mut Font, extended: bool) -> Result<()> {
		match e.name.as_str() {
			"contour" => {
				if self.number_of_contours < 0 {
					return Err(Error::ttlib("can't mix composites and contours in glyph"));
				}
				self.number_of_contours += 1;
				let mut coordinates = Vec::new();
				let mut flags = Vec::new();
				for pt in e.elements().filter(|p| p.name == "pt") {
					let attr = |name: &str| {
						pt.attr(name)
							.ok_or_else(|| Error::ttx(format!("KeyError: {name:?}")))
					};
					let num = |name: &str| -> Result<f64> {
						let v = attr(name)?;
						eval(v)?
							.as_float()
							.ok_or_else(|| Error::value(format!("invalid {name}: {v:?}")))
					};
					coordinates.push((num("x")?, num("y")?));
					let mut flag = u8::from(eval(attr("on")?)?.is_truthy());
					if let Some(v) = pt.attr("overlap")
						&& eval(v)?.is_truthy()
					{
						flag |= FLAG_OVERLAP_SIMPLE;
					}
					if let Some(v) = pt.attr("cubic")
						&& eval(v)?.is_truthy()
					{
						if !extended {
							return Err(Error::ttlib(
								"Cubic flags require the extended GLYF table",
							));
						}
						flag |= FLAG_CUBIC;
					}
					flags.push(flag);
				}
				let first = self.end_pts_of_contours.is_empty() && self.coordinates.is_empty();
				if first {
					self.coordinates = coordinates;
					self.flags = flags;
					self.end_pts_of_contours = vec![self.coordinates.len() as i64 - 1];
				} else {
					self.coordinates.extend(coordinates);
					self.flags.extend(flags);
					self.end_pts_of_contours
						.push(self.coordinates.len() as i64 - 1);
				}
			}
			"component" => {
				if self.number_of_contours > 0 {
					return Err(Error::ttlib("can't mix composites and contours in glyph"));
				}
				self.number_of_contours = -1;
				self.components.push(GlyphComponent::from_xml(e, font)?);
			}
			"instructions" => {
				let mut program = Program::default();
				for child in e.elements() {
					program = Program::from_xml(child)?;
				}
				self.program = Some(program);
			}
			_ => {}
		}
		Ok(())
	}
}

/// The flattened outline of a glyph: points, contour ends and flags.
type Flat = (Vec<(f64, f64)>, Vec<i64>, Vec<u8>);

/// Glyph Data table (`table__g_l_y_f`), with the glyphs by glyph ID.
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/glyf>.
#[derive(Debug, Clone, PartialEq)]
pub struct Glyf {
	/// The glyphs (`glyphs`, which fontTools keys by glyph name). Glyphs past the
	/// glyph order (`ttxautoglyph…`) are kept but not written.
	pub glyphs: BTreeMap<u32, Glyph>,
	/// Glyphs are padded to multiples of this (`padding`): 0, 1, 2 or 4; 1 pads
	/// only when that allows short `loca` offsets.
	pub padding: u8,
	/// Whether this is `GLYF`, the beyond-64k companion of the table (`extended`).
	pub extended: bool,
}

impl Default for Glyf {
	fn default() -> Self {
		Glyf {
			glyphs: BTreeMap::new(),
			padding: 1,
			extended: false,
		}
	}
}

fn recursion_error(order: &GlyphOrder, gid: u32) -> Error {
	// LENIENT(L12): fontTools hits Python's recursion limit, and names the glyph
	// at that depth.
	Error::ttlib(format!(
		"glyph '{}' contains a recursive component reference",
		order.name(gid)
	))
}

fn key_error(order: &GlyphOrder, gid: u32) -> Error {
	Error::ttlib(format!("KeyError: {:?}", order.name(gid)))
}

impl Glyf {
	fn glyph(&self, gid: u32, order: &GlyphOrder) -> Result<&Glyph> {
		self.glyphs.get(&gid).ok_or_else(|| key_error(order, gid))
	}

	/// `getCoordinates`: the outline, with composite glyphs flattened.
	pub fn coordinates(&self, gid: u32, order: &GlyphOrder, round: bool) -> Result<Flat> {
		let mut cache = HashMap::new();
		let mut visiting = Vec::new();
		self.flatten(gid, order, round, &mut cache, &mut visiting)
	}

	fn flatten(
		&self,
		gid: u32,
		order: &GlyphOrder,
		round: bool,
		cache: &mut HashMap<u32, Flat>,
		visiting: &mut Vec<u32>,
	) -> Result<Flat> {
		let g = self.glyph(gid, order)?;
		if g.number_of_contours > 0 {
			return Ok((
				g.coordinates.clone(),
				g.end_pts_of_contours.clone(),
				g.flags.clone(),
			));
		}
		if !g.is_composite() {
			return Ok((Vec::new(), Vec::new(), Vec::new()));
		}
		let (mut all, mut ends, mut flags): Flat = (Vec::new(), Vec::new(), Vec::new());
		for c in &g.components {
			let cg = self.glyph(c.glyph, order)?;
			let flat = match cache.get(&c.glyph) {
				Some(f) => f.clone(),
				None => {
					if visiting.contains(&c.glyph) {
						return Err(recursion_error(order, c.glyph));
					}
					visiting.push(c.glyph);
					let f = self.flatten(c.glyph, order, round, cache, visiting)?;
					visiting.pop();
					cache.insert(c.glyph, f.clone());
					f
				}
			};
			let (mut coords, end_pts, f) = flat;
			// Simple glyphs are rounded before being transformed; nested composites
			// only at the end, so that rounding errors do not compound.
			if round && cg.number_of_contours > 0 {
				for p in &mut coords {
					*p = (ot_round(p.0) as f64, ot_round(p.1) as f64);
				}
			}
			match c.placement {
				ComponentPlacement::Points { first, second } => {
					if let Some(t) = c.transform {
						transform(&mut coords, t);
					}
					let (x1, y1) = py_index(&all, first)?;
					let (x2, y2) = py_index(&coords, second)?;
					translate(&mut coords, (x1 - x2, y1 - y2));
				}
				ComponentPlacement::Offset { x, y } => {
					let mv = (x.value(), y.value());
					match c.transform {
						None => translate(&mut coords, mv),
						Some(t) => {
							let apple = c.flags & SCALED_COMPONENT_OFFSET != 0;
							let ms = c.flags & UNSCALED_COMPONENT_OFFSET != 0;
							if apple && ms {
								return Err(Error::Assertion("not (apple_way and ms_way)".into()));
							}
							// MS (scale, then move) unless the Apple flag is set.
							if apple {
								translate(&mut coords, mv);
								transform(&mut coords, t);
							} else {
								transform(&mut coords, t);
								translate(&mut coords, mv);
							}
						}
					}
				}
			}
			let offset = all.len() as i64;
			ends.extend(end_pts.iter().map(|e| e + offset));
			all.extend(coords);
			flags.extend(f);
			if all.len() > MAX_COMPOSITE_POINTS {
				return Err(Error::ttlib(format!(
					"composite glyph with component '{}' has more than {MAX_COMPOSITE_POINTS} points",
					order.name(c.glyph)
				)));
			}
		}
		Ok((all, ends, flags))
	}

	/// `_getCoordinatesAndControls`: the coordinates of a glyph for `gvar`, rounded
	/// and with the four phantom points, and its contour ends (the indices of the
	/// components, for a composite glyph). `h_metrics` and `v_metrics` are the
	/// metrics of `hmtx` and `vmtx`.
	#[allow(clippy::type_complexity)]
	pub fn coordinates_and_controls(
		&mut self,
		gid: u32,
		order: &GlyphOrder,
		h_metrics: &BTreeMap<u32, (u16, i16)>,
		v_metrics: Option<&BTreeMap<u32, (u16, i16)>>,
	) -> Result<(Vec<(f64, f64)>, Vec<i64>)> {
		// The bounds are as they are, also those read from TTX: `_getPhantomPoints`
		// only calculates them for a glyph without any, which are all 0 here.
		let g = self.glyph(gid, order)?;
		let (mut coords, end_pts) = if g.is_composite() {
			let coords = g
				.components
				.iter()
				.map(|c| match c.placement {
					ComponentPlacement::Offset { x, y } => (x.value(), y.value()),
					ComponentPlacement::Points { .. } => (0.0, 0.0),
				})
				.collect();
			(coords, (0..g.components.len() as i64).collect())
		} else {
			let (coords, end_pts, _) = self.coordinates(gid, order, false)?;
			(coords, end_pts)
		};
		// `_getPhantomPoints`.
		let g = self.glyph(gid, order)?;
		let missing = || key_error(order, gid);
		let &(advance, lsb) = h_metrics.get(&gid).ok_or_else(missing)?;
		let left = (g.x_min - i64::from(lsb)) as f64;
		let right = left + f64::from(advance);
		let (top, bottom) = match v_metrics.filter(|m| !m.is_empty()) {
			Some(metrics) => {
				let &(advance, tsb) = metrics.get(&gid).ok_or_else(missing)?;
				let top = (i64::from(tsb) + g.y_max) as f64;
				(top, top - f64::from(advance))
			}
			None => (0.0, 0.0),
		};
		coords.extend([(left, 0.0), (right, 0.0), (0.0, top), (0.0, bottom)]);
		for c in &mut coords {
			*c = (ot_round(c.0) as f64, ot_round(c.1) as f64);
		}
		Ok((coords, end_pts))
	}

	/// `_setCoordinates`: sets the coordinates of a glyph, which end with the four
	/// phantom points, and from those and its bounds the metrics.
	pub fn set_coordinates(
		&mut self,
		gid: u32,
		order: &GlyphOrder,
		coords: &[(f64, f64)],
		h_metrics: &mut BTreeMap<u32, (u16, i16)>,
		v_metrics: Option<&mut BTreeMap<u32, (u16, i16)>>,
	) -> Result<()> {
		let Some(split) = coords.len().checked_sub(4) else {
			return Err(Error::Assertion("len(coord) >= 4".into()));
		};
		let (points, phantoms) = coords.split_at(split);
		let (left, right, top, bottom) =
			(phantoms[0].0, phantoms[1].0, phantoms[2].1, phantoms[3].1);
		// Whole numbers are integers, as `GlyphCoordinates.__getitem__` gives them.
		let number = |v: f64| {
			if v.fract() == 0.0 {
				Number::Int(v as i64)
			} else {
				Number::Float(v)
			}
		};
		let g = self
			.glyphs
			.get_mut(&gid)
			.ok_or_else(|| key_error(order, gid))?;
		g.data = None;
		if g.is_composite() {
			if points.len() != g.components.len() {
				return Err(Error::Assertion(
					"len(coord) == len(glyph.components)".into(),
				));
			}
			for (p, c) in points.iter().zip(&mut g.components) {
				if let ComponentPlacement::Offset { x, y } = &mut c.placement {
					*x = number(p.0);
					*y = number(p.1);
				}
			}
		} else if g.number_of_contours == 0 {
			if !points.is_empty() {
				return Err(Error::Assertion("len(coord) == 0".into()));
			}
		} else {
			if points.len() != g.coordinates.len() {
				return Err(Error::Assertion(
					"len(coord) == len(glyph.coordinates)".into(),
				));
			}
			g.coordinates = points.to_vec();
		}
		self.recalc_bounds(gid, order, &mut HashSet::new(), &mut vec![gid])?;
		let g = self.glyph(gid, order)?;
		let advance = ot_round(right - left).max(0);
		let lsb = ot_round(g.x_min as f64 - left);
		h_metrics.insert(gid, (fit(advance, "advanceWidth")?, fit(lsb, "lsb")?));
		if let Some(metrics) = v_metrics {
			let advance = ot_round(top - bottom).max(0);
			let tsb = ot_round(top - g.y_max as f64);
			metrics.insert(gid, (fit(advance, "advanceHeight")?, fit(tsb, "tsb")?));
		}
		Ok(())
	}

	/// `recalcBounds`.
	fn recalc_bounds(
		&mut self,
		gid: u32,
		order: &GlyphOrder,
		done: &mut HashSet<u32>,
		visiting: &mut Vec<u32>,
	) -> Result<()> {
		let g = self.glyph(gid, order)?;
		if g.is_composite()
			&& g.components
				.iter()
				.all(GlyphComponent::has_only_integer_translate)
		{
			let components = g.components.clone();
			let mut bounds: Option<[f64; 4]> = None;
			let mut float = false;
			for c in &components {
				if !done.contains(&c.glyph) {
					self.glyph(c.glyph, order)?;
					if visiting.contains(&c.glyph) {
						return Err(recursion_error(order, c.glyph));
					}
					visiting.push(c.glyph);
					self.recalc_bounds(c.glyph, order, done, visiting)?;
					visiting.pop();
					done.insert(c.glyph);
				}
				let cg = self.glyph(c.glyph, order)?;
				// Empty components do not count.
				if cg.y_min == cg.y_max && cg.x_min == cg.x_max {
					continue;
				}
				let ComponentPlacement::Offset { x, y } = c.placement else {
					continue;
				};
				float |= matches!(x, Number::Float(_)) || matches!(y, Number::Float(_));
				let (x, y) = (x.value(), y.value());
				for (px, py) in [
					(cg.x_min as f64 + x, cg.y_min as f64 + y),
					(cg.x_max as f64 + x, cg.y_max as f64 + y),
				] {
					bounds = Some(match bounds {
						None => [px, py, px, py],
						Some(b) => [b[0].min(px), b[1].min(py), b[2].max(px), b[3].max(py)],
					});
				}
			}
			if float && bounds.is_some() {
				// LENIENT(L13): fontTools stores float bounds here, and fails later
				// when packing the glyph header.
				return Err(Error::Struct("required argument is not an integer".into()));
			}
			let b = bounds.unwrap_or([0.0; 4]);
			let g = self
				.glyphs
				.get_mut(&gid)
				.ok_or_else(|| key_error(order, gid))?;
			g.x_min = b[0] as i64;
			g.y_min = b[1] as i64;
			g.x_max = b[2] as i64;
			g.y_max = b[3] as i64;
			return Ok(());
		}
		let (coords, _, _) = {
			let mut cache = HashMap::new();
			let mut v = visiting.clone();
			v.push(gid);
			self.flatten(gid, order, true, &mut cache, &mut v)?
		};
		let b = calc_bounds(&coords);
		let g = self
			.glyphs
			.get_mut(&gid)
			.ok_or_else(|| key_error(order, gid))?;
		g.x_min = ot_round(b[0]);
		g.y_min = ot_round(b[1]);
		g.x_max = ot_round(b[2]);
		g.y_max = ot_round(b[3]);
		Ok(())
	}

	/// `Glyph.compile`.
	fn compile_glyph(
		&mut self,
		gid: u32,
		order: &GlyphOrder,
		recalc: bool,
		done: &mut HashSet<u32>,
	) -> Result<Vec<u8>> {
		let g = self.glyph(gid, order)?;
		if let Some(data) = &g.data {
			if !recalc {
				return Ok(data.clone());
			}
			// The glyph must be expanded to recalculate its bounding box.
			if let Some(g) = self.glyphs.get_mut(&gid) {
				g.data = None;
			}
		}
		let g = self.glyph(gid, order)?;
		if g.number_of_contours == 0 && g.program.as_ref().is_none_or(Program::is_empty) {
			return Ok(Vec::new());
		}
		if recalc {
			self.recalc_bounds(gid, order, done, &mut vec![gid])?;
		}
		let g = self.glyph(gid, order)?;
		let mut data = Vec::new();
		for v in [g.number_of_contours, g.x_min, g.y_min, g.x_max, g.y_max] {
			data.extend_from_slice(&i16_of(v)?.to_be_bytes());
		}
		if g.is_composite() {
			data.extend(g.compile_components(order.len(), self.extended)?);
		} else if g.number_of_contours == 0 {
			let p = g
				.program
				.as_ref()
				.map(|p| p.bytecode.as_slice())
				.unwrap_or_default();
			data.extend_from_slice(&u16_of(p.len() as i64)?.to_be_bytes());
			data.extend_from_slice(p);
		} else {
			data.extend(g.compile_coordinates(self.extended)?);
		}
		Ok(data)
	}

	/// `getCompositeMaxpValues`: points, contours and component depth of a
	/// flattened composite glyph.
	pub fn composite_maxp_values(
		&self,
		gid: u32,
		order: &GlyphOrder,
		depth: i64,
		cache: &mut HashMap<u32, (usize, usize, i64)>,
		visiting: &mut Vec<u32>,
	) -> Result<(usize, usize, i64)> {
		let g = self.glyph(gid, order)?;
		let (mut points, mut contours) = (0, 0);
		let mut max_depth = depth;
		for c in &g.components {
			let base = self.glyph(c.glyph, order)?;
			let (p, n) = if base.number_of_contours == 0 {
				continue;
			} else if base.number_of_contours > 0 {
				base.maxp_values()
			} else {
				let offset = depth + 1;
				let (p, n, d) = match cache.get(&c.glyph) {
					Some(&(p, n, d)) => (p, n, d + offset),
					None => {
						if visiting.contains(&c.glyph) {
							return Err(Error::ttlib(
								"RecursionError: maximum recursion depth exceeded",
							));
						}
						visiting.push(c.glyph);
						let (p, n, d) =
							self.composite_maxp_values(c.glyph, order, offset, cache, visiting)?;
						visiting.pop();
						cache.insert(c.glyph, (p, n, d - offset));
						(p, n, d)
					}
				};
				max_depth = max_depth.max(d);
				(p, n)
			};
			points += p;
			contours += n;
		}
		Ok((points, contours, max_depth))
	}
}

fn translate(coords: &mut [(f64, f64)], (x, y): (f64, f64)) {
	if x == 0.0 && y == 0.0 {
		return;
	}
	for p in coords {
		p.0 += x;
		p.1 += y;
	}
}

fn transform(coords: &mut [(f64, f64)], t: [[f64; 2]; 2]) {
	for p in coords {
		let (x, y) = *p;
		*p = (x * t[0][0] + y * t[1][0], x * t[0][1] + y * t[1][1]);
	}
}

/// Python indexing: negative indices count from the end.
fn py_index(coords: &[(f64, f64)], i: i64) -> Result<(f64, f64)> {
	let n = coords.len() as i64;
	let at = if i < 0 { i + n } else { i };
	usize::try_from(at)
		.ok()
		.and_then(|at| coords.get(at).copied())
		.ok_or_else(|| Error::ttlib("IndexError: array index out of range"))
}

/// `calcBounds`: (0, 0, 0, 0) for no points.
fn calc_bounds(coords: &[(f64, f64)]) -> [f64; 4] {
	let Some(&(x0, y0)) = coords.first() else {
		return [0.0; 4];
	};
	coords.iter().fold([x0, y0, x0, y0], |b, &(x, y)| {
		[b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)]
	})
}

/// `remapComponentsFast`: maps the component glyphs of compact glyph data.
fn remap_components_fast(
	data: &mut [u8],
	extended: bool,
	f: &mut dyn FnMut(u32) -> Result<u32>,
) -> Result<()> {
	if data.len() < 2 || i16::from_be_bytes([data[0], data[1]]) >= 0 {
		// Not composite.
		return Ok(());
	}
	let byte = |data: &[u8], i: usize| -> Result<i64> {
		data.get(i)
			.map(|&b| i64::from(b))
			.ok_or_else(|| Error::ttlib("IndexError: bytearray index out of range"))
	};
	let mut i = 10;
	loop {
		let flags = (byte(data, i)? << 8) | byte(data, i + 1)?;
		let start = i + 2;
		// The size of the glyph ID stays, whatever the new glyph ID is.
		let size = if extended && flags & GID_IS_24_BIT != 0 {
			3
		} else {
			2
		};
		let mut gid = 0;
		for k in 0..size {
			gid = (gid << 8) | byte(data, start + k)?;
		}
		let new = f(gid as u32)?;
		if new >> (8 * size) != 0 {
			return Err(Error::value("OverflowError: int too big to convert"));
		}
		data[start..start + size].copy_from_slice(&new.to_be_bytes()[4 - size..]);
		i = start + size;
		i += if flags & ARG_1_AND_2_ARE_WORDS != 0 {
			4
		} else {
			2
		};
		if flags & WE_HAVE_A_SCALE != 0 {
			i += 2;
		} else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
			i += 4;
		} else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
			i += 8;
		}
		if flags & MORE_COMPONENTS == 0 {
			return Ok(());
		}
	}
}

impl Glyph {
	/// `trim`: removes padding and, if asked, hinting, without expanding the
	/// glyph.
	pub fn trim(&mut self, remove_hinting: bool) -> Result<()> {
		self.trim_ext(remove_hinting, false)
	}

	/// [`Glyph::trim`] for `glyf` or the `extended` `GLYF`.
	pub fn trim_ext(&mut self, remove_hinting: bool, extended: bool) -> Result<()> {
		let Some(data) = self.data.as_mut() else {
			if remove_hinting {
				if self.is_composite() {
					self.program = None;
				} else {
					self.program = Some(Program::from_bytecode(&[]));
				}
			}
			// No padding to trim.
			return Ok(());
		};
		if data.is_empty() {
			return Ok(());
		}
		let err = || Error::ttlib("IndexError: bytearray index out of range");
		let at = |data: &[u8], i: usize| data.get(i).map(|&b| usize::from(b)).ok_or_else(err);
		let num_contours = i16::from_be_bytes([data[0], *data.get(1).ok_or_else(err)?]);
		let mut i = 10;
		if num_contours == 0 {
			// No contour end points or flags in this case.
			if !remove_hinting && data.len() >= i + 2 {
				let instruction_len = (at(data, i)? << 8) | at(data, i + 1)?;
				i += 2 + instruction_len;
			}
			data.truncate(i);
			return Ok(());
		}
		if num_contours >= 0 {
			i += 2 * num_contours as usize;
			let n_coordinates = ((at(data, i - 2)? << 8) | at(data, i - 1)?) + 1;
			let instruction_len = (at(data, i)? << 8) | at(data, i + 1)?;
			if remove_hinting {
				// Zero the instruction length and splice the instructions out.
				data[i] = 0;
				data[i + 1] = 0;
				i += 2;
				if instruction_len > 0 {
					let end = (i + instruction_len).min(data.len());
					data.drain(i..end);
				}
			} else {
				i += 2 + instruction_len;
			}
			let mut coord_bytes = 0;
			let mut j = 0;
			loop {
				let flag = at(data, i)? as u8;
				i += 1;
				let mut repeat = 1;
				if flag & FLAG_REPEAT != 0 {
					repeat = at(data, i)? + 1;
					i += 1;
				}
				let x_bytes = if flag & FLAG_X_SHORT != 0 {
					1
				} else if flag & FLAG_X_SAME == 0 {
					2
				} else {
					0
				};
				let y_bytes = if flag & FLAG_Y_SHORT != 0 {
					1
				} else if flag & FLAG_Y_SAME == 0 {
					2
				} else {
					0
				};
				coord_bytes += (x_bytes + y_bytes) * repeat;
				j += repeat;
				if j >= n_coordinates {
					break;
				}
			}
			if j != n_coordinates {
				return Err(Error::Assertion("bad glyph flags".into()));
			}
			i += coord_bytes;
			// Remove padding.
			data.truncate(i);
		} else if num_contours == -1 {
			let mut we_have_instructions = false;
			loop {
				let mut flags = ((at(data, i)? << 8) | at(data, i + 1)?) as i64;
				if remove_hinting {
					flags &= !WE_HAVE_INSTRUCTIONS;
				}
				if flags & WE_HAVE_INSTRUCTIONS != 0 {
					we_have_instructions = true;
				}
				data[i] = (flags >> 8) as u8;
				data[i + 1] = flags as u8;
				i += if extended && flags & GID_IS_24_BIT != 0 {
					5
				} else {
					4
				};
				i += if flags & ARG_1_AND_2_ARE_WORDS != 0 {
					4
				} else {
					2
				};
				if flags & WE_HAVE_A_SCALE != 0 {
					i += 2;
				} else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
					i += 4;
				} else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
					i += 8;
				}
				if flags & MORE_COMPONENTS == 0 {
					break;
				}
			}
			if we_have_instructions {
				let instruction_len = (at(data, i)? << 8) | at(data, i + 1)?;
				i += 2 + instruction_len;
			}
			// Remove padding.
			data.truncate(i);
		}
		Ok(())
	}
}

impl TableType for Glyf {
	const TAG: Tag = Tag::new(b"glyf");
	const COMPANION: Option<Tag> = Some(Tag::new(b"GLYF"));

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		Self::decompile_as(Self::TAG, data, font)
	}

	fn new_as(tag: Tag) -> Self {
		Glyf {
			extended: tag == Tag::new(b"GLYF"),
			..Default::default()
		}
	}

	fn decompile_as(tag: Tag, data: &[u8], font: &mut Font) -> Result<Self> {
		let extended = tag == Tag::new(b"GLYF");
		let locations = font.require_locked::<Loca>(extended)?.locations().to_vec();
		let order = font.glyph_order()?;
		let mut pos = *locations
			.first()
			.ok_or_else(|| Error::ttlib("IndexError: array index out of range"))?
			as usize;
		let mut next = 0;
		let mut noname = 0;
		let mut glyphs = BTreeMap::new();
		for i in 0..locations.len() - 1 {
			if i >= order.len() {
				noname += 1;
			}
			next = locations[i + 1] as usize;
			let glyph_data = data.get(pos..next).filter(|d| d.len() == next - pos);
			let glyph_data = match glyph_data {
				Some(d) => d,
				None if next == pos => &[][..],
				None => return Err(Error::ttlib("not enough 'glyf' table data")),
			};
			let mut glyph = Glyph::decompile_ext(glyph_data, order.len(), extended)?;
			glyph.data = Some(glyph_data.to_vec());
			glyphs.insert(i as u32, glyph);
			pos = next;
		}
		if data.len() as i64 - next as i64 >= 4 {
			log::warn!(
				"too much 'glyf' table data: expected {next}, received {} bytes",
				data.len()
			);
		}
		if noname > 0 {
			log::warn!("{noname} glyphs have no name");
		}
		Ok(Glyf {
			glyphs,
			padding: 1,
			extended,
		})
	}
}

impl Table for Glyf {
	/// Expands every glyph (`ensureDecompiled`).
	fn ensure_decompiled(&mut self) {
		for g in self.glyphs.values_mut() {
			g.data = None;
		}
	}

	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let mut glyphs = BTreeMap::new();
		for (g, mut glyph) in std::mem::take(&mut self.glyphs) {
			for c in &mut glyph.components {
				c.glyph = f(c.glyph)?;
			}
			if let Some(data) = &mut glyph.data {
				remap_components_fast(data, self.extended, f)?;
			}
			glyphs.insert(f(g)?, glyph);
		}
		self.glyphs = glyphs;
		Ok(())
	}

	fn tag(&self) -> Tag {
		Font::locked_tag::<Self>(self.extended)
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		let order = font.glyph_order()?;
		let padding = self.padding as usize;
		if ![0, 1, 2, 4].contains(&padding) {
			return Err(Error::Assertion("padding in (0, 1, 2, 4)".into()));
		}
		let recalc = font.recalc_bboxes;
		let mut done = HashSet::new();
		let mut data_list = Vec::with_capacity(order.len());
		for gid in 0..order.len() as u32 {
			let mut d = self.compile_glyph(gid, &order, recalc, &mut done)?;
			if padding > 1 {
				d.resize(d.len().div_ceil(padding) * padding, 0);
			}
			data_list.push(d);
		}
		let total: usize = data_list.iter().map(Vec::len).sum();
		if padding == 1 && total < 0x20000 {
			// Pad odd-length glyphs if that allows short `loca` offsets.
			let odd = data_list.iter().filter(|d| d.len() % 2 == 1).count();
			if odd > 0 && total + odd < 0x20000 {
				for d in data_list.iter_mut().filter(|d| d.len() % 2 == 1) {
					d.push(0);
				}
			}
		}
		let mut locations = Vec::with_capacity(data_list.len() + 1);
		let mut at = 0u32;
		for d in &data_list {
			locations.push(at);
			at += d.len() as u32;
		}
		locations.push(at);
		let mut data = data_list.concat();
		let extended = self.extended;
		if let Some(loca) = font.get_locked::<Loca>(extended)? {
			loca.set(locations);
		}
		if let Some(maxp) = font.get_locked::<Maxp>(extended)?
			&& let Ok(n) = u32::try_from(self.glyphs.len())
		{
			maxp.num_glyphs = n;
		}
		if data.is_empty() {
			// An empty table would be rejected by OTS and Windows.
			data.push(0);
		}
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let order = font.glyph_order()?;
		writer.newline();
		writer
			.comment("The xMin, yMin, xMax and yMax values\nwill be recalculated by the compiler.");
		writer.newline();
		writer.newline();
		let mut names: Vec<&String> = order.names().iter().collect();
		names.sort();
		for name in names {
			let Some(gid) = order.id(name) else {
				continue;
			};
			let Some(glyph) = self.glyphs.get(&gid) else {
				log::warn!("glyph '{name}' does not exist in glyf table");
				continue;
			};
			if glyph.number_of_contours != 0
				|| glyph.program.as_ref().is_some_and(|p| !p.is_empty())
			{
				let (x_min, y_min, x_max, y_max) =
					(glyph.x_min, glyph.y_min, glyph.x_max, glyph.y_max);
				writer.begintag_ordered(
					"TTGlyph",
					&[
						("name", name),
						("xMin", &x_min),
						("yMin", &y_min),
						("xMax", &x_max),
						("yMax", &y_max),
					],
				);
				writer.newline();
				glyph.to_xml(writer, &order, self.extended)?;
				writer.endtag("TTGlyph");
				writer.newline();
			} else {
				writer.simpletag("TTGlyph", &[("name", name)]);
				writer.comment("contains no outline data");
				writer.newline();
			}
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		if element.name != "TTGlyph" {
			return Ok(());
		}
		let name = element
			.attr("name")
			.ok_or_else(|| Error::ttx("KeyError: 'name'"))?;
		let gid = font.glyph_id(name)?;
		let mut glyph = Glyph::default();
		for (attr, field) in [
			("xMin", &mut glyph.x_min),
			("yMin", &mut glyph.y_min),
			("xMax", &mut glyph.x_max),
			("yMax", &mut glyph.y_max),
		] {
			let v = element.attr(attr).unwrap_or("0");
			// LENIENT(L13): fontTools keeps float bounds and fails when packing them.
			*field = eval(v)?
				.as_int()
				.ok_or_else(|| Error::value(format!("invalid {attr}: {v:?}")))
				.and_then(int64)?;
		}
		for e in element.elements() {
			glyph.read_xml(e, font, self.extended)?;
		}
		self.glyphs.insert(gid, glyph);
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}
