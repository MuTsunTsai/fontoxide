//! Type 2 charstring glyph width optimizer (`fontTools.cffLib.width`).
//!
//! CFF glyphs whose width equals the Private DICT's `defaultWidthX` value do not need
//! to specify their width in their charstring, saving bytes. This module determines
//! the optimum `defaultWidthX` and `nominalWidthX` values for a font; [`main`] is its
//! command line.

use std::collections::BTreeMap;
use std::io::Write;

use fontoxide_misc::argparse::{Argument, Command, Nargs};

use super::cff2_to_cff::get_hmtx_tag;
use crate::tables::Hmtx;
use crate::{Error, Font, ReadOptions, Result};

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

/// The number of glyphs of each width.
fn frequencies(widths: &[i64]) -> BTreeMap<i64, i64> {
	let mut frequencies = BTreeMap::new();
	for &w in widths {
		*frequencies.entry(w).or_insert(0) += 1;
	}
	frequencies
}

/// `byteCost`: the number of bytes the widths take in the charstrings with
/// the default and nominal widths (`None`: no default width).
pub fn byte_cost(widths: &[i64], default: Option<i64>, nominal: i64) -> i64 {
	frequency_byte_cost(&frequencies(widths), default, nominal)
}

/// `byteCost` of the frequencies of the widths.
fn frequency_byte_cost(widths: &BTreeMap<i64, i64>, default: Option<i64>, nominal: i64) -> i64 {
	let mut cost = 0;
	for (&w, &freq) in widths {
		if Some(w) == default {
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
	let frequencies = frequencies(widths);
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
	let mut lowest = frequency_byte_cost(&frequencies, Some(default), nominal);
	for &end in &ends[1..] {
		let cost = frequency_byte_cost(&frequencies, Some(end), nominal);
		if cost < lowest {
			lowest = cost;
			default = end;
		}
	}
	(default, nominal)
}

/// `optimizeWidthsBruteforce`: [`optimize_widths`] by trying every pair of
/// widths. Very slow; only works for the smallest of fonts.
pub fn optimize_widths_bruteforce(widths: &[i64]) -> (i64, i64) {
	let frequencies = frequencies(widths);
	// Maximum number of bytes using default can possibly save
	let max_default_advantage = 5 * frequencies.values().copied().max().unwrap_or(0);
	let minw = widths.iter().copied().min().unwrap_or(0);
	let maxw = widths.iter().copied().max().unwrap_or(0);

	let mut best_cost = widths.len() as i64 * 5 + 1;
	let (mut best_default, mut best_nominal) = (minw, minw);
	for nominal in minw..=maxw {
		if frequency_byte_cost(&frequencies, None, nominal) > best_cost + max_default_advantage {
			continue;
		}
		for default in minw..=maxw {
			let cost = frequency_byte_cost(&frequencies, Some(default), nominal);
			if cost < best_cost {
				best_cost = cost;
				best_default = default;
				best_nominal = nominal;
			}
		}
	}
	(best_default, best_nominal)
}

const HELP: &str = "usage: fonttools cffLib.width [-h] [-b] FILE [FILE ...]

Calculate optimum defaultWidthX/nominalWidthX values

positional arguments:
  FILE               Input TTF files

options:
  -h, --help         show this help message and exit
  -b, --brute-force  Use brute-force approach (VERY slow)
";

const COMMAND: Command = Command {
	prog: "fonttools cffLib.width",
	module: "cffLib.width",
	help: HELP,
	arguments: &[
		Argument::help(),
		Argument::positional("FILE").nargs(Nargs::OneOrMore),
		Argument::flag(&["-b", "--brute-force"]),
	],
	required_groups: &[],
};

/// `fontTools.cffLib.width.main`: prints the optimum `defaultWidthX` and
/// `nominalWidthX` of each font given on the command line. Returns the exit
/// code.
pub fn main(args: &[&str]) -> Result<i32> {
	run(
		args,
		&mut std::io::stdout().lock(),
		&mut std::io::stderr().lock(),
	)
}

/// [`main`], writing to `out` and `err`.
pub fn run(args: &[&str], out: &mut dyn Write, err: &mut dyn Write) -> Result<i32> {
	let options = match COMMAND.parse_args(args, out, err)? {
		Ok(options) => options,
		Err(code) => return Ok(code),
	};
	for fontfile in options.list("FILE") {
		let mut font = Font::read(std::fs::read(fontfile)?, ReadOptions::default())?;
		let tag = get_hmtx_tag(&font);
		let hmtx = font.get_at::<Hmtx>(tag)?.ok_or(Error::TableNotFound(tag))?;
		let widths: Vec<i64> = hmtx.metrics.values().map(|m| i64::from(m.0)).collect();
		let (default, nominal) = if options.flag("--brute-force") {
			optimize_widths_bruteforce(&widths)
		} else {
			optimize_widths(&widths)
		};
		writeln!(
			out,
			"glyphs={} default={default} nominal={nominal} byteCost={}",
			widths.len(),
			byte_cost(&widths, Some(default), nominal)
		)?;
	}
	Ok(0)
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
