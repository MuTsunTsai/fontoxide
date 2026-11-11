//! Port of `fontTools.misc.psLib`: a tokenizer and a minimal PostScript
//! interpreter, just about enough to parse Type 1 fonts ([`suckfont`]).

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use thiserror::Error;

use crate::eexec;
use crate::ps_operators::{DictRef, Key, Kind, ObjRef, PsObject, PsString};

/// Errors of the tokenizer and the interpreter.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PsError {
	/// `PSTokenError`.
	#[error("{0}")]
	Token(String),
	/// `PSError`.
	#[error("{0}")]
	Ps(String),
	/// The other Python exceptions the interpreter raises on bad input
	/// (`RuntimeError`, `TypeError`, `KeyError`, `IndexError`, ...), with the
	/// exception name.
	#[error("{0}")]
	Python(String),
}

type Result<T> = std::result::Result<T, PsError>;

fn py_err(msg: impl Into<String>) -> PsError {
	PsError::Python(msg.into())
}

/// `string.whitespace`.
fn is_white(b: u8) -> bool {
	matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// `ps_special`; `/` is handled separately.
const PS_SPECIAL: &[u8] = b"()<>[]{}%";

/// The end of a name or number: `endofthingRE` stops at these.
fn ends_thing(b: u8) -> bool {
	is_white(b) || b"[](){}<>/%".contains(&b)
}

/// `bytes.decode(encoding)` for the codecs psLib may be given.
fn decode(encoding: &str, data: &[u8]) -> Result<String> {
	let name = fontoxide_encodings::codecs::normalize(encoding);
	if matches!(name.as_str(), "utf_8" | "utf8" | "u8" | "utf") {
		return String::from_utf8(data.to_vec()).map_err(|e| {
			py_err(format!(
				"UnicodeDecodeError: 'utf-8' codec can't decode byte at position {}",
				e.utf8_error().valid_up_to()
			))
		});
	}
	fontoxide_encodings::codecs::decode(encoding, data)
		.map_err(|e| py_err(format!("UnicodeDecodeError: {e}")))
}

/// Matches `stringRE` at `pos` (which holds `(`): a string literal with one
/// level of balanced embedded parentheses, where a backslash escapes the byte
/// after it. Returns the end of the match.
fn match_string(buf: &[u8], pos: usize) -> Option<usize> {
	let mut i = pos + 1;
	loop {
		match *buf.get(i)? {
			b')' => return Some(i + 1),
			b'\\' => {
				buf.get(i + 1)?;
				i += 2;
			}
			b'(' => {
				i += 1;
				loop {
					match *buf.get(i)? {
						b')' => {
							i += 1;
							break;
						}
						b'\\' => {
							buf.get(i + 1)?;
							i += 2;
						}
						// A second level of nesting is not supported: the
						// whole match fails.
						b'(' => return None,
						_ => i += 1,
					}
				}
			}
			_ => i += 1,
		}
	}
}

/// Matches `hexstringRE` at `pos` (which holds `<`).
fn match_hexstring(buf: &[u8], pos: usize) -> Option<usize> {
	let mut i = pos + 1;
	while let Some(&b) = buf.get(i) {
		if b == b'>' {
			return Some(i + 1);
		}
		if !(is_white(b) || b.is_ascii_hexdigit()) {
			return None;
		}
		i += 1;
	}
	None
}

/// The tokenizer (`PSTokenizer`): reads PostScript tokens from a buffer, which
/// can switch to the decrypted `eexec` part and back.
#[derive(Debug, Clone)]
pub struct PsTokenizer {
	buf: Vec<u8>,
	len: usize,
	pos: usize,
	closed: bool,
	encoding: String,
	dirtybuf: Option<Vec<u8>>,
	id: u64,
}

thread_local! {
	static NEXT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

fn next_id() -> u64 {
	NEXT_ID.with(|c| {
		let id = c.get();
		c.set(id + 1);
		id
	})
}

impl PsTokenizer {
	/// A tokenizer over `buf`, whose tokens are decoded with `encoding`
	/// (`PSTokenizer(buf, encoding)`).
	pub fn new(buf: &[u8], encoding: &str) -> Self {
		PsTokenizer {
			buf: buf.to_vec(),
			len: buf.len(),
			pos: 0,
			closed: false,
			encoding: encoding.to_owned(),
			dirtybuf: None,
			id: next_id(),
		}
	}

	fn check_open(&self) -> Result<()> {
		if self.closed {
			return Err(py_err(
				"AttributeError: 'PSTokenizer' object has no attribute 'buf'",
			));
		}
		Ok(())
	}

	/// The position in the buffer.
	pub fn pos(&self) -> usize {
		self.pos
	}

	/// Reads at most `n` bytes, or all the rest when `n` is `None` (`read`).
	pub fn read(&mut self, n: Option<usize>) -> Result<Vec<u8>> {
		if self.closed {
			return Err(py_err("ValueError: I/O operation on closed file"));
		}
		let newpos = match n {
			None => self.len,
			Some(n) => (self.pos + n).min(self.len),
		};
		let start = self.pos.min(self.buf.len());
		let r = self.buf[start..newpos.clamp(start, self.buf.len())].to_vec();
		self.pos = newpos;
		Ok(r)
	}

	/// `close`.
	pub fn close(&mut self) {
		if !self.closed {
			self.closed = true;
			self.buf = Vec::new();
			self.dirtybuf = None;
		}
	}

	/// The next token and its type (`getnexttoken`): `"do_special"`,
	/// `"do_comment"`, `"do_string"`, `"do_hexstring"`, `"do_literal"`, or `""`
	/// for names and numbers; `None` at the end.
	pub fn get_next_token(&mut self) -> Result<Option<(&'static str, String)>> {
		self.skipwhite()?;
		if self.pos >= self.len {
			return Ok(None);
		}
		let pos = self.pos;
		let buf = &self.buf;
		let ch = buf[pos];
		let (tokentype, end) = if PS_SPECIAL.contains(&ch) {
			match ch {
				b'{' | b'}' | b'[' | b']' => ("do_special", pos + 1),
				b'%' => {
					let mut end = pos + 1;
					while end < buf.len() && !matches!(buf[end], b'\n' | b'\r') {
						end += 1;
					}
					("do_comment", end)
				}
				b'(' => match match_string(buf, pos) {
					Some(end) => ("do_string", end),
					None => {
						return Err(PsError::Token(format!("bad string at character {pos}")));
					}
				},
				b'<' => match match_hexstring(buf, pos) {
					Some(end) => ("do_hexstring", end),
					None => {
						return Err(PsError::Token(format!("bad hexstring at character {pos}")));
					}
				},
				_ => return Err(PsError::Token(format!("bad token at character {pos}"))),
			}
		} else {
			let (tokentype, start) = if ch == b'/' {
				("do_literal", pos + 1)
			} else {
				("", pos)
			};
			let mut end = start;
			while end < buf.len() && !ends_thing(buf[end]) {
				end += 1;
			}
			(tokentype, end)
		};
		let token = decode(&self.encoding, &buf[pos..end])?;
		self.pos = end;
		Ok(Some((tokentype, token)))
	}

	/// Skips whitespace (`skipwhite`).
	pub fn skipwhite(&mut self) -> Result<()> {
		self.check_open()?;
		while self.pos < self.buf.len() && is_white(self.buf[self.pos]) {
			self.pos += 1;
		}
		Ok(())
	}

	/// Switches to the decrypted `eexec` part, after the byte that follows
	/// `eexec` (`starteexec`).
	pub fn starteexec(&mut self) -> Result<()> {
		self.check_open()?;
		self.pos += 1;
		let dirty = self.buf.get(self.pos..).unwrap_or_default().to_vec();
		let (buf, _) = eexec::decrypt(&dirty, 55665);
		self.dirtybuf = Some(dirty);
		self.len = buf.len();
		self.buf = buf;
		self.pos = 4;
		Ok(())
	}

	/// Switches back to the encrypted data, at the same position
	/// (`stopeexec`).
	pub fn stopeexec(&mut self) -> Result<()> {
		self.check_open()?;
		if let Some(dirty) = self.dirtybuf.take() {
			self.buf = dirty;
		}
		Ok(())
	}
}

/// A Python number of the interpreter's arithmetic.
#[derive(Debug, Clone, Copy)]
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

	fn gt(self, o: Num) -> bool {
		match (self, o) {
			(Num::Int(a), Num::Int(b)) => a > b,
			_ => self.f() > o.f(),
		}
	}

	fn lt(self, o: Num) -> bool {
		match (self, o) {
			(Num::Int(a), Num::Int(b)) => a < b,
			_ => self.f() < o.f(),
		}
	}

	fn add(self, o: Num) -> Result<Num> {
		Ok(match (self, o) {
			(Num::Int(a), Num::Int(b)) => Num::Int(
				a.checked_add(b)
					.ok_or_else(|| py_err("OverflowError: integer overflow"))?,
			),
			_ => Num::Float(self.f() + o.f()),
		})
	}

	fn is_positive(self) -> bool {
		match self {
			Num::Int(i) => i > 0,
			Num::Float(f) => f > 0.0,
		}
	}
}

/// Normalizes a Python index into a list of `len` elements.
fn py_index(len: usize, i: i64) -> Option<usize> {
	let i = if i < 0 { i + len as i64 } else { i };
	usize::try_from(i).ok().filter(|&i| i < len)
}

/// The range of `seq[start:stop]`.
fn py_slice(len: usize, start: i64, stop: i64) -> (usize, usize) {
	let clamp = |i: i64| -> usize {
		let i = if i < 0 { i + len as i64 } else { i };
		i.clamp(0, len as i64) as usize
	};
	let (a, b) = (clamp(start), clamp(stop));
	(a, b.max(a))
}

fn index_error() -> PsError {
	py_err("IndexError: list index out of range")
}

/// The unpacked value of a PostScript object (`unpack_item`): the Python
/// values fontTools' T1 font dictionaries hold.
#[derive(Debug, Clone, PartialEq)]
pub enum PsValue {
	/// `None`: the value of operators and `null`.
	None,
	/// A `bool`, from the comparison operators.
	Bool(bool),
	/// An `int`; `true` and `false` unpack to 1 and 0.
	Int(i64),
	/// A `float`.
	Float(f64),
	/// A `str`: names, literals, text strings.
	Str(String),
	/// A `bytes`: strings read by `readstring`, e.g. encrypted charstrings.
	Bytes(Vec<u8>),
	/// A `list`: arrays.
	List(Vec<PsValue>),
	/// A `tuple`: procedures.
	Tuple(Vec<PsValue>),
	/// A `dict`: dictionaries and fonts.
	Dict(IndexMap<Key, PsValue>),
	/// A file object (the tokenizer).
	File,
}

const MAX_DEPTH: usize = 900;

/// The interpreter (`PSInterpreter`), with the operators of `PSOperators`.
#[derive(Debug)]
pub struct PsInterpreter {
	encoding: String,
	/// The dictionary stack (`dictstack`): `systemdict`, `userdict`, then the
	/// dictionaries of `begin`.
	pub dictstack: Vec<DictRef>,
	/// The operand stack (`stack`).
	pub stack: Vec<ObjRef>,
	proclevel: usize,
	procmark: ObjRef,
	mark: ObjRef,
	tokenizer: Option<PsTokenizer>,
	depth: usize,
}

/// The operators, in the order `dir()` lists their methods (`suckoperators`).
const OPERATORS: [&str; 48] = [
	"anchorsearch",
	"array",
	"astore",
	"begin",
	"bind",
	"cleartomark",
	"closefile",
	"count",
	"currentdict",
	"currentfile",
	"cvn",
	"cvx",
	"def",
	"definefont",
	"dict",
	"dup",
	"eexec",
	"end",
	"eq",
	"exch",
	"exec",
	"executeonly",
	"findfont",
	"for",
	"forall",
	"get",
	"getinterval",
	"if",
	"ifelse",
	"index",
	"known",
	"load",
	"matrix",
	"ne",
	"noaccess",
	"not",
	"pop",
	"print",
	"put",
	"putinterval",
	"readonly",
	"readstring",
	"store",
	"string",
	"systemdict",
	"type",
	"userdict",
	"where",
];

fn new_dict() -> DictRef {
	Rc::new(RefCell::new(IndexMap::new()))
}

fn obj(kind: Kind) -> ObjRef {
	PsObject::new(kind)
}

fn boolean(value: bool, py_bool: bool) -> ObjRef {
	obj(Kind::Boolean { value, py_bool })
}

impl Default for PsInterpreter {
	fn default() -> Self {
		Self::new("ascii")
	}
}

impl PsInterpreter {
	/// A new interpreter whose tokens are decoded with `encoding`
	/// (`PSInterpreter(encoding)`).
	pub fn new(encoding: &str) -> Self {
		let mut interp = PsInterpreter {
			encoding: encoding.to_owned(),
			dictstack: vec![new_dict(), new_dict()],
			stack: Vec::new(),
			proclevel: 0,
			procmark: obj(Kind::ProcMark),
			mark: obj(Kind::Mark),
			tokenizer: None,
			depth: 0,
		};
		interp.fillsystemdict();
		interp
	}

	fn fillsystemdict(&mut self) {
		let mut d = self.dictstack[0].borrow_mut();
		d.insert("[".into(), self.mark.clone());
		d.insert("mark".into(), self.mark.clone());
		d.insert("]".into(), obj(Kind::Operator("]")));
		d.insert("true".into(), boolean(true, false));
		d.insert("false".into(), boolean(false, false));
		let encoding = fontoxide_encodings::standard_encoding::STANDARD_ENCODING
			.iter()
			.map(|n| Some(obj(Kind::Name((*n).to_owned()))))
			.collect();
		d.insert("StandardEncoding".into(), obj(Kind::Array(encoding)));
		d.insert("FontDirectory".into(), obj(Kind::Dict(new_dict())));
		for name in OPERATORS {
			d.insert((*name).into(), obj(Kind::Operator(name)));
		}
	}

	/// Interprets PostScript code (`interpret`).
	pub fn interpret(&mut self, data: &[u8]) -> Result<()> {
		self.tokenizer = Some(PsTokenizer::new(data, &self.encoding));
		let result = self.interpret_loop();
		match result {
			Ok(()) => {
				if let Some(t) = self.tokenizer.as_mut() {
					t.close();
				}
				self.tokenizer = None;
				Ok(())
			}
			Err(e) => {
				if let Some(t) = &self.tokenizer {
					let pos = t.pos.min(t.buf.len());
					log::debug!(
						"ps error:\n- - - - - - -\n{}\n>>>\n{}\n- - - - - - -",
						crate::py::bytes_repr(&t.buf[pos.saturating_sub(50)..pos]),
						crate::py::bytes_repr(&t.buf[pos..(pos + 50).min(t.buf.len())])
					);
				}
				Err(e)
			}
		}
	}

	fn tokenizer_mut(&mut self) -> Result<&mut PsTokenizer> {
		self.tokenizer
			.as_mut()
			.ok_or_else(|| py_err("AttributeError: 'NoneType' object has no attribute"))
	}

	fn interpret_loop(&mut self) -> Result<()> {
		while let Some((tokentype, token)) = self.tokenizer_mut()?.get_next_token()? {
			let object = match tokentype {
				"" => Some(self.do_token(&token)),
				"do_comment" => None,
				"do_literal" => Some(obj(Kind::Literal(token[1..].to_owned()))),
				"do_string" => {
					let mut chars = token.chars();
					chars.next();
					chars.next_back();
					Some(obj(Kind::String(PsString::Text(chars.as_str().to_owned()))))
				}
				"do_hexstring" => Some(self.do_hexstring(&token)?),
				_ => Some(self.do_special(&token)?),
			};
			if let Some(object) = object {
				self.handle_object(object)?;
			}
		}
		Ok(())
	}

	/// `handle_object`: pushes literal objects and objects inside procedures,
	/// runs the others.
	fn handle_object(&mut self, object: ObjRef) -> Result<()> {
		let (literal, is_proc, is_op) = {
			let o = object.borrow();
			(
				o.literal,
				matches!(o.kind, Kind::Procedure(_)),
				matches!(o.kind, Kind::Operator(_)),
			)
		};
		if !(self.proclevel > 0 || literal || is_proc) {
			let object = if is_op {
				object
			} else {
				let key = key_of(&object)?;
				self.resolve_name(&key)?
			};
			let (literal, kind) = {
				let o = object.borrow();
				(o.literal, o.kind.clone())
			};
			if literal {
				self.push(object);
			} else {
				match kind {
					Kind::Procedure(_) => self.call_procedure(&object)?,
					Kind::Operator(name) => self.call_operator(name)?,
					_ => {
						return Err(py_err(format!(
							"AttributeError: '{}' object has no attribute 'function'",
							object.borrow().type_name()
						)));
					}
				}
			}
		} else {
			self.push(object);
		}
		Ok(())
	}

	fn call_procedure(&mut self, proc: &ObjRef) -> Result<()> {
		self.depth += 1;
		if self.depth > MAX_DEPTH {
			self.depth -= 1;
			return Err(py_err("RecursionError: maximum recursion depth exceeded"));
		}
		let mut i = 0;
		let result = loop {
			let item = match &proc.borrow().kind {
				Kind::Procedure(items) => match items.get(i) {
					Some(item) => item.clone(),
					None => break Ok(()),
				},
				_ => break Err(py_err("TypeError: not a procedure")),
			};
			i += 1;
			if let Err(e) = self.handle_object(item) {
				break Err(e);
			}
		};
		self.depth -= 1;
		result
	}

	/// `resolve_name`: looks a name up in the dictionary stack, from the top.
	pub fn resolve_name(&self, name: &Key) -> Result<ObjRef> {
		for d in self.dictstack.iter().rev() {
			if let Some(o) = d.borrow().get(name) {
				return Ok(o.clone());
			}
		}
		Err(PsError::Ps(format!("name error: {name}")))
	}

	/// `do_token`: a number, or an executable name.
	fn do_token(&self, token: &str) -> ObjRef {
		if let Some(i) = crate::py::parse_int(token) {
			// LENIENT(L45): integers beyond 64 bits (Python's are unbounded) become
			// names.
			if let Ok(i) = i64::try_from(i) {
				return obj(Kind::Integer(i));
			}
			return obj(Kind::Name(token.to_owned()));
		}
		if let Some(f) = crate::py::parse_float(token) {
			return obj(Kind::Real(f));
		}
		if let Some(hashpos) = token.find('#') {
			let num = crate::py::parse_int(&token[..hashpos]).and_then(|base| {
				let digits = &token[hashpos + 1..];
				match base {
					0 => crate::text_tools::parse_int_literal(digits),
					2..=36 => crate::text_tools::parse_int_radix(digits, base as u32),
					_ => None,
				}
			});
			if let Some(i) = num.and_then(|n| i64::try_from(n).ok()) {
				return obj(Kind::Integer(i));
			}
		}
		obj(Kind::Name(token.to_owned()))
	}

	fn do_hexstring(&self, token: &str) -> Result<ObjRef> {
		let mut hex: String = token[1..token.len() - 1]
			.chars()
			.filter(|c| !crate::py::is_space(*c))
			.collect();
		if hex.len() % 2 == 1 {
			hex.push('0');
		}
		let mut s = String::new();
		for pair in hex.as_bytes().chunks(2) {
			let v = std::str::from_utf8(pair)
				.ok()
				.and_then(|p| u8::from_str_radix(p, 16).ok())
				.ok_or_else(|| py_err("ValueError: invalid literal for int() with base 16"))?;
			s.push(v as char);
		}
		Ok(obj(Kind::String(PsString::Text(s))))
	}

	fn do_special(&mut self, token: &str) -> Result<ObjRef> {
		match token {
			"{" => {
				self.proclevel += 1;
				Ok(self.procmark.clone())
			}
			"}" => {
				let mut proc = Vec::new();
				loop {
					let top = self.pop(&[])?;
					if Rc::ptr_eq(&top, &self.procmark) {
						break;
					}
					proc.push(top);
				}
				self.proclevel = self.proclevel.saturating_sub(1);
				proc.reverse();
				Ok(obj(Kind::Procedure(proc)))
			}
			"[" => Ok(self.mark.clone()),
			"]" => Ok(obj(Kind::Name("]".into()))),
			_ => Err(PsError::Token("huh?".into())),
		}
	}

	/// Pushes an object (`push`).
	pub fn push(&mut self, object: ObjRef) {
		self.stack.push(object);
	}

	/// Pops an object, checking its type if `types` is not empty (`pop`).
	pub fn pop(&mut self, types: &[&str]) -> Result<ObjRef> {
		let Some(top) = self.stack.last() else {
			return Err(PsError::Ps("stack underflow".into()));
		};
		if !types.is_empty() {
			let t = top.borrow().type_name();
			if !types.contains(&t) {
				let repr = if types.len() == 1 {
					format!("('{}',)", types[0])
				} else {
					format!(
						"({})",
						types
							.iter()
							.map(|t| format!("'{t}'"))
							.collect::<Vec<_>>()
							.join(", ")
					)
				};
				return Err(PsError::Ps(format!(
					"typecheck, expected {repr}, found {t}"
				)));
			}
		}
		Ok(self.stack.pop().unwrap_or_else(|| obj(Kind::Null)))
	}

	fn do_makearray(&mut self) -> Result<()> {
		let mut array = Vec::new();
		loop {
			let top = self.pop(&[])?;
			if Rc::ptr_eq(&top, &self.mark) {
				break;
			}
			array.push(Some(top));
		}
		array.reverse();
		self.push(obj(Kind::Array(array)));
		Ok(())
	}

	/// Breaks the reference cycles the dictionaries may hold (`close`).
	pub fn close(&mut self) {
		self.stack.clear();
		for d in self.dictstack.drain(..) {
			d.borrow_mut().clear();
		}
	}

	fn int_value(o: &ObjRef) -> Result<i64> {
		match o.borrow().kind {
			Kind::Integer(i) => Ok(i),
			Kind::Boolean { value, .. } => Ok(value as i64),
			_ => Err(py_err("TypeError: list indices must be integers or slices")),
		}
	}

	fn num_value(o: &ObjRef) -> Result<Num> {
		match o.borrow().kind {
			Kind::Integer(i) => Ok(Num::Int(i)),
			Kind::Real(f) => Ok(Num::Float(f)),
			_ => Err(py_err("TypeError: not a number")),
		}
	}

	fn truthy(o: &ObjRef) -> bool {
		match &o.borrow().kind {
			Kind::Boolean { value, .. } => *value,
			Kind::Integer(i) => *i != 0,
			Kind::Real(f) => *f != 0.0,
			_ => true,
		}
	}

	fn font_directory(&self) -> Result<DictRef> {
		let fd = self.dictstack[0]
			.borrow()
			.get(&Key::from("FontDirectory"))
			.cloned()
			.ok_or_else(|| py_err("KeyError: 'FontDirectory'"))?;
		let d = match &fd.borrow().kind {
			Kind::Dict(d) | Kind::Font(d) => d.clone(),
			_ => return Err(py_err("TypeError: FontDirectory is not a dict")),
		};
		Ok(d)
	}

	fn file_check(&mut self, f: &ObjRef) -> Result<&mut PsTokenizer> {
		let id = match f.borrow().kind {
			Kind::File(id) => id,
			_ => return Err(py_err("TypeError: not a file")),
		};
		let t = self.tokenizer_mut()?;
		if t.id != id {
			// LENIENT(L45): a file object of an earlier `interpret` call; fontTools
			// would operate on that closed tokenizer and fail.
			return Err(py_err("ValueError: I/O operation on closed file"));
		}
		Ok(t)
	}

	fn proc_bind(&self, proc: &ObjRef) {
		let n = proc_len(proc);
		for i in 0..n {
			let item = match &proc.borrow().kind {
				Kind::Procedure(items) => match items.get(i) {
					Some(item) => item.clone(),
					None => return,
				},
				_ => return,
			};
			let (is_proc, literal) = {
				let o = item.borrow();
				(matches!(o.kind, Kind::Procedure(_)), o.literal)
			};
			if is_proc {
				self.proc_bind(&item);
			} else if !literal
				&& let Ok(key) = key_of(&item)
				&& let Ok(resolved) = self.resolve_name(&key)
				&& matches!(resolved.borrow().kind, Kind::Operator(_))
				&& let Kind::Procedure(items) = &mut proc.borrow_mut().kind
			{
				items[i] = resolved;
			}
		}
	}

	/// Runs an operator (`ps_*` methods, and `]`).
	fn call_operator(&mut self, name: &str) -> Result<()> {
		match name {
			"]" => self.do_makearray()?,
			"def" => {
				let o = self.pop(&[])?;
				let name = self.pop(&[])?;
				let key = key_of(&name)?;
				let top = self.dictstack.last().cloned().ok_or_else(index_error)?;
				top.borrow_mut().insert(key, o);
			}
			"bind" => {
				let proc = self.pop(&["proceduretype"])?;
				self.proc_bind(&proc);
				self.push(proc);
			}
			"exch" => {
				if self.stack.len() < 2 {
					return Err(py_err("RuntimeError: stack underflow"));
				}
				let a = self.pop(&[])?;
				let b = self.pop(&[])?;
				self.push(a);
				self.push(b);
			}
			"dup" => {
				let top = self
					.stack
					.last()
					.cloned()
					.ok_or_else(|| py_err("RuntimeError: stack underflow"))?;
				self.push(top);
			}
			"exec" => {
				let o = self.pop(&[])?;
				if matches!(o.borrow().kind, Kind::Procedure(_)) {
					self.call_procedure(&o)?;
				} else {
					self.handle_object(o)?;
				}
			}
			"count" => {
				let n = self.stack.len() as i64;
				self.push(obj(Kind::Integer(n)));
			}
			"eq" | "ne" => {
				let a = self.pop(&[])?;
				let b = self.pop(&[])?;
				let eq = py_eq(&a, &b);
				self.push(boolean(if name == "eq" { eq } else { !eq }, true));
			}
			"cvx" => {
				let o = self.pop(&[])?;
				o.borrow_mut().literal = false;
				self.push(o);
			}
			"matrix" => {
				let m = vec![
					Some(obj(Kind::Real(1.0))),
					Some(obj(Kind::Integer(0))),
					Some(obj(Kind::Integer(0))),
					Some(obj(Kind::Real(1.0))),
					Some(obj(Kind::Integer(0))),
					Some(obj(Kind::Integer(0))),
				];
				self.push(obj(Kind::Array(m)));
			}
			"string" => {
				let num = Self::int_value(&self.pop(&["integertype"])?)?;
				let s = "\0".repeat(num.max(0) as usize);
				self.push(obj(Kind::String(PsString::Text(s))));
			}
			"type" => {
				let o = self.pop(&[])?;
				let t = o.borrow().type_name();
				self.push(obj(Kind::String(PsString::Text(t.to_owned()))));
			}
			"store" => {
				let value = self.pop(&[])?;
				let key = self.pop(&[])?;
				let name = key_of(&key)?;
				for d in self.dictstack.iter().rev() {
					let mut d = d.borrow_mut();
					if d.contains_key(&name) {
						d.insert(name.clone(), value.clone());
						break;
					}
				}
				// UPSTREAM-BUG(B62): fontTools also stores the value in the current
				// dictionary, whether or not it found the name elsewhere.
				let top = self.dictstack.last().cloned().ok_or_else(index_error)?;
				top.borrow_mut().insert(name, value);
			}
			"where" => {
				self.pop(&[])?;
				self.push(boolean(false, false));
			}
			"systemdict" | "userdict" | "currentdict" => {
				let d = match name {
					"systemdict" => self.dictstack[0].clone(),
					"userdict" => self.dictstack.get(1).cloned().ok_or_else(index_error)?,
					_ => self.dictstack.last().cloned().ok_or_else(index_error)?,
				};
				self.push(obj(Kind::Dict(d)));
			}
			"currentfile" => {
				let id = self.tokenizer_mut()?.id;
				self.push(obj(Kind::File(id)));
			}
			"eexec" => {
				let f = self.pop(&["filetype"])?;
				self.file_check(&f)?.starteexec()?;
			}
			"closefile" => {
				let f = self.pop(&["filetype"])?;
				let t = self.file_check(&f)?;
				t.skipwhite()?;
				t.stopeexec()?;
			}
			"cleartomark" => {
				let mut o = self.pop(&[])?;
				while !Rc::ptr_eq(&o, &self.mark) {
					o = self.pop(&[])?;
				}
			}
			"readstring" => {
				let s = self.pop(&["stringtype"])?;
				let old_len = match &s.borrow().kind {
					Kind::String(v) => v.len(),
					_ => 0,
				};
				let f = self.pop(&["filetype"])?;
				let t = self.file_check(&f)?;
				t.pos += 1;
				let new = t.read(Some(old_len))?;
				let same = new.len() == old_len;
				s.borrow_mut().kind = Kind::String(PsString::Bytes(new));
				self.push(s);
				self.push(boolean(same, true));
			}
			"known" => {
				let key = self.pop(&[])?;
				let d = self.pop(&["dicttype", "fonttype"])?;
				let key = key_of(&key)?;
				let known = match &d.borrow().kind {
					Kind::Dict(d) | Kind::Font(d) => d.borrow().contains_key(&key),
					_ => false,
				};
				self.push(boolean(known, true));
			}
			"if" => {
				let proc = self.pop(&["proceduretype"])?;
				if Self::truthy(&self.pop(&["booleantype"])?) {
					self.call_procedure(&proc)?;
				}
			}
			"ifelse" => {
				let proc2 = self.pop(&["proceduretype"])?;
				let proc1 = self.pop(&["proceduretype"])?;
				if Self::truthy(&self.pop(&["booleantype"])?) {
					self.call_procedure(&proc1)?;
				} else {
					self.call_procedure(&proc2)?;
				}
			}
			"readonly" | "executeonly" | "noaccess" => {
				let level = match name {
					"readonly" => 1,
					"executeonly" => 2,
					_ => 3,
				};
				let o = self.pop(&[])?;
				{
					let mut b = o.borrow_mut();
					if b.access < level {
						b.access = level;
					}
				}
				self.push(o);
			}
			"not" => {
				let o = self.pop(&["booleantype", "integertype"])?;
				let new = match o.borrow().kind {
					Kind::Boolean { value, .. } => boolean(!value, true),
					Kind::Integer(i) => obj(Kind::Integer(!i)),
					_ => return Err(py_err("TypeError: bad operand type for not")),
				};
				self.push(new);
			}
			"print" => {
				let s = self.pop(&["stringtype"])?;
				if let Kind::String(v) = &s.borrow().kind {
					match v {
						PsString::Text(t) => println!("PS output ---> {t}"),
						PsString::Bytes(b) => {
							println!("PS output ---> {}", crate::py::bytes_repr(b));
						}
					}
				}
			}
			"anchorsearch" => {
				let seek = self.pop(&["stringtype"])?;
				let s = self.pop(&["stringtype"])?;
				let (sv, seekv) = (string_value(&s), string_value(&seek));
				let found = match (&sv, &seekv) {
					(PsString::Text(a), PsString::Text(b)) => {
						let n = b.chars().count();
						let head: String = a.chars().take(n).collect();
						(head == *b).then(|| PsString::Text(a.chars().skip(n).collect()))
					}
					(PsString::Bytes(a), PsString::Bytes(b)) => {
						let n = b.len().min(a.len());
						(a[..n] == b[..]).then(|| PsString::Bytes(a[n..].to_vec()))
					}
					_ => None,
				};
				match found {
					Some(rest) => {
						self.push(obj(Kind::String(rest)));
						self.push(seek);
						self.push(boolean(true, false));
					}
					None => {
						self.push(s);
						self.push(boolean(false, false));
					}
				}
			}
			"array" => {
				let n = Self::int_value(&self.pop(&["integertype"])?)?;
				self.push(obj(Kind::Array(vec![None; n.max(0) as usize])));
			}
			"astore" => {
				let array = self.pop(&["arraytype"])?;
				let n = match &array.borrow().kind {
					Kind::Array(v) => v.len(),
					_ => 0,
				};
				for i in (0..n).rev() {
					let o = self.pop(&[])?;
					if let Kind::Array(v) = &mut array.borrow_mut().kind {
						v[i] = Some(o);
					}
				}
				self.push(array);
			}
			"load" => {
				let n = self.pop(&[])?;
				let key = key_of(&n)?;
				let o = self.resolve_name(&key)?;
				self.push(o);
			}
			"put" => self.ps_put()?,
			"get" => self.ps_get()?,
			"getinterval" => {
				let count = Self::int_value(&self.pop(&["integertype"])?)?;
				let start = Self::int_value(&self.pop(&["integertype"])?)?;
				let src = self.pop(&["arraytype", "stringtype"])?;
				let new = match &src.borrow().kind {
					Kind::Array(v) => {
						let (a, b) = py_slice(v.len(), start, start.saturating_add(count));
						Kind::Array(v[a..b].to_vec())
					}
					Kind::String(s) => {
						Kind::String(slice_string(s, start, start.saturating_add(count)))
					}
					_ => return Err(py_err("TypeError")),
				};
				self.push(obj(new));
			}
			"putinterval" => {
				let src = self.pop(&["arraytype", "stringtype"])?;
				let start = Self::int_value(&self.pop(&["integertype"])?)?;
				let dst = self.pop(&["arraytype", "stringtype"])?;
				let src_kind = src.borrow().kind.clone();
				let mut d = dst.borrow_mut();
				match (&mut d.kind, src_kind) {
					(Kind::Array(v), Kind::Array(s)) => {
						let (a, b) = py_slice(v.len(), start, start.saturating_add(s.len() as i64));
						v.splice(a..b, s);
					}
					(Kind::String(PsString::Text(v)), Kind::String(PsString::Text(s))) => {
						let chars: Vec<char> = v.chars().collect();
						let n = s.chars().count() as i64;
						let (a, _) = py_slice(chars.len(), 0, start);
						let (b, _) = py_slice(chars.len(), start.saturating_add(n), i64::MAX);
						let mut new: String = chars[..a].iter().collect();
						new.push_str(&s);
						new.extend(&chars[b..]);
						*v = new;
					}
					(Kind::String(PsString::Bytes(v)), Kind::String(PsString::Bytes(s))) => {
						let n = s.len() as i64;
						let (a, _) = py_slice(v.len(), 0, start);
						let (b, _) = py_slice(v.len(), start.saturating_add(n), i64::MAX);
						let mut new = v[..a].to_vec();
						new.extend_from_slice(&s);
						new.extend_from_slice(&v[b..]);
						*v = new;
					}
					// LENIENT(L45): fontTools mixes the types (e.g. characters into an
					// array) or fails with a TypeError.
					_ => return Err(py_err("TypeError: putinterval of different types")),
				}
			}
			"cvn" => {
				let s = self.pop(&["stringtype"])?;
				let v = string_value(&s);
				match v {
					PsString::Text(t) => self.push(obj(Kind::Name(t))),
					// LENIENT(L45): fontTools makes a name with a bytes value.
					PsString::Bytes(_) => return Err(py_err("TypeError: cvn of bytes")),
				}
			}
			"index" => {
				let n = Self::int_value(&self.pop(&["integertype"])?)?;
				if n < 0 {
					return Err(py_err("RuntimeError: index may not be negative"));
				}
				let len = self.stack.len() as i64;
				let at = py_index(self.stack.len(), -1 - n).filter(|_| -1 - n >= -len);
				let o = self
					.stack
					.get(at.ok_or_else(index_error)?)
					.cloned()
					.ok_or_else(index_error)?;
				self.push(o);
			}
			"for" => {
				let proc = self.pop(&["proceduretype"])?;
				let limit = Self::num_value(&self.pop(&["integertype", "realtype"])?)?;
				let increment = Self::num_value(&self.pop(&["integertype", "realtype"])?)?;
				let mut i = Self::num_value(&self.pop(&["integertype", "realtype"])?)?;
				loop {
					if increment.is_positive() {
						if i.gt(limit) {
							break;
						}
					} else if i.lt(limit) {
						break;
					}
					self.push(match i {
						Num::Int(v) => obj(Kind::Integer(v)),
						Num::Float(f) => obj(Kind::Real(f)),
					});
					self.call_procedure(&proc)?;
					i = i.add(increment)?;
				}
			}
			"forall" => self.ps_forall()?,
			"definefont" => {
				let font = self.pop(&["dicttype"])?;
				let name = self.pop(&[])?;
				let d = match &font.borrow().kind {
					Kind::Dict(d) => d.clone(),
					_ => return Err(py_err("TypeError")),
				};
				let font = obj(Kind::Font(d));
				let key = key_of(&name)?;
				self.font_directory()?
					.borrow_mut()
					.insert(key, font.clone());
				self.push(font);
			}
			"findfont" => {
				let name = self.pop(&[])?;
				let key = key_of(&name)?;
				let font = self
					.font_directory()?
					.borrow()
					.get(&key)
					.cloned()
					.ok_or_else(|| py_err(format!("KeyError: {key}")))?;
				self.push(font);
			}
			"pop" => {
				self.pop(&[])?;
			}
			"dict" => {
				self.pop(&["integertype"])?;
				self.push(obj(Kind::Dict(new_dict())));
			}
			"begin" => {
				let d = self.pop(&["dicttype"])?;
				let d = match &d.borrow().kind {
					Kind::Dict(d) => d.clone(),
					_ => return Err(py_err("TypeError")),
				};
				self.dictstack.push(d);
			}
			"end" => {
				if self.dictstack.len() > 2 {
					self.dictstack.pop();
				} else {
					return Err(py_err("RuntimeError: dictstack underflow"));
				}
			}
			_ => return Err(py_err(format!("AttributeError: no operator {name}"))),
		}
		Ok(())
	}

	fn ps_put(&mut self) -> Result<()> {
		let value = self.pop(&[])?;
		let key = self.pop(&[])?;
		let target = self.pop(&["arraytype", "dicttype", "stringtype", "proceduretype"])?;
		let mut t = target.borrow_mut();
		match &mut t.kind {
			Kind::Array(v) => {
				let i = Self::int_value(&key)?;
				let at = py_index(v.len(), i)
					.ok_or_else(|| py_err("IndexError: list assignment index out of range"))?;
				v[at] = Some(value);
			}
			Kind::Procedure(v) => {
				let i = Self::int_value(&key)?;
				let at = py_index(v.len(), i)
					.ok_or_else(|| py_err("IndexError: list assignment index out of range"))?;
				v[at] = value;
			}
			Kind::Dict(d) => {
				let key = key_of(&key)?;
				d.borrow_mut().insert(key, value);
			}
			Kind::String(PsString::Text(s)) => {
				let index = Self::int_value(&key)?;
				let c = match value.borrow().kind {
					Kind::Integer(c) => u32::try_from(c)
						.ok()
						.and_then(char::from_u32)
						.ok_or_else(|| py_err("ValueError: chr() arg not in range"))?,
					_ => return Err(py_err("TypeError: an integer is required")),
				};
				let chars: Vec<char> = s.chars().collect();
				let (a, _) = py_slice(chars.len(), 0, index);
				let (b, _) = py_slice(chars.len(), index.saturating_add(1), i64::MAX);
				let mut new: String = chars[..a].iter().collect();
				new.push(c);
				new.extend(&chars[b..]);
				*s = new;
			}
			_ => return Err(py_err("TypeError: can't concat str to bytes")),
		}
		Ok(())
	}

	fn ps_get(&mut self) -> Result<()> {
		let key = self.pop(&[])?;
		let src = self.pop(&[
			"arraytype",
			"dicttype",
			"stringtype",
			"proceduretype",
			"fonttype",
		])?;
		let result = match &src.borrow().kind {
			Kind::Array(v) => {
				let at = py_index(v.len(), Self::int_value(&key)?).ok_or_else(index_error)?;
				// LENIENT(L45): fontTools pushes `None` and fails when it is used.
				v[at]
					.clone()
					.ok_or_else(|| py_err("AttributeError: 'NoneType' object"))?
			}
			Kind::Procedure(v) => {
				let at = py_index(v.len(), Self::int_value(&key)?).ok_or_else(index_error)?;
				v[at].clone()
			}
			Kind::Dict(d) | Kind::Font(d) => {
				let k = key_of(&key)?;
				d.borrow()
					.get(&k)
					.cloned()
					.ok_or_else(|| py_err(format!("KeyError: {k}")))?
			}
			Kind::String(PsString::Text(s)) => {
				let chars: Vec<char> = s.chars().collect();
				let at = py_index(chars.len(), Self::int_value(&key)?)
					.ok_or_else(|| py_err("IndexError: string index out of range"))?;
				obj(Kind::Integer(chars[at] as i64))
			}
			_ => {
				return Err(py_err(
					"TypeError: ord() expected string of length 1, but int found",
				));
			}
		};
		self.push(result);
		Ok(())
	}

	fn ps_forall(&mut self) -> Result<()> {
		let proc = self.pop(&["proceduretype"])?;
		let src = self.pop(&["arraytype", "stringtype", "dicttype"])?;
		let kind = src.borrow().kind.clone();
		match kind {
			Kind::Array(_) => {
				let mut i = 0;
				loop {
					let item = match &src.borrow().kind {
						Kind::Array(v) => match v.get(i) {
							Some(item) => item.clone(),
							None => break,
						},
						_ => break,
					};
					i += 1;
					// LENIENT(L45): fontTools pushes `None` and fails when it is used.
					let item = item.ok_or_else(|| py_err("AttributeError: 'NoneType' object"))?;
					self.push(item);
					self.call_procedure(&proc)?;
				}
			}
			Kind::String(PsString::Text(s)) => {
				for c in s.chars() {
					self.push(obj(Kind::Integer(c as i64)));
					self.call_procedure(&proc)?;
				}
			}
			Kind::String(PsString::Bytes(b)) => {
				if !b.is_empty() {
					return Err(py_err(
						"TypeError: ord() expected string of length 1, but int found",
					));
				}
			}
			Kind::Dict(d) => {
				let items: Vec<(Key, ObjRef)> = d
					.borrow()
					.iter()
					.map(|(k, v)| (k.clone(), v.clone()))
					.collect();
				let len = items.len();
				for (k, v) in items {
					let name = match k {
						Key::Str(s) => s,
						other => other.to_string(),
					};
					self.push(obj(Kind::Name(name)));
					self.push(v);
					self.call_procedure(&proc)?;
					if d.borrow().len() != len {
						return Err(py_err(
							"RuntimeError: dictionary changed size during iteration",
						));
					}
				}
			}
			_ => {}
		}
		Ok(())
	}
}

fn proc_len(proc: &ObjRef) -> usize {
	match &proc.borrow().kind {
		Kind::Procedure(v) => v.len(),
		Kind::Array(v) => v.len(),
		_ => 0,
	}
}

fn string_value(o: &ObjRef) -> PsString {
	match &o.borrow().kind {
		Kind::String(s) => s.clone(),
		_ => PsString::Text(String::new()),
	}
}

fn slice_string(s: &PsString, start: i64, stop: i64) -> PsString {
	match s {
		PsString::Text(t) => {
			let chars: Vec<char> = t.chars().collect();
			let (a, b) = py_slice(chars.len(), start, stop);
			PsString::Text(chars[a..b].iter().collect())
		}
		PsString::Bytes(v) => {
			let (a, b) = py_slice(v.len(), start, stop);
			PsString::Bytes(v[a..b].to_vec())
		}
	}
}

/// The value of an object as a dictionary key (`hash(obj.value)`).
pub fn key_of(o: &ObjRef) -> Result<Key> {
	Ok(match &o.borrow().kind {
		Kind::Operator(_) | Kind::Null => Key::None,
		Kind::Name(s) | Kind::Literal(s) => Key::Str(s.clone()),
		Kind::Mark => Key::from("mark"),
		Kind::ProcMark => Key::from("procmark"),
		Kind::String(PsString::Text(s)) => Key::Str(s.clone()),
		Kind::String(PsString::Bytes(b)) => Key::Bytes(b.clone()),
		Kind::Integer(i) => Key::Int(*i),
		Kind::Boolean { value, .. } => Key::Int(*value as i64),
		Kind::Real(f) => {
			if f.fract() == 0.0 && f.abs() < 9.2e18 {
				Key::Int(*f as i64)
			} else {
				Key::Float(f.to_bits())
			}
		}
		Kind::File(id) => Key::File(*id),
		Kind::Procedure(_) | Kind::Array(_) => {
			return Err(py_err("TypeError: unhashable type: 'list'"));
		}
		Kind::Dict(_) | Kind::Font(_) => {
			return Err(py_err("TypeError: unhashable type: 'dict'"));
		}
	})
}

/// `a.value == b.value`, with Python's equality.
fn py_eq(a: &ObjRef, b: &ObjRef) -> bool {
	if Rc::ptr_eq(a, b) {
		return true;
	}
	let (a, b) = (a.borrow(), b.borrow());
	let num = |k: &Kind| -> Option<f64> {
		match k {
			Kind::Integer(i) => Some(*i as f64),
			Kind::Real(f) => Some(*f),
			Kind::Boolean { value, .. } => Some(*value as i64 as f64),
			_ => None,
		}
	};
	match (&a.kind, &b.kind) {
		(Kind::Integer(x), Kind::Integer(y)) => return x == y,
		(Kind::Boolean { value: x, .. }, Kind::Integer(y))
		| (Kind::Integer(y), Kind::Boolean { value: x, .. }) => return *x as i64 == *y,
		_ => {}
	}
	if let (Some(x), Some(y)) = (num(&a.kind), num(&b.kind)) {
		return x == y;
	}
	let text = |k: &Kind| -> Option<String> {
		match k {
			Kind::Name(s) | Kind::Literal(s) | Kind::String(PsString::Text(s)) => Some(s.clone()),
			Kind::Mark => Some("mark".into()),
			Kind::ProcMark => Some("procmark".into()),
			_ => None,
		}
	};
	if let (Some(x), Some(y)) = (text(&a.kind), text(&b.kind)) {
		return x == y;
	}
	match (&a.kind, &b.kind) {
		(Kind::String(PsString::Bytes(x)), Kind::String(PsString::Bytes(y))) => x == y,
		(Kind::Operator(_) | Kind::Null, Kind::Operator(_) | Kind::Null) => true,
		(Kind::File(x), Kind::File(y)) => x == y,
		(Kind::Procedure(x), Kind::Procedure(y)) => {
			x.len() == y.len() && x.iter().zip(y).all(|(p, q)| Rc::ptr_eq(p, q))
		}
		(Kind::Array(x), Kind::Array(y)) => {
			x.len() == y.len()
				&& x.iter().zip(y).all(|(p, q)| match (p, q) {
					(Some(p), Some(q)) => Rc::ptr_eq(p, q),
					(None, None) => true,
					_ => false,
				})
		}
		(Kind::Procedure(x), Kind::Array(y)) | (Kind::Array(y), Kind::Procedure(x)) => {
			x.len() == y.len()
				&& x.iter()
					.zip(y)
					.all(|(p, q)| q.as_ref().is_some_and(|q| Rc::ptr_eq(p, q)))
		}
		(Kind::Dict(x) | Kind::Font(x), Kind::Dict(y) | Kind::Font(y)) => {
			if Rc::ptr_eq(x, y) {
				return true;
			}
			let (x, y) = (x.borrow(), y.borrow());
			x.len() == y.len()
				&& x.iter()
					.all(|(k, v)| y.get(k).is_some_and(|w| Rc::ptr_eq(v, w)))
		}
		_ => false,
	}
}

/// `unpack_item`: the Python value of an object, with its dictionaries, arrays
/// and procedures unpacked recursively.
pub fn unpack_item(item: &ObjRef) -> Result<PsValue> {
	unpack(item, 0)
}

fn unpack(item: &ObjRef, depth: usize) -> Result<PsValue> {
	if depth > MAX_DEPTH {
		return Err(py_err("RecursionError: maximum recursion depth exceeded"));
	}
	let o = item.borrow();
	Ok(match &o.kind {
		Kind::Dict(d) | Kind::Font(d) => {
			let d = d.clone();
			drop(o);
			let mut out = IndexMap::new();
			for (k, v) in d.borrow().iter() {
				out.insert(k.clone(), unpack(v, depth + 1)?);
			}
			PsValue::Dict(out)
		}
		Kind::Array(v) => {
			let v = v.clone();
			drop(o);
			let mut out = Vec::with_capacity(v.len());
			for item in &v {
				let item = item.as_ref().ok_or_else(|| {
					py_err("AttributeError: 'NoneType' object has no attribute 'value'")
				})?;
				out.push(unpack(item, depth + 1)?);
			}
			PsValue::List(out)
		}
		Kind::Procedure(v) => {
			let v = v.clone();
			drop(o);
			PsValue::Tuple(
				v.iter()
					.map(|i| unpack(i, depth + 1))
					.collect::<Result<_>>()?,
			)
		}
		Kind::Operator(_) | Kind::Null => PsValue::None,
		Kind::Name(s) | Kind::Literal(s) => PsValue::Str(s.clone()),
		Kind::Mark => PsValue::Str("mark".into()),
		Kind::ProcMark => PsValue::Str("procmark".into()),
		Kind::File(_) => PsValue::File,
		Kind::Boolean { value, py_bool } => {
			if *py_bool {
				PsValue::Bool(*value)
			} else {
				PsValue::Int(*value as i64)
			}
		}
		Kind::String(PsString::Text(s)) => PsValue::Str(s.clone()),
		Kind::String(PsString::Bytes(b)) => PsValue::Bytes(b.clone()),
		Kind::Integer(i) => PsValue::Int(*i),
		Kind::Real(f) => PsValue::Float(*f),
	})
}

/// Finds `/FontName\s+/([^ \t\n\r]+)\s+def` and returns the name.
fn find_font_name(data: &[u8]) -> Option<&[u8]> {
	let is_space = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C);
	let pat = b"/FontName";
	let mut from = 0;
	while let Some(found) = data[from..].windows(pat.len()).position(|w| w == pat) {
		let start = from + found;
		from = start + 1;
		let mut i = start + pat.len();
		let ws = i;
		while i < data.len() && is_space(data[i]) {
			i += 1;
		}
		if i == ws || data.get(i) != Some(&b'/') {
			continue;
		}
		let g = i + 1;
		let mut e = g;
		while e < data.len() && !matches!(data[e], b' ' | b'\t' | b'\n' | b'\r') {
			e += 1;
		}
		// Backtrack the greedy group until `\s+def` follows.
		let mut p = e;
		while p > g {
			let mut q = p;
			while q < data.len() && is_space(data[q]) {
				q += 1;
			}
			if q > p && data[q..].starts_with(b"def") {
				return Some(&data[g..p]);
			}
			p -= 1;
		}
	}
	None
}

