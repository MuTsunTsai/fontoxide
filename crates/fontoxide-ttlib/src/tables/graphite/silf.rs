//! The Graphite Rules table, `Silf` (`S__i_l_f`).

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::text_tools::parse_int_radix;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::{AttrValue, XmlWriter};

use super::{
	Pack, at_least, attr, bininfo, compress, content_string, count, decompress, dict_set, entries,
	eval_float, eval_int, exact, py_get, py_int, py_slice, py_split, py_tail, read_fixed,
	round_version,
};
use crate::binary::Reader;
use crate::table::{Table, TableType, TableXml};
use crate::{Error, Font, Result};

// ---------------------------------------------------------------------------
// Action code

/// The operands of an instruction (the second item of `aCode_info`).
#[derive(Clone, Copy)]
enum Operands {
	/// `0`: none.
	None,
	/// `-1`: a count, then that many bytes.
	Counted,
	/// A `struct` format of fixed operands, without the byte order prefix.
	Fixed(&'static str),
}

/// `aCode_info`: the instructions by opcode.
const ACODE_INFO: [(&str, Operands); 67] = {
	use Operands::{Counted, Fixed, None};
	[
		("NOP", None),
		("PUSH_BYTE", Fixed("b")),
		("PUSH_BYTE_U", Fixed("B")),
		("PUSH_SHORT", Fixed("h")),
		("PUSH_SHORT_U", Fixed("H")),
		("PUSH_LONG", Fixed("L")),
		("ADD", None),
		("SUB", None),
		("MUL", None),
		("DIV", None),
		("MIN", None),
		("MAX", None),
		("NEG", None),
		("TRUNC8", None),
		("TRUNC16", None),
		("COND", None),
		("AND", None),
		("OR", None),
		("NOT", None),
		("EQUAL", None),
		("NOT_EQ", None),
		("LESS", None),
		("GTR", None),
		("LESS_EQ", None),
		("GTR_EQ", None),
		("NEXT", None),
		("NEXT_N", Fixed("b")),
		("COPY_NEXT", None),
		("PUT_GLYPH_8BIT_OBS", Fixed("B")),
		("PUT_SUBS_8BIT_OBS", Fixed("bBB")),
		("PUT_COPY", Fixed("b")),
		("INSERT", None),
		("DELETE", None),
		("ASSOC", Counted),
		("CNTXT_ITEM", Fixed("bB")),
		("ATTR_SET", Fixed("B")),
		("ATTR_ADD", Fixed("B")),
		("ATTR_SUB", Fixed("B")),
		("ATTR_SET_SLOT", Fixed("B")),
		("IATTR_SET_SLOT", Fixed("BB")),
		("PUSH_SLOT_ATTR", Fixed("Bb")),
		("PUSH_GLYPH_ATTR_OBS", Fixed("Bb")),
		("PUSH_GLYPH_METRIC", Fixed("Bbb")),
		("PUSH_FEAT", Fixed("Bb")),
		("PUSH_ATT_TO_GATTR_OBS", Fixed("Bb")),
		("PUSH_ATT_TO_GLYPH_METRIC", Fixed("Bbb")),
		("PUSH_ISLOT_ATTR", Fixed("Bbb")),
		("PUSH_IGLYPH_ATTR", Fixed("Bbb")),
		("POP_RET", None),
		("RET_ZERO", None),
		("RET_TRUE", None),
		("IATTR_SET", Fixed("BB")),
		("IATTR_ADD", Fixed("BB")),
		("IATTR_SUB", Fixed("BB")),
		("PUSH_PROC_STATE", Fixed("B")),
		("PUSH_VERSION", None),
		("PUT_SUBS", Fixed("bHH")),
		("PUT_SUBS2", None),
		("PUT_SUBS3", None),
		("PUT_GLYPH", Fixed("H")),
		("PUSH_GLYPH_ATTR", Fixed("Hb")),
		("PUSH_ATT_TO_GLYPH_ATTR", Fixed("Hb")),
		("BITOR", None),
		("BITAND", None),
		("BITNOT", None),
		("BITSET", Fixed("HH")),
		("SET_FEAT", Fixed("Bb")),
	]
};

fn format_size(c: char) -> usize {
	match c {
		'h' | 'H' => 2,
		'L' => 4,
		_ => 1,
	}
}

/// `disassemble`: the instructions of an action or constraint code, as text.
pub fn disassemble(code: &[u8]) -> Result<Vec<String>> {
	let mut res = Vec::new();
	let mut pc = 0;
	while pc < code.len() {
		let opcode = code[pc] as usize;
		// UPSTREAM-BUG(B71): opcodes past the table are taken as NOP, except the first
		// one, which fails (`>` instead of `>=`).
		let (name, operands) = if opcode > ACODE_INFO.len() {
			ACODE_INFO[0]
		} else {
			*ACODE_INFO
				.get(opcode)
				.ok_or_else(|| Error::ttlib("IndexError: tuple index out of range"))?
		};
		pc += 1;
		if !matches!(operands, Operands::None) && pc >= code.len() {
			return Ok(res);
		}
		let format: Vec<char> = match operands {
			Operands::None => {
				res.push(name.to_owned());
				continue;
			}
			Operands::Counted => {
				let n = code[pc] as usize;
				pc += 1;
				vec!['B'; n]
			}
			Operands::Fixed(f) => f.chars().collect(),
		};
		let size: usize = format.iter().map(|&c| format_size(c)).sum();
		let mut r = at_least(py_tail(code, pc as i64), size)?;
		let mut params = Vec::with_capacity(format.len());
		for c in format {
			params.push(match c {
				'b' => r.i8()? as i64,
				'B' => r.u8()? as i64,
				'h' => r.i16()? as i64,
				'H' => r.u16()? as i64,
				_ => r.u32()? as i64,
			});
		}
		let params: Vec<String> = params.iter().map(i64::to_string).collect();
		res.push(format!("{name}({})", params.join(", ")));
		pc += size;
	}
	Ok(res)
}

/// The instruction name and operand text of a line
/// (`^\s*([^(]+)\s*(?:\(([^)]+)\))?`).
fn match_instruction(line: &str) -> Option<(&str, Option<&str>)> {
	let line = line.trim_start_matches(fontoxide_misc::py::is_space);
	let end = line.find('(').unwrap_or(line.len());
	if end == 0 {
		return None;
	}
	let name = &line[..end];
	let rest = &line[end..];
	let params = rest.strip_prefix('(').and_then(|r| {
		let close = r.find(')')?;
		(close > 0).then(|| &r[..close])
	});
	Some((name, params))
}

/// `assemble`: the code of instructions as text; unknown instructions are
/// skipped.
pub fn assemble<S: AsRef<str>>(instrs: &[S]) -> Result<Vec<u8>> {
	let mut res = Vec::new();
	for inst in instrs {
		let Some((name, params)) = match_instruction(inst.as_ref()) else {
			continue;
		};
		let Some(opcode) = ACODE_INFO.iter().position(|(n, _)| *n == name) else {
			continue;
		};
		res.push(opcode as u8);
		let Some(params) = params else {
			continue;
		};
		let operands = ACODE_INFO[opcode].1;
		if matches!(operands, Operands::None) {
			continue;
		}
		let params: Vec<i64> = params.split(',').map(py_int).collect::<Result<_>>()?;
		match operands {
			Operands::Counted => {
				res.pack_u8(params.len() as i64)?;
				for p in params {
					res.pack_u8(p)?;
				}
			}
			Operands::Fixed(f) => {
				if f.len() != params.len() {
					return Err(Error::Struct(format!(
						"pack expected {} items for packing (got {})",
						f.len(),
						params.len()
					)));
				}
				for (c, p) in f.chars().zip(params) {
					match c {
						'b' => res.pack_i8(p)?,
						'B' => res.pack_u8(p)?,
						'h' => res.pack_i16(p)?,
						'H' => res.pack_u16(p)?,
						_ => res.pack_u32(p)?,
					}
				}
			}
			Operands::None => {}
		}
	}
	Ok(res)
}

/// `writecode`.
fn write_code(tag: &str, writer: &mut XmlWriter, code: &[u8]) -> Result<()> {
	writer.begintag(tag, &[]);
	writer.newline();
	for l in disassemble(code)? {
		writer.write(&l);
		writer.newline();
	}
	writer.endtag(tag);
	writer.newline();
	Ok(())
}

/// `readcode`.
fn read_code(e: &Element) -> Result<Vec<u8>> {
	let text = content_string(e);
	let lines: Vec<&str> = text
		.split('\n')
		.map(fontoxide_misc::py::strip)
		.filter(|l| !l.is_empty())
		.collect();
	assemble(&lines)
}

/// `wrapline`: words on lines of a little over 80 characters.
fn wrap_line(writer: &mut XmlWriter, words: &[String]) {
	let mut line = String::new();
	for w in words {
		if line.chars().count() > 80 {
			line.pop();
			writer.write(&line);
			writer.newline();
			line.clear();
		}
		line.push_str(w);
		line.push(' ');
	}
	if !line.is_empty() {
		line.pop();
		writer.write(&line);
		writer.newline();
	}
}

/// `writesimple`.
fn write_simple(writer: &mut XmlWriter, tag: &str, attrs: &[(&str, i64)]) {
	let attrs: Vec<(&str, &dyn AttrValue)> = attrs
		.iter()
		.map(|(n, v)| (*n, v as &dyn AttrValue))
		.collect();
	writer.simpletag(tag, &attrs);
	writer.newline();
}

/// `getSimple`: sets the fields whose attributes are present.
fn get_simple(e: &Element, fields: &mut [(&str, &mut i64)]) -> Result<()> {
	for (name, field) in fields {
		if let Some(v) = e.attr(name) {
			**field = eval_int(v)?;
		}
	}
	Ok(())
}

/// Python's `hex(n)`.
fn py_hex(n: i64) -> String {
	if n < 0 {
		format!("-0x{:x}", n.unsigned_abs())
	} else {
		format!("0x{n:x}")
	}
}

fn pack_count_error(expected: usize, got: usize) -> Error {
	Error::Struct(format!(
		"pack expected {expected} items for packing (got {got})"
	))
}

/// The start of each item, and the end, from `start` (the `reduce` calls of
/// `Pass.compile`).
fn cumulative<T>(items: &[T], start: i64, len: impl Fn(&T) -> usize) -> Vec<i64> {
	let mut out = Vec::with_capacity(items.len() + 1);
	let mut at = start;
	for item in items {
		out.push(at);
		at += len(item) as i64;
	}
	out.push(at);
	out
}

// ---------------------------------------------------------------------------
// Pass

/// A pass of a [`SilfSubtable`] (`Pass`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pass {
	/// `flags`.
	pub flags: i64,
	/// `maxRuleLoop`.
	pub max_rule_loop: i64,
	/// `maxRuleContext`.
	pub max_rule_context: i64,
	/// `maxBackup`.
	pub max_backup: i64,
	/// `numRules`, set when compiling.
	pub num_rules: i64,
	/// `fsmOffset`, set when compiling.
	pub fsm_offset: i64,
	/// `pcCode`, set when compiling.
	pub pc_code: i64,
	/// `rcCode`, set when compiling.
	pub rc_code: i64,
	/// `aCode`, set when compiling.
	pub a_code: i64,
	/// `oDebug`, set when compiling.
	pub o_debug: i64,
	/// `numRows`.
	pub num_rows: i64,
	/// `numTransitional`.
	pub num_transitional: i64,
	/// `numSuccess`.
	pub num_success: i64,
	/// `numColumns`.
	pub num_columns: i64,
	/// The column of each glyph (`colMap`).
	pub col_map: BTreeMap<u32, i64>,
	/// The rules of each success state (`rules`).
	pub rules: Vec<Vec<i64>>,
	/// `rulePreContexts`.
	pub rule_pre_contexts: Vec<i64>,
	/// `ruleSortKeys`.
	pub rule_sort_keys: Vec<i64>,
	/// The constraint code of each rule (`ruleConstraints`).
	pub rule_constraints: Vec<Vec<u8>>,
	/// `passConstraints`.
	pub pass_constraints: Vec<u8>,
	/// The action code of each rule (`actions`).
	pub actions: Vec<Vec<u8>>,
	/// The rows of the transition table (`stateTrans`).
	pub state_trans: Vec<Vec<u16>>,
	/// `startStates`.
	pub start_states: Vec<i64>,
	/// `minRulePreContext`.
	pub min_rule_pre_context: i64,
	/// `maxRulePreContext`.
	pub max_rule_pre_context: i64,
	/// `collisionThreshold`.
	pub collision_threshold: i64,
}

