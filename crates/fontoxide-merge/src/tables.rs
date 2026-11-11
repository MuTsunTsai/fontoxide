//! Port of `fontTools.merge.tables`: the merge logic of the tables other than
//! `cmap` and the layout tables.

use std::collections::BTreeMap;

use fontoxide_ttlib::cff::charstring::Token;
use fontoxide_ttlib::cff::dict::Value;
use fontoxide_ttlib::cff::font::PrivateRef;
use fontoxide_ttlib::tables::glyf::Glyph;
use fontoxide_ttlib::tables::hhea::{Hhea, Vhea};
use fontoxide_ttlib::tables::{Cff, Glyf, Head, Hmtx, Loca, Maxp, Name, Os2, Post, Vmtx};

use crate::{Error, Result};

/// `equal`: all the values must be the same.
pub(crate) fn equal<T: PartialEq + Copy + std::fmt::Debug>(values: &[T]) -> Result<T> {
	let first = *values
		.first()
		.ok_or_else(|| Error::Type("StopIteration".into()))?;
	if values.iter().any(|v| *v != first) {
		return Err(Error::Assertion(format!(
			"Expected all items to be equal: {values:?}"
		)));
	}
	Ok(first)
}

/// `max`: the first of the largest values.
fn max<T: PartialOrd + Copy>(values: &[T]) -> T {
	let mut m = values[0];
	for &v in &values[1..] {
		if v > m {
			m = v;
		}
	}
	m
}

/// `min`: the first of the smallest values.
fn min<T: PartialOrd + Copy>(values: &[T]) -> T {
	let mut m = values[0];
	for &v in &values[1..] {
		if v < m {
			m = v;
		}
	}
	m
}

/// Collects a field of the tables.
macro_rules! field {
	($tables:expr, $($f:tt)+) => {
		$tables.iter().map(|t| t.$($f)+).collect::<Vec<_>>()
	};
}

/// How a bit is merged (`mergeBits` bitmaps).
#[derive(Debug, Clone, Copy)]
enum Bit {
	/// `bitwise_or`.
	Or,
	/// `bitwise_and`.
	And,
	/// `lambda bit: 0`.
	Zero,
}

/// `mergeBits`: merges 16-bit flags bit by bit.
fn merge_bits(bitmap: impl Fn(u32) -> Bit, values: &[u32]) -> u32 {
	let mut result = 0;
	for bit in 0..16 {
		let mask = 1 << bit;
		let merged = match bitmap(bit) {
			Bit::Or => values.iter().any(|v| v & mask != 0),
			Bit::And => values.iter().all(|v| v & mask != 0),
			Bit::Zero => false,
		};
		result |= u32::from(merged) << bit;
	}
	result
}

/// `maxp.mergeMap`.
pub(crate) fn merge_maxp(tables: &[&Maxp], extended: bool) -> Result<Maxp> {
	let v1 = |f: fn(&Maxp) -> u16| max(&tables.iter().map(|t| f(t)).collect::<Vec<_>>());
	let mut merged = Maxp {
		table_version: equal(&field!(tables, table_version))?,
		num_glyphs: tables.iter().map(|t| t.num_glyphs).sum(),
		v1: tables[0].v1.clone(),
		extended,
	};
	let m = &mut merged.v1;
	m.max_points = v1(|t| t.v1.max_points);
	m.max_contours = v1(|t| t.v1.max_contours);
	m.max_composite_points = v1(|t| t.v1.max_composite_points);
	m.max_composite_contours = v1(|t| t.v1.max_composite_contours);
	m.max_zones = v1(|t| t.v1.max_zones);
	m.max_twilight_points = v1(|t| t.v1.max_twilight_points);
	// maxStorage, maxFunctionDefs and maxInstructionDefs are the first's.
	m.max_stack_elements = v1(|t| t.v1.max_stack_elements);
	// TODO (fontTools): when hinting data is merged correctly, update
	// maxFunctionDefs, maxInstructionDefs and maxSizeOfInstructions.
	m.max_size_of_instructions = v1(|t| t.v1.max_size_of_instructions);
	m.max_component_elements = v1(|t| t.v1.max_component_elements);
	m.max_component_depth = v1(|t| t.v1.max_component_depth);
	Ok(merged)
}

