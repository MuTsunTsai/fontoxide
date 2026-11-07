//! Port of `fontTools.misc.xmlWriter`: the XML writer used for TTX.
//!
//! The output format (indentation, attribute quoting, escaping, hex dumps) matches
//! fontTools exactly, since TTX dumps are compared byte for byte.

use std::borrow::Cow;
use std::fmt;

/// Default indentation (`INDENT`).
pub const INDENT: &str = "  ";

/// Replacement for characters that are not allowed in XML 1.0.
const REPLACEMENT: char = '?';

/// A value that can be written as an XML attribute; mirrors `str(value)` in Python.
pub trait AttrValue {
	/// The attribute text, before escaping.
	fn attr_string(&self) -> Cow<'_, str>;
}

impl AttrValue for str {
	fn attr_string(&self) -> Cow<'_, str> {
		Cow::Borrowed(self)
	}
}

impl AttrValue for String {
	fn attr_string(&self) -> Cow<'_, str> {
		Cow::Borrowed(self)
	}
}

impl AttrValue for Cow<'_, str> {
	fn attr_string(&self) -> Cow<'_, str> {
		Cow::Borrowed(self)
	}
}

impl AttrValue for bool {
	fn attr_string(&self) -> Cow<'_, str> {
		Cow::Borrowed(if *self { "True" } else { "False" })
	}
}

impl AttrValue for f64 {
	fn attr_string(&self) -> Cow<'_, str> {
		Cow::Owned(crate::py::float_repr(*self))
	}
}

impl AttrValue for f32 {
	fn attr_string(&self) -> Cow<'_, str> {
		Cow::Owned(crate::py::float_repr(*self as f64))
	}
}

macro_rules! int_attr {
	($($t:ty),*) => {$(
		impl AttrValue for $t {
			fn attr_string(&self) -> Cow<'_, str> {
				Cow::Owned(self.to_string())
			}
		}
	)*};
}
int_attr!(
	u8, i8, u16, i16, u32, i32, u64, i64, u128, i128, usize, isize
);

impl<T: AttrValue + ?Sized> AttrValue for &T {
	fn attr_string(&self) -> Cow<'_, str> {
		(**self).attr_string()
	}
}

/// A list of attributes: `(name, value)` pairs.
pub type Attrs<'a> = [(&'a str, &'a dyn AttrValue)];

/// Writes TTX-style XML into a string (`XMLWriter`).
pub struct XmlWriter {
	out: String,
	indent_white: String,
	newline_str: String,
	indent_level: usize,
	stack: Vec<String>,
	need_indent: bool,
}

impl fmt::Debug for XmlWriter {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("XmlWriter")
			.field("len", &self.out.len())
			.field("indent_level", &self.indent_level)
			.field("stack", &self.stack)
			.finish()
	}
}

impl Default for XmlWriter {
	fn default() -> Self {
		Self::new()
	}
}

impl XmlWriter {
	/// A writer with the default indentation and `"\n"` line endings, which has
	/// already written the XML declaration.
	pub fn new() -> Self {
		Self::with_options(INDENT, "\n")
	}

	/// A writer with custom indentation and line endings (`indentwhite`,
	/// `newlinestr`).
	pub fn with_options(indent_white: &str, newline_str: &str) -> Self {
		let mut w = XmlWriter {
			out: String::new(),
			indent_white: indent_white.into(),
			newline_str: newline_str.into(),
			indent_level: 0,
			stack: Vec::new(),
			need_indent: true,
		};
		w.write_raw("<?xml version=\"1.0\" encoding=\"UTF-8\"?>", true);
		w.newline();
		w
	}

	/// The line ending in use.
	pub fn newline_str(&self) -> &str {
		&self.newline_str
	}

	/// The output so far.
	pub fn as_str(&self) -> &str {
		&self.out
	}

	/// Consumes the writer and returns its output.
	pub fn into_string(self) -> String {
		self.out
	}

	/// Writes escaped text (`write`).
	pub fn write(&mut self, s: &str) {
		let escaped = escape(s);
		self.write_raw(&escaped, true);
	}

	/// Writes escaped text, optionally without indentation (`write(s, indent=…)`).
	pub fn write_with_indent(&mut self, s: &str, indent: bool) {
		let escaped = escape(s);
		self.write_raw(&escaped, indent);
	}

