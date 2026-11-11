//! `Tests/voltLib/volttofea_test.py`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Once;
use std::sync::atomic::{AtomicUsize, Ordering};

use fontoxide_voltlib::volt_to_fea::main as cli;
use fontoxide_voltlib::{ParserInput, Result, VoltToFea};

fn parse(text: &str) -> Result<String> {
	VoltToFea::new(ParserInput::text(text), None).convert(None, false)
}

fn data(name: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/voltLib/data")
		.join(name)
}

/// A new temporary file path for this test.
fn temp_path() -> PathBuf {
	static COUNTER: AtomicUsize = AtomicUsize::new(0);
	let dir = std::env::temp_dir().join(format!(
		"fontoxide-voltlib-volttofea-{}",
		std::process::id()
	));
	std::fs::create_dir_all(&dir).expect("temp dir");
	dir.join(format!("tmp{}", COUNTER.fetch_add(1, Ordering::SeqCst)))
}

/// Reads a text file with universal newlines, as `open()` does.
fn read_text(path: &PathBuf) -> String {
	std::fs::read_to_string(path)
		.expect("readable")
		.replace("\r\n", "\n")
}

thread_local! {
	static CAPTURED: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

struct Capture;

impl log::Log for Capture {
	fn enabled(&self, _: &log::Metadata<'_>) -> bool {
		true
	}

	fn log(&self, record: &log::Record<'_>) {
		if record.level() <= log::Level::Warn {
			CAPTURED.with(|c| {
				if let Some(records) = c.borrow_mut().as_mut() {
					records.push(record.args().to_string());
				}
			});
		}
	}

	fn flush(&self) {}
}

static CAPTURE: Capture = Capture;
static INIT: Once = Once::new();

/// The warnings `f` logs on this thread (`assertLogs(level="WARNING")`).
fn capture<R>(f: impl FnOnce() -> R) -> (Vec<String>, R) {
	INIT.call_once(|| {
		let _ = log::set_logger(&CAPTURE);
		log::set_max_level(log::LevelFilter::Warn);
	});
	CAPTURED.with(|c| *c.borrow_mut() = Some(Vec::new()));
	let result = f();
	let records = CAPTURED.with(|c| c.borrow_mut().take()).unwrap_or_default();
	(records, result)
}

fn run_cli(source: &str, fea: &str) -> Result<()> {
	let temp = temp_path();
	let source = data(source);
	let code = cli(&[
		source.to_str().expect("utf-8 path"),
		temp.to_str().expect("utf-8 path"),
	])?;
	assert_eq!(code, 0);
	assert_eq!(read_text(&temp), read_text(&data(fea)));
	Ok(())
}

#[test]
fn test_cli_vtp() -> Result<()> {
	run_cli("Nutso.vtp", "Nutso.fea")
}

#[test]
fn test_group_order() -> Result<()> {
	run_cli("NamdhinggoSIL1006.vtp", "NamdhinggoSIL1006.fea")
}

#[test]
fn test_cli_ttf() -> Result<()> {
	run_cli("Nutso.ttf", "Nutso.fea")
}

#[test]
#[allow(non_snake_case)]
fn test_cli_ttf_no_TSIV() -> Result<()> {
	let ttf = data("Empty.ttf");
	let temp = temp_path();
	assert_eq!(
		cli(&[
			ttf.to_str().expect("utf-8 path"),
			temp.to_str().expect("utf-8 path")
		])?,
		1
	);
	Ok(())
}

#[test]
fn test_def_glyph_base() -> Result<()> {
	let fea = parse(r#"DEF_GLYPH ".notdef" ID 0 TYPE BASE END_GLYPH"#)?;
	assert_eq!(
		fea,
		r#"@GDEF_base = [.notdef];
table GDEF {
    GlyphClassDef @GDEF_base, , , ;
} GDEF;
"#
	);
	Ok(())
}

#[test]
fn test_def_glyph_base_2_components() -> Result<()> {
	let fea = parse(r#"DEF_GLYPH "glyphBase" ID 320 TYPE BASE COMPONENTS 2 END_GLYPH"#)?;
	assert_eq!(
		fea,
		r#"@GDEF_base = [glyphBase];
table GDEF {
    GlyphClassDef @GDEF_base, , , ;
} GDEF;
"#
	);
	Ok(())
}

#[test]
fn test_def_glyph_ligature_2_components() -> Result<()> {
	let fea = parse(r#"DEF_GLYPH "f_f" ID 320 TYPE LIGATURE COMPONENTS 2 END_GLYPH"#)?;
	assert_eq!(
		fea,
		r#"@GDEF_ligature = [f_f];
table GDEF {
    GlyphClassDef , @GDEF_ligature, , ;
} GDEF;
"#
	);
	Ok(())
}

#[test]
fn test_def_glyph_mark() -> Result<()> {
	let fea = parse(r#"DEF_GLYPH "brevecomb" ID 320 TYPE MARK END_GLYPH"#)?;
	assert_eq!(
		fea,
		r#"@GDEF_mark = [brevecomb];
table GDEF {
    GlyphClassDef , , @GDEF_mark, ;
} GDEF;
"#
	);
	Ok(())
}

#[test]
fn test_def_glyph_component() -> Result<()> {
	let fea = parse(r#"DEF_GLYPH "f.f_f" ID 320 TYPE COMPONENT END_GLYPH"#)?;
	assert_eq!(
		fea,
		r#"@GDEF_component = [f.f_f];
table GDEF {
    GlyphClassDef , , , @GDEF_component;
} GDEF;
"#
	);
	Ok(())
}

#[test]
fn test_def_glyph_no_type() -> Result<()> {
	let fea = parse(r#"DEF_GLYPH "glyph20" ID 20 END_GLYPH"#)?;
	assert_eq!(fea, "");
	Ok(())
}

#[test]
fn test_def_glyph_case_sensitive() -> Result<()> {
	let fea = parse(
		r#"DEF_GLYPH "A" ID 3 UNICODE 65 TYPE BASE END_GLYPH
DEF_GLYPH "a" ID 4 UNICODE 97 TYPE BASE END_GLYPH
"#,
	)?;
	assert_eq!(
		fea,
		r#"@GDEF_base = [A a];
table GDEF {
    GlyphClassDef @GDEF_base, , , ;
} GDEF;
"#
	);
	Ok(())
}

#[test]
fn test_def_group_glyphs() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "aaccented"
ENUM GLYPH "aacute" GLYPH "abreve" GLYPH "acircumflex" GLYPH "adieresis" GLYPH "ae" GLYPH "agrave" GLYPH "amacron" GLYPH "aogonek" GLYPH "aring" GLYPH "atilde" END_ENUM
END_GROUP
"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@aaccented = [aacute abreve acircumflex adieresis ae agrave amacron aogonek aring atilde];"#
	);
	Ok(())
}

#[test]
fn test_def_group_groups() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "Group1"
ENUM GLYPH "a" GLYPH "b" GLYPH "c" GLYPH "d" END_ENUM
END_GROUP
DEF_GROUP "Group2"
ENUM GLYPH "e" GLYPH "f" GLYPH "g" GLYPH "h" END_ENUM
END_GROUP
DEF_GROUP "TestGroup"
ENUM GROUP "Group1" GROUP "Group2" END_ENUM
END_GROUP
"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@Group1 = [a b c d];
@Group2 = [e f g h];
@TestGroup = [@Group1 @Group2];"#
	);
	Ok(())
}

#[test]
fn test_def_group_groups_not_yet_defined() -> Result<()> {
	let fea = parse(
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
END_GROUP
"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@Group1 = [a b c d];
@Group2 = [e f g h];
@TestGroup1 = [@Group1 @Group2];
@TestGroup2 = [@Group2];
@TestGroup3 = [@Group2 @Group1];"#
	);
	Ok(())
}

#[test]
fn test_def_group_groups_not_yet_defined_nested() -> Result<()> {
	let fea = parse(
		r#"
            DEF_GROUP "Group1"
            ENUM GLYPH "a" GLYPH "b" GLYPH "c" GLYPH "d" END_ENUM
            END_GROUP
            DEF_GROUP "TestGroup1"
            ENUM ENUM GROUP "Group1" GROUP "Group2" END_ENUM END_ENUM
            END_GROUP
            DEF_GROUP "TestGroup2"
            ENUM ENUM ENUM ENUM GROUP "Group2" END_ENUM END_ENUM END_ENUM END_ENUM
            END_GROUP
            DEF_GROUP "TestGroup3"
            ENUM GROUP "Group2" GROUP "Group1" END_ENUM
            END_GROUP
            DEF_GROUP "Group2"
            ENUM GLYPH "e" GLYPH "f" GLYPH "g" GLYPH "h" END_ENUM
            END_GROUP
            "#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@Group1 = [a b c d];
@Group2 = [e f g h];
@TestGroup1 = [@Group1 @Group2];
@TestGroup2 = [@Group2];
@TestGroup3 = [@Group2 @Group1];"#
	);
	Ok(())
}

#[test]
fn test_def_group_glyphs_and_group() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "aaccented"
ENUM GLYPH "aacute" GLYPH "abreve" GLYPH "acircumflex" GLYPH "adieresis" GLYPH "ae" GLYPH "agrave" GLYPH "amacron" GLYPH "aogonek" GLYPH "aring" GLYPH "atilde" END_ENUM
END_GROUP
DEF_GROUP "KERN_lc_a_2ND"
ENUM GLYPH "a" GROUP "aaccented" END_ENUM
END_GROUP"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@aaccented = [aacute abreve acircumflex adieresis ae agrave amacron aogonek aring atilde];
@KERN_lc_a_2ND = [a @aaccented];"#
	);
	Ok(())
}

#[test]
fn test_def_group_range() -> Result<()> {
	let fea = parse(
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
	assert_eq!(
		fea,
		r#"# Glyph classes
@KERN_lc_a_2ND = [a agrave aacute acircumflex atilde b c ccaron ccedilla cdotaccent];
@GDEF_base = [a agrave aacute acircumflex atilde c ccaron ccedilla cdotaccent];
table GDEF {
    GlyphClassDef @GDEF_base, , , ;
} GDEF;
"#
	);
	Ok(())
}

#[test]
fn test_script_without_langsys() -> Result<()> {
	let fea = parse(
		r#"DEF_SCRIPT NAME "Latin" TAG "latn"
END_SCRIPT"#,
	)?;
	assert_eq!(fea, "");
	Ok(())
}

#[test]
fn test_langsys_normal() -> Result<()> {
	let fea = parse(
		r#"DEF_SCRIPT NAME "Latin" TAG "latn"
DEF_LANGSYS NAME "Romanian" TAG "ROM "
END_LANGSYS
DEF_LANGSYS NAME "Moldavian" TAG "MOL "
END_LANGSYS
END_SCRIPT"#,
	)?;
	assert_eq!(fea, "");
	Ok(())
}

#[test]
fn test_langsys_no_script_name() -> Result<()> {
	let fea = parse(
		r#"DEF_SCRIPT TAG "latn"
DEF_LANGSYS NAME "Default" TAG "dflt"
END_LANGSYS
END_SCRIPT"#,
	)?;
	assert_eq!(fea, "");
	Ok(())
}

#[test]
fn test_langsys_lang_in_separate_scripts() -> Result<()> {
	let fea = parse(
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
	assert_eq!(fea, "");
	Ok(())
}

#[test]
fn test_langsys_no_lang_name() -> Result<()> {
	let fea = parse(
		r#"DEF_SCRIPT NAME "Latin" TAG "latn"
DEF_LANGSYS TAG "dflt"
END_LANGSYS
END_SCRIPT"#,
	)?;
	assert_eq!(fea, "");
	Ok(())
}

#[test]
fn test_langsys_short_tag() -> Result<()> {
	let fea = parse(
		r#"
            DEF_SCRIPT NAME "Latin" TAG "latn"
            DEF_LANGSYS NAME "Romanian" TAG "ROM"
            DEF_FEATURE NAME "Fractions" TAG "frac"
            LOOKUP "test"
            END_FEATURE
            END_LANGSYS
            END_SCRIPT
            DEF_LOOKUP "test" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
            IN_CONTEXT
            END_CONTEXT
            AS_SUBSTITUTION
            SUB GLYPH "one" GLYPH "slash" GLYPH "two"
            WITH GLYPH "one_slash_two.frac"
            END_SUB
            END_SUBSTITUTION
            "#,
	)?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup test {
    sub one slash two by one_slash_two.frac;
} test;

# Features
feature frac {
    script latn;
    language ROM exclude_dflt;
    lookup test;
} frac;
"#
	);
	Ok(())
}

#[test]
fn test_feature() -> Result<()> {
	let fea = parse(
		r#"DEF_SCRIPT NAME "Latin" TAG "latn"
DEF_LANGSYS NAME "Romanian" TAG "ROM "
DEF_FEATURE NAME "Fractions" TAG "frac"
LOOKUP "fraclookup"
END_FEATURE
END_LANGSYS
END_SCRIPT
DEF_LOOKUP "fraclookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "one" GLYPH "slash" GLYPH "two"
WITH GLYPH "one_slash_two.frac"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup fraclookup {
    sub one slash two by one_slash_two.frac;
} fraclookup;

# Features
feature frac {
    script latn;
    language ROM exclude_dflt;
    lookup fraclookup;
} frac;
"#
	);
	Ok(())
}

#[test]
fn test_feature_sub_lookups() -> Result<()> {
	let fea = parse(
		r#"DEF_SCRIPT NAME "Latin" TAG "latn"
DEF_LANGSYS NAME "Romanian" TAG "ROM "
DEF_FEATURE NAME "Fractions" TAG "frac"
LOOKUP "fraclookup\1"
LOOKUP "fraclookup\1"
END_FEATURE
END_LANGSYS
END_SCRIPT
DEF_LOOKUP "fraclookup\1" PROCESS_BASE PROCESS_MARKS ALL DIRECTION RTL
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "one" GLYPH "slash" GLYPH "two"
WITH GLYPH "one_slash_two.frac"
END_SUB
END_SUBSTITUTION
DEF_LOOKUP "fraclookup\2" PROCESS_BASE PROCESS_MARKS ALL DIRECTION RTL
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "one" GLYPH "slash" GLYPH "three"
WITH GLYPH "one_slash_three.frac"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup fraclookup {
    lookupflag RightToLeft;
    # fraclookup\1
    sub one slash two by one_slash_two.frac;
    subtable;
    # fraclookup\2
    sub one slash three by one_slash_three.frac;
} fraclookup;

# Features
feature frac {
    script latn;
    language ROM exclude_dflt;
    lookup fraclookup;
} frac;
"#
	);
	Ok(())
}

#[test]
fn test_lookup_comment() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "smcp" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
COMMENTS "Smallcaps lookup for testing"
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
	assert_eq!(
		fea,
		r#"
# Lookups
lookup smcp {
    # Smallcaps lookup for testing
    sub a by a.sc;
    sub b by b.sc;
} smcp;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_single() -> Result<()> {
	let fea = parse(
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
SUB WITH
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup smcp {
    sub a by a.sc;
    sub b by b.sc;
} smcp;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_single_in_context() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "Denominators" ENUM GLYPH "one.dnom" GLYPH "two.dnom" END_ENUM END_GROUP
DEF_LOOKUP "fracdnom" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT LEFT ENUM GROUP "Denominators" GLYPH "fraction" END_ENUM
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
	assert_eq!(
		fea,
		r#"# Glyph classes
@Denominators = [one.dnom two.dnom];

# Lookups
lookup fracdnom_chained {
    sub one by one.dnom;
    sub two by two.dnom;
} fracdnom_chained;

lookup fracdnom {
    sub [@Denominators fraction] [one two]' lookup fracdnom_chained;
} fracdnom;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_single_in_contexts() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "Hebrew" ENUM GLYPH "uni05D0" GLYPH "uni05D1" END_ENUM END_GROUP
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
	assert_eq!(
		fea,
		r#"# Glyph classes
@Hebrew = [uni05D0 uni05D1];

# Lookups
lookup HebrewCurrency_chained {
    sub dollar by dollar.Hebr;
} HebrewCurrency_chained;

lookup HebrewCurrency {
    sub dollar' lookup HebrewCurrency_chained @Hebrew one.Hebr;
    sub @Hebrew one.Hebr dollar' lookup HebrewCurrency_chained;
} HebrewCurrency;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_single_except_context() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "Hebrew" ENUM GLYPH "uni05D0" GLYPH "uni05D1" END_ENUM END_GROUP
DEF_LOOKUP "HebrewCurrency" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
EXCEPT_CONTEXT
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
	assert_eq!(
		fea,
		r#"# Glyph classes
@Hebrew = [uni05D0 uni05D1];

# Lookups
lookup HebrewCurrency_chained {
    sub dollar by dollar.Hebr;
} HebrewCurrency_chained;

lookup HebrewCurrency {
    ignore sub dollar' @Hebrew one.Hebr;
    sub @Hebrew one.Hebr dollar' lookup HebrewCurrency_chained;
} HebrewCurrency;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_skip_base() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "SomeMarks" ENUM GLYPH "marka" GLYPH "markb" END_ENUM END_GROUP
DEF_LOOKUP "SomeSub" SKIP_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@SomeMarks = [marka markb];

# Lookups
lookup SomeSub {
    lookupflag IgnoreBaseGlyphs;
    sub A by A.c2sc;
} SomeSub;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_process_base() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "SomeMarks" ENUM GLYPH "marka" GLYPH "markb" END_ENUM END_GROUP
DEF_LOOKUP "SomeSub" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@SomeMarks = [marka markb];

# Lookups
lookup SomeSub {
    sub A by A.c2sc;
} SomeSub;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_process_marks_all() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "SomeMarks" ENUM GLYPH "marka" GLYPH "markb" END_ENUM END_GROUP
DEF_LOOKUP "SomeSub" PROCESS_BASE PROCESS_MARKS "ALL"DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@SomeMarks = [marka markb];

# Lookups
lookup SomeSub {
    sub A by A.c2sc;
} SomeSub;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_process_marks_none() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "SomeMarks" ENUM GLYPH "marka" GLYPH "markb" END_ENUM END_GROUP
DEF_LOOKUP "SomeSub" PROCESS_BASE PROCESS_MARKS "NONE"DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@SomeMarks = [marka markb];

# Lookups
lookup SomeSub {
    lookupflag IgnoreMarks;
    sub A by A.c2sc;
} SomeSub;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_skip_marks() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "SomeMarks" ENUM GLYPH "marka" GLYPH "markb" END_ENUM END_GROUP
DEF_LOOKUP "SomeSub" PROCESS_BASE SKIP_MARKS DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@SomeMarks = [marka markb];

# Lookups
lookup SomeSub {
    lookupflag IgnoreMarks;
    sub A by A.c2sc;
} SomeSub;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_mark_attachment() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "SomeMarks" ENUM GLYPH "acutecmb" GLYPH "gravecmb" END_ENUM END_GROUP
DEF_LOOKUP "SomeSub" PROCESS_BASE PROCESS_MARKS "SomeMarks" 
DIRECTION RTL
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@SomeMarks = [acutecmb gravecmb];

# Lookups
lookup SomeSub {
    lookupflag RightToLeft MarkAttachmentType @SomeMarks;
    sub A by A.c2sc;
} SomeSub;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_mark_glyph_set() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "SomeMarks" ENUM GLYPH "acutecmb" GLYPH "gravecmb" END_ENUM END_GROUP
DEF_LOOKUP "SomeSub" PROCESS_BASE PROCESS_MARKS MARK_GLYPH_SET "SomeMarks" 
DIRECTION RTL
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@SomeMarks = [acutecmb gravecmb];

# Lookups
lookup SomeSub {
    lookupflag RightToLeft UseMarkFilteringSet @SomeMarks;
    sub A by A.c2sc;
} SomeSub;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_process_all_marks() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "SomeMarks" ENUM GLYPH "acutecmb" GLYPH "gravecmb" END_ENUM END_GROUP
DEF_LOOKUP "SomeSub" PROCESS_BASE PROCESS_MARKS ALL 
DIRECTION RTL
AS_SUBSTITUTION
SUB GLYPH "A"
WITH GLYPH "A.c2sc"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@SomeMarks = [acutecmb gravecmb];

# Lookups
lookup SomeSub {
    lookupflag RightToLeft;
    sub A by A.c2sc;
} SomeSub;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_no_reversal() -> Result<()> {
	let fea = parse(
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
	assert_eq!(
		fea,
		r#"
# Lookups
lookup Lookup_chained {
    sub a by a.alt;
} Lookup_chained;

lookup Lookup {
    sub a' lookup Lookup_chained [a b];
} Lookup;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_reversal() -> Result<()> {
	let fea = parse(
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
	assert_eq!(
		fea,
		r#"# Glyph classes
@DFLT_Num_standardFigures = [zero one two];
@DFLT_Num_numerators = [zero.numr one.numr two.numr];

# Lookups
lookup RevLookup {
    rsub @DFLT_Num_standardFigures' [a b] by @DFLT_Num_numerators;
} RevLookup;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_single_to_multiple() -> Result<()> {
	let fea = parse(
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
	assert_eq!(
		fea,
		r#"
# Lookups
lookup ccmp {
    sub aacute by a acutecomb;
    sub agrave by a gravecomb;
} ccmp;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_multiple_to_single() -> Result<()> {
	let fea = parse(
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
	assert_eq!(
		fea,
		r#"
# Lookups
lookup liga {
    sub f i by f_i;
    sub f t by f_t;
} liga;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_multiple_to_single_with_groups() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "g1" ENUM GLYPH "i" GLYPH "t" END_ENUM END_GROUP
DEF_GROUP "g2" ENUM GLYPH "f_i" GLYPH "f_t" END_ENUM END_GROUP
DEF_LOOKUP "liga" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "f" GROUP "g1"
WITH GROUP "g2"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@g1 = [i t];
@g2 = [f_i f_t];

# Lookups
lookup liga {
    # sub f @g1 by @g2;
    sub f i by f_i;
    sub f t by f_t;
} liga;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_multiple_to_single_with_enums() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "liga" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "f" ENUM GLYPH "i" GLYPH "i.alt" END_ENUM
WITH GLYPH "f_i"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup liga {
    # sub f [i i.alt] by f_i;
    sub f i by f_i;
    sub f i.alt by f_i;
} liga;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_multiple_to_single_with_enums_long_replacement() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "liga" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "f" ENUM GLYPH "i" GLYPH "t" END_ENUM
WITH ENUM GLYPH "f_i" GLYPH "f_t" GLYPH "f_l" END_ENUM
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup liga {
    # sub f [i t] by [f_i f_t f_l];
    sub f i by f_i;
    sub f t by f_t;
} liga;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_reverse_chaining_single() -> Result<()> {
	let fea = parse(
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
	assert_eq!(
		fea,
		r#"
# Lookups
lookup numr {
    rsub [zero one two three four fize six seven eight nine]' [fraction zero.numr one.numr two.numr three.numr four.numr fize.numr six.numr seven.numr eight.numr nine.numr] by [zero.numr one.numr two.numr three.numr four.numr fize.numr six.numr seven.numr eight.numr nine.numr];
} numr;

@GDEF_base = [zero one two three four fize six seven eight nine zero.numr one.numr two.numr three.numr four.numr fize.numr six.numr seven.numr eight.numr nine.numr];
table GDEF {
    GlyphClassDef @GDEF_base, , , ;
} GDEF;
"#
	);
	Ok(())
}

#[test]
fn test_substitution_alternate() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "b" ENUM GLYPH "b" END_ENUM END_GROUP
DEF_LOOKUP "test" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "a"
WITH GLYPH "a.alt1"
END_SUB
SUB GLYPH "a"
WITH GLYPH "a.alt2"
END_SUB
SUB GROUP "b"
WITH GROUP "b"
END_SUB
SUB GLYPH "b"
WITH GLYPH "b.alt"
END_SUB
END_SUBSTITUTION"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@b = [b];

# Lookups
lookup test {
    sub a from [a.alt1 a.alt2];
    sub b from [b b.alt];
} test;
"#
	);
	Ok(())
}

#[test]
fn test_position_attach() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "anchor_top" PROCESS_BASE PROCESS_MARKS ALL DIRECTION RTL
IN_CONTEXT
END_CONTEXT
AS_POSITION
ATTACH GLYPH "a" GLYPH "e"
TO GLYPH "acutecomb" AT ANCHOR "top" GLYPH "gravecomb" AT ANCHOR "top"
END_ATTACH
END_POSITION
DEF_ANCHOR "MARK_top" ON 120 GLYPH acutecomb COMPONENT 1 AT POS DX 0 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "MARK_top" ON 121 GLYPH gravecomb COMPONENT 1 AT POS DX 0 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "top" ON 31 GLYPH a COMPONENT 1 AT POS DX 210 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "top" ON 35 GLYPH e COMPONENT 1 AT POS DX 215 DY 450 END_POS END_ANCHOR
"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Mark classes
markClass acutecomb <anchor 0 450> @top.anchor_top;
markClass gravecomb <anchor 0 450> @top.anchor_top;

# Lookups
lookup anchor_top {
    lookupflag RightToLeft;
    pos base a
        <anchor 210 450> mark @top.anchor_top;
    pos base e
        <anchor 215 450> mark @top.anchor_top;
} anchor_top;
"#
	);
	Ok(())
}

#[test]
fn test_position_attach_mkmk() -> Result<()> {
	let fea = parse(
		r#"DEF_GLYPH "brevecomb" ID 1 TYPE MARK END_GLYPH
DEF_GLYPH "gravecomb" ID 2 TYPE MARK END_GLYPH
DEF_LOOKUP "anchor_top" PROCESS_BASE PROCESS_MARKS ALL DIRECTION RTL
IN_CONTEXT
END_CONTEXT
AS_POSITION
ATTACH GLYPH "gravecomb"
TO GLYPH "acutecomb" AT ANCHOR "top"
END_ATTACH
END_POSITION
DEF_ANCHOR "MARK_top" ON 1 GLYPH acutecomb COMPONENT 1 AT POS DX 0 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "top" ON 2 GLYPH gravecomb COMPONENT 1 AT POS DX 210 DY 450 END_POS END_ANCHOR
"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Mark classes
markClass acutecomb <anchor 0 450> @top.anchor_top;

# Lookups
lookup anchor_top {
    lookupflag RightToLeft;
    pos mark gravecomb
        <anchor 210 450> mark @top.anchor_top;
} anchor_top;

@GDEF_mark = [brevecomb gravecomb];
table GDEF {
    GlyphClassDef , , @GDEF_mark, ;
} GDEF;
"#
	);
	Ok(())
}

#[test]
fn test_position_attach_in_context() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "test" PROCESS_BASE PROCESS_MARKS ALL DIRECTION RTL
EXCEPT_CONTEXT LEFT GLYPH "a" END_CONTEXT
AS_POSITION
ATTACH GLYPH "a"
TO GLYPH "acutecomb" AT ANCHOR "top" GLYPH "gravecomb" AT ANCHOR "top"
END_ATTACH
END_POSITION
DEF_ANCHOR "MARK_top" ON 120 GLYPH acutecomb COMPONENT 1 AT POS DX 0 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "MARK_top" ON 121 GLYPH gravecomb COMPONENT 1 AT POS DX 0 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "top" ON 31 GLYPH a COMPONENT 1 AT POS DX 210 DY 450 END_POS END_ANCHOR
"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Mark classes
markClass acutecomb <anchor 0 450> @top.test;
markClass gravecomb <anchor 0 450> @top.test;

# Lookups
lookup test_chained {
    pos base a
        <anchor 210 450> mark @top.test;
} test_chained;

lookup test {
    lookupflag RightToLeft;
    ignore pos a [acutecomb gravecomb]';
    pos [acutecomb gravecomb]' lookup test_chained;
} test;
"#
	);
	Ok(())
}

#[test]
fn test_position_attach_overlapping_classes() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "above_marks"
ENUM GLYPH "acutecomb" GLYPH "gravecomb" END_ENUM
END_GROUP
DEF_LOOKUP "anchor_top" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_POSITION
ATTACH GLYPH "a" GLYPH "e"
TO GLYPH "gravecomb" AT ANCHOR "top2" GROUP "above_marks" AT ANCHOR "top"
END_ATTACH
END_POSITION
DEF_ANCHOR "MARK_top" ON 120 GLYPH acutecomb COMPONENT 1 AT POS DX 0 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "MARK_top" ON 121 GLYPH gravecomb COMPONENT 1 AT POS DX 0 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "MARK_top2" ON 121 GLYPH gravecomb COMPONENT 1 AT POS DX 0 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "top" ON 31 GLYPH a COMPONENT 1 AT POS DX 210 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "top" ON 35 GLYPH e COMPONENT 1 AT POS DX 215 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "top2" ON 31 GLYPH a COMPONENT 1 AT POS DX 210 DY 550 END_POS END_ANCHOR
DEF_ANCHOR "top2" ON 35 GLYPH e COMPONENT 1 AT POS DX 215 DY 550 END_POS END_ANCHOR
"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@above_marks = [acutecomb gravecomb];

# Mark classes
markClass acutecomb <anchor 0 450> @top.anchor_top;
markClass gravecomb <anchor 0 450> @top2.anchor_top;

# Lookups
lookup anchor_top {
    pos base a
        <anchor 210 550> mark @top2.anchor_top
        <anchor 210 450> mark @top.anchor_top;
    pos base e
        <anchor 215 550> mark @top2.anchor_top
        <anchor 215 450> mark @top.anchor_top;
} anchor_top;
"#
	);
	Ok(())
}

#[test]
fn test_position_attach_overlapping_classes_dropped() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "above_marks"
ENUM GLYPH "gravecomb" END_ENUM
END_GROUP
DEF_LOOKUP "anchor_top" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_POSITION
ATTACH GLYPH "a" GLYPH "e"
TO GLYPH "gravecomb" AT ANCHOR "top2" GROUP "above_marks" AT ANCHOR "top"
END_ATTACH
END_POSITION
DEF_ANCHOR "MARK_top" ON 120 GLYPH acutecomb COMPONENT 1 AT POS DX 0 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "MARK_top" ON 121 GLYPH gravecomb COMPONENT 1 AT POS DX 0 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "MARK_top2" ON 121 GLYPH gravecomb COMPONENT 1 AT POS DX 0 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "top" ON 31 GLYPH a COMPONENT 1 AT POS DX 210 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "top" ON 35 GLYPH e COMPONENT 1 AT POS DX 215 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "top2" ON 31 GLYPH a COMPONENT 1 AT POS DX 210 DY 550 END_POS END_ANCHOR
DEF_ANCHOR "top2" ON 35 GLYPH e COMPONENT 1 AT POS DX 215 DY 550 END_POS END_ANCHOR
"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@above_marks = [gravecomb];

# Mark classes
markClass gravecomb <anchor 0 450> @top2.anchor_top;

# Lookups
lookup anchor_top {
    pos base a
        <anchor 210 550> mark @top2.anchor_top;
    pos base e
        <anchor 215 550> mark @top2.anchor_top;
} anchor_top;
"#
	);
	Ok(())
}

#[test]
fn test_position_attach_cursive() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "SomeLookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION RTL
IN_CONTEXT
END_CONTEXT
AS_POSITION
ATTACH_CURSIVE EXIT GLYPH "a" GLYPH "b" ENTER GLYPH "a" GLYPH "c"
END_ATTACH
END_POSITION
DEF_ANCHOR "exit"  ON 1 GLYPH a COMPONENT 1 AT POS END_POS END_ANCHOR
DEF_ANCHOR "entry" ON 1 GLYPH a COMPONENT 1 AT POS END_POS END_ANCHOR
DEF_ANCHOR "exit"  ON 2 GLYPH b COMPONENT 1 AT POS END_POS END_ANCHOR
DEF_ANCHOR "entry" ON 3 GLYPH c COMPONENT 1 AT POS END_POS END_ANCHOR
"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup SomeLookup {
    lookupflag RightToLeft;
    pos cursive a <anchor 0 0> <anchor 0 0>;
    pos cursive c <anchor 0 0> <anchor NULL>;
    pos cursive b <anchor NULL> <anchor 0 0>;
} SomeLookup;
"#
	);
	Ok(())
}

#[test]
fn test_position_adjust_pair() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "kern1" PROCESS_BASE PROCESS_MARKS ALL DIRECTION RTL
IN_CONTEXT
END_CONTEXT
AS_POSITION
ADJUST_PAIR
 FIRST GLYPH "A" FIRST GLYPH "V"
 SECOND GLYPH "A" SECOND GLYPH "V"
 1 2 BY POS ADV -30 END_POS POS END_POS
 2 1 BY POS ADV -25 END_POS POS END_POS
END_ADJUST
END_POSITION
"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup kern1 {
    lookupflag RightToLeft;
    enum pos A V -30;
    enum pos V A -25;
} kern1;
"#
	);
	Ok(())
}

#[test]
fn test_position_adjust_pair_in_context() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "kern1" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
EXCEPT_CONTEXT LEFT GLYPH "A" END_CONTEXT
AS_POSITION
ADJUST_PAIR
 FIRST GLYPH "A" FIRST GLYPH "V"
 SECOND GLYPH "A" SECOND GLYPH "V"
 2 1 BY POS ADV -25 END_POS POS END_POS
END_ADJUST
END_POSITION
"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup kern1_chained {
    enum pos V A -25;
} kern1_chained;

lookup kern1 {
    ignore pos A V' A';
    pos V' lookup kern1_chained A' lookup kern1_chained;
} kern1;
"#
	);
	Ok(())
}

#[test]
fn test_position_adjust_single() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "TestLookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_POSITION
ADJUST_SINGLE GLYPH "glyph1" BY POS ADV 0 DX 123 END_POS
 GLYPH "glyph2" BY POS ADV 0 DX 456 END_POS
END_ADJUST
END_POSITION
"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup TestLookup {
    pos glyph1 <123 0 0 0>;
    pos glyph2 <456 0 0 0>;
} TestLookup;
"#
	);
	Ok(())
}

