//! Sparse bit sets of Incremental Font Transfer (`fontTools.misc.iftSparseBitSet`).
//!
//! A set of integers is stored as a tree with a branching factor of 2, 4, 8 or
//! 32, written breadth-first: each node is a bit mask of its non-empty children,
//! and a node of all zeros stands for a completely filled subtree. See
//! <https://w3c.github.io/IFT/Overview.html#sparse-bit-set-decoding>.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

/// A malformed sparse bit set (`SparseBitSetDecodeError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl fmt::Display for DecodeError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.0)
	}
}

impl std::error::Error for DecodeError {}

/// The largest tree height whose leaves fit in 32 bits, by branching factor.
fn max_height(branch_factor: u32) -> u32 {
	match branch_factor {
		2 => 31,
		4 => 16,
		8 => 11,
		_ => 7,
	}
}

fn decode_header(byte: u8) -> (u32, u32) {
	let branch_factor = [2, 4, 8, 32][(byte & 0x03) as usize];
	(branch_factor, ((byte >> 2) & 0x1F) as u32)
}

fn encode_header(branch_factor: u32, height: u32) -> u8 {
	let id = match branch_factor {
		2 => 0,
		4 => 1,
		8 => 2,
		_ => 3,
	};
	((height << 2) | id) as u8
}

struct InputBits<'a> {
	data: &'a [u8],
	branch_factor: u32,
	byte_index: usize,
	sub_index: u32,
}

impl InputBits<'_> {
	fn next(&mut self) -> Option<u32> {
		match self.branch_factor {
			2 | 4 => {
				let byte = *self.data.get(self.byte_index)?;
				let mask = (1u32 << self.branch_factor) - 1;
				let v = (byte as u32 >> self.sub_index) & mask;
				self.sub_index += self.branch_factor;
				if self.sub_index >= 8 {
					self.sub_index = 0;
					self.byte_index += 1;
				}
				Some(v)
			}
			8 => {
				let byte = *self.data.get(self.byte_index)?;
				self.byte_index += 1;
				Some(byte as u32)
			}
			_ => {
				let b = self.data.get(self.byte_index..self.byte_index + 4)?;
				self.byte_index += 4;
				Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
			}
		}
	}

	fn bytes_consumed(&self) -> usize {
		self.byte_index + usize::from(self.sub_index > 0)
	}
}

/// Decodes a sparse bit set, adding `bias` to each value and dropping values
/// below 0 or above `max_value` (`decode`). Returns the values and the number of
/// bytes read.
///
/// As in fontTools, decoding stops at the first value above `max_value`, so the
/// bytes after it are not counted as read.
pub fn decode(
	data: &[u8],
	bias: i64,
	max_value: i64,
) -> Result<(BTreeSet<u32>, usize), DecodeError> {
	let header = *data
		.first()
		.ok_or_else(|| DecodeError("Empty data".into()))?;
	let (branch_factor, height) = decode_header(header);
	let max = max_height(branch_factor);
	if height > max {
		return Err(DecodeError(format!(
			"Height {height} exceeds max {max} for branch factor {branch_factor}"
		)));
	}
	let mut result = BTreeSet::new();
	if height == 0 {
		return Ok((result, 1));
	}
	let mut bits_in = InputBits {
		data,
		branch_factor,
		byte_index: 1,
		sub_index: 0,
	};
	let bf = branch_factor as i64;
	let mut queue: VecDeque<(i64, u32)> = VecDeque::from([(0, 1)]);
	while let Some((start, depth)) = queue.pop_front() {
		let mut bits = bits_in
			.next()
			.ok_or_else(|| DecodeError("Unexpected end of data".into()))?;
		if bits == 0 {
			// A filled subtree.
			let node_size = bf.pow(height - depth + 1);
			let fill_start = start + bias;
			if fill_start > max_value {
				continue;
			}
			let fill_end = max_value.min(start + node_size - 1 + bias);
			let fill_start = fill_start.max(0);
			if fill_start <= fill_end {
				result.extend((fill_start..=fill_end).map(|v| v as u32));
			}
			continue;
		}
		let next_node_size = bf.pow(height - depth);
		while bits != 0 {
			let bit = bits.trailing_zeros() as i64;
			if depth == height {
				let value = start + bit + bias;
				if value > max_value {
					queue.clear();
					break;
				}
				if value >= 0 {
					result.insert(value as u32);
				}
			} else {
				queue.push_back((start + bit * next_node_size, depth + 1));
			}
			bits &= !(1 << bit);
		}
	}
	Ok((result, bits_in.bytes_consumed()))
}

