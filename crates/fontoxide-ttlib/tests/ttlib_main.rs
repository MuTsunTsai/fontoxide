//! `Tests/ttLib/main_test.py`: the command line, [`fontoxide_ttlib::cli::main`].

#![allow(non_snake_case)]

use std::path::{Path, PathBuf};

use fontoxide_ttlib::cli::main;
use fontoxide_ttlib::{Font, ReadOptions, TtCollection, WriteOptions};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A temporary directory, removed when dropped (pytest's `tmp_path`).
struct TempDir(PathBuf);

impl TempDir {
	fn new(name: &str) -> std::io::Result<Self> {
		let path = std::env::temp_dir().join(format!(
			"fontoxide-ttlib-main-{}-{name}",
			std::process::id()
		));
		std::fs::create_dir_all(&path)?;
		Ok(TempDir(path))
	}

	fn join(&self, name: &str) -> String {
		self.0.join(name).to_string_lossy().into_owned()
	}
}

impl Drop for TempDir {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

fn test_ttf() -> fontoxide_ttlib::Result<Font> {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttLib/data/TestTTF-Regular.ttx");
	Font::from_ttx(&std::fs::read_to_string(path)?)
}

/// The `ttfont_path` fixture.
fn ttfont_path(tmp: &TempDir) -> fontoxide_ttlib::Result<String> {
	let path = tmp.join("ttfont.ttf");
	std::fs::write(&path, test_ttf()?.write(&WriteOptions::default())?)?;
	Ok(path)
}

/// The `ttcollection_path` fixture.
fn ttcollection_path(tmp: &TempDir) -> fontoxide_ttlib::Result<String> {
	let mut collection = TtCollection {
		fonts: vec![test_ttf()?, test_ttf()?],
		..Default::default()
	};
	let path = tmp.join("ttcollection.ttc");
	std::fs::write(&path, collection.write(true)?)?;
	Ok(path)
}

fn read_font(path: impl AsRef<Path>) -> fontoxide_ttlib::Result<Font> {
	Font::read(std::fs::read(path)?, ReadOptions::default())
}

fn read_collection(path: impl AsRef<Path>) -> fontoxide_ttlib::Result<TtCollection> {
	TtCollection::read(std::fs::read(path)?, ReadOptions::default())
}

fn glyph_order(font: &mut Font) -> fontoxide_ttlib::Result<Vec<String>> {
	Ok(font.glyph_order()?.names().to_vec())
}

#[test]
fn test_ttLib_open_ttfont() -> TestResult {
	let tmp = TempDir::new("open_ttfont")?;
	let path = ttfont_path(&tmp)?;
	assert_eq!(main(&[&path])?, 0);
	Ok(())
}

#[test]
fn test_ttLib_open_save_ttfont() -> TestResult {
	for flavor in [None, Some("woff")] {
		let tmp = TempDir::new(&format!("open_save_ttfont_{}", flavor.unwrap_or("none")))?;
		let path = ttfont_path(&tmp)?;
		let output = tmp.join("TestTTF-Regular.ttf");
		let mut args = vec!["-o", &output, &path];
		if let Some(flavor) = flavor {
			args.extend(["--flavor", flavor]);
		}
		assert_eq!(main(&args)?, 0);

		let mut saved = read_font(&output)?;
		assert_eq!(saved.flavor.map(|f| f.as_str()), flavor);
		assert_eq!(
			glyph_order(&mut saved)?,
			glyph_order(&mut read_font(&path)?)?
		);
	}
	Ok(())
}

#[test]
fn test_ttLib_open_ttcollection() -> TestResult {
	let tmp = TempDir::new("open_ttcollection")?;
	let path = ttcollection_path(&tmp)?;
	assert_eq!(main(&["-y", "0", &path])?, 0);
	Ok(())
}

#[test]
fn test_ttLib_open_ttcollection_save_single_font() -> TestResult {
	for flavor in [None, Some("woff")] {
		let tmp = TempDir::new(&format!(
			"open_ttcollection_save_single_font_{}",
			flavor.unwrap_or("none")
		))?;
		let path = ttcollection_path(&tmp)?;
		for i in 0..2 {
			let output = tmp.join(&format!("TestTTF-Regular#{i}.ttf"));
			let number = i.to_string();
			let mut args = vec!["-y", &number, "-o", &output, &path];
			if let Some(flavor) = flavor {
				args.extend(["--flavor", flavor]);
			}
			assert_eq!(main(&args)?, 0);

			let mut saved = read_font(&output)?;
			let mut collection = read_collection(&path)?;
			assert_eq!(
				glyph_order(&mut saved)?,
				glyph_order(&mut collection.fonts[i])?
			);
		}
	}
	Ok(())
}

#[test]
fn test_ttLib_open_ttcollection_save_ttcollection() -> TestResult {
	let tmp = TempDir::new("open_ttcollection_save_ttcollection")?;
	let path = ttcollection_path(&tmp)?;
	let output = tmp.join("TestTTF.ttc");
	assert_eq!(main(&["-o", &output, &path])?, 0);
	assert_eq!(
		read_collection(&output)?.fonts.len(),
		read_collection(&path)?.fonts.len()
	);
	Ok(())
}

#[test]
fn test_ttLib_open_multiple_fonts_save_ttcollection() -> TestResult {
	let tmp = TempDir::new("open_multiple_fonts_save_ttcollection")?;
	let path = ttfont_path(&tmp)?;
	let output = tmp.join("TestTTF.ttc");
	assert_eq!(main(&["-o", &output, &path, &path])?, 0);

	let mut collection = read_collection(&output)?;
	assert_eq!(collection.fonts.len(), 2);
	let order0 = glyph_order(&mut collection.fonts[0])?;
	assert_eq!(order0, glyph_order(&mut collection.fonts[1])?);
	Ok(())
}

/// `--optimize-font-speed` is not supported (fontoxide has no
/// `OPTIMIZE_FONT_SPEED` option), and rejected rather than ignored.
#[test]
fn optimize_font_speed_is_rejected() -> TestResult {
	let tmp = TempDir::new("optimize_font_speed")?;
	let path = ttfont_path(&tmp)?;
	let output = tmp.join("out.ttf");
	assert!(main(&["--optimize-font-speed", "-o", &output, &path]).is_err());
	Ok(())
}
