//! Shared helpers of the ufoLib tests: a port of `Tests/ufoLib/testSupport.py`.
//!
//! The GLIF tests describe glyphs as small Python programs (`glyph.name =
//! "a"`, `pointPen.addPoint(*[(1, 2)], **{...})`), and the font info tests
//! use Python dictionaries; [`parse_py`] reads the Python literals they use.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_misc::py;
use fontoxide_pens::SegmentType;
use fontoxide_ufolib::{
	Dict, FormatVersionInput, GlifPointPen, GlyphObject, Number, NumberPointPen, Result, UfoReader,
	UfoWriter, UfoWriterOptions, Value, read_glyph_from_string, write_glyph_to_string,
};

/// `Tests/ufoLib/testdata`.
pub fn testdata() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../upstream/fonttools/Tests/ufoLib/testdata")
}

/// `getDemoFontPath`.
pub fn demo_font_path() -> PathBuf {
	testdata().join("DemoFont.ufo")
}

/// `getDemoFontGlyphSetPath`.
pub fn demo_font_glyph_set_path() -> PathBuf {
	demo_font_path().join("glyphs")
}

/// A temporary directory, removed when dropped (`tempfile.mkdtemp`).
#[derive(Debug)]
pub struct TempDir(pub PathBuf);

impl TempDir {
	/// The path of the directory.
	pub fn path(&self) -> &Path {
		&self.0
	}

	/// A path in the directory.
	pub fn join(&self, name: &str) -> PathBuf {
		self.0.join(name)
	}
}

impl Drop for TempDir {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A fresh, empty temporary directory.
pub fn tempdir(name: &str) -> TempDir {
	let n = COUNTER.fetch_add(1, Ordering::SeqCst);
	let dir = std::env::temp_dir().join(format!(
		"fontoxide-ufolib-{}-{name}-{n}",
		std::process::id()
	));
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir)
		.unwrap_or_else(|e| panic!("cannot create {}: {e}", dir.display()));
	TempDir(dir)
}

/// Asserts that a result is a `GlifLibError`.
#[track_caller]
pub fn assert_glif_error<T: std::fmt::Debug>(result: Result<T>) {
	match result {
		Err(e) if e.is_glif_lib_error() => {}
		other => panic!("expected a GlifLibError, got {other:?}"),
	}
}

/// Asserts that a result is a `UFOLibError`.
#[track_caller]
pub fn assert_ufo_error<T: std::fmt::Debug>(result: Result<T>) {
	match result {
		Err(e) if e.is_ufo_lib_error() => {}
		other => panic!("expected a UFOLibError, got {other:?}"),
	}
}

// ---------------
// Python literals
// ---------------

/// A Python value of the literals the tests use.
#[derive(Debug, Clone, PartialEq)]
pub enum PyVal {
	/// `None`.
	None,
	/// `True` or `False`.
	Bool(bool),
	/// An `int`.
	Int(i128),
	/// A `float`.
	Float(f64),
	/// A `str`.
	Str(String),
	/// A `list`.
	List(Vec<PyVal>),
	/// A `tuple`.
	Tuple(Vec<PyVal>),
	/// A `dict`.
	Dict(Vec<(PyVal, PyVal)>),
}

struct Parser<'a> {
	s: &'a [u8],
	text: &'a str,
	pos: usize,
}

