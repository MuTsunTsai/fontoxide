//! Text codecs by Python name (`str.encode` / `bytes.decode`), with the error
//! handlers `strict`, `ignore`, `replace`, `backslashreplace` and
//! `xmlcharrefreplace` ([`Errors`]).
//!
//! Supported: `utf_16_be`, `ascii`, `latin1`, the Macintosh single-byte codecs
//! (`mac_roman`, `mac_iceland`, `mac_turkish`, `mac_croatian`, `mac_latin2`,
//! `mac_romanian`, `mac_greek`, `mac_cyrillic`), the CJK codecs (`shift_jis`,
//! `gb2312`, `big5`, `euc_kr`, `johab`) and fontTools' extensions of those for the
//! Macintosh platform (`x_mac_japanese_ttx`, `x_mac_trad_chinese_ttx`,
//! `x_mac_korean_ttx`, `x_mac_simp_chinese_ttx`).

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

/// A decoding or encoding failure (`UnicodeDecodeError` / `UnicodeEncodeError`),
/// or an unknown codec (`LookupError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
	/// The codec name is not known.
	Unknown(String),
	/// The bytes are not valid in the codec; `position` is the offset of the first
	/// byte that could not be decoded.
	Decode {
		/// The codec.
		encoding: String,
		/// Offset of the offending byte.
		position: usize,
	},
	/// A character cannot be encoded; `position` is its index in characters.
	Encode {
		/// The codec.
		encoding: String,
		/// Index of the offending character.
		position: usize,
	},
	/// An unknown error handler (`LookupError`), or one that cannot handle the
	/// error (`TypeError`).
	Handler(String),
}

impl fmt::Display for CodecError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			CodecError::Unknown(name) => write!(f, "unknown encoding: {name}"),
			CodecError::Decode { encoding, position } => {
				write!(
					f,
					"'{encoding}' codec can't decode byte at position {position}"
				)
			}
			CodecError::Encode { encoding, position } => {
				write!(
					f,
					"'{encoding}' codec can't encode character in position {position}"
				)
			}
			CodecError::Handler(message) => f.write_str(message),
		}
	}
}

impl std::error::Error for CodecError {}

/// Normalizes a codec name like `encodings.normalize_encoding` plus the common
/// aliases, e.g. `"UTF-16BE"` -> `"utf_16_be"`, `"latin-1"` -> `"latin1"`.
pub fn normalize(name: &str) -> String {
	let n: String = name
		.trim()
		.to_ascii_lowercase()
		.chars()
		.map(|c| {
			if c.is_ascii_alphanumeric() || c == '.' {
				c
			} else {
				'_'
			}
		})
		.collect();
	match n.as_str() {
		"utf_16be" | "utf16be" | "utf_16_be" | "unicodebigunmarked" => "utf_16_be".into(),
		"latin_1" | "latin1" | "iso_8859_1" | "iso8859_1" | "l1" | "8859" | "cp819" => {
			"latin1".into()
		}
		"ascii" | "us_ascii" | "646" => "ascii".into(),
		"macroman" | "macintosh" => "mac_roman".into(),
		"sjis" | "s_jis" | "shiftjis" | "csshiftjis" => "shift_jis".into(),
		"euckr" | "korean" | "ksc5601" | "ks_c_5601" | "ks_c_5601_1987" | "ksx1001"
		| "ks_x_1001" => "euc_kr".into(),
		"big5_tw" | "csbig5" => "big5".into(),
		"chinese" | "csiso58gb231280" | "euc_cn" | "euccn" | "eucgb2312_cn" | "gb2312_1980"
		| "gb2312_80" | "iso_ir_58" => "gb2312".into(),
		"cp1361" | "ms1361" => "johab".into(),
		// Python aliases these to the base codecs; fontTools extends them as *_ttx.
		"x_mac_japanese" => "shift_jis".into(),
		"x_mac_trad_chinese" => "big5".into(),
		"x_mac_korean" => "euc_kr".into(),
		"x_mac_simp_chinese" => "gb2312".into(),
		_ => n,
	}
}

