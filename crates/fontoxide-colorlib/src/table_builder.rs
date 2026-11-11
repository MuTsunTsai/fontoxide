//! Generic helper for filling in otTables from tuples and maps and such
//! (`fontTools.colorLib.table_builder`).
//!
//! fontTools sets the attributes of the tables reflectively, from their
//! converters. Here the converters of the classes that colorLib builds are
//! generated from fontTools (`generated.rs`); a table being built is an [`Obj`]
//! holding converted attribute values by name, and becomes a typed table of
//! [`fontoxide_ttlib::ot::types`] once all its attributes are set.

use std::collections::HashMap;

use fontoxide_misc::py;
use fontoxide_ttlib::GlyphOrder;
use indexmap::IndexMap;

use crate::enums::{CompositeMode, ExtendMode};
use crate::generated::{self, Class, FromTable, Table, TableRef};
use crate::{Error, Result, Value};

/// How deep tables may nest when unbuilding, like Python's recursion limit.
pub(crate) const MAX_DEPTH: usize = 100;

/// A glyph name to glyph ID map (`TTFont.getReverseGlyphMap()`).
///
/// fontTools keeps glyph names in the tables it builds and looks them up when
/// compiling them; the typed tables of fontoxide hold glyph IDs, so the
/// builders look the names up in such a map.
pub type GlyphIds = HashMap<String, u32>;

/// The integer converters (`IntValue` subclasses).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntType {
	/// `UInt8`.
	UInt8,
	/// `UShort`.
	UShort,
	/// `Short`.
	Short,
	/// `ULong` (and `VarIndex`).
	ULong,
}

/// The enum converters (`_UInt8Enum` subclasses).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnumType {
	/// `ExtendMode`.
	ExtendMode,
	/// `CompositeMode`.
	CompositeMode,
}

impl EnumType {
	/// The enum class name.
	pub fn name(self) -> &'static str {
		match self {
			EnumType::ExtendMode => "ExtendMode",
			EnumType::CompositeMode => "CompositeMode",
		}
	}

	fn has_value(self, value: i64) -> bool {
		match self {
			EnumType::ExtendMode => ExtendMode::from_value(value).is_some(),
			EnumType::CompositeMode => CompositeMode::from_value(value).is_some(),
		}
	}

	fn value_of(self, name: &str) -> Option<i64> {
		match self {
			EnumType::ExtendMode => ExtendMode::from_py_name(name).map(i64::from),
			EnumType::CompositeMode => CompositeMode::from_py_name(name).map(i64::from),
		}
	}

	/// `EnumClass(value).name`.
	fn py_name(self, value: i64) -> Option<&'static str> {
		match self {
			EnumType::ExtendMode => ExtendMode::from_value(value).map(ExtendMode::py_name),
			EnumType::CompositeMode => CompositeMode::from_value(value).map(CompositeMode::py_name),
		}
	}
}

/// What a converter does with a value in `TableBuilder._convert`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conv {
	/// An `IntValue`: `otRound(value)`.
	Int(IntType),
	/// A `FloatValue`: `float(value)`.
	Float,
	/// An enum: a member, its value or its (case-insensitive) name.
	Enum(EnumType),
	/// A `GlyphID`: a glyph name, kept as it is.
	Glyph,
	/// A `Struct`: a table of a class, built from the value.
	Struct(Class),
	/// A repeated `Struct`: a list of tables, whose length goes into the count
	/// field named second.
	Repeat(Class, &'static str),
}

/// An assignable converter of a class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
	/// The attribute name.
	pub name: &'static str,
	/// The converter.
	pub conv: Conv,
	/// The `DEFAULT` of an `OptionalValue` converter.
	pub default: Option<i64>,
}

/// `repr(cls)` of an otTables class.
pub(crate) fn class_repr(class: Class) -> String {
	format!("<class 'fontTools.ttLib.tables.otTables.{}'>", class.name())
}

/// A table being built: its class, its format and the attributes set so far.
#[derive(Debug, Clone, PartialEq)]
pub struct Obj {
	/// The class.
	pub class: Class,
	/// The `Format` of a format-switching class.
	pub format: Option<u16>,
	/// The attributes, converted.
	pub attrs: IndexMap<String, Value>,
}

