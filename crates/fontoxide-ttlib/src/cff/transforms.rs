//! Transforms of CFF fonts (`fontTools.cffLib.transforms`): desubroutinizing,
//! dropping hints and unused subroutines; and the charstring walks that
//! subsetting needs (`fontTools.subset.cff`).
//!
//! fontTools runs these with decompilers that keep their results on the
//! charstring objects. Here, the charstrings are decompiled first, as fontTools'
//! decompilers do as they go, then walked as programs, and the results are kept
//! by charstring: a glyph, a local subroutine of a Private DICT, or a global
//! subroutine.

use std::collections::{BTreeSet, HashMap};

use fontoxide_misc::array_tools::Point;
use fontoxide_misc::transform::Transform;
use fontoxide_pens::Pen;

use super::charstring::{Num, T2CharString, Token, index_error, subr_bias};
use super::dict::Value;
use super::font::{CffFontSet, PrivateDict, PrivateRef, TopDict, num_regions, run_char_string};
use crate::generated::cff_strings::STANDARD_ENCODING;
use crate::generated::ot;
use crate::{Error, Result};

/// The nesting limit of subroutine calls, as in the interpreter.
///
/// LENIENT(L15): fontTools runs into Python's recursion limit.
const MAX_DEPTH: usize = 500;

fn recursion_error() -> Error {
	Error::ttlib("RecursionError: maximum recursion depth exceeded")
}

fn list_index_error() -> Error {
	Error::ttlib("IndexError: list index out of range")
}

/// A charstring of a font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum CsId {
	/// A glyph's, by index in the CharStrings.
	Glyph(usize),
	/// A local subroutine of a Private DICT.
	Local(PrivateRef, usize),
	/// A global subroutine.
	Global(usize),
}

/// The private DICT of a charstring.
fn private_of(top: &TopDict, r: PrivateRef) -> Option<&PrivateDict> {
	match r {
		PrivateRef::Top => top.private.as_deref(),
		PrivateRef::Fd(i) => top
			.fd_array
			.as_ref()
			.and_then(|f| f.get(i))
			.and_then(|f| f.private.as_deref()),
		PrivateRef::None => None,
	}
}

fn private_of_mut(top: &mut TopDict, r: PrivateRef) -> Option<&mut PrivateDict> {
	match r {
		PrivateRef::Top => top.private.as_deref_mut(),
		PrivateRef::Fd(i) => top
			.fd_array
			.as_mut()
			.and_then(|f| f.get_mut(i))
			.and_then(|f| f.private.as_deref_mut()),
		PrivateRef::None => None,
	}
}

/// The Private DICTs of a font: the FDArray's, or the Top DICT's.
fn all_privates(top: &TopDict) -> Vec<PrivateRef> {
	match &top.fd_array {
		Some(fds) => (0..fds.len()).map(PrivateRef::Fd).collect(),
		None => vec![PrivateRef::Top],
	}
}

fn glyph_count(top: &TopDict) -> usize {
	top.char_strings.as_ref().map_or(0, |c| c.items.len())
}

fn glyph_private(top: &TopDict, gid: usize) -> Result<PrivateRef> {
	top.char_strings
		.as_ref()
		.ok_or_else(index_error)?
		.privates
		.get(gid)
		.copied()
		.ok_or_else(index_error)
}

fn num(v: &Value) -> Option<Num> {
	match v {
		Value::Int(i) => Some(Num::Int(*i)),
		Value::Float(f) => Some(Num::Float(*f)),
		_ => None,
	}
}

fn add(a: Num, b: Num) -> Num {
	match (a, b) {
		(Num::Int(x), Num::Int(y)) => Num::Int(x + y),
		_ => Num::Float(a.value() + b.value()),
	}
}

fn sub(a: Num, b: Num) -> Num {
	match (a, b) {
		(Num::Int(x), Num::Int(y)) => Num::Int(x - y),
		_ => Num::Float(a.value() - b.value()),
	}
}

/// Python `!=` of two optional numbers, where `None` only equals `None`.
fn ne(a: Option<Num>, b: Option<Num>) -> bool {
	match (a, b) {
		(Some(x), Some(y)) => x.value() != y.value(),
		(None, None) => false,
		_ => true,
	}
}

fn token(n: Num) -> Token {
	match n {
		Num::Int(i) => Token::Int(i),
		Num::Float(f) => Token::Float(f),
	}
}

/// `nominalWidthX` and `defaultWidthX` of a private DICT: `None` in CFF2.
fn widths(private: Option<&PrivateDict>) -> Result<(Option<Num>, Option<Num>)> {
	let p = private.ok_or_else(|| {
		Error::ttlib("AttributeError: 'NoneType' object has no attribute 'nominalWidthX'")
	})?;
	if p.cff2 {
		return Ok((None, None));
	}
	let get = |name: &str| p.get(name).as_ref().and_then(num);
	Ok((get("nominalWidthX"), get("defaultWidthX")))
}

