//! Port of fontTools' `Tests/varLib/varStore_test.py`.

#![allow(clippy::type_complexity)]

use std::collections::{BTreeMap, BTreeSet};

use fontoxide_misc::round_tools::ot_round;
use fontoxide_subset::varstore::{collect_device_varidxes, remap_device_varidxes};
use fontoxide_ttlib::ot::table::OtTable;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::var_store::{
	build_var_data, build_var_region_list, build_var_store, get_extremes, optimize,
	optimize_quantized,
};
use fontoxide_ttlib::ot::varc::NO_VARIATION_INDEX;
use fontoxide_ttlib::ot::{OtReader, WriterGraph};
use fontoxide_ttlib::var::{Location, VarStoreInstancer};
use fontoxide_ttlib::{Font, Tag};
use fontoxide_varlib::models::{DictLocation, Support, VariationModel, support_scalar};
use fontoxide_varlib::var_store::OnlineVarStoreBuilder;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// Python's `random.Random`: the same inputs as `test_conservative_random`, and for the
/// others, which use the unseeded global instance, some reproducible ones.
struct Random {
	state: [u32; 624],
	index: usize,
}

impl Random {
	/// `random.Random(seed)`, for a seed below 2 ** 32.
	fn new(seed: u32) -> Self {
		let mut state = [0u32; 624];
		// init_genrand(19650218)
		state[0] = 19650218;
		for i in 1..624 {
			state[i] = 1812433253u32
				.wrapping_mul(state[i - 1] ^ (state[i - 1] >> 30))
				.wrapping_add(i as u32);
		}
		// init_by_array([seed])
		let (mut i, mut j) = (1, 0);
		for _ in 0..624 {
			state[i] = (state[i] ^ (state[i - 1] ^ (state[i - 1] >> 30)).wrapping_mul(1664525))
				.wrapping_add(seed)
				.wrapping_add(j);
			i += 1;
			j = 0;
			if i >= 624 {
				state[0] = state[623];
				i = 1;
			}
		}
		for _ in 0..623 {
			state[i] = (state[i] ^ (state[i - 1] ^ (state[i - 1] >> 30)).wrapping_mul(1566083941))
				.wrapping_sub(i as u32);
			i += 1;
			if i >= 624 {
				state[0] = state[623];
				i = 1;
			}
		}
		state[0] = 0x8000_0000;
		Random { state, index: 624 }
	}

	fn genrand(&mut self) -> u32 {
		if self.index >= 624 {
			for k in 0..624 {
				let y = (self.state[k] & 0x8000_0000) | (self.state[(k + 1) % 624] & 0x7FFF_FFFF);
				self.state[k] = self.state[(k + 397) % 624]
					^ (y >> 1)
					^ if y & 1 != 0 { 0x9908_B0DF } else { 0 };
			}
			self.index = 0;
		}
		let mut y = self.state[self.index];
		self.index += 1;
		y ^= y >> 11;
		y ^= (y << 7) & 0x9D2C_5680;
		y ^= (y << 15) & 0xEFC6_0000;
		y ^ (y >> 18)
	}

	/// `random()`.
	fn random(&mut self) -> f64 {
		let (a, b) = (self.genrand() >> 5, self.genrand() >> 6);
		(f64::from(a) * 67108864.0 + f64::from(b)) / 9007199254740992.0
	}

	/// `_randbelow(n)`, for a small `n`.
	fn below(&mut self, n: u32) -> u32 {
		let k = 32 - n.leading_zeros();
		loop {
			let r = self.genrand() >> (32 - k);
			if r < n {
				return r;
			}
		}
	}

	/// `randint(a, b)`.
	fn randint(&mut self, a: i64, b: i64) -> i64 {
		a + i64::from(self.below((b - a + 1) as u32))
	}

	/// `uniform(a, b)`.
	fn uniform(&mut self, a: f64, b: f64) -> f64 {
		a + (b - a) * self.random()
	}

	/// `shuffle(x)`.
	fn shuffle<T>(&mut self, x: &mut [T]) {
		for i in (1..x.len()).rev() {
			let j = self.below(i as u32 + 1) as usize;
			x.swap(i, j);
		}
	}
}

/// `round(v, 2)`.
fn round2(v: f64) -> f64 {
	format!("{v:.2}").parse().expect("a float")
}

