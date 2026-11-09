//! Generates the OpenType Layout types from fontTools' otTables classes
//! (`codegen/ottables.json`, exported by `reference.py`).
//!
//! Every class becomes a struct (or, for format-switching tables, an enum of one
//! struct per format) with `decompile`, `compile`, `to_xml2`, `from_xml` and
//! `populate_defaults` that follow the class's converter list exactly as
//! `fontTools.ttLib.tables.otBase.BaseTable` does.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct OtTables {
	classes: Vec<Class>,
	#[serde(rename = "lookupTypes")]
	lookup_types: BTreeMap<String, BTreeMap<String, String>>,
	#[serde(rename = "featureParamTypes")]
	feature_param_types: BTreeMap<String, String>,
}

#[derive(Deserialize, Clone)]
struct Class {
	name: String,
	kind: String,
	#[serde(default)]
	hooks: Vec<String>,
	#[serde(default, rename = "inheritedHooks")]
	inherited_hooks: Vec<String>,
	#[serde(default)]
	attrs: BTreeMap<String, serde_json::Value>,
	#[serde(default)]
	converters: Vec<Conv>,
	#[serde(default)]
	formats: BTreeMap<String, Vec<Conv>>,
}

#[derive(Deserialize, Clone)]
struct Conv {
	converter: String,
	name: String,
	repeat: Option<serde_json::Value>,
	aux: Option<serde_json::Value>,
	condition: Option<String>,
	#[serde(rename = "tableClass")]
	table_class: Option<String>,
	#[serde(rename = "staticSize")]
	static_size: Option<serde_json::Value>,
	#[serde(rename = "isCount")]
	is_count: bool,
	#[serde(rename = "isPropagated")]
	is_propagated: bool,
	#[serde(rename = "isLookupType")]
	is_lookup_type: bool,
	/// The value converter of an `AATLookup`.
	#[serde(rename = "valueConverter", default)]
	value_converter: Option<String>,
}

/// Classes whose fontTools representation differs from the binary layout
/// (`postRead`/`preWrite`): written by hand in `crate::ot::custom`; the generator
/// emits their raw layout as `<Name>Raw`.
/// Converters that write an array as one item (`writeArray` overridden):
/// `Long`, `ULong`, `Short`, `UShort`, `Int8`, `UInt8`, the glyph IDs, and their
/// subclasses.
const ARRAY_CONVERTERS: &[&str] = &[
	"Long",
	"ULong",
	"Flags32",
	"VarIndex",
	"Short",
	"UShort",
	"NameID",
	"STATFlags",
	"LookupFlag",
	"Int8",
	"UInt8",
	"ExtendMode",
	"CompositeMode",
	"GlyphID",
	"GlyphID24",
	"GlyphID32",
];

const CUSTOM: &[&str] = &[
	"Coverage",
	"ClassDef",
	"SingleSubst",
	"MultipleSubst",
	"AlternateSubst",
	"LigatureSubst",
	"VarIdxMap",
	"DeltaSetIndexMap",
	"ClipList",
	"MortSubtable",
	"MorxSubtable",
	"EntryIdStringData",
];

/// The count that a class reads before its other fields, and whose reference it
/// creates before writing them (`COLR.decompile` / `COLR.preWrite`): the count
/// follows the offset to the array it counts.
fn pre_count(class: &str) -> Option<&'static str> {
	match class {
		"COLR" => Some("LayerRecordCount"),
		_ => None,
	}
}

/// Classes whose `preWrite` sorts an array by a glyph field: (array, glyph field).
fn sort_by_glyph(class: &str) -> Option<(&'static str, &'static str)> {
	match class {
		"BaseGlyphRecordArray" => Some(("BaseGlyphRecord", "BaseGlyph")),
		"BaseGlyphList" => Some(("BaseGlyphPaintRecord", "BaseGlyph")),
		_ => None,
	}
}

/// Converters that are not implemented yet; fields using them get the
/// `Unsupported` type, which fails when read.
const UNSUPPORTED: &[&str] = &["SubStruct", "VarIdxMapValue", "STXHeader"];

fn snake(name: &str) -> String {
	let chars: Vec<char> = name.chars().collect();
	let mut out = String::new();
	for (i, &c) in chars.iter().enumerate() {
		if c.is_ascii_uppercase() {
			let prev = i.checked_sub(1).map(|j| chars[j]);
			let next = chars.get(i + 1).copied();
			let boundary = match prev {
				None => false,
				Some(p) if p.is_ascii_lowercase() || p.is_ascii_digit() => true,
				Some(p) if p.is_ascii_uppercase() => next.is_some_and(|n| n.is_ascii_lowercase()),
				_ => false,
			};
			if boundary && !out.ends_with('_') {
				out.push('_');
			}
			out.push(c.to_ascii_lowercase());
		} else {
			out.push(c);
		}
	}
	match out.as_str() {
		"type" | "match" | "loop" | "ref" | "box" | "move" | "self" | "use" | "where" | "mod"
		| "fn" | "impl" => {
			format!("{out}_")
		}
		_ => out,
	}
}

fn type_name(name: &str) -> String {
	let mut c = name.chars();
	match c.next() {
		Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
		None => String::new(),
	}
}

fn json_int(v: &Option<serde_json::Value>) -> Option<i64> {
	v.as_ref().and_then(serde_json::Value::as_i64)
}

fn json_str(v: &Option<serde_json::Value>) -> Option<&str> {
	v.as_ref().and_then(serde_json::Value::as_str)
}

/// Translates a fontTools condition expression into Rust over the table `t`.
fn condition(expr: &str, fields: &HashMap<String, String>) -> Result<String> {
	let field = |n: &str| -> Result<String> {
		let f = fields
			.get(n)
			.with_context(|| format!("condition refers to unknown field {n:?}"))?;
		Ok(format!("(t.{f} as i64)"))
	};
	let parts: Vec<&str> = expr.split_whitespace().collect();
	match parts.as_slice() {
		[a, ">=", n] => Ok(format!("{} >= {n}", field(a)?)),
		[a, "&", n] => Ok(format!("{} & {n} != 0", field(a)?)),
		_ if expr.contains(" in (") => {
			let (a, rest) = expr.split_once(" in (").unwrap_or_default();
			let values: Vec<&str> = rest
				.trim_end_matches(')')
				.split(',')
				.map(str::trim)
				.collect();
			Ok(format!(
				"matches!({}, {})",
				field(a.trim())?,
				values.join(" | ")
			))
		}
		_ => bail!("unsupported condition {expr:?}"),
	}
}

