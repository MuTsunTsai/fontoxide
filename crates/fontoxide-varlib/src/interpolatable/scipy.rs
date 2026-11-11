//! The two SciPy routines that `fontTools.varLib.interpolatable` uses when
//! SciPy is installed, ported so that their results, ties included, are the
//! same: `scipy.optimize.linear_sum_assignment` (`rectangular_lsap.cpp`, the
//! shortest augmenting path algorithm of Crouse) and
//! `scipy.sparse.csgraph.minimum_spanning_tree` (Kruskal's algorithm over a
//! stable sort of the edges).

use super::{InterpolatableError, Result};

/// `scipy.optimize.linear_sum_assignment(cost)` for a square matrix: the column
/// assigned to each row, with the minimum total cost.
///
/// The ties are broken as in SciPy (`rectangular_lsap.cpp`): the columns are
/// scanned from the last one, and among columns of equal reduced cost one that
/// ends an augmenting path is preferred.
pub fn linear_sum_assignment(cost: &[Vec<f64>]) -> Result<Vec<usize>> {
	let nr = cost.len();
	if nr == 0 {
		return Ok(Vec::new());
	}
	let nc = cost[0].len();
	if cost.iter().any(|row| row.len() != nc) {
		return Err(InterpolatableError::Value(
			"setting an array element with a sequence. The requested array has an inhomogeneous shape after 1 dimensions.".into(),
		));
	}
	if nc != nr {
		// fontTools only asks for square matrices; SciPy would transpose a tall
		// one, and its result would fail fontTools' assertion anyway.
		return Err(InterpolatableError::Assertion(
			"linear_sum_assignment of a non-square matrix".into(),
		));
	}
	if cost
		.iter()
		.flatten()
		.any(|&c| c.is_nan() || c == f64::NEG_INFINITY)
	{
		return Err(InterpolatableError::Value(
			"matrix contains invalid numeric entries".into(),
		));
	}

	let mut u = vec![0.0f64; nr];
	let mut v = vec![0.0f64; nc];
	let mut shortest_path_costs = vec![0.0f64; nc];
	let mut path = vec![usize::MAX; nc];
	let mut col4row = vec![usize::MAX; nr];
	let mut row4col = vec![usize::MAX; nc];
	let mut sr = vec![false; nr];
	let mut sc = vec![false; nc];
	let mut remaining = vec![0usize; nc];

	for cur_row in 0..nr {
		// `augmenting_path`.
		let mut min_val = 0.0f64;
		let mut num_remaining = nc;
		for (it, r) in remaining.iter_mut().enumerate() {
			// Filling this up in reverse order ensures that the solution of a
			// constant cost matrix is the identity matrix.
			*r = nc - it - 1;
		}
		sr.fill(false);
		sc.fill(false);
		shortest_path_costs.fill(f64::INFINITY);

		let mut i = cur_row;
		let sink;
		loop {
			let mut index = usize::MAX;
			let mut lowest = f64::INFINITY;
			sr[i] = true;
			for (it, &j) in remaining.iter().enumerate().take(num_remaining) {
				let r = min_val + cost[i][j] - u[i] - v[j];
				if r < shortest_path_costs[j] {
					path[j] = i;
					shortest_path_costs[j] = r;
				}
				// When multiple nodes have the minimum cost, we select one which
				// gives us a new sink node.
				if shortest_path_costs[j] < lowest
					|| (shortest_path_costs[j] == lowest && row4col[j] == usize::MAX)
				{
					lowest = shortest_path_costs[j];
					index = it;
				}
			}
			min_val = lowest;
			if min_val == f64::INFINITY || index == usize::MAX {
				return Err(InterpolatableError::Value(
					"cost matrix is infeasible".into(),
				));
			}
			let j = remaining[index];
			sc[j] = true;
			num_remaining -= 1;
			remaining[index] = remaining[num_remaining];
			if row4col[j] == usize::MAX {
				sink = j;
				break;
			}
			i = row4col[j];
		}

		// Update the dual variables.
		u[cur_row] += min_val;
		for i in 0..nr {
			if sr[i] && i != cur_row {
				u[i] += min_val - shortest_path_costs[col4row[i]];
			}
		}
		for j in 0..nc {
			if sc[j] {
				v[j] -= min_val - shortest_path_costs[j];
			}
		}

		// Augment the previous solution.
		let mut j = sink;
		loop {
			let i = path[j];
			row4col[j] = i;
			std::mem::swap(&mut col4row[i], &mut j);
			if i == cur_row {
				break;
			}
		}
	}
	Ok(col4row)
}

/// The edges of `scipy.sparse.csgraph.minimum_spanning_tree(graph)` for a
/// dense graph given as its upper triangle: `graph[i][j]` for `i < j`, where
/// zeros, NaNs and infinities are not edges. Returns the `(row, column)` pairs
/// of the tree in row-major order (`tree.nonzero()`).
pub fn minimum_spanning_tree(graph: &[Vec<f64>]) -> Vec<(usize, usize)> {
	let n = graph.len();
	// The CSR entries, row by row.
	let mut edges: Vec<(usize, usize, f64)> = Vec::new();
	for (i, row) in graph.iter().enumerate() {
		for (j, &w) in row.iter().enumerate() {
			if w != 0.0 && w.is_finite() {
				edges.push((i, j, w));
			}
		}
	}
	// `np.argsort(data, kind="stable")`.
	let mut order: Vec<usize> = (0..edges.len()).collect();
	order.sort_by(|&a, &b| edges[a].2.total_cmp(&edges[b].2));

	let mut predecessors: Vec<usize> = (0..n).collect();
	let mut rank = vec![0usize; n];
	let mut kept = vec![false; edges.len()];
	let mut n_edges_in_mst = 0;
	let root = |predecessors: &[usize], mut v: usize| {
		while predecessors[v] != v {
			v = predecessors[v];
		}
		v
	};
	for &e in &order {
		if n == 0 || n_edges_in_mst >= n - 1 {
			break;
		}
		let (v1, v2, _) = edges[e];
		if v1 >= n || v2 >= n {
			continue;
		}
		let r1 = root(&predecessors, v1);
		let r2 = root(&predecessors, v2);
		// Path compression (of the first step only, as SciPy does).
		predecessors[v1] = r1;
		predecessors[v2] = r2;
		if r1 != r2 {
			n_edges_in_mst += 1;
			kept[e] = true;
			match rank[r1].cmp(&rank[r2]) {
				std::cmp::Ordering::Greater => predecessors[r2] = r1,
				std::cmp::Ordering::Less => predecessors[r1] = r2,
				std::cmp::Ordering::Equal => {
					predecessors[r2] = r1;
					rank[r1] += 1;
				}
			}
		}
	}
	edges
		.iter()
		.zip(&kept)
		.filter(|(_, k)| **k)
		.map(|(&(i, j, _), _)| (i, j))
		.collect()
}
