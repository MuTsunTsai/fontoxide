//! The GSUB substitution subtables that fontTools represents as mappings
//! (`SingleSubst`, `MultipleSubst`, `AlternateSubst`, `LigatureSubst`).

use std::collections::BTreeMap;

use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use super::custom::{Coverage, has_extended_glyph_ids};
use super::table::{OtTable, XmlCtx, simpletag};
use super::{OtReader, OtWriter};
pub use crate::generated::ot::Ligature;
use crate::generated::ot::{
	AlternateSet, AlternateSet2, AlternateSubstRaw, AlternateSubstRawFormat1,
	AlternateSubstRawFormat2, Ligature2, LigatureSet, LigatureSet2, LigatureSubstRaw,
	LigatureSubstRawFormat1, LigatureSubstRawFormat2, MultipleSubstRaw, MultipleSubstRawFormat1,
	MultipleSubstRawFormat2, Sequence, Sequence2, SingleSubstRaw, SingleSubstRawFormat1,
	SingleSubstRawFormat2, SingleSubstRawFormat3, SingleSubstRawFormat4,
};
use crate::{Error, Font, Result};

fn coverage_glyphs(c: &Option<Box<Coverage>>) -> Vec<u32> {
	c.as_ref().map(|c| c.glyphs.clone()).unwrap_or_default()
}

fn coverage_of(glyphs: Vec<u32>) -> Option<Box<Coverage>> {
	Some(Box::new(Coverage { glyphs }))
}

fn none_error(what: &str) -> Error {
	Error::ttlib(format!("'NoneType' object has no attribute '{what}'"))
}

fn attr<'a>(e: &'a Element, name: &str) -> Result<&'a str> {
	e.attr(name)
		.ok_or_else(|| Error::ttx(format!("KeyError: {name:?}")))
}

fn any_extended(
	font: &mut Font,
	count: usize,
	mut glyphs: impl Iterator<Item = u32>,
) -> Result<bool> {
	Ok(has_extended_glyph_ids(font)? && (count > 0xFFFF || glyphs.any(|g| g > 0xFFFF)))
}

macro_rules! lookup_subtable_impl {
	($ty:ident, $raw:ident, $name:literal) => {
		impl OtTable for $ty {
			fn class_name(&self) -> &'static str {
				$name
			}

			fn decompile(r: &mut OtReader<'_>, font: &mut Font) -> Result<Self> {
				$ty::from_raw($raw::decompile(r, font)?)
			}

			fn compile(&self, w: &mut OtWriter<'_>, font: &mut Font) -> Result<()> {
				if self.dont_share {
					w.set_dont_share();
				}
				self.to_raw(w, font)?.compile(w, font)
			}

			fn dont_share(&self) -> bool {
				self.dont_share
			}

			fn remap_glyphs(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
				self.remap(f)
			}

			fn set_dont_share(&mut self) {
				self.dont_share = true;
			}

			fn to_xml2(&self, xw: &mut XmlWriter, font: &mut Font) -> Result<()> {
				self.xml(xw, font)
			}

			fn from_xml(
				&mut self,
				element: &Element,
				font: &mut Font,
				_ctx: &mut XmlCtx,
			) -> Result<()> {
				self.read_xml(element, font)
			}

			fn populate_defaults(&mut self, _ctx: &mut XmlCtx) -> Result<()> {
				Ok(())
			}
		}
	};
}

/// Single substitution (`SingleSubst`, which fontTools keeps as `mapping`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SingleSubst {
	/// Input glyph to output glyph.
	pub mapping: BTreeMap<u32, u32>,
	/// Whether the subtables below are never shared (`DontShare`), set when
	/// resolving offset overflows.
	pub dont_share: bool,
}

impl SingleSubst {
	fn remap(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let mut out = BTreeMap::new();
		for (k, v) in std::mem::take(&mut self.mapping) {
			out.insert(f(k)?, f(v)?);
		}
		self.mapping = out;
		Ok(())
	}