#[test]
fn test_position_adjust_single_in_context() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "TestLookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
EXCEPT_CONTEXT
LEFT GLYPH "leftGlyph"
RIGHT GLYPH "rightGlyph"
END_CONTEXT
AS_POSITION
ADJUST_SINGLE GLYPH "glyph1" BY POS ADV 0 DX 123 END_POS
 GLYPH "glyph2" BY POS ADV 0 DX 456 END_POS
END_ADJUST
END_POSITION
"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup TestLookup_chained {
    pos glyph1 <123 0 0 0>;
    pos glyph2 <456 0 0 0>;
} TestLookup_chained;

lookup TestLookup {
    ignore pos leftGlyph [glyph1 glyph2]' rightGlyph;
    pos [glyph1 glyph2]' lookup TestLookup_chained;
} TestLookup;
"#
	);
	Ok(())
}

#[test]
fn test_def_anchor() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "TestLookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_POSITION
ATTACH GLYPH "a"
TO GLYPH "acutecomb" AT ANCHOR "top"
END_ATTACH
END_POSITION
DEF_ANCHOR "top" ON 120 GLYPH a COMPONENT 1 AT POS DX 250 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "MARK_top" ON 120 GLYPH acutecomb COMPONENT 1 AT POS DX 0 DY 450 END_POS END_ANCHOR"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Mark classes
markClass acutecomb <anchor 0 450> @top.TestLookup;

