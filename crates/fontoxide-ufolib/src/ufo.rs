//! `UFOReader` and `UFOWriter` of `fontTools.ufoLib`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use fontoxide_misc::plistlib::{Dict, Value};
use indexmap::IndexMap;

use crate::base_io::{file_modification_time, get_plist, write_plist};
use crate::converters::{GroupRenameMaps, convert_ufo1_or_ufo2_kerning_to_ufo3_kerning};
use crate::errors::{Result, UfoLibError, ufo_err};
use crate::filenames::{lower, user_name_to_file_name};
use crate::fontinfo;
use crate::fs::{Fs, FsError, MemFs, dirname};
use crate::glif_lib::{GlyphNameToFileNameFunc, GlyphSet, GlyphSetOptions};
use crate::utils::{FormatVersionInput, Number, UfoFormatVersion, value_str};
use crate::validators::{
	font_lib_dict_validator, font_lib_validator, groups_validator, kerning_validator,
	layer_contents_validator, png_validator,
};
use crate::zip;
use crate::{Groups, Kerning, KerningNested};

/// `DEFAULT_GLYPHS_DIRNAME`.
pub const DEFAULT_GLYPHS_DIRNAME: &str = "glyphs";
/// `DATA_DIRNAME`.
pub const DATA_DIRNAME: &str = "data";
/// `IMAGES_DIRNAME`.
pub const IMAGES_DIRNAME: &str = "images";
/// `METAINFO_FILENAME`.
pub const METAINFO_FILENAME: &str = "metainfo.plist";
/// `FONTINFO_FILENAME`.
pub const FONTINFO_FILENAME: &str = "fontinfo.plist";
/// `LIB_FILENAME`.
pub const LIB_FILENAME: &str = "lib.plist";
/// `GROUPS_FILENAME`.
pub const GROUPS_FILENAME: &str = "groups.plist";
/// `KERNING_FILENAME`.
pub const KERNING_FILENAME: &str = "kerning.plist";
/// `FEATURES_FILENAME`.
pub const FEATURES_FILENAME: &str = "features.fea";
/// `LAYERCONTENTS_FILENAME`.
pub const LAYERCONTENTS_FILENAME: &str = "layercontents.plist";
/// `LAYERINFO_FILENAME`.
pub const LAYERINFO_FILENAME: &str = "layerinfo.plist";
/// `DEFAULT_LAYER_NAME`.
pub const DEFAULT_LAYER_NAME: &str = "public.default";

/// `UFOFileStructure`: a UFO is a directory (a package) or a zip file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UfoFileStructure {
	/// `ZIP`: a zipped UFO (`.ufoz`).
	Zip,
	/// `PACKAGE`: a directory.
	Package,
}

impl std::fmt::Display for UfoFileStructure {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(match self {
			UfoFileStructure::Zip => "UFOFileStructure.ZIP",
			UfoFileStructure::Package => "UFOFileStructure.PACKAGE",
		})
	}
}

/// The kerning and groups of a UFO 1 or 2, converted to UFO 3 when first
/// read.
#[derive(Debug, Clone)]
struct UpConvertedKerningData {
	kerning: KerningNested,
	original_kerning: Value,
	groups: Groups,
	original_groups: Value,
	group_rename_maps: GroupRenameMaps,
}

/// `_sniffFileStructure`.
fn sniff_file_structure(ufo_path: &Path) -> Result<UfoFileStructure> {
	if ufo_path.is_file() {
		let data = std::fs::read(ufo_path)?;
		if zip::is_zipfile(&data) {
			return Ok(UfoFileStructure::Zip);
		}
		return ufo_err(format!(
			"The specified UFO does not have a known structure: '{}'",
			ufo_path.display()
		));
	}
	if ufo_path.is_dir() {
		return Ok(UfoFileStructure::Package);
	}
	ufo_err(format!(
		"No such file or directory: '{}'",
		ufo_path.display()
	))
}

/// `makeUFOPath`: the path with its extension replaced by `.ufo`.
///
/// ```
/// use fontoxide_ufolib::make_ufo_path;
/// let expected = std::path::Path::new("directory").join("something.another.thing.ufo");
/// assert_eq!(make_ufo_path("directory/something.another.thing.ext"), expected.to_string_lossy());
/// ```
pub fn make_ufo_path(path: &str) -> String {
	let (dir, name) = match path.rfind(['/', std::path::MAIN_SEPARATOR]) {
		Some(i) => (&path[..i], &path[i + 1..]),
		None => ("", path),
	};
	let parts: Vec<&str> = name.split('.').collect();
	let stem = parts[..parts.len() - 1].join(".");
	let name = format!("{stem}.ufo");
	if dir.is_empty() {
		name
	} else {
		Path::new(dir).join(name).to_string_lossy().into_owned()
	}
}

/// The rooted subdirectory of a zipped UFO (`rootDirs`): exactly one
/// directory, ignoring macOS metadata.
fn zip_root_dir(parent: &Fs) -> Result<String> {
	let root_dirs: Vec<String> = parent
		.scandir("/")?
		.into_iter()
		.filter(|(name, is_dir)| *is_dir && name != "__MACOSX")
		.map(|(name, _)| name)
		.collect();
	if root_dirs.len() != 1 {
		return ufo_err(format!(
			"Expected exactly 1 root directory, found {}",
			root_dirs.len()
		));
	}
	Ok(root_dirs.into_iter().next().unwrap_or_default())
}

/// `os.path.splitext(os.path.basename(path))[0] + ".ufo"`.
fn zip_root_name(path: &str) -> String {
	let base = Path::new(path)
		.file_name()
		.map(|n| n.to_string_lossy().into_owned())
		.unwrap_or_default();
	let stem = match base.rfind('.') {
		Some(i) if i > 0 && base[..i].chars().any(|c| c != '.') => &base[..i],
		_ => &base[..],
	};
	format!("{stem}.ufo")
}

fn dict_of_strings(value: Value, message: &str) -> Result<IndexMap<String, Vec<String>>> {
	let Value::Dict(d) = value else {
		return ufo_err(message);
	};
	let mut out = IndexMap::new();
	for (k, v) in d {
		let Value::Array(a) = v else {
			return ufo_err(message);
		};
		let mut names = Vec::new();
		for n in a {
			let Value::String(n) = n else {
				return ufo_err(message);
			};
			names.push(n);
		}
		out.insert(k, names);
	}
	Ok(out)
}

fn kerning_nested(value: &Value) -> Result<KerningNested> {
	let bad = "The kerning data is not in the correct format.";
	let Value::Dict(d) = value else {
		return ufo_err(bad);
	};
	let mut out = KerningNested::new();
	for (k, v) in d {
		let Value::Dict(seconds) = v else {
			return ufo_err(bad);
		};
		out.insert(
			k.clone(),
			seconds
				.iter()
				.map(|(a, b)| (a.clone(), b.clone()))
				.collect(),
		);
	}
	Ok(out)
}

/// The groups as a plist value.
pub fn groups_to_value(groups: &Groups) -> Value {
	Value::Dict(
		groups
			.iter()
			.map(|(k, v)| {
				(
					k.clone(),
					Value::Array(v.iter().map(|g| Value::String(g.clone())).collect()),
				)
			})
			.collect(),
	)
}

/// Nested kerning as a plist value.
pub fn kerning_nested_to_value(kerning: &KerningNested) -> Value {
	Value::Dict(
		kerning
			.iter()
			.map(|(k, v)| {
				(
					k.clone(),
					Value::Dict(v.iter().map(|(a, b)| (a.clone(), b.clone())).collect()),
				)
			})
			.collect(),
	)
}

// ----------
// UFO Reader
// ----------

