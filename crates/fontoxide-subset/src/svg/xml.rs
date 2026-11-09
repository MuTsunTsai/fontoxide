//! The part of lxml that `fontTools.subset.svg` depends on: parsing with
//! `XMLParser(huge_tree=True, remove_blank_text=True, resolve_entities=False)`
//! and `etree.tostring(element, pretty_print=...)`.
//!
//! Which whitespace is dropped and how the output is formatted follow libxml2
//! (`areBlanks` and the fast and slow paths of `xmlParseCharData`, and
//! `xmlNodeDumpOutputInternal`), because they show up in the subset documents.

/// `lxml.etree.XMLSyntaxError`, or a document this parser declines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum XmlError {
	/// The document is not well-formed.
	Syntax(String),
	/// LENIENT(L25): well-formed, but relies on a feature that is not implemented:
	/// another encoding than UTF-8, an external DTD subset, or declarations other
	/// than simple internal entities in the internal one.
	Unsupported(String),
}

type Result<T> = std::result::Result<T, XmlError>;

fn syntax<T>(message: impl Into<String>) -> Result<T> {
	Err(XmlError::Syntax(message.into()))
}

const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

/// An attribute.
#[derive(Debug, Clone)]
pub(crate) struct Attr {
	prefix: Option<String>,
	/// The local name.
	pub(crate) local: String,
	/// The namespace URI.
	pub(crate) ns: Option<String>,
	/// The value.
	pub(crate) value: String,
}

/// The content of a node.
#[derive(Debug, Clone)]
pub(crate) enum Kind {
	Element {
		prefix: Option<String>,
		local: String,
		ns: Option<String>,
		ns_defs: Vec<(Option<String>, String)>,
		attrs: Vec<Attr>,
	},
	Text(String),
	Comment(String),
	Pi {
		target: String,
		content: Option<String>,
	},
	EntityRef(String),
}

/// A node of the tree.
#[derive(Debug, Clone)]
pub(crate) struct Node {
	pub(crate) kind: Kind,
	pub(crate) parent: Option<usize>,
	pub(crate) children: Vec<usize>,
}

/// A parsed document. Only the root element is kept: comments and processing
/// instructions next to it, and the DTD, are not written by `tostring`.
#[derive(Debug, Clone)]
pub(crate) struct Document {
	pub(crate) nodes: Vec<Node>,
	pub(crate) root: usize,
}

fn is_blank(b: u8) -> bool {
	matches!(b, 0x20 | 0x09 | 0x0A | 0x0D)
}

/// `IS_CHAR`.
fn is_char(c: char) -> bool {
	matches!(c, '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

fn is_name_start(c: char) -> bool {
	matches!(c,
		':' | 'A'..='Z' | '_' | 'a'..='z'
		| '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
		| '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}'
		| '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}'
		| '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
}

fn is_name_char(c: char) -> bool {
	is_name_start(c)
		|| matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
}

/// Whether `xmlParseURI` accepts a namespace name: if so, whether it has a
/// scheme.
///
/// LENIENT(L25): this checks the scheme, the characters of each component and
/// the percent escapes, not the form of an authority.
fn parse_uri(uri: &str) -> Option<bool> {
	/// Whether all characters are `pchar`s or in `extra`.
	fn chars(part: &str, extra: &[u8]) -> bool {
		let bytes = part.as_bytes();
		let mut i = 0;
		while i < bytes.len() {
			let b = bytes[i];
			if b == b'%' {
				let hex = bytes
					.get(i + 1..i + 3)
					.is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit));
				if !hex {
					return false;
				}
				i += 3;
				continue;
			}
			let pchar = b.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@".contains(&b);
			if !pchar && !extra.contains(&b) {
				return false;
			}
			i += 1;
		}
		true
	}
	let (rest, fragment) = uri.split_once('#').unwrap_or((uri, ""));
	if !chars(fragment, b"/?[]") {
		return None;
	}
	let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
	if !chars(query, b"/?") {
		return None;
	}
	// A colon in the first segment ends a scheme.
	let first = rest.split('/').next().unwrap_or_default();
	let (has_scheme, path) = match first.split_once(':') {
		Some((scheme, _)) => {
			let valid = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
				&& scheme
					.bytes()
					.all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b));
			if !valid {
				return None;
			}
			(true, &rest[scheme.len() + 1..])
		}
		None => (false, rest),
	};
	let valid = if let Some(authority) = path.strip_prefix("//") {
		let (authority, path) = authority.split_once('/').unwrap_or((authority, ""));
		chars(authority, b"[]") && chars(path, b"/")
	} else {
		chars(path, b"/")
	};
	valid.then_some(has_scheme)
}

/// What a reference in the input stands for.
enum Reference {
	Text(String),
	/// An entity declared in the DTD, which is kept as a reference.
	Entity(String),
}

struct Parser<'a> {
	text: &'a str,
	pos: usize,
	nodes: Vec<Node>,
	/// The open elements (`ctxt->node` is the last one).
	open: Vec<usize>,
	/// `ctxt->space`, for each open element and the initial -1.
	space: Vec<i8>,
	entities: Vec<String>,
	/// A namespace error (`xmlNsErr`). These don't stop the parser, and lxml only
	/// looks at the level of the last message, so a later warning cancels them.
	ns_error: Option<String>,
}

