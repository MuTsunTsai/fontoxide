//! `Tests/pens/svgPathPen_test.py`: the test of `main`, which is
//! [`fontoxide_cli::svg_path_pen`] (the other tests are in `fontoxide-pens`).

use std::path::PathBuf;

use fontoxide_cli::svg_path_pen::run;
use fontoxide_ttlib::Font;
use fontoxide_ttlib::beyond64k::{ConvertOptions, upper_tables};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn test_main_beyond64k_hhea() -> TestResult {
	let data_dir =
		PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../upstream/fonttools/Tests/ttLib/data");
	let mut font = Font::new();
	font.import_ttx_file(data_dir.join("TestTTF-Regular.ttx"))?;
	upper_tables(&mut font, &ConvertOptions::default())?;

	let tmp_path =
		std::env::temp_dir().join(format!("fontoxide-cli-svg-path-pen-{}", std::process::id()));
	std::fs::create_dir_all(&tmp_path)?;
	let font_path = tmp_path.join("TestTTF-Regular.ttf");
	std::fs::write(&font_path, font.write(&Default::default())?)?;

	let mut out = Vec::new();
	let result = run(
		&[&font_path.to_string_lossy(), "--glyphs", "period"],
		&mut out,
	);
	let _ = std::fs::remove_dir_all(&tmp_path);
	assert_eq!(result?, 0);

	let output = String::from_utf8(out)?;
	assert!(output.contains("<svg "), "{output}");
	assert!(output.contains("<path "), "{output}");
	Ok(())
}
