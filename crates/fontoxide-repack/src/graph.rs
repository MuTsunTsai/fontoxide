//! `graph_t` (`graph/graph.hh`): a serialized table as a graph of objects,
//! with the operations the repacker uses to reorder and restructure it.

use std::collections::BTreeSet;

use crate::hb_map::HbMap;
use crate::priority_queue::PriorityQueue;
use crate::{Error, Link, Object, Result};

/// `HB_GRAPH_INVALID`.
pub(crate) const INVALID: u32 = u32::MAX;
/// `HB_REPACKER_MAX_VERTICES`.
const MAX_VERTICES: usize = 800_000;
/// `HB_REPACKER_MAX_SPACES`.
const MAX_SPACES: usize = 8000;

/// `hb_set_t` after `invert ()`: a set that can also hold "everything except".
#[derive(Debug, Default)]
pub(crate) struct Visited {
	set: BTreeSet<u32>,
	inverted: bool,
}

impl Visited {
	pub fn has(&self, v: u32) -> bool {
		self.set.contains(&v) != self.inverted
	}

	pub fn add(&mut self, v: u32) {
		if self.inverted {
			self.set.remove(&v);
		} else {
			self.set.insert(v);
		}
	}

	pub fn invert(&mut self) {
		self.inverted = !self.inverted;
	}
}

/// `vertex_t`.
#[derive(Debug, Clone)]
pub(crate) struct Vertex {
	pub distance: i64,
	pub space: u32,
	pub start: u32,
	pub end: u32,
	pub priority: u32,
	/// The object's bytes (`obj_.head` to `obj_.tail`).
	pub data: Vec<u8>,
	pub real_links: Vec<Link>,
	pub virtual_links: Vec<Link>,
	incoming_edges: u32,
	single_parent: u32,
	has_incoming_virtual_edges: bool,
	parents: HbMap,
	virtual_parents: BTreeSet<u32>,
}

impl Vertex {
	fn new(data: Vec<u8>) -> Self {
		Vertex {
			distance: 0,
			space: 0,
			start: 0,
			end: 0,
			priority: 0,
			data,
			real_links: Vec::new(),
			virtual_links: Vec::new(),
			incoming_edges: 0,
			single_parent: INVALID,
			has_incoming_virtual_edges: false,
			parents: HbMap::default(),
			virtual_parents: BTreeSet::new(),
		}
	}

	pub fn table_size(&self) -> usize {
		self.data.len()
	}

