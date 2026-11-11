//! Type 1 charstrings (`T1CharString` and `T1OutlineExtractor` of
//! `fontTools.misc.psCharStrings`).

use fontoxide_misc::transform::Transform;
use fontoxide_pens::Pen;

use super::charstring::{Token, index_error};
use crate::{Error, Result};

/// The Type 1 operators: opcode (12 x for two-byte ones) and name (`t1Operators`).
pub const T1_OPERATORS: &[(u16, &str)] = &[
	(1, "hstem"),
	(3, "vstem"),
	(4, "vmoveto"),
	(5, "rlineto"),
	(6, "hlineto"),
	(7, "vlineto"),
	(8, "rrcurveto"),
	(9, "closepath"),
	(10, "callsubr"),
	(11, "return"),
	(13, "hsbw"),
	(14, "endchar"),
	(21, "rmoveto"),
	(22, "hmoveto"),
	(30, "vhcurveto"),
	(31, "hvcurveto"),
	(12 << 8, "dotsection"),
	(12 << 8 | 1, "vstem3"),
	(12 << 8 | 2, "hstem3"),
	(12 << 8 | 6, "seac"),
	(12 << 8 | 7, "sbw"),
	(12 << 8 | 12, "div"),
	(12 << 8 | 16, "callothersubr"),
	(12 << 8 | 17, "pop"),
	(12 << 8 | 33, "setcurrentpoint"),
];

fn operator_name(code: u16) -> Option<&'static str> {
	T1_OPERATORS
		.iter()
		.find(|(c, _)| *c == code)
		.map(|(_, n)| *n)
}

fn operator_code(name: &str) -> Option<u16> {
	T1_OPERATORS
		.iter()
		.find(|(_, n)| *n == name)
		.map(|(c, _)| *c)
}

/// Reads one token of Type 1 bytecode at `index` (`getToken` with
/// `t1OperandEncoding`); `None` at the end, or at an unknown operator, which
/// fontTools also treats as the end.
pub fn read_token_t1(data: &[u8], index: usize) -> Result<Option<(Token, usize)>> {
	let Some(&b0) = data.get(index) else {
		return Ok(None);
	};
	let mut i = index + 1;
	let byte = |i: usize| data.get(i).copied().ok_or_else(index_error);
	let token = match b0 {
		0..=31 => {
			let code = if b0 == 12 {
				let b1 = byte(i)?;
				i += 1;
				12 << 8 | b1 as u16
			} else {
				b0 as u16
			};
			// UPSTREAM-BUG(B15): as in Type 2 charstrings, an unknown operator
			// ends the charstring.
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
			let b = data
				.get(i..i + 4)
				.ok_or_else(|| Error::Struct("unpack requires a buffer of 4 bytes".into()))?;
			i += 4;
			Token::Int(i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as i64)
		}
	};
	Ok(Some((token, i)))
}

/// Encodes an integer for Type 1 charstrings (`encodeIntT1`).
pub fn encode_int_t1(value: i64, out: &mut Vec<u8>) -> Result<()> {
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
	} else {
		let v = i32::try_from(value).map_err(|_| {
			Error::Struct("'l' format requires -2147483648 <= number <= 2147483647".into())
		})?;
		out.push(255);
		out.extend_from_slice(&v.to_be_bytes());
	}
	Ok(())
}

/// A Type 1 charstring (`T1CharString`): bytecode (without the `lenIV` random
/// bytes, decrypted), or a program once decompiled.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct T1CharString {
	/// The bytecode, until decompiled (`bytecode`).
	pub bytecode: Option<Vec<u8>>,
	/// The program, once decompiled (`program`).
	pub program: Vec<Token>,
	/// The advance width, once drawn (`width`).
	pub width: Option<f64>,
}

impl T1CharString {
	/// A charstring from bytecode.
	pub fn from_bytecode(bytecode: Vec<u8>) -> Self {
		T1CharString {
			bytecode: Some(bytecode),
			program: Vec::new(),
			width: None,
		}
	}

