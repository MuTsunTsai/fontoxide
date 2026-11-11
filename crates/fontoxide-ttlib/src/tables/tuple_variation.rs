//! Tuple variations and their packed deltas
//! (`fontTools.ttLib.tables.TupleVariation`), shared by `gvar`, `cvar` and `VARC`.

use std::collections::{BTreeMap, HashMap};

use fontoxide_misc::fixed_tools::{
	fixed_to_float, float_to_fixed, float_to_fixed_to_float, float_to_fixed_to_str,
	str_to_fixed_to_float,
};
use fontoxide_misc::round_tools::ot_round;
use fontoxide_misc::text_tools::{Literal, safe_eval};
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::tables::glyf::Number;
use crate::var::iup_delta;
use crate::{Error, Result};

const DELTAS_ARE_ZERO: u8 = 0x80;
const DELTAS_ARE_WORDS: u8 = 0x40;
const DELTAS_ARE_LONGS: u8 = 0xC0;
const DELTAS_SIZE_MASK: u8 = 0xC0;
const DELTA_RUN_COUNT_MASK: u8 = 0x3F;

/// Packs deltas as runs (`compileDeltaValues_`).
///
/// Each run starts with a header byte whose low 6 bits hold the number of values
/// minus one; the high bits tell whether the values are zero (no data follows),
/// bytes, words or longs. With `optimize_size`, runs are split wherever that
/// makes the data smaller; otherwise all values go in runs of one size.
pub fn compile_delta_values(deltas: &[i64], optimize_size: bool) -> Result<Vec<u8>> {
	let mut out = Vec::new();
	let mut pos = 0;
	let n = deltas.len();
	if optimize_size {
		while pos < n {
			let value = deltas[pos];
			pos = if value == 0 {
				encode_zeroes(deltas, pos, &mut out)
			} else if (-128..=127).contains(&value) {
				encode_bytes(deltas, pos, &mut out, true)
			} else if (-32768..=32767).contains(&value) {
				encode_words(deltas, pos, &mut out, true)
			} else {
				encode_longs(deltas, pos, &mut out, true)?
			};
		}
	} else if n > 0 {
		let min = *deltas.iter().min().unwrap_or(&0);
		let max = *deltas.iter().max().unwrap_or(&0);
		pos = if min == 0 && max == 0 {
			encode_zeroes(deltas, pos, &mut out)
		} else if -128 <= min && max <= 127 {
			encode_bytes(deltas, pos, &mut out, false)
		} else if -32768 <= min && max <= 32767 {
			encode_words(deltas, pos, &mut out, false)
		} else {
			encode_longs(deltas, pos, &mut out, false)?
		};
	}
	if pos != n {
		return Err(Error::Assertion(format!("({pos}, {n})")));
	}
	Ok(out)
}

/// Writes the runs of `values`, at most 64 values each.
fn emit_runs(
	values: &[i64],
	out: &mut Vec<u8>,
	flag: u8,
	mut put: impl FnMut(i64, &mut Vec<u8>) -> Result<()>,
) -> Result<()> {
	for chunk in values.chunks(64) {
		out.push(flag | (chunk.len() - 1) as u8);
		for &v in chunk {
			put(v, out)?;
		}
	}
	Ok(())
}

fn encode_zeroes(deltas: &[i64], offset: usize, out: &mut Vec<u8>) -> usize {
	let mut pos = offset;
	while pos < deltas.len() && deltas[pos] == 0 {
		pos += 1;
	}
	let _ = emit_runs(&deltas[offset..pos], out, DELTAS_ARE_ZERO, |_, _| Ok(()));
	pos
}

fn encode_bytes(deltas: &[i64], offset: usize, out: &mut Vec<u8>, optimize_size: bool) -> usize {
	let mut pos = offset;
	let n = deltas.len();
	while pos < n {
		let value = deltas[pos];
		if !(-128..=127).contains(&value) {
			break;
		}
		// Two or more zeroes are better stored as a run of their own.
		if optimize_size && value == 0 && pos + 1 < n && deltas[pos + 1] == 0 {
			break;
		}
		pos += 1;
	}
	let _ = emit_runs(&deltas[offset..pos], out, 0, |v, out| {
		out.push(v as i8 as u8);
		Ok(())
	});
	pos
}

fn encode_words(deltas: &[i64], offset: usize, out: &mut Vec<u8>, optimize_size: bool) -> usize {
	let mut pos = offset;
	let n = deltas.len();
	let is_byte = |v: i64| (-128..=127).contains(&v);
	while pos < n {
		let value = deltas[pos];
		// A zero starts a new run; so do two values that fit in bytes, while a
		// single one is cheaper stored as a word.
		if optimize_size && value == 0 {
			break;
		}
		if optimize_size && is_byte(value) && pos + 1 < n && is_byte(deltas[pos + 1]) {
			break;
		}
		if !(-32768..=32767).contains(&value) {
			break;
		}
		pos += 1;
	}
	let _ = emit_runs(&deltas[offset..pos], out, DELTAS_ARE_WORDS, |v, out| {
		out.extend_from_slice(&(v as i16).to_be_bytes());
		Ok(())
	});
	pos
}

