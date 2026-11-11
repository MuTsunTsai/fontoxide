//! Change the units-per-EM of a font (`fontTools.ttLib.scaleUpem`).
//!
//! AAT and Graphite tables are not supported. CFF/CFF2 fonts are de-subroutinized.
//!
//! fontTools does this with a visitor (`ScalerVisitor`) that walks every attribute of
//! every table and scales the attributes registered for each class. Here the same
//! fields are scaled table by table; the otData tables are walked with
//! [`OtTable::visit_mut`].
//!
//! LENIENT(L39): a scaled value that does not fit its field fails here, when scaling;
//! fontTools fails later, when compiling the table.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;

use fontoxide_misc::argparse::{Argument, Command};
use fontoxide_misc::cli_tools::make_output_file_name;

use fontoxide_misc::round_tools::ot_round;

use crate::cff::charstring::Token;
use crate::cff::dict::Value;
use crate::cff::font::{CffFontSet, PrivateRef, num_regions};
use crate::cff::specializer::{Arg, NumRegions, commands_to_program, program_to_commands};
use crate::cff::transforms::private_of_mut;
use crate::generated::ot;
use crate::ot::conv::ValueRecord;
use crate::ot::table::OtTable;
use crate::ot::var_store::calculate_num_shorts;
use crate::ot::varc::{NO_VARIATION_INDEX, flags};
use crate::table::Table;
use crate::tables::glyf::{ComponentPlacement, Number};
use crate::tables::kern::KernSubtable;
use crate::tables::otl::{Base, Gdef, Gpos, Gsub, Hvar, Jstf, Math, Mvar, Stat, Varc, Vvar};
use crate::tables::tuple_variation::Delta;
use crate::tables::{
	Avar, Cff, Cff2, Colr, Fvar, Glyf, Gvar, Head, Hhea, Hmtx, Kern, Os2, Post, Vhea, Vmtx, Vorg,
};
use crate::{Error, Font, ReadOptions, Result, WriteOptions};

/// The scaling of `ScalerVisitor`.
#[derive(Debug, Clone, Copy)]
struct Scaler {
	/// `scaleFactor`.
	factor: f64,
}

impl Scaler {
	/// `scale`: `otRound(v * scaleFactor)`.
	fn round(self, v: f64) -> i64 {
		ot_round(v * self.factor)
	}

	/// [`Scaler::round`] into the type of the field.
	fn scale<T: TryFrom<i64> + Into<f64>>(self, v: T) -> Result<T> {
		let scaled = self.round(v.into());
		T::try_from(scaled)
			.map_err(|_| Error::Struct(format!("scaled value {scaled} out of range")))
	}

	fn field<T: TryFrom<i64> + Into<f64> + Copy>(self, v: &mut T) -> Result<()> {
		*v = self.scale(*v)?;
		Ok(())
	}
}

/// Changes the units-per-EM of `font` to `new_upem` (`scale_upem`).
///
/// Every table is decompiled, and those that hold distances in font units are
/// scaled: `head`, `hhea`, `vhea`, `OS/2`, `post`, `hmtx`, `vmtx`, `VORG`, `kern`,
/// `glyf`, `gvar`, `CFF ` and `CFF2` (which are de-subroutinized), `VARC`, `COLR`,
/// the item variation stores, and the otData records with coordinates (GPOS value
/// records and anchors, GDEF and BASE coordinates, MATH values, COLR clip boxes).
pub fn scale_upem(font: &mut Font, new_upem: u16) -> Result<()> {
	let upem = font.require::<Head>()?.units_per_em;
	if upem == 0 {
		return Err(Error::value("ZeroDivisionError: division by zero"));
	}
	let scaler = Scaler {
		factor: f64::from(new_upem) / f64::from(upem),
	};
	font.ensure_decompiled()?;
	let fvar_axes: Vec<String> = match font.get::<Fvar>()? {
		Some(fvar) => fvar
			.axes
			.iter()
			.map(|a| String::from_utf8_lossy(&a.axis_tag).into_owned())
			.collect(),
		None => Vec::new(),
	};
	for tag in font.tags() {
		if let Some(table) = font.get_dyn(tag)? {
			scale_table(table, scaler, &fvar_axes)?;
		}
	}
	Ok(())
}

/// Scales a `Paint` as `ScalerVisitor(scale_factor).visit(paint)` does: each
/// `PaintGlyph` and `PaintGlyph2` below it (or itself) is wrapped in a scale by
/// `1 / scale_factor`, which undoes the scaling of the glyph outline in a paint
/// tree that is scaled as a whole.
pub fn scale_paint(paint: &mut ot::Paint, scale_factor: f64) {
	visit_paint(
		paint,
		Scaler {
			factor: scale_factor,
		},
	);
}

