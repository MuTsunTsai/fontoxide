//! Port of the HarfBuzz repacker (`hb-repacker.hh` and `src/graph/`), as
//! fontTools uses it through uharfbuzz's `serialize_with_tag` to pack `GSUB`
//! and `GPOS` tables.
//!
//! A table is given as a graph of objects (subtables) linked by offsets. The
//! repacker orders the objects by their distance from the root and, if some
//! offsets would overflow, resolves that by moving, duplicating and isolating
//! objects, and for `GSUB`/`GPOS` by splitting subtables and promoting lookups
//! to Extension lookups. The output is byte-identical to the HarfBuzz version
//! bundled with uharfbuzz 0.57.0 (HarfBuzz 15.0.0).
//!
//! Every data structure whose iteration order affects the result follows
//! HarfBuzz exactly: `hb_hashmap_t` (`hb_map`), `hb_priority_queue_t`
//! (`priority_queue`) and the sorted `hb_set_t` (a `BTreeSet`).
//!
//! | fontoxide           | HarfBuzz                                     |
//! |---------------------|----------------------------------------------|
//! | `graph`             | `graph/graph.hh`                             |
//! | `serialize`         | `graph/serialize.hh`                         |
//! | `repacker`          | `hb-repacker.hh`                             |
//! | `gsubgpos`          | `graph/gsubgpos-graph.hh`, `gsubgpos-context`|
//! | `layout`            | `graph/coverage-graph.hh`, `classdef-graph.hh` and the serializers of `Coverage` and `ClassDef` |
//! | `pairpos`           | `graph/pairpos-graph.hh`                     |
//! | `markbasepos`       | `graph/markbasepos-graph.hh`                 |
//! | `ligature`          | `graph/ligature-graph.hh`                    |

#![forbid(unsafe_code)]

mod graph;
mod gsubgpos;
mod hb_map;
mod layout;
mod ligature;
mod markbasepos;
mod pairpos;
mod priority_queue;
mod repacker;
mod serialize;

/// An error of the repacker (`graph_error_t`). fontTools turns any of them into
/// `hb.RepackerError` and falls back to its own serializer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
	#[allow(missing_docs)]
	#[error("Memory Allocation Failed")]
	AllocationFailure,
	#[allow(missing_docs)]
	#[error("Limit Exceeded")]
	LimitExceeded,
	#[allow(missing_docs)]
	#[error("Invalid Argument")]
	InvalidArgument,
	#[allow(missing_docs)]
	#[error("Cycle in Graph")]
	CycleDetected,
	#[allow(missing_docs)]
	#[error("Failed Sanitization")]
	SanitizeFailure,
	#[allow(missing_docs)]
	#[error("Out of Bounds")]
	OutOfBounds,
	#[allow(missing_docs)]
	#[error("Graph is not fully connected")]
	OrphanedNodes,
	#[allow(missing_docs)]
	#[error("Overflows are not able to be resolved")]
	OverflowResolutionFailed,
}

/// A `Result` of the repacker.
pub type Result<T> = std::result::Result<T, Error>;

/// An offset from one object to another (`hb_subset_serialize_link_t`). Links
/// are unsigned and relative to the start of the parent object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
	/// The size of the offset in bytes: 2, 3 or 4; 0 for a virtual link, which
	/// only requires the child to be packed after the parent.
	pub width: u8,
	/// The position of the offset in the parent object.
	pub position: u32,
	/// The index of the child object.
	pub objidx: u32,
}

/// An object of the graph (`hb_subset_serialize_object_t`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Object {
	/// The object's bytes; the offsets in them are ignored.
	pub data: Vec<u8>,
	/// The offsets in the object.
	pub real_links: Vec<Link>,
	/// Ordering constraints: these children are packed after the object.
	pub virtual_links: Vec<Link>,
}

/// Packs a table without overflowing offsets
/// (`hb_subset_serialize_or_fail`, uharfbuzz's `serialize_with_tag`).
///
/// `objects` are in packing order, children before their parents, and the last
/// one is the root; links refer to objects by their index in the list. `tag` is
/// the table tag, which enables the `GSUB`/`GPOS` specific steps.
pub fn serialize_with_tag(tag: [u8; 4], objects: Vec<Object>) -> Result<Vec<u8>> {
	if objects.is_empty() {
		return Err(Error::InvalidArgument);
	}
	repacker::resolve_overflows(objects, tag, 20, true)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn obj(data: &[u8], links: &[(u8, u32, u32)]) -> Object {
		Object {
			data: data.to_vec(),
			real_links: links
				.iter()
				.map(|&(width, position, objidx)| Link {
					width,
					position,
					objidx,
				})
				.collect(),
			virtual_links: Vec::new(),
		}
	}

	#[test]
	fn simple() -> Result<()> {
		// root -> (a, b); a -> c; b -> c.
		let objects = vec![
			obj(&[0xCC], &[]),
			obj(&[0xAA, 0, 0], &[(2, 1, 0)]),
			obj(&[0xBB, 0, 0], &[(2, 1, 0)]),
			obj(&[0, 0, 0, 0], &[(2, 0, 1), (2, 2, 2)]),
		];
		let out = serialize_with_tag(*b"    ", objects)?;
		// The root, then a and b by distance, then the shared c.
		assert_eq!(out, [0, 4, 0, 7, 0xAA, 0, 6, 0xBB, 0, 3, 0xCC]);
		Ok(())
	}

	#[test]
	fn orphan() {
		let objects = vec![obj(&[1], &[]), obj(&[2], &[])];
		assert_eq!(
			serialize_with_tag(*b"    ", objects),
			Err(Error::OrphanedNodes)
		);
	}
}