impl Parser<'_> {
	fn skip(&mut self) {
		while self.pos < self.s.len() {
			match self.s[self.pos] {
				b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
				b'#' => {
					while self.pos < self.s.len() && self.s[self.pos] != b'\n' {
						self.pos += 1;
					}
				}
				_ => break,
			}
		}
	}

	fn peek(&mut self) -> u8 {
		self.skip();
		self.s.get(self.pos).copied().unwrap_or(0)
	}

	fn expect(&mut self, c: u8) {
		assert_eq!(
			self.peek() as char,
			c as char,
			"at {}: {}",
			self.pos,
			&self.text[self.pos..]
		);
		self.pos += 1;
	}

	fn ident(&mut self) -> String {
		self.skip();
		let start = self.pos;
		while self.pos < self.s.len()
			&& (self.s[self.pos].is_ascii_alphanumeric() || self.s[self.pos] == b'_')
		{
			self.pos += 1;
		}
		self.text[start..self.pos].to_owned()
	}

	/// Items up to `close`, with an optional trailing comma.
	fn items(&mut self, close: u8, mut item: impl FnMut(&mut Self)) -> bool {
		let mut trailing_comma = false;
		loop {
			if self.peek() == close {
				self.pos += 1;
				return trailing_comma;
			}
			item(self);
			trailing_comma = false;
			if self.peek() == b',' {
				self.pos += 1;
				trailing_comma = true;
			}
		}
	}

	fn value(&mut self) -> PyVal {
		let v = self.atom();
		// `"a" * 101`
		if self.peek() == b'*' && self.s.get(self.pos + 1) != Some(&b'*') {
			self.pos += 1;
			let PyVal::Int(n) = self.atom() else {
				panic!("unsupported repetition");
			};
			return match v {
				PyVal::Str(s) => PyVal::Str(s.repeat(n as usize)),
				_ => panic!("unsupported repetition"),
			};
		}
		v
	}

	fn atom(&mut self) -> PyVal {
		match self.peek() {
			b'[' => {
				self.pos += 1;
				let mut items = Vec::new();
				self.items(b']', |p| items.push(p.value()));
				PyVal::List(items)
			}
			b'(' => {
				self.pos += 1;
				let mut items = Vec::new();
				let trailing = self.items(b')', |p| items.push(p.value()));
				if items.len() == 1 && !trailing {
					items.remove(0)
				} else {
					PyVal::Tuple(items)
				}
			}
			b'{' => {
				self.pos += 1;
				let mut items = Vec::new();
				self.items(b'}', |p| {
					let k = p.value();
					p.expect(b':');
					let v = p.value();
					items.push((k, v));
				});
				PyVal::Dict(items)
			}
			b'"' | b'\'' => self.string(),
			c if c == b'-' || c == b'+' || c == b'.' || c.is_ascii_digit() => self.number(),
			_ => {
				let name = self.ident();
				match name.as_str() {
					"None" => PyVal::None,
					"True" => PyVal::Bool(true),
					"False" => PyVal::Bool(false),
					"dict" => {
						self.expect(b'(');
						let mut items = Vec::new();
						self.items(b')', |p| {
							let k = p.ident();
							p.expect(b'=');
							let v = p.value();
							items.push((PyVal::Str(k), v));
						});
						PyVal::Dict(items)
					}
					other => panic!("unsupported name {other:?} in {}", self.text),
				}
			}
		}
	}

	fn number(&mut self) -> PyVal {
		let start = self.pos;
		if matches!(self.s[self.pos], b'-' | b'+') {
			self.pos += 1;
		}
		if self.text[self.pos..].starts_with("0x") || self.text[self.pos..].starts_with("0X") {
			self.pos += 2;
			let digits_start = self.pos;
			while self.pos < self.s.len() && self.s[self.pos].is_ascii_hexdigit() {
				self.pos += 1;
			}
			let v = i128::from_str_radix(&self.text[digits_start..self.pos], 16).expect("hex");
			return PyVal::Int(if self.s[start] == b'-' { -v } else { v });
		}
		let mut is_float = false;
		while self.pos < self.s.len() {
			let c = self.s[self.pos];
			if c.is_ascii_digit() || c == b'_' {
				self.pos += 1;
			} else if c == b'.' || c == b'e' || c == b'E' {
				is_float = true;
				self.pos += 1;
				if matches!(c, b'e' | b'E') && matches!(self.s.get(self.pos), Some(b'-' | b'+')) {
					self.pos += 1;
				}
			} else {
				break;
			}
		}
		let t = self.text[start..self.pos].replace('_', "");
		if is_float {
			PyVal::Float(t.parse().expect("float"))
		} else {
			PyVal::Int(t.parse().expect("int"))
		}
	}

	fn string(&mut self) -> PyVal {
		let quote = self.s[self.pos];
		self.pos += 1;
		let mut out = String::new();
		let mut chars = self.text[self.pos..].char_indices();
		while let Some((i, c)) = chars.next() {
			if c as u32 == u32::from(quote) {
				self.pos += i + 1;
				return PyVal::Str(out);
			}
			if c != '\\' {
				out.push(c);
				continue;
			}
			let (_, e) = chars.next().expect("escape");
			let mut hex = |n: usize| {
				let s: String = (0..n).map(|_| chars.next().expect("hex").1).collect();
				char::from_u32(u32::from_str_radix(&s, 16).expect("hex")).expect("char")
			};
			match e {
				'n' => out.push('\n'),
				't' => out.push('\t'),
				'r' => out.push('\r'),
				'0' => out.push('\0'),
				'x' => out.push(hex(2)),
				'u' => out.push(hex(4)),
				'U' => out.push(hex(8)),
				other => out.push(other),
			}
		}
		panic!("unterminated string in {}", self.text)
	}
}