	/// `parents_iter`: the single parent, or the parents in map order.
	pub fn parents_iter(&self) -> impl Iterator<Item = u32> + '_ {
		(self.single_parent != INVALID)
			.then_some(self.single_parent)
			.into_iter()
			.chain(self.parents.keys())
	}

	fn first_parent(&self) -> Option<u32> {
		self.parents_iter().next()
	}

	/// `shrink_buffer`.
	pub fn shrink_buffer(&mut self, count: usize) {
		let len = self.data.len().saturating_sub(count);
		self.data.truncate(len);
	}

	/// `sort_real_links`: by position, then object index.
	pub fn sort_real_links(&mut self) {
		self.real_links
			.sort_by(|a, b| a.position.cmp(&b.position).then(a.objidx.cmp(&b.objidx)));
	}

	pub fn add_real_link(&mut self, width: u8, objidx: u32, position: u32) {
		self.real_links.push(Link {
			width,
			position,
			objidx,
		});
	}

	pub fn add_virtual_link(&mut self, objidx: u32) {
		self.virtual_links.push(Link {
			width: 0,
			position: 0,
			objidx,
		});
	}

	pub fn has_incoming_virtual_edges(&self) -> bool {
		self.has_incoming_virtual_edges
	}

	fn link_positions_valid(&self, num_objects: usize) -> Result<()> {
		let mut assigned = BTreeSet::new();
		for l in &self.real_links {
			if l.objidx as usize >= num_objects {
				return Err(Error::InvalidArgument);
			}
			if !(2..=4).contains(&l.width) {
				return Err(Error::InvalidArgument);
			}
			let start = l.position as usize;
			let end = start + l.width as usize - 1;
			if end >= self.table_size() {
				return Err(Error::OutOfBounds);
			}
			if assigned.range(start..=end).next().is_some() {
				return Err(Error::InvalidArgument);
			}
			assigned.extend(start..=end);
		}
		Ok(())
	}

	/// `position_to_index_map`.
	pub fn position_to_index_map(&self) -> std::collections::HashMap<u32, u32> {
		self.real_links
			.iter()
			.map(|l| (l.position, l.objidx))
			.collect()
	}

	pub fn is_shared(&self) -> bool {
		self.parents.population() as usize > self.virtual_parents.len() + 1
	}

	pub fn incoming_edges(&self) -> u32 {
		self.incoming_edges
	}

	fn incoming_edges_from_parent(&self, parent_index: u32) -> u32 {
		if self.single_parent != INVALID {
			return u32::from(self.single_parent == parent_index);
		}
		self.parents.get(parent_index).unwrap_or(0)
	}

	fn reset_parents(&mut self) {
		self.incoming_edges = 0;
		self.has_incoming_virtual_edges = false;
		self.single_parent = INVALID;
		self.parents.clear();
		self.virtual_parents.clear();
	}

	fn to_result(&self) -> Result<()> {
		if self.parents.in_error() {
			return Err(Error::AllocationFailure);
		}
		Ok(())
	}

	pub fn add_parent(&mut self, parent_index: u32, is_virtual: bool) -> Result<()> {
		self.has_incoming_virtual_edges |= is_virtual;
		if is_virtual {
			self.virtual_parents.insert(parent_index);
		}
		if self.incoming_edges == 0 {
			self.single_parent = parent_index;
			self.incoming_edges = 1;
			return self.to_result();
		} else if self.single_parent != INVALID {
			if !self.parents.set(self.single_parent, 1) {
				return self.to_result();
			}
			self.single_parent = INVALID;
		}
		if let Some(v) = self.parents.get_mut(parent_index) {
			*v += 1;
			self.incoming_edges += 1;
		} else if self.parents.set(parent_index, 1) {
			self.incoming_edges += 1;
		}
		self.to_result()
	}

	pub fn remove_parent(&mut self, parent_index: u32) {
		if parent_index == self.single_parent {
			self.single_parent = INVALID;
			self.incoming_edges -= 1;
			self.virtual_parents.clear();
			return;
		}
		if let Some(v) = self.parents.get(parent_index) {
			self.incoming_edges -= 1;
			if v > 1 {
				if let Some(v) = self.parents.get_mut(parent_index) {
					*v -= 1;
				}
			} else {
				self.parents.del(parent_index);
				self.virtual_parents.remove(&parent_index);
			}
			if self.incoming_edges == 1 {
				self.single_parent = self.parents.keys().next().unwrap_or(INVALID);
				self.parents.clear();
			}
		}
	}

	/// `remove_real_link_unordered (child_index, offset)`.
	fn remove_real_link_unordered(&mut self, child_index: u32, position: u32) {
		if let Some(i) = self
			.real_links
			.iter()
			.position(|l| l.objidx == child_index && l.position == position)
		{
			self.real_links.swap_remove(i);
		}
	}

	pub fn is_leaf(&self) -> bool {
		self.real_links.is_empty() && self.virtual_links.is_empty()
	}

	fn raise_priority(&mut self) -> bool {
		if self.has_max_priority() {
			return false;
		}
		self.priority += 1;
		true
	}

	pub fn give_max_priority(&mut self) -> bool {
		let mut result = false;
		while !self.has_max_priority() {
			result = true;
			self.priority += 1;
		}
		result
	}

	fn has_max_priority(&self) -> bool {
		self.priority >= 3
	}

	fn modified_distance(&self, order: u32) -> i64 {
		let mut modified =
			(self.distance.saturating_add(self.distance_modifier())).clamp(0, i64::MAX >> 18);
		if self.has_max_priority() {
			modified = 0;
		}
		(modified << 18) | (0x3FFFF & i64::from(order))
	}

	fn distance_modifier(&self) -> i64 {
		if self.priority == 0 {
			return 0;
		}
		let table_size = self.table_size() as i64;
		if self.priority == 1 {
			return -table_size / 2;
		}
		-table_size
	}

	/// The links, real then virtual (`all_links`), with whether each is virtual.
	fn all_links(&self) -> impl Iterator<Item = (Link, bool)> + '_ {
		self.real_links
			.iter()
			.map(|l| (*l, false))
			.chain(self.virtual_links.iter().map(|l| (*l, true)))
	}
}

/// `graph_t`.
#[derive(Debug)]
pub(crate) struct Graph {
	pub vertices: Vec<Vertex>,
	/// The topological ordering: `ordering[pos]` is an object index.
	pub ordering: Vec<u32>,
	parents_invalid: bool,
	distance_invalid: bool,
	positions_invalid: bool,
	num_roots_for_space: Vec<u32>,
}

impl Graph {
	/// `graph_t::create`: the objects in packing order, the root last.
	pub fn create(objects: Vec<Object>) -> Result<Graph> {
		if objects.len() > MAX_VERTICES {
			return Err(Error::LimitExceeded);
		}
		let count = objects.len();
		let mut g = Graph {
			vertices: Vec::with_capacity(count),
			ordering: vec![0; count],
			parents_invalid: true,
			distance_invalid: true,
			positions_invalid: true,
			num_roots_for_space: vec![1],
		};
		for (i, obj) in objects.into_iter().enumerate() {
			let mut v = Vertex::new(obj.data);
			v.real_links = obj.real_links;
			v.virtual_links = obj.virtual_links;
			v.link_positions_valid(count)?;
			g.vertices.push(v);
			// Objects are provided in reverse order (the last one is the root).
			g.ordering[count - 1 - i] = i as u32;
		}
		g.is_fully_connected()?;
		Ok(g)
	}