/// An error handler (the `errors` argument of `bytes.decode` and
/// `str.encode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Errors {
	/// `strict`: fail.
	#[default]
	Strict,
	/// `ignore`: drop what cannot be decoded or encoded.
	Ignore,
	/// `replace`: U+FFFD when decoding, `?` when encoding.
	Replace,
	/// `backslashreplace`: `\xNN` for each byte that cannot be decoded, `\xNN`,
	/// `\uNNNN` or `\UNNNNNNNN` for each character that cannot be encoded.
	BackslashReplace,
	/// `xmlcharrefreplace`: `&#N;` for each character that cannot be encoded.
	/// Encoding only.
	XmlCharRefReplace,
}

impl Errors {
	/// The handler with a Python name (`codecs.lookup_error`).
	pub fn from_name(name: &str) -> Result<Self, CodecError> {
		Ok(match name {
			"strict" => Errors::Strict,
			"ignore" => Errors::Ignore,
			"replace" => Errors::Replace,
			"backslashreplace" => Errors::BackslashReplace,
			"xmlcharrefreplace" => Errors::XmlCharRefReplace,
			_ => {
				return Err(CodecError::Handler(format!(
					"unknown error handler name '{name}'"
				)));
			}
		})
	}
}

/// A codec, looked up by normalized name.
enum Codec {
	Utf16Be,
	Ascii,
	Latin1,
	Single(&'static SingleByte),
	Multi(&'static MultiByte, Option<&'static [(u8, char)]>),
}

fn lookup(name: &str) -> Result<Codec, CodecError> {
	Ok(match name {
		"utf_16_be" => Codec::Utf16Be,
		"ascii" => Codec::Ascii,
		"latin1" => Codec::Latin1,
		_ => {
			if let Some(table) = single_byte(name) {
				Codec::Single(table)
			} else if let Some((base, extra)) = extended(name) {
				let codec = multi_byte(base).ok_or_else(|| CodecError::Unknown(name.into()))?;
				Codec::Multi(codec, Some(extra))
			} else {
				let codec = multi_byte(name).ok_or_else(|| CodecError::Unknown(name.into()))?;
				Codec::Multi(codec, None)
			}
		}
	})
}

/// One step of decoding: a character and the number of bytes it takes, or an
/// error over a number of bytes (CPython's `UnicodeDecodeError.start` to
/// `.end`).
enum Step {
	Char(char, usize),
	Error(usize),
}

impl Codec {
	fn decode_step(&self, data: &[u8], i: usize) -> Step {
		match self {
			Codec::Utf16Be => {
				let rest = data.len() - i;
				if rest < 2 {
					// "truncated data".
					return Step::Error(rest);
				}
				let unit = u16::from_be_bytes([data[i], data[i + 1]]);
				match unit {
					0xD800..=0xDBFF => {
						if rest < 4 {
							// "unexpected end of data".
							return Step::Error(rest);
						}
						let low = u16::from_be_bytes([data[i + 2], data[i + 3]]);
						if (0xDC00..=0xDFFF).contains(&low) {
							let c = 0x10000
								+ ((u32::from(unit) - 0xD800) << 10)
								+ (u32::from(low) - 0xDC00);
							match char::from_u32(c) {
								Some(c) => Step::Char(c, 4),
								None => Step::Error(2),
							}
						} else {
							// "illegal UTF-16 surrogate".
							Step::Error(2)
						}
					}
					// "illegal encoding".
					0xDC00..=0xDFFF => Step::Error(2),
					_ => match char::from_u32(u32::from(unit)) {
						Some(c) => Step::Char(c, 2),
						None => Step::Error(2),
					},
				}
			}
			Codec::Ascii => {
				if data[i] < 0x80 {
					Step::Char(data[i] as char, 1)
				} else {
					Step::Error(1)
				}
			}
			Codec::Latin1 => Step::Char(data[i] as char, 1),
			Codec::Single(table) => match table.decode[data[i] as usize] {
				Some(c) => Step::Char(c, 1),
				None => Step::Error(1),
			},
			Codec::Multi(codec, extra) => match codec.decode_step(data, i) {
				Ok((c, n)) => Step::Char(c, n),
				// fontTools' extended codecs map single bytes on errors.
				Err(n) => match extra.and_then(|m| m.iter().find(|(k, _)| *k == data[i])) {
					Some(&(_, c)) => Step::Char(c, 1),
					None => Step::Error(n),
				},
			},
		}
	}