/// Parses a Python literal (`{"a" : [1, 2.5, (3,)]}`, `dict(x=1)`, comments
/// and trailing commas allowed).
pub fn parse_py(text: &str) -> PyVal {
	let mut p = Parser {
		s: text.as_bytes(),
		text,
		pos: 0,
	};
	let v = p.value();
	p.skip();
	assert_eq!(p.pos, text.len(), "trailing text in {text:?}");
	v
}

impl PyVal {
	/// The value as a plist value; `None` for `None`, and dictionary entries
	/// that are `None` are left out (fontTools treats them as missing).
	pub fn to_value(&self) -> Option<Value> {
		Some(match self {
			PyVal::None => return None,
			PyVal::Bool(b) => Value::Bool(*b),
			PyVal::Int(i) => Value::Integer(*i),
			PyVal::Float(f) => Value::Real(*f),
			PyVal::Str(s) => Value::String(s.clone()),
			PyVal::List(a) | PyVal::Tuple(a) => Value::Array(
				a.iter()
					.map(|v| v.to_value().expect("None in a list"))
					.collect(),
			),
			PyVal::Dict(d) => Value::Dict(
				d.iter()
					.filter_map(|(k, v)| {
						let PyVal::Str(k) = k else {
							panic!("non-string key {k:?}");
						};
						v.to_value().map(|v| (k.clone(), v))
					})
					.collect(),
			),
		})
	}

	/// A plist value as a Python value.
	pub fn from_value(value: &Value) -> PyVal {
		match value {
			Value::Bool(b) => PyVal::Bool(*b),
			Value::Integer(i) => PyVal::Int(*i),
			Value::Real(f) => PyVal::Float(*f),
			Value::String(s) => PyVal::Str(s.clone()),
			Value::Array(a) => PyVal::List(a.iter().map(PyVal::from_value).collect()),
			Value::Dict(d) => PyVal::Dict(
				d.iter()
					.map(|(k, v)| (PyVal::Str(k.clone()), PyVal::from_value(v)))
					.collect(),
			),
			other => PyVal::Str(format!("{other:?}")),
		}
	}

	/// `repr(value)` of numbers and booleans.
	fn repr(&self) -> String {
		match self {
			PyVal::None => "None".into(),
			PyVal::Bool(b) => if *b { "True" } else { "False" }.into(),
			PyVal::Int(i) => i.to_string(),
			PyVal::Float(f) => py::float_repr(*f),
			PyVal::Str(s) => py::str_repr(s),
			other => format!("{other:?}"),
		}
	}

	/// The `str` of a string.
	fn as_str(&self) -> Option<&str> {
		match self {
			PyVal::Str(s) => Some(s),
			_ => None,
		}
	}
}

/// A Python literal that must be a plist value.
pub fn py_value(text: &str) -> Value {
	parse_py(text).to_value().expect("not None")
}

/// A Python dictionary literal as a [`Dict`].
pub fn py_dict(text: &str) -> Dict {
	match py_value(text) {
		Value::Dict(d) => d,
		other => panic!("not a dict: {other:?}"),
	}
}

fn element_to_string(value: &PyVal) -> String {
	match value {
		PyVal::Dict(d) => dict_to_string(d),
		PyVal::List(l) => list_to_string(l, "[", "]"),
		PyVal::Tuple(t) => list_to_string(t, "(", ")"),
		PyVal::Int(_) | PyVal::Float(_) | PyVal::Bool(_) => value.repr(),
		PyVal::Str(s) => format!("\"{s}\""),
		PyVal::None => "None".into(),
	}
}