/// `UFOReader`: reads the parts of a UFO (a directory or a zip file).
///
/// Data is validated by default; the `validate` arguments of the methods
/// default to the reader's.
#[derive(Debug)]
pub struct UfoReader {
	fs: Fs,
	path: String,
	validate: bool,
	format_version: UfoFormatVersion,
	file_structure: UfoFileStructure,
	have_previous_file: bool,
	up_converted_kerning_data: RefCell<Option<UpConvertedKerningData>>,
}

impl UfoReader {
	/// `UFOReader(path, validate)`.
	pub fn new(path: impl AsRef<Path>, validate: bool) -> Result<UfoReader> {
		let path = path.as_ref();
		let structure = sniff_file_structure(path)?;
		let fs = match structure {
			UfoFileStructure::Zip => {
				let data = std::fs::read(path)?;
				let label = format!("<zipfs '{}'>", path.display());
				let parent = Fs::mem(zip::read_zip(&data, label).map_err(|e| {
					UfoLibError::UfoLib(format!("unable to open '{}': {e}", path.display()))
				})?);
				let root = zip_root_dir(&parent)?;
				parent.opendir(&root)?
			}
			UfoFileStructure::Package => Fs::os(path, false).map_err(|e| {
				UfoLibError::UfoLib(format!("unable to open '{}': {e}", path.display()))
			})?,
		};
		let mut reader = UfoReader {
			fs,
			path: path.to_string_lossy().into_owned(),
			validate,
			format_version: UfoFormatVersion::Format3_0,
			file_structure: structure,
			have_previous_file: false,
			up_converted_kerning_data: RefCell::new(None),
		};
		reader.read_meta_info(Some(validate))?;
		Ok(reader)
	}

	/// The path the UFO was opened with (`path`, deprecated in fontTools).
	pub fn path(&self) -> &str {
		&self.path
	}

	/// The file system of the UFO (`fs`).
	pub fn fs(&self) -> &Fs {
		&self.fs
	}

	/// `formatVersionTuple`: the format version, read from `metainfo.plist`.
	pub fn format_version_tuple(&self) -> UfoFormatVersion {
		self.format_version
	}

	/// `fileStructure`.
	pub fn file_structure(&self) -> UfoFileStructure {
		self.file_structure
	}

	fn validate_or_default(&self, validate: Option<bool>) -> bool {
		validate.unwrap_or(self.validate)
	}

	// up conversion

	/// `_upConvertKerning`: converts the kerning and groups of a UFO 1 or 2
	/// once, and checks that the files did not change since.
	fn up_convert_kerning(&self, validate: bool) -> Result<()> {
		if let Some(data) = self.up_converted_kerning_data.borrow().as_ref() {
			if self.read_kerning_raw()? != data.original_kerning {
				return ufo_err(
					"The data in kerning.plist has been modified since it was converted to UFO 3 format.",
				);
			}
			if self.read_groups_raw()? != data.original_groups {
				return ufo_err(
					"The data in groups.plist has been modified since it was converted to UFO 3 format.",
				);
			}
			return Ok(());
		}
		let groups_raw = self.read_groups_raw()?;
		let message = "groups.plist is not properly formatted.";
		if validate {
			// the same checks as the conversion to typed groups
			dict_of_strings(groups_raw.clone(), message)?;
		}
		// LENIENT(L35): without validation, malformed groups or kerning are an error
		// here rather than a TypeError or AttributeError in the conversion.
		let groups = dict_of_strings(groups_raw.clone(), message)?;
		let original_kerning = self.read_kerning_raw()?;
		let kerning = kerning_nested(&original_kerning)?;
		let glyph_set = self.get_glyph_set(None, None, None)?;
		let (kerning, groups, maps) =
			convert_ufo1_or_ufo2_kerning_to_ufo3_kerning(&kerning, groups, &|name| {
				glyph_set.contains(name)
			});
		*self.up_converted_kerning_data.borrow_mut() = Some(UpConvertedKerningData {
			kerning,
			original_kerning,
			groups,
			original_groups: groups_raw,
			group_rename_maps: maps,
		});
		Ok(())
	}

	// support methods

	/// `readBytesFromPath`: the data of a file, or `None` if it does not
	/// exist.
	pub fn read_bytes_from_path(&self, path: &str) -> Result<Option<Vec<u8>>> {
		match self.fs.readbytes(path) {
			Ok(data) => Ok(Some(data)),
			Err(FsError::NotFound(_)) => Ok(None),
			Err(e) => Err(e.into()),
		}
	}

	/// `getReadFileForPath`: a reader of a file, or `None` if it does not
	/// exist.
	pub fn get_read_file_for_path(&self, path: &str) -> Result<Option<std::io::Cursor<Vec<u8>>>> {
		Ok(self.read_bytes_from_path(path)?.map(std::io::Cursor::new))
	}

	/// `getFileModificationTime`: the modification time of a file of the UFO,
	/// or `None` if it does not exist.
	pub fn get_file_modification_time(&self, path: &str) -> Result<Option<f64>> {
		file_modification_time(&self.fs, path)
	}

	// metainfo.plist

	/// `_readMetaInfo`: the format version from `metainfo.plist`.
	fn read_meta_info_version(&self, validate: Option<bool>) -> Result<UfoFormatVersion> {
		let validate = self.validate_or_default(validate);
		let data = get_plist(&self.fs, METAINFO_FILENAME, None)?;
		if validate && !matches!(data, Value::Dict(_)) {
			return ufo_err("metainfo.plist is not properly formatted.");
		}
		// LENIENT(L35): without validation, a metainfo.plist that is not a dictionary
		// is a TypeError in fontTools.
		let Some(major) = data.as_dict().and_then(|d| d.get("formatVersion")) else {
			return ufo_err(format!(
				"Missing required formatVersion in '{METAINFO_FILENAME}' on {}",
				self.fs
			));
		};
		let minor = data
			.as_dict()
			.and_then(|d| d.get("formatVersionMinor"))
			.cloned()
			.unwrap_or(Value::Integer(0));
		// `UFOFormatVersion((major, minor))` compares the tuple by value.
		let found = UfoFormatVersion::ALL.into_iter().find(|v| {
			Number::from_value(major) == Some(Number::Int(v.major()))
				&& Number::from_value(&minor) == Some(Number::Int(v.minor()))
		});
		match found {
			Some(v) => Ok(v),
			None => {
				let msg = format!(
					"Unsupported UFO format ({}.{}) in '{METAINFO_FILENAME}' on {}",
					value_str(major),
					value_str(&minor),
					self.fs
				);
				if validate {
					return Err(UfoLibError::UnsupportedUfoFormat(msg));
				}
				let v = UfoFormatVersion::default_version();
				log::warn!(
					"{msg}. Assuming the latest supported version ({v}). Some data may be skipped or parsed incorrectly"
				);
				Ok(v)
			}
		}
	}

	/// `readMetaInfo`: reads `metainfo.plist` and sets the format version.
	pub fn read_meta_info(&mut self, validate: Option<bool>) -> Result<()> {
		self.format_version = self.read_meta_info_version(validate)?;
		Ok(())
	}

	// groups.plist

	/// `_readGroups`: the raw groups, with duplicate glyphs removed from
	/// kerning groups.
	fn read_groups_raw(&self) -> Result<Value> {
		let mut groups = get_plist(&self.fs, GROUPS_FILENAME, Some(Value::Dict(Dict::new())))?;
		if let Value::Dict(d) = &mut groups {
			for (name, glyph_list) in d.iter_mut() {
				if (name.starts_with("public.kern1.") || name.starts_with("public.kern2."))
					&& let Value::Array(list) = glyph_list
				{
					let mut unique: Vec<Value> = Vec::new();
					for g in list.drain(..) {
						if !unique.contains(&g) {
							unique.push(g);
						}
					}
					*list = unique;
				}
			}
		}
		Ok(groups)
	}

