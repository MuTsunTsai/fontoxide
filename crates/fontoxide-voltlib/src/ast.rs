//! The abstract syntax tree of VOLT projects (`fontTools.voltLib.ast`).
//!
//! `str()` of each Python class is the [`Display`](fmt::Display) of the Rust
//! type: it writes the element back as VOLT source.
//!
//! Group names and glyph ranges are resolved against the glyphs and groups of
//! the parsed project (`parser_` in Python), which the [`VoltFile`] keeps.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;
use std::rc::{Rc, Weak};

use indexmap::{IndexMap, IndexSet};

use crate::error::{Error, Location, Result};
use crate::parser::SymbolTable;

/// The glyphs and groups of a parsed project, which group names and ranges
/// refer to (`Parser.glyphs_` and `Parser.groups_`).
#[derive(Debug, Default)]
pub(crate) struct Tables {
	/// The glyph names in definition order (`OrderedSymbolTable`).
	pub(crate) glyphs: RefCell<IndexSet<String>>,
	/// The groups (`SymbolTable`).
	pub(crate) groups: RefCell<SymbolTable<Rc<GroupDefinition>>>,
}

impl Tables {
	/// `resolve_group(group_name)`.
	fn resolve_group(&self, name: &str) -> Option<Rc<GroupDefinition>> {
		self.groups.borrow().resolve(name, true).cloned()
	}

	/// `glyph_range(start, end)` (`OrderedSymbolTable.range`).
	fn glyph_range(&self, start: &str, end: &str) -> Option<Vec<String>> {
		let glyphs = self.glyphs.borrow();
		let start_idx = glyphs.get_index_of(start)?;
		let end_idx = glyphs.get_index_of(end)?;
		if start_idx > end_idx {
			return Some(Vec::new());
		}
		Some(glyphs[start_idx..=end_idx].iter().cloned().collect())
	}
}

fn tables(tables: &Weak<Tables>) -> Result<Rc<Tables>> {
	tables
		.upgrade()
		.ok_or_else(|| Error::Python("the parsed project is no longer available".to_owned()))
}

/// A position or adjustment (`Pos`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pos {
	/// `adv`.
	pub adv: Option<i64>,
	/// `dx`.
	pub dx: Option<i64>,
	/// `dy`.
	pub dy: Option<i64>,
	/// `adv_adjust_by`: adjustment by ppem size.
	pub adv_adjust_by: IndexMap<i64, i64>,
	/// `dx_adjust_by`: adjustment by ppem size.
	pub dx_adjust_by: IndexMap<i64, i64>,
	/// `dy_adjust_by`: adjustment by ppem size.
	pub dy_adjust_by: IndexMap<i64, i64>,
}

impl fmt::Display for Pos {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(" POS")?;
		for (attr, value, adjust_by) in [
			("ADV", self.adv, &self.adv_adjust_by),
			("DX", self.dx, &self.dx_adjust_by),
			("DY", self.dy, &self.dy_adjust_by),
		] {
			if let Some(value) = value {
				write!(f, " {attr} {value}")?;
				for (size, adjustment) in adjust_by {
					write!(f, " ADJUST_BY {adjustment} AT {size}")?;
				}
			}
		}
		f.write_str(" END_POS")
	}
}

/// A parsed VOLT project (`VoltFile`).
#[derive(Debug, Clone)]
pub struct VoltFile {
	/// The statements.
	pub statements: Vec<Statement>,
	/// Keeps the glyphs and groups that the group names and ranges refer to.
	#[allow(dead_code)]
	pub(crate) tables: Rc<Tables>,
}

impl fmt::Display for VoltFile {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("\n")?;
		for (i, s) in self.statements.iter().enumerate() {
			if i > 0 {
				f.write_str("\n")?;
			}
			write!(f, "{s}")?;
		}
		f.write_str(" END\n")
	}
}

