//! `Tests/cffLib/privateDict_test.py`.
//!
//! The Private DICT is decompiled with [`DictDecompiler`] and its blended
//! `BlueValues` compiled with [`encode_arg_delta`], which is what
//! `PrivateDict.decompile` and the compiler of a Private DICT holding only
//! `BlueValues` run.

use fontoxide_ttlib::cff::charstring::encode_int_cff;
use fontoxide_ttlib::cff::dict::{DictDecompiler, Value, encode_arg_delta};
use fontoxide_ttlib::cff::font::{DictKind, dict_table};
use fontoxide_ttlib::{Error, Result};

/// `maxStackLimit`.
const MAX_STACK: i64 = 513;

/// `encodeNumber` of integers.
fn encode(values: &[i64]) -> Vec<u8> {
	let mut out = Vec::new();
	for &v in values {
		encode_int_cff(v, &mut out);
	}
	out
}

/// `PrivateDict(vstore=...).decompile(data)`, with a store of `num_regions`
/// regions; returns `BlueValues`.
fn decompile_blue_values(num_regions: i64, data: &[u8]) -> Result<Value> {
	let regions = |_: i64| -> Result<i64> { Ok(num_regions) };
	let entries =
		DictDecompiler::new(dict_table(DictKind::Private), None, Some(&regions)).decompile(data)?;
	entries
		.into_iter()
		.find(|(n, _)| *n == "BlueValues")
		.map(|(_, v)| v)
		.ok_or_else(|| Error::ttlib("no BlueValues"))
}

fn list(items: &[&[i64]]) -> Value {
	Value::List(
		items
			.iter()
			.map(|l| Value::List(l.iter().map(|&v| Value::Int(v)).collect()))
			.collect(),
	)
}

#[test]
fn test_private_dict_multiple_blend_groups() -> Result<()> {
	// Two delta-encoded values: 10, then 5 relative to the first. The empty
	// blend between them must leave the first result on the operand stack.
	let mut data = encode(&[10, 1, 1]);
	data.push(23);
	data.extend(encode(&[0]));
	data.push(23);
	data.extend(encode(&[5, 2, 1]));
	data.extend([23, 6]);
	assert!(decompile_blue_values(1, &data)?.py_eq(&list(&[&[10, 1], &[15, 2]])));
	Ok(())
}

/// The largest operand stack when a blend operator runs, as
/// `StackCheckingDecompiler` checks it.
fn max_stack_at_blend(data: &[u8], masters: i64) -> Result<usize> {
	let mut stack: Vec<i64> = Vec::new();
	let mut max = 0;
	let mut i = 0;
	while i < data.len() {
		let b0 = data[i];
		i += 1;
		match b0 {
			28 => {
				stack.push(i16::from_be_bytes([data[i], data[i + 1]]) as i64);
				i += 2;
			}
			29 => {
				stack.push(
					i32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as i64,
				);
				i += 4;
			}
			32..=246 => stack.push(b0 as i64 - 139),
			247..=250 => {
				stack.push((b0 as i64 - 247) * 256 + data[i] as i64 + 108);
				i += 1;
			}
			251..=254 => {
				stack.push(-(b0 as i64 - 251) * 256 - data[i] as i64 - 108);
				i += 1;
			}
			23 => {
				max = max.max(stack.len());
				let blends = stack.pop().ok_or_else(|| Error::ttlib("underflow"))?;
				let n = stack.len() - (blends * masters) as usize;
				stack.truncate(n);
				stack.extend(std::iter::repeat_n(0, blends as usize));
			}
			_ => stack.clear(),
		}
	}
	Ok(max)
}

#[test]
fn test_private_dict_split_blends_roundtrip() -> Result<()> {
	// At 250 regions the final groups must shrink to account for the results
	// of preceding groups still occupying the stack.
	for regions in [40i64, 250] {
		let expected = Value::List(
			(0..14)
				.map(|i| {
					let mut l = vec![Value::Int(-40 + 2 * i)];
					l.extend(std::iter::repeat_n(Value::Int(i + 1), regions as usize));
					Value::List(l)
				})
				.collect(),
		);
		let mut data = Vec::new();
		encode_arg_delta(&expected, MAX_STACK, &mut data)?;
		data.push(6); // BlueValues
		assert!(max_stack_at_blend(&data, regions + 1)? <= MAX_STACK as usize);
		assert!(decompile_blue_values(regions, &data)?.py_eq(&expected));
	}
	Ok(())
}

#[test]
fn test_private_dict_blend_preserves_constant_prefix() -> Result<()> {
	let mut data = encode(&[10, 5, 2, 1]);
	data.extend([23, 6]);
	assert!(decompile_blue_values(1, &data)?.py_eq(&list(&[&[10, 0], &[15, 2]])));
	Ok(())
}

#[test]
fn test_private_dict_blend_preserves_constant_suffix() -> Result<()> {
	let mut prefix = encode(&[10, 1, 1]);
	prefix.push(23);
	for zero_blend in [false, true] {
		let mut data = prefix.clone();
		data.extend(encode(&[5]));
		if zero_blend {
			data.extend(encode(&[0]));
			data.push(23);
		}
		data.push(6);
		assert!(decompile_blue_values(1, &data)?.py_eq(&list(&[&[10, 1], &[15, 0]])));
	}
	Ok(())
}

#[test]
fn test_private_dict_unencodable_blend_rejected() {
	let mut a = vec![Value::Int(-20)];
	a.extend(std::iter::repeat_n(Value::Int(0), 512));
	let mut b = vec![Value::Int(-10)];
	b.extend(std::iter::repeat_n(Value::Int(0), 512));
	let blue_values = Value::List(vec![Value::List(a), Value::List(b)]);
	let mut data = Vec::new();
	let err = encode_arg_delta(&blue_values, MAX_STACK, &mut data);
	assert!(matches!(err, Err(e) if e.to_string().contains("operand stack")));
}