fn loc(items: &[(&str, f64)]) -> DictLocation {
	items.iter().map(|&(k, v)| (k.to_owned(), v)).collect()
}

fn fvar_location(location: &DictLocation) -> Location {
	location.iter().cloned().collect()
}

/// `sorted({k for loc in locations for k in loc})`.
fn axis_tags(locations: &[DictLocation]) -> Vec<String> {
	let tags: BTreeSet<&String> = locations.iter().flatten().map(|e| &e.0).collect();
	tags.into_iter().cloned().collect()
}

/// `store.compile(writer, dummyFont); writer.getAllData()`.
fn compile(store: &ot::VarStore) -> TestResult<Vec<u8>> {
	let mut font = Font::new();
	let mut graph = WriterGraph::new(Tag::new(b"test"));
	store.compile(&mut graph.root(), &mut font)?;
	Ok(graph.get_all_data()?)
}

fn decompile(data: &[u8]) -> TestResult<ot::VarStore> {
	let mut font = Font::new();
	let mut reader = OtReader::new(data, Tag::new(b"test"));
	Ok(ot::VarStore::decompile(&mut reader, &mut font)?)
}

fn online_var_store_builder(
	locations: &[DictLocation],
	master_values: &mut [Vec<f64>],
	random: &mut Random,
) -> TestResult {
	let tags = axis_tags(locations);
	let model = VariationModel::new(locations, None, false, None)?;
	let mut builder = OnlineVarStoreBuilder::new(tags.clone())?;
	builder.set_model(&model);
	let mut var_idxs = Vec::new();
	// shuffle input order to ensure optimizer produces stable results
	random.shuffle(master_values);
	for masters in master_values.iter() {
		let (_, var_idx) = builder.store_masters(masters)?;
		var_idxs.push(var_idx);
	}

	let mut store = builder.finish(true);
	let mapping = optimize(&mut store, true)?;
	let var_idxs: Vec<u64> = var_idxs.iter().map(|v| mapping[v]).collect();

	let data = compile(&store)?;
	let store = decompile(&data)?;

	let mut instancer = VarStoreInstancer::new(Some(&store), &tags, Location::new());
	for (masters, &var_idx) in master_values.iter().zip(&var_idxs) {
		let base = masters[0];
		for (&expected, location) in masters.iter().zip(locations) {
			instancer.set_location(fvar_location(location));
			let value = base + instancer.get(var_idx)?;
			assert_eq!(expected, value, "{masters:?} at {location:?}");
		}
	}
	Ok(())
}

#[test]
fn test_online_var_store_builder() -> TestResult {
	let two = [loc(&[]), loc(&[("a", 1.0)])];
	let four = [
		loc(&[]),
		loc(&[("a", 1.0)]),
		loc(&[("b", 1.0)]),
		loc(&[("a", 1.0), ("b", 1.0)]),
	];
	let rows = |r: &[[f64; 4]]| r.iter().map(|v| v.to_vec()).collect::<Vec<_>>();
	for seed in 0..8 {
		let mut random = Random::new(seed);
		online_var_store_builder(
			&two,
			&mut [
				vec![10.0, 10.0], // Test NO_VARIATION_INDEX
				vec![100.0, 2000.0],
				vec![100.0, 22000.0],
			],
			&mut random,
		)?;
		online_var_store_builder(
			&four,
			&mut rows(&[
				[10.0, 20.0, 40.0, 60.0],
				[100.0, 2000.0, 400.0, 6000.0],
				[7100.0, 22000.0, 4000.0, 30000.0],
			]),
			&mut random,
		)?;
		online_var_store_builder(
			&two,
			&mut [vec![10.0, 20.0], vec![42000.0, 100.0], vec![100.0, 52000.0]],
			&mut random,
		)?;
		online_var_store_builder(
			&four,
			&mut rows(&[
				[10.0, 20.0, 40.0, 60.0],
				[40000.0, 42000.0, 400.0, 6000.0],
				[100.0, 22000.0, 4000.0, 173000.0],
			]),
			&mut random,
		)?;
		let mut rand_row = |low: i64, high: i64| -> Vec<f64> {
			(0..4).map(|_| random.randint(low, high) as f64).collect()
		};
		let mut master_values = vec![
			rand_row(-128, 127),
			rand_row(-128, 127),
			rand_row(-128, 127),
			rand_row(-32768, 32767),
			rand_row(-32768, 32767),
			rand_row(-32768, 32767),
		];
		online_var_store_builder(&four, &mut master_values, &mut random)?;
	}
	Ok(())
}