impl Pass {
	/// `Pass.decompile`.
	pub fn decompile(data: &[u8], _version: f64) -> Result<Self> {
		let mut p = Pass::default();
		let mut r = at_least(data, 32)?;
		p.flags = r.u8()? as i64;
		p.max_rule_loop = r.u8()? as i64;
		p.max_rule_context = r.u8()? as i64;
		p.max_backup = r.u8()? as i64;
		p.num_rules = r.u16()? as i64;
		p.fsm_offset = r.u16()? as i64;
		p.pc_code = r.u32()? as i64;
		p.rc_code = r.u32()? as i64;
		p.a_code = r.u32()? as i64;
		p.o_debug = r.u32()? as i64;
		p.num_rows = r.u16()? as i64;
		p.num_transitional = r.u16()? as i64;
		p.num_success = r.u16()? as i64;
		p.num_columns = r.u16()? as i64;
		let data = r.rest();
		let num_range = exact(py_slice(data, 0, 8), 8)?.u16()? as i64;
		let data = py_tail(data, 8);
		for i in 0..num_range {
			let mut r = exact(py_slice(data, 6 * i, 6 * i + 6), 6)?;
			let (first, last, col) = (r.u16()? as u32, r.u16()? as u32, r.u16()? as i64);
			for g in first..=last {
				p.col_map.insert(g, col);
			}
		}
		let data = py_tail(data, 6 * num_range);
		let ns = p.num_success as usize;
		let o_rule_map: Vec<i64> = at_least(data, 2 * (ns + 1))?
			.u16_array(ns + 1)?
			.into_iter()
			.map(i64::from)
			.collect();
		let data = py_tail(data, 2 + 2 * ns as i64);
		let total = o_rule_map[ns];
		let rules: Vec<i64> = at_least(data, 2 * total as usize)?
			.u16_array(total as usize)?
			.into_iter()
			.map(i64::from)
			.collect();
		p.rules = o_rule_map
			.windows(2)
			.map(|w| py_slice(&rules, w[0], w[1]).to_vec())
			.collect();
		let data = py_tail(data, 2 * total);
		let mut r = exact(py_slice(data, 0, 2), 2)?;
		p.min_rule_pre_context = r.u8()? as i64;
		p.max_rule_pre_context = r.u8()? as i64;
		let n = p.max_rule_pre_context - p.min_rule_pre_context + 1;
		let nss = count(n)?;
		p.start_states = exact(py_slice(data, 2, 2 + n * 2), 2 * nss)?
			.u16_array(nss)?
			.into_iter()
			.map(i64::from)
			.collect();
		let data = py_tail(data, 2 + n * 2);
		let nr = p.num_rules;
		let nru = nr as usize;
		p.rule_sort_keys = exact(py_slice(data, 0, 2 * nr), 2 * nru)?
			.u16_array(nru)?
			.into_iter()
			.map(i64::from)
			.collect();
		let data = py_tail(data, 2 * nr);
		p.rule_pre_contexts = exact(py_slice(data, 0, nr), nru)?
			.bytes(nru)?
			.iter()
			.map(|&b| b as i64)
			.collect();
		let data = py_tail(data, nr);
		let mut r = exact(py_slice(data, 0, 3), 3)?;
		p.collision_threshold = r.u8()? as i64;
		let p_constraint = r.u16()? as i64;
		let mut o_constraints: Vec<i64> = exact(py_slice(data, 3, 5 + nr * 2), 2 * (nru + 1))?
			.u16_array(nru + 1)?
			.into_iter()
			.map(i64::from)
			.collect();
		let data = py_tail(data, 5 + nr * 2);
		let o_actions: Vec<i64> = exact(py_slice(data, 0, 2 + nr * 2), 2 * (nru + 1))?
			.u16_array(nru + 1)?
			.into_iter()
			.map(i64::from)
			.collect();
		let data = py_tail(data, 2 * nr + 2);
		let row = p.num_columns * 2;
		for i in 0..p.num_transitional {
			let chunk = py_slice(data, i * row, (i + 1) * row);
			if chunk.len() % 2 != 0 {
				return Err(Error::value("bytes length not a multiple of item size"));
			}
			p.state_trans
				.push(Reader::new(chunk).u16_array(chunk.len() / 2)?);
		}
		let data = py_tail(data, p.num_transitional * row + 1);
		p.pass_constraints = py_slice(data, 0, p_constraint).to_vec();
		let data = py_tail(data, p_constraint);
		for i in (0..o_constraints.len() - 1).rev() {
			if o_constraints[i] == 0 {
				o_constraints[i] = o_constraints[i + 1];
			}
		}
		let codes = |data: &[u8], offsets: &[i64]| -> Vec<Vec<u8>> {
			offsets
				.windows(2)
				.map(|w| {
					if w[1] - w[0] > 1 {
						py_slice(data, w[0], w[1]).to_vec()
					} else {
						Vec::new()
					}
				})
				.collect()
		};
		p.rule_constraints = codes(data, &o_constraints);
		let data = py_tail(data, o_constraints[nru]);
		p.actions = codes(data, &o_actions);
		// Not using debug.
		Ok(p)
	}

