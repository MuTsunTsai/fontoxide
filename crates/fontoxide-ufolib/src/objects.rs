//! A small in-memory font model in the manner of ufoLib2's objects
//! (`ufoLib2.objects`), which fontTools' tests and command lines use to open,
//! change and save UFOs (for example `fontTools.cu2qu.ufo`).
//!
//! Only what such code needs is here: [`Font`] with its info, groups,
//! kerning, lib, features and layers; [`Layer`]s of [`Glyph`]s made of
//! [`Contour`]s of [`Point`]s and [`Component`]s; and saving as ufoLib2 does
//! (UFO 3, in place or elsewhere). Data files and images are kept and copied
//! when saving elsewhere, but cannot be changed. As in ufoLib2, glyphs and
//! layers other than the default one are read when first used ("lazy"), and
//! saving in place only writes what was read.
//!
//! Coordinates are [`Number`]s, so that the `int`s and `float`s of the GLIF
//! files stay apart as they do in Python; drawing into a glyph with `f64`
//! pens takes integral values as `int`s ([`Number::from_f64`]).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use fontoxide_pens::point_pen::SegmentToPointPen;
use fontoxide_pens::{Pen, PointPen, SegmentType, Transform};

use crate::glif_lib::Glyph as GlifGlyph;
use crate::{
	DEFAULT_LAYER_NAME, Dict, GlifPointPen, GlyphSet, Groups, Kerning, Number, NumberPointPen,
	Result, UfoFileStructure, UfoLibError, UfoReader, UfoWriter, UfoWriterOptions, Value,
	WriterGlyphSetOptions,
};

/// `public.objectLibs`.
const OBJECT_LIBS_KEY: &str = "public.objectLibs";

/// A point of a contour (`ufoLib2.objects.Point`).
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
	/// `x`.
	pub x: Number,
	/// `y`.
	pub y: Number,
	/// `type`: the segment type of an on-curve point, `None` for an
	/// off-curve point.
	pub segment_type: Option<SegmentType>,
	/// `smooth`.
	pub smooth: bool,
	/// `name`.
	pub name: Option<String>,
	/// `identifier`.
	pub identifier: Option<String>,
}

impl Point {
	/// The coordinates as `f64`s.
	pub fn pt(&self) -> (f64, f64) {
		(self.x.to_f64(), self.y.to_f64())
	}
}

/// A contour (`ufoLib2.objects.Contour`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Contour {
	/// `points`.
	pub points: Vec<Point>,
	/// `identifier`.
	pub identifier: Option<String>,
}

impl Contour {
	/// `drawPoints`.
	pub fn draw_points(&self, pen: &mut dyn NumberPointPen) -> Result<()> {
		pen.begin_path(self.identifier.as_deref())?;
		for p in &self.points {
			pen.add_point(
				(p.x, p.y),
				p.segment_type,
				p.smooth,
				p.name.as_deref(),
				p.identifier.as_deref(),
			)?;
		}
		pen.end_path()
	}
}

/// A component (`ufoLib2.objects.Component`).
#[derive(Debug, Clone, PartialEq)]
pub struct Component {
	/// `baseGlyph`.
	pub base_glyph: String,
	/// `transformation`: `xx, xy, yx, yy, dx, dy`.
	pub transformation: [Number; 6],
	/// `identifier`.
	pub identifier: Option<String>,
}

/// A glyph (`ufoLib2.objects.Glyph`).
#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
	/// `name`.
	pub name: Option<String>,
	/// `width`, 0 by default.
	pub width: Number,
	/// `height`, 0 by default.
	pub height: Number,
	/// `unicodes`.
	pub unicodes: Vec<i64>,
	/// `note`.
	pub note: Option<String>,
	/// `image`, as GLIF reading gives it (`fileName`, the transformation and
	/// `color`); `None` without an image.
	pub image: Option<Dict>,
	/// `lib`.
	pub lib: Dict,
	/// `anchors`, as GLIF reading gives them.
	pub anchors: Vec<Dict>,
	/// `guidelines`, as GLIF reading gives them.
	pub guidelines: Vec<Dict>,
	/// `contours`.
	pub contours: Vec<Contour>,
	/// `components`.
	pub components: Vec<Component>,
}

