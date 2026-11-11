//! The `fontTools.varLib.models` helpers that designspaceLib uses, on Python
//! numbers.
//!
//! They are repeated here rather than taken from `fontoxide-varlib` because
//! varLib depends on designspaceLib, not the other way round.

use crate::number::Number;
use crate::{DesignSpaceDocumentError, Result};

/// `piecewiseLinearMap(v, dict(mapping))`; `mapping` has no repeated inputs.
pub fn piecewise_linear_map(v: Number, mapping: &[(Number, Number)]) -> Number {
	if mapping.is_empty() {
		return v;
	}
	if let Some(&(_, to)) = mapping.iter().find(|(from, _)| *from == v) {
		return to;
	}
	let pick = |better: fn(&Number, &Number) -> bool, keep: &dyn Fn(Number) -> bool| {
		mapping
			.iter()
			.copied()
			.filter(|(k, _)| keep(*k))
			.reduce(|a, b| if better(&b.0, &a.0) { b } else { a })
	};
	let lt = |a: &Number, b: &Number| a < b;
	let gt = |a: &Number, b: &Number| a > b;
	if let Some((k, to)) = pick(lt, &|_| true)
		&& v < k
	{
		return v + to - k;
	}
	if let Some((k, to)) = pick(gt, &|_| true)
		&& v > k
	{
		return v + to - k;
	}
	// Interpolate.
	match (pick(gt, &|k| k < v), pick(lt, &|k| k > v)) {
		(Some((a, va)), Some((b, vb))) => va + (vb - va) * (v - a) / (b - a),
		// NaN compares false everywhere.
		_ => v,
	}
}

fn fmt3(v: Number) -> String {
	format!("{:.3}", v.to_f64())
}

/// `normalizeValue(v, triple, extrapolate, allow_decreasing=...)`.
pub fn normalize_value(
	v: Number,
	triple: (Number, Number, Number),
	extrapolate: bool,
	allow_decreasing: bool,
) -> Result<Number> {
	let (lower, default, upper) = triple;
	let decreasing = allow_decreasing && lower > upper;
	let valid = if decreasing {
		lower >= default && default >= upper
	} else {
		lower <= default && default <= upper
	};
	if !valid {
		return Err(DesignSpaceDocumentError::Value(format!(
			"Invalid axis values, must be minimum, default, maximum: {}, {}, {}",
			fmt3(lower),
			fmt3(default),
			fmt3(upper)
		)));
	}
	if decreasing {
		let normalized = normalize_value(v, (upper, default, lower), extrapolate, false)?;
		return Ok(if normalized != Number::Int(0) {
			-normalized
		} else {
			Number::Float(0.0)
		});
	}
	let v = if extrapolate {
		v
	} else {
		v.py_min(upper).py_max(lower)
	};
	// Python: `max(min(v, upper), lower)`.
	if v == default || lower == upper {
		return Ok(Number::Float(0.0));
	}
	if (v < default && lower != default) || (v > default && upper == default) {
		Ok((v - default) / (default - lower))
	} else {
		Ok((v - default) / (upper - default))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn n(v: i64) -> Number {
		Number::Int(v)
	}

	#[test]
	fn normalize_value_doctests() -> Result<()> {
		let t = (n(100), n(400), n(900));
		assert_eq!(normalize_value(n(400), t, false, false)?.to_string(), "0.0");
		assert_eq!(
			normalize_value(n(100), t, false, false)?.to_string(),
			"-1.0"
		);
		assert_eq!(normalize_value(n(650), t, false, false)?.to_string(), "0.5");
		let t = (n(38), n(28), n(23));
		assert_eq!(normalize_value(n(38), t, false, true)?.to_string(), "-1.0");
		assert_eq!(normalize_value(n(33), t, false, true)?.to_string(), "-0.5");
		assert_eq!(normalize_value(n(23), t, false, true)?.to_string(), "1.0");
		Ok(())
	}

	#[test]
	fn piecewise() {
		let m = [(n(0), n(0)), (n(1000), n(500))];
		assert_eq!(piecewise_linear_map(n(-10), &m).to_string(), "-10");
		assert_eq!(piecewise_linear_map(n(500), &m).to_string(), "250.0");
		assert_eq!(piecewise_linear_map(n(1000), &m).to_string(), "500");
	}
}
