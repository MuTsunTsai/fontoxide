//! CFF to CFF2 converter (`fontTools.cffLib.CFFToCFF2`).

use std::collections::BTreeMap;

use super::charstring::{Context, Interpreter, T2CharString, Token, subr_bias};
use super::compile::CompileContext;
use super::dict::{PRIVATE_DICT, TOP_DICT};
use super::font::{
	CffFontSet, DictKind, FontDict, PrivateDict, PrivateRef, TopDict, dict_order, run_char_string,
};
use crate::tables::{Cff, Cff2};
use crate::{Error, Font, ReadOptions, Result, Tag, WriteOptions};

fn list_index_error() -> Error {
	Error::ttlib("IndexError: list index out of range")
}

/// Python's `list[i]`, where a negative index counts from the end.
fn py_index(i: i64, len: usize) -> Result<usize> {
	let at = if i < 0 { i + len as i64 } else { i };
	usize::try_from(at)
		.ok()
		.filter(|&a| a < len)
		.ok_or_else(list_index_error)
}

/// Drops what follows the first `return` or `endchar` of a subroutine.
fn truncate_subr(subr: &mut T2CharString) {
	let program = &mut subr.program;
	let i = program
		.iter()
		.position(|t| matches!(t, Token::Op("return")))
		.unwrap_or(program.len());
	let j = program
		.iter()
		.position(|t| matches!(t, Token::Op("endchar")))
		.unwrap_or(program.len());
	program.truncate(i.min(j));
}

/// The local subroutines of a glyph by its FD index (`thisLocalSubrs`).
fn local_subrs<'t>(
	fd_array: &'t mut Option<Vec<FontDict>>,
	private: &'t mut Option<Box<PrivateDict>>,
	fd_index: Option<i64>,
) -> Result<&'t mut [T2CharString]> {
	let subrs = |p: Option<&'t mut PrivateDict>| -> &'t mut [T2CharString] {
		match p.and_then(|p| p.subrs.as_mut()) {
			Some(s) => s.as_mut_slice(),
			None => &mut [],
		}
	};
	match fd_index {
		None => Ok(subrs(private.as_deref_mut())),
		Some(i) => match fd_array.as_mut().filter(|f| !f.is_empty()) {
			Some(fds) => {
				let at = py_index(i, fds.len())?;
				let p = fds[at].private.as_deref_mut().ok_or_else(|| {
					Error::ttlib("AttributeError: 'FontDict' object has no attribute 'Private'")
				})?;
				Ok(subrs(Some(p)))
			}
			// `[topDict.Private.Subrs]`, or no local subroutines at all.
			None => {
				let has = private.as_ref().is_some_and(|p| p.subrs.is_some());
				if has && py_index(i, 1).is_ok() {
					Ok(subrs(private.as_deref_mut()))
				} else {
					Err(list_index_error())
				}
			}
		},
	}
}

/// Removes the entries of a Private DICT that CFF2 doesn't have.
fn private_to_cff2(private: &mut PrivateDict) {
	let order = dict_order(DictKind::Private, true);
	for op in PRIVATE_DICT {
		if !order.contains(&op.name) {
			private.raw.retain(|(k, _)| *k != op.name);
			private.values.remove(op.name);
		}
	}
	private.cff2 = true;
	private.cff1_order = false;
}

