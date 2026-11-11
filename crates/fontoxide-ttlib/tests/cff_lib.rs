//! `Tests/cffLib/cffLib_test.py`: the tests of `CffLibTest`. `CFFToCFF2Test`
//! and `CFF2ToCFFTest` are in `cff_to_cff2.rs` and `cff2_to_cff.rs`.

mod common;

use common::{TestResult, upstream};
use fontoxide_pens::bounds_pen::BoundsPen;
use fontoxide_ttlib::cff::charstring::T2CharString;
use fontoxide_ttlib::cff::compile::pack_fd_select_ranges;
use fontoxide_ttlib::cff::dict::Value;
use fontoxide_ttlib::cff::font::{
	CffFontSet, CharStrings, Encoding, PrivateDict, PrivateRef, TopDict, read_fd_select,
	recalc_font_bbox,
};
use fontoxide_ttlib::tables::hhea::{Hhea, Vhea};
use fontoxide_ttlib::tables::otl::Varc;
use fontoxide_ttlib::tables::{Cff, Cff2, Head};
use fontoxide_ttlib::{Font, GlyphSet, ReadOptions, Tag, TtxOptions, WriteOptions};

fn data(name: &str) -> std::path::PathBuf {
	upstream("cffLib/data").join(name)
}

fn read(path: &std::path::Path, options: ReadOptions) -> fontoxide_ttlib::Result<Font> {
	Font::read(std::fs::read(path)?, options)
}

fn no_recalc() -> ReadOptions {
	ReadOptions {
		recalc_bboxes: false,
		recalc_timestamp: false,
		..ReadOptions::default()
	}
}

/// `font.save(BytesIO())`, then `TTFont(BytesIO(data))`.
fn save_and_load(font: &mut Font, options: ReadOptions) -> fontoxide_ttlib::Result<Font> {
	let data = font.write(&WriteOptions::default())?;
	Font::read(data, options)
}

/// `TTFont(recalcBBoxes=False, recalcTimestamp=False).importXML(path)`.
fn import(path: &std::path::Path) -> fontoxide_ttlib::Result<Font> {
	let mut font = Font::new();
	font.recalc_bboxes = false;
	font.recalc_timestamp = false;
	font.import_ttx_file(path)?;
	Ok(font)
}

fn top_cff2(font: &mut Font) -> fontoxide_ttlib::Result<&mut TopDict> {
	let cff2 = font.require::<Cff2>()?;
	cff2.cff
		.top_dicts
		.first_mut()
		.ok_or_else(|| fontoxide_ttlib::Error::ttlib("no Top DICT"))
}

fn top_cff(font: &mut Font) -> fontoxide_ttlib::Result<&mut TopDict> {
	let cff = font.require::<Cff>()?;
	cff.cff
		.top_dicts
		.first_mut()
		.ok_or_else(|| fontoxide_ttlib::Error::ttlib("no Top DICT"))
}

/// `check` of `_check_VARC_short_CharStrings_roundtrip`.
fn check_varc_short(font: &mut Font) -> TestResult {
	let order = font.glyph_order()?.names().to_vec();
	{
		let top = top_cff2(font)?;
		let cs = top.char_strings.as_ref().ok_or("no CharStrings")?;
		assert!(cs.names.len() < order.len());
		assert_eq!(top.charset.as_deref(), Some(&order[..cs.names.len()]));
		let fd_select = top.fd_select.as_ref().ok_or("no FDSelect")?;
		assert_eq!(fd_select.gid_array.len(), order.len());
		assert_eq!(fd_select.gid_array.last(), Some(&Some(1)));
	}
	let composite = {
		let gid = font
			.require::<Varc>()?
			.coverage
			.as_ref()
			.ok_or("no Coverage")?
			.glyphs[0];
		font.glyph_name(gid)?
	};
	{
		let glyph_set = GlyphSet::new(font)?;
		assert_eq!(glyph_set.keys(), order);
		assert!(glyph_set.contains(&composite));
		let mut pen = BoundsPen::new_bounds(Some(&glyph_set));
		glyph_set.draw(&composite, &mut pen)?;
		assert_eq!(pen.bounds(), Some([600.0, 0.0, 800.0, 200.0]));
		assert_eq!(glyph_set.metrics(&composite)?.width, 500.0);
	}
	assert_eq!(font.require::<Head>()?.x_max, 800);
	assert_eq!(font.require::<Head>()?.y_max, 200);
	assert_eq!(font.require::<Hhea>()?.x_max_extent, 200);
	assert_eq!(font.require::<Vhea>()?.y_max_extent, 200);
	Ok(())
}

