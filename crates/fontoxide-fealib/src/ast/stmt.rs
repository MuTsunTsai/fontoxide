//! Statements and blocks.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use fontoxide_encodings::codecs;
use fontoxide_misc::encoding_tools::get_encoding;
use indexmap::{IndexMap, IndexSet};

use super::expr::anchor_as_fea;
use super::{
	Anchor, GlyphContainer, GlyphSet, IntOrFloat, MarkClass, MarkClassRef, NullGlyph, SHIFT,
	ScalarValue, ValueRecord, glyph_as_fea,
};
use crate::error::{Error, Result};
use crate::location::Location;
use crate::py_set::py_int_set_order;

fn join_fea<'a>(items: impl IntoIterator<Item = &'a GlyphContainer>, sep: &str) -> String {
	items
		.into_iter()
		.map(GlyphContainer::as_fea)
		.collect::<Vec<_>>()
		.join(sep)
}

fn join_marked<'a>(items: impl IntoIterator<Item = &'a GlyphContainer>) -> String {
	items
		.into_iter()
		.map(|g| format!("{}'", g.as_fea()))
		.collect::<Vec<_>>()
		.join(" ")
}

/// `Block.asFea(indent)`: the statements, one per line, indented one more step.
pub fn block_as_fea(statements: &[Statement], indent: &str) -> Result<String> {
	let indent = format!("{indent}{SHIFT}");
	let parts: Result<Vec<String>> = statements.iter().map(|s| s.as_fea(&indent)).collect();
	Ok(format!("{indent}{}\n", parts?.join(&format!("\n{indent}"))))
}

/// A statement of a feature file, block or table.
///
/// The variants are not boxed so that statements can be matched and built
/// directly; a feature file has few enough statements for the size not to
/// matter.
#[derive(Debug, Clone)]
#[allow(clippy::upper_case_acronyms, clippy::large_enum_variant)]
pub enum Statement {
	/// `Comment`.
	Comment(Comment),
	/// `AnonymousBlock`.
	AnonymousBlock(AnonymousBlock),
	/// `FeatureBlock`.
	FeatureBlock(FeatureBlock),
	/// `NestedBlock`.
	NestedBlock(NestedBlock),
	/// `LookupBlock`.
	LookupBlock(Rc<LookupBlock>),
	/// `TableBlock`.
	TableBlock(TableBlock),
	/// `VariationBlock`.
	VariationBlock(VariationBlock),
	/// `GlyphClassDefinition`.
	GlyphClassDefinition(Rc<GlyphClassDefinition>),
	/// `GlyphClassDefStatement`.
	GlyphClassDefStatement(GlyphClassDefStatement),
	/// `MarkClassDefinition`.
	MarkClassDefinition(Rc<MarkClassDefinition>),
	/// `AlternateSubstStatement`.
	AlternateSubstStatement(AlternateSubstStatement),
	/// `AnchorDefinition`.
	AnchorDefinition(AnchorDefinition),
	/// `AttachStatement`.
	AttachStatement(AttachStatement),
	/// `ChainContextPosStatement`.
	ChainContextPosStatement(ChainContextPosStatement),
	/// `ChainContextSubstStatement`.
	ChainContextSubstStatement(ChainContextSubstStatement),
	/// `CursivePosStatement`.
	CursivePosStatement(CursivePosStatement),
	/// `FeatureReferenceStatement`.
	FeatureReferenceStatement(FeatureReferenceStatement),
	/// `IgnorePosStatement`.
	IgnorePosStatement(IgnorePosStatement),
	/// `IgnoreSubstStatement`.
	IgnoreSubstStatement(IgnoreSubstStatement),
	/// `IncludeStatement`.
	IncludeStatement(IncludeStatement),
	/// `LanguageStatement`.
	LanguageStatement(LanguageStatement),
	/// `LanguageSystemStatement`.
	LanguageSystemStatement(LanguageSystemStatement),
	/// `FontRevisionStatement`.
	FontRevisionStatement(FontRevisionStatement),
	/// `LigatureCaretByIndexStatement`.
	LigatureCaretByIndexStatement(LigatureCaretByIndexStatement),
	/// `LigatureCaretByPosStatement`.
	LigatureCaretByPosStatement(LigatureCaretByPosStatement),
	/// `LigatureSubstStatement`.
	LigatureSubstStatement(LigatureSubstStatement),
	/// `LookupFlagStatement`.
	LookupFlagStatement(LookupFlagStatement),
	/// `LookupReferenceStatement`.
	LookupReferenceStatement(LookupReferenceStatement),
	/// `MarkBasePosStatement`.
	MarkBasePosStatement(MarkBasePosStatement),
	/// `MarkLigPosStatement`.
	MarkLigPosStatement(MarkLigPosStatement),
	/// `MarkMarkPosStatement`.
	MarkMarkPosStatement(MarkMarkPosStatement),
	/// `MultipleSubstStatement`.
	MultipleSubstStatement(MultipleSubstStatement),
	/// `PairPosStatement`.
	PairPosStatement(PairPosStatement),
	/// `ReverseChainSingleSubstStatement`.
	ReverseChainSingleSubstStatement(ReverseChainSingleSubstStatement),
	/// `SingleSubstStatement`.
	SingleSubstStatement(SingleSubstStatement),
	/// `ScriptStatement`.
	ScriptStatement(ScriptStatement),
	/// `SinglePosStatement`.
	SinglePosStatement(SinglePosStatement),
	/// `SubtableStatement`.
	SubtableStatement(SubtableStatement),
	/// `ValueRecordDefinition`.
	ValueRecordDefinition(ValueRecordDefinition),
	/// `NameRecord`.
	NameRecord(NameRecord),
	/// `FeatureNameStatement`.
	FeatureNameStatement(FeatureNameStatement),
	/// `STATNameStatement`.
	STATNameStatement(STATNameStatement),
	/// `SizeParameters`.
	SizeParameters(SizeParameters),
	/// `CVParametersNameStatement`.
	CVParametersNameStatement(CVParametersNameStatement),
	/// `CharacterStatement`.
	CharacterStatement(CharacterStatement),
	/// `BaseAxis`.
	BaseAxis(BaseAxis),
	/// `OS2Field`.
	OS2Field(OS2Field),
	/// `HheaField`.
	HheaField(HheaField),
	/// `VheaField`.
	VheaField(VheaField),
	/// `STATDesignAxisStatement`.
	STATDesignAxisStatement(STATDesignAxisStatement),
	/// `ElidedFallbackName`.
	ElidedFallbackName(ElidedFallbackName),
	/// `ElidedFallbackNameID`.
	ElidedFallbackNameID(ElidedFallbackNameID),
	/// `STATAxisValueStatement`.
	STATAxisValueStatement(STATAxisValueStatement),
	/// `ConditionsetStatement`.
	ConditionsetStatement(ConditionsetStatement),
}

