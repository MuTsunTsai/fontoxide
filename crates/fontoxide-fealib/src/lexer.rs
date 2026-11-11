//! Tokenizing feature files (`fontTools.feaLib.lexer`).

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fontoxide_misc::py::{float_repr, is_space, str_repr};

use crate::error::{Error, Result};
use crate::location::FeatureLibLocation;

/// The type of a token (`Lexer.NUMBER`, `Lexer.NAME`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenType {
	/// `NUMBER`: a decimal integer.
	Number,
	/// `HEXADECIMAL`: an integer written `0x...`.
	Hexadecimal,
	/// `OCTAL`: an integer written with a leading zero.
	Octal,
	/// `FLOAT`.
	Float,
	/// `STRING`: a quoted string, without the quotes.
	String,
	/// `NAME`: a keyword or glyph name.
	Name,
	/// `FILENAME`: the file name of an `include` statement.
	Filename,
	/// `GLYPHCLASS`: a glyph class name, without the `@`.
	Glyphclass,
	/// `CID`: a CID written `\123`.
	Cid,
	/// `SYMBOL`: a single punctuation character.
	Symbol,
	/// `COMMENT`.
	Comment,
	/// `NEWLINE`.
	Newline,
	/// `ANONYMOUS_BLOCK`: the content of an `anon` block.
	AnonymousBlock,
}

impl TokenType {
	/// Whether this is one of `Lexer.NUMBERS`.
	pub fn is_number(self) -> bool {
		matches!(
			self,
			TokenType::Number | TokenType::Hexadecimal | TokenType::Octal
		)
	}

	/// The Python name, e.g. `NUMBER`.
	pub fn as_str(self) -> &'static str {
		match self {
			TokenType::Number => "NUMBER",
			TokenType::Hexadecimal => "HEXADECIMAL",
			TokenType::Octal => "OCTAL",
			TokenType::Float => "FLOAT",
			TokenType::String => "STRING",
			TokenType::Name => "NAME",
			TokenType::Filename => "FILENAME",
			TokenType::Glyphclass => "GLYPHCLASS",
			TokenType::Cid => "CID",
			TokenType::Symbol => "SYMBOL",
			TokenType::Comment => "COMMENT",
			TokenType::Newline => "NEWLINE",
			TokenType::AnonymousBlock => "ANONYMOUS_BLOCK",
		}
	}
}

impl fmt::Display for TokenType {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(self.as_str())
	}
}

/// The value of a token: `None`, an `int`, a `float` or a `str`.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Token {
	/// `None` (no token, or a newline).
	#[default]
	None,
	/// An integer (`NUMBER`, `HEXADECIMAL`, `OCTAL`, `CID`).
	///
	// LENIENT(L33): Python integers are unbounded; integers beyond `i64` are an error.
	Int(i64),
	/// A floating-point number (`FLOAT`).
	Float(f64),
	/// A string (every other token type).
	Str(String),
}

impl Token {
	/// Whether the token is the string `s` (`token == s`).
	pub fn is(&self, s: &str) -> bool {
		matches!(self, Token::Str(t) if t == s)
	}

	/// The token as a string slice, if it is a string.
	pub fn as_str(&self) -> Option<&str> {
		match self {
			Token::Str(s) => Some(s),
			_ => None,
		}
	}

	/// `str(token)`.
	pub fn py_str(&self) -> String {
		match self {
			Token::None => "None".to_owned(),
			Token::Int(i) => i.to_string(),
			Token::Float(f) => float_repr(*f),
			Token::Str(s) => s.clone(),
		}
	}

	/// `repr(token)`.
	pub fn py_repr(&self) -> String {
		match self {
			Token::Str(s) => str_repr(s),
			other => other.py_str(),
		}
	}
}

impl fmt::Display for Token {
	/// `str(token)`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.py_str())
	}
}

/// A token with its type and location, as yielded by the lexers.
pub type LexerToken = (TokenType, Token, FeatureLibLocation);

