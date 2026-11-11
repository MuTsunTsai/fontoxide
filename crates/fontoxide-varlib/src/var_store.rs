//! `OnlineVarStoreBuilder` of `fontTools.varLib.varStore`. The other operations on stores
//! are in [`fontoxide_ttlib::ot::var_store`], and the instancer in
//! [`fontoxide_ttlib::var`].

use std::collections::{BTreeMap, HashMap};

use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::ot::var_store::{
	build_var_data, build_var_region, build_var_region_list, build_var_store, calculate_num_shorts,
};
use fontoxide_ttlib::{Error, Result};

use crate::models::{Support, VariationModel};

/// `_getLocationKey`: for comparing, a zero of either sign is the same.
fn location_key(region: &Support) -> Vec<(String, [u64; 3])> {
	let mut key: Vec<(String, [u64; 3])> = region
		.iter()
		.map(|(axis, (lower, peak, upper))| {
			let bits = |v: f64| (v + 0.0).to_bits();
			(axis.clone(), [bits(*lower), bits(*peak), bits(*upper)])
		})
		.collect();
	key.sort();
	key
}

/// A delta as an integer, as `VarData` needs them.
fn delta_as_int(delta: f64) -> Result<i64> {
	if delta.fract() != 0.0 {
		return Err(Error::ttlib(format!(
			"TypeError: delta {delta} is not an integer"
		)));
	}
	Ok(delta as i64)
}

/// `OnlineVarStoreBuilder`.
#[derive(Debug)]
pub struct OnlineVarStoreBuilder {
	axis_tags: Vec<String>,
	region_map: HashMap<Vec<(String, [u64; 3])>, u16>,
	store: ot::VarStore,
	/// `_data`: the index in the store of the data being filled.
	data: Option<usize>,
	model: Option<VariationModel>,
	supports: Vec<Support>,
	var_data_indices: HashMap<Vec<u16>, usize>,
	var_data_caches: HashMap<Vec<u16>, HashMap<Vec<i64>, u64>>,
	/// `_cache`: the key in `var_data_caches`.
	cache: Option<Vec<u16>>,
	outer: usize,
}

impl OnlineVarStoreBuilder {
	/// `OnlineVarStoreBuilder(axisTags)`.
	pub fn new(axis_tags: Vec<String>) -> Result<Self> {
		let region_list = build_var_region_list(&[], &axis_tags)?;
		Ok(OnlineVarStoreBuilder {
			axis_tags,
			region_map: HashMap::new(),
			store: build_var_store(region_list, Vec::new()),
			data: None,
			model: None,
			supports: Vec::new(),
			var_data_indices: HashMap::new(),
			var_data_caches: HashMap::new(),
			cache: None,
			outer: 0,
		})
	}

	/// `setModel`.
	pub fn set_model(&mut self, model: &VariationModel) {
		self.set_supports(&model.supports);
		self.model = Some(model.clone());
	}

	/// `setSupports`.
	pub fn set_supports(&mut self, supports: &[Support]) {
		self.model = None;
		self.supports = supports.to_vec();
		// Drop the support of the base master.
		if self.supports.first().is_some_and(Vec::is_empty) {
			self.supports.remove(0);
		}
		self.cache = None;
		self.data = None;
	}

	/// `finish`.
	pub fn finish(mut self, optimize: bool) -> ot::VarStore {
		if let Some(list) = &mut self.store.var_region_list {
			list.region_count = Some(list.region.len() as u32);
		}
		self.store.var_data_count = Some(self.store.var_data.len() as u32);
		for data in self.store.var_data.iter_mut().flatten() {
			data.item_count = Some(data.item.len() as u32);
			calculate_num_shorts(data, optimize);
		}
		self.store
	}

	/// `_add_VarData`.
	fn add_var_data(&mut self) -> Result<()> {
		let mut region_indices = Vec::with_capacity(self.supports.len());
		for region in &self.supports {
			let key = location_key(region);
			let index = match self.region_map.get(&key) {
				Some(&index) => index,
				None => {
					let support: BTreeMap<String, (f64, f64, f64)> =
						region.iter().cloned().collect();
					let var_region = build_var_region(&support, &self.axis_tags)?;
					let list = self
						.store
						.var_region_list
						.as_mut()
						.ok_or_else(|| Error::ttlib("no VarRegionList"))?;
					let index = list.region.len() as u16;
					list.region.push(var_region);
					self.region_map.insert(key, index);
					index
				}
			};
			region_indices.push(index);
		}

		// Check if we have one already...
		let existing = self.var_data_indices.get(&region_indices).copied();
		if let Some(index) = existing {
			self.outer = index;
			self.data = Some(index);
			self.cache = Some(region_indices.clone());
			let items = self.store.var_data[index]
				.as_ref()
				.map_or(0, |d| d.item.len());
			// Otherwise this is full, and needs a new one.
			if items < 0xFFFF {
				return Ok(());
			}
		}

		self.outer = self.store.var_data.len();
		self.data = Some(self.outer);
		self.store.var_data.push(Some(Box::new(build_var_data(
			region_indices.clone(),
			Vec::new(),
			false,
		))));
		self.var_data_indices
			.insert(region_indices.clone(), self.outer);
		self.var_data_caches
			.entry(region_indices.clone())
			.or_default();
		self.cache = Some(region_indices);
		Ok(())
	}

