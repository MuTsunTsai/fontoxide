//! Text codecs by Python name (`str.encode` / `bytes.decode` with
//! `errors="strict"`).
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

/// Decodes bytes (`bytes.decode(encoding)`).
///
/// ```
/// use fontoxide_encodings::codecs::decode;
/// assert_eq!(decode("mac_roman", b"caf\x8e").as_deref(), Ok("café"));
/// assert_eq!(decode("utf_16_be", b"\0A\0B").as_deref(), Ok("AB"));
/// ```
pub fn decode(encoding: &str, data: &[u8]) -> Result<String, CodecError> {
	let name = normalize(encoding);
	let err = |position| CodecError::Decode {
		encoding: name.clone(),
		position,
	};
	match name.as_str() {
		"utf_16_be" => {
			if data.len() % 2 == 1 {
				return Err(err(data.len() - 1));
			}
			let units = data
				.as_chunks::<2>()
				.0
				.iter()
				.map(|&c| u16::from_be_bytes(c));
			let mut out = String::with_capacity(data.len() / 2);
			for (i, r) in char::decode_utf16(units).enumerate() {
				match r {
					Ok(c) => out.push(c),
					Err(_) => return Err(err(2 * i)),
				}
			}
			Ok(out)
		}
		"ascii" => data
			.iter()
			.enumerate()
			.map(|(i, &b)| if b < 0x80 { Ok(b as char) } else { Err(err(i)) })
			.collect(),
		"latin1" => Ok(data.iter().map(|&b| b as char).collect()),
		_ => {
			if let Some(table) = single_byte(&name) {
				return data
					.iter()
					.enumerate()
					.map(|(i, &b)| table.decode[b as usize].ok_or_else(|| err(i)))
					.collect();
			}
			if let Some((base, extra)) = extended(&name) {
				let codec = multi_byte(base).ok_or_else(|| CodecError::Unknown(name.clone()))?;
				return codec.decode(data, Some(extra)).map_err(err);
			}
			let codec = multi_byte(&name).ok_or_else(|| CodecError::Unknown(name.clone()))?;
			codec.decode(data, None).map_err(err)
		}
	}
}

/// Encodes text (`str.encode(encoding)`).
///
/// ```
/// use fontoxide_encodings::codecs::encode;
/// assert_eq!(encode("mac_roman", "café"), Ok(b"caf\x8e".to_vec()));
/// ```
pub fn encode(encoding: &str, text: &str) -> Result<Vec<u8>, CodecError> {
	let name = normalize(encoding);
	let err = |position| CodecError::Encode {
		encoding: name.clone(),
		position,
	};
	match name.as_str() {
		"utf_16_be" => Ok(text.encode_utf16().flat_map(u16::to_be_bytes).collect()),
		"ascii" => text
			.chars()
			.enumerate()
			.map(|(i, c)| {
				if c.is_ascii() {
					Ok(c as u8)
				} else {
					Err(err(i))
				}
			})
			.collect(),
		"latin1" => text
			.chars()
			.enumerate()
			.map(|(i, c)| u8::try_from(c as u32).map_err(|_| err(i)))
			.collect(),
		_ => {
			if let Some(table) = single_byte(&name) {
				return text
					.chars()
					.enumerate()
					.map(|(i, c)| table.encode.get(&c).copied().ok_or_else(|| err(i)))
					.collect();
			}
			if let Some((base, extra)) = extended(&name) {
				let codec = multi_byte(base).ok_or_else(|| CodecError::Unknown(name.clone()))?;
				return codec.encode(text, Some(extra)).map_err(err);
			}
			let codec = multi_byte(&name).ok_or_else(|| CodecError::Unknown(name.clone()))?;
			codec.encode(text, None).map_err(err)
		}
	}
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
	"shift_jis",
	"gb2312",
	"big5",
	"euc_kr",
	"johab",
);

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
	/// sequences for Hangul syllables outside KS X 1001), for decoding.
	long: HashMap<Vec<u8>, char>,
}

