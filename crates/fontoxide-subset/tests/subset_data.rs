//! The tests of fontTools' `Tests/subset/subset_test.py` that work on its data
//! files.

mod common;

use common::*;
use fontoxide_subset::{Item, Options, Subsetter};
use fontoxide_ttlib::tables::otl::Varc;
use fontoxide_ttlib::tables::{Cbdt, Colr, Glyf, Head, Hmtx, Loca, Maxp, Name, Os2};
use fontoxide_ttlib::{DefaultTable, Flavor, Tag, WriteOptions};

/// A test that compiles a TTX file of `Tests/subset/data`, subsets it with the
/// arguments, and compares tables with an expected TTX file.
fn check(ttx: &str, suffix: &str, args: &[&str], expected: &str, tables: &[&str]) -> TestResult {
	let mut temp = TempDir::new()?;
	let fontpath = temp.compile_font(&getpath(ttx), suffix)?;
	let subsetpath = temp.path(suffix);
	let output = format!("--output-file={subsetpath}");
	let mut argv = vec![fontpath.as_str()];
	argv.extend(args);
	argv.push(&output);
	subset_main(&argv)?;
	expect_ttx(&mut open(&subsetpath)?, &getpath(expected), tables)
}

/// Subsets a compiled TTX file of `Tests/subset/data`.
fn subset_ttx(ttx: &str, suffix: &str, args: &[&str]) -> TestResult<fontoxide_ttlib::Font> {
	let mut temp = TempDir::new()?;
	let fontpath = temp.compile_font(&getpath(ttx), suffix)?;
	let subsetpath = temp.path(suffix);
	let output = format!("--output-file={subsetpath}");
	let mut argv = vec![fontpath.as_str()];
	argv.extend(args);
	argv.push(&output);
	subset_main(&argv)?;
	open(&subsetpath)
}

#[test]
fn test_layout_scripts() -> TestResult {
	check(
		"layout_scripts.ttx",
		".otf",
		&[
			"--glyphs=*",
			"--layout-features=*",
			"--layout-scripts=latn,arab.URD,arab.dflt",
		],
		"expect_layout_scripts.ttx",
		&["GPOS", "GSUB"],
	)
}

#[test]
fn test_no_notdef_outline_otf() -> TestResult {
	check(
		"TestOTF-Regular.ttx",
		".otf",
		&["--no-notdef-outline", "--gids=0"],
		"expect_no_notdef_outline_otf.ttx",
		&["CFF "],
	)
}

#[test]
fn test_no_notdef_outline_cid() -> TestResult {
	check(
		"TestCID-Regular.ttx",
		".otf",
		&["--no-notdef-outline", "--gids=0"],
		"expect_no_notdef_outline_cid.ttx",
		&["CFF "],
	)
}

#[test]
fn test_no_notdef_outline_ttf() -> TestResult {
	check(
		"TestTTF-Regular.ttx",
		".ttf",
		&["--no-notdef-outline", "--gids=0"],
		"expect_no_notdef_outline_ttf.ttx",
		&["glyf", "hmtx"],
	)
}

#[test]
fn test_subset_ankr() -> TestResult {
	check(
		"TestANKR.ttx",
		".ttf",
		&["--glyphs=one"],
		"expect_ankr.ttx",
		&["ankr"],
	)
}

#[test]
fn test_subset_ankr_remove() -> TestResult {
	let font = subset_ttx("TestANKR.ttx", ".ttf", &["--glyphs=two"])?;
	assert!(!font.contains(Tag::new(b"ankr")));
	Ok(())
}

#[test]
fn test_subset_bsln_format_0() -> TestResult {
	check(
		"TestBSLN-0.ttx",
		".ttf",
		&["--glyphs=one"],
		"expect_bsln_0.ttx",
		&["bsln"],
	)
}

#[test]
fn test_subset_bsln_format_0_from_format_1() -> TestResult {
	check(
		"TestBSLN-1.ttx",
		".ttf",
		&["--unicodes=U+0030-0031"],
		"expect_bsln_0.ttx",
		&["bsln"],
	)
}

#[test]
fn test_subset_bsln_format_1() -> TestResult {
	check(
		"TestBSLN-1.ttx",
		".ttf",
		&["--unicodes=U+0030-0031,U+2EA2"],
		"expect_bsln_1.ttx",
		&["bsln"],
	)
}

#[test]
fn test_subset_bsln_format_2() -> TestResult {
	check(
		"TestBSLN-2.ttx",
		".ttf",
		&["--glyphs=one"],
		"expect_bsln_2.ttx",
		&["bsln"],
	)
}

#[test]
fn test_subset_bsln_format_2_from_format_3() -> TestResult {
	check(
		"TestBSLN-3.ttx",
		".ttf",
		&["--unicodes=U+0030"],
		"expect_bsln_2.ttx",
		&["bsln"],
	)
}

#[test]
fn test_subset_bsln_format_3() -> TestResult {
	check(
		"TestBSLN-3.ttx",
		".ttf",
		&["--unicodes=U+0030-0031,U+2EA2"],
		"expect_bsln_3.ttx",
		&["bsln"],
	)
}

#[test]
fn test_subset_base() -> TestResult {
	check(
		"TestBASE.ttx",
		".ttf",
		&["--gids=2,3"],
		"expect_BASE.ttx",
		&["BASE"],
	)
}

