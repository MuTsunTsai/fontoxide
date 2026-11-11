//! `Tests/ttx/ttx_test.py`.

#![allow(non_snake_case)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_cli::ttx::{self, Error, FileType, Io, Options};
use fontoxide_misc::xml_reader::{self, Element};
use fontoxide_ttlib::tables::{Dsig, Head};
use fontoxide_ttlib::{DefaultTable, Font, ReadOptions, Tag, TtxOptions};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// A path under `upstream/fonttools/Tests/ttx/data` (`getpath`, and the
/// `Tests/ttx/data` paths relative to the fontTools checkout).
fn getpath(test_file: &str) -> String {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttx/data")
		.join(test_file)
		.to_string_lossy()
		.into_owned()
}

/// A temporary directory, removed when dropped (`tmpdir`, `tmp_path`).
struct TempDir(PathBuf);

impl TempDir {
	fn new() -> std::io::Result<Self> {
		static COUNTER: AtomicUsize = AtomicUsize::new(0);
		let path = std::env::temp_dir().join(format!(
			"fontoxide-cli-ttx-{}-{}",
			std::process::id(),
			COUNTER.fetch_add(1, Ordering::Relaxed)
		));
		std::fs::create_dir_all(&path)?;
		Ok(TempDir(path))
	}

	fn path(&self) -> String {
		self.0.to_string_lossy().into_owned()
	}

	/// `os.path.join(tempdir, name)`.
	fn join(&self, name: &str) -> String {
		self.0.join(name).to_string_lossy().into_owned()
	}

	/// `temp_font`: copies a test file into the directory.
	fn temp_font(&self, font_path: &str, file_name: &str) -> std::io::Result<String> {
		let path = self.join(file_name);
		std::fs::copy(font_path, &path)?;
		Ok(path)
	}
}

