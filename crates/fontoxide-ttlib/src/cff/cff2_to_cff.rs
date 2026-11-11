//! CFF2 to CFF converter (`fontTools.cffLib.CFF2ToCFF`).

use std::io::Write;

use fontoxide_misc::argparse::{Argument, Command};
use fontoxide_misc::cli_tools::make_output_file_name;

use super::charstring::{T2CharString, Token, subr_bias};
use super::dict::{Conv, Value, operator};
use super::font::{
	CffFontSet, DictKind, FdSelect, PrivateDict, PrivateRef, dict_order, dict_table,
};
use super::specializer::{SpecializeOptions, specialize_program};
use super::transforms::private_of;
use super::width::optimize_widths;
use crate::remove_overlaps::{RemoveOverlapsOptions, remove_overlaps};
use crate::tables::{Cff, Cff2, Hmtx, Post};
use crate::{Error, Font, ReadOptions, Result, Tag, WriteOptions};

/// `dictionary changed size during iteration`.
///
/// UPSTREAM-BUG(B25): keys that aren't part of CFF are deleted from the DICT while
/// iterating over it. This raises at the next step of the iteration.
fn dict_changed_size() -> Error {
	Error::ttlib("RuntimeError: dictionary changed size during iteration")
}

/// `T2StackUseExtractor`.
///
/// The stack use of the subroutines themselves isn't counted (only what they leave on
/// the stack), as in fontTools: each call of `execute` has its own maximum.
struct StackUse<'a> {
	local: &'a [T2CharString],
	local_bias: i64,
	global: &'a [T2CharString],
	global_bias: i64,
	stack: Vec<Token>,
	hint_count: usize,
	hint_mask_bytes: usize,
	depth: usize,
}

impl<'a> StackUse<'a> {
	fn new(local: &'a [T2CharString], global: &'a [T2CharString]) -> Self {
		StackUse {
			local,
			local_bias: subr_bias(local.len()),
			global,
			global_bias: subr_bias(global.len()),
			stack: Vec::new(),
			hint_count: 0,
			hint_mask_bytes: 0,
			depth: 0,
		}
	}

	fn call(&mut self, subrs: &'a [T2CharString], bias: i64) -> Result<()> {
		let index = match self.stack.pop() {
			Some(Token::Int(i)) => i + bias,
			Some(_) => return Err(Error::ttlib("TypeError: list indices must be integers")),
			None => return Err(Error::ttlib("IndexError: list index out of range")),
		};
		let at = if index < 0 {
			index + subrs.len() as i64
		} else {
			index
		};
		let subr = usize::try_from(at)
			.ok()
			.and_then(|i| subrs.get(i))
			.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
		self.execute(subr)?;
		Ok(())
	}

	fn execute(&mut self, cs: &'a T2CharString) -> Result<usize> {
		self.depth += 1;
		if self.depth > 900 {
			return Err(Error::ttlib(
				"RecursionError: maximum recursion depth exceeded",
			));
		}
		let mut max_stack_use = 0;
		let mut index = 0;
		while let Some((token, next)) = cs.token(index)? {
			index = next;
			let Token::Op(op) = token else {
				self.stack.push(token);
				max_stack_use = max_stack_use.max(self.stack.len());
				continue;
			};
			match op {
				"return" | "endchar" | "ignore" => {}
				"callsubr" => self.call(self.local, self.local_bias)?,
				"callgsubr" => self.call(self.global, self.global_bias)?,
				"hstem" | "vstem" | "hstemhm" | "vstemhm" => {
					self.hint_count += self.stack.len() / 2;
					self.stack.clear();
				}
				"hintmask" | "cntrmask" => {
					if self.hint_mask_bytes == 0 {
						self.hint_count += self.stack.len() / 2;
						self.stack.clear();
						self.hint_mask_bytes = self.hint_count.div_ceil(8);
					}
					index = cs.bytes(index, self.hint_mask_bytes)?.1;
				}
				"and" | "or" | "not" | "store" | "abs" | "add" | "sub" | "div" | "load" | "neg"
				| "eq" | "drop" | "put" | "get" | "ifelse" | "random" | "mul" | "sqrt" | "dup"
				| "exch" | "index" | "roll" => {
					return Err(Error::ttlib("NotImplementedError"));
				}
				"blend" | "vsindex" => {
					// There is no variation store to convert a CFF2 font with.
					return Err(Error::ttlib(
						"AttributeError: 'PrivateDict' object has no attribute 'vstore'",
					));
				}
				_ => self.stack.clear(),
			}
		}
		self.depth -= 1;
		Ok(max_stack_use)
	}
}

