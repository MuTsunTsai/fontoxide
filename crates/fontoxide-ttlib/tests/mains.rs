//! fontoxide's own tests (fontTools has none for these): the command lines
//! `cffLib.width`, `cffLib.CFF2ToCFF`, `ttLib.removeOverlaps` and
//! `ttLib.scaleUpem`, compared with fontTools' (the fixtures in `data/mains` are
//! made by `tools/reference/ttlib_mains_fixtures.py`, with the clock fixed).

use std::path::{Path, PathBuf};

use fontoxide_ttlib::cff::{cff2_to_cff, width};
#[cfg(feature = "pathops")]
use fontoxide_ttlib::tables::Hmtx;
#[cfg(feature = "pathops")]
use fontoxide_ttlib::{Font, ReadOptions};
use fontoxide_ttlib::{remove_overlaps, scale_upem};

type TestResult = Result<(), Box<dyn std::error::Error>>;

type Run =
	fn(&[&str], &mut dyn std::io::Write, &mut dyn std::io::Write) -> fontoxide_ttlib::Result<i32>;

fn upstream(path: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests")
		.join(path)
}

fn fixture(name: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data/mains")
		.join(name)
}

/// A new empty temporary directory, unique to each call.
fn tmp_dir(name: &str) -> PathBuf {
	use std::sync::atomic::{AtomicUsize, Ordering};
	static COUNTER: AtomicUsize = AtomicUsize::new(0);
	let n = COUNTER.fetch_add(1, Ordering::Relaxed);
	let dir =
		std::env::temp_dir().join(format!("fontoxide-mains-{name}-{}-{n}", std::process::id()));
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir)
		.unwrap_or_else(|e| panic!("cannot create {}: {e}", dir.display()));
	dir
}

fn s(path: &Path) -> &str {
	path.to_str().unwrap_or_default()
}

/// Runs `run` and formats its exit code and output as the `<case>.txt` fixtures.
fn text_case(run: Run, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
	let (mut out, mut err) = (Vec::new(), Vec::new());
	let code = run(args, &mut out, &mut err)?;
	Ok(format!(
		"exit {code}\n--- stdout\n{}--- stderr\n{}",
		String::from_utf8(out)?,
		String::from_utf8(err)?
	))
}

fn check_text_case(case: &str, run: Run, args: &[&str]) -> TestResult {
	let expected = std::fs::read_to_string(fixture(&format!("{case}.txt")))?;
	assert_eq!(text_case(run, args)?, expected, "{case}");
	Ok(())
}

/// The clock of `tools/reference/ttlib_mains_fixtures.py`.
fn fix_clock() {
	fontoxide_ttlib::set_clock(|| 1_700_000_000.0);
}

#[test]
fn width_texts() -> TestResult {
	check_text_case("width-help", width::run, &["-h"])?;
	check_text_case("width-no-args", width::run, &[])
}

#[test]
fn width_run() -> TestResult {
	let i = upstream("ttLib/data/I.otf");
	let libertine = upstream("cffLib/data/LinLibertine_RBI.otf");
	let mut out = Vec::new();
	assert_eq!(
		width::run(&[s(&i), s(&libertine)], &mut out, &mut Vec::new())?,
		0
	);
	assert_eq!(width::run(&["-b", s(&i)], &mut out, &mut Vec::new())?, 0);
	assert_eq!(
		String::from_utf8(out)?,
		std::fs::read_to_string(fixture("width.txt"))?
	);
	Ok(())
}

#[test]
fn cff2_to_cff_texts() -> TestResult {
	check_text_case("CFF2ToCFF-help", cff2_to_cff::run, &["-h"])?;
	check_text_case("CFF2ToCFF-no-file", cff2_to_cff::run, &["missing.otf"])?;
	check_text_case(
		"CFF2ToCFF-verbose-quiet",
		cff2_to_cff::run,
		&["-v", "-q", "missing.otf"],
	)
}

#[test]
fn cff2_to_cff_run() -> TestResult {
	fix_clock();
	let output = tmp_dir("cff2-to-cff").join("out.otf");
	let input = fixture("CFFToCFF2-1.CFF2.otf");
	let code = cff2_to_cff::run(
		&[s(&input), "-o", s(&output)],
		&mut Vec::new(),
		&mut Vec::new(),
	)?;
	assert_eq!(code, 0);
	assert!(std::fs::read(&output)? == std::fs::read(fixture("CFFToCFF2-1.CFF.otf"))?);
	Ok(())
}