impl Drop for TempDir {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

/// `read_file`: the lines of a file, with their line endings.
fn read_file(path: &str) -> std::io::Result<Vec<String>> {
	let text = std::fs::read_to_string(path)?;
	Ok(text.split_inclusive('\n').map(str::to_owned).collect())
}

/// `parseXML(lines)`: the top-level elements.
fn parse_xml(lines: &[String]) -> Vec<Element> {
	let root = xml_reader::parse(&format!("<root>{}</root>", lines.concat())).expect("valid XML");
	root.elements().cloned().collect()
}

fn read_font(path: impl AsRef<Path>) -> fontoxide_ttlib::Result<Font> {
	Font::read(std::fs::read(path)?, ReadOptions::default())
}

fn assert_has_tables(font: &Font, tables: &[&[u8; 4]]) {
	for table in tables {
		assert!(font.contains(Tag::new(table)), "{}", Tag::new(table));
	}
}

fn options(raw: &[(&str, &str)]) -> ttx::Result<Options> {
	Options::new(raw, 1)
}

fn exit_output(result: ttx::Result<Options>) -> String {
	match result {
		Err(Error::Exit { code: 0, output }) => output,
		other => panic!("expected SystemExit: {other:?}"),
	}
}

fn is_getopt_error<T: std::fmt::Debug>(result: ttx::Result<T>) -> bool {
	matches!(result, Err(Error::Getopt(_)))
}

thread_local! {
	static CAPTURED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

struct Capture;

impl log::Log for Capture {
	fn enabled(&self, _: &log::Metadata<'_>) -> bool {
		true
	}

	fn log(&self, record: &log::Record<'_>) {
		CAPTURED.with(|c| {
			if let Some(records) = c.borrow_mut().as_mut() {
				records.push(format!("{}: {}", record.level(), record.args()));
			}
		});
	}

	fn flush(&self) {}
}

static CAPTURE: Capture = Capture;

/// The messages `f` logs on this thread (`caplog`, `assertLogs`).
fn capture_logs<R>(f: impl FnOnce() -> R) -> (String, R) {
	static INIT: std::sync::Once = std::sync::Once::new();
	INIT.call_once(|| {
		let _ = log::set_logger(&CAPTURE);
	});
	CAPTURED.with(|c| *c.borrow_mut() = Some(Vec::new()));
	let result = f();
	let records = CAPTURED.with(|c| c.borrow_mut().take()).unwrap_or_default();
	(records.join("\n"), result)
}

/// `ttx.main(args)` with standard input, returning the exit code, standard
/// output and standard error.
fn main_with_stdin(args: &[&str], stdin: &[u8]) -> ttx::Result<(i32, Vec<u8>, Vec<u8>)> {
	let mut stdin = stdin;
	let mut stdout = Vec::new();
	let mut stderr = Vec::new();
	let code = ttx::main_with_io(args, &mut Io::new(&mut stdin, &mut stdout, &mut stderr))?;
	Ok((code, stdout, stderr))
}

// -----
// Tests
// -----

#[test]
fn test_saveXML_replace_control_characters() -> TestResult {
	use fontoxide::font_builder::{FontBuilder, NameKey, NameValue, Value};
	use fontoxide_ttlib::tables::glyf::Glyph;

	let mut fb = FontBuilder::new(1000, true, false)?;
	fb.setup_glyph_order(vec![".notdef".to_owned()]);
	fb.setup_character_map(&Default::default(), None, false)?;
	fb.setup_glyf(
		[(".notdef".to_owned(), Glyph::default())]
			.into_iter()
			.collect(),
		true,
	)?;
	fb.setup_horizontal_metrics(&[(".notdef".to_owned(), (600.0, 0.0))].into_iter().collect())?;
	fb.setup_horizontal_header(&[("ascent", Value::Int(800)), ("descent", Value::Int(-200))])?;
	fb.setup_os2(&[])?;
	fb.setup_post(true, &[])?;
	// Set up a name record with an illegal control character.
	let name_str = "Control\x01Char";
	let replaced = name_str.replace('\x01', "?");
	fb.setup_name_table(
		&[(NameKey::from("familyName"), NameValue::from(name_str))],
		true,
		true,
	)?;
	let tmp = TempDir::new()?;
	let ttx_path = tmp.join("test.ttx");
	let (warnings, files) =
		capture_logs(|| fb.font.to_ttx_files(&ttx_path, &TtxOptions::default()));
	for file in files? {
		std::fs::write(&file.path, file.contents)?;
	}
	let xml_content = std::fs::read_to_string(&ttx_path)?;
	assert!(xml_content.contains(&replaced));
	assert!(!xml_content.contains('\x01'));
	assert!(
		warnings.contains("Illegal XML character(s) found"),
		"{warnings}"
	);
	assert!(warnings.contains("\\x01"), "{warnings}");
	Ok(())
}

#[test]
fn test_parseOptions_no_args() {
	match ttx::parse_options(&[]) {
		Err(Error::Getopt(e)) => assert!(e.contains("Must specify at least one input file")),
		other => panic!("{other:?}"),
	}
}

#[test]
fn test_parseOptions_invalid_path() {
	let file_path = "invalid_font_path";
	match ttx::parse_options(&[file_path]) {
		Err(Error::Getopt(e)) => assert!(e.contains(&format!("File not found: \"{file_path}\""))),
		other => panic!("{other:?}"),
	}
}

#[test]
fn test_parseOptions_font2ttx_1st_time() -> TestResult {
	let tmp = TempDir::new()?;
	let temp_path = tmp.temp_font(&getpath("TestOTF.otf"), "TestOTF.otf")?;
	let (jobs, _) = ttx::parse_options(&[&temp_path])?;
	assert_eq!(jobs[0].action.name(), "ttDump");
	assert_eq!(
		(jobs[0].input.as_str(), jobs[0].output.as_str()),
		(
			tmp.join("TestOTF.otf").as_str(),
			tmp.join("TestOTF.ttx").as_str()
		)
	);
	Ok(())
}

#[test]
fn test_parseOptions_font2ttx_2nd_time() -> TestResult {
	let tmp = TempDir::new()?;
	let temp_path = tmp.temp_font(&getpath("TestTTF.ttf"), "TestTTF.ttf")?;
	ttx::parse_options(&[&temp_path])?; // this is NOT a mistake
	let (jobs, _) = ttx::parse_options(&[&temp_path])?;
	assert_eq!(jobs[0].action.name(), "ttDump");
	assert_eq!(
		(jobs[0].input.as_str(), jobs[0].output.as_str()),
		(
			tmp.join("TestTTF.ttf").as_str(),
			tmp.join("TestTTF#1.ttx").as_str()
		)
	);
	Ok(())
}

#[test]
fn test_parseOptions_ttx2font_1st_time() -> TestResult {
	let tmp = TempDir::new()?;
	let temp_path = tmp.temp_font(&getpath("TestTTF.ttx"), "TestTTF.ttx")?;
	let (jobs, _) = ttx::parse_options(&[&temp_path])?;
	assert_eq!(jobs[0].action.name(), "ttCompile");
	assert_eq!(
		(jobs[0].input.as_str(), jobs[0].output.as_str()),
		(
			tmp.join("TestTTF.ttx").as_str(),
			tmp.join("TestTTF.ttf").as_str()
		)
	);
	Ok(())
}

#[test]
fn test_parseOptions_ttx2font_2nd_time() -> TestResult {
	let tmp = TempDir::new()?;
	let temp_path = tmp.temp_font(&getpath("TestOTF.ttx"), "TestOTF.ttx")?;
	ttx::parse_options(&[&temp_path])?; // this is NOT a mistake
	let (jobs, _) = ttx::parse_options(&[&temp_path])?;
	assert_eq!(jobs[0].action.name(), "ttCompile");
	assert_eq!(
		(jobs[0].input.as_str(), jobs[0].output.as_str()),
		(
			tmp.join("TestOTF.ttx").as_str(),
			tmp.join("TestOTF#1.otf").as_str()
		)
	);
	Ok(())
}

#[test]
fn test_parseOptions_multiple_fonts() -> TestResult {
	let tmp = TempDir::new()?;
	let file_names = ["TestOTF.otf", "TestTTF.ttf"];
	let temp_paths = file_names
		.iter()
		.map(|n| tmp.temp_font(&getpath(n), n))
		.collect::<std::io::Result<Vec<_>>>()?;
	let args: Vec<&str> = temp_paths.iter().map(String::as_str).collect();
	let (jobs, _) = ttx::parse_options(&args)?;
	for (job, file_name) in jobs.iter().zip(file_names) {
		assert_eq!(job.action.name(), "ttDump");
		let stem = file_name.split('.').next().unwrap_or_default();
		assert_eq!(job.input, tmp.join(file_name));
		assert_eq!(job.output, tmp.join(&format!("{stem}.ttx")));
	}
	Ok(())
}

#[test]
fn test_parseOptions_mixed_files() -> TestResult {
	let tmp = TempDir::new()?;
	let operations = ["ttDump", "ttCompile"];
	let extensions = [".ttx", ".ttf"];
	let file_names = ["TestOTF.otf", "TestTTF.ttx"];
	let temp_paths = file_names
		.iter()
		.map(|n| tmp.temp_font(&getpath(n), n))
		.collect::<std::io::Result<Vec<_>>>()?;
	let args: Vec<&str> = temp_paths.iter().map(String::as_str).collect();
	let (jobs, _) = ttx::parse_options(&args)?;
	for i in 0..jobs.len() {
		assert_eq!(jobs[i].action.name(), operations[i]);
		let stem = file_names[i].split('.').next().unwrap_or_default();
		assert_eq!(jobs[i].input, tmp.join(file_names[i]));
		assert_eq!(
			jobs[i].output,
			tmp.join(&format!("{stem}{}", extensions[i]))
		);
	}
	Ok(())
}

#[test]
fn test_parseOptions_splitTables() -> TestResult {
	let tmp = TempDir::new()?;
	let temp_path = tmp.temp_font(&getpath("TestTTF.ttf"), "TestTTF.ttf")?;
	let (jobs, options) = ttx::parse_options(&["-s", &temp_path])?;

	let ttx_file_path = &jobs[0].output;
	let temp_folder = Path::new(ttx_file_path).parent().ok_or("no folder")?;
	assert!(options.split_tables);
	assert!(Path::new(ttx_file_path).exists());

	ttx::process(&jobs, &options)?;

	// Read the TTX file but strip the first two and the last lines.
	let lines = read_file(ttx_file_path)?;
	let parsed_xml = parse_xml(&lines[2..lines.len() - 1]);
	assert!(!parsed_xml.is_empty());
	for item in parsed_xml {
		let table_file_name = item.attr("src").ok_or("no src")?;
		assert!(temp_folder.join(table_file_name).exists());
	}
	Ok(())
}

#[test]
fn test_parseOptions_splitGlyphs() -> TestResult {
	let tmp = TempDir::new()?;
	let temp_path = tmp.temp_font(&getpath("TestTTF.ttf"), "TestTTF.ttf")?;
	let (jobs, options) = ttx::parse_options(&["-g", &temp_path])?;

	let ttx_file_path = &jobs[0].output;
	let temp_folder = Path::new(ttx_file_path).parent().ok_or("no folder")?;
	assert!(options.split_glyphs);
	// splitGlyphs also forces splitTables.
	assert!(options.split_tables);
	assert!(Path::new(ttx_file_path).exists());

	ttx::process(&jobs, &options)?;

	let lines = read_file(ttx_file_path)?;
	let mut glyph_files = 0;
	for item in parse_xml(&lines[2..lines.len() - 1]) {
		let table_tag = &item.name;
		let table_file_name = item.attr("src").ok_or("no src")?;
		let table_file_path = temp_folder.join(table_file_name);
		assert!(table_file_path.exists());
		if table_tag != "glyf" {
			continue;
		}
		// Also strip the enclosing 'glyf' element.
		let lines = read_file(&table_file_path.to_string_lossy())?;
		for item in parse_xml(&lines[4..lines.len() - 3]) {
			// Glyphs without outline data only have the 'name' attribute.
			if let Some(glyph_file_name) = item.attr("src") {
				assert!(temp_folder.join(glyph_file_name).exists());
				glyph_files += 1;
			}
		}
	}
	assert!(glyph_files > 0);
	Ok(())
}

/// The files of `-s` and `-g` compile to the same font as the single TTX file.
#[test]
fn split_dump_roundtrip() -> TestResult {
	for option in ["-s", "-g"] {
		let tmp = TempDir::new()?;
		let temp_path = tmp.temp_font(&getpath("TestTTF.ttf"), "TestTTF.ttf")?;
		let single = tmp.join("single.ttx");
		assert_eq!(ttx::main(&["-q", "-o", &single, &temp_path])?, 0);
		let split = tmp.join("split.ttx");
		assert_eq!(ttx::main(&["-q", option, "-o", &split, &temp_path])?, 0);

		let compile = |path: &str| -> fontoxide_ttlib::Result<Vec<u8>> {
			let mut font = Font::new();
			font.recalc_timestamp = false;
			font.import_ttx_file(path)?;
			font.write(&Default::default())
		};
		assert_eq!(compile(&single)?, compile(&split)?, "{option}");
	}
	Ok(())
}

fn guess(file_name: &str) -> Option<&'static str> {
	ttx::guess_file_type(&getpath(file_name)).map(FileType::as_str)
}

#[test]
fn test_guessFileType_ttf() {
	assert_eq!(guess("TestTTF.ttf"), Some("TTF"));
}

#[test]
fn test_guessFileType_otf() {
	assert_eq!(guess("TestOTF.otf"), Some("OTF"));
}

#[test]
fn test_guessFileType_woff() {
	assert_eq!(guess("TestWOFF.woff"), Some("WOFF"));
}

#[test]
fn test_guessFileType_woff2() {
	assert_eq!(guess("TestWOFF2.woff2"), Some("WOFF2"));
}

#[test]
fn test_guessFileType_ttc() {
	assert_eq!(guess("TestTTC.ttc"), Some("TTC"));
}

#[test]
fn test_guessFileType_dfont() {
	assert_eq!(guess("TestDFONT.dfont"), Some("TTF"));
}

#[test]
fn test_guessFileType_ttx_ttf() {
	assert_eq!(guess("TestTTF.ttx"), Some("TTX"));
}

#[test]
fn test_guessFileType_ttx_otf() {
	assert_eq!(guess("TestOTF.ttx"), Some("OTX"));
}

#[test]
fn test_guessFileType_ttx_bom() {
	assert_eq!(guess("TestBOM.ttx"), Some("TTX"));
}

#[test]
fn test_guessFileType_ttx_no_sfntVersion() {
	assert_eq!(guess("TestNoSFNT.ttx"), Some("TTX"));
}

#[test]
fn test_guessFileType_ttx_no_xml() {
	assert_eq!(guess("TestNoXML.ttx"), None);
}

#[test]
fn test_guessFileType_invalid_path() {
	assert_eq!(ttx::guess_file_type("invalid_font_path"), None);
}

// -----------------------
// ttx.Options class tests
// -----------------------

#[test]
fn test_options_flag_h() {
	let out = exit_output(options(&[("-h", "")]));
	assert!(out.contains("TTX -- From OpenType To XML And Back"));
}

#[test]
fn test_options_flag_version() {
	let out = exit_output(options(&[("--version", "")]));
	let version_list: Vec<&str> = out.split('.').collect();
	assert!(version_list.len() >= 3);
	assert!(version_list[0].chars().all(|c| c.is_ascii_digit()));
	assert!(version_list[1].chars().all(|c| c.is_ascii_digit()));
	assert!(version_list[2].trim().chars().all(|c| c.is_ascii_digit()));
}

#[test]
fn test_options_d_goodpath() -> TestResult {
	let tmp = TempDir::new()?;
	let temp_dir_path = tmp.path();
	let tto = options(&[("-d", &temp_dir_path)])?;
	assert_eq!(tto.output_dir.as_deref(), Some(temp_dir_path.as_str()));
	Ok(())
}

#[test]
fn test_options_d_badpath() {
	assert!(is_getopt_error(options(&[("-d", "bogusdir")])));
}

#[test]
fn test_options_o() -> TestResult {
	let tto = options(&[("-o", "testfile.ttx")])?;
	assert_eq!(tto.output_file.as_deref(), Some("testfile.ttx"));
	Ok(())
}

#[test]
fn test_options_f() -> TestResult {
	assert!(options(&[("-f", "")])?.over_write);
	Ok(())
}

#[test]
fn test_options_v() -> TestResult {
	let tto = options(&[("-v", "")])?;
	assert!(tto.verbose);
	assert_eq!(tto.log_level, log::LevelFilter::Debug);
	Ok(())
}

#[test]
fn test_options_q() -> TestResult {
	let tto = options(&[("-q", "")])?;
	assert!(tto.quiet);
	assert_eq!(tto.log_level, log::LevelFilter::Warn);
	Ok(())
}

#[test]
fn test_options_l() -> TestResult {
	assert!(options(&[("-l", "")])?.list_tables);
	Ok(())
}

#[test]
fn test_options_t_nopadding() -> TestResult {
	let tto = options(&[("-t", "CFF2")])?;
	assert_eq!(tto.only_tables, ["CFF2"]);
	Ok(())
}

#[test]
fn test_options_t_withpadding() -> TestResult {
	let tto = options(&[("-t", "CFF")])?;
	assert_eq!(tto.only_tables, ["CFF "]);
	Ok(())
}

#[test]
fn test_options_s() -> TestResult {
	let tto = options(&[("-s", "")])?;
	assert!(tto.split_tables);
	assert!(!tto.split_glyphs);
	Ok(())
}

#[test]
fn test_options_g() -> TestResult {
	let tto = options(&[("-g", "")])?;
	assert!(tto.split_glyphs);
	assert!(tto.split_tables);
	Ok(())
}

#[test]
fn test_options_i() -> TestResult {
	assert!(!options(&[("-i", "")])?.disassemble_instructions);
	Ok(())
}

#[test]
fn test_options_z_validoptions() -> TestResult {
	for option in ["raw", "row", "bitwise", "extfile"] {
		assert_eq!(options(&[("-z", option)])?.bitmap_glyph_data_format, option);
	}
	Ok(())
}

#[test]
fn test_options_z_invalidoption() {
	assert!(is_getopt_error(options(&[("-z", "bogus")])));
}

#[test]
fn test_options_y_validvalue() -> TestResult {
	assert_eq!(options(&[("-y", "1")])?.font_number, 1);
	Ok(())
}

#[test]
fn test_options_y_invalidvalue() {
	assert!(matches!(options(&[("-y", "A")]), Err(Error::Value(_))));
}

#[test]
fn test_options_m() -> TestResult {
	let tto = options(&[("-m", "testfont.ttf")])?;
	assert_eq!(tto.merge_file.as_deref(), Some("testfont.ttf"));
	Ok(())
}

#[test]
fn test_options_b() -> TestResult {
	assert!(!options(&[("-b", "")])?.recalc_bboxes);
	Ok(())
}

#[test]
fn test_options_e() -> TestResult {
	assert!(!options(&[("-e", "")])?.ignore_decompile_errors);
	Ok(())
}

#[test]
fn test_options_unicodedata() -> TestResult {
	let tto = options(&[("--unicodedata", "UnicodeData.txt")])?;
	assert_eq!(tto.unicodedata.as_deref(), Some("UnicodeData.txt"));
	Ok(())
}

#[test]
fn test_options_newline_lf() -> TestResult {
	assert_eq!(options(&[("--newline", "LF")])?.newlinestr, "\n");
	Ok(())
}

#[test]
fn test_options_newline_cr() -> TestResult {
	assert_eq!(options(&[("--newline", "CR")])?.newlinestr, "\r");
	Ok(())
}

#[test]
fn test_options_newline_crlf() -> TestResult {
	assert_eq!(options(&[("--newline", "CRLF")])?.newlinestr, "\r\n");
	Ok(())
}

#[test]
fn test_options_newline_invalid() {
	assert!(is_getopt_error(options(&[("--newline", "BOGUS")])));
}

#[test]
fn test_options_recalc_timestamp() -> TestResult {
	assert_eq!(
		options(&[("--recalc-timestamp", "")])?.recalc_timestamp,
		Some(true)
	);
	Ok(())
}

#[test]
fn test_options_no_recalc_timestamp() -> TestResult {
	assert_eq!(
		options(&[("--no-recalc-timestamp", "")])?.recalc_timestamp,
		Some(false)
	);
	Ok(())
}

#[test]
fn test_options_flavor() -> TestResult {
	assert_eq!(
		options(&[("--flavor", "woff")])?.flavor.as_deref(),
		Some("woff")
	);
	Ok(())
}

#[test]
fn test_options_with_zopfli() -> TestResult {
	assert!(options(&[("--with-zopfli", ""), ("--flavor", "woff")])?.use_zopfli);
	Ok(())
}

#[test]
fn test_options_with_zopfli_fails_without_woff_flavor() {
	assert!(is_getopt_error(options(&[("--with-zopfli", "")])));
}

#[test]
fn test_options_quiet_and_verbose_shouldfail() {
	assert!(is_getopt_error(options(&[("-q", ""), ("-v", "")])));
}

#[test]
fn test_options_mergefile_and_flavor_shouldfail() {
	assert!(is_getopt_error(options(&[
		("-m", "testfont.ttf"),
		("--flavor", "woff")
	])));
}

#[test]
fn test_options_onlytables_and_skiptables_shouldfail() {
	assert!(is_getopt_error(options(&[("-t", "CFF"), ("-x", "CFF2")])));
}

#[test]
fn test_options_mergefile_and_multiplefiles_shouldfail() {
	assert!(is_getopt_error(Options::new(&[("-m", "testfont.ttf")], 2)));
}

#[test]
fn test_options_woff2_and_zopfli_shouldfail() {
	assert!(is_getopt_error(options(&[
		("--with-zopfli", ""),
		("--flavor", "woff2")
	])));
}

// ----------------------------
// ttx.ttCompile function tests
// ----------------------------

const OTF_TABLES: [&[u8; 4]; 10] = [
	b"head", b"hhea", b"maxp", b"OS/2", b"name", b"cmap", b"post", b"CFF ", b"hmtx", b"DSIG",
];

const TTF_TABLES: [&[u8; 4]; 15] = [
	b"head", b"hhea", b"maxp", b"OS/2", b"name", b"cmap", b"hmtx", b"fpgm", b"prep", b"cvt ",
	b"loca", b"glyf", b"post", b"gasp", b"DSIG",
];

fn compile_with_flavor(inttx: &str, out_name: &str, flavor: Option<&str>) -> TestResult<Font> {
	let tmp = TempDir::new()?;
	let out = tmp.join(out_name);
	let mut options = Options::new(&[], 1)?;
	options.flavor = flavor.map(str::to_owned);
	ttx::tt_compile(&getpath(inttx), &out, &options)?;
	// Confirm that the font was built.
	assert!(Path::new(&out).is_file());
	Ok(read_font(&out)?)
}

#[test]
fn test_ttcompile_otf_compile_default() -> TestResult {
	let ttf = compile_with_flavor("TestOTF.ttx", "TestOTF.ttx", None)?;
	assert_has_tables(&ttf, &OTF_TABLES);
	Ok(())
}

#[test]
fn test_ttcompile_otf_to_woff_without_zopfli() -> TestResult {
	let ttf = compile_with_flavor("TestOTF.ttx", "TestOTF.woff", Some("woff"))?;
	assert_has_tables(&ttf, &OTF_TABLES);
	Ok(())
}

#[test]
fn test_ttcompile_otf_to_woff2() -> TestResult {
	let ttf = compile_with_flavor("TestOTF.ttx", "TestTTF.woff2", Some("woff2"))?;
	// DSIG should not be included from the original TTX, as per the WOFF2 spec.
	assert!(!ttf.contains(Tag::new(b"DSIG")));
	assert_has_tables(&ttf, &OTF_TABLES[..9]);
	Ok(())
}

#[test]
fn test_ttcompile_ttf_compile_default() -> TestResult {
	let ttf = compile_with_flavor("TestTTF.ttx", "TestTTF.ttf", None)?;
	assert_has_tables(&ttf, &TTF_TABLES);
	Ok(())
}

#[test]
fn test_ttcompile_ttf_to_woff_without_zopfli() -> TestResult {
	let ttf = compile_with_flavor("TestTTF.ttx", "TestTTF.woff", Some("woff"))?;
	assert_has_tables(&ttf, &TTF_TABLES);
	Ok(())
}

#[test]
fn test_ttcompile_ttf_to_woff2() -> TestResult {
	let ttf = compile_with_flavor("TestTTF.ttx", "TestTTF.woff2", Some("woff2"))?;
	assert!(!ttf.contains(Tag::new(b"DSIG")));
	assert_has_tables(&ttf, &TTF_TABLES[..14]);
	Ok(())
}

/// The `zopfli` tests are not ported, as fontoxide has no zopfli: the option
/// is rejected.
#[test]
fn ttcompile_with_zopfli_is_rejected() -> TestResult {
	let tmp = TempDir::new()?;
	let mut options = Options::new(&[], 1)?;
	options.flavor = Some("woff".into());
	options.use_zopfli = true;
	let result = ttx::tt_compile(&getpath("TestTTF.ttx"), &tmp.join("TestTTF.woff"), &options);
	assert!(result.is_err());
	Ok(())
}

fn modified(path: &str) -> fontoxide_ttlib::Result<u64> {
	Ok(read_font(path)?.require::<Head>()?.modified)
}

/// `timestampSinceEpoch(os.path.getmtime(path))`.
fn mtime_timestamp(path: &str) -> std::io::Result<i64> {
	let mtime = std::fs::metadata(path)?
		.modified()?
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0.0, |d| d.as_secs_f64());
	Ok((mtime + fontoxide_misc::time_tools::timestamp_since_epoch(0) as f64) as i64)
}

