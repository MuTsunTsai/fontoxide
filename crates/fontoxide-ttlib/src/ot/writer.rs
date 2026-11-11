//! `OTTableWriter` (`fontTools.ttLib.tables.otBase`): builds a tree of subtables
//! while compiling, then removes duplicates and lays out the data.
//!
//! The layout reproduces fontTools' pure-Python serializer exactly:
//! `_doneWriting` (sharing of identical subtables, compared structurally),
//! `_gatherTables` (reverse depth-first order, `sortCoverageLast`, Extension
//! subtables placed after everything else) and `getData` (offset computation, with
//! an [`OverflowErrorRecord`] for 16-bit offsets that do not fit).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use fontoxide_misc::Tag;

use super::state::{CountRef, State, StateValue};
use crate::{Error, Result};

/// A node of the writer tree.
pub type NodeId = usize;

#[derive(Debug, Clone)]
enum Item {
	Data(Vec<u8>),
	Offset { node: NodeId, size: usize },
	Count(CountRef),
}

#[derive(Debug)]
struct Node {
	items: Vec<Item>,
	state: Rc<State>,
	name: &'static str,
	repeat_index: Option<usize>,
	parent: Option<NodeId>,
	extension: bool,
	dont_share: bool,
	sort_coverage_last: bool,
	pos: usize,
}

/// Where a 16-bit offset overflowed (`OverflowErrorRecord`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverflowErrorRecord {
	/// The table, e.g. `GSUB`.
	pub table_type: Tag,
	/// The index of the lookup in the lookup list.
	pub lookup_list_index: Option<usize>,
	/// The index of the subtable in the lookup.
	pub sub_table_index: Option<usize>,
	/// The dotted path of the item that overflowed, below the subtable.
	pub item_name: Option<String>,
	/// The index of the item, if it is in an array.
	pub item_index: Option<usize>,
}

/// All writers of one table being compiled.
#[derive(Debug)]
pub struct WriterGraph {
	nodes: Vec<Node>,
	table_tag: Tag,
	/// Whether values are packed for speed rather than size
	/// ([`crate::Font::optimize_font_speed`]); `false` by default.
	pub optimize_font_speed: bool,
}

/// A writer for one (sub)table: a node in a [`WriterGraph`].
#[derive(Debug)]
pub struct OtWriter<'g> {
	graph: &'g mut WriterGraph,
	id: NodeId,
}

impl WriterGraph {
	/// A graph with an empty root writer.
	pub fn new(table_tag: Tag) -> Self {
		WriterGraph {
			nodes: vec![Node {
				items: Vec::new(),
				state: Rc::new(State::new()),
				name: "<none>",
				repeat_index: None,
				parent: None,
				extension: false,
				dont_share: false,
				sort_coverage_last: false,
				pos: 0,
			}],
			table_tag,
			optimize_font_speed: false,
		}
	}

