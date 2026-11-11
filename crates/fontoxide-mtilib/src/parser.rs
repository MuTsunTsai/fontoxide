//! The parser: everything of `fontTools.mtiLib` but the tokenizer and the
//! contextual subtables.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use fontoxide_misc::{Tag, py};
use fontoxide_otllib::GlyphMap;
use fontoxide_otllib::builder::{self as otl, CaretCoord, MaybeExtended};
use fontoxide_ttlib::glyph_order::make_glyph_name;
use fontoxide_ttlib::ot::conv::ValueRecord;
use fontoxide_ttlib::ot::types::{self as ot, Anchor, ClassDef, Coverage, LookupSubTable};
use fontoxide_ttlib::tables::json::JsonValue;
use fontoxide_ttlib::tables::{Cmap, CmapSubtable, Debg, Gdef, Gpos, Gsub};
use fontoxide_ttlib::{Font, Table, TableType};
use indexmap::IndexMap;

use crate::context::{ContextSrc, Which};
use crate::glyphs::Glyphs;
use crate::tokenizer::{Line, Tokenizer, stop_iteration};
use crate::{Error, LOOKUP_DEBUG_INFO_KEY, Result};

// ---------------------------------------------------------------------------
// Python-like helpers

/// `KeyError(key)`.
pub(crate) fn key_error(key: &str) -> Error {
	Error::Value(format!("KeyError: {}", py::str_repr(key)))
}

/// `IndexError`.
pub(crate) fn index_error(what: &str) -> Error {
	Error::Value(format!("IndexError: {what} index out of range"))
}

/// `repr()` of a list of strings, for assertion messages.
pub(crate) fn line_repr(line: &[String]) -> String {
	let items: Vec<String> = line.iter().map(|s| py::str_repr(s)).collect();
	format!("[{}]", items.join(", "))
}

/// `int(s)`.
pub(crate) fn py_int(s: &str) -> Result<i128> {
	py::parse_int(s).ok_or_else(|| {
		Error::Value(format!(
			"ValueError: invalid literal for int() with base 10: {}",
			py::str_repr(s)
		))
	})
}

/// `int(s, 16)`.
fn py_int_hex(s: &str) -> Result<i128> {
	let err = || {
		Error::Value(format!(
			"ValueError: invalid literal for int() with base 16: {}",
			py::str_repr(s)
		))
	};
	let t = py::strip(s);
	let (negative, t) = match t.as_bytes().first() {
		Some(b'-') => (true, &t[1..]),
		Some(b'+') => (false, &t[1..]),
		_ => (false, t),
	};
	let t = t
		.strip_prefix("0x")
		.or_else(|| t.strip_prefix("0X"))
		.map_or(t, |rest| rest.strip_prefix('_').unwrap_or(rest));
	if t.is_empty() || t.starts_with('_') || t.ends_with('_') || t.contains("__") {
		return Err(err());
	}
	let digits: String = t.chars().filter(|&c| c != '_').collect();
	let v = i128::from_str_radix(&digits, 16).map_err(|_| err())?;
	Ok(if negative { -v } else { v })
}

/// A value that must fit its binary field, which fontTools only finds out when
/// compiling (`struct.error`).
pub(crate) fn fit<T: TryFrom<i128>>(v: i128, what: &str) -> Result<T> {
	T::try_from(v).map_err(|_| Error::Value(format!("struct.error: {what} {v} out of range")))
}

/// `list[i]`, with Python's negative indices.
pub(crate) fn py_index(len: usize, i: i128, what: &str) -> Result<usize> {
	let len_i = len as i128;
	let j = if i < 0 { i + len_i } else { i };
	if j < 0 || j >= len_i {
		return Err(index_error(what));
	}
	Ok(j as usize)
}

/// `s.split()`.
pub(crate) fn split_ws(s: &str) -> Vec<&str> {
	s.split(py::is_space).filter(|t| !t.is_empty()).collect()
}

/// `s.split()[0]`.
pub(crate) fn first_token(s: &str) -> Result<&str> {
	split_ws(s)
		.first()
		.copied()
		.ok_or_else(|| index_error("list"))
}

/// `s.title()`.
fn title(s: &str) -> String {
	let mut out = String::with_capacity(s.len());
	let mut prev_cased = false;
	for c in s.chars() {
		let cased = c.is_lowercase() || c.is_uppercase();
		if cased {
			if prev_cased {
				out.extend(c.to_lowercase());
			} else {
				out.extend(c.to_uppercase());
			}
		} else {
			out.push(c);
		}
		prev_cased = cased;
	}
	out
}

/// `stripSplitComma`.
pub(crate) fn strip_split_comma(line: &str) -> Vec<String> {
	if line.is_empty() {
		return Vec::new();
	}
	line.split(',').map(|s| py::strip(s).to_owned()).collect()
}

/// `intSplitComma`.
pub(crate) fn int_split_comma(line: &str) -> Result<Vec<i128>> {
	if line.is_empty() {
		return Ok(Vec::new());
	}
	line.split(',').map(py_int).collect()
}

/// `ValueError` of a tuple unpacking.
pub(crate) fn unpack_error(expected: usize, got: usize) -> Error {
	Error::Value(if got < expected {
		format!("ValueError: not enough values to unpack (expected {expected}, got {got})")
	} else {
		format!("ValueError: too many values to unpack (expected {expected})")
	})
}

/// The fields of a line, which must have exactly `n`.
pub(crate) fn unpack<const N: usize>(line: &[String]) -> Result<[&str; N]> {
	if line.len() != N {
		return Err(unpack_error(N, line.len()));
	}
	Ok(std::array::from_fn(|i| line[i].as_str()))
}

/// `line[i]`.
pub(crate) fn field(line: &[String], i: usize) -> Result<&str> {
	line.get(i)
		.map(String::as_str)
		.ok_or_else(|| index_error("list"))
}

/// `makeGlyph`: a glyph name, from `U <hex>` (a Unicode code point), `# <n>`
/// (a glyph ID) or the name itself.
pub fn make_glyph(s: &str) -> Result<String> {
	if let Some(rest) = s.strip_prefix("U ").or_else(|| s.strip_prefix("u ")) {
		let cp = py_int_hex(rest)?;
		return Ok(make_glyph_name(fit(cp, "code point")?));
	}
	if let Some(rest) = s.strip_prefix("# ") {
		let n = py_int(rest)?;
		return Ok(if n < 0 {
			format!("glyph-{:05}", -n)
		} else {
			format!("glyph{n:05}")
		});
	}
	if s.contains(' ') {
		return Err(Error::Assertion(format!("Space found in glyph name: {s}")));
	}
	if s.is_empty() {
		return Err(Error::Assertion("Glyph name is empty".into()));
	}
	Ok(s.to_owned())
}

/// The fields of the value records (`valueRecordFormatDict`): their masks.
fn value_record_mask(name: &str) -> Option<u16> {
	Some(match name {
		"XPlacement" => 0x0001,
		"YPlacement" => 0x0002,
		"XAdvance" => 0x0004,
		"YAdvance" => 0x0008,
		"XPlaDevice" => 0x0010,
		"YPlaDevice" => 0x0020,
		"XAdvDevice" => 0x0040,
		"YAdvDevice" => 0x0080,
		_ => {
			let n: u16 = name.strip_prefix("Reserved")?.parse().ok()?;
			if !(1..=8).contains(&n) || name != format!("Reserved{n}") {
				return None;
			}
			0x0080 << n
		}
	})
}