	/// `Pass.compile`; `base` is the offset of the pass in the subtable.
	pub fn compile(&mut self, base: i64, _version: f64) -> Result<Vec<u8>> {
		// Build it all up backwards.
		let o_actions = cumulative(&self.actions, 0, Vec::len);
		let o_constraints = cumulative(&self.rule_constraints, 1, Vec::len);
		let mut constraint_code = vec![0u8];
		for c in &self.rule_constraints {
			constraint_code.extend_from_slice(c);
		}
		let mut transes = Vec::new();
		for t in &self.state_trans {
			for v in t {
				transes.extend_from_slice(&v.to_be_bytes());
			}
		}
		if self.state_trans.is_empty() {
			self.start_states = vec![0];
		}
		let o_rule_map = cumulative(&self.rules, 0, Vec::len);
		let gid_col_map: BTreeMap<i64, i64> =
			self.col_map.iter().map(|(&g, &c)| (g as i64, c)).collect();
		let pass_ranges: Vec<(i64, i64, i64)> = entries(&gid_col_map, true)
			.into_iter()
			.filter(|e| e.1 != 0)
			.map(|(first, n, vals)| (first, first + n as i64 - 1, vals[0]))
			.collect();
		let nr = self.actions.len();
		self.num_rules = nr as i64;
		let nr = nr as i64;
		let total = o_rule_map[o_rule_map.len() - 1];
		self.fsm_offset = 32
			+ 8 + pass_ranges.len() as i64 * 6
			+ o_rule_map.len() as i64 * 2
			+ 2 * total
			+ 2 + 2 * self.start_states.len() as i64
			+ 3 * nr + 3
			+ 4 * nr + 4;
		self.pc_code = self.fsm_offset + 2 * self.num_transitional * self.num_columns + 1 + base;
		self.rc_code = self.pc_code + self.pass_constraints.len() as i64;
		self.a_code = self.rc_code + constraint_code.len() as i64;
		self.o_debug = 0;
		let mut data = Vec::new();
		for v in [
			self.flags,
			self.max_rule_loop,
			self.max_rule_context,
			self.max_backup,
		] {
			data.pack_u8(v)?;
		}
		data.pack_u16(self.num_rules)?;
		data.pack_u16(self.fsm_offset)?;
		for v in [self.pc_code, self.rc_code, self.a_code, self.o_debug] {
			data.pack_u32(v)?;
		}
		for v in [
			self.num_rows,
			self.num_transitional,
			self.num_success,
			self.num_columns,
		] {
			data.pack_u16(v)?;
		}
		data.extend(bininfo(pass_ranges.len() as i64, 6)?);
		for (first, last, col) in &pass_ranges {
			data.pack_u16(*first)?;
			data.pack_u16(*last)?;
			data.pack_u16(*col)?;
		}
		for v in &o_rule_map {
			data.pack_u16(*v)?;
		}
		for v in self.rules.iter().flatten() {
			data.pack_u16(*v)?;
		}
		data.pack_u8(self.min_rule_pre_context)?;
		data.pack_u8(self.max_rule_pre_context)?;
		for v in &self.start_states {
			data.pack_u16(*v)?;
		}
		if self.rule_sort_keys.len() as i64 != nr {
			return Err(pack_count_error(nr as usize, self.rule_sort_keys.len()));
		}
		for v in &self.rule_sort_keys {
			data.pack_u16(*v)?;
		}
		if self.rule_pre_contexts.len() as i64 != nr {
			return Err(pack_count_error(nr as usize, self.rule_pre_contexts.len()));
		}
		for v in &self.rule_pre_contexts {
			data.pack_u8(*v)?;
		}
		data.pack_u8(self.collision_threshold)?;
		data.pack_u16(self.pass_constraints.len() as i64)?;
		if o_constraints.len() as i64 != nr + 1 {
			return Err(pack_count_error(nr as usize + 1, o_constraints.len()));
		}
		for v in o_constraints.iter().chain(&o_actions) {
			data.pack_u16(*v)?;
		}
		data.extend(transes);
		data.push(0);
		data.extend_from_slice(&self.pass_constraints);
		data.extend(constraint_code);
		for a in &self.actions {
			data.extend_from_slice(a);
		}
		Ok(data)
	}

