//! Splitting of `LigatureSubst` subtables (`graph/ligature-graph.hh`).

use std::collections::BTreeSet;

use crate::graph::{Graph, INVALID};
use crate::gsubgpos::{Context, SplitContext, actuate_subtable_split};
use crate::layout::{
	as_mutable_table, as_table, coverage_glyphs, filter_coverage, get16, make_coverage, set16,
};
use crate::{Error, Result};

/// `LigatureSubstFormat1_2<SmallTypes>::min_size`.
const FORMAT1_MIN_SIZE: u32 = 6;
/// `LigatureSet<SmallTypes>::min_size`.
const LIGATURE_SET_MIN_SIZE: u32 = 2;

/// `split_subtable<LigatureSubst>`.
pub(crate) fn split_subtable(c: &mut Context, g: &mut Graph, objidx: u32) -> Result<Vec<u32>> {
	let data = &g.v(objidx).data;
	// `LigatureSubst::sanitize`.
	if data.len() < 2 || get16(data, 0) != 1 || format1_sanitize(data).is_err() {
		return Ok(Vec::new());
	}
	split_subtables(c, g, objidx)
}

/// `LigatureSubstFormat1::sanitize`.
fn format1_sanitize(data: &[u8]) -> Result<()> {
	let len = data.len();
	if len < FORMAT1_MIN_SIZE as usize
		|| len < (FORMAT1_MIN_SIZE + 2 * u32::from(get16(data, 4))) as usize
	{
		return Err(Error::SanitizeFailure);
	}
	Ok(())
}

/// `LigatureSet::sanitize`.
fn ligature_set_sanitize(data: &[u8]) -> Result<()> {
	let len = data.len();
	if len < LIGATURE_SET_MIN_SIZE as usize
		|| len < (LIGATURE_SET_MIN_SIZE + 2 * u32::from(get16(data, 0))) as usize
	{
		return Err(Error::SanitizeFailure);
	}
	Ok(())
}

fn ligature_set_position(i: u32) -> u32 {
	FORMAT1_MIN_SIZE + 2 * i
}

fn ligature_count(g: &Graph, liga_set: u32) -> u32 {
	u32::from(get16(&g.v(liga_set).data, 0))
}

/// `LigatureSubstFormat1::split_subtables`.
fn split_subtables(c: &mut Context, g: &mut Graph, this_index: u32) -> Result<Vec<u32>> {
	let split_points = compute_split_points(g, this_index)?;
	if split_points.is_empty() {
		return Ok(Vec::new());
	}
	let mut total = 0;
	let mut liga_counts = Vec::new();
	let count = u32::from(get16(&g.v(this_index).data, 4));
	for i in 0..count {
		let liga_set = as_table(
			g,
			this_index,
			ligature_set_position(i),
			ligature_set_sanitize,
		)?;
		total += ligature_count(g, liga_set);
	}
	for i in 0..count {
		let liga_set = as_table(
			g,
			this_index,
			ligature_set_position(i),
			ligature_set_sanitize,
		)?;
		liga_counts.push(ligature_count(g, liga_set));
	}
	let mut sc = Split {
		this_index,
		original_count: total,
		liga_counts,
	};
	actuate_subtable_split(&mut sc, c, g, &split_points)
}

/// `ligature_index_to_object_id`: the ligature linked at each index of a set.
fn ligature_index_to_object_id(g: &Graph, liga_set: u32) -> Vec<u32> {
	let mut map = vec![INVALID; ligature_count(g, liga_set) as usize];
	for l in &g.v(liga_set).real_links {
		if l.position < 2 {
			continue;
		}
		let array_index = ((l.position - 2) / 2) as usize;
		if let Some(m) = map.get_mut(array_index) {
			*m = l.objidx;
		}
	}
	map
}

/// `compute_split_points`: the coverage is packed last, so it is where an
/// overflow happens; the estimate is the size of everything else.
fn compute_split_points(g: &mut Graph, this_index: u32) -> Result<Vec<u32>> {
	let base_size = FORMAT1_MIN_SIZE;
	let mut accumulated = base_size;
	let mut ligature_index = 0;
	let mut split_points = Vec::new();
	let count = u32::from(get16(&g.v(this_index).data, 4));
	for i in 0..count {
		// The ligature set offset and table.
		accumulated += 2 + LIGATURE_SET_MIN_SIZE;
		let liga_set = as_table(
			g,
			this_index,
			ligature_set_position(i),
			ligature_set_sanitize,
		)?;
		let index_to_id = ligature_index_to_object_id(g, liga_set);
		for liga_id in index_to_id {
			if liga_id == INVALID {
				// No outgoing link: ignored.
				continue;
			}
			let liga_size = g.v(liga_id).table_size() as u32;
			accumulated += 2 + liga_size;
			if accumulated >= 1 << 16 {
				split_points.push(ligature_index);
				// The current ligature goes into the new subtable, with one
				// ligature subst, one ligature set and one ligature.
				accumulated = base_size + 2 * 2 + LIGATURE_SET_MIN_SIZE + liga_size;
			}
			ligature_index += 1;
		}
	}
	Ok(split_points)
}

