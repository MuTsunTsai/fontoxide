//! `Tests/varLib/interpolatable_test.py`.
//!
//! Besides the assertions of the upstream tests, the complete reports of
//! `main` (text, JSON, `--show-all`, and the problems returned) are compared
//! with those of fontTools for every font set the tests use, a few more, and
//! synthetic masters with every kind of problem, with and without SciPy. The
//! references come from `tools/reference/interpolatable_fixtures.py`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_designspacelib::{
	Axis, AxisDescriptor, DesignSpaceDocument, DiscreteAxisDescriptor, LocationValue, Number,
	SourceDescriptor,
};
use fontoxide_pens::{PointPen, SegmentType};
use fontoxide_ttlib::{Font, WriteOptions};
use fontoxide_ufolib::{
	Dict, GlifPointPen, UfoWriter, UfoWriterOptions, Value, WriterGlyphSetOptions,
};
use fontoxide_varlib::interpolatable::helpers::Location;
use fontoxide_varlib::interpolatable::scipy::{linear_sum_assignment, minimum_spanning_tree};
use fontoxide_varlib::interpolatable::{
	Backend, InterpolatableError, MasterPair, Problem, Problems, find_parents_and_order,
	problems_to_json, run, run_with_backend,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const DATA: &str = concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../upstream/fonttools/Tests/varLib/data"
);
const TTLIB_DATA: &str = concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../upstream/fonttools/Tests/ttLib/data"
);
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/interpolatable");

/// A temporary directory, removed when dropped (`self.tempdir`).
struct TempDir(PathBuf);

impl TempDir {
	fn new() -> TestResult<Self> {
		static COUNTER: AtomicUsize = AtomicUsize::new(0);
		let path = std::env::temp_dir().join(format!(
			"fontoxide-interpolatable-{}-{}",
			std::process::id(),
			COUNTER.fetch_add(1, Ordering::Relaxed)
		));
		std::fs::create_dir_all(&path)?;
		Ok(TempDir(path))
	}