impl Statement {
	/// `asFea(indent)`.
	pub fn as_fea(&self, indent: &str) -> Result<String> {
		Ok(match self {
			Statement::Comment(s) => s.as_fea(),
			Statement::AnonymousBlock(s) => s.as_fea(),
			Statement::FeatureBlock(s) => s.as_fea(indent)?,
			Statement::NestedBlock(s) => s.as_fea(indent)?,
			Statement::LookupBlock(s) => s.as_fea(indent)?,
			Statement::TableBlock(s) => s.as_fea(indent)?,
			Statement::VariationBlock(s) => s.as_fea(indent)?,
			Statement::GlyphClassDefinition(s) => s.as_fea(),
			Statement::GlyphClassDefStatement(s) => s.as_fea(),
			Statement::MarkClassDefinition(s) => s.as_fea()?,
			Statement::AlternateSubstStatement(s) => s.as_fea(),
			Statement::AnchorDefinition(s) => s.as_fea(),
			Statement::AttachStatement(s) => s.as_fea(),
			Statement::ChainContextPosStatement(s) => s.as_fea()?,
			Statement::ChainContextSubstStatement(s) => s.as_fea()?,
			Statement::CursivePosStatement(s) => s.as_fea(),
			Statement::FeatureReferenceStatement(s) => s.as_fea(),
			Statement::IgnorePosStatement(s) => s.as_fea(),
			Statement::IgnoreSubstStatement(s) => s.as_fea(),
			Statement::IncludeStatement(s) => s.as_fea(indent),
			Statement::LanguageStatement(s) => s.as_fea(),
			Statement::LanguageSystemStatement(s) => s.as_fea(),
			Statement::FontRevisionStatement(s) => s.as_fea(),
			Statement::LigatureCaretByIndexStatement(s) => s.as_fea(),
			Statement::LigatureCaretByPosStatement(s) => s.as_fea(),
			Statement::LigatureSubstStatement(s) => s.as_fea(),
			Statement::LookupFlagStatement(s) => s.as_fea(),
			Statement::LookupReferenceStatement(s) => s.as_fea(),
			Statement::MarkBasePosStatement(s) => s.as_fea(indent)?,
			Statement::MarkLigPosStatement(s) => s.as_fea(indent)?,
			Statement::MarkMarkPosStatement(s) => s.as_fea(indent)?,
			Statement::MultipleSubstStatement(s) => s.as_fea(),
			Statement::PairPosStatement(s) => s.as_fea()?,
			Statement::ReverseChainSingleSubstStatement(s) => s.as_fea(),
			Statement::SingleSubstStatement(s) => s.as_fea(),
			Statement::ScriptStatement(s) => s.as_fea(),
			Statement::SinglePosStatement(s) => s.as_fea(),
			Statement::SubtableStatement(s) => s.as_fea(),
			Statement::ValueRecordDefinition(s) => s.as_fea(),
			Statement::NameRecord(s) => s.as_fea()?,
			Statement::FeatureNameStatement(s) => s.as_fea(),
			Statement::STATNameStatement(s) => s.as_fea(),
			Statement::SizeParameters(s) => s.as_fea(),
			Statement::CVParametersNameStatement(s) => s.as_fea(),
			Statement::CharacterStatement(s) => s.as_fea(),
			Statement::BaseAxis(s) => s.as_fea(indent),
			Statement::OS2Field(s) => s.as_fea()?,
			Statement::HheaField(s) => s.as_fea()?,
			Statement::VheaField(s) => s.as_fea()?,
			Statement::STATDesignAxisStatement(s) => s.as_fea(indent),
			Statement::ElidedFallbackName(s) => s.as_fea(indent),
			Statement::ElidedFallbackNameID(s) => s.as_fea(),
			Statement::STATAxisValueStatement(s) => s.as_fea(),
			Statement::ConditionsetStatement(s) => s.as_fea(indent),
		})
	}

	/// `location`.
	pub fn location(&self) -> &Location {
		match self {
			Statement::Comment(s) => &s.location,
			Statement::AnonymousBlock(s) => &s.location,
			Statement::FeatureBlock(s) => &s.location,
			Statement::NestedBlock(s) => &s.location,
			Statement::LookupBlock(s) => &s.location,
			Statement::TableBlock(s) => &s.location,
			Statement::VariationBlock(s) => &s.location,
			Statement::GlyphClassDefinition(s) => &s.location,
			Statement::GlyphClassDefStatement(s) => &s.location,
			Statement::MarkClassDefinition(s) => &s.location,
			Statement::AlternateSubstStatement(s) => &s.location,
			Statement::AnchorDefinition(s) => &s.location,
			Statement::AttachStatement(s) => &s.location,
			Statement::ChainContextPosStatement(s) => &s.location,
			Statement::ChainContextSubstStatement(s) => &s.location,
			Statement::CursivePosStatement(s) => &s.location,
			Statement::FeatureReferenceStatement(s) => &s.location,
			Statement::IgnorePosStatement(s) => &s.location,
			Statement::IgnoreSubstStatement(s) => &s.location,
			Statement::IncludeStatement(s) => &s.location,
			Statement::LanguageStatement(s) => &s.location,
			Statement::LanguageSystemStatement(s) => &s.location,
			Statement::FontRevisionStatement(s) => &s.location,
			Statement::LigatureCaretByIndexStatement(s) => &s.location,
			Statement::LigatureCaretByPosStatement(s) => &s.location,
			Statement::LigatureSubstStatement(s) => &s.location,
			Statement::LookupFlagStatement(s) => &s.location,
			Statement::LookupReferenceStatement(s) => &s.location,
			Statement::MarkBasePosStatement(s) => &s.location,
			Statement::MarkLigPosStatement(s) => &s.location,
			Statement::MarkMarkPosStatement(s) => &s.location,
			Statement::MultipleSubstStatement(s) => &s.location,
			Statement::PairPosStatement(s) => &s.location,
			Statement::ReverseChainSingleSubstStatement(s) => &s.location,
			Statement::SingleSubstStatement(s) => &s.location,
			Statement::ScriptStatement(s) => &s.location,
			Statement::SinglePosStatement(s) => &s.location,
			Statement::SubtableStatement(s) => &s.location,
			Statement::ValueRecordDefinition(s) => &s.location,
			Statement::NameRecord(s) => &s.location,
			Statement::FeatureNameStatement(s) => &s.location,
			Statement::STATNameStatement(s) => &s.location,
			Statement::SizeParameters(s) => &s.location,
			Statement::CVParametersNameStatement(s) => &s.location,
			Statement::CharacterStatement(s) => &s.location,
			Statement::BaseAxis(s) => &s.location,
			Statement::OS2Field(s) => &s.location,
			Statement::HheaField(s) => &s.location,
			Statement::VheaField(s) => &s.location,
			Statement::STATDesignAxisStatement(s) => &s.location,
			Statement::ElidedFallbackName(s) => &s.location,
			Statement::ElidedFallbackNameID(s) => &s.location,
			Statement::STATAxisValueStatement(s) => &s.location,
			Statement::ConditionsetStatement(s) => &s.location,
		}
	}

	/// The statements of a block statement (`statements`), or `None`.
	pub fn statements(&self) -> Option<&[Statement]> {
		match self {
			Statement::FeatureBlock(b) => Some(&b.statements),
			Statement::NestedBlock(b) => Some(&b.statements),
			Statement::LookupBlock(b) => Some(&b.statements),
			Statement::TableBlock(b) => Some(&b.statements),
			Statement::VariationBlock(b) => Some(&b.statements),
			_ => None,
		}
	}
}

/// The top-level element of the syntax tree, containing the whole feature file
/// in its `statements` (`FeatureFile`).
#[derive(Debug, Clone, Default)]
pub struct FeatureFile {
	/// The statements.
	pub statements: Vec<Statement>,
	/// The mark classes by name (`markClasses`).
	pub mark_classes: IndexMap<String, MarkClassRef>,
}

impl FeatureFile {
	/// `FeatureFile()`.
	pub fn new() -> Self {
		Self::default()
	}

	/// `asFea()`: the statements separated by newlines.
	pub fn as_fea(&self) -> Result<String> {
		let parts: Result<Vec<String>> = self.statements.iter().map(|s| s.as_fea("")).collect();
		Ok(parts?.join("\n"))
	}
}

/// A comment in a feature file (`Comment`).
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
	/// The text of the comment, including `#`.
	pub text: String,
	/// `location`.
	pub location: Location,
}

impl Comment {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		self.text.clone()
	}
}

/// An anonymous data block (`AnonymousBlock`).
#[derive(Debug, Clone, PartialEq)]
pub struct AnonymousBlock {
	/// The block's tag.
	pub tag: String,
	/// The block data.
	pub content: String,
	/// `location`.
	pub location: Location,
}

impl AnonymousBlock {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		format!("anon {} {{\n{}}} {};\n\n", self.tag, self.content, self.tag)
	}
}

/// A named feature block (`FeatureBlock`).
#[derive(Debug, Clone, Default)]
pub struct FeatureBlock {
	/// The feature tag, padded to four characters.
	pub name: String,
	/// `use_extension`.
	pub use_extension: bool,
	/// `statements`.
	pub statements: Vec<Statement>,
	/// `location`.
	pub location: Location,
}

impl FeatureBlock {
	/// `asFea(indent)`.
	pub fn as_fea(&self, indent: &str) -> Result<String> {
		let name = fontoxide_misc::py::strip(&self.name);
		let mut res = format!("{indent}feature {name} ");
		if self.use_extension {
			res += "useExtension ";
		}
		res += "{\n";
		res += &block_as_fea(&self.statements, indent)?;
		res += &format!("{indent}}} {name};\n");
		Ok(res)
	}
}

/// A block inside another block, for example a `cvParameters` block
/// (`NestedBlock`).
#[derive(Debug, Clone, Default)]
pub struct NestedBlock {
	/// The feature tag.
	pub tag: String,
	/// The block keyword, e.g. `featureNames`.
	pub block_name: String,
	/// `statements`.
	pub statements: Vec<Statement>,
	/// `location`.
	pub location: Location,
}