const MULTI_BYTE: [&str; 5] = ["shift_jis", "gb2312", "big5", "euc_kr", "johab"];

fn multi_byte(name: &str) -> Option<&'static MultiByte> {
	static TABLES: OnceLock<HashMap<&'static str, MultiByte>> = OnceLock::new();
	let tables = TABLES.get_or_init(|| {
		MULTI_BYTE
			.iter()
			.filter_map(|&n| Some((n, parse_multi_byte(codec_text(n)?))))
			.collect()
	});
	tables.get(name)
}

fn parse_hex_bytes(v: &str) -> Vec<u8> {
	(0..v.len() / 2)
		.filter_map(|i| u8::from_str_radix(&v[2 * i..2 * i + 2], 16).ok())
		.collect()
}

fn parse_multi_byte(text: &str) -> MultiByte {
	let mut codec = MultiByte {
		single: [None; 256],
		lead: [false; 256],
		double: HashMap::new(),
		encode: HashMap::new(),
		long: HashMap::new(),
	};
	let hex_char = |s: &str| u32::from_str_radix(s, 16).ok().and_then(char::from_u32);
	// Characters whose encoding differs from what the decoding tables imply; an
	// empty vector means the character does not encode.
	let mut overrides: HashMap<char, Vec<u8>> = HashMap::new();
	for line in text.lines() {
		let Some((kind, items)) = line.split_once(' ') else {
			continue;
		};
		for item in items.split(' ').filter(|s| !s.is_empty()) {
			let Some((k, v)) = item.split_once(':') else {
				continue;
			};
			match kind {
				"single" => {
					if let (Ok(b), Some(c)) = (u8::from_str_radix(k, 16), hex_char(v)) {
						codec.single[b as usize] = Some(c);
					}
				}
				"double" => {
					if let (Ok(seq), Some(c)) = (u16::from_str_radix(k, 16), hex_char(v)) {
						codec.lead[(seq >> 8) as usize] = true;
						codec.double.insert(seq, c);
					}
				}
				"encode" => {
					if let Some(c) = hex_char(k) {
						let bytes = if v == "-" {
							Vec::new()
						} else {
							parse_hex_bytes(v)
						};
						if bytes.len() > 2 {
							codec.long.insert(bytes.clone(), c);
						}
						overrides.insert(c, bytes);
					}
				}
				_ => {}
			}
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
	codec
}

impl MultiByte {
	fn decode(&self, data: &[u8], extra: Option<&[(u8, char)]>) -> Result<String, usize> {
		let mut out = String::with_capacity(data.len());
		let mut i = 0;
		while i < data.len() {
			let b = data[i];
			if !self.long.is_empty()
				&& let Some(&c) = data.get(i..i + 8).and_then(|s| self.long.get(s))
			{
				out.push(c);
				i += 8;
				continue;
			}
			let decoded = if self.lead[b as usize] {
				data.get(i + 1)
					.and_then(|&t| self.double.get(&u16::from_be_bytes([b, t])))
					.map(|&c| (c, 2))
			} else {
				self.single[b as usize].map(|c| (c, 1))
			};
			match decoded {
				Some((c, n)) => {
					out.push(c);
					i += n;
				}
				// fontTools' extended codecs map single bytes on errors.
				None => match extra.and_then(|m| m.iter().find(|(k, _)| *k == b)) {
					Some(&(_, c)) => {
						out.push(c);
						i += 1;
					}
					None => return Err(i),
				},
			}
		}
		Ok(out)
	}

	fn encode(&self, text: &str, extra: Option<&[(u8, char)]>) -> Result<Vec<u8>, usize> {
		let mut out = Vec::with_capacity(text.len());
		for (i, c) in text.chars().enumerate() {
			match self.encode.get(&c) {
				Some(b) => out.extend_from_slice(b),
				None => match extra.and_then(|m| m.iter().find(|(_, v)| *v == c)) {
					Some(&(b, _)) => out.push(b),
					None => return Err(i),
				},
			}
		}
		Ok(out)
	}
}

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