impl Obj {
	/// `cls()`: a table without attributes.
	pub fn new(class: Class) -> Self {
		Obj {
			class,
			format: None,
			attrs: IndexMap::new(),
		}
	}

	/// Sets an attribute (`setattr`).
	pub fn set(&mut self, name: &str, value: impl Into<Value>) {
		self.attrs.insert(name.to_owned(), value.into());
	}

	fn take(&mut self, name: &str) -> Result<Value> {
		// LENIENT(L32): fontTools leaves a field that the source does not give unset,
		// and fails with this error only when compiling or dumping the table.
		self.attrs.swap_remove(name).ok_or_else(|| {
			Error::Attribute(format!(
				"'{}' object has no attribute '{name}'",
				self.class.name()
			))
		})
	}

	pub(crate) fn int_field<T>(&mut self, name: &str, _ty: IntType) -> Result<T>
	where
		i64: TryInto<T>,
	{
		match self.take(name)? {
			// LENIENT(L32): fontTools keeps any integer and fails when compiling a value
			// that does not fit the field; it fails here.
			Value::Int(v) => Ok(fontoxide_ttlib::binary::fit::<T, i64>(v, name)?),
			other => Err(Error::Type(format!(
				"required argument is not an integer: {name} = {}",
				other.py_repr()
			))),
		}
	}

	pub(crate) fn float_field(&mut self, name: &str) -> Result<f64> {
		match self.take(name)? {
			Value::Float(v) => Ok(v),
			Value::Int(v) => Ok(v as f64),
			other => Err(Error::Type(format!(
				"must be real number, not {}",
				other.py_type_name()
			))),
		}
	}

	pub(crate) fn glyph_field(&mut self, name: &str, glyphs: &GlyphIds) -> Result<u32> {
		match self.take(name)? {
			// LENIENT(L32): fontTools keeps the glyph name, and looks it up when compiling.
			Value::Str(glyph) => glyphs
				.get(&glyph)
				.copied()
				.ok_or_else(|| Error::key(&glyph)),
			other => Err(Error::Type(format!(
				"glyph name must be str, not {}",
				other.py_type_name()
			))),
		}
	}

	pub(crate) fn table_field<T: FromTable>(&mut self, name: &str) -> Result<T> {
		let value = self.take(name)?;
		table_of::<T>(value, self.class, name)
	}

	pub(crate) fn tables_field<T: FromTable>(&mut self, name: &str) -> Result<Vec<T>> {
		match self.take(name)? {
			Value::List(items) => items
				.into_iter()
				.map(|v| table_of::<T>(v, self.class, name))
				.collect(),
			other => Ok(vec![table_of::<T>(other, self.class, name)?]),
		}
	}

	pub(crate) fn count_field(&mut self, name: &str) -> Result<Option<u32>> {
		match self.attrs.swap_remove(name) {
			None => Ok(None),
			Some(Value::Int(v)) => Ok(Some(fontoxide_ttlib::binary::fit::<u32, i64>(v, name)?)),
			Some(other) => Err(Error::Type(format!(
				"required argument is not an integer: {name} = {}",
				other.py_repr()
			))),
		}
	}

	pub(crate) fn invalid_format(&self) -> Error {
		Error::Assertion(format!(
			"{} invalid Format: {}",
			class_repr(self.class),
			self.format
				.map_or_else(|| "None".to_owned(), |f| f.to_string())
		))
	}
}

fn table_of<T: FromTable>(value: Value, class: Class, name: &str) -> Result<T> {
	match value {
		Value::Table(t) => T::from_table(t).map_err(|t| {
			Error::Type(format!(
				"{}.{name} must be a {}, not {}",
				class.name(),
				T::CLASS.name(),
				t.class().name()
			))
		}),
		other => Err(Error::Type(format!(
			"{}.{name} must be a {}, not {}",
			class.name(),
			T::CLASS.name(),
			other.py_type_name()
		))),
	}
}

