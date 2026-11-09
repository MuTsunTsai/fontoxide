//! Type 2 charstring operator specializer and generalizer
//! (`fontTools.cffLib.specializer`).
//!
//! Glyph drawing operations can be expressed in multiple different ways. For
//! example, as well as the `lineto` operator, there is also a `hlineto` operator
//! which draws a horizontal line, removing the need to specify a `dx` coordinate.
//! This module converts programs into lists of commands, and those between general
//! and specific forms.

use super::charstring::Token;
use crate::{Error, Result};

/// `maxStackLimit`.
const MAX_STACK_LIMIT: i64 = 513;

/// An argument of a command.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
	/// An `int`.
	Int(i64),
	/// A `float`.
	Float(f64),
	/// A list of the arguments of a `blend` operator, ending with the number of
	/// blends.
	Blend(Vec<Arg>),
	/// The mask bytes after `hintmask` or `cntrmask`.
	Mask(Vec<u8>),
	/// An operator as data: for an operator kept as an argument by `ignoreErrors`, or
	/// in place of the mask of a `hintmask` or `cntrmask`.
	Op(&'static str),
}

impl Arg {
	/// `isinstance(arg, list)`.
	pub fn is_blend(&self) -> bool {
		matches!(self, Arg::Blend(_))
	}

	/// `bool(arg)`.
	pub fn is_truthy(&self) -> bool {
		match self {
			Arg::Int(v) => *v != 0,
			Arg::Float(v) => *v != 0.0,
			Arg::Blend(v) => !v.is_empty(),
			Arg::Mask(v) => !v.is_empty(),
			Arg::Op(v) => !v.is_empty(),
		}
	}

	/// The value of a number.
	pub fn number(&self) -> Result<f64> {
		match self {
			Arg::Int(v) => Ok(*v as f64),
			Arg::Float(v) => Ok(*v),
			_ => Err(type_error()),
		}
	}

	/// `==`, where `1 == 1.0`.
	pub fn py_eq(&self, other: &Arg) -> bool {
		match (self, other) {
			(Arg::Blend(a), Arg::Blend(b)) => {
				a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.py_eq(y))
			}
			(Arg::Mask(a), Arg::Mask(b)) => a == b,
			(Arg::Op(a), Arg::Op(b)) => a == b,
			(Arg::Int(a), Arg::Int(b)) => a == b,
			(a, b) => match (a.number(), b.number()) {
				(Ok(a), Ok(b)) => a == b,
				_ => false,
			},
		}
	}

	/// `a + b` of numbers.
	pub fn plus(&self, other: &Arg) -> Result<Arg> {
		match (self, other) {
			(Arg::Int(a), Arg::Int(b)) => Ok(Arg::Int(a + b)),
			(a, b) => Ok(Arg::Float(a.number()? + b.number()?)),
		}
	}

	/// `a - b` of numbers.
	pub fn minus(&self, other: &Arg) -> Result<Arg> {
		match (self, other) {
			(Arg::Int(a), Arg::Int(b)) => Ok(Arg::Int(a - b)),
			(a, b) => Ok(Arg::Float(a.number()? - b.number()?)),
		}
	}

	fn negated(&self) -> Result<Arg> {
		match self {
			Arg::Int(v) => Ok(Arg::Int(-v)),
			Arg::Float(v) => Ok(Arg::Float(-v)),
			_ => Err(type_error()),
		}
	}
}

/// A command: an operator with its arguments. The operator is empty if none shall be
/// emitted (for the glyph width, the hint mask, and stray arguments at the end).
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
	/// The operator.
	pub op: &'static str,
	/// The arguments.
	pub args: Vec<Arg>,
}

impl Command {
	/// A command.
	pub fn new(op: &'static str, args: Vec<Arg>) -> Self {
		Command { op, args }
	}
}

fn type_error() -> Error {
	Error::ttlib("TypeError: unsupported operand type")
}

fn value_error(args: &[Arg]) -> Error {
	Error::value(format!("{args:?}"))
}

fn assertion(what: &str) -> Error {
	Error::Assertion(what.into())
}

fn int(arg: &Arg) -> Result<i64> {
	match arg {
		Arg::Int(v) => Ok(*v),
		_ => Err(type_error()),
	}
}

/// Number of the region of each `vsindex`.
pub type NumRegions<'a> = &'a dyn Fn(i64) -> Result<i64>;
/// The `(minimum, maximum)` scalars of the regions of each `vsindex`.
pub type ScalarRanges<'a> = &'a dyn Fn(i64) -> Result<Vec<(f64, f64)>>;

/// `stack[start:]`, with what Python does with a slice of `-n`.
fn slice_start(len: usize, n: i64) -> usize {
	if n > 0 {
		len.saturating_sub(n as usize)
	} else {
		((-n) as usize).min(len)
	}
}

