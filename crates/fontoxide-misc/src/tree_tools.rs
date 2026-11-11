//! Port of `fontTools.misc.treeTools`: generic tools for working with trees.

/// A node of a tree built by [`build_n_ary_tree`]: a leaf, or a list of nodes
/// (fontTools uses plain nested lists).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tree<T> {
	/// A leaf node.
	Leaf(T),
	/// A non-leaf node, with at most `n` children.
	Node(Vec<Tree<T>>),
}

/// The error of [`build_n_ary_tree`] when `n` is less than 2 (an
/// `AssertionError` in fontTools).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("n > 1")]
pub struct InvalidArity;

/// Builds an N-ary tree from a sequence of leaf nodes (`build_n_ary_tree`).
///
/// Returns the children of the root, where each non-leaf node is a list of at
/// most `n` nodes.
///
/// ```
/// use fontoxide_misc::tree_tools::{Tree, build_n_ary_tree};
/// let tree = build_n_ary_tree(vec![0, 1, 2], 2)?;
/// let pair = Tree::Node(vec![Tree::Leaf(0), Tree::Leaf(1)]);
/// assert_eq!(tree, [pair, Tree::Leaf(2)]);
/// # Ok::<(), fontoxide_misc::tree_tools::InvalidArity>(())
/// ```
pub fn build_n_ary_tree<T>(leaves: Vec<T>, n: usize) -> Result<Vec<Tree<T>>, InvalidArity> {
	if leaves.is_empty() {
		return Ok(Vec::new());
	}
	if n <= 1 {
		return Err(InvalidArity);
	}
	Ok(build(leaves.into_iter().map(Tree::Leaf).collect(), n))
}

