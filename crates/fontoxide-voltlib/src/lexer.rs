//! The lexer of VOLT projects (`fontTools.voltLib.lexer`).

use crate::error::{Error, Result, VoltLibLocation};

/// The type of a token (`Lexer.NUMBER`, `Lexer.STRING`, `Lexer.NAME`,
/// `Lexer.NEWLINE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
	/// `NUMBER`.
	Number,
	/// `STRING`.
	String,
	/// `NAME`.
	Name,
	/// `NEWLINE`.
	Newline,
}

/// A token and its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
	/// A number.
	Number(i64),
	/// A string, without the quotes.
	String(String),
	/// A name (a keyword or a bare glyph name).
	Name(String),
	/// A line break, which the iterator skips.
	Newline,
}

impl Token {
	/// The type of the token.
	pub fn token_type(&self) -> TokenType {
		match self {
			Token::Number(_) => TokenType::Number,
			Token::String(_) => TokenType::String,
			Token::Name(_) => TokenType::Name,
			Token::Newline => TokenType::Newline,
		}
	}

	/// The text of a string or name token, which Python compares with `==` to
	/// keywords regardless of the token type.
	pub fn text(&self) -> Option<&str> {
		match self {
			Token::String(s) | Token::Name(s) => Some(s),
			_ => None,
		}
	}
}

const CHAR_WHITESPACE_: &str = " \t";
const CHAR_DIGIT_: &str = "0123456789";

fn is_name_start(c: char) -> bool {
	c.is_ascii_alphabetic() || c == '.' || c == '_'
}

fn is_name_continuation(c: char) -> bool {
	is_name_start(c) || c.is_ascii_digit()
}

/// The lexer (`Lexer`): iterates over the tokens of a VOLT project, without the
/// line breaks.
#[derive(Debug, Clone)]
pub struct Lexer {
	filename_: Option<String>,
	line_: usize,
	pos_: usize,
	line_start_: usize,
	text_: Vec<char>,
	failed: bool,
}

impl Lexer {
	/// `Lexer(text, filename)`.
	pub fn new(text: &str, filename: Option<&str>) -> Self {
		Lexer {
			filename_: filename.map(str::to_owned),
			line_: 1,
			pos_: 0,
			line_start_: 0,
			text_: text.chars().collect(),
			failed: false,
		}
	}

	fn location_(&self) -> VoltLibLocation {
		VoltLibLocation {
			path: self
				.filename_
				.clone()
				.unwrap_or_else(|| "<volt>".to_owned()),
			line: self.line_,
			column: self.pos_ - self.line_start_ + 1,
		}
	}

	/// The next token, including line breaks (`next_`); `None` at the end.
	pub fn next_(&mut self) -> Result<Option<(Token, VoltLibLocation)>> {
		self.scan_over_(|c| CHAR_WHITESPACE_.contains(c));
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
			return Ok(Some((Token::Newline, location)));
		}
		if cur_char == '\r' {
			self.pos_ += if next_char == Some('\n') { 2 } else { 1 };
			self.line_ += 1;
			self.line_start_ = self.pos_;
			return Ok(Some((Token::Newline, location)));
		}
		if cur_char == '"' {
			self.pos_ += 1;
			self.scan_until_(|c| "\"\r\n".contains(c));
			if self.pos_ < limit && self.text_[self.pos_] == '"' {
				self.pos_ += 1;
				let s: String = self.text_[start + 1..self.pos_ - 1].iter().collect();
				return Ok(Some((Token::String(s), location)));
			}
			return Err(Error::volt_lib(
				"Expected '\"' to terminate string",
				Some(location),
			));
		}
		if is_name_start(cur_char) {
			self.pos_ += 1;
			self.scan_over_(is_name_continuation);
			let token: String = self.text_[start..self.pos_].iter().collect();
			return Ok(Some((Token::Name(token), location)));
		}
		// LENIENT(L42): a "-" at the very end of the text is an unexpected character;
		// fontTools raises a TypeError (`None in "0123456789"`).
		let negative = cur_char == '-' && next_char.is_some_and(|c| CHAR_DIGIT_.contains(c));
		if CHAR_DIGIT_.contains(cur_char) || negative {
			if negative {
				self.pos_ += 1;
			}
			self.scan_over_(|c| CHAR_DIGIT_.contains(c));
			let text: String = self.text_[start..self.pos_].iter().collect();
			// LENIENT(L42): numbers beyond 64 bits are an error; Python has big integers.
			let value = text.parse::<i64>().map_err(|_| {
				Error::volt_lib(
					format!("Number out of range: {text}"),
					Some(location.clone()),
				)
			})?;
			return Ok(Some((Token::Number(value), location)));
		}
		Err(Error::volt_lib(
			format!("Unexpected character: '{cur_char}'"),
			Some(location),
		))
	}

	fn scan_over_(&mut self, valid: impl Fn(char) -> bool) {
		let mut p = self.pos_;
		while p < self.text_.len() && valid(self.text_[p]) {
			p += 1;
		}
		self.pos_ = p;
	}

	fn scan_until_(&mut self, stop_at: impl Fn(char) -> bool) {
		let mut p = self.pos_;
		while p < self.text_.len() && !stop_at(self.text_[p]) {
			p += 1;
		}
		self.pos_ = p;
	}
}

impl Iterator for Lexer {
	type Item = Result<(Token, VoltLibLocation)>;

	/// `__next__`: the next token that is not a line break. Iteration stops
	/// after an error.
	fn next(&mut self) -> Option<Self::Item> {
		if self.failed {
			return None;
		}
		loop {
			match self.next_() {
				Ok(Some((Token::Newline, _))) => continue,
				Ok(Some(t)) => return Some(Ok(t)),
				Ok(None) => return None,
				Err(e) => {
					self.failed = true;
					return Some(Err(e));
				}
			}
		}
	}
}
