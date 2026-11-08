//! `COLR`: Color table (`fontTools.ttLib.tables.C_O_L_R_`).

use std::collections::BTreeMap;

use fontoxide_misc::Tag;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::fit;
use crate::generated::ot;
use crate::ot::table::{OtTable, XmlCtx};
use crate::ot::{OtReader, WriterGraph};
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

/// `_decompileColorLayersV0`.
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

fn no_color_layers() -> Error {
	Error::ttlib("AttributeError: 'table_C_O_L_R_' object has no attribute 'ColorLayers'")
}

impl Colr {
	/// `_toOTTable`, through `populateCOLRv0`.
	fn to_ot_table(&self) -> Result<ot::COLR> {
		let color_layers = self.color_layers.as_ref().ok_or_else(no_color_layers)?;
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
		let mut table = ot::COLR {
			version: self.version,
			..Default::default()
		};
		if !base_records.is_empty() {
			table.base_glyph_record_array = Some(Box::new(ot::BaseGlyphRecordArray {
				base_glyph_record: base_records,
			}));
		}
		if !layer_records.is_empty() {
			table.layer_record_array = Some(Box::new(ot::LayerRecordArray {
				layer_record: layer_records,
			}));
		}
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