/// A top-level statement of a project.
#[derive(Debug, Clone)]
pub enum Statement {
	/// `GlyphDefinition`.
	GlyphDefinition(GlyphDefinition),
	/// `GroupDefinition`.
	GroupDefinition(Rc<GroupDefinition>),
	/// `ScriptDefinition`.
	ScriptDefinition(ScriptDefinition),
	/// `LookupDefinition`.
	LookupDefinition(LookupDefinition),
	/// `AnchorDefinition`.
	AnchorDefinition(AnchorDefinition),
	/// `SettingDefinition`.
	SettingDefinition(SettingDefinition),
}

impl Statement {
	/// `location`.
	pub fn location(&self) -> &Location {
		match self {
			Statement::GlyphDefinition(s) => &s.location,
			Statement::GroupDefinition(s) => &s.location,
			Statement::ScriptDefinition(s) => &s.location,
			Statement::LookupDefinition(s) => &s.location,
			Statement::AnchorDefinition(s) => &s.location,
			Statement::SettingDefinition(s) => &s.location,
		}
	}
}

impl fmt::Display for Statement {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Statement::GlyphDefinition(s) => s.fmt(f),
			Statement::GroupDefinition(s) => s.fmt(f),
			Statement::ScriptDefinition(s) => s.fmt(f),
			Statement::LookupDefinition(s) => s.fmt(f),
			Statement::AnchorDefinition(s) => s.fmt(f),
			Statement::SettingDefinition(s) => s.fmt(f),
		}
	}
}

/// `f"{u:04X}"` of a Python integer.
fn hex4(u: i64) -> String {
	if u < 0 {
		format!("-{:03X}", u.unsigned_abs())
	} else {
		format!("{u:04X}")
	}
}

/// `DEF_GLYPH` (`GlyphDefinition`).
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphDefinition {
	/// `name`.
	pub name: String,
	/// The glyph ID (`id`).
	pub id: i64,
	/// The code points (`unicode`).
	pub unicode: Option<Vec<i64>>,
	/// `BASE`, `LIGATURE`, `MARK` or `COMPONENT` (`type`).
	pub glyph_type: Option<String>,
	/// The number of ligature components (`components`).
	pub components: Option<i64>,
	/// `location`.
	pub location: Location,
}

impl fmt::Display for GlyphDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "DEF_GLYPH \"{}\" ID {}", self.name, self.id)?;
		if let Some(unicode) = &self.unicode {
			if unicode.len() > 1 {
				let unicodes: Vec<String> =
					unicode.iter().map(|u| format!("U+{}", hex4(*u))).collect();
				write!(f, " UNICODEVALUES \"{}\"", unicodes.join(","))?;
			} else if let Some(u) = unicode.first() {
				write!(f, " UNICODE {u}")?;
			}
		}
		if let Some(t) = &self.glyph_type {
			write!(f, " TYPE {t}")?;
		}
		if let Some(c) = self.components {
			write!(f, " COMPONENTS {c}")?;
		}
		f.write_str(" END_GLYPH")
	}
}

/// `DEF_GROUP` (`GroupDefinition`).
#[derive(Debug)]
pub struct GroupDefinition {
	/// `name`.
	pub name: String,
	/// The glyphs (`enum`).
	pub enum_: Option<Enum>,
	glyphs_: RefCell<Option<Vec<String>>>,
	/// `location`.
	pub location: Location,
}

impl GroupDefinition {
	pub(crate) fn new(name: String, enum_: Option<Enum>, location: Location) -> Self {
		GroupDefinition {
			name,
			enum_,
			glyphs_: RefCell::new(None),
			location,
		}
	}

	/// The glyph names of the group, with the groups it contains expanded
	/// (`glyphSet()`).
	pub fn glyph_set(&self) -> Result<Vec<String>> {
		self.glyph_set_(None)
	}

