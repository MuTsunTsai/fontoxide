//! The part of Python's `difflib` (CPython 3.13) that `fontTools.diff` uses:
//! `SequenceMatcher` without a junk function, with its "popular elements"
//! heuristic (`autojunk`), and `unified_diff`.

use std::collections::HashMap;
use std::hash::Hash;

/// An operation turning a range of `a` into a range of `b` (`get_opcodes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
	/// `replace`: `a[i1:i2]` is replaced by `b[j1:j2]`.
	Replace,
	/// `delete`: `a[i1:i2]` is deleted.
	Delete,
	/// `insert`: `b[j1:j2]` is inserted at `a[i1:i1]`.
	Insert,
	/// `equal`: `a[i1:i2] == b[j1:j2]`.
	Equal,
}

/// An opcode: the tag and the ranges `(i1, i2, j1, j2)`. The ranges are signed:
/// with a negative number of context lines, `get_grouped_opcodes` makes
/// ranges that Python slices as it does negative indices.
pub type Opcode = (Tag, i128, i128, i128, i128);

/// `SequenceMatcher(None, a, b, autojunk)`: finds the longest matching blocks
/// of two sequences, recursively.
#[derive(Debug)]
pub struct SequenceMatcher<'a, T: Eq + Hash> {
	a: &'a [T],
	b: &'a [T],
	/// The indices in `b` of each element, without the popular ones (`b2j`).
	b2j: HashMap<&'a T, Vec<usize>>,
}

/// A matching block: `a[a..a + size] == b[b..b + size]` (`Match`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Match {
	/// The start in `a`.
	pub a: usize,
	/// The start in `b`.
	pub b: usize,
	/// The length.
	pub size: usize,
}

impl<'a, T: Eq + Hash> SequenceMatcher<'a, T> {
	/// `SequenceMatcher(None, a, b, autojunk=autojunk)`.
	pub fn new(a: &'a [T], b: &'a [T], autojunk: bool) -> Self {
		// `__chain_b`.
		let mut b2j: HashMap<&'a T, Vec<usize>> = HashMap::new();
		for (i, elt) in b.iter().enumerate() {
			b2j.entry(elt).or_default().push(i);
		}
		// Purge popular elements that are not junk.
		let n = b.len();
		if autojunk && n >= 200 {
			let ntest = n / 100 + 1;
			b2j.retain(|_, idxs| idxs.len() <= ntest);
		}
		SequenceMatcher { a, b, b2j }
	}

	/// `find_longest_match(alo, ahi, blo, bhi)`: the longest matching block in
	/// `a[alo..ahi]` and `b[blo..bhi]`, the earliest in `a`, then in `b`; then
	/// extended by the popular elements around it.
	pub fn find_longest_match(&self, alo: usize, ahi: usize, blo: usize, bhi: usize) -> Match {
		let (a, b) = (self.a, self.b);
		let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0);
		// j2len[j] = length of longest match ending with a[i-1] and b[j].
		let mut j2len: HashMap<usize, usize> = HashMap::new();
		let mut newj2len: HashMap<usize, usize> = HashMap::new();
		for (i, ai) in a.iter().enumerate().take(ahi).skip(alo) {
			newj2len.clear();
			if let Some(indices) = self.b2j.get(ai) {
				for &j in indices {
					// a[i] matches b[j]
					if j < blo {
						continue;
					}
					if j >= bhi {
						break;
					}
					let k = j
						.checked_sub(1)
						.and_then(|p| j2len.get(&p))
						.copied()
						.unwrap_or(0)
						+ 1;
					newj2len.insert(j, k);
					if k > bestsize {
						besti = i + 1 - k;
						bestj = j + 1 - k;
						bestsize = k;
					}
				}
			}
			std::mem::swap(&mut j2len, &mut newj2len);
		}

