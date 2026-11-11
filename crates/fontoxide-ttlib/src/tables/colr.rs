//! `COLR`: Color table (`fontTools.ttLib.tables.C_O_L_R_`).

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::fit;
use crate::generated::ot;
use crate::ot::table::{OtTable, XmlCtx};
use crate::ot::{OtReader, WriterGraph};
use fontoxide_misc::array_tools::quantize_rect;
use fontoxide_misc::transform::{IDENTITY, Transform};
use fontoxide_pens::{BoundsPen, Pen, TransformPen};

use crate::glyph_set::GlyphSet;
use crate::ot::colr::ClipList;
use crate::ot::types::Paint;
use crate::sstruct::eval;
use crate::table::{Table, TableType};
use crate::{Error, Font, Result};

/// A layer of a version 0 color glyph (`LayerRecord`): a glyph drawn in a
/// palette color.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayerRecord {
	/// The layer glyph (`name`).
	pub glyph: u32,
	/// The palette entry index (`colorID`).
	pub color_id: u16,
}

/// Color table (`table_C_O_L_R_`).
///
/// Version 0 tables are read as [`Colr::color_layers`]; newer ones are kept as
/// the otData table in [`Colr::table`]. When both are set, as can happen from
/// TTX, the table wins. See
/// <https://learn.microsoft.com/en-us/typography/opentype/spec/colr>.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Colr {
	/// The table version.
	pub version: u16,
	/// The layers of each color glyph, for version 0 (`ColorLayers`).
	pub color_layers: Option<BTreeMap<u32, Vec<LayerRecord>>>,
	/// The otData table, for version 1 and later (`table`).
	pub table: Option<ot::COLR>,
}

fn color_layers_v0(table: &ot::COLR) -> Result<BTreeMap<u32, Vec<LayerRecord>>> {
	let Some(layer_array) = table.layer_record_array.as_deref() else {
		return Ok(BTreeMap::new());
	};
	let layers = &layer_array.layer_record;
	let base_array = table.base_glyph_record_array.as_deref().ok_or_else(|| {
		Error::ttlib("AttributeError: 'NoneType' object has no attribute 'BaseGlyphRecord'")
	})?;
	let mut color_layers = BTreeMap::new();
	for rec in &base_array.base_glyph_record {
		let first = rec.first_layer_index as usize;
		let end = first + rec.num_layers as usize;
		if end > layers.len() {
			return Err(Error::Assertion(
				"firstLayerIndex + numLayers <= numLayerRecords".into(),
			));
		}
		let list = layers[first..end]
			.iter()
			.map(|l| LayerRecord {
				glyph: l.layer_glyph,
				color_id: l.palette_index,
			})
			.collect();
		color_layers.insert(rec.base_glyph, list);
	}
	Ok(color_layers)
}

fn no_paint() -> Error {
	Error::ttlib("AttributeError: 'NoneType' object has no attribute 'Format'")
}

/// How deep [`compute_clip_box`] follows paints.
///
/// LENIENT(L38): fontTools does not end if `PaintColrGlyph`s or `PaintColrLayers`
/// form a cycle; fontoxide fails instead.
const MAX_PAINT_DEPTH: usize = 1000;

/// `Paint.getTransform`.
fn paint_transform(paint: &Paint) -> Result<Transform> {
	let around = |cx: i16, cy: i16, t: &dyn Fn(Transform) -> Transform| {
		let (cx, cy) = (f64::from(cx), f64::from(cy));
		t(IDENTITY.translate(cx, cy)).translate(-cx, -cy)
	};
	Ok(match paint {
		Paint::Format12(p) => {
			let t = p.transform.as_deref().ok_or_else(|| {
				Error::ttlib("AttributeError: 'NoneType' object has no attribute 'xx'")
			})?;
			Transform::new(t.xx, t.yx, t.xy, t.yy, t.dx, t.dy)
		}
		Paint::Format14(p) => IDENTITY.translate(f64::from(p.dx), f64::from(p.dy)),
		Paint::Format16(p) => IDENTITY.scale(p.scale_x, Some(p.scale_y)),
		Paint::Format18(p) => around(p.center_x, p.center_y, &|t| {
			t.scale(p.scale_x, Some(p.scale_y))
		}),
		Paint::Format20(p) => IDENTITY.scale(p.scale, None),
		Paint::Format22(p) => around(p.center_x, p.center_y, &|t| t.scale(p.scale, None)),
		Paint::Format24(p) => IDENTITY.rotate(p.angle.to_radians()),
		Paint::Format26(p) => around(p.center_x, p.center_y, &|t| t.rotate(p.angle.to_radians())),
		Paint::Format28(p) => {
			IDENTITY.skew((-p.x_skew_angle).to_radians(), p.y_skew_angle.to_radians())
		}
		Paint::Format30(p) => around(p.center_x, p.center_y, &|t| {
			t.skew((-p.x_skew_angle).to_radians(), p.y_skew_angle.to_radians())
		}),
		Paint::Format3(_)
		| Paint::Format5(_)
		| Paint::Format7(_)
		| Paint::Format9(_)
		| Paint::Format13(_)
		| Paint::Format15(_)
		| Paint::Format17(_)
		| Paint::Format19(_)
		| Paint::Format21(_)
		| Paint::Format23(_)
		| Paint::Format25(_)
		| Paint::Format27(_)
		| Paint::Format29(_)
		| Paint::Format31(_) => {
			return Err(Error::ttlib(format!(
				"NotImplementedError: Variable Paints not supported: {}",
				paint.format().unwrap_or_default()
			)));
		}
		_ => IDENTITY,
	})
}

