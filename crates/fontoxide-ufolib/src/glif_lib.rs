//! Port of `fontTools.ufoLib.glifLib`: reading and writing `.glif` files and
//! glyph sets (directories of them).
//!
//! fontTools reads glyph data into, and writes it from, arbitrary objects
//! through `setattr` and `getattr`; here such objects implement
//! [`GlyphObject`], with attribute values as plist [`Value`]s. [`Glyph`] is a
//! plain implementation.
//!
//! Outlines go through point pens. GLIF keeps Python's distinction between
//! `int` and `float` coordinates (`1` and `1.0` are written differently), so
//! the GLIF side uses [`NumberPointPen`]; [`PointPenAdapter`] connects it to a
//! [`fontoxide_pens::PointPen`], and the [`GlifPointPen`] that writes outlines
//! takes both.

use std::collections::HashSet;
use std::path::Path;

use fontoxide_misc::etree::{self, Element};
use fontoxide_misc::plistlib::{self, Dict, Value};
use fontoxide_misc::py;
use fontoxide_misc::xml_reader;
use fontoxide_pens::{Pen, PointPen, PointToSegmentPen, SegmentType, Transform};
use indexmap::IndexMap;

use crate::base_io::{file_modification_time, get_plist, write_plist};
use crate::errors::{Result, UfoLibError, glif_err};
use crate::filenames::user_name_to_file_name;
use crate::fs::{Fs, FsError, basename};
use crate::utils::{
	FormatVersionInput, GlifFormatVersion, Number, UfoFormatVersion, is_int, is_number,
	parse_float, parse_hex, parse_int, truthy, value_repr,
};
use crate::validators::{
	TypeSpec, anchors_validator, color_validator, generic_type_validator, glyph_lib_validator,
	guidelines_validator, image_validator, is_valid_identifier,
};

/// `CONTENTS_FILENAME`.
pub const CONTENTS_FILENAME: &str = "contents.plist";
/// `LAYERINFO_FILENAME`.
pub const LAYERINFO_FILENAME: &str = "layerinfo.plist";

// ------------
// Glyph objects
// ------------

/// An object that glyph data is read into and written from (fontTools takes
/// any object and uses `setattr` and `getattr`).
///
/// The attributes are `name`, `width`, `height`, `unicodes` (a list of
/// `int`s), `note`, `lib` (a dictionary), `image` (a dictionary),
/// `guidelines` and `anchors` (lists of dictionaries).
pub trait GlyphObject {
	/// `getattr(glyph, attr, None)`.
	fn get_attr(&self, attr: &str) -> Option<Value> {
		let _ = attr;
		None
	}

	/// `setattr(glyph, attr, value)`; an attribute that cannot be set is
	/// ignored (`_relaxedSetattr`).
	fn set_attr(&mut self, attr: &str, value: Value) {
		let _ = (attr, value);
	}
}

/// A glyph's data, as read from a `.glif` file (the attributes that
/// `readGlyph` sets on fontTools' minimal `Glyph` object).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Glyph {
	/// `name`.
	pub name: Option<String>,
	/// `width`, the advance width.
	pub width: Option<Number>,
	/// `height`, the advance height.
	pub height: Option<Number>,
	/// `unicodes`.
	pub unicodes: Option<Vec<i64>>,
	/// `note`.
	pub note: Option<String>,
	/// `lib`.
	pub lib: Option<Dict>,
	/// `image`: `fileName`, the transformation and `color`.
	pub image: Option<Dict>,
	/// `guidelines`.
	pub guidelines: Option<Vec<Dict>>,
	/// `anchors`.
	pub anchors: Option<Vec<Dict>>,
}

fn dict_list(value: Value) -> Option<Vec<Dict>> {
	match value {
		Value::Array(a) => a
			.into_iter()
			.map(|v| match v {
				Value::Dict(d) => Some(d),
				_ => None,
			})
			.collect(),
		_ => None,
	}
}

impl GlyphObject for Glyph {
	fn get_attr(&self, attr: &str) -> Option<Value> {
		match attr {
			"name" => self.name.clone().map(Value::String),
			"width" => self.width.map(Number::to_value),
			"height" => self.height.map(Number::to_value),
			"unicodes" => self
				.unicodes
				.as_ref()
				.map(|u| Value::Array(u.iter().map(|&c| Value::Integer(i128::from(c))).collect())),
			"note" => self.note.clone().map(Value::String),
			"lib" => self.lib.clone().map(Value::Dict),
			"image" => self.image.clone().map(Value::Dict),
			"guidelines" => self
				.guidelines
				.as_ref()
				.map(|g| Value::Array(g.iter().cloned().map(Value::Dict).collect())),
			"anchors" => self
				.anchors
				.as_ref()
				.map(|g| Value::Array(g.iter().cloned().map(Value::Dict).collect())),
			_ => None,
		}
	}

	fn set_attr(&mut self, attr: &str, value: Value) {
		match attr {
			"name" => {
				if let Value::String(s) = value {
					self.name = Some(s);
				}
			}
			"width" => self.width = Number::from_value(&value).or(self.width),
			"height" => self.height = Number::from_value(&value).or(self.height),
			"unicodes" => {
				if let Value::Array(a) = value {
					self.unicodes = Some(
						a.iter()
							.filter_map(|v| match v {
								Value::Integer(i) => i64::try_from(*i).ok(),
								_ => None,
							})
							.collect(),
					);
				}
			}
			"note" => {
				if let Value::String(s) = value {
					self.note = Some(s);
				}
			}
			"lib" => {
				if let Value::Dict(d) = value {
					self.lib = Some(d);
				}
			}
			"image" => {
				if let Value::Dict(d) = value {
					self.image = Some(d);
				}
			}
			"guidelines" => self.guidelines = dict_list(value).or(self.guidelines.take()),
			"anchors" => self.anchors = dict_list(value).or(self.anchors.take()),
			_ => {}
		}
	}
}

impl GlyphObject for Dict {
	fn get_attr(&self, attr: &str) -> Option<Value> {
		self.get(attr).cloned()
	}

	fn set_attr(&mut self, attr: &str, value: Value) {
		self.insert(attr.to_owned(), value);
	}
}

fn relaxed_setattr(glyph: Option<&mut (dyn GlyphObject + '_)>, attr: &str, value: Value) {
	if let Some(g) = glyph {
		g.set_attr(attr, value);
	}
}

fn getattr(glyph: Option<&dyn GlyphObject>, attr: &str) -> Option<Value> {
	glyph.and_then(|g| g.get_attr(attr))
}

// ----------
// Point pens
// ----------

/// The point pen protocol with Python numbers, so that `int` and `float`
/// coordinates stay apart (`AbstractPointPen` as glifLib uses it).
pub trait NumberPointPen {
	/// `beginPath(identifier=...)`.
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()>;

	/// `endPath()`.
	fn end_path(&mut self) -> Result<()>;

	/// `addPoint(pt, segmentType, smooth, name, identifier)`.
	fn add_point(
		&mut self,
		pt: (Number, Number),
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()>;

	/// `addComponent(baseGlyphName, transformation, identifier=...)`.
	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: [Number; 6],
		identifier: Option<&str>,
	) -> Result<()>;
}

/// A [`NumberPointPen`] that draws into a [`PointPen`], with the numbers as
/// `f64`s.
pub struct PointPenAdapter<'p>(pub &'p mut dyn PointPen);

impl std::fmt::Debug for PointPenAdapter<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("PointPenAdapter")
	}
}

impl NumberPointPen for PointPenAdapter<'_> {
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()> {
		Ok(self.0.begin_path(identifier)?)
	}

	fn end_path(&mut self) -> Result<()> {
		Ok(self.0.end_path()?)
	}

	fn add_point(
		&mut self,
		pt: (Number, Number),
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		Ok(self.0.add_point(
			(pt.0.to_f64(), pt.1.to_f64()),
			segment_type,
			smooth,
			name,
			identifier,
		)?)
	}

	fn add_component(
		&mut self,
		base_glyph_name: &str,
		t: [Number; 6],
		identifier: Option<&str>,
	) -> Result<()> {
		let t = Transform::new(
			t[0].to_f64(),
			t[1].to_f64(),
			t[2].to_f64(),
			t[3].to_f64(),
			t[4].to_f64(),
			t[5].to_f64(),
		);
		Ok(self.0.add_component(base_glyph_name, t, identifier)?)
	}
}

// ---------
// Glyph Set
// ---------

/// The function that draws a glyph's outline into the GLIF point pen when
/// writing (`drawPointsFunc`).
pub type DrawPointsFunc<'a> = &'a mut dyn FnMut(&mut GlifPointPen) -> Result<()>;

/// The function that makes a file name for a glyph name, given the
/// lowercase file names already used (`glyphNameToFileNameFunc`).
pub type GlyphNameToFileNameFunc =
	Box<dyn Fn(&str, &HashSet<String>) -> Result<String> + Send + Sync>;

/// The options of [`GlyphSet::new`].
pub struct GlyphSetOptions {
	/// `glyphNameToFileNameFunc`; [`glyph_name_to_file_name`] by default.
	pub glyph_name_to_file_name: Option<GlyphNameToFileNameFunc>,
	/// `ufoFormatVersion`; the latest by default.
	pub ufo_format_version: Option<FormatVersionInput>,
	/// `validateRead`.
	pub validate_read: bool,
	/// `validateWrite`.
	pub validate_write: bool,
	/// `expectContentsFile`: whether `contents.plist` must exist.
	pub expect_contents_file: bool,
}

impl Default for GlyphSetOptions {
	fn default() -> Self {
		GlyphSetOptions {
			glyph_name_to_file_name: None,
			ufo_format_version: None,
			validate_read: true,
			validate_write: true,
			expect_contents_file: false,
		}
	}
}