	/// Writes text in a CDATA section (`writecdata`).
	pub fn writecdata(&mut self, s: &str) {
		let s = s.replace("]]>", "]]]]><![CDATA[>");
		self.write_raw(&format!("<![CDATA[{s}]]>"), true);
	}

	/// Writes bytes, escaping everything outside printable ASCII as a character
	/// reference, so that the bytes can be recovered by encoding to Latin-1
	/// (`write8bit`).
	pub fn write8bit(&mut self, data: &[u8], strip: bool) {
		let s = escape8bit(data);
		let s = if strip { s.trim() } else { &s };
		self.write_raw(s, true);
	}

	/// Writes escaped text without indentation (`write_noindent`).
	pub fn write_noindent(&mut self, s: &str) {
		let escaped = escape(s);
		self.write_raw(&escaped, false);
	}

	/// Writes text as-is, indenting first if at the start of a line (`_writeraw`).
	pub fn write_raw(&mut self, data: &str, indent: bool) {
		if indent && self.need_indent {
			for _ in 0..self.indent_level {
				self.out.push_str(&self.indent_white);
			}
			self.need_indent = false;
		}
		self.out.push_str(data);
	}

	/// Ends the line (`newline`).
	pub fn newline(&mut self) {
		self.out.push_str(&self.newline_str);
		self.need_indent = true;
	}

	/// Writes a comment; continuation lines are indented to align (`comment`).
	pub fn comment(&mut self, data: &str) {
		let data = escape(data);
		let mut lines = data.split('\n');
		let first = lines.next().unwrap_or_default();
		self.write_raw(&format!("<!-- {first}"), true);
		for line in lines {
			self.newline();
			self.write_raw(&format!("     {line}"), true);
		}
		self.write_raw(" -->", true);
	}

	/// Writes an empty element, with attributes sorted by name as when fontTools
	/// passes them as keyword arguments (`simpletag(tag, **kwargs)`).
	pub fn simpletag(&mut self, tag: &str, attrs: &Attrs<'_>) {
		let data = format!("<{tag}{}/>", stringify_attrs_sorted(attrs));
		self.write_raw(&data, true);
	}

	/// Writes an empty element, keeping the attribute order
	/// (`simpletag(tag, [(name, value), …])`).
	pub fn simpletag_ordered(&mut self, tag: &str, attrs: &Attrs<'_>) {
		let data = format!("<{tag}{}/>", stringify_attrs(attrs));
		self.write_raw(&data, true);
	}

	/// Opens an element, with attributes sorted by name (`begintag(tag, **kwargs)`).
	pub fn begintag(&mut self, tag: &str, attrs: &Attrs<'_>) {
		let data = format!("<{tag}{}>", stringify_attrs_sorted(attrs));
		self.open(tag, &data);
	}

	/// Opens an element, keeping the attribute order
	/// (`begintag(tag, [(name, value), …])`).
	pub fn begintag_ordered(&mut self, tag: &str, attrs: &Attrs<'_>) {
		let data = format!("<{tag}{}>", stringify_attrs(attrs));
		self.open(tag, &data);
	}

	fn open(&mut self, tag: &str, data: &str) {
		self.write_raw(data, true);
		self.stack.push(tag.into());
		self.indent();
	}

	/// Closes the innermost element (`endtag`).
	///
	/// # Panics
	///
	/// Panics if `tag` does not match the open element, which is a programming
	/// error (fontTools asserts the same).
	pub fn endtag(&mut self, tag: &str) {
		assert!(
			self.stack.last().is_some_and(|t| t == tag),
			"nonmatching endtag: {tag}"
		);
		self.stack.pop();
		self.dedent();
		self.write_raw(&format!("</{tag}>"), true);
	}