/// `programToCommands`: takes a program and returns a list of commands.
///
/// `vs_index` is the `vsindex` in effect until the program sets one: that of the
/// Private DICT the charstring uses. The charstring may or may not start with a
/// width value: if the first non-blend operator has an odd number of arguments, then
/// the first argument is a width, and is popped off.
pub fn program_to_commands(
	program: &[Token],
	get_num_regions: Option<NumRegions<'_>>,
	mut vs_index: i64,
) -> Result<Vec<Command>> {
	let mut seen_width_op = false;
	let mut len_blend_stack: i64 = 0;
	let mut last_blend_index = 0;
	let mut commands = Vec::new();
	let mut stack: Vec<Arg> = Vec::new();
	let mut it = program.iter();

	while let Some(token) = it.next() {
		let op = match token {
			Token::Int(v) => {
				stack.push(Arg::Int(*v));
				continue;
			}
			Token::Float(v) => {
				stack.push(Arg::Float(*v));
				continue;
			}
			Token::Mask(v) => {
				stack.push(Arg::Mask(v.clone()));
				continue;
			}
			Token::Op(op) => *op,
		};

		if op == "blend" {
			let get_num_regions =
				get_num_regions.ok_or_else(|| assertion("getNumRegions is not None"))?;
			let num_source_fonts = 1 + get_num_regions(vs_index)?;
			// Replace the blend op args on the stack with a single list containing all
			// the blend op args.
			let num_blends = int(stack
				.last()
				.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?)?;
			let num_blend_args = num_blends * num_source_fonts + 1;
			let start = slice_start(stack.len(), num_blend_args);
			let blend = stack.split_off(start);
			stack.push(Arg::Blend(blend));
			let len_stack = stack.len();
			len_blend_stack += num_blends + len_stack as i64 - 1 - last_blend_index as i64;
			last_blend_index = len_stack;
			// If a blend op exists, this is or will be a CFF2 charstring.
			continue;
		} else if op == "vsindex" {
			vs_index = match stack.last() {
				Some(Arg::Int(v)) => *v,
				Some(_) => return Err(assertion("type(vsIndex) is int")),
				None => return Err(Error::ttlib("IndexError: list index out of range")),
			};
		} else if !seen_width_op
			&& matches!(
				op,
				"hstem"
					| "hstemhm" | "vstem"
					| "vstemhm" | "cntrmask"
					| "hintmask" | "hmoveto"
					| "vmoveto" | "rmoveto"
					| "endchar"
			) {
			seen_width_op = true;
			let parity = matches!(op, "hmoveto" | "vmoveto");
			let num_args = if len_blend_stack != 0 {
				// `len_blend_stack` has the number of args represented by the last blend
				// arg and all the preceding args. Now add the args following it.
				len_blend_stack + stack.len().saturating_sub(last_blend_index) as i64
			} else {
				stack.len() as i64
			};
			if num_args != 0 && (num_args.rem_euclid(2) == 1) != parity {
				if stack.is_empty() {
					return Err(Error::ttlib("IndexError: pop from empty list"));
				}
				let width = stack.remove(0);
				commands.push(Command::new("", vec![width]));
			}
		}

		if matches!(op, "hintmask" | "cntrmask") {
			if !stack.is_empty() {
				commands.push(Command::new("", std::mem::take(&mut stack)));
			}
			commands.push(Command::new(op, Vec::new()));
			let mask = match it.next() {
				Some(Token::Mask(v)) => Arg::Mask(v.clone()),
				Some(Token::Int(v)) => Arg::Int(*v),
				Some(Token::Float(v)) => Arg::Float(*v),
				Some(Token::Op(v)) => Arg::Op(v),
				None => return Err(Error::ttlib("StopIteration")),
			};
			commands.push(Command::new("", vec![mask]));
		} else {
			commands.push(Command::new(op, std::mem::take(&mut stack)));
		}
	}
	if !stack.is_empty() {
		commands.push(Command::new("", stack));
	}
	Ok(commands)
}

/// `commandsToProgram`: converts commands back to a program.
pub fn commands_to_program(commands: &[Command]) -> Result<Vec<Token>> {
	let mut program = Vec::new();
	let simple = |arg: &Arg, program: &mut Vec<Token>| -> Result<()> {
		program.push(match arg {
			Arg::Int(v) => Token::Int(*v),
			Arg::Float(v) => Token::Float(*v),
			Arg::Mask(v) => Token::Mask(v.clone()),
			Arg::Op(v) => Token::Op(v),
			Arg::Blend(_) => return Err(type_error()),
		});
		Ok(())
	};
	for command in commands {
		for arg in &command.args {
			if let Arg::Blend(items) = arg {
				for item in items {
					simple(item, &mut program)?;
				}
				program.push(Token::Op("blend"));
			} else {
				simple(arg, &mut program)?;
			}
		}
		if !command.op.is_empty() {
			program.push(Token::Op(command.op));
		}
	}
	Ok(program)
}

/// `_everyN`.
fn every_n(args: &[Arg], n: usize) -> Result<std::slice::Chunks<'_, Arg>> {
	if !args.len().is_multiple_of(n) {
		return Err(value_error(args));
	}
	Ok(args.chunks(n))
}

fn zero() -> Arg {
	Arg::Int(0)
}

/// `_GeneralizerDecombinerCommandsMap`: `None` if the operator stays as it is.
struct Decombiner<'a> {
	get_scalar_ranges: Option<ScalarRanges<'a>>,
	vs_index: i64,
}

