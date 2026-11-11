//! Parsing feature files into an AST (`fontTools.feaLib.parser`).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;

use fontoxide_encodings::codecs;
use indexmap::{IndexMap, IndexSet};

use crate::ast::{
	self, AlternateSubstStatement, Anchor, AnchorDefinition, AnchorMarks, AnonymousBlock,
	AttachStatement, AxisValueLocationStatement, BaseAxis, BaseMinMax, BaseScriptRecord,
	CVParametersNameStatement, ChainContext, ChainContextPosStatement, ChainContextSubstStatement,
	ChainLookups, CharacterStatement, ClassName, Comment, ConditionsetStatement,
	CursivePosStatement, Device, ElidedFallbackName, ElidedFallbackNameID, FeatureBlock,
	FeatureFile, FeatureNameStatement, FeatureReferenceStatement, FontRevisionStatement,
	GlyphClass, GlyphClassDefStatement, GlyphClassDefinition, GlyphClassName, GlyphContainer,
	GlyphName, HheaField, IgnorePosStatement, IgnoreSubstStatement, IncludeStatement, IntOrFloat,
	LanguageStatement, LanguageSystemStatement, LigatureCaretByIndexStatement,
	LigatureCaretByPosStatement, LigatureSubstStatement, LookupBlock, LookupFlagStatement,
	LookupReferenceStatement, MarkBasePosStatement, MarkClass, MarkClassDefinition, MarkClassName,
	MarkClassRef, MarkLigPosStatement, MarkMarkPosStatement, MultipleSubstStatement, NameRecord,
	NestedBlock, NullGlyph, OS2Field, OS2Value, PairPosStatement, ReverseChainSingleSubstStatement,
	STATAxisValueStatement, STATDesignAxisStatement, STATNameStatement, ScalarValue,
	ScriptStatement, SinglePosStatement, SingleSubstStatement, SizeParameters, Statement,
	SubtableStatement, TableBlock, ValueRecord, ValueRecordDefinition, VariationBlock, VheaField,
};
use crate::error::{Error, Result};
use crate::lexer::{FeatureSource, IncludingLexer, NonIncludingLexer, Token, TokenType};
use crate::location::{FeatureLibLocation, Location, location_str};
use crate::variable_scalar::VariableScalar;

/// A symbol table with nested scopes (`SymbolTable`).
#[derive(Debug, Clone)]
pub struct SymbolTable<T> {
	scopes_: Vec<HashMap<String, T>>,
}

impl<T> Default for SymbolTable<T> {
	fn default() -> Self {
		SymbolTable {
			scopes_: vec![HashMap::new()],
		}
	}
}

impl<T> SymbolTable<T> {
	/// `SymbolTable()`.
	pub fn new() -> Self {
		Self::default()
	}

	/// `enter_scope()`.
	pub fn enter_scope(&mut self) {
		self.scopes_.push(HashMap::new());
	}

	/// `exit_scope()`.
	pub fn exit_scope(&mut self) {
		self.scopes_.pop();
	}

	/// `define(name, item)`: defines in the innermost scope.
	pub fn define(&mut self, name: impl Into<String>, item: T) {
		if let Some(scope) = self.scopes_.last_mut() {
			scope.insert(name.into(), item);
		}
	}

	/// `resolve(name)`: the item in the innermost scope that defines it.
	pub fn resolve(&self, name: &str) -> Option<&T> {
		self.scopes_.iter().rev().find_map(|scope| scope.get(name))
	}
}

/// What a glyph class name refers to: a glyph class or a mark class.
#[derive(Debug, Clone)]
pub enum GlyphClassEntry {
	/// A `GlyphClassDefinition`.
	GlyphClass(Rc<GlyphClassDefinition>),
	/// A `MarkClass`.
	MarkClass(MarkClassRef),
}

/// The lexer of a parser: following includes or not.
#[derive(Debug, Clone)]
enum AnyLexer {
	Including(IncludingLexer),
	NonIncluding(NonIncludingLexer),
}

impl AnyLexer {
	fn next(&mut self) -> Result<Option<crate::lexer::LexerToken>> {
		match self {
			AnyLexer::Including(l) => l.next(),
			AnyLexer::NonIncluding(l) => l.next(),
		}
	}

	fn scan_anonymous_block(&mut self, tag: &str) -> Result<crate::lexer::LexerToken> {
		match self {
			AnyLexer::Including(l) => l.scan_anonymous_block(tag),
			AnyLexer::NonIncluding(l) => l.scan_anonymous_block(tag),
		}
	}
}

/// The result of `parse_glyph_pattern_`: prefix, glyphs, lookups, values,
/// suffix, and whether there were marked glyphs.
type GlyphPattern = (
	Vec<GlyphContainer>,
	Vec<GlyphContainer>,
	ChainLookups,
	Vec<Option<ValueRecord>>,
	Vec<GlyphContainer>,
	bool,
);

/// A name record: platform ID, platform encoding ID, language ID and string.
type NameParts = (i64, i64, i64, String);

const SS_FEATURE_TAGS: std::ops::RangeInclusive<u32> = 1..=20;
const CV_FEATURE_TAGS: std::ops::RangeInclusive<u32> = 1..=99;

fn is_numbered_tag(tag: &str, prefix: &str, range: std::ops::RangeInclusive<u32>) -> bool {
	tag.strip_prefix(prefix).is_some_and(|n| {
		n.len() == 2
			&& n.chars().all(|c| c.is_ascii_digit())
			&& n.parse().is_ok_and(|n| range.contains(&n))
	})
}

/// Python truthiness of `any(values)` over a slice of optional value records.
fn any_values(values: &[Option<ValueRecord>]) -> bool {
	values
		.iter()
		.any(|v| v.as_ref().is_some_and(ValueRecord::is_truthy))
}

/// `values[start:end]` with Python's clamping.
fn slice<T>(v: &[T], start: usize, end: usize) -> &[T] {
	let end = end.min(v.len());
	let start = start.min(end);
	&v[start..end]
}

/// `any(lookups)`: whether some marked glyph has a non-empty lookup list.
fn any_lookups(lookups: &ChainLookups) -> bool {
	lookups
		.iter()
		.any(|l| l.as_ref().is_some_and(|l| !l.is_empty()))
}

/// `int(s)` for a string, with Python's error message.
fn py_int(s: &str) -> Result<i64> {
	let invalid = || {
		Error::Value(format!(
			"invalid literal for int() with base 10: {}",
			fontoxide_misc::py::str_repr(s)
		))
	};
	let t = fontoxide_misc::py::strip(s);
	let (negative, digits) = match t.as_bytes().first() {
		Some(b'-') => (true, &t[1..]),
		Some(b'+') => (false, &t[1..]),
		_ => (false, t),
	};
	// Underscores are allowed between digits.
	if digits.is_empty()
		|| digits.starts_with('_')
		|| digits.ends_with('_')
		|| digits.contains("__")
		|| !digits.chars().all(|c| c.is_ascii_digit() || c == '_')
	{
		return Err(invalid());
	}
	let digits: String = digits.chars().filter(|&c| c != '_').collect();
	// LENIENT(L33): integers beyond `i64` are an error.
	let magnitude: i64 = digits.parse().map_err(|_| invalid())?;
	Ok(if negative { -magnitude } else { magnitude })
}

/// `str.isupper()`.
fn py_isupper(s: &str) -> bool {
	let mut cased = false;
	for c in s.chars() {
		if c.is_lowercase() {
			return false;
		}
		if c.is_uppercase() {
			cased = true;
		}
	}
	cased
}

