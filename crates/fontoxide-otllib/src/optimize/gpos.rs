//! Compacting class-based PairPos subtables (`fontTools.otlLib.optimize.gpos`):
//! large class kerning subtables with many zeros are split into smaller,
//! denser ones.

use std::collections::{BTreeMap, HashMap};

use fontoxide_ttlib::Font;
use fontoxide_ttlib::ot::conv::ValueRecord;
use fontoxide_ttlib::ot::types::{self as ot, LookupSubTable};
use fontoxide_ttlib::tables::Gpos;
use indexmap::IndexMap;

use crate::builder::{ValuePair, build_pair_pos_classes_subtable};
use crate::{Error, GlyphMap, Result};

/// Compacts the PairPos lookups of the font's GPOS table at a compression level
/// from 1 to 9 (`compact`). Fonts without GPOS are left alone.
pub fn compact(font: &mut Font, level: u8) -> Result<()> {
	if !font.has::<Gpos>() {
		return Ok(());
	}
	let glyph_map = GlyphMap::from_font(font)?;
	let gpos = font.require::<Gpos>()?;
	// `_getLookupList`.
	let list = if gpos.version >= 0x0001_0002 {
		match &mut gpos.table.lookup_list2 {
			Some(l) => Some(&mut l.lookup),
			None => gpos.table.lookup_list.as_mut().map(|l| &mut l.lookup),
		}
	} else {
		match &mut gpos.table.lookup_list {
			Some(l) => Some(&mut l.lookup),
			None => gpos.table.lookup_list2.as_mut().map(|l| &mut l.lookup),
		}
	};
	let Some(lookups) = list else {
		return Ok(());
	};
	for lookup in lookups.iter_mut().flatten() {
		if lookup.lookup_type == 2 {
			compact_lookup_with_map(&glyph_map, level, lookup)?;
		} else if lookup.lookup_type == 9 && first_extension_type(lookup)? == 2 {
			compact_ext_lookup(&glyph_map, level, lookup)?;
		}
	}
	Ok(())
}

fn first_extension_type(lookup: &ot::Lookup) -> Result<u16> {
	match lookup.sub_table.first().and_then(|s| s.as_deref()) {
		Some(LookupSubTable::ExtensionPos(ot::ExtensionPos::Format1(e))) => {
			Ok(e.extension_lookup_type)
		}
		_ => Err(Error::Value(
			"AttributeError: subtable has no attribute 'ExtensionLookupType'".into(),
		)),
	}
}

/// Compacts the subtables of a PairPos lookup (`compact_lookup`).
pub fn compact_lookup(font: &mut Font, level: u8, lookup: &mut ot::Lookup) -> Result<()> {
	let glyph_map = GlyphMap::from_font(font)?;
	compact_lookup_with_map(&glyph_map, level, lookup)
}

/// [`compact_lookup`] with the font's glyph map.
pub fn compact_lookup_with_map(
	glyph_map: &GlyphMap,
	level: u8,
	lookup: &mut ot::Lookup,
) -> Result<()> {
	let subtables = std::mem::take(&mut lookup.sub_table)
		.into_iter()
		.map(|s| match s.map(|b| *b) {
			Some(LookupSubTable::PairPos(p)) => Ok(p),
			_ => Err(Error::Value(
				"AttributeError: subtable is not a PairPos".into(),
			)),
		})
		.collect::<Result<Vec<_>>>()?;
	let new = compact_pair_pos(glyph_map, level, subtables)?;
	lookup.sub_table_count = Some(new.len() as u32);
	lookup.sub_table = new
		.into_iter()
		.map(|p| Some(Box::new(LookupSubTable::PairPos(p))))
		.collect();
	Ok(())
}

/// `compact_ext_lookup`: the same for an Extension lookup of PairPos subtables.
fn compact_ext_lookup(glyph_map: &GlyphMap, level: u8, lookup: &mut ot::Lookup) -> Result<()> {
	let subtables = std::mem::take(&mut lookup.sub_table)
		.into_iter()
		.map(|s| match s.map(|b| *b) {
			Some(LookupSubTable::ExtensionPos(ot::ExtensionPos::Format1(e))) => {
				match e.ext_sub_table.map(|b| *b) {
					Some(LookupSubTable::PairPos(p)) => Ok(p),
					_ => Err(Error::Value(
						"AttributeError: subtable is not a PairPos".into(),
					)),
				}
			}
			_ => Err(Error::Value(
				"AttributeError: subtable is not an Extension".into(),
			)),
		})
		.collect::<Result<Vec<_>>>()?;
	let new = compact_pair_pos(glyph_map, level, subtables)?;
	lookup.sub_table_count = Some(new.len() as u32);
	lookup.sub_table = new
		.into_iter()
		.map(|p| {
			Some(Box::new(LookupSubTable::ExtensionPos(
				ot::ExtensionPos::Format1(ot::ExtensionPosFormat1 {
					// fontTools leaves it unset; compiling fills it in.
					extension_lookup_type: 2,
					ext_sub_table: Some(Box::new(LookupSubTable::PairPos(p))),
					dont_share: false,
				}),
			)))
		})
		.collect();
	Ok(())
}

