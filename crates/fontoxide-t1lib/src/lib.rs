//! Port of `fontTools.t1Lib`: tools for PostScript Type 1 fonts.
//!
//! Reading and writing raw Type 1 data:
//!
//! - [`read`] reads any Type 1 font file and returns the raw data and its kind:
//!   `"LWFN"`, `"PFB"` or `"OTHER"`.
//! - [`write`] writes raw Type 1 data as `"LWFN"`, `"PFB"` or `"OTHER"` (PFA),
//!   the encrypted part in hexadecimal or binary.
//!
//! [`T1Font`] parses the font program with the minimal PostScript interpreter of
//! [`fontoxide_misc::ps_lib`], and can write it back.

use std::path::Path;

use fontoxide_misc::eexec;
use fontoxide_misc::ps_lib::{self, PsValue};
use fontoxide_misc::ps_operators::{
	Key, TYPE1_FONTINFO_ORDER, TYPE1_POST_EEXEC_ORDER, TYPE1_PRE_EEXEC_ORDER,
};
use fontoxide_misc::py;
use fontoxide_pens::Pen;
pub use fontoxide_ttlib::cff::t1_charstring::T1CharString;
use indexmap::IndexMap;
use thiserror::Error;

/// Errors of this crate.
#[derive(Debug, Error)]
pub enum T1Error {
	/// `T1Error`.
	#[error("{0}")]
	T1(String),
	/// A file could not be read or written.
	#[error(transparent)]
	Io(#[from] std::io::Error),
	/// The PostScript interpreter failed.
	#[error(transparent)]
	Ps(#[from] ps_lib::PsError),
	/// A charstring could not be compiled or drawn.
	#[error(transparent)]
	CharString(#[from] fontoxide_ttlib::Error),
	/// The resource fork of an LWFN file could not be read.
	#[error(transparent)]
	Resource(#[from] fontoxide_misc::mac_res::ResourceError),
	/// Other Python exceptions fontTools raises on bad input (`KeyError`,
	/// `TypeError`, `ValueError`, `binascii.Error`, ...).
	#[error("{0}")]
	Python(String),
}

/// The result type of this crate.
pub type Result<T> = std::result::Result<T, T1Error>;

fn py_err(msg: impl Into<String>) -> T1Error {
	T1Error::Python(msg.into())
}

impl From<eexec::HexError> for T1Error {
	fn from(e: eexec::HexError) -> Self {
		T1Error::Python(format!("binascii.Error: {e}"))
	}
}

/// A value of a parsed font dictionary: the values of
/// [`ps_lib::PsValue`], with the charstrings decrypted into [`T1CharString`]s.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
	/// `None`.
	None,
	/// A `bool`.
	Bool(bool),
	/// An `int`; `true` and `false` of the font program are 1 and 0.
	Int(i64),
	/// A `float`.
	Float(f64),
	/// A `str`.
	Str(String),
	/// A `bytes`.
	Bytes(Vec<u8>),
	/// A `list`: arrays.
	List(Vec<Value>),
	/// A `tuple`: procedures.
	Tuple(Vec<Value>),
	/// A `dict`.
	Dict(Dict),
	/// The file object of the interpreter.
	File,
	/// A charstring or subroutine, once parsed.
	CharString(T1CharString),
}

/// A font dictionary.
pub type Dict = IndexMap<Key, Value>;

impl From<PsValue> for Value {
	fn from(v: PsValue) -> Self {
		match v {
			PsValue::None => Value::None,
			PsValue::Bool(b) => Value::Bool(b),
			PsValue::Int(i) => Value::Int(i),
			PsValue::Float(f) => Value::Float(f),
			PsValue::Str(s) => Value::Str(s),
			PsValue::Bytes(b) => Value::Bytes(b),
			PsValue::List(l) => Value::List(l.into_iter().map(Value::from).collect()),
			PsValue::Tuple(l) => Value::Tuple(l.into_iter().map(Value::from).collect()),
			PsValue::Dict(d) => Value::Dict(d.into_iter().map(|(k, v)| (k, v.into())).collect()),
			PsValue::File => Value::File,
		}
	}
}

impl Value {
	/// The dictionary, for dictionary values.
	pub fn as_dict(&self) -> Option<&Dict> {
		match self {
			Value::Dict(d) => Some(d),
			_ => None,
		}
	}

	/// The dictionary, for dictionary values.
	pub fn as_dict_mut(&mut self) -> Option<&mut Dict> {
		match self {
			Value::Dict(d) => Some(d),
			_ => None,
		}
	}

	/// The string, for string values.
	pub fn as_str(&self) -> Option<&str> {
		match self {
			Value::Str(s) => Some(s),
			_ => None,
		}
	}

	/// Python truthiness (`bool(value)`).
	pub fn is_truthy(&self) -> bool {
		match self {
			Value::None => false,
			Value::Bool(b) => *b,
			Value::Int(i) => *i != 0,
			Value::Float(f) => *f != 0.0,
			Value::Str(s) => !s.is_empty(),
			Value::Bytes(b) => !b.is_empty(),
			Value::List(l) | Value::Tuple(l) => !l.is_empty(),
			Value::Dict(d) => !d.is_empty(),
			Value::File | Value::CharString(_) => true,
		}
	}

	/// Python equality (`==`): numbers compare by value; charstrings, like
	/// Python objects without `__eq__`, only equal themselves, which a copy is
	/// not, so they compare by content here.
	pub fn py_eq(&self, other: &Value) -> bool {
		use Value::*;
		match (self, other) {
			(Bool(_) | Int(_) | Float(_), Bool(_) | Int(_) | Float(_)) => {
				match (self.as_i64(), other.as_i64()) {
					(Some(a), Some(b)) => a == b,
					_ => self.as_f64() == other.as_f64(),
				}
			}
			(Str(a), Str(b)) => a == b,
			(Bytes(a), Bytes(b)) => a == b,
			(List(a), List(b)) | (Tuple(a), Tuple(b)) => {
				a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.py_eq(y))
			}
			(Dict(a), Dict(b)) => {
				a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|w| v.py_eq(w)))
			}
			(None, None) | (File, File) => true,
			(CharString(a), CharString(b)) => a == b,
			_ => false,
		}
	}