#[test]
fn test_subset_clr() -> TestResult {
	check(
		"TestCLR-Regular.ttx",
		".ttf",
		&["--glyphs=smileface"],
		"expect_keep_colr.ttx",
		&["GlyphOrder", "hmtx", "glyf", "COLR", "CPAL"],
	)
}

#[test]
fn test_subset_gvar() -> TestResult {
	check(
		"TestGVAR.ttx",
		".ttf",
		&["--unicodes=U+002B,U+2212"],
		"expect_keep_gvar.ttx",
		&["GlyphOrder", "avar", "fvar", "gvar", "name"],
	)
}

#[test]
fn test_subset_gvar_notdef_outline() -> TestResult {
	check(
		"TestGVAR.ttx",
		".ttf",
		&["--unicodes=U+0030", "--notdef_outline"],
		"expect_keep_gvar_notdef_outline.ttx",
		&["GlyphOrder", "avar", "fvar", "gvar", "name"],
	)
}

#[test]
fn test_subset_lcar_remove() -> TestResult {
	let font = subset_ttx("TestLCAR-0.ttx", ".ttf", &["--glyphs=one"])?;
	assert!(!font.contains(Tag::new(b"lcar")));
	Ok(())
}

#[test]
fn test_subset_lcar_format_0() -> TestResult {
	check(
		"TestLCAR-0.ttx",
		".ttf",
		&["--unicodes=U+FB01"],
		"expect_lcar_0.ttx",
		&["lcar"],
	)
}

#[test]
fn test_subset_lcar_format_1() -> TestResult {
	check(
		"TestLCAR-1.ttx",
		".ttf",
		&["--unicodes=U+FB01"],
		"expect_lcar_1.ttx",
		&["lcar"],
	)
}

#[test]
fn test_subset_math() -> TestResult {
	check(
		"TestMATH-Regular.ttx",
		".ttf",
		&["--unicodes=U+0041,U+0028,U+0302,U+1D400,U+1D435"],
		"expect_keep_math.ttx",
		&["GlyphOrder", "CFF ", "MATH", "hmtx"],
	)
}

#[test]
fn test_subset_math_partial() -> TestResult {
	check(
		"test_math_partial.ttx",
		".ttf",
		&["--text=A"],
		"expect_math_partial.ttx",
		&["MATH"],
	)
}

#[test]
fn test_subset_math_closure_drops_added_glyph_constructions() -> TestResult {
	let mut temp = TempDir::new()?;
	let fontpath = temp.compile_font(&getpath("test_math_closure.ttx"), ".ttf")?;
	let mut font = open(&fontpath)?;
	let mut subsetter = Subsetter::new(Options::default());
	subsetter.populate(&[], &[], &[], "A");
	subsetter.subset(&mut font)?;

	let names = glyph_names(&mut font)?;
	// "A.size1" survives: it is a legitimate variant of the requested "A".
	assert!(names.iter().any(|n| n == "A.size1"));
	// "A.size2" was never closed over, so it is not in the subset.
	assert!(!names.iter().any(|n| n == "A.size2"));
	let a = font.glyph_id("A")?;
	let math = font.require::<fontoxide_ttlib::tables::Math>()?;
	let variants = math.math_variants.as_ref().ok_or("no MathVariants")?;
	// Only "A" keeps a construction.
	assert_eq!(
		variants
			.vert_glyph_coverage
			.as_ref()
			.ok_or("no coverage")?
			.glyphs,
		[a]
	);
	assert_eq!(variants.vert_glyph_count, Some(1));
	// No surviving construction references a glyph outside the subset.
	let count = names.len() as u32;
	for construction in variants.vert_glyph_construction.iter().flatten() {
		for record in &construction.math_glyph_variant_record {
			assert!(record.variant_glyph < count);
		}
	}
	Ok(())
}

#[test]
fn test_subset_opbd_remove() -> TestResult {
	let font = subset_ttx("TestOPBD-0.ttx", ".ttf", &["--glyphs=one"])?;
	assert!(!font.contains(Tag::new(b"opbd")));
	Ok(())
}

#[test]
fn test_subset_opbd_format_0() -> TestResult {
	check(
		"TestOPBD-0.ttx",
		".ttf",
		&["--glyphs=A"],
		"expect_opbd_0.ttx",
		&["opbd"],
	)
}

#[test]
fn test_subset_opbd_format_1() -> TestResult {
	check(
		"TestOPBD-1.ttx",
		".ttf",
		&["--glyphs=A"],
		"expect_opbd_1.ttx",
		&["opbd"],
	)
}

#[test]
fn test_subset_prop_remove_default_zero() -> TestResult {
	let font = subset_ttx("TestPROP.ttx", ".ttf", &["--unicodes=U+0041"])?;
	assert!(!font.contains(Tag::new(b"prop")));
	Ok(())
}

#[test]
fn test_subset_prop_0() -> TestResult {
	check(
		"TestPROP.ttx",
		".ttf",
		&["--unicodes=U+0030-0032", "--no-notdef-glyph"],
		"expect_prop_0.ttx",
		&["prop"],
	)
}

#[test]
fn test_subset_prop_1() -> TestResult {
	check(
		"TestPROP.ttx",
		".ttf",
		&["--unicodes=U+0030-0032", "--notdef-outline"],
		"expect_prop_1.ttx",
		&["prop"],
	)
}

