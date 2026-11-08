//! The parts of `fontTools.varLib` that ttLib relies on to evaluate variations
//! at a location: `models.supportScalar`, `iup.iup_delta`, and the instancers of
//! `varStore` and `multiVarStore`.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use crate::generated::ot;
use crate::ot::varc::NO_VARIATION_INDEX;
use crate::{Error, Result};

/// A location in normalized coordinates, by axis tag. Fonts without `fvar` use
/// the axis indices, as strings, as their tags.
pub type Location = BTreeMap<String, f64>;

/// The region of a master on one axis: lower, peak and upper.
pub type Triple = (f64, f64, f64);

/// `supportScalar(location, support)` with OpenType semantics: the factor of a
/// master with the given support (in the order of its axes) at `location`.
pub fn support_scalar<'a>(
	location: &Location,
	support: impl IntoIterator<Item = (&'a str, Triple)>,
) -> f64 {
	let mut scalar = 1.0;
	for (axis, (lower, peak, upper)) in support {
		if peak == 0.0 || lower > peak || peak > upper || (lower < 0.0 && upper > 0.0) {
			continue;
		}
		let v = location.get(axis).copied().unwrap_or(0.0);
		if v == peak {
			continue;
		}
		if v <= lower || upper <= v {
			scalar = 0.0;
			break;
		}
		if v < peak {
			scalar *= (v - lower) / (peak - lower);
		} else {
			scalar *= (v - upper) / (peak - upper);
		}
	}
	scalar
}

type Point = (f64, f64);

/// `iup_segment`: interpolates the deltas of `coords` between two reference
/// points.
fn iup_segment(coords: &[Point], rc1: Point, rd1: Point, rc2: Point, rd2: Point) -> Vec<Point> {
	let mut out = [Vec::new(), Vec::new()];
	for (j, out) in out.iter_mut().enumerate() {
		let pick = |p: Point| if j == 0 { p.0 } else { p.1 };
		let (mut x1, mut x2, mut d1, mut d2) = (pick(rc1), pick(rc2), pick(rd1), pick(rd2));
		if x1 == x2 {
			let d = if d1 == d2 { d1 } else { 0.0 };
			out.extend(std::iter::repeat_n(d, coords.len()));
			continue;
		}
		if x1 > x2 {
			std::mem::swap(&mut x1, &mut x2);
			std::mem::swap(&mut d1, &mut d2);
		}
		let scale = (d2 - d1) / (x2 - x1);
		for &p in coords {
			let x = pick(p);
			out.push(if x <= x1 {
				d1
			} else if x >= x2 {
				d2
			} else {
				d1 + (x - x1) * scale
			});
		}
	}
	out[0].iter().copied().zip(out[1].iter().copied()).collect()
}

/// `iup_contour`: fills in the missing deltas of a contour.
fn iup_contour(deltas: &[Option<Point>], coords: &[Point]) -> Vec<Point> {
	let indices: Vec<usize> = (0..deltas.len()).filter(|&i| deltas[i].is_some()).collect();
	let d = |i: usize| deltas[i].unwrap_or((0.0, 0.0));
	if indices.len() == deltas.len() {
		return deltas.iter().map(|d| d.unwrap_or((0.0, 0.0))).collect();
	}
	let n = deltas.len();
	let (Some(&first), Some(&last)) = (indices.first(), indices.last()) else {
		return vec![(0.0, 0.0); n];
	};
	let mut out = Vec::with_capacity(n);
	let mut start = first;
	if start != 0 {
		out.extend(iup_segment(
			&coords[..start],
			coords[start],
			d(start),
			coords[last],
			d(last),
		));
	}
	out.push(d(start));
	for &end in &indices[1..] {
		if end - start > 1 {
			out.extend(iup_segment(
				&coords[start + 1..end],
				coords[start],
				d(start),
				coords[end],
				d(end),
			));
		}
		out.push(d(end));
		start = end;
	}
	if start != n - 1 {
		out.extend(iup_segment(
			&coords[start + 1..],
			coords[start],
			d(start),
			coords[first],
			d(first),
		));
	}
	out
}

/// `iup_delta`: fills in the missing deltas of an outline, whose contours end at
/// `ends`, and whose last four points are the phantom points.
pub fn iup_delta(deltas: &[Option<Point>], coords: &[Point], ends: &[usize]) -> Result<Vec<Point>> {
	let expected = ends.last().map_or(0, |e| e + 1) + 4;
	if !ends.is_sorted() || coords.len() != expected || deltas.len() != coords.len() {
		return Err(Error::Assertion(
			"sorted(ends) == ends and len(coords) == (ends[-1] + 1 if ends else 0) + 4".into(),
		));
	}
	let n = coords.len();
	let mut out = Vec::with_capacity(n);
	let mut start = 0;
	for end in ends.iter().copied().chain([n - 4, n - 3, n - 2, n - 1]) {
		let end = (end + 1).clamp(start, n);
		out.extend(iup_contour(&deltas[start..end], &coords[start..end]));
		start = end;
	}
	Ok(out)
}

