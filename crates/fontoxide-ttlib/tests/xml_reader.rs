//! `Tests/misc/xmlReader_test.py`.
//!
//! fontoxide reads TTX from a string ([`Font::import_ttx`]) with the content
//! model of [`fontoxide_misc::xml_reader`]; the tests about closing the file
//! objects and about the progress callback have no counterpart.

use fontoxide_misc::xml_reader::{self, Content, Element};
use fontoxide_ttlib::Font;
use fontoxide_ttlib::tables::Name;

/// The element at depth 3 (`stackSize == 3` in `_endElementHandler`): the
/// first child of the first table.
fn first_table_child(root: &Element) -> &Element {
	root.elements()
		.next()
		.and_then(|table| table.elements().next())
		.expect("table child")
}

#[test]
fn test_decode_utf8() {
	let expected = "fôôbär";
	let data = format!(
		"<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<ttFont>
  <name>
    <namerecord nameID=\"1\" platformID=\"3\" platEncID=\"1\" langID=\"0x409\">
      {expected}
    </namerecord>
  </name>
</ttFont>
"
	);
	let root = xml_reader::parse(&data).expect("valid xml");
	let content: String = first_table_child(&root).text();
	assert_eq!(content.trim(), expected);

	// And through the name table.
	let mut font = Font::from_ttx(&data).expect("valid ttx");
	let name = font.get::<Name>().expect("name").expect("name table");
	assert_eq!(name.names[0].to_unicode().as_deref(), Ok(expected));
}

fn collect_newlines(content: &[Content], out: &mut Vec<char>) {
	for c in content {
		match c {
			Content::Text(t) => out.extend(t.chars().filter(|&c| c == '\r' || c == '\n')),
			Content::Element(e) => collect_newlines(&e.content, out),
		}
	}
}

#[test]
fn test_normalise_newlines() {
	// notice how when CR is escaped, it is not normalised by the XML parser
	let data = concat!(
		"<ttFont>\r",                                   // \r -> \n
		"  <test>\r\n",                                 // \r\n -> \n
		"    a line of text\n",                         // \n
		"    escaped CR and unix newline &#13;\n",      // &#13;\n -> \r\n
		"    escaped CR and macintosh newline &#13;\r", // &#13;\r -> \r\n
		"    escaped CR and windows newline &#13;\r\n", // &#13;\r\n -> \r\n
		"  </test>\n",                                  // \n
		"</ttFont>"
	);
	let root = xml_reader::parse(data).expect("valid xml");
	let mut newlines = Vec::new();
	collect_newlines(&root.content, &mut newlines);
	let mut expected = vec!['\n'; 3];
	for _ in 0..3 {
		expected.extend(['\r', '\n']);
	}
	expected.push('\n');
	assert_eq!(newlines, expected);
}

/// The error part of the test: the file being closed is ownership in Rust.
#[test]
fn test_owned_file_closed_after_malformed_xml() {
	assert!(Font::from_ttx("<ttFont>").is_err());
}

/// The error part of the test: the file being closed is ownership in Rust.
#[test]
fn test_owned_file_closed_after_table_callback_error() {
	assert!(
		Font::from_ttx("<ttFont><ZZZZ raw=\"True\"><hexdata>zz</hexdata></ZZZZ></ttFont>").is_err()
	);
}

/// The error part of the test: a borrowed string is left alone.
#[test]
fn test_borrowed_stream_kept_open_after_malformed_xml() {
	let source = String::from("<ttFont>");
	assert!(Font::from_ttx(&source).is_err());
	assert_eq!(source, "<ttFont>");
}

/// Sub-file content (an element of a table, read with `contentOnly`) is read
/// into the table.
#[test]
fn test_read_sub_file() -> Result<(), Box<dyn std::error::Error>> {
	let expected_content = "testContent";
	let expected_name_id = "1";
	let expected_platform = "3";
	let expected_lang_id = "0x409";

	let dir = std::env::temp_dir().join(format!(
		"fontoxide-ttlib-xml-reader-sub-file-{}",
		std::process::id()
	));
	std::fs::create_dir_all(&dir)?;
	let tmp = dir.join("sub.ttx");
	std::fs::write(
		&tmp,
		format!(
			"<ttFont ttLibVersion=\"3.15\"><name><namerecord nameID=\"{expected_name_id}\" \
			 platformID=\"{expected_platform}\" platEncID=\"1\" langID=\"{expected_lang_id}\">\
			 {expected_content}</namerecord></name></ttFont>"
		),
	)?;
	let tmp2 = dir.join("main.ttx");
	std::fs::write(
		&tmp2,
		format!(
			"<ttFont ttLibVersion=\"3.15\"><name><namerecord src=\"{}\"/></name></ttFont>",
			tmp.to_string_lossy()
		),
	)?;

	let mut ttf = Font::new();
	let result = ttf.import_ttx_file(&tmp2);
	let _ = std::fs::remove_dir_all(&dir);
	result?;
	let name_table = ttf.get::<Name>()?.ok_or("no name table")?;
	let record = &name_table.names[0];
	assert_eq!(record.name_id, 1);
	assert_eq!(record.lang_id, 0x409);
	assert_eq!(record.platform_id, 3);
	assert_eq!(record.to_unicode().as_deref(), Ok(expected_content));
	Ok(())
}