fn encode_longs(
	deltas: &[i64],
	offset: usize,
	out: &mut Vec<u8>,
	optimize_size: bool,
) -> Result<usize> {
	let mut pos = offset;
	while pos < deltas.len() {
		if optimize_size && (-32768..=32767).contains(&deltas[pos]) {
			break;
		}
		pos += 1;
	}
	emit_runs(&deltas[offset..pos], out, DELTAS_ARE_LONGS, |v, out| {
		let v = i32::try_from(v)
			.map_err(|_| Error::value("OverflowError: signed integer is greater than maximum"))?;
		out.extend_from_slice(&v.to_be_bytes());
		Ok(())
	})?;
	Ok(pos)
}

/// Unpacks deltas starting at `offset` (`decompileDeltas_`): `count` of them, or
/// to the end of the data if `None`. Returns the deltas and the offset after them.
pub fn decompile_deltas(
	count: Option<usize>,
	data: &[u8],
	offset: usize,
) -> Result<(Vec<i64>, usize)> {
	let mut result = Vec::new();
	let mut pos = offset;
	loop {
		let more = match count {
			Some(n) => result.len() < n,
			None => pos < data.len(),
		};
		if !more {
			break;
		}
		let header = *data
			.get(pos)
			.ok_or_else(|| Error::ttlib("IndexError: index out of range"))?;
		pos += 1;
		let run = (header & DELTA_RUN_COUNT_MASK) as usize + 1;
		let size = match header & DELTAS_SIZE_MASK {
			DELTAS_ARE_ZERO => {
				result.resize(result.len() + run, 0);
				continue;
			}
			DELTAS_ARE_LONGS => 4,
			DELTAS_ARE_WORDS => 2,
			_ => 1,
		};
		let start = pos.min(data.len());
		let bytes = &data[start..(pos + run * size).min(data.len())];
		if bytes.len() != run * size {
			return Err(Error::Assertion(format!("({}, {run})", bytes.len() / size)));
		}
		result.extend(bytes.chunks_exact(size).map(|c| match size {
			4 => i32::from_be_bytes([c[0], c[1], c[2], c[3]]) as i64,
			2 => i16::from_be_bytes([c[0], c[1]]) as i64,
			_ => c[0] as i8 as i64,
		}));
		pos += run * size;
	}
	if let Some(n) = count
		&& result.len() != n
	{
		return Err(Error::Assertion(format!(
			"expected {n} deltas, got {}",
			result.len()
		)));
	}
	Ok((result, pos))
}

// ---------------------------------------------------------------------------
// Tuple variations (`TupleVariation` and the tuple variation store)

const EMBEDDED_PEAK_TUPLE: u16 = 0x8000;
const INTERMEDIATE_REGION: u16 = 0x4000;
const PRIVATE_POINT_NUMBERS: u16 = 0x2000;
const POINTS_ARE_WORDS: u8 = 0x80;
const POINT_RUN_COUNT_MASK: u8 = 0x7F;
/// The variations share their point numbers (in the tuple variation count).
pub const TUPLES_SHARE_POINT_NUMBERS: u16 = 0x8000;
const TUPLE_COUNT_MASK: u16 = 0x0FFF;
const TUPLE_INDEX_MASK: u16 = 0x0FFF;

/// A delta of a tuple variation: of a point (`gvar`) or of a value (`cvar`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Delta {
	/// An (x, y) delta.
	Point(Number, Number),
	/// A single delta.
	Value(Number),
}

/// A region of a variation on one axis: start, peak and end.
pub type Region = (f64, f64, f64);

/// A variation of the points or values for a region of the design space
/// (`TupleVariation`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TupleVariation {
	/// The region on each axis, by axis tag (`axes`); missing axes do not count.
	pub axes: BTreeMap<String, Region>,
	/// The delta of each point or value; `None` where it is inferred (`gvar`) or
	/// unchanged (`cvar`) (`coordinates`).
	pub coordinates: Vec<Option<Delta>>,
}

/// The default start and end of a region with a peak (`inferRegion_`).
pub fn infer_region(peak: f64) -> (f64, f64) {
	(
		if peak < 0.0 { peak } else { 0.0 },
		if peak > 0.0 { peak } else { 0.0 },
	)
}

fn f2dot14(v: f64) -> Result<[u8; 2]> {
	let fixed = float_to_fixed(v, 14);
	i16::try_from(fixed)
		.map(i16::to_be_bytes)
		.map_err(|_| Error::Struct("'h' format requires -32768 <= number <= 32767".into()))
}

impl TupleVariation {
	/// A variation with no regions and `count` unset deltas.
	pub fn new(count: usize) -> Self {
		TupleVariation {
			axes: BTreeMap::new(),
			coordinates: vec![None; count],
		}
	}

