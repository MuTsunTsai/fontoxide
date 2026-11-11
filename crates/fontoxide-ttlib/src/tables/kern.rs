//! `kern`: Kerning table (`fontTools.ttLib.tables._k_e_r_n`).

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::fixed_tools::{fixed_to_float, float_to_fixed};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes, fit, get_search_range};
use crate::sstruct::{Value, eval};
use crate::table::{Table, TableType, read_hex};
use crate::tables::eblc::{clip, required_attr};
use crate::{Error, Font, Result};

/// A format 0 subtable (`KernTable_format_0`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KernFormat0 {
	/// Whether it has the header of Apple's `kern` table (`apple`).
	pub apple: bool,
	/// The coverage bits (`coverage`).
	pub coverage: u8,
	/// The tuple index of Apple's header (`tupleIndex`).
	pub tuple_index: Option<u16>,
	/// The kerning values by left and right glyph ID (`kernTable`, by glyph name
	/// in fontTools).
	pub kern_table: BTreeMap<(u32, u32), i16>,
}

/// A subtable of `kern`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernSubtable {
	/// `KernTable_format_0`.
	Format0(KernFormat0),
	/// `KernTable_format_unkown`: kept as data, including the header.
	Unknown {
		/// The format.
		format: i64,
		/// The data.
		data: Vec<u8>,
	},
}

impl KernSubtable {
	/// The subtable format (`format`).
	pub fn format(&self) -> i64 {
		match self {
			KernSubtable::Format0(_) => 0,
			KernSubtable::Unknown { format, .. } => *format,
		}
	}
}

impl Kern {
	/// The first subtable with the given format (`getkern`).
	pub fn get_kern(&self, format: i64) -> Option<&KernSubtable> {
		self.kern_tables
			.iter()
			.flatten()
			.find(|t| t.format() == format)
	}
}

fn is_one(version: &Value) -> bool {
	match version {
		Value::Int(i) => *i == 1,
		Value::Float(f) => *f == 1.0,
		Value::Bytes(_) => false,
	}
}

/// A reader over exactly the first `n` bytes, as `struct.unpack` of `data[:n]`
/// requires.
fn header(data: &[u8], n: u64) -> Result<Reader<'_>> {
	let slice = clip(data, 0, n);
	if (slice.len() as u64) < n {
		return Err(Error::Struct(format!(
			"unpack requires a buffer of {n} bytes"
		)));
	}
	Ok(Reader::new(slice))
}

impl KernFormat0 {
	fn decompile(apple: bool, data: &[u8], font: &mut Font) -> Result<Self> {
		let mut table = KernFormat0 {
			apple,
			..Default::default()
		};
		let (subtable_format, rest) = if apple {
			let mut r = header(data, 8)?;
			r.u32()?;
			table.coverage = r.u8()?;
			let format = r.u8()?;
			table.tuple_index = Some(r.u16()?);
			(format, clip(data, 8, u64::MAX))
		} else {
			let mut r = header(data, 6)?;
			let version = r.u16()?;
			r.u16()?;
			let format = r.u8()?;
			table.coverage = r.u8()?;
			if version != 0 {
				return Err(Error::ttlib(format!(
					"unsupported kern subtable version: {version}"
				)));
			}
			(format, clip(data, 6, u64::MAX))
		};
		if subtable_format != 0 {
			return Err(Error::Assertion("unsupported format".into()));
		}
		let num_pairs = usize::from(header(rest, 8)?.u16()?);
		let rest = clip(rest, 8, u64::MAX);
		font.glyph_order()?;
		let mut r = Reader::new(rest);
		for _ in 0..num_pairs {
			let left = u32::from(r.u16()?);
			let right = u32::from(r.u16()?);
			table.kern_table.insert((left, right), r.i16()?);
		}
		if rest.len() > 6 * num_pairs + 4 {
			log::warn!(
				"excess data in 'kern' subtable: {} bytes",
				rest.len() - 6 * num_pairs
			);
		}
		Ok(table)
	}