/// `assert not hasattr(vr, what); setattr(vr, what, value)`.
fn set_value(vr: &mut ValueRecord, what: &str, value: i128, owner: &str) -> Result<()> {
	let dup = || Error::Assertion(format!("({}, {})", owner, py::str_repr(what)));
	let int = |slot: &mut Option<i16>| -> Result<()> {
		if slot.is_some() {
			return Err(dup());
		}
		*slot = Some(fit(value, what)?);
		Ok(())
	};
	match what {
		"XPlacement" => int(&mut vr.x_placement),
		"YPlacement" => int(&mut vr.y_placement),
		"XAdvance" => int(&mut vr.x_advance),
		"YAdvance" => int(&mut vr.y_advance),
		_ => match what
			.strip_prefix("Reserved")
			.and_then(|n| n.parse::<usize>().ok())
		{
			Some(n @ 1..=8) => {
				let slot = &mut vr.reserved[n - 1];
				if slot.is_some() {
					return Err(dup());
				}
				*slot = Some(fit(value, what)?);
				Ok(())
			}
			// fontTools sets the device fields to integers, which fail to compile.
			_ => Err(Error::Value(format!(
				"AttributeError: an integer cannot be a device table ({what})"
			))),
		},
	}
}

/// `assert cond, message`.
pub(crate) fn ensure(cond: bool, message: impl FnOnce() -> String) -> Result<()> {
	if cond {
		Ok(())
	} else {
		Err(Error::Assertion(message()))
	}
}

/// A coverage table of glyph IDs, sorted and without duplicates
/// (`makeCoverage`); `None` without glyphs.
pub(crate) fn coverage_of(ids: impl IntoIterator<Item = u32>) -> Option<Coverage> {
	let glyphs: Vec<u32> = ids
		.into_iter()
		.collect::<BTreeSet<u32>>()
		.into_iter()
		.collect();
	(!glyphs.is_empty()).then_some(Coverage { glyphs })
}

/// The tag of a script or language system record, padded to four characters.
fn pad_tag(s: &str) -> String {
	let n = s.chars().count();
	format!("{s}{}", " ".repeat(4usize.saturating_sub(n)))
}

/// A tag for compiling, which must have four characters.
fn tag(s: &str) -> Result<Tag> {
	Tag::from_str_exact(s).ok_or_else(|| {
		Error::Value(format!(
			"AssertionError: tag {} is not 4 bytes",
			py::str_repr(s)
		))
	})
}

// ---------------------------------------------------------------------------
// The parser

/// A feature reference: an index, or a feature name (`mapFeature`).
#[derive(Debug, Clone)]
enum FeatureRef {
	Index(i128),
	Name(String),
}

#[derive(Debug)]
struct LangSysSrc {
	req: Option<FeatureRef>,
	features: Vec<FeatureRef>,
}

#[derive(Debug)]
struct ScriptSrc {
	tag: String,
	default: Option<LangSysSrc>,
	lang_sys: Vec<(String, LangSysSrc)>,
}

#[derive(Debug)]
struct FeatureSrc {
	tag: String,
	lookups: Vec<String>,
}

/// A subtable, built or waiting for the lookup indices of its lookup records.
#[derive(Debug)]
pub(crate) enum SubtableSrc {
	Built(LookupSubTable),
	Context(ContextSrc),
}

#[derive(Debug)]
struct LookupSrc {
	lookup_type: u16,
	flag: u16,
	mark_filtering_set: Option<u16>,
	subtables: Vec<SubtableSrc>,
}

/// GSUB or GPOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
	Gsub,
	Gpos,
}

/// The parser state: the glyphs, and the lookup and feature names referred to,
/// in order, which fontTools resolves at the end (`DeferredMapping`).
#[derive(Debug)]
pub(crate) struct Parser<'f> {
	pub glyphs: Glyphs<'f>,
	lookup_refs: Vec<String>,
	feature_refs: Vec<String>,
}

/// `build`.
pub(crate) fn build(
	text: &str,
	font: &mut Font,
	table_tag: Option<&str>,
	debug: bool,
	add_glyphs: bool,
) -> Result<Box<dyn Table>> {
	let mut lines = Tokenizer::new(text);
	let mut parser = Parser {
		glyphs: Glyphs::new(font, add_glyphs)?,
		lookup_refs: Vec::new(),
		feature_refs: Vec::new(),
	};
	let table = parser.parse_table(&mut lines, table_tag, debug);
	parser.glyphs.finish();
	table
}

