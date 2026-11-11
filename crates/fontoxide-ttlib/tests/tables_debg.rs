//! `Tests/ttLib/tables/D__e_b_g_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::{TestResult, from_xml, get_xml};
use fontoxide_ttlib::tables::Debg;
use fontoxide_ttlib::tables::json::{JsonValue, loads};
use fontoxide_ttlib::{Font, Table, TableType, Tag};

const DEBG_XML: &str = r#"<json>
  <![CDATA[{
    "com.github.fonttools.feaLib": {
      "GPOS": {
        "0": [
          "<features>:6:5",
          "kern_Default",
          [
            "DFLT",
            "dflt",
            "kern"
          ]
        ]
      }
    }
  }]]>
</json>
"#;

fn debg_data() -> JsonValue {
	loads(
		r#"{"com.github.fonttools.feaLib": {"GPOS": {"0": ["<features>:6:5", "kern_Default", ["DFLT", "dflt", "kern"]]}}}"#,
	)
	.unwrap()
}

#[test]
fn test_compile_decompile_and_roundtrip_ttx() -> TestResult {
	let mut font = Font::new();
	let mut table = Debg::default();
	assert_eq!(table.tag(), Tag::new(b"Debg"));
	assert_eq!(table.data, JsonValue::Object(Vec::new()));
	table.data = debg_data();

	let data = table.compile(&mut font)?;

	let table2 = Debg::decompile(&data, &mut font)?;
	assert_eq!(table.data, table2.data);

	assert_eq!(
		get_xml(&table2, &mut font)?,
		DEBG_XML.lines().collect::<Vec<_>>()
	);

	let mut table3 = Debg::default();
	from_xml(&mut table3, DEBG_XML, &mut font)?;
	assert_eq!(table3.data, debg_data());
	Ok(())
}

/// The CDATA indentation follows the writer's indentation level, as in a TTX
/// dump; checked against fontTools.
#[test]
fn ttx_dump_indents_json() -> TestResult {
	let mut font = Font::new();
	font.set_glyph_order(vec![".notdef".into()]);
	let table = Debg {
		data: loads(r#"{"a": [1, {"b": null}], "c": {}}"#).unwrap(),
	};
	font.insert(Box::new(table));
	let ttx = font.to_ttx(&fontoxide_ttlib::TtxOptions {
		tables: vec![fontoxide_ttlib::TableKey::Table(Tag::new(b"Debg"))],
		..Default::default()
	})?;
	let body: Vec<&str> = ttx.lines().skip(3).collect();
	assert_eq!(
		body,
		[
			"  <Debg>",
			"    <json>",
			"      <![CDATA[{",
			"        \"a\": [",
			"          1,",
			"          {",
			"            \"b\": null",
			"          }",
			"        ],",
			"        \"c\": {}",
			"      }]]>",
			"    </json>",
			"  </Debg>",
			"",
			"</ttFont>",
		]
	);
	Ok(())
}
