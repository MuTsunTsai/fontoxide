//! Port of `fontTools.varLib.interpolatableHelpers`.

use std::collections::{BTreeSet, HashMap, VecDeque};

use fontoxide_misc::complex::Complex;
use fontoxide_misc::py::sum_floats;
use fontoxide_pens::base_pen::SegmentPen;
use fontoxide_pens::recording_pen::RecordingPen;
use fontoxide_pens::statistics_pen::Statistics;
use fontoxide_pens::{BasePen, Pen, Point, PointPen, SegmentType, Transform};

use super::scipy::{linear_sum_assignment, minimum_spanning_tree};
use super::{InterpolatableError, Problem, ProblemType, Problems, Result};

/// A location as a Python `dict`: axis names and values, in insertion order.
pub type Location = Vec<(String, f64)>;

/// The vector of a contour made by [`contour_vector_from_stats`]: signed size,
/// mean x and y, twice the standard deviations, and the correlation scaled by
/// the size.
pub type ContourVector = [f64; 6];

/// `sort_problems`: the glyphs sorted by their most severe problem (the
/// smallest severity plus tolerance), keeping the order of ties.
pub fn sort_problems(problems: Problems) -> Problems {
	let mut items: Vec<(f64, (String, Vec<Problem>))> = problems
		.into_iter()
		.map(|(name, list)| {
			let key = list
				.iter()
				.map(|p| p.kind().severity() as f64 + p.tolerance().unwrap_or(0.0))
				.fold(f64::INFINITY, |a, b| if b < a { b } else { a });
			(key, (name, list))
		})
		.collect();
	// LENIENT(L40): `sorted(..., key=lambda _: -min(...), reverse=True)` with NaN
	// tolerances has no meaningful order; a total order is used here.
	items.sort_by(|a, b| a.0.total_cmp(&b.0));
	items.into_iter().map(|(_, item)| item).collect()
}

/// `rot_list(l, k)`: the list rotated by `k` items forward, so that the item
/// at position 0 is at position `k`. Negative `k` is allowed.
pub fn rot_list<T: Clone>(l: &[T], k: isize) -> Vec<T> {
	let n = l.len() as isize;
	if n == 0 {
		return Vec::new();
	}
	// `l[-k:] + l[:-k]`, with Python's clamping of slice bounds.
	let split = if k > 0 { (n - k).max(0) } else { (-k).min(n) } as usize;
	let mut out = l[split..].to_vec();
	out.extend_from_slice(&l[..split]);
	out
}

/// The pens of the contours of a [`PerContourPen`] (`PerContourPen` without
/// the curve splitting of `BasePen`).
#[derive(Debug, Default)]
pub struct PerContour {
	/// One recording per contour or component (`value`).
	pub value: Vec<RecordingPen>,
	/// The recording being drawn into (`_pen`).
	current: Option<usize>,
}

impl PerContour {
	fn pen(&mut self) -> fontoxide_pens::Result<&mut RecordingPen> {
		match self.current.and_then(|i| self.value.get_mut(i)) {
			Some(pen) => Ok(pen),
			None => Err(fontoxide_pens::Error::Usage(
				"AttributeError: 'NoneType' object has no attribute".into(),
			)),
		}
	}

	/// `_newItem`.
	fn new_item(&mut self) -> &mut RecordingPen {
		self.value.push(RecordingPen::new());
		self.current = Some(self.value.len() - 1);
		let last = self.value.len() - 1;
		&mut self.value[last]
	}
}

impl SegmentPen for PerContour {
	fn move_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.new_item().move_to(pt)
	}
	fn line_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.pen()?.line_to(pt)
	}
	fn q_curve_to_one(
		&mut self,
		_: Option<Point>,
		p1: Point,
		p2: Point,
	) -> fontoxide_pens::Result<()> {
		self.pen()?.q_curve_to(&[p1], Some(p2))
	}
	fn curve_to_one(
		&mut self,
		_: Option<Point>,
		p1: Point,
		p2: Point,
		p3: Point,
	) -> fontoxide_pens::Result<()> {
		self.pen()?.curve_to(&[p1, p2, p3])
	}
	fn close_path(&mut self) -> fontoxide_pens::Result<()> {
		self.pen()?.close_path()?;
		self.current = None;
		Ok(())
	}
	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		self.pen()?.end_path()?;
		self.current = None;
		Ok(())
	}
}

