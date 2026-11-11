//! `DesignSpaceDocument`.

use std::path::Path;

use fontoxide_misc::plistlib;

use crate::descriptors::{
	AnisotropicLocation, Axis, AxisMappingDescriptor, AxisSubset, Condition, InstanceDescriptor,
	LocationLabelDescriptor, LocationValue, RangeAxisSubsetDescriptor, RuleDescriptor,
	SimpleLocation, SourceDescriptor, ValueAxisSubsetDescriptor, VariableFontDescriptor,
	to_anisotropic,
};
use crate::models::normalize_value;
use crate::number::Number;
use crate::paths::{self, posix};
use crate::{DesignSpaceDocumentError, Result, reader, writer};

/// `DesignSpaceDocument`: reads and writes `.designspace` data.
///
/// The reader and writer classes of fontTools can't be swapped here; the
/// descriptors are fixed types.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DesignSpaceDocument {
	/// The path the document was read from or written to (`path`).
	pub path: Option<String>,
	/// The file name of the document (`filename`).
	pub filename: Option<String>,
	/// The format version, e.g. `"4.0"` (`formatVersion`).
	pub format_version: Option<String>,
	/// STAT `elidedFallbackNameID` (`elidedFallbackName`).
	pub elided_fallback_name: Option<String>,
	/// The axes (`axes`).
	pub axes: Vec<Axis>,
	/// The axis mappings (`axisMappings`).
	pub axis_mappings: Vec<AxisMappingDescriptor>,
	/// The STAT format 4 labels (`locationLabels`).
	pub location_labels: Vec<LocationLabelDescriptor>,
	/// The rules (`rules`).
	pub rules: Vec<RuleDescriptor>,
	/// Whether rules apply after other glyph substitution features
	/// (`rulesProcessingLast`).
	pub rules_processing_last: bool,
	/// The sources (`sources`).
	pub sources: Vec<SourceDescriptor>,
	/// The variable fonts (`variableFonts`).
	pub variable_fonts: Vec<VariableFontDescriptor>,
	/// The instances (`instances`).
	pub instances: Vec<InstanceDescriptor>,
	/// Custom data (`lib`).
	pub lib: plistlib::Dict,
	/// The index in [`sources`](Self::sources) of the default master
	/// (`default`), updated by [`find_default`](Self::find_default).
	pub default: Option<usize>,
}

impl DesignSpaceDocument {
	/// `DesignSpaceDocument()`.
	pub fn new() -> Self {
		Self::default()
	}