/// Python list indexing, where negative indices count from the end.
fn py_index(i: i64, len: usize) -> Result<usize> {
	let at = if i < 0 { i + len as i64 } else { i };
	usize::try_from(at)
		.ok()
		.filter(|&a| a < len)
		.ok_or_else(list_index_error)
}

/// Decompiles a glyph's charstring and the subroutines it calls
/// (`T2CharString.decompile`).
fn decompile(top: &mut TopDict, global: &mut [T2CharString], gid: usize) -> Result<()> {
	run_char_string(top, global, gid, None, None)
}

/// What each charstring tells about its hints (`_DehintingT2Decompiler.Hints`).
#[derive(Debug, Clone, Default, PartialEq)]
struct Hints {
	/// Whether calling this charstring produces any hint stems.
	has_hint: bool,
	/// Index to start at to drop all hints.
	last_hint: usize,
	/// Index up to which more hints are known to be possible.
	last_checked: usize,
	/// 0: empty once hints are dropped; 1: not clear yet; 2: no more hints
	/// possible after this charstring.
	status: u8,
	/// Has hintmask instructions; not recursive.
	has_hintmask: bool,
	/// Indices of calls to empty subroutines to remove.
	deletions: Vec<usize>,
}

/// What a walk does besides running the charstrings.
enum Mode<'m> {
	/// `SimpleT2Decompiler`.
	Plain,
	/// `_ClosureGlyphsT2Decompiler`: collects the glyphs of `seac`.
	Closure(&'m mut Vec<&'static str>),
	/// `_MarkingT2Decompiler`: collects the subroutines called, by biased
	/// number. `None` for a font without local subroutines.
	Marking {
		local: Option<&'m mut BTreeSet<i64>>,
		global: &'m mut BTreeSet<i64>,
	},
	/// `_DehintingT2Decompiler`.
	Dehint {
		hints: &'m mut HashMap<CsId, Hints>,
		hinted: &'m mut BTreeSet<CsId>,
	},
}

/// The width state of `T2WidthExtractor`.
#[derive(Debug, Clone, Copy)]
struct Width {
	nominal: Option<Num>,
	default: Option<Num>,
	got: bool,
	/// `width`: `None` when it is CFF2's `defaultWidthX`.
	value: Option<Num>,
}

/// Runs decompiled charstrings (`SimpleT2Decompiler`, or `T2WidthExtractor`
/// with a width).
struct Walker<'a, 'm> {
	glyphs: &'a [T2CharString],
	local: &'a [T2CharString],
	global: &'a [T2CharString],
	private_ref: PrivateRef,
	/// The decompiler's `private`, if it has one.
	private: Option<&'a PrivateDict>,
	store: Option<&'a ot::VarStore>,
	local_bias: i64,
	global_bias: i64,
	stack: Vec<Num>,
	hint_count: usize,
	hint_mask_bytes: usize,
	num_regions: Option<i64>,
	vs_index: i64,
	width: Option<Width>,
	calling: Vec<CsId>,
	mode: Mode<'m>,
}

impl<'a, 'm> Walker<'a, 'm> {
	/// A walker of a glyph's charstring. `has_private` is whether the
	/// decompiler is given the private DICT, `width` whether it extracts the
	/// width.
	fn new(
		top: &'a TopDict,
		global: &'a [T2CharString],
		gid: usize,
		mode: Mode<'m>,
		has_private: bool,
		width: bool,
	) -> Result<Self> {
		let private_ref = glyph_private(top, gid)?;
		let private = private_of(top, private_ref);
		let local: &[T2CharString] = private.and_then(|p| p.subrs.as_deref()).unwrap_or(&[]);
		let width = if width {
			let (nominal, default) = widths(private)?;
			Some(Width {
				nominal,
				default,
				got: false,
				value: Some(Num::Int(0)),
			})
		} else {
			None
		};
		Ok(Walker {
			glyphs: top
				.char_strings
				.as_ref()
				.map_or(&[][..], |c| c.items.as_slice()),
			local,
			global,
			private_ref,
			private: if has_private { private } else { None },
			store: top.var_store.as_deref(),
			local_bias: subr_bias(local.len()),
			global_bias: subr_bias(global.len()),
			stack: Vec::new(),
			hint_count: 0,
			hint_mask_bytes: 0,
			num_regions: None,
			vs_index: 0,
			width,
			calling: Vec::new(),
			mode,
		})
	}