	pub fn root_idx(&self) -> u32 {
		self.ordering[0]
	}

	fn root(&self) -> &Vertex {
		&self.vertices[self.root_idx() as usize]
	}

	pub fn v(&self, i: u32) -> &Vertex {
		&self.vertices[i as usize]
	}

	pub fn vm(&mut self, i: u32) -> &mut Vertex {
		&mut self.vertices[i as usize]
	}

	pub fn len(&self) -> u32 {
		self.vertices.len() as u32
	}

	/// `add_link`: a 16-bit link from `parent_id` at `position` to `child_id`.
	pub fn add_link(&mut self, position: u32, parent_id: u32, child_id: u32) -> Result<()> {
		if parent_id >= self.len() || child_id >= self.len() {
			return Err(Error::OutOfBounds);
		}
		self.vm(parent_id).add_real_link(2, child_id, position);
		self.vm(child_id).add_parent(parent_id, false)
	}

	pub fn sort_shortest_distance_if_needed(&mut self) -> Result<()> {
		if !self.positions_invalid {
			return Ok(());
		}
		self.sort_shortest_distance()
	}

	/// `sort_shortest_distance`: a new topological ordering, by distance from the
	/// root.
	pub fn sort_shortest_distance(&mut self) -> Result<()> {
		self.positions_invalid = true;
		if self.vertices.len() <= 1 {
			return Ok(());
		}
		self.update_distances()?;
		let n = self.vertices.len();
		let mut queue = PriorityQueue::with_capacity(n);
		let mut new_ordering = vec![0u32; n];
		let mut removed_edges = vec![0u32; n];
		self.update_parents()?;
		let root = self.root_idx();
		queue.insert(self.v(root).modified_distance(0), root);
		let mut order = 1u32;
		let mut pos = 0;
		while let Some((_, next_id)) = queue.pop_minimum() {
			if pos >= n {
				// A node was visited more than once: the graph has a cycle.
				return Err(Error::CycleDetected);
			}
			new_ordering[pos] = next_id;
			pos += 1;
			let links: Vec<(Link, bool)> = self.v(next_id).all_links().collect();
			for (link, _) in links {
				if link.objidx as usize >= n {
					return Err(Error::OutOfBounds);
				}
				removed_edges[link.objidx as usize] += 1;
				let v = self.v(link.objidx);
				if v
					.incoming_edges()
					.wrapping_sub(removed_edges[link.objidx as usize])
					== 0
				{
					// The order links are encountered in breaks ties between equal
					// distances.
					queue.insert(v.modified_distance(order), link.objidx);
					order = order.wrapping_add(1);
				}
			}
		}
		self.ordering = new_ordering;
		if pos != n {
			return Err(Error::OrphanedNodes);
		}
		Ok(())
	}

	/// `find_space_roots`.
	fn find_space_roots(&mut self, visited: &mut Visited, roots: &mut BTreeSet<u32>) -> Result<()> {
		let root_index = self.root_idx();
		for idx in 0..self.ordering.len() {
			let i = self.ordering[idx];
			if visited.has(i) {
				continue;
			}
			let links = self.v(i).real_links.clone();
			for l in links {
				if l.width < 3 {
					continue;
				}
				if i == root_index && l.width == 3 {
					// Skip the single 24-bit offset of the root to the lookup list.
					continue;
				}
				// A wide offset forms a root, unless there are 32-bit offsets in its
				// subgraph: then those are the roots.
				let mut sub_roots = BTreeSet::new();
				self.find_32bit_roots(l.objidx, &mut sub_roots)?;
				if !sub_roots.is_empty() {
					for sub_root in sub_roots {
						roots.insert(sub_root);
						self.find_subgraph_visited(sub_root, visited)?;
					}
					continue;
				}
				roots.insert(l.objidx);
				self.find_subgraph_visited(l.objidx, visited)?;
			}
		}
		Ok(())
	}

	/// `index_for_offset`: the object linked from `node_idx` at `position`.
	pub fn index_for_offset(&self, node_idx: u32, position: u32) -> Result<u32> {
		if node_idx >= self.len() {
			return Err(Error::OutOfBounds);
		}
		let node = self.v(node_idx);
		if position as usize >= node.table_size() {
			return Err(Error::OutOfBounds);
		}
		node.real_links
			.iter()
			.find(|l| l.position == position)
			.map(|l| l.objidx)
			.ok_or(Error::InvalidArgument)
	}

	/// `mutable_index_for_offset`: like [`Graph::index_for_offset`], but
	/// duplicates the child first if other parents link to it.
	pub fn mutable_index_for_offset(&mut self, node_idx: u32, position: u32) -> Result<u32> {
		let child_idx = self.index_for_offset(node_idx, position)?;
		if child_idx >= self.len() {
			return Err(Error::OutOfBounds);
		}
		if self.v(child_idx).parents_iter().any(|p| p != node_idx) {
			return self.duplicate_for_parent(node_idx, child_idx);
		}
		Ok(child_idx)
	}

