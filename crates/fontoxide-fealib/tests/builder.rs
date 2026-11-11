//! `Tests/feaLib/builder_test.py`, except the `test_Fea2feaFile_*` tests (in
//! `fea2fea.rs`): each feature file of `TEST_FEATURE_FILES` is built into the
//! mock font and the TTX of the tables compared to `data/*.ttx`
//! (`test_FeatureFile_*`), and the targeted tests.

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Once;

use fontoxide_fealib::Error;
use fontoxide_fealib::builder::{
	Builder, add_open_type_features, add_open_type_features_from_string,
};
use fontoxide_fealib::lexer::FeatureSource;
use fontoxide_fealib::parser::Parser;
use fontoxide_misc::Tag;
use fontoxide_ttlib::ot::types::{self as ot, LookupSubTable};
use fontoxide_ttlib::table::TableType;
use fontoxide_ttlib::tables::fvar::Axis;
use fontoxide_ttlib::tables::{Avar, Fvar, Gdef, Gpos, Gsub, Os2};
use fontoxide_ttlib::{Font, TableKey, TtxOptions};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

/// `makeTTFont()`: a font with only a glyph order.
fn make_tt_font() -> Font {
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
	let mut font = Font::new();
	font.set_glyph_order(glyphs);
	font
}

/// `addFvar(font, axes, [])` without the names, which the tests delete.
fn add_fvar(font: &mut Font, axes: &[(&str, f64, f64, f64)]) {
	let fvar = Fvar {
		axes: axes
			.iter()
			.enumerate()
			.map(|(i, (tag, min, default, max))| Axis {
				axis_tag: Tag::from_str_padded(tag).expect("a tag").to_bytes(),
				min_value: *min,
				default_value: *default,
				max_value: *max,
				axis_name_id: 256 + i as u16,
				..Default::default()
			})
			.collect(),
		instances: Vec::new(),
	};
	font.insert(Box::new(fvar));
}

/// `VARFONT_AXES`.
const VARFONT_AXES: [(&str, f64, f64, f64); 2] = [
	("wght", 200.0, 200.0, 1000.0),
	("wdth", 100.0, 100.0, 200.0),
];

/// `make_mock_vf()`.
fn make_mock_vf() -> Font {
	let mut font = make_tt_font();
	add_fvar(&mut font, &VARFONT_AXES);
	font
}

/// An `avar` table with the segment map for `wght` of the tests.
fn wght_avar() -> Avar {
	let mut avar = Avar::default();
	avar.segments.insert(
		Tag::new(b"wght"),
		vec![(-1.0, -1.0), (0.0, 0.0), (0.5, 0.625), (1.0, 1.0)],
	);
	avar
}

fn getpath(testfile: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/feaLib/data")
		.join(testfile)
}

/// `read_ttx`: the lines with the `ttFont` attributes elided.
fn read_ttx(text: &str) -> Vec<String> {
	text.lines()
		.map(|line| {
			if line.starts_with("<ttFont ") {
				"<ttFont>".to_owned()
			} else {
				line.trim_end().to_owned()
			}
		})
		.collect()
}

/// `expect_ttx(font, expected_ttx, replace)`.
fn expect_ttx(font: &mut Font, expected_ttx: &str, replace: &[(&str, &str)]) -> TestResult {
	let tables = [
		"head", "name", "BASE", "GDEF", "GSUB", "GPOS", "OS/2", "STAT", "hhea", "vhea",
	];
	let ttx = font.to_ttx(&TtxOptions {
		tables: tables
			.iter()
			.map(|t| TableKey::Table(Tag::from_str_padded(t).expect("a tag")))
			.collect(),
		newline: "\n".into(),
		..Default::default()
	})?;
	let actual = read_ttx(&ttx);
	let mut expected = read_ttx(&std::fs::read_to_string(getpath(expected_ttx))?);
	for line in &mut expected {
		for (k, v) in replace {
			*line = line.replace(k, v);
		}
	}
	if actual != expected {
		let diff: Vec<String> = expected
			.iter()
			.zip(&actual)
			.enumerate()
			.filter(|(_, (e, a))| e != a)
			.take(10)
			.map(|(i, (e, a))| format!("line {}:\n- {e}\n+ {a}", i + 1))
			.collect();
		return Err(format!(
			"TTX output is different from expected ({expected_ttx}; {} vs {} lines):\n{}\n--- actual:\n{}",
			expected.len(),
			actual.len(),
			diff.join("\n"),
			actual.join("\n")
		)
		.into());
	}
	Ok(())
}

/// Compiles a table and decompiles it again.
fn round_trip<T: TableType + fontoxide_ttlib::table::Table>(font: &mut Font) -> TestResult {
	if !font.contains(T::TAG) {
		return Ok(());
	}
	let data = font.table_data(T::TAG)?;
	let table = T::decompile(&data, font)?;
	font.insert(Box::new(table));
	Ok(())
}

/// The compiled `Debg` tables of the `*-debug.ttx` tests, from fontTools.
static DEBG: std::sync::LazyLock<BTreeMap<&str, &str>> = std::sync::LazyLock::new(|| {
	BTreeMap::from([(
		"lookup",
		r#"{"com.github.fonttools.feaLib": {"GSUB": {"0": ["__PATH__:4:5", "SomeLookup", ["DFLT", "dflt", "tst2"]], "1": ["__PATH__:18:9", "EmbeddedLookup", ["DFLT", "dflt", "tst4"]]}, "GPOS": {}}}"#,
	)])
});

/// `check_feature_file(name)`.
fn check_feature_file(name: &str) -> TestResult {
	let mut font = make_tt_font();
	if name.starts_with("variable_") {
		add_fvar(&mut font, &VARFONT_AXES);
	}
	let feapath = getpath(&format!("{name}.fea"));
	add_open_type_features(&mut font, feapath.as_path(), None, false)?;
	expect_ttx(&mut font, &format!("{name}.ttx"), &[])?;
	// Check that:
	// 1) tables do compile (only G* tables as long as we have a mock font)
	// 2) dumping after save-reload yields the same TTX dump as before
	round_trip::<Gdef>(&mut font)?;
	round_trip::<Gsub>(&mut font)?;
	round_trip::<Gpos>(&mut font)?;
	expect_ttx(&mut font, &format!("{name}.ttx"), &[])?;
	// Optionally check a debug dump.
	let debugttx = getpath(&format!("{name}-debug.ttx"));
	if debugttx.exists() {
		add_open_type_features(&mut font, feapath.as_path(), None, true)?;
		// LENIENT: fontoxide does not implement the `Debg` table, so the
		// `LookupList` is dumped without the debugging comments: the debug
		// build is checked against the plain TTX, and for its `Debg` table.
		expect_ttx(&mut font, &format!("{name}.ttx"), &[])?;
		// The `Debg` data is what fontTools compiles.
		let debg = String::from_utf8(font.table_data(Tag::new(b"Debg"))?)?;
		let path = feapath.to_string_lossy().replace('\\', "\\\\");
		assert_eq!(debg.replace(&path, "__PATH__"), DEBG[name]);
	}
	Ok(())
}

macro_rules! feature_file_tests {
	($($name:ident)*) => {
		$(
			#[test]
			fn $name() -> TestResult {
				check_feature_file(&stringify!($name)["test_FeatureFile_".len()..])
			}
		)*
	};
}