	/// `readGroups`: the groups, converted to UFO 3 kerning groups if needed.
	///
	/// LENIENT(L35): groups that are not a dictionary of lists of strings are an
	/// error even without validation.
	pub fn read_groups(&self, validate: Option<bool>) -> Result<Groups> {
		let validate = self.validate_or_default(validate);
		let groups = if self.format_version < UfoFormatVersion::Format3_0 {
			self.up_convert_kerning(validate)?;
			self.up_converted_kerning_data
				.borrow()
				.as_ref()
				.map(|d| d.groups.clone())
				.unwrap_or_default()
		} else {
			let raw = self.read_groups_raw()?;
			if validate && let Err(message) = groups_validator(&raw) {
				return ufo_err(message);
			}
			return dict_of_strings(raw, "The group data is not in the correct format.");
		};
		if validate && let Err(message) = groups_validator(&groups_to_value(&groups)) {
			return ufo_err(message);
		}
		Ok(groups)
	}

	/// `getKerningGroupConversionRenameMaps`: the renaming done when
	/// converting the kerning groups of a UFO 1 or 2 (empty for UFO 3).
	pub fn get_kerning_group_conversion_rename_maps(
		&self,
		validate: Option<bool>,
	) -> Result<GroupRenameMaps> {
		if self.format_version >= UfoFormatVersion::Format3_0 {
			return Ok(GroupRenameMaps::default());
		}
		self.read_groups(validate)?;
		Ok(self
			.up_converted_kerning_data
			.borrow()
			.as_ref()
			.map(|d| d.group_rename_maps.clone())
			.unwrap_or_default())
	}

	// fontinfo.plist

	/// `readInfo`: the attributes of `fontinfo.plist`, converted to the UFO 3
	/// specification (fontTools sets them on an object).
	///
	/// LENIENT(L35): fontTools iterates over the version 1 attributes in set order;
	/// here they are taken in the order of the specification.
	pub fn read_info(&self, validate: Option<bool>) -> Result<Dict> {
		let validate = self.validate_or_default(validate);
		let info = get_plist(&self.fs, FONTINFO_FILENAME, Some(Value::Dict(Dict::new())))?;
		// LENIENT(L35): without validation, a fontinfo.plist that is not a dictionary
		// is an AttributeError in fontTools.
		let Value::Dict(info) = info else {
			return ufo_err("fontinfo.plist is not properly formatted.");
		};
		let mut to_set = Dict::new();
		match self.format_version {
			UfoFormatVersion::Format1_0 => {
				for attr in fontinfo::FONT_INFO_ATTRIBUTES_VERSION1 {
					if let Some(v) = info.get(*attr) {
						to_set.insert((*attr).to_owned(), v.clone());
					}
				}
				to_set = fontinfo::convert_font_info_data_version1_to_version2(&to_set)?;
				to_set = fontinfo::convert_font_info_data_version2_to_version3(&to_set)?;
			}
			UfoFormatVersion::Format2_0 => {
				for (attr, _) in fontinfo::FONT_INFO_ATTRIBUTES_VERSION2_VALUE_DATA {
					if let Some(v) = info.get(*attr) {
						to_set.insert((*attr).to_owned(), v.clone());
					}
				}
				to_set = fontinfo::convert_font_info_data_version2_to_version3(&to_set)?;
			}
			UfoFormatVersion::Format3_0 => {
				for (attr, _) in fontinfo::font_info_attributes_version3_value_data() {
					if let Some(v) = info.get(attr) {
						to_set.insert(attr.to_owned(), v.clone());
					}
				}
			}
		}
		if validate {
			to_set = fontinfo::validate_info_version3_data(&to_set)?;
		}
		Ok(to_set)
	}

	// kerning.plist

	fn read_kerning_raw(&self) -> Result<Value> {
		get_plist(&self.fs, KERNING_FILENAME, Some(Value::Dict(Dict::new())))
	}

	/// `readKerning`: the kerning pairs, converted to UFO 3 kerning groups if
	/// needed.
	///
	/// LENIENT(L35): kerning that is not a dictionary of dictionaries is an error
	/// even without validation.
	pub fn read_kerning(&self, validate: Option<bool>) -> Result<Kerning> {
		let validate = self.validate_or_default(validate);
		let nested = if self.format_version < UfoFormatVersion::Format3_0 {
			self.up_convert_kerning(validate)?;
			self.up_converted_kerning_data
				.borrow()
				.as_ref()
				.map(|d| d.kerning.clone())
				.unwrap_or_default()
		} else {
			let raw = self.read_kerning_raw()?;
			if validate && let Err(message) = kerning_validator(&raw) {
				return ufo_err(message);
			}
			kerning_nested(&raw)?
		};
		if validate && let Err(message) = kerning_validator(&kerning_nested_to_value(&nested)) {
			return ufo_err(message);
		}
		let mut kerning = Kerning::new();
		for (left, seconds) in nested {
			for (right, value) in seconds {
				kerning.insert((left.clone(), right), value);
			}
		}
		Ok(kerning)
	}

	// lib.plist

	/// `readLib`: the contents of `lib.plist`.
	pub fn read_lib(&self, validate: Option<bool>) -> Result<Dict> {
		let validate = self.validate_or_default(validate);
		let data = get_plist(&self.fs, LIB_FILENAME, Some(Value::Dict(Dict::new())))?;
		if validate && let Err(message) = font_lib_validator(&data) {
			return ufo_err(message);
		}
		match data {
			Value::Dict(d) => Ok(d),
			// LENIENT(L35): without validation, fontTools returns whatever lib.plist holds.
			other => ufo_err(format!(
				"The lib data is not in the correct format: expected a dictionary, found {}",
				crate::utils::value_repr(&other)
			)),
		}
	}

	// features.fea