	/// Writes binary data as lines of hex digits, 16 bytes per line in groups of
	/// four bytes (`dumphex`).
	pub fn dumphex(&mut self, data: &[u8]) {
		const LINE_LENGTH: usize = 16;
		const HEX_LINE_LENGTH: usize = LINE_LENGTH * 2;
		const CHUNK_SIZE: usize = 8;
		for chunk in data.chunks(LINE_LENGTH) {
			let hexline = crate::text_tools::hex_str(chunk);
			let mut line = String::with_capacity(HEX_LINE_LENGTH + 3);
			for j in (0..HEX_LINE_LENGTH).step_by(CHUNK_SIZE) {
				if j > 0 {
					line.push(' ');
				}
				let end = (j + CHUNK_SIZE).min(hexline.len());
				if j < end {
					line.push_str(&hexline[j..end]);
				}
			}
			self.write_raw(&line, true);
			self.newline();
		}
	}

	/// Increases the indentation (`indent`).
	pub fn indent(&mut self) {
		self.indent_level += 1;
	}

	/// Decreases the indentation (`dedent`).
	pub fn dedent(&mut self) {
		debug_assert!(self.indent_level > 0);
		self.indent_level = self.indent_level.saturating_sub(1);
	}
}

/// Formats attributes in the given order (`stringifyattrs`).
pub fn stringify_attrs(attrs: &Attrs<'_>) -> String {
	let mut data = String::new();
	for (name, value) in attrs {
		data.push(' ');
		data.push_str(name);
		data.push_str("=\"");
		data.push_str(&escape_attr(&value.attr_string()));
		data.push('"');
	}
	data
}

/// Formats attributes sorted by name, as Python's `sorted(kwargs.items())`.
pub fn stringify_attrs_sorted(attrs: &Attrs<'_>) -> String {
	let mut sorted: Vec<_> = attrs.to_vec();
	sorted.sort_by(|a, b| a.0.cmp(b.0));
	stringify_attrs(&sorted)
}

fn is_illegal_xml_char(c: char) -> bool {
	matches!(c as u32, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0xFFFE | 0xFFFF)
}

/// Escapes text, replacing characters that XML 1.0 does not allow with `?`
/// (`escape`).
pub fn escape(data: &str) -> Cow<'_, str> {
	if !data
		.chars()
		.any(|c| matches!(c, '&' | '<' | '>' | '\r') || is_illegal_xml_char(c))
	{
		return Cow::Borrowed(data);
	}
	let mut out = String::with_capacity(data.len() + 8);
	let mut illegal = false;
	for c in data.chars() {
		match c {
			'&' => out.push_str("&amp;"),
			'<' => out.push_str("&lt;"),
			'>' => out.push_str("&gt;"),
			'\r' => out.push_str("&#13;"),
			c if is_illegal_xml_char(c) => {
				illegal = true;
				out.push(REPLACEMENT);
			}
			c => out.push(c),
		}
	}
	if illegal {
		// The replacement happens after the entity escaping above, so the preview
		// shows the escaped text, as in fontTools.
		let escaped: String = data
			.replace('&', "&amp;")
			.replace('<', "&lt;")
			.replace('>', "&gt;")
			.replace('\r', "&#13;");
		let preview = if escaped.chars().count() > 10 {
			let head: String = escaped.chars().take(10).collect();
			let repr = crate::py::str_repr(&head);
			format!("{}...", &repr[1..repr.len() - 1])
		} else {
			crate::py::str_repr(&escaped)
		};
		log::warn!(
			"Illegal XML character(s) found; replacing offending string {} with '{REPLACEMENT}'",
			crate::py::str_repr(&preview)
		);
	}
	Cow::Owned(out)
}

/// Escapes an attribute value (`escapeattr`).
pub fn escape_attr(data: &str) -> Cow<'_, str> {
	match escape(data) {
		Cow::Borrowed(s) if !s.contains('"') => Cow::Borrowed(s),
		s => Cow::Owned(s.replace('"', "&quot;")),
	}
}

/// Escapes bytes as Latin-1 text with character references for everything outside
/// printable ASCII (`escape8bit`).
pub fn escape8bit(data: &[u8]) -> String {
	let mut out = String::with_capacity(data.len());
	for &b in data {
		if (32..=127).contains(&b) && !matches!(b, b'<' | b'&' | b'>') {
			out.push(b as char);
		} else {
			out.push_str("&#");
			out.push_str(&b.to_string());
			out.push(';');
		}
	}
	out
}

#[cfg(test)]
mod tests {
	//! Port of Tests/misc/xmlWriter_test.py.

	use super::*;

	const HEADER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";