impl Decombiner<'_> {
	#[allow(clippy::too_many_lines)]
	fn apply(&mut self, op: &str, args: &[Arg]) -> Result<Option<Vec<Command>>> {
		let mut out = Vec::new();
		let a = |i: usize| args[i].clone();
		let rlineto = |x: Arg, y: Arg| Command::new("rlineto", vec![x, y]);
		let rrcurveto = |v: [Arg; 6]| Command::new("rrcurveto", v.to_vec());
		match op {
			"vsindex" => {
				if let Arg::Int(v) = args
					.first()
					.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?
				{
					self.vs_index = *v;
				}
				out.push(Command::new("vsindex", args.to_vec()));
			}
			"rmoveto" => {
				if args.len() != 2 {
					return Err(value_error(args));
				}
				out.push(Command::new("rmoveto", args.to_vec()));
			}
			"hmoveto" => {
				if args.len() != 1 {
					return Err(value_error(args));
				}
				out.push(Command::new("rmoveto", vec![a(0), zero()]));
			}
			"vmoveto" => {
				if args.len() != 1 {
					return Err(value_error(args));
				}
				out.push(Command::new("rmoveto", vec![zero(), a(0)]));
			}
			"rlineto" => {
				if args.is_empty() {
					return Err(value_error(args));
				}
				for c in every_n(args, 2)? {
					out.push(Command::new("rlineto", c.to_vec()));
				}
			}
			"hlineto" | "vlineto" => {
				if args.is_empty() {
					return Err(value_error(args));
				}
				let mut horizontal = op == "hlineto";
				for arg in args {
					out.push(if horizontal {
						rlineto(arg.clone(), zero())
					} else {
						rlineto(zero(), arg.clone())
					});
					horizontal = !horizontal;
				}
			}
			"rrcurveto" => {
				if args.is_empty() {
					return Err(value_error(args));
				}
				for c in every_n(args, 6)? {
					out.push(Command::new("rrcurveto", c.to_vec()));
				}
			}
			"hhcurveto" => {
				let l = args.len();
				if l < 4 || l % 4 > 1 {
					return Err(value_error(args));
				}
				let mut rest = args;
				if l % 2 == 1 {
					out.push(rrcurveto([a(1), a(0), a(2), a(3), a(4), zero()]));
					rest = &args[5..];
				}
				for c in every_n(rest, 4)? {
					out.push(rrcurveto([
						c[0].clone(),
						zero(),
						c[1].clone(),
						c[2].clone(),
						c[3].clone(),
						zero(),
					]));
				}
			}
			"vvcurveto" => {
				let l = args.len();
				if l < 4 || l % 4 > 1 {
					return Err(value_error(args));
				}
				let mut rest = args;
				if l % 2 == 1 {
					out.push(rrcurveto([a(0), a(1), a(2), a(3), zero(), a(4)]));
					rest = &args[5..];
				}
				for c in every_n(rest, 4)? {
					out.push(rrcurveto([
						zero(),
						c[0].clone(),
						c[1].clone(),
						c[2].clone(),
						zero(),
						c[3].clone(),
					]));
				}
			}
			"hvcurveto" | "vhcurveto" => {
				let l = args.len();
				if l < 4 || !matches!(l % 8, 0 | 1 | 4 | 5) {
					return Err(value_error(args));
				}
				let (mut rest, mut last) = (args, None);
				let mut last_straight = false;
				if l % 2 == 1 {
					last_straight = l % 8 == 5;
					rest = &args[..l - 5];
					last = Some(&args[l - 5..]);
				}
				let mut horizontal = op == "hvcurveto";
				for c in every_n(rest, 4)? {
					out.push(if horizontal {
						rrcurveto([
							c[0].clone(),
							zero(),
							c[1].clone(),
							c[2].clone(),
							zero(),
							c[3].clone(),
						])
					} else {
						rrcurveto([
							zero(),
							c[0].clone(),
							c[1].clone(),
							c[2].clone(),
							c[3].clone(),
							zero(),
						])
					});
					horizontal = !horizontal;
				}
				if let Some(c) = last {
					// Which of the two forms depends on the operator only.
					let hv_first = (op == "hvcurveto") == last_straight;
					out.push(if hv_first {
						rrcurveto([
							c[0].clone(),
							zero(),
							c[1].clone(),
							c[2].clone(),
							c[4].clone(),
							c[3].clone(),
						])
					} else {
						rrcurveto([
							zero(),
							c[0].clone(),
							c[1].clone(),
							c[2].clone(),
							c[3].clone(),
							c[4].clone(),
						])
					});
				}
			}
			"rcurveline" => {
				let l = args.len();
				if l < 8 || l % 6 != 2 {
					return Err(value_error(args));
				}
				for c in every_n(&args[..l - 2], 6)? {
					out.push(Command::new("rrcurveto", c.to_vec()));
				}
				out.push(Command::new("rlineto", args[l - 2..].to_vec()));
			}
			"rlinecurve" => {
				let l = args.len();
				if l < 8 || !l.is_multiple_of(2) {
					return Err(value_error(args));
				}
				for c in every_n(&args[..l - 6], 2)? {
					out.push(Command::new("rlineto", c.to_vec()));
				}
				out.push(Command::new("rrcurveto", args[l - 6..].to_vec()));
			}
			"hflex" => {
				if args.len() != 7 {
					return Err(value_error(args));
				}
				out.push(Command::new(
					"flex",
					vec![
						a(0),
						zero(),
						a(1),
						a(2),
						a(3),
						zero(),
						a(4),
						zero(),
						a(5),
						negate_blend_arg(&args[2])?,
						a(6),
						zero(),
						Arg::Int(50),
					],
				));
			}
			"hflex1" => {
				if args.len() != 9 {
					return Err(value_error(args));
				}
				let dy6 = negate_blend_arg(&sum_blend_args(&[a(1), a(3), a(7)])?)?;
				out.push(Command::new(
					"flex",
					vec![
						a(0),
						a(1),
						a(2),
						a(3),
						a(4),
						zero(),
						a(5),
						zero(),
						a(6),
						a(7),
						a(8),
						dy6,
						Arg::Int(50),
					],
				));
			}
			"flex1" => {
				if args.len() != 11 {
					return Err(value_error(args));
				}
				let ranges = match self.get_scalar_ranges {
					Some(f) => Some(f(self.vs_index)?),
					None => None,
				};
				let (dx, dy) = sums_of_flex(args)?;
				match is_flex1_horizontal(&dx, &dy, ranges.as_deref())? {
					// A flex1 whose direction changes cannot be represented as flex.
					None => out.push(Command::new("flex1", args.to_vec())),
					Some(horizontal) => {
						let (x, y) = if horizontal {
							(a(10), negate_blend_arg(&dy)?)
						} else {
							(negate_blend_arg(&dx)?, a(10))
						};
						let mut new = args[..10].to_vec();
						new.extend([x, y, Arg::Int(50)]);
						out.push(Command::new("flex", new));
					}
				}
			}
			_ => return Ok(None),
		}
		Ok(Some(out))
	}
}

/// `dx` and `dy` of the first five vectors of a flex.
fn sums_of_flex(args: &[Arg]) -> Result<(Arg, Arg)> {
	let of =
		|start: usize| -> Vec<Arg> { args[..10].iter().skip(start).step_by(2).cloned().collect() };
	Ok((sum_blend_args(&of(0))?, sum_blend_args(&of(1))?))
}

