//! Web fonts: the input is read as WOFF 2.0 (or WOFF 1.0, by the first byte) by
//! overwriting its signature, so that the fuzzer need not find it, then dumped
//! and compiled again in the same flavor.

#![no_main]

use fontoxide::ttlib::{Font, ReadOptions, TtxOptions, WriteOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
	let Some((&selector, rest)) = data.split_first() else {
		return;
	};
	if rest.len() < 4 {
		return;
	}
	let mut input = rest.to_vec();
	input[..4].copy_from_slice(if selector & 1 == 0 { b"wOF2" } else { b"wOFF" });
	let options = ReadOptions {
		recalc_timestamp: false,
		..ReadOptions::default()
	};
	let Ok(mut font) = Font::read(input, options) else {
		return;
	};
	let _ = font.ensure_decompiled();
	let _ = font.to_ttx(&TtxOptions::default());
	let _ = font.write(&WriteOptions {
		// The lowest Brotli quality: the transforms are what is tested.
		woff2_quality: 0,
		..WriteOptions::default()
	});
});