/// `compact_pair_pos`: glyph-based subtables are kept, class-based ones are
/// split.
fn compact_pair_pos(
	glyph_map: &GlyphMap,
	level: u8,
	subtables: Vec<ot::PairPos>,
) -> Result<Vec<ot::PairPos>> {
	let mut out = Vec::new();
	for subtable in subtables {
		let extended = match &subtable {
			ot::PairPos::Format2(_) => false,
			ot::PairPos::Format4(_) => true,
			_ => {
				out.push(subtable);
				continue;
			}
		};
		for st in compact_class_pairs(glyph_map, level, &subtable)? {
			out.push(set_class_format(st, extended));
		}
	}
	Ok(out)
}

/// Gives a class-based subtable the format of the one it came from.
fn set_class_format(st: ot::PairPos, extended: bool) -> ot::PairPos {
	macro_rules! convert {
		($from:ident, $to:ident, $v:expr) => {{
			let ot::$from {
				coverage,
				value_format1,
				value_format2,
				class_def1,
				class_def2,
				class1_count,
				class2_count,
				class1_record,
				dont_share,
			} = $v;
			ot::$to {
				coverage,
				value_format1,
				value_format2,
				class_def1,
				class_def2,
				class1_count,
				class2_count,
				class1_record,
				dont_share,
			}
		}};
	}
	match (st, extended) {
		(ot::PairPos::Format2(v), true) => {
			ot::PairPos::Format4(convert!(PairPosFormat2, PairPosFormat4, v))
		}
		(ot::PairPos::Format4(v), false) => {
			ot::PairPos::Format2(convert!(PairPosFormat4, PairPosFormat2, v))
		}
		(st, _) => st,
	}
}

/// The parts of a class-based subtable.
struct ClassPairs<'a> {
	coverage: &'a [u32],
	class_def1: &'a BTreeMap<u32, u32>,
	class_def2: &'a BTreeMap<u32, u32>,
	class1_record: &'a [ot::Class1Record],
}

fn class_pairs(st: &ot::PairPos) -> Option<ClassPairs<'_>> {
	let empty: &'static BTreeMap<u32, u32> = {
		static EMPTY: std::sync::OnceLock<BTreeMap<u32, u32>> = std::sync::OnceLock::new();
		EMPTY.get_or_init(BTreeMap::new)
	};
	let (coverage, cd1, cd2, records) = match st {
		ot::PairPos::Format2(p) => (&p.coverage, &p.class_def1, &p.class_def2, &p.class1_record),
		ot::PairPos::Format4(p) => (&p.coverage, &p.class_def1, &p.class_def2, &p.class1_record),
		_ => return None,
	};
	Some(ClassPairs {
		coverage: coverage.as_ref().map_or(&[][..], |c| &c.glyphs[..]),
		class_def1: cd1.as_ref().map_or(empty, |c| &c.class_defs),
		class_def2: cd2.as_ref().map_or(empty, |c| &c.class_defs),
		class1_record: records,
	})
}

type Pairs = IndexMap<(Vec<String>, Vec<String>), ValuePair>;

