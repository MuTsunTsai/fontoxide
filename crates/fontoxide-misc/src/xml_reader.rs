//! The XML content model of `fontTools.misc.xmlReader`.
//!
//! fontTools hands each table's `fromXML` an element as `(name, attrs, content)`,
//! where `content` is a list of text chunks and child elements. Text is split the
//! way expat delivers it: every newline is a separate chunk, and other text is
//! merged into the preceding chunk unless that chunk is a newline. Some tables
//! depend on this (for example, they skip `"\n"` chunks), so it is reproduced here.
//!
//! The TTX-specific dispatch (tables, `GlyphOrder`, `src` sub-files) lives in
//! `fontoxide-ttlib`.

use std::fmt;

/// A node in an element's content.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
	/// A text chunk.
	Text(String),
	/// A child element.
	Element(Element),
}

impl Content {
	/// The element, if this is one.
	pub fn as_element(&self) -> Option<&Element> {
		match self {
			Content::Element(e) => Some(e),
			Content::Text(_) => None,
		}
	}

	/// The text, if this is a text chunk.
	pub fn as_text(&self) -> Option<&str> {
		match self {
			Content::Text(t) => Some(t),
			Content::Element(_) => None,
		}
	}
}

/// An XML element: name, attributes in document order, and content.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Element {
	/// The element name.
	pub name: String,
	/// The attributes, in document order.
	pub attrs: Vec<(String, String)>,
	/// Text chunks and child elements.
	pub content: Vec<Content>,
}

impl Element {
	/// The value of an attribute (`attrs[name]` / `attrs.get(name)`).
	pub fn attr(&self, name: &str) -> Option<&str> {
		self.attrs
			.iter()
			.find(|(n, _)| n == name)
			.map(|(_, v)| v.as_str())
	}

	/// The child elements, skipping text (the common
	/// `if not isinstance(element, tuple): continue` loop).
	pub fn elements(&self) -> impl Iterator<Item = &Element> {
		self.content.iter().filter_map(Content::as_element)
	}

	/// All text chunks concatenated (`strjoin(content)` over the strings).
	pub fn text(&self) -> String {
		self.content.iter().filter_map(Content::as_text).collect()
	}

	/// The text chunks, in order.
	pub fn text_chunks(&self) -> impl Iterator<Item = &str> {
		self.content.iter().filter_map(Content::as_text)
	}
}

/// An XML syntax error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.0)
	}
}

impl std::error::Error for ParseError {}

/// Parses a document and returns its root element.
pub fn parse(text: &str) -> Result<Element, ParseError> {
	let options = roxmltree::ParsingOptions {
		allow_dtd: true,
		..Default::default()
	};
	let doc = roxmltree::Document::parse_with_options(text, options)
		.map_err(|e| ParseError(e.to_string()))?;
	Ok(convert(doc.root_element()))
}

fn convert(node: roxmltree::Node<'_, '_>) -> Element {
	let mut element = Element {
		name: node.tag_name().name().to_owned(),
		attrs: node
			.attributes()
			.map(|a| (attr_name(&a), a.value().to_owned()))
			.collect(),
		content: Vec::new(),
	};
	for child in node.children() {
		if child.is_element() {
			element.content.push(Content::Element(convert(child)));
		} else if child.is_text() {
			push_text(&mut element.content, child.text().unwrap_or_default());
		}
	}
	element
}

fn attr_name(a: &roxmltree::Attribute<'_, '_>) -> String {
	// fontTools parses without namespace processing, so a prefixed attribute keeps
	// its prefix (e.g. "xml:lang").
	match a.namespace() {
		Some("http://www.w3.org/XML/1998/namespace") => format!("xml:{}", a.name()),
		_ => a.name().to_owned(),
	}
}

/// Appends text the way `XMLReader._characterDataHandler` receives it from expat.
fn push_text(content: &mut Vec<Content>, text: &str) {
	for (i, piece) in text.split('\n').enumerate() {
		if i > 0 {
			append_chunk(content, "\n");
		}
		if !piece.is_empty() {
			append_chunk(content, piece);
		}
	}
}

fn append_chunk(content: &mut Vec<Content>, data: &str) {
	if data != "\n"
		&& let Some(Content::Text(last)) = content.last_mut()
		&& last != "\n"
	{
		last.push_str(data);
		return;
	}
	content.push(Content::Text(data.to_owned()));
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn content_model() {
		let root = parse("<a x=\"1\">\n  <b/>\n  text &amp; more\n</a>").expect("valid xml");
		assert_eq!(root.name, "a");
		assert_eq!(root.attr("x"), Some("1"));
		let texts: Vec<_> = root.text_chunks().collect();
		assert_eq!(texts, ["\n", "  ", "\n", "  text & more", "\n"]);
		assert_eq!(root.elements().count(), 1);
	}

	#[test]
	fn crlf_is_normalized() {
		let root = parse("<a>x\r\ny</a>").expect("valid xml");
		let texts: Vec<_> = root.text_chunks().collect();
		assert_eq!(texts, ["x", "\n", "y"]);
	}
}