#[test]
fn test_ttcompile_timestamp_calcs() -> TestResult {
	for (inpath, outpath1, outpath2) in [
		("TestTTF.ttx", "TestTTF1.ttf", "TestTTF2.ttf"),
		("TestOTF.ttx", "TestOTF1.otf", "TestOTF2.otf"),
	] {
		let tmp = TempDir::new()?;
		let inttx = getpath(inpath);
		let outttf1 = tmp.join(outpath1);
		let outttf2 = tmp.join(outpath2);
		let mut options = Options::new(&[], 1)?;
		// Build with the default options: do not recalculate the timestamp.
		ttx::tt_compile(&inttx, &outttf1, &options)?;
		assert!(Path::new(&outttf1).is_file());
		// Confirm that the timestamp is the modification time of the TTX file.
		let epochtime = mtime_timestamp(&inttx)?;
		assert_eq!(modified(&outttf1)? as i64, epochtime);

		// Recalculate the timestamp.
		options.recalc_timestamp = Some(true);
		ttx::tt_compile(&inttx, &outttf2, &options)?;
		assert!(Path::new(&outttf2).is_file());
		assert!(modified(&outttf2)? as i64 > epochtime);

		// --no-recalc-timestamp keeps the original timestamp.
		options.recalc_timestamp = Some(false);
		ttx::tt_compile(&inttx, &outttf2, &options)?;
		let mut inttf = Font::new();
		inttf.import_ttx_file(&inttx)?;
		assert_eq!(inttf.require::<Head>()?.modified, modified(&outttf2)?);
	}
	Ok(())
}