	/// `readFeatures`: the contents of `features.fea` (empty if missing),
	/// decoded as UTF-8 with an optional byte order mark and with universal
	/// newlines.
	pub fn read_features(&self) -> Result<String> {
		let data = match self.fs.readbytes(FEATURES_FILENAME) {
			Ok(d) => d,
			Err(FsError::NotFound(_)) => return Ok(String::new()),
			Err(e) => return Err(e.into()),
		};
		let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&data);
		let text = std::str::from_utf8(data)
			.map_err(|e| UfoLibError::Value(format!("'utf-8' codec can't decode: {e}")))?;
		Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
	}

	// glyph sets & layers

	/// `_readLayerContents`: the layer names and directories.
	///
	/// LENIENT(L35): without validation, contents that are not a list of pairs of
	/// strings are an error.
	fn read_layer_contents(&self, validate: bool) -> Result<Vec<(String, String)>> {
		if self.format_version < UfoFormatVersion::Format3_0 {
			return Ok(vec![(
				DEFAULT_LAYER_NAME.into(),
				DEFAULT_GLYPHS_DIRNAME.into(),
			)]);
		}
		let contents = get_plist(&self.fs, LAYERCONTENTS_FILENAME, None)?;
		if validate {
			let fs = &self.fs;
			if let Err(message) = layer_contents_validator(&contents, &|d| Ok(fs.exists(d)?))? {
				return ufo_err(message);
			}
		}
		let bad =
			|| UfoLibError::UfoLib("layercontents.plist in not in the correct format.".into());
		let Value::Array(entries) = contents else {
			return Err(bad());
		};
		entries
			.into_iter()
			.map(|e| match e {
				Value::Array(pair) => match pair.as_slice() {
					[Value::String(a), Value::String(b)] => Ok((a.clone(), b.clone())),
					_ => Err(bad()),
				},
				_ => Err(bad()),
			})
			.collect()
	}

	/// `getLayerNames`: the layer names, in order.
	pub fn get_layer_names(&self, validate: Option<bool>) -> Result<Vec<String>> {
		let validate = self.validate_or_default(validate);
		Ok(self
			.read_layer_contents(validate)?
			.into_iter()
			.map(|(name, _)| name)
			.collect())
	}

	/// `getDefaultLayerName`: the name of the layer in the `glyphs`
	/// directory.
	pub fn get_default_layer_name(&self, validate: Option<bool>) -> Result<String> {
		let validate = self.validate_or_default(validate);
		for (name, directory) in self.read_layer_contents(validate)? {
			if directory == DEFAULT_GLYPHS_DIRNAME {
				return Ok(name);
			}
		}
		ufo_err("The default layer is not defined in layercontents.plist.")
	}

	/// `getGlyphSet`: the glyph set of a layer (the default layer if
	/// `None`).
	pub fn get_glyph_set(
		&self,
		layer_name: Option<&str>,
		validate_read: Option<bool>,
		validate_write: Option<bool>,
	) -> Result<GlyphSet> {
		let validate_read = self.validate_or_default(validate_read);
		let validate_write = self.validate_or_default(validate_write);
		let layer_name = match layer_name {
			Some(n) => n.to_owned(),
			None => self.get_default_layer_name(Some(validate_read))?,
		};
		let directory = self
			.read_layer_contents(validate_read)?
			.into_iter()
			.find(|(name, _)| *name == layer_name)
			.map(|(_, d)| d);
		let Some(directory) = directory else {
			return ufo_err(format!(
				"No glyphs directory is mapped to \"{layer_name}\"."
			));
		};
		let sub = match self.fs.opendir(&directory) {
			Ok(sub) => sub,
			Err(FsError::NotFound(_)) => {
				return ufo_err(format!(
					"No '{directory}' directory for layer '{layer_name}'"
				));
			}
			Err(e) => return Err(e.into()),
		};
		GlyphSet::from_fs(
			sub,
			GlyphSetOptions {
				glyph_name_to_file_name: None,
				ufo_format_version: Some(self.format_version.into()),
				validate_read,
				validate_write,
				expect_contents_file: true,
			},
		)
	}

	/// `getCharacterMapping`: Unicode values to the glyphs that have them.
	pub fn get_character_mapping(
		&self,
		layer_name: Option<&str>,
		validate: Option<bool>,
	) -> Result<IndexMap<i64, Vec<String>>> {
		let validate = self.validate_or_default(validate);
		let glyph_set = self.get_glyph_set(layer_name, Some(validate), Some(true))?;
		let mut cmap: IndexMap<i64, Vec<String>> = IndexMap::new();
		for (glyph_name, unicodes) in glyph_set.get_unicodes(None)? {
			for code in unicodes {
				cmap.entry(code).or_default().push(glyph_name.clone());
			}
		}
		Ok(cmap)
	}

	// /data

	/// `getDataDirectoryListing`: the paths of the files in the `data`
	/// directory, relative to it.
	pub fn get_data_directory_listing(&self) -> Result<Vec<String>> {
		let data_fs = match self.fs.opendir(DATA_DIRNAME) {
			Ok(fs) => fs,
			Err(FsError::NotFound(_)) => return Ok(Vec::new()),
			Err(FsError::DirectoryExpected(_)) => {
				return ufo_err("The UFO contains a \"data\" file instead of a directory.");
			}
			Err(e) => return Err(e.into()),
		};
		match data_fs.walk_files("/") {
			Ok(files) => Ok(files
				.into_iter()
				.map(|p| p.trim_start_matches('/').to_owned())
				.collect()),
			Err(_) => Ok(Vec::new()),
		}
	}

	/// `getImageDirectoryListing`: the file names of the images, checked to
	/// be PNG files if validating.
	pub fn get_image_directory_listing(&self, validate: Option<bool>) -> Result<Vec<String>> {
		if self.format_version < UfoFormatVersion::Format3_0 {
			return Ok(Vec::new());
		}
		let validate = self.validate_or_default(validate);
		let images_fs = match self.fs.opendir(IMAGES_DIRNAME) {
			Ok(fs) => fs,
			Err(FsError::NotFound(_)) => return Ok(Vec::new()),
			Err(FsError::DirectoryExpected(_)) => {
				return ufo_err("The UFO contains an \"images\" file instead of a directory.");
			}
			Err(e) => return Err(e.into()),
		};
		let mut result = Vec::new();
		for (name, is_dir) in images_fs.scandir("/")? {
			if is_dir {
				// silently skip this as version control systems often have hidden
				// directories
				continue;
			}
			if validate {
				if png_validator(&images_fs.readbytes(&name)?).is_ok() {
					result.push(name);
				}
			} else {
				result.push(name);
			}
		}
		Ok(result)
	}

	/// `readData`: the data of a file in the `data` directory.
	pub fn read_data(&self, file_name: &str) -> Result<Vec<u8>> {
		let read = || -> std::result::Result<Vec<u8>, FsError> {
			self.fs.opendir(DATA_DIRNAME)?.readbytes(file_name)
		};
		match read() {
			Ok(d) => Ok(d),
			Err(FsError::NotFound(_)) => {
				ufo_err(format!("No data file named '{file_name}' on {}", self.fs))
			}
			Err(e) => Err(e.into()),
		}
	}

	/// `readImage`: the data of an image, checked to be a PNG file if
	/// validating.
	pub fn read_image(&self, file_name: &str, validate: Option<bool>) -> Result<Vec<u8>> {
		let validate = self.validate_or_default(validate);
		if self.format_version < UfoFormatVersion::Format3_0 {
			return ufo_err(format!(
				"Reading images is not allowed in UFO {}.",
				self.format_version.major()
			));
		}
		let read = || -> std::result::Result<Vec<u8>, FsError> {
			self.fs.opendir(IMAGES_DIRNAME)?.readbytes(file_name)
		};
		let data = match read() {
			Ok(d) => d,
			Err(FsError::NotFound(_)) => {
				return ufo_err(format!("No image file named '{file_name}' on {}", self.fs));
			}
			Err(e) => return Err(e.into()),
		};
		if validate && let Err(message) = png_validator(&data) {
			return ufo_err(message);
		}
		Ok(data)
	}
}

// ----------
// UFO Writer
// ----------

/// The options of [`UfoWriter::new`].
#[derive(Debug, Clone)]
pub struct UfoWriterOptions {
	/// `formatVersion`; the latest by default.
	pub format_version: Option<FormatVersionInput>,
	/// `fileCreator`.
	pub file_creator: String,
	/// `structure`: the structure of a new UFO (a package by default); an
	/// existing UFO keeps its structure.
	pub structure: Option<UfoFileStructure>,
	/// `validate`.
	pub validate: bool,
}

impl Default for UfoWriterOptions {
	fn default() -> Self {
		UfoWriterOptions {
			format_version: None,
			file_creator: "com.github.fonttools.ufoLib".into(),
			structure: None,
			validate: true,
		}
	}
}

/// The options of [`UfoWriter::get_glyph_set`].
#[derive(Default)]
pub struct WriterGlyphSetOptions {
	/// `glyphNameToFileNameFunc`.
	pub glyph_name_to_file_name: Option<GlyphNameToFileNameFunc>,
	/// `validateRead`; the writer's by default.
	pub validate_read: Option<bool>,
	/// `validateWrite`; the writer's by default.
	pub validate_write: Option<bool>,
	/// `expectContentsFile`.
	pub expect_contents_file: bool,
}

impl std::fmt::Debug for WriterGlyphSetOptions {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("WriterGlyphSetOptions")
			.field("validate_read", &self.validate_read)
			.field("validate_write", &self.validate_write)
			.field("expect_contents_file", &self.expect_contents_file)
			.finish_non_exhaustive()
	}
}

