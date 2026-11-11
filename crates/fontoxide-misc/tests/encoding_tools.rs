//! `Tests/misc/encodingTools_test.py`.

use fontoxide_misc::encoding_tools::get_encoding;

#[test]
fn test_encoding_unicode() {
	// MS Symbol is Unicode as well
	assert_eq!(get_encoding(3, 0, 0), Some("utf_16_be"));
	assert_eq!(get_encoding(3, 1, 0), Some("utf_16_be"));
	assert_eq!(get_encoding(3, 10, 0), Some("utf_16_be"));
	assert_eq!(get_encoding(0, 3, 0), Some("utf_16_be"));
}

#[test]
fn test_encoding_macroman_misc() {
	assert_eq!(get_encoding(1, 0, 17), Some("mac_turkish"));
	assert_eq!(get_encoding(1, 0, 37), Some("mac_romanian"));
	assert_eq!(get_encoding(1, 0, 45), Some("mac_roman"));
}

#[test]
fn test_extended_mac_encodings() {
	// Mac Japanese
	let encoding = get_encoding(1, 1, 0).expect("known encoding");
	assert_eq!(
		fontoxide_encodings::codecs::decode(encoding, b"\xfe").as_deref(),
		Ok("\u{2122}")
	);
}

#[test]
fn test_extended_unknown() {
	assert_eq!(get_encoding(10, 11, 12), None);
	// `getEncoding(..., default="ascii")`
	assert_eq!(get_encoding(10, 11, 12).unwrap_or("ascii"), "ascii");
}
