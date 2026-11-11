//! Inputs found by fuzzing (`fuzz/artifacts`), replayed as the `font_read`
//! fuzz target does: they used to exhaust memory or run for minutes.

use std::time::{Duration, Instant};

use fontoxide_ttlib::{Font, ReadOptions, TtxOptions, WriteOptions};

fn artifacts() -> std::path::PathBuf {
	std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/artifacts/font_read")
}

#[test]
fn font_read_artifacts_finish_quickly() {
	let mut names: Vec<_> = std::fs::read_dir(artifacts())
		.expect("fuzz/artifacts/font_read")
		.map(|e| e.expect("entry").path())
		.collect();
	names.sort();
	assert!(!names.is_empty());
	for path in names {
		let data = std::fs::read(&path).expect("artifact");
		let start = Instant::now();
		let options = ReadOptions {
			font_number: Some(0),
			recalc_timestamp: false,
			..ReadOptions::default()
		};
		if let Ok(mut font) = Font::read(data, options) {
			font.flavor = None;
			let _ = font.ensure_decompiled();
			let _ = font.to_ttx(&TtxOptions::default());
			let _ = font.write(&WriteOptions::default());
		}
		// Generous for debug builds; the inputs took minutes before.
		assert!(
			start.elapsed() < Duration::from_secs(20),
			"{} took {:?}",
			path.display(),
			start.elapsed()
		);
	}
}
