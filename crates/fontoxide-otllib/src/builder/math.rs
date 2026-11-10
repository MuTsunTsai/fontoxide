//! `buildMathTable`.

use fontoxide_misc::round_tools::ot_round;
use fontoxide_ttlib::Font;
use fontoxide_ttlib::ot::types as ot;
use fontoxide_ttlib::tables::Math;
use indexmap::IndexMap;

use super::{build_coverage, sorted_glyphs};
use crate::{Error, GlyphMap, Result};

/// A side of a glyph for math kerning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MathKernSide {
	/// `TopRight`.
	TopRight,
	/// `TopLeft`.
	TopLeft,
	/// `BottomRight`.
	BottomRight,
	/// `BottomLeft`.
	BottomLeft,
}

/// Math kerns by glyph name and side: correction heights and kern values.
pub type MathKerns = IndexMap<String, IndexMap<MathKernSide, (Vec<f64>, Vec<f64>)>>;

/// A part of a glyph assembly: glyph name, flags, start connector length, end
/// connector length and full advance.
pub type GlyphPart = (String, u16, f64, f64, f64);

/// The contents of a MATH table, the arguments of `buildMathTable`. Numbers are
/// rounded; empty maps count as missing.
#[derive(Debug, Clone, Default)]
pub struct MathTableArgs {
	/// Math constants by name, such as `ScriptPercentScaleDown`; the others are
	/// 0 (`constants`).
	pub constants: Option<IndexMap<String, f64>>,
	/// Italic corrections by glyph name (`italicsCorrections`).
	pub italics_corrections: Option<IndexMap<String, f64>>,
	/// Top accent positions by glyph name (`topAccentAttachments`).
	pub top_accent_attachments: Option<IndexMap<String, f64>>,
	/// Extended shape glyphs (`extendedShapes`).
	pub extended_shapes: Option<Vec<String>>,
	/// Math kerns by glyph name and side: correction heights and kern values,
	/// one more than the heights (`mathKerns`).
	pub math_kerns: Option<MathKerns>,
	/// The minimum connector overlap (`minConnectorOverlap`).
	pub min_connector_overlap: u16,
	/// Vertical variants by glyph name: variant glyph names and advances
	/// (`vertGlyphVariants`).
	pub vert_glyph_variants: Option<IndexMap<String, Vec<(String, f64)>>>,
	/// Horizontal variants by glyph name (`horizGlyphVariants`).
	pub horiz_glyph_variants: Option<IndexMap<String, Vec<(String, f64)>>>,
	/// Vertical assemblies by glyph name: parts and italic correction
	/// (`vertGlyphAssembly`).
	pub vert_glyph_assembly: Option<IndexMap<String, (Vec<GlyphPart>, f64)>>,
	/// Horizontal assemblies by glyph name (`horizGlyphAssembly`).
	pub horiz_glyph_assembly: Option<IndexMap<String, (Vec<GlyphPart>, f64)>>,
}

fn nonempty<T: IsEmpty>(v: &Option<T>) -> Option<&T> {
	v.as_ref().filter(|v| !v.is_empty_value())
}

trait IsEmpty {
	fn is_empty_value(&self) -> bool;
}

impl<K, V> IsEmpty for IndexMap<K, V> {
	fn is_empty_value(&self) -> bool {
		self.is_empty()
	}
}

impl<T> IsEmpty for Vec<T> {
	fn is_empty_value(&self) -> bool {
		self.is_empty()
	}
}

fn round<T: TryFrom<i64>>(value: f64) -> Result<T> {
	let v = ot_round(value);
	T::try_from(v).map_err(|_| {
		Error::TtLib(fontoxide_ttlib::Error::Struct(format!(
			"value {v} out of range"
		)))
	})
}

/// `_mathValueRecord`.
fn value_record(value: f64) -> Result<ot::MathValueRecord> {
	Ok(ot::MathValueRecord {
		value: round(value)?,
		device_table: None,
	})
}