	/// The points with deltas (`getUsedPoints`): an empty list means all, `None`
	/// means none.
	pub fn used_points(&self) -> Option<Vec<usize>> {
		if self.coordinates.iter().all(Option::is_some) {
			return Some(Vec::new());
		}
		let used: Vec<usize> = self
			.coordinates
			.iter()
			.enumerate()
			.filter(|(_, c)| c.is_some())
			.map(|(i, _)| i)
			.collect();
		(!used.is_empty()).then_some(used)
	}

	/// Whether any delta is set (`hasImpact`).
	pub fn has_impact(&self) -> bool {
		self.coordinates.iter().any(Option::is_some)
	}

	/// The peak of each axis, as F2Dot14 (`compileCoord`).
	pub fn compile_coord(&self, axis_tags: &[String]) -> Result<Vec<u8>> {
		let mut out = Vec::with_capacity(axis_tags.len() * 2);
		for axis in axis_tags {
			match self.axes.get(axis) {
				None => out.extend_from_slice(&[0, 0]),
				Some(&(_, peak, _)) => out.extend_from_slice(&f2dot14(peak)?),
			}
		}
		Ok(out)
	}

	/// The start and end of each axis, if any differ from the default
	/// (`compileIntermediateCoord`).
	pub fn compile_intermediate_coord(&self, axis_tags: &[String]) -> Result<Option<Vec<u8>>> {
		let region = |axis: &String| self.axes.get(axis).copied().unwrap_or((0.0, 0.0, 0.0));
		let needed = axis_tags.iter().any(|axis| {
			let (min, peak, max) = region(axis);
			let (dmin, dmax) = infer_region(peak);
			min != dmin || max != dmax
		});
		if !needed {
			return Ok(None);
		}
		let mut mins = Vec::new();
		let mut maxs = Vec::new();
		for axis in axis_tags {
			let (min, _, max) = region(axis);
			mins.extend_from_slice(&f2dot14(min)?);
			maxs.extend_from_slice(&f2dot14(max)?);
		}
		mins.extend(maxs);
		Ok(Some(mins))
	}

	/// The packed deltas: all x (or values), then all y (`compileDeltas`).
	pub fn compile_deltas(&self, optimize_size: bool) -> Result<Vec<u8>> {
		// A float delta fails in `array.array` unless it is a zero, which is
		// written as a run of zeros.
		let int = |n: Number| -> Result<i64> {
			match n {
				Number::Int(i) => Ok(i),
				Number::Float(0.0) => Ok(0),
				Number::Float(_) => Err(Error::ttlib(
					"TypeError: 'float' object cannot be interpreted as an integer",
				)),
			}
		};
		let width = self.coordinates.iter().flatten().next();
		let (mut xs, mut ys) = (Vec::new(), Vec::new());
		for d in self.coordinates.iter().flatten() {
			match (width, d) {
				(Some(Delta::Point(..)), Delta::Point(x, y)) => {
					xs.push(int(*x)?);
					ys.push(int(*y)?);
				}
				(Some(Delta::Value(_)), Delta::Value(v)) => xs.push(int(*v)?),
				_ => return Err(Error::ttlib("TypeError: mixed delta types")),
			}
		}
		let mut out = compile_delta_values(&xs, optimize_size)?;
		out.extend(compile_delta_values(&ys, optimize_size)?);
		Ok(out)
	}

	/// The tuple header and the point and delta data (`compile`); `point_data`
	/// empty means the shared points.
	pub fn compile(
		&self,
		axis_tags: &[String],
		shared_coords: &HashMap<Vec<u8>, usize>,
		point_data: &[u8],
		optimize_size: bool,
	) -> Result<(Vec<u8>, Vec<u8>)> {
		if let Some(unknown) = self.axes.keys().find(|a| !axis_tags.contains(a)) {
			return Err(Error::Assertion(format!(
				"Unknown axis tag found: {unknown:?}"
			)));
		}
		let mut tuple = Vec::new();
		let coord = self.compile_coord(axis_tags)?;
		let mut flags = match shared_coords.get(&coord) {
			Some(&i) => i as u16,
			None => {
				tuple.extend_from_slice(&coord);
				EMBEDDED_PEAK_TUPLE
			}
		};
		if let Some(intermediate) = self.compile_intermediate_coord(axis_tags)? {
			flags |= INTERMEDIATE_REGION;
			tuple.extend(intermediate);
		}
		let mut aux = Vec::new();
		if !point_data.is_empty() {
			flags |= PRIVATE_POINT_NUMBERS;
			aux.extend_from_slice(point_data);
		}
		aux.extend(self.compile_deltas(optimize_size)?);
		let size = u16::try_from(aux.len())
			.map_err(|_| Error::Struct("'H' format requires 0 <= number <= 65535".into()))?;
		let mut header = size.to_be_bytes().to_vec();
		header.extend_from_slice(&flags.to_be_bytes());
		header.extend(tuple);
		Ok((header, aux))
	}

	/// 2 for (x, y) deltas (`gvar`), 1 for single values (`cvar`), or 0 if there
	/// are none (`getCoordWidth`).
	pub fn coord_width(&self) -> u8 {
		match self.coordinates.iter().flatten().next() {
			Some(Delta::Point(..)) => 2,
			Some(Delta::Value(_)) => 1,
			None => 0,
		}
	}

