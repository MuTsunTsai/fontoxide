//! A pen converting quadratic splines to cubic curves
//! (`fontTools.pens.qu2cuPen`).

use std::collections::BTreeMap;

use fontoxide_misc::py::is_close;
use fontoxide_qu2cu::quadratic_to_curves;

use crate::recording_pen::PenOp;
use crate::reverse_contour_pen::ReverseContourPen;
use crate::{Error, Pen, Point, Result, Transform};

enum Out<'p> {
	Plain(&'p mut dyn Pen),
	Reverse(ReverseContourPen<'p>),
}

impl Out<'_> {
	fn pen(&mut self) -> &mut dyn Pen {
		match self {
			Out::Plain(p) => *p,
			Out::Reverse(p) => p,
		}
	}
}

/// Converts quadratic splines to cubic curves within `max_err`
/// (`Qu2CuPen`). Each contour is buffered and converted when it ends.
pub struct Qu2CuPen<'p> {
	out: Out<'p>,
	value: Vec<PenOp>,
	/// `max_err`.
	pub max_err: f64,
	/// `all_cubic`: only cubic curves.
	pub all_cubic: bool,
	/// The number of curves made, by the number of off-curve points as a
	/// string (`stats`); counted if `Some`.
	pub stats: Option<BTreeMap<String, usize>>,
}

impl std::fmt::Debug for Qu2CuPen<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Qu2CuPen")
			.field("max_err", &self.max_err)
			.field("all_cubic", &self.all_cubic)
			.field("stats", &self.stats)
			.finish_non_exhaustive()
	}
}

