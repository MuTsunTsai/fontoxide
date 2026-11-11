//! The `test_Fea2feaFile_*` tests of `Tests/feaLib/builder_test.py`: each feature
//! file of `TEST_FEATURE_FILES` is parsed with the glyph names of the mock font
//! and written back with `asFea()`, and must match the file (normalized).

use std::path::PathBuf;

use fontoxide_fealib::Result;
use fontoxide_fealib::parser::Parser;

/// The glyph order of `makeTTFont()`.
fn glyph_order() -> Vec<String> {
	let mut glyphs: Vec<String> = "
		.notdef space slash fraction semicolon period comma ampersand
		quotedblleft quotedblright quoteleft quoteright
		zero one two three four five six seven eight nine
		zero.oldstyle one.oldstyle two.oldstyle three.oldstyle
		four.oldstyle five.oldstyle six.oldstyle seven.oldstyle
		eight.oldstyle nine.oldstyle onequarter onehalf threequarters
		onesuperior twosuperior threesuperior ordfeminine ordmasculine
		A B C D E F G H I J K L M N O P Q R S T U V W X Y Z
		a b c d e f g h i j k l m n o p q r s t u v w x y z
		A.sc B.sc C.sc D.sc E.sc F.sc G.sc H.sc I.sc J.sc K.sc L.sc M.sc
		N.sc O.sc P.sc Q.sc R.sc S.sc T.sc U.sc V.sc W.sc X.sc Y.sc Z.sc
		A.alt1 A.alt2 A.alt3 B.alt1 B.alt2 B.alt3 C.alt1 C.alt2 C.alt3
		a.alt1 a.alt2 a.alt3 a.end b.alt c.mid d.alt d.mid
		e.begin e.mid e.end m.begin n.end s.end z.end
		Eng Eng.alt1 Eng.alt2 Eng.alt3
		A.swash B.swash C.swash D.swash E.swash F.swash G.swash H.swash
		I.swash J.swash K.swash L.swash M.swash N.swash O.swash P.swash
		Q.swash R.swash S.swash T.swash U.swash V.swash W.swash X.swash
		Y.swash Z.swash
		f_l c_h c_k c_s c_t f_f f_f_i f_f_l f_i o_f_f_i s_t f_i.begin
		a_n_d T_h T_h.swash germandbls ydieresis yacute breve
		grave acute dieresis macron circumflex cedilla umlaut ogonek caron
		damma hamza sukun kasratan lam_meem_jeem noon.final noon.initial
		by feature lookup sub table uni0327 uni0328 e.fina
		idotaccent idotbelow idotless iogonek acutecomb brevecomb ogonekcomb
		dotbelowcomb
	"
	.split_whitespace()
	.map(str::to_owned)
	.collect();
	glyphs.extend((800..=1001).map(|cid| format!("cid{cid:05}")));
	glyphs
}

fn getpath(testfile: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/feaLib/data")
		.join(testfile)
}

/// `normal_fea(lines)`.
fn normal_fea<S: AsRef<str>>(lines: &[S]) -> Vec<String> {
	let mut output = Vec::new();
	let mut skip = 0;
	for l in lines {
		let mut l = l.as_ref().trim().to_owned();
		if l.starts_with("#test-fea2fea:") {
			if l.chars().count() > 15 {
				output.push(l.chars().skip(15).collect::<String>().trim().to_owned());
			}
			skip = 1;
		}
		if let Some(x) = l.find('#') {
			l = l[..x].trim().to_owned();
		}
		if l.is_empty() {
			continue;
		}
		if skip > 0 {
			skip -= 1;
			continue;
		}
		output.push(l);
	}
	output
}

/// `check_fea2fea_file(name)`.
fn check_fea2fea_file(name: &str) -> Result<()> {
	let fname = if name.contains('.') {
		name.to_owned()
	} else {
		format!("{name}.fea")
	};
	let doc = Parser::new(getpath(&fname), glyph_order())?.parse()?;
	let fea = doc.as_fea()?;
	let actual = normal_fea(&fea.split('\n').collect::<Vec<_>>());
	let text =
		std::fs::read_to_string(getpath(&fname)).map_err(|e| fontoxide_fealib::Error::Io {
			path: fname.clone(),
			source: e,
		})?;
	let expected = normal_fea(&text.lines().collect::<Vec<_>>());
	assert_eq!(
		expected,
		actual,
		"Fea2Fea output is different from expected. Generated:\n{}\n",
		actual.join("\n")
	);
	Ok(())
}

macro_rules! fea2fea_tests {
	($($test:ident => $name:literal,)*) => {
		$(
			#[test]
			fn $test() -> Result<()> {
				check_fea2fea_file($name)
			}
		)*
	};
}

