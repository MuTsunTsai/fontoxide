//! `Tests/mtiLib/mti_test.py`.

use std::path::PathBuf;

use fontoxide_misc::xml_writer::XmlWriter;
use fontoxide_mtilib::{BuildOptions, Error, build_with};
use fontoxide_ttlib::{Font, Table};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const GLYPH_ORDER: &str = "\
.notdef a b pakannada phakannada vakannada pevowelkannada phevowelkannada vevowelkannada
uvowelsignkannada uuvowelsignkannada uvowelsignaltkannada uuvowelsignaltkannada
uuvowelsignsinh uvowelsignsinh rakarsinh zero one two three four five six seven eight
nine slash fraction A B C fi fl breve acute uniFB01 ffi grave commaacent dotbelow
dotabove cedilla commaaccent Acircumflex V T acircumflex Aacute Agrave O Oacute Ograve
Ocircumflex aacute agrave aimatrabindigurmukhi aimatragurmukhi aimatratippigurmukhi
aumatrabindigurmukhi aumatragurmukhi bindigurmukhi eematrabindigurmukhi eematragurmukhi
eematratippigurmukhi oomatrabindigurmukhi oomatragurmukhi oomatratippigurmukhi lagurmukhi
lanuktagurmukhi nagurmukhi nanuktagurmukhi ngagurmukhi nganuktagurmukhi nnagurmukhi
nnanuktagurmukhi tthagurmukhi tthanuktagurmukhi bsuperior isuperior vsuperior wsuperior
periodsuperior osuperior tsuperior dollarsuperior fsuperior gsuperior zsuperior dsuperior
psuperior hsuperior oesuperior aesuperior centsuperior esuperior lsuperior qsuperior
csuperior asuperior commasuperior xsuperior egravesuperior usuperior rsuperior nsuperior
ssuperior msuperior jsuperior ysuperior ksuperior guilsinglright guilsinglleft uniF737
uniE11C uniE11D uniE11A uni2077 uni2087 uniE11B uniE119 uniE0DD uniE0DE uniF736 uniE121
uniE122 uniE11F uni2076 uni2086 uniE120 uniE11E uniE0DB uniE0DC uniF733 uniE12B uniE12C
uniE129 uni00B3 uni2083 uniE12A uniE128 uniF732 uniE133 uniE134 uniE131 uni00B2 uni2082
uniE132 uniE130 uniE0F9 uniF734 uniE0D4 uniE0D5 uniE0D2 uni2074 uni2084 uniE0D3 uniE0D1
uniF730 uniE13D uniE13E uniE13A uni2070 uni2080 uniE13B uniE139 uniE13C uniF739 uniE0EC
uniE0ED uniE0EA uni2079 uni2089 uniE0EB uniE0E9 uniF735 uniE0CD uniE0CE uniE0CB uni2075
uni2085 uniE0CC uniE0CA uniF731 uniE0F3 uniE0F4 uniE0F1 uni00B9 uni2081 uniE0F2 uniE0F0
uniE0F8 uniF738 uniE0C0 uniE0C1 uniE0BE uni2078 uni2088 uniE0BF uniE0BD I Ismall t i f IJ
J IJsmall Jsmall tt ij j ffb ffh h ffk k ffl l fft fb ff fh fj fk ft janyevoweltelugu
kassevoweltelugu jaivoweltelugu nyasubscripttelugu kaivoweltelugu ssasubscripttelugu
bayi1 jeemi1 kafi1 ghafi1 laami1 kafm1 ghafm1 laamm1 rayf2 reyf2 yayf2 zayf2 fayi1 ayehf2
hamzayeharabf2 hamzayehf2 yehf2 ray rey zay yay dal del zal rayf1 reyf1 yayf1 zayf1
ayehf1 hamzayeharabf1 hamzayehf1 yehf1 dal1 del1 zal1 onehalf onehalf.alt onequarter
onequarter.alt threequarters threequarters.alt AlefSuperiorNS DammaNS DammaRflxNS
DammatanNS Fatha2dotsNS FathaNS FathatanNS FourDotsAboveNS HamzaAboveNS MaddaNS
OneDotAbove2NS OneDotAboveNS ShaddaAlefNS ShaddaDammaNS ShaddaDammatanNS ShaddaFathatanNS
ShaddaKasraNS ShaddaKasratanNS ShaddaNS SharetKafNS SukunNS ThreeDotsDownAboveNS
ThreeDotsUpAboveNS TwoDotsAboveNS TwoDotsVerticalAboveNS UltapeshNS WaslaNS
AinIni.12m_MeemFin.02 AinIni_YehBarreeFin AinMed_YehBarreeFin BehxIni_MeemFin
BehxIni_NoonGhunnaFin BehxIni_RehFin BehxIni_RehFin.b BehxMed_MeemFin.py
BehxMed_NoonGhunnaFin BehxMed_NoonGhunnaFin.cup BehxMed_RehFin BehxMed_RehFin.cup
BehxMed_YehxFin FehxMed_YehBarreeFin HahIni_YehBarreeFin KafIni_YehBarreeFin
KafMed.12_YehxFin.01 KafMed_MeemFin KafMed_YehBarreeFin LamAlefFin LamAlefFin.cup
LamAlefFin.cut LamAlefFin.short LamAlefSep LamIni_MeemFin LamIni_YehBarreeFin
LamMed_MeemFin LamMed_MeemFin.b LamMed_YehxFin LamMed_YehxFin.cup TahIni_YehBarreeFin
null CR space exclam quotedbl numbersign";