/// `PerContourPen(RecordingPen)`: records each contour separately, with the
/// curves split into single segments.
pub type PerContourPen<'g> = BasePen<'g, PerContour>;

/// `PerContourOrComponentPen(RecordingPen)`: a [`PerContourPen`] that also
/// records each component separately, without decomposing it.
#[derive(Debug)]
pub struct PerContourOrComponentPen<'g> {
	/// The pen.
	pub pen: PerContourPen<'g>,
}

impl Default for PerContourOrComponentPen<'_> {
	fn default() -> Self {
		PerContourOrComponentPen {
			pen: BasePen::with_inner(PerContour::default(), None),
		}
	}
}

impl PerContourOrComponentPen<'_> {
	/// `value`: the recordings.
	pub fn into_value(self) -> Vec<RecordingPen> {
		self.pen.inner.value
	}
}

impl Pen for PerContourOrComponentPen<'_> {
	fn move_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.pen.move_to(pt)
	}
	fn line_to(&mut self, pt: Point) -> fontoxide_pens::Result<()> {
		self.pen.line_to(pt)
	}
	fn curve_to(&mut self, points: &[Point]) -> fontoxide_pens::Result<()> {
		self.pen.curve_to(points)
	}
	fn q_curve_to(
		&mut self,
		off_curves: &[Point],
		on_curve: Option<Point>,
	) -> fontoxide_pens::Result<()> {
		self.pen.q_curve_to(off_curves, on_curve)
	}
	fn close_path(&mut self) -> fontoxide_pens::Result<()> {
		self.pen.close_path()
	}
	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		self.pen.end_path()
	}
	fn add_component(
		&mut self,
		glyph_name: &str,
		transformation: Transform,
	) -> fontoxide_pens::Result<()> {
		self.pen
			.inner
			.new_item()
			.add_component(glyph_name, transformation)
	}
}

/// `SimpleRecordingPointPen`: records the points and whether they are on the
/// curve.
#[derive(Debug, Default, Clone)]
pub struct SimpleRecordingPointPen {
	/// `value`: the points, and whether each is on the curve.
	pub value: Vec<(Point, bool)>,
}

impl PointPen for SimpleRecordingPointPen {
	fn begin_path(&mut self, _: Option<&str>) -> fontoxide_pens::Result<()> {
		Ok(())
	}
	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		Ok(())
	}
	fn add_point(
		&mut self,
		pt: Point,
		segment_type: Option<SegmentType>,
		_: bool,
		_: Option<&str>,
		_: Option<&str>,
	) -> fontoxide_pens::Result<()> {
		self.value.push((pt, segment_type.is_some()));
		Ok(())
	}
	fn add_component(
		&mut self,
		_: &str,
		_: Transform,
		_: Option<&str>,
	) -> fontoxide_pens::Result<()> {
		// `AbstractPointPen.addComponent` raises `NotImplementedError`.
		Err(fontoxide_pens::Error::Usage("NotImplementedError".into()))
	}
}

/// `vdiff_hypot2(v0, v1)`: the squared distance of two vectors, summed in
/// order.
pub fn vdiff_hypot2(v0: &[f64], v1: &[f64]) -> f64 {
	let mut s = 0.0;
	for (x0, x1) in v0.iter().zip(v1) {
		let d = x1 - x0;
		s += d * d;
	}
	s
}

/// `vdiff_hypot2_complex(v0, v1)`: the squared distance of two complex
/// vectors.
pub fn vdiff_hypot2_complex(v0: &[Complex], v1: &[Complex]) -> f64 {
	let mut s = 0.0;
	for (x0, x1) in v0.iter().zip(v1) {
		let d = *x1 - *x0;
		s += d.re * d.re + d.im * d.im;
	}
	s
}

/// `matching_cost(G, matching)`: `sum(G[i][j] for i, j in enumerate(matching))`.
pub fn matching_cost(g: &[Vec<f64>], matching: &[usize]) -> f64 {
	sum_floats(matching.iter().enumerate().map(|(i, &j)| g[i][j]))
}

/// `min_cost_perfect_bipartite_matching_scipy(G)`: the matching found by
/// `scipy.optimize.linear_sum_assignment`, and its cost.
pub fn min_cost_perfect_bipartite_matching_scipy(g: &[Vec<f64>]) -> Result<(Vec<usize>, f64)> {
	let cols = linear_sum_assignment(g)?;
	let cost = matching_cost(g, &cols);
	Ok((cols, cost))
}