	fn compile(&mut self, out: &mut Vec<u8>) -> Result<()> {
		let mut data = Vec::with_capacity(8 + 6 * self.kern_table.len());
		let num_pairs = self.kern_table.len().min(0xFFFF);
		let (search_range, entry_selector, range_shift) = get_search_range(num_pairs as u32, 6);
		data.put_u16(num_pairs as u16);
		data.put_u16((search_range & 0xFFFF) as u16);
		data.put_u16(entry_selector.min(0xFFFF) as u16);
		data.put_u16(range_shift.min(0xFFFF) as u16);
		// All pairs are written, even if there are too many to count.
		for (&(left, right), &value) in &self.kern_table {
			data.put_u16(fit(left, "left")?);
			data.put_u16(fit(right, "right")?);
			data.put_i16(value);
		}
		if self.apple {
			if self.tuple_index.is_none() {
				log::warn!("'tupleIndex' is None; default to 0");
				self.tuple_index = Some(0);
			}
			out.put_u32(fit(data.len() + 8, "length")?);
			out.put_u8(self.coverage);
			out.put_u8(0);
			out.put_u16(self.tuple_index.unwrap_or_default());
		} else {
			let mut length = data.len() + 6;
			if length >= 0x10000 {
				log::warn!(
					"\"kern\" subtable overflow, truncating length value while preserving pairs."
				);
				length &= 0xFFFF;
			}
			out.put_u16(0);
			out.put_u16(length as u16);
			out.put_u8(0);
			out.put_u8(self.coverage);
		}
		out.extend(data);
		Ok(())
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let tuple_index = self.tuple_index.unwrap_or_else(|| {
			if self.apple {
				log::warn!("'tupleIndex' is None; default to 0");
			}
			0
		});
		if self.apple {
			writer.begintag(
				"kernsubtable",
				&[
					("coverage", &self.coverage),
					("format", &0),
					("tupleIndex", &tuple_index),
				],
			);
		} else {
			writer.begintag(
				"kernsubtable",
				&[("coverage", &self.coverage), ("format", &0)],
			);
		}
		writer.newline();
		let order = font.glyph_order()?;
		let mut items: Vec<(String, String, i16)> = self
			.kern_table
			.iter()
			.map(|(&(l, r), &v)| (order.name(l).into_owned(), order.name(r).into_owned(), v))
			.collect();
		items.sort();
		for (left, right, value) in &items {
			writer.simpletag_ordered("pair", &[("l", left), ("r", right), ("v", value)]);
			writer.newline();
		}
		writer.endtag("kernsubtable");
		writer.newline();
		Ok(())
	}

	fn from_xml(apple: bool, element: &Element, font: &mut Font) -> Result<Self> {
		let mut table = KernFormat0 {
			apple,
			coverage: crate::tables::eblc::int_attr(element, "coverage")?,
			..Default::default()
		};
		if apple {
			if element.attr("tupleIndex").is_some() {
				table.tuple_index = Some(crate::tables::eblc::int_attr(element, "tupleIndex")?);
			} else {
				log::warn!("Apple kern subtable is missing 'tupleIndex' attribute");
			}
		}
		for pair in element.elements() {
			let left = font.glyph_id(required_attr(pair, "l")?)?;
			let right = font.glyph_id(required_attr(pair, "r")?)?;
			let value = crate::tables::eblc::int_attr(pair, "v")?;
			table.kern_table.insert((left, right), value);
		}
		Ok(table)
	}
}

/// Kerning table (`table__k_e_r_n`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/kern>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Kern {
	/// The version (`version`): an integer, or for Apple's header a float. `None`
	/// until set, which fails when the table is compiled.
	pub version: Option<Value>,
	/// The subtables (`kernTables`), if set.
	pub kern_tables: Option<Vec<KernSubtable>>,
}

