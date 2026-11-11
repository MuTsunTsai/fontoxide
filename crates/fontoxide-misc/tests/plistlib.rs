//! Port of fontTools' `Tests/misc/plistlib_test.py`.
//!
//! fontTools can read `<data>` as `bytes` or as its `Data` class
//! (`use_builtin_types`); here there is only one data type, so the tests
//! parametrized over it run once. Not ported: the tests of `Data` itself
//! (`test__test_pl`), of Python objects that are not plist values
//! (`test_invalid_type`, `test_custom_mapping`, `test_keys_no_string`,
//! `test_skipkeys`), of bytes written as strings (`test_bytes_string`,
//! `test_non_ascii_bytes`), of the deprecated `fontTools.ufoLib.plistlib`
//! functions (`readPlist`, `writePlist` and the like), and
//! `test_apple_formatting`, an expected failure.

use fontoxide_misc::etree::Element;
use fontoxide_misc::plistlib::{
	self, Date, Dict, PlistError, TreeOptions, Value, date_from_string,
};
use fontoxide_misc::xml_reader;

type Result<T> = std::result::Result<T, PlistError>;

fn datadir() -> std::path::PathBuf {
	std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/misc/testdata")
}

/// `TESTDATA`: `test.plist`, generated with Cocoa.
fn testdata() -> Vec<u8> {
	std::fs::read(datadir().join("test.plist")).expect("test.plist")
}