		// Extend the best by non-junk elements on each end; without a junk
		// function, nothing is junk.
		while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
			besti -= 1;
			bestj -= 1;
			bestsize += 1;
		}
		while besti + bestsize < ahi
			&& bestj + bestsize < bhi
			&& a[besti + bestsize] == b[bestj + bestsize]
		{
			bestsize += 1;
		}
		Match {
			a: besti,
			b: bestj,
			size: bestsize,
		}
	}

	/// `get_matching_blocks`: the matching blocks, increasing in `a` and `b`,
	/// adjacent ones collapsed, ending with `(len(a), len(b), 0)`.
	pub fn get_matching_blocks(&self) -> Vec<Match> {
		let (la, lb) = (self.a.len(), self.b.len());
		let mut queue = vec![(0, la, 0, lb)];
		let mut matching_blocks = Vec::new();
		while let Some((alo, ahi, blo, bhi)) = queue.pop() {
			let x = self.find_longest_match(alo, ahi, blo, bhi);
			let (i, j, k) = (x.a, x.b, x.size);
			if k > 0 {
				matching_blocks.push(x);
				if alo < i && blo < j {
					queue.push((alo, i, blo, j));
				}
				if i + k < ahi && j + k < bhi {
					queue.push((i + k, ahi, j + k, bhi));
				}
			}
		}
		matching_blocks.sort();

		// Collapse adjacent blocks.
		let (mut i1, mut j1, mut k1) = (0, 0, 0);
		let mut non_adjacent = Vec::new();
		for m in matching_blocks {
			if i1 + k1 == m.a && j1 + k1 == m.b {
				k1 += m.size;
			} else {
				if k1 > 0 {
					non_adjacent.push(Match {
						a: i1,
						b: j1,
						size: k1,
					});
				}
				(i1, j1, k1) = (m.a, m.b, m.size);
			}
		}
		if k1 > 0 {
			non_adjacent.push(Match {
				a: i1,
				b: j1,
				size: k1,
			});
		}
		non_adjacent.push(Match {
			a: la,
			b: lb,
			size: 0,
		});
		non_adjacent
	}

	/// `get_opcodes`: how to turn `a` into `b`.
	pub fn get_opcodes(&self) -> Vec<Opcode> {
		let (mut i, mut j) = (0, 0);
		let mut answer = Vec::new();
		for m in self.get_matching_blocks() {
			let (ai, bj, size) = (m.a, m.b, m.size);
			let tag = if i < ai && j < bj {
				Some(Tag::Replace)
			} else if i < ai {
				Some(Tag::Delete)
			} else if j < bj {
				Some(Tag::Insert)
			} else {
				None
			};
			if let Some(tag) = tag {
				answer.push((tag, i as i128, ai as i128, j as i128, bj as i128));
			}
			(i, j) = (ai + size, bj + size);
			if size > 0 {
				answer.push((Tag::Equal, ai as i128, i as i128, bj as i128, j as i128));
			}
		}
		answer
	}

	/// `get_grouped_opcodes(n)`: the groups of changes with `n` lines of
	/// context.
	pub fn get_grouped_opcodes(&self, n: i128) -> Vec<Vec<Opcode>> {
		let mut codes = self.get_opcodes();
		if codes.is_empty() {
			codes.push((Tag::Equal, 0, 1, 0, 1));
		}
		// Fixup leading and trailing groups if they show no changes.
		if let Some(first) = codes.first_mut()
			&& first.0 == Tag::Equal
		{
			let (tag, i1, i2, j1, j2) = *first;
			*first = (tag, i1.max(i2 - n), i2, j1.max(j2 - n), j2);
		}
		if let Some(last) = codes.last_mut()
			&& last.0 == Tag::Equal
		{
			let (tag, i1, i2, j1, j2) = *last;
			*last = (tag, i1, i2.min(i1 + n), j1, j2.min(j1 + n));
		}

		let nn = n + n;
		let mut groups = Vec::new();
		let mut group = Vec::new();
		for (tag, mut i1, i2, mut j1, j2) in codes {
			// End the current group and start a new one whenever there is a
			// large range with no changes.
			if tag == Tag::Equal && i2 - i1 > nn {
				group.push((tag, i1, i2.min(i1 + n), j1, j2.min(j1 + n)));
				groups.push(std::mem::take(&mut group));
				i1 = i1.max(i2 - n);
				j1 = j1.max(j2 - n);
			}
			group.push((tag, i1, i2, j1, j2));
		}
		if !group.is_empty() && !(group.len() == 1 && group[0].0 == Tag::Equal) {
			groups.push(group);
		}
		groups
	}
}

/// `seq[start:stop]` with Python's slicing of negative and out-of-range
/// indices.
fn py_slice<T>(seq: &[T], start: i128, stop: i128) -> &[T] {
	let len = seq.len() as i128;
	let clamp = |i: i128| {
		let i = if i < 0 { i + len } else { i };
		i.clamp(0, len) as usize
	};
	let (start, stop) = (clamp(start), clamp(stop));
	if stop <= start {
		&[]
	} else {
		&seq[start..stop]
	}
}

/// `_format_range_unified`: a range in the "ed" format.
fn format_range_unified(start: i128, stop: i128) -> String {
	let mut beginning = start + 1; // lines start numbering with one
	let length = stop - start;
	if length == 1 {
		return beginning.to_string();
	}
	if length == 0 {
		beginning -= 1; // empty ranges begin at line just before the range
	}
	format!("{beginning},{length}")
}

