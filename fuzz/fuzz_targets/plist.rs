//! `plistlib.loads` on bytes (encoding detection included), and a round trip of
//! what it reads.

#![no_main]

use fontoxide::misc::plistlib;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
	if let Ok(value) = plistlib::loads_bytes(data) {
		let _ = plistlib::dumps(&value, true, true);
	}
});