impl Parser<'_> {
	/// `makeGlyph`, adding the glyph to a growing glyph order.
	pub(crate) fn make_glyph(&mut self, s: &str) -> Result<String> {
		let g = make_glyph(s)?;
		if self.glyphs.is_growing() {
			self.glyphs.id(&g)?;
		}
		Ok(g)
	}

	/// `makeGlyphs`.
	pub(crate) fn make_glyphs(&mut self, l: &[String]) -> Result<Vec<String>> {
		l.iter().map(|s| self.make_glyph(s)).collect()
	}

	/// The glyph ID of `makeGlyph(s)`.
	pub(crate) fn glyph_id(&mut self, s: &str) -> Result<u32> {
		let g = make_glyph(s)?;
		self.glyphs.id(&g)
	}

	/// Notes a reference to a lookup name (`setReference(mapLookup, ...)`).
	pub(crate) fn lookup_ref(&mut self, sym: &str) {
		self.lookup_refs.push(sym.to_owned());
	}

	/// `parseTable`.
	fn parse_table(
		&mut self,
		lines: &mut Tokenizer<'_>,
		table_tag: Option<&str>,
		debug: bool,
	) -> Result<Box<dyn Table>> {
		log::debug!("Parsing table");
		let first = lines.peeks0();
		let tokens = split_ws(&first);
		let head = *tokens.first().ok_or_else(|| index_error("list"))?;
		let tag = if head == "FontDame" {
			Some(*tokens.get(1).ok_or_else(|| index_error("list"))?)
		} else if tokens.iter().take(3).copied().collect::<Vec<_>>().join(" ") == "Font Chef Table"
		{
			Some(*tokens.get(3).ok_or_else(|| index_error("list"))?)
		} else {
			None
		};
		let mut table_tag = table_tag.map(str::to_owned);
		if let Some(tag) = tag {
			let tag = pad_tag(tag);
			lines.next();
			match &table_tag {
				None => table_tag = Some(tag),
				Some(t) => ensure(*t == tag, || {
					format!("({}, {})", py::str_repr(t), py::str_repr(&tag))
				})?,
			}
		}
		let Some(table_tag) = table_tag else {
			return Err(Error::Assertion(
				"Don't know what table to parse and data doesn't specify".into(),
			));
		};
		match table_tag.as_str() {
			"GSUB" => self.parse_gsub_gpos(lines, Layout::Gsub, debug),
			"GPOS" => self.parse_gsub_gpos(lines, Layout::Gpos, debug),
			"GDEF" => self.parse_gdef(lines),
			"cmap" => self.parse_cmap(lines),
			other => Err(key_error(other)),
		}
	}

	/// `parseScriptList`.
	fn parse_script_list(&mut self, lines: &mut Tokenizer<'_>) -> Result<Vec<ScriptSrc>> {
		let mut records: Vec<ScriptSrc> = Vec::new();
		lines.begin_between("script table")?;
		while let Some(mut line) = lines.next() {
			while line.len() < 4 {
				line.push(String::new());
			}
			let [script_tag, lang_sys_tag, default_feature, features] = unpack::<4>(&line)?;
			log::debug!("Adding script {script_tag} language-system {lang_sys_tag}");
			let req = if default_feature.is_empty() {
				None
			} else {
				Some(self.map_feature(default_feature))
			};
			let features = strip_split_comma(features)
				.iter()
				.map(|s| self.map_feature(s))
				.collect();
			let lang_sys = LangSysSrc { req, features };

			// UPSTREAM-BUG(B58): the records are looked up by the tag as written,
			// but hold the tag padded to four characters, so a script with a
			// tag of fewer than four characters gets a record for each of its
			// lines.
			let index = match records.iter().position(|s| s.tag == script_tag) {
				Some(i) => i,
				None => {
					records.push(ScriptSrc {
						tag: pad_tag(script_tag),
						default: None,
						lang_sys: Vec::new(),
					});
					records.len() - 1
				}
			};
			let script = &mut records[index];
			if lang_sys_tag == "default" {
				script.default = Some(lang_sys);
			} else {
				script.lang_sys.push((pad_tag(lang_sys_tag), lang_sys));
			}
		}
		lines.end_between("script table")?;
		for script in &mut records {
			script.lang_sys.sort_by(|a, b| a.0.cmp(&b.0));
		}
		records.sort_by(|a, b| a.tag.cmp(&b.tag));
		Ok(records)
	}

	/// `mapFeature` (resolved later with the feature names).
	fn map_feature(&mut self, sym: &str) -> FeatureRef {
		match py::parse_int(sym) {
			Some(i) => FeatureRef::Index(i),
			None => {
				self.feature_refs.push(sym.to_owned());
				FeatureRef::Name(sym.to_owned())
			}
		}
	}

	/// `parseFeatureList`.
	fn parse_feature_list(
		&mut self,
		lines: &mut Tokenizer<'_>,
		feature_map: &mut IndexMap<String, usize>,
	) -> Result<Vec<FeatureSrc>> {
		let mut records = Vec::new();
		lines.begin_between("feature table")?;
		while let Some(line) = lines.next() {
			let [name, feature_tag, lookups] = unpack::<3>(&line)?;
			ensure(!feature_map.contains_key(name), || {
				format!("Duplicate feature name: {name}")
			})?;
			feature_map.insert(name.to_owned(), records.len());
			// If feature name is integer, make sure it matches its index.
			if let Some(i) = py::parse_int(name)
				&& i != records.len() as i128
			{
				// UPSTREAM-BUG(B60): fontTools formats the message with `%d` and the
				// name, a string, which raises a TypeError instead.
				return Err(Error::Value(
					"TypeError: %d format: a real number is required, not str".into(),
				));
			}
			let lookups = strip_split_comma(lookups);
			for sym in &lookups {
				self.lookup_ref(sym);
			}
			records.push(FeatureSrc {
				tag: feature_tag.to_owned(),
				lookups,
			});
		}
		lines.end_between("feature table")?;
		Ok(records)
	}

	/// `parseLookupFlags`: the lookup flags and the mark filtering set.
	fn parse_lookup_flags(&mut self, lines: &mut Tokenizer<'_>) -> Result<(u16, Option<u16>)> {
		const ALL_FLAGS: [&str; 6] = [
			"righttoleft",
			"ignorebaseglyphs",
			"ignoreligatures",
			"ignoremarks",
			"markattachmenttype",
			"markfiltertype",
		];
		let mut flags: i128 = 0;
		let mut filterset = None;
		while ALL_FLAGS.contains(&lines.peeks0().to_lowercase().as_str()) {
			let line = lines.next_required()?;
			let name = line[0].to_lowercase();
			let flag = match name.as_str() {
				"righttoleft" => Some(0x0001),
				"ignorebaseglyphs" => Some(0x0002),
				"ignoreligatures" => Some(0x0004),
				"ignoremarks" => Some(0x0008),
				_ => None,
			};
			let value = field(&line, 1)?;
			if let Some(flag) = flag {
				let v = value.to_lowercase();
				ensure(v == "yes" || v == "no", || value.to_owned())?;
				if v == "yes" {
					flags |= flag;
				}
				continue;
			}
			if name == "markattachmenttype" {
				flags |= py_int(value)? << 8;
				continue;
			}
			if name == "markfiltertype" {
				flags |= 0x10;
				filterset = Some(fit(py_int(value)?, "MarkFilteringSet")?);
			}
		}
		Ok((fit(flags, "LookupFlag")?, filterset))
	}

	/// `parseSingleSubst`.
	fn parse_single_subst(&mut self, lines: &mut Tokenizer<'_>) -> Result<LookupSubTable> {
		let mut mapping = IndexMap::new();
		while let Some(line) = lines.next() {
			ensure(line.len() == 2, || line_repr(&line))?;
			let line = self.make_glyphs(&line)?;
			let [a, b] = unpack::<2>(&line)?;
			mapping.insert(a.to_owned(), b.to_owned());
		}
		let names: Vec<&str> = mapping
			.iter()
			.flat_map(|(k, v)| [k.as_str(), v.as_str()])
			.collect();
		let gm = self.glyphs.id_map(names)?;
		let st = otl::build_single_subst_subtable(&mapping, &gm)?.ok_or_else(none_subtable)?;
		Ok(LookupSubTable::SingleSubst(st))
	}

	/// `parseMultiple` and `parseAlternate`: the first glyph of each line to
	/// the others.
	fn parse_one_to_many(
		&mut self,
		lines: &mut Tokenizer<'_>,
	) -> Result<IndexMap<String, Vec<String>>> {
		let mut mapping = IndexMap::new();
		while let Some(line) = lines.next() {
			let line = self.make_glyphs(&line)?;
			mapping.insert(line[0].clone(), line[1..].to_vec());
		}
		Ok(mapping)
	}

	fn one_to_many_map(&mut self, mapping: &IndexMap<String, Vec<String>>) -> Result<GlyphMap> {
		let names: Vec<&str> = mapping
			.iter()
			.flat_map(|(k, v)| std::iter::once(k.as_str()).chain(v.iter().map(String::as_str)))
			.collect();
		self.glyphs.id_map(names)
	}

	/// `parseMultiple`.
	fn parse_multiple(&mut self, lines: &mut Tokenizer<'_>) -> Result<LookupSubTable> {
		let mapping = self.parse_one_to_many(lines)?;
		let gm = self.one_to_many_map(&mapping)?;
		let st = otl::build_multiple_subst_subtable(&mapping, &gm)?.ok_or_else(none_subtable)?;
		Ok(LookupSubTable::MultipleSubst(st))
	}

	/// `parseAlternate`.
	fn parse_alternate(&mut self, lines: &mut Tokenizer<'_>) -> Result<LookupSubTable> {
		let mapping = self.parse_one_to_many(lines)?;
		let gm = self.one_to_many_map(&mapping)?;
		let st = otl::build_alternate_subst_subtable(&mapping, &gm)?.ok_or_else(none_subtable)?;
		Ok(LookupSubTable::AlternateSubst(st))
	}

	/// `parseLigature`.
	fn parse_ligature(&mut self, lines: &mut Tokenizer<'_>) -> Result<LookupSubTable> {
		let mut mapping: IndexMap<Vec<String>, String> = IndexMap::new();
		while let Some(line) = lines.next() {
			ensure(line.len() >= 2, || line_repr(&line))?;
			let line = self.make_glyphs(&line)?;
			mapping.insert(line[1..].to_vec(), line[0].clone());
		}
		let names: Vec<&str> = mapping
			.iter()
			.flat_map(|(k, v)| {
				k.iter()
					.map(String::as_str)
					.chain(std::iter::once(v.as_str()))
			})
			.collect();
		let gm = self.glyphs.id_map(names)?;
		let st = otl::build_ligature_subst_subtable(&mapping, &gm)?.ok_or_else(none_subtable)?;
		Ok(LookupSubTable::LigatureSubst(st))
	}

	/// `parseSinglePos`.
	fn parse_single_pos(&mut self, lines: &mut Tokenizer<'_>) -> Result<LookupSubTable> {
		let mut values: IndexMap<String, ValueRecord> = IndexMap::new();
		while let Some(line) = lines.next() {
			ensure(line.len() == 3, || line_repr(&line))?;
			let w = title(&line[0]).replace(' ', "");
			ensure(value_record_mask(&w).is_some(), String::new)?;
			let g = self.make_glyph(&line[1])?;
			let v = py_int(&line[2])?;
			let vr = values.entry(g.clone()).or_default();
			set_value(vr, &w, v, &py::str_repr(&g))?;
		}
		let gm = self.glyphs.reverse_map(values.keys().map(String::as_str))?;
		let st = otl::build_single_pos_subtable(&values, &gm)?;
		Ok(LookupSubTable::SinglePos(st))
	}

	/// The side and field of a pair adjustment line (`left x advance`).
	fn pair_side(first: &str) -> Result<(bool, String, u16)> {
		let token = first_token(first)?;
		let side = token.to_lowercase();
		ensure(side == "left" || side == "right", || py::str_repr(&side))?;
		let what = title(&first[token.len()..]).replace(' ', "");
		let mask = value_record_mask(&what).ok_or_else(|| key_error(&what))?;
		Ok((side == "left", what, mask))
	}

	/// `parsePair`.
	fn parse_pair(&mut self, lines: &mut Tokenizer<'_>) -> Result<LookupSubTable> {
		let mut value_format1: u16 = 0;
		let mut value_format2: u16 = 0;
		let typ = first_token(&lines.peeks0())?.to_lowercase();
		if typ == "left" || typ == "right" {
			let mut values: IndexMap<u32, IndexMap<u32, (ValueRecord, ValueRecord)>> =
				IndexMap::new();
			while let Some(line) = lines.next() {
				ensure(line.len() == 4, || line_repr(&line))?;
				let (left, what, mask) = Self::pair_side(&line[0])?;
				let glyph1 = self.glyph_id(&line[1])?;
				let glyph2 = self.glyph_id(&line[2])?;
				let value = py_int(&line[3])?;
				let rec2 = values.entry(glyph1).or_default().entry(glyph2).or_default();
				let vr = if left {
					value_format1 |= mask;
					&mut rec2.0
				} else {
					value_format2 |= mask;
					&mut rec2.1
				};
				set_value(vr, &what, value, "vr")?;
			}
			let coverage = coverage_of(values.keys().copied()).ok_or_else(none_coverage)?;
			let mut pair_set = Vec::new();
			for glyph1 in &coverage.glyphs {
				let values1 = &values[glyph1];
				let mut seconds: Vec<u32> = values1.keys().copied().collect();
				seconds.sort_unstable();
				let records: Vec<ot::PairValueRecord> = seconds
					.iter()
					.map(|glyph2| {
						let (v1, v2) = &values1[glyph2];
						ot::PairValueRecord {
							second_glyph: *glyph2,
							value1: Some(v1.clone()),
							value2: (value_format2 != 0).then(|| v2.clone()),
						}
					})
					.collect();
				pair_set.push(Some(Box::new(ot::PairSet {
					pair_value_count: Some(records.len() as u32),
					pair_value_record: records,
				})));
			}
			return Ok(LookupSubTable::PairPos(ot::PairPos::Format1(
				ot::PairPosFormat1 {
					coverage: Some(Box::new(coverage)),
					value_format1,
					value_format2,
					pair_set_count: Some(pair_set.len() as u32),
					pair_set,
					dont_share: false,
				},
			)));
		}
		if !typ.ends_with("class") {
			return Err(Error::Assertion(typ));
		}
		let mut class_defs: [Option<ClassDef>; 2] = [None, None];
		while lines.peeks0().ends_with("class definition begin") {
			let first = lines.peeks0();
			let typ = first[..first.len() - "class definition begin".len()].to_lowercase();
			let idx = match typ.as_str() {
				"first" => 0,
				"second" => 1,
				_ => return Err(key_error(&typ)),
			};
			ensure(class_defs[idx].is_none(), String::new)?;
			class_defs[idx] = self.parse_class_def(lines)?;
		}
		let [class_def1, class_def2] = class_defs;
		let (Some(class_def1), Some(class_def2)) = (class_def1, class_def2) else {
			return Err(Error::Value(
				"AttributeError: 'NoneType' object has no attribute 'classDefs'".into(),
			));
		};
		let count = |c: &ClassDef| 1 + c.class_defs.values().copied().max().unwrap_or(0) as usize;
		let class1_count = count(&class_def1);
		let class2_count = count(&class_def2);
		let mut records = vec![
			vec![(ValueRecord::default(), ValueRecord::default()); class2_count];
			class1_count
		];
		while let Some(line) = lines.next() {
			ensure(line.len() == 4, || line_repr(&line))?;
			let (left, what, mask) = Self::pair_side(&line[0])?;
			let class1 = py_int(&line[1])?;
			let class2 = py_int(&line[2])?;
			let value = py_int(&line[3])?;
			let i = py_index(records.len(), class1, "list")?;
			let j = py_index(records[i].len(), class2, "list")?;
			let rec2 = &mut records[i][j];
			let vr = if left {
				value_format1 |= mask;
				&mut rec2.0
			} else {
				value_format2 |= mask;
				&mut rec2.1
			};
			set_value(vr, &what, value, "vr")?;
		}
		let class1_record = records
			.into_iter()
			.map(|rec1| ot::Class1Record {
				class2_record: rec1
					.into_iter()
					.map(|(v1, v2)| ot::Class2Record {
						value1: Some(otl::value_record_with_format(Some(&v1), value_format1)),
						value2: (value_format2 != 0)
							.then(|| otl::value_record_with_format(Some(&v2), value_format2)),
					})
					.collect(),
			})
			.collect();
		let coverage = coverage_of(class_def1.class_defs.keys().copied());
		Ok(LookupSubTable::PairPos(ot::PairPos::Format2(
			ot::PairPosFormat2 {
				coverage: coverage.map(Box::new),
				value_format1,
				value_format2,
				class_def1: Some(Box::new(class_def1)),
				class_def2: Some(Box::new(class_def2)),
				class1_count: Some(class1_count as u32),
				class2_count: Some(class2_count as u32),
				class1_record,
				dont_share: false,
			},
		)))
	}

	/// `parseKernset`.
	fn parse_kernset(&mut self, lines: &mut Tokenizer<'_>) -> Result<LookupSubTable> {
		let typ = first_token(&lines.peeks0())?.to_lowercase();
		if typ == "left" || typ == "right" {
			let tags = [
				"firstclass definition begin",
				"secondclass definition begin",
			];
			lines.push_until(&tags);
			let r = self.parse_pair(lines);
			lines.pop_until(tags.len());
			return r;
		}
		self.parse_pair(lines)
	}

	/// `parseCursive`.
	fn parse_cursive(&mut self, lines: &mut Tokenizer<'_>) -> Result<LookupSubTable> {
		let mut records: IndexMap<String, (Option<Anchor>, Option<Anchor>)> = IndexMap::new();
		while let Some(line) = lines.next() {
			ensure(line.len() == 3 || line.len() == 4, || line_repr(&line))?;
			let entry = match line[0].as_str() {
				"entry" => true,
				"exit" => false,
				other => return Err(key_error(other)),
			};
			let glyph = self.make_glyph(&line[1])?;
			let anchor = make_anchor(&line[2..])?;
			let rec = records.entry(glyph.clone()).or_default();
			let slot = if entry { &mut rec.0 } else { &mut rec.1 };
			ensure(slot.is_none(), || {
				format!("({}, {})", py::str_repr(&glyph), if entry { 0 } else { 1 })
			})?;
			*slot = Some(anchor);
		}
		let gm = self
			.glyphs
			.reverse_map(records.keys().map(String::as_str))?;
		let st = otl::build_cursive_pos_subtable(&records, &gm)?.ok_or_else(none_subtable)?;
		Ok(LookupSubTable::CursivePos(st))
	}

	/// `parseMarkToSomething`.
	fn parse_mark_to_something(
		&mut self,
		lines: &mut Tokenizer<'_>,
		kind: MarkKind,
	) -> Result<LookupSubTable> {
		let mut mark_data: IndexMap<u32, (i128, Anchor)> = IndexMap::new();
		// (glyph, class, extras...) to anchor.
		let mut base_data: IndexMap<(u32, i128, Vec<i128>), Anchor> = IndexMap::new();
		let mut max_klass: i128 = 0;
		while let Some(line) = lines.next() {
			let typ = line[0].as_str();
			ensure(matches!(typ, "mark" | "base" | "ligature"), String::new)?;
			let glyph = self.glyph_id(field(&line, 1)?)?;
			let extra_items = if typ == "ligature" { 2 } else { 0 };
			let extras = line
				.iter()
				.skip(2)
				.take(extra_items)
				.map(|s| py_int(s))
				.collect::<Result<Vec<i128>>>()?;
			let klass = py_int(field(&line, 2 + extra_items)?)?;
			let anchor = make_anchor(line.get(3 + extra_items..).unwrap_or(&[]))?;
			if typ == "mark" {
				ensure(!mark_data.contains_key(&glyph), || glyph.to_string())?;
				mark_data.insert(glyph, (klass, anchor));
			} else {
				let key = (glyph, klass, extras);
				ensure(!base_data.contains_key(&key), || format!("{key:?}"))?;
				base_data.insert(key, anchor);
			}
			max_klass = max_klass.max(klass);
		}

		// Mark
		let mark_coverage = coverage_of(mark_data.keys().copied()).ok_or_else(none_coverage)?;
		let mut mark_record = Vec::new();
		for glyph in &mark_coverage.glyphs {
			let (klass, anchor) = &mark_data[glyph];
			mark_record.push(ot::MarkRecord {
				class: fit(*klass, "Class")?,
				mark_anchor: Some(Box::new(anchor.clone())),
			});
		}
		let mark_array = ot::MarkArray {
			mark_count: Some(mark_record.len() as u32),
			mark_record,
		};
		let class_count: u32 = fit(max_klass + 1, "ClassCount")?;

		// Base
		let base_class_count = base_data.keys().map(|k| k.1 + 1).max().unwrap_or(0);
		let base_class_count: usize = fit(base_class_count, "classCount")?;
		let base_coverage = coverage_of(base_data.keys().map(|k| k.0)).ok_or_else(none_coverage)?;
		let index: BTreeMap<u32, usize> = base_coverage
			.glyphs
			.iter()
			.enumerate()
			.map(|(i, g)| (*g, i))
			.collect();
		let mark_coverage = Some(Box::new(mark_coverage));
		let mark_array = Some(Box::new(mark_array));
		let class_count = Some(class_count);
		Ok(match kind {
			MarkKind::Ligature => {
				let mut records: Vec<Option<ot::LigatureAttach>> = vec![None; index.len()];
				for ((glyph, klass, extras), anchor) in &base_data {
					let [comp_idx, comp_count] = extras[..] else {
						return Err(unpack_error(4, 2 + extras.len()));
					};
					let record = records[index[glyph]].get_or_insert_with(|| {
						let n = usize::try_from(comp_count).unwrap_or(0);
						ot::LigatureAttach {
							component_count: Some(comp_count as u32),
							component_record: vec![
								ot::ComponentRecord {
									ligature_anchor: vec![None; base_class_count],
								};
								n
							],
						}
					});
					ensure(record.component_count == Some(comp_count as u32), || {
						format!("({glyph}, {:?}, {comp_count})", record.component_count)
					})?;
					fit::<u16>(comp_count, "ComponentCount")?;
					let c = py_index(record.component_record.len(), comp_idx - 1, "list")?;
					let anchors = &mut record.component_record[c].ligature_anchor;
					let k = py_index(anchors.len(), *klass, "list")?;
					ensure(anchors[k].is_none(), || {
						format!("({glyph}, {comp_idx}, {klass})")
					})?;
					anchors[k] = Some(Box::new(anchor.clone()));
				}
				let ligature_attach: Vec<Option<Box<ot::LigatureAttach>>> =
					records.into_iter().map(|r| r.map(Box::new)).collect();
				LookupSubTable::MarkLigPos(ot::MarkLigPos::Format1(ot::MarkLigPosFormat1 {
					mark_coverage,
					ligature_coverage: Some(Box::new(base_coverage)),
					class_count,
					mark_array,
					ligature_array: Some(Box::new(ot::LigatureArray {
						ligature_count: Some(ligature_attach.len() as u32),
						ligature_attach,
					})),
					dont_share: false,
				}))
			}
			MarkKind::Base | MarkKind::Mark => {
				let mut anchors_of: Vec<Vec<Option<Box<Anchor>>>> =
					vec![vec![None; base_class_count]; index.len()];
				for ((glyph, klass, extras), anchor) in &base_data {
					if !extras.is_empty() {
						return Err(unpack_error(2, 2 + extras.len()));
					}
					let anchors = &mut anchors_of[index[glyph]];
					let k = py_index(anchors.len(), *klass, "list")?;
					ensure(anchors[k].is_none(), || format!("({glyph}, {klass})"))?;
					anchors[k] = Some(Box::new(anchor.clone()));
				}
				if kind == MarkKind::Base {
					let base_record: Vec<ot::BaseRecord> = anchors_of
						.into_iter()
						.map(|base_anchor| ot::BaseRecord { base_anchor })
						.collect();
					LookupSubTable::MarkBasePos(ot::MarkBasePos::Format1(ot::MarkBasePosFormat1 {
						mark_coverage,
						base_coverage: Some(Box::new(base_coverage)),
						class_count,
						mark_array,
						base_array: Some(Box::new(ot::BaseArray {
							base_count: Some(base_record.len() as u32),
							base_record,
						})),
						dont_share: false,
					}))
				} else {
					let mark2_record: Vec<ot::Mark2Record> = anchors_of
						.into_iter()
						.map(|mark2_anchor| ot::Mark2Record { mark2_anchor })
						.collect();
					LookupSubTable::MarkMarkPos(ot::MarkMarkPos::Format1(ot::MarkMarkPosFormat1 {
						mark1_coverage: mark_coverage,
						mark2_coverage: Some(Box::new(base_coverage)),
						class_count,
						mark1_array: mark_array,
						mark2_array: Some(Box::new(ot::Mark2Array {
							mark2_count: Some(mark2_record.len() as u32),
							mark2_record,
						})),
						dont_share: false,
					}))
				}
			}
		})
	}

	/// `parseClassDef`: `None` for an empty class definition.
	pub(crate) fn parse_class_def(
		&mut self,
		lines: &mut Tokenizer<'_>,
	) -> Result<Option<ClassDef>> {
		let mut class_defs = BTreeMap::new();
		lines.begin_between("class definition")?;
		while let Some(line) = lines.next() {
			let glyph = self.glyph_id(&line[0])?;
			ensure(!class_defs.contains_key(&glyph), || line[0].clone())?;
			let class: u16 = fit(py_int(field(&line, 1)?)?, "class")?;
			class_defs.insert(glyph, u32::from(class));
		}
		lines.end_between("class definition")?;
		Ok((!class_defs.is_empty()).then_some(ClassDef { class_defs }))
	}

	/// `parseCoverage`.
	pub(crate) fn parse_coverage(&mut self, lines: &mut Tokenizer<'_>) -> Result<Option<Coverage>> {
		let mut glyphs = Vec::new();
		lines.begin_between("coverage definition")?;
		while let Some(line) = lines.next() {
			glyphs.push(self.glyph_id(&line[0])?);
		}
		lines.end_between("coverage definition")?;
		Ok(coverage_of(glyphs))
	}

	/// `parseReverseChainedSubst`.
	fn parse_reverse_chained_subst(&mut self, lines: &mut Tokenizer<'_>) -> Result<LookupSubTable> {
		let mut coverages: [Vec<Option<Box<Coverage>>>; 2] = [Vec::new(), Vec::new()];
		while lines.peeks0().ends_with("coverage definition begin") {
			let first = lines.peeks0();
			let typ = first[..first.len() - "coverage definition begin".len()].to_lowercase();
			let idx = match typ.as_str() {
				"backtrack" => 0,
				"lookahead" => 1,
				_ => return Err(key_error(&typ)),
			};
			let coverage = self.parse_coverage(lines)?;
			coverages[idx].push(coverage.map(Box::new));
		}
		let [backtrack_coverage, look_ahead_coverage] = coverages;
		let mut mapping: IndexMap<u32, u32> = IndexMap::new();
		while let Some(line) = lines.next() {
			ensure(line.len() == 2, || line_repr(&line))?;
			let a = self.glyph_id(&line[0])?;
			let b = self.glyph_id(&line[1])?;
			mapping.insert(a, b);
		}
		let coverage = coverage_of(mapping.keys().copied()).ok_or_else(none_coverage)?;
		let substitute: Vec<u32> = coverage.glyphs.iter().map(|g| mapping[g]).collect();
		Ok(LookupSubTable::ReverseChainSingleSubst(
			ot::ReverseChainSingleSubst::Format1(ot::ReverseChainSingleSubstFormat1 {
				coverage: Some(Box::new(coverage)),
				backtrack_glyph_count: Some(backtrack_coverage.len() as u32),
				backtrack_coverage,
				look_ahead_glyph_count: Some(look_ahead_coverage.len() as u32),
				look_ahead_coverage,
				glyph_count: Some(substitute.len() as u32),
				substitute,
				dont_share: false,
			}),
		))
	}

	/// `parseLookup`: `None` for a lookup without subtables.
	fn parse_lookup(
		&mut self,
		lines: &mut Tokenizer<'_>,
		table: Layout,
	) -> Result<Option<LookupSrc>> {
		let line = lines.expect("lookup")?;
		let [_, name, typ] = unpack::<3>(&line)?;
		log::debug!("Parsing lookup type {typ} {name}");
		let (flag, mark_filtering_set) = self.parse_lookup_flags(lines)?;
		let (lookup_type, kind) = match (table, typ) {
			(Layout::Gsub, "single") => (1, SubtableKind::SingleSubst),
			(Layout::Gsub, "multiple") => (2, SubtableKind::Multiple),
			(Layout::Gsub, "alternate") => (3, SubtableKind::Alternate),
			(Layout::Gsub, "ligature") => (4, SubtableKind::Ligature),
			(Layout::Gsub, "context") => (5, SubtableKind::Context(Which::Subst, false)),
			(Layout::Gsub, "chained") => (6, SubtableKind::Context(Which::Subst, true)),
			(Layout::Gsub, "reversechained") => (8, SubtableKind::ReverseChained),
			(Layout::Gpos, "single") => (1, SubtableKind::SinglePos),
			(Layout::Gpos, "pair") => (2, SubtableKind::Pair),
			(Layout::Gpos, "kernset") => (2, SubtableKind::Kernset),
			(Layout::Gpos, "cursive") => (3, SubtableKind::Cursive),
			(Layout::Gpos, "mark to base") => (4, SubtableKind::Mark(MarkKind::Base)),
			(Layout::Gpos, "mark to ligature") => (5, SubtableKind::Mark(MarkKind::Ligature)),
			(Layout::Gpos, "mark to mark") => (6, SubtableKind::Mark(MarkKind::Mark)),
			(Layout::Gpos, "context") => (7, SubtableKind::Context(Which::Pos, false)),
			(Layout::Gpos, "chained") => (8, SubtableKind::Context(Which::Pos, true)),
			_ => return Err(key_error(typ)),
		};

		lines.push_until(&["lookup end"]);
		let mut subtables = Vec::new();
		while lines.peek().is_some() {
			let before = subtables.len();
			let separators = ["% subtable", "subtable end"];
			lines.push_until(&separators);
			while lines.peek().is_some() {
				let mark = progress(lines);
				subtables.push(self.parse_subtable(lines, kind)?);
				if progress(lines) == mark {
					return Err(no_progress());
				}
			}
			lines.pop_until(separators.len());
			let next = lines.peeks0();
			if separators.contains(&next.as_str()) {
				lines.next();
			} else if subtables.len() == before {
				// UPSTREAM-BUG(B59): fontTools stops at the subtable separators
				// whatever their case, but only skips them in lowercase: a
				// `Subtable End` makes it loop forever.
				return Err(no_progress());
			}
		}
		lines.pop_until(1);
		lines.expect("lookup end")?;

		if subtables.is_empty() {
			// fontTools returns `None` for lookups without subtables
			// (https://github.com/fonttools/fonttools/issues/789).
			return Ok(None);
		}
		Ok(Some(LookupSrc {
			lookup_type,
			flag,
			mark_filtering_set,
			subtables,
		}))
	}

	fn parse_subtable(
		&mut self,
		lines: &mut Tokenizer<'_>,
		kind: SubtableKind,
	) -> Result<SubtableSrc> {
		Ok(SubtableSrc::Built(match kind {
			SubtableKind::SingleSubst => self.parse_single_subst(lines)?,
			SubtableKind::Multiple => self.parse_multiple(lines)?,
			SubtableKind::Alternate => self.parse_alternate(lines)?,
			SubtableKind::Ligature => self.parse_ligature(lines)?,
			SubtableKind::ReverseChained => self.parse_reverse_chained_subst(lines)?,
			SubtableKind::SinglePos => self.parse_single_pos(lines)?,
			SubtableKind::Pair => self.parse_pair(lines)?,
			SubtableKind::Kernset => self.parse_kernset(lines)?,
			SubtableKind::Cursive => self.parse_cursive(lines)?,
			SubtableKind::Mark(kind) => self.parse_mark_to_something(lines, kind)?,
			SubtableKind::Context(which, chain) => {
				return Ok(SubtableSrc::Context(
					self.parse_context(lines, which, chain)?,
				));
			}
		}))
	}

	/// `parseGSUBGPOS`.
	fn parse_gsub_gpos(
		&mut self,
		lines: &mut Tokenizer<'_>,
		table: Layout,
		debug: bool,
	) -> Result<Box<dyn Table>> {
		let mut lookup_map: IndexMap<String, usize> = IndexMap::new();
		let mut feature_map: IndexMap<String, usize> = IndexMap::new();
		log::debug!("Parsing {}", table.tag());
		let mut script_list: Option<Vec<ScriptSrc>> = None;
		let mut feature_list: Option<Vec<FeatureSrc>> = None;
		let mut lookups: Option<Vec<Option<LookupSrc>>> = None;
		while let Some(line) = lines.peek() {
			let typ = line[0].to_lowercase();
			match typ.as_str() {
				"script table begin" => {
					ensure(script_list.is_none(), || "ScriptList".into())?;
					script_list = Some(self.parse_script_list(lines)?);
				}
				"feature table begin" => {
					ensure(feature_list.is_none(), || "FeatureList".into())?;
					feature_list = Some(self.parse_feature_list(lines, &mut feature_map)?);
				}
				"lookup" => {
					let [_, name, _] = unpack::<3>(line)?;
					let name = name.to_owned();
					let lookup = self.parse_lookup(lines, table)?;
					let list = lookups.get_or_insert_with(Vec::new);
					ensure(!lookup_map.contains_key(&name), || {
						format!("Duplicate lookup name: {name}")
					})?;
					lookup_map.insert(name, list.len());
					list.push(lookup);
				}
				_ => {
					log::debug!("Skipping {}", line_repr(line));
					lines.next();
				}
			}
		}

		// `lookupMap.applyDeferredMappings()`
		if let Some(missing) = self
			.lookup_refs
			.iter()
			.find(|s| !lookup_map.contains_key(*s))
		{
			return Err(Error::LookupNotFound(missing.clone()));
		}
		if debug {
			self.add_debug_info(table, &lookup_map)?;
		}
		// `featureMap.applyDeferredMappings()`
		if let Some(missing) = self
			.feature_refs
			.iter()
			.find(|s| !feature_map.contains_key(*s))
		{
			return Err(Error::FeatureNotFound(missing.clone()));
		}

		let resolve_feature = |r: &FeatureRef| -> Result<u16> {
			match r {
				FeatureRef::Index(i) => fit(*i, "FeatureIndex"),
				FeatureRef::Name(n) => fit(feature_map[n] as i128, "FeatureIndex"),
			}
		};
		let lang_sys = |src: LangSysSrc| -> Result<ot::LangSys> {
			let feature_index = src
				.features
				.iter()
				.map(resolve_feature)
				.collect::<Result<Vec<u16>>>()?;
			Ok(ot::LangSys {
				lookup_order: None,
				req_feature_index: match &src.req {
					Some(r) => resolve_feature(r)?,
					None => 0xFFFF,
				},
				feature_count: Some(feature_index.len() as u32),
				feature_index,
			})
		};
		let script_list = match script_list {
			None => None,
			Some(records) => {
				let mut script_record = Vec::new();
				for s in records {
					let mut lang_sys_record = Vec::new();
					for (t, l) in s.lang_sys {
						lang_sys_record.push(ot::LangSysRecord {
							lang_sys_tag: tag(&t)?,
							lang_sys: Some(Box::new(lang_sys(l)?)),
						});
					}
					script_record.push(ot::ScriptRecord {
						script_tag: tag(&s.tag)?,
						script: Some(Box::new(ot::Script {
							default_lang_sys: s.default.map(lang_sys).transpose()?.map(Box::new),
							lang_sys_count: Some(lang_sys_record.len() as u32),
							lang_sys_record,
						})),
					});
				}
				Some(Box::new(ot::ScriptList {
					script_count: Some(script_record.len() as u32),
					script_record,
				}))
			}
		};
		let lookup_index =
			|name: &str| -> Result<u16> { fit(lookup_map[name] as i128, "LookupListIndex") };
		let feature_list = match feature_list {
			None => None,
			Some(records) => {
				let mut feature_record = Vec::new();
				for f in records {
					let lookup_list_index = f
						.lookups
						.iter()
						.map(|n| lookup_index(n))
						.collect::<Result<Vec<u16>>>()?;
					feature_record.push(ot::FeatureRecord {
						feature_tag: tag(&f.tag)?,
						feature: Some(Box::new(ot::Feature {
							feature_params: None,
							lookup_count: Some(lookup_list_index.len() as u32),
							lookup_list_index,
						})),
					});
				}
				Some(Box::new(ot::FeatureList {
					feature_count: Some(feature_record.len() as u32),
					feature_record,
				}))
			}
		};
		let lookup_list = match lookups {
			None => None,
			Some(list) => {
				let mut lookup = Vec::new();
				for l in list {
					let Some(l) = l else {
						lookup.push(None);
						continue;
					};
					let mut sub_table = Vec::new();
					for st in l.subtables {
						sub_table.push(Some(Box::new(match st {
							SubtableSrc::Built(st) => st,
							SubtableSrc::Context(c) => c.finish(&lookup_index)?,
						})));
					}
					lookup.push(Some(Box::new(ot::Lookup {
						lookup_type: l.lookup_type,
						lookup_flag: l.flag,
						sub_table_count: Some(sub_table.len() as u32),
						sub_table,
						mark_filtering_set: l.mark_filtering_set.unwrap_or(0),
					})));
				}
				Some(Box::new(ot::LookupList {
					lookup_count: Some(lookup.len() as u32),
					lookup,
				}))
			}
		};
		Ok(match table {
			Layout::Gsub => Box::new(Gsub {
				table: ot::GSUB {
					version: 0x0001_0000,
					script_list,
					feature_list,
					lookup_list,
					..ot::GSUB::default()
				},
			}),
			Layout::Gpos => Box::new(Gpos {
				table: ot::GPOS {
					version: 0x0001_0000,
					script_list,
					feature_list,
					lookup_list,
					..ot::GPOS::default()
				},
			}),
		})
	}

	/// The lookup debugging information: `["", name, ""]` for each lookup.
	fn add_debug_info(
		&mut self,
		table: Layout,
		lookup_map: &IndexMap<String, usize>,
	) -> Result<()> {
		let font = &mut *self.glyphs.font;
		if !font.contains(Debg::TAG) {
			font.insert(Box::new(Debg::default()));
		}
		let Some(debg) = font.get::<Debg>()? else {
			return Ok(());
		};
		let not_dict =
			|| Error::Value("AttributeError: object has no attribute 'setdefault'".into());
		let info = setdefault_object(&mut debg.data, LOOKUP_DEBUG_INFO_KEY).ok_or_else(not_dict)?;
		let debug = setdefault_object(info, &table.tag().as_string()).ok_or_else(not_dict)?;
		let JsonValue::Object(items) = debug else {
			return Err(not_dict());
		};
		for (name, index) in lookup_map {
			let key = index.to_string();
			let value = JsonValue::Array(vec![
				JsonValue::str(""),
				JsonValue::str(name),
				JsonValue::str(""),
			]);
			match items.iter_mut().find(|(k, _)| *k == key) {
				Some((_, v)) => *v = value,
				None => items.push((key, value)),
			}
		}
		Ok(())
	}

	/// `parseAttachList`.
	fn parse_attach_list(&mut self, lines: &mut Tokenizer<'_>) -> Result<Option<ot::AttachList>> {
		let mut points: IndexMap<String, Vec<u16>> = IndexMap::new();
		lines.begin_between("attachment list")?;
		while let Some(line) = lines.next() {
			let glyph = self.make_glyph(&line[0])?;
			ensure(!points.contains_key(&glyph), || glyph.clone())?;
			let p = line[1..]
				.iter()
				.map(|i| fit(py_int(i)?, "PointIndex"))
				.collect::<Result<Vec<u16>>>()?;
			points.insert(glyph, p);
		}
		lines.end_between("attachment list")?;
		let gm = self.glyphs.reverse_map(points.keys().map(String::as_str))?;
		Ok(otl::build_attach_list(&points, &gm)?)
	}

	/// `parseCaretList`.
	fn parse_caret_list(
		&mut self,
		lines: &mut Tokenizer<'_>,
	) -> Result<Option<MaybeExtended<ot::LigCaretList, ot::LigCaretList2>>> {
		let mut carets: IndexMap<String, Vec<CaretCoord>> = IndexMap::new();
		lines.begin_between("carets")?;
		while let Some(line) = lines.next() {
			let glyph = self.make_glyph(&line[0])?;
			ensure(!carets.contains_key(&glyph), || glyph.clone())?;
			let num = py_int(field(&line, 1)?)?;
			let these = line[2..]
				.iter()
				.map(|i| Ok(CaretCoord::Coord(fit(py_int(i)?, "Coordinate")?)))
				.collect::<Result<Vec<CaretCoord>>>()?;
			ensure(num == these.len() as i128, || line_repr(&line))?;
			carets.insert(glyph, these);
		}
		lines.end_between("carets")?;
		let gm = self.glyphs.reverse_map(carets.keys().map(String::as_str))?;
		Ok(otl::build_lig_caret_list(&carets, &IndexMap::new(), &gm)?)
	}

	/// `parseMarkFilteringSets`.
	fn parse_mark_filtering_sets(
		&mut self,
		lines: &mut Tokenizer<'_>,
	) -> Result<ot::MarkGlyphSetsDef> {
		let mut sets: BTreeMap<i128, Vec<u32>> = BTreeMap::new();
		lines.begin_between("set definition")?;
		while let Some(line) = lines.next() {
			ensure(line.len() == 2, || line_repr(&line))?;
			let glyph = self.glyph_id(&line[0])?;
			let st = py_int(&line[1])?;
			sets.entry(st).or_default().push(glyph);
		}
		lines.end_between("set definition")?;
		// `makeMarkFilteringSets`
		let Some((&max, _)) = sets.last_key_value() else {
			return Err(Error::Value(
				"ValueError: max() iterable argument is empty".into(),
			));
		};
		let count = max + 1;
		let mut coverage: Vec<Option<Box<Coverage>>> =
			vec![None; usize::try_from(count).unwrap_or(0)];
		for (k, v) in sets {
			let i = py_index(coverage.len(), k, "list assignment")?;
			coverage[i] = coverage_of(v).map(Box::new);
		}
		Ok(ot::MarkGlyphSetsDef {
			mark_set_table_format: 1,
			mark_set_count: Some(fit(count, "MarkSetCount")?),
			coverage,
		})
	}

	/// `parseGDEF`.
	fn parse_gdef(&mut self, lines: &mut Tokenizer<'_>) -> Result<Box<dyn Table>> {
		log::debug!("Parsing GDEF");
		let mut gdef = ot::GDEF::default();
		while let Some(line) = lines.peek() {
			let typ = line[0].to_lowercase();
			match typ.as_str() {
				"class definition begin" => {
					ensure(gdef.glyph_class_def.is_none(), || "GlyphClassDef".into())?;
					gdef.glyph_class_def = self.parse_class_def(lines)?.map(Box::new);
				}
				"attachment list begin" => {
					ensure(gdef.attach_list.is_none(), || "AttachList".into())?;
					gdef.attach_list = self.parse_attach_list(lines)?.map(Box::new);
				}
				"carets begin" => {
					ensure(
						gdef.lig_caret_list.is_none() && gdef.lig_caret_list2.is_none(),
						|| "LigCaretList".into(),
					)?;
					match self.parse_caret_list(lines)? {
						Some(MaybeExtended::Normal(l)) => gdef.lig_caret_list = Some(Box::new(l)),
						Some(MaybeExtended::Extended(l)) => {
							gdef.lig_caret_list2 = Some(Box::new(l))
						}
						None => {}
					}
				}
				"mark attachment class definition begin" => {
					ensure(gdef.mark_attach_class_def.is_none(), || {
						"MarkAttachClassDef".into()
					})?;
					gdef.mark_attach_class_def = self.parse_class_def(lines)?.map(Box::new);
				}
				"markfilter set definition begin" => {
					ensure(gdef.mark_glyph_sets_def.is_none(), || {
						"MarkGlyphSetsDef".into()
					})?;
					gdef.mark_glyph_sets_def =
						Some(Box::new(self.parse_mark_filtering_sets(lines)?));
				}
				_ => {
					log::debug!("Skipping {typ}");
					lines.next();
				}
			}
		}
		gdef.version = if gdef.mark_glyph_sets_def.is_none() {
			0x0001_0000
		} else {
			0x0001_0002
		};
		Ok(Box::new(Gdef { table: gdef }))
	}

	/// `parseCmap`.
	fn parse_cmap(&mut self, lines: &mut Tokenizer<'_>) -> Result<Box<dyn Table>> {
		log::debug!("Parsing cmap");
		let mut tables = Vec::new();
		while lines.peek().is_some() {
			lines.expect(&format!("cmap subtable {}", tables.len()))?;
			let mut ids = [0i128; 4];
			for (id, name) in ids
				.iter_mut()
				.zip(["platformID", "encodingID", "format", "language"])
			{
				// `parseCmapId`
				let line = lines.next_required()?;
				ensure(line[0] == name, String::new)?;
				*id = py_int(field(&line, 1)?)?;
			}
			let [plat_id, enc_id, fmt, lang] = ids;
			if !matches!(fmt, 0 | 2 | 4 | 6 | 12 | 13 | 14 | 15) {
				return Err(Error::Value(format!("KeyError: {fmt}")));
			}
			let mut table = CmapSubtable::new(fmt as u16);
			table.platform_id = fit(plat_id, "platformID")?;
			table.plat_enc_id = fit(enc_id, "platEncID")?;
			table.language = Some(fit(lang, "language")?);
			let mut cmap = BTreeMap::new();
			loop {
				let line = lines.next().ok_or_else(stop_iteration)?;
				if line[0] == "end subtable" {
					break;
				}
				let code: u32 = fit(py_int_hex(&line[0])?, "code")?;
				let gid = self.glyphs.id(field(&line, 1)?)?;
				cmap.insert(code, gid);
			}
			table.cmap = Arc::new(cmap);
			tables.push(table);
		}
		Ok(Box::new(Cmap {
			table_version: 0,
			tables,
		}))
	}
}

