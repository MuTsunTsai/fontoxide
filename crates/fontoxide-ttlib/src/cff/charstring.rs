//! Type 2 charstrings (`fontTools.misc.psCharStrings`): tokens, encoding, and the
//! interpreters that decompile them into programs and draw them.

use fontoxide_misc::fixed_tools::{
	fixed_to_float, float_to_fixed, float_to_fixed_to_str, str_to_fixed_to_float,
};
use fontoxide_misc::text_tools::{binary2num, num2binary, read_hex};
use fontoxide_misc::transform::Transform;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_pens::Pen;

use crate::{Error, Result};

/// A token of a charstring program.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
	/// An integer operand.
	Int(i64),
	/// A 16.16 fixed operand.
	Float(f64),
	/// An operator, by name.
	Op(&'static str),
	/// The mask bytes after `hintmask` or `cntrmask`.
	Mask(Vec<u8>),
}

/// The Type 2 operators: opcode (12 x for two-byte ones) and name (`t2Operators`).
pub const T2_OPERATORS: &[(u16, &str)] = &[
	(1, "hstem"),
	(3, "vstem"),
	(4, "vmoveto"),
	(5, "rlineto"),
	(6, "hlineto"),
	(7, "vlineto"),
	(8, "rrcurveto"),
	(10, "callsubr"),
	(11, "return"),
	(14, "endchar"),
	(15, "vsindex"),
	(16, "blend"),
	(18, "hstemhm"),
	(19, "hintmask"),
	(20, "cntrmask"),
	(21, "rmoveto"),
	(22, "hmoveto"),
	(23, "vstemhm"),
	(24, "rcurveline"),
	(25, "rlinecurve"),
	(26, "vvcurveto"),
	(27, "hhcurveto"),
	(29, "callgsubr"),
	(30, "vhcurveto"),
	(31, "hvcurveto"),
	(12 << 8, "ignore"),
	(12 << 8 | 3, "and"),
	(12 << 8 | 4, "or"),
	(12 << 8 | 5, "not"),
	(12 << 8 | 8, "store"),
	(12 << 8 | 9, "abs"),
	(12 << 8 | 10, "add"),
	(12 << 8 | 11, "sub"),
	(12 << 8 | 12, "div"),
	(12 << 8 | 13, "load"),
	(12 << 8 | 14, "neg"),
	(12 << 8 | 15, "eq"),
	(12 << 8 | 18, "drop"),
	(12 << 8 | 20, "put"),
	(12 << 8 | 21, "get"),
	(12 << 8 | 22, "ifelse"),
	(12 << 8 | 23, "random"),
	(12 << 8 | 24, "mul"),
	(12 << 8 | 26, "sqrt"),
	(12 << 8 | 27, "dup"),
	(12 << 8 | 28, "exch"),
	(12 << 8 | 29, "index"),
	(12 << 8 | 30, "roll"),
	(12 << 8 | 34, "hflex"),
	(12 << 8 | 35, "flex"),
	(12 << 8 | 36, "hflex1"),
	(12 << 8 | 37, "flex1"),
];

fn operator_name(code: u16) -> Option<&'static str> {
	T2_OPERATORS
		.iter()
		.find(|(c, _)| *c == code)
		.map(|(_, n)| *n)
}

fn operator_code(name: &str) -> Option<(u16, &'static str)> {
	T2_OPERATORS.iter().find(|(_, n)| *n == name).copied()
}

pub(crate) fn index_error() -> Error {
	Error::ttlib("IndexError: index out of range")
}

fn struct_error(n: usize) -> Error {
	Error::Struct(format!("unpack requires a buffer of {n} bytes"))
}