	/// Multiplies the deltas by `scalar` (`scaleDeltas`); they become floats
	/// unless `scalar` is 1.
	pub fn scale_deltas(&mut self, scalar: f64) {
		if scalar == 1.0 {
			return;
		}
		let scale = |n: Number| Number::Float(n.value() * scalar);
		for delta in self.coordinates.iter_mut().flatten() {
			*delta = match *delta {
				Delta::Point(x, y) => Delta::Point(scale(x), scale(y)),
				Delta::Value(v) => Delta::Value(scale(v)),
			};
		}
	}

	/// Rounds the deltas to integers with `otRound` (`roundDeltas`).
	pub fn round_deltas(&mut self) {
		let round = |n: Number| Number::Int(ot_round(n.value()));
		for delta in self.coordinates.iter_mut().flatten() {
			*delta = match *delta {
				Delta::Point(x, y) => Delta::Point(round(x), round(y)),
				Delta::Value(v) => Delta::Value(round(v)),
			};
		}
	}

	/// Fills in the inferred deltas of a `gvar` variation from the glyph's
	/// coordinates (with the four phantom points) and contour ends
	/// (`calcInferredDeltas`).
	pub fn calc_inferred_deltas(&mut self, coords: &[(f64, f64)], ends: &[usize]) -> Result<()> {
		if self.coord_width() == 1 {
			return Err(Error::ttlib(
				"TypeError: Only 'gvar' TupleVariation can have inferred deltas",
			));
		}
		if self.coordinates.iter().all(Option::is_some) {
			return Ok(());
		}
		if self.coordinates.len() != coords.len() {
			return Err(Error::value(format!(
				"Expected len(origCoords) == {}; found {}",
				self.coordinates.len(),
				coords.len()
			)));
		}
		let deltas: Vec<Option<(f64, f64)>> = self
			.coordinates
			.iter()
			.map(|d| match d {
				Some(Delta::Point(x, y)) => Some((x.value(), y.value())),
				_ => None,
			})
			.collect();
		self.coordinates = iup_delta(&deltas, coords, ends)?
			.into_iter()
			.map(|(x, y)| Some(Delta::Point(Number::Float(x), Number::Float(y))))
			.collect();
		Ok(())
	}

	/// Adds the deltas of `other` (`__iadd__`). Missing `gvar` deltas must be
	/// inferred first; missing `cvar` deltas count as zero.
	pub fn add_deltas(&mut self, other: &TupleVariation) -> Result<()> {
		if self.coordinates.len() != other.coordinates.len() {
			return Err(Error::value(
				"cannot sum TupleVariation deltas with different lengths",
			));
		}
		let add = |x: Number, y: Number| match (x, y) {
			(Number::Int(x), Number::Int(y)) => Number::Int(x + y),
			_ => Number::Float(x.value() + y.value()),
		};
		if self.coord_width() == 2 {
			for (d1, d2) in self.coordinates.iter_mut().zip(&other.coordinates) {
				match (*d1, d2) {
					(Some(Delta::Point(x1, y1)), Some(Delta::Point(x2, y2))) => {
						*d1 = Some(Delta::Point(add(x1, *x2), add(y1, *y2)));
					}
					_ => return Err(Error::value("cannot sum gvar deltas with inferred points")),
				}
			}
		} else {
			for (d1, d2) in self.coordinates.iter_mut().zip(&other.coordinates) {
				match (*d1, d2) {
					(Some(Delta::Value(x)), Some(Delta::Value(y))) => {
						*d1 = Some(Delta::Value(add(x, *y)));
					}
					(None, Some(d)) => *d1 = Some(*d),
					_ => {}
				}
			}
		}
		Ok(())
	}

	/// `toXML`.
	pub fn to_xml(&self, writer: &mut XmlWriter, axis_tags: &[String]) {
		writer.begintag("tuple", &[]);
		writer.newline();
		for axis in axis_tags {
			let Some(&(min, peak, max)) = self.axes.get(axis) else {
				continue;
			};
			let (dmin, dmax) = infer_region(peak);
			let s = |v: f64| float_to_fixed_to_str(v, 14);
			if min == dmin && max == dmax {
				writer.simpletag("coord", &[("axis", axis), ("value", &s(peak))]);
			} else {
				writer.simpletag_ordered(
					"coord",
					&[
						("axis", axis),
						("min", &s(min)),
						("value", &s(peak)),
						("max", &s(max)),
					],
				);
			}
			writer.newline();
		}
		let mut wrote = false;
		for (i, d) in self.coordinates.iter().enumerate() {
			match d {
				Some(Delta::Point(x, y)) => {
					writer.simpletag("delta", &[("pt", &i), ("x", &x.repr()), ("y", &y.repr())]);
				}
				Some(Delta::Value(Number::Int(v))) => {
					writer.simpletag("delta", &[("cvt", &i), ("value", v)]);
				}
				Some(Delta::Value(Number::Float(_))) => {
					log::error!("bad delta format");
					writer.comment(&format!("bad delta #{i}"));
				}
				None => continue,
			}
			writer.newline();
			wrote = true;
		}
		if !wrote {
			writer.comment("no deltas");
			writer.newline();
		}
		writer.endtag("tuple");
		writer.newline();
	}

