//! Port of fontTools' `Tests/agl_test.py`.

use fontoxide_misc::agl::{agl2uv, to_unicode, uv2agl};

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