#[test]
fn test_var_store_instancer_out_of_range_index_returns_zero() -> TestResult {
	let var_data = build_var_data(Vec::new(), vec![Vec::new()], false);
	let tags = vec!["wght".to_owned()];
	let store = build_var_store(build_var_region_list(&[], &tags)?, vec![var_data]);
	let instancer = VarStoreInstancer::new(Some(&store), &tags, Location::new());

	assert_eq!(instancer.get(0)?, 0.0);
	assert_eq!(instancer.get(1)?, 0.0); // inner index beyond the single item
	assert_eq!(instancer.get(1 << 16)?, 0.0); // outer index beyond the single VarData
	Ok(())
}

fn condition_variations(lookup_variations: bool) -> ot::FeatureVariations {
	let boxed = |t: ot::ConditionTable| Some(Box::new(t));
	let value = ot::ConditionTable::Format2(ot::ConditionTableFormat2 {
		default_value: -1,
		var_idx: 3,
	});
	let constant = ot::ConditionTable::Format2(ot::ConditionTableFormat2 {
		default_value: 1,
		var_idx: NO_VARIATION_INDEX as u32,
	});
	let negate = ot::ConditionTable::Format5(ot::ConditionTableFormat5 {
		condition_table: boxed(value.clone()),
	});
	let disjunction = ot::ConditionTable::Format4(ot::ConditionTableFormat4 {
		condition_count: Some(2),
		condition_table: vec![boxed(negate), boxed(constant)],
	});
	let conjunction = ot::ConditionTable::Format3(ot::ConditionTableFormat3 {
		condition_count: Some(3),
		condition_table: vec![None, boxed(value), boxed(disjunction)],
	});

	let mut variations = ot::FeatureVariations {
		version: 0x0001_0001,
		feature_variation_count: Some(0),
		lookup_variation_count: Some(0),
		..Default::default()
	};
	if lookup_variations {
		variations.lookup_variation_record = vec![ot::LookupVariationRecord {
			feature_index: 0,
			feature_lookups_table: Some(Box::new(ot::FeatureLookupsTable {
				version: 0x0001_0000,
				flags: 0,
				lookup_condition_count: Some(1),
				lookup_condition_record: vec![ot::LookupConditionRecord {
					condition_table: boxed(conjunction),
					lookup_index_list: None,
				}],
			})),
		}];
		variations.lookup_variation_count = Some(1);
	} else {
		variations.feature_variation_record = vec![ot::FeatureVariationRecord {
			condition_set: Some(Box::new(ot::ConditionSet {
				condition_count: Some(1),
				condition_table: vec![boxed(conjunction)],
			})),
			feature_table_substitution: None,
		}];
		variations.feature_variation_count = Some(1);
	}
	variations
}

fn check_condition_varidxes<T: OtTable>(mut table: T) -> TestResult {
	let mut used = BTreeSet::new();
	collect_device_varidxes(&mut table, &mut used)?;
	assert_eq!(used, BTreeSet::from([3, NO_VARIATION_INDEX]));
	// The same value condition occurs twice. It must be remapped only once.
	remap_device_varidxes(
		&mut table,
		&BTreeMap::from([(3, 1), (1, 0), (NO_VARIATION_INDEX, NO_VARIATION_INDEX)]),
	)?;
	// `value.VarIdx == 1`, and `constant.VarIdx == NO_VARIATION_INDEX`. There are no shared
	// objects here, so all copies of the value condition have to be 1, and none 0.
	let mut used = BTreeSet::new();
	collect_device_varidxes(&mut table, &mut used)?;
	assert_eq!(used, BTreeSet::from([1, NO_VARIATION_INDEX]));
	Ok(())
}