	/// A charstring from a program.
	pub fn from_program(program: Vec<Token>) -> Self {
		T1CharString {
			bytecode: None,
			program,
			width: None,
		}
	}

	/// Whether it is still bytecode (`needsDecompilation`).
	pub fn needs_decompilation(&self) -> bool {
		self.bytecode.is_some()
	}

	/// Sets the program, dropping the bytecode (`setProgram`).
	pub fn set_program(&mut self, program: Vec<Token>) {
		self.program = program;
		self.bytecode = None;
	}

	fn token(&self, index: usize) -> Result<Option<(Token, usize)>> {
		match &self.bytecode {
			Some(b) => read_token_t1(b, index),
			None => Ok(self.program.get(index).map(|t| (t.clone(), index + 1))),
		}
	}

	/// Turns the bytecode into a program (`decompile`).
	pub fn decompile(&mut self) -> Result<()> {
		let Some(bytecode) = &self.bytecode else {
			return Ok(());
		};
		let mut program = Vec::new();
		let mut index = 0;
		while let Some((token, next)) = read_token_t1(bytecode, index)? {
			program.push(token);
			index = next;
		}
		self.set_program(program);
		Ok(())
	}

	/// Encodes the program, unless it is still bytecode (`compile`).
	pub fn compile(&mut self) -> Result<()> {
		if self.bytecode.is_some() {
			return Ok(());
		}
		let program = &self.program;
		if program.last().is_some_and(|t| !matches!(t, Token::Op(_))) {
			return Err(Error::ttlib(
				"T2CharString or Subr has items on the stack after last operator.",
			));
		}
		let mut out = Vec::new();
		for token in program {
			match token {
				Token::Op(name) => {
					let code = operator_code(name)
						.ok_or_else(|| Error::ttlib(format!("illegal operator: {name}")))?;
					if code >> 8 == 12 {
						out.push(12);
					}
					out.push(code as u8);
				}
				Token::Int(v) => encode_int_t1(*v, &mut out)?,
				// UPSTREAM-BUG(B63): `T1CharString.getFixedEncoder` returns `None`
				// instead of the encoder that raises "Type 1 charstrings don't
				// support floating point operands".
				Token::Float(_) => {
					return Err(Error::ttlib("TypeError: 'NoneType' object is not callable"));
				}
				Token::Mask(_) => return Err(Error::Assertion("unsupported type: bytes".into())),
			}
		}
		self.bytecode = Some(out);
		self.program = Vec::new();
		Ok(())
	}

	/// Draws the charstring onto `pen` with the subroutines `subrs`, and sets
	/// [`T1CharString::width`] (`draw`). Like fontTools, this decompiles the
	/// charstring and the subroutines it calls.
	pub fn draw(&mut self, pen: &mut dyn Pen, subrs: &mut [T1CharString]) -> Result<()> {
		let mut extractor = T1OutlineExtractor::new(pen, subrs);
		extractor.execute(self)?;
		self.width = Some(extractor.width.f());
		Ok(())
	}
}

/// A number of the operand stack: Python `int` or `float`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Num {
	Int(i64),
	Float(f64),
}

impl Num {
	fn f(self) -> f64 {
		match self {
			Num::Int(i) => i as f64,
			Num::Float(f) => f,
		}
	}

	fn add(self, o: Num) -> Num {
		match (self, o) {
			(Num::Int(a), Num::Int(b)) => match a.checked_add(b) {
				Some(v) => Num::Int(v),
				None => Num::Float(a as f64 + b as f64),
			},
			_ => Num::Float(self.f() + o.f()),
		}
	}