fn floor_div(a: i64, b: i64) -> Result<i64> {
	if b == 0 {
		return Err(Error::ttlib(
			"ZeroDivisionError: integer division or modulo by zero",
		));
	}
	let q = a / b;
	Ok(if a % b != 0 && (a < 0) != (b < 0) {
		q - 1
	} else {
		q
	})
}

/// `_convertBlendOpToArgs`.
fn convert_blend_op_to_args(blend_list: &[Arg]) -> Result<Vec<Arg>> {
	// Blend op calls can be recursive: convert those first.
	let flattened;
	let args: &[Arg] = if blend_list.iter().any(Arg::is_blend) {
		let mut items = Vec::new();
		for e in blend_list {
			if let Arg::Blend(inner) = e {
				items.extend(convert_blend_op_to_args(inner)?);
			} else {
				items.push(e.clone());
			}
		}
		flattened = items;
		&flattened
	} else {
		blend_list
	};

	// Convert from:
	//   [default font arg sequence x0,...,xn] + [delta tuple for x0] + ... + [delta tuple for xn]
	// to:
	//   [ [x0] + [delta tuple for x0], ..., [xn] + [delta tuple for xn] ]
	let (last, args) = args
		.split_last()
		.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
	let num_blends = int(last)?;
	let l = args.len() as i64;
	let num_regions = floor_div(l, num_blends)? - 1;
	if num_blends * (num_regions + 1) != l {
		return Err(value_error(blend_list));
	}
	if num_regions == 0 {
		return Ok(args.to_vec());
	}
	if num_regions < 0 {
		// `range` with a negative step is empty here, so nothing gets zipped.
		return Ok(Vec::new());
	}
	let (num_blends, num_regions) = (num_blends as usize, num_regions as usize);
	let (defaults, deltas) = args.split_at(num_blends);
	let mut out = Vec::with_capacity(num_blends);
	for (default, deltas) in defaults.iter().zip(deltas.chunks(num_regions)) {
		let mut blend = Vec::with_capacity(num_regions + 2);
		blend.push(default.clone());
		blend.extend_from_slice(deltas);
		blend.push(Arg::Int(1));
		out.push(Arg::Blend(blend));
	}
	Ok(out)
}

/// `generalizeCommands`.
pub fn generalize_commands(
	commands: &[Command],
	ignore_errors: bool,
	get_scalar_ranges: Option<ScalarRanges<'_>>,
	vs_index: i64,
) -> Result<Vec<Command>> {
	let mut result = Vec::with_capacity(commands.len());
	let mut mapping = Decombiner {
		get_scalar_ranges,
		vs_index,
	};
	for command in commands {
		// First, generalize any blend args in the arg list.
		let mut args = command.args.clone();
		if args.iter().any(Arg::is_blend) {
			let mut flat = Vec::with_capacity(args.len());
			let mut failed = None;
			for arg in &args {
				if let Arg::Blend(blend) = arg {
					match convert_blend_op_to_args(blend) {
						Ok(items) => flat.extend(items),
						Err(e @ Error::Value(_)) => {
							failed = Some(e);
							break;
						}
						Err(e) => return Err(e),
					}
				} else {
					flat.push(arg.clone());
				}
			}
			match failed {
				None => args = flat,
				Some(e) if !ignore_errors => return Err(e),
				// UPSTREAM: the arguments stay as they are, and are still processed.
				Some(_) => {
					result.push(Command::new("", args.clone()));
					result.push(Command::new("", vec![op_as_data(command.op)]));
				}
			}
		}
		match mapping.apply(command.op, &args) {
			Ok(None) => result.push(Command::new(command.op, args)),
			Ok(Some(new)) => result.extend(new),
			Err(e @ Error::Value(_)) => {
				if !ignore_errors {
					return Err(e);
				}
				// Store op as data, such that consumers of commands do not have to deal
				// with incorrect number of arguments.
				result.push(Command::new("", args));
				result.push(Command::new("", vec![op_as_data(command.op)]));
			}
			Err(e) => return Err(e),
		}
	}
	Ok(result)
}

/// The operator as an argument.
fn op_as_data(op: &'static str) -> Arg {
	Arg::Op(op)
}

/// `generalizeProgram`.
pub fn generalize_program(
	program: &[Token],
	get_num_regions: Option<NumRegions<'_>>,
	vs_index: i64,
) -> Result<Vec<Token>> {
	commands_to_program(&generalize_commands(
		&program_to_commands(program, get_num_regions, vs_index)?,
		false,
		None,
		vs_index,
	)?)
}

/// `_categorizeVector`: one of r, h, v, or 0 depending on which of X and/or Y are
/// zero, plus the nonzero ones. If both are zero, a single zero still.
///
/// As in fontTools, anything after X and Y stays with the Y of a vector that has one.
#[doc(hidden)]
pub fn categorize_vector(v: &[Arg]) -> Result<(u8, Vec<Arg>)> {
	let [x, y, ..] = v else {
		return Err(Error::ttlib("IndexError: tuple index out of range"));
	};
	Ok(if !x.is_truthy() {
		if !y.is_truthy() {
			(b'0', v[..1].to_vec())
		} else {
			(b'v', v[1..].to_vec())
		}
	} else if !y.is_truthy() {
		(b'h', v[..1].to_vec())
	} else {
		(b'r', v.to_vec())
	})
}

fn merge_categories(a: u8, b: u8) -> Option<u8> {
	if a == b'0' {
		Some(b)
	} else if b == b'0' || a == b {
		Some(a)
	} else {
		None
	}
}

fn negate_category(a: u8) -> u8 {
	match a {
		b'h' => b'v',
		b'v' => b'h',
		a => a,
	}
}

