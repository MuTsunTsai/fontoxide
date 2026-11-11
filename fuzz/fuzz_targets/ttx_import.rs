//! `Font::import_ttx` on arbitrary text (without reading the files that `src`
//! attributes refer to), then compile the font.

#![no_main]

use fontoxide::ttlib::{Error, Font, TtxImportOptions, WriteOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
	let text = String::from_utf8_lossy(data);
	let mut font = Font::new();
	let mut load = |_: &std::path::Path| -> Result<String, Error> {
		Err(Error::ttlib("no external files while fuzzing"))
	};
	if font
		.import_ttx_split(&text, None, &TtxImportOptions::default(), &mut load)
		.is_err()
	{
		return;
	}
	let _ = font.write(&WriteOptions::default());
});