	/// `assign_spaces`: gives each connected subgraph below 24 or 32-bit offsets
	/// its own space.
	pub fn assign_spaces(&mut self) -> Result<bool> {
		self.update_parents()?;
		for v in &mut self.vertices {
			v.space = 0;
			v.priority = 0;
		}
		self.num_roots_for_space = vec![1];
		self.distance_invalid = true;
		self.positions_invalid = true;

		let mut visited = Visited::default();
		let mut roots = BTreeSet::new();
		self.find_space_roots(&mut visited, &mut roots)?;
		// Everything outside the subgraphs of the roots is marked as visited, so
		// subgraphs cannot connect through them.
		visited.invert();
		if roots.is_empty() {
			return Ok(false);
		}
		while let Some(&next) = roots.first() {
			let mut connected_roots = BTreeSet::new();
			self.find_connected_nodes(next, &mut roots, &mut visited, &mut connected_roots)?;
			self.isolate_subgraph(&mut connected_roots)?;
			let next_space = self.next_space();
			if next_space as usize >= MAX_SPACES {
				return Err(Error::LimitExceeded);
			}
			self.num_roots_for_space.push(0);
			for root in connected_roots {
				self.vm(root).space = next_space;
				self.num_roots_for_space[next_space as usize] += 1;
				self.distance_invalid = true;
				self.positions_invalid = true;
			}
		}
		Ok(true)
	}

	/// `isolate_subgraph`: duplicates the nodes reachable from `roots` that are
	/// also linked from outside, so that the subgraph is only reachable through
	/// the roots. `roots` is updated with the new indices.
	pub fn isolate_subgraph(&mut self, roots: &mut BTreeSet<u32>) -> Result<bool> {
		self.update_parents()?;
		let mut subgraph = HbMap::default();
		// The incoming edges of the roots are all wide, so not de-duplicated.
		let mut parents = BTreeSet::new();
		for &root_idx in roots.iter() {
			if root_idx >= self.len() {
				return Err(Error::OutOfBounds);
			}
			let wide = self.wide_parents(root_idx, &mut parents);
			subgraph.set(root_idx, wide);
		}
		for &root_idx in roots.iter() {
			self.find_subgraph_counts(root_idx, &mut subgraph)?;
		}
		if subgraph.in_error() {
			return Err(Error::AllocationFailure);
		}
		let mut index_map = HbMap::default();
		let mut made_changes = false;
		let entries: Vec<(u32, u32)> = subgraph.iter().collect();
		for (node, subgraph_incoming_edges) in entries {
			if subgraph_incoming_edges < self.v(node).incoming_edges() {
				// Only nodes with links from outside the subgraph are duplicated.
				made_changes = true;
				self.duplicate_subgraph(node, &mut index_map)?;
			}
		}
		if index_map.in_error() {
			return Err(Error::AllocationFailure);
		}
		if !made_changes {
			return Ok(false);
		}
		let new_subgraph: Vec<u32> = subgraph
			.keys()
			.map(|k| index_map.get(k).unwrap_or(k))
			.collect();
		self.remap_obj_indices(&index_map, &new_subgraph, false)?;
		let parents: Vec<u32> = parents.into_iter().collect();
		self.remap_obj_indices(&index_map, &parents, true)?;
		let old_roots: Vec<u32> = roots.iter().copied().collect();
		for next in old_roots {
			if let Some(v) = index_map.get(next) {
				roots.remove(&next);
				roots.insert(v);
			}
		}
		Ok(true)
	}

	/// `traverse_directed_bfs`: calls `visit_edge (graph, parent, link, child,
	/// depth)` for the start node and each traversed edge; returning false stops
	/// the traversal from going into the child's edges.
	fn traverse_directed_bfs<F>(&mut self, start_idx: u32, mut visit_edge: F) -> Result<()>
	where
		F: FnMut(&mut Graph, u32, Option<&Link>, u32, u32) -> bool,
	{
		if start_idx >= self.len() {
			return Err(Error::OutOfBounds);
		}
		let capacity = self.vertices.len();
		if !visit_edge(self, INVALID, None, start_idx, 0) {
			return Ok(());
		}
		let mut queue = Vec::with_capacity(capacity);
		queue.push(start_idx);
		let mut head = 0;
		let mut depth = 0;
		while head < queue.len() {
			let level_end = queue.len();
			while head < level_end {
				let node_idx = queue[head];
				head += 1;
				let links: Vec<Link> = self
					.v(node_idx)
					.real_links
					.iter()
					.chain(&self.v(node_idx).virtual_links)
					.copied()
					.collect();
				for link in links {
					if !visit_edge(self, node_idx, Some(&link), link.objidx, depth + 1) {
						continue;
					}
					if queue.len() >= capacity {
						return Err(Error::CycleDetected);
					}
					queue.push(link.objidx);
				}
			}
			depth += 1;
		}
		Ok(())
	}

