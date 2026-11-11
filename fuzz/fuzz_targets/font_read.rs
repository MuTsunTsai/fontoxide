//! `Font::read`, then decompile every table, dump the TTX of all tables and
//! compile the font again. Errors are fine; panics are findings.

#![no_main]

use fontoxide::ttlib::{Font, ReadOptions, TtxOptions, WriteOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
	let options = ReadOptions {
		font_number: Some(0),
		recalc_timestamp: false,
		..ReadOptions::default()
	};
	let Ok(mut font) = Font::read(data.to_vec(), options) else {
		return;
	};
	// Compile as plain sfnt: WOFF 2.0 output (Brotli) is the `woff2` target's job.
	font.flavor = None;
	let _ = font.ensure_decompiled();
	let _ = font.to_ttx(&TtxOptions::default());
	let _ = font.write(&WriteOptions::default());
});
