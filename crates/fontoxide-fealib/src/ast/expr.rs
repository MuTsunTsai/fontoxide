//! Expressions: glyphs, glyph classes, anchors, value records and numbers.

use std::borrow::Cow;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use fontoxide_misc::py::float_repr;
use indexmap::IndexMap;

use super::{Device, glyph_as_fea, range_as_fea};
use super::{GlyphClassDefinition, MarkClassDefinition, device_to_string, device_truthy};
use crate::error::{Error, Result};
use crate::location::Location;
use crate::variable_scalar::VariableScalar;

/// The glyphs of a glyph-containing object (`glyphSet()`), borrowed where
/// possible.
pub type GlyphSet<'a> = Cow<'a, [String]>;

/// An integer or a [`VariableScalar`]: the coordinates of anchors, the values of
/// value records and ligature caret positions (`expect_number_(variable=True)`).
#[derive(Debug, Clone, PartialEq)]
pub enum ScalarValue {
	/// A plain integer.
	Int(i64),
	/// A value that varies across the designspace.
	Variable(VariableScalar),
}

impl ScalarValue {
	/// Python truthiness: `0` is falsy, a `VariableScalar` is always truthy.
	pub fn is_truthy(&self) -> bool {
		match self {
			ScalarValue::Int(i) => *i != 0,
			ScalarValue::Variable(_) => true,
		}
	}
}

impl From<i64> for ScalarValue {
	fn from(value: i64) -> Self {
		ScalarValue::Int(value)
	}
}

impl fmt::Display for ScalarValue {
	/// `str(value)`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			ScalarValue::Int(i) => write!(f, "{i}"),
			ScalarValue::Variable(v) => write!(f, "{v}"),
		}
	}
}

/// An integer or a float, as in STAT locations and conditionsets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntOrFloat {
	/// An `int`.
	Int(i64),
	/// A `float`.
	Float(f64),
}

impl IntOrFloat {
	/// The value as a float (for comparisons).
	pub fn as_f64(self) -> f64 {
		match self {
			IntOrFloat::Int(i) => i as f64,
			IntOrFloat::Float(f) => f,
		}
	}
}

impl fmt::Display for IntOrFloat {
	/// `str(value)`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			IntOrFloat::Int(i) => write!(f, "{i}"),
			IntOrFloat::Float(x) => f.write_str(&float_repr(*x)),
		}
	}
}

/// The NULL glyph, used in glyph deletion substitutions (`NullGlyph`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NullGlyph {
	/// `location`.
	pub location: Location,
}

impl NullGlyph {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		"NULL".to_owned()
	}
}

/// A single glyph name, such as `cedilla` (`GlyphName`).
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphName {
	/// The name itself.
	pub glyph: String,
	/// `location`.
	pub location: Location,
}

impl GlyphName {
	/// `GlyphName(glyph)`.
	pub fn new(glyph: impl Into<String>) -> Self {
		GlyphName {
			glyph: glyph.into(),
			location: None,
		}
	}

	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		glyph_as_fea(&self.glyph)
	}
}

/// A reference to a named class inside a glyph class: what `add_class` takes.
#[derive(Debug, Clone)]
pub enum ClassName {
	/// A `@name` defined as a glyph class.
	GlyphClassName(GlyphClassName),
	/// A `@name` defined with `markClass`.
	MarkClassName(MarkClassName),
}

impl ClassName {
	/// `glyphSet()`.
	pub fn glyph_set(&self) -> GlyphSet<'_> {
		match self {
			ClassName::GlyphClassName(c) => c.glyph_set(),
			ClassName::MarkClassName(c) => c.glyph_set(),
		}
	}

	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		match self {
			ClassName::GlyphClassName(c) => c.as_fea(),
			ClassName::MarkClassName(c) => c.as_fea(),
		}
	}
}

/// An item of [`GlyphClass::original`]: how the class was written.
#[derive(Debug, Clone)]
pub enum GlyphClassItem {
	/// A glyph name.
	Glyph(String),
	/// A range `start - end`; a CID range is stored as `\start`, `\end`.
	Range(String, String),
	/// A class reference.
	Class(ClassName),
}

impl GlyphClassItem {
	/// `asFea(item)`.
	pub fn as_fea(&self) -> String {
		match self {
			GlyphClassItem::Glyph(g) => glyph_as_fea(g),
			GlyphClassItem::Range(a, b) => range_as_fea(a, b),
			GlyphClassItem::Class(c) => c.as_fea(),
		}
	}
}

