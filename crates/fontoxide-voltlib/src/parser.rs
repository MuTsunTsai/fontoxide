//! The parser of VOLT projects (`fontTools.voltLib.parser`).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::ast::{
	AnchorDefinition, ContextDefinition, Coverage, CoverageItem, Enum, FeatureDefinition,
	GlyphDefinition, GlyphName, GroupDefinition, GroupName, LangSysDefinition, LookupDefinition,
	Pos, PositionAdjustPairDefinition, PositionAdjustSingleDefinition,
	PositionAttachCursiveDefinition, PositionAttachDefinition, PositionDefinition, ProcessMarks,
	Range, ScriptDefinition, SettingDefinition, SettingValue, Statement, SubstitutionDefinition,
	SubstitutionKind, Tables, VoltFile,
};
use crate::error::{Error, Location, Result};
use crate::lexer::{Lexer, Token, TokenType};

/// The statements the parser knows, by keyword (`PARSE_FUNCS`), sorted.
const PARSE_FUNCS: [&str; 12] = [
	"CMAP_FORMAT",
	"COMPILER_USEEXTENSIONLOOKUPS",
	"COMPILER_USEPAIRPOSFORMAT2",
	"DEF_ANCHOR",
	"DEF_GLYPH",
	"DEF_GROUP",
	"DEF_LOOKUP",
	"DEF_SCRIPT",
	"DO_NOT_TOUCH_CMAP",
	"GRID_PPEM",
	"PPOSITIONING_PPEM",
	"PRESENTATION_PPEM",
];

/// What to parse: a file, or text with an optional file name for the error
/// locations (a file object in Python).
#[derive(Debug, Clone)]
pub enum ParserInput {
	/// A path to read.
	Path(PathBuf),
	/// The text of a project, and the name of its file, if any.
	Text {
		/// The VOLT source.
		text: String,
		/// The file name for locations (`<volt>` when `None`).
		name: Option<String>,
	},
}

impl ParserInput {
	/// Text without a file name (`StringIO(text)`).
	pub fn text(text: impl Into<String>) -> Self {
		ParserInput::Text {
			text: text.into(),
			name: None,
		}
	}
}

impl From<PathBuf> for ParserInput {
	fn from(path: PathBuf) -> Self {
		ParserInput::Path(path)
	}
}

impl From<&std::path::Path> for ParserInput {
	fn from(path: &std::path::Path) -> Self {
		ParserInput::Path(path.to_owned())
	}
}

/// A symbol table with nested scopes (`SymbolTable`).
#[derive(Debug, Clone)]
pub(crate) struct SymbolTable<T> {
	scopes_: Vec<IndexMap<String, T>>,
}

impl<T> Default for SymbolTable<T> {
	fn default() -> Self {
		SymbolTable {
			scopes_: vec![IndexMap::new()],
		}
	}
}

impl<T> SymbolTable<T> {
	pub(crate) fn enter_scope(&mut self) {
		self.scopes_.push(IndexMap::new());
	}

	pub(crate) fn exit_scope(&mut self) {
		self.scopes_.pop();
	}

	pub(crate) fn define(&mut self, name: &str, item: T) {
		if let Some(scope) = self.scopes_.last_mut() {
			scope.insert(name.to_owned(), item);
		}
	}

	/// `resolve(name, case_insensitive)`.
	// UPSTREAM-BUG(B55): the case-insensitive lookup only searches the outermost scope
	// (the loop variable left over from the exact lookup), so names in an inner
	// scope (the languages of a script) are compared case-sensitively.
	pub(crate) fn resolve(&self, name: &str, case_insensitive: bool) -> Option<&T> {
		for scope in self.scopes_.iter().rev() {
			if let Some(item) = scope.get(name) {
				return Some(item);
			}
		}
		if case_insensitive && let Some(scope) = self.scopes_.first() {
			let lower = name.to_lowercase();
			for (key, item) in scope {
				if key.to_lowercase() == lower {
					return Some(item);
				}
			}
		}
		None
	}
}