	/// `glyphSet(groups)`: `groups` are the groups being expanded, shared by all
	/// the groups below the outermost one.
	// UPSTREAM-BUG(B56): `groups` is one set for the whole expansion and is never
	// unwound, so a group referenced twice below another group (directly or
	// through two different groups) is reported as containing itself.
	fn glyph_set_(&self, groups: Option<&mut HashSet<String>>) -> Result<Vec<String>> {
		if let Some(groups) = &groups
			&& groups.contains(&self.name)
		{
			return Err(Error::volt_lib(
				format!("Group \"{}\" contains itself.", self.name),
				self.location.clone(),
			));
		}
		if let Some(glyphs) = &*self.glyphs_.borrow() {
			return Ok(glyphs.clone());
		}
		let mut own = HashSet::new();
		let groups = match groups {
			None => {
				own.insert(self.name.clone());
				&mut own
			}
			Some(groups) => {
				groups.insert(self.name.clone());
				groups
			}
		};
		let glyphs = match &self.enum_ {
			Some(e) => e.glyph_set_(Some(groups))?,
			None => {
				return Err(Error::Python(
					"AttributeError: 'NoneType' object has no attribute 'glyphSet'".to_owned(),
				));
			}
		};
		*self.glyphs_.borrow_mut() = Some(glyphs.clone());
		Ok(glyphs)
	}
}

impl Clone for GroupDefinition {
	fn clone(&self) -> Self {
		GroupDefinition {
			name: self.name.clone(),
			enum_: self.enum_.clone(),
			glyphs_: RefCell::new(self.glyphs_.borrow().clone()),
			location: self.location.clone(),
		}
	}
}

impl fmt::Display for GroupDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let enum_ = self.enum_.as_ref().map(Enum::to_string).unwrap_or_default();
		write!(f, "DEF_GROUP \"{}\"\n{enum_}\nEND_GROUP", self.name)
	}
}

/// An item of a glyph coverage: a glyph, group, range or enum.
#[derive(Debug, Clone)]
pub enum CoverageItem {
	/// `GlyphName`.
	GlyphName(GlyphName),
	/// `GroupName`.
	GroupName(GroupName),
	/// `Range`.
	Range(Range),
	/// `Enum`.
	Enum(Enum),
}

/// A glyph coverage: the items as written (a tuple in Python).
pub type Coverage = Vec<CoverageItem>;

impl CoverageItem {
	/// The glyph names (`glyphSet()`).
	pub fn glyph_set(&self) -> Result<Vec<String>> {
		match self {
			CoverageItem::GlyphName(g) => Ok(g.glyph_set()),
			CoverageItem::GroupName(g) => g.glyph_set(),
			CoverageItem::Range(r) => r.glyph_set(),
			CoverageItem::Enum(e) => e.glyph_set(),
		}
	}

	/// `location`.
	pub fn location(&self) -> &Location {
		match self {
			CoverageItem::GlyphName(g) => &g.location,
			CoverageItem::GroupName(g) => &g.location,
			CoverageItem::Range(r) => &r.location,
			CoverageItem::Enum(e) => &e.location,
		}
	}
}

impl fmt::Display for CoverageItem {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			CoverageItem::GlyphName(g) => g.fmt(f),
			CoverageItem::GroupName(g) => g.fmt(f),
			CoverageItem::Range(r) => r.fmt(f),
			CoverageItem::Enum(e) => e.fmt(f),
		}
	}
}

/// `"".join(str(c) for c in coverage)`.
pub(crate) fn coverage_str(coverage: &[CoverageItem], sep: &str) -> String {
	coverage
		.iter()
		.map(CoverageItem::to_string)
		.collect::<Vec<_>>()
		.join(sep)
}

/// A single glyph name, such as `cedilla` (`GlyphName`).
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphName {
	/// `glyph`.
	pub glyph: String,
	/// `location`.
	pub location: Location,
}

impl GlyphName {
	/// `glyphSet()`.
	pub fn glyph_set(&self) -> Vec<String> {
		vec![self.glyph.clone()]
	}
}

impl fmt::Display for GlyphName {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, " GLYPH \"{}\"", self.glyph)
	}
}

/// An enum (`Enum`).
#[derive(Debug, Clone)]
pub struct Enum {
	/// The items (`enum`).
	pub enum_: Coverage,
	/// `location`.
	pub location: Location,
}

impl Enum {
	/// The glyph names (`glyphSet()`).
	pub fn glyph_set(&self) -> Result<Vec<String>> {
		self.glyph_set_(None)
	}

