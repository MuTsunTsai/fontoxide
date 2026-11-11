//! Port of `fontTools.merge.unicode`.

/// Whether a code point has the `Default_Ignorable_Code_Point` property of
/// Unicode 14.0 (`is_Default_Ignorable`).
pub fn is_default_ignorable(u: u32) -> bool {
	matches!(
		u,
		0x00AD // SOFT HYPHEN
			| 0x034F // COMBINING GRAPHEME JOINER
			| 0x061C // ARABIC LETTER MARK
			| 0x115F..=0x1160 // HANGUL CHOSEONG FILLER..HANGUL JUNGSEONG FILLER
			| 0x17B4..=0x17B5 // KHMER VOWEL INHERENT AQ..KHMER VOWEL INHERENT AA
			| 0x180B..=0x180D // MONGOLIAN FREE VARIATION SELECTOR ONE..THREE
			| 0x180E // MONGOLIAN VOWEL SEPARATOR
			| 0x180F // MONGOLIAN FREE VARIATION SELECTOR FOUR
			| 0x200B..=0x200F // ZERO WIDTH SPACE..RIGHT-TO-LEFT MARK
			| 0x202A..=0x202E // LEFT-TO-RIGHT EMBEDDING..RIGHT-TO-LEFT OVERRIDE
			| 0x2060..=0x2064 // WORD JOINER..INVISIBLE PLUS
			| 0x2065 // <reserved-2065>
			| 0x2066..=0x206F // LEFT-TO-RIGHT ISOLATE..NOMINAL DIGIT SHAPES
			| 0x3164 // HANGUL FILLER
			| 0xFE00..=0xFE0F // VARIATION SELECTOR-1..VARIATION SELECTOR-16
			| 0xFEFF // ZERO WIDTH NO-BREAK SPACE
			| 0xFFA0 // HALFWIDTH HANGUL FILLER
			| 0xFFF0..=0xFFF8 // <reserved-FFF0>..<reserved-FFF8>
			| 0x1BCA0..=0x1BCA3 // SHORTHAND FORMAT LETTER OVERLAP..SHORTHAND FORMAT UP STEP
			| 0x1D173..=0x1D17A // MUSICAL SYMBOL BEGIN BEAM..MUSICAL SYMBOL END PHRASE
			| 0xE0000 // <reserved-E0000>
			| 0xE0001 // LANGUAGE TAG
			| 0xE0002..=0xE001F // <reserved-E0002>..<reserved-E001F>
			| 0xE0020..=0xE007F // TAG SPACE..CANCEL TAG
			| 0xE0080..=0xE00FF // <reserved-E0080>..<reserved-E00FF>
			| 0xE0100..=0xE01EF // VARIATION SELECTOR-17..VARIATION SELECTOR-256
			| 0xE01F0..=0xE0FFF // <reserved-E01F0>..<reserved-E0FFF>
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn default_ignorable() {
		for u in [0xAD, 0x115F, 0x1160, 0x2065, 0xFE0F, 0xE0FFF] {
			assert!(is_default_ignorable(u), "{u:#X}");
		}
		for u in [0x20, 0x115E, 0x1161, 0x25CC, 0xFFF9, 0xE1000] {
			assert!(!is_default_ignorable(u), "{u:#X}");
		}
	}
}