/// Python's `int(s, 16)`: surrounding whitespace, a sign, a `0x` prefix and
/// underscores between digits are accepted.
// LENIENT(L42): values beyond 64 bits and non-ASCII digits are rejected.
fn py_int_base16(s: &str) -> Option<i64> {
	let t = fontoxide_misc::py::strip(s);
	let (negative, t) = match t.as_bytes().first() {
		Some(b'-') => (true, &t[1..]),
		Some(b'+') => (false, &t[1..]),
		_ => (false, t),
	};
	let t = match t.get(..2) {
		Some("0x" | "0X") => t[2..].strip_prefix('_').unwrap_or(&t[2..]),
		_ => t,
	};
	if t.is_empty() || t.starts_with('_') || t.ends_with('_') || t.contains("__") {
		return None;
	}
	let digits: String = t.chars().filter(|&c| c != '_').collect();
	if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
		return None;
	}
	let value = i64::from_str_radix(&digits, 16).ok()?;
	Some(if negative { -value } else { value })
}

/// `%.200R`: the `repr` of a string, cut to 200 characters.
fn repr200(s: &str) -> String {
	fontoxide_misc::py::str_repr(s).chars().take(200).collect()
}

/// The VOLT project parser (`Parser`).
#[derive(Debug)]
pub struct Parser {
	statements: Vec<Statement>,
	tables: Rc<Tables>,
	/// The anchor components by glyph, then by anchor name (`anchors_`).
	anchors_: HashMap<String, SymbolTable<i64>>,
	scripts_: SymbolTable<()>,
	langs_: SymbolTable<()>,
	lookups_: SymbolTable<()>,
	next_token_: Option<Token>,
	next_token_location_: Location,
	cur_token_: Option<Token>,
	cur_token_location_: Location,
	lexer_: Lexer,
}

impl Parser {
	/// `Parser(path)`: reads the project and the first token.
	// LENIENT(L42): files are read as UTF-8; Python uses the locale encoding.
	pub fn new(input: impl Into<ParserInput>) -> Result<Self> {
		let (text, filename) = match input.into() {
			ParserInput::Path(path) => {
				let name = path.display().to_string();
				let text =
					std::fs::read_to_string(&path).map_err(|e| Error::io(name.clone(), e))?;
				(text, Some(name))
			}
			ParserInput::Text { text, name } => (text, name),
		};
		let mut parser = Parser {
			statements: Vec::new(),
			tables: Rc::new(Tables::default()),
			anchors_: HashMap::new(),
			scripts_: SymbolTable::default(),
			langs_: SymbolTable::default(),
			lookups_: SymbolTable::default(),
			next_token_: None,
			next_token_location_: None,
			cur_token_: None,
			cur_token_location_: None,
			lexer_: Lexer::new(&text, filename.as_deref()),
		};
		parser.advance_lexer_()?;
		Ok(parser)
	}

	/// Parses the project (`parse`).
	pub fn parse(mut self) -> Result<VoltFile> {
		while self.next_token_.is_some() {
			self.advance_lexer_()?;
			let keyword = self
				.cur_token_
				.as_ref()
				.and_then(Token::text)
				.filter(|t| PARSE_FUNCS.contains(t))
				.map(str::to_owned);
			if let Some(keyword) = keyword {
				let statement = match keyword.as_str() {
					"DEF_GLYPH" => Statement::GlyphDefinition(self.parse_def_glyph_()?),
					"DEF_GROUP" => Statement::GroupDefinition(self.parse_def_group_()?),
					"DEF_SCRIPT" => Statement::ScriptDefinition(self.parse_def_script_()?),
					"DEF_LOOKUP" => Statement::LookupDefinition(self.parse_def_lookup_()?),
					"DEF_ANCHOR" => Statement::AnchorDefinition(self.parse_def_anchor_()?),
					"GRID_PPEM" | "PRESENTATION_PPEM" | "PPOSITIONING_PPEM" => {
						Statement::SettingDefinition(self.parse_ppem_()?)
					}
					"CMAP_FORMAT" => Statement::SettingDefinition(self.parse_cmap_format()?),
					_ => Statement::SettingDefinition(self.parse_noarg_option_()),
				};
				self.statements.push(statement);
			} else if self.is_cur_keyword_("END") {
				break;
			} else {
				return Err(Error::volt_lib(
					format!("Expected {}", PARSE_FUNCS.join(", ")),
					self.cur_token_location_.clone(),
				));
			}
		}
		Ok(VoltFile {
			statements: self.statements,
			tables: self.tables,
		})
	}

	fn next_is(&self, keyword: &str) -> bool {
		self.next_token_.as_ref().and_then(Token::text) == Some(keyword)
	}

	fn next_is_any(&self, keywords: &[&str]) -> bool {
		keywords.iter().any(|k| self.next_is(k))
	}