	fn as_i64(&self) -> Option<i64> {
		match self {
			Value::Bool(b) => Some(*b as i64),
			Value::Int(i) => Some(*i),
			_ => None,
		}
	}

	fn as_f64(&self) -> Option<f64> {
		match self {
			Value::Bool(b) => Some(*b as i64 as f64),
			Value::Int(i) => Some(*i as f64),
			Value::Float(f) => Some(*f),
			_ => None,
		}
	}

	/// `repr(value)`.
	pub fn repr(&self) -> String {
		match self {
			Value::Str(s) => py::str_repr(s),
			Value::Bytes(b) => py::bytes_repr(b),
			_ => self.to_py_string(),
		}
	}

	/// `str(value)`.
	pub fn to_py_string(&self) -> String {
		match self {
			Value::None => "None".into(),
			Value::Bool(b) => if *b { "True" } else { "False" }.into(),
			Value::Int(i) => i.to_string(),
			Value::Float(f) => py::float_repr(*f),
			Value::Str(s) => s.clone(),
			Value::Bytes(b) => py::bytes_repr(b),
			Value::List(l) => format!(
				"[{}]",
				l.iter().map(Value::repr).collect::<Vec<_>>().join(", ")
			),
			Value::Tuple(l) => {
				if l.len() == 1 {
					format!("({},)", l[0].repr())
				} else {
					format!(
						"({})",
						l.iter().map(Value::repr).collect::<Vec<_>>().join(", ")
					)
				}
			}
			Value::Dict(d) => {
				let items: Vec<String> = d
					.iter()
					.map(|(k, v)| format!("{}: {}", key_repr(k), v.repr()))
					.collect();
				format!("{{{}}}", items.join(", "))
			}
			// The addresses of Python objects are not reproduced.
			Value::File => "<fontTools.misc.psLib.PSTokenizer object>".into(),
			Value::CharString(cs) => {
				if cs.bytecode.is_some() {
					"<T1CharString (bytecode)>".into()
				} else {
					"<T1CharString (source)>".into()
				}
			}
		}
	}
}

fn key_repr(k: &Key) -> String {
	match k {
		Key::Str(s) => py::str_repr(s),
		other => other.to_string(),
	}
}

fn key_is(k: &Key, s: &str) -> bool {
	k.as_str() == Some(s)
}

fn key_truthy(k: &Key) -> bool {
	match k {
		Key::None => false,
		Key::Int(i) => *i != 0,
		Key::Float(bits) => f64::from_bits(*bits) != 0.0,
		Key::Str(s) => !s.is_empty(),
		Key::Bytes(b) => !b.is_empty(),
		Key::File(_) => true,
	}
}

/// The Python slice `seq[:n]`.
fn head(seq: &[u8], n: i64) -> &[u8] {
	let len = seq.len() as i64;
	let end = if n < 0 { (len + n).max(0) } else { n.min(len) };
	&seq[..end as usize]
}

/// The Python slice `seq[n:]`.
fn tail(seq: &[u8], n: i64) -> &[u8] {
	let len = seq.len() as i64;
	let start = if n < 0 { (len + n).max(0) } else { n.min(len) };
	&seq[start as usize..]
}

/// A Type 1 font (`T1Font`).
///
/// Uses a minimal interpreter that supports just about enough PostScript to
/// parse Type 1 fonts.
#[derive(Debug, Clone)]
pub struct T1Font {
	data: Option<Vec<u8>>,
	encoding: String,
	/// The font dictionary, once parsed (`font`).
	pub font: Option<Dict>,
}

impl T1Font {
	/// Reads a font file (`T1Font(path, encoding, kind)`); `kind` is `"LWFN"`,
	/// `"PFB"` or `"OTHER"`, or `None` to tell from the file.
	pub fn open(path: impl AsRef<Path>, encoding: &str, kind: Option<&str>) -> Result<Self> {
		let path = path.as_ref();
		let data = match kind {
			None => read(path, false)?.0,
			Some("LWFN") => read_lwfn(path, false)?,
			Some("PFB") => read_pfb(path, false)?,
			Some("OTHER") => read_other(path)?,
			Some(other) => return Err(py_err(format!("ValueError: {other}"))),
		};
		Ok(Self::from_data(data, encoding))
	}

	/// A font from raw Type 1 data, as [`read`] returns it.
	pub fn from_data(data: Vec<u8>, encoding: &str) -> Self {
		T1Font {
			data: Some(data),
			encoding: encoding.to_owned(),
			font: None,
		}
	}

	/// Writes the font (`saveAs`).
	pub fn save_as(&mut self, path: impl AsRef<Path>, kind: &str, dohex: bool) -> Result<()> {
		let data = self.get_data()?.to_vec();
		write(path, &data, kind, dohex)
	}

	/// The raw data: as read, or made from the font dictionary once parsed
	/// (`getData`).
	pub fn get_data(&mut self) -> Result<&[u8]> {
		if self.data.is_none() {
			self.data = Some(self.create_data()?);
		}
		Ok(self.data.as_deref().unwrap_or_default())
	}

	/// The parsed font dictionary, parsing it if needed.
	pub fn font_dict(&mut self) -> Result<&mut Dict> {
		if self.font.is_none() {
			self.parse()?;
		}
		self.font
			.as_mut()
			.ok_or_else(|| py_err("AttributeError: 'T1Font' object has no attribute 'font'"))
	}