/// Reads one token of Type 2 bytecode at `index` (`getToken` with
/// `t2OperandEncoding`); `None` at the end, or at an unknown operator, which
/// fontTools also treats as the end.
pub fn read_token(data: &[u8], index: usize) -> Result<Option<(Token, usize)>> {
	let Some(&b0) = data.get(index) else {
		return Ok(None);
	};
	let mut i = index + 1;
	let byte = |i: usize| data.get(i).copied().ok_or_else(index_error);
	let token = match b0 {
		28 => {
			let b = data.get(i..i + 2).ok_or_else(|| struct_error(2))?;
			i += 2;
			Token::Int(i16::from_be_bytes([b[0], b[1]]) as i64)
		}
		0..=31 => {
			let code = if b0 == 12 {
				let b1 = byte(i)?;
				i += 1;
				12 << 8 | b1 as u16
			} else {
				b0 as u16
			};
			// UPSTREAM-BUG(B15): an unknown operator ends the charstring; what
			// follows is dropped when it is recompiled.
			match operator_name(code) {
				Some(name) => Token::Op(name),
				None => return Ok(None),
			}
		}
		32..=246 => Token::Int(b0 as i64 - 139),
		247..=250 => {
			let b1 = byte(i)?;
			i += 1;
			Token::Int((b0 as i64 - 247) * 256 + b1 as i64 + 108)
		}
		251..=254 => {
			let b1 = byte(i)?;
			i += 1;
			Token::Int(-(b0 as i64 - 251) * 256 - b1 as i64 - 108)
		}
		255 => {
			let b = data.get(i..i + 4).ok_or_else(|| struct_error(4))?;
			i += 4;
			Token::Float(fixed_to_float(
				i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as i64,
				16,
			))
		}
	};
	Ok(Some((token, i)))
}

/// Encodes an integer for Type 2 charstrings (`encodeIntT2`).
pub fn encode_int_t2(value: i64, out: &mut Vec<u8>) {
	encode_int(value, out, Some(28), None);
}

/// Encodes an integer for CFF DICTs (`encodeIntCFF`).
pub fn encode_int_cff(value: i64, out: &mut Vec<u8>) {
	encode_int(value, out, Some(28), Some(29));
}

fn encode_int(value: i64, out: &mut Vec<u8>, two: Option<u8>, four: Option<u8>) {
	if (-107..=107).contains(&value) {
		out.push((value + 139) as u8);
	} else if (108..=1131).contains(&value) {
		let v = value - 108;
		out.push(((v >> 8) + 247) as u8);
		out.push((v & 0xFF) as u8);
	} else if (-1131..=-108).contains(&value) {
		let v = -value - 108;
		out.push(((v >> 8) + 251) as u8);
		out.push((v & 0xFF) as u8);
	} else if let Some(op) = two
		&& (-32768..=32767).contains(&value)
	{
		out.push(op);
		out.extend_from_slice(&(value as i16).to_be_bytes());
	} else {
		match four {
			Some(op) => out.push(op),
			None => {
				log::warn!(
					"4-byte T2 number got passed to the IntType handler. This should happen only when reading in old XML files.\n"
				);
				out.push(255);
			}
		}
		out.extend_from_slice(&(value as i32).to_be_bytes());
	}
}

/// Encodes a 16.16 fixed number for Type 2 charstrings (`encodeFixed`).
pub fn encode_fixed(f: f64, out: &mut Vec<u8>) {
	let value = float_to_fixed(f, 16);
	if value & 0xFFFF == 0 {
		encode_int_t2(value >> 16, out);
	} else {
		out.push(255);
		out.extend_from_slice(&(value as i32).to_be_bytes());
	}
}

/// A Type 2 charstring (`T2CharString`): bytecode as read, or a program once
/// decompiled.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct T2CharString {
	/// The bytecode, until decompiled (`bytecode`).
	pub bytecode: Option<Vec<u8>>,
	/// The program, once decompiled (`program`).
	pub program: Vec<Token>,
	/// The font DICT of a CID-keyed glyph read from TTX (`fdSelectIndex`).
	pub fd_select_index: Option<i64>,
}

impl T2CharString {
	/// A charstring from bytecode.
	pub fn from_bytecode(data: Vec<u8>) -> Self {
		T2CharString {
			bytecode: Some(data),
			program: Vec::new(),
			fd_select_index: None,
		}
	}

	/// Whether it is still bytecode (`needsDecompilation`).
	pub fn needs_decompilation(&self) -> bool {
		self.bytecode.is_some()
	}

	/// The token at `index`, from the bytecode or the program (`getToken`).
	fn token(&self, index: usize) -> Result<Option<(Token, usize)>> {
		match &self.bytecode {
			Some(b) => read_token(b, index),
			None => Ok(self.program.get(index).map(|t| (t.clone(), index + 1))),
		}
	}