/// `test_VARC_short_CharStrings_roundtrip`, without the final subsetting step
/// (fontTools' `subset` is ported in another crate).
#[test]
fn test_varc_short_char_strings_roundtrip() -> TestResult {
	for format in [3, 4] {
		let mut font = read(&data("varc-short-cff2.otf"), ReadOptions::default())?;
		top_cff2(&mut font)?
			.fd_select
			.as_mut()
			.ok_or("no FDSelect")?
			.format = Some(format);
		for _ in 0..2 {
			check_varc_short(&mut font)?;
			font = save_and_load(&mut font, ReadOptions::default())?;
		}
		let ttx = font.to_ttx(&TtxOptions::default())?;
		let mut font = Font::new();
		font.import_ttx(&ttx)?;
		let mut font = save_and_load(&mut font, ReadOptions::default())?;
		check_varc_short(&mut font)?;
	}
	Ok(())
}

#[test]
fn test_cff2_count_mismatch_without_varc() -> TestResult {
	let mut font = read(&data("varc-short-cff2.otf"), ReadOptions::default())?;
	top_cff2(&mut font)?;
	assert!(font.remove(Tag::new(b"VARC")));
	let err = font
		.write(&WriteOptions::default())
		.err()
		.ok_or("no error")?;
	assert!(err.to_string().contains("CharStrings count"), "{err}");
	Ok(())
}

#[test]
fn test_cff2_char_strings_must_be_prefix() -> TestResult {
	let mut font = read(&data("varc-short-cff2.otf"), ReadOptions::default())?;
	{
		let cs = top_cff2(&mut font)?
			.char_strings
			.as_mut()
			.ok_or("no CharStrings")?;
		// `strings.charStrings["composite"] = strings.charStrings.pop("unused")`
		let i = cs.index("unused").ok_or("no 'unused'")?;
		cs.names.remove(i);
		let item = cs.items.remove(i);
		let private = cs.privates.remove(i);
		cs.set("composite".into(), item, private);
	}
	let err = font
		.write(&WriteOptions::default())
		.err()
		.ok_or("no error")?;
	assert!(err.to_string().contains("glyph-order prefix"), "{err}");
	Ok(())
}

#[test]
fn test_fd_select_varc_sentinel_bounds() -> TestResult {
	for format in [3u8, 4] {
		for sentinel in [2usize, 3, 4, 5] {
			let data = pack_fd_select_ranges(&vec![Some(0); sentinel], format)?;
			let select = read_fd_select(&data, 0, 3, 4);
			if sentinel == 3 || sentinel == 4 {
				assert_eq!(select?.gid_array.len(), sentinel);
			} else {
				let err = select.err().ok_or("no error")?;
				assert!(err.to_string().contains("sentinel"), "{err}");
			}
		}
	}
	Ok(())
}

/// A Top DICT with the charstrings of `CharStrings.fromXML`.
fn top_dict_with(char_strings: &[(&str, &str)]) -> fontoxide_ttlib::Result<TopDict> {
	let mut cs = CharStrings::default();
	for (name, program) in char_strings {
		let e = fontoxide_misc::xml_reader::parse(&format!(
			"<CharString name=\"{name}\">{program}</CharString>"
		))
		.map_err(|e| fontoxide_ttlib::Error::value(e.to_string()))?;
		cs.set(
			(*name).to_owned(),
			T2CharString::from_xml(&e)?,
			PrivateRef::Top,
		);
	}
	Ok(TopDict {
		char_strings: Some(cs),
		private: Some(Box::new(PrivateDict::default())),
		..TopDict::default()
	})
}

fn ints(v: &[i64]) -> Value {
	Value::List(v.iter().map(|&i| Value::Int(i)).collect())
}

#[test]
fn test_top_dict_recalc_font_bbox() -> TestResult {
	let mut top = top_dict_with(&[
		(".notdef", "endchar"),
		// [100, -100, 300, 100]
		(
			"foo",
			"100 -100 rmoveto 200 hlineto 200 vlineto -200 hlineto endchar",
		),
		// [0, 0, 200, 200]
		(
			"bar",
			"0 0 rmoveto 200 hlineto 200 vlineto -200 hlineto endchar",
		),
		// [-55.1, -55.1, 55.1, 55.1]
		(
			"baz",
			"-55.1 -55.1 rmoveto 110.2 hlineto 110.2 vlineto -110.2 hlineto endchar",
		),
	])?;
	recalc_font_bbox(&mut top, &mut [])?;
	assert_eq!(
		top.values.get("FontBBox"),
		Some(&ints(&[-56, -100, 300, 200]))
	);
	Ok(())
}

#[test]
fn test_top_dict_recalc_font_bbox_empty() -> TestResult {
	let mut top = top_dict_with(&[(".notdef", "endchar"), ("space", "123 endchar")])?;
	recalc_font_bbox(&mut top, &mut [])?;
	assert_eq!(top.values.get("FontBBox"), Some(&ints(&[0, 0, 0, 0])));
	Ok(())
}