	/// A value of the font dictionary, parsing it if needed (`font[key]`).
	pub fn get(&mut self, key: &str) -> Result<&mut Value> {
		self.font_dict()?
			.get_mut(&Key::from(key))
			.ok_or_else(|| py_err(format!("KeyError: {}", py::str_repr(key))))
	}

	/// The glyph set (`getGlyphSet`): the `CharStrings` dictionary, with the
	/// subroutines to draw them.
	pub fn glyph_set(&mut self) -> Result<T1GlyphSet<'_>> {
		T1GlyphSet::new(self.font_dict()?)
	}

	/// Interprets the font program and decrypts the charstrings and
	/// subroutines (`parse`). The raw data is dropped: [`T1Font::get_data`]
	/// then makes it from the font dictionary.
	pub fn parse(&mut self) -> Result<()> {
		let data = self
			.data
			.as_deref()
			.ok_or_else(|| py_err("AttributeError: 'T1Font' object has no attribute 'data'"))?;
		let font = ps_lib::suckfont(data, &self.encoding)?;
		let mut font: Dict = font.into_iter().map(|(k, v)| (k, v.into())).collect();
		let len_iv = {
			let private = font
				.get(&Key::from("Private"))
				.ok_or_else(|| py_err("KeyError: 'Private'"))?
				.as_dict()
				.ok_or_else(|| py_err("TypeError: 'Private' is not a dict"))?;
			let len_iv = match private.get(&Key::from("lenIV")) {
				None => 4,
				Some(Value::Int(i)) => *i,
				Some(Value::Bool(b)) => *b as i64,
				Some(_) => return Err(py_err("TypeError: slice indices must be integers")),
			};
			if len_iv < 0 {
				return Err(T1Error::Python("AssertionError: lenIV >= 0".into()));
			}
			if !private.contains_key(&Key::from("Subrs")) {
				return Err(py_err("KeyError: 'Subrs'"));
			}
			len_iv
		};
		if !font.contains_key(&Key::from("CharStrings")) {
			return Err(py_err("KeyError: 'CharStrings'"));
		}
		let decrypt = |v: &Value| -> Result<T1CharString> {
			let cipher: Vec<u8> = match v {
				Value::Bytes(b) => b.clone(),
				Value::Str(s) => s
					.chars()
					.map(|c| {
						u8::try_from(c as u32)
							.map_err(|_| py_err("ValueError: byte must be in range(0, 256)"))
					})
					.collect::<Result<_>>()?,
				_ => return Err(py_err("TypeError: the charstring is not a string")),
			};
			let (plain, _) = eexec::decrypt(&cipher, 4330);
			Ok(T1CharString::from_bytecode(tail(&plain, len_iv).to_vec()))
		};
		if let Some(Value::Dict(char_strings)) = font.get_mut(&Key::from("CharStrings")) {
			for value in char_strings.values_mut() {
				*value = Value::CharString(decrypt(value)?);
			}
		} else {
			return Err(py_err(
				"AttributeError: 'CharStrings' has no attribute 'items'",
			));
		}
		if let Some(Value::Dict(private)) = font.get_mut(&Key::from("Private"))
			&& let Some(subrs) = private.get_mut(&Key::from("Subrs"))
		{
			match subrs {
				Value::List(subrs) => {
					for value in subrs.iter_mut() {
						*value = Value::CharString(decrypt(value)?);
					}
				}
				_ => return Err(py_err("TypeError: 'Subrs' is not a list")),
			}
		}
		self.font = Some(font);
		self.data = None;
		Ok(())
	}

	fn tobytes(&self, s: &str) -> Result<Vec<u8>> {
		let name = fontoxide_encodings::codecs::normalize(&self.encoding);
		if matches!(name.as_str(), "utf_8" | "utf8" | "u8" | "utf") {
			return Ok(s.as_bytes().to_vec());
		}
		fontoxide_encodings::codecs::encode(&self.encoding, s)
			.map_err(|e| py_err(format!("UnicodeEncodeError: {e}")))
	}

	/// Makes the raw data from the font dictionary (`createData`).
	pub fn create_data(&self) -> Result<Vec<u8>> {
		let sf = self
			.font
			.as_ref()
			.ok_or_else(|| py_err("AttributeError: 'T1Font' object has no attribute 'font'"))?;
		let font_name = sf
			.get(&Key::from("FontName"))
			.ok_or_else(|| py_err("KeyError: 'FontName'"))?
			.to_py_string();
		let mut lines: Vec<Vec<u8>> = vec![
			self.tobytes(&format!("%!FontType1-1.1: {font_name}"))?,
			self.tobytes(&format!(
				"%t1Font: ({})",
				fontoxide_ttlib::FONTTOOLS_VERSION
			))?,
			self.tobytes(&format!("%%BeginResource: font {font_name}"))?,
		];
		// follow t1write.c:writeRegNameKeyedFont
		let mut size = 3 + 1 + 1 + 1;
		for key in font_dictionary_keys() {
			size += sf.contains_key(&Key::from(key)) as usize;
		}
		lines.push(self.tobytes(&format!("{size} dict dup begin"))?);

		let mut eexec_began = false;
		let mut eexec_dict: Vec<(&Key, &Value)> = Vec::new();
		for (key, value) in sf {
			if eexec_began {
				eexec_dict.push((key, value));
				continue;
			}
			if key_is(key, "FontInfo") {
				let fi = value
					.as_dict()
					.ok_or_else(|| py_err("AttributeError: 'FontInfo' has no attribute 'items'"))?;
				// follow t1write.c:writeFontInfoDict
				let mut size = 3;
				for subkey in fontinfo_dictionary_keys() {
					size += fi.contains_key(&Key::from(subkey)) as usize;
				}
				lines.push(self.tobytes(&format!("/FontInfo {size} dict dup begin"))?);
				for (subkey, subvalue) in fi {
					lines.extend(self.make_lines(subkey, subvalue)?);
				}
				lines.push(b"end def".to_vec());
			} else if key
				.as_str()
				.is_some_and(|k| TYPE1_POST_EEXEC_ORDER.contains(&k))
			{
				eexec_dict.push((key, value));
				eexec_began = true;
			} else {
				lines.extend(self.make_lines(key, value)?);
			}
		}
		lines.push(b"end".to_vec());
		let eexec_portion = self.encode_eexec(&eexec_dict)?;
		let mut line = b"currentfile eexec ".to_vec();
		line.extend(eexec_portion);
		lines.push(line);
		for _ in 0..8 {
			lines.push(vec![b'0'; 64]);
		}
		lines.extend([
			b"cleartomark".to_vec(),
			b"%%EndResource".to_vec(),
			b"%%EOF".to_vec(),
		]);
		Ok(lines.join(&b"\n"[..]))
	}

	/// `encode_eexec`: the encrypted part.
	fn encode_eexec(&self, eexec_dict: &[(&Key, &Value)]) -> Result<Vec<u8>> {
		let mut lines: Vec<Vec<u8>> = Vec::new();
		// '-|', '|-', '|'
		let mut rd_key: Option<&Key> = None;
		let mut nd_key: Option<&Key> = None;
		let mut np_key: Option<&Key> = None;
		let mut len_iv: &Value = &Value::Int(4);
		let opt = |k: Option<&Key>| k.map_or_else(|| "None".to_owned(), Key::to_string);

		// Private first: it has the RD, ND and NP keys and lenIV.
		let mut sorted: Vec<&(&Key, &Value)> = eexec_dict.iter().collect();
		sorted.sort_by_key(|(k, _)| !key_is(k, "Private"));

		for (key, value) in sorted {
			if key_is(key, "Private") {
				let pr = value
					.as_dict()
					.ok_or_else(|| py_err("AttributeError: 'Private' has no attribute 'items'"))?;
				// follow t1write.c:writePrivateDict
				let mut size = 3;
				for subkey in PRIVATE_DICTIONARY_KEYS {
					size += pr.contains_key(&Key::from(subkey)) as usize;
				}
				lines.push(b"dup /Private".to_vec());
				lines.push(self.tobytes(&format!("{size} dict dup begin"))?);
				for (subkey, subvalue) in pr {
					if !rd_key.is_some_and(key_truthy) && is_procedure(subvalue, RD_VALUE) {
						rd_key = Some(subkey);
					} else if !nd_key.is_some_and(key_truthy)
						&& ND_VALUES.iter().any(|v| is_procedure(subvalue, v))
					{
						nd_key = Some(subkey);
					} else if !np_key.is_some_and(key_truthy)
						&& PD_VALUES.iter().any(|v| is_procedure(subvalue, v))
					{
						np_key = Some(subkey);
					}

					if key_is(subkey, "lenIV") {
						len_iv = subvalue;
					}

					if key_is(subkey, "OtherSubrs") {
						// XXX: assert that no flex hint is used
						lines.push(self.tobytes(HINTOTHERS)?);
					} else if key_is(subkey, "Subrs") {
						let subrs = compiled(subvalue)?;
						lines.push(format!("/Subrs {} array", subrs.len()).into_bytes());
						for (i, subr) in subrs.iter().enumerate() {
							let mut plain = head(CHAR_IV, iv_len(len_iv)?).to_vec();
							plain.extend_from_slice(subr);
							let (encrypted, _) = eexec::encrypt(&plain, 4330);
							let mut line = self.tobytes(&format!(
								"dup {i} {} {} ",
								encrypted.len(),
								opt(rd_key)
							))?;
							line.extend(encrypted);
							line.extend(self.tobytes(&format!(" {}", opt(np_key)))?);
							lines.push(line);
						}
						lines.push(b"def".to_vec());
						lines.push(b"put".to_vec());
					} else {
						lines.extend(self.make_lines(subkey, subvalue)?);
					}
				}
			} else if key_is(key, "CharStrings") {
				let char_strings = value.as_dict().ok_or_else(|| {
					py_err("AttributeError: 'CharStrings' has no attribute 'items'")
				})?;
				lines.push(b"dup /CharStrings".to_vec());
				lines.push(self.tobytes(&format!("{} dict dup begin", char_strings.len()))?);
				for (glyph_name, char_bin) in char_strings {
					let bytecode = compiled_one(char_bin)?;
					let mut plain = head(CHAR_IV, iv_len(len_iv)?).to_vec();
					plain.extend_from_slice(&bytecode);
					let (encrypted, _) = eexec::encrypt(&plain, 4330);
					let mut line = self.tobytes(&format!(
						"/{glyph_name} {} {} ",
						encrypted.len(),
						opt(rd_key)
					))?;
					line.extend(encrypted);
					line.extend(self.tobytes(&format!(" {}", opt(nd_key)))?);
					lines.push(line);
				}
				lines.push(b"end put".to_vec());
			} else {
				lines.extend(self.make_lines(key, value)?);
			}
		}
		lines.extend([
			b"end".to_vec(),
			b"dup /FontName get exch definefont pop".to_vec(),
			b"mark".to_vec(),
			b"currentfile closefile\n".to_vec(),
		]);
		let mut plain = EEXEC_IV.to_vec();
		plain.extend(lines.join(&b"\n"[..]));
		let (encrypted, _) = eexec::encrypt(&plain, 55665);
		Ok(encrypted)
	}

	/// `_make_lines`: the PostScript that defines `key`.
	fn make_lines(&self, key: &Key, value: &Value) -> Result<Vec<Vec<u8>>> {
		if key_is(key, "FontName") {
			return Ok(vec![
				self.tobytes(&format!("/{key} /{} def", value.to_py_string()))?,
			]);
		}
		let bool_str = |v: &Value| if v.is_truthy() { "true" } else { "false" };
		if ["isFixedPitch", "ForceBold", "RndStemUp"]
			.iter()
			.any(|k| key_is(key, k))
		{
			return Ok(vec![
				self.tobytes(&format!("/{key} {} def", bool_str(value)))?,
			]);
		}
		if key_is(key, "Encoding") {
			let std = &fontoxide_encodings::standard_encoding::STANDARD_ENCODING;
			let is_standard = matches!(value, Value::List(l) if l.len() == 256
				&& l.iter().zip(std.iter()).all(|(v, s)| v.as_str() == Some(s)));
			if is_standard {
				return Ok(vec![self.tobytes(&format!("/{key} StandardEncoding def"))?]);
			}
			// follow fontTools.misc.psOperators._type1_Encoding_repr
			let Value::List(names) = value else {
				return Err(py_err("TypeError: the Encoding is not a list"));
			};
			let mut lines = vec![
				b"/Encoding 256 array".to_vec(),
				b"0 1 255 {1 index exch /.notdef put} for".to_vec(),
			];
			for i in 0..256 {
				let name = names
					.get(i)
					.ok_or_else(|| py_err("IndexError: list index out of range"))?;
				if name.as_str() != Some(".notdef") {
					lines.push(self.tobytes(&format!("dup {i} /{} put", name.to_py_string()))?);
				}
			}
			lines.push(b"def".to_vec());
			return Ok(lines);
		}
		let join = |l: &[Value]| {
			l.iter()
				.map(Value::to_py_string)
				.collect::<Vec<_>>()
				.join(" ")
		};
		let line = match value {
			Value::Str(s) => format!("/{key} ({s}) def"),
			Value::Bool(_) => format!("/{key} {} def", bool_str(value)),
			Value::List(l) => format!("/{key} [{}] def", join(l)),
			Value::Tuple(l) => format!("/{key} {{{}}} def", join(l)),
			other => format!("/{key} {} def", other.to_py_string()),
		};
		Ok(vec![self.tobytes(&line)?])
	}
}