	fn assert_cur_keyword(&self, keyword: &str) -> Result<()> {
		if self.is_cur_keyword_(keyword) {
			Ok(())
		} else {
			Err(Error::Assertion(format!("expected {keyword}")))
		}
	}

	fn cur_text(&self) -> String {
		self.cur_token_
			.as_ref()
			.and_then(Token::text)
			.unwrap_or_default()
			.to_owned()
	}

	fn parse_def_glyph_(&mut self) -> Result<GlyphDefinition> {
		self.assert_cur_keyword("DEF_GLYPH")?;
		let location = self.cur_token_location_.clone();
		let name = self.expect_string_()?;
		self.expect_keyword_("ID")?;
		let gid = self.expect_number_()?;
		if gid < 0 {
			return Err(Error::volt_lib(
				"Invalid glyph ID",
				self.cur_token_location_.clone(),
			));
		}
		let mut gunicode = None;
		if self.next_is("UNICODE") {
			self.expect_keyword_("UNICODE")?;
			let u = self.expect_number_()?;
			if u < 0 {
				return Err(Error::volt_lib(
					"Invalid glyph UNICODE",
					self.cur_token_location_.clone(),
				));
			}
			gunicode = Some(vec![u]);
		} else if self.next_is("UNICODEVALUES") {
			self.expect_keyword_("UNICODEVALUES")?;
			gunicode = self.parse_unicode_values_()?;
		}
		let mut gtype = None;
		if self.next_is("TYPE") {
			self.expect_keyword_("TYPE")?;
			let t = self.expect_name_()?;
			if !["BASE", "LIGATURE", "MARK", "COMPONENT"].contains(&t.as_str()) {
				return Err(Error::Assertion(format!("invalid glyph type {t}")));
			}
			gtype = Some(t);
		}
		let mut components = None;
		if self.next_is("COMPONENTS") {
			self.expect_keyword_("COMPONENTS")?;
			components = Some(self.expect_number_()?);
		}
		self.expect_keyword_("END_GLYPH")?;
		// UPSTREAM-BUG(B54): `OrderedSymbolTable.resolve` does not return its result, so
		// the "Glyph ... already defined" check never fires and a glyph defined
		// twice keeps its first position in the glyph order.
		self.tables.glyphs.borrow_mut().insert(name.clone());
		Ok(GlyphDefinition {
			name,
			id: gid,
			unicode: gunicode,
			glyph_type: gtype,
			components,
			location,
		})
	}

	fn parse_def_group_(&mut self) -> Result<Rc<GroupDefinition>> {
		self.assert_cur_keyword("DEF_GROUP")?;
		let location = self.cur_token_location_.clone();
		let name = self.expect_string_()?;
		let mut enum_ = None;
		if self.next_is("ENUM") {
			enum_ = Some(self.parse_enum_()?);
		}
		self.expect_keyword_("END_GROUP")?;
		if self.tables.groups.borrow().resolve(&name, true).is_some() {
			return Err(Error::volt_lib(
				format!("Glyph group \"{name}\" already defined, group names are case insensitive"),
				location,
			));
		}
		let def_group = Rc::new(GroupDefinition::new(name.clone(), enum_, location));
		self.tables
			.groups
			.borrow_mut()
			.define(&name, def_group.clone());
		Ok(def_group)
	}

	fn parse_def_script_(&mut self) -> Result<ScriptDefinition> {
		self.assert_cur_keyword("DEF_SCRIPT")?;
		let location = self.cur_token_location_.clone();
		let mut name = None;
		if self.next_is("NAME") {
			self.expect_keyword_("NAME")?;
			name = Some(self.expect_string_()?);
		}
		self.expect_keyword_("TAG")?;
		let tag = self.expect_string_()?;
		if self.scripts_.resolve(&tag, true).is_some() {
			return Err(Error::volt_lib(
				format!("Script \"{tag}\" already defined, script tags are case insensitive"),
				location,
			));
		}
		self.langs_.enter_scope();
		let mut langs = Vec::new();
		while !self.next_is("END_SCRIPT") {
			self.advance_lexer_()?;
			let lang = self.parse_langsys_()?;
			self.expect_keyword_("END_LANGSYS")?;
			if self.langs_.resolve(&lang.tag, true).is_some() {
				return Err(Error::volt_lib(
					format!(
						"Language \"{}\" already defined in script \"{tag}\", language tags are case insensitive",
						lang.tag
					),
					location,
				));
			}
			self.langs_.define(&lang.tag, ());
			langs.push(lang);
		}
		self.expect_keyword_("END_SCRIPT")?;
		self.langs_.exit_scope();
		self.scripts_.define(&tag, ());
		Ok(ScriptDefinition {
			name,
			tag,
			langs,
			location,
		})
	}