fn scale_table(table: &mut dyn Table, s: Scaler, fvar_axes: &[String]) -> Result<()> {
	let any = table.as_any_mut();
	macro_rules! try_ot {
		($($ty:ty),*) => {
			$(if let Some(t) = any.downcast_mut::<$ty>() {
				return visit_ot(&mut t.table, s);
			})*
		};
	}
	try_ot!(Gdef, Gpos, Gsub, Base, Jstf, Math, Hvar, Vvar, Mvar, Stat);
	if let Some(t) = any.downcast_mut::<Head>() {
		s.field(&mut t.units_per_em)?;
		s.field(&mut t.x_min)?;
		s.field(&mut t.y_min)?;
		s.field(&mut t.x_max)?;
		s.field(&mut t.y_max)?;
	} else if let Some(t) = any.downcast_mut::<Post>() {
		s.field(&mut t.header.underline_position)?;
		s.field(&mut t.header.underline_thickness)?;
	} else if let Some(t) = any.downcast_mut::<Vorg>() {
		s.field(&mut t.default_vert_origin_y)?;
		for v in t.v_origin_records.values_mut() {
			s.field(v)?;
		}
	} else if let Some(t) = any.downcast_mut::<Hhea>() {
		s.field(&mut t.ascent)?;
		s.field(&mut t.descent)?;
		s.field(&mut t.line_gap)?;
		s.field(&mut t.advance_width_max)?;
		s.field(&mut t.min_left_side_bearing)?;
		s.field(&mut t.min_right_side_bearing)?;
		s.field(&mut t.x_max_extent)?;
		s.field(&mut t.caret_offset)?;
	} else if let Some(t) = any.downcast_mut::<Vhea>() {
		s.field(&mut t.ascent)?;
		s.field(&mut t.descent)?;
		s.field(&mut t.line_gap)?;
		s.field(&mut t.advance_height_max)?;
		s.field(&mut t.min_top_side_bearing)?;
		s.field(&mut t.min_bottom_side_bearing)?;
		s.field(&mut t.y_max_extent)?;
		s.field(&mut t.caret_offset)?;
	} else if let Some(t) = any.downcast_mut::<Os2>() {
		let b = &mut t.base;
		for v in [
			&mut b.x_avg_char_width,
			&mut b.y_subscript_x_size,
			&mut b.y_subscript_y_size,
			&mut b.y_subscript_x_offset,
			&mut b.y_subscript_y_offset,
			&mut b.y_superscript_x_size,
			&mut b.y_superscript_y_size,
			&mut b.y_superscript_x_offset,
			&mut b.y_superscript_y_offset,
			&mut b.y_strikeout_size,
			&mut b.y_strikeout_position,
			&mut b.s_typo_ascender,
			&mut b.s_typo_descender,
			&mut b.s_typo_line_gap,
		] {
			s.field(v)?;
		}
		s.field(&mut b.us_win_ascent)?;
		s.field(&mut b.us_win_descent)?;
		// Zeros (and never written) in versions that do not have them, as the
		// attributes are missing in fontTools.
		s.field(&mut t.sx_height)?;
		s.field(&mut t.s_cap_height)?;
	} else if let Some(t) = any.downcast_mut::<Hmtx>() {
		scale_metrics(&mut t.metrics, s)?;
	} else if let Some(t) = any.downcast_mut::<Vmtx>() {
		scale_metrics(&mut t.metrics, s)?;
	} else if let Some(t) = any.downcast_mut::<Glyf>() {
		scale_glyf(t, s);
	} else if let Some(t) = any.downcast_mut::<Gvar>() {
		for (_, variations) in &mut t.variations {
			for var in variations {
				for delta in var.coordinates.iter_mut().flatten() {
					if let Delta::Point(x, y) = delta {
						*x = Number::Int(s.round(x.value()));
						*y = Number::Int(s.round(y.value()));
					}
				}
			}
		}
	} else if let Some(t) = any.downcast_mut::<Kern>() {
		for table in t.kern_tables.iter_mut().flatten() {
			// UPSTREAM-BUG(B45): fontTools fails with an AttributeError on a subtable of
			// an unknown format, which has no `kernTable`; it is left alone here.
			if let KernSubtable::Format0(table) = table {
				for v in table.kern_table.values_mut() {
					s.field(v)?;
				}
			}
		}
	} else if let Some(t) = any.downcast_mut::<Cff>() {
		scale_cff(&mut t.cff, s)?;
	} else if let Some(t) = any.downcast_mut::<Cff2>() {
		scale_cff(&mut t.cff, s)?;
	} else if let Some(t) = any.downcast_mut::<Varc>() {
		scale_varc(&mut t.table, s, fvar_axes)?;
	} else if let Some(t) = any.downcast_mut::<Colr>() {
		if let Some(table) = &mut t.table {
			scale_colr(table, s)?;
		}
	} else if let Some(t) = any.downcast_mut::<Avar>() {
		visit_ot(&mut t.table, s)?;
	}
	Ok(())
}

fn scale_metrics(metrics: &mut BTreeMap<u32, (u16, i16)>, s: Scaler) -> Result<()> {
	for (advance, side_bearing) in metrics.values_mut() {
		s.field(advance)?;
		s.field(side_bearing)?;
	}
	Ok(())
}

