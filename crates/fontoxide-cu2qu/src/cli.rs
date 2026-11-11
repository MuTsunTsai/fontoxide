//! The `cu2qu` command line (`fontTools.cu2qu.cli`): converts UFO fonts
//! from cubic to quadratic curves.

use std::path::{Path, PathBuf};

use fontoxide_ufolib::UfoLibError;
use fontoxide_ufolib::objects::Font;

use crate::argparse::{self, Kind, Spec, Ty};
use crate::ufo::{self, MaxErr, Options, font_to_quadratic, fonts_to_quadratic};

/// The version of fontTools this is a port of (`fontTools.__version__`).
const VERSION: &str = "4.68.0";

/// The usage message.
pub const USAGE: &str = "\
usage: cu2qu [-h] [--version] [-v] [-e ERROR] [-m] [--keep-direction] [-i |
             -j [N]] [-o OUTPUT | -d DIRECTORY]
             INPUT [INPUT ...]";

/// The help message (`-h`).
pub const HELP: &str = "\
usage: cu2qu [-h] [--version] [-v] [-e ERROR] [-m] [--keep-direction] [-i |
             -j [N]] [-o OUTPUT | -d DIRECTORY]
             INPUT [INPUT ...]

positional arguments:
  INPUT                 one or more input UFO source file(s).

options:
  -h, --help            show this help message and exit
  --version             show program's version number and exit
  -v, --verbose
  -e, --conversion-error ERROR
                        maximum approximation error measured in EM (default:
                        0.001)
  -m, --mixed           whether to used mixed quadratic and cubic curves
  --keep-direction      do not reverse the contour direction
  -i, --interpolatable  whether curve conversion should keep interpolation
                        compatibility
  -j, --jobs [N]        convert using N multiple processes (default: 1)
  -o, --output-file OUTPUT
                        output filename for the converted UFO. By default
                        fonts are modified in place. This only works with a
                        single input.
  -d, --output-dir DIRECTORY
                        output directory where to save converted UFOs
";

const SPECS: &[Spec] = &[
	Spec {
		short: Some("-h"),
		long: "--help",
		kind: Kind::Flag,
		group: None,
		ty: Ty::Str,
	},
	Spec {
		short: None,
		long: "--version",
		kind: Kind::Flag,
		group: None,
		ty: Ty::Str,
	},
	Spec {
		short: Some("-v"),
		long: "--verbose",
		kind: Kind::Count,
		group: None,
		ty: Ty::Str,
	},
	Spec {
		short: Some("-e"),
		long: "--conversion-error",
		kind: Kind::Value,
		group: None,
		ty: Ty::Float,
	},
	Spec {
		short: Some("-m"),
		long: "--mixed",
		kind: Kind::Flag,
		group: None,
		ty: Ty::Str,
	},
	Spec {
		short: None,
		long: "--keep-direction",
		kind: Kind::Flag,
		group: None,
		ty: Ty::Str,
	},
	Spec {
		short: Some("-i"),
		long: "--interpolatable",
		kind: Kind::Flag,
		group: Some(0),
		ty: Ty::Str,
	},
	Spec {
		short: Some("-j"),
		long: "--jobs",
		kind: Kind::OptionalValue,
		group: Some(0),
		ty: Ty::Int,
	},
	Spec {
		short: Some("-o"),
		long: "--output-file",
		kind: Kind::Value,
		group: Some(1),
		ty: Ty::Str,
	},
	Spec {
		short: Some("-d"),
		long: "--output-dir",
		kind: Kind::Value,
		group: Some(1),
		ty: Ty::Str,
	},
];