const CHAR_WHITESPACE_: &str = " \t";
const CHAR_NEWLINE_: &str = "\r\n";
const CHAR_SYMBOL_: &str = ",;:-+'{}[]<>()=";
const CHAR_DIGIT_: &str = "0123456789";
const CHAR_HEXDIGIT_: &str = "0123456789ABCDEFabcdef";
/// The characters that can start a name (`Lexer.CHAR_NAME_START_`).
pub const CHAR_NAME_START_: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_+*:.^~!\\";
/// The characters that can continue a name (`Lexer.CHAR_NAME_CONTINUATION_`).
pub const CHAR_NAME_CONTINUATION_: &str =
	"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_.+*:^~!/-";

/// `x in s` for an optional character, like `next_char in s` in Python, which
/// raises `TypeError` when `next_char` is `None`.
// UPSTREAM-BUG(B33): `Lexer.next_` tests `next_char in ...` after `cur_char == "0"`,
// `"\\"` or `"-"` without checking for the end of the text, so a feature file
// ending in one of those characters raises `TypeError`; it is reproduced as
// `Error::Value`.
fn opt_in(c: Option<char>, s: &str) -> Result<bool> {
	match c {
		Some(c) => Ok(s.contains(c)),
		None => Err(Error::Value(
			"'in <string>' requires string as left operand, not NoneType".to_owned(),
		)),
	}
}

/// `int(text, base)`, with Python's error message.
fn parse_int(text: &str, base: u32) -> Result<i64> {
	let digits = match base {
		16 => text
			.strip_prefix("0x")
			.or_else(|| text.strip_prefix("0X"))
			.unwrap_or(text),
		_ => text,
	};
	let (negative, digits) = match digits.strip_prefix('-') {
		Some(d) => (true, d),
		None => (false, digits),
	};
	let invalid = || {
		Error::Value(format!(
			"invalid literal for int() with base {base}: {}",
			str_repr(text)
		))
	};
	if digits.is_empty() || !digits.chars().all(|c| c.is_digit(base)) {
		return Err(invalid());
	}
	// LENIENT(L33): integers beyond `i64` are an error instead of Python big integers.
	let magnitude = i64::from_str_radix(digits, base)
		.map_err(|_| Error::Value(format!("integer {} is too large", str_repr(text))))?;
	Ok(if negative { -magnitude } else { magnitude })
}

/// `float(text)` for the digits-and-dots text the lexer scans.
fn parse_float(text: &str) -> Result<f64> {
	let valid = {
		let body = text.strip_prefix('-').unwrap_or(text);
		let mut parts = body.split('.');
		let int = parts.next().unwrap_or("");
		let frac = parts.next();
		parts.next().is_none()
			&& int.chars().all(|c| c.is_ascii_digit())
			&& frac.is_none_or(|f| f.chars().all(|c| c.is_ascii_digit()))
			&& (!int.is_empty() || frac.is_some_and(|f| !f.is_empty()))
	};
	if !valid {
		return Err(Error::Value(format!(
			"could not convert string to float: {}",
			str_repr(text)
		)));
	}
	text.parse::<f64>().map_err(|_| {
		Error::Value(format!(
			"could not convert string to float: {}",
			str_repr(text)
		))
	})
}

/// The lexer modes (`MODE_NORMAL_`, `MODE_FILENAME_`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
	Normal,
	Filename,
}

/// A lexer for one feature file (`Lexer`).
///
/// Positions and columns count characters (code points), like Python strings.
#[derive(Debug, Clone)]
pub struct Lexer {
	filename_: Option<String>,
	location_file: Arc<str>,
	line_: usize,
	pos_: usize,
	line_start_: usize,
	text_: Vec<char>,
	mode_: Mode,
}

impl Lexer {
	/// `Lexer(text, filename)`.
	pub fn new(text: &str, filename: Option<&str>) -> Self {
		Lexer {
			filename_: filename.map(str::to_owned),
			location_file: Arc::from(filename.unwrap_or("<features>")),
			line_: 1,
			pos_: 0,
			line_start_: 0,
			text_: text.chars().collect(),
			mode_: Mode::Normal,
		}
	}

	/// The file name given to the lexer (`filename_`).
	pub fn filename(&self) -> Option<&str> {
		self.filename_.as_deref()
	}

	/// The current position in characters (`pos_`).
	pub fn pos(&self) -> usize {
		self.pos_
	}

