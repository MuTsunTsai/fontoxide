//! `graph/serialize.hh`: overflow detection and serialization of a graph.

use std::collections::HashSet;

use crate::graph::Graph;
use crate::{Error, Link, Result};

/// An overflowing link (`overflow_record_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OverflowRecord {
	pub parent: u32,
	pub child: u32,
}

fn compute_offset(graph: &Graph, parent_idx: u32, link: &Link) -> i64 {
	i64::from(graph.v(link.objidx).start) - i64::from(graph.v(parent_idx).start)
}

fn is_valid_offset(offset: i64, link: &Link) -> bool {
	match link.width {
		// Virtual links can't overflow.
		0 => offset >= 0,
		4 => (0..1 << 32).contains(&offset),
		3 => (0..1 << 24).contains(&offset),
		_ => (0..1 << 16).contains(&offset),
	}
}

/// `will_overflow`: whether any offset overflows in the current ordering. With
/// `overflows`, collects the overflowing links (once per parent and child).
pub(crate) fn will_overflow(
	graph: &mut Graph,
	mut overflows: Option<&mut Vec<OverflowRecord>>,
) -> bool {
	if let Some(o) = overflows.as_deref_mut() {
		o.clear();
	}
	graph.update_positions();
	let mut record_set = HashSet::new();
	for &parent_idx in &graph.ordering {
		// Virtual links don't need to be checked.
		for link in &graph.v(parent_idx).real_links {
			let offset = compute_offset(graph, parent_idx, link);
			if is_valid_offset(offset, link) {
				continue;
			}
			let Some(o) = overflows.as_deref_mut() else {
				return true;
			};
			let r = OverflowRecord {
				parent: parent_idx,
				child: link.objidx,
			};
			if record_set.insert(r) {
				o.push(r);
			}
		}
	}
	overflows.is_some_and(|o| !o.is_empty())
}

/// `serialize`: the objects in the current ordering, with the offsets filled
/// in.
pub(crate) fn serialize(graph: &mut Graph) -> Result<Vec<u8>> {
	let size = graph.total_size_in_bytes();
	if size == 0 {
		return Ok(Vec::new());
	}
	// Positions in the output are the same as those of the ordering.
	graph.update_positions();
	let mut out = Vec::with_capacity(size);
	for &i in &graph.ordering {
		let v = graph.v(i);
		let start = out.len();
		out.extend_from_slice(&v.data);
		for link in &v.real_links {
			let width = link.width as usize;
			let position = link.position as usize;
			if position + width > v.table_size() {
				return Err(Error::AllocationFailure);
			}
			let field = &mut out[start + position..start + position + width];
			field.fill(0);
			let child = graph.v(link.objidx);
			if child.table_size() == 0 {
				// An empty object is not packed; the offset stays 0.
				continue;
			}
			let offset = child.start.wrapping_sub(v.start);
			let bytes = offset.to_be_bytes();
			match width {
				2 if offset < 1 << 16 => field.copy_from_slice(&bytes[2..]),
				3 if offset < 1 << 24 => field.copy_from_slice(&bytes[1..]),
				4 => field.copy_from_slice(&bytes),
				_ => return Err(Error::OverflowResolutionFailed),
			}
		}
	}
	Ok(out)
}