/// `os.path.commonprefix([a, b])` on characters.
fn common_prefix(a: &[char], b: &[char]) -> usize {
	a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// Initializes a parser (`Parser`).
///
/// The `glyph_names` serve a double role to help distinguish glyph names from
/// ranges in the presence of hyphens and to ensure that glyph names referenced in
/// a feature file are actually part of a font's glyph set. If they are empty, no
/// glyph name checking takes place, and all glyph tokens containing hyphens are
/// treated as literal glyph names, not as ranges.
///
/// By default, the parser follows `include()` statements in the feature file;
/// [`Parser::with_options`] can turn this off and set the directory included
/// files are searched in.
// LENIENT(L33): `Parser.extensions` (subclassing hooks for new keywords) and the
// deprecated `glyphMap` argument are not supported.
#[derive(Debug)]
pub struct Parser {
	glyph_names_: HashSet<String>,
	doc_: FeatureFile,
	anchors_: SymbolTable<AnchorDefinition>,
	glyphclasses_: SymbolTable<GlyphClassEntry>,
	lookups_: SymbolTable<Rc<LookupBlock>>,
	valuerecords_: SymbolTable<ValueRecordDefinition>,
	next_token_type_: Option<TokenType>,
	next_token_: Token,
	next_token_location_: Location,
	cur_token_type_: Option<TokenType>,
	cur_token_: Token,
	cur_token_location_: Location,
	cur_comments_: VecDeque<(String, FeatureLibLocation)>,
	lexer_: AnyLexer,
	missing: IndexMap<String, Location>,
}

impl Parser {
	/// Keywords that can start a statement in a feature block. A multi-tag
	/// language statement ends before them, so that a missing semicolon is
	/// reported instead of the next statement being read as language tags
	/// (`_statement_keywords`).
	pub const STATEMENT_KEYWORDS: &'static [&'static str] = &[
		"anchorDef",
		"cvParameters",
		"enum",
		"enumerate",
		"feature",
		"featureNames",
		"ignore",
		"language",
		"lookup",
		"lookupflag",
		"markClass",
		"parameters",
		"pos",
		"position",
		"reversesub",
		"rsub",
		"script",
		"sizemenuname",
		"sub",
		"substitute",
		"subtable",
		"valueRecordDef",
	];

	/// `Parser(featurefile, glyphNames)`, following includes.
	pub fn new<S: Into<String>>(
		featurefile: impl Into<FeatureSource>,
		glyph_names: impl IntoIterator<Item = S>,
	) -> Result<Self> {
		Self::with_options(featurefile, glyph_names, true, None)
	}

	/// `Parser(featurefile, glyphNames, followIncludes, includeDir)`.
	pub fn with_options<S: Into<String>>(
		featurefile: impl Into<FeatureSource>,
		glyph_names: impl IntoIterator<Item = S>,
		follow_includes: bool,
		include_dir: Option<PathBuf>,
	) -> Result<Self> {
		let lexer_ = if follow_includes {
			AnyLexer::Including(IncludingLexer::new(featurefile, include_dir)?)
		} else {
			AnyLexer::NonIncluding(NonIncludingLexer::new(featurefile, include_dir)?)
		};
		let mut parser = Parser {
			glyph_names_: glyph_names.into_iter().map(Into::into).collect(),
			doc_: FeatureFile::new(),
			anchors_: SymbolTable::new(),
			glyphclasses_: SymbolTable::new(),
			lookups_: SymbolTable::new(),
			valuerecords_: SymbolTable::new(),
			next_token_type_: None,
			next_token_: Token::None,
			next_token_location_: None,
			cur_token_type_: None,
			cur_token_: Token::None,
			cur_token_location_: None,
			cur_comments_: VecDeque::new(),
			lexer_,
			missing: IndexMap::new(),
		};
		parser.advance_lexer_(true)?;
		Ok(parser)
	}

	/// The glyph names given to the parser (`glyphNames_`).
	pub fn glyph_names(&self) -> &HashSet<String> {
		&self.glyph_names_
	}

	fn cur_loc(&self) -> Location {
		self.cur_token_location_.clone()
	}

	fn cur_str(&self) -> String {
		self.cur_token_.py_str()
	}

	fn type_str(t: Option<TokenType>) -> &'static str {
		t.map_or("None", TokenType::as_str)
	}

	fn comment(&self) -> Statement {
		Statement::Comment(Comment {
			text: self.cur_str(),
			location: self.cur_loc(),
		})
	}

	/// Parses the file into a [`FeatureFile`], the root of the abstract syntax
	/// tree (`parse`).
	pub fn parse(mut self) -> Result<FeatureFile> {
		while self.next_token_type_.is_some() || !self.cur_comments_.is_empty() {
			self.advance_lexer_(true)?;
			let statement = if self.cur_token_type_ == Some(TokenType::Comment) {
				self.comment()
			} else if self.is_cur_keyword_("include") {
				self.parse_include_()?
			} else if self.cur_token_type_ == Some(TokenType::Glyphclass) {
				self.parse_glyphclass_definition_()?
			} else if self.is_cur_keyword_any(&["anon", "anonymous"]) {
				self.parse_anonymous_()?
			} else if self.is_cur_keyword_("anchorDef") {
				Statement::AnchorDefinition(self.parse_anchordef_()?)
			} else if self.is_cur_keyword_("languagesystem") {
				self.parse_languagesystem_()?
			} else if self.is_cur_keyword_("lookup") {
				self.parse_lookup_(false)?
			} else if self.is_cur_keyword_("markClass") {
				self.parse_mark_class_()?
			} else if self.is_cur_keyword_("feature") {
				self.parse_feature_block_(false)?
			} else if self.is_cur_keyword_("conditionset") {
				self.parse_conditionset_()?
			} else if self.is_cur_keyword_("variation") {
				self.parse_feature_block_(true)?
			} else if self.is_cur_keyword_("table") {
				self.parse_table_()?
			} else if self.is_cur_keyword_("valueRecordDef") {
				Statement::ValueRecordDefinition(self.parse_valuerecord_definition_(false)?)
			} else if self.cur_token_type_ == Some(TokenType::Symbol) && self.cur_token_.is(";") {
				continue;
			} else {
				return Err(Error::feature_lib(
					format!(
						"Expected feature, languagesystem, lookup, markClass, \
						 table, or glyph class definition, got {} \"{}\"",
						Self::type_str(self.cur_token_type_),
						self.cur_str()
					),
					self.cur_loc(),
				));
			};
			self.doc_.statements.push(statement);
		}
		// Report any missing glyphs at the end of parsing
		if !self.missing.is_empty() {
			let error: Vec<String> = self
				.missing
				.iter()
				.map(|(name, loc)| format!(" {name} (first found at {})", location_str(loc)))
				.collect();
			return Err(Error::feature_lib(
				format!(
					"The following glyph names are referenced but are missing from the \
					 glyph set:\n{}",
					error.join("\n")
				),
				None,
			));
		}
		Ok(self.doc_)
	}

	/// Parses an anchor in any of the four formats given in the feature file
	/// specification (2.e.vii); `None` for `<anchor NULL>` (`parse_anchor_`).
	fn parse_anchor_(&mut self) -> Result<Option<Anchor>> {
		self.expect_symbol_("<")?;
		self.expect_keyword_("anchor")?;
		let location = self.cur_loc();

		if self.next_token_.is("NULL") {
			// Format D
			self.expect_keyword_("NULL")?;
			self.expect_symbol_(">")?;
			return Ok(None);
		}

		if self.next_token_type_ == Some(TokenType::Name) {
			// Format E
			let name = self.expect_name_()?;
			let Some(anchordef) = self.anchors_.resolve(&name) else {
				return Err(Error::feature_lib(
					format!("Unknown anchor \"{name}\""),
					self.cur_loc(),
				));
			};
			let (x, y, contourpoint) = (anchordef.x, anchordef.y, anchordef.contourpoint);
			self.expect_symbol_(">")?;
			return Ok(Some(Anchor {
				x: ScalarValue::Int(x),
				y: ScalarValue::Int(y),
				name: Some(name),
				contourpoint,
				x_device_table: None,
				y_device_table: None,
				location,
			}));
		}

		let x = self.expect_number_variable_()?;
		let y = self.expect_number_variable_()?;

		let mut contourpoint = None;
		if self.next_token_.is("contourpoint") {
			// Format B
			self.expect_keyword_("contourpoint")?;
			contourpoint = Some(self.expect_number_()?);
		}

		let (x_device_table, y_device_table) = if self.next_token_.is("<") {
			// Format C
			let x = self.parse_device_()?;
			let y = self.parse_device_()?;
			(x, y)
		} else {
			(None, None)
		};

		self.expect_symbol_(">")?;
		Ok(Some(Anchor {
			x,
			y,
			name: None,
			contourpoint,
			x_device_table,
			y_device_table,
			location,
		}))
	}

	/// Parses a sequence of `[<anchor> mark @MARKCLASS]*` (`parse_anchor_marks_`).
	fn parse_anchor_marks_(&mut self) -> Result<AnchorMarks> {
		let mut anchor_marks = Vec::new();
		while self.next_token_.is("<") {
			let anchor = self.parse_anchor_()?;
			if anchor.is_none() && !self.next_token_.is("mark") {
				continue; // <anchor NULL> without mark, eg. in GPOS type 5
			}
			self.expect_keyword_("mark")?;
			let mark_class = self.expect_mark_class_reference_()?;
			anchor_marks.push((anchor, mark_class));
		}
		Ok(anchor_marks)
	}

	/// Parses a named anchor definition (`parse_anchordef_`).
	fn parse_anchordef_(&mut self) -> Result<AnchorDefinition> {
		let location = self.cur_loc();
		let x = self.expect_number_()?;
		let y = self.expect_number_()?;
		let mut contourpoint = None;
		if self.next_token_.is("contourpoint") {
			self.expect_keyword_("contourpoint")?;
			contourpoint = Some(self.expect_number_()?);
		}
		let name = self.expect_name_()?;
		self.expect_symbol_(";")?;
		let anchordef = AnchorDefinition {
			name: name.clone(),
			x,
			y,
			contourpoint,
			location,
		};
		self.anchors_.define(name, anchordef.clone());
		Ok(anchordef)
	}

	/// Parses an anonymous data block (`parse_anonymous_`).
	fn parse_anonymous_(&mut self) -> Result<Statement> {
		let tag = self.expect_tag_()?;
		let (_, content, location) = self.lexer_.scan_anonymous_block(&tag)?;
		self.advance_lexer_(false)?;
		self.expect_symbol_("}")?;
		let end_tag = self.expect_tag_()?;
		if tag != end_tag {
			return Err(Error::Assertion(
				"bad splitting in Lexer.scan_anonymous_block()".to_owned(),
			));
		}
		self.expect_symbol_(";")?;
		Ok(Statement::AnonymousBlock(AnonymousBlock {
			tag,
			content: content.py_str(),
			location: Some(location),
		}))
	}

	/// Parses a GDEF `Attach` statement (`parse_attach_`).
	fn parse_attach_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let glyphs = self.parse_glyphclass_(true, false)?;
		let mut contour_points = IndexSet::new();
		contour_points.insert(self.expect_number_()?);
		while !self.next_token_.is(";") {
			contour_points.insert(self.expect_number_()?);
		}
		self.expect_symbol_(";")?;
		Ok(Statement::AttachStatement(AttachStatement {
			glyphs,
			contour_points,
			location,
		}))
	}

	/// Parses an enumerated pair positioning rule (`parse_enumerate_`).
	fn parse_enumerate_(&mut self, vertical: bool) -> Result<Statement> {
		self.advance_lexer_(false)?;
		self.parse_position_(true, vertical)
	}

	/// Parses `GlyphClassDef @BASE, @LIGATURES, @MARKS, @COMPONENTS;`
	/// (`parse_GlyphClassDef_`).
	fn parse_glyph_class_def_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let parse = |this: &mut Self, end: &str| -> Result<Option<GlyphContainer>> {
			let glyphs = if !this.next_token_.is(end) {
				Some(this.parse_glyphclass_(false, false)?)
			} else {
				None
			};
			this.expect_symbol_(end)?;
			Ok(glyphs)
		};
		let base_glyphs = parse(self, ",")?;
		let ligature_glyphs = parse(self, ",")?;
		let mark_glyphs = parse(self, ",")?;
		let component_glyphs = parse(self, ";")?;
		Ok(Statement::GlyphClassDefStatement(GlyphClassDefStatement {
			base_glyphs,
			mark_glyphs,
			ligature_glyphs,
			component_glyphs,
			location,
		}))
	}

	/// Parses glyph class definitions such as `@UPPERCASE = [A-Z];`
	/// (`parse_glyphclass_definition_`).
	fn parse_glyphclass_definition_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let name = self.cur_str();
		self.expect_symbol_("=")?;
		let glyphs = self.parse_glyphclass_(false, false)?;
		self.expect_symbol_(";")?;
		let glyphclass = Rc::new(GlyphClassDefinition {
			name: name.clone(),
			glyphs,
			location,
		});
		self.glyphclasses_
			.define(name, GlyphClassEntry::GlyphClass(glyphclass.clone()));
		Ok(Statement::GlyphClassDefinition(glyphclass))
	}

	/// Splits a dashed name into a range of known glyphs, rejecting ambiguous
	/// splits (`split_glyph_range_`).
	///
	/// Since v1.20, the OpenType Feature File specification allows for dashes in
	/// glyph names. A sequence like "a-b-c-d" could therefore mean a single glyph
	/// whose name happens to be "a-b-c-d", or a range between any split of it. All
	/// possible splits are tried and the feature file is rejected if more than one
	/// is possible.
	fn split_glyph_range_(&self, name: &str, location: &Location) -> Result<(String, String)> {
		let parts: Vec<&str> = name.split('-').collect();
		let mut solutions = Vec::new();
		for i in 0..parts.len() {
			let start = parts[..i].join("-");
			let limit = parts[i..].join("-");
			if self.glyph_names_.contains(&start) && self.glyph_names_.contains(&limit) {
				solutions.push((start, limit));
			}
		}
		match solutions.len() {
			1 => Ok(solutions.swap_remove(0)),
			0 => Err(Error::feature_lib(
				format!(
					"\"{name}\" is not a glyph in the font, and it can not be split \
					 into a range of known glyphs"
				),
				location.clone(),
			)),
			_ => {
				let ranges: Vec<String> = solutions
					.iter()
					.map(|(s, l)| format!("\"{s} - {l}\""))
					.collect();
				Err(Error::feature_lib(
					format!(
						"Ambiguous glyph range \"{name}\"; please use {} to clarify what you mean",
						ranges.join(" or ")
					),
					location.clone(),
				))
			}
		}
	}

	fn resolve_class_name(&self) -> Result<ClassName> {
		let name = self.cur_str();
		match self.glyphclasses_.resolve(&name) {
			None => Err(Error::feature_lib(
				format!("Unknown glyph class @{name}"),
				self.cur_loc(),
			)),
			Some(GlyphClassEntry::MarkClass(mc)) => Ok(ClassName::MarkClassName(MarkClassName {
				mark_class: mc.clone(),
				location: self.cur_loc(),
			})),
			Some(GlyphClassEntry::GlyphClass(gc)) => {
				Ok(ClassName::GlyphClassName(GlyphClassName {
					glyphclass: gc.clone(),
					location: self.cur_loc(),
				}))
			}
		}
	}

	/// Parses a glyph class, either named or anonymous, or (if `accept_glyphname`)
	/// a glyph name. If `accept_null`, also accepts the special NULL glyph
	/// (`parse_glyphclass_`).
	fn parse_glyphclass_(
		&mut self,
		accept_glyphname: bool,
		accept_null: bool,
	) -> Result<GlyphContainer> {
		if accept_glyphname
			&& matches!(
				self.next_token_type_,
				Some(TokenType::Name | TokenType::Cid)
			) {
			if accept_null && self.next_token_.is("NULL") {
				// If you want a glyph called NULL, you should escape it.
				self.advance_lexer_(false)?;
				return Ok(GlyphContainer::NullGlyph(NullGlyph {
					location: self.cur_loc(),
				}));
			}
			let glyph = self.expect_glyph_()?;
			self.check_glyph_name_in_glyph_set(&[&glyph]);
			return Ok(GlyphContainer::GlyphName(GlyphName {
				glyph,
				location: self.cur_loc(),
			}));
		}
		if self.next_token_type_ == Some(TokenType::Glyphclass) {
			self.advance_lexer_(false)?;
			return Ok(match self.resolve_class_name()? {
				ClassName::MarkClassName(m) => GlyphContainer::MarkClassName(m),
				ClassName::GlyphClassName(g) => GlyphContainer::GlyphClassName(g),
			});
		}

		self.expect_symbol_("[")?;
		let location = self.cur_loc();
		let mut glyphs = GlyphClass {
			location,
			..Default::default()
		};
		while !self.next_token_.is("]") {
			if self.next_token_type_ == Some(TokenType::Name) {
				let glyph = self.expect_glyph_()?;
				let location = self.cur_loc();
				if glyph.contains('-')
					&& !self.glyph_names_.is_empty()
					&& !self.glyph_names_.contains(&glyph)
				{
					let (start, limit) = self.split_glyph_range_(&glyph, &location)?;
					self.check_glyph_name_in_glyph_set(&[&start, &limit]);
					let range = self.make_glyph_range_(&location, &start, &limit)?;
					glyphs.add_range(&start, &limit, range);
				} else if self.next_token_.is("-") {
					let start = glyph;
					self.expect_symbol_("-")?;
					let limit = self.expect_glyph_()?;
					self.check_glyph_name_in_glyph_set(&[&start, &limit]);
					let range = self.make_glyph_range_(&location, &start, &limit)?;
					glyphs.add_range(&start, &limit, range);
				} else {
					if glyph.contains('-') && self.glyph_names_.is_empty() {
						log::warn!(
							"{}",
							Error::feature_lib(
								format!(
									"Ambiguous glyph name that looks like a range: {}",
									fontoxide_misc::py::str_repr(&glyph)
								),
								location,
							)
						);
					}
					self.check_glyph_name_in_glyph_set(&[&glyph]);
					glyphs.append(glyph);
				}
			} else if self.next_token_type_ == Some(TokenType::Cid) {
				let glyph = self.expect_glyph_()?;
				if self.next_token_.is("-") {
					let range_location = self.cur_loc();
					let range_start = match self.cur_token_ {
						Token::Int(i) => i,
						_ => 0,
					};
					self.expect_symbol_("-")?;
					let range_end = self.expect_cid_()?;
					self.check_glyph_name_in_glyph_set(&[
						&format!("cid{range_start:05}"),
						&format!("cid{range_end:05}"),
					]);
					let range = Self::make_cid_range_(&range_location, range_start, range_end)?;
					glyphs.add_cid_range(range_start, range_end, range);
				} else {
					self.check_glyph_name_in_glyph_set(&[&glyph]);
					glyphs.append(glyph);
				}
			} else if self.next_token_type_ == Some(TokenType::Glyphclass) {
				self.advance_lexer_(false)?;
				let gc = self.resolve_class_name()?;
				glyphs.add_class(gc);
			} else {
				return Err(Error::feature_lib(
					format!(
						"Expected glyph name, glyph range, or glyph class reference, found {}",
						self.next_token_.py_repr()
					),
					self.next_token_location_.clone(),
				));
			}
		}
		self.expect_symbol_("]")?;
		Ok(GlyphContainer::GlyphClass(glyphs))
	}

	/// Parses a glyph pattern, including lookups and context, e.g.
	/// `a b c' lookup ChangeC d e` (`parse_glyph_pattern_`).
	fn parse_glyph_pattern_(&mut self, vertical: bool) -> Result<GlyphPattern> {
		let mut prefix = Vec::new();
		let mut glyphs = Vec::new();
		let mut lookups: ChainLookups = Vec::new();
		let mut values: Vec<Option<ValueRecord>> = Vec::new();
		let mut suffix = Vec::new();
		let mut has_marks = false;
		while !["by", "from", ";", ","]
			.iter()
			.any(|t| self.next_token_.is(t))
		{
			let gc = self.parse_glyphclass_(true, false)?;
			let mut marked = false;
			if self.next_token_.is("'") {
				self.expect_symbol_("'")?;
				has_marks = true;
				marked = true;
			}
			if marked {
				if !suffix.is_empty() {
					// makeotf also reports this as an error, while FontForge
					// silently inserts ' in all the intervening glyphs.
					// https://github.com/fonttools/fonttools/pull/1096
					return Err(Error::feature_lib(
						"Unsupported contextual target sequence: at most \
						 one run of marked (') glyph/class names allowed",
						self.cur_loc(),
					));
				}
				glyphs.push(gc);
			} else if !glyphs.is_empty() {
				suffix.push(gc);
			} else {
				prefix.push(gc);
			}

			if self.is_next_value_() {
				values.push(Some(self.parse_valuerecord_(vertical)?));
			} else {
				values.push(None);
			}

			let mut lookuplist: Option<Vec<Rc<LookupBlock>>> = None;
			while self.next_token_.is("lookup") {
				self.expect_keyword_("lookup")?;
				if !marked {
					return Err(Error::feature_lib(
						"Lookups can only follow marked glyphs",
						self.cur_loc(),
					));
				}
				let lookup_name = self.expect_name_()?;
				let Some(lookup) = self.lookups_.resolve(&lookup_name) else {
					return Err(Error::feature_lib(
						format!("Unknown lookup \"{lookup_name}\""),
						self.cur_loc(),
					));
				};
				lookuplist.get_or_insert_with(Vec::new).push(lookup.clone());
			}
			if marked {
				lookups.push(lookuplist);
			}
		}

		if glyphs.is_empty() && suffix.is_empty() {
			// eg., "sub f f i by"
			if !lookups.is_empty() {
				return Err(Error::Assertion("lookups == []".to_owned()));
			}
			let n = prefix.len();
			return Ok((
				Vec::new(),
				prefix,
				vec![None; n],
				values,
				Vec::new(),
				has_marks,
			));
		}
		let positioning_error = || {
			Error::feature_lib(
				"Positioning values are allowed only in the marked glyph \
				 sequence, or after the final glyph node when only one glyph \
				 node is marked.",
				self.cur_loc(),
			)
		};
		if any_values(slice(&values, 0, prefix.len())) {
			return Err(Error::feature_lib(
				"Positioning cannot be applied in the bactrack glyph sequence, \
				 before the marked glyph sequence.",
				self.cur_loc(),
			));
		}
		let marked_start = prefix.len();
		let marked_end = prefix.len() + glyphs.len();
		if any_values(slice(&values, marked_start, marked_end)) {
			if any_values(slice(&values, marked_end, values.len())) {
				return Err(positioning_error());
			}
			values = slice(&values, marked_start, marked_end).to_vec();
		} else if values
			.last()
			.is_some_and(|v| v.as_ref().is_some_and(ValueRecord::is_truthy))
		{
			if glyphs.len() > 1 || any_values(&values[..values.len() - 1]) {
				return Err(positioning_error());
			}
			values = values.split_off(values.len() - 1);
		} else if any_values(&values) {
			return Err(positioning_error());
		}
		Ok((prefix, glyphs, lookups, values, suffix, has_marks))
	}

	/// `parse_ignore_glyph_pattern_(sub)`.
	fn parse_ignore_glyph_pattern_(&mut self, sub: &str) -> Result<ChainContext> {
		let location = self.cur_loc();
		let (prefix, mut glyphs, lookups, _values, mut suffix, has_marks) =
			self.parse_glyph_pattern_(false)?;
		if any_lookups(&lookups) {
			return Err(Error::feature_lib(
				format!("No lookups can be specified for \"ignore {sub}\""),
				location,
			));
		}
		if !has_marks {
			let error = Error::feature_lib(
				format!("Ambiguous \"ignore {sub}\", there should be least one marked glyph"),
				location,
			);
			log::warn!("{error}");
			let rest = if glyphs.is_empty() {
				Vec::new()
			} else {
				glyphs.split_off(1)
			};
			suffix = rest;
		}
		Ok((prefix, glyphs, suffix))
	}

	/// `parse_ignore_context_(sub)`.
	fn parse_ignore_context_(&mut self, sub: &str) -> Result<Vec<ChainContext>> {
		let mut chain_context = vec![self.parse_ignore_glyph_pattern_(sub)?];
		while self.next_token_.is(",") {
			self.expect_symbol_(",")?;
			chain_context.push(self.parse_ignore_glyph_pattern_(sub)?);
		}
		self.expect_symbol_(";")?;
		Ok(chain_context)
	}

	/// Parses an ignore sub/pos rule (`parse_ignore_`).
	fn parse_ignore_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		self.advance_lexer_(false)?;
		if self.cur_token_.is("substitute") || self.cur_token_.is("sub") {
			let chain_contexts = self.parse_ignore_context_("sub")?;
			return Ok(Statement::IgnoreSubstStatement(IgnoreSubstStatement {
				chain_contexts,
				location,
			}));
		}
		if self.cur_token_.is("position") || self.cur_token_.is("pos") {
			let chain_contexts = self.parse_ignore_context_("pos")?;
			return Ok(Statement::IgnorePosStatement(IgnorePosStatement {
				chain_contexts,
				location,
			}));
		}
		Err(Error::feature_lib(
			"Expected \"substitute\" or \"position\"",
			self.cur_loc(),
		))
	}

	/// `parse_include_()`.
	fn parse_include_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let filename = self.expect_filename_()?;
		Ok(Statement::IncludeStatement(IncludeStatement {
			filename,
			location,
		}))
	}

	/// `parse_language_()`.
	fn parse_language_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let mut languages = vec![self.expect_language_tag_()?];
		while self.next_token_type_ == Some(TokenType::Name)
			&& !["exclude_dflt", "include_dflt", "required"]
				.iter()
				.any(|t| self.next_token_.is(t))
			&& !Self::STATEMENT_KEYWORDS
				.iter()
				.any(|t| self.next_token_.is(t))
		{
			languages.push(self.expect_language_tag_()?);
		}
		if languages.len() > 1 && languages.iter().any(|l| l == "dflt") {
			return Err(Error::feature_lib(
				"\"dflt\" must be the only tag in a language statement",
				location,
			));
		}
		let mut include_default = true;
		let mut required = false;
		if self.next_token_.is("exclude_dflt") || self.next_token_.is("include_dflt") {
			include_default = self.expect_name_()? == "include_dflt";
		}
		if self.next_token_.is("required") {
			self.expect_keyword_("required")?;
			required = true;
		}
		self.expect_symbol_(";")?;
		Ok(Statement::LanguageStatement(LanguageStatement::new(
			languages,
			include_default,
			required,
			location,
		)?))
	}

	/// `parse_ligatureCaretByIndex_()`.
	fn parse_ligature_caret_by_index_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let glyphs = self.parse_glyphclass_(true, false)?;
		let mut carets = vec![self.expect_number_()?];
		while !self.next_token_.is(";") {
			carets.push(self.expect_number_()?);
		}
		self.expect_symbol_(";")?;
		Ok(Statement::LigatureCaretByIndexStatement(
			LigatureCaretByIndexStatement {
				glyphs,
				carets,
				location,
			},
		))
	}

	/// `parse_ligatureCaretByPos_()`.
	fn parse_ligature_caret_by_pos_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let glyphs = self.parse_glyphclass_(true, false)?;
		let mut carets = vec![self.expect_number_variable_()?];
		while !self.next_token_.is(";") {
			carets.push(self.expect_number_variable_()?);
		}
		self.expect_symbol_(";")?;
		Ok(Statement::LigatureCaretByPosStatement(
			LigatureCaretByPosStatement {
				glyphs,
				carets,
				location,
			},
		))
	}

	/// Parses a `lookup`: either a lookup block, or a lookup reference inside a
	/// feature (`parse_lookup_`).
	fn parse_lookup_(&mut self, vertical: bool) -> Result<Statement> {
		let location = self.cur_loc();
		let name = self.expect_name_()?;

		if self.next_token_.is(";") {
			let Some(lookup) = self.lookups_.resolve(&name).cloned() else {
				return Err(Error::feature_lib(
					format!("Unknown lookup \"{name}\""),
					self.cur_loc(),
				));
			};
			self.expect_symbol_(";")?;
			return Ok(Statement::LookupReferenceStatement(
				LookupReferenceStatement { lookup, location },
			));
		}

		let mut use_extension = false;
		if self.next_token_.is("useExtension") {
			self.expect_keyword_("useExtension")?;
			use_extension = true;
		}

		let mut block = LookupBlock {
			name: name.clone(),
			use_extension,
			statements: Vec::new(),
			location,
		};
		self.parse_block_(&mut block.statements, &name, vertical, None, false, None)?;
		let block = Rc::new(block);
		self.lookups_.define(name, block.clone());
		Ok(Statement::LookupBlock(block))
	}

	/// Parses a `lookupflag` statement, either specified by number or in words
	/// (`parse_lookupflag_`).
	fn parse_lookupflag_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();

		// format B: "lookupflag 6;"
		if self.next_token_type_ == Some(TokenType::Number) {
			let value = self.expect_number_()?;
			self.expect_symbol_(";")?;
			return Ok(Statement::LookupFlagStatement(LookupFlagStatement {
				value,
				location,
				..Default::default()
			}));
		}

		// format A: "lookupflag RightToLeft MarkAttachmentType @M;"
		let mut value_seen = false;
		let mut value = 0;
		let mut mark_attachment = None;
		let mut mark_filtering_set = None;
		let flags = [
			("RightToLeft", 1),
			("IgnoreBaseGlyphs", 2),
			("IgnoreLigatures", 4),
			("IgnoreMarks", 8),
		];
		let mut seen: Vec<Token> = Vec::new();
		while !self.next_token_.is(";") {
			if seen.contains(&self.next_token_) {
				return Err(Error::feature_lib(
					format!("{} can be specified only once", self.next_token_.py_str()),
					self.next_token_location_.clone(),
				));
			}
			seen.push(self.next_token_.clone());
			if self.next_token_.is("MarkAttachmentType") {
				self.expect_keyword_("MarkAttachmentType")?;
				mark_attachment = Some(self.parse_glyphclass_(false, false)?);
			} else if self.next_token_.is("UseMarkFilteringSet") {
				self.expect_keyword_("UseMarkFilteringSet")?;
				mark_filtering_set = Some(self.parse_glyphclass_(false, false)?);
			} else if let Some(&(_, bit)) = flags.iter().find(|(f, _)| self.next_token_.is(f)) {
				value_seen = true;
				self.expect_name_()?;
				value |= bit;
			} else {
				return Err(Error::feature_lib(
					format!(
						"\"{}\" is not a recognized lookupflag",
						self.next_token_.py_str()
					),
					self.next_token_location_.clone(),
				));
			}
		}
		self.expect_symbol_(";")?;

		if !value_seen && mark_attachment.is_none() && mark_filtering_set.is_none() {
			return Err(Error::feature_lib(
				"lookupflag must have a value",
				self.next_token_location_.clone(),
			));
		}

		Ok(Statement::LookupFlagStatement(LookupFlagStatement {
			value,
			mark_attachment,
			mark_filtering_set,
			location,
		}))
	}

	/// `parse_markClass_()`.
	fn parse_mark_class_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let glyphs = self.parse_glyphclass_(true, false)?;
		if glyphs.glyph_set().is_empty() {
			return Err(Error::feature_lib(
				"Empty glyph class in mark class definition",
				location,
			));
		}
		let anchor = self.parse_anchor_()?;
		let name = self.expect_class_name_()?;
		self.expect_symbol_(";")?;
		let mark_class = match self.doc_.mark_classes.get(&name) {
			Some(mc) => mc.clone(),
			None => {
				let mc = MarkClass::new(name.clone());
				self.doc_.mark_classes.insert(name.clone(), mc.clone());
				self.glyphclasses_
					.define(name, GlyphClassEntry::MarkClass(mc.clone()));
				mc
			}
		};
		let Some(anchor) = anchor else {
			return Err(Error::Assertion(
				"isinstance(anchor, Anchor) and isinstance(glyphs, Expression)".to_owned(),
			));
		};
		let mcdef = Rc::new(MarkClassDefinition::new(
			&mark_class,
			anchor,
			glyphs,
			location,
		));
		MarkClass::add_definition(&mark_class, mcdef.clone())?;
		Ok(Statement::MarkClassDefinition(mcdef))
	}

	/// `parse_position_(enumerated, vertical)`.
	fn parse_position_(&mut self, enumerated: bool, vertical: bool) -> Result<Statement> {
		if !(self.cur_token_.is("position") || self.cur_token_.is("pos")) {
			return Err(Error::Assertion(
				"self.cur_token_ in {\"position\", \"pos\"}".to_owned(),
			));
		}
		if self.next_token_.is("cursive") {
			// GPOS type 3
			return self.parse_position_cursive_(enumerated, vertical);
		} else if self.next_token_.is("base") {
			// GPOS type 4
			return self.parse_position_base_(enumerated, vertical);
		} else if self.next_token_.is("ligature") {
			// GPOS type 5
			return self.parse_position_ligature_(enumerated, vertical);
		} else if self.next_token_.is("mark") {
			// GPOS type 6
			return self.parse_position_mark_(enumerated, vertical);
		}

		let location = self.cur_loc();
		let (prefix, glyphs, lookups, mut values, suffix, has_marks) =
			self.parse_glyph_pattern_(vertical)?;
		self.expect_symbol_(";")?;

		if any_lookups(&lookups) {
			// GPOS type 8: Chaining contextual positioning; explicit lookups
			if any_values(&values) {
				return Err(Error::feature_lib(
					"If \"lookup\" is present, no values must be specified",
					location,
				));
			}
			return Ok(Statement::ChainContextPosStatement(
				ChainContextPosStatement {
					prefix,
					glyphs,
					suffix,
					lookups,
					location,
				},
			));
		}

		// Pair positioning, format A: "pos V 10 A -10;"
		// Pair positioning, format B: "pos V A -20;"
		if prefix.is_empty() && suffix.is_empty() && glyphs.len() == 2 && !has_marks {
			if values.first().is_some_and(Option::is_none) {
				// Format B: "pos V A -20;"
				values.reverse();
			}
			let mut values = values.into_iter();
			let mut glyphs = glyphs.into_iter();
			let index_error = || Error::Value("list index out of range".to_owned());
			let glyphs1 = glyphs.next().ok_or_else(index_error)?;
			let valuerecord1 = values.next().ok_or_else(index_error)?;
			let glyphs2 = glyphs.next().ok_or_else(index_error)?;
			let valuerecord2 = values.next().ok_or_else(index_error)?;
			return Ok(Statement::PairPosStatement(PairPosStatement {
				glyphs1,
				valuerecord1,
				glyphs2,
				valuerecord2,
				enumerated,
				location,
			}));
		}

		if enumerated {
			return Err(Error::feature_lib(
				"\"enumerate\" is only allowed with pair positionings",
				location,
			));
		}
		Ok(Statement::SinglePosStatement(SinglePosStatement {
			pos: glyphs.into_iter().zip(values).collect(),
			prefix,
			suffix,
			force_chain: has_marks,
			location,
		}))
	}

	fn check_not_enumerated(enumerated: bool, what: &str, location: &Location) -> Result<()> {
		if enumerated {
			return Err(Error::feature_lib(
				format!("\"enumerate\" is not allowed with {what}"),
				location.clone(),
			));
		}
		Ok(())
	}

	/// `parse_position_cursive_()`.
	fn parse_position_cursive_(&mut self, enumerated: bool, _vertical: bool) -> Result<Statement> {
		let location = self.cur_loc();
		self.expect_keyword_("cursive")?;
		Self::check_not_enumerated(enumerated, "cursive attachment positioning", &location)?;
		let glyphclass = self.parse_glyphclass_(true, false)?;
		let entry_anchor = self.parse_anchor_()?;
		let exit_anchor = self.parse_anchor_()?;
		self.expect_symbol_(";")?;
		Ok(Statement::CursivePosStatement(CursivePosStatement {
			glyphclass,
			entry_anchor,
			exit_anchor,
			location,
		}))
	}

	/// `parse_position_base_()`.
	fn parse_position_base_(&mut self, enumerated: bool, _vertical: bool) -> Result<Statement> {
		let location = self.cur_loc();
		self.expect_keyword_("base")?;
		Self::check_not_enumerated(enumerated, "mark-to-base attachment positioning", &location)?;
		let base = self.parse_glyphclass_(true, false)?;
		let marks = self.parse_anchor_marks_()?;
		self.expect_symbol_(";")?;
		Ok(Statement::MarkBasePosStatement(MarkBasePosStatement {
			base,
			marks,
			location,
		}))
	}

	/// `parse_position_ligature_()`.
	fn parse_position_ligature_(&mut self, enumerated: bool, _vertical: bool) -> Result<Statement> {
		let location = self.cur_loc();
		self.expect_keyword_("ligature")?;
		Self::check_not_enumerated(
			enumerated,
			"mark-to-ligature attachment positioning",
			&location,
		)?;
		let ligatures = self.parse_glyphclass_(true, false)?;
		let mut marks = vec![self.parse_anchor_marks_()?];
		while self.next_token_.is("ligComponent") {
			self.expect_keyword_("ligComponent")?;
			marks.push(self.parse_anchor_marks_()?);
		}
		self.expect_symbol_(";")?;
		Ok(Statement::MarkLigPosStatement(MarkLigPosStatement {
			ligatures,
			marks,
			location,
		}))
	}

	/// `parse_position_mark_()`.
	fn parse_position_mark_(&mut self, enumerated: bool, _vertical: bool) -> Result<Statement> {
		let location = self.cur_loc();
		self.expect_keyword_("mark")?;
		Self::check_not_enumerated(enumerated, "mark-to-mark attachment positioning", &location)?;
		let base_marks = self.parse_glyphclass_(true, false)?;
		let marks = self.parse_anchor_marks_()?;
		self.expect_symbol_(";")?;
		Ok(Statement::MarkMarkPosStatement(MarkMarkPosStatement {
			base_marks,
			marks,
			location,
		}))
	}

	/// `parse_script_()`.
	fn parse_script_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let script = self.expect_script_tag_()?;
		self.expect_symbol_(";")?;
		Ok(Statement::ScriptStatement(ScriptStatement {
			script,
			location,
		}))
	}

	/// `parse_substitute_()`.
	fn parse_substitute_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let reverse = self.cur_token_.is("reversesub") || self.cur_token_.is("rsub");
		let (old_prefix, old, lookups, values, old_suffix, has_marks) =
			self.parse_glyph_pattern_(false)?;
		if any_values(&values) {
			return Err(Error::feature_lib(
				"Substitution statements cannot contain values",
				location,
			));
		}
		let mut new = Vec::new();
		let keyword;
		if self.next_token_.is("by") {
			keyword = Some("by");
			self.expect_keyword_("by")?;
			while !self.next_token_.is(";") {
				let gc = self.parse_glyphclass_(true, true)?;
				new.push(gc);
			}
		} else if self.next_token_.is("from") {
			keyword = Some("from");
			self.expect_keyword_("from")?;
			new = vec![self.parse_glyphclass_(false, false)?];
		} else {
			keyword = None;
		}
		self.expect_symbol_(";")?;
		if new.is_empty() && !any_lookups(&lookups) {
			return Err(Error::feature_lib(
				"Expected \"by\", \"from\" or explicit lookup references",
				self.cur_loc(),
			));
		}

		// GSUB lookup type 3: Alternate substitution.
		// Format: "substitute a from [a.1 a.2 a.3];"
		if keyword == Some("from") {
			if reverse {
				return Err(Error::feature_lib(
					"Reverse chaining substitutions do not support \"from\"",
					location,
				));
			}
			if old.len() != 1 || old[0].glyph_set().len() != 1 {
				return Err(Error::feature_lib(
					"Expected a single glyph before \"from\"",
					location,
				));
			}
			if new.len() != 1 {
				return Err(Error::feature_lib(
					"Expected a single glyphclass after \"from\"",
					location,
				));
			}
			let mut old = old;
			let mut new = new;
			return Ok(Statement::AlternateSubstStatement(
				AlternateSubstStatement {
					prefix: old_prefix,
					glyph: old.swap_remove(0),
					suffix: old_suffix,
					replacement: new.swap_remove(0),
					location,
				},
			));
		}

		let num_lookups = lookups.iter().filter(|l| l.is_some()).count();

		let mut is_deletion = false;
		if new.len() == 1 && matches!(new[0], GlyphContainer::NullGlyph(_)) {
			if reverse {
				return Err(Error::feature_lib(
					"Reverse chaining substitutions do not support glyph deletion",
					location,
				));
			}
			new = Vec::new(); // Deletion
			is_deletion = true;
		}

		// GSUB lookup type 1: Single substitution.
		// Format A: "substitute a by a.sc;"
		// Format B: "substitute [one.fitted one.oldstyle] by one;"
		// Format C: "substitute [a-d] by [A.sc-D.sc];"
		if !reverse && old.len() == 1 && new.len() == 1 && num_lookups == 0 {
			let glyphs = old[0].glyph_set().len();
			let mut replacements = new[0].glyph_set().len();
			if replacements == 1 {
				replacements = glyphs;
			}
			if glyphs != replacements {
				return Err(Error::feature_lib(
					format!(
						"Expected a glyph class with {glyphs} elements after \"by\", \
						 but found a glyph class with {replacements} elements"
					),
					location,
				));
			}
			return Ok(Statement::SingleSubstStatement(SingleSubstStatement {
				prefix: old_prefix,
				suffix: old_suffix,
				force_chain: has_marks,
				glyphs: old,
				replacements: new,
				location,
			}));
		}

		// Glyph deletion, built as GSUB lookup type 2: Multiple substitution
		// with empty replacement.
		if is_deletion && old.len() == 1 && num_lookups == 0 {
			let mut old = old;
			return Ok(Statement::MultipleSubstStatement(MultipleSubstStatement {
				prefix: old_prefix,
				glyph: old.swap_remove(0),
				suffix: old_suffix,
				replacement: Vec::new(),
				force_chain: has_marks,
				location,
			}));
		}

		// GSUB lookup type 2: Multiple substitution.
		// Format: "substitute f_f_i by f f i;"
		//
		// GlyphsApp introduces two additional formats:
		// Format 1: "substitute [f_i f_l] by [f f] [i l];"
		// Format 2: "substitute [f_i f_l] by f [i l];"
		// http://handbook.glyphsapp.com/en/layout/multiple-substitution-with-classes/
		if !reverse && old.len() == 1 && new.len() > 1 && num_lookups == 0 {
			let count = old[0].glyph_set().len();
			for n in &new {
				let len = n.glyph_set().len();
				if len == 0 {
					return Err(Error::feature_lib("Empty class in replacement", location));
				}
				if len != 1 && len != count {
					return Err(Error::feature_lib(
						format!(
							"Expected a glyph class with 1 or {count} elements after \"by\", \
							 but found a glyph class with {len} elements"
						),
						location,
					));
				}
			}
			let mut old = old;
			return Ok(Statement::MultipleSubstStatement(MultipleSubstStatement {
				prefix: old_prefix,
				glyph: old.swap_remove(0),
				suffix: old_suffix,
				replacement: new,
				force_chain: has_marks,
				location,
			}));
		}

		// GSUB lookup type 4: Ligature substitution.
		// Format: "substitute f f i by f_f_i;"
		if !reverse
			&& old.len() > 1
			&& new.len() == 1
			&& new[0].glyph_set().len() == 1
			&& num_lookups == 0
		{
			let replacement = new[0].glyph_set()[0].clone();
			return Ok(Statement::LigatureSubstStatement(LigatureSubstStatement {
				prefix: old_prefix,
				glyphs: old,
				suffix: old_suffix,
				replacement,
				force_chain: has_marks,
				location,
			}));
		}

		// GSUB lookup type 8: Reverse chaining substitution.
		if reverse {
			if old.len() != 1 {
				return Err(Error::feature_lib(
					"In reverse chaining single substitutions, \
					 only a single glyph or glyph class can be replaced",
					location,
				));
			}
			if new.len() != 1 {
				return Err(Error::feature_lib(
					"In reverse chaining single substitutions, \
					 the replacement (after \"by\") must be a single glyph \
					 or glyph class",
					location,
				));
			}
			if num_lookups != 0 {
				return Err(Error::feature_lib(
					"Reverse chaining substitutions cannot call named lookups",
					location,
				));
			}
			let glyphs = old[0].glyph_set().len();
			let mut replacements = new[0].glyph_set().len();
			if replacements == 1 {
				replacements = glyphs;
			}
			if glyphs != replacements {
				return Err(Error::feature_lib(
					format!(
						"Expected a glyph class with {glyphs} elements after \"by\", \
						 but found a glyph class with {replacements} elements"
					),
					location,
				));
			}
			return Ok(Statement::ReverseChainSingleSubstStatement(
				ReverseChainSingleSubstStatement {
					old_prefix,
					old_suffix,
					glyphs: old,
					replacements: new,
					location,
				},
			));
		}

		if old.len() > 1 && new.len() > 1 {
			return Err(Error::feature_lib(
				"Direct substitution of multiple glyphs by multiple glyphs \
				 is not supported",
				location,
			));
		}

		// If there are remaining glyphs to parse, this is an invalid GSUB statement
		if !new.is_empty() || is_deletion {
			return Err(Error::feature_lib(
				"Invalid substitution statement",
				location,
			));
		}

		// GSUB lookup type 6: Chaining contextual substitution.
		Ok(Statement::ChainContextSubstStatement(
			ChainContextSubstStatement {
				prefix: old_prefix,
				glyphs: old,
				suffix: old_suffix,
				lookups,
				location,
			},
		))
	}

	/// `parse_subtable_()`.
	fn parse_subtable_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		self.expect_symbol_(";")?;
		Ok(Statement::SubtableStatement(SubtableStatement { location }))
	}

	/// Parses a `parameters` statement of the `size` feature
	/// (`parse_size_parameters_`).
	fn parse_size_parameters_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let design_size = self.expect_decipoint_()?;
		let subfamily_id = self.expect_number_()?;
		let mut range_start = 0.0;
		let mut range_end = 0.0;
		if matches!(
			self.next_token_type_,
			Some(TokenType::Number | TokenType::Float)
		) || subfamily_id != 0
		{
			range_start = self.expect_decipoint_()?;
			range_end = self.expect_decipoint_()?;
		}
		self.expect_symbol_(";")?;
		Ok(Statement::SizeParameters(SizeParameters {
			design_size,
			subfamily_id,
			range_start,
			range_end,
			location,
		}))
	}

	/// `parse_size_menuname_()`.
	fn parse_size_menuname_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let (platform_id, plat_enc_id, lang_id, string) = self.parse_name_()?;
		Ok(Statement::FeatureNameStatement(FeatureNameStatement {
			name_id: "size".to_owned(),
			platform_id,
			plat_enc_id,
			lang_id,
			string,
			location,
		}))
	}

	/// `parse_table_()`.
	fn parse_table_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let name = self.expect_tag_()?;
		let mut table = TableBlock {
			name: name.clone(),
			statements: Vec::new(),
			location: location.clone(),
		};
		self.expect_symbol_("{")?;
		let statements = &mut table.statements;
		match name.as_str() {
			"GDEF" => self.parse_table_gdef_(statements)?,
			"head" => self.parse_table_head_(statements)?,
			"hhea" => self.parse_table_hhea_(statements)?,
			"vhea" => self.parse_table_vhea_(statements)?,
			"name" => self.parse_table_name_(statements)?,
			"BASE" => self.parse_table_base_(statements)?,
			"OS/2" => self.parse_table_os_2_(statements)?,
			"STAT" => self.parse_table_stat_(statements)?,
			_ => {
				return Err(Error::feature_lib(
					format!(
						"\"table {}\" is not supported",
						fontoxide_misc::py::strip(&name)
					),
					location,
				));
			}
		}
		self.expect_symbol_("}")?;
		let end_tag = self.expect_tag_()?;
		if end_tag != name {
			return Err(Error::feature_lib(
				format!("Expected \"{}\"", fontoxide_misc::py::strip(&name)),
				self.cur_loc(),
			));
		}
		self.expect_symbol_(";")?;
		Ok(Statement::TableBlock(table))
	}

	fn table_loop_cond(&self) -> bool {
		!self.next_token_.is("}") || !self.cur_comments_.is_empty()
	}

	/// `parse_table_GDEF_(table)`.
	fn parse_table_gdef_(&mut self, statements: &mut Vec<Statement>) -> Result<()> {
		while self.table_loop_cond() {
			self.advance_lexer_(true)?;
			if self.cur_token_type_ == Some(TokenType::Comment) {
				statements.push(self.comment());
			} else if self.is_cur_keyword_("Attach") {
				statements.push(self.parse_attach_()?);
			} else if self.is_cur_keyword_("GlyphClassDef") {
				statements.push(self.parse_glyph_class_def_()?);
			} else if self.is_cur_keyword_("LigatureCaretByIndex") {
				statements.push(self.parse_ligature_caret_by_index_()?);
			} else if self.is_cur_keyword_("LigatureCaretByPos") {
				statements.push(self.parse_ligature_caret_by_pos_()?);
			} else if self.cur_token_.is(";") {
				continue;
			} else {
				return Err(Error::feature_lib(
					"Expected Attach, LigatureCaretByIndex, or LigatureCaretByPos",
					self.cur_loc(),
				));
			}
		}
		Ok(())
	}

	/// `parse_table_head_(table)`.
	fn parse_table_head_(&mut self, statements: &mut Vec<Statement>) -> Result<()> {
		while self.table_loop_cond() {
			self.advance_lexer_(true)?;
			if self.cur_token_type_ == Some(TokenType::Comment) {
				statements.push(self.comment());
			} else if self.is_cur_keyword_("FontRevision") {
				statements.push(self.parse_font_revision_()?);
			} else if self.cur_token_.is(";") {
				continue;
			} else {
				return Err(Error::feature_lib("Expected FontRevision", self.cur_loc()));
			}
		}
		Ok(())
	}

	/// `parse_table_hhea_(table)` and `parse_table_vhea_(table)`.
	fn parse_table_xhea_(
		&mut self,
		statements: &mut Vec<Statement>,
		fields: &[&str],
		vertical: bool,
		expected: &str,
	) -> Result<()> {
		while self.table_loop_cond() {
			self.advance_lexer_(true)?;
			if self.cur_token_type_ == Some(TokenType::Comment) {
				statements.push(self.comment());
			} else if self.cur_token_type_ == Some(TokenType::Name)
				&& fields.iter().any(|f| self.cur_token_.is(f))
			{
				let key = self.cur_str().to_lowercase();
				let value = self.expect_number_()?;
				let location = self.cur_loc();
				statements.push(if vertical {
					Statement::VheaField(VheaField {
						key,
						value,
						location,
					})
				} else {
					Statement::HheaField(HheaField {
						key,
						value,
						location,
					})
				});
				if !self.next_token_.is(";") {
					return Err(Error::feature_lib(
						"Incomplete statement",
						self.next_token_location_.clone(),
					));
				}
			} else if self.cur_token_.is(";") {
				continue;
			} else {
				return Err(Error::feature_lib(expected, self.cur_loc()));
			}
		}
		Ok(())
	}

	/// `parse_table_hhea_(table)`.
	fn parse_table_hhea_(&mut self, statements: &mut Vec<Statement>) -> Result<()> {
		self.parse_table_xhea_(
			statements,
			&["CaretOffset", "Ascender", "Descender", "LineGap"],
			false,
			"Expected CaretOffset, Ascender, Descender or LineGap",
		)
	}

	/// `parse_table_vhea_(table)`.
	fn parse_table_vhea_(&mut self, statements: &mut Vec<Statement>) -> Result<()> {
		self.parse_table_xhea_(
			statements,
			&["VertTypoAscender", "VertTypoDescender", "VertTypoLineGap"],
			true,
			"Expected VertTypoAscender, VertTypoDescender or VertTypoLineGap",
		)
	}

	/// `parse_table_name_(table)`.
	fn parse_table_name_(&mut self, statements: &mut Vec<Statement>) -> Result<()> {
		while self.table_loop_cond() {
			self.advance_lexer_(true)?;
			if self.cur_token_type_ == Some(TokenType::Comment) {
				statements.push(self.comment());
			} else if self.is_cur_keyword_("nameid") {
				statements.push(self.parse_nameid_()?);
			} else if self.cur_token_.is(";") {
				continue;
			} else {
				return Err(Error::feature_lib("Expected nameid", self.cur_loc()));
			}
		}
		Ok(())
	}

	/// The platform, encoding and language IDs of a name record, with defaults
	/// (shared by `parse_name_` and `parse_stat_name_`).
	fn parse_name_ids_(&mut self) -> Result<(i64, i64, i64, Location)> {
		let mut plat_enc_id = None;
		let mut lang_id = None;
		let platform_id;
		let location;
		if self.next_token_type_.is_some_and(TokenType::is_number) {
			platform_id = self.expect_any_number_()?;
			location = self.cur_loc();
			if platform_id != 1 && platform_id != 3 {
				return Err(Error::feature_lib("Expected platform id 1 or 3", location));
			}
			if self.next_token_type_.is_some_and(TokenType::is_number) {
				plat_enc_id = Some(self.expect_any_number_()?);
				lang_id = Some(self.expect_any_number_()?);
			}
		} else {
			platform_id = 3;
			location = self.cur_loc();
		}

		// `platEncID or default`: an explicit 0 is replaced too.
		let or = |v: Option<i64>, default: i64| v.filter(|&v| v != 0).unwrap_or(default);
		let (plat_enc_id, lang_id) = if platform_id == 1 {
			// Macintosh: Roman, English
			(or(plat_enc_id, 0), or(lang_id, 0))
		} else {
			// 3, Windows: Unicode, English
			(or(plat_enc_id, 1), or(lang_id, 0x0409))
		};
		Ok((platform_id, plat_enc_id, lang_id, location))
	}

	/// Parses a name record (`parse_name_`).
	fn parse_name_(&mut self) -> Result<NameParts> {
		let (platform_id, plat_enc_id, lang_id, location) = self.parse_name_ids_()?;
		let string = self.expect_string_()?;
		let string_location = self.cur_loc();
		self.expect_symbol_(";")?;

		let Some(encoding) = ast::name_encoding(platform_id, plat_enc_id, lang_id) else {
			return Err(Error::feature_lib("Unsupported encoding", location));
		};
		let unescaped = Self::unescape_string_(&string, encoding, string_location)?;
		Ok((platform_id, plat_enc_id, lang_id, unescaped))
	}

	/// `parse_stat_name_()`: like `parse_name_` but without the semicolon.
	fn parse_stat_name_(&mut self) -> Result<NameParts> {
		let (platform_id, plat_enc_id, lang_id, location) = self.parse_name_ids_()?;
		let string = self.expect_string_()?;
		let string_location = self.cur_loc();
		let Some(encoding) = ast::name_encoding(platform_id, plat_enc_id, lang_id) else {
			return Err(Error::feature_lib("Unsupported encoding", location));
		};
		let unescaped = Self::unescape_string_(&string, encoding, string_location)?;
		Ok((platform_id, plat_enc_id, lang_id, unescaped))
	}

	/// `parse_nameid_()`.
	fn parse_nameid_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let name_id = self.expect_any_number_()?;
		if name_id > 32767 {
			return Err(Error::feature_lib(
				"Name id value cannot be greater than 32767",
				self.cur_loc(),
			));
		}
		let (platform_id, plat_enc_id, lang_id, string) = self.parse_name_()?;
		Ok(Statement::NameRecord(NameRecord {
			name_id,
			platform_id,
			plat_enc_id,
			lang_id,
			string,
			location,
		}))
	}

	/// Unescapes a name string for an encoding (`unescape_string_`): `\XXXX`
	/// escapes of UTF-16 code units for `utf_16_be`, `\XX` escapes of bytes
	/// otherwise.
	pub fn unescape_string_(string: &str, encoding: &str, location: Location) -> Result<String> {
		Self::unescape_string_inner(string, encoding).map_err(|reason| {
			Error::feature_lib(
				format!("Name string is not valid {encoding}: {reason}"),
				location,
			)
		})
	}

	/// `_unescape_string`; the error is the `UnicodeError` reason.
	fn unescape_string_inner(string: &str, encoding: &str) -> std::result::Result<String, String> {
		let chars: Vec<char> = string.chars().collect();
		// The value of a `\` followed by `n` hexadecimal digits at `i`.
		let hex_at = |i: usize, n: usize| -> Option<u32> {
			if chars.get(i) != Some(&'\\') {
				return None;
			}
			let digits = chars.get(i + 1..i + 1 + n)?;
			if digits.iter().all(char::is_ascii_hexdigit) {
				u32::from_str_radix(&digits.iter().collect::<String>(), 16).ok()
			} else {
				None
			}
		};
		if encoding == "utf_16_be" {
			// `re.sub(r"\\[0-9a-fA-F]{4}", ...)` and then a round trip through
			// UTF-16 with surrogates passed through, so escaped surrogate pairs
			// become characters and unpaired surrogates are errors.
			let mut units: Vec<u16> = Vec::new();
			let mut i = 0;
			while i < chars.len() {
				if let Some(v) = hex_at(i, 4) {
					units.push(v as u16);
					i += 5;
				} else {
					let mut buf = [0u16; 2];
					units.extend_from_slice(chars[i].encode_utf16(&mut buf));
					i += 1;
				}
			}
			decode_utf16_strict(&units)
		} else {
			// Build the full byte sequence from literal characters plus \XX
			// escapes, then decode in one go. This is required for multibyte
			// encodings (e.g. x_mac_japanese_ttx) where bytes belonging to a
			// single character may straddle literal/escape boundaries.
			let mut out: Vec<u8> = Vec::new();
			let mut literal = String::new();
			let mut i = 0;
			let flush =
				|literal: &mut String, out: &mut Vec<u8>| -> std::result::Result<(), String> {
					if !literal.is_empty() {
						let bytes = codecs::encode(encoding, literal)
							.map_err(|_| codec_reason(encoding))?;
						out.extend(bytes);
						literal.clear();
					}
					Ok(())
				};
			while i < chars.len() {
				if let Some(v) = hex_at(i, 2) {
					flush(&mut literal, &mut out)?;
					out.push(v as u8);
					i += 3;
				} else {
					literal.push(chars[i]);
					i += 1;
				}
			}
			flush(&mut literal, &mut out)?;
			codecs::decode(encoding, &out).map_err(|_| codec_reason(encoding))
		}
	}

	/// The `BaseAxis` statement `find_previous` finds, if any (`find_previous`).
	fn find_previous_base_axis(statements: &mut [Statement]) -> Option<&mut BaseAxis> {
		for previous in statements.iter_mut().rev() {
			match previous {
				Statement::Comment(_) => continue,
				Statement::BaseAxis(b) => return Some(b),
				// If we find something that doesn't match what we're looking for,
				// and isn't a comment, fail
				_ => return None,
			}
		}
		// Out of statements to look at
		None
	}

	/// `parse_table_BASE_(table)`.
	fn parse_table_base_(&mut self, statements: &mut Vec<Statement>) -> Result<()> {
		let mut horiz_bases: Option<Vec<String>> = None;
		let mut vert_bases: Option<Vec<String>> = None;
		while self.table_loop_cond() {
			self.advance_lexer_(true)?;
			if self.cur_token_type_.is_none() {
				return Err(Error::feature_lib("Expected '}'", self.cur_loc()));
			}
			if self.cur_token_type_ == Some(TokenType::Comment) {
				statements.push(self.comment());
			} else if self.is_cur_keyword_("HorizAxis.BaseTagList") {
				horiz_bases = Some(self.parse_base_tag_list_()?);
			} else if self.is_cur_keyword_("HorizAxis.BaseScriptList") {
				let Some(bases) = horiz_bases.clone() else {
					return Err(Error::feature_lib(
						"BaseScriptList must be preceded by BaseTagList",
						self.cur_loc(),
					));
				};
				let scripts = self.parse_base_script_list_(&bases)?;
				statements.push(Statement::BaseAxis(BaseAxis::new(
					bases,
					scripts,
					false,
					None,
					self.cur_loc(),
				)));
			} else if self.is_cur_keyword_("VertAxis.BaseTagList") {
				vert_bases = Some(self.parse_base_tag_list_()?);
			} else if self.is_cur_keyword_("VertAxis.BaseScriptList") {
				let Some(bases) = vert_bases.clone() else {
					return Err(Error::feature_lib(
						"BaseScriptList must be preceded by BaseTagList",
						self.cur_loc(),
					));
				};
				let scripts = self.parse_base_script_list_(&bases)?;
				statements.push(Statement::BaseAxis(BaseAxis::new(
					bases,
					scripts,
					true,
					None,
					self.cur_loc(),
				)));
			} else if self.is_cur_keyword_("HorizAxis.MinMax")
				|| self.is_cur_keyword_("VertAxis.MinMax")
			{
				let vertical = self.is_cur_keyword_("VertAxis.MinMax");
				let loc = self.cur_loc();
				let Some(base_script_list) = Self::find_previous_base_axis(statements) else {
					return Err(Error::feature_lib(
						"MinMax must be preceded by BaseScriptList",
						loc,
					));
				};
				if base_script_list.vertical != vertical {
					return Err(Error::feature_lib(
						if vertical {
							"VertAxis.MinMax must be preceded by VertAxis statements"
						} else {
							"HorizAxis.MinMax must be preceded by HorizAxis statements"
						},
						loc,
					));
				}
				let minmax = self.parse_base_minmax_()?;
				if let Some(b) = Self::find_previous_base_axis(statements) {
					b.minmax.push(minmax);
				}
			} else if self.cur_token_.is(";") {
				continue;
			}
		}
		Ok(())
	}

	/// `parse_table_OS_2_(table)`.
	fn parse_table_os_2_(&mut self, statements: &mut Vec<Statement>) -> Result<()> {
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
		while self.table_loop_cond() {
			self.advance_lexer_(true)?;
			if self.cur_token_type_.is_none() {
				return Err(Error::feature_lib("Expected '}'", self.cur_loc()));
			}
			if self.cur_token_type_ == Some(TokenType::Comment) {
				statements.push(self.comment());
			} else if self.cur_token_type_ == Some(TokenType::Name) {
				let key = self.cur_str().to_lowercase();
				let mut value = OS2Value::None;
				if NUMBERS.iter().any(|n| self.cur_token_.is(n)) {
					value = OS2Value::Int(self.expect_number_()?);
				} else if self.is_cur_keyword_("Panose") {
					let mut v = Vec::new();
					for _ in 0..10 {
						v.push(self.expect_number_()?);
					}
					value = OS2Value::IntArray(v);
				} else if RANGES.iter().any(|n| self.cur_token_.is(n)) {
					let mut v = Vec::new();
					while !self.next_token_.is(";") {
						let number = self.expect_number_()?;
						if key == "unicoderange" && !(0..=127).contains(&number) {
							return Err(Error::feature_lib(
								"UnicodeRange bits must be between 0 and 127",
								self.cur_loc(),
							));
						}
						v.push(number);
					}
					value = OS2Value::IntArray(v);
				} else if self.is_cur_keyword_("Vendor") {
					let s = self.expect_string_()?;
					if !s.is_ascii() {
						return Err(Error::feature_lib(
							"Vendor ID must be ASCII",
							self.cur_loc(),
						));
					}
					value = OS2Value::Str(s);
				}
				statements.push(Statement::OS2Field(OS2Field {
					key,
					value,
					location: self.cur_loc(),
				}));
			} else if self.cur_token_.is(";") {
				continue;
			}
		}
		Ok(())
	}

	/// `parse_STAT_ElidedFallbackName()`.
	fn parse_stat_elided_fallback_name(&mut self) -> Result<Vec<STATNameStatement>> {
		self.expect_symbol_("{")?;
		let mut names = Vec::new();
		while self.table_loop_cond() {
			self.advance_lexer_(false)?;
			if self.is_cur_keyword_("name") {
				let (platform_id, plat_enc_id, lang_id, string) = self.parse_stat_name_()?;
				names.push(STATNameStatement {
					name_id: "stat".to_owned(),
					platform_id,
					plat_enc_id,
					lang_id,
					string,
					location: self.cur_loc(),
				});
			} else if !self.cur_token_.is(";") {
				return Err(Error::feature_lib(
					format!("Unexpected token {} in ElidedFallbackName", self.cur_str()),
					self.cur_loc(),
				));
			}
		}
		self.expect_symbol_("}")?;
		if names.is_empty() {
			return Err(Error::feature_lib("Expected \"name\"", self.cur_loc()));
		}
		Ok(names)
	}

	/// `parse_STAT_design_axis()`.
	fn parse_stat_design_axis(&mut self) -> Result<STATDesignAxisStatement> {
		let mut names = Vec::new();
		let axis_tag = self.expect_tag_()?;
		if !["ital", "opsz", "slnt", "wdth", "wght"].contains(&axis_tag.as_str())
			&& !py_isupper(&axis_tag)
		{
			log::warn!("Unregistered axis tag {axis_tag} should be uppercase.");
		}
		let axis_order = self.expect_number_()?;
		self.expect_symbol_("{")?;
		while self.table_loop_cond() {
			self.advance_lexer_(false)?;
			if self.cur_token_type_ == Some(TokenType::Comment) {
				continue;
			} else if self.is_cur_keyword_("name") {
				let location = self.cur_loc();
				let (platform_id, plat_enc_id, lang_id, string) = self.parse_stat_name_()?;
				names.push(STATNameStatement {
					name_id: "stat".to_owned(),
					platform_id,
					plat_enc_id,
					lang_id,
					string,
					location,
				});
			} else if self.cur_token_.is(";") {
				continue;
			} else {
				return Err(Error::feature_lib(
					format!("Expected \"name\", got {}", self.cur_str()),
					self.cur_loc(),
				));
			}
		}
		self.expect_symbol_("}")?;
		Ok(STATDesignAxisStatement {
			tag: axis_tag,
			axis_order,
			names,
			location: self.cur_loc(),
		})
	}

	/// `parse_STAT_axis_value_()`.
	fn parse_stat_axis_value_(&mut self) -> Result<STATAxisValueStatement> {
		self.expect_symbol_("{")?;
		let mut locations: Vec<AxisValueLocationStatement> = Vec::new();
		let mut names = Vec::new();
		let mut flags = 0;
		while self.table_loop_cond() {
			self.advance_lexer_(true)?;
			if self.cur_token_type_ == Some(TokenType::Comment) {
				continue;
			} else if self.is_cur_keyword_("name") {
				let location = self.cur_loc();
				let (platform_id, plat_enc_id, lang_id, string) = self.parse_stat_name_()?;
				names.push(STATNameStatement {
					name_id: "stat".to_owned(),
					platform_id,
					plat_enc_id,
					lang_id,
					string,
					location,
				});
			} else if self.is_cur_keyword_("location") {
				locations.push(self.parse_stat_location()?);
			} else if self.is_cur_keyword_("flag") {
				flags = self.expect_stat_flags()?;
			} else if self.cur_token_.is(";") {
				continue;
			} else {
				return Err(Error::feature_lib(
					format!("Unexpected token {} in AxisValue", self.cur_str()),
					self.cur_loc(),
				));
			}
		}
		self.expect_symbol_("}")?;
		if names.is_empty() {
			return Err(Error::feature_lib("Expected \"Axis Name\"", self.cur_loc()));
		}
		if locations.is_empty() {
			return Err(Error::feature_lib(
				"Expected \"Axis location\"",
				self.cur_loc(),
			));
		}
		if locations.len() > 1 {
			for location in &locations {
				if location.values.len() > 1 {
					return Err(Error::feature_lib(
						format!(
							"Only one value is allowed in a Format 4 Axis Value Record, but {} were found.",
							location.values.len()
						),
						self.cur_loc(),
					));
				}
			}
			let mut format4_tags: Vec<&str> = Vec::new();
			for location in &locations {
				let tag = location.tag.as_str();
				if format4_tags.contains(&tag) {
					return Err(Error::feature_lib(
						format!("Axis tag {tag} already defined."),
						self.cur_loc(),
					));
				}
				format4_tags.push(tag);
			}
		}

		Ok(STATAxisValueStatement {
			names,
			locations,
			flags,
			location: self.cur_loc(),
		})
	}

	/// `parse_STAT_location()`.
	fn parse_stat_location(&mut self) -> Result<AxisValueLocationStatement> {
		let mut values = Vec::new();
		let tag = self.expect_tag_()?;
		if fontoxide_misc::py::strip(&tag).chars().count() != 4 {
			return Err(Error::feature_lib(
				format!("Axis tag {} must be 4 characters", self.cur_str()),
				self.cur_loc(),
			));
		}

		while !self.next_token_.is(";") {
			if self.next_token_type_ == Some(TokenType::Float) {
				values.push(IntOrFloat::Float(self.expect_float_()?));
			} else if self.next_token_type_ == Some(TokenType::Number) {
				values.push(IntOrFloat::Int(self.expect_number_()?));
			} else {
				return Err(Error::feature_lib(
					format!(
						"Unexpected value \"{}\". Expected integer or float.",
						self.next_token_.py_str()
					),
					self.next_token_location_.clone(),
				));
			}
		}
		if !(1..=3).contains(&values.len()) {
			return Err(Error::feature_lib(
				format!(
					"Expected 1, 2 or 3 values for axis {}, but {} were found.",
					fontoxide_misc::py::strip(&tag),
					values.len()
				),
				self.next_token_location_.clone(),
			));
		}
		if let [nominal, min_val, max_val] = values[..]
			&& (nominal.as_f64() < min_val.as_f64() || nominal.as_f64() > max_val.as_f64())
		{
			return Err(Error::feature_lib(
				format!(
					"Default value {nominal} is outside of specified range {min_val}-{max_val}."
				),
				self.next_token_location_.clone(),
			));
		}
		Ok(AxisValueLocationStatement {
			tag,
			values,
			location: None,
		})
	}

	/// `parse_table_STAT_(table)`.
	fn parse_table_stat_(&mut self, statements: &mut Vec<Statement>) -> Result<()> {
		let mut design_axes: Vec<String> = Vec::new();
		while self.table_loop_cond() {
			self.advance_lexer_(true)?;
			if self.cur_token_type_.is_none() {
				return Err(Error::feature_lib("Expected '}'", self.cur_loc()));
			}
			if self.cur_token_type_ == Some(TokenType::Comment) {
				statements.push(self.comment());
			} else if self.cur_token_type_ == Some(TokenType::Name) {
				if self.is_cur_keyword_("ElidedFallbackName") {
					let names = self.parse_stat_elided_fallback_name()?;
					statements.push(Statement::ElidedFallbackName(ElidedFallbackName {
						names,
						location: None,
					}));
				} else if self.is_cur_keyword_("ElidedFallbackNameID") {
					let value = self.expect_number_()?;
					statements.push(Statement::ElidedFallbackNameID(ElidedFallbackNameID {
						value,
						location: None,
					}));
					self.expect_symbol_(";")?;
				} else if self.is_cur_keyword_("DesignAxis") {
					let design_axis = self.parse_stat_design_axis()?;
					design_axes.push(design_axis.tag.clone());
					statements.push(Statement::STATDesignAxisStatement(design_axis));
					self.expect_symbol_(";")?;
				} else if self.is_cur_keyword_("AxisValue") {
					let axis_value_record = self.parse_stat_axis_value_()?;
					for location in &axis_value_record.locations {
						if !design_axes.contains(&location.tag) {
							// Tag must be defined in a DesignAxis before it
							// can be referenced
							return Err(Error::feature_lib(
								format!("DesignAxis not defined for {}.", location.tag),
								self.cur_loc(),
							));
						}
					}
					statements.push(Statement::STATAxisValueStatement(axis_value_record));
					self.expect_symbol_(";")?;
				} else {
					return Err(Error::feature_lib(
						format!("Unexpected token {}", self.cur_str()),
						self.cur_loc(),
					));
				}
			} else if self.cur_token_.is(";") {
				continue;
			}
		}
		Ok(())
	}

	/// Parses BASE table entries (`parse_base_tag_list_`).
	fn parse_base_tag_list_(&mut self) -> Result<Vec<String>> {
		let mut bases = Vec::new();
		while !self.next_token_.is(";") {
			bases.push(self.expect_script_tag_()?);
		}
		self.expect_symbol_(";")?;
		Ok(bases)
	}

	/// `parse_base_script_list_(bases)`.
	fn parse_base_script_list_(&mut self, bases: &[String]) -> Result<Vec<BaseScriptRecord>> {
		let mut scripts = vec![self.parse_base_script_record_(bases)?];
		while self.next_token_.is(",") {
			self.expect_symbol_(",")?;
			scripts.push(self.parse_base_script_record_(bases)?);
		}
		self.expect_symbol_(";")?;
		Ok(scripts)
	}

	/// `parse_base_script_record_(bases)`.
	fn parse_base_script_record_(&mut self, bases: &[String]) -> Result<BaseScriptRecord> {
		let script_tag = self.expect_script_tag_()?;
		let base_tag = self.expect_script_tag_()?;
		if !bases.contains(&base_tag) {
			return Err(Error::feature_lib(
				format!(
					"Default baseline \"{}\" is not in BaseTagList",
					fontoxide_misc::py::strip(&base_tag)
				),
				self.cur_loc(),
			));
		}
		let mut coords = Vec::new();
		for _ in 0..bases.len() {
			coords.push(self.expect_number_()?);
		}
		Ok((script_tag, base_tag, coords))
	}

	/// `parse_base_minmax_()`.
	fn parse_base_minmax_(&mut self) -> Result<BaseMinMax> {
		let script_tag = self.expect_script_tag_()?;
		let language = self.expect_language_tag_()?;
		let min_coord = self.expect_number_()?;
		self.advance_lexer_(false)?;
		if !(self.cur_token_type_ == Some(TokenType::Symbol) && self.cur_token_.is(",")) {
			return Err(Error::feature_lib(
				"Expected a comma between min and max coordinates",
				self.cur_loc(),
			));
		}
		let max_coord = self.expect_number_()?;
		if self.next_token_.is(",") {
			// feature tag...
			return Err(Error::feature_lib(
				"Feature tags are not yet supported in BASE table",
				self.cur_loc(),
			));
		}
		Ok((script_tag, language, min_coord, max_coord))
	}

	/// `parse_device_()`.
	fn parse_device_(&mut self) -> Result<Option<Device>> {
		let mut result = None;
		self.expect_symbol_("<")?;
		self.expect_keyword_("device")?;
		if self.next_token_.is("NULL") {
			self.expect_keyword_("NULL")?;
		} else {
			let mut device = vec![(self.expect_number_()?, self.expect_number_()?)];
			while self.next_token_.is(",") {
				self.expect_symbol_(",")?;
				device.push((self.expect_number_()?, self.expect_number_()?));
			}
			result = Some(device);
		}
		self.expect_symbol_(">")?;
		Ok(result)
	}

	/// `is_next_value_()`.
	fn is_next_value_(&self) -> bool {
		self.next_token_type_ == Some(TokenType::Number)
			|| self.next_token_.is("<")
			|| self.next_token_.is("(")
	}

	/// `parse_valuerecord_(vertical)`.
	fn parse_valuerecord_(&mut self, vertical: bool) -> Result<ValueRecord> {
		if (self.next_token_type_ == Some(TokenType::Symbol) && self.next_token_.is("("))
			|| self.next_token_type_ == Some(TokenType::Number)
		{
			let number = self.expect_number_variable_()?;
			let location = self.cur_loc();
			let mut val = ValueRecord {
				vertical,
				location,
				..Default::default()
			};
			if vertical {
				val.y_advance = Some(number);
			} else {
				val.x_advance = Some(number);
			}
			return Ok(val);
		}
		self.expect_symbol_("<")?;
		let location = self.cur_loc();
		let (x_placement, y_placement, x_advance, y_advance);
		if self.next_token_type_ == Some(TokenType::Name) {
			let name = self.expect_name_()?;
			if name == "NULL" {
				self.expect_symbol_(">")?;
				return Ok(ValueRecord::default());
			}
			let Some(vrd) = self.valuerecords_.resolve(&name) else {
				return Err(Error::feature_lib(
					format!("Unknown valueRecordDef \"{name}\""),
					self.cur_loc(),
				));
			};
			let value = &vrd.value;
			x_placement = value.x_placement.clone();
			y_placement = value.y_placement.clone();
			x_advance = value.x_advance.clone();
			y_advance = value.y_advance.clone();
		} else {
			x_placement = Some(self.expect_number_variable_()?);
			y_placement = Some(self.expect_number_variable_()?);
			x_advance = Some(self.expect_number_variable_()?);
			y_advance = Some(self.expect_number_variable_()?);
		}

		let (x_pla_device, y_pla_device, x_adv_device, y_adv_device);
		if self.next_token_.is("<") {
			x_pla_device = self.parse_device_()?;
			y_pla_device = self.parse_device_()?;
			x_adv_device = self.parse_device_()?;
			y_adv_device = self.parse_device_()?;
			let mut all_deltas: Vec<i64> =
				[&x_pla_device, &y_pla_device, &x_adv_device, &y_adv_device]
					.into_iter()
					.flatten()
					.flatten()
					.map(|&(_, delta)| delta)
					.collect();
			all_deltas.sort_unstable();
			if let (Some(&first), Some(&last)) = (all_deltas.first(), all_deltas.last())
				&& (first < -128 || last > 127)
			{
				return Err(Error::feature_lib(
					"Device value out of valid range (-128..127)",
					self.cur_loc(),
				));
			}
		} else {
			x_pla_device = None;
			y_pla_device = None;
			x_adv_device = None;
			y_adv_device = None;
		}

		self.expect_symbol_(">")?;
		Ok(ValueRecord {
			x_placement,
			y_placement,
			x_advance,
			y_advance,
			x_pla_device,
			y_pla_device,
			x_adv_device,
			y_adv_device,
			vertical,
			location,
		})
	}

	/// Parses a named value record definition (`parse_valuerecord_definition_`).
	fn parse_valuerecord_definition_(&mut self, vertical: bool) -> Result<ValueRecordDefinition> {
		let location = self.cur_loc();
		let value = self.parse_valuerecord_(vertical)?;
		let name = self.expect_name_()?;
		self.expect_symbol_(";")?;
		let vrd = ValueRecordDefinition {
			name: name.clone(),
			value,
			location,
		};
		self.valuerecords_.define(name, vrd.clone());
		Ok(vrd)
	}

	/// `parse_languagesystem_()`.
	fn parse_languagesystem_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let script = self.expect_script_tag_()?;
		let language = self.expect_language_tag_()?;
		self.expect_symbol_(";")?;
		Ok(Statement::LanguageSystemStatement(
			LanguageSystemStatement {
				script,
				language,
				location,
			},
		))
	}

	/// `parse_feature_block_(variation)`.
	fn parse_feature_block_(&mut self, variation: bool) -> Result<Statement> {
		let location = self.cur_loc();
		let tag = self.expect_tag_()?;
		let vertical = ["vkrn", "vpal", "vhal", "valt"].contains(&tag.as_str());

		let mut stylisticset = None;
		let mut cv_feature = None;
		let mut size_feature = false;
		if is_numbered_tag(&tag, "ss", SS_FEATURE_TAGS) {
			stylisticset = Some(tag.clone());
		} else if is_numbered_tag(&tag, "cv", CV_FEATURE_TAGS) {
			cv_feature = Some(tag.clone());
		} else if tag == "size" {
			size_feature = true;
		}

		let conditionset = if variation {
			Some(self.expect_name_()?)
		} else {
			None
		};

		let mut use_extension = false;
		if self.next_token_.is("useExtension") {
			self.expect_keyword_("useExtension")?;
			use_extension = true;
		}

		let mut statements = Vec::new();
		self.parse_block_(
			&mut statements,
			&tag,
			vertical,
			stylisticset.as_deref(),
			size_feature,
			cv_feature.as_deref(),
		)?;
		Ok(match conditionset {
			Some(conditionset) => Statement::VariationBlock(VariationBlock {
				name: tag,
				conditionset,
				use_extension,
				statements,
				location,
			}),
			None => Statement::FeatureBlock(FeatureBlock {
				name: tag,
				use_extension,
				statements,
				location,
			}),
		})
	}

	/// `parse_feature_reference_()`.
	fn parse_feature_reference_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let feature_name = self.expect_tag_()?;
		self.expect_symbol_(";")?;
		Ok(Statement::FeatureReferenceStatement(
			FeatureReferenceStatement {
				feature_name,
				location,
			},
		))
	}

	fn enter_scopes(&mut self) {
		self.anchors_.enter_scope();
		self.valuerecords_.enter_scope();
	}

	fn exit_scopes(&mut self) {
		self.anchors_.exit_scope();
		self.valuerecords_.exit_scope();
	}

	/// Parses a `featureNames` statement found in stylistic set features
	/// (`parse_featureNames_`).
	fn parse_feature_names_(&mut self, tag: &str) -> Result<Statement> {
		let mut block = NestedBlock {
			tag: tag.to_owned(),
			block_name: self.cur_str(),
			statements: Vec::new(),
			location: self.cur_loc(),
		};
		self.expect_symbol_("{")?;
		self.enter_scopes();
		while self.table_loop_cond() {
			self.advance_lexer_(true)?;
			if self.cur_token_type_ == Some(TokenType::Comment) {
				block.statements.push(self.comment());
			} else if self.is_cur_keyword_("name") {
				let location = self.cur_loc();
				let (platform_id, plat_enc_id, lang_id, string) = self.parse_name_()?;
				block
					.statements
					.push(Statement::FeatureNameStatement(FeatureNameStatement {
						name_id: tag.to_owned(),
						platform_id,
						plat_enc_id,
						lang_id,
						string,
						location,
					}));
			} else if self.cur_token_.is(";") {
				continue;
			} else {
				return Err(Error::feature_lib("Expected \"name\"", self.cur_loc()));
			}
		}
		self.expect_symbol_("}")?;
		self.exit_scopes();
		self.expect_symbol_(";")?;
		Ok(Statement::NestedBlock(block))
	}

	/// Parses a `cvParameters` block found in Character Variant features
	/// (`parse_cvParameters_`).
	fn parse_cv_parameters_(&mut self, tag: &str) -> Result<Statement> {
		let mut block = NestedBlock {
			tag: tag.to_owned(),
			block_name: self.cur_str(),
			statements: Vec::new(),
			location: self.cur_loc(),
		};
		self.expect_symbol_("{")?;
		self.enter_scopes();

		while self.table_loop_cond() {
			self.advance_lexer_(true)?;
			if self.cur_token_type_ == Some(TokenType::Comment) {
				block.statements.push(self.comment());
			} else if self.is_cur_keyword_any(&[
				"FeatUILabelNameID",
				"FeatUITooltipTextNameID",
				"SampleTextNameID",
				"ParamUILabelNameID",
			]) {
				let block_name = self.cur_str();
				block
					.statements
					.push(self.parse_cv_name_ids_(tag, &block_name)?);
			} else if self.is_cur_keyword_("Character") {
				block.statements.push(self.parse_cv_character_(tag)?);
			} else if self.cur_token_.is(";") {
				continue;
			} else {
				return Err(Error::feature_lib(
					format!(
						"Expected statement: got {} {}",
						Self::type_str(self.cur_token_type_),
						self.cur_str()
					),
					self.cur_loc(),
				));
			}
		}

		self.expect_symbol_("}")?;
		self.exit_scopes();
		self.expect_symbol_(";")?;
		Ok(Statement::NestedBlock(block))
	}

	/// `parse_cvNameIDs_(tag, block_name)`.
	fn parse_cv_name_ids_(&mut self, tag: &str, block_name: &str) -> Result<Statement> {
		let mut block = NestedBlock {
			tag: tag.to_owned(),
			block_name: block_name.to_owned(),
			statements: Vec::new(),
			location: self.cur_loc(),
		};
		self.expect_symbol_("{")?;
		self.enter_scopes();
		while self.table_loop_cond() {
			self.advance_lexer_(true)?;
			if self.cur_token_type_ == Some(TokenType::Comment) {
				block.statements.push(self.comment());
			} else if self.is_cur_keyword_("name") {
				let location = self.cur_loc();
				let (platform_id, plat_enc_id, lang_id, string) = self.parse_name_()?;
				block.statements.push(Statement::CVParametersNameStatement(
					CVParametersNameStatement {
						name_id: tag.to_owned(),
						platform_id,
						plat_enc_id,
						lang_id,
						string,
						block_name: block_name.to_owned(),
						location,
					},
				));
			} else if self.cur_token_.is(";") {
				continue;
			} else {
				return Err(Error::feature_lib("Expected \"name\"", self.cur_loc()));
			}
		}
		self.expect_symbol_("}")?;
		self.exit_scopes();
		self.expect_symbol_(";")?;
		Ok(Statement::NestedBlock(block))
	}

	/// `parse_cvCharacter_(tag)`.
	fn parse_cv_character_(&mut self, tag: &str) -> Result<Statement> {
		let location = self.cur_loc();
		let character = self.expect_any_number_()?;
		self.expect_symbol_(";")?;
		if !(0..=0xFFFFFF).contains(&character) {
			return Err(Error::feature_lib(
				"Character value must be between 0x0 and 0xffffff",
				location,
			));
		}
		Ok(Statement::CharacterStatement(CharacterStatement {
			character,
			tag: tag.to_owned(),
			location,
		}))
	}

	/// Parses a `FontRevision` statement found in the head table
	/// (`parse_FontRevision_`).
	fn parse_font_revision_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let version = self.expect_float_()?;
		self.expect_symbol_(";")?;
		if version <= 0.0 {
			return Err(Error::feature_lib(
				"Font revision numbers must be positive",
				location,
			));
		}
		Ok(Statement::FontRevisionStatement(FontRevisionStatement {
			revision: version,
			location,
		}))
	}

	/// `parse_conditionset_()`.
	fn parse_conditionset_(&mut self) -> Result<Statement> {
		let location = self.cur_loc();
		let name = self.expect_name_()?;

		let mut conditions: IndexMap<String, (IntOrFloat, IntOrFloat)> = IndexMap::new();
		self.expect_symbol_("{")?;

		// UPSTREAM-BUG(B36): a condition without numbers keeps the values of the
		// previous condition, or raises `UnboundLocalError` for the first one;
		// reproduced.
		let mut min_value: Option<IntOrFloat> = None;
		let mut max_value: Option<IntOrFloat> = None;
		while !self.next_token_.is("}") {
			self.advance_lexer_(false)?;
			if self.cur_token_type_ != Some(TokenType::Name) {
				return Err(Error::feature_lib("Expected an axis name", self.cur_loc()));
			}

			let axis = self.cur_str();
			if conditions.contains_key(&axis) {
				return Err(Error::feature_lib(
					format!("Repeated condition for axis {axis}"),
					self.cur_loc(),
				));
			}

			if self.next_token_type_ == Some(TokenType::Float) {
				min_value = Some(IntOrFloat::Float(self.expect_float_()?));
			} else if self.next_token_type_ == Some(TokenType::Number) {
				min_value = Some(IntOrFloat::Int(self.expect_number_()?));
			}

			if self.next_token_type_ == Some(TokenType::Float) {
				max_value = Some(IntOrFloat::Float(self.expect_float_()?));
			} else if self.next_token_type_ == Some(TokenType::Number) {
				max_value = Some(IntOrFloat::Int(self.expect_number_()?));
			}
			self.expect_symbol_(";")?;

			let unbound = |name: &str| {
				Error::Value(format!(
					"cannot access local variable '{name}' where it is not associated with a value"
				))
			};
			let min = min_value.ok_or_else(|| unbound("min_value"))?;
			let max = max_value.ok_or_else(|| unbound("max_value"))?;
			conditions.insert(axis, (min, max));
		}

		self.expect_symbol_("}")?;

		let finalname = self.expect_name_()?;
		if finalname != name {
			return Err(Error::feature_lib(
				format!("Expected \"{name}\""),
				self.cur_loc(),
			));
		}
		Ok(Statement::ConditionsetStatement(ConditionsetStatement {
			name,
			conditions,
			location,
		}))
	}

	/// Parses the statements of a feature, lookup or variation block and its end
	/// (`parse_block_`).
	fn parse_block_(
		&mut self,
		statements: &mut Vec<Statement>,
		block_name: &str,
		vertical: bool,
		stylisticset: Option<&str>,
		size_feature: bool,
		cv_feature: Option<&str>,
	) -> Result<()> {
		self.expect_symbol_("{")?;
		self.enter_scopes();

		while self.table_loop_cond() {
			self.advance_lexer_(true)?;
			if self.cur_token_type_ == Some(TokenType::Comment) {
				statements.push(self.comment());
			} else if self.cur_token_type_ == Some(TokenType::Glyphclass) {
				statements.push(self.parse_glyphclass_definition_()?);
			} else if self.is_cur_keyword_("anchorDef") {
				statements.push(Statement::AnchorDefinition(self.parse_anchordef_()?));
			} else if self.is_cur_keyword_any(&["enum", "enumerate"]) {
				statements.push(self.parse_enumerate_(vertical)?);
			} else if self.is_cur_keyword_("feature") {
				statements.push(self.parse_feature_reference_()?);
			} else if self.is_cur_keyword_("ignore") {
				statements.push(self.parse_ignore_()?);
			} else if self.is_cur_keyword_("language") {
				statements.push(self.parse_language_()?);
			} else if self.is_cur_keyword_("lookup") {
				statements.push(self.parse_lookup_(vertical)?);
			} else if self.is_cur_keyword_("lookupflag") {
				statements.push(self.parse_lookupflag_()?);
			} else if self.is_cur_keyword_("markClass") {
				statements.push(self.parse_mark_class_()?);
			} else if self.is_cur_keyword_any(&["pos", "position"]) {
				statements.push(self.parse_position_(false, vertical)?);
			} else if self.is_cur_keyword_("script") {
				statements.push(self.parse_script_()?);
			} else if self.is_cur_keyword_any(&["sub", "substitute", "rsub", "reversesub"]) {
				statements.push(self.parse_substitute_()?);
			} else if self.is_cur_keyword_("subtable") {
				statements.push(self.parse_subtable_()?);
			} else if self.is_cur_keyword_("valueRecordDef") {
				statements.push(Statement::ValueRecordDefinition(
					self.parse_valuerecord_definition_(vertical)?,
				));
			} else if let Some(tag) = stylisticset.filter(|_| self.is_cur_keyword_("featureNames"))
			{
				statements.push(self.parse_feature_names_(tag)?);
			} else if let Some(tag) = cv_feature.filter(|_| self.is_cur_keyword_("cvParameters")) {
				statements.push(self.parse_cv_parameters_(tag)?);
			} else if size_feature && self.is_cur_keyword_("parameters") {
				statements.push(self.parse_size_parameters_()?);
			} else if size_feature && self.is_cur_keyword_("sizemenuname") {
				statements.push(self.parse_size_menuname_()?);
			} else if self.cur_token_.is(";") {
				continue;
			} else {
				return Err(Error::feature_lib(
					format!(
						"Expected glyph class definition or statement: got {} {}",
						Self::type_str(self.cur_token_type_),
						self.cur_str()
					),
					self.cur_loc(),
				));
			}
		}

		self.expect_symbol_("}")?;
		self.exit_scopes();

		let name = self.expect_name_()?;
		let block_name = fontoxide_misc::py::strip(block_name);
		if name != block_name {
			return Err(Error::feature_lib(
				format!("Expected \"{block_name}\""),
				self.cur_loc(),
			));
		}
		self.expect_symbol_(";")?;
		Ok(())
	}

	/// `is_cur_keyword_(k)` for one keyword.
	fn is_cur_keyword_(&self, k: &str) -> bool {
		self.cur_token_type_ == Some(TokenType::Name) && self.cur_token_.is(k)
	}

	/// `is_cur_keyword_(k)` for a collection of keywords.
	fn is_cur_keyword_any(&self, k: &[&str]) -> bool {
		self.cur_token_type_ == Some(TokenType::Name) && k.iter().any(|k| self.cur_token_.is(k))
	}

	/// `expect_class_name_()`.
	fn expect_class_name_(&mut self) -> Result<String> {
		self.advance_lexer_(false)?;
		if self.cur_token_type_ != Some(TokenType::Glyphclass) {
			return Err(Error::feature_lib("Expected @NAME", self.cur_loc()));
		}
		Ok(self.cur_str())
	}

	/// `expect_cid_()`.
	fn expect_cid_(&mut self) -> Result<i64> {
		self.advance_lexer_(false)?;
		if self.cur_token_type_ == Some(TokenType::Cid)
			&& let Token::Int(i) = self.cur_token_
		{
			return Ok(i);
		}
		Err(Error::feature_lib("Expected a CID", self.cur_loc()))
	}

	/// `expect_filename_()`.
	fn expect_filename_(&mut self) -> Result<String> {
		self.advance_lexer_(false)?;
		if self.cur_token_type_ != Some(TokenType::Filename) {
			return Err(Error::feature_lib("Expected file name", self.cur_loc()));
		}
		Ok(self.cur_str())
	}

	/// `expect_glyph_()`: a glyph name (without leading backslashes) or `cidNNNNN`.
	fn expect_glyph_(&mut self) -> Result<String> {
		self.advance_lexer_(false)?;
		match (self.cur_token_type_, &self.cur_token_) {
			(Some(TokenType::Name), Token::Str(s)) => Ok(s.trim_start_matches('\\').to_owned()),
			(Some(TokenType::Cid), Token::Int(i)) => Ok(format!("cid{i:05}")),
			_ => Err(Error::feature_lib(
				"Expected a glyph name or CID",
				self.cur_loc(),
			)),
		}
	}

	/// Records glyph names that are not in the glyph set for the error at the end
	/// of parsing (`check_glyph_name_in_glyph_set`). Does nothing without a glyph
	/// set.
	fn check_glyph_name_in_glyph_set(&mut self, names: &[&str]) {
		if !self.glyph_names_.is_empty() {
			for &name in names {
				if self.glyph_names_.contains(name) {
					continue;
				}
				if !self.missing.contains_key(name) {
					self.missing.insert(name.to_owned(), self.cur_loc());
				}
			}
		}
	}

	/// `expect_markClass_reference_()`.
	fn expect_mark_class_reference_(&mut self) -> Result<MarkClassRef> {
		let name = self.expect_class_name_()?;
		match self.glyphclasses_.resolve(&name) {
			None => Err(Error::feature_lib(
				format!("Unknown markClass @{name}"),
				self.cur_loc(),
			)),
			Some(GlyphClassEntry::GlyphClass(_)) => Err(Error::feature_lib(
				format!("@{name} is not a markClass"),
				self.cur_loc(),
			)),
			Some(GlyphClassEntry::MarkClass(mc)) => Ok(mc.clone()),
		}
	}

	/// `expect_tag_()`: a name of at most four characters, padded with spaces.
	fn expect_tag_(&mut self) -> Result<String> {
		self.advance_lexer_(false)?;
		if self.cur_token_type_ != Some(TokenType::Name) {
			return Err(Error::feature_lib("Expected a tag", self.cur_loc()));
		}
		let token = self.cur_str();
		let len = token.chars().count();
		if len > 4 {
			return Err(Error::feature_lib(
				"Tags cannot be longer than 4 characters",
				self.cur_loc(),
			));
		}
		Ok(format!("{token}{}", " ".repeat(4 - len)))
	}

	/// `expect_script_tag_()`.
	fn expect_script_tag_(&mut self) -> Result<String> {
		let tag = self.expect_tag_()?;
		if tag == "dflt" {
			return Err(Error::feature_lib(
				"\"dflt\" is not a valid script tag; use \"DFLT\" instead",
				self.cur_loc(),
			));
		}
		Ok(tag)
	}

	/// `expect_language_tag_()`.
	fn expect_language_tag_(&mut self) -> Result<String> {
		let tag = self.expect_tag_()?;
		if tag == "DFLT" {
			return Err(Error::feature_lib(
				"\"DFLT\" is not a valid language tag; use \"dflt\" instead",
				self.cur_loc(),
			));
		}
		Ok(tag)
	}

	/// `expect_symbol_(symbol)`.
	fn expect_symbol_(&mut self, symbol: &str) -> Result<()> {
		self.advance_lexer_(false)?;
		if self.cur_token_type_ == Some(TokenType::Symbol) && self.cur_token_.is(symbol) {
			return Ok(());
		}
		Err(Error::feature_lib(
			format!("Expected '{symbol}'"),
			self.cur_loc(),
		))
	}

	/// `expect_keyword_(keyword)`.
	fn expect_keyword_(&mut self, keyword: &str) -> Result<()> {
		self.advance_lexer_(false)?;
		if self.cur_token_type_ == Some(TokenType::Name) && self.cur_token_.is(keyword) {
			return Ok(());
		}
		Err(Error::feature_lib(
			format!("Expected \"{keyword}\""),
			self.cur_loc(),
		))
	}

	/// `expect_name_()`.
	fn expect_name_(&mut self) -> Result<String> {
		self.advance_lexer_(false)?;
		if self.cur_token_type_ == Some(TokenType::Name) {
			return Ok(self.cur_str());
		}
		Err(Error::feature_lib("Expected a name", self.cur_loc()))
	}

	/// `expect_number_()`: a decimal integer.
	fn expect_number_(&mut self) -> Result<i64> {
		self.advance_lexer_(false)?;
		if self.cur_token_type_ == Some(TokenType::Number)
			&& let Token::Int(i) = self.cur_token_
		{
			return Ok(i);
		}
		Err(Error::feature_lib("Expected a number", self.cur_loc()))
	}

	/// `expect_number_(variable=True)`: a decimal integer or a variable scalar.
	fn expect_number_variable_(&mut self) -> Result<ScalarValue> {
		self.advance_lexer_(false)?;
		if self.cur_token_type_ == Some(TokenType::Number)
			&& let Token::Int(i) = self.cur_token_
		{
			return Ok(ScalarValue::Int(i));
		}
		if self.cur_token_type_ == Some(TokenType::Symbol) && self.cur_token_.is("(") {
			return Ok(ScalarValue::Variable(self.expect_variable_scalar_()?));
		}
		Err(Error::feature_lib("Expected a number", self.cur_loc()))
	}

	/// `expect_variable_scalar_()`.
	fn expect_variable_scalar_(&mut self) -> Result<VariableScalar> {
		self.advance_lexer_(false)?; // "("
		let mut scalar = VariableScalar::new();
		let mut default_seen = false;
		loop {
			if self.cur_token_type_ == Some(TokenType::Symbol) && self.cur_token_.is(")") {
				break;
			}
			let (location, value) = if let (Some(TokenType::Number), Token::Int(value)) =
				(self.cur_token_type_, &self.cur_token_)
			{
				if default_seen {
					return Err(Error::feature_lib(
						"Duplicate value for the default location",
						self.cur_loc(),
					));
				}
				default_seen = true;
				let value = *value;
				self.advance_lexer_(false)?;
				(IndexMap::new(), value)
			} else {
				self.expect_master_()?
			};
			scalar.add_value(location, value);
		}
		Ok(scalar)
	}

	/// `expect_master_()`: `axis=value,axis=value:scalar`.
	fn expect_master_(&mut self) -> Result<(IndexMap<String, f64>, i64)> {
		let mut location = IndexMap::new();
		loop {
			if self.cur_token_type_ != Some(TokenType::Name) {
				return Err(Error::feature_lib("Expected an axis name", self.cur_loc()));
			}
			let axis = self.cur_str();
			self.advance_lexer_(false)?;
			if !(self.cur_token_type_ == Some(TokenType::Symbol) && self.cur_token_.is("=")) {
				return Err(Error::feature_lib(
					"Expected an equals sign",
					self.cur_loc(),
				));
			}
			let value = self.expect_integer_or_float_()?;
			location.insert(axis, value.as_f64());
			if self.next_token_type_ == Some(TokenType::Name)
				&& self
					.next_token_
					.as_str()
					.is_some_and(|s| s.starts_with(':'))
			{
				// Lexer has just read the value as a glyph name. We'll correct it later
				break;
			}
			self.advance_lexer_(false)?;
			if !(self.cur_token_type_ == Some(TokenType::Symbol) && self.cur_token_.is(",")) {
				return Err(Error::feature_lib(
					"Expected an comma or an equals sign",
					self.cur_loc(),
				));
			}
			self.advance_lexer_(false)?;
		}
		self.advance_lexer_(false)?;
		let token = self.cur_str();
		let value = py_int(&token.chars().skip(1).collect::<String>())?;
		self.advance_lexer_(false)?;
		Ok((location, value))
	}

	/// `expect_any_number_()`: a decimal, hexadecimal or octal integer.
	fn expect_any_number_(&mut self) -> Result<i64> {
		self.advance_lexer_(false)?;
		if self.cur_token_type_.is_some_and(TokenType::is_number)
			&& let Token::Int(i) = self.cur_token_
		{
			return Ok(i);
		}
		Err(Error::feature_lib(
			"Expected a decimal, hexadecimal or octal number",
			self.cur_loc(),
		))
	}

	/// `expect_float_()`.
	fn expect_float_(&mut self) -> Result<f64> {
		self.advance_lexer_(false)?;
		if self.cur_token_type_ == Some(TokenType::Float)
			&& let Token::Float(f) = self.cur_token_
		{
			return Ok(f);
		}
		Err(Error::feature_lib(
			"Expected a floating-point number",
			self.cur_loc(),
		))
	}

	/// `expect_integer_or_float_()`.
	fn expect_integer_or_float_(&mut self) -> Result<IntOrFloat> {
		if self.next_token_type_ == Some(TokenType::Float) {
			Ok(IntOrFloat::Float(self.expect_float_()?))
		} else if self.next_token_type_ == Some(TokenType::Number) {
			Ok(IntOrFloat::Int(self.expect_number_()?))
		} else {
			Err(Error::feature_lib(
				"Expected an integer or floating-point number",
				self.cur_loc(),
			))
		}
	}

	/// `expect_decipoint_()`: a float, or an integer in decipoints.
	fn expect_decipoint_(&mut self) -> Result<f64> {
		if self.next_token_type_ == Some(TokenType::Float) {
			self.expect_float_()
		} else if self.next_token_type_ == Some(TokenType::Number) {
			Ok(self.expect_number_()? as f64 / 10.0)
		} else {
			Err(Error::feature_lib(
				"Expected an integer or floating-point number",
				self.cur_loc(),
			))
		}
	}

	/// `expect_stat_flags()`.
	fn expect_stat_flags(&mut self) -> Result<i64> {
		let mut value = 0;
		let flags = [
			("OlderSiblingFontAttribute", 1),
			("ElidableAxisValueName", 2),
		];
		while !self.next_token_.is(";") {
			if let Some(&(_, bit)) = flags.iter().find(|(f, _)| self.next_token_.is(f)) {
				self.expect_name_()?;
				value |= bit;
			} else {
				// UPSTREAM-BUG(B37): the message names the current token (`flag` or the
				// previous flag) instead of the unexpected next one; reproduced.
				return Err(Error::feature_lib(
					format!("Unexpected STAT flag {}", self.cur_str()),
					self.cur_loc(),
				));
			}
		}
		Ok(value)
	}

	/// `expect_string_()`.
	fn expect_string_(&mut self) -> Result<String> {
		self.advance_lexer_(false)?;
		if self.cur_token_type_ == Some(TokenType::String) {
			return Ok(self.cur_str());
		}
		Err(Error::feature_lib("Expected a string", self.cur_loc()))
	}

	/// `advance_lexer_(comments)`: moves to the next token; with `comments`,
	/// pending comments are returned first.
	fn advance_lexer_(&mut self, comments: bool) -> Result<()> {
		if comments && let Some((text, location)) = self.cur_comments_.pop_front() {
			self.cur_token_type_ = Some(TokenType::Comment);
			self.cur_token_ = Token::Str(text);
			self.cur_token_location_ = Some(location);
			return Ok(());
		}
		self.cur_token_type_ = self.next_token_type_;
		self.cur_token_ = std::mem::take(&mut self.next_token_);
		self.cur_token_location_ = self.next_token_location_.clone();
		loop {
			match self.lexer_.next()? {
				Some((token_type, token, location)) => {
					self.next_token_type_ = Some(token_type);
					self.next_token_ = token;
					self.next_token_location_ = Some(location);
				}
				None => {
					// The location of the last token is kept.
					self.next_token_type_ = None;
					self.next_token_ = Token::None;
				}
			}
			if self.next_token_type_ != Some(TokenType::Comment) {
				break;
			}
			if let Some(location) = self.next_token_location_.clone() {
				self.cur_comments_
					.push_back((self.next_token_.py_str(), location));
			}
		}
		Ok(())
	}

	/// `make_cid_range_(location, start, limit)`:
	/// `(999, 1001)` → `["cid00999", "cid01000", "cid01001"]`.
	fn make_cid_range_(location: &Location, start: i64, limit: i64) -> Result<Vec<String>> {
		if start > limit {
			return Err(Error::feature_lib(
				"Bad range: start should be less than limit",
				location.clone(),
			));
		}
		Ok((start..=limit).map(|cid| format!("cid{cid:05}")).collect())
	}

	/// `make_glyph_range_(location, start, limit)`:
	/// `("a.sc", "d.sc")` → `["a.sc", "b.sc", "c.sc", "d.sc"]`.
	fn make_glyph_range_(
		&self,
		location: &Location,
		start: &str,
		limit: &str,
	) -> Result<Vec<String>> {
		let s: Vec<char> = start.chars().collect();
		let l: Vec<char> = limit.chars().collect();
		if s.len() != l.len() {
			return Err(Error::feature_lib(
				format!("Bad range: \"{start}\" and \"{limit}\" should have the same length"),
				location.clone(),
			));
		}

		let prefix_len = common_prefix(&s, &l);
		let rs: Vec<char> = s.iter().rev().copied().collect();
		let rl: Vec<char> = l.iter().rev().copied().collect();
		let suffix_len = common_prefix(&rs, &rl);
		let prefix: String = s[..prefix_len].iter().collect();
		let suffix: String = s[s.len() - suffix_len..].iter().collect();
		// `start[len(prefix):-len(suffix)]`, which is empty if they overlap.
		let middle = |v: &[char]| -> String {
			let end = if suffix_len > 0 {
				v.len() - suffix_len
			} else {
				v.len()
			};
			if prefix_len >= end {
				String::new()
			} else {
				v[prefix_len..end].iter().collect()
			}
		};
		let start_range = middle(&s);
		let limit_range = middle(&l);

		if start_range >= limit_range {
			return Err(Error::feature_lib(
				"Start of range must be smaller than its end",
				location.clone(),
			));
		}

		let single = |r: &str, f: fn(&char) -> bool| {
			let mut chars = r.chars();
			matches!((chars.next(), chars.next()), (Some(c), None) if f(&c))
		};
		for f in [
			char::is_ascii_uppercase as fn(&char) -> bool,
			char::is_ascii_lowercase,
		] {
			if single(&start_range, f) && single(&limit_range, f) {
				let a = start_range.chars().next().map_or(0, u32::from);
				let b = limit_range.chars().next().map_or(0, u32::from);
				return Ok((a..=b)
					.filter_map(char::from_u32)
					.map(|c| format!("{prefix}{c}{suffix}"))
					.collect());
			}
		}

		let digits = |r: &str| (1..=3).contains(&r.len()) && r.chars().all(|c| c.is_ascii_digit());
		if digits(&start_range) && digits(&limit_range) {
			let a: u32 = start_range.parse().unwrap_or(0);
			let b: u32 = limit_range.parse().unwrap_or(0);
			let width = start_range.len();
			return Ok((a..=b)
				.map(|i| {
					let number = format!("000{i}");
					let number = &number[number.len() - width..];
					format!("{prefix}{number}{suffix}")
				})
				.collect());
		}

		Err(Error::feature_lib(
			format!("Bad range: \"{start}-{limit}\""),
			location.clone(),
		))
	}
}

