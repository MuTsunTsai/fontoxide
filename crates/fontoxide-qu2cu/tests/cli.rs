//! `Tests/qu2cu/qu2cu_cli_test.py`.

use std::path::{Path, PathBuf};

use fontoxide_misc::Tag;
use fontoxide_qu2cu::cli::{Error, Options, font_to_cubic, main};
use fontoxide_ttlib::beyond64k::{ConvertOptions, upper_tables};
use fontoxide_ttlib::tables::Head;
use fontoxide_ttlib::{DefaultTable, Font, ReadOptions, WriteOptions};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn data_dir() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../upstream/fonttools/Tests/qu2cu/data")
}

const TEST_TTF: &str = "NotoSansArabic-Regular.quadratic.subset.ttf";

/// A new empty temporary directory (`tmpdir`), unique to each call.
fn tmp_dir(name: &str) -> PathBuf {
	use std::sync::atomic::{AtomicUsize, Ordering};
	static COUNTER: AtomicUsize = AtomicUsize::new(0);
	let n = COUNTER.fetch_add(1, Ordering::Relaxed);
	let dir =
		std::env::temp_dir().join(format!("fontoxide-qu2cu-{name}-{}-{n}", std::process::id()));
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir)
		.unwrap_or_else(|e| panic!("cannot create {}: {e}", dir.display()));
	dir
}

/// The `test_paths` fixture: a copy of the test font.
fn test_path(name: &str) -> Result<PathBuf> {
	let path = tmp_dir(name).join(TEST_TTF);
	std::fs::copy(data_dir().join(TEST_TTF), &path)?;
	Ok(path)
}

fn s(path: &Path) -> &str {
	path.to_str().unwrap_or_default()
}

fn run_main(args: &[&str]) -> std::result::Result<i32, Error> {
	main(args)
}

fn open(path: &Path) -> Result<Font> {
	Ok(Font::read(std::fs::read(path)?, ReadOptions::default())?)
}

/// `font["head"].glyphDataFormat == 0`, `"GLYF" in font` and
/// `"glyf" not in font`.
fn check_cubic(path: &Path) -> Result {
	let mut font = open(path)?;
	assert_eq!(font.require::<Head>()?.glyph_data_format, 0);
	assert!(font.contains(Tag::new(b"GLYF")));
	assert!(!font.contains(Tag::new(b"glyf")));
	Ok(())
}

fn cubic_path(ttf_path: &Path) -> PathBuf {
	PathBuf::from(s(ttf_path).replace(".ttf", ".cubic.ttf"))
}

#[test]
fn test_no_output() -> Result {
	let ttf_path = test_path("no_output")?;
	run_main(&[s(&ttf_path)])?;
	let output_path = cubic_path(&ttf_path);
	check_cubic(&output_path)?;
	assert!(std::fs::metadata(&ttf_path)?.len() > std::fs::metadata(&output_path)?.len());
	Ok(())
}

#[test]
fn test_output_file() -> Result {
	let ttf_path = test_path("output_file")?;
	let output_path = PathBuf::from(format!("{}.cubic", s(&ttf_path)));
	run_main(&[s(&ttf_path), "-o", s(&output_path)])?;
	check_cubic(&output_path)
}

#[test]
fn test_stats() -> Result {
	let ttf_path = test_path("stats")?;
	run_main(&[s(&ttf_path), "--verbose"])?;
	Ok(())
}

#[test]
fn test_all_cubic() -> Result {
	let ttf_path = test_path("all_cubic")?;
	run_main(&[s(&ttf_path), "-c"])?;
	check_cubic(&cubic_path(&ttf_path))
}

#[test]
fn test_beyond64k_glyf() -> Result {
	let ttf_path = test_path("beyond64k_glyf")?;
	let mut font = open(&ttf_path)?;
	upper_tables(&mut font, &ConvertOptions::default())?;
	std::fs::write(&ttf_path, font.write(&WriteOptions::default())?)?;
	run_main(&[s(&ttf_path)])?;
	check_cubic(&cubic_path(&ttf_path))
}

/// `FakeTTFont`: a font with `head` and a variation table.
fn rejects(tag: &[u8; 4]) -> Result {
	let mut font = open(&data_dir().join(TEST_TTF))?;
	font.insert(Box::new(DefaultTable::new(Tag::new(tag), Vec::new())));
	match font_to_cubic(&mut font, &Options::default()) {
		Err(Error::Value(m)) => assert!(m.contains("Cannot convert variable font")),
		other => panic!("expected ValueError, got {other:?}"),
	}
	Ok(())
}

#[test]
fn test_rejects_variable_fonts() -> Result {
	rejects(b"gvar")
}

#[test]
fn test_rejects_beyond64k_variable_fonts() -> Result {
	rejects(b"GVAR")
}

#[test]
fn test_rejects_non_positive_conversion_error() {
	let path = data_dir().join(TEST_TTF);
	match run_main(&["--conversion-error", "0", s(&path)]) {
		Err(Error::Usage(m)) => assert!(
			m.ends_with("qu2cu: error: --conversion-error must be greater than zero"),
			"{m}"
		),
		other => panic!("expected a usage error, got {other:?}"),
	}
}

// Beyond the upstream tests.

/// The converted fonts are byte for byte those of fontTools
/// (`data/cubic.ttf` and `data/all-cubic.ttf`, made by
/// `tools/reference/qu2cu_cli_fixtures.py` with `SOURCE_DATE_EPOCH=1700000000`).
#[test]
fn test_output_matches_fonttools() -> Result {
	fontoxide_ttlib::set_clock(|| 1_700_000_000.0);
	for (name, args) in [("cubic.ttf", &[][..]), ("all-cubic.ttf", &["-c"][..])] {
		let out = tmp_dir("matches_fonttools").join(name);
		let input = data_dir().join(TEST_TTF);
		let mut all = vec![s(&input), "-o", s(&out)];
		all.extend(args);
		run_main(&all)?;
		let expected = std::fs::read(
			PathBuf::from(env!("CARGO_MANIFEST_DIR"))
				.join("tests/data")
				.join(name),
		)?;
		assert!(
			std::fs::read(&out)? == expected,
			"{name} differs from fontTools'"
		);
	}
	Ok(())
}
