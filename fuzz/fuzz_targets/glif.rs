//! ufoLib: `readGlyphFromString` into a glyph object and a point pen.

#![no_main]

use fontoxide::pens::RecordingPointPen;
use fontoxide::ufolib::glif_lib::{Glyph, PointPenAdapter, read_glyph_from_string};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
	for validate in [true, false] {
		let mut glyph = Glyph::default();
		let mut recording = RecordingPointPen::default();
		let mut pen = PointPenAdapter(&mut recording);
		let _ = read_glyph_from_string(data, Some(&mut glyph), Some(&mut pen), None, validate);
	}
});