fn moveto_name(c: u8) -> &'static str {
	match c {
		b'0' => "0moveto",
		b'h' => "hmoveto",
		b'v' => "vmoveto",
		_ => "rmoveto",
	}
}

fn lineto_name(c: u8) -> &'static str {
	match c {
		b'0' => "0lineto",
		b'h' => "hlineto",
		b'v' => "vlineto",
		_ => "rlineto",
	}
}

fn curveto_name(a: u8, b: u8) -> &'static str {
	match (a, b) {
		(b'0', b'0') => "00curveto",
		(b'0', b'h') => "0hcurveto",
		(b'0', b'v') => "0vcurveto",
		(b'0', _) => "0rcurveto",
		(b'h', b'0') => "h0curveto",
		(b'h', b'h') => "hhcurveto",
		(b'h', b'v') => "hvcurveto",
		(b'h', _) => "hrcurveto",
		(b'v', b'0') => "v0curveto",
		(b'v', b'h') => "vhcurveto",
		(b'v', b'v') => "vvcurveto",
		(b'v', _) => "vrcurveto",
		(_, b'0') => "r0curveto",
		(_, b'h') => "rhcurveto",
		(_, b'v') => "rvcurveto",
		_ => "rrcurveto",
	}
}

/// `_convertToBlendCmds`: a list of blend commands, and the remaining non-blended
/// args, if any.
fn convert_to_blend_cmds(args: Vec<Arg>) -> Result<Vec<Arg>> {
	let num_args = args.len();
	let mut stack_use: i64 = 0;
	let mut new_args = Vec::new();
	let mut i = 0;
	while i < num_args {
		let arg = &args[i];
		i += 1;
		let Arg::Blend(first) = arg else {
			new_args.push(arg.clone());
			stack_use += 1;
			continue;
		};
		let prev_stack_use = stack_use;
		let empty = |b: &[Arg]| {
			if b.is_empty() {
				Err(Error::ttlib("IndexError: list index out of range"))
			} else {
				Ok(())
			}
		};
		// These are each (master 0, delta 1..delta n, 1). Combine as many successive
		// tuples as we can, up to the max stack limit.
		let num_sources = first.len() as i64 - 1;
		let mut blend_list = vec![first];
		stack_use += 1 + num_sources; // 1 for the num_blends arg

		// If we are here, max stack is the CFF2 max stack. Make sure the stack depth
		// does not exceed (maxstack - 1), so that subroutinizer can insert subroutine
		// calls at any point.
		while i < num_args && stack_use + num_sources < MAX_STACK_LIMIT {
			let Arg::Blend(next) = &args[i] else {
				break;
			};
			blend_list.push(next);
			i += 1;
			stack_use += num_sources;
		}
		let num_blends = blend_list.len() as i64;
		// Append the 'num_blends' default font values.
		let mut blend_args = Vec::new();
		for b in &blend_list {
			empty(b)?;
			blend_args.push(b[0].clone());
		}
		for b in &blend_list {
			if !matches!(b.last(), Some(Arg::Int(1)))
				&& !b.last().is_some_and(|l| l.py_eq(&Arg::Int(1)))
			{
				return Err(assertion("arg[-1] == 1"));
			}
			blend_args.extend_from_slice(b.get(1..b.len() - 1).unwrap_or_default());
		}
		blend_args.push(Arg::Int(num_blends));
		new_args.push(Arg::Blend(blend_args));
		stack_use = prev_stack_use + num_blends;
	}
	Ok(new_args)
}

/// `_addArgs`.
fn add_args(a: &Arg, b: &Arg) -> Result<Arg> {
	if let Arg::Blend(vb) = b {
		if let Arg::Blend(va) = a {
			let (Some(la), Some(lb)) = (va.last(), vb.last()) else {
				return Err(Error::ttlib("IndexError: list index out of range"));
			};
			if va.len() != vb.len() || !la.py_eq(lb) {
				return Err(Error::value(""));
			}
			let mut out = Vec::with_capacity(va.len());
			for (x, y) in va[..va.len() - 1].iter().zip(&vb[..vb.len() - 1]) {
				out.push(add_args(x, y)?);
			}
			out.push(la.clone());
			return Ok(Arg::Blend(out));
		}
		return add_args(b, a);
	}
	if let Arg::Blend(va) = a {
		if !va.last().is_some_and(|l| l.py_eq(&Arg::Int(1))) {
			return Err(assertion("a[-1] == 1"));
		}
		let mut out = va.clone();
		out[0] = add_args(&va[0], b)?;
		return Ok(Arg::Blend(out));
	}
	a.plus(b)
}

/// `blendArgValues`: a generalized argument's default and deltas, padded with zeros.
///
/// `arg` is a number or a single-value blend list `[default, *deltas, 1]` as returned
/// by [`generalize_commands`]. The result has `num_values` entries, including the
/// default.
pub fn blend_arg_values(arg: &Arg, num_values: usize) -> Result<Vec<Arg>> {
	let mut values = match arg {
		Arg::Blend(v) => v[..v.len().saturating_sub(1)].to_vec(),
		arg => vec![arg.clone()],
	};
	if values.iter().any(Arg::is_blend) {
		return Err(Error::ttlib(
			"NotImplementedError: Nested blend lists not supported (yet)",
		));
	}
	if values.len() > num_values {
		return Err(assertion("len(values) <= numValues"));
	}
	values.resize(num_values, zero());
	Ok(values)
}

/// `numBlendArgValues`: the number of default and delta values of a generalized
/// argument.
pub fn num_blend_arg_values(arg: &Arg) -> usize {
	match arg {
		Arg::Blend(v) => v.len().saturating_sub(1),
		_ => 1,
	}
}

fn blend_args_equal(a: &Arg, b: &Arg) -> Result<bool> {
	let n = num_blend_arg_values(a).max(num_blend_arg_values(b));
	let (va, vb) = (blend_arg_values(a, n)?, blend_arg_values(b, n)?);
	Ok(va.iter().zip(&vb).all(|(x, y)| x.py_eq(y)))
}