/// `_dictToString`.
fn dict_to_string(d: &[(PyVal, PyVal)]) -> String {
	let mut items: Vec<&(PyVal, PyVal)> = d.iter().collect();
	items.sort_by(|a, b| a.0.as_str().cmp(&b.0.as_str()));
	let text: Vec<String> = items
		.into_iter()
		.filter(|(_, v)| *v != PyVal::None)
		.map(|(k, v)| {
			format!(
				"\"{}\" : {}",
				k.as_str().unwrap_or_default(),
				element_to_string(v)
			)
		})
		.collect();
	if text.is_empty() {
		return String::new();
	}
	format!("{{{}}}", text.join(", "))
}

/// `_listToString` and `_tupleToString`.
fn list_to_string(l: &[PyVal], open: &str, close: &str) -> String {
	if l.is_empty() {
		return String::new();
	}
	let text: Vec<String> = l.iter().map(element_to_string).collect();
	format!("{open}{}{close}", text.join(", "))
}

// ----------------
// GLIF test tools
// ----------------

/// `_writePointPenCommand`.
fn point_pen_command(command: &str, args: &[PyVal], kwargs: &[(PyVal, PyVal)]) -> String {
	let args = list_to_string(args, "[", "]");
	let kwargs = dict_to_string(kwargs);
	match (args.is_empty(), kwargs.is_empty()) {
		(false, false) => format!("pointPen.{command}(*{args}, **{kwargs})"),
		(false, true) => format!("pointPen.{command}(*{args})"),
		(true, false) => format!("pointPen.{command}(**{kwargs})"),
		(true, true) => format!("pointPen.{command}()"),
	}
}

fn number_val(n: Number) -> PyVal {
	match n {
		Number::Int(i) => PyVal::Int(i128::from(i)),
		Number::Float(f) => PyVal::Float(f),
	}
}

fn opt_str(s: Option<&str>) -> PyVal {
	s.map_or(PyVal::None, |s| PyVal::Str(s.to_owned()))
}

fn kw(name: &str, v: PyVal) -> (PyVal, PyVal) {
	(PyVal::Str(name.to_owned()), v)
}

/// The point pen side of `testSupport.Glyph`: records the calls as Python.
#[derive(Debug, Default)]
pub struct PenRecorder {
	/// `outline`.
	pub outline: Vec<String>,
}

impl NumberPointPen for PenRecorder {
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()> {
		self.outline.push(point_pen_command(
			"beginPath",
			&[],
			&[kw("identifier", opt_str(identifier))],
		));
		Ok(())
	}

	fn end_path(&mut self) -> Result<()> {
		self.outline.push(point_pen_command("endPath", &[], &[]));
		Ok(())
	}

	fn add_point(
		&mut self,
		pt: (Number, Number),
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		let args = [PyVal::Tuple(vec![number_val(pt.0), number_val(pt.1)])];
		let kwargs = [
			kw(
				"segmentType",
				opt_str(segment_type.map(SegmentType::as_str)),
			),
			kw("smooth", PyVal::Bool(smooth)),
			kw("name", opt_str(name)),
			kw("identifier", opt_str(identifier)),
		];
		self.outline
			.push(point_pen_command("addPoint", &args, &kwargs));
		Ok(())
	}

	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: [Number; 6],
		identifier: Option<&str>,
	) -> Result<()> {
		let args = [
			PyVal::Str(base_glyph_name.to_owned()),
			PyVal::Tuple(transformation.iter().map(|&n| number_val(n)).collect()),
		];
		self.outline.push(point_pen_command(
			"addComponent",
			&args,
			&[kw("identifier", opt_str(identifier))],
		));
		Ok(())
	}
}

/// A point pen call of a Python glyph description.
#[derive(Debug, Clone)]
struct Command {
	method: String,
	args: Vec<PyVal>,
	kwargs: Vec<(String, PyVal)>,
}

/// `testSupport.Glyph`: glyph attributes as Python values, and an outline.
#[derive(Debug, Default)]
pub struct TestGlyph {
	attrs: Vec<(String, PyVal)>,
	commands: Vec<Command>,
	/// `outline`: the point pen calls, as Python.
	pub outline: Vec<String>,
}

