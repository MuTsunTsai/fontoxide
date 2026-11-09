//! Instancing of item variation stores (`_TupleVarStoreAdapter` and
//! `instantiateItemVariationStore`).

use std::collections::{BTreeMap, HashMap, HashSet};

use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::var_store::{
	build_var_data, build_var_region_list, build_var_store, calculate_num_shorts, prune_regions,
	region_support,
};
use fontoxide_ttlib::ot::varc::NO_VARIATION_INDEX;
use fontoxide_ttlib::tables::glyf::Number;
use fontoxide_ttlib::tables::tuple_variation::{Delta, TupleVariation};
use fontoxide_ttlib::{Error, Result};

use super::limits::NormalizedLimits;
use super::tuple::{instantiate_tuple_variation_store, region_key, round_deltas};

type Region = BTreeMap<String, (f64, f64, f64)>;

fn number_of(delta: &Option<Delta>) -> Number {
	match delta {
		Some(Delta::Value(v)) => *v,
		_ => Number::Int(0),
	}
}

fn region_key_of(axes: &Region) -> Vec<(String, [u64; 3])> {
	region_key(&TupleVariation {
		axes: axes.clone(),
		coordinates: Vec::new(),
	})
}

/// `_TupleVarStoreAdapter`: an item variation store as tuple variations.
#[derive(Debug, Clone)]
pub struct TupleVarStore {
	pub regions: Vec<Region>,
	pub axis_order: Vec<String>,
	pub tuple_var_data: Vec<Vec<TupleVariation>>,
	pub item_counts: Vec<usize>,
}

impl TupleVarStore {
	/// `fromItemVarStore`.
	pub fn from_item_var_store(store: &ot::VarStore, axes: &[String]) -> Result<Self> {
		let list = store
			.var_region_list
			.as_ref()
			.ok_or_else(|| Error::ttlib("AttributeError: no attribute 'Region'"))?;
		let regions = list
			.region
			.iter()
			.map(|r| region_support(r, axes))
			.collect::<Result<Vec<_>>>()?;
		let mut tuple_var_data = Vec::with_capacity(store.var_data.len());
		let mut item_counts = Vec::with_capacity(store.var_data.len());
		for data in &store.var_data {
			let data = data
				.as_ref()
				.ok_or_else(|| Error::ttlib("AttributeError: no attribute 'VarRegionIndex'"))?;
			// `zip(*varData.Item)`: the columns, as many as the shortest item has.
			let columns = data.item.iter().map(Vec::len).min().unwrap_or(0);
			let mut variations = Vec::new();
			for (column, &region) in data.var_region_index.iter().take(columns).enumerate() {
				let axes = regions
					.get(usize::from(region))
					.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
				variations.push(TupleVariation {
					axes: axes.clone(),
					coordinates: data
						.item
						.iter()
						.map(|item| Some(Delta::Value(Number::Int(item[column]))))
						.collect(),
				});
			}
			tuple_var_data.push(variations);
			item_counts.push(data.item_count.map_or(data.item.len(), |c| c as usize));
		}
		Ok(TupleVarStore {
			regions,
			axis_order: axes.to_vec(),
			tuple_var_data,
			item_counts,
		})
	}

	/// `fromItemVarStore`, with the items in the given rows, which can have fractions.
	pub fn from_rows(
		store: &ot::VarStore,
		rows: &[Vec<Vec<Number>>],
		axes: &[String],
	) -> Result<Self> {
		let list = store
			.var_region_list
			.as_ref()
			.ok_or_else(|| Error::ttlib("AttributeError: no attribute 'Region'"))?;
		let regions = list
			.region
			.iter()
			.map(|r| region_support(r, axes))
			.collect::<Result<Vec<_>>>()?;
		let mut tuple_var_data = Vec::with_capacity(store.var_data.len());
		let mut item_counts = Vec::with_capacity(store.var_data.len());
		for (data, rows) in store.var_data.iter().zip(rows) {
			let data = data
				.as_ref()
				.ok_or_else(|| Error::ttlib("AttributeError: no attribute 'VarRegionIndex'"))?;
			let columns = rows.iter().map(Vec::len).min().unwrap_or(0);
			let mut variations = Vec::new();
			for (column, &region) in data.var_region_index.iter().take(columns).enumerate() {
				let axes = regions
					.get(usize::from(region))
					.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
				variations.push(TupleVariation {
					axes: axes.clone(),
					coordinates: rows
						.iter()
						.map(|item| Some(Delta::Value(item[column])))
						.collect(),
				});
			}
			tuple_var_data.push(variations);
			item_counts.push(rows.len());
		}
		Ok(TupleVarStore {
			regions,
			axis_order: axes.to_vec(),
			tuple_var_data,
			item_counts,
		})
	}

	/// The items of each data, by item, then by variation (`asItemVarStore`).
	pub fn items(&self) -> Vec<Vec<Vec<Number>>> {
		self.tuple_var_data
			.iter()
			.zip(&self.item_counts)
			.map(|(variations, &count)| {
				(0..count)
					.map(|i| {
						variations
							.iter()
							.map(|v| match v.coordinates.get(i) {
								Some(Some(Delta::Value(n))) => *n,
								_ => Number::Int(0),
							})
							.collect()
					})
					.collect()
			})
			.collect()
	}