	/// `Pass.toXML`.
	pub fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font, _version: f64) -> Result<()> {
		write_simple(
			writer,
			"info",
			&[
				("flags", self.flags),
				("maxRuleLoop", self.max_rule_loop),
				("maxRuleContext", self.max_rule_context),
				("maxBackup", self.max_backup),
				("minRulePreContext", self.min_rule_pre_context),
				("maxRulePreContext", self.max_rule_pre_context),
				("collisionThreshold", self.collision_threshold),
			],
		);
		write_simple(
			writer,
			"fsminfo",
			&[
				("numRows", self.num_rows),
				("numTransitional", self.num_transitional),
				("numSuccess", self.num_success),
				("numColumns", self.num_columns),
			],
		);
		let order = font.glyph_order()?;
		writer.begintag("colmap", &[]);
		writer.newline();
		let words: Vec<String> = self
			.col_map
			.iter()
			.map(|(&g, c)| format!("{}={c}", order.name(g)))
			.collect();
		wrap_line(writer, &words);
		writer.endtag("colmap");
		writer.newline();
		writer.begintag("staterulemap", &[]);
		writer.newline();
		for (i, r) in self.rules.iter().enumerate() {
			let rules: Vec<String> = r.iter().map(i64::to_string).collect();
			writer.simpletag(
				"state",
				&[
					("number", &(self.num_rows - self.num_success + i as i64)),
					("rules", &rules.join(" ")),
				],
			);
			writer.newline();
		}
		writer.endtag("staterulemap");
		writer.newline();
		writer.begintag("rules", &[]);
		writer.newline();
		for (i, action) in self.actions.iter().enumerate() {
			let index = i as i64;
			writer.begintag(
				"rule",
				&[
					("index", &index),
					("precontext", py_get(&self.rule_pre_contexts, index)?),
					("sortkey", py_get(&self.rule_sort_keys, index)?),
				],
			);
			writer.newline();
			let constraint = py_get(&self.rule_constraints, index)?;
			if !constraint.is_empty() {
				write_code("constraint", writer, constraint)?;
			}
			write_code("action", writer, action)?;
			writer.endtag("rule");
			writer.newline();
		}
		writer.endtag("rules");
		writer.newline();
		if !self.pass_constraints.is_empty() {
			write_code("passConstraint", writer, &self.pass_constraints)?;
		}
		if !self.state_trans.is_empty() {
			writer.begintag("fsm", &[]);
			writer.newline();
			writer.begintag("starts", &[]);
			let starts: Vec<String> = self.start_states.iter().map(i64::to_string).collect();
			writer.write(&starts.join(" "));
			writer.endtag("starts");
			writer.newline();
			for (i, s) in self.state_trans.iter().enumerate() {
				writer.begintag("row", &[("_i", &i)]);
				// No newlines here.
				let row: Vec<String> = s.iter().map(u16::to_string).collect();
				writer.write(&row.join(" "));
				writer.endtag("row");
				writer.newline();
			}
			writer.endtag("fsm");
			writer.newline();
		}
		Ok(())
	}

	/// `Pass.fromXML`.
	pub fn from_xml(&mut self, e: &Element, font: &mut Font, _version: f64) -> Result<()> {
		match e.name.as_str() {
			"info" => get_simple(
				e,
				&mut [
					("flags", &mut self.flags),
					("maxRuleLoop", &mut self.max_rule_loop),
					("maxRuleContext", &mut self.max_rule_context),
					("maxBackup", &mut self.max_backup),
					("minRulePreContext", &mut self.min_rule_pre_context),
					("maxRulePreContext", &mut self.max_rule_pre_context),
					("collisionThreshold", &mut self.collision_threshold),
				],
			)?,
			"fsminfo" => get_simple(
				e,
				&mut [
					("numRows", &mut self.num_rows),
					("numTransitional", &mut self.num_transitional),
					("numSuccess", &mut self.num_success),
					("numColumns", &mut self.num_columns),
				],
			)?,
			"colmap" => {
				for w in py_split(&content_string(e)) {
					let x: Vec<&str> = w.split('=').collect();
					if x.len() != 2 || x[0].is_empty() || x[1].is_empty() {
						continue;
					}
					let gid = font.glyph_id(x[0])?;
					self.col_map.insert(gid, py_int(x[1])?);
				}
			}
			"staterulemap" => {
				for c in e.elements().filter(|c| c.name == "state") {
					self.rules.push(
						attr(c, "rules")?
							.split(' ')
							.map(py_int)
							.collect::<Result<_>>()?,
					);
				}
			}
			"rules" => {
				for c in e.elements().filter(|c| c.name == "rule") {
					self.rule_pre_contexts.push(py_int(attr(c, "precontext")?)?);
					self.rule_sort_keys.push(py_int(attr(c, "sortkey")?)?);
					let mut constraint = Vec::new();
					let mut action = Vec::new();
					for code in c.elements() {
						match code.name.as_str() {
							"constraint" => constraint = read_code(code)?,
							"action" => action = read_code(code)?,
							_ => {}
						}
					}
					self.actions.push(action);
					self.rule_constraints.push(constraint);
				}
			}
			"passConstraint" => self.pass_constraints = read_code(e)?,
			"fsm" => {
				for c in e.elements() {
					match c.name.as_str() {
						"row" => {
							let row = py_split(&content_string(c))
								.map(|v| {
									let v = py_int(v)?;
									u16::try_from(v).map_err(|_| {
										Error::ttlib(if v < 0 {
											"OverflowError: unsigned short is less than minimum"
										} else {
											"OverflowError: unsigned short is greater than maximum"
										})
									})
								})
								.collect::<Result<_>>()?;
							self.state_trans.push(row);
						}
						"starts" => {
							self.start_states = py_split(&content_string(c))
								.map(py_int)
								.collect::<Result<_>>()?;
						}
						_ => {}
					}
				}
			}
			_ => {}
		}
		Ok(())
	}
}

// ---------------------------------------------------------------------------
// Classes

/// The glyph classes of a [`SilfSubtable`] (`Classes`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Classes {
	/// `numClass`, as read or compiled.
	pub num_class: Option<i64>,
	/// `numLinear`, as read or compiled; the TTX numbers the non-linear classes
	/// after the linear ones with it.
	pub num_linear: Option<i64>,
	/// The linear classes: glyphs in order (`linear`).
	pub linear: Vec<Vec<u32>>,
	/// The non-linear classes: glyph and index, in insertion order
	/// (`nonLinear`).
	pub non_linear: Vec<Vec<(u32, i64)>>,
}