impl Default for Glyph {
	fn default() -> Self {
		Glyph::new(None)
	}
}

impl Glyph {
	/// `Glyph(name)`: an empty glyph.
	pub fn new(name: Option<&str>) -> Glyph {
		Glyph {
			name: name.map(str::to_owned),
			width: Number::Int(0),
			height: Number::Int(0),
			unicodes: Vec::new(),
			note: None,
			image: None,
			lib: Dict::new(),
			anchors: Vec::new(),
			guidelines: Vec::new(),
			contours: Vec::new(),
			components: Vec::new(),
		}
	}

	/// `len(glyph)`: the number of contours.
	pub fn len(&self) -> usize {
		self.contours.len()
	}

	/// Whether the glyph has no contours.
	pub fn is_empty(&self) -> bool {
		self.contours.is_empty()
	}

	/// `clearContours`.
	pub fn clear_contours(&mut self) {
		self.contours.clear();
	}

	/// `drawPoints`, with the numbers as they are: the contours, then the
	/// components.
	pub fn draw_points_numbers(&self, pen: &mut dyn NumberPointPen) -> Result<()> {
		for contour in &self.contours {
			contour.draw_points(pen)?;
		}
		for c in &self.components {
			pen.add_component(&c.base_glyph, c.transformation, c.identifier.as_deref())?;
		}
		Ok(())
	}

	/// `drawPoints`.
	pub fn draw_points(&self, pen: &mut dyn PointPen) -> Result<()> {
		self.draw_points_numbers(&mut crate::PointPenAdapter(pen))
	}

	/// `getPointPen`: a point pen that adds what it receives to the glyph.
	pub fn point_pen(&mut self) -> GlyphPointPen<'_> {
		GlyphPointPen {
			glyph: self,
			contour: None,
		}
	}

	/// Draws into the glyph through `getPen()`, a [`SegmentToPointPen`]
	/// (guessing smooth points) onto [`Glyph::point_pen`].
	pub fn draw_with_pen<R>(
		&mut self,
		f: impl FnOnce(&mut dyn Pen) -> fontoxide_pens::Result<R>,
	) -> fontoxide_pens::Result<R> {
		let mut point_pen = self.point_pen();
		let mut pen = SegmentToPointPen::new(&mut point_pen, true);
		f(&mut pen)
	}

	/// The identifiers used in the glyph (`_fetch_glyph_identifiers`).
	fn identifiers(&self) -> HashSet<String> {
		let mut out = HashSet::new();
		let dict_ids = |dicts: &[Dict], out: &mut HashSet<String>| {
			for d in dicts {
				if let Some(Value::String(id)) = d.get("identifier") {
					out.insert(id.clone());
				}
			}
		};
		dict_ids(&self.anchors, &mut out);
		dict_ids(&self.guidelines, &mut out);
		for contour in &self.contours {
			out.extend(contour.identifier.iter().cloned());
			for p in &contour.points {
				out.extend(p.identifier.iter().cloned());
			}
		}
		for c in &self.components {
			out.extend(c.identifier.iter().cloned());
		}
		out
	}

	/// The attributes GLIF reading sets (`readGlyph`), and those it writes.
	fn set_attrs(&mut self, attrs: GlifGlyph) {
		if let Some(w) = attrs.width {
			self.width = w;
		}
		if let Some(h) = attrs.height {
			self.height = h;
		}
		if let Some(u) = attrs.unicodes {
			self.unicodes = u;
		}
		if attrs.note.is_some() {
			self.note = attrs.note;
		}
		// `glyph.image = None` clears it; ufoLib2 has an empty image then.
		self.image = attrs.image;
		if let Some(lib) = attrs.lib {
			self.lib = lib;
		}
		if let Some(a) = attrs.anchors {
			self.anchors = a;
		}
		if let Some(g) = attrs.guidelines {
			self.guidelines = g;
		}
	}

	fn glif_attrs(&self) -> GlifGlyph {
		GlifGlyph {
			name: self.name.clone(),
			width: Some(self.width),
			height: Some(self.height),
			unicodes: (!self.unicodes.is_empty()).then(|| self.unicodes.clone()),
			note: self.note.clone().filter(|n| !n.is_empty()),
			lib: (!self.lib.is_empty()).then(|| self.lib.clone()),
			image: self.image.clone().filter(|i| i.contains_key("fileName")),
			guidelines: (!self.guidelines.is_empty()).then(|| self.guidelines.clone()),
			anchors: (!self.anchors.is_empty()).then(|| self.anchors.clone()),
		}
	}

	fn read(glyph_set: &GlyphSet, name: &str) -> Result<Glyph> {
		let mut glyph = Glyph::new(Some(name));
		let mut attrs = GlifGlyph::default();
		{
			let mut pen = glyph.point_pen();
			glyph_set.read_glyph(name, Some(&mut attrs), Some(&mut pen), None)?;
		}
		glyph.set_attrs(attrs);
		Ok(glyph)
	}
}

