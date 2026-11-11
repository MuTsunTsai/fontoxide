//! `Tests/feaLib/parser_test.py`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Once;

use fontoxide_fealib::ast::*;
use fontoxide_fealib::lexer::FeatureSource;
use fontoxide_fealib::parser::{Parser, SymbolTable};
use fontoxide_fealib::{Error, Result};

const GLYPHNAMES_TEXT: &str = "
	.notdef space A B C D E F G H I J K L M N O P Q R S T U V W X Y Z
	A.sc B.sc C.sc D.sc E.sc F.sc G.sc H.sc I.sc J.sc K.sc L.sc M.sc
	N.sc O.sc P.sc Q.sc R.sc S.sc T.sc U.sc V.sc W.sc X.sc Y.sc Z.sc
	A.swash B.swash X.swash Y.swash Z.swash
	a b c d e f g h i j k l m n o p q r s t u v w x y z
	a.sc b.sc c.sc d.sc e.sc f.sc g.sc h.sc i.sc j.sc k.sc l.sc m.sc
	n.sc o.sc p.sc q.sc r.sc s.sc t.sc u.sc v.sc w.sc x.sc y.sc z.sc
	a.swash b.swash x.swash y.swash z.swash
	foobar foo.09 foo.1234 foo.9876
	one two five six acute grave dieresis umlaut cedilla ogonek macron
	a_f_f_i o_f_f_i f_i f_l f_f_i one.fitted one.oldstyle a.1 a.2 a.3 c_t
	PRE SUF FIX BACK TRACK LOOK AHEAD ampersand ampersand.1 ampersand.2
	cid00001 cid00002 cid00003 cid00004 cid00005 cid00006 cid00007
	cid12345 cid78987 cid00999 cid01000 cid01001 cid00998 cid00995
	cid00111 cid00222
	comma endash emdash figuredash damma hamza
	c_d d.alt n.end s.end f_f
";

/// `GLYPHNAMES`.
fn glyphnames() -> Vec<String> {
	let mut names: Vec<String> = GLYPHNAMES_TEXT
		.split_whitespace()
		.map(str::to_owned)
		.collect();
	names.extend((1..200).map(|i| format!("foo.{i}")));
	names.push("G".repeat(600));
	names
}

/// `self.parse(text)`.
fn parse(text: &str) -> Result<FeatureFile> {
	parse_with(text, glyphnames(), true)
}

/// `self.parse(text, glyphNames, followIncludes)`.
fn parse_with(text: &str, glyph_names: Vec<String>, follow_includes: bool) -> Result<FeatureFile> {
	Parser::with_options(
		FeatureSource::text(text),
		glyph_names,
		follow_includes,
		None,
	)?
	.parse()
}

fn getpath(testfile: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/feaLib/data")
		.join(testfile)
}

/// `assertRaisesRegex(FeatureLibError, expected, self.parse, text)`.
#[track_caller]
fn assert_error(expected: &str, result: Result<FeatureFile>) {
	match result {
		Ok(_) => panic!("expected an error matching {expected:?}"),
		Err(e @ Error::FeatureLib { .. }) => {
			assert!(
				e.to_string().contains(expected),
				"{e} does not contain {expected:?}"
			);
		}
		Err(e) => panic!("expected a FeatureLibError, got {e:?}"),
	}
}

#[track_caller]
fn assert_parse_error(expected: &str, text: &str) {
	assert_error(expected, parse(text));
}

/// `glyphstr(glyphs)`.
fn glyphstr<'a>(glyphs: impl IntoIterator<Item = &'a GlyphContainer>) -> String {
	glyphs
		.into_iter()
		.map(|g| {
			let set = g.glyph_set();
			if set.len() == 1 {
				set[0].clone()
			} else {
				let mut sorted = set.to_vec();
				sorted.sort();
				format!("[{}]", sorted.join(" "))
			}
		})
		.collect::<Vec<_>>()
		.join(" ")
}

/// `mapping(s)`.
fn mapping(glyphs: &[GlyphContainer], replacements: &[GlyphContainer]) -> BTreeMap<String, String> {
	let b: Vec<String> = glyphs.iter().flat_map(|a| a.glyph_set().to_vec()).collect();
	let mut c: Vec<String> = replacements
		.iter()
		.flat_map(|a| a.glyph_set().to_vec())
		.collect();
	if c.len() == 1 {
		c = vec![c[0].clone(); b.len()];
	}
	b.into_iter().zip(c).collect()
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
	pairs
		.iter()
		.map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
		.collect()
}

fn strs(v: &[&str]) -> Vec<String> {
	v.iter().map(|s| (*s).to_owned()).collect()
}

/// Downcasts a statement to a variant, failing the test otherwise.
macro_rules! get {
	($e:expr, $v:ident) => {
		match $e {
			Statement::$v(x) => x,
			other => panic!("expected {}, got {:?}", stringify!($v), other),
		}
	};
}

/// The statements of a block statement.
fn stmts(s: &Statement) -> &[Statement] {
	match s.statements() {
		Some(s) => s,
		None => panic!("not a block: {s:?}"),
	}
}

/// The first statement of the first block.
fn first(doc: &FeatureFile) -> &Statement {
	&stmts(&doc.statements[0])[0]
}

fn int(v: &ScalarValue) -> i64 {
	match v {
		ScalarValue::Int(i) => *i,
		ScalarValue::Variable(v) => panic!("not an int: {v}"),
	}
}

fn anchor_xy(a: &Option<Anchor>) -> (i64, i64) {
	match a {
		Some(a) => (int(&a.x), int(&a.y)),
		None => panic!("no anchor"),
	}
}

fn opt_int(v: &Option<ScalarValue>) -> Option<i64> {
	v.as_ref().map(int)
}