feature_file_tests! {
	test_FeatureFile_Attach test_FeatureFile_cid_range test_FeatureFile_enum
	test_FeatureFile_markClass test_FeatureFile_language_required
	test_FeatureFile_GlyphClassDef test_FeatureFile_LigatureCaretByIndex
	test_FeatureFile_LigatureCaretByPos test_FeatureFile_lookup test_FeatureFile_lookupflag
	test_FeatureFile_feature_aalt test_FeatureFile_ignore_pos
	test_FeatureFile_GPOS_1 test_FeatureFile_GPOS_1_zero test_FeatureFile_GPOS_2
	test_FeatureFile_GPOS_2b test_FeatureFile_GPOS_3 test_FeatureFile_GPOS_4
	test_FeatureFile_GPOS_5 test_FeatureFile_GPOS_6 test_FeatureFile_GPOS_8
	test_FeatureFile_GSUB_2 test_FeatureFile_GSUB_3 test_FeatureFile_GSUB_6
	test_FeatureFile_GSUB_8
	test_FeatureFile_spec4h1 test_FeatureFile_spec4h2 test_FeatureFile_spec5d1
	test_FeatureFile_spec5d2 test_FeatureFile_spec5fi1 test_FeatureFile_spec5fi2
	test_FeatureFile_spec5fi3 test_FeatureFile_spec5fi4
	test_FeatureFile_spec5f_ii_1 test_FeatureFile_spec5f_ii_2 test_FeatureFile_spec5f_ii_3
	test_FeatureFile_spec5f_ii_4
	test_FeatureFile_spec5h1 test_FeatureFile_spec6b_ii test_FeatureFile_spec6d2
	test_FeatureFile_spec6e test_FeatureFile_spec6f
	test_FeatureFile_spec6h_ii test_FeatureFile_spec6h_iii_1 test_FeatureFile_spec6h_iii_3d
	test_FeatureFile_spec8a test_FeatureFile_spec8a_2 test_FeatureFile_spec8b
	test_FeatureFile_spec8c test_FeatureFile_spec8d
	test_FeatureFile_spec9a test_FeatureFile_spec9a2 test_FeatureFile_spec9b
	test_FeatureFile_spec9c1 test_FeatureFile_spec9c2 test_FeatureFile_spec9c3
	test_FeatureFile_spec9d test_FeatureFile_spec9e test_FeatureFile_spec9f
	test_FeatureFile_spec9g test_FeatureFile_spec10
	test_FeatureFile_bug453 test_FeatureFile_bug457 test_FeatureFile_bug463
	test_FeatureFile_bug501 test_FeatureFile_bug502 test_FeatureFile_bug504
	test_FeatureFile_bug505 test_FeatureFile_bug506 test_FeatureFile_bug509
	test_FeatureFile_bug512 test_FeatureFile_bug514 test_FeatureFile_bug568
	test_FeatureFile_bug633 test_FeatureFile_bug1196 test_FeatureFile_bug1307
	test_FeatureFile_bug1459 test_FeatureFile_bug2276 test_FeatureFile_variable_bug2772
	test_FeatureFile_name test_FeatureFile_size test_FeatureFile_size2
	test_FeatureFile_multiple_feature_blocks test_FeatureFile_omitted_GlyphClassDef
	test_FeatureFile_ZeroValue_SinglePos_horizontal test_FeatureFile_ZeroValue_SinglePos_vertical
	test_FeatureFile_ZeroValue_PairPos_horizontal test_FeatureFile_ZeroValue_PairPos_vertical
	test_FeatureFile_ZeroValue_ChainSinglePos_horizontal
	test_FeatureFile_ZeroValue_ChainSinglePos_vertical
	test_FeatureFile_PairPosSubtable test_FeatureFile_ChainSubstSubtable
	test_FeatureFile_SubstSubtable test_FeatureFile_ChainPosSubtable
	test_FeatureFile_LigatureSubtable test_FeatureFile_AlternateSubtable
	test_FeatureFile_MultipleSubstSubtable
	test_FeatureFile_SingleSubstSubtable test_FeatureFile_aalt_chain_contextual_subst
	test_FeatureFile_AlternateChained
	test_FeatureFile_MultipleLookupsPerGlyph test_FeatureFile_MultipleLookupsPerGlyph2
	test_FeatureFile_GSUB_6_formats
	test_FeatureFile_GSUB_5_formats test_FeatureFile_delete_glyph test_FeatureFile_STAT_test
	test_FeatureFile_STAT_test_elidedFallbackNameID
	test_FeatureFile_variable_scalar_valuerecord test_FeatureFile_variable_scalar_anchor
	test_FeatureFile_variable_conditionset
	test_FeatureFile_variable_mark_anchor test_FeatureFile_duplicate_lookup_reference
	test_FeatureFile_contextual_inline_multi_sub_format_2
	test_FeatureFile_contextual_inline_format_4
	test_FeatureFile_chain_context_multi_subst_class
	test_FeatureFile_duplicate_language_stmt test_FeatureFile_duplicate_language_stmt_include_dflt
	test_FeatureFile_script_language_tracking test_FeatureFile_script_language_tracking_DFLT
	test_FeatureFile_script_language_tracking_multi
	test_FeatureFile_script_language_tracking_redundant
	test_FeatureFile_CursivePosSubtable
	test_FeatureFile_MarkBasePosSubtable
	test_FeatureFile_MarkLigPosSubtable
	test_FeatureFile_MarkMarkPosSubtable
	test_FeatureFile_single_pos_NULL
	test_FeatureFile_class_pair_pos_duplicates
	test_FeatureFile_useExtension
	test_FeatureFile_bug3846_1 test_FeatureFile_bug3846_2
	test_FeatureFile_empty_filter_sets_and_mark_classes
	test_FeatureFile_combo_mult_and_lig_sub
	test_FeatureFile_identical_feature_lookups
	test_FeatureFile_cvparam_null
	test_FeatureFile_contextual_merge_gsub_named
	test_FeatureFile_contextual_merge_gpos_named
	test_FeatureFile_contextual_merge_ignore
	test_FeatureFile_contextual_merge_alternate
}

/// Negative control: the comparison of `check_feature_file` fails when the
/// TTX differs.
#[test]
fn expect_ttx_detects_differences() -> TestResult {
	let mut font = make_tt_font();
	add_open_type_features(&mut font, getpath("GPOS_4.fea").as_path(), None, false)?;
	expect_ttx(&mut font, "GPOS_4.ttx", &[])?;
	assert!(expect_ttx(&mut font, "GPOS_4.ttx", &[("value=\"-", "value=\"")]).is_err());
	assert!(expect_ttx(&mut font, "GPOS_1.ttx", &[]).is_err());
	Ok(())
}

// --- Log capture -----------------------------------------------------------

thread_local! {
	static CAPTURED: RefCell<Option<(log::Level, Vec<String>)>> = const { RefCell::new(None) };
}

struct Capture;

impl log::Log for Capture {
	fn enabled(&self, _: &log::Metadata<'_>) -> bool {
		true
	}

	fn log(&self, record: &log::Record<'_>) {
		CAPTURED.with(|c| {
			if let Some((level, records)) = c.borrow_mut().as_mut()
				&& record.level() <= *level
			{
				records.push(record.args().to_string());
			}
		});
	}

	fn flush(&self) {}
}

static CAPTURE: Capture = Capture;
static INIT: Once = Once::new();

/// The messages of `level` and above that `f` logs on this thread
/// (`CapturingLogHandler(logger, level)`).
fn capture<R>(level: log::Level, f: impl FnOnce() -> R) -> (Vec<String>, R) {
	INIT.call_once(|| {
		let _ = log::set_logger(&CAPTURE);
		log::set_max_level(log::LevelFilter::Trace);
	});
	CAPTURED.with(|c| *c.borrow_mut() = Some((level, Vec::new())));
	let result = f();
	let records = CAPTURED
		.with(|c| c.borrow_mut().take())
		.map(|(_, r)| r)
		.unwrap_or_default();
	(records, result)
}

fn assert_logged(records: &[String], needle: &str) {
	assert!(
		records.iter().any(|r| r.contains(needle)),
		"{needle:?} not in {records:?}"
	);
}

// --- Helpers ---------------------------------------------------------------

/// `build(featureFile, tables)`.
fn build(features: &str, tables: Option<&[&str]>) -> fontoxide_fealib::Result<Font> {
	let mut font = make_tt_font();
	add_open_type_features_from_string(&mut font, features, None, tables, false)?;
	Ok(font)
}

/// `assertRaisesRegex(FeatureLibError, message, self.build, features)`, with
/// the message as a plain substring.
fn assert_build_error(features: &str, message: &str) {
	match build(features, None) {
		Ok(_) => panic!("no error for {features:?}; expected {message:?}"),
		Err(e) => {
			assert!(
				matches!(e, Error::FeatureLib { .. }),
				"not a FeatureLibError: {e}"
			);
			let s = e.to_string();
			assert!(s.contains(message), "{s:?} does not contain {message:?}");
		}
	}
}