fn glyph_order() -> Vec<String> {
	GLYPH_ORDER.split_whitespace().map(str::to_owned).collect()
}

/// `create_font`.
fn create_font() -> Font {
	let mut font = Font::new();
	font.set_glyph_order(glyph_order());
	font
}

/// `getpath`.
fn getpath(name: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/mtiLib/data")
		.join(name)
}

/// Reads a text file as Python's text mode does (universal newlines).
fn read_text(path: &PathBuf) -> TestResult<String> {
	Ok(std::fs::read_to_string(path)?.replace("\r\n", "\n"))
}

/// The TTX of a table: the XML declaration and the table element.
fn table_xml(table: &dyn Table, font: &mut Font) -> TestResult<String> {
	let mut writer = XmlWriter::new();
	let tag = table.tag().as_string();
	writer.begintag(&tag, &[]);
	writer.newline();
	table.to_xml(&mut writer, font)?;
	writer.endtag(&tag);
	writer.newline();
	Ok(writer.into_string())
}

/// `expect_ttx`.
fn expect_ttx(expected: &str, actual: &str, what: &str) {
	if expected != actual {
		let e: Vec<&str> = expected.split('\n').collect();
		let a: Vec<&str> = actual.split('\n').collect();
		for (i, (x, y)) in e.iter().zip(&a).enumerate() {
			if x != y {
				eprintln!("first difference at line {}:\n- {x}\n+ {y}", i + 1);
				break;
			}
		}
		panic!("TTX output is different from expected ({what})");
	}
}

/// `check_mti_file`, with the lookup debugging enabled as the
/// `set_lookup_debug_env_var` fixture does.
fn check_mti_file(name: &str, table_tag: Option<&str>) -> TestResult {
	let xml_expected_path = getpath(&match table_tag {
		Some(t) => format!("{name}.ttx.{t}"),
		None => format!("{name}.ttx"),
	});
	let xml_expected = read_text(&xml_expected_path)?;

	let mut font = create_font();
	let text = read_text(&getpath(&format!("{name}.txt")))?;
	let mut table = build_with(
		&text,
		&mut font,
		&BuildOptions {
			table_tag: table_tag.map(str::to_owned),
			debug: true,
			..BuildOptions::default()
		},
	)?;
	let tag = table.tag();
	if let Some(t) = table_tag {
		assert_eq!(tag.as_string(), t);
	}

	// Make sure it compiles.
	let blob = table.compile(&mut font)?;
	// Make sure it decompiles.
	let decompiled = fontoxide_ttlib::table::decompile_table(tag, &blob, &mut font)?;

	let xml_built = table_xml(table.as_ref(), &mut font)?;
	let xml_binary = table_xml(decompiled.as_ref(), &mut font)?;
	expect_ttx(&xml_binary, &xml_built, "decompiled / built");
	expect_ttx(&xml_expected, &xml_built, "expected / built");

	// XML from the object read from XML.
	let mut font2 = Font::new();
	font2.set_glyph_order(glyph_order());
	let body = xml_expected
		.split_once('\n')
		.map_or(xml_expected.as_str(), |(_, b)| b);
	font2.import_ttx(&format!(
		"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ttFont>\n{body}</ttFont>\n"
	))?;
	let from_xml = font2
		.get_dyn(tag)?
		.ok_or("the table was not read from TTX")?
		.clone_table();
	let xml_fromxml = table_xml(from_xml.as_ref(), &mut font)?;
	expect_ttx(&xml_expected, &xml_fromxml, "expected / fromxml");
	Ok(())
}