fn scale_glyf(glyf: &mut Glyf, s: Scaler) {
	for g in glyf.glyphs.values_mut() {
		// The glyphs are expanded (`ensureDecompiled`).
		g.data = None;
		// fontTools only scales the bounds of glyphs that have them: empty glyphs
		// have none, and keep zeros here.
		for v in [&mut g.x_min, &mut g.y_min, &mut g.x_max, &mut g.y_max] {
			*v = s.round(*v as f64);
		}
		if g.is_composite() {
			for c in &mut g.components {
				// UPSTREAM-BUG(B45): fontTools fails with an AttributeError on a
				// component placed by matching points, which has no `x`; there is
				// nothing to scale.
				if let ComponentPlacement::Offset { x, y } = &mut c.placement {
					*x = Number::Int(s.round(x.value()));
					*y = Number::Int(s.round(y.value()));
				}
			}
			continue;
		}
		for (x, y) in &mut g.coordinates {
			*x = s.round(*x) as f64;
			*y = s.round(*y) as f64;
		}
	}
}

/// Scales a `ValueRecord`.
fn scale_value_record(v: &mut Option<ValueRecord>, s: Scaler) -> Result<()> {
	if let Some(v) = v {
		for f in [
			&mut v.x_advance,
			&mut v.y_advance,
			&mut v.x_placement,
			&mut v.y_placement,
		]
		.into_iter()
		.flatten()
		{
			s.field(f)?;
		}
	}
	Ok(())
}

/// The registered visitors of otData classes.
fn visit_ot<T: OtTable>(table: &mut T, s: Scaler) -> Result<()> {
	table.visit_mut(&mut |any| {
		macro_rules! values {
			($($ty:ty => [$($field:ident),*];)*) => {
				$(if let Some(t) = any.downcast_mut::<$ty>() {
					$(scale_values(&mut t.$field, s)?;)*
					return Ok(());
				})*
			};
		}
		values! {
			ot::SinglePosFormat1 => [value];
			ot::SinglePosFormat2 => [value];
			ot::SinglePosFormat3 => [value];
			ot::SinglePosFormat4 => [value];
			ot::PairValueRecord => [value1, value2];
			ot::PairValue2 => [value1, value2];
			ot::Class2Record => [value1, value2];
		}
		if let Some(t) = any.downcast_mut::<ot::Anchor>() {
			match t {
				ot::Anchor::Format1(a) => {
					s.field(&mut a.x_coordinate)?;
					s.field(&mut a.y_coordinate)?;
				}
				ot::Anchor::Format2(a) => {
					s.field(&mut a.x_coordinate)?;
					s.field(&mut a.y_coordinate)?;
				}
				ot::Anchor::Format3(a) => {
					s.field(&mut a.x_coordinate)?;
					s.field(&mut a.y_coordinate)?;
				}
				ot::Anchor::Unknown(_) => {}
			}
		} else if let Some(t) = any.downcast_mut::<ot::CaretValue>() {
			match t {
				ot::CaretValue::Format1(c) => s.field(&mut c.coordinate)?,
				ot::CaretValue::Format3(c) => s.field(&mut c.coordinate)?,
				_ => {}
			}
		} else if let Some(t) = any.downcast_mut::<ot::BaseCoord>() {
			match t {
				ot::BaseCoord::Format1(c) => s.field(&mut c.coordinate)?,
				ot::BaseCoord::Format2(c) => s.field(&mut c.coordinate)?,
				ot::BaseCoord::Format3(c) => s.field(&mut c.coordinate)?,
				ot::BaseCoord::Format4(c) => s.field(&mut c.coordinate)?,
				ot::BaseCoord::Unknown(_) => {}
			}
		} else if let Some(t) = any.downcast_mut::<ot::MathValueRecord>() {
			s.field(&mut t.value)?;
		} else if let Some(t) = any.downcast_mut::<ot::MathConstants>() {
			s.field(&mut t.delimited_sub_formula_min_height)?;
			s.field(&mut t.display_operator_min_height)?;
		} else if let Some(t) = any.downcast_mut::<ot::MathVariants>() {
			s.field(&mut t.min_connector_overlap)?;
		} else if let Some(t) = any.downcast_mut::<ot::MathGlyphVariantRecord>() {
			s.field(&mut t.advance_measurement)?;
		} else if let Some(t) = any.downcast_mut::<ot::GlyphPartRecord>() {
			s.field(&mut t.start_connector_length)?;
			s.field(&mut t.end_connector_length)?;
			s.field(&mut t.full_advance)?;
		} else if let Some(t) = any.downcast_mut::<ot::VarData>() {
			scale_var_data(t, s);
		}
		Ok(())
	})
}

/// The value records of a field: one, or a list.
trait ValueRecords {
	fn records(&mut self) -> Vec<&mut Option<ValueRecord>>;
}

impl ValueRecords for Option<ValueRecord> {
	fn records(&mut self) -> Vec<&mut Option<ValueRecord>> {
		vec![self]
	}
}

impl ValueRecords for Vec<Option<ValueRecord>> {
	fn records(&mut self) -> Vec<&mut Option<ValueRecord>> {
		self.iter_mut().collect()
	}
}

fn scale_values(v: &mut impl ValueRecords, s: Scaler) -> Result<()> {
	for r in v.records() {
		scale_value_record(r, s)?;
	}
	Ok(())
}

/// The visitor of `VarData`.
fn scale_var_data(data: &mut ot::VarData, s: Scaler) {
	for item in &mut data.item {
		for v in item {
			*v = s.round(*v as f64);
		}
	}
	calculate_num_shorts(data, false);
}

