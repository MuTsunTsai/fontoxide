//! `fontTools.diff`, which fontTools has no tests for: differential tests of
//! its command line on fonts of fontTools' tests, against the fixtures of
//! `tools/reference/diff_fixtures.py` (what it writes to standard output and
//! standard error, and its exit code).

#![allow(non_snake_case)]

use std::path::Path;

use fontoxide_diff::utils::get_file_modtime;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// An argument: `Tests/...` is a font of fontTools' tests.
fn resolve(arg: &str) -> String {
	match arg.strip_prefix("Tests/") {
		Some(rest) => Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("../../upstream/fonttools/Tests")
			.join(rest)
			.to_string_lossy()
			.into_owned(),
		None => arg.to_owned(),
	}
}

/// Runs the command line and compares with the fixture, after replacing the
/// file names and modification times as the fixture script does.
fn check(name: &str, args: &[&str]) -> TestResult {
	let expected = std::fs::read_to_string(
		Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("tests/data")
			.join(format!("{name}.txt")),
	)?
	.replace("\r\n", "\n");
	// `--diff=` uses the built-in diff even if there is a `diff` on the PATH.
	let mut args: Vec<String> = args.iter().map(|a| resolve(a)).collect();
	if !args.iter().any(|a| a == "--diff" || a == "-h") {
		args.push("--diff=".into());
	}
	let args: Vec<&str> = args.iter().map(String::as_str).collect();
	let mut out = Vec::new();
	let mut err = Vec::new();
	let result = fontoxide_diff::run(&args, &mut out, &mut err);
	let (mut out, mut err) = (String::from_utf8(out)?, String::from_utf8(err)?);
	let files: Vec<&str> = args
		.iter()
		.copied()
		.filter(|a| Path::new(a).is_file())
		.take(2)
		.collect();
	for (n, file) in files.iter().enumerate() {
		let mtime = get_file_modtime(file)?;
		out = out.replace(&format!("\t{mtime}\n"), &format!("\t<mtime{}>\n", n + 1));
	}
	for (n, file) in files.iter().enumerate() {
		out = out.replace(file, &format!("<file{}>", n + 1));
		err = err.replace(file, &format!("<file{}>", n + 1));
	}
	let code = match result {
		Ok(code) => code.to_string(),
		Err(e) => {
			// An exception that fontTools lets through.
			let exit = expected.rsplit("== exit ").next().unwrap_or_default();
			assert!(exit.starts_with("error"), "{name}: unexpected error {e}");
			return Ok(());
		}
	};
	let actual = format!("{out}== stderr\n{err}== exit {code}\n");
	assert_eq!(actual, expected, "{name}");
	Ok(())
}

const TTF: &str = "Tests/ttx/data/TestTTF.ttf";
const OTF: &str = "Tests/ttx/data/TestOTF.otf";
const TTC: &str = "Tests/ttx/data/TestTTC.ttc";
const TTC2: &str = "Tests/ttx/data/TestTTCv2.ttc";
const WOFF: &str = "Tests/ttx/data/TestWOFF.woff";
const WOFF2: &str = "Tests/ttx/data/TestWOFF2.woff2";
const I_TTF: &str = "Tests/ttLib/data/I.ttf";
const I_OTF: &str = "Tests/ttLib/data/I.otf";
const VARC1: &str = "Tests/ttLib/data/varc-ac00-ac01.ttf";
const VARC2: &str = "Tests/ttLib/data/varc-6868.ttf";

macro_rules! cases {
	($($name:ident: [$($arg:expr),* $(,)?];)*) => {
		$(
			#[test]
			fn $name() -> TestResult {
				check(stringify!($name), &[$($arg),*])
			}
		)*
	};
}

cases! {
	ttf_otf: [TTF, OTF, "--color", "never"];
	ttf_otf_color: [TTF, OTF];
	ttf_otf_color_always: [TTF, OTF, "--color", "always", "-U", "1"];
	ttf_otf_summary: [TTF, OTF, "-l"];
	ttf_otf_summary_include: [TTF, OTF, "-l", "-t", "head", "CFF", "glyf", "zzzz"];
	ttf_otf_summary_exclude: [TTF, OTF, "-l", "-x", "head", "CFF", "glyf"];
	ttf_otf_include: [TTF, OTF, "--color", "never", "-t", "head", "hhea", "GlyphOrder"];
	ttf_otf_exclude: [TTF, OTF, "--color", "never", "-x", "glyf", "CFF", "-U", "0"];
	ttf_otf_always: [TTF, OTF, "--color", "never", "-a", "-t", "maxp", "name"];
	ttf_otf_context: [TTF, OTF, "--color", "never", "--lines", "7"];
	ttf_otf_negative_context: [TTF, OTF, "--color", "never", "-U", "-2"];
	ttf_otf_quiet: [TTF, OTF, "-q"];
	ttf_otf_long_tags: [TTF, OTF, "--color", "never", "-t", "toolong", "zzzz"];
	ttf_otf_same_included: [TTF, OTF, "--color", "never", "-t", "OS/2", "post"];
	ttf_same: [TTF, TTF, "--color", "never"];
	ttf_same_summary: [TTF, TTF, "-l"];
	ttf_same_always: [TTF, TTF, "--color", "never", "-a", "-t", "head"];
	ttf_same_negative_context: [TTF, TTF, "--color", "never", "-U", "-1"];
	ttc_fonts: [TTC, TTC, "--color", "never", "--y1", "0", "--y2", "1"];
	ttc_summary: [TTC, TTC2, "-l", "--y1", "1", "--y2", "0"];
	ttc_no_number: [TTC, TTF, "--color", "never"];
	ttc_no_number_summary: [TTC, TTF, "-l"];
	ttc_bad_number_summary: [TTC, TTF, "-l", "--y1", "7"];
	woff_woff2: [WOFF, WOFF2, "--color", "never"];
	woff_woff2_summary: [WOFF, WOFF2, "-l"];
	woff2_ttf: [WOFF2, TTF, "--color", "never", "-a", "-U", "2"];
	I_ttf_otf: [I_TTF, I_OTF, "--color", "never"];
	varc: [VARC1, VARC2, "--color", "never", "-x", "glyf"];
	always_and_binary: [TTF, OTF, "-a", "-b"];
	always_and_binary_quiet: [TTF, OTF, "-a", "-b", "-q"];
	missing_file1: ["nonexistent.ttf", OTF];
	missing_file2: [TTF, "nonexistent.otf", "-q"];
	missing_diff_tool: [TTF, OTF, "--diff", "nonexistent-diff-tool"];
	empty_diff_tool: [TTF, OTF, "--diff", "", "--color", "never", "-t", "head"];
	bad_color: [TTF, OTF, "--color", "red"];
	bad_lines: [TTF, OTF, "-U", "x"];
	missing_files: [TTF];
	include_eats_files: ["-t", "head", TTF, OTF];
	help: ["-h"];
}