	fn parse_langsys_(&mut self) -> Result<LangSysDefinition> {
		self.assert_cur_keyword("DEF_LANGSYS")?;
		let location = self.cur_token_location_.clone();
		let mut name = None;
		if self.next_is("NAME") {
			self.expect_keyword_("NAME")?;
			name = Some(self.expect_string_()?);
		}
		self.expect_keyword_("TAG")?;
		let tag = self.expect_string_()?;
		let mut features = Vec::new();
		while !self.next_is("END_LANGSYS") {
			self.advance_lexer_()?;
			let feature = self.parse_feature_()?;
			self.expect_keyword_("END_FEATURE")?;
			features.push(feature);
		}
		Ok(LangSysDefinition {
			name,
			tag,
			features,
			location,
		})
	}

	fn parse_feature_(&mut self) -> Result<FeatureDefinition> {
		self.assert_cur_keyword("DEF_FEATURE")?;
		let location = self.cur_token_location_.clone();
		self.expect_keyword_("NAME")?;
		let name = self.expect_string_()?;
		self.expect_keyword_("TAG")?;
		let tag = self.expect_string_()?;
		let mut lookups = Vec::new();
		while !self.next_is("END_FEATURE") {
			self.expect_keyword_("LOOKUP")?;
			lookups.push(self.expect_string_()?);
		}
		Ok(FeatureDefinition {
			name,
			tag,
			lookups,
			location,
		})
	}

	fn parse_def_lookup_(&mut self) -> Result<LookupDefinition> {
		self.assert_cur_keyword("DEF_LOOKUP")?;
		let location = self.cur_token_location_.clone();
		let name = self.expect_string_()?;
		// LENIENT(L42): `char::is_alphabetic` stands for `str.isalpha`, which differs
		// for letter numbers and other alphabetic marks.
		match name.chars().next() {
			None => {
				return Err(Error::Python(
					"IndexError: string index out of range".to_owned(),
				));
			}
			Some(c) if !c.is_alphabetic() => {
				return Err(Error::volt_lib(
					format!("Lookup name \"{name}\" must start with a letter"),
					location,
				));
			}
			_ => {}
		}
		if self.lookups_.resolve(&name, true).is_some() {
			return Err(Error::volt_lib(
				format!("Lookup \"{name}\" already defined, lookup names are case insensitive"),
				location,
			));
		}
		let mut process_base = true;
		if self.next_is("PROCESS_BASE") {
			self.advance_lexer_()?;
		} else if self.next_is("SKIP_BASE") {
			self.advance_lexer_()?;
			process_base = false;
		}
		let mut process_marks = ProcessMarks::Bool(true);
		let mut mark_glyph_set = None;
		if self.next_is("PROCESS_MARKS") {
			self.advance_lexer_()?;
			if self.next_is("MARK_GLYPH_SET") {
				self.advance_lexer_()?;
				mark_glyph_set = Some(self.expect_string_()?);
			} else if self.next_is("ALL") {
				self.advance_lexer_()?;
			} else if self.next_is("NONE") {
				self.advance_lexer_()?;
				process_marks = ProcessMarks::Bool(false);
			} else if self.next_token_type() == Some(TokenType::String) {
				process_marks = ProcessMarks::Group(self.expect_string_()?);
			} else {
				let got = match self.next_token_type() {
					Some(TokenType::Number) => "NUMBER",
					Some(TokenType::String) => "STRING",
					Some(TokenType::Name) => "NAME",
					Some(TokenType::Newline) => "NEWLINE",
					None => "None",
				};
				return Err(Error::volt_lib(
					format!("Expected ALL, NONE, MARK_GLYPH_SET or an ID. Got {got}"),
					location,
				));
			}
		} else if self.next_is("SKIP_MARKS") {
			self.advance_lexer_()?;
			process_marks = ProcessMarks::Bool(false);
		}
		let mut direction = None;
		if self.next_is("DIRECTION") {
			self.expect_keyword_("DIRECTION")?;
			let d = self.expect_name_()?;
			if d != "LTR" && d != "RTL" {
				return Err(Error::Assertion(format!("invalid direction {d}")));
			}
			direction = Some(d);
		}
		let mut reversal = false;
		if self.next_is("REVERSAL") {
			self.expect_keyword_("REVERSAL")?;
			reversal = true;
		}
		let mut comments = None;
		if self.next_is("COMMENTS") {
			self.expect_keyword_("COMMENTS")?;
			comments = Some(self.expect_string_()?.replace("\\n", "\n"));
		}
		let mut context = Vec::new();
		while self.next_is_any(&["EXCEPT_CONTEXT", "IN_CONTEXT"]) {
			context = self.parse_context_()?;
		}
		let as_pos_or_sub = self.expect_name_()?;
		let mut sub = None;
		let mut pos = None;
		if as_pos_or_sub == "AS_SUBSTITUTION" {
			sub = Some(self.parse_substitution_(reversal)?);
		} else if as_pos_or_sub == "AS_POSITION" {
			pos = Some(self.parse_position_()?);
		} else {
			return Err(Error::volt_lib(
				format!("Expected AS_SUBSTITUTION or AS_POSITION. Got {as_pos_or_sub}"),
				location,
			));
		}
		self.lookups_.define(&name, ());
		Ok(LookupDefinition {
			name,
			process_base,
			process_marks,
			mark_glyph_set,
			direction,
			reversal,
			comments,
			context,
			sub,
			pos,
			location,
		})
	}

