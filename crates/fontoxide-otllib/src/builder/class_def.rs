//! `ClassDefBuilder`.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use fontoxide_ttlib::ot::types::ClassDef;
use indexmap::IndexMap;

use crate::{Error, GlyphMap, Result};

/// Collects glyph classes for a class definition table (`ClassDefBuilder`).
///
/// A class is a list of glyph names; fontTools sorts classes given as sets, so
/// pass sets of glyphs sorted. A glyph can only be in one class.
#[derive(Debug, Clone)]
pub struct ClassDefBuilder {
	classes: BTreeSet<Vec<String>>,
	/// The glyphs in a class (`glyphs_`, whose values fontTools never reads).
	glyphs: HashSet<String>,
	use_class0: bool,
}

impl ClassDefBuilder {
	/// A builder (`ClassDefBuilder(useClass0)`). With `use_class0`, the largest
	/// class gets class 0, which needs no encoding; otherwise class 0 stays
	/// empty, meaning all other glyphs.
	pub fn new(use_class0: bool) -> Self {
		ClassDefBuilder {
			classes: BTreeSet::new(),
			glyphs: HashSet::new(),
			use_class0,
		}
	}

	/// Whether `glyphs` can be added: it is one of the classes already, or none
	/// of its glyphs is in a class (`canAdd`).
	pub fn can_add(&self, glyphs: &[String]) -> bool {
		if self.classes.contains(glyphs) {
			return true;
		}
		!glyphs.iter().any(|g| self.glyphs.contains(g))
	}

	/// Adds a class (`add`), failing if one of its glyphs is in another class.
	pub fn add(&mut self, glyphs: &[String]) -> Result<()> {
		if self.classes.contains(glyphs) {
			return Ok(());
		}
		self.classes.insert(glyphs.to_vec());
		for glyph in glyphs {
			if !self.glyphs.insert(glyph.clone()) {
				return Err(Error::otl(
					format!("Glyph {glyph} is already present in class."),
					&None,
				));
			}
		}
		Ok(())
	}

	/// The classes, in the order of their class IDs (`classes`): the largest
	/// first, then by glyph names. Without `use_class0`, class 0 is an empty
	/// class.
	pub fn classes(&self) -> Vec<Vec<String>> {
		let mut result: Vec<Vec<String>> = self.classes.iter().cloned().collect();
		result.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
		if !self.use_class0 {
			result.insert(0, Vec::new());
		}
		result
	}

	/// The class of each glyph not in class 0, by glyph name (`classDefs` of
	/// the built table).
	pub fn class_defs(&self) -> IndexMap<String, u32> {
		let mut out = IndexMap::new();
		for (class_id, glyphs) in self.classes().into_iter().enumerate() {
			if class_id == 0 {
				continue;
			}
			for glyph in glyphs {
				out.insert(glyph, class_id as u32);
			}
		}
		out
	}

	/// Builds the class definition table (`build`).
	pub fn build(&self, glyph_map: &GlyphMap) -> Result<ClassDef> {
		let mut class_defs = BTreeMap::new();
		for (glyph, class) in self.class_defs() {
			class_defs.insert(glyph_map.id(&glyph)?, class);
		}
		Ok(ClassDef { class_defs })
	}
}