/// `_convertCFFToCFF2`: converts a CFF font set to the CFF2 format in place. The
/// conversion cannot be reversed.
fn convert_font_set(cff: &mut CffFontSet, font: &mut Font) -> Result<()> {
	let global = &mut cff.global_subrs;
	let top: &mut TopDict = cff.top_dicts.first_mut().ok_or_else(list_index_error)?;
	let count = top
		.char_strings
		.as_ref()
		.ok_or_else(|| Error::ttlib("AttributeError: CharStrings"))?
		.items
		.len();

	// Clean up T2CharStrings
	for gid in 0..count {
		run_char_string(top, global, gid, None, None)?;
	}

	// Clean up subroutines first
	for subr in global.iter_mut() {
		truncate_subr(subr);
	}
	match &mut top.fd_array {
		Some(fds) if !fds.is_empty() => {
			for fd in fds.iter_mut() {
				let private = fd.private.as_deref_mut().ok_or_else(|| {
					Error::ttlib("AttributeError: 'FontDict' object has no attribute 'Private'")
				})?;
				for subr in private.subrs.iter_mut().flatten() {
					truncate_subr(subr);
				}
			}
		}
		_ => {
			if let Some(private) = top.private.as_deref_mut() {
				for subr in private.subrs.iter_mut().flatten() {
					truncate_subr(subr);
				}
			}
		}
	}

	// Clean up glyph charstrings
	let mut remove_unused_subrs = false;
	let no_regions = |_: i64| -> Result<i64> {
		Err(Error::ttlib(
			"AttributeError: 'NoneType' object has no attribute 'getNumRegions'",
		))
	};
	for gid in 0..count {
		let TopDict {
			char_strings,
			private,
			fd_array,
			..
		} = &mut *top;
		let char_strings = char_strings
			.as_mut()
			.ok_or_else(|| Error::ttlib("AttributeError: CharStrings"))?;
		let fd_index = char_strings.selector(gid)?;
		let local = local_subrs(fd_array, private, fd_index)?;

		// Intentionally use a nominalWidthX that cannot be added to, such that
		// any CharString that has an explicit width encoded is found.
		let mut cs = std::mem::take(&mut char_strings.items[gid]);
		let ctx = Context {
			local_subrs: &mut *local,
			global_subrs: &mut *global,
			nominal_width_x: None,
			default_width_x: Some(0.0),
			vsindex: None,
			num_regions: &no_regions,
			blender: None,
		};
		let mut extractor = Interpreter::width_detector(ctx);
		let result = extractor.execute(&mut cs);
		let explicit_width = extractor.has_explicit_width();
		char_strings.items[gid] = cs;
		if !explicit_width {
			result?;
		}
		let program = &mut char_strings.items[gid].program;
		if explicit_width {
			// The program has an explicit width. We want to drop it, but can't just
			// pop the first number since it may be a subroutine call. Instead, when
			// seeing that, we embed the subroutine and recurse. If this ever happened,
			// we later prune unused subroutines.
			let local_bias = subr_bias(local.len());
			let global_bias = subr_bias(global.len());
			while program.len() >= 2 && matches!(program[1], Token::Op("callsubr" | "callgsubr")) {
				remove_unused_subrs = true;
				let number = match program.remove(0) {
					Token::Int(n) => n,
					other => return Err(Error::Assertion(format!("{other:?}"))),
				};
				let local_call = matches!(program.remove(0), Token::Op("callsubr"));
				let (bias, set): (i64, &[T2CharString]) = if local_call {
					(local_bias, &*local)
				} else {
					(global_bias, &*global)
				};
				let subr = &set[py_index(number + bias, set.len())?];
				program.splice(0..0, subr.program.iter().cloned());
			}
			// Now pop the actual width
			if program.is_empty() {
				return Err(Error::Assertion(format!("{program:?}")));
			}
			program.remove(0);
		}
		if matches!(program.last(), Some(Token::Op("endchar"))) {
			program.pop();
		}
	}

	if remove_unused_subrs {
		cff.remove_unused_subroutines()?;
	}

	// Upconvert TopDict
	cff.major = 2;
	let top: &mut TopDict = cff.top_dicts.first_mut().ok_or_else(list_index_error)?;
	top.cff2 = true;
	match &mut top.fd_array {
		None => {
			let mut private = top.private.take();
			if let Some(p) = private.as_deref_mut() {
				private_to_cff2(p);
			}
			top.fd_array = Some(vec![FontDict {
				cff2: true,
				raw: Vec::new(),
				values: BTreeMap::new(),
				private,
			}]);
			if let Some(cs) = &mut top.char_strings {
				cs.has_fd_array = true;
				cs.privates = vec![PrivateRef::Fd(0); cs.items.len()];
			}
		}
		Some(fds) => {
			// Clean up the PrivateDicts in the fdArray
			let fd_order = dict_order(DictKind::Font, true);
			for fd in fds.iter_mut() {
				fd.cff2 = true;
				fd.raw.retain(|(k, _)| fd_order.contains(k));
				fd.values.retain(|k, _| fd_order.contains(k));
				let private = fd.private.as_deref_mut().ok_or_else(|| {
					Error::ttlib("AttributeError: 'FontDict' object has no attribute 'Private'")
				})?;
				private_to_cff2(private);
			}
		}
	}

	// Now delete up the deprecated topDict operators from CFF 1.0. The charset is
	// kept, as fontTools does; it is not written in CFF2.
	let order = dict_order(DictKind::Top, true);
	for op in TOP_DICT {
		if op.name == "charset" || order.contains(&op.name) {
			continue;
		}
		top.raw.retain(|(k, _)| *k != op.name);
		top.values.remove(op.name);
		match op.name {
			"Encoding" => top.encoding = None,
			"Private" => top.private = None,
			_ => {}
		}
	}

	// The charstrings and the subroutines are still those of CFF: compile, then
	// decompile again.
	let glyph_order = font.glyph_order()?.names().to_vec();
	let ctx = CompileContext {
		glyph_order: &glyph_order,
		recalc_bboxes: font.recalc_bboxes,
		has_varc: font.contains(Tag::new(b"VARC")),
	};
	let data = cff.compile(true, &ctx)?;
	*cff = CffFontSet::decompile(&data, true, Some(&glyph_order))?;
	Ok(())
}

