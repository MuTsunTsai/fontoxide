//! Coverage and ClassDef tables in the graph (`graph/coverage-graph.hh`,
//! `graph/classdef-graph.hh`), with the parts of `Coverage::serialize`,
//! `ClassDef::serialize` and their iterators that these use.
//!
//! Only the 16-bit formats (1 and 2) are handled: fontTools does not write the
//! 24-bit ones.

use std::collections::{BTreeSet, HashMap};

use crate::graph::Graph;
use crate::{Error, Result};

pub(crate) fn get16(data: &[u8], pos: usize) -> u16 {
	data.get(pos..pos + 2)
		.map_or(0, |b| u16::from_be_bytes([b[0], b[1]]))
}

pub(crate) fn set16(data: &mut [u8], pos: usize, v: u16) {
	if let Some(b) = data.get_mut(pos..pos + 2) {
		b.copy_from_slice(&v.to_be_bytes());
	}
}

pub(crate) fn get32(data: &[u8], pos: usize) -> u32 {
	data.get(pos..pos + 4)
		.map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// `as_table`: the object linked at `position` of `parent`, checked with
/// `sanitize`.
pub(crate) fn as_table(
	g: &Graph,
	parent: u32,
	position: u32,
	sanitize: impl Fn(&[u8]) -> Result<()>,
) -> Result<u32> {
	let idx = g.index_for_offset(parent, position)?;
	as_table_from_index(g, idx, sanitize)
}

/// `as_mutable_table`: like [`as_table`], but the object is duplicated first if
/// it has other parents.
pub(crate) fn as_mutable_table(
	g: &mut Graph,
	parent: u32,
	position: u32,
	sanitize: impl Fn(&[u8]) -> Result<()>,
) -> Result<u32> {
	let idx = g.mutable_index_for_offset(parent, position)?;
	as_table_from_index(g, idx, sanitize)
}

fn as_table_from_index(g: &Graph, idx: u32, sanitize: impl Fn(&[u8]) -> Result<()>) -> Result<u32> {
	if idx >= g.len() {
		return Err(Error::OutOfBounds);
	}
	sanitize(&g.v(idx).data)?;
	Ok(idx)
}

/// `Coverage::sanitize` (graph version: only checks the sizes).
pub(crate) fn coverage_sanitize(data: &[u8]) -> Result<()> {
	let len = data.len();
	if len < 2 {
		return Err(Error::SanitizeFailure);
	}
	let count = get16(data, 2) as usize;
	let needed = match get16(data, 0) {
		1 => 4 + 2 * count,
		2 => 4 + 6 * count,
		_ => return Err(Error::SanitizeFailure),
	};
	if len < 4 || len < needed {
		return Err(Error::SanitizeFailure);
	}
	Ok(())
}

/// The glyphs of a coverage table, in coverage index order (`Coverage::iter`).
pub(crate) fn coverage_glyphs(data: &[u8]) -> Vec<u32> {
	let count = get16(data, 2) as usize;
	match get16(data, 0) {
		1 => (0..count)
			.map(|i| u32::from(get16(data, 4 + 2 * i)))
			.collect(),
		2 => {
			// `CoverageFormat2_4::iter_t`, which stops at broken ranges.
			let range = |i: usize| {
				let p = 4 + 6 * i;
				(
					u32::from(get16(data, p)),
					u32::from(get16(data, p + 2)),
					u32::from(get16(data, p + 4)),
				)
			};
			let mut out = Vec::new();
			if count == 0 {
				return out;
			}
			let (first, last, _) = range(0);
			if first > last {
				return out;
			}
			let mut i = 0;
			let mut j = first;
			let mut coverage = 0u32;
			while i < count {
				out.push(j);
				if j >= range(i).1 {
					i += 1;
					if i < count {
						let old = coverage;
						let (first, _, value) = range(i);
						j = first;
						coverage = value;
						if coverage != old.wrapping_add(1) {
							break;
						}
					}
					continue;
				}
				coverage = coverage.wrapping_add(1);
				j += 1;
			}
			out
		}
		_ => Vec::new(),
	}
}

/// `Coverage_serialize`.
fn coverage_serialize(glyphs: &[u32]) -> Vec<u8> {
	const NONE: u32 = u32::MAX - 1;
	let count = glyphs.len();
	let mut num_ranges = 0;
	let mut last = NONE;
	let mut unsorted = false;
	for &g in glyphs {
		if last != NONE && g < last {
			unsorted = true;
		}
		if last.wrapping_add(1) != g {
			num_ranges += 1;
		}
		last = g;
	}
	let format = if !unsorted && count <= num_ranges * 3 {
		1
	} else {
		2
	};
	let mut out = Vec::new();
	out.extend_from_slice(&(format as u16).to_be_bytes());
	if format == 1 {
		out.extend_from_slice(&(count as u16).to_be_bytes());
		for &g in glyphs {
			out.extend_from_slice(&(g as u16).to_be_bytes());
		}
		return out;
	}
	let mut ranges: Vec<[u16; 3]> = Vec::with_capacity(num_ranges);
	let mut last = NONE;
	let mut unsorted = false;
	for (index, &g) in glyphs.iter().enumerate() {
		if last.wrapping_add(1) != g {
			if last != NONE && last.wrapping_add(1) > g {
				unsorted = true;
			}
			ranges.push([g as u16, g as u16, index as u16]);
		}
		if let Some(r) = ranges.last_mut() {
			r[1] = g as u16;
		}
		last = g;
	}
	if unsorted {
		ranges.sort_by_key(|r| r[0]);
	}
	out.extend_from_slice(&(ranges.len() as u16).to_be_bytes());
	for r in ranges {
		for v in r {
			out.extend_from_slice(&v.to_be_bytes());
		}
	}
	out
}

/// `Coverage::make_coverage`: replaces the data of `dest_obj` with a coverage
/// of `glyphs`, failing if it is larger than `max_size`.
pub(crate) fn make_coverage(
	g: &mut Graph,
	glyphs: &[u32],
	dest_obj: u32,
	max_size: usize,
) -> Result<()> {
	let data = coverage_serialize(glyphs);
	if data.len() > max_size {
		return Err(Error::AllocationFailure);
	}
	g.vm(dest_obj).data = data;
	Ok(())
}

/// `Coverage::add_coverage`: a new coverage of `glyphs`, linked from
/// `parent_id` at `link_position`.
pub(crate) fn add_coverage(
	g: &mut Graph,
	parent_id: u32,
	link_position: u32,
	glyphs: &[u32],
	max_size: usize,
) -> Result<()> {
	let coverage_prime_id = g.new_node(Vec::new())?;
	make_coverage(g, glyphs, coverage_prime_id, max_size)?;
	g.vm(parent_id)
		.add_real_link(2, coverage_prime_id, link_position);
	g.vm(coverage_prime_id).add_parent(parent_id, false)
}

/// The glyphs of a coverage whose coverage index is in `[start, end)`.
fn glyphs_in_range(data: &[u8], start: u32, end: u32) -> Vec<u32> {
	coverage_glyphs(data)
		.into_iter()
		.enumerate()
		.filter(|&(i, _)| (start..end).contains(&(i as u32)))
		.map(|(_, g)| g)
		.collect()
}

/// `Coverage::clone_coverage`: a copy of the coverage `coverage_id` with the
/// glyphs at indices `[start, end)`, linked from `new_parent_id`.
pub(crate) fn clone_coverage(
	g: &mut Graph,
	coverage_id: u32,
	new_parent_id: u32,
	link_position: u32,
	start: u32,
	end: u32,
) -> Result<()> {
	let data = &g.v(coverage_id).data;
	let coverage_size = data.len();
	coverage_sanitize(data)?;
	let glyphs = glyphs_in_range(data, start, end);
	add_coverage(g, new_parent_id, link_position, &glyphs, coverage_size)
}

/// `Coverage::filter_coverage`: keeps the glyphs at indices `[start, end)`.
pub(crate) fn filter_coverage(
	g: &mut Graph,
	existing_coverage: u32,
	start: u32,
	end: u32,
) -> Result<()> {
	let data = &g.v(existing_coverage).data;
	let coverage_size = data.len();
	coverage_sanitize(data)?;
	let glyphs = glyphs_in_range(data, start, end);
	make_coverage(g, &glyphs, existing_coverage, coverage_size * 2 + 100)
}

/// `ClassDef::sanitize` (graph version).
pub(crate) fn class_def_sanitize(data: &[u8]) -> Result<()> {
	let len = data.len();
	if len < 2 {
		return Err(Error::SanitizeFailure);
	}
	let (min_size, needed) = match get16(data, 0) {
		1 => (6, 6 + 2 * get16(data, 4) as usize),
		2 => (4, 4 + 6 * get16(data, 2) as usize),
		_ => return Err(Error::SanitizeFailure),
	};
	if len < min_size || len < needed {
		return Err(Error::SanitizeFailure);
	}
	Ok(())
}

/// `ClassDef::get_class`.
pub(crate) fn class_def_get_class(data: &[u8], glyph: u32) -> u32 {
	match get16(data, 0) {
		1 => {
			let start = u32::from(get16(data, 2));
			let count = u32::from(get16(data, 4));
			let i = glyph.wrapping_sub(start);
			if i < count {
				u32::from(get16(data, 6 + 2 * i as usize))
			} else {
				0
			}
		}
		2 => {
			// `hb_bsearch` over the range records.
			let count = i64::from(get16(data, 2));
			let (mut min, mut max) = (0i64, count - 1);
			while min <= max {
				let mid = ((min + max) / 2) as usize;
				let p = 4 + 6 * mid;
				let first = u32::from(get16(data, p));
				let last = u32::from(get16(data, p + 2));
				if glyph < first {
					max = mid as i64 - 1;
				} else if glyph > last {
					min = mid as i64 + 1;
				} else {
					return u32::from(get16(data, p + 4));
				}
			}
			0
		}
		_ => 0,
	}
}

/// `ClassDef_serialize`: from `(glyph, class)` pairs sorted by glyph; class 0 is
/// left out.
fn class_def_serialize(pairs: &[(u32, u32)]) -> Result<Vec<u8>> {
	let glyphs: Vec<(u32, u32)> = pairs.iter().copied().filter(|p| p.1 != 0).collect();
	let mut format = 2;
	if let Some(&(glyph_min, first_class)) = glyphs.first() {
		let mut glyph_max = glyph_min;
		let mut class_max = 0;
		let mut num_ranges = 1u64;
		let mut prev_gid = glyph_min;
		let mut prev_klass = first_class;
		for &(cur_gid, cur_klass) in &glyphs {
			class_max = class_max.max(cur_klass);
			if cur_gid == glyph_min {
				continue;
			}
			glyph_max = glyph_max.max(cur_gid);
			if cur_gid != prev_gid.wrapping_add(1) || cur_klass != prev_klass {
				num_ranges += 1;
			}
			prev_gid = cur_gid;
			prev_klass = cur_klass;
		}
		let mut best_size = u64::MAX;
		let glyph_span = u64::from(glyph_max.wrapping_sub(glyph_min).wrapping_add(1));
		let mut consider = |candidate, size: u64| {
			if size < best_size {
				best_size = size;
				format = candidate;
			}
		};
		if glyph_max <= 0xFFFF && glyph_span <= 0xFFFF && class_max <= 0xFFFF {
			consider(1, 6 + 2 * glyph_span);
		}
		if glyph_max <= 0xFFFF && num_ranges <= 0xFFFF && class_max <= 0xFFFF {
			consider(2, 4 + num_ranges * 6);
		}
		if glyph_max <= 0xFF_FFFF && glyph_span <= 0xFF_FFFF && class_max <= 0xFF_FFFF {
			consider(3, 8 + 3 * glyph_span);
		}
		if glyph_max <= 0xFF_FFFF && num_ranges <= 0xFF_FFFF && class_max <= 0xFFFF {
			consider(4, 5 + num_ranges * 8);
		}
		if best_size == u64::MAX {
			return Err(Error::AllocationFailure);
		}
	}
	let mut out = Vec::new();
	match format {
		1 => {
			out.extend_from_slice(&1u16.to_be_bytes());
			let Some(&(glyph_min, _)) = glyphs.first() else {
				out.extend_from_slice(&[0, 0, 0, 0]);
				return Ok(out);
			};
			// Glyphs are sorted, as they come from coverage tables; HarfBuzz uses the
			// first glyph as the start.
			let glyph_max = glyphs.iter().map(|p| p.0).fold(0, u32::max);
			let glyph_count = glyph_max.wrapping_sub(glyph_min).wrapping_add(1) as usize;
			let mut values = vec![0u16; glyph_count];
			for &(gid, klass) in &glyphs {
				if let Some(v) = values.get_mut(gid.wrapping_sub(glyph_min) as usize) {
					*v = klass as u16;
				}
			}
			out.extend_from_slice(&(glyph_min as u16).to_be_bytes());
			out.extend_from_slice(&(glyph_count as u16).to_be_bytes());
			for v in values {
				out.extend_from_slice(&v.to_be_bytes());
			}
		}
		2 => {
			out.extend_from_slice(&2u16.to_be_bytes());
			let mut ranges: Vec<[u32; 3]> = Vec::new();
			let mut unsorted = false;
			let mut prev: Option<(u32, u32)> = None;
			for &(cur_gid, cur_klass) in &glyphs {
				match prev {
					Some((prev_gid, prev_klass))
						if cur_gid == prev_gid.wrapping_add(1) && cur_klass == prev_klass => {}
					Some((prev_gid, _)) => {
						if cur_gid < prev_gid {
							unsorted = true;
						}
						if let Some(r) = ranges.last_mut() {
							r[1] = prev_gid;
						}
						ranges.push([cur_gid, cur_gid, cur_klass]);
					}
					None => ranges.push([cur_gid, cur_gid, cur_klass]),
				}
				prev = Some((cur_gid, cur_klass));
			}
			if let (Some(r), Some((prev_gid, _))) = (ranges.last_mut(), prev) {
				r[1] = prev_gid;
			}
			if unsorted {
				ranges.sort_by_key(|r| r[0]);
			}
			out.extend_from_slice(&(ranges.len() as u16).to_be_bytes());
			for r in ranges {
				for v in r {
					out.extend_from_slice(&(v as u16).to_be_bytes());
				}
			}
		}
		// Glyphs above 0xFFFF do not occur in the 16-bit tables fontTools writes.
		_ => return Err(Error::AllocationFailure),
	}
	Ok(out)
}

/// `ClassDef::make_class_def`.
pub(crate) fn make_class_def(
	g: &mut Graph,
	pairs: &[(u32, u32)],
	dest_obj: u32,
	max_size: usize,
) -> Result<()> {
	let data = class_def_serialize(pairs)?;
	if data.len() > max_size {
		return Err(Error::AllocationFailure);
	}
	g.vm(dest_obj).data = data;
	Ok(())
}

/// `ClassDef::add_class_def`.
pub(crate) fn add_class_def(
	g: &mut Graph,
	parent_id: u32,
	link_position: u32,
	pairs: &[(u32, u32)],
	max_size: usize,
) -> Result<()> {
	let class_def_prime_id = g.new_node(Vec::new())?;
	make_class_def(g, pairs, class_def_prime_id, max_size)?;
	g.vm(parent_id)
		.add_real_link(2, class_def_prime_id, link_position);
	g.vm(class_def_prime_id).add_parent(parent_id, false)
}

/// The number of runs of consecutive values (`hb_set_t::next_range`).
fn num_glyph_ranges(glyphs: &BTreeSet<u32>) -> u32 {
	let mut count = 0;
	let mut prev: Option<u32> = None;
	for &g in glyphs {
		if prev.is_none_or(|p| p.wrapping_add(1) != g) {
			count += 1;
		}
		prev = Some(g);
	}
	count
}

/// `class_def_size_estimator_t`: estimates the sizes of the coverage and class
/// definition of the first classes of a `PairPosFormat2`.
#[derive(Debug)]
pub(crate) struct ClassDefSizeEstimator {
	num_ranges_per_class: HashMap<u32, u32>,
	glyphs_per_class: HashMap<u32, BTreeSet<u32>>,
	included_classes: BTreeSet<u32>,
	included_glyphs: BTreeSet<u32>,
	included_class_def_glyphs: BTreeSet<u32>,
	class_def_range_count: u32,
	class_max: u32,
	coverage_size_val: u32,
}

impl ClassDefSizeEstimator {
	pub fn new(glyph_and_class: &[(u32, u32)]) -> Self {
		let mut e = ClassDefSizeEstimator {
			num_ranges_per_class: HashMap::new(),
			glyphs_per_class: HashMap::new(),
			included_classes: BTreeSet::new(),
			included_glyphs: BTreeSet::new(),
			included_class_def_glyphs: BTreeSet::new(),
			class_def_range_count: 0,
			class_max: 0,
			coverage_size_val: 0,
		};
		e.reset();
		for &(gid, klass) in glyph_and_class {
			e.glyphs_per_class.entry(klass).or_default().insert(gid);
		}
		for (&klass, glyphs) in &e.glyphs_per_class {
			if klass == 0 {
				// Class 0 is not encoded.
				continue;
			}
			e.num_ranges_per_class
				.insert(klass, num_glyph_ranges(glyphs));
		}
		e
	}

	pub fn reset(&mut self) {
		self.included_glyphs.clear();
		self.included_class_def_glyphs.clear();
		self.included_classes.clear();
		self.class_def_range_count = 0;
		self.class_max = 0;
		self.coverage_size_val = self.compute_coverage_size();
	}

	/// The size of the coverage of all glyphs added by `add_class_def_size`.
	pub fn coverage_size(&self) -> u32 {
		self.coverage_size_val
	}

	/// The size of the class definition with all glyphs of `klass` added.
	pub fn add_class_def_size(&mut self, klass: u32) -> u32 {
		if !self.included_classes.contains(&klass) {
			let glyphs = self.glyphs_per_class.get(&klass);
			if let Some(glyphs) = glyphs {
				let num_glyphs = self.included_glyphs.len();
				self.included_glyphs.extend(glyphs);
				if klass != 0 {
					self.included_class_def_glyphs.extend(glyphs);
				}
				if num_glyphs != self.included_glyphs.len() {
					self.coverage_size_val = self.compute_coverage_size();
				}
			}
			if klass != 0 {
				self.class_def_range_count +=
					self.num_ranges_per_class.get(&klass).copied().unwrap_or(0);
				if self.glyphs_per_class.contains_key(&klass) && klass > self.class_max {
					self.class_max = klass;
				}
			}
			self.included_classes.insert(klass);
		}
		self.class_def_size()
	}

	fn compute_coverage_size(&self) -> u32 {
		let glyph_size = Self::glyph_id_size(&self.included_glyphs);
		let coverage_base_size = 2 + glyph_size;
		let range_size = 3 * glyph_size;
		let format1_size = coverage_base_size + glyph_size * self.included_glyphs.len() as u32;
		let format2_size =
			coverage_base_size + range_size * num_glyph_ranges(&self.included_glyphs);
		format1_size.min(format2_size)
	}

	fn glyph_id_size(glyphs: &BTreeSet<u32>) -> u32 {
		if glyphs.last().is_some_and(|&g| g > 0xFFFF) {
			3
		} else {
			2
		}
	}

	fn class_def_size(&self) -> u32 {
		let (Some(&glyph_min), Some(&glyph_max)) = (
			self.included_class_def_glyphs.first(),
			self.included_class_def_glyphs.last(),
		) else {
			return 4;
		};
		let mut best_size = u32::MAX;
		let glyph_span = glyph_max - glyph_min + 1;
		let range_count = self.class_def_range_count;
		let class_max = self.class_max;
		if glyph_max <= 0xFFFF && glyph_span <= 0xFFFF && class_max <= 0xFFFF {
			best_size = best_size.min(6 + 2 * glyph_span);
		}
		if glyph_max <= 0xFFFF && range_count <= 0xFFFF && class_max <= 0xFFFF {
			best_size = best_size.min(4 + 6 * range_count);
		}
		if glyph_max <= 0xFF_FFFF && glyph_span <= 0xFF_FFFF && class_max <= 0xFF_FFFF {
			best_size = best_size.min(8 + 3 * glyph_span);
		}
		if glyph_max <= 0xFF_FFFF && range_count <= 0xFF_FFFF && class_max <= 0xFFFF {
			best_size = best_size.min(5 + 8 * range_count);
		}
		best_size
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn coverage_round_trip() {
		let glyphs = [1, 2, 3, 10, 20];
		let data = coverage_serialize(&glyphs);
		assert_eq!(get16(&data, 0), 1);
		assert_eq!(coverage_glyphs(&data), glyphs);
		let glyphs: Vec<u32> = (5..40).collect();
		let data = coverage_serialize(&glyphs);
		assert_eq!(data, [0, 2, 0, 1, 0, 5, 0, 39, 0, 0]);
		assert_eq!(coverage_glyphs(&data), glyphs);
	}

	#[test]
	fn class_def() -> Result<()> {
		let pairs = [(3, 1), (4, 1), (5, 2), (6, 0), (9, 1)];
		let data = class_def_serialize(&pairs)?;
		for (g, k) in pairs {
			assert_eq!(class_def_get_class(&data, g), k);
		}
		assert_eq!(class_def_get_class(&data, 100), 0);
		let pairs: Vec<(u32, u32)> = (0..100).map(|g| (g * 3, 1)).collect();
		let data = class_def_serialize(&pairs)?;
		assert_eq!(get16(&data, 0), 1);
		let data = class_def_serialize(&[(10, 1), (11, 1), (1000, 2)])?;
		assert_eq!(get16(&data, 0), 2);
		assert_eq!(class_def_get_class(&data, 11), 1);
		assert_eq!(class_def_get_class(&data, 1000), 2);
		Ok(())
	}
}
