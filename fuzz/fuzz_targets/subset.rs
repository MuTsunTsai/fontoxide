//! Read a font and subset it to a few fixed characters, then compile it.

#![no_main]

use fontoxide::subset::{Options, Subsetter};
use fontoxide::ttlib::{Font, ReadOptions, WriteOptions};
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
	font.flavor = None;
	let mut subsetter = Subsetter::new(Options::default());
	subsetter.populate(&[], &[], &[0x20, 0x41, 0x61, 0x66, 0x69, 0x2014, 0x4E00], "");
	if subsetter.subset(&mut font).is_ok() {
		let _ = font.write(&WriteOptions::default());
	}
});