/// `headFlagsMergeBitMap`.
fn head_flags_bit(bit: u32) -> Bit {
	match bit {
		1 => Bit::And,   // Baseline at y = 0
		2 => Bit::And,   // lsb at x = 0
		3 => Bit::And,   // Force ppem to integer values. FIXME?
		5 => Bit::And,   // Font is vertical
		6 => Bit::Zero,  // Always set to zero
		11 => Bit::And,  // Font data is 'lossless'
		13 => Bit::And,  // Optimized for ClearType
		14 => Bit::And,  // Last resort font. FIXME? equal or first may be better
		15 => Bit::Zero, // Always set to zero
		_ => Bit::Or,
	}
}

/// `head.mergeMap`.
pub(crate) fn merge_head(tables: &[&Head]) -> Result<Head> {
	let units = field!(tables, units_per_em);
	if units.iter().any(|u| *u != units[0]) {
		return Err(Error::Value(format!(
			"Cannot merge fonts with different unitsPerEm values: {units:?}"
		)));
	}
	let now = fontoxide_misc::time_tools::timestamp_now()
		.ok_or_else(|| fontoxide_ttlib::Error::value("cannot get the current time"))?;
	let now = u64::try_from(now).map_err(|_| Error::Value("invalid timestamp".into()))?;
	let flags: Vec<u32> = tables.iter().map(|t| u32::from(t.flags)).collect();
	let mut head = tables[0].clone();
	head.table_version = max(&field!(tables, table_version));
	head.font_revision = max(&field!(tables, font_revision));
	// We need *something* here.
	head.check_sum_adjustment = 0;
	head.magic_number = equal(&field!(tables, magic_number))?;
	head.flags = merge_bits(head_flags_bit, &flags) as u16;
	head.units_per_em = units[0];
	head.created = now;
	head.modified = now;
	head.x_min = min(&field!(tables, x_min));
	head.y_min = min(&field!(tables, y_min));
	head.x_max = max(&field!(tables, x_max));
	head.y_max = max(&field!(tables, y_max));
	// macStyle and indexToLocFormat are the first's.
	head.lowest_rec_ppem = max(&field!(tables, lowest_rec_ppem));
	head.font_direction_hint = 2;
	head.glyph_data_format = equal(&field!(tables, glyph_data_format))?;
	Ok(head)
}

/// `hhea.mergeMap`.
pub(crate) fn merge_hhea(tables: &[&Hhea], extended: bool) -> Result<Hhea> {
	let mut hhea = tables[0].clone();
	hhea.table_version = max(&field!(tables, table_version));
	hhea.ascent = max(&field!(tables, ascent));
	hhea.descent = min(&field!(tables, descent));
	hhea.line_gap = max(&field!(tables, line_gap));
	hhea.advance_width_max = max(&field!(tables, advance_width_max));
	hhea.min_left_side_bearing = min(&field!(tables, min_left_side_bearing));
	hhea.min_right_side_bearing = min(&field!(tables, min_right_side_bearing));
	hhea.x_max_extent = max(&field!(tables, x_max_extent));
	// caretSlopeRise, caretSlopeRun and caretOffset are the first's.
	hhea.reserved0 = equal(&field!(tables, reserved0))?;
	hhea.reserved1 = equal(&field!(tables, reserved1))?;
	hhea.reserved2 = equal(&field!(tables, reserved2))?;
	hhea.reserved3 = equal(&field!(tables, reserved3))?;
	hhea.metric_data_format = equal(&field!(tables, metric_data_format))?;
	// Recalculated: compiling `hmtx` sets it.
	hhea.number_of_h_metrics = 0;
	hhea.extended = extended;
	Ok(hhea)
}