macro_rules! mti_tests {
	($($test:ident: $name:literal, $tag:expr;)*) => {
		$(
			#[test]
			#[allow(non_snake_case)]
			fn $test() -> TestResult {
				check_mti_file($name, $tag)
			}
		)*
	};
}

mti_tests! {
	test_MtiFile_mti_cmap: "mti/cmap", None;
	test_MtiFile_mti_cmap_cmap: "mti/cmap", Some("cmap");
	test_MtiFile_featurename_backward_GSUB: "featurename-backward", Some("GSUB");
	test_MtiFile_featurename_forward_GSUB: "featurename-forward", Some("GSUB");
	test_MtiFile_lookupnames_backward_GSUB: "lookupnames-backward", Some("GSUB");
	test_MtiFile_lookupnames_forward_GSUB: "lookupnames-forward", Some("GSUB");
	test_MtiFile_mixed_toplevels_GSUB: "mixed-toplevels", Some("GSUB");
	test_MtiFile_mti_scripttable_GSUB: "mti/scripttable", Some("GSUB");
	test_MtiFile_mti_chainedclass_GSUB: "mti/chainedclass", Some("GSUB");
	test_MtiFile_mti_chainedcoverage_GSUB: "mti/chainedcoverage", Some("GSUB");
	test_MtiFile_mti_chained_glyph_GSUB: "mti/chained-glyph", Some("GSUB");
	test_MtiFile_mti_gsubalternate_GSUB: "mti/gsubalternate", Some("GSUB");
	test_MtiFile_mti_gsubligature_GSUB: "mti/gsubligature", Some("GSUB");
	test_MtiFile_mti_gsubmultiple_GSUB: "mti/gsubmultiple", Some("GSUB");
	test_MtiFile_mti_gsubreversechanined_GSUB: "mti/gsubreversechanined", Some("GSUB");
	test_MtiFile_mti_gsubsingle_GSUB: "mti/gsubsingle", Some("GSUB");
	test_MtiFile_mti_scripttable_GPOS: "mti/scripttable", Some("GPOS");
	test_MtiFile_mti_chained_glyph_GPOS: "mti/chained-glyph", Some("GPOS");
	test_MtiFile_mti_gposcursive_GPOS: "mti/gposcursive", Some("GPOS");
	test_MtiFile_mti_gposkernset_GPOS: "mti/gposkernset", Some("GPOS");
	test_MtiFile_mti_gposmarktobase_GPOS: "mti/gposmarktobase", Some("GPOS");
	test_MtiFile_mti_gpospairclass_GPOS: "mti/gpospairclass", Some("GPOS");
	test_MtiFile_mti_gpospairglyph_GPOS: "mti/gpospairglyph", Some("GPOS");
	test_MtiFile_mti_gpossingle_GPOS: "mti/gpossingle", Some("GPOS");
	test_MtiFile_mti_mark_to_ligature_GPOS: "mti/mark-to-ligature", Some("GPOS");
	test_MtiFile_mti_gdefattach_GDEF: "mti/gdefattach", Some("GDEF");
	test_MtiFile_mti_gdefclasses_GDEF: "mti/gdefclasses", Some("GDEF");
	test_MtiFile_mti_gdefligcaret_GDEF: "mti/gdefligcaret", Some("GDEF");
	test_MtiFile_mti_gdefmarkattach_GDEF: "mti/gdefmarkattach", Some("GDEF");
	test_MtiFile_mti_gdefmarkfilter_GDEF: "mti/gdefmarkfilter", Some("GDEF");
}

// Beyond the upstream tests.

