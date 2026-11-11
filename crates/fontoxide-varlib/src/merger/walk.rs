//! The generic part of `Merger`: `mergeThings`, `mergeObjects` and `mergeLists`,
//! for the static types of the Rust tables.

use std::fmt::Debug;

use fontoxide_misc::Tag;
use fontoxide_misc::py::{float_repr, str_repr};

use super::{MResult, Merger};
use crate::errors::MergeErrorKind;

/// An object that the merger walks: a table, a record, or a format-switching
/// table.
pub(crate) trait Walk {
	/// `type(self).__name__`.
	fn py_type(&self) -> &'static str;

	/// `sorted(vars(self).keys())`: the Python attributes. Attributes that
	/// fontTools only sets for some versions or flags are always listed.
	fn py_keys(&self) -> &'static [&'static str];

	/// The attributes in the order of the converters (`getConverters()`).
	#[allow(dead_code)]
	fn attr_names(&self) -> &'static [&'static str];

	/// Merges one attribute: `mergeThings(getattr(self, attr), [getattr(l, attr) ...])`,
	/// or the merger registered for the attribute.
	fn merge_attr(&mut self, m: &mut Merger<'_>, lst: &[&Self], attr: &str) -> MResult;

	/// `Merger.mergeObjects(self, lst, exclude)`: the attributes in sorted order.
	fn merge_object(&mut self, m: &mut Merger<'_>, lst: &[&Self], exclude: &[&str]) -> MResult;

	/// The rest of `Merger.mergeThings` once the types match: the merger
	/// registered for the type, or `mergeObjects`.
	fn merge_dispatch(&mut self, m: &mut Merger<'_>, lst: &[&Self]) -> MResult {
		self.merge_object(m, lst, &[])
	}

	/// The `Format` of a format-switching table.
	fn format_number(&self) -> Option<u16> {
		None
	}
}

/// Runs `f`, adding `.name` to the stack of a merge error
/// (`e.stack.append("." + key)`).
pub(crate) fn with_attr<T>(
	m: &mut Merger<'_>,
	name: &str,
	f: impl FnOnce(&mut Merger<'_>) -> MResult<T>,
) -> MResult<T> {
	f(m).map_err(|e| e.push_stack(format!(".{name}")))
}

/// Runs `f`, adding `[i]` to the stack of a merge error.
pub(crate) fn with_index<T>(
	m: &mut Merger<'_>,
	i: usize,
	f: impl FnOnce(&mut Merger<'_>) -> MResult<T>,
) -> MResult<T> {
	f(m).map_err(|e| e.push_stack(format!("[{i}]")))
}

/// `repr` of an integer.
///
/// LENIENT(L36): glyphs are compared by ID, and shown by ID in the messages, where
/// fontTools shows their names.
pub(crate) fn repr_int<T: ToString>(v: &T) -> String {
	v.to_string()
}

/// `repr` of a float.
pub(crate) fn repr_float(v: &f64) -> String {
	float_repr(*v)
}

/// `repr` of a bool.
#[allow(dead_code)]
pub(crate) fn repr_bool(v: &bool) -> String {
	if *v { "True" } else { "False" }.to_owned()
}

/// `repr` of a tag.
pub(crate) fn repr_tag(v: &Tag) -> String {
	str_repr(&v.to_string())
}

/// `repr` of a string.
#[allow(clippy::ptr_arg)]
pub(crate) fn repr_str(v: &String) -> String {
	str_repr(v)
}

/// `repr` of a count that may be unset.
pub(crate) fn repr_opt_int(v: &Option<u32>) -> String {
	v.map_or_else(|| "None".to_owned(), |v| v.to_string())
}

/// An approximate `repr` of other values.
pub(crate) fn repr_debug<T: Debug>(v: &T) -> String {
	format!("{v:?}")
}

/// `Merger.mergeThings` of values that must be equal (`allEqualTo(out, lst)`).
pub(crate) fn merge_eq<T: PartialEq>(
	m: &mut Merger<'_>,
	out: &T,
	lst: &[&T],
	repr: fn(&T) -> String,
) -> MResult {
	if lst.iter().all(|v| *v == out) {
		return Ok(());
	}
	Err(m.should_be_constant(
		repr(out),
		lst.iter().map(|v| (*v == out, repr(v))).collect(),
	))
}

/// `Merger.mergeLists` of values that must be equal.
pub(crate) fn merge_eq_list<T: PartialEq>(
	m: &mut Merger<'_>,
	out: &[T],
	lst: &[&Vec<T>],
	repr: fn(&T) -> String,
) -> MResult {
	check_lengths(m, out.len(), lst.iter().map(|l| l.len()))?;
	for (i, value) in out.iter().enumerate() {
		let values: Vec<&T> = lst.iter().map(|l| &l[i]).collect();
		with_index(m, i, |m| merge_eq(m, value, &values, repr))?;
	}
	Ok(())
}

/// The check of `Merger.mergeLists`: `LengthsDiffer`.
pub(crate) fn check_lengths(
	m: &mut Merger<'_>,
	expected: usize,
	got: impl Iterator<Item = usize>,
) -> MResult {
	let got: Vec<usize> = got.collect();
	if got.iter().all(|g| *g == expected) {
		return Ok(());
	}
	Err(m.merge_error(
		MergeErrorKind::LengthsDiffer,
		Some((
			expected.to_string(),
			got.iter()
				.map(|g| (*g == expected, g.to_string()))
				.collect(),
		)),
	))
}

/// `Merger.mergeThings` of an object: the type check, then its merger.
pub(crate) fn merge_struct<T: Walk>(m: &mut Merger<'_>, out: &mut T, lst: &[&T]) -> MResult {
	let ty = out.py_type();
	if lst.iter().any(|l| l.py_type() != ty) {
		return Err(m.merge_error(
			MergeErrorKind::MismatchedTypes,
			Some((
				str_repr(ty),
				lst.iter()
					.map(|l| (l.py_type() == ty, str_repr(l.py_type())))
					.collect(),
			)),
		));
	}
	out.merge_dispatch(m, lst)
}

/// `VariationMerger.mergeThings` (and `Merger.mergeThings`) of an object that may
/// be `None`: with a variation merger, the masters with `None` don't take part.
pub(crate) fn merge_opt<T: Walk>(
	m: &mut Merger<'_>,
	out: Option<&mut T>,
	lst: &[Option<&T>],
) -> MResult {
	if m.is_variation() && lst.iter().any(Option::is_none) {
		if lst.iter().all(Option::is_none) {
			if out.is_some() {
				return Err(m.found_a_none(lst.iter().map(|l| l.map(|t| t.py_type())).collect()));
			}
			return Ok(());
		}
		// Temporarily subset the masters to those whose values aren't None.
		let mask: Vec<bool> = lst.iter().map(Option::is_some).collect();
		let saved = m.push_sub_model(&mask)?;
		let present: Vec<&T> = lst.iter().flatten().copied().collect();
		merge_present(m, out, &present)?;
		m.pop_sub_model(saved);
		return Ok(());
	}
	match out {
		None => {
			if lst.iter().all(Option::is_none) {
				return Ok(());
			}
			Err(mismatched_none(
				m,
				lst.iter().map(|l| l.map(|t| t.py_type())),
			))
		}
		Some(out) => {
			if lst.iter().any(Option::is_none) {
				let ty = out.py_type();
				return Err(m.merge_error(
					MergeErrorKind::MismatchedTypes,
					Some((
						str_repr(ty),
						lst.iter()
							.map(|l| {
								let t = l.map_or("NoneType", |t| t.py_type());
								(t == ty, str_repr(t))
							})
							.collect(),
					)),
				));
			}
			let present: Vec<&T> = lst.iter().flatten().copied().collect();
			merge_struct(m, out, &present)
		}
	}
}

/// `Merger.mergeThings(out, lst)` once no value of `lst` is `None`.
fn merge_present<T: Walk>(m: &mut Merger<'_>, out: Option<&mut T>, lst: &[&T]) -> MResult {
	match out {
		Some(out) => merge_struct(m, out, lst),
		None => Err(mismatched_none(m, lst.iter().map(|l| Some(l.py_type())))),
	}
}

/// `MismatchedTypes` for `out` being `None`.
fn mismatched_none<'a>(
	m: &mut Merger<'_>,
	got: impl Iterator<Item = Option<&'a str>>,
) -> crate::errors::VarLibError {
	m.merge_error(
		MergeErrorKind::MismatchedTypes,
		Some((
			str_repr("NoneType"),
			got.map(|t| {
				let t = t.unwrap_or("NoneType");
				(t == "NoneType", str_repr(t))
			})
			.collect(),
		)),
	)
}

/// `Merger.mergeLists` of objects.
pub(crate) fn merge_list<T: Walk>(m: &mut Merger<'_>, out: &mut [T], lst: &[&Vec<T>]) -> MResult {
	check_lengths(m, out.len(), lst.iter().map(|l| l.len()))?;
	for (i, value) in out.iter_mut().enumerate() {
		let values: Vec<&T> = lst.iter().map(|l| &l[i]).collect();
		with_index(m, i, |m| merge_struct(m, value, &values))?;
	}
	Ok(())
}

/// `Merger.mergeLists` of objects that may be `None`.
pub(crate) fn merge_opt_list<T: Walk>(
	m: &mut Merger<'_>,
	out: &mut [Option<Box<T>>],
	lst: &[&Vec<Option<Box<T>>>],
) -> MResult {
	check_lengths(m, out.len(), lst.iter().map(|l| l.len()))?;
	for (i, value) in out.iter_mut().enumerate() {
		let values: Vec<Option<&T>> = lst.iter().map(|l| l[i].as_deref()).collect();
		with_index(m, i, |m| merge_opt(m, value.as_deref_mut(), &values))?;
	}
	Ok(())
}

/// `Merger.mergeLists` of unboxed objects that may be `None`.
pub(crate) fn merge_opt_plain_list<T: Walk>(
	m: &mut Merger<'_>,
	out: &mut [Option<T>],
	lst: &[&Vec<Option<T>>],
) -> MResult {
	check_lengths(m, out.len(), lst.iter().map(|l| l.len()))?;
	for (i, value) in out.iter_mut().enumerate() {
		let values: Vec<Option<&T>> = lst.iter().map(|l| l[i].as_ref()).collect();
		with_index(m, i, |m| merge_opt(m, value.as_mut(), &values))?;
	}
	Ok(())
}

/// The check of `mergeObjects` for format-switching tables: the formats have
/// different attributes (`KeysDiffer`) or differ (`ShouldBeConstant` of
/// `.Format`).
// LENIENT(L36): fontTools compares the attributes of every object, some of which only
// exist for some versions or flags (`KeysDiffer`); the Rust tables always have
// them, so only formats are compared here, before the other attributes.
pub(crate) fn check_same_format<T: Walk>(m: &mut Merger<'_>, out: &T, lst: &[&T]) -> MResult {
	let format = out.format_number();
	if lst.iter().all(|l| l.format_number() == format) {
		return Ok(());
	}
	let keys = out.py_keys();
	let keys_repr = |k: &[&str]| -> String {
		let items: Vec<String> = k.iter().map(|s| str_repr(s)).collect();
		format!("[{}]", items.join(", "))
	};
	if lst.iter().any(|l| l.py_keys() != keys) {
		return Err(m.merge_error(
			MergeErrorKind::KeysDiffer,
			Some((
				keys_repr(keys),
				lst.iter()
					.map(|l| (l.py_keys() == keys, keys_repr(l.py_keys())))
					.collect(),
			)),
		));
	}
	let repr = |f: Option<u16>| f.map_or_else(|| "None".to_owned(), |f| f.to_string());
	Err(m
		.should_be_constant(
			repr(format),
			lst.iter()
				.map(|l| (l.format_number() == format, repr(l.format_number())))
				.collect(),
		)
		.push_stack(".Format"))
}