/// `vhea.mergeMap`.
pub(crate) fn merge_vhea(tables: &[&Vhea], extended: bool) -> Result<Vhea> {
	let mut vhea = tables[0].clone();
	vhea.table_version = max(&field!(tables, table_version));
	vhea.ascent = max(&field!(tables, ascent));
	vhea.descent = min(&field!(tables, descent));
	vhea.line_gap = max(&field!(tables, line_gap));
	vhea.advance_height_max = max(&field!(tables, advance_height_max));
	vhea.min_top_side_bearing = min(&field!(tables, min_top_side_bearing));
	vhea.min_bottom_side_bearing = min(&field!(tables, min_bottom_side_bearing));
	vhea.y_max_extent = max(&field!(tables, y_max_extent));
	// caretSlopeRise, caretSlopeRun and caretOffset are the first's.
	vhea.reserved1 = equal(&field!(tables, reserved1))?;
	vhea.reserved2 = equal(&field!(tables, reserved2))?;
	vhea.reserved3 = equal(&field!(tables, reserved3))?;
	vhea.reserved4 = equal(&field!(tables, reserved4))?;
	vhea.metric_data_format = equal(&field!(tables, metric_data_format))?;
	// Recalculated: compiling `vmtx` sets it.
	vhea.number_of_v_metrics = 0;
	vhea.extended = extended;
	Ok(vhea)
}

/// `os2FsTypeMergeBitMap`.
fn os2_fs_type_bit(bit: u32) -> Bit {
	match bit {
		1 => Bit::Or,  // no embedding permitted
		2 => Bit::And, // allow previewing and printing documents
		3 => Bit::And, // allow editing documents
		8 => Bit::Or,  // no subsetting permitted
		9 => Bit::Or,  // no embedding of outlines permitted
		_ => Bit::Zero,
	}
}

/// `mergeOs2FsType`: the least restrictive embedding permissions.
pub fn merge_os2_fs_type(values: &[u16]) -> u16 {
	if values.iter().all(|&v| v == 0) {
		return 0;
	}
	let mut values: Vec<u32> = values.iter().map(|&v| u32::from(v)).collect();
	// Compute least restrictive logic for each fsType value.
	for v in &mut values {
		if *v & 0x000C != 0 {
			// Unset bit 1 (no embedding permitted) if either bit 2 or 3 is set.
			*v &= !0x0002;
		} else if *v & 0x0008 != 0 {
			// Set bit 2 (allow previewing) if bit 3 is set (allow editing).
			*v |= 0x0004;
		} else if *v == 0 {
			// Set bits 2 and 3 if everything is allowed.
			*v = 0x000C;
		}
	}
	let mut fs_type = merge_bits(os2_fs_type_bit, &values);
	// Unset bits 2 and 3 if bit 1 is set (some font is "no embedding").
	if fs_type & 0x0002 != 0 {
		fs_type &= !0x000C;
	}
	fs_type as u16
}

