//! Type 2 charstring glyph width optimizer (`fontTools.cffLib.width`).
//!
//! CFF glyphs whose width equals the Private DICT's `defaultWidthX` value do not need
//! to specify their width in their charstring, saving bytes. This module determines
//! the optimum `defaultWidthX` and `nominalWidthX` values for a font.

use std::collections::BTreeMap;

/// A cumulative sum or maximum of frequencies (`cumSum`).
struct Cumulative {
	values: BTreeMap<i64, i64>,
	/// The value below the range, in the direction of accumulation; 0.
	min: i64,
	max: i64,
	total: i64,
	decreasing: bool,
}

impl Cumulative {
	fn new(f: &BTreeMap<i64, i64>, op: fn(i64, i64) -> i64, decreasing: bool) -> Self {
		let min = *f.keys().next().unwrap_or(&0);
		let max = *f.keys().next_back().unwrap_or(&0);
		let total = f.values().fold(0, |a, &b| op(a, b));
		let mut values = BTreeMap::new();
		let mut v = 0;
		let mut step = |x: i64| {
			v = op(v, f.get(&x).copied().unwrap_or(0));
			values.insert(x, v);
		};
		if decreasing {
			(min..=max).rev().for_each(&mut step);
		} else {
			(min..=max).for_each(&mut step);
		}
		Cumulative {
			values,
			min,
			max,
			total,
			decreasing,
		}
	}

	fn get(&self, x: i64) -> i64 {
		if let Some(&v) = self.values.get(&x) {
			return v;
		}
		let before = if self.decreasing {
			x > self.max
		} else {
			x < self.min
		};
		if before { 0 } else { self.total }
	}
}

/// `byteCost`.
fn byte_cost(widths: &BTreeMap<i64, i64>, default: i64, nominal: i64) -> i64 {
	let mut cost = 0;
	for (&w, &freq) in widths {
		if w == default {
			continue;
		}
		let diff = (w - nominal).abs();
		cost += if diff <= 107 {
			freq
		} else if diff <= 1131 {
			freq * 2
		} else {
			freq * 5
		};
	}
	cost
}

/// `optimizeWidths`: given a list of glyph widths, the best CFF default and nominal
/// glyph widths. This algorithm is linear in UPEM+numGlyphs.
pub fn optimize_widths(widths: &[i64]) -> (i64, i64) {
	let mut frequencies = BTreeMap::new();
	for &w in widths {
		*frequencies.entry(w).or_insert(0) += 1;
	}
	let minw = *frequencies.keys().next().unwrap_or(&0);
	let maxw = *frequencies.keys().next_back().unwrap_or(&0);
	let add = |a: i64, b: i64| a + b;
	let max = |a: i64, b: i64| a.max(b);

	// Cumulative sum/max forward/backward.
	let cum_frq_u = Cumulative::new(&frequencies, add, false);
	let cum_max_u = Cumulative::new(&frequencies, max, false);
	let cum_frq_d = Cumulative::new(&frequencies, add, true);
	let cum_max_d = Cumulative::new(&frequencies, max, true);

	// Cost per nominal choice, without default consideration.
	let nomn_cost = |x: i64| {
		let up = cum_frq_u.get(x) + cum_frq_u.get(x - 108) + cum_frq_u.get(x - 1132) * 3;
		let down = cum_frq_d.get(x) + cum_frq_d.get(x + 108) + cum_frq_d.get(x + 1132) * 3;
		up + down - frequencies.get(&x).copied().unwrap_or(0)
	};
	// Cost-saving per nominal choice, by best default choice.
	let dflt_cost_u = |x: i64| {
		cum_max_u
			.get(x)
			.max(cum_max_u.get(x - 108) * 2)
			.max(cum_max_u.get(x - 1132) * 5)
	};
	let dflt_cost_d = |x: i64| {
		cum_max_d
			.get(x)
			.max(cum_max_d.get(x + 108) * 2)
			.max(cum_max_d.get(x + 1132) * 5)
	};
	let dflt_cost = |x: i64| dflt_cost_u(x).max(dflt_cost_d(x));
	// Combined cost per nominal choice.
	let best_cost = |x: i64| nomn_cost(x) - dflt_cost(x);

	// Best nominal: the first with the lowest cost.
	let mut nominal = minw;
	let mut lowest = best_cost(minw);
	for x in minw..=maxw {
		let cost = best_cost(x);
		if cost < lowest {
			lowest = cost;
			nominal = x;
		}
	}

	// Work back the best default.
	let dflt_c = nomn_cost(nominal) - best_cost(nominal);
	let mut ends = Vec::with_capacity(3);
	if dflt_c == dflt_cost_u(nominal) {
		for mut start in [nominal, nominal - 108, nominal - 1132] {
			while cum_max_u.get(start) != 0 && cum_max_u.get(start) == cum_max_u.get(start - 1) {
				start -= 1;
			}
			ends.push(start);
		}
	} else {
		for mut start in [nominal, nominal + 108, nominal + 1132] {
			while cum_max_d.get(start) != 0 && cum_max_d.get(start) == cum_max_d.get(start + 1) {
				start += 1;
			}
			ends.push(start);
		}
	}
	let mut default = ends[0];
	let mut lowest = byte_cost(&frequencies, default, nominal);
	for &end in &ends[1..] {
		let cost = byte_cost(&frequencies, end, nominal);
		if cost < lowest {
			lowest = cost;
			default = end;
		}
	}
	(default, nominal)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Values from Python.
	#[test]
	fn widths() {
		assert_eq!(optimize_widths(&[500]), (500, 500));
		assert_eq!(optimize_widths(&[500, 500, 600]), (500, 500));
		assert_eq!(
			optimize_widths(&[
				1313, 1214, 367, 857, 1975, 1336, 1070, 1381, 1289, 234, 1340, 126, 1961, 1815,
				1060, 631
			]),
			(126, 1274)
		);
		assert_eq!(
			optimize_widths(&[
				1562, 1714, 1986, 974, 908, 1591, 1740, 1281, 1010, 374, 1899, 848, 299, 173, 378,
				1113, 544, 628, 1476, 993, 1695, 1383, 1851, 716, 962
			]),
			(173, 903)
		);
		assert_eq!(
			optimize_widths(&[
				1806, 890, 1275, 818, 1193, 1298, 934, 1296, 575, 1951, 789, 1496, 1975, 158, 1853,
				672, 1340, 1474, 1524, 434, 1530, 1862, 768, 1209, 1952, 1271, 1265, 313, 1561,
				1442, 532, 1396, 1803
			]),
			(158, 1289)
		);
	}
}
