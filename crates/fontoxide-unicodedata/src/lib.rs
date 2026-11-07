//! Port of `fontTools.unicodedata` (and `fontTools.unicode`): Unicode character
//! properties at the Unicode version fontTools uses.
//!
//! fontTools takes its data from the `unicodedata2` package when installed, which
//! is usually newer than Python's built-in `unicodedata`. The data in `data/` is
//! exported from the fontTools reference environment by `cargo xtask codegen`.

use std::borrow::Cow;
use std::sync::OnceLock;

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
