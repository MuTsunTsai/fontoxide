//! The tests of `Tests/otlLib/optimize_test.py` that need `FontBuilder`.

use fontoxide::font_builder::FontBuilder;
use fontoxide::misc::Tag;
use fontoxide::otllib::optimize;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A unique directory under the temporary one, removed when dropped.
struct TempDir(std::path::PathBuf);

impl TempDir {
	fn new(name: &str) -> std::io::Result<Self> {
		let path =
			std::env::temp_dir().join(format!("fontoxide-optimize-{}-{name}", std::process::id()));
		std::fs::create_dir_all(&path)?;
		Ok(TempDir(path))
	}
}

impl Drop for TempDir {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

/// `test_no_crash_with_missing_gpos`: the command line handles fonts without
/// GPOS.
#[test]
fn test_no_crash_with_missing_gpos() -> TestResult {
	let mut fb = FontBuilder::new(1000, true, false)?;
	fb.setup_glyph_order(
		".notdef space A Aacute B D"
			.split(' ')
			.map(str::to_owned)
			.collect(),
	);
	assert!(!fb.font.contains(Tag::new(b"GPOS")));
	let dir = TempDir::new("missing-gpos")?;
	let input = dir.0.join("in.ttf");
	fb.save_to(&input)?;
	let output = dir.0.join("out.ttf");
	let (input, output) = (
		input.to_string_lossy().into_owned(),
		output.to_string_lossy().into_owned(),
	);
	let ret = optimize::main(&["--gpos-compression-level", "5", &input, "-o", &output])?;
	assert_eq!(ret, 0);
	assert!(std::path::Path::new(&output).exists());
	Ok(())
}