/// `unified_diff(a, b, fromfile, tofile, fromfiledate, tofiledate, n,
/// lineterm)`: the lines of the unified diff of two sequences of lines, with
/// `n` lines of context.
#[allow(clippy::too_many_arguments)]
pub fn unified_diff(
	a: &[&str],
	b: &[&str],
	fromfile: &str,
	tofile: &str,
	fromfiledate: &str,
	tofiledate: &str,
	n: i128,
	lineterm: &str,
) -> Vec<String> {
	let mut lines = Vec::new();
	let mut started = false;
	for group in SequenceMatcher::new(a, b, true).get_grouped_opcodes(n) {
		if !started {
			started = true;
			let date = |d: &str| {
				if d.is_empty() {
					String::new()
				} else {
					format!("\t{d}")
				}
			};
			lines.push(format!("--- {fromfile}{}{lineterm}", date(fromfiledate)));
			lines.push(format!("+++ {tofile}{}{lineterm}", date(tofiledate)));
		}
		let (Some(first), Some(last)) = (group.first(), group.last()) else {
			continue;
		};
		let file1_range = format_range_unified(first.1, last.2);
		let file2_range = format_range_unified(first.3, last.4);
		lines.push(format!("@@ -{file1_range} +{file2_range} @@{lineterm}"));

		for &(tag, i1, i2, j1, j2) in &group {
			if tag == Tag::Equal {
				lines.extend(py_slice(a, i1, i2).iter().map(|line| format!(" {line}")));
				continue;
			}
			if matches!(tag, Tag::Replace | Tag::Delete) {
				lines.extend(py_slice(a, i1, i2).iter().map(|line| format!("-{line}")));
			}
			if matches!(tag, Tag::Replace | Tag::Insert) {
				lines.extend(py_slice(b, j1, j2).iter().map(|line| format!("+{line}")));
			}
		}
	}
	lines
}

#[cfg(test)]
mod tests {
	//! The doctests of `difflib.SequenceMatcher`.

	use super::*;

	fn chars(s: &str) -> Vec<char> {
		s.chars().collect()
	}

	#[test]
	fn longest_match() {
		let (a, b) = (chars(" abcd"), chars("abcd abcd"));
		let s = SequenceMatcher::new(&a, &b, true);
		assert_eq!(
			s.find_longest_match(0, 5, 0, 9),
			Match {
				a: 0,
				b: 4,
				size: 5
			}
		);
		let (a, b) = (chars("ab"), chars("c"));
		let s = SequenceMatcher::new(&a, &b, true);
		assert_eq!(
			s.find_longest_match(0, 2, 0, 1),
			Match {
				a: 0,
				b: 0,
				size: 0
			}
		);
	}

	#[test]
	fn matching_blocks_and_opcodes() {
		let (a, b) = (chars("abxcd"), chars("abcd"));
		let s = SequenceMatcher::new(&a, &b, true);
		let blocks: Vec<_> = s
			.get_matching_blocks()
			.iter()
			.map(|m| (m.a, m.b, m.size))
			.collect();
		assert_eq!(blocks, [(0, 0, 2), (3, 2, 2), (5, 4, 0)]);

		let (a, b) = (chars("qabxcd"), chars("abycdf"));
		let s = SequenceMatcher::new(&a, &b, true);
		assert_eq!(
			s.get_opcodes(),
			[
				(Tag::Delete, 0, 1, 0, 0),
				(Tag::Equal, 1, 3, 0, 2),
				(Tag::Replace, 3, 4, 2, 3),
				(Tag::Equal, 4, 6, 3, 5),
				(Tag::Insert, 6, 6, 5, 6),
			]
		);
	}

	#[test]
	fn grouped_opcodes() {
		let a: Vec<String> = (1..40).map(|i| i.to_string()).collect();
		let mut b = a.clone();
		b.insert(8, "i".into()); // Make an insertion
		b[20] += "x"; // Make a replacement
		b.drain(23..28); // Make a deletion
		b[30] += "y"; // Make another replacement
		let s = SequenceMatcher::new(&a, &b, true);
		use Tag::*;
		assert_eq!(
			s.get_grouped_opcodes(3),
			[
				vec![
					(Equal, 5, 8, 5, 8),
					(Insert, 8, 8, 8, 9),
					(Equal, 8, 11, 9, 12)
				],
				vec![
					(Equal, 16, 19, 17, 20),
					(Replace, 19, 20, 20, 21),
					(Equal, 20, 22, 21, 23),
					(Delete, 22, 27, 23, 23),
					(Equal, 27, 30, 23, 26)
				],
				vec![
					(Equal, 31, 34, 27, 30),
					(Replace, 34, 35, 30, 31),
					(Equal, 35, 38, 31, 34)
				],
			]
		);
	}

	#[test]
	fn unified() {
		let a = ["one", "two", "three", "four"];
		let b = ["zero", "one", "tree", "four"];
		assert_eq!(
			unified_diff(
				&a,
				&b,
				"Original",
				"Current",
				"2005-01-26 23:30:50",
				"2010-04-02 10:20:52",
				3,
				""
			),
			[
				"--- Original\t2005-01-26 23:30:50",
				"+++ Current\t2010-04-02 10:20:52",
				"@@ -1,4 +1,4 @@",
				"+zero",
				" one",
				"-two",
				"-three",
				"+tree",
				" four"
			]
		);
		// Identical sequences have no diff, except with a negative context.
		assert!(unified_diff(&a, &a, "a", "b", "", "", 3, "\n").is_empty());
		assert_eq!(
			unified_diff(&a, &a, "a", "b", "", "", -1, "\n"),
			["--- a\n", "+++ b\n", "@@ -6,-1 +6,-1 @@\n"]
		);
	}
}