/// `OS/2.mergeMap` and `table_O_S_2f_2.merge`.
pub(crate) fn merge_os2(tables: &[&Os2]) -> Result<Os2> {
	let mut os2 = tables[0].clone();
	os2.version = max(&field!(tables, version));
	// xAvgCharWidth will be recalculated at the end on the merged font.
	os2.fs_type = merge_os2_fs_type(&field!(tables, fs_type));
	// panose: FIXME (fontTools): should really be the first Latin font.
	os2.ul_unicode_range1 = tables.iter().fold(0, |a, t| a | t.ul_unicode_range1);
	os2.ul_unicode_range2 = tables.iter().fold(0, |a, t| a | t.ul_unicode_range2);
	os2.ul_unicode_range3 = tables.iter().fold(0, |a, t| a | t.ul_unicode_range3);
	os2.ul_unicode_range4 = tables.iter().fold(0, |a, t| a | t.ul_unicode_range4);
	// UPSTREAM-BUG(B42): the merge map has `fsFirstCharIndex` and `fsLastCharIndex`
	// (min and max), which are not fields, so `usFirstCharIndex` and
	// `usLastCharIndex` are the first's. Compiling recalculates them anyway.
	os2.s_typo_ascender = max(&field!(tables, s_typo_ascender));
	os2.s_typo_descender = min(&field!(tables, s_typo_descender));
	os2.s_typo_line_gap = max(&field!(tables, s_typo_line_gap));
	os2.us_win_ascent = max(&field!(tables, us_win_ascent));
	os2.us_win_descent = max(&field!(tables, us_win_descent));
	// The fields of later versions merge only the tables that have them
	// (`onlyExisting`).
	let v1: Vec<&&Os2> = tables.iter().filter(|t| t.version >= 1).collect();
	if !v1.is_empty() {
		os2.ul_code_page_range1 = v1.iter().fold(0, |a, t| a | t.ul_code_page_range1);
		os2.ul_code_page_range2 = v1.iter().fold(0, |a, t| a | t.ul_code_page_range2);
	}
	let v2: Vec<&&Os2> = tables.iter().filter(|t| t.version >= 2).collect();
	if !v2.is_empty() {
		os2.sx_height = max(&field!(v2, sx_height));
		os2.s_cap_height = max(&field!(v2, s_cap_height));
		os2.us_default_char = v2[0].us_default_char;
		os2.us_break_char = v2[0].us_break_char;
		os2.us_max_context = max(&field!(v2, us_max_context));
	}
	let v5: Vec<&&Os2> = tables.iter().filter(|t| t.version >= 5).collect();
	if !v5.is_empty() {
		os2.us_lower_optical_point_size = min(&field!(v5, us_lower_optical_point_size));
		os2.us_upper_optical_point_size = max(&field!(v5, us_upper_optical_point_size));
	}
	if os2.version < 2 {
		// Bits 8 and 9 are reserved and should be set to zero.
		os2.fs_type &= !0x0300;
	}
	if os2.version >= 3 && os2.fs_type & 0x0008 != 0 {
		// Only one of bits 1, 2, and 3 may be set. We already take care of bit 1
		// implications in mergeOs2FsType. So unset bit 2 if bit 3 is already set.
		os2.fs_type &= !0x0004;
	}
	Ok(os2)
}

/// `post.mergeMap`.
#[allow(clippy::field_reassign_with_default)]
pub(crate) fn merge_post(tables: &[&Post]) -> Result<Post> {
	let mut post = Post::default();
	post.header = tables[0].header.clone();
	post.format_type = max(&field!(tables, format_type));
	post.is_fixed_pitch = min(&field!(tables, is_fixed_pitch));
	post.min_mem_type42 = max(&field!(tables, min_mem_type42));
	post.max_mem_type42 = 0;
	post.min_mem_type1 = max(&field!(tables, min_mem_type1));
	post.max_mem_type1 = 0;
	let mappings: Vec<&BTreeMap<String, String>> =
		tables.iter().filter_map(|t| t.mapping.as_ref()).collect();
	if !mappings.is_empty() {
		let mut mapping = BTreeMap::new();
		for m in mappings {
			mapping.extend(m.iter().map(|(k, v)| (k.clone(), v.clone())));
		}
		post.mapping = Some(mapping);
	}
	if tables.iter().any(|t| t.extra_names.is_some()) {
		post.extra_names = Some(Vec::new());
	}
	Ok(post)
}

/// `hmtx.mergeMap`: the metrics of all glyphs.
pub(crate) fn merge_hmtx(tables: &mut [&mut Hmtx], extended: bool) -> Hmtx {
	let mut metrics = BTreeMap::new();
	for t in tables {
		metrics.append(&mut t.metrics);
	}
	Hmtx { metrics, extended }
}