/// The table that a build step works on: still being built, or built already
/// (a callback may replace it with any table).
#[derive(Debug, Clone, PartialEq)]
pub enum Dest {
	/// A table being built.
	Obj(Obj),
	/// A built table.
	Table(Table),
}

/// The lifecycle callbacks of a [`TableBuilder`] (`BuildCallback`), keyed by
/// class and format.
pub trait BuildCallbacks {
	/// `CREATE_DEFAULT`: a new instance of the class, instead of an empty one.
	fn create_default(&mut self, class: Class, format: Option<u16>) -> Option<Obj> {
		let _ = (class, format);
		None
	}

	/// `BEFORE_BUILD`: receives `(dest, source)`, with a sequence source turned
	/// into a mapping already, and returns them, possibly replaced.
	fn before_build(
		&mut self,
		builder: &TableBuilder<'_>,
		class: Class,
		format: Option<u16>,
		dest: Dest,
		source: Value,
	) -> Result<(Dest, Value)> {
		let _ = (builder, class, format);
		Ok((dest, source))
	}

	/// `AFTER_BUILD`: receives the table with all its attributes set and returns
	/// it, possibly replaced.
	fn after_build(
		&mut self,
		builder: &TableBuilder<'_>,
		class: Class,
		format: Option<u16>,
		dest: Dest,
	) -> Result<Dest> {
		let _ = (builder, class, format);
		Ok(dest)
	}
}

/// No callbacks (`TableBuilder()` without a callback table).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCallbacks;

impl BuildCallbacks for NoCallbacks {}

/// `otRound(value)`.
fn ot_round(value: &Value) -> Result<i64> {
	let v = match value {
		// Python adds 0.5 to the integer, which makes it a float.
		Value::Int(v) => *v as f64,
		Value::Float(v) => *v,
		Value::Str(_) => {
			return Err(Error::Type(
				"can only concatenate str (not \"float\") to str".into(),
			));
		}
		Value::List(_) => {
			return Err(Error::Type(
				"can only concatenate list (not \"float\") to list".into(),
			));
		}
		other => {
			return Err(Error::Type(format!(
				"unsupported operand type(s) for +: '{}' and 'float'",
				other.py_type_name()
			)));
		}
	};
	let v = (v + 0.5).floor();
	if v.is_nan() {
		return Err(Error::Value("cannot convert float NaN to integer".into()));
	}
	if v.is_infinite() {
		return Err(Error::Overflow(
			"cannot convert float infinity to integer".into(),
		));
	}
	// Values beyond i64 do not fit any field anyway.
	Ok(v as i64)
}

/// `float(value)`.
fn to_float(value: &Value) -> Result<f64> {
	match value {
		Value::Int(v) => Ok(*v as f64),
		Value::Float(v) => Ok(*v),
		Value::Str(s) => py::parse_float(s).ok_or_else(|| {
			Error::Value(format!(
				"could not convert string to float: {}",
				py::str_repr(s)
			))
		}),
		other => Err(Error::Type(format!(
			"float() argument must be a string or a real number, not '{}'",
			other.py_type_name()
		))),
	}
}

/// The enum branch of `_convert`.
fn enum_value(e: EnumType, value: &Value) -> Result<i64> {
	match value {
		Value::Str(s) => e
			.value_of(&s.to_uppercase())
			.ok_or_else(|| Error::Value(format!("{s} is not a valid <enum '{}'>", e.name()))),
		Value::Int(v) if e.has_value(*v) => Ok(*v),
		Value::Float(v) if v.fract() == 0.0 && e.has_value(*v as i64) => Ok(*v as i64),
		other => Err(Error::Value(format!(
			"{} is not a valid {}",
			other.py_repr(),
			e.name()
		))),
	}
}