fn build<T>(mut leaves: Vec<Tree<T>>, n: usize) -> Vec<Tree<T>> {
	if leaves.is_empty() {
		return leaves;
	}
	// `ceil(log(len(leaves), n))`; Python computes `log(x) / log(n)`.
	let depth = ((leaves.len() as f64).ln() / (n as f64).ln()).ceil();
	if depth <= 1.0 {
		return leaves;
	}
	let depth = depth as u32;

	// Fully populate complete subtrees of root until we have enough leaves left.
	let mut root = Vec::new();
	let mut unassigned = None;
	let full_step = n.saturating_pow(depth - 1);
	while !leaves.is_empty() {
		let rest = leaves.split_off(full_step.min(leaves.len()));
		let mut subtree = std::mem::replace(&mut leaves, rest);
		if subtree.len() < full_step {
			unassigned = Some(subtree);
			break;
		}
		while subtree.len() > n {
			let mut nodes = Vec::with_capacity(subtree.len().div_ceil(n));
			while !subtree.is_empty() {
				let rest = subtree.split_off(n.min(subtree.len()));
				nodes.push(Tree::Node(std::mem::replace(&mut subtree, rest)));
			}
			subtree = nodes;
		}
		root.push(Tree::Node(subtree));
	}

	if let Some(unassigned) = unassigned {
		// Recurse to fill the last subtree, which is the only partially populated
		// one.
		let subtree = build(unassigned, n);
		if subtree.len() <= n - root.len() {
			// Replace the last subtree with its children if they can still fit.
			root.extend(subtree);
		} else {
			root.push(Tree::Node(subtree));
		}
	}
	root
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A tree of integers written as nested lists, like fontTools' tests.
	fn show(nodes: &[Tree<usize>]) -> String {
		let items: Vec<String> = nodes
			.iter()
			.map(|node| match node {
				Tree::Leaf(v) => v.to_string(),
				Tree::Node(children) => show(children),
			})
			.collect();
		format!("[{}]", items.join(", "))
	}

	fn range(start: usize, end: usize) -> String {
		let items: Vec<String> = (start..end).map(|i| i.to_string()).collect();
		format!("[{}]", items.join(", "))
	}

	fn tree(len: usize, n: usize) -> Result<String, InvalidArity> {
		Ok(show(&build_n_ary_tree((0..len).collect(), n)?))
	}

	/// `Tests/misc/treeTools_test.py::test_build_n_ary_tree`.
	#[test]
	fn test_build_n_ary_tree() -> Result<(), InvalidArity> {
		let cases: &[(usize, usize, &str)] = &[
			(1, 2, "[0]"),
			(2, 2, "[0, 1]"),
			(3, 2, "[[0, 1], 2]"),
			(3, 3, "[0, 1, 2]"),
			(4, 2, "[[0, 1], [2, 3]]"),
			(4, 3, "[[0, 1, 2], 3]"),
			(5, 3, "[[0, 1, 2], 3, 4]"),
			(6, 3, "[[0, 1, 2], [3, 4, 5]]"),
			(7, 3, "[[0, 1, 2], [3, 4, 5], 6]"),
			(8, 3, "[[0, 1, 2], [3, 4, 5], [6, 7]]"),
			(9, 3, "[[0, 1, 2], [3, 4, 5], [6, 7, 8]]"),
			(10, 3, "[[[0, 1, 2], [3, 4, 5], [6, 7, 8]], 9]"),
			(11, 3, "[[[0, 1, 2], [3, 4, 5], [6, 7, 8]], 9, 10]"),
			(12, 3, "[[[0, 1, 2], [3, 4, 5], [6, 7, 8]], [9, 10, 11]]"),
			(
				13,
				3,
				"[[[0, 1, 2], [3, 4, 5], [6, 7, 8]], [9, 10, 11], 12]",
			),
			(
				14,
				3,
				"[[[0, 1, 2], [3, 4, 5], [6, 7, 8]], [[9, 10, 11], 12, 13]]",
			),
			(
				15,
				3,
				"[[[0, 1, 2], [3, 4, 5], [6, 7, 8]], [9, 10, 11], [12, 13, 14]]",
			),
			(
				16,
				3,
				"[[[0, 1, 2], [3, 4, 5], [6, 7, 8]], [[9, 10, 11], [12, 13, 14], 15]]",
			),
			(
				23,
				3,
				"[[[0, 1, 2], [3, 4, 5], [6, 7, 8]], [[9, 10, 11], [12, 13, 14], [15, 16, 17]], [[18, 19, 20], 21, 22]]",
			),
			(
				27,
				3,
				"[[[0, 1, 2], [3, 4, 5], [6, 7, 8]], [[9, 10, 11], [12, 13, 14], [15, 16, 17]], [[18, 19, 20], [21, 22, 23], [24, 25, 26]]]",
			),
			(
				28,
				3,
				"[[[[0, 1, 2], [3, 4, 5], [6, 7, 8]], [[9, 10, 11], [12, 13, 14], [15, 16, 17]], [[18, 19, 20], [21, 22, 23], [24, 25, 26]]], 27]",
			),
		];
		for &(len, n, expected) in cases {
			assert_eq!(tree(len, n)?, expected, "{len} leaves, n = {n}");
		}

		let r = range;
		assert_eq!(tree(257, 256)?, format!("[{}, 256]", r(0, 256)));
		assert_eq!(tree(258, 256)?, format!("[{}, 256, 257]", r(0, 256)));
		assert_eq!(tree(512, 256)?, format!("[{}, {}]", r(0, 256), r(256, 512)));
		assert_eq!(
			tree(513, 256)?,
			format!("[{}, {}, 512]", r(0, 256), r(256, 512))
		);
		let blocks: Vec<String> = (0..256).map(|k| r(k * 256, k * 256 + 256)).collect();
		assert_eq!(tree(256 * 256, 256)?, format!("[{}]", blocks.join(", ")));
		Ok(())
	}

	#[test]
	fn empty_and_invalid_arity() {
		assert_eq!(build_n_ary_tree(Vec::<u8>::new(), 0), Ok(Vec::new()));
		assert_eq!(build_n_ary_tree(vec![1], 1), Err(InvalidArity));
	}
}