fn iv_len(v: &Value) -> Result<i64> {
	match v {
		Value::Int(i) => Ok(*i),
		Value::Bool(b) => Ok(*b as i64),
		_ => Err(py_err("TypeError: slice indices must be integers")),
	}
}

/// Whether a value is the procedure (a tuple of names) `names`.
fn is_procedure(value: &Value, names: &[&str]) -> bool {
	matches!(value, Value::Tuple(t) if t.len() == names.len()
		&& t.iter().zip(names).all(|(v, n)| v.as_str() == Some(n)))
}

/// `charstring.compile(); charstring.bytecode`.
fn compiled_one(v: &Value) -> Result<Vec<u8>> {
	match v {
		Value::CharString(cs) => {
			let mut cs = cs.clone();
			cs.compile()?;
			Ok(cs.bytecode.unwrap_or_default())
		}
		_ => Err(py_err("AttributeError: object has no attribute 'compile'")),
	}
}

fn compiled(v: &Value) -> Result<Vec<Vec<u8>>> {
	match v {
		Value::List(l) => l.iter().map(compiled_one).collect(),
		_ => Err(py_err("TypeError: 'Subrs' is not iterable")),
	}
}

/// The glyph set of a [`T1Font`] (`getGlyphSet`): the charstrings by name, and
/// the subroutines they call.
#[derive(Debug)]
pub struct T1GlyphSet<'a> {
	font: &'a mut Dict,
}