/// The smallest tree height covering `max_value`.
fn tree_height(branch_factor: u64, max_value: u64) -> u32 {
	let mut height = 1;
	let mut capacity = branch_factor;
	while capacity <= max_value {
		capacity *= branch_factor;
		height += 1;
	}
	height
}

struct OutputBits {
	branch_factor: u32,
	data: Vec<u8>,
	sub_index: u32,
}

impl OutputBits {
	fn write(&mut self, value: u32) {
		match self.branch_factor {
			2 | 4 => {
				let value = value & ((1 << self.branch_factor) - 1);
				if self.sub_index == 0 {
					self.data.push(0);
				}
				if let Some(last) = self.data.last_mut() {
					*last |= (value << self.sub_index) as u8;
				}
				self.sub_index += self.branch_factor;
				if self.sub_index >= 8 {
					self.sub_index = 0;
				}
			}
			8 => self.data.push(value as u8),
			_ => self.data.extend_from_slice(&value.to_le_bytes()),
		}
	}
}

fn encode_with(values: &[u64], branch_factor: u32, height: u32) -> Vec<u8> {
	let bf = branch_factor as u64;
	// Layer 0 holds the leaves; each layer above groups `bf` nodes of the one below.
	let mut layers: Vec<BTreeMap<u64, u32>> = vec![BTreeMap::new()];
	for &v in values {
		*layers[0].entry(v / bf).or_default() |= 1 << (v % bf);
	}
	for _ in 1..height {
		let mut layer = BTreeMap::new();
		for &node in layers[layers.len() - 1].keys() {
			*layer.entry(node / bf).or_default() |= 1 << (node % bf);
		}
		layers.push(layer);
	}
	let range_count = |lo: u64, hi: u64| {
		values.partition_point(|&v| v <= hi) - values.partition_point(|&v| v < lo)
	};
	let mut out = OutputBits {
		branch_factor,
		data: Vec::new(),
		sub_index: 0,
	};
	let mut queue: VecDeque<(u64, u32, u64, u64)> = VecDeque::from([(0, 0, 0, bf.pow(height) - 1)]);
	while let Some((node, depth, start, end)) = queue.pop_front() {
		let layer = (height - 1 - depth) as usize;
		let bitmask = layers
			.get(layer)
			.and_then(|l| l.get(&node))
			.copied()
			.unwrap_or(0);
		// A filled internal node is written as zero; leaves always get their bits.
		if depth + 1 < height && range_count(start, end) as u64 == end - start + 1 {
			out.write(0);
			continue;
		}
		out.write(bitmask);
		if bitmask != 0 && depth + 1 < height {
			let child_size = (end - start + 1) / bf;
			let mut bits = bitmask;
			while bits != 0 {
				let bit = bits.trailing_zeros() as u64;
				let child_start = start + bit * child_size;
				queue.push_back((
					node * bf + bit,
					depth + 1,
					child_start,
					child_start + child_size - 1,
				));
				bits &= !(1 << bit);
			}
		}
	}
	let mut data = vec![encode_header(branch_factor, height)];
	data.extend(out.data);
	data
}