/// The glyph data of a strike is keyed by name in fontTools.
fn strike_has(font: &mut fontoxide_ttlib::Font, name: &str) -> TestResult<bool> {
	let gid = glyph_names(font)?.iter().position(|n| n == name);
	let Some(gid) = gid else {
		return Ok(false);
	};
	let cbdt = font.require::<Cbdt>()?;
	let strike = cbdt.strike_data[0].as_ref().ok_or("no strike")?;
	Ok(strike.glyph_ids().contains(&(gid as u32)))
}

#[test]
fn test_google_color() -> TestResult {
	let mut font = subset_ttx("google_color.ttx", ".ttf", &["--gids=0,1"])?;
	assert!(font.contains(Tag::new(b"CBDT")));
	assert!(font.contains(Tag::new(b"CBLC")));
	assert!(strike_has(&mut font, "x")?);
	assert!(!strike_has(&mut font, "y")?);
	Ok(())
}

#[test]
fn test_google_color_all() -> TestResult {
	let mut font = subset_ttx("google_color.ttx", ".ttf", &["--unicodes=*"])?;
	assert!(strike_has(&mut font, "x")?);
	assert!(strike_has(&mut font, "y")?);
	Ok(())
}

#[test]
fn test_sbix() -> TestResult {
	check(
		"sbix.ttx",
		".ttf",
		&["--gids=0,1"],
		"expect_sbix.ttx",
		&["sbix"],
	)
}

#[test]
fn test_var_composite() -> TestResult {
	let mut temp = TempDir::new()?;
	let fontpath = tests_path("ttLib/data/varc-ac00-ac01.ttf");
	assert_eq!(open(&fontpath)?.glyph_order()?.len(), 11);
	let subsetpath = temp.path(".ttf");
	let output = format!("--output-file={subsetpath}");
	for (unicodes, glyphs, items) in [("--unicodes=ac00", 6, 2), ("--unicodes=ac01", 8, 5)] {
		subset_main(&[&fontpath, unicodes, &output])?;
		let mut font = open(&subsetpath)?;
		assert_eq!(font.glyph_order()?.len(), glyphs);
		let varc = font.require::<Varc>()?;
		assert_eq!(
			varc.axis_indices_list.as_ref().ok_or("no list")?.item.len(),
			2
		);
		let store = varc.multi_var_store.as_ref().ok_or("no store")?;
		assert_eq!(store.multi_var_data.len(), 1);
		assert_eq!(
			store.multi_var_data[0]
				.as_ref()
				.ok_or("no data")?
				.item
				.len(),
			items
		);
		assert_eq!(
			store
				.sparse_var_region_list
				.as_ref()
				.ok_or("no regions")?
				.region
				.len(),
			3
		);
	}
	Ok(())
}

#[test]
fn test_passthrough_tables() -> TestResult {
	let mut temp = TempDir::new()?;
	let fontpath = temp.compile_font(&getpath("TestTTF-Regular.ttx"), ".ttf")?;
	let mut font = open(&fontpath)?;
	let unknown = Tag::new(b"ZZZZ");
	font.insert(Box::new(DefaultTable::new(unknown, vec![0; 10])));
	std::fs::write(&fontpath, font.write(&WriteOptions::default())?)?;

	// Tables we can't subset are dropped by default.
	let subsetpath = temp.path(".ttf");
	let output = format!("--output-file={subsetpath}");
	subset_main(&[&fontpath, &output])?;
	assert!(!open(&subsetpath)?.contains(unknown));

	// Unknown tables are kept if --passthrough-tables option is passed.
	let subsetpath = temp.path(".ttf");
	let output = format!("--output-file={subsetpath}");
	subset_main(&[&fontpath, "--passthrough-tables", &output])?;
	assert!(open(&subsetpath)?.contains(unknown));
	Ok(())
}

#[test]
fn test_non_bmp_text_arg_input() -> TestResult {
	let mut font = subset_ttx(
		"TestTTF-Regular_non_BMP_char.ttx",
		".ttf",
		&["--text=A\u{1f6d2}"],
	)?;
	assert_eq!(font.require::<Maxp>()?.num_glyphs, 3);
	assert_eq!(glyph_names(&mut font)?, [".notdef", "A", "u1F6D2"]);
	Ok(())
}

#[test]
fn test_non_bmp_text_file_input() -> TestResult {
	let mut temp = TempDir::new()?;
	let text = temp.path(".txt");
	std::fs::write(&text, "A\u{1f6d2}")?;
	let argument = format!("--text-file={text}");
	let mut font = subset_ttx("TestTTF-Regular_non_BMP_char.ttx", ".ttf", &[&argument])?;
	assert_eq!(font.require::<Maxp>()?.num_glyphs, 3);
	assert_eq!(glyph_names(&mut font)?, [".notdef", "A", "u1F6D2"]);
	Ok(())
}

#[test]
fn test_no_hinting_cff() -> TestResult {
	check(
		"Lobster.subset.ttx",
		".otf",
		&["--no-hinting", "--notdef-outline", "*"],
		"expect_no_hinting_CFF.ttx",
		&["CFF "],
	)
}