/// Adds a `MATH` table to the font, replacing any existing one
/// (`buildMathTable`).
pub fn build_math_table(font: &mut Font, args: &MathTableArgs) -> Result<()> {
	let glyph_map = GlyphMap::from_font(font)?;
	let table = ot::MATH {
		version: 0x0001_0000,
		math_constants: build_math_constants(nonempty(&args.constants))?.map(Box::new),
		math_glyph_info: build_math_glyph_info(&glyph_map, args)?.map(Box::new),
		math_variants: build_math_variants(&glyph_map, args)?.map(Box::new),
	};
	font.insert(Box::new(Math { table }));
	Ok(())
}

macro_rules! constants {
	($c:ident, $get:ident; ints: $($i:ident = $in:literal),*; records: $($r:ident = $rn:literal),*; last: $($l:ident = $ln:literal),*) => {
		$( $c.$i = round($get($in))?; )*
		$( $c.$r = value_record($get($rn))?; )*
		$( $c.$l = round($get($ln))?; )*
	};
}

/// `_buildMathConstants`.
fn build_math_constants(
	constants: Option<&IndexMap<String, f64>>,
) -> Result<Option<ot::MathConstants>> {
	let Some(constants) = constants else {
		return Ok(None);
	};
	let get = |name: &str| constants.get(name).copied().unwrap_or(0.0);
	let mut c = ot::MathConstants::default();
	constants!(c, get;
		ints:
			script_percent_scale_down = "ScriptPercentScaleDown",
			script_script_percent_scale_down = "ScriptScriptPercentScaleDown",
			delimited_sub_formula_min_height = "DelimitedSubFormulaMinHeight",
			display_operator_min_height = "DisplayOperatorMinHeight";
		records:
			math_leading = "MathLeading",
			axis_height = "AxisHeight",
			accent_base_height = "AccentBaseHeight",
			flattened_accent_base_height = "FlattenedAccentBaseHeight",
			subscript_shift_down = "SubscriptShiftDown",
			subscript_top_max = "SubscriptTopMax",
			subscript_baseline_drop_min = "SubscriptBaselineDropMin",
			superscript_shift_up = "SuperscriptShiftUp",
			superscript_shift_up_cramped = "SuperscriptShiftUpCramped",
			superscript_bottom_min = "SuperscriptBottomMin",
			superscript_baseline_drop_max = "SuperscriptBaselineDropMax",
			sub_superscript_gap_min = "SubSuperscriptGapMin",
			superscript_bottom_max_with_subscript = "SuperscriptBottomMaxWithSubscript",
			space_after_script = "SpaceAfterScript",
			upper_limit_gap_min = "UpperLimitGapMin",
			upper_limit_baseline_rise_min = "UpperLimitBaselineRiseMin",
			lower_limit_gap_min = "LowerLimitGapMin",
			lower_limit_baseline_drop_min = "LowerLimitBaselineDropMin",
			stack_top_shift_up = "StackTopShiftUp",
			stack_top_display_style_shift_up = "StackTopDisplayStyleShiftUp",
			stack_bottom_shift_down = "StackBottomShiftDown",
			stack_bottom_display_style_shift_down = "StackBottomDisplayStyleShiftDown",
			stack_gap_min = "StackGapMin",
			stack_display_style_gap_min = "StackDisplayStyleGapMin",
			stretch_stack_top_shift_up = "StretchStackTopShiftUp",
			stretch_stack_bottom_shift_down = "StretchStackBottomShiftDown",
			stretch_stack_gap_above_min = "StretchStackGapAboveMin",
			stretch_stack_gap_below_min = "StretchStackGapBelowMin",
			fraction_numerator_shift_up = "FractionNumeratorShiftUp",
			fraction_numerator_display_style_shift_up = "FractionNumeratorDisplayStyleShiftUp",
			fraction_denominator_shift_down = "FractionDenominatorShiftDown",
			fraction_denominator_display_style_shift_down = "FractionDenominatorDisplayStyleShiftDown",
			fraction_numerator_gap_min = "FractionNumeratorGapMin",
			fraction_num_display_style_gap_min = "FractionNumDisplayStyleGapMin",
			fraction_rule_thickness = "FractionRuleThickness",
			fraction_denominator_gap_min = "FractionDenominatorGapMin",
			fraction_denom_display_style_gap_min = "FractionDenomDisplayStyleGapMin",
			skewed_fraction_horizontal_gap = "SkewedFractionHorizontalGap",
			skewed_fraction_vertical_gap = "SkewedFractionVerticalGap",
			overbar_vertical_gap = "OverbarVerticalGap",
			overbar_rule_thickness = "OverbarRuleThickness",
			overbar_extra_ascender = "OverbarExtraAscender",
			underbar_vertical_gap = "UnderbarVerticalGap",
			underbar_rule_thickness = "UnderbarRuleThickness",
			underbar_extra_descender = "UnderbarExtraDescender",
			radical_vertical_gap = "RadicalVerticalGap",
			radical_display_style_vertical_gap = "RadicalDisplayStyleVerticalGap",
			radical_rule_thickness = "RadicalRuleThickness",
			radical_extra_ascender = "RadicalExtraAscender",
			radical_kern_before_degree = "RadicalKernBeforeDegree",
			radical_kern_after_degree = "RadicalKernAfterDegree";
		last:
			radical_degree_bottom_raise_percent = "RadicalDegreeBottomRaisePercent"
	);
	Ok(Some(c))
}