/// `GlyphPointPen`: adds what it receives to a glyph. It is both a
/// [`NumberPointPen`] and an `f64` [`PointPen`].
#[derive(Debug)]
pub struct GlyphPointPen<'g> {
	glyph: &'g mut Glyph,
	contour: Option<Contour>,
}

impl GlyphPointPen<'_> {
	fn contour(&mut self) -> Result<&mut Contour> {
		self.contour
			.as_mut()
			.ok_or_else(|| UfoLibError::Pen(fontoxide_pens::Error::Pen("Path not begun".into())))
	}
}

impl NumberPointPen for GlyphPointPen<'_> {
	fn begin_path(&mut self, identifier: Option<&str>) -> Result<()> {
		self.contour = Some(Contour {
			points: Vec::new(),
			identifier: identifier.map(str::to_owned),
		});
		Ok(())
	}

	fn end_path(&mut self) -> Result<()> {
		let contour = self
			.contour
			.take()
			.ok_or_else(|| UfoLibError::Pen(fontoxide_pens::Error::Pen("Path not begun".into())))?;
		self.glyph.contours.push(contour);
		Ok(())
	}

	fn add_point(
		&mut self,
		pt: (Number, Number),
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> Result<()> {
		self.contour()?.points.push(Point {
			x: pt.0,
			y: pt.1,
			segment_type,
			smooth,
			name: name.map(str::to_owned),
			identifier: identifier.map(str::to_owned),
		});
		Ok(())
	}

	fn add_component(
		&mut self,
		base_glyph_name: &str,
		transformation: [Number; 6],
		identifier: Option<&str>,
	) -> Result<()> {
		self.glyph.components.push(Component {
			base_glyph: base_glyph_name.to_owned(),
			transformation,
			identifier: identifier.map(str::to_owned),
		});
		Ok(())
	}
}

fn pen_error(e: UfoLibError) -> fontoxide_pens::Error {
	match e {
		UfoLibError::Pen(e) => e,
		e => fontoxide_pens::Error::Glyph(Box::new(e)),
	}
}

impl PointPen for GlyphPointPen<'_> {
	fn begin_path(&mut self, identifier: Option<&str>) -> fontoxide_pens::Result<()> {
		NumberPointPen::begin_path(self, identifier).map_err(pen_error)
	}

	fn end_path(&mut self) -> fontoxide_pens::Result<()> {
		NumberPointPen::end_path(self).map_err(pen_error)
	}

	fn add_point(
		&mut self,
		pt: (f64, f64),
		segment_type: Option<SegmentType>,
		smooth: bool,
		name: Option<&str>,
		identifier: Option<&str>,
	) -> fontoxide_pens::Result<()> {
		NumberPointPen::add_point(
			self,
			(Number::from_f64(pt.0), Number::from_f64(pt.1)),
			segment_type,
			smooth,
			name,
			identifier,
		)
		.map_err(pen_error)
	}

	fn add_component(
		&mut self,
		base_glyph_name: &str,
		t: Transform,
		identifier: Option<&str>,
	) -> fontoxide_pens::Result<()> {
		let t = [t.xx, t.xy, t.yx, t.yy, t.dx, t.dy].map(Number::from_f64);
		NumberPointPen::add_component(self, base_glyph_name, t, identifier).map_err(pen_error)
	}
}

/// A layer (`ufoLib2.objects.Layer`): glyphs by name, read when first used
/// if the layer was read from a UFO.
#[derive(Debug)]
pub struct Layer {
	name: String,
	/// The glyphs; `None` for a glyph not read yet.
	glyphs: IndexMap<String, Option<Glyph>>,
	glyph_set: Option<GlyphSet>,
	/// `color`.
	pub color: Option<String>,
	/// `lib`.
	pub lib: Dict,
}