impl TableType for Kern {
	const TAG: Tag = Tag::new(b"kern");

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		let mut r = header(data, 4)?;
		let version = r.u16()?;
		let num_tables = r.u16()?;
		let (version, num_tables, apple, mut rest) = if data.len() >= 8 && version == 1 {
			let mut r = header(data, 8)?;
			let version = r.u32()?;
			let num_tables = r.u32()?;
			(
				Value::Float(fixed_to_float(i64::from(version), 16)),
				num_tables,
				true,
				clip(data, 8, u64::MAX),
			)
		} else {
			(
				Value::Int(i64::from(version)),
				u32::from(num_tables),
				false,
				clip(data, 4, u64::MAX),
			)
		};
		let mut tables = Vec::new();
		for _ in 0..num_tables {
			let (length, format) = if is_one(&version) {
				let mut r = header(rest, 6)?;
				let length = u64::from(r.u32()?);
				r.u8()?;
				(length, r.u8()?)
			} else {
				// The "version" of the subtable header is always 0, and ignored here.
				let mut r = header(rest, 6)?;
				r.u16()?;
				let mut length = u64::from(r.u16()?);
				let format = r.u8()?;
				if num_tables == 1 && format == 0 {
					// The length is ignored, since some fonts (like OpenSans and
					// Calibri) have a subtable larger than its value.
					let pairs = clip(rest, 6, 8);
					if pairs.len() != 2 {
						return Err(Error::Struct("unpack requires a buffer of 2 bytes".into()));
					}
					let calculated = u64::from(Reader::new(pairs).u16()?) * 6 + 14;
					if length != calculated {
						log::warn!(
							"'kern' subtable longer than defined: {calculated} bytes instead of {length} bytes"
						);
					}
					length = calculated;
				}
				(length, format)
			};
			let table = clip(rest, 0, length);
			tables.push(if format == 0 {
				KernSubtable::Format0(KernFormat0::decompile(apple, table, font)?)
			} else {
				KernSubtable::Unknown {
					format: i64::from(format),
					data: table.to_vec(),
				}
			});
			rest = clip(rest, length, u64::MAX);
		}
		Ok(Kern {
			version: Some(version),
			kern_tables: Some(tables),
		})
	}
}

fn missing_version() -> Error {
	Error::ttlib("AttributeError: 'table__k_e_r_n' object has no attribute 'version'")
}

impl Table for Kern {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let version = self.version.as_ref().ok_or_else(missing_version)?;
		let num_tables = self.kern_tables.as_ref().map_or(0, Vec::len);
		let mut out = Vec::new();
		if is_one(version) {
			out.put_u32(fit(float_to_fixed(1.0, 16), "version")?);
			out.put_u32(fit(num_tables, "nTables")?);
		} else {
			let Value::Int(version) = version else {
				return Err(Error::Struct("required argument is not an integer".into()));
			};
			out.put_u16(fit(*version, "version")?);
			out.put_u16(fit(num_tables, "nTables")?);
		}
		for table in self.kern_tables.iter_mut().flatten() {
			match table {
				KernSubtable::Format0(t) => t.compile(&mut out)?,
				KernSubtable::Unknown { data, .. } => out.extend_from_slice(data),
			}
		}
		Ok(out)
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let version = self.version.as_ref().ok_or_else(missing_version)?;
		writer.simpletag("version", &[("value", version)]);
		writer.newline();
		let tables = self.kern_tables.as_ref().ok_or_else(|| {
			Error::ttlib("AttributeError: 'table__k_e_r_n' object has no attribute 'kernTables'")
		})?;
		for table in tables {
			match table {
				KernSubtable::Format0(t) => t.to_xml(writer, font)?,
				KernSubtable::Unknown { format, data } => {
					writer.begintag("kernsubtable", &[("format", format)]);
					writer.newline();
					writer.comment("unknown 'kern' subtable format");
					writer.newline();
					writer.dumphex(data);
					writer.endtag("kernsubtable");
					writer.newline();
				}
			}
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		if element.name == "version" {
			let literal = eval(required_attr(element, "value")?)?;
			self.version = Some(match literal.as_int() {
				Some(i) => Value::Int(fit(i, "version")?),
				None => Value::Float(
					literal
						.as_float()
						.ok_or_else(|| Error::value("invalid value for version"))?,
				),
			});
			return Ok(());
		}
		if element.name != "kernsubtable" {
			return Ok(());
		}
		let format = eval(required_attr(element, "format")?)?
			.as_int()
			.ok_or_else(|| Error::value("invalid kern subtable format"))?;
		let table = if format == 0 {
			let version = self.version.as_ref().ok_or_else(missing_version)?;
			KernSubtable::Format0(KernFormat0::from_xml(is_one(version), element, font)?)
		} else {
			KernSubtable::Unknown {
				format: fit(format, "format")?,
				data: read_hex(element)?,
			}
		};
		self.kern_tables.get_or_insert_default().push(table);
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		for table in self.kern_tables.iter_mut().flatten() {
			if let KernSubtable::Format0(t) = table {
				let mut kern_table = BTreeMap::new();
				for (&(left, right), &value) in &t.kern_table {
					kern_table.insert((f(left)?, f(right)?), value);
				}
				t.kern_table = kern_table;
			}
		}
		Ok(())
	}
}
