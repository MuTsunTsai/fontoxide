//! Walking the syntax tree: the `build()` methods of the `ast` classes, which
//! call the builder's callbacks.

use indexmap::IndexMap;

use super::{Builder, NameEntry, NameKey};
use crate::ast::{
	GlyphContainer, MultipleSubstStatement, PairPosStatement, ScalarValue, Statement,
};
use crate::error::{Error, Result};
use crate::py_set::py_int_set_order;

/// `glyphSet()` as a list.
fn gs(g: &GlyphContainer) -> Vec<String> {
	g.glyph_set().into_owned()
}

/// `[p.glyphSet() for p in seq]`.
fn gss(seq: &[GlyphContainer]) -> Vec<Vec<String>> {
	seq.iter().map(gs).collect()
}

/// `glyphSet()` of an optional container, empty if there is none.
fn opt_gs(g: &Option<GlyphContainer>) -> Vec<String> {
	g.as_ref().map(gs).unwrap_or_default()
}

/// `dict(zip(originals, replaces))`, with a single replacement repeated
/// (`replaces * len(originals)`).
fn zip_mapping(originals: &[String], replaces: &[String]) -> IndexMap<String, String> {
	let repeated;
	let replaces = if replaces.len() == 1 {
		repeated = vec![replaces[0].clone(); originals.len()];
		&repeated[..]
	} else {
		replaces
	};
	originals
		.iter()
		.cloned()
		.zip(replaces.iter().cloned())
		.collect()
}

fn index_error() -> Error {
	Error::Value("IndexError: list index out of range".to_owned())
}

