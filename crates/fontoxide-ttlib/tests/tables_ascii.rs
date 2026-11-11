//! The `asciiTable` tables (`TTFA`, and the VTT source tables such as `TSIV`),
//! which have no upstream tests; checked against fontTools.

#![allow(clippy::unwrap_used)]

mod common;

use common::{TestResult, names};
use fontoxide_ttlib::tables::AsciiTable;
use fontoxide_ttlib::{Font, TableKey, TableType, Tag, TtxOptions};

const TTFA: Tag = Tag::new(b"TTFA");
const TSIV: Tag = Tag::new(b"TSIV");

#[test]
fn ttx_round_trip() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(names(&[".notdef"]));
	let mut ttfa = AsciiTable::new_as(TTFA);
	ttfa.data = b"a\nb\0c <&>".to_vec();
	font.insert(Box::new(ttfa));
	let mut tsiv = AsciiTable::new_as(TSIV);
	tsiv.data = b"x\ry\r".to_vec();
	font.insert(Box::new(tsiv));

	let ttx = font.to_ttx(&TtxOptions {
		tables: vec![TableKey::Table(TTFA), TableKey::Table(TSIV)],
		..Default::default()
	})?;
	let body: Vec<&str> = ttx.lines().skip(3).collect();
	assert_eq!(
		body,
		[
			"  <TTFA>",
			"    <source>",
			"a",
			"bc &lt;&amp;&gt;",
			"    </source>",
			"  </TTFA>",
			"",
			"  <TSIV>",
			"    <source>",
			"x",
			"y",
			"",
			"    </source>",
			"  </TSIV>",
			"",
			"</ttFont>",
		]
	);

	// The NUL byte is lost, and TSIV lines are separated by "\r".
	let mut font2 = Font::new();
	font2.import_ttx(&ttx)?;
	assert_eq!(font2.table_data(TTFA)?, b"a\nbc <&>");
	assert_eq!(font2.table_data(TSIV)?, b"x\ry\r");
	let table = font2.get_dyn(TSIV)?.unwrap();
	assert_eq!(table.tag(), TSIV);
	Ok(())
}