	/// `postRead`.
	fn from_raw(raw: SingleSubstRaw) -> Result<Self> {
		let mut mapping = BTreeMap::new();
		match raw {
			SingleSubstRaw::Format1(SingleSubstRawFormat1 {
				coverage,
				delta_glyph_id,
			}) => {
				for g in coverage_glyphs(&coverage) {
					mapping.insert(g, ((g as u64 + delta_glyph_id as u64) % 0x10000) as u32);
				}
			}
			SingleSubstRaw::Format3(SingleSubstRawFormat3 {
				coverage,
				delta_glyph_id,
			}) => {
				for g in coverage_glyphs(&coverage) {
					mapping.insert(
						g,
						(g as i64 + delta_glyph_id as i64).rem_euclid(0x100_0000) as u32,
					);
				}
			}
			SingleSubstRaw::Format2(SingleSubstRawFormat2 {
				coverage,
				glyph_count,
				substitute,
			})
			| SingleSubstRaw::Format4(SingleSubstRawFormat4 {
				coverage,
				glyph_count,
				substitute,
			}) => {
				let input = coverage_glyphs(&coverage);
				if Some(input.len() as u32) != glyph_count {
					return Err(Error::Assertion("invalid SingleSubst table".into()));
				}
				for (i, s) in input.into_iter().zip(substitute) {
					mapping.insert(i, s);
				}
			}
			SingleSubstRaw::Unknown(f) => {
				return Err(Error::Assertion(format!("unknown format: {f}")));
			}
		}
		Ok(SingleSubst {
			mapping,
			dont_share: false,
		})
	}

	/// `preWrite`.
	fn to_raw(&self, _w: &mut OtWriter<'_>, font: &mut Font) -> Result<SingleSubstRaw> {
		let extended = any_extended(
			font,
			self.mapping.len(),
			self.mapping.iter().flat_map(|(&a, &b)| [a, b]),
		)?;
		let modulus: i64 = if extended { 0x100_0000 } else { 0x10000 };
		let mut delta: Option<i64> = None;
		let mut same = true;
		for (&a, &b) in &self.mapping {
			let d = *delta.get_or_insert((b as i64 - a as i64).rem_euclid(modulus));
			if (a as i64 + d).rem_euclid(modulus) != b as i64 {
				same = false;
				break;
			}
		}
		let input: Vec<u32> = self.mapping.keys().copied().collect();
		let subst: Vec<u32> = self.mapping.values().copied().collect();
		Ok(match (same, delta, extended) {
			(true, Some(d), false) => SingleSubstRaw::Format1(SingleSubstRawFormat1 {
				coverage: coverage_of(input),
				delta_glyph_id: d as u16,
			}),
			(true, Some(d), true) => SingleSubstRaw::Format3(SingleSubstRawFormat3 {
				coverage: coverage_of(input),
				// DeltaGlyphID is computed modulo the glyph ID space, but stored
				// signed.
				delta_glyph_id: if d >= 0x80_0000 {
					(d - 0x100_0000) as i32
				} else {
					d as i32
				},
			}),
			(_, _, false) => SingleSubstRaw::Format2(SingleSubstRawFormat2 {
				coverage: coverage_of(input),
				glyph_count: None,
				substitute: subst,
			}),
			(_, _, true) => SingleSubstRaw::Format4(SingleSubstRawFormat4 {
				coverage: coverage_of(input),
				glyph_count: None,
				substitute: subst,
			}),
		})
	}

	fn xml(&self, xw: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let order = font.glyph_order()?;
		let mut items: Vec<(String, String)> = self
			.mapping
			.iter()
			.map(|(&a, &b)| (order.name(a).into_owned(), order.name(b).into_owned()))
			.collect();
		items.sort();
		for (a, b) in items {
			simpletag(xw, "Substitution", &[("in", a), ("out", b)]);
			xw.newline();
		}
		Ok(())
	}

