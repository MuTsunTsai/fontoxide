//! Values propagated from parent to child tables while reading or writing
//! (`localState` of `OTTableReader` / `OTTableWriter`).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use fontoxide_misc::Tag;

use crate::{Error, Result};

/// A reference to a count that is written before it is known (`CountReference`).
///
/// fontTools points into the parent table's attribute dict; here the value lives in
/// a shared cell.
#[derive(Debug, Clone)]
pub struct CountRef(Rc<RefCell<CountCell>>);

#[derive(Debug)]
struct CountCell {
	value: Option<u64>,
	size: usize,
	name: &'static str,
}

impl CountRef {
	/// A new reference with an unknown value.
	pub fn new(name: &'static str, size: usize, value: Option<u64>) -> Self {
		CountRef(Rc::new(RefCell::new(CountCell { value, size, name })))
	}

	/// Sets the value; it must agree with any value set before (`setValue`).
	pub fn set_value(&self, value: u64) -> Result<()> {
		let mut c = self.0.borrow_mut();
		match c.value {
			None => {
				c.value = Some(value);
				Ok(())
			}
			Some(v) if v == value => Ok(()),
			Some(v) => Err(Error::Assertion(format!("({:?}, {v}, {value})", c.name))),
		}
	}

	/// The value, if set (`getValue`).
	pub fn value(&self) -> Option<u64> {
		self.0.borrow().value
	}

	/// The size in bytes.
	pub fn size(&self) -> usize {
		self.0.borrow().size
	}

	/// Sets the size in bytes.
	pub fn set_size(&self, size: usize) {
		self.0.borrow_mut().size = size;
	}

	/// The packed value, with an unset value written as 0 (`getCountData`).
	pub fn data(&self) -> Result<Vec<u8>> {
		let c = self.0.borrow();
		let v = c.value.unwrap_or(0);
		let too_big = || Error::Struct(format!("count {v} does not fit in {} bytes", c.size));
		Ok(match c.size {
			1 => vec![u8::try_from(v).map_err(|_| too_big())?],
			2 => u16::try_from(v)
				.map_err(|_| too_big())?
				.to_be_bytes()
				.to_vec(),
			3 => {
				if v >= 1 << 24 {
					return Err(too_big());
				}
				(v as u32).to_be_bytes()[1..].to_vec()
			}
			4 => u32::try_from(v)
				.map_err(|_| too_big())?
				.to_be_bytes()
				.to_vec(),
			s => return Err(Error::value(format!("invalid count size {s}"))),
		})
	}

	/// Whether two references are the same.
	pub fn ptr_eq(&self, other: &CountRef) -> bool {
		Rc::ptr_eq(&self.0, &other.0)
	}
}

/// A propagated value.
#[derive(Debug, Clone)]
pub enum StateValue {
	/// An integer (counts while reading, `FeatureTag`-like values).
	Int(i64),
	/// A count that is being written.
	Count(CountRef),
	/// A value record format (`ValueFormat1`, `ValueFormat2`).
	ValueFormat(u16),
	/// A tag.
	Tag(Option<Tag>),
	/// A list of tags (`FeatureTags`).
	Tags(Rc<Vec<Tag>>),
	/// The length of each item of a list (`AxisIndicesList` of `VARC`, which
	/// fontTools propagates as the table itself).
	Lens(Rc<Vec<usize>>),
}

impl StateValue {
	/// The integer value, if this is an integer or a count with a value.
	pub fn as_int(&self) -> Option<i64> {
		match self {
			StateValue::Int(i) => Some(*i),
			StateValue::Count(c) => c.value().map(|v| v as i64),
			StateValue::ValueFormat(f) => Some(*f as i64),
			_ => None,
		}
	}
}

/// A persistent map of propagated values; children share their parent's map until
/// they change it, as fontTools copies the dict on assignment.
#[derive(Debug, Clone, Default)]
pub struct State(BTreeMap<&'static str, StateValue>);

impl State {
	/// An empty state.
	pub fn new() -> Self {
		State(BTreeMap::new())
	}

	/// A value.
	pub fn get(&self, name: &str) -> Option<&StateValue> {
		self.0.get(name)
	}

	/// Whether a value is present.
	pub fn contains(&self, name: &str) -> bool {
		self.0.contains_key(name)
	}

	/// Sets a value.
	pub fn set(&mut self, name: &'static str, value: StateValue) {
		self.0.insert(name, value);
	}

	/// Removes a value.
	pub fn remove(&mut self, name: &str) {
		self.0.remove(name);
	}
}