/// Interprets a Type 1 font program and returns its font dictionary
/// (`suckfont`).
pub fn suckfont(data: &[u8], encoding: &str) -> Result<IndexMap<Key, PsValue>> {
	let font_name = match find_font_name(data) {
		Some(name) => Some(
			String::from_utf8(name.to_vec())
				.map_err(|_| py_err("UnicodeDecodeError: 'utf-8' codec can't decode"))?,
		),
		None => None,
	};
	let mut interpreter = PsInterpreter::new(encoding);
	interpreter
		.interpret(b"/Helvetica 4 dict dup /Encoding StandardEncoding put definefont pop")?;
	interpreter.interpret(data)?;
	let fontdir = interpreter.font_directory()?;
	let rawfont = {
		let fd = fontdir.borrow();
		match font_name.and_then(|n| fd.get(&Key::Str(n)).cloned()) {
			Some(f) => f,
			None => {
				// Fall back, in case the font name wasn't found.
				let mut names: Vec<Key> = fd.keys().cloned().collect();
				if names.len() > 1 {
					let at = names
						.iter()
						.position(|k| *k == Key::from("Helvetica"))
						.ok_or_else(|| py_err("ValueError: list.remove(x): x not in list"))?;
					names.remove(at);
				}
				if names.iter().any(|k| !matches!(k, Key::Str(_))) && names.len() > 1 {
					return Err(py_err("TypeError: '<' not supported between instances"));
				}
				names.sort();
				let first = names.first().ok_or_else(index_error)?;
				fd.get(first).cloned().ok_or_else(index_error)?
			}
		}
	};
	let font = unpack_item(&rawfont)?;
	interpreter.close();
	match font {
		PsValue::Dict(d) => Ok(d),
		_ => Err(py_err("TypeError: the font is not a dictionary")),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn find_font_name_backtracks() {
		assert_eq!(find_font_name(b"/FontName /Foo def"), Some(&b"Foo"[..]));
		assert_eq!(find_font_name(b"/FontName  /Foo\x0cdef"), Some(&b"Foo"[..]));
		assert_eq!(find_font_name(b"/FontName /Foo"), None);
		assert_eq!(
			find_font_name(b"/FontName/Foo def /FontName /Bar def"),
			Some(&b"Bar"[..])
		);
	}

	#[test]
	fn interpreter_basics() -> Result<()> {
		let mut i = PsInterpreter::new("ascii");
		i.interpret(b"/a 1 def /b [1 2.5 (x) <41 4>] def /c {1 add} def 16#ff 2 eq")?;
		let user = i.dictstack[1].clone();
		assert_eq!(
			unpack_item(&user.borrow()[&Key::from("a")])?,
			PsValue::Int(1)
		);
		assert_eq!(
			unpack_item(&user.borrow()[&Key::from("b")])?,
			PsValue::List(vec![
				PsValue::Int(1),
				PsValue::Float(2.5),
				PsValue::Str("x".into()),
				PsValue::Str("A@".into()),
			])
		);
		assert_eq!(
			unpack_item(&user.borrow()[&Key::from("c")])?,
			PsValue::Tuple(vec![PsValue::Int(1), PsValue::Str("add".into())])
		);
		assert_eq!(unpack_item(&i.stack[0])?, PsValue::Bool(false));
		Ok(())
	}
}