/// An error of the command line.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// `parser.error`: the usage and the message, for exit status 2.
	#[error("{0}")]
	Usage(String),
	/// An error converting the fonts.
	#[error(transparent)]
	Convert(#[from] ufo::Error),
	/// An error reading or writing a UFO.
	#[error(transparent)]
	Ufo(#[from] UfoLibError),
	/// An operating system error.
	#[error(transparent)]
	Io(#[from] std::io::Error),
}

fn usage(message: impl std::fmt::Display) -> Error {
	Error::Usage(format!("{USAGE}\ncu2qu: error: {message}"))
}

/// `_cpu_count`.
fn cpu_count() -> usize {
	std::thread::available_parallelism().map_or(1, usize::from)
}

/// `os.path.basename`.
fn basename(path: &str) -> &str {
	let cut = if cfg!(windows) {
		path.rfind(['/', '\\'])
	} else {
		path.rfind('/')
	};
	cut.map_or(path, |i| &path[i + 1..])
}

/// `_samepath`.
fn samepath(a: &Path, b: &Path) -> bool {
	let norm = |p: &Path| -> String {
		let p = std::fs::canonicalize(p)
			.or_else(|_| std::path::absolute(p))
			.unwrap_or_else(|_| p.to_owned());
		let s = p.to_string_lossy().into_owned();
		if cfg!(windows) { s.to_lowercase() } else { s }
	};
	norm(a) == norm(b)
}

/// `shutil.copytree`.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
	if from.is_dir() {
		std::fs::create_dir_all(to)?;
		for entry in std::fs::read_dir(from)? {
			let entry = entry?;
			copy_tree(&entry.path(), &to.join(entry.file_name()))?;
		}
		Ok(())
	} else {
		std::fs::copy(from, to).map(|_| ())
	}
}

/// `_copytree`: replaces `output_path` with a copy of `input_path`.
fn copytree(input_path: &Path, output_path: &Path) -> std::io::Result<()> {
	if samepath(input_path, output_path) {
		log::debug!("input and output paths are the same file; skipped copy");
		return Ok(());
	}
	if output_path.exists() {
		std::fs::remove_dir_all(output_path)?;
	}
	copy_tree(input_path, output_path)
}

fn path_repr(path: Option<&Path>) -> String {
	path.map_or_else(|| "None".to_owned(), |p| p.display().to_string())
}

/// `_font_to_quadratic`: converts a UFO, saving it to `output_path` or in
/// place, or copying it to `output_path` if nothing changed.
fn convert_font(
	input_path: &str,
	output_path: Option<&Path>,
	options: &Options,
) -> Result<(), Error> {
	let mut ufo = Font::open(input_path, true)?;
	log::info!("Converting curves for {input_path}");
	if !font_to_quadratic(&mut ufo, options, None)?.is_empty() {
		log::info!("Saving {}", path_repr(output_path));
		match output_path {
			Some(path) => ufo.save_as(path, false)?,
			None => ufo.save()?, // save in-place
		}
	} else if let Some(output_path) = output_path {
		copytree(Path::new(input_path), output_path)?;
	}
	Ok(())
}

/// `_main`: converts UFO fonts from cubic to quadratic curves, with the
/// arguments of the `cu2qu` command line. Returns the exit status; a usage
/// error is [`Error::Usage`] (exit status 2 in fontTools).
///
/// The `-v` option is accepted, but logging is up to the caller's `log`
/// implementation. `-j` converts the fonts in threads rather than processes.
pub fn main(args: &[&str]) -> Result<i32, Error> {
	let parsed = argparse::parse(args, SPECS, "INPUT").map_err(usage)?;
	if parsed.flag("--help") {
		print!("{HELP}");
		return Ok(0);
	}
	if parsed.flag("--version") {
		println!("{VERSION}");
		return Ok(0);
	}
	let conversion_error = match parsed.value("--conversion-error") {
		Some(seen) => Some(
			argparse::parse_float(
				"-e/--conversion-error",
				seen.value.as_deref().unwrap_or_default(),
			)
			.map_err(usage)?,
		),
		None => None,
	};
	let jobs = match parsed.value("--jobs") {
		Some(seen) => match &seen.value {
			Some(v) => argparse::parse_int("-j/--jobs", v).map_err(usage)?,
			None => cpu_count() as i128,
		},
		None => 1,
	};
	let infiles = &parsed.positional;
	let output_file = parsed.value("--output-file").and_then(|s| s.value.clone());
	let output_dir = parsed.value("--output-dir").and_then(|s| s.value.clone());

	if infiles.len() > 1 && output_file.is_some() {
		return Err(usage("-o/--output-file can't be used with multile inputs"));
	}

	let output_paths: Vec<Option<PathBuf>> =
		if let Some(output_dir) = output_dir.filter(|d| !d.is_empty()) {
			let dir = Path::new(&output_dir);
			if !dir.exists() {
				std::fs::create_dir(dir)?;
			} else if !dir.is_dir() {
				return Err(usage(format!("'{output_dir}' is not a directory")));
			}
			infiles
				.iter()
				.map(|p| Some(dir.join(basename(p))))
				.collect()
		} else if let Some(output_file) = output_file.filter(|f| !f.is_empty()) {
			vec![Some(PathBuf::from(output_file))]
		} else {
			// save in-place
			vec![None; infiles.len()]
		};

	let options = Options {
		dump_stats: parsed.count("--verbose") > 0,
		max_err_em: conversion_error.map(MaxErr::Value),
		reverse_direction: !parsed.flag("--keep-direction"),
		all_quadratic: !parsed.flag("--mixed"),
		..Options::default()
	};

	if parsed.flag("--interpolatable") {
		log::info!("Converting curves compatibly");
		let mut ufos = infiles
			.iter()
			.map(|p| Font::open(p, true))
			.collect::<Result<Vec<_>, _>>()?;
		if !fonts_to_quadratic(&mut ufos, &options, None)?.is_empty() {
			for (ufo, output_path) in ufos.iter_mut().zip(&output_paths) {
				log::info!("Saving {}", path_repr(output_path.as_deref()));
				match output_path {
					Some(path) => ufo.save_as(path, false)?,
					None => ufo.save()?,
				}
			}
		} else {
			for (input_path, output_path) in infiles.iter().zip(&output_paths) {
				if let Some(output_path) = output_path {
					copytree(Path::new(input_path), output_path)?;
				}
			}
		}
	} else {
		let jobs = if jobs > 1 {
			(infiles.len() as i128).min(jobs) as usize
		} else {
			1
		};
		// `zip` stops at the shorter list: with `-o`, only the first input.
		let tasks: Vec<(&String, Option<&Path>)> = infiles
			.iter()
			.zip(&output_paths)
			.map(|(i, o)| (i, o.as_deref()))
			.collect();
		if jobs > 1 {
			log::info!("Running {jobs} parallel processes");
			run_parallel(&tasks, jobs, &options)?;
		} else {
			for (input_path, output_path) in tasks {
				convert_font(input_path, output_path, &options)?;
			}
		}
	}
	Ok(0)
}

/// `pool.starmap(_font_to_quadratic, ...)` with `jobs` threads; the error of
/// the first failing input is returned.
fn run_parallel(
	tasks: &[(&String, Option<&Path>)],
	jobs: usize,
	options: &Options,
) -> Result<(), Error> {
	use std::sync::Mutex;
	use std::sync::atomic::{AtomicUsize, Ordering};
	let next = AtomicUsize::new(0);
	let errors: Mutex<Vec<(usize, Error)>> = Mutex::new(Vec::new());
	std::thread::scope(|scope| {
		for _ in 0..jobs {
			scope.spawn(|| {
				loop {
					let i = next.fetch_add(1, Ordering::Relaxed);
					let Some(&(input_path, output_path)) = tasks.get(i) else {
						break;
					};
					if let Err(e) = convert_font(input_path, output_path, options) {
						errors
							.lock()
							.unwrap_or_else(|e| e.into_inner())
							.push((i, e));
					}
				}
			});
		}
	});
	let mut errors = errors.into_inner().unwrap_or_else(|e| e.into_inner());
	errors.sort_by_key(|(i, _)| *i);
	match errors.into_iter().next() {
		Some((_, e)) => Err(e),
		None => Ok(()),
	}
}
