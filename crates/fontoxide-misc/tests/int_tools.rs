//! The doctest of `fontTools.misc.intTools.bit_indices`.

use fontoxide_misc::int_tools::{bit_count, bit_indices};

#[test]
fn doctest_bit_indices() {
	assert_eq!(bit_indices(0b101), [0, 2]);
	assert_eq!(bit_indices(0), [0u32; 0]);
	assert_eq!(bit_indices(1 << 100), [100]);
	assert_eq!(bit_count(0b1011), 3);
}