	fn read_xml(&mut self, e: &Element, font: &mut Font) -> Result<()> {
		let a = font.glyph_id(attr(e, "in")?)?;
		let b = font.glyph_id(attr(e, "out")?)?;
		self.mapping.insert(a, b);
		Ok(())
	}
}

lookup_subtable_impl!(SingleSubst, SingleSubstRaw, "SingleSubst");

/// Multiple substitution (`MultipleSubst`, which fontTools keeps as `mapping`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MultipleSubst {
	/// Input glyph to output glyphs.
	pub mapping: BTreeMap<u32, Vec<u32>>,
	/// Whether the subtables below are never shared (`DontShare`), set when
	/// resolving offset overflows.
	pub dont_share: bool,
}

impl MultipleSubst {
	fn remap(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let mut out = BTreeMap::new();
		for (k, v) in std::mem::take(&mut self.mapping) {
			let v = v.into_iter().map(&mut *f).collect::<Result<Vec<_>>>()?;
			out.insert(f(k)?, v);
		}
		self.mapping = out;
		Ok(())
	}

	/// `postRead`.
	fn from_raw(raw: MultipleSubstRaw) -> Result<Self> {
		let (glyphs, subst): (Vec<u32>, Vec<Vec<u32>>) = match raw {
			MultipleSubstRaw::Format1(MultipleSubstRawFormat1 {
				coverage, sequence, ..
			}) => (
				coverage_glyphs(&coverage),
				sequence
					.into_iter()
					.map(|s| {
						s.map(|s| s.substitute)
							.ok_or_else(|| none_error("Substitute"))
					})
					.collect::<Result<_>>()?,
			),
			MultipleSubstRaw::Format2(MultipleSubstRawFormat2 {
				coverage, sequence, ..
			}) => (
				coverage_glyphs(&coverage),
				sequence
					.into_iter()
					.map(|s| {
						s.map(|s| s.substitute)
							.ok_or_else(|| none_error("Substitute"))
					})
					.collect::<Result<_>>()?,
			),
			MultipleSubstRaw::Unknown(f) => {
				return Err(Error::Assertion(format!("unknown format: {f}")));
			}
		};
		Ok(MultipleSubst {
			dont_share: false,
			mapping: glyphs.into_iter().zip(subst).collect(),
		})
	}

	/// `preWrite`.
	fn to_raw(&self, _w: &mut OtWriter<'_>, font: &mut Font) -> Result<MultipleSubstRaw> {
		let extended = any_extended(
			font,
			self.mapping.len(),
			self.mapping
				.iter()
				.flat_map(|(&g, s)| std::iter::once(g).chain(s.iter().copied())),
		)?;
		let glyphs: Vec<u32> = self.mapping.keys().copied().collect();
		Ok(if extended {
			MultipleSubstRaw::Format2(MultipleSubstRawFormat2 {
				coverage: coverage_of(glyphs),
				sequence_count: None,
				sequence: self
					.mapping
					.values()
					.map(|s| {
						Some(Box::new(Sequence2 {
							glyph_count: None,
							substitute: s.clone(),
						}))
					})
					.collect(),
			})
		} else {
			MultipleSubstRaw::Format1(MultipleSubstRawFormat1 {
				coverage: coverage_of(glyphs),
				sequence_count: None,
				sequence: self
					.mapping
					.values()
					.map(|s| {
						Some(Box::new(Sequence {
							glyph_count: None,
							substitute: s.clone(),
						}))
					})
					.collect(),
			})
		})
	}

	fn xml(&self, xw: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let order = font.glyph_order()?;
		let mut items: Vec<(String, Vec<String>)> = self
			.mapping
			.iter()
			.map(|(&a, b)| {
				(
					order.name(a).into_owned(),
					b.iter().map(|&g| order.name(g).into_owned()).collect(),
				)
			})
			.collect();
		items.sort();
		for (a, b) in items {
			simpletag(xw, "Substitution", &[("in", a), ("out", b.join(","))]);
			xw.newline();
		}
		Ok(())
	}