/// `Paint.iterPaintSubTables`: the child paints, the layers of a
/// `PaintColrLayers` and the paint of the base glyph of a `PaintColrGlyph`.
fn paint_sub_tables<'a>(colr: &'a ot::COLR, paint: &'a Paint) -> Result<Vec<&'a Paint>> {
	let children: Vec<&Option<Box<Paint>>> = match paint {
		Paint::Format1(p) => {
			let layers = colr.layer_list.as_deref().map_or(&[][..], |l| &l.paint[..]);
			let start = (p.first_layer_index as usize).min(layers.len());
			let end = (start + p.num_layers as usize).min(layers.len());
			layers[start..end].iter().collect()
		}
		Paint::Format11(p) => {
			let records = colr
				.base_glyph_list
				.as_deref()
				.map_or(&[][..], |l| &l.base_glyph_paint_record[..]);
			let record = records
				.iter()
				.find(|r| r.base_glyph == p.glyph)
				.ok_or_else(|| Error::ttlib("KeyError: glyph not in colr.BaseGlyphList"))?;
			vec![&record.paint]
		}
		Paint::Format10(p) => vec![&p.paint],
		Paint::Format12(p) => vec![&p.paint],
		Paint::Format13(p) => vec![&p.paint],
		Paint::Format14(p) => vec![&p.paint],
		Paint::Format15(p) => vec![&p.paint],
		Paint::Format16(p) => vec![&p.paint],
		Paint::Format17(p) => vec![&p.paint],
		Paint::Format18(p) => vec![&p.paint],
		Paint::Format19(p) => vec![&p.paint],
		Paint::Format20(p) => vec![&p.paint],
		Paint::Format21(p) => vec![&p.paint],
		Paint::Format22(p) => vec![&p.paint],
		Paint::Format23(p) => vec![&p.paint],
		Paint::Format24(p) => vec![&p.paint],
		Paint::Format25(p) => vec![&p.paint],
		Paint::Format26(p) => vec![&p.paint],
		Paint::Format27(p) => vec![&p.paint],
		Paint::Format28(p) => vec![&p.paint],
		Paint::Format29(p) => vec![&p.paint],
		Paint::Format30(p) => vec![&p.paint],
		Paint::Format31(p) => vec![&p.paint],
		Paint::Format32(p) => vec![&p.source_paint, &p.backdrop_paint],
		Paint::Format33(p) => vec![&p.paint],
		_ => Vec::new(),
	};
	children
		.into_iter()
		.map(|c| c.as_deref().ok_or_else(no_paint))
		.collect()
}