impl NestedBlock {
	/// `asFea(indent)`.
	pub fn as_fea(&self, indent: &str) -> Result<String> {
		let mut res = format!("{indent}{} {{\n", self.block_name);
		res += &block_as_fea(&self.statements, indent)?;
		res += &format!("{indent}}};\n");
		Ok(res)
	}
}

/// A named lookup (`LookupBlock`).
#[derive(Debug, Clone, Default)]
pub struct LookupBlock {
	/// The lookup name.
	pub name: String,
	/// `use_extension`.
	pub use_extension: bool,
	/// `statements`.
	pub statements: Vec<Statement>,
	/// `location`.
	pub location: Location,
}

impl LookupBlock {
	/// `asFea(indent)`.
	pub fn as_fea(&self, indent: &str) -> Result<String> {
		let mut res = format!("lookup {} ", self.name);
		if self.use_extension {
			res += "useExtension ";
		}
		res += "{\n";
		res += &block_as_fea(&self.statements, indent)?;
		res += &format!("{indent}}} {};\n", self.name);
		Ok(res)
	}
}

/// A `table ... { }` block (`TableBlock`).
#[derive(Debug, Clone, Default)]
pub struct TableBlock {
	/// The table tag, padded to four characters.
	pub name: String,
	/// `statements`.
	pub statements: Vec<Statement>,
	/// `location`.
	pub location: Location,
}

impl TableBlock {
	/// `asFea(indent)`.
	pub fn as_fea(&self, indent: &str) -> Result<String> {
		let name = fontoxide_misc::py::strip(&self.name);
		let mut res = format!("table {name} {{\n");
		res += &block_as_fea(&self.statements, indent)?;
		res += &format!("}} {name};\n");
		Ok(res)
	}
}

/// A variation feature block, applicable in a given set of conditions
/// (`VariationBlock`).
#[derive(Debug, Clone, Default)]
pub struct VariationBlock {
	/// The feature tag, padded to four characters.
	pub name: String,
	/// The name of the conditionset, or `NULL`.
	pub conditionset: String,
	/// `use_extension`.
	pub use_extension: bool,
	/// `statements`.
	pub statements: Vec<Statement>,
	/// `location`.
	pub location: Location,
}

impl VariationBlock {
	/// `asFea(indent)`.
	pub fn as_fea(&self, indent: &str) -> Result<String> {
		let name = fontoxide_misc::py::strip(&self.name);
		let mut res = format!("{indent}variation {name} ");
		res += &self.conditionset;
		res += " ";
		if self.use_extension {
			res += "useExtension ";
		}
		res += "{\n";
		res += &block_as_fea(&self.statements, indent)?;
		res += &format!("{indent}}} {name};\n");
		Ok(res)
	}
}

/// `@UPPERCASE = [A-Z];` (`GlyphClassDefinition`).
#[derive(Debug, Clone)]
pub struct GlyphClassDefinition {
	/// The class name, without `@`.
	pub name: String,
	/// The glyphs: a glyph class or class reference.
	pub glyphs: GlyphContainer,
	/// `location`.
	pub location: Location,
}

impl GlyphClassDefinition {
	/// `glyphSet()`.
	pub fn glyph_set(&self) -> GlyphSet<'_> {
		self.glyphs.glyph_set()
	}

	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		format!("@{} = {};", self.name, self.glyphs.as_fea())
	}
}

/// `GlyphClassDef @UPPERCASE, [B], [C], [D];` (`GlyphClassDefStatement`).
#[derive(Debug, Clone)]
pub struct GlyphClassDefStatement {
	/// `baseGlyphs`.
	pub base_glyphs: Option<GlyphContainer>,
	/// `markGlyphs`.
	pub mark_glyphs: Option<GlyphContainer>,
	/// `ligatureGlyphs`.
	pub ligature_glyphs: Option<GlyphContainer>,
	/// `componentGlyphs`.
	pub component_glyphs: Option<GlyphContainer>,
	/// `location`.
	pub location: Location,
}

impl GlyphClassDefStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let f =
			|g: &Option<GlyphContainer>| g.as_ref().map(GlyphContainer::as_fea).unwrap_or_default();
		format!(
			"GlyphClassDef {}, {}, {}, {};",
			f(&self.base_glyphs),
			f(&self.ligature_glyphs),
			f(&self.mark_glyphs),
			f(&self.component_glyphs)
		)
	}
}

/// A single `markClass` statement (`MarkClassDefinition`).
#[derive(Debug, Clone)]
pub struct MarkClassDefinition {
	/// The mark class it belongs to (`markClass`); the class owns its
	/// definitions, so this is weak.
	pub mark_class: Weak<RefCell<MarkClass>>,
	/// `anchor`.
	pub anchor: Anchor,
	/// `glyphs`.
	pub glyphs: GlyphContainer,
	/// `location`.
	pub location: Location,
}

impl MarkClassDefinition {
	/// `MarkClassDefinition(markClass, anchor, glyphs)`.
	pub fn new(
		mark_class: &MarkClassRef,
		anchor: Anchor,
		glyphs: GlyphContainer,
		location: Location,
	) -> Self {
		MarkClassDefinition {
			mark_class: Rc::downgrade(mark_class),
			anchor,
			glyphs,
			location,
		}
	}

	/// `glyphSet()`.
	pub fn glyph_set(&self) -> GlyphSet<'_> {
		self.glyphs.glyph_set()
	}

	/// The name of the mark class (`markClass.name`).
	pub fn mark_class_name(&self) -> Result<String> {
		self.mark_class
			.upgrade()
			.map(|mc| mc.borrow().name.clone())
			.ok_or_else(|| Error::Value("weakly-referenced object no longer exists".to_owned()))
	}

	/// `asFea()`.
	pub fn as_fea(&self) -> Result<String> {
		Ok(format!(
			"markClass {} {} @{};",
			self.glyphs.as_fea(),
			self.anchor.as_fea(),
			self.mark_class_name()?
		))
	}
}

/// A `sub ... from ...` statement (`AlternateSubstStatement`).
#[derive(Debug, Clone)]
pub struct AlternateSubstStatement {
	/// `prefix`.
	pub prefix: Vec<GlyphContainer>,
	/// `glyph`.
	pub glyph: GlyphContainer,
	/// `suffix`.
	pub suffix: Vec<GlyphContainer>,
	/// `replacement`.
	pub replacement: GlyphContainer,
	/// `location`.
	pub location: Location,
}

impl AlternateSubstStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let mut res = "sub ".to_owned();
		if !self.prefix.is_empty() || !self.suffix.is_empty() {
			if !self.prefix.is_empty() {
				res += &join_fea(&self.prefix, " ");
				res += " ";
			}
			res += &self.glyph.as_fea();
			res += "'";
			if !self.suffix.is_empty() {
				res += " ";
				res += &join_fea(&self.suffix, " ");
			}
		} else {
			res += &self.glyph.as_fea();
		}
		res += " from ";
		res += &self.replacement.as_fea();
		res += ";";
		res
	}
}

/// A named anchor definition (`AnchorDefinition`).
#[derive(Debug, Clone, PartialEq)]
pub struct AnchorDefinition {
	/// `name`.
	pub name: String,
	/// `x`.
	pub x: i64,
	/// `y`.
	pub y: i64,
	/// `contourpoint`.
	pub contourpoint: Option<i64>,
	/// `location`.
	pub location: Location,
}

impl AnchorDefinition {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let mut res = format!("anchorDef {} {}", self.x, self.y);
		if let Some(cp) = self.contourpoint.filter(|&c| c != 0) {
			res += &format!(" contourpoint {cp}");
		}
		res += &format!(" {};", self.name);
		res
	}
}

/// A `GDEF` table `Attach` statement (`AttachStatement`).
#[derive(Debug, Clone)]
pub struct AttachStatement {
	/// `glyphs`.
	pub glyphs: GlyphContainer,
	/// The contour points (`contourPoints`), a set kept in insertion order.
	pub contour_points: IndexSet<i64>,
	/// `location`.
	pub location: Location,
}

impl AttachStatement {
	/// `asFea()`. The contour points are written in the order CPython iterates
	/// the `set`.
	pub fn as_fea(&self) -> String {
		let points: Vec<String> = py_int_set_order(self.contour_points.iter().copied())
			.iter()
			.map(i64::to_string)
			.collect();
		format!("Attach {} {};", self.glyphs.as_fea(), points.join(" "))
	}
}

