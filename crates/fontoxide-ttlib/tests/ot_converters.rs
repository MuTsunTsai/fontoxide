//! `Tests/ttLib/tables/otConverters_test.py`.
//!
//! The converters are not objects here: the generated code calls a function for
//! each kind of field, so the tests call those functions (`read_char64`,
//! `read_aat_lookup`, ...). `LazyListTest` is not ported, as nothing is read
//! lazily; the `lazy` parameters of the CFF2 INDEX tests are dropped for the same
//! reason (L4).

#![allow(clippy::unwrap_used)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Once;

use fontoxide_misc::text_tools::de_hex_str;
use fontoxide_misc::xml_reader;
use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_ttlib::ot::aat::{
	AatLookup, read_aat_lookup, read_aat_lookup_xml, write_aat_lookup, xml_aat_lookup,
};
use fontoxide_ttlib::ot::conv::{read_char64, write_char64, write_glyph, xml_name_id};
use fontoxide_ttlib::ot::table::{XmlCtx, parse_int, value_attr, xml_simple};
use fontoxide_ttlib::ot::varc::read_index;
use fontoxide_ttlib::ot::{OtReader, OtWriter, WriterGraph};
use fontoxide_ttlib::tables::Name;
use fontoxide_ttlib::tables::otl::Varc;
use fontoxide_ttlib::{Font, ReadOptions, Tag, WriteOptions};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn hex(s: &str) -> Vec<u8> {
	de_hex_str(s).unwrap()
}

fn reader(data: &[u8]) -> OtReader<'_> {
	OtReader::new(data, Tag::new(b"test"))
}

