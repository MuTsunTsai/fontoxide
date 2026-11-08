//! Packed deltas of tuple variations (`fontTools.ttLib.tables.TupleVariation`),
//! shared by `gvar`, `cvar` and `VARC`.

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