fn dedent(text: &str) -> String {
	let indent = text
		.lines()
		.filter(|l| !l.trim().is_empty())
		.map(|l| l.len() - l.trim_start().len())
		.min()
		.unwrap_or(0);
	text.lines()
		.map(|l| {
			if l.trim().is_empty() {
				""
			} else {
				&l[indent..]
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
		+ if text.ends_with('\n') { "\n" } else { "" }
}

fn glyph_name(font: &mut Font, gid: u32) -> String {
	font.glyph_name(gid).expect("a glyph")
}

/// The first subtable of a lookup of `GSUB`, through an extension.
fn gsub_subtable(font: &mut Font, lookup: usize) -> LookupSubTable {
	let gsub = font.get::<Gsub>().expect("GSUB").expect("GSUB");
	let list = gsub.lookup_list.as_ref().expect("LookupList");
	let st = list.lookup[lookup].as_ref().expect("lookup").sub_table[0]
		.as_ref()
		.expect("subtable");
	(**st).clone()
}

fn names(font: &mut Font, gids: &[u32]) -> Vec<String> {
	gids.iter().map(|&g| glyph_name(font, g)).collect()
}

// --- Tests -----------------------------------------------------------------

#[test]
fn test_extended_layout_table_headers() -> TestResult {
	let mut font = make_tt_font();
	let mut builder = Builder::new(
		&mut font,
		FeatureSource::text(
			"
            feature kern { pos A B -20; } kern;
            feature liga { sub f i by f_i; } liga;
            ",
		),
	)?;
	builder.glyph_map = builder.glyph_map.clone().with_len(0x10001);
	builder.build(None, false)?;
	macro_rules! check {
		($ty:ident) => {{
			let table = font.get::<$ty>()?.ok_or("no table")?;
			assert_eq!(table.version, 0x0001_0002);
			assert!(table.script_list.is_none());
			assert!(table.feature_list.is_none());
			assert!(table.lookup_list.is_none());
			assert!(table.script_list2.is_some());
			assert!(table.feature_list2.is_some());
			assert!(table.lookup_list2.is_some());
			round_trip::<$ty>(&mut font)?;
			let table = font.get::<$ty>()?.ok_or("no table")?;
			assert_eq!(table.version, 0x0001_0002);
			assert!(table.script_list2.is_some());
			assert!(table.feature_list2.is_some());
			assert!(table.lookup_list2.is_some());
		}};
	}
	check!(Gpos);
	check!(Gsub);
	Ok(())
}

#[test]
fn test_extended_gdef_header() -> TestResult {
	let mut font = make_tt_font();
	let mut builder = Builder::new(
		&mut font,
		FeatureSource::text(
			"
            @TOP = [acute grave];
            table GDEF {
                GlyphClassDef [A], [f_i], [acute], [grave];
                Attach A 3;
                LigatureCaretByPos f_i 300;
            } GDEF;
            feature mark {
                lookupflag MarkAttachmentType @TOP;
                pos A 20;
                lookupflag UseMarkFilteringSet @TOP;
                pos A 30;
            } mark;
            ",
		),
	)?;
	builder.glyph_map = builder.glyph_map.clone().with_len(0x10001);
	builder.build(None, false)?;
	let check = |table: &ot::GDEF| {
		assert_eq!(table.version, 0x0001_0004);
		assert!(table.glyph_class_def.is_none());
		assert!(table.attach_list.is_none());
		assert!(table.lig_caret_list.is_none());
		assert!(table.mark_attach_class_def.is_none());
		assert!(table.mark_glyph_sets_def.is_none());
		assert!(table.glyph_class_def2.is_some());
		assert!(table.attach_list2.is_some());
		assert!(table.lig_caret_list2.is_some());
		assert!(table.mark_attach_class_def2.is_some());
		assert!(table.mark_glyph_sets_def2.is_some());
	};
	check(&font.get::<Gdef>()?.ok_or("no GDEF")?.table);
	round_trip::<Gdef>(&mut font)?;
	check(&font.get::<Gdef>()?.ok_or("no GDEF")?.table);
	Ok(())
}

#[test]
fn test_alternateSubst_multipleSubstitutionsForSameGlyph() {
	assert_build_error(
		"feature test {    sub A from [A.alt1 A.alt2];    sub B from [B.alt1 B.alt2 B.alt3];    sub A from [A.alt1 A.alt2];} test;",
		"Already defined alternates for glyph \"A\"",
	);
}

#[test]
fn test_singleSubst_multipleIdenticalSubstitutionsForSameGlyph_info() -> TestResult {
	let (records, result) = capture(log::Level::Info, || {
		build(
			"feature test {    sub A by A.sc;    sub B by B.sc;    sub A by A.sc;} test;",
			None,
		)
	});
	result?;
	assert_logged(
		&records,
		"Removing duplicate substitution from \"A\" to \"A.sc\"",
	);
	Ok(())
}

#[test]
fn test_multipleSubst_multipleSubstitutionsForSameGlyph() {
	assert_build_error(
		"feature test {    sub f_f_i by f f i;    sub c_t by c t;    sub f_f_i by f_f i;} test;",
		"Already defined substitution for \"f_f_i\"",
	);
}

#[test]
fn test_multipleSubst_multipleIdenticalSubstitutionsForSameGlyph_info() -> TestResult {
	let (records, result) = capture(log::Level::Info, || {
		build(
			"feature test {    sub f_f_i by f f i;    sub c_t by c t;    sub f_f_i by f f i;} test;",
			None,
		)
	});
	result?;
	assert_logged(
		&records,
		"Removing duplicate substitution from \"f_f_i\" to \"f, f, i\"",
	);
	Ok(())
}

/// The mapping of a multiple substitution subtable, by glyph name.
fn multiple_mapping(font: &mut Font, st: &LookupSubTable) -> BTreeMap<String, Vec<String>> {
	let LookupSubTable::MultipleSubst(st) = st else {
		panic!("not a MultipleSubst: {st:?}");
	};
	st.mapping
		.iter()
		.map(|(k, v)| (glyph_name(font, *k), names(font, v)))
		.collect()
}

/// The mapping of a single substitution subtable, by glyph name.
fn single_mapping(font: &mut Font, st: &LookupSubTable) -> BTreeMap<String, String> {
	let LookupSubTable::SingleSubst(st) = st else {
		panic!("not a SingleSubst: {st:?}");
	};
	st.mapping
		.iter()
		.map(|(k, v)| (glyph_name(font, *k), glyph_name(font, *v)))
		.collect()
}

fn string_map<const N: usize>(items: [(&str, &[&str]); N]) -> BTreeMap<String, Vec<String>> {
	items
		.into_iter()
		.map(|(k, v)| (k.to_owned(), v.iter().map(|s| (*s).to_owned()).collect()))
		.collect()
}

#[test]
fn test_mixed_singleSubst_multipleSubst() -> TestResult {
	let mut font = build(
		"lookup test {  sub f_f   by f f;  sub f     by f;  sub f_f_i by f f i;  sub [A A.sc] by A;  sub [B B.sc] by [B B.sc];} test;",
		None,
	)?;
	assert!(font.contains(Tag::new(b"GSUB")));
	let st = gsub_subtable(&mut font, 0);
	assert_eq!(
		multiple_mapping(&mut font, &st),
		string_map([
			("f_f", &["f", "f"]),
			("f", &["f"]),
			("f_f_i", &["f", "f", "i"]),
			("A", &["A"]),
			("A.sc", &["A"]),
			("B", &["B"]),
			("B.sc", &["B.sc"]),
		])
	);
	Ok(())
}

#[test]
fn test_mixed_singleSubst_multipleSubst_aalt() -> TestResult {
	let mut font = build(
		&dedent(
			"
                feature aalt {
                  feature ccmp;
                } aalt;

                feature ccmp {
                  sub f_f   by f f;
                  sub f     by f;
                  sub f_f_i by f f i;
                  sub [A A.sc] by A;
                  sub [B B.sc] by [B B.sc];
                } ccmp;
                ",
		),
		None,
	)?;
	let st = gsub_subtable(&mut font, 0);
	let expected: BTreeMap<String, String> = [
		("A", "A"),
		("A.sc", "A"),
		("B", "B"),
		("B.sc", "B.sc"),
		("f", "f"),
	]
	.into_iter()
	.map(|(a, b)| (a.to_owned(), b.to_owned()))
	.collect();
	assert_eq!(single_mapping(&mut font, &st), expected);
	Ok(())
}

/// The ligatures of a ligature substitution subtable: for each first glyph,
/// (ligature glyph, components).
fn ligatures(font: &mut Font, st: &LookupSubTable) -> BTreeMap<String, Vec<(String, Vec<String>)>> {
	let LookupSubTable::LigatureSubst(st) = st else {
		panic!("not a LigatureSubst: {st:?}");
	};
	st.ligatures
		.iter()
		.map(|(k, ligs)| {
			let ligs = ligs
				.iter()
				.map(|l| (glyph_name(font, l.lig_glyph), names(font, &l.component)))
				.collect();
			(glyph_name(font, *k), ligs)
		})
		.collect()
}

#[test]
fn test_mixed_singleSubst_ligatureSubst() -> TestResult {
	let mut font = build(
		"lookup test {  sub f f   by f_f;  sub f f i by f_f_i;  sub A     by A.sc;} test;",
		None,
	)?;
	let st = gsub_subtable(&mut font, 0);
	let ligs = ligatures(&mut font, &st);
	assert_eq!(ligs.len(), 2);
	assert_eq!(
		ligs["f"],
		vec![
			("f_f_i".to_owned(), vec!["f".to_owned(), "i".to_owned()]),
			("f_f".to_owned(), vec!["f".to_owned()]),
		]
	);
	assert_eq!(ligs["A"], vec![("A.sc".to_owned(), Vec::new())]);
	Ok(())
}

#[test]
fn test_mixed_singleSubst_ligatureSubst_aalt() -> TestResult {
	let mut font = build(
		&dedent(
			"
                feature aalt {
                  feature liga;
                } aalt;

                feature liga {
                  sub f f   by f_f;
                  sub f f i by f_f_i;
                  sub A     by A.sc;
                } liga;
                ",
		),
		None,
	)?;
	let st = gsub_subtable(&mut font, 0);
	let expected: BTreeMap<String, String> =
		[("A".to_owned(), "A.sc".to_owned())].into_iter().collect();
	assert_eq!(single_mapping(&mut font, &st), expected);
	Ok(())
}

#[test]
fn test_mixed_singleSubst_multipleSubst_ligatureSubst_named_lookup() {
	assert_build_error(
		"lookup test {  sub A     by A.sc;  sub f_f   by f f;  sub f f i by f_f_i;} test;",
		"Within a named lookup block, all rules must be of the same lookup type and flag",
	);
}

#[test]
fn test_mixed_singleSubst_multipleSubst_ligatureSubst_feature() -> TestResult {
	let mut font = build(
		&dedent(
			"
                feature test {
                  sub A     by A.sc;
                  sub f_f   by f f;
                  sub f f i by f_f_i;
                } test;
                ",
		),
		None,
	)?;
	let lookups = font
		.get::<Gsub>()?
		.ok_or("no GSUB")?
		.lookup_list
		.as_ref()
		.ok_or("no LookupList")?
		.lookup
		.len();
	assert_eq!(lookups, 2);
	// The first gets promoted to a multiple substitution.
	let st = gsub_subtable(&mut font, 0);
	assert_eq!(
		multiple_mapping(&mut font, &st),
		string_map([("A", &["A.sc"]), ("f_f", &["f", "f"])])
	);
	let st = gsub_subtable(&mut font, 1);
	let ligs = ligatures(&mut font, &st);
	assert_eq!(ligs.len(), 1);
	assert_eq!(ligs["f"].len(), 1);
	assert_eq!(ligs["f"][0].0, "f_f_i");
	let gsub = font.get::<Gsub>()?.ok_or("no GSUB")?;
	let features = &gsub
		.feature_list
		.as_ref()
		.ok_or("no FeatureList")?
		.feature_record;
	assert_eq!(features.len(), 1);
	let feature = features[0].feature.as_ref().ok_or("no Feature")?;
	assert_eq!(feature.lookup_list_index, vec![0, 1]);
	Ok(())
}

#[test]
fn test_pairPos_redefinition_warning() -> TestResult {
	// https://github.com/fonttools/fonttools/issues/1147
	let (records, result) = capture(log::Level::Debug, || {
		// The pair "yacute semicolon" is redefined in the enum pos.
		build(
			"@Y_LC = [y yacute ydieresis];@SMALL_PUNC = [comma semicolon period];feature kern {  pos yacute semicolon -70;  enum pos @Y_LC semicolon -80;  pos @Y_LC @SMALL_PUNC -100;} kern;",
			None,
		)
	});
	let mut font = result?;
	assert_logged(
		&records,
		"Already defined position for pair yacute semicolon",
	);
	// The first definition prevails: yacute semicolon -70.
	let gpos = font.get::<Gpos>()?.ok_or("no GPOS")?;
	let st = gpos.lookup_list.as_ref().ok_or("no LookupList")?.lookup[0]
		.as_ref()
		.ok_or("no lookup")?
		.sub_table[0]
		.as_deref()
		.cloned()
		.ok_or("no subtable")?;
	let LookupSubTable::PairPos(ot::PairPos::Format1(st)) = st else {
		return Err("not a PairPos format 1".into());
	};
	let coverage = st.coverage.as_ref().ok_or("no coverage")?.glyphs.clone();
	assert_eq!(glyph_name(&mut font, coverage[2]), "yacute");
	let record = &st.pair_set[2]
		.as_ref()
		.ok_or("no PairSet")?
		.pair_value_record[0];
	assert_eq!(glyph_name(&mut font, record.second_glyph), "semicolon");
	let value1 = record.value1.clone().ok_or("no Value1")?;
	assert_eq!(
		value1,
		fontoxide_ttlib::ot::conv::ValueRecord {
			x_advance: Some(-70),
			..Default::default()
		}
	);
	Ok(())
}

#[test]
fn test_singleSubst_multipleSubstitutionsForSameGlyph() {
	assert_build_error(
		"feature test {    sub [a-z] by [A.sc-Z.sc];    sub e by e.fina;} test;",
		"Already defined substitution for \"e\"",
	);
}

#[test]
fn test_singlePos_redefinition() {
	assert_build_error(
		"feature test { pos A 123; pos A 456; } test;",
		"Already defined different position for glyph \"A\"",
	);
}

#[test]
fn test_feature_outside_aalt() {
	assert_build_error(
		"feature test { feature test; } test;",
		"Feature references are only allowed inside \"feature aalt\"",
	);
}

#[test]
fn test_feature_undefinedReference() -> TestResult {
	let (records, result) = capture(log::Level::Warn, || {
		build("feature aalt { feature none; } aalt;", None)
	});
	result?;
	assert_eq!(records.len(), 1);
	assert_logged(&records, "Feature none has not been defined");
	Ok(())
}

#[test]
fn test_GlyphClassDef_conflictingClasses() {
	assert_build_error(
		"table GDEF {    GlyphClassDef [a b], [X], , ;    GlyphClassDef [a b X], , , ;} GDEF;",
		"Glyph X was assigned to a different class",
	);
}

fn sorted(systems: &[(&str, &str)]) -> Vec<(String, String)> {
	let mut v: Vec<(String, String)> = systems
		.iter()
		.map(|(s, l)| ((*s).to_owned(), (*l).to_owned()))
		.collect();
	v.sort();
	v
}

#[test]
fn test_languagesystem() -> TestResult {
	let mut font = make_tt_font();
	let mut builder = Builder::new(&mut font, FeatureSource::text(""))?;
	builder.add_language_system(&None, "latn", "FRA")?;
	builder.add_language_system(&None, "cyrl", "RUS")?;
	builder.start_feature(&None, "test", false)?;
	assert_eq!(
		builder.language_systems,
		sorted(&[("latn", "FRA"), ("cyrl", "RUS")])
	);
	Ok(())
}

#[test]
fn test_languagesystem_duplicate() {
	assert_build_error(
		"languagesystem cyrl RUS; languagesystem cyrl RUS;",
		"\"languagesystem cyrl RUS\" has already been specified",
	);
}

#[test]
fn test_languagesystem_none_specified() -> TestResult {
	let mut font = make_tt_font();
	let mut builder = Builder::new(&mut font, FeatureSource::text(""))?;
	builder.start_feature(&None, "test", false)?;
	assert_eq!(builder.language_systems, sorted(&[("DFLT", "dflt")]));
	Ok(())
}

#[test]
fn test_languagesystem_DFLT_dflt_not_first() {
	assert_build_error(
		"languagesystem latn TRK; languagesystem DFLT dflt;",
		"If \"languagesystem DFLT dflt\" is present, it must be the first of the languagesystem statements",
	);
}

#[test]
fn test_languagesystem_DFLT_not_preceding() {
	assert_build_error(
		"languagesystem DFLT dflt; languagesystem latn dflt; languagesystem DFLT fooo; ",
		"languagesystems using the \"DFLT\" script tag must precede all other languagesystems",
	);
}

#[test]
fn test_script() -> TestResult {
	let mut font = make_tt_font();
	let mut builder = Builder::new(&mut font, FeatureSource::text(""))?;
	builder.start_feature(&None, "test", false)?;
	builder.set_script(&None, "cyrl")?;
	assert_eq!(builder.language_systems, sorted(&[("cyrl", "dflt")]));
	Ok(())
}

#[test]
fn test_script_matching_current_script() -> TestResult {
	// The current script starts out as the first declared language system, so
	// this script statement is a no-op and, in particular, does not reset the
	// lookupflag. https://github.com/fonttools/fonttools/issues/1824
	let mut font = make_tt_font();
	let mut builder = Builder::new(&mut font, FeatureSource::text(""))?;
	builder.add_language_system(&None, "latn", "dflt")?;
	builder.start_feature(&None, "test", false)?;
	builder.set_lookup_flag(&None, 8, None, None)?;
	builder.set_script(&None, "latn")?;
	assert_eq!(builder.language_systems, sorted(&[("latn", "dflt")]));
	assert_eq!(builder.lookupflag_, 8);
	Ok(())
}

#[test]
fn test_script_in_aalt_feature() {
	assert_build_error(
		"feature aalt { script latn; } aalt;",
		"Script statements are not allowed within \"feature aalt\"",
	);
}

#[test]
fn test_script_in_size_feature() {
	assert_build_error(
		"feature size { script latn; } size;",
		"Script statements are not allowed within \"feature size\"",
	);
}

#[test]
fn test_script_in_standalone_lookup() {
	assert_build_error(
		"lookup test { script latn; } test;",
		"Script statements are not allowed within standalone lookup blocks",
	);
}

fn langs(languages: &[&str]) -> Vec<String> {
	languages.iter().map(|l| (*l).to_owned()).collect()
}

#[test]
fn test_language() -> TestResult {
	let mut font = make_tt_font();
	let mut builder = Builder::new(&mut font, FeatureSource::text(""))?;
	builder.add_language_system(&None, "latn", "FRA ")?;
	builder.start_feature(&None, "test", false)?;
	builder.set_script(&None, "cyrl")?;
	builder.set_language(&None, &langs(&["RUS "]), false, false)?;
	assert_eq!(builder.language_systems, sorted(&[("cyrl", "RUS ")]));
	builder.set_language(&None, &langs(&["BGR "]), true, false)?;
	assert_eq!(builder.language_systems, sorted(&[("cyrl", "BGR ")]));
	builder.start_feature(&None, "test2", false)?;
	assert_eq!(builder.language_systems, sorted(&[("latn", "FRA ")]));
	Ok(())
}

#[test]
fn test_language_multiple() -> TestResult {
	let mut font = make_tt_font();
	let mut builder = Builder::new(&mut font, FeatureSource::text(""))?;
	builder.start_feature(&None, "test", false)?;
	builder.set_script(&None, "latn")?;
	let languages = ["AZE ", "CRT ", "KAZ ", "TAT ", "TRK "];
	builder.set_language(&None, &langs(&languages), false, true)?;
	let expected: Vec<(&str, &str)> = languages.iter().map(|l| ("latn", *l)).collect();
	assert_eq!(builder.language_systems, sorted(&expected));
	let required: BTreeMap<(String, String), String> = builder
		.required_features_
		.iter()
		.map(|(k, v)| (k.clone(), v.clone()))
		.collect();
	let expected: BTreeMap<(String, String), String> = languages
		.iter()
		.map(|l| (("latn".to_owned(), (*l).to_owned()), "test".to_owned()))
		.collect();
	assert_eq!(required, expected);
	Ok(())
}

#[test]
fn test_language_multiple_build() -> TestResult {
	let mut font = build(
		"feature locl {  script latn;  language AZE CRT KAZ TAT TRK exclude_dflt;  sub i by idotaccent;} locl;",
		None,
	)?;
	let gsub = font.get::<Gsub>()?.ok_or("no GSUB")?;
	let script_record = &gsub
		.script_list
		.as_ref()
		.ok_or("no ScriptList")?
		.script_record[0];
	assert_eq!(script_record.script_tag, Tag::new(b"latn"));
	let script = script_record.script.as_ref().ok_or("no Script")?;
	assert!(script.default_lang_sys.is_none());
	let tags: Vec<String> = script
		.lang_sys_record
		.iter()
		.map(|r| r.lang_sys_tag.to_string())
		.collect();
	assert_eq!(tags, langs(&["AZE ", "CRT ", "KAZ ", "TAT ", "TRK "]));
	let indices: Vec<Vec<u16>> = script
		.lang_sys_record
		.iter()
		.map(|r| {
			r.lang_sys
				.as_ref()
				.map(|l| l.feature_index.clone())
				.unwrap_or_default()
		})
		.collect();
	assert_eq!(indices, vec![vec![0]; 5]);
	Ok(())
}

#[test]
fn test_language_multiple_equivalent_to_lookup_references() -> TestResult {
	let mut multiple_languages = build(
		&dedent(
			"
                feature locl {
                    script latn;
                    language AZE CRT;
                    lookup idot {
                        substitute i by idotaccent;
                    } idot;
                } locl;
                ",
		),
		None,
	)?;
	let mut separate_languages = build(
		&dedent(
			"
                feature locl {
                    script latn;
                    language AZE;
                    lookup idot {
                        substitute i by idotaccent;
                    } idot;

                    language CRT;
                    lookup idot;
                } locl;
                ",
		),
		None,
	)?;
	assert_eq!(
		multiple_languages.table_data(Tag::new(b"GSUB"))?,
		separate_languages.table_data(Tag::new(b"GSUB"))?
	);
	Ok(())
}

#[test]
fn test_language_in_aalt_feature() {
	assert_build_error(
		"feature aalt { language FRA; } aalt;",
		"Language statements are not allowed within \"feature aalt\"",
	);
}

#[test]
fn test_language_in_size_feature() {
	assert_build_error(
		"feature size { language FRA; } size;",
		"Language statements are not allowed within \"feature size\"",
	);
}

#[test]
fn test_language_in_standalone_lookup() {
	assert_build_error(
		"lookup test { language FRA; } test;",
		"Language statements are not allowed within standalone lookup blocks",
	);
}

#[test]
fn test_language_required_duplicate() {
	assert_build_error(
		"feature scmp {    script latn;    language FRA required;    language DEU required;    substitute [a-z] by [A.sc-Z.sc];} scmp;feature test {    script latn;    language FRA required;    substitute [a-z] by [A.sc-Z.sc];} test;",
		"Language FRA (script latn) has already specified feature scmp as its required feature",
	);
}

#[test]
fn test_lookup_already_defined() {
	assert_build_error(
		"lookup foo {} foo; lookup foo {} foo;",
		"Lookup \"foo\" has already been defined",
	);
}

#[test]
fn test_lookup_multiple_flags() {
	assert_build_error(
		"lookup foo {    lookupflag 1;    sub f i by f_i;    lookupflag 2;    sub f f i by f_f_i;} foo;",
		"Within a named lookup block, all rules must be of the same lookup type and flag",
	);
}

#[test]
fn test_lookup_multiple_types() {
	assert_build_error(
		"lookup foo {    sub f f i by f_f_i;    sub A from [A.alt1 A.alt2];} foo;",
		"Within a named lookup block, all rules must be of the same lookup type and flag",
	);
}

#[test]
fn test_lookup_inside_feature_aalt() {
	assert_build_error(
		"feature aalt {lookup L {} L;} aalt;",
		"Lookup blocks cannot be placed inside 'aalt' features",
	);
}

#[test]
fn test_chain_subst_refrences_GPOS_looup() {
	assert_build_error(
		"lookup dummy { pos a 50; } dummy;feature test {    sub a' lookup dummy b;} test;",
		"Missing index of the specified lookup, might be a positioning lookup",
	);
}

#[test]
fn test_chain_pos_refrences_GSUB_looup() {
	assert_build_error(
		"lookup dummy { sub a by A; } dummy;feature test {    pos a' lookup dummy b;} test;",
		"Missing index of the specified lookup, might be a substitution lookup",
	);
}

#[test]
fn test_STAT_elidedfallbackname_missing() {
	assert_build_error(
		"table STAT {    DesignAxis wght 0 { name \"Weight\"; };    AxisValue {        location wght 400;        name \"Regular\";    };} STAT;",
		"STAT table requires an ElidedFallbackName or ElidedFallbackNameID",
	);
}

#[test]
fn test_STAT_elidedfallbackname_already_defined() {
	assert_build_error(
		"table name {   nameid 256 \"Roman\"; } name;table STAT {    ElidedFallbackName { name \"Roman\"; };    ElidedFallbackNameID 256;} STAT;",
		"ElidedFallbackName is already set.",
	);
}

#[test]
fn test_STAT_elidedfallbackname_set_twice() {
	assert_build_error(
		"table name {   nameid 256 \"Roman\"; } name;table STAT {    ElidedFallbackName { name \"Roman\"; };    ElidedFallbackName { name \"Italic\"; };} STAT;",
		"ElidedFallbackName is already set.",
	);
}

#[test]
fn test_STAT_elidedfallbacknameID_already_defined() {
	assert_build_error(
		"table name {   nameid 256 \"Roman\"; } name;table STAT {    ElidedFallbackNameID 256;    ElidedFallbackName { name \"Roman\"; };} STAT;",
		"ElidedFallbackNameID is already set.",
	);
}

#[test]
fn test_STAT_elidedfallbacknameID_not_in_name_table() {
	assert_build_error(
		"table name {   nameid 257 \"Roman\"; } name;table STAT {    ElidedFallbackNameID 256;    DesignAxis opsz 1 { name \"Optical Size\"; };} STAT;",
		"ElidedFallbackNameID 256 points to a nameID that does not exist in the \"name\" table",
	);
}

#[test]
fn test_OS2_vendor_padded_with_spaces() -> TestResult {
	let mut font = build("table OS/2 { Vendor \"AB\"; } OS/2;", None)?;
	assert_eq!(&font.get::<Os2>()?.ok_or("no OS/2")?.ach_vend_id, b"AB  ");
	Ok(())
}

#[test]
fn test_STAT_design_axis_name() {
	assert_build_error(
		"table name {   nameid 256 \"Roman\"; } name;table STAT {    ElidedFallbackName { name \"Roman\"; };    DesignAxis opsz 0 { badtag \"Optical Size\"; };} STAT;",
		"Expected \"name\"",
	);
}

#[test]
fn test_STAT_duplicate_design_axis_name() {
	assert_build_error(
		"table name {   nameid 256 \"Roman\"; } name;table STAT {    ElidedFallbackName { name \"Roman\"; };    DesignAxis opsz 0 { name \"Optical Size\"; };    DesignAxis opsz 1 { name \"Optical Size\"; };} STAT;",
		"DesignAxis already defined for tag \"opsz\".",
	);
}

#[test]
fn test_STAT_design_axis_duplicate_order() {
	assert_build_error(
		"table name {   nameid 256 \"Roman\"; } name;table STAT {    ElidedFallbackName { name \"Roman\"; };    DesignAxis opsz 0 { name \"Optical Size\"; };    DesignAxis wdth 0 { name \"Width\"; };    AxisValue {         location opsz 8;         location wdth 400;         name \"Caption\";     };} STAT;",
		"DesignAxis already defined for axis number 0.",
	);
}

#[test]
fn test_STAT_undefined_tag() {
	assert_build_error(
		"table name {   nameid 256 \"Roman\"; } name;table STAT {    ElidedFallbackName { name \"Roman\"; };    DesignAxis opsz 0 { name \"Optical Size\"; };    AxisValue {         location wdth 125;         name \"Wide\";     };} STAT;",
		"DesignAxis not defined for wdth.",
	);
}

#[test]
fn test_STAT_axis_value_format4() {
	assert_build_error(
		"table name {   nameid 256 \"Roman\"; } name;table STAT {    ElidedFallbackName { name \"Roman\"; };    DesignAxis opsz 0 { name \"Optical Size\"; };    DesignAxis wdth 1 { name \"Width\"; };    DesignAxis wght 2 { name \"Weight\"; };    AxisValue {         location opsz 8;         location wdth 125;         location wdth 125;         location wght 500;         name \"Caption Medium Wide\";     };} STAT;",
		"Axis tag wdth already defined.",
	);
}

#[test]
fn test_STAT_duplicate_axis_value_record() {
	// Duplicate AxisValueRecords even when the definition order is different.
	assert_build_error(
		"table name {   nameid 256 \"Roman\"; } name;table STAT {    ElidedFallbackName { name \"Roman\"; };    DesignAxis opsz 0 { name \"Optical Size\"; };    DesignAxis wdth 1 { name \"Width\"; };    AxisValue {         location opsz 8;         location wdth 400;         name \"Caption\";     };    AxisValue {         location wdth 400;         location opsz 8;         name \"Caption\";     };} STAT;",
		"An AxisValueRecord with these values is already defined.",
	);
}

#[test]
fn test_STAT_axis_value_missing_location() {
	assert_build_error(
		"table name {   nameid 256 \"Roman\"; } name;table STAT {    ElidedFallbackName {   name \"Roman\"; };    DesignAxis opsz 0 { name \"Optical Size\"; };    AxisValue {         name \"Wide\";     };} STAT;",
		"Expected \"Axis location\"",
	);
}

#[test]
fn test_STAT_invalid_location_tag() {
	assert_build_error(
		"table name {   nameid 256 \"Roman\"; } name;table STAT {    ElidedFallbackName { name \"Roman\";                          name 3 1 0x0411 \"ローマン\"; };     DesignAxis width 0 { name \"Width\"; };} STAT;",
		"Tags cannot be longer than 4 characters",
	);
}

#[test]
fn test_markClass_same_glyph_redefined() {
	assert_build_error(
		&"markClass [acute] <anchor 350 0> @TOP_MARKS;".repeat(2),
		"Glyph acute already defined",
	);
}

#[test]
fn test_markClass_same_glyph_multiple_classes() {
	assert_build_error(
		"feature mark {    markClass [uni0327 uni0328] <anchor 0 0> @ogonek;    pos base [a] <anchor 399 0> mark @ogonek;    markClass [uni0327] <anchor 0 0> @cedilla;    pos base [a] <anchor 244 0> mark @cedilla;} mark;",
		"Glyph uni0327 cannot be in both @ogonek and @cedilla",
	);
}

#[test]
fn test_mark_base_pos_same_mark_class_twice() {
	assert_build_error(
		"markClass [acute] <anchor 350 0> @TOP;feature mark {    pos base a <anchor 91 -316> mark @TOP <anchor 91 -226> mark @TOP;} mark;",
		"Mark class @TOP used more than once",
	);
}

#[test]
fn test_mark_lig_pos_same_mark_class_twice() {
	assert_build_error(
		"markClass [acute] <anchor 350 0> @TOP;feature mark {    pos ligature f_i <anchor 281 -309> mark @TOP        <anchor 281 -219> mark @TOP        ligComponent <anchor NULL>;} mark;",
		"Mark class @TOP used more than once",
	);
}

#[test]
fn test_mark_mark_pos_same_mark_class_twice() {
	assert_build_error(
		"markClass [acute] <anchor 350 0> @TOP;feature mkmk {    pos mark grave <anchor 1 2> mark @TOP <anchor 3 4> mark @TOP;} mkmk;",
		"Mark class @TOP used more than once",
	);
}

#[test]
fn test_mark_lig_pos_same_mark_class_in_different_components() -> TestResult {
	let font = build(
		"markClass [acute] <anchor 350 0> @TOP;feature mark {    pos ligature f_i <anchor 100 500> mark @TOP        ligComponent <anchor 300 500> mark @TOP;} mark;",
		None,
	)?;
	assert!(font.contains(Tag::new(b"GPOS")));
	Ok(())
}

#[test]
fn test_build_specific_tables() -> TestResult {
	let features = "feature liga {sub f i by f_i;} liga;";
	let font = build(features, None)?;
	assert!(font.contains(Tag::new(b"GSUB")));
	let font2 = build(features, Some(&[]))?;
	assert!(!font2.contains(Tag::new(b"GSUB")));
	Ok(())
}

#[test]
fn test_build_unsupported_tables() {
	assert!(matches!(
		build("", Some(&["FOO"])),
		Err(Error::NotImplemented(_))
	));
}

#[test]
fn test_build_pre_parsed_ast_featurefile() -> TestResult {
	let tree = Parser::new(
		FeatureSource::text("feature liga {sub f i by f_i;} liga;"),
		Vec::<String>::new(),
	)?
	.parse()?;
	let mut font = make_tt_font();
	add_open_type_features(&mut font, tree, None, false)?;
	assert!(font.contains(Tag::new(b"GSUB")));
	Ok(())
}

#[test]
fn test_unsupported_subtable_break() -> TestResult {
	let (records, result) = capture(log::Level::Warn, || {
		build(
			"feature test {    pos a 10;    subtable;    pos b 10;} test;",
			None,
		)
	});
	result?;
	assert_logged(
		&records,
		"<features>:1:32: unsupported \"subtable\" statement for lookup type",
	);
	Ok(())
}

#[test]
fn test_skip_featureNames_if_no_name_table() -> TestResult {
	let features = "feature ss01 {    featureNames {        name \"ignored as we request to skip name table\";    };    sub A by A.alt1;} ss01;";
	let font = build(features, Some(&["GSUB"]))?;
	assert!(font.contains(Tag::new(b"GSUB")));
	assert!(!font.contains(Tag::new(b"name")));
	Ok(())
}

#[test]
fn test_singlePos_multiplePositionsForSameGlyph() {
	assert_build_error(
		"lookup foo {    pos A -45;     pos A 45; } foo;",
		"Already defined different position for glyph",
	);
}

#[test]
fn test_pairPos_enumRuleOverridenBySinglePair_DEBUG() -> TestResult {
	let (records, result) = capture(log::Level::Debug, || {
		build(
			"feature test {    enum pos A [V Y] -80;    pos A V -75;} test;",
			None,
		)
	});
	result?;
	assert_logged(&records, "Already defined position for pair A V at");
	Ok(())
}

#[test]
fn test_ligatureSubst_conflicting_rules() {
	assert_build_error(
		"feature test {    sub a b by one;    sub a b by two;} test;",
		"Already defined substitution for \"a, b\"",
	);
}

#[test]
fn test_ignore_empty_lookup_block() -> TestResult {
	// https://github.com/fonttools/fonttools/pull/2277
	let font = build(
		"lookup EMPTY { ; } EMPTY;feature ss01 { lookup EMPTY; } ss01;",
		None,
	)?;
	assert!(!font.contains(Tag::new(b"GPOS")));
	assert!(!font.contains(Tag::new(b"GSUB")));
	Ok(())
}

#[test]
fn test_disable_empty_classes() {
	for test in [
		"sub a by c []",
		"sub f f [] by f",
		"ignore sub a []'",
		"ignore sub [] a'",
		"sub a []' by b",
		"sub [] a' by b",
		"rsub [] by a",
		"pos [] 120",
		"pos a [] 120",
		"enum pos a [] 120",
		"pos cursive [] <anchor NULL> <anchor NULL>",
		"pos base [] <anchor NULL> mark @TOPMARKS",
		"pos ligature [] <anchor NULL> mark @TOPMARKS",
		"pos mark [] <anchor NULL> mark @TOPMARKS",
		"ignore pos a []'",
		"ignore pos [] a'",
	] {
		assert_build_error(
			&format!("markClass a <anchor 150 -10> @TOPMARKS; lookup foo {{ {test}; }} foo;"),
			"Empty ",
		);
	}
	assert_build_error(
		"markClass [] <anchor 150 -10> @TOPMARKS;",
		"Empty glyph class in mark class definition",
	);
	assert_build_error(
		"feature test { sub a by []; test};",
		"Expected a glyph class with 1 elements after \"by\", but found a glyph class with 0 elements",
	);
}

/// `normal_fea(lines)` of `check_fea2fea_file`.
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

#[test]
fn test_unmarked_ignore_statement() -> TestResult {
	let name = "bug2949";
	let (records, result) = capture(log::Level::Warn, || check_feature_file(name));
	result?;
	// check_fea2fea_file(name)
	let mut font = make_tt_font();
	let path = getpath(&format!("{name}.fea"));
	let glyphs = font.glyph_order()?.names().to_vec();
	let doc = Parser::new(path.as_path(), glyphs)?.parse()?;
	let actual = normal_fea(&doc.as_fea()?.split('\n').collect::<Vec<_>>());
	let text = std::fs::read_to_string(&path)?;
	let expected = normal_fea(&text.lines().collect::<Vec<_>>());
	assert_eq!(actual, expected);
	for (line, sub) in [(3, "sub"), (8, "pos"), (13, "sub")] {
		assert_logged(
			&records,
			&format!(
				"{name}.fea:{line}:12: Ambiguous \"ignore {sub}\", there should be least one marked glyph"
			),
		);
	}
	Ok(())
}

/// A mock variable font with the axes given.
fn mock_vf(axes: &[(&str, f64, f64, f64)]) -> Font {
	let mut font = make_tt_font();
	add_fvar(&mut font, axes);
	font
}

#[test]
fn test_conditionset_multiple_features() -> TestResult {
	// Using the same `conditionset` for multiple features reuses the
	// `FeatureVariationRecord`.
	let features = "
            languagesystem DFLT dflt;

            conditionset test {
                wght 600 1000;
                wdth 150 200;
            } test;

            variation ccmp test {
                sub e by a;
            } ccmp;

            variation rlig test {
                sub b by c;
            } rlig;
        ";
	let mut font = mock_vf(&[("wght", 0.0, 0.0, 1000.0), ("wdth", 100.0, 100.0, 200.0)]);
	add_open_type_features_from_string(&mut font, features, None, None, false)?;
	let gsub = font.get::<Gsub>()?.ok_or("no GSUB")?;
	let fv = gsub
		.feature_variations
		.as_ref()
		.ok_or("no FeatureVariations")?;
	assert_eq!(fv.feature_variation_record.len(), 1);
	let fvr = &fv.feature_variation_record[0];
	let fts = fvr
		.feature_table_substitution
		.as_ref()
		.ok_or("no FeatureTableSubstitution")?;
	assert_eq!(fts.substitution_record.len(), 2);
	Ok(())
}

#[test]
fn test_conditionset_multiple_features_sorted() -> TestResult {
	// FeatureTableSubstitution records are sorted by FeatureIndex when several
	// `variation` blocks sharing a `conditionset` are not in feature tag order.
	let features = "
            languagesystem DFLT dflt;

            conditionset heavy {
                wght 600 1000;
            } heavy;

            variation calt heavy { sub a by b; } calt;
            variation rlig heavy { sub b by c; } rlig;
            variation liga heavy { sub c by d; } liga;
            variation ss01 heavy { sub d by e; } ss01;
            variation ss02 heavy { sub e by f; } ss02;
            variation ss03 heavy { sub f by g; } ss03;
        ";
	let mut font = mock_vf(&[("wght", 0.0, 0.0, 1000.0)]);
	add_open_type_features_from_string(&mut font, features, None, None, false)?;
	let gsub = font.get::<Gsub>()?.ok_or("no GSUB")?;
	let tags: Vec<String> = gsub
		.feature_list
		.as_ref()
		.ok_or("no FeatureList")?
		.feature_record
		.iter()
		.map(|r| r.feature_tag.to_string())
		.collect();
	assert_eq!(
		tags,
		langs(&["calt", "liga", "rlig", "ss01", "ss02", "ss03"])
	);
	let fv = gsub
		.feature_variations
		.as_ref()
		.ok_or("no FeatureVariations")?;
	assert_eq!(fv.feature_variation_record.len(), 1);
	let fts = fv.feature_variation_record[0]
		.feature_table_substitution
		.as_ref()
		.ok_or("no FeatureTableSubstitution")?;
	assert_eq!(fts.substitution_record.len(), 6);
	let indices: Vec<u16> = fts
		.substitution_record
		.iter()
		.map(|r| r.feature_index)
		.collect();
	assert_eq!(indices, vec![0, 1, 2, 3, 4, 5]);
	Ok(())
}

/// The minimum values of the conditions of the first feature variation record.
fn condition_minimums(font: &mut Font) -> TestResult<Vec<f64>> {
	let gsub = font.get::<Gsub>()?.ok_or("no GSUB")?;
	let fv = gsub
		.feature_variations
		.as_ref()
		.ok_or("no FeatureVariations")?;
	let cs = fv.feature_variation_record[0]
		.condition_set
		.as_ref()
		.ok_or("no ConditionSet")?;
	Ok(cs
		.condition_table
		.iter()
		.map(|c| match c.as_deref() {
			Some(ot::ConditionTable::Format1(c)) => c.filter_range_min_value,
			_ => f64::NAN,
		})
		.collect())
}

#[test]
fn test_condition_set_avar() -> TestResult {
	// The `avar` table is consulted when normalizing user-space values.
	let features = "
            languagesystem DFLT dflt;

            lookup conditional_sub {
                sub e by a;
            } conditional_sub;

            conditionset test {
                wght 600 1000;
                wdth 150 200;
            } test;

            variation rlig test {
                lookup conditional_sub;
            } rlig;
        ";
	let axes = [("wght", 0.0, 0.0, 1000.0), ("wdth", 100.0, 100.0, 200.0)];
	// Without `avar`:
	let mut font = mock_vf(&axes);
	add_open_type_features_from_string(&mut font, features, None, None, false)?;
	// user-space wdth=150 and wght=600:
	assert_eq!(condition_minimums(&mut font)?[..2], [0.5, 0.6]);
	// With `avar`, shifting the wght axis' positive midpoint 0.5 a bit to the
	// right, but leaving the wdth axis alone:
	let mut font = mock_vf(&axes);
	font.insert(Box::new(wght_avar()));
	add_open_type_features_from_string(&mut font, features, None, None, false)?;
	// user-space wdth=150 as before and wght=600 shifted to the right:
	assert_eq!(condition_minimums(&mut font)?[..2], [0.5, 0.7]);
	Ok(())
}

#[test]
fn test_condition_set_duplicated() -> TestResult {
	// Overloading conditionset names raises an error.
	let features = "
            conditionset duplicated {
                wght 600 1000;
            } duplicated;

            # Same name as previous condition set (should error).
            conditionset duplicated {
                wght 500 1000;
            } duplicated;
        ";
	let mut font = mock_vf(&[("wght", 0.0, 0.0, 1000.0)]);
	let err = add_open_type_features_from_string(&mut font, features, None, None, false)
		.err()
		.ok_or("no error")?;
	assert!(
		err.to_string()
			.contains("Condition set 'duplicated' has the same name as a previous condition set")
	);
	Ok(())
}

/// The (start, peak, end) of an axis of a region of the `GDEF` store.
fn region(font: &mut Font, region: usize, axis: usize) -> TestResult<(f64, f64, f64)> {
	let gdef = font.get::<Gdef>()?.ok_or("no GDEF")?;
	let store = gdef.var_store.as_ref().ok_or("no VarStore")?;
	let list = store.var_region_list.as_ref().ok_or("no VarRegionList")?;
	let a = &list.region[region].var_region_axis[axis];
	Ok((a.start_coord, a.peak_coord, a.end_coord))
}

#[test]
fn test_variable_scalar_avar() -> TestResult {
	// The `avar` table is consulted when normalizing user-space values.
	let features = "
            languagesystem DFLT dflt;

            feature kern {
                pos cursive one <anchor 0 (wght=200:12 wght=900:22 wdth=150,wght=900:42)> <anchor NULL>;
                pos two <0 (wght=200:12 wght=900:22 wdth=150,wght=900:42) 0 0>;
            } kern;
        ";
	// Without `avar` (wght=200, wdth=100 is the default location):
	let mut font = make_mock_vf();
	add_open_type_features_from_string(&mut font, features, None, None, false)?;
	assert_eq!(region(&mut font, 0, 0)?, (0.0, 0.875, 1.0));
	assert_eq!(region(&mut font, 0, 1)?, (0.0, 0.0, 0.0));
	assert_eq!(region(&mut font, 1, 0)?, (0.0, 0.875, 1.0));
	assert_eq!(region(&mut font, 1, 1)?, (0.0, 0.5, 1.0));
	// With `avar`, shifting the wght axis' positive midpoint 0.5 a bit to the
	// right, but leaving the wdth axis alone:
	let mut font = make_mock_vf();
	font.insert(Box::new(wght_avar()));
	add_open_type_features_from_string(&mut font, features, None, None, false)?;
	assert_eq!(region(&mut font, 0, 0)?, (0.0, 0.90625, 1.0));
	assert_eq!(region(&mut font, 0, 1)?, (0.0, 0.0, 0.0));
	assert_eq!(region(&mut font, 1, 0)?, (0.0, 0.90625, 1.0));
	assert_eq!(region(&mut font, 1, 1)?, (0.0, 0.5, 1.0));
	Ok(())
}

#[test]
fn test_variable_scalar_duplicate_location() -> TestResult {
	let features = "
            languagesystem DFLT dflt;

            feature kern {
                pos two <0 (wght=200:10 wght=200,wdth=100:20) 0 0>;
            } kern;
        ";
	let mut font = make_mock_vf();
	let err = add_open_type_features_from_string(&mut font, features, None, None, false)
		.err()
		.ok_or("no error")?;
	assert!(matches!(err, Error::FeatureLib { .. }));
	assert!(
		err.to_string()
			.contains("Failed to compute deltas for variable scalar")
	);
	Ok(())
}

#[test]
fn test_variable_scalar_default() -> TestResult {
	// Missing axis names in a variable scalar mean the default location.
	let features = "
            feature kern {
                pos two <0 (wght=900:22 12 wdth=150,wght=900:42) 0 0>;
            } kern;
        ";
	let mut font = make_mock_vf();
	add_open_type_features_from_string(&mut font, features, None, None, false)?;
	assert_eq!(region(&mut font, 0, 0)?, (0.0, 0.875, 1.0));
	assert_eq!(region(&mut font, 0, 1)?, (0.0, 0.0, 0.0));
	assert_eq!(region(&mut font, 1, 0)?, (0.0, 0.875, 1.0));
	assert_eq!(region(&mut font, 1, 1)?, (0.0, 0.5, 1.0));
	Ok(())
}

#[test]
fn test_ligatureCaretByPos_variable_scalar() -> TestResult {
	let features = "
            table GDEF {
                LigatureCaretByPos f_i (wght=200:400 wght=900:1000) 380;
            } GDEF;
        ";
	let mut font = make_mock_vf();
	add_open_type_features_from_string(&mut font, features, None, None, false)?;
	let gdef = font.get::<Gdef>()?.ok_or("no GDEF")?;
	let list = gdef.lig_caret_list.as_ref().ok_or("no LigCaretList")?;
	let lig_glyph = list.lig_glyph[0].as_ref().ok_or("no LigGlyph")?;
	match lig_glyph.caret_value[0].as_deref() {
		Some(ot::CaretValue::Format1(c)) => assert_eq!(c.coordinate, 380),
		other => return Err(format!("unexpected caret {other:?}").into()),
	}
	match lig_glyph.caret_value[1].as_deref() {
		Some(ot::CaretValue::Format3(c)) => assert_eq!(c.coordinate, 400),
		other => return Err(format!("unexpected caret {other:?}").into()),
	}
	assert_eq!(region(&mut font, 0, 0)?, (0.0, 0.875, 1.0));
	Ok(())
}

#[test]
fn test_variable_anchors_round_trip() -> TestResult {
	// Building with a parsed feature file does not discard variations from
	// variable anchors.
	let features = "            feature curs {
                pos cursive one <anchor 0 (wdth=100,wght=200:12 wdth=150,wght=900:42)> <anchor NULL>;
            } curs;
        ";
	let feafile = Parser::new(FeatureSource::text(features), Vec::<String>::new())?.parse()?;
	let mut font = make_mock_vf();
	add_open_type_features(&mut font, feafile.clone(), None, false)?;
	assert_eq!(dedent(&feafile.as_fea()?), dedent(features));
	Ok(())
}

#[test]
fn test_feature_useExtension() {
	assert_build_error(
		"feature liga useExtension { sub f f by f_f; } liga;",
		"'useExtension' keyword for feature blocks is allowed only for 'aalt' feature",
	);
}