impl TestGlyph {
	fn attr(&self, name: &str) -> Option<&PyVal> {
		self.attrs
			.iter()
			.find(|(n, _)| n == name)
			.map(|(_, v)| v)
			.filter(|v| **v != PyVal::None)
	}

	/// `glyph.attr = value`.
	pub fn set(&mut self, name: &str, value: PyVal) {
		match self.attrs.iter_mut().find(|(n, _)| n == name) {
			Some(slot) => slot.1 = value,
			None => self.attrs.push((name.to_owned(), value)),
		}
	}

	/// `py()`: the glyph as Python.
	pub fn py(&self) -> String {
		let mut text = Vec::new();
		if let Some(name) = self.attr("name") {
			text.push(format!(
				"glyph.name = \"{}\"",
				name.as_str().unwrap_or_default()
			));
		}
		let truthy =
			|v: &PyVal| !matches!(v, PyVal::Int(0) | PyVal::Bool(false)) && *v != PyVal::Float(0.0);
		for attr in ["width", "height"] {
			if let Some(v) = self.attr(attr).filter(|v| truthy(v)) {
				text.push(format!("glyph.{attr} = {}", v.repr()));
			}
		}
		if let Some(PyVal::List(u)) = self.attr("unicodes") {
			let codes: Vec<String> = u.iter().map(PyVal::repr).collect();
			text.push(format!("glyph.unicodes = [{}]", codes.join(", ")));
		}
		if let Some(note) = self.attr("note") {
			text.push(format!(
				"glyph.note = \"{}\"",
				note.as_str().unwrap_or_default()
			));
		}
		for attr in ["lib", "image", "guidelines", "anchors"] {
			if let Some(v) = self.attr(attr) {
				text.push(format!("glyph.{attr} = {}", element_to_string(v)));
			}
		}
		text.extend(self.outline.iter().cloned());
		text.join("\n")
	}

	/// `exec(py, {"glyph": glyph, "pointPen": glyph})`.
	pub fn exec(&mut self, py: &str) {
		for line in py.lines() {
			let line = line.trim();
			if line.is_empty() {
				continue;
			}
			if let Some(rest) = line.strip_prefix("glyph.") {
				let (name, value) = rest.split_once('=').expect("assignment");
				self.set(name.trim(), parse_py(value.trim()));
			} else if let Some(rest) = line.strip_prefix("pointPen.") {
				let open = rest.find('(').expect("call");
				let method = rest[..open].to_owned();
				let inner = &rest[open + 1..rest.len() - 1];
				let mut args = Vec::new();
				let mut kwargs = Vec::new();
				// `*[...]` and `**{...}`, separated by a comma
				let mut p = Parser {
					s: inner.as_bytes(),
					text: inner,
					pos: 0,
				};
				while p.peek() != 0 {
					if inner[p.pos..].starts_with("**") {
						p.pos += 2;
						let PyVal::Dict(d) = p.value() else {
							panic!("kwargs");
						};
						kwargs = d
							.into_iter()
							.map(|(k, v)| (k.as_str().unwrap_or_default().to_owned(), v))
							.collect();
					} else if inner[p.pos..].starts_with('*') {
						p.pos += 1;
						let PyVal::List(a) = p.value() else {
							panic!("args");
						};
						args = a;
					} else {
						panic!("unsupported call {line}");
					}
					if p.peek() == b',' {
						p.pos += 1;
					}
				}
				self.outline.push(line.to_owned());
				self.commands.push(Command {
					method,
					args,
					kwargs,
				});
			} else {
				panic!("unsupported line {line}");
			}
		}
	}