# Lookups
lookup TestLookup {
    pos base a
        <anchor 250 450> mark @top.TestLookup;
} TestLookup;
"#
	);
	Ok(())
}

#[test]
fn test_def_anchor_case_insensitive() -> Result<()> {
	let fea = parse(
		r#"
            DEF_LOOKUP "TestLookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
            IN_CONTEXT
            END_CONTEXT
            AS_POSITION
            ATTACH ENUM GLYPH "a" GLYPH "f_i" END_ENUM
            TO GLYPH "acutecomb" AT ANCHOR "top"
            END_ATTACH
            END_POSITION
            DEF_ANCHOR "top" ON 1 GLYPH a COMPONENT 1 AT POS DX 250 DY 450 END_POS END_ANCHOR
            DEF_ANCHOR "TOP" ON 2 GLYPH f_i COMPONENT 1 AT POS DX 250 DY 450 END_POS END_ANCHOR
            DEF_ANCHOR "Top" ON 2 GLYPH f_i COMPONENT 2 AT POS DX 350 DY 450 END_POS END_ANCHOR
            DEF_ANCHOR "MARK_Top" ON 3 GLYPH acutecomb COMPONENT 1 AT POS DX 0 DY 450 END_POS END_ANCHOR
            "#,
	)?;
	assert_eq!(
		fea,
		r#"
# Mark classes
markClass acutecomb <anchor 0 450> @top.TestLookup;

# Lookups
lookup TestLookup {
    pos base a
        <anchor 250 450> mark @top.TestLookup;
    pos base f_i
        <anchor 250 450> mark @top.TestLookup;
} TestLookup;
"#
	);
	Ok(())
}

