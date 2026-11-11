//! Port of `fontTools.unicodedata` (and `fontTools.unicode`): Unicode character
//! properties at the Unicode version fontTools uses.
//!
//! fontTools takes its data from the `unicodedata2` package when installed, which
//! is usually newer than Python's built-in `unicodedata`. The data in `data/` is
//! exported from the fontTools reference environment by `cargo xtask codegen`;
//! the Script, Script_Extensions and Blocks tables in `ucd.rs` by
//! `tools/reference/unicodedata_fixtures.py`.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::OnceLock;

mod mirrored;
mod ot_tags;
#[rustfmt::skip]
mod ucd;

pub use ot_tags::{InvalidTagError, ot_tag_to_script, ot_tags_from_script};

const NAMES: &str = include_str!("../data/names.txt");
const NAME_RANGES: &str = include_str!("../data/name_ranges.txt");

/// The Unicode version of the data.
pub fn unidata_version() -> &'static str {
	NAMES
		.lines()
		.next()
		.and_then(|l| l.strip_prefix("# Unicode "))
		.and_then(|l| l.split(' ').next())
		.unwrap_or_default()
}

fn names() -> &'static [(u32, &'static str)] {
	static TABLE: OnceLock<Vec<(u32, &'static str)>> = OnceLock::new();
	TABLE.get_or_init(|| {
		NAMES
			.lines()
			.filter(|l| !l.starts_with('#'))
			.filter_map(|l| {
				let (cp, name) = l.split_once(';')?;
				Some((u32::from_str_radix(cp, 16).ok()?, name))
			})
			.collect()
	})
}

fn name_ranges() -> &'static [(&'static str, u32, u32)] {
	static TABLE: OnceLock<Vec<(&'static str, u32, u32)>> = OnceLock::new();
	TABLE.get_or_init(|| {
		NAME_RANGES
			.lines()
			.filter(|l| !l.starts_with('#'))
			.filter_map(|l| {
				let mut parts = l.split(';');
				let prefix = parts.next()?;
				let first = u32::from_str_radix(parts.next()?, 16).ok()?;
				let last = u32::from_str_radix(parts.next()?, 16).ok()?;
				Some((prefix, first, last))
			})
			.collect()
	})
}

const JAMO_L: [&str; 19] = [
	"G", "GG", "N", "D", "DD", "R", "M", "B", "BB", "S", "SS", "", "J", "JJ", "C", "K", "T", "P",
	"H",
];
const JAMO_V: [&str; 21] = [
	"A", "AE", "YA", "YAE", "EO", "E", "YEO", "YE", "O", "WA", "WAE", "OE", "YO", "U", "WEO", "WE",
	"WI", "YU", "EU", "YI", "I",
];
const JAMO_T: [&str; 28] = [
	"", "G", "GG", "GS", "N", "NJ", "NH", "D", "L", "LG", "LM", "LB", "LS", "LT", "LP", "LH", "M",
	"B", "BS", "S", "SS", "NG", "J", "C", "K", "T", "P", "H",
];

/// The character name (`unicodedata.name(chr(code), None)`).
///
/// ```
/// assert_eq!(fontoxide_unicodedata::name(0x41).as_deref(), Some("LATIN CAPITAL LETTER A"));
/// assert_eq!(fontoxide_unicodedata::name(0x4E00).as_deref(), Some("CJK UNIFIED IDEOGRAPH-4E00"));
/// assert_eq!(fontoxide_unicodedata::name(0xAC01).as_deref(), Some("HANGUL SYLLABLE GAG"));
/// assert_eq!(fontoxide_unicodedata::name(0x0), None);
/// ```
pub fn name(code: u32) -> Option<Cow<'static, str>> {
	let table = names();
	if let Ok(i) = table.binary_search_by_key(&code, |&(cp, _)| cp) {
		return Some(Cow::Borrowed(table[i].1));
	}
	let &(prefix, _, _) = name_ranges()
		.iter()
		.find(|&&(_, first, last)| (first..=last).contains(&code))?;
	if prefix == "HANGUL SYLLABLE " {
		let s = code - 0xAC00;
		let (l, v, t) = (s / (21 * 28), s % (21 * 28) / 28, s % 28);
		return Some(Cow::Owned(format!(
			"HANGUL SYLLABLE {}{}{}",
			JAMO_L[l as usize], JAMO_V[v as usize], JAMO_T[t as usize]
		)));
	}
	Some(Cow::Owned(format!("{prefix}{code:04X}")))
}