	/// `fromXML` for one child of a `tuple` element.
	pub fn from_xml(&mut self, e: &Element) -> Result<()> {
		let attr = |name: &str| {
			e.attr(name)
				.ok_or_else(|| Error::ttx(format!("KeyError: {name:?}")))
		};
		let fixed = |s: &str| {
			str_to_fixed_to_float(s, 14)
				.ok_or_else(|| Error::value(format!("could not convert string to float: {s:?}")))
		};
		let number = |s: &str| -> Result<Number> {
			match safe_eval(s) {
				Some(Literal::Int(i)) => i64::try_from(i)
					.map(Number::Int)
					.map_err(|_| Error::Struct(format!("value {i} out of range"))),
				Some(Literal::Bool(b)) => Ok(Number::Int(b as i64)),
				Some(Literal::Float(f)) => Ok(Number::Float(f)),
				_ => Err(Error::value(format!("malformed node or string: {s:?}"))),
			}
		};
		match e.name.as_str() {
			"coord" => {
				let peak = fixed(attr("value")?)?;
				let (dmin, dmax) = infer_region(peak);
				let min = match e.attr("min") {
					Some(s) => fixed(s)?,
					None => float_to_fixed_to_float(dmin, 14),
				};
				let max = match e.attr("max") {
					Some(s) => fixed(s)?,
					None => float_to_fixed_to_float(dmax, 14),
				};
				self.axes.insert(attr("axis")?.to_owned(), (min, peak, max));
			}
			"delta" => {
				let (index, delta) = if e.attr("pt").is_some() {
					let index = number(attr("pt")?)?;
					(
						index,
						Delta::Point(number(attr("x")?)?, number(attr("y")?)?),
					)
				} else if e.attr("cvt").is_some() {
					(number(attr("cvt")?)?, Delta::Value(number(attr("value")?)?))
				} else {
					let mut keys: Vec<&str> = e.attrs.iter().map(|(k, _)| k.as_str()).collect();
					keys.sort();
					log::warn!("bad delta format: {}", keys.join(", "));
					return Ok(());
				};
				let Number::Int(index) = index else {
					return Err(Error::ttlib("TypeError: list indices must be integers"));
				};
				// Python indexing: negative indices count from the end.
				let n = self.coordinates.len() as i64;
				let at = if index < 0 { index + n } else { index };
				let slot = usize::try_from(at)
					.ok()
					.and_then(|at| self.coordinates.get_mut(at))
					.ok_or_else(|| {
						Error::ttlib("IndexError: list assignment index out of range")
					})?;
				*slot = Some(delta);
			}
			_ => {}
		}
		Ok(())
	}
}

/// Reads the peak of each axis (`decompileCoord_`), in the order of `axis_tags`.
/// Returns the peaks and the offset after them.
pub fn decompile_coord(
	axis_tags: &[String],
	data: &[u8],
	offset: usize,
) -> Result<(Vec<f64>, usize)> {
	let mut pos = offset;
	let mut coord = Vec::with_capacity(axis_tags.len());
	for _ in axis_tags {
		let b = data
			.get(pos..pos + 2)
			.ok_or_else(|| Error::Struct("unpack requires a buffer of 2 bytes".into()))?;
		coord.push(fixed_to_float(i16::from_be_bytes([b[0], b[1]]) as i64, 14));
		pos += 2;
	}
	Ok((coord, pos))
}

/// Reads the shared tuples of `gvar` (`decompileSharedTuples`).
pub fn decompile_shared_tuples(
	axis_tags: &[String],
	count: usize,
	data: &[u8],
	offset: usize,
) -> Result<Vec<Vec<f64>>> {
	let mut pos = offset;
	let mut result = Vec::with_capacity(count.min(4096));
	for _ in 0..count {
		let (t, next) = decompile_coord(axis_tags, data, pos)?;
		result.push(t);
		pos = next;
	}
	Ok(result)
}

/// The peaks used more than once, most used first, at most 4096
/// (`compileSharedTuples`). The variations come in their dict order, which
/// decides between peaks used equally often at the cut.
pub fn compile_shared_tuples<'a>(
	axis_tags: &[String],
	variations: impl IntoIterator<Item = &'a TupleVariation>,
) -> Result<Vec<Vec<u8>>> {
	let mut counts: Vec<(Vec<u8>, usize)> = Vec::new();
	let mut index: HashMap<Vec<u8>, usize> = HashMap::new();
	for v in variations {
		let coord = v.compile_coord(axis_tags)?;
		match index.get(&coord) {
			Some(&i) => counts[i].1 += 1,
			None => {
				index.insert(coord.clone(), counts.len());
				counts.push((coord, 1));
			}
		}
	}
	// `most_common(4096)`: by count, then first seen; then sorted by count and bytes.
	counts.sort_by_key(|c| std::cmp::Reverse(c.1));
	counts.truncate(TUPLE_INDEX_MASK as usize + 1);
	counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
	Ok(counts
		.into_iter()
		.filter(|c| c.1 > 1)
		.map(|c| c.0)
		.collect())
}