impl<'a> T1GlyphSet<'a> {
	fn new(font: &'a mut Dict) -> Result<Self> {
		if !matches!(font.get(&Key::from("CharStrings")), Some(Value::Dict(_))) {
			return Err(py_err("KeyError: 'CharStrings'"));
		}
		Ok(T1GlyphSet { font })
	}

	fn char_strings(&self) -> Option<&Dict> {
		self.font.get(&Key::from("CharStrings"))?.as_dict()
	}

	/// The glyph names, in the order of the `CharStrings` dictionary.
	pub fn keys(&self) -> Vec<String> {
		self.char_strings()
			.map(|d| d.keys().map(Key::to_string).collect())
			.unwrap_or_default()
	}

	/// The number of glyphs.
	pub fn len(&self) -> usize {
		self.char_strings().map_or(0, Dict::len)
	}

	/// Whether there are no glyphs.
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// The charstring of a glyph.
	pub fn get(&self, name: &str) -> Option<&T1CharString> {
		match self.char_strings()?.get(&Key::from(name))? {
			Value::CharString(cs) => Some(cs),
			_ => None,
		}
	}

	/// The charstring of a glyph, to edit it.
	pub fn get_mut(&mut self, name: &str) -> Option<&mut T1CharString> {
		match self
			.font
			.get_mut(&Key::from("CharStrings"))?
			.as_dict_mut()?
			.get_mut(&Key::from(name))?
		{
			Value::CharString(cs) => Some(cs),
			_ => None,
		}
	}