/// The value records of glyphs, in coverage order.
fn glyph_values(
	glyph_map: &GlyphMap,
	values: &IndexMap<String, f64>,
) -> Result<(ot::Coverage, Vec<ot::MathValueRecord>)> {
	let coverage = build_coverage(values.keys().map(String::as_str), glyph_map)?;
	let records = sorted_glyphs(values.keys().map(String::as_str), glyph_map)?
		.into_iter()
		.map(|(_, g)| value_record(values[g]))
		.collect::<Result<_>>()?;
	Ok((coverage, records))
}

/// `_buildMathGlyphInfo`.
fn build_math_glyph_info(
	glyph_map: &GlyphMap,
	args: &MathTableArgs,
) -> Result<Option<ot::MathGlyphInfo>> {
	let italics = nonempty(&args.italics_corrections);
	let top_accent = nonempty(&args.top_accent_attachments);
	let shapes = nonempty(&args.extended_shapes);
	let kerns = nonempty(&args.math_kerns);
	if italics.is_none() && top_accent.is_none() && shapes.is_none() && kerns.is_none() {
		return Ok(None);
	}
	let mut info = ot::MathGlyphInfo::default();
	if let Some(italics) = italics {
		let (coverage, records) = glyph_values(glyph_map, italics)?;
		info.math_italics_correction_info = Some(Box::new(ot::MathItalicsCorrectionInfo {
			italics_correction_count: Some(coverage.glyphs.len() as u32),
			coverage: Some(Box::new(coverage)),
			italics_correction: records,
		}));
	}
	if let Some(top_accent) = top_accent {
		let (coverage, records) = glyph_values(glyph_map, top_accent)?;
		info.math_top_accent_attachment = Some(Box::new(ot::MathTopAccentAttachment {
			top_accent_attachment_count: Some(coverage.glyphs.len() as u32),
			top_accent_coverage: Some(Box::new(coverage)),
			top_accent_attachment: records,
		}));
	}
	if let Some(shapes) = shapes {
		info.extended_shape_coverage = Some(Box::new(build_coverage(
			shapes.iter().map(String::as_str),
			glyph_map,
		)?));
	}
	if let Some(kerns) = kerns {
		let glyphs = sorted_glyphs(kerns.keys().map(String::as_str), glyph_map)?;
		let mut records = Vec::with_capacity(glyphs.len());
		for &(_, glyph) in &glyphs {
			let mut record = ot::MathKernInfoRecord::default();
			for (side, (heights, values)) in &kerns[glyph] {
				if heights.len() + 1 != values.len() {
					return Err(Error::Assertion(
						"len(correctionHeights) == len(kernValues) - 1".into(),
					));
				}
				let kern = Some(Box::new(ot::MathKern {
					height_count: Some(heights.len() as u32),
					correction_height: heights
						.iter()
						.map(|&h| value_record(h))
						.collect::<Result<_>>()?,
					kern_value: values
						.iter()
						.map(|&v| value_record(v))
						.collect::<Result<_>>()?,
				}));
				match side {
					MathKernSide::TopRight => record.top_right_math_kern = kern,
					MathKernSide::TopLeft => record.top_left_math_kern = kern,
					MathKernSide::BottomRight => record.bottom_right_math_kern = kern,
					MathKernSide::BottomLeft => record.bottom_left_math_kern = kern,
				}
			}
			records.push(record);
		}
		info.math_kern_info = Some(Box::new(ot::MathKernInfo {
			math_kern_coverage: Some(Box::new(ot::Coverage {
				glyphs: glyphs.iter().map(|&(id, _)| id).collect(),
			})),
			math_kern_count: Some(records.len() as u32),
			math_kern_info_records: records,
		}));
	}
	Ok(Some(info))
}

