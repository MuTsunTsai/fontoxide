//! Tables holding a JSON document: `Debg` (`D__e_b_g`) and `bgcl` (`_b_g_c_l`).

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::table::{DefaultTable, Table, TableType};
use crate::tables::json::{self, DumpOptions, JsonValue};
use crate::{Error, Font, Result};

/// The JSON text of a `<json>` element: its text chunks joined.
fn element_text(element: &Element) -> String {
	element.text()
}

/// Debug information table (`table_D__e_b_g`), as written by feaLib.
#[derive(Debug, Clone, PartialEq)]
pub struct Debg {
	/// The decoded JSON document (`data`).
	pub data: JsonValue,
}

impl Default for Debg {
	fn default() -> Self {
		Debg {
			data: JsonValue::Object(Vec::new()),
		}
	}
}

impl TableType for Debg {
	const TAG: Tag = Tag::new(b"Debg");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let data = json::loads_bytes(data).map_err(|e| Error::value(e.repr))?;
		Ok(Debg { data })
	}
}

impl Table for Debg {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		Ok(json::dumps(&self.data, &DumpOptions::default()).into_bytes())
	}

	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		// The JSON indentation inside the CDATA block matches the writer's.
		let indent = " ".repeat(writer.indent_white().chars().count());
		let data = json::dumps(&self.data, &DumpOptions::indented(&indent));
		let prefix = writer.indent_white().repeat(writer.indent_level() + 1);
		// textwrap.indent, except for a first line "{\n", which stays adjacent to
		// "<![CDATA[{".
		let mut cdata = String::new();
		for line in data.split_inclusive('\n') {
			if line != "{\n" {
				cdata.push_str(&prefix);
			}
			cdata.push_str(line);
		}
		writer.begintag("json", &[]);
		writer.newline();
		writer.writecdata(&cdata);
		writer.newline();
		writer.endtag("json");
		writer.newline();
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		if element.name == "json" {
			self.data = json::loads(&element_text(element)).map_err(|e| Error::value(e.repr))?;
		}
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Ok(())
	}
}

/// Apple Color Emoji background table (`table__b_g_c_l`): a JSON document.
///
/// Data that is not valid JSON is kept as is, with the error (`ERROR`), and is
/// dumped as hex.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bgcl {
	/// The raw data (`data`).
	pub data: Vec<u8>,
	/// The decoded JSON document (`json`); `None` if decoding failed, in which
	/// case `data` is compiled as is.
	pub json: Option<JsonValue>,
	/// The decoding error (`ERROR`).
	pub error: Option<String>,
}

impl TableType for Bgcl {
	const TAG: Tag = Tag::new(b"bgcl");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let mut table = Bgcl {
			data: data.to_vec(),
			json: None,
			error: None,
		};
		let parsed = match std::str::from_utf8(data) {
			Ok(text) => json::loads(text).map_err(|e| e.repr),
			Err(e) => Err(utf8_error_repr(data, &e)),
		};
		match parsed {
			Ok(value) => {
				// Setting the convenience attributes is outside the try block.
				if let Some(msg) = get_error(&value) {
					return Err(Error::ttlib(format!("AttributeError: {msg}")));
				}
				table.json = Some(value);
			}
			Err(repr) => table.error = Some(format!("bgcl JSON parse error: {repr}")),
		}
		Ok(table)
	}
}

/// `repr()` of the `UnicodeDecodeError` of `bytes.decode("utf_8")`.
fn utf8_error_repr(data: &[u8], e: &std::str::Utf8Error) -> String {
	let start = e.valid_up_to();
	let (end, reason) = match e.error_len() {
		Some(len) => {
			let first = data[start];
			let reason = if (0x80..0xC2).contains(&first) || first > 0xF4 {
				"invalid start byte"
			} else {
				"invalid continuation byte"
			};
			(start + len, reason)
		}
		None => (data.len(), "unexpected end of data"),
	};
	format!(
		"UnicodeDecodeError('utf-8', {}, {start}, {end}, '{reason}')",
		fontoxide_misc::py::bytes_repr(data)
	)
}

/// The `AttributeError` of `value.get(...)` for a document that is not an
/// object, which fontTools raises when setting the convenience attributes.
fn get_error(value: &JsonValue) -> Option<String> {
	let type_name = match value {
		JsonValue::Object(_) => return None,
		JsonValue::Null => "NoneType",
		JsonValue::Bool(_) => "bool",
		JsonValue::Int(_) => "int",
		JsonValue::Float(_) => "float",
		JsonValue::Str(_) => "str",
		JsonValue::Array(_) => "list",
	};
	Some(format!("'{type_name}' object has no attribute 'get'"))
}

impl Table for Bgcl {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		match &self.json {
			None => Ok(self.data.clone()),
			Some(value) => {
				let options = DumpOptions {
					indent: None,
					separators: (",", ":"),
					ensure_ascii: false,
				};
				Ok(json::dumps(value, &options).into_bytes())
			}
		}
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let Some(value) = &self.json else {
			let raw = DefaultTable {
				tag: Self::TAG,
				data: self.data.clone(),
				error: self.error.clone(),
			};
			return raw.to_xml(writer, font);
		};
		writer.begintag("json", &[]);
		writer.newline();
		let options = DumpOptions {
			indent: Some("  "),
			separators: (",", ": "),
			ensure_ascii: false,
		};
		writer.writecdata(&json::dumps(value, &options));
		writer.newline();
		writer.endtag("json");
		writer.newline();
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		if element.name != "json" {
			// DefaultTable.fromXML: decompiles the hex data.
			let mut raw = DefaultTable::new(Self::TAG, Vec::new());
			raw.from_xml(element, font)?;
			let decompiled = Bgcl::decompile(&raw.data, font)?;
			self.data = decompiled.data;
			self.json = decompiled.json;
			// A previous error stays set, as the attribute does in fontTools.
			if decompiled.error.is_some() {
				self.error = decompiled.error;
			}
			return Ok(());
		}
		let text = element_text(element);
		let text = fontoxide_misc::py::strip(&text);
		match json::loads(text) {
			Ok(value) => {
				self.data = text.as_bytes().to_vec();
				match get_error(&value) {
					None => self.json = Some(value),
					Some(msg) => {
						self.json = None;
						self.error = Some(format!(
							"bgcl JSON parse error in fromXML: AttributeError({})",
							fontoxide_misc::py::str_repr(&msg)
						));
					}
				}
			}
			Err(e) => {
				self.json = None;
				self.error = Some(format!("bgcl JSON parse error in fromXML: {}", e.repr));
			}
		}
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn decompile_error(&self) -> Option<&str> {
		self.error.as_deref()
	}

	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let _ = f;
		Ok(())
	}
}