/// Packs point numbers as delta-encoded runs (`compilePoints`); an empty list
/// means all points.
pub fn compile_points(points: &[usize]) -> Vec<u8> {
	if points.is_empty() {
		return vec![0];
	}
	let mut points = points.to_vec();
	points.sort_unstable();
	let n = points.len();
	let mut out = Vec::new();
	if n < 0x80 {
		out.push(n as u8);
	} else {
		out.push(((n >> 8) | 0x80) as u8);
		out.push((n & 0xFF) as u8);
	}
	let mut pos = 0;
	let mut last = 0i64;
	while pos < n {
		let mut run = 0;
		let header = out.len();
		out.push(0);
		let mut bytes: Option<bool> = None;
		while pos < n && run <= 127 {
			let cur = points[pos] as i64;
			let delta = cur - last;
			let byte = *bytes.get_or_insert((0..=0xFF).contains(&delta));
			if byte && !(0..=0xFF).contains(&delta) {
				break;
			}
			if byte {
				out.push(delta as u8);
			} else {
				out.push(((delta >> 8) & 0xFF) as u8);
				out.push((delta & 0xFF) as u8);
			}
			last = cur;
			pos += 1;
			run += 1;
		}
		out[header] = if bytes == Some(true) {
			(run - 1) as u8
		} else {
			((run - 1) as u8) | POINTS_ARE_WORDS
		};
	}
	out
}

/// Point numbers read from data: all points, or a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Points {
	/// All points (`range(numPoints)`).
	All(usize),
	/// The listed points.
	List(Vec<i64>),
}

impl Points {
	fn len(&self) -> usize {
		match self {
			Points::All(n) => *n,
			Points::List(l) => l.len(),
		}
	}

	fn get(&self, i: usize) -> i64 {
		match self {
			Points::All(_) => i as i64,
			Points::List(l) => l[i],
		}
	}
}

/// Reads point numbers (`decompilePoints_`).
pub fn decompile_points(
	num_points: usize,
	data: &[u8],
	offset: usize,
	table: &str,
) -> Result<(Points, usize)> {
	let index = || Error::ttlib("IndexError: index out of range");
	let mut pos = offset;
	let mut count = *data.get(pos).ok_or_else(index)? as usize;
	pos += 1;
	if count & POINTS_ARE_WORDS as usize != 0 {
		count = (count & POINT_RUN_COUNT_MASK as usize) << 8
			| *data.get(pos).ok_or_else(index)? as usize;
		pos += 1;
	}
	if count == 0 {
		return Ok((Points::All(num_points), pos));
	}
	let mut deltas: Vec<i64> = Vec::new();
	while deltas.len() < count {
		let header = *data.get(pos).ok_or_else(index)?;
		pos += 1;
		let run = (header & POINT_RUN_COUNT_MASK) as usize + 1;
		let words = header & POINTS_ARE_WORDS != 0;
		let size = if words { run * 2 } else { run };
		let start = pos.min(data.len());
		let bytes = &data[start..(pos + size).min(data.len())];
		if words && !bytes.len().is_multiple_of(2) {
			return Err(Error::value("bytes length not a multiple of item size"));
		}
		let got = if words { bytes.len() / 2 } else { bytes.len() };
		if got != run {
			return Err(Error::Assertion(format!(
				"len(points) == numPointsInRun ({got}, {run})"
			)));
		}
		if words {
			deltas.extend(
				bytes
					.as_chunks::<2>()
					.0
					.iter()
					.map(|&c| u16::from_be_bytes(c) as i64),
			);
		} else {
			deltas.extend(bytes.iter().map(|&b| b as i64));
		}
		pos += size;
	}
	let mut current = 0;
	let points: Vec<i64> = deltas
		.into_iter()
		.map(|d| {
			current += d;
			current
		})
		.collect();
	let mut bad: Vec<String> = points
		.iter()
		.filter(|&&p| p < 0 || p >= num_points as i64)
		.map(i64::to_string)
		.collect::<std::collections::BTreeSet<_>>()
		.into_iter()
		.collect();
	if !bad.is_empty() {
		bad.sort();
		log::warn!("point {} out of range in '{table}' table", bad.join(","));
	}
	Ok((Points::List(points), pos))
}