impl std::fmt::Debug for GlyphSetOptions {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("GlyphSetOptions")
			.field("ufo_format_version", &self.ufo_format_version)
			.field("validate_read", &self.validate_read)
			.field("validate_write", &self.validate_write)
			.field("expect_contents_file", &self.expect_contents_file)
			.finish_non_exhaustive()
	}
}

/// `GlyphSet`: a directory of `.glif` files, with a `contents.plist` mapping
/// glyph names to file names.
pub struct GlyphSet {
	fs: Fs,
	/// `dirName` (deprecated in fontTools): the name of the directory.
	pub dir_name: String,
	have_previous_file: bool,
	ufo_format_version: UfoFormatVersion,
	glyph_name_to_file_name: GlyphNameToFileNameFunc,
	validate_read: bool,
	validate_write: bool,
	existing_file_names: Option<HashSet<String>>,
	reverse_contents: Option<IndexMap<String, String>>,
	/// `contents`: glyph names to file names.
	pub contents: IndexMap<String, String>,
}

impl std::fmt::Debug for GlyphSet {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("GlyphSet")
			.field("fs", &self.fs.to_string())
			.field("ufo_format_version", &self.ufo_format_version)
			.field("contents", &self.contents)
			.finish_non_exhaustive()
	}
}

impl GlyphSet {
	/// `GlyphSet(path, ...)`: the glyph set in an existing directory.
	pub fn new(path: impl AsRef<Path>, options: GlyphSetOptions) -> Result<GlyphSet> {
		let ufo_format_version = Self::normalize_ufo_format_version(options.ufo_format_version)?;
		let path = path.as_ref();
		let fs = Fs::os(path, false).map_err(|_| {
			UfoLibError::GlifLib(format!("No glyphs directory '{}'", path.display()))
		})?;
		Self::with_fs(fs, ufo_format_version, options)
	}

	fn normalize_ufo_format_version(v: Option<FormatVersionInput>) -> Result<UfoFormatVersion> {
		UfoFormatVersion::normalize(v).ok_or_else(|| {
			UfoLibError::UnsupportedUfoFormat(format!(
				"Unsupported UFO format: {}",
				v.map_or("None".to_owned(), |v| v.to_string())
			))
		})
	}

	/// `GlyphSet(fs, ...)`.
	pub(crate) fn from_fs(fs: Fs, options: GlyphSetOptions) -> Result<GlyphSet> {
		let ufo_format_version = Self::normalize_ufo_format_version(options.ufo_format_version)?;
		Self::with_fs(fs, ufo_format_version, options)
	}

	fn with_fs(
		fs: Fs,
		ufo_format_version: UfoFormatVersion,
		options: GlyphSetOptions,
	) -> Result<GlyphSet> {
		let dir_name = match fs.getsyspath("/")? {
			Some(p) => p
				.file_name()
				.map(|n| n.to_string_lossy().into_owned())
				.unwrap_or_default(),
			None => basename(&fs.to_string()),
		};
		let have_previous_file = fs.exists(CONTENTS_FILENAME)?;
		if options.expect_contents_file && !have_previous_file {
			return glif_err(format!("{CONTENTS_FILENAME} is missing."));
		}
		let mut gs = GlyphSet {
			fs,
			dir_name,
			have_previous_file,
			ufo_format_version,
			glyph_name_to_file_name: options.glyph_name_to_file_name.unwrap_or_else(|| {
				Box::new(|name, existing| glyph_name_to_file_name(name, Some(existing)))
			}),
			validate_read: options.validate_read,
			validate_write: options.validate_write,
			existing_file_names: None,
			reverse_contents: None,
			contents: IndexMap::new(),
		};
		// UPSTREAM-BUG(B38): `rebuildContents()` is called without arguments, and its
		// `validateRead` defaults to False rather than None, so contents.plist is
		// never validated when a glyph set is opened.
		gs.rebuild_contents(Some(false))?;
		Ok(gs)
	}

	/// The file system of the glyph set (`fs`).
	pub fn fs(&self) -> &Fs {
		&self.fs
	}

	/// `ufoFormatVersionTuple`.
	pub fn ufo_format_version(&self) -> UfoFormatVersion {
		self.ufo_format_version
	}

	/// `rebuildContents(validateRead)`: reloads `contents.plist`; `None`
	/// validates if the glyph set validates reads.
	///
	/// LENIENT(L35): without validation, fontTools keeps whatever contents.plist
	/// holds; here contents that are not a dictionary of strings are an
	/// error.
	pub fn rebuild_contents(&mut self, validate_read: Option<bool>) -> Result<()> {
		let validate_read = validate_read.unwrap_or(self.validate_read);
		let contents = get_plist(&self.fs, CONTENTS_FILENAME, Some(Value::Dict(Dict::new())))?;
		let bad = || UfoLibError::GlifLib(format!("{CONTENTS_FILENAME} is not properly formatted"));
		let Value::Dict(contents) = contents else {
			return Err(bad());
		};
		let mut out = IndexMap::new();
		let mut invalid = false;
		for (name, file_name) in contents {
			match file_name {
				Value::String(file_name) => {
					if validate_read && !self.fs.exists(&file_name)? {
						return glif_err(format!(
							"{CONTENTS_FILENAME} references a file that does not exist: {file_name}"
						));
					}
					out.insert(name, file_name);
				}
				_ => invalid = true,
			}
		}
		if invalid {
			return Err(bad());
		}
		self.contents = out;
		self.existing_file_names = None;
		self.reverse_contents = None;
		Ok(())
	}

	/// `getReverseContents`: lowercase file names to glyph names.
	pub fn get_reverse_contents(&mut self) -> &IndexMap<String, String> {
		let contents = &self.contents;
		self.reverse_contents.get_or_insert_with(|| {
			contents
				.iter()
				.map(|(k, v)| (crate::filenames::lower(v), k.clone()))
				.collect()
		})
	}

	/// `writeContents`: writes `contents.plist`.
	pub fn write_contents(&self) -> Result<()> {
		let contents: Dict = self
			.contents
			.iter()
			.map(|(k, v)| (k.clone(), Value::String(v.clone())))
			.collect();
		write_plist(
			&self.fs,
			self.have_previous_file,
			CONTENTS_FILENAME,
			&Value::Dict(contents),
		)
	}

	/// `readLayerInfo`: the attributes of `layerinfo.plist`.
	///
	/// LENIENT(L35): without validation, a layerinfo.plist that is not a
	/// dictionary is an error (an `AttributeError` in fontTools).
	pub fn read_layer_info(&self, validate_read: Option<bool>) -> Result<Dict> {
		let validate_read = validate_read.unwrap_or(self.validate_read);
		let info = get_plist(&self.fs, LAYERINFO_FILENAME, Some(Value::Dict(Dict::new())))?;
		let Value::Dict(info) = info else {
			return glif_err("layerinfo.plist is not properly formatted.");
		};
		if validate_read {
			validate_layer_info_version3_data(&info)?;
		}
		Ok(info)
	}

	/// `writeLayerInfo`: writes the `color` and `lib` attributes of `info` to
	/// `layerinfo.plist`, or removes the file if there are none.
	pub fn write_layer_info(&self, info: &Dict, validate_write: Option<bool>) -> Result<()> {
		let validate_write = validate_write.unwrap_or(self.validate_write);
		if self.ufo_format_version.major() < 3 {
			return glif_err(format!(
				"layerinfo.plist is not allowed in UFO {}.",
				self.ufo_format_version.major()
			));
		}
		let mut info_data = Dict::new();
		for attr in ["color", "lib"] {
			if let Some(value) = info.get(attr) {
				if attr == "lib" && !truthy(value) {
					continue;
				}
				info_data.insert(attr.to_owned(), value.clone());
			}
		}
		if !info_data.is_empty() {
			if validate_write {
				validate_layer_info_version3_data(&info_data)?;
			}
			write_plist(
				&self.fs,
				self.have_previous_file,
				LAYERINFO_FILENAME,
				&Value::Dict(info_data),
			)
		} else if self.have_previous_file && self.fs.exists(LAYERINFO_FILENAME)? {
			Ok(self.fs.remove(LAYERINFO_FILENAME)?)
		} else {
			Ok(())
		}
	}

	fn file_name(&self, glyph_name: &str) -> Result<&String> {
		self.contents
			.get(glyph_name)
			.ok_or_else(|| UfoLibError::Key(py::str_repr(glyph_name)))
	}

	/// `getGLIF`: the raw data of a glyph's `.glif` file.
	pub fn get_glif(&self, glyph_name: &str) -> Result<Vec<u8>> {
		let file_name = self.file_name(glyph_name)?;
		match self.fs.readbytes(file_name) {
			Ok(data) => Ok(data),
			Err(FsError::NotFound(_)) => glif_err(format!(
				"The file '{file_name}' associated with glyph '{glyph_name}' in contents.plist does not exist on {}",
				self.fs
			)),
			Err(e) => Err(e.into()),
		}
	}

	/// `getGLIFModificationTime`.
	pub fn get_glif_modification_time(&self, glyph_name: &str) -> Result<Option<f64>> {
		let file_name = self.file_name(glyph_name)?;
		file_modification_time(&self.fs, file_name)
	}