	fn glyph_set_(&self, mut groups: Option<&mut HashSet<String>>) -> Result<Vec<String>> {
		let mut glyphs = Vec::new();
		for element in &self.enum_ {
			match element {
				CoverageItem::GroupName(g) => glyphs.extend(g.glyph_set_(groups.as_deref_mut())?),
				CoverageItem::Enum(e) => glyphs.extend(e.glyph_set_(groups.as_deref_mut())?),
				CoverageItem::GlyphName(g) => glyphs.extend(g.glyph_set()),
				CoverageItem::Range(r) => glyphs.extend(r.glyph_set()?),
			}
		}
		Ok(glyphs)
	}
}

impl fmt::Display for Enum {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, " ENUM{} END_ENUM", coverage_str(&self.enum_, ""))
	}
}

/// A glyph group reference (`GroupName`).
#[derive(Debug, Clone)]
pub struct GroupName {
	/// The group name (`group`).
	pub group: String,
	pub(crate) tables: Weak<Tables>,
	/// `location`.
	pub location: Location,
}

impl GroupName {
	/// The glyph names of the group (`glyphSet()`).
	pub fn glyph_set(&self) -> Result<Vec<String>> {
		self.glyph_set_(None)
	}

	fn glyph_set_(&self, groups: Option<&mut HashSet<String>>) -> Result<Vec<String>> {
		match tables(&self.tables)?.resolve_group(&self.group) {
			Some(group) => group.glyph_set_(groups),
			None => Err(Error::volt_lib(
				format!("Group \"{}\" is used but undefined.", self.group),
				self.location.clone(),
			)),
		}
	}
}

impl fmt::Display for GroupName {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, " GROUP \"{}\"", self.group)
	}
}

/// A glyph range (`Range`).
#[derive(Debug, Clone)]
pub struct Range {
	/// `start`.
	pub start: String,
	/// `end`.
	pub end: String,
	pub(crate) tables: Weak<Tables>,
	/// `location`.
	pub location: Location,
}

impl Range {
	/// The glyphs from `start` to `end` in definition order (`glyphSet()`).
	pub fn glyph_set(&self) -> Result<Vec<String>> {
		tables(&self.tables)?
			.glyph_range(&self.start, &self.end)
			.ok_or_else(|| Error::Python("TypeError: 'NoneType' object is not iterable".to_owned()))
	}
}

impl fmt::Display for Range {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, " RANGE \"{}\" TO \"{}\"", self.start, self.end)
	}
}

/// `DEF_SCRIPT` (`ScriptDefinition`).
#[derive(Debug, Clone)]
pub struct ScriptDefinition {
	/// `name`.
	pub name: Option<String>,
	/// `tag`.
	pub tag: String,
	/// `langs`.
	pub langs: Vec<LangSysDefinition>,
	/// `location`.
	pub location: Location,
}

impl fmt::Display for ScriptDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("DEF_SCRIPT")?;
		if let Some(name) = &self.name {
			write!(f, " NAME \"{name}\"")?;
		}
		write!(f, " TAG \"{}\"\n\n", self.tag)?;
		for lang in &self.langs {
			write!(f, "{lang}")?;
		}
		f.write_str("END_SCRIPT")
	}
}

/// `DEF_LANGSYS` (`LangSysDefinition`).
#[derive(Debug, Clone)]
pub struct LangSysDefinition {
	/// `name`.
	pub name: Option<String>,
	/// `tag`.
	pub tag: String,
	/// `features`.
	pub features: Vec<FeatureDefinition>,
	/// `location`.
	pub location: Location,
}

impl fmt::Display for LangSysDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("DEF_LANGSYS")?;
		if let Some(name) = &self.name {
			write!(f, " NAME \"{name}\"")?;
		}
		write!(f, " TAG \"{}\"\n\n", self.tag)?;
		for feature in &self.features {
			write!(f, "{feature}")?;
		}
		f.write_str("END_LANGSYS\n")
	}
}

/// `DEF_FEATURE` (`FeatureDefinition`).
#[derive(Debug, Clone)]
pub struct FeatureDefinition {
	/// `name`.
	pub name: String,
	/// `tag`.
	pub tag: String,
	/// The lookup names (`lookups`).
	pub lookups: Vec<String>,
	/// `location`.
	pub location: Location,
}