#[test]
fn test_desubroutinize_cff() -> TestResult {
	check(
		"Lobster.subset.ttx",
		".otf",
		&["--desubroutinize", "--notdef-outline", "*"],
		"expect_desubroutinize_CFF.ttx",
		&["CFF "],
	)
}

#[test]
fn test_desubroutinize_hinted_subrs_cff() -> TestResult {
	check(
		"test_hinted_subrs_CFF.ttx",
		".otf",
		&["--desubroutinize", "--notdef-outline", "*"],
		"test_hinted_subrs_CFF.desub.ttx",
		&["CFF "],
	)
}

#[test]
fn test_desubroutinize_cntrmask_cff() -> TestResult {
	check(
		"test_cntrmask_CFF.ttx",
		".otf",
		&["--desubroutinize", "--notdef-outline", "*"],
		"test_cntrmask_CFF.desub.ttx",
		&["CFF "],
	)
}

#[test]
fn test_no_hinting_desubroutinize_cff() -> TestResult {
	check(
		"test_hinted_subrs_CFF.ttx",
		".otf",
		&["--no-hinting", "--desubroutinize", "--notdef-outline", "*"],
		"expect_no_hinting_desubroutinize_CFF.ttx",
		&["CFF "],
	)
}

#[test]
fn test_no_hinting_ttf() -> TestResult {
	let mut font = subset_ttx(
		"TestTTF-Regular.ttx",
		".ttf",
		&["--no-hinting", "--notdef-outline", "*"],
	)?;
	expect_ttx(
		&mut font,
		&getpath("expect_no_hinting_TTF.ttx"),
		&["glyf", "maxp"],
	)?;
	for tag in Options::default().hinting_tables {
		let Item::Str(tag) = tag else {
			continue;
		};
		let tag = Tag::from_str_padded(&tag).ok_or("a tag")?;
		assert!(!font.contains(tag));
	}
	Ok(())
}

#[test]
fn test_notdef_width_cid() -> TestResult {
	check(
		"NotdefWidthCID-Regular.ttx",
		".otf",
		&["--no-notdef-outline", "--gids=0,1"],
		"expect_notdef_width_cid.ttx",
		&["CFF "],
	)
}

fn head_bounds(font: &mut fontoxide_ttlib::Font) -> TestResult<[i16; 4]> {
	let head = font.require::<Head>()?;
	Ok([head.x_min, head.y_min, head.x_max, head.y_max])
}

fn check_recalc_bounds(ttx: &str, suffix: &str) -> TestResult {
	let mut temp = TempDir::new()?;
	let ttx = getpath(ttx);
	let bounds = head_bounds(&mut import(&ttx)?)?;
	let fontpath = temp.compile_font(&ttx, suffix)?;
	let subsetpath = temp.path(suffix);
	let output = format!("--output-file={subsetpath}");

	// By default, the subsetter does not recalculate the bounding box.
	subset_main(&[&fontpath, &output, "*"])?;
	assert_eq!(head_bounds(&mut open(&subsetpath)?)?, bounds);

	subset_main(&[&fontpath, "--recalc-bounds", &output, "*"])?;
	assert_eq!(head_bounds(&mut open(&subsetpath)?)?, [132, 304, 365, 567]);
	Ok(())
}

#[test]
fn test_recalc_bounds_ttf() -> TestResult {
	check_recalc_bounds("TestTTF-Regular.ttx", ".ttf")
}

#[test]
fn test_recalc_bounds_otf() -> TestResult {
	check_recalc_bounds("TestOTF-Regular.ttx", ".otf")
}

fn check_recalc_timestamp(ttx: &str, suffix: &str) -> TestResult {
	let mut temp = TempDir::new()?;
	let ttx = getpath(ttx);
	let modified = import(&ttx)?.require::<Head>()?.modified;
	let fontpath = temp.compile_font(&ttx, suffix)?;
	let subsetpath = temp.path(suffix);
	let output = format!("--output-file={subsetpath}");

	// By default, the subsetter does not recalculate the modified timestamp.
	subset_main(&[&fontpath, &output, "*"])?;
	assert_eq!(open(&subsetpath)?.require::<Head>()?.modified, modified);

	subset_main(&[&fontpath, "--recalc-timestamp", &output, "*"])?;
	assert!(open(&subsetpath)?.require::<Head>()?.modified > modified);
	Ok(())
}

#[test]
fn test_recalc_timestamp_ttf() -> TestResult {
	check_recalc_timestamp("TestTTF-Regular.ttx", ".ttf")
}

#[test]
fn test_recalc_timestamp_otf() -> TestResult {
	check_recalc_timestamp("TestOTF-Regular.ttx", ".otf")
}

#[test]
fn test_recalc_max_context() -> TestResult {
	let mut temp = TempDir::new()?;
	let ttx = getpath("Lobster.subset.ttx");
	let max_context = import(&ttx)?.require::<Os2>()?.us_max_context;
	let fontpath = temp.compile_font(&ttx, ".otf")?;
	let subsetpath = temp.path(".otf");
	let output = format!("--output-file={subsetpath}");

	// By default, the subsetter does not recalculate the usMaxContext.
	subset_main(&[&fontpath, "--drop-tables+=GSUB,GPOS", &output])?;
	assert_eq!(
		open(&subsetpath)?.require::<Os2>()?.us_max_context,
		max_context
	);

	subset_main(&[
		&fontpath,
		"--recalc-max-context",
		"--drop-tables+=GSUB,GPOS",
		&output,
	])?;
	assert_eq!(open(&subsetpath)?.require::<Os2>()?.us_max_context, 0);
	Ok(())
}