	/// The next token that is not a newline, or `None` at the end
	/// (`Lexer.__next__`).
	#[allow(clippy::should_implement_trait)]
	pub fn next(&mut self) -> Result<Option<LexerToken>> {
		loop {
			match self.next_()? {
				Some((TokenType::Newline, _, _)) => continue,
				other => return Ok(other),
			}
		}
	}

	/// Collects all remaining tokens (`list(lexer)`).
	pub fn tokens(&mut self) -> Result<Vec<LexerToken>> {
		let mut result = Vec::new();
		while let Some(token) = self.next()? {
			result.push(token);
		}
		Ok(result)
	}

	/// `location_()`.
	fn location_(&self) -> FeatureLibLocation {
		let column = self.pos_ - self.line_start_ + 1;
		FeatureLibLocation {
			file: self.location_file.clone(),
			line: self.line_,
			column,
		}
	}

	fn slice(&self, start: usize, end: usize) -> String {
		self.text_[start..end].iter().collect()
	}

	/// The next token including newlines, or `None` at the end (`next_`).
	pub fn next_(&mut self) -> Result<Option<LexerToken>> {
		self.scan_over_(CHAR_WHITESPACE_);
		let location = self.location_();
		let start = self.pos_;
		let limit = self.text_.len();
		if start >= limit {
			return Ok(None);
		}
		let cur_char = self.text_[start];
		let next_char = self.text_.get(start + 1).copied();

		if cur_char == '\n' {
			self.pos_ += 1;
			self.line_ += 1;
			self.line_start_ = self.pos_;
			return Ok(Some((TokenType::Newline, Token::None, location)));
		}
		if cur_char == '\r' {
			self.pos_ += if next_char == Some('\n') { 2 } else { 1 };
			self.line_ += 1;
			self.line_start_ = self.pos_;
			return Ok(Some((TokenType::Newline, Token::None, location)));
		}
		if cur_char == '#' {
			self.scan_until_(CHAR_NEWLINE_);
			return Ok(Some((
				TokenType::Comment,
				Token::Str(self.slice(start, self.pos_)),
				location,
			)));
		}

		if self.mode_ == Mode::Filename {
			if cur_char != '(' {
				return Err(Error::feature_lib(
					"Expected '(' before file name",
					Some(location),
				));
			}
			self.scan_until_(")");
			if self.text_.get(self.pos_) != Some(&')') {
				return Err(Error::feature_lib(
					"Expected ')' after file name",
					Some(location),
				));
			}
			self.pos_ += 1;
			self.mode_ = Mode::Normal;
			return Ok(Some((
				TokenType::Filename,
				Token::Str(self.slice(start + 1, self.pos_ - 1)),
				location,
			)));
		}

		if cur_char == '\\' && opt_in(next_char, CHAR_DIGIT_)? {
			self.pos_ += 1;
			self.scan_over_(CHAR_DIGIT_);
			let cid = parse_int(&self.slice(start + 1, self.pos_), 10)?;
			return Ok(Some((TokenType::Cid, Token::Int(cid), location)));
		}
		if cur_char == '@' {
			self.pos_ += 1;
			self.scan_over_(CHAR_NAME_CONTINUATION_);
			let glyphclass = self.slice(start + 1, self.pos_);
			if glyphclass.is_empty() {
				return Err(Error::feature_lib(
					"Expected glyph class name",
					Some(location),
				));
			}
			// `RE_GLYPHCLASS`: `^[A-Za-z_0-9.\-]+$`
			if !glyphclass
				.chars()
				.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
			{
				return Err(Error::feature_lib(
					"Glyph class names must consist of letters, digits, \
					 underscore, period or hyphen",
					Some(location),
				));
			}
			return Ok(Some((
				TokenType::Glyphclass,
				Token::Str(glyphclass),
				location,
			)));
		}
		if CHAR_NAME_START_.contains(cur_char) {
			self.pos_ += 1;
			self.scan_over_(CHAR_NAME_CONTINUATION_);
			let token = self.slice(start, self.pos_);
			if token == "include" {
				self.mode_ = Mode::Filename;
			}
			return Ok(Some((TokenType::Name, Token::Str(token), location)));
		}
		if cur_char == '0' && opt_in(next_char, "xX")? {
			self.pos_ += 2;
			self.scan_over_(CHAR_HEXDIGIT_);
			let value = parse_int(&self.slice(start, self.pos_), 16)?;
			return Ok(Some((TokenType::Hexadecimal, Token::Int(value), location)));
		}
		if cur_char == '0' && opt_in(next_char, CHAR_DIGIT_)? {
			self.scan_over_(CHAR_DIGIT_);
			let value = parse_int(&self.slice(start, self.pos_), 8)?;
			return Ok(Some((TokenType::Octal, Token::Int(value), location)));
		}
		if CHAR_DIGIT_.contains(cur_char) {
			self.scan_over_(CHAR_DIGIT_);
			if self.pos_ >= limit || self.text_[self.pos_] != '.' {
				let value = parse_int(&self.slice(start, self.pos_), 10)?;
				return Ok(Some((TokenType::Number, Token::Int(value), location)));
			}
			self.scan_over_(".");
			self.scan_over_(CHAR_DIGIT_);
			let value = parse_float(&self.slice(start, self.pos_))?;
			return Ok(Some((TokenType::Float, Token::Float(value), location)));
		}
		if cur_char == '-' && opt_in(next_char, CHAR_DIGIT_)? {
			self.pos_ += 1;
			self.scan_over_(CHAR_DIGIT_);
			if self.pos_ >= limit || self.text_[self.pos_] != '.' {
				let value = parse_int(&self.slice(start, self.pos_), 10)?;
				return Ok(Some((TokenType::Number, Token::Int(value), location)));
			}
			self.scan_over_(".");
			self.scan_over_(CHAR_DIGIT_);
			let value = parse_float(&self.slice(start, self.pos_))?;
			return Ok(Some((TokenType::Float, Token::Float(value), location)));
		}
		if CHAR_SYMBOL_.contains(cur_char) {
			self.pos_ += 1;
			return Ok(Some((
				TokenType::Symbol,
				Token::Str(cur_char.to_string()),
				location,
			)));
		}
		if cur_char == '"' {
			self.pos_ += 1;
			self.scan_until_("\"");
			if self.pos_ < limit && self.text_[self.pos_] == '"' {
				self.pos_ += 1;
				// strip newlines embedded within a string
				let string: String = self.text_[start + 1..self.pos_ - 1]
					.iter()
					.filter(|&&c| c != '\r' && c != '\n')
					.collect();
				return Ok(Some((TokenType::String, Token::Str(string), location)));
			}
			return Err(Error::feature_lib(
				"Expected '\"' to terminate string",
				Some(location),
			));
		}
		Err(Error::feature_lib(
			format!("Unexpected character: {}", str_repr(&cur_char.to_string())),
			Some(location),
		))
	}

