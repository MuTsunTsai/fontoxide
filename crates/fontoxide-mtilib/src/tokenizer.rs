//! The line tokenizer of the source files (`Tokenizer`).

use fontoxide_misc::py;

use crate::{Error, Result};

/// A line: its tab-separated fields, stripped (trailing empty fields removed).
pub(crate) type Line = Vec<String>;

/// Reads the lines of a source file, skipping empty lines and comments, and
/// stops at "stopper" lines (`Tokenizer`).
///
/// fontTools' iterator protocol becomes `Option`: `None` where fontTools raises
/// `StopIteration`, either at the end of the input or at a stopper line, which
/// stays buffered.
#[derive(Debug)]
pub(crate) struct Tokenizer<'a> {
	lines: Vec<&'a str>,
	pos: usize,
	lineno: usize,
	stoppers: Vec<String>,
	buffer: Option<Line>,
}

/// Splits text into lines as Python's text files do (universal newlines).
fn split_lines(text: &str) -> Vec<&str> {
	let mut lines = Vec::new();
	let bytes = text.as_bytes();
	let mut start = 0;
	let mut i = 0;
	while i < bytes.len() {
		match bytes[i] {
			b'\n' => {
				lines.push(&text[start..i]);
				i += 1;
				start = i;
			}
			b'\r' => {
				lines.push(&text[start..i]);
				i += if bytes.get(i + 1) == Some(&b'\n') {
					2
				} else {
					1
				};
				start = i;
			}
			_ => i += 1,
		}
	}
	if start < bytes.len() {
		lines.push(&text[start..]);
	}
	lines
}

impl<'a> Tokenizer<'a> {
	/// A tokenizer of the lines of `text`.
	pub fn new(text: &'a str) -> Self {
		Tokenizer {
			lines: split_lines(text),
			pos: 0,
			lineno: 0,
			stoppers: Vec::new(),
			buffer: None,
		}
	}

	/// `_next_line`.
	fn next_line(&mut self) -> Option<Line> {
		let raw = *self.lines.get(self.pos)?;
		self.pos += 1;
		self.lineno += 1;
		let mut line: Line = raw.split('\t').map(|s| py::strip(s).to_owned()).collect();
		if line.len() == 1 && line[0].is_empty() {
			line.clear();
		}
		if line.last().is_some_and(String::is_empty) {
			log::warn!("trailing tab found on line {}: {raw}\n", self.lineno);
			while line.last().is_some_and(String::is_empty) {
				line.pop();
			}
		}
		Some(line)
	}

	/// `_next_nonempty`: skips empty lines and comments (but not `% subtable`).
	fn next_nonempty(&mut self) -> Option<Line> {
		loop {
			let line = self.next_line()?;
			if let Some(first) = line.first()
				&& !first.is_empty()
				&& (!first.starts_with('%') || first == "% subtable")
			{
				return Some(line);
			}
		}
	}

	/// `_next_buffered`.
	fn next_buffered(&mut self) -> Option<Line> {
		match self.buffer.take() {
			Some(line) => Some(line),
			None => self.next_nonempty(),
		}
	}

	fn is_stopper(&self, line: &Line) -> bool {
		let tag = line[0].to_lowercase();
		self.stoppers.contains(&tag)
	}

	/// `__next__`: the next line, `None` at the end or at a stopper.
	pub fn next(&mut self) -> Option<Line> {
		let line = self.next_buffered()?;
		if self.is_stopper(&line) {
			self.buffer = Some(line);
			return None;
		}
		Some(line)
	}

	/// `next(lines)` where fontTools lets `StopIteration` escape.
	pub fn next_required(&mut self) -> Result<Line> {
		self.next().ok_or_else(stop_iteration)
	}

	/// `peek`: the next line without consuming it, `None` at the end or at a
	/// stopper.
	pub fn peek(&mut self) -> Option<&Line> {
		if self.buffer.is_none() {
			self.buffer = Some(self.next_nonempty()?);
		}
		let line = self.buffer.as_ref()?;
		if self.is_stopper(line) {
			return None;
		}
		self.buffer.as_ref()
	}

	/// `peeks()[0]`: the first field of the next line, `""` if there is none.
	pub fn peeks0(&mut self) -> String {
		self.peek().map_or_else(String::new, |l| l[0].clone())
	}

	/// The start of `between(tag)`: expects `<...>tag begin` and stops at
	/// `tag end`.
	pub fn begin_between(&mut self, tag: &str) -> Result<()> {
		self.expect_ends_with(&format!("{tag} begin"))?;
		self.stoppers.push(format!("{tag} end"));
		Ok(())
	}

	/// The end of `between(tag)`: expects `tag end`.
	pub fn end_between(&mut self, tag: &str) -> Result<()> {
		self.stoppers.pop();
		self.expect(&format!("{tag} end"))?;
		Ok(())
	}

	/// The start of `until(tags)`.
	pub fn push_until(&mut self, tags: &[&str]) {
		self.stoppers.extend(tags.iter().map(|t| (*t).to_owned()));
	}

	/// The end of `until(tags)`.
	pub fn pop_until(&mut self, count: usize) {
		let len = self.stoppers.len().saturating_sub(count);
		self.stoppers.truncate(len);
	}

	/// `expect`: the next line, whose first field must be `s` (ignoring case).
	pub fn expect(&mut self, s: &str) -> Result<Line> {
		let line = self.next_required()?;
		let tag = line[0].to_lowercase();
		if tag != s {
			return Err(Error::Assertion(format!("Expected '{s}', got '{tag}'")));
		}
		Ok(line)
	}

	/// `expectendswith`: the next line, whose first field must end with `s`
	/// (ignoring case).
	pub fn expect_ends_with(&mut self, s: &str) -> Result<Line> {
		let line = self.next_required()?;
		let tag = line[0].to_lowercase();
		if !tag.ends_with(s) {
			return Err(Error::Assertion(format!("Expected '*{s}', got '{tag}'")));
		}
		Ok(line)
	}

	/// Where the tokenizer is, to detect loops that make no progress.
	pub fn position(&self) -> (usize, Option<&Line>) {
		(self.pos, self.buffer.as_ref())
	}
}

/// The `StopIteration` that escapes where a line was required.
pub(crate) fn stop_iteration() -> Error {
	Error::Value("StopIteration: unexpected end of the input".into())
}