fn vr_fea(v: &Option<ValueRecord>) -> String {
	match v {
		Some(v) => v.as_fea(),
		None => panic!("no value record"),
	}
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

/// The warnings `f` logs on this thread (`CapturingLogHandler(level="WARNING")`).
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

#[test]
fn test_comments() -> Result<()> {
	let doc = parse(
		" # Initial
                feature test {
                    sub A by B; # simple
                } test;",
	)?;
	let c1 = get!(&doc.statements[0], Comment);
	let c2 = get!(&stmts(&doc.statements[1])[1], Comment);
	assert_eq!(c1.text, "# Initial");
	assert_eq!(c1.as_fea(), "# Initial");
	assert_eq!(c2.text, "# simple");
	assert_eq!(get!(&doc.statements[1], FeatureBlock).name, "test");
	Ok(())
}

#[test]
fn test_only_comments() -> Result<()> {
	let doc = parse("            # Initial\n        ")?;
	let c1 = get!(&doc.statements[0], Comment);
	assert_eq!(c1.text, "# Initial");
	assert_eq!(c1.as_fea(), "# Initial");
	Ok(())
}

fn entry_anchor(doc: &FeatureFile, index: usize) -> Option<Anchor> {
	get!(&stmts(&doc.statements[0])[index], CursivePosStatement)
		.entry_anchor
		.clone()
}

#[test]
fn test_anchor_format_a() -> Result<()> {
	let doc = parse("feature test {    pos cursive A <anchor 120 -20> <anchor NULL>;} test;")?;
	let Some(anchor) = entry_anchor(&doc, 0) else {
		panic!("no anchor")
	};
	assert_eq!(int(&anchor.x), 120);
	assert_eq!(int(&anchor.y), -20);
	assert_eq!(anchor.contourpoint, None);
	assert_eq!(anchor.x_device_table, None);
	assert_eq!(anchor.y_device_table, None);
	Ok(())
}

#[test]
fn test_anchor_format_b() -> Result<()> {
	let doc = parse(
		"feature test {    pos cursive A <anchor 120 -20 contourpoint 5> <anchor NULL>;} test;",
	)?;
	let Some(anchor) = entry_anchor(&doc, 0) else {
		panic!("no anchor")
	};
	assert_eq!(int(&anchor.x), 120);
	assert_eq!(int(&anchor.y), -20);
	assert_eq!(anchor.contourpoint, Some(5));
	assert_eq!(anchor.x_device_table, None);
	assert_eq!(anchor.y_device_table, None);
	Ok(())
}

#[test]
fn test_anchor_format_c() -> Result<()> {
	let doc = parse(
		"feature test {    pos cursive A         <anchor 120 -20 <device 11 111, 12 112> <device NULL>>        <anchor NULL>;} test;",
	)?;
	let Some(anchor) = entry_anchor(&doc, 0) else {
		panic!("no anchor")
	};
	assert_eq!(int(&anchor.x), 120);
	assert_eq!(int(&anchor.y), -20);
	assert_eq!(anchor.contourpoint, None);
	assert_eq!(anchor.x_device_table, Some(vec![(11, 111), (12, 112)]));
	assert_eq!(anchor.y_device_table, None);
	Ok(())
}

#[test]
fn test_anchor_format_d() -> Result<()> {
	let doc = parse("feature test {    pos cursive A <anchor 120 -20> <anchor NULL>;} test;")?;
	let pos = get!(first(&doc), CursivePosStatement);
	assert!(pos.exit_anchor.is_none());
	Ok(())
}

#[test]
fn test_anchor_format_e() -> Result<()> {
	let doc = parse(
		"feature test {    anchorDef 120 -20 contourpoint 7 Foo;    pos cursive A <anchor Foo> <anchor NULL>;} test;",
	)?;
	let Some(anchor) = entry_anchor(&doc, 1) else {
		panic!("no anchor")
	};
	assert_eq!(int(&anchor.x), 120);
	assert_eq!(int(&anchor.y), -20);
	assert_eq!(anchor.contourpoint, Some(7));
	assert_eq!(anchor.x_device_table, None);
	assert_eq!(anchor.y_device_table, None);
	Ok(())
}

#[test]
fn test_anchor_format_e_undefined() {
	assert_parse_error(
		"Unknown anchor \"UnknownName\"",
		"feature test {    position cursive A <anchor UnknownName> <anchor NULL>;} test;",
	);
}

#[test]
fn test_anchor_variable_scalar() -> Result<()> {
	let doc = parse(
		"feature test {    pos cursive A <anchor (wght=200:-100 wght=900:-150 wdth=150.5,wght=900:-120) -20> <anchor NULL>;} test;",
	)?;
	let Some(anchor) = entry_anchor(&doc, 0) else {
		panic!("no anchor")
	};
	assert_eq!(
		anchor.as_fea(),
		"<anchor (wght=200:-100 wght=900:-150 wdth=150.5,wght=900:-120) -20>"
	);
	Ok(())
}

#[test]
fn test_anchordef() -> Result<()> {
	let doc = parse("anchorDef 123 456 foo;")?;
	assert_eq!(doc.statements.len(), 1);
	let foo = get!(&doc.statements[0], AnchorDefinition);
	assert_eq!(foo.name, "foo");
	assert_eq!(foo.x, 123);
	assert_eq!(foo.y, 456);
	assert_eq!(foo.contourpoint, None);
	Ok(())
}

#[test]
fn test_anchordef_contourpoint() -> Result<()> {
	let doc = parse("anchorDef 123 456 contourpoint 5 foo;")?;
	assert_eq!(doc.statements.len(), 1);
	let foo = get!(&doc.statements[0], AnchorDefinition);
	assert_eq!(foo.name, "foo");
	assert_eq!(foo.x, 123);
	assert_eq!(foo.y, 456);
	assert_eq!(foo.contourpoint, Some(5));
	Ok(())
}

#[test]
fn test_anon() -> Result<()> {
	let doc = parse("anon TEST { # a\nfoo\n } TEST; # qux")?;
	let anon = get!(&doc.statements[0], AnonymousBlock);
	assert_eq!(anon.tag, "TEST");
	assert_eq!(anon.content, "foo\n ");
	Ok(())
}

#[test]
fn test_anonymous() -> Result<()> {
	let doc = parse("anonymous TEST {\nbar\n} TEST;")?;
	let anon = get!(&doc.statements[0], AnonymousBlock);
	assert_eq!(anon.tag, "TEST");
	// feature file spec requires passing the final end-of-line
	assert_eq!(anon.content, "bar\n");
	Ok(())
}

#[test]
fn test_anon_missing_brace() {
	assert_parse_error(
		"Expected '} TEST;' to terminate anonymous block",
		"anon TEST { \n no end in sight",
	);
}

#[test]
fn test_anon_regex_metacharacter_tag() -> Result<()> {
	// the closing tag is matched literally
	let doc = parse("anon .+.+ {\n} X;\n} .+.+;\n")?;
	let anon = get!(&doc.statements[0], AnonymousBlock);
	assert_eq!(anon.tag, ".+.+");
	assert_eq!(anon.content, "} X;\n");
	Ok(())
}

#[test]
fn test_anon_invalid_regex_tag() {
	assert_parse_error(
		"Expected '} *;' to terminate anonymous block",
		"anon * {\n no end in sight",
	);
}

#[test]
fn test_attach() -> Result<()> {
	let doc = parse("table GDEF {Attach [a e] 2;} GDEF;")?;
	let s = get!(first(&doc), AttachStatement);
	assert_eq!(glyphstr([&s.glyphs]), "[a e]");
	assert_eq!(
		s.contour_points.iter().copied().collect::<Vec<_>>(),
		vec![2]
	);
	Ok(())
}

#[test]
fn test_feature_block() -> Result<()> {
	let doc = parse("feature liga {} liga;")?;
	assert_eq!(doc.statements.len(), 1);
	let liga = get!(&doc.statements[0], FeatureBlock);
	assert_eq!(liga.name, "liga");
	assert!(!liga.use_extension);
	Ok(())
}

#[test]
fn test_feature_block_use_extension() -> Result<()> {
	let doc = parse("feature liga useExtension {} liga;")?;
	let liga = get!(&doc.statements[0], FeatureBlock);
	assert_eq!(liga.name, "liga");
	assert!(liga.use_extension);
	assert_eq!(
		liga.as_fea("")?,
		"feature liga useExtension {\n    \n} liga;\n"
	);
	Ok(())
}

#[test]
fn test_feature_comment() -> Result<()> {
	let doc = parse("feature liga { # Comment\n } liga;")?;
	let liga = stmts(&doc.statements[0]);
	assert_eq!(liga.len(), 1);
	assert_eq!(get!(&liga[0], Comment).text, "# Comment");
	Ok(())
}

#[test]
fn test_feature_reference() -> Result<()> {
	let doc = parse("feature aalt { feature salt; } aalt;")?;
	let r = get!(first(&doc), FeatureReferenceStatement);
	assert_eq!(r.feature_name, "salt");
	Ok(())
}

#[test]
fn test_feature_names_bad() {
	assert_parse_error(
		"Expected \"name\"",
		"feature ss01 { featureNames { feature test; } ss01;",
	);
}

#[test]
fn test_feature_names_comment() -> Result<()> {
	let doc = parse("feature ss01 { featureNames { # Comment\n }; } ss01;")?;
	let feature = stmts(&doc.statements[0]);
	assert_eq!(feature.len(), 1);
	let feature_names = get!(&feature[0], NestedBlock);
	assert_eq!(feature_names.statements.len(), 1);
	assert_eq!(
		get!(&feature_names.statements[0], Comment).text,
		"# Comment"
	);
	Ok(())
}

#[test]
fn test_feature_names_empty_statements() -> Result<()> {
	let doc = parse("feature ss01 { featureNames { ;;; }; } ss01;")?;
	let feature_names = get!(first(&doc), NestedBlock);
	assert!(feature_names.statements.is_empty());
	Ok(())
}

#[test]
fn test_font_revision() -> Result<()> {
	let doc = parse("table head {FontRevision 2.5;} head;")?;
	let s = get!(first(&doc), FontRevisionStatement);
	assert_eq!(s.revision, 2.5);
	Ok(())
}

#[test]
fn test_font_revision_negative() {
	assert_parse_error(
		"Font revision numbers must be positive",
		"table head {FontRevision -17.2;} head;",
	);
}

#[test]
fn test_strict_glyph_name_check() -> Result<()> {
	parse_with("@bad = [a b ccc];", strs(&["a", "b", "ccc"]), true)?;
	match parse_with("@bad = [a b ccc];", strs(&["a", "b"]), true) {
		Err(e) => {
			let e = e.to_string();
			assert!(
				e.contains("missing from the glyph set:") && e.contains("ccc"),
				"{e}"
			);
		}
		Ok(_) => panic!("expected an error"),
	}
	Ok(())
}

fn class_def(s: &Statement) -> &GlyphClassDefinition {
	get!(s, GlyphClassDefinition)
}

#[test]
fn test_glyphclass() -> Result<()> {
	let doc = parse("@dash = [endash emdash figuredash];")?;
	let gc = class_def(&doc.statements[0]);
	assert_eq!(gc.name, "dash");
	assert_eq!(*gc.glyph_set(), strs(&["endash", "emdash", "figuredash"]));
	Ok(())
}

#[test]
fn test_glyphclass_glyph_name_too_long() -> Result<()> {
	let gname = "G".repeat(600);
	let doc = parse(&format!("@GlyphClass = [{gname}];"))?;
	let gc = class_def(&doc.statements[0]);
	assert_eq!(gc.name, "GlyphClass");
	assert_eq!(*gc.glyph_set(), vec![gname]);
	Ok(())
}

#[test]
fn test_glyphclass_bad() {
	assert_parse_error(
		"Expected glyph name, glyph range, or glyph class reference",
		"@bad = [a 123];",
	);
}

#[test]
fn test_glyphclass_duplicate() -> Result<()> {
	// makeotf accepts this, so we should too
	let doc = parse("@dup = [a b]; @dup = [x y];")?;
	assert_eq!(glyphstr([&class_def(&doc.statements[0]).glyphs]), "[a b]");
	assert_eq!(glyphstr([&class_def(&doc.statements[1]).glyphs]), "[x y]");
	Ok(())
}

#[test]
fn test_glyphclass_empty() -> Result<()> {
	let doc = parse("@empty_set = [];")?;
	let gc = class_def(&doc.statements[0]);
	assert_eq!(gc.name, "empty_set");
	assert!(gc.glyph_set().is_empty());
	Ok(())
}

#[test]
fn test_glyphclass_equality() -> Result<()> {
	let doc = parse("@foo = [a b]; @bar = @foo;")?;
	assert_eq!(
		*class_def(&doc.statements[0]).glyph_set(),
		strs(&["a", "b"])
	);
	assert_eq!(
		*class_def(&doc.statements[1]).glyph_set(),
		strs(&["a", "b"])
	);
	Ok(())
}

#[test]
fn test_glyphclass_from_mark_class() -> Result<()> {
	let doc = parse(
		"markClass [acute grave] <anchor 500 800> @TOP_MARKS;\
		 markClass cedilla <anchor 500 -100> @BOTTOM_MARKS;\
		 @MARKS = [@TOP_MARKS @BOTTOM_MARKS ogonek];\
		 @ALL = @MARKS;",
	)?;
	let last = doc.statements.last().map(class_def);
	assert_eq!(
		*last.map(|d| d.glyph_set().to_vec()).unwrap_or_default(),
		strs(&["acute", "grave", "cedilla", "ogonek"])
	);
	Ok(())
}

#[test]
fn test_glyphclass_range_cid() -> Result<()> {
	let doc = parse(r"@GlyphClass = [\999-\1001];")?;
	let gc = class_def(&doc.statements[0]);
	assert_eq!(gc.name, "GlyphClass");
	assert_eq!(*gc.glyph_set(), strs(&["cid00999", "cid01000", "cid01001"]));
	Ok(())
}

#[test]
fn test_glyphclass_range_cid_bad() {
	assert_parse_error(
		"Bad range: start should be less than limit",
		r"@bad = [\998-\995];",
	);
}

#[test]
fn test_glyphclass_range_uppercase() -> Result<()> {
	let doc = parse("@swashes = [X.swash-Z.swash];")?;
	let gc = class_def(&doc.statements[0]);
	assert_eq!(gc.name, "swashes");
	assert_eq!(*gc.glyph_set(), strs(&["X.swash", "Y.swash", "Z.swash"]));
	Ok(())
}

#[test]
fn test_glyphclass_range_lowercase() -> Result<()> {
	let doc = parse("@defg.sc = [d.sc-g.sc];")?;
	let gc = class_def(&doc.statements[0]);
	assert_eq!(gc.name, "defg.sc");
	assert_eq!(*gc.glyph_set(), strs(&["d.sc", "e.sc", "f.sc", "g.sc"]));
	Ok(())
}

#[test]
fn test_glyphclass_range_dash() -> Result<()> {
	let glyph_names = strs(&["A-foo.sc", "B-foo.sc", "C-foo.sc"]);
	let doc = parse_with("@range = [A-foo.sc-C-foo.sc];", glyph_names, true)?;
	assert_eq!(
		*class_def(&doc.statements[0]).glyph_set(),
		strs(&["A-foo.sc", "B-foo.sc", "C-foo.sc"])
	);
	Ok(())
}

#[test]
fn test_glyphclass_range_dash_with_space() -> Result<()> {
	let gn = strs(&["A-foo.sc", "B-foo.sc", "C-foo.sc"]);
	let doc = parse_with("@range = [A-foo.sc - C-foo.sc];", gn, true)?;
	assert_eq!(
		*class_def(&doc.statements[0]).glyph_set(),
		strs(&["A-foo.sc", "B-foo.sc", "C-foo.sc"])
	);
	Ok(())
}

#[test]
fn test_glyphclass_ambiguous_dash_no_glyph_names() -> Result<()> {
	// Without glyph names, a glyph name containing a hyphen cannot be told apart
	// from a range; it is taken as a glyph name, with a warning.
	let (records, doc) = capture(|| parse_with("@class = [A-foo.sc B-foo.sc C D];", vec![], true));
	let doc = doc?;
	assert_eq!(
		*class_def(&doc.statements[0]).glyph_set(),
		strs(&["A-foo.sc", "B-foo.sc", "C", "D"])
	);
	assert_eq!(records.len(), 2);
	assert!(
		records
			.iter()
			.any(|r| r.contains("Ambiguous glyph name that looks like a range:"))
	);
	Ok(())
}

#[test]
fn test_glyphclass_glyph_name_should_win_over_range() -> Result<()> {
	let glyph_names = strs(&["A-foo.sc-C-foo.sc", "A-foo.sc", "B-foo.sc", "C-foo.sc"]);
	let doc = parse_with("@range = [A-foo.sc-C-foo.sc];", glyph_names, true)?;
	assert_eq!(
		*class_def(&doc.statements[0]).glyph_set(),
		strs(&["A-foo.sc-C-foo.sc"])
	);
	Ok(())
}

#[test]
fn test_glyphclass_range_dash_ambiguous() {
	let glyph_names = strs(&["A", "B", "C", "A-B", "B-C"]);
	assert_error(
		"Ambiguous glyph range \"A-B-C\"; please use \"A - B-C\" or \"A-B - C\" to clarify what you mean",
		parse_with(r"@bad = [A-B-C];", glyph_names, true),
	);
}

#[test]
fn test_glyphclass_range_digit1() -> Result<()> {
	let doc = parse("@range = [foo.2-foo.5];")?;
	assert_eq!(
		*class_def(&doc.statements[0]).glyph_set(),
		strs(&["foo.2", "foo.3", "foo.4", "foo.5"])
	);
	Ok(())
}

#[test]
fn test_glyphclass_range_digit2() -> Result<()> {
	let doc = parse("@range = [foo.09-foo.11];")?;
	assert_eq!(
		*class_def(&doc.statements[0]).glyph_set(),
		strs(&["foo.09", "foo.10", "foo.11"])
	);
	Ok(())
}

#[test]
fn test_glyphclass_range_digit3() -> Result<()> {
	let doc = parse("@range = [foo.123-foo.125];")?;
	assert_eq!(
		*class_def(&doc.statements[0]).glyph_set(),
		strs(&["foo.123", "foo.124", "foo.125"])
	);
	Ok(())
}

#[test]
fn test_glyphclass_range_bad() {
	assert_parse_error(
		"Bad range: \"a\" and \"foobar\" should have the same length",
		"@bad = [a-foobar];",
	);
	assert_parse_error(
		"Bad range: \"A.swash-z.swash\"",
		"@bad = [A.swash-z.swash];",
	);
	assert_parse_error(
		"Start of range must be smaller than its end",
		"@bad = [B.swash-A.swash];",
	);
	assert_parse_error(
		"Bad range: \"foo.1234-foo.9876\"",
		"@bad = [foo.1234-foo.9876];",
	);
}

#[test]
fn test_glyphclass_range_mixed() -> Result<()> {
	let doc = parse("@range = [a foo.09-foo.11 X.sc-Z.sc];")?;
	assert_eq!(
		*class_def(&doc.statements[0]).glyph_set(),
		strs(&["a", "foo.09", "foo.10", "foo.11", "X.sc", "Y.sc", "Z.sc"])
	);
	Ok(())
}

#[test]
fn test_glyphclass_reference() -> Result<()> {
	let doc = parse(
		"@Vowels.lc = [a e i o u]; @Vowels.uc = [A E I O U];\
		 @Vowels = [@Vowels.lc @Vowels.uc y Y];",
	)?;
	let chars = |s: &str| s.chars().map(String::from).collect::<Vec<_>>();
	assert_eq!(*class_def(&doc.statements[0]).glyph_set(), chars("aeiou"));
	assert_eq!(*class_def(&doc.statements[1]).glyph_set(), chars("AEIOU"));
	let vowels = class_def(&doc.statements[2]);
	assert_eq!(*vowels.glyph_set(), chars("aeiouAEIOUyY"));
	assert_eq!(vowels.as_fea(), "@Vowels = [@Vowels.lc @Vowels.uc y Y];");
	assert_parse_error("Unknown glyph class @unknown", "@bad = [@unknown];");
	Ok(())
}

#[test]
fn test_glyphclass_scoping() -> Result<()> {
	let doc = parse(
		"@foo = [a b];\
		 feature liga { @bar = [@foo l]; } liga;\
		 feature smcp { @bar = [@foo s]; } smcp;",
	)?;
	assert_eq!(
		*class_def(&doc.statements[0]).glyph_set(),
		strs(&["a", "b"])
	);
	assert_eq!(
		*class_def(&stmts(&doc.statements[1])[0]).glyph_set(),
		strs(&["a", "b", "l"])
	);
	assert_eq!(
		*class_def(&stmts(&doc.statements[2])[0]).glyph_set(),
		strs(&["a", "b", "s"])
	);
	Ok(())
}

#[test]
fn test_glyphclass_scoping_bug496() -> Result<()> {
	// https://github.com/fonttools/fonttools/issues/496
	let doc = parse(
		"feature F1 { lookup L { @GLYPHCLASS = [A B C];} L; } F1;\
		 feature F2 { sub @GLYPHCLASS by D; } F2;",
	)?;
	let sub = get!(&stmts(&doc.statements[1])[0], SingleSubstStatement);
	assert_eq!(*sub.glyphs[0].glyph_set(), strs(&["A", "B", "C"]));
	Ok(())
}

#[test]
fn test_glyph_class_def() -> Result<()> {
	let doc = parse("table GDEF {GlyphClassDef [b],[l],[m],[C c];} GDEF;")?;
	let s = get!(first(&doc), GlyphClassDefStatement);
	assert_eq!(glyphstr(&s.base_glyphs), "b");
	assert_eq!(glyphstr(&s.ligature_glyphs), "l");
	assert_eq!(glyphstr(&s.mark_glyphs), "m");
	assert_eq!(glyphstr(&s.component_glyphs), "[C c]");
	Ok(())
}

#[test]
fn test_glyph_class_def_no_classes_specified() -> Result<()> {
	let doc = parse("table GDEF {GlyphClassDef ,,,;} GDEF;")?;
	let s = get!(first(&doc), GlyphClassDefStatement);
	assert!(s.base_glyphs.is_none());
	assert!(s.ligature_glyphs.is_none());
	assert!(s.mark_glyphs.is_none());
	assert!(s.component_glyphs.is_none());
	Ok(())
}

fn check_two_contexts(contexts: &[ChainContext]) {
	assert_eq!(contexts.len(), 2);
	let (pref1, glyphs1, suff1) = &contexts[0];
	let (pref2, glyphs2, suff2) = &contexts[1];
	assert_eq!(glyphstr(pref1), "e");
	assert_eq!(glyphstr(glyphs1), "t");
	assert_eq!(glyphstr(suff1), "c");
	assert_eq!(glyphstr(pref2), "q");
	assert_eq!(glyphstr(glyphs2), "u u");
	assert_eq!(glyphstr(suff2), "x");
}

fn check_one_context(contexts: &[ChainContext]) {
	assert_eq!(contexts.len(), 1);
	let (prefix, glyphs, suffix) = &contexts[0];
	assert_eq!(glyphstr(prefix), "f [a e]");
	assert_eq!(glyphstr(glyphs), "d [a u]");
	assert_eq!(glyphstr(suffix), "[e y]");
}

#[test]
fn test_ignore_pos() -> Result<()> {
	let doc = parse("feature test {ignore pos e t' c, q u' u' x;} test;")?;
	check_two_contexts(&get!(first(&doc), IgnorePosStatement).chain_contexts);
	Ok(())
}

#[test]
fn test_ignore_position() -> Result<()> {
	let doc = parse("feature test {    ignore position f [a e] d' [a u]' [e y];} test;")?;
	check_one_context(&get!(first(&doc), IgnorePosStatement).chain_contexts);
	Ok(())
}

#[test]
fn test_ignore_position_with_lookup() {
	assert_parse_error(
		"No lookups can be specified for \"ignore pos\"",
		"lookup L { pos [A A.sc] -100; } L;\
		 feature test { ignore pos f' i', A' lookup L; } test;",
	);
}

#[test]
fn test_ignore_sub() -> Result<()> {
	let doc = parse("feature test {ignore sub e t' c, q u' u' x;} test;")?;
	check_two_contexts(&get!(first(&doc), IgnoreSubstStatement).chain_contexts);
	Ok(())
}

#[test]
fn test_ignore_substitute() -> Result<()> {
	let doc = parse("feature test {    ignore substitute f [a e] d' [a u]' [e y];} test;")?;
	check_one_context(&get!(first(&doc), IgnoreSubstStatement).chain_contexts);
	Ok(())
}

#[test]
fn test_ignore_substitute_with_lookup() {
	assert_parse_error(
		"No lookups can be specified for \"ignore sub\"",
		"lookup L { sub [A A.sc] by a; } L;\
		 feature test { ignore sub f' i', A' lookup L; } test;",
	);
}

#[test]
fn test_include_statement() -> Result<()> {
	let doc = parse_with(
		"            include(../family.fea);
            include # Comment
                (foo)
                  ;
            ",
		glyphnames(),
		false,
	)?;
	assert_eq!(doc.statements.len(), 3);
	let s1 = get!(&doc.statements[0], IncludeStatement);
	assert_eq!(s1.filename, "../family.fea");
	assert_eq!(s1.as_fea(""), "include(../family.fea);");
	let s2 = get!(&doc.statements[1], IncludeStatement);
	assert_eq!(s2.filename, "foo");
	assert_eq!(s2.as_fea(""), "include(foo);");
	let s3 = get!(&doc.statements[2], Comment);
	assert_eq!(s3.text, "# Comment");
	Ok(())
}

#[test]
fn test_include_statement_no_semicolon() -> Result<()> {
	let doc = parse_with(
		"            include(../family.fea)\n            ",
		glyphnames(),
		false,
	)?;
	let s1 = get!(&doc.statements[0], IncludeStatement);
	assert_eq!(s1.filename, "../family.fea");
	assert_eq!(s1.as_fea(""), "include(../family.fea);");
	Ok(())
}

#[test]
fn test_language() -> Result<()> {
	let doc = parse("feature test {language DEU;} test;")?;
	let s = get!(first(&doc), LanguageStatement);
	assert_eq!(s.language(), "DEU ");
	assert!(s.include_default);
	assert!(!s.required);
	Ok(())
}

#[test]
fn test_language_multiple() -> Result<()> {
	let doc = parse("feature test {language AZE CRT KAZ TAT TRK exclude_dflt required;} test;")?;
	let s = get!(first(&doc), LanguageStatement);
	assert_eq!(s.languages, strs(&["AZE ", "CRT ", "KAZ ", "TAT ", "TRK "]));
	// LENIENT: Python also warns here to use `.languages`.
	assert_eq!(s.language(), "AZE ");
	assert!(!s.include_default);
	assert!(s.required);
	assert_eq!(
		s.as_fea(),
		"language AZE CRT KAZ TAT TRK exclude_dflt required;"
	);
	Ok(())
}

#[test]
fn test_language_multiple_missing_semicolon() {
	// The next statement must not be read as more language tags.
	assert_parse_error(
		"Expected ';'",
		"feature test {language AZE CRT\n sub a by b;} test;",
	);
	assert_parse_error(
		"Expected ';'",
		"feature test {language AZE\n lookup foo;} test;",
	);
	assert_parse_error(
		"Expected ';'",
		"feature test {language AZE CRT\n @foo = [a b];} test;",
	);
}

/// `parse_language_` ends a multi-tag statement at the keywords that start a
/// block statement; this fails when `parse_block_` learns a new one that
/// `STATEMENT_KEYWORDS` does not list. The keywords are read from the source of
/// `parse_block_`.
#[test]
fn test_language_statement_keywords_up_to_date() {
	let source = include_str!("../src/parser.rs");
	let start = source.find("fn parse_block_(").unwrap_or(0);
	let end = start + source[start..].find("fn is_cur_keyword_(").unwrap_or(0);
	let body = &source[start..end];
	let mut keywords = std::collections::BTreeSet::new();
	for call in ["is_cur_keyword_(", "is_cur_keyword_any("] {
		for (i, _) in body.match_indices(call) {
			let rest = &body[i + call.len()..];
			let args = &rest[..rest.find(')').unwrap_or(0)];
			for (j, part) in args.split('"').enumerate() {
				if j % 2 == 1 {
					keywords.insert(part.to_owned());
				}
			}
		}
	}
	assert!(keywords.contains("sub")); // the walk found the calls
	let missing: Vec<&String> = keywords
		.iter()
		.filter(|k| !Parser::STATEMENT_KEYWORDS.contains(&k.as_str()))
		.collect();
	assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn test_language_multiple_dflt() {
	assert_parse_error(
		"\"dflt\" must be the only tag in a language statement",
		"feature test {language dflt AZE;} test;",
	);
	assert_parse_error(
		"\"dflt\" must be the only tag in a language statement",
		"feature test {language AZE dflt;} test;",
	);
}

#[test]
fn test_language_exclude_dflt() -> Result<()> {
	let doc = parse("feature test {language DEU exclude_dflt;} test;")?;
	let s = get!(first(&doc), LanguageStatement);
	assert_eq!(s.language(), "DEU ");
	assert!(!s.include_default);
	assert!(!s.required);
	Ok(())
}

#[test]
fn test_language_exclude_dflt_required() -> Result<()> {
	let doc = parse("feature test {  language DEU exclude_dflt required;} test;")?;
	let s = get!(first(&doc), LanguageStatement);
	assert_eq!(s.language(), "DEU ");
	assert!(!s.include_default);
	assert!(s.required);
	Ok(())
}

#[test]
fn test_language_include_dflt() -> Result<()> {
	let doc = parse("feature test {language DEU include_dflt;} test;")?;
	let s = get!(first(&doc), LanguageStatement);
	assert_eq!(s.language(), "DEU ");
	assert!(s.include_default);
	assert!(!s.required);
	Ok(())
}

#[test]
fn test_language_include_dflt_required() -> Result<()> {
	let doc = parse("feature test {  language DEU include_dflt required;} test;")?;
	let s = get!(first(&doc), LanguageStatement);
	assert_eq!(s.language(), "DEU ");
	assert!(s.include_default);
	assert!(s.required);
	Ok(())
}

#[test]
fn test_language_dflt() {
	assert_parse_error(
		"\"DFLT\" is not a valid language tag; use \"dflt\" instead",
		"feature test { language DFLT; } test;",
	);
}

#[test]
fn test_ligature_caret_by_index_glyph_class() -> Result<()> {
	let doc = parse("table GDEF{LigatureCaretByIndex [c_t f_i] 2;}GDEF;")?;
	let s = get!(first(&doc), LigatureCaretByIndexStatement);
	assert_eq!(glyphstr([&s.glyphs]), "[c_t f_i]");
	assert_eq!(s.carets, vec![2]);
	Ok(())
}

#[test]
fn test_ligature_caret_by_index_single_glyph() -> Result<()> {
	let doc = parse("table GDEF{LigatureCaretByIndex f_f_i 3 7;}GDEF;")?;
	let s = get!(first(&doc), LigatureCaretByIndexStatement);
	assert_eq!(glyphstr([&s.glyphs]), "f_f_i");
	assert_eq!(s.carets, vec![3, 7]);
	Ok(())
}

#[test]
fn test_ligature_caret_by_pos_glyph_class() -> Result<()> {
	let doc = parse("table GDEF {LigatureCaretByPos [c_t f_i] 7;} GDEF;")?;
	let s = get!(first(&doc), LigatureCaretByPosStatement);
	assert_eq!(glyphstr([&s.glyphs]), "[c_t f_i]");
	assert_eq!(s.carets, vec![ScalarValue::Int(7)]);
	Ok(())
}

#[test]
fn test_ligature_caret_by_pos_single_glyph() -> Result<()> {
	let doc = parse("table GDEF {LigatureCaretByPos f_i 400 380;} GDEF;")?;
	let s = get!(first(&doc), LigatureCaretByPosStatement);
	assert_eq!(glyphstr([&s.glyphs]), "f_i");
	assert_eq!(s.carets, vec![ScalarValue::Int(400), ScalarValue::Int(380)]);
	Ok(())
}

#[test]
fn test_ligature_caret_by_pos_variable_scalar() -> Result<()> {
	let doc = parse("table GDEF {LigatureCaretByPos f_i (wght=200:400 wght=900:1000) 380;} GDEF;")?;
	let s = get!(first(&doc), LigatureCaretByPosStatement);
	assert_eq!(glyphstr([&s.glyphs]), "f_i");
	assert_eq!(s.carets.len(), 2);
	assert_eq!(s.carets[0].to_string(), "(wght=200:400 wght=900:1000)");
	assert_eq!(s.carets[1], ScalarValue::Int(380));
	Ok(())
}

fn lookup_block(s: &Statement) -> &Rc<LookupBlock> {
	get!(s, LookupBlock)
}

#[test]
fn test_lookup_block() -> Result<()> {
	let doc = parse("lookup Ligatures {} Ligatures;")?;
	let lookup = lookup_block(&doc.statements[0]);
	assert_eq!(lookup.name, "Ligatures");
	assert!(!lookup.use_extension);
	Ok(())
}

#[test]
fn test_lookup_block_use_extension() -> Result<()> {
	let doc = parse("lookup Foo useExtension {} Foo;")?;
	let lookup = lookup_block(&doc.statements[0]);
	assert_eq!(lookup.name, "Foo");
	assert!(lookup.use_extension);
	assert_eq!(
		lookup.as_fea("")?,
		"lookup Foo useExtension {\n    \n} Foo;\n"
	);
	Ok(())
}

#[test]
fn test_lookup_block_name_mismatch() {
	assert_parse_error("Expected \"Foo\"", "lookup Foo {} Bar;");
}

#[test]
fn test_lookup_block_with_horizontal_value_record_def() -> Result<()> {
	let doc = parse("feature liga {  lookup look {    valueRecordDef 123 foo;  } look;} liga;")?;
	let look = first(&doc);
	let foo = get!(&stmts(look)[0], ValueRecordDefinition);
	assert_eq!(opt_int(&foo.value.x_advance), Some(123));
	assert!(foo.value.y_advance.is_none());
	Ok(())
}

#[test]
fn test_lookup_block_with_vertical_value_record_def() -> Result<()> {
	let doc = parse("feature vkrn {  lookup look {    valueRecordDef 123 foo;  } look;} vkrn;")?;
	let look = first(&doc);
	let foo = get!(&stmts(look)[0], ValueRecordDefinition);
	assert!(foo.value.x_advance.is_none());
	assert_eq!(opt_int(&foo.value.y_advance), Some(123));
	Ok(())
}

#[test]
fn test_lookup_comment() -> Result<()> {
	let doc = parse("lookup L { # Comment\n } L;")?;
	let lookup = stmts(&doc.statements[0]);
	assert_eq!(lookup.len(), 1);
	assert_eq!(get!(&lookup[0], Comment).text, "# Comment");
	Ok(())
}

#[test]
fn test_lookup_reference() -> Result<()> {
	let doc = parse("lookup Foo {} Foo;feature Bar {lookup Foo;} Bar;")?;
	let foo = lookup_block(&doc.statements[0]);
	let bar = stmts(&doc.statements[1]);
	assert_eq!(bar.len(), 1);
	let r = get!(&bar[0], LookupReferenceStatement);
	assert!(Rc::ptr_eq(&r.lookup, foo));
	Ok(())
}

#[test]
fn test_lookup_reference_to_lookup_inside_feature() -> Result<()> {
	let doc = parse("feature Qux {lookup Foo {} Foo;} Qux;feature Bar {lookup Foo;} Bar;")?;
	let foo = lookup_block(first(&doc));
	let r = get!(&stmts(&doc.statements[1])[0], LookupReferenceStatement);
	assert!(Rc::ptr_eq(&r.lookup, foo));
	Ok(())
}

#[test]
fn test_lookup_reference_unknown() {
	assert_parse_error("Unknown lookup \"Huh\"", "feature liga {lookup Huh;} liga;");
}

/// `parse_lookupflag_(s)`.
fn parse_lookupflag(s: &str) -> Result<LookupFlagStatement> {
	let doc = parse(&format!("lookup L {{{s}}} L;"))?;
	let last = stmts(&doc.statements[0]).last().cloned();
	match last {
		Some(Statement::LookupFlagStatement(f)) => Ok(f),
		other => panic!("{other:?}"),
	}
}

#[test]
fn test_lookupflag_format_a() -> Result<()> {
	let flag = parse_lookupflag("lookupflag RightToLeft IgnoreMarks;")?;
	assert_eq!(flag.value, 9);
	assert!(flag.mark_attachment.is_none());
	assert!(flag.mark_filtering_set.is_none());
	assert_eq!(flag.as_fea(), "lookupflag RightToLeft IgnoreMarks;");
	Ok(())
}

#[test]
fn test_lookupflag_format_a_mark_attachment_type() -> Result<()> {
	let flag = parse_lookupflag(
		"@TOP_MARKS = [acute grave macron];\
		 lookupflag RightToLeft MarkAttachmentType @TOP_MARKS;",
	)?;
	assert_eq!(flag.value, 1);
	assert!(matches!(
		flag.mark_attachment,
		Some(GlyphContainer::GlyphClassName(_))
	));
	assert_eq!(
		flag.mark_attachment
			.as_ref()
			.map(|m| m.glyph_set().to_vec()),
		Some(strs(&["acute", "grave", "macron"]))
	);
	assert!(flag.mark_filtering_set.is_none());
	assert_eq!(
		flag.as_fea(),
		"lookupflag RightToLeft MarkAttachmentType @TOP_MARKS;"
	);
	Ok(())
}

#[test]
fn test_lookupflag_format_a_mark_attachment_type_glyph_class() -> Result<()> {
	let flag = parse_lookupflag("lookupflag RightToLeft MarkAttachmentType [acute grave macron];")?;
	assert_eq!(flag.value, 1);
	assert!(matches!(
		flag.mark_attachment,
		Some(GlyphContainer::GlyphClass(_))
	));
	assert_eq!(
		flag.mark_attachment
			.as_ref()
			.map(|m| m.glyph_set().to_vec()),
		Some(strs(&["acute", "grave", "macron"]))
	);
	assert!(flag.mark_filtering_set.is_none());
	assert_eq!(
		flag.as_fea(),
		"lookupflag RightToLeft MarkAttachmentType [acute grave macron];"
	);
	Ok(())
}

#[test]
fn test_lookupflag_format_a_use_mark_filtering_set() -> Result<()> {
	let flag = parse_lookupflag(
		"@BOTTOM_MARKS = [cedilla ogonek];\
		 lookupflag UseMarkFilteringSet @BOTTOM_MARKS IgnoreLigatures;",
	)?;
	assert_eq!(flag.value, 4);
	assert!(flag.mark_attachment.is_none());
	assert!(matches!(
		flag.mark_filtering_set,
		Some(GlyphContainer::GlyphClassName(_))
	));
	assert_eq!(
		flag.mark_filtering_set
			.as_ref()
			.map(|m| m.glyph_set().to_vec()),
		Some(strs(&["cedilla", "ogonek"]))
	);
	assert_eq!(
		flag.as_fea(),
		"lookupflag IgnoreLigatures UseMarkFilteringSet @BOTTOM_MARKS;"
	);
	Ok(())
}

#[test]
fn test_lookupflag_format_a_use_mark_filtering_set_glyph_class() -> Result<()> {
	let flag =
		parse_lookupflag("lookupflag UseMarkFilteringSet [cedilla ogonek] IgnoreLigatures;")?;
	assert_eq!(flag.value, 4);
	assert!(flag.mark_attachment.is_none());
	assert!(matches!(
		flag.mark_filtering_set,
		Some(GlyphContainer::GlyphClass(_))
	));
	assert_eq!(
		flag.mark_filtering_set
			.as_ref()
			.map(|m| m.glyph_set().to_vec()),
		Some(strs(&["cedilla", "ogonek"]))
	);
	assert_eq!(
		flag.as_fea(),
		"lookupflag IgnoreLigatures UseMarkFilteringSet [cedilla ogonek];"
	);
	Ok(())
}

#[test]
fn test_lookupflag_format_b() -> Result<()> {
	let flag = parse_lookupflag("lookupflag 7;")?;
	assert_eq!(flag.value, 7);
	assert!(flag.mark_attachment.is_none());
	assert!(flag.mark_filtering_set.is_none());
	assert_eq!(
		flag.as_fea(),
		"lookupflag RightToLeft IgnoreBaseGlyphs IgnoreLigatures;"
	);
	Ok(())
}

#[test]
fn test_lookupflag_format_b_zero() -> Result<()> {
	let flag = parse_lookupflag("lookupflag 0;")?;
	assert_eq!(flag.value, 0);
	assert!(flag.mark_attachment.is_none());
	assert!(flag.mark_filtering_set.is_none());
	assert_eq!(flag.as_fea(), "lookupflag 0;");
	Ok(())
}

#[test]
fn test_lookupflag_no_value() {
	assert_parse_error(
		"lookupflag must have a value",
		"feature test {lookupflag;} test;",
	);
}

#[test]
fn test_lookupflag_repeated() {
	assert_parse_error(
		"RightToLeft can be specified only once",
		"feature test {lookupflag RightToLeft RightToLeft;} test;",
	);
}

#[test]
fn test_lookupflag_unrecognized() {
	assert_parse_error(
		"\"IgnoreCookies\" is not a recognized lookupflag",
		"feature test {lookupflag IgnoreCookies;} test;",
	);
}

fn single_pos(doc: &FeatureFile) -> &SinglePosStatement {
	get!(first(doc), SinglePosStatement)
}

#[test]
fn test_gpos_type_1_glyph() -> Result<()> {
	let doc = parse("feature kern {pos one <1 2 3 4>;} kern;")?;
	let pos = single_pos(&doc);
	assert_eq!(pos.pos.len(), 1);
	let (glyphs, value) = &pos.pos[0];
	assert_eq!(glyphstr([glyphs]), "one");
	assert_eq!(vr_fea(value), "<1 2 3 4>");
	Ok(())
}

#[test]
fn test_gpos_type_1_glyphclass_horizontal() -> Result<()> {
	let doc = parse("feature kern {pos [one two] -300;} kern;")?;
	let pos = single_pos(&doc);
	assert_eq!(pos.pos.len(), 1);
	let (glyphs, value) = &pos.pos[0];
	assert_eq!(glyphstr([glyphs]), "[one two]");
	assert_eq!(vr_fea(value), "-300");
	Ok(())
}

#[test]
fn test_gpos_type_1_glyphclass_vertical() -> Result<()> {
	let doc = parse("feature vkrn {pos [one two] -300;} vkrn;")?;
	let pos = single_pos(&doc);
	assert_eq!(pos.pos.len(), 1);
	let (glyphs, value) = &pos.pos[0];
	assert_eq!(glyphstr([glyphs]), "[one two]");
	assert_eq!(vr_fea(value), "-300");
	Ok(())
}

#[test]
fn test_gpos_type_1_multiple() -> Result<()> {
	let doc = parse("feature f {pos one'1 two'2 [five six]'56;} f;")?;
	let pos = single_pos(&doc);
	assert_eq!(pos.pos.len(), 3);
	assert_eq!(glyphstr([&pos.pos[0].0]), "one");
	assert_eq!(vr_fea(&pos.pos[0].1), "1");
	assert_eq!(glyphstr([&pos.pos[1].0]), "two");
	assert_eq!(vr_fea(&pos.pos[1].1), "2");
	assert_eq!(glyphstr([&pos.pos[2].0]), "[five six]");
	assert_eq!(vr_fea(&pos.pos[2].1), "56");
	assert!(pos.prefix.is_empty());
	assert!(pos.suffix.is_empty());
	Ok(())
}

#[test]
fn test_gpos_type_1_enumerated() {
	assert_parse_error(
		"\"enumerate\" is only allowed with pair positionings",
		"feature test {enum pos T 100;} test;",
	);
	assert_parse_error(
		"\"enumerate\" is only allowed with pair positionings",
		"feature test {enumerate pos T 100;} test;",
	);
}

fn check_chained(text: &str, value: &str, suffix: &str) -> Result<()> {
	let doc = parse(text)?;
	let pos = single_pos(&doc);
	assert_eq!(pos.pos.len(), 1);
	let (glyphs, v) = &pos.pos[0];
	assert_eq!(glyphstr([glyphs]), "[T Y]");
	assert_eq!(vr_fea(v), value);
	assert_eq!(glyphstr(&pos.prefix), "[A B]");
	assert_eq!(glyphstr(&pos.suffix), suffix);
	Ok(())
}

#[test]
fn test_gpos_type_1_chained() -> Result<()> {
	check_chained(
		"feature kern {pos [A B] [T Y]' 20 comma;} kern;",
		"20",
		"comma",
	)
}

#[test]
fn test_gpos_type_1_chained_special_kern_format_valuerecord_format_a() -> Result<()> {
	check_chained(
		"feature kern {pos [A B] [T Y]' comma 20;} kern;",
		"20",
		"comma",
	)
}

#[test]
fn test_gpos_type_1_chained_special_kern_format_valuerecord_format_b() -> Result<()> {
	check_chained(
		"feature kern {pos [A B] [T Y]' comma <0 0 0 0>;} kern;",
		"<0 0 0 0>",
		"comma",
	)
}

#[test]
fn test_gpos_type_1_chained_special_kern_format_valuerecord_format_b_bug2293() -> Result<()> {
	// https://github.com/fonttools/fonttools/issues/2293
	check_chained(
		"feature kern {pos [A B] [T Y]' comma a <0 0 0 0>;} kern;",
		"<0 0 0 0>",
		"comma a",
	)
}

#[test]
fn test_gpos_type_1_chained_exception1() {
	assert_parse_error(
		"Positioning values are allowed",
		"feature kern {    pos [A B]' [T Y]' comma a <0 0 0 0>;} kern;",
	);
}

#[test]
fn test_gpos_type_1_chained_exception2() {
	assert_parse_error(
		"Positioning values are allowed",
		"feature kern {    pos [A B]' <0 0 0 0> [T Y]' comma a <0 0 0 0>;} kern;",
	);
}

#[test]
fn test_gpos_type_1_chained_exception3() {
	assert_parse_error(
		"Positioning cannot be applied",
		"feature kern {    pos [A B] <0 0 0 0> [T Y]' comma a <0 0 0 0>;} kern;",
	);
}

#[test]
fn test_gpos_type_1_chained_exception4() {
	assert_parse_error(
		"Positioning values are allowed",
		"feature kern {    pos a' b c 123 d;} kern;",
	);
}

#[test]
fn test_gpos_type_1_null() -> Result<()> {
	let doc = parse("feature test {pos a <NULL>;} test;")?;
	assert_eq!(single_pos(&doc).as_fea(), "pos a <NULL>;");
	Ok(())
}

#[test]
fn test_gpos_type_1_null_chained() -> Result<()> {
	let doc = parse("feature test {pos a' <NULL>;} test;")?;
	assert_eq!(single_pos(&doc).as_fea(), "pos a' <NULL>;");
	Ok(())
}

fn pair_pos(doc: &FeatureFile) -> &PairPosStatement {
	get!(first(doc), PairPosStatement)
}

#[test]
fn test_gpos_type_2_format_a() -> Result<()> {
	let doc = parse("feature kern {    pos [T V] -60 [a b c] <1 2 3 4>;} kern;")?;
	let pos = pair_pos(&doc);
	assert!(!pos.enumerated);
	assert_eq!(glyphstr([&pos.glyphs1]), "[T V]");
	assert_eq!(vr_fea(&pos.valuerecord1), "-60");
	assert_eq!(glyphstr([&pos.glyphs2]), "[a b c]");
	assert_eq!(vr_fea(&pos.valuerecord2), "<1 2 3 4>");
	Ok(())
}

#[test]
fn test_gpos_type_2_format_a_enumerated() -> Result<()> {
	let doc = parse("feature kern {    enum pos [T V] -60 [a b c] <1 2 3 4>;} kern;")?;
	let pos = pair_pos(&doc);
	assert!(pos.enumerated);
	assert_eq!(glyphstr([&pos.glyphs1]), "[T V]");
	assert_eq!(vr_fea(&pos.valuerecord1), "-60");
	assert_eq!(glyphstr([&pos.glyphs2]), "[a b c]");
	assert_eq!(vr_fea(&pos.valuerecord2), "<1 2 3 4>");
	Ok(())
}

#[test]
fn test_gpos_type_2_format_a_with_null_first() -> Result<()> {
	let doc = parse("feature kern {    pos [T V] <NULL> [a b c] <1 2 3 4>;} kern;")?;
	let pos = pair_pos(&doc);
	assert!(!pos.enumerated);
	assert_eq!(glyphstr([&pos.glyphs1]), "[T V]");
	assert!(
		!pos.valuerecord1
			.as_ref()
			.is_some_and(ValueRecord::is_truthy)
	);
	assert_eq!(vr_fea(&pos.valuerecord1), "<NULL>");
	assert_eq!(glyphstr([&pos.glyphs2]), "[a b c]");
	assert_eq!(vr_fea(&pos.valuerecord2), "<1 2 3 4>");
	assert_eq!(pos.as_fea()?, "pos [T V] <NULL> [a b c] <1 2 3 4>;");
	Ok(())
}

#[test]
fn test_gpos_type_2_format_a_with_null_second() -> Result<()> {
	let doc = parse("feature kern {    pos [T V] <1 2 3 4> [a b c] <NULL>;} kern;")?;
	let pos = pair_pos(&doc);
	assert!(!pos.enumerated);
	assert_eq!(glyphstr([&pos.glyphs1]), "[T V]");
	assert_eq!(vr_fea(&pos.valuerecord1), "<1 2 3 4>");
	assert_eq!(glyphstr([&pos.glyphs2]), "[a b c]");
	assert!(
		!pos.valuerecord2
			.as_ref()
			.is_some_and(ValueRecord::is_truthy)
	);
	assert_eq!(pos.as_fea()?, "pos [T V] [a b c] <1 2 3 4>;");
	Ok(())
}

#[test]
fn test_gpos_type_2_format_b() -> Result<()> {
	let doc = parse("feature kern {    pos [T V] [a b c] <1 2 3 4>;} kern;")?;
	let pos = pair_pos(&doc);
	assert!(!pos.enumerated);
	assert_eq!(glyphstr([&pos.glyphs1]), "[T V]");
	assert_eq!(vr_fea(&pos.valuerecord1), "<1 2 3 4>");
	assert_eq!(glyphstr([&pos.glyphs2]), "[a b c]");
	assert!(pos.valuerecord2.is_none());
	Ok(())
}

#[test]
fn test_gpos_type_2_format_b_enumerated() -> Result<()> {
	let doc = parse("feature kern {    enumerate position [T V] [a b c] <1 2 3 4>;} kern;")?;
	let pos = pair_pos(&doc);
	assert!(pos.enumerated);
	assert_eq!(glyphstr([&pos.glyphs1]), "[T V]");
	assert_eq!(vr_fea(&pos.valuerecord1), "<1 2 3 4>");
	assert_eq!(glyphstr([&pos.glyphs2]), "[a b c]");
	assert!(pos.valuerecord2.is_none());
	Ok(())
}

#[test]
fn test_gpos_type_3() -> Result<()> {
	let doc = parse("feature kern {    position cursive A <anchor 12 -2> <anchor 2 3>;} kern;")?;
	let pos = get!(first(&doc), CursivePosStatement);
	assert_eq!(*pos.glyphclass.glyph_set(), strs(&["A"]));
	assert_eq!(anchor_xy(&pos.entry_anchor), (12, -2));
	assert_eq!(anchor_xy(&pos.exit_anchor), (2, 3));
	Ok(())
}

#[test]
fn test_gpos_type_3_enumerated() {
	assert_parse_error(
		"\"enumerate\" is not allowed with cursive attachment positioning",
		"feature kern {    enumerate position cursive A <anchor 12 -2> <anchor 2 3>;} kern;",
	);
}

fn mark_name(m: &MarkClassRef) -> String {
	m.borrow().name.clone()
}

#[test]
fn test_gpos_type_4() -> Result<()> {
	let doc = parse(
		"markClass [acute grave] <anchor 150 -10> @TOP_MARKS;\
		 markClass [dieresis umlaut] <anchor 300 -10> @TOP_MARKS;\
		 markClass [cedilla] <anchor 300 600> @BOTTOM_MARKS;\
		 feature test {\
		     position base [a e o u] \
		         <anchor 250 450> mark @TOP_MARKS \
		         <anchor 210 -10> mark @BOTTOM_MARKS;\
		 } test;",
	)?;
	let pos = get!(&stmts(&doc.statements[3])[0], MarkBasePosStatement);
	assert_eq!(*pos.base.glyph_set(), strs(&["a", "e", "o", "u"]));
	assert_eq!(pos.marks.len(), 2);
	assert_eq!(anchor_xy(&pos.marks[0].0), (250, 450));
	assert_eq!(mark_name(&pos.marks[0].1), "TOP_MARKS");
	assert_eq!(anchor_xy(&pos.marks[1].0), (210, -10));
	assert_eq!(mark_name(&pos.marks[1].1), "BOTTOM_MARKS");
	Ok(())
}

#[test]
fn test_gpos_type_4_enumerated() {
	assert_parse_error(
		"\"enumerate\" is not allowed with mark-to-base attachment positioning",
		"feature kern {\
		     markClass cedilla <anchor 300 600> @BOTTOM_MARKS;\
		     enumerate position base A <anchor 12 -2> mark @BOTTOM_MARKS;\
		 } kern;",
	);
}

#[test]
fn test_gpos_type_4_not_mark_class() {
	assert_parse_error(
		"@MARKS is not a markClass",
		"@MARKS = [acute grave];\
		 feature test {\
		     position base [a e o u] <anchor 250 450> mark @MARKS;\
		 } test;",
	);
}

#[test]
fn test_gpos_type_5() -> Result<()> {
	let doc = parse(
		"markClass [grave acute] <anchor 150 500> @TOP_MARKS;\
		 markClass [cedilla] <anchor 300 -100> @BOTTOM_MARKS;\
		 feature test {\
		     position \
		         ligature [a_f_f_i o_f_f_i] \
		             <anchor 50 600> mark @TOP_MARKS \
		             <anchor 50 -10> mark @BOTTOM_MARKS \
		         ligComponent \
		             <anchor 30 800> mark @TOP_MARKS \
		         ligComponent \
		             <anchor NULL> \
		         ligComponent \
		             <anchor 30 -10> mark @BOTTOM_MARKS;\
		 } test;",
	)?;
	let pos = get!(&stmts(&doc.statements[2])[0], MarkLigPosStatement);
	assert_eq!(*pos.ligatures.glyph_set(), strs(&["a_f_f_i", "o_f_f_i"]));
	let lens: Vec<usize> = pos.marks.iter().map(Vec::len).collect();
	assert_eq!(lens, vec![2, 1, 0, 1]);
	assert_eq!(anchor_xy(&pos.marks[0][0].0), (50, 600));
	assert_eq!(mark_name(&pos.marks[0][0].1), "TOP_MARKS");
	assert_eq!(anchor_xy(&pos.marks[0][1].0), (50, -10));
	assert_eq!(mark_name(&pos.marks[0][1].1), "BOTTOM_MARKS");
	assert_eq!(anchor_xy(&pos.marks[1][0].0), (30, 800));
	assert_eq!(mark_name(&pos.marks[1][0].1), "TOP_MARKS");
	assert_eq!(anchor_xy(&pos.marks[3][0].0), (30, -10));
	assert_eq!(mark_name(&pos.marks[3][0].1), "BOTTOM_MARKS");
	Ok(())
}

#[test]
fn test_gpos_type_5_enumerated() {
	assert_parse_error(
		"\"enumerate\" is not allowed with mark-to-ligature attachment positioning",
		"feature test {\
		     markClass cedilla <anchor 300 600> @MARKS;\
		     enumerate position \
		         ligature f_i <anchor 100 0> mark @MARKS\
		         ligComponent <anchor NULL>;\
		 } test;",
	);
}

#[test]
fn test_gpos_type_5_not_mark_class() {
	assert_parse_error(
		"@MARKS is not a markClass",
		"@MARKS = [acute grave];\
		 feature test {\
		     position ligature f_i <anchor 250 450> mark @MARKS;\
		 } test;",
	);
}

#[test]
fn test_gpos_type_6() -> Result<()> {
	let doc = parse(
		"markClass damma <anchor 189 -103> @MARK_CLASS_1;\
		 feature test {\
		     position mark hamza <anchor 221 301> mark @MARK_CLASS_1;\
		 } test;",
	)?;
	let pos = get!(&stmts(&doc.statements[1])[0], MarkMarkPosStatement);
	assert_eq!(*pos.base_marks.glyph_set(), strs(&["hamza"]));
	assert_eq!(pos.marks.len(), 1);
	assert_eq!(anchor_xy(&pos.marks[0].0), (221, 301));
	assert_eq!(mark_name(&pos.marks[0].1), "MARK_CLASS_1");
	Ok(())
}

#[test]
fn test_gpos_type_6_enumerated() {
	assert_parse_error(
		"\"enumerate\" is not allowed with mark-to-mark attachment positioning",
		"markClass damma <anchor 189 -103> @MARK_CLASS_1;\
		 feature test {\
		     enum pos mark hamza <anchor 221 301> mark @MARK_CLASS_1;\
		 } test;",
	);
}

#[test]
fn test_gpos_type_6_not_mark_class() {
	assert_parse_error(
		"@MARKS is not a markClass",
		"@MARKS = [acute grave];\
		 feature test {\
		     position mark cedilla <anchor 250 450> mark @MARKS;\
		 } test;",
	);
}

/// Compares chaining lookups by identity, like `assertEqual` on Python lists of
/// the same `LookupBlock` objects.
fn assert_lookups(actual: &ChainLookups, expected: &[Option<&Rc<LookupBlock>>]) {
	assert_eq!(actual.len(), expected.len());
	for (a, e) in actual.iter().zip(expected) {
		match (a, e) {
			(None, None) => {}
			(Some(a), Some(e)) => {
				assert_eq!(a.len(), 1);
				assert!(Rc::ptr_eq(&a[0], e));
			}
			_ => panic!("{a:?} != {e:?}"),
		}
	}
}

#[test]
fn test_gpos_type_8() -> Result<()> {
	let doc = parse(
		"lookup L1 {pos one 100;} L1; lookup L2 {pos two 200;} L2;\
		 feature test {\
		     pos [A a] [B b] I' lookup L1 [N n]' lookup L2 P' [Y y] [Z z];\
		 } test;",
	)?;
	let lookup1 = lookup_block(&doc.statements[0]);
	let lookup2 = lookup_block(&doc.statements[1]);
	let pos = get!(&stmts(&doc.statements[2])[0], ChainContextPosStatement);
	assert_eq!(glyphstr(&pos.prefix), "[A a] [B b]");
	assert_eq!(glyphstr(&pos.glyphs), "I [N n] P");
	assert_eq!(glyphstr(&pos.suffix), "[Y y] [Z z]");
	assert_lookups(&pos.lookups, &[Some(lookup1), Some(lookup2), None]);
	Ok(())
}

#[test]
fn test_gpos_type_8_lookup_with_values() {
	assert_parse_error(
		"If \"lookup\" is present, no values must be specified",
		"lookup L1 {pos one 100;} L1;\
		 feature test {\
		     pos A' lookup L1 B' 20;\
		 } test;",
	);
}

#[test]
fn test_mark_class() -> Result<()> {
	let doc = parse("markClass [acute grave] <anchor 350 3> @MARKS;")?;
	let mc = get!(&doc.statements[0], MarkClassDefinition);
	assert_eq!(mc.mark_class_name()?, "MARKS");
	assert_eq!(*mc.glyph_set(), strs(&["acute", "grave"]));
	assert_eq!((int(&mc.anchor.x), int(&mc.anchor.y)), (350, 3));
	Ok(())
}

#[test]
fn test_nameid_windows_unpaired_surrogate() {
	assert_parse_error(
		"Name string is not valid utf_16_be",
		r#"table name { nameid 9 "Joachim \d800"; } name;"#,
	);
}

#[test]
fn test_nameid_mac_unencodable_character() {
	assert_parse_error(
		"Name string is not valid mac_roman",
		"table name { nameid 9 1 \"A\u{1f600}\"; } name;",
	);
}

#[test]
fn test_stat_name_unpaired_surrogate() {
	assert_parse_error(
		"Name string is not valid utf_16_be",
		r#"table STAT { ElidedFallbackName { name "\dc00"; }; } STAT;"#,
	);
}

fn name_record(doc: &FeatureFile) -> &NameRecord {
	get!(first(doc), NameRecord)
}

#[test]
fn test_nameid_windows_utf16() -> Result<()> {
	let doc = parse(r#"table name { nameid 9 "M\00fcller-Lanc\00e9"; } name;"#)?;
	let name = name_record(&doc);
	assert_eq!(name.name_id, 9);
	assert_eq!(name.platform_id, 3);
	assert_eq!(name.plat_enc_id, 1);
	assert_eq!(name.lang_id, 0x0409);
	assert_eq!(name.string, "Müller-Lancé");
	assert_eq!(name.as_fea()?, r#"nameid 9 "M\00fcller-Lanc\00e9";"#);
	Ok(())
}

#[test]
fn test_nameid_windows_utf16_backslash() -> Result<()> {
	let doc = parse(r#"table name { nameid 9 "Back\005cslash"; } name;"#)?;
	let name = name_record(&doc);
	assert_eq!(name.string, r"Back\slash");
	assert_eq!(name.as_fea()?, r#"nameid 9 "Back\005cslash";"#);
	Ok(())
}

#[test]
fn test_nameid_windows_utf16_quotation_mark() -> Result<()> {
	let doc = parse(r#"table name { nameid 9 "Quotation \0022Mark\0022"; } name;"#)?;
	let name = name_record(&doc);
	assert_eq!(name.string, "Quotation \"Mark\"");
	assert_eq!(name.as_fea()?, r#"nameid 9 "Quotation \0022Mark\0022";"#);
	Ok(())
}

#[test]
fn test_nameid_windows_utf16_surroates() -> Result<()> {
	let doc = parse(r#"table name { nameid 9 "Carrot \D83E\DD55"; } name;"#)?;
	let name = name_record(&doc);
	assert_eq!(name.string, "Carrot 🥕");
	assert_eq!(name.as_fea()?, r#"nameid 9 "Carrot \d83e\dd55";"#);
	Ok(())
}

#[test]
fn test_nameid_mac_roman() -> Result<()> {
	let doc = parse(r#"table name { nameid 9 1 "Joachim M\9fller-Lanc\8e"; } name;"#)?;
	let name = name_record(&doc);
	assert_eq!(name.name_id, 9);
	assert_eq!(name.platform_id, 1);
	assert_eq!(name.plat_enc_id, 0);
	assert_eq!(name.lang_id, 0);
	assert_eq!(name.string, "Joachim Müller-Lancé");
	assert_eq!(name.as_fea()?, r#"nameid 9 1 "Joachim M\9fller-Lanc\8e";"#);
	Ok(())
}

#[test]
fn test_nameid_mac_croatian() -> Result<()> {
	let doc = parse(r#"table name { nameid 9 1 0 18 "Jovica Veljovi\e6"; } name;"#)?;
	let name = name_record(&doc);
	assert_eq!(name.name_id, 9);
	assert_eq!(name.platform_id, 1);
	assert_eq!(name.plat_enc_id, 0);
	assert_eq!(name.lang_id, 18);
	assert_eq!(name.string, "Jovica Veljović");
	assert_eq!(name.as_fea()?, r#"nameid 9 1 0 18 "Jovica Veljovi\e6";"#);
	Ok(())
}

#[test]
fn test_nameid_mac_japanese() -> Result<()> {
	// Literal text and \XX escapes are combined into a single byte sequence
	// before decoding; here \95F is the Shift-JIS byte pair 0x95 0x46.
	let doc = parse(r#"table name { nameid 9 1 1 11 "Kozuka \8f\ac\92\cb\8f\b9\95F"; } name;"#)?;
	let name = name_record(&doc);
	assert_eq!(name.name_id, 9);
	assert_eq!(name.platform_id, 1);
	assert_eq!(name.plat_enc_id, 1);
	assert_eq!(name.lang_id, 11);
	assert_eq!(name.string, "Kozuka 小塚昌彦");
	assert_eq!(
		name.as_fea()?,
		r#"nameid 9 1 1 11 "Kozuka \8f\ac\92\cb\8f\b9\95F";"#
	);
	Ok(())
}

#[test]
fn test_nameid_unsupported_platform() {
	assert_parse_error(
		"Expected platform id 1 or 3",
		"table name { nameid 9 666 \"Foo\"; } name;",
	);
}

#[test]
fn test_nameid_hexadecimal() -> Result<()> {
	let doc = parse(r#"table name { nameid 0x9 0x3 0x1 0x0409 "Test"; } name;"#)?;
	let name = name_record(&doc);
	assert_eq!(name.name_id, 9);
	assert_eq!(name.platform_id, 3);
	assert_eq!(name.plat_enc_id, 1);
	assert_eq!(name.lang_id, 0x0409);
	Ok(())
}

#[test]
fn test_nameid_octal() -> Result<()> {
	let doc = parse(r#"table name { nameid 011 03 012 02011 "Test"; } name;"#)?;
	let name = name_record(&doc);
	assert_eq!(name.name_id, 9);
	assert_eq!(name.platform_id, 3);
	assert_eq!(name.plat_enc_id, 10);
	assert_eq!(name.lang_id, 0o2011);
	Ok(())
}

fn cv_character(doc: &FeatureFile) -> i64 {
	get!(&stmts(first(doc))[0], CharacterStatement).character
}

#[test]
fn test_cv_hexadecimal() -> Result<()> {
	let doc = parse(r"feature cv01 { cvParameters { Character 0x5DDE; }; } cv01;")?;
	assert_eq!(cv_character(&doc), 0x5DDE);
	Ok(())
}

#[test]
fn test_cv_octal() -> Result<()> {
	let doc = parse(r"feature cv01 { cvParameters { Character 056736; }; } cv01;")?;
	assert_eq!(cv_character(&doc), 0o56736);
	Ok(())
}

fn rsub(doc: &FeatureFile) -> &ReverseChainSingleSubstStatement {
	get!(first(doc), ReverseChainSingleSubstStatement)
}

#[test]
fn test_rsub_format_a() -> Result<()> {
	let doc = parse("feature test {rsub a [b B] c' d [e E] by C;} test;")?;
	let rsub = rsub(&doc);
	assert_eq!(glyphstr(&rsub.old_prefix), "a [B b]");
	assert_eq!(*rsub.glyphs[0].glyph_set(), strs(&["c"]));
	assert_eq!(*rsub.replacements[0].glyph_set(), strs(&["C"]));
	assert_eq!(glyphstr(&rsub.old_suffix), "d [E e]");
	Ok(())
}

#[test]
fn test_rsub_format_a_cid() -> Result<()> {
	let doc = parse(r"feature test {rsub \1 [\2 \3] \4' \5 by \6;} test;")?;
	let rsub = rsub(&doc);
	assert_eq!(glyphstr(&rsub.old_prefix), "cid00001 [cid00002 cid00003]");
	assert_eq!(*rsub.glyphs[0].glyph_set(), strs(&["cid00004"]));
	assert_eq!(*rsub.replacements[0].glyph_set(), strs(&["cid00006"]));
	assert_eq!(glyphstr(&rsub.old_suffix), "cid00005");
	Ok(())
}

#[test]
fn test_rsub_format_b() -> Result<()> {
	let doc = parse(
		"feature smcp {    reversesub A B [one.fitted one.oldstyle]' C [d D] by one;} smcp;",
	)?;
	let rsub = rsub(&doc);
	assert_eq!(glyphstr(&rsub.old_prefix), "A B");
	assert_eq!(glyphstr(&rsub.old_suffix), "C [D d]");
	assert_eq!(
		mapping(&rsub.glyphs, &rsub.replacements),
		map(&[("one.fitted", "one"), ("one.oldstyle", "one")])
	);
	Ok(())
}

#[test]
fn test_rsub_format_c() -> Result<()> {
	let doc =
		parse("feature test {    reversesub BACK TRACK [a-d]' LOOK AHEAD by [A.sc-D.sc];} test;")?;
	let rsub = rsub(&doc);
	assert_eq!(glyphstr(&rsub.old_prefix), "BACK TRACK");
	assert_eq!(glyphstr(&rsub.old_suffix), "LOOK AHEAD");
	assert_eq!(
		mapping(&rsub.glyphs, &rsub.replacements),
		map(&[("a", "A.sc"), ("b", "B.sc"), ("c", "C.sc"), ("d", "D.sc")])
	);
	Ok(())
}

#[test]
fn test_rsub_from() {
	assert_parse_error(
		"Reverse chaining substitutions do not support \"from\"",
		"feature test {rsub a from [a.1 a.2 a.3];} test;",
	);
}

#[test]
fn test_rsub_nonsingle() {
	assert_parse_error(
		"In reverse chaining single substitutions, only a single glyph or glyph class can be replaced",
		"feature test {rsub c d by c_d;} test;",
	);
}

#[test]
fn test_rsub_multiple_replacement_glyphs() {
	assert_parse_error(
		"In reverse chaining single substitutions, the replacement (after \"by\") must be a single glyph or glyph class",
		"feature test {rsub f_i by f i;} test;",
	);
}

#[test]
fn test_rsub_deletion() {
	assert_parse_error(
		"Reverse chaining substitutions do not support glyph deletion",
		"lookup test { rsub a b c' e by NULL; } test;",
	);
}

#[test]
fn test_script() -> Result<()> {
	let doc = parse("feature test {script cyrl;} test;")?;
	assert_eq!(get!(first(&doc), ScriptStatement).script, "cyrl");
	Ok(())
}

#[test]
fn test_script_dflt() {
	assert_parse_error(
		"\"dflt\" is not a valid script tag; use \"DFLT\" instead",
		"feature test {script dflt;} test;",
	);
}

#[test]
fn test_stat_design_axis() -> Result<()> {
	let doc = parse("table STAT { DesignAxis opsz 0 {name \"Optical Size\";}; } STAT;")?;
	let da = get!(first(&doc), STATDesignAxisStatement);
	assert_eq!(da.tag, "opsz");
	assert_eq!(da.axis_order, 0);
	assert_eq!(da.names[0].string, "Optical Size");
	Ok(())
}

fn axis_value(doc: &FeatureFile) -> &STATAxisValueStatement {
	get!(&stmts(&doc.statements[0])[1], STATAxisValueStatement)
}

#[test]
fn test_stat_axis_value_format1() -> Result<()> {
	let doc = parse(
		"table STAT { DesignAxis opsz 0 {name \"Optical Size\";}; \
		 AxisValue {location opsz 8; name \"Caption\";}; } STAT;",
	)?;
	let avr = axis_value(&doc);
	assert_eq!(avr.locations[0].tag, "opsz");
	assert_eq!(avr.locations[0].values[0], IntOrFloat::Int(8));
	assert_eq!(avr.names[0].string, "Caption");
	Ok(())
}

#[test]
fn test_stat_axis_value_format2() -> Result<()> {
	let doc = parse(
		"table STAT { DesignAxis opsz 0 {name \"Optical Size\";}; \
		 AxisValue {location opsz 8 6 10; name \"Caption\";}; } STAT;",
	)?;
	let avr = axis_value(&doc);
	assert_eq!(avr.locations[0].tag, "opsz");
	assert_eq!(
		avr.locations[0].values,
		vec![IntOrFloat::Int(8), IntOrFloat::Int(6), IntOrFloat::Int(10)]
	);
	assert_eq!(avr.names[0].string, "Caption");
	Ok(())
}

#[test]
fn test_stat_axis_value_format2_bad_range() {
	assert_parse_error(
		"Default value 5 is outside of specified range 6-10.",
		"table STAT { DesignAxis opsz 0 {name \"Optical Size\";}; \
		 AxisValue {location opsz 5 6 10; name \"Caption\";}; } STAT;",
	);
}

#[test]
fn test_stat_axis_value_too_many_values() {
	assert_parse_error(
		"Expected 1, 2 or 3 values for axis opsz, but 4 were found.",
		"table STAT { DesignAxis opsz 0 {name \"Optical Size\";}; \
		 AxisValue {location opsz 8 6 10 12; name \"Caption\";}; } STAT;",
	);
}

#[test]
fn test_stat_axis_value_no_values() {
	assert_parse_error(
		"Expected 1, 2 or 3 values for axis opsz, but 0 were found.",
		"table STAT { DesignAxis opsz 0 {name \"Optical Size\";}; \
		 AxisValue {location opsz; name \"Caption\";}; } STAT;",
	);
}

#[test]
fn test_stat_axis_value_format4() {
	assert_parse_error(
		"Only one value is allowed in a Format 4 Axis Value Record, but 3 were found.",
		"table STAT { \
		 DesignAxis opsz 0 {name \"Optical Size\";}; \
		 DesignAxis wdth 0 {name \"Width\";}; \
		 AxisValue {\
		 location opsz 8 6 10; \
		 location wdth 400; \
		 name \"Caption\";}; } \
		 STAT;",
	);
}

#[test]
fn test_stat_elidedfallbackname() -> Result<()> {
	let doc = parse(
		"table STAT { ElidedFallbackName {name \"Roman\"; \
		 name 3 1 0x0411 \"ローマン\"; }; } STAT;",
	)?;
	let name_record = get!(first(&doc), ElidedFallbackName);
	assert_eq!(name_record.names[0].string, "Roman");
	assert_eq!(name_record.names[1].string, "ローマン");
	Ok(())
}

#[test]
fn test_stat_elidedfallbacknameid() -> Result<()> {
	let doc = parse(
		"table name { nameid 278 \"Roman\"; } name; \
		 table STAT { ElidedFallbackNameID 278; } STAT;",
	)?;
	let name_record = name_record(&doc);
	assert_eq!(name_record.string, "Roman");
	Ok(())
}

fn single_sub(doc: &FeatureFile) -> &SingleSubstStatement {
	get!(first(doc), SingleSubstStatement)
}

#[test]
fn test_sub_single_format_a() -> Result<()> {
	let doc = parse("feature smcp {substitute a by a.sc;} smcp;")?;
	let sub = single_sub(&doc);
	assert_eq!(glyphstr(&sub.prefix), "");
	assert_eq!(
		mapping(&sub.glyphs, &sub.replacements),
		map(&[("a", "a.sc")])
	);
	assert_eq!(glyphstr(&sub.suffix), "");
	Ok(())
}

#[test]
fn test_sub_single_format_a_chained() -> Result<()> {
	let doc = parse("feature test {sub [A a] d' [C] by d.alt;} test;")?;
	let sub = single_sub(&doc);
	assert_eq!(
		mapping(&sub.glyphs, &sub.replacements),
		map(&[("d", "d.alt")])
	);
	assert_eq!(glyphstr(&sub.prefix), "[A a]");
	assert_eq!(glyphstr(&sub.suffix), "C");
	Ok(())
}

#[test]
fn test_sub_single_format_a_cid() -> Result<()> {
	let doc = parse(r"feature smcp {substitute \12345 by \78987;} smcp;")?;
	let sub = single_sub(&doc);
	assert_eq!(glyphstr(&sub.prefix), "");
	assert_eq!(
		mapping(&sub.glyphs, &sub.replacements),
		map(&[("cid12345", "cid78987")])
	);
	assert_eq!(glyphstr(&sub.suffix), "");
	Ok(())
}

#[test]
fn test_sub_single_format_b() -> Result<()> {
	let doc = parse("feature smcp {    substitute [one.fitted one.oldstyle] by one;} smcp;")?;
	let sub = single_sub(&doc);
	assert_eq!(
		mapping(&sub.glyphs, &sub.replacements),
		map(&[("one.fitted", "one"), ("one.oldstyle", "one")])
	);
	assert_eq!(glyphstr(&sub.prefix), "");
	assert_eq!(glyphstr(&sub.suffix), "");
	Ok(())
}

#[test]
fn test_sub_single_format_b_chained() -> Result<()> {
	let doc = parse(
		"feature smcp {    substitute PRE FIX [one.fitted one.oldstyle]' SUF FIX by one;} smcp;",
	)?;
	let sub = single_sub(&doc);
	assert_eq!(
		mapping(&sub.glyphs, &sub.replacements),
		map(&[("one.fitted", "one"), ("one.oldstyle", "one")])
	);
	assert_eq!(glyphstr(&sub.prefix), "PRE FIX");
	assert_eq!(glyphstr(&sub.suffix), "SUF FIX");
	Ok(())
}

#[test]
fn test_sub_single_format_c() -> Result<()> {
	let doc = parse("feature smcp {    substitute [a-d] by [A.sc-D.sc];} smcp;")?;
	let sub = single_sub(&doc);
	assert_eq!(
		mapping(&sub.glyphs, &sub.replacements),
		map(&[("a", "A.sc"), ("b", "B.sc"), ("c", "C.sc"), ("d", "D.sc")])
	);
	assert_eq!(glyphstr(&sub.prefix), "");
	assert_eq!(glyphstr(&sub.suffix), "");
	Ok(())
}

#[test]
fn test_sub_single_format_c_chained() -> Result<()> {
	let doc = parse("feature smcp {    substitute [a-d]' X Y [Z z] by [A.sc-D.sc];} smcp;")?;
	let sub = single_sub(&doc);
	assert_eq!(
		mapping(&sub.glyphs, &sub.replacements),
		map(&[("a", "A.sc"), ("b", "B.sc"), ("c", "C.sc"), ("d", "D.sc")])
	);
	assert_eq!(glyphstr(&sub.prefix), "");
	assert_eq!(glyphstr(&sub.suffix), "X Y [Z z]");
	Ok(())
}

#[test]
fn test_sub_single_format_c_different_num_elements() {
	assert_parse_error(
		"Expected a glyph class with 4 elements after \"by\", but found a glyph class with 26 elements",
		"feature smcp {sub [a-d] by [A.sc-Z.sc];} smcp;",
	);
}

#[test]
fn test_sub_with_values() {
	assert_parse_error(
		"Substitution statements cannot contain values",
		"feature smcp {sub A' 20 by A.sc;} smcp;",
	);
}

fn multiple_sub(doc: &FeatureFile) -> &MultipleSubstStatement {
	get!(first(doc), MultipleSubstStatement)
}

#[test]
fn test_substitute_multiple() -> Result<()> {
	let doc = parse("lookup Look {substitute f_f_i by f f i;} Look;")?;
	let sub = multiple_sub(&doc);
	assert_eq!(glyphstr([&sub.glyph]), "f_f_i");
	assert_eq!(glyphstr(&sub.replacement), "f f i");
	Ok(())
}

#[test]
fn test_substitute_multiple_chained() -> Result<()> {
	let doc = parse("lookup L {sub [A-C] f_f_i' [X-Z] by f f i;} L;")?;
	let sub = multiple_sub(&doc);
	assert_eq!(glyphstr([&sub.glyph]), "f_f_i");
	assert_eq!(glyphstr(&sub.replacement), "f f i");
	Ok(())
}

#[test]
fn test_substitute_multiple_force_chained() -> Result<()> {
	let doc = parse("lookup L {sub f_f_i' by f f i;} L;")?;
	let sub = multiple_sub(&doc);
	assert_eq!(glyphstr([&sub.glyph]), "f_f_i");
	assert_eq!(glyphstr(&sub.replacement), "f f i");
	assert_eq!(sub.as_fea(), "sub f_f_i' by f f i;");
	Ok(())
}

#[test]
fn test_substitute_multiple_classes() -> Result<()> {
	let doc = parse("lookup Look {substitute [f_i f_l] by [f f] [i l];} Look;")?;
	let sub = multiple_sub(&doc);
	assert_eq!(glyphstr([&sub.glyph]), "[f_i f_l]");
	assert_eq!(glyphstr(&sub.replacement), "[f f] [i l]");
	Ok(())
}

#[test]
fn test_substitute_multiple_classes_mixed() -> Result<()> {
	let doc = parse("lookup Look {substitute [f_i f_l] by f [i l];} Look;")?;
	let sub = multiple_sub(&doc);
	assert_eq!(glyphstr([&sub.glyph]), "[f_i f_l]");
	assert_eq!(glyphstr(&sub.replacement), "f [i l]");
	Ok(())
}

#[test]
fn test_substitute_multiple_classes_mixed_singleton() -> Result<()> {
	let doc = parse("lookup Look {substitute [f_i f_l] by [f] [i l];} Look;")?;
	let sub = multiple_sub(&doc);
	assert_eq!(glyphstr([&sub.glyph]), "[f_i f_l]");
	assert_eq!(glyphstr(&sub.replacement), "f [i l]");
	Ok(())
}

#[test]
fn test_substitute_multiple_classes_mismatch() {
	assert_parse_error(
		"Expected a glyph class with 1 or 3 elements after \"by\", but found a glyph class with 2 elements",
		"lookup Look {substitute [f_i f_l f_f_i] by [f f_f] [i l i];} Look;",
	);
}

#[test]
fn test_substitute_multiple_by_mutliple() {
	assert_parse_error(
		"Direct substitution of multiple glyphs by multiple glyphs is not supported",
		"lookup MxM {sub a b c by d e f;} MxM;",
	);
}

#[test]
fn test_split_marked_glyphs_runs() {
	assert_parse_error(
		"Unsupported contextual target sequence",
		"feature test{    ignore pos a' x x A';} test;",
	);
	assert_parse_error(
		"Unsupported contextual target sequence",
		"lookup shift {\
		     pos a <0 -10 0 0>;\
		     pos A <0 10 0 0>;\
		 } shift;\
		 feature test {\
		     sub a' lookup shift x x A' lookup shift;\
		 } test;",
	);
	assert_parse_error(
		"Unsupported contextual target sequence",
		"feature test {    ignore sub a' x x A';} test;",
	);
	assert_parse_error(
		"Unsupported contextual target sequence",
		"lookup upper {\
		     sub a by A;\
		 } upper;\
		 lookup lower {\
		     sub A by a;\
		 } lower;\
		 feature test {\
		     sub a' lookup upper x x A' lookup lower;\
		 } test;",
	);
}

fn alternate_sub(doc: &FeatureFile) -> &AlternateSubstStatement {
	get!(first(doc), AlternateSubstStatement)
}

#[test]
fn test_substitute_from() -> Result<()> {
	let doc = parse("feature test {  substitute a from [a.1 a.2 a.3];} test;")?;
	let sub = alternate_sub(&doc);
	assert_eq!(glyphstr(&sub.prefix), "");
	assert_eq!(glyphstr([&sub.glyph]), "a");
	assert_eq!(glyphstr(&sub.suffix), "");
	assert_eq!(glyphstr([&sub.replacement]), "[a.1 a.2 a.3]");
	Ok(())
}

#[test]
fn test_substitute_from_chained() -> Result<()> {
	let doc = parse("feature test {  substitute A B a' [Y y] Z from [a.1 a.2 a.3];} test;")?;
	let sub = alternate_sub(&doc);
	assert_eq!(glyphstr(&sub.prefix), "A B");
	assert_eq!(glyphstr([&sub.glyph]), "a");
	assert_eq!(glyphstr(&sub.suffix), "[Y y] Z");
	assert_eq!(glyphstr([&sub.replacement]), "[a.1 a.2 a.3]");
	Ok(())
}

#[test]
fn test_substitute_from_cid() -> Result<()> {
	let doc = parse(r"feature test {  substitute \7 from [\111 \222];} test;")?;
	let sub = alternate_sub(&doc);
	assert_eq!(glyphstr(&sub.prefix), "");
	assert_eq!(glyphstr([&sub.glyph]), "cid00007");
	assert_eq!(glyphstr(&sub.suffix), "");
	assert_eq!(glyphstr([&sub.replacement]), "[cid00111 cid00222]");
	Ok(())
}

#[test]
fn test_substitute_from_glyphclass() -> Result<()> {
	let doc = parse(
		"feature test {\
		   @Ampersands = [ampersand.1 ampersand.2];\
		   substitute ampersand from @Ampersands;\
		 } test;",
	)?;
	let block = stmts(&doc.statements[0]);
	assert_eq!(block.len(), 2);
	let sub = get!(&block[1], AlternateSubstStatement);
	assert_eq!(glyphstr(&sub.prefix), "");
	assert_eq!(glyphstr([&sub.glyph]), "ampersand");
	assert_eq!(glyphstr(&sub.suffix), "");
	assert_eq!(glyphstr([&sub.replacement]), "[ampersand.1 ampersand.2]");
	Ok(())
}

#[test]
fn test_substitute_ligature() -> Result<()> {
	let doc = parse("feature liga {substitute f f i by f_f_i;} liga;")?;
	let sub = get!(first(&doc), LigatureSubstStatement);
	assert_eq!(glyphstr(&sub.glyphs), "f f i");
	assert_eq!(sub.replacement, "f_f_i");
	assert_eq!(glyphstr(&sub.prefix), "");
	assert_eq!(glyphstr(&sub.suffix), "");
	Ok(())
}

#[test]
fn test_substitute_ligature_chained() -> Result<()> {
	let doc = parse("feature F {substitute A B f' i' Z by f_i;} F;")?;
	let sub = get!(first(&doc), LigatureSubstStatement);
	assert_eq!(glyphstr(&sub.glyphs), "f i");
	assert_eq!(sub.replacement, "f_i");
	assert_eq!(glyphstr(&sub.prefix), "A B");
	assert_eq!(glyphstr(&sub.suffix), "Z");
	Ok(())
}

#[test]
fn test_substitute_lookups() -> Result<()> {
	let doc = Parser::new(getpath("spec5fi1.fea"), glyphnames())?.parse()?;
	assert_eq!(doc.statements.len(), 7);
	let ligs = lookup_block(&doc.statements[4]);
	let sub = lookup_block(&doc.statements[5]);
	let feature = stmts(&doc.statements[6]);
	for s in &feature[0..2] {
		let chain = get!(s, ChainContextSubstStatement);
		assert_lookups(&chain.lookups, &[Some(ligs), None, Some(sub)]);
	}
	Ok(())
}

#[test]
fn test_substitute_missing_by() {
	assert_parse_error(
		"Expected \"by\", \"from\" or explicit lookup references",
		"feature liga {substitute f f i;} liga;",
	);
}

#[test]
fn test_substitute_invalid_statement() {
	assert_error(
		"Invalid substitution statement",
		Parser::new(getpath("GSUB_error.fea"), glyphnames()).and_then(Parser::parse),
	);
}

#[test]
fn test_subtable() -> Result<()> {
	let doc = parse("feature test {subtable;} test;")?;
	get!(first(&doc), SubtableStatement);
	Ok(())
}

#[test]
fn test_table_base_script_list_without_tag_list() {
	assert_parse_error(
		"BaseScriptList must be preceded by BaseTagList",
		"table BASE { HorizAxis.BaseScriptList latn romn 0; } BASE;",
	);
}

#[test]
fn test_table_base_default_baseline_not_in_tag_list() {
	assert_parse_error(
		"Default baseline \"ideo\" is not in BaseTagList",
		"table BASE {\
		     VertAxis.BaseTagList romn;\
		     VertAxis.BaseScriptList latn ideo 0;\
		 } BASE;",
	);
}

#[test]
fn test_table_unclosed() {
	for (table, statement) in [
		("OS/2", "WeightClass 400;"),
		("BASE", "HorizAxis.BaseTagList romn;"),
		("STAT", "ElidedFallbackNameID 2;"),
	] {
		for text in [
			"".to_owned(),
			statement.to_owned(),
			format!("{statement} # comment"),
		] {
			assert_parse_error("Expected '}'", &format!("table {table} {{ {text}"));
		}
	}
}

#[test]
fn test_table_os_2_vendor_not_ascii() {
	assert_parse_error(
		"Vendor ID must be ASCII",
		"table OS/2 { Vendor \"\u{00e9}\u{00e9}\"; } OS/2;",
	);
}

#[test]
fn test_table_os_2_unicode_range_bit_out_of_range() {
	assert_parse_error(
		"UnicodeRange bits must be between 0 and 127",
		"table OS/2 { UnicodeRange 0 200; } OS/2;",
	);
}

#[test]
fn test_table_bad_end() {
	assert_parse_error(
		"Expected \"GDEF\"",
		"table GDEF {LigatureCaretByPos f_i 400;} ABCD;",
	);
}

#[test]
fn test_table_comment() -> Result<()> {
	for table in ["BASE", "GDEF", "OS/2", "head", "hhea", "name", "vhea"] {
		let doc = parse(&format!("table {table} {{ # Comment\n }} {table};"))?;
		assert_eq!(get!(first(&doc), Comment).text, "# Comment");
	}
	Ok(())
}

#[test]
fn test_table_unsupported() {
	assert_parse_error(
		"\"table Foo\" is not supported",
		"table Foo {LigatureCaretByPos f_i 400;} Foo;",
	);
}

fn valuedef(doc: &FeatureFile) -> &ValueRecordDefinition {
	get!(first(doc), ValueRecordDefinition)
}

fn assert_no_devices(value: &ValueRecord) {
	assert!(value.x_pla_device.is_none());
	assert!(value.y_pla_device.is_none());
	assert!(value.x_adv_device.is_none());
	assert!(value.y_adv_device.is_none());
}

#[test]
fn test_valuerecord_format_a_horizontal() -> Result<()> {
	let doc = parse("feature liga {valueRecordDef 123 foo;} liga;")?;
	let vd = valuedef(&doc);
	let value = &vd.value;
	assert!(value.x_placement.is_none());
	assert!(value.y_placement.is_none());
	assert_eq!(opt_int(&value.x_advance), Some(123));
	assert!(value.y_advance.is_none());
	assert_no_devices(value);
	assert_eq!(vd.as_fea(), "valueRecordDef 123 foo;");
	assert_eq!(value.as_fea(), "123");
	Ok(())
}

#[test]
fn test_valuerecord_format_a_vertical() -> Result<()> {
	let doc = parse("feature vkrn {valueRecordDef 123 foo;} vkrn;")?;
	let vd = valuedef(&doc);
	let value = &vd.value;
	assert!(value.x_placement.is_none());
	assert!(value.y_placement.is_none());
	assert!(value.x_advance.is_none());
	assert_eq!(opt_int(&value.y_advance), Some(123));
	assert_no_devices(value);
	assert_eq!(vd.as_fea(), "valueRecordDef 123 foo;");
	assert_eq!(value.as_fea(), "123");
	Ok(())
}

#[test]
fn test_valuerecord_format_a_zero_horizontal() -> Result<()> {
	let doc = parse("feature liga {valueRecordDef 0 foo;} liga;")?;
	let vd = valuedef(&doc);
	let value = &vd.value;
	assert!(value.x_placement.is_none());
	assert!(value.y_placement.is_none());
	assert_eq!(opt_int(&value.x_advance), Some(0));
	assert!(value.y_advance.is_none());
	assert_no_devices(value);
	assert_eq!(vd.as_fea(), "valueRecordDef 0 foo;");
	assert_eq!(value.as_fea(), "0");
	Ok(())
}

#[test]
fn test_valuerecord_format_a_zero_vertical() -> Result<()> {
	let doc = parse("feature vkrn {valueRecordDef 0 foo;} vkrn;")?;
	let vd = valuedef(&doc);
	let value = &vd.value;
	assert!(value.x_placement.is_none());
	assert!(value.y_placement.is_none());
	assert!(value.x_advance.is_none());
	assert_eq!(opt_int(&value.y_advance), Some(0));
	assert_no_devices(value);
	assert_eq!(vd.as_fea(), "valueRecordDef 0 foo;");
	assert_eq!(value.as_fea(), "0");
	Ok(())
}

#[test]
fn test_valuerecord_format_a_vertical_contexts_() -> Result<()> {
	for tag in ["vkrn", "vpal", "vhal", "valt"] {
		let doc = parse(&format!("feature {tag} {{valueRecordDef 77 foo;}} {tag};"))?;
		let value = &valuedef(&doc).value;
		assert_eq!(
			opt_int(&value.y_advance),
			Some(77),
			"feature {tag} should be a vertical context for ValueRecord format A"
		);
	}
	Ok(())
}

#[test]
fn test_valuerecord_format_b() -> Result<()> {
	let doc = parse("feature liga {valueRecordDef <1 2 3 4> foo;} liga;")?;
	let vd = valuedef(&doc);
	let value = &vd.value;
	assert_eq!(opt_int(&value.x_placement), Some(1));
	assert_eq!(opt_int(&value.y_placement), Some(2));
	assert_eq!(opt_int(&value.x_advance), Some(3));
	assert_eq!(opt_int(&value.y_advance), Some(4));
	assert_no_devices(value);
	assert_eq!(vd.as_fea(), "valueRecordDef <1 2 3 4> foo;");
	assert_eq!(value.as_fea(), "<1 2 3 4>");
	Ok(())
}

#[test]
fn test_valuerecord_format_b_zero() -> Result<()> {
	let doc = parse("feature liga {valueRecordDef <0 0 0 0> foo;} liga;")?;
	let vd = valuedef(&doc);
	let value = &vd.value;
	assert_eq!(opt_int(&value.x_placement), Some(0));
	assert_eq!(opt_int(&value.y_placement), Some(0));
	assert_eq!(opt_int(&value.x_advance), Some(0));
	assert_eq!(opt_int(&value.y_advance), Some(0));
	assert_no_devices(value);
	assert_eq!(vd.as_fea(), "valueRecordDef <0 0 0 0> foo;");
	assert_eq!(value.as_fea(), "<0 0 0 0>");
	Ok(())
}

#[test]
fn test_valuerecord_format_c() -> Result<()> {
	let doc = parse(
		"feature liga {\
		     valueRecordDef <\
		         1 2 3 4\
		         <device 8 88>\
		         <device 11 111, 12 112>\
		         <device NULL>\
		         <device 33 -113, 44 -114, 55 115>\
		     > foo;\
		 } liga;",
	)?;
	let value = &valuedef(&doc).value;
	assert_eq!(opt_int(&value.x_placement), Some(1));
	assert_eq!(opt_int(&value.y_placement), Some(2));
	assert_eq!(opt_int(&value.x_advance), Some(3));
	assert_eq!(opt_int(&value.y_advance), Some(4));
	assert_eq!(value.x_pla_device, Some(vec![(8, 88)]));
	assert_eq!(value.y_pla_device, Some(vec![(11, 111), (12, 112)]));
	assert!(value.x_adv_device.is_none());
	assert_eq!(
		value.y_adv_device,
		Some(vec![(33, -113), (44, -114), (55, 115)])
	);
	assert_eq!(
		value.as_fea(),
		"<1 2 3 4 <device 8 88> <device 11 111, 12 112> <device NULL> <device 33 -113, 44 -114, 55 115>>"
	);
	Ok(())
}

#[test]
fn test_valuerecord_format_c_all_devices_null() -> Result<()> {
	let doc = parse(
		"feature test {\
		     valueRecordDef <-80 0 -160 0\
		         <device NULL> <device NULL> <device NULL> <device NULL>> foo;\
		 } test;",
	)?;
	let value = &valuedef(&doc).value;
	assert_eq!(opt_int(&value.x_placement), Some(-80));
	assert_eq!(opt_int(&value.y_placement), Some(0));
	assert_eq!(opt_int(&value.x_advance), Some(-160));
	assert_eq!(opt_int(&value.y_advance), Some(0));
	assert_no_devices(value);
	assert_eq!(value.as_fea(), "<-80 0 -160 0>");
	Ok(())
}

#[test]
fn test_valuerecord_format_d() -> Result<()> {
	let doc = parse("feature test {valueRecordDef <NULL> foo;} test;")?;
	let value = &valuedef(&doc).value;
	assert!(!value.is_truthy());
	assert_eq!(value.as_fea(), "<NULL>");
	Ok(())
}

#[test]
fn test_valuerecord_variable_scalar() -> Result<()> {
	let doc = parse(
		"feature test {valueRecordDef <0 (wght=200:-100 wght=900:-150 wdth=150,wght=900:-120) 0 0> foo;} test;",
	)?;
	let value = &valuedef(&doc).value;
	assert_eq!(
		value.as_fea(),
		"<0 (wght=200:-100 wght=900:-150 wdth=150,wght=900:-120) 0 0>"
	);
	Ok(())
}

#[test]
fn test_valuerecord_named() -> Result<()> {
	let doc = parse("valueRecordDef <1 2 3 4> foo;feature liga {valueRecordDef <foo> bar;} liga;")?;
	let value = &get!(&stmts(&doc.statements[1])[0], ValueRecordDefinition).value;
	assert_eq!(opt_int(&value.x_placement), Some(1));
	assert_eq!(opt_int(&value.y_placement), Some(2));
	assert_eq!(opt_int(&value.x_advance), Some(3));
	assert_eq!(opt_int(&value.y_advance), Some(4));
	Ok(())
}

#[test]
fn test_valuerecord_named_unknown() {
	assert_parse_error(
		"Unknown valueRecordDef \"unknown\"",
		"valueRecordDef <unknown> foo;",
	);
}

#[test]
fn test_valuerecord_scoping() -> Result<()> {
	let doc = parse(
		"valueRecordDef 789 foo;\
		 feature liga {valueRecordDef <foo> bar;} liga;\
		 feature smcp {valueRecordDef <foo> bar;} smcp;",
	)?;
	let foo = get!(&doc.statements[0], ValueRecordDefinition);
	assert_eq!(opt_int(&foo.value.x_advance), Some(789));
	for i in 1..3 {
		let v = get!(&stmts(&doc.statements[i])[0], ValueRecordDefinition);
		assert_eq!(opt_int(&v.value.x_advance), Some(789));
	}
	Ok(())
}

#[test]
fn test_valuerecord_device_value_out_of_range() {
	assert_parse_error(
		"Device value out of valid range (-128..127)",
		"valueRecordDef <1 2 3 4 <device NULL> <device NULL> \
		 <device NULL> <device 11 128>> foo;",
	);
}

#[test]
fn test_conditionset() -> Result<()> {
	let doc = parse("conditionset heavy { wght 700 900; } heavy;")?;
	let value = get!(&doc.statements[0], ConditionsetStatement);
	assert_eq!(
		value.conditions.get("wght"),
		Some(&(IntOrFloat::Int(700), IntOrFloat::Int(900)))
	);
	assert_eq!(
		value.as_fea(""),
		"conditionset heavy {\n    wght 700 900;\n} heavy;\n"
	);

	let doc = parse("conditionset heavy { wght 700 900; opsz 17 18;} heavy;")?;
	let value = get!(&doc.statements[0], ConditionsetStatement);
	assert_eq!(
		value.conditions.get("wght"),
		Some(&(IntOrFloat::Int(700), IntOrFloat::Int(900)))
	);
	assert_eq!(
		value.conditions.get("opsz"),
		Some(&(IntOrFloat::Int(17), IntOrFloat::Int(18)))
	);
	assert_eq!(
		value.as_fea(""),
		"conditionset heavy {\n    wght 700 900;\n    opsz 17 18;\n} heavy;\n"
	);
	Ok(())
}

#[test]
fn test_conditionset_same_axis() {
	assert_parse_error(
		"Repeated condition for axis wght",
		"conditionset heavy { wght 700 900; wght 100 200; } heavy;",
	);
}

#[test]
fn test_conditionset_float() -> Result<()> {
	let doc = parse("conditionset heavy { wght 700.0 900.0; } heavy;")?;
	let value = get!(&doc.statements[0], ConditionsetStatement);
	assert_eq!(
		value.conditions.get("wght"),
		Some(&(IntOrFloat::Float(700.0), IntOrFloat::Float(900.0)))
	);
	assert_eq!(
		value.as_fea(""),
		"conditionset heavy {\n    wght 700.0 900.0;\n} heavy;\n"
	);
	Ok(())
}

#[test]
fn test_variation() -> Result<()> {
	let doc = parse("variation rvrn heavy { sub a by b; } rvrn;")?;
	get!(&doc.statements[0], VariationBlock);
	Ok(())
}

#[test]
fn test_languagesystem() -> Result<()> {
	let doc = parse("languagesystem latn DEU;")?;
	let langsys = get!(&doc.statements[0], LanguageSystemStatement);
	assert_eq!(langsys.script, "latn");
	assert_eq!(langsys.language, "DEU ");
	let doc = parse("languagesystem DFLT DEU;")?;
	let langsys = get!(&doc.statements[0], LanguageSystemStatement);
	assert_eq!(langsys.script, "DFLT");
	assert_eq!(langsys.language, "DEU ");
	assert_parse_error(
		"\"dflt\" is not a valid script tag; use \"DFLT\" instead",
		"languagesystem dflt dflt;",
	);
	assert_parse_error(
		"\"DFLT\" is not a valid language tag; use \"dflt\" instead",
		"languagesystem latn DFLT;",
	);
	assert_parse_error("Expected ';'", "languagesystem latn DEU");
	assert_parse_error("longer than 4 characters", "languagesystem foobar DEU;");
	assert_parse_error("longer than 4 characters", "languagesystem latn FOOBAR;");
	Ok(())
}

#[test]
fn test_empty_statement_ignored() -> Result<()> {
	let doc = parse("feature test {;} test;")?;
	assert!(stmts(&doc.statements[0]).is_empty());
	let doc = parse(";;;")?;
	assert!(doc.statements.is_empty());
	for table in ["BASE", "GDEF", "OS/2", "head", "hhea", "name", "vhea"] {
		let doc = parse(&format!("table {table} {{ ;;; }} {table};"))?;
		assert!(stmts(&doc.statements[0]).is_empty());
	}
	Ok(())
}

#[test]
fn test_ufo_features_parse_include_dir() -> Result<()> {
	let fea_path = getpath("include/test.ufo/features.fea");
	let include_dir = fea_path
		.parent()
		.and_then(|p| p.parent())
		.map(PathBuf::from);
	let doc =
		Parser::with_options(fea_path.clone(), Vec::<String>::new(), true, include_dir)?.parse()?;
	assert_eq!(doc.statements.len(), 1);
	assert_eq!(get!(&doc.statements[0], Comment).text, "# Nothing");
	Ok(())
}

#[test]
fn test_unmarked_ignore_statement() -> Result<()> {
	let (records, doc) = capture(|| parse("lookup foo { ignore sub A; } foo;"));
	let doc = doc?;
	assert_eq!(first(&doc).as_fea("")?, "ignore sub A';");
	assert_eq!(records.len(), 1);
	assert!(
		records[0].contains("Ambiguous \"ignore sub\", there should be least one marked glyph")
	);
	Ok(())
}

#[test]
fn test_variable_scalar_default() -> Result<()> {
	let doc = parse(
		"feature test {valueRecordDef <0 (100 wght=200:-100 wght=900:-150 wdth=150,wght=900:-120) 0 0> foo;} test;",
	)?;
	let value = &valuedef(&doc).value;
	assert_eq!(
		value.as_fea(),
		"<0 (100 wght=200:-100 wght=900:-150 wdth=150,wght=900:-120) 0 0>"
	);
	Ok(())
}

#[test]
fn test_variable_scalar_duplicate_bare_value() {
	assert_parse_error(
		"Duplicate value for the default location",
		"feature test {pos a (10 20 wght=900:30);} test",
	);
}

mod symbol_table_test {
	use super::*;

	#[test]
	fn test_scopes() {
		let mut symtab = SymbolTable::new();
		symtab.define("foo", 23);
		assert_eq!(symtab.resolve("foo"), Some(&23));
		symtab.enter_scope();
		assert_eq!(symtab.resolve("foo"), Some(&23));
		symtab.define("foo", 42);
		assert_eq!(symtab.resolve("foo"), Some(&42));
		symtab.exit_scope();
		assert_eq!(symtab.resolve("foo"), Some(&23));
	}

	#[test]
	fn test_resolve_undefined() {
		assert_eq!(SymbolTable::<i32>::new().resolve("abc"), None);
	}
}

/// Quirks of fontTools that are reproduced (checked against fontTools 4.68.0).
mod quirks {
	use super::*;

	/// An `include` at the end of the text ends the token stream silently.
	#[test]
	fn test_include_at_end_of_text() -> Result<()> {
		let doc = parse_with("languagesystem DFLT dflt; include", vec![], true)?;
		assert_eq!(doc.as_fea()?, "languagesystem DFLT dflt;");
		Ok(())
	}

	/// A condition without numbers keeps the values of the previous one.
	#[test]
	fn test_conditionset_missing_values() -> Result<()> {
		let doc = parse_with("conditionset x { wght 1 2; opsz ; } x;", vec![], true)?;
		assert_eq!(
			doc.as_fea()?,
			"conditionset x {\n    wght 1 2;\n    opsz 1 2;\n} x;\n"
		);
		match parse_with("conditionset x { opsz ; } x;", vec![], true) {
			Err(Error::Value(m)) => assert!(m.contains("'min_value'"), "{m}"),
			other => panic!("{other:?}"),
		}
		Ok(())
	}

	/// `pos A B;` parses to a pair positioning without values, which `asFea()`
	/// cannot write.
	#[test]
	fn test_pair_pos_without_values() -> Result<()> {
		let doc = parse_with("feature f { pos A B; } f;", vec![], true)?;
		match doc.as_fea() {
			Err(Error::Value(m)) => {
				assert_eq!(m, "'NoneType' object has no attribute 'asFea'");
			}
			other => panic!("{other:?}"),
		}
		Ok(())
	}
}