	/// `drawPoints(pointPen)` into the GLIF point pen, with Python's dynamic
	/// arguments.
	pub fn draw_points(&self, pen: &mut GlifPointPen) -> Result<()> {
		for c in &self.commands {
			let kwarg = |name: &str| c.kwargs.iter().find(|(k, _)| k == name).map(|(_, v)| v);
			let kwarg_str = |name: &str| kwarg(name).and_then(PyVal::as_str);
			match c.method.as_str() {
				"beginPath" => pen.begin_path(kwarg_str("identifier"))?,
				"endPath" => pen.end_path()?,
				"addPoint" => {
					let PyVal::Tuple(pt) = &c.args[0] else {
						panic!("point");
					};
					let x = pt[0].to_value().expect("x");
					let y = pt[1].to_value().expect("y");
					let smooth = matches!(kwarg("smooth"), Some(PyVal::Bool(true)));
					pen.add_point_values(
						(&x, &y),
						kwarg_str("segmentType"),
						smooth,
						kwarg_str("name"),
						kwarg_str("identifier"),
					)?;
				}
				"addComponent" => {
					let base = c.args[0].as_str().expect("base");
					let PyVal::Tuple(t) = &c.args[1] else {
						panic!("transformation");
					};
					let t: Vec<Value> = t.iter().map(|v| v.to_value().expect("number")).collect();
					pen.add_component_values(base, &t, kwarg_str("identifier"))?;
				}
				other => panic!("unsupported method {other}"),
			}
		}
		Ok(())
	}
}

impl GlyphObject for TestGlyph {
	fn get_attr(&self, attr: &str) -> Option<Value> {
		self.attr(attr).and_then(PyVal::to_value)
	}

	fn set_attr(&mut self, attr: &str, value: Value) {
		self.set(attr, PyVal::from_value(&value));
	}
}

/// `stripText`.
pub fn strip_text(text: &str) -> String {
	py::strip(text)
		.lines()
		.map(py::strip)
		.filter(|l| !l.is_empty())
		.collect::<Vec<_>>()
		.join("\n")
}

/// `pyToGLIF`: the GLIF of a glyph described in Python, without the XML
/// declaration.
pub fn py_to_glif(py: &str, format_version: i64) -> Result<String> {
	let py = strip_text(py);
	let mut glyph = TestGlyph::default();
	glyph.exec(&py);
	let name = match glyph.attr("name") {
		Some(PyVal::Str(s)) => s.clone(),
		other => panic!("the glyph name must be a string in Rust, got {other:?}"),
	};
	let glif = write_glyph_to_string(
		&name,
		Some(&glyph),
		Some(&mut |pen: &mut GlifPointPen| glyph.draw_points(pen)),
		Some(FormatVersionInput::from(format_version)),
		true,
	)?;
	Ok(glif.lines().skip(1).collect::<Vec<_>>().join("\n"))
}

/// `glifToPy`: a glyph read from GLIF, as Python.
pub fn glif_to_py(glif: &str) -> Result<String> {
	let glif = format!("<?xml version=\"1.0\"?>\n{}", strip_text(glif));
	let mut glyph = TestGlyph::default();
	let mut pen = PenRecorder::default();
	read_glyph_from_string(
		glif.as_bytes(),
		Some(&mut glyph),
		Some(&mut pen),
		None,
		true,
	)?;
	glyph.outline = pen.outline;
	Ok(glyph.py())
}

// ----------------------------------
// font info values used by several tests
// ----------------------------------

/// A dictionary of `testSupport.py`, read from its source.
pub fn test_support_dict(name: &str) -> Dict {
	let source = std::fs::read_to_string(testdata().join("../testSupport.py"))
		.expect("testSupport.py")
		.replace("\r\n", "\n");
	let start = source
		.find(&format!("\n{name} = "))
		.unwrap_or_else(|| panic!("no {name}"))
		+ name.len()
		+ 4;
	// the literal ends at the first line that closes it
	let end = source[start..].find("\n}\n").expect("end") + start + 2;
	py_dict(&source[start..end])
}

/// `fontInfoVersion1`.
pub fn font_info_version1() -> Dict {
	test_support_dict("fontInfoVersion1")
}

/// `fontInfoVersion2`.
pub fn font_info_version2() -> Dict {
	test_support_dict("fontInfoVersion2")
}

/// `fontInfoVersion3`.
pub fn font_info_version3() -> Dict {
	test_support_dict("fontInfoVersion3")
}

/// `expectedFontInfo1To2Conversion`.
pub fn expected_font_info_1_to_2_conversion() -> Dict {
	test_support_dict("expectedFontInfo1To2Conversion")
}

/// `expectedFontInfo2To1Conversion`.
pub fn expected_font_info_2_to_1_conversion() -> Dict {
	test_support_dict("expectedFontInfo2To1Conversion")
}

