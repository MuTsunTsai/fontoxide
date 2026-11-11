//! `Tests/misc/etree_test.py`.
//!
//! The entity expansion tests run against
//! [`xml_reader`](fontoxide_misc::xml_reader), which parses fontoxide's XML
//! files.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_misc::etree::{Element, XML_NS, fromstring, tostring};
use fontoxide_misc::xml_reader;

fn tmp_dir(name: &str) -> PathBuf {
	static COUNTER: AtomicUsize = AtomicUsize::new(0);
	let dir = std::env::temp_dir().join(format!(
		"fontoxide-misc-etree-{}-{name}-{}",
		std::process::id(),
		COUNTER.fetch_add(1, Ordering::Relaxed)
	));
	std::fs::create_dir_all(&dir).expect("create temp dir");
	dir
}

/// `Path.as_uri()`.
fn file_uri(path: &std::path::Path) -> String {
	let s = path.to_string_lossy().replace('\\', "/");
	if s.starts_with('/') {
		format!("file://{s}")
	} else {
		format!("file:///{s}")
	}
}

#[test]
fn test_roundtrip_string() {
	for (id, xml) in [
		(
			"simple_xml_no_indent",
			concat!(
				"<root>",
				"<element key=\"value\">text</element>",
				"<element>text</element>tail",
				"<empty-element/>",
				"</root>"
			),
		),
		(
			"simple_xml_indent",
			concat!(
				"<root>\n",
				"  <element key=\"value\">text</element>\n",
				"  <element>text</element>tail\n",
				"  <empty-element/>\n",
				"</root>"
			),
		),
		(
			"xml_ns_attrib_utf_8",
			concat!(
				"<axis default=\"400\" maximum=\"1000\" minimum=\"1\" name=\"weight\" tag=\"wght\">",
				"<labelname xml:lang=\"fa-IR\">قطر</labelname>",
				"</axis>"
			),
		),
	] {
		let root = fromstring(xml).expect(id);
		let result = tostring(&root, false, false).expect(id);
		assert_eq!(result, xml, "{id}");
	}
}

/// Tails, with pretty printing, which does not format mixed content (checked
/// against lxml).
#[test]
fn tails_are_mixed_content() {
	let root = fromstring("<a><b>x</b>t<c><d/></c></a>").expect("valid");
	assert_eq!(root.children[0].tail.as_deref(), Some("t"));
	assert_eq!(
		tostring(&root, true, false).expect("serializable"),
		"<a><b>x</b>t<c><d/></c></a>\n"
	);
	let root = fromstring("<a><b/><c><d/>t</c></a>").expect("valid");
	assert_eq!(
		tostring(&root, true, false).expect("serializable"),
		"<a>\n  <b/>\n  <c><d/>t</c>\n</a>\n"
	);
}

/// The `xml_ns_attrib_utf_8` case of `test_roundtrip_string`, built instead of
/// parsed.
#[test]
fn test_roundtrip_string_built() {
	let mut root = Element::new("axis");
	for (k, v) in [
		("default", "400"),
		("maximum", "1000"),
		("minimum", "1"),
		("name", "weight"),
		("tag", "wght"),
	] {
		root.set(k, v);
	}
	let mut label = Element::new("labelname");
	label.set(format!("{XML_NS}lang"), "fa-IR");
	label.text = Some("قطر".into());
	root.append(label);
	assert_eq!(
		tostring(&root, false, false).expect("serializable"),
		"<axis default=\"400\" maximum=\"1000\" minimum=\"1\" name=\"weight\" tag=\"wght\"><labelname xml:lang=\"fa-IR\">قطر</labelname></axis>"
	);
}

#[test]
fn test_pretty_print() {
	let mut root = Element::new("root");
	let mut element = Element::new("element");
	for (k, v) in [("c", "2"), ("b", "1"), ("a", "0")] {
		element.set(k, v);
	}
	element.text = Some("text".into());
	root.append(element);
	let mut element = Element::new("element");
	element.text = Some("text".into());
	root.append(element);
	root.append(Element::new("empty-element"));

	assert_eq!(
		tostring(&root, true, false).expect("serializable"),
		"<root>\n  <element c=\"2\" b=\"1\" a=\"0\">text</element>\n  <element>text</element>\n  <empty-element/>\n</root>\n"
	);
}

fn assert_secret_not_leaked(xml: &str) {
	match xml_reader::parse(xml) {
		// the undefined entity is rejected outright; the file must not have
		// been read into the error message either
		Err(e) => assert!(!e.to_string().contains("s3cr3t"), "{e}"),
		Ok(root) => assert!(!format!("{root:?}").contains("s3cr3t")),
	}
}

#[test]
fn test_no_external_entity_expansion() {
	let dir = tmp_dir("xxe");
	let secret = dir.join("secret.txt");
	std::fs::write(&secret, "s3cr3t").expect("write");
	let xml = format!(
		"<?xml version=\"1.0\"?><!DOCTYPE root [<!ENTITY xxe SYSTEM \"{}\">]><root>&xxe;</root>",
		file_uri(&secret)
	);
	assert_secret_not_leaked(&xml);
	let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_no_external_parameter_entity_expansion() {
	let dir = tmp_dir("xpe");
	let secret = dir.join("secret.txt");
	std::fs::write(&secret, "s3cr3t").expect("write");
	let dtd = dir.join("evil.dtd");
	std::fs::write(
		&dtd,
		format!(
			"<!ENTITY % sec SYSTEM \"{}\">\n<!ENTITY leak \"%sec;\">\n",
			file_uri(&secret)
		),
	)
	.expect("write");
	let xml = format!(
		"<?xml version=\"1.0\"?><!DOCTYPE root [<!ENTITY % dtd SYSTEM \"{}\"> %dtd;]><root>&leak;</root>",
		file_uri(&dtd)
	);
	assert_secret_not_leaked(&xml);
	let _ = std::fs::remove_dir_all(dir);
}