/// `compact_class_pairs`: the pairs of a class-based subtable, clustered into
/// new subtables.
fn compact_class_pairs(
	glyph_map: &GlyphMap,
	level: u8,
	subtable: &ot::PairPos,
) -> Result<Vec<ot::PairPos>> {
	let Some(st) = class_pairs(subtable) else {
		return Ok(Vec::new());
	};
	let names = glyph_map.names_by_id();
	let name = |gid: u32| -> Result<String> {
		names
			.get(&gid)
			.cloned()
			.ok_or_else(|| Error::Value(format!("KeyError: {gid}")))
	};
	let mut classes1: HashMap<u32, Vec<String>> = HashMap::new();
	for &g in st.coverage {
		classes1
			.entry(st.class_def1.get(&g).copied().unwrap_or(0))
			.or_default()
			.push(name(g)?);
	}
	let mut classes2: HashMap<u32, Vec<String>> = HashMap::new();
	for (&g, &i) in st.class_def2 {
		classes2.entry(i).or_default().push(name(g)?);
	}
	let sorted = |v: Option<&Vec<String>>| {
		let mut v = v.cloned().unwrap_or_default();
		v.sort();
		v
	};
	let mut all_pairs: Pairs = IndexMap::new();
	for (i, class1) in st.class1_record.iter().enumerate() {
		for (j, class2) in class1.class2_record.iter().enumerate() {
			if is_really_zero(class2) {
				continue;
			}
			all_pairs.insert(
				(
					sorted(classes1.get(&(i as u32))),
					sorted(classes2.get(&(j as u32))),
				),
				(class2.value1.clone(), class2.value2.clone()),
			);
		}
	}
	let grouped = cluster_pairs_by_class2_coverage_custom_cost(glyph_map, all_pairs, level)?;
	grouped
		.iter()
		.map(|pairs| build_pair_pos_classes_subtable(pairs, glyph_map, None, None))
		.collect()
}

/// The format of the non-zero fields (`getEffectiveFormat`).
fn effective_format(v: &ValueRecord) -> u16 {
	let mut f = 0;
	let ints = [v.x_placement, v.y_placement, v.x_advance, v.y_advance];
	for (i, x) in ints.iter().enumerate() {
		if x.is_some_and(|x| x != 0) {
			f |= 1 << i;
		}
	}
	let devices = [
		&v.x_pla_device,
		&v.y_pla_device,
		&v.x_adv_device,
		&v.y_adv_device,
	];
	for (i, d) in devices.iter().enumerate() {
		if matches!(d, Some(Some(_))) {
			f |= 0x10 << i;
		}
	}
	for (i, r) in v.reserved.iter().enumerate() {
		if r.is_some_and(|r| r != 0) {
			f |= 0x100 << i;
		}
	}
	f
}

/// `is_really_zero`.
fn is_really_zero(class2: &ot::Class2Record) -> bool {
	class2
		.value1
		.as_ref()
		.is_none_or(|v| effective_format(v) == 0)
		&& class2
			.value2
			.as_ref()
			.is_none_or(|v| effective_format(v) == 0)
}

/// `_getClassRanges`: the ranges of consecutive glyph IDs, and the lowest and
/// highest ID.
fn class_ranges(glyph_ids: impl Iterator<Item = u32>) -> Result<ClassData> {
	let mut ids: Vec<u32> = glyph_ids.collect();
	ids.sort_unstable();
	let (&first, &last_id) = ids
		.first()
		.zip(ids.last())
		.ok_or_else(|| Error::Value("IndexError: list index out of range".into()))?;
	let mut last = first;
	let mut ranges = vec![(first, first)];
	for &id in &ids[1..] {
		if id != last + 1 {
			if let Some(r) = ranges.last_mut() {
				r.1 = last;
			}
			ranges.push((id, id));
		}
		last = id;
	}
	if let Some(r) = ranges.last_mut() {
		r.1 = last;
	}
	Ok((ranges, first, last_id))
}

type ClassData = (Vec<(u32, u32)>, u32, u32);

/// `_classDef_bytes`: the size of the smaller ClassDef format for some classes.
fn class_def_bytes(class_data: &[ClassData], class_ids: &[usize]) -> usize {
	let Some(&first) = class_ids.first() else {
		return 0;
	};
	let (first_ranges, mut min_id, mut max_id) = (
		&class_data[first].0,
		class_data[first].1,
		class_data[first].2,
	);
	let mut range_count = first_ranges.len();
	for &i in &class_ids[1..] {
		let data = &class_data[i];
		range_count += data.0.len();
		min_id = min_id.min(data.1);
		max_id = max_id.max(data.2);
	}
	let glyph_count = (max_id - min_id + 1) as usize;
	let format1 = 6 + glyph_count * 2;
	let format2 = 4 + range_count * 6;
	format1.min(format2)
}

/// A set of indices, as the bits of a Python int.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
struct BitSet(Vec<u64>);

impl BitSet {
	fn single(i: usize) -> Self {
		let mut v = vec![0; i / 64 + 1];
		v[i / 64] = 1 << (i % 64);
		BitSet(v)
	}

	fn all(n: usize) -> Self {
		let mut v = vec![u64::MAX; n / 64];
		if !n.is_multiple_of(64) {
			v.push((1 << (n % 64)) - 1);
		}
		BitSet(v)
	}