	/// The mask bytes at `index` (`getBytes`).
	fn bytes(&self, index: usize, n: usize) -> Result<(Vec<u8>, usize)> {
		let (bytes, next) = match &self.bytecode {
			Some(b) => {
				let end = (index + n).min(b.len());
				(b[index.min(end)..end].to_vec(), index + n)
			}
			None => match self.program.get(index) {
				Some(Token::Mask(m)) => (m.clone(), index + 1),
				_ => return Err(Error::ttlib("TypeError: object of type is not a mask")),
			},
		};
		if bytes.len() != n {
			return Err(Error::Assertion("len(bytes) == nBytes".into()));
		}
		Ok((bytes, next))
	}

	/// Encodes the program, unless it is still bytecode (`compile`).
	pub fn compile(&mut self, is_cff2: bool) -> Result<()> {
		if self.bytecode.is_some() {
			return Ok(());
		}
		let mut program: &[Token] = &self.program;
		if is_cff2 {
			if matches!(program.last(), Some(Token::Op("return" | "endchar"))) {
				program = &program[..program.len() - 1];
			}
		} else if program.last().is_some_and(|t| !matches!(t, Token::Op(_))) {
			return Err(Error::ttlib(
				"T2CharString or Subr has items on the stack after last operator.",
			));
		}
		let mut out = Vec::new();
		let mut i = 0;
		while i < program.len() {
			match &program[i] {
				Token::Op(name) => {
					let (code, _) = operator_code(name)
						.ok_or_else(|| Error::ttlib(format!("illegal operator: {name}")))?;
					if code >> 8 == 12 {
						out.push(12);
					}
					out.push(code as u8);
					if matches!(*name, "hintmask" | "cntrmask") {
						i += 1;
						match program.get(i) {
							Some(Token::Mask(m)) => out.extend_from_slice(m),
							_ => return Err(Error::ttlib("TypeError: expected the hint mask")),
						}
					}
				}
				Token::Int(v) => encode_int_t2(*v, &mut out),
				Token::Float(f) => encode_fixed(*f, &mut out),
				Token::Mask(_) => return Err(Error::Assertion("unsupported type: bytes".into())),
			}
			i += 1;
		}
		self.bytecode = Some(out);
		self.program = Vec::new();
		Ok(())
	}

	/// `toXML`: the bytecode as hex, or the program, one operator per line.
	pub fn to_xml(&self, writer: &mut XmlWriter) -> Result<()> {
		if let Some(b) = &self.bytecode {
			writer.dumphex(b);
			return Ok(());
		}
		let mut args: Vec<String> = Vec::new();
		let mut i = 0;
		while let Some((token, next)) = self.token(i)? {
			i = next;
			match token {
				Token::Op(name) => {
					let mut line = args.clone();
					line.push(name.to_owned());
					if matches!(name, "hintmask" | "cntrmask") {
						let mask = match self.token(i)? {
							Some((Token::Mask(m), next)) => {
								i = next;
								m
							}
							_ => return Err(Error::ttlib("TypeError: expected the hint mask")),
						};
						line.push(mask.iter().map(|&b| num2binary(b as i64, 8)).collect());
					}
					writer.write(&line.join(" "));
					writer.newline();
					args.clear();
				}
				Token::Float(f) => args.push(float_to_fixed_to_str(f, 16)),
				Token::Int(v) => args.push(v.to_string()),
				Token::Mask(_) => {
					return Err(Error::ttlib("TypeError: sequence item: expected str"));
				}
			}
		}
		if !args.is_empty() {
			// Only CFF2 charstrings may end with operands.
			writer.write(&args.join(" "));
		}
		Ok(())
	}