/// What the subtables of a lookup are.
#[derive(Debug, Clone, Copy)]
enum SubtableKind {
	SingleSubst,
	Multiple,
	Alternate,
	Ligature,
	ReverseChained,
	SinglePos,
	Pair,
	Kernset,
	Cursive,
	Mark(MarkKind),
	Context(Which, bool),
}

/// Mark to base, to ligature or to mark (`MarkToBaseHelper` and such).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkKind {
	Base,
	Ligature,
	Mark,
}

impl Layout {
	fn tag(self) -> Tag {
		match self {
			Layout::Gsub => Tag::new(b"GSUB"),
			Layout::Gpos => Tag::new(b"GPOS"),
		}
	}
}

/// `makeAnchor`.
fn make_anchor(data: &[String]) -> Result<Anchor> {
	ensure(data.len() <= 2, String::new)?;
	let coords = int_split_comma(field(data, 0)?)?;
	let [x, y] = coords[..] else {
		return Err(unpack_error(2, coords.len()));
	};
	let point = match data.get(1) {
		Some(p) if !p.is_empty() => Some(fit(py_int(p)?, "AnchorPoint")?),
		_ => None,
	};
	Ok(otl::build_anchor(
		fit(x, "XCoordinate")?,
		fit(y, "YCoordinate")?,
		point,
		None,
		None,
	)?)
}