	fn parse_context_(&mut self) -> Result<Vec<ContextDefinition>> {
		let location = self.cur_token_location_.clone();
		let mut contexts = Vec::new();
		while self.next_is_any(&["EXCEPT_CONTEXT", "IN_CONTEXT"]) {
			let ex_or_in = self.expect_name_()?;
			if !self.next_is("END_CONTEXT") {
				let mut left = Vec::new();
				let mut right = Vec::new();
				while self.next_is_any(&["LEFT", "RIGHT"]) {
					let side = self.expect_name_()?;
					let coverage = self.parse_coverage_()?;
					if side == "LEFT" {
						left.push(coverage);
					} else {
						right.push(coverage);
					}
				}
				self.expect_keyword_("END_CONTEXT")?;
				contexts.push(ContextDefinition {
					ex_or_in,
					left,
					right,
					location: location.clone(),
				});
			} else {
				self.expect_keyword_("END_CONTEXT")?;
			}
		}
		Ok(contexts)
	}

	fn parse_substitution_(&mut self, reversal: bool) -> Result<SubstitutionDefinition> {
		self.assert_cur_keyword("AS_SUBSTITUTION")?;
		let location = self.cur_token_location_.clone();
		let mut src: Vec<Coverage> = Vec::new();
		let mut dest: Vec<Coverage> = Vec::new();
		if !self.next_is("SUB") {
			return Err(Error::volt_lib("Expected SUB", location));
		}
		while self.next_is("SUB") {
			self.expect_keyword_("SUB")?;
			src.push(self.parse_coverage_()?);
			self.expect_keyword_("WITH")?;
			dest.push(self.parse_coverage_()?);
			self.expect_keyword_("END_SUB")?;
		}
		self.expect_keyword_("END_SUBSTITUTION")?;
		let max_src = src.iter().map(Vec::len).max().unwrap_or_default();
		let max_dest = dest.iter().map(Vec::len).max().unwrap_or_default();

		// many to many or mixed is invalid
		if max_src > 1 && max_dest > 1 {
			return Err(Error::volt_lib("Invalid substitution type", location));
		}

		// `dict(zip(src, dest))`: coverages are tuples of objects that compare
		// by identity, so only empty coverages are the same key.
		let mut mapping: Vec<(Coverage, Coverage)> = Vec::new();
		for (s, d) in src.iter().zip(&dest) {
			if s.is_empty()
				&& let Some(entry) = mapping.iter_mut().find(|(k, _)| k.is_empty())
			{
				entry.1 = d.clone();
				continue;
			}
			mapping.push((s.clone(), d.clone()));
		}
		let kind = if max_src == 1 && max_dest == 1 {
			// Alternate substitutions are represented by adding multiple
			// substitutions for the same glyph, so we detect that here
			let mut glyphs = Vec::new();
			for cov in &src {
				for x in cov {
					glyphs.push(x.glyph_set()?);
				}
			}
			let unique: HashSet<&Vec<String>> = glyphs.iter().collect();
			if unique.len() != glyphs.len() {
				SubstitutionKind::Alternate
			} else if reversal {
				// Reversal is valid only for single glyph substitutions
				// and VOLT ignores it otherwise.
				SubstitutionKind::ReverseChainingSingle
			} else {
				SubstitutionKind::Single
			}
		} else if max_src == 1 && max_dest > 1 {
			SubstitutionKind::Multiple
		} else if max_src > 1 && max_dest == 1 {
			SubstitutionKind::Ligature
		} else {
			return Err(Error::Python(
				"UnboundLocalError: cannot access local variable 'sub' where it is not associated with a value"
					.to_owned(),
			));
		};
		Ok(SubstitutionDefinition {
			kind,
			mapping,
			location,
		})
	}

