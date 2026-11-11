//! Port of `fontTools.misc.eexec`: the Type 1 charstring and `eexec` encryption.
//!
//! PostScript Type 1 fonts use the same cipher for the charstrings (initial key
//! 4330) and for the `eexec` encrypted part of the font program (initial key
//! 55665). The cipher uses feedback: the ciphertext modifies the key, so the
//! functions here return the key at the end, for continuing later.

use thiserror::Error;

/// `binascii.Error`: hexadecimal data that cannot be decoded.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct HexError(pub String);

/// Decrypts `cipher` with the initial key `r` (`decrypt`); returns the plaintext
/// and the key for a subsequent decryption.
///
/// ```
/// use fontoxide_misc::eexec::decrypt;
/// let (plain, r) = decrypt(b"\0\0asdadads asds\xb5", 12321);
/// assert_eq!(plain, b"0d\nh\x15\xe8\xc4\xb2\x15\x1d\x108\x1a<6\xa1");
/// assert_eq!(r, 36142);
/// ```
pub fn decrypt(cipher: &[u8], mut r: u16) -> (Vec<u8>, u16) {
	let mut plain = Vec::with_capacity(cipher.len());
	for &c in cipher {
		plain.push(c ^ (r >> 8) as u8);
		r = next_key(c, r);
	}
	(plain, r)
}

/// Encrypts `plain` with the initial key `r` (`encrypt`); returns the
/// ciphertext and the key for a subsequent encryption.
///
/// Like fontTools, this does not add the random bytes the Type 1 specification
/// prefixes the plaintext with.
///
/// ```
/// use fontoxide_misc::eexec::encrypt;
/// let (cipher, r) = encrypt(b"0d\nh\x15\xe8\xc4\xb2\x15\x1d\x108\x1a<6\xa1", 12321);
/// assert_eq!(cipher, b"\0\0asdadads asds\xb5");
/// assert_eq!(r, 36142);
/// ```
pub fn encrypt(plain: &[u8], mut r: u16) -> (Vec<u8>, u16) {
	let mut cipher = Vec::with_capacity(plain.len());
	for &p in plain {
		let c = p ^ (r >> 8) as u8;
		cipher.push(c);
		r = next_key(c, r);
	}
	(cipher, r)
}

fn next_key(cipher: u8, r: u16) -> u16 {
	((cipher as u32 + r as u32)
		.wrapping_mul(52845)
		.wrapping_add(22719)
		& 0xFFFF) as u16
}

/// `hexString`: `binascii.hexlify`, lowercase hexadecimal.
pub fn hex_string(s: &[u8]) -> Vec<u8> {
	const DIGITS: &[u8; 16] = b"0123456789abcdef";
	let mut out = Vec::with_capacity(s.len() * 2);
	for &b in s {
		out.push(DIGITS[(b >> 4) as usize]);
		out.push(DIGITS[(b & 0xF) as usize]);
	}
	out
}

/// `deHexString`: drops ASCII whitespace (`bytes.split()`), then
/// `binascii.unhexlify`.
pub fn de_hex_string(h: &[u8]) -> Result<Vec<u8>, HexError> {
	let digits: Vec<u8> = h.iter().copied().filter(|b| !is_ascii_space(*b)).collect();
	unhexlify(&digits)
}

/// Whitespace as `bytes.split()` sees it.
pub fn is_ascii_space(b: u8) -> bool {
	matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// `binascii.unhexlify`.
pub fn unhexlify(digits: &[u8]) -> Result<Vec<u8>, HexError> {
	if !digits.len().is_multiple_of(2) {
		return Err(HexError("Odd-length string".into()));
	}
	let value = |b: u8| -> Result<u8, HexError> {
		(b as char)
			.to_digit(16)
			.map(|d| d as u8)
			.ok_or_else(|| HexError("Non-hexadecimal digit found".into()))
	};
	digits
		.chunks(2)
		.map(|pair| Ok(value(pair[0])? << 4 | value(pair[1])?))
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn hex_round_trip() {
		assert_eq!(hex_string(b"\x01\xab"), b"01ab");
		assert_eq!(de_hex_string(b"01 A\nb"), Ok(b"\x01\xab".to_vec()));
		assert!(de_hex_string(b"0").is_err());
		assert!(de_hex_string(b"0g").is_err());
	}
}