impl Parser<'_> {
	fn rest(&self) -> &str {
		&self.text[self.pos..]
	}

	fn byte(&self, offset: usize) -> u8 {
		self.text
			.as_bytes()
			.get(self.pos + offset)
			.copied()
			.unwrap_or(0)
	}

	fn eat(&mut self, s: &str) -> bool {
		let found = self.rest().starts_with(s);
		if found {
			self.pos += s.len();
		}
		found
	}

	fn skip_blanks(&mut self) -> bool {
		let start = self.pos;
		while is_blank(self.byte(0)) {
			self.pos += 1;
		}
		self.pos != start
	}

	fn peek_char(&self) -> Option<char> {
		self.rest().chars().next()
	}

	/// `xmlParseName`.
	fn name(&mut self) -> Option<&str> {
		let start = self.pos;
		let first = self.peek_char()?;
		if !is_name_start(first) {
			return None;
		}
		self.pos += first.len_utf8();
		while let Some(c) = self.peek_char() {
			if !is_name_char(c) {
				break;
			}
			self.pos += c.len_utf8();
		}
		let text: &str = self.text;
		Some(&text[start..self.pos])
	}

	/// `xmlParseQName`: the prefix and the local name.
	fn qname(&mut self, what: &str) -> Result<(Option<String>, String)> {
		let Some(name) = self.name() else {
			return syntax(what.to_owned());
		};
		let name = name.to_owned();
		let Some((prefix, local)) = name.split_once(':') else {
			return Ok((None, name));
		};
		let valid = !prefix.is_empty()
			&& local
				.chars()
				.next()
				.is_some_and(|c| is_name_start(c) && c != ':')
			&& !local.contains(':');
		if !valid {
			return syntax(format!("Failed to parse QName '{name}'"));
		}
		Ok((Some(prefix.to_owned()), local.to_owned()))
	}

	fn add_node(&mut self, kind: Kind) -> usize {
		let id = self.nodes.len();
		let parent = self.open.last().copied();
		self.nodes.push(Node {
			kind,
			parent,
			children: Vec::new(),
		});
		if let Some(parent) = parent {
			self.nodes[parent].children.push(id);
		}
		id
	}

	/// `xmlSAX2Characters`: text is merged with a preceding text node.
	fn characters(&mut self, text: &str) {
		if let Some(&node) = self.open.last()
			&& let Some(&last) = self.nodes[node].children.last()
			&& let Kind::Text(t) = &mut self.nodes[last].kind
		{
			t.push_str(text);
			return;
		}
		self.add_node(Kind::Text(text.to_owned()));
	}

	/// `areBlanks`; the next input is the current position.
	fn are_blanks(&self, chunk: &str) -> bool {
		if matches!(self.space.last(), Some(1 | -2)) {
			return false;
		}
		if !chunk.bytes().all(is_blank) {
			return false;
		}
		let Some(&node) = self.open.last() else {
			return false;
		};
		let raw = self.byte(0);
		if raw != b'<' && raw != 0x0D {
			return false;
		}
		let children = &self.nodes[node].children;
		if children.is_empty() && raw == b'<' && self.byte(1) == b'/' {
			return false;
		}
		if let Some(&last) = children.last() {
			if matches!(self.nodes[last].kind, Kind::Text(_)) {
				return false;
			}
			if matches!(self.nodes[children[0]].kind, Kind::Text(_)) {
				return false;
			}
		}
		true
	}

	/// A chunk of whitespace, or of text that starts with whitespace, in
	/// `xmlParseCharData`.
	fn blank_chunk(&mut self, chunk: &str) {
		if !self.are_blanks(chunk) {
			self.characters(chunk);
			if let Some(space) = self.space.last_mut()
				&& *space == -1
			{
				*space = -2;
			}
		}
	}

	/// `xmlParseCharData`.
	fn char_data(&mut self) -> Result<()> {
		let text: &str = self.text;
		// The fast path, for ASCII text.
		loop {
			let start = self.pos;
			loop {
				match self.byte(0) {
					b']' if self.rest().starts_with("]]>") => {
						return syntax("Sequence ']]>' not allowed in content");
					}
					0x09 | 0x0A | 0x20..=0x25 | 0x27..=0x3B | 0x3D..=0x7F => self.pos += 1,
					_ => break,
				}
			}
			let chunk = &text[start..self.pos];
			if !chunk.is_empty() {
				if is_blank(chunk.as_bytes()[0]) {
					self.blank_chunk(chunk);
				} else {
					self.characters(chunk);
				}
			}
			match self.byte(0) {
				b'<' | b'&' => return Ok(()),
				// The fast path goes past a line end if the next line starts with
				// ASCII text.
				0x0D if self.byte(1) == 0x0A => {
					self.pos += 1;
					if !matches!(self.byte(1), 0x09 | 0x0A | 0x20..=0x7F) {
						break;
					}
				}
				_ => break,
			}
		}
		// The slow path (`xmlParseCharDataComplex`), which starts at the first
		// other character and flushes its buffer every 300 bytes.
		let mut buffer = String::new();
		while let Some(c) = self.peek_char() {
			if c == '<' || c == '&' {
				break;
			}
			if !is_char(c) {
				return syntax(format!("PCDATA invalid Char value {}", u32::from(c)));
			}
			if self.rest().starts_with("]]>") {
				return syntax("Sequence ']]>' not allowed in content");
			}
			self.pos += c.len_utf8();
			if c == '\r' {
				if self.byte(0) == b'\n' {
					self.pos += 1;
				}
				buffer.push('\n');
			} else {
				buffer.push(c);
			}
			if buffer.len() >= 300 {
				self.blank_chunk(&buffer);
				buffer.clear();
			}
		}
		if !buffer.is_empty() {
			self.blank_chunk(&buffer);
		}
		Ok(())
	}

	/// `xmlParseCharRef`, after `&#`.
	fn char_ref(&mut self) -> Result<char> {
		let hex = self.eat("x");
		let start = self.pos;
		while if hex {
			self.byte(0).is_ascii_hexdigit()
		} else {
			self.byte(0).is_ascii_digit()
		} {
			self.pos += 1;
		}
		let digits = &self.text[start..self.pos];
		if digits.is_empty() {
			return syntax("xmlParseCharRef: invalid decimal value");
		}
		if !self.eat(";") {
			return syntax("CharRef: invalid decimal value");
		}
		let value = u32::from_str_radix(digits, if hex { 16 } else { 10 }).unwrap_or(u32::MAX);
		match char::from_u32(value).filter(|&c| is_char(c)) {
			Some(c) => Ok(c),
			None => syntax(format!("xmlParseCharRef: invalid xmlChar value {value}")),
		}
	}

	/// A reference, after `&`: its text, or the name of an entity declared in the
	/// DTD.
	fn reference(&mut self) -> Result<Reference> {
		if self.eat("#") {
			return Ok(Reference::Text(self.char_ref()?.to_string()));
		}
		let Some(name) = self.name() else {
			return syntax("xmlParseEntityRef: no name");
		};
		let name = name.to_owned();
		if !self.eat(";") {
			return syntax("EntityRef: expecting ';'");
		}
		let predefined = match name.as_str() {
			"lt" => "<",
			"gt" => ">",
			"amp" => "&",
			"quot" => "\"",
			"apos" => "'",
			_ => {
				if self.entities.contains(&name) {
					return Ok(Reference::Entity(name));
				}
				return syntax(format!("Entity '{name}' not defined"));
			}
		};
		Ok(Reference::Text(predefined.to_owned()))
	}

	/// `xmlParseAttValueInternal`.
	fn att_value(&mut self) -> Result<String> {
		let quote = match self.byte(0) {
			b'"' => '"',
			b'\'' => '\'',
			_ => return syntax("AttValue: \" or ' expected"),
		};
		self.pos += 1;
		let mut value = String::new();
		loop {
			let Some(c) = self.peek_char() else {
				return syntax("AttValue: ' expected");
			};
			self.pos += c.len_utf8();
			match c {
				_ if c == quote => return Ok(value),
				'<' => return syntax("Unescaped '<' not allowed in attributes values"),
				'&' => match self.reference()? {
					Reference::Text(text) => value.push_str(&text),
					Reference::Entity(name) => {
						return Err(XmlError::Unsupported(format!(
							"the entity '{name}' in an attribute value"
						)));
					}
				},
				'\r' => {
					if self.byte(0) == b'\n' {
						self.pos += 1;
					}
					value.push(' ');
				}
				'\t' | '\n' => value.push(' '),
				_ if !is_char(c) => return syntax("invalid character in attribute value"),
				_ => value.push(c),
			}
		}
	}

	/// The namespace in scope for a prefix.
	fn lookup(&self, prefix: Option<&str>, own: &[(Option<String>, String)]) -> Option<String> {
		if prefix == Some("xml") {
			return Some(XML_NS.to_owned());
		}
		let matches = |defs: &[(Option<String>, String)]| {
			defs.iter()
				.find(|(p, _)| p.as_deref() == prefix)
				.map(|(_, uri)| uri.clone())
		};
		if let Some(uri) = matches(own) {
			return Some(uri);
		}
		for &id in self.open.iter().rev() {
			if let Kind::Element { ns_defs, .. } = &self.nodes[id].kind
				&& let Some(uri) = matches(ns_defs)
			{
				return Some(uri);
			}
		}
		None
	}

	fn ns_error(&mut self, message: &str) {
		self.ns_error = Some(message.to_owned());
	}

	fn warning(&mut self) {
		self.ns_error = None;
	}

	/// The `xmlParseURI` check of a namespace name; only the default namespace
	/// is warned about for not being absolute.
	fn check_uri(&mut self, uri: &str, default: bool) {
		match parse_uri(uri) {
			None => self.ns_error(&format!("'{uri}' is not a valid URI")),
			Some(false) if default => self.warning(),
			Some(_) => {}
		}
	}

	/// `xmlParseStartTag2` and the namespace checks of `xmlSAX2StartElementNs`,
	/// after `<`. Returns whether the element was also ended.
	fn start_tag(&mut self) -> Result<bool> {
		// `xmlParseElementStart`.
		let space = match self.space.last() {
			None | Some(-2) => -1,
			Some(&s) => s,
		};
		self.space.push(space);
		let (prefix, local) = self.qname("StartTag: invalid element name")?;
		let mut ns_defs: Vec<(Option<String>, String)> = Vec::new();
		let mut raw_attrs: Vec<(Option<String>, String, String)> = Vec::new();
		let empty;
		loop {
			let blanks = self.skip_blanks();
			if self.eat("/>") {
				empty = true;
				break;
			}
			if self.eat(">") {
				empty = false;
				break;
			}
			if self.pos >= self.text.len() {
				return syntax("Couldn't find end of Start Tag");
			}
			if !blanks {
				return syntax("attributes construct error");
			}
			let (attr_prefix, attr_local) = self.qname("error parsing attribute name")?;
			self.skip_blanks();
			if !self.eat("=") {
				return syntax(format!(
					"Specification mandates value for attribute {attr_local}"
				));
			}
			self.skip_blanks();
			let value = self.att_value()?;
			// Without entity replacement, libxml2 leaves ampersands escaped for a
			// later pass over attribute values, which namespace names don't get.
			let is_namespace = (attr_prefix.is_none() && attr_local == "xmlns")
				|| attr_prefix.as_deref() == Some("xmlns");
			let value = if is_namespace {
				value.replace('&', "&#38;")
			} else {
				value
			};
			if attr_prefix.is_none() && attr_local == "xmlns" {
				if !value.is_empty() {
					self.check_uri(&value, true);
					if value == XML_NS {
						self.ns_error("xml namespace URI cannot be the default namespace");
						continue;
					}
					if value == XMLNS_NS {
						self.ns_error("reuse of the xmlns namespace name is forbidden");
						continue;
					}
				}
				if ns_defs.iter().any(|(p, _)| p.is_none()) {
					return syntax("Attribute xmlns redefined");
				}
				ns_defs.push((None, value));
				continue;
			}
			if attr_prefix.as_deref() == Some("xmlns") {
				if attr_local == "xml" {
					// Not stored: it is implicit.
					if value != XML_NS {
						self.ns_error("xml namespace prefix mapped to wrong URI");
					}
					continue;
				}
				if value == XML_NS {
					self.ns_error("xml namespace URI mapped to wrong prefix");
					continue;
				}
				if attr_local == "xmlns" {
					self.ns_error("redefinition of the xmlns prefix is forbidden");
					continue;
				}
				if value == XMLNS_NS {
					self.ns_error("reuse of the xmlns namespace name is forbidden");
					continue;
				}
				if value.is_empty() {
					self.ns_error(&format!(
						"xmlns:{attr_local}: Empty XML namespace is not allowed"
					));
					continue;
				}
				self.check_uri(&value, false);
				if ns_defs
					.iter()
					.any(|(p, _)| p.as_deref() == Some(attr_local.as_str()))
				{
					return syntax(format!("Attribute xmlns:{attr_local} redefined"));
				}
				ns_defs.push((Some(attr_local), value));
				continue;
			}
			if raw_attrs
				.iter()
				.any(|(p, l, _)| *p == attr_prefix && *l == attr_local)
			{
				return syntax(format!("Attribute {attr_local} redefined"));
			}
			if attr_prefix.as_deref() == Some("xml") && attr_local == "space" {
				let new_space = match value.as_str() {
					"default" => Some(0),
					"preserve" => Some(1),
					_ => {
						self.warning();
						None
					}
				};
				if let (Some(new_space), Some(space)) = (new_space, self.space.last_mut()) {
					*space = new_space;
				}
			}
			raw_attrs.push((attr_prefix, attr_local, value));
		}
		let mut attrs: Vec<Attr> = Vec::with_capacity(raw_attrs.len());
		for (attr_prefix, attr_local, value) in raw_attrs {
			let attr_ns = attr_prefix
				.as_deref()
				.and_then(|p| self.lookup(Some(p), &ns_defs));
			let (attr_prefix, attr_local) = match (attr_prefix, &attr_ns) {
				(Some(p), None) => {
					self.ns_error(&format!(
						"Namespace prefix {p} for {attr_local} on {local} is not defined"
					));
					// libxml2 keeps the qualified name as the local name.
					(None, format!("{p}:{attr_local}"))
				}
				(p, _) => (p, attr_local),
			};
			if attr_ns.is_some()
				&& attrs
					.iter()
					.any(|a| a.local == attr_local && a.ns == attr_ns)
			{
				self.ns_error(&format!("Namespaced Attribute {attr_local} redefined"));
			}
			attrs.push(Attr {
				prefix: attr_prefix,
				local: attr_local,
				ns: attr_ns,
				value,
			});
		}
		let ns = self
			.lookup(prefix.as_deref(), &ns_defs)
			.filter(|uri| !uri.is_empty());
		if let (Some(p), None) = (&prefix, &ns) {
			let message = format!("Namespace prefix {p} on {local} is not defined");
			self.ns_error(&message);
		}
		let id = self.add_node(Kind::Element {
			prefix,
			local,
			ns,
			ns_defs,
			attrs,
		});
		self.open.push(id);
		if empty {
			self.open.pop();
			self.space.pop();
		}
		Ok(empty)
	}

	fn end_tag(&mut self) -> Result<()> {
		let Some(&id) = self.open.last() else {
			return syntax("StartTag: invalid element name");
		};
		let Kind::Element { prefix, local, .. } = &self.nodes[id].kind else {
			return syntax("not an element");
		};
		let expected = match prefix {
			Some(p) => format!("{p}:{local}"),
			None => local.clone(),
		};
		let found = self.name().map(str::to_owned);
		if found.as_deref() != Some(expected.as_str()) {
			return syntax(format!(
				"Opening and ending tag mismatch: {expected} and {}",
				found.unwrap_or_default()
			));
		}
		self.skip_blanks();
		if !self.eat(">") {
			return syntax("expected '>'");
		}
		self.open.pop();
		self.space.pop();
		Ok(())
	}

	/// `xmlParseComment`, after `<!--`.
	fn comment(&mut self) -> Result<String> {
		let Some(end) = self.rest().find("--") else {
			return syntax("Comment not terminated");
		};
		let body = &self.rest()[..end];
		let after = &self.rest()[end..];
		if !after.starts_with("-->") {
			return syntax("Double hyphen within comment");
		}
		if !body.chars().all(is_char) {
			return syntax("xmlParseComment: invalid xmlChar value");
		}
		let body = normalize_line_ends(body);
		self.pos += end + 3;
		Ok(body)
	}

	/// `xmlParsePI`, after `<?`.
	fn pi(&mut self) -> Result<(String, Option<String>)> {
		let Some(target) = self.name() else {
			return syntax("xmlParsePI : no target name");
		};
		let target = target.to_owned();
		if target == "xml" {
			return syntax("XML declaration allowed only at the start of the document");
		}
		if target.eq_ignore_ascii_case("xml") {
			return syntax("Invalid PI name");
		}
		if target.contains(':') {
			self.ns_error(&format!("colons are forbidden from PI names '{target}'"));
		}
		if self.eat("?>") {
			return Ok((target, None));
		}
		if !self.skip_blanks() {
			return syntax(format!("ParsePI: PI {target} space expected"));
		}
		let Some(end) = self.rest().find("?>") else {
			return syntax(format!("ParsePI: PI {target} never end ..."));
		};
		let body = &self.rest()[..end];
		if !body.chars().all(is_char) {
			return syntax("ParsePI: invalid character");
		}
		let body = normalize_line_ends(body);
		self.pos += end + 2;
		Ok((target, Some(body)))
	}

	/// The contents of an element, up to the end of the document.
	fn content(&mut self) -> Result<()> {
		while !self.open.is_empty() {
			if self.pos >= self.text.len() {
				return syntax("Premature end of data");
			}
			match self.byte(0) {
				b'<' => match self.byte(1) {
					b'/' => {
						self.pos += 2;
						self.end_tag()?;
					}
					b'?' => {
						self.pos += 2;
						let (target, content) = self.pi()?;
						self.add_node(Kind::Pi { target, content });
					}
					b'!' => {
						if self.eat("<!--") {
							let comment = self.comment()?;
							self.add_node(Kind::Comment(comment));
						} else if self.eat("<![CDATA[") {
							let Some(end) = self.rest().find("]]>") else {
								return syntax("CData section not finished");
							};
							let body = &self.rest()[..end];
							if !body.chars().all(is_char) {
								return syntax("PCDATA invalid Char value");
							}
							let body = normalize_line_ends(body);
							self.pos += end + 3;
							self.characters(&body);
						} else {
							return syntax("StartTag: invalid element name");
						}
					}
					_ => {
						self.pos += 1;
						self.start_tag()?;
					}
				},
				b'&' => {
					self.pos += 1;
					match self.reference()? {
						Reference::Text(text) => self.characters(&text),
						Reference::Entity(name) => {
							self.add_node(Kind::EntityRef(name));
						}
					}
				}
				_ => self.char_data()?,
			}
		}
		Ok(())
	}

	/// `xmlParseXMLDecl`, after `<?xml`.
	fn xml_decl(&mut self) -> Result<()> {
		let value = |p: &mut Self, what: &str| -> Result<String> {
			p.skip_blanks();
			if !p.eat("=") {
				return syntax(format!("Malformed declaration expecting {what}"));
			}
			p.skip_blanks();
			let quote = if p.eat("\"") {
				"\""
			} else if p.eat("'") {
				"'"
			} else {
				return syntax("String not started expecting ' or \"");
			};
			let Some(end) = p.rest().find(quote) else {
				return syntax("String not closed");
			};
			let value = p.rest()[..end].to_owned();
			p.pos += end + 1;
			Ok(value)
		};
		self.skip_blanks();
		if !self.eat("version") {
			return syntax("Malformed declaration expecting version");
		}
		let version = value(self, "version")?;
		// `xmlParseVersionNum`: one digit, a period and digits.
		let bytes = version.as_bytes();
		if bytes.len() < 2
			|| !bytes[0].is_ascii_digit()
			|| bytes[1] != b'.'
			|| !bytes[2..].iter().all(u8::is_ascii_digit)
		{
			return syntax("Malformed declaration expecting version");
		}
		if !version.starts_with("1.") {
			return syntax(format!("Unsupported version '{version}'"));
		}
		let mut blanks = self.skip_blanks();
		if self.rest().starts_with("encoding") {
			if !blanks {
				return syntax("Blank needed here");
			}
			self.pos += "encoding".len();
			let encoding = value(self, "encoding")?;
			if !encoding.eq_ignore_ascii_case("UTF-8") && !encoding.eq_ignore_ascii_case("UTF8") {
				return Err(XmlError::Unsupported(format!("the encoding '{encoding}'")));
			}
			blanks = self.skip_blanks();
		}
		if self.rest().starts_with("standalone") {
			if !blanks {
				return syntax("Blank needed here");
			}
			self.pos += "standalone".len();
			let standalone = value(self, "standalone")?;
			if standalone != "yes" && standalone != "no" {
				return syntax("standalone accepts only 'yes' or 'no'");
			}
			self.skip_blanks();
		}
		if !self.eat("?>") {
			return syntax("XML declaration not finished");
		}
		Ok(())
	}

	/// `xmlParseDocTypeDecl` and `xmlParseInternalSubset`, after `<!DOCTYPE`.
	fn doctype(&mut self) -> Result<()> {
		let unsupported = |what: &str| Err(XmlError::Unsupported(format!("{what} in the DTD")));
		self.skip_blanks();
		if self.name().is_none() {
			return syntax("xmlParseDocTypeDecl : no DOCTYPE name !");
		}
		self.skip_blanks();
		if self.rest().starts_with("SYSTEM") || self.rest().starts_with("PUBLIC") {
			return unsupported("an external subset");
		}
		if self.eat("[") {
			loop {
				self.skip_blanks();
				if self.eat("]") {
					break;
				}
				if self.eat("<!--") {
					self.comment()?;
				} else if self.eat("<?") {
					self.pi()?;
				} else if self.eat("<!ENTITY") {
					self.skip_blanks();
					let Some(name) = self.name() else {
						return unsupported("a parameter entity");
					};
					let name = name.to_owned();
					self.skip_blanks();
					let quote = match self.byte(0) {
						b'"' => "\"",
						b'\'' => "'",
						_ => return unsupported("an external entity"),
					};
					self.pos += 1;
					let Some(end) = self.rest().find(quote) else {
						return syntax("EntityValue: \" expected");
					};
					let value = &self.rest()[..end];
					if value.contains(['<', '&', '%']) {
						return unsupported("an entity with markup");
					}
					self.pos += end + 1;
					self.skip_blanks();
					if !self.eat(">") {
						return unsupported("an entity with a notation");
					}
					self.entities.push(name);
				} else {
					return unsupported("a declaration");
				}
			}
			self.skip_blanks();
		}
		if !self.eat(">") {
			return syntax("DOCTYPE improperly terminated");
		}
		Ok(())
	}

	/// `Misc*` before or after the root element.
	fn misc(&mut self) -> Result<()> {
		loop {
			self.skip_blanks();
			if self.eat("<!--") {
				self.comment()?;
			} else if self.eat("<?") {
				self.pi()?;
			} else {
				return Ok(());
			}
		}
	}
}