// -------------------------
// ttx.ttList function tests
// -------------------------

fn list(inpath: &str, flavor: Option<&str>) -> TestResult<String> {
	let tmp = TempDir::new()?;
	let mut options = Options::new(&[], 1)?;
	options.list_tables = true;
	options.flavor = flavor.map(str::to_owned);
	let mut out = Vec::new();
	ttx::tt_list(&getpath(inpath), &tmp.join("fake.ttx"), &options, &mut out)?;
	Ok(String::from_utf8(out)?)
}

#[test]
fn test_ttlist_ttf() -> TestResult {
	let out = list("TestTTF.ttf", None)?;
	for table in TTF_TABLES {
		assert!(out.contains(&Tag::new(table).to_string()));
	}
	assert!(
		out.contains("OS/2  0x67230FF8        96       376"),
		"{out}"
	);
	Ok(())
}

#[test]
fn test_ttlist_otf() -> TestResult {
	let out = list("TestOTF.otf", None)?;
	for table in OTF_TABLES {
		assert!(out.contains(&Tag::new(table).to_string()));
	}
	assert!(
		out.contains("OS/2  0x67230FF8        96       272"),
		"{out}"
	);
	Ok(())
}

#[test]
fn test_ttlist_woff() -> TestResult {
	let out = list("TestWOFF.woff", Some("woff"))?;
	for table in OTF_TABLES {
		assert!(out.contains(&Tag::new(table).to_string()));
	}
	assert!(
		out.contains("OS/2  0x67230FF8        84       340"),
		"{out}"
	);
	Ok(())
}