	/// `find_subgraph (node_idx, hb_map_t&)`: counts the incoming edges of the
	/// nodes below `node_idx`.
	fn find_subgraph_counts(&mut self, node_idx: u32, subgraph: &mut HbMap) -> Result<()> {
		self.traverse_directed_bfs(node_idx, |_, _, _, child, depth| {
			if depth == 0 {
				return true;
			}
			if let Some(count) = subgraph.get_mut(child) {
				*count += 1;
				return false;
			}
			subgraph.set(child, 1);
			true
		})
	}

	/// `find_subgraph (node_idx, hb_set_t&)`.
	fn find_subgraph_visited(&mut self, node_idx: u32, subgraph: &mut Visited) -> Result<()> {
		self.traverse_directed_bfs(node_idx, |_, _, _, child, _| {
			if subgraph.has(child) {
				return false;
			}
			subgraph.add(child);
			true
		})
	}

	/// `find_subgraph_size`: the total size of the nodes reachable from
	/// `node_idx` (down to `max_depth`) that are not in `subgraph`.
	pub fn find_subgraph_size(
		&mut self,
		node_idx: u32,
		subgraph: &mut BTreeSet<u32>,
		max_depth: u32,
	) -> Result<usize> {
		let mut size = 0;
		self.traverse_directed_bfs(node_idx, |g, _, _, child, depth| {
			if !subgraph.insert(child) {
				return false;
			}
			size += g.v(child).table_size();
			depth < max_depth
		})?;
		Ok(size)
	}

	/// `find_32bit_roots`: the topmost children of 32-bit offsets below
	/// `node_idx`.
	fn find_32bit_roots(&mut self, node_idx: u32, found: &mut BTreeSet<u32>) -> Result<()> {
		// A breadth first traversal, so that nothing is reached through both 32-bit
		// and narrower links.
		let mut visited = BTreeSet::new();
		self.traverse_directed_bfs(node_idx, |_, parent, link, child, _| {
			if link.is_some() && found.contains(&parent) {
				return false;
			}
			if let Some(link) = link
				&& link.width == 4
			{
				found.insert(link.objidx);
				visited.insert(link.objidx);
				return false;
			}
			visited.insert(child)
		})
	}

	/// `move_child`: moves the link of `old_parent_idx` at `old_position` to
	/// `new_parent_idx` at `new_position`. Returns the child.
	pub fn move_child(
		&mut self,
		old_parent_idx: u32,
		old_position: u32,
		new_parent_idx: u32,
		new_position: u32,
	) -> Result<u32> {
		self.distance_invalid = true;
		self.positions_invalid = true;
		if old_parent_idx >= self.len() || new_parent_idx >= self.len() {
			return Err(Error::OutOfBounds);
		}
		let child_id = self.index_for_offset(old_parent_idx, old_position)?;
		if child_id >= self.len() {
			return Err(Error::OutOfBounds);
		}
		self.vm(new_parent_idx).add_real_link(2, child_id, new_position);
		self.vm(child_id).add_parent(new_parent_idx, false)?;
		self.vm(old_parent_idx)
			.remove_real_link_unordered(child_id, old_position);
		self.vm(child_id).remove_parent(old_parent_idx);
		Ok(child_id)
	}

	/// `move_children`: moves the links of `old_parent_idx` at positions in
	/// `[old_pos_start, old_pos_end)` to `new_parent_idx`, from `new_pos_start`.
	pub fn move_children(
		&mut self,
		old_parent_idx: u32,
		old_pos_start: u32,
		old_pos_end: u32,
		new_parent_idx: u32,
		new_pos_start: u32,
	) -> Result<()> {
		if old_parent_idx >= self.len() || new_parent_idx >= self.len() {
			return Err(Error::OutOfBounds);
		}
		self.distance_invalid = true;
		self.positions_invalid = true;
		let mut old_links = Vec::new();
		let links = self.v(old_parent_idx).real_links.clone();
		for l in links {
			if l.position < old_pos_start || l.position >= old_pos_end {
				old_links.push(l);
				continue;
			}
			let array_pos = l.position - old_pos_start;
			let child_id = l.objidx;
			if child_id >= self.len() {
				return Err(Error::OutOfBounds);
			}
			self.vm(new_parent_idx)
				.add_real_link(2, child_id, new_pos_start + array_pos);
			self.vm(child_id).add_parent(new_parent_idx, false)?;
			self.vm(child_id).remove_parent(old_parent_idx);
		}
		self.vm(old_parent_idx).real_links = old_links;
		Ok(())
	}