	/// The root writer.
	pub fn root(&mut self) -> OtWriter<'_> {
		OtWriter { graph: self, id: 0 }
	}

	/// Names the root writer (`writer.name`, `writer.repeatIndex`), as overflow
	/// records report it, for compiling a part of a table on its own.
	pub fn set_root_name(&mut self, name: &'static str, repeat_index: Option<usize>) {
		self.nodes[0].name = name;
		self.nodes[0].repeat_index = repeat_index;
	}

	fn data_length(&self, id: NodeId) -> usize {
		self.nodes[id]
			.items
			.iter()
			.map(|item| match item {
				Item::Data(d) => d.len(),
				Item::Offset { size, .. } => *size,
				Item::Count(c) => c.size(),
			})
			.sum()
	}

	/// Assembles all data, sharing identical subtables (`getAllData`).
	pub fn get_all_data(mut self) -> Result<Vec<u8>> {
		let mut keys = StructuralKeys::default();
		let mut interned = HashMap::new();
		self.done_writing(0, &mut interned, &mut keys, false)?;
		self.get_all_data_shared()
	}

	/// Packs the table with the HarfBuzz repacker (`getAllDataUsingHarfbuzz`),
	/// sharing identical subtables across Extension subtables too. If the
	/// repacker fails, falls back to `getAllData (remove_duplicate=False)` on
	/// the shared tree (`tryPackingHarfbuzz`); `log_failure` logs that once.
	///
	/// Without the `repacker` feature, fails with [`crate::Error::Import`].
	#[cfg(not(feature = "repacker"))]
	pub fn get_all_data_using_harfbuzz(self, log_failure: bool) -> Result<Vec<u8>> {
		let _ = (self, log_failure);
		Err(crate::Error::Import("No module named 'uharfbuzz'".into()))
	}

	/// Packs the table with the HarfBuzz repacker (`getAllDataUsingHarfbuzz`),
	/// sharing identical subtables across Extension subtables too. If the
	/// repacker fails, falls back to `getAllData (remove_duplicate=False)` on
	/// the shared tree (`tryPackingHarfbuzz`); `log_failure` logs that once.
	#[cfg(feature = "repacker")]
	pub fn get_all_data_using_harfbuzz(mut self, log_failure: bool) -> Result<Vec<u8>> {
		let mut keys = StructuralKeys::default();
		let mut interned = HashMap::new();
		self.done_writing(0, &mut interned, &mut keys, true)?;
		let mut objects = Vec::new();
		let mut done = HashMap::new();
		self.gather_graph_for_harfbuzz(0, &mut objects, &mut done)?;
		match fontoxide_repack::serialize_with_tag(self.table_tag.to_bytes(), objects) {
			Ok(data) => Ok(data),
			Err(e) => {
				if log_failure {
					log::warn!(
						"hb.repack failed to serialize '{}', attempting fonttools resolutions ; the error message was: RepackerError ({e})",
						self.table_tag
					);
				}
				self.get_all_data_shared()
			}
		}
	}

	/// `_gatherGraphForHarfbuzz`: the objects in post-order, children first.
	/// Returns the object's position plus one, as `done` records it.
	///
	/// UPSTREAM-BUG(B17): `sortCoverageLast` looks for an item named `Coverage`
	/// among the items, which are `OffsetToWriter`s and bytes without a `name`,
	/// so it never applies and there are never virtual links.
	#[cfg(feature = "repacker")]
	fn gather_graph_for_harfbuzz(
		&self,
		id: NodeId,
		objects: &mut Vec<fontoxide_repack::Object>,
		done: &mut HashMap<NodeId, u32>,
	) -> Result<u32> {
		let mut item_idx = objects.len() as u32;
		let mut real_links = Vec::new();
		let mut data = Vec::with_capacity(self.data_length(id));
		for item in &self.nodes[id].items {
			match item {
				Item::Data(d) => data.extend_from_slice(d),
				Item::Count(c) => data.extend(c.data()?),
				Item::Offset { node, size } => {
					let child_idx = match done.get(node) {
						Some(&idx) => idx,
						None => {
							item_idx = self.gather_graph_for_harfbuzz(*node, objects, done)?;
							item_idx
						}
					};
					real_links.push(fontoxide_repack::Link {
						width: *size as u8,
						position: data.len() as u32,
						objidx: child_idx - 1,
					});
					// `getDataForHarfbuzz`: offsets are written as 0.
					data.resize(data.len() + size, 0);
				}
			}
		}
		objects.push(fontoxide_repack::Object {
			data,
			real_links,
			virtual_links: Vec::new(),
		});
		item_idx += 1;
		done.insert(id, item_idx);
		Ok(item_idx)
	}

	/// The end of `getAllData`, once `_doneWriting` has run.
	fn get_all_data_shared(mut self) -> Result<Vec<u8>> {
		let mut tables = Vec::new();
		let mut ext_tables = Vec::new();
		let mut done = HashSet::new();
		self.gather_tables(0, &mut tables, Some(&mut ext_tables), &mut done)?;
		tables.reverse();
		ext_tables.reverse();
		let mut pos = 0;
		for &t in tables.iter().chain(&ext_tables) {
			self.nodes[t].pos = pos;
			pos += self.data_length(t);
		}
		let mut data = Vec::with_capacity(pos);
		for &t in tables.iter().chain(&ext_tables) {
			self.get_data(t, &mut data)?;
		}
		Ok(data)
	}

	/// `_doneWriting`: resolves counts and replaces subtables by an identical one
	/// seen before in the same scope.
	fn done_writing(
		&mut self,
		id: NodeId,
		interned: &mut HashMap<usize, NodeId>,
		keys: &mut StructuralKeys,
		share_extension: bool,
	) -> Result<()> {
		let mut local = HashMap::new();
		let interned = if self.nodes[id].extension && !share_extension {
			&mut local
		} else {
			interned
		};
		let dont_share = self.nodes[id].dont_share;
		for i in 0..self.nodes[id].items.len() {
			match self.nodes[id].items[i].clone() {
				Item::Count(c) => self.nodes[id].items[i] = Item::Data(c.data()?),
				Item::Offset { node, size } => {
					self.done_writing(node, interned, keys, share_extension)?;
					if !dont_share {
						let key = keys.key(self, node);
						let canonical = *interned.entry(key).or_insert(node);
						self.nodes[id].items[i] = Item::Offset {
							node: canonical,
							size,
						};
					}
				}
				Item::Data(_) => {}
			}
		}
		Ok(())
	}

	/// `_gatherTables`: collects the tables in reverse order of appearance.
	fn gather_tables(
		&self,
		id: NodeId,
		tables: &mut Vec<NodeId>,
		ext_tables: Option<&mut Vec<NodeId>>,
		done: &mut HashSet<NodeId>,
	) -> Result<()> {
		done.insert(id);
		if self.nodes[id].extension {
			// An Extension subtable's subtree goes into the extension list, with a
			// fresh `done` set; the Extension subtable itself stays in `tables`.
			let ext = ext_tables.ok_or_else(|| {
				Error::Assertion(
					"Program or XML editing error. Extension subtables cannot contain extensions subtables"
						.into(),
				)
			})?;
			let mut fresh = HashSet::new();
			self.gather_children(id, ext, None, &mut fresh)?;
		} else {
			self.gather_children(id, tables, ext_tables, done)?;
		}
		tables.push(id);
		Ok(())
	}

	fn gather_children(
		&self,
		id: NodeId,
		tables: &mut Vec<NodeId>,
		mut ext_tables: Option<&mut Vec<NodeId>>,
		done: &mut HashSet<NodeId>,
	) -> Result<()> {
		let node = &self.nodes[id];
		let items = &node.items;
		let mut sort_coverage_last = false;
		if node.sort_coverage_last {
			let mut found = None;
			for item in items {
				if let Item::Offset { node: sub, .. } = item
					&& self.nodes[*sub].name == "Coverage"
				{
					sort_coverage_last = true;
					found = Some(*sub);
					break;
				}
			}
			// fontTools uses the loop variable after the loop, i.e. the last item
			// when no Coverage was found.
			let sub = match found {
				Some(s) => s,
				None => match items.last() {
					Some(Item::Offset { node: s, .. }) => *s,
					_ => return Err(Error::ttlib("'bytes' object has no attribute 'subWriter'")),
				},
			};
			if !done.contains(&sub) {
				self.gather_tables(sub, tables, ext_tables.as_deref_mut(), done)?;
			}
		}
		for (i, item) in items.iter().enumerate().rev() {
			let Item::Offset { node: sub, .. } = item else {
				continue;
			};
			if sort_coverage_last && i == 1 && self.nodes[*sub].name == "Coverage" {
				// Already gathered above.
				continue;
			}
			if !done.contains(sub) {
				self.gather_tables(*sub, tables, ext_tables.as_deref_mut(), done)?;
			}
		}
		Ok(())
	}

	/// `getData`: the table's data with offsets filled in.
	fn get_data(&self, id: NodeId, out: &mut Vec<u8>) -> Result<()> {
		let node = &self.nodes[id];
		for item in &node.items {
			match item {
				Item::Data(d) => out.extend_from_slice(d),
				Item::Count(c) => out.extend(c.data()?),
				Item::Offset { node: sub, size } => {
					let offset = self.nodes[*sub].pos as i64 - node.pos as i64;
					match size {
						4 => {
							let v = u32::try_from(offset).map_err(|_| {
								Error::Assertion(format!("0 <= {offset} < 0x100000000"))
							})?;
							out.extend_from_slice(&v.to_be_bytes());
						}
						2 => match u16::try_from(offset) {
							Ok(v) => out.extend_from_slice(&v.to_be_bytes()),
							Err(_) => {
								return Err(Error::OffsetOverflow(Box::new(
									self.overflow_error_record(id, *sub),
								)));
							}
						},
						3 => {
							if !(0..1 << 24).contains(&offset) {
								return Err(Error::Assertion(format!("0 <= {offset} < 0x1000000")));
							}
							out.extend_from_slice(&(offset as u32).to_be_bytes()[1..]);
						}
						s => return Err(Error::value(format!("{s}"))),
					}
				}
			}
		}
		Ok(())
	}

	/// `getOverflowErrorRecord`.
	fn overflow_error_record(&self, id: NodeId, item: NodeId) -> OverflowErrorRecord {
		let node = &self.nodes[id];
		let item_node = &self.nodes[item];
		let mut record = OverflowErrorRecord {
			table_type: self.table_tag,
			lookup_list_index: None,
			sub_table_index: None,
			item_name: None,
			item_index: None,
		};
		let parent = |n: NodeId| self.nodes[n].parent;
		if matches!(node.name, "LookupList" | "LookupList2") {
			record.lookup_list_index = item_node.repeat_index;
		} else if node.name == "Lookup" {
			record.lookup_list_index = node.repeat_index;
			record.sub_table_index = item_node.repeat_index;
		} else {
			let mut item_name = item_node.name.to_owned();
			record.item_index = item_node.repeat_index;
			if node.name == "SubTable" {
				record.lookup_list_index = node.parent.and_then(|p| self.nodes[p].repeat_index);
				record.sub_table_index = node.repeat_index;
			} else if node.name == "ExtSubTable" {
				record.lookup_list_index = node
					.parent
					.and_then(parent)
					.and_then(|p| self.nodes[p].repeat_index);
				record.sub_table_index = node.parent.and_then(|p| self.nodes[p].repeat_index);
			} else {
				// Who knows how far below the SubTable level we are! Climb back up
				// to the nearest subtable.
				item_name = format!("{}.{item_name}", node.name);
				let mut p1 = node.parent;
				while let Some(p) = p1 {
					let name = self.nodes[p].name;
					if matches!(name, "ExtSubTable" | "SubTable") {
						break;
					}
					item_name = format!("{name}.{item_name}");
					p1 = self.nodes[p].parent;
				}
				if let Some(p) = p1 {
					if self.nodes[p].name == "ExtSubTable" {
						record.lookup_list_index = parent(p)
							.and_then(parent)
							.and_then(|q| self.nodes[q].repeat_index);
						record.sub_table_index = parent(p).and_then(|q| self.nodes[q].repeat_index);
					} else {
						record.lookup_list_index =
							parent(p).and_then(|q| self.nodes[q].repeat_index);
						record.sub_table_index = self.nodes[p].repeat_index;
					}
				}
			}
			record.item_name = Some(item_name);
		}
		record
	}
}