#[test]
fn test_ttlist_woff2() -> TestResult {
	let out = list("TestWOFF2.woff2", Some("woff2"))?;
	for table in &TTF_TABLES[..14] {
		assert!(out.contains(&Tag::new(table).to_string()));
	}
	assert!(
		out.contains("OS/2  0x67230FF8        96         0"),
		"{out}"
	);
	Ok(())
}

// -------------------
// main function tests
// -------------------

#[test]
fn test_main_default_ttf_dump_to_ttx() -> TestResult {
	let tmp = TempDir::new()?;
	let outpath = tmp.join("TestTTF.ttx");
	assert_eq!(ttx::main(&["-o", &outpath, &getpath("TestTTF.ttf")])?, 0);
	assert!(Path::new(&outpath).is_file());
	Ok(())
}

#[test]
fn test_main_default_ttx_compile_to_ttf() -> TestResult {
	let tmp = TempDir::new()?;
	let outpath = tmp.join("TestTTF.ttf");
	assert_eq!(ttx::main(&["-o", &outpath, &getpath("TestTTF.ttx")])?, 0);
	assert!(Path::new(&outpath).is_file());
	Ok(())
}

#[test]
fn test_main_getopterror_missing_directory() -> TestResult {
	let inpath = getpath("TestTTF.ttf");
	let (code, _, stderr) = main_with_stdin(&["-d", "bogusdir", &inpath], b"")?;
	assert_eq!(code, 2);
	let stderr = String::from_utf8(stderr)?;
	assert!(stderr.contains("ERROR: The -d option value must be an existing directory"));
	Ok(())
}

