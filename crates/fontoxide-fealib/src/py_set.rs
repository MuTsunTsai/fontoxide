//! The iteration order of a CPython `set` of integers.
//!
//! fontTools writes some sets (e.g. the contour points of `Attach` statements)
//! by iterating them. The order of a `set` of `int`s is not randomized: it follows
//! from the integer hashes and CPython's open addressing (`Objects/setobject.c`),
//! which this module reproduces for sets that are only ever added to.

/// `PySet_MINSIZE`.
const MIN_SIZE: usize = 8;
/// `LINEAR_PROBES`.
const LINEAR_PROBES: usize = 9;
/// `PERTURB_SHIFT`.
const PERTURB_SHIFT: u32 = 5;

/// `hash(int)`: the value modulo the Mersenne prime 2**61 - 1, keeping the sign,
/// with -1 mapped to -2.
fn int_hash(value: i64) -> u64 {
	const MODULUS: i128 = (1 << 61) - 1;
	let magnitude = i128::from(value).abs() % MODULUS;
	let mut hash = if value < 0 { -magnitude } else { magnitude };
	if hash == -1 {
		hash = -2;
	}
	// `Py_hash_t` is reinterpreted as `size_t` for probing.
	hash as i64 as u64
}

/// A CPython set table of integers.
#[derive(Debug)]
struct IntSet {
	table: Vec<Option<(i64, u64)>>,
	fill: usize,
	used: usize,
}

impl IntSet {
	fn new() -> Self {
		IntSet {
			table: vec![None; MIN_SIZE],
			fill: 0,
			used: 0,
		}
	}

	fn mask(&self) -> usize {
		self.table.len() - 1
	}

	/// `set_add_entry`.
	fn add(&mut self, key: i64) {
		let hash = int_hash(key);
		let mask = self.mask();
		let mut i = (hash as usize) & mask;
		let mut perturb = hash;
		loop {
			let probes = if i + LINEAR_PROBES <= mask {
				LINEAR_PROBES
			} else {
				0
			};
			for j in i..=i + probes {
				match self.table[j] {
					None => {
						self.table[j] = Some((key, hash));
						self.fill += 1;
						self.used += 1;
						if self.fill * 5 >= mask * 3 {
							let minused = if self.used > 50000 {
								self.used * 2
							} else {
								self.used * 4
							};
							self.resize(minused);
						}
						return;
					}
					Some((k, h)) if h == hash && k == key => return,
					Some(_) => {}
				}
			}
			perturb >>= PERTURB_SHIFT;
			i = (i
				.wrapping_mul(5)
				.wrapping_add(1)
				.wrapping_add(perturb as usize))
				& mask;
		}
	}

	/// `set_table_resize`.
	fn resize(&mut self, minused: usize) {
		let mut newsize = MIN_SIZE;
		while newsize <= minused {
			newsize <<= 1;
		}
		let old = std::mem::replace(&mut self.table, vec![None; newsize]);
		for (key, hash) in old.into_iter().flatten() {
			self.insert_clean(key, hash);
		}
		self.fill = self.used;
	}

	/// `set_insert_clean`.
	fn insert_clean(&mut self, key: i64, hash: u64) {
		let mask = self.mask();
		let mut i = (hash as usize) & mask;
		let mut perturb = hash;
		loop {
			if self.table[i].is_none() {
				self.table[i] = Some((key, hash));
				return;
			}
			if i + LINEAR_PROBES <= mask {
				for slot in &mut self.table[i + 1..=i + LINEAR_PROBES] {
					if slot.is_none() {
						*slot = Some((key, hash));
						return;
					}
				}
			}
			perturb >>= PERTURB_SHIFT;
			i = (i
				.wrapping_mul(5)
				.wrapping_add(1)
				.wrapping_add(perturb as usize))
				& mask;
		}
	}
}

/// The order in which CPython iterates a `set` built by adding `values` one by
/// one (duplicates are dropped).
///
/// ```
/// use fontoxide_fealib::py_int_set_order;
/// assert_eq!(py_int_set_order([5, 12]), vec![12, 5]);
/// assert_eq!(py_int_set_order([3, 1, 2, 1]), vec![1, 2, 3]);
/// ```
pub fn py_int_set_order(values: impl IntoIterator<Item = i64>) -> Vec<i64> {
	let mut set = IntSet::new();
	for v in values {
		set.add(v);
	}
	set.table.into_iter().flatten().map(|(k, _)| k).collect()
}