/// The lookups of a chaining rule: for each marked glyph, `None` or the lookups
/// to apply there.
pub type ChainLookups = Vec<Option<Vec<Rc<LookupBlock>>>>;

fn chain_as_fea(
	keyword: &str,
	prefix: &[GlyphContainer],
	glyphs: &[GlyphContainer],
	suffix: &[GlyphContainer],
	lookups: &ChainLookups,
) -> Result<String> {
	let mut res = format!("{keyword} ");
	if !prefix.is_empty() || !suffix.is_empty() || lookups.iter().any(Option::is_some) {
		if !prefix.is_empty() {
			res += &join_fea(prefix, " ");
			res += " ";
		}
		for (i, g) in glyphs.iter().enumerate() {
			res += &g.as_fea();
			res += "'";
			let lookup = lookups
				.get(i)
				.ok_or_else(|| Error::Value("list index out of range".to_owned()))?;
			if let Some(lus) = lookup {
				for lu in lus {
					res += " lookup ";
					res += &lu.name;
				}
			}
			if i + 1 < glyphs.len() {
				res += " ";
			}
		}
		if !suffix.is_empty() {
			res += " ";
			res += &join_fea(suffix, " ");
		}
	} else {
		res += &join_fea(glyphs, " ");
	}
	res += ";";
	Ok(res)
}

/// A chained contextual positioning statement (`ChainContextPosStatement`).
#[derive(Debug, Clone)]
pub struct ChainContextPosStatement {
	/// `prefix`.
	pub prefix: Vec<GlyphContainer>,
	/// `glyphs`.
	pub glyphs: Vec<GlyphContainer>,
	/// `suffix`.
	pub suffix: Vec<GlyphContainer>,
	/// `lookups`: one entry per glyph in `glyphs`.
	pub lookups: ChainLookups,
	/// `location`.
	pub location: Location,
}

impl ChainContextPosStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> Result<String> {
		chain_as_fea(
			"pos",
			&self.prefix,
			&self.glyphs,
			&self.suffix,
			&self.lookups,
		)
	}
}

/// A chained contextual substitution statement (`ChainContextSubstStatement`).
#[derive(Debug, Clone)]
pub struct ChainContextSubstStatement {
	/// `prefix`.
	pub prefix: Vec<GlyphContainer>,
	/// `glyphs`.
	pub glyphs: Vec<GlyphContainer>,
	/// `suffix`.
	pub suffix: Vec<GlyphContainer>,
	/// `lookups`: one entry per glyph in `glyphs`.
	pub lookups: ChainLookups,
	/// `location`.
	pub location: Location,
}

impl ChainContextSubstStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> Result<String> {
		chain_as_fea(
			"sub",
			&self.prefix,
			&self.glyphs,
			&self.suffix,
			&self.lookups,
		)
	}
}

/// A cursive positioning statement (`CursivePosStatement`).
#[derive(Debug, Clone)]
pub struct CursivePosStatement {
	/// `glyphclass`.
	pub glyphclass: GlyphContainer,
	/// `entryAnchor`.
	pub entry_anchor: Option<Anchor>,
	/// `exitAnchor`.
	pub exit_anchor: Option<Anchor>,
	/// `location`.
	pub location: Location,
}

impl CursivePosStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let f = |a: &Option<Anchor>| {
			a.as_ref()
				.map_or_else(|| "<anchor NULL>".to_owned(), Anchor::as_fea)
		};
		format!(
			"pos cursive {} {} {};",
			self.glyphclass.as_fea(),
			f(&self.entry_anchor),
			f(&self.exit_anchor)
		)
	}
}

/// `feature salt;` (`FeatureReferenceStatement`).
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureReferenceStatement {
	/// `featureName`.
	pub feature_name: String,
	/// `location`.
	pub location: Location,
}

impl FeatureReferenceStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		format!("feature {};", self.feature_name)
	}
}

/// A `(prefix, glyphs, suffix)` context of an `ignore` statement.
pub type ChainContext = (
	Vec<GlyphContainer>,
	Vec<GlyphContainer>,
	Vec<GlyphContainer>,
);

/// An `ignore pos` statement (`IgnorePosStatement`).
#[derive(Debug, Clone)]
pub struct IgnorePosStatement {
	/// `chainContexts`.
	pub chain_contexts: Vec<ChainContext>,
	/// `location`.
	pub location: Location,
}

impl IgnorePosStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let contexts: Vec<String> = self
			.chain_contexts
			.iter()
			.map(|(prefix, glyphs, suffix)| {
				let mut res = String::new();
				if !prefix.is_empty() || !suffix.is_empty() {
					if !prefix.is_empty() {
						res += &join_fea(prefix, " ");
						res += " ";
					}
					res += &join_marked(glyphs);
					if !suffix.is_empty() {
						res += " ";
						res += &join_fea(suffix, " ");
					}
				} else {
					res += &join_fea(glyphs, " ");
				}
				res
			})
			.collect();
		format!("ignore pos {};", contexts.join(", "))
	}
}

/// An `ignore sub` statement (`IgnoreSubstStatement`).
#[derive(Debug, Clone)]
pub struct IgnoreSubstStatement {
	/// `chainContexts`.
	pub chain_contexts: Vec<ChainContext>,
	/// `location`.
	pub location: Location,
}

impl IgnoreSubstStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let contexts: Vec<String> = self
			.chain_contexts
			.iter()
			.map(|(prefix, glyphs, suffix)| {
				let mut res = String::new();
				if !prefix.is_empty() {
					res += &join_fea(prefix, " ");
					res += " ";
				}
				res += &join_marked(glyphs);
				if !suffix.is_empty() {
					res += " ";
					res += &join_fea(suffix, " ");
				}
				res
			})
			.collect();
		format!("ignore sub {};", contexts.join(", "))
	}
}

/// An `include()` statement (`IncludeStatement`).
#[derive(Debug, Clone, PartialEq)]
pub struct IncludeStatement {
	/// The file name to include.
	pub filename: String,
	/// `location`.
	pub location: Location,
}

impl IncludeStatement {
	/// `asFea(indent)`.
	pub fn as_fea(&self, indent: &str) -> String {
		format!("{indent}include({});", self.filename)
	}
}

/// A `language` statement within a feature (`LanguageStatement`).
///
/// Several tags, e.g. `language AZE CRT KAZ TAT TRK;`, are a Glyphs.app extension
/// not in the Adobe FEA spec.
#[derive(Debug, Clone, PartialEq)]
pub struct LanguageStatement {
	/// The four-character language tags.
	pub languages: Vec<String>,
	/// If false, `exclude_dflt`.
	pub include_default: bool,
	/// `required`.
	pub required: bool,
	/// `location`.
	pub location: Location,
}

impl LanguageStatement {
	/// `LanguageStatement(languages, include_default, required)`.
	pub fn new(
		languages: Vec<String>,
		include_default: bool,
		required: bool,
		location: Location,
	) -> Result<Self> {
		if languages.is_empty() || !languages.iter().all(|t| t.chars().count() == 4) {
			return Err(Error::Assertion(
				"language tags must be four characters".to_owned(),
			));
		}
		Ok(LanguageStatement {
			languages,
			include_default,
			required,
			location,
		})
	}

	/// The first language tag (`language`); see `languages` for all of them.
	// LENIENT(L33): Python warns when there are several tags; this does not.
	pub fn language(&self) -> &str {
		self.languages.first().map_or("", String::as_str)
	}

	/// Replaces all the tags with the one given (`language = ...`).
	pub fn set_language(&mut self, language: impl Into<String>) {
		self.languages = vec![language.into()];
	}

	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let tags: Vec<&str> = self
			.languages
			.iter()
			.map(|t| fontoxide_misc::py::strip(t))
			.collect();
		let mut res = format!("language {}", tags.join(" "));
		if !self.include_default {
			res += " exclude_dflt";
		}
		if self.required {
			res += " required";
		}
		res += ";";
		res
	}
}

/// A top-level `languagesystem` statement (`LanguageSystemStatement`).
#[derive(Debug, Clone, PartialEq)]
pub struct LanguageSystemStatement {
	/// `script`.
	pub script: String,
	/// `language`.
	pub language: String,
	/// `location`.
	pub location: Location,
}