fn normalize_line_ends(s: &str) -> String {
	if s.contains('\r') {
		s.replace("\r\n", "\n").replace('\r', "\n")
	} else {
		s.to_owned()
	}
}

impl Document {
	/// `etree.fromstring`.
	pub(crate) fn parse(text: &str) -> Result<Self> {
		let mut p = Parser {
			text,
			pos: 0,
			nodes: Vec::new(),
			open: Vec::new(),
			space: vec![-1],
			entities: Vec::new(),
			ns_error: None,
		};
		p.eat("\u{FEFF}");
		if p.pos >= text.len() {
			return syntax("Document is empty");
		}
		if p.rest().starts_with("<?xml") && is_blank(p.byte(5)) {
			p.pos += 5;
			p.xml_decl()?;
		}
		p.misc()?;
		if p.eat("<!DOCTYPE") {
			p.doctype()?;
			p.misc()?;
		}
		if p.byte(0) != b'<' {
			return syntax("Start tag expected, '<' not found");
		}
		p.pos += 1;
		p.start_tag()?;
		p.content()?;
		p.misc()?;
		if p.pos < text.len() {
			return syntax("Extra content at the end of the document");
		}
		if let Some(message) = p.ns_error {
			return syntax(message);
		}
		Ok(Document {
			nodes: p.nodes,
			root: 0,
		})
	}

