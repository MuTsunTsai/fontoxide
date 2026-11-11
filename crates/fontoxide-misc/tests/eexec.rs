//! `Tests/misc/eexec_test.py`, and the doctests of `fontTools.misc.eexec`.

use fontoxide_misc::eexec::{decrypt, encrypt};

#[test]
fn test_decrypt() {
	let (decrypted, r) = decrypt(b"\0\0asdadads asds\xb5", 12321);
	assert_eq!(decrypted, b"0d\nh\x15\xe8\xc4\xb2\x15\x1d\x108\x1a<6\xa1");
	assert_eq!(r, 36142);
}

#[test]
fn test_encrypt() {
	let (encrypted, r) = encrypt(b"0d\nh\x15\xe8\xc4\xb2\x15\x1d\x108\x1a<6\xa1", 12321);
	assert_eq!(encrypted, b"\0\0asdadads asds\xb5");
	assert_eq!(r, 36142);
}

/// The doctests of `decrypt` and `encrypt` (the same values as above).
#[test]
fn eexec_doctests() {
	let (decrypted, r) = decrypt(b"\0\0asdadads asds\xb5", 12321);
	let (encrypted, r2) = encrypt(&decrypted, 12321);
	assert_eq!(encrypted, b"\0\0asdadads asds\xb5");
	assert_eq!((r, r2), (36142, 36142));
}
