//! The command line of `fontTools.feaLib` (`__main__.py`), which fontTools does
//! not test; these check that it builds features into a font and reports
//! feature file errors with the exit code.

use fontoxide::fealib::cli;
use fontoxide::font_builder::FontBuilder;
use fontoxide::misc::Tag;
use fontoxide::ttlib::{Font, ReadOptions};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A unique directory under the temporary one, removed when dropped.
struct TempDir(std::path::PathBuf);

impl TempDir {
	fn new(name: &str) -> std::io::Result<Self> {
		let path = std::env::temp_dir().join(format!(
			"fontoxide-fealib-cli-{}-{name}",
			std::process::id()
		));
		std::fs::create_dir_all(&path)?;
		Ok(TempDir(path))
	}

	fn file(&self, name: &str, contents: Option<&str>) -> std::io::Result<String> {
		let path = self.0.join(name);
		if let Some(contents) = contents {
			std::fs::write(&path, contents)?;
		}
		Ok(path.to_string_lossy().into_owned())
	}
}

impl Drop for TempDir {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

fn input_font(dir: &TempDir) -> Result<String, Box<dyn std::error::Error>> {
	let mut fb = FontBuilder::new(1000, true, false)?;
	fb.setup_glyph_order(
		[".notdef", "f", "i", "f_i"]
			.iter()
			.map(|s| (*s).to_owned())
			.collect(),
	);
	// The glyph names are kept in `post`.
	fb.setup_maxp()?;
	fb.setup_post(true, &[])?;
	let path = dir.file("in.ttf", None)?;
	fb.save_to(&path)?;
	Ok(path)
}

#[test]
fn main_builds_features() -> TestResult {
	let dir = TempDir::new("build")?;
	let input = input_font(&dir)?;
	let fea = dir.file(
		"features.fea",
		Some("feature liga { sub f i by f_i; } liga;\n"),
	)?;
	let output = dir.file("out.ttf", None)?;
	assert_eq!(cli::main(&[&fea, &input, "-o", &output, "-t", "GSUB"])?, 0);
	let font = Font::read(std::fs::read(&output)?, ReadOptions::default())?;
	assert!(font.contains(Tag::new(b"GSUB")));
	Ok(())
}

#[test]
fn main_reports_feature_file_errors() -> TestResult {
	let dir = TempDir::new("error")?;
	let input = input_font(&dir)?;
	let fea = dir.file("features.fea", Some("feature liga { sub f i by; } liga;\n"))?;
	let output = dir.file("out.ttf", None)?;
	assert_eq!(cli::main(&[&fea, &input, "-o", &output])?, 1);
	assert!(!std::path::Path::new(&output).exists());
	assert!(cli::main(&[&fea, &input, "-o", &output, "--traceback"]).is_err());
	assert!(cli::main(&[&fea, &input, "-t", "FOO"]).is_err());
	Ok(())
}