impl Classes {
	/// `Classes.decompile`.
	pub fn decompile(data: &[u8], version: f64) -> Result<Self> {
		let mut r = at_least(data, 4)?;
		let nc = r.u16()? as i64;
		let nl = r.u16()? as i64;
		let n = nc as usize + 1;
		let o_classes: Vec<i64> = if version >= 4.0 {
			exact(py_slice(data, 4, 8 + 4 * nc), 4 * n)?
				.u32_array(n)?
				.into_iter()
				.map(i64::from)
				.collect()
		} else {
			exact(py_slice(data, 4, 6 + 2 * nc), 2 * n)?
				.u16_array(n)?
				.into_iter()
				.map(i64::from)
				.collect()
		};
		let mut classes = Classes {
			num_class: Some(nc),
			num_linear: Some(nl),
			..Default::default()
		};
		let starts = py_slice(&o_classes, 0, nl);
		let ends = py_slice(&o_classes, 1, nl + 1);
		for (&s, &e) in starts.iter().zip(ends) {
			// `"%d" % ((e - s) / 2)`.
			let n = count(((e - s) as f64 / 2.0).trunc() as i64)?;
			let glyphs = exact(py_slice(data, s, e), 2 * n)?.u16_array(n)?;
			classes
				.linear
				.push(glyphs.into_iter().map(u32::from).collect());
		}
		let starts = py_slice(&o_classes, nl, nc);
		let ends = py_slice(&o_classes, nl + 1, nc + 1);
		for (&s, &e) in starts.iter().zip(ends) {
			let mut non_linear = Vec::new();
			let mut x = s + 8;
			while x < e {
				let mut r = exact(py_slice(data, x, x + 4), 4)?;
				let glyph = r.u16()? as u32;
				dict_set(&mut non_linear, glyph, r.u16()? as i64);
				x += 4;
			}
			classes.non_linear.push(non_linear);
		}
		Ok(classes)
	}

	/// `Classes.compile`.
	pub fn compile(&mut self, version: f64) -> Result<Vec<u8>> {
		let mut data = Vec::new();
		let mut o_classes = Vec::new();
		let n = (self.linear.len() + self.non_linear.len()) as i64;
		let offset = if version >= 4.0 { 8 + 4 * n } else { 6 + 2 * n };
		for l in &self.linear {
			o_classes.push(data.len() as i64 + offset);
			for &g in l {
				data.pack_u16(g as i64)?;
			}
		}
		for l in &self.non_linear {
			o_classes.push(data.len() as i64 + offset);
			let mut gs: Vec<(i64, i64)> = l.iter().map(|&(g, i)| (g as i64, i)).collect();
			data.extend(bininfo(gs.len() as i64, 1)?);
			gs.sort();
			for (g, i) in gs {
				data.pack_u16(g)?;
				data.pack_u16(i)?;
			}
		}
		o_classes.push(data.len() as i64 + offset);
		let num_class = o_classes.len() as i64 - 1;
		self.num_class = Some(num_class);
		self.num_linear = Some(self.linear.len() as i64);
		let mut out = Vec::new();
		out.pack_u16(num_class)?;
		out.pack_u16(self.linear.len() as i64)?;
		for o in o_classes {
			if version >= 4.0 {
				out.pack_u32(o)?;
			} else {
				out.pack_u16(o)?;
			}
		}
		out.extend(data);
		Ok(out)
	}

	/// `Classes.toXML`.
	pub fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font, _version: f64) -> Result<()> {
		let order = font.glyph_order()?;
		writer.begintag("classes", &[]);
		writer.newline();
		writer.begintag("linearClasses", &[]);
		writer.newline();
		for (i, l) in self.linear.iter().enumerate() {
			writer.begintag("linear", &[("_index", &i)]);
			writer.newline();
			let names: Vec<String> = l.iter().map(|&g| order.name(g).into_owned()).collect();
			wrap_line(writer, &names);
			writer.endtag("linear");
			writer.newline();
		}
		writer.endtag("linearClasses");
		writer.newline();
		writer.begintag("nonLinearClasses", &[]);
		writer.newline();
		for (i, l) in self.non_linear.iter().enumerate() {
			let num_linear = self.num_linear.ok_or_else(|| {
				Error::ttlib("AttributeError: 'Classes' object has no attribute 'numLinear'")
			})?;
			writer.begintag("nonLinear", &[("_index", &(i as i64 + num_linear))]);
			writer.newline();
			for (g, index) in l {
				writer.simpletag("map", &[("glyph", &order.name(*g)), ("index", index)]);
				writer.newline();
			}
			writer.endtag("nonLinear");
			writer.newline();
		}
		writer.endtag("nonLinearClasses");
		writer.newline();
		writer.endtag("classes");
		writer.newline();
		Ok(())
	}

	/// `Classes.fromXML`.
	pub fn from_xml(&mut self, e: &Element, font: &mut Font, _version: f64) -> Result<()> {
		match e.name.as_str() {
			"linearClasses" => {
				for c in e.elements().filter(|c| c.name == "linear") {
					let glyphs = py_split(&content_string(c))
						.map(|n| font.glyph_id(n))
						.collect::<Result<_>>()?;
					self.linear.push(glyphs);
				}
			}
			"nonLinearClasses" => {
				for c in e.elements().filter(|c| c.name == "nonLinear") {
					let mut l = Vec::new();
					for m in c.elements().filter(|m| m.name == "map") {
						let glyph = font.glyph_id(attr(m, "glyph")?)?;
						dict_set(&mut l, glyph, eval_int(attr(m, "index")?)?);
					}
					self.non_linear.push(l);
				}
			}
			_ => {}
		}
		Ok(())
	}

	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		for l in &mut self.linear {
			for g in l {
				*g = f(*g)?;
			}
		}
		for l in &mut self.non_linear {
			for (g, _) in l {
				*g = f(*g)?;
			}
		}
		Ok(())
	}
}

// ---------------------------------------------------------------------------
// Silf subtables

/// A justification level (`Silf_justify_format`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Justify {
	/// `attrStretch`.
	pub attr_stretch: i64,
	/// `attrShrink`.
	pub attr_shrink: i64,
	/// `attrStep`.
	pub attr_step: i64,
	/// `attrWeight`.
	pub attr_weight: i64,
	/// `runto`.
	pub runto: i64,
}

impl Justify {
	const NAMES: [&'static str; 5] = [
		"attrStretch",
		"attrShrink",
		"attrStep",
		"attrWeight",
		"runto",
	];

	fn fields(&self) -> [i64; 5] {
		[
			self.attr_stretch,
			self.attr_shrink,
			self.attr_step,
			self.attr_weight,
			self.runto,
		]
	}

	fn fields_mut(&mut self) -> [&mut i64; 5] {
		[
			&mut self.attr_stretch,
			&mut self.attr_shrink,
			&mut self.attr_step,
			&mut self.attr_weight,
			&mut self.runto,
		]
	}
}