	fn read_xml(&mut self, e: &Element, font: &mut Font) -> Result<()> {
		if matches!(e.name.as_str(), "Coverage" | "Sequence") {
			return Err(Error::ttx(
				"MultipleSubst in the TTX format of fontTools 3.0 and earlier is not supported",
			));
		}
		let out = attr(e, "out")?;
		let mut outs = Vec::new();
		if !out.is_empty() {
			for g in out.split(',') {
				outs.push(font.glyph_id(fontoxide_misc::py::strip(g))?);
			}
		}
		let a = font.glyph_id(attr(e, "in")?)?;
		self.mapping.insert(a, outs);
		Ok(())
	}
}

lookup_subtable_impl!(MultipleSubst, MultipleSubstRaw, "MultipleSubst");

/// Alternate substitution (`AlternateSubst`, which fontTools keeps as
/// `alternates`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlternateSubst {
	/// Input glyph to its alternates.
	pub alternates: BTreeMap<u32, Vec<u32>>,
	/// Whether the subtables below are never shared (`DontShare`), set when
	/// resolving offset overflows.
	pub dont_share: bool,
}

impl AlternateSubst {
	fn remap(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let mut out = BTreeMap::new();
		for (k, v) in std::mem::take(&mut self.alternates) {
			let v = v.into_iter().map(&mut *f).collect::<Result<Vec<_>>>()?;
			out.insert(f(k)?, v);
		}
		self.alternates = out;
		Ok(())
	}

	/// `postRead`.
	fn from_raw(raw: AlternateSubstRaw) -> Result<Self> {
		let (input, alts): (Vec<u32>, Vec<Vec<u32>>) = match raw {
			AlternateSubstRaw::Format1(AlternateSubstRawFormat1 {
				coverage,
				alternate_set,
				..
			}) => (
				coverage_glyphs(&coverage),
				alternate_set
					.into_iter()
					.map(|s| {
						s.map(|s| s.alternate)
							.ok_or_else(|| none_error("Alternate"))
					})
					.collect::<Result<_>>()?,
			),
			AlternateSubstRaw::Format2(AlternateSubstRawFormat2 {
				coverage,
				alternate_set,
				..
			}) => (
				coverage_glyphs(&coverage),
				alternate_set
					.into_iter()
					.map(|s| {
						s.map(|s| s.alternate)
							.ok_or_else(|| none_error("Alternate"))
					})
					.collect::<Result<_>>()?,
			),
			AlternateSubstRaw::Unknown(f) => {
				return Err(Error::Assertion(format!("unknown format: {f}")));
			}
		};
		if input.len() != alts.len() {
			return Err(Error::Assertion("len(input) == len(alts)".into()));
		}
		Ok(AlternateSubst {
			dont_share: false,
			alternates: input.into_iter().zip(alts).collect(),
		})
	}

	/// `preWrite`.
	fn to_raw(&self, w: &mut OtWriter<'_>, font: &mut Font) -> Result<AlternateSubstRaw> {
		let extended = any_extended(
			font,
			self.alternates.len(),
			self.alternates
				.iter()
				.flat_map(|(&g, s)| std::iter::once(g).chain(s.iter().copied())),
		)?;
		// Push the coverage table to the end (`sortCoverageLast`).
		w.set_sort_coverage_last();
		let glyphs: Vec<u32> = self.alternates.keys().copied().collect();
		Ok(if extended {
			AlternateSubstRaw::Format2(AlternateSubstRawFormat2 {
				coverage: coverage_of(glyphs),
				alternate_set_count: None,
				alternate_set: self
					.alternates
					.values()
					.map(|s| {
						Some(Box::new(AlternateSet2 {
							glyph_count: None,
							alternate: s.clone(),
						}))
					})
					.collect(),
			})
		} else {
			AlternateSubstRaw::Format1(AlternateSubstRawFormat1 {
				coverage: coverage_of(glyphs),
				alternate_set_count: None,
				alternate_set: self
					.alternates
					.values()
					.map(|s| {
						Some(Box::new(AlternateSet {
							glyph_count: None,
							alternate: s.clone(),
						}))
					})
					.collect(),
			})
		})
	}

