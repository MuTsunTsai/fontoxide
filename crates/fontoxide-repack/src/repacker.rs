//! `hb-repacker.hh`: the overflow resolution algorithm (see HarfBuzz's
//! `docs/repacker.md`).

use std::collections::BTreeSet;

use crate::graph::Graph;
use crate::gsubgpos::{self, Context, GPOS, GSUB};
use crate::serialize::{self, OverflowRecord, will_overflow};
use crate::{Error, Object, Result};

/// `HB_REPACKER_MAX_ITERATIONS`.
const MAX_ITERATIONS: u32 = 500;

/// `lookup_size_t`.
#[derive(Debug, Clone, Copy)]
struct LookupSize {
	lookup_index: u32,
	size: usize,
	num_subtables: u32,
}

/// `lookup_size_t::cmp`: the most subtables per byte first, then the highest
/// lookup index.
fn cmp_lookup_size(a: &LookupSize, b: &LookupSize) -> std::cmp::Ordering {
	let per_byte_a = f64::from(a.num_subtables) / a.size as f64;
	let per_byte_b = f64::from(b.num_subtables) / b.size as f64;
	if per_byte_a == per_byte_b {
		return b.lookup_index.cmp(&a.lookup_index);
	}
	(per_byte_b - per_byte_a)
		.partial_cmp(&0.0)
		.unwrap_or(std::cmp::Ordering::Equal)
}

/// `_presplit_subtables_if_needed`: splits the subtables at risk of
/// overflowing, where splitting is supported.
fn presplit_subtables_if_needed(c: &mut Context, g: &mut Graph) -> Result<()> {
	// New subtables can be added to the lookups, so iterate over the initial set.
	let lookup_indices: Vec<u32> = c.lookups.iter().copied().collect();
	for lookup_index in lookup_indices {
		gsubgpos::split_subtables_if_needed(c, g, lookup_index)?;
	}
	Ok(())
}

/// `_promote_extensions_if_needed`: keeps as many lookups as fit in 64K as
/// non-extension lookups (greedily, the most subtables per byte first) and
/// promotes the rest.
fn promote_extensions_if_needed(c: &mut Context, g: &mut Graph) -> Result<()> {
	if c.lookups.is_empty() {
		return Ok(());
	}
	let mut total_lookup_table_sizes = 0usize;
	let mut lookup_sizes = Vec::with_capacity(c.lookups.len());
	let lookups: Vec<u32> = c.lookups.iter().copied().collect();
	for lookup_index in lookups {
		total_lookup_table_sizes += g.v(lookup_index).table_size();
		let mut visited = BTreeSet::new();
		lookup_sizes.push(LookupSize {
			lookup_index,
			size: g
				.find_subgraph_size(lookup_index, &mut visited, u32::MAX)
				.unwrap_or(0),
			num_subtables: gsubgpos::number_of_subtables(g, lookup_index),
		});
	}
	lookup_sizes.sort_by(cmp_lookup_size);

	let lookup_list_size = g.v(c.lookup_list_index).table_size();
	// Lookup list and lookups.
	let l2_l3_size = lookup_list_size + total_lookup_table_sizes;
	// Lookups and subtables.
	let mut l3_l4_size = total_lookup_table_sizes;
	// Subtables and their descendants.
	let mut l4_plus_size = 0usize;

	// Start by assuming all lookups use extension subtables; the size is removed
	// for the lookups that are not made extensions.
	for p in &lookup_sizes {
		let subtables_size = p.num_subtables as usize * 8;
		l3_l4_size += subtables_size;
		l4_plus_size += subtables_size;
	}

	let mut layers_full = false;
	for p in &lookup_sizes {
		if gsubgpos::is_extension(g, p.lookup_index, c.table_tag) {
			// Already counted by the loop above.
			continue;
		}
		if !layers_full {
			let lookup_size = g.v(p.lookup_index).table_size();
			let mut visited = BTreeSet::new();
			let subtables_size = g
				.find_subgraph_size(p.lookup_index, &mut visited, 1)
				.unwrap_or(0)
				.wrapping_sub(lookup_size);
			let remaining_size = p
				.size
				.wrapping_sub(subtables_size)
				.wrapping_sub(lookup_size);
			l3_l4_size = l3_l4_size.wrapping_add(subtables_size);
			l3_l4_size = l3_l4_size.wrapping_sub(p.num_subtables as usize * 8);
			l4_plus_size = l4_plus_size
				.wrapping_add(subtables_size)
				.wrapping_add(remaining_size);
			if l2_l3_size < 1 << 16 && l3_l4_size < 1 << 16 && l4_plus_size < 1 << 16 {
				// This lookup fits within all layer groups.
				continue;
			}
			layers_full = true;
		}
		gsubgpos::make_extension(c, g, p.lookup_index)?;
	}
	Ok(())
}

/// `_try_isolating_subgraphs`: moves up to half of the roots of the space of
/// the overflows to a new space.
fn try_isolating_subgraphs(overflows: &[OverflowRecord], g: &mut Graph) -> Result<bool> {
	let mut space = 0;
	let mut roots_to_isolate = BTreeSet::new();
	for r in overflows.iter().rev() {
		let (overflow_space, root) = g.space_for(r.parent);
		if overflow_space == 0 {
			continue;
		}
		if g.num_roots_for_space(overflow_space) <= 1 {
			continue;
		}
		if space == 0 {
			space = overflow_space;
		}
		if space == overflow_space {
			roots_to_isolate.insert(root);
		}
	}
	if roots_to_isolate.is_empty() {
		return Ok(false);
	}
	let maximum_to_move = (g.num_roots_for_space(space) / 2).max(1);
	if roots_to_isolate.len() > maximum_to_move as usize {
		// Only move at most half of the roots of a space at a time, removing the
		// others in topological order.
		let mut extra = roots_to_isolate.len() - maximum_to_move as usize;
		for &id in &g.ordering {
			if extra == 0 {
				break;
			}
			if roots_to_isolate.remove(&id) {
				extra -= 1;
			}
		}
	}
	g.isolate_subgraph(&mut roots_to_isolate)?;
	g.move_to_new_space(&roots_to_isolate)?;
	Ok(true)
}