/// `convertCFFToCFF2`: converts the `CFF ` table of a font to a `CFF2` table.
pub fn convert_cff_to_cff2(font: &mut Font) -> Result<()> {
	let mut cff = std::mem::take(&mut font.require::<Cff>()?.cff);
	// UPSTREAM-BUG(B51): the CFF table is removed before the glyph order of a freshly
	// read font is known, so it is then derived from `post` or `cmap`, and compiling
	// the CFF2 table fails when those names differ from the CFF charset.
	font.remove(Tag::new(b"CFF "));
	convert_font_set(&mut cff, font)?;
	font.insert(Box::new(Cff2 { cff }));
	Ok(())
}

const HELP: &str = "\
usage: fonttools cffLib.CFFToCFF2 [-h] [-o OUTPUT.ttf] [--no-recalc-timestamp]
                                  [-v | -q]
                                  INPUT.ttf

Upgrade a CFF font to CFF2.

positional arguments:
  INPUT.ttf             Input OTF file with CFF table.

options:
  -h, --help            show this help message and exit
  -o, --output OUTPUT.ttf
                        Output instance OTF file (default: INPUT-CFF2.ttf).
  --no-recalc-timestamp
                        Don't set the output font's timestamp to the current
                        time.
  -v, --verbose         Run more verbosely.
  -q, --quiet           Turn verbosity off.
";

/// The help text of the command line.
pub fn help() -> &'static str {
	HELP
}

/// `main`: converts a CFF OTF font file to a CFF2 one; returns the exit code.
/// `-v` and `-q` are accepted; logging is up to the `log` crate.
pub fn main(args: &[&str]) -> Result<i32> {
	let usage = |message: String| Error::value(format!("error: {message}"));
	let mut input = None;
	let mut output = None;
	let mut recalc_timestamp = true;
	let mut verbosity: Option<&str> = None;
	let mut it = args.iter();
	while let Some(&arg) = it.next() {
		match arg {
			"-h" | "--help" => {
				print!(
					"{}",
					fontoxide_misc::cli_tools::program_text(
						HELP,
						"fonttools cffLib.CFFToCFF2",
						"cffLib.CFFToCFF2"
					)
				);
				return Ok(0);
			}
			"-o" | "--output" => {
				output = Some(
					*it.next()
						.ok_or_else(|| usage(format!("argument {arg}: expected one argument")))?,
				);
			}
			"--no-recalc-timestamp" => recalc_timestamp = false,
			"-v" | "--verbose" | "-q" | "--quiet" => {
				let name = |a: &str| match a {
					"-v" | "--verbose" => "-v/--verbose",
					_ => "-q/--quiet",
				};
				if let Some(other) = verbosity
					&& name(other) != name(arg)
				{
					return Err(usage(format!(
						"argument {}: not allowed with argument {}",
						name(arg),
						name(other)
					)));
				}
				verbosity = Some(arg);
			}
			_ if arg.starts_with("--output=") => output = Some(&arg["--output=".len()..]),
			_ if arg.len() > 1 && arg.starts_with('-') => {
				return Err(usage(format!("unrecognized arguments: {arg}")));
			}
			_ if input.is_none() => input = Some(arg),
			_ => return Err(usage(format!("unrecognized arguments: {arg}"))),
		}
	}
	let input =
		input.ok_or_else(|| usage("the following arguments are required: INPUT.ttf".to_owned()))?;
	if !std::path::Path::new(input).is_file() {
		return Err(usage(format!("No such file '{input}'")));
	}
	let output = match output {
		Some(output) => output.to_owned(),
		None => fontoxide_misc::cli_tools::make_output_file_name(input, None, None, true, "-CFF2"),
	};
	let mut font = Font::read(
		std::fs::read(input)?,
		ReadOptions {
			recalc_timestamp,
			recalc_bboxes: false,
			..Default::default()
		},
	)?;
	convert_cff_to_cff2(&mut font)?;
	log::info!("Saving {output}");
	std::fs::write(output, font.write(&WriteOptions::default())?)?;
	Ok(0)
}