	/// `duplicate_subgraph`: duplicates the nodes reachable from `node_idx` that
	/// are not in `index_map` yet, recording the new indices. Links are not
	/// reassigned.
	fn duplicate_subgraph(&mut self, node_idx: u32, index_map: &mut HbMap) -> Result<()> {
		let mut result = Ok(());
		self.traverse_directed_bfs(node_idx, |g, _, _, child, _| {
			if result.is_err() || index_map.has(child) {
				return false;
			}
			match g.duplicate(child, false) {
				Ok(r) => {
					index_map.set(child, r);
					true
				}
				Err(e) => {
					result = Err(e);
					false
				}
			}
		})?;
		result?;
		if index_map.in_error() {
			return Err(Error::AllocationFailure);
		}
		Ok(())
	}

	/// `duplicate (node_idx, copy_table)`: a copy of a node, with the same
	/// outgoing links and no parents. Returns the new index.
	pub fn duplicate(&mut self, node_idx: u32, _copy_table: bool) -> Result<u32> {
		if node_idx >= self.len() {
			return Err(Error::OutOfBounds);
		}
		if self.vertices.len() >= MAX_VERTICES {
			return Err(Error::LimitExceeded);
		}
		self.positions_invalid = true;
		self.distance_invalid = true;
		// The data is always copied: HarfBuzz shares the buffer unless asked to
		// copy it, but nothing writes to a shared buffer afterwards.
		let child = &self.vertices[node_idx as usize];
		let mut clone = Vertex::new(child.data.clone());
		clone.distance = child.distance;
		clone.space = child.space;
		let real_links = child.real_links.clone();
		let virtual_links = child.virtual_links.clone();
		self.vertices.push(clone);
		let clone_idx = self.len() - 1;
		self.ordering.push(clone_idx);
		for l in real_links {
			self.vm(clone_idx).real_links.push(l);
			self.vm(l.objidx).add_parent(clone_idx, false)?;
		}
		for l in virtual_links {
			self.vm(clone_idx).virtual_links.push(l);
			self.vm(l.objidx).add_parent(clone_idx, true)?;
		}
		Ok(clone_idx)
	}

	/// Reassigns the links of `parent_idx` to `child_idx` to `clone_idx`.
	fn reassign_links_to(&mut self, parent_idx: u32, child_idx: u32, clone_idx: u32) -> Result<()> {
		let num_real = self.v(parent_idx).real_links.len();
		let total = num_real + self.v(parent_idx).virtual_links.len();
		for i in 0..total {
			let is_virtual = i >= num_real;
			let objidx = self.link(parent_idx, i, num_real).objidx;
			if objidx != child_idx {
				continue;
			}
			self.reassign_link(parent_idx, i, num_real, clone_idx, is_virtual)?;
		}
		Ok(())
	}

	fn link(&self, parent: u32, i: usize, num_real: usize) -> Link {
		let v = self.v(parent);
		if i < num_real {
			v.real_links[i]
		} else {
			v.virtual_links[i - num_real]
		}
	}

	/// `duplicate (parent_idx, child_idx, copy_table)`: a copy of the child,
	/// linked from `parent_idx` instead of the original.
	pub fn duplicate_for_parent(&mut self, parent_idx: u32, child_idx: u32) -> Result<u32> {
		if parent_idx >= self.len() || child_idx >= self.len() {
			return Err(Error::OutOfBounds);
		}
		self.update_parents()?;
		let child = self.v(child_idx);
		let links_to_child = child.incoming_edges_from_parent(parent_idx);
		if child.incoming_edges() <= links_to_child || child.has_incoming_virtual_edges() {
			// All links to the child are from the parent: duplicating would orphan
			// the original.
			return Err(Error::InvalidArgument);
		}
		let clone_idx = self.duplicate(child_idx, true)?;
		let mut parent_idx = parent_idx;
		if parent_idx == clone_idx {
			parent_idx += 1;
		}
		self.reassign_links_to(parent_idx, child_idx, clone_idx)?;
		Ok(clone_idx)
	}

	/// `duplicate (parents, child_idx)`: a copy of the child, linked from all of
	/// `parents` instead of the original.
	pub fn duplicate_for_parents(&mut self, parents: &BTreeSet<u32>, child_idx: u32) -> Result<u32> {
		if parents.is_empty() {
			return Err(Error::InvalidArgument);
		}
		if child_idx >= self.len() {
			return Err(Error::OutOfBounds);
		}
		self.update_parents()?;
		let mut links_to_child = 0;
		for &parent_idx in parents {
			if parent_idx >= self.len() {
				return Err(Error::OutOfBounds);
			}
			links_to_child += self.v(child_idx).incoming_edges_from_parent(parent_idx);
		}
		let child = self.v(child_idx);
		if child.incoming_edges() <= links_to_child || child.has_incoming_virtual_edges() {
			return Err(Error::InvalidArgument);
		}
		let clone_idx = self.duplicate(child_idx, false)?;
		for &parent_idx in parents {
			let parent_idx = if parent_idx == clone_idx {
				parent_idx + 1
			} else {
				parent_idx
			};
			self.reassign_links_to(parent_idx, child_idx, clone_idx)?;
		}
		Ok(clone_idx)
	}

