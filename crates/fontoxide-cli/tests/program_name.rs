//! fontoxide's own tests (no upstream counterpart): the texts of the command
//! lines under fontTools' names and under the `fontoxide` executable.

use fontoxide_cli::ttx::{self, Io};
use fontoxide_cli::{Program, fontoxide_main_with_output, fonttools_main, help, version};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn help_text(program: Program) -> Result<String, Box<dyn std::error::Error>> {
	let mut out = Vec::new();
	program.run(|| help(&mut out))?;
	Ok(String::from_utf8(out)?)
}

#[test]
fn help_lists_modules_under_each_name() -> TestResult {
	let fonttools = help_text(Program::FontTools)?;
	assert!(fonttools.starts_with(&format!(
		"fonttools v{}\n\n",
		fontoxide_ttlib::FONTTOOLS_VERSION
	)));
	assert!(
		fonttools.contains(
			"\nfonttools subset                    OpenType font subsetter and optimizer\n"
		)
	);

	let fontoxide = help_text(Program::Fontoxide)?;
	assert!(fontoxide.starts_with(&format!("{}\n\n", version())));
	assert!(
		fontoxide.contains(
			"\nfontoxide subset                    OpenType font subsetter and optimizer\n"
		)
	);
	assert!(!fontoxide.contains("fonttools"));
	// The same modules.
	assert_eq!(fonttools.lines().count(), fontoxide.lines().count());
	Ok(())
}

#[test]
fn version_flag() {
	let expected = format!(
		"fontoxide {} (fontTools {})\n",
		env!("CARGO_PKG_VERSION"),
		fontoxide_ttlib::FONTTOOLS_VERSION
	);
	for flag in ["--version", "-V"] {
		let mut out = Vec::new();
		assert_eq!(fontoxide_main_with_output(&[flag], &mut out), 0);
		assert_eq!(String::from_utf8_lossy(&out), expected);
	}
	// fontTools' `fonttools` has no such option: it looks for a module.
	assert_eq!(fonttools_main(&["--version"]), 1);
}

fn ttx_help(program: Program) -> Result<String, Box<dyn std::error::Error>> {
	let mut stdin: &[u8] = &[];
	let mut stdout = Vec::new();
	let mut stderr = Vec::new();
	let code = program
		.run(|| ttx::main_with_io(&["-h"], &mut Io::new(&mut stdin, &mut stdout, &mut stderr)))?;
	assert_eq!(code, 0);
	// Text mode on Windows.
	Ok(String::from_utf8(stdout)?.replace("\r\n", "\n"))
}

#[test]
fn ttx_help_under_each_name() -> TestResult {
	let fonttools = ttx_help(Program::FontTools)?;
	assert_eq!(fonttools, format!("{}\n", ttx::DOC));

	let fontoxide = ttx_help(Program::Fontoxide)?;
	assert!(
		fontoxide
			.starts_with("usage: fontoxide ttx [options] inputfile1 [... inputfileN]\n\nTTX -- ")
	);
	assert_eq!(
		fontoxide.replacen("usage: fontoxide ttx ", "usage: ttx ", 1),
		fonttools
	);
	Ok(())
}

#[test]
fn ttx_usage_error_under_each_name() -> TestResult {
	for (program, usage) in [
		(Program::FontTools, "usage: ttx [options]"),
		(Program::Fontoxide, "usage: fontoxide ttx [options]"),
	] {
		let mut stdin: &[u8] = &[];
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		let code = program.run(|| {
			ttx::main_with_io(
				&["--bogus"],
				&mut Io::new(&mut stdin, &mut stdout, &mut stderr),
			)
		})?;
		assert_eq!(code, 2);
		// Text mode on Windows.
		let stderr = String::from_utf8(stderr)?.replace("\r\n", "\n");
		assert!(stderr.starts_with(usage), "{stderr}");
		assert!(
			stderr.ends_with("\nERROR: option --bogus not recognized\n"),
			"{stderr}"
		);
	}
	Ok(())
}

/// An `argparse` `prog` that is not `fonttools <module>`: the usage is moved
/// along with the program name.
#[test]
fn argparse_prog_under_each_name() {
	let error = |program: Program| {
		program
			.run(|| fontoxide_cu2qu::cli::main(&["--bogus"]))
			.err()
			.map(|e| e.to_string())
			.unwrap_or_default()
	};
	let fonttools = error(Program::FontTools);
	assert!(fonttools.starts_with(
		"usage: cu2qu [-h] [--version] [-v] [-e ERROR] [-m] [--keep-direction] [-i |\n             -j [N]]"
	));
	assert!(fonttools.contains("\ncu2qu: error: "), "{fonttools}");

	let message = fonttools.rsplit_once(": error: ").map_or("", |(_, m)| m);
	assert_eq!(
		error(Program::Fontoxide),
		format!(
			"usage: fontoxide cu2qu [-h] [--version] [-v] [-e ERROR] [-m] [--keep-direction] [-i |
                       -j [N]] [-o OUTPUT | -d DIRECTORY]
                       INPUT [INPUT ...]
fontoxide cu2qu: error: {message}"
		)
	);
}

/// A `fonttools <module>` program: every mention is replaced, examples
/// included.
#[test]
fn fonttools_prog_under_each_name() -> TestResult {
	let help = |program: Program| -> Result<String, Box<dyn std::error::Error>> {
		let mut out = Vec::new();
		program.run(|| fontoxide_cli::svg_path_pen::run(&["-h"], &mut out))?;
		Ok(String::from_utf8(out)?)
	};
	let fonttools = help(Program::FontTools)?;
	assert!(fonttools.starts_with("usage: fonttools pens.svgPathPen [-h]"));
	let fontoxide = help(Program::Fontoxide)?;
	assert!(fontoxide.starts_with("usage: fontoxide pens.svgPathPen [-h]"));
	assert!(!fontoxide.contains("fonttools pens.svgPathPen"));
	assert_eq!(
		fontoxide.replace("fontoxide pens.svgPathPen", "fonttools pens.svgPathPen"),
		fonttools
	);

	let error = |program: Program| {
		program
			.run(|| fontoxide_cli::svg_path_pen::run(&["--bogus"], &mut Vec::new()))
			.err()
			.map(|e| e.to_string())
			.unwrap_or_default()
	};
	assert_eq!(
		error(Program::FontTools),
		"fonttools pens.svgPathPen: error: unrecognized arguments: --bogus"
	);
	assert_eq!(
		error(Program::Fontoxide),
		"fontoxide pens.svgPathPen: error: unrecognized arguments: --bogus"
	);

	// The `woff2` sub-commands.
	let woff2 = |program: Program| -> Result<String, Box<dyn std::error::Error>> {
		let mut out = Vec::new();
		program.run(|| fontoxide_ttlib::woff2_cli::run(&["compress", "-h"], &mut out))?;
		Ok(String::from_utf8(out)?)
	};
	assert!(woff2(Program::FontTools)?.starts_with("usage: fonttools ttLib.woff2 compress [-h]"));
	assert!(woff2(Program::Fontoxide)?.starts_with("usage: fontoxide ttLib.woff2 compress [-h]"));
	Ok(())
}

/// The program name only applies inside [`Program::run`].
#[test]
fn program_name_is_scoped() {
	assert_eq!(fontoxide_misc::cli_tools::program_name(), None);
	Program::Fontoxide.run(|| {
		assert_eq!(fontoxide_misc::cli_tools::program_name(), Some("fontoxide"));
	});
	assert_eq!(fontoxide_misc::cli_tools::program_name(), None);
}