	fn parse_position_(&mut self) -> Result<PositionDefinition> {
		self.assert_cur_keyword("AS_POSITION")?;
		let location = self.cur_token_location_.clone();
		let pos_type = self.expect_name_()?;
		let position = match pos_type.as_str() {
			"ATTACH" => PositionDefinition::Attach(self.parse_attach_()?),
			"ATTACH_CURSIVE" => PositionDefinition::AttachCursive(self.parse_attach_cursive_()?),
			"ADJUST_PAIR" => PositionDefinition::AdjustPair(self.parse_adjust_pair_()?),
			"ADJUST_SINGLE" => PositionDefinition::AdjustSingle(self.parse_adjust_single_()?),
			_ => {
				return Err(Error::volt_lib(
					"Expected ATTACH, ATTACH_CURSIVE, ADJUST_PAIR, ADJUST_SINGLE",
					location,
				));
			}
		};
		self.expect_keyword_("END_POSITION")?;
		Ok(position)
	}

	fn parse_attach_(&mut self) -> Result<PositionAttachDefinition> {
		self.assert_cur_keyword("ATTACH")?;
		let location = self.cur_token_location_.clone();
		let coverage = self.parse_coverage_()?;
		let mut coverage_to = Vec::new();
		self.expect_keyword_("TO")?;
		while !self.next_is("END_ATTACH") {
			let cov = self.parse_coverage_()?;
			self.expect_keyword_("AT")?;
			self.expect_keyword_("ANCHOR")?;
			let anchor_name = self.expect_string_()?;
			coverage_to.push((cov, anchor_name));
		}
		self.expect_keyword_("END_ATTACH")?;
		Ok(PositionAttachDefinition {
			coverage,
			coverage_to,
			location,
		})
	}

	fn parse_attach_cursive_(&mut self) -> Result<PositionAttachCursiveDefinition> {
		self.assert_cur_keyword("ATTACH_CURSIVE")?;
		let location = self.cur_token_location_.clone();
		let mut coverages_exit = Vec::new();
		let mut coverages_enter = Vec::new();
		while !self.next_is("ENTER") {
			self.expect_keyword_("EXIT")?;
			coverages_exit.push(self.parse_coverage_()?);
		}
		while !self.next_is("END_ATTACH") {
			self.expect_keyword_("ENTER")?;
			coverages_enter.push(self.parse_coverage_()?);
		}
		self.expect_keyword_("END_ATTACH")?;
		Ok(PositionAttachCursiveDefinition {
			coverages_exit,
			coverages_enter,
			location,
		})
	}

	fn parse_adjust_pair_(&mut self) -> Result<PositionAdjustPairDefinition> {
		self.assert_cur_keyword("ADJUST_PAIR")?;
		let location = self.cur_token_location_.clone();
		let mut coverages_1 = Vec::new();
		let mut coverages_2 = Vec::new();
		let mut adjust_pair = IndexMap::new();
		while self.next_is("FIRST") {
			self.advance_lexer_()?;
			coverages_1.push(self.parse_coverage_()?);
		}
		while self.next_is("SECOND") {
			self.advance_lexer_()?;
			coverages_2.push(self.parse_coverage_()?);
		}
		while !self.next_is("END_ADJUST") {
			let id_1 = self.expect_number_()?;
			let id_2 = self.expect_number_()?;
			self.expect_keyword_("BY")?;
			let pos_1 = self.parse_pos_()?;
			let pos_2 = self.parse_pos_()?;
			adjust_pair.insert((id_1, id_2), (pos_1, pos_2));
		}
		self.expect_keyword_("END_ADJUST")?;
		Ok(PositionAdjustPairDefinition {
			coverages_1,
			coverages_2,
			adjust_pair,
			location,
		})
	}

