//! Splitting of `MarkBasePos` subtables (`graph/markbasepos-graph.hh`).

use std::collections::{BTreeSet, HashMap};

use crate::graph::Graph;
use crate::gsubgpos::{Context, SplitContext, actuate_subtable_split};
use crate::layout::{
	add_coverage, as_mutable_table, as_table, coverage_glyphs, coverage_sanitize, get16,
	make_coverage, set16,
};
use crate::{Error, Result};

/// `MarkBasePosFormat1_2<SmallTypes>::static_size`.
const FORMAT1_SIZE: u32 = 12;
/// `MarkArray::min_size` and `AnchorMatrix::min_size`.
const ARRAY_MIN_SIZE: u32 = 2;
/// `MarkRecord<SmallTypes>::static_size`.
const MARK_RECORD_SIZE: u32 = 4;

/// `split_subtable<MarkBasePos>`.
pub(crate) fn split_subtable(c: &mut Context, g: &mut Graph, objidx: u32) -> Result<Vec<u32>> {
	let data = &g.v(objidx).data;
	// `MarkBasePos::sanitize`.
	if data.len() < 2 || get16(data, 0) != 1 || data.len() < FORMAT1_SIZE as usize {
		return Ok(Vec::new());
	}
	split_subtables(c, g, objidx)
}

/// `AnchorMatrix::sanitize`.
fn anchor_matrix_sanitize(data: &[u8], class_count: u32) -> Result<()> {
	let len = data.len();
	let rows = get16(data, 0) as usize;
	if len < ARRAY_MIN_SIZE as usize
		|| len < ARRAY_MIN_SIZE as usize + 2 * class_count as usize * rows
	{
		return Err(Error::SanitizeFailure);
	}
	Ok(())
}

/// `MarkArray::sanitize`.
fn mark_array_sanitize(data: &[u8]) -> Result<()> {
	let len = data.len();
	if len < ARRAY_MIN_SIZE as usize
		|| len < (ARRAY_MIN_SIZE + MARK_RECORD_SIZE * u32::from(get16(data, 0))) as usize
	{
		return Err(Error::SanitizeFailure);
	}
	Ok(())
}

/// The class of a mark record; 0 if out of range (`Null (MarkRecord)`).
fn mark_class(data: &[u8], mark: u32) -> u32 {
	if mark >= u32::from(get16(data, 0)) {
		return 0;
	}
	u32::from(get16(
		data,
		(ARRAY_MIN_SIZE + MARK_RECORD_SIZE * mark) as usize,
	))
}

/// The position of the anchor offset of a mark record.
fn mark_anchor_position(mark: u32) -> u32 {
	ARRAY_MIN_SIZE + MARK_RECORD_SIZE * mark + 2
}

/// `class_info_t`.
#[derive(Debug, Default, Clone)]
struct ClassInfo {
	marks: BTreeSet<u32>,
	child_indices: Vec<u32>,
}

/// `MarkBasePosFormat1::split_subtables`.
fn split_subtables(c: &mut Context, g: &mut Graph, this_index: u32) -> Result<Vec<u32>> {
	let mut visited = BTreeSet::new();
	let base_coverage_id = g.index_for_offset(this_index, 4)?;
	let base_size =
		FORMAT1_SIZE + ARRAY_MIN_SIZE + ARRAY_MIN_SIZE + g.v(base_coverage_id).table_size() as u32;
	let class_to_info = get_class_info(g, this_index)?;
	let class_count = u32::from(get16(&g.v(this_index).data, 6));
	let base_array = as_table(g, this_index, 10, |d| {
		anchor_matrix_sanitize(d, class_count)
	})?;
	let base_count = u32::from(get16(&g.v(base_array).data, 0));

	let mut partial_coverage_size = 4;
	let mut accumulated = base_size;
	let mut split_points = Vec::new();
	for klass in 0..class_count {
		let info = &class_to_info[klass as usize];
		let num_marks = info.marks.len() as u32;
		partial_coverage_size += 2 * num_marks;
		let mut accumulated_delta = MARK_RECORD_SIZE * num_marks + 2 * base_count;
		for &objidx in &info.child_indices {
			accumulated_delta += g
				.find_subgraph_size(objidx, &mut visited, u32::MAX)
				.unwrap_or(0) as u32;
		}
		accumulated += accumulated_delta;
		let total = accumulated + partial_coverage_size;
		if total >= 1 << 16 {
			split_points.push(klass);
			accumulated = base_size + accumulated_delta;
			partial_coverage_size = 4 + 2 * num_marks;
			// Nodes are not shared between splits.
			visited.clear();
		}
	}

	let mark_array_id = g.index_for_offset(this_index, 8)?;
	let mark_array_links = g.v(mark_array_id).position_to_index_map();
	let mut sc = Split {
		this_index,
		class_to_info,
		mark_array_links,
	};
	actuate_subtable_split(&mut sc, c, g, &split_points)
}

