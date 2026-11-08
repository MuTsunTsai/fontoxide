//! Splitting of `PairPos` subtables (`graph/pairpos-graph.hh`).

use std::collections::{BTreeSet, HashMap};

use crate::graph::Graph;
use crate::gsubgpos::{Context, SplitContext, actuate_subtable_split};
use crate::layout::{
	ClassDefSizeEstimator, add_class_def, add_coverage, as_mutable_table, class_def_get_class,
	class_def_sanitize, clone_coverage, coverage_glyphs, coverage_sanitize, get16, make_class_def,
	make_coverage, set16,
};
use crate::{Error, Result};

/// `PairPosFormat1_3<SmallTypes>::min_size`.
const FORMAT1_MIN_SIZE: u32 = 10;
/// `PairPosFormat2_4<SmallTypes>::min_size`.
const FORMAT2_MIN_SIZE: u32 = 16;

/// `split_subtable<PairPos>`: splits a subtable if needed; nothing is split if
/// it does not sanitize.
pub(crate) fn split_subtable(c: &mut Context, g: &mut Graph, objidx: u32) -> Result<Vec<u32>> {
	if sanitize(&g.v(objidx).data).is_err() {
		return Ok(Vec::new());
	}
	match get16(&g.v(objidx).data, 0) {
		1 => split_format1(c, g, objidx),
		2 => split_format2(c, g, objidx),
		_ => Ok(Vec::new()),
	}
}

/// `PairPos::sanitize`.
fn sanitize(data: &[u8]) -> Result<()> {
	let len = data.len();
	if len < 2 {
		return Err(Error::SanitizeFailure);
	}
	let needed = match get16(data, 0) {
		1 => {
			if len < FORMAT1_MIN_SIZE as usize {
				return Err(Error::SanitizeFailure);
			}
			FORMAT1_MIN_SIZE as usize + 2 * get16(data, 8) as usize
		}
		2 => {
			if len < FORMAT2_MIN_SIZE as usize {
				return Err(Error::SanitizeFailure);
			}
			FORMAT2_MIN_SIZE as usize + get16(data, 12) as usize * class1_record_size(data) as usize
		}
		_ => return Err(Error::SanitizeFailure),
	};
	if len < needed {
		return Err(Error::SanitizeFailure);
	}
	Ok(())
}

/// `ValueFormat::get_len`: the number of values of a value record.
fn value_len(format: u16) -> u32 {
	(format & 0xFF).count_ones()
}

/// `ValueFormat::get_device_table_indices`: the indices of the device offsets
/// in a value record.
fn device_table_indices(format: u16) -> Vec<u32> {
	let mut i = (format & 0x0F).count_ones();
	let mut result = Vec::new();
	for bit in [0x10, 0x20, 0x40, 0x80] {
		if format & bit != 0 {
			result.push(i);
			i += 1;
		}
	}
	result
}

/// `PairPosFormat1::split_subtables`.
fn split_format1(c: &mut Context, g: &mut Graph, this_index: u32) -> Result<Vec<u32>> {
	let mut visited = BTreeSet::new();
	let coverage_id = g.index_for_offset(this_index, 2)?;
	let coverage_size = g.v(coverage_id).table_size() as u32;
	let base_size = FORMAT1_MIN_SIZE;
	let mut partial_coverage_size = 4;
	let mut accumulated = base_size;
	let mut split_points = Vec::new();
	let pair_set_count = u32::from(get16(&g.v(this_index).data, 8));
	for i in 0..pair_set_count {
		let pair_set_index = g.index_for_offset(this_index, FORMAT1_MIN_SIZE + 2 * i)?;
		// Plus the PairSet offset.
		let accumulated_delta = g
			.find_subgraph_size(pair_set_index, &mut visited, u32::MAX)
			.unwrap_or(0) as u32
			+ 2;
		partial_coverage_size += 2;
		accumulated += accumulated_delta;
		let total = accumulated + partial_coverage_size.min(coverage_size);
		if total >= 1 << 16 {
			split_points.push(i);
			accumulated = base_size + accumulated_delta;
			partial_coverage_size = 6;
			// Nodes are not shared between splits.
			visited.clear();
		}
	}
	let mut sc = Format1Split { this_index };
	actuate_subtable_split(&mut sc, c, g, &split_points)
}

struct Format1Split {
	this_index: u32,
}

impl SplitContext for Format1Split {
	fn original_count(&self, g: &Graph) -> u32 {
		u32::from(get16(&g.v(self.this_index).data, 8))
	}

