//! A minimal element tree for building XML, serialized the way lxml does
//! (`fontTools.misc.etree` uses lxml when it is installed, as in the reference
//! environment).
//!
//! Only what fontTools builds with `etree.Element` is supported: elements with
//! attributes, text and child elements, no tails, comments or processing
//! instructions. Attribute names may use the Clark notation for the `xml`
//! namespace (`{http://www.w3.org/XML/1998/namespace}lang`), which is written as
//! `xml:lang`.

use std::fmt;

/// The `xml` namespace in Clark notation (`XML_NS` in `designspaceLib`).
pub const XML_NS: &str = "{http://www.w3.org/XML/1998/namespace}";

/// An error building or serializing a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtreeError(pub String);

impl fmt::Display for EtreeError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.0)
	}
}

impl std::error::Error for EtreeError {}

/// An XML element (`etree.Element`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Element {
	/// The tag (`tag`).
	pub tag: String,
	/// The attributes in insertion order (`attrib`).
	pub attrib: Vec<(String, String)>,
	/// The text before the first child (`text`).
	pub text: Option<String>,
	/// The child elements.
	pub children: Vec<Element>,
}

impl Element {
	/// `etree.Element(tag)`.
	pub fn new(tag: impl Into<String>) -> Self {
		Element {
			tag: tag.into(),
			..Default::default()
		}
	}

	/// `element.attrib[key] = value`: an existing attribute keeps its position.
	pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
		let key = key.into();
		let value = value.into();
		match self.attrib.iter_mut().find(|(k, _)| *k == key) {
			Some(slot) => slot.1 = value,
			None => self.attrib.push((key, value)),
		}
	}

	/// `element.attrib.get(key)`.
	pub fn get(&self, key: &str) -> Option<&str> {
		self.attrib
			.iter()
			.find(|(k, _)| k == key)
			.map(|(_, v)| v.as_str())
	}

	/// `element.append(child)`.
	pub fn append(&mut self, child: Element) {
		self.children.push(child);
	}

	/// `len(element)`.
	pub fn len(&self) -> usize {
		self.children.len()
	}

	/// `len(element) == 0`.
	pub fn is_empty(&self) -> bool {
		self.children.is_empty()
	}
}

/// lxml only accepts XML-compatible strings: no NUL bytes or control characters
/// other than tab, newline and carriage return (`_utf8` in `apihelpers.pxi`).
fn check_xml_string(s: &str) -> Result<(), EtreeError> {
	let ok = s.chars().all(|c| {
		matches!(c, '\t' | '\n' | '\r')
			|| ('\u{20}'..='\u{D7FF}').contains(&c)
			|| ('\u{E000}'..='\u{FFFD}').contains(&c)
			|| c >= '\u{10000}'
	});
	if ok {
		Ok(())
	} else {
		Err(EtreeError(
			"All strings must be XML compatible: Unicode or ASCII, no NULL bytes or control characters"
				.into(),
		))
	}
}

/// The qualified name of a tag or attribute: only the `xml` namespace is
/// supported in Clark notation.
fn qname(name: &str) -> Result<String, EtreeError> {
	if let Some(local) = name.strip_prefix(XML_NS) {
		return Ok(format!("xml:{local}"));
	}
	if name.is_empty() || name.starts_with('{') || name.contains(['<', '>', '&', '"', ' ']) {
		return Err(EtreeError(format!("Invalid tag name {name:?}")));
	}
	Ok(name.to_owned())
}

/// `xmlEscapeContent`.
fn push_text(out: &mut String, text: &str) {
	for c in text.chars() {
		match c {
			'<' => out.push_str("&lt;"),
			'>' => out.push_str("&gt;"),
			'&' => out.push_str("&amp;"),
			'\r' => out.push_str("&#13;"),
			_ => out.push(c),
		}
	}
}

/// `xmlBufAttrSerializeTxtContent`.
fn push_attr_value(out: &mut String, value: &str) {
	for c in value.chars() {
		match c {
			'"' => out.push_str("&quot;"),
			'<' => out.push_str("&lt;"),
			'>' => out.push_str("&gt;"),
			'&' => out.push_str("&amp;"),
			'\n' => out.push_str("&#10;"),
			'\r' => out.push_str("&#13;"),
			'\t' => out.push_str("&#9;"),
			_ => out.push(c),
		}
	}
}

fn write_element(
	out: &mut String,
	element: &Element,
	level: usize,
	pretty: bool,
) -> Result<(), EtreeError> {
	// libxml2 limits the indentation to 60 characters (`MAX_INDENT`).
	let indent = |out: &mut String, level: usize| {
		for _ in 0..level.min(30) {
			out.push_str("  ");
		}
	};
	let tag = qname(&element.tag)?;
	out.push('<');
	out.push_str(&tag);
	for (key, value) in &element.attrib {
		check_xml_string(value)?;
		out.push(' ');
		out.push_str(&qname(key)?);
		out.push_str("=\"");
		push_attr_value(out, value);
		out.push('"');
	}
	if element.text.is_none() && element.children.is_empty() {
		out.push_str("/>");
		return Ok(());
	}
	out.push('>');
	// An element with text has mixed content: libxml2 does not format inside it.
	let format = pretty && element.text.is_none();
	if let Some(text) = &element.text {
		check_xml_string(text)?;
		push_text(out, text);
	}
	if format {
		out.push('\n');
	}
	for child in &element.children {
		if format {
			indent(out, level + 1);
		}
		write_element(out, child, level + 1, format)?;
		if format {
			out.push('\n');
		}
	}
	if format {
		indent(out, level);
	}
	out.push_str("</");
	out.push_str(&tag);
	out.push('>');
	Ok(())
}

/// `etree.ElementTree(root).write(f, encoding="UTF-8", xml_declaration=...,
/// pretty_print=...)`, as a string.
pub fn tostring(
	root: &Element,
	pretty_print: bool,
	xml_declaration: bool,
) -> Result<String, EtreeError> {
	let mut out = String::new();
	if xml_declaration {
		out.push_str("<?xml version='1.0' encoding='UTF-8'?>\n");
	}
	write_element(&mut out, root, 0, pretty_print)?;
	if pretty_print {
		out.push('\n');
	}
	Ok(out)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn lxml_pretty_print() -> Result<(), EtreeError> {
		let mut root = Element::new("a");
		let mut s = Element::new("string");
		s.text = Some(String::new());
		root.append(s);
		let mut t = Element::new("t");
		t.text = Some("x<>&\"\r\n\t".into());
		t.set(format!("{XML_NS}lang"), "a\"<>&\n\r\t'");
		root.append(t);
		let mut e = Element::new("e");
		e.append(Element::new("x"));
		root.append(e);
		// Checked against lxml 6.
		assert_eq!(
			tostring(&root, true, true)?,
			"<?xml version='1.0' encoding='UTF-8'?>\n<a>\n  <string></string>\n  <t xml:lang=\"a&quot;&lt;&gt;&amp;&#10;&#13;&#9;'\">x&lt;&gt;&amp;\"&#13;\n\t</t>\n  <e>\n    <x/>\n  </e>\n</a>\n"
		);
		Ok(())
	}

	#[test]
	fn rejects_control_characters() {
		let mut root = Element::new("a");
		root.text = Some("\u{1}".into());
		assert!(tostring(&root, true, true).is_err());
	}
}