/// `hmtx[glyph]`.
fn metric(font: &mut fontoxide_ttlib::Font, glyph: &str) -> TestResult<(u16, i16)> {
	let gid = font.glyph_id(glyph)?;
	Ok(font.require::<Hmtx>()?.metrics[&gid])
}

fn contours(font: &mut fontoxide_ttlib::Font, glyph: &str) -> TestResult<i64> {
	let gid = font.glyph_id(glyph)?;
	Ok(font.require::<Glyf>()?.glyphs[&gid].number_of_contours)
}

#[test]
fn test_retain_gids_ttf() -> TestResult {
	let mut temp = TempDir::new()?;
	let fontpath = temp.compile_font(&getpath("TestTTF-Regular.ttx"), ".ttf")?;
	let mut font = open(&fontpath)?;
	assert_eq!(metric(&mut font, "A")?, (500, 132));
	assert_eq!(metric(&mut font, "B")?, (400, 132));
	assert!(contours(&mut font, "A")? > 0);
	assert!(contours(&mut font, "B")? > 0);

	let subsetpath = temp.path(".ttf");
	let output = format!("--output-file={subsetpath}");
	subset_main(&[&fontpath, "--retain-gids", &output, "--glyph-names", "B"])?;
	let mut subset = open(&subsetpath)?;
	assert_eq!(glyph_names(&mut subset)?, glyph_names(&mut font)?[0..3]);
	assert_eq!(metric(&mut subset, "A")?, (0, 0));
	assert_eq!(metric(&mut subset, "B")?, (400, 132));
	assert_eq!(contours(&mut subset, "A")?, 0);
	assert!(contours(&mut subset, "B")? > 0);
	Ok(())
}

#[test]
fn test_retain_gids_cff() -> TestResult {
	let mut temp = TempDir::new()?;
	let fontpath = temp.compile_font(&getpath("TestOTF-Regular.ttx"), ".otf")?;
	let mut font = open(&fontpath)?;
	assert_eq!(metric(&mut font, "A")?, (500, 132));
	assert_eq!(metric(&mut font, "B")?, (400, 132));
	assert_eq!(metric(&mut font, "C")?, (500, 0));
	for glyph in ["A", "B", "C"] {
		assert!(!charstring_program(&mut font, "CFF ", glyph)?.is_empty());
	}

	let subsetpath = temp.path(".otf");
	let output = format!("--output-file={subsetpath}");
	subset_main(&[&fontpath, "--retain-gids", &output, "--glyph-names", "B"])?;
	let mut subset = open(&subsetpath)?;
	assert_eq!(glyph_names(&mut subset)?, glyph_names(&mut font)?[0..3]);
	assert_eq!(metric(&mut subset, "A")?, (0, 0));
	assert_eq!(metric(&mut subset, "B")?, (400, 132));
	assert_eq!(charstring_program(&mut subset, "CFF ", "A")?, ["endchar"]);
	assert!(!charstring_program(&mut subset, "CFF ", "B")?.is_empty());
	Ok(())
}

#[test]
fn test_retain_gids_cff2() -> TestResult {
	let mut temp = TempDir::new()?;
	let ttx = tests_path("varLib/data/master_ttx_varfont_otf/TestCFF2VF.ttx");
	let fontpath = temp.compile_font(&ttx, ".otf")?;
	let mut font = open(&fontpath)?;
	assert_eq!(metric(&mut font, "A")?, (600, 31));
	assert_eq!(metric(&mut font, "T")?, (600, 41));
	assert!(!charstring_program(&mut font, "CFF2", "A")?.is_empty());
	assert!(!charstring_program(&mut font, "CFF2", "T")?.is_empty());

	let subsetpath = temp.path(".otf");
	let output = format!("--output-file={subsetpath}");
	subset_main(&[&fontpath, "--retain-gids", &output, "T"])?;
	let mut subset = open(&subsetpath)?;
	assert_eq!(subset.glyph_order()?.len(), 3);
	assert_eq!(metric(&mut subset, "glyph00001")?, (0, 0));
	assert_eq!(metric(&mut subset, "T")?, (600, 41));
	assert!(charstring_program(&mut subset, "CFF2", "glyph00001")?.is_empty());
	assert!(!charstring_program(&mut subset, "CFF2", "T")?.is_empty());
	Ok(())
}

#[test]
fn test_hvar_vvar() -> TestResult {
	check(
		"TestHVVAR.ttx",
		".ttf",
		&["--text=BD"],
		"expect_HVVAR.ttx",
		&["GlyphOrder", "HVAR", "VVAR", "avar", "fvar"],
	)
}

#[test]
fn test_hvar_vvar_retain_gids() -> TestResult {
	check(
		"TestHVVAR.ttx",
		".ttf",
		&["--text=BD", "--retain-gids"],
		"expect_HVVAR_retain_gids.ttx",
		&["GlyphOrder", "HVAR", "VVAR", "avar", "fvar"],
	)
}