impl LanguageSystemStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		format!(
			"languagesystem {} {};",
			self.script,
			fontoxide_misc::py::strip(&self.language)
		)
	}
}

/// A `head` table `FontRevision` statement (`FontRevisionStatement`).
#[derive(Debug, Clone, PartialEq)]
pub struct FontRevisionStatement {
	/// `revision`.
	pub revision: f64,
	/// `location`.
	pub location: Location,
}

impl FontRevisionStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		format!("FontRevision {:.3};", self.revision)
	}
}

/// A `GDEF` table `LigatureCaretByIndex` statement
/// (`LigatureCaretByIndexStatement`).
#[derive(Debug, Clone)]
pub struct LigatureCaretByIndexStatement {
	/// `glyphs`.
	pub glyphs: GlyphContainer,
	/// `carets`.
	pub carets: Vec<i64>,
	/// `location`.
	pub location: Location,
}

impl LigatureCaretByIndexStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let carets: Vec<String> = self.carets.iter().map(i64::to_string).collect();
		format!(
			"LigatureCaretByIndex {} {};",
			self.glyphs.as_fea(),
			carets.join(" ")
		)
	}
}

/// A `GDEF` table `LigatureCaretByPos` statement (`LigatureCaretByPosStatement`).
#[derive(Debug, Clone)]
pub struct LigatureCaretByPosStatement {
	/// `glyphs`.
	pub glyphs: GlyphContainer,
	/// `carets`.
	pub carets: Vec<ScalarValue>,
	/// `location`.
	pub location: Location,
}

impl LigatureCaretByPosStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let carets: Vec<String> = self.carets.iter().map(ScalarValue::to_string).collect();
		format!(
			"LigatureCaretByPos {} {};",
			self.glyphs.as_fea(),
			carets.join(" ")
		)
	}
}

/// A ligature substitution statement (`LigatureSubstStatement`).
#[derive(Debug, Clone)]
pub struct LigatureSubstStatement {
	/// `prefix`.
	pub prefix: Vec<GlyphContainer>,
	/// `glyphs`.
	pub glyphs: Vec<GlyphContainer>,
	/// `suffix`.
	pub suffix: Vec<GlyphContainer>,
	/// The ligature glyph name (`replacement`).
	pub replacement: String,
	/// Whether to write it as a chaining rule (`forceChain`).
	pub force_chain: bool,
	/// `location`.
	pub location: Location,
}

impl LigatureSubstStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let mut res = "sub ".to_owned();
		if !self.prefix.is_empty() || !self.suffix.is_empty() || self.force_chain {
			if !self.prefix.is_empty() {
				res += &join_fea(&self.prefix, " ");
				res += " ";
			}
			res += &join_marked(&self.glyphs);
			if !self.suffix.is_empty() {
				res += " ";
				res += &join_fea(&self.suffix, " ");
			}
		} else {
			res += &join_fea(&self.glyphs, " ");
		}
		res += " by ";
		res += &glyph_as_fea(&self.replacement);
		res += ";";
		res
	}
}

/// A `lookupflag` statement (`LookupFlagStatement`).
#[derive(Debug, Clone, Default)]
pub struct LookupFlagStatement {
	/// The flags, without the mark attachment class and mark filtering set.
	pub value: i64,
	/// `markAttachment`.
	pub mark_attachment: Option<GlyphContainer>,
	/// `markFilteringSet`.
	pub mark_filtering_set: Option<GlyphContainer>,
	/// `location`.
	pub location: Location,
}

impl LookupFlagStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let mut res: Vec<String> = Vec::new();
		let flags = [
			"RightToLeft",
			"IgnoreBaseGlyphs",
			"IgnoreLigatures",
			"IgnoreMarks",
		];
		let mut curr = 1;
		for flag in flags {
			if self.value & curr != 0 {
				res.push(flag.to_owned());
			}
			curr <<= 1;
		}
		if let Some(m) = &self.mark_attachment {
			res.push(format!("MarkAttachmentType {}", m.as_fea()));
		}
		if let Some(m) = &self.mark_filtering_set {
			res.push(format!("UseMarkFilteringSet {}", m.as_fea()));
		}
		if res.is_empty() {
			res.push("0".to_owned());
		}
		format!("lookupflag {};", res.join(" "))
	}
}

/// `lookup ...;` inside a feature (`LookupReferenceStatement`).
#[derive(Debug, Clone)]
pub struct LookupReferenceStatement {
	/// `lookup`.
	pub lookup: Rc<LookupBlock>,
	/// `location`.
	pub location: Location,
}

impl LookupReferenceStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		format!("lookup {};", self.lookup.name)
	}
}

/// The `(anchor, mark class)` pairs of a mark attachment rule; the anchor is
/// `None` for `<anchor NULL>`.
pub type AnchorMarks = Vec<(Option<Anchor>, MarkClassRef)>;

fn marks_as_fea(marks: &AnchorMarks, prefix: &str) -> Result<String> {
	let mut res = String::new();
	for (a, m) in marks {
		res += &format!(
			"{prefix}{} mark @{}",
			anchor_as_fea(a.as_ref())?,
			m.borrow().name
		);
	}
	Ok(res)
}

/// A mark-to-base positioning rule (`MarkBasePosStatement`).
#[derive(Debug, Clone)]
pub struct MarkBasePosStatement {
	/// `base`.
	pub base: GlyphContainer,
	/// `marks`.
	pub marks: AnchorMarks,
	/// `location`.
	pub location: Location,
}

impl MarkBasePosStatement {
	/// `asFea(indent)`.
	pub fn as_fea(&self, indent: &str) -> Result<String> {
		let mut res = format!("pos base {}", self.base.as_fea());
		res += &marks_as_fea(&self.marks, &format!("\n{indent}{SHIFT}"))?;
		res += ";";
		Ok(res)
	}
}

/// A mark-to-ligature positioning rule (`MarkLigPosStatement`).
///
/// `marks` has one list per ligature component.
#[derive(Debug, Clone)]
pub struct MarkLigPosStatement {
	/// `ligatures`.
	pub ligatures: GlyphContainer,
	/// `marks`.
	pub marks: Vec<AnchorMarks>,
	/// `location`.
	pub location: Location,
}

impl MarkLigPosStatement {
	/// `asFea(indent)`.
	pub fn as_fea(&self, indent: &str) -> Result<String> {
		let mut res = format!("pos ligature {}", self.ligatures.as_fea());
		let mut ligs = Vec::new();
		for l in &self.marks {
			let temp = if l.is_empty() {
				format!("\n{indent}{SHIFT}{SHIFT}<anchor NULL>")
			} else {
				marks_as_fea(l, &format!("\n{indent}{SHIFT}{SHIFT}"))?
			};
			ligs.push(temp);
		}
		res += &ligs.join(&format!("\n{indent}{SHIFT}ligComponent"));
		res += ";";
		Ok(res)
	}
}

/// A mark-to-mark positioning rule (`MarkMarkPosStatement`).
#[derive(Debug, Clone)]
pub struct MarkMarkPosStatement {
	/// `baseMarks`.
	pub base_marks: GlyphContainer,
	/// `marks`.
	pub marks: AnchorMarks,
	/// `location`.
	pub location: Location,
}

impl MarkMarkPosStatement {
	/// `asFea(indent)`.
	pub fn as_fea(&self, indent: &str) -> Result<String> {
		let mut res = format!("pos mark {}", self.base_marks.as_fea());
		res += &marks_as_fea(&self.marks, &format!("\n{indent}{SHIFT}"))?;
		res += ";";
		Ok(res)
	}
}

/// A multiple substitution statement (`MultipleSubstStatement`).
#[derive(Debug, Clone)]
pub struct MultipleSubstStatement {
	/// `prefix`.
	pub prefix: Vec<GlyphContainer>,
	/// `glyph`.
	pub glyph: GlyphContainer,
	/// `suffix`.
	pub suffix: Vec<GlyphContainer>,
	/// `replacement`; empty for a glyph deletion.
	pub replacement: Vec<GlyphContainer>,
	/// Whether to write it as a chaining rule (`forceChain`).
	pub force_chain: bool,
	/// `location`.
	pub location: Location,
}