	fn program(&self, id: CsId) -> Result<&'a [Token]> {
		let cs = match id {
			CsId::Glyph(i) => self.glyphs.get(i),
			CsId::Local(_, j) => self.local.get(j),
			CsId::Global(j) => self.global.get(j),
		}
		.ok_or_else(list_index_error)?;
		if cs.needs_decompilation() {
			return Err(Error::ttlib("the charstring is not decompiled"));
		}
		Ok(&cs.program)
	}

	fn current(&self) -> Result<CsId> {
		self.calling.last().copied().ok_or_else(index_error)
	}

	/// The subroutine called with the biased number `raw`.
	fn subr(&self, global: bool, raw: i64) -> Result<CsId> {
		Ok(if global {
			CsId::Global(py_index(raw, self.global.len())?)
		} else {
			CsId::Local(self.private_ref, py_index(raw, self.local.len())?)
		})
	}

	fn pop(&mut self) -> Result<Num> {
		self.stack.pop().ok_or_else(index_error)
	}

	fn popall(&mut self) -> Vec<Num> {
		std::mem::take(&mut self.stack)
	}

	/// `popallWidth`.
	fn popall_width(&mut self, even_odd: usize) -> Result<Vec<Num>> {
		let mut args = self.popall();
		let Some(w) = self.width.as_mut() else {
			return Ok(args);
		};
		if !w.got {
			if (even_odd ^ (args.len() % 2)) != 0 {
				if w.default.is_none() {
					return Err(Error::Assertion(
						"CFF2 CharStrings must not have an initial width value".into(),
					));
				}
				let nominal = w.nominal.ok_or_else(|| {
					Error::ttlib("TypeError: unsupported operand type(s) for +: 'NoneType'")
				})?;
				w.value = Some(add(nominal, args.remove(0)));
			} else {
				w.value = w.default;
			}
			w.got = true;
		}
		Ok(args)
	}

	/// `countHints`.
	fn count_hints(&mut self) -> Result<()> {
		let args = self.popall_width(0)?;
		self.hint_count += args.len() / 2;
		Ok(())
	}

	fn regions(&self, vi: i64) -> Result<i64> {
		match (self.private, self.private_ref) {
			(Some(_), PrivateRef::Fd(_)) => num_regions(self.store, vi),
			// Only the Private DICTs of the FDArray have a `vstore`.
			_ => Err(Error::ttlib(
				"AttributeError: 'NoneType' object has no attribute 'getNumRegions'",
			)),
		}
	}

	fn blend(&mut self) -> Result<()> {
		if self.num_regions.is_none() {
			self.vs_index = self
				.private
				.and_then(|p| p.get("vsindex"))
				.and_then(|v| v.int().ok())
				.unwrap_or(0);
			self.num_regions = Some(self.regions(self.vs_index)?);
		}
		let regions = self.num_regions.unwrap_or(0);
		let blends = self.pop()?.int()?;
		if regions != 0 {
			// `del stack[-(numOps - numBlends):]`: `-0` deletes everything.
			let k = blends * (regions + 1) - blends;
			let n = self.stack.len() as i64;
			let start = if k == 0 { 0 } else { (n - k).max(0) };
			self.stack.truncate(start as usize);
		}
		Ok(())
	}

	/// `execute`.
	fn execute(&mut self, id: CsId) -> Result<()> {
		if !matches!(self.mode, Mode::Dehint { .. }) {
			return self.run(id);
		}
		let old = self.dehint().0.insert(id, Hints::default());
		self.run(id)?;
		let program = self.program(id)?;
		let (hints, hinted) = self.dehint();
		let h = hints.get_mut(&id).ok_or_else(index_error)?;
		if h.has_hint || h.has_hintmask {
			hinted.insert(id);
		}
		if h.status != 2 {
			// Check from last_check, make sure we didn't have any operators.
			for t in program
				.iter()
				.take(program.len().saturating_sub(1))
				.skip(h.last_checked)
			{
				if matches!(t, Token::Op(_)) {
					h.status = 2;
					break;
				}
				h.status = 1; // There's *something* here
			}
			h.last_checked = program.len();
		}
		if old.is_some_and(|old| old != *h) {
			return Err(Error::Assertion(
				"hints.__dict__ == old_hints.__dict__".into(),
			));
		}
		Ok(())
	}

	fn dehint(&mut self) -> (&mut HashMap<CsId, Hints>, &mut BTreeSet<CsId>) {
		match &mut self.mode {
			Mode::Dehint { hints, hinted } => (hints, hinted),
			_ => unreachable!("only called when dehinting"),
		}
	}

	/// `SimpleT2Decompiler.execute`.
	fn run(&mut self, id: CsId) -> Result<()> {
		if self.calling.len() >= MAX_DEPTH {
			return Err(recursion_error());
		}
		self.calling.push(id);
		let program = self.program(id)?;
		let mut index = 0;
		while index < program.len() {
			let token = &program[index];
			index += 1;
			match token {
				Token::Op(op) => {
					if let Some(next) = self.operator(op, index)? {
						index = next;
					}
				}
				Token::Int(v) => self.stack.push(Num::Int(*v)),
				Token::Float(f) => self.stack.push(Num::Float(*f)),
				Token::Mask(_) => return Err(Error::ttlib("TypeError: unexpected mask")),
			}
		}
		self.calling.pop();
		Ok(())
	}

	/// Runs an operator; returns the index after the mask of `hintmask` and
	/// `cntrmask`.
	fn operator(&mut self, op: &'static str, index: usize) -> Result<Option<usize>> {
		match op {
			"callsubr" | "callgsubr" => {
				let global = op == "callgsubr";
				let bias = if global {
					self.global_bias
				} else {
					self.local_bias
				};
				let raw = self.stack.last().copied().ok_or_else(index_error)?.int()? + bias;
				if let Mode::Marking { local, global: g } = &mut self.mode {
					if global {
						g.insert(raw);
					} else {
						local
							.as_mut()
							.ok_or_else(|| {
								Error::ttlib(
									"AttributeError: 'list' object has no attribute '_used'",
								)
							})?
							.insert(raw);
					}
				}
				let subr = self.subr(global, raw)?;
				self.pop()?;
				self.execute(subr)?;
				if matches!(self.mode, Mode::Dehint { .. }) {
					self.process_subr(index, subr)?;
				}
			}
			"hstem" | "vstem" | "hstemhm" | "vstemhm" => {
				self.count_hints()?;
				if matches!(self.mode, Mode::Dehint { .. }) {
					self.process_hint(index)?;
				}
			}
			"hintmask" | "cntrmask" => {
				if self.hint_mask_bytes == 0 {
					self.count_hints()?;
					self.hint_mask_bytes = self.hint_count.div_ceil(8);
				}
				let program = self.program(self.current()?)?;
				match program.get(index) {
					Some(Token::Mask(m)) if m.len() == self.hint_mask_bytes => {}
					Some(Token::Mask(_)) => {
						return Err(Error::Assertion("len(bytes) == nBytes".into()));
					}
					Some(_) => return Err(Error::ttlib("TypeError: expected the hint mask")),
					None => return Err(list_index_error()),
				}
				if matches!(self.mode, Mode::Dehint { .. }) {
					self.process_hintmask(index)?;
				}
				return Ok(Some(index + 1));
			}
			"endchar" if matches!(self.mode, Mode::Closure(_)) => {
				let args = self.popall();
				if args.len() >= 4 {
					// endchar can do seac accent bulding; The T2 spec says it's
					// deprecated, but recent software that shall remain nameless
					// does output it.
					let code = |n: Num| -> Result<&'static str> {
						Ok(STANDARD_ENCODING[py_index(n.int()?, STANDARD_ENCODING.len())?])
					};
					let base = code(args[args.len() - 2])?;
					let accent = code(args[args.len() - 1])?;
					if let Mode::Closure(components) = &mut self.mode {
						components.push(base);
						components.push(accent);
					}
				}
			}
			"rmoveto" | "endchar" if self.width.is_some() => {
				self.popall_width(0)?;
			}
			"hmoveto" | "vmoveto" if self.width.is_some() => {
				self.popall_width(1)?;
			}
			"return" | "ignore" | "endchar" => {}
			"blend" => self.blend()?,
			"vsindex" => {
				let vi = self.pop()?.int()?;
				self.vs_index = vi;
				self.num_regions = Some(self.regions(vi)?);
			}
			"and" | "or" | "not" | "store" | "abs" | "add" | "sub" | "div" | "load" | "neg"
			| "eq" | "drop" | "put" | "get" | "ifelse" | "random" | "mul" | "sqrt" | "dup"
			| "exch" | "index" | "roll" => {
				return Err(Error::ttlib(format!("NotImplementedError: {op}")));
			}
			_ => {
				self.popall();
			}
		}
		Ok(None)
	}

	/// The hints of the charstring being run, and its program.
	fn current_hints(&mut self) -> Result<(&mut Hints, &'a [Token])> {
		let id = self.current()?;
		let program = self.program(id)?;
		let h = self.dehint().0.get_mut(&id).ok_or_else(index_error)?;
		Ok((h, program))
	}

	/// `processHintmask`.
	fn process_hintmask(&mut self, index: usize) -> Result<()> {
		let (h, program) = self.current_hints()?;
		h.has_hintmask = true;
		if h.status != 2 {
			// Check from last_check, see if we may be an implicit vstem
			let end = index.saturating_sub(1).min(program.len());
			let start = h.last_checked.min(end);
			if program[start..end]
				.iter()
				.any(|t| matches!(t, Token::Op(_)))
			{
				h.status = 2;
			} else {
				// We are an implicit vstem
				h.has_hint = true;
				h.last_hint = index + 1;
				h.status = 0;
			}
		}
		h.last_checked = index + 1;
		Ok(())
	}

	/// `processHint`.
	fn process_hint(&mut self, index: usize) -> Result<()> {
		let (h, _) = self.current_hints()?;
		h.has_hint = true;
		h.last_hint = index;
		h.last_checked = index;
		Ok(())
	}

	/// `processSubr`.
	fn process_subr(&mut self, index: usize, subr: CsId) -> Result<()> {
		let subr_hints = self
			.dehint()
			.0
			.get(&subr)
			.cloned()
			.ok_or_else(index_error)?;
		let (h, program) = self.current_hints()?;
		// Check from last_check, make sure we didn't have any operators.
		if h.status != 2 {
			let end = index.saturating_sub(1).min(program.len());
			let start = h.last_checked.min(end);
			if program[start..end]
				.iter()
				.any(|t| matches!(t, Token::Op(_)))
			{
				h.status = 2;
			}
			h.last_checked = index;
		}
		if h.status != 2 {
			if subr_hints.has_hint {
				h.has_hint = true;
				// Decide where to chop off from
				h.last_hint = if subr_hints.status == 0 {
					index
				} else {
					index - 2 // Leave the subr call in
				};
			}
		} else if subr_hints.status == 0 {
			h.deletions.push(index);
		}
		h.status = h.status.max(subr_hints.status);
		Ok(())
	}
}