	fn or(&self, other: &BitSet) -> BitSet {
		let (long, short) = if self.0.len() >= other.0.len() {
			(self, other)
		} else {
			(other, self)
		};
		let mut v = long.0.clone();
		for (a, b) in v.iter_mut().zip(&short.0) {
			*a |= b;
		}
		// Keep one representation per set, for hashing.
		while v.last() == Some(&0) {
			v.pop();
		}
		BitSet(v)
	}

	/// `bit_indices`.
	fn indices(&self) -> Vec<usize> {
		let mut out = Vec::new();
		for (w, &word) in self.0.iter().enumerate() {
			let mut bits = word;
			while bits != 0 {
				let b = bits.trailing_zeros() as usize;
				out.push(w * 64 + b);
				bits &= bits - 1;
			}
		}
		out
	}

	fn normalized(mut self) -> Self {
		while self.0.last() == Some(&0) {
			self.0.pop();
		}
		self
	}
}

struct Context {
	lines: Vec<BitSet>,
	class1_sizes: Vec<usize>,
	class1_data: Vec<ClassData>,
	class2_data: Vec<ClassData>,
	value_format1_bytes: usize,
	value_format2_bytes: usize,
}

/// `Cluster`: a set of class1 lines that would become one subtable.
struct Cluster {
	indices_bitmask: BitSet,
	indices: Vec<usize>,
	cost: usize,
}

impl Cluster {
	fn new(ctx: &Context, mask: BitSet) -> Cluster {
		let indices = mask.indices();
		let column_mask = indices
			.iter()
			.fold(BitSet::default(), |acc, &i| acc.or(&ctx.lines[i]));
		let column_indices = column_mask.indices();
		let width = column_indices.len() + 1;
		let biggest = indices
			.iter()
			.copied()
			.fold(None, |best: Option<usize>, i| match best {
				Some(b) if ctx.class1_sizes[i] <= ctx.class1_sizes[b] => Some(b),
				_ => Some(i),
			});
		let class_def1_ids: Vec<usize> = indices
			.iter()
			.copied()
			.filter(|&i| Some(i) != biggest)
			.collect();
		let class_def1_bytes = class_def_bytes(&ctx.class1_data, &class_def1_ids);
		let class_def2_bytes = class_def_bytes(&ctx.class2_data, &column_indices);
		let cost = 2 // offset to this subtable in the Lookup
			+ 2 // posFormat
			+ 2 // coverageOffset
			+ coverage_bytes(ctx, &indices)
			+ 2 // valueFormat1
			+ 2 // valueFormat2
			+ 2 // classDef1Offset
			+ class_def1_bytes
			+ 2 // classDef2Offset
			+ class_def2_bytes
			+ 2 // class1Count
			+ 2 // class2Count
			+ (ctx.value_format1_bytes + ctx.value_format2_bytes) * indices.len() * width;
		Cluster {
			indices_bitmask: mask,
			indices,
			cost,
		}
	}
}

/// `Cluster.coverage_bytes`. Like fontTools, the range count misses one.
fn coverage_bytes(ctx: &Context, indices: &[usize]) -> usize {
	let format1 = 4 + indices.iter().map(|&i| ctx.class1_sizes[i]).sum::<usize>() * 2;
	let mut ranges: Vec<(u32, u32)> = indices
		.iter()
		.flat_map(|&i| ctx.class1_data[i].0.iter().copied())
		.collect();
	ranges.sort_unstable();
	let mut merged = 0;
	let mut last: Option<u32> = None;
	for (start, end) in ranges {
		if last.is_some_and(|l| start != l + 1) {
			merged += 1;
		}
		last = Some(end);
	}
	let format2 = 4 + merged * 6;
	format1.min(format2)
}