/// `_split_format`: the format of a format-switching class, and the rest of
/// the source.
fn split_format(class: Class, source: Value) -> Result<(u16, Value)> {
	let (format, remainder) = match source {
		Value::List(mut items) => {
			if items.is_empty() {
				return Err(Error::Assertion(format!(
					"{} needs at least format from []",
					class_repr(class)
				)));
			}
			let format = items.remove(0);
			(format, Value::List(items))
		}
		Value::Map(mut map) => {
			let Some(format) = map.shift_remove("Format") else {
				return Err(Error::Assertion(format!(
					"{} needs at least Format from {}",
					class_repr(class),
					Value::Map(map).py_repr()
				)));
			};
			(format, Value::Map(map))
		}
		other => {
			return Err(Error::Value(format!(
				"Not sure how to populate {} from {}",
				class_repr(class),
				other.py_repr()
			)));
		}
	};
	if matches!(format, Value::List(_) | Value::Map(_) | Value::Table(_)) {
		return Err(Error::Assertion(format!(
			"{} Format is not hashable: {}",
			class_repr(class),
			format.py_repr()
		)));
	}
	let formats = class.formats().unwrap_or(&[]);
	let found = match format {
		Value::Int(v) => formats.iter().copied().find(|&f| i64::from(f) == v),
		Value::Float(v) => formats.iter().copied().find(|&f| f64::from(f) == v),
		_ => None,
	};
	match found {
		Some(f) => Ok((f, remainder)),
		None => Err(Error::Assertion(format!(
			"{} invalid Format: {}",
			class_repr(class),
			format.py_repr()
		))),
	}
}

/// Helps to populate otTables from maps, tuples, etc. (`TableBuilder`).
///
/// Lifecycle callbacks ([`BuildCallbacks`]) add logic beyond what is possible
/// based on the otData info of the classes.
#[derive(Debug, Clone, Copy)]
pub struct TableBuilder<'a> {
	glyphs: &'a GlyphIds,
}

impl<'a> TableBuilder<'a> {
	/// A builder that looks glyph names up in `glyphs`.
	pub fn new(glyphs: &'a GlyphIds) -> Self {
		TableBuilder { glyphs }
	}

	/// The glyph name to ID map.
	pub fn glyphs(&self) -> &'a GlyphIds {
		self.glyphs
	}

	/// `_convert`: converts a value for a field and sets it on `dest`.
	pub fn convert(
		&self,
		callbacks: &mut dyn BuildCallbacks,
		dest: &mut Obj,
		field: &Field,
		value: Value,
	) -> Result<()> {
		let value = match field.conv {
			Conv::Enum(e) => Value::Int(enum_value(e, &value)?),
			Conv::Int(_) => Value::Int(ot_round(&value)?),
			Conv::Float => Value::Float(to_float(&value)?),
			Conv::Struct(class) => Value::Table(self.build(callbacks, class, value)?),
			Conv::Repeat(class, count) => {
				let items = match value {
					Value::List(items) => items,
					other => vec![other],
				};
				let mut built = Vec::with_capacity(items.len());
				for item in items {
					built.push(Value::Table(self.build(callbacks, class, item)?));
				}
				dest.set(count, built.len());
				Value::List(built)
			}
			Conv::Glyph => value,
		};
		dest.attrs.insert(field.name.to_owned(), value);
		Ok(())
	}

	/// `build(cls, source)`: a table of a class from a source, which is a
	/// table of the class already, a mapping of fields, or a sequence of fields
	/// in declaration order (after the format, for format-switching classes).
	pub fn build(
		&self,
		callbacks: &mut dyn BuildCallbacks,
		class: Class,
		source: Value,
	) -> Result<Table> {
		let source = match source {
			Value::Table(t) if t.class() == class => return Ok(t),
			other => other,
		};

		let (format, source) = if class.formats().is_some() {
			let (format, source) = split_format(class, source)?;
			(Some(format), source)
		} else {
			(None, source)
		};

		let mut dest = callbacks
			.create_default(class, format)
			.unwrap_or_else(|| Obj::new(class));
		let fields = class.fields(format).ok_or_else(|| dest.invalid_format())?;
		if format.is_some() {
			dest.format = format;
		}

		// Convert sequence => mapping so before thunk only has to handle one format.
		let source = match source {
			Value::List(items) => {
				if items.len() > fields.len() {
					return Err(Error::Assertion(format!(
						"Sequence of {} too long for {}; expected <= {} values",
						items.len(),
						class_repr(class),
						fields.len()
					)));
				}
				Value::Map(
					fields
						.iter()
						.zip(items)
						.map(|(f, v)| (f.name.to_owned(), v))
						.collect(),
				)
			}
			other => other,
		};

		let (mut dest, source) =
			callbacks.before_build(self, class, format, Dest::Obj(dest), source)?;

		match source {
			Value::Map(map) => {
				for (name, value) in map {
					if format.is_some() && name == "Format" {
						continue;
					}
					let Some(field) = fields.iter().find(|f| f.name == name) else {
						let mut names: Vec<&str> = fields.iter().map(|f| f.name).collect();
						names.sort_unstable();
						let names: Vec<String> = names.iter().map(|n| py::str_repr(n)).collect();
						return Err(Error::Value(format!(
							"Unrecognized field {name} for {}; expected one of [{}]",
							class_repr(class),
							names.join(", ")
						)));
					};
					match &mut dest {
						Dest::Obj(obj) => self.convert(callbacks, obj, field, value)?,
						Dest::Table(_) => {
							return Err(Error::Attribute(format!(
								"cannot set {name} on a table built already"
							)));
						}
					}
				}
			}
			other => {
				// Let's try as a 1-tuple.
				dest = Dest::Table(self.build(callbacks, class, Value::List(vec![other]))?);
			}
		}

		if let Dest::Obj(obj) = &mut dest {
			for field in fields {
				if let Some(default) = field.default
					&& !obj.attrs.contains_key(field.name)
				{
					obj.set(field.name, default);
				}
			}
		}

		match callbacks.after_build(self, class, format, dest)? {
			Dest::Obj(mut obj) => generated::assemble(&mut obj, self.glyphs),
			Dest::Table(t) => Ok(t),
		}
	}

	/// [`build`](Self::build) for a typed class.
	pub fn build_as<T: FromTable>(
		&self,
		callbacks: &mut dyn BuildCallbacks,
		source: Value,
	) -> Result<T> {
		let table = self.build(callbacks, T::CLASS, source)?;
		T::from_table(table).map_err(|t| {
			Error::Type(format!(
				"expected a {}, built a {}",
				T::CLASS.name(),
				t.class().name()
			))
		})
	}
}

