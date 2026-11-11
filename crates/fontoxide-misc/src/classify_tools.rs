//! Port of `fontTools.misc.classifyTools`: classifying things into similar
//! sets.

use std::collections::{BTreeMap, BTreeSet};

/// Classifies things into similar sets (`Classifier`): every class is either a
/// subset of, or disjoint from, each of the sets added.
#[derive(Debug, Clone)]
pub struct Classifier<T> {
	/// All things known so far (`_things`).
	things: BTreeSet<T>,
	/// The class sets produced so far (`_sets`); emptied classes stay until
	/// they are processed.
	sets: Vec<BTreeSet<T>>,
	/// The index in `sets` of the class of each thing (`_mapping`).
	mapping: BTreeMap<T, usize>,
	dirty: bool,
	sort: bool,
}

impl<T: Ord + Clone> Default for Classifier<T> {
	fn default() -> Self {
		Self::new(true)
	}
}

impl<T: Ord + Clone> Classifier<T> {
	/// `Classifier(sort=sort)`: with `sort`, the classes are sorted by
	/// decreasing size, then by their sorted things.
	pub fn new(sort: bool) -> Self {
		Classifier {
			things: BTreeSet::new(),
			sets: Vec::new(),
			mapping: BTreeMap::new(),
			dirty: false,
			sort,
		}
	}

	/// Adds a set (`add`).
	///
	/// The classes split by the set are processed in the order of their
	/// smallest thing in it, which is the order of Python's set iteration for
	/// small integers; it only matters to the order of unsorted classes.
	pub fn add(&mut self, set_of_things: impl IntoIterator<Item = T>) {
		let s: BTreeSet<T> = set_of_things.into_iter().collect();
		if s.is_empty() {
			return;
		}
		self.dirty = true;
		let (mut intersection, difference): (BTreeSet<T>, BTreeSet<T>) =
			s.into_iter().partition(|t| self.things.contains(t));
		// Add new class for new things
		if !difference.is_empty() {
			let index = self.sets.len();
			for thing in &difference {
				self.things.insert(thing.clone());
				self.mapping.insert(thing.clone(), index);
			}
			self.sets.push(difference);
		}
		while let Some(first) = intersection.first() {
			// Take one item and process the old class it belongs to
			let Some(&old) = self.mapping.get(first) else {
				break;
			};
			let old_class_intersection: BTreeSet<T> = self.sets[old]
				.intersection(&intersection)
				.cloned()
				.collect();
			// Update old class to remove items from new set, and remove processed
			// items from todo list
			for thing in &old_class_intersection {
				self.sets[old].remove(thing);
				intersection.remove(thing);
			}
			// Add new class for the intersection with old class
			let index = self.sets.len();
			for thing in &old_class_intersection {
				self.mapping.insert(thing.clone(), index);
			}
			self.sets.push(old_class_intersection);
		}
	}

	/// Adds sets (`update`).
	pub fn update<I: IntoIterator<Item = T>>(&mut self, list_of_sets: impl IntoIterator<Item = I>) {
		for s in list_of_sets {
			self.add(s);
		}
	}

	/// Drops the emptied classes and sorts them (`_process`).
	fn process(&mut self) {
		if !self.dirty {
			return;
		}
		let mut sets: Vec<BTreeSet<T>> = std::mem::take(&mut self.sets)
			.into_iter()
			.filter(|s| !s.is_empty())
			.collect();
		if self.sort {
			// `key=lambda s: (-len(s), sorted(s))`
			sets.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.iter().cmp(b.iter())));
		}
		self.mapping.clear();
		for (i, set) in sets.iter().enumerate() {
			for thing in set {
				self.mapping.insert(thing.clone(), i);
			}
		}
		self.sets = sets;
		self.dirty = false;
	}

	/// All things known so far (`getThings`).
	pub fn get_things(&mut self) -> &BTreeSet<T> {
		self.process();
		&self.things
	}

	/// The class sets (`getClasses`).
	pub fn get_classes(&mut self) -> &[BTreeSet<T>] {
		self.process();
		&self.sets
	}

	/// The index in [`Classifier::get_classes`] of the class of each thing
	/// (`getMapping`, which maps to the class sets themselves).
	pub fn get_mapping(&mut self) -> &BTreeMap<T, usize> {
		self.process();
		&self.mapping
	}
}

/// Classifies the things present in any of the sets into the smallest list of
/// sets such that each one is either a subset of, or disjoint from, each of the
/// input sets (`classify`). Returns the classes and the class of each thing.
///
/// With `sort`, the classes are sorted by decreasing size and their sorted
/// things; otherwise they are in the order they were identified.
///
/// ```
/// use std::collections::BTreeSet;
/// use fontoxide_misc::classify_tools::classify;
/// let (classes, mapping) = classify([vec![1, 2], vec![2, 4, 5]], true);
/// assert_eq!(classes, [BTreeSet::from([4, 5]), BTreeSet::from([1]), BTreeSet::from([2])]);
/// assert_eq!(mapping[&5], BTreeSet::from([4, 5]));
/// ```
pub fn classify<T: Ord + Clone, I: IntoIterator<Item = T>>(
	list_of_sets: impl IntoIterator<Item = I>,
	sort: bool,
) -> (Vec<BTreeSet<T>>, BTreeMap<T, BTreeSet<T>>) {
	let mut classifier = Classifier::new(sort);
	classifier.update(list_of_sets);
	let classes = classifier.get_classes().to_vec();
	let mapping = classifier
		.get_mapping()
		.iter()
		.map(|(thing, &i)| (thing.clone(), classes[i].clone()))
		.collect();
	(classes, mapping)
}