/// Packs tuple variations (`compileTupleVariationStore`): the tuple
/// variation count (with [`TUPLES_SHARE_POINT_NUMBERS`]), the tuple headers and
/// the data.
pub fn compile_tuple_variation_store(
	variations: &[TupleVariation],
	axis_tags: &[String],
	shared_coords: &HashMap<Vec<u8>, usize>,
	use_shared_points: bool,
	optimize_size: bool,
) -> Result<(u32, Vec<u8>, Vec<u8>)> {
	// Point sets in order of first use, with their counts.
	let mut point_sets: Vec<(Vec<usize>, usize)> = Vec::new();
	let mut used = Vec::new();
	for v in variations {
		let Some(points) = v.used_points() else {
			continue;
		};
		match point_sets.iter_mut().find(|(p, _)| *p == points) {
			Some(entry) => entry.1 += 1,
			None => point_sets.push((points.clone(), 1)),
		}
		used.push((v, points));
	}
	if used.is_empty() {
		return Ok((0, Vec::new(), Vec::new()));
	}
	let n = used[0].0.coordinates.len();
	if used.iter().any(|(v, _)| v.coordinates.len() != n) {
		return Err(Error::Assertion(
			"Variation sets have different sizes".into(),
		));
	}
	let compiled: Vec<Vec<u8>> = point_sets.iter().map(|(p, _)| compile_points(p)).collect();
	let mut count = used.len() as u32;
	let mut data = Vec::new();
	let mut shared: Option<&Vec<usize>> = None;
	if use_shared_points {
		// The point set that saves the most bytes; the first one on ties.
		let mut best = 0;
		for (i, (_, c)) in point_sets.iter().enumerate() {
			let key = |i: usize, c: usize| compiled[i].len() * (c - 1);
			if key(i, *c) > key(best, point_sets[best].1) {
				best = i;
			}
		}
		shared = Some(&point_sets[best].0);
		data.extend_from_slice(&compiled[best]);
		count |= TUPLES_SHARE_POINT_NUMBERS as u32;
	}
	let mut tuples = Vec::new();
	for (v, points) in &used {
		let point_data: &[u8] = if Some(points) == shared {
			&[]
		} else {
			let i = point_sets
				.iter()
				.position(|(p, _)| p == points)
				.unwrap_or(0);
			&compiled[i]
		};
		let (t, d) = v.compile(axis_tags, shared_coords, point_data, optimize_size)?;
		tuples.extend(t);
		data.extend(d);
	}
	Ok((count, tuples, data))
}

/// The size of a tuple variation header with `flags` (`getTupleSize_`).
pub fn tuple_size(flags: u16, axis_count: usize) -> usize {
	let mut size = 4;
	if flags & EMBEDDED_PEAK_TUPLE != 0 {
		size += axis_count * 2;
	}
	if flags & INTERMEDIATE_REGION != 0 {
		size += axis_count * 4;
	}
	size
}

/// Reads tuple variations (`decompileTupleVariationStore`). `shared_tuples` is
/// `None` for `cvar`.
#[allow(clippy::too_many_arguments)]
pub fn decompile_tuple_variation_store(
	table: &str,
	axis_tags: &[String],
	count: u16,
	point_count: usize,
	shared_tuples: Option<&[Vec<f64>]>,
	data: &[u8],
	mut pos: usize,
	mut data_pos: usize,
) -> Result<Vec<TupleVariation>> {
	let shared_points = if count & TUPLES_SHARE_POINT_NUMBERS != 0 {
		let (p, next) = decompile_points(point_count, data, data_pos, table)?;
		data_pos = next;
		p
	} else {
		Points::List(Vec::new())
	};
	let num_axes = axis_tags.len();
	let mut result = Vec::new();
	for _ in 0..count & TUPLE_COUNT_MASK {
		let header = data
			.get(pos..pos + 4)
			.ok_or_else(|| Error::Struct("unpack requires a buffer of 4 bytes".into()))?;
		let size = u16::from_be_bytes([header[0], header[1]]) as usize;
		let flags = u16::from_be_bytes([header[2], header[3]]);
		let tuple_size = tuple_size(flags, num_axes);
		let slice = |from: usize, len: usize| {
			let from = from.min(data.len());
			&data[from..(from + len).min(data.len())]
		};
		let tuple_data = slice(pos, tuple_size);
		let delta_data = slice(data_pos, size);
		result.push(decompile_tuple_variation(
			point_count,
			shared_tuples,
			&shared_points,
			table,
			axis_tags,
			tuple_data,
			delta_data,
		)?);
		pos += tuple_size;
		data_pos += size;
	}
	Ok(result)
}

