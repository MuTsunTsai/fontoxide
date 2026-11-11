//! `Tests/ttLib/tables/_b_g_c_l_test.py`.

#![allow(clippy::unwrap_used)]

mod common;

use common::{TestResult, get_xml};
use fontoxide_ttlib::tables::Bgcl;
use fontoxide_ttlib::tables::json::loads;
use fontoxide_ttlib::{Font, Table, TableType};

const SAMPLE: &str = r#"{"colors": [[255, 0, 0, 1], [0, 255, 0, 1]], "emojicolors": [[[0], [1], []]], "indexmap": {"10": 0}, "version": 1}"#;

#[test]
fn test_compile_decompile_roundtrip() -> TestResult {
	let mut font = Font::new();
	let sample = loads(SAMPLE).unwrap();
	let mut t1 = Bgcl {
		json: Some(sample.clone()),
		..Default::default()
	};
	let data = t1.compile(&mut font)?;
	assert_eq!(
		data,
		br#"{"colors":[[255,0,0,1],[0,255,0,1]],"emojicolors":[[[0],[1],[]]],"indexmap":{"10":0},"version":1}"#
	);

	let t2 = Bgcl::decompile(&data, &mut font)?;
	assert_eq!(t2.json, Some(sample));
	assert_eq!(t2.error, None);
	Ok(())
}

/// Invalid JSON is kept as raw data with the error, as fontTools does.
#[test]
fn invalid_json_is_kept_raw() -> TestResult {
	let mut font = Font::new();
	let mut table = Bgcl::decompile(b"{\"a\": \xff}", &mut font)?;
	assert_eq!(table.json, None);
	assert_eq!(
		table.decompile_error(),
		Some(
			"bgcl JSON parse error: UnicodeDecodeError('utf-8', b'{\"a\": \\xff}', 6, 7, 'invalid start byte')"
		)
	);
	assert_eq!(table.compile(&mut font)?, b"{\"a\": \xff}");
	assert_eq!(
		get_xml(&table, &mut font)?,
		[
			"<!-- An error occurred during the decompilation of this table -->",
			"<!-- bgcl JSON parse error: UnicodeDecodeError('utf-8', b'{\"a\": \\xff}', 6, 7, 'invalid start byte') -->",
			"<hexdata>",
			"  7b226122 3a20ff7d  ",
			"</hexdata>",
		]
	);
	let table = Bgcl::decompile(b"[1,]", &mut font)?;
	assert_eq!(
		table.decompile_error(),
		Some(
			"bgcl JSON parse error: JSONDecodeError('Illegal trailing comma before end of array: line 1 column 3 (char 2)')"
		)
	);
	Ok(())
}

/// The TTX dump, checked against fontTools.
#[test]
fn to_xml() -> TestResult {
	let mut font = Font::new();
	let table = Bgcl::decompile("{\"a\":[1,\"é\"],\"b\":{}}".as_bytes(), &mut font)?;
	assert_eq!(
		get_xml(&table, &mut font)?,
		[
			"<json>",
			"  <![CDATA[{",
			"  \"a\": [",
			"    1,",
			"    \"é\"",
			"  ],",
			"  \"b\": {}",
			"}]]>",
			"</json>",
		]
	);
	Ok(())
}
