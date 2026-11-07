//! Port of `fontTools.agl`: the Adobe Glyph List.
//!
//! Converts glyph names to Unicode following the
//! [AGL specification](https://github.com/adobe-type-tools/agl-specification), and
//! provides the AGLFN mappings fontTools uses to name glyphs.
//!
//! The data files in `data/` are copied verbatim from fontTools by
//! `cargo xtask codegen`.

use std::collections::HashMap;
use std::sync::OnceLock;

const AGL_TEXT: &str = include_str!("../data/glyphlist.txt");
const AGLFN_TEXT: &str = include_str!("../data/aglfn.txt");
const ZAPF_DINGBATS: &str = include_str!("../data/zapfdingbats.txt");

struct Dicts {
	legacy_agl2uv: HashMap<&'static str, Vec<u32>>,
	agl2uv: HashMap<&'static str, u32>,
	uv2agl: HashMap<u32, &'static str>,
}

fn dicts() -> &'static Dicts {
	static DICTS: OnceLock<Dicts> = OnceLock::new();
	DICTS.get_or_init(|| {
		let mut d = Dicts {
			legacy_agl2uv: HashMap::new(),
			agl2uv: HashMap::new(),
			uv2agl: HashMap::new(),
		};
		for line in AGL_TEXT.lines() {
			if line.is_empty() || line.starts_with('#') {
				continue;
			}
			let Some((name, codes)) = line.split_once(';') else {
				continue;
			};
			let unicodes = codes
				.split(' ')
				.filter_map(|c| u32::from_str_radix(c, 16).ok())
				.collect();
			d.legacy_agl2uv.insert(name, unicodes);
		}
		for line in AGLFN_TEXT.lines() {
			if line.is_empty() || line.starts_with('#') {
				continue;
			}
			let mut fields = line.splitn(3, ';');
			let (Some(code), Some(name)) = (fields.next(), fields.next()) else {
				continue;
			};
			let Ok(code) = u32::from_str_radix(code, 16) else {
				continue;
			};
			d.agl2uv.insert(name, code);
			d.uv2agl.insert(code, name);
		}
		d
	})
}

/// The AGLFN name for a code point (`UV2AGL[codepoint]`).
pub fn uv2agl(codepoint: u32) -> Option<&'static str> {
	dicts().uv2agl.get(&codepoint).copied()
}

/// The code point of an AGLFN name (`AGL2UV[name]`).
pub fn agl2uv(name: &str) -> Option<u32> {
	dicts().agl2uv.get(name).copied()
}

/// The code points of a name in the legacy AGL (`LEGACY_AGL2UV[name]`).
pub fn legacy_agl2uv(name: &str) -> Option<&'static [u32]> {
	dicts().legacy_agl2uv.get(name).map(Vec::as_slice)
}

/// All AGLFN entries, code point to name (`UV2AGL`).
pub fn uv2agl_map() -> &'static HashMap<u32, &'static str> {
	&dicts().uv2agl
}

/// All AGLFN entries, name to code point (`AGL2UV`).
pub fn agl2uv_map() -> &'static HashMap<&'static str, u32> {
	&dicts().agl2uv
}

/// Converts a glyph name to Unicode, e.g. `"longs_t.oldstyle"` → `"ſt"`
/// (`toUnicode`).
///
/// With `is_zapf_dingbats`, the ITC Zapf Dingbats names (`a1`, `a2`, …) are
/// recognized too.
///
/// ```
/// assert_eq!(fontoxide_agl::to_unicode("f_f_i.alt123", false), "ffi");
/// ```
pub fn to_unicode(glyph: &str, is_zapf_dingbats: bool) -> String {
	let glyph = glyph.split('.').next().unwrap_or_default();
	glyph
		.split('_')
		.map(|c| component_to_unicode(c, is_zapf_dingbats))
		.collect()
}

fn component_to_unicode(component: &str, is_zapf_dingbats: bool) -> String {
	if is_zapf_dingbats && let Some(c) = zapf_dingbats_to_unicode(component) {
		return c.to_string();
	}
	if let Some(chars) = legacy_agl2uv(component)
		&& !chars.is_empty()
	{
		return chars.iter().filter_map(|&c| char::from_u32(c)).collect();
	}
	if let Some(s) = uni_to_unicode(component) {
		return s;
	}
	if let Some(c) = u_to_unicode(component) {
		return c.to_string();
	}
	String::new()
}

