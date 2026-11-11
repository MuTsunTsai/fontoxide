//! Port of `fontTools.tfmLib`: reading TFM (TeX Font Metrics) files.
//!
//! The TFM format is described in the TFtoPL WEB source code, whose typeset
//! form can be found on [CTAN](http://mirrors.ctan.org/info/knuth-pdf/texware/tftopl.pdf).
//!
//! All dimensions are relative to the design size. Like fontTools, the reader
//! only checks the file structure: it reads files whose data make no sense as
//! long as they are parsable.

use std::fmt::Write as _;
use std::path::Path;

use fontoxide_misc::py;
use indexmap::IndexMap;
use thiserror::Error;

/// Errors of this crate.
#[derive(Debug, Error)]
pub enum Error {
	/// `TFMException`.
	#[error("{0}")]
	Tfm(String),
	/// The file could not be read.
	#[error(transparent)]
	Io(#[from] std::io::Error),
	/// Other Python exceptions fontTools raises on bad input (`struct.error`,
	/// `IndexError`, `TypeError`, `NameError`).
	#[error("{0}")]
	Python(String),
}

/// The result type of this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// The `fonttype` of a font without special parameters.
pub const VANILLA: u8 = 0;
/// The `fonttype` of math symbol fonts (coding scheme `TeX math sy...`).
pub const MATHSY: u8 = 1;
/// The `fonttype` of math extension fonts (coding scheme `TeX math ex...`).
pub const MATHEX: u8 = 2;

const BASE_PARAMS: [&str; 7] = [
	"SLANT",
	"SPACE",
	"STRETCH",
	"SHRINK",
	"XHEIGHT",
	"QUAD",
	"EXTRASPACE",
];

const MATHSY_PARAMS: [&str; 15] = [
	"NUM1",
	"NUM2",
	"NUM3",
	"DENOM1",
	"DENOM2",
	"SUP1",
	"SUP2",
	"SUP3",
	"SUB1",
	"SUB2",
	"SUPDROP",
	"SUBDROP",
	"DELIM1",
	"DELIM2",
	"AXISHEIGHT",
];

const MATHEX_PARAMS: [&str; 6] = [
	"DEFAULTRULETHICKNESS",
	"BIGOPSPACING1",
	"BIGOPSPACING2",
	"BIGOPSPACING3",
	"BIGOPSPACING4",
	"BIGOPSPACING5",
];

const NO_TAG: u8 = 0;
const LIG_TAG: u8 = 1;
const LIST_TAG: u8 = 2;
const EXT_TAG: u8 = 3;

const STOP_FLAG: u8 = 128;
const KERN_FLAG: u8 = 128;

/// A string of the header: text if it is ASCII, otherwise the bytes, as
/// `sstruct` leaves them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderString {
	/// A `str`.
	Str(String),
	/// A `bytes`.
	Bytes(Vec<u8>),
}

impl std::fmt::Display for HeaderString {
	/// `str(value)`.
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			HeaderString::Str(s) => f.write_str(s),
			HeaderString::Bytes(b) => f.write_str(&py::bytes_repr(b)),
		}
	}
}

/// The face code: a number, or its three-letter name (e.g. `"MRR"`) for codes
/// below 18.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Face {
	/// A face code of 18 or more, or 0 when the header is too short to have it.
	Int(u8),
	/// The name of a face code below 18.
	Str(String),
}

/// A ligature operation: the TFtoPL name (`"LIG"`, `"/LIG"`, `"LIG/>"`, ...), or
/// the code of a nonstandard one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LigOp {
	/// The name.
	Str(String),
	/// A nonstandard code.
	Code(u8),
}

/// The metrics of a character (`tfm.chars[c]`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CharInfo {
	/// `width`.
	pub width: f64,
	/// `height`, if not zero.
	pub height: Option<f64>,
	/// `depth`, if not zero.
	pub depth: Option<f64>,
	/// `italic`, if not zero.
	pub italic: Option<f64>,
	/// `nextlarger`: the next larger character.
	pub nextlarger: Option<u8>,
	/// `varchar`: the pieces of an extensible character (`top`, `mid`, `bot`,
	/// `rep`), in that order.
	pub varchar: Option<IndexMap<&'static str, i64>>,
}