	fn xml(&self, xw: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let order = font.glyph_order()?;
		let mut items: Vec<(String, Vec<String>)> = self
			.alternates
			.iter()
			.map(|(&a, b)| {
				(
					order.name(a).into_owned(),
					b.iter().map(|&g| order.name(g).into_owned()).collect(),
				)
			})
			.collect();
		items.sort();
		for (glyph, alts) in items {
			xw.begintag("AlternateSet", &[("glyph", &glyph)]);
			xw.newline();
			for alt in alts {
				xw.simpletag("Alternate", &[("glyph", &alt)]);
				xw.newline();
			}
			xw.endtag("AlternateSet");
			xw.newline();
		}
		Ok(())
	}

	fn read_xml(&mut self, e: &Element, font: &mut Font) -> Result<()> {
		let glyph = font.glyph_id(attr(e, "glyph")?)?;
		let mut set = Vec::new();
		for child in e.elements() {
			set.push(font.glyph_id(attr(child, "glyph")?)?);
		}
		self.alternates.insert(glyph, set);
		Ok(())
	}
}

lookup_subtable_impl!(AlternateSubst, AlternateSubstRaw, "AlternateSubst");

/// Ligature substitution (`LigatureSubst`, which fontTools keeps as
/// `ligatures`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LigatureSubst {
	/// First component glyph to the ligatures that start with it.
	pub ligatures: BTreeMap<u32, Vec<Ligature>>,
	/// Whether the subtables below are never shared (`DontShare`), set when
	/// resolving offset overflows.
	pub dont_share: bool,
}

impl LigatureSubst {
	fn remap(&mut self, f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		let mut out = BTreeMap::new();
		for (k, mut v) in std::mem::take(&mut self.ligatures) {
			for lig in &mut v {
				lig.remap_glyphs(f)?;
			}
			out.insert(f(k)?, v);
		}
		self.ligatures = out;
		Ok(())
	}

	/// `postRead`.
	fn from_raw(raw: LigatureSubstRaw) -> Result<Self> {
		let (input, sets): (Vec<u32>, Vec<Vec<Ligature>>) = match raw {
			LigatureSubstRaw::Format1(LigatureSubstRawFormat1 {
				coverage,
				ligature_set,
				..
			}) => (
				coverage_glyphs(&coverage),
				ligature_set
					.into_iter()
					.map(|s| {
						let s = s.ok_or_else(|| none_error("Ligature"))?;
						s.ligature
							.into_iter()
							.map(|l| l.map(|l| *l).ok_or_else(|| none_error("LigGlyph")))
							.collect::<Result<Vec<_>>>()
					})
					.collect::<Result<_>>()?,
			),
			LigatureSubstRaw::Format2(LigatureSubstRawFormat2 {
				coverage,
				ligature_set,
				..
			}) => (
				coverage_glyphs(&coverage),
				ligature_set
					.into_iter()
					.map(|s| {
						let s = s.ok_or_else(|| none_error("Ligature"))?;
						s.ligature
							.into_iter()
							.map(|l| {
								l.map(|l| Ligature {
									lig_glyph: l.lig_glyph,
									comp_count: l.comp_count,
									component: l.component,
								})
								.ok_or_else(|| none_error("LigGlyph"))
							})
							.collect::<Result<Vec<_>>>()
					})
					.collect::<Result<_>>()?,
			),
			LigatureSubstRaw::Unknown(f) => {
				return Err(Error::Assertion(format!("unknown format: {f}")));
			}
		};
		if input.len() != sets.len() {
			return Err(Error::Assertion("len(input) == len(ligSets)".into()));
		}
		Ok(LigatureSubst {
			dont_share: false,
			ligatures: input.into_iter().zip(sets).collect(),
		})
	}