impl fmt::Display for FeatureDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		writeln!(f, "DEF_FEATURE NAME \"{}\" TAG \"{}\"", self.name, self.tag)?;
		let lookups: Vec<String> = self
			.lookups
			.iter()
			.map(|l| format!("LOOKUP \"{l}\""))
			.collect();
		writeln!(f, " {}", lookups.join(" "))?;
		f.write_str("END_FEATURE\n")
	}
}

/// `PROCESS_MARKS` of a lookup (`process_marks`): `True`, `False` or a group
/// name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessMarks {
	/// `ALL` (`True`) or `NONE`/`SKIP_MARKS` (`False`).
	Bool(bool),
	/// A mark attachment group.
	Group(String),
}

impl ProcessMarks {
	/// Python truthiness: an empty group name is false.
	pub fn is_truthy(&self) -> bool {
		match self {
			ProcessMarks::Bool(b) => *b,
			ProcessMarks::Group(g) => !g.is_empty(),
		}
	}
}

/// `DEF_LOOKUP` (`LookupDefinition`).
#[derive(Debug, Clone)]
pub struct LookupDefinition {
	/// `name`.
	pub name: String,
	/// `process_base`.
	pub process_base: bool,
	/// `process_marks`.
	pub process_marks: ProcessMarks,
	/// `mark_glyph_set`.
	pub mark_glyph_set: Option<String>,
	/// `LTR` or `RTL` (`direction`).
	pub direction: Option<String>,
	/// `reversal`.
	pub reversal: bool,
	/// `comments`.
	pub comments: Option<String>,
	/// `context`.
	pub context: Vec<ContextDefinition>,
	/// `sub`.
	pub sub: Option<SubstitutionDefinition>,
	/// `pos`.
	pub pos: Option<PositionDefinition>,
	/// `location`.
	pub location: Location,
}

impl fmt::Display for LookupDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "DEF_LOOKUP \"{}\"", self.name)?;
		f.write_str(if self.process_base {
			" PROCESS_BASE"
		} else {
			" SKIP_BASE"
		})?;
		if self.process_marks.is_truthy() {
			f.write_str(" PROCESS_MARKS ")?;
			if let Some(set) = self.mark_glyph_set.as_ref().filter(|s| !s.is_empty()) {
				write!(f, "MARK_GLYPH_SET \"{set}\"")?;
			} else if let ProcessMarks::Group(g) = &self.process_marks {
				write!(f, "\"{g}\"")?;
			} else {
				f.write_str("ALL")?;
			}
		} else {
			f.write_str(" SKIP_MARKS")?;
		}
		if let Some(d) = &self.direction {
			write!(f, " DIRECTION {d}")?;
		}
		if self.reversal {
			f.write_str(" REVERSAL")?;
		}
		if let Some(c) = &self.comments {
			write!(f, "\nCOMMENTS \"{}\"", c.replace('\n', "\\n"))?;
		}
		if self.context.is_empty() {
			f.write_str("\nIN_CONTEXT\nEND_CONTEXT")?;
		} else {
			for c in &self.context {
				write!(f, "\n{c}")?;
			}
		}
		if let Some(sub) = &self.sub {
			write!(f, "\n{sub}")?;
		}
		if let Some(pos) = &self.pos {
			write!(f, "\n{pos}")?;
		}
		Ok(())
	}
}

/// The kind of a substitution (the subclasses of `SubstitutionDefinition`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubstitutionKind {
	/// `SubstitutionSingleDefinition`.
	Single,
	/// `SubstitutionMultipleDefinition`.
	Multiple,
	/// `SubstitutionLigatureDefinition`.
	Ligature,
	/// `SubstitutionAlternateDefinition`.
	Alternate,
	/// `SubstitutionReverseChainingSingleDefinition`.
	ReverseChainingSingle,
}