	/// Appends the encoding of a character, or returns `false` if the codec
	/// cannot encode it.
	fn encode_char(&self, c: char, out: &mut Vec<u8>) -> bool {
		match self {
			Codec::Utf16Be => {
				let mut units = [0u16; 2];
				for unit in c.encode_utf16(&mut units) {
					out.extend_from_slice(&unit.to_be_bytes());
				}
				true
			}
			Codec::Ascii => {
				let ok = c.is_ascii();
				if ok {
					out.push(c as u8);
				}
				ok
			}
			Codec::Latin1 => match u8::try_from(c as u32) {
				Ok(b) => {
					out.push(b);
					true
				}
				Err(_) => false,
			},
			Codec::Single(table) => match table.encode.get(&c) {
				Some(&b) => {
					out.push(b);
					true
				}
				None => false,
			},
			Codec::Multi(codec, extra) => match codec.encode.get(&c) {
				Some(b) => {
					out.extend_from_slice(b);
					true
				}
				None => match extra.and_then(|m| m.iter().find(|(_, v)| *v == c)) {
					Some(&(b, _)) => {
						out.push(b);
						true
					}
					None => false,
				},
			},
		}
	}
}

/// Decodes bytes (`bytes.decode(encoding)`).
///
/// ```
/// use fontoxide_encodings::codecs::decode;
/// assert_eq!(decode("mac_roman", b"caf\x8e").as_deref(), Ok("café"));
/// assert_eq!(decode("utf_16_be", b"\0A\0B").as_deref(), Ok("AB"));
/// ```
pub fn decode(encoding: &str, data: &[u8]) -> Result<String, CodecError> {
	decode_with(encoding, data, Errors::Strict)
}

/// Decodes bytes with an error handler (`bytes.decode(encoding, errors)`).
///
/// ```
/// use fontoxide_encodings::codecs::{Errors, decode_with};
/// assert_eq!(
///     decode_with("ascii", b"a\xffb", Errors::BackslashReplace).as_deref(),
///     Ok("a\\xffb")
/// );
/// assert_eq!(
///     decode_with("ascii", b"a\xffb", Errors::Replace).as_deref(),
///     Ok("a\u{FFFD}b")
/// );
/// ```
pub fn decode_with(encoding: &str, data: &[u8], errors: Errors) -> Result<String, CodecError> {
	let name = normalize(encoding);
	let codec = lookup(&name)?;
	let mut out = String::with_capacity(data.len());
	let mut i = 0;
	while i < data.len() {
		match codec.decode_step(data, i) {
			Step::Char(c, n) => {
				out.push(c);
				i += n;
			}
			Step::Error(n) => {
				match errors {
					Errors::Strict => {
						return Err(CodecError::Decode {
							encoding: name,
							position: i,
						});
					}
					Errors::Ignore => {}
					Errors::Replace => out.push('\u{FFFD}'),
					Errors::BackslashReplace => {
						for b in &data[i..i + n] {
							out.push_str(&format!("\\x{b:02x}"));
						}
					}
					Errors::XmlCharRefReplace => {
						return Err(CodecError::Handler(
							"don't know how to handle UnicodeDecodeError in error callback".into(),
						));
					}
				}
				i += n;
			}
		}
	}
	Ok(out)
}

/// Encodes text (`str.encode(encoding)`).
///
/// ```
/// use fontoxide_encodings::codecs::encode;
/// assert_eq!(encode("mac_roman", "café"), Ok(b"caf\x8e".to_vec()));
/// ```
pub fn encode(encoding: &str, text: &str) -> Result<Vec<u8>, CodecError> {
	encode_with(encoding, text, Errors::Strict)
}

/// Encodes text with an error handler (`str.encode(encoding, errors)`).
///
/// ```
/// use fontoxide_encodings::codecs::{Errors, encode_with};
/// assert_eq!(encode_with("ascii", "café", Errors::Replace), Ok(b"caf?".to_vec()));
/// assert_eq!(
///     encode_with("ascii", "café", Errors::XmlCharRefReplace),
///     Ok(b"caf&#233;".to_vec())
/// );
/// ```
pub fn encode_with(encoding: &str, text: &str, errors: Errors) -> Result<Vec<u8>, CodecError> {
	let name = normalize(encoding);
	let codec = lookup(&name)?;
	let mut out = Vec::with_capacity(text.len());
	for (i, c) in text.chars().enumerate() {
		if codec.encode_char(c, &mut out) {
			continue;
		}
		// The replacements are ASCII, which every codec that can fail encodes as
		// is.
		// LENIENT(L57): fontTools' `x_mac_*_ttx` codecs add the replacement, a
		// `str`, to their `bytes` output, so encoding with any handler but
		// `strict` fails with "TypeError: can't concat str to bytes" when a
		// character cannot be encoded; that crash is not ported, the handler
		// applies as with the other codecs.
		match errors {
			Errors::Strict => {
				return Err(CodecError::Encode {
					encoding: name,
					position: i,
				});
			}
			Errors::Ignore => {}
			Errors::Replace => out.push(b'?'),
			Errors::BackslashReplace => {
				let code = c as u32;
				let escape = if code < 0x100 {
					format!("\\x{code:02x}")
				} else if code < 0x10000 {
					format!("\\u{code:04x}")
				} else {
					format!("\\U{code:08x}")
				};
				out.extend_from_slice(escape.as_bytes());
			}
			Errors::XmlCharRefReplace => {
				out.extend_from_slice(format!("&#{};", c as u32).as_bytes());
			}
		}
	}
	Ok(out)
}

/// Whether the codec is known.
pub fn is_known(encoding: &str) -> bool {
	let name = normalize(encoding);
	matches!(name.as_str(), "utf_16_be" | "ascii" | "latin1")
		|| single_byte(&name).is_some()
		|| extended(&name).is_some()
		|| multi_byte(&name).is_some()
}

macro_rules! codec_data {
	($($name:literal),* $(,)?) => {
		fn codec_text(name: &str) -> Option<&'static str> {
			match name {
				$($name => Some(include_str!(concat!("../data/codecs/", $name, ".txt"))),)*
				_ => None,
			}
		}
	};
}