	/// `new_node`: a node linked to nothing.
	pub fn new_node(&mut self, data: Vec<u8>) -> Result<u32> {
		if self.vertices.len() >= MAX_VERTICES {
			return Err(Error::LimitExceeded);
		}
		self.positions_invalid = true;
		self.distance_invalid = true;
		self.vertices.push(Vertex::new(data));
		let idx = self.len() - 1;
		self.ordering.push(idx);
		Ok(idx)
	}

	/// `remap_child`: a copy of `old_child_idx`, linked from `parent_idx` in its
	/// place.
	pub fn remap_child(&mut self, parent_idx: u32, old_child_idx: u32) -> Result<u32> {
		if parent_idx >= self.len() || old_child_idx >= self.len() {
			return Err(Error::OutOfBounds);
		}
		let new_child_idx = self.duplicate(old_child_idx, false)?;
		self.reassign_links_to(parent_idx, old_child_idx, new_child_idx)?;
		Ok(new_child_idx)
	}

	/// `raise_childrens_priority`.
	pub fn raise_childrens_priority(&mut self, parent_idx: u32) -> Result<bool> {
		if parent_idx >= self.len() {
			return Err(Error::OutOfBounds);
		}
		let children: Vec<u32> = self
			.v(parent_idx)
			.all_links()
			.map(|(l, _)| l.objidx)
			.collect();
		let mut made_change = false;
		for c in children {
			made_change |= self.vm(c).raise_priority();
		}
		Ok(made_change)
	}

	fn is_fully_connected(&mut self) -> Result<()> {
		self.update_parents()?;
		if self.root().incoming_edges() != 0 {
			// The root cannot have parents.
			return Err(Error::OrphanedNodes);
		}
		for i in 0..self.root_idx() {
			if self.v(i).incoming_edges() == 0 {
				return Err(Error::OrphanedNodes);
			}
		}
		Ok(())
	}

	pub fn num_roots_for_space(&self, space: u32) -> u32 {
		self.num_roots_for_space[space as usize]
	}

	pub fn next_space(&self) -> u32 {
		self.num_roots_for_space.len() as u32
	}

	/// `move_to_new_space`.
	pub fn move_to_new_space(&mut self, indices: &BTreeSet<u32>) -> Result<()> {
		if self.num_roots_for_space.len() >= MAX_SPACES {
			return Err(Error::LimitExceeded);
		}
		self.num_roots_for_space.push(0);
		let new_space = self.num_roots_for_space.len() - 1;
		for &index in indices {
			if index >= self.len() {
				return Err(Error::OutOfBounds);
			}
			let space = self.v(index).space as usize;
			self.num_roots_for_space[space] = self.num_roots_for_space[space].wrapping_sub(1);
			self.num_roots_for_space[new_space] += 1;
			self.vm(index).space = new_space as u32;
			self.distance_invalid = true;
			self.positions_invalid = true;
		}
		Ok(())
	}

	/// `space_for`: the space of a node, and the root of that space (the first
	/// ancestor with a space, following first parents).
	pub fn space_for(&self, mut index: u32) -> (u32, u32) {
		loop {
			let node = self.v(index);
			if node.space != 0 {
				return (node.space, index);
			}
			match node.first_parent() {
				Some(p) if node.incoming_edges() != 0 => index = p,
				_ => return (0, index),
			}
		}
	}

	pub fn total_size_in_bytes(&self) -> usize {
		self.vertices.iter().map(Vertex::table_size).sum()
	}

	/// `wide_parents`: the number of incoming 24 or 32-bit links.
	fn wide_parents(&self, node_idx: u32, parents: &mut BTreeSet<u32>) -> u32 {
		let mut count = 0;
		for p in self.v(node_idx).parents_iter() {
			for l in &self.v(p).real_links {
				if l.objidx == node_idx && (l.width == 3 || l.width == 4) {
					count += 1;
					parents.insert(p);
				}
			}
		}
		count
	}

	/// `update_parents`: recomputes the incoming edges of every node.
	pub fn update_parents(&mut self) -> Result<()> {
		if !self.parents_invalid {
			return Ok(());
		}
		let count = self.len();
		for v in &mut self.vertices {
			v.reset_parents();
		}
		for p in 0..count {
			for i in 0..self.v(p).real_links.len() {
				let l = self.v(p).real_links[i];
				if l.objidx >= count {
					return Err(Error::InvalidArgument);
				}
				self.vm(l.objidx).add_parent(p, false)?;
			}
			for i in 0..self.v(p).virtual_links.len() {
				let l = self.v(p).virtual_links[i];
				if l.objidx >= count {
					return Err(Error::InvalidArgument);
				}
				self.vm(l.objidx).add_parent(p, true)?;
			}
		}
		self.parents_invalid = false;
		Ok(())
	}