/// `AS_SUBSTITUTION` (`SubstitutionDefinition` and its subclasses).
#[derive(Debug, Clone)]
pub struct SubstitutionDefinition {
	/// The kind of substitution (the Python class).
	pub kind: SubstitutionKind,
	/// The `SUB ... WITH ...` pairs (`mapping`, a dict keyed by coverage tuples:
	/// only empty coverages are equal keys).
	pub mapping: Vec<(Coverage, Coverage)>,
	/// `location`.
	pub location: Location,
}

impl fmt::Display for SubstitutionDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("AS_SUBSTITUTION\n")?;
		for (src, dst) in &self.mapping {
			write!(
				f,
				"SUB{}\nWITH{}\nEND_SUB\n",
				coverage_str(src, ""),
				coverage_str(dst, "")
			)?;
		}
		f.write_str("END_SUBSTITUTION")
	}
}

/// `AS_POSITION`: one of the positioning definitions.
#[derive(Debug, Clone)]
pub enum PositionDefinition {
	/// `PositionAttachDefinition`.
	Attach(PositionAttachDefinition),
	/// `PositionAttachCursiveDefinition`.
	AttachCursive(PositionAttachCursiveDefinition),
	/// `PositionAdjustPairDefinition`.
	AdjustPair(PositionAdjustPairDefinition),
	/// `PositionAdjustSingleDefinition`.
	AdjustSingle(PositionAdjustSingleDefinition),
}

impl PositionDefinition {
	/// `location`.
	pub fn location(&self) -> &Location {
		match self {
			PositionDefinition::Attach(p) => &p.location,
			PositionDefinition::AttachCursive(p) => &p.location,
			PositionDefinition::AdjustPair(p) => &p.location,
			PositionDefinition::AdjustSingle(p) => &p.location,
		}
	}
}

impl fmt::Display for PositionDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			PositionDefinition::Attach(p) => p.fmt(f),
			PositionDefinition::AttachCursive(p) => p.fmt(f),
			PositionDefinition::AdjustPair(p) => p.fmt(f),
			PositionDefinition::AdjustSingle(p) => p.fmt(f),
		}
	}
}

/// `ATTACH` (`PositionAttachDefinition`).
#[derive(Debug, Clone)]
pub struct PositionAttachDefinition {
	/// The base glyphs (`coverage`).
	pub coverage: Coverage,
	/// The marks and their anchor names (`coverage_to`).
	pub coverage_to: Vec<(Coverage, String)>,
	/// `location`.
	pub location: Location,
}

impl fmt::Display for PositionAttachDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"AS_POSITION\nATTACH{}\nTO",
			coverage_str(&self.coverage, "")
		)?;
		for (coverage, anchor) in &self.coverage_to {
			write!(f, "{} AT ANCHOR \"{anchor}\"", coverage_str(coverage, ""))?;
		}
		f.write_str("\nEND_ATTACH\nEND_POSITION")
	}
}

/// `ATTACH_CURSIVE` (`PositionAttachCursiveDefinition`).
#[derive(Debug, Clone)]
pub struct PositionAttachCursiveDefinition {
	/// `coverages_exit`.
	pub coverages_exit: Vec<Coverage>,
	/// `coverages_enter`.
	pub coverages_enter: Vec<Coverage>,
	/// `location`.
	pub location: Location,
}

impl fmt::Display for PositionAttachCursiveDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("AS_POSITION\nATTACH_CURSIVE")?;
		for coverage in &self.coverages_exit {
			write!(f, "\nEXIT {}", coverage_str(coverage, ""))?;
		}
		for coverage in &self.coverages_enter {
			write!(f, "\nENTER {}", coverage_str(coverage, ""))?;
		}
		f.write_str("\nEND_ATTACH\nEND_POSITION")
	}
}

/// `ADJUST_PAIR` (`PositionAdjustPairDefinition`).
#[derive(Debug, Clone)]
pub struct PositionAdjustPairDefinition {
	/// `coverages_1`.
	pub coverages_1: Vec<Coverage>,
	/// `coverages_2`.
	pub coverages_2: Vec<Coverage>,
	/// The adjustments by 1-based coverage indices (`adjust_pair`).
	pub adjust_pair: IndexMap<(i64, i64), (Pos, Pos)>,
	/// `location`.
	pub location: Location,
}