/// `clear_virtual_links`.
fn clear_virtual_links(g: &mut Graph, node_index: u32) {
	let links = std::mem::take(&mut g.vm(node_index).virtual_links);
	for l in links {
		g.vm(l.objidx).remove_parent(node_index);
	}
}

/// `add_virtual_link`.
fn add_virtual_link(g: &mut Graph, from: u32, to: u32) -> Result<()> {
	if from >= g.len() || to >= g.len() {
		return Err(Error::OutOfBounds);
	}
	g.vm(to).add_parent(from, true)?;
	g.vm(from).add_virtual_link(to);
	Ok(())
}

/// `current_liga_set_bounds`: the range of ligature indices still linked from
/// a set.
fn current_liga_set_bounds(g: &Graph, liga_set: u32) -> (u32, u32) {
	let mut min_index = INVALID;
	let mut max_index = 0;
	for l in &g.v(liga_set).real_links {
		if l.position < 2 {
			continue;
		}
		let liga_index = (l.position - 2) / 2;
		min_index = min_index.min(liga_index);
		max_index = max_index.max(liga_index);
	}
	(min_index, max_index + 1)
}

/// `compact_liga_set`: packs the remaining ligature offsets together.
fn compact_liga_set(g: &mut Graph, liga_set: u32) {
	let v = g.vm(liga_set);
	let count = u32::from(get16(&v.data, 0));
	let num_links = v.real_links.len() as u32;
	if count <= num_links {
		return;
	}
	let to_remove = count - num_links;
	// The links must be processed in position order.
	v.sort_real_links();
	let mut new_position = 2;
	for l in &mut v.real_links {
		l.position = new_position;
		new_position += 2;
	}
	set16(&mut v.data, 0, num_links as u16);
	v.shrink_buffer((to_remove * 2) as usize);
}

/// `new_liga_set`: an empty ligature set of `count` ligatures.
fn new_liga_set(c: &mut Context, g: &mut Graph, count: u32) -> Result<u32> {
	let prime_id = c.create_node(g, LIGATURE_SET_MIN_SIZE as usize + count as usize * 2)?;
	set16(&mut g.vm(prime_id).data, 0, count as u16);
	Ok(prime_id)
}

struct Split {
	this_index: u32,
	original_count: u32,
	liga_counts: Vec<u32>,
}

impl SplitContext for Split {
	fn original_count(&self, _g: &Graph) -> u32 {
		self.original_count
	}

