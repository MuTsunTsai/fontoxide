//! `Tests/encodings/codecs_test.py`.

use fontoxide_encodings::codecs::{decode, encode};

#[test]
fn test_decode_mac_japanese() {
	assert_eq!(
		decode("x_mac_japanese_ttx", b"x\xfe\xfdy").as_deref(),
		Ok("x\u{2122}\u{00A9}y")
	);
}

#[test]
fn test_encode_mac_japanese() {
	assert_eq!(
		encode("x_mac_japanese_ttx", "x\u{2122}\u{00A9}y").as_deref(),
		Ok(&b"x\xfe\xfdy"[..])
	);
}

#[test]
fn test_decode_mac_trad_chinese() {
	assert_eq!(
		decode("x_mac_trad_chinese_ttx", b"\x80").as_deref(),
		Ok("\\")
	);
}

#[test]
fn test_decode_mac_romanian() {
	assert_eq!(decode("mac_romanian", b"x\xfb").as_deref(), Ok("x\u{02DA}"));
}