/// `min_cost_perfect_bipartite_matching(G)`: fontTools picks the SciPy
/// implementation when SciPy is installed, as it is with `fonttools[all]`.
pub fn min_cost_perfect_bipartite_matching(g: &[Vec<f64>]) -> Result<(Vec<usize>, f64)> {
	min_cost_perfect_bipartite_matching_scipy(g)
}

/// `contour_vector_from_stats(stats)`.
pub fn contour_vector_from_stats(stats: &Statistics) -> ContourVector {
	// Don't change the order of items here.
	let size = stats.area.abs().sqrt();
	[
		size.copysign(stats.area),
		stats.mean_x,
		stats.mean_y,
		stats.stddev_x * 2.0,
		stats.stddev_y * 2.0,
		stats.correlation * size,
	]
}

/// `matching_for_vectors(m0, m1)`: the best matching of the contours, its cost,
/// and the cost of the identity matching.
pub fn matching_for_vectors(
	m0: &[ContourVector],
	m1: &[ContourVector],
) -> Result<(Vec<usize>, f64, f64)> {
	let n = m0.len();
	let costs: Vec<Vec<f64>> = m0
		.iter()
		.map(|v0| m1.iter().map(|v1| vdiff_hypot2(v0, v1)).collect())
		.collect();
	let (matching, matching_cost) = min_cost_perfect_bipartite_matching(&costs)?;
	let identity_cost = sum_floats((0..n).map(|i| costs[i].get(i).copied().unwrap_or(f64::NAN)));
	Ok((matching, matching_cost, identity_cost))
}

/// `points_characteristic_bits(points)`: whether each point is on the curve.
/// fontTools packs them in an integer, the first point in the lowest bit.
pub fn points_characteristic_bits(points: &[(Point, bool)]) -> Vec<bool> {
	points.iter().map(|&(_, b)| b).collect()
}

/// `points_complex_vector(points)`: for each point, the point, the vector to
/// the next point, the turn vector and the angle to the next point.
pub fn points_complex_vector(points: &[(Point, bool)]) -> Vec<Complex> {
	let mut vector = Vec::new();
	if points.is_empty() {
		return vector;
	}
	let pts: Vec<Complex> = points
		.iter()
		.map(|&(pt, _)| Complex::from_point(pt))
		.collect();
	let n = pts.len();
	for i in 0..n {
		// The weights are magic numbers.

		// The point itself
		let p0 = pts[i];
		vector.push(p0);

		// The vector to the next point
		let p1 = pts[(i + 1) % n];
		let d0 = p1 - p0;
		vector.push(d0 * 3.0);

		// The turn vector
		let p2 = pts[(i + 2) % n];
		let d1 = p2 - p1;
		vector.push(d1 - d0);

		// The angle to the next point, as a cross product; square root of, to
		// match dimentionality of distance.
		let cross = d0.re * d1.im - d0.im * d1.re;
		let cross = cross.abs().sqrt().copysign(cross);
		vector.push(Complex::real(cross * 4.0));
	}
	vector
}

/// A starting point of a contour that keeps the pattern of on- and off-curve
/// points: the complex vector of the contour from there, the index of the
/// point, and whether the contour is reversed.
#[derive(Debug, Clone, PartialEq)]
pub struct Isomorphism {
	/// The complex vector ([`points_complex_vector`]), rotated.
	pub vector: Vec<Complex>,
	/// The index of the starting point in the original contour.
	pub point: usize,
	/// Whether the contour is reversed.
	pub reverse: bool,
}

/// `add_isomorphisms(points, isomorphisms, reverse)`: adds the rotations, or
/// the mirrored rotations, of the contour that keep its pattern of on- and
/// off-curve points.
pub fn add_isomorphisms(
	points: &[(Point, bool)],
	isomorphisms: &mut Vec<Isomorphism>,
	reverse: bool,
) {
	let reference_bits = points_characteristic_bits(points);
	let n = points.len();
	if n == 0 {
		// fontTools fails with a ZeroDivisionError; contours always have a point.
		return;
	}
	let reversed: Vec<(Point, bool)>;
	let points = if reverse {
		reversed = points.iter().rev().copied().collect();
		&reversed[..]
	} else {
		points
	};
	let bits = points_characteristic_bits(points);
	let vector = points_complex_vector(points);
	let mult = vector.len() / n;
	for i in 0..n {
		// `((bits << (n - i)) & mask) | (bits >> i)`: bit k is bit (k + i) % n.
		let matches = (0..n).all(|k| bits[(k + i) % n] == reference_bits[k]);
		if matches {
			isomorphisms.push(Isomorphism {
				vector: rot_list(&vector, -((i * mult) as isize)),
				point: if reverse { n - 1 - i } else { i },
				reverse,
			});
		}
	}
}