/// `_resolve_shared_overflow`: duplicates a shared child for the parents whose
/// links to it overflow.
fn resolve_shared_overflow(
	overflows: &[OverflowRecord],
	overflow_index: usize,
	g: &mut Graph,
) -> Result<bool> {
	let r = overflows[overflow_index];
	let mut parents = BTreeSet::from([r.parent]);
	for r2 in overflows[..overflow_index].iter().rev() {
		if r2.child == r.child {
			parents.insert(r2.parent);
		}
	}
	let mut result = g.duplicate_for_parents(&parents, r.child);
	if result.is_err() && parents.len() > 2 {
		// All links to the child overflow, so not all of them can move to the
		// duplicate: leave out the lowest index parent, the closest to the child.
		parents.pop_first();
		result = g.duplicate_for_parents(&parents, r.child);
	}
	let Ok(clone) = result else {
		return Ok(false);
	};
	if parents.len() > 1 {
		// Packs the duplicate close to its parents, which are likely in the same
		// layer of the graph.
		g.vm(clone).give_max_priority();
	}
	Ok(true)
}

/// `_process_overflows`.
fn process_overflows(
	overflows: &[OverflowRecord],
	priority_bumped_parents: &mut BTreeSet<u32>,
	g: &mut Graph,
) -> Result<bool> {
	let mut resolution_attempted = false;
	// The furthest overflows first.
	for i in (0..overflows.len()).rev() {
		let r = overflows[i];
		if g.v(r.child).is_shared() && resolve_shared_overflow(overflows, i, g)? {
			return Ok(true);
		}
		if g.v(r.child).is_leaf() && !priority_bumped_parents.contains(&r.parent) {
			// The object is too far from its parent: move it closer.
			if g.raise_childrens_priority(r.parent)? {
				priority_bumped_parents.insert(r.parent);
				resolution_attempted = true;
			}
			continue;
		}
	}
	Ok(resolution_attempted)
}

/// `_assign_spaces_and_sort`.
fn assign_spaces_and_sort(g: &mut Graph) -> Result<()> {
	if g.assign_spaces()? {
		g.sort_shortest_distance()
	} else {
		g.sort_shortest_distance_if_needed()
	}
}

/// `_gsub_gpos_specialization`.
fn gsub_gpos_specialization(
	table_tag: [u8; 4],
	always_recalculate_extensions: bool,
	g: &mut Graph,
) -> Result<()> {
	if !always_recalculate_extensions {
		return assign_spaces_and_sort(g);
	}
	let mut c = match Context::create(table_tag, g) {
		Ok(c) => c,
		// Splitting and extension promotion need a valid GSUB/GPOS; they are
		// skipped otherwise.
		Err(Error::SanitizeFailure) => return assign_spaces_and_sort(g),
		Err(e) => return Err(e),
	};
	presplit_subtables_if_needed(&mut c, g)?;
	promote_extensions_if_needed(&mut c, g)?;
	// `assign_spaces` needs a correct topological ordering to find the roots.
	g.sort_shortest_distance_if_needed()?;
	if !will_overflow(g, None) {
		return Ok(());
	}
	assign_spaces_and_sort(g)
}

/// `hb_resolve_graph_overflows`.
fn resolve_graph_overflows(
	table_tag: [u8; 4],
	max_rounds: u32,
	always_recalculate_extensions: bool,
	g: &mut Graph,
) -> Result<()> {
	g.sort_shortest_distance()?;
	if !will_overflow(g, None) {
		return Ok(());
	}
	let is_gsub_or_gpos = table_tag == GPOS || table_tag == GSUB;
	if is_gsub_or_gpos {
		gsub_gpos_specialization(table_tag, always_recalculate_extensions, g)?;
	}
	let mut round = 0;
	let mut total_iterations = 0;
	let mut overflows = Vec::new();
	while round < max_rounds && total_iterations < MAX_ITERATIONS {
		if !will_overflow(g, Some(&mut overflows)) {
			break;
		}
		log::debug!(
			"overflow resolution round {round}: {} overflows",
			overflows.len()
		);
		total_iterations += 1;
		let mut priority_bumped_parents = BTreeSet::new();
		if !try_isolating_subgraphs(&overflows, g)? {
			round += 1;
			if !process_overflows(&overflows, &mut priority_bumped_parents, g)? {
				log::debug!("no resolution available");
				break;
			}
		}
		g.sort_shortest_distance()?;
	}
	if will_overflow(g, None) {
		if is_gsub_or_gpos && !always_recalculate_extensions {
			// As a last resort, run again with extension promotion and splitting.
			return resolve_graph_overflows(table_tag, max_rounds, true, g);
		}
		return Err(Error::OverflowResolutionFailed);
	}
	Ok(())
}

/// `hb_resolve_overflows`.
pub(crate) fn resolve_overflows(
	objects: Vec<Object>,
	table_tag: [u8; 4],
	max_rounds: u32,
	recalculate_extensions: bool,
) -> Result<Vec<u8>> {
	let mut g = Graph::create(objects)?;
	resolve_graph_overflows(table_tag, max_rounds, recalculate_extensions, &mut g)?;
	serialize::serialize(&mut g)
}