#[test]
fn test_subset_flavor_woff_woff2_none() -> TestResult {
	for (flavor, suffix, expected) in [
		(Some("--flavor=woff"), ".woff", Some(Flavor::Woff)),
		(Some("--flavor=woff2"), ".woff2", Some(Flavor::Woff2)),
		(None, ".ttf", None),
	] {
		let mut args = vec!["*"];
		args.extend(flavor);
		let mut temp = TempDir::new()?;
		let fontpath = temp.compile_font(&getpath("TestTTF-Regular.ttx"), ".ttf")?;
		let path = temp.path(suffix);
		let output = format!("--output-file={path}");
		let mut argv = vec![fontpath.as_str()];
		argv.extend(args);
		argv.push(&output);
		subset_main(&argv)?;
		assert_eq!(open(&path)?.flavor, expected);
	}
	Ok(())
}

/// A font saved with a flavor.
fn flavored(temp: &mut TempDir, ttx: &str, suffix: &str, flavor: Flavor) -> TestResult<String> {
	let path = temp.compile_font(&getpath(ttx), suffix)?;
	let mut font = open(&path)?;
	font.flavor = Some(flavor);
	let input = temp.path(&format!(".{}", flavor.as_str()));
	std::fs::write(&input, font.write(&WriteOptions::default())?)?;
	Ok(input)
}

fn with_new_extension(path: &str, extension: &str) -> String {
	let stem = path.rsplit_once('.').map_or(path, |(stem, _)| stem);
	format!("{stem}{extension}")
}

#[test]
fn test_subset_inherits_flavor() -> TestResult {
	for flavor in [Flavor::Woff, Flavor::Woff2] {
		let mut temp = TempDir::new()?;
		let input = flavored(&mut temp, "TestTTF-Regular.ttx", ".ttf", flavor)?;
		subset_main(&[&input, "*"])?;
		let output = with_new_extension(&input, &format!(".subset.{}", flavor.as_str()));
		assert_eq!(open(&output)?.flavor, Some(flavor));
	}
	Ok(())
}

#[test]
fn test_subset_flavor_none_from_woff() -> TestResult {
	for flavor in [Flavor::Woff, Flavor::Woff2] {
		for token in ["none", "None", "NONE", ""] {
			let mut temp = TempDir::new()?;
			let input = flavored(&mut temp, "TestTTF-Regular.ttx", ".ttf", flavor)?;
			let option = format!("--flavor={token}");
			subset_main(&[&input, "*", &option])?;
			let output = with_new_extension(&input, ".subset.ttf");
			assert_eq!(open(&output)?.flavor, None);
		}
	}
	Ok(())
}

#[test]
fn test_subset_flavor_none_from_cff_woff_uses_otf_extension() -> TestResult {
	let mut temp = TempDir::new()?;
	let input = flavored(&mut temp, "TestOTF-Regular.ttx", ".otf", Flavor::Woff)?;
	subset_main(&[&input, "*", "--flavor=none"])?;
	let out = open(with_new_extension(&input, ".subset.otf"))?;
	assert_eq!(out.flavor, None);
	assert_eq!(out.sfnt_version, Tag::new(b"OTTO"));
	Ok(())
}

#[test]
fn test_subset_flavor_invalid() -> TestResult {
	let mut temp = TempDir::new()?;
	let fontpath = temp.compile_font(&getpath("TestTTF-Regular.ttx"), ".ttf")?;
	assert_eq!(
		fontoxide_subset::main(&[&fontpath, "*", "--flavor=ttf"])?,
		2
	);
	Ok(())
}

#[test]
fn test_subset_context_subst_format_3() -> TestResult {
	// All glyphs are kept via the GSUB closure; no changes are expected.
	let ttx = getpath("TestContextSubstFormat3.ttx");
	let mut font = subset_ttx("TestContextSubstFormat3.ttx", ".ttf", &["--unicodes=*"])?;
	expect_ttx(&mut font, &ttx, &[])
}

#[test]
fn test_cmap_prune_format12() -> TestResult {
	check(
		"CmapSubsetTest.ttx",
		".ttf",
		&["--glyphs=a"],
		"CmapSubsetTest.subset.ttx",
		&["cmap"],
	)
}

#[test]
fn test_cmap_format14() -> TestResult {
	for (unicodes, expected) in [
		("--unicodes=4e05", "cmap14_font1.no_uvs.ttx"),
		("--unicodes=4e05,e0100", "cmap14_font1.uvs.ttx"),
		("--unicodes=4e10", "cmap14_font1.no_uvs_non_default.ttx"),
		("--unicodes=4e10,e0100", "cmap14_font1.uvs_non_default.ttx"),
	] {
		check("cmap14_font1.ttx", ".otf", &[unicodes], expected, &["cmap"])?;
	}
	Ok(())
}

#[test]
fn test_gpos_pair_pos_format2_use_class0() -> TestResult {
	for (text, n) in [("--text='!'", 1), ("--text='#'", 2)] {
		check(
			"GPOS_PairPos_Format2_PR_2221.ttx",
			".ttf",
			&[text, "--layout-features+=test"],
			&format!("GPOS_PairPos_Format2_ClassDef{n}_useClass0.subset.ttx"),
			&["GPOS"],
		)?;
	}
	Ok(())
}

#[test]
fn test_gpos_single_pos_prune_post_subset_no_value() -> TestResult {
	check(
		"GPOS_SinglePos_no_value_issue_2312.ttx",
		".ttf",
		&["*", "--glyph-names"],
		"GPOS_SinglePos_no_value_issue_2312.subset.ttx",
		&["GlyphOrder", "GPOS"],
	)
}