/// A segment as `(op, args)`; `None` for the missing on-curve point of an
/// off-curve-only `qCurveTo`.
type Seg = (&'static str, Vec<Option<Point>>);

fn to_seg(op: &PenOp) -> Seg {
	(op.operator(), op.points())
}

fn from_seg((op, args): Seg) -> Result<PenOp> {
	let all = |pts: Vec<Option<Point>>| -> Result<Vec<Point>> {
		pts.into_iter()
			.collect::<Option<Vec<_>>>()
			.ok_or_else(|| Error::Usage("TypeError: a point is None".into()))
	};
	Ok(match op {
		"moveTo" => PenOp::MoveTo(all(args)?[0]),
		"lineTo" => PenOp::LineTo(all(args)?[0]),
		"curveTo" => PenOp::CurveTo(all(args)?),
		"qCurveTo" => {
			let mut args = args;
			let on = args.pop().flatten();
			PenOp::QCurveTo(all(args)?, on)
		}
		"closePath" => PenOp::ClosePath,
		_ => PenOp::EndPath,
	})
}

impl<'p> Qu2CuPen<'p> {
	/// `Qu2CuPen(other_pen, max_err, all_cubic, reverse_direction)`, without
	/// statistics.
	pub fn new(
		other_pen: &'p mut dyn Pen,
		max_err: f64,
		all_cubic: bool,
		reverse_direction: bool,
	) -> Self {
		Qu2CuPen {
			out: if reverse_direction {
				Out::Reverse(ReverseContourPen::new(other_pen, false))
			} else {
				Out::Plain(other_pen)
			},
			value: Vec::new(),
			max_err,
			all_cubic,
			stats: None,
		}
	}

	/// `_quadratics_to_curve`.
	fn quadratics_to_curve(&mut self, q: &[Vec<Option<Point>>]) -> Result<Vec<Seg>> {
		let quads: Vec<Vec<Point>> = q
			.iter()
			.map(|s| s.iter().copied().collect::<Option<Vec<_>>>())
			.collect::<Option<_>>()
			.ok_or_else(|| {
				Error::Usage("TypeError: cannot unpack non-iterable NoneType object".into())
			})?;
		let curves = quadratic_to_curves(&quads, self.max_err, self.all_cubic)
			.map_err(|e| Error::Glyph(Box::new(e)))?;
		if let Some(stats) = &mut self.stats {
			for curve in &curves {
				*stats.entry((curve.len() - 2).to_string()).or_insert(0) += 1;
			}
		}
		Ok(curves
			.into_iter()
			.map(|curve| {
				let op = if curve.len() == 4 {
					"curveTo"
				} else {
					"qCurveTo"
				};
				(op, curve[1..].iter().copied().map(Some).collect())
			})
			.collect())
	}

	/// `filterContour`.
	fn filter_contour(&mut self, contour: Vec<PenOp>) -> Result<Vec<PenOp>> {
		let mut quadratics: Vec<Vec<Option<Point>>> = Vec::new();
		let mut current_pt: Option<Point> = None;
		let mut new_contour: Vec<Seg> = Vec::new();
		for op in &contour {
			let (name, args) = to_seg(op);
			let last = args.last().copied().flatten();
			if name == "qCurveTo" && (self.all_cubic || (args.len() > 2 && last.is_some())) {
				if last.is_none() {
					return Err(Error::Usage(
						"NotImplementedError: oncurve-less contours with all_cubic not implemented"
							.into(),
					));
				}
				let mut q = vec![current_pt];
				q.extend(args.iter().copied());
				quadratics.push(q);
			} else {
				if !quadratics.is_empty() {
					let segs = self.quadratics_to_curve(&quadratics)?;
					new_contour.extend(segs);
					quadratics.clear();
				}
				new_contour.push((name, args.clone()));
			}
			current_pt = if args.is_empty() { None } else { last };
		}
		if !quadratics.is_empty() {
			let segs = self.quadratics_to_curve(&quadratics)?;
			new_contour.extend(segs);
		}

		if !self.all_cubic {
			// Add the implicit on-curve points back.
			let contour = std::mem::take(&mut new_contour);
			for (op, args) in contour {
				if op == "qCurveTo"
					&& let Some((prev_op, prev_args)) = new_contour.last_mut()
					&& *prev_op == "qCurveTo"
				{
					let n = prev_args.len();
					let pt0 = prev_args[n - 2];
					let pt1 = prev_args[n - 1];
					let pt2 = args[0];
					if let (Some(pt0), Some(pt1), Some(pt2)) = (pt0, pt1, pt2)
						&& is_close(pt2.0 - pt1.0, pt1.0 - pt0.0)
						&& is_close(pt2.1 - pt1.1, pt1.1 - pt0.1)
					{
						prev_args.pop();
						prev_args.extend(args);
						continue;
					}
				}
				new_contour.push((op, args));
			}
		}

		new_contour.into_iter().map(from_seg).collect()
	}

	fn flush_contour(&mut self) -> Result<()> {
		let contour = std::mem::take(&mut self.value);
		let result = self.filter_contour(contour)?;
		for op in result {
			op.replay(self.out.pen())?;
		}
		Ok(())
	}
}

impl Pen for Qu2CuPen<'_> {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.value.push(PenOp::MoveTo(pt));
		Ok(())
	}
	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.value.push(PenOp::LineTo(pt));
		Ok(())
	}
	fn curve_to(&mut self, points: &[Point]) -> Result<()> {
		self.value.push(PenOp::CurveTo(points.to_vec()));
		Ok(())
	}
	fn q_curve_to(&mut self, off_curves: &[Point], on_curve: Option<Point>) -> Result<()> {
		self.value
			.push(PenOp::QCurveTo(off_curves.to_vec(), on_curve));
		Ok(())
	}
	fn close_path(&mut self) -> Result<()> {
		self.value.push(PenOp::ClosePath);
		self.flush_contour()
	}
	fn end_path(&mut self) -> Result<()> {
		self.value.push(PenOp::EndPath);
		self.flush_contour()
	}
	fn add_component(&mut self, glyph_name: &str, transformation: Transform) -> Result<()> {
		self.out.pen().add_component(glyph_name, transformation)
	}
}
