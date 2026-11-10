//! A pen drawing onto several pens (`fontTools.pens.teePen`).

use crate::{Pen, Point, Result, Transform};

/// Passes every call on to each of its pens, in order (`TeePen`).
pub struct TeePen<'p> {
	/// `pens`.
	pub pens: Vec<&'p mut dyn Pen>,
}

impl std::fmt::Debug for TeePen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TeePen")
			.field("pens", &self.pens.len())
			.finish()
	}
}

impl<'p> TeePen<'p> {
	/// `TeePen(*pens)`.
	pub fn new(pens: Vec<&'p mut dyn Pen>) -> Self {
		TeePen { pens }
	}

	fn each(&mut self, mut f: impl FnMut(&mut dyn Pen) -> Result<()>) -> Result<()> {
		self.pens.iter_mut().try_for_each(|p| f(&mut **p))
	}
}

impl Pen for TeePen<'_> {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.each(|p| p.move_to(pt))
	}
	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.each(|p| p.line_to(pt))
	}
	fn curve_to(&mut self, points: &[Point]) -> Result<()> {
		self.each(|p| p.curve_to(points))
	}
	fn q_curve_to(&mut self, off_curves: &[Point], on_curve: Option<Point>) -> Result<()> {
		self.each(|p| p.q_curve_to(off_curves, on_curve))
	}
	fn close_path(&mut self) -> Result<()> {
		self.each(|p| p.close_path())
	}
	fn end_path(&mut self) -> Result<()> {
		self.each(|p| p.end_path())
	}
	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> Result<()> {
		self.each(|p| p.add_component(glyph_name, transformation))
	}
}