// CFF

/// `_cff_scale` of charstring arguments.
fn scale_args(args: &mut [Arg], s: Scaler) -> Result<()> {
	for arg in args.iter_mut() {
		match arg {
			Arg::Int(v) => *arg = Arg::Int(s.round(*v as f64)),
			Arg::Float(v) => *arg = Arg::Int(s.round(*v)),
			Arg::Blend(list) => {
				let num_blends = list.last().cloned();
				scale_args(list, s)?;
				if let (Some(last), Some(n)) = (list.last_mut(), num_blends) {
					*last = n;
				}
			}
			Arg::Mask(_) => {}
			Arg::Op(op) => {
				return Err(Error::ttlib(format!(
					"TypeError: can't multiply sequence by non-int of type 'float' ({op})"
				)));
			}
		}
	}
	Ok(())
}

/// `_cff_scale` of DICT values.
fn scale_dict_values(values: &mut [Value], s: Scaler) -> Result<()> {
	for v in values.iter_mut() {
		match v {
			Value::List(list) => {
				let num_blends = list.last().cloned();
				scale_dict_values(list, s)?;
				if let (Some(last), Some(n)) = (list.last_mut(), num_blends) {
					*last = n;
				}
			}
			_ => *v = scale_dict_value(v, s)?,
		}
	}
	Ok(())
}

fn scale_dict_value(v: &Value, s: Scaler) -> Result<Value> {
	match v.number() {
		Some(n) => Ok(Value::Int(s.round(n))),
		None => Err(Error::ttlib(format!(
			"TypeError: can't multiply {v:?} by a float"
		))),
	}
}

/// `getattr(dict, attr, None)`, then the scaled value set back.
fn scale_dict_entry(value: Option<Value>, s: Scaler) -> Result<Option<Value>> {
	Ok(match value {
		None => None,
		Some(Value::List(mut list)) => {
			scale_dict_values(&mut list, s)?;
			Some(Value::List(list))
		}
		Some(v) => Some(scale_dict_value(&v, s)?),
	})
}

const TOP_DICT_SCALED: [&str; 4] = [
	"UnderlinePosition",
	"UnderlineThickness",
	"FontBBox",
	"StrokeWidth",
];

const PRIVATE_DICT_SCALED: [&str; 10] = [
	"BlueValues",
	"OtherBlues",
	"FamilyBlues",
	"FamilyOtherBlues",
	"StdHW",
	"StdVW",
	"StemSnapH",
	"StemSnapV",
	"defaultWidthX",
	"nominalWidthX",
];

fn scale_cff(cff: &mut CffFontSet, s: Scaler) -> Result<()> {
	cff.desubroutinize()?;
	for top in &mut cff.top_dicts {
		// The CFF2 variation store is visited too.
		if let Some(store) = &mut top.var_store {
			visit_ot(store.as_mut(), s)?;
		}
		let store = top.var_store.clone();
		let get_num_regions = |vi: i64| num_regions(store.as_deref(), vi);
		let mut privates: BTreeSet<PrivateRef> = BTreeSet::new();
		let count = top.char_strings.as_ref().map_or(0, |c| c.items.len());
		for gid in 0..count {
			let private_ref = top
				.char_strings
				.as_ref()
				.and_then(|c| c.privates.get(gid).copied())
				.unwrap_or(PrivateRef::None);
			privates.insert(private_ref);
			let vs_index = private_of_mut(top, private_ref)
				.and_then(|p| p.get("vsindex"))
				.and_then(|v| v.int().ok())
				.unwrap_or(0);
			let Some(cs) = top.char_strings.as_mut().and_then(|c| c.items.get_mut(gid)) else {
				continue;
			};
			let regions: Option<NumRegions<'_>> = match store {
				Some(_) => Some(&get_num_regions),
				None => None,
			};
			let mut commands = program_to_commands(&cs.program, regions, vs_index)?;
			for command in &mut commands {
				if command.op == "vsindex" {
					continue;
				}
				scale_args(&mut command.args, s)?;
			}
			let program: Vec<Token> = commands_to_program(&commands)?;
			cs.bytecode = None;
			cs.program = program;
		}

		// Annoying business of scaling numbers that do not matter whatsoever
		for name in TOP_DICT_SCALED {
			if let Some(v) = scale_dict_entry(top.get(name), s)? {
				top.values.insert(name, v);
			}
		}
		// A new list, written explicitly even where the Top DICT had none.
		if let Some(Value::List(matrix)) = top.get("FontMatrix") {
			let matrix = matrix
				.iter()
				.map(|v| Value::Float(v.number().unwrap_or(0.0) / s.factor))
				.collect();
			top.values.insert("FontMatrix", Value::List(matrix));
		}

		for r in &privates {
			let Some(private) = private_of_mut(top, *r) else {
				continue;
			};
			for name in PRIVATE_DICT_SCALED {
				if let Some(v) = scale_dict_entry(private.get(name), s)? {
					private.values.insert(name, v);
				}
			}
		}
	}
	Ok(())
}

// COLR