/// A `Silf` subtable (`Silf`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SilfSubtable {
	/// `ruleVersion` (version 3 and later).
	pub rule_version: f64,
	/// `passOffset`, set when compiling.
	pub pass_offset: i64,
	/// `pseudosOffset`, set when compiling.
	pub pseudos_offset: i64,
	/// `maxGlyphID`.
	pub max_glyph_id: i64,
	/// `extraAscent`.
	pub extra_ascent: i64,
	/// `extraDescent`.
	pub extra_descent: i64,
	/// `numPasses`, set when compiling.
	pub num_passes: i64,
	/// `iSubst`.
	pub i_subst: i64,
	/// `iPos`.
	pub i_pos: i64,
	/// `iJust`.
	pub i_just: i64,
	/// `iBidi`.
	pub i_bidi: i64,
	/// `flags`.
	pub flags: i64,
	/// `maxPreContext`.
	pub max_pre_context: i64,
	/// `maxPostContext`.
	pub max_post_context: i64,
	/// `attrPseudo`.
	pub attr_pseudo: i64,
	/// `attrBreakWeight`.
	pub attr_break_weight: i64,
	/// `attrDirectionality`.
	pub attr_directionality: i64,
	/// `attrMirroring`.
	pub attr_mirroring: i64,
	/// `attrSkipPasses`.
	pub attr_skip_passes: i64,
	/// `numJLevels`, set when compiling.
	pub num_j_levels: i64,
	/// `jLevels`.
	pub j_levels: Vec<Justify>,
	/// `numLigComp`.
	pub num_lig_comp: i64,
	/// `numUserDefn`.
	pub num_user_defn: i64,
	/// `maxCompPerLig`.
	pub max_comp_per_lig: i64,
	/// `direction`.
	pub direction: i64,
	/// `attCollisions`.
	pub att_collisions: i64,
	/// `numCritFeatures`, set when compiling.
	pub num_crit_features: i64,
	/// `critFeatures`.
	pub crit_features: Vec<i64>,
	/// `scriptTags`.
	pub script_tags: Vec<String>,
	/// `lbGID`.
	pub lb_gid: i64,
	/// `oPasses`, as read.
	pub o_passes: Vec<i64>,
	/// The pseudo-glyph of each code point (`pMap`).
	pub p_map: BTreeMap<i64, u32>,
	/// `classes`.
	pub classes: Classes,
	/// `passes`.
	pub passes: Vec<Pass>,
}

fn script_tag(data: &[u8]) -> Result<String> {
	if !data.is_ascii() {
		return Err(Error::value(
			"UnicodeDecodeError: 'ascii' codec can't decode byte",
		));
	}
	Ok(data.iter().map(|&b| b as char).collect())
}

impl SilfSubtable {
	/// `Silf.decompile`.
	pub fn decompile(data: &[u8], version: f64) -> Result<Self> {
		let mut s = SilfSubtable::default();
		let mut data = data;
		if version >= 3.0 {
			let mut r = at_least(data, 8)?;
			s.rule_version = round_version(read_fixed(&mut r)?)?;
			s.pass_offset = r.u16()? as i64;
			s.pseudos_offset = r.u16()? as i64;
			data = r.rest();
		}
		let mut r = at_least(data, 20)?;
		s.max_glyph_id = r.u16()? as i64;
		s.extra_ascent = r.i16()? as i64;
		s.extra_descent = r.i16()? as i64;
		for f in [
			&mut s.num_passes,
			&mut s.i_subst,
			&mut s.i_pos,
			&mut s.i_just,
			&mut s.i_bidi,
			&mut s.flags,
			&mut s.max_pre_context,
			&mut s.max_post_context,
			&mut s.attr_pseudo,
			&mut s.attr_break_weight,
			&mut s.attr_directionality,
			&mut s.attr_mirroring,
			&mut s.attr_skip_passes,
			&mut s.num_j_levels,
		] {
			*f = r.u8()? as i64;
		}
		data = r.rest();
		for _ in 0..s.num_j_levels {
			let mut r = at_least(data, 8)?;
			let mut j = Justify::default();
			for f in j.fields_mut() {
				*f = r.u8()? as i64;
			}
			s.j_levels.push(j);
			data = &data[8..];
		}
		let mut r = at_least(data, 10)?;
		s.num_lig_comp = r.u16()? as i64;
		s.num_user_defn = r.u8()? as i64;
		s.max_comp_per_lig = r.u8()? as i64;
		s.direction = r.u8()? as i64;
		s.att_collisions = r.u8()? as i64;
		r.skip(3)?;
		s.num_crit_features = r.u8()? as i64;
		data = r.rest();
		let ncf = s.num_crit_features;
		if ncf != 0 {
			s.crit_features = at_least(data, 2 * ncf as usize)?
				.u16_array(ncf as usize)?
				.into_iter()
				.map(i64::from)
				.collect();
		}
		data = py_tail(data, ncf * 2 + 1);
		let nst = at_least(data, 1)?.u8()? as i64;
		for x in (1..1 + 4 * nst).step_by(4) {
			s.script_tags
				.push(script_tag(exact(py_slice(data, x, x + 4), 4)?.bytes(4)?)?);
		}
		data = py_tail(data, 1 + 4 * nst);
		s.lb_gid = exact(py_slice(data, 0, 2), 2)?.u16()? as i64;
		let np = s.num_passes;
		if np != 0 {
			let n = np as usize + 1;
			s.o_passes = exact(py_slice(data, 2, 6 + 4 * np), 4 * n)?
				.u32_array(n)?
				.into_iter()
				.map(i64::from)
				.collect();
		}
		data = py_tail(data, 6 + 4 * np);
		let num_pseudo = exact(py_slice(data, 0, 2), 2)?.u16()? as i64;
		for i in 0..num_pseudo {
			let (unicode, pseudo) = if version >= 3.0 {
				let mut r = exact(py_slice(data, 8 + 6 * i, 14 + 6 * i), 6)?;
				(r.u32()? as i64, r.u16()? as u32)
			} else {
				let mut r = exact(py_slice(data, 8 + 4 * i, 12 + 4 * i), 4)?;
				(r.u16()? as i64, r.u16()? as u32)
			};
			s.p_map.insert(unicode, pseudo);
		}
		// UPSTREAM-BUG(B70): before version 3, the pseudo-glyph entries are read as 4
		// bytes but skipped as 6, so the classes and passes after them are misread.
		data = py_tail(data, 8 + 6 * num_pseudo);
		let mut currpos = 20
			+ 8 * s.num_j_levels
			+ 10 + 2 * ncf
			+ 1 + 1 + 4 * nst
			+ 6 + 4 * np
			+ 8 + 6 * num_pseudo;
		if version >= 3.0 {
			currpos += 8;
		}
		s.classes = Classes::decompile(data, version)?;
		for i in 0..np as usize {
			let pass = py_slice(data, s.o_passes[i] - currpos, s.o_passes[i + 1] - currpos);
			s.passes.push(Pass::decompile(pass, version)?);
		}
		Ok(s)
	}

