//! fontoxide's own tests (`Tests/pens/statisticsPen_test.py` does not test
//! `main`): [`fontoxide_cli::statistics_pen`] compared with what fontTools'
//! `fonttools pens.statisticsPen` prints (tools/reference/cli_mains_fixtures.py).

use std::path::PathBuf;

use fontoxide_cli::statistics_pen::run;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn tests_dir() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../upstream/fonttools/Tests")
}

fn expected(name: &str) -> Result<String, Box<dyn std::error::Error>> {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data/statistics_pen")
		.join(name);
	Ok(std::fs::read_to_string(path)?)
}

/// Runs the command line; the exit code, standard output and standard error.
fn main(args: &[&str]) -> Result<(i32, String, String), Box<dyn std::error::Error>> {
	let mut out = Vec::new();
	let mut err = Vec::new();
	let code = run(args, &mut out, &mut err)?;
	Ok((code, String::from_utf8(out)?, String::from_utf8(err)?))
}

#[test]
fn statistics_match_fonttools() -> TestResult {
	let font = |name: &str| tests_dir().join("ttLib/data").join(name);
	let i = font("I.ttf");
	let i = i.to_str().ok_or("path")?;
	let plex = font("IBMPlexSans-Bold.subset.otf");
	let plex = plex.to_str().ok_or("path")?;
	let cases: [(&str, Vec<&str>); 5] = [
		("I", vec![i]),
		("I-glyph-quiet", vec!["-q", i, "I"]),
		(
			"I-control-wght700",
			vec!["-q", "-c", "--variations", "wght=700", i],
		),
		("plex", vec![plex]),
		("plex-control", vec!["-c", "-q", plex]),
	];
	for (name, args) in cases {
		let (code, out, err) = main(&args)?;
		assert_eq!(code, 0, "{name}");
		assert_eq!(out, expected(&format!("{name}.txt"))?, "{name}");
		assert_eq!(err, "", "{name}");
	}
	Ok(())
}

#[test]
fn control_pen_has_no_moments() -> TestResult {
	let i = tests_dir().join("ttLib/data/I.ttf");
	let mut out = Vec::new();
	let error = run(
		&["-c", i.to_str().ok_or("path")?],
		&mut out,
		&mut Vec::new(),
	)
	.err()
	.ok_or("no error")?;
	assert_eq!(
		error.to_string(),
		"AttributeError: 'StatisticsControlPen' object has no attribute 'momentX'"
	);
	// What fontTools prints before failing.
	assert_eq!(String::from_utf8(out)?, "\nglyph: .notdef\narea: 0\n");
	Ok(())
}

#[test]
fn help_and_usage_errors() -> TestResult {
	let (code, out, err) = main(&["-h"])?;
	assert_eq!((code, err.as_str()), (0, ""));
	assert_eq!(out, expected("help.txt")?);
	let errors: [&[&str]; 4] = [
		&[],
		&["-y"],
		&["--bogus", "a.ttf"],
		&["a.ttf", "-q", "--variations"],
	];
	for (i, args) in errors.into_iter().enumerate() {
		let (code, out, err) = main(args)?;
		assert_eq!(out, "");
		assert_eq!(
			format!("{code}\n{err}"),
			expected(&format!("error-{i}.txt"))?,
			"{args:?}"
		);
	}
	Ok(())
}