/// The name as `fontTools.unicode.Unicode[code]` gives it: `"????"` for code
/// points without a name.
pub fn unicode_name(code: u32) -> Cow<'static, str> {
	name(code).unwrap_or(Cow::Borrowed("????"))
}

/// The mirrored version of a code point, if it has one (`mirrored`).
pub fn mirrored(code: u32) -> Option<u32> {
	mirrored::MIRRORED
		.binary_search_by_key(&code, |&(c, _)| c)
		.ok()
		.map(|i| mirrored::MIRRORED[i].1)
}

/// The index of the range containing `code` in a sorted list of range starts
/// (`bisect_right(RANGES, code) - 1`). The first range starts at 0.
fn range_index(ranges: &[u32], code: u32) -> usize {
	ranges
		.partition_point(|&start| start <= code)
		.saturating_sub(1)
}

/// The four-letter script code of a code point (`script`).
///
/// ```
/// use fontoxide_unicodedata::script;
/// assert_eq!(script('a' as u32), "Latn");
/// assert_eq!(script(',' as u32), "Zyyy");
/// assert_eq!(script(0x10FFFF), "Zzzz");
/// ```
pub fn script(code: u32) -> &'static str {
	ucd::SCRIPT_VALUES[range_index(ucd::SCRIPT_RANGES, code)]
}

/// The Script_Extensions property of a code point: a set of script codes
/// (`script_extension`). Code points without an explicit value have their
/// [`script`].
///
/// ```
/// use std::collections::BTreeSet;
/// use fontoxide_unicodedata::script_extension;
/// assert_eq!(script_extension('a' as u32), BTreeSet::from(["Latn"]));
/// assert_eq!(
///     script_extension(0x060C),
///     BTreeSet::from(["Nkoo", "Arab", "Rohg", "Thaa", "Syrc", "Gara", "Yezi"])
/// );
/// assert_eq!(script_extension(0x10FFFF), BTreeSet::from(["Zzzz"]));
/// ```
pub fn script_extension(code: u32) -> BTreeSet<&'static str> {
	match ucd::SCRIPT_EXTENSION_VALUES[range_index(ucd::SCRIPT_EXTENSION_RANGES, code)] {
		Some(scripts) => scripts.iter().copied().collect(),
		None => BTreeSet::from([script(code)]),
	}
}

/// The long name of a script code (with `_` in it), if the code is known.
fn script_long_name(code: &str) -> Option<&'static str> {
	ucd::SCRIPT_NAMES
		.binary_search_by_key(&code, |&(c, _)| c)
		.ok()
		.map(|i| ucd::SCRIPT_NAMES[i].1)
}

/// The script code as stored in the table, if `code` is a known script code
/// (`code in Scripts.NAMES`).
fn known_script_code(code: &str) -> Option<&'static str> {
	ucd::SCRIPT_NAMES
		.binary_search_by_key(&code, |&(c, _)| c)
		.ok()
		.map(|i| ucd::SCRIPT_NAMES[i].0)
}

/// The long, human-readable name of a four-letter script code, or `None` if it
/// is not known (`script_name`, which raises `KeyError` or returns `default`).
///
/// ```
/// use fontoxide_unicodedata::script_name;
/// assert_eq!(script_name("Egyp").as_deref(), Some("Egyptian Hieroglyphs"));
/// assert_eq!(script_name("QQQQ"), None);
/// ```
pub fn script_name(code: &str) -> Option<String> {
	script_long_name(code).map(|name| name.replace('_', " "))
}

/// `_normalize_property_name`: removes `-`, `_` and spaces and lowercases.
fn normalize_property_name(name: &str) -> String {
	let stripped: String = name
		.chars()
		.filter(|c| !matches!(c, '-' | '_' | ' '))
		.collect();
	stripped.to_lowercase()
}

