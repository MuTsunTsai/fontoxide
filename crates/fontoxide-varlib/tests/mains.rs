//! fontoxide's own tests (no upstream counterpart): the command lines of
//! `fontTools.varLib.avar.build`, `fontTools.varLib.avar.unbuild`,
//! `fontTools.varLib.hvar`, `fontTools.varLib.models` and
//! `fontTools.varLib.varStore`, compared with fontTools' (help texts, usage
//! errors, and the output of `tools/reference/varlib_mains_fixtures.py`, run with
//! `SOURCE_DATE_EPOCH=1700000000`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_varlib::avar::cli::{build_run, unbuild_run};
use fontoxide_varlib::{hvar, models, var_store};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

type Run = fn(
	&[&str],
	&mut dyn std::io::Write,
	&mut dyn std::io::Write,
) -> fontoxide_varlib::errors::Result<i32>;

/// A file of `crates/fontoxide-varlib/tests/data/mains`.
fn data(name: &str) -> String {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data/mains")
		.join(name)
		.to_string_lossy()
		.into_owned()
}

/// A file of `Tests/varLib/data`.
fn upstream(name: &str) -> String {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/varLib/data")
		.join(name)
		.to_string_lossy()
		.into_owned()
}

/// A new, empty temporary directory.
fn tmp_dir(name: &str) -> PathBuf {
	static COUNTER: AtomicUsize = AtomicUsize::new(0);
	let dir = std::env::temp_dir().join(format!(
		"fontoxide-varlib-mains-{}-{name}-{}",
		std::process::id(),
		COUNTER.fetch_add(1, Ordering::Relaxed)
	));
	let _ = std::fs::remove_dir_all(&dir);
	let _ = std::fs::create_dir_all(&dir);
	dir
}

/// Runs a command line: the exit code, standard output and standard error.
fn run(main: Run, args: &[&str]) -> TestResult<(i32, String, String)> {
	fontoxide_ttlib::set_clock(|| 1_700_000_000.0);
	let mut out = Vec::new();
	let mut err = Vec::new();
	let code = main(args, &mut out, &mut err)?;
	Ok((code, String::from_utf8(out)?, String::from_utf8(err)?))
}

/// The output of a command line that succeeds, and writes nothing to standard
/// error.
fn stdout(main: Run, args: &[&str]) -> TestResult<String> {
	let (code, out, err) = run(main, args)?;
	assert_eq!((code, err.as_str()), (0, ""));
	Ok(out)
}

fn assert_same_file(actual: &PathBuf, expected: &str) -> TestResult {
	let actual = std::fs::read(actual)?;
	let expected = std::fs::read(data(expected))?;
	assert!(actual == expected, "{expected:?} differs");
	Ok(())
}

const BUILD_HELP: &str = "\
usage: fonttools varLib.avar.build [-h] [-o OUTPUT_FILE] [-v]
                                   varfont.ttf family.designspace

Add `avar` table from designspace file to variable font.

positional arguments:
  varfont.ttf           Variable-font file.
  family.designspace    Designspace file.

options:
  -h, --help            show this help message and exit
  -o, --output-file OUTPUT_FILE
                        Output font file name.
  -v, --verbose         Run more verbosely.
";

#[test]
fn avar_build_help_and_errors() -> TestResult {
	assert_eq!(
		run(build_run, &["-h"])?,
		(0, BUILD_HELP.to_owned(), String::new())
	);
	assert_eq!(
		run(build_run, &[])?,
		(
			2,
			String::new(),
			"usage: fonttools varLib.avar.build [-h] [-o OUTPUT_FILE] [-v]
                                   varfont.ttf family.designspace
fonttools varLib.avar.build: error: the following arguments are required: varfont.ttf, family.designspace
"
			.to_owned()
		)
	);
	Ok(())
}

#[test]
fn avar_build_matches_fonttools() -> TestResult {
	let dir = tmp_dir("avar_build");
	for (designspace, expected) in [
		("Build.designspace", "avar-build.ttf"),
		("BuildAvar2.designspace", "avar2-build.ttf"),
	] {
		let output = dir.join(expected);
		let out = stdout(
			build_run,
			&[
				&data("master.ttf"),
				&upstream(designspace),
				"-o",
				&output.to_string_lossy(),
			],
		)?;
		assert_eq!(out, "");
		assert_same_file(&output, expected)?;
	}
	Ok(())
}

#[test]
fn avar_build_default_output_name() -> TestResult {
	let dir = tmp_dir("avar_build_default");
	let input = dir.join("master.ttf");
	std::fs::copy(data("master.ttf"), &input)?;
	stdout(
		build_run,
		&[&input.to_string_lossy(), &upstream("Build.designspace")],
	)?;
	assert_same_file(&dir.join("master.avar.ttf"), "avar-build.ttf")
}