/// `find_parents_and_order(glyphsets, locations, discrete_axes=...)`: the
/// master each master is compared to, and the order in which they are
/// compared, for `count` masters.
///
/// With locations, the masters form a minimum spanning tree of their
/// locations, traversed from the base masters; without, each master is
/// compared to the previous one.
pub fn find_parents_and_order(
	count: usize,
	locations: Option<&[Location]>,
	discrete_axes: &BTreeSet<String>,
) -> Result<(Vec<Option<usize>>, Vec<usize>)> {
	let Some(locations) = locations.filter(|l| !l.is_empty()) else {
		let parents = (0..count)
			.map(|i| if i == 0 { None } else { Some(i - 1) })
			.collect();
		return Ok((parents, (0..count).collect()));
	};
	// Order base master first
	let bases: Vec<usize> = locations
		.iter()
		.enumerate()
		.filter(|(_, l)| {
			l.iter()
				.filter(|(k, _)| !discrete_axes.contains(k))
				.all(|(_, v)| *v == 0.0)
		})
		.map(|(i, _)| i)
		.collect();
	if bases.is_empty() {
		log::warn!("No base master location found");
	} else {
		log::info!("Found {} base masters: {:?}", bases.len(), bases);
	}

	// Form a minimum spanning tree of the locations
	let n = locations.len();
	let mut graph = vec![vec![0.0f64; n]; n];
	let axes: BTreeSet<&String> = locations.iter().flatten().map(|(k, _)| k).collect();
	let axes: Vec<&String> = axes.into_iter().collect();
	let get = |l: &Location, k: &str| {
		l.iter()
			.rev()
			.find(|(name, _)| name == k)
			.map_or(0.0, |(_, v)| *v)
	};
	let vectors: Vec<Vec<f64>> = locations
		.iter()
		.map(|l| axes.iter().map(|k| get(l, k)).collect())
		.collect();
	for i in 0..n {
		for j in i + 1..n {
			let discrete = |v: &[f64]| -> Vec<f64> {
				axes.iter()
					.zip(v)
					.filter(|(k, _)| discrete_axes.contains(k.as_str()))
					.map(|(_, v)| *v)
					.collect()
			};
			if discrete(&vectors[i]) != discrete(&vectors[j]) {
				continue;
			}
			graph[i][j] = vdiff_hypot2(&vectors[i], &vectors[j]);
		}
	}

	let tree = minimum_spanning_tree(&graph);
	let mut adjacency: HashMap<usize, BTreeSet<usize>> = HashMap::new();
	for (row, col) in tree {
		adjacency.entry(row).or_default().insert(col);
		adjacency.entry(col).or_default().insert(row);
	}

	// Traverse graph from the base and assign parents
	let mut parents = vec![None; n];
	let mut order = Vec::new();
	let mut visited = vec![false; n];
	let mut queue: VecDeque<usize> = bases.iter().copied().collect();
	while let Some(i) = queue.pop_front() {
		visited[i] = true;
		order.push(i);
		if let Some(neighbors) = adjacency.get(&i) {
			for &j in neighbors {
				if !visited[j] {
					parents[j] = Some(i);
					queue.push_back(j);
				}
			}
		}
	}
	if order.len() != parents.len() {
		return Err(InterpolatableError::Assertion(
			"Not all masters are reachable; report an issue".into(),
		));
	}

	log::info!("Parents: {parents:?}");
	log::info!("Order: {order:?}");
	Ok((parents, order))
}

impl ProblemType {
	/// `InterpolatableProblem.severity`.
	pub fn severity(self) -> u32 {
		match self {
			ProblemType::Missing => 1,
			ProblemType::OpenPath => 2,
			ProblemType::PathCount => 3,
			ProblemType::NodeCount => 4,
			ProblemType::NodeIncompatibility => 5,
			ProblemType::ContourOrder => 6,
			ProblemType::WrongStartPoint => 7,
			ProblemType::Kink => 8,
			ProblemType::Underweight => 9,
			ProblemType::Overweight => 10,
			ProblemType::Nothing => 11,
		}
	}
}
