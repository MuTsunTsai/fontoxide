//! A pen writing SVG path data (`fontTools.pens.svgPathPen`).

use fontoxide_misc::py::float_repr;

use crate::base_pen::{BasePen, SegmentPen};
use crate::{GlyphSet, Point, Result};

/// `str()` of a coordinate: an integer if it is integral, as the coordinates
/// of TrueType glyphs are, otherwise `repr(float)`.
pub fn py_str(x: f64) -> String {
	if x.fract() == 0.0 && x.abs() < 1e16 {
		format!("{}", x as i64)
	} else {
		float_repr(x)
	}
}

/// `pointToString`.
pub fn point_to_string(pt: Point, ntos: fn(f64) -> String) -> String {
	format!("{} {}", ntos(pt.0), ntos(pt.1))
}

/// Collects SVG path commands (`SVGPathPen`). The Y axis points up in fonts
/// but down in SVG.
#[derive(Debug, Clone)]
pub struct SvgPath {
	commands: Vec<String>,
	last_command: Option<char>,
	last: (Option<f64>, Option<f64>),
	/// How numbers are formatted (`ntos`, `str` by default).
	pub ntos: fn(f64) -> String,
}

/// `SVGPathPen`.
pub type SvgPathPen<'g> = BasePen<'g, SvgPath>;

impl<'g> SvgPathPen<'g> {
	/// `SVGPathPen(glyphSet, ntos)`.
	pub fn new_svg_path(glyph_set: Option<&'g dyn GlyphSet>, ntos: fn(f64) -> String) -> Self {
		BasePen::with_inner(
			SvgPath {
				commands: Vec::new(),
				last_command: None,
				last: (None, None),
				ntos,
			},
			glyph_set,
		)
	}

	/// `getCommands`.
	pub fn get_commands(&self) -> String {
		self.inner.commands.concat()
	}

	/// `_commands`.
	pub fn commands(&self) -> &[String] {
		&self.inner.commands
	}
}

impl SegmentPen for SvgPath {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		// `_handleAnchor`: a moveto replaces a preceding one.
		if self.last_command == Some('M') {
			self.commands.pop();
		}
		self.commands
			.push(format!("M{}", point_to_string(pt, self.ntos)));
		self.last_command = Some('M');
		self.last = (Some(pt.0), Some(pt.1));
		Ok(())
	}

	fn line_to(&mut self, pt: Point) -> Result<()> {
		let (x, y) = pt;
		let (lx, ly) = self.last;
		let (cmd, pts) = if Some(x) == lx && Some(y) == ly {
			// A duplicate point.
			return Ok(());
		} else if Some(x) == lx {
			// A vertical line.
			(Some('V'), (self.ntos)(y))
		} else if Some(y) == ly {
			// A horizontal line.
			(Some('H'), (self.ntos)(x))
		} else if self.last_command == Some('M') {
			// After a moveto.
			(None, format!(" {}", point_to_string(pt, self.ntos)))
		} else {
			(Some('L'), point_to_string(pt, self.ntos))
		};
		let mut t = String::new();
		if let Some(cmd) = cmd {
			t.push(cmd);
			self.last_command = Some(cmd);
		}
		t.push_str(&pts);
		self.commands.push(t);
		self.last = (Some(x), Some(y));
		Ok(())
	}

	fn curve_to_one(&mut self, _: Option<Point>, pt1: Point, pt2: Point, pt3: Point) -> Result<()> {
		let n = self.ntos;
		self.commands.push(format!(
			"C{} {} {}",
			point_to_string(pt1, n),
			point_to_string(pt2, n),
			point_to_string(pt3, n)
		));
		self.last_command = Some('C');
		self.last = (Some(pt3.0), Some(pt3.1));
		Ok(())
	}

	fn q_curve_to_one(&mut self, _: Option<Point>, pt1: Point, pt2: Point) -> Result<()> {
		let n = self.ntos;
		self.commands.push(format!(
			"Q{} {}",
			point_to_string(pt1, n),
			point_to_string(pt2, n)
		));
		self.last_command = Some('Q');
		self.last = (Some(pt2.0), Some(pt2.1));
		Ok(())
	}

	fn close_path(&mut self) -> Result<()> {
		self.commands.push("Z".into());
		self.last_command = Some('Z');
		self.last = (None, None);
		Ok(())
	}

	fn end_path(&mut self) -> Result<()> {
		self.last_command = None;
		self.last = (None, None);
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::Pen;
	use fontoxide_misc::round_tools::ot_round;

	fn pen() -> SvgPathPen<'static> {
		SvgPathPen::new_svg_path(None, py_str)
	}

	// The doctests of `SVGPathPen`.
	#[test]
	fn doctests() -> Result<()> {
		let mut p = pen();
		p.move_to((0.0, 0.0))?;
		p.line_to((1.0, 1.0))?;
		p.curve_to(&[(2.0, 2.0), (3.0, 3.0), (4.0, 4.0)])?;
		p.close_path()?;
		assert_eq!(p.get_commands(), "M0 0 1 1C2 2 3 3 4 4Z");

		let mut p = pen();
		p.move_to((0.0, 0.0))?;
		p.move_to((10.0, 10.0))?;
		assert_eq!(p.commands(), ["M10 10"]);

		for (pt, cmd) in [
			((0.0, 0.0), "M0 0"),
			((10.0, 0.0), "M10 0"),
			((0.0, 10.0), "M0 10"),
		] {
			let mut p = pen();
			p.move_to(pt)?;
			assert_eq!(p.commands(), [cmd]);
		}

		let mut p = pen();
		p.move_to((10.0, 10.0))?;
		p.line_to((10.0, 10.0))?;
		assert_eq!(p.commands(), ["M10 10"]);
		let mut p = pen();
		p.move_to((10.0, 10.0))?;
		p.line_to((10.0, 0.0))?;
		assert_eq!(p.commands(), ["M10 10", "V0"]);
		let mut p = pen();
		p.move_to((10.0, 10.0))?;
		p.line_to((0.0, 10.0))?;
		assert_eq!(p.commands(), ["M10 10", "H0"]);
		let mut p = pen();
		p.line_to((70.0, 80.0))?;
		assert_eq!(p.commands(), ["L70 80"]);
		let mut p = pen();
		p.move_to((0.0, 0.0))?;
		p.line_to((10.0, 10.0))?;
		assert_eq!(p.commands(), ["M0 0", " 10 10"]);

		let mut p = pen();
		p.curve_to(&[(10.0, 20.0), (30.0, 40.0), (50.0, 60.0)])?;
		assert_eq!(p.commands(), ["C10 20 30 40 50 60"]);
		let mut p = pen();
		p.q_curve_to(&[(10.0, 20.0)], Some((30.0, 40.0)))?;
		assert_eq!(p.commands(), ["Q10 20 30 40"]);
		let mut p = SvgPathPen::new_svg_path(None, |v| ot_round(v).to_string());
		p.q_curve_to(&[(3.0, 3.0), (7.0, 5.0)], Some((11.0, 4.0)))?;
		assert_eq!(p.commands(), ["Q3 3 5 4", "Q7 5 11 4"]);

		let mut p = pen();
		p.close_path()?;
		assert_eq!(p.commands(), ["Z"]);
		let mut p = pen();
		p.end_path()?;
		assert!(p.commands().is_empty());
		Ok(())
	}
}