fn dict(items: Vec<(&str, Value)>) -> Value {
	Value::Dict(items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn list(items: Vec<Value>) -> Value {
	Value::Array(items)
}

fn gunk() -> Vec<u8> {
	b"<lots of binary gunk>\0\x01\x02\x03".repeat(10)
}

/// `_test_pl`.
fn test_pl() -> Value {
	let mut pl = match dict(vec![
		("aString", "Doodah".into()),
		(
			"aList",
			list(vec![
				"A".into(),
				"B".into(),
				12.into(),
				32.5.into(),
				list(vec![1.into(), 2.into(), 3.into()]),
			]),
		),
		("aFloat", 0.5.into()),
		("anInt", 728.into()),
		("aBigInt", Value::Integer((1 << 63) - 44)),
		("aBigInt2", Value::Integer((1 << 63) + 44)),
		("aNegativeInt", (-5).into()),
		("aNegativeBigInt", Value::Integer(-80_000_000_000)),
		(
			"aDict",
			dict(vec![
				("anotherString", "<hello & 'hi' there!>".into()),
				("aUnicodeValue", "M\u{e4}ssig, Ma\u{df}".into()),
				("aTrueValue", true.into()),
				("aFalseValue", false.into()),
				(
					"deeperDict",
					dict(vec![
						("a", 17.into()),
						("b", 32.5.into()),
						("c", list(vec![1.into(), 2.into(), "text".into()])),
					]),
				),
			]),
		),
		("someData", Value::Data(b"<binary gunk>".to_vec())),
		("someMoreData", Value::Data(gunk())),
		("nestedData", list(vec![Value::Data(gunk())])),
		(
			"aDate",
			Value::Date(Date {
				year: 2004,
				month: 10,
				day: 26,
				hour: 10,
				minute: 33,
				second: 33,
			}),
		),
		("anEmptyDict", dict(vec![])),
		("anEmptyList", list(vec![])),
	]) {
		Value::Dict(d) => d,
		_ => unreachable!(),
	};
	pl.insert("\u{c5}benraa".into(), "That was a unicode key.".into());
	Value::Dict(pl)
}

fn dumps(value: &Value) -> Result<String> {
	plistlib::dumps(value, true, true)
}

#[test]
fn test_io() -> Result<()> {
	let pl = test_pl();
	let dir = std::env::temp_dir().join(format!("fontoxide-plistlib-{}", std::process::id()));
	std::fs::create_dir_all(&dir).expect("mkdir");
	let testpath = dir.join("test.plist");
	std::fs::write(&testpath, dumps(&pl)?).expect("write");
	let pl2 = plistlib::loads_bytes(&std::fs::read(&testpath).expect("read"))?;
	let _ = std::fs::remove_dir_all(&dir);
	assert_eq!(pl, pl2);
	Ok(())
}

#[test]
fn test_int() -> Result<()> {
	for pl in [
		0i128,
		(1 << 8) - 1,
		1 << 8,
		(1 << 16) - 1,
		1 << 16,
		(1 << 32) - 1,
		1 << 32,
		(1 << 63) - 1,
		(1 << 64) - 1,
		1,
		-(1 << 63),
	] {
		let data = dumps(&Value::Integer(pl))?;
		let pl2 = plistlib::loads(&data)?;
		assert!(matches!(pl2, Value::Integer(i) if i == pl));
		assert_eq!(data, dumps(&pl2)?);
	}
	Ok(())
}

#[test]
fn test_int_overflow() {
	for pl in [(1i128 << 64) + 1, i128::MAX, -(1 << 64), i128::MIN] {
		assert!(matches!(
			dumps(&Value::Integer(pl)),
			Err(PlistError::Overflow(_))
		));
	}
}

#[test]
fn test_bytearray() -> Result<()> {
	let pl = Value::Data(b"<binary gunk\0\x01\x02\x03>".to_vec());
	let data = dumps(&pl)?;
	let pl2 = plistlib::loads(&data)?;
	assert!(matches!(pl2, Value::Data(_)));
	assert_eq!(pl2, pl);
	assert_eq!(data, dumps(&pl2)?);
	Ok(())
}

#[test]
fn test_bytes_data() -> Result<()> {
	test_bytearray()
}

#[test]
fn test_indentation_array() -> Result<()> {
	let data = plistlib::loads(
		r#"<plist><array><array><array><array><array><array><array><array><dict><key>test</key><string>aaaaaa</string></dict></array></array></array></array></array></array></array></array></plist>"#,
	)?;
	assert_eq!(plistlib::loads(&dumps(&data)?)?, data);
	Ok(())
}

#[test]
fn test_indentation_dict() -> Result<()> {
	let mut data = dict(vec![("9", "aaaaaa".into())]);
	for key in ["8", "7", "6", "5", "4", "3", "2", "1"] {
		data = dict(vec![(key, data)]);
	}
	assert_eq!(plistlib::loads(&dumps(&data)?)?, data);
	Ok(())
}

#[test]
fn test_indentation_dict_mix() -> Result<()> {
	let mut inner = list(vec![dict(vec![("test", "aaaaaa".into())])]);
	for _ in 0..4 {
		inner = list(vec![inner]);
	}
	let data = dict(vec![(
		"1",
		dict(vec![("2", list(vec![dict(vec![("3", inner)])]))]),
	)]);
	assert_eq!(plistlib::loads(&dumps(&data)?)?, data);
	Ok(())
}

#[test]
fn test_apple_formatting_fromliteral() -> Result<()> {
	let pl2 = plistlib::loads_bytes(&testdata())?;
	assert_eq!(test_pl(), pl2);
	Ok(())
}

#[test]
fn test_apple_roundtrips() -> Result<()> {
	let pl = plistlib::loads_bytes(&testdata())?;
	let data = dumps(&pl)?;
	let pl2 = plistlib::loads(&data)?;
	let data2 = dumps(&pl2)?;
	assert_eq!(data, data2);
	Ok(())
}

#[test]
fn test_bytesio() -> Result<()> {
	let pl = test_pl();
	let b = dumps(&pl)?.into_bytes();
	assert_eq!(pl, plistlib::loads_bytes(&b)?);
	Ok(())
}

fn ordered() -> Value {
	dict(vec![("b", 1.into()), ("a", 2.into()), ("c", 3.into())])
}

fn keys(value: &Value) -> Vec<&str> {
	value
		.as_dict()
		.expect("dict")
		.keys()
		.map(String::as_str)
		.collect()
}

#[test]
fn test_keysort_bytesio() -> Result<()> {
	for sort_keys in [false, true] {
		let pl = ordered();
		let b = plistlib::dumps(&pl, sort_keys, true)?.into_bytes();
		let pl2 = plistlib::loads_bytes(&b)?;
		assert_eq!(pl, pl2);
		if sort_keys {
			assert_eq!(keys(&pl2), ["a", "b", "c"]);
		} else {
			assert_eq!(keys(&pl2), ["b", "a", "c"]);
		}
	}
	Ok(())
}

#[test]
fn test_keysort() -> Result<()> {
	for sort_keys in [false, true] {
		let pl = ordered();
		let data = plistlib::dumps(&pl, sort_keys, true)?;
		let pl2 = plistlib::loads(&data)?;
		assert_eq!(pl, pl2);
		if sort_keys {
			assert_eq!(keys(&pl2), ["a", "b", "c"]);
		} else {
			assert_eq!(keys(&pl2), ["b", "a", "c"]);
		}
	}
	Ok(())
}

fn members(first: Value, second: Value, third: Value) -> Result<()> {
	let pl = dict(vec![
		("first", first.clone()),
		("second", second),
		("third", third.clone()),
	]);
	let pl2 = plistlib::loads(&dumps(&pl)?)?;
	assert_eq!(pl2, pl);
	// the two equal values are distinct objects in fontTools; Rust values are
	// never shared
	Ok(())
}

#[test]
fn test_tuple_members() -> Result<()> {
	// tuples are written as arrays
	members(
		list(vec![1.into(), 2.into()]),
		list(vec![1.into(), 2.into()]),
		list(vec![3.into(), 4.into()]),
	)
}

#[test]
fn test_list_members() -> Result<()> {
	members(
		list(vec![1.into(), 2.into()]),
		list(vec![1.into(), 2.into()]),
		list(vec![3.into(), 4.into()]),
	)
}

#[test]
fn test_dict_members() -> Result<()> {
	members(
		dict(vec![("a", 1.into())]),
		dict(vec![("a", 1.into())]),
		dict(vec![("b", 2.into())]),
	)
}

#[test]
fn test_controlcharacters() -> Result<()> {
	for i in 0u8..128 {
		let c = char::from(i);
		let test_string = format!("string containing {c}");
		if i >= 32 || "\r\n\t".contains(c) {
			// \r, \n and \t are the only legal control chars in XML; lxml
			// escapes \r, so it roundtrips
			let data = dumps(&Value::String(test_string.clone()))?;
			assert_eq!(plistlib::loads(&data)?, Value::String(test_string));
		} else {
			assert!(dumps(&Value::String(test_string)).is_err(), "{i}");
		}
	}
	Ok(())
}

#[test]
fn test_non_bmp_characters() -> Result<()> {
	let pl = dict(vec![("python", "\u{1f40d}".into())]);
	assert_eq!(plistlib::loads(&dumps(&pl)?)?, pl);
	Ok(())
}

#[test]
fn test_nondictroot() -> Result<()> {
	let test1: Value = "abc".into();
	let test2 = list(vec![1.into(), 2.into(), 3.into(), "abc".into()]);
	assert_eq!(plistlib::loads(&dumps(&test1)?)?, test1);
	assert_eq!(plistlib::loads(&dumps(&test2)?)?, test2);
	Ok(())
}

#[test]
fn test_invalidarray() {
	for i in [
		"<key>key inside an array</key>",
		"<key>key inside an array2</key><real>3</real>",
		"<true/><key>key inside an array3</key>",
	] {
		assert!(
			plistlib::loads(&format!("<plist><array>{i}</array></plist>")).is_err(),
			"{i}"
		);
	}
}

#[test]
fn test_invaliddict() {
	for i in [
		"<key><true/>k</key><string>compound key</string>",
		"<key>single key</key>",
		"<string>missing key</string>",
		"<key>k1</key><string>v1</string><real>5.3</real><key>k1</key><key>k2</key><string>double key</string>",
	] {
		assert!(
			plistlib::loads(&format!("<plist><dict>{i}</dict></plist>")).is_err(),
			"{i}"
		);
		assert!(
			plistlib::loads(&format!("<plist><array><dict>{i}</dict></array></plist>")).is_err(),
			"{i}"
		);
	}
}

#[test]
fn test_invalidinteger() {
	assert!(plistlib::loads("<plist><integer>not integer</integer></plist>").is_err());
}

#[test]
fn test_invalidreal() {
	assert!(plistlib::loads("<plist><integer>not real</integer></plist>").is_err());
}

fn encode_utf16(text: &str, big_endian: bool) -> Vec<u8> {
	text.encode_utf16()
		.flat_map(|u| {
			if big_endian {
				u.to_be_bytes()
			} else {
				u.to_le_bytes()
			}
		})
		.collect()
}

#[test]
fn test_xml_encodings() -> Result<()> {
	let pl = test_pl();
	let text = String::from_utf8(testdata()).expect("utf-8");
	// utf-8
	let mut data = b"\xef\xbb\xbf".to_vec();
	data.extend(text.as_bytes());
	assert_eq!(plistlib::loads_bytes(&data)?, pl);
	// utf-16-le and utf-16-be
	let text = text.replace("UTF-8", "utf-16");
	let mut data = b"\xff\xfe".to_vec();
	data.extend(encode_utf16(&text, false));
	assert_eq!(plistlib::loads_bytes(&data)?, pl);
	let mut data = b"\xfe\xff".to_vec();
	data.extend(encode_utf16(&text, true));
	assert_eq!(plistlib::loads_bytes(&data)?, pl);
	Ok(())
}

#[test]
fn test_fromtree() -> Result<()> {
	let text = String::from_utf8(testdata()).expect("utf-8");
	let tree = xml_reader::parse(&text).expect("xml");
	assert_eq!(plistlib::fromtree(&tree)?, test_pl());
	Ok(())
}

/// `_strip`: the text without the whitespace around its lines.
fn strip(text: Option<&str>) -> String {
	text.map(|t| t.lines().map(str::trim).collect())
		.unwrap_or_default()
}

fn compare(e1: &xml_reader::Element, e2: &Element) {
	assert_eq!(e1.name, e2.tag);
	assert_eq!(e1.attrs, e2.attrib);
	let children1: Vec<&xml_reader::Element> = e1.elements().collect();
	assert_eq!(children1.len(), e2.children.len());
	// ignore whitespace
	assert_eq!(
		strip(plistlib::element_text(e1).as_deref()),
		strip(e2.text.as_deref())
	);
	for (c1, c2) in children1.into_iter().zip(&e2.children) {
		compare(c1, c2);
	}
}

#[test]
fn test_totree() -> Result<()> {
	let text = String::from_utf8(testdata()).expect("utf-8");
	let root = xml_reader::parse(&text).expect("xml");
	let tree = root.elements().next().expect("dict"); // ignore root 'plist' element
	let tree2 = plistlib::totree(&test_pl(), TreeOptions::default())?;
	assert_eq!(tree.name, "dict");
	assert_eq!(tree2.tag, "dict");
	// fontTools sorts the keys as Cocoa does
	compare(tree, &tree2);
	Ok(())
}

#[test]
fn test_no_pretty_print() -> Result<()> {
	let data = plistlib::dumps(
		&dict(vec![("data", Value::Data(b"hello".to_vec()))]),
		true,
		false,
	)?;
	assert_eq!(
		data,
		concat!(
			"<?xml version='1.0' encoding='UTF-8'?>",
			"<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">",
			"<plist version=\"1.0\">",
			"<dict>",
			"<key>data</key>",
			"<data>aGVsbG8=</data>",
			"</dict>",
			"</plist>"
		)
	);
	Ok(())
}

#[test]
fn test_load_use_builtin_types_default() -> Result<()> {
	let pl = plistlib::loads_bytes(&testdata())?;
	assert!(matches!(
		pl.as_dict().and_then(|d| d.get("someData")),
		Some(Value::Data(_))
	));
	Ok(())
}

#[test]
fn test_dump_use_builtin_types_default() -> Result<()> {
	let pl = test_pl();
	let pl2 = plistlib::loads(&dumps(&pl)?)?;
	assert!(matches!(
		pl2.as_dict().and_then(|d| d.get("someData")),
		Some(Value::Data(_))
	));
	assert_eq!(pl2, pl);
	Ok(())
}

#[test]
fn test_load_does_not_resolve_external_entities() {
	let dir = std::env::temp_dir().join(format!("fontoxide-plistlib-xxe-{}", std::process::id()));
	std::fs::create_dir_all(&dir).expect("mkdir");
	let secret = dir.join("secret.txt");
	std::fs::write(&secret, "s3cr3t").expect("write");
	let uri = format!(
		"file:///{}",
		secret.display().to_string().replace('\\', "/")
	);
	let data = format!(
		"<?xml version=\"1.0\"?><!DOCTYPE plist [<!ENTITY xxe SYSTEM \"{uri}\">]><plist version=\"1.0\"><dict><key>note</key><string>&xxe;</string></dict></plist>"
	);
	let result = plistlib::loads_bytes(data.as_bytes());
	let _ = std::fs::remove_dir_all(&dir);
	match result {
		// the undefined entity is rejected outright
		Err(_) => {}
		Ok(pl) => {
			let note = pl
				.as_dict()
				.and_then(|d| d.get("note"))
				.and_then(Value::as_str);
			assert!(!note.unwrap_or_default().contains("s3cr3t"));
		}
	}
}

/// Not in fontTools: dates as written by Cocoa.
#[test]
fn test_date_from_string() -> Result<()> {
	assert_eq!(
		date_from_string("2004-10-26T10:33:33Z")?,
		Date {
			year: 2004,
			month: 10,
			day: 26,
			hour: 10,
			minute: 33,
			second: 33
		}
	);
	let _ = Dict::new();
	Ok(())
}
