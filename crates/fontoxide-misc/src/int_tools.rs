//! Port of `fontTools.misc.intTools`: bit counting.

/// The number of 1 bits of the absolute value of an integer (`bit_count`,
/// `popCount`).
///
/// ```
/// use fontoxide_misc::int_tools::bit_count;
/// assert_eq!(bit_count(0b1011), 3);
/// assert_eq!(bit_count(-5), 2);
/// ```
pub fn bit_count(v: i128) -> u32 {
	v.unsigned_abs().count_ones()
}

/// The indices of the set bits of the absolute value of an integer, 0 being the
/// least significant bit (`bit_indices`).
///
/// ```
/// use fontoxide_misc::int_tools::bit_indices;
/// assert_eq!(bit_indices(0b101), [0, 2]);
/// ```
pub fn bit_indices(v: i128) -> Vec<u32> {
	let mut bits = v.unsigned_abs();
	let mut out = Vec::with_capacity(bits.count_ones() as usize);
	while bits != 0 {
		out.push(bits.trailing_zeros());
		bits &= bits - 1;
	}
	out
}