/// `vmtx.mergeMap`.
pub(crate) fn merge_vmtx(tables: &mut [&mut Vmtx], extended: bool) -> Vmtx {
	let mut metrics = BTreeMap::new();
	for t in tables {
		metrics.append(&mut t.metrics);
	}
	Vmtx { metrics, extended }
}

/// `name.mergeMap`: the names of the first font. FIXME (fontTools): does mixing
/// name records make sense?
pub(crate) fn merge_name(tables: &[&Name]) -> Name {
	tables[0].clone()
}

/// `loca.mergeMap`: recalculated when compiling `glyf`.
pub(crate) fn merge_loca(extended: bool) -> Loca {
	Loca {
		locations: None,
		extended,
	}
}

/// `table__g_l_y_f.merge`.
pub(crate) fn merge_glyf(
	tables: &mut [&mut Glyf],
	extended: bool,
	num_glyphs: usize,
) -> Result<Glyf> {
	let mut glyphs = BTreeMap::new();
	for (i, table) in tables.iter_mut().enumerate() {
		let table_extended = table.extended;
		for (gid, mut g) in std::mem::take(&mut table.glyphs) {
			if i > 0 {
				// Drop hints for all but first font, since we don't map functions /
				// CVT values.
				g.trim_ext(true, table_extended)?;
				if let Some(data) = g.data.take() {
					// The glyph is expanded from what is left when it is compiled.
					g = Glyph::decompile_ext(&data, num_glyphs, table_extended)?;
					g.data = Some(data);
				}
			}
			// Expand composite glyphs to load their composite glyph names.
			if g.is_composite() {
				g.data = None;
			}
			glyphs.insert(gid, g);
		}
	}
	Ok(Glyf {
		glyphs,
		extended,
		..Default::default()
	})
}

/// A number of a charstring or a Private DICT, with Python's `int` and
/// `float` arithmetic.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Num {
	Int(i64),
	Float(f64),
}

impl Num {
	fn of_value(v: Option<Value>) -> Result<Num> {
		match v {
			Some(Value::Int(i)) => Ok(Num::Int(i)),
			Some(Value::Float(f)) => Ok(Num::Float(f)),
			_ => Err(Error::Type("TypeError: width is not a number".into())),
		}
	}

	fn of_token(t: &Token) -> Result<Num> {
		match t {
			Token::Int(i) => Ok(Num::Int(*i)),
			Token::Float(f) => Ok(Num::Float(*f)),
			_ => Err(Error::Type("TypeError: width is not a number".into())),
		}
	}

	fn float(self) -> f64 {
		match self {
			Num::Int(i) => i as f64,
			Num::Float(f) => f,
		}
	}

	fn add(self, other: Num) -> Num {
		match (self, other) {
			(Num::Int(a), Num::Int(b)) => Num::Int(a + b),
			(a, b) => Num::Float(a.float() + b.float()),
		}
	}

	fn sub(self, other: Num) -> Num {
		match (self, other) {
			(Num::Int(a), Num::Int(b)) => Num::Int(a - b),
			(a, b) => Num::Float(a.float() - b.float()),
		}
	}

	/// Python `==`, where `1 == 1.0`.
	fn py_eq(self, other: Num) -> bool {
		match (self, other) {
			(Num::Int(a), Num::Int(b)) => a == b,
			(a, b) => a.float() == b.float(),
		}
	}

	fn token(self) -> Token {
		match self {
			Num::Int(i) => Token::Int(i),
			Num::Float(f) => Token::Float(f),
		}
	}
}

