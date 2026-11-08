//! `hb_hashmap_t<unsigned, unsigned>` (`hb-map.hh`).
//!
//! The repacker's results depend on the iteration order of some of its maps
//! (the first parent of a node, the order nodes of a subgraph are duplicated
//! in), so the open addressing table is reproduced exactly: Knuth's
//! multiplicative hash, a prime modulus for the first bucket, triangular
//! probing, tombstones, and the growth policy.

#[derive(Debug, Clone, Copy, Default)]
struct Item {
	key: u32,
	value: u32,
	hash: u32,
	used: bool,
	real: bool,
}

/// An open addressing hash map from `u32` to `u32`, iterated in bucket order.
#[derive(Debug, Clone)]
pub(crate) struct HbMap {
	successful: bool,
	max_chain_length: u32,
	population: u32,
	occupancy: u32,
	mask: u32,
	prime: u32,
	items: Vec<Item>,
}

impl Default for HbMap {
	fn default() -> Self {
		HbMap {
			successful: true,
			max_chain_length: 0,
			population: 0,
			occupancy: 0,
			mask: 0,
			prime: 0,
			items: Vec::new(),
		}
	}
}

/// `hb_hash` of an unsigned integer.
fn hb_hash(v: u32) -> u32 {
	v.wrapping_mul(2_654_435_761)
}

/// `hb_bit_storage`: the number of bits needed to store `v`.
fn bit_storage(v: u32) -> u32 {
	32 - v.leading_zeros()
}

fn prime_for(shift: u32) -> u32 {
	const PRIME_MOD: [u32; 32] = [
		1,
		2,
		3,
		7,
		13,
		31,
		61,
		127,
		251,
		509,
		1021,
		2039,
		4093,
		8191,
		16381,
		32749,
		65521,
		131_071,
		262_139,
		524_287,
		1_048_573,
		2_097_143,
		4_194_301,
		8_388_593,
		16_777_213,
		33_554_393,
		67_108_859,
		134_217_689,
		268_435_399,
		536_870_909,
		1_073_741_789,
		2_147_483_647,
	];
	PRIME_MOD[(shift as usize).min(PRIME_MOD.len() - 1)]
}

impl HbMap {
	/// Whether an allocation failed (`in_error`).
	pub fn in_error(&self) -> bool {
		!self.successful
	}

	/// The number of keys (`get_population`).
	pub fn population(&self) -> u32 {
		self.population
	}

	/// `is_empty`.
	pub fn is_empty(&self) -> bool {
		self.population == 0
	}

	fn alloc(&mut self, new_population: u32) -> bool {
		if !self.successful {
			return false;
		}
		if self.population > 0x3FFF_FFFF || new_population > 0x3FFF_FFFF {
			self.successful = false;
			return false;
		}
		if new_population != 0
			&& new_population.wrapping_add(new_population / 2) < self.mask
		{
			return true;
		}
		let power = bit_storage(
			self.population
				.max(new_population)
				.wrapping_mul(2)
				.max(4),
		);
		let new_size = 1u32 << power;
		let old_items = std::mem::replace(&mut self.items, vec![Item::default(); new_size as usize]);
		self.population = 0;
		self.occupancy = 0;
		self.mask = new_size - 1;
		self.prime = prime_for(power);
		self.max_chain_length = power * 2;
		for item in old_items {
			if item.real {
				self.set_with_hash(item.key, item.hash, item.value);
			}
		}
		true
	}