/// `MarkBasePosFormat1::get_class_info`: the marks of each class, and the
/// children of the mark and base arrays for each class.
fn get_class_info(g: &Graph, this_index: u32) -> Result<Vec<ClassInfo>> {
	let class_count = u32::from(get16(&g.v(this_index).data, 6));
	if class_count == 0 {
		return Ok(Vec::new());
	}
	let mut class_to_info = vec![ClassInfo::default(); class_count as usize];
	let mark_array = as_table(g, this_index, 8, mark_array_sanitize)?;
	let data = &g.v(mark_array).data;
	let mark_count = u32::from(get16(data, 0));
	for mark in 0..mark_count {
		let klass = mark_class(data, mark);
		if klass >= class_count {
			continue;
		}
		class_to_info[klass as usize].marks.insert(mark);
	}
	for link in &g.v(mark_array).real_links {
		let mark = link.position.wrapping_sub(2) / MARK_RECORD_SIZE;
		let klass = mark_class(data, mark);
		if klass >= class_count {
			continue;
		}
		class_to_info[klass as usize]
			.child_indices
			.push(link.objidx);
	}
	let base_array_id = g.index_for_offset(this_index, 10)?;
	for link in &g.v(base_array_id).real_links {
		let index = link.position.wrapping_sub(2) / 2;
		let klass = index % class_count;
		class_to_info[klass as usize]
			.child_indices
			.push(link.objidx);
	}
	Ok(class_to_info)
}

struct Split {
	this_index: u32,
	class_to_info: Vec<ClassInfo>,
	mark_array_links: HashMap<u32, u32>,
}

impl Split {
	/// `marks_for`: the marks of the classes `[start, end)`.
	fn marks_for(&self, start: u32, end: u32) -> BTreeSet<u32> {
		(start..end)
			.flat_map(|klass| self.class_to_info[klass as usize].marks.iter().copied())
			.collect()
	}
}

/// The glyphs of the coverage whose coverage index is in `marks`.
fn coverage_of_marks(data: &[u8], marks: &BTreeSet<u32>) -> Vec<u32> {
	coverage_glyphs(data)
		.into_iter()
		.enumerate()
		.filter(|&(i, _)| marks.contains(&(i as u32)))
		.map(|(_, g)| g)
		.collect()
}

impl SplitContext for Split {
	fn original_count(&self, g: &Graph) -> u32 {
		u32::from(get16(&g.v(self.this_index).data, 6))
	}

	/// `MarkBasePosFormat1::clone_range`: a new MarkBasePos with the classes
	/// `[start, end)`.
	fn clone_range(&mut self, c: &mut Context, g: &mut Graph, start: u32, end: u32) -> Result<u32> {
		let this_index = self.this_index;
		let prime_id = c.create_node(g, FORMAT1_SIZE as usize)?;
		let format = get16(&g.v(this_index).data, 0);
		let prime = &mut g.vm(prime_id).data;
		set16(prime, 0, format);
		set16(prime, 6, (end - start) as u16);

		let base_coverage_id = g.index_for_offset(this_index, 4)?;
		g.add_link(4, prime_id, base_coverage_id)?;

		let mark_coverage = as_table(g, this_index, 2, coverage_sanitize)?;
		let marks = self.marks_for(start, end);
		let new_coverage = coverage_of_marks(&g.v(mark_coverage).data, &marks);
		add_coverage(g, prime_id, 2, &new_coverage, marks.len() * 2 + 4)?;

		let mark_array = as_mutable_table(g, this_index, 8, mark_array_sanitize)?;
		let new_mark_array =
			clone_mark_array(c, g, mark_array, &self.mark_array_links, &marks, start)?;
		g.add_link(8, prime_id, new_mark_array)?;

		let class_count = u32::from(get16(&g.v(this_index).data, 6));
		let base_array = as_mutable_table(g, this_index, 10, |d| {
			anchor_matrix_sanitize(d, class_count)
		})?;
		let new_base_array = clone_anchor_matrix(c, g, base_array, start, end, class_count)?;
		g.add_link(10, prime_id, new_base_array)?;
		Ok(prime_id)
	}

	/// `MarkBasePosFormat1::shrink`: keeps the classes `[0, count)`.
	fn shrink(&mut self, _c: &mut Context, g: &mut Graph, count: u32) -> Result<()> {
		let this_index = self.this_index;
		let old_count = u32::from(get16(&g.v(this_index).data, 6));
		if count >= old_count {
			return Ok(());
		}
		set16(&mut g.vm(this_index).data, 6, count as u16);

		let mark_coverage = as_mutable_table(g, this_index, 2, coverage_sanitize)?;
		let marks = self.marks_for(0, count);
		let new_coverage = coverage_of_marks(&g.v(mark_coverage).data, &marks);
		make_coverage(g, &new_coverage, mark_coverage, 4 + 2 * marks.len())?;

		let base_array =
			as_mutable_table(g, this_index, 10, |d| anchor_matrix_sanitize(d, old_count))?;
		shrink_anchor_matrix(g, base_array, old_count, count)?;

		let mark_array = as_mutable_table(g, this_index, 8, mark_array_sanitize)?;
		shrink_mark_array(g, &self.mark_array_links, mark_array, count)
	}
}