impl Builder<'_> {
	/// Walks statements, calling the builder callbacks (`Block.build`).
	pub fn build_statements(&mut self, statements: &[Statement]) -> Result<()> {
		for s in statements {
			self.build_statement(s)?;
		}
		Ok(())
	}

	/// `Statement.build(builder)`.
	fn build_statement(&mut self, s: &Statement) -> Result<()> {
		match s {
			Statement::Comment(_)
			| Statement::AnonymousBlock(_)
			| Statement::GlyphClassDefinition(_)
			| Statement::MarkClassDefinition(_)
			| Statement::AnchorDefinition(_)
			| Statement::IncludeStatement(_)
			| Statement::ValueRecordDefinition(_) => Ok(()),
			Statement::FeatureBlock(b) => {
				self.start_feature(&b.location, &b.name, b.use_extension)?;
				// language exclude_dflt statements modify builder.features_;
				// limit them to this block with temporary builder.features_
				let features = std::mem::take(&mut self.features_);
				let result = self.build_statements(&b.statements);
				let block_features = std::mem::replace(&mut self.features_, features);
				result?;
				for (key, value) in block_features {
					self.features_.entry(key).or_default().extend(value);
				}
				self.end_feature()
			}
			Statement::VariationBlock(b) => {
				self.start_feature(&b.location, &b.name, b.use_extension)?;
				if b.conditionset != "NULL" && !self.conditionsets_.contains_key(&b.conditionset) {
					return Err(Error::feature_lib(
						format!(
							"variation block used undefined conditionset {}",
							b.conditionset
						),
						b.location.clone(),
					));
				}
				// language exclude_dflt statements modify builder.features_;
				// limit them to this block with temporary builder.features_
				let features = std::mem::take(&mut self.features_);
				let result = self.build_statements(&b.statements);
				let block_features = std::mem::replace(&mut self.features_, features);
				result?;
				for (key, value) in block_features {
					self.feature_variations_
						.entry(key.clone())
						.or_default()
						.entry(b.conditionset.clone())
						.or_default()
						.extend(value);
					// Ensure we make a feature record.
					self.features_.entry(key).or_default();
				}
				self.end_feature()
			}
			Statement::NestedBlock(b) => {
				self.build_statements(&b.statements)?;
				if b.block_name == "ParamUILabelNameID" {
					self.add_to_cv_num_named_params(&b.tag);
				}
				Ok(())
			}
			Statement::LookupBlock(b) => {
				self.start_lookup_block(&b.location, &b.name, b.use_extension)?;
				self.build_statements(&b.statements)?;
				self.end_lookup_block()
			}
			Statement::TableBlock(b) => self.build_statements(&b.statements),
			Statement::GlyphClassDefStatement(s) => self.add_glyph_class_def(
				&s.location,
				&opt_gs(&s.base_glyphs),
				&opt_gs(&s.ligature_glyphs),
				&opt_gs(&s.mark_glyphs),
				&opt_gs(&s.component_glyphs),
			),
			Statement::AlternateSubstStatement(s) => {
				let glyph = gs(&s.glyph);
				let [glyph] = glyph.as_slice() else {
					return Err(Error::Assertion(format!("{glyph:?}")));
				};
				let prefix = gss(&s.prefix);
				let suffix = gss(&s.suffix);
				let replacement = gs(&s.replacement);
				self.add_alternate_subst(&s.location, &prefix, glyph, &suffix, &replacement)
			}
			Statement::AttachStatement(s) => {
				self.add_attach_points(&s.location, &gs(&s.glyphs), &s.contour_points);
				Ok(())
			}
			Statement::ChainContextPosStatement(s) => self.add_chain_context_pos(
				&s.location,
				gss(&s.prefix),
				gss(&s.glyphs),
				gss(&s.suffix),
				&s.lookups,
			),
			Statement::ChainContextSubstStatement(s) => self.add_chain_context_subst(
				&s.location,
				gss(&s.prefix),
				gss(&s.glyphs),
				gss(&s.suffix),
				&s.lookups,
			),
			Statement::CursivePosStatement(s) => self.add_cursive_pos(
				&s.location,
				&gs(&s.glyphclass),
				s.entry_anchor.as_ref(),
				s.exit_anchor.as_ref(),
			),
			Statement::FeatureReferenceStatement(s) => {
				self.add_feature_reference(&s.location, &s.feature_name)
			}
			Statement::IgnorePosStatement(s) => {
				for (prefix, glyphs, suffix) in &s.chain_contexts {
					self.add_chain_context_pos(
						&s.location,
						gss(prefix),
						gss(glyphs),
						gss(suffix),
						&Vec::new(),
					)?;
				}
				Ok(())
			}
			Statement::IgnoreSubstStatement(s) => {
				for (prefix, glyphs, suffix) in &s.chain_contexts {
					self.add_chain_context_subst(
						&s.location,
						gss(prefix),
						gss(glyphs),
						gss(suffix),
						&Vec::new(),
					)?;
				}
				Ok(())
			}
			Statement::LanguageStatement(s) => {
				self.set_language(&s.location, &s.languages, s.include_default, s.required)
			}
			Statement::LanguageSystemStatement(s) => {
				self.add_language_system(&s.location, &s.script, &s.language)
			}
			Statement::FontRevisionStatement(s) => {
				self.set_font_revision(&s.location, s.revision);
				Ok(())
			}
			Statement::LigatureCaretByIndexStatement(s) => {
				// `set(self.carets)`
				let carets = py_int_set_order(s.carets.iter().copied());
				self.add_ligature_caret_by_index_(&s.location, &gs(&s.glyphs), &carets);
				Ok(())
			}
			Statement::LigatureCaretByPosStatement(s) => {
				// `set(self.carets)`: the numbers are deduplicated; variable
				// scalars are distinct objects. The carets are sorted by
				// coordinate when they are built.
				// LENIENT(L34): in CPython, the order of a number and a variable
				// scalar with the same default (which the stable sort keeps)
				// depends on the object's address; here the numbers come first.
				let ints = py_int_set_order(s.carets.iter().filter_map(|c| match c {
					ScalarValue::Int(i) => Some(*i),
					ScalarValue::Variable(_) => None,
				}));
				let mut carets: Vec<ScalarValue> = ints.into_iter().map(ScalarValue::Int).collect();
				carets.extend(
					s.carets
						.iter()
						.filter(|c| matches!(c, ScalarValue::Variable(_)))
						.cloned(),
				);
				self.add_ligature_caret_by_pos_(&s.location, &gs(&s.glyphs), &carets)
			}
			Statement::LigatureSubstStatement(s) => self.add_ligature_subst(
				&s.location,
				&gss(&s.prefix),
				&gss(&s.glyphs),
				&gss(&s.suffix),
				&s.replacement,
				s.force_chain,
			),
			Statement::LookupFlagStatement(s) => {
				let mark_attach = s.mark_attachment.as_ref().map(gs);
				let mark_filter = s.mark_filtering_set.as_ref().map(gs);
				self.set_lookup_flag(
					&s.location,
					s.value,
					mark_attach.as_deref(),
					mark_filter.as_deref(),
				)
			}
			Statement::LookupReferenceStatement(s) => self.add_lookup_call(&s.lookup.name),
			Statement::MarkBasePosStatement(s) => {
				self.add_mark_base_pos(&s.location, &gs(&s.base), &s.marks)
			}
			Statement::MarkLigPosStatement(s) => {
				self.add_mark_lig_pos(&s.location, &gs(&s.ligatures), &s.marks)
			}
			Statement::MarkMarkPosStatement(s) => {
				self.add_mark_mark_pos(&s.location, &gs(&s.base_marks), &s.marks)
			}
			Statement::MultipleSubstStatement(s) => self.build_multiple_subst(s),
			Statement::PairPosStatement(s) => self.build_pair_pos(s),
			Statement::ReverseChainSingleSubstStatement(s) => {
				let prefix = gss(&s.old_prefix);
				let suffix = gss(&s.old_suffix);
				let originals = gs(s.glyphs.first().ok_or_else(index_error)?);
				let replaces = gs(s.replacements.first().ok_or_else(index_error)?);
				self.add_reverse_chain_single_subst(
					&s.location,
					prefix,
					suffix,
					zip_mapping(&originals, &replaces),
				)
			}
			Statement::SingleSubstStatement(s) => {
				let prefix = gss(&s.prefix);
				let suffix = gss(&s.suffix);
				let originals = gs(s.glyphs.first().ok_or_else(index_error)?);
				let replaces = gs(s.replacements.first().ok_or_else(index_error)?);
				self.add_single_subst(
					&s.location,
					&prefix,
					&suffix,
					&zip_mapping(&originals, &replaces),
					s.force_chain,
				)
			}
			Statement::ScriptStatement(s) => self.set_script(&s.location, &s.script),
			Statement::SinglePosStatement(s) => {
				let prefix = gss(&s.prefix);
				let suffix = gss(&s.suffix);
				let pos: Vec<_> = s.pos.iter().map(|(g, v)| (gs(g), v.clone())).collect();
				self.add_single_pos(&s.location, &prefix, &suffix, &pos, s.force_chain)
			}
			Statement::SubtableStatement(s) => self.add_subtable_break(&s.location),
			Statement::NameRecord(s) => {
				self.names_.push(NameEntry {
					name_id: NameKey::Id(s.name_id),
					platform_id: s.platform_id,
					plat_enc_id: s.plat_enc_id,
					lang_id: s.lang_id,
					string: s.string.clone(),
				});
				Ok(())
			}
			Statement::FeatureNameStatement(s) => {
				self.names_.push(NameEntry {
					name_id: NameKey::Feature(s.name_id.clone()),
					platform_id: s.platform_id,
					plat_enc_id: s.plat_enc_id,
					lang_id: s.lang_id,
					string: s.string.clone(),
				});
				self.add_feature_name(&s.name_id);
				Ok(())
			}
			Statement::STATNameStatement(s) => {
				self.names_.push(NameEntry {
					name_id: NameKey::Feature(s.name_id.clone()),
					platform_id: s.platform_id,
					plat_enc_id: s.plat_enc_id,
					lang_id: s.lang_id,
					string: s.string.clone(),
				});
				Ok(())
			}
			Statement::CVParametersNameStatement(s) => {
				let mut item = String::new();
				if s.block_name == "ParamUILabelNameID" {
					item = format!(
						"_{}",
						self.cv_num_named_params_
							.get(&s.name_id)
							.copied()
							.unwrap_or(0)
					);
				}
				self.add_cv_parameter(&s.name_id);
				// LENIENT(L34): fontTools replaces the statement's nameID with the
				// pair here, so building the same tree twice nests the pairs;
				// the tree is not changed here.
				self.names_.push(NameEntry {
					name_id: NameKey::Cv(s.name_id.clone(), format!("{}{item}", s.block_name)),
					platform_id: s.platform_id,
					plat_enc_id: s.plat_enc_id,
					lang_id: s.lang_id,
					string: s.string.clone(),
				});
				Ok(())
			}
			Statement::CharacterStatement(s) => {
				self.add_cv_character(s.character, &s.tag);
				Ok(())
			}
			Statement::SizeParameters(s) => self.set_size_parameters(
				&s.location,
				s.design_size,
				s.subfamily_id,
				s.range_start,
				s.range_end,
			),
			Statement::BaseAxis(s) => {
				self.set_base_axis(
					s.bases.clone(),
					s.scripts.clone(),
					s.vertical,
					s.minmax.clone(),
				);
				Ok(())
			}
			Statement::OS2Field(s) => {
				self.add_os2_field(&s.key, s.value.clone());
				Ok(())
			}
			Statement::HheaField(s) => {
				self.add_hhea_field(&s.key, s.value);
				Ok(())
			}
			Statement::VheaField(s) => {
				self.add_vhea_field(&s.key, s.value);
				Ok(())
			}
			Statement::STATDesignAxisStatement(s) => self.add_design_axis(s, &s.location),
			Statement::ElidedFallbackName(s) => {
				self.set_elided_fallback_name(s.names.clone(), &s.location)
			}
			Statement::ElidedFallbackNameID(s) => {
				self.set_elided_fallback_name_id(s.value, &s.location)
			}
			Statement::STATAxisValueStatement(s) => self.add_axis_value_record(s, &s.location),
			Statement::ConditionsetStatement(s) => {
				self.add_conditionset(&s.location, &s.name, &s.conditions)
			}
		}
	}

	/// `MultipleSubstStatement.build`.
	fn build_multiple_subst(&mut self, s: &MultipleSubstStatement) -> Result<()> {
		let prefix = gss(&s.prefix);
		let suffix = gss(&s.suffix);
		let originals = gs(&s.glyph);
		let count = originals.len();
		let mut replaces: Vec<Vec<String>> = Vec::with_capacity(s.replacement.len());
		for r in &s.replacement {
			let mut replace = gs(r);
			if replace.len() == 1 && replace.len() != count {
				replace = vec![replace[0].clone(); count];
			}
			replaces.push(replace);
		}
		// `list(zip(*replaces))`
		let rows = replaces.iter().map(Vec::len).min().unwrap_or(0);
		let replaces: Vec<Vec<String>> = (0..rows)
			.map(|i| replaces.iter().map(|r| r[i].clone()).collect())
			.collect();
		let mut seen_originals = std::collections::HashSet::new();
		for (i, original) in originals.iter().enumerate() {
			if seen_originals.insert(original.clone()) {
				let replacement = if replaces.is_empty() {
					Vec::new()
				} else {
					replaces.get(i).cloned().ok_or_else(index_error)?
				};
				self.add_multiple_subst(
					&s.location,
					&prefix,
					original,
					&suffix,
					&replacement,
					s.force_chain,
				)?;
			}
		}
		Ok(())
	}

	/// `PairPosStatement.build`: enumerated pairs and pairs of single glyphs are
	/// glyph pairs, others are class pairs.
	fn build_pair_pos(&mut self, s: &PairPosStatement) -> Result<()> {
		if s.enumerated {
			let glyphs1 = gs(&s.glyphs1);
			let glyphs2 = gs(&s.glyphs2);
			let mut seen_pair = false;
			for glyph1 in &glyphs1 {
				for glyph2 in &glyphs2 {
					seen_pair = true;
					self.add_specific_pair_pos(
						&s.location,
						glyph1,
						s.valuerecord1.as_ref(),
						glyph2,
						s.valuerecord2.as_ref(),
					)?;
				}
			}
			if !seen_pair {
				return Err(Error::feature_lib(
					"Empty glyph class in positioning rule",
					s.location.clone(),
				));
			}
			return Ok(());
		}
		match (&s.glyphs1, &s.glyphs2) {
			(GlyphContainer::GlyphName(g1), GlyphContainer::GlyphName(g2)) => self
				.add_specific_pair_pos(
					&s.location,
					&g1.glyph,
					s.valuerecord1.as_ref(),
					&g2.glyph,
					s.valuerecord2.as_ref(),
				),
			_ => self.add_class_pair_pos(
				&s.location,
				&gs(&s.glyphs1),
				s.valuerecord1.as_ref(),
				&gs(&s.glyphs2),
				s.valuerecord2.as_ref(),
			),
		}
	}
}
