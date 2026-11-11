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

/// Not from fontTools: the builders of `fontTools.varLib.builder` that its
/// tests do not cover.
#[test]
fn builder_helpers() -> fontoxide_ttlib::Result<()> {
	use std::collections::BTreeMap;

	use fontoxide_varlib::builder::*;

	let device = build_var_dev_table(0x0003_0005);
	assert_eq!(
		(device.delta_format, device.start_size, device.end_size),
		(0x8000, 3, 5)
	);

	assert_eq!(build_delta_set_index_map([0, 1, 2]), None);
	let map = build_delta_set_index_map([1, 0]).ok_or(Error::value("expected a map"))?;
	assert_eq!((map.format, map.mapping), (Some(0), vec![1, 0]));
	let map =
		build_delta_set_index_map((0..0x10001).rev()).ok_or(Error::value("expected a map"))?;
	assert_eq!(map.format, Some(1));

	let map = build_var_idx_map([7, 8, 9], [0, 2]);
	assert_eq!(map.mapping, BTreeMap::from([(0, 7), (2, 8)]));

	let axes = ["wght".to_owned(), "wdth".to_owned()];
	let support = BTreeMap::from([("wdth".to_owned(), (0.0, 1.0, 1.0))]);
	let region = build_sparse_var_region(&support, &axes)?;
	assert_eq!(region.sparse_region_count, Some(1));
	let axis = region.sparse_var_region_axis[0]
		.as_deref()
		.ok_or(Error::value("no axis"))?;
	assert_eq!(
		(
			axis.axis_index,
			axis.start_coord,
			axis.peak_coord,
			axis.end_coord
		),
		(1, 0.0, 1.0, 1.0)
	);
	let list = build_sparse_var_region_list(&[support.clone(), BTreeMap::new()], &axes)?;
	assert_eq!(list.region_count, Some(2));
	let unknown = BTreeMap::from([("opsz".to_owned(), (0.0, 1.0, 1.0))]);
	assert!(matches!(
		build_sparse_var_region(&unknown, &axes),
		Err(Error::Assertion(_))
	));

	let data = build_multi_var_data(vec![0], vec![vec![1, 2]])?;
	let store = build_multi_var_store(list, vec![data]);
	assert_eq!((store.format, store.multi_var_data_count), (1, Some(1)));

	assert!(matches!(
		build_var_data_checked(vec![0, 1], vec![vec![1]], false),
		Err(Error::Assertion(_))
	));
	Ok(())
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