	/// `preWrite`: the table as it is written, with its format.
	pub fn to_raw(&self, w: &mut OtWriter<'_>, font: &mut Font) -> Result<LigatureSubstRaw> {
		let extended =
			any_extended(
				font,
				self.ligatures.len(),
				self.ligatures.iter().flat_map(|(&g, set)| {
					std::iter::once(g).chain(set.iter().flat_map(|l| {
						std::iter::once(l.lig_glyph).chain(l.component.iter().copied())
					}))
				}),
			)?;
		// Push the coverage table to the end (`sortCoverageLast`).
		w.set_sort_coverage_last();
		let glyphs: Vec<u32> = self.ligatures.keys().copied().collect();
		Ok(if extended {
			LigatureSubstRaw::Format2(LigatureSubstRawFormat2 {
				coverage: coverage_of(glyphs),
				lig_set_count: None,
				ligature_set: self
					.ligatures
					.values()
					.map(|set| {
						Some(Box::new(LigatureSet2 {
							ligature_count: None,
							ligature: set
								.iter()
								.map(|l| {
									Some(Box::new(Ligature2 {
										lig_glyph: l.lig_glyph,
										comp_count: l.comp_count,
										component: l.component.clone(),
									}))
								})
								.collect(),
						}))
					})
					.collect(),
			})
		} else {
			LigatureSubstRaw::Format1(LigatureSubstRawFormat1 {
				coverage: coverage_of(glyphs),
				lig_set_count: None,
				ligature_set: self
					.ligatures
					.values()
					.map(|set| {
						Some(Box::new(LigatureSet {
							ligature_count: None,
							ligature: set.iter().map(|l| Some(Box::new(l.clone()))).collect(),
						}))
					})
					.collect(),
			})
		})
	}

	fn xml(&self, xw: &mut XmlWriter, font: &mut Font) -> Result<()> {
		let order = font.glyph_order()?;
		let mut items: Vec<(String, &Vec<Ligature>)> = self
			.ligatures
			.iter()
			.map(|(&g, set)| (order.name(g).into_owned(), set))
			.collect();
		items.sort_by(|a, b| a.0.cmp(&b.0));
		for (glyph, set) in items {
			xw.begintag("LigatureSet", &[("glyph", &glyph)]);
			xw.newline();
			for lig in set {
				let components: Vec<String> = lig
					.component
					.iter()
					.map(|&c| order.name(c).into_owned())
					.collect();
				xw.simpletag(
					"Ligature",
					&[
						("glyph", &order.name(lig.lig_glyph)),
						("components", &components.join(",")),
					],
				);
				xw.newline();
			}
			xw.endtag("LigatureSet");
			xw.newline();
		}
		Ok(())
	}

	fn read_xml(&mut self, e: &Element, font: &mut Font) -> Result<()> {
		let glyph = font.glyph_id(attr(e, "glyph")?)?;
		let mut ligs = Vec::new();
		for child in e.elements() {
			let lig_glyph = font.glyph_id(attr(child, "glyph")?)?;
			let components = attr(child, "components")?;
			let mut component = Vec::new();
			if !components.is_empty() {
				for c in components.split(',') {
					component.push(font.glyph_id(c)?);
				}
			}
			ligs.push(Ligature {
				lig_glyph,
				// UPSTREAM-BUG(B24): one less than when read from binary.
				comp_count: Some(component.len() as u32),
				component,
			});
		}
		self.ligatures.insert(glyph, ligs);
		Ok(())
	}
}

lookup_subtable_impl!(LigatureSubst, LigatureSubstRaw, "LigatureSubst");