	/// `readGlyph`: reads a glyph's attributes into `glyph_object` and its
	/// outline into `point_pen`. `validate` defaults to the glyph set's
	/// `validateRead`.
	pub fn read_glyph(
		&self,
		glyph_name: &str,
		glyph_object: Option<&mut (dyn GlyphObject + '_)>,
		point_pen: Option<&mut (dyn NumberPointPen + '_)>,
		validate: Option<bool>,
	) -> Result<()> {
		let validate = validate.unwrap_or(self.validate_read);
		let text = self.get_glif(glyph_name)?;
		let result = glif_tree_from_bytes(&text).and_then(|tree| {
			let format_versions =
				GlifFormatVersion::supported_versions(Some(self.ufo_format_version));
			read_glyph_from_tree(&tree, glyph_object, point_pen, &format_versions, validate)
		});
		result.map_err(|e| {
			if !e.is_glif_lib_error() {
				return e;
			}
			let file_name = self.contents.get(glyph_name).cloned().unwrap_or_default();
			let location = match self.fs.getsyspath(&file_name) {
				Ok(Some(p)) => format!("'{}'", p.display()),
				_ => format!("'{file_name}' from '{}'", self.fs),
			};
			e.add_note(&format!(
				"The issue is in glyph '{glyph_name}', located in {location}."
			))
		})
	}

	/// The attributes of a glyph (`glyphSet[glyphName]` after
	/// `drawPoints(None)`).
	pub fn glyph(&self, glyph_name: &str) -> Result<Glyph> {
		let mut glyph = Glyph::default();
		self.read_glyph(glyph_name, Some(&mut glyph), None, None)?;
		Ok(glyph)
	}

	/// `glyphSet[glyphName].drawPoints(pointPen)`.
	pub fn draw_points(&self, glyph_name: &str, point_pen: &mut dyn PointPen) -> Result<()> {
		let mut adapter = PointPenAdapter(point_pen);
		self.read_glyph(glyph_name, None, Some(&mut adapter), None)
	}

	/// `glyphSet[glyphName].draw(pen, outputImpliedClosingLine)`.
	pub fn draw(
		&self,
		glyph_name: &str,
		pen: &mut dyn Pen,
		output_implied_closing_line: bool,
	) -> Result<()> {
		let mut point_pen = PointToSegmentPen::new(pen, output_implied_closing_line);
		self.draw_points(glyph_name, &mut point_pen)
	}

	/// `writeGlyph`: writes a glyph's `.glif` file, choosing a file name for
	/// a new glyph. `draw_points` draws the outline into the pen it is given.
	pub fn write_glyph(
		&mut self,
		glyph_name: &str,
		glyph_object: Option<&dyn GlyphObject>,
		draw_points: Option<DrawPointsFunc<'_>>,
		format_version: Option<FormatVersionInput>,
		validate: Option<bool>,
	) -> Result<()> {
		let format_version = match format_version {
			None => GlifFormatVersion::default_for(Some(self.ufo_format_version)),
			Some(v) => GlifFormatVersion::new(v).ok_or_else(|| {
				UfoLibError::UnsupportedGlifFormat(format!("Unsupported GLIF format version: {v}"))
			})?,
		};
		if !GlifFormatVersion::supported_versions(Some(self.ufo_format_version))
			.contains(&format_version)
		{
			return Err(UfoLibError::UnsupportedGlifFormat(format!(
				"Unsupported GLIF format version ({format_version}) for UFO format version {}.",
				self.ufo_format_version
			)));
		}
		let validate = validate.unwrap_or(self.validate_write);
		let file_name = match self.contents.get(glyph_name) {
			Some(f) => f.clone(),
			None => {
				let contents = &self.contents;
				let existing = self.existing_file_names.get_or_insert_with(|| {
					contents
						.values()
						.map(|f| crate::filenames::lower(f))
						.collect()
				});
				let file_name = (self.glyph_name_to_file_name)(glyph_name, existing)?;
				existing.insert(crate::filenames::lower(&file_name));
				self.contents
					.insert(glyph_name.to_owned(), file_name.clone());
				if let Some(rc) = &mut self.reverse_contents {
					rc.insert(crate::filenames::lower(&file_name), glyph_name.to_owned());
				}
				file_name
			}
		};
		let data = write_glyph_to_bytes(
			glyph_name,
			glyph_object,
			draw_points,
			Some(format_version.into()),
			validate,
		)?;
		if self.have_previous_file
			&& self.fs.exists(&file_name)?
			&& self.fs.readbytes(&file_name)? == data
		{
			return Ok(());
		}
		Ok(self.fs.writebytes(&file_name, &data)?)
	}

	/// `deleteGlyph`: removes a glyph's file.
	pub fn delete_glyph(&mut self, glyph_name: &str) -> Result<()> {
		let file_name = self.file_name(glyph_name)?.clone();
		self.fs.remove(&file_name)?;
		let lower = crate::filenames::lower(&file_name);
		if let Some(e) = &mut self.existing_file_names {
			e.remove(&lower);
		}
		if let Some(rc) = &mut self.reverse_contents {
			rc.shift_remove(&lower);
		}
		self.contents.shift_remove(glyph_name);
		Ok(())
	}

	/// `keys()`: the glyph names.
	pub fn keys(&self) -> Vec<String> {
		self.contents.keys().cloned().collect()
	}

	/// `glyphName in glyphSet`.
	pub fn contains(&self, glyph_name: &str) -> bool {
		self.contents.contains_key(glyph_name)
	}

	/// `len(glyphSet)`.
	pub fn len(&self) -> usize {
		self.contents.len()
	}

	/// Whether there are no glyphs.
	pub fn is_empty(&self) -> bool {
		self.contents.is_empty()
	}

	fn names_or_all(&self, glyph_names: Option<&[&str]>) -> Vec<String> {
		match glyph_names {
			Some(names) => names.iter().map(|s| (*s).to_owned()).collect(),
			None => self.keys(),
		}
	}

	/// `getUnicodes`: the Unicode values of glyphs, from a quick scan of
	/// their `.glif` files.
	pub fn get_unicodes(&self, glyph_names: Option<&[&str]>) -> Result<IndexMap<String, Vec<i64>>> {
		let mut out = IndexMap::new();
		for name in self.names_or_all(glyph_names) {
			let text = self.get_glif(&name)?;
			out.insert(name, fetch_unicodes(&text)?);
		}
		Ok(out)
	}

	/// `getComponentReferences`: the base glyphs of the components of glyphs.
	pub fn get_component_references(
		&self,
		glyph_names: Option<&[&str]>,
	) -> Result<IndexMap<String, Vec<String>>> {
		let mut out = IndexMap::new();
		for name in self.names_or_all(glyph_names) {
			let text = self.get_glif(&name)?;
			out.insert(name, fetch_component_bases(&text)?);
		}
		Ok(out)
	}

	/// `getImageReferences`: the image file names of glyphs.
	pub fn get_image_references(
		&self,
		glyph_names: Option<&[&str]>,
	) -> Result<IndexMap<String, Option<String>>> {
		let mut out = IndexMap::new();
		for name in self.names_or_all(glyph_names) {
			let text = self.get_glif(&name)?;
			out.insert(name, fetch_image_file_name(&text)?);
		}
		Ok(out)
	}
}

/// Glyph sets draw their glyphs, so that pens can decompose components.
impl fontoxide_pens::GlyphSet for GlyphSet {
	fn draw_glyph(&self, glyph_name: &str, pen: &mut dyn Pen) -> fontoxide_pens::Result<bool> {
		if !self.contains(glyph_name) {
			return Ok(false);
		}
		self.draw(glyph_name, pen, false)?;
		Ok(true)
	}

	fn contains(&self, glyph_name: &str) -> bool {
		GlyphSet::contains(self, glyph_name)
	}

	fn draw_glyph_points(
		&self,
		glyph_name: &str,
		pen: &mut dyn PointPen,
	) -> fontoxide_pens::Result<bool> {
		if !self.contains(glyph_name) {
			return Ok(false);
		}
		self.draw_points(glyph_name, pen)?;
		Ok(true)
	}
}

// -----------------------
// Glyph Name to File Name
// -----------------------

/// `glyphNameToFileName`: the default file names of glyphs, made with
/// [`user_name_to_file_name`] and a `.glif` suffix.
pub fn glyph_name_to_file_name(
	glyph_name: &str,
	existing: Option<&HashSet<String>>,
) -> Result<String> {
	let empty = HashSet::new();
	user_name_to_file_name(glyph_name, existing.unwrap_or(&empty), "", ".glif")
}

// -----------------------
// layerinfo.plist Support
// -----------------------

/// `validateLayerInfoVersion3ValueForAttribute`.
pub fn validate_layer_info_version3_value_for_attribute(attr: &str, value: &Value) -> bool {
	match attr {
		"color" => color_validator(value),
		"lib" => generic_type_validator(value, TypeSpec::Dict),
		_ => false,
	}
}

/// `validateLayerInfoVersion3Data`.
pub fn validate_layer_info_version3_data(info_data: &Dict) -> Result<()> {
	for (attr, value) in info_data {
		if attr != "color" && attr != "lib" {
			return glif_err(format!("Unknown attribute {attr}."));
		}
		if !validate_layer_info_version3_value_for_attribute(attr, value) {
			return glif_err(format!(
				"Invalid value for attribute {attr} ({}).",
				value_repr(value)
			));
		}
	}
	Ok(())
}

// -----------------------
// GLIF To and From String
// -----------------------

/// `readGlyphFromString`: reads `.glif` data into `glyph_object` and
/// `point_pen`. `format_versions` are the GLIF versions allowed (all by
/// default); versions that do not exist are ignored, but at least one must.
pub fn read_glyph_from_string(
	data: impl AsRef<[u8]>,
	glyph_object: Option<&mut (dyn GlyphObject + '_)>,
	point_pen: Option<&mut (dyn NumberPointPen + '_)>,
	format_versions: Option<&[FormatVersionInput]>,
	validate: bool,
) -> Result<()> {
	let tree = glif_tree_from_bytes(data.as_ref())?;
	let valid: Vec<GlifFormatVersion> = match format_versions {
		None => GlifFormatVersion::supported_versions(None),
		Some(versions) => {
			let valid: Vec<GlifFormatVersion> = versions
				.iter()
				.filter_map(|v| GlifFormatVersion::new(*v))
				.collect();
			if valid.is_empty() {
				return Err(UfoLibError::Value(format!(
					"None of the requested GLIF formatVersions are supported: [{}]",
					versions
						.iter()
						.map(|v| v.to_string())
						.collect::<Vec<_>>()
						.join(", ")
				)));
			}
			valid
		}
	};
	read_glyph_from_tree(&tree, glyph_object, point_pen, &valid, validate)
}

/// `_writeGlyphToBytes`: the `.glif` data of a glyph, as UTF-8.
pub fn write_glyph_to_bytes(
	glyph_name: &str,
	glyph_object: Option<&dyn GlyphObject>,
	draw_points: Option<DrawPointsFunc<'_>>,
	format_version: Option<FormatVersionInput>,
	validate: bool,
) -> Result<Vec<u8>> {
	Ok(write_glyph_to_string(
		glyph_name,
		glyph_object,
		draw_points,
		format_version,
		validate,
	)?
	.into_bytes())
}

/// `writeGlyphToString`: the `.glif` data of a glyph. The GLIF version is the
/// latest by default.
pub fn write_glyph_to_string(
	glyph_name: &str,
	glyph_object: Option<&dyn GlyphObject>,
	draw_points: Option<DrawPointsFunc<'_>>,
	format_version: Option<FormatVersionInput>,
	validate: bool,
) -> Result<String> {
	let format_version = GlifFormatVersion::normalize(format_version).ok_or_else(|| {
		// UPSTREAM-BUG(B40): the message lacks the f-string prefix, so it reads
		// "Unsupported GLIF format version: {formatVersion!r}" literally.
		UfoLibError::UnsupportedGlifFormat(
			"Unsupported GLIF format version: {formatVersion!r}".into(),
		)
	})?;
	if validate && glyph_name.is_empty() {
		return glif_err("The glyph name is empty.");
	}
	let mut root = Element::new("glyph");
	root.set("name", glyph_name);
	root.set("format", format_version.major().to_string());
	if format_version.minor() != 0 {
		root.set("formatMinor", format_version.minor().to_string());
	}
	let mut identifiers: HashSet<String> = HashSet::new();
	// advance
	write_advance(glyph_object, &mut root, validate)?;
	// unicodes
	if let Some(unicodes) = getattr(glyph_object, "unicodes").filter(truthy) {
		write_unicodes(&unicodes, &mut root, validate)?;
	}
	// note
	if let Some(note) = getattr(glyph_object, "note").filter(truthy) {
		write_note(&note, &mut root, validate)?;
	}
	// image
	if format_version.major() >= 2
		&& let Some(image) = getattr(glyph_object, "image").filter(truthy)
	{
		write_image(&image, &mut root, validate)?;
	}
	// guidelines
	if format_version.major() >= 2
		&& let Some(guidelines) = getattr(glyph_object, "guidelines").filter(truthy)
	{
		write_guidelines(&guidelines, &mut root, &mut identifiers, validate)?;
	}
	// anchors
	let anchors = getattr(glyph_object, "anchors");
	if format_version.major() >= 2
		&& let Some(anchors) = anchors.as_ref().filter(|a| truthy(a))
	{
		write_anchors(anchors, &mut root, &mut identifiers, validate)?;
	}
	// outline
	if let Some(draw_points) = draw_points {
		// UPSTREAM-BUG(B39): the pen is made without the format version, so it writes
		// identifiers (a GLIF 2 feature) into GLIF 1 outlines too.
		let mut pen = GlifPointPen::new(Element::new("outline"), None, identifiers, validate)?;
		draw_points(&mut pen)?;
		if format_version.major() == 1
			&& let Some(anchors) = anchors.as_ref().filter(|a| truthy(a))
		{
			write_anchors_format1(&mut pen, anchors, validate)?;
		}
		let (mut outline, _) = pen.into_parts();
		// prevent lxml from writing self-closing tags
		if outline.is_empty() {
			outline.text = Some("\n  ".into());
		}
		root.append(outline);
	}
	// lib
	if let Some(lib) = getattr(glyph_object, "lib").filter(truthy) {
		write_lib(&lib, &mut root, validate)?;
	}
	etree::tostring(&root, true, true).map_err(|e| UfoLibError::Xml(e.0))
}

fn write_advance(
	glyph: Option<&dyn GlyphObject>,
	element: &mut Element,
	validate: bool,
) -> Result<()> {
	let check = |attr: &str| -> Result<Option<Value>> {
		let Some(value) = getattr(glyph, attr) else {
			return Ok(None);
		};
		if validate && !is_number(&value) {
			return glif_err(format!("{attr} attribute must be int or float"));
		}
		if Number::from_value(&value) == Some(Number::Int(0)) {
			return Ok(None);
		}
		Ok(Some(value))
	};
	let width = check("width")?;
	let height = check("height")?;
	let mut advance = Element::new("advance");
	match (&width, &height) {
		(None, None) => return Ok(()),
		_ => {
			if let Some(h) = &height {
				advance.set("height", value_repr(h));
			}
			if let Some(w) = &width {
				advance.set("width", value_repr(w));
			}
		}
	}
	element.append(advance);
	Ok(())
}

/// `"%04X" % code`.
fn hex4(code: i128) -> String {
	if code < 0 {
		format!("-{:03X}", -code)
	} else {
		format!("{code:04X}")
	}
}

fn write_unicodes(unicodes: &Value, element: &mut Element, validate: bool) -> Result<()> {
	let list: Vec<Value> = match unicodes {
		Value::Array(a) => a.clone(),
		v if validate && is_int(v) => vec![v.clone()],
		Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
		// LENIENT(L35): iterating a number raises a TypeError in fontTools.
		_ => return glif_err("unicode values must be int"),
	};
	let mut seen: Vec<i128> = Vec::new();
	for code in &list {
		if validate && !is_int(code) {
			return glif_err("unicode values must be int");
		}
		let code = match code {
			Value::Bool(b) => i128::from(*b),
			Value::Integer(i) => *i,
			// LENIENT(L35): `"%04X"` of a non-integer raises a TypeError in fontTools.
			_ => return glif_err("unicode values must be int"),
		};
		if seen.contains(&code) {
			continue;
		}
		seen.push(code);
		let mut u = Element::new("unicode");
		u.set("hex", hex4(code));
		element.append(u);
	}
	Ok(())
}

fn write_note(note: &Value, element: &mut Element, validate: bool) -> Result<()> {
	match note {
		Value::String(s) => {
			let mut n = Element::new("note");
			n.text = Some(format!("\n{}\n", py::strip(s)));
			element.append(n);
			Ok(())
		}
		_ if validate => glif_err("note attribute must be str"),
		_ => Ok(()),
	}
}

/// `_transformationInfo`: the transformation attributes and their defaults.
const TRANSFORMATION_INFO: [(&str, i64); 6] = [
	("xScale", 1),
	("xyScale", 0),
	("yxScale", 0),
	("yScale", 1),
	("xOffset", 0),
	("yOffset", 0),
];

/// `value != default` for a number default.
fn differs(value: &Value, default: i64) -> bool {
	Number::from_value(value) != Some(Number::Int(default))
}

/// An attribute value that lxml accepts: a string.
fn attr_string(value: &Value) -> Result<String> {
	match value {
		Value::String(s) => Ok(s.clone()),
		// LENIENT(L35): lxml raises a TypeError for non-string attribute values.
		other => Err(UfoLibError::Value(format!(
			"Argument must be bytes or unicode, got '{}'",
			crate::utils::value_repr(other)
		))),
	}
}

fn dict_of(value: &Value, what: &str) -> Result<Dict> {
	match value {
		Value::Dict(d) => Ok(d.clone()),
		// LENIENT(L35): fontTools fails with a TypeError or AttributeError.
		_ => glif_err(format!(
			"{what} attribute does not have the proper structure."
		)),
	}
}

fn write_image(image: &Value, element: &mut Element, validate: bool) -> Result<()> {
	if validate && !image_validator(image) {
		return glif_err(
			"image attribute must be a dict or dict-like object with the proper structure.",
		);
	}
	let image = dict_of(image, "image")?;
	let mut e = Element::new("image");
	// LENIENT(L35): a missing fileName is a KeyError in fontTools.
	let file_name = image
		.get("fileName")
		.ok_or_else(|| UfoLibError::Key("'fileName'".into()))?;
	e.set("fileName", attr_string(file_name)?);
	for (attr, default) in TRANSFORMATION_INFO {
		if let Some(value) = image.get(attr)
			&& differs(value, default)
		{
			e.set(attr, value_repr(value));
		}
	}
	if let Some(color) = image.get("color") {
		e.set("color", attr_string(color)?);
	}
	element.append(e);
	Ok(())
}

fn items_of(value: &Value) -> Vec<Value> {
	match value {
		Value::Array(a) => a.clone(),
		_ => Vec::new(),
	}
}

fn write_guidelines(
	guidelines: &Value,
	element: &mut Element,
	identifiers: &mut HashSet<String>,
	validate: bool,
) -> Result<()> {
	if validate && !guidelines_validator(guidelines, None) {
		return glif_err("guidelines attribute does not have the proper structure.");
	}
	for guideline in items_of(guidelines) {
		let guideline = dict_of(&guideline, "guidelines")?;
		let mut e = Element::new("guideline");
		for attr in ["x", "y", "angle"] {
			if let Some(v) = guideline.get(attr) {
				e.set(attr, value_repr(v));
			}
		}
		for attr in ["name", "color"] {
			if let Some(v) = guideline.get(attr) {
				e.set(attr, attr_string(v)?);
			}
		}
		if let Some(identifier) = guideline.get("identifier") {
			let identifier = attr_string(identifier)?;
			if validate && identifiers.contains(&identifier) {
				return glif_err(format!("identifier used more than once: {identifier}"));
			}
			e.set("identifier", identifier.clone());
			identifiers.insert(identifier);
		}
		element.append(e);
	}
	Ok(())
}

/// `anchor["x"]`, a `KeyError` if missing.
fn required<'a>(d: &'a Dict, key: &str) -> Result<&'a Value> {
	d.get(key)
		.ok_or_else(|| UfoLibError::Key(py::str_repr(key)))
}

fn write_anchors_format1(pen: &mut GlifPointPen, anchors: &Value, validate: bool) -> Result<()> {
	if validate && !anchors_validator(anchors, None) {
		return glif_err("anchors attribute does not have the proper structure.");
	}
	for anchor in items_of(anchors) {
		let anchor = dict_of(&anchor, "anchors")?;
		let x = required(&anchor, "x")?;
		let y = required(&anchor, "y")?;
		let name = match anchor.get("name") {
			Some(n) => Some(attr_string(n)?),
			None => None,
		};
		pen.begin_path_impl(None)?;
		pen.add_point_values((x, y), Some("move"), false, name.as_deref(), None)?;
		pen.end_path_impl()?;
	}
	Ok(())
}

fn write_anchors(
	anchors: &Value,
	element: &mut Element,
	identifiers: &mut HashSet<String>,
	validate: bool,
) -> Result<()> {
	if validate && !anchors_validator(anchors, None) {
		return glif_err("anchors attribute does not have the proper structure.");
	}
	for anchor in items_of(anchors) {
		let anchor = dict_of(&anchor, "anchors")?;
		let mut e = Element::new("anchor");
		e.set("x", value_repr(required(&anchor, "x")?));
		e.set("y", value_repr(required(&anchor, "y")?));
		for attr in ["name", "color"] {
			if let Some(v) = anchor.get(attr) {
				e.set(attr, attr_string(v)?);
			}
		}
		if let Some(identifier) = anchor.get("identifier") {
			let identifier = attr_string(identifier)?;
			if validate && identifiers.contains(&identifier) {
				return glif_err(format!("identifier used more than once: {identifier}"));
			}
			e.set("identifier", identifier.clone());
			identifiers.insert(identifier);
		}
		element.append(e);
	}
	Ok(())
}

fn write_lib(lib: &Value, element: &mut Element, validate: bool) -> Result<()> {
	if validate && let Err(message) = glyph_lib_validator(lib) {
		return glif_err(message);
	}
	// plist inside GLIF begins with 2 levels of indentation
	let tree = plistlib::totree(
		lib,
		plistlib::TreeOptions {
			sort_keys: true,
			pretty_print: true,
			indent_level: 2,
		},
	)?;
	let mut e = Element::new("lib");
	e.append(tree);
	element.append(e);
	Ok(())
}

// -----------------
// GLIF Tree Support
// -----------------

/// `_glifTreeFromString`: parses `.glif` data, without comments.
fn glif_tree_from_bytes(data: &[u8]) -> Result<xml_reader::Element> {
	let root = plistlib::decode_xml(data)
		.ok()
		.and_then(|text| xml_reader::parse(text.strip_prefix('\u{feff}').unwrap_or(&text)).ok())
		.ok_or_else(|| UfoLibError::GlifLib("GLIF contains invalid XML.".into()))?;
	if root.name != "glyph" {
		return glif_err("The GLIF is not properly formatted.");
	}
	if text_is_not_blank(&root) {
		return glif_err("Invalid GLIF structure.");
	}
	Ok(root)
}

/// `element.text and element.text.strip() != ""`.
fn text_is_not_blank(element: &xml_reader::Element) -> bool {
	plistlib::element_text(element).is_some_and(|t| !py::strip(&t).is_empty())
}

fn child_count(element: &xml_reader::Element) -> usize {
	element.elements().count()
}

/// `str(element)` of lxml, without the address.
fn element_repr(element: &xml_reader::Element) -> String {
	format!("<Element {}>", element.name)
}

fn read_glyph_from_tree(
	tree: &xml_reader::Element,
	glyph: Option<&mut (dyn GlyphObject + '_)>,
	pen: Option<&mut (dyn NumberPointPen + '_)>,
	format_versions: &[GlifFormatVersion],
	validate: bool,
) -> Result<()> {
	// check the format version
	let major = match tree.attr("format") {
		Some(m) => m.to_owned(),
		None => {
			if validate {
				return glif_err("Unspecified format version in GLIF.");
			}
			"0".to_owned()
		}
	};
	let minor = tree.attr("formatMinor").unwrap_or("0").to_owned();
	let version = match (parse_int(&major), parse_int(&minor)) {
		(Some(a), Some(b)) => i64::try_from(a)
			.ok()
			.zip(i64::try_from(b).ok())
			.and_then(|(a, b)| GlifFormatVersion::new((a, b))),
		_ => None,
	};
	let format_version = match version {
		Some(v) => v,
		None => {
			let msg = format!("Unsupported GLIF format: {major}.{minor}");
			if validate {
				return Err(UfoLibError::UnsupportedGlifFormat(msg));
			}
			let v = GlifFormatVersion::default_for(None);
			log::warn!(
				"{msg}. Assuming the latest supported version ({v}). Some data may be skipped or parsed incorrectly."
			);
			v
		}
	};
	if validate && !format_versions.contains(&format_version) {
		return glif_err(format!("Forbidden GLIF format version: {format_version}"));
	}
	match format_version {
		GlifFormatVersion::Format1_0 => read_glyph_from_tree_format1(tree, glyph, pen, validate),
		GlifFormatVersion::Format2_0 => read_glyph_from_tree_format2(tree, glyph, pen, validate),
	}
}

fn unicode_value(element: &xml_reader::Element) -> Result<i128> {
	let Some(v) = element.attr("hex") else {
		return glif_err("A unicode element is missing its required hex attribute.");
	};
	parse_hex(v).ok_or_else(|| {
		UfoLibError::GlifLib("Illegal value for hex attribute of unicode element.".into())
	})
}

fn unicodes_value(unicodes: &[i128]) -> Value {
	Value::Array(unicodes.iter().map(|&u| Value::Integer(u)).collect())
}

fn check_outline_element(element: &xml_reader::Element, seen: bool, validate: bool) -> Result<()> {
	if validate {
		if seen {
			return glif_err("The outline element occurs more than once.");
		}
		if !element.attrs.is_empty() {
			return glif_err("The outline element contains unknown attributes.");
		}
		if text_is_not_blank(element) {
			return glif_err("Invalid outline structure.");
		}
	}
	Ok(())
}

fn read_glyph_from_tree_format1(
	tree: &xml_reader::Element,
	mut glyph: Option<&mut (dyn GlyphObject + '_)>,
	mut pen: Option<&mut (dyn NumberPointPen + '_)>,
	validate: bool,
) -> Result<()> {
	read_name(glyph.as_deref_mut(), tree, validate)?;
	let mut unicodes: Vec<i128> = Vec::new();
	let (mut seen_advance, mut seen_outline, mut seen_lib, mut seen_note) =
		(false, false, false, false);
	for element in tree.elements() {
		match element.name.as_str() {
			"outline" => {
				check_outline_element(element, seen_outline, validate)?;
				seen_outline = true;
				build_outline_format1(glyph.as_deref_mut(), pen.as_deref_mut(), element, validate)?;
			}
			_ if glyph.is_none() => continue,
			"advance" => {
				if validate && seen_advance {
					return glif_err("The advance element occurs more than once.");
				}
				seen_advance = true;
				read_advance(glyph.as_deref_mut(), element)?;
			}
			"unicode" => {
				let v = unicode_value(element)?;
				if !unicodes.contains(&v) {
					unicodes.push(v);
				}
			}
			"note" => {
				if validate && seen_note {
					return glif_err("The note element occurs more than once.");
				}
				seen_note = true;
				read_note(glyph.as_deref_mut(), element);
			}
			"lib" => {
				if validate && seen_lib {
					return glif_err("The lib element occurs more than once.");
				}
				seen_lib = true;
				read_lib(glyph.as_deref_mut(), element, validate)?;
			}
			_ => {
				return glif_err(format!(
					"Unknown element in GLIF: {}",
					element_repr(element)
				));
			}
		}
	}
	if !unicodes.is_empty() {
		relaxed_setattr(glyph, "unicodes", unicodes_value(&unicodes));
	}
	Ok(())
}

fn read_glyph_from_tree_format2(
	tree: &xml_reader::Element,
	mut glyph: Option<&mut (dyn GlyphObject + '_)>,
	mut pen: Option<&mut (dyn NumberPointPen + '_)>,
	validate: bool,
) -> Result<()> {
	read_name(glyph.as_deref_mut(), tree, validate)?;
	let mut unicodes: Vec<i128> = Vec::new();
	let mut guidelines: Vec<Value> = Vec::new();
	let mut anchors: Vec<Value> = Vec::new();
	let (mut seen_advance, mut seen_image, mut seen_outline, mut seen_lib, mut seen_note) =
		(false, false, false, false, false);
	let mut identifiers: HashSet<String> = HashSet::new();
	for element in tree.elements() {
		match element.name.as_str() {
			"outline" => {
				check_outline_element(element, seen_outline, validate)?;
				seen_outline = true;
				if let Some(pen) = pen.as_deref_mut() {
					build_outline_format2(pen, element, &mut identifiers, validate)?;
				}
			}
			_ if glyph.is_none() => continue,
			"advance" => {
				if validate && seen_advance {
					return glif_err("The advance element occurs more than once.");
				}
				seen_advance = true;
				read_advance(glyph.as_deref_mut(), element)?;
			}
			"unicode" => {
				let v = unicode_value(element)?;
				if !unicodes.contains(&v) {
					unicodes.push(v);
				}
			}
			"guideline" => {
				if validate && child_count(element) > 0 {
					return glif_err("Unknown children in guideline element.");
				}
				guidelines.push(Value::Dict(number_attrs(element, &["x", "y", "angle"])?));
			}
			"anchor" => {
				if validate && child_count(element) > 0 {
					return glif_err("Unknown children in anchor element.");
				}
				anchors.push(Value::Dict(number_attrs(element, &["x", "y"])?));
			}
			"image" => {
				if validate {
					if seen_image {
						return glif_err("The image element occurs more than once.");
					}
					if child_count(element) > 0 {
						return glif_err("Unknown children in image element.");
					}
				}
				seen_image = true;
				read_image(glyph.as_deref_mut(), element, validate)?;
			}
			"note" => {
				if validate && seen_note {
					return glif_err("The note element occurs more than once.");
				}
				seen_note = true;
				read_note(glyph.as_deref_mut(), element);
			}
			"lib" => {
				if validate && seen_lib {
					return glif_err("The lib element occurs more than once.");
				}
				seen_lib = true;
				read_lib(glyph.as_deref_mut(), element, validate)?;
			}
			_ => {
				return glif_err(format!(
					"Unknown element in GLIF: {}",
					element_repr(element)
				));
			}
		}
	}
	if !unicodes.is_empty() {
		relaxed_setattr(glyph.as_deref_mut(), "unicodes", unicodes_value(&unicodes));
	}
	if !guidelines.is_empty() {
		let guidelines = Value::Array(guidelines);
		if validate && !guidelines_validator(&guidelines, Some(&mut identifiers)) {
			return glif_err("The guidelines are improperly formatted.");
		}
		relaxed_setattr(glyph.as_deref_mut(), "guidelines", guidelines);
	}
	if !anchors.is_empty() {
		let anchors = Value::Array(anchors);
		if validate && !anchors_validator(&anchors, Some(&mut identifiers)) {
			return glif_err("The anchors are improperly formatted.");
		}
		relaxed_setattr(glyph, "anchors", anchors);
	}
	Ok(())
}

/// `dict(element.attrib)` with the given attributes converted with
/// `_number`.
fn number_attrs(element: &xml_reader::Element, numbers: &[&str]) -> Result<Dict> {
	let mut d = Dict::new();
	for (k, v) in &element.attrs {
		let value = if numbers.contains(&k.as_str()) {
			number(v)?.to_value()
		} else {
			Value::String(v.clone())
		};
		d.insert(k.clone(), value);
	}
	Ok(d)
}

fn read_name(
	glyph: Option<&mut (dyn GlyphObject + '_)>,
	root: &xml_reader::Element,
	validate: bool,
) -> Result<()> {
	let name = root.attr("name").unwrap_or_default();
	if validate && name.is_empty() {
		return glif_err("Empty glyph name in GLIF.");
	}
	if !name.is_empty() {
		relaxed_setattr(glyph, "name", Value::from(name));
	}
	Ok(())
}

fn read_advance(
	mut glyph: Option<&mut (dyn GlyphObject + '_)>,
	advance: &xml_reader::Element,
) -> Result<()> {
	let get = |attr: &str| -> Result<Number> {
		match advance.attr(attr) {
			Some(v) => number(v),
			None => Ok(Number::Int(0)),
		}
	};
	let width = get("width")?;
	relaxed_setattr(glyph.as_deref_mut(), "width", width.to_value());
	let height = get("height")?;
	relaxed_setattr(glyph, "height", height.to_value());
	Ok(())
}

fn read_note(glyph: Option<&mut (dyn GlyphObject + '_)>, note: &xml_reader::Element) {
	let Some(text) = plistlib::element_text(note) else {
		return;
	};
	let note = text
		.split('\n')
		.map(py::strip)
		.filter(|line| !line.is_empty())
		.collect::<Vec<_>>()
		.join("\n");
	relaxed_setattr(glyph, "note", Value::String(note));
}

fn read_lib(
	glyph: Option<&mut (dyn GlyphObject + '_)>,
	lib: &xml_reader::Element,
	validate: bool,
) -> Result<()> {
	let children: Vec<&xml_reader::Element> = lib.elements().collect();
	// LENIENT(L35): fontTools asserts that there is exactly one child.
	let [child] = children.as_slice() else {
		return glif_err("The lib element must contain exactly one element.");
	};
	let plist = plistlib::fromtree(child)?;
	if validate && let Err(message) = glyph_lib_validator(&plist) {
		return glif_err(message);
	}
	relaxed_setattr(glyph, "lib", plist);
	Ok(())
}

fn read_image(
	glyph: Option<&mut (dyn GlyphObject + '_)>,
	image: &xml_reader::Element,
	validate: bool,
) -> Result<()> {
	let mut data: Dict = image
		.attrs
		.iter()
		.map(|(k, v)| (k.clone(), Value::String(v.clone())))
		.collect();
	for (attr, default) in TRANSFORMATION_INFO {
		let value = match image.attr(attr) {
			Some(v) => number(v)?,
			None => Number::Int(default),
		};
		data.insert(attr.to_owned(), value.to_value());
	}
	let data = Value::Dict(data);
	if validate && !image_validator(&data) {
		return glif_err("The image element is not properly formatted.");
	}
	relaxed_setattr(glyph, "image", data);
	Ok(())
}

// ----------------
// GLIF to PointPen
// ----------------

const CONTOUR_ATTRIBUTES_FORMAT2: &[&str] = &["identifier"];
const COMPONENT_ATTRIBUTES_FORMAT1: &[&str] = &[
	"base", "xScale", "xyScale", "yxScale", "yScale", "xOffset", "yOffset",
];
const COMPONENT_ATTRIBUTES_FORMAT2: &[&str] = &[
	"base",
	"xScale",
	"xyScale",
	"yxScale",
	"yScale",
	"xOffset",
	"yOffset",
	"identifier",
];
const POINT_ATTRIBUTES_FORMAT1: &[&str] = &["x", "y", "type", "smooth", "name"];
const POINT_ATTRIBUTES_FORMAT2: &[&str] = &["x", "y", "type", "smooth", "name", "identifier"];
const POINT_TYPE_OPTIONS: &[&str] = &["move", "line", "offcurve", "curve", "qcurve"];

fn build_outline_format1(
	glyph: Option<&mut (dyn GlyphObject + '_)>,
	mut pen: Option<&mut (dyn NumberPointPen + '_)>,
	outline: &xml_reader::Element,
	validate: bool,
) -> Result<()> {
	let mut anchors: Vec<Value> = Vec::new();
	for element in outline.elements() {
		match element.name.as_str() {
			"contour" => {
				let children: Vec<&xml_reader::Element> = element.elements().collect();
				if let [point] = children.as_slice()
					&& point.name == "point"
					&& let Some(anchor) = build_anchor_format1(point, validate)?
				{
					anchors.push(Value::Dict(anchor));
					continue;
				}
				if let Some(pen) = pen.as_deref_mut() {
					build_outline_contour_format1(pen, element, validate)?;
				}
			}
			"component" => {
				if let Some(pen) = pen.as_deref_mut() {
					build_outline_component(pen, element, None, validate)?;
				}
			}
			_ => {
				return glif_err(format!(
					"Unknown element in outline element: {}",
					element_repr(element)
				));
			}
		}
	}
	if glyph.is_some() && !anchors.is_empty() {
		let anchors = Value::Array(anchors);
		if validate && !anchors_validator(&anchors, None) {
			return glif_err("GLIF 1 anchors are not properly formatted.");
		}
		relaxed_setattr(glyph, "anchors", anchors);
	}
	Ok(())
}

fn build_anchor_format1(point: &xml_reader::Element, _validate: bool) -> Result<Option<Dict>> {
	if point.attr("type") != Some("move") {
		return Ok(None);
	}
	let Some(name) = point.attr("name") else {
		return Ok(None);
	};
	// LENIENT(L35): without validation, fontTools fails an assertion on a missing
	// coordinate.
	let Some(x) = point.attr("x") else {
		return glif_err("Required x attribute is missing in point element.");
	};
	let Some(y) = point.attr("y") else {
		return glif_err("Required y attribute is missing in point element.");
	};
	let mut anchor = Dict::new();
	anchor.insert("x".into(), number(x)?.to_value());
	anchor.insert("y".into(), number(y)?.to_value());
	anchor.insert("name".into(), Value::from(name));
	Ok(Some(anchor))
}

fn build_outline_contour_format1(
	pen: &mut dyn NumberPointPen,
	contour: &xml_reader::Element,
	validate: bool,
) -> Result<()> {
	if validate && !contour.attrs.is_empty() {
		return glif_err("Unknown attributes in contour element.");
	}
	pen.begin_path(None)?;
	if child_count(contour) > 0 {
		let massaged = validate_and_massage_point_structures(
			contour,
			POINT_ATTRIBUTES_FORMAT1,
			true,
			validate,
		)?;
		for point in massaged {
			pen.add_point(
				(point.x, point.y),
				point.segment_type()?,
				point.smooth,
				point.name.as_deref(),
				None,
			)?;
		}
	}
	pen.end_path()
}

fn build_outline_format2(
	pen: &mut dyn NumberPointPen,
	outline: &xml_reader::Element,
	identifiers: &mut HashSet<String>,
	validate: bool,
) -> Result<()> {
	for element in outline.elements() {
		match element.name.as_str() {
			"contour" => build_outline_contour_format2(pen, element, identifiers, validate)?,
			"component" => build_outline_component(pen, element, Some(identifiers), validate)?,
			_ => {
				return glif_err(format!(
					"Unknown element in outline element: {}",
					element.name
				));
			}
		}
	}
	Ok(())
}

fn build_outline_contour_format2(
	pen: &mut dyn NumberPointPen,
	contour: &xml_reader::Element,
	identifiers: &mut HashSet<String>,
	validate: bool,
) -> Result<()> {
	if validate {
		for (attr, _) in &contour.attrs {
			if !CONTOUR_ATTRIBUTES_FORMAT2.contains(&attr.as_str()) {
				return glif_err(format!("Unknown attribute in contour element: {attr}"));
			}
		}
	}
	let identifier = contour.attr("identifier");
	if let Some(identifier) = identifier {
		if validate {
			if identifiers.contains(identifier) {
				return glif_err(format!(
					"The identifier {identifier} is used more than once."
				));
			}
			if !is_valid_identifier(identifier) {
				return glif_err(format!("The contour identifier {identifier} is not valid."));
			}
		}
		identifiers.insert(identifier.to_owned());
	}
	pen.begin_path(identifier)?;
	if child_count(contour) > 0 {
		let massaged = validate_and_massage_point_structures(
			contour,
			POINT_ATTRIBUTES_FORMAT2,
			false,
			validate,
		)?;
		for point in massaged {
			if let Some(identifier) = &point.identifier {
				if validate {
					if identifiers.contains(identifier) {
						return glif_err(format!(
							"The identifier {identifier} is used more than once."
						));
					}
					if !is_valid_identifier(identifier) {
						return glif_err(format!("The identifier {identifier} is not valid."));
					}
				}
				identifiers.insert(identifier.clone());
			}
			pen.add_point(
				(point.x, point.y),
				point.segment_type()?,
				point.smooth,
				point.name.as_deref(),
				point.identifier.as_deref(),
			)?;
		}
	}
	pen.end_path()
}

/// `_buildOutlineComponentFormat1` (without identifiers) and
/// `_buildOutlineComponentFormat2`.
fn build_outline_component(
	pen: &mut dyn NumberPointPen,
	component: &xml_reader::Element,
	identifiers: Option<&mut HashSet<String>>,
	validate: bool,
) -> Result<()> {
	let allowed = if identifiers.is_some() {
		COMPONENT_ATTRIBUTES_FORMAT2
	} else {
		COMPONENT_ATTRIBUTES_FORMAT1
	};
	if validate {
		if child_count(component) > 0 {
			return glif_err("Unknown child elements of component element.");
		}
		for (attr, _) in &component.attrs {
			if !allowed.contains(&attr.as_str()) {
				return glif_err(format!("Unknown attribute in component element: {attr}"));
			}
		}
	}
	// LENIENT(L35): without validation, fontTools fails an assertion on a missing
	// base.
	let Some(base) = component.attr("base") else {
		return glif_err("The base attribute is not defined in the component.");
	};
	let mut transformation = [Number::Int(0); 6];
	for (i, (attr, default)) in TRANSFORMATION_INFO.into_iter().enumerate() {
		// `_number(component.get(attr) or default)`: an empty value is the default.
		transformation[i] = match component.attr(attr) {
			Some(v) if !v.is_empty() => number(v)?,
			_ => Number::Int(default),
		};
	}
	let mut identifier = None;
	if let Some(identifiers) = identifiers {
		identifier = component.attr("identifier");
		if let Some(identifier) = identifier {
			if validate {
				if identifiers.contains(identifier) {
					return glif_err(format!(
						"The identifier {identifier} is used more than once."
					));
				}
				if !is_valid_identifier(identifier) {
					return glif_err(format!("The identifier {identifier} is not valid."));
				}
			}
			identifiers.insert(identifier.to_owned());
		}
	}
	pen.add_component(base, transformation, identifier)
}

/// A point of `_validateAndMassagePointStructures`.
#[derive(Debug)]
struct MassagedPoint {
	x: Number,
	y: Number,
	segment_type: Option<String>,
	smooth: bool,
	name: Option<String>,
	identifier: Option<String>,
}

impl MassagedPoint {
	/// LENIENT(L35): without validation, fontTools passes unknown point types on to
	/// the pen; here they are an error.
	fn segment_type(&self) -> Result<Option<SegmentType>> {
		match &self.segment_type {
			None => Ok(None),
			Some(t) => SegmentType::parse(t)
				.map(Some)
				.ok_or_else(|| UfoLibError::GlifLib(format!("Unknown point type: {t}"))),
		}
	}
}

fn validate_and_massage_point_structures(
	contour: &xml_reader::Element,
	point_attributes: &[&str],
	open_contour_off_curve_leniency: bool,
	validate: bool,
) -> Result<Vec<MassagedPoint>> {
	let mut last_on_curve_point: Option<usize> = None;
	let mut have_off_curve_point = false;
	let mut massaged: Vec<MassagedPoint> = Vec::new();
	for (index, element) in contour.elements().enumerate() {
		if element.name != "point" {
			return glif_err(format!(
				"Unknown child element ({}) of contour element.",
				element.name
			));
		}
		if validate {
			for (attr, _) in &element.attrs {
				if !point_attributes.contains(&attr.as_str()) {
					return glif_err(format!("Unknown attribute in point element: {attr}"));
				}
			}
			if child_count(element) > 0 {
				return glif_err("Unknown child elements in point element.");
			}
		}
		let coordinate = |attr: &str| -> Result<Number> {
			match element.attr(attr) {
				Some(v) => number(v),
				None => glif_err(format!(
					"Required {attr} attribute is missing in point element."
				)),
			}
		};
		let x = coordinate("x")?;
		let y = coordinate("y")?;
		let point_type = element.attr("type").unwrap_or("offcurve");
		if validate && !POINT_TYPE_OPTIONS.contains(&point_type) {
			return glif_err(format!("Unknown point type: {point_type}"));
		}
		let segment_type = (point_type != "offcurve").then(|| point_type.to_owned());
		if segment_type.is_none() {
			have_off_curve_point = true;
		} else {
			last_on_curve_point = Some(index);
		}
		if validate && point_type == "move" && index != 0 {
			return glif_err("A move point occurs after the first point in the contour.");
		}
		let smooth = element.attr("smooth").unwrap_or("no");
		if validate && smooth != "no" && smooth != "yes" {
			return glif_err(format!("Unknown point smooth value: {smooth}"));
		}
		let smooth = smooth == "yes";
		if validate && smooth && segment_type.is_none() {
			return glif_err("smooth attribute set in an offcurve point.");
		}
		massaged.push(MassagedPoint {
			x,
			y,
			segment_type,
			smooth,
			name: element.attr("name").map(str::to_owned),
			identifier: element.attr("identifier").map(str::to_owned),
		});
	}
	if open_contour_off_curve_leniency
		&& massaged.first().and_then(|p| p.segment_type.as_deref()) == Some("move")
	{
		// remove offcurves that precede a move. this is technically illegal,
		// but we let it slide because there are fonts out there in the wild like this.
		while massaged.last().is_some_and(|p| p.segment_type.is_none()) {
			massaged.pop();
		}
	}
	// validate the off-curves in the segments
	if validate
		&& have_off_curve_point
		&& let Some(last_on_curve_point) = last_on_curve_point
	{
		// we only care about how many offCurves there are before an onCurve
		// filter out the trailing offCurves
		let mut off_curves_count = (massaged.len() as i64) - 1 - last_on_curve_point as i64;
		for point in &massaged {
			match point.segment_type.as_deref() {
				None => off_curves_count += 1,
				Some(segment_type) => {
					if off_curves_count != 0 {
						match segment_type {
							"move" => return glif_err("move can not have an offcurve."),
							"line" => return glif_err("line can not have an offcurve."),
							"curve" if off_curves_count > 2 => {
								return glif_err("Too many offcurves defined for curve.");
							}
							_ => {}
						}
					}
					off_curves_count = 0;
				}
			}
		}
	}
	Ok(massaged)
}

// ---------------------
// Misc Helper Functions
// ---------------------

/// `_number`: an `int` or a `float`, whichever the string indicates.
///
/// ```
/// use fontoxide_ufolib::glif_lib::number;
/// use fontoxide_ufolib::Number;
/// assert!(matches!(number("1").unwrap(), Number::Int(1)));
/// assert!(matches!(number("1.0").unwrap(), Number::Float(f) if f == 1.0));
/// assert!(number("a").is_err());
/// ```
pub fn number(s: &str) -> Result<Number> {
	if let Some(i) = parse_int(s) {
		return Ok(match i64::try_from(i) {
			Ok(i) => Number::Int(i),
			// LENIENT(L35): Python ints are unbounded; larger ones become floats.
			Err(_) => Number::Float(i as f64),
		});
	}
	parse_float(s)
		.map(Number::Float)
		.ok_or_else(|| UfoLibError::GlifLib(format!("Could not convert {s} to an int or float.")))
}

// --------------------
// Rapid Value Fetching
// --------------------

fn parse_for_fetch(glif: &[u8]) -> Result<xml_reader::Element> {
	let text = plistlib::decode_xml(glif).map_err(|e| UfoLibError::Xml(e.to_string()))?;
	xml_reader::parse(text.strip_prefix('\u{feff}').unwrap_or(&text))
		.map_err(|e| UfoLibError::Xml(e.0))
}

/// Visits the elements in document order with the name of their parent;
/// stops when `visit` returns false.
fn walk_elements<'a>(
	element: &'a xml_reader::Element,
	parent: Option<&str>,
	visit: &mut dyn FnMut(&'a xml_reader::Element, Option<&str>) -> bool,
) -> bool {
	if !visit(element, parent) {
		return false;
	}
	for child in element.elements() {
		if !walk_elements(child, Some(&element.name), visit) {
			return false;
		}
	}
	true
}

/// `_fetchUnicodes`: the Unicode values of the `unicode` elements of a
/// `glyph` element.
///
/// LENIENT(L35): the whole document is parsed, so a syntax error anywhere is an
/// error.
fn fetch_unicodes(glif: &[u8]) -> Result<Vec<i64>> {
	let root = parse_for_fetch(glif)?;
	let mut unicodes: Vec<i64> = Vec::new();
	walk_elements(&root, None, &mut |e, parent| {
		if e.name == "unicode"
			&& parent == Some("glyph")
			&& let Some(v) = e
				.attr("hex")
				.and_then(parse_hex)
				.and_then(|v| i64::try_from(v).ok())
			&& !unicodes.contains(&v)
		{
			unicodes.push(v);
		}
		true
	});
	Ok(unicodes)
}

/// `_fetchImageFileName`: the file name of the first `image` element of a
/// `glyph` element.
fn fetch_image_file_name(glif: &[u8]) -> Result<Option<String>> {
	let root = parse_for_fetch(glif)?;
	let mut file_name = None;
	walk_elements(&root, None, &mut |e, parent| {
		if e.name == "image" && parent == Some("glyph") {
			file_name = e.attr("fileName").map(str::to_owned);
			return false;
		}
		true
	});
	Ok(file_name)
}

/// `_fetchComponentBases`: the base glyphs of the components of the first
/// `outline` element.
fn fetch_component_bases(glif: &[u8]) -> Result<Vec<String>> {
	let root = parse_for_fetch(glif)?;
	let mut bases = Vec::new();
	walk_elements(&root, None, &mut |e, _| {
		if e.name == "outline" {
			for child in e.elements() {
				if child.name == "component"
					&& let Some(base) = child.attr("base")
				{
					bases.push(base.to_owned());
				}
			}
			return false;
		}
		true
	});
	Ok(bases)
}

// --------------
// GLIF Point Pen
// --------------

/// `GLIFPointPen`: a point pen that writes the `<outline>` element of a
/// `.glif` file.
///
/// It takes Python numbers through [`NumberPointPen`] and `f64`s through
/// [`PointPen`]; `f64` coordinates with integral values are written as
/// `int`s.
#[derive(Debug)]
pub struct GlifPointPen {
	format_version: GlifFormatVersion,
	identifiers: HashSet<String>,
	outline: Element,
	contour: Option<usize>,
	prev_off_curve_count: usize,
	prev_point_types: Vec<&'static str>,
	validate: bool,
}

impl GlifPointPen {
	/// `GLIFPointPen(element, formatVersion, identifiers, validate)`.
	pub fn new(
		element: Element,
		format_version: Option<FormatVersionInput>,
		identifiers: HashSet<String>,
		validate: bool,
	) -> Result<Self> {
		let format_version = GlifFormatVersion::normalize(format_version).ok_or_else(|| {
			UfoLibError::Value(format!(
				"Unsupported format version: {}",
				format_version.map_or("None".to_owned(), |v| v.to_string())
			))
		})?;
		Ok(GlifPointPen {
			format_version,
			identifiers,
			outline: element,
			contour: None,
			prev_off_curve_count: 0,
			prev_point_types: Vec::new(),
			validate,
		})
	}

	/// The outline element and the identifiers used.
	pub fn into_parts(self) -> (Element, HashSet<String>) {
		(self.outline, self.identifiers)
	}

	fn check_identifier(&self, identifier: &str) -> Result<()> {
		if self.validate {
			if self.identifiers.contains(identifier) {
				return glif_err(format!("identifier used more than once: {identifier}"));
			}
			if !is_valid_identifier(identifier) {
				return glif_err(format!("identifier not formatted properly: {identifier}"));
			}
		}
		Ok(())
	}

	fn begin_path_impl(&mut self, identifier: Option<&str>) -> Result<()> {
		let mut contour = Element::new("contour");
		if let Some(identifier) = identifier
			&& self.format_version.major() >= 2
		{
			self.check_identifier(identifier)?;
			contour.set("identifier", identifier);
			self.identifiers.insert(identifier.to_owned());
		}
		self.outline.append(contour);
		self.contour = Some(self.outline.children.len() - 1);
		self.prev_off_curve_count = 0;
		Ok(())
	}

	fn current_contour(&mut self) -> Result<&mut Element> {
		// fontTools fails with an AttributeError on `None`.
		match self.contour {
			Some(i) => self
				.outline
				.children
				.get_mut(i)
				.ok_or_else(|| UfoLibError::Value("no current contour".into())),
			None => Err(UfoLibError::Value(
				"'NoneType' object has no attribute: no current contour".into(),
			)),
		}
	}

	fn end_path_impl(&mut self) -> Result<()> {
		if self.prev_point_types.first() == Some(&"move")
			&& self.validate
			&& self.prev_point_types.last() == Some(&"offcurve")
		{
			return glif_err("open contour has loose offcurve point");
		}
		// prevent lxml from writing self-closing tags
		let contour = self.current_contour()?;
		if contour.is_empty() {
			contour.text = Some("\n  ".into());
		}
		self.contour = None;
		self.prev_off_curve_count = 0;
		self.prev_point_types.clear();
		Ok(())
	}

	/// `addPoint` with any values, as Python's duck typing allows: with
	/// validation, coordinates must be numbers; without, they are written with
	/// `repr`. A segment type of `"offcurve"` is an off-curve point.
	pub fn add_point_values(
		&mut self,
		pt: (&Value, &Value),
		segment_type: Option<&str>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		let mut point = Element::new("point");
		// coordinates
		if self.validate && !(is_number(pt.0) && is_number(pt.1)) {
			return glif_err("coordinates must be int or float");
		}
		point.set("x", value_repr(pt.0));
		point.set("y", value_repr(pt.1));
		// segment type
		let segment_type = segment_type.filter(|t| *t != "offcurve");
		if self.validate {
			if segment_type == Some("move") && !self.prev_point_types.is_empty() {
				return glif_err(
					"move occurs after a point has already been added to the contour.",
				);
			}
			if let Some(t @ ("move" | "line")) = segment_type
				&& self.prev_point_types.last() == Some(&"offcurve")
			{
				return glif_err(format!("offcurve occurs before {t} point."));
			}
			if segment_type == Some("curve") && self.prev_off_curve_count > 2 {
				return glif_err("too many offcurve points before curve point.");
			}
		}
		let segment_type: &'static str = match segment_type {
			Some(t) => {
				point.set("type", t);
				match t {
					"move" => "move",
					"line" => "line",
					"curve" => "curve",
					"qcurve" => "qcurve",
					_ => "other",
				}
			}
			None => "offcurve",
		};
		if segment_type == "offcurve" {
			self.prev_off_curve_count += 1;
		} else {
			self.prev_off_curve_count = 0;
		}
		self.prev_point_types.push(segment_type);
		// smooth
		if smooth {
			if self.validate && segment_type == "offcurve" {
				return glif_err("can't set smooth in an offcurve point.");
			}
			point.set("smooth", "yes");
		}
		// name
		if let Some(name) = name {
			point.set("name", name);
		}
		// identifier
		if let Some(identifier) = identifier
			&& self.format_version.major() >= 2
		{
			self.check_identifier(identifier)?;
			point.set("identifier", identifier);
			self.identifiers.insert(identifier.to_owned());
		}
		self.current_contour()?.append(point);
		Ok(())
	}

	/// `addComponent` with any values, as Python's duck typing allows: with
	/// validation, the transformation values must be numbers; without, they are
	/// written with `repr`.
	pub fn add_component_values(
		&mut self,
		glyph_name: &str,
		transformation: &[Value],
		identifier: Option<&str>,
	) -> Result<()> {
		let mut component = Element::new("component");
		component.set("base", glyph_name);
		for ((attr, default), value) in TRANSFORMATION_INFO.into_iter().zip(transformation) {
			if self.validate && !is_number(value) {
				return glif_err("transformation values must be int or float");
			}
			if differs(value, default) {
				component.set(attr, value_repr(value));
			}
		}
		if let Some(identifier) = identifier
			&& self.format_version.major() >= 2
		{
			self.check_identifier(identifier)?;
			component.set("identifier", identifier);
			self.identifiers.insert(identifier.to_owned());
		}
		self.outline.append(component);
		Ok(())
	}
}

impl NumberPointPen for GlifPointPen {
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()> {
		self.begin_path_impl(identifier)
	}

	fn end_path(&mut self) -> Result<()> {
		self.end_path_impl()
	}

	fn add_point(
		&mut self,
		pt: (Number, Number),
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		self.add_point_values(
			(&pt.0.to_value(), &pt.1.to_value()),
			segment_type.map(SegmentType::as_str),
			smooth,
			name,
			identifier,
		)
	}

	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: [Number; 6],
		identifier: Option<&str>,
	) -> Result<()> {
		self.add_component_values(
			base_glyph_name,
			&transformation.map(Number::to_value),
			identifier,
		)
	}
}

impl PointPen for GlifPointPen {
	fn begin_path(&mut self, identifier: Option<&str>) -> fontoxide_pens::Result<()> {
		Ok(self.begin_path_impl(identifier)?)
	}

	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		Ok(self.end_path_impl()?)
	}

	fn add_point(
		&mut self,
		pt: (f64, f64),
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> fontoxide_pens::Result<()> {
		Ok(NumberPointPen::add_point(
			self,
			(Number::from_f64(pt.0), Number::from_f64(pt.1)),
			segment_type,
			smooth,
			name,
			identifier,
		)?)
	}

	fn add_component(
		&mut self,
		base_glyph_name: &str,
		t: Transform,
		identifier: Option<&str>,
	) -> fontoxide_pens::Result<()> {
		let transformation =
			[t.xx, t.xy, t.yx, t.yy, t.dx, t.dy].map(|v| Number::from_f64(v).to_value());
		Ok(self.add_component_values(base_glyph_name, &transformation, identifier)?)
	}
}
