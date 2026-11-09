//! The interface of generated OpenType Layout types (`BaseTable`) and the helpers
//! that the generated code calls for each kind of converter.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::{AttrValue, XmlWriter};

use super::{OtReader, OtWriter};
use crate::{Error, Font, Result};

/// XML attributes as fontTools passes them along (`attrs` lists).
pub type Attrs = Vec<(&'static str, String)>;

/// State while reading TTX (`font._propagator`): counts that children report to
/// an enclosing table, such as `Class2Count`.
#[derive(Debug, Default)]
pub struct XmlCtx {
	propagator: HashMap<&'static str, Rc<Cell<Option<u32>>>>,
}

impl XmlCtx {
	/// Registers a propagated count; returns its cell.
	pub fn push(&mut self, name: &'static str) -> Result<Rc<Cell<Option<u32>>>> {
		if self.propagator.contains_key(name) {
			return Err(Error::Assertion(format!("{name} already propagated")));
		}
		let cell = Rc::new(Cell::new(None));
		self.propagator.insert(name, cell.clone());
		Ok(cell)
	}

	/// Unregisters a propagated count.
	pub fn pop(&mut self, name: &str) {
		self.propagator.remove(name);
	}

	/// Reports a count to the enclosing table, if it is waiting for it
	/// (`propagator[name].setValue(value)`).
	pub fn set_count(&mut self, name: &str, value: u32) -> Result<()> {
		if let Some(cell) = self.propagator.get(name) {
			match cell.get() {
				None => cell.set(Some(value)),
				Some(v) if v == value => {}
				Some(v) => return Err(Error::Assertion(format!("({name:?}, {v}, {value})"))),
			}
		}
		Ok(())
	}
}

/// A generated OpenType Layout type (`BaseTable` and subclasses).
pub trait OtTable: Sized + Default + Clone {
	/// The fontTools class name, used as the TTX element name when there is no
	/// field name (subtables of lookups).
	fn class_name(&self) -> &'static str;

	/// Reads the table (`decompile`).
	fn decompile(r: &mut OtReader<'_>, font: &mut Font) -> Result<Self>;

	/// Writes the table (`compile`).
	fn compile(&self, w: &mut OtWriter<'_>, font: &mut Font) -> Result<()>;

	/// The `Format` attribute for TTX, for format-switching tables.
	fn format(&self) -> Option<u16> {
		None
	}

	/// Writes the TTX element (`toXML`).
	fn to_xml(
		&self,
		xw: &mut XmlWriter,
		font: &mut Font,
		attrs: &[(&'static str, String)],
		name: &str,
	) -> Result<()> {
		let mut attrs: Attrs = attrs.to_vec();
		if let Some(f) = self.format() {
			attrs.push(("Format", f.to_string()));
		}
		begintag(xw, name, &attrs);
		xw.newline();
		self.to_xml2(xw, font)?;
		xw.endtag(name);
		xw.newline();
		Ok(())
	}

	/// Writes the TTX content (`toXML2`).
	fn to_xml2(&self, xw: &mut XmlWriter, font: &mut Font) -> Result<()>;

	/// Reads one child element (`fromXML`).
	#[allow(clippy::wrong_self_convention)]
	fn from_xml(&mut self, element: &Element, font: &mut Font, ctx: &mut XmlCtx) -> Result<()>;

	/// Fills in counts and defaults after reading TTX (`populateDefaults`).
	fn populate_defaults(&mut self, ctx: &mut XmlCtx) -> Result<()>;

	/// Maps every glyph ID in the table (and its subtables) with `f`, as when the
	/// glyph order changes (fontTools refers to glyphs by name, so this happens
	/// implicitly there). `f` fails for glyphs that no longer exist.
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()>;

	/// Calls `f` on the table, then on its subtables, depth-first, as
	/// fontTools' visitors do (`_visit` of `varLib.varStore`). Like those, it
	/// does not look into the types that fontTools reads by hand (`postRead`):
	/// for them, it does nothing.
	fn visit_mut(&mut self, f: &mut dyn FnMut(&mut dyn std::any::Any) -> Result<()>) -> Result<()> {
		let _ = f;
		Ok(())
	}

	/// Whether the subtables below are never shared (`DontShare` on an
	/// instance); only lookup subtables can be marked.
	fn dont_share(&self) -> bool {
		false
	}

	/// Marks the subtables below as never shared, if this is a lookup subtable.
	fn set_dont_share(&mut self) {}

	/// Where its `StructLength` field is, for `StructWithLength`.
	fn struct_length(&self) -> Option<super::aat::StructLength> {
		None
	}

	/// The counts this table receives from its children while reading TTX.
	fn propagated_counts(&self) -> &'static [&'static str] {
		&[]
	}

	/// Stores a count received from children (see [`OtTable::propagated_counts`]).
	fn set_propagated_count(&mut self, _name: &str, _value: Option<u32>) {}

	/// Creates the table for TTX `Format` attribute `format`.
	fn with_format(_format: u16) -> Self {
		Self::default()
	}

	/// Reads a table element (`Struct.xmlRead`).
	fn xml_read(element: &Element, font: &mut Font, ctx: &mut XmlCtx) -> Result<Self> {
		let mut table = match element.attr("Format") {
			Some(f) => Self::with_format(parse_int::<u16>(f)?),
			None => Self::default(),
		};
		let names = table.propagated_counts();
		let mut cells = Vec::with_capacity(names.len());
		for &n in names {
			cells.push((n, ctx.push(n)?));
		}
		for child in element.elements() {
			table.from_xml(child, font, ctx)?;
		}
		for (n, cell) in &cells {
			table.set_propagated_count(n, cell.get());
		}
		let result = table.populate_defaults(ctx);
		for (n, _) in &cells {
			ctx.pop(n);
		}
		result?;
		Ok(table)
	}
}

pub(crate) fn begintag(xw: &mut XmlWriter, name: &str, attrs: &[(&'static str, String)]) {
	let refs: Vec<(&str, &dyn AttrValue)> = attrs
		.iter()
		.map(|(n, v)| (*n, v as &dyn AttrValue))
		.collect();
	xw.begintag_ordered(name, &refs);
}

/// `simpletag(name, attrs)` with attributes in the given order.
pub fn simpletag(xw: &mut XmlWriter, name: &str, attrs: &[(&'static str, String)]) {
	let refs: Vec<(&str, &dyn AttrValue)> = attrs
		.iter()
		.map(|(n, v)| (*n, v as &dyn AttrValue))
		.collect();
	xw.simpletag_ordered(name, &refs);
}

/// `SimpleValue.xmlWrite`: `<name ... value="..."/>`.
pub fn xml_simple(xw: &mut XmlWriter, name: &str, attrs: &[(&'static str, String)], value: String) {
	let mut attrs = attrs.to_vec();
	attrs.push(("value", value));
	simpletag(xw, name, &attrs);
	xw.newline();
}

/// `ComputedInt.xmlWrite`: a comment with the count, if known.
pub fn xml_computed(xw: &mut XmlWriter, name: &str, value: Option<u32>) {
	if let Some(v) = value {
		xw.comment(&format!("{name}={v}"));
		xw.newline();
	}
}

/// `Struct.xmlWrite` / `Table.xmlWrite`: the table element, or for a missing
/// table, an `empty` marker if there are attributes (array items).
pub fn xml_struct<T: OtTable>(
	xw: &mut XmlWriter,
	font: &mut Font,
	value: Option<&T>,
	name: Option<&str>,
	attrs: &[(&'static str, String)],
) -> Result<()> {
	match value {
		None => {
			if !attrs.is_empty() {
				let mut attrs = attrs.to_vec();
				attrs.push(("empty", "1".into()));
				simpletag(xw, name.unwrap_or_default(), &attrs);
				xw.newline();
			}
			Ok(())
		}
		Some(v) => {
			let name = name.unwrap_or(v.class_name());
			v.to_xml(xw, font, attrs, name)
		}
	}
}

/// `Struct.xmlRead`, with the `empty` marker for a missing table.
pub fn xml_read_struct<T: OtTable>(
	element: &Element,
	font: &mut Font,
	ctx: &mut XmlCtx,
) -> Result<Option<T>> {
	if element
		.attr("empty")
		.and_then(fontoxide_misc::text_tools::safe_eval)
		.is_some_and(|v| v.is_truthy())
	{
		return Ok(None);
	}
	T::xml_read(element, font, ctx).map(Some)
}

/// Reads an offset and the table it points to (`Table.read`); `None` for a null
/// offset.
pub fn read_offset<T: OtTable>(
	r: &mut OtReader<'_>,
	font: &mut Font,
	size: usize,
) -> Result<Option<Box<T>>> {
	let offset = match size {
		2 => r.read_u16()? as usize,
		3 => r.read_u24()? as usize,
		_ => r.read_u32()? as usize,
	};
	if offset == 0 {
		return Ok(None);
	}
	let mut sub = r.sub_reader(offset);
	Ok(Some(Box::new(T::decompile(&mut sub, font)?)))
}

/// Writes an offset and the table it points to (`Table.write`).
pub fn write_offset<T: OtTable>(
	w: &mut OtWriter<'_>,
	font: &mut Font,
	value: Option<&T>,
	size: usize,
	name: &'static str,
	repeat_index: Option<usize>,
) -> Result<()> {
	match value {
		None => {
			match size {
				2 => w.write_u16(0),
				3 => w.write_u24(0)?,
				_ => w.write_u32(0),
			}
			Ok(())
		}
		Some(v) => {
			let mut sub = w.sub_table(name, repeat_index, size);
			v.compile(&mut sub, font)
		}
	}
}

/// The `value` attribute of an element.
pub fn value_attr(element: &Element) -> Result<&str> {
	element
		.attr("value")
		.ok_or_else(|| Error::ttx(format!("missing 'value' on <{}>", element.name)))
}

/// `int(s, 0)` converted to the field type.
pub fn parse_int<T: TryFrom<i128>>(s: &str) -> Result<T> {
	let v = fontoxide_misc::text_tools::parse_int_literal(s)
		.ok_or_else(|| Error::value(format!("invalid literal for int() with base 0: {s:?}")))?;
	T::try_from(v).map_err(|_| Error::Struct(format!("value {v} out of range")))
}

/// `float(s)`.
pub fn parse_float(s: &str) -> Result<f64> {
	fontoxide_misc::py::parse_float(s)
		.ok_or_else(|| Error::value(format!("could not convert string to float: {s:?}")))
}

/// Converts a count to the field type, failing like `struct.pack`.
pub fn fit<T: TryFrom<u64>>(v: u64, name: &str) -> Result<T> {
	T::try_from(v).map_err(|_| Error::Struct(format!("count {v} does not fit for {name}")))
}

/// A Python list repr of integers, e.g. `[1, -2]` (`DeltaValue` in TTX).
pub fn int_list_repr(values: &[i64]) -> String {
	let items: Vec<String> = values.iter().map(i64::to_string).collect();
	format!("[{}]", items.join(", "))
}

/// The number of items, as a Python `range(count + aux)` would produce.
pub fn repeat_count(count: Option<i64>, aux: i64, name: &str) -> Result<usize> {
	let count = count.ok_or_else(|| Error::ttlib(format!("KeyError: {name:?}")))?;
	Ok((count + aux).max(0) as usize)
}