	fn sub(self, o: Num) -> Num {
		match (self, o) {
			(Num::Int(a), Num::Int(b)) => match a.checked_sub(b) {
				Some(v) => Num::Int(v),
				None => Num::Float(a as f64 - b as f64),
			},
			_ => Num::Float(self.f() - o.f()),
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

/// The Type 1 outline extractor (`T1OutlineExtractor`).
struct T1OutlineExtractor<'a, 'p> {
	pen: &'p mut dyn Pen,
	subrs: &'a mut [T1CharString],
	stack: Vec<Num>,
	flexing: bool,
	width: Num,
	sbx: Num,
	current: (Num, Num),
	saw_move_to: bool,
	subr_level: usize,
	depth: usize,
}

const MAX_DEPTH: usize = 500;

fn unpack_error(expected: usize, got: usize) -> Error {
	if got > expected {
		Error::value(format!(
			"ValueError: too many values to unpack (expected {expected})"
		))
	} else {
		Error::value(format!(
			"ValueError: not enough values to unpack (expected {expected}, got {got})"
		))
	}
}

impl<'a, 'p> T1OutlineExtractor<'a, 'p> {
	fn new(pen: &'p mut dyn Pen, subrs: &'a mut [T1CharString]) -> Self {
		T1OutlineExtractor {
			pen,
			subrs,
			stack: Vec::new(),
			flexing: false,
			width: Num::Int(0),
			sbx: Num::Int(0),
			current: (Num::Int(0), Num::Int(0)),
			saw_move_to: false,
			subr_level: 0,
			depth: 0,
		}
	}

	/// `T2OutlineExtractor.execute`, running `SimpleT2Decompiler.execute`.
	fn execute(&mut self, cs: &mut T1CharString) -> Result<()> {
		self.depth += 1;
		if self.depth > MAX_DEPTH {
			// LENIENT(L15): fontTools runs into Python's recursion limit.
			return Err(Error::ttlib(
				"RecursionError: maximum recursion depth exceeded",
			));
		}
		self.subr_level += 1;
		let decompiling = cs.needs_decompilation();
		let mut program = Vec::new();
		let mut index = 0;
		while let Some((token, next)) = cs.token(index)? {
			index = next;
			if decompiling {
				program.push(token.clone());
			}
			match token {
				Token::Op(op) => self.operator(op)?,
				Token::Int(v) => self.stack.push(Num::Int(v)),
				Token::Float(f) => self.stack.push(Num::Float(f)),
				Token::Mask(_) => return Err(Error::ttlib("TypeError: unexpected mask")),
			}
		}
		if decompiling {
			cs.set_program(program);
		}
		self.subr_level -= 1;
		if self.subr_level == 0 {
			self.end_path()?;
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

	fn popall_n<const N: usize>(&mut self) -> Result<[Num; N]> {
		let args = self.popall();
		let n = args.len();
		args.try_into().map_err(|_| unpack_error(N, n))
	}

	fn next_point(&mut self, (dx, dy): (Num, Num)) -> (f64, f64) {
		self.current = (self.current.0.add(dx), self.current.1.add(dy));
		(self.current.0.f(), self.current.1.f())
	}

	fn r_move_to(&mut self, p: (Num, Num)) -> Result<()> {
		let p = self.next_point(p);
		self.pen.move_to(p)?;
		self.saw_move_to = true;
		Ok(())
	}

	fn r_line_to(&mut self, p: (Num, Num)) -> Result<()> {
		if !self.saw_move_to {
			self.r_move_to((Num::Int(0), Num::Int(0)))?;
		}
		let p = self.next_point(p);
		self.pen.line_to(p)?;
		Ok(())
	}

	fn r_curve_to(&mut self, p1: (Num, Num), p2: (Num, Num), p3: (Num, Num)) -> Result<()> {
		if !self.saw_move_to {
			self.r_move_to((Num::Int(0), Num::Int(0)))?;
		}
		let a = self.next_point(p1);
		let b = self.next_point(p2);
		let c = self.next_point(p3);
		self.pen.curve_to(&[a, b, c])?;
		Ok(())
	}

	fn close_path(&mut self) -> Result<()> {
		if self.saw_move_to {
			self.pen.close_path()?;
		}
		self.saw_move_to = false;
		Ok(())
	}

	/// `T1OutlineExtractor.endPath`: Type 1 paths can be open.
	fn end_path(&mut self) -> Result<()> {
		if self.saw_move_to {
			self.pen.end_path()?;
		}
		self.saw_move_to = false;
		Ok(())
	}

	fn rrcurveto(&mut self) -> Result<()> {
		let args = self.popall();
		for c in args.chunks(6) {
			if c.len() != 6 {
				return Err(unpack_error(6, c.len()));
			}
			self.r_curve_to((c[0], c[1]), (c[2], c[3]), (c[4], c[5]))?;
		}
		Ok(())
	}

	fn vcurveto(&mut self, args: &[Num]) -> Result<usize> {
		if args.len() < 4 {
			return Err(unpack_error(4, args.len()));
		}
		let (dyc, used) = if args.len() == 5 {
			(args[4], 5)
		} else {
			(Num::Int(0), 4)
		};
		self.r_curve_to((Num::Int(0), args[0]), (args[1], args[2]), (args[3], dyc))?;
		Ok(used)
	}

	fn hcurveto(&mut self, args: &[Num]) -> Result<usize> {
		if args.len() < 4 {
			return Err(unpack_error(4, args.len()));
		}
		let (dxc, used) = if args.len() == 5 {
			(args[4], 5)
		} else {
			(Num::Int(0), 4)
		};
		self.r_curve_to((args[0], Num::Int(0)), (args[1], args[2]), (dxc, args[3]))?;
		Ok(used)
	}

	fn do_flex(&mut self) -> Result<()> {
		let finaly = self.pop()?;
		let finalx = self.pop()?;
		self.pop()?; // flex height is unused
		let mut v = [Num::Int(0); 14];
		for slot in v.iter_mut().rev() {
			*slot = self.pop()?;
		}
		let [
			rpx,
			rpy,
			bcp1x,
			bcp1y,
			bcp2x,
			bcp2y,
			p2x,
			p2y,
			bcp3x,
			bcp3y,
			bcp4x,
			bcp4y,
			p3x,
			p3y,
		] = v;
		self.stack
			.extend([bcp1x.add(rpx), bcp1y.add(rpy), bcp2x, bcp2y, p2x, p2y]);
		self.rrcurveto()?;
		self.stack.extend([bcp3x, bcp3y, bcp4x, bcp4y, p3x, p3y]);
		self.rrcurveto()?;
		self.stack.push(finalx);
		self.stack.push(finaly);
		Ok(())
	}

	fn operator(&mut self, op: &str) -> Result<()> {
		match op {
			"rmoveto" => {
				if self.flexing {
					return Ok(());
				}
				self.end_path()?;
				let args = self.popall();
				if args.len() < 2 {
					return Err(index_error());
				}
				self.r_move_to((args[0], args[1]))?;
			}
			"hmoveto" => {
				if self.flexing {
					self.stack.push(Num::Int(0));
					return Ok(());
				}
				self.end_path()?;
				let x = *self.popall().first().ok_or_else(index_error)?;
				self.r_move_to((x, Num::Int(0)))?;
			}
			"vmoveto" => {
				if self.flexing {
					self.stack.push(Num::Int(0));
					let n = self.stack.len();
					if n < 2 {
						return Err(index_error());
					}
					self.stack.swap(n - 1, n - 2);
					return Ok(());
				}
				self.end_path()?;
				let y = *self.popall().first().ok_or_else(index_error)?;
				self.r_move_to((Num::Int(0), y))?;
			}
			"closepath" => self.close_path()?,
			"setcurrentpoint" => {
				let [x, y] = self.popall_n::<2>()?;
				self.current = (x, y);
			}
			"endchar" => self.end_path()?,
			"hsbw" => {
				let [sbx, wx] = self.popall_n::<2>()?;
				self.width = wx;
				self.sbx = sbx;
				self.current = (sbx, self.current.1);
			}
			"sbw" | "dotsection" | "hstem3" | "vstem3" => {
				self.popall();
			}
			// `countHints`; the count is not used when drawing.
			"hstem" | "vstem" => {
				self.popall();
			}
			"callsubr" => {
				let i = self.pop()?.int()?;
				let n = self.subrs.len() as i64;
				let at = if i < 0 { i + n } else { i };
				let at = usize::try_from(at)
					.ok()
					.filter(|&a| a < self.subrs.len())
					.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
				let mut subr = std::mem::take(&mut self.subrs[at]);
				let result = self.execute(&mut subr);
				self.subrs[at] = subr;
				result?;
			}
			"callothersubr" => {
				let subr_index = self.pop()?;
				let n_args = self.pop()?;
				if subr_index.f() == 0.0 && n_args.f() == 3.0 {
					self.do_flex()?;
					self.flexing = false;
				} else if subr_index.f() == 1.0 && n_args.f() == 0.0 {
					self.flexing = true;
				}
			}
			"pop" | "return" => {}
			"seac" => {
				let [asb, adx, ady, bchar, achar] = self.popall_n::<5>()?;
				let enc = &fontoxide_encodings::standard_encoding::STANDARD_ENCODING;
				let glyph = |c: Num| -> Result<&'static str> {
					let i = c.int()?;
					let at = if i < 0 { i + 256 } else { i };
					usize::try_from(at)
						.ok()
						.and_then(|a| enc.get(a).copied())
						.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))
				};
				let base = glyph(bchar)?;
				self.pen
					.add_component(base, Transform::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0))?;
				let accent = glyph(achar)?;
				// seac weirdness
				let adx = adx.add(self.sbx).sub(asb);
				self.pen
					.add_component(accent, Transform::new(1.0, 0.0, 0.0, 1.0, adx.f(), ady.f()))?;
			}
			"div" => {
				let num2 = self.pop()?;
				let num1 = self.pop()?;
				if num2.f() == 0.0 {
					return Err(Error::ttlib("ZeroDivisionError: division by zero"));
				}
				let d2 = num1.f() / num2.f();
				let result = match (num1, num2) {
					(Num::Int(a), Num::Int(b)) => {
						let mut q = a.wrapping_div(b);
						if a.wrapping_rem(b) != 0 && ((a < 0) != (b < 0)) {
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
			"rlineto" => {
				let args = self.popall();
				for pair in args.chunks(2) {
					if pair.len() < 2 {
						if !self.saw_move_to {
							self.r_move_to((Num::Int(0), Num::Int(0)))?;
						}
						return Err(index_error());
					}
					self.r_line_to((pair[0], pair[1]))?;
				}
			}
			"hlineto" | "vlineto" => {
				let mut horizontal = op == "hlineto";
				for a in self.popall() {
					self.r_line_to(if horizontal {
						(a, Num::Int(0))
					} else {
						(Num::Int(0), a)
					})?;
					horizontal = !horizontal;
				}
			}
			"rrcurveto" => self.rrcurveto()?,
			"vhcurveto" | "hvcurveto" => {
				let args = self.popall();
				let mut rest = &args[..];
				let mut vertical = op == "vhcurveto";
				while !rest.is_empty() {
					let used = if vertical {
						self.vcurveto(rest)?
					} else {
						self.hcurveto(rest)?
					};
					rest = &rest[used..];
					vertical = !vertical;
				}
			}
			// LENIENT(L46): fontTools runs the Type 2 handlers of Type 2 operator
			// names put into a Type 1 program; such programs do not compile.
			_ => {
				self.popall();
			}
		}
		Ok(())
	}
}