	/// `Silf.compile`.
	pub fn compile(&mut self, version: f64) -> Result<Vec<u8>> {
		self.num_passes = self.passes.len() as i64;
		self.num_j_levels = self.j_levels.len() as i64;
		self.num_crit_features = self.crit_features.len() as i64;
		let num_pseudo = self.p_map.len() as i64;
		let hdroffset = if version >= 3.0 { 8 } else { 0 };
		let mut data = Vec::new();
		data.pack_u16(self.max_glyph_id)?;
		data.pack_i16(self.extra_ascent)?;
		data.pack_i16(self.extra_descent)?;
		for v in [
			self.num_passes,
			self.i_subst,
			self.i_pos,
			self.i_just,
			self.i_bidi,
			self.flags,
			self.max_pre_context,
			self.max_post_context,
			self.attr_pseudo,
			self.attr_break_weight,
			self.attr_directionality,
			self.attr_mirroring,
			self.attr_skip_passes,
			self.num_j_levels,
		] {
			data.pack_u8(v)?;
		}
		for j in &self.j_levels {
			for v in j.fields() {
				data.pack_u8(v)?;
			}
			data.extend_from_slice(&[0; 3]);
		}
		data.pack_u16(self.num_lig_comp)?;
		for v in [
			self.num_user_defn,
			self.max_comp_per_lig,
			self.direction,
			self.att_collisions,
		] {
			data.pack_u8(v)?;
		}
		data.extend_from_slice(&[0; 3]);
		data.pack_u8(self.num_crit_features)?;
		// LENIENT(L49): fontTools misspells `numCritFeatures` here (UPSTREAM-BUG), so
		// it cannot compile subtables with critical features.
		for &v in &self.crit_features {
			data.pack_u16(v)?;
		}
		data.pack_u8(0)?;
		data.pack_u8(self.script_tags.len() as i64)?;
		for t in &self.script_tags {
			if !t.is_ascii() {
				return Err(Error::value(
					"UnicodeEncodeError: 'ascii' codec can't encode character",
				));
			}
			data.pack_4s(t.as_bytes());
		}
		data.pack_u16(self.lb_gid)?;
		self.pass_offset = data.len() as i64;

		let mut data1 = bininfo(num_pseudo, 6)?;
		let mut currpos = hdroffset + data.len() as i64 + 4 * (self.num_passes + 1);
		self.pseudos_offset = currpos + data1.len() as i64;
		for (&u, &p) in &self.p_map {
			if version >= 3.0 {
				data1.pack_u32(u)?;
			} else {
				data1.pack_u16(u)?;
			}
			data1.pack_u16(p as i64)?;
		}
		data1.extend(self.classes.compile(version)?);
		currpos += data1.len() as i64;
		let mut data2 = Vec::new();
		let mut datao = Vec::new();
		for p in &mut self.passes {
			let base = currpos + data2.len() as i64;
			datao.pack_u32(base)?;
			data2.extend(p.compile(base, version)?);
		}
		datao.pack_u32(currpos + data2.len() as i64)?;

		let mut out = Vec::new();
		if version >= 3.0 {
			out.pack_fixed(self.rule_version)?;
			out.pack_u16(self.pass_offset)?;
			out.pack_u16(self.pseudos_offset)?;
		}
		out.extend(data);
		out.extend(datao);
		out.extend(data1);
		out.extend(data2);
		Ok(out)
	}

	/// `Silf.toXML`.
	pub fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font, version: f64) -> Result<()> {
		if version >= 3.0 {
			writer.simpletag("version", &[("ruleVersion", &self.rule_version)]);
			writer.newline();
		}
		write_simple(
			writer,
			"info",
			&[
				("flags", self.flags),
				("extraAscent", self.extra_ascent),
				("extraDescent", self.extra_descent),
				("maxGlyphID", self.max_glyph_id),
				("numLigComp", self.num_lig_comp),
				("numUserDefn", self.num_user_defn),
				("maxCompPerLig", self.max_comp_per_lig),
				("direction", self.direction),
				("lbGID", self.lb_gid),
			],
		);
		write_simple(
			writer,
			"passindexes",
			&[
				("iSubst", self.i_subst),
				("iPos", self.i_pos),
				("iJust", self.i_just),
				("iBidi", self.i_bidi),
			],
		);
		write_simple(
			writer,
			"contexts",
			&[
				("maxPreContext", self.max_pre_context),
				("maxPostContext", self.max_post_context),
			],
		);
		write_simple(
			writer,
			"attributes",
			&[
				("attrPseudo", self.attr_pseudo),
				("attrBreakWeight", self.attr_break_weight),
				("attrDirectionality", self.attr_directionality),
				("attrMirroring", self.attr_mirroring),
				("attrSkipPasses", self.attr_skip_passes),
				("attCollisions", self.att_collisions),
			],
		);
		if !self.j_levels.is_empty() {
			writer.begintag("justifications", &[]);
			writer.newline();
			for j in &self.j_levels {
				let attrs: Vec<(&str, i64)> = Justify::NAMES.into_iter().zip(j.fields()).collect();
				write_simple(writer, "justify", &attrs);
			}
			writer.endtag("justifications");
			writer.newline();
		}
		if !self.crit_features.is_empty() {
			writer.begintag("critFeatures", &[]);
			writer.newline();
			let v: Vec<String> = self.crit_features.iter().map(i64::to_string).collect();
			writer.write(&v.join(" "));
			writer.newline();
			writer.endtag("critFeatures");
			writer.newline();
		}
		if !self.script_tags.is_empty() {
			writer.begintag("scriptTags", &[]);
			writer.newline();
			writer.write(&self.script_tags.join(" "));
			writer.newline();
			writer.endtag("scriptTags");
			writer.newline();
		}
		if !self.p_map.is_empty() {
			let order = font.glyph_order()?;
			writer.begintag("pseudoMap", &[]);
			writer.newline();
			for (&k, &v) in &self.p_map {
				writer.simpletag(
					"pseudo",
					&[("unicode", &py_hex(k)), ("pseudo", &order.name(v))],
				);
				writer.newline();
			}
			writer.endtag("pseudoMap");
			writer.newline();
		}
		self.classes.to_xml(writer, font, version)?;
		if !self.passes.is_empty() {
			writer.begintag("passes", &[]);
			writer.newline();
			for (i, p) in self.passes.iter().enumerate() {
				writer.begintag("pass", &[("_index", &i)]);
				writer.newline();
				p.to_xml(writer, font, version)?;
				writer.endtag("pass");
				writer.newline();
			}
			writer.endtag("passes");
			writer.newline();
		}
		Ok(())
	}

	/// `Silf.fromXML`.
	pub fn from_xml(&mut self, e: &Element, font: &mut Font, version: f64) -> Result<()> {
		match e.name.as_str() {
			"version" => self.rule_version = eval_float(e.attr("ruleVersion").unwrap_or("0"))?,
			"info" => get_simple(
				e,
				&mut [
					("flags", &mut self.flags),
					("extraAscent", &mut self.extra_ascent),
					("extraDescent", &mut self.extra_descent),
					("maxGlyphID", &mut self.max_glyph_id),
					("numLigComp", &mut self.num_lig_comp),
					("numUserDefn", &mut self.num_user_defn),
					("maxCompPerLig", &mut self.max_comp_per_lig),
					("direction", &mut self.direction),
					("lbGID", &mut self.lb_gid),
				],
			)?,
			"passindexes" => get_simple(
				e,
				&mut [
					("iSubst", &mut self.i_subst),
					("iPos", &mut self.i_pos),
					("iJust", &mut self.i_just),
					("iBidi", &mut self.i_bidi),
				],
			)?,
			"contexts" => get_simple(
				e,
				&mut [
					("maxPreContext", &mut self.max_pre_context),
					("maxPostContext", &mut self.max_post_context),
				],
			)?,
			"attributes" => get_simple(
				e,
				&mut [
					("attrPseudo", &mut self.attr_pseudo),
					("attrBreakWeight", &mut self.attr_break_weight),
					("attrDirectionality", &mut self.attr_directionality),
					("attrMirroring", &mut self.attr_mirroring),
					("attrSkipPasses", &mut self.attr_skip_passes),
					("attCollisions", &mut self.att_collisions),
				],
			)?,
			"justifications" => {
				for c in e.elements().filter(|c| c.name == "justify") {
					let mut j = Justify::default();
					for (k, v) in &c.attrs {
						let v = py_int(v)?;
						if let Some(i) = Justify::NAMES.iter().position(|n| n == k) {
							*j.fields_mut()[i] = v;
						}
					}
					self.j_levels.push(j);
				}
			}
			"critFeatures" => {
				self.crit_features = py_split(&content_string(e))
					.map(py_int)
					.collect::<Result<_>>()?;
			}
			"scriptTags" => {
				self.script_tags = py_split(&content_string(e)).map(str::to_owned).collect();
			}
			"pseudoMap" => {
				self.p_map.clear();
				let mut last = None;
				for c in e.elements() {
					if c.name == "pseudo" {
						let unicode = attr(c, "unicode")?;
						let k = parse_int_radix(unicode, 16)
							.and_then(|v| i64::try_from(v).ok())
							.ok_or_else(|| {
								Error::value(format!(
									"invalid literal for int() with base 16: {unicode:?}"
								))
							})?;
						last = Some((k, font.glyph_id(attr(c, "pseudo")?)?));
					}
					// As in fontTools, other elements repeat the last mapping.
					let (k, v) = last.ok_or_else(|| {
						Error::ttlib("NameError: cannot access free variable 'k'")
					})?;
					self.p_map.insert(k, v);
				}
			}
			"classes" => {
				self.classes = Classes::default();
				for c in e.elements() {
					self.classes.from_xml(c, font, version)?;
				}
			}
			"passes" => {
				for c in e.elements().filter(|c| c.name == "pass") {
					let mut p = Pass::default();
					for pe in c.elements() {
						p.from_xml(pe, font, version)?;
					}
					self.passes.push(p);
				}
			}
			_ => {}
		}
		Ok(())
	}

	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		for v in self.p_map.values_mut() {
			*v = f(*v)?;
		}
		self.classes.remap_glyphs(f)?;
		for p in &mut self.passes {
			p.col_map = crate::ot::aat::remap_keys(std::mem::take(&mut p.col_map), f)?;
		}
		Ok(())
	}
}