/// `UFOWriter` (also `UFOReaderWriter`): writes the parts of a UFO, and reads
/// them as a [`UfoReader`] does (through `Deref`).
///
/// A zipped UFO is written when the writer is closed ([`UfoWriter::close`],
/// or when it is dropped).
#[derive(Debug)]
pub struct UfoWriter {
	reader: UfoReader,
	file_creator: String,
	down_conversion_group_rename_map: Option<IndexMap<String, String>>,
	/// `layerContents`: layer names to glyph set directories.
	pub layer_contents: IndexMap<String, String>,
	closed: bool,
}

impl std::ops::Deref for UfoWriter {
	type Target = UfoReader;

	fn deref(&self) -> &UfoReader {
		&self.reader
	}
}

impl UfoWriter {
	/// `UFOWriter(path, formatVersion, fileCreator, structure, validate)`.
	pub fn new(path: impl AsRef<Path>, options: UfoWriterOptions) -> Result<UfoWriter> {
		let format_version =
			UfoFormatVersion::normalize(options.format_version).ok_or_else(|| {
				UfoLibError::UnsupportedUfoFormat(format!(
					"Unsupported UFO format: {}",
					options
						.format_version
						.map_or("None".to_owned(), |v| v.to_string())
				))
			})?;
		// normalize path by removing trailing or double slashes
		let path = normpath(path.as_ref());
		let have_previous_file = path.exists();
		let structure = if have_previous_file {
			// ensure we use the same structure as the destination
			let existing = sniff_file_structure(&path)?;
			if let Some(structure) = options.structure
				&& structure != existing
			{
				return ufo_err(format!(
					"A UFO with a different structure ({existing}) already exists at the given path: '{}'",
					path.display()
				));
			}
			existing
		} else {
			if let Some(dir) = path.parent()
				&& !dir.as_os_str().is_empty()
				&& !dir.is_dir()
			{
				return ufo_err(format!(
					"Cannot write to '{}': directory does not exist",
					path.display()
				));
			}
			options.structure.unwrap_or(UfoFileStructure::Package)
		};
		let path_str = path.to_string_lossy().into_owned();
		let fs = match structure {
			UfoFileStructure::Zip => {
				if have_previous_file {
					// we can't write a zip in-place, so we have to copy its contents to
					// a temporary location and work from there, then upon closing
					// UFOWriter we create the final zip file
					let data = std::fs::read(&path)?;
					let mut tree = zip::read_zip(&data, String::new())?;
					tree.read_only = false;
					tree.label = "<tempfs>".into();
					let parent = Fs::mem(tree);
					let root = zip_root_dir(&parent)?;
					parent.opendir(&root)?
				} else {
					let root = zip_root_name(&path_str);
					let parent = Fs::mem(MemFs {
						label: format!("<zipfs-write '{path_str}'>"),
						..Default::default()
					});
					parent.makedir(&root, false)?
				}
			}
			UfoFileStructure::Package => Fs::os(&path, true)?,
		};
		let mut writer = UfoWriter {
			reader: UfoReader {
				fs,
				path: path_str,
				validate: options.validate,
				format_version,
				file_structure: structure,
				have_previous_file,
				up_converted_kerning_data: RefCell::new(None),
			},
			file_creator: options.file_creator,
			down_conversion_group_rename_map: None,
			layer_contents: IndexMap::new(),
			closed: false,
		};
		// if the file already exists, get the format version.
		// this will be needed for up and down conversion.
		let mut previous_format_version = None;
		if have_previous_file {
			let previous = writer
				.reader
				.read_meta_info_version(Some(options.validate))?;
			// catch down conversion
			if previous > format_version {
				return Err(UfoLibError::UnsupportedUfoFormat(format!(
					"The UFO located at this path is a higher version ({previous}) than the version ({format_version}) that is trying to be written. This is not supported."
				)));
			}
			previous_format_version = Some(previous);
		}
		// handle the layer contents
		if previous_format_version.is_some_and(|v| v.major() >= 3) {
			writer.layer_contents = writer
				.reader
				.read_layer_contents(options.validate)?
				.into_iter()
				.collect();
		} else if writer.reader.fs.exists(DEFAULT_GLYPHS_DIRNAME)? {
			// previous < 3: imply the layer contents
			writer
				.layer_contents
				.insert(DEFAULT_LAYER_NAME.into(), DEFAULT_GLYPHS_DIRNAME.into());
		}
		// write the new metainfo
		writer.write_meta_info()?;
		Ok(writer)
	}

	fn fs(&self) -> &Fs {
		&self.reader.fs
	}

	fn have_previous_file(&self) -> bool {
		self.reader.have_previous_file
	}

	fn validate_or_default(&self, validate: Option<bool>) -> bool {
		validate.unwrap_or(self.reader.validate)
	}

	fn format_version(&self) -> UfoFormatVersion {
		self.reader.format_version
	}

	fn write_plist(&self, file_name: &str, obj: &Value) -> Result<()> {
		write_plist(self.fs(), self.have_previous_file(), file_name, obj)
	}

	/// `fileCreator`.
	pub fn file_creator(&self) -> &str {
		&self.file_creator
	}

	// support methods for file system interaction

	/// `copyFromReader`: copies a file or directory of a reader to this UFO.
	pub fn copy_from_reader(
		&self,
		reader: &UfoReader,
		source_path: &str,
		dest_path: &str,
	) -> Result<()> {
		if !reader.fs.exists(source_path)? {
			return ufo_err(format!(
				"The reader does not have data located at \"{source_path}\"."
			));
		}
		if self.fs().exists(dest_path)? {
			return ufo_err(format!("A file named \"{dest_path}\" already exists."));
		}
		// create the destination directory if it doesn't exist
		self.fs().makedirs(&dirname(dest_path), true)?;
		self.fs().copy_from(&reader.fs, source_path, dest_path)?;
		Ok(())
	}

	/// `writeBytesToPath`: writes a file, creating its directory if needed,
	/// unless an existing UFO already has the same data there.
	pub fn write_bytes_to_path(&self, path: &str, data: &[u8]) -> Result<()> {
		if self.have_previous_file()
			&& self.fs().isfile(path)?
			&& self.fs().readbytes(path)? == data
		{
			return Ok(());
		}
		match self.fs().writebytes(path, data) {
			Ok(()) => Ok(()),
			Err(FsError::FileExpected(_)) => ufo_err(format!("A directory exists at '{path}'")),
			Err(FsError::NotFound(_)) => {
				self.fs().makedirs(&dirname(path), true)?;
				Ok(self.fs().writebytes(path, data)?)
			}
			Err(e) => Err(e.into()),
		}
	}

	/// `getFileObjectForPath`: a file of the UFO, opened with a mode of
	/// `open()` (`"r"`, `"w"` or `"a"`, optionally with `"b"`); `None` if a
	/// file to read does not exist. Written data is stored when the file is
	/// closed or dropped.
	pub fn get_file_object_for_path(&self, path: &str, mode: &str) -> Result<Option<UfoFile>> {
		let m = mode.chars().next().unwrap_or('r');
		let existing = match self.fs().readbytes(path) {
			Ok(d) => Some(d),
			Err(FsError::NotFound(_)) => None,
			Err(e) => {
				return Err(UfoLibError::UfoLib(format!(
					"unable to open '{path}' on {}: {e}",
					self.fs()
				)));
			}
		};
		match m {
			'r' => Ok(existing.map(|d| UfoFile {
				fs: self.fs().clone(),
				path: path.to_owned(),
				data: std::io::Cursor::new(d),
				dirty: false,
			})),
			'w' | 'a' | 'x' => {
				if m == 'x' && existing.is_some() {
					return ufo_err(format!(
						"unable to open '{path}' on {}: file exists",
						self.fs()
					));
				}
				let data = if m == 'a' {
					existing.unwrap_or_default()
				} else {
					Vec::new()
				};
				if let Err(FsError::NotFound(_)) = self.fs().writebytes(path, &data) {
					self.fs().makedirs(&dirname(path), true)?;
					self.fs().writebytes(path, &data)?;
				}
				let len = data.len() as u64;
				let mut cursor = std::io::Cursor::new(data);
				cursor.set_position(len);
				Ok(Some(UfoFile {
					fs: self.fs().clone(),
					path: path.to_owned(),
					data: cursor,
					dirty: false,
				}))
			}
			_ => Err(UfoLibError::Value(format!("invalid mode: '{mode}'"))),
		}
	}