fn negate_blend_arg(arg: &Arg) -> Result<Arg> {
	match arg {
		Arg::Blend(v) => {
			let (last, values) = v
				.split_last()
				.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
			let mut out = values
				.iter()
				.map(Arg::negated)
				.collect::<Result<Vec<_>>>()?;
			out.push(last.clone());
			Ok(Arg::Blend(out))
		}
		arg => arg.negated(),
	}
}

fn sum_blend_args(args: &[Arg]) -> Result<Arg> {
	let (first, rest) = args
		.split_first()
		.ok_or_else(|| Error::value("TypeError: reduce() of empty iterable"))?;
	let mut sum = first.clone();
	for arg in rest {
		sum = add_args(&sum, arg)?;
	}
	Ok(sum)
}

/// `_isFlex1Horizontal`: whether a flex1 ends horizontally throughout the scalar
/// ranges, which contain a `(minimum, maximum)` pair per variation region and default
/// to `(0, 1)` for each region. `None` when the direction may change.
fn is_flex1_horizontal(
	dx: &Arg,
	dy: &Arg,
	scalar_ranges: Option<&[(f64, f64)]>,
) -> Result<Option<bool>> {
	let bounds = |arg: &Arg| -> Result<(f64, f64)> {
		let values = blend_arg_values(arg, num_blend_arg_values(arg))?;
		let (default, deltas) = values
			.split_first()
			.ok_or_else(|| Error::value("not enough values to unpack"))?;
		let ranges = scalar_ranges
			.filter(|r| !r.is_empty())
			.map_or_else(|| vec![(0.0, 1.0); deltas.len()], <[_]>::to_vec);
		let default = default.number()?;
		let (mut lo, mut hi) = (default, default);
		for (delta, (min_scalar, max_scalar)) in deltas.iter().zip(ranges) {
			let delta = delta.number()?;
			lo += (min_scalar * delta).min(max_scalar * delta);
			hi += (min_scalar * delta).max(max_scalar * delta);
		}
		Ok((lo, hi))
	};
	let min_abs = |(lo, hi): (f64, f64)| {
		if lo <= 0.0 && 0.0 <= hi {
			0.0
		} else {
			lo.abs().min(hi.abs())
		}
	};
	let max_abs = |(lo, hi): (f64, f64)| lo.abs().max(hi.abs());
	let (x, y) = (bounds(dx)?, bounds(dy)?);
	if min_abs(x) > max_abs(y) {
		return Ok(Some(true));
	}
	if max_abs(x) <= min_abs(y) {
		return Ok(Some(false));
	}
	Ok(None)
}

/// `_specializeFlex`: the shortest flex operator and arguments for `flex` args.
fn specialize_flex(args: Vec<Arg>) -> Result<Command> {
	if args.len() != 13 {
		return Err(Error::value(format!(
			"not enough values to unpack (expected 13, got {})",
			args.len()
		)));
	}
	let a = |i: usize| args[i].clone();
	let (dx6, dy6, fd) = (&args[10], &args[11], &args[12]);
	let (dy1, dy2, dy3, dy4, dy5) = (&args[1], &args[3], &args[5], &args[7], &args[9]);
	if !blend_args_equal(fd, &Arg::Int(50))? {
		return Ok(Command::new("flex", args));
	}
	if blend_args_equal(dy3, &zero())? && blend_args_equal(dy4, &zero())? {
		if blend_args_equal(dy1, &zero())?
			&& blend_args_equal(dy6, &zero())?
			&& blend_args_equal(dy5, &negate_blend_arg(dy2)?)?
		{
			return Ok(Command::new(
				"hflex",
				vec![a(0), a(2), a(3), a(4), a(6), a(8), a(10)],
			));
		}
		let sum = sum_blend_args(&[dy1.clone(), dy2.clone(), dy5.clone()])?;
		if blend_args_equal(dy6, &negate_blend_arg(&sum)?)? {
			return Ok(Command::new(
				"hflex1",
				vec![a(0), a(1), a(2), a(3), a(4), a(6), a(8), a(9), a(10)],
			));
		}
	}
	let (dx, dy) = sums_of_flex(&args)?;
	let horizontal = is_flex1_horizontal(&dx, &dy, None)?;
	if horizontal == Some(true) && blend_args_equal(dy6, &negate_blend_arg(&dy)?)? {
		let mut new = args[..10].to_vec();
		new.push(dx6.clone());
		return Ok(Command::new("flex1", new));
	}
	if horizontal == Some(false) && blend_args_equal(dx6, &negate_blend_arg(&dx)?)? {
		let mut new = args[..10].to_vec();
		new.push(dy6.clone());
		return Ok(Command::new("flex1", new));
	}
	Ok(Command::new("flex", args))
}

/// `_argsStackUse`.
fn args_stack_use(args: &[Arg]) -> Result<i64> {
	let mut stack_len = 0;
	let mut max_len = 0;
	for arg in args {
		if let Arg::Blend(items) = arg {
			// Blended arg
			max_len = max_len.max(stack_len + args_stack_use(items)?);
			stack_len += int(items
				.last()
				.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?)?;
		} else {
			stack_len += 1;
		}
	}
	Ok(stack_len.max(max_len))
}

fn is_curveto(op: &str) -> bool {
	op.len() == 9 && op.ends_with("curveto")
}

/// The options of [`specialize_commands`].
#[derive(Clone, Copy)]
pub struct SpecializeOptions<'a> {
	/// `ignoreErrors`.
	pub ignore_errors: bool,
	/// `generalizeFirst`.
	pub generalize_first: bool,
	/// `preserveTopology`.
	pub preserve_topology: bool,
	/// `maxstack`.
	pub max_stack: i64,
	/// `getScalarRanges`.
	pub get_scalar_ranges: Option<ScalarRanges<'a>>,
	/// `vsIndex`.
	pub vs_index: i64,
}

