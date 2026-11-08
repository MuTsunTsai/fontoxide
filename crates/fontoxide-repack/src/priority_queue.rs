//! `hb_priority_queue_t` (`hb-priority-queue.hh`): a binary min-heap of
//! `(priority, value)` pairs. Ties are resolved by the heap layout, so the
//! sift operations follow HarfBuzz exactly.

#[derive(Debug, Default)]
pub(crate) struct PriorityQueue {
	heap: Vec<(i64, u32)>,
}

impl PriorityQueue {
	pub fn with_capacity(n: usize) -> Self {
		PriorityQueue {
			heap: Vec::with_capacity(n),
		}
	}

	pub fn insert(&mut self, priority: i64, value: u32) {
		self.heap.push((priority, value));
		self.bubble_up(self.heap.len() - 1);
	}

	/// Removes and returns the pair with the lowest priority.
	pub fn pop_minimum(&mut self) -> Option<(i64, u32)> {
		let last = self.heap.pop()?;
		if self.heap.is_empty() {
			return Some(last);
		}
		let result = std::mem::replace(&mut self.heap[0], last);
		self.bubble_down(0);
		Some(result)
	}

	fn bubble_down(&mut self, mut index: usize) {
		loop {
			let left = 2 * index + 1;
			let right = 2 * index + 2;
			if left >= self.heap.len() {
				return;
			}
			let has_right = right < self.heap.len();
			let p = self.heap[index].0;
			if p <= self.heap[left].0 && (!has_right || p <= self.heap[right].0) {
				return;
			}
			let child = if !has_right || self.heap[left].0 < self.heap[right].0 {
				left
			} else {
				right
			};
			self.heap.swap(index, child);
			index = child;
		}
	}

	fn bubble_up(&mut self, mut index: usize) {
		while index != 0 {
			let parent = (index - 1) / 2;
			if self.heap[parent].0 <= self.heap[index].0 {
				return;
			}
			self.heap.swap(index, parent);
			index = parent;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn order() {
		let mut q = PriorityQueue::default();
		for (p, v) in [(5, 0), (1, 1), (3, 2), (1, 3), (0, 4)] {
			q.insert(p, v);
		}
		let mut out = Vec::new();
		while let Some((p, _)) = q.pop_minimum() {
			out.push(p);
		}
		assert_eq!(out, [0, 1, 1, 3, 5]);
	}
}