/// A field value of a built table, for unbuilding.
#[derive(Debug, Clone)]
pub(crate) enum Out<'a> {
	Int(i64),
	Float(f64),
	Enum(EnumType, u8),
	Glyph(u32),
	Table(Option<TableRef<'a>>),
	Tables(Vec<TableRef<'a>>),
}

/// The format and the assignable fields of a built table, in converter order.
pub(crate) type UnbuiltFields<'a> = (Option<u16>, Vec<(&'static str, Out<'a>)>);

/// The callbacks of a [`TableUnbuilder`], keyed by class and format.
pub trait UnbuildCallbacks {
	/// Receives the unbuilt source of a table and returns it, possibly replaced.
	fn unbuilt(
		&mut self,
		unbuilder: &mut TableUnbuilder<'_>,
		class: Class,
		format: Option<u16>,
		source: IndexMap<String, Value>,
	) -> Result<Value> {
		let _ = (unbuilder, class, format);
		Ok(Value::Map(source))
	}
}

impl UnbuildCallbacks for NoCallbacks {}

/// Turns built tables back into mappings (`TableUnbuilder`).
#[derive(Debug, Clone)]
pub struct TableUnbuilder<'a> {
	order: &'a GlyphOrder,
	depth: usize,
}

impl<'a> TableUnbuilder<'a> {
	/// An unbuilder writing glyph names of `order`.
	pub fn new(order: &'a GlyphOrder) -> Self {
		TableUnbuilder { order, depth: 0 }
	}

	/// `unbuild(table)`: a mapping with the `Format` and the assignable fields of
	/// the table, enums as lowercase names and glyphs as names.
	pub fn unbuild(
		&mut self,
		table: TableRef<'_>,
		callbacks: &mut dyn UnbuildCallbacks,
	) -> Result<Value> {
		if self.depth >= MAX_DEPTH {
			return Err(Error::Recursion("maximum recursion depth exceeded".into()));
		}
		self.depth += 1;
		let result = self.unbuild_inner(table, callbacks);
		self.depth -= 1;
		result
	}