/// Switches a Private DICT to the operators of CFF. It stays a CFF2 one otherwise: its
/// defaults, and how its charstrings are desubroutinized.
fn private_to_cff(private: &mut PrivateDict) -> Result<()> {
	let order = dict_order(DictKind::Private, false);
	let table = dict_table(DictKind::Private);
	for &key in &order {
		if private.raw.iter().any(|(k, _)| *k == key) {
			continue;
		}
		if let Some(default) = operator(table, key).and_then(|o| o.default.map(|f| f())) {
			private.raw.push((key, default));
		}
	}
	if private.raw.iter().any(|(k, _)| !order.contains(k)) {
		return Err(dict_changed_size());
	}
	private.cff1_order = true;
	for &key in &order {
		// These two are set to None in CFF2 until the widths are known.
		if matches!(key, "defaultWidthX" | "nominalWidthX") || private.values.contains_key(key) {
			continue;
		}
		let is_plain = operator(table, key).is_some_and(|o| {
			matches!(
				o.conv,
				Conv::Number | Conv::Array | Conv::Ascii | Conv::Latin1 | Conv::Ros
			)
		});
		if is_plain && let Some((_, v)) = private.raw.iter().find(|(k, _)| *k == key) {
			private.values.insert(key, v.clone());
		}
	}
	Ok(())
}