	/// `fromXML`.
	pub fn from_xml(e: &Element) -> Result<Self> {
		if e.attr("raw").is_some_and(|v| !v.is_empty()) {
			let data = read_hex(e.text_chunks())
				.ok_or_else(|| Error::value("non-hexadecimal number found"))?;
			return Ok(T2CharString::from_bytecode(data));
		}
		let text = e.text();
		let content: Vec<&str> = text.split_whitespace().collect();
		let mut program = Vec::new();
		let mut i = 0;
		while i < content.len() {
			let token = content[i];
			i += 1;
			if let Some(v) = fontoxide_misc::py::parse_int(token) {
				let v = i64::try_from(v).map_err(|_| Error::value("int too large"))?;
				program.push(Token::Int(v));
			} else if let Some(f) = str_to_fixed_to_float(token, 16) {
				program.push(Token::Float(f));
			} else {
				let op = operator_code(token).map(|(_, n)| n);
				// Unknown names fail when compiling ("illegal operator").
				program.push(Token::Op(op.unwrap_or(leak_name(token))));
				if matches!(token, "hintmask" | "cntrmask") {
					let mask = content.get(i).ok_or_else(index_error)?;
					let chars: Vec<char> = mask.chars().collect();
					let bytes = chars
						.chunks(8)
						.map(|c| binary2num(&c.iter().collect::<String>()) as u8)
						.collect();
					program.push(Token::Mask(bytes));
					i += 1;
				}
			}
		}
		Ok(T2CharString {
			bytecode: None,
			program,
			fd_select_index: None,
		})
	}
}

/// Names of unknown operators read from TTX; they only matter for the error
/// when compiling.
fn leak_name(name: &str) -> &'static str {
	Box::leak(name.to_owned().into_boxed_str())
}

/// The subroutine number bias for a number of subroutines (`calcSubrBias`).
pub fn subr_bias(count: usize) -> i64 {
	if count < 1240 {
		107
	} else if count < 33900 {
		1131
	} else {
		32768
	}
}

/// A number on the operand stack: an `int`, or a `float` from 16.16 operands
/// and `div`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Num {
	Int(i64),
	Float(f64),
}

impl Num {
	pub(crate) fn value(self) -> f64 {
		match self {
			Num::Int(i) => i as f64,
			Num::Float(f) => f,
		}
	}

	fn int(self) -> Result<i64> {
		match self {
			Num::Int(i) => Ok(i),
			Num::Float(_) => Err(Error::ttlib(
				"TypeError: list indices must be integers or slices, not float",
			)),
		}
	}
}

/// A blender: the delta of a blended value at the location being drawn, from
/// the `vsindex` and the deltas of the regions.
pub type Blender<'a> = &'a dyn Fn(i64, &[f64]) -> Result<f64>;

/// What the interpreter needs from the font: subroutines, and the private
/// DICT of the charstring being run.
#[allow(missing_debug_implementations)]
pub struct Context<'a> {
	/// The local subroutines (`Private.Subrs`).
	pub local_subrs: &'a mut [T2CharString],
	/// The global subroutines.
	pub global_subrs: &'a mut [T2CharString],
	/// `nominalWidthX`, or `None` in CFF2.
	pub nominal_width_x: Option<f64>,
	/// `defaultWidthX`, or `None` in CFF2.
	pub default_width_x: Option<f64>,
	/// The private DICT's `vsindex`, if any.
	pub vsindex: Option<i64>,
	/// The number of regions of a `vsindex` (`Private.getNumRegions`).
	pub num_regions: &'a dyn Fn(i64) -> Result<i64>,
	/// Computes the delta of a blended value at the location being drawn, from
	/// the `vsindex` and the deltas of the regions (`blender`); without it,
	/// `blend` keeps the default values.
	pub blender: Option<Blender<'a>>,
}

/// The interpreter (`SimpleT2Decompiler`, or `T2OutlineExtractor` when there is
/// a pen).
#[allow(missing_debug_implementations)]
pub struct Interpreter<'a, 'p> {
	ctx: Context<'a>,
	pen: Option<&'p mut dyn Pen>,
	stack: Vec<Num>,
	hint_count: usize,
	hint_mask_bytes: usize,
	num_regions: Option<i64>,
	vs_index: i64,
	got_width: bool,
	current: (f64, f64),
	saw_move_to: bool,
	subr_level: usize,
	/// Subroutines being run, to stop runaway recursion.
	depth: usize,
}

const MAX_DEPTH: usize = 500;