/// Groups pairs into subtables by agglomerative clustering of the class1
/// lines, trading the cost of more subtables against the size saved
/// (`cluster_pairs_by_class2_coverage_custom_cost`).
fn cluster_pairs_by_class2_coverage_custom_cost(
	glyph_map: &GlyphMap,
	pairs: Pairs,
	compression: u8,
) -> Result<Vec<Pairs>> {
	if pairs.is_empty() {
		// The subtable was empty.
		return Ok(vec![pairs]);
	}
	// Sorted for determinism.
	let mut all_class1: Vec<&Vec<String>> = pairs.keys().map(|k| &k.0).collect();
	all_class1.sort();
	all_class1.dedup();
	let mut all_class2: Vec<&Vec<String>> = pairs.keys().map(|k| &k.1).collect();
	all_class2.sort();
	all_class2.dedup();
	let lines: Vec<BitSet> = all_class1
		.iter()
		.map(|c1| {
			let mut line = BitSet::default();
			for (i, c2) in all_class2.iter().enumerate() {
				if pairs.contains_key(&((*c1).clone(), (*c2).clone())) {
					line = line.or(&BitSet::single(i));
				}
			}
			line.normalized()
		})
		.collect();
	let data = |classes: &[&Vec<String>]| -> Result<Vec<ClassData>> {
		classes
			.iter()
			.map(|cls| {
				let ids = cls
					.iter()
					.map(|n| glyph_map.id(n))
					.collect::<Result<Vec<_>>>()?;
				class_ranges(ids.into_iter())
			})
			.collect()
	};
	let class1_data = data(&all_class1)?;
	let class2_data = data(&all_class2)?;
	let mut format1 = 0u16;
	let mut format2 = 0u16;
	for (v1, v2) in pairs.values() {
		format1 |= v1.as_ref().map_or(0, effective_format);
		format2 |= v2.as_ref().map_or(0, effective_format);
	}
	let ctx = Context {
		class1_sizes: all_class1.iter().map(|c| c.len()).collect(),
		lines,
		class1_data,
		class2_data,
		value_format1_bytes: format1.count_ones() as usize * 2,
		value_format2_bytes: format2.count_ones() as usize * 2,
	};

	let mut cache: HashMap<BitSet, usize> = HashMap::new();
	let mut store: Vec<Cluster> = Vec::new();
	let mut make_cluster = |mask: BitSet, store: &mut Vec<Cluster>| -> usize {
		if let Some(&i) = cache.get(&mask) {
			return i;
		}
		store.push(Cluster::new(&ctx, mask.clone()));
		cache.insert(mask, store.len() - 1);
		store.len() - 1
	};

	let n = ctx.lines.len();
	// One cluster per line to start with.
	let mut clusters: Vec<usize> = (0..n)
		.map(|i| make_cluster(BitSet::single(i), &mut store))
		.collect();
	// The cost of one cluster with everything.
	let all = make_cluster(BitSet::all(n), &mut store);
	let cost_before_splitting = store[all].cost;
	log::debug!("        len(clusters) = {}", clusters.len());

	while clusters.len() > 1 {
		let mut lowest: Option<i64> = None;
		let mut best = (0, 0, 0);
		for i in 0..clusters.len() {
			for j in i + 1..clusters.len() {
				let mask = store[clusters[i]]
					.indices_bitmask
					.or(&store[clusters[j]].indices_bitmask);
				let merged = make_cluster(mask, &mut store);
				let change = store[merged].cost as i64
					- store[clusters[i]].cost as i64
					- store[clusters[j]].cost as i64;
				if lowest.is_none_or(|l| change < l) {
					lowest = Some(change);
					best = (i, j, merged);
				}
			}
		}
		let Some(lowest) = lowest else {
			return Err(Error::Assertion("lowest_cost_change is not None".into()));
		};
		let (best_cluster, best_other, best_merged) = best;
		// A merge that still reduces the size is always done; otherwise the
		// compression level decides whether to stop.
		if lowest > 0 {
			let cost_after_splitting: usize = clusters.iter().map(|&c| store[c].cost).sum();
			// So that after = before * (1 - size_reduction).
			let size_reduction = 1.0 - cost_after_splitting as f64 / cost_before_splitting as f64;
			// Level 1 accepts one more subtable for a 50% size reduction, level 5
			// five (like packTab).
			let mut max_new_subtables = -(1.0 - size_reduction).log2() * compression as f64;
			log::debug!(
				"            len(clusters) = {:3}    size_reduction={size_reduction:5.2}    max_new_subtables={max_new_subtables}",
				clusters.len()
			);
			if compression == 9 {
				// Level 9 creates any number of subtables.
				max_new_subtables = clusters.len() as f64;
			}
			if clusters.len() as f64 <= max_new_subtables + 1.0 {
				break;
			}
		}
		clusters.remove(best_other);
		clusters[best_cluster] = best_merged;
	}

	// Turn the clusters back into pairs.
	let mut by_class1: IndexMap<&Vec<String>, Pairs> = IndexMap::new();
	for (pair, values) in &pairs {
		by_class1
			.entry(&pair.0)
			.or_default()
			.insert(pair.clone(), values.clone());
	}
	let mut groups = Vec::with_capacity(clusters.len());
	for &c in &clusters {
		let mut group = Pairs::new();
		for &i in &store[c].indices {
			if let Some(p) = by_class1.get(all_class1[i]) {
				group.extend(p.iter().map(|(k, v)| (k.clone(), v.clone())));
			}
		}
		groups.push(group);
	}
	Ok(groups)
}
