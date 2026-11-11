//! `fontTools.varLib.avar.plan`, which fontTools has no tests for: differential
//! tests of its `main` on fonts of fontTools' tests, against the fixtures of
//! `tools/reference/avar_plan_fixtures.py` (the designspace snippets it prints,
//! and the `avar` table of the font it saves).

#![allow(non_snake_case)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_ttlib::{Font, ReadOptions, Tag, WriteOptions};
use fontoxide_varlib::avar::plan;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn tests_dir() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../../upstream/fonttools/Tests")
}

/// A new empty directory for a test.
fn temp_dir() -> PathBuf {
	static COUNTER: AtomicUsize = AtomicUsize::new(0);
	let dir = std::env::temp_dir().join(format!(
		"fontoxide-avar-plan-{}-{}",
		std::process::id(),
		COUNTER.fetch_add(1, Ordering::Relaxed)
	));
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir).expect("temp dir");
	dir
}

/// The font as a file: TTX sources are compiled.
fn font_file(source: &str, dir: &Path) -> Result<String, Box<dyn std::error::Error>> {
	let path = tests_dir().join(source);
	if path.extension().is_some_and(|e| e == "ttx") {
		let mut font = Font::from_ttx(&std::fs::read_to_string(&path)?)?;
		let out = dir.join("font.bin");
		std::fs::write(&out, font.write(&WriteOptions::default())?)?;
		return Ok(out.to_string_lossy().into_owned());
	}
	Ok(path.to_string_lossy().into_owned())
}

/// Runs `main` on the font with the arguments, and compares with the fixture.
fn check(name: &str, source: &str, args: &[&str]) -> TestResult {
	let expected = std::fs::read_to_string(
		Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("tests/data/avar_plan")
			.join(format!("{name}.txt")),
	)?
	.replace("\r\n", "\n");
	let (expected_out, result) = expected.split_once("== ").ok_or("malformed fixture")?;
	let dir = temp_dir();
	let font = font_file(source, &dir)?;
	let output = dir.join("out.ttf").to_string_lossy().into_owned();
	let mut full_args = vec![font.as_str(), "-o", output.as_str()];
	full_args.extend_from_slice(args);
	let mut out = Vec::new();
	let mut err = Vec::new();
	let code = plan::run(&full_args, &mut out, &mut err);
	let out = String::from_utf8(out)?;
	if result.starts_with("error") {
		assert!(code.is_err(), "{name}: expected an error, got {code:?}");
		return Ok(());
	}
	let code = code?;
	assert_eq!(out, expected_out, "{name}: standard output");
	if let Some(exit) = result.strip_prefix("exit ") {
		assert_eq!(code.to_string(), exit.trim(), "{name}: exit code");
		return Ok(());
	}
	assert_eq!(code, 0, "{name}");
	let avar = result
		.strip_prefix("avar\n")
		.ok_or("malformed fixture")?
		.trim();
	let saved = Font::read(std::fs::read(&output)?, ReadOptions::default())?;
	let reader = saved.reader().ok_or("no reader")?;
	let tag = Tag::new(b"avar");
	let actual = if reader.contains(tag) {
		reader
			.table_data(tag)?
			.iter()
			.map(|b| format!("{b:02x}"))
			.collect()
	} else {
		"none".to_owned()
	};
	assert_eq!(actual, avar, "{name}: avar");
	let _ = std::fs::remove_dir_all(&dir);
	Ok(())
}

macro_rules! cases {
	($($name:ident: $source:expr, [$($arg:expr),* $(,)?];)*) => {
		$(
			#[test]
			fn $name() -> TestResult {
				check(stringify!($name), $source, &[$($arg),*])
			}
		)*
	};
}