impl<'a, 'p> Interpreter<'a, 'p> {
	/// An interpreter that decompiles (`SimpleT2Decompiler`), or draws onto
	/// `pen` (`T2OutlineExtractor`).
	pub fn new(ctx: Context<'a>, pen: Option<&'p mut dyn Pen>) -> Self {
		Interpreter {
			ctx,
			pen,
			stack: Vec::new(),
			hint_count: 0,
			hint_mask_bytes: 0,
			num_regions: None,
			vs_index: 0,
			got_width: false,
			current: (0.0, 0.0),
			saw_move_to: false,
			subr_level: 0,
			depth: 0,
		}
	}

	fn drawing(&self) -> bool {
		self.pen.is_some()
	}

	/// Runs a charstring, decompiling it if it is still bytecode (`execute`).
	pub fn execute(&mut self, cs: &mut T2CharString) -> Result<()> {
		if self.drawing() {
			self.subr_level += 1;
		}
		self.run(cs)?;
		if self.drawing() {
			self.subr_level -= 1;
			if self.subr_level == 0 {
				self.end_path()?;
			}
		}
		Ok(())
	}

	fn run(&mut self, cs: &mut T2CharString) -> Result<()> {
		self.depth += 1;
		if self.depth > MAX_DEPTH {
			// LENIENT(L15): fontTools runs into Python's recursion limit.
			return Err(Error::ttlib(
				"RecursionError: maximum recursion depth exceeded",
			));
		}
		let decompiling = cs.needs_decompilation();
		let mut program = Vec::new();
		let mut index = 0;
		while let Some((token, next)) = cs.token(index)? {
			index = next;
			if decompiling {
				program.push(token.clone());
			}
			match token {
				Token::Op(op) => {
					if let Some((mask, next)) = self.operator(op, cs, index)? {
						index = next;
						if decompiling {
							program.push(Token::Mask(mask));
						}
					}
				}
				Token::Int(v) => self.stack.push(Num::Int(v)),
				Token::Float(f) => self.stack.push(Num::Float(f)),
				Token::Mask(_) => return Err(Error::ttlib("TypeError: unexpected mask")),
			}
		}
		if decompiling {
			cs.program = program;
			cs.bytecode = None;
		}
		self.depth -= 1;
		Ok(())
	}

	fn pop(&mut self) -> Result<Num> {
		self.stack.pop().ok_or_else(index_error)
	}

	fn popall(&mut self) -> Vec<Num> {
		std::mem::take(&mut self.stack)
	}

	/// `popallWidth`: the arguments, without the width if this is the first
	/// operator that can carry it.
	fn popall_width(&mut self, even_odd: usize) -> Result<Vec<Num>> {
		let mut args = self.popall();
		if !self.got_width {
			if (even_odd ^ (args.len() % 2)) != 0 {
				if self.ctx.default_width_x.is_none() {
					return Err(Error::Assertion(
						"CFF2 CharStrings must not have an initial width value".into(),
					));
				}
				args.remove(0);
			}
			self.got_width = true;
		}
		Ok(args)
	}

	fn count_hints(&mut self) -> Result<()> {
		let args = if self.drawing() {
			self.popall_width(0)?
		} else {
			self.popall()
		};
		self.hint_count += args.len() / 2;
		Ok(())
	}

	fn call(&mut self, global: bool) -> Result<()> {
		let i = self.pop()?.int()?;
		let subrs_len = if global {
			self.ctx.global_subrs.len()
		} else {
			self.ctx.local_subrs.len()
		};
		let at = i + subr_bias(subrs_len);
		// Python indexing: negative indices count from the end.
		let at = if at < 0 { at + subrs_len as i64 } else { at };
		let at = usize::try_from(at)
			.ok()
			.filter(|&a| a < subrs_len)
			.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
		let mut subr = if global {
			std::mem::take(&mut self.ctx.global_subrs[at])
		} else {
			std::mem::take(&mut self.ctx.local_subrs[at])
		};
		let result = self.run(&mut subr);
		if global {
			self.ctx.global_subrs[at] = subr;
		} else {
			self.ctx.local_subrs[at] = subr;
		}
		result
	}

