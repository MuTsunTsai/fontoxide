//! Port of `fontTools.designspaceLib.types`: ranges and regions of the
//! designspace.

use indexmap::IndexMap;

use crate::descriptors::{AxisSubset, SimpleLocation, VariableFontDescriptor};
use crate::document::DesignSpaceDocument;
use crate::number::Number;
use crate::{DesignSpaceDocumentError, Result};

/// `clamp(value, minimum, maximum)`: `min(max(value, minimum), maximum)`.
pub fn clamp(value: Number, minimum: Number, maximum: Number) -> Number {
	value.py_max(minimum).py_min(maximum)
}

/// `Range`: an inclusive range with a default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
	/// Inclusive minimum of the range (`minimum`).
	pub minimum: Number,
	/// Inclusive maximum of the range (`maximum`).
	pub maximum: Number,
	/// Default value (`default`).
	pub default: Number,
}

impl Range {
	/// `Range(minimum, maximum, default)`: the bounds are sorted and the
	/// default is clamped to them (`__post_init__`).
	pub fn new(
		minimum: impl Into<Number>,
		maximum: impl Into<Number>,
		default: impl Into<Number>,
	) -> Self {
		let (mut minimum, mut maximum) = (minimum.into(), maximum.into());
		// `sorted` is stable: equal bounds keep their order.
		if maximum < minimum {
			std::mem::swap(&mut minimum, &mut maximum);
		}
		Range {
			minimum,
			maximum,
			default: clamp(default.into(), minimum, maximum),
		}
	}

	/// `Range(minimum, maximum)`: the default is `0`, clamped.
	pub fn without_default(minimum: impl Into<Number>, maximum: impl Into<Number>) -> Self {
		Range::new(minimum, maximum, 0)
	}

	/// `value in range`.
	pub fn contains(&self, value: Number) -> bool {
		self.minimum <= value && value <= self.maximum
	}

	/// `other in range`, for a range.
	pub fn contains_range(&self, other: &Range) -> bool {
		self.minimum <= other.minimum && other.maximum <= self.maximum
	}

	/// `intersection`: the overlap of the ranges, with this range's default.
	pub fn intersection(&self, other: &Range) -> Option<Range> {
		if self.maximum < other.minimum || self.minimum > other.maximum {
			None
		} else {
			Some(Range::new(
				self.minimum.py_max(other.minimum),
				self.maximum.py_min(other.maximum),
				// We don't care about the default in this use-case
				self.default,
			))
		}
	}
}

/// A value of a [`Region`]: a range or a single value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RegionValue {
	/// A range of an axis.
	Range(Range),
	/// A single location on an axis.
	Value(Number),
}

impl RegionValue {
	/// `value in self` / `value == self` as `locationInRegion` tests it.
	fn matches(&self, value: Number) -> bool {
		match self {
			RegionValue::Value(v) => value == *v,
			RegionValue::Range(r) => r.contains(value),
		}
	}
}

/// `Region`: axis name to range or single value.
pub type Region = IndexMap<String, RegionValue>;

/// `ConditionSet`: axis name to range.
pub type ConditionSet = IndexMap<String, Range>;

/// `locationInRegion`.
pub fn location_in_region(location: &SimpleLocation, region: &Region) -> bool {
	location
		.iter()
		.all(|(name, value)| region.get(name).is_some_and(|r| r.matches(*value)))
}

/// `regionInRegion`.
pub fn region_in_region(region: &Region, super_region: &Region) -> bool {
	for (name, value) in region {
		let Some(super_value) = super_region.get(name) else {
			return false;
		};
		let inside = match (value, super_value) {
			(RegionValue::Value(v), RegionValue::Value(s)) => v == s,
			// A range is never equal to a number.
			(RegionValue::Range(_), RegionValue::Value(_)) => false,
			(RegionValue::Value(v), RegionValue::Range(s)) => s.contains(*v),
			(RegionValue::Range(r), RegionValue::Range(s)) => s.contains_range(r),
		};
		if !inside {
			return false;
		}
	}
	true
}

/// `userRegionToDesignRegion`.
pub fn user_region_to_design_region(
	doc: &DesignSpaceDocument,
	user_region: &Region,
) -> Result<Region> {
	let mut design_region = Region::new();
	for (name, value) in user_region {
		let axis = doc.get_axis(name).ok_or_else(|| {
			DesignSpaceDocumentError::Document(format!(
				"Cannot find axis named '{name}' for region."
			))
		})?;
		let design = match value {
			RegionValue::Value(v) => RegionValue::Value(axis.map_forward(*v)?),
			RegionValue::Range(r) => RegionValue::Range(Range::new(
				axis.map_forward(r.minimum)?,
				axis.map_forward(r.maximum)?,
				axis.map_forward(r.default)?,
			)),
		};
		design_region.insert(name.clone(), design);
	}
	Ok(design_region)
}

/// `getVFUserRegion`: the user-space region covered by a variable font.
///
/// Omitted bounds inherit the axis limits. An omitted default inherits the
/// axis default; inherited or explicit defaults are clamped to the subset.
/// Axes not listed in the axis subsets are fixed at their default location.
/// Value subsets resolve to a single location.
pub fn get_vf_user_region(
	doc: &DesignSpaceDocument,
	vf: &VariableFontDescriptor,
) -> Result<Region> {
	let mut vf_user_region = Region::new();
	// For each axis, 2 cases:
	//  - it has a range = it's an axis in the VF DS
	//  - it's a single location = use it to know which rules should apply in the VF
	for axis_subset in &vf.axis_subsets {
		let axis = doc.get_axis(axis_subset.name()).ok_or_else(|| {
			DesignSpaceDocumentError::Document(format!(
				"Cannot find axis named '{}' for variable font '{}'.",
				axis_subset.name(),
				vf.name
			))
		})?;
		let axis_name = axis.name_str();
		match axis_subset {
			AxisSubset::Range(subset) => {
				let Some(continuous) = axis.as_continuous() else {
					return Err(DesignSpaceDocumentError::Document(format!(
						"Cannot select a range over '{axis_name}' for variable font '{}' because it's a discrete axis, use only 'userValue' instead.",
						vf.name
					)));
				};
				vf_user_region.insert(
					axis_name,
					RegionValue::Range(Range::new(
						subset.user_minimum.py_max(continuous.minimum),
						subset.user_maximum.py_min(continuous.maximum),
						subset.user_default.unwrap_or(continuous.default),
					)),
				);
			}
			AxisSubset::Value(subset) => {
				vf_user_region.insert(axis_name, RegionValue::Value(subset.user_value));
			}
		}
	}
	// Any axis not mentioned explicitly has a single location = default value
	for axis in &doc.axes {
		let name = axis.name_str();
		if !vf_user_region.contains_key(&name) {
			vf_user_region.insert(name, RegionValue::Value(axis.default()));
		}
	}
	Ok(vf_user_region)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// `test_Range_post_init`: min and max are sorted and the default is
	/// clamped to either.
	#[test]
	fn range_post_init() {
		let r = Range::new(2, -1, -2);
		assert_eq!(r.minimum, Number::Int(-1));
		assert_eq!(r.maximum, Number::Int(2));
		assert_eq!(r.default, Number::Int(-1));
	}
}