	/// `PairPosFormat1::clone_range`: a new PairPos with the PairSets of
	/// `[start, end)`.
	fn clone_range(&mut self, c: &mut Context, g: &mut Graph, start: u32, end: u32) -> Result<u32> {
		let this_index = self.this_index;
		let num_pair_sets = end - start;
		let prime_size = FORMAT1_MIN_SIZE + num_pair_sets * 2;
		let prime_id = c.create_node(g, prime_size as usize)?;
		let this = g.v(this_index).data.clone();
		let prime = &mut g.vm(prime_id).data;
		set16(prime, 0, get16(&this, 0));
		set16(prime, 4, get16(&this, 4));
		set16(prime, 6, get16(&this, 6));
		set16(prime, 8, num_pair_sets as u16);
		for i in start..end {
			g.move_child(
				this_index,
				FORMAT1_MIN_SIZE + 2 * i,
				prime_id,
				FORMAT1_MIN_SIZE + 2 * (i - start),
			)?;
		}
		// Restore the order of the links, disturbed by `move_child`.
		g.vm(this_index).sort_real_links();
		let coverage_id = g.index_for_offset(this_index, 2)?;
		clone_coverage(g, coverage_id, prime_id, 2, start, end)?;
		Ok(prime_id)
	}

	/// `PairPosFormat1::shrink`: keeps the PairSets of `[0, count)`.
	fn shrink(&mut self, _c: &mut Context, g: &mut Graph, count: u32) -> Result<()> {
		let this_index = self.this_index;
		let old_count = u32::from(get16(&g.v(this_index).data, 8));
		if count >= old_count {
			return Ok(());
		}
		set16(&mut g.vm(this_index).data, 8, count as u16);
		g.vm(this_index)
			.shrink_buffer(((old_count - count) * 2) as usize);
		let coverage = as_mutable_table(g, this_index, 2, coverage_sanitize)?;
		let coverage_size = g.v(coverage).table_size();
		let glyphs: Vec<u32> = coverage_glyphs(&g.v(coverage).data)
			.into_iter()
			.take(count as usize)
			.collect();
		make_coverage(g, &glyphs, coverage, coverage_size)
	}
}

/// `PairPosFormat2::get_class1_record_size`.
fn class1_record_size(data: &[u8]) -> u32 {
	let class2_count = u32::from(get16(data, 14));
	class2_count * 2 * (value_len(get16(data, 4)) + value_len(get16(data, 6)))
}

/// The `(glyph, class)` pairs of the glyphs of the coverage, with their class
/// in class definition 1.
fn glyphs_and_classes(g: &Graph, coverage: u32, class_def_1: u32) -> Vec<(u32, u32)> {
	let class_def = &g.v(class_def_1).data;
	coverage_glyphs(&g.v(coverage).data)
		.into_iter()
		.map(|gid| (gid, class_def_get_class(class_def, gid)))
		.collect()
}