/// A pen that draws nothing (`NullPen`).
struct NullPen;

impl Pen for NullPen {
	fn move_to(&mut self, _: Point) -> fontoxide_pens::Result<()> {
		Ok(())
	}
	fn line_to(&mut self, _: Point) -> fontoxide_pens::Result<()> {
		Ok(())
	}
	fn curve_to(&mut self, _: &[Point]) -> fontoxide_pens::Result<()> {
		Ok(())
	}
	fn q_curve_to(&mut self, _: &[Point], _: Option<Point>) -> fontoxide_pens::Result<()> {
		Ok(())
	}
	fn close_path(&mut self) -> fontoxide_pens::Result<()> {
		Ok(())
	}
	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		Ok(())
	}
	fn add_component(&mut self, _: &str, _: Transform) -> fontoxide_pens::Result<()> {
		Ok(())
	}
}

/// The program of a charstring, to change it.
fn program_mut<'t>(
	top: &'t mut TopDict,
	global: &'t mut [T2CharString],
	id: CsId,
) -> Result<&'t mut Vec<Token>> {
	let cs = match id {
		CsId::Glyph(i) => top.char_strings.as_mut().and_then(|c| c.items.get_mut(i)),
		CsId::Local(r, j) => private_of_mut(top, r)
			.and_then(|p| p.subrs.as_mut())
			.and_then(|s| s.get_mut(j)),
		CsId::Global(j) => global.get_mut(j),
	}
	.ok_or_else(list_index_error)?;
	cs.bytecode = None;
	Ok(&mut cs.program)
}