	/// Draws a glyph (`glyphSet[name].draw(pen)`) and returns its width, which
	/// is also set on the charstring.
	pub fn draw(&mut self, name: &str, pen: &mut dyn Pen) -> Result<f64> {
		let key = Key::from(name);
		let mut cs = match self.get_mut(name) {
			Some(cs) => std::mem::take(cs),
			None => return Err(py_err(format!("KeyError: {}", py::str_repr(name)))),
		};
		let mut subrs: Vec<T1CharString> = Vec::new();
		let subrs_slot = self
			.font
			.get_mut(&Key::from("Private"))
			.and_then(Value::as_dict_mut)
			.and_then(|p| p.get_mut(&Key::from("Subrs")));
		if let Some(Value::List(list)) = subrs_slot {
			for v in list.iter_mut() {
				match v {
					Value::CharString(s) => subrs.push(std::mem::take(s)),
					_ => subrs.push(T1CharString::default()),
				}
			}
		}
		let result = cs.draw(pen, &mut subrs);
		// Put the (now decompiled) charstrings back.
		if let Some(Value::List(list)) = self
			.font
			.get_mut(&Key::from("Private"))
			.and_then(Value::as_dict_mut)
			.and_then(|p| p.get_mut(&Key::from("Subrs")))
		{
			for (v, s) in list.iter_mut().zip(subrs) {
				if let Value::CharString(slot) = v {
					*slot = s;
				}
			}
		}
		let width = cs.width;
		if let Some(Value::CharString(slot)) = self
			.font
			.get_mut(&Key::from("CharStrings"))
			.and_then(Value::as_dict_mut)
			.and_then(|d| d.get_mut(&key))
		{
			*slot = cs;
		}
		result?;
		Ok(width.unwrap_or(0.0))
	}
}

impl fontoxide_pens::GlyphSet for T1GlyphSet<'_> {
	/// Draws a glyph without changing the font: a copy of the charstring and
	/// the subroutines is decompiled and drawn.
	fn draw_glyph(&self, glyph_name: &str, pen: &mut dyn Pen) -> fontoxide_pens::Result<bool> {
		let Some(cs) = self.get(glyph_name) else {
			return Ok(false);
		};
		let mut cs = cs.clone();
		let mut subrs: Vec<T1CharString> = match self
			.font
			.get(&Key::from("Private"))
			.and_then(Value::as_dict)
			.and_then(|p| p.get(&Key::from("Subrs")))
		{
			Some(Value::List(list)) => list
				.iter()
				.map(|v| match v {
					Value::CharString(s) => s.clone(),
					_ => T1CharString::default(),
				})
				.collect(),
			_ => Vec::new(),
		};
		cs.draw(pen, &mut subrs)
			.map_err(|e| fontoxide_pens::Error::Glyph(Box::new(e)))?;
		Ok(true)
	}
}

// low level T1 data read and write functions

/// Reads any Type 1 font file and returns the raw data and its kind, `"LWFN"`,
/// `"PFB"` or `"OTHER"` (`read`).
///
/// fontTools tells LWFN files by their Finder file type, which it reads with
/// the `xattr` module on macOS only; this port does not read extended
/// attributes, so LWFN files must be read with [`read_lwfn`].
pub fn read(path: impl AsRef<Path>, only_header: bool) -> Result<(Vec<u8>, &'static str)> {
	let path = path.as_ref();
	let ext = path
		.extension()
		.map(|e| e.to_string_lossy().to_lowercase())
		.unwrap_or_default();
	if ext == "pfb" {
		Ok((read_pfb(path, only_header)?, "PFB"))
	} else {
		Ok((read_other(path)?, "OTHER"))
	}
}

/// Writes raw Type 1 data (`write`). `kind` is `"LWFN"`, `"PFB"` or `"OTHER"`
/// (PFA); `dohex` writes the encrypted part of `"OTHER"` in hexadecimal.
pub fn write(path: impl AsRef<Path>, data: &[u8], kind: &str, dohex: bool) -> Result<()> {
	let path = path.as_ref();
	assert_type1(data)?;
	let kind = kind.to_uppercase();
	let _ = std::fs::remove_file(path);
	let result = match kind.as_str() {
		"LWFN" => write_lwfn(path, data),
		"PFB" => write_pfb(path, data),
		_ => write_other(path, data, dohex),
	};
	if result.is_err() {
		let _ = std::fs::remove_file(path);
	}
	result
}

const HEX_LINE_LENGTH: usize = 80;

/// Reads an LWFN font file (the `POST` resources of its resource fork)
/// (`readLWFN`).
pub fn read_lwfn(path: impl AsRef<Path>, only_header: bool) -> Result<Vec<u8>> {
	let path = path.as_ref();
	let reader = fontoxide_misc::mac_res::ResourceReader::open(path)?;
	let mut data = Vec::new();
	for res in reader.get("POST").unwrap_or_default() {
		let code = *res
			.data
			.first()
			.ok_or_else(|| py_err("IndexError: index out of range"))?;
		let second = *res
			.data
			.get(1)
			.ok_or_else(|| py_err("IndexError: index out of range"))?;
		if second != 0 {
			return Err(T1Error::T1("corrupt LWFN file".into()));
		}
		match code {
			1 | 2 => {
				if only_header && code == 2 {
					break;
				}
				data.extend_from_slice(&res.data[2..]);
			}
			3 | 5 => break,
			4 => data.extend(std::fs::read(path)?),
			0 => {} // comment, ignore
			_ => return Err(T1Error::T1(format!("bad chunk code: {code}"))),
		}
	}
	assert_type1(&data)?;
	Ok(data)
}

/// Reads a PFB font file (`readPFB`).
pub fn read_pfb(path: impl AsRef<Path>, only_header: bool) -> Result<Vec<u8>> {
	let file = std::fs::read(path)?;
	let mut pos = 0;
	let mut read = |n: usize| -> &[u8] {
		let start = pos.min(file.len());
		let end = (pos + n).min(file.len());
		pos = end.max(pos);
		&file[start..end]
	};
	let mut data = Vec::new();
	loop {
		if read(1) != [128] {
			return Err(T1Error::T1("corrupt PFB file".into()));
		}
		let code = *read(1).first().ok_or_else(|| {
			py_err("TypeError: ord() expected a character, but string of length 0 found")
		})?;
		match code {
			1 | 2 => {
				let len = read(4);
				if len.len() != 4 {
					return Err(py_err("ValueError: string must be 4 bytes long"));
				}
				let chunklen = string_to_long(len)? as usize;
				let chunk = read(chunklen);
				if chunk.len() != chunklen {
					return Err(T1Error::Python(
						"AssertionError: len(chunk) == chunklen".into(),
					));
				}
				data.extend_from_slice(chunk);
			}
			3 => break,
			_ => return Err(T1Error::T1(format!("bad chunk code: {code}"))),
		}
		if only_header {
			break;
		}
	}
	assert_type1(&data)?;
	Ok(data)
}