	#[test]
	fn test_comment_escaped() {
		let mut w = XmlWriter::new();
		w.comment("This&that are <comments>");
		assert_eq!(
			w.as_str(),
			format!("{HEADER}<!-- This&amp;that are &lt;comments&gt; -->")
		);
	}

	#[test]
	fn test_comment_multiline() {
		let mut w = XmlWriter::new();
		w.comment("Hello world\nHow are you?");
		assert_eq!(
			w.as_str(),
			format!("{HEADER}<!-- Hello world\n     How are you? -->")
		);
	}

	#[test]
	fn test_encoding_default() {
		assert_eq!(XmlWriter::new().as_str(), HEADER);
	}

	#[test]
	fn test_write() {
		let mut w = XmlWriter::new();
		w.write("foo&bar");
		assert_eq!(w.as_str(), format!("{HEADER}foo&amp;bar"));
	}

	#[test]
	fn test_indent_dedent() {
		let mut w = XmlWriter::new();
		w.write("foo");
		w.newline();
		w.indent();
		w.write("bar");
		w.newline();
		w.dedent();
		w.write("baz");
		assert_eq!(w.as_str(), format!("{HEADER}foo\n  bar\nbaz"));
	}

	#[test]
	fn test_writecdata() {
		let mut w = XmlWriter::new();
		w.writecdata("foo&bar");
		assert_eq!(w.as_str(), format!("{HEADER}<![CDATA[foo&bar]]>"));
	}

	#[test]
	fn test_writecdata_split_terminator() {
		let mut w = XmlWriter::new();
		w.writecdata("foo]]><tag/>bar");
		assert_eq!(
			w.as_str(),
			format!("{HEADER}<![CDATA[foo]]]]><![CDATA[><tag/>bar]]>")
		);
	}

	#[test]
	fn test_simpletag() {
		let mut w = XmlWriter::new();
		w.simpletag("tag", &[("b", &"2"), ("a", &"1")]);
		assert_eq!(w.as_str(), format!("{HEADER}<tag a=\"1\" b=\"2\"/>"));
	}

	#[test]
	fn test_begintag_endtag() {
		let mut w = XmlWriter::new();
		w.begintag("tag", &[("attr", &"value")]);
		w.write("content");
		w.endtag("tag");
		assert_eq!(
			w.as_str(),
			format!("{HEADER}<tag attr=\"value\">content</tag>")
		);
	}

	#[test]
	fn test_dumphex() {
		let mut w = XmlWriter::new();
		w.dumphex(b"Type is a beautiful group of letters, not a group of beautiful letters.");
		let expected = [
			"54797065 20697320 61206265 61757469",
			"66756c20 67726f75 70206f66 206c6574",
			"74657273 2c206e6f 74206120 67726f75",
			"70206f66 20626561 75746966 756c206c",
			"65747465 72732e  ",
			"",
		]
		.join("\n");
		assert_eq!(w.as_str(), format!("{HEADER}{expected}"));
	}

	#[test]
	fn test_stringifyattrs() {
		assert_eq!(stringify_attrs(&[("attr", &0)]), " attr=\"0\"");
		assert_eq!(stringify_attrs(&[("attr", &"0")]), " attr=\"0\"");
	}

	#[test]
	fn test_carriage_return_escaped() {
		let mut w = XmlWriter::new();
		w.write("two lines\r\nseparated by Windows line endings");
		assert_eq!(
			w.as_str(),
			format!("{HEADER}two lines&#13;\nseparated by Windows line endings")
		);
	}

	#[test]
	fn test_newlinestr() {
		let header = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>";
		for nls in ["\n", "\r\n", "\r", ""] {
			let mut w = XmlWriter::with_options(INDENT, nls);
			w.write("hello");
			w.newline();
			w.write("world");
			w.newline();
			assert_eq!(w.as_str(), format!("{header}{nls}hello{nls}world{nls}"));
		}
	}

	#[test]
	fn illegal_characters() {
		assert_eq!(escape("a\u{1}b\u{FFFE}"), "a?b?");
		assert_eq!(escape_attr("\"x\""), "&quot;x&quot;");
		assert_eq!(escape8bit(b"a<\x00\x7f\x80"), "a&#60;&#0;\x7f&#128;");
	}
}
