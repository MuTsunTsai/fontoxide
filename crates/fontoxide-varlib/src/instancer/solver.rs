//! Port of `fontTools.varLib.instancer.solver`: how to represent a tent under
//! new axis limits.

use fontoxide_ttlib::{Error, Result};

use super::limits::NormalizedAxisTriple;

const EPSILON: f64 = 1.0 / (1 << 14) as f64;

/// A region on an axis: lower, peak and upper.
pub type Tent = (f64, f64, f64);

/// `supportScalar({"tag": v}, {"tag": tent})`.
fn scalar_at(v: f64, (lower, peak, upper): Tent) -> f64 {
	if peak == 0.0 || lower > peak || peak > upper || (lower < 0.0 && upper > 0.0) {
		return 1.0;
	}
	if v == peak {
		return 1.0;
	}
	if v <= lower || upper <= v {
		return 0.0;
	}
	if v < peak {
		(v - lower) / (peak - lower)
	} else {
		(upper - v) / (upper - peak)
	}
}

fn reverse_negate(v: Tent) -> Tent {
	(-v.2, -v.1, -v.0)
}

/// `_solve`.
fn solve(tent: Tent, limit: NormalizedAxisTriple) -> Vec<(f64, Option<Tent>)> {
	let (axis_min, axis_def, axis_max) = (limit.minimum, limit.default, limit.maximum);
	let (mut lower, peak, mut upper) = tent;

	// Mirror the problem such that axisDef <= peak.
	if axis_def > peak {
		return solve(reverse_negate(tent), limit.reverse_negate())
			.into_iter()
			.map(|(scalar, t)| (scalar, t.map(reverse_negate)))
			.collect();
	}

	// Case 1: the whole deltaset falls outside the new limit; we can drop it.
	if axis_max <= lower && axis_max < peak {
		return Vec::new();
	}

	// Case 2: only the peak and outermost bound fall outside the new limit; we
	// keep the deltaset, update peak and outermost bound, scale the deltas by the
	// scalar value at the new limit, and solve recursively.
	if axis_max < peak {
		let mult = scalar_at(axis_max, tent);
		let tent = (lower, axis_max, axis_max);
		return solve(tent, limit)
			.into_iter()
			.map(|(scalar, t)| (scalar * mult, t))
			.collect();
	}

	// lower <= axisDef <= peak <= axisMax

	let gain = scalar_at(axis_def, tent);
	let mut out = vec![(gain, None)];

	// First, the positive side.

	// outGain is the scalar of axisMax at the tent.
	let out_gain = scalar_at(axis_max, tent);

	if gain >= out_gain {
		// Case 3a: gain is more than outGain. The tent down-slope crosses the axis
		// into negative, and has to be split. This is also the branch taken if both
		// gain and outGain are 0.

		// Crossing point on the axis.
		let crossing = peak + (1.0 - gain) * (upper - peak);

		// The part before the crossing point.
		out.push((1.0 - gain, Some((lower.max(axis_def), peak, crossing))));

		// The part after the crossing point may use one or two tents, depending on
		// whether upper is before axisMax or not.
		if upper >= axis_max {
			// Case 3a1: just one tent needed.
			out.push((out_gain - gain, Some((crossing, axis_max, axis_max))));
		} else {
			// Case 3a2: two tents needed, to keep down to eternity.

			// A tent's peak cannot fall on axis default. Nudge it.
			if upper == axis_def {
				upper += EPSILON;
			}
			// Downslope.
			out.push((0.0 - gain, Some((crossing, upper, axis_max))));
			// Eternity justify.
			out.push((0.0 - gain, Some((upper, axis_max, axis_max))));
		}
	} else {
		// Case 4: the new limit doesn't fit; we need to chop into two tents, because
		// a triangle with part of one side cut off cannot be a triangle itself.
		// (fontTools' case 3, which scales the axis upper, is disabled because OTS
		// doesn't accept the result.)
		out.push((1.0 - gain, Some((axis_def.max(lower), peak, axis_max))));
		// Don't add a dirac delta!
		if peak < axis_max {
			out.push((out_gain - gain, Some((peak, axis_max, axis_max))));
		}
	}

	// Now, the negative side.

	if lower <= axis_min {
		// Case 1neg: lower extends beyond axisMin: we chop. Simple.
		let scalar = scalar_at(axis_min, tent);
		out.push((scalar - gain, Some((axis_min, axis_min, axis_def))));
	} else {
		// Case 2neg: lower is between axisMin and axisDef: we add two tents to keep
		// it down all the way to eternity.

		// A tent's peak cannot fall on axis default. Nudge it.
		if lower == axis_def {
			lower -= EPSILON;
		}
		// Downslope.
		out.push((0.0 - gain, Some((axis_min, lower, axis_def))));
		// Eternity justify.
		out.push((0.0 - gain, Some((axis_min, axis_min, lower))));
	}

	out
}

/// `rebaseTent`: how to represent a tent under new axis limits, in normalized
/// coordinates. The result has a scalar to multiply the deltas of a variation by,
/// and its new tent, or `None` for one that is always enabled (the "gain").
pub fn rebase_tent(tent: Tent, limit: NormalizedAxisTriple) -> Result<Vec<(f64, Option<Tent>)>> {
	let (axis_min, axis_def, axis_max) = (limit.minimum, limit.default, limit.maximum);
	if !(-1.0 <= axis_min && axis_min <= axis_def && axis_def <= axis_max && axis_max <= 1.0) {
		return Err(Error::Assertion(
			"-1 <= axisMin <= axisDef <= axisMax <= +1".into(),
		));
	}
	let (lower, peak, upper) = tent;
	if !(-2.0 <= lower && lower <= peak && peak <= upper && upper <= 2.0) {
		return Err(Error::Assertion(
			"-2 <= lower <= peak <= upper <= +2".into(),
		));
	}
	if peak == 0.0 {
		return Err(Error::Assertion("peak != 0".into()));
	}
	let n = |v: f64| limit.renormalize_value(v);
	Ok(solve(tent, limit)
		.into_iter()
		.filter(|(scalar, _)| *scalar != 0.0)
		.map(|(scalar, v)| (scalar, v.map(|v| (n(v.0), n(v.1), n(v.2)))))
		.collect())
}