/// The subroutines of a font, and their charstrings, for desubroutinizing.
struct Subrs<'a> {
	glyphs: &'a [T2CharString],
	local: &'a [T2CharString],
	private_ref: PrivateRef,
	global: &'a [T2CharString],
	/// Whether the glyph's Private DICT is a CFF2 one (`private.in_cff2`).
	cff2: bool,
}

impl Subrs<'_> {
	fn program(&self, id: CsId) -> Result<&[Token]> {
		let cs = match id {
			CsId::Glyph(i) => self.glyphs.get(i),
			CsId::Local(_, j) => self.local.get(j),
			CsId::Global(j) => self.global.get(j),
		}
		.ok_or_else(list_index_error)?;
		if cs.needs_decompilation() {
			return Err(Error::ttlib("the charstring is not decompiled"));
		}
		Ok(&cs.program)
	}

	/// The program of a charstring with the subroutines it calls inlined
	/// (`_DesubroutinizingT2Decompiler`). The calls always follow the
	/// subroutine number, else fontTools fails, so this goes by the program.
	///
	/// As in fontTools, a global subroutine is expanded once, with the local
	/// subroutines of the first glyph that calls it.
	fn desubroutinized(
		&self,
		id: CsId,
		memo: &mut HashMap<CsId, Vec<Token>>,
		depth: usize,
	) -> Result<Vec<Token>> {
		if let Some(p) = memo.get(&id) {
			return Ok(p.clone());
		}
		if depth > MAX_DEPTH {
			return Err(recursion_error());
		}
		let program = self.program(id)?;
		let mut patches = Vec::new();
		for (i, t) in program.iter().enumerate() {
			let global = match t {
				Token::Op("callsubr") => false,
				Token::Op("callgsubr") => true,
				_ => continue,
			};
			let n = match i.checked_sub(1).map(|j| &program[j]) {
				Some(Token::Int(n)) => *n,
				Some(Token::Float(_)) => {
					return Err(Error::ttlib(
						"TypeError: list indices must be integers or slices, not float",
					));
				}
				_ => {
					return Err(Error::Assertion(
						"type(desubroutinized[idx - 2]) == int".into(),
					));
				}
			};
			let subr = if global {
				let bias = subr_bias(self.global.len());
				CsId::Global(py_index(n + bias, self.global.len())?)
			} else {
				let bias = subr_bias(self.local.len());
				CsId::Local(self.private_ref, py_index(n + bias, self.local.len())?)
			};
			patches.push((i + 1, self.desubroutinized(subr, memo, depth + 1)?));
		}
		let mut out = program.to_vec();
		for (idx, mut expansion) in patches.into_iter().rev() {
			match expansion.last() {
				Some(Token::Op("return")) => {
					expansion.pop();
				}
				Some(_) => {}
				None => return Err(list_index_error()),
			}
			out.splice(idx - 2..idx, expansion);
		}
		if !self.cff2
			&& let Some(end) = out.iter().position(|t| *t == Token::Op("endchar"))
		{
			// Cut off after first endchar
			out.truncate(end + 1);
		}
		memo.insert(id, out.clone());
		Ok(out)
	}
}