	/// `removePath` (also `removeFileForPath`): removes a file or directory,
	/// and the directories left empty above it if `remove_empty_parents`.
	/// A missing path is an error unless `force`.
	pub fn remove_path(&self, path: &str, force: bool, remove_empty_parents: bool) -> Result<()> {
		match self.fs().remove(path) {
			Ok(()) => {}
			Err(FsError::FileExpected(_)) => self.fs().removetree(path)?,
			Err(FsError::NotFound(_)) => {
				if !force {
					return ufo_err(format!("'{path}' does not exist on {}", self.fs()));
				}
			}
			Err(e) => return Err(e.into()),
		}
		if remove_empty_parents {
			let mut parent = dirname(path);
			// `fs.tools.remove_empty`: removes empty parents up to the root
			while !parent.is_empty() && parent != "/" {
				match self.fs().removedir(&parent) {
					Ok(()) => {}
					Err(FsError::DirectoryNotEmpty(_)) => break,
					Err(e) => return Err(e.into()),
				}
				parent = dirname(&parent);
			}
		}
		Ok(())
	}

	/// `removeFileForPath`: [`UfoWriter::remove_path`].
	pub fn remove_file_for_path(
		&self,
		path: &str,
		force: bool,
		remove_empty_parents: bool,
	) -> Result<()> {
		self.remove_path(path, force, remove_empty_parents)
	}

	// UFO mod time

	/// `setModificationTime`: sets the modification time of the UFO to now.
	pub fn set_modification_time(&self) {
		let path = Path::new(&self.reader.path);
		if path.exists()
			&& let Err(e) = touch(path)
		{
			log::warn!("Failed to set modified time: {e}");
		}
	}

	// metainfo.plist

	fn write_meta_info(&self) -> Result<()> {
		let mut meta = Dict::new();
		meta.insert("creator".into(), Value::String(self.file_creator.clone()));
		meta.insert(
			"formatVersion".into(),
			Value::Integer(i128::from(self.format_version().major())),
		);
		if self.format_version().minor() != 0 {
			meta.insert(
				"formatVersionMinor".into(),
				Value::Integer(i128::from(self.format_version().minor())),
			);
		}
		self.write_plist(METAINFO_FILENAME, &Value::Dict(meta))
	}

	// groups.plist

	/// `setKerningGroupConversionRenameMaps`: the renaming to undo when
	/// writing the groups and kerning of a UFO 1 or 2 (the form returned by
	/// [`UfoReader::get_kerning_group_conversion_rename_maps`]).
	pub fn set_kerning_group_conversion_rename_maps(&mut self, maps: &GroupRenameMaps) {
		if self.format_version() >= UfoFormatVersion::Format3_0 {
			return;
		}
		let mut remap = IndexMap::new();
		for side in [&maps.side1, &maps.side2] {
			for (write_name, data_name) in side {
				remap.insert(data_name.clone(), write_name.clone());
			}
		}
		self.down_conversion_group_rename_map = Some(remap);
	}

	/// `writeGroups`: writes `groups.plist` (or removes it if there are no
	/// groups).
	pub fn write_groups(&self, groups: &Groups, validate: Option<bool>) -> Result<()> {
		let validate = self.validate_or_default(validate);
		if validate && let Err(message) = groups_validator(&groups_to_value(groups)) {
			return ufo_err(message);
		}
		let mut groups = groups.clone();
		// down convert
		if self.format_version() < UfoFormatVersion::Format3_0
			&& let Some(remap) = &self.down_conversion_group_rename_map
		{
			let mut remapped = Groups::new();
			for (name, contents) in &groups {
				if !remap.contains_key(name) {
					remapped.insert(name.clone(), contents.clone());
				}
			}
			for (name, contents) in &groups {
				if let Some(new_name) = remap.get(name) {
					remapped.insert(new_name.clone(), contents.clone());
				}
			}
			groups = remapped;
		}
		if !groups.is_empty() {
			self.write_plist(GROUPS_FILENAME, &groups_to_value(&groups))
		} else if self.have_previous_file() {
			self.remove_path(GROUPS_FILENAME, true, false)
		} else {
			Ok(())
		}
	}

	// fontinfo.plist

	/// `writeInfo`: writes the font info attributes (fontinfo.plist version 3
	/// names) to `fontinfo.plist`, converted to the UFO's version.
	pub fn write_info(&self, info: &Dict, validate: Option<bool>) -> Result<()> {
		let validate = self.validate_or_default(validate);
		// gather version 3 data
		let mut info_data = Dict::new();
		for (attr, _) in fontinfo::font_info_attributes_version3_value_data() {
			if let Some(value) = info.get(attr) {
				info_data.insert(attr.to_owned(), value.clone());
			}
		}
		// down convert data if necessary and validate
		let data: IndexMap<String, Option<Value>> = match self.format_version() {
			UfoFormatVersion::Format3_0 => {
				if validate {
					info_data = fontinfo::validate_info_version3_data(&info_data)?;
				}
				info_data.into_iter().map(|(k, v)| (k, Some(v))).collect()
			}
			UfoFormatVersion::Format2_0 => {
				info_data = fontinfo::convert_font_info_data_version3_to_version2(&info_data);
				if validate {
					info_data = fontinfo::validate_info_version2_data(&info_data)?;
				}
				info_data.into_iter().map(|(k, v)| (k, Some(v))).collect()
			}
			UfoFormatVersion::Format1_0 => {
				info_data = fontinfo::convert_font_info_data_version3_to_version2(&info_data);
				if validate {
					info_data = fontinfo::validate_info_version2_data(&info_data)?;
				}
				fontinfo::convert_font_info_data_version2_to_version1(&info_data)
			}
		};
		// write file if there is anything to write
		if data.is_empty() {
			return Ok(());
		}
		if let Some((attr, _)) = data.iter().find(|(_, v)| v.is_none()) {
			// A value without a version 1 equivalent is `None`, which plistlib
			// cannot write.
			if !self.have_previous_file() {
				self.fs().writebytes(FONTINFO_FILENAME, b"")?;
			}
			return ufo_err(format!(
				"'{FONTINFO_FILENAME}' could not be written on {} because the data is not properly formatted: unsupported type: <class 'NoneType'> (for {attr})",
				self.fs()
			));
		}
		let dict: Dict = data
			.into_iter()
			.filter_map(|(k, v)| v.map(|v| (k, v)))
			.collect();
		self.write_plist(FONTINFO_FILENAME, &Value::Dict(dict))
	}

	// kerning.plist