	fn unbuild_inner(
		&mut self,
		table: TableRef<'_>,
		callbacks: &mut dyn UnbuildCallbacks,
	) -> Result<Value> {
		let class = table_class(table);
		let Some((format, fields)) = generated::unbuild_fields(table) else {
			return Err(Error::Assertion(format!(
				"{} invalid Format",
				class_repr(class)
			)));
		};
		let mut source = IndexMap::new();
		if let Some(format) = format {
			source.insert("Format".to_owned(), Value::Int(i64::from(format)));
		}
		for (name, out) in fields {
			let value = match out {
				Out::Int(v) => Value::Int(v),
				Out::Float(v) => Value::Float(v),
				Out::Enum(e, v) => {
					let member = e
						.py_name(i64::from(v))
						.ok_or_else(|| Error::Value(format!("{v} is not a valid {}", e.name())))?;
					Value::Str(member.to_lowercase())
				}
				Out::Glyph(gid) => Value::Str(self.order.name(gid).into_owned()),
				Out::Table(Some(t)) => self.unbuild(t, callbacks)?,
				Out::Table(None) => {
					return Err(Error::Assertion("isinstance(table, BaseTable)".into()));
				}
				Out::Tables(items) => {
					let mut list = Vec::with_capacity(items.len());
					for t in items {
						list.push(self.unbuild(t, callbacks)?);
					}
					Value::List(list)
				}
			};
			source.insert(name.to_owned(), value);
		}
		callbacks.unbuilt(self, class, format, source)
	}
}

/// The class of a table reference.
pub(crate) fn table_class(table: TableRef<'_>) -> Class {
	match table {
		TableRef::Paint(_) => Class::Paint,
		TableRef::ColorLine(_) => Class::ColorLine,
		TableRef::VarColorLine(_) => Class::VarColorLine,
		TableRef::ColorStop(_) => Class::ColorStop,
		TableRef::VarColorStop(_) => Class::VarColorStop,
		TableRef::Affine2x3(_) => Class::Affine2x3,
		TableRef::VarAffine2x3(_) => Class::VarAffine2x3,
		TableRef::ClipBox(_) => Class::ClipBox,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// `Tests/colorLib/table_builder_test.py::test_intValue_otRound`.
	#[test]
	fn test_int_value_ot_round() -> Result<()> {
		let glyphs = GlyphIds::new();
		let mut dest = Obj::new(Class::ColorStop);
		let converter = Field {
			name: "value",
			conv: Conv::Int(IntType::UShort),
			default: None,
		};
		TableBuilder::new(&glyphs).convert(
			&mut NoCallbacks,
			&mut dest,
			&converter,
			Value::Float(85.6),
		)?;
		assert_eq!(
			dest.attrs["value"],
			Value::Int(86),
			"Should have used otRound"
		);
		Ok(())
	}

	#[test]
	fn conversions() {
		assert_eq!(ot_round(&Value::Float(-0.5)).ok(), Some(0));
		assert!(matches!(
			ot_round(&Value::Float(f64::INFINITY)),
			Err(Error::Overflow(_))
		));
		assert_eq!(
			ot_round(&Value::from("x")).err().map(|e| e.to_string()),
			Some("can only concatenate str (not \"float\") to str".into())
		);
		assert_eq!(to_float(&Value::from("1.5")).ok(), Some(1.5));
		assert_eq!(
			enum_value(EnumType::ExtendMode, &Value::from("Reflect")).ok(),
			Some(2)
		);
		assert_eq!(
			enum_value(EnumType::ExtendMode, &Value::Int(7))
				.err()
				.map(|e| e.to_string()),
			Some("7 is not a valid ExtendMode".into())
		);
		assert_eq!(
			enum_value(EnumType::ExtendMode, &Value::from("foo"))
				.err()
				.map(|e| e.to_string()),
			Some("foo is not a valid <enum 'ExtendMode'>".into())
		);
	}
}