/// Renumbering of subroutines (`_used`, `_old_bias`, `_new_bias`).
#[derive(Debug)]
struct Renumber {
	used: Vec<i64>,
	old_bias: i64,
	new_bias: i64,
}

/// The local subroutines that calls are renumbered with.
#[derive(Debug, Clone, Copy)]
enum LocalRenumber<'r> {
	/// No local subroutines (`None`).
	None,
	/// Local subroutines without calls, so not renumbered.
	Unmarked,
	Some(&'r Renumber),
}

/// How the calls to the local subroutines of a Private DICT are renumbered.
fn local_renumber<'r>(
	locals: &'r HashMap<PrivateRef, Renumber>,
	top: &TopDict,
	r: PrivateRef,
) -> LocalRenumber<'r> {
	match locals.get(&r) {
		Some(l) => LocalRenumber::Some(l),
		None if private_of(top, r).is_some_and(|p| p.subrs.is_some()) => LocalRenumber::Unmarked,
		None => LocalRenumber::None,
	}
}

/// `_cs_subset_subroutines`.
fn subset_calls(program: &mut [Token], local: LocalRenumber<'_>, global: &Renumber) -> Result<()> {
	for i in 1..program.len() {
		let r = match program[i] {
			Token::Op("callsubr") => match local {
				LocalRenumber::Some(r) => r,
				LocalRenumber::None => {
					return Err(Error::ttlib(
						"AttributeError: 'NoneType' object has no attribute '_used'",
					));
				}
				LocalRenumber::Unmarked => {
					return Err(Error::ttlib(
						"AttributeError: 'SubrsIndex' object has no attribute '_used'",
					));
				}
			},
			Token::Op("callgsubr") => global,
			_ => continue,
		};
		let Token::Int(n) = program[i - 1] else {
			return Err(Error::Assertion("isinstance(p[i - 1], int)".into()));
		};
		let at = r
			.used
			.iter()
			.position(|&u| u == n + r.old_bias)
			.ok_or_else(|| Error::value(format!("{} is not in list", n + r.old_bias)))?;
		program[i - 1] = Token::Int(at as i64 - r.new_bias);
	}
	Ok(())
}

/// The hint values that `remove_hints` drops from the Private DICTs.
const HINT_VALUES: [&str; 14] = [
	"BlueValues",
	"OtherBlues",
	"FamilyBlues",
	"FamilyOtherBlues",
	"BlueScale",
	"BlueShift",
	"BlueFuzz",
	"StemSnapH",
	"StemSnapV",
	"StdHW",
	"StdVW",
	"ForceBold",
	"LanguageGroup",
	"ExpansionFactor",
];

/// `_cs_drop_hints`. `width` is the glyph's width and its Private DICT's
/// widths, for a glyph's charstring.
fn drop_hints(
	program: &mut Vec<Token>,
	hints: &Hints,
	width: Option<(Option<Num>, Option<Num>, Option<Num>)>,
) -> Result<()> {
	for &idx in hints.deletions.iter().rev() {
		let end = idx.min(program.len());
		program.drain(idx.saturating_sub(2).min(end)..end);
	}
	if hints.has_hint {
		if hints
			.deletions
			.first()
			.is_some_and(|&d| hints.last_hint > d)
		{
			return Err(Error::Assertion(
				"not hints.deletions or hints.last_hint <= hints.deletions[0]".into(),
			));
		}
		program.drain(..hints.last_hint.min(program.len()));
		if program.is_empty() {
			program.push(Token::Op("endchar"));
		}
		if let Some((width, nominal, default)) = width
			&& ne(width, default)
		{
			// Insert width back if needed
			if default.is_none() {
				return Err(Error::Assertion(
					"CFF2 CharStrings must not have an initial width value".into(),
				));
			}
			let (Some(width), Some(nominal)) = (width, nominal) else {
				return Err(Error::ttlib(
					"TypeError: unsupported operand type(s) for -: 'NoneType'",
				));
			};
			program.insert(0, token(sub(width, nominal)));
		}
	}
	if hints.has_hintmask {
		let mut i = 0;
		while i < program.len() {
			if matches!(program[i], Token::Op("hintmask" | "cntrmask")) {
				program.drain(i..(i + 2).min(program.len()));
				continue;
			}
			i += 1;
		}
	}
	if program.is_empty() {
		return Err(Error::Assertion("len(charstring.program)".into()));
	}
	Ok(())
}