/// The width of a charstring, as `T2WidthExtractor` finds it in a program
/// without subroutines: `None` if it is `defaultWidthX`.
fn extract_width(program: &[Token], nominal_width_x: Num) -> Result<Option<Num>> {
	let mut stack: Vec<Num> = Vec::new();
	let mut hint_count = 0;
	let mut hint_mask_bytes = 0;
	// `reset` sets the width to 0.
	let mut width = Some(Num::Int(0));
	let mut got_width = false;
	let mut pop_all_width = |stack: &mut Vec<Num>, even_odd: usize| -> usize {
		let args = std::mem::take(stack);
		let mut n = args.len();
		if !got_width {
			if (even_odd ^ (args.len() % 2)) != 0 {
				width = Some(nominal_width_x.add(args[0]));
				n -= 1;
			} else {
				width = None;
			}
			got_width = true;
		}
		n
	};
	let mut tokens = program.iter();
	while let Some(token) = tokens.next() {
		let op = match token {
			Token::Int(_) | Token::Float(_) => {
				stack.push(Num::of_token(token)?);
				continue;
			}
			Token::Mask(_) => continue,
			Token::Op(op) => *op,
		};
		match op {
			"hstem" | "vstem" | "hstemhm" | "vstemhm" => {
				hint_count += pop_all_width(&mut stack, 0) / 2;
			}
			"hintmask" | "cntrmask" => {
				if hint_mask_bytes == 0 {
					hint_count += pop_all_width(&mut stack, 0) / 2;
					hint_mask_bytes = hint_count.div_ceil(8);
				}
				// The mask bytes follow.
				tokens.next();
			}
			"rmoveto" | "endchar" => {
				pop_all_width(&mut stack, 0);
			}
			"hmoveto" | "vmoveto" => {
				pop_all_width(&mut stack, 1);
			}
			"return" | "ignore" => {}
			"callsubr" | "callgsubr" => {
				return Err(Error::Type("IndexError: list index out of range".into()));
			}
			"and" | "or" | "not" | "store" | "abs" | "add" | "sub" | "div" | "load" | "neg"
			| "eq" | "drop" | "put" | "get" | "ifelse" | "random" | "mul" | "sqrt" | "dup"
			| "exch" | "index" | "roll" | "blend" | "vsindex" => {
				return Err(Error::NotImplemented(format!("charstring operator {op}")));
			}
			_ => stack.clear(),
		}
	}
	Ok(width)
}

