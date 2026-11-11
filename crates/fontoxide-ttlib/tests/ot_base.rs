//! `Tests/ttLib/tables/otBase_test.py`.
//!
//! `BaseTableTest::test_ensureDecompiled_lazy_array` is not ported: nothing is
//! read lazily here.

#![allow(clippy::unwrap_used)]

use fontoxide_misc::text_tools::de_hex_str;
use fontoxide_ttlib::Tag;
use fontoxide_ttlib::ot::{OtReader, WriterGraph};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn hex(s: &str) -> Vec<u8> {
	de_hex_str(s).unwrap()
}

fn reader(data: &[u8]) -> OtReader<'_> {
	OtReader::new(data, Tag::new(b"test"))
}

// OTTableReaderTest

#[test]
fn test_read_short() -> TestResult {
	let data = hex("CA FE");
	let mut r = reader(&data);
	assert_eq!(r.read_i16()?, -13570);
	assert_eq!(r.pos(), 2);
	Ok(())
}

#[test]
fn test_read_long() -> TestResult {
	let data = hex("CA FE BE EF");
	let mut r = reader(&data);
	assert_eq!(r.read_i32()?, -889274641);
	assert_eq!(r.pos(), 4);
	Ok(())
}

#[test]
fn test_read_uint8() -> TestResult {
	let data = hex("C3");
	let mut r = reader(&data);
	assert_eq!(r.read_u8()?, 0xC3);
	assert_eq!(r.pos(), 1);
	Ok(())
}

#[test]
fn test_read_ushort() -> TestResult {
	let data = hex("CA FE");
	let mut r = reader(&data);
	assert_eq!(r.read_u16()?, 0xCAFE);
	assert_eq!(r.pos(), 2);
	Ok(())
}

#[test]
fn test_read_ushort_array() -> TestResult {
	let data = hex("DE AD BE EF CA FE");
	let mut r = reader(&data);
	assert_eq!(r.read_u16_array(3)?, [0xDEAD, 0xBEEF, 0xCAFE]);
	assert_eq!(r.pos(), 6);
	Ok(())
}

#[test]
fn test_read_int24() -> TestResult {
	// Positive
	let data = hex("3C 13 37");
	let mut r = reader(&data);
	assert_eq!(r.read_i24()?, 3937079);
	assert_eq!(r.pos(), 3);
	// Negative
	let data = hex("C3 13 37");
	let mut r = reader(&data);
	assert_eq!(r.read_i24()?, -3992777);
	assert_eq!(r.pos(), 3);
	Ok(())
}

#[test]
fn test_read_uint24() -> TestResult {
	let data = hex("C3 13 37");
	let mut r = reader(&data);
	assert_eq!(r.read_u24()?, 0xC31337);
	assert_eq!(r.pos(), 3);
	Ok(())
}

#[test]
fn test_read_ulong() -> TestResult {
	let data = hex("CA FE BE EF");
	let mut r = reader(&data);
	assert_eq!(r.read_u32()?, 0xCAFEBEEF);
	assert_eq!(r.pos(), 4);
	Ok(())
}

#[test]
fn test_read_tag() -> TestResult {
	let data = hex("46 6F 6F 64");
	let mut r = reader(&data);
	assert_eq!(r.read_tag()?, Tag::new(b"Food"));
	assert_eq!(r.pos(), 4);
	Ok(())
}

#[test]
fn test_read_data() {
	let data = hex("48 65 6C 6C 6F");
	let mut r = reader(&data);
	assert_eq!(r.read_data(5), b"Hello");
	assert_eq!(r.pos(), 5);
}

#[test]
fn test_get_sub_reader() -> TestResult {
	let data = hex("CAFE F00D");
	let mut r = reader(&data);
	let mut sub = r.sub_reader(2);
	assert_eq!(sub.read_u16()?, 0xF00D);
	assert_eq!(r.read_u16()?, 0xCAFE);
	Ok(())
}

// OTTableWriterTest

/// The data of a writer that `f` writes into (`writer.getData()`).
fn written(
	f: impl FnOnce(&mut fontoxide_ttlib::ot::OtWriter<'_>) -> fontoxide_ttlib::Result<()>,
) -> Vec<u8> {
	let mut graph = WriterGraph::new(Tag::new(b"test"));
	f(&mut graph.root()).unwrap();
	graph.get_all_data().unwrap()
}

#[test]
fn test_write_short() {
	let data = written(|w| {
		w.write_i16(-12345);
		Ok(())
	});
	assert_eq!(data, hex("CF C7"));
}

#[test]
fn test_write_long() {
	let data = written(|w| {
		w.write_i32(-12345678);
		Ok(())
	});
	assert_eq!(data, hex("FF 43 9E B2"));
}

#[test]
fn test_write_uint8() {
	let data = written(|w| {
		w.write_u8(0xBE);
		Ok(())
	});
	assert_eq!(data, hex("BE"));
}

#[test]
fn test_write_ushort() {
	let data = written(|w| {
		w.write_u16(0xBEEF);
		Ok(())
	});
	assert_eq!(data, hex("BE EF"));
}

#[test]
fn test_write_uint24() {
	let data = written(|w| w.write_u24(0xBEEF77));
	assert_eq!(data, hex("BE EF 77"));
}

#[test]
fn test_write_ulong() {
	let data = written(|w| {
		w.write_u32(0xBEEFCAFE);
		Ok(())
	});
	assert_eq!(data, hex("BE EF CA FE"));
}