#[test]
fn avar_unbuild_matches_fonttools() -> TestResult {
	assert_eq!(
		run(unbuild_run, &["--help"])?.1,
		"usage: fonttools varLib.avar.unbuild [-h] varfont.ttf

Print `avar` table as a designspace snippet.

positional arguments:
  varfont.ttf  Variable-font file.

options:
  -h, --help   show this help message and exit
"
	);
	for (font, expected) in [
		("avar-build.ttf", "avar-unbuild.txt"),
		("avar2-build.ttf", "avar2-unbuild.txt"),
	] {
		assert_eq!(
			stdout(unbuild_run, &[&data(font)])?,
			std::fs::read_to_string(data(expected))?
		);
	}
	// Not a variable font.
	assert_eq!(
		run(unbuild_run, &[&data("master.ttf")])?,
		(1, String::new(), "Not a variable font.\n".to_owned())
	);
	assert_eq!(
		run(unbuild_run, &["a.ttf", "b.ttf"])?,
		(
			2,
			String::new(),
			"usage: fonttools varLib.avar.unbuild [-h] varfont.ttf
fonttools varLib.avar.unbuild: error: unrecognized arguments: b.ttf
"
			.to_owned()
		)
	);
	Ok(())
}

#[test]
fn hvar_matches_fonttools() -> TestResult {
	assert_eq!(
		run(hvar::run, &["-h"])?.1,
		"usage: fonttools varLib.hvar [-h] [-o OUTPUT_FILE] varfont.ttf

Add `HVAR` table from to variable font.

positional arguments:
  varfont.ttf           Variable-font file.

options:
  -h, --help            show this help message and exit
  -o, --output-file OUTPUT_FILE
                        Output font file name.
"
	);
	let dir = tmp_dir("hvar");
	let output = dir.join("hvar.ttf");
	stdout(
		hvar::run,
		&[
			&data("MutatorSans.ttf"),
			"--output-file",
			&output.to_string_lossy(),
		],
	)?;
	assert_same_file(&output, "hvar.ttf")?;
	// Not a variable font: an error is logged.
	assert_eq!(run(hvar::run, &[&data("master.ttf")])?.0, 1);
	assert_eq!(
		run(hvar::run, &["a.ttf", "-o"])?,
		(
			2,
			String::new(),
			"usage: fonttools varLib.hvar [-h] [-o OUTPUT_FILE] varfont.ttf
fonttools varLib.hvar: error: argument -o/--output-file: expected one argument
"
			.to_owned()
		)
	);
	Ok(())
}

#[test]
fn models_matches_fonttools() -> TestResult {
	assert_eq!(
		run(models::run, &["-h"])?.1,
		"usage: fonttools varLib.models [-h] [--loglevel LEVEL] (-d DESIGNSPACE |
                               -l LOCATION [LOCATION ...])

Normalize locations on a given designspace

options:
  -h, --help            show this help message and exit
  --loglevel LEVEL      Logging level (defaults to INFO)
  -d, --designspace DESIGNSPACE
  -l, --locations LOCATION [LOCATION ...]
                        Master locations as comma-separate coordinates. One
                        must be all zeros.
"
	);
	assert_eq!(
		stdout(
			models::run,
			&[
				"-l",
				"0,0",
				"1,0",
				"0,1",
				"1,1",
				"0.5,0",
				"0,-1",
				"0.25,-0.5"
			]
		)?,
		std::fs::read_to_string(data("models-l.txt"))?
	);
	assert_eq!(
		stdout(models::run, &["-d", &upstream("Build.designspace")])?,
		std::fs::read_to_string(data("models-d.txt"))?
	);
	let usage = "usage: fonttools varLib.models [-h] [--loglevel LEVEL] (-d DESIGNSPACE |
                               -l LOCATION [LOCATION ...])
";
	assert_eq!(
		run(models::run, &[])?,
		(
			2,
			String::new(),
			format!(
				"{usage}fonttools varLib.models: error: one of the arguments -d/--designspace -l/--locations is required\n"
			)
		)
	);
	assert_eq!(
		run(models::run, &["-d", "a", "-l", "0"])?,
		(
			2,
			String::new(),
			format!(
				"{usage}fonttools varLib.models: error: argument -l/--locations: not allowed with argument -d/--designspace\n"
			)
		)
	);
	Ok(())
}

#[test]
fn var_store_matches_fonttools() -> TestResult {
	assert_eq!(
		run(var_store::run, &["-h"])?.1,
		"usage: varLib.varStore [-h] [--quantization QUANTIZATION] fontfile [outfile]

Optimize a font's GDEF variation store

positional arguments:
  fontfile
  outfile

options:
  -h, --help            show this help message and exit
  --quantization QUANTIZATION
"
	);
	let dir = tmp_dir("var_store");
	let output = dir.join("varstore.ttf");
	assert_eq!(
		stdout(
			var_store::run,
			&[&data("MutatorSans.ttf"), &output.to_string_lossy()]
		)?,
		std::fs::read_to_string(data("varstore.txt"))?
	);
	assert_same_file(&output, "varstore.ttf")?;
	assert_eq!(
		stdout(
			var_store::run,
			&["--quantization", "10", &data("MutatorSans.ttf")]
		)?,
		std::fs::read_to_string(data("varstore-q.txt"))?
	);
	assert_eq!(
		run(var_store::run, &["--quantization", "x", "a.ttf"])?,
		(
			2,
			String::new(),
			"usage: varLib.varStore [-h] [--quantization QUANTIZATION] fontfile [outfile]
varLib.varStore: error: argument --quantization: invalid int value: 'x'
"
			.to_owned()
		)
	);
	Ok(())
}