/// The tables compile to the same bytes as in fontTools
/// (`tools/reference/mtilib_fixtures.py`).
#[test]
fn compiled_tables_match_fonttools() -> TestResult {
	let fixtures = include_str!("data/compiled.txt");
	let mut count = 0;
	for line in fixtures.lines() {
		let (test, hex) = line.split_once(' ').ok_or("fixture line")?;
		let (name, tag) = match test.rsplit_once('_') {
			Some((n, t)) if ["GSUB", "GPOS", "GDEF", "cmap"].contains(&t) => (n, Some(t)),
			_ => (test, None),
		};
		let mut font = create_font();
		let text = read_text(&getpath(&format!("{name}.txt")))?;
		let mut table = build_with(
			&text,
			&mut font,
			&BuildOptions {
				table_tag: tag.map(str::to_owned),
				debug: true,
				..BuildOptions::default()
			},
		)?;
		let blob = table.compile(&mut font)?;
		let actual: String = blob.iter().map(|b| format!("{b:02x}")).collect();
		assert_eq!(actual, hex, "{test}");
		count += 1;
	}
	assert_eq!(count, 30);
	Ok(())
}

/// `main(args, font)` (as the test file's `__main__` runs it) prints the TTX
/// of the decompiled tables, which the tests compare with the expected files.
#[test]
fn main_with_font_prints_the_decompiled_tables() -> TestResult {
	let mut font = create_font();
	let a = getpath("mti/gsubsingle.txt");
	let b = getpath("mti/gsubligature.txt");
	let mut out = Vec::new();
	let code = fontoxide_mtilib::cli::main_with(
		&[
			"-t",
			"GSUB",
			a.to_str().ok_or("path")?,
			b.to_str().ok_or("path")?,
		],
		Some(&mut font),
		&mut out,
	)?;
	assert_eq!(code, 0);
	let expected = read_text(&getpath("mti/gsubsingle.ttx.GSUB"))?
		+ &read_text(&getpath("mti/gsubligature.ttx.GSUB"))?;
	// Without `FONTTOOLS_LOOKUP_DEBUGGING`, there are no debugging comments.
	let expected: String = expected
		.split_inclusive('\n')
		.filter(|l| !(l.trim_start().starts_with("<!-- ") && l.trim_end().ends_with(":  -->")))
		.collect();
	expect_ttx(&expected, &String::from_utf8(out)?, "main");
	Ok(())
}

#[test]
fn main_sniffs_the_table() -> TestResult {
	let mut font = create_font();
	let a = getpath("mti/cmap.txt");
	let mut out = Vec::new();
	fontoxide_mtilib::cli::main_with(&[a.to_str().ok_or("path")?], Some(&mut font), &mut out)?;
	expect_ttx(
		&read_text(&getpath("mti/cmap.ttx"))?,
		&String::from_utf8(out)?,
		"main",
	);
	Ok(())
}

