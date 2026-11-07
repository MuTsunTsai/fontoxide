//! Port of `fontTools.misc.encodingTools`: the text encoding of a platform /
//! encoding / language ID triplet.

/// The Python codec name for an OpenType platformID / platEncID / langID triplet,
/// or `None` if it is not known (`getEncoding`).
///
/// The names are those `fontoxide_encodings::codecs` understands.
///
/// ```
/// use fontoxide_misc::encoding_tools::get_encoding;
/// assert_eq!(get_encoding(3, 1, 0x409), Some("utf_16_be"));
/// assert_eq!(get_encoding(1, 0, 15), Some("mac_iceland"));
/// assert_eq!(get_encoding(1, 0, 0), Some("mac_roman"));
/// assert_eq!(get_encoding(3, 7, 0), None);
/// ```
pub fn get_encoding(platform_id: u16, plat_enc_id: u16, lang_id: u16) -> Option<&'static str> {
	Some(match platform_id {
		// Unicode
		0 => match plat_enc_id {
			0..=6 => "utf_16_be",
			_ => return None,
		},
		// Macintosh; see https://github.com/fonttools/fonttools/issues/236
		1 => match plat_enc_id {
			0 => match lang_id {
				15 => "mac_iceland",
				17 => "mac_turkish",
				18 => "mac_croatian",
				24..=28 | 36 | 38..=40 => "mac_latin2",
				37 => "mac_romanian",
				_ => "mac_roman",
			},
			1 => "x_mac_japanese_ttx",
			2 => "x_mac_trad_chinese_ttx",
			3 => "x_mac_korean_ttx",
			6 => "mac_greek",
			7 => "mac_cyrillic",
			25 => "x_mac_simp_chinese_ttx",
			29 => "mac_latin2",
			35 => "mac_turkish",
			37 => "mac_iceland",
			_ => return None,
		},
		// ISO
		2 => match plat_enc_id {
			0 => "ascii",
			1 => "utf_16_be",
			2 => "latin1",
			_ => return None,
		},
		// Microsoft
		3 => match plat_enc_id {
			0 | 1 | 10 => "utf_16_be",
			2 => "shift_jis",
			3 => "gb2312",
			4 => "big5",
			5 => "euc_kr",
			6 => "johab",
			_ => return None,
		},
		_ => return None,
	})
}