/// A glyph class, such as `[acute cedilla grave]` (`GlyphClass`).
///
/// `glyphs` holds the glyph names; `original` and `curr` remember ranges and class
/// references so that `asFea()` writes them as they were written.
// LENIENT(L33): Python's `glyphs` may hold `GlyphName` objects as well as strings when
// built by hand; here they are names, which serialize the same.
#[derive(Debug, Clone, Default)]
pub struct GlyphClass {
	/// The glyph names in this class.
	pub glyphs: Vec<String>,
	/// The items as written, once a range or class reference was added.
	pub original: Vec<GlyphClassItem>,
	/// How many of `glyphs` are accounted for in `original`.
	pub curr: usize,
	/// `location`.
	pub location: Location,
}

impl GlyphClass {
	/// `GlyphClass(glyphs)`.
	pub fn new(glyphs: Vec<String>) -> Self {
		GlyphClass {
			glyphs,
			..Default::default()
		}
	}

	/// `glyphSet()`.
	pub fn glyph_set(&self) -> GlyphSet<'_> {
		Cow::Borrowed(&self.glyphs)
	}

	/// `asFea()`.
	///
	/// Python also moves the trailing plain glyphs into `original` here; the
	/// output is the same without doing so.
	pub fn as_fea(&self) -> String {
		let items: Vec<String> = if self.original.is_empty() {
			self.glyphs.iter().map(|g| glyph_as_fea(g)).collect()
		} else {
			let mut items: Vec<String> = self.original.iter().map(GlyphClassItem::as_fea).collect();
			if self.curr < self.glyphs.len() {
				items.extend(self.glyphs[self.curr..].iter().map(|g| glyph_as_fea(g)));
			}
			items
		};
		format!("[{}]", items.join(" "))
	}

	/// Adds a list of glyphs (`extend`).
	pub fn extend(&mut self, glyphs: impl IntoIterator<Item = String>) {
		self.glyphs.extend(glyphs);
	}

	/// Adds a single glyph (`append`).
	pub fn append(&mut self, glyph: impl Into<String>) {
		self.glyphs.push(glyph.into());
	}

	fn flush_plain_glyphs(&mut self) {
		if self.curr < self.glyphs.len() {
			self.original.extend(
				self.glyphs[self.curr..]
					.iter()
					.cloned()
					.map(GlyphClassItem::Glyph),
			);
		}
	}

	/// Adds a range (e.g. `A-Z`); `glyphs` is the full list of glyphs in the range
	/// (`add_range`).
	pub fn add_range(&mut self, start: &str, end: &str, glyphs: Vec<String>) {
		self.flush_plain_glyphs();
		self.original
			.push(GlyphClassItem::Range(start.to_owned(), end.to_owned()));
		self.glyphs.extend(glyphs);
		self.curr = self.glyphs.len();
	}

	/// Adds a range by glyph ID (`add_cid_range`).
	pub fn add_cid_range(&mut self, start: i64, end: i64, glyphs: Vec<String>) {
		self.flush_plain_glyphs();
		self.original.push(GlyphClassItem::Range(
			format!("\\{start}"),
			format!("\\{end}"),
		));
		self.glyphs.extend(glyphs);
		self.curr = self.glyphs.len();
	}

	/// Adds the glyphs of a class reference (`add_class`).
	pub fn add_class(&mut self, gc: ClassName) {
		self.flush_plain_glyphs();
		self.glyphs.extend(gc.glyph_set().iter().cloned());
		self.original.push(GlyphClassItem::Class(gc));
		self.curr = self.glyphs.len();
	}
}

/// A glyph class name, such as `@FRENCH_MARKS` (`GlyphClassName`).
#[derive(Debug, Clone)]
pub struct GlyphClassName {
	/// The definition it refers to.
	pub glyphclass: Rc<GlyphClassDefinition>,
	/// `location`.
	pub location: Location,
}

impl GlyphClassName {
	/// `glyphSet()`.
	pub fn glyph_set(&self) -> GlyphSet<'_> {
		self.glyphclass.glyph_set()
	}

	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		format!("@{}", self.glyphclass.name)
	}
}

/// A shared, growing mark class.
pub type MarkClassRef = Rc<RefCell<MarkClass>>;

/// A mark class name, such as `@TOP_MARKS` defined with `markClass`
/// (`MarkClassName`).
#[derive(Debug, Clone)]
pub struct MarkClassName {
	/// The mark class it refers to (`markClass`).
	pub mark_class: MarkClassRef,
	/// `location`.
	pub location: Location,
}

impl MarkClassName {
	/// `glyphSet()`: the glyphs of the mark class as it is now.
	pub fn glyph_set(&self) -> GlyphSet<'_> {
		Cow::Owned(self.mark_class.borrow().glyph_set())
	}

	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		format!("@{}", self.mark_class.borrow().name)
	}
}