impl MultipleSubstStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let mut res = "sub ".to_owned();
		if !self.prefix.is_empty() || !self.suffix.is_empty() || self.force_chain {
			if !self.prefix.is_empty() {
				res += &join_fea(&self.prefix, " ");
				res += " ";
			}
			res += &self.glyph.as_fea();
			res += "'";
			if !self.suffix.is_empty() {
				res += " ";
				res += &join_fea(&self.suffix, " ");
			}
		} else {
			res += &self.glyph.as_fea();
		}
		res += " by ";
		if self.replacement.is_empty() {
			res += &NullGlyph::default().as_fea();
		} else {
			res += &join_fea(&self.replacement, " ");
		}
		res += ";";
		res
	}
}

/// A pair positioning statement (`PairPosStatement`).
#[derive(Debug, Clone)]
pub struct PairPosStatement {
	/// `glyphs1`.
	pub glyphs1: GlyphContainer,
	/// `valuerecord1`.
	pub valuerecord1: Option<ValueRecord>,
	/// `glyphs2`.
	pub glyphs2: GlyphContainer,
	/// `valuerecord2`.
	pub valuerecord2: Option<ValueRecord>,
	/// Whether this is an enumerated pair (`enumerated`).
	pub enumerated: bool,
	/// `location`.
	pub location: Location,
}

impl PairPosStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> Result<String> {
		let mut res = if self.enumerated { "enum " } else { "" }.to_owned();
		let vr1 = || {
			self.valuerecord1
				.as_ref()
				.map(ValueRecord::as_fea)
				.ok_or_else(|| {
					Error::Value("'NoneType' object has no attribute 'asFea'".to_owned())
				})
		};
		match &self.valuerecord2 {
			Some(vr2) if vr2.is_truthy() => {
				res += &format!(
					"pos {} {} {} {};",
					self.glyphs1.as_fea(),
					vr1()?,
					self.glyphs2.as_fea(),
					vr2.as_fea()
				);
			}
			_ => {
				res += &format!(
					"pos {} {} {};",
					self.glyphs1.as_fea(),
					self.glyphs2.as_fea(),
					vr1()?
				);
			}
		}
		Ok(res)
	}
}

/// A reverse chaining substitution statement
/// (`ReverseChainSingleSubstStatement`).
#[derive(Debug, Clone)]
pub struct ReverseChainSingleSubstStatement {
	/// `old_prefix`.
	pub old_prefix: Vec<GlyphContainer>,
	/// `old_suffix`.
	pub old_suffix: Vec<GlyphContainer>,
	/// `glyphs`: one item.
	pub glyphs: Vec<GlyphContainer>,
	/// `replacements`: one item.
	pub replacements: Vec<GlyphContainer>,
	/// `location`.
	pub location: Location,
}

impl ReverseChainSingleSubstStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let mut res = "rsub ".to_owned();
		if !self.old_prefix.is_empty() || !self.old_suffix.is_empty() {
			if !self.old_prefix.is_empty() {
				res += &join_fea(&self.old_prefix, " ");
				res += " ";
			}
			res += &join_marked(&self.glyphs);
			if !self.old_suffix.is_empty() {
				res += " ";
				res += &join_fea(&self.old_suffix, " ");
			}
		} else {
			res += &join_fea(&self.glyphs, " ");
		}
		res += &format!(" by {};", join_fea(&self.replacements, " "));
		res
	}
}

/// A single substitution statement (`SingleSubstStatement`).
#[derive(Debug, Clone)]
pub struct SingleSubstStatement {
	/// `prefix`.
	pub prefix: Vec<GlyphContainer>,
	/// `suffix`.
	pub suffix: Vec<GlyphContainer>,
	/// Whether to write it as a chaining rule (`forceChain`).
	pub force_chain: bool,
	/// `glyphs`: one item.
	pub glyphs: Vec<GlyphContainer>,
	/// `replacements`: one item.
	pub replacements: Vec<GlyphContainer>,
	/// `location`.
	pub location: Location,
}

impl SingleSubstStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let mut res = "sub ".to_owned();
		if !self.prefix.is_empty() || !self.suffix.is_empty() || self.force_chain {
			if !self.prefix.is_empty() {
				res += &join_fea(&self.prefix, " ");
				res += " ";
			}
			res += &join_marked(&self.glyphs);
			if !self.suffix.is_empty() {
				res += " ";
				res += &join_fea(&self.suffix, " ");
			}
		} else {
			res += &join_fea(&self.glyphs, " ");
		}
		res += &format!(" by {};", join_fea(&self.replacements, " "));
		res
	}
}

/// A `script` statement (`ScriptStatement`).
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptStatement {
	/// The script tag.
	pub script: String,
	/// `location`.
	pub location: Location,
}

impl ScriptStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		format!("script {};", fontoxide_misc::py::strip(&self.script))
	}
}

/// A single positioning statement (`SinglePosStatement`).
#[derive(Debug, Clone)]
pub struct SinglePosStatement {
	/// `pos`: `(glyphs, value record)` pairs.
	pub pos: Vec<(GlyphContainer, Option<ValueRecord>)>,
	/// `prefix`.
	pub prefix: Vec<GlyphContainer>,
	/// `suffix`.
	pub suffix: Vec<GlyphContainer>,
	/// Whether to write it as a chaining rule (`forceChain`).
	pub force_chain: bool,
	/// `location`.
	pub location: Location,
}

impl SinglePosStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let mut res = "pos ".to_owned();
		if !self.prefix.is_empty() || !self.suffix.is_empty() || self.force_chain {
			if !self.prefix.is_empty() {
				res += &join_fea(&self.prefix, " ");
				res += " ";
			}
			let items: Vec<String> = self
				.pos
				.iter()
				.map(|(g, v)| {
					format!(
						"{}'{}",
						g.as_fea(),
						v.as_ref()
							.map(|v| format!(" {}", v.as_fea()))
							.unwrap_or_default()
					)
				})
				.collect();
			res += &items.join(" ");
			if !self.suffix.is_empty() {
				res += " ";
				res += &join_fea(&self.suffix, " ");
			}
		} else {
			let items: Vec<String> = self
				.pos
				.iter()
				.map(|(g, v)| {
					format!(
						"{} {}",
						g.as_fea(),
						v.as_ref().map(ValueRecord::as_fea).unwrap_or_default()
					)
				})
				.collect();
			res += &items.join(" ");
		}
		res += ";";
		res
	}
}

/// A subtable break (`SubtableStatement`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubtableStatement {
	/// `location`.
	pub location: Location,
}

impl SubtableStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		"subtable;".to_owned()
	}
}

/// A named value record definition (`ValueRecordDefinition`).
#[derive(Debug, Clone)]
pub struct ValueRecordDefinition {
	/// The value record name.
	pub name: String,
	/// `value`.
	pub value: ValueRecord,
	/// `location`.
	pub location: Location,
}

impl ValueRecordDefinition {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		format!("valueRecordDef {} {};", self.value.as_fea(), self.name)
	}
}

/// `simplify_name_attributes(pid, eid, lid)`.
pub fn simplify_name_attributes(pid: i64, eid: i64, lid: i64) -> String {
	if pid == 3 && eid == 1 && lid == 1033 {
		String::new()
	} else if pid == 1 && eid == 0 && lid == 0 {
		"1".to_owned()
	} else {
		format!("{pid} {eid} {lid}")
	}
}

/// `getEncoding(platformID, platEncID, langID)` for the unbounded integers of
/// feature files.
pub(crate) fn name_encoding(
	platform_id: i64,
	plat_enc_id: i64,
	lang_id: i64,
) -> Option<&'static str> {
	let p = u16::try_from(platform_id).ok()?;
	let e = u16::try_from(plat_enc_id).ok()?;
	// Language IDs only matter for known values; anything else gets the default.
	let l = u16::try_from(lang_id).unwrap_or(u16::MAX);
	get_encoding(p, e, l)
}

fn plat_prefix(pid: i64, eid: i64, lid: i64) -> String {
	let mut plat = simplify_name_attributes(pid, eid, lid);
	if !plat.is_empty() {
		plat += " ";
	}
	plat
}

/// `_escape_quote_and_backslash()`: `"` and `\` escaped for a FEA string
/// literal; other characters are written as they are.
fn escape_quote_and_backslash(string: &str, pid: i64, eid: i64, lid: i64) -> String {
	let utf16 = name_encoding(pid, eid, lid) == Some("utf_16_be");
	string
		.chars()
		.map(|c| {
			if c == '"' || c == '\\' {
				if utf16 {
					format!("\\{:04x}", c as u32)
				} else {
					format!("\\{:02x}", c as u32)
				}
			} else {
				c.to_string()
			}
		})
		.collect()
}