impl Layer {
	/// `Layer(name)`: an empty layer.
	pub fn new(name: &str) -> Layer {
		Layer {
			name: name.to_owned(),
			glyphs: IndexMap::new(),
			glyph_set: None,
			color: None,
			lib: Dict::new(),
		}
	}

	/// `Layer.read(name, glyphSet, lazy=True)` and `readLayerInfo`.
	fn read(name: &str, glyph_set: GlyphSet) -> Result<Layer> {
		let info = glyph_set.read_layer_info(None)?;
		let mut layer = Layer::new(name);
		layer.glyphs = glyph_set.keys().into_iter().map(|n| (n, None)).collect();
		if let Some(Value::String(color)) = info.get("color") {
			layer.color = Some(color.clone());
		}
		if let Some(Value::Dict(lib)) = info.get("lib") {
			layer.lib = lib.clone();
		}
		layer.glyph_set = Some(glyph_set);
		Ok(layer)
	}

	/// `name`.
	pub fn name(&self) -> &str {
		&self.name
	}

	/// `keys()`: the glyph names.
	pub fn keys(&self) -> impl Iterator<Item = &str> {
		self.glyphs.keys().map(String::as_str)
	}

	/// `len(layer)`.
	pub fn len(&self) -> usize {
		self.glyphs.len()
	}

	/// Whether the layer has no glyphs.
	pub fn is_empty(&self) -> bool {
		self.glyphs.is_empty()
	}

	/// `name in layer`.
	pub fn contains(&self, name: &str) -> bool {
		self.glyphs.contains_key(name)
	}

	/// `layer[name]` (`loadGlyph` if needed); `None` if there is no such
	/// glyph.
	pub fn get_mut(&mut self, name: &str) -> Result<Option<&mut Glyph>> {
		let Some(slot) = self.glyphs.get_mut(name) else {
			return Ok(None);
		};
		if slot.is_none() {
			let Some(glyph_set) = &self.glyph_set else {
				return Err(UfoLibError::Key(fontoxide_misc::py::str_repr(name)));
			};
			*slot = Some(Glyph::read(glyph_set, name)?);
		}
		Ok(slot.as_mut())
	}

	/// `del layer[name]`.
	pub fn remove(&mut self, name: &str) -> Result<()> {
		match self.glyphs.shift_remove(name) {
			Some(_) => Ok(()),
			None => Err(UfoLibError::Key(fontoxide_misc::py::str_repr(name))),
		}
	}

	/// `newGlyph`.
	pub fn new_glyph(&mut self, name: &str) -> Result<&mut Glyph> {
		if self.glyphs.contains_key(name) {
			return Err(UfoLibError::Key(format!(
				"glyph named '{name}' already exists"
			)));
		}
		let slot = self
			.glyphs
			.entry(name.to_owned())
			.or_insert(Some(Glyph::new(Some(name))));
		slot.as_mut()
			.ok_or_else(|| UfoLibError::Key(fontoxide_misc::py::str_repr(name)))
	}

	/// `insertGlyph`-like `layer[name] = glyph`.
	pub fn insert(&mut self, name: &str, mut glyph: Glyph) {
		glyph.name = Some(name.to_owned());
		self.glyphs.insert(name.to_owned(), Some(glyph));
	}

	fn load_all(&mut self) -> Result<()> {
		let names: Vec<String> = self
			.glyphs
			.iter()
			.filter(|(_, g)| g.is_none())
			.map(|(n, _)| n.clone())
			.collect();
		for name in names {
			self.get_mut(&name)?;
		}
		Ok(())
	}