/// What a converter's field holds and how it is processed.
#[derive(Clone)]
enum Kind {
	/// An integer; `(rust type, reader method, writer method)`.
	Int(&'static str, &'static str, &'static str),
	/// A count; its byte size.
	Count(usize),
	Int24,
	Version,
	Flags32,
	VarIndex,
	NameId,
	StatFlags,
	LookupFlag,
	ValueFormat,
	Glyph(usize),
	/// A fixed-point or decimal number; `(fraction bits or 0 for DeciPoints, angle bias)`.
	Float(&'static str),
	Tag,
	Char64,
	Enum(&'static str),
	Struct(String),
	Offset(String, usize),
	SubTable(usize),
	FeatureParams,
	FeatureVariations,
	AlternateFeature(String),
	ValueRecord,
	DeltaValue,
	VarDataValue,
	/// A CFF2 INDEX of packed tuples (`TupleList`); whether it is behind an offset
	/// (`TupleListOffset`).
	TupleList(bool),
	/// A CFF2 INDEX of variable composite glyphs (`VarCompositeGlyphList`).
	VarCompositeGlyphList,
	/// IFT mapping entries (`MappingEntriesConverter`).
	MappingEntries,
	/// A glyph-keyed AAT lookup with values of the given Rust type (`AATLookup`).
	AatLookup(String),
	/// An `ankr` lookup of glyph data tables (`AATLookupWithDataOffset`).
	AatLookupWithDataOffset(String),
	/// A struct followed by padding up to its `StructLength` (`StructWithLength`).
	StructWithLength(String),
	/// CIDs to glyphs (`CIDGlyphMap`).
	CidGlyphMap,
	/// Glyphs to CIDs (`GlyphCIDMap`).
	GlyphCidMap,
	/// An AAT state table with actions of the given `ActionKind` variant
	/// (`MorphStateTable`).
	MorphStateTable(String),
	/// A `mort` subtable (`MortSubtableConverter`).
	MortSubtable,
	/// A `morx` subtable (`MorxSubtableConverter`).
	MorxSubtable,
	Unsupported(String),
}

struct Field {
	conv: Conv,
	ident: String,
	kind: Kind,
	/// The Rust type of one value.
	ty: String,
	repeat: Option<Repeat>,
	/// When the field is read and written: `condition` and, for fields that are
	/// not arrays, `aux`.
	cond: Option<String>,
	/// `condition` alone (`isEnabled`).
	enabled: Option<String>,
}

enum Repeat {
	Fixed(i64),
	/// A count read in this table: (field ident, name, aux, whether it is a
	/// computed count held as `Option<u32>`).
	Local(String, String, i64, bool),
	/// A count propagated from a parent table (name), plus aux.
	Propagated(String, i64),
}

fn rust_class(name: &str, raw: bool) -> String {
	if raw {
		format!("{}Raw", type_name(name))
	} else {
		type_name(name)
	}
}

fn kind_of(c: &Conv) -> Result<(Kind, String)> {
	let tc = || c.table_class.clone().unwrap_or_default();
	let target = |name: &str| rust_class(name, false);
	Ok(match c.converter.as_str() {
		_ if c.converter.starts_with("Computed") => {
			let size = json_int(&c.static_size).unwrap_or(2) as usize;
			(Kind::Count(size), "Option<u32>".into())
		}
		"UShort" if c.is_count => (Kind::Count(2), "Option<u32>".into()),
		"UShort" => (Kind::Int("u16", "read_u16", "write_u16"), "u16".into()),
		"Short" => (Kind::Int("i16", "read_i16", "write_i16"), "i16".into()),
		"ULong" => (Kind::Int("u32", "read_u32", "write_u32"), "u32".into()),
		"Long" => (Kind::Int("i32", "read_i32", "write_i32"), "i32".into()),
		"UInt8" => (Kind::Int("u8", "read_u8", "write_u8"), "u8".into()),
		"Int8" => (Kind::Int("i8", "read_i8", "write_i8"), "i8".into()),
		"UInt24" => (Kind::Int("u32", "read_u24", "write_u24"), "u32".into()),
		"Int24" => (Kind::Int24, "i32".into()),
		"Version" => (Kind::Version, "u32".into()),
		"Flags32" => (Kind::Flags32, "u32".into()),
		"VarIndex" => (Kind::VarIndex, "u32".into()),
		"NameID" => (Kind::NameId, "u16".into()),
		"STATFlags" => (Kind::StatFlags, "u16".into()),
		"LookupFlag" => (Kind::LookupFlag, "u16".into()),
		"ValueFormat" => (Kind::ValueFormat, "u16".into()),
		"GlyphID" => (Kind::Glyph(2), "u32".into()),
		"GlyphID24" => (Kind::Glyph(3), "u32".into()),
		"GlyphID32" => (Kind::Glyph(4), "u32".into()),
		"Fixed" => (Kind::Float("Fixed"), "f64".into()),
		"F2Dot14" => (Kind::Float("F2Dot14"), "f64".into()),
		"Angle" => (Kind::Float("Angle"), "f64".into()),
		"BiasedAngle" => (Kind::Float("BiasedAngle"), "f64".into()),
		"DeciPoints" => (Kind::Float("DeciPoints"), "f64".into()),
		"Tag" => (Kind::Tag, "Tag".into()),
		"Char64" => (Kind::Char64, "String".into()),
		"ExtendMode" => (Kind::Enum("ExtendMode"), "u8".into()),
		"CompositeMode" => (Kind::Enum("CompositeMode"), "u8".into()),
		"Struct" => (Kind::Struct(tc()), target(&tc())),
		"Table" => (
			Kind::Offset(tc(), 2),
			format!("Option<Box<{}>>", target(&tc())),
		),
		"LTable" => (
			Kind::Offset(tc(), 4),
			format!("Option<Box<{}>>", target(&tc())),
		),
		"Table24" => (
			Kind::Offset(tc(), 3),
			format!("Option<Box<{}>>", target(&tc())),
		),
		"SubTable" => (Kind::SubTable(2), "Option<Box<LookupSubTable>>".into()),
		"ExtSubTable" => (Kind::SubTable(4), "Option<Box<LookupSubTable>>".into()),
		"FeatureParams" => (Kind::FeatureParams, "Option<Box<FeatureParamsAny>>".into()),
		"FeatureVariations" => (
			Kind::FeatureVariations,
			format!("Option<Box<{}>>", target(&tc())),
		),
		"AlternateFeature" => (
			Kind::AlternateFeature(tc()),
			format!("Option<Box<{}>>", target(&tc())),
		),
		"ValueRecord" => (Kind::ValueRecord, "Option<ValueRecord>".into()),
		"DeltaValue" => (Kind::DeltaValue, "Vec<i64>".into()),
		"VarDataValue" => (Kind::VarDataValue, "Vec<i64>".into()),
		"CFF2Index" if c.table_class.as_deref() == Some("VarCompositeGlyph") => {
			(Kind::VarCompositeGlyphList, "Vec<VarCompositeGlyph>".into())
		}
		"CFF2Index" => (Kind::TupleList(false), "Vec<Vec<i64>>".into()),
		"TupleListOffset" => (Kind::TupleList(true), "Vec<Vec<i64>>".into()),
		"MappingEntriesConverter" => (Kind::MappingEntries, "Vec<MappingEntry>".into()),
		"AATLookup" => {
			let value = match c.value_converter.as_deref() {
				Some("UShort") => "u16".to_owned(),
				Some("GlyphID") => "u32".to_owned(),
				Some("Table") => format!("Option<Box<{}>>", target(&tc())),
				other => bail!("unknown AATLookup value converter {other:?} for {}", c.name),
			};
			(
				Kind::AatLookup(value.clone()),
				format!("AatLookup<{value}>"),
			)
		}
		"AATLookupWithDataOffset" => {
			let value = format!("Option<Box<{}>>", target(&tc()));
			(
				Kind::AatLookupWithDataOffset(target(&tc())),
				format!("AatLookup<{value}>"),
			)
		}
		"StructWithLength" => (Kind::StructWithLength(tc()), target(&tc())),
		"CIDGlyphMap" => (
			Kind::CidGlyphMap,
			"std::collections::BTreeMap<u32, u32>".into(),
		),
		"GlyphCIDMap" => (
			Kind::GlyphCidMap,
			"std::collections::BTreeMap<u32, u32>".into(),
		),
		"MorphStateTable" => {
			let kind = tc()
				.strip_suffix("MorphAction")
				.map(str::to_owned)
				.ok_or_else(|| anyhow::anyhow!("unknown state table action {}", tc()))?;
			(Kind::MorphStateTable(kind), "AatStateTable".into())
		}
		"MortSubtableConverter" => (Kind::MortSubtable, "MortSubtable".into()),
		"MorxSubtableConverter" => (Kind::MorxSubtable, "MorxSubtable".into()),
		other if UNSUPPORTED.contains(&other) => {
			(Kind::Unsupported(other.into()), "Unsupported".into())
		}
		other => bail!("unknown converter {other} for {}", c.name),
	})
}

fn fields_of(convs: &[Conv]) -> Result<Vec<Field>> {
	let mut idents: HashMap<String, String> = HashMap::new();
	for c in convs {
		idents.insert(c.name.clone(), snake(&c.name));
	}
	let mut fields = Vec::new();
	for (i, c) in convs.iter().enumerate() {
		let (kind, ty) = kind_of(c)?;
		let aux_int = json_int(&c.aux).unwrap_or(0);
		let repeat = match &c.repeat {
			None => None,
			// An empty string is falsy: no repeat.
			Some(serde_json::Value::String(s)) if s.is_empty() => None,
			Some(serde_json::Value::Number(n)) => Some(Repeat::Fixed(n.as_i64().unwrap_or(0))),
			Some(serde_json::Value::String(s)) => {
				let local = convs[..i].iter().find(|o| &o.name == s);
				if let Some(o) = local {
					let (k, _) = kind_of(o)?;
					Some(Repeat::Local(
						idents[s].clone(),
						s.clone(),
						aux_int,
						matches!(k, Kind::Count(_)),
					))
				} else {
					Some(Repeat::Propagated(s.clone(), aux_int))
				}
			}
			_ => bail!("bad repeat for {}", c.name),
		};
		let mut conds = Vec::new();
		if let Some(cond) = &c.condition {
			conds.push(condition(cond, &idents)?);
		}
		let enabled = conds.first().cloned();
		if c.repeat.is_none()
			&& let Some(aux) = json_str(&c.aux)
		{
			conds.push(condition(aux, &idents)?);
		}
		fields.push(Field {
			ident: idents[&c.name].clone(),
			conv: c.clone(),
			kind,
			ty,
			repeat,
			cond: (!conds.is_empty()).then(|| conds.join(" && ")),
			enabled,
		});
	}
	Ok(fields)
}

struct Gen<'a> {
	out: String,
	ot: &'a OtTables,
	/// Class name -> (kind, struct or enum).
	classes: BTreeMap<String, Class>,
}

impl Gen<'_> {
	fn line(&mut self, s: &str) {
		self.out.push_str(s);
		self.out.push('\n');
	}

	fn struct_def(
		&mut self,
		ty: &str,
		fields: &[Field],
		doc: &str,
		public: bool,
		dont_share: bool,
	) {
		let vis = "pub";
		let _ = writeln!(self.out, "/// {doc}");
		if !public {
			self.line("#[doc(hidden)]");
		}
		self.line("#[derive(Debug, Clone, PartialEq)]");
		let _ = writeln!(self.out, "{vis} struct {ty} {{");
		for f in fields {
			let ty = if f.repeat.is_some() && !matches!(f.kind, Kind::DeltaValue) {
				format!("Vec<{}>", f.ty)
			} else {
				f.ty.clone()
			};
			let _ = writeln!(self.out, "\t/// `{}` ({}).", f.conv.name, f.conv.converter);
			let _ = writeln!(self.out, "\tpub {}: {ty},", f.ident);
		}
		if dont_share {
			self.line("\t/// Whether the subtables below are never shared (`DontShare`), set when");
			self.line("\t/// resolving offset overflows.");
			self.line("\tpub dont_share: bool,");
		}
		self.line("}");
		self.line("");
		let _ = writeln!(self.out, "impl Default for {ty} {{");
		self.line("\tfn default() -> Self {");
		let _ = writeln!(self.out, "\t\t{ty} {{");
		for f in fields {
			let default = if f.repeat.is_none() && matches!(f.kind, Kind::VarIndex) {
				"0xFFFF_FFFF".to_owned()
			} else {
				"Default::default()".to_owned()
			};
			let _ = writeln!(self.out, "\t\t\t{}: {default},", f.ident);
		}
		if dont_share {
			self.line("\t\t\tdont_share: false,");
		}
		self.line("\t\t}");
		self.line("\t}");
		self.line("}");
		self.line("");
	}

	/// The statements that read all fields into `t`.
	fn decompile_body(&mut self, fields: &[Field]) -> Result<()> {
		for f in fields {
			let mut body = String::new();
			match &f.repeat {
				Some(rep) => {
					let count = match rep {
						Repeat::Fixed(n) => format!("{n}usize"),
						Repeat::Local(ident, name, aux, true) => {
							format!("repeat_count(t.{ident}.map(|v| v as i64), {aux}, {name:?})?")
						}
						Repeat::Local(ident, name, aux, false) => {
							format!("repeat_count(Some(t.{ident} as i64), {aux}, {name:?})?")
						}
						Repeat::Propagated(name, aux) => format!(
							"repeat_count(r.get({name:?}).and_then(StateValue::as_int), {aux}, {name:?})?"
						),
					};
					if let Kind::DeltaValue = f.kind {
						let _ = write!(
							body,
							"t.{} = read_delta_value(r, t.start_size, t.end_size, t.delta_format)?;",
							f.ident
						);
					} else {
						let read = self.read_expr(f)?;
						let _ = write!(
							body,
							"{{ let n = {count}; let mut v = Vec::with_capacity(n.min(4096)); for _ in 0..n {{ v.push({read}); }} t.{} = v; }}",
							f.ident
						);
					}
				}
				None => {
					let read = self.read_expr(f)?;
					let value = match f.kind {
						Kind::Count(_) => format!("Some({read} as u32)"),
						_ => read,
					};
					let _ = write!(body, "t.{} = {value};", f.ident);
					if f.conv.is_propagated {
						let state = match f.kind {
							Kind::Tag => Some(format!("StateValue::Tag(Some(t.{}))", f.ident)),
							Kind::Count(_) => Some(format!(
								"StateValue::Int(t.{}.unwrap_or(0) as i64)",
								f.ident
							)),
							Kind::Int(..) | Kind::Version | Kind::Flags32 | Kind::NameId => {
								Some(format!("StateValue::Int(t.{} as i64)", f.ident))
							}
							// fontTools propagates `AxisIndicesList` as the table; the
							// composite glyphs only need the length of each item.
							Kind::Offset(..) if f.conv.name == "AxisIndicesList" => {
								let _ = write!(
									body,
									" if let Some(l) = t.{}.as_deref() {{ r.set({:?}, axis_indices_state(&l.item)); }}",
									f.ident, f.conv.name
								);
								None
							}
							_ => None,
						};
						if let Some(state) = state {
							let _ = write!(body, " r.set({:?}, {state});", f.conv.name);
						}
					}
				}
			}
			match &f.cond {
				Some(c) => {
					let _ = writeln!(self.out, "\t\tif {c} {{ {body} }}");
				}
				None => {
					let _ = writeln!(self.out, "\t\t{body}");
				}
			}
		}
		Ok(())
	}

	fn read_expr(&self, f: &Field) -> Result<String> {
		Ok(match &f.kind {
			Kind::Int(_, read, _) => format!("r.{read}()?"),
			Kind::Count(size) => match size {
				1 => "r.read_u8()?".into(),
				2 => "r.read_u16()?".into(),
				3 => "r.read_u24()?".into(),
				_ => "r.read_u32()?".into(),
			},
			Kind::Int24 => "r.read_i24()?".into(),
			Kind::Version | Kind::Flags32 | Kind::VarIndex => "r.read_u32()?".into(),
			Kind::NameId | Kind::StatFlags | Kind::LookupFlag => "r.read_u16()?".into(),
			Kind::ValueFormat => {
				let which = if f.conv.name.ends_with('2') { "ValueFormat2" } else { "ValueFormat1" };
				format!("{{ let v = r.read_u16()?; r.set({which:?}, StateValue::ValueFormat(v)); v }}")
			}
			Kind::Glyph(2) => "r.read_u16()? as u32".into(),
			Kind::Glyph(3) => "r.read_u24()?".into(),
			Kind::Glyph(_) => "r.read_u32()?".into(),
			Kind::Float(k) => format!("read_float(r, {k:?})?"),
			Kind::Tag => "r.read_tag()?".into(),
			Kind::Char64 => "read_char64(r)".into(),
			Kind::Enum(_) => "r.read_u8()?".into(),
			Kind::Struct(c) => format!("{}::decompile(r, font)?", rust_class(c, false)),
			Kind::Offset(c, size) => format!("read_offset::<{}>(r, font, {size})?", rust_class(c, false)),
			Kind::SubTable(size) => {
				let lt = if f.conv.name == "ExtSubTable" { "extension_lookup_type" } else { "lookup_type" };
				format!("LookupSubTable::read_offset(r, font, {size}, t.{lt})?")
			}
			Kind::FeatureParams => "FeatureParamsAny::read_offset(r, font)?".into(),
			Kind::FeatureVariations => {
				"{ set_feature_tags(r, font, t.feature_list.as_deref(), t.version)?; read_offset(r, font, 4)? }".into()
			}
			Kind::AlternateFeature(c) => format!(
				"{{ set_alternate_feature_tag(r, t.feature_index); read_offset::<{}>(r, font, 4)? }}",
				rust_class(c, false)
			),
			Kind::ValueRecord => {
				let which = if f.conv.name.ends_with('2') { "ValueFormat2" } else { "ValueFormat1" };
				format!("ValueRecord::read(r, font, {which:?})?")
			}
			Kind::DeltaValue => "read_delta_value(r, t.start_size, t.end_size, t.delta_format)?".into(),
			Kind::VarDataValue => "read_var_data_value(r, t.var_region_count, t.num_shorts)?".into(),
			Kind::TupleList(false) => "read_tuple_list(r)?".into(),
			Kind::TupleList(true) => "read_tuple_list_offset(r)?".into(),
			Kind::VarCompositeGlyphList => "read_var_composite_glyphs(r, font)?".into(),
			Kind::MappingEntries => "read_mapping_entries(r)?".into(),
			Kind::AatLookup(v) => format!("read_aat_lookup::<{v}>(r, font)?"),
			Kind::AatLookupWithDataOffset(c) => {
				format!("read_aat_lookup_with_data_offset::<{c}>(r, font)?")
			}
			Kind::StructWithLength(c) => {
				format!("read_struct_with_length::<{}>(r, font)?", rust_class(c, false))
			}
			Kind::CidGlyphMap => "read_cid_glyph_map(r)?".into(),
			Kind::GlyphCidMap => "read_glyph_cid_map(r, font)?".into(),
			Kind::MorphStateTable(k) => {
				format!("read_morph_state_table(ActionKind::{k}, r, font)?")
			}
			Kind::MortSubtable => "read_mort_subtable(r, font)?".into(),
			Kind::MorxSubtable => "read_morx_subtable(r, font)?".into(),
			Kind::Unsupported(name) => format!("Unsupported::read({name:?})?"),
		})
	}

	fn write_stmt(&self, f: &Field, value: &str, index: &str) -> Result<String> {
		Ok(match &f.kind {
			Kind::Int(_, _, "write_u24") => format!("w.write_u24({value})?;"),
			Kind::Int(_, _, write) => format!("w.{write}({value});"),
			Kind::Count(size) => match size {
				1 => format!(
					"w.write_u8(fit({value}.unwrap_or(0) as u64, {:?})?);",
					f.conv.name
				),
				2 => format!(
					"w.write_u16(fit({value}.unwrap_or(0) as u64, {:?})?);",
					f.conv.name
				),
				3 => format!("w.write_u24({value}.unwrap_or(0))?;"),
				_ => format!("w.write_u32({value}.unwrap_or(0));"),
			},
			Kind::Int24 => format!("write_i24(w, {value})?;"),
			Kind::Version => format!("w.write_u32(version_as_long({value})?);"),
			Kind::Flags32 | Kind::VarIndex => format!("w.write_u32({value});"),
			Kind::NameId | Kind::StatFlags | Kind::LookupFlag => format!("w.write_u16({value});"),
			Kind::ValueFormat => {
				let which = if f.conv.name.ends_with('2') {
					"ValueFormat2"
				} else {
					"ValueFormat1"
				};
				format!("w.write_u16({value}); w.set({which:?}, StateValue::ValueFormat({value}));")
			}
			Kind::Glyph(size) => format!("write_glyph(w, {value}, {size})?;"),
			Kind::Float(k) => format!("write_float(w, {value}, {k:?})?;"),
			Kind::Tag => format!("w.write_tag({value});"),
			Kind::Char64 => format!("write_char64(w, &{value});"),
			Kind::Enum(_) => format!("w.write_u8({value});"),
			Kind::Struct(_) => format!("{value}.compile(w, font)?;"),
			Kind::Offset(_, size) => format!(
				"write_offset(w, font, {value}.as_deref(), {size}, {:?}, {index})?;",
				f.conv.name
			),
			Kind::SubTable(size) => {
				let ext = if *size == 4 {
					"w.set_extension(); "
				} else {
					""
				};
				format!(
					"{ext}write_offset(w, font, {value}.as_deref(), {size}, {:?}, {index})?;",
					f.conv.name
				)
			}
			Kind::FeatureParams => format!(
				"write_offset(w, font, {value}.as_deref(), 2, {:?}, {index})?;",
				f.conv.name
			),
			Kind::FeatureVariations => format!(
				"if {value}.is_some() {{ set_writer_feature_tags(w, self.feature_list.as_deref(), self.version); }} write_offset(w, font, {value}.as_deref(), 4, {:?}, {index})?;",
				f.conv.name
			),
			Kind::AlternateFeature(_) => format!(
				"set_writer_alternate_feature_tag(w, {value}.as_deref().map(|f| f.feature_params.is_some()), self.feature_index)?; write_offset(w, font, {value}.as_deref(), 4, {:?}, {index})?;",
				f.conv.name
			),
			Kind::ValueRecord => {
				let which = if f.conv.name.ends_with('2') {
					"ValueFormat2"
				} else {
					"ValueFormat1"
				};
				format!("ValueRecord::write(w, font, {which:?}, {value}.as_ref())?;")
			}
			Kind::DeltaValue => format!(
				"write_delta_value(w, &{value}, self.start_size, self.end_size, self.delta_format)?;"
			),
			Kind::VarDataValue => format!(
				"write_var_data_value(w, &{value}, count_var_region_count.value(), self.num_shorts)?;"
			),
			Kind::TupleList(false) => format!("write_tuple_list(w, &{value})?;"),
			Kind::TupleList(true) => {
				format!("write_tuple_list_offset(w, &{value}, {:?})?;", f.conv.name)
			}
			Kind::VarCompositeGlyphList => format!("write_var_composite_glyphs(w, &{value})?;"),
			Kind::MappingEntries => format!("write_mapping_entries(w, &{value})?;"),
			Kind::AatLookup(_) => format!("write_aat_lookup(w, font, &{value})?;"),
			Kind::AatLookupWithDataOffset(_) => {
				format!("write_aat_lookup_with_data_offset(w, font, &{value})?;")
			}
			Kind::StructWithLength(_) => format!("write_struct_with_length(w, font, &{value})?;"),
			Kind::CidGlyphMap => format!("write_cid_glyph_map(w, &{value})?;"),
			Kind::GlyphCidMap => format!("write_glyph_cid_map(w, &{value})?;"),
			Kind::MorphStateTable(k) => {
				format!("write_morph_state_table(ActionKind::{k}, w, font, &{value})?;")
			}
			Kind::MortSubtable => format!("write_mort_subtable(w, font, &{value})?;"),
			Kind::MorxSubtable => format!("write_morx_subtable(w, font, &{value})?;"),
			Kind::Unsupported(name) => format!("Unsupported::write({name:?})?;"),
		})
	}

	fn compile_body(
		&mut self,
		class: &Class,
		fields: &[Field],
		format: Option<(u64, &str)>,
	) -> Result<()> {
		if let Some((array, glyph)) = sort_by_glyph(&class.name) {
			// `preWrite` sorts in place, with a stable sort.
			let (array, glyph) = (snake(array), snake(glyph));
			let _ = writeln!(
				self.out,
				"\t\tif !self.{array}.is_sorted_by_key(|r| r.{glyph}) {{ let mut c = self.clone(); c.{array}.sort_by_key(|r| r.{glyph}); return c.compile(w, font); }}"
			);
		}
		if class.attrs.contains_key("sortCoverageLast") {
			self.line("\t\tw.set_sort_coverage_last();");
		}
		if class.attrs.contains_key("DontShare") {
			self.line("\t\tw.set_dont_share();");
		}
		if let Some(lt) = class
			.attrs
			.get("LookupType")
			.and_then(serde_json::Value::as_u64)
		{
			let _ = writeln!(
				self.out,
				"\t\tw.count_ref(\"LookupType\")?.set_value({lt})?;"
			);
		}
		if let Some((fmt, kind)) = format {
			if kind == "FormatSwitching8" {
				let _ = writeln!(self.out, "\t\tw.write_u8({fmt});");
			} else {
				let _ = writeln!(self.out, "\t\tw.write_u16({fmt});");
			}
		}
		let pre = pre_count(&class.name);
		if let Some(name) = pre {
			let ident = snake(name);
			let _ = writeln!(
				self.out,
				"\t\tlet count_{ident} = CountRef::new({name:?}, 2, None); w.set({name:?}, StateValue::Count(count_{ident}.clone()));"
			);
		}
		// Bind the fields under `t` so that conditions read like in decompile.
		self.line("\t\tlet t = self;");
		self.line("\t\tlet _ = t;");
		for f in fields {
			let mut body = String::new();
			match (&f.repeat, &f.kind) {
				(Some(rep), kind) => {
					let len = format!("self.{}.len() as i64", f.ident);
					match rep {
						Repeat::Fixed(n) => {
							let _ = write!(
								body,
								"if self.{}.len() as i64 != {n} {{ return Err(Error::Assertion(format!(\"expected {n} values, got {{}}\", self.{}.len()))); }}",
								f.ident, f.ident
							);
						}
						Repeat::Local(ident, _, aux, true) => {
							let _ =
								write!(body, "count_{ident}.set_value(({len} - {aux}) as u64)?;");
						}
						Repeat::Local(ident, name, aux, false) => {
							let _ = write!(
								body,
								"if self.{ident} as i64 != {len} - {aux} {{ return Err(Error::Assertion(format!(\"({name}, {{}}, {{}})\", self.{ident}, {len} - {aux}))); }}"
							);
						}
						Repeat::Propagated(name, aux) => {
							let _ = write!(
								body,
								"w.count_ref({name:?})?.set_value(({len} - {aux}) as u64)?;"
							);
						}
					}
					if let Kind::DeltaValue = kind {
						body.push_str(&self.write_stmt(f, &format!("self.{}", f.ident), "None")?);
					} else {
						let stmt = self.write_stmt(f, "(*item)", "Some(i)")?;
						let stmt = stmt
							.replace("(*item).as_deref()", "item.as_deref()")
							.replace("(*item).as_ref()", "item.as_ref()");
						let lp = format!(
							"for (i, item) in self.{}.iter().enumerate() {{ let _ = i; {stmt} }}",
							f.ident
						);
						// Converters with their own `writeArray` write the whole array as
						// one item, even when empty; this matters for the sharing of
						// identical subtables.
						if ARRAY_CONVERTERS.contains(&f.conv.converter.as_str()) {
							let _ = write!(body, " w.write_array(|w| {{ {lp} Ok(()) }})?;");
						} else {
							let _ = write!(body, " {lp}");
						}
					}
				}
				(None, Kind::Count(size)) => {
					let _ = write!(body, "w.write_count_reference(count_{}.clone());", f.ident);
					if f.conv.is_propagated {
						let _ = write!(
							body,
							" w.set({:?}, StateValue::Count(count_{}.clone()));",
							f.conv.name, f.ident
						);
					}
					// Declared outside the condition so that arrays can refer to it.
					// `VarRegionList.preWrite` presets `RegionAxisCount`.
					let init = match (class.name.as_str(), f.conv.name.as_str()) {
						("VarRegionList", "RegionAxisCount") => {
							"region_axis_count(font, self.region_axis_count)?"
						}
						_ => "None",
					};
					if pre != Some(f.conv.name.as_str()) {
						let _ = writeln!(
							self.out,
							"\t\tlet count_{} = CountRef::new({:?}, {size}, {init});",
							f.ident, f.conv.name
						);
					}
				}
				(None, _) if f.conv.is_lookup_type => {
					let size = json_int(&f.conv.static_size).unwrap_or(2);
					let _ = write!(
						body,
						"{{ let lt = CountRef::new({:?}, {size}, Some(self.{} as u64)); w.write_count_reference(lt.clone()); w.set(\"LookupType\", StateValue::Count(lt)); }}",
						f.conv.name, f.ident
					);
				}
				(None, _) => {
					let value = match f.kind {
						Kind::Struct(_) => format!("self.{}", f.ident),
						_ => format!("self.{}", f.ident),
					};
					body.push_str(&self.write_stmt(f, &value, "None")?);
					if f.conv.is_propagated {
						let state = match f.kind {
							Kind::Tag => Some(format!("StateValue::Tag(Some(self.{}))", f.ident)),
							Kind::Int(..) | Kind::Version | Kind::Flags32 | Kind::NameId => {
								Some(format!("StateValue::Int(self.{} as i64)", f.ident))
							}
							_ => None,
						};
						if let Some(state) = state {
							let _ = write!(body, " w.set({:?}, {state});", f.conv.name);
						}
					}
				}
			}
			// UPSTREAM-BUG(B1): `BaseTable.compile` checks `aux` conditions only for plain values:
			// counts and lookup types are written even when their `aux` is false
			// (`LookupVariationCount` of `FeatureVariations` version 1.0).
			let cond = if f.repeat.is_none()
				&& (matches!(f.kind, Kind::Count(_)) || f.conv.is_lookup_type)
			{
				&f.enabled
			} else {
				&f.cond
			};
			match cond {
				Some(c) => {
					let _ = writeln!(self.out, "\t\tif {c} {{ {body} }}");
				}
				None => {
					let _ = writeln!(self.out, "\t\t{body}");
				}
			}
		}
		Ok(())
	}

	fn xml_write_stmt(&self, f: &Field, value: &str, attrs: &str) -> String {
		let name = &f.conv.name;
		match &f.kind {
			Kind::Int(..) | Kind::Int24 | Kind::ValueFormat => {
				format!("xml_simple(xw, {name:?}, &{attrs}, {value}.to_string());")
			}
			// Counts with a plain converter (`DesignAxisRecordSize`) show their value.
			Kind::Count(_) if !f.conv.converter.starts_with("Computed") => format!(
				"xml_simple(xw, {name:?}, &{attrs}, {value}.map_or(\"None\".to_owned(), |v| v.to_string()));"
			),
			Kind::Count(_) => format!("xml_computed(xw, {name:?}, {value});"),
			Kind::Version => {
				format!("xml_simple(xw, {name:?}, &{attrs}, format!(\"0x{{:08x}}\", {value}));")
			}
			Kind::Flags32 => {
				format!("xml_simple(xw, {name:?}, &{attrs}, format!(\"0x{{:08X}}\", {value}));")
			}
			Kind::VarIndex => format!("xml_var_index(xw, {name:?}, &{attrs}, {value});"),
			Kind::NameId => format!("xml_name_id(xw, font, {name:?}, &{attrs}, {value})?;"),
			Kind::StatFlags => format!("xml_stat_flags(xw, {name:?}, &{attrs}, {value});"),
			Kind::LookupFlag => format!("xml_lookup_flag(xw, {name:?}, &{attrs}, {value});"),
			Kind::Glyph(_) => {
				format!("xml_simple(xw, {name:?}, &{attrs}, font.glyph_name({value})?);")
			}
			Kind::Float(k) => {
				format!("xml_simple(xw, {name:?}, &{attrs}, float_to_string({value}, {k:?}));")
			}
			Kind::Tag => format!("xml_simple(xw, {name:?}, &{attrs}, {value}.to_string());"),
			Kind::Char64 => format!("xml_simple(xw, {name:?}, &{attrs}, {value}.clone());"),
			Kind::Enum(e) => {
				format!("xml_simple(xw, {name:?}, &{attrs}, enum_to_string({e:?}, {value})?);")
			}
			Kind::Struct(_) | Kind::StructWithLength(_) => {
				format!("xml_struct(xw, font, Some(&{value}), Some({name:?}), &{attrs})?;")
			}
			Kind::Offset(..) | Kind::FeatureVariations | Kind::AlternateFeature(_) => {
				format!("xml_struct(xw, font, {value}.as_deref(), Some({name:?}), &{attrs})?;")
			}
			Kind::SubTable(_) | Kind::FeatureParams => {
				format!("xml_struct(xw, font, {value}.as_deref(), None, &{attrs})?;")
			}
			Kind::ValueRecord => {
				format!("if let Some(v) = &{value} {{ v.to_xml(xw, font, {name:?}, &{attrs})?; }}")
			}
			Kind::DeltaValue | Kind::VarDataValue => {
				format!("xml_simple(xw, {name:?}, &{attrs}, int_list_repr(&{value}));")
			}
			Kind::TupleList(_) => format!("xml_tuple_list(xw, {name:?}, &{attrs}, &{value});"),
			Kind::VarCompositeGlyphList => {
				format!("xml_var_composite_glyphs(xw, font, &{value})?;")
			}
			Kind::MappingEntries => format!("xml_mapping_entries(xw, &{value});"),
			Kind::AatLookup(_) | Kind::AatLookupWithDataOffset(_) => {
				format!("xml_aat_lookup(xw, font, {name:?}, &{attrs}, &{value})?;")
			}
			Kind::CidGlyphMap => {
				format!("xml_cid_glyph_map(xw, font, {name:?}, &{attrs}, &{value})?;")
			}
			Kind::GlyphCidMap => {
				format!("xml_glyph_cid_map(xw, font, {name:?}, &{attrs}, &{value})?;")
			}
			Kind::MorphStateTable(_) => {
				format!("xml_morph_state_table(xw, font, {name:?}, &{attrs}, &{value})?;")
			}
			Kind::MortSubtable => {
				format!("xml_mort_subtable(xw, font, {name:?}, &{attrs}, &{value})?;")
			}
			Kind::MorxSubtable => {
				format!("xml_morx_subtable(xw, font, {name:?}, &{attrs}, &{value})?;")
			}
			Kind::Unsupported(_) => "let _ = &xw;".to_owned(),
		}
	}

	fn gen_to_xml2(&mut self, fields: &[Field]) {
		self.line("\t\tlet t = self;");
		self.line("\t\tlet _ = t;");
		for f in fields {
			let stmt = match (&f.repeat, &f.kind) {
				(Some(_), Kind::DeltaValue) => {
					self.xml_write_stmt(f, &format!("self.{}", f.ident), "Attrs::new()")
				}
				(Some(_), _) => {
					let item = match f.kind {
						Kind::Struct(_) | Kind::Char64 => "(*item)".to_owned(),
						_ => "(*item)".to_owned(),
					};
					let stmt = self.xml_write_stmt(f, &item, "attrs");
					let stmt = stmt
						.replace("(*item).as_deref()", "item.as_deref()")
						.replace("&(*item)", "item");
					format!(
						"for (i, item) in self.{}.iter().enumerate() {{ let attrs: Attrs = vec![(\"index\", i.to_string())]; let _ = &attrs; {stmt} }}",
						f.ident
					)
				}
				(None, _) => self.xml_write_stmt(f, &format!("self.{}", f.ident), "Attrs::new()"),
			};
			match &f.cond {
				Some(c) => {
					let _ = writeln!(self.out, "\t\tif {c} {{ {stmt} }}");
				}
				None => {
					let _ = writeln!(self.out, "\t\t{stmt}");
				}
			}
		}
	}

	fn xml_read_expr(&self, f: &Field) -> String {
		match &f.kind {
			// LENIENT(L9): values are range-checked when read, while fontTools
			// keeps any integer until `struct.pack` fails when compiling.
			Kind::Int(ty, ..) => format!("parse_int::<{ty}>(value_attr(e)?)?"),
			Kind::Int24 => "parse_int::<i32>(value_attr(e)?)?".into(),
			Kind::ValueFormat | Kind::NameId | Kind::StatFlags | Kind::LookupFlag => {
				"parse_int::<u16>(value_attr(e)?)?".into()
			}
			Kind::Count(_) => "Some(parse_int::<u32>(value_attr(e)?)?)".into(),
			Kind::Version => "parse_version(value_attr(e)?)?".into(),
			Kind::Flags32 => "parse_int::<u32>(value_attr(e)?)?".into(),
			Kind::VarIndex => "parse_var_index(e)?".into(),
			Kind::Glyph(_) => "font.glyph_id(value_attr(e)?)?".into(),
			Kind::Float(k) => format!("float_from_string(value_attr(e)?, {k:?})?"),
			Kind::Tag => "parse_tag(value_attr(e)?)?".into(),
			Kind::Char64 => "value_attr(e)?.to_owned()".into(),
			Kind::Enum(name) => format!("enum_from_string({name:?}, value_attr(e)?)?"),
			Kind::Struct(c) | Kind::StructWithLength(c) => format!(
				"xml_read_struct::<{}>(e, font, ctx)?.unwrap_or_default()",
				rust_class(c, false)
			),
			Kind::Offset(c, _) | Kind::AlternateFeature(c) => format!(
				"xml_read_struct::<{}>(e, font, ctx)?.map(Box::new)",
				rust_class(c, false)
			),
			Kind::FeatureVariations => {
				let c = f.conv.table_class.clone().unwrap_or_default();
				format!(
					"xml_read_struct::<{}>(e, font, ctx)?.map(Box::new)",
					rust_class(&c, false)
				)
			}
			Kind::SubTable(_) => {
				"LookupSubTable::xml_read_named(e, font, ctx)?.map(Box::new)".into()
			}
			Kind::FeatureParams => {
				"FeatureParamsAny::xml_read_named(e, font, ctx)?.map(Box::new)".into()
			}
			Kind::ValueRecord => "Some(ValueRecord::from_xml(e, font)?)".into(),
			Kind::DeltaValue | Kind::VarDataValue => "parse_int_list(value_attr(e)?)?".into(),
			Kind::TupleList(_) => "parse_tuple(e)?".into(),
			Kind::VarCompositeGlyphList => "VarCompositeGlyph::from_xml(e, font)?".into(),
			// UPSTREAM-BUG(B9): `xmlWrite` writes the entries without an element
			// of their own, while `xmlRead` expects them in one named after the
			// field, so `entry` elements fail with KeyError.
			Kind::MappingEntries => "read_mapping_entries_xml(e)?".into(),
			Kind::AatLookup(v) => format!("read_aat_lookup_xml::<{v}>(e, font, ctx)?"),
			Kind::AatLookupWithDataOffset(c) => {
				format!("read_aat_lookup_xml::<Option<Box<{c}>>>(e, font, ctx)?")
			}
			Kind::CidGlyphMap => "read_cid_glyph_map_xml(e, font)?".into(),
			Kind::GlyphCidMap => "read_glyph_cid_map_xml(e, font)?".into(),
			Kind::MorphStateTable(k) => {
				format!("read_morph_state_table_xml(ActionKind::{k}, e, font)?")
			}
			Kind::MortSubtable => "read_mort_subtable_xml(e, font)?".into(),
			Kind::MorxSubtable => "read_morx_subtable_xml(e, font)?".into(),
			Kind::Unsupported(name) => format!("Unsupported::xml_read({name:?})?"),
		}
	}

	fn gen_from_xml(&mut self, fields: &[Field]) {
		self.line("\t\tlet e = element;");
		if fields.is_empty() {
			self.line("\t\tlet _ = (font, ctx);");
			self.line("\t\tErr(Error::ttx(format!(\"KeyError: {:?}\", e.name)))");
			return;
		}
		self.line("\t\tmatch e.name.as_str() {");
		for f in fields {
			let read = self.xml_read_expr(f);
			let names = match f.kind {
				Kind::SubTable(_) => self.lookup_class_names(),
				Kind::FeatureParams => self.feature_param_class_names(),
				_ => vec![f.conv.name.clone()],
			};
			let pat = names
				.iter()
				.map(|n| format!("{n:?}"))
				.collect::<Vec<_>>()
				.join(" | ");
			// CFF2 INDEX fields (repeat "") append an item for each element.
			let appends = matches!(f.kind, Kind::TupleList(_) | Kind::VarCompositeGlyphList);
			if appends || (f.repeat.is_some() && !matches!(f.kind, Kind::DeltaValue)) {
				let _ = writeln!(self.out, "\t\t\t{pat} => self.{}.push({read}),", f.ident);
			} else {
				let _ = writeln!(self.out, "\t\t\t{pat} => self.{} = {read},", f.ident);
			}
		}
		self.line("\t\t\tother => return Err(Error::ttx(format!(\"KeyError: {other:?}\"))),");
		self.line("\t\t}");
		self.line("\t\tlet _ = (font, ctx);");
		self.line("\t\tOk(())");
	}

	fn lookup_class_names(&self) -> Vec<String> {
		let set: BTreeSet<String> = self
			.ot
			.lookup_types
			.values()
			.flat_map(|m| m.values().cloned())
			.collect();
		set.into_iter().collect()
	}

	fn feature_param_class_names(&self) -> Vec<String> {
		let mut set: BTreeSet<String> = self.ot.feature_param_types.values().cloned().collect();
		set.insert("FeatureParams".into());
		set.into_iter().collect()
	}

	fn populate_body(&mut self, fields: &[Field]) {
		self.line("\t\tlet t = &*self;");
		self.line("\t\tlet _ = t;");
		for f in fields {
			let stmt = match &f.repeat {
				Some(Repeat::Local(ident, _, aux, true)) if !matches!(f.kind, Kind::DeltaValue) => {
					format!(
						"self.{ident} = Some((self.{}.len() as i64 - {aux}).max(0) as u32);",
						f.ident
					)
				}
				Some(Repeat::Local(ident, name, aux, false))
					if !matches!(f.kind, Kind::DeltaValue) =>
				{
					format!(
						"self.{ident} = fit((self.{}.len() as i64 - {aux}).max(0) as u64, {name:?})?;",
						f.ident
					)
				}
				Some(Repeat::Propagated(name, aux)) if !matches!(f.kind, Kind::DeltaValue) => {
					format!(
						"ctx.set_count({name:?}, (self.{}.len() as i64 - {aux}).max(0) as u32)?;",
						f.ident
					)
				}
				_ => continue,
			};
			match &f.cond {
				Some(c) => {
					let c = c.replace("t.", "self.");
					let _ = writeln!(self.out, "\t\tif {c} {{ {stmt} }}");
				}
				None => {
					let _ = writeln!(self.out, "\t\t{stmt}");
				}
			}
		}
		self.line("\t\tlet _ = ctx;");
		self.line("\t\tOk(())");
	}

	/// `remap_glyphs`: maps every glyph ID the table holds, recursively.
	fn remap_body(&mut self, fields: &[Field]) {
		let mut any = false;
		for f in fields {
			let stmt = match &f.kind {
				Kind::Glyph(_) => "*v = f(*v)?;",
				Kind::Struct(_)
				| Kind::StructWithLength(_)
				| Kind::MorphStateTable(_)
				| Kind::MortSubtable
				| Kind::MorxSubtable => "v.remap_glyphs(&mut *f)?;",
				Kind::Offset(..)
				| Kind::SubTable(_)
				| Kind::FeatureParams
				| Kind::FeatureVariations
				| Kind::AlternateFeature(_) => "if let Some(x) = v { x.remap_glyphs(&mut *f)?; }",
				Kind::AatLookup(_) | Kind::AatLookupWithDataOffset(_) => {
					"remap_aat_lookup(v, &mut *f)?;"
				}
				Kind::CidGlyphMap => "for g in v.values_mut() { *g = f(*g)?; }",
				Kind::GlyphCidMap => "*v = remap_keys(std::mem::take(v), &mut *f)?;",
				Kind::VarCompositeGlyphList => "for g in v.iter_mut() { g.remap_glyphs(&mut *f)?; }",
				_ => continue,
			};
			any = true;
			let body = if f.repeat.is_some() {
				format!("for v in self.{}.iter_mut() {{ {stmt} }}", f.ident)
			} else {
				format!("{{ let v = &mut self.{}; {stmt} }}", f.ident)
			};
			match &f.cond {
				Some(c) => {
					let c = c.replace("t.", "self.");
					let _ = writeln!(self.out, "\t\tif {c} {{ {body} }}");
				}
				None => {
					let _ = writeln!(self.out, "\t\t{body}");
				}
			}
		}
		if !any {
			self.line("\t\tlet _ = f;");
		}
		self.line("\t\tOk(())");
	}

	fn propagated(&mut self, fields: &[Field]) {
		let names: Vec<&Field> = fields
			.iter()
			.filter(|f| f.conv.is_propagated && matches!(f.kind, Kind::Count(_)))
			.collect();
		if names.is_empty() {
			return;
		}
		let list: Vec<String> = names.iter().map(|f| format!("{:?}", f.conv.name)).collect();
		let _ = writeln!(
			self.out,
			"\tfn propagated_counts(&self) -> &'static [&'static str] {{ &[{}] }}",
			list.join(", ")
		);
		self.line("\tfn set_propagated_count(&mut self, name: &str, value: Option<u32>) {");
		self.line("\t\tmatch name {");
		for f in names {
			let _ = writeln!(
				self.out,
				"\t\t\t{:?} => if value.is_some() {{ self.{} = value; }},",
				f.conv.name, f.ident
			);
		}
		self.line("\t\t\t_ => {}");
		self.line("\t\t}");
		self.line("\t}");
	}

	/// A struct with all `OtTable` methods (for a plain class or one format).
	fn plain_class(
		&mut self,
		class: &Class,
		ty: &str,
		convs: &[Conv],
		format: Option<(u64, &str)>,
		public: bool,
	) -> Result<()> {
		let fields = fields_of(convs)?;
		let doc = match format {
			Some((f, _)) => format!("`{}` format {f}.", class.name),
			None => format!("`{}`.", class.name),
		};
		let dont_share = public && self.is_layout_subtable(&class.name);
		self.struct_def(ty, &fields, &doc, public, dont_share);
		let _ = writeln!(self.out, "impl OtTable for {ty} {{");
		let _ = writeln!(
			self.out,
			"\tfn class_name(&self) -> &'static str {{ {:?} }}",
			class.name
		);
		self.line("\tfn decompile(r: &mut OtReader<'_>, font: &mut Font) -> Result<Self> {");
		self.line("\t\tlet mut t = Self::default();");
		self.line("\t\tlet _ = (&mut *r, &mut *font);");
		if let Some(name) = pre_count(&class.name) {
			// `COLR.decompile` reads the count first: it follows the array it counts.
			let mut offset = 0;
			let mut size = None;
			for c in convs {
				let s = json_int(&c.static_size)
					.ok_or_else(|| anyhow::anyhow!("{} has no static size", c.name))?;
				if c.name == name {
					size = Some(s);
					break;
				}
				offset += s;
			}
			let read = match size {
				Some(2) => "read_u16",
				_ => bail!("{name} not found in {}", class.name),
			};
			let _ = writeln!(
				self.out,
				"\t\t{{ let mut sub = r.sub_reader(0); sub.advance({offset}); let n = sub.{read}()?; r.set({name:?}, StateValue::Int(n as i64)); }}"
			);
		}
		self.decompile_body(&fields)?;
		self.line("\t\tOk(t)");
		self.line("\t}");
		self.line("\tfn compile(&self, w: &mut OtWriter<'_>, font: &mut Font) -> Result<()> {");
		self.line("\t\tlet _ = (&mut *w, &mut *font);");
		if dont_share {
			self.line("\t\tif self.dont_share { w.set_dont_share(); }");
		}
		self.compile_body(class, &fields, format)?;
		self.line("\t\tOk(())");
		self.line("\t}");
		if dont_share {
			self.line("\tfn dont_share(&self) -> bool { self.dont_share }");
			self.line("\tfn set_dont_share(&mut self) { self.dont_share = true; }");
		}
		// `StructWithLength` patches this field after writing the struct.
		if let Some(i) = fields.iter().position(|f| f.conv.name == "StructLength") {
			let f = &fields[i];
			let size = json_int(&f.conv.static_size).unwrap_or(4);
			// The format of a format-switching table is an item of its own.
			let item = i + usize::from(format.is_some());
			let _ = writeln!(
				self.out,
				"\tfn struct_length(&self) -> Option<StructLength> {{ Some(StructLength {{ value: self.{}, item: {item}, size: {size} }}) }}",
				f.ident
			);
		}
		// `toXML` writes a `Format` attribute for any table that has one, also
		// for plain classes with a `Format` field (`VarStore`).
		let format_field = fields.iter().find(|f| {
			f.conv.name == "Format" && f.cond.is_none() && matches!(f.kind, Kind::Int(..))
		});
		if format.is_none()
			&& let Some(ff) = format_field
		{
			let _ = writeln!(
				self.out,
				"\tfn format(&self) -> Option<u16> {{ Some(self.{} as u16) }}",
				ff.ident
			);
			let _ = writeln!(
				self.out,
				"\tfn with_format(format: u16) -> Self {{ Self {{ {}: format as _, ..Self::default() }} }}",
				ff.ident
			);
		}
		if let Some((f, _)) = format {
			let _ = writeln!(
				self.out,
				"\tfn format(&self) -> Option<u16> {{ Some({f}) }}"
			);
		}
		self.line("\tfn to_xml2(&self, xw: &mut XmlWriter, font: &mut Font) -> Result<()> {");
		self.line("\t\tlet _ = (&mut *xw, &mut *font);");
		self.gen_to_xml2(&fields);
		self.line("\t\tOk(())");
		self.line("\t}");
		self.line(
			"\tfn from_xml(&mut self, element: &Element, font: &mut Font, ctx: &mut XmlCtx) -> Result<()> {",
		);
		self.gen_from_xml(&fields);
		self.line("\t}");
		self.line("\tfn populate_defaults(&mut self, ctx: &mut XmlCtx) -> Result<()> {");
		self.populate_body(&fields);
		self.line("\t}");
		self.line("\tfn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {");
		self.remap_body(&fields);
		self.line("\t}");
		self.propagated(&fields);
		self.line("}");
		self.line("");
		Ok(())
	}

	fn format_switching_class(&mut self, class: &Class, raw: bool) -> Result<()> {
		let ty = rust_class(&class.name, raw);
		let vis = "pub";
		let mut variants = Vec::new();
		for (fmt, convs) in &class.formats {
			let fmt: u64 = fmt.parse()?;
			let vty = format!("{ty}Format{fmt}");
			self.plain_class(class, &vty, convs, Some((fmt, &class.kind)), !raw)?;
			variants.push((fmt, vty));
		}
		let read_format = if class.kind == "FormatSwitching8" {
			"r.read_u8()? as u16"
		} else {
			"r.read_u16()?"
		};
		let _ = writeln!(self.out, "/// `{}` (format-switching).", class.name);
		self.line("#[derive(Debug, Clone, PartialEq)]");
		let _ = writeln!(self.out, "{vis} enum {ty} {{");
		for (fmt, vty) in &variants {
			let _ = writeln!(self.out, "\t/// Format {fmt}.");
			let _ = writeln!(self.out, "\tFormat{fmt}({vty}),");
		}
		self.line("\t/// A format fontTools does not know; it has no fields.");
		self.line("\tUnknown(u16),");
		self.line("}");
		self.line("");
		let first = variants.first().map(|(f, _)| *f).unwrap_or(1);
		let _ = writeln!(self.out, "impl Default for {ty} {{");
		let _ = writeln!(
			self.out,
			"\tfn default() -> Self {{ Self::with_format({first}) }}"
		);
		self.line("}");
		self.line("");
		let _ = writeln!(self.out, "impl OtTable for {ty} {{");
		let _ = writeln!(
			self.out,
			"\tfn class_name(&self) -> &'static str {{ {:?} }}",
			class.name
		);
		self.line("\tfn decompile(r: &mut OtReader<'_>, font: &mut Font) -> Result<Self> {");
		let _ = writeln!(self.out, "\t\tlet format = {read_format};");
		self.line("\t\tOk(match format {");
		for (fmt, vty) in &variants {
			let _ = writeln!(
				self.out,
				"\t\t\t{fmt} => Self::Format{fmt}({vty}::decompile(r, font)?),"
			);
		}
		self.line("\t\t\tf => Self::Unknown(f),");
		self.line("\t\t})");
		self.line("\t}");
		self.line("\tfn compile(&self, w: &mut OtWriter<'_>, font: &mut Font) -> Result<()> {");
		self.line("\t\tmatch self {");
		for (fmt, _) in &variants {
			let _ = writeln!(
				self.out,
				"\t\t\tSelf::Format{fmt}(v) => v.compile(w, font),"
			);
		}
		let unknown_write = if class.kind == "FormatSwitching8" {
			"w.write_u8(*f as u8)"
		} else {
			"w.write_u16(*f)"
		};
		let _ = writeln!(
			self.out,
			"\t\t\tSelf::Unknown(f) => {{ {unknown_write}; Ok(()) }}"
		);
		self.line("\t\t}");
		self.line("\t}");
		self.line("\tfn format(&self) -> Option<u16> {");
		self.line("\t\tSome(match self {");
		for (fmt, _) in &variants {
			let _ = writeln!(self.out, "\t\t\tSelf::Format{fmt}(_) => {fmt},");
		}
		self.line("\t\t\tSelf::Unknown(f) => *f,");
		self.line("\t\t})");
		self.line("\t}");
		if class.name == "Paint" {
			// `Paint.toXML` adds the format name as a comment.
			self.line("\tfn to_xml(&self, xw: &mut XmlWriter, font: &mut Font, attrs: &[(&'static str, String)], name: &str) -> Result<()> {");
			self.line("\t\tpaint_to_xml(self, xw, font, attrs, name)");
			self.line("\t}");
		}
		for (method, args, call) in [
			(
				"to_xml2",
				"&self, xw: &mut XmlWriter, font: &mut Font",
				"to_xml2(xw, font)",
			),
			(
				"from_xml",
				"&mut self, element: &Element, font: &mut Font, ctx: &mut XmlCtx",
				"from_xml(element, font, ctx)",
			),
			(
				"populate_defaults",
				"&mut self, ctx: &mut XmlCtx",
				"populate_defaults(ctx)",
			),
			(
				"remap_glyphs",
				"&mut self, f: &mut dyn FnMut(u32) -> Result<u32>",
				"remap_glyphs(f)",
			),
		] {
			let _ = writeln!(self.out, "\tfn {method}({args}) -> Result<()> {{");
			self.line("\t\tmatch self {");
			for (fmt, _) in &variants {
				let _ = writeln!(self.out, "\t\t\tSelf::Format{fmt}(v) => v.{call},");
			}
			if method == "from_xml" {
				self.line(
					"\t\t\tSelf::Unknown(_) => Err(Error::ttx(format!(\"KeyError: {:?}\", element.name))),",
				);
			} else {
				self.line("\t\t\tSelf::Unknown(_) => Ok(()),");
			}
			self.line("\t\t}");
			self.line("\t}");
		}
		if !raw && self.is_layout_subtable(&class.name) {
			self.line("\tfn dont_share(&self) -> bool {");
			self.line("\t\tmatch self {");
			for (fmt, _) in &variants {
				let _ = writeln!(self.out, "\t\t\tSelf::Format{fmt}(v) => v.dont_share(),");
			}
			self.line("\t\t\tSelf::Unknown(_) => false,");
			self.line("\t\t}");
			self.line("\t}");
			self.line("\tfn set_dont_share(&mut self) {");
			self.line("\t\tmatch self {");
			for (fmt, _) in &variants {
				let _ = writeln!(
					self.out,
					"\t\t\tSelf::Format{fmt}(v) => v.set_dont_share(),"
				);
			}
			self.line("\t\t\tSelf::Unknown(_) => {}");
			self.line("\t\t}");
			self.line("\t}");
		}
		self.line("\tfn struct_length(&self) -> Option<StructLength> {");
		self.line("\t\tmatch self {");
		for (fmt, _) in &variants {
			let _ = writeln!(self.out, "\t\t\tSelf::Format{fmt}(v) => v.struct_length(),");
		}
		self.line("\t\t\tSelf::Unknown(_) => None,");
		self.line("\t\t}");
		self.line("\t}");
		self.line("\tfn propagated_counts(&self) -> &'static [&'static str] {");
		self.line("\t\tmatch self {");
		for (fmt, _) in &variants {
			let _ = writeln!(
				self.out,
				"\t\t\tSelf::Format{fmt}(v) => v.propagated_counts(),"
			);
		}
		self.line("\t\t\tSelf::Unknown(_) => &[],");
		self.line("\t\t}");
		self.line("\t}");
		self.line("\tfn set_propagated_count(&mut self, name: &str, value: Option<u32>) {");
		self.line("\t\tmatch self {");
		for (fmt, _) in &variants {
			let _ = writeln!(
				self.out,
				"\t\t\tSelf::Format{fmt}(v) => v.set_propagated_count(name, value),"
			);
		}
		self.line("\t\t\tSelf::Unknown(_) => {}");
		self.line("\t\t}");
		self.line("\t}");
		self.line("\tfn with_format(format: u16) -> Self {");
		self.line("\t\tmatch format {");
		for (fmt, vty) in &variants {
			let _ = writeln!(
				self.out,
				"\t\t\t{fmt} => Self::Format{fmt}({vty}::default()),"
			);
		}
		self.line("\t\t\tf => Self::Unknown(f),");
		self.line("\t\t}");
		self.line("\t}");
		self.line("}");
		self.line("");
		Ok(())
	}

	/// Whether a class is a `GSUB` or `GPOS` lookup subtable, which fontTools may
	/// mark `DontShare` when resolving overflows.
	fn is_layout_subtable(&self, class: &str) -> bool {
		["GSUB", "GPOS"].iter().any(|t| {
			self.ot
				.lookup_types
				.get(*t)
				.is_some_and(|m| m.values().any(|c| c == class))
		})
	}

	fn lookup_sub_table_enum(&mut self) {
		let names = self.lookup_class_names();
		self.line("/// A lookup subtable of any lookup type of any table (`SubTable` /");
		self.line("/// `ExtSubTable`), dispatched on the table tag and the lookup type.");
		self.line("#[derive(Debug, Clone, PartialEq)]");
		self.line("pub enum LookupSubTable {");
		for n in &names {
			let _ = writeln!(self.out, "\t/// `{n}`.");
			let _ = writeln!(self.out, "\t{}({}),", type_name(n), type_name(n));
		}
		self.line("}");
		self.line("");
		self.line("impl Default for LookupSubTable {");
		let _ = writeln!(
			self.out,
			"\tfn default() -> Self {{ Self::{}(Default::default()) }}",
			type_name(&names[0])
		);
		self.line("}");
		self.line("");
		self.line("impl LookupSubTable {");
		self.line("\t/// The class for a lookup type of a table (`lookupTypes[tag][type]`).");
		self.line("\tpub fn decompile_for(r: &mut OtReader<'_>, font: &mut Font, lookup_type: u16) -> Result<Self> {");
		self.line("\t\tOk(match (&r.table_tag.0, lookup_type) {");
		for (tag, types) in &self.ot.lookup_types {
			for (t, class) in types {
				let _ = writeln!(
					self.out,
					"\t\t\t(b{tag:?}, {t}) => Self::{}({}::decompile(r, font)?),",
					type_name(class),
					type_name(class)
				);
			}
		}
		self.line("\t\t\t(tag, t) => return Err(Error::ttlib(format!(\"KeyError: lookup type {t} of {}\", String::from_utf8_lossy(tag)))),");
		self.line("\t\t})");
		self.line("\t}");
		self.line("\t/// Reads an offset to a subtable (`SubTable.read`).");
		self.line("\tpub fn read_offset(r: &mut OtReader<'_>, font: &mut Font, size: usize, lookup_type: u16) -> Result<Option<Box<Self>>> {");
		self.line(
			"\t\tlet offset = match size { 2 => r.read_u16()? as usize, _ => r.read_u32()? as usize };",
		);
		self.line("\t\tif offset == 0 { return Ok(None); }");
		self.line("\t\tlet mut sub = r.sub_reader(offset);");
		self.line("\t\tOk(Some(Box::new(Self::decompile_for(&mut sub, font, lookup_type)?)))");
		self.line("\t}");
		self.line("\t/// Reads a subtable element named after its class.");
		self.line("\tpub fn xml_read_named(e: &Element, font: &mut Font, ctx: &mut XmlCtx) -> Result<Option<Self>> {");
		self.line("\t\tOk(match e.name.as_str() {");
		for n in &names {
			let _ = writeln!(
				self.out,
				"\t\t\t{n:?} => xml_read_struct::<{}>(e, font, ctx)?.map(Self::{}),",
				type_name(n),
				type_name(n)
			);
		}
		self.line("\t\t\tother => return Err(Error::ttx(format!(\"KeyError: {other:?}\"))),");
		self.line("\t\t})");
		self.line("\t}");
		self.line("}");
		self.line("");
		self.dispatch_impl("LookupSubTable", &names, false);
	}

	fn feature_params_enum(&mut self) {
		let names = self.feature_param_class_names();
		self.line("/// Feature parameters of any kind (`FeatureParams`), dispatched on the");
		self.line("/// feature tag.");
		self.line("#[derive(Debug, Clone, PartialEq)]");
		self.line("pub enum FeatureParamsAny {");
		for n in &names {
			let _ = writeln!(self.out, "\t/// `{n}`.");
			let _ = writeln!(self.out, "\t{}({}),", type_name(n), type_name(n));
		}
		self.line("}");
		self.line("");
		self.line("impl Default for FeatureParamsAny {");
		self.line("\tfn default() -> Self { Self::FeatureParams(Default::default()) }");
		self.line("}");
		self.line("");
		self.line("impl FeatureParamsAny {");
		self.line(
			"\t/// The class for a feature tag (`featureParamTypes.get(tag, FeatureParams)`).",
		);
		self.line("\tpub fn class_for(tag: Option<Tag>) -> &'static str {");
		self.line("\t\tmatch tag.map(|t| t.0) {");
		for (tag, class) in &self.ot.feature_param_types {
			let _ = writeln!(self.out, "\t\t\tSome(t) if &t == b{tag:?} => {class:?},");
		}
		self.line("\t\t\t_ => \"FeatureParams\",");
		self.line("\t\t}");
		self.line("\t}");
		self.line("\t/// Reads an offset to feature parameters (`FeatureParams.read`).");
		self.line(
			"\tpub fn read_offset(r: &mut OtReader<'_>, font: &mut Font) -> Result<Option<Box<Self>>> {",
		);
		self.line("\t\tlet tag = match r.get(\"FeatureTag\") { Some(StateValue::Tag(t)) => *t, _ => return Err(Error::ttlib(\"KeyError: 'FeatureTag'\")) };");
		self.line("\t\tlet offset = r.read_u16()? as usize;");
		self.line("\t\tif offset == 0 { return Ok(None); }");
		self.line("\t\tlet mut sub = r.sub_reader(offset);");
		self.line("\t\tOk(Some(Box::new(match Self::class_for(tag) {");
		for n in &names {
			let _ = writeln!(
				self.out,
				"\t\t\t{n:?} => Self::{}({}::decompile(&mut sub, font)?),",
				type_name(n),
				type_name(n)
			);
		}
		self.line("\t\t\t_ => unreachable!(),");
		self.line("\t\t})))");
		self.line("\t}");
		self.line("\t/// Reads a feature parameters element named after its class.");
		self.line("\tpub fn xml_read_named(e: &Element, font: &mut Font, ctx: &mut XmlCtx) -> Result<Option<Self>> {");
		self.line("\t\tOk(match e.name.as_str() {");
		for n in &names {
			let _ = writeln!(
				self.out,
				"\t\t\t{n:?} => xml_read_struct::<{}>(e, font, ctx)?.map(Self::{}),",
				type_name(n),
				type_name(n)
			);
		}
		self.line("\t\t\tother => return Err(Error::ttx(format!(\"KeyError: {other:?}\"))),");
		self.line("\t\t})");
		self.line("\t}");
		self.line("}");
		self.line("");
		self.dispatch_impl("FeatureParamsAny", &names, true);
	}

	/// `OtTable` for an enum of classes, delegating to the variant.
	fn dispatch_impl(&mut self, ty: &str, names: &[String], feature_params: bool) {
		let _ = writeln!(self.out, "impl OtTable for {ty} {{");
		self.line("\tfn class_name(&self) -> &'static str {");
		self.line("\t\tmatch self {");
		for n in names {
			let _ = writeln!(
				self.out,
				"\t\t\tSelf::{}(v) => v.class_name(),",
				type_name(n)
			);
		}
		self.line("\t\t}");
		self.line("\t}");
		self.line("\tfn decompile(_r: &mut OtReader<'_>, _font: &mut Font) -> Result<Self> {");
		let _ = writeln!(
			self.out,
			"\t\tErr(Error::ttlib(\"{ty} needs a dispatch context\"))"
		);
		self.line("\t}");
		self.line("\tfn compile(&self, w: &mut OtWriter<'_>, font: &mut Font) -> Result<()> {");
		if feature_params {
			self.line(
				"\t\tlet tag = match w.get(\"FeatureTag\") { Some(StateValue::Tag(t)) => *t, _ => None };",
			);
			self.line("\t\tif Self::class_for(tag) != self.class_name() {");
			self.line("\t\t\treturn Err(Error::Assertion(format!(\"Wrong FeatureParams type for feature '{}': {}\", tag.map(|t| t.to_string()).unwrap_or_default(), self.class_name())));");
			self.line("\t\t}");
		}
		self.line("\t\tmatch self {");
		for n in names {
			let _ = writeln!(
				self.out,
				"\t\t\tSelf::{}(v) => v.compile(w, font),",
				type_name(n)
			);
		}
		self.line("\t\t}");
		self.line("\t}");
		self.line("\tfn format(&self) -> Option<u16> {");
		self.line("\t\tmatch self {");
		for n in names {
			let _ = writeln!(self.out, "\t\t\tSelf::{}(v) => v.format(),", type_name(n));
		}
		self.line("\t\t}");
		self.line("\t}");
		for (method, args, ret) in [
			("dont_share", "&self", " -> bool"),
			("set_dont_share", "&mut self", ""),
		] {
			let _ = writeln!(self.out, "\tfn {method}({args}){ret} {{");
			self.line("\t\tmatch self {");
			for n in names {
				let _ = writeln!(self.out, "\t\t\tSelf::{}(v) => v.{method}(),", type_name(n));
			}
			self.line("\t\t}");
			self.line("\t}");
		}
		if feature_params {
			// FeatureParams.toXML always uses the class name.
			self.line("\tfn to_xml(&self, xw: &mut XmlWriter, font: &mut Font, attrs: &[(&'static str, String)], _name: &str) -> Result<()> {");
			self.line("\t\tmatch self {");
			for n in names {
				let _ = writeln!(
					self.out,
					"\t\t\tSelf::{}(v) => v.to_xml(xw, font, attrs, {n:?}),",
					type_name(n)
				);
			}
			self.line("\t\t}");
			self.line("\t}");
		} else {
			self.line("\tfn to_xml(&self, xw: &mut XmlWriter, font: &mut Font, attrs: &[(&'static str, String)], name: &str) -> Result<()> {");
			self.line("\t\tmatch self {");
			for n in names {
				let _ = writeln!(
					self.out,
					"\t\t\tSelf::{}(v) => v.to_xml(xw, font, attrs, name),",
					type_name(n)
				);
			}
			self.line("\t\t}");
			self.line("\t}");
		}
		for (method, args, call) in [
			(
				"to_xml2",
				"&self, xw: &mut XmlWriter, font: &mut Font",
				"to_xml2(xw, font)",
			),
			(
				"from_xml",
				"&mut self, element: &Element, font: &mut Font, ctx: &mut XmlCtx",
				"from_xml(element, font, ctx)",
			),
			(
				"populate_defaults",
				"&mut self, ctx: &mut XmlCtx",
				"populate_defaults(ctx)",
			),
			(
				"remap_glyphs",
				"&mut self, f: &mut dyn FnMut(u32) -> Result<u32>",
				"remap_glyphs(f)",
			),
		] {
			let _ = writeln!(self.out, "\tfn {method}({args}) -> Result<()> {{");
			self.line("\t\tmatch self {");
			for n in names {
				let _ = writeln!(self.out, "\t\t\tSelf::{}(v) => v.{call},", type_name(n));
			}
			self.line("\t\t}");
			self.line("\t}");
		}
		self.line("}");
		self.line("");
	}
}

/// Generates `generated/ot.rs`.
pub fn generate(ot: &OtTables) -> Result<String> {
	let mut g = Gen {
		out: String::new(),
		ot,
		classes: ot
			.classes
			.iter()
			.map(|c| (c.name.clone(), c.clone()))
			.collect(),
	};
	g.out.push_str(crate::codegen::HEADER);
	g.line("//! OpenType Layout and related types, generated from fontTools' otTables");
	g.line("//! classes (`fontTools.ttLib.tables.otData`, as built by `otConverters`).");
	g.line("");
	g.line("#![allow(clippy::all, unused_braces, unused_parens, unused_variables, unused_mut)]");
	g.line("");
	g.line("use fontoxide_misc::Tag;");
	g.line("use fontoxide_misc::xml_reader::Element;");
	g.line("use fontoxide_misc::xml_writer::XmlWriter;");
	g.line("");
	g.line("use crate::ot::conv::*;");
	g.line("use crate::ot::custom::*;");
	g.line("use crate::ot::varc::*;");
	g.line("use crate::ot::colr::*;");
	g.line("use crate::ot::ift::*;");
	g.line("use crate::ot::aat::*;");
	g.line("use crate::ot::morph::*;");
	g.line("use crate::ot::table::*;");
	g.line("use crate::ot::{CountRef, OtReader, OtWriter, StateValue};");
	g.line("use crate::{Error, Font, Result};");
	g.line("");
	let classes: Vec<Class> = g.classes.values().cloned().collect();
	for class in &classes {
		let raw = CUSTOM.contains(&class.name.as_str());
		let _ = &class.hooks;
		let _ = &class.inherited_hooks;
		match class.kind.as_str() {
			"BaseTable" => {
				let ty = rust_class(&class.name, raw);
				let convs = class.converters.clone();
				g.plain_class(class, &ty, &convs, None, !raw)?;
			}
			_ => g.format_switching_class(class, raw)?,
		}
	}
	g.lookup_sub_table_enum();
	g.feature_params_enum();
	Ok(g.out)
}