	/// `writeKerning`: writes `kerning.plist` (or removes it if there are no
	/// pairs). The values must be numbers when validating.
	pub fn write_kerning(&self, kerning: &Kerning, validate: Option<bool>) -> Result<()> {
		let validate = self.validate_or_default(validate);
		if validate && !kerning.values().all(crate::utils::is_number) {
			return ufo_err("The kerning is not properly formatted.");
		}
		let mut kerning = kerning.clone();
		// down convert
		if self.format_version() < UfoFormatVersion::Format3_0
			&& let Some(remap) = &self.down_conversion_group_rename_map
		{
			let mut remapped = Kerning::new();
			for ((side1, side2), value) in &kerning {
				let side1 = remap.get(side1).unwrap_or(side1).clone();
				let side2 = remap.get(side2).unwrap_or(side2).clone();
				remapped.insert((side1, side2), value.clone());
			}
			kerning = remapped;
		}
		// pack and write
		let mut nested = KerningNested::new();
		for ((left, right), value) in kerning {
			nested.entry(left).or_default().insert(right, value);
		}
		if !nested.is_empty() {
			self.write_plist(KERNING_FILENAME, &kerning_nested_to_value(&nested))
		} else if self.have_previous_file() {
			self.remove_path(KERNING_FILENAME, true, false)
		} else {
			Ok(())
		}
	}

	// lib.plist

	/// `writeLib`: writes `lib.plist` (or removes it if the lib is empty).
	pub fn write_lib(&self, lib: &Dict, validate: Option<bool>) -> Result<()> {
		let validate = self.validate_or_default(validate);
		if validate && let Err(message) = font_lib_dict_validator(lib) {
			return ufo_err(message);
		}
		if !lib.is_empty() {
			self.write_plist(LIB_FILENAME, &Value::Dict(lib.clone()))
		} else if self.have_previous_file() {
			self.remove_path(LIB_FILENAME, true, false)
		} else {
			Ok(())
		}
	}

	// features.fea

	/// `writeFeatures`: writes `features.fea` (or removes it if empty).
	pub fn write_features(&self, features: &str, _validate: Option<bool>) -> Result<()> {
		if self.format_version() == UfoFormatVersion::Format1_0 {
			return ufo_err("features.fea is not allowed in UFO Format Version 1.");
		}
		if !features.is_empty() {
			self.write_bytes_to_path(FEATURES_FILENAME, features.as_bytes())
		} else if self.have_previous_file() {
			self.remove_path(FEATURES_FILENAME, true, false)
		} else {
			Ok(())
		}
	}

	// glyph sets & layers

	/// `writeLayerContents`: writes `layercontents.plist`, with the layers in
	/// `layer_order` (`None` entries are the default layer) or in the order
	/// they were created. Call it after writing all glyph sets.
	pub fn write_layer_contents(
		&self,
		layer_order: Option<&[Option<&str>]>,
		validate: Option<bool>,
	) -> Result<()> {
		let validate = self.validate_or_default(validate);
		if self.format_version() < UfoFormatVersion::Format3_0 {
			return Ok(());
		}
		let layer_order: Vec<String> = match layer_order {
			Some(order) => order
				.iter()
				.map(|n| n.unwrap_or(DEFAULT_LAYER_NAME).to_owned())
				.collect(),
			None => self.layer_contents.keys().cloned().collect(),
		};
		if validate {
			let a: HashSet<&String> = layer_order.iter().collect();
			let b: HashSet<&String> = self.layer_contents.keys().collect();
			if a != b {
				return ufo_err(
					"The layer order content does not match the glyph sets that have been created.",
				);
			}
		}
		let mut contents = Vec::new();
		for name in layer_order {
			let directory = self
				.layer_contents
				.get(&name)
				.ok_or_else(|| UfoLibError::Key(fontoxide_misc::py::str_repr(&name)))?;
			contents.push(Value::Array(vec![
				Value::String(name.clone()),
				Value::String(directory.clone()),
			]));
		}
		self.write_plist(LAYERCONTENTS_FILENAME, &Value::Array(contents))
	}

	fn find_directory_for_layer_name(&self, layer_name: Option<&str>) -> Result<String> {
		for (existing, directory) in &self.layer_contents {
			if layer_name.is_none() && directory == DEFAULT_GLYPHS_DIRNAME {
				return Ok(directory.clone());
			}
			if Some(existing.as_str()) == layer_name {
				if directory.is_empty() {
					break;
				}
				return Ok(directory.clone());
			}
		}
		ufo_err(format!(
			"Could not locate a glyph set directory for the layer named {}.",
			layer_name.unwrap_or("None")
		))
	}

	/// `getGlyphSet`: the glyph set of a layer, created if needed. `None` is
	/// the default layer; `default_layer` tells whether the layer is stored
	/// in the default `glyphs` directory.
	pub fn get_glyph_set(
		&mut self,
		layer_name: Option<&str>,
		default_layer: bool,
		options: WriterGlyphSetOptions,
	) -> Result<GlyphSet> {
		let validate_read = self.validate_or_default(options.validate_read);
		let validate_write = self.validate_or_default(options.validate_write);
		// only default can be written in < 3
		if self.format_version() < UfoFormatVersion::Format3_0
			&& (!default_layer || layer_name.is_some())
		{
			return ufo_err(format!(
				"Only the default layer can be writen in UFO {}.",
				self.format_version().major()
			));
		}
		// locate a layer name when None has been given
		let layer_name: String = match layer_name {
			Some(n) => n.to_owned(),
			None if default_layer => {
				let mut found = None;
				for (existing, directory) in &self.layer_contents {
					if directory == DEFAULT_GLYPHS_DIRNAME {
						found = Some(existing.clone());
					}
				}
				found.unwrap_or_else(|| DEFAULT_LAYER_NAME.to_owned())
			}
			None => return ufo_err("A layer name must be provided for non-default layers."),
		};
		let glyph_set_options = |validate_read, validate_write| GlyphSetOptions {
			glyph_name_to_file_name: options.glyph_name_to_file_name,
			ufo_format_version: Some(self.format_version().into()),
			validate_read,
			validate_write,
			expect_contents_file: options.expect_contents_file,
		};
		if self.format_version() < UfoFormatVersion::Format3_0 {
			let sub = self.fs().makedir(DEFAULT_GLYPHS_DIRNAME, true)?;
			return GlyphSet::from_fs(sub, glyph_set_options(validate_read, validate_write));
		}
		// if the default flag is on, make sure that the default in the file
		// matches the default being written. also make sure that this layer
		// name is not already linked to a non-default layer.
		if default_layer {
			for (existing, directory) in &self.layer_contents {
				if directory == DEFAULT_GLYPHS_DIRNAME {
					if *existing != layer_name {
						return ufo_err(format!(
							"Another layer ('{existing}') is already mapped to the default directory."
						));
					}
				} else if *existing == layer_name {
					return ufo_err("The layer name is already mapped to a non-default layer.");
				}
			}
		}
		let directory = match self.layer_contents.get(&layer_name) {
			Some(d) => d.clone(),
			None => {
				if default_layer {
					DEFAULT_GLYPHS_DIRNAME.to_owned()
				} else {
					let existing: HashSet<String> =
						self.layer_contents.values().map(|d| lower(d)).collect();
					user_name_to_file_name(&layer_name, &existing, "glyphs.", "")?
				}
			}
		};
		let sub = self.fs().makedir(&directory, true)?;
		let glyph_set_options = glyph_set_options(validate_read, validate_write);
		self.layer_contents.insert(layer_name, directory);
		GlyphSet::from_fs(sub, glyph_set_options)
	}