/// `test_main_ttlib_error`, with a font that fails to open with a
/// `TTLibError` in place of a patched `process`.
#[test]
fn test_main_ttlib_error() -> TestResult {
	let tmp = TempDir::new()?;
	let inpath = tmp.join("Truncated.ttf");
	std::fs::write(&inpath, b"true\0\0")?;
	let outpath = tmp.join("Truncated.ttx");
	let (logs, code) = capture_logs(|| ttx::main(&["-o", &outpath, &inpath]));
	assert_eq!(code?, 1);
	assert!(
		logs.contains("Not a TrueType or OpenType font (not enough data)"),
		"{logs}"
	);
	Ok(())
}

/// `test_main_base_exception`, with a malformed TTX file in place of a
/// patched `process`.
#[test]
fn test_main_base_exception() -> TestResult {
	let tmp = TempDir::new()?;
	let inpath = tmp.join("Malformed.ttx");
	std::fs::write(&inpath, "<?xml version=\"1.0\"?>\n<ttFont><head>\n")?;
	let outpath = tmp.join("Malformed.ttf");
	let (logs, code) = capture_logs(|| ttx::main(&["-o", &outpath, &inpath]));
	assert_eq!(code?, 1);
	assert!(logs.contains("Unhandled exception has occurred"), "{logs}");
	Ok(())
}