#[test]
fn main_without_font_adds_glyphs() -> TestResult {
	let a = getpath("mti/gsubsingle.txt");
	let mut out = Vec::new();
	fontoxide_mtilib::cli::main_with(&["--table=GSUB", a.to_str().ok_or("path")?], None, &mut out)?;
	let out = String::from_utf8(out)?;
	assert!(out.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<GSUB>\n"));
	assert!(out.contains("<Substitution in=\"a\" out=\"b\"/>") || out.contains("<SingleSubst"));
	Ok(())
}

#[test]
fn main_usage_errors() {
	let mut out = Vec::new();
	let err = fontoxide_mtilib::cli::main_with(&["-t", "GSUB"], None, &mut out);
	let Err(fontoxide_mtilib::cli::Error::Usage(message)) = err else {
		panic!("expected a usage error");
	};
	assert_eq!(
		message,
		"usage: fonttools mtiLib [-h] [--font FILE] [--table TABLE] FILE [FILE ...]\n\
		 fonttools mtiLib: error: the following arguments are required: FILE"
	);
	let err = fontoxide_mtilib::cli::main_with(&["x.txt", "--table"], None, &mut out);
	let Err(fontoxide_mtilib::cli::Error::Usage(message)) = err else {
		panic!("expected a usage error");
	};
	assert!(message.ends_with("error: argument --table/-t: expected one argument"));
	let code = fontoxide_mtilib::cli::main_with(&["-h"], None, &mut out);
	assert!(matches!(code, Ok(0)));
	assert_eq!(String::from_utf8_lossy(&out), fontoxide_mtilib::cli::HELP);
}

fn build_gsub(src: &str) -> Result<Box<dyn Table>, Error> {
	let mut font = Font::new();
	font.set_glyph_order(vec![".notdef".into(), "a".into(), "b".into(), "c".into()]);
	fontoxide_mtilib::build(src, &mut font, Some("GSUB"))
}

/// UPSTREAM-BUG(B58): scripts with tags of fewer than four characters get a record
/// for each of their lines.
#[test]
fn short_script_tags_are_not_merged() -> TestResult {
	let src = "lookup\tl0\tsingle\na\tb\nlookup end\n\nscript table begin\nlat\tdefault\t\t0\n\
		lat\tTRK\t\t0\nscript table end\n\nfeature table begin\n0\tliga\tl0\nfeature table end\n";
	let mut table = build_gsub(src)?;
	let mut font = Font::new();
	font.set_glyph_order(vec![".notdef".into(), "a".into(), "b".into(), "c".into()]);
	let xml = table_xml(table.as_ref(), &mut font)?;
	assert_eq!(xml.matches("<ScriptTag value=\"lat \"/>").count(), 2);
	table.compile(&mut font)?;
	// Four-character tags are merged.
	let src = src.replace("lat\t", "latn\t");
	let table = build_gsub(&src)?;
	let xml = table_xml(table.as_ref(), &mut font)?;
	assert_eq!(xml.matches("<ScriptTag value=\"latn\"/>").count(), 1);
	Ok(())
}

/// UPSTREAM-BUG(B59): fontTools loops forever on a `Subtable End` that is not in
/// lowercase.
#[test]
fn mixed_case_subtable_end_is_an_error() {
	let src = "lookup\tl0\tsingle\na\tb\nSubtable End\nb\tc\nlookup end\n";
	assert!(build_gsub(src).is_err());
	// In lowercase, it separates two subtables.
	let src = "lookup\tl0\tsingle\na\tb\nsubtable end\nb\tc\nlookup end\n";
	let Ok(table) = build_gsub(src) else {
		panic!("lowercase separators are fine");
	};
	let mut font = Font::new();
	font.set_glyph_order(vec![".notdef".into(), "a".into(), "b".into(), "c".into()]);
	let xml = table_xml(table.as_ref(), &mut font).expect("xml");
	assert!(xml.contains("<!-- SubTableCount=2 -->"));
}

/// UPSTREAM-BUG(B60): a numeric feature name that is not the feature's index makes
/// fontTools fail formatting its assertion message (a TypeError).
#[test]
fn numeric_feature_name_must_be_its_index() {
	let src = "lookup\tl0\tsingle\na\tb\nlookup end\nfeature table begin\n1\tliga\tl0\nfeature table end\n";
	let Err(Error::Value(message)) = build_gsub(src) else {
		panic!("expected an error");
	};
	assert!(message.starts_with("TypeError"));
}

#[test]
fn missing_references() {
	let src = "lookup\tl0\tchained\nglyph\t\ta\t\t1,nope\nlookup end\n";
	assert!(matches!(build_gsub(src), Err(Error::LookupNotFound(s)) if s == "nope"));
	let src = "lookup\tl0\tsingle\na\tb\nlookup end\nscript table begin\nlatn\tdefault\t\tnope\nscript table end\n";
	assert!(matches!(build_gsub(src), Err(Error::FeatureNotFound(s)) if s == "nope"));
}

#[test]
fn make_glyph() -> Result<(), Error> {
	assert_eq!(fontoxide_mtilib::make_glyph("U 41")?, "A");
	assert_eq!(fontoxide_mtilib::make_glyph("u E000")?, "uniE000");
	assert_eq!(fontoxide_mtilib::make_glyph("# 12")?, "glyph00012");
	assert_eq!(fontoxide_mtilib::make_glyph("a.sc")?, "a.sc");
	assert!(fontoxide_mtilib::make_glyph("a b").is_err());
	assert!(fontoxide_mtilib::make_glyph("").is_err());
	Ok(())
}