impl CffFontSet {
	/// The glyphs that the `seac` of a glyph of the first font uses
	/// (`_ClosureGlyphsT2Decompiler`), decompiling its charstring.
	pub fn seac_components(&mut self, gid: usize) -> Result<Vec<&'static str>> {
		let top = self.top_dicts.first_mut().ok_or_else(index_error)?;
		decompile(top, &mut self.global_subrs, gid)?;
		let mut components = Vec::new();
		Walker::new(
			top,
			&self.global_subrs,
			gid,
			Mode::Closure(&mut components),
			false,
			false,
		)?
		.execute(CsId::Glyph(gid))?;
		Ok(components)
	}

	/// Empties the charstring of a glyph of font `font`, keeping its width
	/// unless `ignore_width` (`fontTools.subset.cff._empty_charstring`).
	pub fn empty_char_string(&mut self, font: usize, gid: usize, ignore_width: bool) -> Result<()> {
		let cff2 = self.major > 1;
		let top = self.top_dicts.get_mut(font).ok_or_else(index_error)?;
		let cs = top.char_strings.as_ref().ok_or_else(index_error)?;
		if gid >= cs.items.len() {
			return Err(list_index_error());
		}
		let selector = cs.selector(gid)?;
		let program = if cff2 || ignore_width {
			// CFF2 charstrings have no widths nor 'endchar' operators
			if cff2 {
				Vec::new()
			} else {
				vec![Token::Op("endchar")]
			}
		} else {
			let private = match &top.fd_array {
				Some(fds) => {
					let i = selector.ok_or_else(|| {
						Error::ttlib(
							"TypeError: list indices must be integers or slices, not NoneType",
						)
					})?;
					let i = py_index(i, fds.len())?;
					fds[i].private.as_deref()
				}
				None => top.private.as_deref(),
			};
			let (nominal, default) = widths(private)?;
			// Drawing sets the charstring's width.
			run_char_string(top, &mut self.global_subrs, gid, Some(&mut NullPen), None)?;
			let mut walker = Walker::new(top, &self.global_subrs, gid, Mode::Plain, true, true)?;
			walker.execute(CsId::Glyph(gid))?;
			let width = walker.width.and_then(|w| w.value);
			if ne(width, default) {
				let (Some(width), Some(nominal)) = (width, nominal) else {
					return Err(Error::ttlib(
						"TypeError: unsupported operand type(s) for -: 'NoneType'",
					));
				};
				vec![token(sub(width, nominal)), Token::Op("endchar")]
			} else {
				vec![Token::Op("endchar")]
			}
		};
		*program_mut(top, &mut self.global_subrs, CsId::Glyph(gid))? = program;
		Ok(())
	}

	/// Inlines all subroutines, then drops them (`desubroutinize`).
	pub fn desubroutinize(&mut self) -> Result<()> {
		for top in &mut self.top_dicts {
			let mut memo = HashMap::new();
			for gid in 0..glyph_count(top) {
				decompile(top, &mut self.global_subrs, gid)?;
				let private_ref = glyph_private(top, gid)?;
				let private = private_of(top, private_ref);
				let subrs = Subrs {
					glyphs: top
						.char_strings
						.as_ref()
						.map_or(&[][..], |c| c.items.as_slice()),
					local: private.and_then(|p| p.subrs.as_deref()).unwrap_or(&[]),
					private_ref,
					global: &self.global_subrs,
					cff2: private
						.ok_or_else(|| {
							Error::ttlib(
								"AttributeError: 'NoneType' object has no attribute 'in_cff2'",
							)
						})?
						.cff2,
				};
				let program = subrs.desubroutinized(CsId::Glyph(gid), &mut memo, 0)?;
				*program_mut(top, &mut self.global_subrs, CsId::Glyph(gid))? = program;
			}
			// Delete all the local subrs
			for r in all_privates(top) {
				if let Some(p) = private_of_mut(top, r) {
					p.subrs = None;
					p.raw.retain(|(k, _)| *k != "Subrs");
				}
			}
		}
		// as well as the global subrs
		self.global_subrs.clear();
		Ok(())
	}

	/// Drops the hints of the charstrings and the hint values of the Private
	/// DICTs, then the unused subroutines (`remove_hints`).
	pub fn remove_hints(&mut self) -> Result<()> {
		for top in &mut self.top_dicts {
			let mut hints = HashMap::new();
			let mut hinted = BTreeSet::new();
			let mut glyph_widths = HashMap::new();
			for gid in 0..glyph_count(top) {
				decompile(top, &mut self.global_subrs, gid)?;
				let mut walker = Walker::new(
					top,
					&self.global_subrs,
					gid,
					Mode::Dehint {
						hints: &mut hints,
						hinted: &mut hinted,
					},
					true,
					true,
				)?;
				walker.execute(CsId::Glyph(gid))?;
				glyph_widths.insert(gid, walker.width.and_then(|w| w.value));
			}
			for id in hinted {
				let h = hints.get(&id).ok_or_else(index_error)?;
				let width = match id {
					CsId::Glyph(gid) => {
						let private = private_of(top, glyph_private(top, gid)?);
						let (nominal, default) = widths(private)?;
						Some((glyph_widths.get(&gid).copied().flatten(), nominal, default))
					}
					_ => None,
				};
				drop_hints(program_mut(top, &mut self.global_subrs, id)?, h, width)?;
			}
			// Drop font-wide hinting values
			for r in all_privates(top) {
				let p = private_of_mut(top, r).ok_or_else(|| {
					Error::ttlib("AttributeError: 'NoneType' object has no attribute 'Private'")
				})?;
				for k in HINT_VALUES {
					if p.get(k).is_some() {
						p.values.remove(k);
						p.cleared.insert(k);
					}
				}
			}
		}
		self.remove_unused_subroutines()
	}

	/// Drops the subroutines that no glyph calls, and renumbers the calls
	/// (`remove_unused_subroutines`).
	pub fn remove_unused_subroutines(&mut self) -> Result<()> {
		for top in &mut self.top_dicts {
			// Mark all used subroutines
			let mut global_used = BTreeSet::new();
			let mut local_used: HashMap<PrivateRef, BTreeSet<i64>> = HashMap::new();
			for gid in 0..glyph_count(top) {
				decompile(top, &mut self.global_subrs, gid)?;
				let private_ref = glyph_private(top, gid)?;
				let has_local = private_of(top, private_ref)
					.and_then(|p| p.subrs.as_ref())
					.is_some_and(|s| !s.is_empty());
				let local = has_local.then(|| local_used.entry(private_ref).or_default());
				Walker::new(
					top,
					&self.global_subrs,
					gid,
					Mode::Marking {
						local,
						global: &mut global_used,
					},
					true,
					false,
				)?
				.execute(CsId::Glyph(gid))?;
			}

			// Prepare
			let renumber = |used: Option<&BTreeSet<i64>>, count: usize| {
				let used: Vec<i64> = used
					.map(|u| u.iter().copied().collect())
					.unwrap_or_default();
				Renumber {
					old_bias: subr_bias(count),
					new_bias: subr_bias(used.len()),
					used,
				}
			};
			let global = renumber(Some(&global_used), self.global_subrs.len());
			let mut locals: HashMap<PrivateRef, Renumber> = HashMap::new();
			let mut all_locals = Vec::new();
			for r in all_privates(top) {
				if let Some(subrs) = private_of(top, r).and_then(|p| p.subrs.as_ref())
					&& !subrs.is_empty()
				{
					locals.insert(r, renumber(local_used.get(&r), subrs.len()));
					all_locals.push(r);
				}
			}

			// Renumber glyph charstrings
			for gid in 0..glyph_count(top) {
				let local = local_renumber(&locals, top, glyph_private(top, gid)?);
				let program = program_mut(top, &mut self.global_subrs, CsId::Glyph(gid))?;
				subset_calls(program, local, &global)?;
			}

			// Renumber subroutines themselves
			let global_local = match &top.fd_array {
				None if top.private.as_ref().is_some_and(|p| p.subrs.is_some()) => {
					local_renumber(&locals, top, PrivateRef::Top)
				}
				// Technically we shouldn't do this. But I've run into fonts that do it.
				Some(fds)
					if fds.len() == 1
						&& fds[0].private.as_ref().is_some_and(|p| p.subrs.is_some()) =>
				{
					local_renumber(&locals, top, PrivateRef::Fd(0))
				}
				_ => LocalRenumber::None,
			};
			let subset = |items: &[T2CharString], r: &Renumber| -> Result<Vec<T2CharString>> {
				r.used
					.iter()
					.map(|&i| Ok(items[py_index(i, items.len())?].clone()))
					.collect()
			};
			let mut new_global = subset(&self.global_subrs, &global)?;
			for subr in &mut new_global {
				subset_calls(&mut subr.program, global_local, &global)?;
			}
			let mut new_locals = Vec::new();
			for &r in &all_locals {
				let l = &locals[&r];
				let items = private_of(top, r)
					.and_then(|p| p.subrs.as_deref())
					.ok_or_else(index_error)?;
				let mut items = subset(items, l)?;
				for subr in &mut items {
					subset_calls(&mut subr.program, LocalRenumber::Some(l), &global)?;
				}
				new_locals.push((r, items));
			}
			self.global_subrs = new_global;
			for (r, items) in new_locals {
				if let Some(p) = private_of_mut(top, r) {
					p.subrs = Some(items);
				}
			}

			// Delete local SubrsIndex if empty
			for r in all_privates(top) {
				if let Some(p) = private_of_mut(top, r)
					&& p.subrs.as_ref().is_some_and(|s| s.is_empty())
				{
					p.raw.retain(|(k, _)| *k != "Subrs");
					p.subrs = None;
				}
			}
		}
		Ok(())
	}
}