	/// `DesignSpaceDocument.fromfile(path)`.
	pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
		let mut doc = Self::new();
		doc.read(path)?;
		Ok(doc)
	}

	/// `DesignSpaceDocument.fromstring(string)`.
	pub fn from_string(string: &str) -> Result<Self> {
		let mut doc = Self::new();
		reader::read(&mut doc, string, None)?;
		if !doc.sources.is_empty() {
			doc.find_default()?;
		}
		Ok(doc)
	}

	/// `tostring()`: the document as UTF-8 XML with a declaration. Like
	/// fontTools, writing fills in the locations of format 4 documents.
	///
	/// fontTools' `tostring(encoding="unicode")` fails with lxml; it has no
	/// counterpart here.
	pub fn tostring(&mut self) -> Result<String> {
		writer::write(self, true, true)
	}

	/// `read(path)`: reads a designspace file into this document.
	pub fn read(&mut self, path: impl AsRef<Path>) -> Result<()> {
		let path = paths::path_str(path.as_ref());
		self.path = Some(path.clone());
		self.filename = Some(paths::basename(&path));
		let bytes = std::fs::read(&path)?;
		let text =
			String::from_utf8(bytes).map_err(|e| DesignSpaceDocumentError::Xml(e.to_string()))?;
		reader::read(self, &text, Some(&path))?;
		if !self.sources.is_empty() {
			self.find_default()?;
		}
		Ok(())
	}

	/// `write(path)`: writes the document, after updating the source and
	/// instance file names from their paths.
	pub fn write(&mut self, path: impl AsRef<Path>) -> Result<()> {
		let path = paths::path_str(path.as_ref());
		self.path = Some(path.clone());
		self.filename = Some(paths::basename(&path));
		self.update_paths()?;
		let text = writer::write(self, true, true)?;
		std::fs::write(&path, text)?;
		Ok(())
	}

	fn posix_relative_path(&self, other_path: Option<&str>) -> Result<String> {
		let doc_path = self.path.as_deref().unwrap_or_default();
		let other_path = other_path.ok_or_else(|| {
			DesignSpaceDocumentError::Value(
				"expected str, bytes or os.PathLike object, not NoneType".into(),
			)
		})?;
		Ok(posix(&paths::relpath(
			other_path,
			&paths::dirname(doc_path),
		)?))
	}

	/// `updatePaths`: before saving, the file name of each source and instance
	/// with a path is recomputed relative to the document.
	pub fn update_paths(&mut self) -> Result<()> {
		if self.path.is_none() {
			return Err(DesignSpaceDocumentError::Value(
				"assert self.path is not None".into(),
			));
		}
		let mut updates = Vec::new();
		for (i, path) in self.sources.iter().map(|s| &s.path).enumerate() {
			if path.is_some() {
				updates.push((true, i, self.posix_relative_path(path.as_deref())?));
			}
		}
		for (i, path) in self.instances.iter().map(|s| &s.path).enumerate() {
			if path.is_some() {
				updates.push((false, i, self.posix_relative_path(path.as_deref())?));
			}
		}
		for (is_source, i, filename) in updates {
			if is_source {
				self.sources[i].set_filename(Some(&filename));
			} else {
				self.instances[i].set_filename(Some(&filename));
			}
		}
		Ok(())
	}

	/// `addSource`.
	pub fn add_source(&mut self, source: SourceDescriptor) -> &mut SourceDescriptor {
		self.sources.push(source);
		let last = self.sources.len() - 1;
		&mut self.sources[last]
	}

	/// `addInstance`.
	pub fn add_instance(&mut self, instance: InstanceDescriptor) -> &mut InstanceDescriptor {
		self.instances.push(instance);
		let last = self.instances.len() - 1;
		&mut self.instances[last]
	}

	/// `addAxis`.
	pub fn add_axis(&mut self, axis: impl Into<Axis>) -> &mut Axis {
		self.axes.push(axis.into());
		let last = self.axes.len() - 1;
		&mut self.axes[last]
	}

	/// `addAxisMapping`.
	pub fn add_axis_mapping(
		&mut self,
		mapping: AxisMappingDescriptor,
	) -> &mut AxisMappingDescriptor {
		self.axis_mappings.push(mapping);
		let last = self.axis_mappings.len() - 1;
		&mut self.axis_mappings[last]
	}

	/// `addRule`.
	pub fn add_rule(&mut self, rule: RuleDescriptor) -> &mut RuleDescriptor {
		self.rules.push(rule);
		let last = self.rules.len() - 1;
		&mut self.rules[last]
	}

	/// `addVariableFont`.
	pub fn add_variable_font(&mut self, vf: VariableFontDescriptor) -> &mut VariableFontDescriptor {
		self.variable_fonts.push(vf);
		let last = self.variable_fonts.len() - 1;
		&mut self.variable_fonts[last]
	}

	/// `addLocationLabel`.
	pub fn add_location_label(
		&mut self,
		label: LocationLabelDescriptor,
	) -> &mut LocationLabelDescriptor {
		self.location_labels.push(label);
		let last = self.location_labels.len() - 1;
		&mut self.location_labels[last]
	}

	/// `newDefaultLocation`: the default location in design space coordinates.
	pub fn new_default_location(&self) -> Result<SimpleLocation> {
		let mut loc = SimpleLocation::new();
		for axis in &self.axes {
			loc.insert(axis.name_str(), axis.map_forward(axis.default())?);
		}
		Ok(loc)
	}

	/// `labelForUserLocation`: the location label at exactly this user
	/// location.
	pub fn label_for_user_location(
		&self,
		user_location: &SimpleLocation,
	) -> Option<&LocationLabelDescriptor> {
		self.location_labels
			.iter()
			.find(|label| label.user_location == *user_location)
	}

	/// `updateFilenameFromPath`: sets the file names from the paths, skipping
	/// those that have one unless `force` is set.
	pub fn update_filename_from_path(
		&mut self,
		masters: bool,
		instances: bool,
		force: bool,
	) -> Result<()> {
		if self.path.is_none() {
			return Ok(());
		}
		if masters {
			for i in 0..self.sources.len() {
				if self.sources[i].filename.is_some() && !force {
					continue;
				}
				let filename = self.posix_relative_path(self.sources[i].path.as_deref())?;
				self.sources[i].set_filename(Some(&filename));
			}
		}
		if instances {
			for i in 0..self.instances.len() {
				if self.instances[i].filename.is_some() && !force {
					continue;
				}
				let filename = self.posix_relative_path(self.instances[i].path.as_deref())?;
				self.instances[i].set_filename(Some(&filename));
			}
		}
		Ok(())
	}

	/// `getAxisOrder`: the axis names in document order.
	pub fn get_axis_order(&self) -> Vec<Option<&str>> {
		self.axes.iter().map(Axis::name).collect()
	}

	/// `getAxis`.
	pub fn get_axis(&self, name: &str) -> Option<&Axis> {
		self.axes.iter().find(|a| a.name() == Some(name))
	}

	/// `getAxisByTag`.
	pub fn get_axis_by_tag(&self, tag: &str) -> Option<&Axis> {
		self.axes.iter().find(|a| a.tag() == Some(tag))
	}

	/// `getLocationLabel`.
	pub fn get_location_label(&self, name: &str) -> Option<&LocationLabelDescriptor> {
		self.location_labels.iter().find(|l| l.name == name)
	}

	/// `map_forward`: a user location to a design location; missing axes are at
	/// their default.
	pub fn map_forward(&self, user_location: &SimpleLocation) -> Result<SimpleLocation> {
		let mut out = SimpleLocation::new();
		for axis in &self.axes {
			let name = axis.name_str();
			let v = user_location.get(&name).copied().unwrap_or(axis.default());
			out.insert(name, axis.map_forward(v)?);
		}
		Ok(out)
	}

	/// `map_backward`: a design location to a user location; missing axes are
	/// at their default, and only the x value of anisotropic values is used.
	pub fn map_backward(&self, design_location: &AnisotropicLocation) -> Result<SimpleLocation> {
		let mut out = SimpleLocation::new();
		for axis in &self.axes {
			let name = axis.name_str();
			let v = match design_location.get(&name) {
				Some(v) => axis.map_backward(*v)?,
				None => axis.default(),
			};
			out.insert(name, v);
		}
		Ok(out)
	}

	/// The index of the source at the default location, without updating
	/// [`default`](Self::default).
	pub fn compute_default(&self) -> Result<Option<usize>> {
		let default_design_location = to_anisotropic(&self.new_default_location()?);
		for (i, source) in self.sources.iter().enumerate() {
			if source.get_full_design_location(self)? == default_design_location {
				return Ok(Some(i));
			}
		}
		Ok(None)
	}

	/// `findDefault`: sets and returns the source at the default location.
	pub fn find_default(&mut self) -> Result<Option<&SourceDescriptor>> {
		self.default = self.compute_default()?;
		Ok(self.default.map(|i| &self.sources[i]))
	}

	/// The default source found by [`find_default`](Self::find_default).
	pub fn default_source(&self) -> Option<&SourceDescriptor> {
		self.default.and_then(|i| self.sources.get(i))
	}

	/// `normalizeLocation`: the location normalized per axis; axes not in the
	/// location are skipped.
	pub fn normalize_location(&self, location: &AnisotropicLocation) -> Result<SimpleLocation> {
		let mut new = SimpleLocation::new();
		for axis in &self.axes {
			let name = axis.name_str();
			let Some(value) = location.get(&name) else {
				continue;
			};
			let triple = (
				axis.map_forward(axis.minimum()?)?,
				axis.map_forward(axis.default())?,
				axis.map_forward(axis.maximum()?)?,
			);
			new.insert(name, normalize_value(value.x(), triple, false, true)?);
		}
		Ok(new)
	}

	fn normalize_one(&self, name: &str, value: Number) -> Result<Option<Number>> {
		let mut location = AnisotropicLocation::new();
		location.insert(name.to_owned(), LocationValue::Value(value));
		Ok(self.normalize_location(&location)?.get(name).copied())
	}

	/// `normalize`: scales all locations of sources, instances, rules and axes
	/// to -1, 0, 1.
	pub fn normalize(&mut self) -> Result<()> {
		// masters
		let mut sources = std::mem::take(&mut self.sources);
		let result = sources.iter_mut().try_for_each(|item| {
			item.design_location = to_anisotropic(&self.normalize_location(&item.design_location)?);
			Ok::<_, DesignSpaceDocumentError>(())
		});
		self.sources = sources;
		result?;
		// instances
		let mut instances = std::mem::take(&mut self.instances);
		let result = instances.iter_mut().try_for_each(|item| {
			// glyph masters for this instance
			for glyph_data in item.glyphs.values_mut() {
				let instance_location = glyph_data.instance_location.as_ref().ok_or_else(|| {
					DesignSpaceDocumentError::Value("KeyError: 'instanceLocation'".into())
				})?;
				glyph_data.instance_location =
					Some(to_anisotropic(&self.normalize_location(instance_location)?));
				let masters = glyph_data
					.masters
					.as_mut()
					.ok_or_else(|| DesignSpaceDocumentError::Value("KeyError: 'masters'".into()))?;
				for glyph_master in masters {
					let location = glyph_master.location.as_ref().ok_or_else(|| {
						DesignSpaceDocumentError::Value(
							"argument of type 'NoneType' is not iterable".into(),
						)
					})?;
					glyph_master.location =
						Some(to_anisotropic(&self.normalize_location(location)?));
				}
			}
			item.design_location = to_anisotropic(&self.normalize_location(&item.design_location)?);
			Ok::<_, DesignSpaceDocumentError>(())
		});
		self.instances = instances;
		result?;
		// rules also need the original axis data
		let mut new_rules = Vec::new();
		for rule in &self.rules {
			let mut new_condition_sets = Vec::new();
			for conditions in &rule.condition_sets {
				let mut new_conditions = Vec::new();
				for cond in conditions {
					let minimum = match cond.minimum {
						Some(m) => self.normalize_one(&cond.name, m)?,
						None => None,
					};
					let maximum = match cond.maximum {
						Some(m) => self.normalize_one(&cond.name, m)?,
						None => None,
					};
					new_conditions.push(Condition {
						name: cond.name.clone(),
						minimum,
						maximum,
					});
				}
				new_condition_sets.push(new_conditions);
			}
			new_rules.push(new_condition_sets);
		}
		for (rule, condition_sets) in self.rules.iter_mut().zip(new_rules) {
			rule.condition_sets = condition_sets;
		}
		// the axes
		for i in 0..self.axes.len() {
			let axis = &self.axes[i];
			let name = axis.name_str();
			// Convert user-space bounds to design coordinates before changing the
			// map.
			let missing = || DesignSpaceDocumentError::Value(format!("KeyError: {name:?}"));
			let minimum = self.normalize_one(&name, axis.map_forward(axis.minimum()?)?)?;
			let maximum = self.normalize_one(&name, axis.map_forward(axis.maximum()?)?)?;
			let default = self.normalize_one(&name, axis.map_forward(axis.default())?)?;
			// scale the map outputs
			let mut new_map = Vec::new();
			for &(input_value, output_value) in axis.map() {
				let new_output = self
					.normalize_one(&name, output_value)?
					.ok_or_else(missing)?;
				new_map.push((input_value, new_output));
			}
			let (Some(minimum), Some(maximum), Some(default)) = (minimum, maximum, default) else {
				return Err(missing());
			};
			if let Axis::Continuous(axis) = &mut self.axes[i] {
				if !new_map.is_empty() {
					axis.map = new_map;
				}
				// finally the axis values
				axis.minimum = minimum;
				axis.maximum = maximum;
				axis.default = default;
			}
		}
		Ok(())
	}

	/// `formatTuple`: the format version as `(major, minor)`; `(5, 0)` when it
	/// is not set.
	pub fn format_tuple(&self) -> Result<(i128, i128)> {
		let Some(version) = &self.format_version else {
			return Ok((5, 0));
		};
		let mut numbers = version.split('.').map(|s| {
			fontoxide_misc::py::parse_int(s).ok_or_else(|| {
				DesignSpaceDocumentError::Value(format!(
					"invalid literal for int() with base 10: {}",
					fontoxide_misc::py::str_repr(s)
				))
			})
		});
		let major = numbers.next().unwrap_or(Ok(0))?;
		let minor = numbers.next().unwrap_or(Ok(0))?;
		Ok((major, minor))
	}

	/// `getVariableFonts`: the variable fonts of the document, or the implicit
	/// ones: one per combination of discrete axis values, over all continuous
	/// axes.
	pub fn get_variable_fonts(&self) -> Vec<VariableFontDescriptor> {
		if !self.variable_fonts.is_empty() {
			return self.variable_fonts.clone();
		}
		let mut variable_fonts = Vec::new();
		let mut discrete_axes = Vec::new();
		let mut range_axis_subsets = Vec::new();
		for axis in &self.axes {
			match axis {
				Axis::Discrete(a) => discrete_axes.push(a),
				Axis::Continuous(_) => range_axis_subsets.push(AxisSubset::Range(
					RangeAxisSubsetDescriptor::new(&axis.name_str()),
				)),
			}
		}
		let value_lists: Vec<&[Number]> =
			discrete_axes.iter().map(|a| a.values.as_slice()).collect();
		for values in product(&value_lists) {
			let mut basename = None;
			if let Some(filename) = &self.filename {
				basename = Some(format!("{}-VF", paths::splitext_root(filename)));
			}
			if let Some(path) = &self.path {
				basename = Some(format!(
					"{}-VF",
					paths::splitext_root(&paths::basename(path))
				));
			}
			let basename = basename.unwrap_or_else(|| "VF".to_owned());
			let axis_names: String = discrete_axes
				.iter()
				.zip(&values)
				.map(|(axis, value)| format!("-{}{value}", axis.tag.as_deref().unwrap_or("None")))
				.collect();
			let mut axis_subsets = range_axis_subsets.clone();
			axis_subsets.extend(discrete_axes.iter().zip(&values).map(|(axis, value)| {
				AxisSubset::Value(ValueAxisSubsetDescriptor {
					name: axis.name.clone().unwrap_or_else(|| "None".into()),
					user_value: *value,
				})
			}));
			variable_fonts.push(VariableFontDescriptor {
				name: format!("{basename}{axis_names}"),
				filename: None,
				axis_subsets,
				lib: plistlib::Dict::new(),
			});
		}
		variable_fonts
	}

	/// `deepcopyExceptFonts`: a deep copy (fonts are not attached to
	/// descriptors here).
	pub fn deepcopy_except_fonts(&self) -> Self {
		self.clone()
	}
}

/// `itertools.product(*lists)`.
pub(crate) fn product<T: Copy>(lists: &[&[T]]) -> Vec<Vec<T>> {
	let mut out: Vec<Vec<T>> = vec![Vec::new()];
	for list in lists {
		let mut next = Vec::new();
		for prefix in &out {
			for &item in *list {
				let mut p = prefix.clone();
				p.push(item);
				next.push(p);
			}
		}
		out = next;
	}
	out
}