/// One or more `markClass` statements for the same mark class (`MarkClass`).
///
/// While glyph classes can be defined only once, the feature file format allows
/// expanding mark classes with multiple definitions, each using different glyphs
/// and anchors.
#[derive(Debug, Clone, Default)]
pub struct MarkClass {
	/// The class name, without `@`.
	pub name: String,
	/// The definitions, in order.
	pub definitions: Vec<Rc<MarkClassDefinition>>,
	/// Each glyph and the definition it is in.
	pub glyphs: IndexMap<String, Rc<MarkClassDefinition>>,
}

impl MarkClass {
	/// `MarkClass(name)`, ready to be shared.
	pub fn new(name: impl Into<String>) -> MarkClassRef {
		Rc::new(RefCell::new(MarkClass {
			name: name.into(),
			..Default::default()
		}))
	}

	/// Adds a `markClass` statement to a mark class (`addDefinition`).
	///
	/// This takes the shared reference because the glyphs of the definition may
	/// be the mark class itself (`markClass @X <anchor 0 0> @X;`).
	pub fn add_definition(this: &MarkClassRef, definition: Rc<MarkClassDefinition>) -> Result<()> {
		let glyphs = definition.glyph_set().into_owned();
		let mut this = this.borrow_mut();
		this.definitions.push(definition.clone());
		for glyph in glyphs {
			if let Some(other) = this.glyphs.get(&glyph) {
				let end = match &other.location {
					None => String::new(),
					Some(l) => format!(" at {l}"),
				};
				return Err(Error::feature_lib(
					format!("Glyph {glyph} already defined{end}"),
					definition.location.clone(),
				));
			}
			this.glyphs.insert(glyph, definition.clone());
		}
		Ok(())
	}

	/// `glyphSet()`.
	pub fn glyph_set(&self) -> Vec<String> {
		self.glyphs.keys().cloned().collect()
	}

	/// `asFea()`: the definitions, one per line.
	pub fn as_fea(&self) -> Result<String> {
		let defs: Result<Vec<String>> = self.definitions.iter().map(|d| d.as_fea()).collect();
		Ok(defs?.join("\n"))
	}
}

/// A "glyph-containing object": a glyph name, glyph class, class name or the
/// NULL glyph.
#[derive(Debug, Clone)]
pub enum GlyphContainer {
	/// `GlyphName`.
	GlyphName(GlyphName),
	/// `GlyphClass`.
	GlyphClass(GlyphClass),
	/// `GlyphClassName`.
	GlyphClassName(GlyphClassName),
	/// `MarkClassName`.
	MarkClassName(MarkClassName),
	/// `NullGlyph`.
	NullGlyph(NullGlyph),
}

impl GlyphContainer {
	/// `glyphSet()`.
	pub fn glyph_set(&self) -> GlyphSet<'_> {
		match self {
			GlyphContainer::GlyphName(g) => Cow::Owned(vec![g.glyph.clone()]),
			GlyphContainer::GlyphClass(g) => g.glyph_set(),
			GlyphContainer::GlyphClassName(g) => g.glyph_set(),
			GlyphContainer::MarkClassName(g) => g.glyph_set(),
			GlyphContainer::NullGlyph(_) => Cow::Borrowed(&[]),
		}
	}

	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		match self {
			GlyphContainer::GlyphName(g) => g.as_fea(),
			GlyphContainer::GlyphClass(g) => g.as_fea(),
			GlyphContainer::GlyphClassName(g) => g.as_fea(),
			GlyphContainer::MarkClassName(g) => g.as_fea(),
			GlyphContainer::NullGlyph(g) => g.as_fea(),
		}
	}

	/// `location`.
	pub fn location(&self) -> &Location {
		match self {
			GlyphContainer::GlyphName(g) => &g.location,
			GlyphContainer::GlyphClass(g) => &g.location,
			GlyphContainer::GlyphClassName(g) => &g.location,
			GlyphContainer::MarkClassName(g) => &g.location,
			GlyphContainer::NullGlyph(g) => &g.location,
		}
	}
}

/// An anchor, used inside a `pos` rule (`Anchor`).
///
/// If a `name` is given, it is used in preference to the coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Anchor {
	/// `x`.
	pub x: ScalarValue,
	/// `y`.
	pub y: ScalarValue,
	/// The name of a named anchor (`anchorDef`).
	pub name: Option<String>,
	/// `contourpoint`.
	pub contourpoint: Option<i64>,
	/// `xDeviceTable`.
	pub x_device_table: Option<Device>,
	/// `yDeviceTable`.
	pub y_device_table: Option<Device>,
	/// `location`.
	pub location: Location,
}