#[test]
fn test_condition_varidxes_collected_and_remapped() -> TestResult {
	for lookup_variations in [false, true] {
		check_condition_varidxes(ot::GSUB {
			version: 0x0001_0001,
			feature_variations: Some(Box::new(condition_variations(lookup_variations))),
			..Default::default()
		})?;
		check_condition_varidxes(ot::GPOS {
			version: 0x0001_0001,
			feature_variations: Some(Box::new(condition_variations(lookup_variations))),
			..Default::default()
		})?;
	}
	Ok(())
}

/// `[{i: i / 16384.0} for i in range(numRegions)]`.
fn region_locations(num_regions: usize) -> Vec<DictLocation> {
	(0..num_regions)
		.map(|i| vec![(i.to_string(), i as f64 / 16384.0)])
		.collect()
}

fn stored(num_regions: usize, var_data: &[Vec<f64>]) -> TestResult<ot::VarStore> {
	let locations = region_locations(num_regions);
	let model = VariationModel::new(&locations, None, false, None)?;
	let mut builder = OnlineVarStoreBuilder::new(axis_tags(&locations))?;
	builder.set_model(&model);
	for data in var_data {
		builder.store_masters(data)?;
	}
	Ok(builder.finish(true))
}

#[test]
fn test_optimize() -> TestResult {
	/// `{3: 300}`.
	const SPARSE: [i64; 5] = [0, 0, 0, 300, 0];
	type Case<'a> = (usize, &'a [&'a [i64]], usize, usize);
	let cases: &[Case<'_>] = &[
		(5, &[&[10, 10, 0, 0, 20], &SPARSE], 1, 126),
		(
			5,
			&[
				&[10, 10, 0, 0, 20],
				&[10, 11, 0, 0, 20],
				&[10, 12, 0, 0, 20],
				&[10, 13, 0, 0, 20],
				&SPARSE,
			],
			1,
			175,
		),
		(
			5,
			&[
				&[10, 11, 0, 0, 20],
				&[10, 300, 0, 0, 20],
				&[10, 301, 0, 0, 20],
				&[10, 302, 0, 0, 20],
				&[10, 303, 0, 0, 20],
				&[10, 304, 0, 0, 20],
			],
			1,
			180,
		),
		(
			5,
			&[
				&[0, 11, 12, 0, 20],
				&[0, 13, 12, 0, 20],
				&[0, 14, 12, 0, 20],
				&[0, 15, 12, 0, 20],
				&[0, 16, 12, 0, 20],
				&[10, 300, 0, 0, 20],
				&[10, 301, 0, 0, 20],
				&[10, 302, 0, 0, 20],
				&[10, 303, 0, 0, 20],
				&[10, 304, 0, 0, 20],
			],
			1,
			200,
		),
		(
			5,
			&[
				&[0, 11, 12, 0, 20],
				&[0, 13, 12, 0, 20],
				&[0, 14, 12, 0, 20],
				&[0, 15, 12, 0, 20],
				&[0, 16, 12, 0, 20],
				&[0, 17, 12, 0, 20],
				&[0, 18, 12, 0, 20],
				&[0, 19, 12, 0, 20],
				&[0, 20, 12, 0, 20],
				&[10, 300, 0, 0, 20],
				&[10, 301, 0, 0, 20],
				&[10, 302, 0, 0, 20],
				&[10, 303, 0, 0, 20],
				&[10, 304, 0, 0, 20],
			],
			2,
			218,
		),
		(3, &[&[10, 10, 10]], 0, 12),
	];
	for seed in 0..8 {
		let mut random = Random::new(seed);
		for (num_regions, var_data, expected_num_var_data, expected_bytes) in cases {
			let mut var_data: Vec<Vec<f64>> = var_data
				.iter()
				.map(|r| r.iter().map(|&v| v as f64).collect())
				.collect();
			random.shuffle(&mut var_data);
			let mut store = stored(*num_regions, &var_data)?;
			optimize(&mut store, true)?;

			assert_eq!(store.var_data.len(), *expected_num_var_data, "{var_data:?}");
			assert_eq!(compile(&store)?.len(), *expected_bytes, "{var_data:?}");
		}
	}
	Ok(())
}

#[test]
fn test_quantize() -> TestResult {
	let rows: [[f64; 5]; 10] = [
		[0.0, 11.0, 12.0, 0.0, 20.0],
		[0.0, 13.0, 12.0, 0.0, 20.0],
		[0.0, 14.0, 12.0, 0.0, 20.0],
		[0.0, 15.0, 12.0, 0.0, 20.0],
		[0.0, 16.0, 12.0, 0.0, 20.0],
		[10.0, 300.0, 0.0, 0.0, 20.0],
		[10.0, 301.0, 0.0, 0.0, 20.0],
		[10.0, 302.0, 0.0, 0.0, 20.0],
		[10.0, 303.0, 0.0, 0.0, 20.0],
		[10.0, 304.0, 0.0, 0.0, 20.0],
	];
	let cases = [
		(1, 200),
		(2, 180),
		(3, 170),
		(4, 175),
		(8, 170),
		(32, 92),
		(64, 56),
	];
	for seed in 0..8 {
		let mut random = Random::new(seed);
		for (quantization, expected_bytes) in cases {
			let mut var_data: Vec<Vec<f64>> = rows.iter().map(|r| r.to_vec()).collect();
			random.shuffle(&mut var_data);
			let mut store = stored(5, &var_data)?;
			optimize_quantized(&mut store, true, quantization)?;
			assert_eq!(compile(&store)?.len(), expected_bytes, "{quantization}");
		}
	}
	Ok(())
}

#[test]
fn test_optimize_overflow() -> TestResult {
	let locations = [loc(&[("wght", 0.0)]), loc(&[("wght", 0.5)])];
	let model = VariationModel::new(&locations, None, false, None)?;
	let mut builder = OnlineVarStoreBuilder::new(vec!["wght".to_owned()])?;
	builder.set_model(&model);

	let mut var_data: Vec<i64> = (0..0xFFFF * 2).collect();
	Random::new(1).shuffle(&mut var_data);
	for data in var_data {
		builder.store_masters_rounded(&[0.0, data as f64], |v| v)?;
	}

	let mut store = builder.finish(true);
	optimize(&mut store, true)?;

	// 5 data-sets:
	// - 0..127: 1-byte dataset
	// - 128..32767: 2-byte dataset
	// - 32768..32768+65535-1: 4-byte dataset
	// - 32768+65535..65535+65535-1: 4-byte dataset
	assert_eq!(store.var_data.len(), 4);
	Ok(())
}

// `GetExtremesTest`.

type Tent = (f64, f64, f64);

fn region(items: &[(&str, Tent)]) -> Support {
	items.iter().map(|&(k, v)| (k.to_owned(), v)).collect()
}

fn build_store(regions: &[Support], deltas: &[i64], axes: &[String]) -> TestResult<ot::VarStore> {
	let supports: Vec<BTreeMap<String, Tent>> = regions
		.iter()
		.map(|r| r.iter().cloned().collect())
		.collect();
	let region_list = build_var_region_list(&supports, axes)?;
	let var_data = build_var_data(
		(0..regions.len() as u16).collect(),
		vec![deltas.to_vec()],
		false,
	);
	Ok(build_var_store(region_list, vec![var_data]))
}

type Limits = BTreeMap<String, (f64, f64, f64)>;

/// What `getExtremes` needs of the limits: the minimum and the maximum.
fn min_max(limits: &Limits) -> BTreeMap<String, (f64, f64)> {
	limits
		.iter()
		.map(|(tag, l)| (tag.clone(), (l.0, l.2)))
		.collect()
}

/// A set of floats, where `-0.0 == 0.0`.
fn add(set: &mut Vec<f64>, v: f64) {
	if !set.contains(&v) {
		set.push(v);
	}
}

/// `_trueExtremes`.
fn true_extremes(
	regions: &[Support],
	deltas: &[i64],
	axes: &[String],
	axis_limits: &Limits,
	identity_axis: Option<usize>,
) -> (i64, i64) {
	let mut breakpoints: Vec<Vec<f64>> = vec![vec![-1.0, 0.0, 1.0]; axes.len()];
	let position = |tag: &str| axes.iter().position(|a| a == tag).expect("an axis");
	let unit = 1.0 / 16384.0;
	for region in regions {
		for (tag, (start, peak, end)) in region {
			let set = &mut breakpoints[position(tag)];
			add(set, *start);
			add(set, *peak);
			add(set, *end);
			if start == peak {
				add(set, peak - unit);
			}
			if peak == end {
				add(set, peak + unit);
			}
		}
	}
	for (tag, set) in axes.iter().zip(&mut breakpoints) {
		let (lo, hi) = axis_limits.get(tag).map_or((-1.0, 1.0), |l| (l.0, l.2));
		let mut clamped = Vec::new();
		for v in set.iter() {
			add(&mut clamped, f64::min(f64::max(*v, lo), hi));
		}
		add(&mut clamped, lo);
		add(&mut clamped, hi);
		clamped.sort_by(f64::total_cmp);
		*set = clamped;
	}
	// `itertools.product`.
	let mut extremes: Option<(i64, i64)> = None;
	let mut counters = vec![0; axes.len()];
	loop {
		let location: DictLocation = axes
			.iter()
			.enumerate()
			.map(|(i, tag)| (tag.clone(), breakpoints[i][counters[i]]))
			.collect();
		let sum: f64 = regions
			.iter()
			.zip(deltas)
			.map(|(region, delta)| support_scalar(&location, region) * *delta as f64)
			.sum();
		let mut value = ot_round(sum);
		if let Some(axis) = identity_axis {
			value += ot_round(location[axis].1 * 16384.0);
		}
		extremes = Some(extremes.map_or((value, value), |(lo, hi)| (lo.min(value), hi.max(value))));
		let mut k = axes.len();
		loop {
			if k == 0 {
				return extremes.expect("a value");
			}
			k -= 1;
			counters[k] += 1;
			if counters[k] < breakpoints[k].len() {
				break;
			}
			counters[k] = 0;
		}
	}
}

/// `_check`.
fn check(
	regions: &[Support],
	deltas: &[i64],
	axes: &[String],
	axis_limits: &Limits,
	identity_axis: Option<usize>,
) -> TestResult<((i64, i64), (i64, i64))> {
	let store = build_store(regions, deltas, axes)?;
	let bound = get_extremes(&store, 0, axes, &min_max(axis_limits), identity_axis)?;
	let truth = true_extremes(regions, deltas, axes, axis_limits, identity_axis);
	assert!(
		bound.0 as f64 <= truth.0 as f64 + 1e-9 && bound.1 as f64 >= truth.1 as f64 - 1e-9,
		"getExtremes {bound:?} does not cover true range {truth:?} for regions={regions:?} \
		 deltas={deltas:?} axisLimits={axis_limits:?} identityAxisIndex={identity_axis:?}"
	);
	Ok((bound, truth))
}

fn strings(tags: &[&str]) -> Vec<String> {
	tags.iter().map(|t| (*t).to_owned()).collect()
}

#[test]
fn test_overlapping_same_axis_tents() -> TestResult {
	// The peak-sampling implementation this replaced missed the extremum of overlapping
	// same-axis tents (it lies at another tent's start/end kink, not at any peak) and
	// returned max=0 here; culling then deleted live gvar regions around wght=0.6.
	let (bound, truth) = check(
		&[
			region(&[("wght", (0.0, 0.4, 0.6))]),
			region(&[("wght", (0.2, 0.5, 0.8))]),
		],
		&[-16384, 8192],
		&strings(&["wght"]),
		&Limits::new(),
		None,
	)?;
	// Small stores take the exact (breakpoint-grid) path.
	let approx = |a: i64, b: i64| (a - b).abs() as f64 <= (b.abs() as f64 * 1e-6).max(1e-12);
	assert!(
		approx(bound.0, truth.0) && approx(bound.1, truth.1),
		"{bound:?} {truth:?}"
	);
	Ok(())
}

#[test]
fn test_one_sided_support_extreme() -> TestResult {
	// A peak=end tent contributes at the peak, then drops to zero at the next F2Dot14
	// coordinate. The breakpoint-only grid sees the two rows cancel at 0.25 and misses the
	// live second row immediately to its right, so the returned upper bound must also cover
	// that grid point.
	let regions = [
		region(&[("wght", (0.0, 0.25, 0.25))]),
		region(&[("wght", (0.0, 0.25, 0.5))]),
	];
	let deltas = [-16384, 16384];
	let axes = strings(&["wght"]);
	let store = build_store(&regions, &deltas, &axes)?;

	let (_, bound_hi) = get_extremes(&store, 0, &axes, &BTreeMap::new(), None)?;
	let location = loc(&[("wght", 4097.0 / 16384.0)]);
	let reachable: f64 = regions
		.iter()
		.zip(deltas)
		.map(|(r, delta)| support_scalar(&location, r) * delta as f64)
		.sum();
	let (_, true_hi) = true_extremes(&regions, &deltas, &axes, &Limits::new(), None);

	assert_eq!(reachable, 16380.0);
	assert_eq!(true_hi as f64, reachable);
	assert!(bound_hi as f64 >= reachable);
	Ok(())
}

#[test]
fn test_identity_axis() -> TestResult {
	check(
		&[
			region(&[("wght", (0.0, 0.5, 1.0))]),
			region(&[("wght", (-1.0, -0.5, 0.0))]),
		],
		&[8000, -4000],
		&strings(&["wght"]),
		&Limits::new(),
		Some(0),
	)?;
	Ok(())
}

#[test]
fn test_pinned_axis_limits() -> TestResult {
	check(
		&[region(&[
			("wght", (0.0, 0.5, 1.0)),
			("opsz", (0.0, 1.0, 1.0)),
		])],
		&[10000],
		&strings(&["wght", "opsz"]),
		&Limits::from([("opsz".to_owned(), (0.0, 0.0, 0.0))]),
		None,
	)?;
	Ok(())
}

#[test]
fn test_conservative_random() -> TestResult {
	let mut rng = Random::new(1234);
	for _ in 0..300 {
		let n_axes = rng.randint(1, 3) as usize;
		let axes: Vec<String> = (0..n_axes).map(|i| format!("ax{i}")).collect();
		let mut regions: Vec<Support> = Vec::new();
		let mut deltas = Vec::new();
		for _ in 0..rng.randint(1, 5) {
			let mut region = Support::new();
			for tag in &axes {
				if rng.random() < 0.6 {
					// OT-valid tent on one side of 0
					let (low, high) = if rng.random() < 0.5 {
						(0.0, 1.0)
					} else {
						(-1.0, 0.0)
					};
					let mut coords: Vec<f64> =
						(0..3).map(|_| round2(rng.uniform(low, high))).collect();
					coords.sort_by(|a, b| a.partial_cmp(b).expect("a number"));
					if coords[1] != 0.0 {
						region.push((tag.clone(), (coords[0], coords[1], coords[2])));
					}
				}
			}
			regions.push(region);
			deltas.push(rng.randint(-16384, 16384));
		}
		let mut axis_limits = Limits::new();
		for tag in &axes {
			let r = rng.random();
			if r < 0.25 {
				axis_limits.insert(tag.clone(), (0.0, 0.0, 0.0)); // pinned private axis
			} else if r < 0.4 {
				let low = round2(rng.uniform(-1.0, 0.0));
				let high = round2(rng.uniform(0.0, 1.0));
				axis_limits.insert(tag.clone(), (low, 0.0, high));
			}
		}
		// `rng.choice([None] + list(range(nAxes)))`.
		let identity_axis = match rng.below(n_axes as u32 + 1) {
			0 => None,
			i => Some(i as usize - 1),
		};
		check(&regions, &deltas, &axes, &axis_limits, identity_axis)?;
	}
	// What Python's generator gets after all of this: the inputs were the same.
	assert_eq!(rng.random(), 0.9140245913896461);
	Ok(())
}

#[test]
fn test_many_axes_falls_back_fast() -> TestResult {
	// A row peaking on many distinct axes explodes the breakpoint grid; getExtremes must
	// fall back to the O(regions x axes) interval bound instead of hanging (the recursion
	// it replaced was exponential). The fallback is looser but still conservative.
	let k = 30;
	let axes: Vec<String> = (0..k).map(|i| format!("a{i:02}")).collect();
	let regions: Vec<Support> = axes
		.iter()
		.map(|tag| vec![(tag.clone(), (0.0, 0.5, 1.0))])
		.collect();
	let deltas = vec![1000; k];
	let store = build_store(&regions, &deltas, &axes)?;
	let (bound_lo, bound_hi) = get_extremes(&store, 0, &axes, &BTreeMap::new(), None)?;
	assert!(bound_lo <= 0 && bound_hi >= k as i64 * 1000); // all peaks reachable at once
	Ok(())
}