#[test]
fn test_top_dict_set_encoding() -> TestResult {
	let mut font = import(&data("TestOTF.ttx"))?;
	let mut encoding = vec![".notdef".to_owned(); 256];
	encoding[0x20] = "space".into();
	top_cff(&mut font)?.encoding = Some(Encoding::Custom(encoding));
	let mut font2 = save_and_load(&mut font, ReadOptions::default())?;
	match &top_cff(&mut font2)?.encoding {
		Some(Encoding::Custom(e)) => assert_eq!(e[32], "space"),
		other => panic!("{other:?}"),
	}
	Ok(())
}

#[test]
fn test_top_dict_delattr() -> TestResult {
	let mut font = read(&data("LinLibertine_RBI.otf"), no_recalc())?;
	{
		let top = top_cff(&mut font)?;
		assert!(top.values.remove("Notice").is_some());
		assert!(!top.values.contains_key("Notice"));
		assert_eq!(
			top.values.get("FullName"),
			Some(&Value::Str("Linux Libertine O Bold Italic".into()))
		);
		assert!(top.values.remove("FullName").is_some());
		assert!(!top.values.contains_key("FullName"));
		// Deleting again is an error (`AttributeError`).
		assert!(top.values.remove("Notice").is_none());
		let private = top.private.take();
		assert!(top.private.is_none());
		top.private = private;
	}
	let mut font2 = save_and_load(&mut font, ReadOptions::default())?;
	let top2 = top_cff(&mut font2)?;
	assert!(!top2.values.contains_key("Notice"));
	assert!(!top2.values.contains_key("FullName"));
	assert!(top2.values.contains_key("FamilyName"));
	assert!(top2.private.is_some());
	Ok(())
}

/// `test_CFF_deepcopy`: copying a font with a CFF table (a `Clone` here).
#[test]
fn test_cff_deepcopy() -> TestResult {
	let font = import(&upstream(
		"varLib/data/master_ttx_interpolatable_otf/TestFamily2-Master0.ttx",
	))?;
	let _copy = font.clone();
	Ok(())
}

#[test]
fn test_cff2_var_store_recompiled_after_mutation() -> TestResult {
	let mut font = import(&data("TestSparseCFF2VF.ttx"))?;
	font.write(&WriteOptions::default())?;
	{
		let store = top_cff2(&mut font)?
			.var_store
			.as_mut()
			.ok_or("no VarStore")?;
		let regions = store.var_region_list.as_mut().ok_or("no VarRegionList")?;
		regions.region[0].var_region_axis[0].peak_coord = 0.5;
	}
	let mut font2 = save_and_load(&mut font, ReadOptions::default())?;
	let store2 = top_cff2(&mut font2)?
		.var_store
		.as_ref()
		.ok_or("no VarStore")?;
	let regions2 = store2.var_region_list.as_ref().ok_or("no VarRegionList")?;
	assert_eq!(regions2.region[0].var_region_axis[0].peak_coord, 0.5);
	Ok(())
}

#[test]
fn test_fd_select_format_4() -> TestResult {
	let mut font = import(&data("TestFDSelect4.ttx"))?;
	let mut font2 = save_and_load(&mut font, ReadOptions::default())?;
	let fd_select = top_cff2(&mut font2)?
		.fd_select
		.clone()
		.ok_or("no FDSelect")?;
	assert_eq!(fd_select.format, Some(4));
	assert_eq!(fd_select.gid_array, [Some(0), Some(0), Some(1)]);
	Ok(())
}

fn assert_unique(order: &[String]) {
	let set: std::collections::HashSet<&String> = order.iter().collect();
	assert_eq!(set.len(), order.len());
}

#[test]
fn test_unique_glyph_names() -> TestResult {
	let mut font = read(&data("LinLibertine_RBI.otf"), no_recalc())?;
	assert_unique(font.glyph_order()?.names());
	let mut font2 = save_and_load(&mut font, ReadOptions::default())?;
	assert_unique(font2.glyph_order()?.names());
	Ok(())
}

#[test]
fn test_reading_supplement_encoding() -> TestResult {
	let cff = CffFontSet::decompile(
		&std::fs::read(data("TestSupplementEncoding.cff"))?,
		false,
		None,
	)?;
	match &cff.top_dicts[0].encoding {
		Some(Encoding::Custom(e)) => {
			assert_eq!(e[9], "space");
			assert_eq!(e[32], "space");
		}
		other => panic!("{other:?}"),
	}
	Ok(())
}

#[test]
fn test_non_ascii_font_name_round_trip() -> TestResult {
	let mut font = read(&data("LinLibertine_RBI.otf"), ReadOptions::default())?;
	font.require::<Cff>()?.cff.font_names = vec!["Caf\u{e9}Sans".into()];
	let mut font2 = save_and_load(&mut font, ReadOptions::default())?;
	assert_eq!(font2.require::<Cff>()?.cff.font_names, ["Caf\u{e9}Sans"]);
	Ok(())
}