/// Structural identity of subtables: two nodes get the same key if their items are
/// equal, comparing subtables by their own keys (fontTools' `__eq__` / `__hash__`).
#[derive(Debug, Default)]
struct StructuralKeys {
	ids: HashMap<Vec<KeyItem>, usize>,
	cache: HashMap<NodeId, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum KeyItem {
	Data(Vec<u8>),
	Offset(usize, usize),
}

impl StructuralKeys {
	fn key(&mut self, graph: &WriterGraph, id: NodeId) -> usize {
		if let Some(&k) = self.cache.get(&id) {
			return k;
		}
		let mut items = Vec::with_capacity(graph.nodes[id].items.len());
		// Adjacent data items are kept separate: fontTools compares the item
		// tuples element by element.
		for item in &graph.nodes[id].items {
			items.push(match item {
				Item::Data(d) => KeyItem::Data(d.clone()),
				Item::Count(c) => KeyItem::Data(c.data().unwrap_or_default()),
				Item::Offset { node, size } => KeyItem::Offset(self.key(graph, *node), *size),
			});
		}
		let next = self.ids.len();
		let k = *self.ids.entry(items).or_insert(next);
		self.cache.insert(id, k);
		k
	}
}

impl<'g> OtWriter<'g> {
	/// The node of this writer.
	pub fn id(&self) -> NodeId {
		self.id
	}