fn zapf_dingbats_to_unicode(glyph: &str) -> Option<char> {
	let digits = glyph.strip_prefix('a').filter(|d| !d.is_empty())?;
	let gid: i64 = digits.trim().parse().ok()?;
	let gid = usize::try_from(gid).ok()?;
	let c = ZAPF_DINGBATS.chars().nth(gid)?;
	(c != ' ').then_some(c)
}

fn is_upper_hex(s: &str) -> bool {
	!s.is_empty()
		&& s.bytes()
			.all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
}

fn uni_to_unicode(component: &str) -> Option<String> {
	let digits = component.strip_prefix("uni")?;
	if !is_upper_hex(digits) || digits.len() % 4 != 0 {
		return None;
	}
	let chars: Vec<u32> = digits
		.as_bytes()
		.chunks(4)
		.map(|c| u32::from_str_radix(std::str::from_utf8(c).unwrap_or("0"), 16).unwrap_or(0))
		.collect();
	if chars.iter().any(|c| (0xD800..=0xDFFF).contains(c)) {
		return None;
	}
	Some(chars.into_iter().filter_map(char::from_u32).collect())
}

fn u_to_unicode(component: &str) -> Option<char> {
	let digits = component.strip_prefix('u')?;
	if !is_upper_hex(digits) || !(4..=6).contains(&digits.len()) {
		return None;
	}
	let value = u32::from_str_radix(digits, 16).ok()?;
	if value <= 0xD7FF || (0xE000..=0x10FFFF).contains(&value) {
		char::from_u32(value)
	} else {
		None
	}
}

#[cfg(test)]
mod tests {
	//! Port of Tests/agl_test.py.

	use super::*;

	fn u(s: &str) -> String {
		to_unicode(s, false)
	}

	#[test]
	fn test_spec_examples() {
		assert_eq!(u("Lcommaaccent"), "Ļ");
		assert_eq!(u("uni20AC0308"), "\u{20ac}\u{0308}");
		assert_eq!(u("u1040C"), "\u{1040c}");
		assert_eq!(u("uniD801DC0C"), "");
		assert_eq!(u("uni20ac"), "");
		assert_eq!(
			u("Lcommaaccent_uni20AC0308_u1040C.alternate"),
			"\u{013b}\u{20ac}\u{0308}\u{1040c}"
		);
		assert_eq!(u("Lcommaaccent_uni013B_u013B"), "ĻĻĻ");
		assert_eq!(u("foo"), "");
		assert_eq!(u(".notdef"), "");
	}

	#[test]
	fn test_aglfn() {
		assert_eq!(u("longs_t"), "ſt");
		assert_eq!(u("f_f_i.alt123"), "ffi");
	}

	#[test]
	#[allow(non_snake_case)]
	fn test_uniABCD() {
		assert_eq!(u("uni0041"), "A");
		assert_eq!(u("uni0041_uni0042_uni0043"), "ABC");
		assert_eq!(u("uni004100420043"), "ABC");
		assert_eq!(u("uni"), "");
		assert_eq!(u("uni41"), "");
		assert_eq!(u("uni004101"), "");
		assert_eq!(u("uniDC00"), "");
	}

	#[test]
	#[allow(non_snake_case)]
	fn test_uABCD() {
		assert_eq!(u("u0041"), "A");
		assert_eq!(u("u00041"), "A");
		assert_eq!(u("u000041"), "A");
		assert_eq!(u("u0000041"), "");
		assert_eq!(u("u0041_uni0041_A.alt"), "AAA");
	}

	#[test]
	fn test_union() {
		assert_eq!(u("union"), "∪");
		assert_eq!(u("union_uniFE00"), "\u{222a}\u{fe00}");
	}

	#[test]
	fn test_dingbats() {
		assert_eq!(to_unicode("a20", true), "✔");
		assert_eq!(to_unicode("a20.alt", true), "✔");
		assert_eq!(to_unicode("a206", true), "❰");
		assert_eq!(to_unicode("a20", false), "");
		assert_eq!(to_unicode("a0", true), "");
		assert_eq!(to_unicode("a207", true), "");
		assert_eq!(to_unicode("abcdef", true), "");
	}

	#[test]
	fn mappings() {
		assert_eq!(uv2agl(u32::from('?')), Some("question"));
		assert_eq!(agl2uv("wcircumflex"), Some(373));
	}
}