#[test]
fn test_def_anchor_multi_component() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "TestLookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_POSITION
ATTACH GLYPH "f_f"
TO GLYPH "acutecomb" AT ANCHOR "top"
END_ATTACH
END_POSITION
DEF_GLYPH "f_f" ID 120 TYPE LIGATURE COMPONENTS 2 END_GLYPH
DEF_ANCHOR "top" ON 120 GLYPH f_f COMPONENT 1 AT POS DX 250 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "top" ON 120 GLYPH f_f COMPONENT 2 AT POS DX 450 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "MARK_top" ON 120 GLYPH acutecomb COMPONENT 1 AT POS  END_POS END_ANCHOR"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Mark classes
markClass acutecomb <anchor 0 0> @top.TestLookup;

# Lookups
lookup TestLookup {
    pos ligature f_f
            <anchor 250 450> mark @top.TestLookup
        ligComponent
            <anchor 450 450> mark @top.TestLookup;
} TestLookup;

@GDEF_ligature = [f_f];
table GDEF {
    GlyphClassDef , @GDEF_ligature, , ;
} GDEF;
"#
	);
	Ok(())
}

#[test]
fn test_def_anchor_ligature_missing_component() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "TestLookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_POSITION
ATTACH GLYPH "f_f"
TO GLYPH "acutecomb" AT ANCHOR "top"
END_ATTACH
END_POSITION
DEF_GLYPH "f_f" ID 120 TYPE LIGATURE COMPONENTS 2 END_GLYPH
DEF_ANCHOR "top" ON 120 GLYPH f_f COMPONENT 1 AT POS DX 250 DY 450 END_POS END_ANCHOR
DEF_ANCHOR "MARK_top" ON 120 GLYPH acutecomb COMPONENT 1 AT POS  END_POS END_ANCHOR"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Mark classes
markClass acutecomb <anchor 0 0> @top.TestLookup;