	fn parse_adjust_single_(&mut self) -> Result<PositionAdjustSingleDefinition> {
		self.assert_cur_keyword("ADJUST_SINGLE")?;
		let location = self.cur_token_location_.clone();
		let mut adjust_single = Vec::new();
		while !self.next_is("END_ADJUST") {
			let coverages = self.parse_coverage_()?;
			self.expect_keyword_("BY")?;
			let pos = self.parse_pos_()?;
			adjust_single.push((coverages, pos));
		}
		self.expect_keyword_("END_ADJUST")?;
		Ok(PositionAdjustSingleDefinition {
			adjust_single,
			location,
		})
	}

	fn parse_def_anchor_(&mut self) -> Result<AnchorDefinition> {
		self.assert_cur_keyword("DEF_ANCHOR")?;
		let location = self.cur_token_location_.clone();
		let name = self.expect_string_()?;
		self.expect_keyword_("ON")?;
		let gid = self.expect_number_()?;
		self.expect_keyword_("GLYPH")?;
		let glyph_name = self.expect_name_()?;
		self.expect_keyword_("COMPONENT")?;
		let component = self.expect_number_()?;
		// check for duplicate anchor names on this glyph
		if let Some(anchors) = self.anchors_.get(&glyph_name)
			&& anchors.resolve(&name, true) == Some(&component)
		{
			return Err(Error::volt_lib(
				format!("Anchor \"{name}\" already defined, anchor names are case insensitive"),
				location,
			));
		}
		let locked = if self.next_is("LOCKED") {
			self.advance_lexer_()?;
			true
		} else {
			false
		};
		self.expect_keyword_("AT")?;
		let pos = self.parse_pos_()?;
		self.expect_keyword_("END_ANCHOR")?;
		self.anchors_
			.entry(glyph_name.clone())
			.or_default()
			.define(&name, component);
		Ok(AnchorDefinition {
			name,
			gid,
			glyph_name,
			component,
			locked,
			pos,
			location,
		})
	}

	fn parse_adjust_by_(&mut self) -> Result<(i64, i64)> {
		self.advance_lexer_()?;
		self.assert_cur_keyword("ADJUST_BY")?;
		let adjustment = self.expect_number_()?;
		self.expect_keyword_("AT")?;
		let size = self.expect_number_()?;
		Ok((adjustment, size))
	}

	fn parse_pos_(&mut self) -> Result<Pos> {
		// VOLT syntax doesn't seem to take device Y advance
		self.advance_lexer_()?;
		if !self.is_cur_keyword_("POS") {
			let location = self
				.cur_token_location_
				.as_ref()
				.map(|l| format!("{:?}", (l.path.as_str(), l.line, l.column)))
				.unwrap_or_else(|| "None".to_owned());
			return Err(Error::Assertion(location));
		}
		let mut pos = Pos::default();
		if self.next_is("ADV") {
			self.advance_lexer_()?;
			pos.adv = Some(self.expect_number_()?);
			while self.next_is("ADJUST_BY") {
				let (adjustment, size) = self.parse_adjust_by_()?;
				pos.adv_adjust_by.insert(size, adjustment);
			}
		}
		if self.next_is("DX") {
			self.advance_lexer_()?;
			pos.dx = Some(self.expect_number_()?);
			while self.next_is("ADJUST_BY") {
				let (adjustment, size) = self.parse_adjust_by_()?;
				pos.dx_adjust_by.insert(size, adjustment);
			}
		}
		if self.next_is("DY") {
			self.advance_lexer_()?;
			pos.dy = Some(self.expect_number_()?);
			while self.next_is("ADJUST_BY") {
				let (adjustment, size) = self.parse_adjust_by_()?;
				pos.dy_adjust_by.insert(size, adjustment);
			}
		}
		self.expect_keyword_("END_POS")?;
		Ok(pos)
	}

	fn parse_unicode_values_(&mut self) -> Result<Option<Vec<i64>>> {
		let location = self.cur_token_location_.clone();
		let string = self.expect_string_()?;
		let mut unicode_values = Vec::new();
		for uni in string.split(',').filter(|u| !u.is_empty()) {
			let digits: String = uni.chars().skip(2).collect();
			match py_int_base16(&digits) {
				Some(v) => unicode_values.push(v),
				None => {
					return Err(Error::volt_lib(
						format!(
							"invalid literal for int() with base 16: {}",
							repr200(&digits)
						),
						location,
					));
				}
			}
		}
		Ok(if unicode_values.is_empty() {
			None
		} else {
			Some(unicode_values)
		})
	}

