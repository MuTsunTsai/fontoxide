//! `Tests/misc/testTools_test.py`.
//!
//! fontTools' test helpers `parseXML` and `getXML` are thin wrappers of the
//! TTX reader's content model and of `XMLWriter`; the tests of the fontoxide
//! crates write them as below, so this checks the same behaviour.
#![allow(non_snake_case)]

use fontoxide_misc::xml_reader::{self, Content, Element};
use fontoxide_misc::xml_writer::XmlWriter;

/// `testTools.parseXML`: the content of a fake root around the snippet.
fn parse_xml(snippet: &str) -> Vec<Content> {
	xml_reader::parse(&format!("<root>{snippet}</root>"))
		.expect("valid xml")
		.content
}

fn parse_xml_bytes(snippet: &[u8]) -> Vec<Content> {
	parse_xml(std::str::from_utf8(snippet).expect("utf-8"))
}

fn el(name: &str, attrs: &[(&str, &str)], content: Vec<Content>) -> Content {
	Content::Element(Element {
		name: name.to_owned(),
		attrs: attrs
			.iter()
			.map(|&(k, v)| (k.to_owned(), v.to_owned()))
			.collect(),
		content,
	})
}

fn text(s: &str) -> Content {
	Content::Text(s.to_owned())
}

fn expected_foos() -> Vec<Content> {
	vec![
		el("Foo", &[("n", "1")], vec![]),
		el(
			"Foo",
			&[("n", "2")],
			vec![
				text("    some ünıcòðe text    "),
				el("Bar", &[("color", "red")], vec![]),
				text("    some more text"),
			],
		),
		el("Foo", &[("n", "3")], vec![]),
	]
}

#[test]
fn test_parseXML_str() {
	assert_eq!(
		parse_xml(concat!(
			"<Foo n=\"1\"/>",
			"<Foo n=\"2\">",
			"    some ünıcòðe text",
			"    <Bar color=\"red\"/>",
			"    some more text",
			"</Foo>",
			"<Foo n=\"3\"/>"
		)),
		expected_foos()
	);
}

#[test]
fn test_parseXML_bytes() {
	let mut data: Vec<u8> = Vec::new();
	data.extend_from_slice(b"<Foo n=\"1\"/>");
	data.extend_from_slice(b"<Foo n=\"2\">");
	data.extend_from_slice(b"    some \xc3\xbcn\xc4\xb1c\xc3\xb2\xc3\xb0e text");
	data.extend_from_slice(b"    <Bar color=\"red\"/>");
	data.extend_from_slice(b"    some more text");
	data.extend_from_slice(b"</Foo>");
	data.extend_from_slice(b"<Foo n=\"3\"/>");
	assert_eq!(parse_xml_bytes(&data), expected_foos());
}

#[test]
fn test_parseXML_str_list() {
	let snippets = ["<Foo n=\"1\"/>", "<Foo n=\"2\"/>"];
	assert_eq!(
		parse_xml(&snippets.concat()),
		[
			el("Foo", &[("n", "1")], vec![]),
			el("Foo", &[("n", "2")], vec![])
		]
	);
}

#[test]
fn test_parseXML_bytes_list() {
	let snippets: [&[u8]; 2] = [b"<Foo n=\"1\"/>", b"<Foo n=\"2\"/>"];
	assert_eq!(
		parse_xml_bytes(&snippets.concat()),
		[
			el("Foo", &[("n", "1")], vec![]),
			el("Foo", &[("n", "2")], vec![])
		]
	);
}

#[test]
fn test_getXML() {
	let mut writer = XmlWriter::new();
	writer.simpletag("simple", &[]);
	writer.newline();
	writer.begintag("tag", &[("attr", &"value")]);
	writer.newline();
	writer.write("hello world");
	writer.newline();
	writer.endtag("tag");
	writer.newline(); // toXML always ends with a newline
	let xml = writer.into_string();
	assert!(xml.ends_with('\n'));
	// `getXML` erases the XML declaration.
	let lines: Vec<&str> = xml.lines().skip(1).collect();
	assert_eq!(
		lines,
		[
			"<simple/>",
			"<tag attr=\"value\">",
			"  hello world",
			"</tag>"
		]
	);
}