fn scale_colr(table: &mut ot::COLR, s: Scaler) -> Result<()> {
	if let Some(list) = &mut table.base_glyph_list {
		for record in &mut list.base_glyph_paint_record {
			// The paint of a base glyph is scaled as a whole.
			let old = record.paint.take();
			let mut paint = setup_scale_paint(s.factor, old);
			visit_paint(&mut paint, s);
			record.paint = Some(Box::new(paint));
		}
	}
	if let Some(list) = &mut table.layer_list {
		for paint in list.paint.iter_mut().flatten() {
			visit_paint(paint, s);
		}
	}
	if let Some(clips) = &mut table.clip_list {
		for clip in clips.clips.values_mut() {
			match clip {
				ot::ClipBox::Format1(b) => {
					for v in [&mut b.x_min, &mut b.y_min, &mut b.x_max, &mut b.y_max] {
						s.field(v)?;
					}
				}
				ot::ClipBox::Format2(b) => {
					for v in [&mut b.x_min, &mut b.y_min, &mut b.x_max, &mut b.y_max] {
						s.field(v)?;
					}
				}
				ot::ClipBox::Unknown(_) => {}
			}
		}
	}
	visit_ot(table, s)
}

/// `_setup_scale_paint`: a paint that scales `child` by `scale`.
fn setup_scale_paint(scale: f64, child: Option<Box<ot::Paint>>) -> ot::Paint {
	// UPSTREAM-BUG(B44): `2 - (1 >> 14)` is 2, where `2 - 1 / (1 << 14)`, the largest
	// F2Dot14, was meant: a scale of 2 makes a PaintScaleUniform that cannot be
	// compiled.
	if (-2.0..=2.0).contains(&scale) {
		return ot::Paint::Format20(ot::PaintFormat20 {
			paint: child,
			scale,
		});
	}
	ot::Paint::Format12(ot::PaintFormat12 {
		paint: child,
		transform: Some(Box::new(ot::Affine2x3 {
			xx: scale,
			yy: scale,
			..Default::default()
		})),
	})
}

/// The visitor of `Paint`.
fn visit_paint(paint: &mut ot::Paint, s: Scaler) {
	let glyph = match paint {
		ot::Paint::Format10(p) => Some(ot::Paint::Format10(ot::PaintFormat10 {
			paint: p.paint.take(),
			glyph: p.glyph,
		})),
		ot::Paint::Format33(p) => Some(ot::Paint::Format33(ot::PaintFormat33 {
			paint: p.paint.take(),
			glyph: p.glyph,
		})),
		_ => None,
	};
	match glyph {
		Some(mut glyph) => {
			// The child of the glyph paint is visited; the glyph paint is not.
			if let ot::Paint::Format10(ot::PaintFormat10 {
				paint: Some(child), ..
			})
			| ot::Paint::Format33(ot::PaintFormat33 {
				paint: Some(child), ..
			}) = &mut glyph
			{
				visit_paint(child, s);
			}
			*paint = setup_scale_paint(1.0 / s.factor, Some(Box::new(glyph)));
		}
		None => {
			for child in child_paints(paint) {
				visit_paint(child, s);
			}
		}
	}
}

/// The paints a paint points to.
fn child_paints(paint: &mut ot::Paint) -> Vec<&mut ot::Paint> {
	use ot::Paint as P;
	let children: Vec<&mut Option<Box<ot::Paint>>> = match paint {
		P::Format10(p) => vec![&mut p.paint],
		P::Format12(p) => vec![&mut p.paint],
		P::Format13(p) => vec![&mut p.paint],
		P::Format14(p) => vec![&mut p.paint],
		P::Format15(p) => vec![&mut p.paint],
		P::Format16(p) => vec![&mut p.paint],
		P::Format17(p) => vec![&mut p.paint],
		P::Format18(p) => vec![&mut p.paint],
		P::Format19(p) => vec![&mut p.paint],
		P::Format20(p) => vec![&mut p.paint],
		P::Format21(p) => vec![&mut p.paint],
		P::Format22(p) => vec![&mut p.paint],
		P::Format23(p) => vec![&mut p.paint],
		P::Format24(p) => vec![&mut p.paint],
		P::Format25(p) => vec![&mut p.paint],
		P::Format26(p) => vec![&mut p.paint],
		P::Format27(p) => vec![&mut p.paint],
		P::Format28(p) => vec![&mut p.paint],
		P::Format29(p) => vec![&mut p.paint],
		P::Format30(p) => vec![&mut p.paint],
		P::Format31(p) => vec![&mut p.paint],
		P::Format32(p) => vec![&mut p.source_paint, &mut p.backdrop_paint],
		P::Format33(p) => vec![&mut p.paint],
		_ => Vec::new(),
	};
	children
		.into_iter()
		.filter_map(|c| c.as_deref_mut())
		.collect()
}

// VARC

/// A region: the (start, peak, end) of each axis tag.
type Support = BTreeMap<String, (f64, f64, f64)>;

