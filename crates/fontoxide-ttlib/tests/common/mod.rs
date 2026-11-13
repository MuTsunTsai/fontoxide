//! Helpers shared by the ports of fontTools' table tests (the equivalents of
//! `fontTools.misc.testTools`).

#![allow(dead_code)]

use std::path::PathBuf;

use fontoxide_misc::xml_reader::{self, Element};
use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_ttlib::table::{table_from_xml, table_to_xml};
use fontoxide_ttlib::{Font, Result, Table};

pub type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

/// A path under `upstream/fonttools/Tests`.
pub fn upstream(path: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests")
		.join(path)
}

/// A path under `upstream/fonttools/Tests/ttLib/tables/data`.
pub fn tables_data(path: &str) -> PathBuf {
	upstream("ttLib/tables/data").join(path)
}

/// `deHexStr`: hex digits, whitespace ignored.
pub fn hex(data: &str) -> Vec<u8> {
	fontoxide_misc::text_tools::de_hex_str(data).expect("valid hex")
}

/// `hexStr`-like formatting, for readable assertion failures.
pub fn hex_str(data: &[u8]) -> String {
	data.iter().map(|b| format!("{b:02x}")).collect()
}

/// Owned strings.
pub fn names(list: &[&str]) -> Vec<String> {
	list.iter().map(|s| (*s).to_owned()).collect()
}

/// `FakeFont(glyphs)`: a font with only a glyph order.
pub fn fake_font(glyphs: &[&str]) -> Font {
	let mut font = Font::new();
	font.set_glyph_order(names(glyphs));
	font
}

/// The lines of a writer's output without the XML declaration.
pub fn writer_lines(writer: XmlWriter) -> Vec<String> {
	let text = writer.into_string();
	let body = text.split_once('\n').map_or(text.as_str(), |(_, b)| b);
	body.lines().map(str::to_owned).collect()
}

/// `getXML(table.toXML, font)`: the lines of the table's TTX content.
pub fn get_xml(table: &dyn Table, font: &mut Font) -> Result<Vec<String>> {
	let mut writer = XmlWriter::with_options("  ", "\n");
	table_to_xml(table, &mut writer, font)?;
	Ok(writer_lines(writer))
}

/// `parseXML(xml)`: the top-level elements of an XML fragment.
pub fn parse_xml(xml: &str) -> Vec<Element> {
	let root = xml_reader::parse(&format!("<root>{xml}</root>")).expect("valid XML");
	root.elements().cloned().collect()
}

/// `for name, attrs, content in parseXML(xml): table.fromXML(name, attrs, content, font)`.
pub fn from_xml(table: &mut dyn Table, xml: &str, font: &mut Font) -> Result<()> {
	for element in parse_xml(xml) {
		table_from_xml(table, &element, font)?;
	}
	Ok(())
}

thread_local! {
	static CAPTURED: std::cell::RefCell<Option<Vec<(log::Level, String)>>> =
		const { std::cell::RefCell::new(None) };
}

struct Capture;

impl log::Log for Capture {
	fn enabled(&self, _: &log::Metadata<'_>) -> bool {
		true
	}

	fn log(&self, record: &log::Record<'_>) {
		CAPTURED.with(|c| {
			if let Some(records) = c.borrow_mut().as_mut() {
				records.push((record.level(), record.args().to_string()));
			}
		});
	}

	fn flush(&self) {}
}

static CAPTURE: Capture = Capture;
static INIT: std::sync::Once = std::sync::Once::new();

/// The records `f` logs on this thread at level INFO and above
/// (`CapturingLogHandler`), with the result of `f`.
pub fn capture_logs<R>(f: impl FnOnce() -> R) -> (Vec<(log::Level, String)>, R) {
	INIT.call_once(|| {
		let _ = log::set_logger(&CAPTURE);
		log::set_max_level(log::LevelFilter::Info);
	});
	CAPTURED.with(|c| *c.borrow_mut() = Some(Vec::new()));
	let result = f();
	let records = CAPTURED.with(|c| c.borrow_mut().take()).unwrap_or_default();
	(records, result)
}

/// How many captured records at `level` or more severe contain `text`.
pub fn count_logs(records: &[(log::Level, String)], level: log::Level, text: &str) -> usize {
	records
		.iter()
		.filter(|(l, m)| *l <= level && m.contains(text))
		.count()
}

/// Lines of TTX text with surrounding whitespace stripped and blank lines
/// removed (`stripped` in several upstream tests).
pub fn strip_lines(text: &str) -> Vec<String> {
	text.lines()
		.map(str::trim)
		.filter(|l| !l.is_empty())
		.map(str::to_owned)
		.collect()
}

/// `table = newTable(tag); table.decompile(data, FakeFont(glyphs));
/// getXML(table.toXML)`.
pub fn decompile_to_xml(
	tag: fontoxide_ttlib::Tag,
	glyphs: &[&str],
	data: &[u8],
) -> Result<Vec<String>> {
	let mut font = fake_font(glyphs);
	let table = fontoxide_ttlib::table::decompile_table(tag, data, &mut font)?;
	get_xml(table.as_ref(), &mut font)
}

/// `table = newTable(tag); for ... in parseXML(xml): table.fromXML(...);
/// table.compile(FakeFont(glyphs))`. As in `parseXML`, the lines are joined
/// without separators.
pub fn compile_from_xml(
	tag: fontoxide_ttlib::Tag,
	glyphs: &[&str],
	xml: &[&str],
) -> Result<Vec<u8>> {
	let mut font = fake_font(glyphs);
	let mut table = fontoxide_ttlib::table::new_table(tag);
	from_xml(table.as_mut(), &xml.concat(), &mut font)?;
	table.compile(&mut font)
}
