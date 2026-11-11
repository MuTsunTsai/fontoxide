//! `Tests/misc/classifyTools_test.py`, and the doctest of
//! `fontTools.misc.classifyTools.classify` (the same examples).

use std::collections::{BTreeMap, BTreeSet};

use fontoxide_misc::classify_tools::classify;

type Classes = (Vec<BTreeSet<i32>>, BTreeMap<i32, BTreeSet<i32>>);

fn c(sets: &[&[i32]], sort: bool) -> Classes {
	classify(sets.iter().map(|s| s.iter().copied()), sort)
}

fn s(items: &[i32]) -> BTreeSet<i32> {
	items.iter().copied().collect()
}

fn expected(classes: &[&[i32]], mapping: &[(i32, &[i32])]) -> Classes {
	(
		classes.iter().map(|c| s(c)).collect(),
		mapping.iter().map(|&(k, v)| (k, s(v))).collect(),
	)
}

#[test]
fn test_classify() {
	assert_eq!(c(&[], true), expected(&[], &[]));
	assert_eq!(c(&[&[]], true), expected(&[], &[]));
	assert_eq!(c(&[&[], &[]], true), expected(&[], &[]));
	assert_eq!(c(&[&[1]], true), expected(&[&[1]], &[(1, &[1])]));
	assert_eq!(
		c(&[&[1, 2]], true),
		expected(&[&[1, 2]], &[(1, &[1, 2]), (2, &[1, 2])])
	);
	assert_eq!(
		c(&[&[1], &[2]], true),
		expected(&[&[1], &[2]], &[(1, &[1]), (2, &[2])])
	);
	assert_eq!(
		c(&[&[1, 2], &[2]], true),
		expected(&[&[1], &[2]], &[(1, &[1]), (2, &[2])])
	);
	assert_eq!(
		c(&[&[1, 2], &[2, 4]], true),
		expected(&[&[1], &[2], &[4]], &[(1, &[1]), (2, &[2]), (4, &[4])])
	);
	assert_eq!(
		c(&[&[1, 2], &[2, 4, 5]], true),
		expected(
			&[&[4, 5], &[1], &[2]],
			&[(1, &[1]), (2, &[2]), (4, &[4, 5]), (5, &[4, 5])]
		)
	);
	assert_eq!(
		c(&[&[1, 2], &[2, 4, 5]], false),
		expected(
			&[&[1], &[4, 5], &[2]],
			&[(1, &[1]), (2, &[2]), (4, &[4, 5]), (5, &[4, 5])]
		)
	);
	assert_eq!(
		c(&[&[1, 2, 9], &[2, 4, 5]], false),
		expected(
			&[&[1, 9], &[4, 5], &[2]],
			&[
				(1, &[1, 9]),
				(2, &[2]),
				(4, &[4, 5]),
				(5, &[4, 5]),
				(9, &[1, 9])
			]
		)
	);
	assert_eq!(
		c(&[&[1, 2, 9, 15], &[2, 4, 5]], false),
		expected(
			&[&[1, 9, 15], &[4, 5], &[2]],
			&[
				(1, &[1, 9, 15]),
				(2, &[2]),
				(4, &[4, 5]),
				(5, &[4, 5]),
				(9, &[1, 9, 15]),
				(15, &[1, 9, 15])
			]
		)
	);
	let (classes, mapping) = c(&[&[1, 2, 9, 15], &[2, 4, 5], &[15, 5]], false);
	let classes: BTreeSet<BTreeSet<i32>> = classes.into_iter().collect();
	assert_eq!(
		classes,
		[s(&[1, 9]), s(&[4]), s(&[2]), s(&[5]), s(&[15])]
			.into_iter()
			.collect()
	);
	assert_eq!(
		mapping,
		expected(
			&[],
			&[
				(1, &[1, 9]),
				(2, &[2]),
				(4, &[4]),
				(5, &[5]),
				(9, &[1, 9]),
				(15, &[15])
			]
		)
		.1
	);
}

/// The doctest of `fontTools.misc.classifyTools.classify` has the same examples.
#[test]
fn doctest_classify() {
	test_classify();
}