impl std::fmt::Debug for SpecializeOptions<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("SpecializeOptions")
			.field("ignore_errors", &self.ignore_errors)
			.field("generalize_first", &self.generalize_first)
			.field("preserve_topology", &self.preserve_topology)
			.field("max_stack", &self.max_stack)
			.field("vs_index", &self.vs_index)
			.finish_non_exhaustive()
	}
}

impl Default for SpecializeOptions<'_> {
	fn default() -> Self {
		SpecializeOptions {
			ignore_errors: false,
			generalize_first: true,
			preserve_topology: false,
			max_stack: 48,
			get_scalar_ranges: None,
			vs_index: 0,
		}
	}
}

/// `specializeCommands`, in several carefully ordered rounds of optimizations:
///
/// 0. Generalize commands, so that each line/curve only has arguments for one segment,
///    and uses the generic form (rlineto/rrcurveto).
/// 1. Combine successive rmoveto operations.
/// 2. Specialize rmoveto/rlineto/rrcurveto into horizontal/vertical variants, and flex
///    into hflex/hflex1/flex1. This includes some made-up variants, for when both X and
///    Y components are zero, which simplify the following passes.
/// 3. Merge or delete redundant operations, to the extent requested.
/// 4. Peephole optimization to revert back some of the h/v variants back into their
///    original "relative" operator (rline/rrcurveto) if that saves a byte.
/// 5. Combine adjacent operators when possible, minding not to go over max stack size.
/// 6. Resolve any remaining made-up operators into real operators.
/// 7. For any args which are blend lists, convert them to a blend command.
#[allow(clippy::too_many_lines)]
pub fn specialize_commands(
	commands: &[Command],
	options: &SpecializeOptions<'_>,
) -> Result<Vec<Command>> {
	// 0. Generalize commands.
	let mut commands = if options.generalize_first {
		generalize_commands(
			commands,
			options.ignore_errors,
			options.get_scalar_ranges,
			options.vs_index,
		)?
	} else {
		commands.to_vec()
	};

	// 1. Combine successive rmoveto operations.
	for i in (1..commands.len()).rev() {
		if commands[i].op == "rmoveto" && commands[i - 1].op == "rmoveto" {
			let (v1, v2) = (&commands[i - 1].args, &commands[i].args);
			let add_at = |n: usize| match (v1.get(n), v2.get(n)) {
				(Some(a), Some(b)) => add_args(a, b),
				_ => Err(Error::ttlib("IndexError: list index out of range")),
			};
			let args = vec![add_at(0)?, add_at(1)?];
			commands[i - 1] = Command::new("rmoveto", args);
			commands.remove(i);
		}
	}

	// 2. Specialize. The curve types with a '0' need to be encoded as a h or v variant:
	// a '0' can be thought of a "don't care" and can be used as either an 'h' or a 'v'.
	// As such, a number 0 is always encoded as argument when a '0' variant is used.
	for command in &mut commands {
		match command.op {
			"rmoveto" | "rlineto" => {
				let (c, args) = categorize_vector(&command.args)?;
				*command = Command::new(
					if command.op == "rmoveto" {
						moveto_name(c)
					} else {
						lineto_name(c)
					},
					args,
				);
			}
			"rrcurveto" => {
				let args = &command.args;
				let n = args.len();
				let (c1, args1) = categorize_vector(&args[..n.min(2)])?;
				let (c2, args2) = categorize_vector(&args[n.saturating_sub(2)..])?;
				let mut new = args1;
				new.extend_from_slice(&args[n.min(2)..n.min(4)]);
				new.extend(args2);
				*command = Command::new(curveto_name(c1, c2), new);
			}
			"flex" => {
				*command = specialize_flex(std::mem::take(&mut command.args))?;
			}
			_ => {}
		}
	}

	// 3. Merge or delete redundant operations, to the extent requested. These change
	// the topology (point numbers).
	if !options.preserve_topology {
		let mut i = commands.len();
		while i > 0 {
			i -= 1;
			// A 00curveto is demoted to a (specialized) lineto.
			if commands[i].op == "00curveto" {
				if commands[i].args.len() != 4 {
					return Err(assertion("len(args) == 4"));
				}
				let (c, args) = categorize_vector(&commands[i].args[1..3])?;
				commands[i] = Command::new(lineto_name(c), args);
				// and then...
			}
			let op = commands[i].op;
			// A 0lineto can be deleted.
			if op == "0lineto" {
				commands.remove(i);
				continue;
			}
			// Merge adjacent hlineto's and vlineto's. In CFF2 charstrings from variable
			// fonts, each arg item may be a list of blendable values, one from each
			// source font.
			if i > 0 && matches!(op, "hlineto" | "vlineto") && op == commands[i - 1].op {
				let (args, other) = (&commands[i].args, &commands[i - 1].args);
				if args.len() != 1 || other.len() != 1 {
					return Err(assertion("len(args) == 1 and len(other_args) == 1"));
				}
				let new = match add_args(&args[0], &other[0]) {
					Ok(new) => new,
					Err(Error::Value(_)) => continue,
					Err(e) => return Err(e),
				};
				commands[i - 1] = Command::new(op, vec![new]);
				commands.remove(i);
			}
		}
	}

	// 4. Peephole optimization to revert back some of the h/v variants back into their
	// original "relative" operator (rline/rrcurveto) if that saves a byte.
	for i in 1..commands.len().saturating_sub(1) {
		let op = commands[i].op;
		let (prv, nxt) = (commands[i - 1].op, commands[i + 1].op);
		if matches!(op, "0lineto" | "hlineto" | "vlineto") && prv == "rlineto" && nxt == "rlineto" {
			if commands[i].args.len() != 1 {
				return Err(assertion("len(args) == 1"));
			}
			let arg = commands[i].args[0].clone();
			let args = if op.starts_with('v') {
				vec![zero(), arg]
			} else {
				vec![arg, zero()]
			};
			commands[i] = Command::new("rlineto", args);
			continue;
		}
		if is_curveto(op) && commands[i].args.len() == 5 && prv == "rrcurveto" && nxt == "rrcurveto"
		{
			let b = op.as_bytes();
			if (b[0] == b'r') == (b[1] == b'r') {
				return Err(assertion("(op[0] == \"r\") ^ (op[1] == \"r\")"));
			}
			let pos = if b[0] == b'v' {
				0
			} else if b[0] != b'r' {
				1
			} else if b[1] == b'v' {
				4
			} else {
				5
			};
			let mut args = commands[i].args.clone();
			args.insert(pos, zero());
			commands[i] = Command::new("rrcurveto", args);
		}
	}

	// 5. Combine adjacent operators when possible, minding not to go over max stack size.
	let mut stack_use = match commands.last() {
		Some(c) => args_stack_use(&c.args)?,
		None => 0,
	};
	let mut i = commands.len();
	while i > 1 {
		i -= 1;
		let (op1, op2) = (commands[i - 1].op, commands[i].op);
		let line_or_curve = |op: &str| matches!(op, "rlineto" | "rrcurveto");
		let mut new_op = None;
		// Merge logic...
		if line_or_curve(op1) && line_or_curve(op2) {
			if op1 == op2 {
				new_op = Some(op1);
			} else {
				let l = commands[i].args.len();
				if op2 == "rrcurveto" && l == 6 {
					new_op = Some("rlinecurve");
				} else if l == 2 {
					new_op = Some("rcurveline");
				}
			}
		} else if (op1 == "rlineto" && op2 == "rlinecurve")
			|| (op1 == "rrcurveto" && op2 == "rcurveline")
		{
			new_op = Some(op2);
		} else if (op1 == "vlineto" && op2 == "hlineto") || (op1 == "hlineto" && op2 == "vlineto") {
			new_op = Some(op1);
		} else if is_curveto(op1) && is_curveto(op2) {
			let (b1, b2) = (op1.as_bytes(), op2.as_bytes());
			let (d0, d1, d2, d3) = (b1[0], b1[1], b2[0], b2[1]);
			if d1 == b'r' || d2 == b'r' || (d0 == b'r' && d3 == b'r') {
				continue;
			}
			let Some(d) = merge_categories(d1, d2) else {
				continue;
			};
			if d0 == b'r' {
				let Some(d) = merge_categories(d, d3) else {
					continue;
				};
				new_op = Some(curveto_name(b'r', d));
			} else if d3 == b'r' {
				let Some(d0) = merge_categories(d0, negate_category(d)) else {
					continue;
				};
				new_op = Some(curveto_name(d0, b'r'));
			} else {
				let Some(d0) = merge_categories(d0, d3) else {
					continue;
				};
				new_op = Some(curveto_name(d0, d));
			}
		}

		// Make sure the stack depth does not exceed (maxstack - 1), so that subroutinizer
		// can insert subroutine calls at any point.
		let args1_stack_use = args_stack_use(&commands[i - 1].args)?;
		let combined_stack_use = args1_stack_use.max(commands[i - 1].args.len() as i64 + stack_use);
		match new_op.filter(|_| combined_stack_use < options.max_stack) {
			Some(new_op) => {
				let args2 = commands.remove(i).args;
				commands[i - 1].op = new_op;
				commands[i - 1].args.extend(args2);
				stack_use = combined_stack_use;
			}
			None => stack_use = args1_stack_use,
		}
	}

	// 6. Resolve any remaining made-up operators into real operators.
	for command in &mut commands {
		let op = command.op;
		if op == "0moveto" {
			command.op = "hmoveto";
			continue;
		}
		if op == "0lineto" {
			command.op = "hlineto";
			continue;
		}
		if is_curveto(op) && !matches!(&op[..2], "rr" | "hh" | "vv" | "vh" | "hv") {
			let l = command.args.len();
			let b = op.as_bytes();
			let (mut op0, mut op1) = (b[0], b[1]);
			if (op0 == b'r') != (op1 == b'r') && l % 2 != 1 {
				return Err(assertion("l % 2 == 1"));
			}
			if op0 == b'0' {
				op0 = b'h';
			}
			if op1 == b'0' {
				op1 = b'h';
			}
			if op0 == b'r' {
				op0 = op1;
			}
			if op1 == b'r' {
				op1 = negate_category(op0);
			}
			if !(matches!(op0, b'h' | b'v') && matches!(op1, b'h' | b'v')) {
				return Err(assertion("{op0, op1} <= {\"h\", \"v\"}"));
			}
			if l % 2 == 1 {
				if op0 != op1 {
					// vhcurveto / hvcurveto
					if (op0 == b'h') != (l % 8 == 1) {
						// Swap last two args order
						command.args.swap(l - 2, l - 1);
					}
				} else if op0 == b'h' {
					// hhcurveto: swap first two args order
					command.args.swap(0, 1);
				}
			}
			command.op = curveto_name(op0, op1);
		}
	}

	// 7. For any series of args which are blend lists, convert the series to a single
	// blend arg.
	for command in &mut commands {
		if command.args.iter().any(Arg::is_blend) {
			command.args = convert_to_blend_cmds(std::mem::take(&mut command.args))?;
		}
	}
	Ok(commands)
}

/// `specializeProgram`.
pub fn specialize_program(
	program: &[Token],
	get_num_regions: Option<NumRegions<'_>>,
	options: &SpecializeOptions<'_>,
) -> Result<Vec<Token>> {
	commands_to_program(&specialize_commands(
		&program_to_commands(program, get_num_regions, options.vs_index)?,
		options,
	)?)
}
