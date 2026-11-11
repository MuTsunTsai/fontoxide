//! `Tests/voltLib/parser_test.py`.

use fontoxide_voltlib::ast::{
	AnchorDefinition, CoverageItem, GlyphDefinition, LookupDefinition, Pos, PositionDefinition,
	ProcessMarks, SettingValue, Statement, SubstitutionDefinition, SubstitutionKind, VoltFile,
};
use fontoxide_voltlib::{Parser, ParserInput, Result};
use indexmap::IndexMap;

fn parse_(text: &str) -> Result<VoltFile> {
	Parser::new(ParserInput::text(text))?.parse()
}

/// Parses `text`, checking that the statements write back to it.
fn parse(text: &str) -> Result<VoltFile> {
	let doc = parse_(text)?;
	let written: Vec<String> = doc.statements.iter().map(Statement::to_string).collect();
	assert_eq!(written.join("\n"), text);
	parse_(text)
}

/// The message of the error that parsing `text` fails with.
fn parse_err(text: &str) -> String {
	match parse_(text) {
		Ok(doc) => panic!("parsed: {doc}"),
		Err(e) => e.to_string(),
	}
}

macro_rules! get {
	($s:expr, $v:ident) => {
		match &$s {
			Statement::$v(x) => x,
			s => panic!("unexpected statement {s:?}"),
		}
	};
}

fn glyph(item: &CoverageItem) -> &str {
	match item {
		CoverageItem::GlyphName(g) => &g.glyph,
		other => panic!("not a glyph: {other:?}"),
	}
}

fn group(item: &CoverageItem) -> &str {
	match item {
		CoverageItem::GroupName(g) => &g.group,
		other => panic!("not a group: {other:?}"),
	}
}

fn glyphs(coverage: &[CoverageItem]) -> Vec<&str> {
	coverage.iter().map(glyph).collect()
}

fn single(s: &[Statement]) -> &Statement {
	assert_eq!(s.len(), 1);
	&s[0]
}

fn glyph_tuple(g: &GlyphDefinition) -> (&str, i64, Option<Vec<i64>>, Option<&str>, Option<i64>) {
	(
		g.name.as_str(),
		g.id,
		g.unicode.clone(),
		g.glyph_type.as_deref(),
		g.components,
	)
}

fn anchor_tuple(a: &AnchorDefinition) -> (&str, i64, &str, i64, bool, Pos) {
	(
		a.name.as_str(),
		a.gid,
		a.glyph_name.as_str(),
		a.component,
		a.locked,
		a.pos.clone(),
	)
}

fn mkpos(adv: Option<i64>, dx: Option<i64>, dy: Option<i64>) -> Pos {
	Pos {
		adv,
		dx,
		dy,
		..Default::default()
	}
}

fn lookup(doc: &VoltFile, i: usize) -> &LookupDefinition {
	get!(doc.statements[i], LookupDefinition)
}

fn sub(lookup: &LookupDefinition) -> &SubstitutionDefinition {
	lookup.sub.as_ref().expect("a substitution")
}

/// `assertSubEqual`.
fn assert_sub_equal(
	sub: &SubstitutionDefinition,
	glyph_ref: &[&[&str]],
	replacement_ref: &[&[&str]],
) {
	let sources: Vec<Vec<&str>> = sub.mapping.iter().map(|(k, _)| glyphs(k)).collect();
	let replacement: Vec<Vec<&str>> = sub.mapping.iter().map(|(_, v)| glyphs(v)).collect();
	assert_eq!(sources, glyph_ref);
	assert_eq!(replacement, replacement_ref);
}