/// `_convertCFF2ToCFF`: converts a CFF2 font set to the CFF format in place. The font
/// cannot be variable.
fn convert_font_set(cff: &mut CffFontSet, widths: &[i64]) -> Result<()> {
	cff.major = 1;
	let global: &mut Vec<T2CharString> = &mut cff.global_subrs;
	let top = cff
		.top_dicts
		.first_mut()
		.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?;
	if top.var_store.is_some() {
		return Err(Error::value(
			"Variable CFF2 font cannot be converted to CFF format.",
		));
	}

	let op_order = dict_order(DictKind::Top, false);
	if top.raw.iter().any(|(k, _)| !op_order.contains(k)) {
		return Err(dict_changed_size());
	}
	top.cff2 = false;
	let table = dict_table(DictKind::Top);
	for &key in &op_order {
		let is_plain = operator(table, key).is_some_and(|o| {
			matches!(
				o.conv,
				Conv::Number | Conv::Array | Conv::Ascii | Conv::Latin1 | Conv::Ros
			)
		});
		if is_plain
			&& !top.values.contains_key(key)
			&& let Some((_, v)) = top.raw.iter().find(|(k, _)| *k == key)
		{
			top.values.insert(key, v.clone());
		}
	}

	let count = top.char_strings.as_ref().map_or(0, |c| c.items.len());
	let fd_count = top
		.fd_array
		.as_ref()
		.ok_or_else(|| Error::ttlib("AttributeError: FDArray"))?
		.len();
	if top.fd_select.is_none() {
		// FDSelect is optional in CFF2, but required in CFF.
		top.fd_select = Some(FdSelect {
			format: None,
			gid_array: vec![Some(0); count],
			num_char_strings: None,
		});
	}

	// Load the charstrings and the subroutines before switching the FontDicts to CFF.
	for gid in 0..count {
		super::transforms::decompile(top, global, gid)?;
	}

	let fd_order = dict_order(DictKind::Font, false);
	let fds = top
		.fd_array
		.as_mut()
		.ok_or_else(|| Error::ttlib("AttributeError: FDArray"))?;
	for fd in fds.iter_mut() {
		let private = fd.private.as_deref_mut().ok_or_else(|| {
			Error::ttlib("AttributeError: 'FontDict' object has no attribute 'Private'")
		})?;
		private_to_cff(private)?;
		fd.cff2 = false;
		for &key in &fd_order {
			if key != "Private"
				&& !fd.values.contains_key(key)
				&& let Some((_, v)) = fd.raw.iter().find(|(k, _)| *k == key)
			{
				fd.values.insert(key, v.clone());
			}
		}
	}

	// Add ending operators.
	if let Some(char_strings) = &mut top.char_strings {
		for cs in &mut char_strings.items {
			cs.program.push(Token::Op("endchar"));
		}
	}
	for cs in global.iter_mut() {
		cs.program.push(Token::Op("return"));
	}
	for fd in top.fd_array.iter_mut().flatten() {
		let subrs = fd.private.as_deref_mut().and_then(|p| p.subrs.as_mut());
		for cs in subrs.into_iter().flatten() {
			cs.program.push(Token::Op("return"));
		}
	}

	// Add (optimal) width to CharStrings that need it.
	let selector = |top: &super::font::TopDict, gid: usize| -> Result<usize> {
		let index = top
			.char_strings
			.as_ref()
			.ok_or_else(|| Error::ttlib("AttributeError: CharStrings"))?
			.selector(gid)?;
		Ok(index.unwrap_or(0) as usize)
	};
	let mut by_fd: Vec<(usize, Vec<i64>)> = Vec::new();
	for gid in 0..count {
		let fd = selector(top, gid)?;
		let width = *widths
			.get(gid)
			.ok_or_else(|| Error::ttlib(format!("KeyError: {gid}")))?;
		match by_fd.iter_mut().find(|e| e.0 == fd) {
			Some(e) => e.1.push(width),
			None => by_fd.push((fd, vec![width])),
		}
	}
	let fd_error = || Error::ttlib("IndexError: list index out of range");
	for (fd, list) in &by_fd {
		let (default, nominal) = optimize_widths(list);
		let private = top
			.fd_array
			.as_mut()
			.and_then(|f| f.get_mut(*fd))
			.and_then(|f| f.private.as_deref_mut())
			.ok_or_else(fd_error)?;
		private.values.insert("defaultWidthX", Value::Int(default));
		private.values.insert("nominalWidthX", Value::Int(nominal));
	}
	#[allow(clippy::needless_range_loop)]
	for gid in 0..count {
		let fd = selector(top, gid)?;
		let private = private_of(top, PrivateRef::Fd(fd)).ok_or_else(fd_error)?;
		let number = |name: &str| private.get(name).and_then(|v| v.int().ok()).unwrap_or(0);
		let (default, nominal) = (number("defaultWidthX"), number("nominalWidthX"));
		let width = widths[gid];
		if width != default
			&& let Some(char_strings) = &mut top.char_strings
		{
			char_strings.items[gid]
				.program
				.insert(0, Token::Int(width - nominal));
		}
	}

	// Handle stack use since stack-depth is lower in CFF than in CFF2.
	for gid in 0..count {
		let fd = selector(top, gid)?;
		let private_ref = PrivateRef::Fd(fd);
		let private = private_of(top, private_ref).ok_or_else(fd_error)?;
		let local = private.subrs.as_deref().unwrap_or(&[]);
		let cs = top
			.char_strings
			.as_ref()
			.and_then(|c| c.items.get(gid))
			.ok_or_else(fd_error)?;
		let stack_use = StackUse::new(local, global).execute(cs)?;
		// CFF stack depth is 48.
		if stack_use > 48 {
			super::transforms::desubroutinize_char_string(top, global, gid)?;
			let char_strings = top.char_strings.as_mut().ok_or_else(fd_error)?;
			let cs = &mut char_strings.items[gid];
			cs.program = specialize_program(&cs.program, None, &SpecializeOptions::default())?;
		}
	}

	// Unused subroutines are still in CFF2 (ie. lacking 'return' operator) because they
	// were not decompiled when the 'return' was added. Moreover, some used subroutines
	// may have become unused after the stack-use fixup. So remove all unused ones now.
	cff.remove_unused_subroutines()?;
	let top = cff.top_dicts.first_mut().ok_or_else(fd_error)?;

	if fd_count == 1 {
		// With a single FontDict there's no need for a CID-keyed font; make a name-keyed
		// one instead, which keeps the glyph names and is what most client code expects.
		let mut fds = top.fd_array.take().unwrap_or_default();
		top.private = fds.pop().and_then(|f| f.private);
		top.fd_select = None;
		top.raw
			.retain(|(k, _)| !matches!(*k, "FDArray" | "FDSelect"));
		if let Some(char_strings) = &mut top.char_strings {
			char_strings.has_fd_array = false;
			char_strings.has_fd_select = false;
			char_strings.fd_select = None;
			char_strings.privates = vec![PrivateRef::Top; char_strings.items.len()];
		}
		return Ok(());
	}

	let cid_name = |n: usize| {
		if n == 0 {
			".notdef".to_owned()
		} else {
			format!("cid{n:05}")
		}
	};
	let charset = top.charset.as_deref().unwrap_or_default();
	let mapping: std::collections::HashMap<String, String> = charset
		.iter()
		.enumerate()
		.map(|(n, name)| (name.clone(), cid_name(n)))
		.collect();
	let new_charset = (0..charset.len()).map(cid_name).collect();
	top.charset = Some(new_charset);
	if let Some(char_strings) = &mut top.char_strings {
		for name in &mut char_strings.names {
			*name = mapping
				.get(name)
				.cloned()
				.ok_or_else(|| Error::ttlib(format!("KeyError: {name:?}")))?;
		}
	}
	top.values.insert(
		"ROS",
		Value::Tuple(vec![
			Value::Str("Adobe".to_owned()),
			Value::Str("Identity".to_owned()),
			Value::Int(0),
		]),
	);
	Ok(())
}