# Lookups
lookup TestLookup {
    pos ligature f_f
            <anchor 250 450> mark @top.TestLookup
        ligComponent
            <anchor NULL>;
} TestLookup;

@GDEF_ligature = [f_f];
table GDEF {
    GlyphClassDef , @GDEF_ligature, , ;
} GDEF;
"#
	);
	Ok(())
}

#[test]
fn test_def_anchor_ligature_in_base_lookup() -> Result<()> {
	let fea = parse(
		r#"
            DEF_LOOKUP "TestLookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
            IN_CONTEXT
            END_CONTEXT
            AS_POSITION
            ATTACH GLYPH "f_f" GLYPH "f"
            TO GLYPH "acutecomb" AT ANCHOR "top"
            END_ATTACH
            END_POSITION
            DEF_GLYPH "f" ID 120 TYPE BASE END_GLYPH
            DEF_GLYPH "f_f" ID 121 TYPE LIGATURE COMPONENTS 2 END_GLYPH
            DEF_ANCHOR "top" ON 120 GLYPH f COMPONENT 1 AT POS DX 250 DY 450 END_POS END_ANCHOR
            DEF_ANCHOR "top" ON 120 GLYPH f_f COMPONENT 1 AT POS DX 250 DY 450 END_POS END_ANCHOR
            DEF_ANCHOR "top" ON 120 GLYPH f_f COMPONENT 2 AT POS DX 450 DY 450 END_POS END_ANCHOR
            DEF_ANCHOR "MARK_top" ON 120 GLYPH acutecomb COMPONENT 1 AT POS  END_POS END_ANCHOR
            "#,
	)?;
	assert_eq!(
		fea,
		r#"
# Mark classes
markClass acutecomb <anchor 0 0> @top.TestLookup;

# Lookups
lookup TestLookup {
    pos base f_f
        <anchor 250 450> mark @top.TestLookup;
    pos base f
        <anchor 250 450> mark @top.TestLookup;
} TestLookup;

@GDEF_base = [f];
@GDEF_ligature = [f_f];
table GDEF {
    GlyphClassDef @GDEF_base, @GDEF_ligature, , ;
} GDEF;
"#
	);
	Ok(())
}