	/// `update_positions`: the start and end of each node in the ordering.
	pub fn update_positions(&mut self) {
		if !self.positions_invalid {
			return;
		}
		let mut current_pos = 0u32;
		for idx in 0..self.ordering.len() {
			let i = self.ordering[idx];
			let v = self.vm(i);
			v.start = current_pos;
			current_pos = current_pos.wrapping_add(v.table_size() as u32);
			v.end = current_pos;
		}
		self.positions_invalid = false;
	}

	/// `update_distances`: the shortest distance of each node from the root
	/// (Dijkstra), where a link weighs the child's size plus the range of the
	/// offset times the space.
	fn update_distances(&mut self) -> Result<()> {
		if !self.distance_invalid {
			return Ok(());
		}
		let count = self.vertices.len();
		for v in &mut self.vertices {
			v.distance = i64::MAX;
		}
		let root = self.root_idx();
		self.vm(root).distance = 0;
		let mut queue = PriorityQueue::with_capacity(count);
		queue.insert(0, root);
		let mut visited = vec![false; count];
		while let Some((_, next_idx)) = queue.pop_minimum() {
			if visited[next_idx as usize] {
				continue;
			}
			let next_distance = self.v(next_idx).distance;
			visited[next_idx as usize] = true;
			let links: Vec<(Link, bool)> = self.v(next_idx).all_links().collect();
			for (link, _) in links {
				if link.objidx as usize >= count {
					return Err(Error::OutOfBounds);
				}
				if visited[link.objidx as usize] {
					continue;
				}
				let child = self.vm(link.objidx);
				// Virtual offsets count as 32 bits wide.
				let link_width = if link.width == 0 { 4 } else { link.width };
				let child_weight = child.table_size() as i64
					+ (1i64 << (link_width * 8)) * (i64::from(child.space) + 1);
				let child_distance = next_distance + child_weight;
				if child_distance < child.distance {
					child.distance = child_distance;
					queue.insert(child_distance, link.objidx);
				}
			}
		}
		self.distance_invalid = false;
		Ok(())
	}

	/// `reassign_link`: points link `i` of `parent_idx` at `new_idx`, updating
	/// the parents of the old and new child.
	fn reassign_link(
		&mut self,
		parent_idx: u32,
		i: usize,
		num_real: usize,
		new_idx: u32,
		is_virtual: bool,
	) -> Result<()> {
		let v = self.vm(parent_idx);
		let link = if i < num_real {
			&mut v.real_links[i]
		} else {
			&mut v.virtual_links[i - num_real]
		};
		let old_idx = link.objidx;
		link.objidx = new_idx;
		self.vm(old_idx).remove_parent(parent_idx);
		self.vm(new_idx).add_parent(parent_idx, is_virtual)
	}

	/// `remap_obj_indices`: points the links of the `subgraph` nodes found in
	/// `id_map` at the mapped nodes.
	fn remap_obj_indices(&mut self, id_map: &HbMap, subgraph: &[u32], only_wide: bool) -> Result<()> {
		if id_map.is_empty() {
			return Ok(());
		}
		for &i in subgraph {
			let num_real = self.v(i).real_links.len();
			let total = num_real + self.v(i).virtual_links.len();
			for j in 0..total {
				let link = self.link(i, j, num_real);
				let Some(v) = id_map.get(link.objidx) else {
					continue;
				};
				if only_wide && link.width != 4 && link.width != 3 {
					continue;
				}
				self.reassign_link(i, j, num_real, v, j >= num_real)?;
			}
		}
		Ok(())
	}

	/// `find_connected_nodes`: the nodes of `targets` reachable from
	/// `start_idx`, treating the graph as undirected and skipping `visited`.
	fn find_connected_nodes(
		&mut self,
		start_idx: u32,
		targets: &mut BTreeSet<u32>,
		visited: &mut Visited,
		connected: &mut BTreeSet<u32>,
	) -> Result<()> {
		if visited.has(start_idx) {
			return Ok(());
		}
		if start_idx >= self.len() {
			return Err(Error::OutOfBounds);
		}
		let capacity = self.vertices.len();
		// A depth first search; the graph is treated as undirected, so both the
		// children and the parents of each node are searched.
		let mut stack: Vec<u32> = Vec::with_capacity(capacity);
		let mut handle_node = |n: u32, visited: &mut Visited, stack: &mut Vec<u32>| {
			visited.add(n);
			if targets.remove(&n) {
				connected.insert(n);
			}
			if stack.len() >= capacity {
				return Err(Error::CycleDetected);
			}
			stack.push(n);
			Ok(())
		};
		handle_node(start_idx, visited, &mut stack)?;
		while let Some(node_idx) = stack.pop() {
			let v = self.v(node_idx);
			let neighbours: Vec<u32> = v
				.all_links()
				.map(|(l, _)| l.objidx)
				.chain(v.parents_iter())
				.collect();
			for n in neighbours {
				if visited.has(n) {
					continue;
				}
				handle_node(n, visited, &mut stack)?;
			}
		}
		Ok(())
	}
}