	/// `Layer.write(glyphSet, saveAs)`.
	fn write(&mut self, glyph_set: &mut GlyphSet, save_as: bool) -> Result<()> {
		if !save_as {
			let stale: Vec<String> = glyph_set
				.keys()
				.into_iter()
				.filter(|n| !self.glyphs.contains_key(n))
				.collect();
			for name in stale {
				glyph_set.delete_glyph(&name)?;
			}
		}
		if save_as {
			self.load_all()?;
		}
		for (name, glyph) in &mut self.glyphs {
			let Some(glyph) = glyph else {
				continue;
			};
			let identifiers = glyph.identifiers();
			prune_object_libs(&mut glyph.lib, &identifiers);
			let attrs = glyph.glif_attrs();
			let glyph = &*glyph;
			let mut draw = |pen: &mut GlifPointPen| glyph.draw_points_numbers(pen);
			glyph_set.write_glyph(name, Some(&attrs), Some(&mut draw), None, None)?;
		}
		glyph_set.write_contents()?;
		let mut info = Dict::new();
		if let Some(color) = &self.color {
			info.insert("color".into(), Value::String(color.clone()));
		}
		info.insert("lib".into(), Value::Dict(self.lib.clone()));
		glyph_set.write_layer_info(&info, None)?;
		if save_as {
			self.glyph_set = None;
		}
		Ok(())
	}
}

/// `_prune_object_libs`: drops the object libs of objects that are gone, and
/// empty ones.
fn prune_object_libs(lib: &mut Dict, identifiers: &HashSet<String>) {
	if let Some(Value::Dict(object_libs)) = lib.get_mut(OBJECT_LIBS_KEY) {
		object_libs.retain(|k, v| identifiers.contains(k) && truthy(v));
	}
}

fn truthy(v: &Value) -> bool {
	match v {
		Value::Dict(d) => !d.is_empty(),
		Value::Array(a) => !a.is_empty(),
		Value::String(s) => !s.is_empty(),
		Value::Data(d) => !d.is_empty(),
		Value::Bool(b) => *b,
		Value::Integer(i) => *i != 0,
		Value::Real(f) => *f != 0.0,
		_ => true,
	}
}

/// A font (`ufoLib2.objects.Font`).
#[derive(Debug)]
pub struct Font {
	/// The names of the layers, in order (`layers.layerOrder`).
	layer_order: Vec<String>,
	/// The default layer, always read.
	default_layer: Layer,
	/// The other layers; `None` for a layer not read yet.
	other_layers: IndexMap<String, Option<Layer>>,
	/// `info`: font info attributes ([`UfoReader::read_info`]).
	pub info: Dict,
	/// `features`: the text of `features.fea`.
	pub features: String,
	/// `groups`.
	pub groups: Groups,
	/// `kerning`.
	pub kerning: Kerning,
	/// `lib`.
	pub lib: Dict,
	/// The files of the `data` directory, copied when saving elsewhere.
	data: Vec<String>,
	/// The images, copied when saving elsewhere.
	images: Vec<String>,
	path: Option<PathBuf>,
	reader: Option<UfoReader>,
	file_structure: Option<UfoFileStructure>,
	validate: bool,
}

impl Default for Font {
	fn default() -> Self {
		Font::new()
	}
}

impl Font {
	/// `Font()`: a new empty font, with an empty default layer.
	pub fn new() -> Font {
		Font {
			layer_order: vec![DEFAULT_LAYER_NAME.to_owned()],
			default_layer: Layer::new(DEFAULT_LAYER_NAME),
			other_layers: IndexMap::new(),
			info: Dict::new(),
			features: String::new(),
			groups: Groups::new(),
			kerning: Kerning::new(),
			lib: Dict::new(),
			data: Vec::new(),
			images: Vec::new(),
			path: None,
			reader: None,
			file_structure: None,
			validate: true,
		}
	}

	/// `Font.open(path, lazy=True, validate)`.
	pub fn open(path: impl AsRef<Path>, validate: bool) -> Result<Font> {
		let reader = UfoReader::new(path.as_ref(), validate)?;
		let default_name = reader.get_default_layer_name(None)?;
		let layer_order = reader.get_layer_names(None)?;
		let default_layer = load_layer(&reader, &default_name, validate)?;
		let other_layers = layer_order
			.iter()
			.filter(|n| **n != default_name)
			.map(|n| (n.clone(), None))
			.collect();
		Ok(Font {
			layer_order,
			default_layer,
			other_layers,
			data: reader.get_data_directory_listing()?,
			images: reader.get_image_directory_listing(None)?,
			info: reader.read_info(None)?,
			features: reader.read_features()?,
			groups: reader.read_groups(None)?,
			kerning: reader.read_kerning(None)?,
			lib: reader.read_lib(None)?,
			path: Some(path.as_ref().to_owned()),
			file_structure: Some(reader.file_structure()),
			reader: Some(reader),
			validate,
		})
	}