/// `table_C_F_F_.merge`: the charstrings of all fonts in the first's CFF, with
/// subroutines inlined and widths adjusted to its Private DICT.
pub(crate) fn merge_cff(tables: &mut [&mut Cff]) -> Result<Cff> {
	if tables.iter().any(|t| {
		t.cff
			.top_dicts
			.first()
			.is_some_and(|top| top.fd_select.is_some())
	}) {
		return Err(Error::NotImplemented(
			"Merging CID-keyed CFF tables is not supported yet".into(),
		));
	}
	for table in tables.iter_mut() {
		table.cff.desubroutinize()?;
	}
	let index_error = || Error::Type("IndexError: list index out of range".into());
	let (first, rest) = tables.split_first_mut().ok_or_else(index_error)?;
	let mut new_cff = std::mem::take(&mut **first);
	let strings = new_cff.cff.strings.take();
	let new_font = new_cff.cff.top_dicts.first_mut().ok_or_else(index_error)?;
	let private = new_font
		.private
		.as_deref()
		.ok_or_else(|| Error::Type("AttributeError: Private".into()))?;
	let new_default_width_x = Num::of_value(private.get("defaultWidthX"))?;
	let new_nominal_width_x = Num::of_value(private.get("nominalWidthX"))?;

	// The strings that are glyph names come first. fontTools compiles a new
	// string index anyway.
	let mut stored_names_strings = Vec::new();
	let mut glyph_order_strings = Vec::new();
	let glyph_order: std::collections::HashSet<&String> =
		new_font.charset.iter().flatten().collect();
	for name in strings.iter().flat_map(|s| &s.strings) {
		if glyph_order.contains(name) {
			glyph_order_strings.push(name.clone());
		} else {
			stored_names_strings.push(name.clone());
		}
	}

	let mut chrset = new_font.charset.clone().unwrap_or_default();
	let new_cs = new_font
		.char_strings
		.as_mut()
		.ok_or_else(|| Error::Type("AttributeError: CharStrings".into()))?;
	log::debug!("FONT 0 CharStrings: {}.", new_cs.names.len());

	for (i, table) in rest.iter_mut().enumerate() {
		let font_strings = table.cff.strings.take();
		let font = table.cff.top_dicts.first_mut().ok_or_else(index_error)?;
		let font_private = font
			.private
			.as_deref()
			.ok_or_else(|| Error::Type("AttributeError: Private".into()))?;
		let default_width_x = Num::of_value(font_private.get("defaultWidthX"))?;
		let nominal_width_x = Num::of_value(font_private.get("nominalWidthX"))?;
		let widths_differ = !default_width_x.py_eq(new_default_width_x)
			|| !nominal_width_x.py_eq(new_nominal_width_x);
		let font_glyph_order: std::collections::HashSet<&String> =
			font.charset.iter().flatten().collect();
		for name in font_strings.iter().flat_map(|s| &s.strings) {
			if font_glyph_order.contains(name) {
				glyph_order_strings.push(name.clone());
			}
		}
		chrset.extend(font.charset.iter().flatten().cloned());
		let cs = font
			.char_strings
			.as_mut()
			.ok_or_else(|| Error::Type("AttributeError: CharStrings".into()))?;
		log::debug!("Font {} CharStrings: {}.", i + 1, cs.names.len());
		for (name, mut c) in std::mem::take(&mut cs.names)
			.into_iter()
			.zip(std::mem::take(&mut cs.items))
		{
			if widths_differ {
				let width = match extract_width(&c.program, nominal_width_x)? {
					Some(width) => {
						// The following will be wrong if the width is added by a
						// subroutine. Ouch! (fontTools' note)
						if c.program.is_empty() {
							return Err(Error::Type("IndexError: pop from empty list".into()));
						}
						c.program.remove(0);
						width
					}
					None => default_width_x,
				};
				if !width.py_eq(new_default_width_x) {
					c.program.insert(0, width.sub(new_nominal_width_x).token());
				}
			}
			new_cs.names.push(name);
			new_cs.items.push(c);
			new_cs.privates.push(PrivateRef::Top);
			if let Some(fd) = &mut new_cs.fd_select {
				fd.push(None);
			}
		}
	}

	new_font.num_glyphs = Some(chrset.len());
	new_font.charset = Some(chrset);
	if let Some(mut strings) = strings {
		strings.strings = glyph_order_strings;
		strings.strings.extend(stored_names_strings);
		new_cff.cff.strings = Some(strings);
	}
	Ok(new_cff)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn fs_type() {
		assert_eq!(merge_os2_fs_type(&[0, 0]), 0);
		// Installable and preview & print: preview & print.
		assert_eq!(merge_os2_fs_type(&[0, 4]), 4);
		// Restricted wins over editable.
		assert_eq!(merge_os2_fs_type(&[2, 8]), 2);
		assert_eq!(merge_os2_fs_type(&[0x0100, 0]), 0x0100);
	}

	#[test]
	fn width() -> Result<()> {
		let p = |t: &[Token]| t.to_vec();
		let w = extract_width(
			&p(&[
				Token::Int(10),
				Token::Int(20),
				Token::Op("hmoveto"),
				Token::Op("endchar"),
			]),
			Num::Int(100),
		)?;
		assert_eq!(w, Some(Num::Int(110)));
		let w = extract_width(&p(&[Token::Int(20), Token::Op("hmoveto")]), Num::Int(100))?;
		assert_eq!(w, None);
		let w = extract_width(&p(&[Token::Op("endchar")]), Num::Float(1.5))?;
		assert_eq!(w, None);
		let w = extract_width(&p(&[Token::Int(5), Token::Op("endchar")]), Num::Float(1.5))?;
		assert_eq!(w, Some(Num::Float(6.5)));
		Ok(())
	}
}