codec_data!(
	"mac_roman",
	"mac_iceland",
	"mac_turkish",
	"mac_croatian",
	"mac_latin2",
	"mac_romanian",
	"mac_greek",
	"mac_cyrillic",
);

/// The tables of the multi-byte codecs, in the binary format that
/// `tools/reference/codecs_export.py` describes.
fn codec_binary(name: &str) -> Option<&'static [u8]> {
	Some(match name {
		"shift_jis" => include_bytes!("../data/codecs/shift_jis.bin"),
		"gb2312" => include_bytes!("../data/codecs/gb2312.bin"),
		"big5" => include_bytes!("../data/codecs/big5.bin"),
		"euc_kr" => include_bytes!("../data/codecs/euc_kr.bin"),
		"johab" => include_bytes!("../data/codecs/johab.bin"),
		_ => return None,
	})
}

struct SingleByte {
	decode: [Option<char>; 256],
	encode: HashMap<char, u8>,
}

const SINGLE_BYTE: [&str; 8] = [
	"mac_roman",
	"mac_iceland",
	"mac_turkish",
	"mac_croatian",
	"mac_latin2",
	"mac_romanian",
	"mac_greek",
	"mac_cyrillic",
];

fn single_byte(name: &str) -> Option<&'static SingleByte> {
	static TABLES: OnceLock<HashMap<&'static str, SingleByte>> = OnceLock::new();
	let tables = TABLES.get_or_init(|| {
		SINGLE_BYTE
			.iter()
			.filter_map(|&n| {
				let text = codec_text(n)?;
				let line = text.lines().find(|l| !l.starts_with('#'))?;
				let mut decode = [None; 256];
				let mut encode = HashMap::new();
				for (b, cp) in line.split(' ').enumerate().take(256) {
					if let Some(c) = u32::from_str_radix(cp, 16).ok().and_then(char::from_u32) {
						decode[b] = Some(c);
						// Like a charmap codec: the first byte for a character wins.
						encode.entry(c).or_insert(b as u8);
					}
				}
				Some((n, SingleByte { decode, encode }))
			})
			.collect()
	});
	tables.get(name)
}

struct MultiByte {
	single: [Option<char>; 256],
	/// Whether the byte starts a two-byte sequence.
	lead: [bool; 256],
	double: HashMap<u16, char>,
	encode: HashMap<char, Vec<u8>>,
	/// Longer sequences that only show up through encoding (the EUC-KR make-up
	/// sequences for Hangul syllables outside KS X 1001).
	long: HashMap<Vec<u8>, char>,
	/// The initial, medial and final of each jamo byte of the make-up sequences,
	/// from `long`.
	jamo: OnceLock<[[Option<u32>; 3]; 256]>,
}

const MULTI_BYTE: [&str; 5] = ["shift_jis", "gb2312", "big5", "euc_kr", "johab"];

