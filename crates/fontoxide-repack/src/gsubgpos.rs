//! `GSUB`/`GPOS` in the graph (`graph/gsubgpos-graph.hh`,
//! `graph/gsubgpos-context.cc`): the lookups, extension promotion and the
//! splitting of subtables.

use std::collections::{BTreeSet, HashMap};

use crate::graph::Graph;
use crate::layout::{get16, get32, set16};
use crate::{Error, Result, ligature, markbasepos, pairpos};

pub(crate) const GSUB: [u8; 4] = *b"GSUB";
pub(crate) const GPOS: [u8; 4] = *b"GPOS";

/// `Lookup::min_size`.
pub(crate) const LOOKUP_MIN_SIZE: u32 = 6;
/// The size of `ExtensionFormat1`.
const EXTENSION_SIZE: usize = 8;

/// `gsubgpos_graph_context_t`.
#[derive(Debug)]
pub(crate) struct Context {
	pub table_tag: [u8; 4],
	pub lookup_list_index: u32,
	/// The lookups (`lookups`; the pointers to their data are not needed, it is
	/// always read from the vertex).
	pub lookups: BTreeSet<u32>,
	subtable_to_extension: HashMap<u32, u32>,
	split_subtables: HashMap<u32, Vec<u32>>,
}

impl Context {
	/// `gsubgpos_graph_context_t::create`.
	pub fn create(table_tag: [u8; 4], g: &mut Graph) -> Result<Context> {
		if table_tag != GPOS && table_tag != GSUB {
			return Err(Error::InvalidArgument);
		}
		let root = g.root_idx();
		gstar_sanitize(&g.v(root).data)?;
		let mut c = Context {
			table_tag,
			lookup_list_index: 0,
			lookups: BTreeSet::new(),
			subtable_to_extension: HashMap::new(),
			split_subtables: HashMap::new(),
		};
		find_lookups(g, &mut c.lookups)?;
		c.lookup_list_index = lookup_list_index(g)?;
		Ok(c)
	}

	/// `create_node`: a new node of `size` zero bytes.
	pub fn create_node(&mut self, g: &mut Graph, size: usize) -> Result<u32> {
		if size > i32::MAX as usize {
			// Sizes computed from inconsistent data; `hb_calloc` would fail.
			return Err(Error::AllocationFailure);
		}
		g.new_node(vec![0; size])
	}
}

fn version(data: &[u8]) -> u32 {
	get32(data, 0)
}

/// `GSTAR::sanitize`.
fn gstar_sanitize(data: &[u8]) -> Result<()> {
	let len = data.len();
	if len < 10 || get16(data, 0) != 1 {
		return Err(Error::SanitizeFailure);
	}
	let v = version(data);
	let size = 10 + if v >= 0x0001_0001 { 4 } else { 0 } + if v >= 0x0001_0002 { 12 } else { 0 };
	if len < size {
		return Err(Error::SanitizeFailure);
	}
	Ok(())
}

/// Whether the 32-bit lookup list of version 1.2 is used (`lookupList2` set).
fn uses_lookup_list2(data: &[u8]) -> bool {
	version(data) >= 0x0001_0002 && get32(data, 22) != 0
}

/// `get_lookup_list_index`.
fn lookup_list_index(g: &Graph) -> Result<u32> {
	let root = g.root_idx();
	let position = if uses_lookup_list2(&g.v(root).data) {
		22
	} else {
		8
	};
	g.index_for_offset(root, position)
}

/// `GSTAR::find_lookups`.
fn find_lookups(g: &mut Graph, lookups: &mut BTreeSet<u32>) -> Result<()> {
	let item_size = if uses_lookup_list2(&g.v(g.root_idx()).data) {
		3
	} else {
		2
	};
	let lookup_list_idx = lookup_list_index(g)?;
	let data = &g.v(lookup_list_idx).data;
	let count = get16(data, 0) as usize;
	// `LookupList::sanitize`.
	if data.len() < 2 || data.len() < item_size * count {
		return Err(Error::SanitizeFailure);
	}
	for i in 0..count {
		let lookup_idx = g.index_for_offset(lookup_list_idx, (2 + i * item_size) as u32)?;
		if lookup_sanitize(&g.v(lookup_idx).data).is_err() {
			continue;
		}
		lookups.insert(lookup_idx);
	}
	Ok(())
}

fn lookup_type(data: &[u8]) -> u16 {
	get16(data, 0)
}

fn use_mark_filtering_set(data: &[u8]) -> bool {
	get16(data, 2) & 0x0010 != 0
}

/// `Lookup::number_of_subtables`.
pub(crate) fn number_of_subtables(g: &Graph, lookup: u32) -> u32 {
	u32::from(get16(&g.v(lookup).data, 4))
}