/// `test_harfbuzz_repacker`, without what mocks `uharfbuzz` or looks at the log:
/// both packers compile to the same TTX.
#[test]
fn test_harfbuzz_repacker() -> TestResult {
	for option in [
		None,
		Some("--harfbuzz-repacker"),
		Some("--no-harfbuzz-repacker"),
	] {
		let mut args = vec!["--unicodes=0x53a9", "--layout-features=*"];
		args.extend(option);
		check(
			"harfbuzz_repacker.ttx",
			".otf",
			&args,
			"expect_harfbuzz_repacker.ttx",
			&["GSUB"],
		)?;
	}
	Ok(())
}

#[test]
fn test_retain_east_asian_spacing_features() -> TestResult {
	let ttx = getpath("NotoSansCJKjp-Regular.subset.ttx");
	let text = std::fs::read_to_string(&ttx)?;
	assert!(text.contains("FeatureTag value=\"halt\""));
	assert!(text.contains("FeatureTag value=\"vhal\""));
	// The subset output is the same as the input.
	let mut font = subset_ttx(
		"NotoSansCJKjp-Regular.subset.ttx",
		".otf",
		&["--unicodes=*"],
	)?;
	expect_ttx(&mut font, &ttx, &[])
}

/// The `ttf_path` fixture: `TestTTF-Regular.ttx` of `Tests/ttLib/data`.
fn ttf_path(temp: &mut TempDir) -> TestResult<String> {
	let mut font = import_recalculating(&tests_path("ttLib/data/TestTTF-Regular.ttx"))?;
	let path = temp.path(".ttf");
	std::fs::write(&path, font.write(&WriteOptions::default())?)?;
	Ok(path)
}

fn import_recalculating(ttx: &str) -> TestResult<fontoxide_ttlib::Font> {
	let mut font = fontoxide_ttlib::Font::new();
	font.import_ttx(&std::fs::read_to_string(ttx)?)?;
	Ok(font)
}

#[test]
fn test_subset_empty_glyf() -> TestResult {
	let mut temp = TempDir::new()?;
	let path = ttf_path(&mut temp)?;
	let subsetpath = temp.path(".subset");
	let output = format!("--output-file={subsetpath}");
	// Only keep the empty .notdef and space glyphs, resulting in an empty glyf.
	subset_main(&[
		&path,
		"--no-notdef-outline",
		"--glyph-names",
		&output,
		"--glyphs=.notdef space",
	])?;
	let mut font = open(&subsetpath)?;
	assert_eq!(glyph_names(&mut font)?, [".notdef", "space"]);
	assert_eq!(font.table_data(Tag::new(b"glyf"))?, [0]);
	let glyf = font.require::<Glyf>()?;
	assert!(glyf.glyphs.values().all(|g| g.number_of_contours == 0));
	let loca = font.require::<Loca>()?;
	assert!(loca.locations.iter().flatten().all(|&l| l == 0));
	Ok(())
}

#[test]
fn test_subset_colr_glyph_closure() -> TestResult {
	let mut temp = TempDir::new()?;
	let mut font = import_recalculating(&getpath("BungeeColor-Regular.ttx"))?;
	let path = temp.path(".ttf");
	std::fs::write(&path, font.write(&WriteOptions::default())?)?;
	let subsetpath = temp.path(".subset.ttf");
	let output = format!("--output-file={subsetpath}");
	subset_main(&[&path, "--glyph-names", &output, "--glyphs=Agrave"])?;
	let mut subset = open(&subsetpath)?;
	assert_eq!(
		glyph_names(&mut subset)?,
		[
			".notdef",
			"A",
			"grave",
			"Agrave",
			".notdef.alt001",
			".notdef.alt002",
			"A.alt002",
			"Agrave.alt001",
			"Agrave.alt002",
			"grave.alt002",
		]
	);
	let notdef = subset.glyph_id(".notdef")?;
	let agrave = subset.glyph_id("Agrave")?;
	let grave = subset.glyph_id("grave")?;
	let colr = subset.require::<Colr>()?;
	let layers = colr.color_layers.as_ref().ok_or("no ColorLayers")?;
	assert!(layers.contains_key(&notdef));
	assert!(layers.contains_key(&agrave));
	// Agrave's glyf uses grave: kept in glyf, but not in COLR.
	assert!(!layers.contains_key(&grave));
	Ok(())
}

#[test]
fn test_subset_recalc_x_avg_char_width() -> TestResult {
	let mut temp = TempDir::new()?;
	let path = ttf_path(&mut temp)?;
	let before = open(&path)?.require::<Os2>()?.x_avg_char_width;
	let subsetpath = temp.path(".subset.ttf");
	let output = format!("--output-file={subsetpath}");
	subset_main(&[
		&path,
		&output,
		// Only the ellipsis, which is very wide, bumps up the average.
		"--glyphs=ellipsis",
		"--recalc-average-width",
		"--no-prune-unicode-ranges",
	])?;
	let mut subset = open(&subsetpath)?;
	let after = subset.require::<Os2>()?.x_avg_char_width;
	assert_ne!(after, before);
	// The value is the actual new value.
	let widths: Vec<u16> = subset
		.require::<Hmtx>()?
		.metrics
		.values()
		.map(|m| m.0)
		.collect();
	let os2 = subset.require::<Os2>()?;
	assert_eq!(os2.recalc_avg_char_width(widths), after);
	Ok(())
}