/// UPSTREAM-BUG(B26): a CFF2 font with an FDSelect fails, as in fontTools 4.68.0.
#[test]
fn cff2_to_cff_fd_select() -> TestResult {
	let output = tmp_dir("cff2-to-cff-fdselect").join("out.otf");
	let input = upstream("cffLib/data/varc-short-cff2.otf");
	let result = cff2_to_cff::run(
		&[s(&input), "-o", s(&output)],
		&mut Vec::new(),
		&mut Vec::new(),
	);
	let message = result.err().map(|e| e.to_string()).unwrap_or_default();
	assert!(
		message.contains("TypeError: 'NoneType' object is not callable"),
		"{message}"
	);
	Ok(())
}

/// A variable font fails before the FDSelect is read.
#[test]
fn cff2_to_cff_variable() {
	let output = tmp_dir("cff2-to-cff-variable").join("out.otf");
	let input = upstream("ttLib/data/I.otf");
	let result = cff2_to_cff::run(
		&[s(&input), "-o", s(&output)],
		&mut Vec::new(),
		&mut Vec::new(),
	);
	let message = result.err().map(|e| e.to_string()).unwrap_or_default();
	assert!(
		message.contains("Variable CFF2 font cannot be converted to CFF format."),
		"{message}"
	);
}

#[test]
fn scale_upem_texts() -> TestResult {
	check_text_case("scaleUpem-help", scale_upem::run, &["-h"])?;
	check_text_case("scaleUpem-no-upem", scale_upem::run, &["font.ttf"])
}

/// `GSUB` and `GPOS` are packed with the HarfBuzz repacker, as fontTools does with
/// uharfbuzz.
#[test]
#[cfg(feature = "repacker")]
fn scale_upem_run() -> TestResult {
	fix_clock();
	let output = tmp_dir("scale-upem").join("scaled.ttf");
	let input = upstream("ttLib/data/I.ttf");
	let mut out = Vec::new();
	let code = scale_upem::run(
		&[s(&input), "512", "--output-file", s(&output)],
		&mut out,
		&mut Vec::new(),
	)?;
	assert_eq!(code, 0);
	assert_eq!(String::from_utf8(out)?, format!("Writing {}\n", s(&output)));
	assert!(std::fs::read(&output)? == std::fs::read(fixture("I-scaled.ttf"))?);
	Ok(())
}

/// `int(options.new_upem)` fails with a `ValueError`.
#[test]
fn scale_upem_invalid_upem() {
	let input = upstream("ttLib/data/I.ttf");
	let result = scale_upem::run(&[s(&input), "x"], &mut Vec::new(), &mut Vec::new());
	let message = result.err().map(|e| e.to_string()).unwrap_or_default();
	assert!(
		message.contains("invalid literal for int() with base 10: 'x'"),
		"{message}"
	);
}

#[test]
#[cfg(feature = "pathops")]
fn remove_overlaps_texts() -> TestResult {
	check_text_case("removeOverlaps-help", remove_overlaps::run, &["-h"])?;
	check_text_case("removeOverlaps-no-output", remove_overlaps::run, &["a.ttf"])?;
	check_text_case(
		"removeOverlaps-unknown",
		remove_overlaps::run,
		&["a.ttf", "b.ttf", "--bogus"],
	)
}

/// The outlines are not byte-identical (linesweeper instead of skia-pathops, see
/// `fontoxide_ttlib::remove_overlaps`): the glyphs and their advances are compared.
#[test]
#[cfg(feature = "pathops")]
fn remove_overlaps_run() -> TestResult {
	let output = tmp_dir("remove-overlaps").join("out.otf");
	let input = upstream("ttLib/data/IBMPlexSans-Bold.subset.otf");
	let code = remove_overlaps::run(&[s(&input), s(&output)], &mut Vec::new(), &mut Vec::new())?;
	assert_eq!(code, 0);
	let read = |path: &Path| -> Result<Font, Box<dyn std::error::Error>> {
		Ok(Font::read(std::fs::read(path)?, ReadOptions::default())?)
	};
	let mut ours = read(&output)?;
	let mut theirs = read(&fixture("IBMPlexSans-Bold.removeOverlaps.otf"))?;
	assert_eq!(ours.tags(), theirs.tags());
	assert_eq!(ours.glyph_order()?.names(), theirs.glyph_order()?.names());
	assert_eq!(
		ours.require::<Hmtx>()?.metrics,
		theirs.require::<Hmtx>()?.metrics
	);
	Ok(())
}

/// Without skia-pathops, importing the module fails.
#[test]
#[cfg(not(feature = "pathops"))]
fn remove_overlaps_without_pathops() {
	let result = remove_overlaps::run(&["-h"], &mut Vec::new(), &mut Vec::new());
	assert!(matches!(result, Err(fontoxide_ttlib::Error::Import(_))));
}