/// `dict.setdefault(key, {})`, `None` if `value` is not a dict.
fn setdefault_object<'a>(value: &'a mut JsonValue, key: &str) -> Option<&'a mut JsonValue> {
	let JsonValue::Object(items) = value else {
		return None;
	};
	let i = match items.iter().position(|(k, _)| k == key) {
		Some(i) => i,
		None => {
			items.push((key.to_owned(), JsonValue::Object(Vec::new())));
			items.len() - 1
		}
	};
	Some(&mut items[i].1)
}

/// What the tokenizer has read, to detect loops that would not end.
fn progress(lines: &Tokenizer<'_>) -> (usize, bool) {
	let (pos, buffer) = lines.position();
	(pos, buffer.is_some())
}

fn no_progress() -> Error {
	Error::Value("the parser makes no progress (fontTools loops forever here)".into())
}

/// A builder that returned `None` where fontTools then reads an attribute.
fn none_subtable() -> Error {
	Error::Value("AttributeError: 'NoneType' object has no attribute 'LookupType'".into())
}

/// A coverage that `makeCoverage` returned as `None`, whose glyphs fontTools
/// then reads.
pub(crate) fn none_coverage() -> Error {
	Error::Value("AttributeError: 'NoneType' object has no attribute 'glyphs'".into())
}

/// The lines left, for `list(lines)`.
pub(crate) fn rest(lines: &mut Tokenizer<'_>) -> Vec<Line> {
	let mut out = Vec::new();
	while let Some(line) = lines.next() {
		out.push(line);
	}
	out
}