/// Adds names with `addName`.
fn add_names(font: &mut fontoxide_ttlib::Font, names: &[&str]) -> TestResult<Vec<u16>> {
	let name = font.require::<Name>()?;
	names
		.iter()
		.map(|n| Ok(name.add_name_default(n)?))
		.collect()
}

fn name_ids(font: &mut fontoxide_ttlib::Font) -> TestResult<std::collections::BTreeSet<u16>> {
	Ok(font
		.require::<Name>()?
		.names
		.iter()
		.map(|n| n.name_id)
		.collect())
}

fn keep_all_names_options() -> Options {
	Options {
		name_ids: vec![Item::from("*")],
		name_legacy: true,
		name_languages: vec![Item::from("*")],
		..Default::default()
	}
}

fn best_cmap_unicodes(font: &mut fontoxide_ttlib::Font) -> TestResult<Vec<u32>> {
	let cmap = font.require::<fontoxide_ttlib::tables::Cmap>()?;
	Ok(cmap
		.get_best_cmap(&fontoxide_ttlib::tables::cmap::DEFAULT_CMAP_PREFERENCES)
		.ok_or("no best cmap")?
		.keys()
		.copied()
		.collect())
}

#[test]
fn test_prune_user_name_ids_with_keep_all() -> TestResult {
	let mut temp = TempDir::new()?;
	let mut font = open(ttf_path(&mut temp)?)?;
	let keep = name_ids(&mut font)?;
	let names: Vec<String> = (0..10).map(|i| format!("Test{i}")).collect();
	add_names(
		&mut font,
		&names.iter().map(String::as_str).collect::<Vec<_>>(),
	)?;
	let mut subsetter = Subsetter::new(keep_all_names_options());
	subsetter.populate(&[], &[], &best_cmap_unicodes(&mut font)?, "");
	subsetter.subset(&mut font)?;
	let ids = name_ids(&mut font)?;
	assert!(ids.iter().all(|&n| n <= 255));
	assert_eq!(ids, keep);
	Ok(())
}

#[test]
fn test_prune_unused_user_name_ids_with_keep_all() -> TestResult {
	let mut temp = TempDir::new()?;
	let mut font = open(ttf_path(&mut temp)?)?;
	let mut keep = name_ids(&mut font)?;
	let names: Vec<String> = (0..10).map(|i| format!("Test{i}")).collect();
	add_names(
		&mut font,
		&names.iter().map(String::as_str).collect::<Vec<_>>(),
	)?;
	let id = add_names(&mut font, &["Test STAT"])?[0];
	keep.insert(id);
	font.insert(Box::new(fontoxide_ttlib::tables::Stat {
		table: fontoxide_ttlib::ot::types::STAT {
			elided_fallback_name_id: id,
			..Default::default()
		},
	}));
	let mut subsetter = Subsetter::new(keep_all_names_options());
	subsetter.populate(&[], &[], &best_cmap_unicodes(&mut font)?, "");
	subsetter.subset(&mut font)?;
	assert_eq!(name_ids(&mut font)?, keep);
	Ok(())
}

#[test]
fn test_cv_xx_feature_params_name_ids_are_retained() -> TestResult {
	let mut font = import_recalculating(&getpath("Andika-Regular.subset.ttx"))?;
	let keep = name_ids(&mut font)?;
	let mut options = Options {
		glyph_names: true,
		..Default::default()
	};
	options.layout_features.push(Item::from("cv43"));
	let names = glyph_names(&mut font)?;
	let mut subsetter = Subsetter::new(options);
	subsetter.populate(
		&names.iter().map(String::as_str).collect::<Vec<_>>(),
		&[],
		&[],
		"",
	);
	subsetter.subset(&mut font)?;
	assert_eq!(name_ids(&mut font)?, keep);
	Ok(())
}

#[test]
fn test_preserve_name_ids_when_used_elsewhere() -> TestResult {
	let mut font = import_recalculating(&getpath("PreserveSillyNamesTest.ttx"))?;
	let names = glyph_names(&mut font)?;
	let mut subsetter = Subsetter::new(Options {
		obfuscate_names: true,
		..Default::default()
	});
	subsetter.populate(
		&names.iter().map(String::as_str).collect::<Vec<_>>(),
		&[],
		&[],
		"",
	);
	subsetter.subset(&mut font)?;
	// All names of STAT and fvar use an ID from 256 up, except for 5, which is not
	// obfuscated.
	let ok = |id: u16| id >= 256 || id == 5;
	let stat = font.require::<fontoxide_ttlib::tables::Stat>()?;
	assert!(ok(stat.elided_fallback_name_id));
	for axis in &stat.design_axis_record.as_ref().ok_or("no axes")?.axis {
		assert!(ok(axis.axis_name_id));
	}
	let fvar = font.require::<fontoxide_ttlib::tables::Fvar>()?;
	for axis in &fvar.axes {
		assert!(ok(axis.axis_name_id));
	}
	for instance in &fvar.instances {
		assert!(ok(instance.subfamily_name_id));
	}
	Ok(())
}