type Variants = IndexMap<String, Vec<(String, f64)>>;
type Assemblies = IndexMap<String, (Vec<GlyphPart>, f64)>;

/// `_buildMathVariants`.
fn build_math_variants(
	glyph_map: &GlyphMap,
	args: &MathTableArgs,
) -> Result<Option<ot::MathVariants>> {
	let vv = nonempty(&args.vert_glyph_variants);
	let hv = nonempty(&args.horiz_glyph_variants);
	let va = nonempty(&args.vert_glyph_assembly);
	let ha = nonempty(&args.horiz_glyph_assembly);
	if vv.is_none() && hv.is_none() && va.is_none() && ha.is_none() {
		return Ok(None);
	}
	// fontTools leaves the counts that populateDefaults set (0); they are
	// computed when compiling.
	let mut variants = ot::MathVariants {
		min_connector_overlap: args.min_connector_overlap,
		vert_glyph_count: Some(0),
		horiz_glyph_count: Some(0),
		..Default::default()
	};
	if vv.is_some() || va.is_some() {
		let (coverage, constructions) = build_construction(glyph_map, vv, va)?;
		variants.vert_glyph_coverage = Some(Box::new(coverage));
		variants.vert_glyph_construction = constructions;
	}
	if hv.is_some() || ha.is_some() {
		let (coverage, constructions) = build_construction(glyph_map, hv, ha)?;
		variants.horiz_glyph_coverage = Some(Box::new(coverage));
		variants.horiz_glyph_construction = constructions;
	}
	Ok(Some(variants))
}

/// `_buildMathGlyphConstruction`.
fn build_construction(
	glyph_map: &GlyphMap,
	variants: Option<&Variants>,
	assemblies: Option<&Assemblies>,
) -> Result<(ot::Coverage, Vec<Option<Box<ot::MathGlyphConstruction>>>)> {
	let names = variants
		.into_iter()
		.flat_map(|v| v.keys())
		.chain(assemblies.into_iter().flat_map(|a| a.keys()))
		.map(String::as_str);
	let glyphs = sorted_glyphs(names, glyph_map)?;
	let mut constructions = Vec::with_capacity(glyphs.len());
	for &(_, name) in &glyphs {
		let mut construction = ot::MathGlyphConstruction {
			variant_count: Some(0),
			..Default::default()
		};
		if let Some(list) = variants.and_then(|v| v.get(name)) {
			construction.variant_count = Some(list.len() as u32);
			construction.math_glyph_variant_record = list
				.iter()
				.map(|(variant, advance)| {
					Ok(ot::MathGlyphVariantRecord {
						variant_glyph: glyph_map.id(variant)?,
						advance_measurement: round(*advance)?,
					})
				})
				.collect::<Result<_>>()?;
		}
		if let Some((parts, ic)) = assemblies.and_then(|a| a.get(name)) {
			construction.glyph_assembly = Some(Box::new(ot::GlyphAssembly {
				italics_correction: value_record(*ic)?,
				part_count: Some(parts.len() as u32),
				part_records: parts
					.iter()
					.map(|(part, flags, start, end, advance)| {
						Ok(ot::GlyphPartRecord {
							glyph: glyph_map.id(part)?,
							start_connector_length: round(*start)?,
							end_connector_length: round(*end)?,
							full_advance: round(*advance)?,
							part_flags: *flags,
						})
					})
					.collect::<Result<_>>()?,
			}));
		}
		constructions.push(Some(Box::new(construction)));
	}
	let coverage = ot::Coverage {
		glyphs: glyphs.iter().map(|&(id, _)| id).collect(),
	};
	Ok((coverage, constructions))
}