fn multi_byte(name: &str) -> Option<&'static MultiByte> {
	static TABLES: OnceLock<HashMap<&'static str, MultiByte>> = OnceLock::new();
	let tables = TABLES.get_or_init(|| {
		MULTI_BYTE
			.iter()
			.filter_map(|&n| Some((n, parse_multi_byte(codec_binary(n)?)?)))
			.collect()
	});
	tables.get(name)
}

/// A big-endian reader over the bytes of a codec table.
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
	fn take(&mut self, n: usize) -> Option<&[u8]> {
		let (head, rest) = self.0.split_at_checked(n)?;
		self.0 = rest;
		Some(head)
	}

	fn u8(&mut self) -> Option<u8> {
		Some(self.take(1)?[0])
	}

	fn u16(&mut self) -> Option<u16> {
		let b = self.take(2)?;
		Some(u16::from_be_bytes([b[0], b[1]]))
	}

	fn u32(&mut self) -> Option<u32> {
		let b = self.take(4)?;
		Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
	}

	fn char(&mut self) -> Option<Option<char>> {
		let cp = self.u16()?;
		Some(if cp == 0xFFFF {
			None
		} else {
			char::from_u32(u32::from(cp))
		})
	}
}

/// The number of initial, medial and final jamo of the EUC-KR make-up
/// sequences.
const JAMO_COUNTS: [usize; 3] = [19, 21, 28];

fn parse_multi_byte(data: &[u8]) -> Option<MultiByte> {
	let mut codec = MultiByte {
		single: [None; 256],
		lead: [false; 256],
		double: HashMap::new(),
		encode: HashMap::new(),
		long: HashMap::new(),
		jamo: OnceLock::new(),
	};
	let mut r = Bytes(data);
	for _ in 0..r.u16()? {
		let b = r.u8()?;
		codec.single[b as usize] = r.char()?;
	}
	for _ in 0..r.u16()? {
		let first = r.u16()?;
		let len = r.u16()?;
		for k in 0..len {
			let seq = first.checked_add(k)?;
			if let Some(c) = r.char()? {
				codec.lead[(seq >> 8) as usize] = true;
				codec.double.insert(seq, c);
			}
		}
	}
	// Characters whose encoding differs from what the decoding tables imply; an
	// empty vector means the character does not encode.
	let mut overrides: HashMap<char, Vec<u8>> = HashMap::new();
	for _ in 0..r.u16()? {
		let c = char::from_u32(r.u32()?)?;
		let len = r.u8()?;
		overrides.insert(c, r.take(len.into())?.to_vec());
	}
	if r.u8()? == 1 {
		// The Hangul syllables outside KS X 1001 encode as make-up sequences.
		let parts = JAMO_COUNTS.map(|n| r.take(n).map(<[u8]>::to_vec));
		let [Some(cho), Some(jung), Some(jong)] = parts else {
			return None;
		};
		let decodable: std::collections::HashSet<char> = codec.double.values().copied().collect();
		for s in 0..(19 * 21 * 28) {
			let c = char::from_u32(0xAC00 + s)?;
			if decodable.contains(&c) {
				continue;
			}
			let (i, m, f) = (
				(s / 588) as usize,
				((s % 588) / 28) as usize,
				(s % 28) as usize,
			);
			let bytes = vec![
				EUCKR_JAMO_FIRSTBYTE,
				EUCKR_JAMO_FILLER,
				EUCKR_JAMO_FIRSTBYTE,
				cho[i],
				EUCKR_JAMO_FIRSTBYTE,
				jung[m],
				EUCKR_JAMO_FIRSTBYTE,
				jong[f],
			];
			codec.long.insert(bytes.clone(), c);
			overrides.insert(c, bytes);
		}
	}
	// The first sequence that decodes to a character is its encoding, unless
	// overridden.
	for (b, c) in codec.single.iter().enumerate() {
		if let Some(c) = c {
			codec.encode.entry(*c).or_insert_with(|| vec![b as u8]);
		}
	}
	let mut doubles: Vec<(u16, char)> = codec.double.iter().map(|(&s, &c)| (s, c)).collect();
	doubles.sort();
	for (seq, c) in doubles {
		codec
			.encode
			.entry(c)
			.or_insert_with(|| seq.to_be_bytes().to_vec());
	}
	for (c, bytes) in overrides {
		if bytes.is_empty() {
			codec.encode.remove(&c);
		} else {
			codec.encode.insert(c, bytes);
		}
	}
	Some(codec)
}