	/// Advances over the characters in `valid` (`scan_over_`).
	pub fn scan_over_(&mut self, valid: &str) {
		let mut p = self.pos_;
		while p < self.text_.len() && valid.contains(self.text_[p]) {
			p += 1;
		}
		self.pos_ = p;
	}

	/// Advances up to the first character in `stop_at` (`scan_until_`).
	pub fn scan_until_(&mut self, stop_at: &str) {
		let mut p = self.pos_;
		while p < self.text_.len() && !stop_at.contains(self.text_[p]) {
			p += 1;
		}
		self.pos_ = p;
	}

	/// Scans the content of an anonymous block up to `} tag;`
	/// (`scan_anonymous_block`).
	///
	/// Like fontTools, the line count is not advanced over the content.
	pub fn scan_anonymous_block(&mut self, tag: &str) -> Result<LexerToken> {
		let location = self.location_();
		let tag: Vec<char> = fontoxide_misc::py::strip(tag).chars().collect();
		self.scan_until_(CHAR_NEWLINE_);
		self.scan_over_(CHAR_NEWLINE_);
		// `re.split(r"}\s*" + re.escape(tag) + r"\s*;", text, maxsplit=1)`
		let text = &self.text_[self.pos_..];
		let found = (0..text.len()).find(|&start| {
			if text[start] != '}' {
				return false;
			}
			let mut p = start + 1;
			while p < text.len() && is_space(text[p]) {
				p += 1;
			}
			if !text[p..].starts_with(&tag) {
				return false;
			}
			p += tag.len();
			while p < text.len() && is_space(text[p]) {
				p += 1;
			}
			p < text.len() && text[p] == ';'
		});
		let Some(end) = found else {
			return Err(Error::feature_lib(
				format!(
					"Expected '}} {};' to terminate anonymous block",
					tag.iter().collect::<String>()
				),
				Some(location),
			));
		};
		let content: String = text[..end].iter().collect();
		self.pos_ += end;
		Ok((TokenType::AnonymousBlock, Token::Str(content), location))
	}
}