/// `Lookup::sanitize`.
fn lookup_sanitize(data: &[u8]) -> Result<()> {
	if data.len() < LOOKUP_MIN_SIZE as usize {
		return Err(Error::SanitizeFailure);
	}
	let size = 6 + 2 * get16(data, 4) as usize + if use_mark_filtering_set(data) { 2 } else { 0 };
	if data.len() < size {
		return Err(Error::SanitizeFailure);
	}
	Ok(())
}

fn extension_type(table_tag: [u8; 4]) -> u16 {
	match table_tag {
		GPOS => 9,
		GSUB => 7,
		_ => 0,
	}
}

/// `Lookup::is_extension`.
pub(crate) fn is_extension(g: &Graph, lookup: u32, table_tag: [u8; 4]) -> bool {
	lookup_type(&g.v(lookup).data) == extension_type(table_tag)
}

fn is_supported_gsub_type(lookup_type: u16, c: &Context) -> bool {
	c.table_tag == GSUB && lookup_type == 4
}

fn is_supported_gpos_type(lookup_type: u16, c: &Context) -> bool {
	c.table_tag == GPOS && (lookup_type == 2 || lookup_type == 4)
}

/// `Lookup::make_extension`: wraps the subtables of a lookup in Extension
/// subtables.
pub(crate) fn make_extension(c: &mut Context, g: &mut Graph, this_index: u32) -> Result<()> {
	let ext_type = extension_type(c.table_tag);
	if ext_type == 0 || is_extension(g, this_index, c.table_tag) {
		return Ok(());
	}
	let mut seen = BTreeSet::new();
	let mut subtables = Vec::new();
	for i in 0..number_of_subtables(g, this_index) {
		let subtable_index = g.index_for_offset(this_index, LOOKUP_MIN_SIZE + 2 * i)?;
		if seen.insert(subtable_index) {
			subtables.push(subtable_index);
		}
	}
	for subtable_index in subtables {
		make_subtable_extension(c, g, this_index, subtable_index)?;
	}
	set16(&mut g.vm(this_index).data, 0, ext_type);
	Ok(())
}

/// `Lookup::create_extension_subtable`: an Extension subtable pointing at
/// `subtable_index` (without adding it as a parent).
fn create_extension_subtable(
	c: &mut Context,
	g: &mut Graph,
	subtable_index: u32,
	lookup_type: u16,
) -> Result<u32> {
	let ext_index = c.create_node(g, EXTENSION_SIZE)?;
	let data = &mut g.vm(ext_index).data;
	set16(data, 0, 1);
	set16(data, 2, lookup_type);
	g.vm(ext_index).add_real_link(4, subtable_index, 4);
	Ok(ext_index)
}

/// `Lookup::make_subtable_extension`.
fn make_subtable_extension(
	c: &mut Context,
	g: &mut Graph,
	lookup_index: u32,
	subtable_index: u32,
) -> Result<()> {
	let lookup_type = lookup_type(&g.v(lookup_index).data);
	let existing = c.subtable_to_extension.get(&subtable_index).copied();
	let ext_index = match existing {
		Some(e) => e,
		None => {
			let e = create_extension_subtable(c, g, subtable_index, lookup_type)?;
			c.subtable_to_extension.insert(subtable_index, e);
			e
		}
	};
	let mut replaced = 0;
	for l in &mut g.vm(lookup_index).real_links {
		if l.objidx == subtable_index {
			// The lookup points at the extension instead.
			l.objidx = ext_index;
			replaced += 1;
		}
	}
	for _ in 0..replaced {
		g.vm(subtable_index).remove_parent(lookup_index);
		g.vm(ext_index).add_parent(lookup_index, false)?;
	}
	if existing.is_none() {
		g.vm(subtable_index).add_parent(ext_index, false)?;
	}
	Ok(())
}

/// The subtables a split produced, by position in the lookup.
type NewSubtables = Vec<(u32, Vec<u32>)>;

/// `Lookup::split_subtables_if_needed`.
pub(crate) fn split_subtables_if_needed(
	c: &mut Context,
	g: &mut Graph,
	this_index: u32,
) -> Result<()> {
	let mut lookup_type = lookup_type(&g.v(this_index).data);
	let is_ext = is_extension(g, this_index, c.table_tag);
	if c.table_tag != GPOS && c.table_tag != GSUB {
		return Ok(());
	}
	if !is_ext && !is_supported_gpos_type(lookup_type, c) && !is_supported_gsub_type(lookup_type, c)
	{
		return Ok(());
	}
	let mut all_new_subtables: NewSubtables = Vec::new();
	for i in 0..number_of_subtables(g, this_index) {
		// Subtables that cannot be read are not split.
		let Ok(mut subtable_index) = g.index_for_offset(this_index, LOOKUP_MIN_SIZE + 2 * i) else {
			continue;
		};
		if is_ext {
			let ext_subtable_index = subtable_index;
			if g.v(ext_subtable_index).table_size() < EXTENSION_SIZE {
				continue;
			}
			subtable_index = g.index_for_offset(ext_subtable_index, 4)?;
			lookup_type = get16(&g.v(ext_subtable_index).data, 2);
			if !is_supported_gpos_type(lookup_type, c) && !is_supported_gsub_type(lookup_type, c) {
				continue;
			}
		}
		if let Some(split_result) = c.split_subtables.get(&subtable_index) {
			if split_result.is_empty() {
				continue;
			}
			all_new_subtables.push((i, split_result.clone()));
		} else {
			let new_sub_tables = match (c.table_tag, lookup_type) {
				(GPOS, 2) => pairpos::split_subtable(c, g, subtable_index)?,
				(GPOS, 4) => markbasepos::split_subtable(c, g, subtable_index)?,
				(GSUB, 4) => ligature::split_subtable(c, g, subtable_index)?,
				_ => Vec::new(),
			};
			c.split_subtables
				.insert(subtable_index, new_sub_tables.clone());
			if !new_sub_tables.is_empty() {
				all_new_subtables.push((i, new_sub_tables));
			}
		}
	}
	if !all_new_subtables.is_empty() {
		add_sub_tables(c, g, this_index, lookup_type, &all_new_subtables)?;
	}
	Ok(())
}