impl CharInfo {
	/// `repr()` of the Python dictionary.
	pub fn repr(&self) -> String {
		let mut items = vec![format!("'width': {}", py::float_repr(self.width))];
		for (name, v) in [
			("height", self.height),
			("depth", self.depth),
			("italic", self.italic),
		] {
			if let Some(v) = v {
				items.push(format!("'{name}': {}", py::float_repr(v)));
			}
		}
		if let Some(n) = self.nextlarger {
			items.push(format!("'nextlarger': {n}"));
		}
		if let Some(v) = &self.varchar {
			let inner: Vec<String> = v.iter().map(|(k, v)| format!("'{k}': {v}")).collect();
			items.push(format!("'varchar': {{{}}}", inner.join(", ")));
		}
		format!("{{{}}}", items.join(", "))
	}
}

/// A TFM file (`TFM`).
#[derive(Debug, Clone, PartialEq)]
pub struct Tfm {
	/// `checksum`.
	pub checksum: u32,
	/// `designsize`, in points.
	pub designsize: f64,
	/// `codingscheme`.
	pub codingscheme: String,
	/// `family`.
	pub family: HeaderString,
	/// `seven_bit_safe_flag`.
	pub seven_bit_safe_flag: bool,
	/// `face`.
	pub face: Face,
	/// `extraheader`: the header words after the standard ones, as
	/// `HEADER18`...
	pub extraheader: IndexMap<String, i32>,
	/// `fonttype`: [`VANILLA`], [`MATHSY`] or [`MATHEX`].
	pub fonttype: u8,
	/// `fontdimens`: the font parameters by name.
	pub fontdimens: IndexMap<String, f64>,
	/// `right_boundary_char`.
	pub right_boundary_char: Option<u8>,
	/// `left_boundary_char`: 256 if there is a left boundary program.
	pub left_boundary_char: Option<u16>,
	/// `chars`: the metrics by character code.
	pub chars: IndexMap<u16, CharInfo>,
	/// `kerning`: by left character (256 for the left boundary), then right
	/// character.
	pub kerning: IndexMap<u16, IndexMap<u8, f64>>,
	/// `ligatures`: by left character (256 for the left boundary), then right
	/// character, the operation and the ligature character.
	pub ligatures: IndexMap<u16, IndexMap<u8, (LigOp, u8)>>,
}

fn tfm_err(msg: impl Into<String>) -> Error {
	Error::Tfm(msg.into())
}

fn struct_error(n: usize) -> Error {
	Error::Python(format!(
		"struct.error: unpack requires a buffer of {n} bytes"
	))
}

fn index_error() -> Error {
	Error::Python("IndexError: index out of range".into())
}

/// The data, read the way `sstruct` and indexing read it.
struct Data<'a>(&'a [u8]);

impl Data<'_> {
	fn byte(&self, i: i64) -> Result<u8> {
		usize::try_from(i)
			.ok()
			.and_then(|i| self.0.get(i).copied())
			.ok_or_else(index_error)
	}

	fn bytes(&self, i: i64, n: usize) -> Result<&[u8]> {
		usize::try_from(i)
			.ok()
			.and_then(|i| self.0.get(i..i.checked_add(n)?))
			.ok_or_else(|| struct_error(n))
	}

	/// A 12.20 fixed number (`read_fixed`).
	fn fixed(&self, i: i64) -> Result<f64> {
		let b = self.bytes(i, 4)?;
		Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64 / (1 << 20) as f64)
	}

	/// The four bytes of a lig/kern command.
	fn lig_kern_command(&self, i: i64) -> Result<[u8; 4]> {
		let b = self.bytes(i, 4)?;
		Ok([b[0], b[1], b[2], b[3]])
	}
}

/// A Pascal string of `size` bytes (`struct` format `p`).
fn pascal(b: &[u8]) -> HeaderString {
	let n = (b[0] as usize).min(b.len() - 1);
	let s = &b[1..1 + n];
	if s.is_ascii() {
		HeaderString::Str(String::from_utf8_lossy(s).into_owned())
	} else {
		HeaderString::Bytes(s.to_vec())
	}
}

