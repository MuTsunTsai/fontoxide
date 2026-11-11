//! `Tests/cu2qu/cli_test.py`, and the files the command line writes, checked
//! byte for byte against fontTools (with ufoLib2) through the digests of
//! `data/cli_outputs.txt` (`tools/reference/cu2qu_ufo_fixtures.py`).

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::{copy_tree, test_ufos, tmp_dir};
use fontoxide_cu2qu::cli::{Error, main};
use fontoxide_cu2qu::ufo::CURVE_TYPE_LIB_KEY;
use fontoxide_ufolib::Value;
use fontoxide_ufolib::objects::Font;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The `test_paths` fixture: copies of the test UFOs.
fn test_paths(name: &str) -> Result<(PathBuf, Vec<PathBuf>)> {
	let tmp = tmp_dir(name);
	let mut out = Vec::new();
	for path in test_ufos() {
		let new_path = tmp.join(path.file_name().ok_or("no file name")?);
		copy_tree(&path, &new_path)?;
		out.push(new_path);
	}
	Ok((tmp, out))
}

/// `run_main`: empty arguments are dropped.
fn run_main(args: &[&str]) -> std::result::Result<i32, Error> {
	let args: Vec<&str> = args.iter().copied().filter(|a| !a.is_empty()).collect();
	main(&args)
}

fn s(path: &Path) -> &str {
	path.to_str().unwrap_or_default()
}

#[test]
fn test_single_input_no_output() -> Result {
	let (_tmp, paths) = test_paths("single_input_no_output")?;
	let ufo_path = &paths[0];
	run_main(&[s(ufo_path)])?;
	let font = Font::open(ufo_path, true)?;
	assert_eq!(
		font.lib.get(CURVE_TYPE_LIB_KEY),
		Some(&Value::String("quadratic".into()))
	);
	Ok(())
}

#[test]
fn test_single_input_output_file() -> Result {
	let tmp = tmp_dir("single_input_output_file");
	let input_path = &test_ufos()[0];
	let output_path = tmp.join(input_path.file_name().ok_or("no file name")?);
	run_main(&["-o", s(&output_path), s(input_path)])?;
	assert!(output_path.is_dir());
	Ok(())
}

#[test]
fn test_multiple_inputs_output_dir() -> Result {
	let tmp = tmp_dir("multiple_inputs_output_dir");
	let output_dir = tmp.join("output_dir");
	let [a, b] = test_ufos();
	run_main(&["-d", s(&output_dir), s(&a), s(&b)])?;
	assert!(output_dir.is_dir());
	let outputs: Vec<String> = std::fs::read_dir(&output_dir)?
		.map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
		.collect::<std::io::Result<_>>()?;
	assert!(outputs.contains(&"RobotoSubset-Regular.ufo".to_owned()));
	assert!(outputs.contains(&"RobotoSubset-Bold.ufo".to_owned()));
	Ok(())
}

#[test]
fn test_interpolatable_inplace() -> Result {
	let (_tmp, paths) = test_paths("interpolatable_inplace")?;
	run_main(&["-i", s(&paths[0]), s(&paths[1])])?;
	run_main(&["-i", s(&paths[0]), s(&paths[1])])?; // idempotent
	Ok(())
}

#[test]
fn test_copytree() -> Result {
	// "normal" and "interpolatable"
	for mode in ["", "-i"] {
		let tmp = tmp_dir("copytree");
		let output_dir = tmp.join("output_dir");
		let [a, b] = test_ufos();
		run_main(&[mode, "-d", s(&output_dir), s(&a), s(&b)])?;

		let output_dir_2 = tmp.join("output_dir_2");
		let converted: Vec<PathBuf> = std::fs::read_dir(&output_dir)?
			.map(|e| e.map(|e| e.path()))
			.collect::<std::io::Result<_>>()?;
		let mut args = vec![mode, "-d", s(&output_dir_2)];
		args.extend(converted.iter().map(|p| s(p)));
		// no conversion when curves are already quadratic, just copy
		run_main(&args)?;
		// running again overwrites existing with the copy
		run_main(&args)?;
		for p in &converted {
			let copy = output_dir_2.join(p.file_name().ok_or("no file name")?);
			assert_eq!(
				std::fs::read(p.join("lib.plist"))?,
				std::fs::read(copy.join("lib.plist"))?
			);
		}
	}
	Ok(())
}

#[test]
fn test_multiprocessing() -> Result {
	let (_tmp, paths) = test_paths("multiprocessing")?;
	run_main(&[s(&paths[0]), s(&paths[1]), "-j"])?;
	Ok(())
}

#[test]
fn test_keep_direction() -> Result {
	let (_tmp, paths) = test_paths("keep_direction")?;
	run_main(&["--keep-direction", s(&paths[0]), s(&paths[1])])?;
	Ok(())
}