/// The `UnicodeError.reason` of a codec failure.
// LENIENT(L33): the reason is chosen by codec family; Python's multibyte codecs say
// "incomplete multibyte sequence" for a truncated character at the end.
fn codec_reason(encoding: &str) -> String {
	match codecs::normalize(encoding).as_str() {
		"ascii" => "ordinal not in range(128)".to_owned(),
		e if e.starts_with("mac_") || e == "latin1" => "character maps to <undefined>".to_owned(),
		_ => "illegal multibyte sequence".to_owned(),
	}
}

/// Decodes UTF-16 code units strictly, with the reasons of Python's `utf_16_be`
/// codec.
fn decode_utf16_strict(units: &[u16]) -> std::result::Result<String, String> {
	let mut out = String::with_capacity(units.len());
	let mut i = 0;
	while i < units.len() {
		let u = units[i];
		match u {
			0xD800..=0xDBFF => match units.get(i + 1) {
				None => return Err("unexpected end of data".to_owned()),
				Some(&lo @ 0xDC00..=0xDFFF) => {
					let c = 0x10000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(lo) - 0xDC00);
					out.extend(char::from_u32(c));
					i += 2;
					continue;
				}
				Some(_) => return Err("illegal UTF-16 surrogate".to_owned()),
			},
			0xDC00..=0xDFFF => return Err("illegal encoding".to_owned()),
			_ => out.extend(char::from_u32(u32::from(u))),
		}
		i += 1;
	}
	Ok(out)
}