/// The data of a writer that `f` writes into (`writer.getData()`).
fn written(f: impl FnOnce(&mut OtWriter<'_>) -> fontoxide_ttlib::Result<()>) -> Vec<u8> {
	let mut graph = WriterGraph::new(Tag::new(b"test"));
	f(&mut graph.root()).unwrap();
	graph.get_all_data().unwrap()
}

/// `FakeFont(glyphs)`.
fn fake_font(glyphs: &str) -> Font {
	let mut font = Font::new();
	font.set_glyph_order(glyphs.split_whitespace().map(str::to_owned).collect());
	font
}

/// The lines written by `f` (`makeXMLWriter` and `getvalue()`), without the XML
/// declaration.
fn xml(f: impl FnOnce(&mut XmlWriter) -> fontoxide_ttlib::Result<()>) -> Vec<String> {
	let mut xw = XmlWriter::new();
	f(&mut xw).unwrap();
	xw.as_str().lines().skip(1).map(str::to_owned).collect()
}

fn element(text: &str) -> xml_reader::Element {
	xml_reader::parse(text).unwrap()
}

thread_local! {
	static CAPTURED: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

struct Capture;

impl log::Log for Capture {
	fn enabled(&self, _: &log::Metadata<'_>) -> bool {
		true
	}

	fn log(&self, record: &log::Record<'_>) {
		if record.level() <= log::Level::Warn {
			CAPTURED.with(|c| {
				if let Some(records) = c.borrow_mut().as_mut() {
					records.push(record.args().to_string());
				}
			});
		}
	}

	fn flush(&self) {}
}

static CAPTURE: Capture = Capture;
static INIT: Once = Once::new();

/// The warnings `f` logs on this thread (`CapturingLogHandler(log, "WARNING")`).
fn capture<R>(f: impl FnOnce() -> R) -> (Vec<String>, R) {
	INIT.call_once(|| {
		let _ = log::set_logger(&CAPTURE);
		log::set_max_level(log::LevelFilter::Warn);
	});
	CAPTURED.with(|c| *c.borrow_mut() = Some(Vec::new()));
	let result = f();
	let records = CAPTURED.with(|c| c.borrow_mut().take()).unwrap_or_default();
	(records, result)
}

// Char64Test

#[test]
fn char64_test_read() {
	let mut data = b"Hello\0junk after zero byte".to_vec();
	data.extend([0; 100]);
	let mut r = reader(&data);
	assert_eq!(read_char64(&mut r), "Hello");
	assert_eq!(r.pos(), 64);
}

#[test]
fn char64_test_read_replace_not_ascii() {
	let mut data = b"Hello \xe4 world".to_vec();
	data.extend([0; 100]);
	let mut r = reader(&data);
	let (logs, value) = capture(|| read_char64(&mut r));
	assert_eq!(value, "Hello \u{FFFD} world");
	assert_eq!(r.pos(), 64);
	assert!(logs.contains(&"replaced non-ASCII characters in \"Hello \u{FFFD} world\"".to_owned()));

	// Each non-ASCII byte is replaced, also in the message, whether or not the
	// bytes are UTF-8.
	let mut data = "Caf\u{e9}".as_bytes().to_vec();
	data.extend([0; 100]);
	let (logs, value) = capture(|| read_char64(&mut reader(&data)));
	assert_eq!(value, "Caf\u{FFFD}\u{FFFD}");
	assert_eq!(
		logs,
		["replaced non-ASCII characters in \"Caf\u{FFFD}\u{FFFD}\""]
	);
}

#[test]
fn char64_test_write() {
	let data = written(|w| {
		write_char64(w, "Hello world");
		Ok(())
	});
	let mut expected = b"Hello world".to_vec();
	expected.extend([0; 53]);
	assert_eq!(data, expected);
}

#[test]
fn char64_test_write_replace_not_ascii() {
	let (logs, data) = capture(|| {
		written(|w| {
			write_char64(w, "Hello \u{2603}");
			Ok(())
		})
	});
	let mut expected = b"Hello ?".to_vec();
	expected.extend([0; 57]);
	assert_eq!(data, expected);
	assert!(logs.contains(&"replacing non-ASCII characters in \"Hello \u{2603}\"".to_owned()));
}

#[test]
fn char64_test_write_truncated() {
	let long = "A".repeat(80);
	let (logs, data) = capture(|| {
		written(|w| {
			write_char64(w, &long);
			Ok(())
		})
	});
	assert_eq!(data, vec![b'A'; 64]);
	assert!(logs.contains(&format!("truncating overlong \"{long}\" to 64 bytes")));
}

#[test]
fn char64_test_xml_read() -> TestResult {
	assert_eq!(value_attr(&element(r#"<Element value="Foo"/>"#))?, "Foo");
	Ok(())
}

#[test]
fn char64_test_xml_write() {
	let lines = xml(|xw| {
		xml_simple(xw, "Element", &[("attr", "v".into())], "Hello world".into());
		Ok(())
	});
	assert_eq!(lines, [r#"<Element attr="v" value="Hello world"/>"#]);
}

// GlyphIDTest

#[test]
fn glyph_id_test_read_array() -> TestResult {
	let mut font = fake_font(".notdef A B C");
	let order = font.glyph_order()?;
	let data = hex("0002 0001 DEAD 0002");
	let mut r = reader(&data);
	let names: Vec<String> = r
		.read_u16_array(4)?
		.into_iter()
		.map(|g| order.name(g as u32).into_owned())
		.collect();
	assert_eq!(names, ["B", "A", "glyph57005", "B"]);
	assert_eq!(r.pos(), 8);
	Ok(())
}

#[test]
fn glyph_id_test_read() -> TestResult {
	let mut font = fake_font(".notdef A B C");
	let data = hex("0003");
	let mut r = reader(&data);
	assert_eq!(font.glyph_name(r.read_u16()? as u32)?, "C");
	assert_eq!(r.pos(), 2);
	Ok(())
}

#[test]
fn glyph_id_test_write() -> TestResult {
	let mut font = fake_font(".notdef A B C");
	let gid = font.glyph_id("B")?;
	assert_eq!(written(|w| write_glyph(w, gid, 2)), hex("0002"));
	Ok(())
}

// LongTest

#[test]
fn long_test_read() -> TestResult {
	let data = hex("FF0000EE");
	let mut r = reader(&data);
	assert_eq!(r.read_i32()?, -16776978);
	assert_eq!(r.pos(), 4);
	Ok(())
}

#[test]
fn long_test_write() {
	let data = written(|w| {
		w.write_i32(-16777213);
		Ok(())
	});
	assert_eq!(data, hex("FF000003"));
}

#[test]
fn long_test_xml_read() -> TestResult {
	let e = element(r#"<Foo value="314159"/>"#);
	assert_eq!(parse_int::<i32>(value_attr(&e)?)?, 314159);
	Ok(())
}

#[test]
fn long_test_xml_write() {
	let lines = xml(|xw| {
		xml_simple(xw, "Foo", &[("attr", "v".into())], 291.to_string());
		Ok(())
	});
	assert_eq!(lines, [r#"<Foo attr="v" value="291"/>"#]);
}

// NameIDTest

fn name_id_font() -> Font {
	let mut name = Name::default();
	name.set_name("Demibold Condensed", 0x123, 3, 0, 0x409);
	name.set_name("Copyright 2018", 0, 3, 0, 0x409);
	let mut font = Font::new();
	font.insert(Box::new(name));
	font
}

#[test]
fn name_id_test_read() -> TestResult {
	let data = hex("0123");
	assert_eq!(reader(&data).read_u16()?, 0x123);
	Ok(())
}

#[test]
fn name_id_test_write() {
	let data = written(|w| {
		w.write_u16(0x123);
		Ok(())
	});
	assert_eq!(data, hex("0123"));
}

#[test]
fn name_id_test_xml_write() {
	let mut font = name_id_font();
	let lines = xml(|xw| xml_name_id(xw, &mut font, "FooNameID", &[("attr", "val".into())], 291));
	assert_eq!(
		lines,
		[r#"<FooNameID attr="val" value="291"/>  <!-- Demibold Condensed -->"#]
	);
}

#[test]
fn name_id_test_xml_write_missing_id() {
	let mut font = name_id_font();
	let (logs, lines) = capture(|| {
		xml(|xw| xml_name_id(xw, &mut font, "Entity", &[("attrib", "val".into())], 666))
	});
	assert!(logs.contains(&"name id 666 missing from name table".to_owned()));
	assert_eq!(
		lines,
		[r#"<Entity attrib="val" value="666"/>  <!-- missing from name table -->"#]
	);
}

#[test]
fn name_id_test_xml_write_null() {
	let mut font = name_id_font();
	let lines = xml(|xw| xml_name_id(xw, &mut font, "FooNameID", &[("attr", "val".into())], 0));
	assert_eq!(lines, [r#"<FooNameID attr="val" value="0"/>"#]);
}

// UInt8Test

#[test]
fn uint8_test_read() -> TestResult {
	let data = hex("FE");
	let mut r = reader(&data);
	assert_eq!(r.read_u8()?, 254);
	assert_eq!(r.pos(), 1);
	Ok(())
}

#[test]
fn uint8_test_write() {
	let data = written(|w| {
		w.write_u8(253);
		Ok(())
	});
	assert_eq!(data, hex("FD"));
}

#[test]
fn uint8_test_xml_read() -> TestResult {
	let e = element(r#"<Foo value="254"/>"#);
	assert_eq!(parse_int::<u8>(value_attr(&e)?)?, 254);
	Ok(())
}

#[test]
fn uint8_test_xml_write() {
	let lines = xml(|xw| {
		xml_simple(xw, "Foo", &[("attr", "v".into())], 251.to_string());
		Ok(())
	});
	assert_eq!(lines, [r#"<Foo attr="v" value="251"/>"#]);
}

// AATLookupTest: an `AATLookup` of `GlyphID` values.

const AAT_GLYPHS: &str = ".notdef A B C D E F G H A.alt B.alt";

/// Reads a lookup and names its glyphs.
fn read_lookup(font: &mut Font, data: &str) -> fontoxide_ttlib::Result<BTreeMap<String, String>> {
	let data = hex(data);
	let lookup: AatLookup<u32> = read_aat_lookup(&mut reader(&data), font)?;
	named(font, &lookup)
}

fn named(
	font: &mut Font,
	lookup: &AatLookup<u32>,
) -> fontoxide_ttlib::Result<BTreeMap<String, String>> {
	let order = font.glyph_order()?;
	Ok(lookup
		.iter()
		.map(|(&g, &v)| (order.name(g).into_owned(), order.name(v).into_owned()))
		.collect())
}

fn mapping(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
	pairs
		.iter()
		.map(|&(a, b)| (a.to_owned(), b.to_owned()))
		.collect()
}

fn lookup_of(font: &mut Font, pairs: &[(&str, &str)]) -> AatLookup<u32> {
	pairs
		.iter()
		.map(|&(a, b)| (font.glyph_id(a).unwrap(), font.glyph_id(b).unwrap()))
		.collect()
}

#[test]
fn aat_lookup_test_read_format0() -> TestResult {
	let mut font = fake_font(AAT_GLYPHS);
	// The data has values for 6 of the 11 glyphs: like fontTools, the array is
	// read as far as it goes.
	assert_eq!(
		read_lookup(&mut font, "0000 0000 0001 0002 0000 7D00 0001")?,
		mapping(&[
			(".notdef", ".notdef"),
			("A", "A"),
			("B", "B"),
			("C", ".notdef"),
			("D", "glyph32000"),
			("E", "A"),
		])
	);
	Ok(())
}

#[test]
fn aat_lookup_test_read_format2() -> TestResult {
	let mut font = fake_font(AAT_GLYPHS);
	assert_eq!(
		read_lookup(
			&mut font,
			"0002 0006 0002 000C 0001 0006 \
			 0002 0001 0003 \
			 0007 0005 0008 \
			 FFFF FFFF FFFF"
		)?,
		mapping(&[("A", "C"), ("B", "C"), ("E", "H"), ("F", "H"), ("G", "H")])
	);
	Ok(())
}

#[test]
fn aat_lookup_test_read_format4() -> TestResult {
	let mut font = fake_font(AAT_GLYPHS);
	assert_eq!(
		read_lookup(
			&mut font,
			"0004 0006 0003 000C 0001 0006 \
			 0002 0001 001E \
			 0005 0004 001E \
			 FFFF FFFF FFFF \
			 0007 0008"
		)?,
		mapping(&[("A", "G"), ("B", "H"), ("D", "G"), ("E", "H")])
	);
	Ok(())
}

#[test]
fn aat_lookup_test_read_format6() -> TestResult {
	let mut font = fake_font(AAT_GLYPHS);
	assert_eq!(
		read_lookup(
			&mut font,
			"0006 0004 0002 0008 0001 0004 \
			 0003 0001 \
			 0005 0002 \
			 FFFF FFFF"
		)?,
		mapping(&[("C", "A"), ("E", "B")])
	);
	Ok(())
}

#[test]
fn aat_lookup_test_read_format8() -> TestResult {
	let mut font = fake_font(AAT_GLYPHS);
	assert_eq!(
		read_lookup(&mut font, "0008 0003 0003 0007 0001 0002")?,
		mapping(&[("C", "G"), ("D", "A"), ("E", "B")])
	);
	Ok(())
}

#[test]
fn aat_lookup_test_read_unknown_format() {
	let mut font = fake_font(AAT_GLYPHS);
	let error = read_lookup(&mut font, "0009").unwrap_err();
	assert!(
		error.to_string().contains("unsupported lookup format: 9"),
		"{error}"
	);
}

fn write_lookup(glyphs: &str, pairs: &[(&str, &str)]) -> Vec<u8> {
	let mut font = fake_font(glyphs);
	let lookup = lookup_of(&mut font, pairs);
	written(|w| write_aat_lookup(w, &mut font, &lookup))
}

#[test]
fn aat_lookup_test_write_format0() {
	assert_eq!(
		write_lookup(
			".notdef A B C",
			&[(".notdef", ".notdef"), ("A", "C"), ("B", "C"), ("C", "A")]
		),
		hex("0000 0000 0003 0003 0001")
	);
}

#[test]
fn aat_lookup_test_write_format2() {
	assert_eq!(
		write_lookup(
			".notdef A B C D E F G H",
			&[
				("B", "C"),
				("C", "C"),
				("D", "C"),
				("E", "C"),
				("G", "A"),
				("H", "A"),
			]
		),
		hex("0002 0006 0002 000C 0001 0000 \
			 0005 0002 0003 \
			 0008 0007 0001 \
			 FFFF FFFF 0000")
	);
}

#[test]
fn aat_lookup_test_write_format6() {
	assert_eq!(
		write_lookup(
			".notdef A B C D E",
			&[("A", "C"), ("C", "B"), ("D", "D"), ("E", "E")]
		),
		hex("0006 0004 0004 0010 0002 0000 \
			 0001 0003 \
			 0003 0002 \
			 0004 0004 \
			 0005 0005 \
			 FFFF 0000")
	);
}

#[test]
fn aat_lookup_test_write_format8() {
	assert_eq!(
		write_lookup(
			".notdef A B C D E F G H",
			&[
				("B", "B"),
				("C", "A"),
				("D", "B"),
				("E", "C"),
				("F", "B"),
				("G", "A"),
			]
		),
		hex("0008 0002 0006 0002 0001 0002 0003 0002 0001")
	);
}

#[test]
fn aat_lookup_test_xml_read() -> TestResult {
	let mut font = fake_font(AAT_GLYPHS);
	let e =
		element(r#"<Foo><Lookup glyph="A" value="A.alt"/><Lookup glyph="B" value="B.alt"/></Foo>"#);
	let lookup: AatLookup<u32> = read_aat_lookup_xml(&e, &mut font, &mut XmlCtx::default())?;
	assert_eq!(
		named(&mut font, &lookup)?,
		mapping(&[("A", "A.alt"), ("B", "B.alt")])
	);
	Ok(())
}

#[test]
fn aat_lookup_test_xml_write() {
	let mut font = fake_font(AAT_GLYPHS);
	let lookup = lookup_of(&mut font, &[("A", "A.alt"), ("B", "B.alt")]);
	let lines = xml(|xw| xml_aat_lookup(xw, &mut font, "Foo", &[("attr", "val".into())], &lookup));
	assert_eq!(
		lines,
		[
			r#"<Foo attr="val">"#,
			r#"  <Lookup glyph="A" value="A.alt"/>"#,
			r#"  <Lookup glyph="B" value="B.alt"/>"#,
			"</Foo>",
		]
	);
}

// CFF2Index

fn read_cff2_index(data: &str) -> fontoxide_ttlib::Result<Vec<Vec<u8>>> {
	let data = hex(data);
	Ok(read_index(&mut reader(&data))?
		.into_iter()
		.map(<[u8]>::to_vec)
		.collect())
}

#[test]
fn test_cff2_index_read() -> TestResult {
	assert_eq!(
		read_cff2_index("00000002 01 01 03 06 AABB CCDDEE")?,
		[vec![0xAA, 0xBB], vec![0xCC, 0xDD, 0xEE]]
	);
	// More than 8 items are read lazily by fontTools; here the offset array ends
	// the data.
	assert_eq!(
		read_cff2_index(&format!("0000000A 01{}", " 01".repeat(11)))?,
		vec![Vec::<u8>::new(); 10]
	);
	Ok(())
}

#[test]
fn test_cff2_index_count_exceeds_data() {
	for data in [
		// count far larger than the data
		"00100000 01 01 01".to_owned(),
		// one offset short
		"00000003 01 01 01 01".to_owned(),
		// one offset short, lazy
		format!("0000000A 01{}", " 01".repeat(10)),
	] {
		let error = read_cff2_index(&data).unwrap_err();
		assert!(
			error.to_string().contains("exceeds available data"),
			"{error}"
		);
	}
}

#[test]
fn test_cff2_index_invalid_off_size() {
	for off_size in ["00", "05"] {
		let error = read_cff2_index(&format!("00000001 {off_size} 01 01 AA")).unwrap_err();
		assert!(error.to_string().contains("offSize"), "{error}");
	}
}

#[test]
fn test_empty_varc_axis_tuples_roundtrip() -> TestResult {
	let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttLib/data/varc-ac00-ac01.ttf");
	// The `optimize_speed` parameter is dropped: there is no `OPTIMIZE_FONT_SPEED`
	// option, values are always packed for size.
	{
		let mut font = Font::read(std::fs::read(&path)?, ReadOptions::default())?;
		let items = &mut font
			.require::<Varc>()?
			.axis_indices_list
			.as_deref_mut()
			.ok_or("no AxisIndicesList")?
			.item;
		// Exceed the lazy INDEX decoding threshold (count > 8).
		items.extend(std::iter::repeat_n(Vec::new(), 9));
		let expected = items.clone();
		let data = font.write(&WriteOptions::default())?;
		let mut reloaded = Font::read(data, ReadOptions::default())?;
		let items = &reloaded
			.require::<Varc>()?
			.axis_indices_list
			.as_deref()
			.ok_or("no AxisIndicesList")?
			.item;
		assert_eq!(*items, expected);
	}
	Ok(())
}