// ---------------------------------------------------------------------------
// The table

/// Graphite Rules table (`table_S__i_l_f`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Silf {
	/// The table version (`version`).
	pub version: f64,
	/// `compilerVersion` (version 3 and later; 0 before).
	pub compiler_version: i64,
	/// The compression scheme (`scheme`).
	pub scheme: i64,
	/// The subtables (`silfs`).
	pub silfs: Vec<SilfSubtable>,
}

impl TableType for Silf {
	const TAG: Tag = Tag::new(b"Silf");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let mut silf = Silf {
			version: round_version(read_fixed(&mut at_least(data, 4)?)?)?,
			..Default::default()
		};
		let mut owned = None;
		if silf.version >= 5.0 {
			let (d, scheme) = decompress(data)?;
			silf.scheme = scheme;
			owned = Some(d);
		}
		let data = owned.as_deref().unwrap_or(data);
		let (num_silf, base) = if silf.version < 3.0 {
			(exact(py_slice(data, 4, 6), 2)?.u16()? as usize, 8)
		} else {
			let mut r = at_least(data, 12)?;
			silf.version = read_fixed(&mut r)?;
			silf.compiler_version = r.u32()? as i64;
			(r.u16()? as usize, 12)
		};
		let offsets = at_least(py_tail(data, base), 4 * num_silf)?.u32_array(num_silf)?;
		for offset in offsets {
			let sub = py_tail(data, offset as i64);
			silf.silfs.push(SilfSubtable::decompile(sub, silf.version)?);
		}
		Ok(silf)
	}
}

impl Table for Silf {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let num_silf = self.silfs.len() as i64;
		let mut hdr = Vec::new();
		hdr.pack_fixed(self.version)?;
		if self.version < 3.0 {
			hdr.pack_u16(num_silf)?;
			hdr.pack_u16(0)?;
		} else {
			hdr.pack_u32(self.compiler_version)?;
			hdr.pack_u16(num_silf)?;
			hdr.extend_from_slice(&[0; 2]);
		}
		let mut offset = hdr.len() as i64 + 4 * num_silf;
		let mut data = Vec::new();
		let version = self.version;
		for s in &mut self.silfs {
			hdr.pack_u32(offset)?;
			let sub = s.compile(version)?;
			offset += sub.len() as i64;
			data.extend(sub);
		}
		hdr.extend(data);
		if self.version >= 5.0 {
			return compress(self.scheme, &hdr);
		}
		Ok(hdr)
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		for s in &mut self.silfs {
			s.remap_glyphs(f)?;
		}
		Ok(())
	}
}

impl TableXml for Silf {
	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		writer.comment("Attributes starting with _ are informative only");
		writer.newline();
		writer.simpletag(
			"version",
			&[
				("version", &self.version),
				("compilerVersion", &self.compiler_version),
				("compressionScheme", &self.scheme),
			],
		);
		writer.newline();
		for s in &self.silfs {
			writer.begintag("silf", &[]);
			writer.newline();
			s.to_xml(writer, font, self.version)?;
			writer.endtag("silf");
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"version" => {
				self.scheme = eval_int(attr(element, "compressionScheme")?)?;
				self.version = eval_float(attr(element, "version")?)?;
				self.compiler_version = eval_int(attr(element, "compilerVersion")?)?;
			}
			"silf" => {
				let mut s = SilfSubtable::default();
				for e in element.elements() {
					s.from_xml(e, font, self.version)?;
				}
				self.silfs.push(s);
			}
			_ => {}
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn code_round_trip() -> Result<()> {
		let lines = [
			"PUT_GLYPH_8BIT_OBS(0)",
			"ASSOC(0, 1)",
			"NEXT",
			"PUSH_BYTE(-1)",
			"PUT_SUBS(-1, 300, 2)",
			"ASSOC()",
		];
		let code = assemble(&lines)?;
		// `ASSOC()` has no operands to match, so it is just the opcode.
		assert_eq!(
			disassemble(&code)?,
			[
				"PUT_GLYPH_8BIT_OBS(0)",
				"ASSOC(0, 1)",
				"NEXT",
				"PUSH_BYTE(-1)",
				"PUT_SUBS(-1, 300, 2)",
			]
		);
		// A truncated operand ends the listing.
		assert_eq!(disassemble(&[25, 1])?, ["NEXT"]);
		assert_eq!(disassemble(&[33, 0])?, ["ASSOC()"]);
		assert!(disassemble(&[67]).is_err());
		assert_eq!(disassemble(&[68])?, ["NOP"]);
		assert_eq!(assemble(&["PUSH_BYTE (1)", "UNKNOWN", "NEXT(3)"])?, [25]);
		Ok(())
	}
}