	fn path(&self) -> &Path {
		&self.0
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

/// `get_test_input`.
fn test_input(parts: &[&str]) -> String {
	let mut path = PathBuf::from(DATA);
	for p in parts {
		path.push(p);
	}
	path.to_string_lossy().into_owned()
}

/// `get_file_list`: the files of `folder` with a prefix and a suffix, sorted.
fn file_list(folder: impl AsRef<Path>, suffix: &str, prefix: &str) -> TestResult<Vec<String>> {
	let mut out = Vec::new();
	for entry in std::fs::read_dir(folder.as_ref())? {
		let entry = entry?;
		let name = entry.file_name().to_string_lossy().into_owned();
		if name.starts_with(prefix) && name.ends_with(suffix) {
			out.push(entry.path().to_string_lossy().into_owned());
		}
	}
	out.sort();
	Ok(out)
}

/// `compile_font`: a TTX file saved as a binary font in `dir`.
fn compile_font(path: &str, suffix: &str, dir: &Path) -> TestResult<(Font, String)> {
	let name = Path::new(path)
		.file_name()
		.map(|n| n.to_string_lossy().replace(".ttx", suffix))
		.unwrap_or_default();
	let save_path = dir.join(name).to_string_lossy().into_owned();
	let mut font = Font::new();
	font.recalc_bboxes = false;
	font.recalc_timestamp = false;
	font.import_ttx(&std::fs::read_to_string(path)?)?;
	save(&mut font, &save_path)?;
	Ok((font, save_path))
}

/// `font.save(path, reorderTables=None)`.
fn save(font: &mut Font, path: &str) -> TestResult {
	let data = font.write(&WriteOptions {
		reorder_tables: None,
		..Default::default()
	})?;
	std::fs::write(path, data)?;
	Ok(())
}

/// Compiles the TTX files of a folder with a prefix.
fn compile_all(folder: &str, prefix: &str, suffix: &str, dir: &Path) -> TestResult<Vec<String>> {
	for path in file_list(folder, ".ttx", prefix)? {
		compile_font(&path, suffix, dir)?;
	}
	file_list(dir, suffix, prefix)
}

/// `interpolatable_main(args)`, with the report discarded.
fn interpolatable_main(args: &[&str]) -> Result<Option<Problems>, InterpolatableError> {
	run(args, &mut Vec::new())
}

fn strs(v: &[String]) -> Vec<&str> {
	v.iter().map(String::as_str).collect()
}

/// The report of `main` as the fixture script writes it.
fn report(args: &[&str], inputs: &[String], backend: Backend) -> String {
	let mut all: Vec<&str> = args.to_vec();
	all.extend(strs(inputs));
	let mut out = Vec::new();
	let result = run_with_backend(&all, &mut out, backend);
	let mut text = format!("=== {}\n{}", args.join("\t"), String::from_utf8_lossy(&out));
	match result {
		Ok(None) => text.push_str("--- return\nNone\n"),
		Ok(Some(problems)) => {
			text.push_str("--- return\n");
			text.push_str(&problems_to_json(&problems));
			text.push('\n');
		}
		Err(e) => {
			text.push_str("--- error\n");
			text.push_str(&e.to_string());
			text.push('\n');
		}
	}
	text
}

/// Compares the reports of `main` with those of fontTools, with and without
/// SciPy. `lenient` lists the option sets whose fontTools report ends with an
/// error that fontoxide does not raise.
fn check_case_with(name: &str, inputs: &[String], lenient: &[&str]) -> TestResult {
	for (suffix, backend) in [
		(".txt", Backend::Scipy),
		(".fallback.txt", Backend::Fallback),
	] {
		let expected =
			std::fs::read_to_string(Path::new(FIXTURES).join(format!("{name}{suffix}")))?;
		let expected = expected.replace("\r\n", "\n");
		for block in expected.split("=== ").skip(1) {
			let header = block.lines().next().unwrap_or_default();
			let args: Vec<&str> = header.split('\t').filter(|a| !a.is_empty()).collect();
			let expected = format!("=== {block}");
			let actual = report(&args, inputs, backend);
			if lenient.contains(&header) {
				// fontTools fails; fontoxide reports the same lines first.
				let (expected_head, _) = expected
					.split_once("--- error\n")
					.ok_or("expected an error")?;
				assert!(
					actual.starts_with(expected_head),
					"{name}{suffix} [{header}]:\n{actual}"
				);
				continue;
			}
			if actual != expected {
				let first = actual
					.lines()
					.zip(expected.lines())
					.position(|(a, b)| a != b)
					.unwrap_or(0);
				panic!(
					"{name}{suffix} [{header}] differs at line {first}:\n  actual:   {:?}\n  expected: {:?}",
					actual.lines().nth(first),
					expected.lines().nth(first)
				);
			}
		}
	}
	Ok(())
}

fn check_case(name: &str, inputs: &[String]) -> TestResult {
	check_case_with(name, inputs, &[])
}

fn masters(m1: &str, m2: &str, i1: usize, i2: usize) -> MasterPair {
	MasterPair {
		master_1: m1.into(),
		master_2: m2.into(),
		master_1_idx: i1,
		master_2_idx: i2,
	}
}

fn missing(master: &str, master_idx: usize) -> Vec<Problem> {
	vec![Problem::Missing {
		master: master.into(),
		master_idx,
	}]
}

// -----
// Tests
// -----

#[test]
fn test_interpolatable_ttf() -> TestResult {
	let tmp = TempDir::new()?;
	let ttx_dir = test_input(&["master_ttx_interpolatable_ttf"]);
	let ttf_paths = compile_all(&ttx_dir, "TestFamily2-", ".ttf", tmp.path())?;
	assert!(interpolatable_main(&strs(&ttf_paths))?.is_none());
	check_case("ttf_TestFamily2", &ttf_paths)
}

#[test]
fn test_interpolatable_beyond64k_variable_ttf() -> TestResult {
	let tmp = TempDir::new()?;
	let ttx_path = test_input(&["MutatorSans_All_Variable.ttx"]);
	let (mut font, font_path) = compile_font(&ttx_path, ".ttf", tmp.path())?;
	fontoxide_ttlib::beyond64k::upper_tables(&mut font, &Default::default())?;
	save(&mut font, &font_path)?;
	assert!(interpolatable_main(&[&font_path])?.is_none());
	check_case("beyond64k", &[font_path])
}

#[test]
fn test_interpolatable_otf() -> TestResult {
	let tmp = TempDir::new()?;
	let ttx_dir = test_input(&["master_ttx_interpolatable_otf"]);
	let otf_paths = compile_all(&ttx_dir, "TestFamily2-", ".otf", tmp.path())?;
	assert!(interpolatable_main(&strs(&otf_paths))?.is_none());
	check_case("otf_TestFamily2", &otf_paths)
}

fn cff2_font(tmp: &TempDir) -> TestResult<String> {
	let ttx_path = test_input(&[
		"variable_ttx_interpolatable_cff2",
		"interpolatable-test.ttx",
	]);
	Ok(compile_font(&ttx_path, ".otf", tmp.path())?.1)
}

#[test]
fn test_interpolatable_cff2() -> TestResult {
	let tmp = TempDir::new()?;
	let otf_path = cff2_font(&tmp)?;
	let problems = interpolatable_main(&[&otf_path])?.ok_or("no problems")?;
	let uni0408 = &problems["uni0408"];
	assert_eq!(uni0408.len(), 1);
	let Problem::Underweight {
		contour,
		masters: m,
		tolerance,
	} = &uni0408[0]
	else {
		panic!("{uni0408:?}");
	};
	assert_eq!(*contour, 0);
	assert_eq!(
		*m,
		masters("'wght=200.0 opsz=20.0'", "'wght=200.0 opsz=60.0'", 2, 3)
	);
	assert!((tolerance - 0.9184032411892079).abs() < 1e-6);
	check_case("cff2", &[otf_path])
}

#[test]
fn test_interpolatable_ufo() -> TestResult {
	let ttx_dir = test_input(&["master_ufo"]);
	let ufo_paths = file_list(&ttx_dir, ".ufo", "TestFamily2-")?;
	assert!(interpolatable_main(&strs(&ufo_paths))?.is_none());
	check_case("ufo_TestFamily2", &ufo_paths)
}

#[test]
fn test_interpolatable_cff2_json() -> TestResult {
	let tmp = TempDir::new()?;
	let otf_path = cff2_font(&tmp)?;
	let mut output = Vec::new();
	let problems = run(&["--json", &otf_path], &mut output)?.ok_or("no problems")?;
	// One glyph: the order of the report and of the sorted problems agree.
	assert_eq!(
		String::from_utf8(output)?,
		problems_to_json(&problems) + "\n"
	);
	let m = problems["uni0408"][0].masters().ok_or("no masters")?;
	assert_eq!(m.master_1_idx, 2);
	assert_eq!(m.master_2_idx, 3);
	Ok(())
}

#[test]
fn test_designspace() -> TestResult {
	let designspace_path = test_input(&["InterpolateLayout.designspace"]);
	assert!(interpolatable_main(&[&designspace_path])?.is_none());
	check_case("ds_InterpolateLayout", &[designspace_path])
}

#[test]
fn test_designspace_decreasing_axis_map() -> TestResult {
	let tmp = TempDir::new()?;
	let mut doc = DesignSpaceDocument::from_file(test_input(&["InterpolateLayout.designspace"]))?;
	if let Some(Axis::Continuous(axis)) = doc.axes.first_mut() {
		axis.map = vec![
			(Number::Int(0), Number::Int(1000)),
			(Number::Int(1000), Number::Int(0)),
		];
	}
	for source in &mut doc.sources {
		let weight = source
			.design_location
			.get("weight")
			.map_or(Number::Int(0), |v| v.x());
		source.design_location.clear();
		source.design_location.insert(
			"weight".into(),
			LocationValue::Value(Number::Int(1000) - weight),
		);
		// save absolute paths with tostring() rather than write(), which makes
		// them relative to the temp dir and fails if it's on another drive
		source.filename = source.path.clone();
	}
	let path = tmp.join("tmp1.designspace");
	std::fs::write(&path, doc.tostring()?)?;
	assert!(interpolatable_main(&[&path])?.is_none());
	// The fixture script names the file differently; the report does not show it.
	check_case("ds_decreasing", &[path])
}

// `test_glyphsapp`: n/a, glyphsLib is not available.

#[test]
fn test_vf() -> TestResult {
	let tmp = TempDir::new()?;
	let ttx_dir = test_input(&["master_ttx_varfont_ttf"]);
	let ttf_paths = compile_all(&ttx_dir, "SparseMasters-", ".ttf", tmp.path())?;
	let mut args = vec!["--quiet"];
	args.extend(strs(&ttf_paths));
	assert!(interpolatable_main(&args)?.is_none());
	check_case("vf_SparseMasters-VF", &ttf_paths)
}

fn check_sparse(problems: &Problems, medium: &str) {
	for glyph in ["a", "s", "edotabove", "dotabovecomb"] {
		assert_eq!(problems[glyph], missing(medium, 1), "{glyph}");
	}
}

#[test]
fn test_sparse_interpolatable_ttfs() -> TestResult {
	let tmp = TempDir::new()?;
	let ttx_dir = test_input(&["master_ttx_interpolatable_ttf"]);
	let p = compile_all(&ttx_dir, "SparseMasters-", ".ttf", tmp.path())?;

	// without --ignore-missing
	let mut args = vec!["--quiet"];
	args.extend(strs(&p));
	check_sparse(
		&interpolatable_main(&args)?.ok_or("no problems")?,
		"SparseMasters-Medium",
	);

	// normal order, with --ignore-missing
	let mut args = vec!["--ignore-missing"];
	args.extend(strs(&p));
	assert!(interpolatable_main(&args)?.is_none());
	// purposely putting the sparse master (medium) first
	assert!(interpolatable_main(&["--ignore-missing", &p[1], &p[0], &p[2]])?.is_none());
	// purposely putting the sparse master (medium) last
	assert!(interpolatable_main(&["--ignore-missing", &p[0], &p[2], &p[1]])?.is_none());

	check_case("ttf_SparseMasters", &p)?;
	check_case(
		"ttf_SparseMasters_medium_first",
		&[p[1].clone(), p[0].clone(), p[2].clone()],
	)?;
	check_case(
		"ttf_SparseMasters_medium_last",
		&[p[0].clone(), p[2].clone(), p[1].clone()],
	)
}

#[test]
fn test_sparse_interpolatable_ufos() -> TestResult {
	let ttx_dir = test_input(&["master_ufo"]);
	let p = file_list(&ttx_dir, ".ufo", "SparseMasters-")?;

	// without --ignore-missing
	let mut args = vec!["--quiet"];
	args.extend(strs(&p));
	check_sparse(
		&interpolatable_main(&args)?.ok_or("no problems")?,
		"SparseMasters-Medium",
	);

	// normal order, with --ignore-missing
	let mut args = vec!["--ignore-missing"];
	args.extend(strs(&p));
	assert!(interpolatable_main(&args)?.is_none());
	// purposely putting the sparse master (medium) first
	assert!(interpolatable_main(&["--ignore-missing", &p[1], &p[0], &p[2]])?.is_none());
	// purposely putting the sparse master (medium) last
	assert!(interpolatable_main(&["--ignore-missing", &p[0], &p[2], &p[1]])?.is_none());

	check_case("ufo_SparseMasters", &p)?;
	check_case(
		"ufo_SparseMasters_medium_first",
		&[p[1].clone(), p[0].clone(), p[2].clone()],
	)?;
	check_case(
		"ufo_SparseMasters_medium_last",
		&[p[0].clone(), p[2].clone(), p[1].clone()],
	)
}

#[test]
fn test_sparse_designspace() -> TestResult {
	let designspace_path = test_input(&["SparseMasters_ufo.designspace"]);
	let problems = interpolatable_main(&["--quiet", &designspace_path])?.ok_or("no problems")?;
	check_sparse(&problems, "SparseMasters-Medium");
	// normal order, with --ignore-missing
	assert!(interpolatable_main(&["--ignore-missing", &designspace_path])?.is_none());
	check_case("ds_SparseMasters", &[designspace_path])
}

// `test_sparse_glyphsapp`: n/a, glyphsLib is not available.
// `test_html_report_escapes_glyph_name`: n/a, the HTML report (cairo) is not
// ported.

#[test]
fn test_html_report_is_unsupported() {
	let result = interpolatable_main(&["--quiet", "--html", "report.html", "a.ttf", "b.ttf"]);
	assert!(matches!(result, Err(InterpolatableError::Unsupported(_))));
}

#[test]
fn test_interpolatable_var_composite() -> TestResult {
	let input_path = Path::new(TTLIB_DATA)
		.join("varc-ac00-ac01.ttf")
		.to_string_lossy()
		.into_owned();
	// Just make sure the code runs.
	interpolatable_main(&[&input_path])?;
	check_case("varc", &[input_path])
}

/// The `layered_designspace` fixture: `Regular.ufo` with an `intermediate`
/// layer, and `Bold.ufo`.
struct Layered {
	_tmp: TempDir,
	document: DesignSpaceDocument,
	path: String,
}

impl Layered {
	fn new() -> TestResult<Self> {
		let tmp = TempDir::new()?;
		let paths = [tmp.join("Regular.ufo"), tmp.join("Bold.ufo")];
		for (i, path) in paths.iter().enumerate() {
			let mut writer = UfoWriter::new(
				path,
				UfoWriterOptions {
					format_version: Some(3.into()),
					..Default::default()
				},
			)?;
			let mut info = Dict::new();
			info.insert("unitsPerEm".into(), Value::Integer(1000));
			writer.write_info(&info, None)?;
			let mut layers: Vec<(Option<&str>, usize, &[&str])> = vec![(None, 1, &["A", "B"])];
			if i == 0 {
				layers.push((Some("intermediate"), 2, &["A"]));
			}
			for (layer, contours, glyphs) in layers {
				let mut glyph_set = writer.get_glyph_set(
					layer,
					layer.is_none(),
					WriterGlyphSetOptions::default(),
				)?;
				for glyph in glyphs {
					let mut glyph_object = Dict::new();
					glyph_object.insert("width".into(), Value::Integer(500));
					let mut draw = |pen: &mut GlifPointPen| -> fontoxide_ufolib::Result<()> {
						for i in 0..contours {
							let y = (i * 300) as f64;
							PointPen::begin_path(pen, None)?;
							for point in
								[(0.0, y), (200.0, y), (200.0, y + 200.0), (0.0, y + 200.0)]
							{
								PointPen::add_point(
									pen,
									point,
									Some(SegmentType::Line),
									false,
									None,
									None,
								)?;
							}
							PointPen::end_path(pen)?;
						}
						Ok(())
					};
					glyph_set.write_glyph(
						glyph,
						Some(&glyph_object),
						Some(&mut draw),
						None,
						None,
					)?;
				}
				glyph_set.write_contents()?;
			}
			writer.write_layer_contents(None, None)?;
			writer.close()?;
		}

		let mut document = DesignSpaceDocument::new();
		let axis = AxisDescriptor {
			name: Some("Weight".into()),
			tag: Some("wght".into()),
			minimum: Number::Int(100),
			default: Number::Int(100),
			maximum: Number::Int(900),
			..Default::default()
		};
		document.axes.push(Axis::Continuous(axis));
		for (path, location, layer) in [
			(&paths[0], 100, None),
			(&paths[0], 400, Some("intermediate")),
			(&paths[1], 900, None),
		] {
			let mut source = SourceDescriptor::new();
			source.set_path(Some(path));
			source
				.design_location
				.insert("Weight".into(), LocationValue::Value(Number::Int(location)));
			source.layer_name = layer.map(str::to_owned);
			document.sources.push(source);
		}
		let path = tmp.join("Layered.designspace");
		document.write(&path)?;
		Ok(Layered {
			_tmp: tmp,
			document,
			path,
		})
	}

	fn write(&mut self) -> TestResult {
		self.document.write(&self.path)?;
		Ok(())
	}
}

fn path_count_a() -> Problem {
	Problem::PathCount {
		masters: masters("Regular", "Regular (intermediate)", 0, 1),
		value_1: 1,
		value_2: 2,
	}
}

fn designspace_named_layer(axes: &str) -> TestResult {
	let mut layered = Layered::new()?;
	if axes == "decreasing" {
		if let Some(Axis::Continuous(axis)) = layered.document.axes.first_mut() {
			axis.map = vec![
				(Number::Int(100), Number::Int(900)),
				(Number::Int(900), Number::Int(100)),
			];
		}
		for source in &mut layered.document.sources {
			let weight = source
				.design_location
				.get("Weight")
				.map_or(Number::Int(0), |v| v.x());
			source.design_location.insert(
				"Weight".into(),
				LocationValue::Value(Number::Int(1000) - weight),
			);
		}
	} else if axes == "discrete" {
		let axis = DiscreteAxisDescriptor {
			name: Some("Style".into()),
			tag: Some("STYL".into()),
			values: vec![Number::Int(0), Number::Int(1)],
			default: Number::Int(0),
			..Default::default()
		};
		layered.document.axes.push(Axis::Discrete(axis));
		for (i, source) in layered.document.sources.iter_mut().enumerate() {
			source.design_location.insert(
				"Style".into(),
				LocationValue::Value(Number::Int(i64::from(i == 2))),
			);
		}
		// Each discrete subspace needs its own default master.
		let mut source = SourceDescriptor::new();
		source.path = layered.document.sources[0].path.clone();
		source
			.design_location
			.insert("Weight".into(), LocationValue::Value(Number::Int(100)));
		source
			.design_location
			.insert("Style".into(), LocationValue::Value(Number::Int(1)));
		layered.document.sources.push(source);
	}
	layered.write()?;
	let problems =
		interpolatable_main(&["--quiet", "--glyphs", "A", &layered.path])?.ok_or("no problems")?;
	assert_eq!(problems["A"][0], path_count_a());
	Ok(())
}

#[test]
fn test_designspace_named_layer_continuous() -> TestResult {
	designspace_named_layer("continuous")
}

#[test]
fn test_designspace_named_layer_decreasing() -> TestResult {
	designspace_named_layer("decreasing")
}

#[test]
fn test_designspace_named_layer_discrete() -> TestResult {
	designspace_named_layer("discrete")
}

fn designspace_default_layer(layer: Option<&str>) -> TestResult {
	let mut layered = Layered::new()?;
	layered.document.sources[1].layer_name = layer.map(str::to_owned);
	layered.write()?;
	assert!(interpolatable_main(&["--quiet", &layered.path])?.is_none());
	Ok(())
}

#[test]
fn test_designspace_default_layer_none() -> TestResult {
	designspace_default_layer(None)
}

#[test]
fn test_designspace_default_layer_public_default() -> TestResult {
	designspace_default_layer(Some("public.default"))
}

#[test]
fn test_designspace_sparse_layer() -> TestResult {
	let layered = Layered::new()?;
	let args = ["--quiet", "--glyphs", "B", &layered.path];
	assert_eq!(
		interpolatable_main(&args)?.ok_or("no problems")?["B"],
		missing("Regular (intermediate)", 1)
	);
	let mut args2 = vec!["--ignore-missing"];
	args2.extend(args);
	assert!(interpolatable_main(&args2)?.is_none());
	Ok(())
}

#[test]
fn test_designspace_missing_layer() -> TestResult {
	let mut layered = Layered::new()?;
	layered.document.sources[1].layer_name = Some("nonexistent".into());
	layered.write()?;
	match interpolatable_main(&["--quiet", &layered.path]) {
		Err(InterpolatableError::Ufo(e)) => assert!(e.to_string().contains("nonexistent"), "{e}"),
		other => panic!("{other:?}"),
	}
	Ok(())
}

#[test]
fn test_designspace_layer_name_filter() -> TestResult {
	let layered = Layered::new()?;
	assert!(
		interpolatable_main(&[
			"--quiet",
			"--name",
			"Regular",
			"--name",
			"Bold",
			&layered.path
		])?
		.is_none()
	);
	let problems = interpolatable_main(&[
		"--quiet",
		"--glyphs",
		"A",
		"--name",
		"Regular",
		"--name",
		"Regular (intermediate)",
		&layered.path,
	])?
	.ok_or("no problems")?;
	let Problem::PathCount { masters: m, .. } = &problems["A"][0] else {
		panic!("{problems:?}");
	};
	assert_eq!(m.master_2, "Regular (intermediate)");
	Ok(())
}

fn loc(items: &[(&str, f64)]) -> Location {
	items.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
}

#[test]
fn test_find_parents_and_order_scipy() -> TestResult {
	type Case = (
		Vec<Location>,
		Vec<&'static str>,
		Vec<Option<usize>>,
		Vec<usize>,
	);
	let cases: Vec<Case> = vec![
		(
			vec![loc(&[]), loc(&[("wght", 0.5)]), loc(&[("wght", 1.0)])],
			vec![],
			vec![None, Some(0), Some(1)],
			vec![0, 1, 2],
		),
		(
			vec![loc(&[("wght", 1.0)]), loc(&[]), loc(&[("wght", 0.5)])],
			vec![],
			vec![Some(2), None, Some(1)],
			vec![1, 2, 0],
		),
		(
			vec![
				loc(&[]),
				loc(&[("wght", 1.0)]),
				loc(&[("wdth", 2.0)]),
				loc(&[("wght", 1.0), ("wdth", 2.0)]),
			],
			vec![],
			vec![None, Some(0), Some(0), Some(2)],
			vec![0, 1, 2, 3],
		),
		(
			vec![
				loc(&[("ital", 0.0)]),
				loc(&[("ital", 0.0), ("wght", 0.5)]),
				loc(&[("ital", 1.0)]),
				loc(&[("ital", 1.0), ("wght", 0.5)]),
			],
			vec!["ital"],
			vec![None, Some(0), None, Some(2)],
			vec![0, 2, 1, 3],
		),
	];
	for (locations, discrete_axes, parents, order) in cases {
		let discrete: BTreeSet<String> = discrete_axes.iter().map(|s| (*s).to_owned()).collect();
		let (actual_parents, actual_order) =
			find_parents_and_order(locations.len(), Some(&locations), &discrete, Backend::Scipy)?;
		assert_eq!(actual_parents, parents);
		assert_eq!(actual_order, order);
	}
	Ok(())
}

#[test]
fn test_find_parents_and_order_without_locations() -> TestResult {
	for backend in [Backend::Scipy, Backend::Fallback] {
		let (parents, order) = find_parents_and_order(3, None, &BTreeSet::new(), backend)?;
		assert_eq!(parents, [None, Some(0), Some(1)]);
		assert_eq!(order, [0, 1, 2]);
	}
	Ok(())
}

// ------------------------------------------------------------------------
// Beyond the upstream tests: complete reports, and the SciPy routines.

#[test]
fn reports_of_other_ttf_masters() -> TestResult {
	let tmp = TempDir::new()?;
	let ttx_dir = test_input(&["master_ttx_interpolatable_ttf"]);
	for prefix in [
		"TestFamily-",
		"TestFamily3-",
		"TestFamily4-",
		"InconsistentUseMyMetrics-",
	] {
		let paths = compile_all(&ttx_dir, prefix, ".ttf", tmp.path())?;
		check_case(&format!("ttf_{}", prefix.trim_end_matches('-')), &paths)?;
	}
	Ok(())
}

#[test]
fn reports_of_other_ufo_masters() -> TestResult {
	let dir = test_input(&["master_ufo"]);
	for prefix in ["TestFamily-", "TestFamily3-", "TestFamily4-"] {
		let paths = file_list(&dir, ".ufo", prefix)?;
		check_case(&format!("ufo_{}", prefix.trim_end_matches('-')), &paths)?;
	}
	Ok(())
}

#[test]
fn report_of_mutator_iup() -> TestResult {
	let tmp = TempDir::new()?;
	let (_, path) = compile_font(
		&test_input(&["master_ttx_varfont_ttf", "Mutator_IUP.ttx"]),
		".ttf",
		tmp.path(),
	)?;
	check_case("vf_Mutator_IUP", &[path])
}

fn synthetic(name: &str) -> String {
	Path::new(FIXTURES)
		.join("synthetic")
		.join(name)
		.to_string_lossy()
		.into_owned()
}

#[test]
fn reports_of_synthetic_binary_masters() -> TestResult {
	let tmp = TempDir::new()?;
	for (kind, suffix) in [("TTF", ".ttf"), ("OTF", ".otf")] {
		let paths: Vec<String> = (0..3)
			.map(|m| {
				compile_font(
					&synthetic(&format!("Synthetic{kind}-Master{m}.ttx")),
					suffix,
					tmp.path(),
				)
			})
			.map(|r| r.map(|(_, p)| p))
			.collect::<TestResult<_>>()?;
		let kind = kind.to_lowercase();
		check_case(&format!("synthetic_{kind}"), &paths)?;
		check_case(
			&format!("synthetic_{kind}_reordered"),
			&[paths[2].clone(), paths[0].clone(), paths[1].clone()],
		)?;
	}
	Ok(())
}

#[test]
fn reports_of_synthetic_ufo_masters() -> TestResult {
	let paths: Vec<String> = (0..3)
		.map(|m| synthetic(&format!("SyntheticUFO-Master{m}.ufo")))
		.collect();
	check_case("synthetic_ufo", &paths)
}

/// LENIENT(L40): fontTools fails with a `TypeError` when a glyph of several
/// contours has an open contour.
#[test]
fn reports_of_open_contours() -> TestResult {
	let paths: Vec<String> = (0..3)
		.map(|m| synthetic(&format!("SyntheticOpen-Master{m}.ufo")))
		.collect();
	check_case_with(
		"synthetic_open",
		&paths,
		&["--glyphs\topen.1", "--glyphs\topen.2"],
	)
}

/// Reads the rows of a matrix, then the last field.
fn parse_matrix_line(line: &str) -> TestResult<(Vec<Vec<f64>>, String)> {
	let mut fields: Vec<&str> = line.split(" | ").collect();
	let last = fields.pop().ok_or("empty line")?.trim().to_owned();
	let rows = fields
		.iter()
		.map(|row| {
			row.split(' ')
				.map(str::parse::<f64>)
				.collect::<Result<Vec<_>, _>>()
		})
		.collect::<Result<Vec<_>, _>>()?;
	Ok((rows, last))
}

#[test]
fn linear_sum_assignment_matches_scipy() -> TestResult {
	let text = std::fs::read_to_string(Path::new(FIXTURES).join("lsap.txt"))?;
	for line in text.lines().filter(|l| !l.is_empty()) {
		let (cost, cols) = parse_matrix_line(line)?;
		let expected: Vec<usize> = cols.split(' ').map(str::parse).collect::<Result<_, _>>()?;
		assert_eq!(linear_sum_assignment(&cost)?, expected, "{line}");
	}
	Ok(())
}

#[test]
fn minimum_spanning_tree_matches_scipy() -> TestResult {
	let text = std::fs::read_to_string(Path::new(FIXTURES).join("mst.txt"))?;
	for line in text.lines().filter(|l| !l.is_empty()) {
		let (graph, edges) = parse_matrix_line(line)?;
		let expected: Vec<(usize, usize)> = edges
			.split(' ')
			.filter(|e| !e.is_empty())
			.map(|e| -> TestResult<(usize, usize)> {
				let (a, b) = e.split_once(',').ok_or("bad edge")?;
				Ok((a.parse()?, b.parse()?))
			})
			.collect::<TestResult<_>>()?;
		assert_eq!(minimum_spanning_tree(&graph), expected, "{line}");
	}
	Ok(())
}

/// Compares the reports of fontoxide and fontTools on other fonts, and times
/// them: `FONTOXIDE_INTERPOLATABLE_FONTS` lists the fonts (separated by `;`),
/// and `FONTOXIDE_INTERPOLATABLE_REFERENCE` is a folder with the text and JSON
/// reports of fontTools (with SciPy) as `<font stem>.txt` and `.json`.
#[test]
#[ignore = "needs fonts and reference reports"]
fn reports_of_other_fonts() -> TestResult {
	let (Ok(fonts), Ok(reference)) = (
		std::env::var("FONTOXIDE_INTERPOLATABLE_FONTS"),
		std::env::var("FONTOXIDE_INTERPOLATABLE_REFERENCE"),
	) else {
		return Ok(());
	};
	for font in fonts.split(';').filter(|f| !f.is_empty()) {
		let stem = Path::new(font)
			.file_stem()
			.map(|s| s.to_string_lossy().into_owned())
			.unwrap_or_default();
		for (ext, args) in [("txt", vec![]), ("json", vec!["--json"])] {
			let expected =
				std::fs::read_to_string(Path::new(&reference).join(format!("{stem}.{ext}")))?;
			let mut all = args.clone();
			all.push(font);
			let mut out = Vec::new();
			let start = std::time::Instant::now();
			run(&all, &mut out)?;
			eprintln!("{stem} {ext}: {:.2?}", start.elapsed());
			assert_eq!(
				String::from_utf8(out)?,
				expected.replace("\r\n", "\n"),
				"{stem}.{ext}"
			);
		}
	}
	Ok(())
}