	/// The attributes of an element.
	pub(crate) fn attrs(&self, id: usize) -> &[Attr] {
		match &self.nodes[id].kind {
			Kind::Element { attrs, .. } => attrs,
			_ => &[],
		}
	}

	/// The value of an attribute (`el.attrib.get`).
	pub(crate) fn attr(&self, id: usize, ns: Option<&str>, local: &str) -> Option<&str> {
		self.attrs(id)
			.iter()
			.find(|a| a.local == local && a.ns.as_deref() == ns)
			.map(|a| a.value.as_str())
	}

	/// Sets the value of an attribute that exists.
	pub(crate) fn set_attr(&mut self, id: usize, ns: Option<&str>, local: &str, value: String) {
		if let Kind::Element { attrs, .. } = &mut self.nodes[id].kind
			&& let Some(a) = attrs
				.iter_mut()
				.find(|a| a.local == local && a.ns.as_deref() == ns)
		{
			a.value = value;
		}
	}

	/// The namespace URI of an element.
	pub(crate) fn namespace(&self, id: usize) -> Option<&str> {
		match &self.nodes[id].kind {
			Kind::Element { ns, .. } => ns.as_deref(),
			_ => None,
		}
	}

	/// The children that lxml iterates over (`for e in el`): all but text.
	pub(crate) fn element_children(&self, id: usize) -> Vec<usize> {
		self.nodes[id]
			.children
			.iter()
			.copied()
			.filter(|&c| !matches!(self.nodes[c].kind, Kind::Text(_)))
			.collect()
	}