/// `_get_hmtx` of `fontTools.cffLib.width`: the tag of the table with the widths.
pub fn get_hmtx_tag(font: &Font) -> Tag {
	let uppercase = Tag::new(b"HMTX");
	if font.contains(uppercase) {
		uppercase
	} else {
		Tag::new(b"hmtx")
	}
}

/// `convertCFF2ToCFF`: converts the `CFF2` table of a font to a `CFF ` table.
pub fn convert_cff2_to_cff(font: &mut Font, update_post_table: bool) -> Result<()> {
	let cff2 = Tag::new(b"CFF2");
	if !font.contains(cff2) {
		return Err(Error::value("Input font does not contain a CFF2 table."));
	}
	let widths: Vec<i64> = {
		let hmtx = font.require::<Hmtx>()?;
		let last = hmtx.metrics.keys().next_back().copied();
		last.map_or_else(Vec::new, |last| {
			(0..=last)
				.map(|g| hmtx.metrics.get(&g).map_or(0, |m| i64::from(m.0)))
				.collect()
		})
	};
	let mut cff = std::mem::take(&mut font.require::<Cff2>()?.cff);
	convert_font_set(&mut cff, &widths)?;
	font.remove(cff2);
	font.insert(Box::new(Cff { cff }));

	if update_post_table && font.contains(Tag::new(b"post")) {
		// The only version supported for fonts with a CFF table is 3, not 2.
		let post = font.require::<Post>()?;
		if post.format_type == 2.0 {
			post.format_type = 3.0;
		}
	}
	Ok(())
}

const HELP: &str = "\
usage: fonttools cffLib.CFF2ToCFF [-h] [-o OUTPUT.ttf] [--no-recalc-timestamp]
                                  [--remove-overlaps]
                                  [--ignore-overlap-errors] [-v | -q]
                                  INPUT.ttf

Convert a non-variable CFF2 font to CFF.

positional arguments:
  INPUT.ttf             Input OTF file with CFF table.