#[test]
fn test_def_anchor_mark_in_base_lookup() -> Result<()> {
	let fea = parse(
		r#"
            DEF_LOOKUP "TestLookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
            IN_CONTEXT
            END_CONTEXT
            AS_POSITION
            ATTACH GLYPH "acutecomb" GLYPH "f"
            TO GLYPH "acutecomb" AT ANCHOR "top"
            END_ATTACH
            END_POSITION
            DEF_GLYPH "f" ID 120 TYPE BASE END_GLYPH
            DEF_GLYPH "acutecomb" ID 121 TYPE MARK END_GLYPH
            DEF_ANCHOR "top" ON 120 GLYPH f COMPONENT 1 AT POS DX 250 DY 450 END_POS END_ANCHOR
            DEF_ANCHOR "top" ON 120 GLYPH acutecomb COMPONENT 1 AT POS  END_POS END_ANCHOR
            DEF_ANCHOR "MARK_top" ON 120 GLYPH acutecomb COMPONENT 1 AT POS  END_POS END_ANCHOR
            "#,
	)?;
	assert_eq!(
		fea,
		r#"
# Mark classes
markClass acutecomb <anchor 0 0> @top.TestLookup;

# Lookups
lookup TestLookup {
    pos base acutecomb
        <anchor 0 0> mark @top.TestLookup;
    pos base f
        <anchor 250 450> mark @top.TestLookup;
} TestLookup;

@GDEF_base = [f];
@GDEF_mark = [acutecomb];
table GDEF {
    GlyphClassDef @GDEF_base, , @GDEF_mark, ;
} GDEF;
"#
	);
	Ok(())
}