/// `PairPosFormat2::split_subtables`.
fn split_format2(c: &mut Context, g: &mut Graph, this_index: u32) -> Result<Vec<u32>> {
	let base_size = FORMAT2_MIN_SIZE;
	let class_def_2_id = g.index_for_offset(this_index, 10)?;
	let class_def_2_size = g.v(class_def_2_id).table_size() as u32;
	let coverage_id = g.index_for_offset(this_index, 2)?;
	coverage_sanitize(&g.v(coverage_id).data)?;
	let class_def_1_id = g.index_for_offset(this_index, 8)?;
	class_def_sanitize(&g.v(class_def_1_id).data)?;
	let gid_and_class = glyphs_and_classes(g, coverage_id, class_def_1_id);
	let mut estimator = ClassDefSizeEstimator::new(&gid_and_class);

	let this = g.v(this_index).data.clone();
	let class1_count = u32::from(get16(&this, 12));
	let class2_count = u32::from(get16(&this, 14));
	let class1_record_size = class1_record_size(&this);
	let value_1_len = value_len(get16(&this, 4));
	let value_2_len = value_len(get16(&this, 6));
	let total_value_len = value_1_len + value_2_len;

	let mut accumulated = base_size;
	let mut coverage_size;
	let mut class_def_1_size;
	let mut max_coverage_size = 4;
	let mut max_class_def_1_size = 4;
	let mut split_points = Vec::new();

	let device_tables = g.v(this_index).position_to_index_map();
	let format1_device_table_indices = device_table_indices(get16(&this, 4));
	let format2_device_table_indices = device_table_indices(get16(&this, 6));
	let has_device_tables =
		!format1_device_table_indices.is_empty() || !format2_device_table_indices.is_empty();

	let mut visited = BTreeSet::new();
	for i in 0..class1_count {
		let mut accumulated_delta = class1_record_size;
		class_def_1_size = estimator.add_class_def_size(i);
		coverage_size = estimator.coverage_size();
		max_coverage_size = max_coverage_size.max(coverage_size);
		max_class_def_1_size = max_class_def_1_size.max(class_def_1_size);
		if has_device_tables {
			for j in 0..class2_count {
				let value1_index = total_value_len * (class2_count * i + j);
				let value2_index = value1_index + value_1_len;
				accumulated_delta += size_of_value_record_children(
					g,
					&device_tables,
					&format1_device_table_indices,
					value1_index,
					&mut visited,
				);
				accumulated_delta += size_of_value_record_children(
					g,
					&device_tables,
					&format2_device_table_indices,
					value2_index,
					&mut visited,
				);
			}
		}
		accumulated += accumulated_delta;
		// The largest object packs last and can exceed the limit.
		let total = accumulated + coverage_size + class_def_1_size + class_def_2_size
			- coverage_size.max(class_def_1_size).max(class_def_2_size);
		if total >= 1 << 16 {
			split_points.push(i);
			// The split does not include i, so its size starts the next one.
			accumulated = base_size + accumulated_delta;
			estimator.reset();
			estimator.add_class_def_size(i);
			visited.clear();
		}
	}

	let mut sc = Format2Split {
		this_index,
		class1_record_size,
		value_record_len: total_value_len,
		value1_record_len: value_1_len,
		max_coverage_size,
		max_class_def_size: max_class_def_1_size,
		device_tables,
		format1_device_table_indices,
		format2_device_table_indices,
	};
	actuate_subtable_split(&mut sc, c, g, &split_points)
}

/// `PairPosFormat2::size_of_value_record_children`.
fn size_of_value_record_children(
	g: &mut Graph,
	device_tables: &HashMap<u32, u32>,
	device_table_indices: &[u32],
	value_record_index: u32,
	visited: &mut BTreeSet<u32>,
) -> u32 {
	let mut size = 0;
	for &i in device_table_indices {
		let record_position = FORMAT2_MIN_SIZE + 2 * (value_record_index + i);
		let Some(&obj_idx) = device_tables.get(&record_position) else {
			continue;
		};
		size += g
			.find_subgraph_size(obj_idx, visited, u32::MAX)
			.unwrap_or(0) as u32;
	}
	size
}

struct Format2Split {
	this_index: u32,
	class1_record_size: u32,
	value_record_len: u32,
	value1_record_len: u32,
	max_coverage_size: u32,
	max_class_def_size: u32,
	device_tables: HashMap<u32, u32>,
	format1_device_table_indices: Vec<u32>,
	format2_device_table_indices: Vec<u32>,
}

impl Format2Split {
	/// `PairPosFormat2::clone_class1_records`.
	fn clone_class1_records(&self, g: &mut Graph, prime_id: u32, start: u32, end: u32) -> Result<()> {
		let this_index = self.this_index;
		let start_addr = (FORMAT2_MIN_SIZE + start * self.class1_record_size) as usize;
		let len = ((end - start) * self.class1_record_size) as usize;
		let records = g.v(this_index).data[start_addr..start_addr + len].to_vec();
		let base = FORMAT2_MIN_SIZE as usize;
		g.vm(prime_id).data[base..base + len].copy_from_slice(&records);
		if self.format1_device_table_indices.is_empty() && self.format2_device_table_indices.is_empty() {
			// No device tables to move over.
			return Ok(());
		}
		let class2_count = u32::from(get16(&g.v(this_index).data, 14));
		for i in start..end {
			for j in 0..class2_count {
				let value1_index = self.value_record_len * (class2_count * i + j);
				let value2_index = value1_index + self.value1_record_len;
				let new_value1_index = self.value_record_len * (class2_count * (i - start) + j);
				let new_value2_index = new_value1_index + self.value1_record_len;
				self.transfer_device_tables(
					g,
					prime_id,
					&self.format1_device_table_indices,
					value1_index,
					new_value1_index,
				)?;
				self.transfer_device_tables(
					g,
					prime_id,
					&self.format2_device_table_indices,
					value2_index,
					new_value2_index,
				)?;
			}
		}
		Ok(())
	}