	fn set_with_hash(&mut self, key: u32, hash: u32, value: u32) -> bool {
		if !self.successful {
			return false;
		}
		if self.occupancy + self.occupancy / 2 >= self.mask && !self.alloc(0) {
			return false;
		}
		let hash = hash & 0x3FFF_FFFF;
		let mut tombstone = None;
		let mut i = hash % self.prime;
		let mut length = 0u32;
		let mut step = 0u32;
		while self.items[i as usize].used {
			if self.items[i as usize].key == key {
				break;
			}
			if !self.items[i as usize].real && tombstone.is_none() {
				tombstone = Some(i);
			}
			step += 1;
			i = (i + step) & self.mask;
			length += 1;
		}
		let item = &mut self.items[tombstone.unwrap_or(i) as usize];
		if item.used {
			self.occupancy -= 1;
			self.population -= u32::from(item.real);
		}
		*item = Item {
			key,
			value,
			hash,
			used: true,
			real: true,
		};
		self.occupancy += 1;
		self.population += 1;
		if length > self.max_chain_length && self.occupancy > self.mask / 8 {
			// This ensures we jump to the next larger size.
			self.alloc(self.mask.wrapping_sub(8));
		}
		true
	}

	/// `set`: inserts or overwrites; false if an allocation failed.
	pub fn set(&mut self, key: u32, value: u32) -> bool {
		self.set_with_hash(key, hb_hash(key), value)
	}

	fn fetch_item(&self, key: u32) -> Option<usize> {
		if self.items.is_empty() {
			return None;
		}
		let hash = hb_hash(key) & 0x3FFF_FFFF;
		let mut i = hash % self.prime;
		let mut step = 0u32;
		while self.items[i as usize].used {
			if self.items[i as usize].key == key {
				return self.items[i as usize].real.then_some(i as usize);
			}
			step += 1;
			i = (i + step) & self.mask;
		}
		None
	}

	/// The value of a key (`has (key, &v)`).
	pub fn get(&self, key: u32) -> Option<u32> {
		self.fetch_item(key).map(|i| self.items[i].value)
	}

	/// A mutable reference to the value of a key.
	pub fn get_mut(&mut self, key: u32) -> Option<&mut u32> {
		self.fetch_item(key).map(|i| &mut self.items[i].value)
	}

	/// `has`.
	pub fn has(&self, key: u32) -> bool {
		self.fetch_item(key).is_some()
	}

	/// `del`: leaves a tombstone.
	pub fn del(&mut self, key: u32) {
		if let Some(i) = self.fetch_item(key) {
			self.items[i].real = false;
			self.population -= 1;
		}
	}

	/// `reset` / `clear`: removes everything, keeping the allocated size.
	pub fn clear(&mut self) {
		self.successful = true;
		if self.population == 0 && self.occupancy == 0 {
			return;
		}
		self.items.fill(Item::default());
		self.population = 0;
		self.occupancy = 0;
	}

	/// `iter`: the entries in bucket order.
	pub fn iter(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
		self.items
			.iter()
			.filter(|item| item.real)
			.map(|item| (item.key, item.value))
	}

	/// `keys`: the keys in bucket order.
	pub fn keys(&self) -> impl Iterator<Item = u32> + '_ {
		self.iter().map(|(k, _)| k)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn basics() {
		let mut m = HbMap::default();
		for k in [5, 3, 100, 7, 3] {
			assert!(m.set(k, k * 2));
		}
		assert_eq!(m.population(), 4);
		assert_eq!(m.get(100), Some(200));
		m.del(3);
		assert!(!m.has(3));
		assert_eq!(m.population(), 3);
		let keys: Vec<_> = m.keys().collect();
		assert_eq!(keys.len(), 3);
		m.clear();
		assert!(m.is_empty());
		// The size is kept.
		assert_eq!(m.items.len(), 8);
	}

	#[test]
	fn bucket_order() {
		// hb_hash(k) % 7 for the first table of 8 buckets.
		let mut m = HbMap::default();
		m.set(1, 0);
		m.set(2, 0);
		let buckets = [(hb_hash(1) & 0x3FFF_FFFF) % 7, (hb_hash(2) & 0x3FFF_FFFF) % 7];
		let expected = if buckets[0] < buckets[1] { [1, 2] } else { [2, 1] };
		assert_eq!(m.keys().collect::<Vec<_>>(), expected);
	}
}