/// A name record (`NameRecord`).
#[derive(Debug, Clone, PartialEq)]
pub struct NameRecord {
	/// `nameID`.
	pub name_id: i64,
	/// `platformID`.
	pub platform_id: i64,
	/// `platEncID`.
	pub plat_enc_id: i64,
	/// `langID`.
	pub lang_id: i64,
	/// The name string (`string`).
	pub string: String,
	/// `location`.
	pub location: Location,
}

impl NameRecord {
	/// `asFea()`.
	pub fn as_fea(&self) -> Result<String> {
		let escape = |c: u32, width: usize| -> String {
			// Also escape U+0022 QUOTATION MARK and U+005C REVERSE SOLIDUS
			if (0x20..=0x7E).contains(&c) && c != 0x22 && c != 0x5C {
				char::from_u32(c).map(String::from).unwrap_or_default()
			} else {
				format!("\\{c:0width$x}")
			}
		};
		let encoding = name_encoding(self.platform_id, self.plat_enc_id, self.lang_id)
			.ok_or_else(|| Error::feature_lib("Unsupported encoding", self.location.clone()))?;
		let s = codecs::encode(encoding, &self.string).map_err(|e| Error::Value(e.to_string()))?;
		let escaped_string: String = if encoding == "utf_16_be" {
			s.chunks(2)
				.map(|pair| {
					let hi = u32::from(pair[0]);
					let lo = pair.get(1).copied().map_or(0, u32::from);
					escape(hi * 256 + lo, 4)
				})
				.collect()
		} else {
			s.iter().map(|&b| escape(u32::from(b), 2)).collect()
		};
		let plat = plat_prefix(self.platform_id, self.plat_enc_id, self.lang_id);
		Ok(format!(
			"nameid {} {plat}\"{escaped_string}\";",
			self.name_id
		))
	}
}

/// A `sizemenuname` or `name` statement of a feature (`FeatureNameStatement`).
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureNameStatement {
	/// The feature tag, or `size` for `sizemenuname` (`nameID`).
	pub name_id: String,
	/// `platformID`.
	pub platform_id: i64,
	/// `platEncID`.
	pub plat_enc_id: i64,
	/// `langID`.
	pub lang_id: i64,
	/// The name string (`string`).
	pub string: String,
	/// `location`.
	pub location: Location,
}

impl FeatureNameStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let tag = if self.name_id == "size" {
			"sizemenuname"
		} else {
			"name"
		};
		let plat = plat_prefix(self.platform_id, self.plat_enc_id, self.lang_id);
		let s = escape_quote_and_backslash(
			&self.string,
			self.platform_id,
			self.plat_enc_id,
			self.lang_id,
		);
		format!("{tag} {plat}\"{s}\";")
	}
}

/// A STAT table `name` statement (`STATNameStatement`).
#[derive(Debug, Clone, PartialEq)]
pub struct STATNameStatement {
	/// Always `stat` from the parser (`nameID`).
	pub name_id: String,
	/// `platformID`.
	pub platform_id: i64,
	/// `platEncID`.
	pub plat_enc_id: i64,
	/// `langID`.
	pub lang_id: i64,
	/// The name string (`string`).
	pub string: String,
	/// `location`.
	pub location: Location,
}

impl STATNameStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let plat = plat_prefix(self.platform_id, self.plat_enc_id, self.lang_id);
		let s = escape_quote_and_backslash(
			&self.string,
			self.platform_id,
			self.plat_enc_id,
			self.lang_id,
		);
		format!("name {plat}\"{s}\";")
	}
}

/// A `parameters` statement of the `size` feature (`SizeParameters`).
#[derive(Debug, Clone, PartialEq)]
pub struct SizeParameters {
	/// `DesignSize`, in points.
	pub design_size: f64,
	/// `SubfamilyID`.
	pub subfamily_id: i64,
	/// `RangeStart`, in points.
	pub range_start: f64,
	/// `RangeEnd`, in points.
	pub range_end: f64,
	/// `location`.
	pub location: Location,
}

impl SizeParameters {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let mut res = format!("parameters {:.1} {}", self.design_size, self.subfamily_id);
		if self.range_start != 0.0 || self.range_end != 0.0 {
			res += &format!(
				" {} {}",
				(self.range_start * 10.0).trunc() as i64,
				(self.range_end * 10.0).trunc() as i64
			);
		}
		res + ";"
	}
}

/// A name statement inside a `cvParameters` block
/// (`CVParametersNameStatement`).
#[derive(Debug, Clone, PartialEq)]
pub struct CVParametersNameStatement {
	/// The feature tag (`nameID`).
	pub name_id: String,
	/// `platformID`.
	pub platform_id: i64,
	/// `platEncID`.
	pub plat_enc_id: i64,
	/// `langID`.
	pub lang_id: i64,
	/// The name string (`string`).
	pub string: String,
	/// The enclosing block, e.g. `FeatUILabelNameID` (`block_name`).
	pub block_name: String,
	/// `location`.
	pub location: Location,
}

impl CVParametersNameStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let plat = plat_prefix(self.platform_id, self.plat_enc_id, self.lang_id);
		let s = escape_quote_and_backslash(
			&self.string,
			self.platform_id,
			self.plat_enc_id,
			self.lang_id,
		);
		format!("name {plat}\"{s}\";")
	}
}

/// A `Character` statement of a `cvParameters` block (`CharacterStatement`).
#[derive(Debug, Clone, PartialEq)]
pub struct CharacterStatement {
	/// The Unicode value.
	pub character: i64,
	/// The feature tag.
	pub tag: String,
	/// `location`.
	pub location: Location,
}

impl CharacterStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		if self.character < 0 {
			format!("Character -{:#x};", self.character.unsigned_abs())
		} else {
			format!("Character {:#x};", self.character)
		}
	}
}

/// A `(script tag, default baseline tag, coordinates)` record of a BASE axis.
pub type BaseScriptRecord = (String, String, Vec<i64>);

/// A `(script tag, language tag, min, max)` record of a BASE axis.
pub type BaseMinMax = (String, String, i64, i64);

/// A BASE axis definition: a `BaseTagList`/`BaseScriptList` pair (`BaseAxis`).
#[derive(Debug, Clone, PartialEq)]
pub struct BaseAxis {
	/// The baseline tags.
	pub bases: Vec<String>,
	/// The script records.
	pub scripts: Vec<BaseScriptRecord>,
	/// `VertAxis` if true, `HorizAxis` if false.
	pub vertical: bool,
	/// The `MinMax` records.
	pub minmax: Vec<BaseMinMax>,
	/// `location`.
	pub location: Location,
}

impl BaseAxis {
	/// `BaseAxis(bases, scripts, vertical, minmax)`.
	// UPSTREAM-BUG(B34): `BaseAxis.__init__` ignores its `minmax` argument and starts
	// with an empty list; reproduced.
	pub fn new(
		bases: Vec<String>,
		scripts: Vec<BaseScriptRecord>,
		vertical: bool,
		_minmax: Option<Vec<BaseMinMax>>,
		location: Location,
	) -> Self {
		BaseAxis {
			bases,
			scripts,
			vertical,
			minmax: Vec::new(),
			location,
		}
	}

	/// `asFea(indent)`.
	pub fn as_fea(&self, indent: &str) -> String {
		let direction = if self.vertical { "Vert" } else { "Horiz" };
		let scripts: Vec<String> = self
			.scripts
			.iter()
			.map(|(a, b, c)| {
				let coords: Vec<String> = c.iter().map(i64::to_string).collect();
				format!("{a} {b} {}", coords.join(" "))
			})
			.collect();
		let minmaxes: Vec<String> = self
			.minmax
			.iter()
			.map(|(a, b, c, d)| format!("\n{direction}Axis.MinMax {a} {b} {c}, {d};"))
			.collect();
		format!(
			"{direction}Axis.BaseTagList {};\n{indent}{direction}Axis.BaseScriptList {};",
			self.bases.join(" "),
			scripts.join(", ")
		) + &minmaxes.join("\n")
	}
}

/// The value of an [`OS2Field`].
#[derive(Debug, Clone, PartialEq)]
pub enum OS2Value {
	/// `None`, for an unknown key.
	None,
	/// A number.
	Int(i64),
	/// An array of numbers (`UnicodeRange`, `CodePageRange`, `Panose`).
	IntArray(Vec<i64>),
	/// A string (`Vendor`).
	Str(String),
}