#[test]
fn test_main_ttf_dump_stdin_to_stdout() -> TestResult {
	let inpath = getpath("TestTTF.ttf");
	let (code, stdout, _) = main_with_stdin(&["-q", "-o", "-", "-"], &std::fs::read(&inpath)?)?;
	assert_eq!(code, 0);
	// The same as dumping the file to a file, but for the line endings of
	// Python's text-mode standard output on Windows.
	let tmp = TempDir::new()?;
	let outpath = tmp.join("TestTTF.ttx");
	assert_eq!(ttx::main(&["-q", "-o", &outpath, &inpath])?, 0);
	let expected = std::fs::read_to_string(&outpath)?;
	let expected = if cfg!(windows) {
		expected.replace('\n', "\r\n")
	} else {
		expected
	};
	assert_eq!(String::from_utf8(stdout)?, expected);
	Ok(())
}

#[test]
fn test_main_ttx_compile_stdin_to_stdout() -> TestResult {
	let inpath = getpath("TestTTF.ttx");
	let (code, stdout, _) = main_with_stdin(&["-q", "-o", "-", "-"], &std::fs::read(&inpath)?)?;
	assert_eq!(code, 0);
	let font = Font::read(stdout, ReadOptions::default())?;
	assert_has_tables(&font, &TTF_TABLES);
	Ok(())
}

#[test]
fn test_main_gnu_style_opts_and_args_intermixed() -> TestResult {
	// https://github.com/fonttools/fonttools/issues/3507
	let tmp = TempDir::new()?;
	let outpath = tmp.join("TestTTF.ttx");
	assert_eq!(
		ttx::main(&["-t", "cmap", &getpath("TestTTF.ttf"), "-o", &outpath])?,
		0
	);
	assert!(Path::new(&outpath).is_file());
	Ok(())
}

#[test]
fn test_roundtrip_DSIG_split_at_XML_parse_buffer_size() -> TestResult {
	let inpath = getpath("roundtrip_DSIG_split_at_XML_parse_buffer_size.ttx");
	let mut font = Font::new();
	font.import_ttx_file(&inpath)?;
	// Just enough dummy bytes to hit the cut off point whereby the DSIG data
	// gets split into two chunks (https://github.com/fonttools/fonttools/issues/2614).
	font.insert(Box::new(DefaultTable::new(
		Tag::new(b"DMMY"),
		b"\x01\x02\x03\x04".repeat(2438),
	)));
	let tmp = TempDir::new()?;
	let ttx_path = tmp.join("roundtrip_DSIG_split_at_XML_parse_buffer_size.ttx");
	for file in font.to_ttx_files(&ttx_path, &TtxOptions::default())? {
		std::fs::write(&file.path, file.contents)?;
	}

	let outpath = tmp.join("font.ttf");
	assert_eq!(ttx::main(&["-q", "-o", &outpath, &ttx_path])?, 0);

	assert!(Path::new(&outpath).is_file());
	let mut font = read_font(&outpath)?;
	let dsig = font.require::<Dsig>()?;
	let records = dsig.signature_records.as_ref().ok_or("no records")?;
	// base64.b64decode(b"0000000100000000")
	assert_eq!(
		records[0].pkcs7,
		b"\xd3\x4d\x34\xd3\x4d\x35\xd3\x4d\x34\xd3\x4d\x34"
	);
	Ok(())
}

/// `test_main_ttx_compile_optimize_font_speed` is not ported, as fontoxide has
/// no `OPTIMIZE_FONT_SPEED` option: the option is rejected.
#[test]
fn optimize_font_speed_is_rejected() -> TestResult {
	let tmp = TempDir::new()?;
	let out = tmp.join("TestTTF-speed.ttf");
	let (_, code) =
		capture_logs(|| ttx::main(&["--optimize-font-speed", "-o", &out, &getpath("TestTTF.ttx")]));
	assert_eq!(code?, 1);
	Ok(())
}

// ----------------------------------------------------------------
// Byte identity with fontTools (tools/reference/ttx_fixtures.py)
// ----------------------------------------------------------------

/// A path under `tests/data/ttx`.
fn fixture(name: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data/ttx")
		.join(name)
}

/// The files of a directory, by name.
fn read_dir(path: &Path) -> std::io::Result<Vec<(String, Vec<u8>)>> {
	let mut files = Vec::new();
	for entry in std::fs::read_dir(path)? {
		let entry = entry?;
		files.push((
			entry.file_name().to_string_lossy().into_owned(),
			std::fs::read(entry.path())?,
		));
	}
	files.sort();
	Ok(files)
}

