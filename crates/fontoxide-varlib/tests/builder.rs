//! Port of fontTools' `Tests/varLib/builder_test.py`.
//!
//! `test_empty_vhvar_size` and `test_vhvar_beyond_64k` are not here: they need
//! `fontTools.varLib.build`, which is not ported.

#![allow(clippy::type_complexity)]

use fontoxide_ttlib::Error;
use fontoxide_ttlib::ot::var_store::{build_multi_var_data, build_var_data};

fn items(rows: &[&[i64]]) -> Vec<Vec<i64>> {
	rows.iter().map(|r| r.to_vec()).collect()
}

#[test]
fn test_build_multi_var_data() -> fontoxide_ttlib::Result<()> {
	let cases: &[(&[u16], &[&[i64]])] = &[
		(&[], &[]),
		(&[], &[&[]]),
		(&[0], &[&[100, 0, 0]]),
		(&[0, 1], &[&[100, 0, 0, 200, 0, 0]]),
		(&[0, 1], &[&[1, 2], &[3, 4, 5, 6]]),
	];
	for (region_indices, rows) in cases {
		let data = build_multi_var_data(region_indices.to_vec(), items(rows))?;
		assert_eq!(data.item, items(rows));
		assert_eq!(data.item.len(), rows.len());
		assert_eq!(data.var_region_index, *region_indices);
		assert_eq!(data.var_region_count, Some(region_indices.len() as u32));
	}
	Ok(())
}

#[test]
fn test_build_multi_var_data_invalid_tuple() {
	let cases: &[(&[u16], &[i64])] = &[(&[], &[1]), (&[0, 1], &[1, 2, 3])];
	for (region_indices, item) in cases {
		let result = build_multi_var_data(region_indices.to_vec(), vec![item.to_vec()]);
		assert!(matches!(result, Err(Error::Assertion(_))), "{item:?}");
	}
}

#[test]
fn test_build_var_data_no_optimize() {
	let cases: &[(&[u16], &[&[i64]], u16)] = &[
		// 0_regions_0_deltas
		(&[], &[], 0),
		// 1_region_1_uint8
		(&[0], &[&[1]], 0),
		// 1_region_1_short
		(&[0], &[&[128]], 1),
		// 3_regions_2_shorts_ordered
		(&[0, 1, 2], &[&[128, 1, 2], &[3, -129, 5], &[6, 7, 8]], 2),
		// 3_regions_2_shorts_unordered
		(&[0, 1, 2], &[&[0, 128, 2], &[3, 4, 5], &[6, 7, -129]], 3),
		// 1_region_1_long
		(&[0], &[&[32768]], 0x8001),
		// 3_regions_1_long_ordered
		(
			&[0, 1, 2],
			&[&[32768, 1, 2], &[3, -129, 5], &[6, 7, 8]],
			0x8001,
		),
		// 3_regions_2_longs_ordered
		(
			&[0, 1, 2],
			&[&[32768, 1, 2], &[3, -32769, 5], &[6, 7, 8]],
			0x8002,
		),
		// 3_regions_2_longs_unordered
		(
			&[0, 1, 2],
			&[&[0, 32768, 2], &[3, 4, 5], &[6, 7, -32769]],
			0x8003,
		),
	];
	for (region_indices, rows, expected_num_shorts) in cases {
		let data = build_var_data(region_indices.to_vec(), items(rows), false);
		assert_eq!(data.item_count, Some(rows.len() as u32));
		assert_eq!(data.num_shorts, *expected_num_shorts, "{rows:?}");
		assert_eq!(data.var_region_count, Some(region_indices.len() as u32));
		assert_eq!(data.var_region_index, *region_indices);
		assert_eq!(data.item, items(rows));
	}
}

#[test]
fn test_build_var_data_optimize() {
	type Case<'a> = (&'a [&'a [i64]], u16, &'a [u16], &'a [&'a [i64]]);
	let cases: &[Case<'_>] = &[
		// 0/3_shorts_no_reorder
		(
			&[&[0, 1, 2], &[3, 4, 5], &[6, 7, 8]],
			0,
			&[0, 1, 2],
			&[&[0, 1, 2], &[3, 4, 5], &[6, 7, 8]],
		),
		// 1/3_shorts_reorder
		(
			&[&[0, 128, 2], &[3, 4, 5], &[6, 7, 8]],
			1,
			&[1, 0, 2],
			&[&[128, 0, 2], &[4, 3, 5], &[7, 6, 8]],
		),
		// 2/3_shorts_reorder
		(
			&[&[0, 1, 128], &[3, 4, 5], &[6, -129, 8]],
			2,
			&[1, 2, 0],
			&[&[1, 128, 0], &[4, 5, 3], &[-129, 8, 6]],
		),
		// 2/3_shorts_same_row_reorder
		(
			&[&[128, 1, -129], &[3, 4, 5], &[6, 7, 8]],
			2,
			&[0, 2, 1],
			&[&[128, -129, 1], &[3, 5, 4], &[6, 8, 7]],
		),
		// 3/3_shorts_no_reorder
		(
			&[&[0, 1, 128], &[3, -129, 5], &[256, 7, 8]],
			3,
			&[0, 1, 2],
			&[&[0, 1, 128], &[3, -129, 5], &[256, 7, 8]],
		),
		// 1/3_shorts_1/3_zeroes
		(
			&[&[0, 128, 2], &[0, 4, 5], &[0, 7, 8]],
			1,
			&[1, 2],
			&[&[128, 2], &[4, 5], &[7, 8]],
		),
		// 1/3_longs_reorder
		(
			&[&[0, 32768, 2], &[3, 4, 5], &[6, 7, 8]],
			0x8001,
			&[1, 0, 2],
			&[&[32768, 0, 2], &[4, 3, 5], &[7, 6, 8]],
		),
		// 2/3_longs_reorder
		(
			&[&[0, 1, 32768], &[3, 4, 5], &[6, -32769, 8]],
			0x8002,
			&[1, 2, 0],
			&[&[1, 32768, 0], &[4, 5, 3], &[-32769, 8, 6]],
		),
		// 2/3_longs_same_row_reorder
		(
			&[&[32768, 1, -32769], &[3, 4, 5], &[6, 7, 8]],
			0x8002,
			&[0, 2, 1],
			&[&[32768, -32769, 1], &[3, 5, 4], &[6, 8, 7]],
		),
		// 3/3_longs_no_reorder
		(
			&[&[0, 1, 32768], &[3, -32769, 5], &[65536, 7, 8]],
			0x8003,
			&[0, 1, 2],
			&[&[0, 1, 32768], &[3, -32769, 5], &[65536, 7, 8]],
		),
		// 1/3_longs_1/3_zeroes
		(
			&[&[0, 32768, 2], &[0, 4, 5], &[0, 7, 8]],
			0x8001,
			&[1, 2],
			&[&[32768, 2], &[4, 5], &[7, 8]],
		),
	];
	for (rows, expected_num_shorts, expected_regions, expected_items) in cases {
		let data = build_var_data(vec![0, 1, 2], items(rows), true);
		assert_eq!(data.item_count, Some(rows.len() as u32));
		assert_eq!(data.num_shorts, *expected_num_shorts, "{rows:?}");
		assert_eq!(data.var_region_count, Some(expected_regions.len() as u32));
		assert_eq!(data.var_region_index, *expected_regions);
		assert_eq!(data.item, items(expected_items));
	}
}