options:
  -h, --help            show this help message and exit
  -o, --output OUTPUT.ttf
                        Output instance OTF file (default: INPUT-CFF2.ttf).
  --no-recalc-timestamp
                        Don't set the output font's timestamp to the current
                        time.
  --remove-overlaps     Merge overlapping contours and components. Requires
                        skia-pathops
  --ignore-overlap-errors
                        Don't crash if the remove-overlaps operation fails for
                        some glyphs.
  -v, --verbose         Run more verbosely.
  -q, --quiet           Turn verbosity off.
";

const COMMAND: Command = Command {
	prog: "fonttools cffLib.CFF2ToCFF",
	module: "cffLib.CFF2ToCFF",
	help: HELP,
	arguments: &[
		Argument::help(),
		Argument::positional("INPUT.ttf"),
		Argument::option(&["-o", "--output"]),
		Argument::flag(&["--no-recalc-timestamp"]),
		Argument::flag(&["--remove-overlaps"]),
		Argument::flag(&["--ignore-overlap-errors"]),
		Argument::flag(&["-v", "--verbose"]).group(0),
		Argument::flag(&["-q", "--quiet"]).group(0),
	],
	required_groups: &[],
};

/// `fontTools.cffLib.CFF2ToCFF.main`: converts a non-variable CFF2 OTF font file
/// to a CFF one. Returns the exit code.
///
/// `-v` and `-q` are accepted; logging is up to the caller's `log`
/// implementation. `--remove-overlaps` needs the `pathops` feature (as
/// fontTools needs skia-pathops): without it, it fails with [`Error::Import`].
pub fn main(args: &[&str]) -> Result<i32> {
	run(
		args,
		&mut std::io::stdout().lock(),
		&mut std::io::stderr().lock(),
	)
}

/// [`main`], writing to `out` and `err`.
pub fn run(args: &[&str], out: &mut dyn Write, err: &mut dyn Write) -> Result<i32> {
	let options = match COMMAND.parse_args(args, out, err)? {
		Ok(options) => options,
		Err(code) => return Ok(code),
	};
	let infile = options.str("INPUT.ttf").unwrap_or_default();
	if !std::path::Path::new(infile).is_file() {
		return Ok(COMMAND.error(err, &format!("No such file '{infile}'"))?);
	}
	let outfile = match options.str("--output").filter(|o| !o.is_empty()) {
		Some(output) => output.to_owned(),
		None => make_output_file_name(infile, None, None, true, "-CFF"),
	};
	let mut font = Font::read(
		std::fs::read(infile)?,
		ReadOptions {
			recalc_timestamp: !options.flag("--no-recalc-timestamp"),
			recalc_bboxes: false,
			..Default::default()
		},
	)?;

	// UPSTREAM-BUG(B26): the conversion clears `cff2GetGlyphOrder` before the
	// charstrings of the freshly read font are loaded, which read the FDSelect with
	// it; the check of a variable font comes first.
	if font.contains(Tag::new(b"CFF2"))
		&& font
			.require::<Cff2>()?
			.cff
			.top_dicts
			.first()
			.is_some_and(|t| t.var_store.is_none() && t.raw.iter().any(|(k, _)| *k == "FDSelect"))
	{
		return Err(Error::ttlib("TypeError: 'NoneType' object is not callable"));
	}
	convert_cff2_to_cff(&mut font, true)?;

	if options.flag("--remove-overlaps") {
		log::debug!("Removing overlaps");
		let data = font.write(&WriteOptions::default())?;
		font = Font::read(
			data,
			ReadOptions {
				recalc_timestamp: false,
				recalc_bboxes: false,
				..Default::default()
			},
		)?;
		remove_overlaps(
			&mut font,
			&RemoveOverlapsOptions {
				ignore_errors: options.flag("--ignore-overlap-errors"),
				..Default::default()
			},
		)?;
	}

	log::info!("Saving {outfile}");
	std::fs::write(&outfile, font.write(&WriteOptions::default())?)?;
	Ok(0)
}