/// Writes a plist file (`plistlib.dump`).
pub fn write_plist(path: &Path, value: &Value) {
	let text = fontoxide_ufolib::plistlib::dumps(value, true, true).expect("plist");
	std::fs::write(path, text).expect("write");
}

/// Reads a plist file (`plistlib.load`).
pub fn read_plist(path: &Path) -> Value {
	fontoxide_ufolib::plistlib::loads_bytes(&std::fs::read(path).expect("read")).expect("plist")
}

/// A `UFOLibError` with the message, for tests of the messages.
pub fn message<T>(result: Result<T>) -> String {
	match result {
		Err(e) => e.to_string(),
		Ok(_) => panic!("expected an error"),
	}
}

/// A font info dictionary with changes given as Python literals; `None`
/// removes the attribute.
pub fn modified_info(base: &Dict, changes: &[(&str, &str)]) -> Dict {
	let mut info = base.clone();
	for (attr, value) in changes {
		match parse_py(value).to_value() {
			Some(v) => {
				info.insert((*attr).to_owned(), v);
			}
			None => {
				info.shift_remove(*attr);
			}
		}
	}
	info
}

/// The font info read tests: each case writes `fontinfo.plist` with
/// changes to `base` and reads it with validation, which must fail or not.
pub fn check_read(
	format_version: i64,
	base: &Dict,
	cases: &[(&[(&str, &str)], bool)],
) -> Result<()> {
	let dir = tempdir("readinfo");
	let mut meta = Dict::new();
	meta.insert("creator".into(), Value::from("test"));
	meta.insert("formatVersion".into(), Value::from(format_version));
	write_plist(&dir.join("metainfo.plist"), &Value::Dict(meta));
	for (changes, error) in cases {
		let info = modified_info(base, changes);
		write_plist(&dir.join("fontinfo.plist"), &Value::Dict(info));
		let reader = UfoReader::new(dir.path(), true)?;
		let result = reader.read_info(None);
		if *error {
			assert!(
				matches!(&result, Err(e) if e.is_ufo_lib_error()),
				"{changes:?}: expected a UFOLibError, got {result:?}"
			);
		} else {
			assert!(result.is_ok(), "{changes:?}: {result:?}");
		}
	}
	Ok(())
}

/// The font info write tests: each case writes `base` with changes, which
/// must fail or not. `tear_down` removes the UFO between cases.
pub fn check_write(
	format_version: i64,
	base: &Dict,
	tear_down: bool,
	cases: &[(&[(&str, &str)], bool)],
) -> Result<()> {
	let dir = tempdir("writeinfo");
	let ufo = dir.join("test.ufo");
	for (changes, error) in cases {
		let info = modified_info(base, changes);
		let writer = UfoWriter::new(
			&ufo,
			UfoWriterOptions {
				format_version: Some(format_version.into()),
				..Default::default()
			},
		)?;
		let result = writer.write_info(&info, None);
		if *error {
			assert!(
				matches!(&result, Err(e) if e.is_ufo_lib_error()),
				"{changes:?}: expected a UFOLibError, got {result:?}"
			);
		} else {
			assert!(result.is_ok(), "{changes:?}: {result:?}");
		}
		drop(writer);
		if tear_down && ufo.exists() {
			std::fs::remove_dir_all(&ufo).expect("tear down");
		}
	}
	Ok(())
}

// ------------
// log capture
// ------------

struct ThreadLogger;

thread_local! {
	static RECORDS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl log::Log for ThreadLogger {
	fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
		metadata.level() <= log::Level::Warn
	}

	fn log(&self, record: &log::Record<'_>) {
		if self.enabled(record.metadata()) {
			RECORDS.with(|r| r.borrow_mut().push(record.args().to_string()));
		}
	}

	fn flush(&self) {}
}

static LOGGER: ThreadLogger = ThreadLogger;

/// `caplog`: clears and returns the warnings logged by this thread.
pub fn take_logs() -> Vec<String> {
	let _ = log::set_logger(&LOGGER);
	log::set_max_level(log::LevelFilter::Warn);
	RECORDS.with(|r| std::mem::take(&mut *r.borrow_mut()))
}