#[test]
fn test_anchor_adjust_device() -> Result<()> {
	let fea = parse(
		r#"DEF_ANCHOR "MARK_top" ON 123 GLYPH diacglyph COMPONENT 1 AT POS DX 0 DY 456 ADJUST_BY 12 AT 34 ADJUST_BY 56 AT 78 END_POS END_ANCHOR
DEF_ANCHOR "top" ON 121 GLYPH baseglyph COMPONENT 1 AT POS DX 0 DY 0END_POS END_ANCHOR
DEF_LOOKUP "test" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
AS_POSITION
ATTACH GLYPH "baseglyph"
TO GLYPH "diacglyph" AT ANCHOR "top"
END_ATTACH
END_POSITION"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Mark classes
markClass diacglyph <anchor 0 456 <device NULL> <device 34 12, 78 56>> @top.test;

# Lookups
lookup test {
    pos base baseglyph
        <anchor 0 0> mark @top.test;
} test;
"#
	);
	Ok(())
}

#[test]
fn test_nested_enum() -> Result<()> {
	let fea = parse(
		r#"
            DEF_GLYPH "a" ID 1 END_GLYPH
            DEF_GLYPH "b" ID 2 END_GLYPH
            DEF_GLYPH "c" ID 3 END_GLYPH
            DEF_GLYPH "d" ID 4 END_GLYPH
            DEF_GLYPH "e" ID 5 END_GLYPH
            DEF_GLYPH "f" ID 6 END_GLYPH
            DEF_GLYPH "g" ID 7 END_GLYPH
            DEF_GLYPH "h" ID 8 END_GLYPH
            DEF_GLYPH "i" ID 9 END_GLYPH
            DEF_GLYPH "j" ID 10 END_GLYPH
            DEF_GLYPH "k" ID 11 END_GLYPH
            DEF_GLYPH "l" ID 12 END_GLYPH
            DEF_GLYPH "m" ID 13 END_GLYPH
            DEF_GLYPH "n" ID 14 END_GLYPH
            DEF_GLYPH "o" ID 15 END_GLYPH
            DEF_GLYPH "p" ID 16 END_GLYPH
            DEF_GLYPH "q" ID 17 END_GLYPH
            DEF_LOOKUP "lookup" PROCESS_BASE SKIP_MARKS DIRECTION RTL
            IN_CONTEXT
              RIGHT GLYPH "space"
            END_CONTEXT
            AS_POSITION
            ADJUST_SINGLE
              ENUM GLYPH "a" GLYPH "b" END_ENUM BY POS ADV -10 DX -10 END_POS
              ENUM GLYPH "c" GLYPH "d" END_ENUM BY POS ADV -20 DX -20 END_POS
              ENUM RANGE "e" TO "f" GLYPH "g" RANGE "h" TO "k" GLYPH "l" END_ENUM BY POS ADV -30 DX -40 END_POS
              RANGE "m" TO "q" BY POS ADV -50 DX -60 END_POS
            END_ADJUST
            END_POSITION
            "#,
	)?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup lookup_chained {
    pos [a b] <-10 0 -10 0>;
    pos [c d] <-20 0 -20 0>;
    pos [e f g h i j k l] <-40 0 -30 0>;
    pos [m n o p q] <-60 0 -50 0>;
} lookup_chained;

lookup lookup {
    lookupflag RightToLeft IgnoreMarks;
    pos [a b c d e f g h i j k l m n o p q]' lookup lookup_chained space;
} lookup;
"#
	);
	Ok(())
}