/// `Paint.computeClipBox`: the control bounds of the glyphs that a paint
/// draws, transformed as the paints above them say, quantized.
fn compute_clip_box(
	colr: &ot::COLR,
	paint: &Paint,
	glyph_set: &GlyphSet<'_>,
	quantization: i64,
) -> Result<Option<ot::ClipBox>> {
	/// The paints from the root to `paint`, with their transforms: fontTools only
	/// computes these (`getTransform`) when it reaches a glyph paint, so the
	/// errors are kept until then.
	fn walk<'a>(
		colr: &'a ot::COLR,
		paint: &'a Paint,
		path: &mut Vec<&'a Paint>,
		glyph_set: &GlyphSet<'_>,
		pen: &mut dyn Pen,
	) -> Result<()> {
		if path.len() > MAX_PAINT_DEPTH {
			return Err(Error::ttlib("RecursionError: COLR paint graph is too deep"));
		}
		path.push(paint);
		let children = paint_sub_tables(colr, paint)?;
		if let Paint::Format10(ot::PaintFormat10 { glyph, .. })
		| Paint::Format33(ot::PaintFormat33 { glyph, .. }) = paint
		{
			let mut transformation = IDENTITY;
			for p in path.iter() {
				transformation = transformation.transform(&paint_transform(p)?);
			}
			let mut transform_pen = TransformPen::new(&mut *pen, transformation);
			glyph_set.draw(&glyph_set.glyph_order().name(*glyph), &mut transform_pen)?;
		}
		for child in children {
			walk(colr, child, path, glyph_set, pen)?;
		}
		path.pop();
		Ok(())
	}

	let mut pen = BoundsPen::new_control_bounds(Some(glyph_set), false);
	walk(colr, paint, &mut Vec::new(), glyph_set, &mut pen)?;
	let Some(bounds) = pen.bounds() else {
		return Ok(None);
	};
	let [x_min, y_min, x_max, y_max] = quantize_rect(bounds, quantization as f64)
		.ok_or_else(|| Error::value("factor must be >= 1"))?;
	Ok(Some(ot::ClipBox::Format1(ot::ClipBoxFormat1 {
		x_min: fit(x_min, "xMin")?,
		y_min: fit(y_min, "yMin")?,
		x_max: fit(x_max, "xMax")?,
		y_max: fit(y_max, "yMax")?,
	})))
}

fn no_color_layers() -> Error {
	Error::ttlib("AttributeError: 'table_C_O_L_R_' object has no attribute 'ColorLayers'")
}

impl Colr {
	/// `_decompileColorLayersV0`.
	pub fn decompile_color_layers_v0(table: &ot::COLR) -> Result<BTreeMap<u32, Vec<LayerRecord>>> {
		color_layers_v0(table)
	}

	/// `populateCOLRv0`, with the base glyphs in glyph order.
	pub fn populate_color_layers_v0(
		table: &mut ot::COLR,
		color_layers: &BTreeMap<u32, Vec<LayerRecord>>,
	) -> Result<()> {
		let mut base_records = Vec::new();
		let mut layer_records = Vec::new();
		for (&base_glyph, layers) in color_layers {
			base_records.push(ot::BaseGlyphRecord {
				base_glyph,
				first_layer_index: fit(layer_records.len(), "FirstLayerIndex")?,
				num_layers: fit(layers.len(), "NumLayers")?,
			});
			for l in layers {
				layer_records.push(ot::LayerRecord {
					layer_glyph: l.glyph,
					palette_index: l.color_id,
				});
			}
		}
		table.base_glyph_record_count = Some(fit(base_records.len(), "BaseGlyphRecordCount")?);
		table.layer_record_count = Some(fit(layer_records.len(), "LayerRecordCount")?);
		table.base_glyph_record_array = (!base_records.is_empty()).then(|| {
			Box::new(ot::BaseGlyphRecordArray {
				base_glyph_record: base_records,
			})
		});
		table.layer_record_array = (!layer_records.is_empty()).then(|| {
			Box::new(ot::LayerRecordArray {
				layer_record: layer_records,
			})
		});
		Ok(())
	}

	/// Sets the clip box of each base glyph of a version 1 table to the
	/// control bounds of its paint, quantized (`COLR.computeClipBoxes`). A glyph
	/// set from [`GlyphSet::new`] draws the glyphs; version 0 tables are left
	/// alone.
	pub fn compute_clip_boxes(
		&mut self,
		glyph_set: &GlyphSet<'_>,
		quantization: i64,
	) -> Result<()> {
		let Some(table) = &mut self.table else {
			return Ok(());
		};
		if table.version == 0 {
			return Ok(());
		}
		let records = &table
			.base_glyph_list
			.as_deref()
			.ok_or_else(|| {
				Error::ttlib(
					"AttributeError: 'NoneType' object has no attribute 'BaseGlyphPaintRecord'",
				)
			})?
			.base_glyph_paint_record;
		let mut clips = BTreeMap::new();
		for rec in records {
			let clip_box = rec
				.paint
				.as_deref()
				.ok_or_else(no_paint)
				.and_then(|paint| compute_clip_box(table, paint, glyph_set, quantization))
				.map_err(|e| {
					Error::ttlib(format!(
						"Failed to compute COLR ClipBox for '{}': {e}",
						glyph_set.glyph_order().name(rec.base_glyph)
					))
				})?;
			if let Some(clip_box) = clip_box {
				clips.insert(rec.base_glyph, clip_box);
			}
		}
		if clips.is_empty() {
			table.clip_list = None;
		} else {
			let clip_list = table.clip_list.get_or_insert_with(|| {
				Box::new(ClipList {
					format: Some(1),
					..Default::default()
				})
			});
			clip_list.clips = clips;
		}
		Ok(())
	}