	/// `path`: where the font was opened from or last saved to.
	pub fn path(&self) -> Option<&Path> {
		self.path.as_deref()
	}

	/// The default layer.
	pub fn default_layer(&self) -> &Layer {
		&self.default_layer
	}

	/// The default layer, mutably.
	pub fn default_layer_mut(&mut self) -> &mut Layer {
		&mut self.default_layer
	}

	/// `layers.layerOrder`: the names of the layers.
	pub fn layer_order(&self) -> &[String] {
		&self.layer_order
	}

	/// A layer by name (`layers[name]`), read if needed; `None` if there is
	/// no such layer.
	pub fn layer_mut(&mut self, name: &str) -> Result<Option<&mut Layer>> {
		if name == self.default_layer.name {
			return Ok(Some(&mut self.default_layer));
		}
		let Some(slot) = self.other_layers.get_mut(name) else {
			return Ok(None);
		};
		if slot.is_none() {
			let reader = self.reader.as_ref().ok_or_else(|| {
				UfoLibError::UfoLib("the font has no reader to load layers from".into())
			})?;
			*slot = Some(load_layer(reader, name, self.validate)?);
		}
		Ok(slot.as_mut())
	}

	/// `keys()`: the glyph names of the default layer.
	pub fn keys(&self) -> impl Iterator<Item = &str> {
		self.default_layer.keys()
	}

	/// `len(font)`.
	pub fn len(&self) -> usize {
		self.default_layer.len()
	}

	/// Whether the default layer has no glyphs.
	pub fn is_empty(&self) -> bool {
		self.default_layer.is_empty()
	}

	/// `name in font`.
	pub fn contains(&self, name: &str) -> bool {
		self.default_layer.contains(name)
	}

	/// `font[name]`: a glyph of the default layer; `None` if there is no such
	/// glyph.
	pub fn get_mut(&mut self, name: &str) -> Result<Option<&mut Glyph>> {
		self.default_layer.get_mut(name)
	}

	/// `del font[name]`.
	pub fn remove(&mut self, name: &str) -> Result<()> {
		self.default_layer.remove(name)
	}

	/// `newGlyph`: a new glyph in the default layer.
	pub fn new_glyph(&mut self, name: &str) -> Result<&mut Glyph> {
		self.default_layer.new_glyph(name)
	}

	/// `info.unitsPerEm`.
	pub fn units_per_em(&self) -> Option<Number> {
		self.info.get("unitsPerEm").and_then(Number::from_value)
	}

	/// `font.save()`: saves the font in place, where it was opened from or
	/// last saved to.
	pub fn save(&mut self) -> Result<()> {
		let Some(path) = self.path.clone() else {
			return Err(UfoLibError::Value(
				"'path' is required when saving a new Font".into(),
			));
		};
		self.save_to(&path, false)
	}

	/// `font.save(path, overwrite=overwrite)`: saves the font elsewhere, as
	/// UFO 3 in the structure it was read with. Without `overwrite`, an
	/// existing `path` is an error.
	pub fn save_as(&mut self, path: impl AsRef<Path>, overwrite: bool) -> Result<()> {
		let path = normpath(path.as_ref());
		let mut tmp = None;
		let target = if path.exists() {
			if !overwrite {
				return Err(UfoLibError::Io(std::io::Error::new(
					std::io::ErrorKind::AlreadyExists,
					format!(
						"[Errno 17] path {} already exists",
						fontoxide_misc::py::str_repr(&path.to_string_lossy())
					),
				)));
			}
			let dir = temp_dir()?;
			let file_name = path.file_name().map(PathBuf::from).unwrap_or_default();
			let target = dir.join(file_name);
			tmp = Some(dir);
			target
		} else {
			path.clone()
		};
		let result = self.save_to(&target, true).and_then(|()| {
			if tmp.is_some() {
				if path.is_dir() {
					std::fs::remove_dir_all(&path)?;
				} else if path.is_file() {
					std::fs::remove_file(&path)?;
				}
				move_path(&target, &path)?;
			}
			Ok(())
		});
		if let Some(dir) = tmp {
			let _ = std::fs::remove_dir_all(dir);
		}
		result?;
		self.path = Some(path);
		Ok(())
	}