impl MultiByte {
	/// Decodes the character at `i`: the character and its length, or the
	/// length of the error (`UnicodeDecodeError.end - .start`).
	fn decode_step(&self, data: &[u8], i: usize) -> Result<(char, usize), usize> {
		let b = data[i];
		if !self.long.is_empty()
			&& b == EUCKR_JAMO_FIRSTBYTE
			&& data.get(i + 1) == Some(&EUCKR_JAMO_FILLER)
		{
			// A make-up sequence of KS X 1001:1998 Annex 3: CPython's EUC-KR decoder
			// needs all 8 bytes ("incomplete multibyte sequence" otherwise).
			let Some(seq) = data.get(i..i + 8) else {
				return Err(data.len() - i);
			};
			return self.makeup(seq).map(|c| (c, 8)).ok_or(1);
		}
		let decoded = if self.lead[b as usize] {
			data.get(i + 1)
				.and_then(|&t| self.double.get(&u16::from_be_bytes([b, t])))
				.map(|&c| (c, 2))
		} else {
			self.single[b as usize].map(|c| (c, 1))
		};
		// CPython's CJK codecs report one byte, also for an incomplete sequence at
		// the end, which is then a single byte too.
		decoded.ok_or(1)
	}

	/// The syllable of an EUC-KR make-up sequence (`\xa4\xd4` and three
	/// `\xa4`-prefixed jamo).
	fn makeup(&self, seq: &[u8]) -> Option<char> {
		if seq[2] != EUCKR_JAMO_FIRSTBYTE
			|| seq[4] != EUCKR_JAMO_FIRSTBYTE
			|| seq[6] != EUCKR_JAMO_FIRSTBYTE
		{
			return None;
		}
		let jamo = self.jamo.get_or_init(|| {
			// The jamo bytes, from the sequences that encoding produces, which
			// cover every initial, medial and final.
			let mut jamo = [[None; 3]; 256];
			for (bytes, &c) in &self.long {
				let s = c as u32 - 0xAC00;
				let parts = [s / 588, (s % 588) / 28, s % 28];
				for (k, part) in parts.into_iter().enumerate() {
					jamo[bytes[3 + 2 * k] as usize][k] = Some(part);
				}
			}
			jamo
		});
		let cho = jamo[seq[3] as usize][0]?;
		let jung = jamo[seq[5] as usize][1]?;
		let jong = jamo[seq[7] as usize][2]?;
		char::from_u32(0xAC00 + cho * 588 + jung * 28 + jong)
	}
}

/// The bytes that start an EUC-KR make-up sequence (`EUCKR_JAMO_FIRSTBYTE`,
/// `EUCKR_JAMO_FILLER`).
const EUCKR_JAMO_FIRSTBYTE: u8 = 0xA4;
const EUCKR_JAMO_FILLER: u8 = 0xD4;

