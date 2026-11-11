//! OpenType script tags of Unicode scripts (`fontTools.unicodedata.OTTags`,
//! `ot_tags_from_script` and `ot_tag_to_script`).

use std::fmt;

use crate::known_script_code;

/// `DEFAULT_SCRIPT`.
pub const DEFAULT_SCRIPT: &str = "DFLT";

/// `SCRIPT_ALIASES`.
const SCRIPT_ALIASES: [(&str, &str); 1] = [("jamo", "hang")];

/// `SCRIPT_EXCEPTIONS`.
const SCRIPT_EXCEPTIONS: [(&str, &str); 10] = [
	("Hira", "kana"),
	("Hrkt", "kana"),
	("Laoo", "lao "),
	("Yiii", "yi  "),
	("Nkoo", "nko "),
	("Vaii", "vai "),
	("Zmth", "math"),
	("Zinh", DEFAULT_SCRIPT),
	("Zyyy", DEFAULT_SCRIPT),
	("Zzzz", DEFAULT_SCRIPT),
];

/// `SCRIPT_EXCEPTIONS_REVERSED`.
const SCRIPT_EXCEPTIONS_REVERSED: [(&str, &str); 1] = [("math", "Zmth")];

/// `NEW_SCRIPT_TAGS` (each script has a single new tag).
const NEW_SCRIPT_TAGS: [(&str, &str); 10] = [
	("Beng", "bng2"),
	("Deva", "dev2"),
	("Gujr", "gjr2"),
	("Guru", "gur2"),
	("Knda", "knd2"),
	("Mlym", "mlm2"),
	("Orya", "ory2"),
	("Taml", "tml2"),
	("Telu", "tel2"),
	("Mymr", "mym2"),
];

fn lookup(table: &[(&'static str, &'static str)], key: &str) -> Option<&'static str> {
	table.iter().find(|(k, _)| *k == key).map(|&(_, v)| v)
}

/// The OpenType script tags of a Unicode script code, the newest first;
/// `["DFLT"]` for special or unknown script codes (`ot_tags_from_script`).
///
/// ```
/// use fontoxide_unicodedata::ot_tags_from_script;
/// assert_eq!(ot_tags_from_script("Deva"), ["dev2", "deva"]);
/// assert_eq!(ot_tags_from_script("Hira"), ["kana"]);
/// assert_eq!(ot_tags_from_script("Aaaa"), ["DFLT"]);
/// ```
pub fn ot_tags_from_script(script_code: &str) -> Vec<String> {
	if let Some(tag) = lookup(&SCRIPT_EXCEPTIONS, script_code) {
		return vec![tag.to_owned()];
	}
	if known_script_code(script_code).is_none() {
		return vec![DEFAULT_SCRIPT.to_owned()];
	}
	let mut chars = script_code.chars();
	let first: String = chars
		.next()
		.into_iter()
		.flat_map(char::to_lowercase)
		.collect();
	let mut tags = vec![first + chars.as_str()];
	if let Some(new) = lookup(&NEW_SCRIPT_TAGS, script_code) {
		tags.push(new.to_owned());
		// last in, first out
		tags.reverse();
	}
	tags
}

/// An invalid OpenType tag passed to [`ot_tag_to_script`] (`ValueError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidTagError(pub String);

impl fmt::Display for InvalidTagError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "invalid OpenType tag: {}", py_str_repr(&self.0))
	}
}

impl std::error::Error for InvalidTagError {}

/// `repr(str)` for the error message (only quotes and backslashes need care
/// for what can get here).
fn py_str_repr(s: &str) -> String {
	let quote = if s.contains('\'') && !s.contains('"') {
		'"'
	} else {
		'\''
	};
	let mut out = String::new();
	out.push(quote);
	for c in s.chars() {
		match c {
			'\\' => out.push_str("\\\\"),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\t' => out.push_str("\\t"),
			c if c == quote => {
				out.push('\\');
				out.push(c);
			}
			c if (c as u32) < 0x20 || c == '\x7f' => out.push_str(&format!("\\x{:02x}", c as u32)),
			c => out.push(c),
		}
	}
	out.push(quote);
	out
}

/// `str.isspace()` for one character.
fn is_py_space(c: char) -> bool {
	c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// The Unicode script code of an OpenType script tag; `None` for `DFLT` and
/// tags without a Unicode script. Fails if the tag is empty, contains a space
/// or is longer than four characters, after stripping surrounding white space
/// (`ot_tag_to_script`).
///
/// ```
/// use fontoxide_unicodedata::ot_tag_to_script;
/// assert_eq!(ot_tag_to_script("bng2"), Ok(Some("Beng")));
/// assert_eq!(ot_tag_to_script("nko "), Ok(Some("Nkoo")));
/// assert_eq!(ot_tag_to_script("DFLT"), Ok(None));
/// assert!(ot_tag_to_script("zzzzz").is_err());
/// ```
pub fn ot_tag_to_script(tag: &str) -> Result<Option<&'static str>, InvalidTagError> {
	let tag = tag.trim_matches(is_py_space);
	if tag.is_empty() || tag.contains(' ') || tag.chars().count() > 4 {
		return Err(InvalidTagError(tag.to_owned()));
	}
	let tag = lookup(&SCRIPT_ALIASES, tag).unwrap_or(tag);
	let mut tag: Vec<char> = tag.chars().collect();
	// pad with spaces
	tag.resize(4, ' ');
	let tag_str: String = tag.iter().collect();
	if tag_str == DEFAULT_SCRIPT {
		// it's unclear which Unicode script the "DFLT" OpenType tag maps to
		return Ok(None);
	}
	if let Some(&(code, _)) = NEW_SCRIPT_TAGS.iter().find(|(_, t)| *t == tag_str) {
		return Ok(Some(code));
	}
	if let Some(code) = lookup(&SCRIPT_EXCEPTIONS_REVERSED, &tag_str) {
		return Ok(Some(code));
	}
	// This side of the conversion is fully algorithmic: any spaces at the end
	// of the tag are replaced by repeating the last letter (e.g. 'nko ' ->
	// 'Nkoo'), and the first character is uppercased.
	let mut script_code: Vec<char> = tag[0].to_uppercase().collect();
	script_code.push(tag[1]);
	for i in 2..4 {
		let c = if tag[i] == ' ' {
			script_code[i - 1]
		} else {
			tag[i]
		};
		script_code.push(c);
	}
	let script_code: String = script_code.into_iter().collect();
	Ok(known_script_code(&script_code))
}