const SIZE_NAMES: [&str; 12] = [
	"lf", "lh", "bc", "ec", "nw", "nh", "nd", "ni", "nl", "nk", "ne", "np",
];

impl Tfm {
	/// Reads a TFM file (`TFM(path)`).
	pub fn open(path: impl AsRef<Path>) -> Result<Self> {
		Self::from_bytes(&std::fs::read(path)?)
	}

	/// Reads TFM data (`TFM(file)`).
	pub fn from_bytes(data: &[u8]) -> Result<Self> {
		if data.len() < 24 {
			return Err(tfm_err("Too short input file"));
		}
		let mut sizes = [0i64; 12];
		for (i, s) in sizes.iter_mut().enumerate() {
			*s = i16::from_be_bytes([data[2 * i], data[2 * i + 1]]) as i64;
		}
		let [lf, lh, bc, ec, nw, nh, nd, ni, nl, nk, ne, np] = sizes;

		// Do some file structure sanity checks. TeX and TFtoPL do additional
		// functional checks and might even correct "errors" in the input file,
		// but we instead try to output the file as it is as long as it is
		// parsable, even if the data make no sense.
		if lf < 0 {
			return Err(tfm_err("The file claims to have negative or zero length!"));
		}
		if (data.len() as i64) < lf * 4 {
			return Err(tfm_err("The file has fewer bytes than it claims!"));
		}
		for (_name, length) in SIZE_NAMES.iter().zip(sizes) {
			if length < 0 {
				// UPSTREAM-BUG(B64): the message is not an f-string, so the name is
				// not filled in.
				return Err(tfm_err("The subfile size: '{name}' is negative!"));
			}
		}
		if lh < 2 {
			return Err(tfm_err(format!("The header length is only {lh}!")));
		}
		if bc > ec + 1 || ec > 255 {
			return Err(tfm_err(format!(
				"The character code range {bc}..{ec} is illegal!"
			)));
		}
		if nw == 0 || nh == 0 || nd == 0 || ni == 0 {
			return Err(tfm_err("Incomplete subfiles for character dimensions!"));
		}
		if ne > 256 {
			// UPSTREAM-BUG(B65): the message refers to an undefined variable.
			return Err(Error::Python("NameError: name 'ne' is not defined".into()));
		}
		if lf != 6 + lh + (ec - bc + 1) + nw + nh + nd + ni + nl + nk + ne + np {
			return Err(tfm_err(
				"Subfile sizes don\u{2019}t add up to the stated total",
			));
		}

		// Subfile offsets, in 32-bit words.
		let char_base = 6 + lh - bc;
		let width_base = char_base + ec + 1;
		let height_base = width_base + nw;
		let depth_base = height_base + nh;
		let italic_base = depth_base + nd;
		let lig_kern_base = italic_base + ni;
		let kern_base = lig_kern_base + nl;
		let exten_base = kern_base + nk;
		let param_base = exten_base + ne;

		let d = Data(data);
		let char_info = |c: i64| 4 * (char_base + c);
		let width_index = |c: i64| d.byte(char_info(c));
		let noneexistent =
			|c: i64| -> Result<bool> { Ok(c < bc || c > ec || width_index(c)? == 0) };
		let height_index = |c: i64| -> Result<u8> { Ok(d.byte(char_info(c) + 1)? / 16) };
		let depth_index = |c: i64| -> Result<u8> { Ok(d.byte(char_info(c) + 1)? % 16) };
		let italic_index = |c: i64| -> Result<u8> { Ok(d.byte(char_info(c) + 2)? / 4) };
		let tag = |c: i64| -> Result<u8> { Ok(d.byte(char_info(c) + 2)? % 4) };
		let remainder = |c: i64| d.byte(char_info(c) + 3);
		let lig_step = |i: i64| 4 * (lig_kern_base + i);

		// All attributes start with empty values regardless of the header size.
		let mut tfm = Tfm {
			checksum: 0,
			designsize: 0.0,
			codingscheme: String::new(),
			family: HeaderString::Str(String::new()),
			seven_bit_safe_flag: false,
			face: Face::Int(0),
			extraheader: IndexMap::new(),
			fonttype: VANILLA,
			fontdimens: IndexMap::new(),
			right_boundary_char: None,
			left_boundary_char: None,
			chars: IndexMap::new(),
			kerning: IndexMap::new(),
			ligatures: IndexMap::new(),
		};

		let offset = 24;
		let length = lh * 4;
		let mut codingscheme = HeaderString::Str(String::new());
		if length >= 8 {
			let h = d.bytes(
				offset,
				if length >= 72 {
					72
				} else if length >= 68 {
					68
				} else if length >= 48 {
					48
				} else {
					8
				},
			)?;
			tfm.checksum = u32::from_be_bytes([h[0], h[1], h[2], h[3]]);
			tfm.designsize = i32::from_be_bytes([h[4], h[5], h[6], h[7]]) as f64 / (1 << 20) as f64;
			if length >= 48 {
				codingscheme = pascal(&h[8..48]);
			}
			if length >= 68 {
				tfm.family = pascal(&h[48..68]);
			}
			if length >= 72 {
				tfm.seven_bit_safe_flag = h[68] != 0;
				let face = h[71];
				tfm.face = if face < 18 {
					let s = (face % 2) as usize;
					let b = (face / 2) as usize;
					Face::Str(
						[
							"MBL".as_bytes()[b % 3] as char,
							"RI".as_bytes()[s] as char,
							"RCE".as_bytes()[b / 3] as char,
						]
						.iter()
						.collect(),
					)
				} else {
					Face::Int(face)
				};
				for i in 0..(lh - 18).max(0) {
					let b = d.bytes(offset + 72 + 4 * i, 4)?;
					tfm.extraheader.insert(
						format!("HEADER{}", i + 18),
						i32::from_be_bytes([b[0], b[1], b[2], b[3]]),
					);
				}
			}
		}
		tfm.codingscheme = match codingscheme {
			HeaderString::Str(s) => s,
			// `bytes.startswith` with a `str` argument.
			HeaderString::Bytes(_) => {
				return Err(Error::Python(
					"TypeError: startswith first arg must be bytes or a tuple of bytes, not str"
						.into(),
				));
			}
		};

		let scheme = tfm.codingscheme.to_uppercase();
		if scheme.starts_with("TEX MATH SY") {
			tfm.fonttype = MATHSY;
		} else if scheme.starts_with("TEX MATH EX") {
			tfm.fonttype = MATHEX;
		}

		for i in 0..np {
			let name = if i <= 6 {
				BASE_PARAMS[i as usize].to_owned()
			} else if tfm.fonttype == MATHSY && i <= 21 {
				MATHSY_PARAMS[i as usize - 7].to_owned()
			} else if tfm.fonttype == MATHEX && i <= 12 {
				MATHEX_PARAMS[i as usize - 7].to_owned()
			} else {
				format!("PARAMETER{}", i + 1)
			};
			let v = d.fixed(4 * (param_base + i))?;
			tfm.fontdimens.insert(name, v);
		}

		let mut lig_kern_map: IndexMap<i64, i64> = IndexMap::new();
		if nl > 0 {
			let cmd = d.lig_kern_command(lig_step(0))?;
			if cmd[0] == 255 {
				tfm.right_boundary_char = Some(cmd[1]);
			}
			let cmd = d.lig_kern_command(lig_step(nl - 1))?;
			if cmd[0] == 255 {
				tfm.left_boundary_char = Some(256);
				lig_kern_map.insert(256, 256 * cmd[2] as i64 + cmd[3] as i64);
			}
		}

		for c in bc..=ec {
			if width_index(c)? == 0 {
				continue;
			}
			let mut info = CharInfo {
				width: d.fixed(4 * (width_base + width_index(c)? as i64))?,
				..CharInfo::default()
			};
			if height_index(c)? > 0 {
				info.height = Some(d.fixed(4 * (height_base + height_index(c)? as i64))?);
			}
			if depth_index(c)? > 0 {
				info.depth = Some(d.fixed(4 * (depth_base + depth_index(c)? as i64))?);
			}
			if italic_index(c)? > 0 {
				info.italic = Some(d.fixed(4 * (italic_base + italic_index(c)? as i64))?);
			}
			match tag(c)? {
				NO_TAG => {}
				LIG_TAG => {
					lig_kern_map.insert(c, remainder(c)? as i64);
				}
				LIST_TAG => info.nextlarger = Some(remainder(c)?),
				EXT_TAG => {
					let mut varchar = IndexMap::new();
					let exten = 4 * (exten_base + remainder(c)? as i64);
					for i in 0..4 {
						let part = d.byte(exten + i)?;
						if i == 3 || part > 0 {
							let name = match i {
								0 => "top",
								1 => "mid",
								2 => "bot",
								_ => "rep",
							};
							let v = if noneexistent(part as i64)? {
								c
							} else {
								part as i64
							};
							varchar.insert(name, v);
						}
					}
					info.varchar = Some(varchar);
				}
				_ => {}
			}
			// `c` is in `0..=255`.
			tfm.chars.insert(c as u16, info);
		}

		let mut items: Vec<(i64, i64)> = lig_kern_map.into_iter().collect();
		items.sort();
		for (c, mut i) in items {
			let cmd = d.lig_kern_command(lig_step(i))?;
			if cmd[0] > STOP_FLAG {
				i = 256 * cmd[2] as i64 + cmd[3] as i64;
			}
			while i < nl {
				let [skip_byte, next_char, op_byte, rem] = d.lig_kern_command(lig_step(i))?;
				if skip_byte <= STOP_FLAG {
					if op_byte >= KERN_FLAG {
						let r = 256 * (op_byte - KERN_FLAG) as i64 + rem as i64;
						let kern = d.fixed(4 * (kern_base + r))?;
						tfm.kerning
							.entry(c as u16)
							.or_default()
							.insert(next_char, kern);
					} else {
						let mut r = op_byte;
						let lig = if r == 4 || (r > 7 && r != 11) {
							// Ligature step with nonstandard code, we output
							// the code verbatim.
							LigOp::Code(r)
						} else {
							let mut lig = String::new();
							if r % 4 > 1 {
								lig.push('/');
							}
							lig.push_str("LIG");
							if r % 2 != 0 {
								lig.push('/');
							}
							while r > 3 {
								lig.push('>');
								r -= 4;
							}
							LigOp::Str(lig)
						};
						tfm.ligatures
							.entry(c as u16)
							.or_default()
							.insert(next_char, (lig, rem));
					}
				}
				if skip_byte >= STOP_FLAG {
					break;
				}
				i += skip_byte as i64 + 1;
			}
		}
		Ok(tfm)
	}