/// `decompileTupleVariation_`.
fn decompile_tuple_variation(
	point_count: usize,
	shared_tuples: Option<&[Vec<f64>]>,
	shared_points: &Points,
	table: &str,
	axis_tags: &[String],
	data: &[u8],
	delta_data: &[u8],
) -> Result<TupleVariation> {
	let b = data
		.get(2..4)
		.ok_or_else(|| Error::Struct("unpack requires a buffer of 2 bytes".into()))?;
	let flags = u16::from_be_bytes([b[0], b[1]]);
	let mut pos = 4;
	let peak = if flags & EMBEDDED_PEAK_TUPLE == 0 {
		let shared = shared_tuples
			.ok_or_else(|| Error::ttlib("TypeError: 'NoneType' object is not subscriptable"))?;
		shared
			.get((flags & TUPLE_INDEX_MASK) as usize)
			.cloned()
			.ok_or_else(|| Error::ttlib("IndexError: list index out of range"))?
	} else {
		let (p, next) = decompile_coord(axis_tags, data, pos)?;
		pos = next;
		p
	};
	let (start, end) = if flags & INTERMEDIATE_REGION != 0 {
		let (s, next) = decompile_coord(axis_tags, data, pos)?;
		let (e, _) = decompile_coord(axis_tags, data, next)?;
		(s, e)
	} else {
		peak.iter().map(|&p| infer_region(p)).unzip()
	};
	let mut axes = BTreeMap::new();
	for (i, axis) in axis_tags.iter().enumerate() {
		let region = (start[i], peak[i], end[i]);
		if region != (0.0, 0.0, 0.0) {
			axes.insert(axis.clone(), region);
		}
	}
	let mut pos = 0;
	let private;
	let points = if flags & PRIVATE_POINT_NUMBERS != 0 {
		let (p, next) = decompile_points(point_count, delta_data, pos, table)?;
		pos = next;
		private = p;
		&private
	} else {
		shared_points
	};
	let mut coordinates = vec![None; point_count];
	let in_range = |p: i64| p >= 0 && (p as usize) < point_count;
	if table == "cvar" {
		let (values, _) = decompile_deltas(Some(points.len()), delta_data, pos)?;
		for (i, v) in values.into_iter().enumerate() {
			let p = points.get(i);
			if in_range(p) {
				coordinates[p as usize] = Some(Delta::Value(Number::Int(v)));
			}
		}
	} else {
		let (xs, next) = decompile_deltas(Some(points.len()), delta_data, pos)?;
		let (ys, _) = decompile_deltas(Some(points.len()), delta_data, next)?;
		for (i, (x, y)) in xs.into_iter().zip(ys).enumerate() {
			let p = points.get(i);
			if in_range(p) {
				coordinates[p as usize] = Some(Delta::Point(Number::Int(x), Number::Int(y)));
			}
		}
	}
	Ok(TupleVariation { axes, coordinates })
}

#[cfg(test)]
mod tests {
	use super::*;

	fn hex(data: &[u8]) -> String {
		data.iter()
			.map(|b| format!("{b:02X}"))
			.collect::<Vec<_>>()
			.join(" ")
	}

	// Cases from fontTools' TupleVariation_test.py.
	#[test]
	fn compile_deltas() -> Result<()> {
		let c = |d: &[i64]| compile_delta_values(d, true).map(|v| hex(&v)).ok();
		assert_eq!(c(&[]).as_deref(), Some(""));
		assert_eq!(c(&[0]).as_deref(), Some("80"));
		assert_eq!(c(&[0; 64]).as_deref(), Some("BF"));
		assert_eq!(c(&[0; 65]).as_deref(), Some("BF 80"));
		assert_eq!(c(&[0; 130]).as_deref(), Some("BF BF 81"));
		assert_eq!(
			c(&[15, 15, 0, 15, 15]).as_deref(),
			Some("04 0F 0F 00 0F 0F")
		);
		assert_eq!(
			c(&[15, 15, 0, 0, 15, 15]).as_deref(),
			Some("01 0F 0F 81 01 0F 0F")
		);
		assert_eq!(
			c(&[0x6666, 0, 0x7777]).as_deref(),
			Some("40 66 66 80 40 77 77")
		);
		assert_eq!(
			c(&[0x6666, 2, 0x7777]).as_deref(),
			Some("42 66 66 00 02 77 77")
		);
		assert_eq!(
			c(&[0x6666, 2, 3, 0x7777]).as_deref(),
			Some("40 66 66 01 02 03 40 77 77")
		);
		assert_eq!(
			c(&[0x123456, 0x7777]).as_deref(),
			Some("C0 00 12 34 56 40 77 77")
		);
		let speed = |d: &[i64]| compile_delta_values(d, false).map(|v| hex(&v)).ok();
		assert_eq!(
			speed(&[15, 15, 0, 0, 15, 15]).as_deref(),
			Some("05 0F 0F 00 00 0F 0F")
		);
		assert_eq!(
			speed(&[0x6666, 0, 0x7777]).as_deref(),
			Some("42 66 66 00 00 77 77")
		);
		Ok(())
	}

	#[test]
	fn decompile_round_trips() -> Result<()> {
		for deltas in [
			vec![],
			vec![0; 130],
			vec![15, 15, 0, 0, 15, 15],
			vec![0x6666, 2, 3, 0x7777, -0x123456],
		] {
			let data = compile_delta_values(&deltas, true)?;
			assert_eq!(
				decompile_deltas(Some(deltas.len()), &data, 0)?,
				(deltas.clone(), data.len())
			);
			assert_eq!(decompile_deltas(None, &data, 0)?.0, deltas);
		}
		assert!(decompile_deltas(Some(2), &[0x01, 0x05], 0).is_err());
		Ok(())
	}
}
