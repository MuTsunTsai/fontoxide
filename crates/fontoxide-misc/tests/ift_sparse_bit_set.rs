//! `Tests/misc/iftSparseBitSet_test.py`.
#![allow(non_snake_case)]

use std::collections::BTreeSet;

use fontoxide_misc::ift_sparse_bit_set::{decode, encode};

/// The default `maxValue` of `decode`.
const MAX: i64 = 0xFFFF_FFFF;

fn roundtrip(values: impl IntoIterator<Item = u64>) {
	let values: BTreeSet<u64> = values.into_iter().collect();
	let encoded = encode(values.iter().copied()).expect("encodable");
	let (decoded, consumed) = decode(&encoded, 0, MAX).expect("decodable");
	let decoded: BTreeSet<u64> = decoded.into_iter().map(u64::from).collect();
	assert_eq!(decoded, values);
	assert_eq!(consumed, encoded.len());
}

#[test]
fn test_roundtrip_empty_success() {
	roundtrip([]);
}

#[test]
fn test_roundtrip_singleValue_success() {
	roundtrip([42]);
}

#[test]
fn test_roundtrip_multipleValues_success() {
	roundtrip([1, 10, 100, 1000]);
}

#[test]
fn test_roundtrip_largeRange_success() {
	// Test zero-node optimization (filling subtrees).
	roundtrip(100..1100);
	// 1000 values would take ~125 bytes if 1 bit per value.
	// With zero-node optimization, it should be much smaller.
	assert!(encode(100..1100).expect("encodable").len() < 50);
}

#[test]
fn test_roundtrip_sparseValues_success() {
	roundtrip([1, 1_000_000]);
}

#[test]
fn test_decode_positiveBias_shiftsValues() {
	// b"\x04\x01" is {0} with BF=2, height=1.
	assert_eq!(
		decode(b"\x04\x01", 100, MAX),
		Ok((BTreeSet::from([100]), 2))
	);
}

#[test]
fn test_decode_negativeBias_clipsToZero() {
	// b"\x04\x01" is {0}. With bias -1, it becomes -1 and should be ignored.
	assert_eq!(decode(b"\x04\x01", -1, MAX), Ok((BTreeSet::new(), 2)));
}

#[test]
fn test_decode_headerOnlyHeight0_success() {
	// Height 0 always means empty set.
	assert_eq!(decode(b"\x00", 0, MAX), Ok((BTreeSet::new(), 1)));
}

#[test]
fn test_decode_emptyData_raisesError() {
	let err = decode(b"", 0, MAX).expect_err("empty data");
	assert!(err.to_string().contains("Empty data"), "{err}");
}

#[test]
fn test_decode_truncatedData_raisesError() {
	// Encoded {0} is b"\x04\x01". Truncate to b"\x04".
	let err = decode(b"\x04", 0, MAX).expect_err("truncated data");
	assert!(err.to_string().contains("Unexpected end of data"), "{err}");
}

#[test]
fn test_decode_invalidHeight_raisesError() {
	// BF=32 (id 3), Max Height=7. Try height 8.
	let header = (8 << 2) | 3;
	let err = decode(&[header], 0, MAX).expect_err("invalid height");
	assert!(err.to_string().contains("exceeds max 7"), "{err}");
}

#[test]
fn test_encode_valueOutOfRange_raisesError() {
	// BF=32, Max Height=7, Max Capacity=32^7 = 2^35.
	// Value 2^35 requires height 8, which is not supported.
	let err = encode([1u64 << 35]).expect_err("out of range");
	assert!(err.contains("Cannot encode max value"), "{err}");
}

fn decode_with_max(values: impl IntoIterator<Item = u64>, max_value: i64) -> BTreeSet<u32> {
	let encoded = encode(values).expect("encodable");
	let (decoded, consumed) = decode(&encoded, 0, max_value).expect("decodable");
	assert_eq!(consumed, encoded.len());
	decoded
}

#[test]
fn test_decode_maxValue_truncates_output() {
	assert_eq!(
		decode_with_max([1, 2, 10, 20], 15),
		BTreeSet::from([1, 2, 10])
	);
}

#[test]
fn test_decode_maxValue_with_zero_node() {
	// A dense range from 0 to 99 will use the zero-node optimization.
	// maxValue should truncate this dense range.
	assert_eq!(decode_with_max(0..100, 49), (0..50).collect());
}

#[test]
fn test_decode_maxValue_leaves_empty_set() {
	assert_eq!(decode_with_max([100, 200, 300], 50), BTreeSet::new());
}

#[test]
fn test_roundtrip_almost_full_range() {
	// A range of 64 with one value missing should NOT use zero-node optimization
	roundtrip((0..64).filter(|&v| v != 32));
}

#[test]
fn test_roundtrip_full_child_sparse_parent() {
	// This set fills one child node completely (e.g., 8-15 for bf=8),
	// but the parent node is sparse.
	roundtrip(8..16);
}