	/// `storeMasters`: the base value and the variation index of the deltas, with the
	/// default `round=round`.
	pub fn store_masters(&mut self, master_values: &[f64]) -> Result<(f64, u64)> {
		self.store_masters_rounded(master_values, f64::round_ties_even)
	}

	/// `storeMasters(master_values, round=round)`.
	pub fn store_masters_rounded(
		&mut self,
		master_values: &[f64],
		round: impl Fn(f64) -> f64,
	) -> Result<(f64, u64)> {
		let model = self.model.as_ref().ok_or_else(|| {
			Error::ttlib("AttributeError: 'NoneType' object has no attribute 'getDeltas'")
		})?;
		let deltas = model.get_deltas_rounded(master_values, round)?;
		let (base, deltas) = deltas
			.split_first()
			.ok_or_else(|| Error::ttlib("IndexError: pop from empty list"))?;
		let base = *base;
		let deltas = deltas.to_vec();
		Ok((base, self.store_deltas_rounded(&deltas, |v| v)?))
	}

	/// `storeDeltas`, with the default `round=round`.
	pub fn store_deltas(&mut self, deltas: &[f64]) -> Result<u64> {
		self.store_deltas_rounded(deltas, f64::round_ties_even)
	}

	/// `storeDeltas(deltas, round=round)`.
	pub fn store_deltas_rounded(
		&mut self,
		deltas: &[f64],
		round: impl Fn(f64) -> f64,
	) -> Result<u64> {
		let deltas: Vec<i64> = deltas
			.iter()
			.map(|d| delta_as_int(round(*d)))
			.collect::<Result<_>>()?;
		self.store_ints(&deltas)
	}

	fn store_ints(&mut self, deltas: &[i64]) -> Result<u64> {
		let deltas = if deltas.len() == self.supports.len() + 1 {
			&deltas[1..]
		} else if deltas.len() == self.supports.len() {
			deltas
		} else {
			return Err(Error::Assertion(
				"len(deltas) == len(self._supports)".into(),
			));
		};

		if self.data.is_none() {
			self.add_var_data()?;
		}
		let (Some(data_index), Some(cache_key)) = (self.data, self.cache.clone()) else {
			return Err(Error::ttlib("no VarData"));
		};
		if let Some(&var_idx) = self
			.var_data_caches
			.get(&cache_key)
			.and_then(|c| c.get(deltas))
		{
			return Ok(var_idx);
		}

		let inner = self.store.var_data[data_index]
			.as_ref()
			.map_or(0, |d| d.item.len());
		if inner == 0xFFFF {
			// Full array. Start new one.
			self.add_var_data()?;
			return self.store_ints(deltas);
		}
		// `addItem`.
		if let Some(data) = self.store.var_data[data_index].as_mut() {
			data.item.push(deltas.to_vec());
			data.item_count = Some(data.item.len() as u32);
		}

		let var_idx = ((self.outer as u64) << 16) + inner as u64;
		self.var_data_caches
			.entry(cache_key)
			.or_default()
			.insert(deltas.to_vec(), var_idx);
		Ok(var_idx)
	}
}

const HELP: &str = "\
usage: varLib.varStore [-h] [--quantization QUANTIZATION] fontfile [outfile]

Optimize a font's GDEF variation store

positional arguments:
  fontfile
  outfile

options:
  -h, --help            show this help message and exit
  --quantization QUANTIZATION
";

const COMMAND: fontoxide_misc::argparse::Command = {
	use fontoxide_misc::argparse::{Argument, Nargs, Ty};
	fontoxide_misc::argparse::Command {
		prog: "varLib.varStore",
		module: "varLib.varStore",
		help: HELP,
		arguments: &[
			Argument::help(),
			Argument::option(&["--quantization"]).ty(Ty::Int),
			Argument::positional("fontfile"),
			Argument::positional("outfile").nargs(Nargs::Optional),
		],
		required_groups: &[],
	}
};