#[test]
fn test_use_extension() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "liga1" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_SUBSTITUTION
SUB GLYPH "f" GLYPH "i" WITH GLYPH "f_i"
END_SUB
END_SUBSTITUTION
DEF_LOOKUP "kern1" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
IN_CONTEXT
END_CONTEXT
AS_POSITION
ADJUST_PAIR
 FIRST GLYPH "A" FIRST GLYPH "V"
 SECOND GLYPH "A" SECOND GLYPH "V"
 1 2 BY POS ADV -30 END_POS POS END_POS
 2 1 BY POS ADV -25 END_POS POS END_POS
END_ADJUST
END_POSITION
COMPILER_USEEXTENSIONLOOKUPS
"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup liga1 useExtension {
    sub f i by f_i;
} liga1;

lookup kern1 useExtension {
    enum pos A V -30;
    enum pos V A -25;
} kern1;
"#
	);
	Ok(())
}

#[test]
fn test_unsupported_compiler_flags() -> Result<()> {
	let (logs, fea) = capture(|| parse("CMAP_FORMAT 0 3 4"));
	let fea = fea?;
	assert_eq!(fea, "");
	assert_eq!(logs, ["Unsupported setting ignored: CMAP_FORMAT",]);
	Ok(())
}

#[test]
fn test_sanitize_lookup_name() -> Result<()> {
	let fea = parse(
		r#"DEF_LOOKUP "Test Lookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR IN_CONTEXT END_CONTEXT
AS_POSITION ADJUST_PAIR END_ADJUST END_POSITION
DEF_LOOKUP "Test-Lookup" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR IN_CONTEXT END_CONTEXT
AS_POSITION ADJUST_PAIR END_ADJUST END_POSITION
"#,
	)?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup Test_Lookup {
    
} Test_Lookup;

lookup Test_Lookup_ {
    
} Test_Lookup_;
"#
	);
	Ok(())
}

#[test]
fn test_sanitize_group_name() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "aaccented glyphs"
ENUM GLYPH "aacute" GLYPH "abreve" END_ENUM
END_GROUP
DEF_GROUP "aaccented+glyphs"
ENUM GLYPH "aacute" GLYPH "abreve" END_ENUM
END_GROUP
"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@aaccented_glyphs = [aacute abreve];
@aaccented_glyphs_ = [aacute abreve];"#
	);
	Ok(())
}

#[test]
fn test_group_nested_enum() -> Result<()> {
	let fea = parse(
		r#"DEF_GROUP "foo"
ENUM ENUM GLYPH "foo" GLYPH "foo.1" GLYPH "foo.2" END_ENUM END_ENUM
END_GROUP"#,
	)?;
	assert_eq!(
		fea,
		r#"# Glyph classes
@foo = [foo foo.1 foo.2];"#
	);
	Ok(())
}

#[test]
fn test_aalt_feature() -> Result<()> {
	let (logs, fea) = capture(|| {
		parse(
			r#"
                DEF_SCRIPT NAME "Latin" TAG "latn"
                DEF_LANGSYS NAME "English" TAG "ENG "
                DEF_FEATURE NAME "Access All Alternates" TAG "aalt"
                LOOKUP "test1"
                END_FEATURE
                END_LANGSYS
                END_SCRIPT
                DEF_SCRIPT NAME "Default" TAG "DFLT"
                DEF_LANGSYS NAME "English" TAG "ENG "
                DEF_FEATURE NAME "Access All Alternates" TAG "aalt"
                LOOKUP "test2"
                END_FEATURE
                END_LANGSYS
                DEF_LANGSYS NAME "Default" TAG "dflt"
                DEF_FEATURE NAME "Access All Alternates" TAG "aalt"
                LOOKUP "test3"
                END_FEATURE
                END_LANGSYS
                END_SCRIPT
                DEF_LOOKUP "test1" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
                IN_CONTEXT
                END_CONTEXT
                AS_SUBSTITUTION
                SUB GLYPH "a"
                WITH GLYPH "a.alt"
                END_SUB
                END_SUBSTITUTION
                DEF_LOOKUP "test2" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
                IN_CONTEXT
                END_CONTEXT
                AS_SUBSTITUTION
                SUB GLYPH "b"
                WITH GLYPH "b.alt"
                END_SUB
                END_SUBSTITUTION
                DEF_LOOKUP "test3" PROCESS_BASE PROCESS_MARKS ALL DIRECTION LTR
                IN_CONTEXT
                END_CONTEXT
                AS_SUBSTITUTION
                SUB GLYPH "c"
                WITH GLYPH "c.alt"
                END_SUB
                END_SUBSTITUTION
                "#,
		)
	});
	let fea = fea?;
	assert_eq!(
		fea,
		r#"
# Lookups
lookup test1 {
    sub a by a.alt;
} test1;

lookup test2 {
    sub b by b.alt;
} test2;

lookup test3 {
    sub c by c.alt;
} test3;

# Features
feature aalt {
    lookup test3;
} aalt;
"#
	);
	assert_eq!(
		logs,
		[
			"FEA syntax does not allow script statements in 'aalt' feature, so only lookups from the first script will be included.",
			"FEA syntax does not allow language statements in 'aalt' feature, so only lookups from the first language will be included.",
		]
	);
	Ok(())
}