cases! {
	I_ttf: "ttLib/data/I.ttf", [];
	I_ttf_sanitize: "ttLib/data/I.ttf", ["-s"];
	I_ttf_values: "ttLib/data/I.ttf", [
		"--samples", "3",
		"--weights", "150 333.5 600 999",
		"--widths", "50 75 125 150",
		"--slants", "-5 -2.5 -9",
		"--sizes", "10 12 72",
	];
	I_ttf_pins: "ttLib/data/I.ttf", [
		"--weight-pins", "250:300 700:650",
		"--weight-design-limits", "1:4:10",
		"--width-pins", "50:60",
		"--slant-design-limits=-12:0:0",
		"--optical-size-pins", "20:18 72:80",
	];
	I_ttf_option_like_value: "ttLib/data/I.ttf", ["--slant-design-limits", "-12:0:0"];
	I_ttf_glyphs: "ttLib/data/I.ttf", ["-g", "I .notdef I"];
	I_ttf_frequencies: "ttLib/data/I.ttf", ["-g", "I:2.5 .notdef:0", "-s"];
	I_ttf_zero_samples: "ttLib/data/I.ttf", ["--samples", "0"];
	I_ttf_bad_limits: "ttLib/data/I.ttf", ["--weight-design-limits", "1:4"];
	I_ttf_bad_pins: "ttLib/data/I.ttf", ["--weight-pins", "250"];
	I_otf: "ttLib/data/I.otf", [];
	I_otf_sanitize: "ttLib/data/I.otf", ["-s", "--samples", "5"];
	Amstelvar_avar2: "ttLib/tables/data/Amstelvar-avar2.subset.ttf", [];
	Amstelvar_avar2_sanitize: "ttLib/tables/data/Amstelvar-avar2.subset.ttf", ["-s"];
	NotoSans_cubic: "ttLib/tables/data/NotoSans-VF-cubic.subset.ttf", [];
	NotoSans_cubic_sanitize: "ttLib/tables/data/NotoSans-VF-cubic.subset.ttf", ["-s"];
	varc_6868: "ttLib/data/varc-6868.ttf", [];
	varc_ac00_ac01: "ttLib/data/varc-ac00-ac01.ttf", ["-s"];
	TestGVAR: "subset/data/TestGVAR.ttx", [];
	TestHVVAR: "subset/data/TestHVVAR.ttx", ["-s"];
	PartialInstancerTest: "varLib/instancer/data/PartialInstancerTest-VF.ttx", ["-s"];
	PartialInstancerTest2: "varLib/instancer/data/PartialInstancerTest2-VF.ttx", [];
	CFF2Instancer_1: "varLib/instancer/data/CFF2Instancer-VF-1.ttx", [];
	CFF2Instancer_2: "varLib/instancer/data/CFF2Instancer-VF-2.ttx", [];
	MutatorSans: "varLib/data/MutatorSans_All_Variable.ttx", [];
	TestCFF2VF: "varLib/data/master_ttx_varfont_otf/TestCFF2VF.ttx", ["-s"];
	SparseMasters: "varLib/data/master_ttx_varfont_ttf/SparseMasters-VF.ttx", [];
	interpolatable_cff2: "varLib/data/variable_ttx_interpolatable_cff2/interpolatable-test.ttx", ["-s"];
	static_font: "ttLib/data/TestTTF-Regular.ttx", [];
}

/// `-h` prints the help.
#[test]
fn help() -> TestResult {
	let mut out = Vec::new();
	let code = plan::run(&["-h"], &mut out, &mut Vec::new())?;
	assert_eq!(code, 0);
	assert!(String::from_utf8(out)?.starts_with("usage: fonttools varLib.avar.plan [-h]"));
	Ok(())
}

/// The slants are those of fontTools (`SLANTS`).
#[test]
fn slants() {
	let slants = plan::slants();
	assert_eq!(slants.len(), 41);
	assert_eq!(slants[20], 0.0);
	assert_eq!(slants[0], -45.0);
	assert_eq!(slants[40], 45.0);
}

/// `makeDesignspaceSnippet`.
#[test]
fn snippet() {
	assert_eq!(
		plan::make_designspace_snippet("wght", "Weight", (100.0, 400.0, 900.0), &Vec::new()),
		"    <axis tag=\"wght\" name=\"Weight\" minimum=\"100\" default=\"400\" maximum=\"900\"/>"
	);
	assert_eq!(
		plan::make_designspace_snippet(
			"wdth",
			"Width",
			(75.0, 100.0, 100.0),
			&vec![(75.0, 80.5), (100.0, 100.0)]
		),
		"    <axis tag=\"wdth\" name=\"Width\" minimum=\"75\" default=\"100\" maximum=\"100\">\n      <map input=\"75\" output=\"80.5\"/>\n      <map input=\"100\" output=\"100\"/>\n    </axis>"
	);
}