	fn parse_enum_(&mut self) -> Result<Enum> {
		self.expect_keyword_("ENUM")?;
		let location = self.cur_token_location_.clone();
		let enum_ = Enum {
			enum_: self.parse_coverage_()?,
			location,
		};
		self.expect_keyword_("END_ENUM")?;
		Ok(enum_)
	}

	fn parse_coverage_(&mut self) -> Result<Coverage> {
		let mut coverage = Vec::new();
		let location = self.cur_token_location_.clone();
		while self.next_is_any(&["GLYPH", "GROUP", "RANGE", "ENUM"]) {
			if self.next_is("ENUM") {
				coverage.push(CoverageItem::Enum(self.parse_enum_()?));
			} else if self.next_is("GLYPH") {
				self.expect_keyword_("GLYPH")?;
				let name = self.expect_string_()?;
				coverage.push(CoverageItem::GlyphName(GlyphName {
					glyph: name,
					location: location.clone(),
				}));
			} else if self.next_is("GROUP") {
				self.expect_keyword_("GROUP")?;
				let name = self.expect_string_()?;
				coverage.push(CoverageItem::GroupName(GroupName {
					group: name,
					tables: Rc::downgrade(&self.tables),
					location: location.clone(),
				}));
			} else {
				self.expect_keyword_("RANGE")?;
				let start = self.expect_string_()?;
				self.expect_keyword_("TO")?;
				let end = self.expect_string_()?;
				coverage.push(CoverageItem::Range(Range {
					start,
					end,
					tables: Rc::downgrade(&self.tables),
					location: location.clone(),
				}));
			}
		}
		Ok(coverage)
	}

	fn parse_ppem_(&mut self) -> Result<SettingDefinition> {
		let location = self.cur_token_location_.clone();
		let name = self.cur_text();
		let value = self.expect_number_()?;
		Ok(SettingDefinition {
			name,
			value: SettingValue::Number(value),
			location,
		})
	}

	fn parse_noarg_option_(&mut self) -> SettingDefinition {
		SettingDefinition {
			name: self.cur_text(),
			value: SettingValue::True,
			location: self.cur_token_location_.clone(),
		}
	}

	fn parse_cmap_format(&mut self) -> Result<SettingDefinition> {
		let location = self.cur_token_location_.clone();
		let name = self.cur_text();
		let value = vec![
			self.expect_number_()?,
			self.expect_number_()?,
			self.expect_number_()?,
		];
		Ok(SettingDefinition {
			name,
			value: SettingValue::Numbers(value),
			location,
		})
	}

	fn next_token_type(&self) -> Option<TokenType> {
		self.next_token_.as_ref().map(Token::token_type)
	}

	fn is_cur_keyword_(&self, k: &str) -> bool {
		matches!(&self.cur_token_, Some(Token::Name(n)) if n == k)
	}

	fn expect_string_(&mut self) -> Result<String> {
		self.advance_lexer_()?;
		match &self.cur_token_ {
			Some(Token::String(s)) => Ok(s.clone()),
			_ => Err(Error::volt_lib(
				"Expected a string",
				self.cur_token_location_.clone(),
			)),
		}
	}

	fn expect_keyword_(&mut self, keyword: &str) -> Result<String> {
		self.advance_lexer_()?;
		if self.is_cur_keyword_(keyword) {
			return Ok(keyword.to_owned());
		}
		Err(Error::volt_lib(
			format!("Expected \"{keyword}\""),
			self.cur_token_location_.clone(),
		))
	}

	fn expect_name_(&mut self) -> Result<String> {
		self.advance_lexer_()?;
		match &self.cur_token_ {
			Some(Token::Name(s)) => Ok(s.clone()),
			_ => Err(Error::volt_lib(
				"Expected a name",
				self.cur_token_location_.clone(),
			)),
		}
	}

	fn expect_number_(&mut self) -> Result<i64> {
		self.advance_lexer_()?;
		match &self.cur_token_ {
			Some(Token::Number(n)) => Ok(*n),
			_ => Err(Error::volt_lib(
				"Expected a number",
				self.cur_token_location_.clone(),
			)),
		}
	}

	fn advance_lexer_(&mut self) -> Result<()> {
		self.cur_token_ = self.next_token_.take();
		self.cur_token_location_ = self.next_token_location_.clone();
		if self.is_cur_keyword_("END") {
			return Ok(());
		}
		if let Some((token, location)) = self.lexer_.next().transpose()? {
			self.next_token_ = Some(token);
			self.next_token_location_ = Some(location);
		}
		Ok(())
	}
}