	/// `rebuildRegions`: the regions of the variations, keeping the order of the
	/// ones that existed before, then the new ones.
	pub fn rebuild_regions(&mut self) {
		let mut unique: Vec<(Vec<(String, [u64; 3])>, Region)> = Vec::new();
		let mut seen = HashSet::new();
		for var in self.tuple_var_data.iter().flatten() {
			let key = region_key(var);
			if seen.insert(key.clone()) {
				unique.push((key, var.axes.clone()));
			}
		}
		let mut new_regions = Vec::new();
		for region in &self.regions {
			let key = region_key_of(region);
			if seen.remove(&key) {
				new_regions.push(region.clone());
			}
		}
		new_regions.extend(
			unique
				.into_iter()
				.filter(|(key, _)| seen.contains(key))
				.map(|(_, region)| region),
		);
		self.regions = new_regions;
	}

	/// `instantiate`: the default deltas by data and item.
	pub fn instantiate(&mut self, limits: &NormalizedLimits, round: bool) -> Result<Vec<Vec<f64>>> {
		let defaults = self.instantiate_numbers(limits, round)?;
		Ok(defaults
			.iter()
			.map(|d| d.iter().map(|n| n.value()).collect())
			.collect())
	}

	/// `instantiate`, keeping integers apart from the others.
	pub fn instantiate_numbers(
		&mut self,
		limits: &NormalizedLimits,
		round: bool,
	) -> Result<Vec<Vec<Number>>> {
		let mut defaults = Vec::with_capacity(self.tuple_var_data.len());
		for (variations, &count) in self.tuple_var_data.iter_mut().zip(&self.item_counts) {
			let default = instantiate_tuple_variation_store(variations, limits, None, round)?;
			defaults.push(if default.is_empty() {
				vec![Number::Int(0); count]
			} else {
				default.iter().map(number_of).collect()
			});
		}
		// Rebuild the regions whose axes were dropped or limited.
		self.rebuild_regions();
		let pinned = limits.pinned_location();
		self.axis_order.retain(|t| !pinned.contains_key(t));
		Ok(defaults)
	}

	/// `asItemVarStore`.
	pub fn as_item_var_store(&self, calculate_shorts: bool) -> Result<ot::VarStore> {
		let order: Vec<_> = self.regions.iter().map(region_key_of).collect();
		let mut var_datas = Vec::with_capacity(self.tuple_var_data.len());
		for (variations, &count) in self.tuple_var_data.iter().zip(&self.item_counts) {
			let Some(first) = variations.first() else {
				var_datas.push(build_var_data(Vec::new(), vec![Vec::new(); count], true));
				continue;
			};
			if first.coordinates.len() != count {
				return Err(Error::Assertion(
					"len(variations[0].coordinates) == itemCount".into(),
				));
			}
			let mut indices = Vec::with_capacity(variations.len());
			for var in variations {
				let key = region_key(var);
				let index = order
					.iter()
					.position(|o| *o == key)
					.ok_or_else(|| Error::value("region is not in list"))?;
				indices.push(index as u16);
			}
			let mut data = build_var_data(indices, Vec::new(), false);
			data.item = (0..count)
				.map(|i| {
					variations
						.iter()
						.map(|v| match v.coordinates.get(i) {
							Some(Some(Delta::Value(Number::Int(n)))) => *n,
							Some(Some(Delta::Value(Number::Float(f)))) => *f as i64,
							_ => 0,
						})
						.collect()
				})
				.collect();
			data.item_count = Some(count as u32);
			if calculate_shorts {
				calculate_num_shorts(&mut data, false);
			}
			var_datas.push(data);
		}
		let list = build_var_region_list(&self.regions, &self.axis_order)?;
		let mut store = build_var_store(list, var_datas);
		// Remove the unused regions.
		prune_regions(&mut store)?;
		Ok(store)
	}
}

/// The default deltas of an instantiated store, by variation index.
#[derive(Debug, Clone, Default)]
pub struct DefaultDeltas(pub HashMap<u64, f64>);

impl DefaultDeltas {
	pub fn from_array(array: &[Vec<f64>]) -> Self {
		let mut map = HashMap::new();
		for (major, deltas) in array.iter().enumerate() {
			for (minor, &delta) in deltas.iter().enumerate() {
				map.insert(((major as u64) << 16) + minor as u64, delta);
			}
		}
		map.insert(NO_VARIATION_INDEX, 0.0);
		DefaultDeltas(map)
	}

	/// `deltas[varIdx]`.
	pub fn get(&self, index: u64) -> Result<f64> {
		self.0
			.get(&index)
			.copied()
			.ok_or_else(|| Error::ttlib(format!("KeyError: {index}")))
	}
}

/// `instantiateItemVariationStore`: updates the store in place for the limits, and
/// returns the deltas to add to the default instance.
pub fn instantiate_item_variation_store(
	store: &mut ot::VarStore,
	axes: &[String],
	limits: &NormalizedLimits,
) -> Result<DefaultDeltas> {
	let mut tuple_store = TupleVarStore::from_item_var_store(store, axes)?;
	let defaults = tuple_store.instantiate(limits, true)?;
	let new_store = tuple_store.as_item_var_store(true)?;
	store.var_region_list = new_store.var_region_list;
	let new_count = new_store.var_data_count;
	if store.var_data_count.is_some() && store.var_data_count != new_count {
		return Err(Error::Assertion(
			"itemVarStore.VarDataCount == newItemVarStore.VarDataCount".into(),
		));
	}
	store.var_data_count = new_count;
	store.var_data = new_store.var_data;
	Ok(DefaultDeltas::from_array(&defaults))
}

/// `roundDeltas` of all the variations.
pub fn round_all(store: &mut TupleVarStore) {
	store
		.tuple_var_data
		.iter_mut()
		.flatten()
		.for_each(round_deltas);
}