/// `OnlineMultiVarStoreBuilder` (`fontTools.varLib.multiVarStore`), as far as
/// `scaleUpem` uses it.
struct MultiVarStoreBuilder {
	axis_tags: Vec<String>,
	region_map: HashMap<Vec<(String, [u64; 3])>, usize>,
	regions: Vec<ot::SparseVarRegion>,
	data: Vec<ot::MultiVarData>,
	supports: Vec<Support>,
	/// The index of the current `MultiVarData` (`_outer`, if `_data` is set).
	outer: Option<usize>,
	var_data_indices: HashMap<Vec<u16>, usize>,
	var_data_caches: HashMap<Vec<u16>, HashMap<Vec<Vec<i64>>, u64>>,
	/// The key of the current cache.
	cache: Vec<u16>,
}

impl MultiVarStoreBuilder {
	fn new(axis_tags: Vec<String>) -> Self {
		MultiVarStoreBuilder {
			axis_tags,
			region_map: HashMap::new(),
			regions: Vec::new(),
			data: Vec::new(),
			supports: Vec::new(),
			outer: None,
			var_data_indices: HashMap::new(),
			var_data_caches: HashMap::new(),
			cache: Vec::new(),
		}
	}

	/// `setSupports`.
	fn set_supports(&mut self, mut supports: Vec<Support>) {
		if supports.first().is_some_and(|s| s.is_empty()) {
			// Drop base master support
			supports.remove(0);
		}
		self.supports = supports;
		self.outer = None;
	}

	/// `buildSparseVarRegion`.
	fn build_region(&self, support: &Support) -> Result<ot::SparseVarRegion> {
		if !support.keys().all(|t| self.axis_tags.contains(t)) {
			return Err(Error::Assertion("Unknown axis tag found.".into()));
		}
		let axes: Vec<Option<Box<ot::SparseVarRegionAxis>>> = self
			.axis_tags
			.iter()
			.enumerate()
			.filter_map(|(i, tag)| {
				support.get(tag).map(|&(start, peak, end)| {
					Some(Box::new(ot::SparseVarRegionAxis {
						axis_index: i as u16,
						start_coord: start,
						peak_coord: peak,
						end_coord: end,
					}))
				})
			})
			.collect();
		Ok(ot::SparseVarRegion {
			sparse_region_count: Some(axes.len() as u32),
			sparse_var_region_axis: axes,
		})
	}

	/// `_add_MultiVarData`.
	fn add_multi_var_data(&mut self) -> Result<()> {
		let mut region_indices = Vec::new();
		for region in self.supports.clone() {
			let key: Vec<(String, [u64; 3])> = region
				.iter()
				.map(|(t, &(a, b, c))| (t.clone(), [a.to_bits(), b.to_bits(), c.to_bits()]))
				.collect();
			let idx = match self.region_map.get(&key) {
				Some(&i) => i,
				None => {
					let r = self.build_region(&region)?;
					let i = self.regions.len();
					self.regions.push(r);
					self.region_map.insert(key, i);
					i
				}
			};
			region_indices.push(idx as u16);
		}
		let mut index = self.var_data_indices.get(&region_indices).copied();
		if let Some(i) = index
			&& self.data[i].item.len() == 0xFFFF
		{
			// This is full.  Need new one.
			index = None;
		}
		let outer = match index {
			Some(i) => i,
			None => {
				let i = self.data.len();
				self.data.push(ot::MultiVarData {
					format: 1,
					var_region_count: Some(region_indices.len() as u32),
					var_region_index: region_indices.clone(),
					item: Vec::new(),
				});
				self.var_data_indices.insert(region_indices.clone(), i);
				self.var_data_caches
					.entry(region_indices.clone())
					.or_default();
				i
			}
		};
		self.outer = Some(outer);
		self.cache = region_indices;
		Ok(())
	}

	/// `storeDeltas` of integer deltas.
	fn store_deltas(&mut self, deltas: Vec<Vec<i64>>) -> Result<u64> {
		if !deltas.iter().any(|d| d.iter().any(|&v| v != 0)) {
			return Ok(NO_VARIATION_INDEX);
		}
		let outer = match self.outer {
			Some(o) => o,
			None => {
				self.add_multi_var_data()?;
				self.outer.ok_or_else(|| Error::ttlib("no MultiVarData"))?
			}
		};
		if let Some(&idx) = self
			.var_data_caches
			.get(&self.cache)
			.and_then(|c| c.get(&deltas))
		{
			return Ok(idx);
		}
		let inner = self.data[outer].item.len();
		if inner == 0xFFFF {
			// Full array. Start new one.
			self.add_multi_var_data()?;
			return self.store_deltas(deltas);
		}
		// `MultiVarData.addItem`.
		let data = &mut self.data[outer];
		if deltas.len() != data.var_region_index.len() {
			return Err(Error::Assertion(
				"len(deltas) == self.VarRegionCount".into(),
			));
		}
		data.item.push(deltas.iter().flatten().copied().collect());
		let idx = ((outer as u64) << 16) + inner as u64;
		self.var_data_caches
			.entry(self.cache.clone())
			.or_default()
			.insert(deltas, idx);
		Ok(idx)
	}