/// The size of a store compiled on its own (`store.compile(writer, font)`,
/// `len(writer.getAllData())`).
fn compiled_size(store: &ot::VarStore, font: &mut fontoxide_ttlib::Font) -> Result<usize> {
	use fontoxide_ttlib::ot::table::OtTable;
	let mut graph = fontoxide_ttlib::ot::WriterGraph::new(fontoxide_ttlib::Tag::new(b"GDEF"));
	store.compile(&mut graph.root(), font)?;
	Ok(graph.get_all_data()?.len())
}

/// `Object_remap_device_varidxes`: maps the variation indices of the `Device`
/// tables (`DeltaFormat` 0x8000; an unknown index is a `KeyError`) and of the
/// format 2 condition tables (an unknown index becomes `NO_VARIATION_INDEX`).
fn remap_device_varidxes<T: fontoxide_ttlib::ot::table::OtTable>(
	table: &mut T,
	map: &HashMap<u64, u64>,
) -> Result<()> {
	use fontoxide_ttlib::ot::varc::NO_VARIATION_INDEX;
	table.visit_mut(&mut |x| {
		if let Some(d) = x.downcast_mut::<ot::Device>() {
			if d.delta_format == 0x8000 {
				let old = (u64::from(d.start_size) << 16) + u64::from(d.end_size);
				let new = *map
					.get(&old)
					.ok_or_else(|| Error::ttlib(format!("KeyError: {old}")))?;
				d.start_size = (new >> 16) as u16;
				d.end_size = (new & 0xFFFF) as u16;
			}
		} else if let Some(c) = x.downcast_mut::<ot::ConditionTableFormat2>() {
			let new = map
				.get(&u64::from(c.var_idx))
				.copied()
				.unwrap_or(NO_VARIATION_INDEX);
			c.var_idx = new as u32;
		}
		Ok(())
	})
}

/// `main`: optimizes the variation store of the `GDEF` table of a font, printing
/// its size before and after, and saves the font to `outfile` if given, with the
/// variation indices of `GDEF`, `GSUB`, `GPOS`, `MATH` and `JSTF` remapped.
/// Returns the exit code.
pub fn main(args: &[&str]) -> crate::errors::Result<i32> {
	run(
		args,
		&mut std::io::stdout().lock(),
		&mut std::io::stderr().lock(),
	)
}

/// [`main`], writing the sizes and the help to `out`, and the usage errors to
/// `err`.
pub fn run(
	args: &[&str],
	out: &mut dyn std::io::Write,
	err: &mut dyn std::io::Write,
) -> crate::errors::Result<i32> {
	use fontoxide_misc::argparse;
	use fontoxide_ttlib::tables::{Gdef, Gpos, Gsub, Jstf, Math};
	use fontoxide_ttlib::{Font, ReadOptions, WriteOptions};

	let options = match COMMAND.parse_args(args, out, err)? {
		Ok(options) => options,
		Err(code) => return Ok(code),
	};
	let quantization = match options.str("--quantization") {
		Some(q) => {
			argparse::parse_int("--quantization", q).map_err(crate::errors::VarLibError::Usage)?
		}
		None => 1,
	};
	let quantization = i64::try_from(quantization)
		.map_err(|_| Error::value("OverflowError: Python int too large to convert"))?;
	let fontfile = options.str("fontfile").unwrap_or_default();
	let outfile = options.str("outfile");

	let mut font = Font::read(std::fs::read(fontfile)?, ReadOptions::default())?;
	let mut store = font
		.require::<Gdef>()?
		.table
		.var_store
		.take()
		.ok_or_else(|| {
			Error::ttlib("AttributeError: 'NoneType' object has no attribute 'compile'")
		})?;

	let size = compiled_size(&store, &mut font)?;
	writeln!(out, "Before: {size:>7} bytes")?;

	let varidx_map =
		fontoxide_ttlib::ot::var_store::optimize_quantized(&mut store, true, quantization)?;

	let size = compiled_size(&store, &mut font)?;
	writeln!(out, "After:  {size:>7} bytes")?;
	font.require::<Gdef>()?.table.var_store = Some(store);

	if let Some(outfile) = outfile {
		let map = varidx_map;
		remap_device_varidxes(&mut font.require::<Gdef>()?.table, &map)?;
		if let Some(t) = font.get::<Gsub>()? {
			remap_device_varidxes(&mut t.table, &map)?;
		}
		if let Some(t) = font.get::<Gpos>()? {
			remap_device_varidxes(&mut t.table, &map)?;
		}
		if let Some(t) = font.get::<Math>()? {
			remap_device_varidxes(&mut t.table, &map)?;
		}
		if let Some(t) = font.get::<Jstf>()? {
			remap_device_varidxes(&mut t.table, &map)?;
		}
		std::fs::write(outfile, font.write(&WriteOptions::default())?)?;
	}
	Ok(0)
}