#[test]
fn test_conversion_error() -> Result {
	let (_tmp, paths) = test_paths("conversion_error")?;
	run_main(&["--conversion-error", "0.002", s(&paths[0]), s(&paths[1])])?;
	Ok(())
}

#[test]
fn test_conversion_error_short() -> Result {
	let (_tmp, paths) = test_paths("conversion_error_short")?;
	run_main(&["-e", "0.003", s(&paths[0])])?;
	Ok(())
}

// Beyond the upstream tests.

fn usage_error(args: &[&str], message: &str) {
	match run_main(args) {
		Err(Error::Usage(m)) => assert!(
			m.starts_with("usage: cu2qu ") && m.ends_with(&format!("cu2qu: error: {message}")),
			"{m}"
		),
		other => panic!("expected a usage error, got {other:?}"),
	}
}

#[test]
fn test_usage_errors() {
	usage_error(&[], "the following arguments are required: INPUT");
	usage_error(
		&["-i", "-j", "2", "a.ufo"],
		"argument -j/--jobs: not allowed with argument -i/--interpolatable",
	);
	usage_error(
		&["-o", "x", "-d", "y", "a.ufo"],
		"argument -d/--output-dir: not allowed with argument -o/--output-file",
	);
	usage_error(
		&["-o", "x", "a.ufo", "b.ufo"],
		"-o/--output-file can't be used with multile inputs",
	);
	usage_error(
		&["-e", "abc", "a.ufo"],
		"argument -e/--conversion-error: invalid float value: 'abc'",
	);
	usage_error(&["--bogus", "a.ufo"], "unrecognized arguments: --bogus");
	usage_error(
		&["-e"],
		"argument -e/--conversion-error: expected one argument",
	);
	usage_error(
		&["-j", "a.ufo"],
		"argument -j/--jobs: invalid int value: 'a.ufo'",
	);
}

#[test]
fn test_help_and_version() -> Result {
	assert_eq!(run_main(&["-h"])?, 0);
	assert_eq!(run_main(&["--vers"])?, 0);
	Ok(())
}

/// 64-bit FNV-1a.
fn fnv1a64(data: &[u8]) -> u64 {
	let mut h: u64 = 0xcbf2_9ce4_8422_2325;
	for &b in data {
		h ^= u64::from(b);
		h = h.wrapping_mul(0x0000_0100_0000_01b3);
	}
	h
}

fn digests(dir: &Path, base: &Path, out: &mut BTreeMap<String, (usize, String)>) -> Result {
	for entry in std::fs::read_dir(dir)? {
		let path = entry?.path();
		if path.is_dir() {
			digests(&path, base, out)?;
		} else {
			let data = std::fs::read(&path)?;
			let rel = path
				.strip_prefix(base)?
				.components()
				.map(|c| c.as_os_str().to_string_lossy().into_owned())
				.collect::<Vec<_>>()
				.join("/");
			out.insert(rel, (data.len(), format!("{:016x}", fnv1a64(&data))));
		}
	}
	Ok(())
}

/// The command line writes the same files as fontTools in each mode of
/// `tools/reference/cu2qu_ufo_fixtures.py`.
#[test]
fn test_outputs_match_fonttools() -> Result {
	let mut expected: BTreeMap<&str, BTreeMap<String, (usize, String)>> = BTreeMap::new();
	for line in include_str!("data/cli_outputs.txt").lines() {
		let f: Vec<&str> = line.split('\t').collect();
		expected
			.entry(f[0])
			.or_default()
			.insert(f[1].to_owned(), (f[2].parse()?, f[3].to_owned()));
	}
	let modes: [(&str, &[&str], bool); 6] = [
		("default", &[], true),
		("interpolatable", &["-i"], true),
		(
			"mixed",
			&["-i", "-m", "--keep-direction", "-e", "0.002"],
			true,
		),
		("jobs", &["-j", "2", "-e", "0.003"], true),
		("inplace", &[], false),
		("inplace-interpolatable", &["-i", "--keep-direction"], false),
	];
	for (mode, args, to_dir) in modes {
		let (tmp, inputs) = test_paths(mode)?;
		let out = tmp.join("out");
		let mut all: Vec<&str> = args.to_vec();
		if to_dir {
			all.extend(["-d", s(&out)]);
		}
		all.extend(inputs.iter().map(|p| s(p)));
		run_main(&all)?;
		let mut got = BTreeMap::new();
		digests(
			if to_dir { &out } else { &tmp },
			if to_dir { &out } else { &tmp },
			&mut got,
		)?;
		let want = &expected[mode];
		let differ: Vec<&String> = want
			.keys()
			.chain(got.keys())
			.filter(|k| want.get(*k) != got.get(*k))
			.collect();
		assert!(differ.is_empty(), "{mode}: {differ:?}");
	}
	Ok(())
}