	/// `parent.remove(el)`: the tail, the text after the element, goes with it.
	pub(crate) fn remove(&mut self, id: usize) {
		let Some(parent) = self.nodes[id].parent else {
			return;
		};
		let children = &mut self.nodes[parent].children;
		let Some(at) = children.iter().position(|&c| c == id) else {
			return;
		};
		let next = children.get(at + 1).copied();
		children.remove(at);
		if let Some(next) = next
			&& matches!(self.nodes[next].kind, Kind::Text(_))
		{
			self.nodes[parent].children.remove(at);
		}
	}

	/// An element and its descendants, in document order.
	pub(crate) fn descendants_or_self(&self, id: usize) -> Vec<usize> {
		let mut result = Vec::new();
		let mut stack = vec![id];
		while let Some(id) = stack.pop() {
			result.push(id);
			stack.extend(self.nodes[id].children.iter().rev());
		}
		result
	}

	/// `etree.tostring(root, pretty_print=pretty)`, with its default ASCII output
	/// encoding: all other characters become character references, wherever they
	/// are.
	pub(crate) fn to_string(&self, pretty: bool) -> String {
		enum Step {
			Open(usize),
			Close(usize, bool),
			AfterNode(usize),
		}
		// `ctxt->indent_nr`: `MAX_INDENT` of 60 characters, of two each.
		const MAX_INDENT: usize = 30;
		let mut out = String::new();
		let indent = |out: &mut String, level: usize| {
			for _ in 0..level.min(MAX_INDENT) {
				out.push_str("  ");
			}
		};
		let mut format = pretty;
		let mut level = 0;
		let mut steps = vec![Step::Open(self.root)];
		while let Some(step) = steps.pop() {
			match step {
				Step::Open(id) => {
					steps.push(Step::AfterNode(id));
					match &self.nodes[id].kind {
						Kind::Element {
							prefix,
							local,
							ns_defs,
							attrs,
							..
						} => {
							if id != self.root && format {
								indent(&mut out, level);
							}
							out.push('<');
							push_qname(&mut out, prefix.as_deref(), local);
							for (prefix, uri) in ns_defs {
								match prefix {
									Some(p) => {
										out.push_str(" xmlns:");
										out.push_str(p);
									}
									None => out.push_str(" xmlns"),
								}
								out.push('=');
								push_quoted(&mut out, uri);
							}
							for a in attrs {
								out.push(' ');
								push_qname(&mut out, a.prefix.as_deref(), &a.local);
								out.push_str("=\"");
								push_attr_value(&mut out, &a.value);
								out.push('"');
							}
							let children = &self.nodes[id].children;
							if children.is_empty() {
								out.push_str("/>");
							} else {
								let unformatted = format
									&& children.iter().any(|&c| {
										matches!(
											self.nodes[c].kind,
											Kind::Text(_) | Kind::EntityRef(_)
										)
									});
								let restore = format;
								if unformatted {
									format = false;
								}
								out.push('>');
								if format {
									out.push('\n');
								}
								level += 1;
								steps.push(Step::Close(id, restore));
								steps.extend(children.iter().rev().map(|&c| Step::Open(c)));
							}
						}
						Kind::Text(text) => push_text(&mut out, text),
						Kind::Comment(text) => {
							if format {
								indent(&mut out, level);
							}
							out.push_str("<!--");
							out.push_str(text);
							out.push_str("-->");
						}
						Kind::Pi { target, content } => {
							if format {
								indent(&mut out, level);
							}
							out.push_str("<?");
							out.push_str(target);
							if let Some(content) = content {
								out.push(' ');
								out.push_str(content);
							}
							out.push_str("?>");
						}
						Kind::EntityRef(name) => {
							out.push('&');
							out.push_str(name);
							out.push(';');
						}
					}
				}
				Step::Close(id, restore) => {
					level -= 1;
					if let Kind::Element { prefix, local, .. } = &self.nodes[id].kind {
						if format {
							indent(&mut out, level);
						}
						out.push_str("</");
						push_qname(&mut out, prefix.as_deref(), local);
						out.push('>');
					}
					format = restore;
				}
				Step::AfterNode(id) => {
					if id != self.root && format {
						out.push('\n');
					}
				}
			}
		}
		if pretty {
			out.push('\n');
		}
		to_ascii(out)
	}
}