	/// `_toOTTable`.
	fn to_ot_table(&self) -> Result<ot::COLR> {
		let color_layers = self.color_layers.as_ref().ok_or_else(no_color_layers)?;
		let mut table = ot::COLR {
			version: self.version,
			..Default::default()
		};
		Self::populate_color_layers_v0(&mut table, color_layers)?;
		Ok(table)
	}
}

impl TableType for Colr {
	const TAG: Tag = Tag::new(b"COLR");

	fn decompile(data: &[u8], font: &mut Font) -> Result<Self> {
		let mut reader = OtReader::new(data, Self::TAG);
		let table = ot::COLR::decompile(&mut reader, font)?;
		Ok(if table.version == 0 {
			Colr {
				version: 0,
				color_layers: Some(color_layers_v0(&table)?),
				table: None,
			}
		} else {
			Colr {
				version: table.version,
				color_layers: None,
				table: Some(table),
			}
		})
	}
}

impl Table for Colr {
	fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		if let Some(layers) = &mut self.color_layers {
			let mut out = BTreeMap::new();
			for (g, mut records) in std::mem::take(layers) {
				for r in &mut records {
					r.glyph = f(r.glyph)?;
				}
				out.insert(f(g)?, records);
			}
			*layers = out;
		}
		if let Some(table) = &mut self.table {
			table.remap_glyphs(f)?;
		}
		Ok(())
	}

	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, font: &mut Font) -> Result<Vec<u8>> {
		let built;
		let table = match &self.table {
			Some(t) => t,
			None => {
				built = self.to_ot_table()?;
				&built
			}
		};
		let mut graph = WriterGraph::new(Self::TAG);
		table.compile(&mut graph.root(), font)?;
		graph.get_all_data()
	}

	fn to_xml(&self, writer: &mut XmlWriter, font: &mut Font) -> Result<()> {
		if let Some(table) = &self.table {
			return table.to_xml2(writer, font);
		}
		writer.simpletag("version", &[("value", &self.version)]);
		writer.newline();
		let color_layers = self.color_layers.as_ref().ok_or_else(no_color_layers)?;
		let order = font.glyph_order()?.clone();
		for (&base_glyph, layers) in color_layers {
			writer.begintag("ColorGlyph", &[("name", &order.name(base_glyph))]);
			writer.newline();
			for layer in layers {
				writer.simpletag(
					"layer",
					&[
						("name", &order.name(layer.glyph)),
						("colorID", &layer.color_id),
					],
				);
				writer.newline();
			}
			writer.endtag("ColorGlyph");
			writer.newline();
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, font: &mut Font) -> Result<()> {
		let attr = |e: &Element, name: &str| -> Result<String> {
			e.attr(name)
				.map(str::to_owned)
				.ok_or_else(|| Error::ttx(format!("KeyError: {name:?}")))
		};
		// LENIENT(L7): fontTools keeps any value and fails when writing one that
		// does not fit; values are range-checked here.
		let int = |s: &str, name: &str| -> Result<u16> {
			let v = eval(s)?
				.as_int()
				.ok_or_else(|| Error::value(format!("invalid {name}: {s:?}")))?;
			fit(v, name)
		};
		match element.name.as_str() {
			"version" => self.version = int(&attr(element, "value")?, "version")?,
			"ColorGlyph" => {
				let base_glyph = font.glyph_id(&attr(element, "name")?)?;
				let mut layers = Vec::new();
				for e in element.elements() {
					layers.push(LayerRecord {
						glyph: font.glyph_id(&attr(e, "name")?)?,
						color_id: int(&attr(e, "colorID")?, "colorID")?,
					});
				}
				self.color_layers
					.get_or_insert_with(BTreeMap::new)
					.insert(base_glyph, layers);
			}
			_ => {
				let table = self.table.get_or_insert_with(ot::COLR::default);
				let mut ctx = XmlCtx::default();
				table.from_xml(element, font, &mut ctx)?;
				table.populate_defaults(&mut ctx)?;
				self.version = table.version;
			}
		}
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}