/// Reads any (font) file, with the encrypted part in binary (`readOther`).
pub fn read_other(path: impl AsRef<Path>) -> Result<Vec<u8>> {
	let data = std::fs::read(path)?;
	assert_type1(&data)?;
	let mut out = Vec::new();
	for (is_encrypted, chunk) in find_encrypted_chunks(&data)? {
		if is_encrypted && is_hex(head(&chunk, 4)) {
			out.extend(de_hex_string(&chunk)?);
		} else {
			out.extend(chunk);
		}
	}
	Ok(out)
}

// file writing tools

/// `writeLWFN`: fontTools writes resource forks with the Carbon `Res` module
/// of classic MacPython, which no longer exists, so it always fails.
fn write_lwfn(_path: &Path, _data: &[u8]) -> Result<()> {
	Err(py_err("NameError: name 'Res' is not defined"))
}

fn write_pfb(path: &Path, data: &[u8]) -> Result<()> {
	let mut out = Vec::new();
	for (is_encrypted, chunk) in find_encrypted_chunks(data)? {
		out.push(128);
		out.push(if is_encrypted { 2 } else { 1 });
		out.extend(long_to_string(chunk.len() as u64));
		out.extend(chunk);
	}
	out.extend([128, 3]);
	std::fs::write(path, out)?;
	Ok(())
}

fn write_other(path: &Path, data: &[u8], dohex: bool) -> Result<()> {
	let mut out = Vec::new();
	let hexlinelen = HEX_LINE_LENGTH / 2;
	for (is_encrypted, chunk) in find_encrypted_chunks(data)? {
		if is_encrypted && dohex {
			for line in chunk.chunks(hexlinelen) {
				out.extend(eexec::hex_string(line));
				out.push(b'\r');
			}
		} else {
			out.extend(chunk);
		}
	}
	std::fs::write(path, out)?;
	Ok(())
}

// decryption tools

const EEXEC_BEGIN: &[u8] = b"currentfile eexec";
const EEXEC_INTERNAL_END: &[u8] = b"currentfile closefile";
const EEXEC_BEGIN_MARKER: &[u8] = b"%-- eexec start\r";
const EEXEC_END_MARKER: &[u8] = b"%-- eexec end\r";

/// `isHex`: `[0-9A-Fa-f]*$` matches (`$` also matches before a final newline).
pub fn is_hex(text: &[u8]) -> bool {
	let body = text.strip_suffix(b"\n").unwrap_or(text);
	body.iter().all(u8::is_ascii_hexdigit)
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
	haystack
		.get(from..)?
		.windows(needle.len())
		.position(|w| w == needle)
		.map(|p| p + from)
}

/// `EEXECEND.search(data, start)`: the start of 512 ASCII zeros interrupted by
/// arbitrary whitespace (`(0[ \t\r\n]*){512}`).
fn find_eexec_end(data: &[u8], start: usize) -> Option<usize> {
	let is_ws = |b: u8| matches!(b, b' ' | b'\t' | b'\r' | b'\n');
	// The number of zeros in the chain from each position, from the end.
	let n = data.len();
	let mut count = vec![0usize; n + 1];
	let mut next_non_ws = n;
	for i in (start.min(n)..n).rev() {
		if data[i] == b'0' {
			count[i] = 1 + count[next_non_ws];
		}
		if !is_ws(data[i]) {
			next_non_ws = i;
		}
	}
	(start.min(n)..n).find(|&i| count[i] >= 512)
}

/// Decrypts the `eexec` parts of Type 1 data, with markers around them
/// (`decryptType1`).
pub fn decrypt_type1(data: &[u8]) -> Result<Vec<u8>> {
	let mut out = Vec::new();
	for (is_encrypted, chunk) in find_encrypted_chunks(data)? {
		if is_encrypted {
			let chunk = if is_hex(head(&chunk, 4)) {
				de_hex_string(&chunk)?
			} else {
				chunk
			};
			let (decrypted, _) = eexec::decrypt(&chunk, 55665);
			let decrypted = tail(&decrypted, 4);
			let n = decrypted.len() as i64;
			let end = EEXEC_INTERNAL_END.len() as i64;
			let slice = |a: i64, b: i64| -> &[u8] {
				let a = if a < 0 { (n + a).max(0) } else { a.min(n) } as usize;
				let b = if b < 0 { (n + b).max(0) } else { b.min(n) } as usize;
				&decrypted[a..b.max(a)]
			};
			if slice(-end - 1, -1) != EEXEC_INTERNAL_END
				&& slice(-end - 2, -2) != EEXEC_INTERNAL_END
			{
				return Err(T1Error::T1("invalid end of eexec part".into()));
			}
			out.extend_from_slice(EEXEC_BEGIN_MARKER);
			out.extend_from_slice(slice(0, -end - 2));
			out.push(b'\r');
			out.extend_from_slice(EEXEC_END_MARKER);
		} else {
			let n = chunk.len() as i64;
			let end = EEXEC_BEGIN.len() as i64;
			let a = (n - end - 1).max(0) as usize;
			let b = (n - 1).max(0) as usize;
			if chunk.get(a..b.max(a)) == Some(EEXEC_BEGIN) {
				out.extend_from_slice(&chunk[..a]);
			} else {
				out.extend(chunk);
			}
		}
	}
	Ok(out)
}