	/// `renameGlyphSet`: renames a layer (`None` is the default layer), and
	/// moves it into the default directory if `default_layer`.
	pub fn rename_glyph_set(
		&mut self,
		layer_name: Option<&str>,
		new_layer_name: Option<&str>,
		default_layer: bool,
	) -> Result<()> {
		if self.format_version() < UfoFormatVersion::Format3_0 {
			return Ok(());
		}
		let mut new_layer_name = new_layer_name.map(str::to_owned);
		if let Some(name) = layer_name
			&& Some(name) == new_layer_name.as_deref()
		{
			let directory = self
				.layer_contents
				.get(name)
				.ok_or_else(|| UfoLibError::Key(fontoxide_misc::py::str_repr(name)))?;
			// if the default is off and the layer is already not the default, skip
			if directory != DEFAULT_GLYPHS_DIRNAME && !default_layer {
				return Ok(());
			}
			// if the default is on and the layer is already the default, skip
			if directory == DEFAULT_GLYPHS_DIRNAME && default_layer {
				return Ok(());
			}
		} else {
			// make sure the new layer name doesn't already exist
			let name = new_layer_name.get_or_insert_with(|| DEFAULT_LAYER_NAME.to_owned());
			if self.layer_contents.contains_key(name.as_str()) {
				return ufo_err(format!("A layer named {name} already exists."));
			}
			// make sure the default layer doesn't already exist
			if default_layer
				&& self
					.layer_contents
					.values()
					.any(|d| d == DEFAULT_GLYPHS_DIRNAME)
			{
				return ufo_err("A default layer already exists.");
			}
		}
		let new_layer_name = new_layer_name.unwrap_or_else(|| DEFAULT_LAYER_NAME.to_owned());
		// get the paths
		let old_directory = self.find_directory_for_layer_name(layer_name)?;
		let new_directory = if default_layer {
			DEFAULT_GLYPHS_DIRNAME.to_owned()
		} else {
			let existing: HashSet<String> =
				self.layer_contents.values().map(|d| lower(d)).collect();
			user_name_to_file_name(&new_layer_name, &existing, "glyphs.", "")?
		};
		// update the internal mapping
		if let Some(name) = layer_name {
			self.layer_contents.shift_remove(name);
		}
		self.layer_contents
			.insert(new_layer_name, new_directory.clone());
		// do the file system copy
		self.fs().movedir(&old_directory, &new_directory)?;
		Ok(())
	}

	/// `deleteGlyphSet`: removes a layer (`None` is the default layer).
	pub fn delete_glyph_set(&mut self, layer_name: Option<&str>) -> Result<()> {
		if self.format_version() < UfoFormatVersion::Format3_0 {
			return Ok(());
		}
		let directory = self.find_directory_for_layer_name(layer_name)?;
		self.remove_path(&directory, false, false)?;
		if let Some(name) = layer_name {
			self.layer_contents.shift_remove(name);
		}
		Ok(())
	}

	/// `writeData`: writes a file in the `data` directory.
	pub fn write_data(&self, file_name: &str, data: &[u8]) -> Result<()> {
		self.write_bytes_to_path(&format!("{DATA_DIRNAME}/{file_name}"), data)
	}

	/// `removeData`: removes a file from the `data` directory.
	pub fn remove_data(&self, file_name: &str) -> Result<()> {
		self.remove_path(&format!("{DATA_DIRNAME}/{file_name}"), false, true)
	}

	// /images

	/// `writeImage`: writes a PNG image to the `images` directory.
	pub fn write_image(&self, file_name: &str, data: &[u8], validate: Option<bool>) -> Result<()> {
		let validate = self.validate_or_default(validate);
		if self.format_version() < UfoFormatVersion::Format3_0 {
			return ufo_err(format!(
				"Images are not allowed in UFO {}.",
				self.format_version().major()
			));
		}
		if validate && let Err(message) = png_validator(data) {
			return ufo_err(message);
		}
		self.write_bytes_to_path(&format!("{IMAGES_DIRNAME}/{file_name}"), data)
	}

	/// `removeImage`: removes an image from the `images` directory.
	pub fn remove_image(&self, file_name: &str) -> Result<()> {
		if self.format_version() < UfoFormatVersion::Format3_0 {
			return ufo_err(format!(
				"Images are not allowed in UFO {}.",
				self.format_version().major()
			));
		}
		self.remove_path(&format!("{IMAGES_DIRNAME}/{file_name}"), false, true)
	}

	/// `copyImageFromReader`: copies an image of a reader to this UFO.
	pub fn copy_image_from_reader(
		&self,
		reader: &UfoReader,
		source_file_name: &str,
		dest_file_name: &str,
	) -> Result<()> {
		if self.format_version() < UfoFormatVersion::Format3_0 {
			return ufo_err(format!(
				"Images are not allowed in UFO {}.",
				self.format_version().major()
			));
		}
		self.copy_from_reader(
			reader,
			&format!("{IMAGES_DIRNAME}/{source_file_name}"),
			&format!("{IMAGES_DIRNAME}/{dest_file_name}"),
		)
	}

	/// `close`: writes a zipped UFO. Called on drop if not called before.
	pub fn close(&mut self) -> Result<()> {
		if self.closed {
			return Ok(());
		}
		self.closed = true;
		if self.reader.file_structure == UfoFileStructure::Zip
			&& let Some(tree) = self.fs().mem_tree()
		{
			let root = zip_root_name(&self.reader.path);
			let tree = tree.lock().unwrap_or_else(|e| e.into_inner());
			let data = zip::write_zip(&tree, self.fs().prefix(), Some(&root))?;
			std::fs::write(&self.reader.path, data)?;
		}
		Ok(())
	}
}

impl Drop for UfoWriter {
	fn drop(&mut self) {
		if let Err(e) = self.close() {
			log::warn!("Failed to close the UFO writer: {e}");
		}
	}
}

/// `os.path.normpath`.
fn normpath(path: &Path) -> PathBuf {
	let mut out = PathBuf::new();
	for c in path.components() {
		match c {
			std::path::Component::CurDir => {}
			std::path::Component::ParentDir => {
				if !out.pop() {
					out.push("..");
				}
			}
			other => out.push(other.as_os_str()),
		}
	}
	if out.as_os_str().is_empty() {
		out.push(".");
	}
	out
}

/// `os.utime(path, None)`.
fn touch(path: &Path) -> std::io::Result<()> {
	let mut options = std::fs::OpenOptions::new();
	if path.is_dir() {
		#[cfg(windows)]
		{
			use std::os::windows::fs::OpenOptionsExt;
			// FILE_FLAG_BACKUP_SEMANTICS, needed to open a directory.
			options.write(true).custom_flags(0x0200_0000);
		}
		#[cfg(not(windows))]
		{
			options.read(true);
		}
	} else {
		options.write(true);
	}
	let file = options.open(path)?;
	file.set_modified(std::time::SystemTime::now())
}

/// A file of a UFO opened by [`UfoWriter::get_file_object_for_path`]: it reads
/// and writes in memory, and stores written data in the UFO when closed or
/// dropped.
#[derive(Debug)]
pub struct UfoFile {
	fs: Fs,
	path: String,
	data: std::io::Cursor<Vec<u8>>,
	dirty: bool,
}

impl UfoFile {
	/// Stores the data written in the UFO.
	pub fn close(mut self) -> Result<()> {
		self.flush_to_fs()
	}

	fn flush_to_fs(&mut self) -> Result<()> {
		if self.dirty {
			self.fs.writebytes(&self.path, self.data.get_ref())?;
			self.dirty = false;
		}
		Ok(())
	}
}

impl Read for UfoFile {
	fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
		self.data.read(buf)
	}
}

impl Write for UfoFile {
	fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
		self.dirty = true;
		self.data.write(buf)
	}

	fn flush(&mut self) -> std::io::Result<()> {
		self.flush_to_fs()
			.map_err(|e| std::io::Error::other(e.to_string()))
	}
}

impl Drop for UfoFile {
	fn drop(&mut self) {
		if let Err(e) = self.flush_to_fs() {
			log::warn!("Failed to write '{}': {e}", self.path);
		}
	}
}