fn push_qname(out: &mut String, prefix: Option<&str>, local: &str) {
	if let Some(prefix) = prefix {
		out.push_str(prefix);
		out.push(':');
	}
	out.push_str(local);
}

/// `xmlBufWriteQuotedString`: namespace names are not escaped.
fn push_quoted(out: &mut String, s: &str) {
	if s.contains('"') {
		if s.contains('\'') {
			out.push('"');
			out.push_str(&s.replace('"', "&quot;"));
			out.push('"');
		} else {
			out.push('\'');
			out.push_str(s);
			out.push('\'');
		}
	} else {
		out.push('"');
		out.push_str(s);
		out.push('"');
	}
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

fn to_ascii(s: String) -> String {
	if s.is_ascii() {
		return s;
	}
	let mut out = String::with_capacity(s.len());
	for c in s.chars() {
		if c.is_ascii() {
			out.push(c);
		} else {
			out.push_str(&format!("&#{};", u32::from(c)));
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	fn plain(doc: &str) -> String {
		Document::parse(doc)
			.map(|d| d.to_string(false))
			.unwrap_or_else(|e| format!("{e:?}"))
	}

	fn pretty(doc: &str) -> String {
		Document::parse(doc)
			.map(|d| d.to_string(true))
			.unwrap_or_else(|e| format!("{e:?}"))
	}

	fn is_error(doc: &str) -> bool {
		matches!(Document::parse(doc), Err(XmlError::Syntax(_)))
	}

	/// The results of lxml 6.1.3 (libxml2 2.11.9).
	#[test]
	fn blank_text() {
		assert_eq!(
			plain(
				"<svg><a>\n  <b/>\n  text\n  <c/>\n </a>\n <d> </d>\n <e>\n </e>\n <f xml:space=\"preserve\">\n <g/>\n </f>\n <h> <i/> </h>\n</svg>\n"
			),
			"<svg><a><b/>\n  text\n  <c/>\n </a><d> </d><e>\n </e><f xml:space=\"preserve\">\n <g/>\n </f><h><i/></h></svg>"
		);
		assert_eq!(plain("<a> <![CDATA[]]> </a>"), "<a> </a>");
		assert_eq!(plain("<a><![CDATA[]]></a>"), "<a></a>");
		assert_eq!(plain("<a><b/><![CDATA[ ]]><c/></a>"), "<a><b/> <c/></a>");
		assert_eq!(plain("<a> &#x20; </a>"), "<a>   </a>");
		assert_eq!(plain("<a>&#x20;<b/></a>"), "<a> <b/></a>");
		assert_eq!(plain("<a> x <b/> <c/> </a>"), "<a> x <b/> <c/> </a>");
		assert_eq!(plain("<a>x <b/> <c/> </a>"), "<a>x <b/> <c/> </a>");
		assert_eq!(plain("<a> <b> </b> <c/></a>"), "<a><b> </b><c/></a>");
		assert_eq!(plain("<a> <!--c--> <b/></a>"), "<a><!--c--><b/></a>");
		assert_eq!(plain("<a>\r\n<b/>\r\n</a>"), "<a><b/></a>");
		assert_eq!(plain("<a> \r\n </a>"), "<a>\n </a>");
		assert_eq!(plain("<a>\t<b/>\t</a>"), "<a><b/></a>");
		assert_eq!(
			plain("<a>\u{4e2d} <b/> <c/></a>"),
			"<a>&#20013; <b/> <c/></a>"
		);
		assert_eq!(
			plain(
				"<a xml:space=\"preserve\"><b xml:space=\"default\"> <c/> </b> <d> <e/> </d></a>"
			),
			"<a xml:space=\"preserve\"><b xml:space=\"default\"><c/></b> <d> <e/> </d></a>"
		);
	}

	#[test]
	fn serialization() {
		assert_eq!(
			plain(
				"<svg xmlns=\"http://www.w3.org/2000/svg\" a=\"\u{4e2d}&#10;&#9;&lt;&quot;'&gt;&amp;\">\u{4e2d}\u{6587}&gt;&#13;\"'<g/>x<!-- c -->y<?pi d?><![CDATA[<z>&]]></svg>"
			),
			"<svg xmlns=\"http://www.w3.org/2000/svg\" a=\"&#20013;&#10;&#9;&lt;&quot;'&gt;&amp;\">&#20013;&#25991;&gt;&#13;\"'<g/>x<!-- c -->y<?pi d?>&lt;z&gt;&amp;</svg>"
		);
		assert_eq!(
			plain(
				"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- before --><?p q?><!DOCTYPE svg [<!ENTITY e \"v\">]><svg xmlns=\"http://www.w3.org/2000/svg\">&e;<g  b = \"1\"   a=\"2\" ></g></svg><!-- after -->"
			),
			"<svg xmlns=\"http://www.w3.org/2000/svg\">&e;<g b=\"1\" a=\"2\"/></svg>"
		);
		assert_eq!(
			plain(
				"<\u{4e2d} \u{6587}=\"\u{5b57}\">\u{4e2d}<!--\u{4e2d}--><?\u{4e2d} \u{6587}?></\u{4e2d}>"
			),
			"<&#20013; &#25991;=\"&#23383;\">&#20013;<!--&#20013;--><?&#20013; &#25991;?></&#20013;>"
		);
		assert_eq!(plain("<a b=\"&#10;\n\t\r\n x\"/>"), "<a b=\"&#10;    x\"/>");
		assert_eq!(plain("<a>\u{7f}\u{85}</a>"), "<a>\u{7f}&#133;</a>");
		assert_eq!(plain("\u{feff}<a/>"), "<a/>");
		assert_eq!(plain("<a><?pi   x  y  ?></a>"), "<a><?pi x  y  ?></a>");
	}

	#[test]
	fn pretty_print() {
		assert_eq!(
			pretty(
				"<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:x=\"u:x\"><x:g x:id=\"1\" id=\"2\"><h xmlns=\"u:y\"/><i xmlns=\"\"/></x:g></svg>"
			),
			"<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:x=\"u:x\">\n  <x:g x:id=\"1\" id=\"2\">\n    <h xmlns=\"u:y\"/>\n    <i xmlns=\"\"/>\n  </x:g>\n</svg>\n"
		);
		assert_eq!(
			pretty("<svg><a>text<b/></a><c><d/>tail</c><e><f/> <g/></e></svg>"),
			"<svg>\n  <a>text<b/></a>\n  <c><d/>tail</c>\n  <e>\n    <f/>\n    <g/>\n  </e>\n</svg>\n"
		);
		assert_eq!(pretty("<a><?pi?></a>"), "<a>\n  <?pi?>\n</a>\n");
		assert_eq!(pretty("<a><!-- a- --></a>"), "<a>\n  <!-- a- -->\n</a>\n");
		assert_eq!(pretty("<a>x</a>"), "<a>x</a>\n");
	}

	#[test]
	fn errors() {
		for doc in [
			"",
			" ",
			"x<a/>",
			"<a/>x",
			"<a/><b/>",
			"<a>",
			"</a>",
			"<a></b>",
			"<a></ a>",
			"< a/>",
			"<a/ >",
			"<x:a/>",
			"<a x:b=\"1\"/>",
			"<a xmlns:x=\"\"/>",
			"<a xmlns:xml=\"u\"/>",
			"<a xmlns:xmlns=\"u\"/>",
			"<a xmlns=\"http://www.w3.org/XML/1998/namespace\"/>",
			"<a xmlns=\"http://www.w3.org/2000/xmlns/\"/>",
			"<a xmlns:y=\"http://www.w3.org/2000/xmlns/\"/>",
			"<a xmlns:x=\"u\" xmlns:y=\"u\" x:b=\"1\" y:b=\"2\"/>",
			"<a b=\"1\" b=\"2\"/>",
			"<a xmlns=\"u\" xmlns=\"v\"/>",
			"<a:b:c/>",
			"<:a/>",
			"<a:/>",
			"<a b:c:d=\"1\"/>",
			"<?a:b c?><a/>",
			"<a><?xml c?></a>",
			"<a><?XML c?></a>",
			"<a xmlns=\"&lt;\"/>",
			"<a xmlns:x=\"\tb\nc\"/>",
			"<a><!-- a--b --></a>",
			"<a><!---></a>",
			"<a><!-- x ---></a>",
			"<a b=\"<\"/>",
			"<a b=c/>",
			"<a b/>",
			"<a b=\"1\"c=\"2\"/>",
			"<a>]]></a>",
			"<a>&#0;</a>",
			"<a>&#xFFFE;</a>",
			"<a>\u{1}</a>",
			"<a>\u{fffe}</a>",
			"<a>&bad;</a>",
			"<a b=\"&bad;\"/>",
			"<?xml version=\"2.0\"?><a/>",
			"<?xml?><a/>",
			"<?xml encoding=\"UTF-8\"?><a/>",
			"<?xml version=\"1.0\" standalone=\"maybe\"?><a/>",
			" <?xml version=\"1.0\"?><a/>",
			"<a/><?xml version=\"1.0\"?>",
			"<1a/>",
			"<-a/>",
			"<\u{301}a/>",
		] {
			assert!(is_error(doc), "{doc:?} parsed to {:?}", plain(doc));
		}
		assert_eq!(
			plain("<a xmlns:xml=\"http://www.w3.org/XML/1998/namespace\"/>"),
			"<a/>"
		);
	}

	#[test]
	fn unsupported() {
		for doc in [
			"<?xml version=\"1.0\" encoding=\"latin-1\"?><a/>",
			"<!DOCTYPE a SYSTEM \"x\"><a/>",
			"<!DOCTYPE a [<!ELEMENT a ANY>]><a/>",
			"<!DOCTYPE a [<!ENTITY e \"<b/>\">]><a/>",
			"<!DOCTYPE a [<!ENTITY e \"v\">]><a b=\"&e;\"/>",
		] {
			assert!(
				matches!(Document::parse(doc), Err(XmlError::Unsupported(_))),
				"{doc:?}"
			);
		}
	}

	/// Compares with `tools/reference/lxml_vectors.py`, given its output in
	/// `FONTOXIDE_LXML_VECTORS`: one case per line, the document and both results
	/// in hexadecimal, separated by tabs; `!` for an error.
	#[test]
	#[ignore = "needs the vectors of tools/reference/lxml_vectors.py"]
	fn lxml_vectors() {
		fn unhex(s: &str) -> String {
			let bytes: Vec<u8> = (0..s.len() / 2)
				.map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap_or(0))
				.collect();
			String::from_utf8(bytes).unwrap_or_default()
		}
		let path = std::env::var("FONTOXIDE_LXML_VECTORS").unwrap_or_default();
		let vectors = std::fs::read_to_string(&path).unwrap_or_default();
		assert!(!vectors.is_empty(), "no vectors at {path:?}");
		let (mut total, mut skipped, mut failed) = (0, 0, 0);
		for line in vectors.lines() {
			let mut fields = line.split('\t');
			let (Some(doc), Some(a), Some(b)) = (fields.next(), fields.next(), fields.next())
			else {
				continue;
			};
			let doc = unhex(doc);
			total += 1;
			let (ours_plain, ours_pretty) = match Document::parse(&doc) {
				Ok(d) => (d.to_string(false), d.to_string(true)),
				Err(XmlError::Syntax(_)) => ("!".to_owned(), "!".to_owned()),
				Err(XmlError::Unsupported(_)) => {
					skipped += 1;
					continue;
				}
			};
			let expected_plain = if a == "!" { "!".to_owned() } else { unhex(a) };
			let expected_pretty = if b == "!" { "!".to_owned() } else { unhex(b) };
			if ours_plain != expected_plain || ours_pretty != expected_pretty {
				failed += 1;
				if failed <= 30 {
					eprintln!(
						"{doc:?}\n  ours   {ours_plain:?}\n  lxml   {expected_plain:?}\n  ours+  {ours_pretty:?}\n  lxml+  {expected_pretty:?}"
					);
				}
			}
		}
		eprintln!("{total} vectors, {skipped} unsupported, {failed} different");
		assert_eq!(failed, 0);
	}
}