	/// `repr(tfm)`.
	pub fn repr(&self) -> String {
		format!(
			"<TFM for {} in {} at {}pt>",
			self.family,
			self.codingscheme,
			py::format_g_lower(self.designsize, 6)
		)
	}
}

/// `repr()` of a dictionary of numbers by integer key, as `tfm.kerning[c]`.
pub fn repr_kerning(d: &IndexMap<u8, f64>) -> String {
	let mut s = String::from("{");
	for (i, (k, v)) in d.iter().enumerate() {
		if i > 0 {
			s.push_str(", ");
		}
		let _ = write!(s, "{k}: {}", py::float_repr(*v));
	}
	s.push('}');
	s
}

/// `repr()` of the ligatures of a character, as `tfm.ligatures[c]`.
pub fn repr_ligatures(d: &IndexMap<u8, (LigOp, u8)>) -> String {
	let mut s = String::from("{");
	for (i, (k, (op, r))) in d.iter().enumerate() {
		if i > 0 {
			s.push_str(", ");
		}
		let op = match op {
			LigOp::Str(o) => py::str_repr(o),
			LigOp::Code(c) => c.to_string(),
		};
		let _ = write!(s, "{k}: ({op}, {r})");
	}
	s.push('}');
	s
}

/// `repr()` of the font parameters, as `tfm.fontdimens`.
pub fn repr_fontdimens(d: &IndexMap<String, f64>) -> String {
	let items: Vec<String> = d
		.iter()
		.map(|(k, v)| format!("{}: {}", py::str_repr(k), py::float_repr(*v)))
		.collect();
	format!("{{{}}}", items.join(", "))
}