impl Anchor {
	/// `Anchor(x, y)`.
	pub fn new(x: impl Into<ScalarValue>, y: impl Into<ScalarValue>) -> Self {
		Anchor {
			x: x.into(),
			y: y.into(),
			name: None,
			contourpoint: None,
			x_device_table: None,
			y_device_table: None,
			location: None,
		}
	}

	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		if let Some(name) = &self.name {
			return format!("<anchor {name}>");
		}
		let mut res = format!("<anchor {} {}", self.x, self.y);
		if let Some(cp) = self.contourpoint.filter(|&c| c != 0) {
			res += &format!(" contourpoint {cp}");
		}
		if device_truthy(self.x_device_table.as_ref())
			|| device_truthy(self.y_device_table.as_ref())
		{
			res += " ";
			res += &device_to_string(self.x_device_table.as_ref());
			res += " ";
			res += &device_to_string(self.y_device_table.as_ref());
		}
		res += ">";
		res
	}
}

/// `asFea()` of an anchor that may be `None`, which Python cannot write.
pub(crate) fn anchor_as_fea(anchor: Option<&Anchor>) -> Result<String> {
	anchor
		.map(Anchor::as_fea)
		.ok_or_else(|| Error::Value("'NoneType' object has no attribute 'asFea'".to_owned()))
}

/// A value record (`ValueRecord`).
#[derive(Debug, Clone, Default)]
pub struct ValueRecord {
	/// `xPlacement`.
	pub x_placement: Option<ScalarValue>,
	/// `yPlacement`.
	pub y_placement: Option<ScalarValue>,
	/// `xAdvance`.
	pub x_advance: Option<ScalarValue>,
	/// `yAdvance`.
	pub y_advance: Option<ScalarValue>,
	/// `xPlaDevice`.
	pub x_pla_device: Option<Device>,
	/// `yPlaDevice`.
	pub y_pla_device: Option<Device>,
	/// `xAdvDevice`.
	pub x_adv_device: Option<Device>,
	/// `yAdvDevice`.
	pub y_adv_device: Option<Device>,
	/// Whether a single number means a vertical advance.
	pub vertical: bool,
	/// `location`.
	pub location: Location,
}

impl PartialEq for ValueRecord {
	/// `ValueRecord.__eq__`, which (like fontTools) does not compare
	/// `yPlaDevice`, `yAdvDevice` and `vertical`.
	// LENIENT(L33): a `VariableScalar` compares by value here; Python compares it by
	// identity.
	fn eq(&self, other: &Self) -> bool {
		self.x_placement == other.x_placement
			&& self.y_placement == other.y_placement
			&& self.x_advance == other.x_advance
			&& self.y_advance == other.y_advance
			&& self.x_pla_device == other.x_pla_device
			&& self.x_adv_device == other.x_adv_device
	}
}

fn opt_str(v: Option<&ScalarValue>) -> String {
	match v {
		Some(v) => v.to_string(),
		None => "None".to_owned(),
	}
}

/// `v or 0`, formatted.
fn or_zero(v: Option<&ScalarValue>) -> String {
	match v {
		Some(v) if v.is_truthy() => v.to_string(),
		_ => "0".to_owned(),
	}
}

impl ValueRecord {
	/// Python truthiness (`__bool__`): whether any field is set.
	pub fn is_truthy(&self) -> bool {
		self.x_placement.is_some()
			|| self.y_placement.is_some()
			|| self.x_advance.is_some()
			|| self.y_advance.is_some()
			|| self.x_pla_device.is_some()
			|| self.y_pla_device.is_some()
			|| self.x_adv_device.is_some()
			|| self.y_adv_device.is_some()
	}

	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		if !self.is_truthy() {
			return "<NULL>".to_owned();
		}
		// Try format A, if possible.
		if self.x_placement.is_none() && self.y_placement.is_none() {
			if self.x_advance.is_none() && self.vertical {
				return opt_str(self.y_advance.as_ref());
			} else if self.y_advance.is_none() && !self.vertical {
				return opt_str(self.x_advance.as_ref());
			}
		}
		// Make any remaining None value 0 to avoid generating invalid records.
		let x = or_zero(self.x_placement.as_ref());
		let y = or_zero(self.y_placement.as_ref());
		let x_advance = or_zero(self.x_advance.as_ref());
		let y_advance = or_zero(self.y_advance.as_ref());
		// Try format B, if possible.
		if self.x_pla_device.is_none()
			&& self.y_pla_device.is_none()
			&& self.x_adv_device.is_none()
			&& self.y_adv_device.is_none()
		{
			return format!("<{x} {y} {x_advance} {y_advance}>");
		}
		// Last resort is format C.
		format!(
			"<{x} {y} {x_advance} {y_advance} {} {} {} {}>",
			device_to_string(self.x_pla_device.as_ref()),
			device_to_string(self.y_pla_device.as_ref()),
			device_to_string(self.x_adv_device.as_ref()),
			device_to_string(self.y_adv_device.as_ref()),
		)
	}
}