/// The dumps of `ttx` with several options, including the files of `-s` and
/// `-g`, are those of fontTools.
#[test]
fn dumps_match_fonttools() -> TestResult {
	for (case, font, options) in [
		("ttf", "TestTTF.ttf", &[][..]),
		("ttf_s", "TestTTF.ttf", &["-s"]),
		("ttf_g", "TestTTF.ttf", &["-g"]),
		("ttf_i", "TestTTF.ttf", &["-i"]),
		("ttf_crlf", "TestTTF.ttf", &["--newline", "CRLF"]),
		(
			"ttf_t",
			"TestTTF.ttf",
			&["-t", "cmap", "-t", "glyf", "-t", "GlyphOrder"],
		),
		("ttf_x", "TestTTF.ttf", &["-x", "glyf", "-x", "GlyphOrder"]),
		(
			"ttf_g_t",
			"TestTTF.ttf",
			&["-g", "-t", "glyf", "-t", "head"],
		),
		("otf", "TestOTF.otf", &[]),
		("otf_s", "TestOTF.otf", &["-s"]),
		("woff", "TestWOFF.woff", &[]),
		("woff2", "TestWOFF2.woff2", &[]),
		("ttc_1", "TestTTC.ttc", &["-y", "1"]),
	] {
		let tmp = TempDir::new()?;
		let stem = font.split('.').next().unwrap_or_default();
		let output = tmp.join(&format!("{stem}.ttx"));
		let input = getpath(font);
		let mut args = vec!["-q"];
		args.extend(options);
		args.extend(["-o", &output, &input]);
		assert_eq!(ttx::main(&args)?, 0, "{case}");
		let expected = read_dir(&fixture(&format!("dump_{case}")))?;
		let actual = read_dir(&tmp.0)?;
		let names = |files: &[(String, Vec<u8>)]| -> Vec<String> {
			files.iter().map(|(n, _)| n.clone()).collect()
		};
		assert_eq!(names(&actual), names(&expected), "{case}");
		for ((name, actual), (_, expected)) in actual.iter().zip(&expected) {
			assert!(
				actual == expected,
				"{case}/{name}:\n{}",
				String::from_utf8_lossy(actual)
			);
		}
	}
	Ok(())
}

/// The table lists of `ttx -l` are those of fontTools.
#[test]
fn lists_match_fonttools() -> TestResult {
	for font in [
		"TestTTF.ttf",
		"TestOTF.otf",
		"TestWOFF.woff",
		"TestWOFF2.woff2",
	] {
		let input = getpath(font);
		let mut out = Vec::new();
		ttx::tt_list(&input, "", &Options::new(&[], 1)?, &mut out)?;
		let actual = String::from_utf8(out)?.replace(&input, font);
		let stem = font.split('.').next().unwrap_or_default();
		let expected = std::fs::read_to_string(fixture(&format!("list_{stem}.txt")))?;
		assert_eq!(actual, expected, "{font}");
	}
	Ok(())
}

/// The fonts that `ttx` compiles with several options are those of fontTools.
#[test]
fn compiles_match_fonttools() -> TestResult {
	for (case, font, options) in [
		("ttf", "TestTTF.ttx", &[][..]),
		("ttf_b", "TestTTF.ttx", &["-b"]),
		("ttf_woff", "TestTTF.ttx", &["--flavor", "woff"]),
		("ttf_woff2", "TestTTF.ttx", &["--flavor", "woff2"]),
		("otf", "TestOTF.ttx", &[]),
		("otf_woff", "TestOTF.ttx", &["--flavor", "woff"]),
		("otf_woff2", "TestOTF.ttx", &["--flavor", "woff2"]),
	] {
		let tmp = TempDir::new()?;
		let output = tmp.join("font.bin");
		let input = getpath(font);
		let mut args = vec!["-q", "--no-recalc-timestamp"];
		args.extend(options);
		args.extend(["-o", &output, &input]);
		assert_eq!(ttx::main(&args)?, 0, "{case}");
		let expected = std::fs::read(fixture(&format!("compile_{case}.bin")))?;
		let actual = std::fs::read(&output)?;
		if expected.starts_with(b"wOFF") {
			// The zlib streams of flate2 differ from those of zlib: compare the
			// table directories and the uncompressed tables.
			assert_eq!(woff_canonical(actual)?, woff_canonical(expected)?, "{case}");
		} else {
			assert!(actual == expected, "{case}");
		}
	}
	Ok(())
}

/// A table record of WOFF 1.0 without the compressed length and offset:
/// the tag, the checksum, the length and the uncompressed data.
type WoffTable = (Tag, u32, u32, Vec<u8>);

/// The table records of a WOFF 1.0 file without the compressed lengths and
/// offsets, with the uncompressed tables.
fn woff_canonical(data: Vec<u8>) -> TestResult<Vec<WoffTable>> {
	use fontoxide_ttlib::sfnt::{SfntReader, SfntReaderOptions};
	let reader = SfntReader::new(data, SfntReaderOptions::default())?;
	let mut tables = Vec::new();
	for e in reader.entries() {
		tables.push((
			e.tag,
			e.check_sum,
			e.orig_length,
			reader.table_data(e.tag)?.to_vec(),
		));
	}
	Ok(tables)
}

/// Compiling the files of `-g` gives the font of compiling the single file.
#[test]
fn split_glyphs_import() -> TestResult {
	let compile = |path: PathBuf| -> fontoxide_ttlib::Result<Vec<u8>> {
		let mut font = Font::new();
		font.recalc_timestamp = false;
		font.import_ttx_file(path)?;
		font.write(&Default::default())
	};
	assert_eq!(
		compile(fixture("dump_ttf_g/TestTTF.ttx"))?,
		compile(fixture("dump_ttf/TestTTF.ttx"))?
	);
	Ok(())
}