	/// Runs an operator; returns the mask bytes of `hintmask` / `cntrmask`.
	fn operator(
		&mut self,
		op: &str,
		cs: &T2CharString,
		index: usize,
	) -> Result<Option<(Vec<u8>, usize)>> {
		match op {
			"return" | "ignore" => {}
			"endchar" if !self.drawing() => {}
			"callsubr" => self.call(false)?,
			"callgsubr" => self.call(true)?,
			"hstem" | "vstem" | "hstemhm" | "vstemhm" => self.count_hints()?,
			"hintmask" | "cntrmask" => {
				if self.hint_mask_bytes == 0 {
					self.count_hints()?;
					self.hint_mask_bytes = self.hint_count.div_ceil(8);
				}
				return cs.bytes(index, self.hint_mask_bytes).map(Some);
			}
			"blend" => self.blend()?,
			"vsindex" => {
				let vi = self.pop()?.int()?;
				self.vs_index = vi;
				self.num_regions = Some((self.ctx.num_regions)(vi)?);
			}
			"and" | "or" | "not" | "store" | "abs" | "add" | "sub" | "load" | "neg" | "eq"
			| "drop" | "put" | "get" | "ifelse" | "random" | "mul" | "sqrt" | "dup" | "exch"
			| "index" | "roll" => {
				return Err(Error::ttlib(format!("NotImplementedError: {op}")));
			}
			"div" if !self.drawing() => {
				return Err(Error::ttlib("NotImplementedError: div"));
			}
			_ if self.drawing() => self.draw_operator(op)?,
			_ => {
				self.popall();
			}
		}
		Ok(None)
	}

	fn blend(&mut self) -> Result<()> {
		if self.num_regions.is_none() {
			self.vs_index = self.ctx.vsindex.unwrap_or(0);
			self.num_regions = Some((self.ctx.num_regions)(self.vs_index)?);
		}
		let regions = self.num_regions.unwrap_or(0);
		let blends = self.pop()?.int()?;
		let ops = blends * (regions + 1);
		let Some(blender) = self.ctx.blender else {
			if regions != 0 {
				// `del stack[-(numOps - numBlends):]`: `-0` deletes everything.
				let k = ops - blends;
				let n = self.stack.len() as i64;
				let start = if k == 0 { 0 } else { (n - k).max(0) };
				self.stack.truncate(start as usize);
			}
			return Ok(());
		};
		let mut argi = self.stack.len() as i64 - ops;
		if argi < 0 || blends < 0 || regions < 0 {
			return Err(Error::ttlib("IndexError: list index out of range"));
		}
		let end_args = argi + blends;
		let mut tuplei = end_args;
		while argi < end_args {
			let next_ti = tuplei + regions;
			let deltas: Vec<f64> = self.stack[tuplei as usize..next_ti as usize]
				.iter()
				.map(|n| n.value())
				.collect();
			let delta = blender(self.vs_index, &deltas)?;
			let at = &mut self.stack[argi as usize];
			*at = Num::Float(at.value() + delta);
			tuplei = next_ti;
			argi += 1;
		}
		self.stack.truncate(end_args as usize);
		Ok(())
	}

	// --- Drawing (`T2OutlineExtractor`)

	fn next_point(&mut self, (dx, dy): (f64, f64)) -> (f64, f64) {
		self.current = (self.current.0 + dx, self.current.1 + dy);
		self.current
	}

	fn r_move_to(&mut self, p: (f64, f64)) -> Result<()> {
		let p = self.next_point(p);
		if let Some(pen) = self.pen.as_mut() {
			pen.move_to(p)?;
		}
		self.saw_move_to = true;
		Ok(())
	}

	fn r_line_to(&mut self, p: (f64, f64)) -> Result<()> {
		if !self.saw_move_to {
			self.r_move_to((0.0, 0.0))?;
		}
		let p = self.next_point(p);
		if let Some(pen) = self.pen.as_mut() {
			pen.line_to(p)?;
		}
		Ok(())
	}

	fn r_curve_to(&mut self, p1: (f64, f64), p2: (f64, f64), p3: (f64, f64)) -> Result<()> {
		if !self.saw_move_to {
			self.r_move_to((0.0, 0.0))?;
		}
		let a = self.next_point(p1);
		let b = self.next_point(p2);
		let c = self.next_point(p3);
		if let Some(pen) = self.pen.as_mut() {
			pen.curve_to(&[a, b, c])?;
		}
		Ok(())
	}