/// Splits Type 1 data into plain and `eexec` encrypted chunks
/// (`findEncryptedChunks`); encrypted chunks are binary.
pub fn find_encrypted_chunks(data: &[u8]) -> Result<Vec<(bool, Vec<u8>)>> {
	let mut chunks = Vec::new();
	let mut data = data.to_vec();
	while let Some(e_begin) = find(&data, EEXEC_BEGIN, 0) {
		let e_begin = e_begin + EEXEC_BEGIN.len() + 1;
		let e_end = find_eexec_end(&data, e_begin)
			.ok_or_else(|| T1Error::T1("can't find end of eexec part".into()))?;
		let mut cypher_text = data[e_begin.min(data.len())..(e_end + 2).min(data.len())].to_vec();
		if is_hex(head(&cypher_text, 4)) {
			cypher_text = de_hex_string(&cypher_text)?;
		}
		let (plain_text, _) = eexec::decrypt(&cypher_text, 55665);
		let e_end_local = find(&plain_text, EEXEC_INTERNAL_END, 0)
			.ok_or_else(|| T1Error::T1("can't find end of eexec part".into()))?;
		chunks.push((false, data[..e_begin.min(data.len())].to_vec()));
		let cut = (e_end_local + EEXEC_INTERNAL_END.len() + 1).min(cypher_text.len());
		chunks.push((true, cypher_text[..cut].to_vec()));
		data = data[e_end..].to_vec();
	}
	chunks.push((false, data));
	Ok(chunks)
}

/// `deHexString`.
pub fn de_hex_string(hexstring: &[u8]) -> Result<Vec<u8>> {
	Ok(eexec::de_hex_string(hexstring)?)
}

/// Checks that the data is an encrypted Type 1 font (`assertType1`).
pub fn assert_type1(data: &[u8]) -> Result<()> {
	if !(data.starts_with(b"%!PS-AdobeFont") || data.starts_with(b"%!FontType1")) {
		return Err(T1Error::T1("not a PostScript font".into()));
	}
	if !has_font_type_1(data) {
		return Err(T1Error::T1("not a Type 1 font".into()));
	}
	if find(data, b"currentfile eexec", 0).is_none() {
		return Err(T1Error::T1("not an encrypted Type 1 font".into()));
	}
	// XXX what else?
	Ok(())
}

/// `_fontType1RE.search(data)`: `/FontType\s+1\s+def`.
fn has_font_type_1(data: &[u8]) -> bool {
	let is_space = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C);
	let mut from = 0;
	while let Some(at) = find(data, b"/FontType", from) {
		from = at + 1;
		let mut i = at + 9;
		let s = i;
		while i < data.len() && is_space(data[i]) {
			i += 1;
		}
		if i == s || data.get(i) != Some(&b'1') {
			continue;
		}
		i += 1;
		let s = i;
		while i < data.len() && is_space(data[i]) {
			i += 1;
		}
		if i > s && data[i..].starts_with(b"def") {
			return true;
		}
	}
	false
}

// pfb helpers

/// `longToString`: 4 bytes, little-endian.
pub fn long_to_string(long: u64) -> [u8; 4] {
	(long as u32).to_le_bytes()
}

/// `stringToLong`: 4 bytes, little-endian.
pub fn string_to_long(s: &[u8]) -> Result<u32> {
	let b: [u8; 4] = s
		.try_into()
		.map_err(|_| py_err("ValueError: string must be 4 bytes long"))?;
	Ok(u32::from_le_bytes(b))
}

// PS stream helpers

/// `font_dictionary_keys`: the keys before `eexec`, except `FontMatrix`, which
/// is always counted.
fn font_dictionary_keys() -> impl Iterator<Item = &'static str> {
	TYPE1_PRE_EEXEC_ORDER
		.iter()
		.copied()
		.filter(|k| *k != "FontMatrix")
}

/// `FontInfo_dictionary_keys`: with the keys AFDKO `tx` may use.
fn fontinfo_dictionary_keys() -> impl Iterator<Item = &'static str> {
	TYPE1_FONTINFO_ORDER
		.iter()
		.copied()
		.chain(["FSType", "Copyright"])
}

/// `Private_dictionary_keys`.
const PRIVATE_DICTIONARY_KEYS: [&str; 20] = [
	"Subrs",
	"OtherSubrs",
	"UniqueID",
	"BlueValues",
	"OtherBlues",
	"FamilyBlues",
	"FamilyOtherBlues",
	"BlueScale",
	"BlueShift",
	"BlueFuzz",
	"StdHW",
	"StdVW",
	"StemSnapH",
	"StemSnapV",
	"ForceBold",
	"LanguageGroup",
	"password",
	"lenIV",
	"MinFeature",
	"RndStemUp",
];

/// t1write_hintothers.h (`hintothers`).
const HINTOTHERS: &str = "/OtherSubrs[{}{}{}{systemdict/internaldict known not{pop 3}{1183615869
systemdict/internaldict get exec dup/startlock known{/startlock get exec}{dup
/strtlck known{/strtlck get exec}{pop 3}ifelse}ifelse}ifelse}executeonly]def";

/// follow t1write.c:writeRegNameKeyedFont (`eexec_IV`).
const EEXEC_IV: &[u8] = b"cccc";
/// `char_IV`.
const CHAR_IV: &[u8] = b"\x0c\x0c\x0c\x0c";
/// `RD_value`.
const RD_VALUE: &[&str] = &["string", "currentfile", "exch", "readstring", "pop"];
/// `ND_values`.
const ND_VALUES: &[&[&str]] = &[&["def"], &["noaccess", "def"]];
/// `PD_values`.
const PD_VALUES: &[&[&str]] = &[&["put"], &["noaccess", "put"]];