impl OS2Value {
	fn py_str(&self) -> String {
		match self {
			OS2Value::None => "None".to_owned(),
			OS2Value::Int(i) => i.to_string(),
			OS2Value::IntArray(a) => {
				let items: Vec<String> = a.iter().map(i64::to_string).collect();
				format!("[{}]", items.join(", "))
			}
			OS2Value::Str(s) => s.clone(),
		}
	}

	/// `" ".join(map(str, x))`.
	fn intarr2str(&self) -> Result<String> {
		match self {
			OS2Value::IntArray(a) => Ok(a.iter().map(i64::to_string).collect::<Vec<_>>().join(" ")),
			OS2Value::Str(s) => Ok(s.chars().map(String::from).collect::<Vec<_>>().join(" ")),
			OS2Value::Int(_) => Err(Error::Value("'int' object is not iterable".to_owned())),
			OS2Value::None => Err(Error::Value("'NoneType' object is not iterable".to_owned())),
		}
	}
}

/// An entry in the `OS/2` table (`OS2Field`).
#[derive(Debug, Clone, PartialEq)]
pub struct OS2Field {
	/// The lowercased keyword.
	pub key: String,
	/// `value`.
	pub value: OS2Value,
	/// `location`.
	pub location: Location,
}

impl OS2Field {
	/// `asFea()`: empty for an unknown key.
	pub fn as_fea(&self) -> Result<String> {
		const NUMBERS: [&str; 12] = [
			"FSType",
			"TypoAscender",
			"TypoDescender",
			"TypoLineGap",
			"winAscent",
			"winDescent",
			"XHeight",
			"CapHeight",
			"WeightClass",
			"WidthClass",
			"LowerOpSize",
			"UpperOpSize",
		];
		const RANGES: [&str; 2] = ["UnicodeRange", "CodePageRange"];
		if let Some(name) = NUMBERS.iter().find(|n| n.to_lowercase() == self.key) {
			return Ok(format!("{name} {};", self.value.py_str()));
		}
		if let Some(name) = RANGES.iter().find(|n| n.to_lowercase() == self.key) {
			return Ok(format!("{name} {};", self.value.intarr2str()?));
		}
		if self.key == "panose" {
			return Ok(format!("Panose {};", self.value.intarr2str()?));
		}
		if self.key == "vendor" {
			return Ok(format!("Vendor \"{}\";", self.value.py_str()));
		}
		// should raise exception
		Ok(String::new())
	}
}

/// An entry in the `hhea` table (`HheaField`).
#[derive(Debug, Clone, PartialEq)]
pub struct HheaField {
	/// The lowercased keyword.
	pub key: String,
	/// `value`.
	pub value: i64,
	/// `location`.
	pub location: Location,
}

fn field_as_fea(fields: &[&str], key: &str, value: i64) -> Result<String> {
	let name = fields
		.iter()
		.find(|f| f.to_lowercase() == key)
		.ok_or_else(|| Error::Value(fontoxide_misc::py::str_repr(key)))?;
	Ok(format!("{name} {value};"))
}

impl HheaField {
	/// `asFea()`.
	pub fn as_fea(&self) -> Result<String> {
		field_as_fea(
			&["CaretOffset", "Ascender", "Descender", "LineGap"],
			&self.key,
			self.value,
		)
	}
}

/// An entry in the `vhea` table (`VheaField`).
#[derive(Debug, Clone, PartialEq)]
pub struct VheaField {
	/// The lowercased keyword.
	pub key: String,
	/// `value`.
	pub value: i64,
	/// `location`.
	pub location: Location,
}

impl VheaField {
	/// `asFea()`.
	pub fn as_fea(&self) -> Result<String> {
		field_as_fea(
			&["VertTypoAscender", "VertTypoDescender", "VertTypoLineGap"],
			&self.key,
			self.value,
		)
	}
}

/// A STAT table design axis (`STATDesignAxisStatement`).
#[derive(Debug, Clone, PartialEq)]
pub struct STATDesignAxisStatement {
	/// The axis tag.
	pub tag: String,
	/// `axisOrder`.
	pub axis_order: i64,
	/// `names`.
	pub names: Vec<STATNameStatement>,
	/// `location`.
	pub location: Location,
}

fn names_as_fea(names: &[STATNameStatement], indent: &str) -> String {
	let parts: Vec<String> = names.iter().map(STATNameStatement::as_fea).collect();
	parts.join(&format!("\n{indent}"))
}

impl STATDesignAxisStatement {
	/// `asFea(indent)`.
	pub fn as_fea(&self, indent: &str) -> String {
		let indent = format!("{indent}{SHIFT}");
		let mut res = format!("DesignAxis {} {} {{ \n", self.tag, self.axis_order);
		res += &names_as_fea(&self.names, &indent);
		res += "\n";
		res += "};";
		res
	}
}

/// The STAT table `ElidedFallbackName` (`ElidedFallbackName`).
#[derive(Debug, Clone, PartialEq)]
pub struct ElidedFallbackName {
	/// `names`.
	pub names: Vec<STATNameStatement>,
	/// `location`.
	pub location: Location,
}

impl ElidedFallbackName {
	/// `asFea(indent)`.
	pub fn as_fea(&self, indent: &str) -> String {
		let indent = format!("{indent}{SHIFT}");
		let mut res = "ElidedFallbackName { \n".to_owned();
		res += &names_as_fea(&self.names, &indent);
		res += "\n";
		res += "};";
		res
	}
}

/// The STAT table `ElidedFallbackNameID` (`ElidedFallbackNameID`).
#[derive(Debug, Clone, PartialEq)]
pub struct ElidedFallbackNameID {
	/// The name ID.
	pub value: i64,
	/// `location`.
	pub location: Location,
}

impl ElidedFallbackNameID {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		format!("ElidedFallbackNameID {};", self.value)
	}
}

/// A STAT table axis value record (`STATAxisValueStatement`).
#[derive(Debug, Clone, PartialEq)]
pub struct STATAxisValueStatement {
	/// `names`.
	pub names: Vec<STATNameStatement>,
	/// `locations`.
	pub locations: Vec<AxisValueLocationStatement>,
	/// `flags`.
	pub flags: i64,
	/// `location`.
	pub location: Location,
}

impl STATAxisValueStatement {
	/// `asFea()`.
	pub fn as_fea(&self) -> String {
		let mut res = "AxisValue {\n".to_owned();
		for location in &self.locations {
			res += &location.as_fea("");
		}
		for name_record in &self.names {
			res += &name_record.as_fea();
			res += "\n";
		}
		if self.flags != 0 {
			let flags = ["OlderSiblingFontAttribute", "ElidableAxisValueName"];
			let mut flag_strings = Vec::new();
			let mut curr = 1;
			for flag in flags {
				if self.flags & curr != 0 {
					flag_strings.push(flag);
				}
				curr <<= 1;
			}
			res += &format!("flag {};\n", flag_strings.join(" "));
		}
		res += "};";
		res
	}
}

/// A STAT table axis value location (`AxisValueLocationStatement`).
#[derive(Debug, Clone, PartialEq)]
pub struct AxisValueLocationStatement {
	/// The axis tag.
	pub tag: String,
	/// The values: one, or nominal, minimum and maximum.
	pub values: Vec<IntOrFloat>,
	/// `location`.
	pub location: Location,
}

impl AxisValueLocationStatement {
	/// `asFea(res)`: appends to `res` (this is not an indent).
	pub fn as_fea(&self, res: &str) -> String {
		let values: Vec<String> = self.values.iter().map(IntOrFloat::to_string).collect();
		format!("{res}location {} {};\n", self.tag, values.join(" "))
	}
}

/// A variable layout conditionset (`ConditionsetStatement`).
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionsetStatement {
	/// The name of this conditionset.
	pub name: String,
	/// Axis tags mapped to `(min, max)` user-space coordinates.
	pub conditions: IndexMap<String, (IntOrFloat, IntOrFloat)>,
	/// `location`.
	pub location: Location,
}

impl ConditionsetStatement {
	/// `asFea(indent=indent)` (with `res=""`).
	pub fn as_fea(&self, indent: &str) -> String {
		let mut res = format!("{indent}conditionset {} {{\n", self.name);
		for (tag, (minvalue, maxvalue)) in &self.conditions {
			res += &format!("{indent}{SHIFT}{tag} {minvalue} {maxvalue};\n");
		}
		res += &format!("{indent}}} {};\n", self.name);
		res
	}
}