impl fmt::Display for PositionAdjustPairDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("AS_POSITION\nADJUST_PAIR\n")?;
		for coverage in &self.coverages_1 {
			write!(f, " FIRST {}", coverage_str(coverage, " "))?;
		}
		f.write_str("\n")?;
		for coverage in &self.coverages_2 {
			write!(f, " SECOND {}", coverage_str(coverage, " "))?;
		}
		f.write_str("\n")?;
		for ((id_1, id_2), (pos_1, pos_2)) in &self.adjust_pair {
			writeln!(f, " {id_1} {id_2} BY{pos_1}{pos_2}")?;
		}
		f.write_str("\nEND_ADJUST\nEND_POSITION")
	}
}

/// `ADJUST_SINGLE` (`PositionAdjustSingleDefinition`).
#[derive(Debug, Clone)]
pub struct PositionAdjustSingleDefinition {
	/// `adjust_single`.
	pub adjust_single: Vec<(Coverage, Pos)>,
	/// `location`.
	pub location: Location,
}

impl fmt::Display for PositionAdjustSingleDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("AS_POSITION\nADJUST_SINGLE")?;
		for (coverage, pos) in &self.adjust_single {
			write!(f, "{} BY{pos}", coverage_str(coverage, ""))?;
		}
		f.write_str("\nEND_ADJUST\nEND_POSITION")
	}
}

/// `IN_CONTEXT` or `EXCEPT_CONTEXT` (`ContextDefinition`).
#[derive(Debug, Clone)]
pub struct ContextDefinition {
	/// `IN_CONTEXT` or `EXCEPT_CONTEXT` (`ex_or_in`).
	pub ex_or_in: String,
	/// `left`.
	pub left: Vec<Coverage>,
	/// `right`.
	pub right: Vec<Coverage>,
	/// `location`.
	pub location: Location,
}

impl fmt::Display for ContextDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		writeln!(f, "{}", self.ex_or_in)?;
		for coverage in &self.left {
			writeln!(f, " LEFT{}", coverage_str(coverage, ""))?;
		}
		for coverage in &self.right {
			writeln!(f, " RIGHT{}", coverage_str(coverage, ""))?;
		}
		f.write_str("END_CONTEXT")
	}
}

/// `DEF_ANCHOR` (`AnchorDefinition`).
#[derive(Debug, Clone, PartialEq)]
pub struct AnchorDefinition {
	/// `name`.
	pub name: String,
	/// `gid`.
	pub gid: i64,
	/// `glyph_name`.
	pub glyph_name: String,
	/// `component`.
	pub component: i64,
	/// `locked`.
	pub locked: bool,
	/// `pos`.
	pub pos: Pos,
	/// `location`.
	pub location: Location,
}

impl fmt::Display for AnchorDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let locked = if self.locked { " LOCKED" } else { "" };
		write!(
			f,
			"DEF_ANCHOR \"{}\" ON {} GLYPH {} COMPONENT {}{locked} AT {} END_ANCHOR",
			self.name, self.gid, self.glyph_name, self.component, self.pos
		)
	}
}

/// The value of a setting (`value`): `True`, a number or a tuple of numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingValue {
	/// A flag without argument (`True`).
	True,
	/// A number.
	Number(i64),
	/// A tuple of numbers.
	Numbers(Vec<i64>),
}

/// A setting such as `GRID_PPEM` or `COMPILER_USEEXTENSIONLOOKUPS`
/// (`SettingDefinition`).
#[derive(Debug, Clone, PartialEq)]
pub struct SettingDefinition {
	/// `name`.
	pub name: String,
	/// `value`.
	pub value: SettingValue,
	/// `location`.
	pub location: Location,
}

impl fmt::Display for SettingDefinition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match &self.value {
			SettingValue::True => f.write_str(&self.name),
			SettingValue::Number(n) => write!(f, "{} {n}", self.name),
			SettingValue::Numbers(v) => {
				let v: Vec<String> = v.iter().map(i64::to_string).collect();
				write!(f, "{} {}", self.name, v.join(" "))
			}
		}
	}
}