fn index_error() -> Error {
	Error::ttlib("IndexError: list index out of range")
}

fn none_error(what: &str) -> Error {
	Error::ttlib(format!(
		"AttributeError: 'NoneType' object has no attribute '{what}'"
	))
}

/// The support of a region, by axis tag, in the order of the region's axes
/// (`get_support`); a later entry for the same axis replaces the earlier value.
fn insert_support(support: &mut Vec<(String, Triple)>, tag: &str, triple: Triple) {
	match support.iter_mut().find(|(t, _)| t == tag) {
		Some(entry) => entry.1 = triple,
		None => support.push((tag.to_owned(), triple)),
	}
}

/// `VarStoreInstancer`: the deltas of an item variation store at a location.
#[derive(Debug)]
pub struct VarStoreInstancer<'a> {
	store: Option<&'a ot::VarStore>,
	axes: &'a [String],
	location: Location,
	scalars: RefCell<HashMap<usize, f64>>,
}

impl<'a> VarStoreInstancer<'a> {
	/// An instancer for `store` (with the font's axis tags) at `location`.
	pub fn new(store: Option<&'a ot::VarStore>, axes: &'a [String], location: Location) -> Self {
		VarStoreInstancer {
			store,
			axes,
			location,
			scalars: RefCell::new(HashMap::new()),
		}
	}

	fn var_data(&self, major: usize) -> Result<&'a ot::VarData> {
		let store = self.store.ok_or_else(index_error)?;
		store
			.var_data
			.get(major)
			.ok_or_else(index_error)?
			.as_deref()
			.ok_or_else(|| none_error("VarRegionIndex"))
	}

	fn scalar(&self, region: usize) -> Result<f64> {
		if let Some(&s) = self.scalars.borrow().get(&region) {
			return Ok(s);
		}
		let regions = self
			.store
			.and_then(|s| s.var_region_list.as_deref())
			.ok_or_else(|| none_error("Region"))?;
		let region_axes = &regions
			.region
			.get(region)
			.ok_or_else(index_error)?
			.var_region_axis;
		let mut support = Vec::new();
		for (i, r) in region_axes.iter().enumerate() {
			if r.peak_coord != 0.0 {
				let tag = self.axes.get(i).ok_or_else(index_error)?;
				insert_support(
					&mut support,
					tag,
					(r.start_coord, r.peak_coord, r.end_coord),
				);
			}
		}
		let s = support_scalar(
			&self.location,
			support.iter().map(|(t, r)| (t.as_str(), *r)),
		);
		self.scalars.borrow_mut().insert(region, s);
		Ok(s)
	}

	fn interpolate(&self, var_data: &ot::VarData, deltas: &[f64]) -> Result<f64> {
		let mut scalars = Vec::with_capacity(var_data.var_region_index.len());
		for &region in &var_data.var_region_index {
			scalars.push(self.scalar(region as usize)?);
		}
		let mut delta = 0.0;
		for (&d, &s) in deltas.iter().zip(&scalars) {
			if s == 0.0 {
				continue;
			}
			delta += d * s;
		}
		Ok(delta)
	}

	/// The delta of a variation index (`instancer[varidx]`); 0 for
	/// `NO_VARIATION_INDEX` and for indices beyond the store.
	pub fn get(&self, var_idx: u64) -> Result<f64> {
		if var_idx == NO_VARIATION_INDEX {
			return Ok(0.0);
		}
		let (major, minor) = ((var_idx >> 16) as usize, (var_idx & 0xFFFF) as usize);
		let count = self.store.map_or(0, |s| s.var_data.len());
		if major >= count {
			return Ok(0.0);
		}
		let var_data = self.var_data(major)?;
		let Some(item) = var_data.item.get(minor) else {
			return Ok(0.0);
		};
		let deltas: Vec<f64> = item.iter().map(|&d| d as f64).collect();
		self.interpolate(var_data, &deltas)
	}

	/// `interpolateFromDeltas(varDataIndex, deltas)`.
	pub fn interpolate_from_deltas(&self, var_data_index: i64, deltas: &[f64]) -> Result<f64> {
		let major = usize::try_from(var_data_index).map_err(|_| index_error())?;
		let var_data = self.var_data(major)?;
		self.interpolate(var_data, deltas)
	}
}

/// `MultiVarStoreInstancer`: the delta tuples of a multiple item variation
/// store at a location.
#[derive(Debug)]
pub struct MultiVarStoreInstancer<'a> {
	store: Option<&'a ot::MultiVarStore>,
	axes: &'a [String],
	location: Location,
	scalars: RefCell<HashMap<usize, f64>>,
}

impl<'a> MultiVarStoreInstancer<'a> {
	/// An instancer for `store` (with the font's axis tags) at `location`.
	pub fn new(
		store: Option<&'a ot::MultiVarStore>,
		axes: &'a [String],
		location: Location,
	) -> Self {
		MultiVarStoreInstancer {
			store,
			axes,
			location,
			scalars: RefCell::new(HashMap::new()),
		}
	}

