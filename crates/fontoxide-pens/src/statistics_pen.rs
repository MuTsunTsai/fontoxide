//! Area, center of mass, variance, covariance, correlation and slant of a
//! glyph (`fontTools.pens.statisticsPen`).

use fontoxide_misc::complex::Complex;
use fontoxide_misc::py::sum_floats;

use crate::base_pen::{BasePen, SegmentPen};
use crate::moments_pen::Moments;
use crate::{Error, GlyphSet, Point, Result};

/// The statistics (`StatisticsBase`).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Statistics {
	/// `area`.
	pub area: f64,
	/// `meanX`.
	pub mean_x: f64,
	/// `meanY`.
	pub mean_y: f64,
	/// `varianceX`.
	pub variance_x: f64,
	/// `varianceY`.
	pub variance_y: f64,
	/// `stddevX`.
	pub stddev_x: f64,
	/// `stddevY`.
	pub stddev_y: f64,
	/// `covariance`.
	pub covariance: f64,
	/// `correlation`.
	pub correlation: f64,
	/// `slant`.
	pub slant: f64,
}

impl Statistics {
	/// `StatisticsBase._update`, from the variances and the covariance.
	fn update(&mut self) {
		// The variances should never be negative, but both pens produce
		// negative values; fontTools takes the absolute value.
		self.variance_x = self.variance_x.abs();
		self.variance_y = self.variance_y.abs();
		let stddev_x = self.variance_x.sqrt();
		let stddev_y = self.variance_y.sqrt();
		self.stddev_x = stddev_x;
		self.stddev_y = stddev_y;
		let correlation = if stddev_x * stddev_y == 0.0 {
			f64::NAN
		} else {
			// Clamped to [-1, 1] (`max(-1, min(1, correlation))`).
			let c = self.covariance / (stddev_x * stddev_y);
			let m = if c < 1.0 { c } else { 1.0 };
			if m < -1.0 { -1.0 } else { m }
		};
		// A NaN is not above 1e-3: it becomes 0.
		self.correlation = if correlation.abs() > 1e-3 {
			correlation
		} else {
			0.0
		};
		let slant = if self.variance_y != 0.0 {
			self.covariance / self.variance_y
		} else {
			f64::NAN
		};
		self.slant = if slant.abs() > 1e-3 { slant } else { 0.0 };
	}
}

/// The moments and the statistics derived from them (`StatisticsPen`).
#[derive(Debug, Default, Clone)]
pub struct StatisticsMoments {
	/// The moments.
	pub moments: Moments,
	/// The statistics; `area` duplicates the one of the moments.
	pub stats: Statistics,
}

/// Computes statistics from the moments of the outline (`StatisticsPen`).
/// They are wrong, but well defined, for self-intersecting outlines; the
/// area is negative for clockwise contours.
pub type StatisticsPen<'g> = BasePen<'g, StatisticsMoments>;

impl<'g> StatisticsPen<'g> {
	/// `StatisticsPen(glyphset)`.
	pub fn new_statistics(glyph_set: Option<&'g dyn GlyphSet>) -> Self {
		BasePen::with_inner(StatisticsMoments::default(), glyph_set)
	}

	/// The statistics. The area is the one of the moments (the same attribute
	/// in fontTools).
	pub fn statistics(&self) -> Statistics {
		Statistics {
			area: self.inner.moments.area,
			..self.inner.stats
		}
	}
}

impl StatisticsMoments {
	/// `StatisticsPen._update`.
	fn update(&mut self) {
		let m = &self.moments;
		let area = m.area;
		if area == 0.0 {
			self.stats = Statistics::default();
			return;
		}
		let s = &mut self.stats;
		s.area = area;
		// Center of mass.
		let mean_x = m.moment_x / area;
		let mean_y = m.moment_y / area;
		s.mean_x = mean_x;
		s.mean_y = mean_y;
		// Var(X) = E[X^2] - E[X]^2
		s.variance_x = m.moment_xx / area - mean_x * mean_x;
		s.variance_y = m.moment_yy / area - mean_y * mean_y;
		// Covariance(X,Y) = E[X.Y] - E[X]E[Y]
		s.covariance = m.moment_xy / area - mean_x * mean_y;
		s.update();
	}
}