	/// `finish`.
	fn finish(self) -> ot::MultiVarStore {
		ot::MultiVarStore {
			format: 1,
			sparse_var_region_list: Some(Box::new(ot::SparseVarRegionList {
				region_count: Some(self.regions.len() as u32),
				region: self
					.regions
					.into_iter()
					.map(|r| Some(Box::new(r)))
					.collect(),
			})),
			multi_var_data_count: Some(self.data.len() as u32),
			multi_var_data: self.data.into_iter().map(|d| Some(Box::new(d))).collect(),
		}
	}
}

fn index_error() -> Error {
	Error::ttlib("IndexError: list index out of range")
}

/// `MultiVarStore.get_supports`, and the item of a variation index with its number
/// of values per region.
fn store_item<'s>(
	store: Option<&'s ot::MultiVarStore>,
	var_idx: u64,
	fvar_axes: &[String],
) -> Result<(Vec<Support>, &'s [i64], usize)> {
	let store = store.ok_or_else(|| {
		Error::ttlib("AttributeError: 'NoneType' object has no attribute 'MultiVarData'")
	})?;
	let (major, minor) = ((var_idx >> 16) as usize, (var_idx & 0xFFFF) as usize);
	let data = store
		.multi_var_data
		.get(major)
		.and_then(|d| d.as_deref())
		.ok_or_else(index_error)?;
	let regions = store
		.sparse_var_region_list
		.as_deref()
		.map_or(&[][..], |l| l.region.as_slice());
	let mut supports = Vec::new();
	for &r in &data.var_region_index {
		let region = regions
			.get(r as usize)
			.and_then(|r| r.as_deref())
			.ok_or_else(index_error)?;
		let mut support = Support::new();
		for axis in region.sparse_var_region_axis.iter().flatten() {
			let tag = fvar_axes
				.get(axis.axis_index as usize)
				.ok_or_else(index_error)?;
			support.insert(
				tag.clone(),
				(axis.start_coord, axis.peak_coord, axis.end_coord),
			);
		}
		supports.push(support);
	}
	let item = data.item.get(minor).ok_or_else(index_error)?;
	let count = data
		.var_region_count
		.map_or(data.var_region_index.len(), |c| c as usize);
	Ok((supports, item, count))
}

/// `batched(vec, len(vec) // VarRegionCount)`.
fn batched(item: &[i64], region_count: usize) -> Result<Vec<Vec<i64>>> {
	if region_count == 0 {
		return Err(Error::value("ZeroDivisionError: integer division by zero"));
	}
	let m = item.len() / region_count;
	if m == 0 {
		return Err(Error::value("ValueError: n must be at least one"));
	}
	Ok(item.chunks(m).map(<[i64]>::to_vec).collect())
}

/// The transform values that are scaled, in the order of the stored values.
const TRANSFORM_SCALED: [(u64, bool); 9] = [
	(flags::HAVE_TRANSLATE_X, true),
	(flags::HAVE_TRANSLATE_Y, true),
	(flags::HAVE_ROTATION, false),
	(flags::HAVE_SCALE_X, false),
	(flags::HAVE_SCALE_Y, false),
	(flags::HAVE_SKEW_X, false),
	(flags::HAVE_SKEW_Y, false),
	(flags::HAVE_TCENTER_X, true),
	(flags::HAVE_TCENTER_Y, true),
];

fn scale_varc(varc: &mut ot::VARC, s: Scaler, fvar_axes: &[String]) -> Result<()> {
	let store = varc.multi_var_store.take();
	let store = store.as_deref();
	let mut builder = MultiVarStoreBuilder::new(fvar_axes.to_vec());
	let glyphs = varc
		.var_composite_glyphs
		.as_deref_mut()
		.map_or(&mut [][..], |g| g.var_composite_glyph.as_mut_slice());
	for g in glyphs {
		for component in &mut g.components {
			let t = &mut component.transform;
			for v in [
				&mut t.translate_x,
				&mut t.translate_y,
				&mut t.t_center_x,
				&mut t.t_center_y,
			] {
				*v = s.round(*v) as f64;
			}

			if component.axis_values_var_index != NO_VARIATION_INDEX {
				let (supports, item, count) =
					store_item(store, component.axis_values_var_index, fvar_axes)?;
				builder.set_supports(supports);
				component.axis_values_var_index = if item.is_empty() {
					NO_VARIATION_INDEX
				} else {
					builder.store_deltas(batched(item, count)?)?
				};
			}

			if component.transform_var_index != NO_VARIATION_INDEX {
				let (supports, item, count) =
					store_item(store, component.transform_var_index, fvar_axes)?;
				builder.set_supports(supports);
				component.transform_var_index = if item.is_empty() {
					NO_VARIATION_INDEX
				} else {
					let mut vec = batched(item, count)?;
					for v in &mut vec {
						// Scale translate & tCenter
						let mut i = 0;
						for (flag, scaled) in TRANSFORM_SCALED {
							if component.flags & flag != 0 {
								if scaled {
									let x = v.get_mut(i).ok_or_else(index_error)?;
									*x = s.round(*x as f64);
								}
								i += 1;
							}
						}
					}
					builder.store_deltas(vec)?
				};
			}
		}
	}

	let Some(store) = store else {
		return Ok(());
	};

	// Condition deltas are dimensionless; preserve them without UPEM scaling.
	if let Some(list) = &mut varc.condition_list {
		let mut var_idxes = BTreeSet::new();
		for condition in list.condition_table.iter().flatten() {
			collect_var_idxes(condition, &mut var_idxes);
		}
		let mut mapping: HashMap<u64, u64> = HashMap::new();
		mapping.insert(NO_VARIATION_INDEX, NO_VARIATION_INDEX);
		for &var_idx in var_idxes.iter().filter(|&&v| v != NO_VARIATION_INDEX) {
			let (supports, item, _) = store_item(Some(store), var_idx, fvar_axes)?;
			builder.set_supports(supports);
			let deltas = item.iter().map(|&v| vec![v]).collect();
			mapping.insert(var_idx, builder.store_deltas(deltas)?);
		}
		for condition in list.condition_table.iter_mut().flatten() {
			remap_var_idxes(condition, &mapping)?;
		}
	}

	varc.multi_var_store = Some(Box::new(builder.finish()));
	Ok(())
}