/// fontTools' `x_mac_*_ttx` codecs (`fontTools.encodings.codecs`): a base codec
/// plus single bytes that are mapped where the base codec fails.
fn extended(name: &str) -> Option<(&'static str, &'static [(u8, char)])> {
	const JAPANESE: &[(u8, char)] = &[
		(0xFC, '\u{007C}'),
		(0x7E, '\u{007E}'),
		(0x80, '\u{005C}'),
		(0xA0, '\u{00A0}'),
		(0xFD, '\u{00A9}'),
		(0xFE, '\u{2122}'),
		(0xFF, '\u{2026}'),
	];
	const TRAD_CHINESE: &[(u8, char)] = &[
		(0x80, '\u{005C}'),
		(0xA0, '\u{00A0}'),
		(0xFD, '\u{00A9}'),
		(0xFE, '\u{2122}'),
		(0xFF, '\u{2026}'),
	];
	const KOREAN: &[(u8, char)] = &[
		(0x80, '\u{00A0}'),
		(0x81, '\u{20A9}'),
		(0x82, '\u{2014}'),
		(0x83, '\u{00A9}'),
		(0xFE, '\u{2122}'),
		(0xFF, '\u{2026}'),
	];
	const SIMP_CHINESE: &[(u8, char)] = &[
		(0x80, '\u{00FC}'),
		(0xA0, '\u{00A0}'),
		(0xFD, '\u{00A9}'),
		(0xFE, '\u{2122}'),
		(0xFF, '\u{2026}'),
	];
	match name {
		"x_mac_japanese_ttx" => Some(("shift_jis", JAPANESE)),
		"x_mac_trad_chinese_ttx" => Some(("big5", TRAD_CHINESE)),
		"x_mac_korean_ttx" => Some(("euc_kr", KOREAN)),
		"x_mac_simp_chinese_ttx" => Some(("gb2312", SIMP_CHINESE)),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn single_byte_codecs() {
		assert_eq!(decode("mac_roman", b"\x80\xff").as_deref(), Ok("Äˇ"));
		assert_eq!(encode("mac_cyrillic", "Ж"), Ok(vec![0x86]));
		assert!(encode("mac_roman", "Ж").is_err());
	}

	#[test]
	fn multi_byte_codecs() {
		assert!(decode("big5", "中文".as_bytes()).is_err());
		assert_eq!(decode("big5", b"\xa4\xa4\xa4\xe5").as_deref(), Ok("中文"));
		assert_eq!(encode("big5", "中文"), Ok(b"\xa4\xa4\xa4\xe5".to_vec()));
		assert_eq!(decode("shift_jis", b"\x82\xa0A").as_deref(), Ok("あA"));
		assert!(decode("shift_jis", b"\x82").is_err());
		assert_eq!(decode("euc_kr", b"\xb0\xa1").as_deref(), Ok("가"));
		// A Hangul syllable outside KS X 1001, as an 8-byte make-up sequence.
		let s = encode("euc_kr", "\u{AC02}").unwrap_or_default();
		assert_eq!(s.len(), 8);
		assert_eq!(decode("euc_kr", &s).as_deref(), Ok("\u{AC02}"));
	}

	#[test]
	fn extended_codecs() {
		assert_eq!(
			decode("x_mac_japanese_ttx", b"\x80\xfdA").as_deref(),
			Ok("\\©A")
		);
		assert_eq!(encode("x_mac_japanese_ttx", "©"), Ok(vec![0xFD]));
		assert_eq!(
			decode("x_mac_trad_chinese_ttx", b"\xa4\xa4\xff").as_deref(),
			Ok("中…")
		);
	}

	/// The error ranges of CPython's decoders, which the handlers replace (from
	/// the reference environment; a fuzz comparison of 52,500 inputs over all
	/// codecs and handlers matched too).
	#[test]
	#[allow(clippy::unwrap_used)]
	fn error_handlers() {
		let d = |codec, data: &[u8], errors| decode_with(codec, data, errors).unwrap();
		assert_eq!(
			d("euc_kr", b"\xa4\xd4\xa4\xa1A", Errors::BackslashReplace),
			"\\xa4\\xd4\\xa4\\xa1\\x41"
		);
		assert_eq!(
			d(
				"euc_kr",
				b"\xa4\xd4\xa4\xa1\xa4\xbf\xa3\xd4",
				Errors::Replace
			),
			"\u{FFFD}\u{6E21}\u{B7}\u{C5D4}\u{FFFD}"
		);
		// A make-up sequence of a syllable in KS X 1001.
		assert_eq!(
			d(
				"euc_kr",
				b"\xa4\xd4\xa4\xa1\xa4\xbf\xa4\xd4",
				Errors::Strict
			),
			"\u{AC00}"
		);
		assert_eq!(
			d("utf_16_be", b"\xd8\x00\x00A\x00", Errors::BackslashReplace),
			"\\xd8\\x00A\\x00"
		);
		assert_eq!(d("utf_16_be", b"\xd8\x00\x00", Errors::Replace), "\u{FFFD}");
		assert_eq!(d("ascii", b"a\xffb", Errors::Ignore), "ab");
		assert_eq!(
			encode_with("big5", "a\u{E9}\u{1F600}", Errors::BackslashReplace),
			Ok(b"a\\xe9\\U0001f600".to_vec())
		);
		assert!(matches!(
			Errors::from_name("bogus"),
			Err(CodecError::Handler(_))
		));
		assert!(decode_with("ascii", b"\xff", Errors::XmlCharRefReplace).is_err());
	}

	#[test]
	fn utf16() {
		assert!(decode("utf_16_be", b"\0").is_err());
		assert!(decode("utf_16_be", b"\xd8\x00").is_err());
		assert_eq!(
			encode("UTF-16BE", "\u{1F600}"),
			Ok(vec![0xD8, 0x3D, 0xDE, 0x00])
		);
	}
}