#[test]
fn test_def_glyph_base() -> Result<()> {
	let doc = parse(r#"DEF_GLYPH ".notdef" ID 0 TYPE BASE END_GLYPH"#)?;
	let def_glyph = get!(single(&doc.statements), GlyphDefinition);
	assert_eq!(
		glyph_tuple(def_glyph),
		(".notdef", 0, None, Some("BASE"), None)
	);
	Ok(())
}

#[test]
fn test_def_glyph_base_with_unicode() -> Result<()> {
	let doc = parse(r#"DEF_GLYPH "space" ID 3 UNICODE 32 TYPE BASE END_GLYPH"#)?;
	let def_glyph = get!(single(&doc.statements), GlyphDefinition);
	assert_eq!(
		glyph_tuple(def_glyph),
		("space", 3, Some(vec![0x0020]), Some("BASE"), None)
	);
	Ok(())
}

#[test]
fn test_def_glyph_base_with_unicodevalues() -> Result<()> {
	let doc = parse_(r#"DEF_GLYPH "CR" ID 2 UNICODEVALUES "U+0009" TYPE BASE END_GLYPH"#)?;
	let def_glyph = get!(single(&doc.statements), GlyphDefinition);
	assert_eq!(
		glyph_tuple(def_glyph),
		("CR", 2, Some(vec![0x0009]), Some("BASE"), None)
	);
	Ok(())
}

#[test]
fn test_def_glyph_base_with_mult_unicodevalues() -> Result<()> {
	let doc = parse(r#"DEF_GLYPH "CR" ID 2 UNICODEVALUES "U+0009,U+000D" TYPE BASE END_GLYPH"#)?;
	let def_glyph = get!(single(&doc.statements), GlyphDefinition);
	assert_eq!(
		glyph_tuple(def_glyph),
		("CR", 2, Some(vec![0x0009, 0x000D]), Some("BASE"), None)
	);
	Ok(())
}

#[test]
fn test_def_glyph_base_with_empty_unicodevalues() -> Result<()> {
	let doc = parse_(r#"DEF_GLYPH "i.locl" ID 269 UNICODEVALUES "" TYPE BASE END_GLYPH"#)?;
	let def_glyph = get!(single(&doc.statements), GlyphDefinition);
	assert_eq!(
		glyph_tuple(def_glyph),
		("i.locl", 269, None, Some("BASE"), None)
	);
	Ok(())
}

#[test]
fn test_def_glyph_base_2_components() -> Result<()> {
	let doc = parse(r#"DEF_GLYPH "glyphBase" ID 320 TYPE BASE COMPONENTS 2 END_GLYPH"#)?;
	let def_glyph = get!(single(&doc.statements), GlyphDefinition);
	assert_eq!(
		glyph_tuple(def_glyph),
		("glyphBase", 320, None, Some("BASE"), Some(2))
	);
	Ok(())
}

#[test]
fn test_def_glyph_ligature_2_components() -> Result<()> {
	let doc = parse(r#"DEF_GLYPH "f_f" ID 320 TYPE LIGATURE COMPONENTS 2 END_GLYPH"#)?;
	let def_glyph = get!(single(&doc.statements), GlyphDefinition);
	assert_eq!(
		glyph_tuple(def_glyph),
		("f_f", 320, None, Some("LIGATURE"), Some(2))
	);
	Ok(())
}

#[test]
fn test_def_glyph_mark() -> Result<()> {
	let doc = parse(r#"DEF_GLYPH "brevecomb" ID 320 TYPE MARK END_GLYPH"#)?;
	let def_glyph = get!(single(&doc.statements), GlyphDefinition);
	assert_eq!(
		glyph_tuple(def_glyph),
		("brevecomb", 320, None, Some("MARK"), None)
	);
	Ok(())
}

#[test]
fn test_def_glyph_component() -> Result<()> {
	let doc = parse(r#"DEF_GLYPH "f.f_f" ID 320 TYPE COMPONENT END_GLYPH"#)?;
	let def_glyph = get!(single(&doc.statements), GlyphDefinition);
	assert_eq!(
		glyph_tuple(def_glyph),
		("f.f_f", 320, None, Some("COMPONENT"), None)
	);
	Ok(())
}

#[test]
fn test_def_glyph_no_type() -> Result<()> {
	let doc = parse(r#"DEF_GLYPH "glyph20" ID 20 END_GLYPH"#)?;
	let def_glyph = get!(single(&doc.statements), GlyphDefinition);
	assert_eq!(glyph_tuple(def_glyph), ("glyph20", 20, None, None, None));
	Ok(())
}

#[test]
fn test_def_glyph_case_sensitive() -> Result<()> {
	let doc = parse(
		r#"DEF_GLYPH "A" ID 3 UNICODE 65 TYPE BASE END_GLYPH
DEF_GLYPH "a" ID 4 UNICODE 97 TYPE BASE END_GLYPH"#,
	)?;
	let def_glyphs = &doc.statements;
	assert_eq!(
		glyph_tuple(get!(def_glyphs[0], GlyphDefinition)),
		("A", 3, Some(vec![0x41]), Some("BASE"), None)
	);
	assert_eq!(
		glyph_tuple(get!(def_glyphs[1], GlyphDefinition)),
		("a", 4, Some(vec![0x61]), Some("BASE"), None)
	);
	Ok(())
}

#[test]
fn test_def_group_glyphs() -> Result<()> {
	let doc = parse(
		r#"DEF_GROUP "aaccented"
 ENUM GLYPH "aacute" GLYPH "abreve" GLYPH "acircumflex" GLYPH "adieresis" GLYPH "ae" GLYPH "agrave" GLYPH "amacron" GLYPH "aogonek" GLYPH "aring" GLYPH "atilde" END_ENUM
END_GROUP"#,
	)?;
	let def_group = get!(single(&doc.statements), GroupDefinition);
	assert_eq!(def_group.name, "aaccented");
	assert_eq!(
		def_group.enum_.as_ref().expect("enum").glyph_set()?,
		[
			"aacute",
			"abreve",
			"acircumflex",
			"adieresis",
			"ae",
			"agrave",
			"amacron",
			"aogonek",
			"aring",
			"atilde",
		]
	);
	Ok(())
}

#[test]
fn test_def_group_groups() -> Result<()> {
	let doc = parse(
		r#"DEF_GROUP "Group1"
 ENUM GLYPH "a" GLYPH "b" GLYPH "c" GLYPH "d" END_ENUM
END_GROUP
DEF_GROUP "Group2"
 ENUM GLYPH "e" GLYPH "f" GLYPH "g" GLYPH "h" END_ENUM
END_GROUP
DEF_GROUP "TestGroup"
 ENUM GROUP "Group1" GROUP "Group2" END_ENUM
END_GROUP"#,
	)?;
	assert_eq!(doc.statements.len(), 3);
	let test_group = get!(doc.statements[2], GroupDefinition);
	let groups: Vec<&str> = test_group
		.enum_
		.as_ref()
		.expect("enum")
		.enum_
		.iter()
		.map(group)
		.collect();
	assert_eq!(
		(test_group.name.as_str(), groups),
		("TestGroup", vec!["Group1", "Group2"])
	);
	Ok(())
}

#[test]
fn test_def_group_groups_not_yet_defined() -> Result<()> {
	let doc = parse(
		r#"DEF_GROUP "Group1"
 ENUM GLYPH "a" GLYPH "b" GLYPH "c" GLYPH "d" END_ENUM
END_GROUP
DEF_GROUP "TestGroup1"
 ENUM GROUP "Group1" GROUP "Group2" END_ENUM
END_GROUP
DEF_GROUP "TestGroup2"
 ENUM GROUP "Group2" END_ENUM
END_GROUP
DEF_GROUP "TestGroup3"
 ENUM GROUP "Group2" GROUP "Group1" END_ENUM
END_GROUP
DEF_GROUP "Group2"
 ENUM GLYPH "e" GLYPH "f" GLYPH "g" GLYPH "h" END_ENUM
END_GROUP"#,
	)?;
	assert_eq!(doc.statements.len(), 5);
	let groups_of = |i: usize| {
		let g = get!(doc.statements[i], GroupDefinition);
		let groups: Vec<&str> = g
			.enum_
			.as_ref()
			.expect("enum")
			.enum_
			.iter()
			.map(group)
			.collect();
		(g.name.as_str(), groups)
	};
	assert_eq!(groups_of(1), ("TestGroup1", vec!["Group1", "Group2"]));
	assert_eq!(groups_of(2), ("TestGroup2", vec!["Group2"]));
	assert_eq!(groups_of(3), ("TestGroup3", vec!["Group2", "Group1"]));
	Ok(())
}

#[test]
fn test_def_group_glyphs_and_group() -> Result<()> {
	let doc = parse(
		r#"DEF_GROUP "aaccented"
 ENUM GLYPH "aacute" GLYPH "abreve" GLYPH "acircumflex" GLYPH "adieresis" GLYPH "ae" GLYPH "agrave" GLYPH "amacron" GLYPH "aogonek" GLYPH "aring" GLYPH "atilde" END_ENUM
END_GROUP
DEF_GROUP "KERN_lc_a_2ND"
 ENUM GLYPH "a" GROUP "aaccented" END_ENUM
END_GROUP"#,
	)?;
	assert_eq!(doc.statements.len(), 2);
	let def_group2 = get!(doc.statements[1], GroupDefinition);
	let items = &def_group2.enum_.as_ref().expect("enum").enum_;
	assert_eq!(
		(
			def_group2.name.as_str(),
			items[0].glyph_set()?,
			group(&items[1])
		),
		("KERN_lc_a_2ND", vec!["a".to_owned()], "aaccented")
	);
	Ok(())
}

#[test]
fn test_def_group_range() -> Result<()> {
	let doc = parse(
		r#"DEF_GLYPH "a" ID 163 UNICODE 97 TYPE BASE END_GLYPH
DEF_GLYPH "agrave" ID 194 UNICODE 224 TYPE BASE END_GLYPH
DEF_GLYPH "aacute" ID 195 UNICODE 225 TYPE BASE END_GLYPH
DEF_GLYPH "acircumflex" ID 196 UNICODE 226 TYPE BASE END_GLYPH
DEF_GLYPH "atilde" ID 197 UNICODE 227 TYPE BASE END_GLYPH
DEF_GLYPH "c" ID 165 UNICODE 99 TYPE BASE END_GLYPH
DEF_GLYPH "ccaron" ID 209 UNICODE 269 TYPE BASE END_GLYPH
DEF_GLYPH "ccedilla" ID 210 UNICODE 231 TYPE BASE END_GLYPH
DEF_GLYPH "cdotaccent" ID 210 UNICODE 267 TYPE BASE END_GLYPH
DEF_GROUP "KERN_lc_a_2ND"
 ENUM RANGE "a" TO "atilde" GLYPH "b" RANGE "c" TO "cdotaccent" END_ENUM
END_GROUP"#,
	)?;
	let def_group = get!(doc.statements[doc.statements.len() - 1], GroupDefinition);
	assert_eq!(def_group.name, "KERN_lc_a_2ND");
	assert_eq!(
		def_group.enum_.as_ref().expect("enum").glyph_set()?,
		[
			"a",
			"agrave",
			"aacute",
			"acircumflex",
			"atilde",
			"b",
			"c",
			"ccaron",
			"ccedilla",
			"cdotaccent",
		]
	);
	Ok(())
}

#[test]
fn test_group_duplicate() {
	assert!(
		parse_err(
			r#"DEF_GROUP "dupe"
ENUM GLYPH "a" GLYPH "b" END_ENUM
END_GROUP
DEF_GROUP "dupe"
ENUM GLYPH "x" END_ENUM
END_GROUP"#
		)
		.contains(r#"Glyph group "dupe" already defined, group names are case insensitive"#)
	);
}

#[test]
fn test_group_duplicate_case_insensitive() {
	assert!(
		parse_err(
			r#"DEF_GROUP "dupe"
ENUM GLYPH "a" GLYPH "b" END_ENUM
END_GROUP
DEF_GROUP "Dupe"
ENUM GLYPH "x" END_ENUM
END_GROUP"#
		)
		.contains(r#"Glyph group "Dupe" already defined, group names are case insensitive"#)
	);
}

#[test]
fn test_script_without_langsys() -> Result<()> {
	let doc = parse(
		r#"DEF_SCRIPT NAME "Latin" TAG "latn"

END_SCRIPT"#,
	)?;
	let script = get!(single(&doc.statements), ScriptDefinition);
	assert_eq!(script.name.as_deref(), Some("Latin"));
	assert_eq!(script.tag, "latn");
	assert!(script.langs.is_empty());
	Ok(())
}

#[test]
fn test_langsys_normal() -> Result<()> {
	let doc = parse(
		r#"DEF_SCRIPT NAME "Latin" TAG "latn"

DEF_LANGSYS NAME "Romanian" TAG "ROM "

END_LANGSYS
DEF_LANGSYS NAME "Moldavian" TAG "MOL "

END_LANGSYS
END_SCRIPT"#,
	)?;
	let def_script = get!(single(&doc.statements), ScriptDefinition);
	assert_eq!(
		(def_script.name.as_deref(), def_script.tag.as_str()),
		(Some("Latin"), "latn")
	);
	let def_lang = &def_script.langs[0];
	assert_eq!(
		(def_lang.name.as_deref(), def_lang.tag.as_str()),
		(Some("Romanian"), "ROM ")
	);
	let def_lang = &def_script.langs[1];
	assert_eq!(
		(def_lang.name.as_deref(), def_lang.tag.as_str()),
		(Some("Moldavian"), "MOL ")
	);
	Ok(())
}

#[test]
fn test_langsys_no_script_name() -> Result<()> {
	let doc = parse(
		r#"DEF_SCRIPT TAG "latn"

DEF_LANGSYS NAME "Default" TAG "dflt"

END_LANGSYS
END_SCRIPT"#,
	)?;
	let langsys = get!(single(&doc.statements), ScriptDefinition);
	assert_eq!(
		(langsys.name.as_deref(), langsys.tag.as_str()),
		(None, "latn")
	);
	let lang = &langsys.langs[0];
	assert_eq!(
		(lang.name.as_deref(), lang.tag.as_str()),
		(Some("Default"), "dflt")
	);
	Ok(())
}

#[test]
fn test_langsys_no_script_tag_fails() {
	assert!(
		parse_err(
			r#"DEF_SCRIPT NAME "Latin"

DEF_LANGSYS NAME "Default" TAG "dflt"

END_LANGSYS
END_SCRIPT"#
		)
		.contains(r#"Expected "TAG""#)
	);
}

#[test]
fn test_langsys_duplicate_script() {
	assert!(
		parse_err(
			r#"DEF_SCRIPT NAME "Default" TAG "DFLT"

DEF_LANGSYS NAME "Default" TAG "dflt"

END_LANGSYS
END_SCRIPT
DEF_SCRIPT TAG "DFLT"

DEF_LANGSYS NAME "Default" TAG "dflt"

END_LANGSYS
END_SCRIPT"#
		)
		.contains(r#"Script "DFLT" already defined, script tags are case insensitive"#)
	);
}

#[test]
fn test_langsys_duplicate_lang() {
	assert!(
		parse_err(
			r#"DEF_SCRIPT NAME "Default" TAG "DFLT"
DEF_LANGSYS NAME "Default" TAG "dflt"
END_LANGSYS
DEF_LANGSYS NAME "Default" TAG "dflt"
END_LANGSYS
END_SCRIPT"#
		)
		.contains(
			r#"Language "dflt" already defined in script "DFLT", language tags are case insensitive"#
		)
	);
}

#[test]
fn test_langsys_lang_in_separate_scripts() -> Result<()> {
	let doc = parse(
		r#"DEF_SCRIPT NAME "Default" TAG "DFLT"

DEF_LANGSYS NAME "Default" TAG "dflt"

END_LANGSYS
DEF_LANGSYS NAME "Default" TAG "ROM "

END_LANGSYS
END_SCRIPT
DEF_SCRIPT NAME "Latin" TAG "latn"

DEF_LANGSYS NAME "Default" TAG "dflt"

END_LANGSYS
DEF_LANGSYS NAME "Default" TAG "ROM "

END_LANGSYS
END_SCRIPT"#,
	)?;
	assert_eq!(doc.statements.len(), 2);
	for s in &doc.statements {
		let langsys = get!(s, ScriptDefinition);
		assert_eq!(
			(langsys.langs[0].tag.as_str(), langsys.langs[1].tag.as_str()),
			("dflt", "ROM ")
		);
	}
	Ok(())
}

#[test]
fn test_langsys_no_lang_name() -> Result<()> {
	let doc = parse(
		r#"DEF_SCRIPT NAME "Latin" TAG "latn"

DEF_LANGSYS TAG "dflt"

END_LANGSYS
END_SCRIPT"#,
	)?;
	let langsys = get!(single(&doc.statements), ScriptDefinition);
	assert_eq!(
		(langsys.name.as_deref(), langsys.tag.as_str()),
		(Some("Latin"), "latn")
	);
	let lang = &langsys.langs[0];
	assert_eq!((lang.name.as_deref(), lang.tag.as_str()), (None, "dflt"));
	Ok(())
}

#[test]
fn test_langsys_no_langsys_tag_fails() {
	assert!(
		parse_err(
			r#"DEF_SCRIPT NAME "Latin" TAG "latn"

DEF_LANGSYS NAME "Default"

END_LANGSYS
END_SCRIPT"#
		)
		.contains(r#"Expected "TAG""#)
	);
}

#[test]
fn test_feature() -> Result<()> {
	let doc = parse(
		r#"DEF_SCRIPT NAME "Latin" TAG "latn"

DEF_LANGSYS NAME "Romanian" TAG "ROM "

DEF_FEATURE NAME "Fractions" TAG "frac"
 LOOKUP "fraclookup"
END_FEATURE
END_LANGSYS
END_SCRIPT"#,
	)?;
	let def_script = get!(single(&doc.statements), ScriptDefinition);
	let def_feature = &def_script.langs[0].features[0];
	assert_eq!(
		(
			def_feature.name.as_str(),
			def_feature.tag.as_str(),
			def_feature.lookups.clone()
		),
		("Fractions", "frac", vec!["fraclookup".to_owned()])
	);
	let doc = parse(
		r#"DEF_SCRIPT NAME "Latin" TAG "latn"

DEF_LANGSYS NAME "Romanian" TAG "ROM "

DEF_FEATURE NAME "Kerning" TAG "kern"
 LOOKUP "kern1" LOOKUP "kern2"
END_FEATURE
END_LANGSYS
END_SCRIPT"#,
	)?;
	let def_script = get!(single(&doc.statements), ScriptDefinition);
	let def_feature = &def_script.langs[0].features[0];
	assert_eq!(
		(
			def_feature.name.as_str(),
			def_feature.tag.as_str(),
			def_feature.lookups.clone()
		),
		(
			"Kerning",
			"kern",
			vec!["kern1".to_owned(), "kern2".to_owned()]
		)
	);
	Ok(())
}

#[test]
fn test_lookup_duplicate() {
	assert!(
		parse_err(
			r#"DEF_LOOKUP "dupe"
AS_SUBSTITUTION
SUB GLYPH "a"
WITH GLYPH "a.alt"
END_SUB
END_SUBSTITUTION
DEF_LOOKUP "dupe"
AS_SUBSTITUTION
SUB GLYPH "b"
WITH GLYPH "b.alt"
END_SUB
END_SUBSTITUTION
"#
		)
		.contains(r#"Lookup "dupe" already defined, lookup names are case insensitive"#)
	);
}

#[test]
fn test_lookup_duplicate_insensitive_case() {
	assert!(
		parse_err(
			r#"DEF_LOOKUP "dupe"
AS_SUBSTITUTION
SUB GLYPH "a"
WITH GLYPH "a.alt"
END_SUB
END_SUBSTITUTION
DEF_LOOKUP "Dupe"
AS_SUBSTITUTION
SUB GLYPH "b"
WITH GLYPH "b.alt"
END_SUB
END_SUBSTITUTION
"#
		)
		.contains(r#"Lookup "Dupe" already defined, lookup names are case insensitive"#)
	);
}

#[test]
fn test_lookup_name_starts_with_letter() {
	assert!(
		parse_err(
			r#"DEF_LOOKUP "\lookupname"
AS_SUBSTITUTION
SUB GLYPH "a"
WITH GLYPH "a.alt"
END_SUB
END_SUBSTITUTION
"#
		)
		.contains(r#"Lookup name "\lookupname" must start with a letter"#)
	);
}

#[test]
fn test_lookup_comments() -> Result<()> {
	let doc = parse(
		r#"DEF_LOOKUP "test" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
COMMENTS "Hello\nWorld"
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "a"
WITH GLYPH "b"
END_SUB
END_SUBSTITUTION"#,
	)?;
	let lookup = get!(single(&doc.statements), LookupDefinition);
	assert_eq!(lookup.name, "test");
	assert_eq!(lookup.comments.as_deref(), Some("Hello\nWorld"));
	Ok(())
}

#[test]
fn test_substitution_empty() {
	assert!(
		parse_err(
			r#"DEF_LOOKUP "empty_substitution" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
END_SUBSTITUTION"#
		)
		.contains("Expected SUB")
	);
}

#[test]
fn test_substitution_invalid_many_to_many() {
	assert!(
		parse_err(
			r#"DEF_LOOKUP "invalid_substitution" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "f" GLYPH "i"
WITH GLYPH "f.alt" GLYPH "i.alt"
END_SUB
END_SUBSTITUTION"#
		)
		.contains("Invalid substitution type")
	);
}

#[test]
fn test_substitution_invalid_reverse_chaining_single() -> Result<()> {
	let doc = parse(
		r#"DEF_LOOKUP "invalid_substitution" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR REVERSAL
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "f" GLYPH "i"
WITH GLYPH "f_i"
END_SUB
END_SUBSTITUTION"#,
	)?;
	let lookup = get!(single(&doc.statements), LookupDefinition);
	assert_eq!(sub(lookup).kind, SubstitutionKind::Ligature);
	Ok(())
}

#[test]
fn test_substitution_invalid_mixed() {
	assert!(
		parse_err(
			r#"DEF_LOOKUP "invalid_substitution" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "fi"
WITH GLYPH "f" GLYPH "i"
END_SUB
SUB GLYPH "f" GLYPH "l"
WITH GLYPH "f_l"
END_SUB
END_SUBSTITUTION"#
		)
		.contains("Invalid substitution type")
	);
}

#[test]
fn test_substitution_single() -> Result<()> {
	let doc = parse(
		r#"DEF_LOOKUP "smcp" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "a"
WITH GLYPH "a.sc"
END_SUB
SUB GLYPH "b"
WITH GLYPH "b.sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	let lookup = get!(single(&doc.statements), LookupDefinition);
	assert_eq!(lookup.name, "smcp");
	assert_sub_equal(sub(lookup), &[&["a"], &["b"]], &[&["a.sc"], &["b.sc"]]);
	Ok(())
}

#[test]
fn test_substitution_single_in_context() -> Result<()> {
	let doc = parse(
		r#"DEF_GROUP "Denominators"
 ENUM GLYPH "one.dnom" GLYPH "two.dnom" END_ENUM
END_GROUP
DEF_LOOKUP "fracdnom" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
 LEFT ENUM GROUP "Denominators" GLYPH "fraction" END_ENUM
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "one"
WITH GLYPH "one.dnom"
END_SUB
SUB GLYPH "two"
WITH GLYPH "two.dnom"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(doc.statements.len(), 2);
	let lookup = lookup(&doc, 1);
	let context = &lookup.context[0];

	assert_eq!(lookup.name, "fracdnom");
	assert_eq!(context.ex_or_in, "IN_CONTEXT");
	assert_eq!(context.left.len(), 1);
	assert_eq!(context.left[0].len(), 1);
	let CoverageItem::Enum(e) = &context.left[0][0] else {
		panic!("not an enum");
	};
	assert_eq!(e.enum_.len(), 2);
	assert_eq!(group(&e.enum_[0]), "Denominators");
	assert_eq!(glyph(&e.enum_[1]), "fraction");
	assert!(context.right.is_empty());
	assert_sub_equal(
		sub(lookup),
		&[&["one"], &["two"]],
		&[&["one.dnom"], &["two.dnom"]],
	);
	Ok(())
}

#[test]
fn test_substitution_single_in_contexts() -> Result<()> {
	let doc = parse(
		r#"DEF_GROUP "Hebrew"
 ENUM GLYPH "uni05D0" GLYPH "uni05D1" END_ENUM
END_GROUP
DEF_LOOKUP "HebrewCurrency" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
 RIGHT GROUP "Hebrew"
 RIGHT GLYPH "one.Hebr"
END_CONTEXT
IN_CONTEXT
 LEFT GROUP "Hebrew"
 LEFT GLYPH "one.Hebr"
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "dollar"
WITH GLYPH "dollar.Hebr"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(doc.statements.len(), 2);
	let lookup = lookup(&doc, 1);
	let context1 = &lookup.context[0];
	let context2 = &lookup.context[1];

	assert_eq!(lookup.name, "HebrewCurrency");

	assert_eq!(context1.ex_or_in, "IN_CONTEXT");
	assert!(context1.left.is_empty());
	assert_eq!(context1.right.len(), 2);
	assert_eq!(context1.right[0].len(), 1);
	assert_eq!(context1.right[1].len(), 1);
	assert_eq!(group(&context1.right[0][0]), "Hebrew");
	assert_eq!(glyph(&context1.right[1][0]), "one.Hebr");

	assert_eq!(context2.ex_or_in, "IN_CONTEXT");
	assert_eq!(context2.left.len(), 2);
	assert_eq!(context2.left[0].len(), 1);
	assert_eq!(context2.left[1].len(), 1);
	assert_eq!(group(&context2.left[0][0]), "Hebrew");
	assert_eq!(glyph(&context2.left[1][0]), "one.Hebr");
	assert!(context2.right.is_empty());
	Ok(())
}

#[test]
fn test_substitution_skip_base() -> Result<()> {
	let doc = parse(
		r#"DEF_GROUP "SomeMarks"
 ENUM GLYPH "marka" GLYPH "markb" END_ENUM
END_GROUP
DEF_LOOKUP "SomeSub" SKIP_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(doc.statements.len(), 2);
	let lookup = lookup(&doc, 1);
	assert_eq!(
		(lookup.name.as_str(), lookup.process_base),
		("SomeSub", false)
	);
	Ok(())
}

#[test]
fn test_substitution_process_base() -> Result<()> {
	let doc = parse(
		r#"DEF_GROUP "SomeMarks"
 ENUM GLYPH "marka" GLYPH "markb" END_ENUM
END_GROUP
DEF_LOOKUP "SomeSub" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(doc.statements.len(), 2);
	let lookup = lookup(&doc, 1);
	assert_eq!(
		(lookup.name.as_str(), lookup.process_base),
		("SomeSub", true)
	);
	Ok(())
}

#[test]
fn test_substitution_process_marks() -> Result<()> {
	let doc = parse(
		r#"DEF_GROUP "SomeMarks"
 ENUM GLYPH "marka" GLYPH "markb" END_ENUM
END_GROUP
DEF_LOOKUP "SomeSub" PROCESS_BASE PROCESS_MARKS "SomeMarks"
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(doc.statements.len(), 2);
	let lookup = lookup(&doc, 1);
	assert_eq!(
		(lookup.name.as_str(), &lookup.process_marks),
		("SomeSub", &ProcessMarks::Group("SomeMarks".into()))
	);
	Ok(())
}

#[test]
fn test_substitution_process_marks_all() -> Result<()> {
	let doc = parse(
		r#"DEF_LOOKUP "SomeSub" PROCESS_BASE PROCESS_MARKS ALL
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	let lookup = get!(single(&doc.statements), LookupDefinition);
	assert_eq!(
		(lookup.name.as_str(), &lookup.process_marks),
		("SomeSub", &ProcessMarks::Bool(true))
	);
	Ok(())
}

#[test]
fn test_substitution_process_marks_none() -> Result<()> {
	let doc = parse_(
		r#"DEF_LOOKUP "SomeSub" PROCESS_BASE PROCESS_MARKS "NONE"
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	let lookup = get!(single(&doc.statements), LookupDefinition);
	assert_eq!(
		(lookup.name.as_str(), &lookup.process_marks),
		("SomeSub", &ProcessMarks::Bool(false))
	);
	Ok(())
}

#[test]
fn test_substitution_process_marks_bad() {
	assert!(
		parse_err(
			r#"DEF_GROUP "SomeMarks" ENUM GLYPH "marka" GLYPH "markb" END_ENUM END_GROUP
DEF_LOOKUP "SomeSub" PROCESS_BASE PROCESS_MARKS SomeMarks AS_SUBSTITUTION
SUB GLYPH "A" WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#
		)
		.contains("Expected ALL, NONE, MARK_GLYPH_SET or an ID")
	);
}

#[test]
fn test_substitution_skip_marks() -> Result<()> {
	let doc = parse(
		r#"DEF_GROUP "SomeMarks"
 ENUM GLYPH "marka" GLYPH "markb" END_ENUM
END_GROUP
DEF_LOOKUP "SomeSub" PROCESS_BASE SKIP_MARKS DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(doc.statements.len(), 2);
	let lookup = lookup(&doc, 1);
	assert_eq!(
		(lookup.name.as_str(), &lookup.process_marks),
		("SomeSub", &ProcessMarks::Bool(false))
	);
	Ok(())
}

#[test]
fn test_substitution_mark_attachment() -> Result<()> {
	let doc = parse(
		r#"DEF_GROUP "SomeMarks"
 ENUM GLYPH "acutecmb" GLYPH "gravecmb" END_ENUM
END_GROUP
DEF_LOOKUP "SomeSub" PROCESS_BASE PROCESS_MARKS "SomeMarks" DIRECTION RTL
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(doc.statements.len(), 2);
	let lookup = lookup(&doc, 1);
	assert_eq!(
		(lookup.name.as_str(), &lookup.process_marks),
		("SomeSub", &ProcessMarks::Group("SomeMarks".into()))
	);
	Ok(())
}

#[test]
fn test_substitution_mark_glyph_set() -> Result<()> {
	let doc = parse(
		r#"DEF_GROUP "SomeMarks"
 ENUM GLYPH "acutecmb" GLYPH "gravecmb" END_ENUM
END_GROUP
DEF_LOOKUP "SomeSub" PROCESS_BASE PROCESS_MARKS MARK_GLYPH_SET "SomeMarks" DIRECTION RTL
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(doc.statements.len(), 2);
	let lookup = lookup(&doc, 1);
	assert_eq!(
		(lookup.name.as_str(), lookup.mark_glyph_set.as_deref()),
		("SomeSub", Some("SomeMarks"))
	);
	Ok(())
}

#[test]
fn test_substitution_process_all_marks() -> Result<()> {
	let doc = parse(
		r#"DEF_GROUP "SomeMarks"
 ENUM GLYPH "acutecmb" GLYPH "gravecmb" END_ENUM
END_GROUP
DEF_LOOKUP "SomeSub" PROCESS_BASE PROCESS_MARKS ALL DIRECTION RTL
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(doc.statements.len(), 2);
	let lookup = lookup(&doc, 1);
	assert_eq!(
		(lookup.name.as_str(), &lookup.process_marks),
		("SomeSub", &ProcessMarks::Bool(true))
	);
	Ok(())
}

#[test]
fn test_substitution_no_reversal() -> Result<()> {
	// TODO: check right context with no reversal
	let doc = parse(
		r#"DEF_LOOKUP "Lookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
 RIGHT ENUM GLYPH "a" GLYPH "b" END_ENUM
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "a"
WITH GLYPH "a.alt"
END_SUB
END_SUBSTITUTION"#,
	)?;
	let lookup = get!(single(&doc.statements), LookupDefinition);
	assert_eq!((lookup.name.as_str(), lookup.reversal), ("Lookup", false));
	Ok(())
}

#[test]
fn test_substitution_reversal() -> Result<()> {
	let doc = parse(
		r#"DEF_GROUP "DFLT_Num_standardFigures"
 ENUM GLYPH "zero" GLYPH "one" GLYPH "two" END_ENUM
END_GROUP
DEF_GROUP "DFLT_Num_numerators"
 ENUM GLYPH "zero.numr" GLYPH "one.numr" GLYPH "two.numr" END_ENUM
END_GROUP
DEF_LOOKUP "RevLookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR REVERSAL
IN_CONTEXT
 RIGHT ENUM GLYPH "a" GLYPH "b" END_ENUM
END_CONTEXT
AS_SUBSTITUTION
SUB GROUP "DFLT_Num_standardFigures"
WITH GROUP "DFLT_Num_numerators"
END_SUB
END_SUBSTITUTION"#,
	)?;
	let lookup = lookup(&doc, doc.statements.len() - 1);
	assert_eq!((lookup.name.as_str(), lookup.reversal), ("RevLookup", true));
	Ok(())
}

#[test]
fn test_substitution_single_to_multiple() -> Result<()> {
	let doc = parse(
		r#"DEF_LOOKUP "ccmp" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "aacute"
WITH GLYPH "a" GLYPH "acutecomb"
END_SUB
SUB GLYPH "agrave"
WITH GLYPH "a" GLYPH "gravecomb"
END_SUB
END_SUBSTITUTION"#,
	)?;
	let lookup = get!(single(&doc.statements), LookupDefinition);
	assert_eq!(lookup.name, "ccmp");
	assert_sub_equal(
		sub(lookup),
		&[&["aacute"], &["agrave"]],
		&[&["a", "acutecomb"], &["a", "gravecomb"]],
	);
	Ok(())
}

#[test]
fn test_substitution_multiple_to_single() -> Result<()> {
	let doc = parse(
		r#"DEF_LOOKUP "liga" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "f" GLYPH "i"
WITH GLYPH "f_i"
END_SUB
SUB GLYPH "f" GLYPH "t"
WITH GLYPH "f_t"
END_SUB
END_SUBSTITUTION"#,
	)?;
	let lookup = get!(single(&doc.statements), LookupDefinition);
	assert_eq!(lookup.name, "liga");
	assert_sub_equal(
		sub(lookup),
		&[&["f", "i"], &["f", "t"]],
		&[&["f_i"], &["f_t"]],
	);
	Ok(())
}

#[test]
fn test_substitution_reverse_chaining_single() -> Result<()> {
	let doc = parse(
		r#"DEF_GLYPH "zero" ID 163 UNICODE 48 TYPE BASE END_GLYPH
DEF_GLYPH "one" ID 194 UNICODE 49 TYPE BASE END_GLYPH
DEF_GLYPH "two" ID 195 UNICODE 50 TYPE BASE END_GLYPH
DEF_GLYPH "three" ID 196 UNICODE 51 TYPE BASE END_GLYPH
DEF_GLYPH "four" ID 197 UNICODE 52 TYPE BASE END_GLYPH
DEF_GLYPH "fize" ID 165 UNICODE 53 TYPE BASE END_GLYPH
DEF_GLYPH "six" ID 209 UNICODE 54 TYPE BASE END_GLYPH
DEF_GLYPH "seven" ID 210 UNICODE 55 TYPE BASE END_GLYPH
DEF_GLYPH "eight" ID 211 UNICODE 56 TYPE BASE END_GLYPH
DEF_GLYPH "nine" ID 212 UNICODE 57 TYPE BASE END_GLYPH
DEF_GLYPH "zero.numr" ID 213 TYPE BASE END_GLYPH
DEF_GLYPH "one.numr" ID 214 TYPE BASE END_GLYPH
DEF_GLYPH "two.numr" ID 215 TYPE BASE END_GLYPH
DEF_GLYPH "three.numr" ID 216 TYPE BASE END_GLYPH
DEF_GLYPH "four.numr" ID 217 TYPE BASE END_GLYPH
DEF_GLYPH "fize.numr" ID 218 TYPE BASE END_GLYPH
DEF_GLYPH "six.numr" ID 219 TYPE BASE END_GLYPH
DEF_GLYPH "seven.numr" ID 220 TYPE BASE END_GLYPH
DEF_GLYPH "eight.numr" ID 221 TYPE BASE END_GLYPH
DEF_GLYPH "nine.numr" ID 222 TYPE BASE END_GLYPH
DEF_LOOKUP "numr" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR REVERSAL
IN_CONTEXT
 RIGHT ENUM GLYPH "fraction" RANGE "zero.numr" TO "nine.numr" END_ENUM
END_CONTEXT
AS_SUBSTITUTION
SUB RANGE "zero" TO "nine"
WITH RANGE "zero.numr" TO "nine.numr"
END_SUB
END_SUBSTITUTION"#,
	)?;
	let lookup = lookup(&doc, doc.statements.len() - 1);

	let range = |item: &CoverageItem| match item {
		CoverageItem::Range(r) => (r.start.clone(), r.end.clone()),
		other => panic!("not a range: {other:?}"),
	};
	let mapping = &sub(lookup).mapping;
	let glyphs: Vec<Vec<(String, String)>> = mapping
		.iter()
		.map(|(k, _)| k.iter().map(range).collect())
		.collect();
	let replacement: Vec<Vec<(String, String)>> = mapping
		.iter()
		.map(|(_, v)| v.iter().map(range).collect())
		.collect();

	assert_eq!(lookup.name, "numr");
	assert_eq!(glyphs, [[("zero".to_owned(), "nine".to_owned())]]);
	assert_eq!(
		replacement,
		[[("zero.numr".to_owned(), "nine.numr".to_owned())]]
	);

	assert_eq!(lookup.context[0].right.len(), 1);
	assert_eq!(lookup.context[0].right[0].len(), 1);
	let CoverageItem::Enum(enum_) = &lookup.context[0].right[0][0] else {
		panic!("not an enum");
	};
	assert_eq!(enum_.enum_.len(), 2);
	assert_eq!(glyph(&enum_.enum_[0]), "fraction");
	assert_eq!(
		range(&enum_.enum_[1]),
		("zero.numr".to_owned(), "nine.numr".to_owned())
	);
	Ok(())
}

#[test]
fn test_position_empty() {
	assert!(
		parse_err(
			r#"DEF_LOOKUP "empty_position" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
EXCEPT_CONTEXT
 LEFT GLYPH "glyph"
END_CONTEXT
AS_POSITION
END_POSITION"#
		)
		.contains("Expected ATTACH, ATTACH_CURSIVE, ADJUST_PAIR, ADJUST_SINGLE")
	);
}

#[test]
fn test_position_attach() -> Result<()> {
	let doc = parse(
		r#"DEF_LOOKUP "anchor_top" PROCESS_BASE PROCESS_MARKS ALL DIRECTION RTL
IN_CONTEXT
END_CONTEXT
AS_POSITION
ATTACH GLYPH "a" GLYPH "e"
TO GLYPH "acutecomb" AT ANCHOR "top" GLYPH "gravecomb" AT ANCHOR "top"
END_ATTACH
END_POSITION
DEF_ANCHOR "MARK_top" ON 120 GLYPH acutecomb COMPONENT 1 AT  POS DX 0 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "MARK_top" ON 121 GLYPH gravecomb COMPONENT 1 AT  POS DX 0 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "top" ON 31 GLYPH a COMPONENT 1 AT  POS DX 210 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "top" ON 35 GLYPH e COMPONENT 1 AT  POS DX 215 DY 450 END_POS END_ANCHOR"#,
	)?;
	assert_eq!(doc.statements.len(), 5);
	let lookup = lookup(&doc, 0);
	let Some(PositionDefinition::Attach(pos)) = &lookup.pos else {
		panic!("not an attachment");
	};
	let coverage = glyphs(&pos.coverage);
	let coverage_to: Vec<(Vec<&str>, &str)> = pos
		.coverage_to
		.iter()
		.map(|(e, a)| (glyphs(e), a.as_str()))
		.collect();
	assert_eq!(
		(lookup.name.as_str(), coverage, coverage_to),
		(
			"anchor_top",
			vec!["a", "e"],
			vec![(vec!["acutecomb"], "top"), (vec!["gravecomb"], "top")]
		)
	);
	let anchor = |i: usize| anchor_tuple(get!(doc.statements[i], AnchorDefinition));
	assert_eq!(
		anchor(1),
		(
			"MARK_top",
			120,
			"acutecomb",
			1,
			false,
			mkpos(None, Some(0), Some(450))
		)
	);
	assert_eq!(
		anchor(2),
		(
			"MARK_top",
			121,
			"gravecomb",
			1,
			false,
			mkpos(None, Some(0), Some(450))
		)
	);
	assert_eq!(
		anchor(3),
		("top", 31, "a", 1, false, mkpos(None, Some(210), Some(450)))
	);
	assert_eq!(
		anchor(4),
		("top", 35, "e", 1, false, mkpos(None, Some(215), Some(450)))
	);
	Ok(())
}

#[test]
fn test_position_attach_cursive() -> Result<()> {
	let doc = parse(
		r#"DEF_LOOKUP "SomeLookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION RTL
IN_CONTEXT
END_CONTEXT
AS_POSITION
ATTACH_CURSIVE
EXIT  GLYPH "a" GLYPH "b"
ENTER  GLYPH "c"
END_ATTACH
END_POSITION"#,
	)?;
	let lookup = get!(single(&doc.statements), LookupDefinition);
	let Some(PositionDefinition::AttachCursive(pos)) = &lookup.pos else {
		panic!("not a cursive attachment");
	};
	let exit: Vec<Vec<&str>> = pos.coverages_exit.iter().map(|v| glyphs(v)).collect();
	let enter: Vec<Vec<&str>> = pos.coverages_enter.iter().map(|v| glyphs(v)).collect();
	assert_eq!(
		(lookup.name.as_str(), exit, enter),
		("SomeLookup", vec![vec!["a", "b"]], vec![vec!["c"]])
	);
	Ok(())
}

#[test]
fn test_position_adjust_pair() -> Result<()> {
	let doc = parse(
		r#"DEF_LOOKUP "kern1" PROCESS_BASE PROCESS_MARKS ALL DIRECTION RTL
IN_CONTEXT
END_CONTEXT
AS_POSITION
ADJUST_PAIR
 FIRST  GLYPH "A"
 SECOND  GLYPH "V"
 1 2 BY POS ADV -30 END_POS POS END_POS
 2 1 BY POS ADV -30 END_POS POS END_POS

END_ADJUST
END_POSITION"#,
	)?;
	let lookup = get!(single(&doc.statements), LookupDefinition);
	let Some(PositionDefinition::AdjustPair(pos)) = &lookup.pos else {
		panic!("not a pair adjustment");
	};
	let coverages_1: Vec<Vec<&str>> = pos.coverages_1.iter().map(|v| glyphs(v)).collect();
	let coverages_2: Vec<Vec<&str>> = pos.coverages_2.iter().map(|v| glyphs(v)).collect();
	let expected: IndexMap<(i64, i64), (Pos, Pos)> = [
		((1, 2), (mkpos(Some(-30), None, None), Pos::default())),
		((2, 1), (mkpos(Some(-30), None, None), Pos::default())),
	]
	.into_iter()
	.collect();
	assert_eq!(
		(
			lookup.name.as_str(),
			coverages_1,
			coverages_2,
			&pos.adjust_pair
		),
		("kern1", vec![vec!["A"]], vec![vec!["V"]], &expected)
	);
	Ok(())
}

#[test]
fn test_position_adjust_single() -> Result<()> {
	let doc = parse(
		r#"DEF_LOOKUP "TestLookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_POSITION
ADJUST_SINGLE GLYPH "glyph1" BY POS ADV 0 DX 123 END_POS GLYPH "glyph2" BY POS ADV 0 DX 456 END_POS
END_ADJUST
END_POSITION"#,
	)?;
	let lookup = get!(single(&doc.statements), LookupDefinition);
	let Some(PositionDefinition::AdjustSingle(pos)) = &lookup.pos else {
		panic!("not a single adjustment");
	};
	let adjust: Vec<(Vec<&str>, Pos)> = pos
		.adjust_single
		.iter()
		.map(|(a, b)| (glyphs(a), b.clone()))
		.collect();
	assert_eq!(
		(lookup.name.as_str(), adjust),
		(
			"TestLookup",
			vec![
				(vec!["glyph1"], mkpos(Some(0), Some(123), None)),
				(vec!["glyph2"], mkpos(Some(0), Some(456), None)),
			]
		)
	);
	Ok(())
}

#[test]
fn test_def_anchor() -> Result<()> {
	let doc = parse(
		r#"DEF_ANCHOR "top" ON 120 GLYPH a COMPONENT 1 AT  POS DX 250 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "MARK_top" ON 120 GLYPH acutecomb COMPONENT 1 AT  POS DX 0 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "bottom" ON 120 GLYPH a COMPONENT 1 AT  POS DX 250 DY 0 END_POS END_ANCHOR"#,
	)?;
	assert_eq!(doc.statements.len(), 3);
	let anchor = |i: usize| anchor_tuple(get!(doc.statements[i], AnchorDefinition));
	assert_eq!(
		anchor(0),
		("top", 120, "a", 1, false, mkpos(None, Some(250), Some(450)))
	);
	assert_eq!(
		anchor(1),
		(
			"MARK_top",
			120,
			"acutecomb",
			1,
			false,
			mkpos(None, Some(0), Some(450))
		)
	);
	assert_eq!(
		anchor(2),
		(
			"bottom",
			120,
			"a",
			1,
			false,
			mkpos(None, Some(250), Some(0))
		)
	);
	Ok(())
}

#[test]
fn test_def_anchor_multi_component() -> Result<()> {
	let doc = parse(
		r#"DEF_ANCHOR "top" ON 120 GLYPH a COMPONENT 1 AT  POS DX 250 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "top" ON 120 GLYPH a COMPONENT 2 AT  POS DX 250 DY 450 END_POS END_ANCHOR"#,
	)?;
	assert_eq!(doc.statements.len(), 2);
	let anchor1 = get!(doc.statements[0], AnchorDefinition);
	let anchor2 = get!(doc.statements[1], AnchorDefinition);
	assert_eq!(
		(
			anchor1.name.as_str(),
			anchor1.gid,
			anchor1.glyph_name.as_str(),
			anchor1.component
		),
		("top", 120, "a", 1)
	);
	assert_eq!(
		(
			anchor2.name.as_str(),
			anchor2.gid,
			anchor2.glyph_name.as_str(),
			anchor2.component
		),
		("top", 120, "a", 2)
	);
	Ok(())
}

#[test]
fn test_def_anchor_duplicate() {
	assert!(
		parse_err(
			r#"DEF_ANCHOR "dupe" ON 120 GLYPH a COMPONENT 1 AT  POS DX 250 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "dupe" ON 120 GLYPH a COMPONENT 1 AT  POS DX 250 DY 450 END_POS END_ANCHOR"#
		)
		.contains(r#"Anchor "dupe" already defined, anchor names are case insensitive"#)
	);
}

#[test]
fn test_def_anchor_locked() -> Result<()> {
	let doc = parse(
		r#"DEF_ANCHOR "top" ON 120 GLYPH a COMPONENT 1 LOCKED AT  POS DX 250 DY 450 END_POS END_ANCHOR"#,
	)?;
	let anchor = get!(single(&doc.statements), AnchorDefinition);
	assert_eq!(
		anchor_tuple(anchor),
		("top", 120, "a", 1, true, mkpos(None, Some(250), Some(450)))
	);
	Ok(())
}

#[test]
fn test_anchor_adjust_device() -> Result<()> {
	let doc = parse(
		r#"DEF_ANCHOR "MARK_top" ON 123 GLYPH diacglyph COMPONENT 1 AT  POS DX 0 DY 456 ADJUST_BY 12 AT 34 ADJUST_BY 56 AT 78 END_POS END_ANCHOR"#,
	)?;
	let anchor = get!(single(&doc.statements), AnchorDefinition);
	let mut expected = mkpos(None, Some(0), Some(456));
	expected.dy_adjust_by = [(34, 12), (78, 56)].into_iter().collect();
	assert_eq!(
		(anchor.name.as_str(), anchor.pos.clone()),
		("MARK_top", expected)
	);
	Ok(())
}

fn settings(doc: &VoltFile) -> Vec<(&str, &SettingValue)> {
	doc.statements
		.iter()
		.map(|s| {
			let s = get!(s, SettingDefinition);
			(s.name.as_str(), &s.value)
		})
		.collect()
}

#[test]
fn test_ppem() -> Result<()> {
	let doc = parse(
		r#"GRID_PPEM 20
PRESENTATION_PPEM 72
PPOSITIONING_PPEM 144"#,
	)?;
	assert_eq!(
		settings(&doc),
		[
			("GRID_PPEM", &SettingValue::Number(20)),
			("PRESENTATION_PPEM", &SettingValue::Number(72)),
			("PPOSITIONING_PPEM", &SettingValue::Number(144)),
		]
	);
	Ok(())
}

#[test]
fn test_compiler_flags() -> Result<()> {
	let doc = parse(
		r#"COMPILER_USEEXTENSIONLOOKUPS
COMPILER_USEPAIRPOSFORMAT2"#,
	)?;
	assert_eq!(
		settings(&doc),
		[
			("COMPILER_USEEXTENSIONLOOKUPS", &SettingValue::True),
			("COMPILER_USEPAIRPOSFORMAT2", &SettingValue::True),
		]
	);
	Ok(())
}

#[test]
fn test_cmap() -> Result<()> {
	let doc = parse(
		r#"CMAP_FORMAT 0 3 4
CMAP_FORMAT 1 0 6
CMAP_FORMAT 3 1 4"#,
	)?;
	assert_eq!(
		settings(&doc),
		[
			("CMAP_FORMAT", &SettingValue::Numbers(vec![0, 3, 4])),
			("CMAP_FORMAT", &SettingValue::Numbers(vec![1, 0, 6])),
			("CMAP_FORMAT", &SettingValue::Numbers(vec![3, 1, 4])),
		]
	);
	Ok(())
}

#[test]
fn test_do_not_touch_cmap() -> Result<()> {
	let doc = parse(
		r#"DO_NOT_TOUCH_CMAP
CMAP_FORMAT 0 3 4
CMAP_FORMAT 1 0 6
CMAP_FORMAT 3 1 4"#,
	)?;
	assert_eq!(
		settings(&doc),
		[
			("DO_NOT_TOUCH_CMAP", &SettingValue::True),
			("CMAP_FORMAT", &SettingValue::Numbers(vec![0, 3, 4])),
			("CMAP_FORMAT", &SettingValue::Numbers(vec![1, 0, 6])),
			("CMAP_FORMAT", &SettingValue::Numbers(vec![3, 1, 4])),
		]
	);
	Ok(())
}

#[test]
fn test_stop_at_end() -> Result<()> {
	let doc = parse_("DEF_GLYPH \".notdef\" ID 0 TYPE BASE END_GLYPH END\0\0\0\0")?;
	let def_glyph = get!(single(&doc.statements), GlyphDefinition);
	assert_eq!(
		glyph_tuple(def_glyph),
		(".notdef", 0, None, Some("BASE"), None)
	);
	assert_eq!(
		doc.to_string(),
		"\nDEF_GLYPH \".notdef\" ID 0 TYPE BASE END_GLYPH END\n"
	);
	Ok(())
}