	/// `Font.write(writer, saveAs)` with a new `UFOWriter`.
	fn save_to(&mut self, path: &Path, save_as: bool) -> Result<()> {
		let mut writer = UfoWriter::new(
			path,
			UfoWriterOptions {
				structure: self.file_structure,
				validate: self.validate,
				..UfoWriterOptions::default()
			},
		)?;
		writer.write_features(&self.features, None)?;
		writer.write_groups(&self.groups, None)?;
		writer.write_info(&self.info, None)?;
		writer.write_kerning(&self.kerning, None)?;
		let mut guideline_ids = HashSet::new();
		if let Some(Value::Array(guidelines)) = self.info.get("guidelines") {
			for g in guidelines {
				if let Value::Dict(g) = g
					&& let Some(Value::String(id)) = g.get("identifier")
				{
					guideline_ids.insert(id.clone());
				}
			}
		}
		prune_object_libs(&mut self.lib, &guideline_ids);
		writer.write_lib(&self.lib, None)?;
		// the layers
		if !save_as {
			for existing in writer.get_layer_names(None)? {
				if !self.layer_order.contains(&existing) {
					writer.delete_glyph_set(Some(&existing))?;
				}
			}
		}
		for name in self.layer_order.clone() {
			let default = name == self.default_layer.name;
			let layer = if default {
				Some(&mut self.default_layer)
			} else if save_as {
				self.layer_mut(&name)?
			} else {
				self.other_layers.get_mut(&name).and_then(Option::as_mut)
			};
			let Some(layer) = layer else {
				continue;
			};
			let mut glyph_set =
				writer.get_glyph_set(Some(&name), default, WriterGlyphSetOptions::default())?;
			layer.write(&mut glyph_set, save_as)?;
		}
		let order: Vec<Option<&str>> = self.layer_order.iter().map(|n| Some(n.as_str())).collect();
		writer.write_layer_contents(Some(&order), None)?;
		// data and images: only copied when saving elsewhere, as they cannot
		// be changed.
		if save_as && let Some(reader) = &self.reader {
			for file_name in &self.data {
				writer.write_data(file_name, &reader.read_data(file_name)?)?;
			}
			for file_name in &self.images {
				writer.write_image(file_name, &reader.read_image(file_name, None)?, None)?;
			}
		}
		writer.close()?;
		writer.set_modification_time();
		Ok(())
	}
}

/// `LayerSet._loadLayer(reader, name, lazy=True)`.
fn load_layer(reader: &UfoReader, name: &str, validate: bool) -> Result<Layer> {
	let glyph_set = reader.get_glyph_set(Some(name), Some(validate), Some(true))?;
	Layer::read(name, glyph_set)
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

/// A new temporary directory (`tempfile.TemporaryDirectory`).
fn temp_dir() -> std::io::Result<PathBuf> {
	use std::sync::atomic::{AtomicUsize, Ordering};
	static COUNTER: AtomicUsize = AtomicUsize::new(0);
	let base = std::env::temp_dir();
	loop {
		let n = COUNTER.fetch_add(1, Ordering::Relaxed);
		let dir = base.join(format!("fontoxide-ufo-{}-{n}", std::process::id()));
		match std::fs::create_dir(&dir) {
			Ok(()) => return Ok(dir),
			Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
			Err(e) => return Err(e),
		}
	}
}

/// `shutil.move`: renames, or copies and removes across file systems.
fn move_path(from: &Path, to: &Path) -> std::io::Result<()> {
	if std::fs::rename(from, to).is_ok() {
		return Ok(());
	}
	copy_tree(from, to)?;
	if from.is_dir() {
		std::fs::remove_dir_all(from)
	} else {
		std::fs::remove_file(from)
	}
}

/// `shutil.copytree` (or `copy2` for a file).
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
	if from.is_dir() {
		std::fs::create_dir_all(to)?;
		for entry in std::fs::read_dir(from)? {
			let entry = entry?;
			copy_tree(&entry.path(), &to.join(entry.file_name()))?;
		}
		Ok(())
	} else {
		std::fs::copy(from, to).map(|_| ())
	}
}