/// The four-letter script code of a long script name, matched loosely (case,
/// spaces, `-` and `_` are ignored), or `None` if there is none
/// (`script_code`, which raises `KeyError` or returns `default`).
///
/// ```
/// use fontoxide_unicodedata::script_code;
/// assert_eq!(script_code("egyptian-hieroglyphs"), Some("Egyp"));
/// assert_eq!(script_code("Does not exist"), None);
/// ```
pub fn script_code(script_name: &str) -> Option<&'static str> {
	static CODES: OnceLock<Vec<(String, &'static str)>> = OnceLock::new();
	let codes = CODES.get_or_init(|| {
		// `{_normalize_property_name(v): k for k, v in Scripts.NAMES.items()}`:
		// a later entry wins on equal keys.
		let mut codes: Vec<(String, &'static str)> = Vec::new();
		for &(code, name) in ucd::SCRIPT_NAMES {
			let key = normalize_property_name(name);
			match codes.iter_mut().find(|(k, _)| *k == key) {
				Some(slot) => slot.1 = code,
				None => codes.push((key, code)),
			}
		}
		codes.sort();
		codes
	});
	let key = normalize_property_name(script_name);
	codes
		.binary_search_by(|(k, _)| k.as_str().cmp(&key))
		.ok()
		.map(|i| codes[i].1)
}

/// Scripts that contain right-to-left characters according to the Bidi_Class
/// property (`RTL_SCRIPTS`), from HarfBuzz.
pub const RTL_SCRIPTS: [&str; 37] = [
	"Arab", "Hebr", "Syrc", "Thaa", "Cprt", "Khar", "Phnx", "Nkoo", "Lydi", "Avst", "Armi", "Phli",
	"Prti", "Sarb", "Orkh", "Samr", "Mand", "Merc", "Mero", "Mani", "Mend", "Nbat", "Narb", "Palm",
	"Phlp", "Hatr", "Hung", "Adlm", "Rohg", "Sogo", "Sogd", "Elym", "Chrs", "Yezi", "Ougr", "Gara",
	"Sidt",
];

/// The horizontal direction of a script (`HorizDirection`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HorizDirection {
	/// `"LTR"`.
	Ltr,
	/// `"RTL"`.
	Rtl,
}

impl HorizDirection {
	/// `"LTR"` or `"RTL"`.
	pub fn as_str(self) -> &'static str {
		match self {
			HorizDirection::Ltr => "LTR",
			HorizDirection::Rtl => "RTL",
		}
	}
}

/// Right-to-left for scripts that contain right-to-left characters according
/// to the Bidi_Class property, otherwise left-to-right; `None` for unknown
/// script codes (`script_horizontal_direction`, which raises `KeyError` or
/// returns `default`).
///
/// ```
/// use fontoxide_unicodedata::{HorizDirection, script_horizontal_direction};
/// assert_eq!(script_horizontal_direction("Arab"), Some(HorizDirection::Rtl));
/// assert_eq!(script_horizontal_direction("Latn"), Some(HorizDirection::Ltr));
/// assert_eq!(script_horizontal_direction("Azzz"), None);
/// ```
pub fn script_horizontal_direction(script_code: &str) -> Option<HorizDirection> {
	known_script_code(script_code)?;
	Some(if RTL_SCRIPTS.contains(&script_code) {
		HorizDirection::Rtl
	} else {
		HorizDirection::Ltr
	})
}

/// The block of a code point, `"No_Block"` outside of blocks (`block`).
///
/// ```
/// use fontoxide_unicodedata::block;
/// assert_eq!(block('a' as u32), "Basic Latin");
/// assert_eq!(block(0x060C), "Arabic");
/// assert_eq!(block(0xEFFFF), "No_Block");
/// ```
pub fn block(code: u32) -> &'static str {
	ucd::BLOCK_VALUES[range_index(ucd::BLOCK_RANGES, code)]
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn names() {
		assert_eq!(unicode_name(0x20), "SPACE");
		assert_eq!(unicode_name(0xD7A3), "HANGUL SYLLABLE HIH");
		assert_eq!(unicode_name(0xAC00), "HANGUL SYLLABLE GA");
		assert_eq!(unicode_name(0x20000), "CJK UNIFIED IDEOGRAPH-20000");
		assert_eq!(unicode_name(0x1F600), "GRINNING FACE");
		assert_eq!(unicode_name(0xD800), "????");
		assert_eq!(unicode_name(0x110000), "????");
		assert_eq!(unicode_name(0x7F), "????");
		assert!(unidata_version().starts_with(char::is_numeric));
	}
}