	fn end_path(&mut self) -> Result<()> {
		if self.saw_move_to
			&& let Some(pen) = self.pen.as_mut()
		{
			pen.close_path()?;
		}
		self.saw_move_to = false;
		Ok(())
	}

	fn draw_operator(&mut self, op: &str) -> Result<()> {
		let unpack = |args: &[Num], n: usize| -> Result<Vec<f64>> {
			if args.len() != n {
				return Err(Error::value(format!(
					"ValueError: not enough values to unpack (expected {n}, got {})",
					args.len()
				)));
			}
			Ok(args.iter().map(|a| a.value()).collect())
		};
		match op {
			"rmoveto" => {
				self.end_path()?;
				let a = self.popall_width(0)?;
				let v = unpack(&a, 2)?;
				self.r_move_to((v[0], v[1]))?;
			}
			"hmoveto" => {
				self.end_path()?;
				let a = self.popall_width(1)?;
				let x = a.first().ok_or_else(index_error)?.value();
				self.r_move_to((x, 0.0))?;
			}
			"vmoveto" => {
				self.end_path()?;
				let a = self.popall_width(1)?;
				let y = a.first().ok_or_else(index_error)?.value();
				self.r_move_to((0.0, y))?;
			}
			"endchar" => {
				self.end_path()?;
				let args = self.popall_width(0)?;
				if !args.is_empty() {
					// `seac`: a base and an accent glyph from the Standard Encoding.
					let v = unpack(&args, 4)?;
					let code = |n: Num| -> Result<usize> {
						usize::try_from(n.int()?).map_err(|_| index_error())
					};
					let enc = &crate::generated::cff_strings::STANDARD_ENCODING;
					let base = *enc.get(code(args[2])?).ok_or_else(index_error)?;
					let accent = *enc.get(code(args[3])?).ok_or_else(index_error)?;
					if let Some(pen) = self.pen.as_mut() {
						pen.add_component(base, Transform::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0))?;
						pen.add_component(accent, Transform::new(1.0, 0.0, 0.0, 1.0, v[0], v[1]))?;
					}
				}
			}
			"rlineto" => {
				let args = self.popall();
				for pair in args.chunks(2) {
					let x = pair[0].value();
					let y = pair
						.get(1)
						.ok_or_else(|| Error::value("not enough values"))?
						.value();
					self.r_line_to((x, y))?;
				}
			}
			"hlineto" | "vlineto" => {
				let mut horizontal = op == "hlineto";
				for a in self.popall() {
					let v = a.value();
					self.r_line_to(if horizontal { (v, 0.0) } else { (0.0, v) })?;
					horizontal = !horizontal;
				}
			}
			"rrcurveto" => {
				let args = self.popall();
				for c in args.chunks(6) {
					let v = unpack(c, 6)?;
					self.r_curve_to((v[0], v[1]), (v[2], v[3]), (v[4], v[5]))?;
				}
			}
			"rcurveline" => {
				let args = self.popall();
				let n = args.len();
				let mut i = 0;
				while (i as i64) < n as i64 - 2 {
					let v = unpack(&args[i..(i + 6).min(n)], 6)?;
					self.r_curve_to((v[0], v[1]), (v[2], v[3]), (v[4], v[5]))?;
					i += 6;
				}
				let last = unpack(&args[n.saturating_sub(2)..], 2)?;
				self.r_line_to((last[0], last[1]))?;
			}
			"rlinecurve" => {
				let args = self.popall();
				let n = args.len();
				let split = n.saturating_sub(6);
				for pair in args[..split].chunks(2) {
					let v = unpack(pair, 2)?;
					self.r_line_to((v[0], v[1]))?;
				}
				let v = unpack(&args[split..], 6)?;
				self.r_curve_to((v[0], v[1]), (v[2], v[3]), (v[4], v[5]))?;
			}
			"vvcurveto" | "hhcurveto" => {
				let mut args = self.popall();
				let mut d1 = if args.len() % 2 == 1 {
					args.remove(0).value()
				} else {
					0.0
				};
				for c in args.chunks(4) {
					let v = unpack(c, 4)?;
					if op == "vvcurveto" {
						self.r_curve_to((d1, v[0]), (v[1], v[2]), (0.0, v[3]))?;
					} else {
						self.r_curve_to((v[0], d1), (v[1], v[2]), (v[3], 0.0))?;
					}
					d1 = 0.0;
				}
			}
			"vhcurveto" | "hvcurveto" => {
				let mut args = self.popall();
				let mut vertical = op == "vhcurveto";
				while !args.is_empty() {
					args = self.half_curve(args, vertical)?;
					vertical = !vertical;
				}
			}
			"hflex" => {
				let v = unpack(&self.popall(), 7)?;
				self.r_curve_to((v[0], 0.0), (v[1], v[2]), (v[3], 0.0))?;
				self.r_curve_to((v[4], 0.0), (v[5], -v[2]), (v[6], 0.0))?;
			}
			"flex" => {
				let v = unpack(&self.popall(), 13)?;
				self.r_curve_to((v[0], v[1]), (v[2], v[3]), (v[4], v[5]))?;
				self.r_curve_to((v[6], v[7]), (v[8], v[9]), (v[10], v[11]))?;
			}
			"hflex1" => {
				let v = unpack(&self.popall(), 9)?;
				let dy6 = -(v[1] + v[3] + 0.0 + 0.0 + v[7]);
				self.r_curve_to((v[0], v[1]), (v[2], v[3]), (v[4], 0.0))?;
				self.r_curve_to((v[5], 0.0), (v[6], v[7]), (v[8], dy6))?;
			}
			"flex1" => {
				let v = unpack(&self.popall(), 11)?;
				let dx = v[0] + v[2] + v[4] + v[6] + v[8];
				let dy = v[1] + v[3] + v[5] + v[7] + v[9];
				let (dx6, dy6) = if dx.abs() > dy.abs() {
					(v[10], -dy)
				} else {
					(-dx, v[10])
				};
				self.r_curve_to((v[0], v[1]), (v[2], v[3]), (v[4], v[5]))?;
				self.r_curve_to((v[6], v[7]), (v[8], v[9]), (dx6, dy6))?;
			}
			"div" => {
				let num2 = self.pop()?;
				let num1 = self.pop()?;
				if num2.value() == 0.0 {
					return Err(Error::ttlib("ZeroDivisionError: division by zero"));
				}
				// `num1 // num2` if it equals `num1 / num2`, else the latter: an int
				// only for exact integer division.
				let d2 = num1.value() / num2.value();
				let result = match (num1, num2) {
					(Num::Int(a), Num::Int(b)) => {
						let mut q = a / b;
						if a % b != 0 && ((a < 0) != (b < 0)) {
							q -= 1;
						}
						if q as f64 == d2 {
							Num::Int(q)
						} else {
							Num::Float(d2)
						}
					}
					_ => Num::Float(d2),
				};
				self.stack.push(result);
			}
			_ => {
				self.popall();
			}
		}
		Ok(())
	}

	/// `vcurveto` / `hcurveto`: one curve of `vhcurveto` / `hvcurveto`.
	fn half_curve(&mut self, args: Vec<Num>, vertical: bool) -> Result<Vec<Num>> {
		if args.len() < 4 {
			return Err(Error::value("not enough values to unpack (expected 4)"));
		}
		let v: Vec<f64> = args[..4].iter().map(|a| a.value()).collect();
		let rest = &args[4..];
		let (last, rest) = if rest.len() == 1 {
			(rest[0].value(), Vec::new())
		} else {
			(0.0, rest.to_vec())
		};
		if vertical {
			self.r_curve_to((0.0, v[0]), (v[1], v[2]), (v[3], last))?;
		} else {
			self.r_curve_to((v[0], 0.0), (v[1], v[2]), (last, v[3]))?;
		}
		Ok(rest)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn ints_round_trip() -> Result<()> {
		for v in [-32768, -1131, -108, -107, 0, 107, 108, 1131, 1132, 32767] {
			let mut out = Vec::new();
			encode_int_t2(v, &mut out);
			assert_eq!(read_token(&out, 0)?, Some((Token::Int(v), out.len())));
		}
		let mut out = Vec::new();
		encode_fixed(1.5, &mut out);
		assert_eq!(read_token(&out, 0)?, Some((Token::Float(1.5), 5)));
		Ok(())
	}
}