	fn scalar(&self, region: usize) -> Result<f64> {
		if let Some(&s) = self.scalars.borrow().get(&region) {
			return Ok(s);
		}
		let regions = self
			.store
			.and_then(|s| s.sparse_var_region_list.as_deref())
			.ok_or_else(|| none_error("Region"))?;
		let sparse = regions
			.region
			.get(region)
			.ok_or_else(index_error)?
			.as_deref()
			.ok_or_else(|| none_error("SparseVarRegionAxis"))?;
		let mut support = Vec::new();
		for r in &sparse.sparse_var_region_axis {
			let r = r.as_deref().ok_or_else(|| none_error("AxisIndex"))?;
			let tag = self
				.axes
				.get(r.axis_index as usize)
				.ok_or_else(index_error)?;
			insert_support(
				&mut support,
				tag,
				(r.start_coord, r.peak_coord, r.end_coord),
			);
		}
		let s = support_scalar(
			&self.location,
			support.iter().map(|(t, r)| (t.as_str(), *r)),
		);
		self.scalars.borrow_mut().insert(region, s);
		Ok(s)
	}

	/// The delta tuple of a variation index (`instancer[varidx]`); empty for
	/// `NO_VARIATION_INDEX`.
	pub fn get(&self, var_idx: u64) -> Result<Vec<f64>> {
		if var_idx == NO_VARIATION_INDEX {
			return Ok(Vec::new());
		}
		let (major, minor) = ((var_idx >> 16) as usize, (var_idx & 0xFFFF) as usize);
		let var_data = self
			.store
			.ok_or_else(index_error)?
			.multi_var_data
			.get(major)
			.ok_or_else(index_error)?
			.as_deref()
			.ok_or_else(|| none_error("VarRegionIndex"))?;
		let mut scalars = Vec::with_capacity(var_data.var_region_index.len());
		for &r in &var_data.var_region_index {
			scalars.push(self.scalar(r as usize)?);
		}
		let deltas = var_data.item.get(minor).ok_or_else(index_error)?;
		interpolate_tuples(deltas, &scalars)
	}
}

/// `interpolateFromDeltasAndScalars` of `MultiVarStoreInstancer`.
fn interpolate_tuples(deltas: &[i64], scalars: &[f64]) -> Result<Vec<f64>> {
	if deltas.is_empty() {
		return Ok(Vec::new());
	}
	if scalars.is_empty() {
		return Err(Error::ttlib("ZeroDivisionError: integer modulo by zero"));
	}
	if !deltas.len().is_multiple_of(scalars.len()) {
		return Err(Error::Assertion(format!(
			"({}, {})",
			deltas.len(),
			scalars.len()
		)));
	}
	let m = deltas.len() / scalars.len();
	let mut delta = vec![0.0; m];
	for (chunk, &s) in deltas.chunks(m).zip(scalars) {
		if s == 0.0 {
			continue;
		}
		for (out, &d) in delta.iter_mut().zip(chunk) {
			*out += d as f64 * s;
		}
	}
	Ok(delta)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn loc(items: &[(&str, f64)]) -> Location {
		items.iter().map(|&(k, v)| (k.to_owned(), v)).collect()
	}

	// Cases from fontTools' doctests of supportScalar.
	#[test]
	fn scalar() {
		assert_eq!(support_scalar(&loc(&[]), []), 1.0);
		assert_eq!(support_scalar(&loc(&[("wght", 0.2)]), []), 1.0);
		let s = support_scalar(&loc(&[("wght", 0.2)]), [("wght", (0.0, 2.0, 3.0))]);
		assert_eq!(s, 0.1);
		let s = support_scalar(&loc(&[("wght", 2.5)]), [("wght", (0.0, 2.0, 4.0))]);
		assert_eq!(s, 0.75);
		let s = support_scalar(
			&loc(&[("wght", 2.5), ("wdth", 0.5)]),
			[("wght", (0.0, 2.0, 4.0)), ("wdth", (-1.0, 0.0, 1.0))],
		);
		assert_eq!(s, 0.75);
	}

	// Cases from fontTools' Tests/varLib/iup_test.py.
	#[test]
	fn iup() -> Result<()> {
		let coords = [
			(1.0, 1.0),
			(2.0, 2.0),
			(3.0, 3.0),
			(0.0, 0.0),
			(0.0, 0.0),
			(0.0, 0.0),
			(0.0, 0.0),
		];
		let deltas = [
			Some((1.0, 1.0)),
			None,
			Some((3.0, 3.0)),
			Some((0.0, 0.0)),
			Some((0.0, 0.0)),
			Some((0.0, 0.0)),
			Some((0.0, 0.0)),
		];
		assert_eq!(
			iup_delta(&deltas, &coords, &[2])?,
			[
				(1.0, 1.0),
				(2.0, 2.0),
				(3.0, 3.0),
				(0.0, 0.0),
				(0.0, 0.0),
				(0.0, 0.0),
				(0.0, 0.0)
			]
		);
		let deltas = [None, None, None, None, None, None, None];
		assert_eq!(iup_delta(&deltas, &coords, &[2])?, [(0.0, 0.0); 7]);
		Ok(())
	}
}