	fn node(&mut self) -> &mut Node {
		&mut self.graph.nodes[self.id]
	}

	/// Creates a writer for a subtable, which inherits the propagated state
	/// (`getSubWriter`), and appends an offset to it (`writeSubTable`).
	pub fn sub_table(
		&mut self,
		name: &'static str,
		repeat_index: Option<usize>,
		offset_size: usize,
	) -> OtWriter<'_> {
		let id = self.sub_writer(name, repeat_index);
		self.node().items.push(Item::Offset {
			node: id,
			size: offset_size,
		});
		OtWriter {
			graph: self.graph,
			id,
		}
	}

	/// Creates a writer that is not linked from this one (`getSubWriter`), for
	/// computing data that is then copied in.
	pub fn sub_writer(&mut self, name: &'static str, repeat_index: Option<usize>) -> NodeId {
		let state = self.graph.nodes[self.id].state.clone();
		let id = self.graph.nodes.len();
		self.graph.nodes.push(Node {
			items: Vec::new(),
			state,
			name,
			repeat_index,
			parent: Some(self.id),
			extension: false,
			dont_share: false,
			sort_coverage_last: false,
			pos: 0,
		});
		id
	}

	/// A handle to another node of the same graph.
	pub fn at(&mut self, id: NodeId) -> OtWriter<'_> {
		OtWriter {
			graph: self.graph,
			id,
		}
	}

	/// Appends raw data (`writeData`).
	pub fn write_data(&mut self, data: &[u8]) {
		self.node().items.push(Item::Data(data.to_vec()));
	}

	/// Writes what `f` writes as a single item, an empty one if `f` writes
	/// nothing (`writeArray`); `f` may only write data.
	pub fn write_array(&mut self, f: impl FnOnce(&mut Self) -> Result<()>) -> Result<()> {
		let start = self.node().items.len();
		f(self)?;
		let items = self.node().items.split_off(start);
		let mut data = Vec::new();
		for item in items {
			match item {
				Item::Data(d) => data.extend(d),
				_ => return Err(Error::Assertion("writeArray of non-data items".into())),
			}
		}
		self.node().items.push(Item::Data(data));
		Ok(())
	}

	/// Appends a big-endian `u8`.
	pub fn write_u8(&mut self, v: u8) {
		self.write_data(&[v]);
	}

	/// Appends a big-endian `i8`.
	pub fn write_i8(&mut self, v: i8) {
		self.write_data(&v.to_be_bytes());
	}

	/// Appends a big-endian `u16`.
	pub fn write_u16(&mut self, v: u16) {
		self.write_data(&v.to_be_bytes());
	}

	/// Appends a big-endian `i16`.
	pub fn write_i16(&mut self, v: i16) {
		self.write_data(&v.to_be_bytes());
	}

	/// Appends a big-endian 24-bit integer.
	pub fn write_u24(&mut self, v: u32) -> Result<()> {
		if v >= 1 << 24 {
			return Err(Error::Assertion(format!("0 <= {v} < 0x1000000")));
		}
		self.write_data(&v.to_be_bytes()[1..]);
		Ok(())
	}

	/// Appends a big-endian `u32`.
	pub fn write_u32(&mut self, v: u32) {
		self.write_data(&v.to_be_bytes());
	}

	/// Appends a big-endian `i32`.
	pub fn write_i32(&mut self, v: i32) {
		self.write_data(&v.to_be_bytes());
	}

	/// Appends a tag.
	pub fn write_tag(&mut self, tag: Tag) {
		self.write_data(&tag.0);
	}

	/// Appends an array as one item (`write…Array`).
	pub fn write_u16_array(&mut self, values: &[u16]) {
		let data: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
		self.write_data(&data);
	}

	/// Appends a count to be filled in later (`writeCountReference`).
	pub fn write_count_reference(&mut self, count: CountRef) {
		self.node().items.push(Item::Count(count));
	}

	/// The current length of this writer's data, without subtables
	/// (`getDataLength`).
	pub fn data_length(&self) -> usize {
		self.graph.data_length(self.id)
	}

	/// Replaces the item at `index` with data (used by `StructWithLength`).
	pub fn replace_item(&mut self, index: usize, data: Vec<u8>) {
		self.node().items[index] = Item::Data(data);
	}

	/// The number of items.
	pub fn item_count(&self) -> usize {
		self.graph.nodes[self.id].items.len()
	}

	/// Marks the writer as an Extension subtable (`writer.Extension = True`).
	pub fn set_extension(&mut self) {
		self.node().extension = true;
	}

	/// Prevents sharing of this table's subtables (`writer.DontShare`).
	pub fn set_dont_share(&mut self) {
		self.node().dont_share = true;
	}

	/// Places the Coverage subtable last (`writer.sortCoverageLast`).
	pub fn set_sort_coverage_last(&mut self) {
		self.node().sort_coverage_last = true;
	}

	/// A propagated value (`writer[name]`).
	pub fn get(&self, name: &str) -> Option<&StateValue> {
		self.graph.nodes[self.id].state.get(name)
	}

	/// Propagates a value to subtables created from now on (`writer[name] = v`).
	pub fn set(&mut self, name: &'static str, value: StateValue) {
		Rc::make_mut(&mut self.node().state).set(name, value);
	}

	/// The count reference propagated under `name`, failing like a `KeyError`.
	pub fn count_ref(&self, name: &str) -> Result<CountRef> {
		match self.get(name) {
			Some(StateValue::Count(c)) => Ok(c.clone()),
			_ => Err(Error::ttlib(format!("KeyError: {name:?}"))),
		}
	}

	/// The table tag.
	pub fn table_tag(&self) -> Tag {
		self.graph.table_tag
	}

	/// See [`WriterGraph::optimize_font_speed`] (`font.cfg[OPTIMIZE_FONT_SPEED]`).
	pub fn optimize_font_speed(&self) -> bool {
		self.graph.optimize_font_speed
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn shares_identical_subtables() -> Result<()> {
		let mut g = WriterGraph::new(Tag::new(b"GSUB"));
		{
			let mut root = g.root();
			root.write_u16(1);
			for i in 0..2 {
				let mut sub = root.sub_table("Coverage", Some(i), 2);
				sub.write_u16(0xABCD);
			}
		}
		let data = g.get_all_data()?;
		// Two offsets to the same subtable, which comes after the root.
		assert_eq!(data, [0, 1, 0, 6, 0, 6, 0xAB, 0xCD]);
		Ok(())
	}

	#[test]
	fn subtables_are_laid_out_in_reverse_order() -> Result<()> {
		let mut g = WriterGraph::new(Tag::new(b"GSUB"));
		{
			let mut root = g.root();
			root.sub_table("A", None, 2).write_u16(0xAAAA);
			root.sub_table("B", None, 2).write_u16(0xBBBB);
		}
		let data = g.get_all_data()?;
		// _gatherTables walks the items backwards and reverses: root, A, B.
		assert_eq!(data, [0, 4, 0, 6, 0xAA, 0xAA, 0xBB, 0xBB]);
		Ok(())
	}

	#[test]
	fn count_references_are_resolved() -> Result<()> {
		let mut g = WriterGraph::new(Tag::new(b"GSUB"));
		{
			let mut root = g.root();
			let count = CountRef::new("GlyphCount", 2, None);
			root.write_count_reference(count.clone());
			count.set_value(3)?;
		}
		assert_eq!(g.get_all_data()?, [0, 3]);
		Ok(())
	}
}