impl SegmentPen for StatisticsMoments {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.moments.move_to(pt)
	}
	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.moments.line_to(pt)
	}
	fn q_curve_to_one(&mut self, c: Option<Point>, p1: Point, p2: Point) -> Result<()> {
		self.moments.q_curve_to_one(c, p1, p2)
	}
	fn curve_to_one(&mut self, c: Option<Point>, p1: Point, p2: Point, p3: Point) -> Result<()> {
		self.moments.curve_to_one(c, p1, p2, p3)
	}
	fn close_path(&mut self) -> Result<()> {
		self.moments.close()?;
		self.update();
		Ok(())
	}
	fn end_path(&mut self) -> Result<()> {
		self.moments.end()
	}
}

/// Statistics of the control polygon (`StatisticsControlPen`).
#[derive(Debug, Default, Clone)]
pub struct ControlStatistics {
	/// The statistics.
	pub stats: Statistics,
	nodes: Vec<Complex>,
	start: Option<Point>,
	current: Option<Point>,
}

/// Computes statistics from the control polygon of the outline
/// (`StatisticsControlPen`).
pub type StatisticsControlPen<'g> = BasePen<'g, ControlStatistics>;

impl<'g> StatisticsControlPen<'g> {
	/// `StatisticsControlPen(glyphset)`.
	pub fn new_statistics_control(glyph_set: Option<&'g dyn GlyphSet>) -> Self {
		BasePen::with_inner(ControlStatistics::default(), glyph_set)
	}

	/// The statistics.
	pub fn statistics(&self) -> Statistics {
		self.inner.stats
	}
}

impl ControlStatistics {
	/// `StatisticsControlPen._update`.
	fn update(&mut self) {
		let nodes = &self.nodes;
		let n = nodes.len() as f64;
		let s = &mut self.stats;
		// Triangle formula. `sum` of floats is compensated (Python 3.12+).
		let area = sum_floats(nodes.iter().enumerate().map(|(i, p0)| {
			let p1 = nodes[(i + 1) % nodes.len()];
			p0.re * p1.im - p1.re * p0.im
		}));
		s.area = area / 2.0;
		// Center of mass. `sum` of complex numbers is not compensated in
		// Python 3.13; it starts with the integer 0.
		let mut sum_nodes = Complex::real(0.0);
		for &p in nodes {
			sum_nodes += p;
		}
		s.mean_x = sum_nodes.re / n;
		s.mean_y = sum_nodes.im / n;
		if nodes.len() > 1 {
			let sum = |f: &dyn Fn(&Complex) -> f64| sum_floats(nodes.iter().map(f));
			s.variance_x = (sum(&|p| p.re * p.re) - (sum_nodes.re * sum_nodes.re) / n) / (n - 1.0);
			s.variance_y = (sum(&|p| p.im * p.im) - (sum_nodes.im * sum_nodes.im) / n) / (n - 1.0);
			s.covariance = (sum(&|p| p.re * p.im) - (sum_nodes.re * sum_nodes.im) / n) / (n - 1.0);
		} else {
			s.variance_x = 0.0;
			s.variance_y = 0.0;
			s.covariance = 0.0;
		}
		s.update();
	}
}

impl SegmentPen for ControlStatistics {
	fn move_to(&mut self, pt: Point) -> Result<()> {
		self.nodes.push(Complex::from_point(pt));
		self.start = Some(pt);
		self.current = Some(pt);
		Ok(())
	}
	fn line_to(&mut self, pt: Point) -> Result<()> {
		self.nodes.push(Complex::from_point(pt));
		self.current = Some(pt);
		Ok(())
	}
	fn q_curve_to_one(&mut self, _: Option<Point>, p1: Point, p2: Point) -> Result<()> {
		self.nodes.extend([p1, p2].map(Complex::from_point));
		self.current = Some(p2);
		Ok(())
	}
	fn curve_to_one(&mut self, _: Option<Point>, p1: Point, p2: Point, p3: Point) -> Result<()> {
		self.nodes.extend([p1, p2, p3].map(Complex::from_point));
		self.current = Some(p3);
		Ok(())
	}
	fn close_path(&mut self) -> Result<()> {
		if self.current != self.start
			&& let Some(start) = self.start
		{
			self.nodes.push(Complex::from_point(start));
		}
		self.current = None;
		self.update();
		Ok(())
	}
	fn end_path(&mut self) -> Result<()> {
		if self.current != self.start {
			return Err(Error::Pen(
				"Glyph statistics not defined on open contours.".into(),
			));
		}
		self.current = None;
		self.update();
		Ok(())
	}
}