/// `Lookup::add_sub_tables`: inserts the new subtables after the ones they were
/// split from.
fn add_sub_tables(
	c: &mut Context,
	g: &mut Graph,
	this_index: u32,
	lookup_type: u16,
	subtable_ids: &NewSubtables,
) -> Result<()> {
	let is_ext = is_extension(g, this_index, c.table_tag);
	fix_existing_subtable_links(g, this_index, subtable_ids);
	let new_subtable_count: usize = subtable_ids.iter().map(|p| p.1.len()).sum();
	let old = g.v(this_index).data.clone();
	let new_size = old.len() + new_subtable_count * 2;
	let mut buffer = vec![0u8; new_size];
	buffer[..old.len()].copy_from_slice(&old);
	if use_mark_filtering_set(&old) && old.len() >= 2 {
		buffer[new_size - 2..].copy_from_slice(&old[old.len() - 2..]);
	}
	set16(
		&mut buffer,
		4,
		get16(&old, 4).wrapping_add(new_subtable_count as u16),
	);
	g.vm(this_index).data = buffer;

	let mut shift = 0;
	for (first, ids) in subtable_ids {
		let offset_start = first + shift + 1;
		shift += ids.len() as u32;
		for (offset_index, &subtable_id) in (offset_start..).zip(ids) {
			let mut subtable_id = subtable_id;
			if is_ext {
				let ext_id = create_extension_subtable(c, g, subtable_id, lookup_type)?;
				g.vm(subtable_id).add_parent(ext_id, false)?;
				subtable_id = ext_id;
			}
			g.vm(this_index)
				.add_real_link(2, subtable_id, LOOKUP_MIN_SIZE + 2 * offset_index);
			g.vm(subtable_id).add_parent(this_index, false)?;
		}
	}
	// The packing order depends on the link order, so sort the links again.
	g.vm(this_index).sort_real_links();
	Ok(())
}

/// `Lookup::fix_existing_subtable_links`: moves the links after each insertion
/// point.
fn fix_existing_subtable_links(g: &mut Graph, this_index: u32, subtable_ids: &NewSubtables) {
	let v = g.vm(this_index);
	let mut shift = 0;
	for (first, ids) in subtable_ids {
		let insert_index = first + shift;
		let pos_offset = ids.len() as u32 * 2;
		let insert_offset = LOOKUP_MIN_SIZE + insert_index * 2;
		shift += ids.len() as u32;
		for l in v.real_links.iter_mut().chain(v.virtual_links.iter_mut()) {
			if l.position > insert_offset {
				l.position += pos_offset;
			}
		}
	}
}

/// `actuate_subtable_split` (`graph/split-helpers.hh`): clones each range
/// between split points into a new subtable, then shrinks the original to the
/// first range.
pub(crate) fn actuate_subtable_split<S: SplitContext>(
	sc: &mut S,
	c: &mut Context,
	g: &mut Graph,
	split_points: &[u32],
) -> Result<Vec<u32>> {
	let mut new_objects = Vec::new();
	if split_points.is_empty() {
		return Ok(new_objects);
	}
	for (i, &start) in split_points.iter().enumerate() {
		let end = split_points
			.get(i + 1)
			.copied()
			.unwrap_or_else(|| sc.original_count(g));
		new_objects.push(sc.clone_range(c, g, start, end)?);
	}
	sc.shrink(c, g, split_points[0])?;
	Ok(new_objects)
}

/// The `split_context_t` of a subtable type.
pub(crate) trait SplitContext {
	fn original_count(&self, g: &Graph) -> u32;
	fn clone_range(&mut self, c: &mut Context, g: &mut Graph, start: u32, end: u32) -> Result<u32>;
	fn shrink(&mut self, c: &mut Context, g: &mut Graph, count: u32) -> Result<()>;
}