/// Encodes a set of integers with the branching factor giving the shortest
/// data, preferring the smaller factor on ties (`encode`). Fails if the largest
/// value needs too tall a tree.
pub fn encode(values: impl IntoIterator<Item = u64>) -> Result<Vec<u8>, String> {
	let values: Vec<u64> = values
		.into_iter()
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect();
	let Some(&max_value) = values.last() else {
		return Ok(vec![encode_header(2, 0)]);
	};
	let mut best: Option<Vec<u8>> = None;
	for branch_factor in [2, 4, 8, 32] {
		let height = tree_height(branch_factor as u64, max_value);
		if height > max_height(branch_factor) {
			continue;
		}
		let encoded = encode_with(&values, branch_factor, height);
		if best.as_ref().is_none_or(|b| encoded.len() < b.len()) {
			best = Some(encoded);
		}
	}
	best.ok_or_else(|| format!("Cannot encode max value {max_value}"))
}

#[cfg(test)]
mod tests {
	use super::*;

	const MAX: i64 = 0xFFFF_FFFF;

	fn round_trip(values: &[u64]) -> Result<(), String> {
		let encoded = encode(values.iter().copied())?;
		let (decoded, consumed) = decode(&encoded, 0, MAX).map_err(|e| e.0)?;
		let expected: BTreeSet<u32> = values.iter().map(|&v| v as u32).collect();
		assert_eq!(decoded, expected);
		assert_eq!(consumed, encoded.len());
		Ok(())
	}

	// Cases from fontTools' iftSparseBitSet_test.py.
	#[test]
	fn round_trips() -> Result<(), String> {
		round_trip(&[])?;
		round_trip(&[42])?;
		round_trip(&[1, 10, 100, 1000])?;
		round_trip(&[1, 1_000_000])?;
		round_trip(&(8..16).collect::<Vec<_>>())?;
		let almost: Vec<u64> = (0..64).filter(|&v| v != 32).collect();
		round_trip(&almost)?;
		let range: Vec<u64> = (100..1100).collect();
		round_trip(&range)?;
		assert!(encode(range)?.len() < 50);
		Ok(())
	}

	#[test]
	fn encodings_match_fonttools() -> Result<(), String> {
		let hex = |values: Vec<u64>| -> Result<String, String> {
			Ok(encode(values)?.iter().map(|b| format!("{b:02x}")).collect())
		};
		assert_eq!(hex(vec![0])?, "0401");
		assert_eq!(hex(vec![5])?, "0c26");
		assert_eq!(hex(vec![1, 10, 100, 1000])?, "28676e5a9d655956");
		assert_eq!(hex((100..1100).collect())?, "2cdf34c9d2cd4200");
		assert_eq!(hex(vec![1, 1_000_000])?, "5067666555555665555519");
		assert_eq!(hex((0..64).step_by(3).collect())?, "0aff4992244992244992");
		assert_eq!(hex(vec![0x10FFFF])?, "5456a9aaaaaa02");
		assert_eq!(hex(vec![7, 8, 9, 31, 32, 33, 1023])?, "1519786818888383");
		Ok(())
	}

	#[test]
	fn decoding() -> Result<(), DecodeError> {
		assert_eq!(decode(b"\x04\x01", 100, MAX)?, (BTreeSet::from([100]), 2));
		assert_eq!(decode(b"\x04\x01", -1, MAX)?, (BTreeSet::new(), 2));
		assert_eq!(decode(b"\x00", 0, MAX)?, (BTreeSet::new(), 1));
		assert!(decode(b"", 0, MAX).is_err());
		assert!(decode(b"\x04", 0, MAX).is_err());
		assert!(decode(&[(8 << 2) | 3], 0, MAX).is_err());
		assert!(encode([1u64 << 35]).is_err());
		Ok(())
	}

	#[test]
	fn max_value_truncates() -> Result<(), String> {
		let decode = |values: Vec<u64>, max: i64| -> Result<BTreeSet<u32>, String> {
			let encoded = encode(values)?;
			Ok(decode(&encoded, 0, max).map_err(|e| e.0)?.0)
		};
		assert_eq!(decode(vec![1, 2, 10, 20], 15)?, BTreeSet::from([1, 2, 10]));
		assert_eq!(decode((0..100).collect(), 49)?, (0..50).collect());
		assert_eq!(decode(vec![100, 200, 300], 50)?, BTreeSet::new());
		Ok(())
	}
}