	/// `PairPosFormat2::transfer_device_tables`.
	fn transfer_device_tables(
		&self,
		g: &mut Graph,
		prime_id: u32,
		device_table_indices: &[u32],
		old_value_record_index: u32,
		new_value_record_index: u32,
	) -> Result<()> {
		for &i in device_table_indices {
			let record_position = FORMAT2_MIN_SIZE + 2 * (old_value_record_index + i);
			if !self.device_tables.contains_key(&record_position) {
				continue;
			}
			g.move_child(
				self.this_index,
				record_position,
				prime_id,
				FORMAT2_MIN_SIZE + 2 * (new_value_record_index + i),
			)?;
		}
		// Restore the order of the links, disturbed by `move_child`.
		g.vm(self.this_index).sort_real_links();
		Ok(())
	}
}

impl SplitContext for Format2Split {
	fn original_count(&self, g: &Graph) -> u32 {
		u32::from(get16(&g.v(self.this_index).data, 12))
	}

	/// `PairPosFormat2::clone_range`: a new PairPos with the class 1 records of
	/// `[start, end)`.
	fn clone_range(&mut self, c: &mut Context, g: &mut Graph, start: u32, end: u32) -> Result<u32> {
		let this_index = self.this_index;
		let num_records = end - start;
		let prime_size = FORMAT2_MIN_SIZE + num_records * self.class1_record_size;
		let prime_id = c.create_node(g, prime_size as usize)?;
		let this = g.v(this_index).data.clone();
		let prime = &mut g.vm(prime_id).data;
		set16(prime, 0, get16(&this, 0));
		set16(prime, 4, get16(&this, 4));
		set16(prime, 6, get16(&this, 6));
		set16(prime, 12, num_records as u16);
		set16(prime, 14, get16(&this, 14));
		self.clone_class1_records(g, prime_id, start, end)?;

		let coverage_id = g.index_for_offset(this_index, 2)?;
		let class_def_1_id = g.index_for_offset(this_index, 8)?;
		coverage_sanitize(&g.v(coverage_id).data)?;
		class_def_sanitize(&g.v(class_def_1_id).data)?;
		// Classes must be from 0 to N, so `start` is subtracted.
		let klass_map: Vec<(u32, u32)> = glyphs_and_classes(g, coverage_id, class_def_1_id)
			.into_iter()
			.filter(|&(_, klass)| klass >= start && klass < end)
			.map(|(gid, klass)| (gid, klass - start))
			.collect();
		let glyphs: Vec<u32> = klass_map.iter().map(|p| p.0).collect();
		add_coverage(g, prime_id, 2, &glyphs, self.max_coverage_size as usize)?;
		add_class_def(g, prime_id, 8, &klass_map, self.max_class_def_size as usize)?;

		let class_def_2_id = g.index_for_offset(this_index, 10)?;
		g.vm(prime_id).add_real_link(2, class_def_2_id, 10);
		g.vm(class_def_2_id).add_parent(prime_id, false)?;
		Ok(prime_id)
	}

	/// `PairPosFormat2::shrink`: keeps the class 1 records of `[0, count)`.
	fn shrink(&mut self, _c: &mut Context, g: &mut Graph, count: u32) -> Result<()> {
		let this_index = self.this_index;
		let old_count = u32::from(get16(&g.v(this_index).data, 12));
		if count >= old_count {
			return Ok(());
		}
		set16(&mut g.vm(this_index).data, 12, count as u16);
		g.vm(this_index)
			.shrink_buffer(((old_count - count) * self.class1_record_size) as usize);
		let coverage = as_mutable_table(g, this_index, 2, coverage_sanitize)?;
		let class_def_1 = as_mutable_table(g, this_index, 8, class_def_sanitize)?;
		let klass_map: Vec<(u32, u32)> = glyphs_and_classes(g, coverage, class_def_1)
			.into_iter()
			.filter(|&(_, klass)| klass < count)
			.collect();
		let new_coverage: Vec<u32> = klass_map.iter().map(|p| p.0).collect();
		// The existing ranges may not be kept: the worst case is format 1.
		make_coverage(g, &new_coverage, coverage, 4 + new_coverage.len() * 2)?;
		let class_def_1_size = g.v(class_def_1).table_size();
		make_class_def(g, &klass_map, class_def_1, class_def_1_size)
	}
}