fea2fea_tests! {
	test_fea2fea_file_attach => "Attach",
	test_fea2fea_file_cid_range => "cid_range",
	test_fea2fea_file_enum => "enum",
	test_fea2fea_file_mark_class => "markClass",
	test_fea2fea_file_language_required => "language_required",
	test_fea2fea_file_glyph_class_def => "GlyphClassDef",
	test_fea2fea_file_ligature_caret_by_index => "LigatureCaretByIndex",
	test_fea2fea_file_ligature_caret_by_pos => "LigatureCaretByPos",
	test_fea2fea_file_lookup => "lookup",
	test_fea2fea_file_lookupflag => "lookupflag",
	test_fea2fea_file_feature_aalt => "feature_aalt",
	test_fea2fea_file_ignore_pos => "ignore_pos",
	test_fea2fea_file_gpos_1 => "GPOS_1",
	test_fea2fea_file_gpos_1_zero => "GPOS_1_zero",
	test_fea2fea_file_gpos_2 => "GPOS_2",
	test_fea2fea_file_gpos_2b => "GPOS_2b",
	test_fea2fea_file_gpos_3 => "GPOS_3",
	test_fea2fea_file_gpos_4 => "GPOS_4",
	test_fea2fea_file_gpos_5 => "GPOS_5",
	test_fea2fea_file_gpos_6 => "GPOS_6",
	test_fea2fea_file_gpos_8 => "GPOS_8",
	test_fea2fea_file_gsub_2 => "GSUB_2",
	test_fea2fea_file_gsub_3 => "GSUB_3",
	test_fea2fea_file_gsub_6 => "GSUB_6",
	test_fea2fea_file_gsub_8 => "GSUB_8",
	test_fea2fea_file_spec4h1 => "spec4h1",
	test_fea2fea_file_spec4h2 => "spec4h2",
	test_fea2fea_file_spec5d1 => "spec5d1",
	test_fea2fea_file_spec5d2 => "spec5d2",
	test_fea2fea_file_spec5fi1 => "spec5fi1",
	test_fea2fea_file_spec5fi2 => "spec5fi2",
	test_fea2fea_file_spec5fi3 => "spec5fi3",
	test_fea2fea_file_spec5fi4 => "spec5fi4",
	test_fea2fea_file_spec5f_ii_1 => "spec5f_ii_1",
	test_fea2fea_file_spec5f_ii_2 => "spec5f_ii_2",
	test_fea2fea_file_spec5f_ii_3 => "spec5f_ii_3",
	test_fea2fea_file_spec5f_ii_4 => "spec5f_ii_4",
	test_fea2fea_file_spec5h1 => "spec5h1",
	test_fea2fea_file_spec6b_ii => "spec6b_ii",
	test_fea2fea_file_spec6d2 => "spec6d2",
	test_fea2fea_file_spec6e => "spec6e",
	test_fea2fea_file_spec6f => "spec6f",
	test_fea2fea_file_spec6h_ii => "spec6h_ii",
	test_fea2fea_file_spec6h_iii_1 => "spec6h_iii_1",
	test_fea2fea_file_spec6h_iii_3d => "spec6h_iii_3d",
	test_fea2fea_file_spec8a => "spec8a",
	test_fea2fea_file_spec8a_2 => "spec8a_2",
	test_fea2fea_file_spec8b => "spec8b",
	test_fea2fea_file_spec8c => "spec8c",
	test_fea2fea_file_spec8d => "spec8d",
	test_fea2fea_file_spec9a => "spec9a",
	test_fea2fea_file_spec9a2 => "spec9a2",
	test_fea2fea_file_spec9b => "spec9b",
	test_fea2fea_file_spec9c1 => "spec9c1",
	test_fea2fea_file_spec9c2 => "spec9c2",
	test_fea2fea_file_spec9c3 => "spec9c3",
	test_fea2fea_file_spec9d => "spec9d",
	test_fea2fea_file_spec9e => "spec9e",
	test_fea2fea_file_spec9f => "spec9f",
	test_fea2fea_file_spec9g => "spec9g",
	test_fea2fea_file_spec10 => "spec10",
	test_fea2fea_file_bug453 => "bug453",
	test_fea2fea_file_bug457 => "bug457",
	test_fea2fea_file_bug463 => "bug463",
	test_fea2fea_file_bug501 => "bug501",
	test_fea2fea_file_bug502 => "bug502",
	test_fea2fea_file_bug504 => "bug504",
	test_fea2fea_file_bug505 => "bug505",
	test_fea2fea_file_bug506 => "bug506",
	test_fea2fea_file_bug509 => "bug509",
	test_fea2fea_file_bug512 => "bug512",
	test_fea2fea_file_bug514 => "bug514",
	test_fea2fea_file_bug568 => "bug568",
	test_fea2fea_file_bug633 => "bug633",
	test_fea2fea_file_bug1196 => "bug1196",
	test_fea2fea_file_bug1307 => "bug1307",
	test_fea2fea_file_bug1459 => "bug1459",
	test_fea2fea_file_bug2276 => "bug2276",
	test_fea2fea_file_variable_bug2772 => "variable_bug2772",
	test_fea2fea_file_name => "name",
	test_fea2fea_file_size => "size",
	test_fea2fea_file_size2 => "size2",
	test_fea2fea_file_multiple_feature_blocks => "multiple_feature_blocks",
	test_fea2fea_file_omitted_glyph_class_def => "omitted_GlyphClassDef",
	test_fea2fea_file_zero_value_single_pos_horizontal => "ZeroValue_SinglePos_horizontal",
	test_fea2fea_file_zero_value_single_pos_vertical => "ZeroValue_SinglePos_vertical",
	test_fea2fea_file_zero_value_pair_pos_horizontal => "ZeroValue_PairPos_horizontal",
	test_fea2fea_file_zero_value_pair_pos_vertical => "ZeroValue_PairPos_vertical",
	test_fea2fea_file_zero_value_chain_single_pos_horizontal => "ZeroValue_ChainSinglePos_horizontal",
	test_fea2fea_file_zero_value_chain_single_pos_vertical => "ZeroValue_ChainSinglePos_vertical",
	test_fea2fea_file_pair_pos_subtable => "PairPosSubtable",
	test_fea2fea_file_chain_subst_subtable => "ChainSubstSubtable",
	test_fea2fea_file_subst_subtable => "SubstSubtable",
	test_fea2fea_file_chain_pos_subtable => "ChainPosSubtable",
	test_fea2fea_file_ligature_subtable => "LigatureSubtable",
	test_fea2fea_file_alternate_subtable => "AlternateSubtable",
	test_fea2fea_file_multiple_subst_subtable => "MultipleSubstSubtable",
	test_fea2fea_file_single_subst_subtable => "SingleSubstSubtable",
	test_fea2fea_file_aalt_chain_contextual_subst => "aalt_chain_contextual_subst",
	test_fea2fea_file_alternate_chained => "AlternateChained",
	test_fea2fea_file_multiple_lookups_per_glyph => "MultipleLookupsPerGlyph",
	test_fea2fea_file_multiple_lookups_per_glyph2 => "MultipleLookupsPerGlyph2",
	test_fea2fea_file_gsub_6_formats => "GSUB_6_formats",
	test_fea2fea_file_gsub_5_formats => "GSUB_5_formats",
	test_fea2fea_file_delete_glyph => "delete_glyph",
	test_fea2fea_file_stat_test => "STAT_test",
	test_fea2fea_file_stat_test_elided_fallback_name_id => "STAT_test_elidedFallbackNameID",
	test_fea2fea_file_variable_scalar_valuerecord => "variable_scalar_valuerecord",
	test_fea2fea_file_variable_scalar_anchor => "variable_scalar_anchor",
	test_fea2fea_file_variable_conditionset => "variable_conditionset",
	test_fea2fea_file_variable_mark_anchor => "variable_mark_anchor",
	test_fea2fea_file_duplicate_lookup_reference => "duplicate_lookup_reference",
	test_fea2fea_file_contextual_inline_multi_sub_format_2 => "contextual_inline_multi_sub_format_2",
	test_fea2fea_file_contextual_inline_format_4 => "contextual_inline_format_4",
	test_fea2fea_file_chain_context_multi_subst_class => "chain_context_multi_subst_class",
	test_fea2fea_file_duplicate_language_stmt => "duplicate_language_stmt",
	test_fea2fea_file_duplicate_language_stmt_include_dflt => "duplicate_language_stmt_include_dflt",
	test_fea2fea_file_script_language_tracking => "script_language_tracking",
	test_fea2fea_file_script_language_tracking_dflt => "script_language_tracking_DFLT",
	test_fea2fea_file_script_language_tracking_multi => "script_language_tracking_multi",
	test_fea2fea_file_script_language_tracking_redundant => "script_language_tracking_redundant",
	test_fea2fea_file_cursive_pos_subtable => "CursivePosSubtable",
	test_fea2fea_file_mark_base_pos_subtable => "MarkBasePosSubtable",
	test_fea2fea_file_mark_lig_pos_subtable => "MarkLigPosSubtable",
	test_fea2fea_file_mark_mark_pos_subtable => "MarkMarkPosSubtable",
	test_fea2fea_file_single_pos_null => "single_pos_NULL",
	test_fea2fea_file_class_pair_pos_duplicates => "class_pair_pos_duplicates",
	test_fea2fea_file_use_extension => "useExtension",
	test_fea2fea_file_bug3846_1 => "bug3846_1",
	test_fea2fea_file_bug3846_2 => "bug3846_2",
	test_fea2fea_file_empty_filter_sets_and_mark_classes => "empty_filter_sets_and_mark_classes",
	test_fea2fea_file_combo_mult_and_lig_sub => "combo_mult_and_lig_sub",
	test_fea2fea_file_identical_feature_lookups => "identical_feature_lookups",
	test_fea2fea_file_cvparam_null => "cvparam_null",
	test_fea2fea_file_contextual_merge_gsub_named => "contextual_merge_gsub_named",
	test_fea2fea_file_contextual_merge_gpos_named => "contextual_merge_gpos_named",
	test_fea2fea_file_contextual_merge_ignore => "contextual_merge_ignore",
	test_fea2fea_file_contextual_merge_alternate => "contextual_merge_alternate",
	// The fea2fea half of `test_unmarked_ignore_statement`.
	test_fea2fea_file_bug2949 => "bug2949",
}