	/// `LigatureSubstFormat1::clone_range`: a new LigatureSubst with the
	/// ligatures `[start, end)`, counted across all ligature sets.
	fn clone_range(&mut self, c: &mut Context, g: &mut Graph, start: u32, end: u32) -> Result<u32> {
		let this_index = self.this_index;
		// Oversized for now: the final size is only known after processing.
		let this_count = u32::from(get16(&g.v(this_index).data, 4));
		let prime_size = FORMAT1_MIN_SIZE + 2 * this_count;
		let liga_subst_prime_id = c.create_node(g, prime_size as usize)?;
		let format = get16(&g.v(this_index).data, 0);
		let prime = &mut g.vm(liga_subst_prime_id).data;
		set16(prime, 0, format);
		set16(prime, 4, this_count as u16);

		// A placeholder coverage, so that virtual links can be added to it; it is
		// given the right glyphs at the end.
		let coverage_id = g.index_for_offset(this_index, 2)?;
		let coverage_prime_id = g.duplicate(coverage_id, false)?;
		g.vm(liga_subst_prime_id)
			.add_real_link(2, coverage_prime_id, 2);
		g.vm(coverage_prime_id)
			.add_parent(liga_subst_prime_id, false)?;

		// Clone or move the ligature sets with ligatures in [start, end).
		let mut count = 0;
		let mut liga_set_count = 0;
		let mut liga_set_start = INVALID;
		let mut liga_set_end = 0;
		for i in 0..self.liga_counts.len() as u32 {
			let num_ligas = self.liga_counts[i as usize];
			let mut current_start = count;
			let mut current_end = count + num_ligas;
			if current_start >= end || start >= current_end {
				// No intersection.
				count += num_ligas;
				continue;
			}
			let liga_set_index = as_mutable_table(
				g,
				this_index,
				ligature_set_position(i),
				ligature_set_sanitize,
			)?;
			// Some ligatures may have been removed already.
			let liga_bounds = current_liga_set_bounds(g, liga_set_index);
			current_start = count.wrapping_add(liga_bounds.0).max(current_start);
			current_end = count.wrapping_add(liga_bounds.1).min(current_end);

			let liga_set_prime_id;
			if current_start >= start && current_end <= end {
				// The whole set is in [start, end): it is moved.
				liga_set_end = i;
				liga_set_start = liga_set_start.min(i);
				liga_set_prime_id = g.move_child(
					this_index,
					ligature_set_position(i),
					liga_subst_prime_id,
					ligature_set_position(liga_set_count),
				)?;
				liga_set_count += 1;
				compact_liga_set(g, liga_set_index);
			} else {
				// The set overlaps [start, end): the ligatures in the range move to a
				// new set.
				let start_index = start.max(current_start).wrapping_sub(count);
				let end_index = end.min(current_end).wrapping_sub(count);
				let liga_count = end_index.wrapping_sub(start_index);
				liga_set_prime_id = new_liga_set(c, g, liga_count)?;
				g.move_children(
					liga_set_index,
					start_index.wrapping_mul(2).wrapping_add(2),
					end_index.wrapping_mul(2).wrapping_add(2),
					liga_set_prime_id,
					2,
				)?;
				liga_set_end = i;
				liga_set_start = liga_set_start.min(i);
				g.add_link(
					ligature_set_position(liga_set_count),
					liga_subst_prime_id,
					liga_set_prime_id,
				)?;
				liga_set_count += 1;
			}

			// The new set and its ligatures need a virtual link to the new
			// coverage.
			clear_virtual_links(g, liga_set_prime_id);
			add_virtual_link(g, liga_set_prime_id, coverage_prime_id)?;
			let ligatures: Vec<u32> = g
				.v(liga_set_prime_id)
				.real_links
				.iter()
				.map(|l| l.objidx)
				.collect();
			for l in ligatures {
				clear_virtual_links(g, l);
				add_virtual_link(g, l, coverage_prime_id)?;
			}
			count += num_ligas;
		}

		let prime = g.vm(liga_subst_prime_id);
		let prime_count = u32::from(get16(&prime.data, 4));
		prime.shrink_buffer((prime_count.wrapping_sub(liga_set_count) * 2) as usize);
		set16(&mut prime.data, 4, liga_set_count as u16);
		filter_coverage(g, coverage_prime_id, liga_set_start, liga_set_end + 1)?;
		Ok(liga_subst_prime_id)
	}

	/// `LigatureSubstFormat1::shrink`: keeps the ligatures `[0, count)`.
	fn shrink(&mut self, _c: &mut Context, g: &mut Graph, count: u32) -> Result<()> {
		let this_index = self.this_index;
		if count >= self.original_count {
			return Ok(());
		}
		let mut count = count;
		let mut retained_indices = BTreeSet::new();
		let mut new_liga_set_count = 0;
		for i in 0..self.liga_counts.len() as u32 {
			let liga_set = as_mutable_table(
				g,
				this_index,
				ligature_set_position(i),
				ligature_set_sanitize,
			)?;
			// The virtual links to the coverage are removed from all descendants
			// (else the coverage would be duplicated when it is changed), and added
			// back to the retained ones below.
			clear_virtual_links(g, liga_set);
			retained_indices.insert(liga_set);
			for liga_index in ligature_index_to_object_id(g, liga_set) {
				if liga_index != INVALID {
					clear_virtual_links(g, liga_index);
					retained_indices.insert(liga_index);
				}
			}
			let num_ligas = self.liga_counts[i as usize];
			if num_ligas >= count {
				// Drop the trailing ligatures of this set and all later sets.
				let num_ligas_to_remove = num_ligas - count;
				new_liga_set_count = i + 1;
				let v = g.vm(liga_set);
				v.shrink_buffer((num_ligas_to_remove * 2) as usize);
				set16(&mut v.data, 0, count as u16);
				break;
			}
			count -= num_ligas;
		}

		let v = g.vm(this_index);
		let liga_set_count = u32::from(get16(&v.data, 4));
		v.shrink_buffer((liga_set_count.wrapping_sub(new_liga_set_count) * 2) as usize);
		set16(&mut v.data, 4, new_liga_set_count as u16);

		// The coverage matches the ligature sets, so it is rebuilt.
		let mut coverage_idx = g.index_for_offset(this_index, 2)?;
		let coverage_size = g.v(coverage_idx).table_size();
		let coverage_table = g.v(coverage_idx).data.clone();
		if g.v(coverage_idx).is_shared() {
			coverage_idx = g.remap_child(this_index, coverage_idx)?;
		}
		for i in retained_indices {
			add_virtual_link(g, i, coverage_idx)?;
		}
		let new_coverage: Vec<u32> = coverage_glyphs(&coverage_table)
			.into_iter()
			.take(new_liga_set_count as usize)
			.collect();
		make_coverage(g, &new_coverage, coverage_idx, coverage_size)
	}
}