/// Where a feature file comes from: a path, or text in memory (a file object).
#[derive(Debug, Clone)]
pub enum FeatureSource {
	/// A path to open (a `str` passed as `featurefile`).
	Path(PathBuf),
	/// Text that has already been read (a file object such as `StringIO`), with
	/// the file object's `name`, if it has one.
	Text {
		/// The text.
		text: String,
		/// The name of the file object.
		name: Option<String>,
	},
}

impl FeatureSource {
	/// Text in memory without a file name (`StringIO(text)`).
	pub fn text(text: impl Into<String>) -> Self {
		FeatureSource::Text {
			text: text.into(),
			name: None,
		}
	}
}

impl From<PathBuf> for FeatureSource {
	fn from(path: PathBuf) -> Self {
		FeatureSource::Path(path)
	}
}

impl From<&Path> for FeatureSource {
	fn from(path: &Path) -> Self {
		FeatureSource::Path(path.to_path_buf())
	}
}

/// Reads a feature file like `open(path, "r", encoding="utf-8-sig")`: a leading
/// byte order mark is dropped and newlines are translated to `\n` (universal
/// newlines mode).
fn read_feature_file(path: &Path) -> std::io::Result<String> {
	let bytes = std::fs::read(path)?;
	let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
	let text = std::str::from_utf8(bytes)
		.map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
	Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// `IncludingLexer.make_lexer_(file_or_path)`.
fn make_lexer_(source: &FeatureSource) -> std::io::Result<Lexer> {
	Ok(match source {
		FeatureSource::Path(path) => {
			let text = read_feature_file(path)?;
			Lexer::new(&text, Some(&path.to_string_lossy()))
		}
		FeatureSource::Text { text, name } => Lexer::new(text, name.as_deref()),
	})
}

/// A lexer that follows include statements (`IncludingLexer`).
///
/// The OpenType feature file specification states that due to historical
/// reasons, relative imports should be resolved in this order:
///
/// 1. If the source font is UFO format, then relative to the UFO's font
///    directory
/// 2. relative to the top-level include file
/// 3. relative to the parent include file
///
/// Only 1 (via `include_dir`) and 2 are supported.
#[derive(Debug, Clone)]
pub struct IncludingLexer {
	lexers_: Vec<Lexer>,
	/// The path of the top-level file, or `None` for text in memory
	/// (`featurefilepath`).
	pub featurefilepath: Option<String>,
	/// The directory relative includes are resolved against (`includeDir`).
	pub include_dir: Option<PathBuf>,
}

impl IncludingLexer {
	/// `IncludingLexer(featurefile, includeDir=include_dir)`.
	///
	/// If `include_dir` is given, it is used as the directory of all relative
	/// include statements. Otherwise the directory of the top-level file is used,
	/// or the current working directory for text in memory.
	pub fn new(
		featurefile: impl Into<FeatureSource>,
		include_dir: Option<PathBuf>,
	) -> Result<Self> {
		let source = featurefile.into();
		let lexer = make_lexer_(&source).map_err(|source_err| Error::Io {
			path: match &source {
				FeatureSource::Path(p) => p.to_string_lossy().into_owned(),
				FeatureSource::Text { name, .. } => name.clone().unwrap_or_default(),
			},
			source: source_err,
		})?;
		let featurefilepath = lexer.filename_.clone();
		Ok(IncludingLexer {
			lexers_: vec![lexer],
			featurefilepath,
			include_dir,
		})
	}

	/// The next token, following includes; `None` at the end (`__next__`).
	#[allow(clippy::should_implement_trait)]
	pub fn next(&mut self) -> Result<Option<LexerToken>> {
		while let Some(lexer) = self.lexers_.last_mut() {
			let Some((token_type, token, location)) = lexer.next()? else {
				self.lexers_.pop();
				continue;
			};
			if token_type == TokenType::Name && token.is("include") {
				let fname = lexer.next()?;
				let (fname_type, fname_token, fname_location) = match fname {
					Some(t) => t,
					// UPSTREAM-BUG(B35): at the end of the text, `lexer.next()` raises
					// `StopIteration`, which escapes `__next__` and ends this round of
					// iteration as if all files had ended; reproduced.
					None => return Ok(None),
				};
				if fname_type != TokenType::Filename {
					return Err(Error::feature_lib(
						"Expected file name",
						Some(fname_location),
					));
				}
				let fname_token = fname_token.py_str();
				let path = if Path::new(&fname_token).is_absolute() {
					PathBuf::from(&fname_token)
				} else {
					let curpath = if let Some(dir) = &self.include_dir {
						dir.clone()
					} else if let Some(featurefilepath) = &self.featurefilepath {
						Path::new(featurefilepath)
							.parent()
							.map(Path::to_path_buf)
							.unwrap_or_default()
					} else {
						// If the IncludingLexer was initialized from an in-memory
						// file-like stream, it doesn't have a 'name' pointing to
						// its filesystem path, therefore we fall back to using the
						// current working directory to resolve relative includes.
						std::env::current_dir().map_err(|e| Error::Io {
							path: ".".to_owned(),
							source: e,
						})?
					};
					curpath.join(&fname_token)
				};
				if self.lexers_.len() >= 5 {
					return Err(Error::feature_lib(
						"Too many recursive includes",
						Some(fname_location),
					));
				}
				match make_lexer_(&FeatureSource::Path(path.clone())) {
					Ok(lexer) => self.lexers_.push(lexer),
					Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
						return Err(Error::IncludedFeaNotFound {
							filename: fname_token,
							location: fname_location,
							source: e,
						});
					}
					Err(e) => {
						return Err(Error::Io {
							path: path.to_string_lossy().into_owned(),
							source: e,
						});
					}
				}
			} else {
				return Ok(Some((token_type, token, location)));
			}
		}
		Ok(None)
	}

	/// Collects all remaining tokens (`list(lexer)`).
	pub fn tokens(&mut self) -> Result<Vec<LexerToken>> {
		let mut result = Vec::new();
		while let Some(token) = self.next()? {
			result.push(token);
		}
		Ok(result)
	}

	/// `scan_anonymous_block(tag)` on the innermost file.
	pub fn scan_anonymous_block(&mut self, tag: &str) -> Result<LexerToken> {
		match self.lexers_.last_mut() {
			Some(lexer) => lexer.scan_anonymous_block(tag),
			None => Err(Error::Value("list index out of range".to_owned())),
		}
	}
}

/// A lexer that does not follow `include` statements but emits them as they are
/// (`NonIncludingLexer`).
#[derive(Debug, Clone)]
pub struct NonIncludingLexer {
	inner: IncludingLexer,
}

impl NonIncludingLexer {
	/// `NonIncludingLexer(featurefile, includeDir=include_dir)`.
	pub fn new(
		featurefile: impl Into<FeatureSource>,
		include_dir: Option<PathBuf>,
	) -> Result<Self> {
		Ok(NonIncludingLexer {
			inner: IncludingLexer::new(featurefile, include_dir)?,
		})
	}

	/// The path of the top-level file (`featurefilepath`).
	pub fn featurefilepath(&self) -> Option<&str> {
		self.inner.featurefilepath.as_deref()
	}

	/// The next token; `None` at the end (`__next__`).
	#[allow(clippy::should_implement_trait)]
	pub fn next(&mut self) -> Result<Option<LexerToken>> {
		match self.inner.lexers_.first_mut() {
			Some(lexer) => lexer.next(),
			None => Ok(None),
		}
	}

	/// `scan_anonymous_block(tag)`.
	pub fn scan_anonymous_block(&mut self, tag: &str) -> Result<LexerToken> {
		self.inner.scan_anonymous_block(tag)
	}
}