/// `AnchorMatrix::shrink`: keeps the columns of the classes `[0, new_class_count)`.
fn shrink_anchor_matrix(
	g: &mut Graph,
	this_index: u32,
	old_class_count: u32,
	new_class_count: u32,
) -> Result<()> {
	if new_class_count >= old_class_count {
		return Err(Error::InvalidArgument);
	}
	let v = g.vm(this_index);
	let base_count = u32::from(get16(&v.data, 0));
	v.data
		.truncate((ARRAY_MIN_SIZE + 2 * base_count * new_class_count) as usize);
	// Reposition the links in the new indexing scheme.
	for link in &mut v.real_links {
		let index = link.position.wrapping_sub(2) / 2;
		let base = index / old_class_count;
		let klass = index % old_class_count;
		if klass >= new_class_count {
			// These should have been removed already.
			return Err(Error::InvalidArgument);
		}
		let new_index = base * new_class_count + klass;
		link.position = ARRAY_MIN_SIZE + 2 * new_index;
	}
	Ok(())
}

/// `AnchorMatrix::clone`: a new matrix with the columns of the classes
/// `[start, end)`, whose links are moved from this one.
fn clone_anchor_matrix(
	c: &mut Context,
	g: &mut Graph,
	this_index: u32,
	start: u32,
	end: u32,
	class_count: u32,
) -> Result<u32> {
	let base_count = u32::from(get16(&g.v(this_index).data, 0));
	let new_class_count = end - start;
	let size = ARRAY_MIN_SIZE + 2 * new_class_count * base_count;
	let prime_id = c.create_node(g, size as usize)?;
	set16(&mut g.vm(prime_id).data, 0, base_count as u16);

	let mut num_links = g.v(this_index).real_links.len();
	let mut i = 0;
	while i < num_links {
		let link = g.v(this_index).real_links[i];
		let old_index = link.position.wrapping_sub(2) / 2;
		let klass = old_index % class_count;
		if klass < start || klass >= end {
			i += 1;
			continue;
		}
		let base = old_index / class_count;
		let new_index = base * new_class_count + (klass - start);
		let child_idx = link.objidx;
		g.add_link(ARRAY_MIN_SIZE + 2 * new_index, prime_id, child_idx)?;
		g.vm(child_idx).remove_parent(this_index);
		g.vm(this_index).real_links.swap_remove(i);
		num_links -= 1;
	}
	// Restore the order of the links after the removals.
	g.vm(this_index).sort_real_links();
	Ok(prime_id)
}

/// `MarkArray::shrink`: keeps the marks of the classes `[0, new_class_count)`.
fn shrink_mark_array(
	g: &mut Graph,
	mark_array_links: &HashMap<u32, u32>,
	this_index: u32,
	new_class_count: u32,
) -> Result<()> {
	let links = std::mem::take(&mut g.vm(this_index).real_links);
	for link in links {
		g.vm(link.objidx).remove_parent(this_index);
	}
	let mark_count = u32::from(get16(&g.v(this_index).data, 0));
	let mut new_index = 0;
	for mark in 0..mark_count {
		let klass = mark_class(&g.v(this_index).data, mark);
		if klass >= new_class_count {
			continue;
		}
		set16(
			&mut g.vm(this_index).data,
			(ARRAY_MIN_SIZE + MARK_RECORD_SIZE * new_index) as usize,
			klass as u16,
		);
		if let Some(&objidx) = mark_array_links.get(&mark_anchor_position(mark)) {
			g.add_link(mark_anchor_position(new_index), this_index, objidx)?;
		}
		new_index += 1;
	}
	let v = g.vm(this_index);
	set16(&mut v.data, 0, new_index as u16);
	v.data
		.truncate((ARRAY_MIN_SIZE + MARK_RECORD_SIZE * new_index) as usize);
	Ok(())
}

/// `MarkArray::clone`: a new mark array with `marks`, whose anchor links are
/// moved from this one.
fn clone_mark_array(
	c: &mut Context,
	g: &mut Graph,
	this_index: u32,
	pos_to_index: &HashMap<u32, u32>,
	marks: &BTreeSet<u32>,
	start_class: u32,
) -> Result<u32> {
	let size = ARRAY_MIN_SIZE + MARK_RECORD_SIZE * marks.len() as u32;
	let prime_id = c.create_node(g, size as usize)?;
	set16(&mut g.vm(prime_id).data, 0, marks.len() as u16);
	for (i, &mark) in marks.iter().enumerate() {
		let i = i as u32;
		let klass = mark_class(&g.v(this_index).data, mark).wrapping_sub(start_class);
		set16(
			&mut g.vm(prime_id).data,
			(ARRAY_MIN_SIZE + MARK_RECORD_SIZE * i) as usize,
			klass as u16,
		);
		let offset_pos = mark_anchor_position(mark);
		if pos_to_index.contains_key(&offset_pos) {
			g.move_child(this_index, offset_pos, prime_id, mark_anchor_position(i))?;
		}
	}
	// Restore the order of the links, disturbed by `move_child`.
	g.vm(this_index).sort_real_links();
	Ok(prime_id)
}