/// `ConditionTable.collect_varidxes`.
fn collect_var_idxes(condition: &ot::ConditionTable, out: &mut BTreeSet<u64>) {
	match condition {
		ot::ConditionTable::Format2(c) => {
			out.insert(u64::from(c.var_idx));
		}
		ot::ConditionTable::Format3(ot::ConditionTableFormat3 {
			condition_table, ..
		})
		| ot::ConditionTable::Format4(ot::ConditionTableFormat4 {
			condition_table, ..
		}) => {
			for c in condition_table.iter().flatten() {
				collect_var_idxes(c, out);
			}
		}
		ot::ConditionTable::Format5(c) => {
			if let Some(c) = &c.condition_table {
				collect_var_idxes(c, out);
			}
		}
		_ => {}
	}
}

/// `ConditionTable.remap_varidxes`.
fn remap_var_idxes(condition: &mut ot::ConditionTable, mapping: &HashMap<u64, u64>) -> Result<()> {
	match condition {
		ot::ConditionTable::Format2(c) => {
			let new = mapping
				.get(&u64::from(c.var_idx))
				.ok_or_else(|| Error::ttlib(format!("KeyError: {}", c.var_idx)))?;
			c.var_idx = u32::try_from(*new).map_err(|_| {
				Error::Struct("'L' format requires 0 <= number <= 4294967295".into())
			})?;
		}
		ot::ConditionTable::Format3(ot::ConditionTableFormat3 {
			condition_table, ..
		})
		| ot::ConditionTable::Format4(ot::ConditionTableFormat4 {
			condition_table, ..
		}) => {
			for c in condition_table.iter_mut().flatten() {
				remap_var_idxes(c, mapping)?;
			}
		}
		ot::ConditionTable::Format5(c) => {
			if let Some(c) = &mut c.condition_table {
				remap_var_idxes(c, mapping)?;
			}
		}
		_ => {}
	}
	Ok(())
}

const HELP: &str = "\
usage: fonttools ttLib.scaleUpem [-h] [--output-file path] font new-upem

Change the units-per-EM of fonts

positional arguments:
  font                Font file.
  new-upem            New units-per-EM integer value.

options:
  -h, --help          show this help message and exit
  --output-file path  Output file.
";

const COMMAND: Command = Command {
	prog: "fonttools ttLib.scaleUpem",
	module: "ttLib.scaleUpem",
	help: HELP,
	arguments: &[
		Argument::help(),
		Argument::positional("font"),
		Argument::positional("new-upem"),
		Argument::option(&["--output-file"]),
	],
	required_groups: &[],
};

/// `fontTools.ttLib.scaleUpem.main`: changes the units-per-EM of a font file.
/// Returns the exit code.
///
/// LENIENT(L55): a new units-per-EM that does not fit `head.unitsPerEm` fails before
/// scaling; fontTools fails when saving the font.
pub fn main(args: &[&str]) -> Result<i32> {
	run(
		args,
		&mut std::io::stdout().lock(),
		&mut std::io::stderr().lock(),
	)
}

/// [`main`], writing to `out` and `err`.
pub fn run(args: &[&str], out: &mut dyn Write, err: &mut dyn Write) -> Result<i32> {
	let options = match COMMAND.parse_args(args, out, err)? {
		Ok(options) => options,
		Err(code) => return Ok(code),
	};
	let font_file = options.str("font").unwrap_or_default();
	let mut font = Font::read(std::fs::read(font_file)?, ReadOptions::default())?;
	let new_upem = options.str("new-upem").unwrap_or_default();
	let new_upem = fontoxide_misc::py::parse_int(new_upem).ok_or_else(|| {
		Error::value(format!(
			"invalid literal for int() with base 10: {}",
			fontoxide_misc::py::str_repr(new_upem)
		))
	})?;
	let new_upem = u16::try_from(new_upem)
		.map_err(|_| Error::Struct("'H' format requires 0 <= number <= 65535".into()))?;
	let output_file = match options.str("--output-file") {
		Some(output) => output.to_owned(),
		None => make_output_file_name(font_file, None, None, true, "-scaled"),
	};
	scale_upem(&mut font, new_upem)?;
	writeln!(out, "Writing {output_file}")?;
	std::fs::write(&output_file, font.write(&WriteOptions::default())?)?;
	Ok(0)
}
